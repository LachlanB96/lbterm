#Requires -Version 7
# Covers: lbterm-extras/src/questions/weights.rs, wezterm-gui/src/termwindow/render/triggers.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/questions.rs, lbterm/perf/fake-reply.ps1
<#
The weights (lbterm/QUESTIONS.md, "The weights"): an option's share of 100 drawn in its colour in
a Claude pane, red at 0 through amber at 50 to green at 100, all the time. A test window that
looks like Lachlan's LBTerm (New-TestConfig) runs fake-reply.ps1 -Reply weights in its pane, a
stand-in for Claude Code's fullscreen renderer showing three questions whose options carry
shares of 0, 30, 50, 70 and 100 (2a's line wrapped, its share on the row under its label), an
extra and a sentence each with a bracketed number that isn't a share, and sets the pane's
claude_session user var as claudehook.exe does. lbterm_summaries is a folder of its own holding
that session's questions.json, so a hold colours the three questions, the extra, their options
and the sentence (Q2's passage); the transcripts' folder is an empty one of its own.

A window that hasn't got the keyboard runs no mouse binding (PITFALLS 357), so F9 stands in for
it, bound to LbtermQuestions 'Show' and pressed while a posted right press is down, as in
perf\questions.ps1.

1. The reply, settled, is captured at rest (rest.png): the shares coloured and nothing else.
2. Held, settled: the question colours' bands over the shares, the questions pane out (held.png).
   Both captures come before anything is judged, so a build without the colours still gives
   lbterm.ps1 look its halves: how they look is Lachlan's to judge by eye.
3. Let go, the bands go.
4. The state file says which shares the frame drew, at which rows and columns: the seven
   options', the wrapped one's on its second row, and not the extra's or the sentence's.
5. A second window with `lbterm_question_weights = false` (Lachlan's 10+ of 2026-11-14), the same
   reply: the state file says its frame drew no share.

Mouse and keys are posted messages, never real input.
  pwsh -File lbterm\perf\weights.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'weights' -Bin $Bin
$sid = '6f1d2c84-3a5e-4b7d-9c20-e8a14b53d7f1'
$sums = Join-Path $T.Out 'summaries'
$projects = Join-Path $T.Out 'projects'
New-Item -ItemType Directory -Force $sums, $projects | Out-Null

# The reply's questions as Terminal's question worker writes them: the extra too, and Q2 about
# the sentence with the bracketed number
[IO.File]::WriteAllText((Join-Path $sums "$sid.questions.json"), (@{
  sessionId = $sid
  messages = @(@{ id = 'reply'; at = 1; questions = @(
    @{ label = 'Q1'; number = 1; text = 'Q1. Where should the counts go?'; pos = 90 }
    @{ label = 'Q2'; number = 2; text = 'Q2. Which build does the check run on?'; pos = 260
       about = @(@{ text = 'The last check took (40) minutes'; pos = 45 }) }
    @{ label = 'Q3'; number = 3; text = 'Q3. Keep the old captures?'; pos = 520 }
    @{ label = 'E4'; number = 4; text = 'E4. Colour the brackets too (60): an extra has no share, so this stays as it is'; pos = 640 }
  ) })
} | ConvertTo-Json -Depth 8))

$Lua = @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath $projects)' }
config.lbterm_prompt_history = nil
config.initial_cols = 100
config.initial_rows = 24
config.cursor_blink_rate = 0
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.LbtermQuestions 'Show' })
"@
$Reply = { New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-reply.ps1') -ArgumentList '-Sid', $sid, '-Reply', 'weights' }

Invoke-Test {
  $w = Open-TestWindow -Program (& $Reply) -Lua $Lua
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and $s.panes[0].session -eq $sid } -What 'the window, its pane with its session' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $pane 'Weights test' | Out-Null
  $cr = Get-ClientRect $w
  $x = [int]($cr.Right / 3); $y = [int]($cr.Bottom / 2)

  # 1. At rest: the shares coloured, nothing else
  $rest = Wait-Still $w -IntervalMs 400 -Name 'rest'

  # 2. Held: the bands over the shares, settled once the pane is out
  Send-Mouse $w RDown $x $y -Keys R
  Start-Sleep -Milliseconds 100
  Send-Key $w F9
  Wait-Change { param($s) $null -ne $s.questions } -What 'the question colours' -Animating | Out-Null
  $held = Wait-Still $w -IntervalMs 400 -Name 'held'

  # 3. Let go: the bands go
  Send-Mouse $w RUp $x $y
  Wait-Change { param($s) $null -eq $s.questions -and $null -eq $s.questions_pane } -What 'the colours gone' -Animating | Out-Null

  # 4. What the frame drew, as the state file says: each option's share, its row from the pane's
  # top (the reply starts at the top: it's shorter than the screen) and its digits' first column.
  # Neither the sentence's (40) on row 0 nor the extra's (60) is one
  $want = @(
    @{ row = 3; col = 21; share = 70 }   # 1a. Monthly JSONL (70)
    @{ row = 4; col = 30; share = 30 }   # 1b. One file for all of it (30)
    @{ row = 5; col = 31; share = 0 }    # 1c. A table in the database (0)
    @{ row = 9; col = 21; share = 50 }   # 2a's line, wrapped: its tests with it (50)
    @{ row = 10; col = 31; share = 50 }  # 2b. A fresh build each time (50)
    @{ row = 13; col = 11; share = 100 } # 3a. Yes (100)
    @{ row = 14; col = 10; share = 0 }   # 3b. No (0)
  )
  $st = Wait-State { param($s) @($s.panes[0].weights).Count -ge $want.Count } -What 'the shares drawn' -TimeoutSec 5
  $got = @($st.panes[0].weights)
  $said = ($got | ForEach-Object { "$($_.share)@$($_.row),$($_.col)" }) -join ' '
  if ($got.Count -ne $want.Count) { Add-Failure "drew $($got.Count) shares, not $($want.Count): $said" }
  for ($i = 0; $i -lt [Math]::Min($got.Count, $want.Count); $i++) {
    $g = $got[$i]; $x = $want[$i]
    if ($g.row -ne $x.row -or $g.col -ne $x.col -or $g.share -ne $x.share) {
      Add-Failure "share $($i + 1) is $($g.share) at row $($g.row), column $($g.col), not $($x.share) at $($x.row), $($x.col): $said"
    }
  }
  $rest.Dispose(); $held.Dispose()

  # 5. The switch off: the same reply, no share drawn
  Stop-TestWindow $w
  $off = Open-TestWindow -Program (& $Reply) -Lua $Lua -Set 'lbterm_question_weights=false'
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and $s.panes[0].session -eq $sid } -Window $off -What 'the window with the weights off, its pane with its session' | Out-Null
  $still = Wait-Still $off -IntervalMs 400
  $still.Dispose()
  $st = Wait-State { param($s) $s.panes.Count -eq 1 } -Window $off -What 'the state, the reply drawn'
  # The state file leaves `weights` out when there are none, and @($null) counts 1: count only what's there
  $drew = if ($st.panes[0].weights) { @($st.panes[0].weights).Count } else { 0 }
  if ($drew -ne 0) { Add-Failure "with lbterm_question_weights off the frame drew $drew shares: $($st.panes[0].weights | ConvertTo-Json -Compress)" }
} -Passed 'weights passed'
