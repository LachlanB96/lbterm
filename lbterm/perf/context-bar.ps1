#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/context_bar.rs, wezterm-gui/src/termwindow/render/objectives.rs, wezterm-gui/src/termwindow/render/borders.rs, wezterm-gui/src/termwindow/render/fancy_tab_bar.rs, wezterm-gui/src/termwindow/render/sides.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/summaries.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm-extras/src/context_bar.rs, lbterm-extras/src/context.rs, lbterm-extras/src/conversation.rs, lbterm-extras/src/state.rs, config/src/lbterm.rs
# Variant: -MotionOff
<#
The context bar at rest (`lbterm_context_bar`; lbterm/CONTEXT.md, "The context bar"), in a test
window that looks like Lachlan's LBTerm (New-TestConfig), with folders of its own for the
transcripts and the summaries, the objectives strip asked for too (the bar takes its room) and the
scroll strip off. Its one tab is a stand-in Claude Code session (its pane's `claude_session` user
var set as claudehook.exe sets it), whose transcript is written as Claude Code writes one: the
files loaded at the start (the user's CLAUDE.md importing a rule, two project CLAUDE.md files and
the memory index), the skills list, the first request (its shape and blobs) and its reply, which
make the conversation's scale; then another project's CLAUDE.md pulled in, a memory file read, and
the reply after them. Its `<session>.usage.json` says a 200k window, as Terminal's statusline
writes it.

1. The bar waiting for the window's size while the usage file says none, and drawing its sections
   when a change to that file alone says it (the pill off, so only the bar's own wake brings it).
   The bar right of the panes, in the room the objectives strip would take (no objectives in the
   state file), with every pane's right edge left of it; its sections in the bar's order with
   their kinds and colours (the memory index and the memory file yellow, the other project's file
   amber, the CLAUDE.md files grey), each but free with a rect, stacked one under the next from
   the track's top to where the filled part ends, which is what's in use of the window to the
   pixel; the import and the memory file with a Remove button (bar.png).
2. The pointer on the bar: `pointer.over` names it, and the sections aren't laid out again for
   the repaint that lights its ring. Then the window rests: no frame painted while nothing changes.
3. A bigger reply: what's in use grows, free shrinks, and the filled part goes further down.
4. A compaction, and the start's files loaded again: the pulled-in file and the memory file leave
   the bar, which says it's an estimate until the next reply, which makes it exact again
   (compacted.png).
5. The wheel: 24 memory files read, so the pane's rows don't all fit, and its totals (Claude Code's
   parts, the conversation, free) are pinned at its foot under a rule while only the files scroll
   (Lachlan's 3a of 2026-11-09). Opened, its first row is the first section and the files it
   hasn't room for are counted (`left_out`); a notch down scrolls three files, counted above
   (`above`), the row now under the pointer lit, and neither the sections nor the widened bar laid
   out again for it; turned on past the end it stops at the last page (the last file shown, none
   left out, the totals still under it), a notch up from there moves at once, and up past the top
   it's back at the first. Closed and opened again, it's at the top (wheel.png).

Step 2b holds the widened bar (Lachlan's 4b of 2026-11-09) to the closed rail's width, its right
edge the bar's, with each section's number on its own section where it has a line's room and the
rest in a pane as wide as the bar beside it, none overlapping and none outside the bar or that pane.

Where the bar is and what it says come from the window's state file (lbterm/STATE.md). The
pointer is posted messages, never real input.

  pwsh -File lbterm\perf\context-bar.ps1
#>
param([string]$Bin, [switch]$Capture, [switch]$MotionOff)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'context-bar' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
Remove-Item -Recurse -Force $projects, $sums -ErrorAction SilentlyContinue
$folder = Join-Path $projects 'C--work-Harbor'
New-Item -ItemType Directory -Force $folder, $sums | Out-Null

$sid = '5b0d8e21-6c4a-4f37-9e12-3d7a8c0f4b96'
$own = 'C:/work/Harbor'
$memory = 'C:/Users/someone/.claude/projects/C--work-Harbor/memory'

# A record of the transcript, as Claude Code writes one
$start = [DateTime]::new(2026, 11, 8, 9, 0, 0, [DateTimeKind]::Utc)
$second = 0
function ConvertTo-Record([hashtable]$r) {
  $script:second += 5
  $r.timestamp = $start.AddSeconds($script:second).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $r.isSidechain = $false
  $r.cwd = $own
  $r.sessionId = $sid
  $r | ConvertTo-Json -Compress -Depth 10
}
# The files Claude Code loads at the start and again after a compaction
function Get-Instructions {
  @{ type = 'attachment'; attachment = @{ type = 'instructions'; files = @(
    @{ path = 'C:/Users/someone/.claude/CLAUDE.md'; type = 'User'; content = ('u' * 4000) + "`n@C:/work/rules/asking.md`n" }
    @{ path = 'C:/work/rules/asking.md'; type = 'User'; content = 'a' * 1200 }
    @{ path = 'C:/work/CLAUDE.md'; type = 'Project'; content = 'p' * 8000 }
    @{ path = "$own/CLAUDE.md"; type = 'Project'; content = 'h' * 2000 }
    @{ path = "$memory/MEMORY.md"; type = 'AutoMem'; content = 'm' * 800 }
  ) } }
}
# A main-chain reply whose request was `$n` tokens
function Get-Reply([int]$n) {
  @{ type = 'assistant'; message = @{ role = 'assistant'; model = 'm'; content = @(@{ type = 'text'; text = 'Done.' })
    usage = @{ input_tokens = 2; cache_read_input_tokens = $n - 1002; cache_creation_input_tokens = 1000; output_tokens = 50 } } }
}
$raw = { param($r) $r | ConvertTo-Json -Compress -Depth 10 }
$tools = @(@{ name = 'Read'; description = 'd' * 3990 })
$ledger = 'C:/work/Ledger/CLAUDE.md'
$transcript = Join-Path $folder "$sid.jsonl"
$lines = @(
  (ConvertTo-Record @{ type = 'user'; message = @{ role = 'user'; content = 'go' } })
  (ConvertTo-Record (Get-Instructions))
  (ConvertTo-Record @{ type = 'attachment'; attachment = @{ type = 'skill_listing'; isInitial = $true; content = 'x' }; rendered = @(@{ content = 'k' * 800 }) })
  # The first request: its system prompt and tools, and its messages, the files among them
  (& $raw @{ type = 'api-request-shape'; shapeHash = 's1'; shape = @{ system = @(@{ type = 'text'; text = 's' * 4000 }); tools = $tools } })
  (& $raw @{ type = 'api-request-blob'; hash = 'm1'; message = @{ role = 'user'; content = @(@{ type = 'text'; text = 'b' * 16000 }) } })
  (& $raw @{ type = 'api-request'; querySource = 'repl_main_thread'; shapeHash = 's1'; keep = 0; tail = @('m1') })
  (ConvertTo-Record (Get-Reply 9000))
  # Another project's CLAUDE.md pulled in, and a memory file read
  (ConvertTo-Record @{ type = 'attachment'; attachment = @{ type = 'nested_memory'; path = $ledger; displayPath = 'CLAUDE.md'; content = @{ path = $ledger; type = 'Project'; content = 'n' * 20000 } } })
  (ConvertTo-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'tool_use'; id = 'r1'; name = 'Read'; input = @{ file_path = "$memory/feedback_x.md" } }) } })
  (ConvertTo-Record @{ type = 'user'; message = @{ role = 'user'; content = @(@{ type = 'tool_result'; tool_use_id = 'r1'; content = 'r' * 1200 }) } })
  (ConvertTo-Record (Get-Reply 40000))
)
[IO.File]::WriteAllText($transcript, ($lines -join "`n") + "`n")
function Add-Records([string[]]$more) { [IO.File]::AppendAllText($transcript, ($more -join "`n") + "`n") }

