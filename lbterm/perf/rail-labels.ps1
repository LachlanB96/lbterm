#Requires -Version 7
# Covers: lbterm-panel/src/**, lbterm-extras/src/summaries.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, lbterm/motion/demo-tab.ps1
<#
The tab rail's labels and the open tab's marks (lbterm/TAB-PANEL.md), end to end. A test window
looking like Lachlan's LBTerm (New-TestConfig) with lbterm_summaries pointed at a scratch folder
(<out>\summaries) gets five tabs: three with a Claude session whose label the summary worker
would have written there, and the folder its statusline would have (the open panel's project
chips say Garden, Terminal and LBTerm, not the Projects their shells report), one renamed, one
plain. The third is the open tab.

1. Closed: rail-labels.png, and the rail three times as big in rail-labels-rail.png. The open tab
   must have its red bar down the rail's left edge and its red ring.
2. A session's label file is rewritten, as the worker would: the rail must change where that
   label is, with nothing done to the window (rail-labels-changed.png).
3. Open, the pointer posted onto the rail: rail-labels-open.png. The ring goes round the whole row.

And Lachlan's 12a (lbterm/NEW-TAB.md, "Landing 4"), a quieter rail: closed, only the idle timer's
lit bars are drawn (none over the working pad or the shells', none of the unlit #24242C anywhere
above a pad, the bottom one alone over the question two minutes old, all three over the one
twenty minutes old) and the state file says
which (`bars`); open, every label but the open tab's has no bright pixel left in its room, where
closed each has its words, and the state file's `labels` says `faded` (`shown` closed).

Its USERPROFILE is a scratch folder, so lbterm.lua's handlers write nothing into the real
~\.claude. Fails with exit 1.

  pwsh -File lbterm\perf\rail-labels.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'rail-labels' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $summaries, $home2 | Out-Null

$vault = 'C:\Notes\Life\Projects'
$sessions = @(
  @{ sid = '11111111-1111-4111-8111-111111111111'; label = 'Tab bar bugs'; state = 'working'; cwd = "$vault\Garden" }
  @{ sid = '22222222-2222-4222-8222-222222222222'; label = 'Lodestone'; state = 'waiting'; age = 20; cwd = "$vault\Terminal" }
  @{ sid = '33333333-3333-4333-8333-333333333333'; label = 'Scene realism'; state = 'waiting'; age = 2; cwd = 'C:\Code\LBTerm' }
)
foreach ($s in $sessions) {
  [IO.File]::WriteAllText((Join-Path $summaries "$($s.sid).label.txt"), $s.label)
  [IO.File]::WriteAllText((Join-Path $summaries "$($s.sid).cwd.txt"), $s.cwd)
}

# The accent the open tab's ring and bar are drawn in, #FF5449, give or take the soft edge
$accent = New-ColourRange 201 255 51 129 41 119
# How many accent pixels lie in a column range, over the whole height
function Measure-Accent($shot, [int]$x0, [int]$x1) { [LbtermE2E.Px]::CountRange($shot, (Get-Columns $x0 $x1 -Of $shot), $accent) }
function Demo([string]$Title, [string]$State = 'none', [double]$Age = 0, [string]$Session) {
  $a = @('-Title', $Title, '-State', $State, '-AgeMinutes', $Age)
  if ($Session) { $a += @('-Session', $Session) }
  New-PwshProgram -File $demoTab -ArgumentList $a
}

