#Requires -Version 7
# Covers: lbterm-extras/src/prompts.rs, wezterm-gui/src/termwindow/render/prompt.rs, wezterm-gui/src/termwindow/render/strip.rs, lbterm-extras/src/strip.rs, lbterm/perf/fake-claude.ps1
<#
The strip's last prompt (lbterm-extras/src/prompts.rs, wezterm-gui/src/termwindow/render/prompt.rs)
end to end, in a test window that looks like Lachlan's LBTerm (New-TestConfig) with
lbterm_prompt_history pointed at a history of its own, <out>\history.jsonl. Its pane runs
fake-claude.ps1, a stand-in for Claude Code's fullscreen renderer: twelve messages and their
answers, scrolled by the program itself with PgUp and Ctrl+End, the pane's claude_session and
claude_prompt user vars set as claudehook.exe sets them. The history holds those twelve, a slash
command (left out of the list), another session's prompt, and one from before a compaction,
which the conversation no longer shows.

1. The strip's second row shows the arrow and the last prompt, cut short (start.png).
2. A click on the arrow drops the list of the session's prompts, newest first (list.png).
3. Down eight times and Enter: the list closes and the jump scrolls the conversation back to
   message 5, through the header, which doesn't jump here; its row is marked, then not
   (jumped.png, unmarked.png). The pane's text is read with `lbterm cli get-text`.
4. A click on the strip's prompt jumps to the last message, back at the bottom.
5. The oldest prompt in the list, from before the compaction: the jump reaches the top of the
   conversation and stops, and a note takes the prompt's place on the strip (note.png).
6. The pointer resting on the prompt cut short shows all of it in the strip's card
   (prompt-card.png).

Where the arrow and the prompt are, and when the list, the jump, the mark, the note and the card
come and go, are what the state file (lbterm/STATE.md) says. Mouse and keys are posted messages,
never real input; a posted right press puts the pointer on the prompt without TME_LEAVE taking it
away again, as in button-hover.ps1.

  pwsh -File lbterm\perf\strip-prompt.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'strip-prompt' -Bin $Bin
$history = Join-Path $T.Out 'history.jsonl'
$sums = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $sums | Out-Null

# The pane's text, until a line below the top one matches `$match` (the top is the header, which
# shows the last message above the view: not there yet), up to `$seconds`
function Wait-Text([string]$match, [double]$seconds = 8) {
  Wait-Until { @(Invoke-Cli get-text --pane-id $pane | Select-Object -Skip 1) -match $match } -TimeoutSec $seconds -IntervalMs 250
}

$sid = '7d2a9c41-5e3b-4f60-8a71-2c9e0b4d6f13'
$long = 'can we work on the previous user sent message feature? Currently, I have to scroll up in the conversation, then my last prompt to you appears. I want this always there, and to look more aesthetic and readable, every time, in every tab'
$records = [Collections.Generic.List[string]]::new()
$record = { param($display, $at, $who) $records.Add((@{ display = $display; pastedContents = @{}; timestamp = $at; project = 'C:\Windows'; sessionId = $who } | ConvertTo-Json -Compress)) }
& $record 'a message from before the compaction' 1000 $sid
for ($m = 1; $m -le 12; $m++) {
  $text = if ($m -eq 12) { $long } else { "message number $m, about part $m of the work" }
  & $record $text (2000 + $m * 1000) $sid
}
& $record '/model' 9500 $sid
& $record 'another session''s prompt' 9600 '11111111-2222-3333-4444-555555555555'
[IO.File]::WriteAllText($history, ($records -join "`n") + "`n")

