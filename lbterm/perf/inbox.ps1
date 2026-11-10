#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/inbox.rs, wezterm-gui/src/inbox.rs, lbterm-extras/src/inbox.rs, wezterm-gui/src/termwindow/render/fancy_tab_bar.rs, wezterm-gui/src/termwindow/render/strip_swap.rs
<#
The InMail button (`lbterm_inbox`; wezterm-gui/src/termwindow/render/inbox.rs, wezterm-gui/src/
inbox.rs, lbterm-extras/src/inbox.rs; lbterm/INBOX.md) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig), its folders three projects' InMail folders in
target\perf\inbox\projects (one of them never made). Its `lbterm-inbox-open` handler writes down
each message it's given; for project Two it lets the default run, a new tab in the folder above the
message's, and for the others it returns false.

1. One message waiting: the button sits in the strip's first row over the rail, saying 1 in the
   waiting orange (one.png).
2. A message sent to Two: the button says 2 within a couple of seconds, brightening once (two.png).
3. A click on the button drops the list, newest first: Two's, then One's (list.png).
4. Down and Enter: the row flashes white as the list goes (menu.list.flash, Lachlan's 9b), the
   handler hears Two's message (its project, subject, sender, folder and file) and lets the
   default run: a new tab, its pane in Two's folder.
5. The list again, a click on One's row: the handler hears it and returns false, so no tab opens.
   Esc closes the list.
5b. A conversation takes One's message (Lachlan's E1 and E2 of 2026-11-08): a claim beside it, as
   InMail writes one. The count stays 2; One's row is `checked` and says `taken HH:MM` in the rail's
   working green (taken.png); a pick hands the handler the claim's session as `taken`. Handed back
   (`released`), the row is plain again.
6. One's message archived (moved out): the button says 1. Both gone: the button sits back, no count
   and no orange (none.png), and its list says none wait.

Where the button and the list's rows are, and when the count, the list and the tabs have changed,
come from the window's state file (lbterm/STATE.md); the pictures say how they look. Clicks and keys
are posted messages, never real input.

  pwsh -File lbterm\perf\inbox.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'inbox' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$report = Join-Path $T.Out 'report.txt'
Remove-Item -Recurse -Force $projects, $report -ErrorAction SilentlyContinue
$folders = foreach ($p in 'One', 'Two', 'Three') { Join-Path $projects "$p\InMail" }
New-Item -ItemType Directory -Force $folders[0], $folders[1] | Out-Null

