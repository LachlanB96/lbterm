#Requires -Version 7
<#
LBTerm's command line: build, check, install and start the fork, and see how far it is from upstream.

  pwsh -File lbterm\lbterm.ps1 build      release build of lbterm, lbterm-gui, lbterm-mux-server, strip-ansi-escapes
  pwsh -File lbterm\lbterm.ps1 check      THE CHECK: build, LBTerm's tests, then a smoke test of the built exes
  pwsh -File lbterm\lbterm.ps1 test -Package lbterm-extras   cargo test on the crates named, with the build's settings
  pwsh -File lbterm\lbterm.ps1 install    copy the build to %LOCALAPPDATA%\Programs\LBTerm, beside a running LBTerm
  pwsh -File lbterm\lbterm.ps1 rollback   swap back to the build the last install replaced, and restart into it
  pwsh -File lbterm\lbterm.ps1 start      open the installed LBTerm
  pwsh -File lbterm\lbterm.ps1 restart    end the GUI and open the installed one; the tabs live on in the mux server
  pwsh -File lbterm\lbterm.ps1 try        build, and open the build in a window of its own beside the daily LBTerm
  pwsh -File lbterm\lbterm.ps1 upstream   fetch wezterm/wezterm and count the commits on each side
  pwsh -File lbterm\lbterm.ps1 publish [-Push] [-Public <dir>] [-Message <text>]
                                          export main to the public repo's clone as one commit; -Push sends it
  pwsh -File lbterm\lbterm.ps1 logs       every LBTerm process's log, newest first: daily, try or test, errors, warnings
  pwsh -File lbterm\lbterm.ps1 logs -Pid 5964        print one log (-Latest: the newest), -Follow to follow it
  pwsh -File lbterm\lbterm.ps1 logs -Errors -Kind daily -Since 2h   ERROR and WARN lines grouped by kind
  pwsh -File lbterm\lbterm.ps1 logs -Slow     the slow-frame lines (lbterm::slow) grouped by kind and site
  pwsh -File lbterm\lbterm.ps1 e2e [-Only a,b] [-Changed] [-Bin <dir>] [-Retry 1] [-WaitIdle]
                                          the end-to-end tests in lbterm\perf, one at a time (not part of the check)

logs reads ~\.local\share\lbterm, where every lbterm-gui, lbterm-mux-server and lbterm (cli) process
writes <exe>-log-<pid>.txt, from the daily LBTerm, try windows and test windows alike. Each log's
kind: a running process's is where its exe is (the install folder is daily, a tree's target\try\bin
try, anywhere else test); a finished one's is what its log names (a try build's socket or server, a
tree's target or the scratchpad, LBTERM_TEST, debug logging, which only tests turn on; else the
install folder, the daily mux server's socket, the conductor hub's pipe), or unknown when it names
none of them. -Kind daily|try|test|unknown and -Since (18:00, 2026-10-21 18:00, 30m, 2h, 1d) narrow
every view. -Errors takes out what changes from one line to the next (numbers, pids, hex, paths
but a Rust source file's name) so the same problem groups together. The lines carry times of day
only: they're dated back from when each log was last written.

e2e runs each test script (its Variant lines too) against -Bin (this tree's target\release by
default), with its output in target\e2e\<run>\<script>; a failure runs again -Retry times and is
flaky if a rerun passes. -Changed picks the scripts whose Covers globs match a file changed since
main (committed or not). report.json and a table at the end; exit 1 when one fails.

A running LBTerm keeps its files open, so install renames each one aside (<name>.<stamp>.old) and copies
the new one in; the next install deletes the old ones nothing holds any more. With the mux domain on
(lbterm/MUX.md) the tabs live in lbterm-mux-server, so restart swaps the GUI without ending them.

Install writes the build's version (wezterm.version in Lua) to version.txt beside the exes, and
keeps the build it replaces whole in %LOCALAPPDATA%\Programs\LBTerm-previous. rollback swaps the
two, so a second rollback goes forward again.

Cargo runs under a thermal guard (guarded.sh), because this PC's CPU must never pass 94 C: it pins
the build to 8 logical CPUs, pauses cargo, rustc, cl and link while the CPU is hot, and refuses to
start at 80 C or when HWiNFO isn't reporting the temperature. Each cargo run waits for the CPU to
drop below 78 C first, and when the guard refuses all the same (other builds heat it back up in the
second between), waits and tries again, up to 5 times. The guard is this PC's own: "guard" in
~\.config\lbterm\dev.json names it (and "thermalStop" the file it writes at 91 C). Without one a
build refuses to start; -NoGuard builds without it.

publish makes the public repo's next commit in its clone (-Public, else LBTerm-public beside the
main checkout): main's tree without .claude\ and CLAUDE.md, on top of the last export, with
WezTerm's history under it and never the private repo's (Publish-Lbterm says how). The clone's
hooks run lbterm\publish\leak-check.sh on the commit, its message, and every push. Its message
lists the merges into main since the last export unless -Message gives one. -Push pushes the
clone's main to its origin.

