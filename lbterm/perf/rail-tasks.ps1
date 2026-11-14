#Requires -Version 7
# Variant: -NoCompositor
# Covers: lbterm-extras/src/tasks.rs, lbterm-extras/src/summaries.rs, wezterm-gui/src/termwindow/render/tasks_card.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/render/objectives.rs
<#
The card of what a tab's Claude session has running (lbterm/TAB-PANEL.md, "What's running"), end to
end. A test window with lbterm_summaries pointed at a scratch folder (<out>\summaries) gets two
tabs. The first stands in for a Claude session that has handed the turn back with three tasks still
going: it says its session (the claude_session user var) and prints the OSC 7501 records Claude Code
and Terminal's statusline send (done, then working under bg/<id> for each task), and the folder has
the <sid>.tasks.json the statusline would have written: a subagent with its own transcript (a made-up
one in <out>\projects, three tool calls in), a background command and a Monitor. The second tab is
plain.

1. The pointer rests on the first row: the panel opens, then the card drops beside the row
   (rail-tasks.png). It must list the three tasks with their ages, the subagent's latest tool call,
   its tool calls and its tokens under it, and the card must be on the capture: its words, on a
   3 px grid over its rect, far from the panes' colour beside it (a card whose z-layers another owner
   claims is in the state file and on no screen, lbterm/PITFALLS.md 300).
2. The subagent's transcript grows by a tool call while the card is open: the card must say so
   within a few seconds, with nothing done to the window, and keep its width though the line got
   shorter (rail-tasks-later.png).
3. The pointer goes over to the card: it stays, and so does the panel.
4. Onto the second row, which has nothing running: the card goes. Back to the first: it comes back.
5. Off the window: it goes with the panel.

-NoCompositor runs it all without DWM: the card comes and goes at once, drawn over the panes in the
window's own frame. Its USERPROFILE is a scratch folder. Fails with exit 1.

  pwsh -File lbterm\perf\rail-tasks.ps1 [-NoCompositor]
#>
param([string]$Bin, [switch]$NoCompositor)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('rail-tasks' + $(if ($NoCompositor) { '-nocompositor' } else { '' })) -Bin $Bin
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
$sid = '44444444-4444-4444-8444-444444444444'
$subagents = Join-Path $T.Out "projects\$sid\subagents"
New-Item -ItemType Directory -Force $summaries, $home2, $subagents | Out-Null

# The made-up subagent's transcript, as Claude Code writes one: a record per assistant message part
$transcript = Join-Path $subagents 'agent-a1b2c3d4e5f6a7b8c.jsonl'
function Tool-Call([string]$Name, [hashtable]$ToolInput, [int]$Read) {
  @{
    type = 'assistant'; isSidechain = $true
    message = @{
      role = 'assistant'
      content = @(@{ type = 'tool_use'; id = 'toolu_x'; name = $Name; input = $ToolInput })
      usage = @{ input_tokens = 2; cache_creation_input_tokens = 400; cache_read_input_tokens = $Read; output_tokens = 180 }
    }
  } | ConvertTo-Json -Compress -Depth 8
}
$first = @(
  (@{ type = 'user'; message = @{ role = 'user'; content = 'Review the card' } } | ConvertTo-Json -Compress -Depth 5)
  (Tool-Call 'Read' @{ file_path = 'C:\src\lbterm\TAB-PANEL.md' } 30000)
  (Tool-Call 'Grep' @{ pattern = 'owns_zindex' } 41000)
  (Tool-Call 'Edit' @{ file_path = 'C:\src\render\tasks_card.rs' } 52000)
)
[IO.File]::WriteAllText($transcript, ($first -join "`n") + "`n")

