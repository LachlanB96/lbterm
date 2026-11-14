#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/projects.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/context.rs, wezterm-gui/src/termwindow/render/objectives.rs, wezterm-gui/src/termwindow/render/hover_fade.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/termwindow/state.rs, wezterm-gui/src/tabbar.rs, lbterm-extras/src/projects.rs, lbterm-extras/src/state.rs, config/src/lbterm.rs
<#
The projects pad on the window strip's second row (lbterm/OBJECTIVES.md, "The projects card";
Lachlan's 1 of 2026-11-08), with the objectives strip off and the context bar in its room, as his
config will have it: a test window looking like his LBTerm (New-TestConfig), with folders of its own
for the summaries, the prompt history, the transcripts and two projects' InMail, `lbterm_projects`
listing those two projects (Alpha with a message waiting, Beta) with a rest of 1500 ms, and two
tabs: A, a stand-in Claude Code session in Alpha's folder with its `<session>.usage.json`, and B, a
plain shell.

1. Tab A: the context pill under the InMail button, and the pad right of it on the same row, half a
   column between them, as tall as the pill; the strip's items name it; no objectives strip
   (pad.png).
2. Resting on the pad: it lights at once (`strip.lit`), its wait (`projects.open`) runs out its
   1500 ms first, then the card opens
   under the strip, its left edge at the pad's or the panes', with Alpha's row first (a session,
   its message), then Beta's; the pad lit, its squares in the title's white (open.png).
