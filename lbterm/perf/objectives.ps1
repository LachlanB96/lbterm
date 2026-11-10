#Requires -Version 7
# Covers: lbterm-extras/src/objectives.rs, lbterm-extras/src/projects.rs, lbterm-extras/src/summaries.rs, wezterm-gui/src/termwindow/render/objectives.rs, wezterm-gui/src/termwindow/render/projects.rs, wezterm-gui/src/termwindow/render/inbox.rs, wezterm-gui/src/termwindow/render/borders.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm/motion/demo-tab.ps1
<#
The objectives panel down the right (lbterm/OBJECTIVES.md), end to end. A test window looking like
Lachlan's LBTerm (New-TestConfig) with the tab panel, lbterm_summaries pointed at a scratch folder
(<out>\summaries) and lbterm_objectives on gets four tabs: three with a Claude session whose
objectives file Terminal's objectives.ts would have written there, and one plain.

1. Closed: objectives.png. Each row's mark sits level with its pad on the rail, and says what became
   of what its session was started for: open (grey ring), drifted (yellow: the first ask open, the
   last step on something asked later), done (green dot), and none for the plain tab.
2. The first session's file is rewritten, its first objective waiting on Lachlan: the mark goes
   orange, with nothing done to the window (objectives-waiting.png).
3. The pointer posted onto the strip, level with the second tab: the list card opens with each tab's
   first objective and the tally, and the detail card beside it with the second session's whole list
   (objectives-open.png).
