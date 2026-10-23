#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/gpu_lost.rs, lbterm-extras/src/gpu.rs, wezterm-gui/src/termwindow/webgpu.rs
<#
Losing the GPU device and carrying on (lbterm/GPU-LOST.md): a test window with the compositor and
the WebGpu front end (on the right-most screen, without focus) gets a pane that counts, and
LBTERM_TEST_REMOVE_DEVICE=3 makes LBTerm remove its own D3D12 device three seconds in, as a
driver reset would. The window must still be open after, its log must say the GPU state was
built again, and the count must have gone on: gpu-before.png and gpu-after.png.

  pwsh -File lbterm\perf\gpu-lost.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'gpu-lost' -Bin $Bin

$counter = @'
for ($i = 1; $i -le 200; $i++) { "tick $i"; Start-Sleep -Milliseconds 100 }
Start-Sleep 30
'@

# A capture saved as $name.png: a fingerprint of the picture, and how many of its pixels are lit
# (text, the frame), every 4 px
function Measure-Shot($w, [string]$name) {
  $shot = Get-Shot $w -Name $name
  $f = [LbtermE2E.Px]::Fingerprint($shot, 4)
  $shot.Dispose()
  [pscustomobject]@{ Lit = $f[0]; Hash = $f[1] }
}

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'", 'lbterm_compositor=true', 'initial_rows=20' -Program (New-PwshProgram $counter) `
    -Log 'lbterm_gui::termwindow=info,warn' -Environment @{ LBTERM_TEST_REMOVE_DEVICE = '3' }
  $h = $w.Handle
  # Before the device goes, 3 s after the window started: a fixed time, since that's how the
  # GUI times it
  Start-Sleep 2
  $before = Measure-Shot $w 'gpu-before'
  # How far the pane has counted
  $count = {
    $text = (Invoke-Cli get-text --pane-id (Get-Panes)[0].pane_id) -join "`n"
    [int](([regex]::Matches($text, 'tick (\d+)') | ForEach-Object { [int]$_.Groups[1].Value } | Measure-Object -Maximum).Maximum)
  }
  # The device goes at 3 s. Until the log says the GPU state was built again, the window has
  # painted frames since and the count has passed 60 (was a fixed 6 s), a look every 500 ms at
  # who has the foreground
  $focusTaken = 0; $checks = 0; $rebuilt = $null
  while ($checks -lt 24 -and (Test-WindowAlive $w)) {
    Start-Sleep -Milliseconds 500
    $checks++
    if ([LbtermE2E.Win]::ForegroundPid() -eq [uint32]$w.Pid) { $focusTaken++ }
    if ($null -eq $rebuilt -and (Get-LogLines $w 'GPU state built again')) { $rebuilt = Get-StateFrame $w }
    if ($null -ne $rebuilt -and (Get-StateFrame $w) -gt $rebuilt + 1 -and (& $count) -ge 60) { break }
  }
  if ($focusTaken) { Add-Failure "the test window had the foreground at $focusTaken of $checks checks" }
  if (-not (Test-WindowAlive $w)) { Add-Failure 'the GUI process died' }
  elseif (-not [LbtermE2E.Win]::IsWindow($h)) { Add-Failure 'the window was closed' }
  else {
    # Painting again: two pictures two frames apart differ as the count goes on, with text in both
    $frame = Get-StateFrame $w
    $after = Measure-Shot $w 'gpu-after'
    try { Wait-State { $true } -AfterFrame ($frame + 1) -What 'two frames more' -TimeoutSec 5 | Out-Null } catch { }
    $later = Measure-Shot $w 'gpu-later'
    "lit pixels: $($before.Lit) before, $($after.Lit) after, $($later.Lit) two frames later"
    if ($after.Lit -lt $before.Lit / 3) { Add-Failure "the window is nearly blank after the rebuild ($($after.Lit) lit pixels, $($before.Lit) before)" }
    if ($after.Hash -eq $later.Hash) { Add-Failure 'the window stopped repainting: two pictures two frames apart are the same' }
    $reached = & $count
    "the count reached $reached"
    if ($reached -lt 60) { Add-Failure "the count only reached $reached" }
  }
  Stop-TestWindow $w
  $lines = Get-LogLines $w 'GPU|device'
  $lines | Select-Object -Last 8
  if (-not ($lines -match 'GPU device lost')) { Add-Failure 'the log never says the device was lost' }
  if (-not ($lines -match 'GPU state built again')) { Add-Failure 'the log never says the GPU state was built again' }
} -Passed 'gpu-lost: the device went, the window stayed, and the panes went on'