3. Leaving: the card closes after its grace (`projects.close`), and the pad goes back to its grey.
4. A left press on the pad opens it at once, without waiting out the rest; leaving closes it.
5. Tab B, with no Claude session: the pill goes and its room stays empty, the pad where it was in tab A
   (Lachlan's 6a of 2026-11-09: nothing moves as he switches tabs; shell.png).

Where everything is comes from the window's state file (lbterm/STATE.md). The pointer is posted
messages, never real input.

  pwsh -File lbterm\perf\projects.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'projects' -Bin $Bin
$sums = Join-Path $T.Out 'summaries'
$transcripts = Join-Path $T.Out 'transcripts'
$history = Join-Path $T.Out 'history.jsonl'
$alpha = Join-Path $T.Out 'projects\Alpha'
$beta = Join-Path $T.Out 'projects\Beta'
Remove-Item -Recurse -Force $sums, $transcripts, (Join-Path $T.Out 'projects') -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $sums, $transcripts, (Join-Path $beta 'InMail') | Out-Null
[IO.File]::WriteAllText($history, '')
New-InMailMessage -Folder (Join-Path $alpha 'InMail') -To Alpha -From Beta -Subject 'A message' -At (Get-Date).AddHours(-1) | Out-Null

$sid = '5d3c2b1a-0f9e-4d8c-b7a6-958473625140'
Write-SessionFiles -Summaries $sums -Sid $sid -Label 'In Alpha' -Cwd $alpha
[IO.File]::WriteAllText((Join-Path $sums "$sid.usage.json"), '{"usd":3.21,"window":200000}')
$restMs = 1500

# The pad's squares: the labels' grey at rest, the title's white lit (lbterm_panel::palette)
$grey = 0xA8A8BC
$white = 0xECECF4
function Measure-Pad($shot, $pad, [int]$rgb) {
  [LbtermE2E.Px]::CountNear($shot, (New-Rect $pad.x $pad.y $pad.w $pad.h), $rgb, 12)
}
function Get-StripItem($s, [string]$name) { @($s.strip.items | Where-Object name -eq $name)[0] }

Invoke-Test {
  $w = Open-TestWindow -PostedPointer -Program (New-ClaudeSessionProgram -Sid $sid) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = '$(ConvertTo-LuaPath $history)'
config.lbterm_scroll_strip = nil
rawset(config, 'lbterm_objectives', nil)
rawset(config, 'lbterm_tab_panel', { enabled = true, open_delay_ms = 0 })
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath $transcripts)' })
rawset(config, 'lbterm_context_bar', {})
rawset(config, 'lbterm_inbox', { folders = { [[$alpha\InMail]], [[$beta\InMail]] } })
rawset(config, 'lbterm_projects', { open_delay_ms = $restMs, close_delay_ms = 200, projects = { { name = 'Alpha', dir = [[$alpha]] }, { name = 'Beta', dir = [[$beta]] } } })
config.initial_cols = 110
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  # 1. The pad right of the pill, on its row
  $s = Wait-Change { param($s) $s.strip.context -eq '$3.21' -and $s.projects.pad -and (Get-StripItem $s 'inbox') } -What 'the pill and the pad' -TimeoutSec 20
  $pill = (Get-StripItem $s 'context').rect
  $pad = $s.projects.pad
  $button = (Get-StripItem $s 'inbox').rect
  $padA = $pad.x
  "pill x $($pill.x)..$($pill.x + $pill.w) y $($pill.y)+$($pill.h); pad x $($pad.x)..$($pad.x + $pad.w) y $($pad.y)+$($pad.h); InMail x $($button.x)..$($button.x + $button.w)"
  if ($s.objectives) { Add-Failure 'the objectives strip is drawn with lbterm_objectives off' }
  if (-not $s.context_bar) { Add-Failure 'no context bar in the objectives strip''s room' }
  $item = (Get-StripItem $s 'projects').rect
  if (-not $item -or $item.x -ne $pad.x -or $item.w -ne $pad.w) { Add-Failure "the strip's items don't name the pad where the state says it is" }
  $between = $pad.x - ($pill.x + $pill.w)
  $cell = [Math]::Round(8 * $s.size.dpi / 96)
  if ($between -lt 1 -or $between -gt $cell) { Add-Failure "the pad is $between px right of the pill, not half a column" }
  if ([Math]::Abs($pad.y - $pill.y) -gt 1 -or [Math]::Abs($pad.h - $pill.h) -gt 1) { Add-Failure "the pad (y $($pad.y), $($pad.h) tall) isn't on the pill's row at its height (y $($pill.y), $($pill.h) tall)" }
  if ($pad.y -lt $button.y + $button.h) { Add-Failure "the pad isn't under the InMail button's row: its top $($pad.y), the button's bottom $($button.y + $button.h)" }
  if ($s.projects.open) { Add-Failure 'the card is open before the pointer came' }
  $rest = Get-Shot $w -Name 'pad'
  $restWhite = Measure-Pad $rest $pad $white
  $restGrey = Measure-Pad $rest $pad $grey
  $rest.Dispose()
  "at rest: $restGrey grey pixels and $restWhite white in the pad"
  if ($restGrey -lt 4) { Add-Failure "the pad's squares aren't drawn in their grey at rest ($restGrey pixels)" }

  # 2. Resting on it: its wait, then the card under the strip, the pad lit
  # The pointer's arrival paints a frame of its own (the pad lights, the wait starts): Wait-State from
  # the frame before it, not Wait-Change, which waits on past a frame with a wait under way for one
  # that's settled
  $c = Get-RectCenter $pad
  $f0 = Get-StateFrame $w
  $rested = [Diagnostics.Stopwatch]::StartNew()
  Set-PointerHold $w $c[0] $c[1]
  $s = Wait-State { param($s) @($s.waits | Where-Object name -eq 'projects.open').Count -or $s.projects.open } -What 'the rest before the card opens' -AfterFrame $f0 -TimeoutSec 5
  $wait = @($s.waits | Where-Object name -eq 'projects.open')[0]
  if ($s.projects.open) { Add-Failure "the card opened with no rest seen in the state file" } else { "resting: projects.open with $($wait.left_ms) ms left" }
  if ($s.pointer.over -ne 'projects.pad') { Add-Failure "pointer.over says '$($s.pointer.over)' on the pad" }
  if (@($s.strip.lit) -notcontains 'projects') { Add-Failure "the pad doesn't light as the pointer comes (strip.lit: $(@($s.strip.lit) -join ', '))" }
  $s = Wait-Change { param($s) $s.projects.open -and @($s.projects.rows).Count -eq 2 } -What 'the card open' -Animating -TimeoutSec 10
  $took = $rested.ElapsedMilliseconds
  if ($took -lt $restMs - 150) { Add-Failure "the card opened $took ms after the pointer came, before its rest of $restMs ms was out" } else { "the card opened $took ms after the pointer came (its rest $restMs)" }
  $card = $s.projects.card
  $rows = @($s.projects.rows)
  "card at $($card.x),$($card.y) $($card.w)x$($card.h): $(($rows | ForEach-Object { "$($_.name) $($_.idle + $_.working + $_.waiting) sessions $($_.mail) mail" }) -join ' | ')"
  if ($rows[0].name -ne 'Alpha' -or ($rows[0].idle + $rows[0].working + $rows[0].waiting) -ne 1 -or $rows[0].mail -ne 1) { Add-Failure "the first row isn't Alpha with its session and its message" }
  if ($rows[1].name -ne 'Beta') { Add-Failure "the second row isn't Beta" }
  $top = $s.strip.rect.y + $s.strip.rect.h
  if ($card.y -lt $pad.y + $pad.h -or ($s.strip.rect -and $card.y -lt $top)) { Add-Failure "the card doesn't hang under the strip: its top $($card.y), the pad's bottom $($pad.y + $pad.h), the strip's $top" }
  # Its left edge the pad's, or past the closed rail, which is drawn over it: never left of either,
  # nor past the panes' cards
  $panesLeft = ($s.panes | ForEach-Object { $_.rect.x } | Measure-Object -Minimum).Minimum
  $railRight = $s.rail.rect.x + $s.rail.rect.w
  if ($card.x -lt $pad.x - 1 -or $card.x -lt $railRight -or $card.x -gt $panesLeft) { Add-Failure "the card's left edge is at $($card.x): the pad's at $($pad.x), the closed rail ends at $railRight, the panes start at $panesLeft" }
  else { "card's left $($card.x): pad $($pad.x), rail's end $railRight, panes $panesLeft" }
  $open = Get-Shot $w -Name 'open'
  $openWhite = Measure-Pad $open $pad $white
  $open.Dispose()
  "open: $openWhite white pixels in the pad"
  if ($openWhite -le $restWhite + 3) { Add-Failure "the pad isn't lit while its card is open ($openWhite white pixels, $restWhite at rest)" }

  # 3. Away: the grace, then shut, the pad grey again
  Clear-PointerHold
  $f0 = Get-StateFrame $w
  Send-Mouse $w Move 300 300
  $s = Wait-State { param($s) @($s.waits | Where-Object name -eq 'projects.close').Count -or -not $s.projects.open } -What 'the grace before the card closes' -AfterFrame $f0 -TimeoutSec 5
  if (-not $s.projects.open) { Add-Failure 'the card closed with no grace seen in the state file' } else { 'closing: projects.close' }
  $s = Wait-Change { param($s) -not $s.projects.open -and -not $s.projects.card } -What 'the card closed' -Animating -TimeoutSec 10
  Wait-Settled -What 'the card faded' | Out-Null

  # 4. A press opens it at once, without the rest
  $f0 = Get-StateFrame $w
  $t0 = [Diagnostics.Stopwatch]::StartNew()
  Send-Click $w $c[0] $c[1] -Move
  try {
    $s = Wait-Change { param($s) $s.projects.open } -What 'the card open from a press' -AfterFrame $f0 -TimeoutSec (($restMs - 500) / 1000)
    "a press opened it in $($t0.ElapsedMilliseconds) ms (the rest is $restMs)"
  } catch { Add-Failure "a press on the pad didn't open the card before its rest was out: $_" }
  Send-Mouse $w Move 300 300
  $s = Wait-Change { param($s) -not $s.projects.open } -What 'the card closed again' -Animating -TimeoutSec 10

  # 5. A plain tab: the pill goes, its room kept empty, the pad where it was (6a)
  $paneA = (Get-Panes)[0].pane_id
  New-Tab -From $paneA -Program (New-PwshProgram 'Start-Sleep 600') | Out-Null
  $s = Wait-Change { param($s) -not $s.strip.context -and $s.projects.pad -and $s.active_tab -eq 1 } -What 'the plain tab, no pill' -TimeoutSec 10
  $pad = $s.projects.pad
  $button = (Get-StripItem $s 'inbox').rect
  "plain tab: pad x $($pad.x)..$($pad.x + $pad.w); InMail x $($button.x)..$($button.x + $button.w)"
  if ([Math]::Abs($pad.x - $padA) -gt 1) { Add-Failure "the pad moved with the pill gone: at x $($pad.x) in the plain tab, $padA in the Claude tab (Lachlan's 6a keeps the pill's room empty)" }
  (Get-Shot $w -Name 'shell').Dispose()
} -Passed 'projects pad passed'