# The list, as the statusline writes it (Terminal's rememberTasks)
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$tasks = @{
  sessionId = $sid
  tasks = @(
    @{ id = 'a1b2c3d4e5f6a7b8c'; kind = 'agent'; description = 'Review the tasks card'; agentType = 'Explore'; startedAt = $now - 12 * 60000; transcript = $transcript }
    @{ id = 'b9x8y7z6w'; kind = 'command'; description = 'Run the LBTerm check'; startedAt = $now - 3 * 60000 }
    @{ id = 'm5n4b3v2c'; kind = 'monitor'; description = 'Wait for the build queue'; startedAt = $now - 40000 }
  )
}
[IO.File]::WriteAllText((Join-Path $summaries "$sid.tasks.json"), ($tasks | ConvertTo-Json -Compress -Depth 5))

# The first tab: its session, then Claude done with the three still working
$records = @('state=done:app=claude') + @($tasks.tasks | ForEach-Object { "state=working:id=bg/$($_.id)" })
$seq = (Get-UserVarSequence claude_session $sid) + (($records | ForEach-Object { "$([char]27)]7501;$_$([char]27)\" }) -join '')
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($seq))
$claude = New-PwshProgram "[Console]::Write([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$b64'))); Write-Host 'claude'; Start-Sleep 600"
$plain = New-PwshProgram "Write-Host 'plain'; Start-Sleep 600"

Invoke-Test {
  # LBTERM_POSTED_POINTER: a posted move hovers (the window doesn't ask Windows to say when the
  # real pointer, elsewhere, leaves it), so the panel and the card open under the held pointer
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo `
    -Set "lbterm_compositor=$(if ($NoCompositor) { 'false' } else { 'true' })", 'lbterm_tab_panel={enabled=true,open_delay_ms=0}', "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'" `
    -Program $claude
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  Invoke-Cli spawn --window-id 0 -Program $plain | Out-Null
  Invoke-Cli activate-tab --pane-id 0 --tab-index 1 | Out-Null
  $s = Wait-Change { param($s) $r = @($s.rail.rows); $r.Count -eq 2 -and $r[0].waiting -eq 3 -and $s.active_tab -eq 1 } -What 'two rows, the first waiting on three tasks' -Animating
  $rows = @($s.rail.rows)
  $onFirst = Get-RectCenter $rows[0].pad
  $onSecond = Get-RectCenter $rows[1].pad

  # 1. Resting on the first row: the panel, then the card
  Set-PointerHold $w $onFirst[0] $onFirst[1]
  $s = Wait-Change { param($s) $s.rail.open -and $s.tasks -and $s.tasks.row -eq 0 } -What 'the card beside the first row' -Animating -TimeoutSec 20
  # The card on the screen, not only in the state file, which says what the window meant to show: with
  # the compositor on, a card whose z-layers another owner claims has its quads drawn into that
  # owner's visual and shows nowhere, while its words are all in the state file (lbterm/PITFALLS.md
  # 300 and 285, the cost pane's 80 to 82 of 2026-11-09). The card's fill is the panes' own colour
  # (its ring and words tell it apart), and the panes under it are empty here, so its words are what's
  # looked for: the points of a 3 px grid over its rect far from the panes' colour beside it, a few
  # hundred with the card's four rows drawn and none without it
  $shot = Get-Shot $w -Name 'rail-tasks'
  try {
    $r = $s.tasks.rect
    $outside = $shot.GetPixel([Math]::Min([int]($r.x + $r.w + 24), $shot.Width - 2), [int]($r.y + $r.h / 2))
    $points = 0; $apart = 0
    for ($y = [int]$r.y + 2; $y -lt [Math]::Min($r.y + $r.h - 2, $shot.Height); $y += 3) {
      for ($x = [int]$r.x + 2; $x -lt [Math]::Min($r.x + $r.w - 2, $shot.Width); $x += 3) {
        $p = $shot.GetPixel($x, $y); $points++
        if ([Math]::Abs($p.R - $outside.R) + [Math]::Abs($p.G - $outside.G) + [Math]::Abs($p.B - $outside.B) -gt 90) { $apart++ }
      }
    }
    "the card on the capture: $apart of $points points of its rect far from the panes' colour beside it ($outside)"
    if ($apart -lt [Math]::Max(20, $points / 100)) { Add-Failure "the card isn't drawn: $apart of $points points inside its rect $($r.x),$($r.y) $($r.w)x$($r.h) differ from the panes beside it (rail-tasks.png)" }
  } finally { $shot.Dispose() }
  $lines = @($s.tasks.lines)
  "the card says:"; $lines | ForEach-Object { "  $_" }
  $want = @(
    @('^Agent Review the tasks card 12m$', 'the subagent, its description and age'),
    @('^Edit tasks_card\.rs · 3 tools · 53k tokens$', "the subagent's latest tool call, its tool calls and tokens"),
    @('^Command Run the LBTerm check 3m$', 'the command'),
    @('^Monitor Wait for the build queue (\d+s|1m)$', 'the Monitor')
  )
  for ($i = 0; $i -lt $want.Count; $i++) {
    if ($lines[$i] -notmatch $want[$i][0]) { Add-Failure "line $($i + 1) isn't $($want[$i][1]): '$($lines[$i])'" }
  }
  $card = $s.tasks.rect
  $panel = $s.rail.rect
  if ($card.x -lt $panel.x + $panel.w) { Add-Failure "the card ($($card.x)) isn't clear of the open panel (to $($panel.x + $panel.w))" }

  # 2. The subagent does more: read while the card is open
  [IO.File]::AppendAllText($transcript, (Tool-Call 'Write' @{ file_path = 'C:\src\notes.md' } 61000) + "`n")
  $s = Wait-State { param($s) $s.tasks -and @($s.tasks.lines)[1] -like 'Write notes.md*' } -What "the card saying the subagent's new tool call" -TimeoutSec 10
  "after the transcript grew: $(@($s.tasks.lines)[1])"
  if (@($s.tasks.lines)[1] -notmatch '· 4 tools · 62k tokens$') { Add-Failure "the subagent's line didn't count the new call: '$(@($s.tasks.lines)[1])'" }
  # Its line got shorter, but the card keeps its width while it stays open
  if ($s.tasks.rect.w -lt $card.w) { Add-Failure "the card got narrower as the subagent's line did ($($card.w) to $($s.tasks.rect.w) px)" }
  Wait-Settled | Out-Null
  Get-Shot $w -Name 'rail-tasks-later' | ForEach-Object { $_.Dispose() }

  # 3. Over to the card: it stays, and the panel with it
  $onCard = Get-RectCenter $card
  Set-PointerHold $w $onCard[0] $onCard[1]
  Wait-Ms 600
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  if (-not $s.tasks) { Add-Failure 'the card closed when the pointer went over to it' }
  if (-not $s.rail.open) { Add-Failure 'the panel closed when the pointer went over to the card' }
  if ($s.pointer.over -ne 'rail.tasks') { Add-Failure "the pointer on the card is over '$($s.pointer.over)', not rail.tasks" }

  # 4. The second row has nothing running: the card goes; back on the first, it comes back
  Set-PointerHold $w $onSecond[0] $onSecond[1]
  Wait-Change { param($s) -not $s.tasks -and $s.rail.open } -What 'the card gone on the plain row' -Animating | Out-Null
  Set-PointerHold $w $onFirst[0] $onFirst[1]
  Wait-Change { param($s) $s.tasks -and $s.tasks.row -eq 0 } -What 'the card back on the first row' -Animating | Out-Null

  # 5. Off the window: the card goes with the panel
  Clear-PointerHold
  Send-Leave $w
  Wait-Change { param($s) -not $s.tasks -and -not $s.rail.open } -What 'the card and the panel closed' -Animating | Out-Null
} -Passed "rail-tasks: resting on a row lists its session's tasks beside it, a subagent's progress read live; the card stays for the pointer and goes with the row"
