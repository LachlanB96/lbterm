#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/sides.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, wezterm-gui/src/termwindow/render/objectives.rs, wezterm-gui/src/termwindow/render/projects.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/state.rs, lbterm-motion/src/plan.rs, lbterm-motion/src/flight.rs, lbterm-motion/src/spans.rs, lbterm-extras/src/state.rs
<#
The strips down the window's right (the scroll strip and the objectives strip: lbterm/SCROLL-STRIP.md,
lbterm/OBJECTIVES.md) fading into the next tab's with its panes, end to end, in a test window that
looks like Lachlan's LBTerm (New-TestConfig) with the tab panel, both strips and lbterm_summaries
pointed at a scratch folder, and motion at -Slowdown. Three tabs, each a Claude session with an
objectives file (done, waiting, open: a green dot, an orange ring, a grey ring on the objectives
strip). Tab one prints 300 lines with a shell prompt every 15 (OSC 133), so its scroll strip has a
coral bar at each and a short thumb at the bottom; tab two prints nothing, so its thumb is the whole
track; tab three prints 60 lines with a prompt every 20. Each tab paints a block of its own, as in
tab-switch.ps1: one's on the left, two's on the right, three's across the middle, dimmer. F9 and F8
are the next and the previous tab, posted as keys.

1. One to two: the state file's first frame on tab two has `sides.dissolve` in flight with
   `pane.dissolve`, and `fading_sides` 1. On the scroll strip, tab one's share of each frame on the
   way is read where one is brighter (its bars, which two's thumb covers) and where two is (its
   thumb, between one's bars): the same share on both, a true cross-fade, and the panes' share,
   in step (strips.png, the middle frame). It follows tab_switch (fitted, printed). The objectives
   strip, the same in both tabs, shows no change in any frame. Settled, nothing fades
   (`fading_sides` 0) and the strips are tab two's.
2. Back mid-fade: one to two, and back to one before the fade is a quarter through: the strips'
   share of tab one never jumps up past where it had got to, and keeps with the panes'.
3. Rapid: F9 five times 40 ms apart, round all three: it ends settled on tab three, nothing left
   fading, the strips as tab three shows them.
3b. Round to the oldest: from tab one, the mouse's forward button three times 40 ms apart, as fast
   as a hand clicks, round to one while two still fades under it, so one comes back from on top
   of the stack of tabs in flight and two's strips are played again under what's left
   (lbterm_motion::flight): each tab's share of the scroll strip keeps its panes' to 0.12 (both
   read by least squares of the three tabs' settled shots, `[LbtermE2E.Px]::Mix`), two's never
   rises again after its peak, and one's never jumps at the third press. Since feat/dcomp-recorder
   those three are compositor/imp/tests/flight.rs's, from what DWM was handed; this step runs the
   presses, judges they went round with nothing left fading, and keeps its frames.
4. Close: back on tab one, settled, then its pane killed: the rows below move up, and the old marks
   fade out where they were, over the new ones, in step with the closed tab's panes; tab three's
   grey ring, which had the third row, shows the panes' share there on the way (close.png).
5. Motion off, in a second window with the same tabs: a switch is there in the next frame, with no
   frames between on the strips, no `sides.dissolve`, and the strips land on exactly what the first
   window's settled on.

-Sheets also keeps each step's frames, in target\motion\side-strips-<step> (switch, back, rapid,
close, off), and has lbterm\motion\sheet.py make each a contact sheet of the strips and the right of
the pane beside them at twice their size (contact.png; read its middle frames at full size, as the
lbterm-motion skill says, frame<n>.png), and for the switch and the close how much of the first
frame shows on the scroll strip's track (the close: the third row's mark) over time, fitted to
tab_switch (fade.png).

-Measure instead switches between tabs one and two -Switches times, 250 ms apart, each waited on
until it has settled, and prints how long each took from the key to the state file's frame on the
new tab (median and worst), the GUI's CPU time a switch over what it costs at rest for as long
(measured first), the frames each painted, and the GUI's dedicated GPU memory at rest and while a
tab left fades. -Slow adds what held the GUI thread over 1 ms, by site (LBTERM_SLOW_EVERY,
lbterm_extras::slow). It checks nothing, so it runs against an older build too (-Bin): run it
interleaved with one, before and after. A locked workstation stops DWM, so check and measure
unlocked.

Captures go to target\perf\side-strips. Fails with exit 1.

  pwsh -File lbterm\perf\side-strips.ps1 [-Slowdown 5] [-Sheets]
  pwsh -File lbterm\perf\side-strips.ps1 -Measure [-Switches 40] [-Slow] [-Bin <build>]
  pwsh -File lbterm\perf\side-strips.ps1 -Log lbterm_gui::termwindow::render::sides=debug,lbterm_gui::termwindow::compositor=debug
#>
param([double]$Slowdown = 5, [switch]$Measure, [int]$Switches = 40, [switch]$Slow, [switch]$Sheets, [string]$Bin, [string]$Log)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'side-strips' -Bin $Bin
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $summaries, $home2 | Out-Null

