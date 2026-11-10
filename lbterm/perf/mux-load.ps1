#Requires -Version 7
# Variant: -ServerBin installed
# Variant: -GuiBin installed
# Covers: codec/src/lib.rs, wezterm-mux-server-impl/src/dispatch.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/local.rs, wezterm-client/src/client.rs
<#
What a busy mux domain costs its lbterm-mux-server and its window, the way Lachlan's 8 or more
Claude Code tabs keep his busy: -Claude tabs drawing as a working Claude Code session does (a
stand-in: its status block, spinner and clock redrawn every 50 ms and a line of its answer every
100 ms, on the alternate screen) and -Burst tabs printing forty lines every 5 ms, faster than
anything should, all behind a tab in view that echoes the keys posted to it, every tab in a test
mux server of its own (a socket under the test's folder; never Lachlan's). Drawn as Lachlan's
window is (New-TestConfig). After a few seconds to settle, over -Seconds:

- the server: its CPU, its main thread's (where every connection's PDUs are encoded and
  decoded, its sessions run and its panes' changes are worked out), its page faults, its
  reads, writes and other I/O calls, and its memory at the end and at its peak;
- the window: its CPU, its GUI thread's, its page faults, its memory;
- a key every -KeyEveryMs posted to the echoing tab: its time from the post to the first frame
  showing its echo (the state file's time_ms and the pane's cursor).

-Count runs it all again with WEZTERM_LOG=info,codec=debug in the window and its server, and counts
the PDUs each sent and their bytes, by type, and how many the server compressed, from their logs
(the logging costs both, so that pass's CPU isn't the measure; the wire's bytes are the same with
or without it).

-ServerBin runs another build's lbterm-mux-server.exe for the domain, and -GuiBin another build's
lbterm-gui.exe for the window (installed: a copy of the daily LBTerm's, in the output folder, as
one running from the install folder would be taken for the daily one): this build's window
against an older server, or an older window against this build's server. Lachlan's window and
server are restarted apart, so each has to work with the other's build: the keys must echo and
every tab must reach the window. The window's counters over the span (lbterm/PERF.md) say what
the pair did with the tabs out of view (lbterm/MUX.md, "Tabs out of view: output held at the
server"): this build's window says what it shows (SetShownPanes) as it attaches and makes its
tabs, and this build's server then holds the busy tabs' lines (PaneHeld, the pushes a second the
window took next to nothing but the echo's), which shows in the server's main thread; on a server
that answers ErrorResponse (an older one) it says no more, and on either side of an older build
every tab is pushed as before and no PaneHeld comes. The window's log must say no reconnect and
no domain detached, whichever builds run (PITFALLS 188).

It stops the test when the window or the server holds over -MaxMemGB, private bytes or working set,
read every 200 ms on a timer of its own (the harness's Start-MemoryGuard): on a mux domain six tabs
printing without a pause grew a test server to 29.5 GB and its window to 11.5 GB in about seven
minutes (2026-10-29), nothing bounding what the server queues for a client that can't keep up.

It fails, unless -Measure, when a key's echo never shows or takes over -EchoMs at the 95th
percentile, or when a tab doesn't reach the window. What a PDU costs to encode and decode is
held by codec's tests (codec/tests/lbterm_codec_cost.rs fails on a zstd context made for each
PDU again), not here: the server's page faults and CPU go with how fast the tabs print and what
else the PC runs. mux-load.json in the output folder has every number.

  pwsh -File lbterm\perf\mux-load.ps1 [-Claude 6] [-Burst 2] [-Seconds 20] [-Count] [-Measure] [-ServerBin <build>|installed] [-GuiBin <build>|installed] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$Claude = 6,
  [int]$Burst = 2,
  [int]$Seconds = 20,
  [int]$KeyEveryMs = 150,
  [int]$Cols = 120,
  [int]$Rows = 40,
  [switch]$Count,
  [switch]$Measure,
  [double]$MaxMemGB = 2,
  [int]$EchoMs = 1000
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-load' -Bin $Bin
# The installed build's $Files, copied into the test's folder: a server or window running from the
# install folder would be taken for the daily one (the Warden's registry and `lbterm.ps1 logs` go
# by the exe's folder)
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
public static class LbtermMl {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("kernel32.dll")] static extern bool GetProcessIoCounters(IntPtr h, out IO c);
  [DllImport("kernel32.dll")] static extern bool K32GetProcessMemoryInfo(IntPtr h, out PMC c, uint cb);
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [StructLayout(LayoutKind.Sequential)] public struct IO { public ulong Reads, Writes, Others, ReadBytes, WriteBytes, OtherBytes; }
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
  // Cycles, page faults, I/O calls and memory of a process, at once
  public static ulong[] Sample(int pid) {
    var r = new ulong[8];
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED | PROCESS_VM_READ, false, (uint)pid);
    if (h == IntPtr.Zero) return r;
    try {
      ulong c; QueryProcessCycleTime(h, out c); r[0] = c;
      PMC m; if (K32GetProcessMemoryInfo(h, out m, (uint)Marshal.SizeOf(typeof(PMC)))) {
        r[1] = m.PageFaults; r[5] = (ulong)m.Pagefile; r[6] = (ulong)m.PeakWorkingSet; r[7] = (ulong)m.WorkingSet;
      }
      IO io; if (GetProcessIoCounters(h, out io)) { r[2] = io.Reads; r[3] = io.Writes; r[4] = io.Others; }
      return r;
    } finally { CloseHandle(h); }
  }
  // The process's first thread, its main one: the one that started first
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      int best = 0; DateTime first = DateTime.MaxValue;
      foreach (ProcessThread t in p.Threads) {
        try { if (t.StartTime < first) { first = t.StartTime; best = t.Id; } } catch (Exception) { }
      }
      return best;
    }
  }
  // A key pressed and let go, posted as the harness's Send-Key does
  public static void PostKey(IntPtr h, uint vk) {
    uint scan = MapVirtualKey(vk, 0);
    long down = 1 | ((long)(scan & 0xFF) << 16), up = down | 0xC0000000L;
    PostMessage(h, 0x0100, (IntPtr)vk, (IntPtr)unchecked((int)down));
    PostMessage(h, 0x0101, (IntPtr)vk, (IntPtr)unchecked((int)up));
  }
}
'@

