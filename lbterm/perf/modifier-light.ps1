#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/held.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/composited.rs, lbterm-panel/src/palette.rs, lbterm-motion/src/motion.rs
<#
The active outline's colour while modifiers are held (wezterm-gui/src/termwindow/held.rs;
lbterm/FEEDBACK.md), in a test window of its own class on the right-most screen that never takes
focus. Posted key messages leave Windows' key state alone, so a pointer message carrying
MK_CONTROL and MK_SHIFT, which LBTerm reads the modifiers from too, stands in for the keys.

1. At rest the outline is white: none of the held colours along the pane's card.
2. Ctrl held: the state file says ctrl, and the outline is blue (ctrl.png).
3. Ctrl and Shift: cyan, the two colours added as light (ctrl-shift.png).
4. Shift alone: green.
5. Let go: none held, and once the light has settled no colour is left (released.png).
6. Shift held as the keyboard goes (a posted WM_KILLFOCUS): nothing is held any more.

  pwsh -File lbterm\perf\modifier-light.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'modifier-light' -Bin $Bin
$cfg = Join-Path $T.Out 'config\held.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
[IO.File]::WriteAllText($cfg, @"
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  initial_cols = 100, initial_rows = 26,
  window_close_confirmation = 'NeverPrompt',
  enable_tab_bar = false,
  cursor_blink_rate = 0,
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
}
"@)

# The held colours (lbterm_panel::palette::held) where the outline's line is, give or take its
# soft edges: Ctrl #3D7BFF, Ctrl+Shift #6BFFFF, Shift #2EE06A
$blue = New-ColourRange 20 110 80 170 200 255
$cyan = New-ColourRange 60 160 200 255 200 255
$green = New-ColourRange 0 100 170 255 60 160

Invoke-Test {
  # The pointer stays where it's posted, so no WM_MOUSELEAVE comes between the moves
  $w = Open-TestWindow -ConfigFile $cfg -PostedPointer -WaitIdle -TimeoutSec 15
  $e = Wait-Change { param($s) $s.frame -gt 0 -and @($s.panes).Count -eq 1 } -What 'the window'
  $r = $e.panes[0].rect
  # The card's bottom edge, clear of its rounded corners: the outline and nothing else changes there
  $edge = New-Rect ([int]$r.x + 30) ([int]($r.y + $r.h) - 5) ([int]$r.w - 60) 10
  $mid = @([int]($r.x + $r.w / 2), [int]($r.y + $r.h / 2))
  $count = { param($shot, $range) [LbtermE2E.Px]::CountRange($shot, $edge, $range) }
  # Each colour along the edge once the window has settled on `held`
  $look = { param([string[]]$keys, [string[]]$held, [string]$name)
    Send-Mouse $w Move $mid[0] $mid[1] -Keys $keys
    $want = ($held -join ',')
    Wait-Change { param($s) (@($s.held) -join ',') -eq $want } -What "held: $want" | Out-Null
    $shot = Get-Shot $w -Name $name
    $n = @{ blue = (& $count $shot $blue); cyan = (& $count $shot $cyan); green = (& $count $shot $green) }
    $shot.Dispose()
    "${name}: held [$want], along the edge blue $($n.blue), cyan $($n.cyan), green $($n.green)"
    $n
  }
  $long = [int]$r.w / 4

  # 1. At rest
  $n = & $look @() @() 'rest'
  $n = $n[-1]
  if ($n.blue + $n.cyan + $n.green -gt 5) { Add-Failure "the outline at rest shows a held colour: $($n | ConvertTo-Json -Compress)" }

  # 2. Ctrl
  $n = (& $look @('Ctrl') @('ctrl') 'ctrl')[-1]
  if ($n.blue -lt $long) { Add-Failure "Ctrl held: only $($n.blue) blue pixels along the outline (want $long or more)" }

  # 3. Ctrl and Shift: cyan
  $n = (& $look @('Ctrl', 'Shift') @('ctrl', 'shift') 'ctrl-shift')[-1]
  if ($n.cyan -lt $long) { Add-Failure "Ctrl+Shift held: only $($n.cyan) cyan pixels along the outline" }
  if ($n.blue -gt 5) { Add-Failure "Ctrl+Shift held: $($n.blue) blue pixels left from Ctrl alone" }

  # 4. Shift alone: green
  $n = (& $look @('Shift') @('shift') 'shift')[-1]
  if ($n.green -lt $long) { Add-Failure "Shift held: only $($n.green) green pixels along the outline" }

  # 5. Let go
  $n = (& $look @() @() 'released')[-1]
  if ($n.blue + $n.cyan + $n.green -gt 5) { Add-Failure "let go, the outline still shows a held colour: $($n | ConvertTo-Json -Compress)" }

  # 6. Shift held as the keyboard goes
  Send-Mouse $w Move $mid[0] $mid[1] -Keys Shift
  Wait-Change { param($s) (@($s.held) -join ',') -eq 'shift' } -What 'held: shift' | Out-Null
  Send-Focus $w -Off
  $e = Wait-Change { param($s) -not $s.keyboard } -What 'the keyboard gone'
  if (@($e.held).Count) { Add-Failure "without the keyboard the window still holds $(@($e.held) -join ', ')" }
  else { 'keyboard gone: nothing held' }
} -Passed 'modifier-light: the active outline is blue with Ctrl, cyan with Ctrl+Shift, green with Shift, white again when let go, and nothing is held without the keyboard'
