#Requires -Version 7
# Covers: window/src/os/windows/window.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-input-types/src/lib.rs, term/src/input.rs, config/src/lua.rs
<#
The mouse's side buttons end to end: a test window whose config binds `button = 'Back'` and
`'Forward'` to the previous and next tab gets WM_XBUTTONDOWN and WM_XBUTTONUP posted, as a mouse
sends them, and the state file must show each hop: back, back, back round to the last tab,
forward round to the first. With Ctrl held nothing is bound, so the tab stays. Over a pane whose
program has the mouse (SGR reporting on) the buttons still hop, since LBTerm never hands them to it.

  pwsh -File lbterm\perf\side-buttons.ps1            # this tree's build
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'side-buttons' -Bin $Bin

# The program in the last tab takes the mouse, as Claude Code does, and waits
$grab = @'
$e = [char]27
[Console]::Write("$e[?1000h$e[?1002h$e[?1006h")
Start-Sleep -Seconds 120
'@

Invoke-Test {
  $config = Join-Path $T.Out 'config\side-buttons.lua'
  New-Item -ItemType Directory -Force (Split-Path $config) | Out-Null
  [IO.File]::WriteAllText($config, @'
local wezterm = require 'wezterm'
local act = wezterm.action
return {
  automatically_reload_config = false,
  lbterm_crash_restore = false,
  wsl_domains = {},
  mouse_bindings = {
    { event = { Down = { streak = 1, button = 'Back' } }, mods = 'NONE', alt_screen = 'Any', action = act.ActivateTabRelative(-1) },
    { event = { Down = { streak = 1, button = 'Forward' } }, mods = 'NONE', alt_screen = 'Any', action = act.ActivateTabRelative(1) },
  },
}
'@)
  $w = Open-TestWindow -ConfigFile $config -Program (New-PwshProgram)
  Wait-State { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  New-Tab -From $first -Program (New-PwshProgram) | Out-Null
  New-Tab -From $first -Program (New-PwshProgram $grab) | Out-Null
  $s = Wait-Change { param($x) @($x.tabs).Count -eq 3 -and $x.active_tab -eq 2 -and @($x.panes | Where-Object mouse_reporting).Count -eq 1 } -What 'three tabs, the last taking the mouse'
  $x, $y = Get-RectCenter @($s.panes)[0].rect

  $steps = @(
    @{ Name = 'back over the pane that has the mouse'; Forward = $false; Want = 1 },
    @{ Name = 'back'; Forward = $false; Want = 0 },
    @{ Name = 'back round to the last tab'; Forward = $false; Want = 2 },
    @{ Name = 'forward round to the first tab'; Forward = $true; Want = 0 },
    @{ Name = 'forward'; Forward = $true; Want = 1 }
  )
  foreach ($step in $steps) {
    $want = $step.Want
    Send-SideButton $w $x $y -Forward:$step.Forward
    $landed = { param($sw) $sw.active_tab -eq $want }.GetNewClosure()
    try { Wait-Change $landed -What $step.Name -TimeoutSec 5 | Out-Null; "$($step.Name): tab $want" }
    catch { Add-Failure "$($step.Name): wanted tab $want, $($_.Exception.Message)" }
  }

  # Ctrl held: nothing's bound, so the open tab stays. Nothing paints for a press that does nothing,
  # so this one waits a fixed time: a hop above took one frame
  Send-SideButton $w $x $y -Keys Ctrl
  Start-Sleep -Milliseconds 800
  $after = Get-StateWindow (Read-State)
  if ($after.active_tab -ne 1) { Add-Failure "Ctrl and the back button moved to tab $($after.active_tab)" } else { 'Ctrl and back: stayed on tab 1' }
} -Passed 'side buttons: every hop landed, over a pane with the mouse too, and Ctrl and a button did nothing'
