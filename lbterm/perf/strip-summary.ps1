#Requires -Version 7
# Covers: lbterm-extras/src/strip.rs, lbterm-extras/src/summaries.rs, lbterm-panel/src/hover.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/hover_fade.rs
<#
The window strip's words (lbterm-extras/src/strip.rs, wezterm-gui/src/termwindow/render/strip.rs)
end to end, in a test window that looks like Lachlan's LBTerm (New-TestConfig) with
lbterm_summaries pointed at a folder of its own, <out>\summaries:

1. A tab that isn't a Claude session: the strip shows its title alone (title.png).
2. Its pane says it's a Claude session (claudehook.exe's `claude_session` user var, sent with
   OSC 1337) whose summary is in the folder: the summary and "now" join the title, and all that
   changed lies in the strip, so the panes' cards didn't move (summary.png).
3. expanded.flag in the folder (Terminal's Alt+S): the detail drops down in a card under the
   words, over the pane (card.png). Taking it away takes the card away again (unpinned.png).
   The pointer arriving on the words brightens the summary at once, within the strip and before
   the card's 300 ms rest is up (answered.png: every action answers at once, lbterm/DESIGN.md),
   then opens the card after the wait (hovered.png), and leaving the window closes it and puts
   the strip back as it was (left.png).
4. The worker writes a new summary: the strip says it (updated.png).

Each step waits on what the state file (lbterm/STATE.md) says the strip shows, settled, then
captures it; the pictures are compared to see that only the strip changed. The pointer arrives
as a posted right press on the words where the file says they are, as in button-hover.ps1: a
posted WM_MOUSEMOVE makes the window ask for TME_LEAVE, and with the real pointer elsewhere
Windows answers with WM_MOUSELEAVE at once, so a hover from a move is gone before the next frame.
The delays are lbterm_panel::Hover's, tested there; the card's layout is lbterm-extras'.

  pwsh -File lbterm\perf\strip-summary.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'strip-summary' -Bin $Bin
$sums = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $sums | Out-Null

$sid = '5f0c2d7e-1b3a-4e6f-9a8b-7c6d5e4f3a21'
[IO.File]::WriteAllText((Join-Path $sums "$sid.txt"), "Strip summary: the words and the card, end to end`n")
[IO.File]::WriteAllText((Join-Path $sums "$sid.detail.txt"), @"
Goal: show the active tab's Claude summary on the window strip, the detail under the pointer.
Finding: the strip is built again only when what it says changes.

State: the card draws over the pane, under the tab panel.
Next: install it, then let Lachlan try it.
"@)
# The pane says it's a Claude session (OSC 1337 SetUserVar, as claudehook.exe sends it) once the
# go file appears, and draws nothing else, so the pane holds still between the shots
$go = Join-Path $T.Out 'go'

Invoke-Test {
  $w = Open-TestWindow -Program (New-ClaudeSessionProgram $sid -WaitFor $go) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.initial_cols = 110
config.initial_rows = 20
-- Nothing moves but what the strip says
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $pane 'Strip summary test' | Out-Null
  # What the step does, settled, then the picture of it
  $shows = { param([scriptblock]$what, [string]$name, [string]$desc) $script:entry = Wait-Change $what -What $desc; Get-Shot $w -Name $name }

  # 1. Not a Claude session yet: the title alone
  $title = & $shows { param($s) $s.strip.title -eq 'Strip summary test' } 'title' 'the title on the strip'

  # 2. A Claude session whose summary is in the folder
  [IO.File]::WriteAllText($go, '')
  $summary = & $shows { param($s) $s.strip.summary -match 'the words and the card' } 'summary' 'the summary on the strip'
  $strip = 40
  $d = Get-DiffBox $title $summary
  if (-not $d) { Add-Failure 'the strip showed no summary for the Claude session' }
  elseif ($d[3] -ge $strip) { Add-Failure "the summary changed more than the strip: $(Format-Box $d)" }
  else { "summary: the strip changed at $(Format-Box $d), the panes not at all" }
  # Where the summary's words are, for the pointer below
  $item = @($script:entry.strip.items | Where-Object name -eq 'words')[0]
  if (-not $item) { throw 'the state file has no words on the strip' }
  $words = Get-RectCenter $item.rect

  # 3. Alt+S's flag pins the card open
  $flag = Join-Path $sums 'expanded.flag'
  [IO.File]::WriteAllText($flag, '')
  $card = & $shows { param($s) $s.strip.card -eq 'summary' } 'card' 'the card pinned open'
  $d = Get-DiffBox $summary $card
  if (-not $d) { Add-Failure 'expanded.flag opened no card' }
  elseif ($d[3] -lt $strip + 40) { Add-Failure "the card didn't drop under the strip: $(Format-Box $d)" }
  else { "card: $(Format-Box $d)" }
  Remove-Item $flag
  $unpinned = & $shows { param($s) -not $s.strip.card } 'unpinned' 'the card to go with the flag'
  $d = Get-DiffBox $summary $unpinned
  if ($d) { Add-Failure "the card left something behind when the flag went: $(Format-Box $d)" }
  else { 'unpinned: the card went, and the window is as it was' }

  # 3b. The pointer resting on the words opens it, and leaving the window closes it. A posted
  # right press is the pointer arriving (posted moves can't be: TME_LEAVE), and does nothing
  # else on the words; a posted WM_MOUSELEAVE is it leaving
  Send-Click $w $words[0] $words[1] -Right
  # The summary's answer: its hover fade (button_hover, 120 ms) over, while the card's 300 ms
  # rest is still under way
  $answering = Wait-State { param($s)
    @($s.strip.lit) -contains 'words' -and -not ($s.animations | Where-Object name -eq 'strip.hover') -and
      ($s.waits | Where-Object name -eq 'strip.card.open') } -What 'the words lit, the card still waiting'
  $answered = Get-Shot $w -Name 'answered'
  $d = Get-DiffBox $unpinned $answered
  if (-not $d) { Add-Failure 'nothing answered the pointer on the words before the card opened' }
  elseif ($d[3] -ge $strip) { Add-Failure "before the card's rest was up, more than the strip changed: $(Format-Box $d)" }
  else { "answered: the summary brightened at $(Format-Box $d) before the card opened ($(@($answering.waits | ForEach-Object { "$($_.name) $($_.left_ms) ms left" }) -join ', '))" }
  $hovered = & $shows { param($s) $s.strip.card -eq 'summary' } 'hovered' 'the card under the pointer'
  $d = Get-DiffBox $unpinned $hovered
  if (-not $d) { Add-Failure 'the card never opened under the pointer' }
  elseif ($d[3] -lt $strip + 40) { Add-Failure "the card didn't drop under the strip for the pointer: $(Format-Box $d)" }
  else { "hovered: the card opened under the pointer at $(Format-Box $d)" }
  Send-Leave $w
  $left = & $shows { param($s) -not $s.strip.card -and -not $s.pointer } 'left' 'the card to close as the pointer left'
  $d = Get-DiffBox $unpinned $left
  if ($d) { Add-Failure "the card stayed after the pointer left: $(Format-Box $d)" }
  else { 'left: the card closed when the pointer left' }

  # 4. A new summary from the worker
  [IO.File]::WriteAllText((Join-Path $sums "$sid.txt"), "Strip summary: a new line from the worker`n")
  $updated = & $shows { param($s) $s.strip.summary -match 'a new line from the worker' } 'updated' 'the new summary'
  $d = Get-DiffBox $left $updated
  if (-not $d) { Add-Failure 'the strip never showed the new summary' }
  elseif ($d[3] -ge $strip) { Add-Failure "the new summary changed more than the strip: $(Format-Box $d)" }
  else { "updated: the strip changed at $(Format-Box $d)" }

  foreach ($s in @($title, $summary, $card, $unpinned, $answered, $hovered, $left, $updated)) { $s.Dispose() }
} -Passed 'strip summary passed'
