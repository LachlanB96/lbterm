# LBTerm's end-to-end harness: the runner behind `lbterm.ps1 e2e`, and what every test script in
# lbterm\perf and lbterm\motion shares, in place of the copy each used to carry.
#
#   Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
#   $T = Start-Test 'my-test' -Bin $Bin
#   Invoke-Test {
#     $w = Open-TestWindow -Lua "config.initial_rows = 20" -Program (New-PwshProgram 'Start-Sleep 600')
#     $shot = Wait-Still $w -Name 'rest'
#     if (-not ...) { Add-Failure 'what went wrong' }
#   } -Passed 'my-test: what it showed'
#
# A test window keeps the rules in CLAUDE.md: opened with Start-Unfocused (no console, never the
# foreground, LBTERM_NO_ACTIVATE), on the right-most screen, with a class of its own
# (com.lachlanb.lbterm.perf-<test>), crash restore off, automatic reloads off (New-TestConfig, in a
# config folder of its own), and LBTERM_STATE_FILE=<out>\state.json. Mouse and keys reach it as
# posted messages. Invoke-Test always closes what the test opened, and ends the script with
# result.json in its output folder (name, pass, failures, seconds, captures) and exit 0 or 1.
#
# lbterm\tests\harness.Tests.ps1 tests the parts that need no window: the Covers mapping, the
# result format, the verdicts, geometry and the state file's predicates.
Set-StrictMode -Off
$ErrorActionPreference = 'Stop'

$script:Repo = Split-Path $PSScriptRoot -Parent
$script:Pwsh = 'C:\Program Files\PowerShell\7\pwsh.exe'
# Start-Unfocused, Wait-Window, New-TestConfig, Get-OffscreenPosition and $TestWindowPosition
. (Join-Path $PSScriptRoot 'motion\offscreen.ps1')
# Write-FileWhole and Move-FileWhole: a file another process reads, written through a rename that waits out a reader
. (Join-Path $PSScriptRoot 'write-whole.ps1')

#region Win32, captures and pixels: the one Add-Type

if (-not ('LbtermE2E.Win' -as [type])) {
  Add-Type -AssemblyName System.Drawing
  # .NET 10 (pwsh 7.6) moved Graphics' GDI+ plumbing into these; older runtimes have neither
  $refs = @('System.Drawing.Common', 'System.Drawing.Primitives', 'System.Collections', 'System.Runtime.InteropServices', 'System.Threading', 'System.ComponentModel.Primitives') +
    @('System.Private.Windows.GdiPlus', 'System.Private.Windows.Core' | Where-Object { Test-Path (Join-Path $PSHOME "$_.dll") })
  Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Drawing; using System.Drawing.Imaging;
using System.Runtime.InteropServices; using System.Text; using System.Threading;
namespace LbtermE2E {
  public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code, uint type);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll", SetLastError = true)] public static extern bool RegisterHotKey(IntPtr h, int id, uint mods, uint vk);
    [DllImport("user32.dll")] public static extern bool UnregisterHotKey(IntPtr h, int id);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO i);

    // Milliseconds since the last real key or mouse input
    public static uint IdleMs() {
      var i = new LASTINPUTINFO(); i.cbSize = (uint)Marshal.SizeOf(i); GetLastInputInfo(ref i);
      return (uint)Environment.TickCount - i.dwTime;
    }
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
    // Whether Shift, Ctrl, Alt or a Windows key is held down on the PC now (the real keyboard's):
    // a posted key takes the modifiers its window's thread sees, and that follows the real ones
    public static bool ModifierHeld() {
      foreach (int vk in new[] { 0x10, 0x11, 0x12, 0x5B, 0x5C }) { if ((GetAsyncKeyState(vk) & 0x8000) != 0) return true; }
      return false;
    }
    // The process owning the foreground window
    public static uint ForegroundPid() { uint p; GetWindowThreadProcessId(GetForegroundWindow(), out p); return p; }
    // (x, y) as a mouse message's lParam: two 16-bit fields, a negative one as its two's complement
    public static IntPtr LParam(int x, int y) { return (IntPtr)unchecked((int)(((uint)(y & 0xFFFF) << 16) | (uint)(x & 0xFFFF))); }
    // A wheel message's wParam: the delta in the high word, the buttons held in the low
    public static IntPtr WheelWParam(int delta, int keys) { return (IntPtr)unchecked((int)(((uint)(delta & 0xFFFF) << 16) | (uint)(keys & 0xFFFF))); }
    // A WM_KEYDOWN's or WM_KEYUP's lParam: a repeat of 1, the scan code, the extended bit, and for
    // an up the previous-state and transition bits
    public static IntPtr KeyLParam(uint scan, bool extended, bool up) {
      long l = 1 | ((long)(scan & 0xFF) << 16) | (extended ? 1L << 24 : 0);
      if (up) l |= 0xC0000000L;
      return (IntPtr)unchecked((int)l);
    }
    public static void Post(IntPtr h, uint msg, IntPtr w, IntPtr l) { PostMessage(h, msg, w, l); }
    public static void Mouse(IntPtr h, uint msg, int keys, int x, int y) { PostMessage(h, msg, (IntPtr)keys, LParam(x, y)); }
    public static void Key(IntPtr h, uint vk, bool extended) {
      uint scan = MapVirtualKey(vk, 0);
      PostMessage(h, 0x0100, (IntPtr)vk, KeyLParam(scan, extended, false));
      PostMessage(h, 0x0101, (IntPtr)vk, KeyLParam(scan, extended, true));
    }
    // The process's visible top-level windows of the class
    public static List<IntPtr> Windows(uint pid, string cls) {
      var found = new List<IntPtr>();
      EnumWindows((h, l) => {
        uint p; GetWindowThreadProcessId(h, out p);
        var s = new StringBuilder(256); GetClassName(h, s, 256);
        if (p == pid && IsWindowVisible(h) && s.ToString() == cls) found.Add(h);
        return true;
      }, IntPtr.Zero);
      return found;
    }
    // The client area within the window's own rectangle, or an empty one when it has none
    public static Rectangle ClientInWindow(IntPtr h) {
      RECT wr, cr; POINT o = new POINT();
      if (!GetWindowRect(h, out wr) || !GetClientRect(h, out cr) || !ClientToScreen(h, ref o)) return Rectangle.Empty;
      return new Rectangle(o.X - wr.Left, o.Y - wr.Top, cr.Right, cr.Bottom);
    }
    // The whole window as PrintWindow(PW_RENDERFULLCONTENT) draws it, DirectComposition content
    // and all, whatever covers it; null when there's no window to draw
    public static Bitmap CaptureWindow(IntPtr h) {
      RECT wr;
      if (!GetWindowRect(h, out wr) || wr.Right <= wr.Left || wr.Bottom <= wr.Top) return null;
      var bmp = new Bitmap(wr.Right - wr.Left, wr.Bottom - wr.Top, PixelFormat.Format32bppArgb);
      using (var g = Graphics.FromImage(bmp)) {
        var hdc = g.GetHdc();
        try { PrintWindow(h, hdc, 2); } finally { g.ReleaseHdc(hdc); }
      }
      return bmp;
    }
    // The client area, as the GUI draws it; null when there's no window or no client area yet
    public static Bitmap Capture(IntPtr h) {
      var client = ClientInWindow(h);
      if (client.Width <= 0 || client.Height <= 0) return null;
      using (var bmp = CaptureWindow(h)) {
        if (bmp == null) return null;
        client.Intersect(new Rectangle(0, 0, bmp.Width, bmp.Height));
        if (client.Width <= 0 || client.Height <= 0) return null;
        return bmp.Clone(client, PixelFormat.Format32bppArgb);
      }
    }

    // Lachlan's own pointer crossing the test window can hover what no posted message explains:
    // it's looked for every 50 ms, so a failure can say so
    static Timer watch; static readonly List<string> crossed = new List<string>();
    public static void WatchPointer(IntPtr h) {
      if (watch != null) watch.Dispose();
      watch = new Timer(_ => {
        POINT p; RECT r;
        if (GetCursorPos(out p) && GetWindowRect(h, out r) && p.X >= r.Left && p.X < r.Right && p.Y >= r.Top && p.Y < r.Bottom)
          lock (crossed) crossed.Add(DateTime.Now.ToString("HH:mm:ss.fff"));
      }, null, 0, 50);
    }
    public static string[] Crossed() { lock (crossed) return crossed.ToArray(); }
  }

  // A visible top-level window that appeared while a test ran: its process, the processes above
  // it (parent first), its title and class, and when, with how long Lachlan's hands had been off
  // the keyboard and mouse, and the classes of every window its process showed then (so a ghost
  // card's process can be known by its test window)
  public class SeenWindow {
    public long Hwnd; public uint Pid; public string Exe; public uint[] Ancestors; public string[] AncestorExes;
    public string Title; public string Class; public DateTime At; public uint IdleMs; public int Width; public int Height;
    public string[] ProcessClasses;
  }

  // Who had the foreground, each time it changed while a test ran: its window's process, title
  // and class, when, and how long Lachlan's hands had been off the keyboard and mouse
  public class ForegroundSeen {
    public long Hwnd; public uint Pid; public string Exe; public string Title; public string Class; public DateTime At; public uint IdleMs;
  }

  // Polls the desktop's top-level windows from a timer of its own while a test runs, and keeps each
  // one that becomes visible: not those there when it started, nor cloaked or empty ones (which
  // nobody sees). Which are the test's own is decided afterwards, by process. It notes the
  // foreground each time it changes too, so a test window that took it is known with what had it
  // before, though no input was being posted then.
  public static class WindowWatch {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int value, int size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr snap, ref PROCESSENTRY32W e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr snap, ref PROCESSENTRY32W e);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct PROCESSENTRY32W {
      public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID;
      public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags;
      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }

    static Timer timer;
    static readonly object gate = new object();
    static readonly object polling = new object();
    static readonly HashSet<long> known = new HashSet<long>();
    static readonly List<SeenWindow> found = new List<SeenWindow>();
    static readonly List<ForegroundSeen> foregrounds = new List<ForegroundSeen>();
    static long lastForeground;

    // The foreground, kept when it isn't the window it was at the last look
    static void NoteForeground() {
      IntPtr h = Win.GetForegroundWindow();
      lock (gate) { if (foregrounds.Count > 0 && h.ToInt64() == lastForeground) return; lastForeground = h.ToInt64(); }
      uint pid = 0;
      if (h != IntPtr.Zero) Win.GetWindowThreadProcessId(h, out pid);
      string exe = "?";
      KeyValuePair<uint, string> me;
      if (pid != 0 && Processes().TryGetValue(pid, out me)) exe = me.Value;
      var t = new StringBuilder(256); var c = new StringBuilder(256);
      if (h != IntPtr.Zero) { GetWindowText(h, t, 256); Win.GetClassName(h, c, 256); }
      var f = new ForegroundSeen { Hwnd = h.ToInt64(), Pid = pid, Exe = exe, Title = t.ToString(), Class = c.ToString(), At = DateTime.Now, IdleMs = Win.IdleMs() };
      lock (gate) foregrounds.Add(f);
    }

    // Every foreground noted since Start, the first the one there was then
    public static ForegroundSeen[] Foregrounds() { lock (gate) return foregrounds.ToArray(); }

    static List<IntPtr> Visible() {
      var l = new List<IntPtr>();
      Win.EnumWindows((h, p) => { if (Win.IsWindowVisible(h)) l.Add(h); return true; }, IntPtr.Zero);
      return l;
    }
    // DWMWA_CLOAKED: shown to nobody (a suspended app's frame, another desktop's window)
    static bool Cloaked(IntPtr h) { int v; return DwmGetWindowAttribute(h, 14, out v, 4) == 0 && v != 0; }

    // Every process's parent and file name, now
    static Dictionary<uint, KeyValuePair<uint, string>> Processes() {
      var map = new Dictionary<uint, KeyValuePair<uint, string>>();
      IntPtr snap = CreateToolhelp32Snapshot(2, 0);
      if (snap == IntPtr.Zero || snap == new IntPtr(-1)) return map;
      try {
        var e = new PROCESSENTRY32W(); e.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32W));
        if (Process32FirstW(snap, ref e)) {
          do { map[e.th32ProcessID] = new KeyValuePair<uint, string>(e.th32ParentProcessID, e.szExeFile); } while (Process32NextW(snap, ref e));
        }
      } finally { CloseHandle(snap); }
      return map;
    }

    public static void Start(int intervalMs) {
      lock (gate) {
        if (timer != null) { timer.Dispose(); timer = null; }
        known.Clear(); found.Clear(); foregrounds.Clear();
        foreach (var h in Visible()) known.Add(h.ToInt64());
      }
      NoteForeground();
      lock (gate) timer = new Timer(_ => Poll(), null, intervalMs, intervalMs);
    }

    // Stops, looks once more, and gives what it saw
    public static SeenWindow[] Stop() {
      lock (gate) { if (timer != null) { timer.Dispose(); timer = null; } }
      Poll();
      lock (gate) return found.ToArray();
    }

    public static bool Running() { lock (gate) return timer != null; }

    class Fresh { public IntPtr H; public uint Pid; public string Title; public string Class; public Win.RECT R; }

    static void Poll() {
      if (!Monitor.TryEnter(polling)) return;
      try {
        NoteForeground();
        var fresh = new List<Fresh>();
        foreach (var h in Visible()) {
          lock (gate) { if (known.Contains(h.ToInt64())) continue; }
          // Left unknown, so one uncloaked or given a size later is seen then
          if (Cloaked(h)) continue;
          Win.RECT r;
          if (!Win.GetWindowRect(h, out r) || r.Right - r.Left <= 0 || r.Bottom - r.Top <= 0) continue;
          // What it is, read now: one that flashes up and goes before the next read would name no
          // process, title or class (process 0, the System Process, 0x0)
          uint pid;
          if (Win.GetWindowThreadProcessId(h, out pid) == 0) continue;
          var t = new StringBuilder(256); GetWindowText(h, t, 256);
          var c = new StringBuilder(256); Win.GetClassName(h, c, 256);
          fresh.Add(new Fresh { H = h, Pid = pid, Title = t.ToString(), Class = c.ToString(), R = r });
        }
        if (fresh.Count == 0) return;
        var procs = Processes();
        uint idle = Win.IdleMs();
        var classes = new Dictionary<uint, List<string>>();
        foreach (var v in Visible()) {
          uint vp; Win.GetWindowThreadProcessId(v, out vp);
          var vc = new StringBuilder(256); Win.GetClassName(v, vc, 256);
          List<string> l;
          if (!classes.TryGetValue(vp, out l)) classes[vp] = l = new List<string>();
          if (!l.Contains(vc.ToString())) l.Add(vc.ToString());
        }
        foreach (var f in fresh) {
          IntPtr h = f.H; uint pid = f.Pid;
          List<string> mine;
          var ancestors = new List<uint>(); var names = new List<string>();
          uint at = pid; var seen = new HashSet<uint>();
          KeyValuePair<uint, string> me;
          string exe = procs.TryGetValue(pid, out me) ? me.Value : "?";
          for (int i = 0; i < 16; i++) {
            KeyValuePair<uint, string> p;
            if (!procs.TryGetValue(at, out p) || p.Key == 0 || !seen.Add(p.Key)) break;
            ancestors.Add(p.Key);
            KeyValuePair<uint, string> up;
            names.Add(procs.TryGetValue(p.Key, out up) ? up.Value : "?");
            at = p.Key;
          }
          var w = new SeenWindow {
            Hwnd = h.ToInt64(), Pid = pid, Exe = exe, Ancestors = ancestors.ToArray(), AncestorExes = names.ToArray(),
            Title = f.Title, Class = f.Class, At = DateTime.Now, IdleMs = idle,
            Width = f.R.Right - f.R.Left, Height = f.R.Bottom - f.R.Top,
            ProcessClasses = classes.TryGetValue(pid, out mine) ? mine.ToArray() : new string[0]
          };
          lock (gate) { if (known.Add(h.ToInt64())) found.Add(w); }
        }
      } finally { Monitor.Exit(polling); }
    }
  }

  // Probes over captures. Rectangles are clipped to the shot; brightness is (3r + 6g + b) / 10.
  public static class Px {
    public static int[] Pixels(Bitmap b) {
      var d = b.LockBits(new Rectangle(0, 0, b.Width, b.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
      var p = new int[b.Width * b.Height]; Marshal.Copy(d.Scan0, p, 0, p.Length); b.UnlockBits(d); return p;
    }
    public static int Lum(int c) { return (((c >> 16) & 255) * 3 + ((c >> 8) & 255) * 6 + (c & 255)) / 10; }
    public static int Rgb(int a, int b) {
      return Math.Abs(((a >> 16) & 255) - ((b >> 16) & 255)) + Math.Abs(((a >> 8) & 255) - ((b >> 8) & 255)) + Math.Abs((a & 255) - (b & 255));
    }
    public static Rectangle All(Bitmap b) { return new Rectangle(0, 0, b.Width, b.Height); }
    static Rectangle Clip(Bitmap b, Rectangle r) { r.Intersect(All(b)); return r; }
    static bool Same(Bitmap a, Bitmap b) { return a.Width == b.Width && a.Height == b.Height; }

    // The box round every pixel inside r that differs between two shots: x0, y0, x1, y1
    // (inclusive), or null when none does. Shots of different sizes differ everywhere.
    public static int[] DiffBox(Bitmap a, Bitmap b, Rectangle r) {
      if (!Same(a, b)) return new[] { 0, 0, Math.Max(a.Width, b.Width) - 1, Math.Max(a.Height, b.Height) - 1 };
      r = Clip(a, r); var pa = Pixels(a); var pb = Pixels(b);
      int x0 = int.MaxValue, y0 = int.MaxValue, x1 = -1, y1 = -1;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        if (pa[y * a.Width + x] == pb[y * a.Width + x]) continue;
        x0 = Math.Min(x0, x); y0 = Math.Min(y0, y); x1 = Math.Max(x1, x); y1 = Math.Max(y1, y);
      }
      return x1 < 0 ? null : new[] { x0, y0, x1, y1 };
    }
    // Mean brightness difference over r; 255 for shots of different sizes
    public static double LumDistance(Bitmap a, Bitmap b, Rectangle r) {
      if (!Same(a, b)) return 255;
      r = Clip(a, r); var pa = Pixels(a); var pb = Pixels(b); double sum = 0; int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        int i = y * a.Width + x; sum += Math.Abs(Lum(pa[i]) - Lum(pb[i])); n++;
      }
      return n == 0 ? 0 : sum / n;
    }
    // Mean of the three channels' differences, summed, over r; 765 for shots of different sizes
    public static double RgbDistance(Bitmap a, Bitmap b, Rectangle r) {
      if (!Same(a, b)) return 765;
      r = Clip(a, r); var pa = Pixels(a); var pb = Pixels(b); double sum = 0; int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        int i = y * a.Width + x; sum += Rgb(pa[i], pb[i]); n++;
      }
      return n == 0 ? 0 : sum / n;
    }
    // Mean brightness over r
    public static double Light(Bitmap a, Rectangle r) {
      r = Clip(a, r); var p = Pixels(a); double sum = 0; int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) { sum += Lum(p[y * a.Width + x]); n++; }
      return n == 0 ? 0 : sum / n;
    }
    // Mean of how much brighter b is than a, over r
    public static double Brighter(Bitmap a, Bitmap b, Rectangle r) {
      if (!Same(a, b)) return 0;
      r = Clip(a, r); var pa = Pixels(a); var pb = Pixels(b); double sum = 0; int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        int i = y * a.Width + x; sum += Math.Max(0, Lum(pb[i]) - Lum(pa[i])); n++;
      }
      return n == 0 ? 0 : sum / n;
    }
    // How many pixels of r differ at all
    public static int Differing(Bitmap a, Bitmap b, Rectangle r) {
      if (!Same(a, b)) return r.Width * r.Height;
      r = Clip(a, r); var pa = Pixels(a); var pb = Pixels(b); int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) if (pa[y * a.Width + x] != pb[y * a.Width + x]) n++;
      return n;
    }
    // How many pixels of r differ in brightness by more than `by`
    public static int Changed(Bitmap a, Bitmap b, Rectangle r, int by) {
      if (!Same(a, b)) return r.Width * r.Height;
      r = Clip(a, r); var pa = Pixels(a); var pb = Pixels(b); int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        int i = y * a.Width + x; if (Math.Abs(Lum(pb[i]) - Lum(pa[i])) > by) n++;
      }
      return n;
    }
    // How far from (cx, cy), within `box` of it, the pixels brighter in b than a by more than `by` reach
    public static double Reach(Bitmap a, Bitmap b, int cx, int cy, int box, int by) {
      if (!Same(a, b)) return 0;
      var pa = Pixels(a); var pb = Pixels(b); double far = 0;
      for (int y = Math.Max(0, cy - box); y < Math.Min(a.Height, cy + box); y++)
        for (int x = Math.Max(0, cx - box); x < Math.Min(a.Width, cx + box); x++) {
          int i = y * a.Width + x;
          if (Lum(pb[i]) - Lum(pa[i]) > by) far = Math.Max(far, Math.Sqrt((x - cx) * (x - cx) + (y - cy) * (y - cy)));
        }
      return far;
    }
    // The first row of r with a pixel brighter than `over`: y, and the x span of such pixels on it
    public static int[] FirstBrightRow(Bitmap b, Rectangle r, int over) {
      r = Clip(b, r); var p = Pixels(b);
      for (int y = r.Top; y < r.Bottom; y++) {
        int x0 = -1, x1 = -1;
        for (int x = r.Left; x < r.Right; x++) if (Lum(p[y * b.Width + x]) > over) { if (x0 < 0) x0 = x; x1 = x; }
        if (x0 >= 0) return new[] { y, x0, x1 };
      }
      return null;
    }
    // The box round the pixels of r at least `floor` bright (text), or null
    public static int[] BrightBox(Bitmap b, Rectangle r, int floor) {
      r = Clip(b, r); var p = Pixels(b);
      int x0 = int.MaxValue, y0 = int.MaxValue, x1 = -1, y1 = -1;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        if (Lum(p[y * b.Width + x]) < floor) continue;
        x0 = Math.Min(x0, x); y0 = Math.Min(y0, y); x1 = Math.Max(x1, x); y1 = Math.Max(y1, y);
      }
      return x1 < 0 ? null : new[] { x0, y0, x1, y1 };
    }
    static bool InRange(int c, int[] k) {
      int r = (c >> 16) & 255, g = (c >> 8) & 255, bl = c & 255;
      return r >= k[0] && r <= k[1] && g >= k[2] && g <= k[3] && bl >= k[4] && bl <= k[5];
    }
    // How many pixels of r have each channel within its range: rLo, rHi, gLo, gHi, bLo, bHi (inclusive)
    public static int CountRange(Bitmap b, Rectangle r, int[] k) {
      r = Clip(b, r); var p = Pixels(b); int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) if (InRange(p[y * b.Width + x], k)) n++;
      return n;
    }
    // Every `step`th row of r, every column: the count of pixels in range (a sparse CountRange)
    public static int CountRangeSparse(Bitmap b, Rectangle r, int[] k, int stepX, int stepY) {
      r = Clip(b, r); var p = Pixels(b); int n = 0;
      for (int y = r.Top; y < r.Bottom; y += stepY) for (int x = r.Left; x < r.Right; x += stepX) if (InRange(p[y * b.Width + x], k)) n++;
      return n;
    }
    // The mean row of the pixels of r in range, -1 with none
    public static double MeanYInRange(Bitmap b, Rectangle r, int[] k) {
      r = Clip(b, r); var p = Pixels(b); double sum = 0; int n = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) if (InRange(p[y * b.Width + x], k)) { sum += y; n++; }
      return n == 0 ? -1 : sum / n;
    }
    // The rows where a run of in-range pixels starts, going down column x
    public static int[] RunStartsDown(Bitmap b, int x, int[] k) {
      var p = Pixels(b); var starts = new List<int>(); bool inRun = false;
      if (x < 0 || x >= b.Width) return starts.ToArray();
      for (int y = 0; y < b.Height; y++) {
        bool hit = InRange(p[y * b.Width + x], k);
        if (hit && !inRun) starts.Add(y);
        inRun = hit;
      }
      return starts.ToArray();
    }
    // The rows of r holding a run of at least `min` in-range pixels side by side, top first: a
    // strike or an underline across words, where glyphs alone never run that far
    public static int[] RowsWithRun(Bitmap b, Rectangle r, int[] k, int min) {
      r = Clip(b, r); var p = Pixels(b); var rows = new List<int>();
      for (int y = r.Top; y < r.Bottom; y++) {
        int run = 0, best = 0;
        for (int x = r.Left; x < r.Right; x++) { run = InRange(p[y * b.Width + x], k) ? run + 1 : 0; if (run > best) best = run; }
        if (best >= min) rows.Add(y);
      }
      return rows.ToArray();
    }
    // How many pixels of r are within `tol` of colour rgb in every channel
    public static int CountNear(Bitmap b, Rectangle r, int rgb, int tol) {
      int cr = (rgb >> 16) & 255, cg = (rgb >> 8) & 255, cb = rgb & 255;
      return CountRange(b, r, new[] { cr - tol, cr + tol, cg - tol, cg + tol, cb - tol, cb + tol });
    }
    // Over the pixels of r where the old and new shots differ: the share a frame already shows as
    // the new, and the share it shows as either (a glide shows one or the other at every pixel, a
    // dissolve blends them). Also how many there are.
    public static double[] Glide(Bitmap old, Bitmap now, Bitmap frame, Rectangle r) {
      if (!Same(old, now) || !Same(old, frame)) return new double[] { 0, 0, 0 };
      r = Clip(old, r); var po = Pixels(old); var pn = Pixels(now); var pf = Pixels(frame); int n = 0, isNew = 0, either = 0;
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        int i = y * old.Width + x;
        if (Rgb(po[i], pn[i]) <= 90) continue;
        n++;
        bool asNew = Rgb(pf[i], pn[i]) <= 40, asOld = Rgb(pf[i], po[i]) <= 40;
        if (asNew) isNew++;
        if (asNew || asOld) either++;
      }
      return new double[] { n == 0 ? 0 : (double)isNew / n, n == 0 ? 0 : (double)either / n, n };
    }
    // How much of each of `parts` (shots of the same size, each one picture whole) a frame shows
    // over r, the shares adding up to 1: least squares over every pixel's three channels. DWM lays
    // opaque visuals over each other in the values it stores, so a cross-fade of several pictures
    // is each picture weighted by what shows of it (perf\tab-switch.ps1, tabs in flight). Empty
    // for shots of different sizes, or parts too alike over r to tell apart.
    public static double[] Mix(Bitmap frame, Bitmap[] parts, Rectangle r) {
      int n = parts.Length;
      if (n == 0) return new double[0];
      foreach (var part in parts) if (!Same(frame, part)) return new double[0];
      if (n == 1) return new[] { 1.0 };
      r = Clip(frame, r); var pf = Pixels(frame); var pp = new int[n][];
      for (int k = 0; k < n; k++) pp[k] = Pixels(parts[k]);
      // The last share is 1 less the others: frame - last = sum of share_k (part_k - last)
      int m = n - 1; var a = new double[m, m]; var v = new double[m]; var d = new double[m];
      for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
        int i = y * frame.Width + x;
        for (int c = 0; c < 24; c += 8) {
          double last = (pp[m][i] >> c) & 255, f = ((pf[i] >> c) & 255) - last;
          for (int k = 0; k < m; k++) d[k] = ((pp[k][i] >> c) & 255) - last;
          for (int j = 0; j < m; j++) { v[j] += d[j] * f; for (int k = 0; k < m; k++) a[j, k] += d[j] * d[k]; }
        }
      }
      // Gaussian elimination, the largest pivot first
      for (int col = 0; col < m; col++) {
        int best = col;
        for (int row = col + 1; row < m; row++) if (Math.Abs(a[row, col]) > Math.Abs(a[best, col])) best = row;
        if (Math.Abs(a[best, col]) < 1e-6) return new double[0];
        for (int k = 0; k < m; k++) { var t = a[col, k]; a[col, k] = a[best, k]; a[best, k] = t; }
        { var t = v[col]; v[col] = v[best]; v[best] = t; }
        for (int row = col + 1; row < m; row++) {
          double f = a[row, col] / a[col, col];
          for (int k = col; k < m; k++) a[row, k] -= f * a[col, k];
          v[row] -= f * v[col];
        }
      }
      var s = new double[n]; double sum = 0;
      for (int row = m - 1; row >= 0; row--) {
        double t = v[row];
        for (int k = row + 1; k < m; k++) t -= a[row, k] * s[k];
        s[row] = t / a[row, row]; sum += s[row];
      }
      s[m] = 1 - sum;
      return s;
    }
    // How far down `a` shows what `b` has in r: the shift (within max either way) where they differ least
    public static int BestShift(Bitmap a, Bitmap b, Rectangle r, int max) {
      var pa = Pixels(a); var pb = Pixels(b); int best = 0; double least = double.MaxValue;
      for (int s = -max; s <= max; s++) {
        double sum = 0; int n = 0;
        for (int y = r.Top; y < r.Bottom; y++) {
          int ya = y + s;
          if (ya < 0 || ya >= a.Height || y < 0 || y >= b.Height) continue;
          for (int x = Math.Max(0, r.Left); x < Math.Min(r.Right, Math.Min(a.Width, b.Width)); x++) { sum += Math.Abs(Lum(pa[ya * a.Width + x]) - Lum(pb[y * b.Width + x])); n++; }
        }
        if (n > 0 && sum / n < least) { least = sum / n; best = s; }
      }
      return best;
    }
    // Clusters of pixels brighter than the middle of the top row, in rows [0, rows), from the
    // right, columns within `gap` of each other joined: x0, x1, y0, y1
    public static List<int[]> Clusters(Bitmap b, int rows, int gap) {
      var p = Pixels(b); Func<int, int> raw = c => ((c >> 16) & 255) * 3 + ((c >> 8) & 255) * 6 + (c & 255);
      int bg = raw(p[b.Width / 2]);
      var cols = new List<int[]>();
      for (int x = b.Width - 1; x >= 0; x--) {
        int y0 = -1, y1 = -1;
        for (int y = 0; y < rows && y < b.Height; y++) if (raw(p[y * b.Width + x]) > bg + 600) { if (y0 < 0) y0 = y; y1 = y; }
        if (y0 >= 0) cols.Add(new[] { x, y0, y1 });
      }
      var found = new List<int[]>();
      foreach (var c in cols) {
        var last = found.Count > 0 ? found[found.Count - 1] : null;
        if (last != null && last[0] - c[0] <= gap) { last[0] = c[0]; last[2] = Math.Min(last[2], c[1]); last[3] = Math.Max(last[3], c[2]); }
        else found.Add(new[] { c[0], c[0], c[1], c[2] });
      }
      return found;
    }
    // A fingerprint of a shot every `step` pixels, and how many of those are lit (r + g + b > 150)
    public static long[] Fingerprint(Bitmap b, int step) {
      var p = Pixels(b); long lit = 0, sum = 0;
      for (int y = 0; y < b.Height; y += step) for (int x = 0; x < b.Width; x += step) {
        int c = p[y * b.Width + x], r = (c >> 16) & 255, g = (c >> 8) & 255, bl = c & 255;
        if (r + g + bl > 150) lit++;
        sum = (sum * 31 + r + 3 * g + 7 * bl) % 1000000007;
      }
      return new long[] { lit, sum };
    }

    // `c` shown at `alpha` over `under`, channel by channel in the stored values, as DWM shows a
    // visual at an opacity over the one under it
    public static int Over(int c, double alpha, int under) {
      Func<int, int> ch = s => (int)Math.Round(alpha * ((c >> s) & 255) + (1 - alpha) * ((under >> s) & 255), MidpointRounding.AwayFromZero);
      return unchecked((int)0xFF000000) | (ch(16) << 16) | (ch(8) << 8) | ch(0);
    }
    // Whether every channel of c lies within those of lo and hi, `slack` either way
    static bool Between(int c, int lo, int hi, int slack) {
      for (int s = 0; s <= 16; s += 8) {
        int v = (c >> s) & 255, a = (lo >> s) & 255, b = (hi >> s) & 255;
        if (v < Math.Min(a, b) - slack || v > Math.Max(a, b) + slack) return false;
      }
      return true;
    }
    static int Sum(int c) { return ((c >> 16) & 255) + ((c >> 8) & 255) + (c & 255); }
    static double Linear(int v) { double s = v / 255.0; return s <= 0.04045 ? s / 12.92 : Math.Pow((s + 0.055) / 1.055, 2.4); }
    static double Encoded(double l) { return l <= 0.0031308 ? l * 12.92 : 1.055 * Math.Pow(l, 1 / 2.4) - 0.055; }

    // How far `comp` is, in levels in its worst channel, from what DWM makes of an edge in `line`
    // partly covering `back`, where the GPU drawing without the compositor made `plain` of it. The
    // GPU blends in linear light, so plain's coverage is c = (lin(plain) - lin(back)) /
    // (lin(line) - lin(back)), read in the channel where line and back are furthest apart; DWM
    // lays the visual's encoded, premultiplied edge over the visual under it in stored values,
    // 255 * srgb(c * lin(line)) + (1 - c) * back. Large for anything but such a blend: an edge a
    // pixel wider or narrower in one shot is line in one and back in the other, which no coverage
    // gives. Where line and back are the same there's no edge to blend: how far comp is from plain.
    public static double BlendError(int comp, int plain, int line, int back) {
      int pick = 0; double span = 0;
      for (int s = 0; s <= 16; s += 8) {
        double d = Linear((line >> s) & 255) - Linear((back >> s) & 255);
        if (Math.Abs(d) > Math.Abs(span)) { span = d; pick = s; }
      }
      double err = 0;
      if (Math.Abs(span) < 1e-4) {
        for (int s = 0; s <= 16; s += 8) err = Math.Max(err, Math.Abs(((comp >> s) & 255) - ((plain >> s) & 255)));
        return err;
      }
      double c = Math.Max(0, Math.Min(1, (Linear((plain >> pick) & 255) - Linear((back >> pick) & 255)) / span));
      for (int s = 0; s <= 16; s += 8) {
        double shown = 255 * Encoded(c * Linear((line >> s) & 255)) + (1 - c) * ((back >> s) & 255);
        err = Math.Max(err, Math.Abs(((comp >> s) & 255) - shown));
      }
      return err;
    }

    // What Compare found: how many pixels differ by more than its tolerance (summed over the
    // channels), how many of those are soft (SoftArc of them found by the second pass) and how
    // many are other (SoftOutside of them soft but outside where soft pixels are expected), the
    // most a soft one differs, the furthest a soft one is from DWM's blend (BlendError) and how
    // many soft ones are brighter in the compositor's shot; the box round the others and the first
    // 40 of them (x, y, the compositor's colour, the other's as shown); and a picture: the
    // compositor's shot at half, soft pixels yellow, others red.
    public sealed class Comparison {
      public int Width, Height, Dimmed, Differing, Soft, SoftArc, SoftOutside, Other, MaxSoft, SoftBrighter;
      public double MaxBlendError;
      public int[] OtherBox;
      public List<int[]> OtherAt = new List<int[]>();
      public Bitmap Marked;
    }

    // A shot drawn with the compositor against one drawn without it (motion\compare.ps1). In
    // `plain` the rectangles `dimmed` (the cards of the panes without the keyboard, drawn there
    // undimmed) are first shown as DWM shows them with the compositor: their visual at `alpha`
    // over the window background `under` (Over). A pixel that still differs is soft when it
    // lies between two neighbours, across a row, a column or a diagonal, that both shots agree on,
    // with its colour in each shot within theirs: an edge's partly covered pixel, which DWM blends
    // over the visual under it in the stored (gamma-encoded) values and the GPU without the
    // compositor blends in linear light, so the same coverage comes out a few levels apart. A
    // second pass takes one between two neighbours that agree or are soft, its colour in both
    // shots within theirs in both: a rounded corner's arc, where every pixel is partly covered.
    // Either way the compositor's pixel must be what DWM's blend makes of the plain one's coverage,
    // within `blendSlack` levels a channel (BlendError), with the edge's colour and what's under it
    // the first pixels both shots agree on that way, one step on or up to three: being between
    // its neighbours alone let an edge a pixel wider or narrower through (the review of
    // 2026-10-30: a line a pixel wider was 30 soft pixels, 546 levels summed). Soft pixels count
    // only inside `softIn` (where an edge of one visual lies over another), when it names any.
    // Anything else is other, something drawn differently: a part missing, moved, recoloured, or
    // an edge in another place.
    public static Comparison Compare(Bitmap comp, Bitmap plain, Rectangle[] dimmed, double alpha, int under, int tol, Rectangle[] softIn) {
      return Compare(comp, plain, dimmed, alpha, under, tol, softIn, 4);
    }

    public static Comparison Compare(Bitmap comp, Bitmap plain, Rectangle[] dimmed, double alpha, int under, int tol, Rectangle[] softIn, double blendSlack) {
      if (!Same(comp, plain)) throw new ArgumentException(string.Format("the shots differ in size: {0}x{1} and {2}x{3}", comp.Width, comp.Height, plain.Width, plain.Height));
      int w = comp.Width, h = comp.Height; var pa = Pixels(comp); var pb = Pixels(plain);
      var res = new Comparison { Width = w, Height = h };
      var dim = new bool[w * h];
      foreach (var d in dimmed ?? new Rectangle[0]) {
        var r = Clip(comp, d);
        for (int y = r.Top; y < r.Bottom; y++) for (int x = r.Left; x < r.Right; x++) {
          int i = y * w + x; if (dim[i]) continue;
          dim[i] = true; pb[i] = Over(pb[i], alpha, under); res.Dimmed++;
        }
      }
      // 0 alike, 1 differing, 2 soft
      var kind = new byte[w * h];
      for (int i = 0; i < w * h; i++) if (Rgb(pa[i], pb[i]) > tol) { kind[i] = 1; res.Differing++; }
      int[][] dirs = { new[] { 1, 0 }, new[] { 0, 1 }, new[] { 1, 1 }, new[] { 1, -1 } };
      // The first pixel both shots agree on from (x, y), a step and up to three along (dx, dy): the
      // edge's colour or what's under it, for its blend; -1 when there's none
      Func<int, int, int, int, int> agreed = (x, y, dx, dy) => {
        for (int n = 1; n <= 3; n++) {
          int xx = x + n * dx, yy = y + n * dy;
          if (xx < 0 || yy < 0 || xx >= w || yy >= h) return -1;
          if (kind[yy * w + xx] == 0) return yy * w + xx;
        }
        return -1;
      };
      var blend = new double[w * h];
      // across the pixel at (x, y): its two neighbours one way, each as `ok` says, its colour in
      // both shots within the neighbours' (in plain's, and with `both` in comp's too), and the
      // compositor's what DWM blends of the plain one's coverage of the edge that way
      Func<int, int, Func<int, bool>, bool, bool> across = (x, y, ok, both) => {
        int i = y * w + x;
        foreach (var dir in dirs) {
          int x1 = x - dir[0], y1 = y - dir[1], x2 = x + dir[0], y2 = y + dir[1];
          if (x1 < 0 || x2 < 0 || x1 >= w || x2 >= w || y1 < 0 || y2 < 0 || y1 >= h || y2 >= h) continue;
          int j = y1 * w + x1, k = y2 * w + x2;
          if (!ok(j) || !ok(k)) continue;
          bool inPlain = Between(pa[i], pb[j], pb[k], 2) && Between(pb[i], pb[j], pb[k], 2);
          bool inComp = Between(pa[i], pa[j], pa[k], 2) && Between(pb[i], pa[j], pa[k], 2);
          if (!inPlain || (both && !inComp)) continue;
          int lj = agreed(x, y, -dir[0], -dir[1]), lk = agreed(x, y, dir[0], dir[1]);
          if (lj < 0 || lk < 0) continue;
          // Which side is the edge and which what's under it isn't known: the nearer of the two
          double e = Math.Min(BlendError(pa[i], pb[i], pb[lj], pb[lk]), BlendError(pa[i], pb[i], pb[lk], pb[lj]));
          if (e <= blendSlack) { blend[i] = e; return true; }
        }
        return false;
      };
      var firstPass = new List<int>();
      for (int y = 0; y < h; y++) for (int x = 0; x < w; x++)
        if (kind[y * w + x] == 1 && across(x, y, j => kind[j] == 0, false)) firstPass.Add(y * w + x);
      foreach (var i in firstPass) kind[i] = 2;
      var secondPass = new List<int>();
      for (int y = 0; y < h; y++) for (int x = 0; x < w; x++)
        if (kind[y * w + x] == 1 && across(x, y, j => kind[j] != 1, true)) secondPass.Add(y * w + x);
      foreach (var i in secondPass) kind[i] = 2;
      res.SoftArc = secondPass.Count;
      if (softIn != null && softIn.Length > 0)
        for (int y = 0; y < h; y++) for (int x = 0; x < w; x++) {
          int i = y * w + x;
          if (kind[i] != 2) continue;
          bool inside = false;
          foreach (var r in softIn) if (r.Contains(x, y)) { inside = true; break; }
          if (!inside) { kind[i] = 1; res.SoftOutside++; }
        }
      int x0 = int.MaxValue, y0 = int.MaxValue, xn = -1, yn = -1;
      var marked = new int[w * h];
      for (int y = 0; y < h; y++) for (int x = 0; x < w; x++) {
        int i = y * w + x, c = pa[i];
        marked[i] = unchecked((int)0xFF000000) | ((((c >> 16) & 255) / 2) << 16) | ((((c >> 8) & 255) / 2) << 8) | ((c & 255) / 2);
        if (kind[i] == 2) {
          res.Soft++; res.MaxSoft = Math.Max(res.MaxSoft, Rgb(pa[i], pb[i])); res.MaxBlendError = Math.Max(res.MaxBlendError, blend[i]);
          if (Sum(pa[i]) > Sum(pb[i])) res.SoftBrighter++;
          marked[i] = unchecked((int)0xFFFFC800);
        } else if (kind[i] == 1) {
          res.Other++;
          x0 = Math.Min(x0, x); y0 = Math.Min(y0, y); xn = Math.Max(xn, x); yn = Math.Max(yn, y);
          if (res.OtherAt.Count < 40) res.OtherAt.Add(new[] { x, y, pa[i] & 0xFFFFFF, pb[i] & 0xFFFFFF });
          marked[i] = unchecked((int)0xFFFF0000);
        }
      }
      if (xn >= 0) res.OtherBox = new[] { x0, y0, xn, yn };
      var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb);
      var bd = bmp.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
      Marshal.Copy(marked, 0, bd.Scan0, marked.Length); bmp.UnlockBits(bd);
      res.Marked = bmp;
      return res;
    }
  }
}
'@ -ReferencedAssemblies $refs
}
# Real pixels, not the scaled ones a DPI-unaware process is shown
[void][LbtermE2E.Win]::SetProcessDPIAware()
# lbterm.exe prints UTF-8: read it as that, whatever code page the console started in
try { [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false) } catch { }

