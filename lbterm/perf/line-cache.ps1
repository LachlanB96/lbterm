#Requires -Version 7
# Covers: lfucache/src/lib.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/mod.rs, wezterm-gui/src/termwindow/render/screen_line.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/glyphcache.rs, wezterm-gui/src/counters.rs, config/src/config.rs
<#
What the line caches keep (lbterm/PERF.md, "What the caches keep"; finding fonts-8): a test window
drawn as Lachlan's (New-TestConfig), its four line caches at upstream's 1024 entries, filled with
lines looked up many times, then a tab it hasn't shown before.

1. -Tabs tabs, each a screen of text of its own that then stays still, shown in turn -ShowSec
   each, -Cycles times round, the window made to paint about -Fps frames a second meanwhile
   (InvalidateRect from here, which is no input: Windows sends the window a WM_PAINT and it paints
   a frame, every line of the tab looked up in the caches). -Tabs screens of lines are more than
   the 1024 entries hold, so the caches end full of lines each seen a hundred times or more.
2. A tab never shown before, the same: every frame from the one that first shows it is gone
   through (the harness's frame recorder) for -ProbeSec, and the lines each laid out again
   counted (the state file's work.lines: a line that missed the line quad cache and was laid out
   as quads again). Its lines are all new, so the frame that first shows it lays out a screen of
   them; nothing in the tab changes after, so no frame after it should lay out any. In a cache
   whose every entry had been looked up many times lately, upstream's rule put each entry put in
   out with the next, so a frame's new lines were laid out again frame after frame (finding
   fonts-8, which lfucache's tests hold); a window's frames seldom bring its caches there, since
   the decay takes a line seen fewer than about a tenth of a cache's length of lookups to nothing,
   and this holds that a new tab's lines stay.

What the revisits of the second round laid out is said too, not judged: -Tabs screens don't all
fit in 1024 entries, so a tab is laid out again when it comes round. With metrics (a build with
them), the caches' hits, misses and evictions before and over the probe are said, and kept in
lines.json (metrics' `caches`).

It fails when the frames after the first that showed the new tab lay out more than -PerFrame
lines each on average, or when fewer than 10 such frames came.

-Measure: what a frame of lines all found in the caches costs, instead (lbterm/PERF.md, "What
upstream's metrics cost"), to compare builds: a window of its own, one tab of two panes side by
side, each a screen of text that stays still, made to paint about -Fps frames a second for
-MeasureSec, -Runs times: the GUI thread's cycles and the process's a frame, by the state file's
frames. Every line a frame looks up is found, so what a frame costs is the lookups, the copying
of each line's quads into the frame and drawing it, and what upstream's metrics took for each.

  pwsh -File lbterm\perf\line-cache.ps1 [-Tabs 18] [-ShowSec 2] [-Cycles 2] [-Bin <build>]
  pwsh -File lbterm\perf\line-cache.ps1 -Measure [-Runs 3] [-MeasureSec 10] [-Bin <build>]
#>
param(
  [string]$Bin,
  [int]$Tabs = 18,
  [double]$ShowSec = 2,
  [int]$Cycles = 2,
  [double]$ProbeSec = 3,
  [int]$Fps = 50,
  [int]$Rows = 60,
  [double]$PerFrame = 1,
  [switch]$Measure,
  [int]$Runs = 3,
  [double]$MeasureSec = 10
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('line-cache' + $(if ($Measure) { '-measure' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LbtermLineCache {
  [DllImport("user32.dll")] static extern bool InvalidateRect(IntPtr h, IntPtr rect, bool erase);
  // A WM_PAINT for the window, as Windows sends one when it's uncovered: no input, no focus
  public static bool Paint(IntPtr h) { return InvalidateRect(h, IntPtr.Zero, false); }
}
'@

# Each tab's screen: lines of its own (its number in each), the cursor left under them, then
# nothing more printed
$tabScript = Join-Path $T.Out 'tab.ps1'
[IO.File]::WriteAllText($tabScript, @'
param([int]$N)
$rows = $Host.UI.RawUI.WindowSize.Height
$w = [Math]::Max(20, $Host.UI.RawUI.WindowSize.Width - 2)
$abc = 'the quick brown fox jumps over the lazy dog 0123456789 ' * 4
[Console]::Write("`e[2J`e[H")
for ($i = 1; $i -lt $rows; $i++) {
  $s = ("tab {0,2} line {1,2}: " -f $N, $i) + $abc.Substring(($N * 7 + $i) % 50)
  [Console]::WriteLine($s.Substring(0, [Math]::Min($w, $s.Length)))
}
while ($true) { Start-Sleep -Seconds 60 }
'@)

# The window painting about $Fps frames a second for $Seconds
function Invoke-Frames($Window, [double]$Seconds) {
  $h = [IntPtr][long]$Window.Handle
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $every = 1000.0 / $Fps
  $n = 0
  while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
    [void][LbtermLineCache]::Paint($h)
    $n++
    $wait = [int]($n * $every - $clock.Elapsed.TotalMilliseconds)
    if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
  }
}

function Get-Lines($State) { if ($State.work) { [long]$State.work.lines } else { $null } }

function Get-Caches($Window) {
  try { (Get-Metrics -Window $Window).caches } catch { $null }
}

if ($Measure) {
  Invoke-Test {
    $cfg = Join-Path $T.Out 'config\line-cache.lua'
    New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
    New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 200
config.initial_rows = $Rows
config.window_close_confirmation = 'NeverPrompt'
"@
    $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram -File $tabScript -ArgumentList 1) -TimeoutSec 30
    Wait-State { param($s) @($s.panes).Count -ge 1 } -What 'its pane' -TimeoutSec 30 | Out-Null
    $first = (Get-Panes)[0].pane_id
    Invoke-Cli split-pane --pane-id $first --right -Program (New-PwshProgram -File $tabScript -ArgumentList 2) -Check | Out-Null
    Wait-State { param($s) @($s.panes).Count -eq 2 } -What 'two panes' -TimeoutSec 30 | Out-Null
    Start-Sleep -Seconds 4
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    $lineCount = (@($sw.panes) | Measure-Object rows -Sum).Sum
    $rate = Get-CycleRate
    # Warm: every line laid out once and found from then on
    Invoke-Frames $w 1
    Get-QuietFrame -QuietMs 500 -TimeoutSec 10 -Window $w | Out-Null
    $results = [Collections.Generic.List[object]]::new()
    for ($r = 1; $r -le $Runs; $r++) {
      $null = try { Get-Metrics -Window $w -Reset } catch { $null }
      $s0 = Read-State
      $a = Get-CpuSnapshot -Window $w
      Invoke-Frames $w $MeasureSec
      Get-QuietFrame -QuietMs 300 -TimeoutSec 10 -Window $w | Out-Null
      $b = Get-CpuSnapshot -Window $w
      $s1 = Read-State
      # What upstream's histograms and the caches counted over the run, where the build has them
      $m = try { Get-Metrics -Window $w } catch { $null }
      $frames = [long]$s1.frame - [long]$s0.frame
      $laid = (Get-Lines $s1) - (Get-Lines $s0)
      $gui = ($b.ThreadCycles - $a.ThreadCycles) / $rate
      $cpu = ($b.Cycles - $a.Cycles) / $rate
      $one = [pscustomobject]@{
        run = $r; frames = $frames; lines_laid_out = $laid
        gui_thread_ms = [Math]::Round($gui, 1); cpu_ms = [Math]::Round($cpu, 1)
        gui_ms_a_frame = [Math]::Round($gui / [Math]::Max(1, $frames), 3); cpu_ms_a_frame = [Math]::Round($cpu / [Math]::Max(1, $frames), 3)
        histograms = $(if ($m) { @($m.histograms) } else { $null }); caches = $(if ($m) { $m.caches } else { $null })
      }
      $results.Add($one)
      Write-Host ("run {0}: {1} frames of {2} lines, {3} laid out again; the GUI thread {4} ms ({5} a frame), the process {6} ms ({7} a frame)" -f $r, $frames, $lineCount, $laid, $one.gui_thread_ms, $one.gui_ms_a_frame, $one.cpu_ms, $one.cpu_ms_a_frame)
      if ($frames -lt $MeasureSec * 10) { Add-Failure "run ${r}: only $frames frames in $MeasureSec s" }
    }
    $med = { param($k) $v = @($results | ForEach-Object $k | Sort-Object); $v[[int][Math]::Floor(($v.Count - 1) / 2)] }
    $T.Notes.Add("line-cache -Measure: $lineCount lines a frame, the GUI thread $(& $med 'gui_ms_a_frame') ms a frame at the median, the process $(& $med 'cpu_ms_a_frame')")
    [IO.File]::WriteAllText((Join-Path $T.Out 'measure.json'), ([pscustomobject]@{ build = (Read-State).build; lines = $lineCount; runs = @($results) } | ConvertTo-Json -Depth 6))
  } -Passed "line-cache -Measure: what a frame of cached lines costs, measured"
  return
}

Invoke-Test {
  $cfg = Join-Path $T.Out 'config\line-cache.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 100
config.initial_rows = $Rows
config.window_close_confirmation = 'NeverPrompt'
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
-- upstream's sizes, whatever this build's defaults
config.shape_cache_size = 1024
config.line_state_cache_size = 1024
config.line_quad_cache_size = 1024
config.line_to_ele_shape_cache_size = 1024
"@
  $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram -File $tabScript -ArgumentList 0) -TimeoutSec 30
  Wait-State { param($s) @($s.panes).Count -ge 1 } -What 'its pane' -TimeoutSec 30 | Out-Null
  $first = (Get-Panes)[0].pane_id
  for ($n = 1; $n -le $Tabs; $n++) {
    Invoke-Cli spawn --pane-id $first --no-activate -Program (New-PwshProgram -File $tabScript -ArgumentList $n) -Check | Out-Null
  }
  $want = $Tabs + 1
  Wait-State { param($s) @($s.tabs).Count -eq $want } -What "the $want tabs" -TimeoutSec 60 | Out-Null
  if ($null -eq (Get-Lines (Read-State))) { throw "this build's state file has no work.lines: it's from before the lines laid out were counted" }
  # Every tab's pwsh up and its screen printed
  Start-Sleep -Seconds 4
  $rowsShown = [int]@((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).panes)[0].rows

  # 1. The tabs in turn, all but the last, which is the probe's
  $revisits = [Collections.Generic.List[long]]::new()
  for ($c = 0; $c -lt $Cycles; $c++) {
    for ($i = 0; $i -lt $Tabs; $i++) {
      $before = Get-Lines (Read-State)
      Invoke-Cli activate-tab --pane-id $first --tab-index $i -Check | Out-Null
      Invoke-Frames $w $ShowSec
      if ($c -gt 0) { $revisits.Add((Get-Lines (Read-State)) - $before) }
    }
  }
  $mid = Get-Caches $w

  # 2. The tab never shown
  Get-Metrics -Window $w -Reset -ErrorAction SilentlyContinue | Out-Null
  $base = Get-Lines (Read-State)
  $rec = Start-FrameRecorder -Window $w
  try {
    Invoke-Cli activate-tab --pane-id $first --tab-index $Tabs -Check | Out-Null
    Invoke-Frames $w $ProbeSec
    Start-Sleep -Milliseconds 300
    $rec.Look()
    $frames = @($rec.Take())
  } finally { $rec.Dispose() }
  $caches = Get-Caches $w

  # Lines laid out in each frame from the first that showed the new tab (a frame the writer
  # dropped is in the next one's)
  $lines = [Collections.Generic.List[long]]::new()
  $shown = $false
  $last = $base
  foreach ($f in $frames) {
    $s = try { $f.Text | ConvertFrom-Json -Depth 32 } catch { $null }
    if (-not $s) { continue }
    $sw = Get-StateWindow $s -Hwnd ([long]$w.Handle)
    $now = Get-Lines $s
    if (-not $shown -and $sw -and [int]$sw.active_tab -eq $Tabs) { $shown = $true }
    if ($shown) { $lines.Add($now - $last) }
    $last = $now
  }
  if (-not $shown) { throw "no frame recorded showed tab $Tabs" }
  $after = @($lines | Select-Object -Skip 1)
  $sum = ($after | Measure-Object -Sum).Sum
  $mean = if ($after.Count) { [Math]::Round($sum / $after.Count, 2) } else { 0 }
  $revisitMean = if ($revisits.Count) { [Math]::Round(($revisits | Measure-Object -Average).Average, 1) } else { 0 }

  Write-Host "the window: $rowsShown rows a tab, $want tabs; $Cycles rounds of $Tabs tabs shown $ShowSec s each at about $Fps frames a second"
  Write-Host "revisits in the rounds after the first: lines laid out a visit $revisitMean ($(($revisits | Select-Object -First 18) -join ', '))"
  Write-Host "the new tab: $($lines.Count) frames from the first that showed it; lines laid out in the first $(if ($lines.Count) { $lines[0] } else { 0 }) (its screen), then $sum over $($after.Count) frames, $mean a frame: $(($after | Select-Object -First 40) -join ' ')"
  foreach ($pair in @(@('in the caches before the probe', $mid), @('over the probe', $caches))) {
    if ($pair[1]) { Write-Host "metrics' caches $($pair[0]): $(($pair[1].PSObject.Properties | ForEach-Object { "$($_.Name) $($_.Value.hits)/$($_.Value.misses)/$($_.Value.evictions)" }) -join ', ') (hits/misses/evictions)" }
  }
  $T.Notes.Add("line-cache: the new tab's frames after its first laid out $sum lines over $($after.Count) frames ($mean a frame); revisits $revisitMean lines a visit")
  [IO.File]::WriteAllText((Join-Path $T.Out 'lines.json'), ([pscustomobject]@{
    rows = $rowsShown; tabs = $want; probe_lines = @($lines); after_first = $sum; after_frames = $after.Count; per_frame = $mean
    revisits = @($revisits); caches_before = $mid; caches_probe = $caches
  } | ConvertTo-Json -Depth 8))

  if ($after.Count -lt 10) { Add-Failure "only $($after.Count) frames after the first that showed the new tab: too few to judge" }
  if ($mean -gt $PerFrame) { Add-Failure "the new tab's lines were laid out again: $sum lines over the $($after.Count) frames after the first ($mean a frame, over $PerFrame), though nothing in it changed: they were put out of the caches by each other" }
} -Passed "line-cache: a new tab's lines stay in caches full of lines seen many times, laid out once"