# What Terminal's statusline writes beside a session's summary
# (the window's size `null` until Claude Code has said it)
$usage = Join-Path $sums "$sid.usage.json"
function Set-Usage([string]$Window) {
  Write-FileWhole -Path $usage -Text "{`"usd`":1.25,`"window`":$Window}"
}
Set-Usage 'null'

$kinds = 'user', 'import', 'project', 'project', 'memory_index', 'elsewhere', 'memory', 'system', 'tools', 'skills', 'conversation', 'free'
$colours = @{ user = '#8A8A9E'; import = '#8A8A9E'; project = '#8A8A9E'; memory_index = '#F2E86D'; memory = '#F2E86D'; elsewhere = '#FFBF00'
  system = '#5E5E6E'; tools = '#5E5E6E'; skills = '#5E5E6E'; conversation = '#7A9CC6'; free = '#2A2A36' }
function Get-Kinds($bar) { @($bar.sections | ForEach-Object kind) }
function Get-Section($bar, [string]$kind) { @($bar.sections | Where-Object kind -eq $kind)[0] }

# The open tab's panes' sizes: none may change as the bar widens or the pane opens (Lachlan's 7a)
function Get-PaneSizes($s) { @($s.panes | ForEach-Object { "$($_.id) $($_.cols)x$($_.rows) $($_.rect.w)x$($_.rect.h)" }) -join '; ' }
function Test-PaneSizes($s, [string]$Before, [string]$When) {
  $now = Get-PaneSizes $s
  if ($now -ne $Before) { Add-Failure "$When the panes are $now, not $Before" } else { "$When the panes are as they were: $now" }
}
# -Capture: the window grabbed as fast as PrintWindow goes for $Ms after $Act, at real speed, into
# motion-<Name>, and sheet.py's contact sheet of the right of the window, where the pane, the drawer
# and the riding scroll strip move (lbterm/motion/sheet.py, --tiles)
function Save-Motion([string]$Name, [int]$Ms, [scriptblock]$Act, $s) {
  $done = @{ Act = $false }
  $frames = Get-Frames $w $Ms -Raw -Each { param($ms) if (-not $done.Act) { & $Act; $done.Act = $true } }
  $dir = Join-Path $T.Out "motion-$Name"
  Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $dir | Out-Null
  Save-Frames $frames -Dir $dir -Window $w
  Remove-Frames $frames
  $bx = $s.context_bar.rect
  $x = [Math]::Max(0, $bx.x - 540)
  $sheet = Join-Path $PSScriptRoot '..\motion\sheet.py'
  python $sheet $dir --tiles --count 16 --crop $x $bx.y ($s.size.width - $x) ([Math]::Min(260, $bx.h)) --zoom 1 | Out-Host
  "$($frames.Count) frames of the $Name in $dir"
}
$script:seen = @{}
$script:sawWiden = $false
$script:sawClose = $false