#endregion

#region Pure helpers: geometry, command lines, programs

# A length in 96-DPI pixels at a window's scale (GetDpiForWindow / 96), rounded
function ConvertTo-Px([double]$Value, [double]$Scale = 1) { [int][Math]::Round($Value * $Scale) }

function New-Rect([int]$X, [int]$Y, [int]$Width, [int]$Height) { [System.Drawing.Rectangle]::new($X, $Y, $Width, $Height) }

# Columns x0..x1 (inclusive) the whole height of a shot, or rows y0..y1 its whole width
function Get-Columns([int]$X0, [int]$X1, [Parameter(Mandatory)]$Of) { [System.Drawing.Rectangle]::new($X0, 0, $X1 - $X0 + 1, $Of.Height) }
function Get-Rows([int]$Y0, [int]$Y1, [Parameter(Mandatory)]$Of) { [System.Drawing.Rectangle]::new(0, $Y0, $Of.Width, $Y1 - $Y0 + 1) }

# A box (x0, y0, x1, y1) as words, for messages
function Format-Box($Box) { if ($Box) { "x $($Box[0])..$($Box[2]), y $($Box[1])..$($Box[3])" } else { 'nothing' } }

# Numbers to two places, for traces in messages
function Format-Trace($Values, [string]$Format = '{0:N2}') { ($Values | ForEach-Object { $Format -f $_ }) -join ' ' }