Invoke-Test {
  $fake = Join-Path $PSScriptRoot 'fake-claude.ps1'
  $w = Open-TestWindow -Program (New-PwshProgram -File $fake -ArgumentList '-Sid', $sid, '-Messages', 12, '-Long', $long) -Lua @"
config.lbterm_prompt_history = '$(ConvertTo-LuaPath $history)'
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.initial_cols = 110
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $pane 'Strip prompt test' | Out-Null

  # 1. The second row: the arrow, then the prompt, between the first row and the panes
  $e = Wait-Change { param($s) $s.strip.prompt -and @($s.strip.items | Where-Object name -eq 'arrow') } -What 'the prompt on the strip'
  $start = Get-Shot $w -Name 'start'
  $row = [LbtermE2E.Px]::BrightBox($start, (New-Rect 40 30 860 30), 150)
  if (-not $row) { throw "nothing on the strip's second row (start.png)" }
  "second row: $(Format-Box $row)"
  $arrow = Get-RectCenter @($e.strip.items | Where-Object name -eq 'arrow')[0].rect
  $text = Get-RectCenter @($e.strip.items | Where-Object name -eq 'prompt')[0].rect
  if ($row[2] - $row[0] -lt 400) { Add-Failure "the prompt on the second row is short for a cut-short prompt: $(Format-Box $row)" }

  # 2. The list
  Send-Click $w $arrow[0] $arrow[1]
  $m = Wait-Change { param($s) $s.menu.kind -eq 'prompts' } -What 'the list of prompts'
  $list = Get-Shot $w -Name 'list'
  $d = Get-DiffBox $start $list
  if (-not $d -or $d[3] - $d[1] -lt 250) { Add-Failure "the list didn't drop under the strip with its 13 prompts: $(Format-Box $d)" }
  else { "list: $(Format-Box $d), $(@($m.menu.items).Count) prompts in it" }

  # 3. Down eight, Enter: message 5, through the header and up the conversation
  Send-Key $w (@('Down') * 8)
  Send-Key $w Enter
  if (Wait-Text '^\s*\u276f message number 5,') {
    # LBTerm marks the row once the screen has held still, a moment after it's drawn
    Wait-State { param($s) $s.landed -and -not $s.strip.jumping } -What 'the landed row marked' | Out-Null
    $jumped = Get-Shot $w -Name 'jumped'
    Wait-Settled -What 'the mark to go' | Out-Null
    $unmarked = Get-Shot $w -Name 'unmarked'
    $m = Get-DiffBox $jumped $unmarked
    if (-not $m) { Add-Failure 'no mark on the row the jump landed on' } else { "jumped to message 5, marked at $(Format-Box $m)" }
    $jumped.Dispose(); $unmarked.Dispose()
  } else { Add-Failure 'the jump never brought message 5 into view' }

  # 4. The strip's own prompt: the last message, at the bottom again
  Send-Click $w $text[0] $text[1]
  if (Wait-Text '^\s*\u276f can we work on the previous') { 'jumped to the last prompt' }
  else { Add-Failure 'a click on the strip''s prompt never brought the last message into view' }
  Wait-Settled -What 'the jump to the last prompt to settle' | Out-Null

  # 5. From before the compaction: to the top, and a note
  Send-Click $w $arrow[0] $arrow[1]
  Wait-Change { param($s) $s.menu.kind -eq 'prompts' } -What 'the list again' | Out-Null
  Send-Key $w End
  Send-Key $w Enter
  # At the top, message 1 is the first row, where a header would be: its first answer says so
  if (-not (Wait-Text 'answer 1, line 1\s*$' 15)) { Add-Failure 'the jump to a compacted prompt never reached the top of the conversation' }
  else { 'reached the top of the conversation' }
  # The note, once the jump gives up there, against the second row once it's gone, the pointer
  # where it was for both
  $n = Wait-State { param($s) $s.strip.note } -What 'the note on the strip'
  $note = Get-Shot $w -Name 'note'
  Wait-State { param($s) -not $s.strip.note -and $s.settled } -What 'the note to go' | Out-Null
  $gone = Get-Shot $w -Name 'note-gone'
  $band = New-Rect 0 ($row[1] - 3) $note.Width ($row[3] - $row[1] + 6)
  $nb = [LbtermE2E.Px]::DiffBox($note.Clone($band, $note.PixelFormat), $gone.Clone($band, $gone.PixelFormat), (New-Rect 0 0 $band.Width $band.Height))
  if (-not $nb) { Add-Failure 'no note on the strip after a jump that couldn''t get there' } else { "note on the second row ('$($n.strip.note)'), then the prompt again: $(Format-Box $nb)" }
  $note.Dispose(); $gone.Dispose()

  # 6. The pointer resting on the prompt: all of it in the card
  $rest = Get-Shot $w
  Send-Click $w $text[0] $text[1] -Right
  Wait-Change { param($s) $s.strip.card -eq 'prompt' } -What 'the prompt''s card' | Out-Null
  $card = Get-Shot $w -Name 'prompt-card'
  $c = Get-DiffBox $rest $card
  if (-not $c -or $c[3] -lt 80) { Add-Failure "the pointer on the prompt opened no card: $(Format-Box $c)" } else { "prompt card: $(Format-Box $c)" }
  Send-Leave $w
  $rest.Dispose(); $card.Dispose(); $start.Dispose(); $list.Dispose()
} -Passed 'strip prompt passed'
