#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/tab_mouse.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/tabs.rs, lbterm-panel/src/**, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-client/src/domain.rs, mux/src/tab.rs, mux/src/window.rs
<#
Tabs with the mouse end to end (lbterm/TAB-MOUSE.md), in a test window of its own class on the
right-most screen that never takes focus. The pointer is posted mouse messages, so the real one
never moves, and the keys are posted key messages. Where to click (the rows' pads, the menu's
items) and when the window has answered come from its state file (lbterm/STATE.md).

1. The tab menu opens on a right-click on a tab's row in the tab panel (menu.png), and Esc
   closes it.
2. Its teal swatch rings that tab's pad (colour.png: teal pixels round the pad).
3. Rename tab, typed in place, renames it (in the server too, with -Mux).
4. Duplicate tab opens a tab in the same folder; the keyboard (Up, Enter: Close tab) closes it.
5. Dragging the first tab down the panel moves it to the slot let go on.
6. Dragging a tab out below the window and letting go gives it a window of its own, its
   top-left where it was let go (torn-off.png).
7. Dragging another tab onto that window moves it there.
8. Close other tabs leaves the first window one tab.

  pwsh -File lbterm\perf\tab-mouse.ps1          # tabs in the GUI (the local domain)
  pwsh -File lbterm\perf\tab-mouse.ps1 -Mux     # tabs in a test lbterm-mux-server, as LBTerm runs
