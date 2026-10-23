#Requires -Version 7
# Covers: lbterm-extras/src/questions.rs, wezterm-gui/src/termwindow/render/questions.rs, wezterm-gui/src/termwindow/mouseevent.rs, lbterm/perf/fake-reply.ps1
<#
The question colours (lbterm-extras/src/questions.rs, wezterm-gui/src/termwindow/render/questions.rs)
end to end, in a test window that looks like Lachlan's LBTerm (New-TestConfig) with
lbterm_summaries pointed at a folder of its own, <out>\summaries. Its pane runs fake-reply.ps1, a
stand-in for Claude Code's fullscreen renderer: one reply, the findings at its top and two
questions at its bottom, more than a screen of it, with mouse reporting on and the pane's
claude_session set. The folder holds that session's questions.json as Terminal's question worker
writes it: Q1 is about the rail's labels, Q2 about the old agents.

A window that hasn't got the keyboard runs no mouse binding (mouseevent.rs lets only the wheel
through), so F9 stands in for the binding: bound to LbtermQuestions 'Show' here, it's pressed
while a posted right press is down, which holds the colours as that press would in Lachlan's
window, where the press itself runs the binding.

1. At the bottom of the reply: the right press and F9 colour both questions and their options,
   and since what they're about is scrolled off the top, a marker for each at the top (held.png,
   against rest.png).
2. A drag with the button still down changes nothing.
3. Letting go takes it all away.
4. Held again, a move with no button down, as after letting go outside the window, takes it
   away too.
5. At the top of the reply (Home): the findings are coloured, and the markers are at the bottom,
   since the questions are below (top-held.png, against top.png).

Mouse and keys are posted messages, never real input.

  pwsh -File lbterm\perf\questions.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'questions' -Bin $Bin
$sums = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $sums | Out-Null

# Q1's and Q2's colours (lbterm_extras::questions::COLOURS)
# Shots are compared from under the window strip down: the colours are only ever in the pane, and
# the strip has notices of its own ("Config reloaded" when any session saves lbterm.lua, which the
# test window loads too)
$PaneTop = 36
$Q1 = 0x5AA9FF; $Q2 = 0x4DD38A

# A shot once two 400 ms apart match: what the pane's program draws, which the state file can't say
function Settled($w, [string]$name) { Wait-Still $w -IntervalMs 400 -Name $name }
# The question colours shown, or not, as the state file says, settled, then the picture of it
function Held($w, [bool]$shown, [string]$name) {
  Wait-Change { param($s) ($null -ne $s.questions) -eq $shown } -What $(if ($shown) { 'the question colours' } else { 'the colours gone' }) -Animating | Out-Null
  Get-Shot $w -Name $name
}
function Below($a, $b) { Get-DiffBox $a $b -Region (New-Rect 0 $PaneTop $a.Width ($a.Height - $PaneTop)) }

# Q1's and Q2's colours in the top and bottom quarters of a shot: chips and bars are drawn near
# full strength there, bands at a fifth to a third, so a match at 24 is a chip or a bar
function Colours($shot) {
  $q = [int]($shot.Height / 4)
  $near = { param($rgb, $y0, $y1) [LbtermE2E.Px]::CountNear($shot, (New-Rect 0 $y0 $shot.Width ($y1 - $y0)), $rgb, 24) }
  [pscustomobject]@{
    Q1Top = & $near $Q1 0 $q; Q2Top = & $near $Q2 0 $q
    Q1Bottom = & $near $Q1 ($shot.Height - $q) $shot.Height
    Q2Bottom = & $near $Q2 ($shot.Height - $q) $shot.Height
    Q1 = & $near $Q1 0 $shot.Height; Q2 = & $near $Q2 0 $shot.Height
  }
}

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

Invoke-Test {
  $w = Open-TestWindow -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-reply.ps1') -ArgumentList '-Sid', $sid) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = nil
config.initial_cols = 100
config.initial_rows = 24
config.cursor_blink_rate = 0
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.LbtermQuestions 'Show' })
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $pane = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $pane 'Question colours test' | Out-Null
  $cr = Get-ClientRect $w
  $x = [int]($cr.Right / 2); $y = [int]($cr.Bottom / 2)
  $hold = {
    Send-Mouse $w RDown $x $y -Keys R
    Start-Sleep -Milliseconds 100
    Send-Key $w F9
  }

  # 1. At the bottom: both questions coloured, a marker for each at the top
  $rest = Settled $w 'rest'
  & $hold
  $held = Held $w $true 'held'
  $d = Below $rest $held
  "held: $(Format-Box $d)"
  $c = Colours $held
  $r = Colours $rest
  if ($c.Q1 - $r.Q1 -lt 100 -or $c.Q2 - $r.Q2 -lt 100) { Add-Failure "the questions weren't coloured: Q1 $($r.Q1) -> $($c.Q1), Q2 $($r.Q2) -> $($c.Q2) pixels (held.png)" }
  if ($c.Q1Top - $r.Q1Top -lt 100 -or $c.Q2Top - $r.Q2Top -lt 100) { Add-Failure "no marker at the top for what's above: Q1 $($c.Q1Top), Q2 $($c.Q2Top) (held.png)" }
  if ($c.Q2Bottom - $r.Q2Bottom -lt 30) { Add-Failure "Q2's bar isn't at the bottom, where Q2 is: $($c.Q2Bottom)" }

  # 2. A drag with the button down: nothing changes
  Send-Mouse $w Move ($x + 40) ($y + 20) -Keys R
  $dragged = Settled $w
  $m = Below $held $dragged
  if ($m) { Add-Failure "a drag with the button held changed the screen: $(Format-Box $m)" }

  # 3. Let go: all of it goes
  Send-Mouse $w RUp ($x + 40) ($y + 20)
  $gone = Held $w $false 'let-go'
  $m = Below $rest $gone
  if ($m) { Add-Failure "letting go left something: $(Format-Box $m) (let-go.png)" }

  # 4. Held again, then a move with no button down
  & $hold
  $again = Held $w $true $null
  if (-not (Below $rest $again)) { Add-Failure 'held a second time, nothing showed' }
  Send-Mouse $w Move $x $y
  $moved = Held $w $false 'moved-up'
  $m = Below $rest $moved
  if ($m) { Add-Failure "a move with the button up left something: $(Format-Box $m) (moved-up.png)" }

  # 5. At the top: the findings coloured, the markers at the bottom
  Send-Key $w Home
  Start-Sleep -Milliseconds 500
  $top = Settled $w 'top'
  & $hold
  $topHeld = Held $w $true 'top-held'
  $c = Colours $topHeld
  $r = Colours $top
  if ($c.Q1 - $r.Q1 -lt 100 -or $c.Q2 - $r.Q2 -lt 100) { Add-Failure "the findings weren't coloured: Q1 $($c.Q1), Q2 $($c.Q2) (top-held.png)" }
  if ($c.Q1Bottom - $r.Q1Bottom -lt 100 -or $c.Q2Bottom - $r.Q2Bottom -lt 100) { Add-Failure "no marker at the bottom for the questions below: Q1 $($c.Q1Bottom), Q2 $($c.Q2Bottom) (top-held.png)" }
  Send-Mouse $w RUp $x $y

  foreach ($s in $rest, $held, $dragged, $gone, $again, $moved, $top, $topHeld) { $s.Dispose() }
} -Passed 'question colours passed'
