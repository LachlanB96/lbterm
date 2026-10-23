#Requires -Version 7
# Covers: wezterm-gui/src/state_probe.rs, wezterm-gui/src/termwindow/state.rs, lbterm-extras/src/state.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, lbterm-panel/src/hover.rs
<#
The state file (lbterm/STATE.md) end to end: a test window of its own class with LBTERM_STATE_FILE
set, the compositor and the tab panel on and motion at -Slowdown, driven with `lbterm cli` and
posted messages. Each step waits on what the file says, not on a capture or a fixed sleep:

1. The window opens: the file names it (this process, its HWND), one tab and one pane, settled.
2. A tab made without taking the keyboard (`cli spawn --no-activate`): its row grows into the
   rail. The file says animating while it does (rail.row.grow, rail.row.enter), then
   `animating: false` on a later frame, two rows, the first tab still the open one.
3. A split (`cli split-pane --right`): the panes move and the new one comes in (pane.layout,
   pane.enter), then settle: two panes side by side, the new one active.
4. The rail opening: a posted move onto the first row's pad, where the file says it is. At once
   the ring's light goes to pending and a wait, rail.open, is under way; then the panel opens
   (rail.edge) and settles open, full, with the pointer on rail.row 0.
5. The pointer leaving (a posted WM_MOUSELEAVE): the panel waits out its grace (rail.close),
   closes and settles closed.
6. The tab menu: a posted right click on the second row's pad opens it for that tab, its six
   items where the file says; a posted move onto Duplicate tab's rectangle picks it (focus, and
   the pointer over menu.item); a posted Esc closes it with nothing done.
7. A second test window, with no config, so on the default front end (not WebGpu), and the
   variable set: it stays alive, and its file names it by its HWND. (Until 9fbea202d the GUI
   asked the window for its HWND through window_handle() while it was borrowed, and a window like
   this one panicked within a second, window.rs:778.)
Throughout, the process's frame count and the window's only go up, and the window never takes
the foreground.

Every snapshot seen is logged to target\perf\state\seen.txt and the last one kept as
last-state.json. Fails with exit 1.

  pwsh -File lbterm\perf\state.ps1 [-Slowdown 3]
