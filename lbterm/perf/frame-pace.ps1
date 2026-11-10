#Requires -Version 7
# Variant: -Mux
# Covers: window/src/os/windows/window.rs, window/src/os/windows/connection.rs, window/src/os/windows/pace.rs, window/src/spawn.rs, lbterm-extras/src/pace.rs, wezterm-gui/src/state_probe.rs
<#
How the GUI thread paces its frames (lbterm-extras/src/pace.rs; window/src/os/windows/pace.rs,
wm_paint in window.rs and the message loop in connection.rs), in test windows with -MaxFps as
their max_fps. With -Mux the lines and maximize windows' tabs live in a test lbterm-mux-server, as
Lachlan's do, and what a pane prints reaches the GUI as the client's PDUs.

1. lines, in a window drawn as Lachlan's is (New-TestConfig: WebGpu, the compositor), 160 x 50:
   the pane prints a line every 50 ms in two writes 5 ms apart, as a program updating a status
   line does, so the line's end comes while the frame its start painted holds the next back.
   Over -Seconds: frames (the state file's), the GUI thread's CPU and the process's
   (QueryThreadCycleTime and QueryProcessCycleTime, exact), and the state file's `pace`: the
   message loop's turns, the paints and the thread's own cycles in them (a wait on DWM inside a
   paint isn't the thread's), the WM_PAINTs that came while a frame was held back, and how late
   the holds ended. A held WM_PAINT that left the window unvalidated came straight back every
   time the queue was empty until the hold ended: thousands of turns a second for nothing. With
   its two writes together (until 2026-10-30) nothing landed in a hold on this branch, locked or
   not, so it judged nothing, and a build without the fix passed.
2. stream, in a Software window (GDI presents, which never wait on DWM, so the frames show the
   pacing alone, on a locked desktop too), 60 x 12, never on the mux domain: the pane prints
   without a pause, so every frame has more to show and every frame is held. Frames a second
   against max_fps, or the screen's refresh rate if that's lower (the pacing never paints faster
   than the screen shows), the gaps between frames (the slow log's pass lines, LBTERM_SLOW_EVERY
   with lbterm_slow_ms = 1, to the millisecond), how late the holds ended, and the held WM_PAINTs
   and CPU as lines has them. A hold that ends on Windows' 15.6 ms timer tick paints far fewer
   than max_fps, unevenly. -StreamFrontEnd WebGpu runs it drawn as Lachlan's instead, which on a
   locked desktop DWM can take a frame of every 260 ms or so. On a mux domain a pane printing
   without a pause holds the GUI thread in the client's own work, at 1.4 to 2 frames a second on
   every build (2026-10-30), which isn't the pacing's to judge.
3. maximize, drawn as Lachlan's: the pointer on the strip's maximize button and off it again
   (posted client moves, LBTERM_POSTED_POINTER), then WM_NCMOUSEMOVE over it (HTMAXBUTTON, as
   Windows 11 sends it once it knows where the button is, for snap layouts) every millisecond for
   2 s, posted. Each one painted a whole frame at once, around the hold; only the hover's change
   should paint, and the hover has to show.

It fails when a held WM_PAINT comes back: more than -HeldPerFrame held a frame in lines or
-StreamHeldPerFrame in stream, or the GUI thread spending more than -CpuShare (lines) or
-StreamCpuShare (stream) of the span beyond its paints' own cycles, judged when five or more
WM_PAINTs came during a hold. Stream's frames a second, gaps and lateness can't show one: the
timer still ends each hold on time while the loop goes round. It fails too when more than
-LateShare of the holds end over 2 ms late (with five or more), when stream paints under -FpsShare
of what it should or its gaps' 95th percentile is over a period and a quarter, or when the
maximize moves paint more than -MaxButtonFrames frames or leave no hover. A check with too little
to judge says so, in the output and result.json's notes, rather than passing. -Measure prints the
numbers and fails on nothing; frame-pace.json in the output folder has every number.

Measured on 2026-10-30, three runs each, interleaved, the desktop locked and live alike unless
said, max_fps 50 unless said, this branch against itself with only ValidateRect taken out of the
held WM_PAINT (which the checks above must fail):

  lines      this branch 0.5 held a frame, one hold a line (128 to 130 in 10 s), 104 to 118 ms
             of the GUI thread beyond its paints (149 on a mux domain), holds 0.29 to 0.34 ms
             late. Without ValidateRect 41 to 52 held a frame, 613 to 675 ms beyond the paints
  stream     this branch 6.8 to 7.2 held a frame (one each time output asked for a frame), 50
             frames a second, the GUI thread at 27 to 32% of a core, 535 to 770 ms beyond its
             paints, gaps p50 20 p95 21 to 22 ms, 0 or 1 hold over 2 ms late. Without
             ValidateRect 181 to 320 held a frame, 80 to 93% of a core, 5,591 to 7,063 ms beyond
             the paints, and the same frames and gaps. Upstream's wm_paint (main with this
             branch's counts, locked): 562 to 581 held a frame, 31.8 to 32.1 frames a second at
             90 to 92%, gaps 26 to 48 ms; max_fps 120 on the 60 Hz screen 60.0 to 62.1 at 83 to
             95%, where this branch paints 60.0 to 60.1 at 33%.
             Drawn as Lachlan's (-StreamFrontEnd WebGpu -Measure), live desktop, the installed
             build before this branch: 29.9 to 31.7 frames a second at 73 to 82% of a core, gaps
             p50 31 ms; this branch 49.9 to 50 at 42 to 45%, gaps p50 20 p95 22. At max_fps 120
             on the 60 Hz screen 33.1 to 36.5 at 82 to 92%; this branch 59.9 to 60 at 48 to 51%,
             gaps p50 17 p95 19
  maximize   upstream's 2,000 moves painted 2,002 frames, 8.0 to 8.2 s of the GUI thread; this
             branch 2 frames, 116 to 142 ms

  pwsh -File lbterm\perf\frame-pace.ps1 [-Mux] [-MaxFps 50] [-Seconds 10] [-Only lines,stream,maximize] [-StreamFrontEnd Software|WebGpu] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [switch]$Mux,
  [int]$MaxFps = 50,
  [int]$Seconds = 10,
  [string[]]$Only = @('lines', 'stream', 'maximize'),
  [switch]$Measure,
  # The GUI thread's CPU in lines beyond its paints' own, as a share of the span
  [double]$CpuShare = 0.05,
  # Held WM_PAINTs a frame, at most, in lines
  [double]$HeldPerFrame = 4,
  # The same in stream, which holds every frame and asks for one for each batch of output: one
  # held WM_PAINT for each, about 7 a frame (2026-10-30), where a held one coming back made
  # hundreds
  [double]$StreamCpuShare = 0.25,
  [double]$StreamHeldPerFrame = 20,
  # Holds ending over 2 ms late, as a share of those that ended with a frame asked
  [double]$LateShare = 0.1,
  # Frames a second in stream, as a share of what it should paint
  [double]$FpsShare = 0.9,
  [int]$MaxButtonFrames = 10,
  [ValidateSet('Software', 'WebGpu')][string]$StreamFrontEnd = 'Software'
)
$ErrorActionPreference = 'Stop'
# -Only a,b through pwsh -File comes as one string
$Only = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
foreach ($o in $Only) { if ($o -notin 'lines', 'stream', 'maximize') { throw "-Only takes lines, stream and maximize, not '$o'" } }
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('frame-pace' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
public static class LbtermPace {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct MONITORINFOEX { public int cbSize; public int l, t, r, b, wl, wt, wr, wb; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFOEX mi);
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
    public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
  }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettings(string name, int mode, ref DEVMODE dm);
  const uint THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000;
  // Cycles a second, spinning on this thread for ms: the cycle counter's rate
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
  // The process's first thread, the GUI's: the one that started first
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      return p.Threads.Cast<ProcessThread>().OrderBy(t => t.StartTime).First().Id;
    }
  }
  // The refresh rate of the screen the window is on, as the mode list's whole number (the GUI
  // reads the exact rate from QueryDisplayConfig: 160 here is 159.952, which FpsShare's margin
  // covers); 0 when Windows doesn't say
  public static int RefreshHz(IntPtr h) {
    var mi = new MONITORINFOEX(); mi.cbSize = Marshal.SizeOf(mi);
    if (!GetMonitorInfo(MonitorFromWindow(h, 2), ref mi)) return 0;
    var dm = new DEVMODE(); dm.dmSize = (short)Marshal.SizeOf(dm);
    if (!EnumDisplaySettings(mi.szDevice, -1, ref dm)) return 0;
    return dm.dmDisplayFrequency > 1 ? dm.dmDisplayFrequency : 0;
  }
  // msg posted every intervalUs for ms, spinning between: how many went
  public static int PostEvery(IntPtr h, uint msg, IntPtr w, IntPtr l, int intervalUs, int ms) {
    var sw = Stopwatch.StartNew(); long next = 0; int n = 0;
    long tick = Stopwatch.Frequency * intervalUs / 1000000;
    long end = Stopwatch.Frequency * ms / 1000;
    while (sw.ElapsedTicks < end) {
      if (sw.ElapsedTicks >= next) { PostMessage(h, msg, w, l); n++; next += tick; }
      else Thread.SpinWait(20);
    }
    return n;
  }
}
'@

