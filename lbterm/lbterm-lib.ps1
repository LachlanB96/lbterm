#Requires -Version 7
<#
What lbterm.ps1 does to files, kept apart from it so lbterm\tests\lbterm.Tests.ps1 can try each
piece on folders of its own. Dot-sourced. Nothing here runs cargo or knows where the real build,
install, cache or logs are: every path comes in as a parameter.
#>

# The files a build is: what the check looks for and install copies
$LbtermExes = 'lbterm.exe', 'lbterm-gui.exe', 'lbterm-mux-server.exe', 'strip-ansi-escapes.exe'
# Copied next to the exes by wezterm-gui's build.rs; upstream's ci/deploy.sh ships the same set.
$LbtermExtras = 'conpty.dll', 'OpenConsole.exe', 'libEGL.dll', 'libGLESv2.dll', 'mesa\opengl32.dll'

# The openssl-src version Cargo.lock pins ('300.6.1+3.6.3'), or nothing
function Get-OpensslSrcVersion([Parameter(Mandatory)][string]$LockFile) {
  $m = [regex]::Match([IO.File]::ReadAllText($LockFile), '(?m)^name = "openssl-src"\r?\nversion = "([^"]+)"')
  if ($m.Success) { $m.Groups[1].Value }
}

# Whether $Dir holds a built OpenSSL from openssl-src $SrcVersion: its headers, saying the
# OpenSSL version after the '+', and the two static libraries openssl-sys links
function Test-OpensslBuild([Parameter(Mandatory)][string]$Dir, [Parameter(Mandatory)][string]$SrcVersion) {
  $header = Join-Path $Dir 'include\openssl\opensslv.h'
  foreach ($f in $header, (Join-Path $Dir 'lib\libssl.lib'), (Join-Path $Dir 'lib\libcrypto.lib')) {
    if (-not (Test-Path $f)) { return $false }
  }
  $openssl = ($SrcVersion -split '\+', 2)[1]
  [bool]$openssl -and [IO.File]::ReadAllText($header) -match "define OPENSSL_VERSION_STR `"$([regex]::Escape($openssl))`""
}

# Keeps the OpenSSL openssl-src built under $BuildDir (a target's release\build) in $Cache, if
# one of $SrcVersion is there and $Cache doesn't have it yet. Whether $Cache has it afterwards.
function Save-OpensslCache {
  param(
    [Parameter(Mandatory)][string]$BuildDir,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Cache,
    [Parameter(Mandatory)][string]$SrcVersion
  )
  if (Test-OpensslBuild $Cache $SrcVersion) { return $true }
  $built = Get-ChildItem $BuildDir -Directory -Filter 'openssl-sys-*' -ErrorAction SilentlyContinue |
    ForEach-Object { Join-Path $_.FullName 'out\openssl-build\install' } |
    Where-Object { Test-OpensslBuild $_ $SrcVersion } |
    Select-Object -First 1
  if (-not $built) { return $false }
  # Copied beside it and renamed into place, so a build that stops halfway never leaves a cache
  # that looks whole
  $partial = "$Cache.partial"
  if (Test-Path $partial) { Remove-Item $partial -Recurse -Force }
  New-Item -ItemType Directory -Force $partial | Out-Null
  Copy-Item (Join-Path $built 'include'), (Join-Path $built 'lib') $partial -Recurse
  if (Test-Path $Cache) { Remove-Item $Cache -Recurse -Force }
  Move-Item $partial $Cache
  $true
}

# ---- Cargo only inside a job, where a PC asks for it (lbterm.ps1 build, check, test, unify, try) ----
# This script starts no job of its own: cargo runs in whatever job it's in, and one release rustc
# can take more cores than -j says. A PC whose heavy work goes through a queue that holds it to its
# CPUs says so in its dev settings (~\.config\lbterm\dev.json, which never leaves it), "job":
#   { "env": "WARDEN_JOB",
#     "run": "warden-run --name lbterm-{command} --cpus 4 --mem 8 --max 2h -- pwsh -NoProfile -File {script} {command}{args}",
#     "adopt": ["warden-run", "adopt", "{pid}"] }
# `env` is the variable the queue gives every process it starts; `run` the line to start this
# script in it instead; `adopt` the command that hands the job a test window started outside it
# (motion\start-unfocused.ps1's -Job). Without "job", nothing is asked.

# The PC's dev settings, or nothing without the file: LBTERM_DEV's file (a test's), else the PC's
function Get-DevSettings([string]$File = ($env:LBTERM_DEV ? $env:LBTERM_DEV : (Join-Path $HOME '.config\lbterm\dev.json'))) {
  if (-not (Test-Path -LiteralPath $File)) { return $null }
  Get-Content -Raw -LiteralPath $File | ConvertFrom-Json
}

# Whether this process is in a Windows job object
function Test-InJobObject {
  if (-not ('LbtermJobObject' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class LbtermJobObject {
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
  public static bool Here() {
    bool inJob;
    return IsProcessInJob(System.Diagnostics.Process.GetCurrentProcess().Handle, IntPtr.Zero, out inJob) && inJob;
  }
}
'@
  }
  [LbtermJobObject]::Here()
}

# The command line's parameters after the command, as they'd be typed again: ' -Package a,b -NoSccache'
function Format-LbtermArguments([System.Collections.IDictionary]$Bound) {
  $out = foreach ($k in $Bound.Keys) {
    if ($k -eq 'Command') { continue }
    $v = $Bound[$k]
    if ($v -is [switch]) { if ($v.IsPresent) { "-$k" } }
    elseif ($v -is [array]) { "-$k $($v -join ',')" }
    else { "-$k $v" }
  }
  -join ($out | ForEach-Object { " $_" })
}

# Why `lbterm.ps1 $Command` may not run cargo here, or nothing when it may: with the PC's "job"
# setting, it runs only where its variable ($Value) is set and the process is in a job object ($InJob):
# a process that left the job (a child started outside it) keeps the variable but not the job
function Get-JobRefusal {
  param(
    $Settings,
    [Parameter(Mandatory)][string]$Command,
    [Parameter(Mandatory)][string]$Script,
    [string]$Arguments = '',
    [string]$Value,
    [bool]$InJob
  )
  $job = $Settings.job
  if (-not $job -or -not $job.env) { return $null }
  if ($Value -and $InJob) { return $null }
  $why = if (-not $Value) { "$($job.env) isn't set" } else { "$($job.env) is set but this process isn't in a job" }
  $line = ([string]$job.run).Replace('{command}', $Command).Replace('{script}', $Script.Replace('\', '/')).Replace('{args}', $Arguments)
  # A test-window run is a GPU job that only draws (--gpu-draw, since 2026-11-07: LBTerm has no
  # compute pipeline and puts no model on the GPU, so it runs beside Earshot's listener), so the
  # GPU's other rules hold for it (the warden's GPU jobs at once, a game open, the GPU's
  # temperature); a measuring run (budgets) also takes the lock lbterm-measure, so no other LBTerm
  # measurement runs beside it and skews its numbers (the warden runs up to 3 GPU jobs at once
  # since 2026-11-04). e2e checks behaviour, not time: no lock
  if ($Command -in 'e2e', 'budgets', 'look') {
    $flags = if ($Command -eq 'budgets') { '--gpu-draw --lock lbterm-measure ' } else { '--gpu-draw ' }
    $line = $line -replace '^(\S*warden-run(\.exe)?)\s', ('$1 ' + $flags)
  }
  $what = if ($Command -in 'e2e', 'budgets', 'look') {
    'opens test windows, which this PC runs only inside a job, so the warden queues them while the PC is hot'
  } else { 'runs cargo, which this PC runs only inside a job that holds it to its CPUs' }
  "lbterm.ps1 $Command $what ($why). Start it in one:`n  $line"
}

# ---- Test-window runs named, and only while the displays are awake (lbterm.ps1 e2e, budgets) ----

# Why `lbterm.ps1 e2e` or `budgets` won't start as asked, or nothing (Lachlan's 10a and 11a of
# 2026-11-09). A whole set needs -AllScripts: the sets of 80 to 150 scripts the tracks of 2026-11-08 and 09
# ran took two to three hours each, read as a fail on one script that fails on main's build too, and
# told nothing a run of the named scripts wouldn't. And none while the displays sleep ($Asleep,
# harness.psm1's Get-DisplaysAsleep, after Request-DisplaysAwake's hold has had its chance): DWM then plays no motion and blocks the draw step, so most
# scripts fail on every build (PITFALLS 239)
function Get-RunRefusal {
  param([Parameter(Mandatory)][string]$Command, [string[]]$Only, [switch]$Smoke, [switch]$AllScripts, [string]$Against, [string]$Asleep)
  if ($Command -eq 'look') {
    # A look is one capture before and after a change: named scripts, and the build before
    if (-not @($Only | Where-Object { $_ }).Count) { return 'lbterm.ps1 look shows named scripts only: -Only the scripts whose captures show the look that changed' }
    if (-not $Against) { return 'lbterm.ps1 look needs the build before the change: -Against <dir> (a copy of the installed build, or main''s)' }
  } elseif (-not @($Only | Where-Object { $_ }).Count -and -not $AllScripts -and -not ($Smoke -and $Command -eq 'e2e')) {
    $what = if ($Command -eq 'e2e') { 'scripts with -Only, or run -Smoke (the smoke set and the scripts the branch changed, a branch''s last stage; e2e -Changed lists the ones whose Covers lines match it)' } else { 'scenarios with -Only' }
    return "lbterm.ps1 $Command runs no whole set: name the $what. A failure is run again by name, beside main's build on that one alone. Every one needs -AllScripts, only when Lachlan asks for it (his 11a of 2026-11-09)"
  }
  if ($Asleep) { return "lbterm.ps1 $Command waits for the displays: $Asleep. Nothing it measures or captures can be judged until they wake (PITFALLS 239)" }
}

# ---- sccache (lbterm.ps1's Invoke-Cargo, unless -NoSccache) ----

