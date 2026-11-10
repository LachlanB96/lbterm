#Requires -Version 7
# Covers: lbterm-extras/src/links.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/state.rs, lbterm-panel/src/claude.rs, lbterm-panel/src/layout.rs, lbterm-panel/src/palette.rs, wezterm-gui/src/termwindow/tab_links.rs, wezterm-gui/src/termwindow/render/link_drag.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/state_files.rs, lbterm/motion/demo-tab.ps1
<#
Tab links (lbterm/LINKS.md), end to end. A test window looking like Lachlan's LBTerm
(New-TestConfig) with the tab panel, lbterm_summaries and lbterm_links pointed at scratch files,
and a handler of its own for lbterm-tab-link that writes each event to <out>\events.jsonl. The test
plays the links file's program (Terminal's links.ts): it writes the file and reads the events. Four
stand-in Claude tabs: Scroll strip, Tab Wave, Heat steps (waiting on Lachlan) and Usage chart.

1. The file links Scroll strip to Tab Wave and Heat steps to Usage chart: both waiting rows are
   purple ("linked"), Heat steps with an orange ring (it asks him something too), Scroll strip
   saying what it waits for; a purple line down the rail's edge between each pair (tab-links.png).
2. The first link goes stuck: its row and line go red, its words say why.
3. Usage chart says it waits for Scroll strip's objective: a suggested line, dashed, in a lane of
   its own; a click on its dashes fires confirm, and the line is drawn as a link at once.
