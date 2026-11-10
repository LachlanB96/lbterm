#Requires -Version 7
# Covers: lbterm-extras/src/conversation.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, wezterm-gui/src/termwindow/render/objectives.rs, lbterm/perf/fake-claude.ps1
<#
The scroll strip (lbterm/SCROLL-STRIP.md) end to end, in a test window that looks like Lachlan's
LBTerm (New-TestConfig) with lbterm_scroll_strip's transcripts and lbterm_summaries pointed at
folders of its own. Its pane runs fake-claude.ps1, a stand-in for Claude Code's fullscreen view:
twelve messages and their answers, scrolled by the program itself (PgUp, PgDn, Ctrl+Home,
Ctrl+End), the pane's claude_session set as claudehook.exe sets it. The transcript says the same
twelve messages, with an edit, a commit, a failed build and each turn's end, and the summaries
folder a permission prompt on the commit, as claudehook records one.

1. The strip shows the conversation: a mark for each prompt and each event, the thumb at the
   bottom (start.png).
2. A click on the third prompt's mark scrolls the conversation back to message 3, and its row is
   marked (jumped.png); the thumb moves up.
3. Resting on the fifth prompt's mark opens the card with its words (card.png).
4. A click on the track low down scrolls back near the end.
5. Dragging the thumb to the top scrolls to the top of the conversation.
6. A second tab printing 40 shell prompts: the strip is its scrollback's, a bar at each prompt,
   and a click on one scrolls the scrollback to it (shell.png).

Where the marks, the thumb and the card are, and when a seek is done, are what the state file
(lbterm/STATE.md) says; the pane's text is read with `lbterm cli get-text`. Mouse and keys are
posted messages, never real input.

  pwsh -File lbterm\perf\scroll-strip.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'scroll-strip' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
$folder = Join-Path $projects 'C--work-demo'
New-Item -ItemType Directory -Force $folder, $sums | Out-Null

# The pane's text, until a line below the top one matches `$match` (the top is the header), up to
# `$seconds`
function Wait-Text([string]$match, [double]$seconds = 10) {
  Wait-Until { @(Invoke-Cli get-text --pane-id $pane | Select-Object -Skip 1) -match $match } -TimeoutSec $seconds -IntervalMs 250
}
# The strip's marks of a kind, top to bottom
function Marks($s, [string]$kind) { @($s.scroll_strip.marks | Where-Object kind -eq $kind | Sort-Object { $_.rect.y }) }

