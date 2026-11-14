#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/context_bar.rs, wezterm-gui/src/context_edits.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/termwindow/control.rs, lbterm-extras/src/context_edits.rs, lbterm-extras/src/context.rs, lbterm-extras/src/context_bar.rs, lbterm-extras/src/state.rs, lbterm/perf/fake-prompt.ps1
<#
The context bar's buttons (lbterm/CONTEXT.md, "The buttons"): Remove, Undo and Compact now, in a
test window with the context bar on, against files of the test's own: a memory folder with its
MEMORY.md index (LF) and a memory file, a user CLAUDE.md (CRLF) importing a rule with an `@` on a
line with words round it,
all in the test's output folder, never Lachlan's. Its tab runs fake-prompt.ps1, a stand-in for
Claude Code's screen (its input box between two rules), which writes every key it's sent; the
transcript is written as Claude Code writes one, its paths those files.

1. The import's Remove: only its `@` comes off the CLAUDE.md's line, the line's words and the path
   staying as plain text (Lachlan's answer to Q2 of 2026-11-09), the file's other lines and its CRLF
   endings as they were, the rule kept; the row says the import is off, with Undo, the section stays
   on the bar, hatched, and the state file says it's removed.
2. The memory file's Remove: its line comes off MEMORY.md, the memory file kept; Undo puts it back,
   MEMORY.md byte for byte as it was, the row saying it's back with Remove again.
3. Its line taken out by hand since: Remove is refused, and the row says why.
4. Compact now dim with why while something's typed in the box, while Claude works (the pane's
   claude_status working green, as UserPromptSubmit's hook sets it, and Claude Code's compacting
   line over the box with claude_status working, as Terminal's PreCompact hook sets it) and while
   a dialog has the box's place; at an empty
   prompt it's lit, and a press types /compact and Enter at once, the keys arriving as their own
   writes, and writes no settings file (Lachlan's 1b of 2026-11-10: no settings.json beside the
   transcripts' folder, where round 3's Compact now put its rules); the pane says it's sent. The
   pane closed and opened again before the compaction: still sent, dim (the sent is the pane's
   till the compaction, not the hover pane's).
5. The compaction in the transcript, the start's files loaded again from disk, and nothing
   attached after its summary (as when the import isn't among the five files Claude Code
   restores): the import and the memory file are gone from the bar, Compact now lit again, and
   the import, its line still out, listed in the pane off the bar with Undo.
   Then another compaction, which attaches the removed import again after its summary (a `file`
   attachment, as real Claude Code writes at every compaction while it's among the five): its own
   section, `attached`, its row the import's Undo saying `Attached again after the summary, @ off
   in CLAUDE.md`. Still no settings file.
6. That Undo on the attached row: the `@` back, the CLAUDE.md byte for byte as it was, the row
   saying the import's back on.

  pwsh -File lbterm\perf\context-edits.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'context-edits' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
$files = Join-Path $T.Out 'files'
Remove-Item -Recurse -Force $projects, $sums, $files -ErrorAction SilentlyContinue
$own = (Join-Path $files 'work') -replace '\\', '/'
$memory = (Join-Path $files 'memory') -replace '\\', '/'
$user = (Join-Path $files 'user') -replace '\\', '/'
$folder = Join-Path $projects 'C--work-Edits'
New-Item -ItemType Directory -Force $folder, $sums, $own, $memory, $user | Out-Null

# The files the conversation loads: the test's own, written as each kind usually is
$rule = "$own/rules-asking.md"
$userMd = "$user/CLAUDE.md"
$index = "$memory/MEMORY.md"
$note = "$memory/feedback_x.md"
$userText = "# Global`r`n`r`nSome words about how to work.`r`n`r`nRead @$rule before you ask anything.`r`n`r`nMore words after it.`r`n"
$indexText = "# Memory`n- [Shared scratchpad](feedback_s.md) — subagents share it`n- [The x one](feedback_x.md) — why x`n- [Posted hover](feedback_p.md) — posted moves`n"
[IO.File]::WriteAllText($userMd, $userText)
[IO.File]::WriteAllText($rule, "Ask by number.`n" + ('a' * 1200))
[IO.File]::WriteAllText($index, $indexText)
[IO.File]::WriteAllText($note, "x matters`n" + ('r' * 1200))
[IO.File]::WriteAllText("$own/CLAUDE.md", "# Work`n" + ('h' * 2000))

$sid = '7c2e9b14-0d3f-4a6b-8e51-2f9a0c7d3e58'
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
# What Claude Code loads at the start, and again after a compaction: the files as they are on disk
# then, the import among them while the CLAUDE.md still has its line
function Get-Instructions {
  $list = @(@{ path = $userMd; type = 'User'; content = [IO.File]::ReadAllText($userMd) })
  if ([IO.File]::ReadAllText($userMd).Contains("@$rule")) { $list += @{ path = $rule; type = 'User'; content = [IO.File]::ReadAllText($rule) } }
  $list += @{ path = "$own/CLAUDE.md"; type = 'Project'; content = [IO.File]::ReadAllText("$own/CLAUDE.md") }
  $list += @{ path = $index; type = 'AutoMem'; content = [IO.File]::ReadAllText($index) }
  @{ type = 'attachment'; attachment = @{ type = 'instructions'; files = $list } }
}
function Get-Reply([int]$n) {
  @{ type = 'assistant'; message = @{ role = 'assistant'; model = 'm'; content = @(@{ type = 'text'; text = 'Done.' })
    usage = @{ input_tokens = 2; cache_read_input_tokens = $n - 1002; cache_creation_input_tokens = 1000; output_tokens = 50 } } }
}
$raw = { param($r) $r | ConvertTo-Json -Compress -Depth 10 }
$transcript = Join-Path $folder "$sid.jsonl"
$lines = @(
  (ConvertTo-Record @{ type = 'user'; message = @{ role = 'user'; content = 'go' } })
  (ConvertTo-Record (Get-Instructions))
  (& $raw @{ type = 'api-request-shape'; shapeHash = 's1'; shape = @{ system = @(@{ type = 'text'; text = 's' * 4000 }); tools = @(@{ name = 'Read'; description = 'd' * 3990 }) } })
  (& $raw @{ type = 'api-request-blob'; hash = 'm1'; message = @{ role = 'user'; content = @(@{ type = 'text'; text = 'b' * 9000 }) } })
  (& $raw @{ type = 'api-request'; querySource = 'repl_main_thread'; shapeHash = 's1'; keep = 0; tail = @('m1') })
  (ConvertTo-Record (Get-Reply 6000))
  (ConvertTo-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'tool_use'; id = 'r1'; name = 'Read'; input = @{ file_path = $note } }) } })
  (ConvertTo-Record @{ type = 'user'; message = @{ role = 'user'; content = @(@{ type = 'tool_result'; tool_use_id = 'r1'; content = [IO.File]::ReadAllText($note) }) } })
  (ConvertTo-Record (Get-Reply 30000))
)
[IO.File]::WriteAllText($transcript, ($lines -join "`n") + "`n")
function Add-Records([string[]]$more) { [IO.File]::AppendAllText($transcript, ($more -join "`n") + "`n") }
[IO.File]::WriteAllText((Join-Path $sums "$sid.usage.json"), '{"usd":0.5,"window":200000}')

