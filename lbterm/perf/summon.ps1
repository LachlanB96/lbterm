#Requires -Version 7
# Covers: wezterm-gui/src/summon.rs, lbterm-extras/src/summon.rs
<#
Checks the summon key without taking focus from anyone: a test window (never focused, as all
test windows are) is given F24 as its summon key, which nobody presses. While it runs, this
script's own RegisterHotKey for F24 must fail, because LBTerm holds the key; after the window
closes, it must succeed, because LBTerm let it go.

What a press does (bring the window forward, un-minimised) needs a real key and the window to
take focus, so it's left to trying PageUp by hand.

  pwsh -File lbterm\perf\summon.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'summon' -Bin $Bin

$F24 = 0x87
$NOREPEAT = 0x4000
function Free { $ok = [LbtermE2E.Win]::RegisterHotKey([IntPtr]::Zero, 77, $NOREPEAT, $F24); if ($ok) { [void][LbtermE2E.Win]::UnregisterHotKey([IntPtr]::Zero, 77) }; $ok }

Invoke-Test {
  if (-not (Free)) { throw 'F24 is already held by something else; this test needs it free' }
  $w = Open-TestWindow -NoConfig -Set "lbterm_summon={key='F24'}" -Program (New-PwshProgram)
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window''s first frame' | Out-Null
  if (Free) { Add-Failure 'F24 was free while the unfocused window ran: LBTerm should hold it' }
  Stop-TestWindow $w
  Start-Sleep 1
  if (-not (Free)) { Add-Failure 'F24 still held after the window closed' }
} -Passed 'summon: the key is held while LBTerm runs unfocused, and let go after'
