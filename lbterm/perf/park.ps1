#Requires -Version 7
# Covers: mux/src/tab.rs, mux/src/lbterm_parked.rs, mux/src/lbterm_repaint.rs, codec/src/lib.rs, wezterm-client/src/domain.rs, wezterm-client/src/client.rs, wezterm-client/src/discovery.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/dispatch.rs, wezterm-mux-server-impl/src/lbterm_flow.rs, wezterm-mux-server/src/main.rs, wezterm-gui/src/park.rs, wezterm-gui/src/main.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/state.rs, lbterm-panel/src/words.rs, lbterm-panel/src/palette.rs, lbterm-panel/src/look.rs, lbterm-panel/src/claude.rs, wezterm-gui/src/termwindow/compositor.rs, config/src/lbterm.rs
# Variant: -ServerBin installed
<#
Parking a tab (Lachlan's 8a; lbterm/NEW-TAB.md, "Landing 4"; lbterm/MUX.md, "Parked tabs"), end
to end, on a mux domain of the test's own as Lachlan's tabs are. A test window with three tabs on
the test's server: one (the open tab, working), two (working) and three (a question).

1. The server publishes `<socket>.parked.json` as it starts serving, naming no pane; no row says
   `parked`; the rail closed (park-before.png).
2. Two's tab menu (a right click on its pad) has "Park tab (P)" first in Wait on…'s group; P parks
   it: the menu goes, the state file's row says `parked`, the file names two's pane (the server's
   id), and two's pad is drawn 40% of the way from what's under it (each channel of a pixel inside
   the pad within 8 of the blend of the rail's colour and the pixel as it was), the other pads as
   they were (park-two.png).
3. A split in two (parked) adds its new pane to the file.
4. `LbtermPark 'Toggle'` (the event test-park performing it, through the control pipe, as LyreBird
   fires lbterm-park-toggle) parks the open tab: its row says so, the file names its pane, and its
   ring and bar beside the pill stay (park-open-tab.png). Open, the panel shows the rows' words, the
   parked ones with a Parked chip (park-panel.png, for the eye).
5. Two's menu says "Unpark tab (P)"; P unparks it: its row clear, the file without its panes, its
   pad as it was.
6. Closing the parked open tab takes its pane out of the file.
7. A window with no mux domain (the GUI's own tabs), its one tab a Claude session done with a
   background task still running, so its pad's arc turns (`rail.spinner` among the loops): the
   action parks it, and its arc is drawn still and dimmed with its pad (none of the full Working
   green in the arc's square, pixels of the 40% blend over the pill, nothing changing in 350 ms,
   no `rail.spinner` loop: Row::arc_turns, PITFALLS 255); the GUI writes
   `gui-sock-<pid>.parked.json` naming its pane, and it's still there 13 s after the window
   started, past the update checker's sweep of dead gui-sock-* at 10 s (PITFALLS 254; removed
   after).

-ServerBin installed (or a build's folder) runs another build's lbterm-mux-server for the domain:
one from before 8a answers GetParkedTabs with an error, so the menus offer no Park tab, P goes
nowhere, the action does nothing, no file is published, and the connection stays (PITFALLS 159,
188): the run checks that instead of steps 2 to 6.

Its USERPROFILE is a scratch folder, so lbterm.lua's handlers write nothing into the real
~\.claude, Lachlan's ~\.claude\parked included. Fails with exit 1.

  pwsh -File lbterm\perf\park.ps1 [-Bin <build>] [-ServerBin <build>|installed]
#>
param([string]$Bin, [string]$ServerBin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'park' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $home2 | Out-Null
$old = [bool]$ServerBin
if ($ServerBin -eq 'installed') {
  $ServerBin = Join-Path $T.Out 'installed-server'
  New-Item -ItemType Directory -Force $ServerBin | Out-Null
  foreach ($f in 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe') { Copy-Item (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $ServerBin }
}
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$published = "$($T.Sock).parked.json"

# Claude Code's glyph starts each title, so the rail reads the hooks' state as a Claude session's
function Demo([string]$Title, [string]$State, [double]$Age = 0) {
  New-PwshProgram -File $demoTab -ArgumentList @('-Title', $Title, '-State', $State, '-AgeMinutes', $Age)
}
function Read-Published([string]$Path = $published) {
  if (-not (Test-Path -LiteralPath $Path)) { return $null }
  Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}
function Format-Panes($p) { if ($null -eq $p) { 'no file' } else { "[$(@($p.panes) -join ',')]" } }
# The rows by their labels, as the state file has them
function Get-Row($s, [string]$Label) { @($s.rail.rows | Where-Object label -eq $Label)[0] }
function Menu-Rows($m) { @($m.menu.items | ForEach-Object { if ($_.key) { "$($_.label) ($($_.key))" } else { $_.label } }) -join ', ' }

$lua = @"
config.window_close_confirmation = 'NeverPrompt'
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true, open_delay_ms = 0 }
-- The action as LyreBird's lbterm-park-toggle will reach it, through the control pipe
wezterm.on('test-park', function(window, pane) window:perform_action(wezterm.action.LbtermPark 'Toggle', pane) end)
"@

Invoke-Test {
  $cfg = Join-Path $T.Out 'config\park.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua (@"
config.unix_domains = { {
  name = 'park',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@ + $lua)
  $w = Open-TestWindow -ConfigFile $cfg -Domain 'park' -Socket $T.Sock -UserProfile $home2 -PostedPointer `
    -Program (Demo one working) -TimeoutSec 20
  if (-not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  $onePane = [int](Get-Panes -Server)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $onePane 'one' -Server | Out-Null
  $twoPane = New-Tab -From $onePane -NoActivate -Server -Program (Demo two working)
  Invoke-Cli set-tab-title --pane-id $twoPane 'two' -Server | Out-Null
  $threePane = New-Tab -From $onePane -NoActivate -Server -Program (Demo three waiting 2)
  Invoke-Cli set-tab-title --pane-id $threePane 'three' -Server | Out-Null
  $s = Wait-Change { param($s) @($s.rail.rows).Count -eq 3 -and (Get-Row $s 'three') -and (Get-Row $s 'one').active } -What 'three rows, one open' -Animating
  $px = { param($v) ConvertTo-Px $v $w.Scale }

  # 1. The file as the server starts serving, and nothing parked
  $p = Read-Published
  "1. published: $(Format-Panes $p) (pid $($p.pid))"
  if ($old) {
    if ($p) { Add-Failure "a server from before 8a published $published" }
  } elseif (-not $p -or @($p.panes).Count) { Add-Failure "the server didn't publish an empty $published as it started serving: $(Format-Panes $p)" }
  if (@($s.rail.rows | Where-Object parked).Count) { Add-Failure 'a row says parked before anything was' }

  # The tab menu on a row: a right click on its pad
  $menu = { param([string]$label)
    $e = Wait-Settled -What 'the rail'
    $x, $y = Get-RectCenter (Get-Row $e $label).pad
    Send-Click $w $x $y -Right -Move
    Wait-Change { param($s) $s.menu.kind -eq 'tab' } -What "the tab menu on $label"
  }
  # The pointer off the window, the rail closed and no menu
  $leave = {
    Send-Leave $w
    Wait-Change { param($s) -not $s.rail.open -and -not $s.menu } -What 'the rail closed' -Animating
  }
  # A pixel inside a row's pad, clear of its number and corner marks, and the rail beside the pad
  $padPoint = { param($row) @([int]($row.pad.x + (& $px 4)), [int]($row.pad.y + $row.pad.h / 2)) }
  $railPoint = { param($row) @([int]($row.pad.x - (& $px 5)), [int]($row.pad.y + $row.pad.h / 2)) }
  # The pads as they were, once each stand-in's state has come and its pad's colour has settled:
  # taken as three's state arrived, its orange pad was caught part way in (118,74,50 for
  # 255,138,61; the e2e of 2026-11-09 19:32), and step 2 judged three against that
  Wait-Change { param($s) (Get-Row $s 'two').state -eq 'working' -and (Get-Row $s 'three').state -eq 'waiting' } -What 'two working and three waiting' -Animating | Out-Null
  $s = Wait-Settled -What 'the rail'
  $before = Get-Shot $w -Name 'park-before'
  $beforeRows = @{}
  foreach ($r in $s.rail.rows) { $beforeRows[$r.label] = $r }
  $pixel = { param($shot, $pt) $shot.GetPixel($pt[0], $pt[1]) }

  if ($old) {
    # Against a server that can't park: no row, P goes nowhere, the action does nothing, and the
    # window stays connected
    $m = & $menu 'two'
    "older server, two's menu: $(Menu-Rows $m)"
    if (@($m.menu.items | Where-Object key -eq 'P').Count) { Add-Failure "a server that can't park had Park tab offered: $(Menu-Rows $m)" }
    $f = $m.frame
    Send-Key $w P
    $after = Wait-State { param($s) $true } -AfterFrame $f -What 'a frame after P'
    if (-not $after.menu) { Add-Failure 'P with no Park row closed the menu' }
    Send-Key $w Escape
    & $leave | Out-Null
    Send-Emit 'test-park' -Window $w
    Start-Sleep -Milliseconds 500
    $after = Wait-Settled -What 'the rail after the action'
    if (@($after.rail.rows | Where-Object parked).Count) { Add-Failure 'the action parked a tab on a server that can''t park' }
    if (@($after.rail.rows).Count -ne 3) { Add-Failure "the window has $(@($after.rail.rows).Count) rows after the action, not 3: the connection dropped?" }
    if (Read-Published) { Add-Failure "a server from before 8a has $published" }
    $before.Dispose()
    return
  }

  # 2. Park two from its menu
  $m = & $menu 'two'
  $rows = Menu-Rows $m
  "2. two's menu: $rows"
  $items = @($m.menu.items | ForEach-Object label)
  $at = [array]::IndexOf($items, 'Park tab')
  if ($at -lt 0 -or @($m.menu.items)[$at].key -ne 'P') { Add-Failure "two's menu has no 'Park tab (P)': $rows" }
  Send-Key $w P
  $s = Wait-Change { param($s) -not $s.menu -and (Get-Row $s 'two').parked } -What 'two parked' -Animating
  & $leave | Out-Null
  $s = Wait-Settled -What 'the rail'
  $p = Read-Published
  "   published: $(Format-Panes $p)"
  if (-not $p -or @($p.panes) -notcontains $twoPane) { Add-Failure "two's pane $twoPane isn't in the published file: $(Format-Panes $p)" }
  if (@($p.panes) -contains $onePane -or @($p.panes) -contains $threePane) { Add-Failure "an unparked tab's pane is in the published file: $(Format-Panes $p)" }
  if ((Get-Row $s 'one').parked -or (Get-Row $s 'three').parked) { Add-Failure 'another row says parked' }
  $shot = Get-Shot $w -Name 'park-two'
  Save-Zoomed $shot (New-Rect 0 0 (& $px 80) ([Math]::Min($shot.Height, (& $px 320)))) 'park-two-rail'
  $checkDim = { param($shot, [string]$label, [bool]$dim)
    $row = $beforeRows[$label]
    $was = & $pixel $before (& $padPoint $row)
    $now = & $pixel $shot (& $padPoint $row)
    $under = & $pixel $before (& $railPoint $row)
    $want = foreach ($c in 'R', 'G', 'B') { if ($dim) { [int][Math]::Round($under.$c + ($was.$c - $under.$c) * 0.4) } else { $was.$c } }
    $got = @($now.R, $now.G, $now.B)
    "   $label's pad: was $($was.R),$($was.G),$($was.B) over $($under.R),$($under.G),$($under.B); now $($got -join ','), want $($want -join ',')"
    foreach ($k in 0..2) {
      if ([Math]::Abs($got[$k] - $want[$k]) -gt 8) { Add-Failure "$label's pad is $($got -join ','), not $($want -join ',') ($(if ($dim) { 'dimmed' } else { 'as it was' }))"; break }
    }
  }
  & $checkDim $shot 'two' $true
  & $checkDim $shot 'three' $false
  $shot.Dispose()

  # 3. A split in the parked tab
  $got = (Invoke-Cli split-pane --pane-id $twoPane --right -Server -Program (Demo split working) -Merge) -join ' '
  if ($got.Trim() -notmatch '^\d+$') { throw "split-pane printed: $got" }
  $splitPane = [int]$got.Trim()
  $ok = Wait-Until { $q = Read-Published; $q -and @($q.panes) -contains $splitPane } -TimeoutSec 5 -IntervalMs 100
  "3. after a split in two: $(Format-Panes (Read-Published))"
  if (-not $ok) { Add-Failure "the split's pane $splitPane isn't in the published file" }

  # 4. The action parks the open tab
  Send-Emit 'test-park' -Window $w
  $s = Wait-Change { param($s) (Get-Row $s 'one').parked } -What 'one parked by the action' -Animating
  $s = Wait-Settled -What 'the rail'
  $p = Read-Published
  "4. the action: one parked, published $(Format-Panes $p)"
  if (@($p.panes) -notcontains $onePane) { Add-Failure "the open tab's pane $onePane isn't in the published file: $(Format-Panes $p)" }
  if (-not (Get-Row $s 'one').active) { Add-Failure 'parking the open tab moved off it' }
  $shot = Get-Shot $w -Name 'park-open-tab'
  # Its ring and bar are the open tab's, not the row's: they stay lit
  $accent = New-ColourRange 201 255 51 129 41 119
  $bar = [LbtermE2E.Px]::CountRange($shot, (Get-Columns (& $px 5) (& $px 9) -Of $shot), $accent)
  "   $bar accent pixels where the open tab's bar goes"
  if ($bar -lt (& $px 20)) { Add-Failure "the parked open tab lost its red bar ($bar pixels)" }
  & $checkDim $shot 'one' $true
  $shot.Dispose()
  # Open: the rows' words, a Parked chip on the parked ones, for the eye
  Set-PointerHold $w (& $px 40) (& $px 160)
  Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating | Out-Null
  (Get-Shot $w -Name 'park-panel').Dispose()
  Clear-PointerHold
  & $leave | Out-Null

  # 5. Unpark two from its menu
  $m = & $menu 'two'
  $rows = Menu-Rows $m
  "5. two's menu: $rows"
  if (-not @($m.menu.items | Where-Object { $_.label -eq 'Unpark tab' -and $_.key -eq 'P' }).Count) { Add-Failure "parked, two's menu has no 'Unpark tab (P)': $rows" }
  Send-Key $w P
  $s = Wait-Change { param($s) -not $s.menu -and -not (Get-Row $s 'two').parked } -What 'two unparked' -Animating
  & $leave | Out-Null
  $p = Read-Published
  "   published: $(Format-Panes $p)"
  if (@($p.panes) -contains $twoPane -or @($p.panes) -contains $splitPane) { Add-Failure "two's panes are still in the published file: $(Format-Panes $p)" }
  # Two's pad as it was (its split's narrower pane is the same tab's: the pad's colour is the tab's)
  $s = Wait-Settled -What 'the rail'
  $shot = Get-Shot $w -Name 'park-unparked'
  & $checkDim $shot 'two' $false
  $shot.Dispose()

  # 6. The parked open tab closed
  Invoke-Cli kill-pane --pane-id $onePane -Server | Out-Null
  $ok = Wait-Until { $q = Read-Published; $q -and @($q.panes) -notcontains $onePane } -TimeoutSec 5 -IntervalMs 100
  "6. after closing one: $(Format-Panes (Read-Published))"
  if (-not $ok) { Add-Failure "the closed tab's pane $onePane is still in the published file" }
  $before.Dispose()
  Stop-TestWindow $w

  # 7. The GUI's own tabs, with no mux domain: a Claude session done with one background task of
  # its own still running (OSC 7501 records, as pad-waiting.ps1's), so its pad's arc turns
  $cfg2 = Join-Path $T.Out 'config\park-local.lua'
  New-TestConfig -Path $cfg2 -Lua $lua
  $seq = ('state=done:app=claude', 'state=working:id=bg/b1' | ForEach-Object { "`$e]7501;$_`$e\" }) -join ''
  $tasks = New-PwshProgram "`$e = [char]27; [Console]::Write(`"$seq`"); Write-Host 'local'; Start-Sleep 600"
  $w2 = Open-TestWindow -ConfigFile $cfg2 -UserProfile $home2 -Program $tasks -TimeoutSec 20
  $started = (Get-Process -Id $w2.Pid).StartTime
  $s = Wait-Change { param($s) $r = @($s.rail.rows); $r.Count -eq 1 -and $r[0].waiting -eq 1 -and @($s.loops) -contains 'rail.spinner' } -What 'its row waiting on its task, the arc turning' -Animating
  $gui = "$($w2.Socket).parked.json"
  if (Test-Path -LiteralPath $gui) { Add-Failure "the GUI published $gui with nothing parked" }
  $row = @($s.rail.rows)[0]
  $px2 = { param($v) ConvertTo-Px $v $w2.Scale }
  # The arc's square, on the pad's top right corner (lbterm_panel::layout's spinner), and the pill
  # beside the pad that a parked row's colours are dimmed toward
  $half = (& $px2 6) + 1
  $square = New-Rect ([int]($row.pad.x + $row.pad.w - (& $px2 3) - $half)) ([int]($row.pad.y + (& $px2 3) - $half)) (2 * $half) (2 * $half)
  $green = 0x3DDC84
  $turning = Get-Shot $w2 -Name 'park-local-turning'
  $pill = $turning.GetPixel([int]($row.pad.x - (& $px2 5)), [int]($row.pad.y + $row.pad.h / 2))
  $lit = [LbtermE2E.Px]::CountNear($turning, $square, $green, 8)
  $turning.Dispose()
  Send-Emit 'test-park' -Window $w2
  Wait-Change { param($s) @($s.rail.rows)[0].parked } -What 'its tab parked' | Out-Null
  # A parked row's arc is still and dimmed with its pad: no loop of DWM's for it (Row::arc_turns)
  $s = Wait-Change { param($s) @($s.loops) -notcontains 'rail.spinner' } -What 'the parked row''s arc still' -Animating
  $s = Wait-Settled -What 'the rail'
  $a = Get-Shot $w2 -Name 'park-local-parked'
  Start-Sleep -Milliseconds 350
  $b = Get-Shot $w2
  $moved = [LbtermE2E.Px]::Differing($a, $b, $square)
  # The arc's core, opaque on a disc of the pill's colour: 40% of the way from the pill to the green
  $dimGreen = foreach ($pair in @(@($pill.R, 0x3D), @($pill.G, 0xDC), @($pill.B, 0x84))) { [int][Math]::Round($pair[0] + ($pair[1] - $pair[0]) * 0.4) }
  $dim = [LbtermE2E.Px]::CountNear($a, $square, ($dimGreen[0] -shl 16) -bor ($dimGreen[1] -shl 8) -bor $dimGreen[2], 8)
  $full = [LbtermE2E.Px]::CountNear($a, $square, $green, 8)
  $a.Dispose(); $b.Dispose()
  "7. the GUI's own tab, waiting on its task: $lit green pixels in the arc's square turning; parked, $full green and $dim of the blend $($dimGreen -join ',') over the pill's $($pill.R),$($pill.G),$($pill.B), $moved changed in 350 ms; loops $(@($s.loops) -join ', ')"
  if ($lit -lt 3) { Add-Failure "the turning arc showed only $lit green pixels in its square: the check would see nothing" }
  if ($full -gt 0) { Add-Failure "the parked row's arc has $full pixels of the full Working green: the compositor's undimmed template" }
  if ($dim -lt 3) { Add-Failure "the parked row's arc has only $dim pixels of the dimmed green" }
  if ($moved -gt 0) { Add-Failure "the parked row's arc moved ($moved pixels in 350 ms)" }
  if (@($s.loops) -contains 'rail.spinner') { Add-Failure "rail.spinner among the loops with only a parked row waiting: $(@($s.loops) -join ', ')" }
  $ok = Wait-Until { $q = Read-Published $gui; $q -and @($q.panes).Count -eq 1 -and $q.pid -eq $w2.Pid } -TimeoutSec 5 -IntervalMs 100
  "   published: $gui $(Format-Panes (Read-Published $gui))"
  if (-not $ok) { Add-Failure "the GUI didn't publish its parked tab's pane in $gui" }
  # Every GUI's update checker sweeps RUNTIME_DIR's gui-sock-* 10 s after it starts (AT-REST.md;
  # PITFALLS, Watching files): the file must outlive it. Nothing in the state file says the sweep
  # ran, so this waits on the clock: till 13 s after the window's process started
  $wait = 13000 - ((Get-Date) - $started).TotalMilliseconds
  if ($wait -gt 0) { Start-Sleep -Milliseconds ([int]$wait) }
  $q = Read-Published $gui
  "   $([int]((Get-Date) - $started).TotalSeconds) s after the window started: $(Format-Panes $q)"
  if (-not $q -or @($q.panes).Count -ne 1) { Add-Failure "the GUI's $gui was gone $([int]((Get-Date) - $started).TotalSeconds) s after it started, its tab still parked: swept as a dead gui-sock-*" }
  Stop-TestWindow $w2
  Remove-Item -LiteralPath $gui, "$gui.tmp" -ErrorAction SilentlyContinue
} -Passed 'park: Park tab and the action park and unpark, at the server and in the GUI, the rows dimmed and the panes published beside the socket'
