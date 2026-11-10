#Requires -Version 7
# Variant: -Local
# Covers: term/src/terminalstate/performer.rs, mux/src/localpane.rs, mux/src/window.rs, wezterm-client/src/lbterm_names.rs, wezterm-client/src/pane/clientpane.rs, wezterm-client/src/domain.rs, wezterm-client/src/titles.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/titles.rs, wezterm-gui/src/termwindow/taskbar.rs, wezterm-gui/src/tabbar.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, lbterm-extras/src/titles.rs, lbterm-panel/src/claude.rs, term/src/terminal.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
What titles cost (lbterm/MUX.md, "What a title costs"): a window that looks like Lachlan's LBTerm
(New-TestConfig) with -Tabs tabs on a mux domain of the test's own, as his live in his server
(-Local: the window's own panes), -Working of them running a stand-in (fake-titles.ps1) that sends
what a working Claude Code session sends besides its screen, at its rates. The open tab is quiet.

1. Cost, in a window with no debug logging: -Seconds each of rest, `titles` (every working tab's
   ◐/◑ title every 960 ms, as Claude Code 2.1.312's), `repeat` (the same title again every 100 ms,
   a program setting a title it has), `mix` (titles, OSC 7501 reports and OSC 9;4 progress every
   2.5 s), and `draw` (the open tab's spinner line every 100 ms: what drawing costs). For each, the
   GUI's CPU and its main thread's, the mux server's, the frames painted (QueryProcessCycleTime,
   exact; the state file's frame count).
2. Counts, in a second window with WEZTERM_LOG=lbterm::title=debug and codec=debug: for each storm,
   the alerts the terminals raised (the server's `alert pane` lines), update_title's passes and the
   Lua events they brought (the GUI's `update_title` and `lua event` lines), and every PDU each side
   sent, by type (codec's `encode` lines).
3. What must still show at once: a background tab's new title on its rail row, first and with the
   window at rest, so nothing else paints it there (a background tab's output repaints nothing,
   and the window repaints for its title only when its row would read otherwise:
   render/tab_panel.rs); the open tab's new title in the window's caption and in the state file; a
   background tab's title in the caption once it's the open tab.

It holds a background tab's title by counts, not by a share of a frame (perf/background-titles,
lbterm/FLOWS.md's "A title out of view" and What could go 13). The cost window's metrics are taken
over every phase (Get-Metrics -Reset before it and Get-Metrics after, outside the cycle reads), and
the titles and repeat phases' tasks, by the line that queued each (PITFALLS 310), are set
against the rest phases': a site that ran at least once for every ten titles more than at rest is
one the titles ran, and on a mux domain so are the GUI thread's wakes. On a mux domain it fails
when any site, or the wakes, ran for the titles: since perf/background-titles' fix-up the client
thread applies a background tab's names push, drops its title alert and keeps the mux window's
title (What could go 13, parts 1 to 3), so a title that changes nothing on screen hands the GUI
thread nothing. Below that one-in-ten line, the counting window holds each of those to its exact
count from its `lbterm::title` lines: no names push to the GUI thread, no title alert reaching the
window's subscription (`dropped in the subscription`) or its task (`nothing shown changed`), and
no mux window title handed (`window title ... to the GUI thread`), each a failure at one. With
the window's own panes (-Local) a title is the pane's task that delivers its held alerts
(mux/src/localpane.rs, deliver_held) and the output it is: its notice's task (mux/src/lib.rs,
send_actions_to_mux), and the window's hop and event task for it (wezterm-gui's
termwindow/mod.rs only, each no more than once a notice), since the window's mux subscription
takes only a mux pane's output out of the open tab with no task (termwindow/budget.rs,
lbterm_hidden_output); every other site, the window's included, is held to none. Before
perf/background-titles' stage 3 those two window sites ran twice a title, the alert's once more.
Each site's runs a title, with why it's left, are printed and kept in cost.json (TitleTasks, and
RepeatTasks for the repeat phase). -Measure prints the sites and holds nothing.
cycles a background title and a frame of the open tab are printed and kept in cost.json as
information only: until 2026-11-10 a background title was held to 4% of a frame, which failed on
every build once frames got cheaper (3 to 4 ms against 6.6 on 2026-10-30) with the title's cost
where it was (lbterm/MUX.md, "What a title costs").

It fails too when a title storm paints a frame (nothing on screen changes),
when a background tab's title, or a title set again unchanged, runs update_title at all (the
test's config hides the tabs from the tab bar and has no Lua handler to read them: nothing an
update shows changes; mix's first progress report in each tab runs one, for the taskbar), when a
title brings a Lua event (the config has no handler for one), when the GUI sends the server a WindowTitleChanged
during a storm of the server's own titles, when a storm's title repaints its tab's rail row (◐ and
◑ change no row: the counting window's `title pane` lines), when a title set again unchanged raises
an alert or an update, when a title raises more than one alert, when on a mux domain any of a
titles storm's pushes of a background tab's title alone goes to the GUI thread (the counting
window's `names push pane` lines, wezterm-client's lbterm_names.rs), when a titles storm's alert of a background tab reaches the window's
event task to find nothing to do there (the counting window's `title pane ... nothing shown
changed` lines; termwindow/titles.rs drops those in the window's mux subscription, before any
task, and says `dropped in the subscription`), or when a title doesn't show. Each
working tab starts the storms titled `◐ <name>`, as its rows stay. cost.json in
target\perf\title-storm has every number.

Measured on 2026-10-30, six 10 s runs of each build in turn: before these fixes (main, with the
counting lines), a background tab's title cost 0.75 to 0.93 ms of the GUI thread (5.0 to 6.3% of a
frame), 1.5 to 1.74 ms of the GUI process and 1.15 to 1.39 ms of the server, and brought 2 alerts,
3 update_title passes, 5.7 to 5.9 Lua events and a WindowTitleChanged sent back; a title set again
unchanged cost 0.46 to 0.57 ms of the GUI thread. The installed build (before main's paint fixes,
so its frames cost more) measured 0.68 to 0.74 ms, 2.7 to 3.1% of its frame. After: 0.23 to 0.26
ms of the GUI thread (1.5 to 1.7%), 0.64 to 0.73 ms of the process and 1.0 to 1.15 ms of the
server, 1 alert, no pass (one in the open tab is 11 to 14 us), no Lua event and nothing sent back;
unchanged, 3 us.

Until a review of the branch, the rail row's check ran straight after the open tab's title and
passed on its frames: a background tab's title never reached its row by itself, on main or on the
branch. Measured again on 2026-10-30 as the script is now, eight runs of the installed build and
eight of the fixed branch, in turn: its row never showed it before, and shows it in 64 to 87 ms
with one frame after; a background title costs 0.633 to 0.724 ms of the GUI thread before and
0.165 to 0.194 ms after (2.4 to 2.9% of a frame); a title set again, which now turns the mux
window's title as upstream's did, 397 to 468 us before and 24 to 35 us after. -Local, four runs
each: 0.466 to 0.561 ms and 0.137 to 0.145 ms a title, 356 to 442 us and 58 to 62 us one again.

Every phase's metrics reply (tasks, timers and messages by the line that spawned them, the PDUs
each way) is saved as metrics-<phase>-<n>.json beside cost.json, the counting window's too; -Metrics
also prints each one's top lines: which work a title still puts on the GUI thread (lbterm/FLOWS.md,
"A title out of view", run 16). A build from before the window's metrics can't be run.

  pwsh -File lbterm\perf\title-storm.ps1 [-Local] [-Tabs 12] [-Working 8] [-Seconds 10] [-NoCount] [-Measure] [-Metrics] [-Bin <build>]
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
  # Each phase's metrics summed up as it ends (they're always taken and saved)
  [switch]$Metrics
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('title-storm' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class LbtermTitleCost {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
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
  public static string Caption(IntPtr h) { var s = new StringBuilder(1024); GetWindowTextW(h, s, s.Capacity); return s.ToString(); }
}
'@

$ctl = Join-Path $T.Out 'ctl'
New-Item -ItemType Directory -Force $ctl | Out-Null
$helper = Join-Path $PSScriptRoot 'fake-titles.ps1'
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
  # Sessions started apart: each at another point in the 960 ms cycle
  New-PwshProgram -File $helper -ArgumentList '-Name', $Name, '-Control', (Get-Control $Name), '-PhaseMs', ($Index * 113 % 960)
}

$cfg = Join-Path $T.Out 'config\title-storm.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
# The server reads the same file (serve_command), so both know the test domain
$domain = if (-not $Local) {
  @"
config.unix_domains = { {
  name = 'titlestorm',
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

$rate = [LbtermTitleCost]::Rate(300)
$ms = { param([double]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }
$mux = -not $Local

# A window with every tab running its stand-in, settled. Its server's process, on a mux domain.
function Open-StormWindow([string]$Log) {
  $open = @{ ConfigFile = $cfg; Program = (Get-Program 'open' 0); TimeoutSec = 20 }
  if ($Log) { $open.Log = $Log }
  if ($mux) { $open.Domain = 'titlestorm'; $open.Socket = $T.Sock }
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
  # Each working tab titled as a working Claude Code is, so its row is in the shape the storms
  # keep it in: its first ◐ would otherwise give its row a new title and its pad a state (the
  # title ConPTY gives a pane as it starts is its program's path), which the rail shows
  foreach ($n in $workTabs) { Set-Control $n "title:$([char]0x25d0) $n" }
  Start-Sleep 1
  Wait-Settled -What "$Tabs tabs at rest" | Out-Null
  $server = if ($mux) { @(Get-TestServers)[0] } else { $null }
  if ($mux -and -not $server) { throw 'no test mux server running' }
  [pscustomobject]@{ Window = $w; First = $first; Server = $(if ($server) { [int]$server.ProcessId } else { $null }); Main = [LbtermTitleCost]::MainThread($w.Pid) }
}

function Get-Frame($w) { $s = Read-State -Window $w; [long]$s.frame }
function Get-Size([string]$Path) { if (Test-Path -LiteralPath $Path) { (Get-Item -LiteralPath $Path).Length } else { 0 } }
function Read-From([string]$Path, [long]$From) {
  if (-not (Test-Path -LiteralPath $Path)) { return '' }
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
}
function Get-ServerLog([int]$ProcessId) { Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$ProcessId.txt" }

# Sets every listed tab's control line, and waits for each stand-in to say how many titles it had
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
  # Before the cycles are read, so the pipe's request isn't in them
  Get-Metrics -Window $w -Reset | Out-Null
  $f0 = Get-Frame $w
  $c0 = [LbtermTitleCost]::ProcessCycles($w.Pid); $m0 = [LbtermTitleCost]::ThreadCycles($o.Main)
  $sv0 = if ($o.Server) { [LbtermTitleCost]::ProcessCycles($o.Server) } else { 0 }
  $sentBefore = if ($Mode -ne 'rest') { Set-Mode $Tabs $Mode } else { 0 }
  Start-Sleep -Seconds $Seconds
  $sentAfter = if ($Mode -ne 'rest') { Set-Mode $Tabs 'rest' } else { 0 }
  # What's on its way through the server and the GUI arrives
  Start-Sleep -Milliseconds 500
  $row = [ordered]@{
    Mode = $Mode; Seconds = $Seconds; Titles = $sentAfter - $sentBefore
    Frames = (Get-Frame $w) - $f0
    GuiMs = & $ms ([LbtermTitleCost]::ProcessCycles($w.Pid) - $c0)
    MainMs = & $ms ([LbtermTitleCost]::ThreadCycles($o.Main) - $m0)
    ServerMs = $(if ($o.Server) { & $ms ([LbtermTitleCost]::ProcessCycles($o.Server) - $sv0) } else { $null })
  }
  # The GUI thread's tasks by the line that queued each (PITFALLS 310), and its wakes: what the
  # titles are held to
  $met = Get-Metrics -Window $w
  $met | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out "metrics-$Mode-$($cost.Count + $counted.Count).json")
  $row.Tasks = @($met.tasks | Where-Object { $_ } | ForEach-Object { [pscustomobject]@{ Site = [string]$_.site; Runs = [int]$_.count; Ms = [double]$_.total_ms } })
  $row.Wakes = [int]$met.totals.wakes
  if ($Metrics) {
    Write-Host "$Mode, the window's metrics:"
    Format-Metrics $met -Top 8 | ForEach-Object { Write-Host "  $_" }
  }
  if ($Count) {
    $g = Read-From $guiLog $g0
    $s = if ($srvLog) { Read-From $srvLog $s0 } else { '' }
    # The terminals' alerts are the server's lines on a mux domain, the GUI's own without one
    $alerts = if ($mux) { $s } else { $g }
    $byKind = { param($text, $pattern) $h = [ordered]@{}; foreach ($m in [regex]::Matches($text, $pattern)) { $k = $m.Groups[1].Value; $h[$k] = 1 + [int]$h[$k] }; $h }
    $row.Alerts = & $byKind $alerts 'lbterm::title\s+> alert pane \d+ (\w+)'
    $row.Passes = [regex]::Matches($g, 'update_title window').Count
    # How long each took, where the build says (the median, in microseconds)
    $us = @([regex]::Matches($g, 'update_title window \d+: (\d+) us') | ForEach-Object { [int]$_.Groups[1].Value } | Sort-Object)
    $row.PassUs = if ($us) { $us[[int][Math]::Floor(($us.Count - 1) / 2)] } else { $null }
    $row.LuaEvents = & $byKind $g 'lua event ([\w-]+) window'
    # The rows a title repainted (render/tab_panel.rs, lbterm_panel_title_changed): ◐ and ◑
    # change no row
    $row.RowRepaints = [regex]::Matches($g, 'lbterm::title\s+> title pane \d+ window \d+: repaint').Count
    # A background tab's names push (its title alone) applied on the client thread, with nothing
    # for the GUI thread, or handed to it (wezterm-client's lbterm_names.rs; a build before it logs
    # neither)
    $row.NamesHere = [regex]::Matches($g, 'lbterm::title\s+> names push pane \d+ .*: applied on the client thread').Count
    $row.NamesGui = [regex]::Matches($g, 'lbterm::title\s+> names push pane \d+ .*: changes more than its names').Count
    # A pane's title alert dropped in the window's mux subscription before any task, and one that
    # reached the window's event task to find nothing to do there (termwindow/titles.rs,
    # lbterm_title_alert_dropped; render/tab_panel.rs, lbterm_panel_title_changed)
    $row.AlertsDropped = [regex]::Matches($g, 'lbterm::title\s+> title pane \d+ window \d+: nothing shown changed, dropped in the subscription').Count
    $row.AlertsIdle = [regex]::Matches($g, 'lbterm::title\s+> title pane \d+ window \d+: nothing shown changed(?!, dropped)').Count
    # ...and one dropped on the client thread before it's handed at all, and the mux window's
    # title kept there or handed to the GUI thread (wezterm-client's lbterm_titles.rs)
    $row.AlertsClient = [regex]::Matches($g, 'lbterm::title\s+> title pane \d+ decided in \d+ us: nothing shown changed, dropped on the client thread').Count
    $row.WindowKept = [regex]::Matches($g, 'lbterm::title\s+> window title \(server.s \d+\) in \d+ us: kept on the client thread').Count
    $row.WindowGui = [regex]::Matches($g, 'lbterm::title\s+> window title \(server.s \d+\) in \d+ us: to the GUI thread').Count
    # What each costs the client thread, which also reads key echoes: the median of its lines' µs
    $median = { param($re) $v = @([regex]::Matches($g, $re) | ForEach-Object { [int]$_.Groups[1].Value } | Sort-Object); if ($v) { $v[[int][Math]::Floor(($v.Count - 1) / 2)] } else { $null } }
    $row.NamesUs = & $median 'lbterm::title\s+> names push pane \d+ \(server.s \d+\) seqno \d+ in (\d+) us: applied'
    $row.AlertUs = & $median 'lbterm::title\s+> title pane \d+ decided in (\d+) us'
    $row.WindowUs = & $median 'lbterm::title\s+> window title \(server.s \d+\) in (\d+) us'
    $row.GuiSent = & $byKind $g 'encode(?:_async)? (\w+) size='
    $row.ServerSent = & $byKind $s 'encode(?:_async)? (\w+) size='
  }
  $line = "$Mode$(if ($Count) { ' (counted)' }): $($row.Titles) titles in $Seconds s; $($row.Frames) frames; GUI $($row.GuiMs) ms (main thread $($row.MainMs))$(if ($o.Server) { ", server $($row.ServerMs) ms" })"
  if ($Count) {
    $fmt = { param($h) if ($h.Count) { ($h.GetEnumerator() | ForEach-Object { "$($_.Key) $($_.Value)" }) -join ', ' } else { 'none' } }
    $line += "; alerts: $(& $fmt $row.Alerts); update_title passes $($row.Passes)$(if ($null -ne $row.PassUs) { " ($($row.PassUs) us at the median)" }); rows repainted $($row.RowRepaints); names pushes applied on the client thread $($row.NamesHere), handed to the GUI thread $($row.NamesGui); title alerts dropped on the client thread $($row.AlertsClient), in the subscription $($row.AlertsDropped), reaching the window for nothing $($row.AlertsIdle); mux window titles kept on the client thread $($row.WindowKept), handed to the GUI thread $($row.WindowGui); the client thread's median a names push $($row.NamesUs) us, a title alert's decision $($row.AlertUs) us, a window title $($row.WindowUs) us; Lua events: $(& $fmt $row.LuaEvents); the GUI sent: $(& $fmt $row.GuiSent); the server sent: $(& $fmt $row.ServerSent)"
  }
  Write-Host $line
  [pscustomobject]$row
}

$cost = [Collections.Generic.List[object]]::new()
$counted = [Collections.Generic.List[object]]::new()
$checks = [Collections.Generic.List[object]]::new()
Invoke-Test {
  # 1. Cost
  $o = Open-StormWindow
  foreach ($phase in @('rest', 'titles', 'repeat', 'mix')) { $cost.Add((Measure-Phase $o $phase $workTabs)) }
  $cost.Add((Measure-Phase $o 'draw' @('open')))
  $cost.Add((Measure-Phase $o 'rest' $workTabs))

  # 3. What must show at once
  $w = $o.Window
  $caption = { [LbtermTitleCost]::Caption($w.Handle) }
  # A background tab's new words on its rail row, first, with the window at rest: nothing else
  # paints it then (the open tab is quiet, a background tab's output repaints nothing), so the
  # title has to bring its own paint. After the open tab's title it would ride on that one's
  # frames, as it did until the window repainted for it (a reviewer's probe, 2026-10-30: never
  # shown in 4 s, on main and on this branch's first build)
  Wait-Settled -What 'the window at rest before the checks' | Out-Null
  $f0 = Get-Frame $w
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Set-Control 'work 2' 'title:check three'
  $shown = $true
  try {
    Wait-State { param($s) @($s.rail.rows | Where-Object { $_.tab_index -eq 2 -and $_.title -eq 'check three' }).Count -eq 1 } -What 'the background tab''s new title on its rail row' -TimeoutSec 2 | Out-Null
  } catch { $shown = $false }
  $checks.Add([pscustomobject]@{ What = 'a background tab''s title on its rail row, by itself'; Ms = $clock.ElapsedMilliseconds; Shown = $shown; Caption = ''; Frames = (Get-Frame $w) - $f0 })
  if (-not $shown) { Add-Failure "a background tab's new title didn't reach its rail row in 2 s, with nothing else painting the window ($((Get-Frame $w) - $f0) frames painted)" }
  $clock.Restart()
  Set-Control 'open' 'title:check one'
  $shown = Wait-Until { (& $caption) -like '*check one' } -TimeoutSec 2 -IntervalMs 5
  $checks.Add([pscustomobject]@{ What = 'the open tab''s title in the caption'; Ms = $clock.ElapsedMilliseconds; Shown = $shown; Caption = (& $caption) })
  if (-not $shown) { Add-Failure "the open tab's new title didn't reach the window's caption in 2 s: it says '$(& $caption)'" }
  try {
    Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.title -eq 'check one' }).Count -eq 1 } -What 'the open tab''s new title in the state file' -TimeoutSec 2 | Out-Null
  } catch { Add-Failure "the open tab's new title didn't reach what the window painted: $($_.Exception.Message)" }
  # A background tab's title shows once it's the open one
  Set-Control 'work 1' 'title:check two'
  Start-Sleep -Milliseconds 300
  $clock.Restart()
  Invoke-Cli activate-tab --pane-id $o.First --tab-index 1 -Server:$mux -Check | Out-Null
  $shown = Wait-Until { (& $caption) -like '*check two' } -TimeoutSec 2 -IntervalMs 5
  $checks.Add([pscustomobject]@{ What = 'a background tab''s title in the caption once open'; Ms = $clock.ElapsedMilliseconds; Shown = $shown; Caption = (& $caption) })
  if (-not $shown) { Add-Failure "a background tab's title didn't reach the caption in 2 s once it was the open tab: it says '$(& $caption)'" }
  # The mux window's own title (cli list's window_title, Lua's MuxWindow:get_title()) is upstream's:
  # whichever pane set one last, a title set again unchanged included, which raises no alert
  # (mux/src/localpane.rs, title_set_again)
  $windowTitle = { [string]@(Get-Panes -Server:$mux)[0].window_title }
  Set-Mode @('work 3') 'title:window x' | Out-Null
  $null = Wait-Until { (& $windowTitle) -eq 'window x' } -TimeoutSec 2 -IntervalMs 20
  Set-Mode @('work 4') 'title:window y' | Out-Null
  $null = Wait-Until { (& $windowTitle) -eq 'window y' } -TimeoutSec 2 -IntervalMs 20
  Set-Mode @('work 3') 'rest' | Out-Null
  $clock.Restart()
  Set-Mode @('work 3') 'title:window x' | Out-Null
  $shown = Wait-Until { (& $windowTitle) -eq 'window x' } -TimeoutSec 2 -IntervalMs 20
  $checks.Add([pscustomobject]@{ What = 'the window''s title from a pane setting its own again'; Ms = $clock.ElapsedMilliseconds; Shown = $shown; Caption = (& $windowTitle) })
  if (-not $shown) { Add-Failure "a pane setting the title it had again didn't give the mux window its title back: the window's title is '$(& $windowTitle)', where upstream's is whichever pane set one last" }
  foreach ($c in $checks) { Write-Host "$($c.What): $(if ($c.Shown) { "$($c.Ms) ms" } else { 'never' })$(if ($null -ne $c.Frames) { ", $($c.Frames) frames" }) ('$($c.Caption)')" }

  # 2. Counts
  if (-not $NoCount) {
    Stop-TestWindow $w
    Stop-TestServers
    Start-Sleep 1
    $o = Open-StormWindow -Log 'info,codec=debug,lbterm::title=debug'
    foreach ($phase in @('titles', 'repeat', 'mix')) { $counted.Add((Measure-Phase $o $phase $workTabs -Count)) }
  }

  # What a background title costs the GUI thread, which typing waits on (the process's other
  # threads, the mux client's reader and the state file's writer, move with what else the PC is
  # doing), and a frame of the open tab: information, not a budget
  $at = { param($m) @($cost | Where-Object Mode -eq $m)[0] }
  $rest = & $at 'rest'; $titles = & $at 'titles'; $draw = & $at 'draw'
  $perTitle = if ($titles.Titles) { ($titles.MainMs - $rest.MainMs) / $titles.Titles } else { 0 }
  $perFrame = if ($draw.Frames) { ($draw.MainMs - $rest.MainMs) / $draw.Frames } else { 0 }
  Write-Host ("a background title costs {0:N3} ms of the GUI thread, a frame of the open tab {1:N3} ms ({2:P1}): information, not a budget" -f $perTitle, $perFrame, $(if ($perFrame) { $perTitle / $perFrame } else { 0 }))

  # What a background title runs on the GUI thread, site by site: a site that ran at least once
  # for every ten titles more over the titles (or repeat) phase than over either rest phase is
  # the titles'. Left, each with why: with the window's own panes, the pane's task that delivers
  # its held alerts (mux/src/localpane.rs, deliver_held), and the output a title is: its notice's
  # task (mux/src/lib.rs, send_actions_to_mux) and the window's hop and event task for it
  # (wezterm-gui/src/termwindow/mod.rs only), each no more than once a notice, since the window's
  # mux subscription takes only a mux pane's output out of the open tab with no task
  # (termwindow/budget.rs, lbterm_hidden_output; FLOWS.md, "A title out of view", -Local). On a
  # mux domain nothing is left (What could go 13, parts 1 to 3: the client thread hands the GUI
  # thread nothing for such a title), and the wakes are held as the sites are. Every other site
  # is held to none
  $leftSites = if ($mux) { @() } else { @('mux/src/localpane.rs:') }
  $noticeSite = 'mux/src/lib.rs:'
  $windowSite = 'wezterm-gui/src/termwindow/mod.rs:'
  $restRuns = @{}
  foreach ($r in @($cost | Where-Object Mode -eq 'rest')) { foreach ($task in $r.Tasks) { $restRuns[$task.Site] = [Math]::Max([int]$restRuns[$task.Site], $task.Runs) } }
  $restWakes = [int](@($cost | Where-Object Mode -eq 'rest' | ForEach-Object { [int]$_.Wakes }) | Measure-Object -Maximum).Maximum
  # (Not $t: PowerShell's names are case-insensitive, and $T is the test)
  $siteTasks = {
    param($phase)
    $least = [Math]::Max(1, [Math]::Ceiling($phase.Titles / 10))
    $tasks = @(foreach ($task in $phase.Tasks) {
      $runs = $task.Runs - [int]$restRuns[$task.Site]
      if ($runs -ge $least) { [pscustomobject]@{ Site = $task.Site; Runs = $runs; Ms = $task.Ms; Why = '' } }
    })
    $notices = if ($mux) { 0 } else { [int](@($tasks | Where-Object { $_.Site.StartsWith($noticeSite) }) | Measure-Object Runs -Sum).Sum }
    foreach ($task in $tasks) {
      $site = $task.Site
      if (@($leftSites | Where-Object { $site.StartsWith($_) }).Count) { $task.Why = "a local pane's alerts, delivered" }
      elseif (-not $mux -and $site.StartsWith($noticeSite)) { $task.Why = "a local pane's output: its notice" }
      elseif (-not $mux -and $site.StartsWith($windowSite) -and $task.Runs -le $notices) { $task.Why = "a local pane's output out of view, at most once a notice ($notices)" }
    }
    , $tasks
  }
  $titleTasks = & $siteTasks $titles
  $repeat = & $at 'repeat'
  $repeatTasks = if ($repeat) { & $siteTasks $repeat } else { @() }
  $phases = @(@{ Name = 'titles'; Row = $titles; Tasks = $titleTasks }, @{ Name = 'repeat'; Row = $repeat; Tasks = $repeatTasks })
  foreach ($phase in $phases) {
    if (-not $phase.Row) { continue }
    Write-Host "$($phase.Name): $($phase.Row.Titles) background titles, $($phase.Row.Wakes) wakes of the GUI thread (rest $restWakes), its tasks by site beyond rest:"
    if (-not $phase.Tasks) { Write-Host '  none' }
    foreach ($task in $phase.Tasks) { Write-Host ("  {0}: {1} runs, {2:N2} a title, {3:N3} ms{4}" -f $task.Site, $task.Runs, $(if ($phase.Row.Titles) { $task.Runs / $phase.Row.Titles } else { 0 }), $task.Ms, $(if ($task.Why) { " ($($task.Why))" } else { '' })) }
  }

  if (-not $Measure) {
    if (-not $titles.Titles) { Add-Failure "titles: the stand-ins sent no title in $Seconds s: nothing to hold" }
    foreach ($phase in $phases) {
      if (-not $phase.Row) { continue }
      $held = @($phase.Tasks | Where-Object { -not $_.Why })
      if ($held) { Add-Failure "$($phase.Name): background tabs' titles, which change nothing on screen, ran tasks on the GUI thread: $(($held | ForEach-Object { "$($_.Site) $($_.Runs) runs ($('{0:N3}' -f $_.Ms) ms)" }) -join ', ') for $($phase.Row.Titles) titles" }
      # On a mux domain such a title hands the GUI thread nothing, so it isn't woken either
      $least = [Math]::Max(1, [Math]::Ceiling($phase.Row.Titles / 10))
      if ($mux -and $phase.Row.Wakes - $restWakes -ge $least) { Add-Failure "$($phase.Name): background tabs' titles woke the GUI thread $($phase.Row.Wakes) times in $Seconds s against $restWakes at rest, for $($phase.Row.Titles) titles: a title that changes nothing on screen hands it nothing" }
    }
    # mix's OSC 7501 reports repaint (the rail shows them): only the titles' storms are held to none
    foreach ($m in 'titles', 'repeat') {
      $r = & $at $m
      if ($r.Frames -gt 1) { Add-Failure "$m painted $($r.Frames) frames in $Seconds s: background tabs' titles change nothing on screen" }
    }
    $titled = @($counted | Where-Object Mode -eq 'titles')[0]
    if ($titled -and $titled.Titles -and -not $titled.Alerts.Count) { Write-Host 'NOTE: this build logs nothing under lbterm::title (alerts, update_title passes, Lua events): its counts are the PDUs alone' }
    foreach ($r in $counted) {
      # Background tabs' titles show nowhere an update reaches; mix's first OSC 9;4 in each tab is a
      # progress alert, which the taskbar shows
      $allowed = if ($r.Mode -eq 'mix') { $Working } else { 0 }
      if ($r.Passes -gt $allowed) { Add-Failure "$($r.Mode): update_title ran $($r.Passes) times for $($r.Titles) titles from background tabs (budget $allowed): nothing it shows changed" }
      if ($r.LuaEvents.Count) { Add-Failure "$($r.Mode): Lua events with no handler in the config: $(($r.LuaEvents.GetEnumerator() | ForEach-Object { "$($_.Key) $($_.Value)" }) -join ', ')" }
      if ($mux -and $r.GuiSent['WindowTitleChanged']) { Add-Failure "$($r.Mode): the GUI sent the server WindowTitleChanged $($r.GuiSent['WindowTitleChanged']) times in a storm of the server's own titles" }
      if ($r.RowRepaints) { Add-Failure "$($r.Mode): $($r.RowRepaints) titles repainted their tab's rail row: Claude Code's ◐ and ◑ change no row" }
      # A background tab's title comes as a push of its title alone, which changes nothing the
      # pane holds but that: the client thread applies it (lbterm/FLOWS.md, row t6)
      if ($mux -and $r.Mode -eq 'titles' -and $r.NamesGui) { Add-Failure "titles: $($r.NamesGui) background tabs' title pushes went to the GUI thread ($($r.NamesHere) applied on the client thread): each changes nothing but its title, folder and seqno" }
      # ...its alert is dropped there before it's handed, and the mux window's title kept there
      # (lbterm/FLOWS.md, What could go 13, parts 2 and 3)
      if ($mux -and $r.Mode -eq 'titles' -and $r.AlertsDropped) { Add-Failure "titles: $($r.AlertsDropped) background tabs' title alerts were handed to the GUI thread to be dropped in the window's mux subscription ($($r.AlertsClient) dropped on the client thread)" }
      if ($mux -and $r.Mode -in 'titles', 'repeat' -and $r.WindowGui) { Add-Failure "$($r.Mode): $($r.WindowGui) of the server's mux window titles were handed to the GUI thread ($($r.WindowKept) kept on the client thread): nothing in the test's config reads one" }
      # ...and its alert, which changes no row, caption or handler's input, goes no further than
      # the window's mux subscription (lbterm/FLOWS.md, rows t12 to t14)
      if ($r.Mode -eq 'titles' -and $r.AlertsIdle) { Add-Failure "titles: $($r.AlertsIdle) background tabs' title alerts reached the window's task to find nothing to do: the window's mux subscription drops those ($($r.AlertsDropped) dropped)" }
      if ($r.Mode -eq 'repeat') {
        # Each tab sends the title it has (its last ◐ or ◑): none is new
        $n = 0; foreach ($v in $r.Alerts.Values) { $n += $v }
        if ($n) { Add-Failure "repeat: the same title sent $($r.Titles) times in $Working tabs raised $n alerts: $(($r.Alerts.GetEnumerator() | ForEach-Object { "$($_.Key) $($_.Value)" }) -join ', ')" }
      } elseif ($r.Titles) {
        $raised = [int]$r.Alerts['WindowTitleChanged'] + [int]$r.Alerts['IconTitleChanged']
        if ($raised -gt [Math]::Ceiling(1.1 * $r.Titles)) { Add-Failure "$($r.Mode): $($r.Titles) titles raised $raised title alerts (WindowTitleChanged $([int]$r.Alerts['WindowTitleChanged']), IconTitleChanged $([int]$r.Alerts['IconTitleChanged'])): one a title" }
      }
    }
  }
  [pscustomobject]@{ Mux = $mux; Tabs = $Tabs; Working = $Working; PerTitleMs = $perTitle; PerFrameMs = $perFrame; TitleTasks = $titleTasks; RepeatTasks = $repeatTasks; Cost = $cost; Counted = $counted; Checks = $checks } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "title-storm$(if ($Local) { ' (local)' }): $Working working tabs' titles run $(if ($mux) { "no task of the window's on the GUI thread" } else { "the window's tasks for their output alone, none for their alerts" }), raise one alert each, paint and update nothing and send nothing back; a title shows at once"