# Where sccache.exe is, or nothing: on PATH, from cargo install, or from winget, which doesn't put
# its portable packages on PATH in every shell (Claude's never had it): its Links folder, then the
# newest of its package folders, Packages\Mozilla.sccache_<source>\sccache-v<version>-<target>.
function Find-Sccache {
  param(
    [string[]]$PathDirs = @($env:PATH -split ';'),
    [string]$CargoHome = $(if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }),
    [string[]]$WingetRoots = @((Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet'), (Join-Path $env:ProgramFiles 'WinGet'))
  )
  $candidates = @($PathDirs) + (Join-Path $CargoHome 'bin') + @($WingetRoots | ForEach-Object { Join-Path $_ 'Links' })
  foreach ($dir in $candidates) {
    if (-not $dir.Trim()) { continue }
    # [IO.Path] rather than Join-Path, which throws on a PATH entry naming a drive that isn't there
    $exe = try { [IO.Path]::Combine($dir.Trim(), 'sccache.exe') } catch { continue }
    if (Test-Path -LiteralPath $exe -PathType Leaf) { return [IO.Path]::GetFullPath($exe) }
  }
  $packages = foreach ($root in $WingetRoots) {
    Get-ChildItem -LiteralPath (Join-Path $root 'Packages') -Directory -Filter 'Mozilla.sccache_*' -ErrorAction SilentlyContinue |
      ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Directory -Filter 'sccache-v*' -ErrorAction SilentlyContinue } |
      ForEach-Object {
        $exe = Join-Path $_.FullName 'sccache.exe'
        $version = $null
        if ($_.Name -match '^sccache-v(\d+(?:\.\d+){1,3})' -and [version]::TryParse($Matches[1], [ref]$version) -and
          (Test-Path -LiteralPath $exe -PathType Leaf)) {
          [pscustomobject]@{ Exe = $exe; Version = $version }
        }
      }
  }
  $packages | Sort-Object Version -Descending | Select-Object -First 1 -ExpandProperty Exe
}

# Whether anything accepts a connection on 127.0.0.1:$Port, where sccache's server listens
function Test-SccacheServer([int]$Port = 4226, [int]$TimeoutMs = 500) {
  $client = [Net.Sockets.TcpClient]::new()
  try { $client.ConnectAsync('127.0.0.1', $Port).Wait($TimeoutMs) -and $client.Connected }
  catch { $false }
  finally { $client.Dispose() }
}

# The environment sccache's server starts with ($Server) and cargo runs its clients with:
#   SCCACHE_IDLE_TIMEOUT  how long the server waits with nothing to do before it exits
#   SCCACHE_CLIENT_SIDE   each compile runs in its own sccache client, under cargo, so inside the
#                     warden's job, on its CPUs and paused with it; the server, started outside
#                     the job, only reads and writes the cache. In server mode it would run them.
#   SCCACHE_IGNORE_SERVER_IO_ERROR  a client that loses the server compiles anyway, uncached
# The server gets SCCACHE_START_SERVER and SCCACHE_NO_DAEMON instead of the client's two, which
# make sccache.exe the server itself rather than a client starting one.
function Get-SccacheEnvironment {
  param(
    [int]$IdleSeconds = 3600,
    [switch]$Server
  )
  $vars = [ordered]@{ SCCACHE_IDLE_TIMEOUT = "$IdleSeconds" }
  if ($Server) {
    $vars.SCCACHE_START_SERVER = '1'
    $vars.SCCACHE_NO_DAEMON = '1'
  } else {
    $vars.SCCACHE_CLIENT_SIDE = '1'
    $vars.SCCACHE_IGNORE_SERVER_IO_ERROR = '1'
  }
  $vars
}

# ---- One set of features for the build and the check's tests (lbterm-unify) ----

# What the build asks cargo for: the four exes
$LbtermBuildPackages = 'wezterm', 'wezterm-gui', 'wezterm-mux-server', 'strip-ansi-escapes'
# The crates whose tests the check runs: config/src/lbterm.rs and wezterm/tests/lbterm.rs, plus
# the rest of those two crates' tests, lbterm-motion (springs, curves, DComp cubics, the pane
# layout planner) and lbterm-panel (the tab panel's rows, hover timing and layout), the parser and
# terminal, which hold OSC 7501 (program_status.rs in each), mux (localpane.rs: process lists
# refreshed off the GUI thread), lbterm-extras (the smaller additions' logic: taskbar progress,
# scrollbar marks and hiding, the slow-frame log, the Conductor's seat and view), lbterm-triggers (rules, colours, what's new, the
# hearts), lbterm-notify (OSC 99), codec (OSC 7501 records crossing from the mux server, a PDU zstd
# can't compress going uncompressed), promise (a task named by where it was spawned,
# tests\lbterm_spawn_site.rs), wezterm-client (a unix domain reconnecting when its server hangs
# up, client.rs), mux-lua (Lua's spawn_tab, spawn_window and split starting in the folder
# they're given) and wezterm-font (lbterm.rs: fonts sharing what their fallback searches found,
# a reload of the same settings keeping them, the bundled fonts parsed once), lfucache (the
# line caches' counts, and an entry just used kept while an older one can go) and wezterm-gui
# (since feat/dcomp-recorder: the compositor's tests over a stand-in DirectComposition device,
# compositor/recorder.rs, and the quads' hashes, the menus' fades, the new-tab pad and the line
# keys, which ran only by hand before; lbterm/COMPOSITOR.md, "Checking it without a window", has
# what it adds to the check)
$LbtermTestPackages = 'config', 'wezterm', 'lbterm-motion', 'lbterm-panel', 'wezterm-escape-parser', 'wezterm-term',
  'mux', 'lbterm-extras', 'lbterm-triggers', 'lbterm-notify', 'codec', 'promise', 'wezterm-client', 'mux-lua',
  'wezterm-mux-server-impl', 'wezterm-font', 'lfucache', 'wezterm-gui'
# The crate every cargo run of lbterm.ps1 selects as well, so they all agree on features
$LbtermUnify = 'lbterm-unify'

# Cargo works out a library's features from the packages a command names (and, for cargo test,
# their dev-dependencies), and a library built with other features is another artifact, as is
# everything above it. So the check's cargo test, naming other packages than the build, compiled
# about 150 crates a second time (2026-10-24: 6 m 49 s after a 12 m 19 s build): upstream's test
# libraries k9 and criterion turn on serde's alloc and windows-sys 0.59's Win32_System_Console,
# and without wezterm-gui named, what it turns on in smallvec, winapi, bytemuck, ahash and the rest
# was missing. lbterm-unify depends on each library that differs, with every feature any of
# lbterm.ps1's selections turns on, and every cargo run names it too (the trick cargo-hakari plays,
# without a line in each upstream Cargo.toml). `lbterm.ps1 unify` writes its Cargo.toml.

# cargo tree's lines in `--prefix none -f "{p}|{f}"` form, as a map of "<name> v<version>" to its
# name, version, source (registry, path or git), path and the union of its nodes' features.
function ConvertFrom-CargoTree([AllowEmptyCollection()][string[]]$Lines) {
  $map = [ordered]@{}
  foreach ($line in $Lines) {
    $l = ($line -replace ' \(\*\)\s*$', '').Trim()
    if (-not $l -or $l -notmatch '\|') { continue }
    $p, $f = $l -split '\|', 2
    $p = $p -replace ' \(proc-macro\)', ''
    if ($p -notmatch '^(?<name>\S+) v(?<ver>\S+)(?: \((?<src>.+)\))?$') { continue }
    $key = "$($Matches.name) v$($Matches.ver)"
    $src = $Matches.src
    if (-not $map.Contains($key)) {
      $map[$key] = [pscustomobject]@{
        Name     = $Matches.name
        Version  = $Matches.ver
        Source   = if (-not $src) { 'registry' } elseif ($src -match '^[a-z+]+://') { 'git' } else { 'path' }
        Path     = if ($src -and $src -notmatch '^[a-z+]+://') { $src }
        Features = [Collections.Generic.SortedSet[string]]::new([StringComparer]::Ordinal)
        Variants = [Collections.Generic.HashSet[string]]::new()
      }
    }
    $feats = @($f -split ',' | Where-Object { $_ })
    foreach ($x in $feats) { [void]$map[$key].Features.Add($x) }
    [void]$map[$key].Variants.Add(($feats | Sort-Object) -join ',')
  }
  $map
}

# The features a host (build scripts, proc macros and what they use) and a target (everything
# else) resolve gives each library, from two cargo trees: $Target is the tree without proc macros
# and build-dependencies, $Full the whole of it. Host is what the full tree has besides the target
# side's variant, per library.
function Split-CargoFeatures($Target, $Full) {
  $hostSide = [ordered]@{}
  foreach ($key in $Full.Keys) {
    $variants = @($Full[$key].Variants | Where-Object { -not ($Target.Contains($key) -and $Target[$key].Variants.Contains($_)) })
    if (-not $variants) { continue }
    $p = $Full[$key]
    $set = [Collections.Generic.SortedSet[string]]::new([StringComparer]::Ordinal)
    foreach ($v in $variants) { foreach ($x in $v -split ',' | Where-Object { $_ }) { [void]$set.Add($x) } }
    $hostSide[$key] = [pscustomobject]@{ Name = $p.Name; Version = $p.Version; Source = $p.Source; Path = $p.Path; Features = $set }
  }
  [pscustomobject]@{ Target = $Target; Host = $hostSide }
}

# The libraries that more than one of $Selections (a map of a selection's name to its
# Split-CargoFeatures) resolve with different features, on the same side: each with the union of
# its features across them, its side and the selections that differ.
function Compare-CargoFeatures($Selections) {
  foreach ($side in 'Target', 'Host') {
    $seen = [ordered]@{}
    foreach ($sel in $Selections.Keys) {
      $map = $Selections[$sel].$side
      foreach ($key in $map.Keys) {
        if (-not $seen.Contains($key)) { $seen[$key] = [Collections.Generic.List[object]]::new() }
        $seen[$key].Add([pscustomobject]@{ Selection = $sel; Package = $map[$key] })
      }
    }
    foreach ($key in $seen.Keys) {
      $in = $seen[$key]
      if ($in.Count -lt 2) { continue }
      $sets = @($in | ForEach-Object { @($_.Package.Features) -join ',' } | Sort-Object -Unique)
      if ($sets.Count -lt 2) { continue }
      $union = [Collections.Generic.SortedSet[string]]::new([StringComparer]::Ordinal)
      foreach ($i in $in) { foreach ($x in $i.Package.Features) { [void]$union.Add($x) } }
      $p = $in[0].Package
      [pscustomobject]@{
        Key = $key; Side = $side; Name = $p.Name; Version = $p.Version; Source = $p.Source; Path = $p.Path
        Features = @($union)
        Missing = @($in | Where-Object { @($_.Package.Features).Count -lt $union.Count } | ForEach-Object Selection)
      }
    }
  }
}