$sessions = [ordered]@{
  one = @{ sid = 'a1a1a1a1-1111-4111-8111-111111111111'; status = 'done' }
  two = @{ sid = 'b2b2b2b2-2222-4222-8222-222222222222'; status = 'waiting' }
  three = @{ sid = 'c3c3c3c3-3333-4333-8333-333333333333'; status = 'open' }
}
foreach ($name in $sessions.Keys) {
  $s = $sessions[$name]
  [IO.File]::WriteAllText((Join-Path $summaries "$($s.sid).label.txt"), $name)
  $o = [ordered]@{ id = 1; text = "Tab $name's ask"; from = 'first'; ref = $null; at = 1; status = $s.status; evidence = ''; statusAt = 1 }
  $json = [ordered]@{ sessionId = $s.sid; updatedAt = 1; current = 1; objectives = @($o) } | ConvertTo-Json -Depth 5 -Compress
  [IO.File]::WriteAllText((Join-Path $summaries "$($s.sid).objectives.json"), $json)
}

# Each tab's program: its session named as claudehook.exe names it, its lines (a shell prompt every
# `every`, so the scroll strip marks it), the screen cleared, its block, the cursor hidden
function Stand-In([string]$name, [int]$lines, [int]$every, [int]$from, [int]$to, [int]$grey) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sessions[$name].sid))
  $script = @"
