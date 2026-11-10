#Requires -Version 7
# Variant: -Local
# Covers: term/src/program_status.rs, term/src/terminalstate/performer.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-client/src/pane/clientpane.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/projects.rs, wezterm-gui/src/termwindow/render/strip.rs, lbterm-panel/src/claude.rs
<#
What OSC 7501 reports cost (lbterm/MUX.md, "What a status report costs"): a window that looks like
Lachlan's LBTerm (New-TestConfig) with -Tabs tabs on a mux domain of the test's own, as his live in
his server (-Local: the window's own panes), -Working of them running a stand-in
(fake-status.ps1) that sends the records a working Claude Code session and claudehook send. The
open tab is quiet. Each working tab first sends its records once (`base`): its own, a subagent's
and two background tasks'.

1. Cost, in a window with no debug logging: -Seconds each of rest, `resend` (the four records again
   word for word every 250 ms: claudehook's minute resend of a running task's record, sped up),
   `parts` (its subagent turning between working and paused every 250 ms, which its row counts the
   same), `words` (its own message turning every 2.5 s, as Claude Code's task summary does: a
   change its row shows) and `flip` (its state turning between working and a permission prompt
   every 2.5 s). For each, the GUI's CPU and its main thread's, the mux server's, the frames painted
   (QueryProcessCycleTime, exact; the state file's frame count).
2. Counts, in a second window with WEZTERM_LOG=codec=debug and lbterm::status=debug: the
   SetProgramStatus PDUs the server sent for each, and the GUI's word on each (`repaint` or
   `nothing shown changed`, from builds that say).
3. What must still show at once: a background tab's own record blocked, working again and done on
   its rail row, and its tasks' count; the open tab's blocked.

It fails when records said again word for word bring a PDU or a frame, when a change no row shows
paints a frame, when a change a row shows doesn't paint (fewer frames than half the changes), when
a report said again costs the GUI thread more than -ResendShare of a frame of a row's change, or
when a row doesn't show its record within 1 s. cost.json in target\perf\status-storm has every
number.

Measured on 2026-10-30, three 10 s runs of each build in turn: before these fixes (main), the 1,120
records said again cost 2,172 to 2,506 ms of the GUI thread (1.9 to 2.2 ms each), painted 260 to
270 frames and brought 283 to 297 SetProgramStatus, and the 280 subagent pauses 2,121 to 2,682 ms
(7.6 to 9.6 ms each, a frame each). After: 0.5 to 1.2 ms and 30 to 36 ms, no frame, and no PDU for
the records said again; the server's time for those went from 212 to 274 ms to 62 to 81. A change
a row shows costs what it did, about 15 ms of the GUI thread for its two frames, and every row
showed its record within 64 to 124 ms in both builds.

  pwsh -File lbterm\perf\status-storm.ps1 [-Local] [-Tabs 12] [-Working 8] [-Seconds 10] [-NoCount] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [switch]$Local,
  [int]$Tabs = 12,
  [int]$Working = 8,
  [int]$Seconds = 10,
  # Only the cost window, not the counting one (and no budgets that need its counts)
  [switch]$NoCount,
  # Only measure: no budgets
  [switch]$Measure,
  [double]$ResendShare = 0.02
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('status-storm' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermStatusCost {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  const uint THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000;
  // Cycles a second, spinning on this thread for ms: the cycle counter's rate
  public static double Rate(int ms) {
    ulong a, b; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out a);
    while (sw.ElapsedMilliseconds < ms) { }
    QueryThreadCycleTime(GetCurrentThread(), out b);
    return (b - a) / sw.Elapsed.TotalSeconds;
  }
  // Every thread of the process, exited ones included
  public static ulong ProcessCycles(int pid) {
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, (uint)pid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryProcessCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  public static ulong ThreadCycles(int id) {
    IntPtr h = OpenThread(THREAD_QUERY_LIMITED, false, (uint)id);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryThreadCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  // The process's first thread: the GUI's main thread
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      ProcessThread first = null;
      foreach (ProcessThread t in p.Threads) {
        try { if (first == null || t.StartTime < first.StartTime) first = t; } catch { }
      }
      return first == null ? 0 : first.Id;
    }
  }
}
'@

$ctl = Join-Path $T.Out 'ctl'
New-Item -ItemType Directory -Force $ctl | Out-Null
$helper = Join-Path $PSScriptRoot 'fake-status.ps1'
$names = @('open') + @(1..($Tabs - 1) | ForEach-Object { if ($_ -le $Working) { "work $_" } else { "idle $_" } })
$workTabs = @($names | Where-Object { $_ -like 'work *' })
function Get-Control([string]$Name) { Join-Path $ctl (($Name -replace ' ', '-') + '.txt') }
# A tab's control line; its stand-in reads the file every 20 ms, which holds it a moment
function Set-Control([string]$Name, [string]$Line) {
  $f = Get-Control $Name
  for ($i = 0; ; $i++) {
    try { [IO.File]::WriteAllText($f, $Line); return } catch { if ($i -ge 100) { throw }; Start-Sleep -Milliseconds 5 }
  }
}
function Get-Program([string]$Name, [int]$Index) {
  Set-Control $Name 'rest'
  # Sessions started apart: each at another point in the 2.5 s cycle
  New-PwshProgram -File $helper -ArgumentList '-Name', $Name, '-Control', (Get-Control $Name), '-PhaseMs', ($Index * 311 % 2500)
}

$cfg = Join-Path $T.Out 'config\status-storm.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
# The server reads the same file (serve_command), so both know the test domain
$domain = if (-not $Local) {
  @"
config.unix_domains = { {
  name = 'statusstorm',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 140
config.initial_rows = 40
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.window_close_confirmation = 'NeverPrompt'
$domain
"@

$rate = [LbtermStatusCost]::Rate(300)
$ms = { param([double]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }
$mux = -not $Local

# A window with every tab running its stand-in, each working tab's records sent, settled. Its
# server's process, on a mux domain.
function Open-StormWindow([string]$Log) {
  $open = @{ ConfigFile = $cfg; Program = (Get-Program 'open' 0); TimeoutSec = 20 }
  if ($Log) { $open.Log = $Log }
  if ($mux) { $open.Domain = 'statusstorm'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  if ($mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window''s first frame' | Out-Null
  $first = (Get-Panes -Server:$mux)[0].pane_id
  if ($mux) {
    # `connect` runs the server's default program in the first tab, whatever comes after `--`:
    # the open tab's stand-in goes in a tab of its own, and that one goes
    $default = $first
    $first = New-Tab -From $default -Program (Get-Program 'open' 0) -Server
    # Once the window has it: a window whose only tab goes closes, and the process with it
    Wait-Change { param($s) @($s.tabs).Count -eq 2 } -What 'the open tab''s stand-in' | Out-Null
    Invoke-Cli kill-pane --pane-id $default -Server -Check | Out-Null
    Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'the default tab gone' | Out-Null
  }
  for ($i = 1; $i -lt $names.Count; $i++) { New-Tab -From $first -NoActivate -Program (Get-Program $names[$i] $i) -Server:$mux | Out-Null }
  Wait-Change { param($s) @($s.tabs).Count -eq $Tabs } -What "$Tabs tabs" | Out-Null
  Start-Sleep 2
  # Every tab's title and folder, as a Claude tab's are, and each working tab's records
  # (fake-status.ps1's `hello` and `base`)
  foreach ($n in $names) { Set-Control $n $(if ($n -in $workTabs) { 'base' } else { 'hello' }) }
  # Each working tab's row counts its subagent and its two background tasks
  Wait-State { param($s) @($s.rail.rows | Where-Object { $_.agents -eq 3 }).Count -eq $Working } -What "$Working rows with their records" -TimeoutSec 10 | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.title -eq 'open' }).Count -eq 1 } -What 'the open tab''s title' -TimeoutSec 10 | Out-Null
  Start-Sleep 1
  Wait-Settled -What "$Tabs tabs at rest" | Out-Null
  $server = if ($mux) { @(Get-TestServers)[0] } else { $null }
  if ($mux -and -not $server) { throw 'no test mux server running' }
  [pscustomobject]@{ Window = $w; First = $first; Server = $(if ($server) { [int]$server.ProcessId } else { $null }); Main = [LbtermStatusCost]::MainThread($w.Pid) }
}

function Get-Frame($w) { $s = Read-State -Window $w; [long]$s.frame }
function Get-Size([string]$Path) { if (Test-Path -LiteralPath $Path) { (Get-Item -LiteralPath $Path).Length } else { 0 } }
function Read-From([string]$Path, [long]$From) {
  if (-not (Test-Path -LiteralPath $Path)) { return '' }
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
}
function Get-ServerLog([int]$ProcessId) { Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$ProcessId.txt" }

# Sets every listed tab's control line, and waits for each stand-in to say how many reports it had
# sent by then (its .sent file, written as it sees the line)
function Set-Mode([string[]]$Tabs, [string]$Mode) {
  $before = @{}
  foreach ($n in $Tabs) { $f = "$(Get-Control $n).sent"; $before[$n] = if (Test-Path $f) { (Get-Item $f).LastWriteTimeUtc } else { [datetime]::MinValue } }
  foreach ($n in $Tabs) { Set-Control $n $Mode }
  $sent = 0
  foreach ($n in $Tabs) {
    $f = "$(Get-Control $n).sent"
    $was = $before[$n]
    if (-not (Wait-Until { (Test-Path $f) -and (Get-Item $f).LastWriteTimeUtc -gt $was }.GetNewClosure() -TimeoutSec 3 -IntervalMs 20)) { throw "the stand-in in tab '$n' didn't answer its control line '$Mode'" }
    $sent += [int](Get-Content -Raw $f)
  }
  $sent
}

# One phase: the tabs' mode for -Seconds, what it cost, and with -Count the log lines it made
function Measure-Phase($o, [string]$Mode, [string[]]$Tabs, [switch]$Count) {
  $w = $o.Window
  $guiLog = $w.Log; $srvLog = if ($o.Server) { Get-ServerLog $o.Server } else { $null }
  $g0 = Get-Size $guiLog; $s0 = if ($srvLog) { Get-Size $srvLog } else { 0 }
  $f0 = Get-Frame $w
  $c0 = [LbtermStatusCost]::ProcessCycles($w.Pid); $m0 = [LbtermStatusCost]::ThreadCycles($o.Main)
  $sv0 = if ($o.Server) { [LbtermStatusCost]::ProcessCycles($o.Server) } else { 0 }
  $sentBefore = if ($Mode -ne 'rest') { Set-Mode $Tabs $Mode } else { 0 }
  Start-Sleep -Seconds $Seconds
  $sentAfter = if ($Mode -ne 'rest') { Set-Mode $Tabs 'rest' } else { 0 }
  # What's on its way through the server and the GUI arrives
  Start-Sleep -Milliseconds 500
  $row = [ordered]@{
    Mode = $Mode; Seconds = $Seconds; Reports = $sentAfter - $sentBefore
    Frames = (Get-Frame $w) - $f0
    GuiMs = & $ms ([LbtermStatusCost]::ProcessCycles($w.Pid) - $c0)
    MainMs = & $ms ([LbtermStatusCost]::ThreadCycles($o.Main) - $m0)
    ServerMs = $(if ($o.Server) { & $ms ([LbtermStatusCost]::ProcessCycles($o.Server) - $sv0) } else { $null })
  }
  if ($Count) {
    $g = Read-From $guiLog $g0
    $s = if ($srvLog) { Read-From $srvLog $s0 } else { '' }
    $row.SetProgramStatus = [regex]::Matches($s, 'encode(?:_async)? SetProgramStatus size=').Count
    $row.Repaints = [regex]::Matches($g, 'lbterm::status\s+> program status pane \d+ window \d+: repaint').Count
    $row.Unshown = [regex]::Matches($g, 'lbterm::status\s+> program status pane \d+ window \d+: nothing shown changed').Count
  }
  $line = "$Mode$(if ($Count) { ' (counted)' }): $($row.Reports) reports in $Seconds s; $($row.Frames) frames; GUI $($row.GuiMs) ms (main thread $($row.MainMs))$(if ($o.Server) { ", server $($row.ServerMs) ms" })"
  if ($Count) { $line += "; SetProgramStatus sent $($row.SetProgramStatus); the GUI: repaint $($row.Repaints), nothing shown changed $($row.Unshown)" }
  Write-Host $line
  [pscustomobject]$row
}

$phases = @('rest', 'resend', 'parts', 'words', 'flip')
$cost = [Collections.Generic.List[object]]::new()
$counted = [Collections.Generic.List[object]]::new()
$checks = [Collections.Generic.List[object]]::new()
Invoke-Test {
  # 1. Cost
  $o = Open-StormWindow
  foreach ($phase in $phases) { $cost.Add((Measure-Phase $o $phase $workTabs)) }
  $cost.Add((Measure-Phase $o 'rest' $workTabs))

  # 3. What must show at once: each record on its tab's row, as the state file says the frame drew
  $w = $o.Window
  # Wait-State hands its test the window's entry in the state file; the blocks below run in this
  # scope, so they see $Test, $Index and $row as they are when each is asked
  $row = { param($s, $i) @($s.rail.rows | Where-Object { $_.tab_index -eq $i })[0] }
  $show = {
    param([string]$Tab, [int]$Index, [string]$Line, [scriptblock]$Test, [string]$What)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Set-Control $Tab $Line
    $shown = $true
    try { Wait-State { param($s) & $Test (& $row $s $Index) } -What $What -TimeoutSec 1 | Out-Null } catch { $shown = $false }
    $checks.Add([pscustomobject]@{ What = $What; Ms = $clock.ElapsedMilliseconds; Shown = $shown })
    if (-not $shown) {
      $r = & $row (Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)) $Index
      Add-Failure "$What didn't show within 1 s: the row says state $($r.state), status '$($r.status)', agents $($r.agents), unread $($r.unread)"
    }
  }
  # Tab 2 is a background tab; the open tab is tab 0
  & $show 'work 2' 2 'state:blocked' { param($r) $r.state -eq 'waiting' -and $r.status -eq 'Permission' } 'a background tab blocked on its row'
  & $show 'work 2' 2 'state:working' { param($r) $r.state -eq 'working' } 'a background tab working again on its row'
  & $show 'work 2' 2 'agents:3' { param($r) $r.agents -eq 5 } 'a background tab''s three subagents (and two tasks) on its row'
  & $show 'work 2' 2 'state:done' { param($r) $r.state -eq 'idle' -and $r.status -match 'Waiting on' -and $r.unread } 'a background tab done, waiting on its tasks, unread'
  & $show 'open' 0 'state:blocked' { param($r) $r.state -eq 'waiting' } 'the open tab blocked on its row'
  foreach ($c in $checks) { Write-Host "$($c.What): $(if ($c.Shown) { "$($c.Ms) ms" } else { 'never' })" }

  # 2. Counts
  if (-not $NoCount) {
    Stop-TestWindow $w
    Stop-TestServers
    Start-Sleep 1
    $o = Open-StormWindow -Log 'info,codec=debug,lbterm::status=debug'
    foreach ($phase in $phases | Where-Object { $_ -ne 'rest' }) { $counted.Add((Measure-Phase $o $phase $workTabs -Count)) }
  }

  # The budgets
  if (-not $Measure) {
    $at = { param($m) @($cost | Where-Object Mode -eq $m)[0] }
    $rest = & $at 'rest'; $resend = & $at 'resend'; $words = & $at 'words'
    foreach ($m in 'resend', 'parts') {
      $r = & $at $m
      if ($r.Frames -gt 1) { Add-Failure "$m painted $($r.Frames) frames in $Seconds s for $($r.Reports) reports: no row changes" }
    }
    foreach ($m in 'words', 'flip') {
      $r = & $at $m
      if ($r.Frames -lt [Math]::Floor($r.Reports / 2)) { Add-Failure "$m painted $($r.Frames) frames for $($r.Reports) reports, each a change its tab's row shows" }
    }
    # A frame for a row's change, against a report said again: the GUI thread's, which typing waits on
    $perFrame = if ($words.Frames) { ($words.MainMs - $rest.MainMs) / $words.Frames } else { 0 }
    $perResend = if ($resend.Reports) { ($resend.MainMs - $rest.MainMs) / $resend.Reports } else { 0 }
    Write-Host ("a report said again costs {0:N3} ms of the GUI thread, a frame for a row's change {1:N3} ms: {2:P1} (budget {3:P0})" -f $perResend, $perFrame, $(if ($perFrame) { $perResend / $perFrame } else { 0 }), $ResendShare)
    if ($perFrame -le 0) { Add-Failure "the rows' changes painted no frames ($($words.Frames)) or cost nothing: nothing to hold the resends to" }
    elseif ($perResend -gt $ResendShare * $perFrame) { Add-Failure ("a report said again costs {0:N3} ms of the GUI thread, {1:P1} of the {2:N3} ms a frame for a row's change takes (budget {3:P0}): it changes nothing" -f $perResend, ($perResend / $perFrame), $perFrame, $ResendShare) }
    foreach ($r in $counted) {
      if ($mux -and $r.Mode -eq 'resend' -and $r.SetProgramStatus) { Add-Failure "resend: the server sent SetProgramStatus $($r.SetProgramStatus) times for $($r.Reports) records said again word for word" }
    }
  }
  [pscustomobject]@{ Mux = $mux; Tabs = $Tabs; Working = $Working; Cost = $cost; Counted = $counted; Checks = $checks } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "status-storm$(if ($Local) { ' (local)' }): records said again send and paint nothing, a change no row shows paints nothing, a change a row shows paints and shows at once"
