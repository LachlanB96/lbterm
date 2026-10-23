#Requires -Version 7
<#
Checks that the compositor draws what upstream's renderer draws (lbterm/COMPOSITOR.md).

  pwsh -File lbterm\motion\compare.ps1

Opens target\release\lbterm-gui.exe (or -Bin's) twice, looking like Lachlan's LBTerm
(New-TestConfig), with lbterm_compositor on and off (motion off, so layouts land at once), splits
both into the same three panes, captures each client area with PrintWindow(PW_RENDERFULLCONTENT),
and diffs them with pixeldiff.py into target\motion\compare. Without the compositor each pane
draws its card's outline itself; with it the outline is pieces of the card templates that DWM
lays over the pane, so the two must match. Expect only the focused window's cursor (solid in one,
hollow in the other).
Needs Python with Pillow and numpy.
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
# On the right-most screen, without taking focus: the harness's test windows
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $repo 'target\motion\compare'
$T = Start-Test 'motion-compare' -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.compare'

function Open([bool]$compositor) {
  $on = if ($compositor) { 'true' } else { 'false' }
  Open-TestWindow -WorkingDirectory $env:USERPROFILE -TimeoutSec 10 -Set "lbterm_compositor=$on", "lbterm_motion={enabled='off'}"
}

function Layout($w) {
  Use-TestWindow $w
  Start-Sleep 2
  $right = Invoke-Cli split-pane --pane-id 0 --right --percent 40
  Start-Sleep -Milliseconds 700
  Invoke-Cli split-pane --pane-id $right --bottom --percent 30 | Out-Null
  Start-Sleep 2
}

try {
  $comp = Open $true
  $plain = Open $false
  Layout $comp
  Layout $plain
  (Get-Shot $comp -Name 'compositor').Dispose()
  (Get-Shot $plain -Name 'upstream').Dispose()
} finally {
  Close-Test
}
python (Join-Path $PSScriptRoot 'pixeldiff.py') (Join-Path $out 'compositor.png') (Join-Path $out 'upstream.png') (Join-Path $out 'diff.png')
