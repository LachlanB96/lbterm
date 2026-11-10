#Requires -Version 7
<#
Key-to-screen latency: from a real keypress (SendInput) to its glyph on the screen, for LBTerm on
its local domain and on a mux domain and for Windows Terminal, one at a time, the same way.
lbterm/BENCH.md says how to read it. (Upstream WezTerm was a kind until 2026-11-04, when it was
uninstalled.)

It needs the keyboard and the foreground, so it runs for real only when Lachlan has said he's away
from the PC: -Run -Away. Without -Run it's a dry run that does everything that needs no focus.

For each terminal (in -Kinds order, -Rounds times round, so drift in the PC's load falls on all):
  1. It opens the terminal unfocused on the right-most screen through the benchmark's launcher,
     running keyprobe.ps1: a row of ten solid cells, then a ">" prompt read with ConPTY's own line
     input, which echoes what's typed (no shell in the path).
  2. It finds the cell grid from the solid row in a capture of the window (PrintWindow) and takes
     the cell after the prompt, where a typed glyph lands, as the probe.
  3. -Run: it gives the window the foreground (AttachThreadInput and SetForegroundWindow: no
     input of its own for that), reads the probe cell off the screen (GDI BitBlt of the desktop,
     as DWM has composed it) for 1.5 s to learn its resting looks (a blinking cursor has two),
     then -Samples times: a pause of 80 to 200 ms, an "x" key down and up by SendInput, and the
     probe read in a tight loop until it looks like none of its resting looks; the time from the
     SendInput call to that read is the sample. Then Backspace, and a wait for the cell to rest.
  4. It closes the terminal and goes on to the next; at the end it gives the foreground back to
     the window that had it.

Lachlan coming back ends it at once: low-level keyboard and mouse hooks watch for any input that
isn't injected, and the first one stops the samples, closes the terminal and gives the foreground
back. It won't start unless the keyboard and mouse have been idle for -IdleSec (60 s), the
workstation is unlocked, and -Away is given.

The dry run (no -Run) checks, with nothing focused and no input sent: the right-most screen and its
refresh rate; LBTerm on each domain opening there unfocused with the probe; the cell grid
found in its capture (key-<kind>-probe.png, the probe cell outlined); whether the window shows on
the screen as PrintWindow draws it (another window over it would spoil a screen read, though in
-Run the window has the foreground and is on top); how fast the probe can be read off the screen;
the probe's resting looks over 1.5 s; and that the hooks and SendInput are there to call. Windows
Terminal takes the foreground as it opens, so the dry run doesn't open it: its grid is found in
the -Run.

What a sample includes: the terminal reading the key, sending it to ConPTY, ConPTY echoing it, the
terminal parsing and drawing it, presenting, and DWM composing the desktop (at the screen's
refresh rate) before BitBlt can read it. It doesn't include the keyboard's own latency or the
display's scan-out. The screen read's own time is the resolution (printed).

  pwsh -File lbterm\bench\key-latency.ps1 -Work <scratch folder>                    # the dry run
  pwsh -File lbterm\bench\key-latency.ps1 -Work <scratch folder> -Run -Away         # Lachlan away
#>
param(
  [string[]]$Kinds = @('lbterm', 'lbterm-mux', 'wt'),
  [int]$Samples = 100,
  [int]$Rounds = 2,
  [Parameter(Mandatory)][string]$Work,
  [string]$From = (Join-Path $env:LOCALAPPDATA 'Programs\LBTerm'),
  [string]$Out,
  [int]$Cols = 100,
  [int]$Rows = 30,
  [string]$WtProfile = 'PowerShell',
  [int]$IdleSec = 60,
  [switch]$Run,
  [switch]$Away
)
$ErrorActionPreference = 'Stop'
$Kinds = @($Kinds -split ',' | ForEach-Object Trim | Where-Object { $_ })
foreach ($k in $Kinds) { if ($k -notin 'lbterm', 'lbterm-mux', 'wt') { throw "-Kinds $k`: not one of lbterm, lbterm-mux, wt" } }
if ($Run -and -not $Away) { throw '-Run sends real keys and takes the foreground: only with -Away, once Lachlan has said he is away from the PC' }
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot 'bench-lib.ps1')
foreach ($v in @(Get-ChildItem Env: | Where-Object { $_.Name -match '^(WEZTERM_|TERM_PROGRAM|CLAUDE|ANTHROPIC_|LBTERM_)' })) { Remove-Item "Env:$($v.Name)" }

