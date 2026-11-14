#Requires -Version 7
# Covers: lbterm-extras/src/questions.rs, lbterm-extras/src/questions/pane.rs, lbterm-extras/src/drafts.rs, lbterm-extras/src/conversation.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/termwindow/render/questions.rs, wezterm-gui/src/termwindow/render/questions_pane.rs, wezterm-gui/src/termwindow/mouseevent.rs, lbterm/perf/fake-reply.ps1
<#
The question colours (lbterm-extras/src/questions.rs, wezterm-gui/src/termwindow/render/questions.rs)
end to end, in a test window that looks like Lachlan's LBTerm (New-TestConfig) with
lbterm_summaries pointed at a folder of its own, <out>\summaries. Its pane runs fake-reply.ps1, a
stand-in for Claude Code's fullscreen renderer: one reply, the findings at its top and two
questions at its bottom, more than a screen of it, with mouse reporting on and the pane's
claude_session set. The folder holds that session's questions.json as Terminal's question worker
writes it, with two replies: the one on screen, whose Q1 is about the rail's labels and Q2 about
the old agents, and an older one off the screen that asked two questions of its own. The
transcripts' folder is <out>\projects, holding a stand-in transcript of the session: a prompt in
words, the older reply, his "1+" (which answers the older reply's Q1, since it came after that
reply and before the one on screen), and the reply on screen; each reply's time in the file is 12
s after its last record, as the worker's is.

A window that hasn't got the keyboard runs no mouse binding (mouseevent.rs lets only the wheel
through), so F9 stands in for the binding: bound to LbtermQuestions 'Show' here, it's pressed
while a posted right press is down, which holds the colours as that press would in Lachlan's
window, where the press itself runs the binding.

