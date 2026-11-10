#Requires -Version 7
<#
LBTerm against Windows Terminal: startup, throughput, CPU and memory, everything that needs no
focus (key-to-screen is key-latency.ps1's, run only with nobody at the keyboard). What is measured
and how to read it is lbterm/BENCH.md.

Three terminals, interleaved in every round in this order: LBTerm on its local domain (lbterm),
LBTerm on a mux domain of the benchmark's own, every tab in lbterm-mux-server (lbterm-mux), and
Windows Terminal (wt). LBTerm runs the benchmark's config (New-BenchConfig: Cascadia Mono 12 pt,
-Cols x -Rows, -Scrollback lines, WebGpu); Windows Terminal its own settings with the PowerShell
profile (-WtProfile), Cascadia Mono 12 pt by default, opened at -Cols x -Rows. Every tab runs
pwsh 7 with -NoProfile. Upstream WezTerm ran too until 2026-11-04, when it was uninstalled: its
numbers from earlier runs stay in BENCH.md as the baseline.

Phases (-Phase, in this order, each its own rounds through the three):
  smoke       one window of each: its placement, the foreground, the columns and rows the console
              reports, a capture (smoke-<kind>.png), a small throughput run
  startup     -StartupRounds launches of each (a warm-up launch of each first, not counted): from
              the launch to the window showing, to the tab's program being created, and to it
              running (ready.ps1 writes the time)
  throughput  -Rounds windows of each, the producer (producer.ps1) writing -SizeMB MB of each
              pattern (plain, wrap, sgr, unicode, vte) in turn: MB/s and lines/s, the terminal's CPU
              while it writes and after (the tail, until it's quiet)
  idle        -Rounds of each: 1 tab of an idle shell at rest for -RestSec, 8 tabs the same, and
              -ScrollLines lines of scrollback; CPU at rest and memory each time (and LBTerm again
              with scrollback_lines = -ScrollLines, which Windows Terminal can't be given without
              editing its settings)

Each window opens unfocused (Start-Unfocused) on the right-most screen. Every launch is checked: the
window on that screen alone, the foreground never one of its processes (a watcher every 20 ms
from launch to close). A terminal that breaks either is closed at once and not used again in the
run (a FAIL in result.json, no retry). wsl.exe and its kin starting at any point are noted.

Results: <Out>\results.jsonl, one line a measurement, and env.json (versions, the screen, what
else was running); summarize.ps1 makes the tables. <Out> is target\bench\<time> unless -Out says.

Run it through the warden on Lachlan's PC, so it queues with builds:
  warden-run --name bench --cpus 4 --mem 8 --max 90m -- "C:/Program Files/PowerShell/7/pwsh.exe" -NoProfile -File lbterm/bench/bench.ps1 -Work <scratch folder>
#>
param(
  # Lists take commas too (pwsh -File passes "a,b" as one string)
  [string[]]$Phase = @('smoke', 'startup', 'throughput', 'idle'),
  [string[]]$Kinds = @('lbterm', 'lbterm-mux', 'wt'),
  [int]$Rounds = 3,
  [int]$StartupRounds = 8,
  [int]$SizeMB = 16,
  [string[]]$Patterns = @('plain', 'wrap', 'sgr', 'unicode', 'vte'),
  [int]$RestSec = 60,
  [int]$Cols = 100,
  [int]$Rows = 30,
  [int]$Scrollback = 9001,
  [int]$ScrollLines = 100000,
  # Where the copy of LBTerm, the data and the tabs' working folder go (a scratch folder)
  [Parameter(Mandatory)][string]$Work,
  # The LBTerm build to copy and measure (its top-level files): the installed one by default
  [string]$From = (Join-Path $env:LOCALAPPDATA 'Programs\LBTerm'),
  [string]$Out,
  [string]$WtProfile = 'PowerShell',
  # Checks what needs no window (the builds, configs and fonts, the screen, the data) and stops
  [switch]$DryRun,
  # Lachlan has said he's away from the keyboard: Windows Terminal, which takes the foreground as
  # it opens, may run
  [switch]$Away,
  # Lua table entries added to LBTerm's configs only (`lbterm_triggers = {},`), to try a lead, and
  # the name its results carry (summarize.ps1 shows it beside the terminal's)
  [string]$LbtermLua = '',
  [string]$Variant = ''
)
$ErrorActionPreference = 'Stop'
function Split-List([string[]]$Value, [string[]]$Allowed, [string]$Name) {
  $l = @($Value -split ',' | ForEach-Object Trim | Where-Object { $_ })
  foreach ($v in $l) { if ($v -notin $Allowed) { throw "-$Name $v`: not one of $($Allowed -join ', ')" } }
  $l
}
$Phase = Split-List $Phase @('smoke', 'startup', 'throughput', 'idle') 'Phase'
$Kinds = Split-List $Kinds @('lbterm', 'lbterm-mux', 'wt') 'Kinds'
$Patterns = Split-List $Patterns @('plain', 'wrap', 'sgr', 'unicode', 'vte') 'Patterns'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot 'bench-lib.ps1')