# A message as InMail writes one
function Send-Message([string]$To, [string]$From, [string]$Subject, [datetime]$At) {
  $name = '{0:yyyy-MM-dd-HHmm}-from-{1}-{2}.md' -f $At, $From, ($Subject.ToLower() -replace '[^a-z0-9]+', '-').Trim('-')
  $text = "---`nfrom: $From`nto: $To`nsent: {0:yyyy-MM-ddTHH:mm}`nsubject: `"$Subject`"`n---`n`nThe body.`n" -f $At
  $file = Join-Path $projects "$To\InMail\$name"
  [IO.File]::WriteAllText($file, $text)
  $file
}
function Reported { @(Get-Content $report -ErrorAction SilentlyContinue) }
# The button's place on the strip, from the state file
function Get-Button($s) { @($s.strip.items | Where-Object name -eq 'inbox')[0] }
# Pixels of the waiting orange (#FF8A3D) in a box of a capture
function Get-Orange($shot, $rect) {
  [LbtermE2E.Px]::CountRange($shot, (New-Rect $rect.x $rect.y $rect.w $rect.h), (New-ColourRange 200 255 100 175 20 110))
}

$now = Get-Date
$one = Send-Message -To 'One' -From 'Two' -Subject 'The first message' -At $now.AddHours(-3)
# With backslashes, as Lachlan's config names them: the handler is given the message's folder and
# its project's as the config wrote them
$luaFolders = ($folders | ForEach-Object { "[[$_]]" }) -join ', '

Invoke-Test {
  $w = Open-TestWindow -Lua @"
rawset(config, 'lbterm_inbox', { folders = { $luaFolders } })
wezterm.on('lbterm-inbox-open', function(window, pane, m)
  local f = io.open('$(ConvertTo-LuaPath $report)', 'a')
  if f then
    f:write(table.concat({ m.project, m.subject, m.from, m.dir, m.folder, m.name, m.sent, m.taken or '' }, '|') .. '\n')
    f:close()
  end
  if m.project ~= 'Two' then return false end
end)
config.initial_cols = 110
config.initial_rows = 28
"@
  # 1. One waiting
  $s = Wait-Change { param($s) $s.strip.inbox -eq 1 -and (Get-Button $s) } -What 'the button saying 1'
  $b = (Get-Button $s).rect
  $rail = $s.rail.rect
  "button: x $($b.x)..$($b.x + $b.w), y $($b.y)..$($b.y + $b.h); rail x $($rail.x)..$($rail.x + $rail.w)"
  $middle = $b.x + $b.w / 2
  $railMiddle = $rail.x + $rail.w / 2
  if ([Math]::Abs($middle - $railMiddle) -gt 2) { Add-Failure "the button isn't centred over the rail: its middle $middle, the rail's $railMiddle" }
  if ($b.y + $b.h -gt $rail.y) { Add-Failure "the button runs into the rail: its bottom $($b.y + $b.h), the rail's top $($rail.y)" }
  $words = @($s.strip.items | Where-Object name -eq 'words')[0]
  if ($words -and $b.x + $b.w -gt $words.rect.x) { Add-Failure "the button runs into the strip's words at x $($words.rect.x)" }
  $shot = Get-Shot $w -Name 'one'
  $orange = Get-Orange $shot $b
  if ($orange -lt 6) { Add-Failure "no count in the waiting orange on the button ($orange pixels; one.png)" } else { "the count in orange: $orange pixels" }
  $shot.Dispose()

  # 2. A message for Two: the count goes up within the thread's second, and the button glints
  $two = Send-Message -To 'Two' -From 'One' -Subject 'A newer message' -At $now.AddMinutes(-5)
  $s = Wait-Change { param($s) $s.strip.inbox -eq 2 } -What 'the button saying 2' -TimeoutSec 5
  Get-Shot $w -Name 'two' | ForEach-Object Dispose
  'two waiting'

  # 3. The list, newest first
  $c = Get-RectCenter (Get-Button $s).rect
  $before = Get-Shot $w
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'inbox' } -What 'the list'
  $labels = @($s.menu.items | ForEach-Object label)
  "list: $($labels -join ' / ')"
  if ($labels.Count -ne 2 -or $labels[0] -ne 'Two: A newer message' -or $labels[1] -ne 'One: The first message') {
    Add-Failure "the list isn't the two messages newest first: $($labels -join ' / ')"
  }
  $card = $s.menu.rect
  if ([Math]::Abs($card.x - $b.x) -gt 12 -or $card.y -lt $b.y + $b.h) {
    Add-Failure "the list doesn't hang under the button: its card at x $($card.x), y $($card.y); the button x $($b.x), bottom $($b.y + $b.h)"
  }
  $list = Get-Shot $w -Name 'list'
  $under = $b.y + $b.h + 1
  $d = Get-DiffBox $before $list -Region (New-Rect 0 $under $list.Width ($list.Height - $under))
  if (-not $d -or $d[3] - $d[1] -lt 40) { Add-Failure "no list under the strip: $(Format-Box $d)" } else { "list drawn at $(Format-Box $d)" }
  if (-not (Get-DiffBox $before $list -Region (New-Rect $b.x $b.y $b.w $b.h))) { Add-Failure 'the button didn''t stay lit with its list open' }
  $before.Dispose(); $list.Dispose()

  # 4. Down, Enter: Two's message to the handler, then the default's new tab in Two's folder
  Send-Key $w Down
  Wait-Change { param($s) $s.menu.focus -eq 0 } -What 'the first row picked by the keys' | Out-Null
  $flashed = [System.Collections.Generic.HashSet[string]]::new()
  Send-Key $w Enter
  $s = Wait-Change { param($s) -not $s.menu -and @($s.tabs).Count -eq 2 } -What 'the list closed and a new tab' -OnState { param($s) foreach ($a in @($s.animations)) { [void]$flashed.Add($a.name) } }
  if (-not $flashed.Contains('menu.list.flash')) { Add-Failure "the row picked didn't flash: no menu.list.flash in [$(@($flashed) -join ', ')]" } else { 'the row picked flashed (menu.list.flash)' }
  if (-not (Wait-Until { @(Reported).Count -ge 1 } -TimeoutSec 5)) { Add-Failure 'the handler heard nothing' }
  $heard = @(Reported)[0] -split '\|'
  "heard: $(@(Reported)[0])"
  $want = @('Two', 'A newer message', 'One', (Join-Path $projects 'Two'), $folders[1], (Split-Path $two -Leaf), ('{0:yyyy-MM-ddTHH:mm}' -f $now.AddMinutes(-5)))
  for ($i = 0; $i -lt $want.Count; $i++) {
    if ($heard[$i] -ne $want[$i]) { Add-Failure "the handler's message field $i is '$($heard[$i])', not '$($want[$i])'" }
  }
  $inTwo = Wait-Until {
    @(Get-Panes | Where-Object { [Uri]::UnescapeDataString([string]$_.cwd) -replace '/', '\' -match [regex]::Escape((Join-Path $projects 'Two')) }).Count -eq 1
  } -TimeoutSec 15 -IntervalMs 500
  if (-not $inTwo) { Add-Failure "the new tab isn't in Two's folder: $((Get-Panes | ForEach-Object cwd) -join ', ')" } else { "the new tab is in $(Join-Path $projects 'Two')" }

  # 5. One's row by a click: the handler takes it, so no tab
  $s = Wait-Settled
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'inbox' -and @($s.menu.items).Count -eq 2 } -What 'the list again'
  $row = Get-RectCenter $s.menu.items[1].rect
  Send-Click $w $row[0] $row[1]
  $s = Wait-Change { param($s) -not $s.menu } -What 'the list closed by a pick'
  if (-not (Wait-Until { @(Reported).Count -ge 2 } -TimeoutSec 5)) { Add-Failure 'the handler never heard One''s message' }
  elseif ((@(Reported)[1] -split '\|')[0] -ne 'One') { Add-Failure "the second message the handler heard is $(@(Reported)[1])" }
  Wait-Ms 500
  if (@((Read-State).windows[0].tabs).Count -ne 2) { Add-Failure 'a tab opened for a message the handler took' } else { 'the handler took One''s: no tab' }
  Send-Click $w $c[0] $c[1]
  Wait-Change { param($s) $s.menu.kind -eq 'inbox' } -What 'the list, to close' | Out-Null
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'the list closed by Esc' | Out-Null

  # 5b. Taken: a claim beside One's message, as InMail writes one when a conversation reads it
  $session = '0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d'
  [IO.File]::WriteAllText("$one.taken", '{"session":"' + $session + '","at":"' + $now.ToString('yyyy-MM-ddTHH:mm') + '"}')
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'inbox' -and @($s.menu.items | Where-Object checked).Count -eq 1 } -What 'the list with One''s message taken' -TimeoutSec 5
  if ($s.strip.inbox -ne 2) { Add-Failure "a claim changed the count: the button says $($s.strip.inbox)" }
  $taken = @($s.menu.items | Where-Object checked | ForEach-Object label)
  if ($taken.Count -ne 1 -or $taken[0] -ne 'One: The first message') { Add-Failure "the taken row is $($taken -join ' / ')" } else { "taken: $($taken[0])" }
  $shot = Get-Shot $w -Name 'taken'
  $r = $s.menu.items[1].rect
  # The rail's working green, #3DDC84
  $green = [LbtermE2E.Px]::CountRange($shot, (New-Rect $r.x $r.y $r.w $r.h), (New-ColourRange 30 100 190 245 100 165))
  if ($green -lt 6) { Add-Failure "no 'taken' in the working green on One's row ($green pixels; taken.png)" } else { "taken in green: $green pixels" }
  $shot.Dispose()
  $row = Get-RectCenter $r
  Send-Click $w $row[0] $row[1]
  Wait-Change { param($s) -not $s.menu } -What 'the list closed by the taken pick' | Out-Null
  if (-not (Wait-Until { @(Reported).Count -ge 3 } -TimeoutSec 5)) { Add-Failure 'the handler never heard the taken message' }
  else {
    $heard = @(Reported)[2] -split '\|'
    if ($heard[0] -ne 'One' -or $heard[7] -ne $session) { Add-Failure "the handler heard $(@(Reported)[2]), not One's with taken $session" } else { "the handler was given taken $session" }
  }
  # Handed back: InMail adds released and why, and the row is plain again
  [IO.File]::WriteAllText("$one.taken", '{"session":"' + $session + '","released":"' + $now.ToString('yyyy-MM-ddTHH:mm') + '","why":"a test"}')
  Wait-Settled | Out-Null
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'inbox' -and @($s.menu.items).Count -eq 2 -and @($s.menu.items | Where-Object checked).Count -eq 0 } -What 'the list with nothing taken' -TimeoutSec 5
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'the list closed' | Out-Null

  # 6. Archived: one, then none (the claim left beside One's archived message counts nothing)
  Remove-Item $one
  Wait-Change { param($s) $s.strip.inbox -eq 1 } -What 'the button saying 1 again' -TimeoutSec 5 | Out-Null
  Remove-Item $two
  $s = Wait-Change { param($s) $s.strip.inbox -eq 0 } -What 'the button with none' -TimeoutSec 5
  $shot = Get-Shot $w -Name 'none'
  $b = (Get-Button $s).rect
  $orange = Get-Orange $shot $b
  if ($orange -gt 0) { Add-Failure "orange on the button with nothing waiting ($orange pixels; none.png)" }
  $shot.Dispose()
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'inbox' } -What 'the empty list'
  if (@($s.menu.items).Count -ne 1 -or $s.menu.items[0].label -ne 'No InMail waiting') { Add-Failure "the empty list says $(@($s.menu.items | ForEach-Object label) -join ' / ')" }
  else { 'none waiting: the button sits back, its list says so' }
  Send-Key $w Escape
} -Passed 'inbox passed'
