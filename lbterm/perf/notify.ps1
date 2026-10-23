#Requires -Version 7
# Covers: lbterm-notify/**, wezterm-gui/src/toasts.rs, wezterm-gui/src/termwindow/notify.rs, wezterm-toast-notification/src/windows.rs, term/src/terminalstate/performer.rs
<#
Sends a kitty notification (OSC 99) from a test window and reads the GUI's log to see that a
Windows toast was shown, and what Windows says about LBTerm's toasts (0 = enabled). A real toast
appears in the corner of the main screen, titled "LBTerm OSC 99 test".

  pwsh -File lbterm\perf\notify.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'notify' -Bin $Bin

# A title and a body in two chunks, shown however the window stands (o=always), expiring after 6 s
$script = @'
$e = [char]27
[Console]::Write("$e]99;i=lbterm-test:d=0:o=always:w=6000;LBTerm OSC 99 test$e\")
[Console]::Write("$e]99;i=lbterm-test:p=body;A kitty notification from a test window$e\")
Start-Sleep 10
'@

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'" -Program (New-PwshProgram) `
    -Log 'lbterm_gui::toasts=debug,lbterm_gui::termwindow::notify=debug,info'
  # Painted: its pane is there to spawn beside
  Wait-State { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  New-Tab -From $first -Program (New-PwshProgram $script) | Out-Null
  # The GUI says what Windows made of the toast (was a fixed 4 s, then 1 s after the window went)
  Wait-LogLine $w 'toast .* shown' -TimeoutSec 10 | Out-Null
  Stop-TestWindow $w
  if (-not (Test-Path $w.Log)) { throw "no log for pid $($w.Pid)" }
  $lines = Get-LogLines $w 'toast'
  $lines
  if (-not ($lines -match 'shown')) { Add-Failure 'no toast was shown' }
} -Passed 'notify: a toast was shown'
