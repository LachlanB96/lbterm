#Requires -Version 7
# Covers: lbterm-panel/src/**, lbterm-extras/src/lights.rs, lbterm-extras/src/tabs.rs, lbterm-motion/src/**, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/compositor.rs
<#
The tab rail's answers (lbterm/FEEDBACK.md, lbterm/TAB-PANEL.md) end to end, in a test window
that looks like Lachlan's LBTerm (New-TestConfig) with the tab panel on and motion at -Slowdown.
Each tab runs a stand-in that prints whatever this script leaves in its command file: OSC 7501
records, as Claude Code sends them.

1. The open tab changes: its ring and bar glide from its old row to its new one (focus_move),
   never past either (marker.png, a frame on the way), and the new row's open fill comes in
   over frames (the rows' surfaces are clear where nothing's drawn, so it once showed at once).
2. A tab's program goes from working to blocked: its row dissolves into the new look over `swap`
   (status.png).
3. `lbterm cli send-text` types into a tab: its pad glints, and the glint goes (glint.png).
4. A tab renamed through `lbterm cli set-tab-title`: its label dissolves into the new name
   (rename.png). (Renaming from the tab menu glints its pad too.)
5. The panel open, a tab's progress goes from 20% to 80%: its bar glides, every pixel of it
   either the old bar or the new one on the way, not a dissolve between them (progress.png).
6. A tab made without taking the keyboard grows into the rail (grow.png).
7. A middle tab closed: it fades and the rows under it slide up (close.png).
8. A row dragged down the rail: it's lifted and follows the pointer (lifted.png), and lands
   where it's let go.
9. The open tab closed: its panes' last frame fades into the next tab's (tab_switch), the
   strip's words with them, and the next tab's row takes the open look over frames
   (open-close.png).

Each step records frames until the state file (lbterm/STATE.md) says the rail shows the change
and nothing moves any more, and the frames say how it got there. The rows' pads come from the
file too. Captures go to target\perf\rail-feedback. Fails with exit 1.

  pwsh -File lbterm\perf\rail-feedback.ps1 [-Slowdown 4]