# One argument as CommandLineToArgvW reads it back: quoted when it has a space, a tab or a quote,
# backslashes before a quote doubled
function ConvertTo-CommandLineArg([AllowEmptyString()][string]$Arg) {
  if ($Arg -and $Arg -notmatch '[\s"]') { return $Arg }
  $sb = [Text.StringBuilder]::new('"')
  $slashes = 0
  foreach ($c in $Arg.ToCharArray()) {
    if ($c -eq '\') { $slashes++; continue }
    if ($c -eq '"') { [void]$sb.Append('\' * (2 * $slashes + 1)); [void]$sb.Append('"') }
    else { [void]$sb.Append('\' * $slashes); [void]$sb.Append($c) }
    $slashes = 0
  }
  [void]$sb.Append('\' * (2 * $slashes)); [void]$sb.Append('"')
  $sb.ToString()
}

# A Lua string literal's path: forward slashes
function ConvertTo-LuaPath([string]$Path) { $Path -replace '\\', '/' }

# A script as pwsh's -EncodedCommand takes it (UTF-16, base64)
function ConvertTo-EncodedCommand([Parameter(Mandatory)][string]$Script) {
  [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Script))
}

# What a tab runs to run $Script: pwsh 7 by its full path, no profile, the script encoded; without
# one, a shell. -File runs a script file instead, with -ArgumentList after it.
function New-PwshProgram([string]$Script, [string]$File, [object[]]$ArgumentList = @()) {
  $pwsh = @('C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile')
  if ($File) { return $pwsh + @('-File', $File) + @($ArgumentList | ForEach-Object { [string]$_ }) }
  if (-not $Script) { return $pwsh }
  $pwsh + @('-EncodedCommand', (ConvertTo-EncodedCommand $Script))
}

# OSC 1337 SetUserVar, as claudehook.exe sends claude_session and claude_status
function Get-UserVarSequence([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$Value) {
  "$([char]27)]1337;SetUserVar=$Name=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Value)))$([char]7)"
}

# A tab that says it holds Claude session $Sid (the claude_session user var), once $WaitFor
# exists when given, then draws nothing and waits
function New-ClaudeSessionProgram([Parameter(Mandatory)][string]$Sid, [string]$WaitFor, [int]$Seconds = 600) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Sid))
  $wait = if ($WaitFor) { "while (-not (Test-Path '$WaitFor')) { Start-Sleep -Milliseconds 100 }; " } else { '' }
  New-PwshProgram "$wait[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7); Start-Sleep $Seconds"
}

# What each key name is as a virtual-key code, and which are extended keys (the navigation block)
$script:Vk = @{
  Back = 0x08; Tab = 0x09; Enter = 0x0D; Return = 0x0D; Shift = 0x10; Control = 0x11; Alt = 0x12; Escape = 0x1B; Esc = 0x1B
  Space = 0x20; PageUp = 0x21; PageDown = 0x22; End = 0x23; Home = 0x24; Left = 0x25; Up = 0x26; Right = 0x27; Down = 0x28
  Insert = 0x2D; Delete = 0x2E
  F1 = 0x70; F2 = 0x71; F3 = 0x72; F4 = 0x73; F5 = 0x74; F6 = 0x75; F7 = 0x76; F8 = 0x77; F9 = 0x78; F10 = 0x79; F11 = 0x7A; F12 = 0x7B
  F24 = 0x87
}
$script:ExtendedVk = @(0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E)

# A key's virtual-key code from its name ('Enter', 'F5', 'Y') or its code
function ConvertTo-Vk($Key) {
  if ($Key -is [int] -or $Key -is [uint32] -or $Key -is [long]) { return [uint32]$Key }
  $s = [string]$Key
  if ($script:Vk.ContainsKey($s)) { return [uint32]$script:Vk[$s] }
  if ($s.Length -eq 1 -and $s -match '[A-Za-z0-9]') { return [uint32][char]$s.ToUpperInvariant() }
  throw "no virtual-key code for '$Key'"
}

#endregion

#region The test: Start-Test, failures, Invoke-Test and its result

$script:Test = $null

# The test this script runs: its output folder (LBTERM_E2E_OUT under the runner, else
# target\perf\<name>, emptied first), the build, the class its windows get, and what it records
# A mux domain's socket for a test whose output folder is $Out: short, since Windows' AF_UNIX
# takes 108 characters and a build slot's output folder alone runs to 90, and the folder's own,
# so two checkouts running the same test don't share one
function Get-TestSocket([Parameter(Mandatory)][string]$Out) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($Out.ToLowerInvariant())
  $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).Substring(0, 8).ToLowerInvariant()
  Join-Path $HOME ".local\share\lbterm\e2e\$hash\sock"
}

function Start-Test {
  param(
    [Parameter(Mandatory)][string]$Name,
    [string]$Bin,
    # Another output folder (the motion scripts keep target\motion\<name>)
    [string]$Out,
    # The class its windows get unless they're given one, else com.lachlanb.lbterm.perf-<name>
    [string]$Class
  )
  if (-not $Bin) { $Bin = Join-Path $script:Repo 'target\release' }
  if (-not (Test-Path (Join-Path $Bin 'lbterm-gui.exe'))) { throw "no build in $Bin (lbterm-gui.exe): run lbterm.ps1 build first, or pass -Bin" }
  $Bin = (Resolve-Path $Bin).Path
  if (-not $Out) { $Out = if ($env:LBTERM_E2E_OUT) { $env:LBTERM_E2E_OUT } else { Join-Path $script:Repo "target\perf\$Name" } }
  New-Item -ItemType Directory -Force $Out | Out-Null
  $Out = (Resolve-Path $Out).Path
  # What an earlier run left, its windows first (they hold the files open); never the runner's log
  Stop-E2eLeftovers $Out | Out-Null
  Get-ChildItem $Out -Force | Where-Object Name -ne 'output.txt' | ForEach-Object {
    try { Remove-Item $_.FullName -Recurse -Force -ErrorAction Stop } catch { Write-Warning "couldn't clear $($_.FullName): $($_.Exception.Message)" }
  }
  # Its mux domain's socket (Get-TestSocket), and nothing an earlier run left of it
  $sock = Get-TestSocket $Out
  if (Test-Path -LiteralPath (Split-Path $sock)) { Remove-Item -LiteralPath (Split-Path $sock) -Recurse -Force }
  New-Item -ItemType Directory -Force (Split-Path $sock) | Out-Null
  $script:Test = [pscustomobject]@{
    Name       = $Name
    Repo       = $script:Repo
    Bin        = $Bin
    Gui        = Join-Path $Bin 'lbterm-gui.exe'
    Cli        = Join-Path $Bin 'lbterm.exe'
    MuxServer  = Join-Path $Bin 'lbterm-mux-server.exe'
    Out        = $Out
    # Where a script's mux domain listens (socket_path, Open-TestWindow -Socket)
    Sock       = $sock
    Class      = $(if ($Class) { $Class } else { "com.lachlanb.lbterm.perf-$Name" })
    Clock      = [Diagnostics.Stopwatch]::StartNew()
    Failures   = [Collections.Generic.List[string]]::new()
    Notes      = [Collections.Generic.List[string]]::new()
    Captures   = [Collections.Generic.List[string]]::new()
    Windows    = [Collections.Generic.List[object]]::new()
    Classes    = [Collections.Generic.HashSet[string]]::new()
    Foreground = [Collections.Generic.HashSet[uint32]]::new()
    Stolen     = $null
    ForegroundLog = @()
    WindowCheck = $false
    NewWindows = @()
    AllowWindow = $null
    Current    = $null
    LastState  = $null
    Hold       = $null
    # What the pass line says, when the body knows better than -Passed what it checked (a part it
    # couldn't check, said in Notes)
    PassedLine = $null
    # Why the test could judge none of what it's for, when it couldn't (Set-Unjudged)
    Unjudged   = $null
    Done       = $false
  }
  $script:Test
}

function Get-Test { if (-not $script:Test) { throw 'no test started: Start-Test first' }; $script:Test }

# A failure, kept for the end (the test goes on)
function Add-Failure([Parameter(Mandatory)][string]$Message) { (Get-Test).Failures.Add($Message) }

# The test could judge none of what it's for, and why (Lachlan's E5 of 2026-11-09): its window
# covered the whole time, Windows giving it nothing to read. Not a failure, and not a pass on
# nothing either: result.json says `unjudged`, and lbterm.ps1 e2e counts it apart from the passes
# (perf/present.ps1 passed only while covered until 2026-11-08: PITFALLS 283). A failure still
# fails the test; the first reason given is kept
function Set-Unjudged([Parameter(Mandatory)][string]$Why) {
  $t = Get-Test
  if (-not $t.Unjudged) { $t.Unjudged = $Why }
}

# A file in the output folder the result names as a capture
function Add-Capture([Parameter(Mandatory)][string]$Path) {
  $t = Get-Test
  $rel = if ($Path.StartsWith($t.Out, [StringComparison]::OrdinalIgnoreCase)) { $Path.Substring($t.Out.Length).TrimStart('\') } else { $Path }
  if (-not $t.Captures.Contains($rel)) { $t.Captures.Add($rel) }
}

# result.json's shape. notes (what didn't fail the test but is worth seeing: another test's
# window) only when there are some.
function ConvertTo-TestResult {
  param([Parameter(Mandatory)][string]$Name, [string[]]$Failures = @(), [double]$Seconds, [string[]]$Captures = @(), [string[]]$Notes = @(), [string]$Unjudged)
  $f = @($Failures | Where-Object { $_ })
  $r = [ordered]@{
    name     = $Name
    pass     = $f.Count -eq 0
    failures = $f
    seconds  = [Math]::Round($Seconds, 1)
    captures = @($Captures | Where-Object { $_ })
  }
  $n = @($Notes | Where-Object { $_ })
  if ($n) { $r.notes = $n }
  if ($Unjudged -and $f.Count -eq 0) { $r.unjudged = $Unjudged }
  $r
}

function Write-TestResult([Parameter(Mandatory)]$Result, [Parameter(Mandatory)][string]$Path) {
  [IO.File]::WriteAllText($Path, ($Result | ConvertTo-Json -Depth 4))
}

# Whether a test window ever had the foreground: the processes seen owning it, as input was posted
# and by the desktop's watch, against the test's own
function Test-ForegroundTaken {
  $t = Get-Test
  $ours = @($t.Windows | ForEach-Object { [uint32]$_.Pid })
  @(@($t.Foreground) + @($t.ForegroundLog | ForEach-Object { [uint32]$_.Pid }) | Where-Object { $_ -in $ours } | Select-Object -Unique)
}

# A test window having had the foreground, as a failure: whose, first when, how long after
# Lachlan's last real key or mouse input, what had the foreground just before and what had it
# next. -Log is the desktop's watch (LbtermE2E.WindowWatch's Foregrounds, the first the one there
# was as the test began), which sees it between posts too; without an entry of the test's there,
# -Stolen is the look as input was posted. A click of his on the test window reads as his input
# just before; a window of his closing hands the foreground on with no input of his near
function Format-ForegroundTaken {
  param([uint32[]]$Pids = @(), [object[]]$Log = @(), [uint32[]]$Ours = @(), [string]$Stolen)
  $log = @($Log | Where-Object { $_ })
  $at = { param($e) if ($e.At -is [datetime]) { $e.At.ToString('HH:mm:ss.fff') } else { "$($e.At)" } }
  $name = { param($e) "$($e.Exe) (pid $($e.Pid)), '$($e.Title)' [$($e.Class)]" }
  $m = "a test window took the foreground (pid $($Pids -join ', '))"
  $i = -1
  for ($k = 0; $k -lt $log.Count; $k++) { if ([uint32]$log[$k].Pid -in $Ours) { $i = $k; break } }
  if ($i -lt 0) { if ($Stolen) { $m += ", first $Stolen" }; return $m }
  $e = $log[$i]
  $m += ", first at $(& $at $e), $($e.IdleMs) ms after the last real key or mouse input"
  $m += if ($i -gt 0) { ", from $(& $name $log[$i - 1])" } else { ', which had it as the test began' }
  $next = @($log | Select-Object -Skip ($i + 1) | Where-Object { [uint32]$_.Pid -notin $Ours })[0]
  $m += if ($next) { "; it went to $(& $name $next) at $(& $at $next)" } else { "; a test window still had it at the test's end" }
  $m
}

# Who has the foreground now, noted: a test window must never be it. The first time one is, when,
# and how long after Lachlan's last real key or mouse input (a click of his on a test window
# gives it the foreground too)
function Watch-Foreground {
  if (-not $script:Test) { return }
  $fg = [LbtermE2E.Win]::ForegroundPid()
  [void]$script:Test.Foreground.Add($fg)
  if (-not $script:Test.Stolen -and $fg -in @($script:Test.Windows | ForEach-Object { [uint32]$_.Pid })) {
    $script:Test.Stolen = "at $(Get-Date -Format 'HH:mm:ss.fff'), $([LbtermE2E.Win]::IdleMs()) ms after the last real key or mouse input"
  }
}

# Closes what the test opened: its windows (and any window or mux server whose command line names
# its output folder), and the crash-restore notes of its classes. -Keep leaves the windows open.
function Close-Test([switch]$Keep) {
  $t = Get-Test
  Watch-Foreground
  if ($Keep) { return }
  foreach ($w in $t.Windows) { Stop-Process -Id $w.Pid -Force -ErrorAction SilentlyContinue }
  foreach ($w in $t.Windows) { try { (Get-Process -Id $w.Pid -ErrorAction Stop).WaitForExit(5000) | Out-Null } catch { } }
  # Its mux servers whatever build they are: one of the installed build's (spawn-no-activate's
  # -OldServer) is one of the test's when its command line names the test's folder
  Stop-TestServers
  Stop-E2eLeftovers $t.Out | Out-Null
  foreach ($c in $t.Classes) { Remove-Item (Join-Path $env:APPDATA "lbterm\session-$c.json") -ErrorAction SilentlyContinue }
  Remove-Item -LiteralPath (Split-Path $t.Sock) -Recurse -Force -ErrorAction SilentlyContinue
}

# The end of a test: result.json, the failures (FAIL lines) or -Passed, and exit 1 or 0
function Complete-Test([string]$Passed) {
  $t = Get-Test
  $stole = Test-ForegroundTaken
  if ($t.WindowCheck) {
    $ours = @($t.Windows | ForEach-Object { [uint32]$_.Pid })
    $foreign = Get-ForeignWindows -Seen $t.NewWindows -Allowed $ours -Allow $t.AllowWindow
    $others = Get-OtherTestWindows -Seen $t.NewWindows -Allowed $ours -Allow $t.AllowWindow
    foreach ($fw in $foreign) { $t.Failures.Add((Format-ForeignWindow $fw)) }
    foreach ($ow in $others) { $n = Format-ForeignWindow $ow -Note; $t.Notes.Add($n); "NOTE: $n" }
    "window check: $(@($t.NewWindows).Count) new top-level windows, $($foreign.Count) not the test's own, $($others.Count) other test windows (noted)"
  }
  if ($stole) { $t.Failures.Add((Format-ForegroundTaken -Pids $stole -Log $t.ForegroundLog -Ours @($t.Windows | ForEach-Object { [uint32]$_.Pid }) -Stolen $t.Stolen)) }
  $crossed = [LbtermE2E.Win]::Crossed()
  if ($t.Failures.Count -and (Test-WorkstationLocked)) {
    "NOTE: the workstation is locked (LogonUI.exe): captures leave DirectComposition's content out, so a failure that rests on one says nothing; rerun unlocked"
  }
  if ($t.Failures.Count -and $crossed.Count) {
    "NOTE: the real pointer was over a test window between $($crossed[0]) and $($crossed[-1]): its hover can explain a failure, so rerun with the pointer elsewhere"
  }
  $result = ConvertTo-TestResult -Name $t.Name -Failures $t.Failures -Seconds $t.Clock.Elapsed.TotalSeconds -Captures $t.Captures -Notes $t.Notes -Unjudged $t.Unjudged
  Write-TestResult $result (Join-Path $t.Out 'result.json')
  $t.Done = $true
  if ($t.Captures.Count) { "captures in $($t.Out)" }
  if (-not $result.pass) { $t.Failures | ForEach-Object { "FAIL $_" }; exit 1 }
  if ($result.unjudged) { "UNJUDGED $($result.unjudged)"; exit 0 }
  if ($t.PassedLine) { $t.PassedLine } elseif ($Passed) { $Passed }
  exit 0
}

# The test's body, then always Close-Test and Complete-Test: a throw in it is a failure, with
# the line it came from
function Invoke-Test {
  # -Finally runs after the body however it ended, before the windows close (to write a log).
  # -AllowWindow (param($w): a seen window) lets through one that isn't the test's own on purpose (a
  # toast); -NoWindowCheck, or LBTERM_E2E_WINDOW_CHECK=off, leaves the desktop unwatched
  param([Parameter(Mandatory, Position = 0)][scriptblock]$Body, [string]$Passed, [switch]$Keep, [scriptblock]$Finally, [scriptblock]$AllowWindow, [switch]$NoWindowCheck)
  $t = Get-Test
  $t.AllowWindow = $AllowWindow
  $t.WindowCheck = -not $NoWindowCheck -and $env:LBTERM_E2E_WINDOW_CHECK -ne 'off'
  if ($t.WindowCheck) { [LbtermE2E.WindowWatch]::Start(100) }
  # Lachlan's daily mux socket, which no test may touch: Get-SocketStamp
  $dailySocket = Join-Path $HOME '.local\share\lbterm\sock'
  $dailyBefore = Get-SocketStamp $dailySocket
  try { & $Body }
  catch {
    # The test script's own line, not the library's, when the throw came from in here
    $frame = @($_.ScriptStackTrace -split "`n" | Where-Object { $_ -match ', (.+\.ps1): line (\d+)' -and $_ -notmatch 'harness\.psm1' }) | Select-Object -First 1
    $at = if ($frame -and $frame -match ', (.+\.ps1): line (\d+)') { " ($([IO.Path]::GetFileName($Matches[1])):$($Matches[2]))" }
    elseif ($_.InvocationInfo.ScriptName) { " ($([IO.Path]::GetFileName($_.InvocationInfo.ScriptName)):$($_.InvocationInfo.ScriptLineNumber))" } else { '' }
    $t.Failures.Add("$($_.Exception.Message)$at")
  }
  finally {
    if ($t.WindowCheck) { $t.NewWindows = @([LbtermE2E.WindowWatch]::Stop()); $t.ForegroundLog = @([LbtermE2E.WindowWatch]::Foregrounds()) }
    # A memory guard that stopped the test fails it, whether the body saw it or not
    $guard = Stop-MemoryGuard
    if ($guard -and -not @($t.Failures | Where-Object { $_ -like "*$guard*" }).Count) { $t.Failures.Add("the memory guard stopped the test: $guard") }
    if ($Finally) { try { & $Finally } catch { $t.Failures.Add("-Finally: $($_.Exception.Message)") } }
    Close-Test -Keep:$Keep
  }
  if ((Get-SocketStamp $dailySocket) -ne $dailyBefore) {
    $t.Failures.Add("Lachlan's daily mux socket ($dailySocket) was made again while this test ran: a mux server it started took the daily server's path, which cuts every new client off from his tabs until the daily server takes it back (it does within seconds since fix/mux-socket-guard) or restarts. Find the test server that bound it: its config fell back to the default domain")
  }
  Complete-Test -Passed $Passed
}

# Which file is at a socket's path, as a string to compare: when it was made and last written,
# or $null when there's none. A server that takes a path removes the file and binds a new one, and
# the new one's last write is when it did (its creation time can be the old file's: NTFS gives a
# file made again under a name within 15 s the old one's)
function Get-SocketStamp([Parameter(Mandatory)][string]$Path) {
  $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
  if ($item) { '{0}|{1}' -f $item.CreationTimeUtc.Ticks, $item.LastWriteTimeUtc.Ticks }
}

#endregion

#region Windows that aren't the test's

# Invoke-Test watches the desktop while the test runs (LbtermE2E.WindowWatch: every top-level
# window that becomes visible, polled every 100 ms) and sorts each new one (Get-WindowKind):
#   own      an LBTerm window of a process the test opened, or of one those started: a torn-off
#            tab's window, a tab's ghost card, a restart's new GUI
#   noise    a tooltip, a menu, the input method's: they come and go by themselves
#   allowed  one the test's -AllowWindow lets through (a toast it makes on purpose)
#   test     another LBTerm test window, another test's or another session's running beside this
#            one: a test window's class, or a window of a process that shows one. A note in the
#            result, never a failure.
#   foreign  everything else, and the test fails for each: a console or Windows Terminal window,
#            even one a test process opened (the mux server's console came up in front of Lachlan
#            that way until b5330e78d), any other app's window, an LBTerm window that isn't a test's
$script:NoiseWindowClasses = @(
  'tooltips_class32'          # a tooltip
  '#32768'                    # a menu
  'SysShadow'                 # a menu's or tooltip's shadow
  # Firefox's popups (its menus, panels, tooltips and address-bar list) are windows of this class,
  # untitled, coming and going under Lachlan's own pointer: two failed menu-keys on 2026-11-09,
  # 288x76 and 40x40, 0 and 109 ms after his last input (his E4). Its own windows are
  # MozillaWindowClass, which still count
  'MozillaDropShadowWindowClass'
  'IME', 'MSCTFIME UI'        # the input method's
  'Xaml_WindowedPopupClass'   # a XAML app's flyout
  'MozillaDropShadowWindowClass' # Firefox's popups: a menu, a tooltip, a link's address, a dropdown; its
                              # windows are MozillaWindowClass (feat/hex-swatch's smoke runs, 2026-11-09,
                              # failed on 14 of them, 22 to 210 px tall, as Lachlan browsed)
  'DFTitleBarWindow:*'        # DisplayFusion's title-bar buttons, put on every new window (a test's too)
  # ITMan's always-on-top pin (bun.exe, 'ITMan keep on top', 46x40 or 46x22): ITMan draws it as a
  # fourth caption button just left of the minimise button of the window in front (and of any kept
  # on top, and of a title bar under the mouse), placed from DWM's caption button bounds, so by
  # design it moves and shows again at every change of foreground window, 0 to 80 ms after
  # Lachlan's own input (a dozen times in 20 minutes on 2026-11-08, failing every test window
  # running then). It's ITMan's feature, not a stray: LBTerm's own title bar keeps 46 px clear for
  # it (config::lbterm::CAPTION_PIN_GAP, main's 4d9c7f9ac, from ITMan's InMail of 2026-11-08)
  'ITManOnTop'
)

# The classes test windows get: the perf scripts' (Start-Test's com.lachlanb.lbterm.perf-<name>),
# the motion scripts' and lbterm.ps1 try's
$script:TestWindowClasses = @(
  'com.lachlanb.lbterm.perf*'
  'com.lachlanb.lbterm.motion', 'com.lachlanb.lbterm.compare', 'com.lachlanb.lbterm.focus', 'com.lachlanb.lbterm.panel'
  'com.lachlanb.lbterm.try'
)

# Whether a seen window is an LBTerm process's (lbterm-gui, lbterm-mux-server, lbterm)
function Test-LbtermWindow([Parameter(Mandatory)]$Seen) { [string]$Seen.Exe -like 'lbterm*.exe' }

# Whether a seen window is the test's own: an LBTerm window whose process, or one above it, is one
# the test opened
function Test-OwnWindow([Parameter(Mandatory)]$Seen, [uint32[]]$Allowed = @()) {
  if (-not (Test-LbtermWindow $Seen)) { return $false }
  foreach ($p in @($Seen.Pid) + @($Seen.Ancestors)) { if ([uint32]$p -in $Allowed) { return $true } }
  $false
}

# Whether a seen window is one that comes and goes by itself (a tooltip, a menu, a browser's status
# bubble)
function Test-NoiseWindow([Parameter(Mandatory)]$Seen) {
  foreach ($c in $script:NoiseWindowClasses) { if ([string]$Seen.Class -like $c) { return $true } }
  # Chrome's and Edge's untitled windows: the status bubble, a strip along a browser window's foot
  # shown as a real pointer crosses a link (270x23 and 1280x23 in one budgets run, 0 ms after
  # Lachlan's last input, 2026-10-30), and the popups (the address bar's list, menus, autofill and
  # extension popups: 409x600, 270x58 and 270x142 failed all four title-storm tries on 2026-11-10,
  # 0 to 1,265 ms after his input; his E3). Each is a Chrome_WidgetWin_1 with no title; a browser
  # window, a page a test opened included, has its page's title, and still counts
  if (([string]$Seen.Class -eq 'Chrome_WidgetWin_1') -and ([string]$Seen.Exe -in @('chrome.exe', 'msedge.exe')) -and -not $Seen.Title) { return $true }
  $false
}

# Whether a window class is a test window's
function Test-TestWindowClass([string]$Class) {
  foreach ($c in $script:TestWindowClasses) { if ($Class -like $c) { return $true } }
  $false
}

# Whether a seen window is another LBTerm test window: an LBTerm process's, with a test window's
# class or of a process showing one then (its ghost card, its torn-off tab)
function Test-OtherTestWindow([Parameter(Mandatory)]$Seen) {
  if (-not (Test-LbtermWindow $Seen)) { return $false }
  foreach ($c in @($Seen.Class) + @($Seen.ProcessClasses)) { if ($c -and (Test-TestWindowClass $c)) { return $true } }
  $false
}

# What a seen window is to the test: own, noise, allowed, test or foreign (above)
function Get-WindowKind {
  param([Parameter(Mandatory)]$Seen, [uint32[]]$Allowed = @(), [scriptblock]$Allow)
  if (Test-NoiseWindow $Seen) { 'noise' }
  elseif (Test-OwnWindow $Seen $Allowed) { 'own' }
  elseif ($Allow -and (& $Allow $Seen)) { 'allowed' }
  elseif (Test-OtherTestWindow $Seen) { 'test' }
  else { 'foreign' }
}

# The seen windows the test fails for: neither its own, nor noise, nor let through by -Allow
# (param($w)), nor another test's
function Get-ForeignWindows {
  param([object[]]$Seen = @(), [uint32[]]$Allowed = @(), [scriptblock]$Allow)
  @($Seen | Where-Object { $_ -and (Get-WindowKind -Seen $_ -Allowed $Allowed -Allow $Allow) -eq 'foreign' })
}

# The seen windows that are other LBTerm test windows: notes, not failures
function Get-OtherTestWindows {
  param([object[]]$Seen = @(), [uint32[]]$Allowed = @(), [scriptblock]$Allow)
  @($Seen | Where-Object { $_ -and (Get-WindowKind -Seen $_ -Allowed $Allowed -Allow $Allow) -eq 'test' })
}

# A foreign window as a failure (or with -Note, another test's as a note): what opened it, what it
# said, when, and how long after Lachlan's last real key or mouse input (a window of his own just
# after he clicked reads as his)
function Format-ForeignWindow([Parameter(Mandatory)]$Seen, [switch]$Note) {
  $chain = @(for ($i = 0; $i -lt [Math]::Min(3, @($Seen.Ancestors).Count); $i++) { "$(@($Seen.AncestorExes)[$i]) $(@($Seen.Ancestors)[$i])" })
  $from = if ($chain) { ", started by $($chain -join ' < ')" } else { '' }
  $at = if ($Seen.At -is [datetime]) { $Seen.At.ToString('HH:mm:ss.fff') } else { "$($Seen.At)" }
  $what = if ($Note) { 'another LBTerm test window appeared (not counted)' } else { "a window that isn't the test's appeared" }
  "${what}: $($Seen.Exe) (pid $($Seen.Pid)$from), '$($Seen.Title)' [$($Seen.Class)], $($Seen.Width)x$($Seen.Height), at $at, $($Seen.IdleMs) ms after the last real key or mouse input"
}

#endregion

#region Test windows

# Waits until Lachlan's hands have been off the keyboard and mouse for $Ms (up to $TimeoutSec),
# so a window opening as he types can't catch a key
function Wait-Idle([int]$Ms = 3000, [int]$TimeoutSec = 120) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ([LbtermE2E.Win]::IdleMs() -lt $Ms -and $clock.Elapsed.TotalSeconds -lt $TimeoutSec) { Start-Sleep -Milliseconds 250 }
}

# The GUI socket a window's cli talks to
function Get-GuiSocket([int]$ProcessId) { Join-Path $env:USERPROFILE ".local\share\lbterm\gui-sock-$ProcessId" }

# A window object for a GUI process: its handle, sockets, files and DPI scale
function New-WindowInfo {
  param([int]$ProcessId, [IntPtr]$Handle, [string]$Class, [string]$Config, [string]$Domain, [string]$ServerSocket, [string]$StateFile, [long]$LogStart = 0, $Ran)
  $scale = if ($Handle -ne [IntPtr]::Zero) { [LbtermE2E.Win]::GetDpiForWindow($Handle) / 96.0 } else { 1.0 }
  if ($scale -le 0) { $scale = 1.0 }
  # A process the test didn't start was let run as it was created
  if (-not $Ran) { $Ran = (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue).StartTime }
  [pscustomobject]@{
    Pid          = $ProcessId
    Handle       = $Handle
    Class        = $Class
    Config       = $Config
    Domain       = $Domain
    Socket       = Get-GuiSocket $ProcessId
    ServerSocket = $ServerSocket
    StateFile    = $StateFile
    Log          = Get-GuiLogPath $ProcessId
    # Where this process's lines start in it: a GUI that has a pid an earlier one had appends to
    # that one's log (Get-LogStart)
    LogStart     = $LogStart
    Scale        = $scale
    # When its process was let run: time a start from here, not from its process's StartTime,
    # which for a window handed to the test's job is the 0.5 to 2 s of the handoff before
    Ran          = $Ran
    Opened       = Get-Date
  }
}

# Takes a GUI process the test didn't open itself (one a restart started) as one of its windows,
# once it shows a window; it's closed with the rest
function Add-TestWindow {
  # -Ran: when it was let run, for one started -Held (Resume-StartedProcess's)
  param([Parameter(Mandatory)][int]$ProcessId, [object]$Like, [int]$TimeoutSec = 20, $Ran)
  $t = Get-Test
  $h = Wait-Window -Id $ProcessId -Seconds $TimeoutSec
  $w = New-WindowInfo -ProcessId $ProcessId -Handle $h -Class $Like.Class -Config $Like.Config -Domain $Like.Domain -ServerSocket $Like.ServerSocket -StateFile $Like.StateFile -Ran $Ran
  $t.Windows.Add($w)
  Add-Content (Join-Path $t.Out 'pids.txt') $ProcessId
  $w
}

# Opens a test window and waits for it to show. Its config is one of: -ConfigFile (the script
# wrote it, setting wsl_domains = {}), -NoConfig (-n: none, crash restore off, no WSL domains), or else New-TestConfig (Lachlan's
# lbterm.lua without what reaches outside the window) with -Lua's lines added, in a folder of its
# own. -Set adds --config name=value. -Domain connects to a mux domain (whose socket is -Socket)
# instead of starting one of its own. -Program is what the first tab runs.
function Open-TestWindow {
  param(
    [string]$ConfigFile,
    [string]$Lua = '',
    [switch]$NoConfig,
    [string[]]$Set = @(),
    [string]$Domain,
    [string]$Socket,
    [object[]]$Program,
    [string]$Cwd,
    [hashtable]$Environment = @{},
    # LBTERM_POSTED_POINTER: a posted move hovers until a posted WM_MOUSELEAVE
    [switch]$PostedPointer,
    # WEZTERM_LOG, as module=level
    [string]$Log,
    # USERPROFILE for the window: a scratch folder, so lbterm.lua's handlers write nothing real
    [string]$UserProfile,
    [string]$Class,
    # Another lbterm-gui.exe: a copy of the build
    [string]$Exe,
    [string]$WorkingDirectory,
    # Without --always-new-process
    [switch]$SameProcess,
    # Arguments before the verb, as they go on the command line
    [string]$Before = '',
    # Waits until Lachlan's hands are off the keyboard and mouse first
    [switch]$WaitIdle,
    [int]$TimeoutSec = 20
  )
  $t = Get-Test
  # The displays kept on while this script's windows run, a script run on its own included (E4)
  if ($said = Request-DisplaysAwake) {
    if (Get-DisplaysAsleep) { Write-Warning "${said}: what this window shows can't be judged (PITFALLS 239)" } else { Write-Host $said }
  }
  $n = $t.Windows.Count
  if (-not $Class) { $Class = $t.Class }
  [void]$t.Classes.Add($Class)
  if (-not $Exe) { $Exe = $t.Gui }
  $cfgArgs = if ($ConfigFile) { "--config-file $(ConvertTo-CommandLineArg $ConfigFile)" }
  elseif ($NoConfig) { '-n --config lbterm_crash_restore=false --config wsl_domains={}' }
  else {
    # In a folder of its own: WezTerm watches a config's folder, and captures, the state file or
    # anything else written beside it would reload the window
    $ConfigFile = Join-Path $t.Out "config\test$(if ($n) { "-$n" }).lua"
    New-Item -ItemType Directory -Force (Split-Path $ConfigFile) | Out-Null
    New-TestConfig -Path $ConfigFile -Lua $Lua
    "--config-file $(ConvertTo-CommandLineArg $ConfigFile)"
  }
  # Every test window's config sets wsl_domains = {} (-NoConfig's on the command line). Unset, every
  # spawn and a mux server's start run `wsl.exe -l -v`, and where WSL isn't installed, Windows 11
  # 25H2's wsl.exe answers with WSL's installer: a window in front of whoever is at the keyboard,
  # whose next keypress installs it (cli-list.ps1's -NoConfig window, three times on 2026-10-28)
  if ($ConfigFile -and (Get-Content -Raw -LiteralPath $ConfigFile) -notmatch '\bwsl_domains\s*=') {
    throw "the test config $ConfigFile doesn't set wsl_domains = {}: without it every spawn runs wsl.exe, which opens WSL's installer in front where WSL isn't installed"
  }
  $sets = ($Set | ForEach-Object { "--config $(ConvertTo-CommandLineArg $_)" }) -join ' '
  $verb = if ($Domain) { "connect $Domain" } elseif ($SameProcess) { 'start' } else { 'start --always-new-process' }
  $tail = "--position $TestWindowPosition --class $Class"
  if ($Cwd) { $tail += " --cwd $(ConvertTo-CommandLineArg $Cwd)" }
  if ($Program) { $tail += ' -- ' + (($Program | ForEach-Object { ConvertTo-CommandLineArg ([string]$_) }) -join ' ') }
  $stateFile = Join-Path $t.Out "state$(if ($n) { "-$n" }).json"
  Remove-Item $stateFile -ErrorAction SilentlyContinue
  $envs = @{ LBTERM_STATE_FILE = $stateFile }
  if ($PostedPointer) { $envs.LBTERM_POSTED_POINTER = '1' }
  # WEZTERM_LOG for a window: -Log, else LBTERM_E2E_LOG (lbterm.ps1 e2e -Log), so a trace can be
  # turned on for a script that sets none without editing it. The window's environment never takes
  # WEZTERM_ variables from the test's own (Start-Unfocused leaves them out), so WEZTERM_LOG set
  # round a run reaches no window
  if ($Log) { $envs.WEZTERM_LOG = $Log } elseif ($env:LBTERM_E2E_LOG) { $envs.WEZTERM_LOG = $env:LBTERM_E2E_LOG }
  if ($UserProfile) { New-Item -ItemType Directory -Force $UserProfile | Out-Null; $envs.USERPROFILE = $UserProfile }
  foreach ($k in $Environment.Keys) { $envs[$k] = $Environment[$k] }
  # LBTERM_E2E_WAIT_IDLE (lbterm.ps1 e2e -WaitIdle) asks it of every window
  if ($WaitIdle -or $env:LBTERM_E2E_WAIT_IDLE) { Wait-Idle }
  $arguments = (@($cfgArgs, $sets, $Before, $verb, $tail) | Where-Object { $_ }) -join ' '
  # -Job: in the job the test runs in, on its CPUs, and ended with it (start-unfocused.ps1)
  $start = @{ Exe = $Exe; Arguments = $arguments; Environment = $envs; Job = $true; PassThru = $true }
  if ($WorkingDirectory) { $start.WorkingDirectory = $WorkingDirectory }
  $started = Start-Unfocused @start
  $procId = $started.Id
  # Before it has written anything: what's in its log already is an earlier process's
  $logStart = Get-LogStart $procId
  Add-Content (Join-Path $t.Out 'pids.txt') $procId
  $h = Wait-Window -Id $procId -Seconds $TimeoutSec
  $w = New-WindowInfo -ProcessId $procId -Handle $h -Class $Class -Config $ConfigFile -Domain $Domain -ServerSocket $Socket -StateFile $stateFile -LogStart $logStart -Ran $started.Ran
  $t.Windows.Add($w)
  if ($h -eq [IntPtr]::Zero) {
    $alive = [bool](Get-Process -Id $procId -ErrorAction SilentlyContinue)
    throw "the test window (pid $procId) didn't open$(if (-not $alive) { ' (it ended)' }). Its log: $($w.Log)"
  }
  $t.Current = $w
  Use-TestWindow $w
  Watch-Foreground
  $w
}

# Makes $Window the one the cli talks to and Invoke-Cli and the waits default to: its GUI socket
# (or with -Server its mux server's) in WEZTERM_UNIX_SOCKET, and no WEZTERM_PANE from the shell
function Use-TestWindow([Parameter(Mandatory)]$Window, [switch]$Server) {
  $t = Get-Test
  $t.Current = $Window
  $env:WEZTERM_UNIX_SOCKET = if ($Server) { $Window.ServerSocket } else { $Window.Socket }
  Remove-Item Env:WEZTERM_PANE -ErrorAction SilentlyContinue
}

function Resolve-Window($Window) {
  if ($Window) { return $Window }
  $t = Get-Test
  if (-not $t.Current) { throw 'no test window open' }
  $t.Current
}

# A window's handle: a window object's, or a handle as it is
function Resolve-Handle($Window) {
  if ($Window -is [IntPtr]) { return $Window }
  (Resolve-Window $Window).Handle
}

# Ends a test window as a crash would, and waits for it to go
function Stop-TestWindow([Parameter(Mandatory)]$Window, [int]$TimeoutMs = 5000) {
  Watch-Foreground
  $p = Get-Process -Id $Window.Pid -ErrorAction SilentlyContinue
  if ($p) { Stop-Process -Id $Window.Pid -Force -ErrorAction SilentlyContinue; $p.WaitForExit($TimeoutMs) | Out-Null }
}

# Whether a test window's process is still running
function Test-WindowAlive([Parameter(Mandatory)]$Window) { [bool](Get-Process -Id $Window.Pid -ErrorAction SilentlyContinue) }

# The visible top-level windows of a test window's process with its class (a torn-off tab's too)
function Get-ProcessWindows([Parameter(Mandatory)]$Window) { @([LbtermE2E.Win]::Windows([uint32]$Window.Pid, $Window.Class)) }

# The test's mux servers: those whose command line names its output folder, where its configs are
function Get-TestServers {
  $t = Get-Test
  @(Get-CimInstance Win32_Process -Filter "Name = 'lbterm-mux-server.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -and ($_.CommandLine -replace '/', '\') -like "*$($t.Out)\*" })
}

function Stop-TestServers {
  foreach ($s in Get-TestServers) { Stop-Process -Id $s.ProcessId -Force -ErrorAction SilentlyContinue }
}

# The window's client rectangle (0, 0, width, height), and the window's own on the screen
function Get-ClientRect($Window) {
  $r = [LbtermE2E.Win+RECT]::new(); [void][LbtermE2E.Win]::GetClientRect((Resolve-Handle $Window), [ref]$r)
  [System.Drawing.Rectangle]::new(0, 0, $r.Right, $r.Bottom)
}
function Get-WindowRect($Window) {
  $r = [LbtermE2E.Win+RECT]::new(); [void][LbtermE2E.Win]::GetWindowRect((Resolve-Handle $Window), [ref]$r)
  $r
}
# A client point on the screen
function ConvertTo-ScreenPoint($Window, [int]$X, [int]$Y) {
  $p = [LbtermE2E.Win+POINT]::new(); $p.X = $X; $p.Y = $Y
  [void][LbtermE2E.Win]::ClientToScreen((Resolve-Handle $Window), [ref]$p)
  $p
}

# Resizes a window without activating it or moving it
function Set-WindowSize($Window, [int]$Width, [int]$Height) {
  # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
  [void][LbtermE2E.Win]::SetWindowPos((Resolve-Handle $Window), [IntPtr]::Zero, 0, 0, $Width, $Height, 0x0002 -bor 0x0004 -bor 0x0010)
}

# Notes when Lachlan's real pointer crosses the window: Complete-Test says so beside a failure
function Start-PointerWatch($Window) { [LbtermE2E.Win]::WatchPointer((Resolve-Handle $Window)) }

# Tells the window a drag of its frame begins (WM_ENTERSIZEMOVE), or with -End that it's over
# (WM_EXITSIZEMOVE), sent and waited for as Windows' sizing loop sends them: the sizes a test gives
# it between the two (Set-WindowSize) are a drag's (live_resizing). Nothing is focused or moved
function Send-SizeMove([Parameter(Mandatory)]$Window, [switch]$End) {
  if (-not ('LbtermE2E.SizeMove' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
namespace LbtermE2E {
  public static class SizeMove {
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    public static bool Send(IntPtr h, bool end) {
      IntPtr r;
      return SendMessageTimeout(h, end ? 0x0232u : 0x0231u, IntPtr.Zero, IntPtr.Zero, 0, 30000, out r) != IntPtr.Zero;
    }
  }
}
'@
  }
  if (-not [LbtermE2E.SizeMove]::Send((Resolve-Handle $Window), [bool]$End)) { throw "the window didn't take $(if ($End) { 'WM_EXITSIZEMOVE' } else { 'WM_ENTERSIZEMOVE' }) in 30 s" }
}

# How long the window's thread goes without taking a message: a WM_NULL sent every -EveryMs from a
# thread of the test's own (SendMessageTimeout, which returns once the window's thread has taken
# it, which it does only in PeekMessage, GetMessage or a wait that pumps), each wait timed. Windows
# calls a window not responding once its thread hasn't taken a message for 5 s, and DWM then puts
# its ghost window over it (lbterm/PERF.md, finding 33): a wait of 5 s here is that. A WM_NULL does
# nothing in the window (DefWindowProc), but each one wakes an idle message loop, so don't run it
# beside a measurement of a window at rest. Stop-HangWatch says what it saw
function Start-HangWatch([Parameter(Mandatory)]$Window, [int]$EveryMs = 10) {
  if (-not ('LbtermE2E.HangWatch' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
namespace LbtermE2E {
  public class HangWatch {
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr h);
    readonly IntPtr h; readonly int every; volatile bool stop; readonly Thread thread;
    readonly Stopwatch clock = Stopwatch.StartNew();
    public readonly List<double> Waits = new List<double>();
    public double Longest; public double LongestAtMs; public int Failed;
    public readonly double StartUnixMs = (DateTime.UtcNow - DateTime.UnixEpoch).TotalMilliseconds;
    public HangWatch(IntPtr h, int every) {
      this.h = h; this.every = every;
      thread = new Thread(Run) { IsBackground = true };
      thread.Start();
    }
    void Run() {
      while (!stop) {
        double t0 = clock.Elapsed.TotalMilliseconds;
        IntPtr r;
        // SMTO_NORMAL, and a minute: a wait that long is reported as a minute
        IntPtr ok = SendMessageTimeout(h, 0, IntPtr.Zero, IntPtr.Zero, 0, 60000, out r);
        double took = clock.Elapsed.TotalMilliseconds - t0;
        if (ok == IntPtr.Zero && !IsWindow(h)) break;
        lock (Waits) {
          if (ok == IntPtr.Zero) Failed++;
          Waits.Add(took);
          if (took > Longest) { Longest = took; LongestAtMs = t0; }
        }
        double next = t0 + every;
        while (!stop && clock.Elapsed.TotalMilliseconds < next) Thread.Sleep(1);
      }
    }
    public double[] Stop() { stop = true; thread.Join(65000); lock (Waits) { return Waits.ToArray(); } }
  }
}
'@
  }
  [LbtermE2E.HangWatch]::new((Resolve-Handle $Window), $EveryMs)
}

# What a hang watch saw (Start-HangWatch): how many messages were sent, the longest wait for one
# (ms) and when it began (ms after the watch started), the 99th percentile, and how many waited
# over 100 ms, 1 s and 5 s (the last is Windows' not responding)
function Stop-HangWatch([Parameter(Mandatory)]$Watch) {
  $waits = @($Watch.Stop())
  $sorted = @($waits | Sort-Object)
  [ordered]@{
    Sent = $waits.Count
    LongestMs = [Math]::Round($Watch.Longest, 1)
    LongestAtMs = [Math]::Round($Watch.LongestAtMs, 0)
    P99Ms = if ($sorted.Count) { [Math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int][Math]::Floor(0.99 * ($sorted.Count - 1) + 0.5))], 1) } else { $null }
    Over100Ms = @($waits | Where-Object { $_ -gt 100 }).Count
    Over1s = @($waits | Where-Object { $_ -gt 1000 }).Count
    Over5s = @($waits | Where-Object { $_ -gt 5000 }).Count
    Failed = $Watch.Failed
  }
}

# A GUI process's log (every lbterm-gui writes one, named by its pid)
function Get-GuiLogPath([int]$ProcessId) { Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-gui.exe-log-$ProcessId.txt" }

# How long a GUI's log is now: taken as its process starts, it's where that process's lines will
# begin. Nothing a process logs names it, so one given a pid an earlier GUI had appends to that
# one's log, and a test reading the whole file found the earlier process's lines (a budgets run's
# `tab took 26.5 ms` read by budget-log.ps1 -Mux as its own, 2026-10-30)
function Get-LogStart([int]$ProcessId) {
  $f = Get-Item -LiteralPath (Get-GuiLogPath $ProcessId) -ErrorAction SilentlyContinue
  if ($f) { [long]$f.Length } else { 0 }
}

# The lines of a log from byte $Start on (read shared, as the GUI writes it); none for no file
function Read-LogLines([string]$Path, [long]$Start = 0) {
  if (-not (Test-Path -LiteralPath $Path)) { return @() }
  $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
  try {
    if ($Start -gt 0 -and $Start -le $fs.Length) { [void]$fs.Seek($Start, [IO.SeekOrigin]::Begin) }
    $text = [IO.StreamReader]::new($fs, [Text.Encoding]::UTF8).ReadToEnd()
  } finally { $fs.Dispose() }
  @($text -split '\r?\n' | Where-Object { $_ })
}

# Whether a test window's log has a line matching $Pattern, waiting up to $TimeoutSec: its own
# lines, not an earlier process's of the same pid
function Wait-LogLine($Window, [Parameter(Mandatory)][string]$Pattern, [double]$TimeoutSec = 10) {
  $w = Resolve-Window $Window
  Wait-Until { @(Read-LogLines $w.Log $w.LogStart | Where-Object { $_ -match $Pattern }).Count -gt 0 } -TimeoutSec $TimeoutSec
}

# The lines of a test window's log matching $Pattern: its own, not an earlier process's
function Get-LogLines($Window, [Parameter(Mandatory)][string]$Pattern) {
  $w = Resolve-Window $Window
  @(Read-LogLines $w.Log $w.LogStart | Where-Object { $_ -match $Pattern })
}

#endregion

#region The cli

# `lbterm cli --no-auto-start <Arguments> [-- <Program>]` against the current test window's GUI
# socket (-Window for another, -Server for its mux server's, -Socket for any). Its stdout lines,
# or with -Json what they parse to. $LASTEXITCODE is the cli's. -Check throws when it fails;
# -Merge keeps stderr with stdout.
function Invoke-Cli {
  [CmdletBinding(PositionalBinding = $false)]
  param(
    [Parameter(ValueFromRemainingArguments)][object[]]$Arguments,
    [object]$Window,
    [switch]$Server,
    [string]$Socket,
    [object[]]$Program,
    [switch]$Json,
    [switch]$Check,
    [switch]$Merge
  )
  $t = Get-Test
  $sock = if ($Socket) { $Socket } else {
    $w = Resolve-Window $Window
    if ($Server) { $w.ServerSocket } else { $w.Socket }
  }
  $env:WEZTERM_UNIX_SOCKET = $sock
  Remove-Item Env:WEZTERM_PANE -ErrorAction SilentlyContinue
  $line = @('cli', '--no-auto-start') + @($Arguments | ForEach-Object { [string]$_ })
  if ($Program) { $line += @('--') + @($Program | ForEach-Object { [string]$_ }) }
  $all = @(& $t.Cli @line 2>&1)
  $code = $LASTEXITCODE
  $isErr = { param($o) $o -is [System.Management.Automation.ErrorRecord] }
  $out = if ($Merge) { @($all | ForEach-Object { "$_" }) } else { @($all | Where-Object { -not (& $isErr $_) } | ForEach-Object { "$_" }) }
  if ($Check -and $code) {
    throw "lbterm $($line -join ' ') failed ($code): $((@($all | Where-Object { & $isErr $_ }) | ForEach-Object { "$_" }) -join ' ')"
  }
  $global:LASTEXITCODE = $code
  if ($Json) {
    $text = ($out | Out-String).Trim()
    if (-not $text) { return @() }
    try { return @($text | ConvertFrom-Json) } catch { return @() }
  }
  $out
}

# The panes, in window and tab order, as `cli list` has them (the server's with -Server)
function Get-Panes([object]$Window, [switch]$Server) {
  $panes = @(Invoke-Cli list --format json -Json -Window $Window -Server:$Server)
  if ($Server) { return $panes }
  # On the GUI's own socket, without a Configuration Error window's pane: a build that doesn't know
  # a key in the test's config (an older one measured as "before") opens that window beside the
  # test's (mux/src/connui.rs titles it "wezterm Configuration Error"), and its pane can come first
  # where the scripts take (Get-Panes)[0] as the test window's (conductor.ps1 and estimates.ps1
  # failed so on 2026-10-24). The test's notes say once that it was there. The server never has it
  $bad = @($panes | Where-Object { Test-ConfigErrorPane $_ })
  if ($bad.Count -and $script:Test) {
    $note = "a Configuration Error window was open beside the test window (this build doesn't know a key in the test's config); Get-Panes left its pane out"
    if (-not $script:Test.Notes.Contains($note)) { $script:Test.Notes.Add($note) }
  }
  @($panes | Where-Object { -not (Test-ConfigErrorPane $_) })
}

# Whether a pane from cli list is a Configuration Error window's (mux/src/connui.rs)
function Test-ConfigErrorPane($Pane) {
  ([string]$Pane.title -like '*Configuration Error*') -or ([string]$Pane.window_title -like '*Configuration Error*')
}

# One pane per tab, in order, for window $WindowId (or all)
function Get-Tabs([object]$WindowId = $null, [object]$Window, [switch]$Server) {
  $seen = @{}
  foreach ($p in Get-Panes -Window $Window -Server:$Server) {
    if ($null -ne $WindowId -and $p.window_id -ne $WindowId) { continue }
    if ($seen.ContainsKey($p.tab_id)) { continue }
    $seen[$p.tab_id] = $true
    $p
  }
}

# A new tab beside pane $From (in its window), running $Program: its pane id
function New-Tab {
  param([Parameter(Mandatory)][int]$From, [object[]]$Program, [string]$Cwd, [switch]$NoActivate, [object]$Window, [switch]$Server)
  $a = @('spawn', '--pane-id', $From)
  if ($Cwd) { $a += @('--cwd', $Cwd) }
  if ($NoActivate) { $a += '--no-activate' }
  $got = (Invoke-Cli @a -Program $Program -Window $Window -Server:$Server -Merge) -join ' '
  if ($got.Trim() -notmatch '^\d+$') { throw "spawning a tab beside pane $From printed: $got" }
  [int]$got.Trim()
}

#endregion

#region Waiting

# Whether $Condition came true within $TimeoutSec, asked every $IntervalMs
function Wait-Until {
  param([Parameter(Mandatory, Position = 0)][scriptblock]$Condition, [double]$TimeoutSec = 5, [int]$IntervalMs = 150)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt $TimeoutSec) {
    if (& $Condition) { return $true }
    Start-Sleep -Milliseconds $IntervalMs
  }
  [bool](& $Condition)
}

# Sleeps $Ms, keeping a held pointer (Set-PointerHold) where it is every 16 ms
function Wait-Ms([int]$Ms) {
  $t = Get-Test
  if (-not $t.Hold) { Start-Sleep -Milliseconds $Ms; return }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.ElapsedMilliseconds -lt $Ms) { Send-HeldPointer; Start-Sleep -Milliseconds 16 }
}