4. A right click on a row that doesn't move opens the tab menu.
5. A right-drag of Usage chart onto Tab Wave: while it drags, a ring round Tab Wave and the cards
   (what it waits for, Tab Wave's objectives); let go, toggle fires and the line shows at once.
6. Tab Wave says it waits for Scroll strip's objective 1: dragged over Scroll strip, both cards'
   matched blocks light green (tab-links-match.png); let go off the rows, nothing fires.
7. The tab menu on Scroll strip, whose link is stuck: Stop waiting on Tab Wave, "LBTerm to land the
   links is now complete by Tab Wave" (Lachlan's words, his E2), and Wait on Tab Wave again, which
   fires again.
8. The menu again, waiting once more: the row in his words fires release, and the row says it's
   telling Scroll strip at once.
9. The pointer resting on that line, the panel closed: its card opens beside the rail without the
   panel opening (his E3), saying who waits on whom, where it stands in green and what for
   (tab-links-line.png); off the line, it goes.

Its USERPROFILE is a scratch folder. Fails with exit 1.

  pwsh -File lbterm\perf\tab-links.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'tab-links' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
$linksFile = Join-Path $T.Out 'tab-links.json'
$events = Join-Path $T.Out 'events.jsonl'
New-Item -ItemType Directory -Force $summaries, $home2 | Out-Null
Remove-Item -LiteralPath $events -ErrorAction SilentlyContinue

$A = 'aaaaaaaa-1111-4111-8111-111111111111'
$B = 'bbbbbbbb-2222-4222-8222-222222222222'
$C = 'cccccccc-3333-4333-8333-333333333333'
$D = 'dddddddd-4444-4444-8444-444444444444'
$tabs = @(
  @{ sid = $A; title = 'Scroll strip'; state = 'idle' }
  @{ sid = $B; title = 'Tab Wave'; state = 'idle' }
  @{ sid = $C; title = 'Heat steps'; state = 'waiting' }
  @{ sid = $D; title = 'Usage chart'; state = 'idle' }
)
function Objective([int]$Id, [string]$Status, [string]$Text) {
  [ordered]@{ id = $Id; text = $Text; from = 'prompt'; at = 1; status = $Status; evidence = ''; statusAt = 1 }
}
function Write-Objectives([string]$Sid, [int]$Current, $List) {
  $json = [ordered]@{ sessionId = $Sid; updatedAt = 1; current = $Current; objectives = @($List) } | ConvertTo-Json -Depth 5 -Compress
  [IO.File]::WriteAllText((Join-Path $summaries "$Sid.objectives.json"), $json)
}
Write-Objectives $A 1 @(Objective 1 open "Draw the rail's lines")
Write-Objectives $B 2 @((Objective 1 done 'Build the tab wave'), (Objective 2 open 'Land feat/tab-links in LBTerm'))
Write-Objectives $C 1 @(Objective 1 waiting 'Lower the heat steps')
Write-Objectives $D 1 @(Objective 1 open 'Build the usage chart')

function Link([string]$Waiter, [string]$On, [string]$State = 'waiting', [string]$What = '', [string]$Note = '') {
  [ordered]@{ id = "$Waiter-$On"; waiter = $Waiter; on = $On; at = 1; by = 'test'; what = $What; state = $State; stateAt = 1; note = $Note }
}
function Write-Links($Links, $Waits = @()) {
  $json = [ordered]@{ updatedAt = 1; links = @($Links); waits = @($Waits) } | ConvertTo-Json -Depth 5 -Compress
  Write-FileWhole -Path $linksFile -Text $json
}
function Read-Events { @(if (Test-Path $events) { Get-Content -LiteralPath $events | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } }) }
function Wait-Event([int]$Count, [string]$What) {
  $deadline = [DateTime]::Now.AddSeconds(10)
  while ([DateTime]::Now -lt $deadline) {
    $all = Read-Events
    if ($all.Count -ge $Count) { return $all[$Count - 1] }
    Start-Sleep -Milliseconds 100
  }
  Add-Failure "no lbterm-tab-link event for $What"
  $null
}
function Demo($tab) { New-PwshProgram -File $demoTab -ArgumentList @('-Title', $tab.title, '-State', $tab.state, '-Session', $tab.sid) }
# The colours (lbterm_panel::palette): the link's purple, red, green, and the orange ring
$purple = 0x9F6EEB; $red = 0xFF5449; $green = 0x3DDC84; $orange = 0xFF8A3D
function Measure-Lane($shot, $s, $line) {
  $from = @($s.rail.rows)[$line.waiter].pad; $to = @($s.rail.rows)[$line.on].pad
  $top = [Math]::Min($from.y, $to.y) + 4; $bottom = [Math]::Max($from.y, $to.y) + $from.h - 4
  [LbtermE2E.Px]::CountNear($shot, (New-Rect ([int]$line.x - 2) $top 5 ($bottom - $top)), $(switch ($line.look) { 'error' { $red } 'done' { $green } default { $purple } }), 40)
}

Write-Links @((Link $A $B -What 'LBTerm to land the links'), (Link $C $D))
$handler = @"
wezterm.on('lbterm-tab-link', function(window, pane, l)
  local f = io.open([[$events]], 'a')
  f:write(wezterm.json_encode({ action = l.action, waiter = l.waiter, on = l.on, objective = l.objective }) .. '\n')
  f:close()
  return false
end)
"@

