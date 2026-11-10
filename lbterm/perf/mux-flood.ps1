#Requires -Version 7
# Variant: -ServerBin installed -Compat
# Variant: -GuiBin installed -Compat
# Variant: -Kind alerts
# Covers: wezterm-mux-server-impl/src/dispatch.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/lbterm_flow.rs, wezterm-mux-server-impl/src/local.rs, wezterm-client/src/client.rs, wezterm-client/src/lbterm_backlog.rs, wezterm-client/src/pane/*.rs, mux/src/lib.rs, mux/src/lbterm_output.rs, mux/src/localpane.rs, codec/src/lib.rs
<#
What a mux domain does when its tabs print faster than its window can take: -Flood tabs printing
without a pause, each its title too as it goes, behind a tab in view that echoes the keys posted
to it, every tab in a test mux server of its own (a socket under the test's folder; never
Lachlan's), its window drawn as Lachlan's (New-TestConfig) but with -Scrollback lines of
scrollback, 5,000 unless asked, so a window's line cache, which holds as many lines a pane as its
scrollback does, fills within the test and its bound shows, and without the scrollbar and the
scroll strip unless -Marks: their prompt marks read the whole scrollback of the tab in view on a
mux domain, fetching every line the window doesn't hold (two thirds of the GUI thread with a
flooding tab in view, 2026-10-31), which isn't what this measures. On 2026-10-29 six such tabs
grew a test server to 29.5 GB and its window to 11.5 GB in about seven minutes: nothing bounded
what the server queued for a client that couldn't keep up, what the window queued for its GUI
thread, or the lines the window kept.

Every 200 ms, on a timer of its own, the server's and the window's private bytes and working sets,
and the CPU of each and its main thread; it kills both, stopping the test, when either holds over
-MaxMemGB, and says when.

1. flood, for -Seconds: a key every -KeyEveryMs posted to the echoing tab, each timed from its
   post to the first frame showing its echo (the state file's time_ms and the pane's cursor).