#endregion

#region Mouse and keys: posted messages, never real input

$script:WM = @{
  Move = 0x0200; LDown = 0x0201; LUp = 0x0202; RDown = 0x0204; RUp = 0x0205; MDown = 0x0207; MUp = 0x0208
  Wheel = 0x020A; Leave = 0x02A3; SetFocus = 0x0007; KillFocus = 0x0008; Close = 0x0010
  XDown = 0x020B; XUp = 0x020C
}
$script:MK = @{ None = 0; L = 1; R = 2; Shift = 4; Ctrl = 8; M = 0x10; X1 = 0x20; X2 = 0x40 }

# One posted mouse message: Move, LDown, LUp, RDown, RUp, MDown, MUp; -Keys says which buttons are
# held (L, R, M), as its wParam does
function Send-Mouse {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Mandatory, Position = 1)][string]$Message, [Parameter(Position = 2)][int]$X, [Parameter(Position = 3)][int]$Y, [string[]]$Keys = @())
  $k = 0; foreach ($b in $Keys) { $k = $k -bor $script:MK[$b] }
  [LbtermE2E.Win]::Mouse((Resolve-Handle $Window), [uint32]$script:WM[$Message], $k, $X, $Y)
  Watch-Foreground
}

# A click: press and release at (x, y), the left button or -Right; -Move first, as a hand's
# pointer arrives before it presses; then -PauseMs for the window to answer
function Send-Click {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Position = 1)][int]$X, [Parameter(Position = 2)][int]$Y, [switch]$Right, [switch]$Move, [int]$PauseMs = 0)
  $h = Resolve-Handle $Window
  if ($Move) { [LbtermE2E.Win]::Mouse($h, $script:WM.Move, 0, $X, $Y) }
  if ($Right) { [LbtermE2E.Win]::Mouse($h, $script:WM.RDown, 2, $X, $Y); [LbtermE2E.Win]::Mouse($h, $script:WM.RUp, 0, $X, $Y) }
  else { [LbtermE2E.Win]::Mouse($h, $script:WM.LDown, 1, $X, $Y); [LbtermE2E.Win]::Mouse($h, $script:WM.LUp, 0, $X, $Y) }
  if ($PauseMs) { Start-Sleep -Milliseconds $PauseMs }
  Watch-Foreground
}

# A drag in steps, as a hand would make it: press at the start, -Steps moves -StepMs apart, let go
function Send-Drag {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Position = 1)][int]$X0, [Parameter(Position = 2)][int]$Y0, [Parameter(Position = 3)][int]$X1, [Parameter(Position = 4)][int]$Y1, [int]$Steps = 12, [int]$StepMs = 40, [int]$HoldMs = 100, [int]$SettleMs = 500)
  $h = Resolve-Handle $Window
  [LbtermE2E.Win]::Mouse($h, $script:WM.Move, 0, $X0, $Y0)
  [LbtermE2E.Win]::Mouse($h, $script:WM.LDown, 1, $X0, $Y0)
  Start-Sleep -Milliseconds $HoldMs
  foreach ($k in 1..$Steps) {
    [LbtermE2E.Win]::Mouse($h, $script:WM.Move, 1, [int]($X0 + ($X1 - $X0) * $k / $Steps), [int]($Y0 + ($Y1 - $Y0) * $k / $Steps))
    Start-Sleep -Milliseconds $StepMs
    Watch-Foreground
  }
  [LbtermE2E.Win]::Mouse($h, $script:WM.LUp, 0, $X1, $Y1)
  Start-Sleep -Milliseconds $SettleMs
  Watch-Foreground
}

# A turn of the wheel at client point (x, y): +120 a notch away from the user
function Send-Wheel {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Position = 1)][int]$X, [Parameter(Position = 2)][int]$Y, [int]$Delta = 120)
  $h = Resolve-Handle $Window
  # WM_MOUSEWHEEL carries screen coordinates
  $p = ConvertTo-ScreenPoint $h $X $Y
  [LbtermE2E.Win]::Post($h, $script:WM.Wheel, [LbtermE2E.Win]::WheelWParam($Delta, 0), [LbtermE2E.Win]::LParam($p.X, $p.Y))
}

# A side button's press and release at client point (x, y): -Forward (XBUTTON2), else back
# (XBUTTON1), with -Keys held (Shift, Ctrl); WM_XBUTTONDOWN's wParam names the button in its high word
function Send-SideButton {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Position = 1)][int]$X, [Parameter(Position = 2)][int]$Y, [switch]$Forward, [string[]]$Keys = @())
  $h = Resolve-Handle $Window
  $k = 0; foreach ($b in $Keys) { $k = $k -bor $script:MK[$b] }
  $which = if ($Forward) { 2 } else { 1 }
  $held = if ($Forward) { $script:MK.X2 } else { $script:MK.X1 }
  [LbtermE2E.Win]::Mouse($h, $script:WM.XDown, ($which -shl 16) -bor $held -bor $k, $X, $Y)
  [LbtermE2E.Win]::Mouse($h, $script:WM.XUp, ($which -shl 16) -bor $k, $X, $Y)
  Watch-Foreground
}

# The pointer leaving the window (WM_MOUSELEAVE)
function Send-Leave([Parameter(Mandatory)]$Window) { [LbtermE2E.Win]::Mouse((Resolve-Handle $Window), $script:WM.Leave, 0, 0, 0); Watch-Foreground }

# The keyboard arriving (WM_SETFOCUS) or, with -Off, going (WM_KILLFOCUS), without the
# foreground ever moving
function Send-Focus([Parameter(Mandatory)]$Window, [switch]$Off) {
  [LbtermE2E.Win]::Mouse((Resolve-Handle $Window), $(if ($Off) { $script:WM.KillFocus } else { $script:WM.SetFocus }), 0, 0, 0)
  Watch-Foreground
}

# The window closed as its close button does (WM_CLOSE)
function Send-Close([Parameter(Mandatory)]$Window) { [LbtermE2E.Win]::Mouse((Resolve-Handle $Window), $script:WM.Close, 0, 0, 0) }

# Waits, up to -TimeoutMs, until no modifier is held on the PC ($Held, the real keyboard's: the
# harness's ModifierHeld), and returns how long it waited. A key a test posts is read with the
# modifiers its window's thread sees, which follow the real keyboard's: on 2026-11-01 at 23:13,
# with Lachlan holding Shift in a game, a test's posted A arrived as a capital and F5 as
# Shift+F5, which its binding didn't take (perf/mux-gui.ps1), and a Ctrl held would turn a letter
# into a control character no echo shows. A wait is noted in the test's notes, once a test with
# the longest; one that runs out posts the key anyway and says so
function Wait-ModifiersUp([int]$TimeoutMs = 2000, [scriptblock]$Held = { [LbtermE2E.Win]::ModifierHeld() }) {
  if (-not (& $Held)) { return 0 }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ((& $Held) -and $clock.ElapsedMilliseconds -lt $TimeoutMs) { Start-Sleep -Milliseconds 5 }
  $waited = [int]$clock.ElapsedMilliseconds
  $t = $script:Test
  if ($t) {
    if ($null -eq $t.PSObject.Properties['ModifierWaits']) { $t | Add-Member -NotePropertyName ModifierWaits -NotePropertyValue ([pscustomobject]@{ Count = 0; LongestMs = 0; Note = $null }) }
    $w = $t.ModifierWaits
    $w.Count++
    $w.LongestMs = [Math]::Max($w.LongestMs, $waited)
    $note = "a modifier was held on the PC (its real keyboard) as keys were posted: $($w.Count) times, the longest wait $($w.LongestMs) ms$(if (& $Held) { ', the last still held when the key went' })"
    if ($t.PSObject.Properties['Notes'] -and $null -ne $t.Notes) {
      if ($w.Note) { [void]$t.Notes.Remove($w.Note) }
      $t.Notes.Add($note)
    }
    $w.Note = $note
  }
  $waited
}

# Keys pressed and let go, by name ('Enter', 'Down', 'F5', 'Y') or code, -GapMs apart. The
# navigation keys go as extended keys, as a keyboard's own do.
function Send-Key {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Mandatory, Position = 1)][object[]]$Key, [int]$GapMs = 0)
  $h = Resolve-Handle $Window
  foreach ($k in $Key) {
    Wait-ModifiersUp | Out-Null
    $vk = ConvertTo-Vk $k
    [LbtermE2E.Win]::Key($h, $vk, ($vk -in $script:ExtendedVk))
    if ($GapMs) { Start-Sleep -Milliseconds $GapMs }
  }
  Watch-Foreground
}

