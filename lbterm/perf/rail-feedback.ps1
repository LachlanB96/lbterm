#Requires -Version 7
# Covers: lbterm-panel/src/**, lbterm-extras/src/lights.rs, lbterm-extras/src/tabs.rs, lbterm-motion/src/**, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/compositor.rs
<#
The tab rail's answers (lbterm/FEEDBACK.md, lbterm/TAB-PANEL.md) end to end, in a test window
that looks like Lachlan's LBTerm (New-TestConfig) with the tab panel on and motion at -Slowdown.
Each tab runs a stand-in that prints whatever this script leaves in its command file: OSC 7501
records, as Claude Code sends them.

1. The open tab changes: its ring and bar glide from its old row to its new one (focus_move),
   never past either (marker.png, a frame on the way).
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

Captures go to target\perf\rail-feedback. Fails with exit 1.

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

# A shot once two 700 ms apart match over columns x0..x1, by brightness (a held pointer kept in
# place meanwhile)
function Settled($w, [string]$name, [int]$x0, [int]$x1) { Wait-Still $w -Columns $x0, $x1 -Lum -Tries 20 -NoThrow -Name $name }

function Distance($a, $b, [int]$x0, [int]$x1) { [LbtermE2E.Px]::LumDistance($a, $b, (Get-Columns $x0 $x1 -Of $a)) }