2. stall: the window's GUI thread held still for -StallSec (under Windows' 5 s before it calls
   the window not responding), as one busy elsewhere would be, while the tabs print on: what
   each process holds meanwhile, and how soon a key posted after it echoes. Then -SwitchTrials
   times a key into the echo, and the GUI thread held -SwitchHoldMs while the server moves the
   window's focus to a flooding tab and straight back: the window must end on the echo, as the
   server does (not against another build's server, which a flood saturates).
3. view, for -ViewSec: a flooding tab brought into view: the frames the window paints a second,
   and how old the newest line the window holds of it is (each block of lines carries the time it
   was written; `cli get-text` from the window's own socket reads what the window has).
4. final: the tabs told to stop (a file each looks for between blocks), each then clearing its
   screen to five lines of its own, a title, a progress of its own (OSC 9;4), claude_status set
   orange (OSC 1337) and its cursor at row 7, column 5: within -SettleSec the window must hold
   every tab's final lines, title and cursor (its own cli), and its rail row the progress and the
   waiting state.
5. together: the echo tab in view, every flooding tab writes a screen of -Rows lines of characters
   that hardly compress and a title at the same moment (one event they all wait on) and goes quiet,
   -TogetherRounds times: within -TogetherSec the window's rail must show every tab's title, read
   from the state file alone, nothing sent to the window or its server while it waits (a request
   would wake the server's connection and let a push it held go). A server that stopped pushing
   once a socket took a round's pushes at once left a tab or three of six out for good
   (2026-10-31). Then the memory each process holds 3 s after.

It fails, unless -Measure, when the guard stops it, when either process's private bytes rise more
than -ServerGrowMB or -GuiGrowMB over what it held as the flood began, or grow more than
-GrowMBPerSec over the flood's second half, when a key's echo never shows or takes over -EchoMs
at the 95th percentile, when the first key after the stall doesn't echo within -EchoWaitSec, when
the window in view paints under -MinFps or holds lines over -LagMs old at the median, when a
tab's final state hasn't reached the window within -SettleSec, when a tab's screen written
with the others' hasn't within -TogetherSec, or when the window doesn't end the stall on the
echo. mux-flood.json in the output
folder has every number and every sample, and with a build that has them the window's counters
over the flood and over the view (flood-metrics.json, view-metrics.json; lbterm/PERF.md).

-ServerBin runs another build's lbterm-mux-server.exe for the domain, and -GuiBin another build's
lbterm-gui.exe for the window (installed: a copy of the daily LBTerm's, in the output folder, as
one running from the install folder would be taken for the daily one): Lachlan's window and server
are restarted apart, so each has to work with the other's build. -Compat, for those, floods with
two tabs for 10 s and fails only on a key that doesn't echo within -EchoWaitSec, a tab whose
last state, or screen written with the others', doesn't reach the window, or a window that
doesn't end the stall on the echo: a server from before this bound nothing, and grows under six.

  pwsh -File lbterm\perf\mux-flood.ps1 [-Flood 6] [-Seconds 30] [-ViewSec 5] [-Kind lines|alerts] [-Scrollback <lines>] [-Marks] [-Measure] [-Compat] [-ServerBin <build>|installed] [-GuiBin <build>|installed] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$Flood = 6,
  [int]$Seconds = 30,
  [int]$ViewSec = 5,
  [int]$SettleSec = 30,
  [double]$StallSec = 3,
  # The switch away and back after the stall: how many times, and how long the GUI thread is held
  [int]$SwitchTrials = 6,
  [int]$SwitchHoldMs = 600,
  [int]$KeyEveryMs = 150,
  [int]$Cols = 120,
  [int]$Rows = 40,
  # The scrollback each pane keeps, server and window (1,000 for -Kind alerts); 0: Lachlan's
  # (lbterm.lua's 50,000)
  [int]$Scrollback = 5000,
  # Lachlan's scrollbar and scroll strip, and their prompt marks, kept
  [switch]$Marks,
  # Another build's server or window: two tabs for 10 s, judged on the echo and the last state
  [switch]$Compat,
  # What the tabs flood with: lines (blocks of 200, a title with each), or alerts (blocks of 200
  # OSC 1337 user vars, each a new value, and a line and a title with each)
  [ValidateSet('lines', 'alerts')][string]$Kind = 'lines',
  [switch]$Measure,
  [double]$MaxMemGB = 2,
  [double]$ServerGrowMB = 150,
  [double]$GuiGrowMB = 250,
  # MB a second either process may grow by over the flood's second half
  [double]$GrowMBPerSec = 3,
  [double]$EchoMs = 250,
  [double]$MinFps = 20,
  [double]$LagMs = 500,
  [int]$EchoWaitSec = 5,
  # 5. together: how many times every tab writes a screen at the same moment, and how long the
  # window has to show them all
  [int]$TogetherRounds = 3,
  [double]$TogetherSec = 5,
  # More lines for the test's config
  [string]$Lua = ''
)
$ErrorActionPreference = 'Stop'
if ($Compat) {
  if (-not $PSBoundParameters.ContainsKey('Flood')) { $Flood = 2 }
  if (-not $PSBoundParameters.ContainsKey('Seconds')) { $Seconds = 10 }
}
# Alerts bring a line a block, so a window's line cache would still be filling at the flood's end
# with 5,000 lines of scrollback (it levels off after about 40 s, 2026-10-31): 1,000 fill in time
if ($Kind -eq 'alerts' -and -not $PSBoundParameters.ContainsKey('Scrollback')) { $Scrollback = 1000 }
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-flood' -Bin $Bin
function Copy-Installed([string]$Name, [string[]]$Files) {
  $dir = Join-Path $T.Out "installed-$Name"
  New-Item -ItemType Directory -Force $dir | Out-Null
  foreach ($f in $Files) { Copy-Item -Recurse (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $dir }
  $dir
}
if ($ServerBin -eq 'installed') { $ServerBin = Copy-Installed 'server' 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe' }
if ($GuiBin -eq 'installed') { $GuiBin = Copy-Installed 'gui' 'lbterm-gui.exe', 'conpty.dll', 'OpenConsole.exe', 'libEGL.dll', 'libGLESv2.dll', 'mesa' }
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$guiExe = if ($GuiBin) { Join-Path (Resolve-Path $GuiBin).Path 'lbterm-gui.exe' } else { $T.Gui }
if (-not (Test-Path $guiExe)) { throw "no lbterm-gui.exe in $GuiBin" }

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermMf {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("kernel32.dll")] static extern bool K32GetProcessMemoryInfo(IntPtr h, out PMC c, uint cb);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [StructLayout(LayoutKind.Sequential)] public struct PMC {
    public uint cb, PageFaults; public UIntPtr PeakWorkingSet, WorkingSet, QPeakPaged, QPaged, QPeakNonPaged, QNonPaged, Pagefile, PeakPagefile;
  }
  const uint THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000, PROCESS_VM_READ = 0x0010;
  public static double Rate(int ms) {
    ulong a, b; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out a);
    while (sw.ElapsedMilliseconds < ms) { }
    QueryThreadCycleTime(GetCurrentThread(), out b);
    return (b - a) / sw.Elapsed.TotalSeconds;
  }
  public static ulong ThreadCycles(int tid) {
    IntPtr h = OpenThread(THREAD_QUERY_LIMITED, false, (uint)tid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryThreadCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  // Cycles, private bytes, working set and its peak of a process, at once
  public static ulong[] Sample(int pid) {
    var r = new ulong[4];
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED | PROCESS_VM_READ, false, (uint)pid);
    if (h == IntPtr.Zero) return r;
    try {
      ulong c; QueryProcessCycleTime(h, out c); r[0] = c;
      PMC m; if (K32GetProcessMemoryInfo(h, out m, (uint)Marshal.SizeOf(typeof(PMC)))) {
        r[1] = (ulong)m.Pagefile; r[2] = (ulong)m.WorkingSet; r[3] = (ulong)m.PeakWorkingSet;
      }
      return r;
    } finally { CloseHandle(h); }
  }
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      int best = 0; DateTime first = DateTime.MaxValue;
      foreach (ProcessThread t in p.Threads) {
        try { if (t.StartTime < first) { first = t.StartTime; best = t.Id; } } catch (Exception) { }
      }
      return best;
    }
  }
  // The guard and the memory samples, on a timer of their own so a wait on an echo can't hold
  // them up: every periodMs both processes' private bytes and working sets, and once either is
  // over limit bytes, both killed and Tripped saying when. Each sample: seconds, phase, the
  // server's private bytes and working set, the window's, then the cycles of the server, its main
  // thread, the window and its GUI thread
  public static readonly System.Collections.Generic.List<double[]> Samples = new System.Collections.Generic.List<double[]>();
  public static volatile string Tripped;
  public static volatile int Phase;
  static System.Threading.Timer timer;
  static Stopwatch clock;
  public static void Watch(int gui, int server, int guiMain, int serverMain, double limit, int periodMs) {
    Tripped = null; lock (Samples) { Samples.Clear(); } clock = Stopwatch.StartNew();
    timer = new System.Threading.Timer(_ => {
      if (Tripped != null) return;
      var g = Sample(gui); var s = Sample(server);
      ulong sm = ThreadCycles(serverMain), gm = ThreadCycles(guiMain);
      double top = Math.Max(Math.Max(g[1], g[2]), Math.Max(s[1], s[2]));
      if (s[0] != 0 && g[0] != 0) {
        lock (Samples) { Samples.Add(new double[] { clock.Elapsed.TotalSeconds, Phase, s[1], s[2], g[1], g[2], s[0], sm, g[0], gm }); }
      }
      if (top > limit) {
        Tripped = string.Format("{0} over the limit at {1:F1} s: the server's private bytes {2:F0} MB (working set {3:F0}), the window's {4:F0} MB ({5:F0})",
          g[1] > limit || g[2] > limit ? "the window" : "the server", clock.Elapsed.TotalSeconds, s[1] / 1048576, s[2] / 1048576, g[1] / 1048576, g[2] / 1048576);
        foreach (var id in new[] { gui, server }) { try { Process.GetProcessById(id).Kill(); } catch (Exception) { } }
      }
    }, null, 0, periodMs);
  }
  public static void Unwatch() { var t = timer; timer = null; if (t != null) { t.Dispose(); } }
  public static double[][] Taken() { lock (Samples) { return Samples.ToArray(); } }
  [DllImport("kernel32.dll")] static extern uint SuspendThread(IntPtr h);
  [DllImport("kernel32.dll")] static extern uint ResumeThread(IntPtr h);
  // A thread held still for ms, as a GUI thread busy elsewhere would be: the window's GUI thread,
  // which owns its window, under Windows' 5 s before it calls one not responding
  public static bool Stall(int tid, int ms) {
    IntPtr h = OpenThread(0x0002, false, (uint)tid);
    if (h == IntPtr.Zero) return false;
    try {
      if (SuspendThread(h) == 0xFFFFFFFF) return false;
      try { System.Threading.Thread.Sleep(ms); } finally { ResumeThread(h); }
      return true;
    } finally { CloseHandle(h); }
  }
  // The same on a thread of this process's own, so the test can act meanwhile: Join it, then
  // Stalled says whether the thread was held
  public static volatile bool Stalled;
  public static System.Threading.Thread StallLater(int tid, int ms) {
    Stalled = false;
    var th = new System.Threading.Thread(() => { Stalled = Stall(tid, ms); });
    th.IsBackground = true;
    th.Start();
    return th;
  }
  public static void PostKey(IntPtr h, uint vk) {
    uint scan = MapVirtualKey(vk, 0);
    long down = 1 | ((long)(scan & 0xFF) << 16), up = down | 0xC0000000L;
    PostMessage(h, 0x0100, (IntPtr)vk, (IntPtr)unchecked((int)down));
    PostMessage(h, 0x0101, (IntPtr)vk, (IntPtr)unchecked((int)up));
  }
}
'@
$rate = [LbtermMf]::Rate(300)

$EchoProgram = New-PwshProgram -Script @'
[Console]::Out.Write('> ')
while ($true) { $k = [Console]::ReadKey($true); [Console]::Out.Write($k.KeyChar) }
'@

# Tab $Tag printing blocks of 200 lines without a pause, its title with each, until $Stop exists;
# then its final screen. Each line carries the time its block was written
# Its tokens are filled in with -creplace, which minds case, and the stop file's path last
# (PITFALLS 143): -replace ignores case, so the path, filled in first, had the 'kind' of e2e's
# mux-flood-kind-alerts folder turned to 'alerts' by the KIND token, and the tabs watched for a
# stop file that never came.
function New-FloodProgram([int]$Tag, [string]$Stop) {
  $pct = 10 + $Tag
  New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27; $st = "$e\"
$pad = 'x' * 64
$alerts = 'KIND' -eq 'alerts'
$n = 0
# The blocks written so far, for the test to read at its phases' ends: what the server was given,
# counted where it's made, so builds are compared per line (lbterm/PITFALLS.md, 181)
$printed = [IO.MemoryMappedFiles.MemoryMappedFile]::CreateOrOpen('XEVENTX-printed', 1024)
$count = $printed.CreateViewAccessor()
while (-not (Test-Path -LiteralPath 'STOP')) {
  $sb = [Text.StringBuilder]::new()
  $ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  [void]$sb.Append("$e]2;flood TAG block $n$st")
  if ($alerts) {
    for ($l = 0; $l -lt 200; $l++) { [void]$sb.Append("$e]1337;SetUserVar=flood=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("$n.$l")))$st") }
    [void]$sb.Append("flood TAG $n 0 t=$ms $pad`n")
  } else {
    for ($l = 0; $l -lt 200; $l++) { [void]$sb.Append("flood TAG $n $l t=$ms $pad`n") }
  }
  [Console]::Out.Write($sb.ToString())
  $n++
  $count.Write(TAG * 8, [long]$n)
}
$status = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('orange'))
$sb = [Text.StringBuilder]::new()
[void]$sb.Append("$e[2J$e[H")
for ($r = 1; $r -le 5; $r++) { [void]$sb.Append("$e[$r;1Hfinal TAG row $r") }
[void]$sb.Append("$e]2;flood TAG done$st$e]9;4;1;PCT$st$e]1337;SetUserVar=claude_status=$status$st$e[7;5H")
[Console]::Out.Write($sb.ToString())
# 5. together: every tab at the same moment (one event for them all), a screen of characters that
# hardly compress and a title, then quiet
for ($round = 1; $round -le XROUNDSX; $round++) {
  $go = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, "XEVENTX-$round")
  [void]$go.WaitOne()
  $rnd = [Random]::new(TAG * 1000 + $round)
  $sb = [Text.StringBuilder]::new()
  [void]$sb.Append("$e[2J$e[H")
  for ($y = 1; $y -le XLINESX; $y++) {
    [void]$sb.Append("$e[$y;1H")
    for ($x = 0; $x -lt XWIDTHX; $x++) { [void]$sb.Append([char]$rnd.Next(33, 127)) }
  }
  [void]$sb.Append("$e]2;flood TAG together $round$st$e[1;1H")
  [Console]::Out.Write($sb.ToString())
}
Start-Sleep 900
'@ -creplace 'TAG', $Tag -creplace 'PCT', $pct -creplace 'KIND', $Kind -creplace 'XROUNDSX', $TogetherRounds -creplace 'XEVENTX', $TogetherEvent -creplace 'XLINESX', $Rows -creplace 'XWIDTHX', ($Cols - 2) -creplace 'STOP', $Stop.Replace('$', '$$'))
}