4. Onto the list's third row: the detail follows it.
5. The pointer leaves the window: both cards close.
6. The projects pad (lbterm_projects: two scratch projects, Alpha with a message in its InMail and
   the first two sessions, one in a folder of it, and Beta with the third, and Gamma, a repo in
   Beta's folder with a message of its own): its card opens with Alpha first, two idle sessions and
   its mail, then Beta, then Gamma under it with its mail and the blue repo mark
   (objectives-projects.png).
7. New on Beta's row: with no handler for the event in a test window's config, LBTerm opens a tab
   there itself, in front, and the card closes.
8. The InMail button's list: Gamma's message first, with the blue repo mark before who it's for,
   and Alpha's without one (objectives-inbox.png).

Its USERPROFILE is a scratch folder, so lbterm.lua's handlers write nothing into the real
~\.claude. Fails with exit 1.

  pwsh -File lbterm\perf\objectives.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'objectives' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $summaries, $home2 | Out-Null

function Objective([int]$Id, [string]$From, [string]$Status, [string]$Text, [string]$Evidence = '') {
  [ordered]@{ id = $Id; text = $Text; from = $From; ref = $null; at = 1; status = $Status; evidence = $Evidence; statusAt = 1 }
}
function Write-Objectives([string]$Sid, [int]$Current, $List) {
  $json = [ordered]@{ sessionId = $Sid; updatedAt = 1; current = $Current; objectives = @($List) } | ConvertTo-Json -Depth 5 -Compress
  [IO.File]::WriteAllText((Join-Path $summaries "$Sid.objectives.json"), $json)
}
$sessions = @(
  @{ sid = '11111111-1111-4111-8111-111111111111'; label = 'Defender CPU'; mark = 'open' }
  @{ sid = '22222222-2222-4222-8222-222222222222'; label = 'Tab rail'; mark = 'drifted' }
  @{ sid = '33333333-3333-4333-8333-333333333333'; label = 'Usage chart'; mark = 'done' }
)
foreach ($s in $sessions) { [IO.File]::WriteAllText((Join-Path $summaries "$($s.sid).label.txt"), $s.label) }
Write-Objectives $sessions[0].sid 1 @(Objective 1 first open 'Find why Defender uses 30% of the CPU')
Write-Objectives $sessions[1].sid 2 @(
  (Objective 1 first open 'Fix the tab rail'),
  (Objective 2 prompt done 'Rename the dashboard' 'Landed; check passed'))
Write-Objectives $sessions[2].sid 1 @(Objective 1 inmail done 'Build the usage chart' 'Archived 2026-10-24 09:20')

# Two projects for the projects card: the first two sessions in Alpha (one in a folder of it), the
# third in Beta, as their statuslines would say; a message waits in Alpha's InMail. Gamma is a repo
# in Beta's folder, with a message of its own
$alpha = Join-Path $T.Out 'projects\Alpha'
$beta = Join-Path $T.Out 'projects\Beta'
$gamma = Join-Path $beta 'code\Gamma'
New-Item -ItemType Directory -Force (Join-Path $alpha 'InMail'), (Join-Path $alpha 'seat'), (Join-Path $beta 'InMail'), (Join-Path $gamma 'InMail') | Out-Null
[IO.File]::WriteAllText((Join-Path $alpha 'InMail\2026-10-24-0900-from-Beta-a-message.md'),
  "---`nfrom: Beta`nto: Alpha`nsent: 2026-10-24T09:00`nsubject: A message`n---`n`nBody.`n")
[IO.File]::WriteAllText((Join-Path $gamma 'InMail\2026-10-24-0910-from-Alpha-for-the-repo.md'),
  "---`nfrom: Alpha`nto: Gamma`nsent: 2026-10-24T09:10`nsubject: For the repo`n---`n`nBody.`n")
[IO.File]::WriteAllText((Join-Path $summaries "$($sessions[0].sid).cwd.txt"), $alpha)
[IO.File]::WriteAllText((Join-Path $summaries "$($sessions[1].sid).cwd.txt"), (Join-Path $alpha 'seat'))
[IO.File]::WriteAllText((Join-Path $summaries "$($sessions[2].sid).cwd.txt"), $beta)
$lua = { param($p) "'$(ConvertTo-LuaPath $p)'" }
$projectsSet = "lbterm_projects={projects={{name='Alpha',dir=$(& $lua $alpha)},{name='Beta',dir=$(& $lua $beta)},{name='Gamma',dir=$(& $lua $gamma),parent='Beta'}}}"
$inboxSet = "lbterm_inbox={folders={$(& $lua (Join-Path $alpha 'InMail')),$(& $lua (Join-Path $beta 'InMail')),$(& $lua (Join-Path $gamma 'InMail'))}}"
# The repo mark's blue (lbterm_extras::projects::REPO)
$repoBlue = 0x4FB3FF

# Each mark's colour (lbterm_extras::objectives::Mark::colour), as a pixel near it
$colours = @{ open = 0xA8A8BC; drifted = 0xF2E86D; done = 0x3DDC84; waiting = 0xFF8A3D }
function Measure-Mark($shot, $mark, [string]$name) {
  $r = $mark.rect
  [LbtermE2E.Px]::CountNear($shot, (New-Rect ($r.x - 1) ($r.y - 1) ($r.w + 2) ($r.h + 2)), $colours[$name], 40)
}
function Demo([string]$Title, [string]$Session) {
  $a = @('-Title', $Title, '-State', 'idle', '-AgeMinutes', 3)
  if ($Session) { $a += @('-Session', $Session) }
  New-PwshProgram -File $demoTab -ArgumentList $a
}

Invoke-Test {
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo `
    -Set 'lbterm_compositor=true', 'lbterm_tab_panel={enabled=true,open_delay_ms=0}', "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'", 'lbterm_objectives={open_delay_ms=0}', $projectsSet, $inboxSet `
    -Program (Demo one $sessions[0].sid)
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail -and $s.objectives } -What 'the window, its rail and the objectives strip' | Out-Null
  foreach ($s in $sessions | Select-Object -Skip 1) {
    Invoke-Cli spawn --window-id 0 -Program (Demo two $s.sid) | Out-Null
  }
  Invoke-Cli spawn --window-id 0 -Program (Demo plain) | Out-Null
  $s = Wait-Change { param($s) @($s.objectives.marks).Count -eq 4 -and @($s.objectives.marks | Where-Object mark).Count -eq 3 } -What 'four rows, three marked' -Animating

  # 1. Closed: the marks, level with the pads, in their colours
  $closed = Get-Shot $w -Name 'objectives'
  $strip = $s.objectives.strip
  if ($strip.x + $strip.w -gt $s.size.width) { Add-Failure "the strip runs off the window ($($strip.x) + $($strip.w) of $($s.size.width))" }
  $panes = @($s.panes)
  if ($panes.Count -and ($panes[0].rect.x + $panes[0].rect.w) -gt $strip.x) { Add-Failure "a pane's card runs under the strip (its right edge $($panes[0].rect.x + $panes[0].rect.w), the strip at $($strip.x))" }
  for ($i = 0; $i -lt 4; $i++) {
    $mark = @($s.objectives.marks)[$i]
    $pad = @($s.rail.rows)[$i].pad
    $want = if ($i -lt 3) { $sessions[$i].mark } else { $null }
    if ($mark.mark -ne $want) { Add-Failure "row $i's mark is '$($mark.mark)', not '$want'" }
    $off = [Math]::Abs(($mark.rect.y + $mark.rect.h / 2) - ($pad.y + $pad.h / 2))
    if ($off -gt 1.5) { Add-Failure "row $i's mark is $off px off its pad's middle" }
    if ($want) {
      $n = Measure-Mark $closed $mark $want
      "row ${i}: $want, $n pixels in its colour, $off px off the pad's middle"
      if ($n -lt 8) { Add-Failure "row $i's $want mark isn't drawn in its colour ($n pixels)" }
    }
  }

  # 2. The first ask waits on Lachlan now: the mark goes orange
  Write-Objectives $sessions[0].sid 1 @(Objective 1 first waiting 'Find why Defender uses 30% of the CPU' 'Asked him where the 30% was seen')
  $s = Wait-Change { param($s) @($s.objectives.marks)[0].mark -eq 'waiting' } -What 'the first mark waiting' -Animating
  $waiting = Get-Shot $w -Name 'objectives-waiting'
  $n = Measure-Mark $waiting @($s.objectives.marks)[0] waiting
  "row 0 after its file changed: waiting, $n orange pixels"
  if ($n -lt 8) { Add-Failure "row 0's mark didn't go orange ($n pixels)" }
  $closed.Dispose(); $waiting.Dispose()

  # 3. Onto the strip level with the second tab: the list, and the second session's detail
  $m = @($s.objectives.marks)[1].rect
  Set-PointerHold $w ($strip.x + [int]($strip.w / 2)) ($m.y + [int]($m.h / 2))
  $s = Wait-Change { param($s) $s.objectives.list -and $s.objectives.detail } -What 'the list and the detail open' -Animating
  $rows = @($s.objectives.list.rows)
  $texts = $rows | ForEach-Object text
  "list: $($texts -join ' | ')"
  if ($rows[0].text -ne 'Find why Defender uses 30% of the CPU') { Add-Failure "the first row says '$($rows[0].text)'" }
  if ($rows[1].tally -ne '1/2') { Add-Failure "the second row's tally is '$($rows[1].tally)', not 1/2" }
  if ($rows[2].text -ne 'InMail: Build the usage chart') { Add-Failure "the third row says '$($rows[2].text)'" }
  if ($rows[3].text -ne 'plain' -and -not $rows[3].text) { Add-Failure "the plain tab's row has no name" }
  $lines = @($s.objectives.detail.lines)
  "detail of row $($s.objectives.detail.row): $($lines -join ' | ')"
  if ($s.objectives.detail.row -ne 1) { Add-Failure "the detail is for row $($s.objectives.detail.row), not 1" }
  foreach ($want in 'Tab rail', 'Open', 'Fix the tab rail', 'Done', 'now', 'Rename the dashboard', 'Landed; check passed') {
    if ($lines -notcontains $want) { Add-Failure "the detail card hasn't '$want'" }
  }
  if ($s.objectives.detail.rect.x + $s.objectives.detail.rect.w -gt $s.objectives.list.rect.x) { Add-Failure 'the detail card overlaps the list' }
  Get-Shot $w -Name 'objectives-open' | ForEach-Object Dispose

  # 4. Along the list to the third row: the detail follows
  $r = $rows[2].rect
  Set-PointerHold $w ($r.x + [int]($r.w / 2)) ($r.y + [int]($r.h / 2))
  $s = Wait-Change { param($s) $s.objectives.detail.row -eq 2 } -What "the third row's detail" -Animating
  if (@($s.objectives.detail.lines) -notcontains 'Archived 2026-10-24 09:20') { Add-Failure "the third row's detail hasn't its evidence" }
  if ($s.pointer.over -ne 'objectives.list') { Add-Failure "the pointer is over '$($s.pointer.over)', not the list" }

  # 5. Gone: both close
  Clear-PointerHold
  Send-Leave $w
  $s = Wait-Change { param($s) -not $s.objectives.list -and -not $s.objectives.detail } -What 'the cards closed' -Animating

  # 6. The projects card, from its pad
  $pad = $s.objectives.projects.pad
  if (-not $pad) { throw 'no projects pad on the strip' }
  Set-PointerHold $w ($pad.x + [int]($pad.w / 2)) ($pad.y + [int]($pad.h / 2))
  $s = Wait-Change { param($s) $s.objectives.projects.open -and @($s.objectives.projects.rows).Count -eq 3 } -What 'the projects card open' -Animating
  if ($s.objectives.list) { Add-Failure "the objectives list opened with the projects card" }
  $rows = @($s.objectives.projects.rows)
  "projects: $(($rows | ForEach-Object { "$($_.name)$(if ($_.parent) { " (in $($_.parent))" }): $($_.idle) idle, $($_.mail) mail, chips $(($_.chips | ForEach-Object action) -join ',')" }) -join ' | ')"
  if ($rows[0].name -ne 'Alpha' -or $rows[0].idle -ne 2 -or $rows[0].mail -ne 1) { Add-Failure "the first row isn't Alpha with two idle sessions and one message" }
  if ($rows[1].name -ne 'Beta' -or $rows[1].idle -ne 1 -or $rows[1].mail -ne 0) { Add-Failure "the second row isn't Beta with one idle session" }
  if ($rows[2].name -ne 'Gamma' -or $rows[2].parent -ne 'Beta' -or $rows[2].mail -ne 1) { Add-Failure "the third row isn't Gamma, Beta's repo, with its message" }
  if ($rows[0].parent -or $rows[1].parent) { Add-Failure 'a project of its own has a parent' }
  if ((@($rows[0].chips) | ForEach-Object action) -join ',' -ne 'new,inmail,resume') { Add-Failure "Alpha's buttons aren't New, InMail, Resume" }
  if ((@($rows[1].chips) | ForEach-Object action) -join ',' -ne 'new,resume') { Add-Failure "Beta's buttons aren't New, Resume (no mail, no InMail)" }
  if ((@($rows[2].chips) | ForEach-Object action) -join ',' -ne 'new,inmail,resume') { Add-Failure "Gamma's buttons aren't New, InMail, Resume" }
  $r = $rows[1].rect
  Set-PointerHold $w ($r.x + 20) ($r.y + [int]($r.h / 2))
  $s = Wait-Change { param($s) $s.objectives.projects.hovered -eq 1 } -What "Beta's row under the pointer" -Animating
  $shot = Get-Shot $w -Name 'objectives-projects'
  # The repo mark in its blue at the start of Gamma's row, and none on the projects' rows
  $mark = { param($row) [LbtermE2E.Px]::CountNear($shot, (New-Rect $row.rect.x $row.rect.y ([int]($row.rect.w / 4)) $row.rect.h), $repoBlue, 30) }
  $blue = & $mark $rows[2]
  "Gamma's repo mark: $blue blue pixels; Alpha's row $(& $mark $rows[0]), Beta's $(& $mark $rows[1])"
  # The mark is small: 6 pixels in its blue at 96 DPI (2026-10-28), none on the other rows
  if ($blue -lt 3) { Add-Failure "Gamma's row has no blue repo mark ($blue pixels)" }
  if ((& $mark $rows[0]) -ge 3 -or (& $mark $rows[1]) -ge 3) { Add-Failure "a project's own row has the repo mark" }
  $shot.Dispose()

  # 7. New on Beta's row: a tab opens there, in front, and the card closes
  $before = @($s.tabs).Count
  $chip = @(@($s.objectives.projects.rows)[1].chips | Where-Object action -eq 'new')[0]
  Clear-PointerHold
  Send-Click $w ($chip.rect.x + [int]($chip.rect.w / 2)) ($chip.rect.y + [int]($chip.rect.h / 2)) -Move
  $s = Wait-Change { param($s) @($s.tabs).Count -eq $before + 1 } -What 'the new tab' -Animating
  if ($s.active_tab -ne $before) { Add-Failure "the new tab isn't the open one (open: $($s.active_tab), tabs: $(@($s.tabs).Count))" }
  Send-Leave $w
  $s = Wait-Change { param($s) -not $s.objectives.projects.open } -What 'the projects card closed' -Animating

  # 8. The InMail button's list: Gamma's message, newest, with the repo mark before who it's for;
  #    Alpha's without it
  $button = @($s.strip.items | Where-Object name -eq 'inbox')[0]
  if (-not $button) { throw 'no InMail button on the strip' }
  $c = Get-RectCenter $button.rect
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'inbox' } -What 'the InMail list'
  $items = @($s.menu.items)
  "inbox list: $(($items | ForEach-Object label) -join ' / ')"
  if ($items.Count -ne 2 -or $items[0].label -ne 'Gamma: For the repo' -or $items[1].label -ne 'Alpha: A message') {
    Add-Failure "the InMail list isn't Gamma's then Alpha's message: $(($items | ForEach-Object label) -join ' / ')"
  } else {
    $shot = Get-Shot $w -Name 'objectives-inbox'
    $blueIn = { param($item) [LbtermE2E.Px]::CountNear($shot, (New-Rect $item.rect.x $item.rect.y $item.rect.w $item.rect.h), $repoBlue, 30) }
    $gammaBlue, $alphaBlue = (& $blueIn $items[0]), (& $blueIn $items[1])
    "InMail list's repo mark: Gamma's row $gammaBlue blue pixels, Alpha's $alphaBlue"
    if ($gammaBlue -lt 3) { Add-Failure "Gamma's message has no blue repo mark in the InMail list ($gammaBlue pixels)" }
    if ($alphaBlue -ge 3) { Add-Failure "Alpha's message, a project's, has the repo mark in the InMail list" }
    $shot.Dispose()
  }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'the InMail list closed' | Out-Null
} -Passed 'objectives: a mark per tab level with its pad, kept up to date; the list and the detail on resting, closed on leaving; the projects card with each project''s sessions and mail, a repo under its project with the blue mark, New opening a tab there in front'
