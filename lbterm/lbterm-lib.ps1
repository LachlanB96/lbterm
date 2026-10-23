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

# ---- This PC's own paths (~\.config\lbterm\dev.json) ----

# The settings in $Path, a JSON file of this PC's own, so the repo names none of its paths:
#   guard        the thermal guard cargo runs under (guarded.sh, with cputemp.ps1 beside it)
#   thermalStop  the file the guard writes at 91 C, after which it refuses everything
#   config       the lbterm.lua test windows look like (New-TestConfig, motion/offscreen.ps1)
# Each one missing (or the file) is $null. The guard comes back with slashes, since it runs under
# Git's bash, whose dirname doesn't split at backslashes.
function Get-DevSettings([Parameter(Mandatory)][string]$Path) {
  $json = if (Test-Path -LiteralPath $Path) { Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json }
  [pscustomobject]@{
    guard       = if ($json.guard) { $json.guard -replace '\\', '/' }
    thermalStop = $json.thermalStop
    config      = $json.config
  }
}

# ---- Cargo under the thermal guard (lbterm.ps1's Invoke-Cargo) ----

# What guarded.sh (the thermal guard) is told to run for $Command: Git's bash, writing
# $Marker and then handing over to $Command with exec. The guard reads the CPU before it starts
# anything and refuses with exit 1, so the marker is what tells its refusal from $Command's own
# exit 1 (cargo exits 1 on a bad argument).
function Get-GuardedCommand {
  param(
    [Parameter(Mandatory)][string]$Marker,
    [Parameter(Mandatory)][string]$Command,
    [string[]]$Arguments = @()
  )
  @('bash', '-c', ': > "$0" && exec "$@"', ($Marker -replace '\\', '/'), $Command) + @($Arguments)
}

# What to do after one guarded run that exited with $ExitCode, $Started saying whether the command
# got going (its marker), on attempt $Attempt of $Attempts:
#   done     it ran and passed
#   failed   it ran and failed, or something other than the guard's refusal went wrong: never again
#   stop     the guard refused for THERMAL-STOP ($ThermalStop), which stays until Lachlan deletes it
#   retry    the guard refused (the CPU at 80 C or more, or HWiNFO not reporting): wait, run again
#   refused  the guard refused on the last attempt
function Get-GuardOutcome {
  param(
    [Parameter(Mandatory)][int]$ExitCode,
    [Parameter(Mandatory)][bool]$Started,
    [bool]$ThermalStop,
    [Parameter(Mandatory)][int]$Attempt,
    [Parameter(Mandatory)][int]$Attempts
  )
  if ($ExitCode -eq 0) { return 'done' }
  if ($Started -or $ExitCode -ne 1) { return 'failed' }
  if ($ThermalStop) { return 'stop' }
  if ($Attempt -lt $Attempts) { 'retry' } else { 'refused' }
}

# Runs $Command under the thermal guard ($Guard, guarded.sh, through Git's $Bash), and again when
# the guard refuses to start it, up to $Attempts runs in all. $Wait runs before each one
# (lbterm.ps1 waits there for the CPU to cool below 78 C). With other builds running, the CPU
# can climb back past 80 C between that wait and the guard's own reading a second later, and the
# guard refuses. $Marker is the file Get-GuardedCommand writes as the command starts; $StopFile
# is the guard's THERMAL-STOP. Throws unless the command ran and passed.
function Invoke-Guarded {
  param(
    [Parameter(Mandatory)][string]$Bash,
    [Parameter(Mandatory)][string]$Guard,
    [Parameter(Mandatory)][string]$Command,
    [string[]]$Arguments = @(),
    [Parameter(Mandatory)][string]$Marker,
    [string]$StopFile,
    [ValidateRange(1, 100)][int]$Attempts = 5,
    [scriptblock]$Wait = {}
  )
  $what = (@($Command) + @($Arguments)) -join ' '
  for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
    & $Wait
    Remove-Item -LiteralPath $Marker -Force -ErrorAction SilentlyContinue
    & $Bash $Guard @(Get-GuardedCommand -Marker $Marker -Command $Command -Arguments $Arguments)
    $code = $LASTEXITCODE
    $started = Test-Path -LiteralPath $Marker
    Remove-Item -LiteralPath $Marker -Force -ErrorAction SilentlyContinue
    $stop = [bool]$StopFile -and (Test-Path -LiteralPath $StopFile)
    switch (Get-GuardOutcome -ExitCode $code -Started $started -ThermalStop $stop -Attempt $attempt -Attempts $Attempts) {
      'done' { return }
      'failed' { throw "$what failed (exit $code)" }
      'stop' {
        $why = (Get-Content -LiteralPath $StopFile -Raw -ErrorAction SilentlyContinue) -replace '\s+$', ''
        throw "The thermal guard refused to start $what because $StopFile is there ($why). Nothing builds until Lachlan deletes it."
      }
      'refused' { throw "The thermal guard refused to start $what $Attempts times: the CPU kept reaching 80 C before it could start (or HWiNFO stopped reporting)" }
      'retry' { "lbterm.ps1: the thermal guard refused to start cargo (try $attempt of $Attempts); waiting for the CPU to cool, then trying again" }
    }
  }
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
# first. Slow-frame lines are left to Group-LbtermSlowFrames.
function Group-LbtermLogProblems {
  param([object[]]$Logs, [Nullable[datetime]]$Since)
  $groups = @{}
  $keys = @{}
  $seen = [Collections.Generic.HashSet[string]]::new()
  foreach ($log in $Logs) {
    foreach ($e in $log.Entries) {
      if ($e.Level -ne 'ERROR' -and $e.Level -ne 'WARN') { continue }
      if ($e.Target -eq $LbtermSlowTarget) { continue }
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
# Lachlan's other projects
$PublishExcludes = '.claude', 'CLAUDE.md'

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
