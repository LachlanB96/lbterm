#Requires -Version 7
# Variant: -Local -Only echo -EchoMs 15
# Covers: term/src/screen.rs, term/src/lbterm_changes.rs, term/src/terminalstate/mod.rs, mux/src/lib.rs, mux/src/lbterm_wait.rs, mux/src/localpane.rs, mux/src/domain.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/lbterm_push.rs
<#
What a mux server's push of a pane's changes, a key's echo and a pane itself cost, in a test mux
server of the test's own (a socket under the test's folder; never Lachlan's), its window drawn as
Lachlan's (New-TestConfig, so 50,000 lines of scrollback). Each part opens its own window and
server:

1. echo: a tab in view echoing what's typed and nothing else running: -Keys keys posted
   -KeyEveryMs apart, each timed from its post to the first frame showing its echo (the state
   file's time_ms and the pane's cursor). -Local runs the tab in the window's own domain; -Frames
   lists every frame painted after each key until the echo's, with where the cursor was.
2. history: the same echo with -History lines printed into it first, and -Shell tabs behind it
   with as many lines each, printing a line every -ShellEveryMs on the primary screen as a build
   does: the server's CPU and its main thread's over -Seconds, and the keys' echoes.
3. memory: the server's private bytes with one tab, and again with -Idle more idle tabs: what a tab
   holds before it prints anything.
4. sendtext: -Sends `cli send-text` calls into the echoing tab (as the Conductor and Relay type),
   with the cli's wezterm_client=debug log: the PDUs the server sent each call before its answer,
   and the server's main thread's time for them all.
5. spawn: the echo, while tabs are spawned beside it and killed again, one at a time, on a
   thread of the test's own, for -Spawns times 12 keys: the keys' echoes then against before,
   how long each spawn and kill took the cli, and the server's main thread's time for each.
6. flood: the echo, while another tab prints without a pause for -FloodSec: the keys' echoes
   then against before. -FloodColour prints every word in a colour of its own.