# A config of the scenario's own, each in a folder of its own (a config's folder is watched)
function New-PaceConfig([string]$Name, [string]$FrontEnd, [int]$Cols, [int]$Rows) {
  $cfg = Join-Path $T.Out "config-$Name\frame-pace.lua"
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  $domain = if ($Mux) {
    @"
config.unix_domains = { {
  name = 'pacemux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  } else { '' }
  $front = if ($FrontEnd -ne 'WebGpu') { "config.front_end = '$FrontEnd'`nconfig.lbterm_compositor = false" } else { '' }
  New-TestConfig -Path $cfg -Lua @"
config.max_fps = $MaxFps
config.lbterm_slow_ms = 1
config.initial_cols = $Cols
config.initial_rows = $Rows
config.cursor_blink_rate = 0
$front
$domain
"@
  $cfg
}

$programs = @{
  # A line every 50 ms, in two writes 5 ms apart: a status's start, then its time and the line's
  # end, which comes while the frame the start painted holds the next back
  lines  = New-PwshProgram -Script "`$i = 0; while (`$true) { [Console]::Out.Write(`"line `$i `"); [Threading.Thread]::Sleep(5); [Console]::Out.Write([DateTime]::Now.ToString('HH:mm:ss.fff') + `"``n`"); `$i++; [Threading.Thread]::Sleep(50) }"
  # Without a pause
  stream = New-PwshProgram -Script "`$s = 'x' * 61 + ' '; while (`$true) { [Console]::Out.Write(`$s) }"
  # Nothing
  quiet  = New-PwshProgram -Script 'Start-Sleep 900'
}

$rate = [LbtermPace]::Rate(300)
$ms = { param([ulong]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }

# Paint parts the slow log names in a span of the log, with when each was logged
function Get-Paints([string]$Text) {
  @([regex]::Matches($Text, '(?m)^(\d\d):(\d\d):(\d\d)\.(\d{3})\s+WARN\s+lbterm::slow\s+> slow paint window-\d+/(\w+): ([\d.]+) ms$') | ForEach-Object {
      $g = $_.Groups
      [pscustomobject]@{
        At = ((([int]$g[1].Value * 60) + [int]$g[2].Value) * 60 + [int]$g[3].Value) * 1000 + [int]$g[4].Value
        Part = $g[5].Value; Ms = [double]$g[6].Value
      }
    })
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

# A window running the scenario's program, at its first frame
function Open-PaceWindow([string]$Program, [string]$Config, [switch]$PostedPointer, [switch]$Local) {
  $open = @{ ConfigFile = $Config; Program = $programs[$Program]; Environment = @{ LBTERM_SLOW_EVERY = '1' }; TimeoutSec = 20; PostedPointer = $PostedPointer }
  $domain = $Mux -and -not $Local
  if ($domain) { $open.Domain = 'pacemux'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window''s first frame' | Out-Null
  # On a mux domain `connect` starts the server's own shell, not the program: the program goes in
  # a tab of its own there, and the shell's goes once the window has the new tab (before, the
  # window can be left with none, and closes)
  if ($domain -and $Program -ne 'quiet') {
    $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
      $panes = @(Get-Panes -Server)
      if (-not $panes) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
    $shell = $panes[0].pane_id
    Invoke-Cli spawn --pane-id $shell -Server -Program $programs[$Program] -Check | Out-Null
    Wait-State { param($s) @($s.tabs).Count -eq 2 } -What "the program's tab in the window" | Out-Null
    Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
    Wait-State { param($s) @($s.panes).Count -eq 1 -and $null -ne @($s.panes)[0].server_id -and @($s.panes)[0].server_id -ne $shell } -What "the program's tab on the server" | Out-Null
  }
  $w | Add-Member -NotePropertyName Main -NotePropertyValue ([LbtermPace]::MainThread($w.Pid))
  $w
}

# A scenario's window gone, and on a mux domain its server, so the next starts with its own
function Close-PaceWindow($w) {
  Stop-TestWindow $w
  if ($Mux) { Stop-TestServers }
}

# What the window did over the next $Sec seconds
function Get-Span($w, [double]$Sec) {
  $read = {
    $s = Get-StateSnapshot $w.StateFile
    $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
    [pscustomobject]@{ Frame = [long]$sw.frame; Pace = $s.pace }
  }
  $logAt = (Get-Item $w.Log).Length
  $a = & $read
  $c0 = [LbtermPace]::ThreadCycles($w.Main); $p0 = [LbtermPace]::ProcessCycles($w.Pid)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Start-Sleep -Milliseconds ([int]($Sec * 1000))
  $c1 = [LbtermPace]::ThreadCycles($w.Main); $p1 = [LbtermPace]::ProcessCycles($w.Pid)
  $span = $clock.Elapsed.TotalSeconds
  $b = & $read
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($logAt, 'Begin'); $text = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  $paints = Get-Paints $text
  $passes = @($paints | Where-Object Part -eq 'pass' | ForEach-Object At)
  $gaps = @(for ($i = 1; $i -lt $passes.Count; $i++) { [double]($passes[$i] - $passes[$i - 1]) })
  $frames = $b.Frame - $a.Frame
  $gui = & $ms ($c1 - $c0)
  $r = [ordered]@{
    Seconds = [Math]::Round($span, 2); Frames = $frames; Fps = [Math]::Round($frames / $span, 1)
    GuiMs = $gui; GuiShare = [Math]::Round($gui / ($span * 1000), 3); ProcessMs = & $ms ($p1 - $p0)
    GuiMsPerFrame = if ($frames) { [Math]::Round($gui / $frames, 2) } else { $null }
    # The slow log's paint parts are wall time, waits on DWM in them included
    PaintLoggedMs = [Math]::Round(($paints | Measure-Object Ms -Sum).Sum, 1)
    Gaps = $gaps.Count; GapP50 = Get-Pct $gaps 0.5; GapP95 = Get-Pct $gaps 0.95
    GapMin = ($gaps | Measure-Object -Minimum).Minimum; GapMax = ($gaps | Measure-Object -Maximum).Maximum
    # From the state file's pace (a build before it has none)
    Turns = $null; Paints = $null; Held = $null; HeldPerFrame = $null; PaintCpuMs = $null; BeyondPaintMs = $null
    Holds = $null; Late = $null; LateMeanMs = $null
  }
  if ($a.Pace -and $b.Pace) {
    $d = { param($name) [long]$b.Pace.$name - [long]$a.Pace.$name }
    $r.Turns = & $d 'turns'; $r.Paints = & $d 'paints'; $r.Held = & $d 'held'
    $r.HeldPerFrame = if ($frames) { [Math]::Round($r.Held / $frames, 2) } else { $null }
    $r.PaintCpuMs = & $ms ([ulong](& $d 'paint_cycles'))
    $r.BeyondPaintMs = [Math]::Round($gui - $r.PaintCpuMs, 1)
    $r.Holds = & $d 'holds'; $r.Late = & $d 'late'
    $r.LateMeanMs = if ($r.Holds) { [Math]::Round((& $d 'late_us') / $r.Holds / 1000, 2) } else { $null }
  }
  [pscustomobject]$r
}

function Format-Span([string]$Name, $r) {
  $pace = if ($null -ne $r.Turns) {
    "; paints' own CPU $($r.PaintCpuMs) ms, beyond them $($r.BeyondPaintMs) ms; $($r.Turns) loop turns, $($r.Paints) paints, $($r.Held) WM_PAINTs held ($($r.HeldPerFrame) a frame); $($r.Holds) holds ended with a frame asked, $($r.Late) over 2 ms late, $($r.LateMeanMs) ms late on average"
  } else { '; no pace in the state file' }
  "${Name}: $($r.Frames) frames in $($r.Seconds) s ($($r.Fps)/s); GUI thread $($r.GuiMs) ms ($([Math]::Round($r.GuiShare * 100, 1))% of a core, $($r.GuiMsPerFrame) ms a frame); process $($r.ProcessMs) ms; paints logged $($r.PaintLoggedMs) ms of wall time; gaps between passes p50 $($r.GapP50) p95 $($r.GapP95) min $($r.GapMin) max $($r.GapMax) ms$pace"
}

# A scenario that judged nothing says so, in its output and the test's notes (result.json, the
# e2e runner's report), rather than passing for it
function Add-Unjudged([string]$Message) {
  "NOTE: $Message"
  (Get-Test).Notes.Add($Message)
}

# The holds' lateness, when there are enough to judge
function Test-Late([string]$Name, $r) {
  if ($null -eq $r.Holds) { return }
  if ($r.Holds -lt 5) { Add-Unjudged "${Name}: only $($r.Holds) holds ended with a frame asked, too few to judge how late they end"; return }
  if ($r.Late / $r.Holds -gt $LateShare) { Add-Failure "${Name}: $($r.Late) of $($r.Holds) holds ended over 2 ms late, $($r.LateMeanMs) ms on average: on Windows' 15.6 ms timer tick?" }
}

# A held WM_PAINT coming back round the loop: held WM_PAINTs a frame and the GUI thread's CPU
# beyond its paints, when enough WM_PAINTs came during a hold to judge (a frame asked for during
# a hold is what makes one; with none there's nothing that could come back). Not by the holds
# that ended with a frame asked: one going round paints as soon as its hold's time is up, before
# the loop ends the hold, so a spin cuts those (10 to 14 in lines, 37 to 49 in stream, where
# this build has 129 and 485, 2026-10-30)
function Test-Held([string]$Name, $r, [double]$PerFrame, [double]$Share) {
  if ($r.Held -lt 5) {
    Add-Unjudged "${Name}: only $($r.Held) WM_PAINTs came while a frame was held back, too few to judge whether a held one comes back"
    return
  }
  if ($r.BeyondPaintMs -gt $Share * $r.Seconds * 1000) { Add-Failure "${Name}: the GUI thread spent $($r.BeyondPaintMs) ms beyond its paints' $($r.PaintCpuMs) ms in $($r.Seconds) s, over $($Share * 100)% of the span" }
  if ($r.HeldPerFrame -gt $PerFrame) { Add-Failure "${Name}: $($r.Held) WM_PAINTs held back in $($r.Frames) frames, $($r.HeldPerFrame) a frame: a held one is coming back" }
}

$results = [ordered]@{ Mux = [bool]$Mux; MaxFps = $MaxFps; Seconds = $Seconds; Locked = (Test-WorkstationLocked) }
Invoke-Test {
  if ('lines' -in $Only) {
    $w = Open-PaceWindow 'lines' (New-PaceConfig 'lines' 'WebGpu' 160 50)
    Start-Sleep 2
    $r = Get-Span $w $Seconds
    $results.lines = $r
    Format-Span 'lines' $r
    if (-not $Measure) {
      if ($r.Frames -lt $Seconds * 3) { Add-Failure "lines: only $($r.Frames) frames in $($r.Seconds) s: the pane's lines didn't reach the window" }
      if ($null -eq $r.Held) { Add-Failure "lines: the state file has no pace (a build before frame pacing)" }
      else {
        Test-Held 'lines' $r $HeldPerFrame $CpuShare
        Test-Late 'lines' $r
      }
    }
    Close-PaceWindow $w
  }
  if ('stream' -in $Only) {
    # Never on the mux domain (the header says why)
    $w = Open-PaceWindow 'stream' (New-PaceConfig 'stream' $StreamFrontEnd 60 12) -Local
    Start-Sleep 2
    $hz = [LbtermPace]::RefreshHz((Resolve-Handle $w))
    $should = if ($hz) { [Math]::Min($MaxFps, $hz) } else { $MaxFps }
    $r = Get-Span $w $Seconds
    $r | Add-Member -NotePropertyName RefreshHz -NotePropertyValue $hz
    $r | Add-Member -NotePropertyName ShouldFps -NotePropertyValue $should
    $results.stream = $r
    Format-Span "stream ($StreamFrontEnd, max_fps $MaxFps, the screen $hz Hz, $should a second the most)" $r
    if (-not $Measure) {
      if ($r.Fps -lt $FpsShare * $should) { Add-Failure "stream: $($r.Fps) frames a second where it should paint $should (max_fps $MaxFps, the screen $hz Hz), under $($FpsShare * 100)% of it" }
      $period = 1000 / $should
      if ($r.Gaps -ge 20 -and $r.GapP95 -gt 1.25 * $period) { Add-Failure "stream: 95% of frames came within $($r.GapP95) ms of the last, over a period and a quarter ($([Math]::Round(1.25 * $period, 1)) ms)" }
      if ($null -eq $r.Held) { Add-Failure "stream: the state file has no pace (a build before frame pacing)" }
      else {
        Test-Held 'stream' $r $StreamHeldPerFrame $StreamCpuShare
        Test-Late 'stream' $r
      }
    }
    Close-PaceWindow $w
  }
  if ('maximize' -in $Only) {
    $w = Open-PaceWindow 'quiet' (New-PaceConfig 'maximize' 'WebGpu' 160 50) -PostedPointer
    $e = Wait-Settled -What 'the quiet window at rest'
    $max = @($e.strip.items | Where-Object name -eq 'maximize')[0]
    if (-not $max) { throw "no maximize button on the strip: $(@($e.strip.items | ForEach-Object name) -join ', ')" }
    $x, $y = Get-RectCenter $max.rect
    # The pointer's way there in the client area first, so the GUI knows where the button is, as
    # it does before Windows sends WM_NCMOUSEMOVE for it; then off it, onto the pane
    Send-Mouse $w Move $x $y
    Wait-Change { param($s) $s.pointer -and $s.pointer.button -eq 'maximize' } -What 'the pointer on maximize' | Out-Null
    $pane = @($e.panes)[0]
    Send-Mouse $w Move ([int]($pane.rect.x + $pane.rect.w / 2)) ([int]($pane.rect.y + $pane.rect.h / 2))
    Wait-Change { param($s) $s.pointer -and $s.pointer.button -ne 'maximize' } -What 'the pointer off maximize' | Out-Null
    $p = ConvertTo-ScreenPoint $w $x $y
    $lp = [LbtermE2E.Win]::LParam($p.X, $p.Y)
    $f0 = Get-StateFrame $w
    $c0 = [LbtermPace]::ThreadCycles($w.Main)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    # WM_NCMOUSEMOVE, HTMAXBUTTON
    $posted = [LbtermPace]::PostEvery((Resolve-Handle $w), 0x00A0, [IntPtr]9, $lp, 1000, 2000)
    # Until the queue has been worked through: the frame count still for 500 ms, at most 30 s
    $last = -1; $still = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt 30) {
      $f = Get-StateFrame $w
      if ($f -ne $last) { $last = $f; $still.Restart() } elseif ($still.ElapsedMilliseconds -ge 500) { break }
      Start-Sleep -Milliseconds 20
    }
    $frames = $last - $f0
    $gui = & $ms ([LbtermPace]::ThreadCycles($w.Main) - $c0)
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    $r = [pscustomobject]@{ Posted = $posted; Frames = $frames; GuiMs = $gui; Seconds = [Math]::Round($clock.Elapsed.TotalSeconds - 0.5, 2); Hover = $sw.pointer.button }
    $results.maximize = $r
    "maximize: $posted WM_NCMOUSEMOVEs posted over 2 s, $frames frames painted, GUI thread $gui ms, worked through in $($r.Seconds) s; the pointer on $(if ($r.Hover) { $r.Hover } else { 'no button' })"
    if (-not $Measure) {
      if ($frames -gt $MaxButtonFrames) { Add-Failure "maximize: $frames frames for $posted moves over the button: only the hover's change should paint" }
      if ($r.Hover -ne 'maximize') { Add-Failure "maximize: after the moves over it the pointer is on $(if ($r.Hover) { $r.Hover } else { 'no button' }), not maximize: the hover didn't show" }
    }
    Close-PaceWindow $w
  }
  [pscustomobject]$results | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'frame-pace.json')
} -Passed "frame-pace$(if ($Mux) { ' (mux)' }): steady output costs the GUI thread what its paints cost, frames come at max_fps on time, and the maximize button paints only its hover"