# Letters and digits typed as their keys
function Send-Chars([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Text, [int]$GapMs = 0) {
  Send-Key $Window @($Text.ToUpperInvariant().ToCharArray() | ForEach-Object { [string]$_ }) -GapMs $GapMs
}

# Keeps the pointer at (x, y): a posted move there before every capture and every 16 ms of
# Wait-Ms, so it stays whatever else happens (with LBTERM_POSTED_POINTER). Clear-PointerHold ends it.
function Set-PointerHold([Parameter(Mandatory)]$Window, [int]$X, [int]$Y) {
  (Get-Test).Hold = [pscustomobject]@{ Handle = (Resolve-Handle $Window); X = $X; Y = $Y }
  Send-HeldPointer
}
function Clear-PointerHold { (Get-Test).Hold = $null }
function Send-HeldPointer {
  $hold = $script:Test.Hold
  if ($hold) { [LbtermE2E.Win]::Mouse($hold.Handle, $script:WM.Move, 0, $hold.X, $hold.Y) }
}

# The messages Send-SideButton posts, as Start-TimedPosts takes them: each { Msg, W, L }
function Get-SideButtonMessages([int]$X, [int]$Y, [switch]$Forward) {
  $which = if ($Forward) { 2 } else { 1 }
  $held = if ($Forward) { $script:MK.X2 } else { $script:MK.X1 }
  $l = [LbtermE2E.Win]::LParam($X, $Y).ToInt64()
  [pscustomobject]@{ Msg = [uint32]$script:WM.XDown; W = [long](($which -shl 16) -bor $held); L = $l }
  [pscustomobject]@{ Msg = [uint32]$script:WM.XUp; W = [long]($which -shl 16); L = $l }
}

# The messages Send-Click -Move posts (the pointer there, the left button pressed and let go) and
# with -Leave Send-Leave's after them, as Start-TimedPosts takes them
function Get-ClickMessages([int]$X, [int]$Y, [switch]$Leave) {
  $l = [LbtermE2E.Win]::LParam($X, $Y).ToInt64()
  [pscustomobject]@{ Msg = [uint32]$script:WM.Move; W = [long]0; L = $l }
  [pscustomobject]@{ Msg = [uint32]$script:WM.LDown; W = [long]1; L = $l }
  [pscustomobject]@{ Msg = [uint32]$script:WM.LUp; W = [long]0; L = $l }
  if ($Leave) { [pscustomobject]@{ Msg = [uint32]$script:WM.Leave; W = [long]0; L = [long]0 } }
}

# Posted messages on a clock of their own, off the script's thread, so a run of presses keeps its
# gaps whatever the captures beside it take (Get-Frames' -Each runs once a capture, 40 to 120 ms
# apart on a busy PC): -Posts is a list of @{ AtMs; Messages }, each post's messages (from
# Get-SideButtonMessages, Get-ClickMessages) posted to the window together when the clock reaches
# AtMs. Returns at once with the poster: .Done, .Wait(<ms>), .Sent (each post's ms on its clock,
# NaN until it went) and .StartUnixMs (when its clock started, as Get-UnixMs counts). Call
# Watch-Foreground once it's done: the posts can't. -Window may be [IntPtr]::Zero, which posts to
# the poster's own thread (the Pester tests). The first call compiles the poster (about 200 ms):
# with no posts it does only that, before a run that's timed.
function Start-TimedPosts {
  param([Parameter(Mandatory)][AllowNull()]$Window, [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Posts)
  if (-not ('LbtermE2E.TimedPosts' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
namespace LbtermE2E {
  public class TimedPosts {
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    readonly IntPtr h; readonly double[] at; readonly int[] counts; readonly uint[] msgs; readonly long[] ws, ls;
    public readonly double[] Sent;
    public double StartUnixMs { get; private set; }
    readonly ManualResetEventSlim started = new ManualResetEventSlim(false);
    readonly Thread thread;
    // Post i's counts[i] messages, in order through msgs, ws and ls, when the clock reaches at[i]
    public TimedPosts(IntPtr h, double[] at, int[] counts, uint[] msgs, long[] ws, long[] ls) {
      this.h = h; this.at = at; this.counts = counts; this.msgs = msgs; this.ws = ws; this.ls = ls;
      Sent = new double[at.Length];
      for (int i = 0; i < Sent.Length; i++) Sent[i] = double.NaN;
      thread = new Thread(Run) { IsBackground = true, Priority = ThreadPriority.Highest };
      thread.Start();
      started.Wait();
    }
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
    [DllImport("kernel32.dll")] static extern bool SetThreadPriority(IntPtr h, int priority);
    void Run() {
      // Time-critical (15), above every normal-priority thread whatever the process's class: in a
      // warden job (below-normal) ThreadPriority.Highest only matched normal programs outside it,
      // and beside other agents' flooding test windows a post went 190 to 250 ms late (twice on
      // 2026-11-01). It sleeps until 30 ms before each post and yields its core after, so at that
      // priority it holds one core for at most those 30 ms a post
      SetThreadPriority(GetCurrentThread(), 15);
      var clock = Stopwatch.StartNew();
      StartUnixMs = (DateTime.UtcNow - DateTime.UnixEpoch).TotalMilliseconds;
      started.Set();
      int m = 0;
      for (int i = 0; i < at.Length; i++) {
        // Sleep while there's time, which can overrun by a timer tick, then yield to the moment
        double left;
        while ((left = at[i] - clock.Elapsed.TotalMilliseconds) > 0) {
          if (left > 30) Thread.Sleep(1); else Thread.Yield();
        }
        for (int k = 0; k < counts[i]; k++, m++) PostMessage(h, msgs[m], (IntPtr)ws[m], (IntPtr)ls[m]);
        Sent[i] = clock.Elapsed.TotalMilliseconds;
      }
    }
    public bool Done { get { return !thread.IsAlive; } }
    public bool Wait(int ms) { return thread.Join(ms); }
  }
}
'@ -ReferencedAssemblies System.Threading, System.Threading.Thread, System.Runtime.InteropServices, System.Diagnostics.Process
  }
  $h = if ($null -eq $Window -or $Window -is [IntPtr]) { [IntPtr]$Window } else { Resolve-Handle $Window }
  $at = [Collections.Generic.List[double]]::new(); $counts = [Collections.Generic.List[int]]::new()
  $msgs = [Collections.Generic.List[uint32]]::new(); $ws = [Collections.Generic.List[long]]::new(); $ls = [Collections.Generic.List[long]]::new()
  foreach ($p in $Posts) {
    $m = @($p.Messages)
    $at.Add([double]$p.AtMs); $counts.Add($m.Count)
    foreach ($x in $m) { $msgs.Add([uint32]$x.Msg); $ws.Add([long]$x.W); $ls.Add([long]$x.L) }
  }
  [LbtermE2E.TimedPosts]::new($h, $at.ToArray(), $counts.ToArray(), $msgs.ToArray(), $ws.ToArray(), $ls.ToArray())
}

#endregion

#region Captures

# The window's client area as the GUI draws it (PrintWindow, which sees DirectComposition
# content), saved as <out>\<Name>.png with -Name. Throws when there's no window to capture.
function Get-Shot {
  param([Parameter(Position = 0)]$Window, [string]$Name)
  $h = Resolve-Handle $Window
  Send-HeldPointer
  $shot = [LbtermE2E.Win]::Capture($h)
  Watch-Foreground
  if (-not $shot) {
    $alive = if ($Window -and $Window.Pid) { if (Test-WindowAlive $Window) { 'still running' } else { 'gone' } } else { 'unknown' }
    throw "no window to capture (handle $h, the test window $alive)"
  }
  if ($Name) { Save-Shot $shot $Name }
  $shot
}

# Saves a capture in the output folder as <Name>.png
function Save-Shot([Parameter(Mandatory)]$Shot, [Parameter(Mandatory)][string]$Name) {
  $path = Join-Path (Get-Test).Out "$Name.png"
  $Shot.Save($path)
  Add-Capture $path
}

# Frames as fast as PrintWindow gives them for $Ms, each with when it was taken: { Ms, Shot }.
# -Each runs before every frame with the milliseconds so far (to post a move or start a command
# on time). -Raw keeps whole-window bitmaps and crops them later (Save-Frames), which is faster.
# -Until (param($w, $s): the window's state entry and the snapshot) ends the recording -TailMs
# after the first frame it's true of, $Ms at most: a fade recorded until the state file says it's
# over, or an interruption made a set way into an animation.
function Get-Frames {
  param(
    [Parameter(Position = 0)]$Window,
    [Parameter(Mandatory, Position = 1)][int]$Ms,
    [scriptblock]$Each,
    [switch]$Raw,
    [scriptblock]$Until,
    [int]$TailMs = 0
  )
  $h = Resolve-Handle $Window
  $win = if ($Until) { Resolve-Window $Window } else { $null }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $frames = [Collections.Generic.List[object]]::new()
  $end = $Ms
  $lastFrame = -1
  while ($clock.ElapsedMilliseconds -lt $end) {
    if ($Each) { & $Each $clock.Elapsed.TotalMilliseconds }
    Send-HeldPointer
    $shot = if ($Raw) { [LbtermE2E.Win]::CaptureWindow($h) } else { [LbtermE2E.Win]::Capture($h) }
    # After, not before: PrintWindow can block while the GUI thread is busy, and the picture is
    # of the moment it returns
    if ($shot) { $frames.Add([pscustomobject]@{ Ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 1); Shot = $shot }) }
    if ($Until -and $end -eq $Ms) {
      $s = Get-StateSnapshot $win.StateFile
      if (-not $s) { if ($clock.ElapsedMilliseconds -gt 5000) { throw (Get-NoStateMessage $win.StateFile) } }
      elseif ($s.pid -eq $win.Pid -and $s.frame -ne $lastFrame) {
        $lastFrame = $s.frame
        $w = Get-StateWindow $s -Hwnd ([long]$win.Handle)
        if ($w -and (& $Until $w $s)) { $end = [Math]::Min($Ms, $clock.ElapsedMilliseconds + $TailMs) }
      }
    }
  }
  Watch-Foreground
  , [object[]]$frames.ToArray()
}

# Frees the frames' bitmaps
function Remove-Frames($Frames) { foreach ($f in $Frames) { if ($f.Shot) { $f.Shot.Dispose() } } }

# Writes frames as frame000.png... and frames.csv (frame, ms) into $Dir, cropping raw ones to the
# window's client area
function Save-Frames {
  param([Parameter(Mandatory)]$Frames, [Parameter(Mandatory)][string]$Dir, $Window)
  $client = if ($Window) { [LbtermE2E.Win]::ClientInWindow((Resolve-Handle $Window)) } else { [System.Drawing.Rectangle]::Empty }
  $lines = [Collections.Generic.List[string]]::new(); $lines.Add('frame,ms')
  for ($i = 0; $i -lt $Frames.Count; $i++) {
    $bmp = $Frames[$i].Shot
    $path = Join-Path $Dir ('frame{0:D3}.png' -f $i)
    if (-not $client.IsEmpty -and ($bmp.Width -ne $client.Width -or $bmp.Height -ne $client.Height)) {
      $crop = $bmp.Clone($client, $bmp.PixelFormat); $crop.Save($path); $crop.Dispose()
    } else { $bmp.Save($path) }
    $lines.Add("$i,$([Math]::Round([double]$Frames[$i].Ms, 1))")
  }
  $lines | Set-Content (Join-Path $Dir 'frames.csv')
}

# Frames' $Area one under another, -Zoom times, each labelled with its time: <out>\<Name>.png
function Save-Sheet {
  param([Parameter(Mandatory)]$Frames, [Parameter(Mandatory)][System.Drawing.Rectangle]$Area, [Parameter(Mandatory)][string]$Name, [int]$Zoom = 3)
  $label = 70
  $sheet = [System.Drawing.Bitmap]::new($Area.Width * $Zoom + $label, [Math]::Max(1, $Area.Height * $Zoom * $Frames.Count))
  $g = [System.Drawing.Graphics]::FromImage($sheet)
  $g.InterpolationMode = 'NearestNeighbor'; $g.PixelOffsetMode = 'Half'; $g.Clear([System.Drawing.Color]::Black)
  $font = [System.Drawing.Font]::new('Consolas', 10)
  for ($i = 0; $i -lt $Frames.Count; $i++) {
    $y = $i * $Area.Height * $Zoom
    $g.DrawString(('{0:N0} ms' -f $Frames[$i].Ms), $font, [System.Drawing.Brushes]::White, 2, $y + 2)
    $g.DrawImage($Frames[$i].Shot, [System.Drawing.Rectangle]::new($label, $y, $Area.Width * $Zoom, $Area.Height * $Zoom), $Area, 'Pixel')
  }
  $g.Dispose(); $font.Dispose()
  Save-Shot $sheet $Name
  $sheet.Dispose()
}

# A region of a shot -Zoom times (-ZoomY times down, if that's different), nearest neighbour:
# <out>\<Name>.png
function Save-Zoomed {
  param([Parameter(Mandatory)]$Shot, [Parameter(Mandatory)][System.Drawing.Rectangle]$Area, [Parameter(Mandatory)][string]$Name, [int]$Zoom = 3, [int]$ZoomY = 0)
  if (-not $ZoomY) { $ZoomY = $Zoom }
  $big = [System.Drawing.Bitmap]::new($Area.Width * $Zoom, $Area.Height * $ZoomY)
  $g = [System.Drawing.Graphics]::FromImage($big)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
  $g.DrawImage($Shot, [System.Drawing.Rectangle]::new(0, 0, $big.Width, $big.Height), $Area, [System.Drawing.GraphicsUnit]::Pixel)
  $g.Dispose()
  Save-Shot $big $Name
  $big.Dispose()
}

# A shot once two taken -IntervalMs apart match over -Region (or -Columns x0,x1 or -Rows y0,y1,
# inclusive; all of it by default, and at the same size): what was asked for has been drawn.
# Pixels match exactly, or with -Lum by brightness alone. Saved as -Name. After -Tries without
# that it throws, or with -NoThrow gives the last shot. -MinTries asks for that many comparisons
# first.
function Wait-Still {
  param(
    [Parameter(Position = 0)]$Window,
    [System.Drawing.Rectangle]$Region = [System.Drawing.Rectangle]::Empty,
    [int[]]$Columns,
    [int[]]$Rows,
    [int]$IntervalMs = 700,
    [int]$Tries = 15,
    [int]$MinTries = 0,
    [string]$Name,
    [switch]$Lum,
    [switch]$NoThrow
  )
  $shot = Get-Shot $Window
  $moving = $null
  for ($i = 0; $i -lt $Tries; $i++) {
    Wait-Ms $IntervalMs
    $again = Get-Shot $Window
    $r = if ($Columns) { Get-Columns $Columns[0] $Columns[1] -Of $again }
    elseif ($Rows) { Get-Rows $Rows[0] $Rows[1] -Of $again }
    elseif ($Region.IsEmpty) { [LbtermE2E.Px]::All($again) } else { $Region }
    $moving = if ($Lum) {
      $d = [LbtermE2E.Px]::LumDistance($shot, $again, $r)
      if ($d -ne 0) { , @($r.Left, $r.Top, ($r.Right - 1), ($r.Bottom - 1)) }
    } else { [LbtermE2E.Px]::DiffBox($shot, $again, $r) }
    $shot.Dispose(); $shot = $again
    if (-not $moving -and $i + 1 -ge $MinTries) { break }
  }
  if ($moving -and -not $NoThrow) {
    throw "$(if ($Name) { "$Name " })never held still: $(Format-Box $moving) kept changing"
  }
  if ($Name) { Save-Shot $shot $Name }
  $shot
}

# The box round what differs between two shots (inside -Region), or nothing
function Get-DiffBox([Parameter(Mandatory)]$A, [Parameter(Mandatory)]$B, [System.Drawing.Rectangle]$Region = [System.Drawing.Rectangle]::Empty) {
  $r = if ($Region.IsEmpty) { [LbtermE2E.Px]::All($A) } else { $Region }
  $box = [LbtermE2E.Px]::DiffBox($A, $B, $r)
  if ($box) { , $box }
}

# A colour range as Px's probes take it: each channel's low and high, inclusive
function New-ColourRange([int]$RLo, [int]$RHi, [int]$GLo, [int]$GHi, [int]$BLo, [int]$BHi) { [int[]]@($RLo, $RHi, $GLo, $GHi, $BLo, $BHi) }

#endregion

#region The state file (lbterm/STATE.md)

# What a GUI with LBTERM_STATE_FILE set writes after each frame it paints (lbterm/STATE.md):
# version 1, pid, build, frame (only goes up), time_ms and windows, each with its hwnd, frame,
# animating, animations and waits by name, settled, pointer, tabs, panes, rail, strip, menu and the
# rest, rectangles as {x, y, w, h} in client pixels. These read it and wait on it.

$script:StateVersion = 1

function Resolve-StatePath($Window, [string]$Path) {
  if ($Path) { return $Path }
  (Resolve-Window $Window).StateFile
}

function Get-NoStateMessage([string]$Path) {
  $bin = if ($script:Test) { $script:Test.Bin } else { 'this build' }
  "no state file at ${Path}: the build in $bin doesn't write LBTERM_STATE_FILE (it predates the state file, lbterm/STATE.md), so state waits can't run against it"
}

# The state file, parsed; nothing when it isn't there or is caught unreadable. Opened shared for
# read, write and delete, so the GUI's rename over it is never held up by this reading it
# (lbterm/STATE.md: a reader without FILE_SHARE_DELETE blocks the writer).
function Get-StateSnapshot([Parameter(Mandatory)][string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  try {
    $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
    try { $text = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
    if (-not $text.Trim()) { return $null }
    $s = $text | ConvertFrom-Json -Depth 32
  } catch { return $null }
  if ($s.version -ne $script:StateVersion) { throw "the state file at $Path is version $($s.version); the harness reads version $script:StateVersion" }
  $s
}

# The test window's snapshot (-Window, or the current one; or the file at -Path). Throws when
# there's none: the build doesn't write it.
function Read-State([object]$Window, [string]$Path) {
  $p = Resolve-StatePath $Window $Path
  $s = Get-StateSnapshot $p
  if (-not $s) {
    # Caught unreadable, perhaps: once more
    Start-Sleep -Milliseconds 50
    $s = Get-StateSnapshot $p
  }
  if (-not $s) { throw (Get-NoStateMessage $p) }
  $s
}

# A window's entry in a snapshot: by -Hwnd, else by -WindowId, else the first
function Get-StateWindow([Parameter(Mandatory)]$State, [long]$Hwnd = 0, [object]$WindowId) {
  $ws = @($State.windows)
  if ($Hwnd) { return @($ws | Where-Object { [long]$_.hwnd -eq $Hwnd })[0] }
  if ($null -ne $WindowId) { return @($ws | Where-Object { $_.id -eq $WindowId })[0] }
  $ws[0]
}

# Whether a window's entry has settled: `settled` (nothing animating and no waits under way), or
# with -Animating only nothing animating (a notice or a landed mark may still be waiting to go)
function Test-StateSettled([Parameter(Mandatory)][AllowNull()]$StateWindow, [switch]$Animating) {
  if (-not $StateWindow) { return $false }
  if ($Animating) { return -not $StateWindow.animating }
  if ($null -ne $StateWindow.settled) { return [bool]$StateWindow.settled }
  -not $StateWindow.animating
}

# What a window's entry says, for messages: its frame, and what's animating and waiting
function Format-StateWindow($StateWindow) {
  if (-not $StateWindow) { return 'nothing from this window' }
  $anim = @($StateWindow.animations | ForEach-Object { $_.name }) -join ', '
  $waits = @($StateWindow.waits | ForEach-Object { "$($_.name) $($_.left_ms) ms" }) -join ', '
  "frame $($StateWindow.frame), animating $($StateWindow.animating)$(if ($anim) { " ($anim)" }), settled $($StateWindow.settled)$(if ($waits) { ", waiting on $waits" })"
}

# The middle of a state rectangle ({x, y, w, h}), as client coordinates to post a pointer to
function Get-RectCenter([Parameter(Mandatory)]$Rect) { @([int]($Rect.x + $Rect.w / 2), [int]($Rect.y + $Rect.h / 2)) }

# The rail's animations, by the names the compositor gives them (compositor.rs's quoted `rail.` and
# `light.` names, every one of them, which harness.Tests.ps1 holds this list to), sorted by whether
# nothing of it may play under a menu's card (Lachlan's 7b of 2026-11-08; FLOWS.md, "A menu, from
# what opens it to the end of its fade", step 10): Held, what a test judges, each animation name
# the hold drives (the rows' and the marker's and the progress fills', held by
# lbterm_motion::rail::Hold, and the panel's edge, light, words and sweep and a pad's glint, held
# by the hover through the same cards); Exempt, each with why it may. A test that judges the hold
# takes Held whole, never a hand-picked few (PITFALLS 210)
function Get-RailUnderCard {
  [pscustomobject]@{
    Held = @(
      'rail.edge', 'rail.light', 'rail.words', 'rail.sweep'
      'rail.row.enter', 'rail.row.exit', 'rail.row.grow', 'rail.row.hover', 'rail.row.slide', 'rail.row.swap'
      'rail.marker', 'rail.progress', 'light.glint', 'light.send'
    )
    Exempt = [ordered]@{
      'rail.wave'       = "the wave stops and fades out as a card comes over it, by design (PITFALLS 187)"
      'rail.spinner'    = "a loop: a hold keeps what the tree shows, so one already turning turns on, and a new one waits"
      'rail.starting'   = "a loop, as rail.spinner"
      'rail.tasks'      = "the tasks card, a card of its own"
      'rail.plus.hover' = "the + pad's hover card, a card of its own"
      'light.press'     = "a press's light where the pointer pressed, not the rail's"
      'light.copy'      = "copied text's light in a pane, not the rail's"
    }
  }
}

# A watcher for what plays under a menu's card: $w.Watch($state) on every frame read (a wait's
# -OnState), and $w.Judge('what') once after, which gives the line to print, or fails the test
# naming each frame that had one of `$Cards` (menu.tab, menu.list) beside a held name
# (Get-RailUnderCard), each frame once. The menu must have settled before what's judged, so the
# card's name is its fade out alone (PITFALLS 194). Methods, not closures, so Judge's Add-Failure
# is the module's wherever the harness was imported
function New-UnderCardWatch([string[]]$Cards = @('menu.tab', 'menu.list')) {
  $w = [pscustomobject]@{ Cards = $Cards; Held = (Get-RailUnderCard).Held; Seen = [System.Collections.Generic.List[string]]::new() }
  $w | Add-Member -MemberType ScriptMethod -Name Watch -Value {
    param($s)
    $a = @($s.animations | ForEach-Object name)
    $card = @($a | Where-Object { $_ -in $this.Cards })
    if ($card.Count) {
      $both = @($a | Where-Object { $_ -in $this.Held })
      # Each frame once: a wait reads the frame it ends on again as it settles
      $entry = "frame $($s.frame): $($both -join ', ') beside $($card -join ', ')"
      if ($both.Count -and -not $this.Seen.Contains($entry)) { $this.Seen.Add($entry) }
    }
  }
  $w | Add-Member -MemberType ScriptMethod -Name Judge -Value {
    param([string]$What)
    $line = if ($this.Seen.Count) { Add-Failure "${What}: the rail moved under the menu's card fading out: $($this.Seen -join '; ')" }
    else { "${What}: nothing of the rail moved under the menu's card (7b)" }
    $this.Seen.Clear()
    $line
  }
  $w
}

# A state rectangle as a System.Drawing.Rectangle, for the pixel probes
function ConvertTo-Rect([Parameter(Mandatory)]$Rect) { [System.Drawing.Rectangle]::new([int]$Rect.x, [int]$Rect.y, [int]$Rect.w, [int]$Rect.h) }

# The test window's entry in the first snapshot -Until (param($w, $s): the window's entry and the
# whole snapshot) is true of, read every -IntervalMs as frames are painted, within -TimeoutSec.
# The window is found by its HWND (-Window, or the current one), or with -Path by -WindowId or
# first. -AfterFrame takes only a snapshot of a later frame of the window than that (so not one
# painted before an action took effect). A snapshot with another pid than the window's is an
# earlier run's and isn't taken. -OnState sees each new snapshot first (to log it). A held
# pointer (Set-PointerHold) is kept where it is at every read. A snapshot is written before its
# frame is presented, so a capture straight after it can still show the frame before (or nothing,
# for a window's first): it returns once the snapshot is -PresentMs old (two refreshes at 60 Hz),
# or 400 ms for a window's first two frames. Throws on timeout with what the window last said
# (-What names what was awaited), or when no state file appears within -NoFileSec: the build
# doesn't write one.
function Wait-State {
  param(
    [Parameter(Mandatory, Position = 0)][scriptblock]$Until,
    [object]$Window,
    [string]$Path,
    [object]$WindowId,
    [string]$What = 'the state asked for',
    [double]$TimeoutSec = 15,
    [long]$AfterFrame = -1,
    [scriptblock]$OnState,
    [double]$NoFileSec = 5,
    [int]$IntervalMs = 10,
    [int]$PresentMs = 34
  )
  $p = Resolve-StatePath $Window $Path
  $win = if (-not $Path) { Resolve-Window $Window } else { $null }
  $wantPid = if ($win) { $win.Pid } else { $null }
  $hwnd = if ($win) { [long]$win.Handle } else { 0 }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $last = $null
  $seen = $false
  $lastFrame = -1
  $nextAlive = 500
  while ($true) {
    Send-HeldPointer
    $s = Get-StateSnapshot $p
    if ($s) {
      $seen = $true
      if ($null -eq $wantPid -or $s.pid -eq $wantPid) {
        $w = Get-StateWindow $s -Hwnd $hwnd -WindowId $WindowId
        if ($w -and $s.frame -ne $lastFrame) {
          $lastFrame = $s.frame
          $last = $w
          if ($OnState) { & $OnState $w $s }
          if ($w.frame -gt $AfterFrame -and (& $Until $w $s)) {
            if ($script:Test) { $script:Test.LastState = $s }
            Wait-Presented $s $w $PresentMs
            return $w
          }
        }
      }
    }
    if (-not $seen -and $clock.Elapsed.TotalSeconds -ge [Math]::Min($NoFileSec, $TimeoutSec)) {
      # A window that has died wrote nothing: that, not an old build
      if ($win -and -not (Test-WindowAlive $win)) { throw "the test window (pid $($win.Pid)) ended before it wrote its state file; its log: $($win.Log)" }
      throw (Get-NoStateMessage $p)
    }
    # Every half second, whether the window is still there to say anything
    if ($win -and $clock.ElapsedMilliseconds -ge $nextAlive) {
      $nextAlive = $clock.ElapsedMilliseconds + 500
      if (-not (Test-WindowAlive $win)) { throw "the test window (pid $($win.Pid)) ended while waiting for ${What}; its log: $($win.Log)" }
    }
    if ($clock.Elapsed.TotalSeconds -ge $TimeoutSec) { break }
    Start-Sleep -Milliseconds $IntervalMs
  }
  # No file all along: said as such, though the time ran out between the look for it above and
  # this (with a timeout as short as -NoFileSec it can: the harness's own test caught it once)
  if (-not $seen) {
    if ($win -and -not (Test-WindowAlive $win)) { throw "the test window (pid $($win.Pid)) ended before it wrote its state file; its log: $($win.Log)" }
    throw (Get-NoStateMessage $p)
  }
  throw "timed out after $TimeoutSec s waiting for ${What}: the window last said $(Format-StateWindow $last)"
}

# Sleeps until the snapshot's frame has had time to reach the screen: -PresentMs after it was
# taken, 400 ms for a window's first two frames (the first present takes longer)
function Wait-Presented($State, $StateWindow, [int]$PresentMs = 34) {
  if ($PresentMs -le 0 -or -not $State.time_ms) { return }
  $age = if ($StateWindow.frame -le 2) { [Math]::Max(400, $PresentMs) } else { $PresentMs }
  $left = [long]$State.time_ms + $age - [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  if ($left -gt 0) { Start-Sleep -Milliseconds ([Math]::Min($left, $age)) }
}

# The window's entry once it has settled (nothing animating and no waits under way; with
# -Animating only nothing animating), on a frame after -AfterFrame
function Wait-Settled {
  param([object]$Window, [string]$Path, [object]$WindowId, [double]$TimeoutSec = 15, [long]$AfterFrame = -1, [switch]$Animating, [scriptblock]$OnState, [string]$What = 'the window to settle')
  $settleAnimating = [bool]$Animating
  Wait-State { param($sw) Test-StateSettled $sw -Animating:$settleAnimating } -Window $Window -Path $Path -WindowId $WindowId -TimeoutSec $TimeoutSec -AfterFrame $AfterFrame -OnState $OnState -What $What
}

# The window's entry once -Until is true of it (what an action does) and then it has settled on a
# later frame: STATE.md's "wait on a change, then on settling", since a frame painted before the
# action took effect can say settled too
function Wait-Change {
  param(
    [Parameter(Mandatory, Position = 0)][scriptblock]$Until,
    [object]$Window,
    [string]$Path,
    [string]$What = 'the change asked for',
    [double]$TimeoutSec = 15,
    [long]$AfterFrame = -1,
    [switch]$Animating,
    [scriptblock]$OnState
  )
  $w = Wait-State $Until -Window $Window -Path $Path -What $What -TimeoutSec $TimeoutSec -AfterFrame $AfterFrame -OnState $OnState
  if (Test-StateSettled $w -Animating:$Animating) { return $w }
  # Named apart from Wait-State's own parameters: the block below runs inside it, where $Until is itself
  $changeShown = $Until; $changeAnimating = [bool]$Animating
  Wait-State { param($cw, $cs) (& $changeShown $cw $cs) -and (Test-StateSettled $cw -Animating:$changeAnimating) } -Window $Window -Path $Path -What "$What, settled" -TimeoutSec $TimeoutSec -AfterFrame $w.frame -OnState $OnState
}

# The window's frame now, to wait for a later one after an action: Wait-State -AfterFrame
function Get-StateFrame([object]$Window) {
  $win = Resolve-Window $Window
  $s = Read-State -Window $win
  $w = Get-StateWindow $s -Hwnd ([long]$win.Handle)
  if ($w) { [long]$w.frame } else { -1 }
}

#endregion

#region The counters (lbterm/PERF.md)

# The window's counters: its control pipe's `metrics` reply (lbterm/CONTROL.md), parsed. Answered
# on the pipe's own thread, so asking adds nothing to what the GUI thread does. -Reset starts the
# counts again after this reply, so the next one covers what happened in between. Throws when the
# window doesn't answer, or says no (a build from before metrics: "no op called").
function Get-Metrics([object]$Window, [switch]$Reset, [int]$TimeoutMs = 8000) {
  $w = Resolve-Window $Window
  $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', "lbterm-control-$($w.Class)", [IO.Pipes.PipeDirection]::InOut)
  try {
    $pipe.Connect($TimeoutMs)
    $writer = [IO.StreamWriter]::new($pipe, [Text.UTF8Encoding]::new($false))
    $writer.AutoFlush = $true
    $reader = [IO.StreamReader]::new($pipe, [Text.UTF8Encoding]::new($false))
    $writer.WriteLine((ConvertTo-Json -Compress @{ id = 'metrics'; op = 'metrics'; reset = [bool]$Reset }))
    $task = $reader.ReadLineAsync()
    if (-not $task.Wait($TimeoutMs)) { throw "no metrics from the window (pid $($w.Pid)) in $TimeoutMs ms" }
    $reply = $task.Result | ConvertFrom-Json -Depth 16
  } finally { $pipe.Dispose() }
  if (-not $reply.ok) { throw "the window (pid $($w.Pid)) refused metrics: $($reply.error)" }
  $reply
}

# What a metrics reply says, in a few lines: the totals, then the -Top of each list
function Format-Metrics([Parameter(Mandatory)]$Metrics, [int]$Top = 3) {
  $m = $Metrics
  $t = $m.totals
  "$($m.span_ms) ms: $($t.frames) frames ($($t.unasked) unasked), $($t.requests) requests, $($t.tasks) tasks ($($t.task_ms) ms), $($t.messages) messages ($($t.message_ms) ms), $($t.paints) paint parts ($($t.paint_ms) ms), $($t.timers_fired) timers fired, $($t.wakes) wakes; CPU $($m.process.cpu_ms) ms, the GUI thread's $($m.process.gui_thread_ms) ms"
  foreach ($r in @($m.requests | Select-Object -First $Top)) { "  asked by $($r.site): $($r.frames) frames, $($r.requests) requests" }
  foreach ($x in @($m.tasks | Select-Object -First $Top)) { "  task $($x.site): $($x.count) runs, $($x.total_ms) ms, the longest $($x.max_ms) ms" }
  foreach ($x in @($m.messages | Select-Object -First $Top)) { "  message $($x.site): $($x.count), $($x.total_ms) ms" }
  foreach ($x in @($m.timers | Select-Object -First $Top)) { "  timer $($x.site): $($x.fired) fired of $($x.armed) armed" }
  foreach ($x in @($m.mux | Select-Object -First $Top)) { "  pdu $($x.pdu): $($x.sent) sent ($($x.sent_bytes) B), $($x.received) received ($($x.received_bytes) B)" }
  foreach ($x in @($m.threads | Select-Object -First $Top)) { "  thread $(if ($x.name) { $x.name } else { '(unnamed)' }) $($x.tid): $($x.cpu_ms) ms" }
}

# A list of a metrics reply (tasks, messages, requests, timers, mux) as a table by site (by pdu
# for mux): what to compare one reply's entry with another's
function ConvertTo-MetricsTable($List, [string]$Key = 'site') {
  $table = @{}
  foreach ($e in @($List)) { if ($null -ne $e) { $table[[string]$e.$Key] = $e } }
  $table
}

# The GUI process's CPU and its GUI thread's (the process's first), in milliseconds, as Windows
# counts them: what a build without metrics can be measured by
function Get-GuiCpu([Parameter(Mandatory)]$Window) {
  $w = Resolve-Window $Window
  $p = Get-Process -Id $w.Pid -ErrorAction Stop
  $main = @($p.Threads | Sort-Object { $_.StartTime } | Select-Object -First 1)[0]
  [pscustomobject]@{ Process = $p.TotalProcessorTime.TotalMilliseconds; Thread = $main.TotalProcessorTime.TotalMilliseconds }
}

# A Lua event run in a test window, as a key bound to EmitEvent would run it: the control pipe's
# `emit` (lbterm/CONTROL.md), through `lbterm control --class <its class> emit`
function Send-Emit([Parameter(Mandatory, Position = 0)][string]$Event, [object]$Window) {
  $t = Get-Test
  $w = Resolve-Window $Window
  $out = @(& $t.Cli control --class $w.Class emit $Event 2>&1)
  if ($LASTEXITCODE) { throw "emit ${Event}: $($out -join ' ')" }
}

# The process's frames as the state file counts them (every window's), once nothing has painted for
# -QuietMs; throws when it keeps painting for -TimeoutSec
function Get-QuietFrame([int]$QuietMs = 700, [double]$TimeoutSec = 10, [object]$Window) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $frame = (Read-State -Window $Window).frame
  $since = [Diagnostics.Stopwatch]::StartNew()
  while ($since.ElapsedMilliseconds -lt $QuietMs) {
    if ($clock.Elapsed.TotalSeconds -gt $TimeoutSec) { throw "the window kept painting for $TimeoutSec s (frame $frame)" }
    Start-Sleep -Milliseconds 50
    $now = (Read-State -Window $Window).frame
    if ($now -ne $frame) { $frame = $now; $since.Restart() }
  }
  [long]$frame
}

#endregion

#region What the terminal costs: CPU by cycles, its processes (lbterm/PERF.md, "The budget runner")

# Windows counts a process's and a thread's CPU time in clock ticks of 15.625 ms, so 5 s at rest
# reads 0, 15.6 or 31.3 ms: a whole tick for a thread that ran a microsecond across one. Their
# cycles (QueryProcessCycleTime, QueryThreadCycleTime) are exact: the time-stamp counter's ticks
# while each thread ran, kernel mode included, which count at the processor's nominal rate whatever
# its clock does. And ShowWindow, for a window minimized without anything being activated.
# Compiled on first use: only the measuring needs it. (A method named Process can't be called from
# PowerShell: its binder refuses the Int64 it returns.)
function Initialize-CpuReader {
  if ('LbtermE2E.Cpu' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
namespace LbtermE2E {
  public static class Cpu {
    const uint ProcessQuery = 0x1000, ThreadQuery = 0x0800;
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
    [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong cycles);
    [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong cycles);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr h, out IntPtr desc);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
    [DllImport("ntdll.dll")] static extern int NtQueryInformationThread(IntPtr h, int cls, out IntPtr info, int len, IntPtr retLen);
    // A process's cycles, every thread's it ever ran; -1 when it's gone
    public static long ProcessCycles(uint pid) {
      IntPtr h = OpenProcess(ProcessQuery, false, pid);
      if (h == IntPtr.Zero) return -1;
      try { ulong c; return QueryProcessCycleTime(h, out c) ? (long)c : -1; } finally { CloseHandle(h); }
    }
    // A thread's cycles; -1 when it's gone
    public static long ThreadCycles(uint tid) {
      IntPtr h = OpenThread(ThreadQuery, false, tid);
      if (h == IntPtr.Zero) return -1;
      try { ulong c; return QueryThreadCycleTime(h, out c) ? (long)c : -1; } finally { CloseHandle(h); }
    }
    // A thread's name as its program gave it (Rust names its threads), "" for none
    public static string ThreadName(uint tid) {
      IntPtr h = OpenThread(ThreadQuery, false, tid);
      if (h == IntPtr.Zero) return "";
      try {
        IntPtr p;
        if (GetThreadDescription(h, out p) < 0 || p == IntPtr.Zero) return "";
        try { return Marshal.PtrToStringUni(p) ?? ""; } finally { LocalFree(p); }
      } finally { CloseHandle(h); }
    }
    // Where a thread started (ThreadQuerySetWin32StartAddress), 0 when it can't be read
    public static long StartAddress(uint tid) {
      IntPtr h = OpenThread(0x0040, false, tid);
      if (h == IntPtr.Zero) return 0;
      try { IntPtr a; return NtQueryInformationThread(h, 9, out a, IntPtr.Size, IntPtr.Zero) == 0 ? a.ToInt64() : 0; } finally { CloseHandle(h); }
    }
  }
  public static class Show {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  }
}
'@
}

# The time-stamp counter's ticks a millisecond: the processor's nominal MHz as Windows measured it
# at boot (~MHz, 3000 on this PC's i9-9980XE), times 1000. Cycles over this are milliseconds of CPU.
function Get-CycleRate {
  $mhz = Get-ItemPropertyValue -Path 'HKLM:\HARDWARE\DESCRIPTION\System\CentralProcessor\0' -Name '~MHz'
  if (-not ($mhz -gt 0)) { throw "the processor's ~MHz reads '$mhz'" }
  [double]$mhz * 1000
}

# A test window's GUI thread: its process's first thread, the one main runs on, by id
function Get-GuiThreadId([Parameter(Mandatory)]$Window) {
  $w = Resolve-Window $Window
  if ($w.PSObject.Properties['GuiThread'] -and $w.GuiThread) { return $w.GuiThread }
  $p = Get-Process -Id $w.Pid -ErrorAction Stop
  $tid = [int](@($p.Threads | Sort-Object { $_.StartTime } | Select-Object -First 1)[0].Id)
  $w | Add-Member -NotePropertyName GuiThread -NotePropertyValue $tid -Force
  $tid
}

# The conpty hosts (OpenConsole.exe) a GUI or a mux server started, one a pane: part of what the
# terminal costs, though they're processes of their own
function Get-ConptyIds([int[]]$Parents = @()) {
  if (-not $Parents) { return @() }
  @(Get-CimInstance Win32_Process -Filter "Name = 'OpenConsole.exe'" -ErrorAction SilentlyContinue |
    Where-Object { [int]$_.ParentProcessId -in $Parents } | ForEach-Object { [int]$_.ProcessId })
}

# A thread of a test window's process by name: the one its program gave it, else the module it
# started in, in brackets ("(nvwgf2umx.dll)", "(ntdll.dll)" for a thread-pool worker's), else "".
# Kept on the window by id, so each thread is looked up once.
function Get-ThreadLabel([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][int]$Tid, $Process) {
  $w = Resolve-Window $Window
  if (-not ($w.PSObject.Properties['ThreadLabels'] -and $w.ThreadLabels)) { $w | Add-Member -NotePropertyName ThreadLabels -NotePropertyValue @{} -Force }
  if ($w.ThreadLabels.ContainsKey($Tid)) { return $w.ThreadLabels[$Tid] }
  $label = [LbtermE2E.Cpu]::ThreadName([uint32]$Tid)
  if (-not $label) {
    $a = [LbtermE2E.Cpu]::StartAddress([uint32]$Tid)
    if ($a) {
      if (-not ($w.PSObject.Properties['Modules'] -and $w.Modules)) {
        $p = if ($Process) { $Process } else { Get-Process -Id $w.Pid -ErrorAction Stop }
        $mods = @($p.Modules | ForEach-Object { [pscustomobject]@{ Name = $_.ModuleName; Base = $_.BaseAddress.ToInt64(); End = $_.BaseAddress.ToInt64() + $_.ModuleMemorySize } })
        $w | Add-Member -NotePropertyName Modules -NotePropertyValue $mods -Force
      }
      $m = @($w.Modules | Where-Object { $a -ge $_.Base -and $a -lt $_.End })[0]
      if ($m) { $label = "($($m.Name))" }
    }
  }
  $w.ThreadLabels[$Tid] = [string]$label
  [string]$label
}

# Every thread of each of the processes -Pids (a test's mux servers) with its cycles so far and its
# name: the one its program gave it ("main" for a Rust program's first), else the module it started
# in, in brackets, else "". Names and modules kept by thread id, so each is looked up once.
function Get-ServerThreadCycles([int[]]$Pids = @()) {
  Initialize-CpuReader
  if (-not $script:ServerThreadNames) { $script:ServerThreadNames = @{}; $script:ServerModules = @{} }
  foreach ($id in @($Pids)) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if (-not $p) { continue }
    foreach ($t in @($p.Threads)) {
      $tid = [int]$t.Id
      $key = "$id/$tid"
      if (-not $script:ServerThreadNames.ContainsKey($key)) {
        $label = [LbtermE2E.Cpu]::ThreadName([uint32]$tid)
        if (-not $label) {
          $a = [LbtermE2E.Cpu]::StartAddress([uint32]$tid)
          if ($a) {
            if (-not $script:ServerModules.ContainsKey($id)) {
              $script:ServerModules[$id] = @(try { $p.Modules | ForEach-Object { [pscustomobject]@{ Name = $_.ModuleName; Base = $_.BaseAddress.ToInt64(); End = $_.BaseAddress.ToInt64() + $_.ModuleMemorySize } } } catch { })
            }
            $m = @($script:ServerModules[$id] | Where-Object { $a -ge $_.Base -and $a -lt $_.End })[0]
            if ($m) { $label = "($($m.Name))" }
          }
        }
        $script:ServerThreadNames[$key] = [string]$label
      }
      [pscustomobject]@{ Pid = $id; Tid = $tid; Name = $script:ServerThreadNames[$key]; Cycles = [LbtermE2E.Cpu]::ThreadCycles([uint32]$tid) }
    }
  }
}

# What a test window's terminal has spent so far: when (the stopwatch's timestamp), the GUI
# process's cycles and its GUI thread's, every one of its threads' with its name (ThreadList), Windows'
# own CPU times for the process and the GUI thread (ms, in its 15.625 ms ticks), its memory, handles
# and threads, and the cycles of each of -Servers (its mux servers, and each of their threads,
# ServerThreads) and -Conpty (conpty hosts), by pid, -1 for one that's gone. Two of these make a
# span (ConvertTo-RestSample in budgets.psm1).
function Get-CpuSnapshot {
  param([Parameter(Mandatory)]$Window, [int[]]$Servers = @(), [int[]]$Conpty = @())
  Initialize-CpuReader
  $w = Resolve-Window $Window
  $tid = Get-GuiThreadId $w
  $p = Get-Process -Id $w.Pid -ErrorAction Stop
  $thread = @($p.Threads | Where-Object Id -eq $tid)[0]
  $of = { param($ids) @($ids | ForEach-Object { [pscustomobject]@{ Pid = $_; Cycles = [LbtermE2E.Cpu]::ProcessCycles([uint32]$_) } }) }
  [pscustomobject]@{
    At           = [Diagnostics.Stopwatch]::GetTimestamp()
    Cycles       = [LbtermE2E.Cpu]::ProcessCycles([uint32]$w.Pid)
    ThreadCycles = [LbtermE2E.Cpu]::ThreadCycles([uint32]$tid)
    CpuMs        = $p.TotalProcessorTime.TotalMilliseconds
    ThreadMs     = $(if ($thread) { $thread.TotalProcessorTime.TotalMilliseconds } else { $null })
    PrivateBytes = $p.PrivateMemorySize64
    Handles      = $p.HandleCount
    Threads      = $p.Threads.Count
    ThreadList   = @($p.Threads | ForEach-Object { [pscustomobject]@{ Tid = [int]$_.Id; Name = (Get-ThreadLabel $w $_.Id $p); Cycles = [LbtermE2E.Cpu]::ThreadCycles([uint32]$_.Id) } })
    Servers      = & $of $Servers
    ServerThreads = @(Get-ServerThreadCycles $Servers)
    Conpty       = & $of $Conpty
  }
}

# What the rest of the PC did while something was measured (lbterm/PERF.md, "The PC while it
# measured"). Each logical processor's busy time so far (NtQuerySystemInformation's processor
# performance: kernel time, which counts idle, less idle, plus user; in 100 ns), counted for every
# processor whatever this process's affinity (Environment.ProcessorCount counts only the job's).
# And each one's clock over a span, as Windows' "% Processor Performance" says it (Task Manager's
# speed): a share of the nominal clock, above 100 with turbo, below it when it's slowed, read through
# PDH, whose rate counter averages it between two collections: Start opens a query and collects,
# Read collects again, reads every processor's and closes it.
function Initialize-PcReader {
  if ('LbtermE2E.Pc' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
namespace LbtermE2E {
  public static class Pc {
    [StructLayout(LayoutKind.Sequential)] struct ProcessorTimes { public long Idle, Kernel, User, Dpc, Interrupt; public uint Interrupts; }
    [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int cls, [Out] ProcessorTimes[] buf, int len, out int ret);
    public static long[] Busy() {
      var a = new ProcessorTimes[256];
      int size = Marshal.SizeOf(typeof(ProcessorTimes)), ret;
      int st = NtQuerySystemInformation(8, a, size * a.Length, out ret);
      if (st != 0) throw new Exception("NtQuerySystemInformation(SystemProcessorPerformanceInformation): 0x" + st.ToString("x"));
      var busy = new long[ret / size];
      for (int i = 0; i < busy.Length; i++) busy[i] = a[i].Kernel - a[i].Idle + a[i].User;
      return busy;
    }
    [DllImport("pdh.dll", CharSet = CharSet.Unicode)] static extern uint PdhOpenQuery(string source, IntPtr user, out IntPtr query);
    [DllImport("pdh.dll", CharSet = CharSet.Unicode)] static extern uint PdhAddEnglishCounter(IntPtr query, string path, IntPtr user, out IntPtr counter);
    [DllImport("pdh.dll")] static extern uint PdhCollectQueryData(IntPtr query);
    [DllImport("pdh.dll", CharSet = CharSet.Unicode)] static extern uint PdhGetFormattedCounterArray(IntPtr counter, uint format, ref uint size, out uint count, IntPtr items);
    [DllImport("pdh.dll")] static extern uint PdhCloseQuery(IntPtr query);
    const uint Double = 0x200, NoCap100 = 0x8000, MoreData = 0x800007D2;
    public sealed class Clock { internal IntPtr Query, Counter; }
    static Exception Failed(string what, uint e) { return new Exception(what + ": 0x" + e.ToString("x")); }
    public static Clock Start() {
      var k = new Clock();
      uint e = PdhOpenQuery(null, IntPtr.Zero, out k.Query);
      if (e != 0) throw Failed("PdhOpenQuery", e);
      e = PdhAddEnglishCounter(k.Query, @"\Processor Information(*)\% Processor Performance", IntPtr.Zero, out k.Counter);
      if (e == 0) e = PdhCollectQueryData(k.Query);
      if (e != 0) { PdhCloseQuery(k.Query); throw Failed("PDH's % Processor Performance", e); }
      return k;
    }
    // Each processor's share of the nominal clock since Start, in per cent, by PDH's instance name
    // ("0,5" is group 0's processor 5; "_Total" and "0,_Total" the averages)
    public static Dictionary<string, double> Read(Clock k) {
      try {
        uint e = PdhCollectQueryData(k.Query);
        if (e != 0) throw Failed("PdhCollectQueryData", e);
        uint size = 0, count;
        e = PdhGetFormattedCounterArray(k.Counter, Double | NoCap100, ref size, out count, IntPtr.Zero);
        if (e != MoreData) throw Failed("PdhGetFormattedCounterArray", e);
        IntPtr items = Marshal.AllocHGlobal((int)size);
        try {
          e = PdhGetFormattedCounterArray(k.Counter, Double | NoCap100, ref size, out count, items);
          if (e != 0) throw Failed("PdhGetFormattedCounterArray", e);
          // PDH_FMT_COUNTERVALUE_ITEM_W: the name's pointer, then the status and the double after it
          int step = IntPtr.Size + 16;
          var clock = new Dictionary<string, double>();
          for (int i = 0; i < count; i++) {
            IntPtr item = items + i * step;
            uint status = (uint)Marshal.ReadInt32(item + IntPtr.Size);
            if (status <= 1) clock[Marshal.PtrToStringUni(Marshal.ReadIntPtr(item))] = BitConverter.Int64BitsToDouble(Marshal.ReadInt64(item + IntPtr.Size + 8));
          }
          return clock;
        } finally { Marshal.FreeHGlobal(items); }
      } finally { PdhCloseQuery(k.Query); }
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct Entry {
      public uint Size, Usage, Pid; public IntPtr Heap; public uint Module, Threads, Parent; public int Priority; public uint Flags;
      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string Exe;
    }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr snap, ref Entry e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr snap, ref Entry e);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong cycles);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long created, out long exited, out long kernel, out long user);
    // The cycles so far of -Roots and every process they started, and those started, down the
    // tree, by pid: a measurement's own processes, its panes' programs included. A child is one whose
    // parent pid is its parent's and that started after it, since a pid can be used again.
    public static Dictionary<int, long> TreeCycles(int[] roots) {
      var children = new Dictionary<uint, List<uint>>();
      IntPtr snap = CreateToolhelp32Snapshot(2, 0);
      if (snap == new IntPtr(-1)) throw new Exception("CreateToolhelp32Snapshot: " + Marshal.GetLastWin32Error());
      try {
        var e = new Entry(); e.Size = (uint)Marshal.SizeOf(typeof(Entry));
        for (bool ok = Process32FirstW(snap, ref e); ok; ok = Process32NextW(snap, ref e)) {
          if (e.Pid == 0 || e.Pid == e.Parent) continue;
          List<uint> list;
          if (!children.TryGetValue(e.Parent, out list)) children[e.Parent] = list = new List<uint>();
          list.Add(e.Pid);
        }
      } finally { CloseHandle(snap); }
      var cycles = new Dictionary<int, long>();
      var queue = new Queue<uint>();
      foreach (int r in roots) if (r > 0) queue.Enqueue((uint)r);
      while (queue.Count > 0) {
        uint pid = queue.Dequeue();
        if (cycles.ContainsKey((int)pid)) continue;
        IntPtr h = OpenProcess(0x1000, false, pid);
        if (h == IntPtr.Zero) continue;
        long created;
        try {
          ulong c; long x, k, u;
          if (!QueryProcessCycleTime(h, out c) || !GetProcessTimes(h, out created, out x, out k, out u)) continue;
          cycles[(int)pid] = (long)c;
        } finally { CloseHandle(h); }
        List<uint> kids;
        if (!children.TryGetValue(pid, out kids)) continue;
        foreach (uint kid in kids) {
          IntPtr kh = OpenProcess(0x1000, false, kid);
          if (kh == IntPtr.Zero) continue;
          try { long kc, x, k, u; if (GetProcessTimes(kh, out kc, out x, out k, out u) && kc >= created) queue.Enqueue(kid); } finally { CloseHandle(kh); }
        }
      }
      return cycles;
    }
  }
}
'@
}

# The start of a stretch to say what the PC did over: when, every processor's busy time, the CPUs
# -ProcessId (the measured process, else this one) may run on, which in a warden job are the job's,
# the cycles of the measurement's own processes (Own: this process, -Roots, which are the GUI and
# its mux servers, and every process they started, its panes' programs included, by pid), and the
# clock's reader. Stop-PcWatch ends it; ConvertTo-PcSpan (budgets.psm1) makes it numbers.
function Start-PcWatch([int]$ProcessId = $PID, [int[]]$Roots = @()) {
  Initialize-PcReader
  $mask = [long](Get-Process -Id $ProcessId -ErrorAction Stop).ProcessorAffinity
  $clock = [LbtermE2E.Pc]::Start()
  $own = [LbtermE2E.Pc]::TreeCycles([int[]](@($PID) + @($Roots)))
  [pscustomobject]@{
    At    = [Diagnostics.Stopwatch]::GetTimestamp()
    Busy  = [LbtermE2E.Pc]::Busy()
    Mask  = $mask
    Own   = $own
    Clock = $clock
  }
}

# The end of a Start-PcWatch: when, every processor's busy time, the measurement's own processes'
# cycles (-Roots as at the start, or with a process started since: a start's window) and each
# processor's clock over the stretch
function Stop-PcWatch([Parameter(Mandatory)]$Watch, [int[]]$Roots = @()) {
  $busy = [LbtermE2E.Pc]::Busy()
  $at = [Diagnostics.Stopwatch]::GetTimestamp()
  $own = [LbtermE2E.Pc]::TreeCycles([int[]](@($PID) + @($Roots)))
  $clock = [LbtermE2E.Pc]::Read($Watch.Clock)
  [pscustomobject]@{ At = $at; Busy = $busy; Own = $own; Clock = $clock }
}

# A memory guard for a test whose processes can outrun what it measures (a mux domain flooded
# faster than its window takes it grew a test server to 29.5 GB and its window to 11.5 GB in about
# seven minutes, 2026-10-29): on a timer of its own, so a wait on an echo or a state can't hold it
# up, every -PeriodMs it reads each of -Pids' private bytes and working set (Windows can trim the
# working set while private bytes grow, so both), and once any holds more than -LimitGB it kills
# them all and keeps what it found, for Get-MemoryGuard. One guard at a time: starting one stops the
# last. Invoke-Test stops it as the test ends, and fails the test when it tripped.
function Initialize-MemoryGuard {
  if ('LbtermE2E.MemoryGuard' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices;
namespace LbtermE2E {
  public static class MemoryGuard {
    [StructLayout(LayoutKind.Sequential)] struct Counters {
      public uint cb, PageFaults; public UIntPtr PeakWorkingSet, WorkingSet, PeakPaged, Paged, PeakNonPaged, NonPaged, Private, PeakPrivate;
    }
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern bool K32GetProcessMemoryInfo(IntPtr h, out Counters c, uint cb);
    static readonly object gate = new object();
    static System.Threading.Timer timer;
    static int[] pids = new int[0];
    static string[] names = new string[0];
    static Stopwatch clock;
    public static volatile string Tripped;
    // A process's private bytes and working set, or nothing when it's gone
    public static ulong[] Read(int pid) {
      IntPtr h = OpenProcess(0x1000 | 0x0010, false, (uint)pid);
      if (h == IntPtr.Zero) return null;
      try {
        Counters c;
        if (!K32GetProcessMemoryInfo(h, out c, (uint)Marshal.SizeOf(typeof(Counters)))) return null;
        return new ulong[] { (ulong)c.Private, (ulong)c.WorkingSet };
      } finally { CloseHandle(h); }
    }
    public static void Start(int[] ids, double limit, int periodMs) {
      Stop();
      lock (gate) {
        Tripped = null;
        pids = ids;
        names = new string[ids.Length];
        for (int i = 0; i < ids.Length; i++) {
          try { using (var p = Process.GetProcessById(ids[i])) { names[i] = p.ProcessName; } } catch (Exception) { names[i] = "?"; }
        }
        clock = Stopwatch.StartNew();
        timer = new System.Threading.Timer(_ => Look(limit), null, 0, periodMs);
      }
    }
    static void Look(double limit) {
      lock (gate) {
        if (Tripped != null || timer == null) return;
        for (int i = 0; i < pids.Length; i++) {
          var m = Read(pids[i]);
          if (m == null || Math.Max(m[0], m[1]) <= limit) continue;
          Tripped = string.Format("{0} (pid {1}) held {2:F0} MB private, {3:F0} MB working set, {4:F1} s after the guard started, over its {5:F1} GB: stopped",
            names[i], pids[i], m[0] / 1048576.0, m[1] / 1048576.0, clock.Elapsed.TotalSeconds, limit / 1073741824.0);
          foreach (var id in pids) { try { using (var p = Process.GetProcessById(id)) { p.Kill(); } } catch (Exception) { } }
          return;
        }
      }
    }
    // Stops the guard: what it tripped on, or null
    public static string Stop() {
      System.Threading.Timer t;
      lock (gate) { t = timer; timer = null; }
      if (t != null) { using (var done = new System.Threading.ManualResetEvent(false)) { t.Dispose(done); done.WaitOne(2000); } }
      return Tripped;
    }
  }
}
'@
}

function Start-MemoryGuard([Parameter(Mandatory)][int[]]$Pids, [double]$LimitGB = 2, [int]$PeriodMs = 200) {
  Initialize-MemoryGuard
  [LbtermE2E.MemoryGuard]::Start([int[]]@($Pids | Select-Object -Unique), $LimitGB * 1GB, $PeriodMs)
}

# What the guard tripped on ("<process> (pid n) held ... MB ..., over its 2.0 GB: stopped"), or nothing
function Get-MemoryGuard { if ('LbtermE2E.MemoryGuard' -as [type]) { [LbtermE2E.MemoryGuard]::Tripped } }

# Stops the guard, and says what it tripped on, if it did
function Stop-MemoryGuard { if ('LbtermE2E.MemoryGuard' -as [type]) { [LbtermE2E.MemoryGuard]::Stop() } }

# Minimizes a test window without activating anything (SW_SHOWMINNOACTIVE), or with -Restore
# shows it again without activating it (SW_SHOWNOACTIVATE). Whether it's minimized after.
function Set-WindowMinimized([Parameter(Mandatory)]$Window, [switch]$Restore) {
  Initialize-CpuReader
  $h = Resolve-Handle $Window
  [void][LbtermE2E.Show]::ShowWindow($h, $(if ($Restore) { 4 } else { 7 }))
  Watch-Foreground
  [LbtermE2E.Show]::IsIconic($h)
}

#endregion

#region What covers a test window (lbterm/PERF.md, "What's hard to time")

# DWM presents a screen only when something on it changes, so a test window another covers shows
# its motion and its echoes to nobody, and the screen gets no frame for them: what reached the
# screen is measured only while the window can be seen. Get-WindowCover says what's above it:
# every visible window over it in the z-order (EnumWindows goes topmost first), not cloaked, that
# overlaps it, by its process's file name and its class, never its title, with how much of the test
# window's part on its screen it covers (a window wider than its portrait screen has a part on
# none). Covered is the most any one of them covers, 0 to 1.
function Initialize-CoverReader {
  if ('LbtermE2E.Cover' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
namespace LbtermE2E {
  public class Covering { public string Exe; public string Class; public double Share; }
  public static class Cover {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT v, int size);
    [StructLayout(LayoutKind.Sequential)] struct MONITORINFO { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
    [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO i);
    // What a window takes up on screen: DWM's frame bounds, without the invisible resize border
    static bool Bounds(IntPtr h, out RECT r) {
      if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) == 0) return true;
      return GetWindowRect(h, out r);
    }
    // ... cut to its screen: a window wider than its screen has a part nobody could see anyway
    static RECT OnScreen(IntPtr h, RECT r) {
      var i = new MONITORINFO(); i.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
      if (!GetMonitorInfo(MonitorFromWindow(h, 2), ref i)) return r;
      var m = i.rcMonitor;
      return new RECT { Left = Math.Max(r.Left, m.Left), Top = Math.Max(r.Top, m.Top), Right = Math.Min(r.Right, m.Right), Bottom = Math.Min(r.Bottom, m.Bottom) };
    }
    public static Covering[] Over(IntPtr me) {
      var list = new List<Covering>();
      RECT mine;
      if (!Bounds(me, out mine)) return list.ToArray();
      mine = OnScreen(me, mine);
      if (mine.Right <= mine.Left || mine.Bottom <= mine.Top) return list.ToArray();
      double area = Math.Max(1.0, (double)(mine.Right - mine.Left) * (mine.Bottom - mine.Top));
      EnumWindows((h, l) => {
        if (h == me) return false;
        if (!IsWindowVisible(h) || IsIconic(h)) return true;
        int cloaked; if (DwmGetWindowAttribute(h, 14, out cloaked, 4) == 0 && cloaked != 0) return true;
        RECT r; if (!Bounds(h, out r)) return true;
        int w = Math.Min(r.Right, mine.Right) - Math.Max(r.Left, mine.Left);
        int t = Math.Min(r.Bottom, mine.Bottom) - Math.Max(r.Top, mine.Top);
        if (w <= 0 || t <= 0) return true;
        var c = new StringBuilder(256); GetClassName(h, c, 256);
        uint pid; GetWindowThreadProcessId(h, out pid);
        string exe = "";
        try { exe = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName + ".exe"; } catch { }
        list.Add(new Covering { Exe = exe, Class = c.ToString(), Share = Math.Round(w * (double)t / area, 3) });
        return true;
      }, IntPtr.Zero);
      return list.ToArray();
    }

    // The windows over `me` that cover more than `least` of it, as their bounds
    static List<RECT> Above(IntPtr me, RECT mine, double least) {
      var list = new List<RECT>();
      double area = Math.Max(1.0, (double)(mine.Right - mine.Left) * (mine.Bottom - mine.Top));
      EnumWindows((h, l) => {
        if (h == me) return false;
        if (!IsWindowVisible(h) || IsIconic(h)) return true;
        int cloaked; if (DwmGetWindowAttribute(h, 14, out cloaked, 4) == 0 && cloaked != 0) return true;
        RECT r; if (!Bounds(h, out r)) return true;
        // Small ones (another app's title-bar buttons) only where they'd cover enough of it here
        if ((double)(r.Right - r.Left) * (r.Bottom - r.Top) <= least * area) return true;
        list.Add(r);
        return true;
      }, IntPtr.Zero);
      return list;
    }

    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);

    // Moves `me` within `screen` (its bounds) to the first place, in steps of 40 px from the top
    // left, where none of the windows over it overlaps it, without changing its size, its place in
    // the z-order or activating it (SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE). False when there's
    // no such place, and it stays where it is
    public static bool MoveClear(IntPtr me, RECT screen) {
      RECT mine; if (!GetWindowRect(me, out mine)) return false;
      RECT dwm; if (!Bounds(me, out dwm)) dwm = mine;
      // A window wider or taller than its screen: the part that can be on it
      int w = Math.Min(dwm.Right - dwm.Left, screen.Right - screen.Left), ht = Math.Min(dwm.Bottom - dwm.Top, screen.Bottom - screen.Top);
      var above = Above(me, dwm, 0.02);
      for (int y = screen.Top; y + ht <= screen.Bottom; y += 40)
        for (int x = screen.Left; x + w <= screen.Right; x += 40) {
          bool clear = true;
          foreach (var r in above)
            if (r.Left < x + w && x < r.Right && r.Top < y + ht && y < r.Bottom) { clear = false; break; }
          if (!clear) continue;
          // GetWindowRect has the invisible resize border round DWM's bounds: keep it outside
          int dx = dwm.Left - mine.Left, dy = dwm.Top - mine.Top;
          return SetWindowPos(me, IntPtr.Zero, x - dx, y - dy, 0, 0, 0x0001 | 0x0004 | 0x0010);
        }
      return false;
    }
  }
}
'@
}

# Where a window of rectangle $Window (Left, Top, Right, Bottom) goes so all of it is on $Screen
# (X, Y, Right, Bottom), $Margin in from its edges where it can be: where it is when it's on it
# already, else moved in, and narrowed or shortened to the screen less two margins when it's bigger
# than that. Off is how many of its pixels were off the screen on each side. A window wider than its
# screen opens so: Get-OffscreenPosition puts its left 20 px in from the right-most screen's and
# its width is its columns', so a 110-column window on the 1080-wide portrait screen hung 70 px
# past the screen's right edge, where the context bar and the scroll strip are. Nothing there
# reaches any screen, so DWM presents nothing for motion there, and the walk counts its refreshes
# missed (lbterm/PITFALLS.md, 306; FLOWS.md, "The context bar's widening", round 4's stage 5)
function Get-OnScreenRect([Parameter(Mandatory)]$Window, [Parameter(Mandatory)]$Screen, [int]$Margin = 20) {
  $w = $Window.Right - $Window.Left; $h = $Window.Bottom - $Window.Top
  $off = [pscustomobject]@{
    Left = [Math]::Max(0, $Screen.X - $Window.Left); Right = [Math]::Max(0, $Window.Right - $Screen.Right)
    Top = [Math]::Max(0, $Screen.Y - $Window.Top); Bottom = [Math]::Max(0, $Window.Bottom - $Screen.Bottom)
  }
  $on = ($off.Left + $off.Right + $off.Top + $off.Bottom) -eq 0
  $fit = {
    param([int]$at, [int]$size, [int]$from, [int]$to)
    $size = [Math]::Min($size, $to - $from - 2 * $Margin)
    $at = [Math]::Min([Math]::Max($at, $from + $Margin), $to - $Margin - $size)
    $at, $size
  }
  $x, $w2 = if ($on) { $Window.Left, $w } else { & $fit $Window.Left $w $Screen.X $Screen.Right }
  $y, $h2 = if ($on) { $Window.Top, $h } else { & $fit $Window.Top $h $Screen.Y $Screen.Bottom }
  [pscustomobject]@{ X = $x; Y = $y; Width = $w2; Height = $h2; On = $on; Off = $off; Resized = ($w2 -ne $w -or $h2 -ne $h) }
}

# Puts a test window all on its screen (Get-OnScreenRect), without activating it or raising it,
# and says what it did: $null when it was on it already
function Move-TestWindowOnScreen([object]$Window) {
  Add-Type -AssemblyName System.Windows.Forms
  $h = [IntPtr](Resolve-Handle $Window)
  $s = [System.Windows.Forms.Screen]::FromHandle($h)
  $r = Get-WindowRect $Window
  $to = Get-OnScreenRect -Window $r -Screen $s.Bounds
  if ($to.On) { return $null }
  # SWP_NOZORDER | SWP_NOACTIVATE
  [void][LbtermE2E.Win]::SetWindowPos($h, [IntPtr]::Zero, $to.X, $to.Y, $to.Width, $to.Height, 0x0004 -bor 0x0010)
  $sides = @('Left', 'Right', 'Top', 'Bottom') | Where-Object { $to.Off.$_ -gt 0 } | ForEach-Object { "$($to.Off.$_) px past its $($_.ToLower())" }
  "$($r.Right - $r.Left) by $($r.Bottom - $r.Top) px, $($sides -join ' and ') edge of $($s.DeviceName) ($($s.Bounds.Width) by $($s.Bounds.Height)), $(if ($to.Resized) { "made $($to.Width) by $($to.Height) and " })moved all onto it"
}

# Moves a test window on its screen to where nothing over it in the z-order overlaps it, so what it
# draws reaches the screen, without activating it or raising it over anything. Its size and z-order
# stay as they are. True when it was moved or nothing covered it
function Move-TestWindowClear([object]$Window) {
  Initialize-CoverReader
  Add-Type -AssemblyName System.Windows.Forms
  $h = [IntPtr](Resolve-Handle $Window)
  if ((Get-WindowCover $Window).Covered -le 0.02) { return $true }
  $s = [System.Windows.Forms.Screen]::FromHandle($h).Bounds
  $r = New-Object LbtermE2E.Cover+RECT
  $r.Left = $s.X; $r.Top = $s.Y; $r.Right = $s.Right; $r.Bottom = $s.Bottom
  [LbtermE2E.Cover]::MoveClear($h, $r)
}

function Get-WindowCover([object]$Window) {
  Initialize-CoverReader
  $over = @([LbtermE2E.Cover]::Over([IntPtr](Resolve-Handle $Window)))
  [pscustomobject]@{
    Covered = if ($over) { ($over | Measure-Object Share -Maximum).Maximum } else { 0 }
    Windows = $over
    Said    = if ($over) { ($over | ForEach-Object { "$($_.Exe) ($($_.Class)) $([Math]::Round($_.Share * 100))%" }) -join ', ' } else { 'nothing' }
  }
}

#endregion

#region What DWM does (lbterm/COMPOSITOR.md, "Loops")

# DWM composes a frame only when something on a screen has changed, and a window's loops (its
# animations that never end: a waiting pad's arc, the Conductor's turning hues and light) change
# something every refresh, so while one plays DWM composes every refresh of the fastest screen.
# What it did over a span, by its own numbers: the compositor clock's completed frames
# (DCompositionGetFrameId, Windows 11), and how many of the span's 50 ms steps saw none (it was
# idle that long); dwm.exe's CPU (the Process counter's raw count, in 100 ns, which Windows keeps in
# its 15.625 ms ticks: dwm.exe can't be opened for its cycles without elevation); and its GPU
# engines' running time (the GPU Engine counters for its pid, 100 ns). All of it is the whole
# desktop's: every window's, other sessions' test windows and Lachlan's own included, so compare
# spans interleaved. DwmGetCompositionTimingInfo can't say it: its counts are the caller's (PERF.md,
# finding 41).
function Initialize-DwmReader {
  if ('LbtermE2E.DwmClock' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
namespace LbtermE2E {
  public static class DwmClock {
    [DllImport("dcomp.dll")] static extern int DCompositionGetFrameId(int kind, out ulong id);
    // The newest frame the compositor clock completed (COMPOSITION_FRAME_ID_COMPLETED), 0 when it can't say
    public static ulong Frame() { ulong id; return DCompositionGetFrameId(2, out id) >= 0 ? id : 0; }
    // Polls the newest completed frame every stepMs for ms: how many steps there were, and how
    // many saw no new frame
    public static int[] Idle(int ms, int stepMs) {
      var sw = System.Diagnostics.Stopwatch.StartNew();
      ulong last = Frame(); int steps = 0, idle = 0;
      while (sw.ElapsedMilliseconds < ms) {
        System.Threading.Thread.Sleep(stepMs);
        ulong now = Frame(); steps++;
        if (now == last) idle++;
        last = now;
      }
      return new int[] { steps, idle };
    }

    // How many frames DWM has presented to each screen (DCompositionGetTargetStatistics' presented
    // count), as pairs of the screen's target id and its count. DWM presents to a screen only when
    // something on it changed, where its clock completes a frame for every screen at once: a
    // light moving on one screen shows in that screen's count alone
    [StructLayout(LayoutKind.Sequential)] struct Luid { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] struct TargetId { public Luid Display; public Luid Render; public uint Source; public uint Target; public uint Unique; }
    [StructLayout(LayoutKind.Sequential)] struct Stats { public uint Presents; public uint Refreshes; public uint VirtualRefreshes; public ulong Time; }
    [StructLayout(LayoutKind.Sequential)] struct TargetStats { public uint Outstanding; public ulong PresentTime; public ulong Vblank; public Stats Presented; public Stats Completed; }
    [StructLayout(LayoutKind.Sequential)] struct FrameStats { public ulong Start; public ulong Target; public ulong Period; }
    [DllImport("dcomp.dll")] static extern int DCompositionGetStatistics(ulong frameId, out FrameStats stats, uint count, [Out] TargetId[] ids, out uint actual);
    [DllImport("dcomp.dll")] static extern int DCompositionGetTargetStatistics(ulong frameId, ref TargetId id, out TargetStats stats);
    public static long[] Presents() {
      ulong f = Frame(); var ids = new TargetId[16]; FrameStats fs; uint n;
      if (f == 0 || DCompositionGetStatistics(f, out fs, (uint)ids.Length, ids, out n) < 0) return new long[0];
      var o = new System.Collections.Generic.List<long>();
      for (int i = 0; i < n && i < ids.Length; i++) {
        TargetStats ts;
        if (DCompositionGetTargetStatistics(f, ref ids[i], out ts) >= 0) { o.Add(ids[i].Target); o.Add(ts.Presented.Presents); }
      }
      return o.ToArray();
    }

    // The target id of the screen a window is on (QueryDisplayConfig's, which DWM's statistics
    // name it by), 0 when it can't be told
    [StructLayout(LayoutKind.Sequential)] struct PathSource { public Luid Adapter; public uint Id; public uint Mode; public uint Status; }
    [StructLayout(LayoutKind.Sequential)] struct PathTarget { public Luid Adapter; public uint Id; public uint Mode; public uint Tech; public uint Rotation; public uint Scaling; public uint RefreshN; public uint RefreshD; public uint ScanLine; public int Available; public uint Status; }
    [StructLayout(LayoutKind.Sequential)] struct DisplayPath { public PathSource Source; public PathTarget Target; public uint Flags; }
    [StructLayout(LayoutKind.Sequential, Size = 64)] struct DisplayMode { public uint Type; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct SourceName { public uint Type; public uint Size; public Luid Adapter; public uint Id; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string Gdi; }
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct MonitorInfo { public uint Size; public Rect Monitor; public Rect Work; public uint Flags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string Device; }
    [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
    [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags, ref uint np, [Out] DisplayPath[] paths, ref uint nm, [Out] DisplayMode[] modes, IntPtr topology);
    [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(ref SourceName info);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfoW(IntPtr monitor, ref MonitorInfo info);
    public static long Screen(IntPtr hwnd) {
      var mi = new MonitorInfo { Size = (uint)Marshal.SizeOf<MonitorInfo>() };
      if (!GetMonitorInfoW(MonitorFromWindow(hwnd, 2), ref mi)) return 0;
      uint np, nm;
      if (GetDisplayConfigBufferSizes(2, out np, out nm) != 0) return 0;
      var paths = new DisplayPath[np]; var modes = new DisplayMode[nm];
      if (QueryDisplayConfig(2, ref np, paths, ref nm, modes, IntPtr.Zero) != 0) return 0;
      for (int i = 0; i < np; i++) {
        var name = new SourceName { Type = 1, Size = (uint)Marshal.SizeOf<SourceName>(), Adapter = paths[i].Source.Adapter, Id = paths[i].Source.Id };
        if (DisplayConfigGetDeviceInfo(ref name) == 0 && string.Equals(name.Gdi, mi.Device, StringComparison.OrdinalIgnoreCase)) return paths[i].Target.Id;
      }
      return 0;
    }
  }
}
'@
}

# dwm.exe's counters, made once (about 1.5 s): its CPU and each of its GPU engines' running time
# -Window names the screen whose presents a span counts apart (screen_presents): the one that
# window is on as the meter is made
function New-DwmMeter([object]$Window) {
  Initialize-DwmReader
  $dwm = @(Get-Process dwm -ErrorAction Stop)[0].Id
  $engines = [System.Diagnostics.PerformanceCounterCategory]::new('GPU Engine').GetInstanceNames() | Where-Object { $_ -like "pid_$($dwm)_*" }
  [pscustomobject]@{
    Pid = $dwm
    Cpu = [System.Diagnostics.PerformanceCounter]::new('Process', '% Processor Time', 'dwm', $true)
    Gpu = @($engines | ForEach-Object { [System.Diagnostics.PerformanceCounter]::new('GPU Engine', 'Running Time', $_, $true) })
    Screen = $(if ($Window) { [long][LbtermE2E.DwmClock]::Screen([IntPtr][long](Resolve-Handle $Window)) } else { [long]0 })
  }
}

# What DWM has done so far: the stopwatch's timestamp, the compositor clock's newest frame,
# dwm.exe's CPU and GPU time in 100 ns (an engine gone since is counted as nothing), and the
# frames it has presented to each screen, by the screen's target id
function Read-DwmMeter([Parameter(Mandatory)]$Meter) {
  $pairs = [LbtermE2E.DwmClock]::Presents()
  $presents = @{}
  for ($i = 0; $i + 1 -lt $pairs.Count; $i += 2) { $presents[[long]$pairs[$i]] = [double]$pairs[$i + 1] }
  [pscustomobject]@{
    At       = [Diagnostics.Stopwatch]::GetTimestamp()
    Frame    = [LbtermE2E.DwmClock]::Frame()
    Cpu      = [double]$Meter.Cpu.NextSample().RawValue
    Gpu      = [double](($Meter.Gpu | ForEach-Object { try { $_.NextSample().RawValue } catch { 0 } } | Measure-Object -Sum).Sum)
    Presents = $presents
    Screen   = $Meter.Screen
  }
}

# A span's numbers from two readings and its 50 ms steps ([steps, idle]): seconds, the compositor
# clock's frames a second, the share of steps with none, dwm.exe's CPU and GPU in ms a second,
# and with each screen's presents in the readings, the frames presented a second to every screen
# together, to each by its target id, and to the meter's screen (null where the readings don't say). The clock completes a
# frame for every screen at once whenever any one composes, so a window's own cost shows in its
# screen's presents where another screen's animation keeps the clock at every refresh
function ConvertTo-DwmSpan([Parameter(Mandatory)]$Before, [Parameter(Mandatory)]$After, [int[]]$Steps = @(0, 0), [double]$Frequency = [Diagnostics.Stopwatch]::Frequency) {
  $s = ([double]$After.At - [double]$Before.At) / $Frequency
  if ($s -le 0) { throw "a span of $s s" }
  # A presented count is 32 bits: one that went past it comes round from 0
  $gone = { param($a, $b) $d = [double]$b - [double]$a; if ($d -lt 0) { $d + 4294967296 } else { $d } }
  $all = $null; $screen = $null; $screens = $null
  if ($Before.Presents -and $After.Presents) {
    $all = 0; $screens = [ordered]@{}
    foreach ($k in @($After.Presents.Keys | Sort-Object)) {
      if ($Before.Presents.ContainsKey($k)) {
        $n = & $gone $Before.Presents[$k] $After.Presents[$k]
        $all += $n; $screens["$k"] = [Math]::Round($n / $s, 1)
      }
    }
    $all = [Math]::Round($all / $s, 1)
    $k = $After.Screen
    if ($k -and $Before.Presents.ContainsKey($k) -and $After.Presents.ContainsKey($k)) { $screen = [Math]::Round((& $gone $Before.Presents[$k] $After.Presents[$k]) / $s, 1) }
  }
  [pscustomobject]@{
    seconds         = [Math]::Round($s, 3)
    frames          = [Math]::Round(([double]$After.Frame - [double]$Before.Frame) / $s, 1)
    idle            = $(if ($Steps[0] -gt 0) { [Math]::Round($Steps[1] / $Steps[0], 3) })
    cpu_ms          = [Math]::Round(([double]$After.Cpu - [double]$Before.Cpu) / 1e4 / $s, 2)
    gpu_ms          = [Math]::Round(([double]$After.Gpu - [double]$Before.Gpu) / 1e4 / $s, 2)
    presents        = $all
    screen_presents = $screen
    screens         = $screens
  }
}

# What DWM did over the next -Seconds, polling its clock every 50 ms meanwhile
function Measure-DwmSpan([Parameter(Mandatory)]$Meter, [double]$Seconds = 10) {
  $before = Read-DwmMeter $Meter
  $steps = [LbtermE2E.DwmClock]::Idle([int]($Seconds * 1000), 50)
  $after = Read-DwmMeter $Meter
  ConvertTo-DwmSpan $before $after $steps
}

#endregion

#region Timing an action: every frame as the state file says it, when it was painted

# Now, as the state file's time_ms counts (Unix milliseconds), with the fraction: when an action
# was posted, to take from the time_ms of the frame that showed it. Both are the system clock
# (GetSystemTimePreciseAsFileTime in .NET and in Rust's SystemTime)
function Get-UnixMs { ([DateTime]::UtcNow - [DateTime]::UnixEpoch).TotalMilliseconds }

# Polling the state file misses frames: a read and a parse in PowerShell take a few milliseconds,
# Start-Sleep's shortest is the system's timer tick (15.6 ms unless something has raised it), and
# the GUI writes over the file after each frame, 8 ms apart at max_fps 120. So the first frame that
# shows something can be gone before it's read, and an action timed from it comes out a frame long.
# A recorder reads the file as each new one is renamed into place (a FileSystemWatcher on its
# folder) and keeps every frame's snapshot, in order, with its process frame and time_ms, for
# Wait-Recorded to go through. The writer itself only drops a snapshot when a newer one is handed
# over while it's still writing the last (lbterm/STATE.md), which at 8 ms a frame it rarely is.
# The recorder can lose one too: it reads the file when the watcher's event is handled, on a pool
# thread, and a frame the GUI has written over by then was never kept. On a PC busy enough to hold
# that thread up for a frame or more (two frames 25 ms apart were lost in a check beside cargo,
# 2026-11-12), the first frame kept that shows something can be a later one than the first that
# showed it (lbterm/PITFALLS.md, 355). It says when: the snapshot's top frame counts every frame
# the process has painted, so each frame kept carries how many came between it and the one kept
# before it (Missed), the recorder their sum, and Wait-Recorded hands the found frame's on.
function Start-FrameRecorder {
  param([Parameter(Mandatory, ParameterSetName = 'Window')]$Window, [Parameter(Mandatory, ParameterSetName = 'Path')][string]$Path)
  if (-not ('LbtermE2E.FrameRecorder' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.IO; using System.Text; using System.Text.RegularExpressions; using System.Collections.Generic;
namespace LbtermE2E {
  public class RecordedFrame { public long Frame; public long TimeMs; public string Text; public long Missed; }
  public class FrameRecorder : IDisposable {
    static readonly Regex Top = new Regex("\"frame\":\\s*(\\d+),\\s*\"time_ms\":\\s*(\\d+)");
    readonly string path; readonly FileSystemWatcher watch;
    readonly List<RecordedFrame> frames = new List<RecordedFrame>();
    long last = -1, missed = 0;
    public FrameRecorder(string path) {
      this.path = path;
      watch = new FileSystemWatcher(Path.GetDirectoryName(path)) { NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite, InternalBufferSize = 65536 };
      watch.Renamed += (s, e) => Look(); watch.Created += (s, e) => Look(); watch.Changed += (s, e) => Look();
      watch.EnableRaisingEvents = true;
      Look();
    }
    public int Count { get { lock (frames) return frames.Count; } }
    public long Last { get { lock (frames) return last; } }
    // Frames painted since the first one read that were never read: written over before the
    // read, or dropped by the writer
    public long Missed { get { lock (frames) return missed; } }
    // The file now, kept when it's a frame later than the last kept
    public void Look() {
      lock (frames) {
        string text;
        try {
          using (var fs = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete))
          using (var r = new StreamReader(fs, Encoding.UTF8)) text = r.ReadToEnd();
        } catch (IOException) { return; } catch (UnauthorizedAccessException) { return; }
        var m = Top.Match(text.Length > 400 ? text.Substring(0, 400) : text);
        if (!m.Success) return;
        long f = long.Parse(m.Groups[1].Value);
        if (f <= last) return;
        long gap = last >= 0 ? f - last - 1 : 0;
        missed += gap;
        last = f;
        frames.Add(new RecordedFrame { Frame = f, TimeMs = long.Parse(m.Groups[2].Value), Text = text, Missed = gap });
      }
    }
    // What's been kept since the last take, oldest first
    public RecordedFrame[] Take() { lock (frames) { var a = frames.ToArray(); frames.Clear(); return a; } }
    public void Dispose() { watch.Dispose(); }
  }
}
'@ -ReferencedAssemblies System.IO.FileSystem.Watcher, System.Text.RegularExpressions, System.Collections, System.Runtime, System.Threading, System.ComponentModel.Primitives
  }
  $p = if ($Path) { $Path } else { (Resolve-Window $Window).StateFile }
  [LbtermE2E.FrameRecorder]::new($p)
}

# The first frame a recorder kept, after process frame -AfterFrame, whose snapshot -Until
# (param($w, $s): the window's entry, found by -Hwnd, else the first, and the whole snapshot) is
# true of: {Window, State, Frame, TimeMs, Missed}, the frames before it taken and gone. Missed is
# how many frames the process painted straight before that one, after -AfterFrame, that the
# recorder never read: 0 says it is the first frame that showed it, more that one of those may
# have been, and a time taken from it can be that many frames long. -Queue (a List[object]) holds
# frames taken but not yet gone through, so a second wait carries on from the frame after the
# first's. Throws after -TimeoutSec with what the last frame said.
function Wait-Recorded {
  param([Parameter(Mandatory)]$Recorder, [Parameter(Mandatory, Position = 0)][scriptblock]$Until, [long]$Hwnd = 0,
    [long]$AfterFrame = -1, [Collections.Generic.List[object]]$Queue, [double]$TimeoutSec = 15, [string]$What = 'the frame asked for')
  if ($null -eq $Queue) { $Queue = [Collections.Generic.List[object]]::new() }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $last = $null
  while ($true) {
    # A frame written while nothing was watching (or a watcher event lost) is still read
    $Recorder.Look()
    foreach ($f in $Recorder.Take()) { $Queue.Add($f) }
    while ($Queue.Count) {
      $f = $Queue[0]
      $Queue.RemoveAt(0)
      if ($f.Frame -le $AfterFrame) { continue }
      $s = try { $f.Text | ConvertFrom-Json -Depth 32 } catch { $null }
      if (-not $s) { continue }
      $w = Get-StateWindow $s -Hwnd $Hwnd
      $last = $w
      if ($w -and (& $Until $w $s)) {
        if ($script:Test) { $script:Test.LastState = $s }
        $missed = if ($AfterFrame -ge 0) { [Math]::Min([long]$f.Missed, [Math]::Max(0L, [long]$f.Frame - $AfterFrame - 1)) } else { [long]$f.Missed }
        return [pscustomobject]@{ Window = $w; State = $s; Frame = $f.Frame; TimeMs = $f.TimeMs; Missed = $missed }
      }
    }
    if ($clock.Elapsed.TotalSeconds -ge $TimeoutSec) { break }
    Start-Sleep -Milliseconds 5
  }
  throw "timed out after $TimeoutSec s waiting for ${What}: the last frame recorded said $(Format-StateWindow $last)"
}

#endregion

#region Stand-ins: the Conductor's pipes, InMail, a Claude session's files

# A pipe standing in for one of the Conductor's (its hub, its seat), served here: every line any
# connection sends is kept; a connection that sends a line with -Subscribe in it ('"lbterm.subscribe"'
# for the hub, '"seat.subscribe"' for the seat) is pushed to; with -Reply (the seat's way) any other
# line with an id is a command, answered on its connection with a seat.reply, ok, or refused once
# when the object's Refuse names its type. perf/conductor.ps1 and conductor-view.ps1 carry the
# same as classes of their own.
function Start-PlayedPipe {
  param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$Subscribe, [switch]$Reply)
  if (-not ('LbtermE2E.PlayedPipe' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
namespace LbtermE2E {
  public class PlayedPipe {
    readonly string name, subscribe; readonly bool reply;
    readonly List<StreamWriter> subscribers = new List<StreamWriter>();
    readonly List<string> lines = new List<string>();
    public string Refuse = "";
    public PlayedPipe(string name, string subscribe, bool reply) {
      this.name = name; this.subscribe = subscribe; this.reply = reply;
      new Thread(Accept) { IsBackground = true }.Start();
    }
    public string Name { get { return name; } }
    public int Subscribers { get { lock (subscribers) return subscribers.Count; } }
    void Accept() {
      while (true) {
        var s = new NamedPipeServerStream(name, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances,
          PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
        s.WaitForConnection();
        new Thread(() => Talk(s)) { IsBackground = true }.Start();
      }
    }
    void Talk(NamedPipeServerStream s) {
      var reader = new StreamReader(s);
      var writer = new StreamWriter(s) { AutoFlush = true, NewLine = "\n" };
      bool subscribed = false;
      try {
        string line;
        while ((line = reader.ReadLine()) != null) {
          lock (lines) lines.Add(line);
          if (line.Contains(subscribe)) {
            if (!subscribed) { subscribed = true; lock (subscribers) subscribers.Add(writer); }
            continue;
          }
          if (!reply) continue;
          var id = Regex.Match(line, "\"id\":\"([^\"]+)\"");
          if (!id.Success) continue;
          var type = Regex.Match(line, "\"type\":\"([^\"]+)\"").Groups[1].Value;
          string answer;
          lock (lines) {
            if (Refuse == type) {
              Refuse = "";
              answer = "{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":false,\"error\":\"Refused by the test seat\"}";
            } else {
              answer = "{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":true}";
            }
          }
          writer.WriteLine(answer);
        }
      } catch (IOException) { }
      lock (subscribers) subscribers.Remove(writer);
    }
    // The line to every subscriber: how many it reached
    public int Push(string line) {
      int n = 0;
      lock (subscribers) foreach (var w in subscribers.ToArray()) { try { w.WriteLine(line); n++; } catch (IOException) { } }
      return n;
    }
    public string[] Heard() { lock (lines) return lines.ToArray(); }
    // The line pushed every everyMs from a timer of its own, @n@ in it the push's number from 1 and
    // @downN@ N less that (@down240@: 239, 238, ...); 0 stops it
    Timer repeat;
    public void Repeat(string template, int everyMs) {
      if (repeat != null) { repeat.Dispose(); repeat = null; }
      if (everyMs <= 0) return;
      int n = 0;
      repeat = new Timer(_ => {
        int k = Interlocked.Increment(ref n);
        Push(Regex.Replace(template.Replace("@n@", k.ToString()), @"@down(\d+)@", m => (long.Parse(m.Groups[1].Value) - k).ToString()));
      }, null, everyMs, everyMs);
    }
  }
}
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread, System.Text.RegularExpressions
  }
  [LbtermE2E.PlayedPipe]::new($Name, $Subscribe, [bool]$Reply)
}

# A line pushed to a played pipe's subscribers, its new lines and the spaces after them made one
# line; how many it reached
function Send-PlayedLine([Parameter(Mandatory)]$Pipe, [Parameter(Mandatory)][string]$Line) {
  $Pipe.Push(($Line -replace '\r?\n\s*', ' '))
}

# A line pushed to a played pipe's subscribers every -EveryMs, as the real seat pushes its view
# whenever any of it changes (Conductor's SEAT.md: at most every 250 ms): @n@ in it is the push's
# number from 1, @downN@ N less that (@down240@ counts down from 239). -EveryMs 0 stops it.
function Start-PlayedRepeat([Parameter(Mandatory)]$Pipe, [string]$Line = '', [int]$EveryMs = 250) {
  $Pipe.Repeat(($Line -replace '\r?\n\s*', ' '), $EveryMs)
}

# What LBTerm sent a played pipe, as objects; -Type only those of that type
function Get-PlayedLines([Parameter(Mandatory)]$Pipe, [string]$Type) {
  @($Pipe.Heard() | Where-Object { -not $Type -or $_ -match ('"type":"' + [regex]::Escape($Type) + '"') } |
    ForEach-Object { try { $_ | ConvertFrom-Json } catch { } })
}

# A message as InMail writes one, in -Folder (a project's InMail folder, made when it isn't there):
# its file
function New-InMailMessage {
  param([Parameter(Mandatory)][string]$Folder, [Parameter(Mandatory)][string]$To, [Parameter(Mandatory)][string]$From,
    [Parameter(Mandatory)][string]$Subject, [datetime]$At = (Get-Date))
  New-Item -ItemType Directory -Force $Folder | Out-Null
  $name = '{0:yyyy-MM-dd-HHmm}-from-{1}-{2}.md' -f $At, $From, ($Subject.ToLower() -replace '[^a-z0-9]+', '-').Trim('-')
  $file = Join-Path $Folder $name
  [IO.File]::WriteAllText($file, ("---`nfrom: $From`nto: $To`nsent: {0:yyyy-MM-ddTHH:mm}`nsubject: `"$Subject`"`n---`n`nThe body.`n" -f $At))
  $file
}

# An objective as Terminal's objectives.ts keeps one
function New-Objective([int]$Id, [string]$From, [string]$Status, [string]$Text, [string]$Evidence = '') {
  [ordered]@{ id = $Id; text = $Text; from = $From; ref = $null; at = 1; status = $Status; evidence = $Evidence; statusAt = 1 }
}

# What Terminal's workers keep for a Claude session in a summaries folder (lbterm_summaries), each
# only when given: its label, summary and detail, its objectives (with -Current the one it's on)
# and the folder it runs in
function Write-SessionFiles {
  param([Parameter(Mandatory)][string]$Summaries, [Parameter(Mandatory)][string]$Sid, [string]$Label, [string]$Summary,
    [string]$Detail, [object[]]$Objectives, [int]$Current = 1, [string]$Cwd)
  New-Item -ItemType Directory -Force $Summaries | Out-Null
  $put = { param($ext, $text) [IO.File]::WriteAllText((Join-Path $Summaries "$Sid.$ext"), $text) }
  if ($Label) { & $put 'label.txt' $Label }
  if ($Summary) { & $put 'txt' "$Summary`n" }
  if ($Detail) { & $put 'detail.txt' $Detail }
  if ($Cwd) { & $put 'cwd.txt' $Cwd }
  if ($Objectives) {
    & $put 'objectives.json' ([ordered]@{ sessionId = $Sid; updatedAt = 1; current = $Current; objectives = @($Objectives) } | ConvertTo-Json -Depth 5 -Compress)
  }
}

#endregion

#region The runner: lbterm.ps1 e2e

# The tags a test script's header carries, in comment lines before its param block:
#   # Covers: wezterm-gui/src/termwindow/render/strip.rs, lbterm-extras/src/strip*.rs
#   # Args: -ClickMark 10        the arguments the runner gives it
#   # Variant: -Mux              one more run, with these arguments (as <name>-mux)
#   # Helper: <why>              not a test (a stand-in other scripts run); never run
function Get-ScriptHeader {
  param([Parameter(Mandatory)][string]$Path)
  $covers = [Collections.Generic.List[string]]::new()
  $variants = [Collections.Generic.List[string]]::new()
  $runArgs = ''
  $helper = $false
  $hasCovers = $false
  foreach ($line in [IO.File]::ReadLines($Path)) {
    if ($line -match '^\s*param\s*\(' -or $line -match '^\s*\$ErrorActionPreference') { break }
    if ($line -match '^\s*#\s*Covers:\s*(.*)$') {
      $hasCovers = $true
      foreach ($g in ($Matches[1] -split '[,\s]+')) { if ($g) { $covers.Add($g) } }
    } elseif ($line -match '^\s*#\s*Variant:\s*(.+)$') { $variants.Add($Matches[1].Trim()) }
    elseif ($line -match '^\s*#\s*Args:\s*(.+)$') { $runArgs = $Matches[1].Trim() }
    elseif ($line -match '^\s*#\s*Helper:') { $helper = $true }
  }
  [pscustomobject]@{
    Covers    = @($covers)
    HasCovers = $hasCovers
    Variants  = @($variants)
    Args      = $runArgs
    Helper    = $helper
  }
}

# A variant's name from its arguments: '-Mux' -> 'mux', '-Slowdown 2' -> 'slowdown-2'
function Get-VariantSuffix([string]$Arguments) {
  (($Arguments -split '\s+' | Where-Object { $_ } | ForEach-Object { $_.TrimStart('-').ToLowerInvariant() }) -join '-') -replace '[^a-z0-9.-]', ''
}

# Every run the test scripts in $Dir make: one per script (helpers left out), and one more per
# Variant line. Each has its name (the folder its output goes in), the script, its arguments
# and its header.
function Get-E2eEntries {
  param([Parameter(Mandatory)][string]$Dir)
  foreach ($f in Get-ChildItem $Dir -Filter *.ps1 | Sort-Object Name) {
    $head = Get-ScriptHeader $f.FullName
    if ($head.Helper) { continue }
    $base = [IO.Path]::GetFileNameWithoutExtension($f.Name)
    [pscustomobject]@{ Name = $base; Script = $base; Path = $f.FullName; Args = @($head.Args -split '\s+' | Where-Object { $_ }); Header = $head }
    foreach ($v in $head.Variants) {
      [pscustomobject]@{ Name = "$base-$(Get-VariantSuffix $v)"; Script = $base; Path = $f.FullName; Args = @($v -split '\s+' | Where-Object { $_ }); Header = $head }
    }
  }
}

# A glob over repo-relative paths as a regex: * within a folder, ** across folders, ? one
# character; a glob ending in / covers everything under it. Forward or back slashes.
function ConvertTo-GlobRegex([Parameter(Mandatory)][string]$Glob) {
  $g = ($Glob.Trim() -replace '\\', '/') -replace '^\./', ''
  $sb = [Text.StringBuilder]::new('^')
  for ($i = 0; $i -lt $g.Length; $i++) {
    $c = $g[$i]
    if ($c -eq '*') {
      if ($i + 1 -lt $g.Length -and $g[$i + 1] -eq '*') {
        $i++
        if ($i + 1 -lt $g.Length -and $g[$i + 1] -eq '/') { $i++; [void]$sb.Append('(?:.*/)?') }
        else { [void]$sb.Append('.*') }
      } else { [void]$sb.Append('[^/]*') }
    } elseif ($c -eq '?') { [void]$sb.Append('[^/]') }
    else { [void]$sb.Append([regex]::Escape([string]$c)) }
  }
  if ($g.EndsWith('/')) { [void]$sb.Append('.*') }
  [void]$sb.Append('$')
  $sb.ToString()
}

# Whether any of $Globs covers the repo-relative $File
function Test-Covers([string[]]$Globs, [Parameter(Mandatory)][string]$File) {
  $f = ($File -replace '\\', '/') -replace '^\./', ''
  foreach ($g in $Globs) {
    if ($g -and $f -match (ConvertTo-GlobRegex $g)) { return $true }
  }
  $false
}

# What every test leans on: a change to one of these picks every script
$script:SharedFiles = @('lbterm/harness.psm1', 'lbterm/motion/start-unfocused.ps1', 'lbterm/motion/offscreen.ps1')

# The entries a change to $Files (repo-relative) picks: those whose Covers globs match one, whose
# own script changed, or all of them when the harness changed. Entries without a Covers line are
# Uncovered: listed, and run only when their own script changed.
function Select-E2eChanged {
  param([object[]]$Entries, [string[]]$Files)
  $files = @($Files | Where-Object { $_ } | ForEach-Object { ($_ -replace '\\', '/') })
  $shared = @($files | Where-Object { $_ -in $script:SharedFiles })
  $selected = [Collections.Generic.List[object]]::new()
  $uncovered = [Collections.Generic.List[object]]::new()
  foreach ($e in $Entries) {
    $own = 'lbterm/perf/' + [IO.Path]::GetFileName($e.Path)
    $pick = $shared.Count -gt 0 -or $own -in $files
    if (-not $pick -and $e.Header.HasCovers) {
      foreach ($f in $files) { if (Test-Covers $e.Header.Covers $f) { $pick = $true; break } }
    }
    if (-not $e.Header.HasCovers) { $uncovered.Add($e) }
    if ($pick) { $selected.Add($e) }
  }
  [pscustomobject]@{ Selected = @($selected); Uncovered = @($uncovered) }
}

# The entries -Only names: an entry's own name, or its script's (which picks its variants too),
# wildcards allowed
function Select-E2eOnly([object[]]$Entries, [string[]]$Only) {
  $names = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
  $picked = @($Entries | Where-Object { $e = $_; $names | Where-Object { $e.Name -like $_ -or $e.Script -like $_ } })
  $unknown = @($names | Where-Object { $n = $_; -not ($Entries | Where-Object { $_.Name -like $n -or $_.Script -like $n }) })
  if ($unknown) { throw "no test script named $($unknown -join ', ') in lbterm\perf" }
  $picked
}

# The smoke set (lbterm/perf/smoke.json, Lachlan's 1a of 2026-11-09): the runs a branch's last stage
# always makes, each named exactly, so a script's variants come only when the file names them too.
# A name no script makes throws, so a renamed script can't leave the set quietly smaller
function Get-E2eSmoke {
  param([Parameter(Mandatory)][object[]]$Entries, [Parameter(Mandatory)][string]$File)
  $set = Get-Content $File -Raw | ConvertFrom-Json
  $names = @($set.scripts | ForEach-Object name)
  $unknown = @($names | Where-Object { $n = $_; -not ($Entries | Where-Object Name -eq $n) })
  if ($unknown) { throw "$File names $($unknown -join ', '), which no script in lbterm\perf runs" }
  @($Entries | Where-Object { $_.Name -in $names })
}

# The runs of every test script the branch added or changed (its own file among $Files, which are
# repo-relative), each with its variants: what the branch wrote to judge itself
function Select-E2eOwn {
  param([object[]]$Entries, [string[]]$Files)
  $files = @($Files | Where-Object { $_ } | ForEach-Object { $_ -replace '\\', '/' })
  @($Entries | Where-Object { ('lbterm/perf/' + [IO.Path]::GetFileName($_.Path)) -in $files })
}

# The files changed on this branch since it left main, and those not committed yet
function Get-ChangedFiles([Parameter(Mandatory)][string]$Repo) {
  $base = git -C $Repo merge-base main HEAD 2>$null
  $files = @()
  if ($base) { $files += @(git -C $Repo diff --name-only "$base" HEAD) }
  $files += @(git -C $Repo diff --name-only HEAD)
  $files += @(git -C $Repo ls-files --others --exclude-standard)
  @($files | Where-Object { $_ } | Sort-Object -Unique)
}

# What one run of a script came to, from the result.json it wrote (scripts on the harness) or,
# for one that writes none, its exit code and the FAIL lines it printed
function Read-E2eAttempt {
  param([Parameter(Mandatory)][string]$Dir, [int]$Exit, [double]$Seconds, [bool]$TimedOut)
  $resultFile = Join-Path $Dir 'result.json'
  $result = $null
  if (Test-Path $resultFile) { try { $result = Get-Content $resultFile -Raw | ConvertFrom-Json } catch { } }
  $failures = @()
  $captures = @()
  $notes = @()
  if ($result) {
    $failures = @($result.failures)
    $captures = @($result.captures)
    $notes = @($result.notes | Where-Object { $_ })
  }
  $log = Join-Path $Dir 'output.txt'
  if (-not $result -and (Test-Path $log)) {
    $failures = @(Get-Content $log | Where-Object { $_ -match '^\s*FAIL:?\s+' } | ForEach-Object { $_ -replace '^\s*FAIL:?\s+', '' })
  }
  if ($TimedOut) { $failures = @($failures) + "timed out after $([int]$Seconds) s" }
  elseif ($Exit -ne 0 -and -not $failures -and (Test-Path $log)) {
    # A throw: the error PowerShell printed, its first line
    $err = Get-Content $log | Where-Object { $_ -match '\S' } | Select-Object -Last 12
    $failures = @("exit $Exit" + $(if ($err) { ': ' + (($err | Select-Object -First 3) -join ' ').Trim() } else { '' }))
  }
  $pass = -not $TimedOut -and $Exit -eq 0 -and ($null -eq $result -or $result.pass -eq $true)
  $unjudged = if ($pass -and $result -and $result.unjudged) { [string]$result.unjudged } else { $null }
  [pscustomobject]@{
    Pass = $pass; Unjudged = $unjudged; Exit = $Exit; Seconds = [Math]::Round($Seconds, 1); TimedOut = $TimedOut
    Dir = $Dir; Failures = @($failures); Captures = @($captures); Notes = @($notes)
  }
}

# pass: the first run passed. unjudged: it passed having judged none of what it's for
# (Set-Unjudged). flaky: a later run passed. fail: none did.
function Get-E2eVerdict([object[]]$Attempts) {
  if (-not $Attempts) { return 'fail' }
  if ($Attempts[0].Pass) { if ($Attempts[0].Unjudged) { return 'unjudged' }; return 'pass' }
  if (@($Attempts | Where-Object Pass).Count) { return 'flaky' }
  'fail'
}

# report.json's shape: the run, the build, and each entry with its attempts
function New-E2eReport {
  param([string]$Run, [string]$Bin, [string]$Commit, [datetime]$Started, [object[]]$Results, [string]$Stopped)
  $entries = foreach ($r in $Results) {
    $last = $r.Attempts[-1]
    [ordered]@{
      name     = $r.Name
      script   = $r.Script
      args     = @($r.Args)
      result   = $r.Result
      flaky    = $r.Result -eq 'flaky'
      seconds  = $last.Seconds
      failures = @($r.Attempts | Where-Object { -not $_.Pass } | Select-Object -First 1 | ForEach-Object Failures)
      out      = $last.Dir
      captures = @($last.Captures)
      notes    = @($r.Attempts | ForEach-Object { $_.Notes } | Where-Object { $_ } | Select-Object -Unique)
      unjudged = $(if ($r.Result -eq 'unjudged') { $r.Attempts[0].Unjudged } else { $null })
      attempts = @($r.Attempts | ForEach-Object { [ordered]@{ pass = $_.Pass; unjudged = $_.Unjudged; exit = $_.Exit; seconds = $_.Seconds; timedOut = $_.TimedOut; out = $_.Dir; failures = @($_.Failures) } })
    }
  }
  $entries = @($entries)
  [ordered]@{
    run      = $Run
    bin      = $Bin
    commit   = $Commit
    started  = $Started.ToString('o')
    seconds  = [Math]::Round(((Get-Date) - $Started).TotalSeconds, 1)
    summary  = [ordered]@{
      pass  = @($entries | Where-Object { $_.result -eq 'pass' }).Count
      unjudged = @($entries | Where-Object { $_.result -eq 'unjudged' }).Count
      flaky = @($entries | Where-Object { $_.result -eq 'flaky' }).Count
      fail  = @($entries | Where-Object { $_.result -eq 'fail' }).Count
      notes = @($entries | ForEach-Object { $_.notes } | Where-Object { $_ }).Count
    }
    scripts  = $entries
    stopped  = $(if ($Stopped) { $Stopped } else { $null })
  }
}

# The short table the runner prints: script, result, seconds, flaky, and how many notes
function Format-E2eTable($Report) {
  $w = [Math]::Max(6, (@($Report.scripts | ForEach-Object { $_.name.Length }) + 6 | Measure-Object -Maximum).Maximum)
  ('{0}  {1,-8}  {2,7}  {3}' -f 'script'.PadRight($w), 'result', 'seconds', 'flaky')
  foreach ($s in $Report.scripts) {
    '{0}  {1,-8}  {2,7:N1}  {3}' -f $s.name.PadRight($w), $s.result, $s.seconds, $(if ($s.flaky) { 'flaky' } elseif ($s.unjudged) { $s.unjudged } else { '' })
  }
  $m = $Report.summary
  if ($Report.stopped) { "STOPPED: $($Report.stopped)" }
  if ($m.notes) { "notes: $($m.notes), windows of other tests (not failures), in report.json" }
  "$($m.pass) passed, $(if ($m.unjudged) { "$($m.unjudged) unjudged (judged nothing: not passes), " })$($m.flaky) flaky, $($m.fail) failed in $([Math]::Round($Report.seconds / 60, 1)) min"
}

# The test GUIs and mux servers a script left running: those it registered in pids.txt and any
# whose command line names its output folder (where its configs live). Never the installed
# LBTerm's. Their ids.
function Stop-E2eLeftovers([Parameter(Mandatory)][string]$Dir) {
  $installed = Join-Path $env:LOCALAPPDATA 'Programs\LBTerm'
  $ids = [Collections.Generic.HashSet[int]]::new()
  $pidFile = Join-Path $Dir 'pids.txt'
  if (Test-Path $pidFile) { foreach ($l in Get-Content $pidFile) { if ($l -match '^\d+$') { [void]$ids.Add([int]$l) } } }
  $stopped = @()
  $procs = @(Get-CimInstance Win32_Process -Filter "Name = 'lbterm-gui.exe' OR Name = 'lbterm-mux-server.exe'" -ErrorAction SilentlyContinue)
  foreach ($p in $procs) {
    if ($p.ExecutablePath -and $p.ExecutablePath -like "$installed\*") { continue }
    $named = $p.CommandLine -and (($p.CommandLine -replace '/', '\') -like "*$Dir\*")
    if ($ids.Contains([int]$p.ProcessId) -or $named) {
      Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
      $stopped += [int]$p.ProcessId
    }
  }
  $stopped
}

# An attempt's folder renamed out of the way of the next attempt (<name>.<n>). A process the
# last attempt started can still be on its way out (a test window's mux server, a pane's console
# host and shell, whose folder is in it), and Windows refuses to rename a folder while a process
# holds a file in it or has it as its folder: the rename is tried again every 250 ms for up to
# -TimeoutSec, then throws saying so. It killed a whole budget run until 2026-10-31
function Rename-AttemptFolder([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$NewName, [double]$TimeoutSec = 20) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($true) {
    try { Rename-Item -LiteralPath $Path $NewName -ErrorAction Stop; return }
    catch {
      if ($clock.Elapsed.TotalSeconds -ge $TimeoutSec) { throw "the last attempt's folder $Path is still held after $TimeoutSec s, so it can't be kept as ${NewName}: $($_.Exception.Message)" }
      Start-Sleep -Milliseconds 250
    }
  }
}

# One run of a script: its output (stdout and stderr, in order) in $Dir\output.txt, through
# cmd's redirection so nothing interleaves wrongly, with LBTERM_E2E_OUT naming $Dir
function Invoke-E2eAttempt {
  param([Parameter(Mandatory)]$Entry, [Parameter(Mandatory)][string]$Dir, [Parameter(Mandatory)][string]$Bin, [int]$TimeoutSec = 600, [switch]$WaitIdle)
  New-Item -ItemType Directory -Force $Dir | Out-Null
  $log = Join-Path $Dir 'output.txt'
  $quoted = { param($s) if ($s -match '[\s"]') { '"' + ($s -replace '"', '\"') + '"' } else { $s } }
  $line = @("`"$script:Pwsh`"", '-NoProfile', '-File', (& $quoted $Entry.Path), '-Bin', (& $quoted $Bin)) + @($Entry.Args | ForEach-Object { & $quoted $_ })
  $psi = [Diagnostics.ProcessStartInfo]::new($env:ComSpec)
  # A UTF-8 console, as Lachlan's terminal has: pwsh decodes what `lbterm cli get-text` prints
  # (Claude Code's ❯ and the like) in the console's code page
  $psi.Arguments = "/d /s /c `"chcp 65001 >nul & $($line -join ' ') > `"$log`" 2>&1`""
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.WorkingDirectory = $script:Repo
  $psi.Environment['LBTERM_E2E_OUT'] = $Dir
  if ($WaitIdle) { $psi.Environment['LBTERM_E2E_WAIT_IDLE'] = '1' }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $p = [Diagnostics.Process]::Start($psi)
  $timedOut = -not $p.WaitForExit($TimeoutSec * 1000)
  if ($timedOut) {
    & taskkill.exe /T /F /PID $p.Id 2>&1 | Out-Null
    $p.WaitForExit(10000) | Out-Null
  } else { $p.WaitForExit() }
  $exit = if ($timedOut) { -1 } else { $p.ExitCode }
  $seconds = $clock.Elapsed.TotalSeconds
  $left = @(Stop-E2eLeftovers $Dir)
  $attempt = Read-E2eAttempt -Dir $Dir -Exit $exit -Seconds $seconds -TimedOut $timedOut
  if ($left -and -not $timedOut) {
    Add-Content $log "harness: stopped $($left.Count) test process(es) the script left running: $($left -join ', ')"
  }
  $attempt
}

# Whether the workstation is locked: the lock screen (LogonUI.exe) is up. Windows then reports
# other screens and PrintWindow leaves DirectComposition's content out, so a capture of a test
# window shows none of its cards, outlines or arcs, and a test's failure says nothing
function Test-WorkstationLocked { [bool](Get-Process LogonUI -ErrorAction SilentlyContinue) }

# The power plan's display-off time on mains power, in seconds, from powercfg's text (its
# "Current AC Power Setting Index" for VIDEOIDLE, in hex); 0 when it says none, or never
function ConvertFrom-PowercfgIndex([string]$Text) {
  if ($Text -match 'Current AC Power Setting Index:\s*0x([0-9a-fA-F]+)') { [Convert]::ToInt64($Matches[1], 16) } else { 0 }
}

function Get-DisplayOffSeconds {
  ConvertFrom-PowercfgIndex ((powercfg /query SCHEME_CURRENT SUB_VIDEO VIDEOIDLE 2>$null) -join "`n")
}

# The displays' power as Windows has it, and a hold that keeps them on (Lachlan's E4 of
# 2026-11-10). `Displays.Power` reads GUID_CONSOLE_DISPLAY_STATE, which Windows sends a window as
# soon as it registers for it (0 off, 1 on, 2 dimmed): a message-only window of its own on the
# calling thread, pumped until the answer comes, then destroyed. `Displays.Hold` starts one
# background thread that asks for ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED and
# sleeps, so the request lasts as long as the process does, as a video player's does; it's not
# input, and the idle time Windows counts goes on. `Displays.SystemState` is the execution state
# in force across the PC (CallNtPowerInformation's SystemExecutionState), for the tests
function Use-DisplaysType {
  if ('LbtermE2E.Displays' -as [type]) { return }
  Add-Type -TypeDefinition @'
using System; using System.ComponentModel; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
namespace LbtermE2E {
  public static class Displays {
    delegate IntPtr WndProc(IntPtr h, uint m, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASSEX { public uint cbSize, style; public WndProc lpfnWndProc; public int cbClsExtra, cbWndExtra; public IntPtr hInstance, hIcon, hCursor, hbrBackground; public string lpszMenuName, lpszClassName; public IntPtr hIconSm; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }
    [StructLayout(LayoutKind.Sequential)] struct SETTING { public Guid PowerSetting; public uint DataLength; public uint Data; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClassEx(ref WNDCLASSEX c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateWindowEx(uint ex, string cls, string name, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProc(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr RegisterPowerSettingNotification(IntPtr h, ref Guid g, int flags);
    [DllImport("user32.dll")] static extern bool UnregisterPowerSettingNotification(IntPtr h);
    [DllImport("user32.dll")] static extern bool PeekMessage(out MSG m, IntPtr h, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG m);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandle(string name);
    [DllImport("kernel32.dll")] static extern uint SetThreadExecutionState(uint flags);
    [DllImport("powrprof.dll")] static extern uint CallNtPowerInformation(int level, IntPtr input, uint inputLength, out uint output, uint outputLength);
    static Guid consoleDisplayState = new Guid("6FE69556-704A-47A0-8F24-C28D936FDA47");
    static readonly WndProc proc = Proc;
    static string cls;
    [ThreadStatic] static int answer;
    static Thread holder;
    static IntPtr Proc(IntPtr h, uint m, IntPtr w, IntPtr l) {
      // WM_POWERBROADCAST with PBT_POWERSETTINGCHANGE
      if (m == 0x0218 && w.ToInt64() == 0x8013) {
        var s = (SETTING)Marshal.PtrToStructure(l, typeof(SETTING));
        if (s.PowerSetting == consoleDisplayState) answer = (int)s.Data;
        return new IntPtr(1);
      }
      return DefWindowProc(h, m, w, l);
    }
    // 0 off, 1 on, 2 dimmed; -1 when Windows said nothing in timeoutMs
    public static int Power(int timeoutMs) {
      lock (proc) {
        if (cls == null) {
          var name = "LbtermE2EDisplays" + Process.GetCurrentProcess().Id;
          var wc = new WNDCLASSEX { cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX)), lpfnWndProc = proc, hInstance = GetModuleHandle(null), lpszClassName = name };
          if (RegisterClassEx(ref wc) == 0) throw new Win32Exception();
          cls = name;
        }
      }
      answer = -1;
      // HWND_MESSAGE: a message-only window, never shown
      IntPtr h = CreateWindowEx(0, cls, "", 0, 0, 0, 0, 0, new IntPtr(-3), IntPtr.Zero, GetModuleHandle(null), IntPtr.Zero);
      if (h == IntPtr.Zero) throw new Win32Exception();
      IntPtr reg = IntPtr.Zero;
      try {
        reg = RegisterPowerSettingNotification(h, ref consoleDisplayState, 0);
        if (reg == IntPtr.Zero) throw new Win32Exception();
        var clock = Stopwatch.StartNew();
        while (answer < 0 && clock.ElapsedMilliseconds < timeoutMs) {
          MSG msg;
          while (PeekMessage(out msg, IntPtr.Zero, 0, 0, 1)) { TranslateMessage(ref msg); DispatchMessage(ref msg); }
          if (answer < 0) Thread.Sleep(5);
        }
        return answer;
      } finally {
        if (reg != IntPtr.Zero) UnregisterPowerSettingNotification(reg);
        DestroyWindow(h);
      }
    }
    public static bool Holding { get { return holder != null; } }
    public static void Hold() {
      lock (proc) {
        if (holder != null) return;
        var ready = new ManualResetEventSlim();
        uint before = 0;
        holder = new Thread(() => {
          before = SetThreadExecutionState(0x80000000u | 0x2u | 0x1u);
          ready.Set();
          Thread.Sleep(Timeout.Infinite);
        }) { IsBackground = true, Name = "lbterm-displays-awake" };
        holder.Start();
        ready.Wait();
        if (before == 0) { holder = null; throw new Win32Exception("SetThreadExecutionState refused the displays' hold"); }
      }
    }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint count, INPUT[] inputs, int size);
    // One relative mouse move of no pixels (INPUT_MOUSE, MOUSEEVENTF_MOVE, 0 and 0): the pointer
    // stays where it is and nothing is pressed, but Windows takes it as input, which turns displays
    // that are off back on (and resets the idle time, as any input does)
    public static void NoMove() {
      var input = new INPUT[] { new INPUT { type = 0, mi = new MOUSEINPUT { dwFlags = 0x0001 } } };
      if (SendInput(1, input, Marshal.SizeOf(typeof(INPUT))) != 1) throw new Win32Exception();
    }
    public static uint SystemState() {
      uint s;
      uint status = CallNtPowerInformation(16, IntPtr.Zero, 0, out s, 4);
      if (status != 0) throw new Win32Exception("CallNtPowerInformation(SystemExecutionState) said 0x" + status.ToString("x"));
      return s;
    }
  }
}
'@
}

# The displays' power, as Windows has it now: on, off or dimmed. Throws when Windows says nothing,
# rather than guess
function Get-DisplayPower {
  Use-DisplaysType
  switch ([LbtermE2E.Displays]::Power(2000)) {
    0 { 'off' } 1 { 'on' } 2 { 'dimmed' }
    default { throw "Windows didn't say whether the displays are on (GUID_CONSOLE_DISPLAY_STATE) within 2 s" }
  }
}

# A mouse move of no pixels (LbtermE2E.Displays.NoMove), in a function of its own so tests can
# stand in for it
function Send-NoMove { Use-DisplaysType; [LbtermE2E.Displays]::NoMove() }

# Keeps the displays on while this process runs, and turns them back on if they're off (Lachlan's
# E4 and 2a of 2026-11-10): lbterm.ps1 e2e, budgets and look before they start, and Open-TestWindow
# for a script run on its own, so a run started while he's away neither stops at displays that went
# to sleep mid-run nor at ones already asleep. The hold alone keeps them on but doesn't bring back
# displays already off (tried 2026-11-10 with no input for 34 to 46 s: off for the 12 s after the
# hold), so displays found off get a mouse move of no pixels (Send-NoMove), which does, and the
# idle time starts again from it. Says what it did or found, or nothing when they were on
function Request-DisplaysAwake([int]$WaitSec = 5) {
  Use-DisplaysType
  [LbtermE2E.Displays]::Hold()
  if ((Get-DisplayPower) -ne 'off') { return }
  Send-NoMove
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (($power = Get-DisplayPower) -eq 'off' -and $clock.Elapsed.TotalSeconds -lt $WaitSec) { Start-Sleep -Milliseconds 100 }
  if ($power -eq 'off') { "the displays stayed off for $WaitSec s after a mouse move of no pixels" }
  else { "the displays were off: a mouse move of no pixels turned them back on in $([Math]::Round($clock.Elapsed.TotalSeconds, 1)) s" }
}

# Why a test-window run can't be judged now because the displays are asleep, or nothing: Windows
# says they're off. DWM then plays no motion into a capture and blocks a window's draw step (8 to
# 11 frames a second), so every script that judges motion or counts frames fails on every build
# (PITFALLS 239: the runs of 2026-11-09 02:30 to 05:00 failed 22 and 23 scripts, main's build the
# same ones). Lachlan's 11a of 2026-11-09: no e2e or budgets run while they sleep. Before E4 this
# was worked out from the input's idle time against the power plan's display-off time, which a
# hold (Request-DisplaysAwake) makes untrue: the idle time goes on while the displays stay on
function Get-DisplaysAsleep {
  param([string]$Power = (Get-DisplayPower))
  if ($Power -ne 'off') { return $null }
  $idle = [Math]::Round([LbtermE2E.Win]::IdleMs() / 1000)
  "the displays are asleep: Windows says they're off (no input for $idle s; the power plan turns them off after $(Get-DisplayOffSeconds) s, and a mouse move of no pixels didn't bring them back)"
}

# lbterm.ps1 e2e -Changed: the scripts whose Covers lines match a file changed since main, to pick
# by name the ones that judge the change; runs nothing (Lachlan's 10a of 2026-11-09)
function Format-E2eChanged {
  param([string]$PerfDir = (Join-Path $script:Repo 'lbterm\perf'), [string[]]$Files = (Get-ChangedFiles $script:Repo))
  $entries = @(Get-E2eEntries $PerfDir)
  $pick = Select-E2eChanged -Entries $entries -Files $Files
  "$(@($Files).Count) changed files match the Covers lines of $(@($pick.Selected).Count) of $($entries.Count) scripts: $((@($pick.Selected) | ForEach-Object Name) -join ', ')"
  if ($pick.Uncovered) { "no Covers line, so they aren't judged by it: $((@($pick.Uncovered) | ForEach-Object Name) -join ', ')" }
  'Nothing was run. A branch''s last stage runs lbterm.ps1 e2e -Smoke (the smoke set and the scripts the branch changed); add one of these with -Only only when it judges what the change does and nothing in that does'
}

# lbterm.ps1 e2e: the scripts one at a time, a failure run again up to $Retry times (flaky if a
# rerun passes), report.json and the table at the end. Returns the report. The run stops when the
# workstation is locked (Test-WorkstationLocked) or the displays go to sleep (Get-DisplaysAsleep),
# before a script or after one failed: a failure then is the lock's or the sleep's, so it isn't
# counted, and the report says where the run stopped.
function Invoke-E2eRun {
  param(
    [Parameter(Mandatory)][string]$Bin,
    [string[]]$Only,
    [switch]$Changed,
    # The smoke set (perf/smoke.json) and the scripts the branch added or changed, beside -Only's
    [switch]$Smoke,
    [int]$Retry = 1,
    [int]$TimeoutMin = 10,
    # Each test window waits for Lachlan's hands to be off the keyboard and mouse for 3 s
    [switch]$WaitIdle,
    [string]$Root = (Join-Path $script:Repo 'target\e2e'),
    [string]$PerfDir = (Join-Path $script:Repo 'lbterm\perf')
  )
  if (-not (Test-Path (Join-Path $Bin 'lbterm-gui.exe'))) { throw "no build in $Bin (lbterm-gui.exe): run lbterm.ps1 build first, or pass -Bin" }
  $Bin = (Resolve-Path $Bin).Path
  $entries = @(Get-E2eEntries $PerfDir)
  if ($Smoke) {
    $smokeRuns = @(Get-E2eSmoke -Entries $entries -File (Join-Path $PerfDir 'smoke.json'))
    $ownRuns = @(Select-E2eOwn -Entries $entries -Files (Get-ChangedFiles $script:Repo))
    $onlyRuns = if ($Only) { @(Select-E2eOnly $entries $Only) } else { @() }
    "the smoke set: $(($smokeRuns | ForEach-Object Name) -join ', ')"
    "the scripts this branch changed: $(if ($ownRuns) { ($ownRuns | ForEach-Object Name) -join ', ' } else { 'none' })"
    if ($onlyRuns) { "and -Only's: $(($onlyRuns | ForEach-Object Name) -join ', ')" }
    $seen = [Collections.Generic.HashSet[string]]::new()
    $entries = @(@($smokeRuns) + @($ownRuns) + @($onlyRuns) | Where-Object { $seen.Add($_.Name) })
  } elseif ($Only) { $entries = @(Select-E2eOnly $entries $Only) }
  if ($Changed) {
    $files = Get-ChangedFiles $script:Repo
    $pick = Select-E2eChanged -Entries $entries -Files $files
    if ($pick.Uncovered) {
      "no Covers line, so -Changed can't tell what they test: $(($pick.Uncovered | ForEach-Object Name) -join ', ')"
    }
    "$($files.Count) changed files pick $($pick.Selected.Count) of $($entries.Count): $(($pick.Selected | ForEach-Object Name) -join ', ')"
    $entries = @($pick.Selected)
  }
  $started = Get-Date
  $run = $started.ToString('yyyyMMdd-HHmmss')
  $runDir = Join-Path $Root $run
  New-Item -ItemType Directory -Force $runDir | Out-Null
  $commit = (git -C $script:Repo rev-parse --short=8 HEAD 2>$null)
  "e2e run $run against $Bin, $($entries.Count) scripts, output in $runDir"
  $results = [Collections.Generic.List[object]]::new()
  $stopped = $null
  :scripts foreach ($e in $entries) {
    $attempts = [Collections.Generic.List[object]]::new()
    for ($n = 0; $n -le $Retry; $n++) {
      if (Test-WorkstationLocked) { $stopped = "the workstation was locked (LogonUI.exe) before $($e.Name)"; break scripts }
      if ($asleep = Get-DisplaysAsleep) { $stopped = "$asleep, before $($e.Name)"; break scripts }
      $dir = Join-Path $runDir $e.Name
      # An earlier attempt keeps its folder as <name>.<n>
      if ($n -gt 0 -and (Test-Path $dir)) { Rename-AttemptFolder $dir "$($e.Name).$n" ; $attempts[-1].Dir = Join-Path $runDir "$($e.Name).$n" }
      $a = Invoke-E2eAttempt -Entry $e -Dir $dir -Bin $Bin -TimeoutSec ($TimeoutMin * 60) -WaitIdle:$WaitIdle
      if (-not $a.Pass -and (Test-WorkstationLocked)) { $stopped = "the workstation was locked (LogonUI.exe) while $($e.Name) ran: its failure isn't counted ($($a.Dir))"; break scripts }
      if (-not $a.Pass -and ($asleep = Get-DisplaysAsleep)) { $stopped = "$asleep while $($e.Name) ran: its failure isn't counted ($($a.Dir))"; break scripts }
      $attempts.Add($a)
      $state = if ($a.Pass -and $a.Unjudged) { 'unjudged' } elseif ($a.Pass) { 'pass' } elseif ($a.TimedOut) { 'timeout' } else { 'fail' }
      "  $($e.Name)$(if ($n) { " (again, $n)" }): $state in $($a.Seconds) s$(if (-not $a.Pass -and $a.Failures) { ': ' + ($a.Failures | Select-Object -First 1) })"
      if ($a.Pass) { break }
    }
    $results.Add([pscustomobject]@{ Name = $e.Name; Script = $e.Script; Args = $e.Args; Attempts = @($attempts); Result = (Get-E2eVerdict $attempts) })
    # Written after every script, so a run stopped halfway still has its report
    $report = New-E2eReport -Run $run -Bin $Bin -Commit $commit -Started $started -Results $results
    $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $runDir 'report.json') -Encoding utf8NoBOM
  }
  $script:LastReport = $null
  if ($stopped) {
    # The table says it too, when there is one
    if (-not $results.Count) { "STOPPED: $stopped" }
    $report = New-E2eReport -Run $run -Bin $Bin -Commit $commit -Started $started -Results $results -Stopped $stopped
    $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $runDir 'report.json') -Encoding utf8NoBOM
  }
  if (-not $results.Count) { if (-not $stopped) { 'nothing to run' }; return }
  $script:LastReport = $report
  ''
  Format-E2eTable $report
  "report: $(Join-Path $runDir 'report.json')"
}

# The report of the last Invoke-E2eRun in this session, or nothing
function Get-E2eLastReport { $script:LastReport }

# ---- A look judged by eye (lbterm.ps1 look; Lachlan's 3b of 2026-11-09) ----
#
# How something looks or moves is Lachlan's to judge, so a change to a look is shown to him once,
# before and after side by side, rather than held to a pixel threshold on every branch afterwards.

# One picture of a capture before and after a change: the two side by side, each with its label
# over it, on the window background. Each is drawn at its own pixel size (DrawImage with a size,
# so a bitmap's DPI never scales it). Returns $Out
function New-LookPair {
  param(
    [Parameter(Mandatory)][string]$Before,
    [Parameter(Mandatory)][string]$After,
    [Parameter(Mandatory)][string]$Out,
    [string]$BeforeLabel = 'before',
    [string]$AfterLabel = 'after',
    [int]$Gap = 16,
    [int]$Band = 28
  )
  Add-Type -AssemblyName System.Drawing
  $a = [Drawing.Bitmap]::FromFile($Before)
  $b = [Drawing.Bitmap]::FromFile($After)
  try {
    $pair = [Drawing.Bitmap]::new($a.Width + $Gap + $b.Width, $Band + [Math]::Max($a.Height, $b.Height))
    try {
      $g = [Drawing.Graphics]::FromImage($pair)
      try {
        $g.Clear([Drawing.Color]::FromArgb(255, 24, 24, 30))
        $font = [Drawing.Font]::new('Segoe UI', 11, [Drawing.GraphicsUnit]::Pixel)
        $brush = [Drawing.SolidBrush]::new([Drawing.Color]::White)
        $g.DrawString($BeforeLabel, $font, $brush, 4, 6)
        $g.DrawString($AfterLabel, $font, $brush, $a.Width + $Gap + 4, 6)
        $g.DrawImage($a, 0, $Band, $a.Width, $a.Height)
        $g.DrawImage($b, $a.Width + $Gap, $Band, $b.Width, $b.Height)
        $font.Dispose(); $brush.Dispose()
      } finally { $g.Dispose() }
      New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
      $pair.Save($Out, [Drawing.Imaging.ImageFormat]::Png)
    } finally { $pair.Dispose() }
  } finally { $a.Dispose(); $b.Dispose() }
  $Out
}

# The captures two runs of a script both saved (the PNGs at the top of each output folder), by name
function Get-LookCaptures([Parameter(Mandatory)][string]$BeforeDir, [Parameter(Mandatory)][string]$AfterDir) {
  @(Get-ChildItem -LiteralPath $BeforeDir -Filter *.png -File -ErrorAction SilentlyContinue |
      Where-Object { Test-Path -LiteralPath (Join-Path $AfterDir $_.Name) } | Sort-Object Name | ForEach-Object Name)
}

# lbterm.ps1 look: each script -Only names run once on the build in $Against (before) and once on
# $Bin (after), in turn, and every capture both runs saved made one picture, the two side by side
# (target\look\<run>\<run name>-<capture>.png). A script's own checks are said but judge nothing
# here: the pairs are for Lachlan's eye, shown to him with show-image. It stops on a locked PC,
# whose captures leave DirectComposition's content out (testing.md, "A locked PC")
function Invoke-LookRun {
  param(
    [Parameter(Mandatory)][string]$Bin,
    [Parameter(Mandatory)][string]$Against,
    [Parameter(Mandatory)][string[]]$Only,
    [string]$BeforeLabel = 'before',
    [string]$AfterLabel = 'after',
    [int]$TimeoutMin = 10,
    [string]$Root = (Join-Path $script:Repo 'target\look'),
    [string]$PerfDir = (Join-Path $script:Repo 'lbterm\perf')
  )
  foreach ($b in $Bin, $Against) {
    if (-not (Test-Path (Join-Path $b 'lbterm-gui.exe'))) { throw "no build in $b (lbterm-gui.exe)" }
  }
  $Bin = (Resolve-Path $Bin).Path
  $Against = (Resolve-Path $Against).Path
  $entries = @(Select-E2eOnly @(Get-E2eEntries $PerfDir) $Only)
  $runDir = Join-Path $Root (Get-Date).ToString('yyyyMMdd-HHmmss')
  "look: $($entries.Count) runs, before $BeforeLabel ($Against), after $AfterLabel ($Bin), in $runDir"
  $pairs = [Collections.Generic.List[string]]::new()
  foreach ($e in $entries) {
    $dirs = @{}
    foreach ($side in 'before', 'after') {
      if (Test-WorkstationLocked) { throw "the workstation is locked (LogonUI.exe): captures now leave DirectComposition's content out, so the pairs would show nothing of it" }
      $dirs[$side] = Join-Path $runDir "$($e.Name)\$side"
      $build = if ($side -eq 'before') { $Against } else { $Bin }
      $a = Invoke-E2eAttempt -Entry $e -Dir $dirs[$side] -Bin $build -TimeoutSec ($TimeoutMin * 60)
      $said = if ($a.Pass) { 'its checks passed' } else { "its checks failed ($(@($a.Failures)[0]))" }
      "  $($e.Name), ${side}: $said"
    }
    foreach ($n in Get-LookCaptures $dirs.before $dirs.after) {
      $out = Join-Path $runDir "$($e.Name)-$n"
      [void]$pairs.Add((New-LookPair -Before (Join-Path $dirs.before $n) -After (Join-Path $dirs.after $n) -Out $out -BeforeLabel $BeforeLabel -AfterLabel $AfterLabel))
    }
  }
  if (-not $pairs.Count) { 'no capture both runs saved: nothing to show'; return }
  "$($pairs.Count) pairs. Show them to Lachlan:"
  "  show-image $(($pairs | ForEach-Object { '"' + ($_ -replace '\\', '/') + '"' }) -join ' ')"
}

#endregion