1. At the bottom of the reply: the right press and F9 colour both questions and their options
   (held.png, against rest.png). The questions bar lies down the window's right, the closed rail
   mirrored, as the state file says: the closed rail's width a gutter in from the window's edge,
   from under the tab bar to the window's foot; the reply on screen's Q1 and Q2 in their colours,
   neither answered, then the older reply's Q1, ticked, and Q2, open, nothing more. The older
   reply's two have arrows up (it's above the reply on screen), the reply on screen's none, and
   the "Q1 ↑" markers at the top, which the arrows stand for while the bar is out, aren't drawn
   (Lachlan's E15 of 2026-11-14). held.png is taken once the transcript's been read and the bar
   is out. The colours are counted left of it.
2. A drag with the button still down changes nothing.
3. Letting go takes it all away, the pane too: the state file has none, and once the slide back
   is done nothing differs from rest.png.
4. Held again, a move with no button down, as after letting go outside the window, takes it
   away too.
5. At the top of the reply (Home): the findings are coloured; the reply on screen's Q1 and Q2
   have arrows down, since their words are below, the older reply's still up, and no marker is
   drawn at the bottom (top-held.png, against top.png).
6. None of the replies on screen (FLOWS.md, "A right hold, to the question colours", What could
   go 5, the case the pane is for): the questions file is renamed over with the two replies'
   texts and passages changed to words that aren't on the screen, and a newer reply added. Held,
   the colours find nothing (no question colour left of the bar, against top.png), and the bar
   still lists all three replies' rows, the newer reply's Q1 first and the older one's Q1 still
   ticked, with no arrows: with nothing of any reply on the screen, which way isn't known
   (none-held.png).

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
# The bar's rows' arrows, top to bottom, '-' for none (STATE.md, `questions_pane`)
function Arrows($s) { (@($s.questions_pane.rows) | ForEach-Object { if ($_.arrow) { $_.arrow } else { '-' } }) -join ' ' }

# Q1's and Q2's colours in the top and bottom quarters of a shot: chips and bars are drawn near
# full strength there, bands at a fifth to a third, so a match at 24 is a chip or a bar
# Left of the questions pane, whose chips are in the same colours, when it's out ($right)
function Colours($shot, [int]$right = $shot.Width) {
  $q = [int]($shot.Height / 4)
  $near = { param($rgb, $y0, $y1) [LbtermE2E.Px]::CountNear($shot, (New-Rect 0 $y0 $right ($y1 - $y0)), $rgb, 24) }
  [pscustomobject]@{
    Q1Top = & $near $Q1 0 $q; Q2Top = & $near $Q2 0 $q
    Q1Bottom = & $near $Q1 ($shot.Height - $q) $shot.Height
    Q2Bottom = & $near $Q2 ($shot.Height - $q) $shot.Height
    Q1 = & $near $Q1 0 $shot.Height; Q2 = & $near $Q2 0 $shot.Height
  }
}

$sid = '3c8e5a17-9b2d-4f04-a6e1-7d5c0b9f2e48'
# The session ten minutes ago: a prompt, the older reply, his "1+", the reply on screen
$t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - 600000
$older = "Two things to settle about the slot first.`n`nQ1. Keep the slot's build between runs?`n`nQ2. Run the smoke set after every stage?"
$onScreen = "Here's what I found in the fleet and the rail.`n`nQ1. Should the rail show labels for try windows too?`n`nQ2. Remove the two old background agents with ``claude rm``?"
[IO.File]::WriteAllText((Join-Path $sums "$sid.questions.json"), (@{
  sessionId = $sid
  messages = @(
    @{ id = 'older'; at = $t0 + 15000; questions = @(
      @{ label = 'Q1'; number = 1; text = 'Q1. Keep the slot''s build between runs?'; pos = 45 }
      @{ label = 'Q2'; number = 2; text = 'Q2. Run the smoke set after every stage?'; pos = 87 }
    ) }
    @{ id = 'reply'; at = $t0 + 45000; questions = @(
      @{ label = 'Q1'; number = 1; text = 'Q1. Should the rail show labels for try windows too?'; pos = 200
         about = @(@{ text = 'The tab rail''s labels come from `lbterm_summaries`, read on a thread of its own'; pos = 40 }) }
      @{ label = 'Q2'; number = 2; text = 'Q2. Remove the two old background agents with `claude rm`?'; pos = 220
         about = @(@{ text = 'The fleet still lists two old background agents that aren''t running'; pos = 10 }) }
    ) }
  )
} | ConvertTo-Json -Depth 8))

# The transcript, as Claude Code writes it: one record a line, compact, so the reader's
# '"type":"user"' finds each (lbterm-extras/src/conversation.rs, WANTED)
$projects = Join-Path $T.Out 'projects'
$folder = Join-Path $projects 'C--work-demo'
New-Item -ItemType Directory -Force $folder | Out-Null
$records = [Collections.Generic.List[string]]::new()
function Add-Record([string]$type, $message, [long]$ms, [switch]$Human) {
  $r = [ordered]@{
    parentUuid = $null; isSidechain = $false; userType = 'external'; cwd = 'C:\work\demo'; sessionId = $sid
    message = $message; type = $type; uuid = '00000000-0000-4000-8000-{0:D12}' -f ($records.Count + 1)
    timestamp = [DateTimeOffset]::FromUnixTimeMilliseconds($ms).UtcDateTime.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", [Globalization.CultureInfo]::InvariantCulture)
  }
  if ($Human) { $r.origin = @{ kind = 'human' } }
  $records.Add(($r | ConvertTo-Json -Compress -Depth 8))
}
function Add-Prompt([string]$text, [long]$ms) { Add-Record user @{ role = 'user'; content = $text } $ms -Human }
function Add-Says([string]$text, [long]$ms) { Add-Record assistant @{ role = 'assistant'; content = @(@{ type = 'text'; text = $text }) } $ms }
Add-Prompt 'Look over the slot, then the fleet and the rail' $t0
Add-Says $older ($t0 + 3000)
Add-Prompt '1+' ($t0 + 30000)
Add-Says $onScreen ($t0 + 33000)
[IO.File]::WriteAllText((Join-Path $folder "$sid.jsonl"), (($records -join "`n") + "`n"))

Invoke-Test {
  $w = Open-TestWindow -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-reply.ps1') -ArgumentList '-Sid', $sid) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath $projects)' }
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
  # The questions pane, as the state file says it drew it, once the transcript's been read (the
  # reading thread follows the session from the hold's start) and its slide is done. A build
  # without the pane fails here and still gives held.png, for lbterm.ps1 look
  $st = $null
  try {
    # questions is the held pane's id, 0 in a test window: tested against $null, never as a truth
    $st = Wait-Change { param($s) $null -ne $s.questions -and $s.questions_pane -and @($s.questions_pane.rows | Where-Object answered).Count -ge 1 } -What 'the questions pane, a row ticked' -Animating
  } catch { Add-Failure "no questions pane with a row ticked: $($_.Exception.Message)" }
  $held = Held $w $true $null
  $held.Dispose()
  $held = Settled $w 'held'
  $d = Below $rest $held
  "held: $(Format-Box $d)"
  if (-not $st) { $st = Wait-State { param($s) $s.questions_pane -and $s.questions_pane.rows.Count -ge 1 } -What 'the questions pane' }
  $qp = $st.questions_pane
  "pane: $($qp.rect.x),$($qp.rect.y) $($qp.rect.w)x$($qp.rect.h): $(($qp.rows | ForEach-Object { "$($_.label)$(if ($_.answered) { ' ticked' } else { ' open' })" }) -join ', ')$(if ($qp.more) { ", +$($qp.more)" })"
  $labels = ($qp.rows | ForEach-Object { "$($_.label)/$($_.colour)/$($_.answered)/$($_.age)" }) -join ' '
  # The reply on screen first, neither answered; then the older one, its Q1 answered by "1+"
  if ($labels -ne 'Q1/0/False/0 Q2/1/False/0 Q1/0/True/1 Q2/1/False/1' -or $qp.more -ne 0) { Add-Failure "the pane should list the reply on screen's Q1 and Q2, open, then the older reply's Q1, ticked, and Q2, open, nothing more: $labels, more $($qp.more)" }
  $want = [int][math]::Round(68 * $st.size.dpi / 96)
  if ([math]::Abs($qp.rect.w - $want) -gt 1) { Add-Failure "the bar is $($qp.rect.w) px wide, not the closed rail's $want" }
  if ($qp.rect.x + $qp.rect.w -gt $cr.Right -or $qp.rect.x + $qp.rect.w -lt $cr.Right - 12) { Add-Failure "the bar isn't at the window's right: it ends at $($qp.rect.x + $qp.rect.w) of $($cr.Right)" }
  # The closed rail's height: from under the tab bar to a gutter above the window's foot
  if ($qp.rect.y + $qp.rect.h -lt $cr.Bottom - 16 -or $qp.rect.h -lt $cr.Bottom / 2) { Add-Failure "the bar isn't the rail's height: $($qp.rect.y) to $($qp.rect.y + $qp.rect.h) of $($cr.Bottom)" }
  if ((Arrows $st) -ne '- - up up') { Add-Failure "the older reply's rows should point up and the reply on screen's not at all: $(Arrows $st)" }
  $c = Colours $held $qp.rect.x
  $r = Colours $rest
  if ($c.Q1 - $r.Q1 -lt 100 -or $c.Q2 - $r.Q2 -lt 100) { Add-Failure "the questions weren't coloured: Q1 $($r.Q1) -> $($c.Q1), Q2 $($r.Q2) -> $($c.Q2) pixels (held.png)" }
  if ($c.Q1Top - $r.Q1Top -ge 30 -or $c.Q2Top - $r.Q2Top -ge 30) { Add-Failure "a marker at the top while the bar is out: Q1 $($c.Q1Top), Q2 $($c.Q2Top) (held.png)" }
  if ($c.Q2Bottom - $r.Q2Bottom -lt 30) { Add-Failure "Q2's bar isn't at the bottom, where Q2 is: $($c.Q2Bottom)" }

  # 2. A drag with the button down: nothing changes
  Send-Mouse $w Move ($x + 40) ($y + 20) -Keys R
  $dragged = Settled $w
  $m = Below $held $dragged
  if ($m) { Add-Failure "a drag with the button held changed the screen: $(Format-Box $m)" }

  # 3. Let go: all of it goes
  Send-Mouse $w RUp ($x + 40) ($y + 20)
  $after = Wait-Change { param($s) $null -eq $s.questions } -What 'the colours gone' -Animating
  if ($after.questions_pane) { Add-Failure "let go, the state file still has the questions pane: $($after.questions_pane | ConvertTo-Json -Compress -Depth 4)" }
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

  # 5. At the top: the findings coloured, the reply on screen's arrows down, no marker
  Send-Key $w Home
  Start-Sleep -Milliseconds 500
  $top = Settled $w 'top'
  & $hold
  $topHeld = Held $w $true 'top-held'
  $st5 = $null
  try {
    $st5 = Wait-State { param($s) $s.questions_pane -and (Arrows $s) -eq 'down down up up' } -What 'the bar, the reply on screen''s arrows down' -TimeoutSec 5
  } catch { Add-Failure "at the top the reply on screen's rows should point down and the older reply's up: $(Arrows (Wait-State { param($s) $s.questions_pane } -What 'the bar'))" }
  $c = Colours $topHeld ((Wait-State { param($s) $s.questions_pane } -What 'the questions bar').questions_pane.rect.x)
  $r = Colours $top
  if ($c.Q1 - $r.Q1 -lt 100 -or $c.Q2 - $r.Q2 -lt 100) { Add-Failure "the findings weren't coloured: Q1 $($c.Q1), Q2 $($c.Q2) (top-held.png)" }
  if ($c.Q1Bottom - $r.Q1Bottom -ge 30 -or $c.Q2Bottom - $r.Q2Bottom -ge 30) { Add-Failure "a marker at the bottom while the bar is out: Q1 $($c.Q1Bottom), Q2 $($c.Q2Bottom) (top-held.png)" }
  Send-Mouse $w RUp $x $y

  # 6. None of the replies on screen: the colours find nothing, and the pane still lists them
  $away = { param($label) "$label. Something asked in a reply that has scrolled away for good?" }
  $file = Join-Path $sums "$sid.questions.json"
  [IO.File]::WriteAllText("$file.tmp", (@{
    sessionId = $sid
    messages = @(
      @{ id = 'older'; at = $t0 + 15000; questions = @(
        @{ label = 'Q1'; number = 1; text = (& $away 'Q1'); pos = 45 }
        @{ label = 'Q2'; number = 2; text = (& $away 'Q2'); pos = 87 }
      ) }
      @{ id = 'reply'; at = $t0 + 45000; questions = @(
        @{ label = 'Q1'; number = 1; text = (& $away 'Q1'); pos = 200; about = @(@{ text = 'A passage no screen of this test ever shows'; pos = 40 }) }
        @{ label = 'Q2'; number = 2; text = (& $away 'Q2'); pos = 220 }
      ) }
      @{ id = 'newer'; at = $t0 + 50000; questions = @(
        @{ label = 'Q1'; number = 1; text = (& $away 'Q1'); pos = 10 }
      ) }
    )
  } | ConvertTo-Json -Depth 8))
  Move-Item -Force "$file.tmp" $file
  & $hold
  # The renamed file read (its newer reply's row) and the slide done; a build without the pane
  # fails here and still gives none-held.png
  $st = $null
  try {
    $st = Wait-Change { param($s) $null -ne $s.questions -and $s.questions_pane -and $s.questions_pane.rows.Count -eq 5 } -What 'the questions pane of replies none of which is on screen' -Animating
  } catch { Add-Failure "no questions pane listing the replies off the screen: $($_.Exception.Message)" }
  $noneHeld = Held $w $true 'none-held'
  $paneX = if ($st) { [int]$st.questions_pane.rect.x } else { $noneHeld.Width }
  if ($st) {
    $labels = ($st.questions_pane.rows | ForEach-Object { "$($_.label)/$($_.colour)/$($_.answered)/$($_.age)" }) -join ' '
    if ($labels -ne 'Q1/0/False/0 Q1/0/False/1 Q2/1/False/1 Q1/0/True/2 Q2/1/False/2') { Add-Failure "with no reply on screen the bar should list the newer reply's Q1, then the two before it as in step 1: $labels (none-held.png)" }
    if ((Arrows $st) -ne '- - - - -') { Add-Failure "with nothing of any reply on screen which way isn't known, so no arrows: $(Arrows $st)" }
  }
  $c = Colours $noneHeld $paneX
  $r = Colours $top $paneX
  if ($c.Q1 - $r.Q1 -ge 30 -or $c.Q2 - $r.Q2 -ge 30) { Add-Failure "with no reply on screen the colours found something: Q1 $($r.Q1) -> $($c.Q1), Q2 $($r.Q2) -> $($c.Q2) (none-held.png)" }
  Send-Mouse $w RUp $x $y

  foreach ($s in $rest, $held, $dragged, $gone, $again, $moved, $top, $topHeld, $noneHeld) { $s.Dispose() }
} -Passed 'question colours passed'