# The sections' rects lie one under the next from the track's top, each run once (neighbours of one
# colour can share one), and end where the filled part does: what's in use of the window, to the
# pixel
function Test-Stacked($bar, [string]$When) {
  $track = $bar.track
  $want = [Math]::Round([double]$bar.in_use * $track.h / $bar.window, [MidpointRounding]::AwayFromZero)
  if ($bar.filled -ne $want) { Add-Failure "$When`: the filled part ends $($bar.filled) px down, not $want ($($bar.in_use) of $($bar.window) in $($track.h) px)" }
  $y = $track.y
  $last = $null
  foreach ($s in $bar.sections) {
    if ($s.kind -eq 'free') { if ($s.rect) { Add-Failure "$When`: free space has a rect" }; continue }
    if (-not $s.rect) { if ($s.tokens -gt 0) { Add-Failure "$When`: $($s.kind) has $($s.tokens) tokens and no rect" }; continue }
    if ($last -and $s.rect.y -eq $last.y -and $s.rect.h -eq $last.h) { continue }
    if ($s.rect.y -ne $y) { Add-Failure "$When`: $($s.kind) starts at $($s.rect.y), not $y under the one before" }
    if ($s.rect.x -ne $track.x -or $s.rect.w -ne $track.w) { Add-Failure "$When`: $($s.kind) isn't across the track: x $($s.rect.x) w $($s.rect.w), the track's $($track.x) $($track.w)" }
    $y = $s.rect.y + $s.rect.h
    $last = $s.rect
  }
  if ($y - $track.y -ne $bar.filled) { Add-Failure "$When`: the sections end $($y - $track.y) px down, the filled part $($bar.filled)" }
}
function Format-Bar($bar) { ($bar.sections | ForEach-Object { "$($_.kind) $($_.tokens)$(if ($_.rect) { " @$($_.rect.y)+$($_.rect.h)" })" }) -join ', ' }

