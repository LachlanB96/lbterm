#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/context.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/summaries.rs, lbterm-extras/src/context.rs, lbterm-extras/src/conversation.rs, lbterm-extras/src/summaries.rs, lbterm-panel/src/words.rs, wezterm-gui/src/termwindow/render/menu_fade.rs
<#
The context pill (`lbterm_context`; lbterm/CONTEXT.md) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig), with folders of its own for the transcripts, the summaries, the
prompt history and an InMail folder with nothing in it, and the scroll strip off, so the pill's list
is what has the conversation read. Two tabs are stand-in Claude Code sessions (their panes'
`claude_session` user var set as claudehook.exe sets it): A, whose transcript loaded four files at
its start and pulled in another project's CLAUDE.md after, and B. Each has a `<session>.usage.json`
as Terminal's statusline writes it.

1. The pill on the strip's second row, under the InMail button and centred under it, saying A's
   $12.34 (pill.png).
2. A click drops its list under it: the heading (five files, their tokens, their share of the 1M
   window), the dollars, and each file with its kind, path and tokens, the pulled-in one checked
   for amber (list.png), its card on screen as the List slot's (render/menu_fade.rs; PITFALLS 175).
   A key it doesn't scroll with (Q) flares its card's edge (menu.list.hairline, the name kept from
   before Lachlan's 9a) and leaves it open.
3. A's usage rewritten: the pill and the list say $12.50.
4. A compaction in A's transcript: the pulled-in file leaves the list. Esc closes it.
5. Tab B: the pill says $148.
6. A's usage rewritten while B is open and the panel shut: the window paints nothing for it.
   Back to A, the pill says the new $13.00, so it was read.
7. The panel open: each row's cost, $13.00 and $148 (panel.png); B's usage rewritten while it's
   open, and B's row says $150. It opens at all because the list, opened by a click and closed
   with Esc, no longer leaves the click's capture behind (lbterm/PITFALLS.md, 135).

Where the pill, the list and the rows are, and what they say, come from the window's state file
(lbterm/STATE.md). Clicks and keys are posted messages, never real input.

  pwsh -File lbterm\perf\context.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'context' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
$inmail = Join-Path $T.Out 'inmail\InMail'
$history = Join-Path $T.Out 'history.jsonl'
Remove-Item -Recurse -Force $projects, $sums, $inmail -ErrorAction SilentlyContinue
$folder = Join-Path $projects 'C--work-Harbor'
New-Item -ItemType Directory -Force $folder, $sums, $inmail | Out-Null
[IO.File]::WriteAllText($history, '')

$sidA = '7c1e4a92-3b5d-4f60-8e21-9a0b4c6d2e71'
$sidB = '2f9d6b13-8a47-4c05-b3e2-6d1f0a9c5b84'
$own = 'C:\work\Harbor'
$home1 = $env:USERPROFILE

# A record of A's transcript, as Claude Code writes one
$start = [DateTime]::new(2026, 11, 7, 9, 0, 0, [DateTimeKind]::Utc)
$second = 0
function ConvertTo-Record([hashtable]$r) {
  $script:second += 5
  $r.timestamp = $start.AddSeconds($script:second).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $r.isSidechain = $false
  $r.cwd = $own
  $r.sessionId = $sidA
  $r | ConvertTo-Json -Compress -Depth 10
}
function Get-Instructions {
  @{ type = 'attachment'; attachment = @{ type = 'instructions'; files = @(
    @{ path = "$home1\.claude\CLAUDE.md"; type = 'User'; content = 'u' * 4000 }
    @{ path = 'C:\work\CLAUDE.md'; type = 'Project'; content = 'p' * 8000 }
    @{ path = "$own\CLAUDE.md"; type = 'Project'; content = 'h' * 2000 }
    @{ path = "$home1\.claude\projects\C--work-Harbor\memory\MEMORY.md"; type = 'AutoMem'; content = 'm' * 800 }
  ) } }
}
$ledger = 'C:\work\Ledger\CLAUDE.md'
$transcript = Join-Path $folder "$sidA.jsonl"
$lines = @(
  (ConvertTo-Record @{ type = 'user'; message = @{ role = 'user'; content = 'go' } })
  (ConvertTo-Record (Get-Instructions))
  (ConvertTo-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'text'; text = 'Reading the ledger.' }) } })
  (ConvertTo-Record @{ type = 'attachment'; attachment = @{ type = 'nested_memory'; path = $ledger; displayPath = 'CLAUDE.md'; content = @{ path = $ledger; type = 'Project'; content = 'n' * 20000 } } })
)
[IO.File]::WriteAllText($transcript, ($lines -join "`n") + "`n")

