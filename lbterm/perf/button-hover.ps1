#Requires -Version 7
# Variant: -Off
# Covers: wezterm-gui/src/termwindow/render/hover_fade.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/restart.rs, lbterm-extras/src/hover.rs, lbterm-extras/src/restart.rs, lbterm-motion/src/**
<#
The strip's hover fade (`lbterm_motion.button_hover`, render/hover_fade.rs) end to end, in a test
window of its own class with the compositor on, run from a copy of this tree's build whose
version.txt names another build, so the restart pill shows too:

1. At rest (rest.png). The window buttons and the pill are found in the strip as clusters of
   bright pixels: from the right, close, maximize, minimize, the pill.
2. A right press on close (posted, never real input; a right press on a window button does
   nothing else, and unlike a posted move it doesn't make the window track the pointer
   leaving, which would end the hover at once). Frames are captured as fast as PrintWindow
   allows while close's look under the pointer fades in.
3. A right press on maximize: close fades out while maximize fades in.
4. The same on the pill, then a posted WM_MOUSELEAVE: everything fades out, and the strip ends
   exactly as it was at rest.

Each fade's progress is how far its button's pixels are from rest, as a share of the way to the
look it settles on. It must only rise (or fall), never pass its end by more than capture noise,
and settle within the token's time. Run at -Slowdown 5 (600 ms fades) to see them in the
captures; hover-<step>.png tiles the strip's right end over time. -Off checks motion off: the
look is there in the first frame. Where the buttons and the pill are, and when the window is at
rest, come from the state file (lbterm/STATE.md).

  pwsh -File lbterm\perf\button-hover.ps1 [-Slowdown 5] [-Off]