`$e = [char]27; `$bel = [char]7
[Console]::Write("`$e]1337;SetUserVar=claude_session=$b64`$bel")
`$sb = [Text.StringBuilder]::new()
for (`$i = 1; `$i -le $lines; `$i++) {
  if ($every -gt 0 -and `$i % $every -eq 1) { [void]`$sb.Append("`$e]133;A`$e\PS C:\demo> `$e]133;B`$e\Get-Thing `$i`r`n`$e]133;C`$e\") }
  else { [void]`$sb.Append("  output line `$i`r`n") }
}
[Console]::Write(`$sb.ToString())
`$fill = "`$e[48;2;$grey;$grey;${grey}m" + (' ' * ($to - $from)) + "`$e[0m"
`$out = "`$e[?25l`$e[2J"
for (`$r = 2; `$r -le 21; `$r++) { `$out += "`$e[`$r;$($from + 1)H" + `$fill }
[Console]::Write(`$out)
Start-Sleep -Seconds 86400
"@
  New-PwshProgram $script
}

$lua = @"
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true }
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.ActivateTabRelative(1) })
table.insert(config.keys, { key = 'F8', mods = 'NONE', action = wezterm.action.ActivateTabRelative(-1) })
"@

# A window with the three tabs, on tab one, each tab's marks and strip settled
function Open-Tabs([string]$motion) {
  # -Slow: every stretch of the GUI thread over 1 ms in the log (lbterm_extras::slow), by site
  $slowEnv = if ($Slow) { @{ LBTERM_SLOW_EVERY = '1' } } else { @{} }
  $slowLua = if ($Slow) { "`nconfig.lbterm_slow_ms = 1`n" } else { '' }
  $w = Open-TestWindow -UserProfile $home2 -Log $Log -WorkingDirectory $T.Repo -Environment $slowEnv `
    -Set "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'", 'lbterm_objectives={}', 'lbterm_scroll_strip={}' `
    -Program (Stand-In one 300 15 0 40 220) -Lua ($lua + $slowLua + "`nconfig.lbterm_motion = { enabled = '$motion', slowdown = $Slowdown }`n")
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail -and $s.objectives -and $s.scroll_strip } -What 'the window, its rail and both strips' | Out-Null
  $panes = @{ one = (Get-Panes)[0].pane_id }
  $panes.two = New-Tab -From $panes.one -Program (Stand-In two 0 0 50 90 220)
  $panes.three = New-Tab -From $panes.one -Program (Stand-In three 60 20 20 70 120)
  foreach ($n in 'one', 'two', 'three') { Invoke-Cli set-tab-title --pane-id $panes[$n] $n | Out-Null }
  Invoke-Cli activate-tab --pane-id $panes.one --tab-index 0 | Out-Null
  $s = Wait-Change { param($s)
    $s.active_tab -eq 0 -and @($s.rail.rows).Count -eq 3 -and @($s.objectives.marks | Where-Object mark).Count -eq 3 -and
    $s.scroll_strip.mode -eq 'shell' -and @($s.scroll_strip.marks).Count -ge 10 } -What 'tab one, three marked rows, its prompts on the scroll strip' -Animating -TimeoutSec 30
  # Its program has drawn its block
  Start-Sleep -Milliseconds 300
  [pscustomobject]@{ Window = $w; Panes = $panes; State = $s }
}

# A switch: the key, then the state's first frame on tab `index` (how long that took), recording
# frames until it has settled there when -Frames
function Switch-Tab($w, [string]$key, [int]$index, [switch]$Frames, [scriptblock]$Each) {
  $before = Get-StateFrame $w
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Send-Key $w $key
  $seen = @{ First = $null; Ms = $null; All = [Collections.Generic.List[object]]::new() }
  $record = { param($s)
    $seen.All.Add($s)
    if (-not $seen.First -and $s.active_tab -eq $index) { $seen.First = $s; $seen.Ms = $clock.Elapsed.TotalMilliseconds }
    $s.active_tab -eq $index -and -not $s.animating }
  if ($Frames) {
    $shots = Get-Frames $w ([int](200 * $Slowdown + 1500)) -TailMs 120 -Each $Each -Until { param($s) & $record $s }
  } else {
    $shots = @()
    Wait-State { param($s) & $record $s } -AfterFrame $before -What "tab $index" | Out-Null
  }
  [pscustomobject]@{ Shots = $shots; First = $seen.First; Ms = $seen.Ms; States = @($seen.All) }
}

# The mean light over several rectangles, read from a copy of just the part of the shot they're in
# (each probe copies the whole shot's pixels)
function Get-Light($shot, $rects) {
  $rects = @($rects)
  if (-not $rects.Count) { return 0 }
  $box = $rects[0]
  foreach ($r in $rects) { $box = [System.Drawing.Rectangle]::Union($box, $r) }
  $box.Intersect([System.Drawing.Rectangle]::new(0, 0, $shot.Width, $shot.Height))
  if ($box.Width -le 0 -or $box.Height -le 0) { return 0 }
  $part = $shot.Clone($box, $shot.PixelFormat)
  try {
    $sum = 0.0; $n = 0
    foreach ($r in $rects) {
      $at = [System.Drawing.Rectangle]::new($r.X - $box.X, $r.Y - $box.Y, $r.Width, $r.Height)
      $sum += [LbtermE2E.Px]::Light($part, $at) * $r.Width * $r.Height; $n += $r.Width * $r.Height
    }
    if ($n) { $sum / $n } else { 0 }
  } finally { $part.Dispose() }
}

# -Sheets: a step's frames in target\motion\side-strips-<step>, and sheet.py's contact sheet of the
# strips and the right of the pane beside them, at twice their size; with `$Fade`, how much of the
# first frame shows there over time, fitted to tab_switch, else frames picked evenly
function Save-StepSheet($Shots, [string]$Step, $Fade) {
  if (-not $Sheets) { return }
  $dir = Join-Path $T.Repo "target\motion\side-strips-$Step"
  New-Item -ItemType Directory -Force $dir | Out-Null
  Get-ChildItem -LiteralPath $dir -File | Remove-Item
  Save-Frames @($Shots) -Dir $dir
  $s = $script:geometry
  $x = [int]$s.scroll_strip.strip.x - 100
  $crop = @($x, [int]$s.objectives.strip.y, ([int]($s.objectives.strip.x + $s.objectives.strip.w) - $x + 2), 260)
  $sheet = @((Join-Path $T.Repo 'lbterm\motion\sheet.py'), $dir, '--crop') + $crop + @('--zoom', 2, '--count', 18, '--cols', 9)
  $sheet += if ($Fade) { @('--fade', $Fade.X, $Fade.Y, $Fade.Width, $Fade.Height, '--fade-ms', (200 * $Slowdown)) } else { @('--tiles') }
  python @sheet | ForEach-Object { "sheet ${Step}: $_" }
}

# The curve tab_switch is: cubic-bezier(0.23, 1, 0.32, 1), y for x = t
$curve = { param($t)
  if ($t -le 0) { return 0.0 }; if ($t -ge 1) { return 1.0 }
  $a = 0.0; $b = 1.0
  for ($k = 0; $k -lt 40; $k++) { $u = ($a + $b) / 2; $x = 3 * (1 - $u) * (1 - $u) * $u * 0.23 + 3 * (1 - $u) * $u * $u * 0.32 + $u * $u * $u; if ($x -lt $t) { $a = $u } else { $b = $u } }
  $u = ($a + $b) / 2; 1 - [Math]::Pow(1 - $u, 3) }

Invoke-Test {
  $tabs = Open-Tabs 'on'
  $w = $tabs.Window

  if ($Measure) {
    $proc = Get-Process -Id $w.Pid
    $cpuNow = { $proc.Refresh(); $proc.TotalProcessorTime.TotalMilliseconds }
    $gpu = { try { (Get-Counter "\GPU Process Memory(pid_$($w.Pid)_*)\Dedicated Usage" -ErrorAction Stop).CounterSamples | Measure-Object CookedValue -Sum | ForEach-Object Sum } catch { $null } }
    $gpuAtRest = & $gpu
    $restMs = $Switches * 300
    $c0 = & $cpuNow; Start-Sleep -Milliseconds $restMs; $rest = (& $cpuNow) - $c0
    $times = [Collections.Generic.List[double]]::new()
    $f0 = Get-StateFrame $w
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $c0 = & $cpuNow
    for ($i = 0; $i -lt $Switches; $i++) {
      $to = if ($i % 2 -eq 0) { 1 } else { 0 }
      $key = if ($to -eq 1) { 'F9' } else { 'F8' }
      $r = Switch-Tab $w $key $to
      $times.Add($r.Ms)
      Start-Sleep -Milliseconds 250
    }
    Wait-Settled -Animating | Out-Null
    $busy = (& $cpuNow) - $c0
    $wall = $clock.Elapsed.TotalMilliseconds
    $frames = (Get-StateFrame $w) - $f0
    Switch-Tab $w F9 1 | Out-Null
    $gpuFading = & $gpu
    Wait-Settled -Animating | Out-Null
    $sorted = @($times | Sort-Object)
    $median = $sorted[[int]($sorted.Count / 2)]
    $net = $busy - $rest * $wall / $restMs
    "measure: $Switches switches at slowdown $Slowdown with both strips; key to the state file's frame on the new tab: median $('{0:N1}' -f $median) ms, worst $('{0:N1}' -f $sorted[-1]) ms"
    "measure: the GUI's CPU at rest $('{0:N0}' -f $rest) ms over $('{0:N1}' -f ($restMs / 1000)) s; switching $('{0:N0}' -f $busy) ms over $('{0:N1}' -f ($wall / 1000)) s: $('{0:N1}' -f ($net / $Switches)) ms a switch more than at rest, in $('{0:N1}' -f ($frames / $Switches)) frames a switch"
    if ($null -ne $gpuAtRest) { "measure: the GUI's dedicated GPU memory $('{0:N1}' -f ($gpuAtRest / 1MB)) MB at rest, $('{0:N1}' -f ($gpuFading / 1MB)) MB while a tab left fades" }
    else { 'measure: no GPU memory counter for the GUI' }
    if ($Slow) {
      # What held the GUI thread, by site, summed over the whole run (the setup's included)
      $lines = @(Get-Content -LiteralPath $w.Log | Where-Object { $_ -match 'lbterm::slow' -and $_ -match ': ([\d.]+) ms$' })
      $sites = @{}
      foreach ($l in $lines) {
        if ($l -match '(?:slow \w+ )(.+): ([\d.]+) ms$') { $sites[$Matches[1]] = [double]$sites[$Matches[1]] + [double]$Matches[2] }
      }
      "measure: the GUI thread over 1 ms, $($lines.Count) times, by site (ms summed):"
      $sites.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 12 | ForEach-Object { "  $('{0,8:N1}' -f $_.Value)  $($_.Key)" }
    }
    return
  }

  $one = Get-Shot $w -Name 'one'
  $s1 = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $card = @($s1.panes)[0].rect
  # Where tab one's block is (the left of the pane) and tab two's (the right)
  $left = New-Rect ([int]($card.x + 0.08 * $card.w)) ([int]($card.y + 0.15 * $card.h)) ([int](0.25 * $card.w)) ([int](0.3 * $card.h))
  $right = New-Rect ([int]($card.x + 0.6 * $card.w)) ([int]($card.y + 0.15 * $card.h)) ([int](0.25 * $card.w)) ([int](0.3 * $card.h))
  $lum = { param($shot, $r) [LbtermE2E.Px]::Light($shot, $r) }
  $objStrip = ConvertTo-Rect $s1.objectives.strip
  $script:geometry = $s1

  # 1. One to two
  $r = Switch-Tab $w F9 1 -Frames
  $two = Get-Shot $w -Name 'two'
  $f = $r.First
  $s2 = @($r.States)[-1]
  $names = @($f.animations | ForEach-Object name)
  "switch: the first frame on tab two came $('{0:N0}' -f $r.Ms) ms after the key; animating $($names -join ', '); fading sides $($f.fading_sides)"
  if (-not $f) { Add-Failure 'the state file never showed tab two' }
  else {
    if ($names -notcontains 'sides.dissolve') { Add-Failure "the strips didn't dissolve: the first frame on tab two animates $($names -join ', ')" }
    if ($names -notcontains 'pane.dissolve') { Add-Failure "tab one's panes didn't dissolve: the first frame on tab two animates $($names -join ', ')" }
    if ($f.fading_sides -ne 1) { Add-Failure "the first frame on tab two had $($f.fading_sides) pictures of the strips fading, not 1" }
  }
  if ($s2.fading_sides) { Add-Failure "pictures of the strips still fading once it settled: $($s2.fading_sides)" }
  if ($s2.scroll_strip.mode -ne 'shell' -or @($s2.scroll_strip.marks).Count) { Add-Failure "tab two's scroll strip isn't a bare shell's: $($s2.scroll_strip.mode), $(@($s2.scroll_strip.marks).Count) marks" }

  # The scroll strip: where tab one is brighter (its bars, under two's whole-track thumb) and where
  # two is (its thumb, between one's bars and above one's thumb), clear of the thumbs' round ends
  $track = $s1.scroll_strip.track
  $oneThumb = $s1.scroll_strip.thumb
  $twoThumb = $s2.scroll_strip.thumb
  if (-not $oneThumb -or -not $twoThumb) { throw "a scroll strip without a thumb: one's $oneThumb, two's $twoThumb" }
  $top = [int]$twoThumb.y + [int]$twoThumb.w; $bottom = [int][Math]::Min($oneThumb.y, $twoThumb.y + $twoThumb.h - $twoThumb.w)
  $bars = @($s1.scroll_strip.marks | Where-Object { $_.rect.y -ge $top -and $_.rect.y + $_.rect.h -le $bottom - 2 } | ForEach-Object { ConvertTo-Rect $_.rect })
  $gaps = [Collections.Generic.List[object]]::new()
  $run = $null
  for ($y = $top; $y -lt $bottom - 2; $y++) {
    $clear = -not ($bars | Where-Object { $y -ge $_.Y - 2 -and $y -lt $_.Bottom + 2 })
    if ($clear -and $null -eq $run) { $run = $y }
    if ((-not $clear -or $y -eq $bottom - 3) -and $null -ne $run) {
      $end = if ($clear) { $y + 1 } else { $y }
      if ($end -gt $run) { $gaps.Add((New-Rect ([int]$track.x + 1) $run ([int]$track.w - 2) ($end - $run))) }
      $run = $null
    }
  }
  $hiB = Get-Light $one $bars; $loB = Get-Light $two $bars
  $hiA = Get-Light $two $gaps; $loA = Get-Light $one $gaps
  "strips: tab one's bars $('{0:N0}' -f $hiB) against two's thumb $('{0:N0}' -f $loB) there ($($bars.Count) bars); two's thumb $('{0:N0}' -f $hiA) against one's track $('{0:N0}' -f $loA) ($($gaps.Count) rows)"
  if ($hiB - $loB -lt 30 -or $hiA - $loA -lt 30) { Add-Failure "the tabs' scroll strips aren't different enough to read a share from: bars $('{0:N0}' -f $loB) to $('{0:N0}' -f $hiB), thumb $('{0:N0}' -f $loA) to $('{0:N0}' -f $hiA)" }
  $paneLo = @{ L = & $lum $two $left; R = & $lum $one $right }
  $paneHi = @{ L = & $lum $one $left; R = & $lum $two $right }
  $share = { param($shot)
    $p = (((& $lum $shot $left) - $paneLo.L) / [Math]::Max(1, $paneHi.L - $paneLo.L) + ($paneHi.R - (& $lum $shot $right)) / [Math]::Max(1, $paneHi.R - $paneLo.R)) / 2
    $b = ((Get-Light $shot $bars) - $loB) / [Math]::Max(1, $hiB - $loB)
    $a = ($hiA - (Get-Light $shot $gaps)) / [Math]::Max(1, $hiA - $loA)
    # The objectives strip: how many of its pixels changed past what DWM's rounding of an opacity
    # over the same picture comes to (a level or two)
    [pscustomobject]@{ Panes = $p; Bars = $b; Thumb = $a; Objectives = [LbtermE2E.Px]::LumDistance($one, $shot, $objStrip); Changed = [LbtermE2E.Px]::Changed($one, $shot, $objStrip, 6) } }
  $trace = @($r.Shots | ForEach-Object { $x = & $share $_.Shot; $x | Add-Member Ms $_.Ms; $x | Add-Member Shot $_.Shot; $x })
  $mid = @($trace | Where-Object { $_.Panes -gt 0.05 -and $_.Panes -lt 0.95 })
  "switch: $($mid.Count) of $($trace.Count) frames on the way; tab one's share, panes: $(Format-Trace @($mid | ForEach-Object Panes))"
  "switch: the scroll strip's, at one's bars: $(Format-Trace @($mid | ForEach-Object Bars)); at two's thumb: $(Format-Trace @($mid | ForEach-Object Thumb))"
  if ($mid.Count -lt 3) { Add-Failure "the panes cut: $($mid.Count) frames between tab one and tab two" }
  $stripMid = @($trace | Where-Object { $_.Bars -gt 0.05 -and $_.Bars -lt 0.95 })
  if ($stripMid.Count -lt 3) { Add-Failure "the scroll strip cut: $($stripMid.Count) frames between tab one's and tab two's" }
  $apart = @($mid | Where-Object { [Math]::Abs($_.Bars - $_.Thumb) -gt 0.12 })
  if ($apart.Count -gt [Math]::Max(1, $mid.Count / 10)) { Add-Failure "not a cross-fade on the scroll strip: in $($apart.Count) of $($mid.Count) frames its two parts showed different shares of tab one (the old over a blank, or the new fading in on its own)" }
  $behind = @($mid | Where-Object { [Math]::Abs(($_.Bars + $_.Thumb) / 2 - $_.Panes) -gt 0.1 })
  if ($behind.Count -gt [Math]::Max(2, $mid.Count / 6)) { Add-Failure "the scroll strip and the panes weren't in step in $($behind.Count) of $($mid.Count) frames" }
  $odd = $mid | Sort-Object { -[Math]::Abs(($_.Bars + $_.Thumb) / 2 - $_.Panes) } | Select-Object -First 1
  if ($odd) { "switch: the strip furthest from the panes, $('{0:N2}' -f (($odd.Bars + $odd.Thumb) / 2)) against $('{0:N2}' -f $odd.Panes)" }
  $flicker = @($trace | Where-Object { $_.Changed -gt 0 })
  "switch: the objectives strip against how it was: at most $('{0:N2}' -f (($trace | Measure-Object Objectives -Maximum).Maximum)) levels on average, $(($trace | Measure-Object Changed -Maximum).Maximum) pixels more than 6 levels apart"
  if ($flicker.Count) {
    $worst = $flicker | Sort-Object Changed -Descending | Select-Object -First 1
    Save-Shot $worst.Shot 'objectives-changed'
    Add-Failure "the objectives strip, the same in both tabs, changed in $($flicker.Count) frames, at $(($flicker | ForEach-Object { '{0:N0} ms' -f $_.Ms }) -join ', ') (most $($worst.Changed) pixels, objectives-changed.png)"
  }
  if ([LbtermE2E.Px]::LumDistance($one, $two, $objStrip) -gt 0.5) { Add-Failure "the objectives strip isn't the same in both tabs" }
  # Fitted to tab_switch, stretched by the slowdown: the start that fits best, and how far off
  $whole = 200 * $Slowdown
  if ($stripMid.Count -ge 3) {
    $points = @($trace | Where-Object { $_.Bars -lt 0.98 })
    $t0s = @(-$whole..0 | Where-Object { $_ % 2 -eq 0 } | ForEach-Object { $stripMid[0].Ms + $_ })
    $fit = $t0s | ForEach-Object { $t0 = $_
      $sq = 0; foreach ($p in $points) { $d = (1 - (& $curve (($p.Ms - $t0) / $whole))) - ($p.Bars + $p.Thumb) / 2; $sq += $d * $d }
      [pscustomobject]@{ T0 = $t0; Rms = [Math]::Sqrt($sq / [Math]::Max(1, $points.Count)) } } | Sort-Object Rms | Select-Object -First 1
    "switch: the scroll strip against tab_switch at slowdown ${Slowdown}: rms $('{0:N3}' -f $fit.Rms) over $($points.Count) frames"
    if ($fit.Rms -gt 0.08) { Add-Failure "the strips' fade didn't follow tab_switch: rms $('{0:N3}' -f $fit.Rms)" }
  }
  $half = $mid | Sort-Object { [Math]::Abs($_.Panes - 0.5) } | Select-Object -First 1
  if ($half) { Save-Shot $half.Shot 'strips' }
  $script:trackRect = New-Rect ([int]$track.x) $top ([int]$track.w) ($bottom - $top)
  Save-StepSheet $r.Shots 'switch' $script:trackRect
  Remove-Frames $r.Shots

  # 2. Back mid-fade
  Switch-Tab $w F8 0 | Out-Null
  Wait-Settled -Animating | Out-Null
  $script:turned = $null
  $back = Switch-Tab $w F9 1 -Frames -Each { param($ms)
    if ($script:turned) { return }
    $s = Get-StateSnapshot $w.StateFile
    $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
    $d = if ($sw) { @($sw.animations | Where-Object name -eq 'sides.dissolve')[0] }
    if ($d -and $d.progress -ge 0.12) { Send-Key $w F8; $script:turned = $ms }
  }
  $atOne = Wait-Change { param($s) $s.active_tab -eq 0 } -What 'back on tab one' -Animating
  $trace = @($back.Shots | ForEach-Object { $x = & $share $_.Shot; $x | Add-Member Ms $_.Ms; $x | Add-Member Shot $_.Shot; $x })
  $before = @($trace | Where-Object { $script:turned -and $_.Ms -lt $script:turned })
  $after = @($trace | Where-Object { $script:turned -and $_.Ms -ge $script:turned })
  $coming = @($after | Where-Object { $_.Panes -lt 0.97 })
  $strip = { param($x) ($x.Bars + $x.Thumb) / 2 }
  "back: turned $('{0:N0}' -f $script:turned) ms in; the strips' share of tab one before: $(Format-Trace @($before | ForEach-Object { & $strip $_ })); after, on the way: $(Format-Trace @($coming | ForEach-Object { & $strip $_ }))"
  "back: the panes' before: $(Format-Trace @($before | ForEach-Object Panes)); after: $(Format-Trace @($coming | ForEach-Object Panes))"
  if (-not $script:turned) { Add-Failure "never turned back: no sides.dissolve 12% through" }
  else {
    $low = (@($before | ForEach-Object { & $strip $_ }) | Measure-Object -Minimum).Minimum
    if ($null -eq $low -or $low -gt 0.9) { Add-Failure "the strips never started fading before the turn (tab one's share stayed at $low)" }
    $peakBefore = (@($before | ForEach-Object { & $strip $_ }) | Select-Object -Last 2 | Measure-Object -Maximum).Maximum
    $high = (@($after | Select-Object -First 3 | ForEach-Object { & $strip $_ }) | Measure-Object -Maximum).Maximum
    if ($high -gt $peakBefore + 0.35) { Add-Failure "the switch back jumped on the strips: tab one's share went from $('{0:N2}' -f $peakBefore) to $('{0:N2}' -f $high) in the frames after the turn" }
    $apart = @($coming | Where-Object { [Math]::Abs((& $strip $_) - $_.Panes) -gt 0.25 })
    if ($apart.Count -gt [Math]::Max(2, $coming.Count / 4)) { Add-Failure "the strips and the panes turned back apart in $($apart.Count) of $($coming.Count) frames" }
  }
  if (@($atOne.fading_sides)[0]) { Add-Failure "pictures of the strips still fading once back on one: $($atOne.fading_sides)" }
  $turn = $after | Select-Object -First 2 | Select-Object -Last 1
  if ($turn) { Save-Shot $turn.Shot 'back' }
  if ($script:turned) { "back: the turn's key went at $('{0:N0}' -f $script:turned) ms" }
  Save-StepSheet $back.Shots 'back'
  Remove-Frames $back.Shots

  # 3. Rapid: five on, round all three
  Switch-Tab $w F9 1 | Out-Null; Wait-Settled -Animating | Out-Null
  Switch-Tab $w F9 2 | Out-Null; Wait-Settled -Animating | Out-Null
  $threeShot = Get-Shot $w -Name 'three'
  $s3 = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  Switch-Tab $w F9 0 | Out-Null; Wait-Settled -Animating | Out-Null
  $sides = New-Rect ([int]$s1.scroll_strip.strip.x) ([int]$s1.objectives.strip.y) ([int]($s1.objectives.strip.x + $s1.objectives.strip.w - $s1.scroll_strip.strip.x)) ([int]$s1.objectives.strip.h)
  $script:sent = 0
  $frames = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
    if ($script:sent -lt 5 -and $ms -ge 40 * $script:sent) { Send-Key $w F9; $script:sent++ }
  } -Until { param($s) $script:sent -ge 5 -and $s.active_tab -eq 2 -and -not $s.animating }
  $end = Wait-Settled -Animating
  $endShot = Get-Shot $w -Name 'rapid-end'
  "rapid: $($script:sent) keys, ended on tab $($end.active_tab); fading sides $($end.fading_sides), panes $(@($end.fading_panes).Count)"
  if ($end.active_tab -ne 2) { Add-Failure "five keys on from tab one ended on tab $($end.active_tab), not three" }
  if ($end.fading_sides) { Add-Failure "pictures of the strips still fading after the rapid switches: $($end.fading_sides)" }
  $d = [LbtermE2E.Px]::LumDistance($threeShot, $endShot, $sides)
  if ($d -gt 0.5) { Add-Failure "the rapid switches didn't land on tab three's strips: $('{0:N2}' -f $d) apart (rapid-end.png, three.png)" }
  Save-StepSheet $frames 'rapid'
  Remove-Frames $frames

  # 3b. Round to the oldest: from tab one, the forward button three times 40 ms apart, as fast as a
  # hand clicks, so the third goes round to one while two still fades under it: one's strips come
  # back with its panes from on top of the stack, two's go on from what they showed, played again
  # under the stack that's left (lbterm_motion::flight), three's go in at the bottom. What each tab
  # shows of the scroll strip and of the panes, by least squares of the three tabs' settled shots
  Switch-Tab $w F9 0 | Out-Null; Wait-Settled -Animating | Out-Null
  $cx, $cy = Get-RectCenter $card
  $refs3 = [System.Drawing.Bitmap[]]@($one, $two, $threeShot)
  $trackRect = ConvertTo-Rect $track
  $paneRect = ConvertTo-Rect $card
  $script:sentB = [Collections.Generic.List[double]]::new()
  $frames = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
    if ($script:sentB.Count -lt 3 -and $ms -ge 40 * $script:sentB.Count) { Send-SideButton $w $cx $cy -Forward; $script:sentB.Add($ms) }
  } -Until { param($s) $script:sentB.Count -ge 3 -and $s.active_tab -eq 0 -and -not $s.animating }
  $round = Wait-Settled -Animating
  $trace = @($frames | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Strips = [LbtermE2E.Px]::Mix($_.Shot, $refs3, $trackRect); Panes = [LbtermE2E.Px]::Mix($_.Shot, $refs3, $paneRect); Shot = $_.Shot } })
  $named = @('one', 'two', 'three')
  "round: the presses at $(Format-Trace @($script:sentB) '{0:N0}') ms, ended on tab $($round.active_tab + 1); fading sides $($round.fading_sides)"
  for ($k = 0; $k -lt 3; $k++) { "round: tab $($named[$k]), its panes $(Format-Trace @($trace | ForEach-Object { if ($_.Panes.Count -eq 3) { $_.Panes[$k] } })); its scroll strip $(Format-Trace @($trace | ForEach-Object { if ($_.Strips.Count -eq 3) { $_.Strips[$k] } }))" }
  if ($script:sentB.Count -lt 3 -or $round.active_tab -ne 0) { Add-Failure "round to the oldest: three presses from tab one ended on tab $($round.active_tab + 1), not one" }
  elseif ($round.fading_sides) { Add-Failure "round to the oldest: pictures of the strips still fading once it settled: $($round.fading_sides)" }
  else {
    # Each tab's share of the scroll strip keeping its panes', two's never rising again in flight
    # and one's never jumping as it comes back are the compositor's given the panes each press
    # left and the strips' picture, which compositor/imp/tests/flight.rs holds every 2 ms from
    # what DWM was handed (feat/dcomp-recorder). The run is judged here, its traces printed above
    # and its frames kept for the sheet and lbterm.ps1 look; steps 1 and 2 judge what the picture
    # shows, a true cross-fade in step with the panes
    "round to the oldest: three presses went round to tab one, nothing left fading"
  }
  Save-StepSheet $frames 'round'
  Remove-Frames $frames
  Switch-Tab $w F8 2 | Out-Null; Wait-Settled -Animating | Out-Null

  # 4. Close: tab one, settled, then its pane gone. The rows below move up; the third row's mark
  # (tab three's grey ring) is the strips' share of how they were
  Switch-Tab $w F9 0 | Out-Null
  $sBefore = Wait-Settled -Animating
  $beforeShot = Get-Shot $w -Name 'before-close'
  $third = ConvertTo-Rect @($sBefore.objectives.marks | Where-Object row -eq 2)[0].rect
  $states = [Collections.Generic.List[object]]::new()
  $script:killed = $false
  $shots = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
    if (-not $script:killed) { $script:killed = $true; Invoke-Cli kill-pane --pane-id $tabs.Panes.one | Out-Null }
  } -Until { param($s) $states.Add($s); @($s.tabs).Count -eq 2 -and -not $s.animating }
  $r = [pscustomobject]@{ Shots = $shots; States = @($states) }
  $sAfter = Wait-Settled -Animating
  $afterShot = Get-Shot $w -Name 'after-close'
  $names = @($r.States | ForEach-Object { $_.animations } | ForEach-Object name | Select-Object -Unique)
  "close: $(@($sAfter.objectives.marks).Count) rows marked after; animating on the way $($names -join ', ')"
  if ($names -notcontains 'sides.dissolve') { Add-Failure "the strips didn't dissolve as the tab closed: $($names -join ', ')" }
  $hiC = & $lum $beforeShot $third; $loC = & $lum $afterShot $third
  $hiP = & $lum $beforeShot $left; $loP = & $lum $afterShot $left
  if ($hiC - $loC -lt 20) { Add-Failure "the third row's mark didn't show against the strip ($('{0:N0}' -f $loC) to $('{0:N0}' -f $hiC))" }
  $trace = @($r.Shots | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Mark = ((& $lum $_.Shot $third) - $loC) / [Math]::Max(1, $hiC - $loC); Panes = ((& $lum $_.Shot $left) - $loP) / [Math]::Max(1, $hiP - $loP); Shot = $_.Shot } })
  $mid = @($trace | Where-Object { $_.Panes -gt 0.05 -and $_.Panes -lt 0.95 })
  "close: tab one's share, panes: $(Format-Trace @($mid | ForEach-Object Panes)); the third row's mark: $(Format-Trace @($mid | ForEach-Object Mark))"
  if (@($trace | Where-Object { $_.Mark -gt 0.05 -and $_.Mark -lt 0.95 }).Count -lt 2) { Add-Failure "the marks cut as the tab closed while its panes faded" }
  $behind = @($mid | Where-Object { [Math]::Abs($_.Mark - $_.Panes) -gt 0.2 })
  if ($behind.Count -gt [Math]::Max(2, $mid.Count / 4)) { Add-Failure "the marks and the closed tab's panes weren't in step in $($behind.Count) of $($mid.Count) frames" }
  if ($sAfter.fading_sides) { Add-Failure "pictures of the strips still fading after the close: $($sAfter.fading_sides)" }
  $halfway = $mid | Sort-Object { [Math]::Abs($_.Panes - 0.5) } | Select-Object -First 1
  if ($halfway) { Save-Shot $halfway.Shot 'close' }
  Save-StepSheet $r.Shots 'close' $third
  Remove-Frames $r.Shots
  $beforeShot.Dispose(); $afterShot.Dispose()

  # 5. Motion off, in a window of its own: there at once, landing on the same strips
  $off = Open-Tabs 'off'
  $wo = $off.Window
  $offOne = Get-Shot $wo
  $r = Switch-Tab $wo F9 1 -Frames
  $offTwo = Get-Shot $wo -Name 'off-two'
  $between = @($r.Shots | Where-Object { $b = ((Get-Light $_.Shot $bars) - $loB) / [Math]::Max(1, $hiB - $loB); $b -gt 0.05 -and $b -lt 0.95 })
  $names = @($r.States | ForEach-Object { $_.animations } | ForEach-Object name | Select-Object -Unique)
  $box = Get-DiffBox $two $offTwo $sides
  "off: $($between.Count) of $($r.Shots.Count) frames between on the strips; animating $($names -join ', '); against the window with motion: $(Format-Box $box)"
  if ($between.Count) { Add-Failure "with motion off the strips took $($between.Count) frames" }
  if ($names -contains 'sides.dissolve') { Add-Failure 'with motion off the strips still dissolved' }
  if ($box) { Add-Failure "with motion off tab two's strips aren't what they are with motion on: they differ over $(Format-Box $box) (off-two.png, two.png)" }
  Save-StepSheet $r.Shots 'off'
  Remove-Frames $r.Shots
  $offOne.Dispose()
  Use-TestWindow $w
} -Passed "the strips down the right: the old tab's fade out over the new tab's on tab_switch in step with its panes, a true cross-fade, a switch back starts from what shows, rapid switches land with nothing left fading, a switch round to the oldest of three in flight keeps each tab's strips with its panes, a closed tab's marks fade where they were, motion off lands on the same strips"
