#Requires -Version 7
# Covers: wezterm-gui/src/restore.rs, wezterm-gui/src/overlay/restore.rs, lbterm-extras/src/restore.rs
<#
Crash restore end to end (lbterm/RESTORE.md), in test windows of their own class, so the real
note (session-com.lachlanb.lbterm.json) is never touched:

1. A test window gets two more tabs: one in C:\Windows, and one that sets the claude_session
   user var as claudehook does. LBTerm writes its note a couple of seconds later.
2. The window is killed, as a crash would end it. The note must say not clean, three tabs.
3. A new test window opens with LBTERM_RESTORE_CLAUDE naming a stub claude.cmd that echoes its
   arguments, so no real Claude Code starts (pwsh's profile would find the real one first on
   PATH). It must show the prompt (captured: restore-prompt.png).
4. Enter is posted to it: the three tabs come back, the Claude one running
   `claude --resume <id>` in its folder (captured: restore-done.png).

  pwsh -File lbterm\perf\restore.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'restore' -Bin $Bin
$stub = Join-Path $T.Out 'stub'
New-Item -ItemType Directory -Force $stub | Out-Null
$note = Join-Path $env:APPDATA "lbterm\session-$($T.Class).json"
Remove-Item $note -ErrorAction SilentlyContinue
$sid = '0b6d7c2e-6a51-4c1e-9d3a-2f1e8b7a9c40'
'@echo STUB claude %*' | Set-Content (Join-Path $stub 'claude.cmd')

# No config, as -n has it, but with crash restore on: it's what this tests (a later --config wins)
function Open-Window([hashtable]$environment = @{}) {
  Open-TestWindow -NoConfig -Set 'lbterm_crash_restore=true', "front_end='WebGpu'", 'initial_cols=90' -Cwd 'C:\Windows' `
    -Program (New-PwshProgram) -Environment $environment
}

# A pane's folder as cli list has it (a file:// URL), as a path
function ConvertFrom-CwdUrl([string]$Url) {
  try { ([uri]$Url).LocalPath.TrimEnd('\') } catch { $Url }
}

# The note as it is now, or nothing
function Read-Note { try { Get-Content $note -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $null } }

Invoke-Test {
  $first = Open-Window
  # Painted: its pane is there to spawn beside (was 1 s)
  Wait-State { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -What 'the window' | Out-Null
  $pane0 = (Get-Panes)[0].pane_id
  New-Tab -From $pane0 -Cwd 'C:\Windows\System32' -Program (New-PwshProgram) | Out-Null
  # A Claude tab, as claudehook marks one: the claude_session user var. In this tree's folder,
  # which no other tab is in. -Command, not New-PwshProgram's -EncodedCommand: procinfo reads
  # nothing of a process whose command line passes 520 characters, its folder included, and the
  # note would have none to restore the tab in
  $setVar = "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('$sid')) + [char]7); Start-Sleep 60"
  New-Tab -From $pane0 -Cwd $T.Repo -Program @('C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-Command', $setVar) | Out-Null
  # LBTerm has written the three tabs down, the Claude one with its session, 2 s after they
  # changed (was a fixed 4 s); the checks below say what's missing if it never does
  Wait-Until { $n = Read-Note; $n -and @($n.windows.tabs).Count -eq 3 -and @($n.windows.tabs | Where-Object { $_.claude_session -eq $sid }).Count } -TimeoutSec 10 -IntervalMs 100 | Out-Null
  # As a crash would end it
  Stop-TestWindow $first

  if (-not (Test-Path $note)) { throw "no note at $note" }
  $saved = Get-Content $note -Raw | ConvertFrom-Json
  $tabs = @($saved.windows | ForEach-Object { $_.tabs })
  "note: clean=$($saved.clean), $($tabs.Count) tabs, Claude in $(@($tabs | Where-Object claude_session).Count)"
  if ($saved.clean) { Add-Failure 'the note says clean after a kill' }
  if ($tabs.Count -ne 3) { Add-Failure "the note has $($tabs.Count) tabs, not 3" }
  if (-not ($tabs | Where-Object { $_.claude_session -eq $sid })) { Add-Failure 'the Claude tab lost its session id' }

  $second = Open-Window @{ LBTERM_RESTORE_CLAUDE = Join-Path $stub 'claude.cmd' }
  # The prompt is an overlay, there before the window's first frame (frame 0 names it): painted
  # once a frame has been (was 3 s)
  Wait-Change { param($x) $x.frame -gt 0 -and $x.overlay } -Animating -What 'the restore prompt painted' | Out-Null
  Get-Shot $second -Name 'restore-prompt' | ForEach-Object Dispose
  # Enter opens the picked tabs, all of them to start with
  Send-Key $second 'Enter'
  # The prompt gone and the tabs open (was 3 s); the count below says so if they never are
  try { Wait-Change { param($x) -not $x.overlay -and @($x.tabs).Count -eq 4 } -Animating -What 'the three tabs restored' -TimeoutSec 10 | Out-Null } catch { }
  # The Claude tab's folder, from a walk of its processes, and what its stub printed (pwsh loads
  # its profile first)
  $inRepo = { @(Get-Panes | Where-Object { $_.cwd -and (ConvertFrom-CwdUrl $_.cwd) -eq $T.Repo })[0] }
  Wait-Until { & $inRepo } -TimeoutSec 10 | Out-Null
  $panes = Get-Panes
  "after Enter: $($panes.Count) tabs"
  if ($panes.Count -ne 4) { Add-Failure "$($panes.Count) tabs after restoring, not 4 (the new one and three)" }
  $claudePane = & $inRepo
  if ($claudePane) {
    $read = { (Invoke-Cli get-text --pane-id $claudePane.pane_id) -join "`n" }
    Wait-Until { (& $read) -match "STUB claude --resume $sid" } -TimeoutSec 10 -IntervalMs 250 | Out-Null
    $text = & $read
    if ($text -notmatch "STUB claude --resume $sid") { Add-Failure "the Claude tab ran: $(($text | Out-String).Trim())" }
  } else { Add-Failure 'no restored tab in the Claude tab''s folder' }
  Invoke-Cli activate-tab --pane-id $panes[0].pane_id --tab-index 0 | Out-Null
  # The first tab open again (was 1 s)
  Wait-Change { param($x) $x.active_tab -eq 0 } -Animating -What 'the first tab open' | Out-Null
  Get-Shot $second -Name 'restore-done' | ForEach-Object Dispose
} -Passed 'restore: the note, the prompt and the restored tabs all as they should be'