#>
param([double]$Slowdown = 3, [string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'state' -Bin $Bin
$home2 = Join-Path $T.Out 'home'

$seen = [Collections.Generic.List[object]]::new()
$clock = [Diagnostics.Stopwatch]::StartNew()
$script:last = $null
$script:lastProcessFrame = -1
$script:lastWindowFrame = -1

# Each new snapshot: logged, and its frame counts checked
$log = {
  param($w, $s)
  if ($s.frame -lt $script:lastProcessFrame) { Add-Failure "the process's frame went back from $($script:lastProcessFrame) to $($s.frame)" }
  if ($w.frame -lt $script:lastWindowFrame) { Add-Failure "the window's frame went back from $($script:lastWindowFrame) to $($w.frame)" }
  $script:lastProcessFrame = $s.frame
  $script:lastWindowFrame = $w.frame
  $script:last = $s
  $seen.Add([pscustomobject]@{
      Ms         = $clock.ElapsedMilliseconds
      Frame      = $w.frame
      Animating  = $w.animating
      Animations = @($w.animations | ForEach-Object { $_.name })
      Waits      = @($w.waits | ForEach-Object { $_.name })
      Step       = $script:step
    })
}

# This window in each new snapshot, until -Until says yes; a failure and nothing after -Seconds
function Watch([scriptblock]$Until, [string]$What, [double]$Seconds = 15) {
  $script:step = $What
  try { Wait-State $Until -What $What -TimeoutSec $Seconds -OnState $log }
  catch { Add-Failure $_.Exception.Message; $null }
}

# The animations named in the snapshots logged since `from`
function Seen-Since([int]$from) {
  @($seen | Select-Object -Skip $from | ForEach-Object { $_.Animations } | Sort-Object -Unique)
}

Invoke-Test {
  $win = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo -Program (New-PwshProgram) -Lua @"
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true, open_delay_ms = 150, close_delay_ms = 300 }
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.lbterm_motion = { enabled = 'on', slowdown = $Slowdown }
"@

  # 1. The window is in the file, settled, one tab and one pane
  $w = Watch { param($w) $w.frame -gt 0 -and $w.settled -and $w.rail -and $w.panes.Count -eq 1 } 'the window, settled'
  if ($w) {
    if ($script:last.pid -ne $win.Pid) { Add-Failure "the file's pid is $($script:last.pid), not $($win.Pid)" }
    if ($script:last.version -ne 1) { Add-Failure "version $($script:last.version)" }
    if ($w.tabs.Count -ne 1) { Add-Failure "$($w.tabs.Count) tabs at the start" }
    "1. window $($w.id) (hwnd $($w.hwnd)) at frame $($w.frame): $($w.tabs.Count) tab, $($w.panes.Count) pane, settled"
  }
  $first = $w.active_pane

  # 2. A new tab, made without taking the keyboard: its row grows in, then everything settles
  $from = $seen.Count
  $two = New-Tab -From $first -NoActivate -Program (New-PwshProgram)
  $grown = Watch { param($w) $w.rail.rows.Count -eq 2 } 'the new tab on the rail'
  $w = Watch { param($w) $w.rail.rows.Count -eq 2 -and -not $w.animating } 'the new row to settle'
  $names = Seen-Since $from
  "2. new tab (pane $two): animations seen $($names -join ', '); settled at frame $($w.frame)"
  if ($names -notcontains 'rail.row.grow' -or $names -notcontains 'rail.row.enter') { Add-Failure "the new row's growing in wasn't in the file: $($names -join ', ')" }
  if ($w) {
    if ($w.active_tab -ne 0) { Add-Failure "the new tab took the window: active_tab $($w.active_tab)" }
    if (-not $w.rail.rows[0].active) { Add-Failure 'the first row lost the open tab' }
  }

  # 3. A split of the first tab's pane: the panes move and the new one comes in, then settle
  $from = $seen.Count
  $split = [int](Invoke-Cli split-pane --pane-id $first --right -Program (New-PwshProgram))
  $w = Watch { param($w) $w.panes.Count -eq 2 } 'the split'
  $w = Watch { param($w) $w.panes.Count -eq 2 -and -not $w.animating } 'the split to settle'
  $names = Seen-Since $from
  "3. split (pane $split): animations seen $($names -join ', '); settled at frame $($w.frame)"
  if (-not ($names | Where-Object { $_ -like 'pane.*' })) { Add-Failure "the split moved nothing the file saw: $($names -join ', ')" }
  if ($w) {
    $left, $right = @($w.panes | Sort-Object { $_.rect.x })
    if ($right.rect.x -le $left.rect.x + $left.rect.w - 1) { Add-Failure "the panes aren't side by side: $($left.rect | ConvertTo-Json -Compress) $($right.rect | ConvertTo-Json -Compress)" }
    if ($w.active_pane -ne $split) { Add-Failure "the active pane is $($w.active_pane), not the new $split" }
  }

  # 4. The pointer onto the first row's pad: the light at once, then the panel opens and settles
  $x, $y = Get-RectCenter $w.rail.rows[0].pad
  Set-PointerHold $win $x $y
  $from = $seen.Count
  $waiting = Watch { param($w) $w.rail.phase -eq 'opening' -or $w.rail.open } 'the rail to answer the pointer'
  if ($waiting -and $waiting.rail.phase -eq 'opening') {
    "4. pointer on the pad: phase opening, light $($waiting.rail.light), waits $(($waiting.waits | ForEach-Object { "$($_.name) $($_.left_ms) ms" }) -join ', ')"
    if ($waiting.rail.light -ne 'pending') { Add-Failure "the ring's light was $($waiting.rail.light) while it waited to open" }
    if (-not ($waiting.waits | Where-Object name -eq 'rail.open')) { Add-Failure 'no rail.open wait while it waited to open' }
    if ($waiting.settled) { Add-Failure 'settled while the rail waited to open' }
  } elseif ($waiting) {
    Add-Failure 'the rail opened without a frame saying it waited'
  }
  $w = Watch { param($w) $w.rail.open -and -not $w.animating } 'the rail to open and settle'
  $names = Seen-Since $from
  "   opened: animations seen $($names -join ', '); settled at frame $($w.frame), pointer over $($w.pointer.over) $($w.pointer.row)"
  if ($names -notcontains 'rail.edge') { Add-Failure "the panel's edge moving wasn't in the file: $($names -join ', ')" }
  if ($w) {
    $script:last | ConvertTo-Json -Depth 20 | Set-Content (Join-Path $T.Out 'rail-open.json')
    if ($w.rail.light -ne 'full') { Add-Failure "open, the ring's light is $($w.rail.light)" }
    if ($w.pointer.over -ne 'rail.row' -or $w.pointer.row -ne 0) { Add-Failure "the pointer is over $($w.pointer.over) $($w.pointer.row), not rail.row 0" }
    if ($w.rail.hovered -ne 0) { Add-Failure "the hovered row is $($w.rail.hovered)" }
  }

  # 5. The pointer leaves the window: the grace, then the panel closes and settles
  Clear-PointerHold
  Send-Leave $win
  $from = $seen.Count
  $closing = Watch { param($w) $w.rail.phase -ne 'open' } 'the rail to answer the pointer leaving'
  if ($closing) {
    "5. pointer gone: phase $($closing.rail.phase), waits $(($closing.waits | ForEach-Object { "$($_.name) $($_.left_ms) ms" }) -join ', ')"
    if ($closing.rail.phase -eq 'closing' -and -not ($closing.waits | Where-Object name -eq 'rail.close')) { Add-Failure 'no rail.close wait while it waited to close' }
    if ($null -ne $closing.pointer) { Add-Failure "the pointer is still in the file after it left: $($closing.pointer.over)" }
  }
  $w = Watch { param($w) $w.rail.phase -eq 'closed' -and $w.settled } 'the rail to close and settle'
  $names = Seen-Since $from
  "   closed: animations seen $($names -join ', '); settled at frame $($w.frame)"
  if ($names -notcontains 'rail.edge') { Add-Failure "the panel's edge closing wasn't in the file: $($names -join ', ')" }

  # 6. The tab menu: a right click on the second row's pad opens it, a move onto Duplicate tab,
  # where the file says it is, picks it, and Esc closes it
  $row = $w.rail.rows[1]
  $x, $y = Get-RectCenter $row.pad
  Send-Click $win $x $y -Right
  $m = Watch { param($w) $w.menu } 'the tab menu'
  if ($m) {
    $labels = @($m.menu.items | ForEach-Object label)
    "6. tab menu for tab $($m.menu.tab): $($labels -join ', ')"
    if ($m.menu.kind -ne 'tab' -or $m.menu.tab -ne $row.tab) { Add-Failure "the menu is $($m.menu.kind) for tab $($m.menu.tab), not the tab menu for $($row.tab)" }
    if ($labels.Count -ne 6 -or @($m.menu.items | Where-Object { -not $_.rect }).Count) { Add-Failure "the menu's items: $($m.menu.items | ConvertTo-Json -Compress -Depth 5)" }
    $dup = [Array]::IndexOf($labels, 'Duplicate tab')
    $x, $y = Get-RectCenter $m.menu.items[$dup].rect
    Set-PointerHold $win $x $y
    $w = Watch { param($w) $w.menu.focus -eq $dup -and $w.pointer.over -eq 'menu.item' } 'the pointer on Duplicate tab'
    if ($w) {
      "   the pointer on item $($w.pointer.item), focus $($w.menu.focus)"
      if ($w.pointer.item -ne $dup) { Add-Failure "the pointer is on item $($w.pointer.item), not Duplicate tab ($dup)" }
    }
    Clear-PointerHold
    Send-Key $win Escape
    $w = Watch { param($w) -not $w.menu } 'Esc to close the menu'
    if ($w) { "   Esc: the menu closed at frame $($w.frame)" }
    if (@($w.tabs).Count -ne 2) { Add-Failure "$(@($w.tabs).Count) tabs after the menu: something was picked" }
  }
  Send-Leave $win

  # 7. A window on the default front end with the variable set: alive, and in its own file by its
  # HWND (the state file of a window that died would never name it)
  $script:step = 'the default front end'
  $plain = $null
  try {
    $plain = Open-TestWindow -NoConfig -Program (New-PwshProgram)
    $p = Wait-State { param($w) $w.frame -gt 0 } -Window $plain -What 'the default front end''s window in its file' -TimeoutSec 15
    Start-Sleep 2
    if (-not (Test-WindowAlive $plain)) { Add-Failure "the window on the default front end (pid $($plain.Pid)) died; its log: $($plain.Log)" }
    else {
      $again = Get-StateWindow (Read-State -Window $plain) -Hwnd ([long]$plain.Handle)
      if (-not $again -or $again.frame -lt $p.frame) { Add-Failure "the default front end's file stopped naming its window (hwnd $([long]$plain.Handle))" }
      else { "7. default front end: pid $($plain.Pid) alive, its file names hwnd $($again.hwnd) at frame $($again.frame)" }
    }
  } catch { Add-Failure "the window on the default front end: $($_.Exception.Message)$(if ($plain) { " (alive: $(Test-WindowAlive $plain); its log: $($plain.Log))" })" }
  "$($seen.Count) snapshots seen; the log in $($T.Out)"
} -Finally {
  $seen | Format-Table -AutoSize -Wrap | Out-String -Width 220 | Set-Content (Join-Path $T.Out 'seen.txt')
  if ($script:last) { $script:last | ConvertTo-Json -Depth 20 | Set-Content (Join-Path $T.Out 'last-state.json') }
} -Passed 'state file: the window, a new tab growing in, a split, the rail opening and closing, each followed to animating false; the tab menu where the file says'
