#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/agenda.rs, lbterm-extras/src/agenda.rs, wezterm-gui/src/state_files.rs, wezterm-gui/src/termwindow/render/inbox.rs, wezterm-gui/src/termwindow/render/fancy_tab_bar.rs, wezterm-gui/src/termwindow/render/strip_swap.rs, wezterm-gui/src/termwindow/render/menu_fade.rs
<#
The agenda's button (`lbterm_agenda`; wezterm-gui/src/termwindow/render/agenda.rs,
lbterm-extras/src/agenda.rs; lbterm/AGENDA.md) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig), with the InMail button beside it (one message in a folder of the
test's own). The agenda's file is a stand-in, in a folder of its own, rewritten whole the way its
program writes it; the `lbterm-agenda-press` handler writes down each press it's given.

1. The button sits after the InMail button over the rail, before the strip's words, its counts the
   file's: the urgent in the waiting orange, the failing in red (button.png).
2. A click drops the list from the button: Urgent, Failing (a group of two folded), Coming up, Yours,
   then the agents' to-dos folded away (list.png).
3. A click on the group opens it; another on the agents' line shows their to-dos.
4. A click on a row opens it: its detail and its buttons under it (opened.png).
5. Drop, which asks first: the first press says so on the button and runs nothing; the second runs
   it, and the handler hears do, the row and the button.
6. The file rewritten without that row and with one fewer failing: the button's count follows and the
   row goes from the list.
7. Snooze 1 d on another row: the handler hears snooze and the days. Open on it: the handler hears
   open and its page, and the list closes.
8. The keys: the list opened again, Down to the first row and Enter opens it; Esc closes the list.
Every press of a line (3, 4, 5's and 7's rows, 8's Enter) and of a button (5's two, 7's Snooze and
Open) flashes it white, as every other menu's pick does (Lachlan's 6 of 2026-11-08): menu.list.flash
in the state file's animations after it. The window has motion on for it (his config leaves it to
Windows' setting, which a test can't rely on).
9. The file removed: the button goes.

Where everything is and when it changed come from the window's state file (lbterm/STATE.md); the
pictures say how it looks. Clicks and keys are posted messages, never real input.

  pwsh -File lbterm\perf\agenda.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'agenda' -Bin $Bin
