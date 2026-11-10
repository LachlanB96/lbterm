#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/last_tab.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/overlay/confirm_close_pane.rs, wezterm-gui/src/termwindow/mod.rs, lbterm-extras/src/conductor.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, mux/src/lbterm_keep.rs, mux/src/lib.rs, wezterm-client/src/domain.rs, wezterm-gui/src/frontend.rs
<#
Closing tabs while the Conductor's pad tops the rail (wezterm-gui/src/termwindow/last_tab.rs;
lbterm/TAB-MOUSE.md, "The last tab"), in a test window of its own class on the right-most screen
that never takes focus, with a pad (a state file of the test's own) and a seat pipe nobody
answers, so the Conductor's view can open. Keys and the pointer are posted messages.

1. The tab menu's Close tab closes a tab at once, without a question, even in a mux server
   (where a pane can't say it's idle and every close used to ask).
2. F8 (CloseCurrentTab, no question: Lachlan's Ctrl+W) closes the open tab.
3. Close tab on the last tab leaves the window open: a new tab takes its place, in default_cwd,
   and the Conductor's view opens over it (the server keeps running, with -Mux).
4. Esc leaves the view; F7 (CloseCurrentPane asking first: Lachlan's Ctrl+Shift+W) on the new
   tab's only pane does the same, asking first in a mux server (Y answers).
5. The last tab's program ending by itself does the same: `exit` typed in it, then `lbterm cli
   kill-pane` on it (with -Mux the server, which dropped its window, keeps running with one pane).
6. Closing the window still ends it (and the server).
7. A window without the pad closes with its last tab, as before.

  pwsh -File lbterm\perf\last-tab.ps1          # tabs in the GUI (the local domain)
  pwsh -File lbterm\perf\last-tab.ps1 -Mux     # tabs in a test lbterm-mux-server, as LBTerm runs
#>
param([string]$Bin, [switch]$Mux, [switch]$Keep)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('last-tab' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\tabs.lua'
$plainCfg = Join-Path $T.Out 'plain\plain.lua'
$state = Join-Path $T.Out 'button.json'
New-Item -ItemType Directory -Force (Split-Path $cfg), (Split-Path $plainCfg) | Out-Null
[IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)" }')

$domain = if ($Mux) {
  @"
  unix_domains = { {
    name = 'perflast',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@
} else { '' }
# The tabs open in C:\Windows; default_cwd, where a new tab taking the last one's place opens, is
# System32, so where it opened says which it was
$common = @"
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  lbterm_tab_panel = { enabled = true, open_delay_ms = 0, labels = false },
  enable_tab_bar = false,
  initial_cols = 100,
  initial_rows = 32,
  window_close_confirmation = 'NeverPrompt',
  -- A killed test window would otherwise greet the next with the crash-restore prompt
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows/System32',
  -- Posted keys carry no Ctrl, so F8 and F7 stand in for Lachlan's Ctrl+W and Ctrl+Shift+W
  keys = {
    { key = 'F8', mods = 'NONE', action = wezterm.action.CloseCurrentTab { confirm = false } },
    { key = 'F7', mods = 'NONE', action = wezterm.action.CloseCurrentPane { confirm = true } },
  },
"@
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
return {
$common
  lbterm_conductor = {
    state_file = '$(ConvertTo-LuaPath $state)',
    seat_pipe = [[\\.\pipe\lbterm-perf-noseat-$PID]],
  },
$domain}
"@)
[IO.File]::WriteAllText($plainCfg, @"
local wezterm = require 'wezterm'
return {
$common
}
"@)

Invoke-Test {
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perflast -Socket $sock -WaitIdle -TimeoutSec 15 }
  else { Open-TestWindow -ConfigFile $cfg -Cwd 'C:\Windows' -WaitIdle -TimeoutSec 15 }
  if (-not (Wait-Until { (Get-Panes).Count -ge 1 } -TimeoutSec 10)) { throw 'the test window has no pane' }
  $win1 = (Get-Panes)[0].window_id
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }

  # Three tabs, named, in C:\Windows
  $origin = (Get-Panes -Server:$Mux)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $origin 'alpha' -Server:$Mux | Out-Null
  foreach ($n in 'bravo', 'charlie') {
    $pane = New-Tab -From $origin -Cwd 'C:\Windows' -Server:$Mux
    Invoke-Cli set-tab-title --pane-id $pane $n -Server:$Mux | Out-Null
  }
  if (-not (Wait-Until { @(Get-Tabs $win1).Count -eq 3 })) { throw "the window has $(@(Get-Tabs $win1).Count) tabs, not 3" }
  $e = Wait-Change { param($s) @($s.rail.rows).Count -eq 3 -and $s.rail.conductor } -What 'three rows under the Conductor''s pad'
  $tabId = @{}
  foreach ($tab in Get-Tabs $win1) { $tabId[$tab.tab_title] = $tab.tab_id }

  $row = { param([int]$i) $script:e = Wait-Settled -What 'the rail'; Get-RectCenter $script:e.rail.rows[$i].pad }
  $menu = { param([int]$i)
    $x, $y = & $row $i
    Send-Click $w $x $y -Right -Move
    Wait-Change { param($s) $s.menu.kind -eq 'tab' } -What "the tab menu on row $i"
  }
  $pick = { param($m, [string]$label)
    $it = @($m.menu.items | Where-Object label -eq $label)[0]
    if (-not $it) { throw "no '$label' in the tab menu: $(@($m.menu.items | ForEach-Object label) -join ', ')" }
    $x, $y = Get-RectCenter $it.rect
    Send-Click $w $x $y -Move
  }
  # Every state the window writes from here on, for a question that should never show
  $asked = [System.Collections.Generic.List[string]]::new()
  $watchAsk = { param($s) if ($s.overlay) { $asked.Add($s.overlay) } }

  # 1. Close tab on bravo, from the menu: gone, with no question
  $m = & $menu 1
  & $pick $m 'Close tab'
  Wait-Change { param($s) @($s.tabs).Count -eq 2 -and -not $s.menu } -What 'bravo closed' -OnState $watchAsk | Out-Null
  if (@(Get-Tabs $win1 | Where-Object tab_id -eq $tabId.bravo).Count) { Add-Failure 'bravo is still there after Close tab' }
  if ($asked.Count) { Add-Failure "Close tab asked first ($($asked[0]))" } else { 'menu: Close tab closed bravo without asking' }

  # 2. F8 on the open tab, whichever of alpha and charlie it is
  $open = @((Read-State).windows[0].tabs | Where-Object active)[0]
  Send-Key $w F8
  Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'F8 closing the open tab' | Out-Null
  $last = @(Get-Tabs $win1)
  "F8: closed tab $($open.id), $($last[0].tab_title) is left"

  # 3. Close tab on the last tab: the window stays, with a new tab in default_cwd and the view
  $m = & $menu 0
  & $pick $m 'Close tab'
  $e = Wait-Change { param($s) @($s.tabs).Count -eq 1 -and $s.tabs[0].id -ne $last[0].tab_id -and $s.conductor_view } -What 'a new tab in the last one''s place, under the view' -TimeoutSec 20
  if (-not (Test-WindowAlive $w)) { throw 'the window closed with its last tab, the pad still there' }
  $fresh = @(Get-Tabs $win1)
  "last tab: tab $($fresh[0].tab_id) took tab $($last[0].tab_id)'s place in $($fresh[0].cwd), the view open"
  if ($fresh.Count -ne 1) { Add-Failure "$($fresh.Count) tabs after closing the last, not 1" }
  if ($fresh[0].cwd -notmatch 'System32') { Add-Failure "the new tab is in $($fresh[0].cwd), not default_cwd's System32" }
  if ($Mux -and @(Get-Panes -Server).Count -ne 1) { Add-Failure "the server has $(@(Get-Panes -Server).Count) panes, not 1" }
  (Get-Shot $w -Name 'replaced').Dispose()

  # 4. Esc leaves the view; F7 on the new tab's only pane
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.conductor_view } -What 'Esc leaving the view' | Out-Null
  $before = $fresh[0].tab_id
  Send-Key $w F7
  if ($Mux) {
    # A mux server's pane can't say it's idle, so Ctrl+Shift+W asks: the panel folded, so the
    # question shows where it stays
    $q = Wait-State { param($s) $s.overlay } -What 'the close to ask'
    if ($q.rail.open) { Add-Failure 'the panel is open under the question' }
    Send-Key $w Y
  }
  $e = Wait-Change { param($s) @($s.tabs).Count -eq 1 -and $s.tabs[0].id -ne $before -and $s.conductor_view } -What 'F7: a new tab again, under the view' -TimeoutSec 20
  if (-not (Test-WindowAlive $w)) { throw 'the window closed with its last pane, the pad still there' }
  "F7: tab $($e.tabs[0].id) took tab $before's place"

  # 5. The last tab's program ending by itself: `exit` typed in it, then `lbterm cli kill-pane`
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.conductor_view } -What 'Esc leaving the view' | Out-Null
  $before = $e.tabs[0].id
  # The new tab's pwsh has drawn its prompt once its pane says where it is
  Wait-Until { (Get-Tabs $win1 | Select-Object -First 1).cwd } -TimeoutSec 10 | Out-Null
  Send-Chars $w 'exit'
  Send-Key $w Enter
  $e = Wait-Change { param($s) @($s.tabs).Count -eq 1 -and $s.tabs[0].id -ne $before -and $s.conductor_view } -What 'exit: a new tab in the last one''s place' -TimeoutSec 20
  if (-not (Test-WindowAlive $w)) { throw 'the window closed when its last tab''s program exited, the pad still there' }
  "exit: tab $($e.tabs[0].id) took tab $before's place"
  $before = $e.tabs[0].id
  $pane = (Get-Panes -Server:$Mux)[0].pane_id
  Invoke-Cli kill-pane --pane-id $pane -Server:$Mux | Out-Null
  $e = Wait-Change { param($s) @($s.tabs).Count -eq 1 -and $s.tabs[0].id -ne $before -and $s.conductor_view } -What 'kill-pane: a new tab in the last one''s place' -TimeoutSec 20
  if (-not (Test-WindowAlive $w)) { throw 'the window closed when its last pane was killed, the pad still there' }
  "kill-pane: tab $($e.tabs[0].id) took tab $before's place"
  if ($Mux) {
    if (@(Get-Panes -Server).Count -ne 1) { Add-Failure "the server has $(@(Get-Panes -Server).Count) panes after the exits, not 1" }
    if (@(Get-TestServers).Count -ne 1) { Add-Failure "$(@(Get-TestServers).Count) test mux servers after the exits, not 1" }
  }
  if (@(Get-Tabs $win1).Count -ne 1 -and @(Get-Tabs).Count -ne 1) { Add-Failure "$(@(Get-Tabs).Count) tabs after the exits, not 1" }
  (Get-Shot $w -Name 'after-exit').Dispose()

  # 6. Closing the window still ends it
  Send-Close $w
  if (-not (Wait-Until { -not (Test-WindowAlive $w) } -TimeoutSec 10)) { Add-Failure 'the window is still open after WM_CLOSE' }
  else { 'close: the window ended' }
  if ($Mux -and -not (Wait-Until { -not (Get-TestServers) } -TimeoutSec 10)) { Add-Failure 'the test mux server is still running after the window closed' }

  # 7. No pad: the last tab closes the window, as WezTerm does
  $p = Open-TestWindow -ConfigFile $plainCfg -Cwd 'C:\Windows' -WaitIdle -TimeoutSec 15
  if (-not (Wait-Until { (Get-Panes -Window $p).Count -ge 1 } -TimeoutSec 10)) { throw 'the plain test window has no pane' }
  $pe = Wait-Change { param($s) @($s.rail.rows).Count -eq 1 -and -not $s.rail.conductor } -Window $p -What 'one row, no pad'
  $x, $y = Get-RectCenter $pe.rail.rows[0].pad
  Send-Click $p $x $y -Right -Move
  $pm = Wait-Change { param($s) $s.menu.kind -eq 'tab' } -Window $p -What 'the tab menu without a pad'
  $it = @($pm.menu.items | Where-Object label -eq 'Close tab')[0]
  $x, $y = Get-RectCenter $it.rect
  Send-Click $p $x $y -Move
  if (-not (Wait-Until { -not (Test-WindowAlive $p) } -TimeoutSec 10)) { Add-Failure 'without the pad, the window is still open after its last tab closed' }
  else { 'no pad: the window closed with its last tab' }
} -Keep:$Keep -Passed "last-tab$(if ($Mux) { ' (mux)' }): Close tab doesn't ask, and the last tab gives way to a new one while the pad stays"