Invoke-Test {
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo -Lua $handler `
    -Set 'lbterm_compositor=true', 'lbterm_tab_panel={enabled=true,open_delay_ms=0}', "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'", "lbterm_links={file='$(ConvertTo-LuaPath $linksFile)'}" `
    -Program (Demo $tabs[0])
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $panes = @(0)
  foreach ($tab in $tabs | Select-Object -Skip 1) { $panes += [int](Invoke-Cli spawn --window-id 0 -Program (Demo $tab)) }
  # Claude Code's ✳ before the title, as it titles a session that has handed back
  for ($i = 0; $i -lt 4; $i++) { Invoke-Cli set-tab-title --pane-id $panes[$i] "$([char]0x2733) $($tabs[$i].title)" | Out-Null }
  Invoke-Cli activate-tab --pane-id 0 --tab-index 0 | Out-Null
  $s = Wait-Change { param($s) @($s.rail.rows).Count -eq 4 -and @($s.rail.links).Count -eq 2 -and @($s.rail.rows)[0].state -eq 'linked' } -What 'four rows, two links' -Animating

  # 1. The waiting rows purple, Heat steps ringed orange, a line down the edge between each pair
  $rows = @($s.rail.rows)
  "rows: $(($rows | ForEach-Object { "$($_.label)=$($_.state)/$($_.link)$(if ($_.asks) { '+asks' })" }) -join ', ')"
  if ($rows[0].status -ne 'Waiting on Tab Wave: LBTerm to land the links') { Add-Failure "Scroll strip's row says '$($rows[0].status)'" }
  if ($rows[2].state -ne 'linked' -or -not $rows[2].asks) { Add-Failure "Heat steps isn't purple with the orange ring ($($rows[2].state), asks $($rows[2].asks))" }
  if ($rows[1].state -eq 'linked' -or $rows[3].state -eq 'linked') { Add-Failure 'a tab waited on went purple' }
  $lines = @($s.rail.links)
  "lines: $(($lines | ForEach-Object { "$($_.waiter)->$($_.on) lane $($_.lane) $($_.look)" }) -join ', ')"
  $shot = Get-Shot $w -Name 'tab-links'
  foreach ($line in $lines) {
    $n = Measure-Lane $shot $s $line
    "line $($line.waiter)->$($line.on): $n pixels of its colour down lane $($line.lane)"
    if ($n -lt 12) { Add-Failure "the line from row $($line.waiter) to $($line.on) isn't drawn ($n pixels)" }
  }
  $pad = $rows[2].pad
  $ring = [LbtermE2E.Px]::CountNear($shot, (New-Rect ($pad.x - 4) ($pad.y - 4) ($pad.w + 8) ($pad.h + 8)), $orange, 40)
  "Heat steps' pad: $ring orange pixels round it"
  if ($ring -lt 12) { Add-Failure "Heat steps' orange ring isn't drawn ($ring pixels)" }
  $shot.Dispose()

  # 2. Stuck: red, with why
  Write-Links @((Link $A $B -State stuck -What 'LBTerm to land the links' -Note 'The rail still has no lines'), (Link $C $D))
  $s = Wait-Change { param($s) @($s.rail.rows)[0].link -eq 'stuck' } -What 'the first link stuck' -Animating
  $row = @($s.rail.rows)[0]
  if ($row.state -ne 'error' -or $row.status -ne 'Still blocked: The rail still has no lines') { Add-Failure "a stuck link's row is $($row.state), '$($row.status)'" }
  $line = @($s.rail.links) | Where-Object { $_.waiter -eq 0 }
  if ($line.look -ne 'error') { Add-Failure "a stuck link's line is $($line.look)" }
  $shot = Get-Shot $w -Name 'tab-links-stuck'
  $n = Measure-Lane $shot $s $line
  "stuck: $n red pixels down its lane"
  if ($n -lt 12) { Add-Failure "the stuck line isn't red ($n pixels)" }
  $shot.Dispose()

  # 3. A suggested link, dashed in a lane of its own; a click on its dashes makes it
  Write-Links @((Link $A $B -State stuck -What 'LBTerm to land the links' -Note 'The rail still has no lines'), (Link $C $D)) @([ordered]@{ session = $D; what = 'the rail to draw the lines'; at = 1; on = $A; objective = 1; matchedAt = 2 })
  $s = Wait-Change { param($s) @($s.rail.links | Where-Object look -eq 'suggested').Count -eq 1 } -What 'a suggested line' -Animating
  $suggested = @($s.rail.links | Where-Object look -eq 'suggested')[0]
  "suggested: $($suggested.waiter)->$($suggested.on) in lane $($suggested.lane)"
  if ($suggested.waiter -ne 3 -or $suggested.on -ne 0 -or $suggested.lane -eq 0) { Add-Failure "the suggested line is $($suggested.waiter)->$($suggested.on) in lane $($suggested.lane)" }
  $through = @($s.rail.rows)[1].rect
  Send-Click $w ([int]$suggested.x) ([int]($through.y + $through.h / 2)) -Move
  $e = Wait-Event 1 'the suggested line clicked'
  "clicked: $($e | ConvertTo-Json -Compress)"
  if ($e -and ($e.action -ne 'confirm' -or $e.waiter -ne $D -or $e.on -ne $A -or $e.objective -ne 1)) { Add-Failure "the click fired $($e | ConvertTo-Json -Compress)" }
  $s = Wait-Change { param($s) @($s.rail.links | Where-Object { $_.waiter -eq 3 -and $_.on -eq 0 -and $_.look -eq 'waiting' }).Count -eq 1 } -What 'the made link drawn at once' -Animating
  # As links.ts would have it: made, the suggestion with it
  Write-Links @((Link $A $B -State stuck -What 'LBTerm to land the links' -Note 'The rail still has no lines'), (Link $C $D), (Link $D $A))
  $s = Wait-State { param($s) @($s.rail.links).Count -eq 3 -and @($s.rail.links | Where-Object look -eq 'suggested').Count -eq 0 } -What 'the made link in the file'

  # 4. A right click that doesn't move: the tab menu
  $p1 = Get-RectCenter @($s.rail.rows)[1].pad
  Send-Click $w $p1[0] $p1[1] -Right -Move
  $s = Wait-Change { param($s) $s.menu -and $s.menu.kind -eq 'tab' } -What 'the tab menu'
  if ($s.menu.tab -ne @($s.rail.rows)[1].tab) { Add-Failure "the menu is for tab $($s.menu.tab), not Tab Wave's" }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'the menu shut' | Out-Null

  # 5. Usage chart right-dragged onto Tab Wave: the ring and the cards while it drags, toggle let go
  $s = Wait-State { param($s) $s.rail } -What 'the rail'
  $from = Get-RectCenter @($s.rail.rows)[3].pad
  $to = Get-RectCenter @($s.rail.rows)[1].pad
  Send-Mouse $w Move $from[0] $from[1]
  Send-Mouse $w RDown $from[0] $from[1] -Keys R
  foreach ($k in 1..10) {
    Send-Mouse $w Move ([int]($from[0] + ($to[0] - $from[0]) * $k / 10)) ([int]($from[1] + ($to[1] - $from[1]) * $k / 10)) -Keys R
    Start-Sleep -Milliseconds 30
  }
  $s = Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.over -eq 1 } -What 'the drag over Tab Wave' -Animating
  $drag = $s.rail.link_drag
  "dragging row $($drag.row) over $($drag.over), matched $($drag.matched), cards $([bool]$drag.waits) $([bool]$drag.aims)"
  if ($drag.row -ne 3 -or -not $drag.waits -or -not $drag.aims) { Add-Failure "the drag shows row $($drag.row), cards $([bool]$drag.waits) $([bool]$drag.aims)" }
  if ($drag.aims -and $drag.aims.x -lt $s.rail.rect.x + $s.rail.rect.w) { Add-Failure 'the objectives card covers the rail' }
  Get-Shot $w -Name 'tab-links-drag' | ForEach-Object Dispose
  Send-Mouse $w RUp $to[0] $to[1]
  $e = Wait-Event 2 'the drop'
  "dropped: $($e | ConvertTo-Json -Compress)"
  if ($e -and ($e.action -ne 'toggle' -or $e.waiter -ne $D -or $e.on -ne $B)) { Add-Failure "the drop fired $($e | ConvertTo-Json -Compress)" }
  Wait-Change { param($s) -not $s.rail.link_drag -and @($s.rail.links | Where-Object { $_.waiter -eq 3 -and $_.on -eq 1 }).Count -eq 1 } -What 'the dropped link drawn at once' -Animating | Out-Null

  # 6. Tab Wave's wait matches Scroll strip's objective 1: dragged over it, both blocks green
  Write-Links @((Link $A $B -State stuck -What 'LBTerm to land the links' -Note 'The rail still has no lines'), (Link $C $D), (Link $D $A), (Link $D $B)) @([ordered]@{ session = $B; what = "the rail's lines drawn"; at = 1; on = $A; objective = 1; matchedAt = 2 })
  $s = Wait-Change { param($s) @($s.rail.links | Where-Object look -eq 'suggested').Count -eq 1 } -What "Tab Wave's suggestion" -Animating
  $from = Get-RectCenter @($s.rail.rows)[1].pad
  $to = Get-RectCenter @($s.rail.rows)[0].pad
  Send-Mouse $w Move $from[0] $from[1]
  Send-Mouse $w RDown $from[0] $from[1] -Keys R
  foreach ($k in 1..8) {
    Send-Mouse $w Move $from[0] ([int]($from[1] + ($to[1] - $from[1]) * $k / 8)) -Keys R
    Start-Sleep -Milliseconds 30
  }
  $s = Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.over -eq 0 } -What 'the drag over Scroll strip' -Animating
  $drag = $s.rail.link_drag
  "matched $($drag.matched): waits $($drag.waits_match | ConvertTo-Json -Compress), aims $($drag.aims_match | ConvertTo-Json -Compress)"
  if (-not $drag.matched -or -not $drag.waits_match -or -not $drag.aims_match) { Add-Failure 'the matching tab lit nothing' }
  else {
    $shot = Get-Shot $w -Name 'tab-links-match'
    foreach ($m in $drag.waits_match, $drag.aims_match) {
      $n = [LbtermE2E.Px]::CountNear($shot, (ConvertTo-Rect $m), $green, 50)
      "a matched block: $n green pixels"
      if ($n -lt 20) { Add-Failure "a matched block isn't green ($n pixels)" }
    }
    $shot.Dispose()
  }
  # Let go off the rows, over the panes: nothing
  $pane = @($s.panes)[0].rect
  Send-Mouse $w Move ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h / 2)) -Keys R
  Send-Mouse $w RUp ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h / 2))
  Wait-Change { param($s) -not $s.rail.link_drag } -What 'the drag put down' | Out-Null
  Start-Sleep -Milliseconds 300
  if ((Read-Events).Count -ne 2) { Add-Failure "letting go off the rows fired $((Read-Events).Count - 2) events" }

  # 7. The menu on Scroll strip, whose link is stuck: Stop waiting, and Wait on it again
  $s = Wait-State { param($s) $s.rail } -What 'the rail'
  $p0 = Get-RectCenter @($s.rail.rows)[0].pad
  Send-Click $w $p0[0] $p0[1] -Right -Move
  $s = Wait-Change { param($s) $s.menu -and $s.menu.kind -eq 'tab' } -What "Scroll strip's menu"
  $labels = @($s.menu.items | ForEach-Object label)
  "menu: $($labels -join ' | ')"
  foreach ($want in 'Stop waiting on Tab Wave', 'LBTerm to land the links is now complete by Tab Wave', 'Wait on Tab Wave again') {
    if ($labels -notcontains $want) { Add-Failure "the menu hasn't '$want'" }
  }
  $again = @($s.menu.items | Where-Object label -eq 'Wait on Tab Wave again')[0]
  if ($again) {
    $c = Get-RectCenter $again.rect
    Send-Click $w $c[0] $c[1] -Move
    $e = Wait-Event 3 'Wait on Tab Wave again'
    "picked: $($e | ConvertTo-Json -Compress)"
    if ($e -and ($e.action -ne 'again' -or $e.waiter -ne $A -or $e.on -ne $B)) { Add-Failure "Wait again fired $($e | ConvertTo-Json -Compress)" }
  }
  Wait-Change { param($s) -not $s.menu } -What 'the menu shut' | Out-Null
  # As links.ts would have it: waiting again
  $others = @((Link $C $D), (Link $D $A), (Link $D $B))
  Write-Links (@((Link $A $B -What 'LBTerm to land the links')) + $others)
  $s = Wait-Change { param($s) @($s.rail.rows)[0].link -eq 'waiting' } -What 'Scroll strip waiting again' -Animating

  # 8. Marked complete by hand, in his words: release, and the row telling it at once
  Send-Click $w $p0[0] $p0[1] -Right -Move
  $s = Wait-Change { param($s) $s.menu -and $s.menu.kind -eq 'tab' } -What "Scroll strip's menu again"
  $release = @($s.menu.items | Where-Object label -eq 'LBTerm to land the links is now complete by Tab Wave')[0]
  if (-not $release) { Add-Failure "the waiting row's menu hasn't the complete row: $(@($s.menu.items | ForEach-Object label) -join ' | ')" }
  else {
    $c = Get-RectCenter $release.rect
    Send-Click $w $c[0] $c[1] -Move
    $e = Wait-Event 4 'the complete row'
    "picked: $($e | ConvertTo-Json -Compress)"
    if ($e -and ($e.action -ne 'release' -or $e.waiter -ne $A -or $e.on -ne $B)) { Add-Failure "the complete row fired $($e | ConvertTo-Json -Compress)" }
    $s = Wait-Change { param($s) @($s.rail.rows)[0].link -eq 'telling' } -What 'Scroll strip telling at once' -Animating
    "row: $(@($s.rail.rows)[0].status)"
  }
  # As links.ts would have it: sending, marked complete by him
  Write-Links (@((Link $A $B -State sending -What 'LBTerm to land the links' -Note 'Lachlan marked it complete')) + $others)
  $s = Wait-State { param($s) @($s.rail.links | Where-Object { $_.waiter -eq 0 -and $_.on -eq 1 -and $_.look -eq 'done' }).Count -eq 1 } -What 'the line green in the file'

  # 9. Resting on that line, the panel closed: its card beside the rail, the panel staying shut
  $pane = @($s.panes)[0].rect
  Send-Mouse $w Move ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h / 2))
  $s = Wait-Change { param($s) -not $s.rail.open } -What 'the panel shut' -Animating
  $links = @($s.rail.links)
  $k = 0..($links.Count - 1) | Where-Object { $links[$_].waiter -eq 0 -and $links[$_].on -eq 1 } | Select-Object -First 1
  $line = $links[$k]
  $y = [int](($line.top + $line.bottom) / 2)
  Send-Mouse $w Move ([int]$line.x) $y
  $s = Wait-State { param($s) $s.rail.line_card } -What "the line's card"
  $card = $s.rail.line_card
  "line card: line $($card.line), $($card.lines -join ' / ')"
  if ($card.line -ne $k) { Add-Failure "the card is line $($card.line)'s, not $k's" }
  if ($s.rail.open) { Add-Failure 'resting on a line opened the panel' }
  # Its words wrap to the card: read them joined
  $words = @($card.lines) -join ' '
  foreach ($want in 'Scroll strip waits on Tab Wave', 'Done, telling it to carry on: Lachlan marked it complete', 'waits for LBTerm to land the links') {
    if (-not $words.Contains($want)) { Add-Failure "the line's card hasn't '$want'" }
  }
  if ($card.rect.x -lt $s.rail.rect.x + $s.rail.rect.w) { Add-Failure 'the line card covers the rail' }
  $shot = Get-Shot $w -Name 'tab-links-line'
  $n = [LbtermE2E.Px]::CountNear($shot, (ConvertTo-Rect $card.rect), $green, 50)
  "line card: $n green pixels (where it stands)"
  if ($n -lt 10) { Add-Failure "the line card's standing isn't green ($n pixels)" }
  $shot.Dispose()
  Send-Mouse $w Move ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h / 2))
  Wait-State { param($s) -not $s.rail.line_card } -What "the line's card gone" | Out-Null
} -Passed 'tab links: purple, red and ringed rows, lines in lanes, a suggestion made by a click, the menu on a still click, a right-drag firing toggle with its cards, a match lit green, the menu''s link rows, a task marked complete in his words, a line''s card'
