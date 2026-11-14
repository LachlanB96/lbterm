#Requires -Version 7
# Covers: lbterm-extras/src/drafts.rs, lbterm-extras/src/control.rs, lbterm-extras/src/questions.rs, lbterm-extras/src/state.rs, wezterm-gui/src/drafts.rs, wezterm-gui/src/control.rs, wezterm-gui/src/termwindow/control.rs, wezterm-gui/src/termwindow/render/draft.rs, wezterm-gui/src/termwindow/render/questions.rs, wezterm/src/control.rs, config/src/lbterm.rs, lbterm/perf/fake-prompt.ps1
<#
Voice drafts end to end (lbterm/CONTROL.md, "Drafts"), through `lbterm control draft`, in a test
window that looks like Lachlan's LBTerm (New-TestConfig) with lbterm_summaries pointed at a folder
of its own. Its pane runs fake-prompt.ps1, a stand-in for Claude Code: a reply asking Q1 and Q2,
with the prompt, a permission prompt or a menu under it as the test says, and every key it's
sent written down. The folder holds the session's questions.json, as Terminal's question worker
writes it.

1. A piece added shows a card over the pane, above the input box, Q1 is ticked and Q2, the next
   to answer, ringed in red (draft-next.png); a second piece ticks Q2 and the ring goes
   (draft.png).
2. "Scratch that" takes the newest piece back, and its tick, and Q2 is ringed again.
3. With a permission prompt in the box's place, send is refused, nothing is typed, and the draft
   stays.
4. At the prompt, send types it all: Shift+Enter's ESC CR for each new line, then Enter; the card
   goes.
