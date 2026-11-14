#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/context.rs, wezterm-gui/src/termwindow/render/cost.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/summaries.rs, lbterm-extras/src/summaries.rs
<#
The context pill (`lbterm_context`; lbterm/CONTEXT.md) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig), with folders of its own for the summaries, the prompt history,
an empty transcripts folder and an InMail folder with nothing in it, the scroll strip and the
projects pad off. Two tabs are stand-in Claude Code sessions (their panes' `claude_session` user var
set as claudehook.exe sets it), A and B, each with a `<session>.usage.json` as Terminal's statusline
writes it. The list of the files in the context that a click on the pill dropped until 2026-11-08
went with Lachlan's 5a (the context bar's pane lists them now: perf/context-bar.ps1).

1. The pill on the strip's second row, under the InMail button and centred under it, saying A's
   $12.34 (pill.png).
2. A click on it opens no list: it opens the cost pane (Lachlan's 7c of 2026-11-09; perf/cost.ps1
   has the pane), which with LBTerm's own count off, the default since the context bar's round 4
   (Lachlan's 2b of 2026-11-10), and no `<session>.cost.json` in the summaries folder, shows the
   pill's dollars and says the Claude project hasn't published the split for this conversation
   yet, counting nothing; a second click shuts it; the pill says the same.
3. A's usage rewritten: the pill says $12.50.
4. Tab B: the pill says $148.
5. A's usage rewritten while B is open and the panel shut: the window paints nothing for it.
   Back to A, the pill says the new $13.00, so it was read.
6. The panel open: each row's cost, $13.00 and $148 (panel.png); B's usage rewritten while it's
   open, and B's row says $150. It opens at all because the click on the pill in step 2 left no
   capture behind (lbterm/PITFALLS.md, 135).

Where the pill and the rows are, and what they say, come from the window's state file
(lbterm/STATE.md). Clicks are posted messages, never real input.

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
New-Item -ItemType Directory -Force $projects, $sums, $inmail | Out-Null
[IO.File]::WriteAllText($history, '')

$sidA = '7c1e4a92-3b5d-4f60-8e21-9a0b4c6d2e71'
$sidB = '2f9d6b13-8a47-4c05-b3e2-6d1f0a9c5b84'

# What Terminal's statusline writes beside a session's summary
function Set-Usage([string]$Sid, [double]$Usd, [int]$Window) {
  $file = Join-Path $sums "$Sid.usage.json"
  Write-FileWhole -Path $file -Text ('{{"usd":{0},"window":{1}}}' -f $Usd.ToString([Globalization.CultureInfo]::InvariantCulture), $Window)
}
Set-Usage $sidA 12.34 1000000
Set-Usage $sidB 148.73 200000

function Get-StripItem([object]$s, [string]$name) { @($s.strip.items | Where-Object name -eq $name)[0] }

Invoke-Test {
  $w = Open-TestWindow -PostedPointer -Program (New-ClaudeSessionProgram -Sid $sidA) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = '$(ConvertTo-LuaPath $history)'
config.lbterm_scroll_strip = nil
rawset(config, 'lbterm_projects', nil)
rawset(config, 'lbterm_context_bar', nil)
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
  (Get-Shot $w -Name 'pill').Dispose()

  # 2. A click on it opens no list (Lachlan's 5a) but the cost pane (his 7c), which with the count
  # off (the default, his 2b of 2026-11-10) and no split in the summaries folder says the Claude
  # project hasn't published one; a second click shuts it, and the pill says the same throughout.
  # Until round 5 of feat/context-bar this waited for the count's 'No transcript' note, which only
  # a window with lbterm_context.count_cost on gives, and failed from round 4's stage 3 on
  $c = Get-RectCenter $pill
  Send-Click $w $c[0] $c[1] -Move
  $s = Wait-Change { param($s) $s.cost.open -and $s.cost.note -like "The Claude project hasn't published*" } -What 'the cost pane saying the Claude project hasn''t published the split' -Animating -TimeoutSec 10
  if ($s.cost.count -ne $false -or $null -ne $s.cost.source -or $null -ne $s.cost.total) { Add-Failure "the pane with the count off: count '$($s.cost.count)', source '$($s.cost.source)', total '$($s.cost.total)'" }
  if ($s.cost.words -notcontains '$12.34') { Add-Failure "the pane doesn't say the pill's `$12.34: $($s.cost.words -join ' | ')" }
  "the cost pane: $($s.cost.note)"
  Send-Click $w $c[0] $c[1]
  $s = Wait-Change { param($s) -not $s.cost.open } -What 'the cost pane shut by a second click' -Animating -TimeoutSec 5
  if ($s.strip.context -ne '$12.34') { Add-Failure "the pill says '$($s.strip.context)' after the clicks" }
  $f0 = Get-StateFrame $w

  # 3. The dollars move: the pill follows
  Set-Usage $sidA 12.5 1000000
  $s = Wait-State { param($s) $s.strip.context -eq '$12.50' } -AfterFrame $f0 -What 'the pill saying $12.50' -TimeoutSec 5
  if ($s.menu) { Add-Failure "a click on the pill opened a menu: $($s.menu.kind)" } else { 'a click on the pill opens no menu' }
  'the dollars followed'

  # 4. Tab B: its own dollars
  $paneA = (Get-Panes)[0].pane_id
  $paneB = New-Tab -From $paneA -Program (New-ClaudeSessionProgram -Sid $sidB)
  $s = Wait-Change { param($s) $s.strip.context -eq '$148' } -What 'the pill saying B''s $148'
  'tab B: $148'

  # 5. A's dollars while B is open and the panel shut: nothing painted for them, but read
  $s = Wait-Settled -What 'the window to settle'
  $frame = Get-StateFrame $w
  Set-Usage $sidA 13 1000000
  Wait-Ms 1500
  $after = Get-StateFrame $w
  if ($after -ne $frame) { Add-Failure "the window painted $($after - $frame) frame(s) for a cost it doesn't show" } else { "no frame for A's cost while B shows (frame $frame)" }
  Invoke-Cli activate-pane --pane-id $paneA | Out-Null
  Wait-Change { param($s) $s.strip.context -eq '$13.00' } -What 'the pill saying A''s new $13.00' | Out-Null
  'back on A: $13.00'

  # 6. The panel open: each row's cost, and B's following its file
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
