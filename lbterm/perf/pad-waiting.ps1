#Requires -Version 7
# Variant: -NoCompositor
# Covers: lbterm-panel/src/**, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/compositor.rs, term/src/program_status.rs, wezterm-escape-parser/src/program_status.rs
<#
A pad whose program has handed the turn back but still has tasks of its own running gets a turning
green arc, and its row says "Waiting on N tasks" (lbterm/TAB-PANEL.md). Terminal's statusline reports
Claude Code's background tasks as OSC 7501 records under bg/<id>; here three stand-in tabs print the
records themselves: Claude done with two tasks still working, Claude done with none, Claude working.

The closed rail is captured twice, 350 ms apart (a quarter turn): with the compositor the arc turns
in DWM, so the only pixels that differ must sit in one small square on the first pad. Then the panel
is opened and captured (pad-waiting-open.png) for the words. -NoCompositor draws the arc on the pad,
still: nothing may change. Fails with exit 1.

  pwsh -File lbterm\perf\pad-waiting.ps1 [-NoCompositor]
#>
param([string]$Bin, [switch]$NoCompositor)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('pad-waiting' + $(if ($NoCompositor) { '-nocompositor' } else { '' })) -Bin $Bin
$home2 = Join-Path $T.Out 'home'

# What each stand-in prints: OSC 7501 records as Claude Code (and the statusline, for bg/) send them
$records = @{
  waiting = @('state=done:app=claude', 'state=working:id=bg/b1', 'state=working:id=bg/b2')
  done = @('state=done:app=claude')
  working = @('state=working:app=claude')
}
function Stand-In([string]$name) {
  $seq = ($records[$name] | ForEach-Object { "`$e]7501;$_`$e\" }) -join ''
  New-PwshProgram "`$e = [char]27; [Console]::Write(`"$seq`"); Write-Host '$name'; Start-Sleep 600"
}

Invoke-Test {
  # LBTERM_POSTED_POINTER: the posted moves that open the panel at the end hover (without it the
  # window asks for TME_LEAVE and Windows ends each hover at once, so the panel never opened)
  $w = Open-TestWindow -UserProfile $home2 -WorkingDirectory $T.Repo -PostedPointer -Program (Stand-In waiting) `
    -Set "lbterm_compositor=$(if ($NoCompositor) { 'false' } else { 'true' })", 'lbterm_tab_panel={enabled=true,open_delay_ms=0}'
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  foreach ($name in 'done', 'working') {
    Invoke-Cli spawn --window-id 0 -Program (Stand-In $name) | Out-Null
  }
  # Each tab on screen once, so a result counts as seen, then the waiting one back
  foreach ($i in 1, 2, 0) {
    Invoke-Cli activate-tab --pane-id 0 --tab-index $i | Out-Null
    Wait-Change { param($s) $s.active_tab -eq $i } -What "tab $i open" -Animating | Out-Null
  }
  Wait-Change { param($s) @($s.rail.rows)[0].waiting -eq 2 } -What 'the first pad waiting on two tasks' -Animating | Out-Null

  $railW = ConvertTo-Px 80 $w.Scale
  $a = Get-Shot $w
  Start-Sleep -Milliseconds 350
  $b = Get-Shot $w
  Save-Shot $a 'pad-waiting'; Save-Shot $b 'pad-waiting-later'
  # What changed on the rail between the two
  $rail = Get-Columns 0 ($railW - 1) -Of $a
  $n = [LbtermE2E.Px]::Differing($a, $b, $rail)
  $box = [LbtermE2E.Px]::DiffBox($a, $b, $rail)
  $a.Dispose(); $b.Dispose()
  $square = [Math]::Round(16 * $w.Scale)
  if ($NoCompositor) {
    "without the compositor the rail changed in $n pixels"
    if ($n -ne 0) { Add-Failure "the arc moved without the compositor ($n pixels)" }
  } elseif ($n -eq 0) {
    Add-Failure 'nothing on the rail turned in 350 ms'
  } else {
    "the rail changed in $n pixels, within $(Format-Box $box)"
    if ($box[2] - $box[0] + 1 -gt $square -or $box[3] - $box[1] + 1 -gt $square) { Add-Failure "what changed spans more than one arc's square ($square px)" }
  }

  # Open: the words
  Set-PointerHold $w (ConvertTo-Px 40 $w.Scale) (ConvertTo-Px 160 $w.Scale)
  Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating | Out-Null
  (Get-Shot $w -Name 'pad-waiting-open').Dispose()
  Clear-PointerHold
} -Passed 'pad-waiting: the pad waiting on its tasks turns its arc, and nothing else moves'