# This shell's terminal and Claude are nothing to the tabs: the cli's spawns pass its environment
# on, and a cli call without a socket of its own must find none (never the daily mux server's)
foreach ($v in @(Get-ChildItem Env: | Where-Object { $_.Name -match '^(WEZTERM_|TERM_PROGRAM|CLAUDE|ANTHROPIC_|LBTERM_)' })) { Remove-Item "Env:$($v.Name)" }

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $Out) { $Out = Join-Path $repo "target\bench\$(Get-Date -Format 'yyyyMMdd-HHmmss')" }
New-Item -ItemType Directory -Force $Out, $Work | Out-Null
$Out = (Resolve-Path $Out).Path; $Work = (Resolve-Path $Work).Path
$results = Join-Path $Out 'results.jsonl'
$log = Join-Path $Out 'bench.log'

function Write-Log([string]$Line) {
  $l = "$(Get-Date -Format 'HH:mm:ss.fff') $Line"
  Add-Content -LiteralPath $log $l
  $l | Out-Host
}
function Add-Result([hashtable]$R) {
  $R.at = (Get-Date).ToString('o')
  if ($Variant -and $R.kind -like 'lbterm*') { $R.variant = $Variant }
  Add-Content -LiteralPath $results ([pscustomobject]$R | ConvertTo-Json -Compress -Depth 8)
}

#region The build, the data, the configs

# A copy of the build (Copy-BenchBuild), never the installed folder itself
$bin = Copy-BenchBuild -From $From -To (Join-Path $Work 'lbterm')
$lbtermVersion = (& (Join-Path $bin 'lbterm.exe') --version) -join ' '

$data = Join-Path $Work 'data'
$cwd = Join-Path $Work 'cwd'
New-Item -ItemType Directory -Force $data, $cwd | Out-Null
$sizes = @{}
$target = [long]$SizeMB * 1MB
$want = [ordered]@{
  plain   = { [LbtermBench.Data]::Plain($args[0], $target, 80) }
  wrap    = { [LbtermBench.Data]::Plain($args[0], $target, 4 * $Cols - 20) }
  sgr     = { [LbtermBench.Data]::Sgr($args[0], $target, 80) }
  unicode = { [LbtermBench.Data]::Unicode($args[0], $target, 80) }
  vte     = { [LbtermBench.Data]::Vte($args[0], $target, $Cols, $Rows) }
  # -ScrollLines lines of 80 columns (82 bytes each)
  scroll  = { [LbtermBench.Data]::Plain($args[0], [long]$ScrollLines * 82 - 1, 80) }
}
$manifest = Join-Path $data 'manifest.json'
$key = "$SizeMB/$Cols/$Rows/$ScrollLines"
$old = if (Test-Path $manifest) { Get-Content -Raw $manifest | ConvertFrom-Json -AsHashtable } else { @{} }
foreach ($p in $want.Keys) {
  $file = Join-Path $data "$p.txt"
  if ($old.key -eq $key -and $old.files -and $old.files[$p] -and (Test-Path $file)) { $sizes[$p] = $old.files[$p]; continue }
  $r = & $want[$p] $file
  $sizes[$p] = @{ bytes = $r[0]; lines = $r[1] }
}
[IO.File]::WriteAllText($manifest, (@{ key = $key; files = $sizes } | ConvertTo-Json -Depth 4))
$dll = Join-Path $data 'BenchCon.dll'
if (-not (Test-Path $dll) -or (Get-Item $dll).LastWriteTimeUtc -lt (Get-Item (Join-Path $PSScriptRoot 'BenchCon.cs')).LastWriteTimeUtc) {
  Remove-Item $dll -ErrorAction SilentlyContinue
  Add-Type -TypeDefinition (Get-Content -Raw (Join-Path $PSScriptRoot 'BenchCon.cs')) -OutputAssembly $dll -OutputType Library
}

$launcher = Install-BenchLauncher -Data $data