#>
param([string]$Bin, [switch]$Mux, [switch]$Keep)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('tab-mouse' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = Join-Path $T.Out 'sock'
$cfg = Join-Path $T.Out 'config\tabs.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

$domain = if ($Mux) {
  @"
  unix_domains = { {
    name = 'perftabs',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@
} else { '' }
# No tab bar, so the panel starts at the window's top. Labels off: they make the rows taller, and
# perf\rail-labels.ps1 covers them
[IO.File]::WriteAllText($cfg, @"
return {
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
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
$domain}
"@)

# The teal swatch's colour, #39A59C, give or take 29 a channel
$teal = New-ColourRange 28 86 136 194 127 185

Invoke-Test {
  # The window must never take focus: it starts once Lachlan's hands have been off the keyboard
  # and mouse for a few seconds, and the foreground window is checked all the way through
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perftabs -Socket $sock -WaitIdle -TimeoutSec 15 }
  else { Open-TestWindow -ConfigFile $cfg -Cwd 'C:\Windows' -WaitIdle -TimeoutSec 15 }
  if (-not (Wait-Until { (Get-Panes).Count -ge 1 } -TimeoutSec 10)) { throw 'the test window has no pane' }
  $win1 = (Get-Panes)[0].window_id

  # Five tabs, named; charlie's in another folder for Duplicate to copy. With -Mux they're made
  # in the server, as panes there make theirs: a spawn through the GUI's socket into a client
  # domain lands in windows of its own
  if ($Mux) {
    # The GUI starts the server as it connects: wait until the server answers with the pane
    if (-not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }
  }
  $origin = (Get-Panes -Server:$Mux)[0].pane_id
  $names = 'alpha', 'bravo', 'charlie', 'delta', 'echo'
  Invoke-Cli set-tab-title --pane-id $origin 'alpha' -Server:$Mux | Out-Null
  foreach ($n in $names[1..4]) {
    $cwd = if ($n -eq 'charlie') { 'C:\Windows\System32' } else { 'C:\Windows' }
    $pane = New-Tab -From $origin -Cwd $cwd -Server:$Mux
    Invoke-Cli set-tab-title --pane-id $pane $n -Server:$Mux | Out-Null
  }
  if (-not (Wait-Until { @(Get-Tabs $win1).Count -eq 5 })) {
    throw "the window has $(@(Get-Tabs $win1).Count) tabs, not 5: $((Get-Panes | ForEach-Object { "w$($_.window_id)/t$($_.tab_id)/p$($_.pane_id) '$($_.tab_title)'" }) -join '; ')"
  }
  $tabId = @{}
  foreach ($tab in Get-Tabs $win1) { $tabId[$tab.tab_title] = $tab.tab_id }
  $e = Wait-Change { param($s) @($s.rail.rows).Count -eq 5 } -What 'five rows on the rail'

  $px = { param($v) ConvertTo-Px $v $w.Scale }
  $cr = Get-ClientRect $w
  # Row i's pad, where the rail draws it now
  $row = { param([int]$i) $script:e = Wait-Settled -What 'the rail'; Get-RectCenter $script:e.rail.rows[$i].pad }
  $paneSpot = @((& $px 600), (& $px 300))
  # The tab menu, opened by a right click on row i: the window's entry once it shows
  $menu = { param([int]$i)
    $x, $y = & $row $i
    Send-Click $w $x $y -Right -Move
    Wait-Change { param($s) $s.menu.kind -eq 'tab' } -What "the tab menu on row $i"
  }
  # An item of the open menu, by label: a click on it
  $pick = { param($m, [string]$label)
    $it = @($m.menu.items | Where-Object label -eq $label)[0]
    if (-not $it) { throw "no '$label' in the tab menu: $(@($m.menu.items | ForEach-Object label) -join ', ')" }
    $x, $y = Get-RectCenter $it.rect
    Send-Click $w $x $y -Move
  }

  # 1. The menu, and Esc
  $m = & $menu 1
  (Get-Shot $w -Name 'menu').Dispose()
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'Esc to close the menu' | Out-Null
  (Get-Shot $w -Name 'menu-closed').Dispose()

  # 2. Teal (the fifth colour, after None) on bravo: its swatch sits on the Colour row, 20 px
  # apart from the menu's left edge (menu.rs)
  $m = & $menu 1
  $colour = @($m.menu.items | Where-Object label -eq 'Colour')[0]
  $sx = $m.menu.rect.x + (& $px (74 + 20 * 5 + 7)); $sy = (Get-RectCenter $colour.rect)[1]
  Send-Click $w $sx $sy -Move
  Send-Mouse $w Move $paneSpot[0] $paneSpot[1]
  Wait-Change { param($s) -not $s.menu -and $s.tabs[1].colour -and -not $s.rail.open } -What 'the colour on bravo, the panel closed' | Out-Null
  $shot = Get-Shot $w -Name 'colour'
  $pads = (Wait-Settled).rail.rows
  $count = { param($i)
    $y0 = (Get-RectCenter $pads[$i].pad)[1] - (& $px 22)
    [LbtermE2E.Px]::CountRange($shot, (New-Rect 0 $y0 (& $px 80) (& $px 44)), $teal)
  }
  $ringed = & $count 1; $plain = & $count 0
  $shot.Dispose()
  "colour: $ringed teal pixels round bravo's pad, $plain round alpha's"
  if ($ringed -lt 20 -or $plain -gt 5) { Add-Failure "teal round bravo's pad: $ringed pixels, round alpha's: $plain" }

  # 3. Rename bravo, typed in place
  $m = & $menu 1
  & $pick $m 'Rename tab'
  # The old name comes up selected, so typing replaces it
  Wait-State { param($s) $null -ne $s.menu.renaming } -What 'the name to type in' | Out-Null
  (Get-Shot $w -Name 'rename').Dispose()
  Send-Chars $w 'moved'
  Send-Key $w Enter
  if (-not (Wait-Until { (Get-Tabs $win1 | Where-Object tab_id -eq $tabId.bravo).tab_title -eq 'moved' })) {
    Add-Failure "bravo is titled '$((Get-Tabs $win1 | Where-Object tab_id -eq $tabId.bravo).tab_title)' after Rename, not 'moved'"
  } else { 'rename: bravo is now moved' }
  if ($Mux) {
    $inServer = @(Get-Panes -Server | Where-Object tab_title -eq 'moved').Count
    if ($inServer -ne 1) { Add-Failure "the server has $inServer tabs titled 'moved', not 1" } else { 'rename: the server has it too' }
  }

  # 4. Duplicate charlie, then close the copy from the keyboard
  $m = & $menu 2
  & $pick $m 'Duplicate tab'
  $copy = $null
  if (Wait-Until { @(Get-Tabs $win1).Count -eq 6 }) {
    $copy = Get-Tabs $win1 | Where-Object { $_.tab_id -notin $tabId.Values } | Select-Object -First 1
    "duplicate: tab $($copy.tab_id) in $($copy.cwd)"
    if ($copy.cwd -notmatch 'System32') { Add-Failure "the duplicate is in $($copy.cwd), not charlie's System32" }
  } else { Add-Failure "Duplicate left $(@(Get-Tabs $win1).Count) tabs, not 6" }
  if ($copy) {
    Wait-Change { param($s) @($s.rail.rows).Count -eq 6 } -What 'six rows on the rail' | Out-Null
    $idx = [array]::IndexOf(@(Get-Tabs $win1 | ForEach-Object tab_id), $copy.tab_id)
    & $menu $idx | Out-Null
    Send-Key $w Up      # from nothing to the last row, Close tab
    Send-Key $w Enter
    # A mux server's pane can't say it's idle, so closing it asks first
    if ($Mux) {
      Wait-State { param($s) $s.overlay } -What 'the close to ask' | Out-Null
      (Get-Shot $w -Name 'close-asks').Dispose(); Send-Key $w Y
    }
    if (Wait-Until { @(Get-Tabs $win1).Count -eq 5 }) { 'keyboard: Up, Enter closed the duplicate' }
    else { Add-Failure "Up, Enter left $(@(Get-Tabs $win1).Count) tabs, not 5" }
  }

  # 5. Drag alpha from the first row to the fourth
  Wait-Change { param($s) @($s.rail.rows).Count -eq 5 } -What 'five rows again' | Out-Null
  $a = & $row 0; $b = & $row 3
  Send-Drag $w $a[0] $a[1] $b[0] $b[1]
  $order = @(Get-Tabs $win1 | ForEach-Object tab_title)
  "drag: $($order -join ', ')"
  if ($order[3] -ne 'alpha') { Add-Failure "alpha is at $([array]::IndexOf($order, 'alpha')) after the drag, not 3" }

  # 6. Drag echo (the last row) out below the window: a window of its own, where it's let go
  $o = ConvertTo-ScreenPoint $w 0 0
  $dropX = & $px 200; $dropY = $cr.Bottom + (& $px 120)
  $a = & $row 4
  Send-Drag $w $a[0] $a[1] $dropX $dropY
  $win2 = $null; $h2 = $null
  $one = { param($title) @(Get-Tabs | Where-Object tab_title -eq $title) }
  if (Wait-Until { $e = & $one 'echo'; $e.Count -eq 1 -and $e[0].window_id -ne $win1 } -TimeoutSec 8) {
    $win2 = (& $one 'echo')[0].window_id
    # The new window, as the state file has it: its HWND, once it has painted and settled
    Wait-State { param($e, $s) @($s.windows).Count -eq 2 -and -not (@($s.windows) | Where-Object { $_.frame -lt 1 -or -not $_.settled }) } -What 'the torn-off window, settled' | Out-Null
    $h2 = [IntPtr][long](@($T.LastState.windows) | Where-Object { [long]$_.hwnd -ne [long]$w.Handle } | Select-Object -First 1).hwnd
    if ($h2 -ne [IntPtr]::Zero) {
      $wr2 = Get-WindowRect $h2
      $want = @(($o.X + $dropX), ($o.Y + $dropY))
      "tear-off: echo in window $win2 at $($wr2.Left),$($wr2.Top); let go at $($want -join ',')"
      if ([Math]::Abs($wr2.Left - $want[0]) -gt (& $px 40) -or [Math]::Abs($wr2.Top - $want[1]) -gt (& $px 40)) {
        Add-Failure "the torn-off window is at $($wr2.Left),$($wr2.Top), not near $($want -join ',')"
      }
      (Get-Shot $h2 -Name 'torn-off').Dispose()
    } else { Add-Failure 'no second window for the torn-off tab' }
  } else { Add-Failure "after dragging echo out: $((& $one 'echo' | ForEach-Object { "window $($_.window_id)" }) -join ', ')" }

  # 7. Drag delta onto the new window
  if ($win2 -and $h2) {
    $c2 = ConvertTo-ScreenPoint $h2 0 0
    $idx = [array]::IndexOf(@(Get-Tabs $win1 | ForEach-Object tab_title), 'delta')
    $a = & $row $idx
    Send-Drag $w $a[0] $a[1] ($c2.X - $o.X + (& $px 300)) ($c2.Y - $o.Y + (& $px 150))
    if (Wait-Until { $d = & $one 'delta'; $d.Count -eq 1 -and $d[0].window_id -eq $win2 } -TimeoutSec 8) { 'move: delta joined the torn-off window' }
    else { Add-Failure "after dragging delta onto window ${win2}: $((& $one 'delta' | ForEach-Object { "window $($_.window_id)" }) -join ', ')" }
  }

  # 8. Close other tabs in the first window
  $m = & $menu 0
  & $pick $m 'Close other tabs'
  if ($Mux) {
    Wait-State { param($s) $s.overlay } -What 'closing the others to ask' | Out-Null
    (Get-Shot $w -Name 'close-others-asks').Dispose(); Send-Key $w Y
  }
  if (Wait-Until { @(Get-Tabs $win1).Count -eq 1 }) { 'close others: one tab left in the first window' }
  else { Add-Failure "Close other tabs left $(@(Get-Tabs $win1).Count) tabs in the first window" }
  Wait-Settled -What 'the window after closing the others' | Out-Null
  (Get-Shot $w -Name 'end').Dispose()
} -Keep:$Keep -Passed "tab-mouse$(if ($Mux) { ' (mux)' }): menu, colour, rename, duplicate, keys, drag, tear-off, move and close others all work"
