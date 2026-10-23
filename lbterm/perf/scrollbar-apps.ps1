#Requires -Version 7
# Covers: lbterm-extras/src/scrollbar.rs, wezterm-gui/src/scrollbar.rs, wezterm-gui/src/termwindow/render/card.rs, wezterm-client/src/pane/clientpane.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
The scrollbar hides while a program has the whole screen (lbterm_hide_scroll_bar_in_apps,
lbterm-extras/src/scrollbar.rs): a fullscreen program such as Claude Code scrolls its own view,
so the window's viewport never moves and the thumb would follow nothing.

Test windows (no config of Lachlan's: the scrollbar on, its thumb pure green so a capture can
find it) run a pwsh pane that prints 300 lines, then does what this script asks through a step
file: the alternate screen on and off, mouse reporting on and off. After each step a capture
(PrintWindow, which sees DirectComposition content) counts the green down the window's right:

1. A window of its own: the thumb shows at a shell, goes on the alternate screen, comes back,
   goes with mouse reporting on, comes back. A second tab at a shell shows it while the first is
   on the alternate screen, and switching back to the first hides it again.
2. The same steps on a mux domain of its own (target\perf\scrollbar-apps\sock), where the pane
   lives in a test lbterm-mux-server and the GUI learns its modes from the server.
3. lbterm_hide_scroll_bar_in_apps = false: the thumb stays through both.

What the GUI's Lua saw (pane:is_alt_screen_active()) is printed with each step. Fails with exit 1;
the captures are in target\perf\scrollbar-apps.

  pwsh -File lbterm\perf\scrollbar-apps.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'scrollbar-apps' -Bin $Bin