# The harness's test: its output folder (target\bench\<run>\harness), the class its windows get
# (com.lachlanb.lbterm.perf-bench), the mux domain's short socket, its window check and its cleanup
$T = Start-Test 'bench' -Bin $bin -Out (Join-Path $Out 'harness')
$configDir = Join-Path $T.Out 'config'
$launches = Join-Path $T.Out 'launches'
New-Item -ItemType Directory -Force $configDir, $launches | Out-Null
$setup = [pscustomobject]@{
  Bin = $bin; Cwd = $cwd; Class = $T.Class; Socket = $T.Sock
  ConfigDir = $configDir; Launches = $launches; Launcher = $launcher
  Cols = $Cols; Rows = $Rows; Scrollback = $Scrollback; WtProfile = $WtProfile; Data = $data
  LbtermLua = $LbtermLua
}
$script:BenchSetup = $setup
$global:BenchPids = [Collections.Generic.HashSet[uint32]]::new()

#endregion

#region What it's run against

$wtPackage = Get-AppxPackage Microsoft.WindowsTerminal
$conpty = [ordered]@{}
foreach ($p in @(@('lbterm', $bin), @('wt', $wtPackage.InstallLocation))) {
  $oc = Join-Path $p[1] 'OpenConsole.exe'
  $conpty[$p[0]] = if (Test-Path $oc) { (Get-Item $oc).VersionInfo.FileVersion } else { 'none' }
}
# The font LBTerm's benchmark config resolves: Cascadia Mono, as Windows Terminal's, or the
# benchmark is unfair
$fonts = [ordered]@{}
$cfg = New-BenchConfig -Path (Join-Path $configDir 'fonts-lbterm.lua') -Cols $Cols -Rows $Rows -Scrollback $Scrollback -Cwd $cwd
$txt = (& (Join-Path $bin 'lbterm.exe') --config-file $cfg ls-fonts --text 'a' 2>&1 | Out-String)
$fonts.lbterm = (($txt -split "`n") | Where-Object { $_ -match '\.ttf|\.otf' } | Select-Object -First 1 | ForEach-Object Trim) -join ' | '
if ($txt -notmatch 'Cascadia Mono') { throw "LBTerm's benchmark config doesn't resolve Cascadia Mono: $txt" }
$screen = Get-BenchScreen
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$gpu = @(Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name) $($_.DriverVersion)" })
$envInfo = [ordered]@{
  started = (Get-Date).ToString('o')
  lbterm = $lbtermVersion; lbtermFrom = 'installed build, copied (top-level files)'
  wt ="$($wtPackage.Version)"
  conpty = $conpty
  fonts = $fonts
  pwsh = (Get-Item $script:BenchPwsh).VersionInfo.ProductVersion
  os = "$((Get-CimInstance Win32_OperatingSystem).Caption) $([Environment]::OSVersion.Version)"
  cpu = "$($cpu.Name), $($cpu.NumberOfCores) cores / $($cpu.NumberOfLogicalProcessors) threads, now $($cpu.CurrentClockSpeed) MHz (turbo off by Lachlan's choice since 2026-10-24)"
  gpu = $gpu
  screen = "$($screen.Width)x$($screen.Height) at $($screen.X),$($screen.Y)"
  settings = [ordered]@{ cols = $Cols; rows = $Rows; scrollback = $Scrollback; scrollLines = $ScrollLines; sizeMB = $SizeMB; restSec = $RestSec; rounds = $Rounds; startupRounds = $StartupRounds; wtProfile = $WtProfile; lbtermLua = $LbtermLua; variant = $Variant }
  data = $sizes
  load = Get-BenchLoad
}
[IO.File]::WriteAllText((Join-Path $Out 'env.json'), ($envInfo | ConvertTo-Json -Depth 6))
Write-Log "bench: LBTerm $lbtermVersion, Windows Terminal $($wtPackage.Version); ConPTY $($conpty.Values -join ' / '); fonts: $($fonts.Values -join ' ; ')"
Write-Log "bench: right-most screen $($envInfo.screen); out $Out"
if ($DryRun) {
  $p = Get-BenchPositions
  Write-Log "dry run: windows would open at --position $($p.Lbterm) (LBTerm) and --pos $($p.Wt) (Windows Terminal)"
  Write-Log "dry run: data $(($sizes.GetEnumerator() | ForEach-Object { "$($_.Key) $([Math]::Round($_.Value.bytes / 1MB, 1)) MB / $($_.Value.lines) lines" }) -join ', ')"
  Write-Log 'dry run: nothing opened'
  Remove-Item -LiteralPath $T.Out -Recurse -Force -ErrorAction SilentlyContinue
  exit 0
}

#endregion

#region One launch at a time

$banned = @{}
# With -Away a terminal may take the foreground (Windows Terminal does as it opens): nothing ends it
# for that. What ends the run instead is Lachlan coming back: any real key or mouse input while a
# terminal runs stops that one and every one after it, as key-latency.ps1 does (2026-10-31: the
# guard still ended Windows Terminal's window in an -Away run, so it was never measured)
if ($Away) { $script:BenchGuard = $false }
$script:Back = $null