Invoke-Test {
  $first = $sessions[0]
  # LBTERM_POSTED_POINTER: a posted move hovers (the window doesn't ask Windows to say when the
  # real pointer, elsewhere, leaves it), so the panel opens under the moves in step 3
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo `
    -Set 'lbterm_compositor=true', 'lbterm_tab_panel={enabled=true,open_delay_ms=0}', "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'" `
    -Program (Demo one $first.state -Session $first.sid)
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  foreach ($s in $sessions | Select-Object -Skip 1) {
    Invoke-Cli spawn --window-id 0 -Program (Demo two $s.state $s.age $s.sid) | Out-Null
  }
  $named = Invoke-Cli spawn --window-id 0 -Program (Demo named)
  Invoke-Cli set-tab-title --pane-id $named 'Weekly review' | Out-Null
  Invoke-Cli spawn --window-id 0 -Program (Demo plain) | Out-Null
  Invoke-Cli activate-tab --pane-id 0 --tab-index 2 | Out-Null
  $ready = Wait-Change { param($s) @($s.rail.rows).Count -eq 5 -and $s.active_tab -eq 2 -and @($s.rail.rows | Where-Object label -eq 'Lodestone').Count } -What 'five rows, the third open, the labels read' -Animating

  $px = { param($v) ConvertTo-Px $v $w.Scale }
  $railW = & $px 80

  # 1. Closed
  $closed = Get-Shot $w -Name 'rail-labels'
  Save-Zoomed $closed (New-Rect 0 0 $railW ([Math]::Min($closed.Height, (& $px 520)))) 'rail-labels-rail'
  # The rail's ring starts a gutter in; the bar is a pixel past it, the pill's ring further in
  $bar = Measure-Accent $closed (& $px 5) (& $px 9)
  $ring = Measure-Accent $closed (& $px 15) (& $px 20)
  "closed: $bar accent pixels where the bar goes, $ring where the pill's ring goes"
  if ($bar -lt (& $px 20)) { Add-Failure "no red bar down the rail's edge beside the open tab ($bar pixels)" }
  if ($ring -lt (& $px 20)) { Add-Failure "no red ring round the open tab's pill ($ring pixels)" }

  # 1b. The idle timer's bars: only the lit ones are drawn (Lachlan's 12a). Above each pad, the
  # three bars' place (3 px bars 2 px apart, 2 px over the pad): no unlit stripe's #24242C in any
  # row; no bar over the working pad (row 0) or the shells' (3, 4); over the question two minutes
  # old (row 2), the bottom bar green and nothing above it; three over the one twenty (row 1)
  $unlit = New-ColourRange 30 42 30 42 39 50
  $green = New-ColourRange 30 110 180 255 100 170
  $rows = @($ready.rail.rows)
  $want = @{ 0 = 0; 1 = 3; 2 = 1; 3 = 0; 4 = 0 }
  foreach ($i in 0..4) {
    $pad = $rows[$i].pad
    $stack = New-Rect ([int]$pad.x) ([int]$pad.y - (& $px 15)) ([int]$pad.w) (& $px 13)
    $grey = [LbtermE2E.Px]::CountRange($closed, $stack, $unlit)
    $lit = @($rows[$i].bars)
    "row $($i): bars $($lit -join ',') in the state file; $grey unlit pixels over its pad"
    if ($grey -gt 0) { Add-Failure "row $i has $grey pixels of an unlit bar over its pad" }
    if ($want.ContainsKey($i) -and $lit.Count -ne $want[$i]) { Add-Failure "row $i's state file says $($lit.Count) bars, not $($want[$i])" }
  }
  # Twenty minutes on, the timer's third step of eight in the hour idle_full_minutes gives by
  # default: every bar lit, all green (a hook's `default`, the demo's idle, is no Claude to the
  # rail without the program's own word, so the stack is a question's)
  if ((@($rows[1].bars) -join ',') -ne 'green,green,green') { Add-Failure "the question twenty minutes old has bars $(@($rows[1].bars) -join ','), not three green" }
  $pad1 = $rows[1].pad
  $stack1 = New-Rect ([int]$pad1.x) ([int]$pad1.y - (& $px 15)) ([int]$pad1.w) (& $px 13)
  $g3 = [LbtermE2E.Px]::CountRange($closed, $stack1, $green)
  "row 1: $g3 green pixels over its pad"
  if ($g3 -lt (& $px 40)) { Add-Failure "the question twenty minutes old has $g3 green pixels over its pad, not its three bars" }
  if (@($rows[2].bars) -join ',' -ne 'green') { Add-Failure "the question two minutes old has bars $(@($rows[2].bars) -join ','), not the bottom one green" }
  $pad2 = $rows[2].pad
  $bottomBar = New-Rect ([int]$pad2.x) ([int]$pad2.y - (& $px 5)) ([int]$pad2.w) (& $px 3)
  $upperBars = New-Rect ([int]$pad2.x) ([int]$pad2.y - (& $px 15)) ([int]$pad2.w) (& $px 9)
  $g0 = [LbtermE2E.Px]::CountRange($closed, $bottomBar, $green)
  $g1 = [LbtermE2E.Px]::CountRange($closed, $upperBars, $green)
  "row 2: $g0 green pixels in its bottom bar, $g1 above it"
  if ($g0 -lt (& $px 10)) { Add-Failure "the question's bottom bar isn't green ($g0 pixels)" }
  if ($g1 -gt 0) { Add-Failure "the question has $g1 green pixels above its bottom bar, where nothing is lit" }
  if ($ready.rail.labels -ne 'shown') { Add-Failure "closed, the state file's labels say '$($ready.rail.labels)', not shown" }
  # Where each label is drawn, under its pad, short of the pill's sides: closed, its words are
  # there, bright
  $labelRoom = { param($pad) New-Rect ([int]($pad.x + $pad.w / 2) - (& $px 19)) ([int]($pad.y + $pad.h) + (& $px 3)) (& $px 38) (& $px 22) }
  foreach ($i in 0..4) {
    if (-not [LbtermE2E.Px]::BrightBox($closed, (& $labelRoom $rows[$i].pad), 90)) { Add-Failure "closed, row $i has no label under its pad" }
  }

  # 2. The worker writes a new label for the second tab
  [IO.File]::WriteAllText((Join-Path $summaries "$($sessions[1].sid).label.txt"), 'Retiring it')
  Wait-Change { param($s) @($s.rail.rows)[1].label -eq 'Retiring it' } -What 'the new label on the rail' -Animating | Out-Null
  $changed = Get-Shot $w -Name 'rail-labels-changed'
  $diff = [LbtermE2E.Px]::Differing($closed, $changed, (Get-Columns 0 ($railW - 1) -Of $closed))
  "after the label file changed: $diff pixels of the rail differ"
  if ($diff -lt 10) { Add-Failure "the rail didn't change when a label file did ($diff pixels)" }
  $closed.Dispose(); $changed.Dispose()

  # 3. Open: posted moves onto the rail, every frame, as panel-demo.ps1 does
  Set-PointerHold $w (& $px 40) (& $px 160)
  $opened = Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating
  $open = Get-Shot $w -Name 'rail-labels-open'
  Clear-PointerHold
  # 12a: the labels faded out where the open panel's titles say the same names: no bright pixel
  # left in any label's room but the open tab's (row 2), whose highlight's red ring runs round
  # its pill's foot; the state file saying so
  if ($opened.rail.labels -ne 'faded') { Add-Failure "open, the state file's labels say '$($opened.rail.labels)', not faded" }
  foreach ($i in 0, 1, 3, 4) {
    $box = [LbtermE2E.Px]::BrightBox($open, (& $labelRoom @($opened.rail.rows)[$i].pad), 90)
    if ($box) { Add-Failure "open, row $i's label still shows under its pad (bright from $($box -join ','))" }
  }
  # The ring runs along the open row, well past the rail
  $along = Measure-Accent $open (& $px 120) (& $px 200)
  "open: $along accent pixels along the open row"
  if ($along -lt (& $px 40)) { Add-Failure "no ring along the open tab's row in the open panel ($along pixels)" }
  $open.Dispose()
} -Passed 'rail-labels: labels under the pads from the summary worker, kept up to date and faded out while the panel is open; only lit idle bars drawn; the open tab ringed and barred'
