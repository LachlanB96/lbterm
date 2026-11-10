#Requires -Version 7
# Variant: -Mux
# Covers: window/src/spawn.rs, window/src/os/windows/connection.rs, window/src/os/windows/window.rs, window/src/os/windows/work.rs, lbterm-extras/src/work.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/termwindow/render/mod.rs, wezterm-gui/src/termwindow/render/screen_line.rs, wezterm-gui/src/termwindow/render/conductor_view.rs, wezterm-gui/src/state_probe.rs, config/src/lbterm.rs, lfucache/src/lib.rs, wezterm-gui/src/quad.rs
<#
What the GUI thread spends besides painting, and what it lays out again, in test windows drawn
as Lachlan's (New-TestConfig: WebGpu, the compositor). The counts are the state file's `work`
and `pace` (lbterm/STATE.md), the GUI thread's CPU its cycles (QueryThreadCycleTime). With -Mux
the keys and tabs windows' tabs live in a test lbterm-mux-server, as Lachlan's do.

1. keys: a tab in view echoing what's typed, -Busy tabs printing forty lines every 5 ms behind it, and
   -Keys keys posted -KeyEveryMs apart. Each key's time from the post to the message loop taking
   it off the queue (the state file's work.key_qpc against QueryPerformanceCounter read as it's
   posted: all it waited, behind a paint, a task or a drain), and to the first frame showing its
   echo (the state file's time_ms and the pane's cursor); and from `work` how long a message seen
   waiting for the window waited in a drain of the task queue after it was seen. A drain ran every
   task queued, the low-priority ones too, before the loop took a message, so keys and clicks
   waited behind whatever the panes' output had queued. Only a drain long enough to be checked
   with a message waiting judges it: on a mux domain about one in five drains ran over 2 ms
   (2026-10-30); in a window of its own none, and the check says it judged nothing. On a mux
   domain on 2026-10-30, three rounds against this branch with only the cut taken out: a key
   waited from its post to its taking p95 1.2 to 1.9 ms (max 1.4 to 2.1) with the cut and 2.8 to
   4.8 (max 3.3 to 5.3) without; a message seen waiting stayed 0 to 0.06 ms in its drain after it
   was seen, against 1.21 to 1.27 with 2 to 6 of 11 to 19 over 2 ms; the echo's frame came 24 to
   26 ms after the key at the median on both. In a window of its own a key was taken 0.07 ms
   after its post at the median.
2. pointer: -Moves posted moves over a quiet pane's text, 2 ms apart, a pixel to and fro: the
   tasks run and the mouse cursor set for them. Each move set the cursor's shape again, in a task
   of its own, though it stayed the I-beam.
3. ime: a pane redrawing a status line in place every 50 ms (the cursor saved and put back, as
   a spinner does), for -Seconds: the frames and the times the IME's window was moved. Each
   frame moved it to the cursor in a task of its own, though the cursor stayed put.
4. focus: a pane of dense coloured text and -Changes posted WM_KILLFOCUS and WM_SETFOCUS in turn,
   each waited on to its settled frame: the lines laid out again, the accent colour's reads
   (the registry and a UISettings object) and the GUI thread's CPU for each. Each change read
   them again and laid every line of the window out again, for the cursor's look alone.
5. panes: two panes of dense text and -Changes moves of the keyboard between them (cli
   activate-pane): the lines laid out again for each. Each move laid out both panes' lines,
   though with the compositor only the cursor's lines change.
6. tabs: -Tabs tabs (20) of -TabRows rows (90, Lachlan's window's) of varied output, words,
   numbers and hashes in runs of colour, each tab its own, visited once, then -Rounds rounds of
   hops through every tab: side-button presses (Back and Forward are Lachlan's fast way through
   his tabs, with clicks on the rail's pads; Ctrl+Tab is Windows' own on his PC), or with -Hop pad
   a click on the next tab's pad. For each hop the lines laid out again, the lines and runs
   shaped again (the state file's work.lines, line_shapes and shapes: the line quad, line-to-
   element and shape caches' misses), the GUI thread's CPU and its time to the frame, and the
   window's private memory and working set at rest after the last. A window keeps 1024 lines in
   each cache in upstream's default: 20 tabs of 90 rows are 1,800 lines, so every hop laid its
   tab out and shaped it again; LBTerm's default is 4096 (config/src/lbterm.rs,
   default_line_cache_size; Lachlan's 1a, 2026-10-30). -CacheSize sets the four caches' size.
   On 2026-10-30, three rounds interleaved, 91 rows: with 1024 (the installed build) each hop
   laid out its tab's 91 lines and shaped 84 of them again (289 to 295 runs), 35.6 to 38.1 ms of
   the GUI thread a hop, its frame 27 to 29 ms after the press at the median; with 4096 none,
   19.1 to 20.9 ms and 11 to 12, the window holding 719 to 745 MB of private memory at rest after
   against 693 to 696. By pad clicks 44.7 to 49.1 ms against 28.0, on a mux domain 37.1 to 38.9
   against 21.1 to 22.2. With 2048 a hop missed 0.9 to 2.9 lines; with 6 tabs none on any build.
7. unseen: a pane printing a line every 250 ms, for -Seconds each shown, minimized
   (SW_SHOWMINNOACTIVE, never activating it) and shown again (SW_SHOWNOACTIVATE): frames, the GUI
   thread's CPU, the slow log's passes and the wall time its draws and presents took, which on a
   locked desktop is mostly waiting on DWM, and how soon the window paints once it's shown again.
   A minimized window painted every frame it was asked for, which showed nowhere; now it paints
   one a second at most. -Skip gives the window LBTERM_UNSEEN=skip, so it goes by the session's
   lock and the screens as a daily window does (a window with a state file otherwise paints on,
   for the tests that read its frames).
8. lock: the same pane in a window with LBTERM_UNSEEN=skip, for -Seconds each shown, told the
   session locked and told it unlocked: WM_WTSSESSION_CHANGE posted to it, as Windows sends it,
   since a test can't lock the PC. Frames, the GUI thread's CPU, and how soon it paints once
   unlocked. A window painted on a locked desktop as it did on a live one, and waited on DWM,
   which took a frame about every 260 ms.
9. fill (only with -Only fill; it judges nothing): the caches filled, for what they cost full.
   A pane prints -FillLines varied lines (-FillWide: lines the pane's width, in runs of eight
   colours, as dense text) forty at a time 5 ms apart, so each frame lays out its lines at new
   places, then rests: the lines laid out and runs shaped, and the window's private memory and
   working set at rest, against the same window before it printed. An entry is kept until a
   newer one needs its room, so a window whose panes print holds its caches full whatever its
   tabs. On 2026-10-30 (90 rows, three rounds) the window grew from about 500 MB by 60 to 79 MB
   with 1024 (the installed build), 107 to 109 with 2048 and 165 to 173 with 4096; with
   -FillWide by 193 to 194 with 1024 and 405 to 407 with 4096. On 2026-10-31, with a cached
   quad's rare corners and gradient boxed (wezterm-gui's BoxedQuad, 104 bytes for 220),
   interleaved with the build before it and the installed one: 4096 by 118 to 122 MB (160 to
   171 before; the installed 1024, 62 to 78), -FillWide 298 to 306 (402 to 412; 191 to 192),
   2048 by 70 and 81 (105 and 105), the GUI thread's time for the fill and a hop through 20
   tabs the same.

