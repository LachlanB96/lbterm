#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/taskbar.rs, lbterm-extras/src/taskbar.rs, wezterm-escape-parser/src/osc.rs
<#
Checks taskbar progress end to end: a test window (no config, on the right-most screen, without
focus, WebGpu front end) gets a pane that sends ConEmu's OSC 9;4 in each state through ConPTY, and the GUI's log
must show the taskbar button set to each one.

  pwsh -File lbterm\perf\taskbar-progress.ps1            # this tree's build
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'taskbar-progress' -Bin $Bin

# Each state for a second: normal 40%, error 75%, indeterminate, paused, cleared
$script = @'
$e = [char]27
foreach ($s in '1;40', '2;75', '3', '4', '0') { [Console]::Write("$e]9;4;$s$e\"); Start-Sleep -Milliseconds 1200 }
'@
$want = 'Normal(40)', 'Error(75)', 'Indeterminate', 'Paused', 'None'

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'" -Program (New-PwshProgram) -Log 'lbterm_gui::termwindow::taskbar=debug,info'
  # Painted: its pane is there to spawn beside
  Wait-State { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  New-Tab -From $first -Program (New-PwshProgram $script) | Out-Null
  # Every state in the log, or the 6 s the pane takes and some (was a fixed 8 s, then 1 s after
  # the window went)
  $read = { @(Get-LogLines $w 'taskbar progress for window \d+: (.+)$' | ForEach-Object { if ($_ -match 'taskbar progress for window \d+: (.+)$') { $Matches[1] } }) }
  Wait-Until { $got = & $read; -not @($want | Where-Object { $_ -notin $got }) } -TimeoutSec 12 -IntervalMs 200 | Out-Null
  Stop-TestWindow $w
  if (-not (Test-Path $w.Log)) { throw "no log for pid $($w.Pid)" }
  $seen = & $read
  $seen
  $missing = @($want | Where-Object { $_ -notin $seen })
  if ($missing) { Add-Failure "never shown: $($missing -join ', ')" }
} -Passed 'taskbar progress: every state reached the button'