# What Terminal's statusline writes beside a session's summary
function Set-Usage([string]$Sid, [double]$Usd, [int]$Window) {
  $file = Join-Path $sums "$Sid.usage.json"
  Write-FileWhole -Path $file -Text ('{{"usd":{0},"window":{1}}}' -f $Usd.ToString([Globalization.CultureInfo]::InvariantCulture), $Window)
}
Set-Usage $sidA 12.34 1000000
Set-Usage $sidB 148.73 200000

function Get-StripItem([object]$s, [string]$name) { @($s.strip.items | Where-Object name -eq $name)[0] }
function Get-Labels($s) { @($s.menu.items | ForEach-Object label) }
$userPath = '~\.claude\CLAUDE.md'
$want = @(
  '5 context files · ≈8.7k tokens · 0.9% of the 1M window'
  '$12.34 at list price: every run of it and its subagents'
  "user · $userPath · ≈1k"
  'project · C:\work\CLAUDE.md · ≈2k'
  'project · C:\work\Harbor\CLAUDE.md · ≈500'
  'memory · ~\.claude\projects\C--work-Harbor\memory\MEMORY.md · ≈200'
  'pulled in · C:\work\Ledger\CLAUDE.md · ≈5k'
)

Invoke-Test {
  $w = Open-TestWindow -PostedPointer -Program (New-ClaudeSessionProgram -Sid $sidA) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = '$(ConvertTo-LuaPath $history)'
config.lbterm_scroll_strip = nil
rawset(config, 'lbterm_tab_panel', { enabled = true, open_delay_ms = 0 })
rawset(config, 'lbterm_inbox', { folders = { [[$inmail]] } })
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath $projects)' })
config.initial_cols = 110
config.initial_rows = 28
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  # 1. The pill under the InMail button
  $s = Wait-Change { param($s) $s.strip.context -eq '$12.34' -and (Get-StripItem $s 'context') -and (Get-StripItem $s 'inbox') } -What 'the pill saying $12.34'
  $pill = (Get-StripItem $s 'context').rect
  $button = (Get-StripItem $s 'inbox').rect
  "pill: x $($pill.x)..$($pill.x + $pill.w), y $($pill.y)..$($pill.y + $pill.h); InMail button x $($button.x)..$($button.x + $button.w), y $($button.y)..$($button.y + $button.h)"
  if ([Math]::Abs(($pill.x + $pill.w / 2) - ($button.x + $button.w / 2)) -gt 2) { Add-Failure "the pill isn't centred under the InMail button: its middle $($pill.x + $pill.w / 2), the button's $($button.x + $button.w / 2)" }
  if ($pill.y -lt $button.y + $button.h) { Add-Failure "the pill isn't under the InMail button: its top $($pill.y), the button's bottom $($button.y + $button.h)" }
  if ($s.rail -and $pill.y + $pill.h -gt $s.rail.rect.y) { Add-Failure "the pill runs into the rail: its bottom $($pill.y + $pill.h), the rail's top $($s.rail.rect.y)" }
  $pillShot = Get-Shot $w -Name 'pill'

  # 2. Its list: the heading, the dollars, every file, the pulled-in one checked
  $c = Get-RectCenter $pill
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) $s.menu.kind -eq 'context' -and (Get-Labels $s).Count -eq 7 } -What 'the list with its five files' -TimeoutSec 15
  $labels = Get-Labels $s
  "list: $($labels -join ' / ')"
  for ($i = 0; $i -lt $want.Count; $i++) {
    if ($labels[$i] -ne $want[$i]) { Add-Failure "line $i of the list says '$($labels[$i])', not '$($want[$i])'" }
  }
  $checked = @($s.menu.items | Where-Object checked | ForEach-Object label)
  if ($checked.Count -ne 1 -or $checked[0] -notlike 'pulled in*Ledger*') { Add-Failure "the amber lines aren't the ledger's alone: $($checked -join ' / ')" }
  $card = $s.menu.rect
  if ($card.y -lt $pill.y + $pill.h) { Add-Failure "the list doesn't hang under the pill: its card's top $($card.y), the pill's bottom $($pill.y + $pill.h)" }
  if ([Math]::Abs($card.x - $button.x) -gt 12) { Add-Failure "the list isn't under the buttons: its card at x $($card.x), the InMail button at $($button.x)" }
  # On screen: its card is the List slot's (render/menu_fade.rs), the one the compositor fades in,
  # where a card drawn at those z-layers without it was put nowhere (lbterm/PITFALLS.md, 175)
  $listShot = Get-Shot $w -Name 'list'
  if (-not (Get-DiffBox $pillShot $listShot -Region (New-Rect $card.x $card.y $card.w $card.h))) { Add-Failure "the list's card is in the state file but not on screen: nothing changed inside $($card.x),$($card.y) $($card.w)x$($card.h)" }
  else { 'list: on screen' }
  $pillShot.Dispose(); $listShot.Dispose()
  # A key it doesn't scroll with (Q) flares its card's edge and leaves it open (Lachlan's 7b:
  # an open list has every key), where it played nothing and went nowhere before
  $f0 = Get-StateFrame $w
  Send-Key $w Q
  try {
    Wait-State { param($s) @($s.animations | ForEach-Object name) -contains 'menu.list.hairline' } -AfterFrame $f0 -What "Q's hairline" -TimeoutSec 10 | Out-Null
    if (-not (Read-State).windows[0].menu) { Add-Failure 'Q over the list closed it' } else { "list: Q flares its card's edge" }
  } catch { Add-Failure "Q over the list didn't flare its card's edge (menu.list.hairline): $_" }

  # 3. The dollars move: the pill and the list follow
  Set-Usage $sidA 12.5 1000000
  $s = Wait-Change { param($s) $s.strip.context -eq '$12.50' -and (Get-Labels $s)[1] -like '$12.50 at list price*' } -What 'the pill and the list saying $12.50' -TimeoutSec 5
  'the dollars followed'

  # 4. A compaction: the pulled-in file goes; Esc closes the list
  $more = @(
    (ConvertTo-Record @{ type = 'system'; subtype = 'compact_boundary'; content = 'Conversation compacted' })
    (ConvertTo-Record (Get-Instructions))
  )
  [IO.File]::AppendAllText($transcript, ($more -join "`n") + "`n")
  $s = Wait-Change { param($s) (Get-Labels $s).Count -eq 6 } -What 'the list without the pulled-in file' -TimeoutSec 10
  $labels = Get-Labels $s
  if ($labels -like 'pulled in*') { Add-Failure "the pulled-in file is still listed after the compaction: $($labels -join ' / ')" }
  if ($labels[0] -ne '4 context files · ≈3.7k tokens · 0.4% of the 1M window') { Add-Failure "the heading after the compaction says '$($labels[0])'" }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'the list closed' | Out-Null

  # 5. Tab B: its own dollars
  $paneA = (Get-Panes)[0].pane_id
  $paneB = New-Tab -From $paneA -Program (New-ClaudeSessionProgram -Sid $sidB)
  $s = Wait-Change { param($s) $s.strip.context -eq '$148' } -What 'the pill saying B''s $148'
  'tab B: $148'

  # 6. A's dollars while B is open and the panel shut: nothing painted for them, but read
  $s = Wait-Settled -What 'the window to settle'
  $frame = Get-StateFrame $w
  Set-Usage $sidA 13 1000000
  Wait-Ms 1500
  $after = Get-StateFrame $w
  if ($after -ne $frame) { Add-Failure "the window painted $($after - $frame) frame(s) for a cost it doesn't show" } else { "no frame for A's cost while B shows (frame $frame)" }
  Invoke-Cli activate-pane --pane-id $paneA | Out-Null
  Wait-Change { param($s) $s.strip.context -eq '$13.00' } -What 'the pill saying A''s new $13.00' | Out-Null
  'back on A: $13.00'

  # 7. The panel open: each row's cost, and B's following its file
  $s = Wait-State { param($s) @($s.rail.rows).Count -eq 2 } -What 'two rows on the rail'
  $pad = Get-RectCenter @($s.rail.rows)[1].pad
  Set-PointerHold $w $pad[0] $pad[1]
  $s = Wait-Change { param($s) $s.rail.open -and @($s.rail.rows)[0].cost -eq '$13.00' -and @($s.rail.rows)[1].cost -eq '$148' } -What 'the open rows with their costs' -Animating -TimeoutSec 15
  (Get-Shot $w -Name 'panel').Dispose()
  Set-Usage $sidB 150.2 200000
  Wait-Change { param($s) $s.rail.open -and @($s.rail.rows)[1].cost -eq '$150' } -What 'B''s row saying $150' -Animating -TimeoutSec 5 | Out-Null
  'the open rows follow their costs'
  Clear-PointerHold
} -Passed 'context pill passed'