$rate = [LbtermMl]::Rate(300)
$ms = { param([ulong]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }

$Progs = @{
  # Each key it's given, written back
  echo   = New-PwshProgram -Script @'
[Console]::Out.Write('> ')
while ($true) { $k = [Console]::ReadKey($true); [Console]::Out.Write($k.KeyChar) }
'@
  # A stand-in for a working Claude Code session: on the alternate screen, its status block, its
  # spinner and clock redrawn every 50 ms, and a line of its answer every 100 ms scrolling the
  # conversation above it
  claude = New-PwshProgram -Script @'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$h = [Console]::WindowHeight; $w = [Console]::WindowWidth
$area = [Math]::Max(4, $h - 8)
[Console]::Out.Write("$e[?1049h$e[2J$e[1;${area}r")
$spin = [char[]]@(0xB7, 0x2722, 0x2733, 0x2736, 0x273B, 0x273D)
$words = 'the codec reads a frame header from its buffer and zstd keeps one context for the thread so each push costs less' -split ' '
$clock = [Diagnostics.Stopwatch]::StartNew()
$rule = [string][char]0x2500 * $w
for ($i = 0; ; $i++) {
  $sb = [Text.StringBuilder]::new()
  if ($i % 2 -eq 0) {
    $line = '  '
    for ($j = 0; $line.Length -lt $w - 12; $j++) { $line += "$e[38;5;$(250 - ($j % 3) * 4)m" + $words[($i + $j) % $words.Count] + ' ' }
    [void]$sb.Append("$e[$area;1H`n$line$e[0m")
  }
  $row = $area + 2
  [void]$sb.Append("$e[$row;1H$e[38;5;174m$($spin[$i % 6]) Working$([char]0x2026) $e[38;5;246m($([int]$clock.Elapsed.TotalSeconds)s $([char]0xB7) $([char]0x2193) $($i * 7) tokens $([char]0xB7) esc to interrupt)$e[0m$e[K")
  [void]$sb.Append("$e[$($row + 1);1H$e[38;5;240m$rule$e[0m$e[$($row + 2);1H$([char]0x276F) $e[K$e[$($row + 3);1H$e[38;5;240m$rule$e[0m")
  [void]$sb.Append("$e[$($row + 4);1H$e[38;5;246m  ? for shortcuts$e[K$e[0m")
  [Console]::Out.Write($sb.ToString())
  [Threading.Thread]::Sleep(50)
}
'@
  # Forty lines at a time, 5 ms apart
  burst  = New-PwshProgram -Script @'
$s = (('x' * 60) + "`n") * 40
while ($true) { [Console]::Out.Write($s); [Threading.Thread]::Sleep(5) }
'@
}

function New-LoadConfig([string]$Name) {
  $cfg = Join-Path $T.Out "config-$Name\mux-load.lua"
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.unix_domains = { {
  name = 'loadmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  $cfg
}

# The window on the test domain, its tabs the echo first and then the busy ones, all in the
# server; with $Log, WEZTERM_LOG for the window and the server it starts
function Open-LoadWindow([string]$Name, [string]$Log) {
  $cfg = New-LoadConfig $Name
  $open = @{ ConfigFile = $cfg; Domain = 'loadmux'; Socket = $T.Sock; TimeoutSec = 30; Exe = $guiExe }
  if ($Log) { $open.Log = $Log }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What "the $Name window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $sp = [int]$server[0].ProcessId
  # Before the tabs print: a guard on a timer of its own, so an echo waited on can't hold it up
  Start-MemoryGuard -Pids $w.Pid, $sp -LimitGB $MaxMemGB
  $shell = $panes[0].pane_id
  $echo = New-Tab -From $shell -Program $Progs.echo -Server
  $busy = @()
  for ($i = 0; $i -lt $Claude; $i++) { $busy += New-Tab -From $shell -Program $Progs.claude -Server }
  for ($i = 0; $i -lt $Burst; $i++) { $busy += New-Tab -From $shell -Program $Progs.burst -Server }
  $want = 2 + $Claude + $Burst
  Wait-State { param($s) @($s.tabs).Count -eq $want } -What "the $want tabs in the window" -TimeoutSec 30 | Out-Null
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  $want--
  Wait-State { param($s) @($s.tabs).Count -eq $want } -What "the $want tabs once the shell's gone" -TimeoutSec 30 | Out-Null
  Invoke-Cli activate-pane --pane-id $echo -Server -Check | Out-Null
  $w | Add-Member -NotePropertyName Main -NotePropertyValue ([LbtermMl]::MainThread($w.Pid))
  $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue $sp
  $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue ([LbtermMl]::MainThread($sp))
  $w | Add-Member -NotePropertyName ServerLog -NotePropertyValue (Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$sp.txt")
  $w | Add-Member -NotePropertyName Echo -NotePropertyValue $echo
  $w | Add-Member -NotePropertyName Busy -NotePropertyValue $busy
  $w
}

# Stops the test once the memory guard has stopped the window and its server (Start-MemoryGuard:
# over -MaxMemGB of private bytes or working set, read every 200 ms)
function Assert-LoadMemory($w) {
  $guard = Get-MemoryGuard
  if ($guard) { throw "the memory guard stopped the test: $guard (its output outran it)" }
}

function Get-LoadMark($w) {
  [pscustomobject]@{
    Clock = [Diagnostics.Stopwatch]::GetTimestamp()
    Gui = [LbtermMl]::Sample($w.Pid); GuiMain = [LbtermMl]::ThreadCycles($w.Main)
    Server = [LbtermMl]::Sample($w.ServerPid); ServerMain = [LbtermMl]::ThreadCycles($w.ServerMain)
  }
}

function Get-LoadDelta($a, $b) {
  $s = ($b.Clock - $a.Clock) / [Diagnostics.Stopwatch]::Frequency
  $per = { param($x) [Math]::Round($x / $s, 1) }
  [ordered]@{
    Seconds = [Math]::Round($s, 2)
    ServerCpuMsPerSec = & $per (& $ms ($b.Server[0] - $a.Server[0])); ServerMainMsPerSec = & $per (& $ms ($b.ServerMain - $a.ServerMain))
    ServerFaultsPerSec = & $per ($b.Server[1] - $a.Server[1])
    ServerReadsPerSec = & $per ($b.Server[2] - $a.Server[2]); ServerWritesPerSec = & $per ($b.Server[3] - $a.Server[3]); ServerOtherIoPerSec = & $per ($b.Server[4] - $a.Server[4])
    ServerPrivateMB = [Math]::Round($b.Server[5] / 1MB, 1); ServerPeakWorkingSetMB = [Math]::Round($b.Server[6] / 1MB, 1)
    GuiCpuMsPerSec = & $per (& $ms ($b.Gui[0] - $a.Gui[0])); GuiMainMsPerSec = & $per (& $ms ($b.GuiMain - $a.GuiMain))
    GuiFaultsPerSec = & $per ($b.Gui[1] - $a.Gui[1])
    GuiReadsPerSec = & $per ($b.Gui[2] - $a.Gui[2]); GuiWritesPerSec = & $per ($b.Gui[3] - $a.Gui[3]); GuiOtherIoPerSec = & $per ($b.Gui[4] - $a.Gui[4])
    GuiPrivateMB = [Math]::Round($b.Gui[5] / 1MB, 1); GuiPeakWorkingSetMB = [Math]::Round($b.Gui[6] / 1MB, 1)
  }
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

# Keys posted to the echoing tab for -Seconds, each timed to the frame showing its echo; the
# load's numbers over the same span. -Counting: the PDUs each side logged over it too
function Measure-Load($w, [switch]$Counting) {
  Invoke-Cli activate-pane --pane-id $w.Echo -Server -Check | Out-Null
  Start-Sleep 3
  $cursorX = { param($sw) $p = @($sw.panes | Where-Object active)[0]; if ($p) { [int]$p.cursor.x } else { -1 } }
  $x0 = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
  if ($x0 -lt 0) { throw "the echoing pane's cursor isn't in the state file" }
  $h = Resolve-Handle $w
  $times = [Collections.Generic.List[double]]::new()
  $late = [Collections.Generic.List[object]]::new()
  $echoWait = [Math]::Max(2, $EchoMs / 1000 * 3)
  if ($Counting) { $gAt = (Get-Item $w.Log).Length; $sAt = (Get-Item $w.ServerLog).Length }
  # The window's counters from its start (what it said as it attached and made its tabs, and the
  # answer an older server gives), then over the span (lbterm/MUX.md, "Tabs out of view: output
  # held at the server"); a window without counters (a build too old) has none of these
  $m0 = try { Get-Metrics $w -Reset -TimeoutMs 4000 } catch { $null }
  $a = Get-LoadMark $w
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $i = 0
  while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
    $want = $x0 + $i + 1
    # The echo wraps at the pane's edge: a new line of them before it does
    # (Enter, which the echo writes back as a carriage return)
    if ($want -ge $Cols - 2) {
      [LbtermMl]::PostKey($h, 0x0D)
      Wait-State { param($sw) (& $cursorX $sw) -in 0..3 }.GetNewClosure() -What 'the echo back at the line''s start' -TimeoutSec 10 -IntervalMs 5 | Out-Null
      $x0 = & $cursorX (Get-StateWindow (Get-Test).LastState -Hwnd ([long]$w.Handle)); $i = 0; $want = $x0 + 1
    }
    $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    [LbtermMl]::PostKey($h, 0x58)
    Watch-Foreground
    $shown = { param($sw) (& $cursorX $sw) -ge $want }.GetNewClosure()
    try {
      Wait-State $shown -What "key $($i + 1)'s echo" -TimeoutSec $echoWait -IntervalMs 5 | Out-Null
      $times.Add([long](Get-Test).LastState.time_ms - $t0)
    } catch {
      # Which key, where its echo should have put the cursor, and where it was
      $seen = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
      $late.Add([ordered]@{ Key = $i; At = [Math]::Round($clock.Elapsed.TotalSeconds, 2); Want = $want; Cursor = $seen })
    }
    Assert-LoadMemory $w
    $i++
    $left = $KeyEveryMs - ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $t0)
    if ($left -gt 0) { Start-Sleep -Milliseconds $left }
  }
  $d = Get-LoadDelta $a (Get-LoadMark $w)
  $m1 = if ($m0) { try { Get-Metrics $w -TimeoutMs 4000 } catch { $null } }
  if ($m0 -and $m1) {
    $d.SetupShown = Get-Pdus $m0 'SetShownPanes' 'sent'
    $d.SetupErrors = Get-Pdus $m0 'ErrorResponse' 'received'
    # The window's own word that its server doesn't know SetShownPanes (wezterm-client's
    # lbterm_shown.rs, at info): an ErrorResponse alone says nothing, since every build answers
    # the harness's kill of the start's shell with one ("no such pane 0")
    $d.SetupLacks = Test-ServerLacks $w.Log
    $d.SpanShown = Get-Pdus $m1 'SetShownPanes' 'sent'
    $d.PaneHeld = (Get-Pdus $m0 'PaneHeld' 'received') + (Get-Pdus $m1 'PaneHeld' 'received')
    $d.PushesPerSec = [Math]::Round((Get-Pdus $m1 'GetPaneRenderChangesResponse' 'received') / $d.Seconds, 1)
    # The mux client thread a second: the window's busiest thread without a name other than its
    # GUI thread (wezterm-client's client.rs starts it with a bare thread::spawn; async-io's is
    # named), which the process's and the GUI thread's figures don't split out. The reply is
    # kept beside mux-load.json
    $client = @($m1.threads | Where-Object { -not $_.name -and [long]$_.tid -ne [long]$w.Main })[0]
    $span = if ($m1.span_ms) { [double]$m1.span_ms / 1000 } else { $d.Seconds }
    $d.ClientThreadMsPerSec = if ($client) { [Math]::Round([double]$client.cpu_ms / $span, 1) } else { 0 }
    $m1 | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out "$(if ($Counting) { 'count' } else { 'load' })-metrics.json")
  }
  # Whatever ended the window's connection (a PDU it didn't know, PITFALLS 188)
  $d.Connection = @(Get-ConnectionLines $w.Log $w.LogStart)
  if ($Counting) {
    $d.ServerSent = Get-PduCounts $w.ServerLog $sAt
    $d.WindowSent = Get-PduCounts $w.Log $gAt
  }
  $d.Keys = $times.Count + $late.Count
  $d.KeysLate = $late.Count
  $d.LateKeys = @($late)
  $d.EchoWaitSec = $echoWait
  $d.EchoP50 = Get-Pct $times 0.5; $d.EchoP95 = Get-Pct $times 0.95; $d.EchoMax = ($times | Measure-Object -Maximum).Maximum
  # Every key's, in mux-load.json, for comparing builds finer than a percentile in whole ms
  $d.EchoMs = @($times)
  # Every tab reached the window
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $d.Tabs = @($s.tabs).Count
  [pscustomobject]$d
}

# The PDUs each side's log has from offset $At on: count and bytes by type, and the server's
# compression attempts and how many lost
function Get-PduCounts([string]$Log, [long]$At) {
  if (-not (Test-Path $Log)) { return $null }
  $fs = [IO.File]::Open($Log, 'Open', 'Read', 'ReadWrite, Delete')
  $text = try { [void]$fs.Seek($At, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  $by = @{}
  $pdus = 0; $bytes = 0
  foreach ($m in [regex]::Matches($text, 'encode(?:_async)? (\w+) size=(\d+)')) {
    $n = $m.Groups[1].Value; $b = [long]$m.Groups[2].Value
    if (-not $by[$n]) { $by[$n] = [ordered]@{ Count = 0; Bytes = 0 } }
    $by[$n].Count++; $by[$n].Bytes += $b; $pdus++; $bytes += $b
  }
  $tried = 0; $lost = 0
  foreach ($m in [regex]::Matches($text, 'serialized\+compress len (\d+) vs (\d+)')) {
    $tried++; if ([long]$m.Groups[1].Value -ge [long]$m.Groups[2].Value) { $lost++ }
  }
  [ordered]@{ Pdus = $pdus; Bytes = $bytes; Compressed = $tried - $lost; CompressionLost = $lost; ByType = $by }
}

# A PDU's count in a metrics reply, sent or received
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}

# The window's log lines that say its connection ended: a reconnect, or the domain detached
# (wezterm-client's client.rs and domain.rs)
# (from $From, the window's `LogStart`: a GUI given a pid an earlier one had appends to its log,
# whose lines aren't this window's, PITFALLS 240)
function Get-ConnectionLines([string]$Log, [long]$From = 0) {
  if (-not $Log -or -not (Test-Path -LiteralPath $Log)) { return @() }
  $fs = [IO.File]::Open($Log, 'Open', 'Read', 'ReadWrite, Delete')
  $text = try { if ($From -gt 0 -and $From -le $fs.Length) { [void]$fs.Seek($From, 'Begin') }; [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  @($text -split "`r?`n" | Where-Object { $_ -match 'the connection to the mux server ended|client disconnected|Reconnected!|reattaching after a reconnect|detached domain' })
}

# Whether the window's log says its server doesn't know SetShownPanes (an older build)
function Test-ServerLacks([string]$Log) {
  if (-not $Log -or -not (Test-Path -LiteralPath $Log)) { return $false }
  $fs = [IO.File]::Open($Log, 'Open', 'Read', 'ReadWrite, Delete')
  $text = try { [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  $text -match "the mux server doesn't know SetShownPanes"
}

function Format-Load($d) {
  "server CPU $($d.ServerCpuMsPerSec) ms/s (main thread $($d.ServerMainMsPerSec)), $($d.ServerFaultsPerSec) page faults/s, I/O $($d.ServerReadsPerSec) reads $($d.ServerWritesPerSec) writes $($d.ServerOtherIoPerSec) other /s, private $($d.ServerPrivateMB) MB (peak working set $($d.ServerPeakWorkingSetMB)); " +
  "window CPU $($d.GuiCpuMsPerSec) ms/s (GUI thread $($d.GuiMainMsPerSec)$(if ($null -ne $d.ClientThreadMsPerSec) { ", client thread $($d.ClientThreadMsPerSec)" })), $($d.GuiFaultsPerSec) page faults/s, I/O $($d.GuiReadsPerSec) reads $($d.GuiWritesPerSec) writes $($d.GuiOtherIoPerSec) other /s, private $($d.GuiPrivateMB) MB (peak $($d.GuiPeakWorkingSetMB)); " +
  "$($d.Keys) keys, echo p50 $($d.EchoP50) p95 $($d.EchoP95) max $($d.EchoMax) ms$(if ($d.KeysLate) { ", $($d.KeysLate) not shown within $($d.EchoWaitSec) s" }); $($d.Tabs) tabs"
  if ($null -ne $d.PushesPerSec) { "said: $($d.SetupShown) SetShownPanes at the start ($($d.SetupErrors) ErrorResponse back$(if ($d.SetupLacks) { ', the server doesn''t know it' })), $($d.SpanShown) over the span; the window took $($d.PushesPerSec) pushes a second and $($d.PaneHeld) PaneHeld (the server $(if ($d.PaneHeld) { 'held the tabs out of view' } else { 'pushed every tab' })); the connection $(if ($d.Connection.Count) { "ended: $($d.Connection -join ' | ')" } else { 'never ended' })" }
}

$results = [ordered]@{
  Build = $null; Window = $guiExe; Server = $serverExe; Claude = $Claude; Burst = $Burst; Seconds = $Seconds
  Locked = (Test-WorkstationLocked); Load = $null; Count = $null
}
Invoke-Test {
  $w = Open-LoadWindow 'load'
  $results.Build = (Read-State).build
  $d = Measure-Load $w
  $results.Load = $d
  "load ($Claude Claude-like tabs, $Burst bursting, an echo in view; server $serverExe): $(Format-Load $d)"
  Stop-TestWindow $w
  Stop-TestServers
  if (-not $Measure) {
    if ($d.KeysLate) { Add-Failure "$($d.KeysLate) of $($d.Keys) keys' echoes didn't show within $($d.EchoWaitSec) s: $(($d.LateKeys | ForEach-Object { "key $($_.Key) at $($_.At) s, the cursor at $($_.Cursor) for $($_.Want)" }) -join '; ')" }
    if ($d.EchoP95 -gt $EchoMs) { Add-Failure "a key's echo took $($d.EchoP95) ms at the 95th percentile, over $EchoMs" }
    if ($d.Tabs -ne 1 + $Claude + $Burst) { Add-Failure "the window has $($d.Tabs) tabs, not $(1 + $Claude + $Burst)" }
    # Each pairing (PITFALLS 159): the connection held, and what was said fits the builds: this
    # build's window says what it shows and its server holds the tabs out of view; a server
    # that answers ErrorResponse is told no more and holds nothing; another build's window says
    # nothing and is told of no hold, which would end its connection (PITFALLS 188)
    if ($d.Connection.Count) { Add-Failure "the window's connection ended: $($d.Connection -join ' | ')" }
    if ($null -ne $d.PushesPerSec) {
      if ($GuiBin) {
        if ($d.SetupShown -or $d.SpanShown) { Add-Failure "another build's window sent SetShownPanes: -GuiBin isn't an older window" }
        if ($d.PaneHeld) { Add-Failure "the server sent another build's window $($d.PaneHeld) PaneHeld" }
      }
      else {
        if (-not $d.SetupShown) { Add-Failure "this build's window sent no SetShownPanes as it attached and made its tabs" }
        if ($d.SetupLacks -and $d.PaneHeld) { Add-Failure "a server that doesn't know SetShownPanes sent $($d.PaneHeld) PaneHeld" }
        if (-not $ServerBin -and -not $d.PaneHeld) { Add-Failure "this build's server held none of the $($Claude + $Burst) tabs out of view (no PaneHeld came)" }
      }
    }
  }

  if ($Count) {
    $w = Open-LoadWindow 'count' -Log 'info,codec=debug'
    $c = Measure-Load $w -Counting
    $results.Count = $c
    $server = $c.ServerSent; $gui = $c.WindowSent; $span = $c.Seconds
    $top = ($server.ByType.GetEnumerator() | Sort-Object { $_.Value.Count } -Descending | Select-Object -First 4 | ForEach-Object { "$($_.Key) $($_.Value.Count)" }) -join ', '
    $wtop = ($gui.ByType.GetEnumerator() | Sort-Object { $_.Value.Count } -Descending | Select-Object -First 3 | ForEach-Object { "$($_.Key) $($_.Value.Count)" }) -join ', '
    "count (codec=debug, $span s, $($c.Keys) keys): the server sent $($server.Pdus) PDUs ($([Math]::Round($server.Pdus / $span)) /s, $([Math]::Round($server.Bytes / $span / 1KB, 1)) KB/s; $top), $($server.Compressed) compressed, $($server.CompressionLost) tried and sent as they were; the window sent $($gui.Pdus) ($([Math]::Round($gui.Bytes / $span / 1KB, 1)) KB/s; $wtop), $($gui.Compressed) compressed; $(Format-Load $c)"
    Stop-TestWindow $w
    Stop-TestServers
  }
  [pscustomobject]$results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'mux-load.json')
} -Passed "mux-load: $Claude Claude-like and $Burst bursting tabs on a mux domain: keys echo and every tab reaches the window"
