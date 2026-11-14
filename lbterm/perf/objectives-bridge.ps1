#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/objectives.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs
<#
A click on the scroll strip while the objectives cards are open (lbterm/OBJECTIVES.md, "The cards";
Lachlan's E2 of 2026-11-10), end to end, in a test window that looks like Lachlan's LBTerm
(New-TestConfig) with the tab panel, both strips and lbterm_summaries pointed at folders of its
own, the pointer posted (-PostedPointer, so it hovers until a posted leave). Its pane runs
fake-claude.ps1 with twelve messages and the transcript they match, as scroll-strip.ps1's, and its
session has an objectives file, so the objectives strip has a mark.

1. The pointer held on the objectives strip, level with the tab's mark: the list opens.
2. Over the scroll strip's third prompt mark, the bridge: the list stays open, and the scroll
   strip's card doesn't open.
3. A click there: the objectives list and detail shut within 2 s (at once, in the next frame,
   on a fixed build), and the conversation goes to message 3, its row marked, as a click with the
   cards shut does. On a build from before the fix the click does nothing: the list stays, and
   this fails waiting for it to shut.

Where the marks, the cards and the landing are is what the state file (lbterm/STATE.md) says; the
pane's text is read with `lbterm cli get-text`. Mouse and keys are posted messages, never real
input. Fails with exit 1.

  pwsh -File lbterm\perf\objectives-bridge.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'objectives-bridge' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
$folder = Join-Path $projects 'C--work-demo'
New-Item -ItemType Directory -Force $folder, $sums, $home2 | Out-Null

# The transcript: twelve messages and their answers, as fake-claude.ps1 draws them
$sid = '5b2e9c41-7d3a-4f8e-a6b0-2c9d1e4f7a35'
$start = [DateTime]::new(2026, 11, 10, 9, 0, 0, [DateTimeKind]::Utc)
$records = [Collections.Generic.List[string]]::new()
$second = 0
function Add-Record([hashtable]$r) {
  $script:second += 5
  $r.timestamp = $start.AddSeconds($script:second).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $r.isSidechain = $false
  $records.Add(($r | ConvertTo-Json -Compress -Depth 8))
}
for ($m = 1; $m -le 12; $m++) {
  Add-Record @{ type = 'user'; origin = @{ kind = 'human' }; message = @{ role = 'user'; content = "message number $m, about part $m of the work" } }
  Add-Record @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'text'; text = ((1..14 | ForEach-Object { "answer $m, line $_" }) -join "`n") }) } }
  Add-Record @{ type = 'system'; subtype = 'turn_duration'; durationMs = 12000 }
}
[IO.File]::WriteAllText((Join-Path $folder "$sid.jsonl"), ($records -join "`n") + "`n")
$objective = [ordered]@{ id = 1; text = 'Find the part that breaks'; from = 'first'; ref = $null; at = 1; status = 'open'; evidence = ''; statusAt = 1 }
$json = [ordered]@{ sessionId = $sid; updatedAt = 1; current = 1; objectives = @($objective) } | ConvertTo-Json -Depth 5 -Compress
[IO.File]::WriteAllText((Join-Path $sums "$sid.objectives.json"), $json)
[IO.File]::WriteAllText((Join-Path $sums "$sid.label.txt"), 'Bridge test')

# The scroll strip's prompt marks, top to bottom
function Prompts($s) { @($s.scroll_strip.marks | Where-Object kind -eq 'prompt' | Sort-Object { $_.rect.y }) }
# The pane's text, until a line below the top one matches `$match` (the top is the header)
function Wait-Text([string]$match, [double]$seconds = 10) {
  Wait-Until { @(Invoke-Cli get-text --pane-id $pane | Select-Object -Skip 1) -match $match } -TimeoutSec $seconds -IntervalMs 250
}

Invoke-Test {
  $fake = Join-Path $PSScriptRoot 'fake-claude.ps1'
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo `
    -Program (New-PwshProgram -File $fake -ArgumentList '-Sid', $sid, '-Messages', 12) -Lua @"
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true }
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_objectives = { open_delay_ms = 0 }
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath $projects)' }
config.initial_cols = 110
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@
  $s = Wait-State { param($s)
    $s.frame -gt 0 -and $s.scroll_strip.mode -eq 'claude' -and (Prompts $s).Count -eq 12 -and @($s.objectives.marks | Where-Object mark).Count -eq 1
  } -What 'the conversation on the scroll strip and the objectives mark' -TimeoutSec 20
  $pane = (Get-Panes)[0].pane_id
  $strip = $s.objectives.strip
  $mark = @($s.objectives.marks)[0].rect

  # 1. The list, with the pointer held on the objectives strip level with the tab's mark
  Set-PointerHold $w ([int]($strip.x + $strip.w / 2)) ([int]($mark.y + $mark.h / 2))
  $s = Wait-Change { param($s) $s.objectives.list } -What 'the objectives list open' -Animating

  # 2. Over the third prompt's mark on the scroll strip, the bridge: the list stays, no scroll
  # strip card. 600 ms is past the scroll strip card's open delay and the objectives' close delay
  $at = Get-RectCenter (Prompts $s)[2].rect
  Set-PointerHold $w $at[0] $at[1]
  Wait-Ms 600
  $s = Wait-State { $true } -What 'the state'
  if (-not $s.objectives.list) { Add-Failure 'the objectives list closed on the bridge, before any click' }
  if ($s.scroll_strip.card) { Add-Failure "the scroll strip's card opened on the bridge while the objectives list was open" }
  "on the bridge: the objectives list open, the scroll strip's card $(if ($s.scroll_strip.card) { 'open' } else { 'shut' })"

  # 3. The click: the cards shut, and the conversation goes to message 3
  Send-Click $w $at[0] $at[1]
  Wait-State { param($s) -not $s.objectives.list -and -not $s.objectives.detail } -What 'the objectives cards shut by a click on the bridge' -TimeoutSec 2 | Out-Null
  if (Wait-Text '^\s*❯ message number 3,') {
    Wait-State { param($s) $s.landed -and -not $s.scroll_strip.seeking } -What 'the landed row marked' | Out-Null
    (Get-Shot $w -Name 'bridge-click').Dispose()
    'the click on the bridge went to message 3, and the objectives cards shut'
  } else {
    Add-Failure "a click on the third prompt's mark with the objectives list open never brought message 3 into view (note: $((Wait-State { $true }).scroll_strip.note))"
  }
  Clear-PointerHold
  Send-Leave $w
} -Passed 'objectives bridge passed'