5. LbtermDraft 'Edit' (F9 here) types a draft without Enter.
6. With a menu in the box's place (fake-prompt's picker screen, AskUserQuestion's as Claude Code
   drew it), send is refused. Drafts made a picker's picks until 2026-11-14, when the picker's code
   went (Lachlan's 2a).
7. Another Claude session in the pane lets its draft go.

Keys are posted messages, never real input.

  pwsh -File lbterm\perf\draft.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'draft' -Bin $Bin
$sums = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $sums | Out-Null
$mode = Join-Path $T.Out 'mode.txt'
$keys = Join-Path $T.Out 'keys.txt'
[IO.File]::WriteAllText($mode, 'prompt')
[IO.File]::WriteAllText($keys, '')

$sid = '5b1f0c6e-2a47-4d8e-9c31-6e0a7f4b2d19'
[IO.File]::WriteAllText((Join-Path $sums "$sid.questions.json"), (@{
  sessionId = $sid
  messages = @(@{ id = 'reply'; at = 1; questions = @(
    @{ label = 'Q1'; number = 1; text = 'Q1. Should the rail show labels for try windows too?'; pos = 10; about = @() }
    @{ label = 'Q2'; number = 2; text = 'Q2. Remove the two old background agents with claude rm?'; pos = 30; about = @() }
  ) })
} | ConvertTo-Json -Depth 8))

# `lbterm control --class <its class> <args>`: its exit code, its reply and what it said on stderr
function Invoke-Control {
  [CmdletBinding(PositionalBinding = $false)]
  param([Parameter(ValueFromRemainingArguments)][string[]]$Rest)
  $errFile = Join-Path $T.Out 'control-stderr.txt'
  $out = @(& $T.Cli control --class $T.Class @Rest 2>$errFile)
  $code = $LASTEXITCODE
  $text = ($out | Out-String).Trim()
  [pscustomobject]@{
    Code  = $code
    Reply = $(if ($text) { $text | ConvertFrom-Json } else { $null })
    Error = ((Get-Content $errFile -Raw -ErrorAction SilentlyContinue) ?? '').Trim()
  }
}

# The keys the stand-in has written down from line $From on, as text: ESC CR or Alt+Enter as a
# new line, a lone CR as <enter>
function Get-Typed([int]$From = 0) {
  $lines = @(Get-Content -LiteralPath $keys -ErrorAction SilentlyContinue | Where-Object { $_ })
  $sb = [Text.StringBuilder]::new()
  for ($i = $From; $i -lt $lines.Count; $i++) {
    $l = $lines[$i]
    if ($l -eq '27' -and $i + 1 -lt $lines.Count -and $lines[$i + 1] -eq '13') { [void]$sb.Append("`n"); $i++; continue }
    if ($l -eq 'M-13') { [void]$sb.Append("`n"); continue }
    if ($l -eq '13') { [void]$sb.Append('<enter>'); continue }
    [void]$sb.Append([char][int]($l -replace '^M-', ''))
  }
  $sb.ToString()
}
function Get-KeyCount { @(Get-Content -LiteralPath $keys -ErrorAction SilentlyContinue | Where-Object { $_ }).Count }

function Set-Mode([string]$Name, $Window) {
  [IO.File]::WriteAllText($mode, $Name)
  # What the stand-in draws, which the state file can't say
  Start-Sleep -Milliseconds 300
  Wait-Still $Window -IntervalMs 300 | Out-Null
}

Invoke-Test {
  $w = Open-TestWindow -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-prompt.ps1') -ArgumentList '-Sid', $sid, '-Mode', $mode, '-Keys', $keys) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = nil
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.LbtermDraft 'Edit' })
"@
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  if (-not (Wait-Until { (Invoke-Control focus).Reply.session -eq $sid } -TimeoutSec 10)) { throw "the pane never named session $sid" }
  Wait-Still $w -IntervalMs 300 | Out-Null

  # 1. Two pieces: a card above the input box, Q1 then Q2 ticked
  $r = Invoke-Control draft add '1a'
  if ($r.Code -ne 0 -or $r.Reply.draft.text -ne '1a' -or ($r.Reply.draft.answered -join ',') -ne '1') { Add-Failure "draft add 1a: exit $($r.Code), $($r.Reply | ConvertTo-Json -Compress) $($r.Error)" }
  $s = Wait-Change { param($x) @($x.drafts).Count -eq 1 -and $x.drafts[0].text -eq '1a' -and ($x.ticked -join ',') -eq '1' -and $x.next.label -eq 'Q2' -and -not $x.next.off } -What 'the card with 1a, Q1 ticked, Q2 ringed'
  # The ring is red, round Q2's rows
  $shot = Get-Shot $w -Name 'draft-next'
  $ring = $s.next.rect
  $red = [LbtermE2E.Px]::CountNear($shot, (ConvertTo-Rect $ring), 0xFF3B30, 40)
  if ($red -lt $ring.w + $ring.h) { Add-Failure "Q2's ring ($($ring | ConvertTo-Json -Compress)) has $red red pixels, under its width and height's $($ring.w + $ring.h)" }
  else { "one piece: the card says 1a, Q1 ticked, Q2 ringed in red ($red pixels)" }
  Invoke-Control draft add '\n2. yes' | Out-Null
  $s = Wait-Change { param($x) @($x.drafts).Count -eq 1 -and $x.drafts[0].text -eq "1a`n2. yes" -and ($x.ticked -join ',') -eq '1,2' -and $null -eq $x.next } -What 'the card with both, both ticked, no ring'
  $card = $s.drafts[0].rect
  $pane = $s.panes[0].rect
  $cellH = $pane.h / 30
  # Over the pane, its bottom above the input box: the box's upper rule is four rows up from the bottom
  if ($card.y -lt $pane.y -or $card.y + $card.h -gt $pane.y + $pane.h - 3.5 * $cellH) { Add-Failure "the card ($($card.x),$($card.y) $($card.w)x$($card.h)) isn't over the pane above the box (pane $($pane.y)..$($pane.y + $pane.h))" }
  else { 'two pieces: the card sits above the input box, Q1 and Q2 ticked, no ring left' }
  Get-Shot $w -Name 'draft' | Out-Null

  # 2. Scratch that
  $r = Invoke-Control draft scratch
  if ($r.Reply.removed -ne "`n2. yes" -or $r.Reply.draft.text -ne '1a') { Add-Failure "scratch: $($r.Reply | ConvertTo-Json -Compress) $($r.Error)" }
  Wait-Change { param($x) $x.drafts[0].text -eq '1a' -and ($x.ticked -join ',') -eq '1' -and $x.next.label -eq 'Q2' } -What 'the newest piece and its tick gone, Q2 ringed again' | Out-Null
  'scratch that: back to 1a, only Q1 ticked, Q2 ringed again'
  Invoke-Control draft add '\n2. yes' | Out-Null
  Invoke-Control draft add '\n\nalso, check the build' | Out-Null

  # 3. A permission prompt has the box's place
  Set-Mode 'permission' $w
  $before = Get-KeyCount
  $r = Invoke-Control draft send
  Start-Sleep -Milliseconds 400
  if ($r.Code -ne 1 -or $r.Error -notmatch "prompt isn't showing") { Add-Failure "send over a permission prompt: exit $($r.Code), '$($r.Error)'" }
  elseif ((Get-KeyCount) -ne $before) { Add-Failure "send over a permission prompt typed: $(Get-Typed $before)" }
  elseif ((Invoke-Control draft get).Reply.draft.text -ne "1a`n2. yes`n`nalso, check the build") { Add-Failure 'the draft went with the refusal' }
  else { "send over a permission prompt: refused ($($r.Error)), nothing typed, the draft kept" }

  # 4. At the prompt: sent whole
  Set-Mode 'prompt' $w
  $before = Get-KeyCount
  $r = Invoke-Control draft send
  $want = "1a`n2. yes`n`nalso, check the build<enter>"
  $typed = $null
  $done = Wait-Until { $script:typed = Get-Typed $before; $script:typed -eq $want } -TimeoutSec 5
  if ($r.Code -ne 0 -or -not $done) { Add-Failure "send: exit $($r.Code) '$($r.Error)', typed '$($script:typed -replace "`n", '\n')'" }
  else { 'send: typed with Shift+Enter between lines and Enter after' }
  Wait-Change { param($x) @($x.drafts).Count -eq 0 -and @($x.ticked).Count -eq 0 -and $null -eq $x.next } -What 'the card, ticks and ring gone' | Out-Null

  # 5. Edit, from a key
  Invoke-Control draft add '1b' | Out-Null
  Wait-Change { param($x) @($x.drafts).Count -eq 1 } -What 'the card for 1b' | Out-Null
  $before = Get-KeyCount
  Send-Key $w F9
  $done = Wait-Until { $script:typed = Get-Typed $before; $script:typed -eq '1b' } -TimeoutSec 5
  Start-Sleep -Milliseconds 300
  if (-not $done -or (Get-Typed $before) -ne '1b') { Add-Failure "edit typed '$(Get-Typed $before)', not 1b alone" }
  else { 'edit (F9): typed 1b without Enter' }
  Wait-Change { param($x) @($x.drafts).Count -eq 0 } -What 'the card gone after edit' | Out-Null

  # 6. A menu in the box's place
  Set-Mode 'picker' $w
  Invoke-Control draft add '1a' | Out-Null
  $before = Get-KeyCount
  $r = Invoke-Control draft send
  Start-Sleep -Milliseconds 300
  if ($r.Code -ne 1 -or $r.Error -notmatch "prompt isn't showing" -or (Get-Typed $before)) { Add-Failure "text sent into a menu: exit $($r.Code), '$($r.Error)', typed '$(Get-Typed $before)'" }
  else { 'text into a menu: refused, nothing typed' }

  # 7. Another session in the pane
  Set-Mode 'newsession' $w
  $gone = Wait-Until { $null -eq (Invoke-Control draft get).Reply.draft } -TimeoutSec 5
  if (-not $gone) { Add-Failure 'the draft outlived the session it was for' }
  else { 'another session in the pane: the draft let go' }
} -Passed 'draft: the card, its ticks and the next question''s red ring follow the pieces, send types it all only into the prompt, edit types it without Enter, a new session lets it go'