$sock = Join-Path $T.Out 'sock'
$cfg = Join-Path $T.Out 'config\apps.lua'
$report = Join-Path $T.Out 'report.json'
$step = Join-Path $T.Out 'step.txt'
$ack = Join-Path $T.Out 'ack.txt'
$driver = Join-Path $T.Out 'driver.ps1'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The pane: 300 lines of scrollback, then each step the script writes, answered in ack.txt
[IO.File]::WriteAllText($driver, @"
`$e = [char]27
# The answer, written again until the script isn't reading it
function Ack([string]`$v) {
  for (`$i = 0; `$i -lt 100; `$i++) { try { [IO.File]::WriteAllText('$ack', `$v); return } catch { Start-Sleep -Milliseconds 20 } }
}
1..300 | ForEach-Object { "line `$_" }
# The title says the lines are out: the frame that shows it shows all of them
[Console]::Write("`$e]2;ready" + [char]7)
Ack 'ready'
`$last = 'ready'
while (`$true) {
  `$s = (Get-Content '$step' -Raw -ErrorAction SilentlyContinue)
  if (`$s) { `$s = `$s.Trim() }
  if (`$s -and `$s -ne `$last) {
    switch (`$s) {
      'alt-on' { [Console]::Write("`$e[?1049h`$e[2J`$e[Ha fullscreen program") }
      'alt-off' { [Console]::Write("`$e[?1049l") }
      'mouse-on' { [Console]::Write("`$e[?1000h`$e[?1006h") }
      'mouse-off' { [Console]::Write("`$e[?1000l`$e[?1006l") }
    }
    `$last = `$s
    Ack `$s
  }
  Start-Sleep -Milliseconds 50
}
"@)

# The server reads the same file (serve_command), so both know the test domain
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local report = '$(ConvertTo-LuaPath $report)'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
wezterm.on('update-status', function(window, pane)
  local f = io.open(report, 'w')
  if f then f:write(encode({ pane_id = pane:pane_id(), alt = pane:is_alt_screen_active() })); f:close() end
end)
return {
  front_end = 'WebGpu',
  enable_scroll_bar = true,
  colors = { scrollbar_thumb = '#00ff00' },
  initial_cols = 90,
  initial_rows = 24,
  status_update_interval = 250,
  lbterm_crash_restore = false,
  automatically_reload_config = false,
  window_close_confirmation = 'NeverPrompt',
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-File', '$(ConvertTo-LuaPath $driver)' },
  default_cwd = 'C:/Windows',
  unix_domains = { {
    name = 'perfapps',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
}
"@)

# The thumb's green: G over 180, R and B under 90
$green = New-ColourRange 0 89 181 255 0 89

# The window's client area, saved as $name.png; the green pixels in its right-hand 80 px, every
# other row
function Measure-Thumb($w, [string]$name) {
  $shot = Get-Shot $w -Name $name
  $x0 = [Math]::Max(0, $shot.Width - 80)
  $n = [LbtermE2E.Px]::CountRangeSparse($shot, (New-Rect $x0 0 ($shot.Width - $x0) $shot.Height), $green, 1, 2)
  $shot.Dispose()
  $n
}

# The pane with the keyboard, in a window's state
function Get-ActivePane($x) { @($x.panes | Where-Object active)[0] }

# What each step turns on or off in the pane, as the state file names it
$modes = @{
  'alt-on' = 'alt_screen', $true; 'alt-off' = 'alt_screen', $false
  'mouse-on' = 'mouse_reporting', $true; 'mouse-off' = 'mouse_reporting', $false
}

# Ask the pane for a step and wait for its answer, then for the window to show it: a frame
# painted with the pane in that mode, and nothing moving after it
function Send-Step($w, [string]$s) {
  # Written again until the pane isn't reading it
  if (-not (Wait-Until { try { [IO.File]::WriteAllText($step, $s); $true } catch { $false } } -TimeoutSec 5 -IntervalMs 20)) { throw "couldn't write the step '$s'" }
  $done = { (Get-Content $ack -Raw -ErrorAction SilentlyContinue) -match "^$s\s*$" }
  if (-not (Wait-Until $done -TimeoutSec 10 -IntervalMs 50)) { throw "the pane never did '$s'" }
  $field, $on = $modes[$s]
  Wait-Change { param($x) $p = Get-ActivePane $x; $p -and [bool]$p.$field -eq $on } -Window $w -Animating -What "the pane's $field $on" | Out-Null
}

# The pane's 300 lines printed, and painted: the title it sets after them shows (was 1 s)
function Wait-Ready($w) {
  if (-not (Wait-Until { Test-Path $ack } -TimeoutSec 20 -IntervalMs 100)) { throw 'the pane never printed its lines' }
  Wait-Change { param($x) (Get-ActivePane $x).title -eq 'ready' } -Window $w -Animating -What 'the 300 lines painted' | Out-Null
}

function Get-Alt {
  try { (Get-Content $report -Raw -ErrorAction Stop | ConvertFrom-Json).alt } catch { '?' }
}

# One step and what it should leave: the thumb shown or not
function Test-Step($w, [string]$run, [string]$s, [bool]$want) {
  if ($s -ne 'start') { Send-Step $w $s }
  $n = Measure-Thumb $w "$run-$s"
  $shown = $n -gt 20
  "${run}: $s, Lua's alternate screen $(Get-Alt): $n green px, thumb $(if ($shown) { 'shown' } else { 'hidden' })"
  if ($shown -ne $want) { Add-Failure "${run}: after $s the thumb is $(if ($shown) { 'shown' } else { 'hidden' }), not $(if ($want) { 'shown' } else { 'hidden' })" }
}

function Reset-Steps {
  Remove-Item $step, $ack, $report -ErrorAction SilentlyContinue
}

Invoke-Test {
  # 1. A window of its own
  Reset-Steps
  $w = Open-TestWindow -ConfigFile $cfg -TimeoutSec 15
  Wait-Ready $w
  Test-Step $w 'local' 'start' $true
  Test-Step $w 'local' 'alt-on' $false
  Test-Step $w 'local' 'alt-off' $true
  Test-Step $w 'local' 'mouse-on' $false
  Test-Step $w 'local' 'mouse-off' $true
  Test-Step $w 'local' 'alt-on' $false
  # A second tab at a shell, which the window switches to, then back to the first
  $first = (Get-Panes)[0].pane_id
  $second = New-Tab -From $first -Program (New-PwshProgram '1..300 | ForEach-Object { "other $_" }; [Console]::Write([char]27 + "]2;other" + [char]7); Start-Sleep 600')
  Invoke-Cli activate-pane --pane-id $second | Out-Null
  # The second tab open, its lines painted: the title it sets after them shows (was 3 s)
  Wait-Change { param($x) $p = Get-ActivePane $x; $p.id -eq $second -and $p.title -eq 'other' } -Window $w -Animating -What 'the second tab open, its lines painted' | Out-Null
  $n = Measure-Thumb $w 'local-second-tab'
  "local: a second tab at a shell, the first still on the alternate screen: $n green px"
  if ($n -le 20) { Add-Failure 'local: the thumb is hidden on a second tab at a shell' }
  Invoke-Cli activate-pane --pane-id $first | Out-Null
  # The first tab open again (was 1 s)
  Wait-Change { param($x) $x.active_pane -eq $first } -Window $w -Animating -What 'the first tab open again' | Out-Null
  $n = Measure-Thumb $w 'local-back'
  "local: back on the first tab: $n green px"
  if ($n -gt 20) { Add-Failure 'local: the thumb shows on returning to the tab on the alternate screen' }
  Stop-TestWindow $w

  # 2. On a mux domain, its modes coming from the server
  Reset-Steps
  $w = Open-TestWindow -ConfigFile $cfg -Domain perfapps -Socket $sock -TimeoutSec 15
  Wait-Ready $w
  if ((Get-TestServers).Count -ne 1) { Add-Failure "mux: $((Get-TestServers).Count) test mux servers running, not 1" }
  Test-Step $w 'mux' 'start' $true
  Test-Step $w 'mux' 'alt-on' $false
  Test-Step $w 'mux' 'alt-off' $true
  Test-Step $w 'mux' 'mouse-on' $false
  Test-Step $w 'mux' 'mouse-off' $true
  Stop-TestWindow $w
  Stop-TestServers

  # 3. Turned off
  Reset-Steps
  $w = Open-TestWindow -ConfigFile $cfg -Set 'lbterm_hide_scroll_bar_in_apps=false' -TimeoutSec 15
  Wait-Ready $w
  Test-Step $w 'off' 'start' $true
  Test-Step $w 'off' 'alt-on' $true
  Test-Step $w 'off' 'mouse-on' $true
} -Passed 'scrollbar-apps: the scrollbar hides while a program has the screen, locally and on a mux domain, and stays when turned off'
