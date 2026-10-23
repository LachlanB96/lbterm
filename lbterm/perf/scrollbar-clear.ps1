#Requires -Version 7
# Covers: wezterm-gui/src/scrollbar.rs, lbterm-extras/src/scrollbar.rs, wezterm-gui/src/termwindow/resize.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/compositor.rs
<#
The scrollbar never covers text: a test window with Lachlan's config (centred content, 6pt
padding, the scrollbar on; New-TestConfig) fills every column of a pane with solid red blocks,
and a capture (PrintWindow, which sees DirectComposition content) must show the whole of every
row, as wide as the terminal says it is, ending two pixels (at 96 DPI) left of the scrollbar's
thumb. Before 2026-10-20
centring forgot the room the scrollbar keeps inside the card, pushed the text right by half of
it, and the thumb hid the last 4 px of every row. Fails with exit 1; the capture is
target\perf\scrollbar-clear\scrollbar-clear.png either way.

  pwsh -File lbterm\perf\scrollbar-clear.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'scrollbar-clear' -Bin $Bin

Invoke-Test {
  $w = Open-TestWindow -Set 'enable_scroll_bar=true' -Program (New-PwshProgram)
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  # Every column of a dozen rows in solid red, then nothing more
  $script = @'
$e = [char]27
$w = [Console]::WindowWidth
[Console]::Write("$e[2J$e[H")
for ($i = 0; $i -lt 12; $i++) { [Console]::Write("$e[38;2;255;0;0m" + ([string][char]0x2588 * $w) + "$e[0m") }
Start-Sleep 60
'@
  $pane = New-Tab -From $first -Program (New-PwshProgram $script)
  Invoke-Cli activate-pane --pane-id $pane | Out-Null
  # The red rows written: the pane's cursor below the twelfth, and the window settled
  Wait-Change { param($s) $p = @($s.panes | Where-Object id -eq $pane)[0]; $s.active_pane -eq $pane -and $p.cursor.y -ge 11 } -What 'the red rows' -Animating | Out-Null
  $size = (Get-Panes | Where-Object pane_id -eq $pane).size

  $shot = Get-Shot $w -Name 'scrollbar-clear'
  $isRed = { param($c) $c.R -gt 150 -and $c.G -lt 80 -and $c.B -lt 80 }
  # A row through the middle of the red: the first one down the middle column that's red, then
  # half a cell further
  $mid = [int]($shot.Width / 2)
  $y = 0
  while ($y -lt $shot.Height -and -not (& $isRed $shot.GetPixel($mid, $y))) { $y++ }
  if ($y -ge $shot.Height) { throw "no red rows in the capture" }
  $y += [int]($size.pixel_height / $size.rows / 2)
  $left = 0
  while ($left -lt $shot.Width -and -not (& $isRed $shot.GetPixel($left, $y))) { $left++ }
  $right = $shot.Width - 1
  while ($right -gt $left -and -not (& $isRed $shot.GetPixel($right, $y))) { $right-- }
  $shown = $right - $left + 1
  # The thumb: the first thing past the text that isn't the pane's background, which shows
  # under the red rows
  $cellH = [int]($size.pixel_height / $size.rows)
  $bg = $shot.GetPixel($mid, [Math]::Min($shot.Height - 1, $y + 14 * $cellH))
  $thumb = $right + 1
  while ($thumb -lt $shot.Width) {
    $c = $shot.GetPixel($thumb, $y)
    if ([Math]::Abs($c.R - $bg.R) + [Math]::Abs($c.G - $bg.G) + [Math]::Abs($c.B - $bg.B) -gt 30) { break }
    $thumb++
  }
  $width = $shot.Width
  $shot.Dispose()
  $gap = $thumb - $right - 1
  $wantGap = [Math]::Max(1, [Math]::Round(2 * $w.Scale))

  "$($size.cols) columns, $($size.pixel_width) px of text: shown x $left..$right ($shown px), thumb from x $thumb ($gap px clear)"
  if ($shown -ne $size.pixel_width) { Add-Failure "$($size.pixel_width - $shown) px of the text don't show" }
  if ($thumb -ge $width) { Add-Failure "no scrollbar right of the text" }
  elseif ($gap -lt $wantGap) { Add-Failure "the thumb is $gap px from the text, not at least $wantGap" }
} -Passed 'scrollbar-clear: every column shows whole, clear of the thumb'