Release builds here are incremental and linked by rust-lld, and OpenSSL is built once per version
and kept (Invoke-Cargo says why). When sccache is installed, cargo runs through it (RUSTC_WRAPPER)
and its server is started outside Claude's job; -NoSccache turns it off for a run (Use-Sccache).
Run cargo through this script: a cargo run without the same settings builds a second set of
everything beside this one.
#>
param(
  [Parameter(Position = 0)]
  [ValidateSet('build', 'check', 'test', 'install', 'rollback', 'start', 'restart', 'try', 'upstream', 'publish', 'logs', 'e2e')]
  [string]$Command = 'build',
  # test: the crates whose tests run
  [string[]]$Package,
  [switch]$NoGuard,
  # cargo without sccache, though it's installed
  [switch]$NoSccache,
  [int]$Jobs = 4,
  # logs: one process's log ($PID is PowerShell's own, so the variable has another name)
  [Alias('Pid')][int]$ProcessId,
  # logs: the newest log, printed; with -Follow, followed as it's written
  [switch]$Latest,
  [switch]$Follow,
  # logs: ERROR and WARN lines grouped by kind; the slow-frame lines grouped by kind and site
  [switch]$Errors,
  [switch]$Slow,
  # logs: only the daily LBTerm's, try windows', test windows', or those that say nothing of which
  [ValidateSet('daily', 'try', 'test', 'unknown')][string]$Kind,
  # logs: only what was written since then: 18:00, 2026-10-21 18:00, 30m, 2h, 1d
  [string]$Since,
  # e2e: the scripts to run (names, or a script's name for it and its variants), else all
  [string[]]$Only,
  # e2e: only the scripts whose Covers lines match what changed since main
  [switch]$Changed,
  # e2e: the build the test windows run, else target\release
  [string]$Bin,
  # e2e: how many times a failure runs again
  [int]$Retry = 1,
  # e2e: how long one script may run before it's stopped
  [int]$TimeoutMin = 10,
  # e2e: each test window waits until the keyboard and mouse have been idle for 3 s
  [switch]$WaitIdle,
  # publish: the public repo's clone, else LBTerm-public beside the main checkout
  [string]$Public,
  # publish: the export's commit message, else the merges since the last one
  [string]$Message,
  # publish: push the clone's main to its origin
  [switch]$Push
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lbterm-lib.ps1')

$repo    = Split-Path $PSScriptRoot -Parent
$release = Join-Path $repo 'target\release'
# The main checkout, which a worktree shares its OpenSSL with
$main    = [IO.Path]::GetFullPath((Split-Path (git -C $repo rev-parse --path-format=absolute --git-common-dir) -Parent))
$install = Join-Path $env:LOCALAPPDATA 'Programs\LBTerm'
# The build the last install replaced, which rollback swaps back in
$previous = Join-Path $env:LOCALAPPDATA 'Programs\LBTerm-previous'
# This PC's own paths, which the repo doesn't name: the thermal guard, and the file it writes at
# 91 C, after which it refuses everything until Lachlan deletes it
$devFile = Join-Path $HOME '.config\lbterm\dev.json'
$dev     = Get-DevSettings -Path $devFile
$guard   = $dev.guard
$thermalStop = $dev.thermalStop
# How many times a cargo run is started before the guard's refusals count as a failure
$guardAttempts = 5
$pwsh    = 'C:\Program Files\PowerShell\7\pwsh.exe'
$bash    = 'C:\Program Files\Git\bin\bash.exe'
# The vendored OpenSSL build needs a Windows perl. Git's MSYS perl writes paths nmake can't read.
$perl    = 'C:\Strawberry\perl\bin'

$exes   = $LbtermExes
$extras = $LbtermExtras
# The mux domain's socket (lbterm/MUX.md): lbterm.lua leaves socket_path at its default
$muxSock = Join-Path $env:USERPROFILE '.local\share\lbterm\sock'

function Get-Installed([string[]]$names) {
  @(Get-Process $names -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$install\*" })
}

# A GUI that can't attach to the running server would leave its tabs out of reach. The codec
# version only changes with an upstream merge that changes the protocol; $cli is the lbterm.exe
# of the build about to be put in.
function Assert-ServerReachable([string]$cli, [string]$what) {
  $server = Get-Installed lbterm-mux-server
  if (-not $server) { return }
  $env:WEZTERM_UNIX_SOCKET = $muxSock
  $out = & $cli cli --no-auto-start list 2>&1 | Out-String
  if ($out -match 'codec version') {
    throw "The running lbterm-mux-server (pid $($server.Id -join ', ')) speaks another protocol than $what, so its GUI couldn't reach the tabs. Close LBTerm, which ends them, then try again.`n$out"
  }
}

# Ends the GUI and opens the installed one. Killing the GUI is what a crash does: panes in the mux
# server live on, and the new GUI attaches to them. Local panes would die with it, so only with a
# server running.
function Restart-Gui {
  $gui = Join-Path $install 'lbterm-gui.exe'
  if (-not (Test-Path $gui)) { throw "LBTerm isn't installed: run install first" }
  if (-not (Get-Installed lbterm-mux-server)) {
    throw "No lbterm-mux-server is running, so LBTerm's tabs live in the GUI and would end with it. Close and open LBTerm yourself."
  }
  $cli = Join-Path $install 'lbterm.exe'
  $env:WEZTERM_UNIX_SOCKET = $muxSock
  $inServer = @(& $cli cli --no-auto-start list --format json | ConvertFrom-Json).Count
  foreach ($p in Get-Installed lbterm-gui) {
    # A GUI started without the mux domain has panes of its own, and lists more than the server
    $env:WEZTERM_UNIX_SOCKET = Join-Path $env:USERPROFILE ".local\share\lbterm\gui-sock-$($p.Id)"
    $inGui = @(& $cli cli --no-auto-start list --format json | ConvertFrom-Json).Count
    if ($LASTEXITCODE -or $inGui -gt $inServer) {
      throw "LBTerm (pid $($p.Id)) has tabs of its own, outside the mux server, which would end with it. Close and open it yourself."
    }
  }
  foreach ($p in Get-Installed lbterm-gui) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(5000) | Out-Null }
  explorer.exe $gui
  'restarted LBTerm: its tabs come back from the mux server'
}

# sccache's server, running before cargo starts. Started here, outside Claude's job, with
# Start-Unfocused (WMI, no console): a server cargo's first client starts is that client's child,
# so inside the warden's job, and it dies with the job while other builds still use it.
function Start-SccacheServer([string]$Exe) {
  $port = if ($env:SCCACHE_SERVER_PORT -match '^\d+$') { [int]$env:SCCACHE_SERVER_PORT } else { 4226 }
  if (Test-SccacheServer $port) { return "sccache: using its server on port $port" }
  # The client's settings stay out of the server's environment, which Start-Unfocused copies
  foreach ($k in @((Get-SccacheEnvironment).Keys) + 'RUSTC_WRAPPER') { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
  . (Join-Path $PSScriptRoot 'motion\start-unfocused.ps1')
  $id = Start-Unfocused -Exe $Exe -NoConsole -WorkingDirectory (Split-Path $Exe) -Environment ([hashtable](Get-SccacheEnvironment -Server))
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not (Test-SccacheServer $port) -and $clock.Elapsed.TotalSeconds -lt 15) { Start-Sleep -Milliseconds 200 }
  $alive = [bool](Get-Process -Id $id -ErrorAction SilentlyContinue)
  $listening = Test-SccacheServer $port
  if ($alive -and $listening) {
    "sccache: started its server outside this job (pid $id, port $port)"
  } elseif ($listening) {
    # Another build's client started one in between, so this one found the port taken
    "sccache: its server (pid $id) exited; using the one another build started on port $port"
  } else {
    "sccache: its server (pid $id) isn't listening on port $port after 15 s; cargo's first client will start one inside this job"
  }
}

# sccache, when it's installed (Find-Sccache) and -NoSccache isn't given: cargo runs rustc through it
# (RUSTC_WRAPPER), which caches the registry's crates. LBTerm's own crates build incrementally,
# which it can't cache, so it hands them straight to rustc; bin and proc-macro crates (build scripts
# too) link, so neither can those. Cargo's fingerprint doesn't include RUSTC_WRAPPER, and the build
# scripts that read it don't rerun for it, so switching sccache on or off rebuilds nothing.
#
# C isn't cached. cc-rs hands a compile to RUSTC_WRAPPER only when CC or CXX names the compiler,
# which also needs vcvars64's INCLUDE and LIB, and cc-rs reruns its build scripts when those change:
# 53 crates and 5.5 minutes once in a tree built without them. Tried on 2026-10-22: sccache's
# preprocessor cache works for GCC and Clang only, so an MSVC hit still runs cl /E, and with
# windows.h in most of the sources a fresh worktree's C took as long from the cache (613 s across
# the crates) as compiled (567 s). Only harfbuzz, one large C++ unit, gained (27 s to 1 s).
function Use-Sccache {
  Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
  if ($NoSccache) { return }
  $sccache = Find-Sccache
  if (-not $sccache) { return }
  Start-SccacheServer $sccache
  foreach ($e in (Get-SccacheEnvironment).GetEnumerator()) { Set-Item "Env:$($e.Key)" $e.Value }
  $env:RUSTC_WRAPPER = $sccache
}

function Invoke-Cargo([string[]]$cargoArgs) {
  if (-not (Test-Path "$perl\perl.exe")) { throw "No Strawberry Perl at $perl (winget install StrawberryPerl.StrawberryPerl)" }
  $env:PATH = "$perl;$env:PATH"
  # The guard runs cargo under Git's bash, which can put its MSYS perl back in front on PATH (it did
  # when lbterm.ps1 was started from a Git Bash shell). openssl-src takes this over PATH.
  $env:OPENSSL_SRC_PERL = "$perl\perl.exe"
  # Incremental release builds, linked by LLVM's lld (it comes with rustc). Measured 2026-10-20: a
  # one-line change to wezterm-gui went from 2 m 06 s to 30 s, one to term (which rebuilds the ~30
  # crates above it) from about 5.5 minutes to 34 s, and cli-list.ps1 timed the result the same.
  # Through the release profile, not CARGO_INCREMENTAL: sccache (RUSTC_WRAPPER) refuses to run at
  # all while CARGO_INCREMENTAL is set. Cargo's profile, so its fingerprint, is the same either way;
  # sccache hands LBTerm's own crates (built incrementally) straight to rustc and caches the
  # registry's, which cargo never builds incrementally.
  Remove-Item Env:CARGO_INCREMENTAL, Env:CARGO_BUILD_INCREMENTAL -ErrorAction SilentlyContinue
  $env:CARGO_PROFILE_RELEASE_INCREMENTAL = 'true'
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'rust-lld.exe'
  Use-Sccache
  # OpenSSL, once per version. openssl-src's own build (one nmake job) was 12.5 of the 17 minutes
  # a build from nothing took, and every worktree repeated it. The first build of a version keeps
  # what it built in the main checkout's target\openssl\<version>; builds after link that instead,
  # since OPENSSL_NO_VENDOR wins over the openssl crate's vendored feature.
  $sslVersion = Get-OpensslSrcVersion (Join-Path $repo 'Cargo.lock')
  $sslCache = Join-Path $main "target\openssl\$sslVersion"
  $builds = Join-Path $repo 'target\release\build'
  $cached = $sslVersion -and (Save-OpensslCache -BuildDir $builds -Cache $sslCache -SrcVersion $sslVersion)
  if ($cached) {
    $env:OPENSSL_NO_VENDOR = '1'
    $env:OPENSSL_DIR = $sslCache
    $env:OPENSSL_STATIC = '1'
  }
  Push-Location $repo   # cargo reads .cargo/config.toml from the working directory
  try {
    if ($NoGuard) {
      & cargo @cargoArgs
      if ($LASTEXITCODE) { throw "cargo $($cargoArgs -join ' ') failed (exit $LASTEXITCODE)" }
    } elseif (-not $guard) {
      throw "No thermal guard: $devFile names none (`"guard`": the path of guarded.sh). Pass -NoGuard to build without one."
    } else {
      # The guard refuses to start at 80 C, and a build leaves the CPU there, so the check's test
      # run was refused straight after its build: each try waits for the CPU to cool first. With
      # other builds running it can climb back past 80 C in the second before the guard reads it
      # (five builds failed that way on 2026-10-21 and 22), so a refusal waits and tries again.
      $cputemp = Join-Path (Split-Path $guard) 'cputemp.ps1'
      $cool = { & $pwsh -NoProfile -File $cputemp -WaitBelow 78 -TimeoutMin 30 | Out-Null }.GetNewClosure()
      $target = Join-Path $repo 'target'
      New-Item -ItemType Directory -Force $target | Out-Null
      Invoke-Guarded -Bash $bash -Guard $guard -Command cargo -Arguments $cargoArgs `
        -Marker (Join-Path $target "lbterm-cargo-$PID.started") -StopFile $thermalStop -Attempts $guardAttempts -Wait $cool
    }
  } finally { Pop-Location }
  if ($sslVersion -and -not $cached -and (Save-OpensslCache -BuildDir $builds -Cache $sslCache -SrcVersion $sslVersion)) {
    "kept OpenSSL $sslVersion for the builds after this one: $sslCache"
  }
}

# stdout of an exe, read through a real pipe. PowerShell's own & neither captures nor waits for a
# GUI-subsystem exe like lbterm-gui.
function Get-Output([string]$exe, [string]$arg) {
  $psi = [Diagnostics.ProcessStartInfo]::new($exe, $arg)
  $psi.RedirectStandardOutput = $true
  $psi.UseShellExecute = $false
  $p = [Diagnostics.Process]::Start($psi)
  $out = $p.StandardOutput.ReadToEnd()
  $p.WaitForExit()
  $out.Trim()
}

function Build {
  Invoke-Cargo @('build', '--release', '-j', $Jobs,
    '-p', 'wezterm', '-p', 'wezterm-gui', '-p', 'wezterm-mux-server', '-p', 'strip-ansi-escapes')
}

# Where every LBTerm process writes its log (config's RUNTIME_DIR)
$logDir = Join-Path $env:USERPROFILE '.local\share\lbterm'

# The LBTerm processes running now, by pid, as Get-LbtermLogs takes them
function Get-LiveLbterm {
  $live = @{}
  foreach ($p in Get-Process lbterm, lbterm-gui, lbterm-mux-server -ErrorAction SilentlyContinue) {
    $started = try { $p.StartTime } catch { [datetime]::MinValue }
    $live[$p.Id] = @{ Name = $p.ProcessName; Path = $p.Path; StartTime = $started }
  }
  $live
}

function Get-LogHeader($log) {
  $state = if ($log.Alive) { 'running' } else { 'gone' }
  "$($log.Process) $($log.Pid): $($log.Kind), $state ($($log.Reason)); started $($log.Started.ToString('yyyy-MM-dd HH:mm:ss')), last written $($log.Written.ToString('yyyy-MM-dd HH:mm:ss')), $(Format-LbtermLogSize $log.Size), $($log.Errors) errors, $($log.Warnings) warnings"
  $log.Path
}

# One log, its lines since $From
function Show-Log($log, $From) {
  Get-LogHeader $log
  ''
  foreach ($e in $log.Entries) { if ($null -eq $From -or $e.Time -ge $From) { $e.Lines } }
}

# Follows a log from its last 20 lines as its process writes it, until the process exits (or Ctrl+C)
function Watch-Log($log) {
  Get-LogHeader $log
  ''
  $stream = [IO.File]::Open($log.Path, 'Open', 'Read', [IO.FileShare]'ReadWrite, Delete')
  try {
    $reader = [IO.StreamReader]::new($stream)
    @($reader.ReadToEnd() -split '\r?\n' | Where-Object { $_ }) | Select-Object -Last 20
    # Found by name: an install renames a running exe aside, and Get-Process -Id then names the
    # process after the renamed file (lbterm-mux-server.exe.20261020220512.old)
    $proc = if ($log.Alive) { Get-Process -Name $log.Exe -ErrorAction SilentlyContinue | Where-Object Id -eq $log.Pid }
    if (-not $proc) { "-- $($log.Process) $($log.Pid) has gone: nothing more will come"; return }
    "-- following $($log.Process) $($log.Pid) until it exits; Ctrl+C stops"
    $partial = ''
    while ($true) {
      $chunk = $reader.ReadToEnd()
      if ($chunk) {
        # Only whole lines: the rest waits for its newline
        $parts = ($partial + $chunk) -split '\r?\n'
        $partial = $parts[-1]
        $parts | Select-Object -SkipLast 1
        continue
      }
      if ($proc.HasExited) {
        if ($partial) { $partial }
        "-- $($log.Process) $($log.Pid) has exited"
        break
      }
      Start-Sleep -Milliseconds 250
    }
  } finally { $stream.Dispose() }
}

switch ($Command) {
  'build' { Build }

  'check' {
    # lbterm-lib.ps1, which this script's own file handling is, in a fresh shell so nothing
    # loaded here leaks in
    & 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -Command @"
Import-Module Pester -MinimumVersion 5.0
`$c = New-PesterConfiguration
`$c.Run.Path = '$((Join-Path $PSScriptRoot 'tests') -replace "'", "''")'
`$c.Run.Exit = `$true
`$c.Output.Verbosity = 'Normal'
Invoke-Pester -Configuration `$c
"@
    if ($LASTEXITCODE) { throw "Pester: lbterm\tests failed" }

    Build
    # config/src/lbterm.rs and wezterm/tests/lbterm.rs, plus the rest of those two crates' tests,
    # lbterm-motion (springs, curves, DComp cubics, the pane layout planner) and lbterm-panel
    # (the tab panel's rows, hover timing and layout), the parser and terminal, which hold OSC
    # 7501 (program_status.rs in each), mux (localpane.rs: process lists refreshed off the GUI
    # thread), lbterm-extras (the smaller additions' logic: taskbar progress, scrollbar marks and hiding,
    # the slow-frame log), lbterm-triggers (rules, colours, what's new, the hearts), lbterm-notify
    # (OSC 99), codec (OSC 7501 records crossing from the mux server, a PDU zstd can't compress
    # going uncompressed) and promise (a task named by
    # where it was spawned, tests\lbterm_spawn_site.rs)
    Invoke-Cargo @('test', '--release', '-j', $Jobs, '-p', 'config', '-p', 'wezterm', '-p', 'lbterm-motion', '-p', 'lbterm-panel',
      '-p', 'wezterm-escape-parser', '-p', 'wezterm-term', '-p', 'mux', '-p', 'lbterm-extras', '-p', 'lbterm-triggers',
      '-p', 'lbterm-notify', '-p', 'codec', '-p', 'promise')

    # The exes cargo test doesn't run carry LBTerm's names
    $fail = [Collections.Generic.List[string]]::new()
    foreach ($f in $exes + $extras) {
      if (-not (Test-Path (Join-Path $release $f))) { $fail.Add("missing target\release\$f") }
    }
    # Test and demo windows open with Start-Unfocused (motion\start-unfocused.ps1): Explorer runs
    # a .cmd in a console that comes up in front and takes the keyboard. Only start and restart
    # here use Explorer, to open the real LBTerm.
    foreach ($s in Get-ChildItem $PSScriptRoot -Recurse -Include *.ps1, *.psm1) {
      if ($s.FullName -eq $PSCommandPath) { continue }
      $ast = [System.Management.Automation.Language.Parser]::ParseFile($s.FullName, [ref]$null, [ref]$null)
      $calls = $ast.FindAll({ param($a) $a -is [System.Management.Automation.Language.CommandAst] -and $a.GetCommandName() -match '^explorer(\.exe)?$' }, $true)
      foreach ($c in $calls) { $fail.Add("$($s.Name):$($c.Extent.StartLineNumber) opens a window through Explorer: use Start-Unfocused") }
    }
    $gui = Join-Path $release 'lbterm-gui.exe'
    if (Test-Path $gui) {
      # Only the name: upstream's GUI parses its arguments before env_bootstrap assigns the
      # version, so its --version says "someone forgot to call assign_version_info" (WezTerm too).
      $v = Get-Output $gui '--version'
      if ($v -notmatch '^lbterm-gui ') { $fail.Add("lbterm-gui --version printed '$v'") }
      $info = (Get-Item $gui).VersionInfo
      if ($info.ProductName -ne 'LBTerm') { $fail.Add("lbterm-gui.exe ProductName is '$($info.ProductName)'") }
      if ($info.LegalCopyright -notmatch 'Wez Furlong') { $fail.Add("lbterm-gui.exe lost Wez Furlong's copyright notice") }
    }
    $mux = Join-Path $release 'lbterm-mux-server.exe'
    if (Test-Path $mux) {
      $v = Get-Output $mux '--version'
      if ($v -notmatch '^lbterm-mux-server ') { $fail.Add("lbterm-mux-server --version printed '$v'") }
    }
    # The version names the commit just built (wezterm-version/build.rs). A stale one makes
    # install take the build for the one it replaces, keep no previous, and show no restart pill
    $head = (git -C $repo -c core.abbrev=8 show -s --format=%h HEAD).Trim()
    $built = Get-BuildVersion $release
    if (-not (Test-VersionNamesCommit $built $head)) {
      $fail.Add("lbterm --version says '$built', not HEAD's ${head}: the version stamp is stale")
    }
    if ($fail.Count) { $fail | ForEach-Object { "FAIL $_" }; exit 1 }
    'check passed'
  }

  'test' {
    # A crate's tests on their own, while working on it: the check runs them all
    if (-not $Package) { throw 'test needs -Package <crate>[,<crate>...]' }
    # pwsh -File hands a list over as one string: lbterm-extras,config
    Invoke-Cargo (@('test', '--release', '-j', $Jobs) + @($Package -split ',' | Where-Object { $_.Trim() } | ForEach-Object { '-p', $_.Trim() }))
  }

  'install' {
    foreach ($f in $exes + $extras) {
      if (-not (Test-Path (Join-Path $release $f))) { throw "target\release\$f is missing: run build first" }
    }
    $server = Get-Installed lbterm-mux-server
    Assert-ServerReachable (Join-Path $release 'lbterm.exe') 'this build'
    Remove-OldBuilds $install
    $version = Get-BuildVersion $release
    if (Save-Previous -Install $install -Previous $previous -NewVersion $version) {
      "kept the build this replaces ($(Get-BuildVersion $previous)) in ${previous}: lbterm.ps1 rollback goes back to it"
    }
    $moved = Copy-Build -From $release -To $install
    Set-BuildVersion $install $version
    "installed lbterm $version to $install"
    if ($moved) {
      if (Get-Installed lbterm-gui) { 'LBTerm is running the old build until it restarts: lbterm.ps1 restart keeps its tabs when the mux domain is on.' }
      if ($server) { "lbterm-mux-server keeps the old build until its last tab closes." }
    }
  }

  'rollback' {
    if (-not (Test-Path (Join-Path $previous 'lbterm-gui.exe'))) {
      throw "No earlier build is kept in $previous. Install keeps the build it replaces, from the first install after 2026-10-20 on."
    }
    Assert-ServerReachable (Join-Path $previous 'lbterm.exe') 'the kept build'
    Remove-OldBuilds $install
    $from = Get-BuildVersion $install
    $to = Get-BuildVersion $previous
    Switch-Build -Install $install -Previous $previous | Out-Null
    "rolled back from lbterm $from to lbterm $to; lbterm.ps1 rollback again goes forward"
    if (Get-Installed lbterm-gui) {
      try { Restart-Gui } catch { "LBTerm isn't restarted: $($_.Exception.Message)" }
    }
  }

  'restart' { Restart-Gui }

  'try' {
    # This tree's build, in a window beside the daily LBTerm: Lachlan's config, but its own class,
    # its own mux server and socket (New-TryConfig), LBTERM_TRY=1 so Terminal's hooks name its
    # panes' files apart (their ids collide with the daily server's), on the right-hand screen,
    # opened without the keyboard. It runs from a copy in target\try\bin, so builds can still
    # write target\release while it's open.
    Build
    $try = Join-Path $repo 'target\try'
    $bin = Join-Path $try 'bin'
    # The try window this replaces, with its server and their tabs
    $old = @(Get-Process lbterm-gui, lbterm-mux-server -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$bin\*" })
    foreach ($p in $old) { Stop-Process -Id $p.Id -Force; $p.WaitForExit(5000) | Out-Null }
    if ($old) { 'closed the try window this replaces, and its tabs' }
    Remove-OldBuilds $bin
    Copy-Build -From $release -To $bin | Out-Null
    $sock = Join-Path $try 'sock'
    if (Test-Path $sock) { Remove-Item $sock }
    $cfg = Join-Path $try 'try.lua'
    [IO.File]::WriteAllText($cfg, (New-TryConfig -UserConfig (Join-Path $env:USERPROFILE '.lbterm.lua') `
      -Self $cfg -Socket $sock -Server (Join-Path $bin 'lbterm-mux-server.exe')))
    . (Join-Path $PSScriptRoot 'motion\offscreen.ps1')
    $id = Start-Unfocused -Exe (Join-Path $bin 'lbterm-gui.exe') `
      -Arguments "--config-file `"$cfg`" connect try --class $LbtermTryClass --position $TestWindowPosition" `
      -Environment @{ LBTERM_TRY = '1' } -WorkingDirectory $env:USERPROFILE
    if ((Wait-Window -Id $id -Seconds 20) -eq [IntPtr]::Zero) {
      throw "The try window (pid $id) didn't open. Its log: lbterm.ps1 logs -Pid $id ($logDir\lbterm-gui.exe-log-$id.txt)"
    }
    "opened lbterm $(Get-BuildVersion $release) in a try window (pid $id) on the right-hand screen: click it to type. Closing it ends its tabs and its server; the next try replaces it."
  }

  'start' {
    $gui = Join-Path $install 'lbterm-gui.exe'
    if (-not (Test-Path $gui)) { throw "LBTerm isn't installed: run install first" }
    # Through Explorer: a GUI started as a child of Claude's sandboxed shell inherits its job object.
    explorer.exe $gui
  }

  'e2e' {
    # On demand only, never part of the check (Lachlan's choice): test windows take minutes
    Import-Module (Join-Path $PSScriptRoot 'harness.psm1') -Force -DisableNameChecking
    if (-not $Bin) { $Bin = $release }
    Invoke-E2eRun -Bin $Bin -Only $Only -Changed:$Changed -Retry $Retry -TimeoutMin $TimeoutMin -WaitIdle:$WaitIdle
    $report = Get-E2eLastReport
    if ($report -and $report.summary.fail) { exit 1 }
  }

  'upstream' {
    git -C $repo fetch -q upstream
    if ($LASTEXITCODE) { throw 'git fetch upstream failed' }
    $ahead, $behind = (git -C $repo rev-list --left-right --count main...upstream/main) -split '\s+'
    "main: $ahead commits of LBTerm's own, $behind upstream commits not merged yet"
  }

  'publish' {
    $clone = if ($Public) { $Public } else { Join-Path (Split-Path $main -Parent) 'LBTerm-public' }
    $new = Publish-Lbterm -Repo $repo -Public $clone -Message $Message
    if ($new) { "exported main as $($new.Substring(0, 9)) in $clone" } else { "$clone already has main as it is" }
    if ($Push) {
      git -C $clone push -u origin main
      if ($LASTEXITCODE) { throw "git push from $clone failed" }
    } else {
      $unpushed = @(git -C $clone rev-list main --not --remotes=origin --remotes=upstream 2>$null).Count
      if ($unpushed) { "$unpushed commit(s) not pushed yet: publish -Push sends them" }
    }
  }

  'logs' {
    # Reads the logs, never changes them: a GUI starting deletes those a week old
    $from = if ($Since) { ConvertTo-LbtermSince $Since }
    $all = @(Get-LbtermLogs -Dir $logDir -Install $install -Live (Get-LiveLbterm))
    if ($ProcessId) {
      $logs = @($all | Where-Object Pid -eq $ProcessId)
      if (-not $logs) { throw "No log of pid $ProcessId in $logDir" }
    } else {
      $logs = @(Select-LbtermLogs $all -Kind $Kind -Since $from)
      if ($Latest -or $Follow) { $logs = @($logs | Select-Object -First 1) }
    }
    $kindWord = if ($Kind -and -not $ProcessId) { " $Kind" }
    $sinceWords = if ($from) { " written since $($from.ToString('yyyy-MM-dd HH:mm'))" }
    if (-not $logs) { "No$kindWord logs$sinceWords in $logDir"; break }
    $what = "$($logs.Count)$kindWord log$(if ($logs.Count -ne 1) { 's' })$sinceWords$(if (-not $kindWord) { " ($(Format-LbtermLogKinds $logs))" })"

    if ($Errors) {
      $groups = @(Group-LbtermLogProblems $logs -Since $from)
      "ERROR and WARN lines in $what, grouped by kind, most often first"
      ''
      if ($groups) { Format-LbtermLogProblems $groups } else { 'None.' }
      $slowLines = (@(Group-LbtermSlowFrames $logs -Since $from) | Measure-Object Lines -Sum).Sum
      if ($slowLines) { ''; "Left out: $slowLines slow-frame lines (-Slow)." }
    }
    if ($Slow) {
      $groups = @(Group-LbtermSlowFrames $logs -Since $from)
      if ($Errors) { '' }
      "Slow frames ($LbtermSlowTarget) in $what, by kind and site, the most first"
      ''
      if ($groups) { Format-LbtermSlowFrames $groups } else { 'None.' }
    }
    if ($Errors -or $Slow) { break }

    if ($Follow) { Watch-Log $logs[0] }
    elseif ($ProcessId -or $Latest) {
      $first = $true
      foreach ($log in $logs) { if (-not $first) { '' }; $first = $false; Show-Log $log $from }
    } else {
      # Newest first; all of them only when -Since says how far back
      $shown = if ($from) { $logs } else { @($logs | Select-Object -First 30) }
      Format-LbtermLogList $shown
      if ($shown.Count -lt $logs.Count) { ''; "The newest $($shown.Count) of $what. -Since 1d (or a time) lists all since then, -Kind one kind." }
    }
  }
}
