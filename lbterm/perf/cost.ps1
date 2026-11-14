#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/cost.rs, wezterm-gui/src/cost.rs, lbterm-extras/src/cost.rs, wezterm-gui/src/termwindow/render/context.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/objectives.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm/perf/threads.ps1, lbterm-extras/src/summaries.rs, wezterm-gui/src/summaries.rs
# Variant: -Off
<#
The cost pane the context pill's press opens (lbterm/CONTEXT.md, "The cost pane"; Lachlan's 7c of
2026-11-09), and the projects pad's fixed place beside the pill (his 6a), in a test window that
looks like Lachlan's LBTerm, with folders of its own. Its Claude session is a stand-in: its pane's
`claude_session` user var set as claudehook.exe sets it, its `<session>.usage.json` as Terminal's
statusline writes it, and its transcript and subagents' copied from lbterm-extras' fixture
(src/cost/fixture: his turns, a subagent, a workflow of two agents and its journal, a compaction),
whose total the Claude project's count makes $0.673102 (.claude/tests/cost-prices.test.ts). The
context bar is on, as in his try window, with LBTerm's own count of the cost
(`lbterm_context.count_cost`, the pill's setting since the pane hangs from it) on, or with -Off
left at its default, off (Lachlan's 2b of 2026-11-10), when the pane shows the Claude project's
split instead (`@Claude:cost`): a stand-in `<session>.cost.json` in the summaries folder, in its
version 1 shape (`api Claude cost`), whose numbers aren't the fixture's, so a pane counted from
the transcripts beside it would show.

1. The pill says $0.67, the projects pad right of it.
2. Resting on the pill opens nothing.
3. A press opens the pane: under the strip, from the panes' left to near the window's right, over
   no part of the rail; its total $0.673102, his turns $0.224552, one subagent $0.00151, one
   workflow $0.44704, one compaction, the four rows' words, a graph with a mark where the subagent
   and the workflow started and the compaction named with its sizes, `42k → 6.0k` (pane.png); and
   the GUI has its `lbterm-cost` thread.
   With -Off, before the Claude project has written the split: the pane says the pill's $0.67,
   the window's 200k and that it isn't published yet for this conversation (`cost.count` false,
   `cost.source` null, no total, no rows, no graph), and the GUI had no `lbterm-cost` thread
   before the press.
4. A request appended to the transcript and the dollars moved: the pane's total follows. With -Off
   the split written and the dollars moved: the pane shows the stand-in's split (`cost.source`
   `claude`, its total $0.87, his turns $0.30, two subagents $0.07, two workflow runs of three
   agents $0.50, two compactions, a mark at each subagent and each run, the compactions' sizes
   `700k → …` and `500k → …`, the first named on the graph; pane-off.png), none of it the
   fixture's count; the split written again with a third compaction and no cent moved: the open
   pane says `Compactions (3)` and has its sizes, `600k → 70k`, since it reads the split on the
   file's own write (round 5's stage 2 of feat/context-bar); then a version 2 written and the dollars moved again: it says
   which version it is and shows no split.
5. A second subagent's transcript and the dollars moved: two subagents (not with -Off).
6. The pointer leaving the pill and the pane: `cost.close`, then it's shut.
7. A press opens it, another on the pill shuts it at once. With -Off, the split written again
   while it's shut (back to version 1, a third compaction, no cent moved): the next press opens it
   on that split (`cost.source` `claude`, `Compactions (3)`), read because the summaries thread
   heard the write while it was shut (round 5's stage 2: an opening reads the split only once it
   was written since it was last asked for); shut and opened again with nothing written, the same.