if (-not ('LbtermKey.Probe' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
namespace LbtermKey {
  // Reads a rectangle of the desktop as DWM last composed it (GDI BitBlt of the screen DC)
  public sealed class Grabber : IDisposable {
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
    [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr d, int x, int y, int w, int h, IntPtr s, int sx, int sy, uint rop);
    [StructLayout(LayoutKind.Sequential)] struct BITMAPINFOHEADER { public int biSize, biWidth, biHeight; public short biPlanes, biBitCount; public int biCompression, biSizeImage, biXPelsPerMeter, biYPelsPerMeter, biClrUsed, biClrImportant; }
    [DllImport("gdi32.dll")] static extern IntPtr CreateDIBSection(IntPtr dc, ref BITMAPINFOHEADER bmi, uint usage, out IntPtr bits, IntPtr sec, uint off);
    IntPtr screen, mem, dib, old, bits; public readonly int X, Y, W, H;
    public Grabber(int x, int y, int w, int h) {
      X = x; Y = y; W = w; H = h;
      screen = GetDC(IntPtr.Zero); mem = CreateCompatibleDC(screen);
      var bi = new BITMAPINFOHEADER { biSize = 40, biWidth = w, biHeight = -h, biPlanes = 1, biBitCount = 32 };
      dib = CreateDIBSection(mem, ref bi, 0, out bits, IntPtr.Zero, 0); old = SelectObject(mem, dib);
    }
    public int[] Grab() {
      BitBlt(mem, 0, 0, W, H, screen, X, Y, 0x00CC0020);
      var p = new int[W * H]; Marshal.Copy(bits, p, 0, p.Length); return p;
    }
    public void Dispose() { SelectObject(mem, old); DeleteObject(dib); DeleteDC(mem); ReleaseDC(IntPtr.Zero, screen); }
  }

  public static class Probe {
    static int Lum(int c) { return (((c >> 16) & 255) * 3 + ((c >> 8) & 255) * 6 + (c & 255)) / 10; }
    // How many pixels differ in brightness by more than `by`
    public static int Diff(int[] a, int[] b, int by) { int n = 0; for (int i = 0; i < a.Length; i++) if (Math.Abs(Lum(a[i]) - Lum(b[i])) > by) n++; return n; }
    static bool Resting(int[] f, List<int[]> rest, int by, int most) { foreach (var r in rest) if (Diff(f, r, by) <= most) return true; return false; }

    // The probe's resting looks: every distinct look over `ms`
    public static List<int[]> Rest(Grabber g, int ms, int by, int most) {
      var looks = new List<int[]>(); var clock = Stopwatch.StartNew();
      while (clock.ElapsedMilliseconds < ms) { var f = g.Grab(); if (!Resting(f, looks, by, most)) looks.Add(f); Thread.Sleep(2); }
      return looks;
    }
    // How long a read of the probe takes, in ms, over n reads
    public static double ReadMs(Grabber g, int n) { var c = Stopwatch.StartNew(); for (int i = 0; i < n; i++) g.Grab(); return c.Elapsed.TotalMilliseconds / n; }

    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KEYBDINPUT ki; [FieldOffset(8)] long pad0; [FieldOffset(16)] long pad1; [FieldOffset(24)] long pad2; [FieldOffset(32)] long pad3; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
    public static int InputSize() { return Marshal.SizeOf(typeof(INPUT)); }
    static void Key(ushort vk) {
      ushort sc = (ushort)MapVirtualKey(vk, 0);
      var down = new INPUT { type = 1, ki = new KEYBDINPUT { wVk = vk, wScan = sc } };
      var up = new INPUT { type = 1, ki = new KEYBDINPUT { wVk = vk, wScan = sc, dwFlags = 2 } };
      SendInput(2, new[] { down, up }, Marshal.SizeOf(typeof(INPUT)));
    }

    public static volatile bool Abort;
    // `n` samples: key-to-screen in ms (-1 when nothing showed within timeoutMs), stopping early on Abort
    public static List<double> Measure(Grabber g, List<int[]> rest, int n, int by, int most, int timeoutMs, int seed) {
      var r = new Random(seed); var res = new List<double>();
      for (int i = 0; i < n && !Abort; i++) {
        Thread.Sleep(80 + r.Next(120));
        long t0 = Stopwatch.GetTimestamp();
        Key(0x58); // X
        double ms = -1;
        while (!Abort) {
          var f = g.Grab();
          double el = (Stopwatch.GetTimestamp() - t0) * 1000.0 / Stopwatch.Frequency;
          if (!Resting(f, rest, by, most)) { ms = el; break; }
          if (el > timeoutMs) break;
        }
        res.Add(ms);
        Key(0x08); // Backspace
        var c = Stopwatch.StartNew();
        while (!Abort && c.ElapsedMilliseconds < timeoutMs && !Resting(g.Grab(), rest, by, most)) Thread.Sleep(1);
      }
      return res;
    }
  }

  // Real input (not injected) seen by low-level hooks on a thread of their own: Lachlan is back
  public static class Watch {
    delegate IntPtr HookProc(int code, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr SetWindowsHookEx(int id, HookProc f, IntPtr mod, uint thread);
    [DllImport("user32.dll")] static extern bool UnhookWindowsHookEx(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int code, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandle(string name);
    [DllImport("user32.dll")] static extern bool GetMessage(out MSG m, IntPtr h, uint a, uint b);
    [DllImport("user32.dll")] static extern bool PostThreadMessage(uint t, uint msg, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public IntPtr w, l; public uint t; public int x, y; }
    static HookProc kb, mouse; static Thread thread; static uint tid; public static string Seen;
    public static void Start() {
      Seen = null; Probe.Abort = false;
      var ready = new ManualResetEvent(false);
      thread = new Thread(() => {
        tid = GetCurrentThreadId();
        kb = (c, w, l) => { if (c >= 0 && (Marshal.ReadInt32(l, 8) & 0x10) == 0) { Seen = "a key"; Probe.Abort = true; } return CallNextHookEx(IntPtr.Zero, c, w, l); };
        mouse = (c, w, l) => { if (c >= 0 && (Marshal.ReadInt32(l, 12) & 0x1) == 0) { Seen = "the mouse"; Probe.Abort = true; } return CallNextHookEx(IntPtr.Zero, c, w, l); };
        IntPtr mod = GetModuleHandle(null);
        IntPtr hk = SetWindowsHookEx(13, kb, mod, 0), hm = SetWindowsHookEx(14, mouse, mod, 0);
        ready.Set();
        MSG m; while (GetMessage(out m, IntPtr.Zero, 0, 0)) { }
        UnhookWindowsHookEx(hk); UnhookWindowsHookEx(hm);
      });
      thread.IsBackground = true; thread.Start(); ready.WaitOne();
    }
    public static void Stop() { if (thread != null) { PostThreadMessage(tid, 0x0012, IntPtr.Zero, IntPtr.Zero); thread.Join(2000); thread = null; } }
  }

  // The foreground given to a window without input of its own: AttachThreadInput to the thread
  // that has it, then SetForegroundWindow
  public static class Focus {
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
    public static long Current() { return GetForegroundWindow().ToInt64(); }
    public static bool Give(long hwnd) {
      var h = new IntPtr(hwnd); var fg = GetForegroundWindow();
      uint me = GetCurrentThreadId(), them = GetWindowThreadProcessId(fg, IntPtr.Zero);
      bool attached = them != me && AttachThreadInput(me, them, true);
      try { BringWindowToTop(h); SetForegroundWindow(h); } finally { if (attached) AttachThreadInput(me, them, false); }
      return GetForegroundWindow() == h;
    }
  }

  public static class Display {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct DEVMODE {
      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName; public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
      public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput; public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName; public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
      public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
    public static int Hz(string device) { var d = new DEVMODE(); d.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE)); return EnumDisplaySettings(device, -1, ref d) ? d.dmDisplayFrequency : 0; }
  }
}
'@
}

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $Out) { $Out = Join-Path $repo "target\bench\key-$(Get-Date -Format 'yyyyMMdd-HHmmss')" }
New-Item -ItemType Directory -Force $Out, $Work | Out-Null
$Out = (Resolve-Path $Out).Path; $Work = (Resolve-Path $Work).Path
$log = Join-Path $Out 'key.log'
function Write-Log([string]$Line) { $l = "$(Get-Date -Format 'HH:mm:ss.fff') $Line"; Add-Content -LiteralPath $log $l; $l | Out-Host }

$bin = Copy-BenchBuild -From $From -To (Join-Path $Work 'lbterm')
$data = Join-Path $Work 'data'; $cwd = Join-Path $Work 'cwd'
New-Item -ItemType Directory -Force $data, $cwd | Out-Null
$T = Start-Test 'bench-key' -Bin $bin -Out (Join-Path $Out 'harness')
$setup = [pscustomobject]@{
  Bin = $bin; Cwd = $cwd; Class = $T.Class; Socket = $T.Sock
  ConfigDir = (Join-Path $T.Out 'config'); Launches = (Join-Path $T.Out 'launches'); Launcher = (Install-BenchLauncher -Data $data)
  Cols = $Cols; Rows = $Rows; Scrollback = 9001; WtProfile = $WtProfile; Data = $data
}
New-Item -ItemType Directory -Force $setup.ConfigDir, $setup.Launches | Out-Null
$script:BenchSetup = $setup
$global:BenchPids = [Collections.Generic.HashSet[uint32]]::new()
# -Run gives the terminals the foreground itself: the watcher mustn't end them for having it
$script:BenchGuard = -not $Run

$screen = [System.Windows.Forms.Screen]::AllScreens | Sort-Object { $_.Bounds.Right } | Select-Object -Last 1
$hz = [LbtermKey.Display]::Hz($screen.DeviceName)
Write-Log "key-latency: $(if ($Run) { 'RUN' } else { 'dry run' }); right-most screen $($screen.DeviceName) $($screen.Bounds.Width)x$($screen.Bounds.Height) at $hz Hz; SendInput's INPUT is $([LbtermKey.Probe]::InputSize()) bytes"

if ($Run) {
  if (Test-WorkstationLocked) { throw 'the workstation is locked: SendInput and screen reads would reach nothing' }
  $idle = [LbtermBench.Native]::IdleMs()
  if ($idle -lt $IdleSec * 1000) { throw "real input $([Math]::Round($idle / 1000, 1)) s ago, under -IdleSec $IdleSec`: not starting" }
}

# The cell grid from the solid row: rows of the client capture holding a run of bright pixels 5
# to 20 cells' worth long; the probe is the cell after the ">" on the next row. Its rectangle on
# the screen, and the cell's size.
function Find-Probe($Term) {
  $h = [IntPtr][long]$Term.Hwnd
  $shot = [LbtermE2E.Win]::Capture($h)
  if (-not $shot) { throw 'no capture of the window' }
  try {
    $px = [LbtermE2E.Px]::Pixels($shot); $w = $shot.Width
    $rowsHit = [Collections.Generic.List[object]]::new()
    for ($y = 0; $y -lt $shot.Height; $y++) {
      $run = 0; $best = 0; $end = 0
      for ($x = 0; $x -lt $w; $x++) {
        if ([LbtermE2E.Px]::Lum($px[$y * $w + $x]) -gt 150) { $run++; if ($run -gt $best) { $best = $run; $end = $x } } else { $run = 0 }
      }
      if ($best -ge 40 -and $best -le 300) { $rowsHit.Add([pscustomobject]@{ Y = $y; X0 = $end - $best + 1; Len = $best }) }
    }
    if (-not $rowsHit.Count) { throw 'no solid row found in the capture' }
    # The first group of consecutive rows
    $group = @($rowsHit[0]); for ($i = 1; $i -lt $rowsHit.Count -and $rowsHit[$i].Y -eq $rowsHit[$i - 1].Y + 1; $i++) { $group += $rowsHit[$i] }
    $len = ($group | ForEach-Object Len | Sort-Object)[[int]($group.Count / 2)]
    $cw = $len / 10.0; $ch = $group.Count; $x0 = ($group | ForEach-Object X0 | Sort-Object)[[int]($group.Count / 2)]; $y0 = $group[0].Y
    $cell = [System.Drawing.Rectangle]::new([int]($x0 + $cw + 1), [int]($y0 + $ch + 1), [int]($cw - 2), [int]($ch - 2))
    $origin = ConvertTo-ScreenPoint $h 0 0
    $marked = Join-Path $Out "key-$($Term.Kind)-probe.png"
    $g = [System.Drawing.Graphics]::FromImage($shot); $g.DrawRectangle([System.Drawing.Pens]::Red, $cell); $g.Dispose(); $shot.Save($marked)
    [pscustomobject]@{ Screen = [System.Drawing.Rectangle]::new($origin.X + $cell.X, $origin.Y + $cell.Y, $cell.Width, $cell.Height); Client = $cell; CellW = $cw; CellH = $ch; Png = $marked }
  } finally { $shot.Dispose() }
}

# Whether the probe on the screen looks as PrintWindow draws it: nothing over the window
function Test-OnScreen($Term, $Probe) {
  $g = [LbtermKey.Grabber]::new($Probe.Screen.X, $Probe.Screen.Y, $Probe.Screen.Width, $Probe.Screen.Height)
  try {
    $onScreen = $g.Grab()
    $shot = [LbtermE2E.Win]::Capture([IntPtr][long]$Term.Hwnd)
    $crop = $shot.Clone($Probe.Client, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb); $shot.Dispose()
    $drawn = [LbtermE2E.Px]::Pixels($crop); $crop.Dispose()
    [LbtermKey.Probe]::Diff($onScreen, $drawn, 40) -le [int]($onScreen.Length / 20)
  } finally { $g.Dispose() }
}

$results = Join-Path $Out 'key-results.jsonl'
$prevForeground = [LbtermKey.Focus]::Current()
Protect-Existing
[LbtermBench.ForegroundWatch]::Start()
$stopped = $null

Invoke-Test {
  if ($Run) { [LbtermKey.Watch]::Start() }
  try {
    for ($round = 1; $round -le $(if ($Run) { $Rounds } else { 1 }); $round++) {
      foreach ($k in $Kinds) {
        if ($stopped) { break }
        if ($k -eq 'wt' -and -not $Run) { Write-Log 'wt: not opened in the dry run (it takes the foreground as it opens); its grid is found in -Run'; continue }
        $ready = Join-Path $Out "ready-$k-$round"
        Remove-Item $ready -ErrorAction SilentlyContinue
        $prog = @($script:BenchPwsh, '-NoProfile', '-File', (Join-Path $PSScriptRoot 'keyprobe.ps1'), '-Ready', $ready)
        $term = $null
        try {
          if (-not $Run) { Wait-Idle -Ms 1500 -TimeoutSec 30 }
          $term = Start-BenchTerminal -Kind $k -Setup $setup -Program $prog
          # In -Run the foreground is ours to give, so only the dry run holds a terminal to it
          if ($term.Violation -and ($Run -and $term.Violation -notlike 'its window is at*' -and $term.Violation -notlike 'no window*')) { $term.Violation = $null }
          if ($term.Violation) { throw $term.Violation }
          if ($term.Error) { throw $term.Error }
          if (-not (Wait-BenchFile $ready 30)) { throw 'the probe never said it was ready' }
          Start-Sleep -Milliseconds 700
          $probe = Find-Probe $term
          Write-Log "$k`: cell $([Math]::Round($probe.CellW, 2))x$($probe.CellH) px, probe at $($probe.Screen.X),$($probe.Screen.Y) ($($probe.Png))"
          if (-not $Run) {
            $seen = Test-OnScreen $term $probe
            $g = [LbtermKey.Grabber]::new($probe.Screen.X, $probe.Screen.Y, $probe.Screen.Width, $probe.Screen.Height)
            try {
              $readMs = [LbtermKey.Probe]::ReadMs($g, 200)
              $rest = [LbtermKey.Probe]::Rest($g, 1500, 60, [int]($probe.Screen.Width * $probe.Screen.Height / 20))
            } finally { $g.Dispose() }
            Write-Log ("{0}: on the screen as drawn: {1}; a read of the probe takes {2:N3} ms; {3} resting look(s) over 1.5 s unfocused" -f $k, $seen, $readMs, $rest.Count)
            Add-Content $results (@{ kind = $k; dry = $true; cellW = $probe.CellW; cellH = $probe.CellH; onScreen = $seen; readMs = $readMs; restingLooks = $rest.Count } | ConvertTo-Json -Compress)
            continue
          }
          if (-not [LbtermKey.Focus]::Give([long]$term.Hwnd)) { throw "couldn't give the window the foreground" }
          Start-Sleep -Milliseconds 300
          $g = [LbtermKey.Grabber]::new($probe.Screen.X, $probe.Screen.Y, $probe.Screen.Width, $probe.Screen.Height)
          try {
            $most = [int]($probe.Screen.Width * $probe.Screen.Height / 20)
            $rest = [LbtermKey.Probe]::Rest($g, 1500, 60, $most)
            $readMs = [LbtermKey.Probe]::ReadMs($g, 100)
            $ms = @([LbtermKey.Probe]::Measure($g, $rest, $Samples, 60, $most, 2000, $round * 100 + [array]::IndexOf($Kinds, $k)))
          } finally { $g.Dispose() }
          if ([LbtermKey.Probe]::Abort) { $stopped = "real input ($([LbtermKey.Watch]::Seen)) during $k round $round" }
          $ok = @($ms | Where-Object { $_ -ge 0 } | Sort-Object)
          $pick = { param($q) if ($ok.Count) { [Math]::Round($ok[[Math]::Min($ok.Count - 1, [int][Math]::Floor($q * $ok.Count))], 1) } }
          Write-Log ("{0} round {1}: {2} samples ({3} timed out), median {4} ms, p90 {5}, p99 {6}, min {7}, max {8}; {9} resting look(s); a read takes {10:N3} ms" -f $k, $round, $ok.Count, ($ms.Count - $ok.Count), (& $pick 0.5), (& $pick 0.9), (& $pick 0.99), (& $pick 0), (& $pick 0.9999), $rest.Count, $readMs)
          Add-Content $results (@{ kind = $k; round = $round; samples = $ms; restingLooks = $rest.Count; readMs = $readMs; hz = $hz; cellW = $probe.CellW; cellH = $probe.CellH } | ConvertTo-Json -Compress -Depth 4)
        } catch {
          $fg = if ($term) { Test-BenchForeground $term }
          Add-Failure "$k round $round`: $(if ($fg -and -not $Run) { $fg } else { $_.Exception.Message })"
          Write-Log "ERROR $k round $round`: $($_.Exception.Message)$(if ($fg -and -not $Run) { " ($fg)" })"
        } finally {
          Stop-BenchTerminal $term $setup
          Start-Sleep -Milliseconds 500
        }
      }
    }
  } finally {
    if ($Run) {
      [LbtermKey.Watch]::Stop()
      [void][LbtermKey.Focus]::Give($prevForeground)
    }
    [LbtermBench.ForegroundWatch]::Stop()
  }
  if ($stopped) { Write-Log "stopped: $stopped" }
} -AllowWindow {
  param($w)
  $global:BenchPids.Contains([uint32]$w.Pid) -or [bool](@($w.Ancestors) | Where-Object { $global:BenchPids.Contains([uint32]$_) }) -or (Test-Protected ([uint32]$w.Pid))
} -Passed "key-latency: $(if ($Run) { 'done' } else { 'dry run passed' }), $Out"