# Opens a terminal, runs $Body with it, and closes it, whatever happens. A launch that broke the
# rules (the foreground, the screen) bans its kind for the rest of the run; so does finding it
# broke them while it ran, checked again before it closes. With -Away, real input while it ran
# ends the run.
function Use-Terminal {
  param([string]$Kind, [string[]]$Program, [object[]]$More = @(), [int]$Scrollback = 0, [scriptblock]$Body, [string]$What)
  if ($banned.ContainsKey($Kind) -or $script:Back) { return $null }
  Wait-Idle -Ms 1500 -TimeoutSec 30
  $term = $null
  $started = [Diagnostics.Stopwatch]::StartNew()
  try {
    $term = Start-BenchTerminal -Kind $Kind -Setup $setup -Program $Program -More $More -Scrollback $Scrollback
    if ($term.Violation) { throw $term.Violation }
    if ($term.Error) { throw $term.Error }
    $bodyResult = & $Body $term
    if ($Away) {
      $idle = [LbtermBench.Native]::IdleMs()
      if ($idle -lt $started.ElapsedMilliseconds) { $script:Back = "real input $idle ms ago, while $Kind ($What) ran: Lachlan is back, so the run stops here"; throw $script:Back }
    } else {
      $fgAfter = Test-BenchForeground $term
      if ($fgAfter) { throw $fgAfter }
    }
    $bodyResult
  } catch {
    # The foreground first: a measurement that failed because the watcher ended the window
    $fg = if ($term -and -not $Away) { Test-BenchForeground $term }
    $why = if ($fg) { $fg } else { $_.Exception.Message }
    $msg = "$Kind ($What): $why"
    if ($why -like 'CLICKED*') {
      # Lachlan's click: the window went at once and this measurement with it; not the terminal's doing
      $T.Notes.Add($msg)
      Write-Log "SKIPPED $msg"
    } elseif ($why -like 'VIOLATION*' -or $why -like 'its window is at*' -or $why -like 'the window that showed*' -or $why -like 'the Windows Terminal window showed*') {
      # The foreground taken, or the window somewhere but the right-most screen: closed, and this
      # kind of terminal isn't started again in the run
      $banned[$Kind] = $msg
      Add-Failure $msg
      Write-Log "STOP using $Kind`: $msg"
    } else {
      Add-Failure $msg
      Write-Log "ERROR $msg"
      # What the window showed and what ran, for working out why
      if ($term -and $term.Hwnd) {
        $shot = [LbtermE2E.Win]::CaptureWindow([IntPtr][long]$term.Hwnd)
        if ($shot) { $png = Join-Path $Out "error-$Kind-$(Get-Date -Format 'HHmmss').png"; $shot.Save($png); $shot.Dispose(); Write-Log "  its window: $png" }
        Write-Log "  its processes: $((@(Get-BenchProcesses $term $setup) | ForEach-Object { "$($_.Role):$($_.Exe):$($_.Pid)" }) -join ' ')"
      }
    }
    $null
  } finally {
    Stop-BenchTerminal $term $setup
    Start-Sleep -Milliseconds 500
  }
}

# Waits for ready.ps1's file and reads it: its time, its pid, the columns and rows it saw
function Read-Ready([string]$Path, [double]$TimeoutSec = 30) {
  if (-not (Wait-BenchFile $Path $TimeoutSec)) { throw "no $([IO.Path]::GetFileName($Path)) within $TimeoutSec s" }
  for ($i = 0; $i -lt 20; $i++) {
    $f = (Get-Content -Raw -LiteralPath $Path -ErrorAction SilentlyContinue)
    if ($f -and $f.Trim() -match '^(\d+) (\d+) (\d+) (\d+)$') { return [pscustomobject]@{ At = [long]$Matches[1]; Pid = [uint32]$Matches[2]; Cols = [int]$Matches[3]; Rows = [int]$Matches[4] } }
    Start-Sleep -Milliseconds 20
  }
  throw "couldn't read $Path"
}

function New-Ready([string]$Name, [switch]$Idle, [int]$Sleep = 0) {
  $path = Join-Path $Out "ready\$Name"
  New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
  Remove-Item $path -ErrorAction SilentlyContinue
  $prog = @($script:BenchPwsh, '-NoProfile')
  if ($Idle) { $prog += '-NoExit' }
  $prog += @('-File', (Join-Path $PSScriptRoot 'ready.ps1'), '-Out', $path)
  if ($Sleep) { $prog += @('-Sleep', "$Sleep") }
  [pscustomobject]@{ Path = $path; Program = $prog }
}

