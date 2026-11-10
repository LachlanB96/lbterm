#Requires -Version 7
# Variant: -Mux
# Covers: lbterm-extras/src/closing.rs, wezterm-gui/src/termwindow/last_tab.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/overlay/confirm_close_pane.rs, wezterm-gui/src/termwindow/mod.rs
<#
Close marks (lbterm/TAB-MOUSE.md, "Close marks"), in a test window of its own class on the
right-most screen that never takes focus, with lbterm_close_marks a folder of the test's own. The
window's first tab holds no session; five more hold one each, set with claude_session as
claudehook.exe sets it. Keys and the pointer are posted messages.

1. F8 (CloseCurrentTab, no question: Lachlan's Ctrl+W) on the open tab: its mark, and no other.
2. The tab menu's Close tab: its mark.
3. F7 (CloseCurrentPane asking first: Ctrl+Shift+W). In a mux server it asks: No leaves no mark,
   Yes writes it. Locally pwsh closes without asking, and the mark is there.
4. The menu's Close other tabs on the last tab: a mark for the one with a session it closed, none
   for the tab it kept.
5. Closing the window (WM_CLOSE) writes none for that tab.

  pwsh -File lbterm\perf\close-marks.ps1          # tabs in the GUI (the local domain)
  pwsh -File lbterm\perf\close-marks.ps1 -Mux     # tabs in a test lbterm-mux-server, as LBTerm runs
#>
param([string]$Bin, [switch]$Mux, [switch]$Keep)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('close-marks' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = $T.Sock
$marks = Join-Path $T.Out 'marks'
$cfg = Join-Path $T.Out 'config\marks.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

$domain = if ($Mux) {
  @"
  unix_domains = { {
    name = 'perfmarks',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@
} else { '' }
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  lbterm_tab_panel = { enabled = true, open_delay_ms = 0, labels = false },
  enable_tab_bar = false,
  initial_cols = 100,
  initial_rows = 32,
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  wsl_domains = {},
  lbterm_close_marks = '$(ConvertTo-LuaPath $marks)',
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  -- Posted keys carry no Ctrl, so F8 and F7 stand in for Lachlan's Ctrl+W and Ctrl+Shift+W
  keys = {
    { key = 'F8', mods = 'NONE', action = wezterm.action.CloseCurrentTab { confirm = false } },
    { key = 'F7', mods = 'NONE', action = wezterm.action.CloseCurrentPane { confirm = true } },
  },
$domain}
"@)

# A program that says it's Claude session `sid`, as claudehook.exe does, and waits
function Claude-Program([string]$sid) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sid))
  New-PwshProgram "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7); Start-Sleep 600"
}
$sids = 1..5 | ForEach-Object { "c1c1c1c1-0000-4000-8000-00000000000$_" }
function Marked { @(Get-ChildItem $marks -Filter '*.closing' -ErrorAction SilentlyContinue | ForEach-Object { $_.BaseName } | Sort-Object) }
function Expect-Marks([string[]]$want, [string]$after) {
  $got = Marked
  if (($got -join ',') -ne ((@($want) | Sort-Object) -join ',')) { Add-Failure "after $after the marks are [$($got -join ', ')], not [$(@($want) -join ', ')]" }
  else { "$after`: marks [$((@($want) | ForEach-Object { $_.Substring(34) }) -join ', ')]" }
}
# The state file has only the open tab's panes, so each session's tab is known by its id: the
# tabs open in order, the first holding no session
$tabOf = @{}
function Tab-Of($s, [string]$sid) { @($s.tabs | Where-Object id -eq $tabOf[$sid])[0] }

