#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/tab_mouse.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/termwindow/render/new_tab.rs, wezterm-gui/src/termwindow/render/menu_fade.rs, wezterm-gui/src/termwindow/tab_links.rs, wezterm-gui/src/termwindow/last_tab.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/state.rs, lbterm-extras/src/tabs.rs, lbterm-panel/src/**, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-client/src/domain.rs, mux/src/tab.rs, mux/src/window.rs, lbterm-motion/src/rail.rs, lbterm-motion/src/menu.rs, wezterm-gui/src/termwindow/compositor.rs
<#
Tabs with the mouse end to end (lbterm/TAB-MOUSE.md), in a test window of its own class on the
right-most screen that never takes focus. The pointer is posted mouse messages, so the real one
never moves, and the keys are posted key messages. Where to click (the rows' pads, the menu's
items) and when the window has answered come from its state file (lbterm/STATE.md).

1. The tab menu opens on a right-click on a tab's row in the tab panel (menu.png), and Esc
   closes it.
2. Its teal swatch rings that tab's pad (colour.png: teal pixels round the pad).
3. Rename tab, typed in place, renames it (in the server too, with -Mux).
4. Shell in its folder (Duplicate tab until landing 2) opens a tab in the same folder; the keyboard
   (Up, Enter: Close tab) closes it, without asking.
5. Dragging the first tab down the panel moves it to the slot let go on.
6. Dragging a tab out below the window and letting go gives it a window of its own, its
   top-left where it was let go (torn-off.png).
7. Dragging another tab onto that window moves it there.
8. Close other tabs leaves the first window one tab, without asking.
9. The rows a tab's facts give, each with its key (lbterm/NEW-TAB.md, section 7): a shell tab in no
   project has neither Copy link nor New session.
10. An unnamed Claude tab, its session labelled Tab Wave in the test's summaries and its folder in
    the config's project Sys, has both; R starts Rename from the label the row shows.
11. L, Copy link: its text, `[Tab Wave](lbterm://session/<id>) in Sys`, read from the state file's
    `copied`, where a window with the state file on puts it instead of the clipboard (no test may
    touch Lachlan's); the row picked flashes white (menu.tab.flash) and the pad glints
    (light.glint), once the menu's card has gone: no frame has the glint beside the card's fade
    out (menu.tab), Lachlan's 7b of 2026-11-08, nothing of the rail playing under a menu's card.
12. N, New session in Sys: the menu goes first, then it's a pick: lbterm-project-action with
    {action='new', source='tab menu'}, the tab's folder and the project's, its Starting row, and
    (no handler taking it here) PowerShell in the tab's folder, in front. Sys is kept as a recent
    pick in the recent picks' file, as a pick in the picker is (Lachlan's 8b).