It fails, unless -Measure, when a message seen waiting waited in its drain over -WaitMs on average
after it was seen, or more than -LongWaitShare of them over 2 ms (and says it judged nothing, in
its output and result.json's notes, when no message was seen waiting), when the pointer's moves run more than -PerMove tasks or
cursor sets a move, when the ime frames move the IME's window more than -ImePerFrame times a frame,
when a focus change lays out more than -FocusLines lines or reads the accent colour at all, when a
pane move lays out more than -PaneLines lines, when a hop after the first round (back to a tab
already seen) lays out or shapes again more than -TabLines lines, or shapes more than -TabShapes
runs, on average, when the minimized window paints more than -UnseenPerSecond frames a
second, or -Skip's while the workstation is locked, or the lock window while it's told the session
is locked, or when the window shown again or told the session is unlocked paints its first frame
more than -RestoreMs after. It fails on a build without `work` in its state file.
gui-thread.json in the output folder has every number.

  pwsh -File lbterm\perf\gui-thread.ps1 [-Mux] [-Only keys,pointer,ime,focus,panes,tabs,unseen,lock,fill] [-Seconds 10] [-Skip] [-Tabs 20] [-TabRows 90] [-Hop side|pad] [-CacheSize <n>] [-FillLines 20000] [-FillWide] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [switch]$Mux,
  [string[]]$Only = @('keys', 'pointer', 'ime', 'focus', 'panes', 'tabs', 'unseen', 'lock'),
  [int]$Seconds = 10,
  [int]$Busy = 6,
  [int]$Keys = 25,
  [int]$KeyEveryMs = 150,
  [int]$Moves = 500,
  [int]$Changes = 20,
  [int]$Tabs = 20,
  [int]$TabRows = 90,
  [ValidateSet('side', 'pad')][string]$Hop = 'side',
  [int]$Rounds = 3,
  [int]$Cols = 110,
  [int]$Rows = 60,
  [switch]$Skip,
  [switch]$Measure,
  # A message's wait in a drain after it was seen, on average, in ms
  [double]$WaitMs = 1.0,
  [double]$LongWaitShare = 0.1,
  [double]$PerMove = 0.05,
  [double]$ImePerFrame = 0.1,
  [double]$FocusLines = 4,
  [double]$PaneLines = 6,
  [double]$TabLines = 4,
  [double]$TabShapes = 10,
  # Frames a second a window that can't be seen may paint, and how soon it paints once shown
  [double]$UnseenPerSecond = 1.0,
  [int]$RestoreMs = 400,
  [double]$MaxMemGB = 2,
  # The tabs and fill windows' four line caches' size (config.shape_cache_size and the rest), 0
  # for the build's default (4096 each in LBTerm since 2026-10-30, upstream's 1024 before)
  [int]$CacheSize = 0,
  [int]$FillLines = 20000,
  [switch]$FillWide
)
$ErrorActionPreference = 'Stop'
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$all = 'keys', 'pointer', 'ime', 'focus', 'panes', 'tabs', 'unseen', 'lock', 'fill'
foreach ($o in $Only) { if ($o -notin $all) { throw "-Only takes $($all -join ', '), not '$o'" } }
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('gui-thread' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
public static class LbtermGt {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  public static bool Post(IntPtr h, uint msg, long w, long l) { return PostMessage(h, msg, (IntPtr)w, (IntPtr)l); }
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  const uint THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000;
  public static double Rate(int ms) {
    ulong a, b; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out a);
    while (sw.ElapsedMilliseconds < ms) { }
    QueryThreadCycleTime(GetCurrentThread(), out b);
    return (b - a) / sw.Elapsed.TotalSeconds;
  }
  public static ulong ProcessCycles(int pid) {
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, (uint)pid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryProcessCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  public static ulong ThreadCycles(int tid) {
    IntPtr h = OpenThread(THREAD_QUERY_LIMITED, false, (uint)tid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryThreadCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  // The process's first thread, the GUI's: the one that started first (a thread that ends while
  // they're listed is passed over)
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      int best = 0; DateTime first = DateTime.MaxValue;
      foreach (ProcessThread t in p.Threads) {
        try { if (t.StartTime < first) { first = t.StartTime; best = t.Id; } } catch (Exception) { }
      }
      return best;
    }
  }
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  // A key pressed and let go, posted as the harness's Send-Key does: QueryPerformanceCounter's
  // count (Stopwatch's) just before the press is posted, the clock work.key_qpc is on
  public static long PostKey(IntPtr h, uint vk) {
    uint scan = MapVirtualKey(vk, 0);
    long down = 1 | ((long)(scan & 0xFF) << 16), up = down | 0xC0000000L;
    long posted = Stopwatch.GetTimestamp();
    PostMessage(h, 0x0100, (IntPtr)vk, (IntPtr)unchecked((int)down));
    PostMessage(h, 0x0101, (IntPtr)vk, (IntPtr)unchecked((int)up));
    return posted;
  }
  // n WM_MOUSEMOVEs to (x, y) and (x + 1, y) in turn, intervalUs apart, spinning between
  public static int MoveEvery(IntPtr h, int x, int y, int n, int intervalUs) {
    var sw = Stopwatch.StartNew(); long next = 0; int sent = 0;
    long tick = Stopwatch.Frequency * intervalUs / 1000000;
    while (sent < n) {
      if (sw.ElapsedTicks >= next) {
        int px = x + (sent & 1);
        PostMessage(h, 0x0200, IntPtr.Zero, (IntPtr)((y << 16) | (px & 0xffff)));
        sent++; next += tick;
      } else Thread.SpinWait(20);
    }
    return sent;
  }
}
'@

$rate = [LbtermGt]::Rate(300)
$ms = { param([ulong]$cycles) [Math]::Round($cycles / $rate * 1000, 2) }

$Progs = @{
  # Without a pause
  stream = New-PwshProgram -Script @'
$s = 'x' * 61 + ' '
while ($true) { [Console]::Out.Write($s) }
'@
  # Nothing
  quiet  = New-PwshProgram -Script 'Start-Sleep 900'
  # Each key it's given, written back
  echo   = New-PwshProgram -Script @'
[Console]::Out.Write('> ')
while ($true) { $k = [Console]::ReadKey($true); [Console]::Out.Write($k.KeyChar) }
'@
  # A status line redrawn in place every 50 ms, the cursor saved and put back where it was
  status = New-PwshProgram -Script @'
$e = [char]27
[Console]::Out.Write('ready> ')
while ($true) {
  [Console]::Out.Write("${e}7${e}[1;1H" + [DateTime]::Now.ToString('HH:mm:ss.fff') + "${e}8")
  [Threading.Thread]::Sleep(50)
}
'@
  # A screen of dense text in runs of eight colours, then nothing
  dense  = New-PwshProgram -Script @'
$e = [char]27
$w = [Console]::WindowWidth - 1; $h = [Console]::WindowHeight - 1
for ($r = 0; $r -lt $h; $r++) {
  $b = [Text.StringBuilder]::new()
  for ($c = 0; $c -lt $w; $c++) {
    if ($c % 8 -eq 0) { [void]$b.Append("$e[3$((($r + [Math]::Floor($c / 8)) % 7) + 1)m") }
    [void]$b.Append([char](33 + (($r * 7 + $c) % 90)))
  }
  [Console]::Out.Write($b.ToString() + "$e[0m`n")
}
Start-Sleep 900
'@
  # Forty lines at a time, 5 ms apart: on a mux domain output without a pause outruns the
  # window, and its server and the window grew without bound (29.5 GB and 11.5 GB in about seven
  # minutes with six such tabs, on 2026-10-30)
  burst  = New-PwshProgram -Script @'
$s = (('x' * 60) + "`n") * 40
while ($true) { [Console]::Out.Write($s); [Threading.Thread]::Sleep(5) }
'@
  # A screen of varied output, then nothing: words, numbers and hashes, some in a colour, in lines
  # of any length, each tab's its own
  varied = New-PwshProgram -Script @'
$e = [char]27
$w = [Console]::WindowWidth - 1; $h = [Console]::WindowHeight - 1
$r = [Random]::new()
$words = -split 'cache line pane tab shape glyph quad frame window rail strip fade build check merge branch commit test slot warden summary session prompt output render layout switch hop surface visual config default memory thread GUI mux server socket state file JSON lbterm term cell row column font colour cursor scroll text error warning passed failed ok the a to of and in is it for on that with'
for ($i = 0; $i -lt $h; $i++) {
  $want = $r.Next(8, $w); $b = [Text.StringBuilder]::new(); $n = 0
  while ($true) {
    $k = $r.Next(10)
    $t = if ($k -lt 6) { $words[$r.Next($words.Count)] } elseif ($k -lt 8) { [string]$r.Next(0, 100000) } else { '{0:x8}' -f $r.Next() }
    if ($n + $t.Length + 1 -gt $want) { break }
    if ($r.Next(5) -eq 0) { [void]$b.Append("$e[3$($r.Next(1, 8))m$t$e[0m ") } else { [void]$b.Append("$t ") }
    $n += $t.Length + 1
  }
  [Console]::Out.Write($b.ToString() + "`n")
}
Start-Sleep 900
'@
  # A line every 250 ms
  slow   = New-PwshProgram -Script @'
$i = 0
while ($true) { [Console]::Out.Write("line $i " + [DateTime]::Now.ToString('HH:mm:ss.fff') + "`n"); $i++; [Threading.Thread]::Sleep(250) }
'@
}

# -FillLines lines, forty at a time 5 ms apart, varied or with -FillWide the pane's width in runs of
# eight colours, then the pane's title 'fill done' and nothing
$Progs.fill = New-PwshProgram -Script (@'
$e = [char]27
$w = [Console]::WindowWidth - 1
$r = [Random]::new()
$words = -split 'cache line pane tab shape glyph quad frame window rail strip fade build check merge branch commit test slot warden summary session prompt output render layout switch hop surface visual config default memory thread GUI mux server socket state file JSON lbterm term cell row column font colour cursor scroll text error warning passed failed ok the a to of and in is it for on that with'
$wide = __WIDE__
$b = [Text.StringBuilder]::new()
for ($i = 0; $i -lt __LINES__; $i++) {
  if ($wide) {
    for ($c = 0; $c -lt $w; $c++) {
      if ($c % 8 -eq 0) { [void]$b.Append("$e[3$($r.Next(1, 8))m") }
      [void]$b.Append([char](33 + $r.Next(90)))
    }
    [void]$b.Append("$e[0m")
  } else {
    $want = $r.Next(8, $w); $n = 0
    while ($true) {
      $k = $r.Next(10)
      $t = if ($k -lt 6) { $words[$r.Next($words.Count)] } elseif ($k -lt 8) { [string]$r.Next(0, 100000) } else { '{0:x8}' -f $r.Next() }
      if ($n + $t.Length + 1 -gt $want) { break }
      if ($r.Next(5) -eq 0) { [void]$b.Append("$e[3$($r.Next(1, 8))m$t$e[0m ") } else { [void]$b.Append("$t ") }
      $n += $t.Length + 1
    }
  }
  [void]$b.Append("`n")
  if ($i % 40 -eq 39) { [Console]::Out.Write($b.ToString()); [void]$b.Clear(); [Threading.Thread]::Sleep(5) }
}
[Console]::Out.Write($b.ToString() + "$e]2;fill done$([char]7)")
Start-Sleep 900
'@ -replace '__LINES__', $FillLines -replace '__WIDE__', $(if ($FillWide) { '$true' } else { '$false' }))

function New-GtConfig([string]$Name, [switch]$Domain, [string]$Lua = '') {
  $cfg = Join-Path $T.Out "config-$Name\gui-thread.lua"
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  $dom = if ($Domain) {
    @"
config.unix_domains = { {
  name = 'gtmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  } else { '' }
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
$Lua
$dom
"@
  $cfg
}

# A window whose tabs run $Programs (one each), at the frame that shows them all, its GUI thread
# found; with -Domain its tabs are the test server's. -NoFrame waits for no frame (one tab only):
# a window skipping its paints while the workstation is locked paints none
function Open-GtWindow([string]$Name, [string[]]$Programs, [switch]$Domain, [switch]$PostedPointer, [hashtable]$Environment = @{}, [string]$Lua = '', [switch]$NoFrame) {
  $cfg = New-GtConfig $Name -Domain:$Domain -Lua $Lua
  $open = @{ ConfigFile = $cfg; Environment = $Environment; TimeoutSec = 30; PostedPointer = $PostedPointer }
  if ($Domain) { $open.Domain = 'gtmux'; $open.Socket = $T.Sock } else { $open.Program = $Progs[$Programs[0]] }
  $w = Open-TestWindow @open
  if ($NoFrame) {
    $w | Add-Member -NotePropertyName Main -NotePropertyValue ([LbtermGt]::MainThread($w.Pid))
    return $w
  }
  Wait-State { param($s) $s.frame -gt 0 } -What "the $Name window's first frame" -TimeoutSec 30 | Out-Null
  if ($Domain) {
    # `connect` starts the server's own shell, not a program: the programs go in tabs of their
    # own there, and the shell's tab goes once the window has them
    $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
      $panes = @(Get-Panes -Server)
      if (-not $panes) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
    $shell = $panes[0].pane_id
    foreach ($p in $Programs) { New-Tab -From $shell -Program $Progs[$p] -Server | Out-Null }
    $want = $Programs.Count + 1
    Wait-State { param($s) @($s.tabs).Count -eq $want } -What "the $want tabs in the window" -TimeoutSec 30 | Out-Null
    Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  } else {
    $first = (Get-Panes)[0].pane_id
    for ($i = 1; $i -lt $Programs.Count; $i++) { New-Tab -From $first -Program $Progs[$Programs[$i]] | Out-Null }
  }
  $n = $Programs.Count
  Wait-State { param($s) @($s.tabs).Count -eq $n } -What "the $n tabs" -TimeoutSec 30 | Out-Null
  $w | Add-Member -NotePropertyName Main -NotePropertyValue ([LbtermGt]::MainThread($w.Pid))
  $w
}

# Stops the test if the window or a test mux server holds more than -MaxMemGB: a window that
# can't keep up with its panes' output on a mux domain grew without bound
function Assert-GtMemory($w) {
  $procs = @(Get-Process -Id $w.Pid -ErrorAction SilentlyContinue) + @(Get-TestServers | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue })
  foreach ($p in $procs) {
    $gb = $p.WorkingSet64 / 1GB
    if ($gb -gt $MaxMemGB) { throw "$($p.ProcessName) (pid $($p.Id)) holds $([Math]::Round($gb, 1)) GB, over ${MaxMemGB}: its output outran it" }
  }
}

function Close-GtWindow($w, [switch]$Domain) {
  Stop-TestWindow $w
  if ($Domain) { Stop-TestServers }
}

# The state file's counts and the GUI thread's now
function Get-Mark($w) {
  $s = Get-StateSnapshot $w.StateFile
  $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
  [pscustomobject]@{
    Frame = [long]$sw.frame; Work = $s.work; Pace = $s.pace; Time = [long]$s.time_ms
    Cycles = [LbtermGt]::ThreadCycles($w.Main); Process = [LbtermGt]::ProcessCycles($w.Pid)
    Clock = [Diagnostics.Stopwatch]::GetTimestamp()
  }
}

# What changed from mark $a to mark $b
function Get-Delta($a, $b) {
  $r = [ordered]@{
    Seconds = [Math]::Round(($b.Clock - $a.Clock) / [Diagnostics.Stopwatch]::Frequency, 2)
    Frames = $b.Frame - $a.Frame
    GuiMs = & $ms ($b.Cycles - $a.Cycles); ProcessMs = & $ms ($b.Process - $a.Process)
    PaintMs = $null; HasWork = [bool]($a.Work -and $b.Work)
  }
  if ($a.Pace -and $b.Pace) { $r.PaintMs = & $ms ([ulong]([long]$b.Pace.paint_cycles - [long]$a.Pace.paint_cycles)) }
  foreach ($k in 'tasks', 'drains', 'drain_us', 'long_drains', 'waited', 'waited_us', 'long_waits', 'ime', 'cursors', 'accent', 'lines', 'line_shapes', 'shapes', 'cut', 'unseen') {
    $r[$k] = if ($r.HasWork -and $null -ne $b.Work.$k) { [long]$b.Work.$k - [long]$a.Work.$k } else { $null }
  }
  [pscustomobject]$r
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

function Format-Work($d) {
  if (-not $d.HasWork) { return 'no work in the state file' }
  $wait = if ($d.waited) { [Math]::Round($d.waited_us / $d.waited / 1000, 2) } else { 0 }
  "$($d.tasks) tasks in $($d.drains) drains ($([Math]::Round($d.drain_us / 1000, 1)) ms, $($d.long_drains) over 2 ms); $($d.waited) messages seen waiting in a drain, $wait ms on average, $($d.long_waits) over 2 ms; ime $($d.ime), cursors $($d.cursors), accent $($d.accent), lines $($d.lines)$(if ($null -ne $d.cut) { ", drains cut $($d.cut)" })$(if ($null -ne $d.unseen) { ", unseen paints $($d.unseen)" })"
}

# A scenario that judged nothing says so, in its output and the test's notes (result.json, the
# e2e runner's report), rather than passing for it
function Add-Unjudged([string]$Message) {
  "NOTE: $Message"
  (Get-Test).Notes.Add($Message)
}

function Require-Work([string]$Name, $d) {
  if (-not $d.HasWork -and -not $Measure) { Add-Failure "${Name}: the state file has no work (a build before the GUI thread's counts)" }
}

# Paint parts the slow log names in a span of the log
function Get-Paints([string]$Text) {
  @([regex]::Matches($Text, '(?m)^\d\d:\d\d:\d\d\.\d{3}\s+WARN\s+lbterm::slow\s+> slow paint window-\d+/(\w+): ([\d.]+) ms$') | ForEach-Object {
      [pscustomobject]@{ Part = $_.Groups[1].Value; Ms = [double]$_.Groups[2].Value }
    })
}

function Read-LogFrom($w, [long]$At) {
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($At, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
}

# The first frame after $T0 (Unix ms) with $Until true of it: the snapshot's time_ms less $T0
function Wait-Frame($Until, [long]$T0, [string]$What) {
  Wait-State $Until -What $What -TimeoutSec 15 -IntervalMs 5 | Out-Null
  [long](Get-Test).LastState.time_ms - $T0
}

function Get-PaneCentre($w) {
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $p = @($s.panes | Where-Object active)[0]
  if (-not $p) { $p = @($s.panes)[0] }
  Get-RectCenter $p.rect
}

# -CacheSize's lines for a window's config: the four line caches' size, or nothing for the build's
function Get-CacheLua {
  if (-not $CacheSize) { return '' }
  ('shape', 'line_state', 'line_quad', 'line_to_ele_shape' | ForEach-Object { "config.${_}_cache_size = $CacheSize" }) -join "`n"
}

$results = [ordered]@{ Mux = [bool]$Mux; Skip = [bool]$Skip; Locked = (Test-WorkstationLocked); Build = $null }
Invoke-Test {
  if ('keys' -in $Only) {
    $w = Open-GtWindow 'keys' (@('echo') + @('burst') * $Busy) -Domain:$Mux
    $results.Build = (Read-State).build
    # The echoing tab in view, the others printing behind it
    $echo = (@(Get-Panes -Server:$Mux) | Sort-Object tab_id | Select-Object -First 1).pane_id
    Invoke-Cli activate-pane --pane-id $echo -Server:$Mux -Check | Out-Null
    Wait-State { param($s) $s.active_tab -eq 0 } -What 'the echoing tab in view' | Out-Null
    Start-Sleep 2
    $cursorX = { param($sw) $p = @($sw.panes | Where-Object active)[0]; if ($p) { [int]$p.cursor.x } else { -1 } }
    $x0 = & $cursorX (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle))
    if ($x0 -lt 0) { throw "the echoing pane's cursor isn't in the state file" }
    $a = Get-Mark $w
    $h = Resolve-Handle $w
    $keys0 = if ($a.Work) { [long]$a.Work.keys } else { 0 }
    $times = [Collections.Generic.List[double]]::new()
    $queue = [Collections.Generic.List[double]]::new()
    for ($i = 0; $i -lt $Keys; $i++) {
      $want = $x0 + $i + 1
      $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
      # 'X', as Send-Key posts it, with the clock read as it's posted
      $posted = [LbtermGt]::PostKey($h, 0x58)
      Watch-Foreground
      $shown = { param($sw) (& $cursorX $sw) -ge $want }.GetNewClosure()
      $times.Add((Wait-Frame $shown $t0 "key $($i + 1)'s echo"))
      # The echo's frame has the press taken before it: this key's, when it's the one more
      $seen = (Get-Test).LastState.work
      if ($seen -and $null -ne $seen.key_qpc -and [long]$seen.keys -eq $keys0 + $i + 1) {
        $queue.Add([Math]::Round(([long]$seen.key_qpc - $posted) * 1000.0 / [Diagnostics.Stopwatch]::Frequency, 3))
      }
      Assert-GtMemory $w
      $left = $KeyEveryMs - ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $t0)
      if ($left -gt 0) { Start-Sleep -Milliseconds $left }
    }
    $b = Get-Mark $w
    $d = Get-Delta $a $b
    $d | Add-Member -NotePropertyName KeyP50 -NotePropertyValue (Get-Pct $times 0.5)
    $d | Add-Member -NotePropertyName KeyP95 -NotePropertyValue (Get-Pct $times 0.95)
    $d | Add-Member -NotePropertyName KeyMax -NotePropertyValue ($times | Measure-Object -Maximum).Maximum
    $d | Add-Member -NotePropertyName Keys -NotePropertyValue @($times)
    # From the post to the loop taking it, for the keys whose taking the state file had
    $d | Add-Member -NotePropertyName QueueN -NotePropertyValue $queue.Count
    $d | Add-Member -NotePropertyName QueueP50 -NotePropertyValue (Get-Pct $queue 0.5)
    $d | Add-Member -NotePropertyName QueueP95 -NotePropertyValue (Get-Pct $queue 0.95)
    $d | Add-Member -NotePropertyName QueueMax -NotePropertyValue ($queue | Measure-Object -Maximum).Maximum
    $d | Add-Member -NotePropertyName QueueMean -NotePropertyValue $(if ($queue.Count) { [Math]::Round(($queue | Measure-Object -Average).Average, 3) })
    $d | Add-Member -NotePropertyName Queue -NotePropertyValue @($queue)
    $results.keys = $d
    $taken = if ($queue.Count) { "to the loop taking it p50 $($d.QueueP50) p95 $($d.QueueP95) max $($d.QueueMax) mean $($d.QueueMean) ms ($($queue.Count) keys), " } else { 'the loop''s taking of them not in the state file, ' }
    "keys (an echo in view, $Busy tabs printing behind it$(if ($Mux) { ', on a mux domain' })): $Keys keys, from the post ${taken}to the frame of the echo p50 $($d.KeyP50) p95 $($d.KeyP95) max $($d.KeyMax) ms; $($d.Frames) frames in $($d.Seconds) s; GUI thread $($d.GuiMs) ms; $(Format-Work $d)"
    Require-Work 'keys' $d
    if (-not $Measure -and $d.HasWork) {
      if (-not $d.waited) {
        Add-Unjudged "keys: no message was seen waiting in a drain of the task queue ($($d.long_drains) of $($d.drains) drains over 2 ms), so whether a drain stops for one wasn't judged"
      } else {
        $mean = $d.waited_us / $d.waited / 1000
        if ($mean -gt $WaitMs) { Add-Failure "keys: a message seen waiting waited $([Math]::Round($mean, 2)) ms on average in a drain after it was seen, over $WaitMs" }
        if ($d.long_waits / $d.waited -gt $LongWaitShare) { Add-Failure "keys: $($d.long_waits) of $($d.waited) messages seen waiting waited over 2 ms in a drain" }
      }
      if ($queue.Count -lt $Keys) { Add-Failure "keys: the state file had the loop's taking of $($queue.Count) of $Keys keys (work.keys and key_qpc)" }
    }
    Close-GtWindow $w -Domain:$Mux
  }
  if ('pointer' -in $Only) {
    $w = Open-GtWindow 'pointer' @('quiet') -PostedPointer
    if (-not $results.Build) { $results.Build = (Read-State).build }
    Wait-Settled -What 'the quiet window at rest' | Out-Null
    $x, $y = Get-PaneCentre $w
    # A move over the text paints nothing, so the state file says where the pointer is only once
    # something else paints: a frame to read by is the tab's title, set through the cli (its own
    # few tasks)
    $pid0 = (Get-Panes)[0].pane_id
    $kick = { param($t) $f = Get-StateFrame $w; Invoke-Cli set-tab-title --pane-id $pid0 $t -Check | Out-Null; Wait-State { param($s) $s.frame -gt $f }.GetNewClosure() -What "the title $t" | Out-Null }
    Send-Mouse $w Move $x $y
    & $kick 'one'
    $over = (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).pointer
    if (-not $over -or $null -eq $over.pane) { throw "the pointer posted to $x, $y isn't over the pane: $($over | ConvertTo-Json -Compress)" }
    $k0 = Get-Mark $w
    & $kick 'two'
    $a = Get-Mark $w
    $kd = Get-Delta $k0 $a
    $sent = [LbtermGt]::MoveEvery((Resolve-Handle $w), $x, $y, $Moves, 2000)
    Start-Sleep -Milliseconds 500
    & $kick 'three'
    $b = Get-Mark $w
    $d = Get-Delta $a $b
    $d | Add-Member -NotePropertyName Moves -NotePropertyValue $sent
    $d | Add-Member -NotePropertyName KickTasks -NotePropertyValue $kd.tasks
    $results.pointer = $d
    "pointer: $sent moves over the pane: $(if ($d.HasWork) { "$([Math]::Round(($d.tasks - $kd.tasks) / $sent, 3)) tasks a move beyond a title's own $($kd.tasks), $([Math]::Round($d.cursors / $sent, 3)) cursor sets a move" } else { 'no work' }); GUI thread $($d.GuiMs) ms ($([Math]::Round($d.GuiMs * 1000 / $sent, 1)) us a move); $(Format-Work $d)"
    Require-Work 'pointer' $d
    if (-not $Measure -and $d.HasWork) {
      if (($d.tasks - $kd.tasks) / $sent -gt $PerMove) { Add-Failure "pointer: $($d.tasks - $kd.tasks) tasks for $sent moves over the same text, over $PerMove a move" }
      if ($d.cursors / $sent -gt $PerMove) { Add-Failure "pointer: the cursor set $($d.cursors) times for $sent moves that left it the I-beam" }
    }
    Close-GtWindow $w
  }
  if ('ime' -in $Only) {
    $w = Open-GtWindow 'ime' @('status')
    if (-not $results.Build) { $results.Build = (Read-State).build }
    Start-Sleep 2
    $a = Get-Mark $w
    Start-Sleep $Seconds
    $b = Get-Mark $w
    $d = Get-Delta $a $b
    $results.ime = $d
    "ime: $($d.Frames) frames in $($d.Seconds) s, $(if ($d.HasWork -and $d.Frames) { "$([Math]::Round($d.ime / $d.Frames, 3)) IME moves and $([Math]::Round($d.tasks / $d.Frames, 2)) tasks a frame" } else { 'no work' }); GUI thread $($d.GuiMs) ms ms; $(Format-Work $d)"
    Require-Work 'ime' $d
    if (-not $Measure -and $d.HasWork) {
      if ($d.Frames -lt $Seconds * 5) { Add-Failure "ime: only $($d.Frames) frames in $($d.Seconds) s: the status line didn't reach the window" }
      elseif ($d.ime / $d.Frames -gt $ImePerFrame) { Add-Failure "ime: the IME's window moved $($d.ime) times in $($d.Frames) frames with the cursor still" }
    }
    Close-GtWindow $w
  }
  if ('focus' -in $Only) {
    $w = Open-GtWindow 'focus' @('dense')
    if (-not $results.Build) { $results.Build = (Read-State).build }
    Wait-Settled -What 'the dense text at rest' -TimeoutSec 20 | Out-Null
    $each = [Collections.Generic.List[object]]::new()
    $has = $true
    for ($i = 0; $i -lt $Changes; $i++) {
      $on = ($i % 2) -eq 1
      $a = Get-Mark $w
      Send-Focus $w -Off:(-not $on)
      $got = { param($sw) [bool]$sw.keyboard -eq $on }.GetNewClosure()
      Wait-Change $got -What "focus $(if ($on) { 'in' } else { 'out' }) $($i + 1)" | Out-Null
      $each.Add((Get-Delta $a (Get-Mark $w)))
    }
    $sum = { param($k) ($each | Measure-Object $k -Sum).Sum }
    $d = [pscustomobject]@{
      Changes = $Changes; Frames = & $sum 'Frames'; GuiMs = [Math]::Round((& $sum 'GuiMs'), 1); PaintMs = [Math]::Round((& $sum 'PaintMs'), 1)
      HasWork = [bool]($each | Where-Object HasWork); Lines = & $sum 'lines'; Accent = & $sum 'accent'; Tasks = & $sum 'tasks'
      LinesEach = @($each | ForEach-Object lines); GuiEach = @($each | ForEach-Object GuiMs)
    }
    $results.focus = $d
    "focus: $Changes changes, $($d.Frames) frames; a change: GUI thread $([Math]::Round($d.GuiMs / $Changes, 2)) ms (p50 $(Get-Pct $d.GuiEach 0.5)); $(if ($d.HasWork) { "lines laid out $([Math]::Round($d.Lines / $Changes, 1)) a change, accent reads $($d.Accent), tasks $($d.Tasks)" } else { 'no work' })"
    Require-Work 'focus' $d
    if (-not $Measure -and $d.HasWork) {
      if ($d.Lines / $Changes -gt $FocusLines) { Add-Failure "focus: $([Math]::Round($d.Lines / $Changes, 1)) lines laid out again a focus change, over ${FocusLines}: only the cursor's look changes" }
      if ($d.Accent -gt 0) { Add-Failure "focus: the accent colour was read $($d.Accent) times for $Changes focus changes with no setting changed" }
    }
    Close-GtWindow $w
  }
  if ('panes' -in $Only) {
    $w = Open-GtWindow 'panes' @('dense')
    if (-not $results.Build) { $results.Build = (Read-State).build }
    $first = (Get-Panes)[0].pane_id
    $out = (Invoke-Cli split-pane --pane-id $first --right -Program $Progs.dense -Merge) -join ' '
    if ($out.Trim() -notmatch '^\d+$') { throw "split-pane printed: $out" }
    $second = [int]$out.Trim()
    Wait-Change { param($s) @($s.panes).Count -eq 2 } -What 'two panes' -TimeoutSec 20 | Out-Null
    Start-Sleep 2
    Wait-Settled -What 'both panes at rest' | Out-Null
    $each = [Collections.Generic.List[object]]::new()
    $ids = @($first, $second)
    $now = (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).active_pane
    for ($i = 0; $i -lt $Changes; $i++) {
      $to = if ($now -eq $ids[0]) { $ids[1] } else { $ids[0] }
      $a = Get-Mark $w
      Invoke-Cli activate-pane --pane-id $to -Check | Out-Null
      $got = { param($sw) $sw.active_pane -eq $to }.GetNewClosure()
      Wait-Change $got -What "pane $to active" | Out-Null
      $each.Add((Get-Delta $a (Get-Mark $w)))
      $now = $to
    }
    $sum = { param($k) ($each | Measure-Object $k -Sum).Sum }
    $d = [pscustomobject]@{
      Changes = $Changes; Frames = & $sum 'Frames'; GuiMs = [Math]::Round((& $sum 'GuiMs'), 1); PaintMs = [Math]::Round((& $sum 'PaintMs'), 1)
      HasWork = [bool]($each | Where-Object HasWork); Lines = & $sum 'lines'; Tasks = & $sum 'tasks'
      LinesEach = @($each | ForEach-Object lines); GuiEach = @($each | ForEach-Object GuiMs)
    }
    $results.panes = $d
    "panes: $Changes moves, $($d.Frames) frames; a move: GUI thread $([Math]::Round($d.GuiMs / $Changes, 2)) ms (p50 $(Get-Pct $d.GuiEach 0.5)); $(if ($d.HasWork) { "lines laid out $([Math]::Round($d.Lines / $Changes, 1)) a move" } else { 'no work' })"
    Require-Work 'panes' $d
    if (-not $Measure -and $d.HasWork -and $d.Lines / $Changes -gt $PaneLines) { Add-Failure "panes: $([Math]::Round($d.Lines / $Changes, 1)) lines laid out again a pane move, over ${PaneLines}: with the compositor only the cursors' lines change" }
    Close-GtWindow $w
  }
  if ('tabs' -in $Only) {
    $w = Open-GtWindow 'tabs' (@('varied') * $Tabs) -Domain:$Mux -Lua "config.initial_rows = $TabRows`n$(Get-CacheLua)"
    if (-not $results.Build) { $results.Build = (Read-State).build }
    Start-Sleep 3
    $x, $y = Get-PaneCentre $w
    $sw0 = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    $active = [int]$sw0.active_tab
    $paneRows = [int]@($sw0.panes)[0].rows
    $each = [Collections.Generic.List[object]]::new()
    for ($i = 0; $i -lt $Tabs * ($Rounds + 1); $i++) {
      $want = ($active + 1) % $Tabs
      if ($Hop -eq 'pad') {
        $row = @(@((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).rail.rows) | Where-Object tab_index -eq $want)[0]
        if (-not $row -or -not $row.visible) { throw "tab $want has no pad showing on the rail to click" }
        $px, $py = Get-RectCenter $row.pad
      }
      $a = Get-Mark $w
      $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
      if ($Hop -eq 'pad') { Send-Click $w $px $py -Move; Send-Leave $w } else { Send-SideButton $w $x $y -Forward }
      $land = { param($sw) $sw.active_tab -eq $want }.GetNewClosure()
      $ms0 = Wait-Frame $land $t0 "hop $($i + 1) to tab $want"
      Wait-Settled -What "tab $want settled" | Out-Null
      $dd = Get-Delta $a (Get-Mark $w)
      $dd | Add-Member -NotePropertyName ToFrame -NotePropertyValue $ms0
      $dd | Add-Member -NotePropertyName Round -NotePropertyValue ([Math]::Floor($i / $Tabs))
      $each.Add($dd)
      $active = $want
    }
    # At rest after the last hop
    Start-Sleep 2
    $proc = Get-Process -Id $w.Pid
    $later = @($each | Where-Object Round -ge 1)
    $sum = { param($k) ($later | Measure-Object $k -Sum).Sum }
    $first = @($each | Where-Object Round -eq 0)
    $firstSum = { param($k) ($first | Measure-Object $k -Sum).Sum }
    $hops = [Math]::Max(1, $later.Count)
    $d = [pscustomobject]@{
      Tabs = $Tabs; PaneRows = $paneRows; Hop = $Hop; CacheSize = $(if ($CacheSize) { $CacheSize } else { 'default' })
      Hops = $later.Count; Frames = & $sum 'Frames'; GuiMs = [Math]::Round((& $sum 'GuiMs'), 1); PaintMs = [Math]::Round((& $sum 'PaintMs'), 1)
      HasWork = [bool]($each | Where-Object HasWork); Lines = & $sum 'lines'; LineShapes = & $sum 'line_shapes'; Shapes = & $sum 'shapes'
      FirstRoundLines = & $firstSum 'lines'; FirstRoundLineShapes = & $firstSum 'line_shapes'; FirstRoundShapes = & $firstSum 'shapes'
      ToFrameP50 = Get-Pct @($later | ForEach-Object ToFrame) 0.5; ToFrameP95 = Get-Pct @($later | ForEach-Object ToFrame) 0.95
      LinesEach = @($each | ForEach-Object lines); ShapesEach = @($each | ForEach-Object shapes); GuiEach = @($later | ForEach-Object GuiMs)
      PrivateMB = [Math]::Round($proc.PrivateMemorySize64 / 1MB); WorkingSetMB = [Math]::Round($proc.WorkingSet64 / 1MB)
    }
    # A build before the shaping counts has none to judge
    $counted = $d.HasWork -and $null -ne $later[0].line_shapes
    $results.tabs = $d
    $missed = if (-not $d.HasWork) { 'no work' }
    elseif ($counted) { "laid out $([Math]::Round($d.Lines / $hops, 1)) lines, shaped $([Math]::Round($d.LineShapes / $hops, 1)) lines and $([Math]::Round($d.Shapes / $hops, 1)) runs again a hop" }
    else { "laid out $([Math]::Round($d.Lines / $hops, 1)) lines again a hop (this build's state file doesn't count shaping)" }
    $how = if ($Hop -eq 'pad') { 'clicks on the pads' } else { 'side buttons' }
    "tabs ($Tabs of varied output, $Cols x $paneRows$(if ($Mux) { ', on a mux domain' }), $how, caches of $($d.CacheSize)): the first round laid out $($d.FirstRoundLines) lines$(if ($counted) { " and shaped $($d.FirstRoundLineShapes) lines and $($d.FirstRoundShapes) runs" }); after it $($d.Hops) hops, $($d.Frames) frames; a hop: GUI thread $([Math]::Round($d.GuiMs / $hops, 2)) ms (p50 $(Get-Pct $d.GuiEach 0.5)), to the frame p50 $($d.ToFrameP50) p95 $($d.ToFrameP95) ms, $missed; at rest after, the window's private memory $($d.PrivateMB) MB, working set $($d.WorkingSetMB) MB"
    Require-Work 'tabs' $d
    if (-not $Measure -and $d.HasWork) {
      $over = @()
      if ($d.Lines / $hops -gt $TabLines) { $over += "laid out $([Math]::Round($d.Lines / $hops, 1)) lines again a hop, over $TabLines" }
      if ($counted -and $d.LineShapes / $hops -gt $TabLines) { $over += "shaped $([Math]::Round($d.LineShapes / $hops, 1)) lines again a hop, over $TabLines" }
      if ($counted -and $d.Shapes / $hops -gt $TabShapes) { $over += "shaped $([Math]::Round($d.Shapes / $hops, 1)) runs again a hop, over $TabShapes" }
      if ($over) { Add-Failure "tabs: hopping back to tabs already seen ($Tabs of $paneRows rows, caches of $($d.CacheSize)) $($over -join '; '): the tabs' lines didn't stay in the caches" }
      if ($paneRows -lt $TabRows - 5) { Add-Failure "tabs: the panes have $paneRows rows, not the $TabRows asked for" }
    }
    Close-GtWindow $w -Domain:$Mux
  }
  if ('unseen' -in $Only) {
    $envs = @{ LBTERM_SLOW_EVERY = '1' }
    if ($Skip) { $envs.LBTERM_UNSEEN = 'skip' }
    $w = Open-GtWindow 'unseen' @('slow') -Environment $envs -Lua 'config.lbterm_slow_ms = 1' -NoFrame
    $h = Resolve-Handle $w
    Start-Sleep 3
    if (-not $results.Build) { $s = Get-StateSnapshot $w.StateFile; if ($s) { $results.Build = $s.build } }
    $phases = [ordered]@{}
    $restoreMs = $null
    foreach ($phase in 'shown', 'minimized', 'restored') {
      if ($phase -eq 'minimized') { [void][LbtermGt]::ShowWindow($h, 7); Start-Sleep -Milliseconds 500 }
      if ($phase -eq 'restored') {
        # Shown again without being activated, and how long until the frame that shows it
        $f = Get-StateFrame $w
        $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [void][LbtermGt]::ShowWindow($h, 4)
        $restoreMs = Wait-Frame { param($sw) $sw.frame -gt $f }.GetNewClosure() $t0 'the frame after it was shown again'
        Start-Sleep -Milliseconds 500
      }
      $locked = Test-WorkstationLocked
      $logAt = (Get-Item $w.Log).Length
      $a = Get-Mark $w
      Start-Sleep $Seconds
      $b = Get-Mark $w
      $paints = Get-Paints (Read-LogFrom $w $logAt)
      $d = Get-Delta $a $b
      $d | Add-Member -NotePropertyName Iconic -NotePropertyValue ([LbtermGt]::IsIconic($h))
      $d | Add-Member -NotePropertyName Locked -NotePropertyValue ($locked -and (Test-WorkstationLocked))
      $d | Add-Member -NotePropertyName Passes -NotePropertyValue @($paints | Where-Object Part -eq 'pass').Count
      $d | Add-Member -NotePropertyName DrawPresentMs -NotePropertyValue ([Math]::Round((($paints | Where-Object Part -in 'draw', 'present') | Measure-Object Ms -Sum).Sum, 1))
      $d | Add-Member -NotePropertyName DrawPresentMax -NotePropertyValue (($paints | Where-Object Part -in 'draw', 'present') | Measure-Object Ms -Maximum).Maximum
      if ($phase -eq 'restored') { $d | Add-Member -NotePropertyName RestoreMs -NotePropertyValue $restoreMs }
      $phases[$phase] = $d
      "unseen, ${phase}$(if ($d.Iconic) { ' (iconic)' })$(if ($d.Locked) { ', the workstation locked' })$(if ($Skip) { ', LBTERM_UNSEEN=skip' }): $($d.Frames) frames by the state file, $($d.Passes) passes logged, draws and presents $($d.DrawPresentMs) ms of wall time (longest $($d.DrawPresentMax)); GUI thread $($d.GuiMs) ms, process $($d.ProcessMs) ms in $($d.Seconds) s$(if ($phase -eq 'restored') { "; its first frame $restoreMs ms after it was shown" }); $(Format-Work $d)"
    }
    $results.unseen = $phases
    if (-not $Measure) {
      # One a second at most, and the span's ends may fall either side of one
      $most = [Math]::Ceiling($Seconds * $UnseenPerSecond) + 2
      $min = $phases.minimized
      if ($min.Iconic -and $min.Passes -gt $most) { Add-Failure "unseen, minimized: $($min.Passes) frames painted in $($min.Seconds) s, over $most" }
      if (-not $min.Iconic) { Add-Failure 'unseen: the window was never minimized' }
      if ($null -eq $restoreMs -or $restoreMs -gt $RestoreMs) { Add-Failure "unseen: the first frame came $restoreMs ms after the window was shown again, over $RestoreMs" }
      if ($Skip) {
        foreach ($phase in 'shown', 'restored') {
          $d = $phases[$phase]
          if ($d.Locked -and $d.Passes -gt $most) { Add-Failure "unseen, ${phase}: $($d.Passes) frames painted in $($d.Seconds) s with the workstation locked, over $most" }
        }
        if (-not ($phases.Values | Where-Object Locked)) { "unseen: the workstation wasn't locked, so how a locked desktop holds frames back wasn't seen" }
      }
    }
    Close-GtWindow $w
  }
  if ('lock' -in $Only) {
    $w = Open-GtWindow 'lock' @('slow') -Environment @{ LBTERM_UNSEEN = 'skip' } -NoFrame
    $h = Resolve-Handle $w
    Start-Sleep 3
    if (-not $results.Build) { $s = Get-StateSnapshot $w.StateFile; if ($s) { $results.Build = $s.build } }
    $phases = [ordered]@{}
    $unlockMs = $null
    $locked = Test-WorkstationLocked
    foreach ($phase in 'shown', 'locked', 'unlocked') {
      # WTS_SESSION_LOCK 7, WTS_SESSION_UNLOCK 8
      if ($phase -eq 'locked') { [void][LbtermGt]::Post($h, 0x02B1, 7, 0); Start-Sleep -Milliseconds 300 }
      if ($phase -eq 'unlocked') {
        $f = Get-StateFrame $w
        $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [void][LbtermGt]::Post($h, 0x02B1, 8, 0)
        $unlockMs = Wait-Frame { param($sw) $sw.frame -gt $f }.GetNewClosure() $t0 'the frame after the unlock'
        Start-Sleep -Milliseconds 300
      }
      $a = Get-Mark $w
      Start-Sleep $Seconds
      $d = Get-Delta $a (Get-Mark $w)
      if ($phase -eq 'unlocked') { $d | Add-Member -NotePropertyName UnlockMs -NotePropertyValue $unlockMs }
      $phases[$phase] = $d
      "lock, ${phase}$(if ($phase -ne 'shown') { ' (posted)' }): $($d.Frames) frames in $($d.Seconds) s; GUI thread $($d.GuiMs) ms, process $($d.ProcessMs) ms$(if ($phase -eq 'unlocked') { "; its first frame $unlockMs ms after the unlock" }); $(Format-Work $d)"
    }
    $results.lock = $phases
    if (-not $Measure) {
      $most = [Math]::Ceiling($Seconds * $UnseenPerSecond) + 2
      if ($phases.locked.Frames -gt $most) { Add-Failure "lock: $($phases.locked.Frames) frames painted in $($phases.locked.Seconds) s told the session is locked, over $most" }
      if ($phases.unlocked.Frames -lt $Seconds * 2) { Add-Failure "lock: only $($phases.unlocked.Frames) frames in $($phases.unlocked.Seconds) s once told it's unlocked" }
      if ($null -eq $unlockMs -or $unlockMs -gt $RestoreMs) { Add-Failure "lock: the first frame came $unlockMs ms after the unlock, over $RestoreMs" }
      if ($locked) { "lock: the workstation was locked, so the shown phase painted as a locked one" }
    }
    Close-GtWindow $w
  }
  if ('fill' -in $Only) {
    $w = Open-GtWindow 'fill' @('quiet') -Lua "config.initial_rows = $TabRows`n$(Get-CacheLua)"
    if (-not $results.Build) { $results.Build = (Read-State).build }
    Wait-Settled -What 'the quiet window at rest' | Out-Null
    Start-Sleep 2
    # Read at once: a Process object reads its numbers the first time one is asked for, and keeps them
    $p = Get-Process -Id $w.Pid
    $before = [pscustomobject]@{ PrivateMemorySize64 = $p.PrivateMemorySize64; WorkingSet64 = $p.WorkingSet64 }
    $a = Get-Mark $w
    $first = (Get-Panes)[0].pane_id
    New-Tab -From $first -Program $Progs.fill | Out-Null
    Wait-State { param($sw) @($sw.panes | Where-Object { $_.title -eq 'fill done' }).Count -gt 0 } -What "the $FillLines lines printed" -TimeoutSec 180 | Out-Null
    Wait-Settled -What 'the printed tab at rest' | Out-Null
    Start-Sleep 3
    $b = Get-Mark $w
    $after = Get-Process -Id $w.Pid
    $dd = Get-Delta $a $b
    $d = [pscustomobject]@{
      Lines = $FillLines; Wide = [bool]$FillWide; CacheSize = $(if ($CacheSize) { $CacheSize } else { 'default' }); Frames = $dd.Frames; Seconds = $dd.Seconds; GuiMs = $dd.GuiMs
      LaidOut = $dd.lines; LineShapes = $dd.line_shapes; Shapes = $dd.shapes
      PrivateMBBefore = [Math]::Round($before.PrivateMemorySize64 / 1MB); PrivateMB = [Math]::Round($after.PrivateMemorySize64 / 1MB)
      WorkingSetMBBefore = [Math]::Round($before.WorkingSet64 / 1MB); WorkingSetMB = [Math]::Round($after.WorkingSet64 / 1MB)
    }
    $results.fill = $d
    $what = if ($FillWide) { "lines the pane's width" } else { 'varied lines' }
    "fill ($FillLines $what in a second tab, caches of $($d.CacheSize)): $($d.Frames) frames in $($d.Seconds) s, $($d.LaidOut) lines laid out, $($d.LineShapes) lines and $($d.Shapes) runs shaped; GUI thread $($d.GuiMs) ms; at rest, the window's private memory $($d.PrivateMBBefore) MB before and $($d.PrivateMB) after ($($d.PrivateMB - $d.PrivateMBBefore) MB more, the tab's scrollback with it), working set $($d.WorkingSetMBBefore) and $($d.WorkingSetMB) MB"
    Close-GtWindow $w
  }
  [pscustomobject]$results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'gui-thread.json')
} -Passed "gui-thread$(if ($Mux) { ' (mux)' }): input waits on no backlog, unchanged cursors and IME windows aren't set again, focus and pane moves lay out only what they change, hopping back to any of $Tabs tabs lays out and shapes nothing again, and a window that can't be seen paints once a second at most"