# The frames that are neither `before` nor `after` over columns x0..x1
function Between($frames, $before, $after, [int]$x0, [int]$x1) {
  $whole = Distance $before $after $x0 $x1
  @($frames | Where-Object {
      (Distance $before $_.Shot $x0 $x1) -gt 0.04 * $whole -and
      (Distance $after $_.Shot $x0 $x1) -gt 0.04 * $whole })
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
  [IO.File]::WriteAllText("$file.part", (($records | ForEach-Object { "$e]7501;$_$e\" }) -join ''))
  Move-Item -LiteralPath "$file.part" -Destination $file -Force
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
  Start-Sleep 2
  $pane = @{ one = (Get-Panes)[0].pane_id }
  foreach ($n in 'two', 'three') { $pane[$n] = New-Tab -From $pane.one -Program (Stand-In $n) }
  foreach ($n in 'one', 'two', 'three') { Invoke-Cli set-tab-title --pane-id $pane[$n] $n | Out-Null }
  Send-To two 'state=working:app=claude'
  Send-To three 'state=working:app=claude:progress=20'
  Invoke-Cli activate-tab --pane-id $pane.one --tab-index 0 | Out-Null

  $px = { param($v) ConvertTo-Px $v $w.Scale }
  # The closed rail, short of the panes' cards
  $rail = & $px 74
  $start = Settled $w 'start' 0 $rail

  # 1. The open tab changes: its marker glides from the first row to the third
  $markerX = & $px 30
  $accentY = { param($shot) [LbtermE2E.Px]::MeanYInRange($shot, (Get-Columns 0 $markerX -Of $shot), $accent) }
  $from = & $accentY $start
  Invoke-Cli activate-tab --pane-id $pane.one --tab-index 2 | Out-Null
  $frames = Get-Frames $w ([int](300 * $Slowdown + 600))
  $third = Settled $w 'marker-end' 0 $rail
  $to = & $accentY $third
  $ys = @($frames | ForEach-Object { & $accentY $_.Shot } | Where-Object { $_ -ge 0 })
  $span = $to - $from
  $onWay = @($ys | Where-Object { ($_ - $from) / $span -gt 0.1 -and ($_ - $from) / $span -lt 0.9 })
  $past = @($ys | Where-Object { ($_ - $from) / $span -gt 1.04 -or ($_ - $from) / $span -lt -0.04 })
  "marker: row y $('{0:N0}' -f $from) to $('{0:N0}' -f $to); $($onWay.Count) of $($ys.Count) frames on the way, $($past.Count) past either end"
  if ($from -lt 0 -or $to -lt 0 -or $span -lt (& $px 40)) { Add-Failure "no marker found on the rail ($from, $to)" }
  elseif ($onWay.Count -lt 2) { Add-Failure "the marker jumped: $($onWay.Count) frames on the way between rows" }
  if ($past.Count) { Add-Failure "the marker went past its rows in $($past.Count) frames" }
  $mid = @($frames | Where-Object { $y = & $accentY $_.Shot; $y -ge 0 -and [Math]::Abs(($y - $from) / $span - 0.5) -lt 0.25 })
  Save-Middle $mid 'marker'
  Remove-Frames $frames
  # A row's height, for the drag
  $pitch = $span / 2

  # 2. Tab two blocks on a question: its row dissolves into the new look
  Send-To two 'state=blocked:kind=question:app=claude'
  $frames = Get-Frames $w ([int](200 * $Slowdown + 800))
  $blocked = Settled $w 'status-end' 0 $rail
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
  $frames = Get-Frames $w ([int](580 * $Slowdown + 800))
  $lights = @($frames | ForEach-Object { & $light $_.Shot })
  $peak = ($lights | Measure-Object -Maximum).Maximum
  $after = Settled $w 'glint-end' 0 $rail
  "glint: the rail's light $('{0:N2}' -f $rest), at its brightest $('{0:N2}' -f $peak), after $('{0:N2}' -f (& $light $after))"
  if ($peak -lt $rest + 0.15) { Add-Failure 'typing from the cli lit nothing on the rail' }
  if ((Distance $blocked $after 0 $rail) -gt 0) { Add-Failure 'the glint left something behind (glint-end.png)' }
  $brightest = [Array]::IndexOf($lights, $peak)
  if ($brightest -ge 0) { Save-Shot $frames[$brightest].Shot 'glint' }
  Remove-Frames $frames

  # 4. Tab one renamed: its label dissolves into the name
  Invoke-Cli set-tab-title --pane-id $pane.one 'Renamed' | Out-Null
  $frames = Get-Frames $w ([int](580 * $Slowdown + 800))
  $renamed = Settled $w 'rename-end' 0 $rail
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
  $at20 = Settled $w 'progress-20' 0 $panel
  Send-To three 'state=working:app=claude:progress=80'
  $frames = Get-Frames $w ([int](220 * $Slowdown + 800))
  $at80 = Settled $w 'progress-80' 0 $panel
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
  $closed = Settled $w 'closed' 0 $panel

  # 6. A tab made without taking the keyboard grows into the rail
  $pane.four = New-Tab -From $pane.one -NoActivate -Program (Stand-In four)
  $frames = Get-Frames $w ([int](220 * $Slowdown + 800))
  $grown = Settled $w 'grow-end' 0 $rail
  $between = Between $frames $closed $grown 0 $rail
  "grow: $($between.Count) of $($frames.Count) frames between the rail without the tab and with it"
  if ((Distance $closed $grown 0 $rail) -eq 0) { Add-Failure 'the new tab never showed on the rail' }
  elseif ($between.Count -lt 2) { Add-Failure "the new tab came at once: $($between.Count) frames between" }
  Save-Middle $between 'grow'
  Remove-Frames $frames

  # 7. Tab two closed: it fades and three and four slide up
  Invoke-Cli kill-pane --pane-id $pane.two | Out-Null
  $frames = Get-Frames $w ([int](220 * $Slowdown + 800))
  $gone = Settled $w 'close-end' 0 $rail
  $between = Between $frames $grown $gone 0 $rail
  "close: $($between.Count) of $($frames.Count) frames between"
  if ((Distance $grown $gone 0 $rail) -eq 0) { Add-Failure 'the closed tab stayed on the rail' }
  elseif ($between.Count -lt 2) { Add-Failure "the closed tab went at once: $($between.Count) frames between" }
  Save-Middle $between 'close'
  Remove-Frames $frames

  # 8. The first row dragged down to the second's slot and a little past its middle: lifted on
  # the way, under the pointer, landed in that slot. The panel opens under the pointer first
  $x = & $px 36
  $y0 = [int]($from); $y1 = [int]($from + 1.3 * $pitch)
  Send-Mouse $w Move $x $y0
  (Settled $w 'drag-start' 0 $panel).Dispose()
  Send-Mouse $w LDown $x $y0 -Keys L
  Start-Sleep -Milliseconds 100
  foreach ($k in 1..12) {
    Send-Mouse $w Move $x ([int]($y0 + ($y1 - $y0) * $k / 12)) -Keys L
    Start-Sleep -Milliseconds 40
  }
  Start-Sleep -Milliseconds ([int](220 * $Slowdown + 400))
  $lifted = Get-Shot $w -Name 'lifted'
  Send-Mouse $w LUp $x $y1
  $landed = Settled $w 'landed' 0 $rail
  $d = Distance $lifted $landed 0 $rail
  "drag: the lifted rail differs from where it lands by $('{0:N2}' -f $d)"
  if ($d -lt 0.2) { Add-Failure 'the dragged row showed no lift on its way (lifted.png)' }
  # Its open part (the highlight and words, right of the rail) goes with it, under the pointer
  $want = $y1 - [int]($from + $pitch)
  $x0 = & $px 90; $x1 = & $px 280
  $ya = [int]($from + 0.6 * $pitch); $yb = [int]($from + 1.4 * $pitch)
  $shift = [LbtermE2E.Px]::BestShift($lifted, $landed, (New-Rect $x0 $ya ($x1 - $x0 + 1) ($yb - $ya + 1)), [int]($pitch / 2))
  "drag: the row's open part shows $shift px off its slot, the pointer $want px"
  if ([Math]::Abs($shift - $want) -gt (& $px 4)) { Add-Failure "the dragged row's open part stayed behind: $shift px off its slot, the pointer $want px (lifted.png)" }
  $order = @(Get-Panes | ForEach-Object tab_title)
  "drag: tabs now $($order -join ', ')"
  if ($order[1] -ne 'Renamed') { Add-Failure "the dragged tab is at $([array]::IndexOf($order, 'Renamed')), not 1" }
} -Passed 'rail feedback: the marker glides, a new state and a new name dissolve in, typed-into and renamed pads glint, progress glides, tabs grow in and fold away, a dragged row lifts'
