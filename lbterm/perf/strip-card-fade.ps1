#Requires -Version 7
# Variant: -Off
# Covers: wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm-extras/src/strip.rs, lbterm-extras/src/summaries.rs, lbterm-motion/src/**
<#
The strip's detail card fading (render/strip.rs, compositor.rs `prepare_strip_card`) end to end,
in a test window of its own class with the compositor on and lbterm_summaries pointed at a folder
of its own, <out>\summaries, whose one session the window's pane names:

1. Open and closed, settled (open.png, closed.png): where the card is, from the two.
2. expanded.flag appears (Terminal's Alt+S): the card fades in on `pane_enter`. Frames are
   captured as fast as PrintWindow allows; the card's progress is how far its pixels are from
   closed, as a share of the way to open. It must only rise, not pass its end, and settle.
3. The flag goes: it fades out on `pane_exit`, and the window ends exactly as it was closed.
4. Interrupted: the flag appears and goes again a third of the way into the fade. The card must
   turn round where it is, never drop to nothing or jump to full.

Run at -Slowdown 5 (an 800 ms fade in, 600 ms out) to see them in the captures; fade-<step>.png
tiles the card's top over time. -Off checks motion off: the card is there, or gone, between one
frame and the next. The file watcher takes a moment to see the flag, so the first frames of each
step can still be at rest. When the card has opened or closed and stopped moving comes from the
state file (lbterm/STATE.md), which also ends each recording.

  pwsh -File lbterm\perf\strip-card-fade.ps1 [-Slowdown 5] [-Off]
#>
param([double]$Slowdown = 5, [switch]$Off, [string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('strip-card-fade' + $(if ($Off) { '-off' } else { '' })) -Bin $Bin
$sums = Join-Path $T.Out 'summaries'
$cfg = Join-Path $T.Out 'config\fade.lua'
New-Item -ItemType Directory -Force $sums, (Split-Path $cfg) | Out-Null

$inMs = 160 * $Slowdown
$outMs = 120 * $Slowdown
# How long the folder's watcher can take to see the flag, and a frame
$react = 250

function Trace($p) { Format-Trace $p }

# A fade's progress went one way, didn't pass its end, and settled by its token's time after it
# started (the frames before it starts are the watcher's delay)
function Test-Fade([string]$what, $frames, $p, [int]$tokenMs, [switch]$Out) {
  $ms = @($frames | ForEach-Object Ms)
  "$($what): $(Trace $p)"
  $start = for ($i = 0; $i -lt $p.Count; $i++) { if (($Out -and $p[$i] -lt 0.97) -or (-not $Out -and $p[$i] -gt 0.03)) { $i; break } }
  if ($null -eq $start) { Add-Failure "$($what): nothing changed"; return }
  if ($Off) {
    $between = @($p | Where-Object { $_ -gt 0.05 -and $_ -lt 0.95 })
    if ($between.Count) { Add-Failure "$($what) with motion off passed through $($between.Count) frames on the way: $(Trace $p)" }
    return
  }
  for ($i = $start + 1; $i -lt $p.Count; $i++) {
    $back = if ($Out) { $p[$i] - $p[$i - 1] } else { $p[$i - 1] - $p[$i] }
    if ($back -gt 0.08) { Add-Failure "$($what) turned back at $($ms[$i]) ms: $(Trace $p)"; break }
  }
  if (-not $Out -and ($p | Measure-Object -Maximum).Maximum -gt 1.08) { Add-Failure "$($what) passed its end: $(Trace $p)" }
  $settled = for ($i = $start; $i -lt $p.Count; $i++) { if ((-not $Out -and $p[$i] -ge 0.97) -or ($Out -and $p[$i] -le 0.03)) { $ms[$i] - $ms[[math]::Max($start - 1, 0)]; break } }
  if ($null -eq $settled) { Add-Failure "$($what) never settled: $(Trace $p)" }
  elseif ($settled -gt $tokenMs + 200) { Add-Failure "$($what) took $settled ms, past its $tokenMs ms token" }
  else { "  settled $settled ms after it started (token $tokenMs ms)" }
  if ($p.Count - $start -lt 4) { Add-Failure "$($what): only $($p.Count - $start) frames of it captured, too few to see a fade" }
}

$sid = '5f0c2d7e-1b3a-4e6f-9a8b-7c6d5e4f3a21'
[IO.File]::WriteAllText((Join-Path $sums "$sid.txt"), "Card fade: in on pane_enter, out on pane_exit`n")
[IO.File]::WriteAllText((Join-Path $sums "$sid.detail.txt"), @"
Goal: the strip's detail card fades in and out in DWM.
Finding: a closing card is drawn as it was until it has faded.
Next: try it under the pointer.
"@)
$flag = Join-Path $sums 'expanded.flag'

$enabled = if ($Off) { 'off' } else { 'on' }
$prog = (New-ClaudeSessionProgram $sid | ForEach-Object { "'$_'" }) -join ', '
[IO.File]::WriteAllText($cfg, @"
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  lbterm_motion = { enabled = '$enabled', slowdown = $Slowdown },
  initial_cols = 100, initial_rows = 20,
  window_close_confirmation = 'NeverPrompt',
  window_decorations = 'INTEGRATED_BUTTONS|RESIZE',
  cursor_blink_rate = 0,
  default_cursor_style = 'SteadyBlock',
  lbterm_crash_restore = false,
  lbterm_tab_panel = { enabled = true },
  lbterm_summaries = '$(ConvertTo-LuaPath $sums)',
  default_prog = { $prog },
  default_cwd = 'C:/Windows',
}
"@)

# The card open or closed as $open says, settled, then the picture of it
function Card($w, [bool]$open, [string]$name) {
  Wait-Change { param($s) ($s.strip.card -eq 'summary') -eq $open } -What $(if ($open) { 'the card open' } else { 'the card closed' }) -Animating | Out-Null
  Get-Shot $w -Name $name
}

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg

  # 1. Where the card is
  $closed = Card $w $false $null
  [IO.File]::WriteAllText($flag, '')
  $open = Card $w $true 'open'
  Remove-Item $flag
  $closed.Dispose(); $closed = Card $w $false 'closed'
  $d = Get-DiffBox $closed $open
  if (-not $d) { throw "expanded.flag opened no card (open.png, closed.png)" }
  # The card itself, below the strip (where the summary brightens too)
  $top = [math]::Max($d[1], 40)
  $card = New-Rect $d[0] $top ($d[2] - $d[0] + 1) ($d[3] - $top + 1)
  $area = New-Rect $d[0] $top ([math]::Min($card.Width, 360)) ([math]::Min($card.Height, 50))
  "card $card"
  $full = [LbtermE2E.Px]::RgbDistance($closed, $open, $card)
  $progress = { param($frames) , @($frames | ForEach-Object { [LbtermE2E.Px]::RgbDistance($closed, $_.Shot, $card) / $full }) }

  # 2. In
  [IO.File]::WriteAllText($flag, '')
  $in = Get-Frames $w ($react + 2 * $inMs + 300) -TailMs 300 -Until { param($s) $s.strip.card -eq 'summary' -and -not $s.animating }
  Save-Sheet $in $area 'fade-in' -Zoom 2
  Test-Fade 'card in' $in (& $progress $in) $inMs

  # 3. Out
  Remove-Item $flag
  $outFrames = Get-Frames $w ($react + 2 * $outMs + 300) -TailMs 300 -Until { param($s) -not $s.strip.card -and -not $s.animating }
  Save-Sheet $outFrames $area 'fade-out' -Zoom 2
  Test-Fade 'card out' $outFrames (& $progress $outFrames) $outMs -Out
  $end = Card $w $false $null
  $left = Get-DiffBox $closed $end
  if ($left) { Add-Failure "the window doesn't end as it was closed: $(Format-Box $left)"; Save-Shot $end 'end' }

  # 4. Interrupted a third of the way in: it turns round where it is
  if (-not $Off) {
    [IO.File]::WriteAllText($flag, '')
    # A third of the way in, by the clock: the fade runs in DWM, which writes no snapshot while it
    # runs, so the file can't say how far it has got
    $a = Get-Frames $w ($react + [int]($inMs / 3))
    Remove-Item $flag
    $b = Get-Frames $w ($react + 2 * $outMs + 300) -TailMs 300 -Until { param($s) -not $s.strip.card -and -not $s.animating }
    $all = @($a) + @($b)
    Save-Sheet $all $area 'fade-interrupted' -Zoom 2
    $p = & $progress $all
    "card interrupted: $(Trace $p)"
    $peak = ($p | Measure-Object -Maximum).Maximum
    if ($peak -lt 0.05) { Add-Failure "the card never started to open before it was turned round: $(Trace $p)" }
    if ($peak -gt 0.97) { Add-Failure "the card got all the way open before it was turned round: $(Trace $p)" }
    for ($i = 1; $i -lt $p.Count; $i++) {
      if ([math]::Abs($p[$i] - $p[$i - 1]) -gt 0.3) { Add-Failure "the card jumped from $('{0:N2}' -f $p[$i - 1]) to $('{0:N2}' -f $p[$i]) at $($all[$i].Ms) ms: it started again"; break }
    }
    if ($p[-1] -gt 0.03) { Add-Failure "the card never finished closing after the turn: $(Trace $p)" }
  }
} -Passed $(if ($Off) { 'card fade, motion off: the card came and went between one frame and the next' } else { 'card fade: in and out went one way and settled, an interrupted fade turned round where it was' })