# 5. together's event for round $Round, which every flooding tab waits on: set, the tabs write at once
$TogetherEvent = "Local\lbterm-mux-flood-$PID"
function Set-Together([int]$Round) {
  $ev = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, "$TogetherEvent-$Round")
  [void]$ev.Set()
  # Kept until the test ends, so the tabs that haven't opened it yet still find it set
  $script:TogetherHandles += $ev
}
$script:TogetherHandles = @()

# What the flooding tabs have written, each tab's count of blocks at its tag's place in a map they
# share with the test (New-FloodProgram), opened before they start and kept until the test ends.
# A block is 200 lines, or under -Kind alerts one line behind 200 user vars
$Printed = [IO.MemoryMappedFiles.MemoryMappedFile]::CreateOrOpen("$TogetherEvent-printed", 1024)
$PrintedView = $Printed.CreateViewAccessor()
function Get-Printed {
  $blocks = [long]0
  for ($i = 1; $i -le $Flood; $i++) { $blocks += $PrintedView.ReadInt64($i * 8) }
  [pscustomobject]@{ Ticks = [Diagnostics.Stopwatch]::GetTimestamp(); Blocks = $blocks }
}
# Into a phase's figures ($d): the lines the floods wrote a second between two reads, and the
# server's main thread a line, so a build that let the floods print more isn't taken for one that
# does more a line (lbterm/PITFALLS.md, 162 and 181)
function Add-Printed($d, $From, $To) {
  $sec = ($To.Ticks - $From.Ticks) / [Diagnostics.Stopwatch]::Frequency
  if ($sec -le 0) { return }
  $perBlock = if ($Kind -eq 'alerts') { 1 } else { 200 }
  $d.BlocksPerSec = [Math]::Round(($To.Blocks - $From.Blocks) / $sec, 1)
  $d.LinesPerSec = [Math]::Round(($To.Blocks - $From.Blocks) * $perBlock / $sec)
  if ($d.BlocksPerSec -gt 0) {
    $d.ServerMainUsPerBlock = [Math]::Round(1000 * $d.ServerMainMsPerSec / $d.BlocksPerSec, 1)
    $d.ServerMainUsPerLine = [Math]::Round(1000 * $d.ServerMainMsPerSec / ($d.BlocksPerSec * $perBlock), 3)
  }
}
# The server's push log (`lbterm::push` lines, lbterm_pushstats.rs) between two lengths of it:
# shown pane $Rid's pushes that found a change, those that waited for a frame's word (a build with
# the pace: MUX.md, "A shown pane's pace"), and the seconds those lines cover, each line its
# connection's last second or so, which the pushes a second are counted over (as mux-pace counts)
function Get-ServerLogLength { $l = Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$($results.ServerPid).txt"; if (Test-Path -LiteralPath $l) { (Get-Item -LiteralPath $l).Length } else { 0 } }
function Get-ShownPushes([long]$From, [long]$To, [int]$Rid) {
  $r = [ordered]@{ Lines = 0; Pushed = 0; Waited = 0; Seconds = 0.0 }
  $l = Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$($results.ServerPid).txt"
  if ($To -le $From -or -not (Test-Path -LiteralPath $l)) { return $r }
  $fs = [IO.File]::Open($l, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); $text = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  if ($text.Length -gt ($To - $From)) { $text = $text.Substring(0, [int]($To - $From)) }
  foreach ($line in ($text -split "`r?`n")) {
    if ($line -match "lbterm::push.* ([\d.]+) s: \d+ rounds.*; pane $Rid shown asked \d+ alone \d+ pushed (\d+)([^;]*)") {
      $r.Lines++; $r.Seconds += [double]$Matches[1]; $r.Pushed += [int]$Matches[2]
      if ($Matches[3] -match ' waited (\d+) ') { $r.Waited += [int]$Matches[1] }
    }
  }
  $r
}
function Format-Printed($d) {
  if ($null -eq $d.LinesPerSec) { return '' }
  "; the floods wrote $($d.LinesPerSec) lines/s ($($d.BlocksPerSec) blocks), the server's main thread $($d.ServerMainUsPerLine) us a line ($($d.ServerMainUsPerBlock) a block)"
}

function New-FloodConfig {
  $cfg = Join-Path $T.Out 'config\mux-flood.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
$(if ($Scrollback) { "config.scrollback_lines = $Scrollback" })
$(if (-not $Marks) { "config.enable_scroll_bar = false`nrawset(config, 'lbterm_scroll_strip', nil)" })
$Lua
config.unix_domains = { {
  name = 'floodmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  $cfg
}

$Stop = Join-Path $T.Out 'stop'
Remove-Item $Stop -ErrorAction SilentlyContinue

function Open-FloodWindow {
  $w = Open-TestWindow -ConfigFile (New-FloodConfig) -Domain 'floodmux' -Socket $T.Sock -TimeoutSec 30 -Exe $guiExe
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $shell = $panes[0].pane_id
  $echo = New-Tab -From $shell -Program $EchoProgram -Server
  $want = 2
  Wait-State { param($s) @($s.tabs).Count -eq $want } -What 'the echo tab in the window' -TimeoutSec 30 | Out-Null
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 1 } -What 'the echo tab alone' -TimeoutSec 30 | Out-Null
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $sp = [int]$server[0].ProcessId
  $w | Add-Member -NotePropertyName Main -NotePropertyValue ([LbtermMf]::MainThread($w.Pid))
  $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue $sp
  $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue ([LbtermMf]::MainThread($sp))
  $w | Add-Member -NotePropertyName Echo -NotePropertyValue $echo
  $w
}

# The flooding tabs, started at once: their server pane ids in tag order
function Start-Floods($w) {
  $ids = @()
  for ($i = 1; $i -le $Flood; $i++) { $ids += New-Tab -From $w.Echo -Program (New-FloodProgram $i $Stop) -NoActivate -Server }
  Wait-State { param($s) @($s.tabs).Count -eq 1 + $Flood } -What "the $Flood flooding tabs in the window" -TimeoutSec 60 | Out-Null
  $ids
}

$Phases = @('start', 'flood', 'stall', 'recover', 'view', 'final', 'together', 'after')
# Which phase the samples from now on are of
function Set-Phase([string]$Name) { [LbtermMf]::Phase = [Array]::IndexOf($Phases, $Name) }
# The watcher's samples so far, as rows; -Phase only those of one phase. -Cycles keeps each
# process's and main thread's cycles, for Get-Cpu
function Get-Samples([string]$Phase, [switch]$Cycles) {
  $want = if ($Phase) { [Array]::IndexOf($Phases, $Phase) } else { -1 }
  foreach ($r in [LbtermMf]::Taken()) {
    if ($want -ge 0 -and [int]$r[1] -ne $want) { continue }
    $row = [ordered]@{
      T = [Math]::Round($r[0], 2); Phase = $Phases[[int]$r[1]]
      ServerPrivateMB = [Math]::Round($r[2] / 1MB, 1); ServerWorkingSetMB = [Math]::Round($r[3] / 1MB, 1)
      GuiPrivateMB = [Math]::Round($r[4] / 1MB, 1); GuiWorkingSetMB = [Math]::Round($r[5] / 1MB, 1)
    }
    if ($Cycles) { $row.Server = $r[6]; $row.ServerMain = $r[7]; $row.Gui = $r[8]; $row.GuiMain = $r[9] }
    [pscustomobject]$row
  }
}

# The CPU each process and its main thread used a second over a phase's samples, first to last
function Get-Cpu([string]$Phase) {
  $rows = @(Get-Samples $Phase -Cycles)
  if ($rows.Count -lt 2) { return [ordered]@{ Seconds = 0 } }
  $a = $rows[0]; $b = $rows[-1]
  $s = $b.T - $a.T
  $per = { param([double]$cycles) [Math]::Round($cycles / $rate * 1000 / $s, 1) }
  [ordered]@{
    Seconds = [Math]::Round($s, 2)
    ServerCpuMsPerSec = & $per ($b.Server - $a.Server); ServerMainMsPerSec = & $per ($b.ServerMain - $a.ServerMain)
    GuiCpuMsPerSec = & $per ($b.Gui - $a.Gui); GuiMainMsPerSec = & $per ($b.GuiMain - $a.GuiMain)
  }
}
# Whether the guard has stopped the test: its words, or $null
function Get-Guard { [LbtermMf]::Tripped }

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

$results = [ordered]@{
  Window = $guiExe; Server = $serverExe; Flood = $Flood; Kind = $Kind; Scrollback = $Scrollback; Marks = [bool]$Marks; Compat = [bool]$Compat; Seconds = $Seconds; Locked = (Test-WorkstationLocked)
  Guard = $null; Flooding = $null; Stall = $null; View = $null; Final = $null; Together = $null; Samples = $null
}

# The peak of a field over samples, and how fast it grew over their second half (MB a second)
# The mux client thread a second, from a metrics reply: the window's busiest thread without a name
# other than its GUI thread (wezterm-client's client.rs starts it with a bare thread::spawn;
# async-io's is named), over the reply's own span; the process's and the GUI thread's figures
# don't split it out
function Get-ClientThread($m, $w) {
  $span = [double]$m.span_ms
  if (-not $span) { return $null }
  $t = @($m.threads | Where-Object { -not $_.name -and [long]$_.tid -ne [long]$w.Main })[0]
  if ($t) { [Math]::Round([double]$t.cpu_ms * 1000 / $span, 1) } else { 0 }
}

function Get-Peak($Rows, [string]$Field) { ($Rows | ForEach-Object $Field | Measure-Object -Maximum).Maximum }
function Get-Growth($Rows, [string]$Field) {
  $half = @($Rows | Select-Object -Skip ([int](@($Rows).Count / 2)))
  if ($half.Count -lt 2 -or $half[-1].T -le $half[0].T) { return $null }
  [Math]::Round(($half[-1].$Field - $half[0].$Field) / ($half[-1].T - $half[0].T), 1)
}

Invoke-Test {
  $w = Open-FloodWindow
  $results.Build = (Read-State).build
  $results.GuiPid = $w.Pid; $results.ServerPid = $w.ServerPid
  Set-Phase 'start'
  [LbtermMf]::Watch($w.Pid, $w.ServerPid, $w.Main, $w.ServerMain, $MaxMemGB * 1GB, 200)
  try {
    # Spawned without taking the view, so the echo stays the active tab: a cli call here could
    # wait as long as the server is behind
    $floods = Start-Floods $w

    # 1. flood: keys to the echo while the memory is watched; the window's counters (lbterm/PERF.md)
    # over it, from a build that has them
    Set-Phase 'flood'
    $printedFrom = Get-Printed
    $hasMetrics = $true; $metricsLines = @()
    try { Get-Metrics $w -Reset -TimeoutMs 2000 | Out-Null } catch { $hasMetrics = $false }
    $cursorX = { param($sw) $p = @($sw.panes | Where-Object active)[0]; if ($p) { [int]$p.cursor.x } else { -1 } }
    $h = Resolve-Handle $w
    $x0 = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
    $times = [Collections.Generic.List[double]]::new(); $late = 0
    # Each key whose echo didn't show, when it was posted and where the cursor was after the wait.
    # The next key is then expected one past where the cursor is: counting on from where it should
    # have been made one key that never showed fail every key after it on that line, 5 s each
    # (2026-11-01: 6 of 28 in a run whose window had one key it never showed, by its own counters)
    $lateKeys = [Collections.Generic.List[object]]::new()
    $phase = [Diagnostics.Stopwatch]::StartNew()
    $i = 0
    while ($phase.Elapsed.TotalSeconds -lt $Seconds -and -not (Get-Guard)) {
      $want = $x0 + $i + 1
      if ($want -ge $Cols - 2) {
        [LbtermMf]::PostKey($h, 0x0D)
        try {
          Wait-State { param($sw) (& $cursorX $sw) -in 0..3 }.GetNewClosure() -What 'the echo back at the line''s start' -TimeoutSec $EchoWaitSec -IntervalMs 5 | Out-Null
          $x0 = & $cursorX (Get-StateWindow (Get-Test).LastState -Hwnd ([long]$w.Handle)); $i = 0; $want = $x0 + 1
        } catch {
          if (Get-Guard) { break }
          $late++
          $lateKeys.Add([ordered]@{ Key = 'Enter'; At = [DateTimeOffset]::UtcNow.ToLocalTime().ToString('HH:mm:ss.fff'); Cursor = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)) })
        }
      }
      $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
      [LbtermMf]::PostKey($h, 0x58)
      Watch-Foreground
      $shown = { param($sw) (& $cursorX $sw) -ge $want }.GetNewClosure()
      try {
        Wait-State $shown -What "key $($i + 1)'s echo" -TimeoutSec $EchoWaitSec -IntervalMs 5 | Out-Null
        $times.Add([long](Get-Test).LastState.time_ms - $t0)
      } catch {
        if (Get-Guard) { break }
        $late++
        $seen = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
        $lateKeys.Add([ordered]@{ Key = $i + 1; At = [DateTimeOffset]::FromUnixTimeMilliseconds($t0).ToLocalTime().ToString('HH:mm:ss.fff'); Want = $want; Cursor = $seen })
        # The next key's echo is looked for one past the cursor the window shows now
        if ($seen -ge 0) { $x0 = $seen - ($i + 1) }
      }
      $i++
      $left = $KeyEveryMs - ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $t0)
      if ($left -gt 0) { Start-Sleep -Milliseconds $left }
    }
    $printedTo = Get-Printed
    $d = Get-Cpu 'flood'
    Add-Printed $d $printedFrom $printedTo
    $d.Keys = $times.Count + $late; $d.KeysLate = $late; $d.LateKeys = @($lateKeys)
    $d.EchoP50 = Get-Pct $times 0.5; $d.EchoP95 = Get-Pct $times 0.95; $d.EchoMax = ($times | Measure-Object -Maximum).Maximum
    $fl = @(Get-Samples 'flood')
    foreach ($f in 'ServerPrivateMB', 'ServerWorkingSetMB', 'GuiPrivateMB', 'GuiWorkingSetMB') { $d["${f}Peak"] = Get-Peak $fl $f }
    $d.ServerGrowthMBPerSec = Get-Growth $fl 'ServerPrivateMB'; $d.GuiGrowthMBPerSec = Get-Growth $fl 'GuiPrivateMB'
    $d.Guard = Get-Guard
    if ($hasMetrics -and -not $d.Guard) {
      try {
        $m = Get-Metrics $w
        $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out 'flood-metrics.json')
        $d.ClientThreadMsPerSec = Get-ClientThread $m $w
        $d.Metrics = [ordered]@{ Totals = $m.totals; Tasks = @($m.tasks | Select-Object -First 10); Messages = @($m.messages | Select-Object -First 5); Mux = @($m.mux | Select-Object -First 10) }
        $metricsLines = @(Format-Metrics $m -Top 6 | ForEach-Object { "  metrics: $_" })
      } catch { $metricsLines = @("  metrics: none ($($_.Exception.Message))") }
    }
    $results.Flooding = $d
    "flood ($Flood tabs printing without a pause, an echo in view, $($d.Seconds) s): server CPU $($d.ServerCpuMsPerSec) ms/s (main thread $($d.ServerMainMsPerSec)), private peak $($d.ServerPrivateMBPeak) MB (growing $($d.ServerGrowthMBPerSec) MB/s over the second half); window CPU $($d.GuiCpuMsPerSec) ms/s (GUI thread $($d.GuiMainMsPerSec)$(if ($null -ne $d.ClientThreadMsPerSec) { ", client thread $($d.ClientThreadMsPerSec)" })), private peak $($d.GuiPrivateMBPeak) MB (growing $($d.GuiGrowthMBPerSec) MB/s)$(Format-Printed $d); $($d.Keys) keys, echo p50 $($d.EchoP50) p95 $($d.EchoP95) max $($d.EchoMax) ms$(if ($late) { ", $late not shown within $EchoWaitSec s ($(@($lateKeys | ForEach-Object { "key $($_.Key) at $($_.At), the cursor at $($_.Cursor)$(if ($_.Want) { " for $($_.Want)" })" }) -join '; '))" })$(if ($d.Guard) { "; STOPPED: $($d.Guard)" })"
    $metricsLines

    if (-not (Get-Guard) -and $StallSec -gt 0) {
      # 2. stall: the window's GUI thread held still while the tabs print on, as one busy
      # elsewhere would be: what the server and the window hold meanwhile, and a key's echo after
      Set-Phase 'stall'
      if (-not [LbtermMf]::Stall($w.Main, [int]($StallSec * 1000))) { throw "couldn't hold the window's GUI thread ($($w.Main))" }
      Set-Phase 'recover'
      $x = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
      $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
      if ($x -le 5) {
        [LbtermMf]::PostKey($h, 0x58)
        $after = { param($sw) (& $cursorX $sw) -ge $x + 1 }.GetNewClosure()
      } else {
        [LbtermMf]::PostKey($h, 0x0D)
        $after = { param($sw) (& $cursorX $sw) -in 0..3 }.GetNewClosure()
      }
      $echo = $null
      try {
        Wait-State $after -What 'the first echo after the stall' -TimeoutSec $EchoWaitSec -IntervalMs 5 | Out-Null
        $echo = [long](Get-Test).LastState.time_ms - $t0
      } catch { if (Get-Guard) { } }
      Start-Sleep -Milliseconds 500
      $st = @(Get-Samples 'stall'); $rc = @(Get-Samples 'recover'); $both = @($st) + @($rc)
      $results.Stall = [ordered]@{
        Seconds = $StallSec; EchoAfterMs = $echo
        ServerPrivateMBBefore = @(Get-Samples 'flood')[-1].ServerPrivateMB; ServerPrivateMBPeak = Get-Peak $both 'ServerPrivateMB'
        GuiPrivateMBBefore = @(Get-Samples 'flood')[-1].GuiPrivateMB; GuiPrivateMBPeak = Get-Peak $both 'GuiPrivateMB'
        Guard = Get-Guard
      }
      $sd = $results.Stall
      "stall (the window's GUI thread held $StallSec s while the tabs printed on): server private $($sd.ServerPrivateMBBefore) MB before, $($sd.ServerPrivateMBPeak) at most; window $($sd.GuiPrivateMBBefore) MB before, $($sd.GuiPrivateMBPeak) at most; the first key after echoed in $(if ($null -ne $echo) { "$echo ms" } else { "more than $EchoWaitSec s" })$(if ($sd.Guard) { "; STOPPED: $($sd.Guard)" })"

      # The switch away and back: a key into the echo, then, the GUI thread held -SwitchHoldMs as a
      # busy one would be, the server moves the window's focus to a flooding tab and straight back
      # to the echo, as Lachlan switching away and back would. The window must end on the echo, as
      # the server does: one that took the typed pane's focus change ahead of the other's, with its
      # pushes, ended on the flooding tab (the review's probe, 2026-10-31: 12 trials of 12)
      # Not against another build's server (-ServerBin): one from before this branch's bound can't
      # keep up with even two flooding tabs (2026-10-31: its main thread busy 913 ms of each second,
      # keys not echoed in 5 s), so it may never get to the switch back in time, which says nothing
      # of the window
      $echoId = $w.Echo
      $switch = @()
      $trials = if ($ServerBin) { 0 } else { $SwitchTrials }
      for ($k = 1; $k -le $trials -and -not (Get-Guard); $k++) {
        [LbtermMf]::PostKey($h, 0x58)
        Start-Sleep -Milliseconds 300
        $held = [LbtermMf]::StallLater($w.Main, $SwitchHoldMs)
        Start-Sleep -Milliseconds 20
        Invoke-Cli activate-pane --pane-id $floods[0] -Server -Check | Out-Null
        Invoke-Cli activate-pane --pane-id $echoId -Server -Check | Out-Null
        $held.Join()
        if (-not [LbtermMf]::Stalled) { throw "couldn't hold the window's GUI thread ($($w.Main))" }
        $endedOn = $null
        try {
          Start-Sleep -Milliseconds 1000
          Wait-State { param($sw) @($sw.panes | Where-Object { $_.active -and $_.server_id -eq $echoId }).Count -eq 1 }.GetNewClosure() -What 'the echo in view after the switch away and back' -TimeoutSec $EchoWaitSec | Out-Null
        } catch {
          $endedOn = @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).panes | Where-Object active)[0].server_id
          if ($null -eq $endedOn) { $endedOn = 'none' }
          # The echo back in view for what follows, as the server has it
          Invoke-Cli activate-pane --pane-id $echoId -Server -Check | Out-Null
          try {
            Wait-State { param($sw) @($sw.panes | Where-Object { $_.active -and $_.server_id -eq $echoId }).Count -eq 1 }.GetNewClosure() -What 'the echo in view again' -TimeoutSec 10 | Out-Null
          } catch { "switch $($k): the echo wasn't in view again 10 s after it was activated once more" }
        }
        $switch += [ordered]@{ Trial = $k; EndedOn = $endedOn }
      }
      $wrong = @($switch | Where-Object { $null -ne $_.EndedOn })
      $sd.Switches = $switch.Count
      $sd.SwitchesWrong = @($wrong | ForEach-Object { $_.EndedOn })
      if ($trials) { "switch (the server moving focus to pane $($floods[0]) and back to the echo, pane $echoId, while the GUI thread was held $SwitchHoldMs ms): the window ended on the echo $($switch.Count - $wrong.Count) times of $($switch.Count)$(if ($wrong) { ", on pane $($sd.SwitchesWrong -join ', ') the others" })" }
      else { "switch: not tried against another build's server (-ServerBin)" }
    }

    if (-not (Get-Guard)) {
      # 3. view: a flooding tab in view, its frames and how old the window's newest line of it is
      Set-Phase 'view'
      $printedFrom = Get-Printed
      Invoke-Cli activate-pane --pane-id $floods[0] -Server -Check | Out-Null
      $first = $floods[0]
      $logFrom = Get-ServerLogLength
      $vw = Wait-State { param($sw) @($sw.panes | Where-Object { $_.active -and $_.server_id -eq $first }).Count -eq 1 }.GetNewClosure() -What 'the first flooding tab in view' -TimeoutSec 10
      $guiPane = @($vw.panes | Where-Object active)[0].id
      if ($hasMetrics) { try { Get-Metrics $w -Reset -TimeoutMs 2000 | Out-Null } catch { } }
      $f0 = Read-State
      $lags = [Collections.Generic.List[double]]::new()
      $calls = [Collections.Generic.List[double]]::new()
      $vclock = [Diagnostics.Stopwatch]::StartNew()
      while ($vclock.Elapsed.TotalSeconds -lt $ViewSec -and -not (Get-Guard)) {
        Start-Sleep -Milliseconds 400
        # The window answers get-text on its GUI thread, some time between the call and its
        # answer: the line's age is taken at the middle, and the call's own time kept beside it
        $asked = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $text = (Invoke-Cli get-text --pane-id $guiPane) -join "`n"
        $answered = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $calls.Add($answered - $asked)
        $newest = ([regex]::Matches($text, 't=(\d{13})') | ForEach-Object { [long]$_.Groups[1].Value } | Measure-Object -Maximum).Maximum
        if ($newest) { $lags.Add(($asked + $answered) / 2 - $newest) }
      }
      $f1 = Read-State
      $viewMetrics = @()
      if ($hasMetrics) {
        try {
          $vm = Get-Metrics $w
          $vm | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out 'view-metrics.json')
          $viewClient = Get-ClientThread $vm $w
          $viewMetrics = @(Format-Metrics $vm -Top 4 | Select-Object -First 9 | ForEach-Object { "  metrics: $_" })
        } catch { }
      }
      $printedTo = Get-Printed
      $logTo = Get-ServerLogLength
      $v = Get-Cpu 'view'
      Add-Printed $v $printedFrom $printedTo
      # The pane in view's pushes a second against the frames (FLOWS.md's pane-output flow, What
      # could go 12), from the server's push log over the phase
      $sp = Get-ShownPushes $logFrom $logTo $first
      $v.ShownPushesPerSec = if ($sp.Seconds) { [Math]::Round($sp.Pushed / $sp.Seconds, 1) } else { $null }
      $v.ShownPushesWaited = $sp.Waited
      if ($null -ne $viewClient) { $v.ClientThreadMsPerSec = $viewClient }
      $v.FramesPerSec = [Math]::Round(([long]$f1.frame - [long]$f0.frame) / (([long]$f1.time_ms - [long]$f0.time_ms) / 1000), 1)
      $v.LagP50 = Get-Pct $lags 0.5; $v.LagMax = ($lags | Measure-Object -Maximum).Maximum; $v.LagSamples = $lags.Count
      $v.GetTextP50 = Get-Pct $calls 0.5; $v.GetTextMax = ($calls | Measure-Object -Maximum).Maximum
      $vs = @(Get-Samples 'view')
      foreach ($f in 'ServerPrivateMB', 'GuiPrivateMB') { $v["${f}Peak"] = Get-Peak $vs $f }
      $v.Guard = Get-Guard
      $results.View = $v
      "view (a flooding tab in view, $($v.Seconds) s): $($v.FramesPerSec) frames/s, the pane in view $(if ($null -ne $v.ShownPushesPerSec) { "$($v.ShownPushesPerSec) pushes/s ($($v.ShownPushesWaited) waited for a frame's word)" } else { 'with no push in the log' }), the newest line the window holds $($v.LagP50) ms old at the median ($($v.LagMax) at most, $($v.LagSamples) looks, each get-text $($v.GetTextP50) ms at the median, $($v.GetTextMax) at most); server CPU $($v.ServerCpuMsPerSec) ms/s (main thread $($v.ServerMainMsPerSec)), window CPU $($v.GuiCpuMsPerSec) ms/s (GUI thread $($v.GuiMainMsPerSec)$(if ($null -ne $v.ClientThreadMsPerSec) { ", client thread $($v.ClientThreadMsPerSec)" })); private peaks: server $($v.ServerPrivateMBPeak) MB, window $($v.GuiPrivateMBPeak) MB$(Format-Printed $v)$(if ($v.Guard) { "; STOPPED: $($v.Guard)" })"
      $viewMetrics
    }

    if (-not (Get-Guard)) {
      # 4. final: every tab stops on a screen of its own, which the window must end with
      Set-Phase 'final'
      Invoke-Cli activate-pane --pane-id $w.Echo -Server -Check | Out-Null
      Set-Content -LiteralPath $Stop 'stop'
      $stopped = [Diagnostics.Stopwatch]::StartNew()
      $missing = @()
      $seenAt = $null
      while ($stopped.Elapsed.TotalSeconds -lt $SettleSec -and -not (Get-Guard)) {
        $missing = @()
        $gp = @(Get-Panes)
        $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
        for ($i = 1; $i -le $Flood; $i++) {
          $p = @($gp | Where-Object { $_.title -eq "flood $i done" })
          if (-not $p) { $missing += "tab $i's title"; continue }
          $p = $p[0]
          if ($p.cursor_x -ne 4 -or $p.cursor_y -ne 6) { $missing += "tab $i's cursor (at $($p.cursor_x),$($p.cursor_y))" }
          $row = @($s.rail.rows | Where-Object { $_.title -eq "flood $i done" })
          if (-not $row) { $missing += "tab $i's rail row" }
          elseif ($row[0].state -ne 'waiting') { $missing += "tab $i's claude_status ($($row[0].state))" }
          $text = (Invoke-Cli get-text --pane-id $p.pane_id) -join "`n"
          for ($r = 1; $r -le 5; $r++) { if ($text -notmatch "final $i row $r\b") { $missing += "tab $i's row $r"; break } }
          if ($text -match 'flood \d+ \d+ \d+ t=') { $missing += "tab $i's flood lines still on its screen" }
        }
        if (-not $missing) { $seenAt = [Math]::Round($stopped.Elapsed.TotalSeconds, 2); break }
        Start-Sleep -Milliseconds 250
      }
      if ($seenAt) {
        # OSC 9;4's progress is the open tab's panes' in the state file (the rail's bar is OSC
        # 7501's): each tab in view in turn
        for ($i = 1; $i -le $Flood; $i++) {
          $sid = $floods[$i - 1]; $want = "$(10 + $i)%"
          Invoke-Cli activate-pane --pane-id $sid -Server -Check | Out-Null
          try {
            Wait-State { param($sw) @($sw.panes | Where-Object { $_.active -and $_.server_id -eq $sid -and $_.progress -eq $want }).Count -eq 1 }.GetNewClosure() -What "tab $i's progress" -TimeoutSec 5 | Out-Null
          } catch { $missing += "tab $i's progress" }
        }
        if ($missing) { $seenAt = $null }
      }
      $results.Final = [ordered]@{ ShownAfterSec = $seenAt; Missing = $missing; Guard = (Get-Guard) }
      if ($seenAt) { "final: every tab's last screen, title, cursor and claude_status in the window $seenAt s after they were told to stop, and its progress once in view" }
      else { "final: after $([Math]::Round($stopped.Elapsed.TotalSeconds, 1)) s the window still lacks $($missing -join ', ')$(if (Get-Guard) { "; STOPPED: $(Get-Guard)" })" }

      # 5. together: every tab, out of view, writes a screen and a title at the same moment and goes
      # quiet. Judged from the state file alone: a cli call to the window or its server is a
      # request, which wakes the server's connection to the window and would let a push it held go
      Set-Phase 'together'
      $echoId = $w.Echo
      Invoke-Cli activate-pane --pane-id $echoId -Server -Check | Out-Null
      Wait-State { param($sw) @($sw.panes | Where-Object { $_.active -and $_.server_id -eq $echoId }).Count -eq 1 }.GetNewClosure() -What 'the echo tab in view again' -TimeoutSec 10 | Out-Null
      Start-Sleep 1
      $together = @()
      for ($round = 1; $round -le $TogetherRounds -and -not (Get-Guard); $round++) {
        Set-Together $round
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $seen = @{}
        while ($clock.Elapsed.TotalSeconds -lt $TogetherSec -and -not (Get-Guard)) {
          $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
          for ($i = 1; $i -le $Flood; $i++) {
            if (-not $seen.ContainsKey($i) -and @($s.rail.rows | Where-Object { $_.title -eq "flood $i together $round" }).Count) { $seen[$i] = $clock.ElapsedMilliseconds }
          }
          if ($seen.Count -eq $Flood) { break }
          Start-Sleep -Milliseconds 20
        }
        $lacking = @(1..$Flood | Where-Object { -not $seen.ContainsKey($_) })
        $lastMs = if ($seen.Count) { ($seen.Values | Measure-Object -Maximum).Maximum } else { $null }
        $together += [ordered]@{ Round = $round; LastMs = $lastMs; Missing = $lacking }
        "together, round ${round}: $(if ($lacking) { "after $TogetherSec s the window still lacks the screen and title of tab $($lacking -join ', ')" } else { "every tab's screen and title in the window within $lastMs ms" })$(if (Get-Guard) { "; STOPPED: $(Get-Guard)" })"
        Start-Sleep -Milliseconds 500
      }
      $results.Together = $together

      # The memory each holds once the window has every tab's last state, and a few seconds on
      Set-Phase 'after'
      Start-Sleep 3
      $last = @(Get-Samples)[-1]
      $results.Final.ServerPrivateMB = $last.ServerPrivateMB; $results.Final.GuiPrivateMB = $last.GuiPrivateMB
      "after: 3 s on the server holds $($last.ServerPrivateMB) MB private, the window $($last.GuiPrivateMB) MB"
    }
  } finally {
    [LbtermMf]::Unwatch()
    $results.Guard = Get-Guard
    $results.Samples = @(Get-Samples)
    [pscustomobject]$results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'mux-flood.json')
  }
  Stop-TestWindow $w
  Stop-TestServers

  if ($Compat -and -not $Measure) {
    $fd = $results.Flooding
    if ($results.Guard) { Add-Failure "the guard stopped the test, $($results.Guard)" }
    if ($fd.KeysLate) { Add-Failure "$($fd.KeysLate) of $($fd.Keys) keys' echoes didn't show within $EchoWaitSec s beside the flood" }
    if ($results.Final -and -not $results.Final.ShownAfterSec) { Add-Failure "the window didn't end with every tab's last state within $SettleSec s: $($results.Final.Missing -join ', ')" }
    foreach ($r in @($results.Together | Where-Object { $_.Missing })) { Add-Failure "round $($r.Round) of the tabs writing at once: the window lacked the screen and title of tab $($r.Missing -join ', ') after $TogetherSec s" }
    if ($results.Stall -and @($results.Stall.SwitchesWrong).Count) { Add-Failure "the server moved focus to a flooding tab and back to the echo while the window's GUI thread was held, and the window ended on pane $($results.Stall.SwitchesWrong -join ', '), not the echo, $(@($results.Stall.SwitchesWrong).Count) times of $($results.Stall.Switches)" }
  } elseif (-not $Measure) {
    if ($results.Guard) { Add-Failure "the guard stopped the test, $($results.Guard): its tabs' output outran it" }
    # What each held as the flood began, its tabs open: what it grew by from there is judged
    $began = @($results.Samples | Where-Object { $_.Phase -eq 'flood' })[0]
    $peakS = Get-Peak $results.Samples 'ServerPrivateMB'
    $peakG = Get-Peak $results.Samples 'GuiPrivateMB'
    if ($began -and $peakS - $began.ServerPrivateMB -gt $ServerGrowMB) { Add-Failure "the server's private bytes rose to $peakS MB, $([Math]::Round($peakS - $began.ServerPrivateMB, 1)) over the $($began.ServerPrivateMB) it held as the flood began, more than $ServerGrowMB" }
    if ($began -and $peakG - $began.GuiPrivateMB -gt $GuiGrowMB) { Add-Failure "the window's private bytes rose to $peakG MB, $([Math]::Round($peakG - $began.GuiPrivateMB, 1)) over the $($began.GuiPrivateMB) it held as the flood began, more than $GuiGrowMB" }
    $fd = $results.Flooding
    if ($fd.ServerGrowthMBPerSec -gt $GrowMBPerSec) { Add-Failure "the server grew $($fd.ServerGrowthMBPerSec) MB a second over the flood's second half, over $GrowMBPerSec" }
    if ($fd.GuiGrowthMBPerSec -gt $GrowMBPerSec) { Add-Failure "the window grew $($fd.GuiGrowthMBPerSec) MB a second over the flood's second half, over $GrowMBPerSec" }
    if ($fd.KeysLate) { Add-Failure "$($fd.KeysLate) of $($fd.Keys) keys' echoes didn't show within $EchoWaitSec s beside the flood" }
    if ($fd.EchoP95 -gt $EchoMs) { Add-Failure "a key's echo took $($fd.EchoP95) ms at the 95th percentile beside the flood, over $EchoMs" }
    if ($results.Stall -and $null -eq $results.Stall.EchoAfterMs) { Add-Failure "a key posted after the window's GUI thread was held $StallSec s didn't echo within $EchoWaitSec s" }
    if ($results.Stall -and @($results.Stall.SwitchesWrong).Count) { Add-Failure "the server moved focus to a flooding tab and back to the echo while the window's GUI thread was held, and the window ended on pane $($results.Stall.SwitchesWrong -join ', '), not the echo, $(@($results.Stall.SwitchesWrong).Count) times of $($results.Stall.Switches)" }
    if ($results.View) {
      if ($results.View.FramesPerSec -lt $MinFps) { Add-Failure "a flooding tab in view painted $($results.View.FramesPerSec) frames a second, under $MinFps" }
      if ($null -eq $results.View.LagP50 -or $results.View.LagP50 -gt $LagMs) { Add-Failure "the newest line the window held of a flooding tab in view was $($results.View.LagP50) ms old at the median, over $LagMs" }
    }
    if ($results.Final -and -not $results.Final.ShownAfterSec) { Add-Failure "the window didn't end with every tab's last state within $SettleSec s: $($results.Final.Missing -join ', ')" }
    foreach ($r in @($results.Together | Where-Object { $_.Missing })) { Add-Failure "round $($r.Round) of the tabs writing at once: the window lacked the screen and title of tab $($r.Missing -join ', ') after $TogetherSec s" }
  }
} -Passed "mux-flood: $Flood tabs printing $(if ($Kind -eq 'alerts') { 'user vars and titles' } else { 'lines' }) without a pause on a mux domain: $(if ($Compat) { 'keys echo and every tab''s last state and screens written together reach the window' } else { 'both processes stay bounded, keys echo, a tab in view keeps up and every tab''s last state and screens written together reach the window' })"
