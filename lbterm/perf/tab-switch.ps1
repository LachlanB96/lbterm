#Requires -Version 7
# Covers: lbterm-motion/src/plan.rs, lbterm-motion/src/flight.rs, lbterm-motion/src/outline.rs, lbterm-motion/src/motion.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/strip_swap.rs, wezterm-gui/src/termwindow/render/conductor_view.rs, wezterm-gui/src/termwindow/state.rs, lbterm-extras/src/state.rs, config/src/lbterm.rs, wezterm-gui/src/termwindow/frame_wait.rs, lbterm-extras/src/frame_wait.rs
# Variant: -Cli
# Variant: -Mux
# Variant: -Mux -ServerBin installed
# Variant: -Mux -GuiBin installed
<#
A tab switch's motion (lbterm/DESIGN.md, `tab_switch`) end to end, in a test window that looks
like Lachlan's LBTerm (New-TestConfig) with the tab panel on and motion at -Slowdown. Three tabs
each paint blocks of their own: tab one's on the left of the pane, tab two's on the right, tab
three's across the middle, dimmer. Tabs are switched as Lachlan switches them, by the mouse's
side buttons (posted WM_XBUTTONDOWN, through his lbterm.lua's hop, which skips parked tabs) and,
in step 6b, a click on a pad on the rail (on his PC Ctrl+Tab switches windows), so a switch lands
within a frame of when the script asks for it. A posted button takes Ctrl and Shift from the
message, where a posted key takes them from the keyboard as it is, so a key posted while
Lachlan holds Shift in a game is Shift+F8 and does nothing. F9 and F8 are bound to the next and
the previous tab for the Conductor's view (step 8), which is left by a key. With -Cli the plain
switches go through `lbterm cli activate-tab` instead, as the Conductor's and Relay's do.

1. One to two: the state file's first frame on tab two has its pane with the keyboard and
   `pane.dissolve` in flight; the frames on the way are a true cross-fade (on the left, where one
   is bright, and on the right, where two is, each shows the same share of tab one), that share
   follows `tab_switch` (fitted, printed), the strip's words dissolve in the same frames
   (switch.png, the middle frame), and once it's over no pane visual is left fading
   (`fading_panes`).
2. Back mid-fade: one to two again, and back to one before one's fade is a quarter through, while
   one's program moves a block along its last row. One comes back with the block where it is now
   (its dissolving visual held the old place), and the switch back starts from what showed:
   tab one's share never jumps more than 0.1 above where it had got to (back.png).
3. Seen again after its fade: one to two, settled, back to one: the same as before it was left.
   One to two with nothing captured (a capture paints the window itself): the compositor's own
   wake puts tab one's faded pane away with no frame, as Lachlan's windows need with no state
   probe painting, and its log says so before the state probe's frame, 20 ms after the fade's end.
4. Rapid: the forward button five times 40 ms apart, round all three tabs: it ends settled on the tab five on,
   nothing fading after, no frame on the way darker than the darkest tab (rapid.png), and the
   pool of pane surfaces back to one spare (`spare_panes`) once nothing fades.
5. Motion off, in a second window with the same tabs: a switch is there in the next frame, with
   no frames between, and lands on exactly what the first window settled on. A fourth tab there,
   split in two with the keyboard in its right pane, left for tab three by the back button: the
   right pane's outline is gone in the next frame, with no frame between.
6. Two panes: a fourth tab split in two, one pane with the keyboard and one without, switched
   from to tab three by the mouse's back button, as Lachlan hops tabs, tab three's block under
   both: each pane shows the same share of tab three in every frame on the way, to 0.12
   (two-panes.png). The pane without the keyboard, dimmed by its opacity, let 15% of tab three
   through at once until 2026-10-30. The active outline (Lachlan's 2a, 2026-10-30): along the
   right pane's left side, which tab three's outline doesn't draw, it shows tab four's panes'
   share in every frame on the way, to 0.12, and follows tab_switch from where they start
   (fitted, printed; outline.png), where until then it went at once; tab three's own left side is
   lit at once, 85% or more in every frame; the top, which both outlines draw, is never brighter
   than at rest by more than 1.5 levels, so no line is lit twice, nor where four's right pane's
   top-left curve runs into it (the corner check: the line's rows there, against the brighter of
   the two at rest, never 1.5 levels over). Measured on strips a few pixels about each line, each
   against a gutter of card beside it in the same shot, the window background and the cards
   being one colour.
6b. Turned round: back to four (the forward button), from it to three again (back) and, 15% into
   that fade, back to four by a click on its pad on the rail. Four's outline turns round with its
   panes, in step with them to 0.12 in every frame before and after the click (turned.png);
   three's goes from all of it on tab_switch (fitted from the click on); the top both draw stays
   within 1.5 levels of its rest throughout, the corner check too; four's panes end as they were.
6c. Closed: a fifth tab of one pane, its block where three's is, closed by `lbterm cli kill-pane`
   (as a session ending closes its tab), and the window back on tab four: the first frame after
   has `pane.dissolve` and `outline.dissolve`; five's outline along the room's left side, which
   four's doesn't draw, shows five's panes' share to 0.12 in every frame on the way and follows
   tab_switch from where they start (close.png); four's right pane's left side is lit at once,
   85% or more in every frame; the top both draw is never lit twice, the corner check too.
6d. Back past a newer fade: a sixth tab split in two with the keyboard in its left pane; four to
   three by the back button, on to six by a click on its pad and back to four by a click on its
   pad, six's 4% into the fade before and four's as soon as six shows, so three tabs are in
   flight. Three's leftover runs along four's right pane's top, which six's outline doesn't draw:
   four's outline comes back up there from nothing while three's goes on from what it showed, so
   the line rises from where it was, never lit twice, the corner check too (past.png). Where it
   was is what three's fade had got to at the frame the state file shows four in again, and
   four's own side what its fade had: each falls to that and rises from it, so no frame shows
   either more than 0.15 under it, however far the captures trail the state file. Until
   2026-10-31 the click was judged against the last frame captured before it, which could show
   three's fade not yet begun.
6e. Turned round twice: four to three by the back button, back to four by a click on its pad 15%
   into that fade, and back to three by a click on its pad as soon as four shows again, then in a
   second run 150 ms after. The top both draw stays within 1.5 levels of its rest, either way, in
   every frame, the corner check too, and it ends on three's outline. Until 2026-10-31 the second
   click dimmed that line by 36 to 44% (the review's probe).
7. Three in a row: from tab one (its title long, so a stretch of the strip shows its words alone)
   to two, and on to three a few percent into that fade: tab one's share of the strip goes on
   fading with its panes' share, not gone at the second key (in-a-row.png). With one visual for
   the strip's swap it was, until 2026-10-30.
   No switch here waits for a push (the counters' frame_wait, read after step 8): a tab none of
   whose panes a mux server holds draws its first frame at once (lbterm/MUX.md, "Tabs out of
   view: output held at the server"; perf/mux-hidden.ps1 has the switches that wait).
7b. A held tab (Lachlan's 2b of 2026-11-08): from tab two to tab one by F8, tab one's block
   moving out of view. On a mux server that holds it (its PaneHeld came first), only the panes
   wait for its push: the switch's first frame has the rail's ring and bar gliding (`rail.marker`)
   and the strip's words dissolving into tab one's (`strip.swap`), the rail's open row tab one's,
   while its panes are tab two's (the state's `panes_tab`) with nothing dissolving, until the frame
   that draws tab one's panes, where tab two's dissolve (`pane.dissolve`) and the rail's glide is
   further on, not started again; the wait ends by the push. Locally and on a server of another
   build, which hold nothing, the switch's first frame draws tab one's panes and nothing waits.
8. The Conductor's view, in a window of its own whose config names a seat (none answers: the view
   says so): open over tab one, then left for tab two by a key, and open again over two, left for
   tab one by a click on its row. Nothing of the tab under the view showed, so it goes at once:
   the first frame on the new tab has no `pane.dissolve`, and the tab that was under the view
   never shows in any frame after (view-left.png). Until 2026-10-30 it showed whole for a frame
   and faded out.
9. Tabs in flight (Lachlan's 3a), in a window of their own: five tabs, each a block in a band of
   the pane no other tab draws and a title of its own, tab one split in two with the keyboard in
   its right pane; from one to five by the forward button 40 ms apart, as fast as a hand clicks,
   and back to one by a click on its pad on the rail 40 ms after that, so four tabs are in flight
   and the one switched back to is the oldest, on top of them all. The presses go on a clock of
   their own (Start-TimedPosts), which the captures beside them can't spread: posted once a
   capture they went 45 to 120 ms apart on a busy PC. Frame by frame, each tab's share of the
   panes (its band) and of the strip's words (the five tabs' strips mixed by least squares,
   `[LbtermE2E.Px]::Mix`) never rises while it's in flight, from the state file's frame that left
   it and as long again as the captures trail the state file in this run (Measure-CaptureLag,
   on tab one's own fade before the click: 45 ms and more on a busy PC, where a fixed 34 ms failed
   a run), and never steps further between two frames than the curve can move in the time
   between them; tab one, switched back to, never falls under 75% of what its own fade had got
   to at the switch, and rises after it; each tab's words keep its panes' share to 0.15;
   tab one's outline where only it draws keeps its panes' share to 0.12, falling and coming back;
   the room's outline, the other tabs', never jumps (steps no further between two frames than
   the curve can move in the time between them); no line is lit twice, the corner check too
   (flight-back.png, the frame just after the click). Until 2026-10-31 each layer's opacity went
   on tab_switch itself: the second tab rose after it was left, as the first faded off it, and at
   the click the second jumped up and the first all but vanished before coming back.
   Since feat/dcomp-recorder the panes' own share is judged here no more (the captures still read
   it, for the words' and the outline's checks and the printed traces): never rising in flight,
   not stepping, and tab one coming back from what it showed and rising are the compositor's given
   the panes each switch left, which compositor/imp/tests/flight.rs holds every 2 ms from what DWM
   was handed, with the strips' pictures beside them. The words (the strip's swap), the outlines,
   the corner and the run itself (four tabs in flight, the switches posted on their clock) stay.

-Measure instead switches back and forth -Switches times, 250 ms apart, each waited on until it
has settled, and prints how long each took from the key to the state file's frame on the new tab
(median and worst), the GUI's CPU time a switch over what it costs at rest for as long (measured
first), the frames each switch painted, and the GUI's dedicated GPU memory at rest and while a
tab left fades; then a burst of 24 presses of the forward button 33 ms apart, faster than a
hand makes them, and the GPU memory and the spare pane surfaces at rest after it. -Split splits tab two in two first, the
keyboard in its right pane, so every switch leaves some of the outline fading (the right pane's
left side one way, the whole tab's left half the other). -Slow adds what held the GUI thread over
1 ms, by site (LBTERM_SLOW_EVERY, lbterm_extras::slow). It checks nothing, so it runs against an
older build too (-Bin): run it interleaved with one, before and after. A locked workstation stops
DWM, so measure unlocked.

-Mux puts each window's tabs on a test mux server of its own (a socket under the test's folder,
never Lachlan's), as Lachlan's are, and judges every step the same; -ServerBin runs another build's
lbterm-mux-server.exe for it and -GuiBin another build's lbterm-gui.exe for the window (installed:
a copy of the daily LBTerm's in the output folder, as perf/mux-resize.ps1 makes it), so a switch
works with either side older (PITFALLS 159). On a server of this build a tab out of view that
prints (one's moving block in step 2, once it's left) has its lines held there (lbterm/MUX.md,
"Tabs out of view: output held at the server"), so a switch back to it waits for its push: the
frame waits after step 8 must each end with the push, none with the bound.

Captures go to target\perf\tab-switch. Fails with exit 1.

  pwsh -File lbterm\perf\tab-switch.ps1 [-Slowdown 5]
  pwsh -File lbterm\perf\tab-switch.ps1 -Measure [-Switches 40] [-Split] [-Slow] [-Bin <build>]
  pwsh -File lbterm\perf\tab-switch.ps1 -Log lbterm_gui::termwindow::render::strip_swap=debug  # more of the GUI's log (WEZTERM_LOG)
  pwsh -File lbterm\perf\tab-switch.ps1 -Flight [-Bin <build>]  # step 9 alone
#>
param([double]$Slowdown = 5, [switch]$Measure, [int]$Switches = 40, [switch]$Split, [switch]$Slow, [switch]$Cli, [switch]$Flight, [string]$Bin, [string]$Log,
  [switch]$Mux, [string]$ServerBin, [string]$GuiBin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'tab-switch' -Bin $Bin
if (($ServerBin -or $GuiBin) -and -not $Mux) { throw '-ServerBin and -GuiBin are for -Mux' }
# The installed build's $Files, copied into the test's folder: a server or window running from the
# install folder would be taken for the daily one (the Warden's registry and `lbterm.ps1 logs` go
# by the exe's folder)
function Copy-Installed([string]$Name, [string[]]$Files) {
  $dir = Join-Path $T.Out "installed-$Name"
  New-Item -ItemType Directory -Force $dir | Out-Null
  foreach ($f in $Files) { Copy-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $dir }
  $dir
}
if ($ServerBin -eq 'installed') { $ServerBin = Copy-Installed 'server' 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe' }
if ($GuiBin -eq 'installed') { $GuiBin = Copy-Installed 'gui' 'lbterm-gui.exe', 'conpty.dll', 'OpenConsole.exe', 'libEGL.dll', 'libGLESv2.dll', 'mesa' }
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
$guiExe = if ($GuiBin) { Join-Path (Resolve-Path $GuiBin).Path 'lbterm-gui.exe' } else { $T.Gui }
if ($Mux -and -not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
if (-not (Test-Path $guiExe)) { throw "no lbterm-gui.exe in $GuiBin" }
# -Mux: each window Open-Tabs opens on a test mux server of its own (a socket of its own, never
# Lachlan's), started by the window as Lachlan's starts his, with a config naming its domain
$script:MuxWindows = 0
function Get-MuxOpen {
  if (-not $script:Mux) { return @{ Lua = ''; Open = @{} } }
  $script:MuxWindows++
  $sock = "$($T.Sock)$($script:MuxWindows)"
  $cfg = Join-Path $T.Out "config\tab-switch-server-$($script:MuxWindows).lua"
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  $domain = @"
config.unix_domains = { {
  name = 'tsmux',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  New-TestConfig -Path $cfg -Lua $domain
  @{ Lua = "`n$domain"; Open = @{ Domain = 'tsmux'; Socket = $sock } }
}
# -Mux: every cli call goes to the window's server, through its socket (the harness's -Server),
# and names the server's panes. The window's own socket can't spawn beside a pane of a mux domain
# ("pane 0 isn't one of this server's panes"), and the server's focus and tab titles reach the
# window (PaneFocused, TabTitleChanged). Get-Panes isn't an advanced function, so it's given
# -Server where it's called (Use-ServerTabs)
if ($Mux) {
  $PSDefaultParameterValues['Invoke-Cli:Server'] = $true
  $PSDefaultParameterValues['New-Tab:Server'] = $true
}

# A window's first tab and its program on a mux domain: the server a connecting window starts
# opens its first tab with its own default program, never the one the window's `connect` names
# (Open-TestWindow's -Program), so $Program is spawned on the server beside it, made the window's,
# and the server's first tab killed, as budgets.ps1's Open-BudgetWindow does (PITFALLS 202). The
# program's pane id, the server's. Without -Mux, the window's first pane, which runs $Program
# $script:Mux, never $Mux: PowerShell's names are case-insensitive, and a caller's own $mux (Open-Tabs'
# Get-MuxOpen) is found first
function Use-ServerTabs([object[]]$Program) {
  if (-not $script:Mux) { return (Get-Panes)[0].pane_id }
  if (-not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 20)) { throw "the window's mux server didn't answer" }
  $first = (Get-Panes -Server)[0].pane_id
  $own = New-Tab -From $first -Program $Program
  # Both in the window before the first goes: a window left without a tab closes
  Wait-State { param($s) @($s.tabs).Count -eq 2 } -What "the program's tab beside the server's first" -TimeoutSec 15 | Out-Null
  Invoke-Cli activate-pane --pane-id $own -Check | Out-Null
  Invoke-Cli kill-pane --pane-id $first -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 1 -and @($s.panes)[0].server_id -eq $own } -What "the program's own tab alone" -TimeoutSec 15 | Out-Null
  $own
}

# A pane of the state file as the cli names it: on a mux domain the server's id (`server_id`),
# which every cli call here takes, else the window's own (`id`); and the state's pane with the
# keyboard, the same way
# How many of a PDU the window's metrics say it sent or received ('sent', 'received')
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}
function Get-CliId($Pane) { if ($script:Mux) { $Pane.server_id } else { $Pane.id } }
function Get-ActiveCliId($State) {
  $p = @($State.panes | Where-Object { $_.id -eq $State.active_pane })[0]
  if ($p) { Get-CliId $p } else { $null }
}

$commands = Join-Path $T.Out 'commands'
New-Item -ItemType Directory -Force $commands | Out-Null

# A tab or a split the window then shows, as a local window shows what the cli spawns or splits:
# on a mux domain the server's new tab or pane isn't activated in the window, so it's activated
# through the server (its focus reaches the window as PaneFocused). The new pane's id
function Add-ShownTab([int]$From, [object[]]$Program) {
  $id = New-Tab -From $From -Program $Program
  if ($script:Mux) { Invoke-Cli activate-pane --pane-id $id -Check | Out-Null }
  $id
}
function Add-ShownSplit([int]$Pane, [object[]]$Program) {
  $id = [int]((Invoke-Cli split-pane --pane-id $Pane --right --percent 50 -Program $Program) -join '').Trim()
  if ($script:Mux) { Invoke-Cli activate-pane --pane-id $id -Check | Out-Null }
  $id
}


# Each tab's program: its blocks, the cursor hidden, then whatever turns up in its command file.
# `tick` moves a block along the last row a column every 40 ms until `stop`
$e = [char]27
function Stand-In([string]$name, [int]$from, [int]$to, [int]$rows, [int]$grey) {
  $file = Join-Path $commands "$name.txt"
  $script = @"
`$e = [char]27
`$fill = "`$e[48;2;$grey;$grey;${grey}m" + (' ' * ($to - $from)) + "`$e[0m"
`$out = "`$e[?25l`$e[2J"
for (`$r = 1; `$r -le $rows; `$r++) { `$out += "`$e[`$(`$r + 1);$($from + 1)H" + `$fill }
[Console]::Write(`$out)
`$tick = `$false; `$col = 0
while (`$true) {
  if (Test-Path -LiteralPath '$file') {
    `$t = [IO.File]::ReadAllText('$file'); Remove-Item -LiteralPath '$file'
    if (`$t -eq 'tick') { `$tick = `$true } elseif (`$t -eq 'stop') { `$tick = `$false; [Console]::Write("`$e[28;1H`$e[2K") }
  }
  if (`$tick) {
    `$col = (`$col + 1) % 90
    [Console]::Write("`$e[28;1H`$e[2K`$e[28;`$(`$col + 1)H`$e[48;2;230;230;230m    `$e[0m")
    Start-Sleep -Milliseconds 40
  } else { Start-Sleep -Milliseconds 50 }
}
"@
  New-PwshProgram $script
}
function Send-To([string]$name, [string]$text) {
  $file = Join-Path $commands "$name.txt"
  Write-FileWhole -Path $file -Text $text -Temp "$file.part"
}

$lua = @"
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true }
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.ActivateTabRelative(1) })
table.insert(config.keys, { key = 'F8', mods = 'NONE', action = wezterm.action.ActivateTabRelative(-1) })
"@

# The GUI's log: what the compositor says it did (debug), with whatever -Log asks for too
$logSpec = (@('info', 'lbterm_gui::termwindow::compositor=debug', $Log) | Where-Object { $_ }) -join ','

# tab_switch at -Slowdown, cubic-bezier(0.23, 1, 0.32, 1): its y is 1 - (1 - u)^3 at the u whose
# x is Get-BezierX's. Get-FadeAt: how much of a fade from all of it is left `ms` after it started;
# Get-FadeMs: how long into one a share `v` is left
$fadeWhole = 200 * $Slowdown
function Get-BezierX([double]$u) { 3 * (1 - $u) * (1 - $u) * $u * 0.23 + 3 * (1 - $u) * $u * $u * 0.32 + $u * $u * $u }
function Get-FadeAt([double]$ms) {
  $t = $ms / $fadeWhole
  if ($t -le 0) { return 1.0 }
  if ($t -ge 1) { return 0.0 }
  $a = 0.0; $b = 1.0
  for ($k = 0; $k -lt 40; $k++) { $u = ($a + $b) / 2; if ((Get-BezierX $u) -lt $t) { $a = $u } else { $b = $u } }
  [Math]::Pow(1 - ($a + $b) / 2, 3)
}
# (Doubles throughout: [Math]::Max(0, $v) takes the int overload and rounds $v to 0 or 1)
function Get-FadeMs([double]$v) { (Get-BezierX (1.0 - [Math]::Pow([Math]::Min(1.0, [Math]::Max(0.0, $v)), 1.0 / 3.0))) * $fadeWhole }

# How far this run's captures trail the state file's frames: a capture's picture is of a moment
# before it returned, by as long as PrintWindow and DWM took, which on a busy PC is more than two
# refreshes (45 ms in the review of 2026-10-31, where a fixed 34 ms failed a run). From a fade on
# tab_switch from all of it that started at the state file's frame `start` (the frames' clock) and
# that nothing else moved before `until` (`trace`: Ms and Value), each capture's time less when the
# fade showed what that capture shows: the most of them and a refresh more (17 ms), in 17 to
# 150 ms; `fallback` when no capture caught the fade on its way
function Measure-CaptureLag($trace, [double]$start, [double]$until, [double]$fallback = 50) {
  $lags = @($trace | Where-Object { $_.Ms -ge $start -and $_.Ms -lt $until -and $_.Value -gt 0.05 -and $_.Value -lt 0.95 } |
    ForEach-Object { $_.Ms - ($start + (Get-FadeMs $_.Value)) })
  if (-not $lags.Count) { return [pscustomobject]@{ Lag = $fallback; Seen = @() } }
  $most = ($lags | Measure-Object -Maximum).Maximum
  [pscustomobject]@{ Lag = [Math]::Min(150.0, [Math]::Max(17.0, $most + 17.0)); Seen = $lags }
}

# A window with the three tabs, on tab one, settled; `extra` more of its config
# Each window a class of its own, so its control pipe (`lbterm-control-<class>`) is its own: with
# the test's one class every window listened on the same pipe name, and Get-Metrics on one window
# reached whichever of them answered (PITFALLS 223)
$script:WindowClasses = 0
function New-WindowClass { $script:WindowClasses++; "$($T.Class).w$($script:WindowClasses)" }

function Open-Tabs([string]$motion, [string]$extra = '', [switch]$PostedPointer) {
  # -Slow: every stretch of the GUI thread over 1 ms in the log (lbterm_extras::slow), by site
  $slowEnv = if ($Slow) { @{ LBTERM_SLOW_EVERY = '1' } } else { @{} }
  $slowLua = if ($Slow) { "`nconfig.lbterm_slow_ms = 1`n" } else { '' }
  $mux = Get-MuxOpen
  $open = $mux.Open
  $w = Open-TestWindow @open -Class (New-WindowClass) -Exe $guiExe -Environment $slowEnv -Log $logSpec -PostedPointer:$PostedPointer -WorkingDirectory $T.Repo -Program (Stand-In one 0 40 20 220) -Lua ($lua + $slowLua + $extra + "`nconfig.lbterm_motion = { enabled = '$motion', slowdown = $Slowdown }`n" + $mux.Lua)
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $panes = @{ one = Use-ServerTabs (Stand-In one 0 40 20 220) }
  $panes.two = New-Tab -From $panes.one -Program (Stand-In two 50 90 20 220)
  $panes.three = New-Tab -From $panes.one -Program (Stand-In three 20 70 10 120)
  foreach ($n in 'one', 'two', 'three') { Invoke-Cli set-tab-title --pane-id $panes[$n] $n | Out-Null }
  Invoke-Cli activate-tab --pane-id $panes.one --tab-index 0 | Out-Null
  $st = Wait-Change { param($s) $s.active_tab -eq 0 -and $s.strip.title -eq 'one' -and @($s.rail.rows).Count -eq 3 } -What 'tab one, three rows' -Animating
  # Its program has drawn its blocks
  Start-Sleep -Milliseconds 300
  # Tab one's pane as the window numbers it (the state's fading panes and the log's lines name
  # the window's ids, which on a mux domain aren't the server's the cli takes)
  [pscustomobject]@{ Window = $w; Panes = $panes; GuiOne = @($st.panes)[0].id }
}

# Five tabs for step 9, in a window of their own, on tab one, settled: each a block in a band of
# the pane no other tab draws (tab one's in its left pane, split off the right, which has the
# keyboard) and a title of its own
function Open-Flight {
  $bands = @(@(4, 12), @(18, 26), @(30, 38), @(58, 66), @(72, 80))
  $titles = @('alpha one', 'bravo two two', 'charlie three', 'delta four four four', 'echo five')
  $mux = Get-MuxOpen
  $open = $mux.Open
  $w = Open-TestWindow @open -Class (New-WindowClass) -Exe $guiExe -Log $logSpec -WorkingDirectory $T.Repo -Program (Stand-In fl1 $bands[0][0] $bands[0][1] 18 220) -Lua ($lua + "`nconfig.lbterm_motion = { enabled = 'on', slowdown = $Slowdown }`n" + $mux.Lua)
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $panes = [Collections.Generic.List[int]]::new()
  $panes.Add((Use-ServerTabs (Stand-In fl1 $bands[0][0] $bands[0][1] 18 220)))
  for ($k = 1; $k -lt 5; $k++) { $panes.Add((New-Tab -From $panes[0] -Program (Stand-In "fl$($k + 1)" $bands[$k][0] $bands[$k][1] 18 220))) }
  for ($k = 0; $k -lt 5; $k++) { Invoke-Cli set-tab-title --pane-id $panes[$k] $titles[$k] | Out-Null }
  Invoke-Cli activate-tab --pane-id $panes[0] --tab-index 0 | Out-Null
  Wait-Change { param($s) $s.active_tab -eq 0 -and @($s.rail.rows).Count -eq 5 } -What 'tab one of five' -Animating | Out-Null
  Add-ShownSplit $panes[0] (Stand-In fl1b 0 0 0 18) | Out-Null
  Wait-Change { param($s) $s.active_tab -eq 0 -and @($s.panes).Count -eq 2 } -What 'tab one in two panes' -Animating | Out-Null
  # Their programs have drawn their blocks
  Start-Sleep -Milliseconds 300
  [pscustomobject]@{ Window = $w; Panes = $panes.ToArray(); Titles = $titles }
}

# Step 9's checks on one tab's share, frame by frame (`Ms` and `Value` each), by when the tab was
# left (`left`, the frame of the switch that left it, from the state file) and, for the tab that
# comes back, when it was switched back to (`back`, the same): once it's in flight, from as long
# as the captures trail the state file (`lag`, Measure-CaptureLag), its share never rises more
# than `tol` over where it had got to; the tab coming back never falls under `fall` of what its
# own fade showed at the switch (`expect`: a frame captured after the switch's own frame shows
# its fade going on until the switch showed, or it coming back from there), and rises once the
# switch has shown; and no share steps between two frames by more than the curve can move in the
# time between them, `slack` more for what a capture adds. Each problem is a failure; what was
# left out is said.
function Test-FlightTrace($trace, [string]$what, [double]$left, $back, [double]$tol, [double]$slack, [int]$spikes = 0, [double]$lag = 34, $expect = $null) {
  $steep = 2 * 4.35 / (200 * $Slowdown)
  $fall = 0.75
  $problems = [Collections.Generic.List[string]]::new()
  # `spikes` frames each far from both its neighbours, which agree, can be left out: the strip's
  # words now and then read as neither tab in one captured frame (step 1), in main's build too
  if ($spikes -gt 0) {
    $kept = [Collections.Generic.List[object]]::new(); $out = 0
    for ($i = 0; $i -lt $trace.Count; $i++) {
      if ($i -gt 0 -and $i -lt $trace.Count - 1) {
        $a = $trace[$i - 1].Value; $v = $trace[$i].Value; $b = $trace[$i + 1].Value
        if ([Math]::Abs($v - $a) -gt 0.15 -and [Math]::Abs($v - $b) -gt 0.15 -and [Math]::Abs($b - $a) -lt 0.1 -and $out -lt $spikes) { $out++; continue }
      }
      $kept.Add($trace[$i])
    }
    if ($out) { "flight, $($what): $out frame$(if ($out -gt 1) { 's' }) read as neither left out" }
    $trace = $kept.ToArray()
  }
  for ($i = 1; $i -lt $trace.Count; $i++) {
    $d = $trace[$i].Value - $trace[$i - 1].Value
    $limit = $slack + $steep * ($trace[$i].Ms - $trace[$i - 1].Ms)
    if ([Math]::Abs($d) -gt $limit) { $problems.Add("stepped $('{0:N2}' -f $trace[$i - 1].Value) to $('{0:N2}' -f $trace[$i].Value) at $($trace[$i].Ms) ms (in $('{0:N0}' -f ($trace[$i].Ms - $trace[$i - 1].Ms)) ms, $('{0:N2}' -f $limit) at most)") }
  }
  $until = if ($null -ne $back) { [double]$back } else { [double]::MaxValue }
  $low = [double]::MaxValue
  foreach ($p in @($trace | Where-Object { $_.Ms -ge $left + $lag -and $_.Ms -lt $until })) {
    if ($p.Value -gt $low + $tol) { $problems.Add("rose in flight to $('{0:N2}' -f $p.Value) from $('{0:N2}' -f $low) at $($p.Ms) ms"); break }
    $low = [Math]::Min($low, $p.Value)
  }
  if ($null -ne $back) {
    # Never under where its own fade had got to at the switch: the old jump left 5 to 8% of it.
    # Until 2026-10-31 this was the last frame captured before the switch's frame, which with
    # the captures 45 ms behind showed it 100 ms of its fade earlier (0.50, where it was 0.26)
    if ($null -ne $expect) {
      foreach ($p in @($trace | Where-Object { $_.Ms -ge $back })) {
        if ($p.Value -lt $expect * $fall) { $problems.Add("fell to $('{0:N2}' -f $p.Value) at $($p.Ms) ms as it was switched back to, where its fade had got to $('{0:N2}' -f $expect)"); break }
      }
    }
    $high = [double]::MinValue
    foreach ($p in @($trace | Where-Object { $_.Ms -ge $back + $lag })) {
      if ($p.Value -lt $high - $tol) { $problems.Add("fell to $('{0:N2}' -f $p.Value) from $('{0:N2}' -f $high) at $($p.Ms) ms after it was switched back to"); break }
      $high = [Math]::Max($high, $p.Value)
    }
  }
  foreach ($p in $problems) { Add-Failure "tabs in flight, $($what): $p" }
}

# A switch: the key, then the state's first frame on tab `index` (how long that took), recording
# frames until it has settled there when -Frames
function Switch-Tab($w, [string]$key, [int]$index, [switch]$Frames, [scriptblock]$Each) {
  $before = Get-StateFrame $w
  # `Back` and `Forward` are the mouse's side buttons over the middle of the pane, as Lachlan hops
  # tabs (his lbterm.lua's, which skips parked tabs); anything else a key
  $side = $key -in 'Back', 'Forward'
  if ($side) { $x, $y = Get-PaneMiddle $w }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  # -Cli: the switches 1 and 3 make go through `lbterm cli activate-tab`, as the Conductor's and
  # Relay's do, the rest still buttons
  if ($Cli -and -not $Each) { Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index $index | Out-Null }
  elseif ($side) { Send-SideButton $w $x $y -Forward:($key -eq 'Forward') }
  else { Send-Key $w $key }
  # The blocks below run inside the harness's waits, which find this function's variables
  $seen = @{ First = $null; Ms = $null; All = [Collections.Generic.List[object]]::new() }
  $record = { param($s)
    $seen.All.Add($s)
    if (-not $seen.First -and $s.active_tab -eq $index) { $seen.First = $s; $seen.Ms = $clock.Elapsed.TotalMilliseconds }
    $s.active_tab -eq $index -and -not $s.animating }
  if ($Frames) {
    $shots = Get-Frames $w ([int](200 * $Slowdown + 1500)) -TailMs 120 -Each $Each -Until { param($s) & $record $s }
  } else {
    $shots = @()
    Wait-State { param($s) & $record $s } -AfterFrame $before -What "tab $index" | Out-Null
  }
  [pscustomobject]@{ Shots = $shots; First = $seen.First; Ms = $seen.Ms; States = @($seen.All) }
}

# The middle of the open tab's first pane, where a side button is pressed
function Get-PaneMiddle($w) {
  $p = @((Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).panes)[0]
  Get-RectCenter $p.rect
}

# The active outline where tab four (split in two, the keyboard in its right pane) and tab three
# (one pane) draw it, from shots of each at rest: `Old`, a strip a few pixels about the right
# pane's left side, which only four's outline has; `New`, about three's left side, which only
# three's has (four's left pane hasn't the keyboard); `Shared`, about the right pane's top, which
# both draw. Each along the middle of its run, clear of tab three's block, at the column (or row)
# near the card's edge where the two shots differ most (for `Shared`, where four's is brightest).
# The window background and the cards are one colour, so only the outline differs there. Beside
# each, a gutter further into its card (`OldBg`, `NewBg`, `SharedBg`), where no outline is: a
# line's light is read over what's under it in the same shot, since now and then a frame of a
# fade comes out a level darker over the whole of a pane, the line on it too (DWM's blend of one
# visual over another of the same colour, rounded down: 17 for 18), which read as a seventh of
# the line gone. `Corner`: where four's right pane's top-left curve runs into the top line, on the
# line's rows, which three's outline draws straight (the design system's sizes: half a gutter of
# 2 px, the 12 px curve, a 1 px hairline with a soft pixel either side)
function Find-OutlineStrips($fourShot, $threeShot, $onRect, $threeRect, [double]$scale) {
  $p = { param($v) ConvertTo-Px $v $scale }
  $y0 = [int]($onRect.y + 0.45 * $onRect.h); $y1 = [int]($onRect.y + 0.85 * $onRect.h)
  $column = { param([double]$edge)
    $best = [int]$edge; $most = -1
    foreach ($x in ([int]$edge - (& $p 4))..([int]$edge + (& $p 4))) {
      $r = New-Rect $x $y0 1 ($y1 - $y0)
      $d = [Math]::Abs([LbtermE2E.Px]::Light($fourShot, $r) - [LbtermE2E.Px]::Light($threeShot, $r))
      if ($d -gt $most) { $most = $d; $best = $x }
    }
    New-Rect ($best - (& $p 2)) $y0 ((& $p 4) + 1) ($y1 - $y0) }
  $x0 = [int]($onRect.x + 0.3 * $onRect.w); $x1 = [int]($onRect.x + 0.7 * $onRect.w)
  $top = [int]$onRect.y; $most = -1
  foreach ($y in ([int]$onRect.y - (& $p 4))..([int]$onRect.y + (& $p 4))) {
    $l = [LbtermE2E.Px]::Light($fourShot, (New-Rect $x0 $y ($x1 - $x0) 1))
    if ($l -gt $most) { $most = $l; $top = $y }
  }
  $in = & $p 8
  $old = & $column $onRect.x; $new = & $column $threeRect.x
  $shared = New-Rect $x0 ($top - (& $p 2)) ($x1 - $x0) ((& $p 4) + 1)
  $band = & $p 2; $radius = & $p 12; $hair = & $p 1; $soft = & $p 1
  $boxEnd = [int]$onRect.x - $band + ($band + $radius + $soft)
  $reach = [Math]::Ceiling($soft + [Math]::Sqrt([Math]::Pow($radius + $soft / 2, 2) - [Math]::Pow($radius - $hair / 2, 2)))
  $cx = $boxEnd - $reach - (& $p 3)
  $corner = New-Rect $cx ([int]$onRect.y - $soft) ($boxEnd - $cx) ($hair + 2 * $soft)
  $s = [pscustomobject]@{
    Corner = $corner; CornerBg = (New-Rect $corner.X ($corner.Y + $in + (& $p 4)) $corner.Width $corner.Height)
    Old = $old; OldBg = (New-Rect ($old.X + $in) $old.Y $old.Width $old.Height)
    New = $new; NewBg = (New-Rect ($new.X + $in) $new.Y $new.Width $new.Height)
    Shared = $shared; SharedBg = (New-Rect $shared.X ($shared.Y + $in) $shared.Width $shared.Height)
  }
  $s | Add-Member -NotePropertyName Lit -NotePropertyValue ([pscustomobject]@{
    Old = (Get-LineLight $fourShot $s 'Old') - (Get-LineLight $threeShot $s 'Old')
    New = (Get-LineLight $threeShot $s 'New') - (Get-LineLight $fourShot $s 'New')
    # The same line in both: what lighting it twice would add shows against this
    Shared = Get-LineLight $fourShot $s 'Shared'
    SharedRest = ((Get-LineLight $fourShot $s 'Shared') + (Get-LineLight $threeShot $s 'Shared')) / 2
    # The brighter of the two at rest where the curve meets the line: three's straight line
    Corner = [Math]::Max((Get-LineLight $fourShot $s 'Corner'), (Get-LineLight $threeShot $s 'Corner'))
  })
  $s
}

# A strip's light over the gutter beside it in the same shot (Find-OutlineStrips)
function Get-LineLight($shot, $strips, [string]$which) {
  [LbtermE2E.Px]::Light($shot, $strips.$which) - [LbtermE2E.Px]::Light($shot, $strips."${which}Bg")
}

# How much of each outline shows in a shot, by Find-OutlineStrips' strips: `Old` four's share
# of its right pane's left side, `New` three's of its own left side, `Shared` how much
# brighter the line both draw is than at rest, and `Corner` how much brighter the top line is
# where four's curve runs into it than the brighter of the two at rest, in levels
function Measure-Outline($shot, $strips, $fourShot, $threeShot) {
  [pscustomobject]@{
    Corner = (Get-LineLight $shot $strips 'Corner') - $strips.Lit.Corner
    Old = ((Get-LineLight $shot $strips 'Old') - (Get-LineLight $threeShot $strips 'Old')) / [Math]::Max(0.5, $strips.Lit.Old)
    New = ((Get-LineLight $shot $strips 'New') - (Get-LineLight $fourShot $strips 'New')) / [Math]::Max(0.5, $strips.Lit.New)
    Shared = (Get-LineLight $shot $strips 'Shared') - $strips.Lit.SharedRest
  }
}

# Fails the test for frames in which the top line, where four's right pane's top-left curve runs
# into it, is brighter than the brighter of the two outlines at rest there by more than 1.5
# levels: the curve's end and the straight line both lit (Measure-Outline's `Corner`)
function Test-CornerOnce($frames, [string]$when) {
  $over = @($frames | Where-Object { $_.Corner -gt 1.5 })
  "corner, $($when): where the curve meets the top line, levels over the brighter rest $(Format-Trace @($frames | ForEach-Object Corner) '{0:N1}')"
  if ($over.Count) {
    $most = $over | Sort-Object { -$_.Corner } | Select-Object -First 1
    $name = 'corner-' + ($when -replace '[^a-z]+', '-').Trim('-')
    Save-Shot $most.Shot $name
    Add-Failure "where four's right pane's top-left curve runs into the top line it was lit twice $($when): up to $('{0:N1}' -f $most.Corner) levels over the brighter of the two at rest, in $($over.Count) of $(@($frames).Count) frames ($name.png)"
  }
}

# Step 9, tabs in flight (Lachlan's 3a), in a window of their own: five tabs, each a block in a
# band of the pane no other tab draws and a title of its own, tab one split in two with the
# keyboard in its right pane. From one to five by the forward button 40 ms apart, as fast as a
# hand clicks, and back to one by a click on its pad 40 ms after that: four tabs are in flight,
# and the one switched back to is the oldest, on top of them all
function Test-Flight {
  $lum = { param($shot, $r) [LbtermE2E.Px]::Light($shot, $r) }
  $fl = Open-Flight
  $w9 = $fl.Window
  $px9 = { param($v) ConvertTo-Px $v $w9.Scale }
  $refs = [Collections.Generic.List[System.Drawing.Bitmap]]::new()
  $states9 = [Collections.Generic.List[object]]::new()
  for ($k = 0; $k -lt 5; $k++) {
    Invoke-Cli activate-tab --pane-id $fl.Panes[0] --tab-index $k | Out-Null
    $states9.Add((Wait-Change { param($s) $s.active_tab -eq $k } -What "tab $($k + 1) of five" -Animating))
    Start-Sleep -Milliseconds 200
    $refs.Add((Get-Shot $w9 -Name "flight-$($k + 1)"))
  }
  Invoke-Cli activate-tab --pane-id $fl.Panes[0] --tab-index 0 | Out-Null
  $atOne = Wait-Change { param($s) $s.active_tab -eq 0 } -What 'back on tab one of five' -Animating
  Start-Sleep -Milliseconds 200
  $onRect9 = @($states9[0].panes | Where-Object active)[0].rect
  $leftRect9 = @($states9[0].panes | Where-Object { -not $_.active })[0].rect
  $roomRect9 = @($states9[1].panes)[0].rect
  # Each tab's band: its block in its own shot, a margin in; its light there, and the others'
  $bands9 = [Collections.Generic.List[object]]::new()
  for ($k = 0; $k -lt 5; $k++) {
    $area = ConvertTo-Rect $(if ($k -eq 0) { $leftRect9 } else { $roomRect9 })
    $b = [LbtermE2E.Px]::BrightBox($refs[$k], $area, 150)
    $m = & $px9 3
    $bands9.Add($(if ($b) { New-Rect ($b[0] + $m) ($b[1] + $m) ($b[2] - $b[0] - 2 * $m) ($b[3] - $b[1] - 2 * $m) } else { $null }))
  }
  $wordRects = @($states9 | ForEach-Object { @($_.strip.items | Where-Object name -eq 'words')[0].rect } | Where-Object { $_ })
  if (@($bands9 | Where-Object { $null -eq $_ }).Count -or $wordRects.Count -lt 5 -or -not $onRect9 -or -not $leftRect9) {
    Add-Failure "tabs in flight: the five tabs didn't show their blocks, words and panes to follow (blocks $(@($bands9 | ForEach-Object { if ($_) { 'found' } else { 'none' } }) -join ', '), $($wordRects.Count) words)"
  } else {
    $full9 = @(0..4 | ForEach-Object { & $lum $refs[$_] $bands9[$_] })
    $bg9 = @(0..4 | ForEach-Object { $k = $_; (@(0..4 | Where-Object { $_ -ne $k } | ForEach-Object { & $lum $refs[$_] $bands9[$k] }) | Measure-Object -Average).Average })
    $paneShare = { param($shot, [int]$k) ((& $lum $shot $bands9[$k]) - $bg9[$k]) / [Math]::Max(1, $full9[$k] - $bg9[$k]) }
    $x0 = (@($wordRects | ForEach-Object x) | Measure-Object -Minimum).Minimum
    $y0 = (@($wordRects | ForEach-Object y) | Measure-Object -Minimum).Minimum
    $x1 = (@($wordRects | ForEach-Object { $_.x + $_.w }) | Measure-Object -Maximum).Maximum
    $y1 = (@($wordRects | ForEach-Object { $_.y + $_.h }) | Measure-Object -Maximum).Maximum
    $words9 = New-Rect ([int]$x0) ([int]$y0) ([int]($x1 - $x0)) ([int]($y1 - $y0))
    $parts9 = [System.Drawing.Bitmap[]]$refs.ToArray()
    $self = @(0..4 | ForEach-Object { $m = [LbtermE2E.Px]::Mix($refs[$_], $parts9, $words9); if ($m.Count -eq 5) { $m[$_] } else { 0 } })
    $strips9 = Find-OutlineStrips $refs[0] $refs[1] $onRect9 $roomRect9 $w9.Scale
    "flight: each tab's block $(Format-Trace $full9 '{0:N0}') over $(Format-Trace $bg9 '{0:N0}'); each tab's words read as itself $(Format-Trace $self); the outline lit $('{0:N1}' -f $strips9.Lit.Old) levels on tab one's right pane's left side, $('{0:N1}' -f $strips9.Lit.New) on the room's"
    $outline = $strips9.Lit.Old -ge 1.5 -and $strips9.Lit.New -ge 1.5
    if (-not $outline) { Add-Failure "tabs in flight: the active outline didn't show where tab one's and the others' differ ($('{0:N1}' -f $strips9.Lit.Old) and $('{0:N1}' -f $strips9.Lit.New) levels)" }
    if (@($self | Where-Object { $_ -lt 0.9 }).Count) { Add-Failure "tabs in flight: the five tabs' words can't be told apart in the strip ($(Format-Trace $self))" }
    else {
      $padOne = @(@($atOne.rail.rows) | Where-Object tab_index -eq 0)[0]
      $mx9, $my9 = Get-RectCenter $leftRect9
      $ox, $oy = Get-RectCenter $padOne.pad
      # The four presses and the click 40 ms apart on a clock of their own (Start-TimedPosts),
      # started with the first capture: posted from Get-Frames' -Each, once a capture, they went
      # 45 to 120 ms apart on a busy PC (the review of 2026-10-31)
      $plan9 = @(@(0, 40, 80, 120) | ForEach-Object { @{ AtMs = $_; Messages = @(Get-SideButtonMessages $mx9 $my9 -Forward) } }) + @(@{ AtMs = 160; Messages = @(Get-ClickMessages $ox $oy -Leave) })
      $script:f9 = @{ Poster = $null; Unix0 = $null; Watched = $false }
      # Compiled before the captures start, which its first use held up 200 ms
      (Start-TimedPosts ([IntPtr]::Zero) @()).Wait(1000) | Out-Null
      $landed9 = [Collections.Generic.List[object]]::new()
      $seen9 = [Collections.Generic.List[object]]::new()
      $shots9 = Get-Frames $w9 ([int](200 * $Slowdown + 3000)) -TailMs 120 -Each { param($ms)
        $f = $script:f9
        if ($null -eq $f.Poster) { $f.Unix0 = (Get-UnixMs) - $ms; $f.Poster = Start-TimedPosts $w9 $plan9; return }
        if (-not $f.Watched -and $f.Poster.Done) { $f.Watched = $true; Watch-Foreground }
      } -Until { param($s, $snap)
        $seen9.Add($s)
        if ($null -ne $script:f9.Unix0) { $landed9.Add([pscustomobject]@{ Tab = $s.active_tab; Ms = $snap.time_ms - $script:f9.Unix0 }) }
        $null -ne $script:f9.Poster -and $script:f9.Poster.Done -and $s.active_tab -eq 0 -and -not $s.animating }
      $poster9 = $script:f9.Poster
      $at9 = @(if ($poster9) { $poster9.Sent | Where-Object { -not [double]::IsNaN($_) } | ForEach-Object { $poster9.StartUnixMs + $_ - $script:f9.Unix0 } })
      $gaps9 = @(for ($k = 1; $k -lt $at9.Count; $k++) { $at9[$k] - $at9[$k - 1] })
      # When each switch showed: the state file's first frame on the tab it went to, after its press
      $to9 = @(1, 2, 3, 4, 0)
      $shown9 = @(for ($k = 0; $k -lt [Math]::Min(5, $at9.Count); $k++) {
        $first = @($landed9 | Where-Object { $_.Tab -eq $to9[$k] -and $_.Ms -ge $at9[$k] - 5 } | Select-Object -First 1)
        if ($first.Count) { $first[0].Ms } else { $at9[$k] + 50 }
      })
      $click9 = if ($shown9.Count -ge 5) { $shown9[4] } else { $null }
      $flight = @($shots9 | ForEach-Object {
        $o = Measure-Outline $_.Shot $strips9 $refs[0] $refs[1]
        $shot = $_.Shot
        [pscustomobject]@{ Ms = $_.Ms; Panes = @(0..4 | ForEach-Object { & $paneShare $shot $_ }); Words = [LbtermE2E.Px]::Mix($shot, $parts9, $words9); Old = $o.Old; New = $o.New; Shared = $o.Shared; Corner = $o.Corner; Shot = $shot }
      })
      $mostSwaps = (@($seen9 | ForEach-Object { [int]$_.fading_swaps }) | Measure-Object -Maximum).Maximum
      $mostPanes = (@($seen9 | ForEach-Object { @($_.fading_panes).Count }) | Measure-Object -Maximum).Maximum
      "flight: the switches posted at $(Format-Trace @($at9) '{0:N0}') ms ($(Format-Trace $gaps9 '{0:N0}') ms apart), the click back to one last, each shown at $(Format-Trace @($shown9) '{0:N0}') ms; up to $mostPanes pane visuals fading at once and $mostSwaps of the strip's words older than the newest"
      # How far the captures trail the state file, on tab one's own fade before the click (tab
      # one is the oldest, on top, so what shows of it is its fade alone, in every build), and
      # what that fade had got to at the click
      $lag9 = Measure-CaptureLag @($flight | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Value = $_.Panes[0] } }) $shown9[0] $(if ($null -ne $click9) { $click9 } else { [double]::MaxValue })
      $expect9 = if ($null -ne $click9 -and $shown9.Count) { Get-FadeAt ($click9 - $shown9[0]) } else { $null }
      "flight: the captures trail the state file's frames by $(Format-Trace $lag9.Seen '{0:N0}') ms on tab one's fade, so a tab is in flight from $('{0:N0}' -f $lag9.Lag) ms after the frame that left it; tab one's fade had got to $(if ($null -ne $expect9) { '{0:N2}' -f $expect9 } else { '(no click)' }) at the click"
      for ($k = 0; $k -lt 5; $k++) {
        "flight: tab $($k + 1), its panes $(Format-Trace @($flight | ForEach-Object { $_.Panes[$k] })); its words $(Format-Trace @($flight | ForEach-Object { if ($_.Words.Count -eq 5) { $_.Words[$k] } }))"
      }
      # Around the click back to one: each tab's share in the last frame before it and the three after
      if ($null -ne $click9) {
        $pre9 = @($flight | Where-Object { $_.Ms -lt $click9 } | Select-Object -Last 1)
        $post9 = @($flight | Where-Object { $_.Ms -ge $click9 } | Select-Object -First 3)
        if ($pre9.Count -and $post9.Count) {
          "flight: around the click at $('{0:N0}' -f $click9) ms, the frame before at $($pre9[0].Ms) ms and the three after at $(@($post9 | ForEach-Object Ms) -join ', ') ms: " + ((0..4 | ForEach-Object { $k = $_; "tab $($k + 1) $('{0:N2}' -f $pre9[0].Panes[$k]) to $(Format-Trace @($post9 | ForEach-Object { $_.Panes[$k] }))" }) -join '; ')
        }
      }
      "flight: tab one's outline, its own side $(Format-Trace @($flight | ForEach-Object Old)); the room's left side $(Format-Trace @($flight | ForEach-Object New)); the top both draw, levels over rest $(Format-Trace @($flight | ForEach-Object Shared) '{0:N1}')"
      $last9 = @($seen9)[-1]
      if ($at9.Count -lt 5 -or $last9.active_tab -ne 0) { Add-Failure "tabs in flight: the run didn't happen as planned ($($at9.Count) switches, ended on tab $($last9.active_tab + 1))" }
      elseif ($mostSwaps -lt 2) { Add-Failure "tabs in flight: at most $mostSwaps of the strip's words older than the newest fading at once, where four tabs were in flight" }
      else {
        $failed = (Get-Test).Failures.Count
        for ($k = 0; $k -lt 5; $k++) {
          # Each tab's share of its panes, never rising in flight, not stepping and tab one coming
          # back from what it showed, is the compositor's given the panes left, and its test holds
          # it from what DWM was handed (compositor/imp/tests/flight.rs, feat/dcomp-recorder); the
          # words, the outlines and the run itself are judged here
          $backAt = if ($k -eq 0) { $click9 } else { $null }
          $expect = if ($k -eq 0) { $expect9 } else { $null }
          $wordsTrace = @($flight | Where-Object { $_.Words.Count -eq 5 } | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Value = $_.Words[$k] } })
          Test-FlightTrace $wordsTrace "tab $($k + 1)'s words" $shown9[$k] $backAt 0.08 0.12 ([Math]::Max(2, [int]($wordsTrace.Count / 10))) -lag $lag9.Lag -expect $expect
          # Each tab's words keep its panes' share, but for a captured frame or two read as neither
          $apart = @($flight | Where-Object { $_.Words.Count -eq 5 -and [Math]::Abs($_.Words[$k] - $_.Panes[$k]) -gt 0.15 })
          if ($apart.Count -gt [Math]::Max(2, $flight.Count / 4)) { Add-Failure "tabs in flight: tab $($k + 1)'s words and panes apart by more than 0.15 in $($apart.Count) of $($flight.Count) frames" }
        }
        # Tab one's outline, where only it draws, in step with its panes before the click and as it
        # comes back; the room's, the other tabs', never jumping; no line lit twice
        $apart = @($flight | Where-Object { $outline -and [Math]::Abs($_.Old - $_.Panes[0]) -gt 0.12 })
        if ($apart.Count) {
          $most = $apart | Sort-Object { -[Math]::Abs($_.Old - $_.Panes[0]) } | Select-Object -First 1
          Save-Shot $most.Shot 'flight-outline-apart'
          Add-Failure "tabs in flight: tab one's outline and its panes apart in $($apart.Count) of $($flight.Count) frames, the most $('{0:N2}' -f $most.Old) against $('{0:N2}' -f $most.Panes[0]) at $($most.Ms) ms (flight-outline-apart.png)"
        }
        # The room's outline steps no further between two frames than the curve can move in the
        # time between them, as Test-FlightTrace's shares: a fixed 0.25 failed its own fade across
        # a capture held up 123 ms (0.83 to 0.56, 2026-10-31)
        $newTrace = @($flight | Where-Object { $outline } | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Value = $_.New } })
        $steep9 = 2 * 4.35 / (200 * $Slowdown)
        for ($i = 1; $i -lt $newTrace.Count; $i++) {
          $limit = 0.1 + $steep9 * ($newTrace[$i].Ms - $newTrace[$i - 1].Ms)
          if ($newTrace[$i].Ms -gt $at9[0] + 100 -and [Math]::Abs($newTrace[$i].Value - $newTrace[$i - 1].Value) -gt $limit) { Add-Failure "tabs in flight: the room's outline jumped from $('{0:N2}' -f $newTrace[$i - 1].Value) to $('{0:N2}' -f $newTrace[$i].Value) at $($newTrace[$i].Ms) ms (in $('{0:N0}' -f ($newTrace[$i].Ms - $newTrace[$i - 1].Ms)) ms, $('{0:N2}' -f $limit) at most)"; break }
        }
        $twice = @($flight | Where-Object { $outline -and $_.Shared -gt 1.5 })
        if ($twice.Count) { Add-Failure "tabs in flight: the top line tab one's and the room's outlines both draw was lit twice, up to $('{0:N1}' -f (@($twice | ForEach-Object Shared) | Measure-Object -Maximum).Maximum) levels over its rest in $($twice.Count) frames" }
        if ($outline) { Test-CornerOnce $flight 'with tabs in flight' }
        $atClick = @($flight | Where-Object { $_.Ms -ge $click9 }) | Select-Object -First 3 | Select-Object -Last 1
        if ($atClick) { Save-Shot $atClick.Shot 'flight-back' }
        if ((Get-Test).Failures.Count -eq $failed) { "flight: no tab's share of its words rose while it was in flight or stepped, and tab one's outline went with its panes (the panes' own share: compositor/imp/tests/flight.rs)" }
      }
      Remove-Frames $shots9
    }
  }
  foreach ($r9 in $refs) { $r9.Dispose() }
}