8. A tab with no Claude session: no pill, and the pad where it was (Lachlan's 6a); back, the same.
   With -Off, one `lbterm-cost` thread after all of it, the one the press started to read the
   file.

Where everything is and what it says come from the window's state file (lbterm/STATE.md, `cost`).
The pointer is posted messages, never real input.

  pwsh -File lbterm\perf\cost.ps1 [-Off]
#>
param([string]$Bin, [switch]$Off)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot 'threads.ps1')
$T = Start-Test $(if ($Off) { 'cost-off' } else { 'cost' }) -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
$inmail = Join-Path $T.Out 'inmail\InMail'
$alpha = Join-Path $T.Out 'alpha'
$history = Join-Path $T.Out 'history.jsonl'
Remove-Item -Recurse -Force $projects, $sums, $inmail, $alpha -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $projects, $sums, $inmail, $alpha | Out-Null
[IO.File]::WriteAllText($history, '')

$sid = '5d2c8e17-6a40-4b93-9f1e-3c7a0b2d4e86'
$work = Join-Path $projects 'C--work'
New-Item -ItemType Directory -Force $work | Out-Null
$fixture = Join-Path $PSScriptRoot '..\..\lbterm-extras\src\cost\fixture'
$transcript = Join-Path $work "$sid.jsonl"
Copy-Item (Join-Path $fixture 'sess.jsonl') $transcript
Copy-Item -Recurse (Join-Path $fixture 'sess') (Join-Path $work $sid)