$sid = '3f8e2b17-9c4d-4a6e-b1f0-5d7c2e9a4b18'
$start = [DateTime]::new(2026, 10, 28, 9, 0, 0, [DateTimeKind]::Utc)
$records = [Collections.Generic.List[string]]::new()
$second = 0
function Add-Record([hashtable]$r) {
  $script:second += 5
  $r.timestamp = $start.AddSeconds($script:second).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $r.isSidechain = $false
  $records.Add(($r | ConvertTo-Json -Compress -Depth 8))
}
function Add-Tool([string]$id, [string]$name, [hashtable]$tool, [string]$result, [bool]$failed = $false) {
  Add-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'tool_use'; id = $id; name = $name; input = $tool }) } }
  Add-Record @{ type = 'user'; message = @{ role = 'user'; content = @(@{ type = 'tool_result'; tool_use_id = $id; content = $result; is_error = $failed }) } }
}
$commitAt = 0
for ($m = 1; $m -le 12; $m++) {
  Add-Record @{ type = 'user'; origin = @{ kind = 'human' }; message = @{ role = 'user'; content = "message number $m, about part $m of the work" } }
  if ($m -eq 4) { Add-Tool 't4' 'Edit' @{ file_path = 'C:\work\demo\src\main.rs' } 'ok' }
  if ($m -eq 7) {
    Add-Tool 't7' 'Bash' @{ command = 'git commit -am "Part seven"' } '[main 1a2b3c4] Part seven'
    $commitAt = [DateTimeOffset]::new($start.AddSeconds($second - 5)).ToUnixTimeMilliseconds()
  }
  if ($m -eq 9) { Add-Tool 't9' 'Bash' @{ command = 'cargo build' } 'error: could not compile `demo`' $true }
  Add-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'text'; text = ((1..14 | ForEach-Object { "answer $m, line $_" }) -join "`n") }) } }
  Add-Record @{ type = 'system'; subtype = 'turn_duration'; durationMs = 12000 }
}
[IO.File]::WriteAllText((Join-Path $folder "$sid.jsonl"), ($records -join "`n") + "`n")
# The permission prompt on the commit, as claudehook records it
[IO.File]::WriteAllText((Join-Path $sums "$sid.events.jsonl"), "{`"at`":$($commitAt + 300),`"event`":`"PermissionRequest`",`"tool`":`"Bash`"}`n")

Invoke-Test {
  $fake = Join-Path $PSScriptRoot 'fake-claude.ps1'
  $w = Open-TestWindow -Program (New-PwshProgram -File $fake -ArgumentList '-Sid', $sid, '-Messages', 12) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath $projects)' }
config.initial_cols = 110
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $pane 'Scroll strip test' | Out-Null

  # 1. The conversation on the strip: every kind the transcript has, the thumb at the bottom
  $e = Wait-Change { param($s) $s.scroll_strip.mode -eq 'claude' -and $s.scroll_strip.thumb -and (Marks $s 'prompt').Count -eq 12 } -What 'the conversation on the strip'
  $first = Get-Shot $w -Name 'start'
  $strip = $e.scroll_strip
  $kinds = @($strip.marks.kind | Sort-Object -Unique)
  "marks: $(@($strip.marks).Count) ($($kinds -join ', ')), $($strip.rows) rows, the view at $($strip.view_top)"
  foreach ($want in 'prompt', 'edit', 'commit', 'permission', 'error', 'turn-end') {
    if ($want -notin $kinds) { Add-Failure "no $want mark on the strip" }
  }
  $bottom = $strip.track.y + $strip.track.h
  if ([Math]::Abs($strip.thumb.y + $strip.thumb.h - $bottom) -gt 3) { Add-Failure "the thumb isn't at the bottom of the track: $($strip.thumb | ConvertTo-Json -Compress), the track ends at $bottom" }
  $objectives = $e.objectives.strip
  if ($objectives -and $objectives.x -le $strip.strip.x + $strip.strip.w) { Add-Failure "the objectives strip isn't right of the scroll strip: $($objectives | ConvertTo-Json -Compress)" }
  if ($e.scrollbar.shown) { Add-Failure 'the pane''s own scrollbar still shows beside the strip' }
  # Beside the conversation only: it stops above Claude Code's input box and status, the pane's
  # last eight rows
  $p = @($e.panes)[0]
  $cell = $p.rect.h / $p.rows
  $track = $strip.track
  if ($track.y + $track.h -gt $p.rect.y + $p.rect.h - 7 * $cell) { Add-Failure "the strip runs past the conversation into the input box: the track ends at $($track.y + $track.h), the pane at $($p.rect.y + $p.rect.h), rows of $cell px" }
  else { "the track ends at $($track.y + $track.h), $([int](($p.rect.y + $p.rect.h - $track.y - $track.h) / $cell)) rows above the pane's foot" }

  # 2. The third prompt's mark: message 3 on screen, marked, the thumb up the track
  $at = Get-RectCenter (Marks $e 'prompt')[2].rect
  Send-Click $w $at[0] $at[1]
  if (Wait-Text '^\s*\u276f message number 3,') {
    $j = Wait-State { param($s) $s.landed -and -not $s.scroll_strip.seeking } -What 'the landed row marked'
    (Get-Shot $w -Name 'jumped').Dispose()
    "jumped to message 3; the thumb at $($j.scroll_strip.thumb.y), the view at $($j.scroll_strip.view_top)"
    if ($j.scroll_strip.thumb.y -ge $strip.thumb.y) { Add-Failure "the thumb didn't move up: $($j.scroll_strip.thumb.y) from $($strip.thumb.y)" }
  } else { Add-Failure "a click on the third prompt's mark never brought message 3 into view (note: $((Wait-State { $true }).scroll_strip.note))" }
  Wait-Settled -What 'the jump to settle' | Out-Null
  # The pointer still rests on the mark it clicked, whose card opens: away from it
  Send-Leave $w
  Wait-State { param($s) -not $s.scroll_strip.card -and $s.settled } -What 'the third prompt''s card to close' | Out-Null

  # 3. Resting on the fifth prompt's mark: its card
  $s = Wait-State { $true } -What 'the state'
  $at = Get-RectCenter (Marks $s 'prompt')[4].rect
  Send-Click $w $at[0] $at[1] -Right
  $c = Wait-State { param($s) @($s.scroll_strip.card.lines) -match 'message number 5,' } -What 'the card on the fifth prompt''s mark'
  (Get-Shot $w -Name 'card').Dispose()
  $lines = @($c.scroll_strip.card.lines)
  "card: $($lines -join ' / ')"
  if (-not ($lines -match 'message number 5,')) { Add-Failure "the card doesn't say the fifth prompt: $($lines -join ' / ')" }
  if (-not ($lines -match '^You')) { Add-Failure "the card's head isn't 'You': $($lines -join ' / ')" }
  Send-Leave $w
  Wait-State { param($s) -not $s.scroll_strip.card -and $s.settled } -What 'the card to close' | Out-Null

  # 4. The track low down, clear of the marks and the thumb: near the end again
  $s = Wait-State { $true } -What 'the state'
  $track = $s.scroll_strip.track
  $marksY = @($s.scroll_strip.marks | ForEach-Object { $_.rect.y + $_.rect.h / 2 })
  $y = $null
  for ($try = [int]($track.y + $track.h * 0.9); $try -gt $track.y + $track.h * 0.6; $try--) {
    $clear = -not ($marksY | Where-Object { [Math]::Abs($_ - $try) -lt 9 })
    $offThumb = $try -lt $s.scroll_strip.thumb.y -or $try -gt $s.scroll_strip.thumb.y + $s.scroll_strip.thumb.h
    if ($clear -and $offThumb) { $y = $try; break }
  }
  if ($null -eq $y) { Add-Failure 'no clear place low on the track to click' }
  else {
    Send-Click $w ([int]($track.x + $track.w / 2)) $y
    $l = Wait-State { param($s) -not $s.scroll_strip.seeking -and $s.scroll_strip.thumb.y -gt $track.y + $track.h * 0.4 } -What 'the view near the end' -TimeoutSec 20
    $middle = $l.scroll_strip.thumb.y + $l.scroll_strip.thumb.h / 2
    "track clicked at ${y}: the thumb's middle at $middle"
    if ([Math]::Abs($middle - $y) -gt $track.h * 0.25) { Add-Failure "the track click at $y left the thumb's middle at $middle" }
  }
  Wait-Settled -What 'the track click to settle' | Out-Null

  # 5. The thumb dragged to the top: the conversation's start
  $s = Wait-State { $true } -What 'the state'
  $thumb = Get-RectCenter $s.scroll_strip.thumb
  Send-Drag $w $thumb[0] $thumb[1] $thumb[0] ([int]$s.scroll_strip.track.y)
  if (Wait-Text 'answer 1, line 1\s*$' 20) { 'dragged to the top' }
  else {
    $x = Wait-State { $true } -What 'the state'
    $top = (Invoke-Cli get-text --pane-id $pane | Select-Object -First 8) -join ' / '
    Add-Failure "dragging the thumb to the top never reached the conversation's start: the view at $($x.scroll_strip.view_top), seeking $($x.scroll_strip.seeking), note '$($x.scroll_strip.note)', the screen starts: $top"
  }
  Wait-State { param($s) -not $s.scroll_strip.seeking } -What 'the drag''s seek to end' | Out-Null

  # 6. A shell's scrollback in a second tab
  $script = @"
`$e = [char]27
for (`$i = 1; `$i -le 40; `$i++) {
  [Console]::Write("`$e]133;A`$e\PS C:\demo> `$e]133;B`$e\Get-Thing `$i`n`$e]133;C`$e\")
  for (`$j = 0; `$j -lt (`$i % 4 + 1); `$j++) { [Console]::WriteLine("output line `$j of thing `$i") }
  [Console]::Write("`$e]133;D;0`$e\")
}
Start-Sleep 120
"@
  $shell = New-Tab -From $pane -Program (New-PwshProgram $script)
  Invoke-Cli activate-pane --pane-id $shell | Out-Null
  $h = Wait-Change { param($s) $s.scroll_strip.mode -eq 'shell' -and (Marks $s 'prompt').Count -ge 20 } -What 'the scrollback on the strip' -TimeoutSec 20
  "shell: $((Marks $h 'prompt').Count) prompt marks"
  $before = @($h.panes | Where-Object active)[0].scroll_top
  $at = Get-RectCenter (Marks $h 'prompt')[9].rect
  Send-Click $w $at[0] $at[1]
  # The row's mark lasts a moment: caught as it shows, not after the window has settled
  $after = Wait-State { param($s) $s.landed -and @($s.panes | Where-Object active)[0].scroll_top -ne $before } -What 'the scrollback scrolled to the tenth prompt'
  (Get-Shot $w -Name 'shell').Dispose()
  "scrolled from $before to $(@($after.panes | Where-Object active)[0].scroll_top)"
  $first.Dispose()
} -Passed 'scroll strip passed'