# lbterm-unify's Cargo.toml: a dependency on each of $Pins (Compare-CargoFeatures' libraries,
# merged across rounds) with exactly its features, target-side ones as [dependencies] and host-side
# ones as [build-dependencies]. $Dir is the crate's folder, which path dependencies are named from.
function New-UnifyManifest {
  param(
    [AllowEmptyCollection()][object[]]$Pins = @(),
    [Parameter(Mandatory)][string]$Dir
  )
  $lines = [Collections.Generic.List[string]]::new()
  foreach ($l in @(
      '[package]'
      'name = "lbterm-unify"'
      'version = "0.1.0"'
      'edition = "2021"'
      'description = "Never built into anything: every cargo run of lbterm.ps1 names it, so the build and the check''s tests resolve the same features and share their artifacts"'
      'license = "MIT"'
      'publish = false'
      ''
      '[lib]'
      'path = "src/lib.rs"'
      'test = false'
      'doctest = false'
      ''
      '# Written by `lbterm.ps1 unify` (lbterm-lib.ps1, "One set of features"): every library that two of'
      '# lbterm.ps1''s cargo runs would build with different features, with all of them. Do not edit by hand.')) { $lines.Add($l) }
  foreach ($side in 'Target', 'Host') {
    # (not $pins: PowerShell's names ignore case, so that would be -Pins)
    $these = @($Pins | Where-Object Side -eq $side | Sort-Object Name, Version)
    if (-not $these -and $side -eq 'Host') { continue }
    $lines.Add('')
    $lines.Add($(if ($side -eq 'Target') { '[dependencies]' } else { '[build-dependencies]' }))
    $names = @($these | Group-Object Name | Where-Object Count -gt 1 | ForEach-Object Name)
    foreach ($p in $these) {
      $key = if ($names -contains $p.Name) { "$($p.Name)-$($p.Version -replace '[^0-9A-Za-z]', '_')" } else { $p.Name }
      $where = switch ($p.Source) {
        'registry' { "version = `"$($p.Version)`"" }
        'path' { "path = `"$([IO.Path]::GetRelativePath($Dir, $p.Path) -replace '\\', '/')`"" }
        default { throw "$($p.Name) $($p.Version) comes from git, which lbterm.ps1 unify doesn't pin: pin it by hand" }
      }
      $pkg = if ($key -ne $p.Name) { "package = `"$($p.Name)`", " } else { '' }
      $feats = (@($p.Features) | ForEach-Object { "`"$_`"" }) -join ', '
      $lines.Add("$key = { $pkg$where, default-features = false, features = [$feats] }")
    }
  }
  ($lines -join "`n") + "`n"
}

# What every cargo run's features depend on, as one hash: Cargo.lock, every Cargo.toml in the
# tree, the selections and cargo's version. The check compares the features again only when it
# changes (Assert-Unified in lbterm.ps1).
function Get-UnifyKey {
  param(
    [Parameter(Mandatory)][string]$Repo,
    [string]$Cargo = ''
  )
  $sha = [Security.Cryptography.SHA256]::Create()
  $parts = [Collections.Generic.List[string]]::new()
  $parts.Add($Cargo)
  $parts.Add(($LbtermBuildPackages + '|' + $LbtermTestPackages + '|' + $LbtermUnify) -join ',')
  # A pathspec's * crosses folders, so this is every Cargo.toml, committed or not
  $files = @(git -C $Repo ls-files --cached --others --exclude-standard -- 'Cargo.lock' '*Cargo.toml' | Sort-Object -Unique)
  foreach ($f in $files) {
    $path = Join-Path $Repo $f
    if (Test-Path -LiteralPath $path) { $parts.Add("$f`n$([IO.File]::ReadAllText($path))") }
  }
  $bytes = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($parts -join "`n`0"))
  ([BitConverter]::ToString($bytes) -replace '-', '').Substring(0, 16).ToLowerInvariant()
}

# ---- Pester beside cargo (lbterm.ps1 check) ----

# Starts lbterm\tests' Pester run in a pwsh of its own, its output going to $Log, so the check
# can build while it runs (it needs no build, and spends most of its 100 s waiting on git and
# bash). Wait-PesterRun prints the log and gives its exit code.
function Start-PesterRun {
  param(
    [Parameter(Mandatory)][string]$Tests,
    [Parameter(Mandatory)][string]$Log,
    [string]$Pwsh = 'C:\Program Files\PowerShell\7\pwsh.exe',
    # Test files under $Tests to leave out
    [string[]]$Exclude = @()
  )
  New-Item -ItemType Directory -Force (Split-Path $Log) | Out-Null
  if (Test-Path -LiteralPath $Log) { Remove-Item -LiteralPath $Log -Force }
  $q = { param($s) "'" + ($s -replace "'", "''") + "'" }
  # Run.Exit: Pester's own exit code, non-zero when a test fails (*> takes every stream, its result
  # object too). A Pester that won't load stops the script, so pwsh exits 1 rather than 0.
  $cmd = @(
    "`$ErrorActionPreference = 'Stop'"
    'Import-Module Pester -MinimumVersion 5.0'
    '$c = New-PesterConfiguration'
    "`$c.Run.Path = $(& $q $Tests)"
    if ($Exclude) { "`$c.Run.ExcludePath = @($(($Exclude | ForEach-Object { & $q $_ }) -join ', '))" }
    '$c.Run.Exit = $true'
    "`$c.Output.Verbosity = 'Normal'"
    "Invoke-Pester -Configuration `$c *> $(& $q $Log)"
  ) -join "`n"
  $psi = [Diagnostics.ProcessStartInfo]::new($Pwsh)
  foreach ($a in '-NoProfile', '-NonInteractive', '-Command', $cmd) { $psi.ArgumentList.Add($a) }
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  [pscustomobject]@{ Process = [Diagnostics.Process]::Start($psi); Log = $Log; Started = Get-Date }
}

# Waits for Start-PesterRun's run, prints its log (not with -Quiet), and gives its exit code
function Wait-PesterRun($Run, [switch]$Quiet) {
  $Run.Process.WaitForExit()
  if (-not $Quiet -and (Test-Path -LiteralPath $Run.Log)) { Get-Content -LiteralPath $Run.Log | Out-Host }
  $Run.Process.ExitCode
}

# Ends Start-PesterRun's run if it's still going (the check failed before it was waited for)
function Stop-PesterRun($Run) {
  if ($Run -and -not $Run.Process.HasExited) { try { $Run.Process.Kill($true) } catch {} }
}

# ---- Build slots (lbterm.ps1 slot) ----

# A worktree built from nothing takes 11 to 12 minutes, so a branch starts in a slot instead: one of
# a few worktrees, .claude\worktrees\slot-<n>, that are never removed. Each is locked with this
# reason, so land keeps it, build and all, and moves it to main with nothing checked out (Backup's
# land, a reason starting "slot"); the next branch made there rebuilds only what's changed.
# Lachlan's pick of four on 2026-10-24, six since 2026-10-31 ("we can probably allow 6 slots")
$LbtermSlotCount = 6
$LbtermSlotReason = 'slot: an LBTerm build slot (lbterm.ps1 slot); land keeps it and its build, moved to main'

# git with its output and exit code, never on the console
function Invoke-LbtermGit([string]$Dir, [string[]]$Arg) {
  $out = & git -C $Dir @Arg 2>&1
  [pscustomobject]@{ Code = $LASTEXITCODE; Lines = @($out | ForEach-Object { "$_" } | Where-Object { $_ }) }
}

# `git worktree list --porcelain`, the main checkout first
function Get-GitWorktrees([Parameter(Mandatory)][string]$Repo) {
  $list = [Collections.Generic.List[object]]::new()
  foreach ($l in (Invoke-LbtermGit $Repo @('worktree', 'list', '--porcelain')).Lines) {
    if ($l.StartsWith('worktree ')) {
      $list.Add([pscustomobject]@{ Path = [IO.Path]::GetFullPath($l.Substring(9)); Head = $null; Branch = $null; Locked = $false; LockReason = '' })
    } elseif ($list.Count -and $l.StartsWith('HEAD ')) { $list[-1].Head = $l.Substring(5) }
    elseif ($list.Count -and $l.StartsWith('branch refs/heads/')) { $list[-1].Branch = $l.Substring(18) }
    elseif ($list.Count -and $l -match '^locked\b ?(.*)$') { $list[-1].Locked = $true; $list[-1].LockReason = $Matches[1] }
  }
  $list
}

# The slots of the repo whose main checkout is $Main, made or not: slot-1 to slot-$Count
function Get-LbtermSlots {
  param(
    [Parameter(Mandatory)][string]$Main,
    [int]$Count = $LbtermSlotCount,
    [string]$Base = 'main'
  )
  $trees = @(Get-GitWorktrees $Main)
  foreach ($n in 1..$Count) {
    $path = [IO.Path]::GetFullPath((Join-Path $Main ".claude\worktrees\slot-$n"))
    $t = $trees | Where-Object { $_.Path -eq $path } | Select-Object -First 1
    $isSlot = [bool]$t -and $t.Locked -and $t.LockReason -match '^slot\b'
    $behind = if ($t -and $t.Head) { [int](Invoke-LbtermGit $Main @('rev-list', '--count', "$($t.Head)..$Base")).Lines[0] }
    [pscustomobject]@{
      Name = "slot-$n"; Path = $path; Exists = [bool]$t; IsSlot = $isSlot
      Branch = $t.Branch; Head = $t.Head; Behind = $behind
      Free = $isSlot -and -not $t.Branch
    }
  }
}

# Holds $Slot for a moment, so two sessions taking one at once can't both have it: a file in its
# own git folder, made only if it isn't there (or is older than $Minutes: a claim that died).
function Lock-LbtermSlot($Slot, [int]$Minutes = 10) {
  $gitDir = (Invoke-LbtermGit $Slot.Path @('rev-parse', '--absolute-git-dir')).Lines[0]
  if (-not $gitDir) { return $null }
  $file = Join-Path $gitDir 'lbterm-slot-claim'
  for ($try = 0; $try -lt 2; $try++) {
    try {
      $s = [IO.File]::Open($file, 'CreateNew', 'Write', 'None')
      $w = [IO.StreamWriter]::new($s); $w.Write("pid $PID at $(Get-Date -Format o)"); $w.Dispose()
      return $file
    } catch [IO.IOException] {
      $age = (Get-Date) - [IO.File]::GetLastWriteTime($file)
      if ($try -or $age.TotalMinutes -lt $Minutes) { return $null }
      Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue
    }
  }
}

# A slot for $Branch in the repo whose main checkout is $Main: a free one (the closest to $Base,
# so the least to rebuild), else a new one while there are fewer than $Count and the disk has
# $MinFreeGB to spare. The branch is made from $Base there, or checked out if it's there already.
# A result with Path when it has one; with Busy (the slots and their branches) or Refused when not.
function Request-LbtermSlot {
  param(
    [Parameter(Mandatory)][string]$Main,
    [Parameter(Mandatory)][string]$Branch,
    [string]$Base = 'main',
    [int]$Count = $LbtermSlotCount,
    [double]$MinFreeGB = 20,
    [double]$FreeGB = -1
  )
  $trees = @(Get-GitWorktrees $Main)
  $has = $trees | Where-Object Branch -eq $Branch | Select-Object -First 1
  if ($has) { throw "$Branch is checked out already, in $($has.Path)" }
  $exists = -not (Invoke-LbtermGit $Main @('rev-parse', '--verify', '--quiet', "refs/heads/$Branch")).Code
  $switch = if ($exists) { @('switch', '--quiet', $Branch) } else { @('switch', '--quiet', '-c', $Branch, $Base) }
  $slots = @(Get-LbtermSlots -Main $Main -Count $Count -Base $Base)

  foreach ($s in $slots | Where-Object Free | Sort-Object Behind) {
    $claim = Lock-LbtermSlot $s
    if (-not $claim) { continue }
    try {
      $again = Get-GitWorktrees $Main | Where-Object { $_.Path -eq $s.Path } | Select-Object -First 1
      if (-not $again -or $again.Branch) { continue }
      if ((Invoke-LbtermGit $s.Path @('status', '--porcelain')).Lines.Count) { continue }
      $r = Invoke-LbtermGit $s.Path $switch
      if ($r.Code) { throw "git switch in $($s.Path) failed: $($r.Lines -join ' ')" }
      return [pscustomobject]@{ Path = $s.Path; Name = $s.Name; New = $false; Behind = $s.Behind }
    } finally { Remove-Item -LiteralPath $claim -Force -ErrorAction SilentlyContinue }
  }

  $missing = $slots | Where-Object { -not $_.Exists } | Select-Object -First 1
  if ($missing) {
    if ($FreeGB -lt 0) { $FreeGB = (Get-PSDrive ([IO.Path]::GetPathRoot($Main).Substring(0, 1))).Free / 1GB }
    if ($FreeGB -lt $MinFreeGB) {
      return [pscustomobject]@{ Refused = "a new slot would need about 9 to 13 GB once built, and $('{0:N1}' -f $FreeGB) GB is free (under the $MinFreeGB GB a new slot asks for)"; Slots = $slots }
    }
    # Made with the branch out, so no other session can take it as free before it's ours
    $add = if ($exists) { @('worktree', 'add', '--quiet', $missing.Path, $Branch) } else { @('worktree', 'add', '--quiet', '-b', $Branch, $missing.Path, $Base) }
    $r = Invoke-LbtermGit $Main $add
    if (-not $r.Code) {
      $l = Invoke-LbtermGit $Main @('worktree', 'lock', '--reason', $LbtermSlotReason, $missing.Path)
      if ($l.Code) { throw "made $($missing.Path) but couldn't lock it as a slot: $($l.Lines -join ' ')" }
      return [pscustomobject]@{ Path = $missing.Path; Name = $missing.Name; New = $true; Behind = 0 }
    }
    # another session made it in between: it's that one's
  }
  [pscustomobject]@{ Busy = @($slots | Where-Object Exists); Slots = $slots }
}

# Puts slot $Name back without landing its branch (given up, or carried on elsewhere): it moves to
# $Base with nothing checked out, and the branch stays. Refuses uncommitted work.
function Clear-LbtermSlot {
  param(
    [Parameter(Mandatory)][string]$Main,
    [Parameter(Mandatory)][string]$Name,
    [string]$Base = 'main'
  )
  $s = Get-LbtermSlots -Main $Main -Count 99 -Base $Base | Where-Object { $_.Name -eq $Name -or $_.Path -eq [IO.Path]::GetFullPath($Name) } | Select-Object -First 1
  if (-not $s -or -not $s.IsSlot) { throw "$Name isn't a slot" }
  if (-not $s.Branch) { return "$($s.Name) is free already" }
  $dirty = (Invoke-LbtermGit $s.Path @('status', '--porcelain')).Lines
  if ($dirty.Count) { throw "$($s.Name) has $($dirty.Count) uncommitted change(s) on $($s.Branch): commit them or throw them away first" }
  $r = Invoke-LbtermGit $s.Path @('switch', '--quiet', '--detach', $Base)
  if ($r.Code) { throw "git switch --detach $Base in $($s.Path) failed: $($r.Lines -join ' ')" }
  $ahead = (Invoke-LbtermGit $Main @('rev-list', '--count', "$Base..$($s.Branch)")).Lines[0]
  "$($s.Name) is free, on $Base; $($s.Branch) stays, with $ahead commit(s) $Base doesn't have"
}

# ---- What builds and test runs leave behind (every build and e2e run, and lbterm.ps1 prune) ----

# A tree's target\ grows as it's used, and cargo prunes none of it (.claude/docs/building.md, "A
# slot's target"): rustc keeps an incremental folder for each crate and fingerprint, and a new
# fingerprint (main merged in, unify, the tests' build beside the release one) leaves the old
# folder where it was; and e2e keeps every run's output, captures included. With six slots at 9 to
# 22 GB each, C: fell under the warden's 20 GB floor and every job on the PC was refused, on
# 2026-11-04 and again on 2026-11-08 (Root's InMail, Lachlan's 1b: LBTerm keeps its slots small).

# The incremental folders in $Dir (a tree's target\release\incremental, <crate>-<hash> each) no
# build has written in $IdleDays days and that are more than $MarginDays older than their crate's
# newest there. So a tree keeps each crate's last folder (the release build's and the tests' both,
# built within a day of each other), and the next build makes one that went again, for that crate
# only. rustc makes a session folder in a crate's folder each time it compiles it, so one in use
# was written minutes ago.
function Get-StaleIncremental {
  param(
    [Parameter(Mandatory)][string]$Dir,
    [datetime]$Now = (Get-Date),
    [double]$IdleDays = 2,
    [double]$MarginDays = 1
  )
  if (-not (Test-Path -LiteralPath $Dir)) { return }
  $crates = Get-ChildItem -LiteralPath $Dir -Directory | Group-Object { $_.Name -replace '-[^-]+$', '' }
  foreach ($c in $crates) {
    $newest = ($c.Group | Measure-Object LastWriteTime -Maximum).Maximum
    $c.Group | Where-Object { $_.LastWriteTime -lt $Now.AddDays(-$IdleDays) -and $_.LastWriteTime -lt $newest.AddDays(-$MarginDays) }
  }
}

# The e2e runs in $Dir (a tree's target\e2e, each a folder named by when it started) last written
# more than $KeepDays ago, never the newest: a run's captures and logs are read while its branch is
# worked on, and a branch that lands takes no run with it
function Get-StaleRuns {
  param(
    [Parameter(Mandatory)][string]$Dir,
    [datetime]$Now = (Get-Date),
    [double]$KeepDays = 3
  )
  if (-not (Test-Path -LiteralPath $Dir)) { return }
  $runs = @(Get-ChildItem -LiteralPath $Dir -Directory | Where-Object Name -match '^\d{8}-\d{6}' | Sort-Object LastWriteTime)
  if ($runs.Count -lt 2) { return }
  $runs[0..($runs.Count - 2)] | Where-Object { $_.LastWriteTime -lt $Now.AddDays(-$KeepDays) }
}

# Deletes from $Tree (the main checkout or a worktree) what Get-StaleIncremental (incremental)
# and Get-StaleRuns (runs) pick. What went, its size, and any folder that couldn't go (a file in
# it held open), which stays for the next time.
function Remove-StaleBuildFiles {
  param(
    [Parameter(Mandatory)][string]$Tree,
    [ValidateSet('incremental', 'runs')][string[]]$What = @('incremental', 'runs'),
    [datetime]$Now = (Get-Date)
  )
  $stale = @(
    if ('incremental' -in $What) { Get-StaleIncremental -Dir (Join-Path $Tree 'target\release\incremental') -Now $Now }
    if ('runs' -in $What) {
      Get-StaleRuns -Dir (Join-Path $Tree 'target\e2e') -Now $Now
      Get-StaleRuns -Dir (Join-Path $Tree 'target\look') -Now $Now
    }
  )
  $bytes = 0L
  $gone = 0
  $kept = [Collections.Generic.List[string]]::new()
  foreach ($d in $stale) {
    $size = [long](Get-ChildItem -LiteralPath $d.FullName -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
    try {
      Remove-Item -LiteralPath $d.FullName -Recurse -Force -ErrorAction Stop
      $bytes += $size
      $gone++
    } catch { $kept.Add("$($d.FullName): $($_.Exception.Message)") }
  }
  [pscustomobject]@{ Tree = $Tree; Folders = $gone; Bytes = $bytes; Kept = @($kept) }
}

# One line for what Remove-StaleBuildFiles did in a tree, or nothing when it did nothing
function Format-StaleBuildFiles($Result, [string]$What) {
  $lines = @()
  if ($Result.Folders) { $lines += "pruned $($Result.Folders) $What folder(s) ($('{0:N1}' -f ($Result.Bytes / 1GB)) GB) from $($Result.Tree)" }
  foreach ($k in $Result.Kept) { $lines += "couldn't prune $k" }
  $lines
}

# Copies a build's files from $From to $To, version.txt too when $From has one. A running LBTerm
# holds its files open, so they can't be overwritten, but Windows lets them be renamed: each one
# in use goes aside as <name>.<stamp>.old and the new one takes its place. How many went aside.
function Copy-Build {
  param(
    [Parameter(Mandatory)][string]$From,
    [Parameter(Mandatory)][string]$To,
    [string]$Stamp = (Get-Date -Format 'yyyyMMddHHmmss')
  )
  $moved = 0
  foreach ($f in $LbtermExes + $LbtermExtras + 'version.txt') {
    $src = Join-Path $From $f
    if (-not (Test-Path $src)) {
      if ($f -eq 'version.txt') { continue }
      throw "$src is missing"
    }
    $dest = Join-Path $To $f
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    try { Copy-Item $src $dest -Force -ErrorAction Stop }
    catch [IO.IOException] {
      Move-Item $dest "$dest.$Stamp.old"
      Copy-Item $src $dest
      $moved++
    }
  }
  $moved
}

# Deletes what earlier copies renamed aside in $Dir, where nothing holds it any more
function Remove-OldBuilds([Parameter(Mandatory)][string]$Dir) {
  Get-ChildItem $Dir -Recurse -Filter '*.old' -ErrorAction SilentlyContinue |
    ForEach-Object { try { Remove-Item $_.FullName -ErrorAction Stop } catch {} }
}

# The build in $Dir, as LBTerm's version string ('20261020-194410-04e8e722', what wezterm.version
# says in Lua): its version.txt, or what its lbterm.exe says. Nothing when neither is there.
function Get-BuildVersion([Parameter(Mandatory)][string]$Dir) {
  $file = Join-Path $Dir 'version.txt'
  if (Test-Path $file) { return [IO.File]::ReadAllText($file).Trim() }
  $exe = Join-Path $Dir 'lbterm.exe'
  if (Test-Path $exe) { ((& $exe --version) -replace '^lbterm\s+', '').Trim() }
}

function Set-BuildVersion([Parameter(Mandatory)][string]$Dir, [Parameter(Mandatory)][string]$Version) {
  [IO.File]::WriteAllText((Join-Path $Dir 'version.txt'), $Version)
}

# Whether a build's version string names commit $Commit, its short hash as wezterm-version asks
# git for it (core.abbrev=8). A build of HEAD should; one stamped at an older commit and never
# stamped again would let install take a new build for the one already installed.
function Test-VersionNamesCommit([string]$Version, [Parameter(Mandatory)][string]$Commit) {
  [bool]$Version -and $Version.EndsWith("-$Commit")
}

# Keeps the build installed in $Install as $Previous before $NewVersion replaces it, so rollback
# has something to go back to. Not when it's the same build: installing one twice mustn't
# overwrite the build to go back to with itself. Whether it kept one.
function Save-Previous {
  param(
    [Parameter(Mandatory)][string]$Install,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Previous,
    [Parameter(Mandatory)][string]$NewVersion
  )
  if (-not (Test-Path (Join-Path $Install 'lbterm-gui.exe'))) { return $false }
  $current = Get-BuildVersion $Install
  if (-not $current -or $current -eq $NewVersion) { return $false }
  # Copied beside it and renamed into place, so a copy that stops halfway never replaces a whole one
  $partial = "$Previous.partial"
  if (Test-Path $partial) { Remove-Item $partial -Recurse -Force }
  Copy-Build -From $Install -To $partial | Out-Null
  Set-BuildVersion $partial $current
  if (Test-Path $Previous) { Remove-Item $Previous -Recurse -Force }
  Move-Item $partial $Previous
  $true
}

# Swaps the builds in $Install and $Previous, so a second rollback goes forward again. How many
# of the installed files were in use and went aside.
function Switch-Build {
  param(
    [Parameter(Mandatory)][string]$Install,
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$Previous
  )
  if (-not (Test-Path (Join-Path $Previous 'lbterm-gui.exe'))) { throw "No build kept in $Previous to go back to" }
  $swap = "$Previous.swap"
  if (Test-Path $swap) { Remove-Item $swap -Recurse -Force }
  Copy-Build -From $Install -To $swap | Out-Null
  Set-BuildVersion $swap (Get-BuildVersion $Install)
  $moved = Copy-Build -From $Previous -To $Install
  Remove-Item $Previous -Recurse -Force
  Move-Item $swap $Previous
  $moved
}

# The window class a try window runs under: its own, so it never hands its window to the daily
# LBTerm (or the daily one to it), and so Terminal's AHK hotkeys leave it out
$LbtermTryClass = 'com.lachlanb.lbterm.try'

# The config a try window runs (lbterm.ps1 try): Lachlan's own, $UserConfig (~\.lbterm.lua, as
# LBTerm loads it), on a mux domain of its own so it never attaches to the daily server's tabs,
# without the summon key (the daily LBTerm holds it) and without crash restore (a try window is
# thrown away). $Self is where this text goes: the server the window starts reads it too.
function New-TryConfig {
  param(
    [Parameter(Mandatory)][string]$UserConfig,
    [Parameter(Mandatory)][string]$Self,
    [Parameter(Mandatory)][string]$Socket,
    [Parameter(Mandatory)][string]$Server
  )
  $lua = { param($p) $p -replace '\\', '/' }
  @"
-- Written by lbterm.ps1 try: Lachlan's config, run by a build of the LBTerm tree in a window
-- and on a mux server of its own. Rewritten by every try.
local config = dofile('$(& $lua $UserConfig)')
config.unix_domains = { {
  name = 'try',
  socket_path = '$(& $lua $Socket)',
  serve_command = { '$(& $lua $Server)', '--config-file', '$(& $lua $Self)', '--daemonize' },
} }
config.default_gui_startup_args = { 'connect', 'try' }
config.lbterm_summon = nil
config.lbterm_crash_restore = false
return config
"@
}

# ---- Logs (lbterm.ps1 logs) ----

# Every LBTerm process writes a log of its own to ~\.local\share\lbterm (env-bootstrap's
# ringlog.rs), <exe>-log-<pid>.txt, and appends to it, so a pid used again by the same exe adds to
# the log before. A GUI starting deletes the logs nothing has written to for a week. `log` beside
# them is the daemonized mux servers' stderr, the same lines again, so it's left out.
$LbtermLogName = [regex]'^(?<exe>lbterm(?:-gui|-mux-server)?)\.exe-log-(?<pid>\d+)\.txt$'
# What the logs view calls each exe
$LbtermLogProcess = @{ 'lbterm-gui' = 'gui'; 'lbterm-mux-server' = 'mux'; 'lbterm' = 'cli' }
$LbtermLogKinds = 'daily', 'try', 'test', 'unknown'

# A line as ringlog.rs writes it to the file: the time of day only, the level padded to 6, the
# target padded to the longest so far, ' > ' and the message, whose further lines follow as they are
#   23:12:19.784  INFO   lbterm_gui::termwindow::webgpu > lbterm_compositor: drawing through ...
$LbtermLogLine = [regex]'^(?<time>\d\d:\d\d:\d\d\.\d{3})  (?<level>ERROR|WARN|INFO|DEBUG|TRACE)\s+(?<target>\S+)\s+> ?(?<msg>.*)$'

# The entries in a log's $Text, oldest first: Time, Level, Target, Message (its lines joined) and
# Lines, as written. The file holds times of day only: $End, when it was last written, dates the
# last entry, and each one before it is a day earlier whenever the clock runs backwards to it (past
# midnight, or a pid used again). A day or more with no lines leaves no trace that way, so $Start,
# when the file was made (with its first line), puts the entries before the longest silence that
# many days further back.
function ConvertFrom-LbtermLog {
  param(
    [Parameter(Mandatory)][AllowEmptyString()][string]$Text,
    [Parameter(Mandatory)][datetime]$End,
    [Nullable[datetime]]$Start
  )
  $entries = [Collections.Generic.List[object]]::new()
  $ofDay = [Collections.Generic.List[timespan]]::new()
  $entry = $null
  foreach ($line in $Text -split '\r?\n') {
    $m = $LbtermLogLine.Match($line)
    if ($m.Success) {
      $entry = [pscustomobject]@{
        Time    = [datetime]::MinValue
        Level   = $m.Groups['level'].Value
        Target  = $m.Groups['target'].Value
        Message = $m.Groups['msg'].Value
        Lines   = [Collections.Generic.List[string]]::new()
      }
      $entry.Lines.Add($line)
      $entries.Add($entry)
      $ofDay.Add([timespan]::ParseExact($m.Groups['time'].Value, 'hh\:mm\:ss\.fff', [cultureinfo]::InvariantCulture))
    } elseif ($entry -and $line.Trim()) {
      $entry.Message += "`n$line"
      $entry.Lines.Add($line)
    }
  }
  # Threads take the time before they take the file, so lines can be a few ms out of order
  $slack = [timespan]::FromSeconds(1)
  $day = $End.Date
  for ($i = $entries.Count - 1; $i -ge 0; $i--) {
    if ($i -eq $entries.Count - 1) {
      # Written just before the file's time: the day before, when that's just after midnight
      if ($ofDay[$i] -gt $End.TimeOfDay + [timespan]::FromMinutes(1)) { $day = $day.AddDays(-1) }
    } elseif ($ofDay[$i] -gt $ofDay[$i + 1] + $slack) {
      $day = $day.AddDays(-1)
    }
    $entries[$i].Time = $day + $ofDay[$i]
  }
  if ($null -ne $Start -and $entries.Count -gt 1) {
    # Whole days between the first line as dated and the file's making. A week or more is a
    # creation time NTFS carried over from a deleted log of the same name, not a silence.
    $days = [Math]::Round(($entries[0].Time - $Start).TotalDays)
    if ($days -ge 1 -and $days -lt 7) {
      $gap = 1
      for ($i = 2; $i -lt $entries.Count; $i++) {
        if ($entries[$i].Time - $entries[$i - 1].Time -gt $entries[$gap].Time - $entries[$gap - 1].Time) { $gap = $i }
      }
      for ($i = 0; $i -lt $gap; $i++) { $entries[$i].Time = $entries[$i].Time.AddDays(-$days) }
    }
  }
  $entries.ToArray()
}

# Which LBTerm a running exe belongs to, by where it runs from: $Install (the install folder) is
# the daily LBTerm, a tree's target\try\bin a try window (lbterm.ps1 try), anywhere else a test
function Get-LbtermExeKind {
  param([Parameter(Mandatory)][string]$ExePath, [Parameter(Mandatory)][string]$Install)
  $dir = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($ExePath)).TrimEnd('\')
  if ($dir -eq [IO.Path]::GetFullPath($Install).TrimEnd('\')) { return 'daily' }
  if ($dir -match '\\target\\try\\bin$') { return 'try' }
  'test'
}

# For a log whose process has gone, what it says about the LBTerm that wrote it. Nothing in a log
# names its own exe, so this goes by the paths and names it mentions, strongest first. A try
# window's socket and server are under a tree's target\try, and its GUI's log names both, since it
# always starts a server of its own. A test window's are under a tree's target or the scratchpad,
# it may name LBTERM_TEST or its lbterm-perf class or pipe, and only test scripts turn on debug
# logging (WEZTERM_LOG). Try and test windows name daily things too (Lachlan's config's conductor
# pipe, the installed mux server run with a test config), so those count only when nothing else is
# there: the install folder, the daily mux server's socket, the conductor hub's pipe.
$LbtermLogClues = @(
  @{ Kind = 'try'; Says = 'names a try build'; Pattern = '[a-z]:\\[^"\s;,)]*?\\target\\try\\[^"\s;,)]*' }
  @{ Kind = 'test'; Says = "names a tree's build"; Pattern = '[a-z]:\\[^"\s;,)]*?\\target\\[^"\s;,)]*' }
  @{ Kind = 'test'; Says = 'names a worktree'; Pattern = '[a-z]:\\[^"\s;,)]*?\\\.claude\\worktrees\\[^"\s;,)]*' }
  @{ Kind = 'test'; Says = 'names a temporary folder'; Pattern = '[a-z]:\\[^"\s;,)]*?\\AppData\\Local\\Temp\\[^"\s;,)]*' }
  @{ Kind = 'test'; Says = 'names a test'; Pattern = 'LBTERM[_-]TEST\w*|lbterm[.-]perf[\w.-]*' }
  @{ Kind = 'test'; Says = 'logs at debug level, which only test scripts turn on'; Pattern = '(?m)^\d\d:\d\d:\d\d\.\d{3}  (DEBUG|TRACE) +\S+' }
  @{ Kind = 'daily'; Says = 'names the installed build'; Pattern = '[a-z]:\\[^"\s;,)]*?\\Programs\\LBTerm\\[^"\s;,)]*' }
  @{ Kind = 'daily'; Says = "names the daily mux server's socket"; Pattern = '[a-z]:\\[^"\s;,)]*?\\\.local\\share\\lbterm\\sock\b' }
  @{ Kind = 'daily'; Says = "listens on the conductor hub's pipe"; Pattern = '\\pipe\\conductor-hub\b' }
)

# Kind (try, test, daily or unknown) and Reason, the words in $Text that said so
function Get-LbtermLogEvidence([AllowEmptyString()][string]$Text) {
  # Rust's Debug doubles backslashes, and some paths are written with slashes
  $t = $Text -replace '\\\\', '\' -replace '/', '\'
  foreach ($clue in $LbtermLogClues) {
    $m = [regex]::Match($t, $clue.Pattern, 'IgnoreCase')
    if ($m.Success) { return [pscustomobject]@{ Kind = $clue.Kind; Reason = "$($clue.Says): $($m.Value.Trim().TrimEnd('.', ':'))" } }
  }
  [pscustomobject]@{ Kind = 'unknown'; Reason = 'it names no build, socket or test' }
}

# A log's text, read beside the process still writing it: File.ReadAllText shares the file only
# for reading, which a file open for writing refuses
function Read-SharedText([Parameter(Mandatory)][string]$Path) {
  $stream = [IO.File]::Open($Path, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
  try { [IO.StreamReader]::new($stream).ReadToEnd() } finally { $stream.Dispose() }
}

# Every log in $Dir, newest (last written) first: Name, Path, Exe, Process (gui, mux or cli), Pid,
# Alive, Kind, Reason, Started, Written, Size, Errors, Warnings and Entries. $Live holds the LBTerm
# processes running now by pid, each @{ Name = 'lbterm-gui'; Path; StartTime }. One is a log's
# writer when it's the same exe and started before the log was last written: one started since,
# under a pid used again, hasn't written to it. A running process's kind is where its exe is
# (Get-LbtermExeKind), a finished one's what its log says (Get-LbtermLogEvidence).
function Get-LbtermLogs {
  param(
    [Parameter(Mandatory)][string]$Dir,
    [Parameter(Mandatory)][string]$Install,
    [hashtable]$Live = @{}
  )
  $logs = foreach ($file in Get-ChildItem $Dir -File -ErrorAction SilentlyContinue) {
    $m = $LbtermLogName.Match($file.Name)
    if (-not $m.Success) { continue }
    # A GUI starting may delete it between the listing and here
    try { $text = Read-SharedText $file.FullName } catch { continue }
    $exe = $m.Groups['exe'].Value
    $procId = [int]$m.Groups['pid'].Value
    $entries = @(ConvertFrom-LbtermLog $text $file.LastWriteTime -Start $file.CreationTime)
    $errors = $warnings = 0
    foreach ($e in $entries) {
      if ($e.Level -eq 'ERROR') { $errors++ } elseif ($e.Level -eq 'WARN') { $warnings++ }
    }
    $p = $Live[$procId]
    $alive = [bool]($p -and $p.Name -eq $exe -and $p.StartTime -le $file.LastWriteTime.AddSeconds(1))
    if ($alive -and $p.Path) {
      $kind, $reason = (Get-LbtermExeKind $p.Path $Install), "runs from $($p.Path)"
    } else {
      $e = Get-LbtermLogEvidence $text
      $kind, $reason = $e.Kind, $e.Reason
    }
    [pscustomobject]@{
      Name     = $file.Name
      Path     = $file.FullName
      Exe      = $exe
      Process  = $LbtermLogProcess[$exe]
      Pid      = $procId
      Alive    = $alive
      Kind     = $kind
      Reason   = $reason
      Started  = if ($alive) { $p.StartTime } else { $file.CreationTime }
      Written  = $file.LastWriteTime
      Size     = $file.Length
      Errors   = $errors
      Warnings = $warnings
      Entries  = $entries
    }
  }
  $logs | Sort-Object Written -Descending
}

# $Logs of one kind (all of them without one), written since $Since
function Select-LbtermLogs {
  param([object[]]$Logs, [string]$Kind, [Nullable[datetime]]$Since)
  $Logs | Where-Object { (-not $Kind -or $_.Kind -eq $Kind) -and ($null -eq $Since -or $_.Written -ge $Since) }
}

# -Since as typed: 30m, 2h or 1d back from $Now, or a time ('18:00' is today's, '2026-10-21 18:00')
function ConvertTo-LbtermSince {
  param([Parameter(Mandatory)][string]$Text, [datetime]$Now = (Get-Date))
  if ($Text -match '^\s*(\d+)\s*(s|m|h|d)\s*$') {
    $n = [int]$Matches[1]
    return $(switch ($Matches[2]) { 's' { $Now.AddSeconds(-$n) } 'm' { $Now.AddMinutes(-$n) } 'h' { $Now.AddHours(-$n) } 'd' { $Now.AddDays(-$n) } })
  }
  $when = [datetime]::MinValue
  if ([datetime]::TryParse($Text, [cultureinfo]::CurrentCulture, 'AllowWhiteSpaces', [ref]$when)) { return $when }
  throw "-Since '$Text' is neither a time (18:00, 2026-10-21 18:00) nor a while back (30m, 2h, 1d)"
}

# A message with what changes from one time to the next taken out, so the same problem reads the
# same each time: paths become <path> (a test's sockets and configs are all over the disk), and
# GUIDs, hex (0x80070057, a commit's f3b4ccda) and numbers (pids, sizes, error codes) <id>, <hex>
# and <n>. A Rust source file keeps its name (<path>\wgpu_core.rs:<n>:<n>): where in the code a
# panic was tells one from another.
function ConvertTo-LbtermLogKey([AllowEmptyString()][string]$Message) {
  $s = ($Message -replace '\s+', ' ').Trim() -replace '\\\\', '\'
  $s = [regex]::Replace($s, '(?i)\b[a-z]:[\\/][^\s"''<>|*?(),;]*', {
    param($m)
    $path = $m.Value
    $tail = ''
    while ($path -match '[.:]$') { $tail = "$($path[-1])$tail"; $path = $path.Substring(0, $path.Length - 1) }
    $leaf = ($path -split '[\\/]')[-1]
    if ($leaf -match '\.rs(:\d+)*$') { "<path>\$leaf$tail" } else { "<path>$tail" }
  })
  $s = $s -replace '(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b', '<id>'
  $s = $s -replace '(?i)\b0x[0-9a-f]+\b', '<hex>'
  $s = $s -replace '(?i)\b(?=[0-9a-f]*[a-f])(?=[0-9a-f]*\d)[0-9a-f]{6,}\b', '<hex>'
  $s -replace '\d+(\.\d+)?', '<n>'
}

# The ERROR and WARN entries of $Logs (since $Since) grouped by kind: level, target and
# ConvertTo-LbtermLogKey's message. Each with Count, First, Last and the Logs it's in; most often
# first. Slow-frame lines are left to Group-LbtermSlowFrames, and the budget's to
# Group-LbtermBudgetLines.
function Group-LbtermLogProblems {
  param([object[]]$Logs, [Nullable[datetime]]$Since)
  $groups = @{}
  $keys = @{}
  $seen = [Collections.Generic.HashSet[string]]::new()
  foreach ($log in $Logs) {
    foreach ($e in $log.Entries) {
      if ($e.Level -ne 'ERROR' -and $e.Level -ne 'WARN') { continue }
      if ($e.Target -eq $LbtermSlowTarget -or $e.Target -eq $LbtermBudgetTarget) { continue }
      if ($null -ne $Since -and $e.Time -lt $Since) { continue }
      # Normalised once per distinct message: most are the same few lines again and again
      $message = $keys[$e.Message]
      if ($null -eq $message) { $message = $keys[$e.Message] = ConvertTo-LbtermLogKey $e.Message }
      $key = "$($e.Level)|$($e.Target)|$message"
      $g = $groups[$key]
      if (-not $g) {
        $g = $groups[$key] = [pscustomobject]@{
          Level = $e.Level; Target = $e.Target; Message = $message; Count = 0
          First = $e.Time; Last = $e.Time; Logs = [Collections.Generic.List[object]]::new()
        }
      }
      $g.Count++
      if ($e.Time -lt $g.First) { $g.First = $e.Time }
      if ($e.Time -gt $g.Last) { $g.Last = $e.Time }
      if ($seen.Add("$key|$($log.Path)")) { $g.Logs.Add($log) }
    }
  }
  $groups.Values | Sort-Object @{ Expression = 'Count'; Descending = $true }, @{ Expression = 'Last'; Descending = $true }
}

# The slow-frame lines' target (feat/slow-frames): a WARN for each frame over its threshold, or one
# for many in the last 10 s
$LbtermSlowTarget = 'lbterm::slow'
$LbtermSlowOne = [regex]'^slow (?<kind>message|task|paint) (?<site>.+?): (?<ms>\d+(?:\.\d+)?) ms$'
$LbtermSlowMany = [regex]'^slow (?<kind>message|task|paint) (?<site>.+?): (?<n>\d+) times over (?<over>\d+(?:\.\d+)?) ms in the last 10 s, max (?<ms>\d+(?:\.\d+)?) ms$'

# A slow-frame message, 'slow paint window.rs:812: 41 ms' or 'slow task <site>: 12 times over
# 16 ms in the last 10 s, max 48 ms': its Kind, Site, Count (how many slow frames it stands for)
# and Ms (the slowest). Nothing for any other message.
function ConvertFrom-LbtermSlowLine([AllowEmptyString()][string]$Message) {
  $m = $LbtermSlowMany.Match($Message)
  $count = 0
  if ($m.Success) { $count = [int]$m.Groups['n'].Value }
  else {
    $m = $LbtermSlowOne.Match($Message)
    if (-not $m.Success) { return }
    $count = 1
  }
  [pscustomobject]@{
    Kind  = $m.Groups['kind'].Value
    Site  = $m.Groups['site'].Value
    Count = $count
    Ms    = [double]::Parse($m.Groups['ms'].Value, [cultureinfo]::InvariantCulture)
  }
}

# The slow-frame lines of $Logs (since $Since) grouped by kind and site: Count (slow frames), Lines,
# Worst (ms), First, Last and Logs; the most slow frames first
function Group-LbtermSlowFrames {
  param([object[]]$Logs, [Nullable[datetime]]$Since)
  $groups = @{}
  $seen = [Collections.Generic.HashSet[string]]::new()
  foreach ($log in $Logs) {
    foreach ($e in $log.Entries) {
      if ($e.Target -ne $LbtermSlowTarget) { continue }
      if ($null -ne $Since -and $e.Time -lt $Since) { continue }
      $slow = ConvertFrom-LbtermSlowLine $e.Message
      if (-not $slow) { continue }
      $key = "$($slow.Kind)|$($slow.Site)"
      $g = $groups[$key]
      if (-not $g) {
        $g = $groups[$key] = [pscustomobject]@{
          Kind = $slow.Kind; Site = $slow.Site; Count = 0; Lines = 0; Worst = 0.0
          First = $e.Time; Last = $e.Time; Logs = [Collections.Generic.List[object]]::new()
        }
      }
      $g.Count += $slow.Count
      $g.Lines++
      if ($slow.Ms -gt $g.Worst) { $g.Worst = $slow.Ms }
      if ($e.Time -lt $g.First) { $g.First = $e.Time }
      if ($e.Time -gt $g.Last) { $g.Last = $e.Time }
      if ($seen.Add("$key|$($log.Path)")) { $g.Logs.Add($log) }
    }
  }
  $groups.Values | Sort-Object @{ Expression = 'Count'; Descending = $true }, @{ Expression = 'Worst'; Descending = $true }
}

# The budget's lines' target (perf/budgets, lbterm/PERF.md): what ran at rest, and actions over their
# time. Each shape as lbterm-extras/src/budget.rs writes it; its tests hold the same shapes
$LbtermBudgetTarget = 'lbterm::budget'
$LbtermBudgetFrames = [regex]'^window-(?<window>\d+) painted (?<n>\d+) frames (?:in (?<s>\d+) s at rest|at rest in (?<spans>\d+) more spans? of \d+ s in the last \d+ s), asked by (?<sites>.+)$'
$LbtermBudgetRuns = [regex]'^(?<kind>timers fired|tasks ran) (?<n>\d+) times (?:in (?<s>\d+) s at rest|at rest in (?<spans>\d+) more spans? of \d+ s in the last \d+ s)(?:, (?<ms>\d+(?:\.\d+)?) ms in all)?: (?<sites>.+)$'
$LbtermBudgetAction = [regex]'^(?<action>[a-z-]+) took (?<ms>\d+(?:\.\d+)?) ms, (?<over>\d+(?:\.\d+)?) ms over its (?<budget>\d+(?:\.\d+)?) ms: (?<went>.+)$'
$LbtermBudgetActions = [regex]'^(?<action>[a-z-]+): (?<n>\d+) times over its (?<budget>\d+(?:\.\d+)?) ms in the last (?<s>\d+) s, max (?<ms>\d+(?:\.\d+)?) ms$'

# 'a.rs:1 (300), b.rs:2 (12, 4.5 ms), and 3 more (9); and 2 nobody asked for' as its sites, most
# first: Site, Count and Ms (tasks only). What follows the first '; ' is notes, not sites
function ConvertFrom-LbtermBudgetSites([string]$Text) {
  $list = ($Text -split '; ', 2)[0]
  foreach ($part in ($list -split ', (?=\S+ \(|and \d+ more \()')) {
    if ($part -match '^(?<site>\S+) \((?<n>\d+)(?:, (?<ms>\d+(?:\.\d+)?) ms)?\)$') {
      [pscustomobject]@{
        Site  = $Matches['site']
        Count = [int]$Matches['n']
        Ms    = if ($Matches['ms']) { [double]::Parse($Matches['ms'], [cultureinfo]::InvariantCulture) } else { $null }
      }
    } elseif ($part -match '^and (?<k>\d+) more \((?<n>\d+)\)$') {
      [pscustomobject]@{ Site = "($($Matches['k']) more)"; Count = [int]$Matches['n']; Ms = $null }
    }
  }
}

# A budget line's message: Kind (frames, timers, tasks or action), What (the window, or the
# action's name), Count (frames, timers or tasks at rest, or actions over), Spans (how many 10 s
# spans it covers, an action's line 1), Ms (the tasks' time, or how long the action took, the
# longest in a summary), Budget (an action's time), Sites (who asked, or what the time went on,
# most first) and Top (the first of them). Nothing for any other message.
function ConvertFrom-LbtermBudgetLine([AllowEmptyString()][string]$Message) {
  $num = { param($s) if ($s) { [double]::Parse($s, [cultureinfo]::InvariantCulture) } else { $null } }
  $m = $LbtermBudgetFrames.Match($Message)
  if ($m.Success) {
    $sites = @(ConvertFrom-LbtermBudgetSites $m.Groups['sites'].Value)
    return [pscustomobject]@{
      Kind = 'frames'; What = "window-$($m.Groups['window'].Value)"; Count = [int]$m.Groups['n'].Value
      Spans = if ($m.Groups['spans'].Success) { [int]$m.Groups['spans'].Value } else { 1 }
      Ms = $null; Budget = $null; Sites = $sites; Top = if ($sites) { $sites[0].Site } else { '' }
    }
  }
  $m = $LbtermBudgetRuns.Match($Message)
  if ($m.Success) {
    $sites = @(ConvertFrom-LbtermBudgetSites $m.Groups['sites'].Value)
    return [pscustomobject]@{
      Kind = if ($m.Groups['kind'].Value -eq 'timers fired') { 'timers' } else { 'tasks' }
      What = 'process'; Count = [int]$m.Groups['n'].Value
      Spans = if ($m.Groups['spans'].Success) { [int]$m.Groups['spans'].Value } else { 1 }
      Ms = & $num $m.Groups['ms'].Value; Budget = $null; Sites = $sites; Top = if ($sites) { $sites[0].Site } else { '' }
    }
  }
  $m = $LbtermBudgetAction.Match($Message)
  if ($m.Success) {
    $went = foreach ($part in ($m.Groups['went'].Value -split ', ')) {
      if ($part -match '^(?<kind>message|task|paint) (?<site>\S+) (?<ms>\d+(?:\.\d+)?) ms(?: \((?<n>\d+)\))?$') {
        [pscustomobject]@{ Site = "$($Matches['kind']) $($Matches['site'])"; Count = if ($Matches['n']) { [int]$Matches['n'] } else { 1 }; Ms = & $num $Matches['ms'] }
      } elseif ($part -match '^(?<what>others|the rest) (?<ms>\d+(?:\.\d+)?) ms$') {
        [pscustomobject]@{ Site = $Matches['what']; Count = 0; Ms = & $num $Matches['ms'] }
      }
    }
    $went = @($went)
    return [pscustomobject]@{
      Kind = 'action'; What = $m.Groups['action'].Value; Count = 1; Spans = 1
      Ms = & $num $m.Groups['ms'].Value; Budget = & $num $m.Groups['budget'].Value
      Sites = $went; Top = if ($went) { $went[0].Site } else { '' }
    }
  }
  $m = $LbtermBudgetActions.Match($Message)
  if ($m.Success) {
    # The 10 s's first was said on a line of its own: these are the rest
    return [pscustomobject]@{
      Kind = 'action'; What = $m.Groups['action'].Value; Count = [int]$m.Groups['n'].Value - 1; Spans = 0
      Ms = & $num $m.Groups['ms'].Value; Budget = & $num $m.Groups['budget'].Value; Sites = @(); Top = ''
    }
  }
}

# The budget's lines of $Logs (since $Since) grouped: what ran at rest by its kind and who asked
# most (Count added up, Worst the most in one line, Ms the tasks' time), and actions over their
# time by action (Count of them, Worst the longest, Budget its time, Top what the time went on
# most often first). Each with Lines, First, Last and Logs; at rest first, the most first
function Group-LbtermBudgetLines {
  param([object[]]$Logs, [Nullable[datetime]]$Since)
  $groups = @{}
  $seen = [Collections.Generic.HashSet[string]]::new()
  foreach ($log in $Logs) {
    foreach ($e in $log.Entries) {
      if ($e.Target -ne $LbtermBudgetTarget) { continue }
      if ($null -ne $Since -and $e.Time -lt $Since) { continue }
      $b = ConvertFrom-LbtermBudgetLine $e.Message
      if (-not $b) { continue }
      $key = if ($b.Kind -eq 'action') { "action|$($b.What)" } else { "$($b.Kind)|$($b.Top)" }
      $g = $groups[$key]
      if (-not $g) {
        $g = $groups[$key] = [pscustomobject]@{
          Kind = $b.Kind; Site = if ($b.Kind -eq 'action') { $b.What } else { $b.Top }; Count = 0; Lines = 0
          Worst = 0.0; Ms = 0.0; Budget = $b.Budget; Tops = @{}; Top = ''
          First = $e.Time; Last = $e.Time; Logs = [Collections.Generic.List[object]]::new()
        }
      }
      $g.Count += $b.Count
      $g.Lines++
      $worst = if ($b.Kind -eq 'action') { $b.Ms } else { $b.Count }
      if ($worst -gt $g.Worst) { $g.Worst = $worst }
      if ($b.Kind -eq 'tasks' -and $b.Ms) { $g.Ms += $b.Ms }
      if ($b.Kind -eq 'action' -and $b.Top) { $g.Tops[$b.Top] = 1 + [int]$g.Tops[$b.Top] }
      if ($e.Time -lt $g.First) { $g.First = $e.Time }
      if ($e.Time -gt $g.Last) { $g.Last = $e.Time }
      if ($seen.Add("$key|$($log.Path)")) { $g.Logs.Add($log) }
    }
  }
  foreach ($g in $groups.Values) {
    if ($g.Tops.Count) { $g.Top = ($g.Tops.GetEnumerator() | Sort-Object @{ Expression = 'Value'; Descending = $true }, Name | Select-Object -First 1).Name }
  }
  $order = @{ frames = 0; timers = 1; tasks = 2; action = 3 }
  $groups.Values | Sort-Object @{ Expression = { $order[$_.Kind] } }, @{ Expression = 'Count'; Descending = $true }, @{ Expression = 'Worst'; Descending = $true }
}

# The -Budget view: what ran at rest, then what was over its time
function Format-LbtermBudget([object[]]$Groups) {
  foreach ($g in $Groups) {
    if ($g.Kind -eq 'action') {
      '{0,6}  over    {1,-14}  worst {2,7:0.#} ms, its {3} ms{4}' -f $g.Count, $g.Site, $g.Worst, $g.Budget, $(if ($g.Top) { "; mostly $($g.Top)" } else { '' })
    } else {
      $time = if ($g.Kind -eq 'tasks' -and $g.Ms) { (', {0:0.#} ms' -f $g.Ms) } else { '' }
      $who = @{ frames = 'asked by'; timers = 'armed at'; tasks = 'spawned at' }[$g.Kind]
      '{0,6}  {1,-6}  at rest{2}, at most {3} in a line, most {4} {5}' -f $g.Count, $g.Kind, $time, $g.Worst, $who, $g.Site
    }
    "        $($g.Lines) line$(if ($g.Lines -ne 1) { 's' }) $(Format-LbtermSeen $g)"
  }
}

# How many of $Logs are of each kind: 'daily 2, test 40'
function Format-LbtermLogKinds([object[]]$Logs) {
  $counts = foreach ($k in $LbtermLogKinds) {
    $n = @($Logs | Where-Object Kind -eq $k).Count
    if ($n) { "$k $n" }
  }
  $counts -join ', '
}

# Which of $Logs, newest first: 'gui 5964, mux 14184, cli 220 and 12 more'
function Format-LbtermLogNames([object[]]$Logs, [int]$Max = 4) {
  $sorted = @($Logs | Sort-Object Written -Descending)
  $names = @($sorted | Select-Object -First $Max | ForEach-Object { "$($_.Process) $($_.Pid)" }) -join ', '
  if ($sorted.Count -gt $Max) { "$names and $($sorted.Count - $Max) more" } else { $names }
}

function Format-LbtermLogSize([long]$Bytes) {
  if ($Bytes -lt 1KB) { "$Bytes B" }
  elseif ($Bytes -lt 1MB) { '{0:0.#} KB' -f ($Bytes / 1KB) }
  else { '{0:0.#} MB' -f ($Bytes / 1MB) }
}

# 'from 2026-10-19 21:02 to 2026-10-21 14:11 in 11 logs (test 9, unknown 2): gui 1234, ...'
function Format-LbtermSeen($Group) {
  $first = $Group.First.ToString('yyyy-MM-dd HH:mm')
  $last = $Group.Last.ToString('yyyy-MM-dd HH:mm')
  $when = if ($first -eq $last) { "at $first" } else { "from $first to $last" }
  $n = $Group.Logs.Count
  "$when in $n log$(if ($n -ne 1) { 's' }) ($(Format-LbtermLogKinds $Group.Logs)): $(Format-LbtermLogNames $Group.Logs)"
}

# The logs view: one row per log
function Format-LbtermLogList([object[]]$Logs) {
  $table = ($Logs | Format-Table -AutoSize -Property @(
      @{ n = 'kind'; e = { $_.Kind } }
      @{ n = 'process'; e = { $_.Process } }
      @{ n = 'pid'; e = { $_.Pid } }
      @{ n = 'alive'; e = { if ($_.Alive) { 'yes' } else { 'no' } } }
      @{ n = 'started'; e = { $_.Started.ToString('yyyy-MM-dd HH:mm:ss') } }
      @{ n = 'last written'; e = { $_.Written.ToString('yyyy-MM-dd HH:mm:ss') } }
      @{ n = 'size'; e = { Format-LbtermLogSize $_.Size }; align = 'right' }
      @{ n = 'errors'; e = { $_.Errors } }
      @{ n = 'warnings'; e = { $_.Warnings } }
    ) | Out-String -Width 4096).Trim("`r", "`n")
  ($table -split '\r?\n' | ForEach-Object TrimEnd) -join "`n"
}

# The -Errors view: two lines a kind, then when and where
function Format-LbtermLogProblems([object[]]$Groups) {
  foreach ($g in $Groups) {
    $message = if ($g.Message.Length -gt 400) { $g.Message.Substring(0, 400) + ' ...' } else { $g.Message }
    '{0,6}  {1,-5}  {2}' -f $g.Count, $g.Level, $g.Target
    "        $message"
    "        $(Format-LbtermSeen $g)"
  }
}

# The -Slow view
function Format-LbtermSlowFrames([object[]]$Groups) {
  foreach ($g in $Groups) {
    '{0,6}  slow {1,-7}  worst {2,6:0.#} ms  {3}' -f $g.Count, $g.Kind, $g.Worst, $g.Site
    "        $($g.Lines) line$(if ($g.Lines -ne 1) { 's' }) $(Format-LbtermSeen $g)"
  }
}

# ---- The public repo (lbterm.ps1 publish) ----

# What the public repo leaves out: the notes and memory for Claude, which name this PC's paths and
# Lachlan's other projects, and the contract (api.json), which says what those projects use of it
$PublishExcludes = '.claude', 'CLAUDE.md', 'api.json'

# The tree of $Commit in $Repo less $Exclude, written as a tree object through an index of its own
# (the checkout's own is untouched); its id. Submodules stay the commits they are.
function Get-PublicTree {
  param(
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][string]$Commit,
    [string[]]$Exclude = $PublishExcludes
  )
  $index = Join-Path (git -C $Repo rev-parse --absolute-git-dir) 'lbterm-publish.index'
  $saved = $env:GIT_INDEX_FILE
  $env:GIT_INDEX_FILE = $index
  try {
    git -C $Repo read-tree $Commit
    if ($LASTEXITCODE) { throw "git read-tree $Commit failed" }
    git -C $Repo rm -r -q --cached --ignore-unmatch -- @Exclude | Out-Null
    if ($LASTEXITCODE) { throw "git rm --cached $($Exclude -join ' ') failed" }
    $tree = git -C $Repo write-tree
    if ($LASTEXITCODE) { throw 'git write-tree failed' }
    $tree
  } finally {
    $env:GIT_INDEX_FILE = $saved
    Remove-Item -LiteralPath $index -Force -ErrorAction SilentlyContinue
  }
}

function Test-Ancestor([string]$Repo, [string]$Ancestor, [string]$Commit) {
  git -C $Repo merge-base --is-ancestor $Ancestor $Commit 2>$null
  $LASTEXITCODE -eq 0
}

# The private commit the public clone's last export was made from (its LBTerm-Commit trailer), or
# nothing before the first
function Get-PublishedFrom([Parameter(Mandatory)][string]$Public) {
  $message = (git -C $Public log -1 --format=%B 2>$null) -join "`n"
  $m = [regex]::Match($message, '(?m)^LBTerm-Commit: ([0-9a-f]{40})\s*$')
  if ($m.Success) { $m.Groups[1].Value }
}

# The message of an export of $Commit: the merges into main since the last export ($Since), a line
# each without their "Merge <branch>: ", or for the first export a line saying what LBTerm is; then
# the LBTerm-Commit trailer the next export counts from
function Get-PublishMessage {
  param(
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][string]$Commit,
    [string]$Since,
    [datetime]$Now = (Get-Date)
  )
  $date = $Now.ToString('yyyy-MM-dd')
  $lines = @("LBTerm, $date", '')
  $merges = if ($Since) { @(git -C $Repo log --first-parent --merges --format=%s "$Since..$Commit") }
  if ($merges) {
    $lines += $merges | ForEach-Object { '- ' + ($_ -replace '^Merge [^:]+: ', '') }
  } elseif ($Since) {
    $lines += 'Changes since the last export.'
  } else {
    $lines += "Lachlan Brown's fork of WezTerm, as it stood on $date."
  }
  ($lines + @('', "LBTerm-Commit: $Commit")) -join "`n"
}

# Exports $Ref of the private $Repo to the public clone $Public as one new commit: its tree less
# $PublishExcludes, on top of the last export, with the upstream commit $Ref is based on (its
# merge-base with $UpstreamRef) as the parent of the first and a second parent whenever it moves.
# So the public repo carries WezTerm's history and its own exports, never a commit of the private
# repo's. The tree crosses as a commit on that upstream base, under refs/lbterm/publish for the
# length of a fetch. The clone's hooks (lbterm/publish/hooks: leak-check.sh) check the commit and
# its message; when they refuse, the clone is put back as it was and this throws. Returns the new
# commit, or nothing when an export would change nothing. Pushing is up to the caller.
function Publish-Lbterm {
  param(
    [Parameter(Mandatory)][string]$Repo,
    [Parameter(Mandatory)][string]$Public,
    [string]$Ref = 'main',
    [string]$UpstreamRef = 'upstream/main',
    [string]$Message,
    [datetime]$Now = (Get-Date)
  )
  if (-not (Test-Path -LiteralPath (Join-Path $Public '.git'))) { throw "No public clone at $Public" }
  if ((git -C $Public config core.hooksPath) -ne 'lbterm/publish/hooks') {
    throw "The public clone's hooks aren't on, so nothing would check an export: git -C `"$Public`" config core.hooksPath lbterm/publish/hooks"
  }
  $email = git -C $Public config user.email
  if (-not $email -or $email -notmatch '@users\.noreply\.github\.com$') {
    throw "The public clone commits as `"$email`": give it the GitHub noreply identity (git -C `"$Public`" config user.email <id>+<user>@users.noreply.github.com)"
  }
  $name = git -C $Public config user.name
  if (git -C $Public status --porcelain) { throw "The public clone at $Public has changes of its own: commit or remove them first" }

  $commit = git -C $Repo rev-parse --verify -q "$Ref^{commit}"
  if (-not $commit) { throw "No $Ref in $Repo" }
  $base = git -C $Repo merge-base $commit $UpstreamRef
  if (-not $base) { throw "$Ref and $UpstreamRef share no commit" }
  $tree = Get-PublicTree -Repo $Repo -Commit $commit
  $head = git -C $Public rev-parse --verify -q 'HEAD^{commit}'
  $newBase = -not $head -or -not (Test-Ancestor $Public $base $head)
  if (-not $newBase -and (git -C $Public rev-parse 'HEAD^{tree}') -eq $tree) { return }

  $transport = git -C $Repo -c "user.name=$name" -c "user.email=$email" commit-tree $tree -p $base -m 'lbterm publish'
  if ($LASTEXITCODE) { throw 'git commit-tree failed' }
  git -C $Repo update-ref refs/lbterm/publish $transport
  try {
    git -C $Public fetch -q --no-tags $Repo refs/lbterm/publish
    if ($LASTEXITCODE) { throw "The public clone couldn't fetch the export from $Repo" }
  } finally {
    git -C $Repo update-ref -d refs/lbterm/publish
  }
  # What leak-check.sh --all leaves out as WezTerm's
  git -C $Public update-ref refs/remotes/upstream/base $base

  if (-not $Message) {
    $Message = Get-PublishMessage -Repo $Repo -Commit $commit -Since (Get-PublishedFrom $Public) -Now $Now
  }
  $messageFile = Join-Path (git -C $Public rev-parse --absolute-git-dir) 'LBTERM_PUBLISH_MSG'
  [IO.File]::WriteAllText($messageFile, "$Message`n")
  try {
    if (-not $head) {
      git -C $Public update-ref HEAD $base
    } elseif ($newBase) {
      git -C $Public merge -q -s ours --no-ff --no-commit $base
      if ($LASTEXITCODE) { throw "git merge of the upstream base $base failed" }
    }
    git -C $Public read-tree -u --reset $tree
    if ($LASTEXITCODE) { throw 'git read-tree of the export failed' }
    git -C $Public commit -q -F $messageFile
    if ($LASTEXITCODE) {
      if ($head) {
        git -C $Public reset -q --hard $head
      } else {
        # Back to an empty clone: no branch, no index, none of the export's files
        git -C $Public update-ref -d HEAD
        git -C $Public read-tree --empty
        git -C $Public clean -f -d -q
      }
      throw "The public clone's hooks refused the export (above), so nothing was committed"
    }
  } finally {
    Remove-Item -LiteralPath $messageFile -Force -ErrorAction SilentlyContinue
  }
  git -C $Public rev-parse HEAD
}