# The stand-in's screen, and the keys it's sent
$mode = Join-Path $T.Out 'mode.txt'
$keys = Join-Path $T.Out 'keys.txt'
Set-Content -LiteralPath $mode -Value 'prompt' -NoNewline
Remove-Item -LiteralPath $keys -ErrorAction SilentlyContinue
function Set-Mode([string]$m) { [IO.File]::WriteAllText($mode, $m) }

function Get-Bar($s) { $s.context_bar }
function Get-Section($bar, [string]$kind) { @($bar.sections | Where-Object kind -eq $kind)[0] }
function Get-Row($s, [string]$kind) { @($s.context_bar.pane.rows | Where-Object kind -eq $kind)[0] }
function Get-Bytes([string]$path) { [Convert]::ToBase64String([IO.File]::ReadAllBytes($path)) }

Invoke-Test {
  $w = Open-TestWindow -PostedPointer -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-prompt.ps1') -ArgumentList '-Sid', $sid, '-Mode', $mode, '-Keys', $keys) -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
rawset(config, 'lbterm_tab_panel', { enabled = true, open_delay_ms = 0 })
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath $projects)' })
rawset(config, 'lbterm_context_bar', {})
config.initial_cols = 110
config.initial_rows = 32
config.cursor_blink_rate = 0
"@
  $s = Wait-Change { param($s) $s.context_bar -and $s.context_bar.in_use -eq 30000 -and (Get-Section $s.context_bar 'memory') } -What 'the bar with its sections' -TimeoutSec 20
  $bar = Get-Bar $s
  "bar: $(@($bar.sections | ForEach-Object { "$($_.kind) $($_.tokens)$(if ($_.button) { " [$($_.button)]" })" }) -join ', ')"
  $buttons = @($bar.sections | Where-Object button -eq 'remove' | ForEach-Object kind)
  if (($buttons -join ',') -ne 'import,memory') { Add-Failure "the Remove buttons are on $($buttons -join ', '), not the import and the memory file" }

  # The pane, opened by a rest on the user's CLAUDE.md, and kept open with the pointer on it
  $c = Get-RectCenter (Get-Section $bar 'user').rect
  Send-Mouse $w Move $c[0] $c[1]
  $s = Wait-Change { param($s) $s.context_bar.pane -and $s.context_bar.pane.compact } -What 'the pane open with Compact now' -TimeoutSec 10
  function Press-Row([string]$kind) {
    $s = Read-StateFor
    $row = Get-Row $s $kind
    # The button's at the row's right end, on its first line: a row with a note is a line taller,
    # its first line as tall as a row without one (the user's)
    $line = (Get-Row $s 'user').rect.h
    Send-Click $w ($row.rect.x + $row.rect.w - 30) ($row.rect.y + [int]($line / 2)) -Move
  }
  function Read-StateFor { Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle) }

  # 1. The import's Remove: only its @ off the CLAUDE.md's line, the words and the path kept as
  # plain text, CRLF kept, the rule kept
  Press-Row 'import'
  $s = Wait-Change { param($s) (Get-Section $s.context_bar 'import').removed -and (Get-Row $s 'import').button -eq 'Undo' } -What 'the import removed' -TimeoutSec 10
  $now = [IO.File]::ReadAllText($userMd)
  $want = $userText.Replace("@$rule", $rule)
  if ($now -ne $want) { Add-Failure "the CLAUDE.md after Remove is '$($now -replace "`r", '\r' -replace "`n", '\n')', not '$($want -replace "`r", '\r' -replace "`n", '\n')'" } else { 'Remove: only the @ off the import''s line, its words and the path kept, the other lines and CRLF endings as they were' }
  if (-not (Test-Path -LiteralPath $rule)) { Add-Failure 'the imported rule was deleted' }
  if (@(Get-ChildItem -LiteralPath $user -Force).Count -ne 1) { Add-Failure "something was left beside the CLAUDE.md: $(@(Get-ChildItem -LiteralPath $user -Force | ForEach-Object Name) -join ', ')" }
  $r = Get-Row $s 'import'
  if ($r.note -cne 'Import off in CLAUDE.md: a compaction can attach it again') { Add-Failure "the import's row says '$($r.note)'" }
  if (-not (Get-Section $s.context_bar 'import')) { Add-Failure 'the import left the bar before a compaction' }
  (Get-Shot $w -Name 'removed').Dispose()

  # 2. The memory file's Remove, then Undo: MEMORY.md byte for byte as it was
  $before = Get-Bytes $index
  Press-Row 'memory'
  $s = Wait-Change { param($s) (Get-Section $s.context_bar 'memory').removed } -What 'the memory file removed' -TimeoutSec 10
  $now = [IO.File]::ReadAllText($index)
  if ($now -ne $indexText.Replace("- [The x one](feedback_x.md) — why x`n", '')) { Add-Failure "MEMORY.md after Remove is '$now'" } else { 'Remove: the memory file''s line off MEMORY.md' }
  if (-not (Test-Path -LiteralPath $note)) { Add-Failure 'the memory file was deleted' }
  Press-Row 'memory'
  $s = Wait-Change { param($s) -not (Get-Section $s.context_bar 'memory').removed -and (Get-Row $s 'memory').button -eq 'Remove' } -What 'the memory file put back' -TimeoutSec 10
  if ((Get-Bytes $index) -ne $before) { Add-Failure "MEMORY.md after Undo isn't as it was: '$([IO.File]::ReadAllText($index))'" } else { 'Undo: MEMORY.md byte for byte as it was' }
  if ((Get-Row $s 'memory').note -ne 'Back in MEMORY.md') { Add-Failure "the memory file's row says '$((Get-Row $s 'memory').note)' after Undo" }

  # 3. Its line taken out by hand since: refused, with why
  [IO.File]::WriteAllText($index, $indexText.Replace("- [The x one](feedback_x.md) — why x`n", ''))
  $byHand = Get-Bytes $index
  Press-Row 'memory'
  $s = Wait-Change { param($s) (Get-Row $s 'memory').note -eq 'Its line isn''t in MEMORY.md any more' } -What 'Remove refused' -TimeoutSec 10
  if ((Get-Bytes $index) -ne $byHand) { Add-Failure 'a refused Remove wrote MEMORY.md' } else { 'a line gone already: refused, the row says why, the file untouched' }

  # 4. Compact now: dim with why, then typed at an empty prompt
  foreach ($case in @(@('typed', 'Something is typed in the box'), @('working', 'Claude is working'), @('spinner', 'Claude is working'), @('permission', 'A dialog has the box''s place'))) {
    Set-Mode $case[0]
    $s = Wait-Change { param($s) $s.context_bar.pane.compact.why -eq $case[1] } -What "Compact now dim: $($case[1])" -TimeoutSec 10
    if ($s.context_bar.pane.compact.enabled) { Add-Failure "Compact now is enabled with the screen '$($case[0])'" }
    $f0 = Get-StateFrame $w
    Send-Click $w ((Get-RectCenter $s.context_bar.pane.compact.rect)[0]) ((Get-RectCenter $s.context_bar.pane.compact.rect)[1]) -Move
    # The press's frame: one dim types nothing, so the window having handled it is all to wait for
    Wait-State { param($s) $s.pointer.over -eq 'context.compact' } -AfterFrame $f0 -What "the press of Compact now dim ($($case[0]))" -TimeoutSec 10 | Out-Null
    Wait-Ms 300
    if ((Test-Path -LiteralPath $keys) -and (Get-Content -LiteralPath $keys)) { Add-Failure "a press of Compact now dim ($($case[0])) typed: $((Get-Content -LiteralPath $keys) -join ',')" }
  }
  Set-Mode 'prompt'
  $s = Wait-Change { param($s) $s.context_bar.pane.compact.enabled } -What 'Compact now lit at an empty prompt' -TimeoutSec 10
  $at = Get-RectCenter $s.context_bar.pane.compact.rect
  Send-Mouse $w Move $at[0] $at[1]
  $s = Wait-Change { param($s) $s.pointer.over -eq 'context.compact' } -What 'the pointer on Compact now'
  (Get-Shot $w -Name 'compact').Dispose()
  Send-Click $w $at[0] $at[1]
  # Typed at once, writing no settings file: no rule anywhere (lbterm/CONTEXT.md, "Compact now")
  $settings = Join-Path $T.Out 'settings.json'
  $typed = Wait-Until { $k = @(Get-Content -LiteralPath $keys -ErrorAction SilentlyContinue); $k.Count -ge 9 -and $k[-1] -eq '13' } -TimeoutSec 10
  $got = @(Get-Content -LiteralPath $keys -ErrorAction SilentlyContinue)
  $text = -join ($got | Where-Object { $_ -ne '13' } | ForEach-Object { [char][int]$_ })
  if (-not $typed -or $text -ne '/compact' -or $got[-1] -ne '13') { Add-Failure "Compact now typed '$text' then $($got[-1]), not /compact and Enter" } else { 'Compact now: /compact and Enter typed at the empty prompt' }
  if (Test-Path -LiteralPath $settings) { Add-Failure "Compact now wrote $($settings): '$(Get-Content -Raw -LiteralPath $settings)'" } else { 'Compact now: no settings file written' }
  $s = Wait-Change { param($s) $s.context_bar.pane.compact.why -like 'Sent /compact*' } -What 'the pane saying it sent /compact' -TimeoutSec 5
  # The pane closed and opened again before the compaction: still sent, dim, at the empty prompt
  # (it was the window's, cleared as the pane closed, before the review of 2026-11-09)
  Send-Mouse $w Move 120 200
  Wait-Change { param($s) -not $s.context_bar.pane } -What 'the pane closed' -TimeoutSec 10 | Out-Null
  $c = Get-RectCenter (Get-Section (Get-Bar (Read-StateFor)) 'user').rect
  Send-Mouse $w Move $c[0] $c[1]
  $s = Wait-Change { param($s) $s.context_bar.pane -and $s.context_bar.pane.compact } -What 'the pane open again' -TimeoutSec 10
  if ($s.context_bar.pane.compact.enabled -or $s.context_bar.pane.compact.why -notlike 'Sent /compact*') { Add-Failure "Compact now opened again before the compaction is '$($s.context_bar.pane.compact.why)', enabled $($s.context_bar.pane.compact.enabled), not still sent" } else { 'Compact now: still sent with the pane opened again, until the compaction' }

  # 5. The compaction, the files loaded again from disk: the import and the memory file go
  Add-Records @(
    (ConvertTo-Record @{ type = 'system'; subtype = 'compact_boundary'; content = 'Conversation compacted'; compactMetadata = @{ trigger = 'manual'; preTokens = 30000; postTokens = 2500 } })
    (ConvertTo-Record (Get-Instructions))
  )
  $s = Wait-Change { param($s) $s.context_bar.estimate } -What 'the bar after the compaction' -TimeoutSec 10
  $kinds = @($s.context_bar.sections | ForEach-Object kind)
  "compacted: $($kinds -join ', ')"
  if ($kinds -contains 'import' -or $kinds -contains 'memory') { Add-Failure "the import or the memory file is still on the bar after the compaction: $($kinds -join ', ')" }
  $s = Wait-Change { param($s) $s.context_bar.pane.compact.enabled } -What 'Compact now lit again once the compaction is read' -TimeoutSec 10
  # The import's line still out: listed in the pane off the bar (a row past the sections) with Undo;
  # the memory file's Remove was refused (step 3), so it has nothing to undo and isn't listed
  $count = @($s.context_bar.sections).Count
  $gone = @($s.context_bar.pane.rows | Where-Object { $_.section -ge $count })
  "off the bar: $(@($gone | ForEach-Object { "$($_.kind) [$($_.button)] $($_.note)" }) -join '; ')"
  $r = Get-Row $s 'import'
  if ($gone.Count -ne 1 -or -not $r -or $r.button -ne 'Undo' -or $r.note -ne 'Import off in CLAUDE.md: Undo puts its @ back' -or $r.section -lt $count) { Add-Failure "after the compaction the pane lists $($gone.Count) off the bar, the import's row '$($r.note)' [$($r.button)], not the import with Undo" }
  (Get-Shot $w -Name 'gone').Dispose()

  # Another compaction, the import among the five Claude Code restores: real Claude Code attaches
  # it again after the summary, and the bar shows it as its own section, its row the import's
  Add-Records @(
    (ConvertTo-Record @{ type = 'system'; subtype = 'compact_boundary'; content = 'Conversation compacted'; compactMetadata = @{ trigger = 'manual'; preTokens = 9000; postTokens = 2500 } })
    (ConvertTo-Record @{ type = 'attachment'; attachment = @{ type = 'file'; filename = $rule; displayPath = 'rule.md'; content = @{ type = 'text' } }; rendered = @(@{ content = [IO.File]::ReadAllText($rule) }) })
    # The next turn's: the start's files from disk again, the attached copy staying beside them
    (ConvertTo-Record (Get-Instructions))
  )
  $s = Wait-Change { param($s) (Get-Row $s 'attached') -and (Get-Row $s 'user') } -What "the import attached again after a compaction's summary, and the next turn's files" -TimeoutSec 10
  $r = Get-Row $s 'attached'
  "attached again: [$($r.button)] $($r.note)"
  if ($r.button -ne 'Undo' -or $r.note -cne 'Attached again after the summary, @ off in CLAUDE.md') { Add-Failure "the attached import's row is '$($r.note)' [$($r.button)], not its Undo saying it's attached again with its @ off" }
  if (Test-Path -LiteralPath $settings) { Add-Failure "a settings file is beside the transcripts' folder after the compactions: '$(Get-Content -Raw -LiteralPath $settings)'" }
  if (@($s.context_bar.pane.rows | Where-Object { $_.section -ge @($s.context_bar.sections).Count }).Count) { Add-Failure 'the import is listed off the bar while it is on it, attached again' }

  # 6. That Undo, on the attached row: the @ back, the CLAUDE.md byte for byte as it was, the row
  # saying so
  Press-Row 'attached'
  $s = Wait-Change { param($s) (Get-Row $s 'attached').note -eq 'Import back on in CLAUDE.md' } -What 'the import put back after the compaction' -TimeoutSec 10
  if ([IO.File]::ReadAllText($userMd) -ne $userText) { Add-Failure "the CLAUDE.md after Undo past the compaction isn't as it was: '$([IO.File]::ReadAllText($userMd) -replace "`r", '\r' -replace "`n", '\n')'" } else { 'Undo after the compaction: the CLAUDE.md byte for byte as it was' }
  Send-Mouse $w Move 120 200
  Wait-Change { param($s) -not $s.context_bar.pane } -What 'the pane closed' -TimeoutSec 10 | Out-Null
} -Passed 'context bar buttons passed: Remove and Undo edited only their line or their import''s @ (the words round it kept), a gone line refused, Compact now dim with why (its working line too) and typed at once at an empty prompt with no settings file written, sent till the compaction, the compaction followed, the import attached again by the next compaction shown with its Undo, Undo past it'
