#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/strip_swap.rs, lbterm-extras/src/strip.rs, lbterm-extras/src/summaries.rs, lbterm-motion/src/**, mux/src/lbterm_repaint.rs
<#
The window strip's answers (lbterm/FEEDBACK.md, render/strip.rs and render/strip_swap.rs) end to
end, in a test window that looks like Lachlan's LBTerm (New-TestConfig) with lbterm_summaries
pointed at a folder of its own and motion at -Slowdown:

1. A Claude session's summary on the strip (summary.png).
2. The worker writes a new one: the old words dissolve into the new ones over `swap` (frames
   between that are neither the old strip nor the new), and the row brightens past both before it
   settles on the new (swap.png, the frame furthest from both).
3. The pane split and zoomed with `lbterm cli zoom-pane`: a badge after the title says so
   (zoomed.png); unzoomed, it goes (unzoomed.png).
4. The config file saved: "Config reloaded" in the caption's place (reloaded.png), gone after
   strip::NOTICE_FOR (notice-gone.png).

Each step waits on what the state file (lbterm/STATE.md) says the strip shows, then compares the
pictures. Captures go to target\perf\strip-feedback.

  pwsh -File lbterm\perf\strip-feedback.ps1 [-Slowdown 4]
#>
param([double]$Slowdown = 4, [string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'strip-feedback' -Bin $Bin
$sums = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $sums | Out-Null

# The strip's rows: the top of the window, well clear of the panes' text
$strip = 44
function Distance($a, $b) { [LbtermE2E.Px]::LumDistance($a, $b, (Get-Rows 0 ($strip - 1) -Of $a)) }
function Light($a) { [LbtermE2E.Px]::Light($a, (Get-Rows 0 ($strip - 1) -Of $a)) }
# What the step does, settled (a notice may still be waiting to go), then the picture of it
function Shows([scriptblock]$what, [string]$name, [string]$desc) {
  Wait-Change $what -What $desc -Animating | Out-Null
  Get-Shot $w -Name $name
}

$sid = '3b8e1f60-4c2d-4a97-b5e1-9d0c7a6f2e84'
[IO.File]::WriteAllText((Join-Path $sums "$sid.txt"), "Strip feedback: the first summary`n")

Invoke-Test {
  $w = Open-TestWindow -Program (New-ClaudeSessionProgram $sid) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.initial_cols = 110
config.initial_rows = 20
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.lbterm_motion = { enabled = 'on', slowdown = $Slowdown }
-- Step 4 saves this file: reloading on, which New-TestConfig turns off. Its folder is its own
config.automatically_reload_config = true
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $pane 'Strip feedback test' | Out-Null

  # 1. The first summary
  $first = Shows { param($s) $s.strip.summary -match 'the first summary' -and $s.strip.title -eq 'Strip feedback test' } 'summary' 'the first summary'

  # 2. A new summary dissolves in, and the row brightens once: frames until the strip has the new
  # words and nothing moves on it any more
  [IO.File]::WriteAllText((Join-Path $sums "$sid.txt"), "Strip feedback: a second summary, longer than the first one`n")
  $frames = Get-Frames $w ([int](600 * $Slowdown)) -TailMs 150 -Until { param($s) $s.strip.summary -match 'a second summary' -and -not $s.animating }
  $second = Shows { param($s) $s.strip.summary -match 'a second summary' } 'summary-new' 'the new summary'
  $whole = Distance $first $second
  if ($whole -eq 0) { Add-Failure 'the strip never showed the new summary' }
  $between = @($frames | Where-Object { (Distance $first $_.Shot) -gt 0.02 * $whole -and (Distance $second $_.Shot) -gt 0.02 * $whole })
  "swap: $($between.Count) of $($frames.Count) frames neither the old strip nor the new"
  if ($between.Count -lt 2) { Add-Failure "the summary swapped at once: $($between.Count) frames between the old words and the new" }
  $settledLight = Light $second
  $peak = ($frames | ForEach-Object { Light $_.Shot } | Measure-Object -Maximum).Maximum
  "brightened to $('{0:N2}' -f $peak) over the strip, settled at $('{0:N2}' -f $settledLight)"
  if ($peak -le $settledLight + 0.3) { Add-Failure 'the new summary never brightened its row' }
  if ($between.Count) { Save-Shot $between[[int]($between.Count / 2)].Shot 'swap' }
  Remove-Frames $frames

  # 3. Zoomed: a badge
  $split = [int](Invoke-Cli split-pane --pane-id $pane --bottom -Program (New-PwshProgram 'Start-Sleep 600'))
  $beforeZoom = Shows { param($s) @($s.panes).Count -eq 2 } 'split' 'the split'
  # The window paints a zoom from the cli by itself (perf\cli-repaint.ps1): a state wait, which
  # until 2026-10-22 waited for a frame that never came without a capture to paint it
  Invoke-Cli zoom-pane --pane-id $split --zoom | Out-Null
  $zoomed = Shows { param($s) $s.strip.badge -match 'Zoomed' } 'zoomed' 'the zoomed tab''s badge'
  $d = Distance $beforeZoom $zoomed
  "zoomed: '$((Get-StateWindow $T.LastState -Hwnd ([long]$w.Handle)).strip.badge)', the strip changed by $('{0:N2}' -f $d)"
  if ($d -eq 0) { Add-Failure 'zooming a pane put nothing on the strip' }
  Invoke-Cli zoom-pane --pane-id $split --unzoom | Out-Null
  $unzoomed = Shows { param($s) -not $s.strip.badge -and @($s.panes).Count -eq 2 } 'unzoomed' 'the badge gone'
  if ((Distance $beforeZoom $unzoomed) -gt 0) { Add-Failure 'the badge stayed after unzooming (unzoomed.png)' }

  # 4. The config saved: a notice in the caption's place. The test config runs Lachlan's
  # lbterm.lua, whose files another session may save meanwhile: a notice from that goes first
  $quiet = Shows { param($s) -not $s.strip.notice -and $s.settled } 'before-save' 'no notice on the strip'
  Add-Content -Path $w.Config -Value "-- saved again $(Get-Date -Format o)"
  $n = Wait-Change { param($s) $s.strip.notice -match 'reload' } -What 'the reload''s notice' -Animating
  $reloaded = Get-Shot $w -Name 'reloaded'
  $d = Distance $quiet $reloaded
  "reloaded: '$($n.strip.notice)', the strip changed by $('{0:N2}' -f $d)"
  if ($d -eq 0) { Add-Failure 'saving the config put no notice on the strip' }
  $gone = Shows { param($s) -not $s.strip.notice -and $s.settled } 'notice-gone' 'the notice to go'
  if ((Distance $quiet $gone) -gt 0) { Add-Failure 'the notice stayed past its time (notice-gone.png)' }
} -Passed 'strip feedback: a new summary dissolves in and brightens once, a zoomed tab wears a badge, a saved config says so'