function Set-Usage([double]$Usd) {
  Write-FileWhole -Path (Join-Path $sums "$sid.usage.json") -Text ('{{"usd":{0},"window":200000}}' -f $Usd.ToString([Globalization.CultureInfo]::InvariantCulture))
}
function Add-Line([string]$File, [string]$Line) { [IO.File]::AppendAllText($File, "$Line`n") }
function Get-StripItem([object]$s, [string]$name) { @($s.strip.items | Where-Object name -eq $name)[0] }
function Test-Near([double]$a, [double]$b) { [Math]::Abs($a - $b) -lt 1e-6 }
# The GUI's threads named lbterm-cost (wezterm-gui/src/cost.rs: it counts, or with the count off
# reads the Claude project's file; none before the first press)
function Get-CostThreads($w) { @([LbtermRest]::Take($w.Pid).Threads.Values | Where-Object Name -eq 'lbterm-cost') }
# What the pane says before the Claude project has written the split (lbterm_extras::cost::NOT_PUBLISHED)
$unpublished = "The Claude project hasn't published what spent it for this conversation yet"
# The Claude project's split as Terminal's statusline writes it (@Claude:cost, v 1), a stand-in
# whose numbers aren't the fixture's: his turns $0.30, subagents aone and atwo $0.07, workflow
# runs wf_a (two agents) and wf_b $0.50, two compactions, a step a minute something was spent in
# (its dollars written as text, whatever the culture)
function Set-Split([int]$V = 1, [switch]$ThirdCompaction) {
  $t0 = 1794218400000  # 2026-11-09T10:00:00Z
  $spender = { param($kind, $id, $run, $start, $usd, $tokens, $dollars)
    '{{"kind":"{0}","id":"{1}","run":{2},"label":null,"start":{3},"end":{4},"requests":2,"usd":{5},"tokens":[{6}],"dollars":[{7}]}}' -f $kind, $id, $(if ($run) { "`"$run`"" } else { 'null' }), ($t0 + $start), ($t0 + $start + 60000), $usd, $tokens, $dollars }
  $spenders = @(
    (& $spender main $sid $null 0 '0.3' '100,20000,300000,4000' '0.01,0.1,0.11,0.08'),
    (& $spender subagent aone $null 600000 '0.04' '10,1000,2000,300' '0.01,0.01,0.01,0.01'),
    (& $spender workflow w1 wf_a 1200000 '0.2' '1,2,3,4' '0.05,0.05,0.05,0.05'),
    (& $spender subagent atwo $null 1500000 '0.03' '5,5,5,5' '0.0075,0.0075,0.0075,0.0075'),
    (& $spender workflow w2 wf_b 1800000 '0.25' '2,2,2,2' '0.0625,0.0625,0.0625,0.0625'),
    (& $spender workflow w3 wf_a 2400000 '0.05' '1,1,1,1' '0.0125,0.0125,0.0125,0.0125')
  ) -join ','
  $compactions = '{{"at":{0},"trigger":"auto","before":700000,"after":16000}},{{"at":{1},"trigger":"manual","before":500000,"after":15000}}' -f ($t0 + 2700000), ($t0 + 2900000)
  if ($ThirdCompaction) { $compactions += ',{{"at":{0},"trigger":"auto","before":600000,"after":14000,"next":70000}}' -f ($t0 + 2950000) }
  $steps = '[{0},0.2],[{1},0.4],[{2},0.7],[{3},0.87]' -f $t0, ($t0 + 600000), ($t0 + 1800000), ($t0 + 3000000)
  Write-FileWhole -Path (Join-Path $sums "$sid.cost.json") -Text ('{{"v":{0},"usd":0.87,"spenders":[{1}],"compactions":[{2}],"steps":[{3}]}}' -f $V, $spenders, $compactions, $steps)
}
$count = if ($Off) { '' } else { ', count_cost = true' }
Set-Usage 0.67

Invoke-Test {
  $w = Open-TestWindow -PostedPointer -Program (New-ClaudeSessionProgram -Sid $sid) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = '$(ConvertTo-LuaPath $history)'
config.lbterm_scroll_strip = nil
rawset(config, 'lbterm_context_bar', {})
rawset(config, 'lbterm_tab_panel', { enabled = true, open_delay_ms = 0 })
rawset(config, 'lbterm_inbox', { folders = { [[$inmail]] } })
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath $projects)'$count })
rawset(config, 'lbterm_projects', { open_delay_ms = 300, close_delay_ms = 200, projects = { { name = 'Alpha', dir = [[$alpha]] } } })
config.initial_cols = 170
config.initial_rows = 34
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  # 1. The pill and the pad after it
  $s = Wait-Change { param($s) $s.strip.context -eq '$0.67' -and $s.cost -and $s.projects } -What 'the pill saying $0.67 and the pad'
  $pill = $s.cost.pill
  $pad = $s.projects.pad
  "pill x $($pill.x)..$($pill.x + $pill.w); pad x $($pad.x)..$($pad.x + $pad.w)"
  if ($pad.x -lt $pill.x + $pill.w) { Add-Failure "the pad isn't right of the pill: its left $($pad.x), the pill's right $($pill.x + $pill.w)" }
  $c = Get-RectCenter $pill

  # 2. A rest on the pill opens nothing
  Send-Mouse $w Move $c[0] $c[1]
  Wait-Ms 700
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  if ($s.cost.open) { Add-Failure 'resting on the pill opened the pane' } else { 'a rest on the pill opens nothing' }

  if ($Off) {
    $threads = Get-CostThreads $w
    if ($threads.Count) { Add-Failure "the GUI has $($threads.Count) lbterm-cost thread(s) before the pane first opened" } else { 'no lbterm-cost thread before the press' }
  }
  $f0 = Get-StateFrame $w
  Send-Click $w $c[0] $c[1]
  if ($Off) {
    # 3. With the count off, before the Claude project has written the split: the pill's dollars
    # and window, and that it isn't published yet; nothing counted
    $s = Wait-State { param($s) $s.cost.open -and $s.cost.note -eq $unpublished } -AfterFrame $f0 -What 'the pane open, the split not published yet' -TimeoutSec 10
    $p = $s.cost
    "pane x $($p.rect.x)..$($p.rect.x + $p.rect.w), y $($p.rect.y)..$($p.rect.y + $p.rect.h); count $($p.count); says $($p.words -join ' | ')"
    if ($p.count -ne $false) { Add-Failure "cost.count is '$($p.count)', not false, with count_cost left at its default" }
    if ($null -ne $p.source) { Add-Failure "cost.source is '$($p.source)' with no split to show" }
    foreach ($word in '$0.67', '200k context window', $unpublished) {
      if ($p.words -notcontains $word) { Add-Failure "the pane doesn't say '$word': $($p.words -join ' | ')" }
    }
    if ($null -ne $p.total -or @($p.rows).Count -or $p.graph -or $p.steps -or @($p.marks).Count) { Add-Failure "a split with none published: total $($p.total), $(@($p.rows).Count) rows, graph $($p.graph), $($p.steps) steps" }
    if ($p.rect.x -lt $s.rail.rect.x + $s.rail.rect.w - 1 -or $p.rect.y -lt $pill.y + $pill.h) { Add-Failure "the pane at $($p.rect | ConvertTo-Json -Compress) is over the rail or the pill" }
    'the split not published yet: the pill''s dollars and window, and that it isn''t'

    # 4. The split written and the dollars moved: the Claude project's split, not a count
    Send-Mouse $w Move $c[0] $c[1]
    Set-Split
    Set-Usage 0.87
    $s = Wait-Change { param($s) $s.cost.open -and $s.cost.source -eq 'claude' } -What 'the pane showing the Claude project''s split' -TimeoutSec 10
    $p = $s.cost
    "total $($p.total); rows $(@($p.rows | ForEach-Object { "$($_.spender) $($_.runs) $($_.usd)" }) -join ', '); marks $(@($p.marks) -join ','); steps $($p.steps)"
    if (-not (Test-Near $p.total 0.87)) { Add-Failure "its total is $($p.total), not the split's usd, 0.87" }
    $rows = @{}; foreach ($r in $p.rows) { $rows[$r.spender] = $r }
    if (-not (Test-Near $rows.turns.usd 0.3)) { Add-Failure "his turns cost $($rows.turns.usd), not the split's 0.30 (the fixture's count is 0.224552)" }
    if ($rows.turns.input -ne 100 -or $rows.turns.writes -ne 20000 -or $rows.turns.reads -ne 300000 -or $rows.turns.output -ne 4000) { Add-Failure "his turns' tokens: $($rows.turns | ConvertTo-Json -Compress)" }
    if ($rows.subagents.runs -ne 2 -or -not (Test-Near $rows.subagents.usd 0.07)) { Add-Failure "subagents: $($rows.subagents.runs) for $($rows.subagents.usd), not 2 for 0.07" }
    if ($rows.workflows.runs -ne 2 -or -not (Test-Near $rows.workflows.usd 0.5)) { Add-Failure "workflows: $($rows.workflows.runs) for $($rows.workflows.usd), not 2 runs (three agents) for 0.50" }
    if ($p.compactions -ne 2) { Add-Failure "$($p.compactions) compactions, not the split's 2" }
    # Each compaction's sizes, the split's before and next (none yet), and the first named on the
    # graph, left of its rule, since right of it would cross the second's rule; the second, 3
    # minutes on, has room on neither side (lbterm-extras' cost tests hold where a name goes)
    if ((@($p.compacted) -join ' | ') -ne '700k → … | 500k → …') { Add-Failure "the compactions' sizes are '$(@($p.compacted) -join ' | ')', not '700k → … | 500k → …'" }
    if ($p.words -notcontains '700k → …') { Add-Failure "the graph doesn't name the first compaction '700k → …': $($p.words -join ' | ')" }
    if ($p.steps -ne 4) { Add-Failure "$($p.steps) steps, not the split's 4" }
    foreach ($word in '$0.87', 'Your turns', 'Subagents (2)', 'Workflows (2)', 'Compactions (2)', 'Over time') {
      if ($p.words -notcontains $word) { Add-Failure "the pane doesn't say '$word': $($p.words -join ' | ')" }
    }
    if (-not $p.graph) { Add-Failure 'no graph' } elseif ((@($p.marks) -join ',') -ne 'subagents,workflows,subagents,workflows') { Add-Failure "the graph's marks are '$(@($p.marks) -join ',')', not a subagent's, a run's, a subagent's and a run's" }
    if ($p.rect.x -lt $s.rail.rect.x + $s.rail.rect.w - 1 -or $p.rect.y -lt $pill.y + $pill.h) { Add-Failure "the pane at $($p.rect | ConvertTo-Json -Compress) is over the rail or the pill" }
    (Get-Shot $w -Name 'pane-off').Dispose()
    'the Claude project''s split: its total, each row, the runs and the graph, none of it counted'

    # A split written again with no cent moved (a compaction, which records no dollars): the open
    # pane follows the file's own write, not the pill's cents (round 5's stage 2 of
    # feat/context-bar, PITFALLS 323; a build before it read the split only when the cents moved,
    # and fails here)
    Set-Split -ThirdCompaction
    $s = Wait-Change { param($s) $s.cost.open -and $s.cost.compactions -eq 3 } -What 'the open pane following a split written with no cent moved' -TimeoutSec 10
    if ($s.cost.words -notcontains 'Compactions (3)') { Add-Failure "the pane doesn't say 'Compactions (3)': $($s.cost.words -join ' | ')" }
    if (@($s.cost.compacted)[2] -ne '600k → 70k') { Add-Failure "the third compaction's sizes are '$(@($s.cost.compacted)[2])', not the split's 600k before and 70k next" }
    if (-not (Test-Near $s.cost.total 0.87) -or $s.strip.context -ne '$0.87') { Add-Failure "the total $($s.cost.total) and the pill '$($s.strip.context)' after a split with no cent moved, not 0.87 and `$0.87" }
    'a split written with no cent moved: the open pane follows it (Compactions (3))'

    # A version this doesn't read: it says which, with no split
    Set-Split -V 2
    Set-Usage 0.97
    $s = Wait-Change { param($s) $s.cost.open -and $s.cost.words -contains '$0.97' -and $s.cost.note -like '*version 2*' } -What 'the pane saying the split is version 2' -TimeoutSec 10
    if ($null -ne $s.cost.source -or @($s.cost.rows).Count) { Add-Failure "a version 2 split shown: source $($s.cost.source), $(@($s.cost.rows).Count) rows" }
    "a version 2: $($s.cost.note)"
    $pillSays = '$0.97'
  } else {
  # 3. A press opens it, counted
  $s = Wait-State { param($s) $s.cost.open -and $null -ne $s.cost.total } -AfterFrame $f0 -What 'the pane open with its count' -TimeoutSec 10
  $p = $s.cost
  if ($p.count -ne $true) { Add-Failure "cost.count is '$($p.count)', not true, with count_cost on" }
  "pane x $($p.rect.x)..$($p.rect.x + $p.rect.w), y $($p.rect.y)..$($p.rect.y + $p.rect.h); total $($p.total)"
  if (-not (Test-Near $p.total 0.673102)) { Add-Failure "its total is $($p.total), not the Claude project's count of the fixture, 0.673102" }
  $rows = @{}; foreach ($r in $p.rows) { $rows[$r.spender] = $r }
  if (-not (Test-Near $rows.turns.usd 0.224552)) { Add-Failure "his turns cost $($rows.turns.usd), not 0.224552" }
  if ($rows.subagents.runs -ne 1 -or -not (Test-Near $rows.subagents.usd 0.00151)) { Add-Failure "subagents: $($rows.subagents.runs) for $($rows.subagents.usd), not 1 for 0.00151" }
  if ($rows.workflows.runs -ne 1 -or -not (Test-Near $rows.workflows.usd 0.44704)) { Add-Failure "workflows: $($rows.workflows.runs) for $($rows.workflows.usd), not 1 (two agents) for 0.44704 (its journal left out)" }
  if ($rows.turns.input -ne 6013 -or $rows.turns.writes -ne 21500 -or $rows.turns.reads -ne 20000 -or $rows.turns.output -ne 2500) { Add-Failure "his turns' tokens: $($rows.turns | ConvertTo-Json -Compress)" }
  if ($p.compactions -ne 1) { Add-Failure "$($p.compactions) compactions, not 1" }
  # Its sizes: the fixture's preTokens, and the sonnet request after it (the Claude project's next)
  if ((@($p.compacted) -join ' | ') -ne '42k → 6.0k') { Add-Failure "the compaction's sizes are '$(@($p.compacted) -join ' | ')', not '42k → 6.0k'" }
  foreach ($word in '$0.67', 'Your turns', 'Subagents (1)', 'Workflows (1)', 'Compactions (1)', 'Over time', '42k → 6.0k') {
    if ($p.words -notcontains $word) { Add-Failure "the pane doesn't say '$word': $($p.words -join ' | ')" }
  }
  if (-not $p.graph) { Add-Failure 'no graph' } elseif ((@($p.marks) -join ',') -ne 'subagents,workflows') { Add-Failure "the graph's marks are '$(@($p.marks) -join ',')', not a subagent's then a workflow's" }
  $panesLeft = $s.rail.rect.x + $s.rail.rect.w
  if ($p.rect.x -lt $panesLeft - 1) { Add-Failure "the pane starts at $($p.rect.x), over the rail, whose right is $panesLeft" }
  if ($p.rect.y -lt $pill.y + $pill.h) { Add-Failure "the pane's top $($p.rect.y) is over the pill, whose bottom is $($pill.y + $pill.h)" }
  if ($p.rect.x + $p.rect.w -lt $s.size.width - 40) { Add-Failure "the pane ends at $($p.rect.x + $p.rect.w), not near the window's right $($s.size.width)" }
  (Get-Shot $w -Name 'pane').Dispose()
  'the pane: its total, each row and the graph'
  $threads = Get-CostThreads $w
  if ($threads.Count -ne 1) { Add-Failure "the GUI has $($threads.Count) lbterm-cost threads with the count on, not 1" } else { 'its lbterm-cost thread' }

  # 4. A new request: the pane follows once the dollars move
  Send-Mouse $w Move $c[0] $c[1]
  Add-Line $transcript '{"type":"assistant","timestamp":"2026-11-09T11:30:00.000Z","message":{"id":"m9","model":"claude-opus-5-5","usage":{"input_tokens":0,"output_tokens":10000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}'
  Set-Usage 0.87
  $s = Wait-Change { param($s) $s.cost.open -and (Test-Near $s.cost.total 0.873102) } -What 'the total following a new request' -TimeoutSec 10
  'a new request: the total follows'

  # 5. Another subagent
  Add-Line (Join-Path $work "$sid\subagents\agent-atwo.jsonl") '{"type":"assistant","timestamp":"2026-11-09T11:40:00.000Z","message":{"id":"s9","model":"claude-haiku-4-5","usage":{"input_tokens":1000000,"output_tokens":0}}}'
  Set-Usage 0.97
  $s = Wait-Change { param($s) $s.cost.open -and @($s.cost.rows | Where-Object spender -eq 'subagents')[0].runs -eq 2 -and (Test-Near $s.cost.total 0.973102) } -What 'two subagents' -TimeoutSec 10
  $pillSays = '$0.97'
  'another subagent: two'
  }

  # 6. Leaving it: its grace, then shut
  $away = @([int]($s.size.width / 2), [int]($s.size.height - 40))
  Send-Mouse $w Move $away[0] $away[1]
  $s = Wait-Change { param($s) -not $s.cost.open } -What 'the pane shut after its grace' -Animating -TimeoutSec 5
  'leaving it shuts it'

  # 7. A press opens it, another shuts it
  Send-Click $w $c[0] $c[1] -Move
  Wait-Change { param($s) $s.cost.open } -What 'the pane open again' -Animating -TimeoutSec 5 | Out-Null
  Send-Click $w $c[0] $c[1]
  Wait-Change { param($s) -not $s.cost.open } -What 'the pane shut by a press' -Animating -TimeoutSec 5 | Out-Null
  'a press on the pill opens and shuts it'
  if ($Off) {
    # 7'. A split written while the pane is shut, no cent moved: the next opening reads it, since
    # the summaries thread counted the write (Summaries::split_written) past what the split was
    # last asked for at (cost.rs's ASKED and asks). Nothing in the state file shows that count
    # while the pane is shut, so the write is given its batch's settle and more before the press
    Set-Split -ThirdCompaction
    Wait-Ms 800
    Send-Click $w $c[0] $c[1] -Move
    $s = Wait-Change { param($s) $s.cost.open -and $s.cost.source -eq 'claude' -and $s.cost.compactions -eq 3 } -What 'the pane opening on a split written while it was shut' -TimeoutSec 10
    if ($s.cost.words -notcontains 'Compactions (3)' -or -not (Test-Near $s.cost.total 0.87)) { Add-Failure "opened on a split written while shut, the pane says $($s.cost.words -join ' | ')" }
    "a split written while it was shut: the next opening shows it (source $($s.cost.source), compactions $($s.cost.compactions), total $($s.cost.total))"
    Send-Click $w $c[0] $c[1]
    Wait-Change { param($s) -not $s.cost.open } -What 'the pane shut again' -Animating -TimeoutSec 5 | Out-Null
    # Nothing written since: it opens on the same split, asking nothing
    Send-Click $w $c[0] $c[1] -Move
    $s = Wait-Change { param($s) $s.cost.open } -What 'the pane open again, nothing written' -Animating -TimeoutSec 5
    if ($s.cost.source -ne 'claude' -or $s.cost.compactions -ne 3) { Add-Failure "opened again with nothing written: source $($s.cost.source), compactions $($s.cost.compactions), not the same split" } else { 'opened again with nothing written: the same split' }
    Send-Click $w $c[0] $c[1]
    Wait-Change { param($s) -not $s.cost.open } -What 'the pane shut a third time' -Animating -TimeoutSec 5 | Out-Null
  }
  Send-Mouse $w Move $away[0] $away[1]

  # 8. A tab with no Claude session: the pad stays where it is
  $paneA = (Get-Panes)[0].pane_id
  $shell = New-Tab -From $paneA
  $s = Wait-Change { param($s) -not $s.strip.context -and $s.projects } -What 'a tab without the pill'
  if ($s.projects.pad.x -ne $pad.x) { Add-Failure "the pad moved from $($pad.x) to $($s.projects.pad.x) in a tab with no Claude session" } else { "a shell tab: the pad stays at $($pad.x)" }
  Invoke-Cli activate-pane --pane-id $paneA | Out-Null
  $s = Wait-Change { param($s) $s.strip.context -eq $pillSays } -What 'back on the Claude tab'
  if ($s.projects.pad.x -ne $pad.x) { Add-Failure "back on the Claude tab the pad is at $($s.projects.pad.x), not $($pad.x)" }
  if ($Off) {
    $threads = Get-CostThreads $w
    if ($threads.Count -ne 1) { Add-Failure "the GUI has $($threads.Count) lbterm-cost threads after all of it with the count off, not the one reader" } else { 'one lbterm-cost thread, the reader' }
  }
} -Passed $(if ($Off) { 'cost pane passed with the count off: the press opens it with the pill''s dollars and window and that the split isn''t published yet, then the Claude project''s split once it''s written and the dollars move (its rows, runs, compactions and marks, none of it counted), and which version a version 2 is; it shuts on leaving and on a press, and the projects pad stays put in a shell tab' } else { 'cost pane passed: the press opens it with the Claude project''s total, split, its graph marked, on its lbterm-cost thread; it follows new requests and subagents, shuts on leaving and on a press, and the projects pad stays put in a shell tab' })
