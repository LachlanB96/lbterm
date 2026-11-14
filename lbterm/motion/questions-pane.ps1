#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/questions_pane.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/composited.rs, lbterm/perf/fake-reply.ps1
<#
The questions bar's slide and its push (lbterm/QUESTIONS.md, "The questions bar"), frame by frame:
it comes out from under the window's right edge on panel_open as a right hold starts, pushing the
tab rail, the panes and the strips left as far (Lachlan's 16 of 2026-11-14), and goes back under it
on panel_close as the hold ends, turned round from where it is by a release mid-slide.

Opens the build (-Bin, else target\release) with the compositor on and motion at -Slowdown, its
left pane running perf/fake-reply.ps1 (a Claude session with two questions, its questions file in
a summaries folder of its own, as perf/questions.ps1 sets it up) and a quiet pane split off to its
right, so nothing bright lies under where the card comes in. A test window runs no mouse binding
(lbterm/PITFALLS.md, 357), so F9 is bound to LbtermQuestions 'Show' and posted during a posted
right press, as perf/questions.ps1 does. One hold first finds the card's place in the state file;
then the recorded hold: the press and F9 at the start, the release at -ThenMs (before slowdown; 0
holds it to the end). Frames go to target\motion\<Name>, then sheet.py tiles 24 of them
(contact.png), to judge by eye: the rail going off the left edge, the conversation and the strips
moving left at their size, the bar coming in at the right, nothing apart that should meet. No
edge is fitted since the push: the strips move just left of the bar, so the first bright pixel on
a row is theirs for part of the slide and the bar's for the rest, which no one curve fits. The
curves themselves, the bar's and the push's from one commit, are held by the compositor test
`the_questions_bar_pushes_the_rail_and_the_conversation_left_as_far_as_it_comes_out`. -MotionOff
records it with motion off: there and gone at once.

  pwsh -File lbterm\motion\questions-pane.ps1
  pwsh -File lbterm\motion\questions-pane.ps1 -Name questions-pane-back -ThenMs 100
  pwsh -File lbterm\motion\questions-pane.ps1 -Name questions-pane-off -MotionOff -Slowdown 1

Mouse and keys are posted messages, never real input. Needs Python with Pillow and numpy.
#>
param(
  [string]$Name = 'questions-pane',
  [string]$Bin,
  [double]$Slowdown = 5,
  [int]$ThenMs = 0,
  [double]$Seconds = 2.5,
  [switch]$MotionOff
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $repo "target\motion\$Name"
$T = Start-Test "motion-$Name" -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.motion'
$sums = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $sums | Out-Null

# The session perf/questions.ps1 uses, with the same two questions
$sid = '3c8e5a17-9b2d-4f04-a6e1-7d5c0b9f2e48'
[IO.File]::WriteAllText((Join-Path $sums "$sid.questions.json"), (@{
  sessionId = $sid
  messages = @(@{ id = 'reply'; at = 1; questions = @(
    @{ label = 'Q1'; number = 1; text = 'Q1. Should the rail show labels for try windows too?'; pos = 200
       about = @(@{ text = 'The tab rail''s labels come from `lbterm_summaries`, read on a thread of its own'; pos = 40 }) }
    @{ label = 'Q2'; number = 2; text = 'Q2. Remove the two old background agents with `claude rm`?'; pos = 220
       about = @(@{ text = 'The fleet still lists two old background agents that aren''t running'; pos = 10 }) }
  ) })
} | ConvertTo-Json -Depth 8))

$motion = if ($MotionOff) { "lbterm_motion={enabled='off'}" } else { "lbterm_motion={enabled='on',slowdown=$Slowdown}" }
$scale = 1.0
try {
  $w = Open-TestWindow -Program (New-PwshProgram -File (Join-Path $PSScriptRoot '..\perf\fake-reply.ps1') -ArgumentList '-Sid', $sid) -Set @('lbterm_compositor=true', $motion) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = nil
config.initial_cols = 120
config.initial_rows = 26
config.cursor_blink_rate = 0
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.LbtermQuestions 'Show' })
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  # A quiet pane on the right, under where the card comes in
  Invoke-Cli split-pane --pane-id $pane --right --percent 30 -Program (New-PwshProgram -Script 'Start-Sleep 600') | Out-Null
  Wait-State { param($s) $s.panes.Count -eq 2 } -What 'the split' | Out-Null
  Wait-Change { param($s) -not $s.animating } -What 'the split to settle' | Out-Null
  $cr = Get-ClientRect $w
  $x = [int]($cr.Right * 0.3); $y = [int]($cr.Bottom / 2)

  # One hold to find the card's place, then let go and wait for it to go
  Send-Mouse $w RDown $x $y -Keys R
  Start-Sleep -Milliseconds 100
  Send-Key $w F9
  $st = Wait-State { param($s) $s.questions_pane -and $s.questions_pane.rows.Count -ge 1 } -What 'the questions pane'
  $rect = $st.questions_pane.rect
  if ($st.size.dpi) { $scale = $st.size.dpi / 96 }
  Send-Mouse $w RUp $x $y
  Wait-Change { param($s) $null -eq $s.questions_pane -and -not $s.animating } -What 'the pane gone' | Out-Null
  Start-Sleep -Milliseconds 300
  "pane at rest: $($rect.x),$($rect.y) $($rect.w)x$($rect.h), scale $scale"

  $done = @{ Press = $false; Release = $false }
  $frames = Get-Frames $w ([int]($Seconds * 1000)) -Raw -Each {
    param($ms)
    if (-not $done.Press) {
      Send-Mouse $w RDown $x $y -Keys R
      Send-Key $w F9
      $done.Press = $true
    }
    if ($ThenMs -gt 0 -and -not $done.Release -and $ms -ge $ThenMs * $Slowdown) {
      Send-Mouse $w RUp $x $y
      $done.Release = $true
    }
  }
  if (-not $done.Release) { Send-Mouse $w RUp $x $y }
  Save-Frames $frames -Dir $out -Window $w
  Remove-Frames $frames
  "$($frames.Count) frames in $Seconds s to $out"
} finally {
  Close-Test
}

# A contact sheet to judge by eye (see the header: no edge is fitted since the push)
python (Join-Path $PSScriptRoot 'sheet.py') $out --tiles --count 24 | Out-Null
"contact sheet: $(Join-Path $out 'contact.png')"