13. That shell tab's R starts from its row's label (pwsh, where a test window once started it from
    the pane's title, pwsh.exe), and X closes it: its row fades where it was (rail.row.exit), and
    that and the marker's move play once the menu's card has gone, never beside its fade (7b).
14. Three more stand-in Claude tabs, each with its label and its objectives all done: Fin One and
    Fin Two say Your turn (an OSC 7501 idle record), Unread says Done with a result never shown
    (its white dot). Tab Wave's menu has Wait on… (W) and Close finished tabs (2) (F), whose second
    line names Fin One and Fin Two (`menu.items[].detail`). W turns the card to Wait on…'s page
    (‹ Back, then Fin One, Fin Two and Unread; its rows dissolve on swap, menu.tab.swap), Left turns
    it back, and picking Unread on the page fires lbterm-tab-link {action='toggle', waiter, on}
    with the two sessions.
15. Fin Two waits on Tab Wave through a tab link (the links file written as links.ts would), so
    it isn't finished (PITFALLS 153): Close finished tabs (1) names Fin One alone. F, with Fin One
    the open tab, closes it and Fin Two, Unread and Tab Wave stay; its row fades where it was
    (rail.row.exit) as the rows below slide up (rail.row.slide), its panes dissolve into the next
    tab's (pane.dissolve), and Fin Two, open now, slides up into Fin One's row with the marker
    riding on it (rail.marker on the rows' slide, ending as Fin Two's slide ends: Lachlan's 3b;
    judged here from left_ms for the plan render/tab_panel.rs makes of each real close, where
    compositor/imp/tests/rail.rs holds the compositor's half for one plan built by hand);
    its session has its close mark and it's kept to reopen (new_tab.closed), Fin Two's and
    Unread's sessions have none. None of the rail's moves shows beside the card's fade (7b).
16. A middle click on the + pad closes nothing; on Unread's row it closes that tab without asking,
    its close mark written and the tab kept to reopen, and F9 (bound to LbtermNewTab 'Reopen', a
    posted stand-in for Ctrl+Shift+T, since a posted key carries no modifier) fires
    lbterm-reopen-tab with its session and label and opens it again.
17. A middle click on the open tab's row with a row below it: that row slides up into its slot and
    opens, the marker riding up with it (rail.marker, ending as the row's slide does, 3b).
18. The same by Ctrl+W, his CloseCurrentTab, posted as F8 bound to it (a posted key carries no
    Ctrl): the marker rides up with the row taking the slot, as for the middle click and Close
    finished tabs, the three closes Lachlan's 3b names. F7 (bound to open a tab) first, while
    fewer than three rows are left for it and step 19.
19. A middle click on the open tab's row, the last: the tab above opens and the marker glides a row
    up to it (rail.marker, focus_move, ending after any slide would).

  pwsh -File lbterm\perf\tab-mouse.ps1          # tabs in the GUI (the local domain)
  pwsh -File lbterm\perf\tab-mouse.ps1 -Mux     # tabs in a test lbterm-mux-server, as LBTerm runs
#>
param([string]$Bin, [switch]$Mux, [switch]$Keep)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('tab-mouse' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\tabs.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
# The Claude tab's label and folder, as Terminal's workers would keep them (step 10), and each
# lbterm-project-action the window fires (step 12)
$summaries = Join-Path $T.Out 'summaries'
$events = Join-Path $T.Out 'events.jsonl'
# The recent picks' file, which New session in Sys adds Sys to (step 12)
$recent = Join-Path $T.Out 'recent.json'
# Each lbterm-tab-link and lbterm-reopen-tab the window fires (steps 14 and 16), the links file it
# reads (none written: no tab waits on another when it starts) and its close marks (15, 16)
$linkEvents = Join-Path $T.Out 'links.jsonl'
$reopens = Join-Path $T.Out 'reopen.jsonl'
$linksFile = Join-Path $T.Out 'tab-links.json'
$marks = Join-Path $T.Out 'marks'
New-Item -ItemType Directory -Force $marks | Out-Null

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
# perf\rail-labels.ps1 covers them. Each lbterm-project-action is written down and left to LBTerm's
# default (New session's: PowerShell in the folder, in front). One project, Sys, holding System32:
# charlie's folder and the Claude tab's
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
wezterm.on('lbterm-project-action', function(window, pane, p)
  local f = io.open([[$events]], 'a')
  if f then
    f:write(encode({ name = p.name, dir = p.dir, project_dir = p.project_dir, action = p.action, behind = p.behind, source = p.source, pane = pane:pane_id() }) .. '\n')
    f:close()
  end
end)
wezterm.on('lbterm-tab-link', function(window, pane, l)
  local f = io.open([[$linkEvents]], 'a')
  if f then
    f:write(encode({ action = l.action, waiter = l.waiter, on = l.on }) .. '\n')
    f:close()
  end
end)
-- Written down and left to LBTerm: the default program in the tab's folder
wezterm.on('lbterm-reopen-tab', function(window, pane, r)
  local f = io.open([[$reopens]], 'a')
  if f then
    f:write(encode({ label = r.label, dir = r.dir, session = r.session }) .. '\n')
    f:close()
  end
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  -- On, not following Windows (the default, 'system'): with motion off nothing glints, and
  -- Copy link's glint is one of what's tested (step 11)
  lbterm_motion = { enabled = 'on' },
  lbterm_tab_panel = { enabled = true, open_delay_ms = 0, labels = false },
  enable_tab_bar = false,
  initial_cols = 100,
  initial_rows = 32,
  window_close_confirmation = 'NeverPrompt',
  -- A killed test window would otherwise greet the next with the crash-restore prompt
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  lbterm_summaries = '$(ConvertTo-LuaPath $summaries)',
  lbterm_projects = { projects = { { name = 'Sys', dir = [[C:\Windows\System32]] } }, recent_file = '$(ConvertTo-LuaPath $recent)' },
  lbterm_links = { file = '$(ConvertTo-LuaPath $linksFile)' },
  lbterm_close_marks = '$(ConvertTo-LuaPath $marks)',
  -- Ctrl+Shift+T as in Lachlan's config, and F9 for it here: a posted key carries no modifier
  keys = {
    { key = 't', mods = 'CTRL|SHIFT', action = wezterm.action.LbtermNewTab 'Reopen' },
    { key = 'F9', action = wezterm.action.LbtermNewTab 'Reopen' },
    -- F8 for his Ctrl+W (CloseCurrentTab), and F7 to open a tab when step 18 needs more rows
    { key = 'F8', action = wezterm.action.CloseCurrentTab { confirm = false } },
    { key = 'F7', action = wezterm.action.SpawnCommandInNewTab { args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' } } },
  },
$domain}
"@)

# The events the handler wrote, in order
function Get-Events([string]$Path = $events) { if (Test-Path $Path) { @(Get-Content $Path | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json }) } else { @() } }
# The sessions with a close mark
function Get-Marked { @(Get-ChildItem $marks -Filter '*.closing' -ErrorAction SilentlyContinue | ForEach-Object { $_.BaseName }) }
# A stand-in Claude tab: its session's user var, then one OSC 7501 record of Claude Code's, then
# nothing more
function New-ReportingProgram([string]$Sid, [string]$State) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Sid))
  New-PwshProgram "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7 + [char]27 + ']7501;state=${State}:app=claude' + [char]27 + '\'); Start-Sleep 600"
}
# A folder as text to compare: backslashes, no slash at its end, any case
function Folder-Key([string]$dir) { $dir.Replace('/', '\').TrimEnd('\').ToLowerInvariant() }
# An open menu's rows as words, each with its key: "Rename tab (R), Colour, ..."
function Menu-Rows($m) { @($m.menu.items | ForEach-Object { if ($_.key) { "$($_.label) ($($_.key))" } else { $_.label } }) -join ', ' }
# Park tab's place in a row list, first in Wait on's group, on a build that parks (Lachlan's 8a;
# its rail rows say `parked`): every tab here can be parked, shells' too
function Park-Row($m) { if (@($m.rail.rows)[0].PSObject.Properties['parked']) { 'Park tab (P), ' } else { '' } }

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

  # Nothing of the rail plays under a menu's card (Lachlan's 7b of 2026-11-08): what a pick sets
  # going on the rail (a closed tab's row leaving, the rows below sliding up, the marker moving, a
  # status word's dissolve, a pad's glint) waits while the menu's card fades out and plays once it
  # has gone. Every frame read after the pick: none has the tab menu's fade (menu.tab) beside any
  # name the hold drives (the harness's Get-RailUnderCard, every one of the compositor's, PITFALLS
  # 210). The menu has settled before each pick, so menu.tab is its fade out alone (a fade in
  # turned round would be counted to its own end, STATE.md's animations)
  $under = New-UnderCardWatch -Cards 'menu.tab'
  $watchCard = { param($s) $under.Watch($s) }
  $heldUnder = { param([string]$What) $under.Judge($What) }

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

  # 4. Shell in charlie's folder, then close the copy from the keyboard
  $m = & $menu 2
  & $pick $m 'Shell in its folder'
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
    # Picked from the menu it never asks, a mux server's pane included (perf\last-tab.ps1
    # watches for the question)
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
  # Picked from the menu it never asks, a mux server's panes included
  $asked = [System.Collections.Generic.List[string]]::new()
  Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'the others closed' -OnState { param($s) if ($s.overlay) { $asked.Add($s.overlay) } } | Out-Null
  if ($asked.Count) { Add-Failure "Close other tabs asked first ($($asked[0]))" }
  if (Wait-Until { @(Get-Tabs $win1).Count -eq 1 }) { 'close others: one tab left in the first window, without asking' }
  else { Add-Failure "Close other tabs left $(@(Get-Tabs $win1).Count) tabs in the first window" }
  Wait-Settled -What 'the window after closing the others' | Out-Null

  # 9. The rows the facts give (lbterm/NEW-TAB.md, section 7), each with its key: moved, the tab
  # left, is a shell in C:\Windows, which no project holds
  $m = & $menu 0
  $got = Menu-Rows $m
  $want = "Rename tab (R), Colour, Shell in its folder (S), $(Park-Row $m)Move to new window (M), Close other tabs (O), Close tab (X)"
  "rows, a shell tab in no project: $got"
  if ($got -ne $want) { Add-Failure "a shell tab in no project has [$got], not [$want]" }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'Esc to close the menu' | Out-Null

  # 10. An unnamed Claude tab: its session's label Tab Wave and its statusline's folder System32,
  # which Sys holds, as Terminal's workers would write them. The facts are read as the menu opens,
  # so it opens once the row says the label, which needs the session's user var in the GUI
  $sid = 'c4c4c4c4-0000-4000-8000-0000000000a1'
  Write-SessionFiles -Summaries $summaries -Sid $sid -Label 'Tab Wave' -Cwd 'C:\Windows\System32'
  $keep = @(Get-Panes -Server:$Mux | Where-Object tab_title -eq 'moved')[0].pane_id
  New-Tab -From $keep -Cwd 'C:\Windows\System32' -Program (New-ClaudeSessionProgram $sid) -NoActivate -Server:$Mux | Out-Null
  $e = Wait-Change { param($s) @($s.rail.rows | Where-Object label -eq 'Tab Wave').Count -eq 1 } -What "the Claude tab's row saying its label" -TimeoutSec 60
  $claudeTab = @($e.rail.rows | Where-Object label -eq 'Tab Wave')[0].tab
  $claudeRow = { $s = Wait-Settled -What 'the rail'; @($s.rail.rows | Where-Object tab -eq $claudeTab)[0].row }
  $m = & $menu (& $claudeRow)
  $got = Menu-Rows $m
  $want = "Rename tab (R), Colour, Copy link (L), New session in Sys (N), Shell in its folder (S), $(Park-Row $m)Move to new window (M), Close other tabs (O), Close tab (X)"
  "rows, a Claude tab in Sys: $got"
  if ($got -ne $want) { Add-Failure "a Claude tab in a project has [$got], not [$want]" }
  # R: the name to type starts as the label under its pad, not its pane's title
  Send-Key $w R
  $r = Wait-Change { param($s) $null -ne $s.menu.renaming } -What 'the name to type in'
  if ($r.menu.renaming -ne 'Tab Wave') { Add-Failure "Rename on the Claude tab started from '$($r.menu.renaming)', not its label 'Tab Wave'" }
  else { 'rename: starts from the label, Tab Wave' }
  Send-Key $w Escape   # back to the menu
  Wait-Change { param($s) $s.menu -and $null -eq $s.menu.renaming } -What 'Esc back to the menu' | Out-Null
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'Esc to close the menu' | Out-Null

  # 11. L, Copy link: the text to the state file, never to the clipboard, and the pad glints
  $seen = [System.Collections.Generic.HashSet[string]]::new()
  $m = & $menu (& $claudeRow)
  Send-Key $w L
  $c = Wait-Change { param($s) -not $s.menu -and $s.copied } -What 'Copy link' -TimeoutSec 30 -OnState { param($s) foreach ($a in @($s.animations)) { [void]$seen.Add($a.name) }; & $watchCard $s }
  $want = "[Tab Wave](lbterm://session/$sid) in Sys"
  if ($c.copied -ne $want) { Add-Failure "Copy link wrote [$($c.copied)], not [$want]" } else { "copy link: $($c.copied)" }
  if (-not $seen.Contains('light.glint')) { Add-Failure "the Claude tab's pad didn't glint on Copy link: light.glint never showed, only [$(@($seen) -join ', ')]" } else { 'copy link: the pad glinted' }
  if (-not $seen.Contains('menu.tab.flash')) { Add-Failure "Copy link's row didn't flash as it was picked: menu.tab.flash never showed, only [$(@($seen) -join ', ')]" } else { 'copy link: its row flashed (menu.tab.flash)' }
  & $heldUnder 'copy link'

  # 12. N, New session in Sys: the menu goes, then it's a pick. No snapshot may show the menu with
  # the start's record in it: both happen in one turn of the GUI, so the frame after has neither
  # the menu nor no Starting row
  $before = @(Get-Tabs $win1 | ForEach-Object tab_id)
  $order = @{ menuWithStart = $false }
  $m = & $menu (& $claudeRow)
  Send-Key $w N
  $n = Wait-Change { param($s) -not $s.menu -and @($s.new_tab.starts | Where-Object { $_.label -eq 'Sys' -and $_.ended }).Count } -What "the new session's Starting row, ended" -TimeoutSec 60 -OnState {
    param($s) if ($s.menu -and @($s.new_tab.starts | Where-Object label -eq 'Sys').Count) { $order.menuWithStart = $true }
  }
  if ($order.menuWithStart) { Add-Failure 'a frame showed the tab menu with New session already started' }
  $start = @($n.new_tab.starts | Where-Object label -eq 'Sys')[-1]
  "new session: its Starting row $($start.ended)$(if ($null -ne $start.shown) { ", shown at frame $($start.shown)" } else { ', the tab came before the next frame' })"
  if ($start.ended -ne 'arrived') { Add-Failure "New session's Starting row $($start.ended), where its tab should have come" }
  if (-not (Wait-Until { (Get-Events).Count -ge 1 } -TimeoutSec 30)) { Add-Failure 'New session fired no lbterm-project-action' }
  else {
    $ev = (Get-Events)[-1]
    "new session: fired $($ev.action) for $($ev.name) from the $($ev.source), dir $($ev.dir), project_dir $($ev.project_dir)"
    if ($ev.action -ne 'new' -or $ev.source -ne 'tab menu' -or $ev.name -ne 'Sys' -or $ev.behind) { Add-Failure "New session fired $($ev | ConvertTo-Json -Compress)" }
    if ((Folder-Key $ev.dir) -ne (Folder-Key 'C:\Windows\System32') -or (Folder-Key $ev.project_dir) -ne (Folder-Key 'C:\Windows\System32')) { Add-Failure "New session's folders are $($ev.dir) and $($ev.project_dir), not System32's" }
  }
  # A recent pick, as one in the picker is (8b): its file written by a thread of its own
  if (-not (Wait-Until { (Test-Path $recent) -and ((Get-Content $recent -Raw) -match '"Sys"') } -TimeoutSec 15)) { Add-Failure "New session in Sys wasn't kept as a recent pick in $recent" }
  else { "new session: Sys kept as a recent pick ($((Get-Content $recent -Raw).Trim()))" }
  $new = $null
  if (Wait-Until { @(Get-Tabs $win1 | Where-Object { $_.tab_id -notin $before }).Count -eq 1 } -TimeoutSec 30) {
    $new = @(Get-Tabs $win1 | Where-Object { $_.tab_id -notin $before })[0]
    "new session: tab $($new.tab_id) in $($new.cwd)"
    if ($new.cwd -notmatch 'System32') { Add-Failure "New session's tab is in $($new.cwd), not System32" }
    $front = Wait-Change { param($s) @($s.tabs | Where-Object active)[0].id -eq $new.tab_id } -What "the new session's tab in front" -TimeoutSec 30
    if (-not $front) { Add-Failure "New session's tab isn't in front" }
  } else { Add-Failure "New session left $(@(Get-Tabs $win1).Count) tabs in the first window, not 3" }

  # 13. That tab, a shell no one named: R starts from its row's label, and X closes it
  if ($new) {
    $s = Wait-Change { param($s) @($s.rail.rows | Where-Object tab -eq $new.tab_id).Count -eq 1 } -What "the new tab's row"
    $label = @($s.rail.rows | Where-Object tab -eq $new.tab_id)[0].label
    $m = & $menu (@($s.rail.rows | Where-Object tab -eq $new.tab_id)[0].row)
    Send-Key $w R
    $r = Wait-Change { param($s) $null -ne $s.menu.renaming } -What 'the name to type in'
    if ($r.menu.renaming -ne $label) { Add-Failure "Rename on the new shell started from '$($r.menu.renaming)', not its row's label '$label'" }
    else { "rename: the shell starts from its row's label, $label" }
    Send-Key $w Escape
    Wait-Change { param($s) $s.menu -and $null -eq $s.menu.renaming } -What 'Esc back to the menu' | Out-Null
    $seen.Clear()
    Send-Key $w X
    # Close tab: the row leaves and the marker moves once the card has gone, not under it (7b).
    # Read from the key on: the frame after it is the one that would have started them
    Wait-Change { param($s) -not $s.menu -and @($s.rail.rows | Where-Object tab -eq $new.tab_id).Count -eq 0 } -What "the closed tab's row gone" -TimeoutSec 30 -OnState { param($s) foreach ($a in @($s.animations)) { [void]$seen.Add($a.name) }; & $watchCard $s } | Out-Null
    if (Wait-Until { @(Get-Tabs $win1 | Where-Object tab_id -eq $new.tab_id).Count -eq 0 } -TimeoutSec 30) { 'keys: X closed the new tab' }
    else { Add-Failure 'X left the new tab open' }
    if (-not $seen.Contains('rail.row.exit')) { Add-Failure "Close tab's row didn't fade out where it was: no rail.row.exit in [$(@($seen) -join ', ')]" }
    else { "keys: X's row faded out (rail.row.exit), played $(@($seen | Where-Object { $_ -like 'rail.*' }) -join ', ')" }
    & $heldUnder 'close tab'
  }

  # 14. Three more stand-in Claude tabs with their objectives all done: Fin One and Fin Two have
  # handed the turn back (Your turn), Unread has a result never shown (Done, its white dot). Each
  # opens behind, so none is seen and Unread's dot stays
  $done = @(New-Objective 1 first done 'Finish it' 'Done')
  $fin = [ordered]@{
    'Fin One' = @{ Sid = 'f1f1f1f1-0000-4000-8000-0000000000f1'; State = 'idle' }
    'Fin Two' = @{ Sid = 'f2f2f2f2-0000-4000-8000-0000000000f2'; State = 'idle' }
    'Unread'  = @{ Sid = 'eeeeeeee-0000-4000-8000-0000000000e1'; State = 'done' }
  }
  foreach ($label in $fin.Keys) {
    Write-SessionFiles -Summaries $summaries -Sid $fin[$label].Sid -Label $label -Cwd 'C:\Windows' -Objectives $done
    New-Tab -From $keep -Cwd 'C:\Windows' -Program (New-ReportingProgram $fin[$label].Sid $fin[$label].State) -NoActivate -Server:$Mux | Out-Null
  }
  $said = { param($s, [string]$label) @($s.rail.rows | Where-Object label -eq $label)[0] }
  $e = Wait-Change { param($s)
    $one = & $said $s 'Fin One'; $two = & $said $s 'Fin Two'; $un = & $said $s 'Unread'
    $one -and $two -and $un -and $one.status -eq 'Your turn' -and $two.status -eq 'Your turn' -and $un.status -eq 'Done' -and $un.unread
  } -What 'the three stand-ins saying Your turn, Your turn and Done, unread' -TimeoutSec 60
  $finTabs = @{}; foreach ($label in $fin.Keys) { $finTabs[$label] = (& $said $e $label).tab }

  # Fin One the open tab, so Close finished tabs closes the tab in front (step 15)
  $x, $y = Get-RectCenter (& $said (Wait-Settled -What 'the rail') 'Fin One').pad
  Send-Click $w $x $y -Move
  Wait-Change { param($s) @($s.tabs | Where-Object active)[0].id -eq $finTabs['Fin One'] } -What 'Fin One in front' | Out-Null
  Send-Mouse $w Move $paneSpot[0] $paneSpot[1]

  $m = & $menu (& $claudeRow)
  $got = Menu-Rows $m
  $want = 'Rename tab (R), Colour, Copy link (L), New session in Sys (N), Shell in its folder (S), ' + (Park-Row $m) + 'Wait on' + [char]0x2026 + ' (W), Move to new window (M), Close finished tabs (2) (F), Close other tabs (O), Close tab (X)'
  "rows, a Claude tab beside three others, two finished: $got"
  if ($got -ne $want) { Add-Failure "Tab Wave's menu beside the stand-ins has [$got], not [$want]" }
  $second = @($m.menu.items | Where-Object key -eq 'F')[0].detail
  if ($second -ne 'Fin One, Fin Two') { Add-Failure "Close finished tabs' second line says [$second], not [Fin One, Fin Two]" }
  else { "close finished: its second line says $second" }

  # W: the page of the other Claude tabs, its rows dissolving in; Left goes back
  $seen = [System.Collections.Generic.HashSet[string]]::new()
  # In each snapshot that has the marker moving beside rows sliding, how much longer the marker has
  # left than the slowest row: riding with a row on its spring, it ends as the row does (3b); a
  # glide on focus_move (300 ms against the rows' 220) ends after them
  $ride = [System.Collections.Generic.List[double]]::new()
  $watch = {
    param($s)
    foreach ($a in @($s.animations)) { [void]$seen.Add($a.name) }
    $marker = @($s.animations | Where-Object name -eq 'rail.marker')
    $slides = @($s.animations | Where-Object name -eq 'rail.row.slide')
    if ($marker.Count -and $slides.Count) {
      $ride.Add(($marker | Measure-Object left_ms -Maximum).Maximum - ($slides | Measure-Object left_ms -Maximum).Maximum)
    }
  }
  # The marker rode up with the row that opened in the closed one's slot: it played, and in every
  # snapshot it showed with the rows sliding it ended with them. Whether it rides or glides turns
  # on the plan render/tab_panel.rs makes of this close (the closed tab's row gone from the rows as
  # the marker moves), so it's judged here for each real close; compositor/imp/tests/rail.rs holds
  # the compositor's half, for one plan built by hand (PITFALLS 305)
  $rodeUp = {
    param([string]$What)
    if (-not $seen.Contains('rail.marker')) { Add-Failure "${What}: the marker didn't move with the row opening in its slot, no rail.marker in [$(@($seen) -join ', ')]"; return }
    if (-not $ride.Count) { Add-Failure "${What}: no snapshot had the marker moving beside the rows' slide"; return }
    $late = ($ride | Measure-Object -Maximum).Maximum
    if ($late -gt 5) { Add-Failure "${What}: the marker had up to $late ms more to go than the rows' slide, so it glided rather than riding with its row ($($ride.Count) snapshots)" }
    else { "${What}: the marker rode up with the row opening in its slot (rail.marker ending within $late ms of its slide, $($ride.Count) snapshots)" }
  }
  Send-Key $w W
  $page = Wait-Change { param($s) $s.menu -and @($s.menu.items)[0].label -eq ([char]0x2039 + ' Back') } -What "Wait on…'s page" -OnState $watch
  $got = @($page.menu.items | ForEach-Object label) -join ', '
  $want = ([char]0x2039 + ' Back') + ', Fin One, Fin Two, Unread'
  if ($got -ne $want) { Add-Failure "Wait on…'s page has [$got], not [$want]" } else { "wait on: its page has $got" }
  if (-not $seen.Contains('menu.tab.swap')) { Add-Failure "the card's rows didn't dissolve into Wait on…'s page: menu.tab.swap never showed, only [$(@($seen) -join ', ')]" }
  else { "wait on: the rows dissolved (menu.tab.swap)" }
  Send-Key $w Left
  Wait-Change { param($s) $s.menu -and @($s.menu.items)[0].label -eq 'Rename tab' } -What 'Left back to the menu' | Out-Null
  'wait on: Left went back'
  Send-Key $w W
  $page = Wait-Change { param($s) $s.menu -and @($s.menu.items)[0].label -eq ([char]0x2039 + ' Back') } -What "Wait on…'s page again"
  & $pick $page 'Unread'
  Wait-Change { param($s) -not $s.menu } -What 'the menu gone after picking Unread' | Out-Null
  if (-not (Wait-Until { @(Get-Events $linkEvents).Count -ge 1 } -TimeoutSec 30)) { Add-Failure "picking Unread on Wait on…'s page fired no lbterm-tab-link" }
  else {
    $ev = @(Get-Events $linkEvents)[-1]
    "wait on: fired $($ev.action), $($ev.waiter) on $($ev.on)"
    if ($ev.action -ne 'toggle' -or $ev.waiter -ne $sid -or $ev.on -ne $fin['Unread'].Sid) { Add-Failure "Wait on… fired $($ev | ConvertTo-Json -Compress), not toggle from $sid on $($fin['Unread'].Sid)" }
  }

  # 15. Fin Two waits on Tab Wave through a tab link (the links file says so, as links.ts would
  # write it): it has handed the turn back, but links.ts means to tell it to carry on, so it isn't
  # finished (PITFALLS 153). Close finished tabs then names Fin One alone, and F closes that one,
  # the open tab, at once: Fin Two, Unread and Tab Wave stay
  $link = [ordered]@{ id = 'fin-two-on-wave'; waiter = $fin['Fin Two'].Sid; on = $sid; at = 1; by = 'test'; what = 'Tab Wave to finish'; state = 'waiting'; stateAt = 1; note = '' }
  $json = [ordered]@{ updatedAt = 1; links = @($link); waits = @() } | ConvertTo-Json -Depth 5 -Compress
  Write-FileWhole -Path $linksFile -Text $json
  Wait-Change { param($s) (& $said $s 'Fin Two').link -eq 'waiting' } -What 'Fin Two waiting on Tab Wave' -TimeoutSec 30 | Out-Null
  $before = @(Get-Tabs $win1 | ForEach-Object tab_id)
  $wasY = (& $said (Wait-Settled -What 'the rail') 'Fin One').pad.y
  $seen.Clear()
  $ride.Clear()
  $m = & $menu (& $claudeRow)
  $finRow = @($m.menu.items | Where-Object key -eq 'F')[0]
  if ($finRow.label -ne 'Close finished tabs (1)' -or $finRow.detail -ne 'Fin One') { Add-Failure "with Fin Two waiting on Tab Wave, Close finished tabs reads [$($finRow.label)] [$($finRow.detail)], not [Close finished tabs (1)] [Fin One]" }
  else { "close finished: Fin Two waits on Tab Wave, so it names $($finRow.detail) alone" }
  Send-Key $w F
  $both = { param($s) & $watch $s; & $watchCard $s }
  $closed = Wait-Change { param($s) -not $s.menu -and @($s.tabs | Where-Object id -eq $finTabs['Fin One']).Count -eq 0 } -What 'Fin One gone' -TimeoutSec 30 -OnState $both
  Wait-Settled -What 'the rail after closing the finished tab' -OnState $both | Out-Null
  $left = @(Get-Tabs $win1 | ForEach-Object tab_id)
  "close finished: $($before.Count) tabs before, $($left.Count) after"
  if ($left.Count -ne $before.Count - 1) { Add-Failure "Close finished tabs left $($left.Count) tabs of $($before.Count), not one fewer" }
  foreach ($stay in @($finTabs['Fin Two'], $finTabs['Unread'], $claudeTab)) { if ($stay -notin $left) { Add-Failure "Close finished tabs closed tab $stay, which wasn't finished" } }
  foreach ($name in 'rail.row.exit', 'rail.row.slide', 'pane.dissolve') {
    if (-not $seen.Contains($name)) { Add-Failure "Close finished tabs played no ${name}: [$(@($seen) -join ', ')]" }
  }
  "close finished: played $(@($seen | Where-Object { $_ -like 'rail.*' -or $_ -like 'pane.*' }) -join ', ')"
  if (-not $seen.Contains('menu.tab.flash')) { Add-Failure "Close finished tabs' row didn't flash as it was picked: no menu.tab.flash in [$(@($seen) -join ', ')]" }
  # The open tab now is the one that slid into Fin One's place (the mux keeps the index), Fin Two,
  # its row ending where Fin One's was: the marker rides up with it on its spring rather than
  # waiting in the slot while it slides up under it (lbterm/FLOWS.md, "The tab menu's facts, and a
  # pick from it", step 10; Lachlan's 3b). Step 18 has it glide, the open row not moving
  $open = @((Wait-Settled -What 'the rail').rail.rows | Where-Object active)[0]
  if ($open.tab -ne $finTabs['Fin Two']) { Add-Failure "after Fin One closed the open tab is $($open.tab), not Fin Two ($($finTabs['Fin Two']))" }
  elseif ($open.pad.y -ne $wasY) { Add-Failure "Fin Two's row ended at $($open.pad.y), not Fin One's $wasY" }
  else { "close finished: Fin Two, open now, took Fin One's row" }
  & $rodeUp 'close finished'
  & $heldUnder 'close finished'
  $marked = Get-Marked
  if ($fin['Fin One'].Sid -notin $marked) { Add-Failure "Fin One's session has no close mark: [$($marked -join ', ')]" }
  foreach ($label in 'Fin Two', 'Unread') { if ($fin[$label].Sid -in $marked) { Add-Failure "$label's session has a close mark, though it stayed" } }
  $kept = @($closed.new_tab.closed | ForEach-Object label)
  if ('Fin One' -notin $kept -or 'Fin Two' -in $kept) { Add-Failure "the closed tabs kept to reopen are [$($kept -join ', ')], not Fin One without Fin Two" }
  else { "close finished: marked and kept to reopen: $($kept -join ', ')" }

  # 16. A middle click: on the + pad nothing, on Unread's row that tab, its close mark written; F9
  # (Reopen, Ctrl+Shift+T's stand-in) opens it again
  $s = Wait-Settled -What 'the rail'
  $count = @($s.tabs).Count
  if ($s.new_tab) {
    $px2, $py2 = Get-RectCenter $s.new_tab.rect
    Send-Mouse $w Move $px2 $py2
    Send-Mouse $w MDown $px2 $py2 -Keys M
    Send-Mouse $w MUp $px2 $py2
    # Posted messages are taken in order: once a menu opened after the click shows, the click has
    # been answered (a posted move doesn't light the pad: it hovers only with LBTERM_POSTED_POINTER)
    & $menu (& $claudeRow) | Out-Null
    Send-Key $w Escape
    $after = Wait-Change { param($s) -not $s.menu } -What 'the menu opened after the middle click on the + pad, closed'
    if (@($after.tabs).Count -ne $count) { Add-Failure "a middle click on the + pad left $(@($after.tabs).Count) tabs of $count" }
    else { 'middle click: the + pad closed nothing' }
  } else { Add-Failure 'the state file has no new_tab: no + pad to click' }
  $x, $y = Get-RectCenter (& $said (Wait-Settled -What 'the rail') 'Unread').pad
  Send-Mouse $w Move $x $y
  Send-Mouse $w MDown $x $y -Keys M
  Send-Mouse $w MUp $x $y
  $s = Wait-Change { param($s) @($s.tabs | Where-Object id -eq $finTabs['Unread']).Count -eq 0 } -What 'Unread closed by a middle click' -TimeoutSec 30
  if ($fin['Unread'].Sid -notin (Get-Marked)) { Add-Failure "a middle click closed Unread without its close mark" } else { 'middle click: Unread closed, its close mark written' }
  if (@($s.new_tab.closed)[0].label -ne 'Unread') { Add-Failure "the newest tab kept to reopen is [$(@($s.new_tab.closed)[0].label)], not Unread" }
  $tabsBefore = @(Get-Tabs $win1 | ForEach-Object tab_id)
  Send-Key $w F9
  if (-not (Wait-Until { @(Get-Events $reopens).Count -ge 1 } -TimeoutSec 30)) { Add-Failure 'F9 (Reopen) fired no lbterm-reopen-tab' }
  else {
    $ev = @(Get-Events $reopens)[-1]
    "reopen: $($ev.label), session $($ev.session), in $($ev.dir)"
    if ($ev.label -ne 'Unread' -or $ev.session -ne $fin['Unread'].Sid) { Add-Failure "Reopen fired $($ev | ConvertTo-Json -Compress), not Unread's" }
  }
  if (Wait-Until { @(Get-Tabs $win1 | Where-Object { $_.tab_id -notin $tabsBefore }).Count -eq 1 } -TimeoutSec 30) { 'reopen: a tab opened in its place' }
  else { Add-Failure 'Reopen opened no tab' }

  # 17. A middle click on the open tab's row with a row below it: that row slides up into the slot
  # and opens, and the marker rides up with it (3b)
  $s = Wait-Settled -What 'the rail'
  $rows = @($s.rail.rows | Where-Object { -not $_.pending })
  if ($rows.Count -lt 3) { Add-Failure "only $($rows.Count) rows left for the middle click on the open row" }
  else {
    $mid = $rows[-2]
    $x, $y = Get-RectCenter $mid.pad
    Send-Click $w $x $y -Move
    Wait-Change { param($s) @($s.rail.rows | Where-Object active)[0].tab -eq $mid.tab } -What 'the row above the last open' | Out-Null
    $count = @((Wait-Settled -What 'the rail').tabs).Count
    $seen.Clear()
    $ride.Clear()
    Send-Mouse $w MDown $x $y -Keys M
    Send-Mouse $w MUp $x $y
    Wait-Change { param($s) @($s.tabs).Count -eq $count - 1 } -What 'the open tab closed by a middle click' -TimeoutSec 30 -OnState $watch | Out-Null
    $s = Wait-Settled -What 'the rail after the middle click' -OnState $watch
    $open = @($s.rail.rows | Where-Object active)[0]
    if ($open.tab -ne $rows[-1].tab -or $open.pad.y -ne $mid.pad.y) { Add-Failure "after the open row closed, the open row is tab $($open.tab) at $($open.pad.y), not tab $($rows[-1].tab) in its slot at $($mid.pad.y)" }
    & $rodeUp 'middle click'
  }

  # 18. Ctrl+W (F8 bound to CloseCurrentTab, a posted key carrying no Ctrl) on the open tab's row
  # with a row below it: that row slides up into the slot and opens, the marker riding up with it
  # (3b names Ctrl+W, the middle click and Close finished tabs). F7 opens tabs while there are too
  # few rows for this step and the next
  $s = Wait-Settled -What 'the rail, for F8'
  $rows = @($s.rail.rows | Where-Object { -not $_.pending })
  while ($rows.Count -lt 3) {
    $n = $rows.Count
    Send-Key $w F7
    Wait-Change { param($s) @($s.rail.rows | Where-Object { -not $_.pending }).Count -eq $n + 1 } -What 'a tab F7 opened, for F8' -TimeoutSec 30 | Out-Null
    $s = Wait-Settled -What 'the rail with the tab F7 opened'
    $rows = @($s.rail.rows | Where-Object { -not $_.pending })
  }
  $mid = $rows[-2]
  $x, $y = Get-RectCenter $mid.pad
  Send-Click $w $x $y -Move
  Wait-Change { param($s) @($s.rail.rows | Where-Object active)[0].tab -eq $mid.tab } -What 'the row above the last open, for F8' | Out-Null
  $count = @((Wait-Settled -What 'the rail').tabs).Count
  $seen.Clear()
  $ride.Clear()
  Send-Key $w F8
  Wait-Change { param($s) @($s.tabs).Count -eq $count - 1 } -What 'the open tab closed by F8 (CloseCurrentTab)' -TimeoutSec 30 -OnState $watch | Out-Null
  $s = Wait-Settled -What 'the rail after F8' -OnState $watch
  $open = @($s.rail.rows | Where-Object active)[0]
  if ($open.tab -ne $rows[-1].tab -or $open.pad.y -ne $mid.pad.y) { Add-Failure "after F8 closed the open row, the open row is tab $($open.tab) at $($open.pad.y), not tab $($rows[-1].tab) in its slot at $($mid.pad.y)" }
  & $rodeUp 'F8 (CloseCurrentTab, his Ctrl+W)'

  # 19. A middle click on the open tab's row, the last row: the tab above it opens, a row up, and
  # the marker glides there (rail.marker, focus_move) as the row fades where it was
  $s = Wait-Settled -What 'the rail'
  $rows = @($s.rail.rows | Where-Object { -not $_.pending })
  $last = $rows[-1]
  $x, $y = Get-RectCenter $last.pad
  Send-Click $w $x $y -Move
  Wait-Change { param($s) @($s.rail.rows | Where-Object active)[0].tab -eq $last.tab } -What 'the last row open' | Out-Null
  $count = @((Wait-Settled -What 'the rail').tabs).Count
  $seen.Clear()
  Send-Mouse $w MDown $x $y -Keys M
  Send-Mouse $w MUp $x $y
  Wait-Change { param($s) @($s.tabs).Count -eq $count - 1 } -What 'the open tab closed by a middle click' -TimeoutSec 30 -OnState $watch | Out-Null
  $s = Wait-Settled -What 'the rail after the middle click' -OnState $watch
  $open = @($s.rail.rows | Where-Object active)[0]
  if ($open.pad.y -ge $last.pad.y) { Add-Failure "after the open last row closed the open row's pad is at $($open.pad.y), not above $($last.pad.y)" }
  if (-not $seen.Contains('rail.marker')) { Add-Failure "the open tab's middle click left the marker to land at once: no rail.marker in [$(@($seen) -join ', ')]" }
  else { "middle click: the open tab closed, the marker glided a row up to the next (rail.marker)" }

  Wait-Settled -What 'the window at the end' | Out-Null
  (Get-Shot $w -Name 'end').Dispose()
} -Keep:$Keep -Passed "tab-mouse$(if ($Mux) { ' (mux)' }): menu, colour, rename, shell in its folder, keys, drag, tear-off, move, close others, the rows a tab's facts give, Rename from the label, Copy link, New session, Wait on…, Close finished tabs, a middle click's close and its reopen, and the marker riding on Ctrl+W all work"