#>
param([double]$Slowdown = 5, [switch]$Off, [string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
$T = Start-Test ('button-hover' + $(if ($Off) { '-off' } else { '' })) -Bin $Bin
$bin = Join-Path $T.Out 'bin'
$cfg = Join-Path $T.Out 'config\hover.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

$curveMs = 120 * $Slowdown

# A fade's progress per frame: the region's distance from rest over its distance at the end
function Get-Progress($frames, $rest, [System.Drawing.Rectangle]$r, [switch]$Out) {
  $d = @($frames | ForEach-Object { [LbtermE2E.Px]::RgbDistance($rest, $_.Shot, $r) })
  # Fading in, the end is the last frame; fading out, the start is the first
  $full = if ($Out) { $d[0] } else { $d[-1] }
  if ($full -le 0) { return $null }
  , @($d | ForEach-Object { $_ / $full })
}

# Whether a fade went one way, didn't pass its end, and was there by the token's time (and a frame)
function Test-Fade([string]$what, $frames, $progress, [switch]$Out) {
  if (-not $progress) { Add-Failure "$($what): nothing changed"; return }
  $p = @($progress)
  $ms = @($frames | ForEach-Object Ms)
  $trace = Format-Trace $p
  "$($what): $trace"
  if ($Off) {
    if ($p[0] -lt 0.95) { Add-Failure "$($what) with motion off isn't there at once: $trace" }
    return
  }
  for ($i = 1; $i -lt $p.Count; $i++) {
    $back = if ($Out) { $p[$i] - $p[$i - 1] } else { $p[$i - 1] - $p[$i] }
    if ($back -gt 0.08) { Add-Failure "$($what) turned back at $($ms[$i]) ms: $trace" ; break }
  }
  if (-not $Out -and ($p | Measure-Object -Maximum).Maximum -gt 1.08) { Add-Failure "$($what) passed its end: $trace" }
  $settled = for ($i = 0; $i -lt $p.Count; $i++) { if ((-not $Out -and $p[$i] -ge 0.97) -or ($Out -and $p[$i] -le 0.03)) { $ms[$i]; break } }
  if ($null -eq $settled) { Add-Failure "$($what) never settled: $trace" }
  elseif ($settled -gt $curveMs + 150) { Add-Failure "$($what) took $settled ms, past the $curveMs ms token" }
  else { "  settled by $settled ms (token $curveMs ms)" }
  if ($p.Count -lt 4) { Add-Failure "$($what): only $($p.Count) frames captured, too few to see a fade" }
}

# The build, with a version.txt naming another build so the pill shows
Copy-Build -From $T.Bin -To $bin | Out-Null
Set-BuildVersion $bin '20990101-000000-00000000'

$enabled = if ($Off) { 'off' } else { 'on' }
[IO.File]::WriteAllText($cfg, @"
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  lbterm_motion = { enabled = '$enabled', slowdown = $Slowdown },
  initial_cols = 100, initial_rows = 24,
  window_close_confirmation = 'NeverPrompt',
  window_decorations = 'INTEGRATED_BUTTONS|RESIZE',
  cursor_blink_rate = 0,
  default_cursor_style = 'SteadyBlock',
  lbterm_crash_restore = false,
  lbterm_tab_panel = { enabled = true },
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
}
"@)

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Exe (Join-Path $bin 'lbterm-gui.exe') -SameProcess
  # At rest: the pill showing, the shell's prompt drawn (two shots a second apart match), settled
  $e = Wait-Change { param($s) $s.strip.restart -and $s.settled } -What 'the strip at rest, the pill showing'
  $rest = Wait-Still $w -IntervalMs 1000 -NoThrow -Name 'rest'

  # The buttons and the pill, where the file says
  $item = { param($name) $it = @($e.strip.items | Where-Object name -eq $name)[0]; if (-not $it) { throw "no $name on the strip: $(@($e.strip.items | ForEach-Object name) -join ', ')" }; ConvertTo-Rect $it.rect }
  $rClose = & $item 'close'; $rMax = & $item 'maximize'; $rPill = & $item 'restart'
  $area = New-Rect ($rPill.X - 6) 0 ($rest.Width - $rPill.X + 6) ($rClose.Height + 2)
  "close $rClose, maximize $rMax, pill $rPill"
  $at = { param($r) @([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) }
  $press = { param($r) $x, $y = & $at $r; Send-Click $w $x $y -Right }

  $record = 2 * $curveMs + 300
  # 2. Close fades in
  & $press $rClose
  $frames = Get-Frames $w $record
  Save-Sheet $frames $area 'hover-close'
  Test-Fade 'close in' $frames (Get-Progress $frames $rest $rClose)

  # 3. Close out as maximize comes in
  & $press $rMax
  $frames2 = Get-Frames $w $record
  Save-Sheet $frames2 $area 'hover-across'
  # With motion off a fade out is over before the first frame: the end state below covers it
  if (-not $Off) { Test-Fade 'close out' $frames2 (Get-Progress $frames2 $rest $rClose -Out) -Out }
  Test-Fade 'maximize in' $frames2 (Get-Progress $frames2 $rest $rMax)

  # 4. The pill, then the pointer leaving
  & $press $rPill
  $frames3 = Get-Frames $w $record
  Save-Sheet $frames3 $area 'hover-pill'
  Test-Fade 'pill in' $frames3 (Get-Progress $frames3 $rest $rPill)
  Send-Leave $w
  $frames4 = Get-Frames $w $record
  Save-Sheet $frames4 $area 'hover-leave'
  if (-not $Off) { Test-Fade 'pill out' $frames4 (Get-Progress $frames4 $rest $rPill -Out) -Out }
  foreach ($f in @($frames, $frames2, $frames3, $frames4)) { Remove-Frames $f }

  # 5. Interrupted: close, maximize a quarter of the way into close's fade, close again a
  # quarter into its fade out. Close must turn round where it is, not start again from nothing.
  if (-not $Off) {
    # A quarter of the way into each fade, by the clock: one name (strip.hover) covers every
    # button's fade, and a snapshot's progress is only as new as its frame
    $quarter = [int]($curveMs / 4)
    & $press $rClose
    $a = Get-Frames $w $quarter
    & $press $rMax
    $b = Get-Frames $w $quarter
    & $press $rClose
    $c = Get-Frames $w $record
    $all = @($a) + @($b) + @($c)
    Save-Sheet $all $area 'hover-interrupted'
    $full = [LbtermE2E.Px]::RgbDistance($rest, $c[-1].Shot, $rClose)
    $p = @($all | ForEach-Object { [LbtermE2E.Px]::RgbDistance($rest, $_.Shot, $rClose) / $full })
    "close interrupted: $(Format-Trace $p)"
    $turn = $a.Count + $b.Count
    $before = $p[$turn - 1]; $after = $p[$turn]
    if ($before -lt 0.05) { Add-Failure "close had faded right out before it was turned round ($('{0:N2}' -f $before)): nothing was interrupted" }
    if ($after -lt $before - 0.15) { Add-Failure "close dropped from $('{0:N2}' -f $before) to $('{0:N2}' -f $after) when turned round: it started again" }
    if ($p[-1] -lt 0.97) { Add-Failure "close never got back to full after the turn ($('{0:N2}' -f $p[-1]))" }
    Remove-Frames $all
    Send-Leave $w
    Remove-Frames (Get-Frames $w $record)
  }

  $end = Get-Shot $w
  $strip = New-Rect 0 0 $rest.Width $area.Height
  $d = [LbtermE2E.Px]::RgbDistance($rest, $end, $strip)
  if ($d -ne 0) {
    Add-Failure "the strip doesn't end as it was at rest (distance $('{0:N3}' -f $d))"
    Save-Shot $end 'end'
  }
} -Passed "button hover: every fade went one way and settled within $curveMs ms; the strip ended as it was"
