#Requires -Version 7
# Helper: what each screen's compositor counts do, frame by frame (lbterm/PERF.md, "Frames missed during motion"); run by hand, opens no window; not a test
<#
Reads DirectComposition's compositor clock statistics as wezterm-gui/src/present.rs's Clock does
(DCompositionGetFrameId for the newest completed frame, DCompositionGetStatistics and
DCompositionGetTargetStatistics for every frame since, every screen's target), for -Seconds, and
says for each screen what its counts did: the frames read, its refresh (vblankDuration), its
refreshes by its own counter (presentedStats.refreshCount) against those by its vblank times
(presentedStats.time, a refresh apart), the presents DWM made to it, and every frame at which the
counter and the times disagreed (a counter that jumped while the time moved on its grid): the
rebases PITFALLS 166 is about. Each screen is named by its device name (\\.\DISPLAYn) and bounds,
matched to its target by adapter and VidPn source (D3DKMTOpenAdapterFromGdiDisplayName), as
present.rs matches a window's monitor.

A probe of DWM's counts, not of LBTerm: it opens no window and starts nothing, so it takes no
lock and no warden job (the warden's hook asks for one at any lbterm/perf path: end the Bash
tool's line with `# warden:light`). The counts are the screens', whoever presents to them, so a
test window of another session's on the right-most screen moves that screen's.

  pwsh -NoProfile -File lbterm/perf/screen-counts.ps1 [-Seconds 20] [-Out <file.json>] [-Rows <file.csv>]

-Rows keeps every frame's raw counts, one line a frame and screen (id, start, period, source, hr,
outstanding, present_time, vblank, the presented counts and time, the completed counts).
#>
param([double]$Seconds = 10, [string]$Out, [string]$Rows)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Collections.Generic; using System.Threading;
public static class LbtermScreenCounts {
  [StructLayout(LayoutKind.Sequential)] public struct TID { public uint dLow; public int dHigh; public uint rLow; public int rHigh; public uint source; public uint target; public uint unique; }
  [StructLayout(LayoutKind.Sequential)] public struct FST { public ulong start, targetTime, period; }
  [StructLayout(LayoutKind.Sequential)] public struct CST { public uint presents, refreshes, virt; public ulong time; }
  [StructLayout(LayoutKind.Sequential)] public struct TST { public uint outstanding; public ulong presentTime, vblank; public CST presented, completed; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct OPEN { [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string name; public uint hAdapter; public uint luidLow; public int luidHigh; public uint source; }
  [DllImport("dcomp.dll")] static extern int DCompositionGetFrameId(int kind, out ulong id);
  [DllImport("dcomp.dll")] static extern int DCompositionGetStatistics(ulong id, out FST stats, uint n, [Out] TID[] ids, out uint actual);
  [DllImport("dcomp.dll")] static extern int DCompositionGetTargetStatistics(ulong id, ref TID target, out TST stats);
  [DllImport("gdi32.dll")] public static extern int D3DKMTOpenAdapterFromGdiDisplayName(ref OPEN o);
  // COMPOSITION_FRAME_ID_COMPLETED, as present.rs's FRAME_COMPLETED
  const int Completed = 2;
  public static List<ulong[]> Run(double seconds) {
    var rows = new List<ulong[]>();
    ulong latest; if (DCompositionGetFrameId(Completed, out latest) < 0) throw new Exception("DCompositionGetFrameId failed: no compositor clock");
    FST fs; var ids = new TID[16]; uint n;
    if (DCompositionGetStatistics(latest, out fs, 16, ids, out n) < 0) throw new Exception("DCompositionGetStatistics failed");
    int targets = (int)Math.Min(n, 16u);
    var end = DateTime.UtcNow.AddSeconds(seconds); ulong last = latest;
    while (DateTime.UtcNow < end) {
      Thread.Sleep(2);
      ulong now; if (DCompositionGetFrameId(Completed, out now) < 0) continue;
      for (ulong id = last + 1; id <= now; id++) {
        uint m; if (DCompositionGetStatistics(id, out fs, 0, null, out m) < 0) continue;
        for (int t = 0; t < targets; t++) {
          TST ts; var tid = ids[t];
          int hr = DCompositionGetTargetStatistics(id, ref tid, out ts);
          if (hr < 0) continue;
          rows.Add(new ulong[] { id, fs.start, fs.period, tid.source, tid.dLow, (ulong)(uint)tid.dHigh, ts.outstanding, ts.presentTime, ts.vblank, ts.presented.presents, ts.presented.refreshes, ts.presented.time, ts.completed.presents, ts.completed.refreshes });
        }
      }
      last = now;
    }
    return rows;
  }
}
'@

Add-Type -AssemblyName System.Windows.Forms
# Each screen's adapter and source, so a target is named by the screen it is
$screens = @{}
foreach ($s in [System.Windows.Forms.Screen]::AllScreens) {
  $o = New-Object 'LbtermScreenCounts+OPEN'
  $o.name = $s.DeviceName
  if ([LbtermScreenCounts]::D3DKMTOpenAdapterFromGdiDisplayName([ref]$o) -eq 0) {
    $screens["$($o.luidLow):$($o.luidHigh):$($o.source)"] = "$($s.DeviceName) $($s.Bounds.Width)x$($s.Bounds.Height) at $($s.Bounds.X),$($s.Bounds.Y)$(if ($s.Primary) { ', primary' })"
  }
}

$all = [LbtermScreenCounts]::Run($Seconds)
if ($Rows) {
  $lines = @('id,start,period,source,adapter_low,adapter_high,outstanding,present_time,vblank,presents,refreshes,time,completed_presents,completed_refreshes')
  $lines += $all | ForEach-Object { $_ -join ',' }
  Set-Content -LiteralPath $Rows -Value $lines
}

$f = [double][System.Diagnostics.Stopwatch]::Frequency
$report = foreach ($g in ($all | Group-Object { "$($_[4]):$([int][uint32]$_[5]):$($_[3])" })) {
  $v = @($g.Group | Sort-Object { $_[0] })
  $vb = [double]$v[0][8]
  $first = $v[0]; $last = $v[-1]
  $byCounter = [int64]([uint32](([int64]$last[10] - [int64]$first[10]) -band 0xffffffff))
  $byTime = if ($vb -gt 0) { [math]::Round(([double]$last[11] - [double]$first[11]) / $vb) } else { $null }
  $rebases = [System.Collections.Generic.List[object]]::new()
  for ($i = 1; $i -lt $v.Count; $i++) {
    $a = $v[$i - 1]; $b = $v[$i]
    if ($vb -le 0) { break }
    $dTime = [math]::Round(([double]$b[11] - [double]$a[11]) / $vb)
    $dCount = [int64]$b[10] - [int64]$a[10]
    if ($dCount -ne $dTime) {
      $rebases.Add([ordered]@{ frame = $b[0]; ms = [math]::Round(([double]$b[1] - [double]$first[1]) * 1000 / $f, 1); counter = $dCount; time = $dTime; presents = [int64]$b[9] - [int64]$a[9]; outstanding = $b[6] })
    }
  }
  [ordered]@{
    screen = $(if ($screens[$g.Name]) { $screens[$g.Name] } else { "source $($first[3]) (no screen of this PC's names it)" })
    source = $first[3]
    frames = $v.Count
    seconds = [math]::Round(([double]$last[1] - [double]$first[1]) / $f, 2)
    refresh_ms = [math]::Round($vb * 1000 / $f, 3)
    refreshes_by_counter = $byCounter
    refreshes_by_time = $byTime
    presents = [int64]$last[9] - [int64]$first[9]
    disagreements = $rebases.Count
    rebases = @($rebases | Select-Object -First 20)
  }
}
foreach ($r in $report) {
  '{0}: {1} frames over {2} s, a refresh {3} ms; refreshes {4} by its counter, {5} by its times; {6} presents; the counter and the times disagreed at {7} frames' -f $r.screen, $r.frames, $r.seconds, $r.refresh_ms, $r.refreshes_by_counter, $r.refreshes_by_time, $r.presents, $r.disagreements
  foreach ($x in $r.rebases) { '  frame {0} at {1} ms: the counter {2}, the times {3} (presents {4}, outstanding {5})' -f $x.frame, $x.ms, $x.counter, $x.time, $x.presents, $x.outstanding }
}
if ($Out) { $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Out }