Invoke-Test {
  $w = Open-TestWindow -PostedPointer -Program (New-ClaudeSessionProgram -Sid $sid) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_prompt_history = nil
rawset(config, 'lbterm_tab_panel', { enabled = true, open_delay_ms = 0 })
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath $projects)' })
rawset(config, 'lbterm_objectives', {})
rawset(config, 'lbterm_context_bar', {})
$(if ($MotionOff) { "config.lbterm_motion = { enabled = 'off' }" })
config.initial_cols = 110
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  # 1. The bar waits for the window's size, which a change to the usage file alone brings: with
  # the pill off, nothing else repaints for it (lbterm/PITFALLS.md, 205). Then it's in the
  # objectives strip's room, its sections in order
  $s = Wait-Change { param($s) $s.context_bar.waiting -eq 'Waiting for the context window''s size' } -What 'the bar waiting for the window''s size' -TimeoutSec 20
  if ($s.strip.context) { Add-Failure "the context pill shows ('$($s.strip.context)'), whose repaint would carry the bar's" }
  Set-Usage '200000'
  $s = Wait-Change { param($s) $s.context_bar -and (Get-Kinds $s.context_bar).Count -eq $kinds.Count -and $s.context_bar.in_use -eq 40000 } -What 'the bar with its twelve sections' -TimeoutSec 20
  $bar = $s.context_bar
  "bar: $(Format-Bar $bar); filled $($bar.filled) of $($bar.track.h) px; scale $($bar.scale)"
  if ($s.objectives) { Add-Failure 'the objectives strip is drawn beside the bar, which takes its room' }
  if ($bar.session -ne $sid) { Add-Failure "the bar stands for '$($bar.session)', not the tab's session" }
  $got = Get-Kinds $bar
  if (($got -join ',') -ne ($kinds -join ',')) { Add-Failure "the sections are $($got -join ', '), not $($kinds -join ', ')" }
  foreach ($sec in $bar.sections) {
    if ($colours[$sec.kind] -and $sec.colour -ne $colours[$sec.kind]) { Add-Failure "$($sec.kind) is $($sec.colour), not $($colours[$sec.kind])" }
  }
  $buttons = @($bar.sections | Where-Object button -eq 'remove' | ForEach-Object kind)
  if (($buttons -join ',') -ne 'import,memory') { Add-Failure "the Remove buttons are on $($buttons -join ', '), not the import and the memory file" }
  if ((Get-Section $bar 'elsewhere').note -ne 'Leaves at the next compaction') { Add-Failure "the pulled-in file's note is '$((Get-Section $bar 'elsewhere').note)'" }
  if ($bar.estimate -or $bar.window -ne 200000 -or -not $bar.scale) { Add-Failure "the bar's numbers: estimate $($bar.estimate), window $($bar.window), scale $($bar.scale)" }
  Test-Stacked $bar 'at the start'
  $floor = [Math]::Round(3 * $s.size.dpi / 96)
  $small = Get-Section $bar 'memory_index'
  if ($small.rect.h -lt $floor) { Add-Failure "the memory index, $($small.tokens) tokens, is $($small.rect.h) px, under the $floor px floor" }
  $right = ($s.panes | ForEach-Object { $_.rect.x + $_.rect.w } | Measure-Object -Maximum).Maximum
  if ($right -gt $bar.rect.x) { Add-Failure "a pane reaches x $right, past the bar's left edge at $($bar.rect.x)" }
  $edge = $s.size.width - ($bar.rect.x + $bar.rect.w)
  if ($edge -lt 0 -or $edge -gt 12 * $s.size.dpi / 96) { Add-Failure "the bar ends $edge px from the window's right edge, not a gutter" }
  (Get-Shot $w -Name 'bar').Dispose()

  # 2. The pointer on it: named, and no layout for the ring's light; then nothing while it rests
  $layouts = $bar.layouts
  $c = Get-RectCenter $bar.rect
  Send-Mouse $w Move $c[0] $c[1]
  $s = Wait-Change { param($s) $s.pointer.over -eq 'context.bar' } -What 'the pointer on the bar'
  if ($s.context_bar.layouts -ne $layouts) { Add-Failure "the sections were laid out again ($layouts to $($s.context_bar.layouts)) for the pointer's light" } else { "the pointer's light laid nothing out (layouts $layouts)" }
  # Resting there widens it and opens the pane (2b has them); leaving closes them, a frame each
  # for the pane sliding back, the bar narrowing and their visuals put away
  Send-Leave $w
  $s = Wait-Change { param($s) -not $s.context_bar.widened -and -not $s.context_bar.pane -and $s.context_bar.ride -eq 0 } -What 'the bar back at rest' -TimeoutSec 10
  $frame = Get-StateFrame $w
  Wait-Ms 1500
  $after = Get-StateFrame $w
  if ($after -ne $frame) { Add-Failure "the window painted $($after - $frame) frame(s) while nothing changed" } else { "no frame at rest (frame $frame)" }

  # 2b. Resting on a section: the bar widens over the terminal, a drawer of each section's tokens,
  # then the pane slides out beside it, the scroll strip riding on their left edge. No pane is
  # resized by any of it (CONTEXT.md, "At rest and widened", "The pane")
  $sizes = Get-PaneSizes $s
  $rest = $s.scroll_strip.strip
  if (-not $rest) { Add-Failure 'no scroll strip beside the bar' }
  $user = Get-Section $bar 'user'
  $c = Get-RectCenter $user.rect
  if ($Capture) {
    # At real speed: the rest, the bar widening, the pane after it (Save-Motion)
    Save-Motion 'open' 1400 { Send-Mouse $w Move $c[0] $c[1] } $s
  } else {
    # The bar widened alone first: the pointer resting on its ring above the track, where no
    # section is, so no pane opens; the thin sections' pane comes out from under the bar's moving
    # edge in the same slide (round 4's stage 6, Lachlan's E9)
    Send-Mouse $w Move ([int]($bar.rect.x + $bar.rect.w / 2)) ([int]($bar.rect.y + 1))
    $s = Wait-Change { param($s) $s.context_bar.widened } -What 'the bar widened' -TimeoutSec 10 -OnState { param($sw) if ($sw.waits | Where-Object name -eq 'context.widen') { $script:sawWiden = $true }; foreach ($a in $sw.animations) { $script:seen[$a.name] = $true } }
    $s = Wait-Settled -What 'the widening to settle'
    if ($s.context_bar.pane) { Add-Failure 'the pane opened under a pointer resting on no section' }
    if ($s.context_bar.widened.thin -and -not $MotionOff -and -not $script:seen['context.thin']) { Add-Failure "the thin sections' pane came out with no context.thin motion" }
    (Get-Shot $w -Name 'widened').Dispose()
    Send-Mouse $w Move $c[0] $c[1]
  }
  $s = Wait-Change { param($s) $s.context_bar.widened } -What 'the bar widened' -TimeoutSec 10 -OnState { param($sw) if ($sw.waits | Where-Object name -eq 'context.widen') { $script:sawWiden = $true } }
  if (-not $script:sawWiden -and -not $Capture) { Add-Failure 'no context.widen wait while the pointer rested on the bar' }
  # The bar itself widens to the closed rail's width (Lachlan's 4b of 2026-11-09), its right edge
  # the bar's, each section's number on it where it has a line's room, the rest in the pane as wide
  # as the bar beside it (`thin`), none overlapping, none outside the bar or that pane
  $d = $s.context_bar.widened
  $rail = [Math]::Round(68 * $s.size.dpi / 96)
  $bx = $s.context_bar.rect
  if ($d.rect.x -ne $bx.x + $bx.w - $rail) { Add-Failure "the widened bar starts at $($d.rect.x), not the closed rail's $rail px left of the bar's right edge ($($bx.x + $bx.w - $rail))" }
  if ($d.rect.x + $d.rect.w -ne $bx.x + $bx.w) { Add-Failure "the widened bar ends at $($d.rect.x + $d.rect.w), not on the bar's right edge $($bx.x + $bx.w)" }
  $texts = @($d.labels | ForEach-Object { "$($_.section):$($_.text)" })
  "widened: $($d.rect.x),$($d.rect.y) $($d.rect.w)x$($d.rect.h); numbers $($texts -join ', ')$(if ($d.thin) { "; thin sections' pane $($d.thin.x),$($d.thin.y) $($d.thin.w)x$($d.thin.h)" })"
  if ($d.labels.Count -lt 4) { Add-Failure "the widened bar has $($d.labels.Count) numbers" }
  if ($d.thin -and ($d.thin.w -ne $d.rect.w -or [Math]::Abs($d.thin.x + $d.thin.w - $d.rect.x) -gt 2)) { Add-Failure "the thin sections' pane is at $($d.thin.x) $($d.thin.w) wide, not as wide as the bar ($($d.rect.w)) and on its left edge" }
  $onBar = @($d.labels | Where-Object { $_.rect.x -ge $d.rect.x })
  $beside = @($d.labels | Where-Object { $_.rect.x -lt $d.rect.x })
  if ($onBar.Count -lt 1) { Add-Failure 'no number on its own section' }
  if ($beside.Count -and -not $d.thin) { Add-Failure "numbers $(@($beside | ForEach-Object section) -join ',') beside the bar with no pane for them" }
  foreach ($set in @(, $onBar) + @(, $beside)) {
    $prev = $null
    foreach ($l in ($set | Sort-Object { $_.rect.y })) {
      if ($prev -and $l.rect.y -lt $prev.rect.y + $prev.rect.h) { Add-Failure "number $($l.section) at y $($l.rect.y) overlaps the one above it" }
      $prev = $l
    }
  }
  foreach ($l in $onBar) {
    if ($l.rect.x + $l.rect.w -gt $d.rect.x + $d.rect.w) { Add-Failure "number $($l.section) runs past the widened bar's edge" }
  }
  foreach ($l in $beside) {
    if ($l.rect.x -lt $d.thin.x -or $l.rect.x + $l.rect.w -gt $d.thin.x + $d.thin.w) { Add-Failure "number $($l.section) is outside the thin sections' pane" }
  }
  Test-PaneSizes $s $sizes 'with the bar widened'
  $s = Wait-Change { param($s) $s.context_bar.pane } -What 'the pane open' -TimeoutSec 10
  $p = $s.context_bar.pane
  $cb = $s.context_bar
  "pane: $($p.rect.x),$($p.rect.y) $($p.rect.w)x$($p.rect.h); '$($p.heading)'; $(@($p.rows | ForEach-Object { "$($_.kind) $($_.tokens)$(if ($_.button) { " [$($_.button)]" })$(if ($_.lit) { ' (lit)' })" }) -join ', ')"
  if ([Math]::Abs($p.rect.x + $p.rect.w - $d.rect.x) -gt 2) { Add-Failure "the pane ends at $($p.rect.x + $p.rect.w), not on the widened bar's left edge $($d.rect.x)" }
  if ($p.rect.y -ne $bx.y -or $p.rect.h -ne $bx.h) { Add-Failure "the pane isn't as tall as the bar: y $($p.rect.y) h $($p.rect.h), the bar's $($bx.y) $($bx.h)" }
  if ($p.heading -ne '40k of 200k in use · 20%') { Add-Failure "the pane's heading is '$($p.heading)'" }
  $order = @($p.rows | ForEach-Object section)
  if (($order -join ',') -ne ((0..($order.Count - 1)) -join ',') -or $order.Count + $p.left_out -ne $cb.sections.Count) { Add-Failure "the pane's rows are sections $($order -join ','), $($p.left_out) left out, of $($cb.sections.Count)" }
  foreach ($r in $p.rows) {
    $sec = $cb.sections[$r.section]
    if ($sec.button -and $r.button -ne 'Remove') { Add-Failure "$($sec.kind)'s row has no Remove button" }
    if (-not $sec.button -and $r.button) { Add-Failure "$($sec.kind)'s row has a button '$($r.button)'" }
    if ($sec.path -and -not $r.path) { Add-Failure "$($sec.kind)'s row has no path" }
    if ($sec.note -and $r.note -ne $sec.note) { Add-Failure "$($sec.kind)'s row's note is '$($r.note)', not '$($sec.note)'" }
  }
  $userRow = @($p.rows | Where-Object section -eq 0)[0]
  if (-not $userRow.path -or $userRow.path -notlike '*CLAUDE.md') { Add-Failure "the user's row's path is '$($userRow.path)'" }
  if (-not $userRow.lit -or $cb.lit -ne 0) { Add-Failure "the row of the section under the pointer isn't lit (lit $($cb.lit))" }
  if ($s.scroll_strip -and $cb.ride -ne $p.rect.x - $bx.x) { Add-Failure "the scroll strip rides $($cb.ride) px, not to the pane's left edge ($($p.rect.x - $bx.x))" }
  if ($s.scroll_strip -and $rest -and $s.scroll_strip.strip.x -ne $rest.x + $cb.ride) { Add-Failure "the scroll strip is at x $($s.scroll_strip.strip.x), not $($rest.x + $cb.ride) on the pane's edge" }
  $anims = @($s.animations | ForEach-Object name)
  Test-PaneSizes $s $sizes 'with the pane open'
  (Get-Shot $w -Name 'pane').Dispose()

  # 2c. Along the bar to the conversation: the pane stays where it is and lights that row; then
  # onto the pane and its buttons
  # Widened, the sections are where the widened bar lays them out (a line's room each), so the
  # pointer goes to the conversation's number on it
  $i = [array]::IndexOf(@($cb.sections | ForEach-Object kind), 'conversation')
  $num = @($s.context_bar.widened.labels | Where-Object { $_.section -eq $i -and $_.rect.x -ge $s.context_bar.widened.rect.x })[0]
  if (-not $num) { Add-Failure "the conversation's number isn't on the widened bar"; $num = @{ rect = (Get-Section $cb 'conversation').rect } }
  $c = Get-RectCenter $num.rect
  Send-Mouse $w Move $c[0] $c[1]
  $s = Wait-Change { param($s) $s.context_bar.lit -eq $i } -What 'the conversation lit' -TimeoutSec 5
  if (-not $s.context_bar.pane -or $s.context_bar.pane.rect.x -ne $p.rect.x) { Add-Failure 'the pane moved or closed as the pointer moved along the bar' }
  if (-not (@($s.context_bar.pane.rows | Where-Object lit)[0].section -eq $i)) { Add-Failure "the lit row isn't the conversation's" }
  if ($s.context_bar.layouts -ne $cb.layouts) { Add-Failure "the sections were laid out again as the pointer moved" }
  $row = @($s.context_bar.pane.rows | Where-Object button)[0]
  $c = Get-RectCenter $row.rect
  Send-Mouse $w Move ($c[0] - 40) $c[1]
  $s = Wait-Change { param($s) $s.pointer.over -eq 'context.pane' } -What 'the pointer on the pane'
  if ($s.context_bar.lit -ne $row.section) { Add-Failure "on the pane the lit section is $($s.context_bar.lit), not its row's $($row.section)" }
  Send-Mouse $w Move ($row.rect.x + $row.rect.w - 30) $c[1]
  $s = Wait-Change { param($s) $s.pointer.over -eq 'context.button' } -What 'the pointer on a Remove button'

  # 2d. Away: the pane slides back after the grace, then the bar narrows, and the scroll strip
  # rides home; nothing resized
  if ($Capture) {
    Save-Motion 'close' 1400 { Send-Mouse $w Move 120 200 } $s
  } else {
    Send-Mouse $w Move 120 200
  }
  $s = Wait-Change { param($s) -not $s.context_bar.pane -and -not $s.context_bar.widened } -What 'the pane and drawer closed' -TimeoutSec 10 -OnState { param($sw) if ($sw.waits | Where-Object name -eq 'context.close') { $script:sawClose = $true }; foreach ($a in $sw.animations) { $script:seen[$a.name] = $true } }
  # A capture takes the frames the waits and the motions were in
  if (-not $script:sawClose -and -not $Capture) { Add-Failure 'no context.close wait as the pointer left' }
  if ($s.context_bar.ride -ne 0) { Add-Failure "the scroll strip still rides $($s.context_bar.ride) px" }
  if ($s.scroll_strip -and $rest -and $s.scroll_strip.strip.x -ne $rest.x) { Add-Failure "the scroll strip is at $($s.scroll_strip.strip.x), not home at $($rest.x)" }
  $moved = @($anims + @($script:seen.Keys)) | Sort-Object -Unique
  "motions seen: $($moved -join ', ')"
  foreach ($m in 'context.pane', 'context.widen') { if ($moved -notcontains $m -and -not $Capture -and -not $MotionOff) { Add-Failure "no $m motion seen" } }
  if ($MotionOff -and $moved.Count) { Add-Failure "with motion off, motions played: $($moved -join ', ')" }
  Test-PaneSizes $s $sizes 'after they closed'
  $s = Wait-Settled -What 'the window to settle'

  # 2e. Turned round: open again, leave, and come back while the pane slides in. It opens again
  # at once from where it is, with no rest waited out again (lbterm_panel::Hover's reopen)
  if (-not $MotionOff) {
    $c = Get-RectCenter $user.rect
    Send-Mouse $w Move $c[0] $c[1]
    $s = Wait-Change { param($s) $s.context_bar.pane } -What 'the pane open again' -TimeoutSec 10
    Send-Mouse $w Move 120 200
    $s = Wait-State { param($sw) @($sw.animations | Where-Object name -eq 'context.pane').Count -gt 0 } -Window $w -What 'the pane sliding back' -TimeoutSec 10
    $script:reopenWait = $false
    Send-Mouse $w Move $c[0] $c[1]
    $s = Wait-Change { param($s) $s.context_bar.pane -and $s.context_bar.widened } -What 'the pane back out' -TimeoutSec 10 -OnState { param($sw) if ($sw.waits | Where-Object name -eq 'context.open') { $script:reopenWait = $true } }
    if ($script:reopenWait) { Add-Failure 'coming back while the pane slid in waited out its rest again' } else { 'coming back while it slid in opened it again at once' }
    Send-Mouse $w Move 120 200
    $s = Wait-Change { param($s) -not $s.context_bar.pane -and -not $s.context_bar.widened -and $s.context_bar.ride -eq 0 } -What 'closed again' -TimeoutSec 10
  }

  # 2f. Turned round going back: the bar widened alone, left, and come back onto the widened bar's
  # left, off the bar at rest, while the thin numbers' pane slides in. Going back is one commit
  # since round 3's stage 3: the thin pane's slide in, and the bar's narrowing held in DComp until
  # it's done (lbterm_extras::context_bar::slide_wait), the card taking the pointer until then
  # (Out::takes_pointer). So the pointer there opens it again at once, no rest waited out, and it
  # goes back whole once left again. It guards against that path breaking, not against its own
  # risk: it passes on 8ff322c6a, before round 3, too (there the card took the pointer through the
  # thin pane's 150 ms by `widened = held || showing[3]`), and nothing here reads the held slide's
  # retarget, whose speed (from rest, Tween::velocity) only compositor.rs's
  # a_tween_waiting_on_its_delay_is_at_rest holds
  if (-not $MotionOff) {
    $s = Wait-Settled -What 'the window to settle before turning round'
    $bar = $s.context_bar
    Send-Mouse $w Move ([int]($bar.rect.x + $bar.rect.w / 2)) ([int]($bar.rect.y + 1))
    $s = Wait-Change { param($s) $s.context_bar.widened } -What 'the bar widened again' -TimeoutSec 10
    $s = Wait-Settled -What 'the widening to settle again'
    $d = $s.context_bar.widened
    if (-not $d.thin) {
      'no thin sections at this size, so no thin pane to turn round on: 2f skipped'
    } else {
      Send-Mouse $w Move 120 200
      $s = Wait-State { param($sw) @($sw.animations | Where-Object name -eq 'context.thin').Count -gt 0 } -Window $w -What "the thin numbers' pane sliding in" -TimeoutSec 10
      $script:rewiden = $false
      Send-Mouse $w Move ([int]($d.rect.x + 3)) ([int]($d.rect.y + 1))
      $s = Wait-Settled -What 'the turn to settle' -OnState { param($sw) if ($sw.waits | Where-Object name -eq 'context.widen') { $script:rewiden = $true } }
      if ($script:rewiden) { Add-Failure "coming back onto the widened bar while the thin pane slid in waited out its rest again" }
      if (-not $s.context_bar.widened) { Add-Failure "coming back onto the widened bar while the thin pane slid in left it narrowed" }
      elseif (-not $s.context_bar.widened.thin) { Add-Failure "the bar opened again without its thin numbers' pane" }
      else { "coming back while the thin pane slid in opened the bar again at once: $($s.context_bar.widened.rect.x),$($s.context_bar.widened.rect.y) $($s.context_bar.widened.rect.w)x$($s.context_bar.widened.rect.h)" }
      if ($s.context_bar.pane) { Add-Failure 'the pane opened under a pointer on the widened bar''s ring' }
      Send-Mouse $w Move 120 200
      $s = Wait-Change { param($s) -not $s.context_bar.widened -and $s.context_bar.ride -eq 0 } -What 'gone back again' -TimeoutSec 10
      $s = Wait-Settled -What 'the window to settle after going back again'
      if ($s.scroll_strip -and $rest -and $s.scroll_strip.strip.x -ne $rest.x) { Add-Failure "after turning round the scroll strip is at $($s.scroll_strip.strip.x), not home at $($rest.x)" }
    }
  }

  # 2g. For the look (lbterm.ps1 look): the bar widened alone at real speed, caught as it slides,
  # saved beside the stills as slide-<ms>.png (ms from the pointer's arrival, the rest being 150),
  # for Lachlan to judge by eye against the build before: his E9 of 2026-11-10 has the thin
  # numbers' pane come out from under the bar's moving edge in the bar's own slide, where before
  # it slid out after it. PrintWindow has DWM draw the window's tree at the call, so each says
  # where DWM had the slides then (FLOWS.md, "The context bar's widening"). Nothing here judges
  # them: their times move with the PC, and the look is his
  if (-not $MotionOff -and -not $Capture) {
    $s = Wait-Settled -What 'the window to settle before the look'
    $bar = $s.context_bar
    $done = @{ Moved = $false }
    $frames = Get-Frames $w 900 -Each { param($ms) if (-not $done.Moved) { Send-Mouse $w Move ([int]($bar.rect.x + $bar.rect.w / 2)) ([int]($bar.rect.y + 1)); $done.Moved = $true } }
    foreach ($at in 210, 270, 350, 470, 650) {
      $f = $frames | Sort-Object { [Math]::Abs($_.Ms - $at) } | Select-Object -First 1
      if ($f) { Save-Shot -Shot $f.Shot -Name "slide-$at" }
    }
    "the widening caught for the look at $(@($frames | ForEach-Object { [int]$_.Ms }) -join ', ') ms"
    Remove-Frames $frames
    Send-Mouse $w Move 120 200
    $s = Wait-Change { param($s) -not $s.context_bar.widened -and $s.context_bar.ride -eq 0 } -What 'gone back after the look' -TimeoutSec 10
    $s = Wait-Settled -What 'the window to settle after the look'
  }

  # 3. A bigger reply: free shrinks, the filled part goes further down
  $before = $s.context_bar
  Add-Records @((ConvertTo-Record (Get-Reply 80000)))
  $s = Wait-Change { param($s) $s.context_bar.in_use -eq 80000 } -What 'the bar at 80k in use' -TimeoutSec 10
  $bar = $s.context_bar
  "after the reply: $(Format-Bar $bar); filled $($bar.filled)"
  if ((Get-Section $bar 'free').tokens -ne 120000) { Add-Failure "free is $((Get-Section $bar 'free').tokens), not 120000" }
  if ($bar.filled -le $before.filled) { Add-Failure "the filled part didn't grow: $($before.filled) to $($bar.filled)" }
  if ($bar.layouts -le $before.layouts) { Add-Failure "the bar wasn't laid out again for the reply" }
  Test-Stacked $bar 'after the reply'

  # 4. A compaction: the pulled-in file and the memory file leave, an estimate until the next reply
  Add-Records @(
    (ConvertTo-Record @{ type = 'system'; subtype = 'compact_boundary'; content = 'Conversation compacted'; compactMetadata = @{ trigger = 'manual'; preTokens = 80000; postTokens = 3000 } })
    (ConvertTo-Record (Get-Instructions))
  )
  $s = Wait-Change { param($s) $s.context_bar.estimate } -What 'the bar an estimate after the compaction' -TimeoutSec 10
  $bar = $s.context_bar
  "compacted: $(Format-Bar $bar); in use about $($bar.in_use)"
  $left = Get-Kinds $bar
  if ($left -contains 'elsewhere' -or $left -contains 'memory') { Add-Failure "the pulled-in file or the memory file is still on the bar after the compaction: $($left -join ', ')" }
  if ((Get-Section $bar 'conversation').tokens -ne 3000) { Add-Failure "the conversation after the compaction is $((Get-Section $bar 'conversation').tokens), not its postTokens 3000" }
  Test-Stacked $bar 'after the compaction'
  (Get-Shot $w -Name 'compacted').Dispose()
  Add-Records @((ConvertTo-Record (Get-Reply 12000)))
  $s = Wait-Change { param($s) -not $s.context_bar.estimate -and $s.context_bar.in_use -eq 12000 } -What 'the bar exact again at the next reply' -TimeoutSec 10
  Test-Stacked $s.context_bar 'after the next reply'
  "the next reply: $(Format-Bar $s.context_bar)"

  # 5. The wheel: more rows than the pane has room for
  $reads = for ($n = 1; $n -le 24; $n++) {
    (ConvertTo-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'tool_use'; id = "w$n"; name = 'Read'; input = @{ file_path = "$memory/note_$n.md" } }) } })
    (ConvertTo-Record @{ type = 'user'; message = @{ role = 'user'; content = @(@{ type = 'tool_result'; tool_use_id = "w$n"; content = 'w' * (300 + 40 * $n) }) } })
  }
  Add-Records @($reads + (ConvertTo-Record (Get-Reply 30000)))
  $s = Wait-Change { param($s) @($s.context_bar.sections | Where-Object kind -eq 'memory').Count -eq 24 -and $s.context_bar.in_use -eq 30000 } -What 'the bar with 24 memory files' -TimeoutSec 10
  $cb = $s.context_bar
  $count = @($cb.sections).Count
  $c = Get-RectCenter (Get-Section $cb 'user').rect
  Send-Mouse $w Move $c[0] $c[1]
  $s = Wait-Change { param($s) $s.context_bar.pane -and $s.context_bar.widened } -What "the pane open over $count sections" -TimeoutSec 10
  $p = $s.context_bar.pane
  # The totals (every section without a path: Claude Code's parts, the conversation, free) pinned
  # at the foot under a rule, and only the files scroll (Lachlan's 3a of 2026-11-09)
  $totals = @(0..($count - 1) | Where-Object { -not $cb.sections[$_].path })
  $files = $count - $totals.Count
  $pinnedRows = @($p.rows | Where-Object { $totals -contains $_.section })
  $fileRows = @($p.rows | Where-Object { $totals -notcontains $_.section })
  $shown = $fileRows.Count
  "the pane: $shown of $files files shown, $($p.left_out) left out, $($p.above) above; pinned $($p.pinned): $(@($pinnedRows | ForEach-Object kind) -join ', ')"
  if (-not $p.pinned) { Add-Failure "the pane's rows don't fit and its totals aren't pinned" }
  if ((@($pinnedRows | ForEach-Object section) -join ',') -ne ($totals -join ',')) { Add-Failure "the pinned rows are sections $(@($pinnedRows | ForEach-Object section) -join ','), not the totals $($totals -join ',')" }
  if ($fileRows.Count -and $pinnedRows.Count -and $pinnedRows[0].rect.y -lt $fileRows[-1].rect.y + $fileRows[-1].rect.h + 2) { Add-Failure "the pinned totals start at $($pinnedRows[0].rect.y), not under the last file shown ($($fileRows[-1].rect.y + $fileRows[-1].rect.h)) and its rule" }
  if ($pinnedRows.Count -and $p.compact -and $pinnedRows[-1].rect.y + $pinnedRows[-1].rect.h -gt $p.compact.rect.y) { Add-Failure "the last pinned total runs into Compact now" }
  if ($p.above -ne 0 -or @($p.rows)[0].section -ne 0) { Add-Failure "the pane opened scrolled: $($p.above) above, its first row section $(@($p.rows)[0].section)" }
  if ($p.left_out -lt 4 -or $shown + $p.left_out -ne $files) { Add-Failure "the pane shows $shown files and leaves $($p.left_out) out of ${files}: the test needs files it hasn't room for" }
  $drawn = $s.context_bar.widened.labels | ConvertTo-Json -Compress -Depth 5
  $c = Get-RectCenter @($p.rows)[2].rect
  Send-Mouse $w Move ($c[0] - 40) $c[1]
  $s = Wait-Change { param($s) $s.pointer.over -eq 'context.pane' } -What 'the pointer on the pane'
  Send-Wheel $w ($c[0] - 40) $c[1] -Delta -120
  $s = Wait-Change { param($s) $s.context_bar.pane.above -eq 3 } -What 'the pane three rows down' -TimeoutSec 5
  $p = $s.context_bar.pane
  if (@($p.rows)[0].section -ne 3) { Add-Failure "a notch down shows section $(@($p.rows)[0].section) first, not 3" }
  $under = @($p.rows | Where-Object { $c[1] -ge $_.rect.y -and $c[1] -lt $_.rect.y + $_.rect.h })[0]
  if (-not $under -or $s.context_bar.lit -ne $under.section -or -not $under.lit) { Add-Failure "after the wheel the lit section is $($s.context_bar.lit), not the row now under the pointer's $($under.section)" }
  if ($s.context_bar.layouts -ne $cb.layouts) { Add-Failure 'the sections were laid out again for the wheel' }
  if (($s.context_bar.widened.labels | ConvertTo-Json -Compress -Depth 5) -ne $drawn) { Add-Failure 'the widened bar changed for the wheel' }
  (Get-Shot $w -Name 'wheel').Dispose()
  for ($n = 0; $n -lt 12; $n++) { Send-Wheel $w ($c[0] - 40) $c[1] -Delta -120 }
  $s = Wait-Change { param($s) $s.context_bar.pane.left_out -eq 0 -and $s.context_bar.pane.above -gt 3 } -What 'the pane at its last page' -TimeoutSec 5
  $s = Wait-Settled -What 'the wheel to settle'
  $p = $s.context_bar.pane
  $fileRows = @($p.rows | Where-Object { $totals -notcontains $_.section })
  "at the last page: $($p.above) above, files $($fileRows[0].section) to $($fileRows[-1].section), the totals still pinned under them"
  if ($fileRows[-1].section -ne $files - 1 -or $p.left_out -ne 0) { Add-Failure "past the end the last file is section $($fileRows[-1].section) of $files, $($p.left_out) left out" }
  if ((@($p.rows | Where-Object { $totals -contains $_.section } | ForEach-Object section) -join ',') -ne ($totals -join ',')) { Add-Failure 'the totals scrolled with the files' }
  $last = $p.above
  Send-Wheel $w ($c[0] - 40) $c[1] -Delta 120
  $s = Wait-Change { param($s) $s.context_bar.pane.above -eq [Math]::Max(0, $last - 3) } -What 'a notch up from the last page' -TimeoutSec 5
  for ($n = 0; $n -lt 12; $n++) { Send-Wheel $w ($c[0] - 40) $c[1] -Delta 120 }
  $s = Wait-Change { param($s) $s.context_bar.pane.above -eq 0 -and @($s.context_bar.pane.rows)[0].section -eq 0 } -What 'the pane back at the top' -TimeoutSec 5
  Send-Wheel $w ($c[0] - 40) $c[1] -Delta -120
  $s = Wait-Change { param($s) $s.context_bar.pane.above -eq 3 } -What 'the pane three rows down again' -TimeoutSec 5
  Send-Mouse $w Move 120 200
  $s = Wait-Change { param($s) -not $s.context_bar.pane -and -not $s.context_bar.widened } -What 'the pane closed' -TimeoutSec 10
  $c = Get-RectCenter (Get-Section $s.context_bar 'user').rect
  Send-Mouse $w Move $c[0] $c[1]
  $s = Wait-Change { param($s) $s.context_bar.pane -and $s.context_bar.widened } -What 'the pane open again' -TimeoutSec 10
  if ($s.context_bar.pane.above -ne 0) { Add-Failure "opened again, the pane is still scrolled $($s.context_bar.pane.above) rows" }
  # -Capture grabs frames where the waits and motions are judged, so it says what went unjudged
  # rather than passing for them (lbterm/PITFALLS.md, 11)
  if ($Capture) { (Get-Test).PassedLine = 'context bar passed, captured: the widen and close waits and the motions seen not judged (-Capture); run without it to judge them' }
} -Passed "context bar passed: at rest, widened, its pane, closing, turned round, the wheel$(if ($MotionOff) { ', with motion off' })"
