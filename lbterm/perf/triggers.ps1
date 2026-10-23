#Requires -Version 7
# Covers: lbterm-triggers/**, wezterm-gui/src/termwindow/render/triggers.rs
<#
Shows the triggers' easter eggs (lbterm/TRIGGERS.md): a test window (no config, so the built-in
rules, with the WebGpu front end and the compositor, on the right-most screen, without focus)
gets a pane that prints a line with "love" and "LBTerm" in it, and the window is captured
(PrintWindow, which sees DirectComposition content) at four moments as the hearts rise:
target\perf\triggers\triggers-<ms>.png.

  pwsh -File lbterm\perf\triggers.ps1
#>
param([string]$Bin, [int[]]$At = @(150, 450, 900, 1500))
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'triggers' -Bin $Bin

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'", 'lbterm_compositor=true', 'initial_rows=16', 'initial_cols=70' -Program (New-PwshProgram)
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  $script = @'
'Welcome to LBTerm.'
'I love this terminal. Lovely, but not a glove or MLB.'
'cd C:\Code\LBTerm'
Start-Sleep 30
'@
  $pane = New-Tab -From $first -Program (New-PwshProgram $script)
  Invoke-Cli activate-pane --pane-id $pane | Out-Null
  # The hearts start when the line is first drawn: the state file says when they're in the air
  Wait-State { param($s) $s.hearts -gt 0 } -What 'the hearts' | Out-Null
  $shown = [Diagnostics.Stopwatch]::StartNew()
  foreach ($ms in $At) {
    $wait = $ms - $shown.ElapsedMilliseconds
    if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
    (Get-Shot $w -Name "triggers-$ms").Dispose()
  }
  "captured triggers-{$($At -join ',')}.png"
} -Passed 'triggers: captured as the hearts rise'
