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
  $script:Test = [pscustomobject]@{
    Name       = $Name
    Repo       = $script:Repo
    Bin        = $Bin
    Gui        = Join-Path $Bin 'lbterm-gui.exe'
    Cli        = Join-Path $Bin 'lbterm.exe'
    MuxServer  = Join-Path $Bin 'lbterm-mux-server.exe'
    Out        = $Out
    Class      = $(if ($Class) { $Class } else { "com.lachlanb.lbterm.perf-$Name" })
    Clock      = [Diagnostics.Stopwatch]::StartNew()
    Failures   = [Collections.Generic.List[string]]::new()
    Captures   = [Collections.Generic.List[string]]::new()
    Windows    = [Collections.Generic.List[object]]::new()
    Classes    = [Collections.Generic.HashSet[string]]::new()
    Foreground = [Collections.Generic.HashSet[uint32]]::new()
    Stolen     = $null
    Current    = $null
    LastState  = $null
    Hold       = $null
    Done       = $false
  }
  $script:Test
}

function Get-Test { if (-not $script:Test) { throw 'no test started: Start-Test first' }; $script:Test }

# A failure, kept for the end (the test goes on)
function Add-Failure([Parameter(Mandatory)][string]$Message) { (Get-Test).Failures.Add($Message) }

# A file in the output folder the result names as a capture
function Add-Capture([Parameter(Mandatory)][string]$Path) {
  $t = Get-Test
  $rel = if ($Path.StartsWith($t.Out, [StringComparison]::OrdinalIgnoreCase)) { $Path.Substring($t.Out.Length).TrimStart('\') } else { $Path }
  if (-not $t.Captures.Contains($rel)) { $t.Captures.Add($rel) }
}

# result.json's shape
function ConvertTo-TestResult {
  param([Parameter(Mandatory)][string]$Name, [string[]]$Failures = @(), [double]$Seconds, [string[]]$Captures = @())
  $f = @($Failures | Where-Object { $_ })
  [ordered]@{
    name     = $Name
    pass     = $f.Count -eq 0
    failures = $f
    seconds  = [Math]::Round($Seconds, 1)
    captures = @($Captures | Where-Object { $_ })
  }
}

function Write-TestResult([Parameter(Mandatory)]$Result, [Parameter(Mandatory)][string]$Path) {
  [IO.File]::WriteAllText($Path, ($Result | ConvertTo-Json -Depth 4))
}

# Whether a test window ever had the foreground: the processes seen owning it, against the
# test's own
function Test-ForegroundTaken {
  $t = Get-Test
  $ours = @($t.Windows | ForEach-Object { [uint32]$_.Pid })
  @($t.Foreground | Where-Object { $_ -in $ours })
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
}

# The end of a test: result.json, the failures (FAIL lines) or -Passed, and exit 1 or 0
function Complete-Test([string]$Passed) {
  $t = Get-Test
  $stole = Test-ForegroundTaken
  if ($stole) { $t.Failures.Add("a test window took the foreground (pid $($stole -join ', '))$(if ($t.Stolen) { ", first $($t.Stolen)" })") }
  $crossed = [LbtermE2E.Win]::Crossed()
  if ($t.Failures.Count -and $crossed.Count) {
    "NOTE: the real pointer was over a test window between $($crossed[0]) and $($crossed[-1]): its hover can explain a failure, so rerun with the pointer elsewhere"
  }
  $result = ConvertTo-TestResult -Name $t.Name -Failures $t.Failures -Seconds $t.Clock.Elapsed.TotalSeconds -Captures $t.Captures
  Write-TestResult $result (Join-Path $t.Out 'result.json')
  $t.Done = $true
  if ($t.Captures.Count) { "captures in $($t.Out)" }
  if (-not $result.pass) { $t.Failures | ForEach-Object { "FAIL $_" }; exit 1 }
  if ($Passed) { $Passed }
  exit 0
}

# The test's body, then always Close-Test and Complete-Test: a throw in it is a failure, with
# the line it came from
function Invoke-Test {
  # -Finally runs after the body however it ended, before the windows close (to write a log)
  param([Parameter(Mandatory, Position = 0)][scriptblock]$Body, [string]$Passed, [switch]$Keep, [scriptblock]$Finally)
  $t = Get-Test
  try { & $Body }
  catch {
    # The test script's own line, not the library's, when the throw came from in here
    $frame = @($_.ScriptStackTrace -split "`n" | Where-Object { $_ -match ', (.+\.ps1): line (\d+)' -and $_ -notmatch 'harness\.psm1' }) | Select-Object -First 1
    $at = if ($frame -and $frame -match ', (.+\.ps1): line (\d+)') { " ($([IO.Path]::GetFileName($Matches[1])):$($Matches[2]))" }
    elseif ($_.InvocationInfo.ScriptName) { " ($([IO.Path]::GetFileName($_.InvocationInfo.ScriptName)):$($_.InvocationInfo.ScriptLineNumber))" } else { '' }
    $t.Failures.Add("$($_.Exception.Message)$at")
  }
  finally {
    if ($Finally) { try { & $Finally } catch { $t.Failures.Add("-Finally: $($_.Exception.Message)") } }
    Close-Test -Keep:$Keep
  }
  Complete-Test -Passed $Passed
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
  param([int]$ProcessId, [IntPtr]$Handle, [string]$Class, [string]$Config, [string]$Domain, [string]$ServerSocket, [string]$StateFile)
  $scale = if ($Handle -ne [IntPtr]::Zero) { [LbtermE2E.Win]::GetDpiForWindow($Handle) / 96.0 } else { 1.0 }
  if ($scale -le 0) { $scale = 1.0 }
  [pscustomobject]@{
    Pid          = $ProcessId
    Handle       = $Handle
    Class        = $Class
    Config       = $Config
    Domain       = $Domain
    Socket       = Get-GuiSocket $ProcessId
    ServerSocket = $ServerSocket
    StateFile    = $StateFile
    Log          = Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-gui.exe-log-$ProcessId.txt"
    Scale        = $scale
    Opened       = Get-Date
  }
}

# Takes a GUI process the test didn't open itself (one a restart started) as one of its windows,
# once it shows a window; it's closed with the rest
function Add-TestWindow {
  param([Parameter(Mandatory)][int]$ProcessId, [object]$Like, [int]$TimeoutSec = 20)
  $t = Get-Test
  $h = Wait-Window -Id $ProcessId -Seconds $TimeoutSec
  $w = New-WindowInfo -ProcessId $ProcessId -Handle $h -Class $Like.Class -Config $Like.Config -Domain $Like.Domain -ServerSocket $Like.ServerSocket -StateFile $Like.StateFile
  $t.Windows.Add($w)
  Add-Content (Join-Path $t.Out 'pids.txt') $ProcessId
  $w
}

# Opens a test window and waits for it to show. Its config is one of: -ConfigFile (the script
# wrote it), -NoConfig (-n: none, crash restore off), or else New-TestConfig (Lachlan's
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
  $n = $t.Windows.Count
  if (-not $Class) { $Class = $t.Class }
  [void]$t.Classes.Add($Class)
  if (-not $Exe) { $Exe = $t.Gui }
  $cfgArgs = if ($ConfigFile) { "--config-file $(ConvertTo-CommandLineArg $ConfigFile)" }
  elseif ($NoConfig) { '-n --config lbterm_crash_restore=false' }
  else {
    # In a folder of its own: WezTerm watches a config's folder, and captures, the state file or
    # anything else written beside it would reload the window
    $ConfigFile = Join-Path $t.Out "config\test$(if ($n) { "-$n" }).lua"
    New-Item -ItemType Directory -Force (Split-Path $ConfigFile) | Out-Null
    New-TestConfig -Path $ConfigFile -Lua $Lua
    "--config-file $(ConvertTo-CommandLineArg $ConfigFile)"
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
  if ($Log) { $envs.WEZTERM_LOG = $Log }
  if ($UserProfile) { New-Item -ItemType Directory -Force $UserProfile | Out-Null; $envs.USERPROFILE = $UserProfile }
  foreach ($k in $Environment.Keys) { $envs[$k] = $Environment[$k] }
  # LBTERM_E2E_WAIT_IDLE (lbterm.ps1 e2e -WaitIdle) asks it of every window
  if ($WaitIdle -or $env:LBTERM_E2E_WAIT_IDLE) { Wait-Idle }
  $arguments = (@($cfgArgs, $sets, $Before, $verb, $tail) | Where-Object { $_ }) -join ' '
  $start = @{ Exe = $Exe; Arguments = $arguments; Environment = $envs }
  if ($WorkingDirectory) { $start.WorkingDirectory = $WorkingDirectory }
  $procId = Start-Unfocused @start
  Add-Content (Join-Path $t.Out 'pids.txt') $procId
  $h = Wait-Window -Id $procId -Seconds $TimeoutSec
  $w = New-WindowInfo -ProcessId $procId -Handle $h -Class $Class -Config $ConfigFile -Domain $Domain -ServerSocket $Socket -StateFile $stateFile
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

# Whether a test window's log has a line matching $Pattern, waiting up to $TimeoutSec
function Wait-LogLine($Window, [Parameter(Mandatory)][string]$Pattern, [double]$TimeoutSec = 10) {
  $log = (Resolve-Window $Window).Log
  Wait-Until { Select-String -Path $log -Pattern $Pattern -Quiet -ErrorAction SilentlyContinue } -TimeoutSec $TimeoutSec
}

# The lines of a test window's log matching $Pattern
function Get-LogLines($Window, [Parameter(Mandatory)][string]$Pattern) {
  $log = (Resolve-Window $Window).Log
  if (-not (Test-Path $log)) { return @() }
  @(Select-String -Path $log -Pattern $Pattern | ForEach-Object Line)
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
function Get-Panes([object]$Window, [switch]$Server) { @(Invoke-Cli list --format json -Json -Window $Window -Server:$Server) }

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
}
$script:MK = @{ None = 0; L = 1; R = 2; M = 0x10 }

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

# Keys pressed and let go, by name ('Enter', 'Down', 'F5', 'Y') or code, -GapMs apart. The
# navigation keys go as extended keys, as a keyboard's own do.
function Send-Key {
  param([Parameter(Mandatory, Position = 0)]$Window, [Parameter(Mandatory, Position = 1)][object[]]$Key, [int]$GapMs = 0)
  $h = Resolve-Handle $Window
  foreach ($k in $Key) {
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
  if ($result) {
    $failures = @($result.failures)
    $captures = @($result.captures)
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
  [pscustomobject]@{
    Pass = $pass; Exit = $Exit; Seconds = [Math]::Round($Seconds, 1); TimedOut = $TimedOut
    Dir = $Dir; Failures = @($failures); Captures = @($captures)
  }
}

# pass: the first run passed. flaky: a later one did. fail: none did.
function Get-E2eVerdict([object[]]$Attempts) {
  if (-not $Attempts) { return 'fail' }
  if ($Attempts[0].Pass) { return 'pass' }
  if (@($Attempts | Where-Object Pass).Count) { return 'flaky' }
  'fail'
}

# report.json's shape: the run, the build, and each entry with its attempts
function New-E2eReport {
  param([string]$Run, [string]$Bin, [string]$Commit, [datetime]$Started, [object[]]$Results)
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
      attempts = @($r.Attempts | ForEach-Object { [ordered]@{ pass = $_.Pass; exit = $_.Exit; seconds = $_.Seconds; timedOut = $_.TimedOut; out = $_.Dir; failures = @($_.Failures) } })
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
      flaky = @($entries | Where-Object { $_.result -eq 'flaky' }).Count
      fail  = @($entries | Where-Object { $_.result -eq 'fail' }).Count
    }
    scripts  = $entries
  }
}

# The short table the runner prints: script, result, seconds, flaky
function Format-E2eTable($Report) {
  $w = [Math]::Max(6, (@($Report.scripts | ForEach-Object { $_.name.Length }) + 6 | Measure-Object -Maximum).Maximum)
  ('{0}  {1,-6}  {2,7}  {3}' -f 'script'.PadRight($w), 'result', 'seconds', 'flaky')
  foreach ($s in $Report.scripts) {
    '{0}  {1,-6}  {2,7:N1}  {3}' -f $s.name.PadRight($w), $s.result, $s.seconds, $(if ($s.flaky) { 'flaky' } else { '' })
  }
  $m = $Report.summary
  "$($m.pass) passed, $($m.flaky) flaky, $($m.fail) failed in $([Math]::Round($Report.seconds / 60, 1)) min"
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

# lbterm.ps1 e2e: the scripts one at a time, a failure run again up to $Retry times (flaky if a
# rerun passes), report.json and the table at the end. Returns the report.
function Invoke-E2eRun {
  param(
    [Parameter(Mandatory)][string]$Bin,
    [string[]]$Only,
    [switch]$Changed,
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
  if ($Only) { $entries = @(Select-E2eOnly $entries $Only) }
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
  foreach ($e in $entries) {
    $attempts = [Collections.Generic.List[object]]::new()
    for ($n = 0; $n -le $Retry; $n++) {
      $dir = Join-Path $runDir $e.Name
      # An earlier attempt keeps its folder as <name>.<n>
      if ($n -gt 0 -and (Test-Path $dir)) { Rename-Item $dir "$($e.Name).$n" ; $attempts[-1].Dir = Join-Path $runDir "$($e.Name).$n" }
      $a = Invoke-E2eAttempt -Entry $e -Dir $dir -Bin $Bin -TimeoutSec ($TimeoutMin * 60) -WaitIdle:$WaitIdle
      $attempts.Add($a)
      $state = if ($a.Pass) { 'pass' } elseif ($a.TimedOut) { 'timeout' } else { 'fail' }
      "  $($e.Name)$(if ($n) { " (again, $n)" }): $state in $($a.Seconds) s$(if (-not $a.Pass -and $a.Failures) { ': ' + ($a.Failures | Select-Object -First 1) })"
      if ($a.Pass) { break }
    }
    $results.Add([pscustomobject]@{ Name = $e.Name; Script = $e.Script; Args = $e.Args; Attempts = @($attempts); Result = (Get-E2eVerdict $attempts) })
    # Written after every script, so a run stopped halfway still has its report
    $report = New-E2eReport -Run $run -Bin $Bin -Commit $commit -Started $started -Results $results
    $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $runDir 'report.json') -Encoding utf8NoBOM
  }
  $script:LastReport = $null
  if (-not $results.Count) { 'nothing to run'; return }
  $script:LastReport = $report
  ''
  Format-E2eTable $report
  "report: $(Join-Path $runDir 'report.json')"
}

# The report of the last Invoke-E2eRun in this session, or nothing
function Get-E2eLastReport { $script:LastReport }

#endregion
