#Requires -Version 7
# Helper: what covers the screen test windows open on, before a run that needs it seen (lbterm/PITFALLS.md, 182); run by hand, opens no window; not a test
<#
Lists every visible, unminimised, uncloaked top-level window over the right-most screen (the one
the harness opens test windows on: the screen whose right edge is furthest right, which Windows
numbers as it likes, DISPLAY1 on this PC since at least 2026-11-08), with its process, its class,
the share of the screen it covers (its DWM extended frame bounds against the screen's bounds) and
whether it's topmost (WS_EX_TOPMOST), and says whether the PC is locked (LogonUI running).

A run that measures what reached the screen (`budgets`' `missed` and screen times, the walk of
perf/budgets.ps1 -Log) judges nothing while the test window is covered, and the harness's own
Get-WindowCover needs a window to ask about: this says it beforehand, so a stage that finds the
screen covered reports the lines to run rather than holding the measuring lock to judge nothing.
Another session's test window there shows too (its class com.lachlanb.lbterm.*); the desktop
(explorer's Progman) always covers it all and is no cover.

It opens no window and starts nothing, so it takes no lock and no warden job (the warden's hook
asks for one at any lbterm/perf path: end the Bash tool's line with `# warden:light`).

  pwsh -NoProfile -File lbterm/perf/screen-cover.ps1 [-MinShare 0.02]
#>
param([double]$MinShare = 0.02)
Add-Type -AssemblyName System.Windows.Forms
if (-not ('LbtermScreenCover' -as [type])) {
  Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public static class LbtermScreenCover {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int s);
  // Each window: its handle, its extended frame bounds, its class, its process and whether it's topmost
  public static List<object[]> All() {
    var list = new List<object[]>();
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h) || IsIconic(h)) return true;
      int cloaked; DwmGetWindowAttribute(h, 14, out cloaked, 4); if (cloaked != 0) return true;
      RECT r; DwmGetWindowAttribute(h, 9, out r, 16);
      var sb = new StringBuilder(256); GetClassName(h, sb, 256); uint pid; GetWindowThreadProcessId(h, out pid);
      int ex = GetWindowLong(h, -20);
      list.Add(new object[] { h, r.L, r.T, r.R, r.B, sb.ToString(), (int)pid, (ex & 8) != 0 });
      return true; }, IntPtr.Zero);
    return list;
  }
}
'@
}
$screen = [System.Windows.Forms.Screen]::AllScreens | Sort-Object { $_.Bounds.Right } | Select-Object -Last 1
$b = $screen.Bounds
"the right-most screen: $($screen.DeviceName) at x $($b.X), $($b.Width)x$($b.Height)"
$cover = 0.0
foreach ($w in [LbtermScreenCover]::All()) {
  $l = [Math]::Max($w[1], $b.Left); $t = [Math]::Max($w[2], $b.Top); $r = [Math]::Min($w[3], $b.Right); $bt = [Math]::Min($w[4], $b.Bottom)
  if ($r -le $l -or $bt -le $t) { continue }
  $share = ($r - $l) * ($bt - $t) / ($b.Width * $b.Height)
  if ($share -lt $MinShare) { continue }
  $exe = (Get-Process -Id $w[6] -ErrorAction SilentlyContinue).ProcessName
  $what = if ($w[5] -eq 'Progman' -and $exe -eq 'explorer') { ' (the desktop, no cover)' } elseif ($w[5] -like 'com.lachlanb.lbterm*') { ' (a test window)' } else { $cover = [Math]::Max($cover, $share); '' }
  '{0,-22} {1,-40} {2,4:P0} topmost {3}{4}' -f $exe, $w[5], $share, $w[7], $what
}
"covered: $([Math]::Round($cover * 100))% by another program's window; locked: $([bool](Get-Process -Name LogonUI -ErrorAction SilentlyContinue))"