Invoke-Test {
  # -Flight: step 9 alone
  if ($Flight) { Test-Flight; return }
  $tabs = Open-Tabs 'on'
  $w = $tabs.Window
  $px = { param($v) ConvertTo-Px $v $w.Scale }
  # Where the side buttons are pressed: the middle of the first pane, left of any split
  $mx, $my = Get-PaneMiddle $w

  if ($Measure) {
    if ($Split) {
      Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index 1 | Out-Null
      Add-ShownSplit $tabs.Panes.two (Stand-In twob 0 0 0 18) | Out-Null
      Wait-Change { param($s) $s.active_tab -eq 1 -and @($s.panes).Count -eq 2 } -What 'tab two in two panes' -Animating | Out-Null
      Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index 0 | Out-Null
      Wait-Change { param($s) $s.active_tab -eq 0 } -What 'tab one' -Animating | Out-Null
    }
    $proc = Get-Process -Id $w.Pid
    $cpuNow = { $proc.Refresh(); $proc.TotalProcessorTime.TotalMilliseconds }
    $gpu = { try { (Get-Counter "\GPU Process Memory(pid_$($w.Pid)_*)\Dedicated Usage" -ErrorAction Stop).CounterSamples | Measure-Object CookedValue -Sum | ForEach-Object Sum } catch { $null } }
    $gpuAtRest = & $gpu
    # The GUI at rest first, for as long as the switches will take: what it costs doing nothing
    $restMs = $Switches * 300
    $c0 = & $cpuNow; Start-Sleep -Milliseconds $restMs; $rest = (& $cpuNow) - $c0
    $times = [Collections.Generic.List[double]]::new()
    $f0 = Get-StateFrame $w
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $c0 = & $cpuNow
    for ($i = 0; $i -lt $Switches; $i++) {
      $to = if ($i % 2 -eq 0) { 1 } else { 0 }
      $key = if ($to -eq 1) { 'Forward' } else { 'Back' }
      $r = Switch-Tab $w $key $to
      $times.Add($r.Ms)
      Start-Sleep -Milliseconds 250
    }
    Wait-Settled -Animating | Out-Null
    $busy = (& $cpuNow) - $c0
    $wall = $clock.Elapsed.TotalMilliseconds
    $frames = (Get-StateFrame $w) - $f0
    # One more, its GPU memory read while the tab left still fades
    Switch-Tab $w Forward 1 | Out-Null
    $gpuFading = & $gpu
    $rested = Wait-Settled -Animating
    # A burst, faster than a hand presses the forward button: 24 presses 33 ms apart, several tabs
    # fading at once; then what's kept at rest once they're done
    $gap = [Diagnostics.Stopwatch]::StartNew()
    for ($k = 0; $k -lt 24; $k++) {
      while ($gap.ElapsedMilliseconds -lt 33 * $k) { Start-Sleep -Milliseconds 1 }
      Send-SideButton $w $mx $my -Forward
    }
    Start-Sleep -Milliseconds 300
    $burst = Wait-Settled -Animating
    Start-Sleep -Milliseconds 500
    $gpuBurst = & $gpu
    $sorted = @($times | Sort-Object)
    $median = $sorted[[int]($sorted.Count / 2)]
    $net = $busy - $rest * $wall / $restMs
    "measure: $Switches switches at slowdown $Slowdown$(if ($Split) { ', tab two split in two' }); key to the state file's frame on the new tab: median $('{0:N1}' -f $median) ms, worst $('{0:N1}' -f $sorted[-1]) ms"
    "measure: the GUI's CPU at rest $('{0:N0}' -f $rest) ms over $('{0:N1}' -f ($restMs / 1000)) s; switching $('{0:N0}' -f $busy) ms over $('{0:N1}' -f ($wall / 1000)) s: $('{0:N1}' -f ($net / $Switches)) ms a switch more than at rest, in $('{0:N1}' -f ($frames / $Switches)) frames a switch"
    if ($null -ne $gpuAtRest) { "measure: the GUI's dedicated GPU memory $('{0:N1}' -f ($gpuAtRest / 1MB)) MB at rest, $('{0:N1}' -f ($gpuFading / 1MB)) MB while a tab left fades, $('{0:N1}' -f ($gpuBurst / 1MB)) MB at rest after a burst of 24 presses 33 ms apart" }
    else { 'measure: no GPU memory counter for the GUI' }
    # An older build's state file has no spare_panes
    "measure: spare pane surfaces at rest $(if ($null -ne $rested.spare_panes) { $rested.spare_panes } else { '(not said)' }) after the switches, $(if ($null -ne $burst.spare_panes) { $burst.spare_panes } else { '(not said)' }) after the burst"
    if ($Slow) {
      # What held the GUI thread, by site, summed over the whole run (the setup's included)
      $lines = @(Get-Content -LiteralPath $w.Log | Where-Object { $_ -match 'lbterm::slow' -and $_ -match ': ([\d.]+) ms$' })
      $sites = @{}
      foreach ($l in $lines) {
        if ($l -match '(?:slow \w+ )(.+): ([\d.]+) ms$') { $sites[$Matches[1]] = [double]$sites[$Matches[1]] + [double]$Matches[2] }
      }
      "measure: the GUI thread over 1 ms, $($lines.Count) times, by site (ms summed):"
      $sites.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 12 | ForEach-Object { "  $('{0,8:N1}' -f $_.Value)  $($_.Key)" }
    }
    return
  }

  $one = Get-Shot $w -Name 'one'
  $state = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $card = @($state.panes)[0].rect
  # Where tab one's blocks are (the left of the pane) and tab two's (the right), short of the
  # rows the moving block uses; the strip's words
  $left = New-Rect ([int]($card.x + 0.08 * $card.w)) ([int]($card.y + 0.15 * $card.h)) ([int](0.25 * $card.w)) ([int](0.3 * $card.h))
  $right = New-Rect ([int]($card.x + 0.6 * $card.w)) ([int]($card.y + 0.15 * $card.h)) ([int](0.25 * $card.w)) ([int](0.3 * $card.h))
  $words = @($state.strip.items | Where-Object name -eq 'words')[0].rect
  $stripRect = if ($words) { New-Rect $words.x $words.y ([Math]::Max($words.w, (& $px 60))) $words.h } else { Get-Rows 0 (& $px 30) -Of $one }
  $lum = { param($shot, $r) [LbtermE2E.Px]::Light($shot, $r) }

  # 1. One to two
  $r = Switch-Tab $w Forward 1 -Frames
  $two = Get-Shot $w -Name 'two'
  $f = $r.First
  "switch: the first frame on tab two came $('{0:N0}' -f $r.Ms) ms after the key; animating $(@($f.animations | ForEach-Object name) -join ', ')"
  if (-not $f) { Add-Failure 'the state file never showed tab two' }
  else {
    if (@($f.animations | ForEach-Object name) -notcontains 'pane.dissolve') { Add-Failure "tab one's panes didn't dissolve: the first frame on tab two animates $(@($f.animations | ForEach-Object name) -join ', ')" }
    $twoPane = @($f.panes)[0]
    if (-not $twoPane -or (Get-CliId $twoPane) -ne $tabs.Panes.two -or $f.active_pane -ne $twoPane.id) { Add-Failure "the keyboard wasn't in tab two's pane on its first frame (active pane $($f.active_pane))" }
    if (@($f.fading_panes) -notcontains $tabs.GuiOne) { Add-Failure "tab one's pane wasn't fading on tab two's first frame (fading $(@($f.fading_panes) -join ', '))" }
  }
  $settled = @($r.States)[-1]
  if (@($settled.fading_panes).Count) { Add-Failure "pane visuals still fading once it settled: $(@($settled.fading_panes) -join ', ')" }
  # Tab one's share of each frame, from each side: (frame - two) / (one - two)
  $lo = @{ L = & $lum $two $left; R = & $lum $one $right }
  $hi = @{ L = & $lum $one $left; R = & $lum $two $right }
  if ($hi.L - $lo.L -lt 40 -or $hi.R - $lo.R -lt 40) { Add-Failure "the tabs' blocks didn't show: left $('{0:N0}' -f $lo.L) to $('{0:N0}' -f $hi.L), right $('{0:N0}' -f $lo.R) to $('{0:N0}' -f $hi.R)" }
  $share = { param($shot)
    $l = ((& $lum $shot $left) - $lo.L) / [Math]::Max(1, $hi.L - $lo.L)
    $rr = ($hi.R - (& $lum $shot $right)) / [Math]::Max(1, $hi.R - $lo.R)
    @($l, $rr) }
  $stripShare = { param($shot)
    $whole = [LbtermE2E.Px]::LumDistance($one, $two, $stripRect)
    if ($whole -lt 0.5) { return $null }
    1 - [LbtermE2E.Px]::LumDistance($one, $shot, $stripRect) / $whole }
  $trace = @($r.Shots | ForEach-Object { $s = & $share $_.Shot; [pscustomobject]@{ Ms = $_.Ms; L = $s[0]; R = $s[1]; Strip = (& $stripShare $_.Shot); Shot = $_.Shot } })
  $mid = @($trace | Where-Object { $_.L -gt 0.05 -and $_.L -lt 0.95 })
  "switch: $($mid.Count) of $($trace.Count) frames on the way; tab one's share, left: $(Format-Trace @($mid | ForEach-Object L)); right: $(Format-Trace @($mid | ForEach-Object R))"
  if ($mid.Count -lt 3) { Add-Failure "the switch cut: $($mid.Count) frames between tab one and tab two" }
  $apart = @($mid | Where-Object { [Math]::Abs($_.L - $_.R) -gt 0.12 })
  if ($apart.Count) { Add-Failure "not a cross-fade: in $($apart.Count) frames the two sides showed different shares of tab one (the old over a blank, or the new fading in on its own)" }
  $inStep = @($mid | Where-Object { $null -ne $_.Strip })
  "switch: the strip's words, tab one's share: $(Format-Trace @($inStep | ForEach-Object Strip))"
  $behind = @($inStep | Where-Object { [Math]::Abs($_.Strip - $_.L) -gt 0.25 })
  if (-not $inStep.Count) { Add-Failure "the strip's words didn't change between the tabs" }
  elseif (@($inStep | Where-Object { $_.Strip -gt 0.05 -and $_.Strip -lt 0.95 }).Count -lt 2) { Add-Failure "the strip's words came at once while the panes faded" }
  # A capture now and then shows the strip's swap visual as neither picture for a frame or two, in
  # main's build before this change too (a same-tab swap as well): PrintWindow's, not the fade's
  elseif ($behind.Count -gt [Math]::Max(2, $inStep.Count / 4)) { Add-Failure "the strip's words and the panes weren't in step in $($behind.Count) of $($inStep.Count) frames" }
  $odd = $inStep | Sort-Object { -[Math]::Abs($_.Strip - $_.L) } | Select-Object -First 1
  if ($odd -and [Math]::Abs($odd.Strip - $odd.L) -gt 0.1) { Save-Shot $odd.Shot 'strip-apart'; "switch: the strip furthest from the panes, $('{0:N2}' -f $odd.Strip) against $('{0:N2}' -f $odd.L) (strip-apart.png)" }
  # Fitted to tab_switch, stretched by the slowdown: the start that fits best, and how far off
  $curve = { param($t) # cubic-bezier(0.23, 1, 0.32, 1): y for x = t
    if ($t -le 0) { return 0.0 }; if ($t -ge 1) { return 1.0 }
    $a = 0.0; $b = 1.0
    for ($k = 0; $k -lt 40; $k++) { $u = ($a + $b) / 2; $x = 3 * (1 - $u) * (1 - $u) * $u * 0.23 + 3 * (1 - $u) * $u * $u * 0.32 + $u * $u * $u; if ($x -lt $t) { $a = $u } else { $b = $u } }
    $u = ($a + $b) / 2; 1 - [Math]::Pow(1 - $u, 3) }
  $whole = 200 * $Slowdown
  $fit = $null
  if ($mid.Count -ge 3) {
    $points = @($trace | Where-Object { $_.L -lt 0.98 })
    $t0s = @(-$whole..0 | Where-Object { $_ % 2 -eq 0 } | ForEach-Object { $mid[0].Ms + $_ })
    $fit = $t0s | ForEach-Object { $t0 = $_
      $sq = 0; foreach ($p in $points) { $d = (1 - (& $curve (($p.Ms - $t0) / $whole))) - ($p.L + $p.R) / 2; $sq += $d * $d }
      [pscustomobject]@{ T0 = $t0; Rms = [Math]::Sqrt($sq / [Math]::Max(1, $points.Count)) } } | Sort-Object Rms | Select-Object -First 1
    "switch: against tab_switch at slowdown ${Slowdown}: rms $('{0:N3}' -f $fit.Rms) of the whole fade over $($points.Count) frames"
    if ($fit.Rms -gt 0.08) { Add-Failure "the fade didn't follow tab_switch: rms $('{0:N3}' -f $fit.Rms)" }
  }
  $half = $mid | Sort-Object { [Math]::Abs($_.L - 0.5) } | Select-Object -First 1
  if ($half) { Save-Shot $half.Shot 'switch' }
  Remove-Frames $r.Shots

  # 2. Back mid-fade, one's block moving along its last row
  Switch-Tab $w Back 0 | Out-Null
  Wait-Settled -Animating | Out-Null
  # The last rows of the pane, under every tab's blocks, where one's moving block runs
  $row = New-Rect ([int]$card.x + 4) ([int]($card.y + 0.8 * $card.h)) ([int]$card.w - 8) ([int](0.2 * $card.h) - 4)
  Send-To one 'tick'
  Start-Sleep -Milliseconds 400
  $blockAt = { param($shot) $box = [LbtermE2E.Px]::BrightBox($shot, $row, 60); if ($box) { $box[0] } else { -1 } }
  $leftAt = & $blockAt (Get-Shot $w)
  if ($leftAt -lt 0) { Add-Failure "tab one's moving block didn't show (searched $row)" }
  $script:turned = $null
  $back = Switch-Tab $w Forward 1 -Frames -Each { param($ms)
    if ($script:turned) { return }
    $s = Get-StateSnapshot $w.StateFile
    $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
    $d = if ($sw) { @($sw.animations | Where-Object name -eq 'pane.dissolve')[0] }
    if ($d -and $d.progress -ge 0.12) { Send-SideButton $w $mx $my; $script:turned = $ms }
  }
  # It went back to one: wait for that
  $atOne = Wait-Change { param($s) $s.active_tab -eq 0 } -What 'back on tab one' -Animating
  Send-To one 'stop'
  $trace = @($back.Shots | ForEach-Object { $s = & $share $_.Shot; [pscustomobject]@{ Ms = $_.Ms; Share = ($s[0] + $s[1]) / 2; Block = (& $blockAt $_.Shot); Strip = (& $stripShare $_.Shot); Shot = $_.Shot } })
  $before = @($trace | Where-Object { $script:turned -and $_.Ms -lt $script:turned })
  $after = @($trace | Where-Object { $script:turned -and $_.Ms -ge $script:turned })
  $coming = @($after | Where-Object { $_.Share -lt 0.97 })
  "back: turned $('{0:N0}' -f $script:turned) ms in; tab one's share before: $(Format-Trace @($before | ForEach-Object Share)); after, on the way: $(Format-Trace @($coming | ForEach-Object Share))"
  "back: the strip's words' share of tab one, before: $(Format-Trace @($before | ForEach-Object Strip)); after: $(Format-Trace @($coming | ForEach-Object Strip))"
  "back: tab one's block at x $leftAt just before; in the frames before the turn at $((@($before | ForEach-Object Block)) -join ' '); after it, on the way, at $((@($coming | ForEach-Object Block)) -join ' ')"
  if (-not $script:turned) { Add-Failure "never turned back: no pane.dissolve 12% through" }
  else {
    $low = (@($before | ForEach-Object Share) | Measure-Object -Minimum).Minimum
    $high = (@($after | Select-Object -First 3 | ForEach-Object Share) | Measure-Object -Maximum).Maximum
    if ($null -eq $low -or $low -gt 0.9) { Add-Failure "tab one never started fading before the turn (its share stayed at $low)" }
    # A restart from the beginning shows tab one whole again at once
    $peakBefore = (@($before | ForEach-Object Share) | Select-Object -Last 2 | Measure-Object -Maximum).Maximum
    # At most +0.06 measured (2026-10-30): a fade starting again from even 0.6 shows
    if ($high -gt $peakBefore + 0.1) { Add-Failure "the switch back jumped: tab one's share went from $('{0:N2}' -f $peakBefore) to $('{0:N2}' -f $high) in the frames after the turn" }
    # The dissolving visual held where the block was; once back, the block is where it is now,
    # a column on every 40 ms
    $ghost = @($before | Where-Object { $_.Block -ge 0 } | ForEach-Object Block | Select-Object -Unique)
    $live = @($coming | Where-Object { $_.Block -ge 0 } | ForEach-Object Block)
    if ($ghost.Count -ne 1) { Add-Failure "tab one's fading visual didn't hold its last frame: its block was at $($ghost -join ', ')" }
    elseif (-not $live.Count) { Add-Failure "tab one's block didn't show while it came back" }
    elseif (@($live | Where-Object { [Math]::Abs($_ - $ghost[0]) -gt (& $px 12) }).Count -lt [Math]::Max(1, $live.Count - 1)) {
      Add-Failure "tab one came back showing its last frame, not what it shows now (its block at $($live -join ', '), left at $($ghost[0]))"
    }
    # The strip's words turn back with the panes
    $apart = @($coming | Where-Object { $null -ne $_.Strip -and [Math]::Abs($_.Strip - $_.Share) -gt 0.25 })
    if ($apart.Count -gt [Math]::Max(2, $coming.Count / 4)) { Add-Failure "the strip's words and the panes turned back apart in $($apart.Count) of $($coming.Count) frames" }
  }
  $turn = $after | Select-Object -First 2 | Select-Object -Last 1
  if ($turn) { Save-Shot $turn.Shot 'back' }
  Remove-Frames $back.Shots
  Start-Sleep -Milliseconds 200

  # 3. Seen again after its fade: as it was
  $again = Get-Shot $w -Name 'one-again'
  $d = [LbtermE2E.Px]::LumDistance($one, $again, $left) + [LbtermE2E.Px]::LumDistance($one, $again, $right)
  if ($d -gt 1) { Add-Failure "tab one, back after the fades, isn't as it was: $('{0:N2}' -f $d) apart (one-again.png)" }
  # This switch with nothing captured, which would paint the window itself: tab one's faded pane
  # is put away by the compositor's own wake 2 ms after its fade ends, with no frame, before the
  # state probe's frame 18 ms after that, whose `prepare` would put it away otherwise
  $awayLine = "put away without a frame: panes \[$($tabs.GuiOne)\]"
  $away = @(Get-LogLines $w $awayLine).Count
  Switch-Tab $w Forward 1 | Out-Null
  Wait-Settled -Animating | Out-Null
  if (Wait-Until { @(Get-LogLines $w $awayLine).Count -gt $away } -TimeoutSec 2) { "again: tab one's pane put away without a frame as its fade ended" }
  else { Add-Failure "tab one's faded pane wasn't put away by the compositor's wake: no new 'put away without a frame' line for pane $($tabs.GuiOne) in $($w.Log)" }
  Switch-Tab $w Back 0 | Out-Null
  $settledOne = Wait-Settled -Animating
  $again = Get-Shot $w -Name 'one-again-settled'
  $d = [LbtermE2E.Px]::LumDistance($one, $again, $left) + [LbtermE2E.Px]::LumDistance($one, $again, $right)
  "again: tab one after leaving it and coming back, $('{0:N2}' -f $d) from how it was; fading $(@($settledOne.fading_panes).Count)"
  if ($d -gt 1) { Add-Failure "tab one, seen again, isn't as it was: $('{0:N2}' -f $d) apart (one-again-settled.png)" }

  # 4. Rapid: five on, round all three
  Switch-Tab $w Forward 1 | Out-Null; Wait-Settled -Animating | Out-Null
  Switch-Tab $w Forward 2 | Out-Null; $three = Wait-Settled -Animating
  $threeShot = Get-Shot $w -Name 'three'
  Switch-Tab $w Forward 0 | Out-Null; Wait-Settled -Animating | Out-Null
  $panesRect = New-Rect ([int]$card.x) ([int]$card.y) ([int]$card.w) ([int](0.6 * $card.h))
  $darkest = @($one, $two, $threeShot | ForEach-Object { & $lum $_ $panesRect }) | Measure-Object -Minimum | ForEach-Object Minimum
  $script:sent = 0
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $frames = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
    if ($script:sent -lt 5 -and $ms -ge 40 * $script:sent) { Send-SideButton $w $mx $my -Forward; $script:sent++ }
  } -Until { param($s) $script:sent -ge 5 -and $s.active_tab -eq 2 -and -not $s.animating }
  $end = Wait-Settled -Animating
  $endShot = Get-Shot $w -Name 'rapid-end'
  $lights = @($frames | ForEach-Object { & $lum $_.Shot $panesRect })
  $dim = ($lights | Measure-Object -Minimum).Minimum
  "rapid: $($script:sent) keys, ended on tab $($end.active_tab) after $('{0:N0}' -f $clock.Elapsed.TotalMilliseconds) ms; the panes' light $(Format-Trace $lights '{0:N0}'), the darkest tab $('{0:N0}' -f $darkest)"
  if ($end.active_tab -ne 2) { Add-Failure "five keys on from tab one ended on tab $($end.active_tab), not three" }
  if (@($end.fading_panes).Count) { Add-Failure "pane visuals still fading after the rapid switches: $(@($end.fading_panes) -join ', ')" }
  # Five tabs fading at once held five surfaces; at rest one spare, as many as a tab here has panes
  "rapid: $($end.spare_panes) spare pane surfaces at rest after"
  if ($null -eq $end.spare_panes) { Add-Failure "the state file doesn't say spare_panes: an older build?" }
  elseif ($end.spare_panes -gt 1) { Add-Failure "$($end.spare_panes) spare pane surfaces kept at rest after the rapid switches, where every tab has one pane: one is enough" }
  if ($dim -lt $darkest - 3) { Add-Failure "a frame on the way was darker than any tab: $('{0:N0}' -f $dim) against $('{0:N0}' -f $darkest)" }
  if ([LbtermE2E.Px]::LumDistance($threeShot, $endShot, $panesRect) -gt 1) { Add-Failure "the rapid switches didn't land on tab three as it shows (rapid-end.png)" }
  $darkFrame = $frames | Sort-Object { & $lum $_.Shot $panesRect } | Select-Object -First 1
  if ($darkFrame) { Save-Shot $darkFrame.Shot 'rapid' }
  Remove-Frames $frames
  Switch-Tab $w Forward 0 | Out-Null; Wait-Settled -Animating | Out-Null
  Switch-Tab $w Forward 1 | Out-Null; Wait-Settled -Animating | Out-Null
  $twoAgain = Get-Shot $w

  # 5. Motion off, in a window of its own: there at once, landing on the same picture
  $off = Open-Tabs 'off'
  $wo = $off.Window
  $r = Switch-Tab $wo Forward 1 -Frames
  $offTwo = Get-Shot $wo -Name 'off-two'
  $between = @($r.Shots | Where-Object { $s = & $share $_.Shot; $s[0] -gt 0.05 -and $s[0] -lt 0.95 })
  $box = Get-DiffBox $twoAgain $offTwo
  "off: $($between.Count) of $($r.Shots.Count) frames between; against the window with motion: $(Format-Box $box)"
  if ($between.Count) { Add-Failure "with motion off the switch took $($between.Count) frames" }
  if ($r.First -and @($r.First.animations | ForEach-Object name) -contains 'pane.dissolve') { Add-Failure 'with motion off the panes still dissolved' }
  if ($box) { Add-Failure "with motion off tab two isn't what it is with motion on: they differ over $(Format-Box $box) (off-two.png, two.png)" }
  Remove-Frames $r.Shots

  # 5b. Motion off with the outline: a fourth tab split in two, the keyboard in its right pane,
  # left for tab three by the back button: the right pane's outline is gone in the next frame, as
  # it always was, with no frame between
  $off.Panes.four = Add-ShownTab -From $off.Panes.one -Program (Stand-In offfour 0 0 0 18)
  Invoke-Cli set-tab-title --pane-id $off.Panes.four four | Out-Null
  Add-ShownSplit $off.Panes.four (Stand-In offfourb 0 0 0 18) | Out-Null
  $offFour = Wait-Change { param($s) $s.active_tab -eq 3 -and @($s.panes).Count -eq 2 } -What 'tab four in two panes, motion off' -Animating
  Start-Sleep -Milliseconds 300
  $offFourShot = Get-Shot $wo -Name 'off-four'
  $offOn = @($offFour.panes | Where-Object active)[0]
  $r = Switch-Tab $wo Back 2 -Frames
  $offThreeShot = Get-Shot $wo -Name 'off-three'
  $offStrips = Find-OutlineStrips $offFourShot $offThreeShot $offOn.rect @(@($r.States)[-1].panes)[0].rect $wo.Scale
  if ($offStrips.Lit.Old -lt 1.5) { Add-Failure "with motion off the right pane's outline didn't show ($('{0:N1}' -f $offStrips.Lit.Old) levels)" }
  else {
    $offLines = @($r.Shots | ForEach-Object { Measure-Outline $_.Shot $offStrips $offFourShot $offThreeShot })
    $between = @($offLines | Where-Object { $_.Old -gt 0.15 -and $_.Old -lt 0.85 })
    "off: the right pane's outline in each frame $(Format-Trace @($offLines | ForEach-Object Old)); tab three's $(Format-Trace @($offLines | ForEach-Object New))"
    if ($between.Count) { Add-Failure "with motion off tab four's outline faded: $($between.Count) frames between all of it and none" }
    if ($offLines[-1].Old -gt 0.1 -or $offLines[-1].New -lt 0.9) { Add-Failure "with motion off the outline didn't land on tab three's ($('{0:N2}' -f $offLines[-1].Old) of four's, $('{0:N2}' -f $offLines[-1].New) of three's)" }
  }
  Remove-Frames $r.Shots
  $offFourShot.Dispose(); $offThreeShot.Dispose()
  Use-TestWindow $w

  # 6. Two panes: tab four, split in two with nothing drawn in either, the new pane on the right
  # with the keyboard; switched from to tab three, whose block lies under both
  $tabs.Panes.four = Add-ShownTab -From $tabs.Panes.one -Program (Stand-In four 0 0 0 18)
  Invoke-Cli set-tab-title --pane-id $tabs.Panes.four four | Out-Null
  Add-ShownSplit $tabs.Panes.four (Stand-In fourb 0 0 0 18) | Out-Null
  $four = Wait-Change { param($s) $s.active_tab -eq 3 -and @($s.panes).Count -eq 2 } -What 'tab four, in two panes' -Animating
  Start-Sleep -Milliseconds 300
  $fourShot = Get-Shot $w -Name 'four'
  $on = @($four.panes | Where-Object active)[0]
  $off = @($four.panes | Where-Object { -not $_.active })[0]
  $strips = $null
  if (-not $on -or -not $off) { Add-Failure "tab four's panes aren't one with the keyboard and one without: $(@($four.panes | ForEach-Object { "$($_.id) active $($_.active)" }) -join '; ')" }
  else {
    # By the mouse's back button, as Lachlan hops tabs
    $r = Switch-Tab $w Back 2 -Frames
    $threeEnd = Get-Shot $w -Name 'three-after-four'
    $threeState = @($r.States)[-1]
    # Tab three's block, and where it lies under each of tab four's panes, a margin in
    $block = [LbtermE2E.Px]::BrightBox($threeEnd, (ConvertTo-Rect @($threeState.panes)[0].rect), 60)
    $inside = { param($p)
      if (-not $block) { return $null }
      $m = & $px 4
      $x0 = [Math]::Max($block[0], $p.rect.x) + $m; $x1 = [Math]::Min($block[2], $p.rect.x + $p.rect.w) - $m
      $y0 = [Math]::Max($block[1], $p.rect.y) + $m; $y1 = [Math]::Min($block[3], $p.rect.y + $p.rect.h) - $m
      if ($x1 - $x0 -lt 6 -or $y1 - $y0 -lt 6) { return $null }
      New-Rect $x0 $y0 ($x1 - $x0) ($y1 - $y0) }
    $rOn = & $inside $on; $rOff = & $inside $off
    if (-not $rOn -or -not $rOff) { Add-Failure "tab three's block doesn't lie under both of tab four's panes (block $($block -join ','))" }
    else {
      # Tab three's share of each frame, under each pane
      $of3 = { param($shot, $rect) ((& $lum $shot $rect) - (& $lum $fourShot $rect)) / [Math]::Max(1, (& $lum $threeEnd $rect) - (& $lum $fourShot $rect)) }
      $trace = @($r.Shots | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; On = (& $of3 $_.Shot $rOn); Off = (& $of3 $_.Shot $rOff); Shot = $_.Shot } })
      $mid = @($trace | Where-Object { [Math]::Max($_.On, $_.Off) -gt 0.03 -and [Math]::Min($_.On, $_.Off) -lt 0.97 })
      "two panes: $($mid.Count) of $($trace.Count) frames on the way; tab three's share under the pane with the keyboard: $(Format-Trace @($mid | ForEach-Object On)); under the one without: $(Format-Trace @($mid | ForEach-Object Off))"
      if ($mid.Count -lt 3) { Add-Failure "the switch from two panes cut: $($mid.Count) frames between tab four and tab three" }
      $apart = @($mid | Where-Object { [Math]::Abs($_.On - $_.Off) -gt 0.12 })
      if ($apart.Count) { Add-Failure "not a cross-fade in both panes: in $($apart.Count) frames the pane without the keyboard showed a different share of tab three than the one with it (the most $('{0:N2}' -f (@($apart | ForEach-Object { [Math]::Abs($_.On - $_.Off) }) | Measure-Object -Maximum).Maximum))" }
      $worst = $mid | Sort-Object { -[Math]::Abs($_.On - $_.Off) } | Select-Object -First 1
      if ($worst) { Save-Shot $worst.Shot 'two-panes'; "two panes: furthest apart $('{0:N2}' -f $worst.On) against $('{0:N2}' -f $worst.Off) at $($worst.Ms) ms (two-panes.png)" }

      # The active outline (Lachlan's 2a, 2026-10-30): the right pane's left side, which tab
      # three's outline doesn't draw, fades with tab four's panes, where until then it went at
      # once; three's own is there at once; the top, which both draw, is never lit twice
      $strips = Find-OutlineStrips $fourShot $threeEnd $on.rect @($threeState.panes)[0].rect $w.Scale
      "outline: lit $('{0:N1}' -f $strips.Lit.Old) levels over the strip on the right pane's left side, $('{0:N1}' -f $strips.Lit.New) on three's left side, $('{0:N1}' -f $strips.Lit.Shared) on the top both draw"
      if ($strips.Lit.Old -lt 1.5 -or $strips.Lit.New -lt 1.5) { Add-Failure "the active outline didn't show where tab four's and tab three's differ: lit $('{0:N1}' -f $strips.Lit.Old) and $('{0:N1}' -f $strips.Lit.New) levels"; $strips = $null }
      else {
        $lines = @($mid | ForEach-Object { $o = Measure-Outline $_.Shot $strips $fourShot $threeEnd; [pscustomobject]@{ Ms = $_.Ms; Panes = 1 - $_.On; Old = $o.Old; New = $o.New; Shared = $o.Shared; Corner = $o.Corner; Shot = $_.Shot } })
        "outline: on the way, tab four's share of its panes $(Format-Trace @($lines | ForEach-Object Panes)); of its outline's own side $(Format-Trace @($lines | ForEach-Object Old)); tab three's outline $(Format-Trace @($lines | ForEach-Object New)); the top both draw, levels over rest $(Format-Trace @($lines | ForEach-Object Shared) '{0:N1}')"
        $apart = @($lines | Where-Object { [Math]::Abs($_.Old - $_.Panes) -gt 0.12 })
        if ($apart.Count) {
          $most = $apart | Sort-Object { -[Math]::Abs($_.Old - $_.Panes) } | Select-Object -First 1
          Save-Shot $most.Shot 'outline-apart'
          Add-Failure "tab four's outline didn't fade with its panes: in $($apart.Count) of $($lines.Count) frames on the way its own side showed another share than they did, the most $('{0:N2}' -f $most.Old) against $('{0:N2}' -f $most.Panes) (outline-apart.png)"
        }
        $late = @($lines | Where-Object { $_.New -lt 0.85 })
        if ($late.Count) { Add-Failure "tab three's outline wasn't there at once: under 85% in $($late.Count) of $($lines.Count) frames, the least $('{0:N2}' -f (@($late | ForEach-Object New) | Measure-Object -Minimum).Minimum)" }
        $twice = @($lines | Where-Object { $_.Shared -gt 1.5 })
        if ($twice.Count) { Add-Failure "the line both tabs' outlines draw was lit twice: up to $('{0:N1}' -f (@($twice | ForEach-Object Shared) | Measure-Object -Maximum).Maximum) levels over its rest in $($twice.Count) frames" }
        Test-CornerOnce $lines 'from four to three'
        if ($lines.Count -ge 3) {
          # The fade's start fitted to the panes, then the outline against tab_switch from it
          $rmsAt = { param($t0, [string]$what) $sq = 0; foreach ($p in $lines) { $d = (1 - (& $curve (($p.Ms - $t0) / $whole))) - $p.$what; $sq += $d * $d }; [Math]::Sqrt($sq / $lines.Count) }
          $t0s = @(-$whole..0 | Where-Object { $_ % 2 -eq 0 } | ForEach-Object { $lines[0].Ms + $_ })
          $t0 = ($t0s | Sort-Object { & $rmsAt $_ 'Panes' } | Select-Object -First 1)
          $fitPanes = & $rmsAt $t0 'Panes'; $fitOld = & $rmsAt $t0 'Old'
          "outline: against tab_switch at slowdown ${Slowdown}, from the start that fits tab four's panes (rms $('{0:N3}' -f $fitPanes)): its outline's own side rms $('{0:N3}' -f $fitOld) of the whole fade over $($lines.Count) frames"
          if ($fitOld -gt 0.08) { Add-Failure "tab four's outline didn't fade on tab_switch with its panes: rms $('{0:N3}' -f $fitOld) from where they started" }
        }
        $half = $lines | Sort-Object { [Math]::Abs($_.Old - 0.5) } | Select-Object -First 1
        if ($half) { Save-Shot $half.Shot 'outline' }
      }
    }
    Remove-Frames $r.Shots

    # 6b. Turned round: back to tab four (the forward button), then from it to three again (the
    # back button) and, a sixth of the way into that fade, back to four by a click on its pad on
    # the rail, as Lachlan does. Four's outline turns round with its panes, in step with them in
    # every frame; three's, there at once, goes from all of it on tab_switch; the top both draw
    # stays as it was
    if ($strips) {
      Switch-Tab $w Forward 3 | Out-Null
      $atFour = Wait-Settled -Animating
      $pad = @(@($atFour.rail.rows) | Where-Object tab_index -eq 3)[0]
      if (-not $pad -or -not $pad.visible) { Add-Failure 'no pad showing on the rail for tab four to click' }
      else {
        $padX, $padY = Get-RectCenter $pad.pad
        $script:turned6 = $null
        $back = Switch-Tab $w Back 2 -Frames -Each { param($ms)
          if ($script:turned6) { return }
          $s = Get-StateSnapshot $w.StateFile
          $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
          $d = if ($sw -and $sw.active_tab -eq 2) { @($sw.animations | Where-Object name -eq 'pane.dissolve')[0] }
          if ($d -and $d.progress -ge 0.15) { Send-Click $w $padX $padY -Move; Send-Leave $w; $script:turned6 = $ms }
        }
        $fourAgain = Wait-Change { param($s) $s.active_tab -eq 3 } -What 'back on tab four' -Animating
        $fourAgainShot = Get-Shot $w -Name 'four-again'
        $turn = @($back.Shots | ForEach-Object { $o = Measure-Outline $_.Shot $strips $fourShot $threeEnd; [pscustomobject]@{ Ms = $_.Ms; Panes = 1 - (& $of3 $_.Shot $rOn); Old = $o.Old; New = $o.New; Shared = $o.Shared; Corner = $o.Corner; Shot = $_.Shot } })
        $before6 = @($turn | Where-Object { $script:turned6 -and $_.Ms -lt $script:turned6 -and $_.Panes -lt 0.97 })
        $after6 = @($turn | Where-Object { $script:turned6 -and $_.Ms -ge $script:turned6 })
        $moving = @($after6 | Where-Object { $_.Panes -lt 0.97 })
        "turned: the click $('{0:N0}' -f $script:turned6) ms in; tab four's panes before $(Format-Trace @($before6 | ForEach-Object Panes)), its outline's own side $(Format-Trace @($before6 | ForEach-Object Old)); after, its panes $(Format-Trace @($moving | ForEach-Object Panes)), its outline $(Format-Trace @($moving | ForEach-Object Old))"
        "turned: tab three's outline before $(Format-Trace @($before6 | ForEach-Object New)), after $(Format-Trace @($after6 | Select-Object -First 12 | ForEach-Object New)); the top both draw, levels over rest $(Format-Trace @($turn | ForEach-Object Shared) '{0:N1}')"
        if (-not $script:turned6) { Add-Failure 'never turned back to tab four: no pane.dissolve 15% through on tab three' }
        elseif (-not $before6.Count) { Add-Failure "tab four never started fading before the click" }
        else {
          $apart = @(@($before6) + @($moving) | Where-Object { [Math]::Abs($_.Old - $_.Panes) -gt 0.12 })
          if ($apart.Count) {
            $most = $apart | Sort-Object { -[Math]::Abs($_.Old - $_.Panes) } | Select-Object -First 1
            Save-Shot $most.Shot 'turned-apart'
            Add-Failure "tab four's outline didn't turn round with its panes: in $($apart.Count) of $($before6.Count + $moving.Count) frames apart by over 0.12, the most $('{0:N2}' -f $most.Old) against $('{0:N2}' -f $most.Panes) (turned-apart.png)"
          }
          # Three's outline, there at once, fades from all of it once the click lands: the curve
          # from 1, its start found between the click and a frame or two after
          $lastNew = @($before6 | ForEach-Object New)[-1]
          if ($lastNew -lt 0.85) { Add-Failure "tab three's outline wasn't all there before the click ($('{0:N2}' -f $lastNew))" }
          $going = @($after6 | Where-Object { $_.Ms -le $script:turned6 + $whole + 300 })
          $t0s = @(0..[int](0.25 * $whole) | Where-Object { $_ % 2 -eq 0 } | ForEach-Object { $script:turned6 + $_ })
          $fitNew = $t0s | ForEach-Object { $t0 = $_
            $sq = 0; foreach ($p in $going) { $d = (1 - (& $curve (($p.Ms - $t0) / $whole))) - $p.New; $sq += $d * $d }
            [pscustomobject]@{ T0 = $t0; Rms = [Math]::Sqrt($sq / [Math]::Max(1, $going.Count)) } } | Sort-Object Rms | Select-Object -First 1
          "turned: tab three's outline against tab_switch from all of it: rms $('{0:N3}' -f $fitNew.Rms) over $($going.Count) frames, starting $('{0:N0}' -f ($fitNew.T0 - $script:turned6)) ms after the click"
          if ($fitNew.Rms -gt 0.1) { Add-Failure "tab three's outline didn't fade from all of it on tab_switch once the click turned the switch round: rms $('{0:N3}' -f $fitNew.Rms)" }
          $end = $turn[-1]
          if ($end.Old -lt 0.9 -or $end.New -gt 0.1) { Add-Failure "after the turn tab four's outline isn't all back ($('{0:N2}' -f $end.Old)) or tab three's isn't gone ($('{0:N2}' -f $end.New))" }
        }
        $moved = @($turn | Where-Object { [Math]::Abs($_.Shared) -gt 1.5 })
        if ($moved.Count) { Add-Failure "the line both tabs' outlines draw changed across the turn: by up to $('{0:N1}' -f (@($moved | ForEach-Object { [Math]::Abs($_.Shared) }) | Measure-Object -Maximum).Maximum) levels in $($moved.Count) frames" }
        Test-CornerOnce $turn 'turned back to four'
        # Its panes as they were before it was left (the rail's timers move on, so only the panes)
        $both = New-Rect ([int]$off.rect.x) ([int]$on.rect.y) ([int]($on.rect.x + $on.rect.w - $off.rect.x)) ([int]$on.rect.h)
        $d = [LbtermE2E.Px]::LumDistance($fourShot, $fourAgainShot, $both)
        if ($d -gt 0.5) { Add-Failure "tab four's panes after the turn aren't as they were: $('{0:N2}' -f $d) apart (four-again.png)" }
        $mid6 = $moving | Sort-Object { [Math]::Abs($_.Old - 0.6) } | Select-Object -First 1
        if ($mid6) { Save-Shot $mid6.Shot 'turned' }
        Remove-Frames $back.Shots
        $fourAgainShot.Dispose()
      }

      # 6c. Closed: a fifth tab of one pane, its block where three's is, closed by its pane going
      # (a session that ends closes its tab the same way), and the window back on tab four. Five's
      # outline along the room's left side, which four's doesn't draw, fades with five's panes as
      # a tab switched from does; four's own side is there at once; the top both draw, never lit
      # twice
      $tabs.Panes.five = Add-ShownTab -From $tabs.Panes.one -Program (Stand-In five 20 70 10 120)
      Invoke-Cli set-tab-title --pane-id $tabs.Panes.five five | Out-Null
      Wait-Change { param($s) $s.active_tab -eq 4 -and @($s.tabs).Count -eq 5 } -What 'tab five' -Animating | Out-Null
      Start-Sleep -Milliseconds 300
      $fiveShot = Get-Shot $w -Name 'five'
      $closing = [Collections.Generic.List[object]]::new()
      $script:killed = $false
      $shots = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
        if (-not $script:killed) { $script:killed = $true; Invoke-Cli kill-pane --pane-id $tabs.Panes.five | Out-Null }
      } -Until { param($s) $closing.Add($s); @($s.tabs).Count -eq 4 -and -not $s.animating }
      $closed = @($closing | Where-Object { @($_.tabs).Count -eq 4 })[0]
      $names = @($closed.animations | ForEach-Object name)
      "close: the first frame after tab five closed is on tab $($closed.active_tab), animating $($names -join ', ')"
      if (-not $closed) { Add-Failure "tab five never closed" }
      elseif ($closed.active_tab -ne 3) { Add-Failure "closing tab five went to tab $($closed.active_tab), not four" }
      elseif ($names -notcontains 'pane.dissolve' -or $names -notcontains 'outline.dissolve') { Add-Failure "tab five's panes and outline didn't both fade as it closed: the first frame after animates $($names -join ', ')" }
      # Five's share of each frame under four's pane with the keyboard, where five's block lies
      $of5 = { param($shot) ((& $lum $shot $rOn) - (& $lum $fourShot $rOn)) / [Math]::Max(1, (& $lum $fiveShot $rOn) - (& $lum $fourShot $rOn)) }
      $close = @($shots | ForEach-Object { $o = Measure-Outline $_.Shot $strips $fourShot $threeEnd; [pscustomobject]@{ Ms = $_.Ms; Panes = (& $of5 $_.Shot); Five = $o.New; Four = $o.Old; Shared = $o.Shared; Corner = $o.Corner; Shot = $_.Shot } })
      $way = @($close | Where-Object { $_.Panes -gt 0.03 -and $_.Panes -lt 0.97 })
      "close: $($way.Count) of $($close.Count) frames on the way; tab five's share of its panes $(Format-Trace @($way | ForEach-Object Panes)); of its outline's left side $(Format-Trace @($way | ForEach-Object Five)); tab four's outline $(Format-Trace @($way | ForEach-Object Four)); the top both draw, levels over rest $(Format-Trace @($way | ForEach-Object Shared) '{0:N1}')"
      if ($way.Count -lt 3) { Add-Failure "the close cut: $($way.Count) frames between tab five and tab four" }
      else {
        $apart = @($way | Where-Object { [Math]::Abs($_.Five - $_.Panes) -gt 0.12 })
        if ($apart.Count) {
          $most = $apart | Sort-Object { -[Math]::Abs($_.Five - $_.Panes) } | Select-Object -First 1
          Save-Shot $most.Shot 'close-apart'
          Add-Failure "tab five's outline didn't fade with its panes as it closed: in $($apart.Count) of $($way.Count) frames apart by over 0.12, the most $('{0:N2}' -f $most.Five) against $('{0:N2}' -f $most.Panes) (close-apart.png)"
        }
        $late = @($way | Where-Object { $_.Four -lt 0.85 })
        if ($late.Count) { Add-Failure "tab four's outline wasn't there at once as tab five closed: under 85% in $($late.Count) of $($way.Count) frames, the least $('{0:N2}' -f (@($late | ForEach-Object Four) | Measure-Object -Minimum).Minimum)" }
        $twice = @($way | Where-Object { $_.Shared -gt 1.5 })
        if ($twice.Count) { Add-Failure "the line both tabs' outlines draw was lit twice as tab five closed: up to $('{0:N1}' -f (@($twice | ForEach-Object Shared) | Measure-Object -Maximum).Maximum) levels over its rest in $($twice.Count) frames" }
        Test-CornerOnce $way 'as tab five closed'
        # The fade's start fitted to five's panes, then its outline against tab_switch from it
        $rms5 = { param($t0, [string]$what) $sq = 0; foreach ($p in $way) { $d = (1 - (& $curve (($p.Ms - $t0) / $whole))) - $p.$what; $sq += $d * $d }; [Math]::Sqrt($sq / $way.Count) }
        $t0s = @(-$whole..0 | Where-Object { $_ % 2 -eq 0 } | ForEach-Object { $way[0].Ms + $_ })
        $t0 = $t0s | Sort-Object { & $rms5 $_ 'Panes' } | Select-Object -First 1
        $fitPanes = & $rms5 $t0 'Panes'; $fitFive = & $rms5 $t0 'Five'
        "close: against tab_switch at slowdown ${Slowdown}, from the start that fits tab five's panes (rms $('{0:N3}' -f $fitPanes)): its outline's left side rms $('{0:N3}' -f $fitFive) over $($way.Count) frames"
        if ($fitFive -gt 0.08) { Add-Failure "tab five's outline didn't fade on tab_switch with its panes as it closed: rms $('{0:N3}' -f $fitFive) from where they started" }
        $half = $way | Sort-Object { [Math]::Abs($_.Five - 0.5) } | Select-Object -First 1
        if ($half) { Save-Shot $half.Shot 'close' }
      }
      $end = $close[-1]
      if ($end -and ($end.Five -gt 0.1 -or $end.Four -lt 0.9)) { Add-Failure "after the close tab five's outline isn't gone ($('{0:N2}' -f $end.Five)) or tab four's isn't all there ($('{0:N2}' -f $end.Four))" }
      Remove-Frames $shots
      $fiveShot.Dispose()

      # 6d. Back past a newer fade: a tab "six" split in two with the keyboard in its left pane;
      # from four to three by the back button, on to six by a click on its pad and back to four by
      # a click on its pad, each a moment into the fade before (three tabs in flight). Three's
      # leftover runs along four's right pane's top, which six's outline doesn't draw: as four's
      # outline comes back up there from where it was, three's goes on from what it showed less
      # that, so the line goes on from what it was and is never lit twice. Kept whole, it was lit
      # by both, jumping up by four's share at the click
      $tabs.Panes.six = Add-ShownTab -From $tabs.Panes.one -Program (Stand-In six 0 0 0 18)
      Invoke-Cli set-tab-title --pane-id $tabs.Panes.six six | Out-Null
      Add-ShownSplit $tabs.Panes.six (Stand-In sixb 0 0 0 18) | Out-Null
      Wait-Change { param($s) $s.active_tab -eq 4 -and @($s.panes).Count -eq 2 } -What 'tab six in two panes' -Animating | Out-Null
      Invoke-Cli activate-pane --pane-id $tabs.Panes.six | Out-Null
      Wait-Change { param($s) $s.active_tab -eq 4 -and (Get-ActiveCliId $s) -eq $tabs.Panes.six } -What "tab six's left pane with the keyboard" -Animating | Out-Null
      Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index 3 | Out-Null
      $atFour = Wait-Change { param($s) $s.active_tab -eq 3 } -What 'back on tab four' -Animating
      $rows = @($atFour.rail.rows)
      $padSix = @($rows | Where-Object tab_index -eq 4)[0]; $padFour = @($rows | Where-Object tab_index -eq 3)[0]
      if (-not $padSix -or -not $padFour -or -not $padSix.visible -or -not $padFour.visible) { Add-Failure 'no pads showing on the rail for tabs four and six to click' }
      else {
        $sixX, $sixY = Get-RectCenter $padSix.pad
        $fourX, $fourY = Get-RectCenter $padFour.pad
        # Six's click a moment (4% of the fade) after the state file shows three, four's as soon
        # as it shows six, while three's outline still shows nearly all of it: kept whole, it and
        # four's coming back would light the top together well over all of one line
        $gaps = @{ 1 = 0.04 * $whole; 2 = 0 }
        $script:run = @{ Step = 0; Seen = $null; Back = $null; Unix0 = $null }
        $landed6 = [Collections.Generic.List[object]]::new()
        $shots = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
          $r6 = $script:run
          if ($r6.Step -eq 0) { $r6.Unix0 = (Get-UnixMs) - $ms; Send-SideButton $w $mx $my; $r6.Step = 1; return }
          if ($r6.Step -ge 3) { return }
          $s = Get-StateSnapshot $w.StateFile
          $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
          if (-not $sw) { return }
          $want = if ($r6.Step -eq 1) { 2 } else { 4 }
          if ($null -eq $r6.Seen -and $sw.active_tab -eq $want) { $r6.Seen = $ms }
          if ($null -ne $r6.Seen -and $ms -ge $r6.Seen + $gaps[$r6.Step]) {
            if ($r6.Step -eq 1) { Send-Click $w $sixX $sixY -Move; Send-Leave $w; $r6.Step = 2; $r6.Seen = $null }
            else { Send-Click $w $fourX $fourY -Move; Send-Leave $w; $r6.Step = 3; $r6.Back = $ms }
          }
        } -Until { param($s, $snap)
          if ($null -ne $script:run.Unix0) { $landed6.Add([pscustomobject]@{ Tab = $s.active_tab; Ms = $snap.time_ms - $script:run.Unix0 }) }
          $script:run.Step -ge 3 -and $s.active_tab -eq 3 -and -not $s.animating }
        $lit = [Math]::Max(0.5, $strips.Lit.Shared)
        $past = @($shots | ForEach-Object { $o = Measure-Outline $_.Shot $strips $fourShot $threeEnd; [pscustomobject]@{ Ms = $_.Ms; Top = (Get-LineLight $_.Shot $strips 'Shared') / $lit; Shared = $o.Shared; Old = $o.Old; Corner = $o.Corner; Shot = $_.Shot } })
        $clicked = $script:run.Back
        # When each switch showed, by the state file's frames: three, six, then four again
        $tThree = @($landed6 | Where-Object Tab -eq 2 | Select-Object -First 1 | ForEach-Object Ms)
        $tSix = @($landed6 | Where-Object { $tThree.Count -and $_.Tab -eq 4 -and $_.Ms -gt $tThree[0] } | Select-Object -First 1 | ForEach-Object Ms)
        $tBack = @($landed6 | Where-Object { $tSix.Count -and $_.Tab -eq 3 -and $_.Ms -gt $tSix[0] } | Select-Object -First 1 | ForEach-Object Ms)
        $pre = @($past | Where-Object { $clicked -and $_.Ms -lt $clicked })
        "past: the click back to four $('{0:N0}' -f $clicked) ms in; three, six and four again shown at $(Format-Trace @($tThree + $tSix + $tBack) '{0:N0}') ms; the right pane's top, before $(Format-Trace @($pre | Select-Object -Last 6 | ForEach-Object Top)), after $(Format-Trace @($past | Where-Object { $clicked -and $_.Ms -ge $clicked } | Select-Object -First 12 | ForEach-Object Top)); four's own side before $(Format-Trace @($pre | Select-Object -Last 6 | ForEach-Object Old)), after $(Format-Trace @($past | Where-Object { $clicked -and $_.Ms -ge $clicked } | Select-Object -First 12 | ForEach-Object Old))"
        if (-not $clicked -or -not $tBack.Count -or $pre.Count -lt 2) { Add-Failure "the run of switches back to four didn't happen as planned (the click at $clicked ms, the switches shown at $(@($tThree + $tSix + $tBack) -join ', ') ms, $($pre.Count) frames before the click)" }
        else {
          # At the click four's own side shows what its fade (from all of it as three showed) had
          # got to, and the right pane's top what three's leftover's had (from all of it as six
          # showed). Each falls to that and rises from it, so no frame may show either under it,
          # whenever its capture was taken: a turn that took light off a line, or an outline gone
          # and back, would. Until 2026-10-31 the click was judged against the last frame
          # captured before it, which trails the screen (45 to 160 ms on a busy PC) and could show
          # it before three's fade had begun, so three's own fade counted against the jump
          $refOld = Get-FadeAt ($tBack[0] - $tThree[0])
          $refTop = Get-FadeAt ($tBack[0] - $tSix[0])
          $lowTop = $past | Sort-Object Top | Select-Object -First 1
          $lowOld = @($past | Where-Object { $_.Ms -ge $tThree[0] }) | Sort-Object Old | Select-Object -First 1
          "past: at the click four's fade had got to $('{0:N2}' -f $refOld) and three's on the top to $('{0:N2}' -f $refTop); the least of each in any frame: the top $('{0:N2}' -f $lowTop.Top) at $($lowTop.Ms) ms, four's side $('{0:N2}' -f $lowOld.Old) at $($lowOld.Ms) ms"
          if ($lowTop.Top -lt $refTop - 0.15) {
            Save-Shot $lowTop.Shot 'past-dip'
            Add-Failure "the right pane's top dipped to $('{0:N2}' -f $lowTop.Top) at $($lowTop.Ms) ms as four came back past three's fade, where three's had got to $('{0:N2}' -f $refTop) at the click (past-dip.png)"
          }
          if ($lowOld -and $lowOld.Old -lt $refOld - 0.15) { Add-Failure "four's own side dipped to $('{0:N2}' -f $lowOld.Old) at $($lowOld.Ms) ms as it came back, where its fade had got to $('{0:N2}' -f $refOld) at the click" }
          $post = @($past | Where-Object { $_.Ms -ge $tBack[0] } | Select-Object -First 2)
          $twice = @($past | Where-Object { $_.Shared -gt 1.5 })
          if ($twice.Count) { Add-Failure "the right pane's top was lit twice as four came back past three's fade: up to $('{0:N1}' -f (@($twice | ForEach-Object Shared) | Measure-Object -Maximum).Maximum) levels over its rest in $($twice.Count) frames" }
          Test-CornerOnce @($past | Where-Object { $_.Ms -ge $clicked - 100 }) 'back to four past three''s fade'
          $last = $past[-1]
          if ($last.Old -lt 0.9 -or [Math]::Abs($last.Shared) -gt 1.5) { Add-Failure "after coming back four's outline isn't as it was: its own side $('{0:N2}' -f $last.Old), the top $('{0:N1}' -f $last.Shared) levels from rest" }
          $at = $post | Select-Object -First 2 | Select-Object -Last 1
          if ($at) { Save-Shot $at.Shot 'past' }
        }
        Remove-Frames $shots
      }

      # 6e. Turned round twice: from four to three by the back button, back to four by a click on
      # its pad 15% into that fade (6b's turn), and back to three by a click on its pad while
      # four's outline still comes up, as soon as four shows and 150 ms after (the second click
      # early, four's outline showing less than three's on the top; and later). The top both
      # draw goes on from what it was through both turns: within 1.5 levels of its rest, either
      # way, in every frame. Until 2026-10-31 the second click took off twice what three's
      # outline showed there, 2.6 and 3.2 of its 7.2 levels in the review's probe (150 ms and
      # 90 ms after), creeping back over 300 ms
      foreach ($gap6e in 0, 150) {
        $atFour6e = Wait-Settled -Animating
        $rows6e = @($atFour6e.rail.rows)
        $padF = @($rows6e | Where-Object tab_index -eq 3)[0]; $padT = @($rows6e | Where-Object tab_index -eq 2)[0]
        if ($atFour6e.active_tab -ne 3 -or -not $padF -or -not $padT -or -not $padF.visible -or -not $padT.visible) { Add-Failure "turned round twice: not on tab four with the pads of three and four showing (on tab $($atFour6e.active_tab + 1))"; break }
        $fX, $fY = Get-RectCenter $padF.pad
        $tX, $tY = Get-RectCenter $padT.pad
        $script:twice = @{ Step = 0; Seen = $null; First = $null; Second = $null }
        $shots = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
          $r6 = $script:twice
          if ($r6.Step -eq 0) { Send-SideButton $w $mx $my; $r6.Step = 1; return }
          if ($r6.Step -ge 3) { return }
          $s = Get-StateSnapshot $w.StateFile
          $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
          if (-not $sw) { return }
          if ($r6.Step -eq 1) {
            $d = if ($sw.active_tab -eq 2) { @($sw.animations | Where-Object name -eq 'pane.dissolve')[0] }
            if ($d -and $d.progress -ge 0.15) { Send-Click $w $fX $fY -Move; Send-Leave $w; $r6.Step = 2; $r6.First = $ms }
            return
          }
          if ($null -eq $r6.Seen -and $sw.active_tab -eq 3) { $r6.Seen = $ms }
          if ($null -ne $r6.Seen -and $ms -ge $r6.Seen + $gap6e) { Send-Click $w $tX $tY -Move; Send-Leave $w; $r6.Step = 3; $r6.Second = $ms }
        } -Until { param($s) $script:twice.Step -ge 3 -and $s.active_tab -eq 2 -and -not $s.animating }
        $r6 = $script:twice
        $lines6e = @($shots | ForEach-Object { $o = Measure-Outline $_.Shot $strips $fourShot $threeEnd; [pscustomobject]@{ Ms = $_.Ms; Old = $o.Old; New = $o.New; Shared = $o.Shared; Corner = $o.Corner; Shot = $_.Shot } })
        "twice, $gap6e ms after four showed: the clicks at $(if ($r6.First) { '{0:N0}' -f $r6.First }) and $(if ($r6.Second) { '{0:N0}' -f $r6.Second }) ms; the top both draw, levels from its rest $(Format-Trace @($lines6e | Select-Object -First 30 | ForEach-Object Shared) '{0:N1}'); four's own side $(Format-Trace @($lines6e | Select-Object -First 30 | ForEach-Object Old)); three's $(Format-Trace @($lines6e | Select-Object -First 30 | ForEach-Object New))"
        if ($r6.Step -lt 3) { Add-Failure "turned round twice ($gap6e ms after four showed): the run didn't happen as planned (step $($r6.Step))" }
        else {
          $off6e = @($lines6e | Where-Object { [Math]::Abs($_.Shared) -gt 1.5 })
          if ($off6e.Count) {
            $most = $off6e | Sort-Object { -[Math]::Abs($_.Shared) } | Select-Object -First 1
            Save-Shot $most.Shot "twice-$gap6e"
            Add-Failure "turned round twice ($gap6e ms after four showed): the top both outlines draw went $('{0:N1}' -f $most.Shared) levels from its rest at $($most.Ms) ms, past 1.5 in $($off6e.Count) of $($lines6e.Count) frames (twice-$gap6e.png)"
          }
          Test-CornerOnce $lines6e "turned round twice, $gap6e ms after four showed"
          $end6e = $lines6e[-1]
          if ($end6e.New -lt 0.9 -or $end6e.Old -gt 0.1) { Add-Failure "turned round twice ($gap6e ms after four showed): it didn't end on three's outline (three's $('{0:N2}' -f $end6e.New), four's $('{0:N2}' -f $end6e.Old))" }
        }
        Remove-Frames $shots
        # Back on four for the next
        Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index 3 | Out-Null
        Wait-Change { param($s) $s.active_tab -eq 3 } -What 'back on tab four' -Animating | Out-Null
      }
    }
  }

  # 7. Three in a row: tab one's title long, so past the end of tab three's a stretch of the strip
  # shows one's words alone; one to two, and on to three a few percent into one's fade
  $wordsThree = @($threeState.strip.items | Where-Object name -eq 'words')[0].rect
  Invoke-Cli set-tab-title --pane-id $tabs.Panes.one 'one one one one one one one one one' | Out-Null
  Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index 0 | Out-Null
  $oneLong = Wait-Change { param($s) $s.active_tab -eq 0 -and $s.strip.title -match '^one one' } -What 'tab one, its title long' -Animating
  Start-Sleep -Milliseconds 200
  $oneShot = Get-Shot $w -Name 'one-long'
  $wordsOne = @($oneLong.strip.items | Where-Object name -eq 'words')[0].rect
  $oneBlock = [LbtermE2E.Px]::BrightBox($oneShot, (ConvertTo-Rect @($oneLong.panes)[0].rect), 150)
  if (-not $wordsOne -or -not $wordsThree -or -not $oneBlock) { Add-Failure "no words in the strip, or no block in tab one, to follow (one's words $($wordsOne | ConvertTo-Json -Compress), three's $($wordsThree | ConvertTo-Json -Compress))" }
  else {
    $sx = [int]($wordsThree.x + $wordsThree.w + (& $px 8))
    $stripOnly = New-Rect $sx ([int]$wordsOne.y) ([int]($wordsOne.x + $wordsOne.w - 2 - $sx)) ([int]$wordsOne.h)
    # Where only tab one draws: the left of its block, under tab three's (rows 2 to 11) and left of it
    $bw = $oneBlock[2] - $oneBlock[0]; $bh = $oneBlock[3] - $oneBlock[1]
    $paneOnly = New-Rect ($oneBlock[0] + 2) ([int]($oneBlock[1] + 0.6 * $bh)) ([int](0.35 * $bw)) ([int](0.4 * $bh) - 2)
    $script:sent7 = 0; $script:second = $null
    $frames = Get-Frames $w ([int](200 * $Slowdown + 2500)) -TailMs 120 -Each { param($ms)
      if ($script:sent7 -eq 0) { Send-SideButton $w $mx $my -Forward; $script:sent7 = 1; return }
      if ($script:sent7 -eq 1) {
        $s = Get-StateSnapshot $w.StateFile
        $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
        $d = if ($sw -and $sw.active_tab -eq 1) { @($sw.animations | Where-Object name -eq 'pane.dissolve')[0] }
        if ($d -and $d.progress -ge 0.06) { Send-SideButton $w $mx $my -Forward; $script:sent7 = 2; $script:second = $ms }
      }
    } -Until { param($s) $script:sent7 -eq 2 -and $s.active_tab -eq 2 -and -not $s.animating }
    $endShot = Get-Shot $w -Name 'three-in-a-row'
    $wholeStrip = [LbtermE2E.Px]::LumDistance($oneShot, $endShot, $stripOnly)
    $ofOne = { param($shot, $rect) ((& $lum $shot $rect) - (& $lum $endShot $rect)) / [Math]::Max(1, (& $lum $oneShot $rect) - (& $lum $endShot $rect)) }
    $trace = @($frames | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Panes = (& $ofOne $_.Shot $paneOnly); Strip = (1 - [LbtermE2E.Px]::LumDistance($oneShot, $_.Shot, $stripOnly) / [Math]::Max(0.5, $wholeStrip)); Shot = $_.Shot } })
    $afterSecond = @($trace | Where-Object { $script:second -and $_.Ms -ge $script:second -and $_.Panes -gt 0.05 -and $_.Panes -lt 0.95 })
    "in a row: the second key $('{0:N0}' -f $script:second) ms in; after it, tab one's share of its panes: $(Format-Trace @($afterSecond | ForEach-Object Panes)); of the strip: $(Format-Trace @($afterSecond | ForEach-Object Strip))"
    if (-not $script:second) { Add-Failure 'the second key never went: no pane.dissolve 6% through on tab two' }
    elseif ($wholeStrip -lt 2) { Add-Failure "tab one's long title and tab three's short one look alike in the strip past three's ($('{0:N2}' -f $wholeStrip))" }
    elseif ($afterSecond.Count -lt 2) { Add-Failure "tab one's panes didn't go on fading after the second key ($($afterSecond.Count) frames)" }
    else {
      $gone = @($afterSecond | Where-Object { [Math]::Abs($_.Strip - $_.Panes) -gt 0.15 })
      # The capture glitch step 1 allows for: a frame or two of the strip's swap read as neither
      if ($gone.Count -gt [Math]::Max(2, $afterSecond.Count / 4)) { Add-Failure "tab one's words left the strip while its panes still showed it: $($gone.Count) of $($afterSecond.Count) frames after the second key apart by more than 0.15" }
      $odd = $afterSecond | Sort-Object { -[Math]::Abs($_.Strip - $_.Panes) } | Select-Object -First 1
      if ($odd) { Save-Shot $odd.Shot 'in-a-row' }
    }
    Remove-Frames $frames
  }

  # 7b. Only the panes wait for a held tab's push (Lachlan's 2b of 2026-11-08): from tab two to
  # tab one by a key (F8, the tab before), tab one's block moving out of view. On a mux domain whose server holds it
  # (its PaneHeld came), the window paints the switch's first frame at once, the rail's ring and
  # bar gliding to tab one's row and the strip's words dissolving into its, while in the panes'
  # place it draws tab two's as they were (the state file's `panes_tab`), none dissolving, until
  # tab one's push has come; that frame draws tab one's panes, tab two's dissolving over them,
  # and the rail's glide is further on than it was, not started again. Locally, and on a server of
  # another build, which holds nothing, the switch's first frame draws tab one's panes. A window of
  # another build (-GuiBin) says no `panes_tab`, and isn't asked
  if (-not $GuiBin) {
    $holds = $script:Mux -and -not $ServerBin
    Invoke-Cli activate-tab --pane-id $tabs.Panes.one --tab-index 1 | Out-Null
    $sw0 = Wait-Change { param($s) $s.active_tab -eq 1 } -What 'tab two' -Animating
    Wait-Settled -Animating | Out-Null
    $oneTab = [long]@($sw0.tabs)[0].id
    $twoTab = [long]@($sw0.tabs)[1].id
    $held0 = Get-Pdus (Get-Metrics $w) 'PaneHeld' 'received'
    Send-To one 'tick'
    if ($holds -and -not (Wait-Until { (Get-Pdus (Get-Metrics $w) 'PaneHeld' 'received') -gt $held0 } -TimeoutSec 5)) {
      Add-Failure "tab one's block moved out of view for 5 s and its server never said it held it (no PaneHeld)"
    }
    # A few of the block's ticks more, held all the while
    Start-Sleep -Milliseconds 200
    Get-Metrics $w -Reset | Out-Null
    $rec = Start-FrameRecorder -Window $w
    try {
      $before = Get-StateFrame $w
      [void]$rec.Take()
      $seen = [Collections.Generic.List[object]]::new()
      $onOne = { param($sw, $s) $sw | Add-Member -NotePropertyName at_ms -NotePropertyValue $s.time_ms -Force; $seen.Add($sw); $sw.active_tab -eq 0 -and $sw.panes_tab -eq $oneTab }.GetNewClosure()
      $t0 = Get-UnixMs
      Send-Key $w F8
      $f = $null
      try { $f = Wait-Recorded -Recorder $rec $onOne -Hwnd ([long]$w.Handle) -AfterFrame $before -TimeoutSec 10 -What "tab one's panes drawn" }
      catch { Add-Failure "after the key to tab one, its panes never drew: $($_.Exception.Message)" }
    } finally { $rec.Dispose() }
    Wait-Settled -Animating | Out-Null
    $fw = (Get-Metrics $w).frame_wait
    $kept = @($seen | Where-Object { $_.active_tab -eq 0 -and $_.panes_tab -ne $oneTab })
    $names = { param($sw) @($sw.animations | ForEach-Object name) }
    $first = if ($f) { $f.Window }
    # Every frame from the key to tab one's panes: its number, when after the key, the open tab,
    # whose panes it drew and what it animates
    "held tab, the frames: $(@($seen | ForEach-Object { "$($_.frame) +$([Math]::Round([double]$_.at_ms - $t0, 1)) ms tab $($_.active_tab) panes of $($_.panes_tab) [$((& $names $_) -join ' ')]" }) -join '; ')"
    "held tab: $($kept.Count) frames on tab one drew tab $(@($kept | ForEach-Object panes_tab | Select-Object -Unique) -join ', ')'s panes before its own$(if ($kept.Count) { ", the first animating $((& $names $kept[0]) -join ', ')" }); the first with its own animating $(if ($first) { (& $names $first) -join ', ' }); $($fw.waits) waits, $($fw.pushed) ended by the push, $($fw.bounded) by the bound, $($fw.held) frames kept, $($fw.max_ms) ms at most"
    # Whether the switch waits is timing, as in step 2: its first paint comes 30 to 50 ms after the
    # key, and the push asked at the switch lands before it (none of five runs of 2026-11-08
    # waited; the two that seemed to were reading other windows' counters, PITFALLS 223). So no
    # wait isn't a failure, and a wait must have drawn its frames as Lachlan's 2b has it.
    # perf/mux-hidden.ps1's busy switches, which always wait, hold it every time
    if ($holds -and [int]$fw.waits -eq 0) {
      if ($kept.Count) { Add-Failure "$($kept.Count) frames on tab one drew another tab's panes, though no switch waited" }
    } elseif ($holds) {
      if ([int]$fw.bounded -gt 0) { Add-Failure "the switch to held tab one waited out the bound ($($fw.max_ms) ms): its push didn't come" }
      # The recorder reads the state file as it changes, so it can miss a frame the next one wrote
      # over (a busy PC): a gap in the frames' numbers says one went unseen, and a wait whose
      # frames all went unseen can't be judged here
      $nums = @($seen | ForEach-Object { [long]$_.frame })
      $gap = $nums.Count -and ($nums[0] -gt $before + 1 -or @(for ($i = 1; $i -lt $nums.Count; $i++) { if ($nums[$i] -gt $nums[$i - 1] + 1) { $i } }).Count)
      if ($kept.Count -lt 1 -and -not $gap) { Add-Failure "the switch to tab one waited for its push ($($fw.max_ms) ms) and no frame drew meanwhile (frames $($before + 1) on, none missed): the window's paint waited with its panes" }
      elseif ($kept.Count -lt 1) { "held tab: the wait's frames went unrecorded (frames after $before, $($nums -join ', ') recorded), so its frames aren't judged here: perf/mux-hidden.ps1's busy switches judge them" }
      foreach ($k in $kept) {
        $rows = @($k.rail.rows)
        $openRow = if ($null -ne $k.rail.open_row) { $rows[[int]$k.rail.open_row] }
        $why = @()
        if ($k.panes_tab -ne $twoTab) { $why += "its panes were tab $($k.panes_tab)'s, not two's, the tab before" }
        if (-not $openRow -or $openRow.tab_index -ne 0) { $why += "the rail's open row wasn't tab one's" }
        if ($k.strip.title -notmatch '^one') { $why += "the strip said '$($k.strip.title)'" }
        if ((& $names $k) -contains 'pane.dissolve') { $why += 'panes dissolved' }
        if ($why) { Add-Failure "frame $($k.frame), on tab one with its panes waiting: $($why -join '; ')" }
      }
      if ($kept.Count -and -not ((& $names $kept[0]) -contains 'rail.marker')) { Add-Failure "the rail's ring and bar didn't glide in the wait's first frame: it animates $((& $names $kept[0]) -join ', ')" }
      if ($kept.Count -and -not ((& $names $kept[0]) -contains 'strip.swap')) { Add-Failure "the strip's words didn't dissolve in the wait's first frame: it animates $((& $names $kept[0]) -join ', ')" }
      if ($first -and -not ((& $names $first) -contains 'pane.dissolve')) { Add-Failure "tab two's panes didn't dissolve as tab one's drew: its first frame animates $((& $names $first) -join ', ')" }
      $m0 = if ($kept.Count) { @($kept[0].animations | Where-Object name -eq 'rail.marker')[0] }
      $m1 = if ($first) { @($first.animations | Where-Object name -eq 'rail.marker')[0] }
      if ($m0 -and $m1 -and [double]$m1.progress -le [double]$m0.progress) { Add-Failure "the rail's glide started again as tab one's panes drew: $($m0.progress) in the wait's first frame, $($m1.progress) after" }
    } else {
      if ($kept.Count) { Add-Failure "$($kept.Count) frames on tab one drew another tab's panes, with nothing held" }
      if ($fw -and [int]$fw.waits -gt 0) { Add-Failure "the switch to tab one waited for a push, with nothing held" }
    }
    Send-To one 'stop'
    Start-Sleep -Milliseconds 300
  }

  # 8. The Conductor's view, in a window of its own whose config names a seat no one answers on
  # (and no hub, and a state file that isn't there: the Conductor off)
  $viewLua = @"