Invoke-Test {
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfmarks -Socket $sock -WaitIdle -TimeoutSec 15 }
  else { Open-TestWindow -ConfigFile $cfg -WaitIdle -TimeoutSec 15 }
  if (-not (Wait-Until { (Get-Panes).Count -ge 1 } -TimeoutSec 10)) { throw 'the test window has no pane' }
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }
  $origin = (Get-Panes -Server:$Mux)[0].pane_id
  $pane = @{}
  foreach ($sid in $sids) { $pane[$sid] = New-Tab -From $origin -NoActivate -Program (Claude-Program $sid) -Server:$Mux }
  $e = Wait-Change { param($s) @($s.tabs).Count -eq 6 -and @($s.rail.rows).Count -eq 6 } -What 'six tabs'
  $ordered = @($e.tabs | Sort-Object index)
  for ($k = 0; $k -lt 5; $k++) { $tabOf[$sids[$k]] = $ordered[$k + 1].id }
  if (Marked) { throw "marks before anything closed: $(Marked -join ', ')" }

  # Opens a session's tab and waits for its pane to say the session: the user var has reached the
  # GUI, which is what a close reads
  $open = { param([string]$sid)
    Invoke-Cli activate-pane --pane-id $pane[$sid] -Server:$Mux | Out-Null
    Wait-Change { param($s) (Tab-Of $s $sid).active -and @($s.panes | Where-Object session -eq $sid).Count } -What "$sid's tab open, saying its session" | Out-Null
  }
  $menu = { param([string]$sid)
    $s = Wait-Settled -What 'the rail'
    $tab = Tab-Of $s $sid
    $row = @($s.rail.rows | Where-Object tab -eq $tab.id)[0]
    $x, $y = Get-RectCenter $row.pad
    Send-Click $w $x $y -Right -Move
    Wait-Change { param($s) $s.menu.kind -eq 'tab' } -What "the tab menu on $sid's row"
  }
  $pick = { param($m, [string]$label)
    $it = @($m.menu.items | Where-Object label -eq $label)[0]
    if (-not $it) { throw "no '$label' in the tab menu: $(@($m.menu.items | ForEach-Object label) -join ', ')" }
    $x, $y = Get-RectCenter $it.rect
    Send-Click $w $x $y -Move
  }
  $gone = { param([string]$sid, [string]$what)
    Wait-Change { param($s) -not (Tab-Of $s $sid) } -What $what | Out-Null
  }

  # 1. F8 on the open tab
  & $open $sids[0]
  Send-Key $w F8
  & $gone $sids[0] 'F8 closing the first session''s tab'
  Expect-Marks $sids[0] 'F8 (Ctrl+W)'

  # 2. The menu's Close tab
  & $open $sids[1]
  $m = & $menu $sids[1]
  & $pick $m 'Close tab'
  & $gone $sids[1] 'Close tab'
  Expect-Marks $sids[0..1] 'the menu''s Close tab'

  # 3. F7: in a mux server it asks, and No leaves no mark
  & $open $sids[2]
  if ($Mux) {
    Send-Key $w F7
    Wait-State { param($s) $s.overlay } -What 'Ctrl+Shift+W to ask' | Out-Null
    Send-Key $w N
    Wait-Change { param($s) -not $s.overlay } -What 'the question answered No' | Out-Null
    if (-not (Tab-Of (Read-State).windows[0] $sids[2])) { Add-Failure 'No closed the pane' }
    Expect-Marks $sids[0..1] 'F7 answered No'
    Send-Key $w F7
    Wait-State { param($s) $s.overlay } -What 'Ctrl+Shift+W to ask again' | Out-Null
    Send-Key $w Y
  } else {
    Send-Key $w F7
  }
  & $gone $sids[2] 'F7 closing the pane'
  Expect-Marks $sids[0..2] "F7 (Ctrl+Shift+W)$(if ($Mux) { ' answered Yes' })"

  # 4. Close other tabs on the last session's tab: the first tab (no session) and the fourth go
  & $open $sids[3]
  & $open $sids[4]
  $m = & $menu $sids[4]
  & $pick $m 'Close other tabs'
  Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'Close other tabs leaving one' | Out-Null
  Expect-Marks $sids[0..3] 'Close other tabs'

  # 5. Closing the window marks nothing
  Send-Close $w
  if (-not (Wait-Until { -not (Test-WindowAlive $w) } -TimeoutSec 10)) { Add-Failure 'the window is still open after WM_CLOSE' }
  Start-Sleep -Milliseconds 500
  Expect-Marks $sids[0..3] 'closing the window'
} -Keep:$Keep -Passed "close-marks$(if ($Mux) { ' (mux)' }): a tab or pane closed by hand marks its Claude session, a window closing doesn't"