function New-Producer([string]$Name, [string[]]$Names) {
  $dir = Join-Path $Out "tp\$Name"
  if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
  New-Item -ItemType Directory -Force $dir | Out-Null
  [pscustomobject]@{ Dir = $dir; Program = @($script:BenchPwsh, '-NoProfile', '-File', (Join-Path $PSScriptRoot 'producer.ps1'), '-Dir', $dir, '-Data', $data, '-Patterns', ($Names -join ',')) }
}

# The producer's ready.json, once it has loaded its files
function Read-ProducerReady($P, [double]$TimeoutSec = 60) {
  $f = Join-Path $P.Dir 'ready.json'
  if (-not (Wait-BenchFile $f $TimeoutSec)) {
    $err = Join-Path $P.Dir 'error.txt'
    throw "the producer didn't start within $TimeoutSec s$(if (Test-Path $err) { ": $(Get-Content -Raw $err)" })"
  }
  Start-Sleep -Milliseconds 50
  Get-Content -Raw $f | ConvertFrom-Json
}

# One pattern through the producer: quiet first, then go (with the title marker it sets after
# the data), its done file, the marker in the window's title (the terminal has taken it all), and
# quiet again. The CPU of each span: Run from go to the marker, Tail from the marker to quiet.
function Invoke-Pattern($Term, $P, [string]$Name, [double]$TimeoutSec = 300) {
  $q0 = Wait-BenchQuiet $Term $setup -TimeoutSec 15
  $marker = "bench-done-$Name-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
  $watch = [LbtermBench.TitleWatch]::Start([long]$Term.Hwnd, $marker, [int]($TimeoutSec * 1000))
  $c0 = Get-BenchCpu $Term $setup
  $th0 = Get-BenchThreads $Term
  [IO.File]::WriteAllText((Join-Path $P.Dir "go-$Name"), $marker)
  $done = Join-Path $P.Dir "done-$Name.json"
  if (-not (Wait-BenchFile $done $TimeoutSec)) { $watch.Cancel(); throw "the producer didn't finish $Name within $TimeoutSec s" }
  $seen = $watch.Wait([int]($TimeoutSec * 1000))
  if (-not $seen) { throw "the window's title never showed $marker" }
  $c1 = Get-BenchCpu $Term $setup
  $th1 = Get-BenchThreads $Term
  $q1 = Wait-BenchQuiet $Term $setup -TimeoutSec 60
  $c2 = Get-BenchCpu $Term $setup
  $d = Get-Content -Raw $done | ConvertFrom-Json
  [pscustomobject]@{
    Done = $d; Seen = $seen; EndToEnd = ($seen - $d.start) / 1e7; Behind = ($seen - $d.end) / 1e7
    Run = Measure-BenchCpu $c0 $c1; Tail = Measure-BenchCpu $c1 $c2; TailMs = $q1.Ms; TailQuiet = $q1.Quiet; QuietBefore = $q0.Quiet
    Threads = Measure-BenchThreads $th0 $th1
  }
}

# Seven more tabs in an LBTerm window, through its cli, each running its own ready.ps1
function Add-BenchTabs($Term, [object[]]$Programs) {
  $cli = Join-Path $bin 'lbterm.exe'
  $sock = switch ($Term.Kind) {
    'lbterm' { Get-GuiSocket $Term.GuiPid }
    'lbterm-mux' { $setup.Socket }
  }
  $env:WEZTERM_UNIX_SOCKET = $sock
  try {
    $list = $null
    for ($i = 0; $i -lt 50 -and -not $list; $i++) {
      $j = (& $cli cli --no-auto-start list --format json 2>$null) -join "`n"
      if ($LASTEXITCODE -eq 0 -and $j) { $list = @($j | ConvertFrom-Json) }
      if (-not $list) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $list) { throw "the $($Term.Kind) cli couldn't list the window's panes ($sock)" }
    $pane = $list[0].pane_id
    foreach ($prog in $Programs) {
      $line = @('cli', '--no-auto-start', 'spawn', '--pane-id', "$pane", '--cwd', $cwd, '--') + @($prog)
      $got = (& $cli @line 2>&1) -join ' '
      if ($LASTEXITCODE) { throw "cli spawn failed: $got" }
    }
  } finally { Remove-Item Env:WEZTERM_UNIX_SOCKET -ErrorAction SilentlyContinue }
}

function Get-Kinds { @($Kinds | Where-Object { -not $banned.ContainsKey($_) }) }