It stops a part when the window or the server holds over -MaxMemGB, private bytes or working set,
read every 200 ms on a timer of its own (the harness's Start-MemoryGuard): on a mux domain six tabs
printing without a pause grew a test server to 29.5 GB and its window to 11.5 GB in about seven
minutes (2026-10-29), nothing bounding what the server queues for a client that can't keep up.

It fails, unless -Measure, when a key's echo never shows, when the echo's median is over -EchoMs,
a tab holds over -TabMB of the server's private memory, a send-text gets anything pushed before
its answer, or the server's main thread spends over -SpawnMainMs on each spawn and kill.
mux-push.json in the output folder has every number.

  pwsh -File lbterm\perf\mux-push.ps1 [-Only echo,history,memory,sendtext,spawn,flood] [-Local] [-Frames] [-FloodColour] [-Lua <config lines>] [-Log <WEZTERM_LOG>] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string[]]$Only = @('echo', 'history', 'memory', 'sendtext', 'spawn', 'flood'),
  [switch]$Local,
  [int]$Keys = 60,
  [int]$KeyEveryMs = 150,
  [int]$History = 50000,
  [int]$Shell = 2,
  [int]$ShellEveryMs = 10,
  [int]$Seconds = 15,
  [int]$Idle = 10,
  [int]$Sends = 50,
  [int]$Spawns = 5,
  [int]$FloodSec = 5,
  # flood: every word in a colour of its own (24-bit SGR), as a styled program's output is
  [switch]$FloodColour,
  [int]$Cols = 120,
  [int]$Rows = 40,
  [string]$Lua = '',
  # WEZTERM_LOG for the windows and the servers they start
  [string]$Log,
  # echo: every frame after each key until its echo's, from the state file read without a pause
  [switch]$Frames,
  [switch]$Measure,
  [double]$MaxMemGB = 2,
  [double]$EchoMs = 40,
  [double]$TabMB = 4,
  [double]$SpawnMainMs = 35
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-push' -Bin $Bin
$Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermMp {
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
  // Cycles, page faults and private bytes of a process
  public static ulong[] Sample(int pid) {
    var r = new ulong[3];
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED | PROCESS_VM_READ, false, (uint)pid);
    if (h == IntPtr.Zero) return r;
    try {
      ulong c; QueryProcessCycleTime(h, out c); r[0] = c;
      PMC m; if (K32GetProcessMemoryInfo(h, out m, (uint)Marshal.SizeOf(typeof(PMC)))) { r[1] = m.PageFaults; r[2] = (ulong)m.Pagefile; }
      return r;
    } finally { CloseHandle(h); }
  }
  // The process's first thread, its main one
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
$rate = [LbtermMp]::Rate(300)
$ms = { param([ulong]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }

# Each key it's given, written back, once $Lines lines are printed (and a file says they are)
function Get-EchoProgram([int]$Lines, [string]$Done) {
  New-PwshProgram -Script @"
`$n = $Lines
if (`$n -gt 0) {
  `$sb = [Text.StringBuilder]::new()
  for (`$i = 1; `$i -le `$n; `$i++) { [void]`$sb.Append("history line `$i of `$n, as a build or a log leaves it`n") }
  [Console]::Out.Write(`$sb.ToString())
}
Set-Content -LiteralPath '$Done' 'printed'
[Console]::Out.Write('> ')
while (`$true) { `$k = [Console]::ReadKey(`$true); [Console]::Out.Write(`$k.KeyChar) }
"@
}

# $Lines lines first, then a line every $EveryMs on the primary screen, as a build does
function Get-ShellProgram([int]$Lines, [int]$EveryMs, [string]$Done) {
  New-PwshProgram -Script @"
`$n = $Lines
if (`$n -gt 0) {
  `$sb = [Text.StringBuilder]::new()
  for (`$i = 1; `$i -le `$n; `$i++) { [void]`$sb.Append("history line `$i of `$n, as a build or a log leaves it`n") }
  [Console]::Out.Write(`$sb.ToString())
}
Set-Content -LiteralPath '$Done' 'printed'
for (`$i = 0; ; `$i++) { [Console]::Out.Write("   Compiling crate-`$i v0.1.0 (C:\src\crate-`$i)`n"); [Threading.Thread]::Sleep($EveryMs) }
"@
}

# Without a pause, until killed; -FloodColour, each word in a colour of its own
$FloodProgram = if ($FloodColour) {
  New-PwshProgram -Script @'
$e = [char]27
$sb = [Text.StringBuilder]::new()
for ($l = 0; $l -lt 400; $l++) {
  for ($w = 0; $w -lt 12; $w++) { [void]$sb.Append("$e[38;2;$(40 + $w * 17);$(200 - $l % 150);$(90 + $w * 11)mword$w ") }
  [void]$sb.Append("$e[0m`n")
}
$s = $sb.ToString()
while ($true) { [Console]::Out.Write($s) }
'@
} else {
  New-PwshProgram -Script @'
$s = (('x' * 100) + "`n") * 400
while ($true) { [Console]::Out.Write($s) }
'@
}

$IdleProgram = New-PwshProgram -Script 'Start-Sleep 600'

function New-PushConfig([string]$Name) {
  $cfg = Join-Path $T.Out "config-$Name\mux-push.lua"
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  $serverExe = $T.MuxServer
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.unix_domains = { {
  name = 'pushmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
$Lua
"@
  $cfg
}

# A window on the test domain (or its own, -Local) with the echoing tab in view: its pane, its
# server's pid and main thread
function Open-PushWindow([string]$Name, [int]$EchoHistory = 0) {
  $cfg = New-PushConfig $Name
  $done = Join-Path $T.Out "$Name-echo.done"
  $echo = Get-EchoProgram $EchoHistory $done
  if ($Local) {
    $open = @{ ConfigFile = $cfg; Program = $echo; TimeoutSec = 30 }
    if ($Log) { $open.Log = $Log }
    $w = Open-TestWindow @open
    Start-MemoryGuard -Pids $w.Pid -LimitGB $MaxMemGB
    Wait-State { param($s) $s.frame -gt 0 } -What "the $Name window's first frame" -TimeoutSec 30 | Out-Null
    $pane = @(Get-Panes)[0].pane_id
    $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue $w.Pid
    $w | Add-Member -NotePropertyName OnServer -NotePropertyValue $false
  } else {
    $open = @{ ConfigFile = $cfg; Domain = 'pushmux'; Socket = $T.Sock; TimeoutSec = 30 }
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
    # Before the tabs print: a guard on a timer of its own, so an echo waited on can't hold it up
    Start-MemoryGuard -Pids $w.Pid, ([int]$server[0].ProcessId) -LimitGB $MaxMemGB
    $shell = $panes[0].pane_id
    $pane = New-Tab -From $shell -Program $echo -Server
    Wait-State { param($s) @($s.tabs).Count -eq 2 } -What 'the echoing tab in the window' -TimeoutSec 30 | Out-Null
    Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
    Wait-State { param($s) @($s.tabs).Count -eq 1 } -What 'one tab once the shell has gone' -TimeoutSec 30 | Out-Null
    $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue ([int]$server[0].ProcessId)
    $w | Add-Member -NotePropertyName OnServer -NotePropertyValue $true
  }
  $w | Add-Member -NotePropertyName Echo -NotePropertyValue $pane
  $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue ([LbtermMp]::MainThread($w.ServerPid))
  $w | Add-Member -NotePropertyName ServerLog -NotePropertyValue (Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$($w.ServerPid).txt")
  Wait-Done $done "the echoing tab's $EchoHistory lines"
  $w
}

function Wait-Done([string]$File, [string]$What) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not (Test-Path -LiteralPath $File)) {
    if ($clock.Elapsed.TotalSeconds -gt 120) { throw "$What weren't printed within 120 s" }
    Start-Sleep -Milliseconds 100
  }
}

# A tab beside the echo, on the server (or in the window's own domain)
function New-PushTab($w, [object[]]$Program) {
  if ($w.OnServer) { New-Tab -From $w.Echo -Program $Program -Server -NoActivate } else { New-Tab -From $w.Echo -Program $Program -NoActivate }
}

function Kill-PushPane($w, [int]$Pane) {
  if ($w.OnServer) { Invoke-Cli kill-pane --pane-id $Pane -Server -Check | Out-Null } else { Invoke-Cli kill-pane --pane-id $Pane -Check | Out-Null }
}

# Stops the part once the memory guard has stopped the window and its server (Start-MemoryGuard:
# over -MaxMemGB of private bytes or working set, read every 200 ms)
function Assert-PushMemory($w) {
  $guard = Get-MemoryGuard
  if ($guard) { throw "the memory guard stopped the test: $guard (its output outran it)" }
}

function Get-PushMark($w) {
  [pscustomobject]@{
    Clock = [Diagnostics.Stopwatch]::GetTimestamp()
    Gui = [LbtermMp]::Sample($w.Pid); Server = [LbtermMp]::Sample($w.ServerPid); ServerMain = [LbtermMp]::ThreadCycles($w.ServerMain)
  }
}

function Get-PushDelta($a, $b) {
  $s = ($b.Clock - $a.Clock) / [Diagnostics.Stopwatch]::Frequency
  $per = { param($x) [Math]::Round($x / $s, 1) }
  [ordered]@{
    Seconds = [Math]::Round($s, 2)
    ServerCpuMsPerSec = & $per (& $ms ($b.Server[0] - $a.Server[0])); ServerMainMsPerSec = & $per (& $ms ($b.ServerMain - $a.ServerMain))
    ServerFaultsPerSec = & $per ($b.Server[1] - $a.Server[1]); ServerPrivateMB = [Math]::Round($b.Server[2] / 1MB, 1)
    GuiCpuMsPerSec = & $per (& $ms ($b.Gui[0] - $a.Gui[0])); GuiPrivateMB = [Math]::Round($b.Gui[2] / 1MB, 1)
  }
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

# $Count keys posted to the echoing tab, -KeyEveryMs apart, each timed from its post to the first
# frame showing its echo; $During runs between keys (it's given the key's number)
function Measure-Echo($w, [int]$Count, [scriptblock]$During) {
  $cursorX = { param($sw) $p = @($sw.panes | Where-Object active)[0]; if ($p) { [int]$p.cursor.x } else { -1 } }
  $x0 = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
  if ($x0 -lt 0) { throw "the echoing pane's cursor isn't in the state file" }
  $h = Resolve-Handle $w
  $times = [Collections.Generic.List[double]]::new()
  $posted = [Collections.Generic.List[long]]::new()
  $frameLog = [Collections.Generic.List[string]]::new()
  $late = [Collections.Generic.List[object]]::new()
  $i = 0
  for ($k = 0; $k -lt $Count; $k++) {
    $want = $x0 + $i + 1
    # A new line of them before the echo wraps at the pane's edge
    if ($want -ge $Cols - 2) {
      [LbtermMp]::PostKey($h, 0x0D)
      Wait-State { param($sw) (& $cursorX $sw) -in 0..3 }.GetNewClosure() -What 'the echo back at the line''s start' -TimeoutSec 10 -IntervalMs 5 | Out-Null
      $x0 = & $cursorX (Get-StateWindow (Get-Test).LastState -Hwnd ([long]$w.Handle)); $i = 0; $want = $x0 + 1
    }
    $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $posted.Add($t0)
    [LbtermMp]::PostKey($h, 0x58)
    Watch-Foreground
    $shown = { param($sw) (& $cursorX $sw) -ge $want }.GetNewClosure()
    try {
      if ($Frames) {
        # Each frame painted after the post, read as fast as the file can be: its time from the
        # post and where the cursor was
        $seen = [ordered]@{}; $clock = [Diagnostics.Stopwatch]::StartNew(); $done = $false
        while (-not $done -and $clock.ElapsedMilliseconds -lt 5000) {
          $s = Get-StateSnapshot $w.StateFile
          if (-not $s) { continue }
          $sw = Get-StateWindow $s -Hwnd ([long]$w.Handle)
          if ([long]$s.time_ms -ge $t0 -and -not $seen.Contains([string]$s.frame)) { $seen[[string]$s.frame] = "+$([long]$s.time_ms - $t0) x=$(& $cursorX $sw)" }
          if ((& $cursorX $sw) -ge $want) { $done = $true; (Get-Test).LastState = $s }
        }
        if (-not $done) { throw "key $($k + 1)'s echo didn't show" }
        $frameLog.Add("key $($k + 1) (echo at x=$want): $(($seen.GetEnumerator() | ForEach-Object { "frame $($_.Key) $($_.Value)" }) -join ', ')")
      } else {
        Wait-State $shown -What "key $($k + 1)'s echo" -TimeoutSec 5 -IntervalMs 5 | Out-Null
      }
      $times.Add([long](Get-Test).LastState.time_ms - $t0)
    } catch {
      $late.Add($k)
      $x0 = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)); $i = -1
    }
    Assert-PushMemory $w
    $i++
    if ($During) { & $During $k }
    $left = $KeyEveryMs - ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $t0)
    if ($left -gt 0) { Start-Sleep -Milliseconds $left }
  }
  [ordered]@{
    Keys = $Count; Late = $late.Count
    P50 = Get-Pct $times 0.5; P95 = Get-Pct $times 0.95
    Min = ($times | Measure-Object -Minimum).Minimum; Max = ($times | Measure-Object -Maximum).Maximum
    Times = @($times); Posted = @($posted); Frames = @($frameLog)
  }
}

function Format-Echo($e) { "echo p50 $($e.P50) p95 $($e.P95) (min $($e.Min), max $($e.Max)) ms over $($e.Keys) keys$(if ($e.Late) { ", $($e.Late) not shown" })" }

function Test-Echo($e, [string]$Part) {
  if ($Measure) { return }
  if ($e.Late) { Add-Failure "${Part}: $($e.Late) of $($e.Keys) keys' echoes didn't show within 5 s" }
  if ($e.P50 -gt $EchoMs) { Add-Failure "${Part}: a key's echo took $($e.P50) ms at the median, over $EchoMs" }
}

$results = [ordered]@{ Build = $null; Local = [bool]$Local; Locked = (Test-WorkstationLocked) }
Invoke-Test {
  if ('echo' -in $Only) {
    $w = Open-PushWindow 'echo'
    $results.Build = (Read-State).build
    Start-Sleep 2
    $e = Measure-Echo $w $Keys
    $results.Echo = $e
    $results.EchoLogs = [ordered]@{ Window = $w.Log; Server = $w.ServerLog }
    "echo$(if ($Local) { ' (the window''s own domain)' }): $(Format-Echo $e)"
    Test-Echo $e 'echo'
    Stop-TestWindow $w; Stop-TestServers
  }

  if ('history' -in $Only) {
    $w = Open-PushWindow 'history' -EchoHistory $History
    $results.Build = (Read-State).build
    for ($i = 0; $i -lt $Shell; $i++) {
      $done = Join-Path $T.Out "history-shell-$i.done"
      New-PushTab $w (Get-ShellProgram $History $ShellEveryMs $done) | Out-Null
      Wait-Done $done "shell tab $i's $History lines"
    }
    Start-Sleep 3
    $a = Get-PushMark $w
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $e = Measure-Echo $w ([Math]::Max(1, [int]($Seconds * 1000 / $KeyEveryMs)))
    $d = Get-PushDelta $a (Get-PushMark $w)
    $d.Echo = $e
    $results.History = $d
    "history ($History lines in the echo and in $Shell shell tabs printing a line every $ShellEveryMs ms): server CPU $($d.ServerCpuMsPerSec) ms/s (main thread $($d.ServerMainMsPerSec)), $($d.ServerFaultsPerSec) page faults/s, private $($d.ServerPrivateMB) MB; window CPU $($d.GuiCpuMsPerSec) ms/s; $(Format-Echo $e)"
    Test-Echo $e 'history'
    Stop-TestWindow $w; Stop-TestServers
  }

  if ('memory' -in $Only -and -not $Local) {
    $w = Open-PushWindow 'memory'
    Start-Sleep 2
    $before = (Get-PushMark $w).Server[2]
    $tabs = @()
    for ($i = 0; $i -lt $Idle; $i++) { $tabs += New-PushTab $w $IdleProgram }
    Start-Sleep 3
    $after = (Get-PushMark $w).Server[2]
    $perTab = [Math]::Round(($after - $before) / $Idle / 1MB, 2)
    $results.Memory = [ordered]@{ Tabs = $Idle; BeforeMB = [Math]::Round($before / 1MB, 1); AfterMB = [Math]::Round($after / 1MB, 1); PerTabMB = $perTab }
    "memory: the server's private bytes $($results.Memory.BeforeMB) MB with one tab, $($results.Memory.AfterMB) MB with $Idle idle tabs more: $perTab MB a tab"
    if (-not $Measure -and $perTab -gt $TabMB) { Add-Failure "memory: an idle tab holds $perTab MB of the server's private bytes, over $TabMB" }
    Stop-TestWindow $w; Stop-TestServers
  }

  if ('sendtext' -in $Only -and -not $Local) {
    $w = Open-PushWindow 'sendtext' -EchoHistory 200
    Start-Sleep 2
    $cliLogs = Join-Path $env:USERPROFILE '.local\share\lbterm'
    $a = Get-PushMark $w
    $pushed = @{}; $calls = 0; $pushedCalls = 0
    $old = $env:WEZTERM_LOG
    $env:WEZTERM_LOG = 'info,wezterm_client=debug'
    try {
      for ($i = 0; $i -lt $Sends; $i++) {
        $since = Get-Date
        $out = @(Invoke-Cli send-text --pane-id $w.Echo --no-paste 'y' -Server -Merge)
        if ($LASTEXITCODE) { throw "send-text failed: $($out -join ' ')" }
        $calls++
        $text = $out -join "`n"
        # Its log lines go to stderr or to its log file (its pid isn't known: the newest since the call)
        if ($text -notmatch 'decoded serial') {
          $log = Get-ChildItem $cliLogs -Filter 'lbterm.exe-log-*.txt' | Where-Object LastWriteTime -ge $since.AddSeconds(-1) | Sort-Object LastWriteTime -Descending | Select-Object -First 1
          if ($log) { $text = [IO.File]::ReadAllText($log.FullName) }
        }
        if ($text -notmatch 'decoded serial') { throw "send-text's log has no 'decoded serial' line to count PDUs by: $($out -join ' ')" }
        $any = $false
        foreach ($m in [regex]::Matches($text, 'decoded serial 0 (\w+)')) {
          $any = $true
          $k = $m.Groups[1].Value
          if (-not $pushed[$k]) { $pushed[$k] = 0 }
          $pushed[$k]++
        }
        if ($any) { $pushedCalls++ }
        Assert-PushMemory $w
      }
    } finally { $env:WEZTERM_LOG = $old }
    $d = Get-PushDelta $a (Get-PushMark $w)
    $mainMs = [Math]::Round($d.ServerMainMsPerSec * $d.Seconds / $calls, 2)
    $results.SendText = [ordered]@{ Calls = $calls; CallsPushedTo = $pushedCalls; Pushed = $pushed; ServerMainMsPerCall = $mainMs; Seconds = $d.Seconds }
    $what = ($pushed.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Name) $($_.Value)" }) -join ', '
    "sendtext: $calls send-texts, $pushedCalls of them sent something before their answer ($(if ($what) { $what } else { 'nothing' })); the server's main thread $mainMs ms a call"
    if (-not $Measure -and $pushedCalls) { Add-Failure "sendtext: $pushedCalls of $calls send-texts had the pane pushed to them before their answer ($what)" }
    Stop-TestWindow $w; Stop-TestServers
  }

  if ('spawn' -in $Only) {
    $w = Open-PushWindow 'spawn'
    Start-Sleep 2
    $quiet = Measure-Echo $w 30
    # Tabs spawned beside the echo and killed again, one at a time, on a thread of the test's own
    # while the keys go, as the Conductor and the projects card open and close tabs while Lachlan
    # types; each spawn and kill timed to the cli
    $sock = if ($w.OnServer) { $T.Sock } else { $w.Socket }
    $stop = [hashtable]::Synchronized(@{ Stop = $false })
    $job = Start-ThreadJob -ArgumentList $T.Cli, $sock, $w.Echo, $IdleProgram, $stop -ScriptBlock {
      param($cli, $sock, $pane, $prog, $stop)
      $env:WEZTERM_UNIX_SOCKET = $sock
      $spawns = [Collections.Generic.List[double]]::new(); $kills = [Collections.Generic.List[double]]::new()
      while (-not $stop.Stop) {
        $c = [Diagnostics.Stopwatch]::StartNew()
        $id = ((& $cli cli --no-auto-start spawn --pane-id $pane --no-activate -- @prog 2>&1) -join ' ').Trim()
        $spawns.Add([Math]::Round($c.Elapsed.TotalMilliseconds, 1))
        if ($id -notmatch '^\d+$') { throw "spawn printed: $id" }
        Start-Sleep -Milliseconds 150
        $c.Restart()
        & $cli cli --no-auto-start kill-pane --pane-id $id 2>&1 | Out-Null
        $kills.Add([Math]::Round($c.Elapsed.TotalMilliseconds, 1))
        Start-Sleep -Milliseconds 150
      }
      [pscustomobject]@{ Spawns = @($spawns); Kills = @($kills) }
    }
    $a = Get-PushMark $w
    try {
      $busy = Measure-Echo $w ([Math]::Max(10, $Spawns * 12))
    } finally {
      $stop.Stop = $true
      $made = Receive-Job $job -Wait -AutoRemoveJob
    }
    $d = Get-PushDelta $a (Get-PushMark $w)
    $n = @($made.Spawns).Count
    # The server's main thread's time over the spawns, for each spawn and kill
    $mainPer = if ($n) { [Math]::Round($d.ServerMainMsPerSec * $d.Seconds / $n, 1) } else { $null }
    $avg = { param($v) [Math]::Round((@($v) | Measure-Object -Average).Average, 1) }
    $results.Spawn = [ordered]@{ Quiet = $quiet; Spawning = $busy; SpawnMs = @($made.Spawns); KillMs = @($made.Kills); ServerMainMsPerSpawn = $mainPer; Load = $d }
    "spawn: before, $(Format-Echo $quiet); with tabs spawned and killed beside it, $(Format-Echo $busy); $n spawns, $(& $avg $made.Spawns) ms each to the cli on average (max $((@($made.Spawns) | Measure-Object -Maximum).Maximum)), kills $(& $avg $made.Kills) ms; the server's main thread $mainPer ms for each spawn and kill"
    Test-Echo $quiet 'spawn (before)'
    if (-not $Measure -and $busy.Late) { Add-Failure "spawn: $($busy.Late) keys' echoes didn't show within 5 s while tabs were spawned" }
    if (-not $Measure -and $mainPer -gt $SpawnMainMs) { Add-Failure "spawn: the server's main thread spent $mainPer ms on each spawn and kill, over $SpawnMainMs" }
    Stop-TestWindow $w; Stop-TestServers
  }

  if ('flood' -in $Only) {
    $w = Open-PushWindow 'flood'
    Start-Sleep 2
    $quiet = Measure-Echo $w 20
    $flood = New-PushTab $w $FloodProgram
    try {
      $busy = Measure-Echo $w ([Math]::Max(1, [int]($FloodSec * 1000 / $KeyEveryMs)))
    } finally { Kill-PushPane $w $flood }
    $results.Flood = [ordered]@{ Quiet = $quiet; Flooding = $busy }
    "flood: before, $(Format-Echo $quiet); with a tab printing without a pause, $(Format-Echo $busy)"
    Test-Echo $quiet 'flood (before)'
    if (-not $Measure -and $busy.Late) { Add-Failure "flood: $($busy.Late) keys' echoes didn't show within 5 s beside a flooding tab" }
    Stop-TestWindow $w; Stop-TestServers
  }
  [pscustomobject]$results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'mux-push.json')
} -Passed "mux-push: $($Only -join ', ')$(if ($Local) { ' (the window''s own domain)' }): keys echo, an idle tab holds little and a send-text gets only its answer"