#>
param([double]$Slowdown = 4, [string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'rail-feedback' -Bin $Bin
$commands = Join-Path $T.Out 'commands'
$sums = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $commands, $sums, $home2 | Out-Null

# The accent the open tab's ring and bar are drawn in, #FF5449, give or take the soft edge
$accent = New-ColourRange 201 255 51 129 41 119

# The rail as it shows `what`, settled, captured over columns x0..x1 (a held pointer kept in place)
function Shows($w, [scriptblock]$what, [string]$name, [string]$desc) {
  $script:entry = Wait-Change $what -What $desc -Animating
  Get-Shot $w -Name $name
}
# Frames until the rail shows `what` and nothing moves any more (at most $Ms), a little after
function Record($w, [int]$Ms, [scriptblock]$what) {
  Get-Frames $w $Ms -TailMs 120 -Until { param($s) (& $what $s) -and -not $s.animating }
}

function Distance($a, $b, [int]$x0, [int]$x1) { [LbtermE2E.Px]::LumDistance($a, $b, (Get-Columns $x0 $x1 -Of $a)) }

# The frames that are neither `before` nor `after` over columns x0..x1
function Between($frames, $before, $after, [int]$x0, [int]$x1) {
  $whole = Distance $before $after $x0 $x1
  @($frames | Where-Object {
      (Distance $before $_.Shot $x0 $x1) -gt 0.04 * $whole -and
      (Distance $after $_.Shot $x0 $x1) -gt 0.04 * $whole })
}

# The frames whose `rect` is neither `before` nor `after`, the open tab's ring left out: it
# glides over the rows on its way
function Coming($frames, $before, $after, $rect) {
  $whole = [LbtermE2E.Px]::LumDistance($before, $after, $rect)
  @($frames | Where-Object {
      [LbtermE2E.Px]::CountRange($_.Shot, $rect, $accent) -eq 0 -and
      [LbtermE2E.Px]::LumDistance($before, $_.Shot, $rect) -gt 0.1 * $whole -and
      [LbtermE2E.Px]::LumDistance($after, $_.Shot, $rect) -gt 0.1 * $whole })
}

# Beside the open row's pad, inside its pill: where its open fill shows
function Fill-Of($state) {
  $pad = @($state.rail.rows | Where-Object { $_.active })[0].pad
  New-Rect ($pad.x - (& $px 6)) $pad.y (& $px 4) $pad.h
}

function Save-Middle($frames, [string]$name) {
  if ($frames.Count) { Save-Shot $frames[[int]($frames.Count / 2)].Shot $name }
}

# What a stand-in tab runs: print whatever turns up in its command file
$e = [char]27
function Stand-In([string]$name) {
  $file = Join-Path $commands "$name.txt"
  New-PwshProgram "Write-Host 'rail feedback: $name'; while (`$true) { if (Test-Path -LiteralPath '$file') { `$t = [IO.File]::ReadAllText('$file'); Remove-Item -LiteralPath '$file'; [Console]::Write(`$t) }; Start-Sleep -Milliseconds 50 }"
}
# Leave `records` (OSC 7501 fields) for stand-in `name`, whole: written aside, then renamed in
function Send-To([string]$name, [string[]]$records) {
  $file = Join-Path $commands "$name.txt"
  Write-FileWhole -Path $file -Text (($records | ForEach-Object { "$e]7501;$_$e\" }) -join '') -Temp "$file.part"
}

Invoke-Test {
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo -Program (Stand-In one) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true, open_delay_ms = 0 }
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.lbterm_motion = { enabled = 'on', slowdown = $Slowdown }
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $pane = @{ one = (Get-Panes)[0].pane_id }
  foreach ($n in 'two', 'three') { $pane[$n] = New-Tab -From $pane.one -Program (Stand-In $n) }
  foreach ($n in 'one', 'two', 'three') { Invoke-Cli set-tab-title --pane-id $pane[$n] $n | Out-Null }
  Send-To two 'state=working:app=claude'
  Send-To three 'state=working:app=claude:progress=20'
  Invoke-Cli activate-tab --pane-id $pane.one --tab-index 0 | Out-Null

  $px = { param($v) ConvertTo-Px $v $w.Scale }
  # The closed rail, short of the panes' cards
  $rail = & $px 74
  $start = Shows $w { param($s) $rows = @($s.rail.rows); $rows.Count -eq 3 -and $rows[1].state -eq 'working' -and $rows[2].progress -and $s.active_tab -eq 0 } 'start' 'three rows, two working'

  # 1. The open tab changes: its marker glides from the first row to the third
  $markerX = & $px 30
  $accentY = { param($shot) [LbtermE2E.Px]::MeanYInRange($shot, (Get-Columns 0 $markerX -Of $shot), $accent) }
  $from = & $accentY $start
  Invoke-Cli activate-tab --pane-id $pane.one --tab-index 2 | Out-Null
  $frames = Record $w ([int](300 * $Slowdown + 600)) { param($s) $s.rail.open_row -eq 2 }
  $third = Shows $w { param($s) $s.rail.open_row -eq 2 } 'marker-end' 'the third tab open'
  $to = & $accentY $third
  $ys = @($frames | ForEach-Object { & $accentY $_.Shot } | Where-Object { $_ -ge 0 })
  $span = $to - $from
  $onWay = @($ys | Where-Object { ($_ - $from) / $span -gt 0.1 -and ($_ - $from) / $span -lt 0.9 })
  $past = @($ys | Where-Object { ($_ - $from) / $span -gt 1.04 -or ($_ - $from) / $span -lt -0.04 })
  "marker: row y $('{0:N0}' -f $from) to $('{0:N0}' -f $to); $($onWay.Count) of $($ys.Count) frames on the way, $($past.Count) past either end"
  if ($from -lt 0 -or $to -lt 0 -or $span -lt (& $px 40)) { Add-Failure "no marker found on the rail ($from, $to)" }
  elseif ($onWay.Count -lt 2) { Add-Failure "the marker jumped: $($onWay.Count) frames on the way between rows" }
  if ($past.Count) { Add-Failure "the marker went past its rows in $($past.Count) frames" }
  $fill = Fill-Of $script:entry
  $coming = Coming $frames $start $third $fill
  "marker: the third row's open fill $('{0:N1}' -f [LbtermE2E.Px]::LumDistance($start, $third, $fill)) brighter, $($coming.Count) frames on the way in"
  if ([LbtermE2E.Px]::LumDistance($start, $third, $fill) -lt 2) { Add-Failure "the third row showed no open fill beside its pad (marker-end.png)" }
  elseif ($coming.Count -lt 2) { Add-Failure "the third row's open fill came at once: $($coming.Count) frames on the way in" }
  $mid = @($frames | Where-Object { $y = & $accentY $_.Shot; $y -ge 0 -and [Math]::Abs(($y - $from) / $span - 0.5) -lt 0.25 })
  Save-Middle $mid 'marker'
  Remove-Frames $frames

  # 2. Tab two blocks on a question: its row dissolves into the new look
  Send-To two 'state=blocked:kind=question:app=claude'
  $blockedNow = { param($s) @($s.rail.rows)[1].state -eq 'waiting' }
  $frames = Record $w ([int](200 * $Slowdown + 800)) $blockedNow
  $blocked = Shows $w $blockedNow 'status-end' 'the second row blocked'
  $between = Between $frames $third $blocked 0 $rail
  "status: $($between.Count) of $($frames.Count) frames between the working row and the blocked one"
  if ((Distance $third $blocked 0 $rail) -eq 0) { Add-Failure 'the row never showed the new state' }
  elseif ($between.Count -lt 2) { Add-Failure "the row's new state came at once: $($between.Count) frames between" }
  Save-Middle $between 'status'
  Remove-Frames $frames

  # 3. The cli types into tab two: its pad glints, then nothing's left of it
  $light = { param($shot) [LbtermE2E.Px]::Light($shot, (Get-Columns 0 $rail -Of $shot)) }
  $rest = & $light $blocked
  Invoke-Cli send-text --pane-id $pane.two --no-paste 'x' | Out-Null
  $script:glinted = $false
  $frames = Record $w ([int](580 * $Slowdown + 800)) { param($s)
    if (@($s.animations | ForEach-Object name) -contains 'light.glint') { $script:glinted = $true }
    $script:glinted }
  $lights = @($frames | ForEach-Object { & $light $_.Shot })
  $peak = ($lights | Measure-Object -Maximum).Maximum
  $after = Shows $w { param($s) $true } 'glint-end' 'the glint to go'
  "glint: the rail's light $('{0:N2}' -f $rest), at its brightest $('{0:N2}' -f $peak), after $('{0:N2}' -f (& $light $after))"
  if ($peak -lt $rest + 0.15) { Add-Failure 'typing from the cli lit nothing on the rail' }
  if ((Distance $blocked $after 0 $rail) -gt 0) { Add-Failure 'the glint left something behind (glint-end.png)' }
  $brightest = [Array]::IndexOf($lights, $peak)
  if ($brightest -ge 0) { Save-Shot $frames[$brightest].Shot 'glint' }
  Remove-Frames $frames

  # 4. Tab one renamed: its label dissolves into the name
  Invoke-Cli set-tab-title --pane-id $pane.one 'Renamed' | Out-Null
  $renamedNow = { param($s) @($s.rail.rows)[0].title -eq 'Renamed' }
  $frames = Record $w ([int](580 * $Slowdown + 800)) $renamedNow
  $renamed = Shows $w $renamedNow 'rename-end' 'the first row renamed'
  $between = Between $frames $after $renamed 0 $rail
  "rename: $($between.Count) of $($frames.Count) frames between the old label and the new"
  if ((Distance $after $renamed 0 $rail) -eq 0) { Add-Failure 'the rail never showed the new name' }
  elseif ($between.Count -lt 2) { Add-Failure "the new name came at once: $($between.Count) frames between" }
  Save-Middle $between 'rename'
  Remove-Frames $frames

  # 5. Open, the pointer below the rows so none is hovered: tab three's progress glides on
  $cr = Get-ClientRect $w
  Set-PointerHold $w (& $px 36) ($cr.Bottom - (& $px 60))
  $panel = & $px 330
  $at20 = Shows $w { param($s) $s.rail.open -and $null -eq $s.rail.hovered } 'progress-20' 'the panel open, no row hovered'
  Send-To three 'state=working:app=claude:progress=80'
  $at80Now = { param($s) "$(@($s.rail.rows)[2].progress)" -match '80' }
  $frames = Record $w ([int](220 * $Slowdown + 800)) $at80Now
  $at80 = Shows $w $at80Now 'progress-80' 'the third row at 80%'
  $glide = @($frames | ForEach-Object { , [LbtermE2E.Px]::Glide($at20, $at80, $_.Shot, (Get-Columns 0 $panel -Of $at20)) })
  $partway = @($glide | Where-Object { $_[0] -gt 0.15 -and $_[0] -lt 0.85 })
  $clean = @($partway | Where-Object { $_[1] -ge 0.8 })
  "progress: $([int]($glide[0][2])) pixels change; $($partway.Count) of $($frames.Count) frames part of the way, $($clean.Count) of them old or new at every pixel"
  if ($glide.Count -eq 0 -or $glide[0][2] -lt (& $px 20)) { Add-Failure 'the open panel showed no progress change' }
  elseif ($partway.Count -lt 2) { Add-Failure "the progress bar jumped: $($partway.Count) frames part of the way" }
  elseif ($clean.Count -lt $partway.Count / 2) { Add-Failure 'the progress bar dissolved instead of gliding' }
  $i = [Array]::FindIndex([object[]]$glide, [Predicate[object]] { param($g) $g[0] -gt 0.3 -and $g[0] -lt 0.7 })
  if ($i -ge 0) { Save-Shot $frames[$i].Shot 'progress' }
  Remove-Frames $frames
  # Off the panel, over the panes: it closes
  Clear-PointerHold
  Send-Mouse $w Move (& $px 600) (& $px 300)
  $closed = Shows $w { param($s) $s.rail.phase -eq 'closed' -and $s.settled } 'closed' 'the panel closed'

  # 6. A tab made without taking the keyboard grows into the rail
  $pane.four = New-Tab -From $pane.one -NoActivate -Program (Stand-In four)
  $fourNow = { param($s) @($s.rail.rows).Count -eq 4 }
  $frames = Record $w ([int](220 * $Slowdown + 800)) $fourNow
  $grown = Shows $w $fourNow 'grow-end' 'four rows'
  $between = Between $frames $closed $grown 0 $rail
  "grow: $($between.Count) of $($frames.Count) frames between the rail without the tab and with it"
  if ((Distance $closed $grown 0 $rail) -eq 0) { Add-Failure 'the new tab never showed on the rail' }
  elseif ($between.Count -lt 2) { Add-Failure "the new tab came at once: $($between.Count) frames between" }
  Save-Middle $between 'grow'
  Remove-Frames $frames

  # 7. Tab two closed: it fades and three and four slide up
  Invoke-Cli kill-pane --pane-id $pane.two | Out-Null
  $threeNow = { param($s) @($s.rail.rows).Count -eq 3 }
  $frames = Record $w ([int](220 * $Slowdown + 800)) $threeNow
  $gone = Shows $w $threeNow 'close-end' 'three rows again'
  $between = Between $frames $grown $gone 0 $rail
  "close: $($between.Count) of $($frames.Count) frames between"
  if ((Distance $grown $gone 0 $rail) -eq 0) { Add-Failure 'the closed tab stayed on the rail' }
  elseif ($between.Count -lt 2) { Add-Failure "the closed tab went at once: $($between.Count) frames between" }
  Save-Middle $between 'close'
  Remove-Frames $frames

  # 8. The first row dragged down to the second's slot and a little past its middle: lifted on
  # the way, under the pointer, landed in that slot. The panel opens under the pointer first;
  # the pads are where the file says
  # Where the open tab's marker sat on the first row, and a row's height, as the marker's glide
  # measured them (from the pads' middles the open part came out 6 px short of the pointer)
  $x = & $px 36
  $pitch = $span / 2
  $y0 = [int]($from); $y1 = [int]($from + 1.3 * $pitch)
  Send-Mouse $w Move $x $y0
  (Shows $w { param($s) $s.rail.open } 'drag-start' 'the panel open under the pointer').Dispose()
  Send-Mouse $w LDown $x $y0 -Keys L
  Start-Sleep -Milliseconds 100
  foreach ($k in 1..12) {
    Send-Mouse $w Move $x ([int]($y0 + ($y1 - $y0) * $k / 12)) -Keys L
    Start-Sleep -Milliseconds 40
  }
  # Lifted: the rail says a row is being dragged; then the time its open part takes to catch the
  # pointer up, by the clock, since the state file doesn't count that as animating
  $lift = Wait-State { param($s) $null -ne $s.rail.dragging } -What 'the row lifted'
  Start-Sleep -Milliseconds ([int](220 * $Slowdown + 400))
  $lifted = Get-Shot $w -Name 'lifted'
  Send-Mouse $w LUp $x $y1
  $landed = Shows $w { param($s) $null -eq $s.rail.dragging } 'landed' 'the row let go'
  $d = Distance $lifted $landed 0 $rail
  "drag: row $($lift.rail.dragging) lifted; the lifted rail differs from where it lands by $('{0:N2}' -f $d)"
  if ($d -lt 0.2) { Add-Failure 'the dragged row showed no lift on its way (lifted.png)' }
  # Its open part (the highlight and words, right of the rail) goes with it, under the pointer
  $want = $y1 - [int]($from + $pitch)
  $x0 = & $px 90; $x1 = & $px 280
  $ya = [int]($y0 - 0.4 * $pitch); $yb = [int]($y0 + 0.4 * $pitch)
  $ya += [int]$pitch; $yb += [int]$pitch
  $shift = [LbtermE2E.Px]::BestShift($lifted, $landed, (New-Rect $x0 $ya ($x1 - $x0 + 1) ($yb - $ya + 1)), [int]($pitch / 2))
  "drag: the row's open part shows $shift px off its slot, the pointer $want px"
  if ([Math]::Abs($shift - $want) -gt (& $px 4)) { Add-Failure "the dragged row's open part stayed behind: $shift px off its slot, the pointer $want px (lifted.png)" }
  $order = @(Get-Panes | ForEach-Object tab_title)
  "drag: tabs now $($order -join ', ')"
  if ($order[1] -ne 'Renamed') { Add-Failure "the dragged tab is at $([array]::IndexOf($order, 'Renamed')), not 1" }

  # 9. The open tab closed: its panes fade into the next tab's, and that tab's row comes in
  Send-Mouse $w Move (& $px 600) (& $px 300)
  $at = Shows $w { param($s) $s.rail.phase -eq 'closed' -and $s.settled } 'open-close-start' 'the panel closed again'
  $was = $script:entry
  $tabs = @($was.rail.rows).Count
  Invoke-Cli kill-pane --pane-id @($was.panes)[0].id | Out-Null
  $nextNow = { param($s) @($s.rail.rows).Count -eq $tabs - 1 }.GetNewClosure()
  $frames = Record $w ([int](250 * $Slowdown + 800)) $nextNow
  $next = Shows $w $nextNow 'open-close-end' 'the next tab open'
  $cr = Get-ClientRect $w
  $panes = Between $frames $at $next (& $px 90) ($cr.Width - 1)
  "open close: $($panes.Count) of $($frames.Count) frames between the closed tab's panes and the next tab's"
  if ((Distance $at $next (& $px 90) ($cr.Width - 1)) -eq 0) { Add-Failure 'the panes never showed the next tab' }
  elseif ($panes.Count -lt 2) { Add-Failure "the next tab's panes came at once: $($panes.Count) frames between" }
  Save-Middle $panes 'open-close'
  $fill = Fill-Of $script:entry
  $coming = Coming $frames $at $next $fill
  "open close: the next row's open fill, $($coming.Count) frames on the way in"
  if ($coming.Count -lt 2) { Add-Failure "the next tab's row took its open fill at once: $($coming.Count) frames on the way in" }
  Remove-Frames $frames
} -Passed 'rail feedback: the marker glides and the open row dissolves in, a new state and a new name dissolve in, typed-into and renamed pads glint, progress glides, tabs grow in and fold away, a dragged row lifts, a closed open tab dissolves into the next'