# What else ran as a round began: the whole PC's busy cores over a second and the top processes
function Write-Load([string]$What) {
  $l = Get-BenchLoad
  Add-Result @{ phase = 'load'; what = $What; busyCores = $l.BusyCores; top = $l.Top }
  Write-Log "load before $What`: $($l.BusyCores) cores busy; $($l.Top -join ', ')"
}

# Windows Terminal takes the foreground as it opens, however it's started (2026-10-30: 614 ms
# after the launch, through wt.exe started with SW_SHOWNOACTIVATE, 2.3 s after Lachlan's last
# key), so it runs only when Lachlan has said he's away (-Away)
if ('wt' -in $Kinds -and -not $Away) {
  $banned['wt'] = 'not run: Windows Terminal takes the foreground as it opens, so it runs only with -Away (Lachlan away from the keyboard)'
  Write-Log "wt: $($banned.wt)"
}

#endregion

Protect-Existing
[LbtermBench.ForegroundWatch]::Start()
[LbtermBench.ProcessWatch]::Start(@('wsl.exe', 'wslhost.exe', 'wslrelay.exe', 'wslservice.exe', 'wslg.exe', 'bash.exe', 'WslInstaller.exe'))

Invoke-Test {
  if ('smoke' -in $Phase) {
    Write-Log 'phase smoke'
    foreach ($k in Get-Kinds) {
      $p = New-Producer "smoke-$k" @('plain')
      Use-Terminal -Kind $k -Program $p.Program -What 'smoke' -Body {
        param($term)
        $ready = Read-ProducerReady $p
        $shot = [LbtermE2E.Win]::CaptureWindow([IntPtr][long]$term.Hwnd)
        if ($shot) { $png = Join-Path $Out "smoke-$k.png"; $shot.Save($png); $shot.Dispose() }
        $mem = Measure-BenchMemory $term $setup
        $procs = @(Get-BenchProcesses $term $setup | ForEach-Object { "$($_.Role):$($_.Exe):$($_.Pid)" })
        $x = Invoke-Pattern $term $p 'plain'
        $shot = [LbtermE2E.Win]::CaptureWindow([IntPtr][long]$term.Hwnd)
        if ($shot) { $png = Join-Path $Out "smoke-$k-plain.png"; $shot.Save($png); $shot.Dispose() }
        Write-Log ("smoke {0}: plain {1:N1} MB/s shown ({2:N1} taken), {3:N2} s behind the writes; terminal CPU {4:N2} s (gui {5:N2}, server {6:N2}, conpty {7:N2}), {8:N2} s after" -f $k, ($x.Done.bytes / 1MB / $x.EndToEnd), ($x.Done.bytes / 1MB / $x.Done.seconds), $x.Behind, $x.Run.Terminal, $x.Run.Gui, $x.Run.Server, $x.Run.Conpty, $x.Tail.Terminal)
        Write-Log "smoke $k`: window $($term.WindowClass) at $($term.Placement.Left),$($term.Placement.Top) $($term.Placement.Width)x$($term.Placement.Height), shown $(ConvertTo-Ms ($term.Visible - $term.Launch)) ms after the launch; console $($ready.console.cols)x$($ready.console.rows), mode $($ready.console.modeBefore)->$($ready.console.modeAfter), cp $($ready.console.codePage); $($procs -join ' ')"
        Add-Result @{ phase = 'smoke'; kind = $k; console = $ready.console; placement = $term.Placement; memory = $mem; processes = $procs }
      } | Out-Null
    }
  }

  if ('startup' -in $Phase) {
    Write-Log 'phase startup'
    for ($r = 0; $r -le $StartupRounds; $r++) {
      if ($r % 3 -eq 0) { Write-Load "startup round $r" }
      foreach ($k in Get-Kinds) {
        $ready = New-Ready "startup-$k-$r" -Sleep 60
        Use-Terminal -Kind $k -Program $ready.Program -What "startup $r" -Body {
          param($term)
          $rd = Read-Ready $ready.Path
          $progCreated = [LbtermBench.Native]::Created($rd.Pid)
          $res = @{
            phase = 'startup'; kind = $k; round = $r; warmup = ($r -eq 0)
            visibleMs = ConvertTo-Ms ($term.Visible - $term.Launch)
            programCreatedMs = $(if ($progCreated) { ConvertTo-Ms ($progCreated - $term.Launch) } else { $null })
            programRunningMs = ConvertTo-Ms ($rd.At - $term.Launch)
            launchCreatedMs = $(if ($term.LaunchCreated) { ConvertTo-Ms ($term.LaunchCreated - $term.Launch) } else { $null })
            createProcessMs = ConvertTo-Ms ($term.LaunchReturned - $term.Launch)
            cols = $rd.Cols; rows = $rd.Rows
          }
          Add-Result $res
          Write-Log "startup $k r$r$(if ($r -eq 0) { ' (warm-up)' }): window $($res.visibleMs) ms, program created $($res.programCreatedMs) ms, running $($res.programRunningMs) ms (launch process created $($res.launchCreatedMs) ms)"
        } | Out-Null
      }
    }
  }

  if ('throughput' -in $Phase) {
    Write-Log "phase throughput ($SizeMB MB a pattern)"
    for ($r = 1; $r -le $Rounds; $r++) {
      Write-Load "throughput round $r"
      foreach ($k in Get-Kinds) {
        $p = New-Producer "tp-$k-$r" $Patterns
        Use-Terminal -Kind $k -Program $p.Program -What "throughput $r" -Body {
          param($term)
          Read-ProducerReady $p | Out-Null
          foreach ($n in $Patterns) {
            $x = Invoke-Pattern $term $p $n
            $mb = $x.Done.bytes / 1MB
            $res = @{
              phase = 'throughput'; kind = $k; round = $r; pattern = $n
              bytes = $x.Done.bytes; lines = $sizes[$n].lines; seconds = $x.Done.seconds; endToEnd = [Math]::Round($x.EndToEnd, 4); behind = [Math]::Round($x.Behind, 4)
              # Taken (the producer's writes) and shown (until the title after the data showed)
              mbps = [Math]::Round($mb / $x.Done.seconds, 2); linesPerSec = [Math]::Round($sizes[$n].lines / $x.Done.seconds)
              mbpsShown = [Math]::Round($mb / $x.EndToEnd, 2); linesPerSecShown = [Math]::Round($sizes[$n].lines / $x.EndToEnd)
              cpuRun = $x.Run; cpuTail = $x.Tail; tailMs = $x.TailMs; tailQuiet = $x.TailQuiet; quietBefore = $x.QuietBefore
              cpuPerMB = [Math]::Round(($x.Run.Terminal + $x.Tail.Terminal) / $mb, 4)
              threads = $x.Threads
            }
            Add-Result $res
            Write-Log ("throughput {0} r{1} {2}: {3:N1} MB/s shown ({4:N1} taken), {5:N0} lines/s, {6:N2} s to the marker ({7:N2} s behind the writes); terminal CPU {8:N2} s to the marker ({9:N0}% of a core: gui {10:N2}, server {11:N2}, conpty {12:N2}), {13:N2} s after; {14:N3} CPU s/MB" -f $k, $r, $n, $res.mbpsShown, $res.mbps, $res.linesPerSecShown, $x.EndToEnd, $x.Behind, $x.Run.Terminal, $x.Run.TerminalPercent, $x.Run.Gui, $x.Run.Server, $x.Run.Conpty, $x.Tail.Terminal, $res.cpuPerMB)
            Write-Log "  busiest threads: $($x.Threads -join '; ')"
          }
        } | Out-Null
      }
    }
  }

  if ('idle' -in $Phase) {
    Write-Log "phase idle ($RestSec s at rest)"
    for ($r = 1; $r -le $Rounds; $r++) {
      Write-Load "idle round $r"
      foreach ($k in Get-Kinds) {
        # One tab of an idle shell
        $ready = New-Ready "idle1-$k-$r" -Idle
        Use-Terminal -Kind $k -Program $ready.Program -What "1 tab $r" -Body {
          param($term)
          Read-Ready $ready.Path | Out-Null
          Start-Sleep -Seconds 10
          $c0 = Get-BenchCpu $term $setup; Start-Sleep -Seconds $RestSec; $c1 = Get-BenchCpu $term $setup
          $mem = Measure-BenchMemory $term $setup
          $cpu = Measure-BenchCpu $c0 $c1
          Add-Result @{ phase = 'idle'; kind = $k; round = $r; tabs = 1; cpu = $cpu; memory = $mem }
          Write-Log "idle $k r$r 1 tab: terminal $($cpu.TerminalPercent)% of a core (gui $($cpu.Gui) s, server $($cpu.Server) s, conpty $($cpu.Conpty) s, shells $($cpu.Shell) s over $($cpu.Seconds) s); $($mem.PrivateMB) MB private, $($mem.WorkingSetMB) MB working set"
        } | Out-Null

        # Eight tabs of idle shells
        $readies = @(1..8 | ForEach-Object { New-Ready "idle8-$k-$r-$_" -Idle })
        $more = if ($k -eq 'wt') { @($readies[1..7] | ForEach-Object { , $_.Program }) } else { @() }
        Use-Terminal -Kind $k -Program $readies[0].Program -More $more -What "8 tabs $r" -Body {
          param($term)
          Read-Ready $readies[0].Path | Out-Null
          if ($k -ne 'wt') { Add-BenchTabs $term @($readies[1..7] | ForEach-Object { , $_.Program }) }
          foreach ($x in $readies) { Read-Ready $x.Path | Out-Null }
          Start-Sleep -Seconds 10
          $c0 = Get-BenchCpu $term $setup; Start-Sleep -Seconds $RestSec; $c1 = Get-BenchCpu $term $setup
          $mem = Measure-BenchMemory $term $setup
          $cpu = Measure-BenchCpu $c0 $c1
          Add-Result @{ phase = 'idle'; kind = $k; round = $r; tabs = 8; cpu = $cpu; memory = $mem }
          Write-Log "idle $k r$r 8 tabs: terminal $($cpu.TerminalPercent)% of a core (gui $($cpu.Gui) s, server $($cpu.Server) s, conpty $($cpu.Conpty) s, shells $($cpu.Shell) s); $($mem.PrivateMB) MB private, $($mem.WorkingSetMB) MB working set ($($mem.Conptys) conpty, $($mem.Shells) shells)"
        } | Out-Null

        # -ScrollLines lines of scrollback: the scrollback set alike (-Scrollback), then for
        # LBTerm every line kept
        foreach ($keep in @($false, $true)) {
          if ($keep -and $k -eq 'wt') { continue }
          $p = New-Producer "scroll-$k-$r-$(if ($keep) { 'keep' } else { 'alike' })" @('scroll')
          $sb = if ($keep) { $ScrollLines } else { 0 }
          $kept = if ($keep) { $ScrollLines } else { $Scrollback }
          Use-Terminal -Kind $k -Program $p.Program -Scrollback $sb -What "scrollback $r" -Body {
            param($term)
            Read-ProducerReady $p | Out-Null
            $before = Measure-BenchMemory $term $setup
            $x = Invoke-Pattern $term $p 'scroll'
            Start-Sleep -Seconds 5
            $mem = Measure-BenchMemory $term $setup
            Add-Result @{ phase = 'scrollback'; kind = $k; round = $r; scrollbackLines = $kept; lines = $sizes.scroll.lines; before = $before; memory = $mem; seconds = $x.Done.seconds; endToEnd = $x.EndToEnd }
            Write-Log "scrollback $k r$r ($kept kept): $($before.PrivateMB) -> $($mem.PrivateMB) MB private, $($before.WorkingSetMB) -> $($mem.WorkingSetMB) MB working set; $($sizes.scroll.lines) lines in $([Math]::Round($x.Done.seconds, 2)) s"
          } | Out-Null
        }
      }
    }
  }

  # wsl.exe and its kin, and the System32 bash.exe (WSL's): Git's bash.exe runs all the time
  $wsl = @([LbtermBench.ProcessWatch]::All() | Where-Object { $_.Exe -ine 'bash.exe' -or ($_.Path -and $_.Path -like "$env:SystemRoot\System32\*") })
  foreach ($w in $wsl) { Write-Log "NOTE: $($w.Exe) ($($w.Path)) started (pid $($w.Pid), parent $($w.Parent)) at $(([datetime]::new($w.At, 'Utc')).ToLocalTime().ToString('HH:mm:ss.fff'))" }
  Add-Result @{ phase = 'watch'; wsl = @($wsl | ForEach-Object { "$($_.Exe) $($_.Pid) parent $($_.Parent) at $(([datetime]::new($_.At, 'Utc')).ToLocalTime().ToString('HH:mm:ss.fff'))" }); banned = $banned; load = Get-BenchLoad }
  foreach ($b in $banned.GetEnumerator()) { Write-Log "banned $($b.Key): $($b.Value)" }
} -Finally {
  [LbtermBench.ForegroundWatch]::Stop()
  [LbtermBench.ProcessWatch]::Stop()
} -AllowWindow {
  param($w)
  # The benchmark's own, and windows of processes running before it began (Lachlan's Chrome
  # showing a popup as he works): bench.log notes those
  $ours = $global:BenchPids.Contains([uint32]$w.Pid) -or [bool](@($w.Ancestors) | Where-Object { $global:BenchPids.Contains([uint32]$_) })
  if (-not $ours -and (Test-Protected ([uint32]$w.Pid))) { Write-Log "NOTE: a window of a process that was running before the benchmark showed: $($w.Exe) $($w.Pid) '$($w.Title)' [$($w.Class)] $($w.Width)x$($w.Height), $($w.IdleMs) ms after the last real input"; return $true }
  $ours
} -Passed "bench: done, results in $results"