rawset(config, 'lbterm_conductor', {
  state_file = '$(ConvertTo-LuaPath (Join-Path $T.Out 'no-conductor.json'))',
  seat_pipe = [[\\.\pipe\lbterm-perf-noseat-$PID]],
})
table.insert(config.keys, { key = 'F7', mods = 'NONE', action = wezterm.action.LbtermConductorView 'Show' })

"@
  $vt = Open-Tabs 'on' $viewLua -PostedPointer
  $wv = $vt.Window
  $vs = Get-StateWindow (Read-State) -Hwnd ([long]$wv.Handle)
  $vc = @($vs.panes)[0].rect
  $vLeft = New-Rect ([int]($vc.x + 0.08 * $vc.w)) ([int]($vc.y + 0.15 * $vc.h)) ([int](0.25 * $vc.w)) ([int](0.3 * $vc.h))
  $vRight = New-Rect ([int]($vc.x + 0.6 * $vc.w)) ([int]($vc.y + 0.15 * $vc.h)) ([int](0.25 * $vc.w)) ([int](0.3 * $vc.h))
  # Leaving the view for tab $to by $act: the first frame on it has no pane.dissolve, and in no
  # frame does the tab that was under the view show in $hidden, where only it draws
  $leave = { param([scriptblock]$act, [int]$to, $hidden, $viewShot, [string]$how)
    $states = [Collections.Generic.List[object]]::new()
    $script:acted = $false
    $shots = Get-Frames $wv ([int](200 * $Slowdown + 1500)) -TailMs 120 -Each { param($ms) if (-not $script:acted) { & $act; $script:acted = $true } } -Until { param($s) $states.Add($s); $s.active_tab -eq $to -and -not $s.conductor_view -and -not $s.animating }
    $end = Get-Shot $wv
    $first = @($states | Where-Object { $_.active_tab -eq $to })[0]
    $names = @($first.animations | ForEach-Object name)
    $ceiling = [Math]::Max((& $lum $viewShot $hidden), (& $lum $end $hidden)) + 8
    $lights = @($shots | ForEach-Object { & $lum $_.Shot $hidden })
    $peak = ($lights | Measure-Object -Maximum).Maximum
    "view, $($how): the first frame on tab $to animates $($names -join ', '); where the tab under the view drew, $(Format-Trace $lights '{0:N0}') (the view $('{0:N0}' -f (& $lum $viewShot $hidden)), the tab after $('{0:N0}' -f (& $lum $end $hidden)))"
    if (-not $first) { Add-Failure "view, $($how): the state file never showed tab $to" }
    elseif ($names -contains 'pane.dissolve') { Add-Failure "view, $($how): the panes under the Conductor's view dissolved as tab $to opened, though they never showed" }
    if ($peak -gt $ceiling) {
      Add-Failure "view, $($how): the tab under the Conductor's view showed after it left, its light $('{0:N0}' -f $peak) where it drew against at most $('{0:N0}' -f $ceiling) (view-left.png)"
      $bright = $shots | Sort-Object { -(& $lum $_.Shot $hidden) } | Select-Object -First 1
      Save-Shot $bright.Shot 'view-left'
    }
    Remove-Frames $shots
    $end.Dispose() }
  # Over tab one, left for tab two by a key
  Send-Key $wv F7
  Wait-Change { param($s) $s.conductor_view -and $s.active_tab -eq 0 } -What 'the view open over tab one' -Animating | Out-Null
  $viewOne = Get-Shot $wv -Name 'view-over-one'
  if ((& $lum $viewOne $vLeft) -gt 120) { Add-Failure "the view doesn't cover tab one's block: $('{0:N0}' -f (& $lum $viewOne $vLeft)) there (view-over-one.png)" }
  & $leave { Send-Key $wv F9 } 1 $vLeft $viewOne 'a key'
  # Over tab two, left for tab one by a click on its row
  Send-Key $wv F7
  $over = Wait-Change { param($s) $s.conductor_view -and $s.active_tab -eq 1 } -What 'the view open over tab two' -Animating
  $viewTwo = Get-Shot $wv -Name 'view-over-two'
  $row = @(@($over.rail.rows) | Where-Object tab_index -eq 0)[0]
  if (-not $row) { Add-Failure 'no rail row for tab one' }
  else {
    $x, $y = Get-RectCenter $row.pad
    & $leave { Send-Click $wv $x $y -Move; Send-Leave $wv } 0 $vRight $viewTwo 'a click on its row'
  }
  $viewOne.Dispose(); $viewTwo.Dispose()
  # No switch in the first window waited for a held pane's push: none is held on a local domain
  # (frame_wait.rs); on a mux domain whose server holds a tab out of view (one's moving block,
  # step 2), a switch to it waits for its push, which must come before the bound. Another
  # build's window (-GuiBin) has no such wait
  if (-not $GuiBin) {
    $fw = (Get-Metrics $w).frame_wait
    if ($null -eq $fw) { Add-Failure 'the window has no frame_wait counters: a build without the switch wait' }
    elseif (-not $script:Mux -and [int]$fw.waits -gt 0) { Add-Failure "$($fw.waits) switches waited for a held pane's push ($($fw.max_ms) ms at most), with no pane held" }
    elseif ($script:Mux) {
      "frame waits on the mux domain: $($fw.waits) switches waited for a held pane's push, $($fw.pushed) ended by it and $($fw.bounded) by the bound, $($fw.max_ms) ms at most"
      if ([int]$fw.bounded -gt 0) { Add-Failure "$($fw.bounded) of $($fw.waits) switches on the mux domain waited out the bound ($($fw.max_ms) ms at most) for a held pane's push that didn't come" }
      # Whether a switch here waits is timing: step 2 is back on tab one mid-fade, tens of ms
      # after leaving it, and waits only when the server's word of the hold (PaneHeld, after
      # SetShownPanes and the block's next tick) beat that switch's first paint; the fix-up's runs
      # of 2026-11-08 had one wait in one and none in the next. So no wait isn't a failure here:
      # perf/mux-hidden.ps1 judges the wait on switches that always find a held pane. An older
      # server (-ServerBin) holds nothing and no switch may wait
      if ($ServerBin -and [int]$fw.waits -gt 0) { Add-Failure "$($fw.waits) switches waited for a held pane's push on a server of another build, which holds nothing" }
    }
  }

  # 9. Tabs in flight (Lachlan's 3a), in a window of their own (Test-Flight)
  Test-Flight
  Use-TestWindow $w
} -Passed 'tab switch: the old tab fades out over the new on tab_switch with the keyboard there at once, the strip in step, a switch back starts from what shows and the tab comes back with what it shows now, rapid switches land in order with nothing left fading, motion off lands on the same frame, the old tab''s outline fades with its panes as it''s switched from or closed and turns round with them, twice over too, and with four tabs in flight and a switch back to the oldest no tab''s words rise while in flight or step'