$stateDir = Join-Path $T.Out 'state\agenda'
$file = Join-Path $stateDir 'awake.json'
$inmail = Join-Path $T.Out 'projects\One\InMail'
$report = Join-Path $T.Out 'report.txt'
Remove-Item -Recurse -Force (Join-Path $T.Out 'state'), (Join-Path $T.Out 'projects'), $report -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stateDir, $inmail | Out-Null
[IO.File]::WriteAllText((Join-Path $inmail '2026-11-01-0900-from-Two-hello.md'), "---`nfrom: Two`nto: One`nsent: 2026-11-01T09:00`nsubject: `"Hello`"`n---`n`nThe body.`n")

function Row([string]$Id, [string]$Kind, [string]$Who, [string]$Level, [string]$Title, [string]$Project, [hashtable]$More = @{}) {
  $r = [ordered]@{ source = 'Index'; id = $Id; kind = $Kind; who = $Who; level = $Level; title = $Title; project = $Project }
  foreach ($k in $More.Keys) { $r[$k] = $More[$k] }
  $r
}
$rows = @(
  (Row 'u1' 'todo' 'you' 'urgent' 'Pay the car registration' 'Ledger' @{ whenText = 'today'; actions = @(@{ label = 'Tick' }) })
  (Row 'f1' 'failing' 'agent' 'normal' 'The check fails' 'Kiln Lab' @{ detail = 'Three tests fail on main'; open = '/d/projects'; actions = @(@{ label = 'Recheck' }) })
  (Row 'f2a' 'failing' 'agent' 'normal' 'A disk filled up' 'Harbor' @{ group = 'Unresolved incidents' })
  (Row 'f2b' 'failing' 'agent' 'normal' 'A driver crashed' 'Harbor' @{ group = 'Unresolved incidents' })
  (Row 'c1' 'upcoming' 'you' 'normal' 'An interview' 'Atlas' @{ whenText = 'Tue 12:00' })
  (Row 'y1' 'todo' 'you' 'normal' 'Reply to the landlord' 'Postbox' @{ actions = @(@{ label = 'Done' }, @{ label = 'Drop'; confirm = 'Drop this for good?' }) })
  (Row 'a1' 'todo' 'agent' 'normal' 'Read the InMail' 'Lantern' @{ group = 'InMail' })
)
# Written whole and renamed into place, as its program writes it
function Write-Agenda($Rows, [int]$Urgent, [int]$Failing) {
  $text = [ordered]@{ changed = [DateTimeOffset]::Now.ToUnixTimeMilliseconds(); counts = [ordered]@{ urgent = $Urgent; failing = $Failing }; rows = @($Rows) } | ConvertTo-Json -Depth 6
  Write-FileWhole -Path $file -Text $text
}
function Reported { @(Get-Content $report -ErrorAction SilentlyContinue) }
function Get-Item-Named($s, [string]$Name) { @($s.strip.items | Where-Object name -eq $Name)[0] }
function Labels($s) { @($s.menu.items | ForEach-Object label) }
function Item($s, [string]$Label) { @($s.menu.items | Where-Object label -eq $Label)[0] }
function Click-Item($w, $s, [string]$Label) {
  $i = Item $s $Label
  if (-not $i -or -not $i.rect) { throw "no '$Label' shown in the list: $((Labels $s) -join ' / ')" }
  $c = Get-RectCenter $i.rect
  Send-Click $w $c[0] $c[1]
}
function Count-Colour($shot, $rect, $range) { [LbtermE2E.Px]::CountRange($shot, (New-Rect $rect.x $rect.y $rect.w $rect.h), $range) }
$orange = New-ColourRange 200 255 100 175 20 110
$red = New-ColourRange 200 255 50 110 40 110

Write-Agenda $rows 1 3

Invoke-Test {
  $w = Open-TestWindow -Lua @"
rawset(config, 'lbterm_inbox', { folders = { [[$inmail]] } })
rawset(config, 'lbterm_agenda', { file = [[$file]] })
wezterm.on('lbterm-agenda-press', function(window, pane, p)
  local f = io.open('$(ConvertTo-LuaPath $report)', 'a')
  if f then
    f:write(table.concat({ p.what, p.source, p.id, p.project, tostring(p.button or p.days or p.open or ''), tostring(p.file == [[$file]]) }, '|') .. '\n')
    f:close()
  end
  return false
end)
config.initial_cols = 120
config.initial_rows = 34
-- On, whatever Windows' Animation effects say: the presses' flashes are judged (with motion off
-- there are none)
config.lbterm_motion = config.lbterm_motion or {}
config.lbterm_motion.enabled = 'on'
"@
  # Each press's flash (Lachlan's 6 of 2026-11-08): every frame read while waiting on what the press
  # did, and on the window settling after it
  $seen = [System.Collections.Generic.HashSet[string]]::new()
  $collect = { param($s) foreach ($a in @($s.animations)) { [void]$seen.Add($a.name) } }
  $flashed = { param([string]$What)
    if ($seen.Contains('menu.list.flash')) { "   $What flashed (menu.list.flash)" }
    else { Add-Failure "$What didn't flash white: no menu.list.flash in [$(@($seen) -join ', ')]" }
    $seen.Clear()
  }
  # 1. The button after the InMail one, its counts the file's
  $s = Wait-Change { param($s) $s.strip.agenda.urgent -eq 1 -and (Get-Item-Named $s 'agenda') -and (Get-Item-Named $s 'inbox') } -What 'the agenda button beside the InMail one'
  if ($s.strip.agenda.failing -ne 3) { Add-Failure "the button says $($s.strip.agenda.failing) failing, not the file's 3" }
  $b = (Get-Item-Named $s 'agenda').rect
  $ib = (Get-Item-Named $s 'inbox').rect
  $rail = $s.rail.rect
  "agenda button x $($b.x)..$($b.x + $b.w), InMail's $($ib.x)..$($ib.x + $ib.w), rail $($rail.x)..$($rail.x + $rail.w)"
  if ($b.x -lt $ib.x + $ib.w) { Add-Failure "the agenda button isn't after the InMail button: it starts at $($b.x), InMail's ends at $($ib.x + $ib.w)" }
  if ($b.y + $b.h -gt $rail.y) { Add-Failure "the button runs into the rail: its bottom $($b.y + $b.h), the rail's top $($rail.y)" }
  $words = Get-Item-Named $s 'words'
  if ($words -and $b.x + $b.w -gt $words.rect.x) { Add-Failure "the button runs into the strip's words at x $($words.rect.x)" }
  $shot = Get-Shot $w -Name 'button'
  $o = Count-Colour $shot $b $orange
  $r = Count-Colour $shot $b $red
  if ($o -lt 6) { Add-Failure "no urgent count in the waiting orange on the button ($o pixels; button.png)" }
  if ($r -lt 6) { Add-Failure "no failing count in red on the button ($r pixels; button.png)" }
  "counts: $o orange pixels, $r red"
  $shot.Dispose()

  # 2. The list, from the button
  $bc = Get-RectCenter $b
  Send-Click $w $bc[0] $bc[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'agenda' } -What 'the list'
  $want = @('Urgent (1)', 'Ledger: Pay the car registration', 'Failing (3)', 'Kiln Lab: The check fails', 'Unresolved incidents · Harbor (2)',
    'Coming up (1)', 'Atlas: An interview', 'Yours to do (1)', 'Postbox: Reply to the landlord', "Agents' to-dos (1)")
  "list: $((Labels $s) -join ' / ')"
  if (((Labels $s) -join '|') -ne ($want -join '|')) { Add-Failure "the list isn't urgent, failing, coming up, yours and the agents' line: $((Labels $s) -join ' / ')" }
  $card = $s.menu.rect
  if ([Math]::Abs($card.x - $b.x) -gt 12 -or $card.y -lt $b.y + $b.h) {
    Add-Failure "the list doesn't hang under its button: its card at x $($card.x), y $($card.y); the button x $($b.x), bottom $($b.y + $b.h)"
  }
  Get-Shot $w -Name 'list' | ForEach-Object Dispose

  # 3. The group opened, the agents' to-dos shown
  $s = Wait-Settled -What 'the list, settled'
  $seen.Clear()
  Click-Item $w $s 'Unresolved incidents · Harbor (2)'
  $s = Wait-Change { param($s) (Labels $s) -contains 'Harbor: A driver crashed' } -What 'the group opened' -OnState $collect
  if (-not (Item $s 'Unresolved incidents · Harbor (2)').checked) { Add-Failure 'the opened group isn''t checked' }
  & $flashed 'the group'
  Click-Item $w $s "Agents' to-dos (1)"
  $s = Wait-Change { param($s) (Labels $s) -contains 'Lantern: Read the InMail' } -What "the agents' to-dos shown" -OnState $collect
  & $flashed "the agents' line"
  'group opened, agents shown'

  # 4. A row opened: its detail and buttons
  Click-Item $w $s 'Kiln Lab: The check fails'
  $s = Wait-Change { param($s) (Labels $s) -contains 'button: Recheck' } -What 'the row opened' -OnState $collect
  & $flashed 'the row opening'
  $l = Labels $s
  $at = [Array]::IndexOf($l, 'Kiln Lab: The check fails')
  $under = $l[($at + 1)..($at + 6)] -join ' / '
  "opened: $under"
  if ($under -ne 'detail: Three tests fail on main / button: Recheck / button: Claude / button: Snooze 1 d / button: 7 d / button: Open') {
    Add-Failure "the opened row's detail and buttons aren't under it: $under"
  }
  Get-Shot $w -Name 'opened' | ForEach-Object Dispose

  # 5. Drop asks first
  Click-Item $w $s 'Postbox: Reply to the landlord'
  $s = Wait-Change { param($s) (Labels $s) -contains 'button: Drop' } -What "the landlord's row opened"
  if ((Labels $s) -contains 'button: Recheck') { Add-Failure 'opening a row left the one before open' }
  $seen.Clear()
  Click-Item $w $s 'button: Drop'
  $s = Wait-Change { param($s) (Labels $s) -contains 'button: Drop this for good?' } -What 'Drop asking first' -OnState $collect
  & $flashed 'Drop, asking first'
  Wait-Ms 300
  if (@(Reported).Count) { Add-Failure "the first press on Drop ran it: $(Reported)" }
  Click-Item $w $s 'button: Drop this for good?'
  $s = Wait-Change { param($s) -not ((Labels $s) -match '^button: ') } -What 'the row closed after the press' -OnState $collect
  & $flashed 'Drop, pressed again'
  if (-not (Wait-Until { @(Reported).Count -ge 1 } -TimeoutSec 5)) { Add-Failure 'the handler heard nothing for Drop' }
  elseif (@(Reported)[0] -ne 'do|Index|y1|Postbox|Drop|true') { Add-Failure "the handler heard '$(@(Reported)[0])' for Drop" }
  else { "Drop asked, then ran: $(@(Reported)[0])" }

  # 6. The file says what the press did: the row gone, one fewer failing
  Write-Agenda @($rows | Where-Object { $_.id -notin 'y1', 'f2b' }) 1 2
  $s = Wait-Change { param($s) $s.strip.agenda.failing -eq 2 -and -not ((Labels $s) -contains 'Postbox: Reply to the landlord') } -What 'the button and list following the file' -TimeoutSec 5
  if ((Labels $s) -contains 'Unresolved incidents · Harbor (1)') { Add-Failure 'a group of one is still folded' }
  'the file rewritten: 2 failing, the row gone'

  # 7. Snooze and Open
  Click-Item $w $s 'Kiln Lab: The check fails'
  $s = Wait-Change { param($s) (Labels $s) -contains 'button: Snooze 1 d' } -What 'the failing row opened'
  $seen.Clear()
  Click-Item $w $s 'button: Snooze 1 d'
  $s = Wait-Change { param($s) -not ((Labels $s) -match '^button: ') } -What 'the row closed after Snooze' -OnState $collect
  & $flashed 'Snooze 1 d'
  if (-not (Wait-Until { @(Reported).Count -ge 2 } -TimeoutSec 5)) { Add-Failure 'the handler heard nothing for Snooze' }
  elseif (@(Reported)[1] -ne 'snooze|Index|f1|Kiln Lab|1|true') { Add-Failure "the handler heard '$(@(Reported)[1])' for Snooze" }
  $s = Wait-Settled
  Click-Item $w $s 'Kiln Lab: The check fails'
  $s = Wait-Change { param($s) (Labels $s) -contains 'button: Open' } -What 'the failing row opened again'
  $seen.Clear()
  Click-Item $w $s 'button: Open'
  $s = Wait-Change { param($s) -not $s.menu } -What 'the list closed by Open' -OnState $collect
  & $flashed 'Open, as the list closes'
  if (-not (Wait-Until { @(Reported).Count -ge 3 } -TimeoutSec 5)) { Add-Failure 'the handler heard nothing for Open' }
  elseif (@(Reported)[2] -ne 'open|Index|f1|Kiln Lab|/d/projects|true') { Add-Failure "the handler heard '$(@(Reported)[2])' for Open" }
  else { 'snooze and open heard' }

  # 8. The keys
  Send-Click $w $bc[0] $bc[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'agenda' } -What 'the list again'
  Send-Key $w Down
  $s = Wait-Change { param($s) $null -ne $s.menu.focus } -What 'a line picked by the keys'
  if ($s.menu.items[$s.menu.focus].label -ne 'Ledger: Pay the car registration') { Add-Failure "Down picked '$($s.menu.items[$s.menu.focus].label)', not the first row" }
  $seen.Clear()
  Send-Key $w Enter
  $s = Wait-Change { param($s) (Labels $s) -contains 'button: Tick' } -What 'Enter opening the row' -OnState $collect
  & $flashed 'Enter on the row'
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'the list closed by Esc' | Out-Null
  'keys: Down, Enter, Esc'

  # 9. No file, no button
  Remove-Item $file
  Wait-Change { param($s) $null -eq $s.strip.agenda -and -not (Get-Item-Named $s 'agenda') } -What 'the button gone with its file' -TimeoutSec 5 | Out-Null
  'the file removed: no button'
} -Passed 'agenda passed'
