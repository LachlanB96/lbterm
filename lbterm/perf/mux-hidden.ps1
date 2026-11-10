#Requires -Version 7
# Variant: -ServerBin installed
# Variant: -GuiBin installed
# Covers: wezterm-client/src/pane/*.rs, wezterm-client/src/client.rs, wezterm-client/src/lbterm_backlog.rs, wezterm-client/src/lbterm_shown.rs, wezterm-client/src/domain.rs, wezterm-mux-server-impl/src/lbterm_flow.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-gui/src/termwindow/frame_wait.rs, lbterm-extras/src/frame_wait.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/overlay/copy.rs, wezterm-gui/src/overlay/quickselect.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/budget.rs, wezterm-gui/src/termwindow/hidden_scan.rs, lbterm-extras/src/hidden_scan.rs, wezterm-gui/src/termwindow/render/pane.rs
<#
What tabs out of view cost a window on a mux domain, what a pane in view costs in polls, and
whether a switch to a busy tab shows its screen at once (lbterm/MUX.md, "Tabs out of view").
Every tab in a test mux server of its own (a socket under the test's folder; never Lachlan's), the
window drawn as Lachlan's (New-TestConfig, his hyperlink rules included) without the scrollbar's
and the scroll strip's prompt marks, which read the whole scrollback of the tab in view.

-Busy tabs out of view each stand in for a working Claude Code session: on the alternate screen,
a line of its answer every -EveryMs scrolling the conversation (each line with a URL and a path
in it, as an answer has, for the hyperlink rules to look at) and its status block and spinner
redrawn. A tab in view, quiet, then printing a line every 100 ms.

Each busy tab puts its cursor, after each line, where it says how many lines it has written (row
and column: 100 a row), and writes the count to a file of its own, so a frame's drawn cursor says
which of its screens the frame drew. A server of this build holds a tab's lines while the window
doesn't show it (lbterm/MUX.md, "Tabs out of view: output held at the server"): the window says
what it shows (SetShownPanes) and the server tells it what it holds (PaneHeld, in its counters).

1. quiet: the busy tabs wait, for -QuietSec: the window's GUI thread, its baseline.
2. hidden: the busy tabs work out of view, for -Seconds: the GUI thread, and the pushes of their
   changes it took (the window's counters: lbterm/PERF.md), so what each push cost it. On a
   server that holds them (PaneHeld came), the pushes a second instead, which should be none:
   no busy tab is ever quiet for the 250 ms that would bring its lines.
2b. names: each busy tab in turn, still working out of view, told by a file of its own to set its
   title, its claude_session user var and its OSC 9;4 progress to a word of its own and to ring
   the bell: the time from the file to the window having all three (the title as its own socket's
   `cli list` gives the pane, the progress on its tab's row of the rail, the user var as Lua's
   user-var-changed hears it, logged; the state file's `panes` are the open tab's alone, so it
   can't say them), the bell counted as the NotifyAlerts the window took, which the server
   sends at once whatever it holds (a push of the title alone, and its alerts).
3. busy switches: each busy tab in turn brought into view by a key while they all work, and
   back: the time from the key to the first frame that draws its pane, and the window's own time
   for it (its counters' tab action), with the other tabs' pushes still coming; the screen that
   frame drew against the one the tab had written at the key (its drawn cursor against the
   count in its file), which must be that one or the one before; and the frames that waited for
   the tab's push, how many and how long (the counters' frame_wait).
4. polls: the tab in view prints a line every 100 ms for -PollSec: the GetPaneRenderChanges the
   window sent (its counters), which a unix domain's pushes leave nothing to find.
6. two windows: F12 opens a second window of the same GUI on the domain (SpawnCommandInNewWindow,
   on the test screen) with a busy tab of its own, which the first window doesn't have, while the
   first shows the ticker: for -WindowSec, every frame of the second window drawing its tab must
   draw the screen its program had written as the frame came (its drawn cursor against its file,
   at most a line behind, as in 3), and on a server that holds, the pushes the GUI took a second
   are no more than that one tab's lines (-EveryMs) and -MaxHeldPushes: the set the windows say
   is both windows' tabs, and the busy tabs neither shows stay held. Then the second window's tab
   is closed, and the window with it.
7. minimized: the first window on busy tab 1 is minimized (SW_SHOWMINNOACTIVE) for -WindowSec:
   it still shows that tab (lbterm/MUX.md: nothing is said at minimize), so on a server that
   holds the pushes the GUI took a second are no more than that tab's lines and -MaxHeldPushes;
   then restored without activating it, and its first frame must draw tab 1 at most a line
   behind the screen written at the restore, without waiting for a push (frame_wait).
8. reconnect (PITFALLS 201): a new busy tab prints out of view (on a server that holds, it's
   held: a PaneHeld must come), then finishes and writes the file the test server hangs up for
   (LBTERM_TEST_HANGUP, in the window's environment and so the server's), which it does within
   200 ms, before the hold's 250 ms quiet push. Once the window's log says Reconnected!, the
   switch to the tab must draw its final screen in its first frame, and no frame wait may end by
   the bound: the window asks a held pane's push of the new connection at the reattach, which
   holds and owes nothing for it. Step 8 ends the connection on purpose, so the connection check
   below reads the log to its start.

Each pairing with another build (PITFALLS 159): -ServerBin runs another build's
lbterm-mux-server.exe for the domain and -GuiBin another build's lbterm-gui.exe for the window
(installed: a copy of the daily LBTerm's in the output folder, as perf/mux-resize.ps1 makes it).
Whichever builds run, the window's log to step 8 must say no reconnect and no domain detached (a window
ends its connection on a PDU it doesn't know, PITFALLS 188), and the window's counters say what
was said: this build's window says what it shows (SetShownPanes) at its first frame, and on a
server that answers ErrorResponse (one that doesn't know the PDU, lbterm/MUX.md's "Each pairing
with an older build") says it no more on that connection, its switches sending none and no
PaneHeld coming, every tab pushed as before; a window of another build (-GuiBin) sends none and
takes no PaneHeld, every tab pushed to it as before. Whether the server held is the PaneHeld the
window took. The pairings are judged on what's right (the screens drawn, the names, the
switches, the links, the lines fetched, the PDUs, the connection), and the costs (-PushUs,
-MaxPolls, -BusySwitchMs) on this build's pair alone.
5. switch: every busy tab stops on a screen of its own, every line a URL and a path, its cursor
   somewhere of its own, out of view; then, once they've been quiet 2 s (a tab out of view that
   stops has its screen scanned for links 250 ms on, lbterm_extras::hidden_scan), each in turn is
   brought into view by a key (ActivateTab) and back: the time from the key to the first frame
   that draws it with its cursor where its program left it (the state file's drawn_cursor, frame
   by frame), any frame before that drawing it elsewhere (a stale screen), the window's own time
   and its GUI thread's for the switch, the lines the window then holds of it against what it
   wrote, whether its URLs are links there (`cli get-text --escapes` from the window's own
   socket: OSC 8), the lines the window had to fetch for it (its GetLines), and the frames that
   waited for a push (none: a tab quiet out of view has had its quiet push before the switch).

It fails, unless -Measure, when a push of a tab out of view costs the GUI thread over -PushUs
microseconds at the median of the -Busy tabs' (the hidden span's GUI thread over the quiet's,
over the pushes taken), when the tab in view printing brings over -MaxPolls polls, when a switch
shows a stale screen or the wrong lines, fetches lines, leaves its URLs without links, or takes
over -SwitchMs at the median from the key to its first right frame, or when a switch to a tab at
work takes over -BusySwitchMs at the median to draw it. -Compat (a window too old for counters)
runs and judges the switches alone (5). mux-hidden.json in the output folder has every number,
and the window's counters over the hidden span and the polls (hidden-metrics.json,
polls-metrics.json).

  pwsh -File lbterm\perf\mux-hidden.ps1 [-Busy 6] [-Seconds 10] [-EveryMs 100] [-WindowSec 3] [-Measure] [-Compat] [-ServerBin <build>|installed] [-GuiBin <build>|installed] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$WindowSec = 3,
  [int]$Busy = 6,
  [int]$Seconds = 10,
  [int]$QuietSec = 3,
  [int]$PollSec = 5,
  [int]$EveryMs = 100,
  [int]$Cols = 120,
  [int]$Rows = 40,
  [int]$Scrollback = 5000,
  [switch]$Measure,
  [switch]$Compat,
  # What the window's log takes besides its own (WEZTERM_LOG's form: `lbterm::shown=debug` has the
  # switch's waits)
  [string]$Log,
  [double]$MaxMemGB = 2,
  # Bounds: the GUI thread a push of a tab out of view, in µs; the polls while the tab in view
  # prints; a switch's key to its first right frame, the median, in ms; a switch to a tab at work,
  # its key to the first frame drawing it, the median, in ms
  [double]$PushUs = 400,
  [int]$MaxPolls = 12,
  [double]$SwitchMs = 60,
  [double]$BusySwitchMs = 80,
  # A server that holds: the busy tabs' pushes a second out of view, at most; a name's way to the
  # state file, at most, in ms
  [double]$MaxHeldPushes = 1,
  [double]$NamesMs = 1000,
  [string]$Lua = ''
)
$ErrorActionPreference = 'Stop'
if ($Busy -lt 1 -or $Busy -gt 9) { throw "-Busy is 1 to 9: F2 to F10 bring each into view, the next F key step 8's tab and F12 step 6's window" }
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-hidden' -Bin $Bin
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
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$guiExe = if ($GuiBin) { Join-Path (Resolve-Path $GuiBin).Path 'lbterm-gui.exe' } else { $T.Gui }
if (-not (Test-Path $guiExe)) { throw "no lbterm-gui.exe in $GuiBin" }
# Which pairing: this build's pair, this window on another build's server, or another build's
# window on this server; the costs are judged on this build's pair alone
$Pairing = if ($GuiBin -and $ServerBin) { 'other window, other server' } elseif ($GuiBin) { 'other window' } elseif ($ServerBin) { 'other server' } else { 'this build' }
$OwnPair = $Pairing -eq 'this build'
$rate = Get-CycleRate

$Event = "Local\lbterm-mux-hidden-$PID"
$StopFile = Join-Path $T.Out 'stop'
# The file the test server hangs up on its window for when it appears (LBTERM_TEST_HANGUP, as
# perf/mux-reconnect.ps1 has it): step 8's tab writes it as it finishes
$HangupFile = Join-Path $T.Out 'hangup'
Remove-Item -LiteralPath $HangupFile -ErrorAction SilentlyContinue
Remove-Item $StopFile -ErrorAction SilentlyContinue

# What busy tab $Tag ends on: its lines and its cursor (row and column from 1)
function Get-FinalLine([int]$Tag, [int]$Row) { "final $Tag row $Row https://example.com/hidden/$Tag/$Row and C:/work/tab-$Tag/notes-$Row.md end" }
function Get-FinalCursor([int]$Tag) { @{ Row = 3 + $Tag; Col = 10 + 2 * $Tag } }
$FinalRows = $Rows - 4
# Where busy tab $Tag is told to set its names, and where it says how many lines it has written
function Get-NamesFile([int]$Tag) { Join-Path $T.Out "names-$Tag" }
function Get-CountFile([int]$Tag) { Join-Path $T.Out "count-$Tag" }
# The busy tab's lines written, from its file: the count, and the rows its cursor goes round
function Read-Count([int]$Tag) {
  $raw = Get-Content -LiteralPath (Get-CountFile $Tag) -Raw -ErrorAction SilentlyContinue
  if (-not $raw) { return $null }
  $p = $raw.Trim() -split ' '
  @{ Count = [int]$p[0]; Area = [int]$p[1] }
}
# How many lines a frame that drew the busy tab's cursor at $Cursor (from 0) is behind $Count
function Get-Lag($Cursor, $Count) {
  $cycle = 100 * $Count.Area
  $drawn = [int]$Cursor.y * 100 + [int]$Cursor.x
  $lag = (($Count.Count % $cycle) - $drawn + $cycle) % $cycle
  if ($lag -gt $cycle / 2) { 0 } else { $lag }
}

# A stand-in for a working Claude Code session, out of view: waits for the event, then on the
# alternate screen a line of its answer every -EveryMs scrolling the conversation, its status block
# and spinner redrawn with it; once the stop file is there, its final screen. $Go and $Stop: its own
# event and stop file (step 8's); $Hangup: a file it writes 30 ms after its final screen, which the
# test server hangs up for within 200 ms (dispatch.rs's hang-up tick), under the 250 ms a hold
# waits for quiet, so the hold outlives the connection that said it (step 8)
function New-BusyProgram([int]$Tag, [string]$Go = $Event, [string]$Stop = $StopFile, [string]$Hangup = '') {
  $c = Get-FinalCursor $Tag
  New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$h = [Console]::WindowHeight; $w = [Console]::WindowWidth
$area = [Math]::Max(4, $h - 8)
$go = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, 'XEVENTX')
[void]$go.WaitOne()
[Console]::Out.Write("$e[?1049h$e[2J$e[1;${area}r")
$spin = [char[]]@(0xB7, 0x2722, 0x2733, 0x2736, 0x273B, 0x273D)
$words = 'the window keeps what a tab out of view prints and lays it out once it is drawn' -split ' '
$clock = [Diagnostics.Stopwatch]::StartNew()
$rule = [string][char]0x2500 * $w
for ($i = 0; -not (Test-Path -LiteralPath 'XSTOPX'); $i++) {
  $sb = [Text.StringBuilder]::new()
  if (Test-Path -LiteralPath 'XNAMESX') {
    $n = [IO.File]::ReadAllText('XNAMESX').Trim()
    Remove-Item -LiteralPath 'XNAMESX'
    $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($n))
    [Console]::Out.Write("$e]2;$n$e\$e]1337;SetUserVar=claude_session=$b64$([char]7)$e]9;4;1;42$e\$([char]7)$e]7501;state=working:progress=42$e\")
  }
  $line = "  see https://example.com/answer/TAG/$i and C:/work/tab-TAG/file-$i.md:"
  for ($j = 0; $line.Length -lt $w - 14; $j++) { $line += ' ' + $words[($i + $j) % $words.Count] }
  [void]$sb.Append("$e[$area;1H`n$line")
  $row = $area + 2
  [void]$sb.Append("$e[$row;1H$e[38;5;174m$($spin[$i % 6]) Working$([char]0x2026) $e[38;5;246m($([int]$clock.Elapsed.TotalSeconds)s $([char]0xB7) $([char]0x2193) $($i * 7) tokens)$e[0m$e[K")
  [void]$sb.Append("$e[$($row + 1);1H$e[38;5;240m$rule$e[0m$e[$($row + 2);1H$([char]0x276F) $e[K$e[$($row + 3);1H$e[38;5;240m$rule$e[0m")
  # The cursor says which screen this is: line $i at row 1 + (i / 100) % area, column 1 + i % 100
  [void]$sb.Append("$e[$(1 + [Math]::Floor($i / 100) % $area);$(1 + $i % 100)H")
  [Console]::Out.Write($sb.ToString())
  [IO.File]::WriteAllText('XCOUNTX', "$i $area")
  [Threading.Thread]::Sleep(XEVERYX)
}
$sb = [Text.StringBuilder]::new()
[void]$sb.Append("$e[r$e[2J$e[H")
for ($r = 1; $r -le XROWSX; $r++) { [void]$sb.Append("$e[$r;1Hfinal TAG row $r https://example.com/hidden/TAG/$r and C:/work/tab-TAG/notes-$r.md end") }
[void]$sb.Append("$e[XCROWX;XCCOLXH")
[Console]::Out.Write($sb.ToString())
if ('XHANGUPX') { [Threading.Thread]::Sleep(30); [IO.File]::WriteAllText('XHANGUPX', 'now') }
Start-Sleep 900
'@ -creplace 'XHANGUPX', $Hangup.Replace('$', '$$') -creplace 'XEVENTX', $Go -creplace 'XEVERYX', $EveryMs -creplace 'XROWSX', $FinalRows -creplace 'XCROWX', $c.Row -creplace 'XCCOLX', $c.Col -creplace 'XNAMESX', (Get-NamesFile $Tag).Replace('$', '$$') -creplace 'XCOUNTX', (Get-CountFile $Tag).Replace('$', '$$') -creplace 'TAG', $Tag -creplace 'XSTOPX', $Stop.Replace('$', '$$'))
}

# The tab in view: quiet until its own event, then a line every 100 ms for -PollSec
$Ticker = New-PwshProgram -Script (@'
$go = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, 'XEVENTX-ticker')
[Console]::Out.Write('ready> ')
[void]$go.WaitOne()
for ($i = 0; $i -lt XTICKSX; $i++) { [Console]::Out.WriteLine("tick $i https://example.com/tick/$i"); [Threading.Thread]::Sleep(100) }
[Console]::Out.Write('done> ')
Start-Sleep 900
'@ -replace 'XEVENTX', $Event -replace 'XTICKSX', ($PollSec * 10))

$Handles = @()
function Set-Event([string]$Name) {
  $ev = [Threading.EventWaitHandle]::new($false, [Threading.EventResetMode]::ManualReset, $Name)
  [void]$ev.Set()
  # Kept until the test ends, so a program that hasn't opened it yet still finds it set
  $script:Handles += $ev
}

function New-HiddenConfig {
  $cfg = Join-Path $T.Out 'config\mux-hidden.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  $keys = @('F1') + @(2..12 | ForEach-Object { "F$_" })
  $bind = (0..($Busy + 1) | ForEach-Object { "  { key = '$($keys[$_])', mods = 'NONE', action = wezterm.action.ActivateTab($_) }," }) -join "`n"
  # F12 (F24 never reached its binding, 2026-11-08; the tabs' keys stop at F11): a second window of the same GUI on the domain, on the test screen beside the first, with
  # a busy tab of its own (6. two windows)
  $posX, $posY = (& (Get-Module harness) { $TestWindowPosition }) -split ',' | ForEach-Object { [int]$_ }
  $second = ((New-BusyProgram ($Busy + 1)) | ForEach-Object { "'$($_ -replace '\\', '/')'" }) -join ', '
  $bind += "`n  { key = 'F12', mods = 'NONE', action = wezterm.action.SpawnCommandInNewWindow { domain = { DomainName = 'hiddenmux' }, args = { $second }, position = { x = $($posX + 60), y = $($posY + 60), origin = 'ScreenCoordinateSystem' } } },"
  New-TestConfig -Path $cfg -Lua @"
-- Each user var a pane sets, as Lua's events hear it, in the window's log (2b. names)
wezterm.on('user-var-changed', function(window, pane, name, value)
  wezterm.log_error('mux-hidden user var ' .. name .. '=' .. value)
end)
config.initial_cols = $Cols
config.initial_rows = $Rows
config.scrollback_lines = $Scrollback
config.enable_scroll_bar = false
rawset(config, 'lbterm_scroll_strip', nil)
config.keys = config.keys or {}
for _, k in ipairs({
$bind
}) do table.insert(config.keys, k) end
$Lua
config.unix_domains = { {
  name = 'hiddenmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  $cfg
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

# The GUI thread's milliseconds a second between two snapshots
function Get-ThreadRate($a, $b) {
  $s = ($b.At - $a.At) / [Diagnostics.Stopwatch]::Frequency
  [ordered]@{ Seconds = [Math]::Round($s, 2); GuiThreadMsPerSec = [Math]::Round(($b.ThreadCycles - $a.ThreadCycles) / $rate / $s, 1); GuiMsPerSec = [Math]::Round(($b.Cycles - $a.Cycles) / $rate / $s, 1) }
}

# A PDU's count in a metrics reply, sent or received
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}

$results = [ordered]@{ Window = $guiExe; Server = $serverExe; Pairing = $Pairing; Busy = $Busy; EveryMs = $EveryMs; Compat = [bool]$Compat; Locked = (Test-WorkstationLocked); Quiet = $null; Hidden = $null; BusySwitches = $null; Polls = $null; TwoWindows = $null; Minimized = $null; Switches = $null; Said = $null; Connection = $null }

# The window's log lines since $From (its length then) that say its connection ended: a reconnect,
# or the domain detached (wezterm-client's client.rs and domain.rs)
function Get-ConnectionLines($w, [long]$From) {
  if (-not $w.Log -or -not (Test-Path -LiteralPath $w.Log)) { return @() }
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  try {
    [void]$fs.Seek($From, 'Begin')
    $text = [IO.StreamReader]::new($fs).ReadToEnd()
  } finally { $fs.Dispose() }
  @($text -split "`r?`n" | Where-Object { $_ -match 'the connection to the mux server ended|client disconnected|Reconnected!|reattaching after a reconnect|detached domain' })
}
# Whether the window's log says its server doesn't know SetShownPanes (an older build: lbterm_shown.rs
# logs it once a connection, at info)
function Test-ServerLacks($w) {
  if (-not $w.Log -or -not (Test-Path -LiteralPath $w.Log)) { return $false }
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  $text = try { [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  $text -match "the mux server doesn't know SetShownPanes"
}

# The window's log, read from $From for Lua's user var lines (2b): whether one says $Value
function Test-UserVarLogged($w, [long]$From, [string]$Value) {
  if (-not $w.Log -or -not (Test-Path -LiteralPath $w.Log)) { return $false }
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  try {
    [void]$fs.Seek($From, 'Begin')
    $text = [IO.StreamReader]::new($fs).ReadToEnd()
  } finally { $fs.Dispose() }
  $text.Contains("mux-hidden user var claude_session=$Value")
}
# Whether the window's log from $From (its length then) has $Text
function Test-LogSince($w, [long]$From, [string]$Text) {
  if (-not $w.Log -or -not (Test-Path -LiteralPath $w.Log)) { return $false }
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  try {
    [void]$fs.Seek($From, 'Begin')
    $all = [IO.StreamReader]::new($fs).ReadToEnd()
  } finally { $fs.Dispose() }
  $all.Contains($Text)
}
function Get-LogLength($w) { if ($w.Log -and (Test-Path -LiteralPath $w.Log)) { (Get-Item -LiteralPath $w.Log).Length } else { 0 } }

Invoke-Test {
  $logOpen = if ($Log) { @{ Log = "info,$Log" } } else { @{} }
  $w = Open-TestWindow @logOpen -ConfigFile (New-HiddenConfig) -Domain 'hiddenmux' -Socket $T.Sock -TimeoutSec 30 -Exe $guiExe -Environment @{ LBTERM_TEST_HANGUP = $HangupFile }
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  Start-MemoryGuard -Pids $w.Pid, ([int]$server[0].ProcessId) -LimitGB $MaxMemGB
  $shell = $panes[0].pane_id
  $ticker = New-Tab -From $shell -Program $Ticker -Server
  $busyIds = @()
  for ($i = 1; $i -le $Busy; $i++) { $busyIds += New-Tab -From $shell -Program (New-BusyProgram $i) -NoActivate -Server }
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  $want = 1 + $Busy
  Wait-State { param($s) @($s.tabs).Count -eq $want } -What "the $want tabs in the window" -TimeoutSec 30 | Out-Null
  Invoke-Cli activate-pane --pane-id $ticker -Server -Check | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $ticker }).Count -eq 1 -and -not $s.animating } -What 'the ticker in view' -TimeoutSec 15 | Out-Null
  $results.Build = (Read-State).build
  $hasMetrics = $true
  # What the window said and was told from its start to here, before anything resets its
  # counters: SetShownPanes at its attach and each tab made, and the answer an older server gives
  $setup = $null
  try { $setup = Get-Metrics $w -Reset -TimeoutMs 2000 } catch { $hasMetrics = $false }
  if (-not $hasMetrics -and -not $Compat) { throw "the window has no counters (lbterm/PERF.md): a build too old for this test" }
  if ($setup) {
    $results.Said = [ordered]@{
      SetupShown = Get-Pdus $setup 'SetShownPanes' 'sent'
      SetupErrors = Get-Pdus $setup 'ErrorResponse' 'received'
      # The window's own word that its server doesn't know SetShownPanes (wezterm-client's
      # lbterm_shown.rs): an ErrorResponse alone says nothing, since every build answers the
      # harness's kill of the start's shell with one ("no such pane 0")
      SetupLacks = Test-ServerLacks $w
      SetupHeld = Get-Pdus $setup 'PaneHeld' 'received'
      SwitchShown = 0
    }
    "said at the start ($Pairing): $($results.Said.SetupShown) SetShownPanes sent, $($results.Said.SetupErrors) ErrorResponse$(if ($results.Said.SetupLacks) { ' (the server doesn''t know SetShownPanes)' }) and $($results.Said.SetupHeld) PaneHeld taken"
  }
  Start-Sleep 2

  if (-not $Compat) {
    # 1. quiet
    $a = Get-CpuSnapshot $w
    Start-Sleep $QuietSec
    $b = Get-CpuSnapshot $w
    $results.Quiet = Get-ThreadRate $a $b
    "quiet ($($results.Quiet.Seconds) s, the busy tabs waiting): the GUI thread $($results.Quiet.GuiThreadMsPerSec) ms/s, the window $($results.Quiet.GuiMsPerSec) ms/s"
  }

  # 2. hidden. The holds start as the tabs do, each its PaneHeld then: counted from the go, though
  # the span's own figures start a second after it (a PaneHeld comes once a hold, and a hold
  # lasts while its tab prints, so the span alone sees none)
  if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
  Set-Event $Event
  Start-Sleep 1
  $go = if ($hasMetrics) { Get-Metrics $w -Reset }
  $a = Get-CpuSnapshot $w
  Start-Sleep $Seconds
  $b = Get-CpuSnapshot $w
  if (-not $Compat) {
    $h = Get-ThreadRate $a $b
    $m = Get-Metrics $w
    $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out 'hidden-metrics.json')
    $h.PushesPerSec = [Math]::Round((Get-Pdus $m 'GetPaneRenderChangesResponse' 'received') / $h.Seconds, 1)
    $h.GetLinesPerSec = [Math]::Round((Get-Pdus $m 'GetLines' 'sent') / $h.Seconds, 1)
    $h.PollsPerSec = [Math]::Round((Get-Pdus $m 'GetPaneRenderChanges' 'sent') / $h.Seconds, 1)
    $h.Frames = $m.totals.frames
    # A server that holds tells the window what it holds; one that doesn't (another build's)
    # pushes every change
    $h.PaneHeld = $(if ($go) { Get-Pdus $go 'PaneHeld' 'received' } else { 0 }) + (Get-Pdus $m 'PaneHeld' 'received')
    $h.Holds = $h.PaneHeld -gt 0
    $over = $h.GuiThreadMsPerSec - $results.Quiet.GuiThreadMsPerSec
    $h.PushUs = if ($h.PushesPerSec -gt 0) { [Math]::Round($over * 1000 / $h.PushesPerSec, 1) } else { $null }
    $results.Hidden = $h
    "hidden ($($h.Seconds) s, $Busy tabs working out of view): the GUI thread $($h.GuiThreadMsPerSec) ms/s, $($h.PushesPerSec) pushes/s taken, $($h.PushUs) us of the GUI thread a push over the quiet span; $($h.Frames) frames, $($h.GetLinesPerSec) GetLines/s, $($h.PollsPerSec) polls/s; $($h.PaneHeld) PaneHeld (the server $(if ($h.Holds) { 'holds them' } else { 'pushes them' }))"

    # 2b. names: each busy tab out of view sets its title, a user var and its progress and rings
    # the bell; what the rail reads of them comes at once whatever the server holds
    $names = @()
    $alerts0 = Get-Pdus (Get-Metrics $w) 'NotifyAlert' 'received'
    for ($k = 1; $k -le $Busy; $k++) {
      $sid = $busyIds[$k - 1]
      $word = "named-$k-$(Get-Random -Maximum 100000)"
      $logFrom = Get-LogLength $w
      $t0 = Get-UnixMs
      [IO.File]::WriteAllText((Get-NamesFile $k), $word)
      # The title as the window's own socket lists the pane (out of view, so not in the state
      # file's panes, which are the open tab's), the progress on its tab's rail row (OSC 7501's,
      # its program's own record: the rail draws no OSC 9;4, which the state file has for the open
      # tab's panes alone), the user var as Lua heard it: when each was first seen
      $seen = [ordered]@{ Title = $null; Progress = $null; UserVar = $null }
      $tabId = $null
      while (((Get-UnixMs) - $t0) -lt 5000 -and ($null -eq $seen.Title -or $null -eq $seen.Progress -or $null -eq $seen.UserVar)) {
        if ($null -eq $seen.Title) {
          $p = @(Get-Panes | Where-Object title -eq $word)[0]
          if ($p) { $seen.Title = (Get-UnixMs) - $t0; $tabId = $p.tab_id }
        }
        if ($null -ne $tabId -and $null -eq $seen.Progress) {
          $row = @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).rail.rows | Where-Object { $null -ne $_ -and $_.tab -eq $tabId })[0]
          if ($row -and [int]$row.progress -eq 42) { $seen.Progress = (Get-UnixMs) - $t0 }
        }
        if ($null -eq $seen.UserVar -and (Test-UserVarLogged $w $logFrom $word)) { $seen.UserVar = (Get-UnixMs) - $t0 }
        Start-Sleep -Milliseconds 20
      }
      # The window never had the title: whether its server has it, which tells a title lost on the
      # way from one the pane never set
      $serverTitle = if ($null -eq $seen.Title) { [bool]@(Get-Panes -Server | Where-Object title -eq $word).Count } else { $true }
      $ms = if ($null -ne $seen.Title -and $null -ne $seen.Progress -and $null -ne $seen.UserVar) { [Math]::Round(($seen.Values | Measure-Object -Maximum).Maximum, 1) } else { $null }
      $names += [ordered]@{ Tab = $k; Ms = $ms; TitleMs = $seen.Title; ProgressMs = $seen.Progress; UserVarMs = $seen.UserVar; ServerTitle = $serverTitle }
    }
    $results.Names = $names
    $results.NamesBells = (Get-Pdus (Get-Metrics $w) 'NotifyAlert' 'received') - $alerts0
    $results.NamesMax = ($names | ForEach-Object { if ($null -eq $_.Ms) { [double]::PositiveInfinity } else { $_.Ms } } | Measure-Object -Maximum).Maximum
    "names (each busy tab's title, user var and progress, out of view): all three in the window $(($names | ForEach-Object { if ($null -ne $_.Ms) { "$($_.Ms)" } else { "never (title $($_.TitleMs)$(if (-not $_.ServerTitle) { ', the server hasn''t it either' }), progress $($_.ProgressMs), user var $($_.UserVarMs))" } }) -join ', ') ms after the file; $($results.NamesBells) NotifyAlerts taken meanwhile"
    Format-Metrics $m -Top 4 | ForEach-Object { "  metrics: $_" }

    # 3. busy switches: each busy tab brought into view by a key while they all work, and back:
    # the key to the first frame that draws its pane, and the window's own time for it, with the
    # other tabs' pushes still coming
    $rec = Start-FrameRecorder -Window $w
    $busySwitches = @()
    try {
      for ($k = 1; $k -le $Busy; $k++) {
        $sid = $busyIds[$k - 1]
        $before = (Read-State).frame
        $beforeW = [long](Get-StateFrame $w)
        if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
        [void]$rec.Take()
        $count = Read-Count $k
        $t0 = Get-UnixMs
        Send-Key $w "F$($k + 1)"
        # Every frame from the key to the first that draws its pane: while its push hasn't come,
        # the switch's frames draw the tab before's panes in its place (`panes_tab`, Lachlan's 2b)
        $seen = [Collections.Generic.List[object]]::new()
        $shows = { param($sw, $s) $sw | Add-Member -NotePropertyName at_ms -NotePropertyValue $s.time_ms -Force; $seen.Add($sw); @($sw.panes | Where-Object { $_.server_id -eq $sid -and $_.drawn_cursor }).Count -gt 0 }.GetNewClosure()
        $ms = $null; $lag = $null
        try {
          $f = Wait-Recorded -Recorder $rec $shows -Hwnd ([long]$w.Handle) -AfterFrame $before -TimeoutSec 10 -What "busy tab $k's pane drawn"
          $ms = [Math]::Round($f.TimeMs - $t0, 1)
          $p = @($f.Window.panes | Where-Object server_id -eq $sid)[0]
          if ($count) { $lag = Get-Lag $p.drawn_cursor $count }
        } catch { }
        $ownMs = $null; $wait = $null
        if ($hasMetrics) {
          $sm = Get-Metrics $w
          $tab = @($sm.budget.actions | Where-Object { $_.action -eq 'tab' -and $_.count -gt 0 })[0]
          if ($tab) { $ownMs = [Math]::Round([double]$tab.max_ms, 1) }
          $wait = $sm.frame_wait
          # The switch said what the window shows, unless the server can't hear it
          if ($results.Said) { $results.Said.SwitchShown += Get-Pdus $sm 'SetShownPanes' 'sent' }
        }
        # The frames on its tab that drew another tab's panes in its place: the wait's (Lachlan's
        # 2b), each with the rail and the strip already on the busy tab
        # (a window of an older build says no `panes_tab`, and keeps none)
        $kept = @($seen | Where-Object { $_.PSObject.Properties.Name -contains 'panes_tab' -and $null -ne $_.active_tab -and $null -ne $_.tabs -and $_.panes_tab -ne @($_.tabs)[[int]$_.active_tab].id })
        # The switch's first frame, whatever it drew in the panes' place: the rail's and the strip's
        $firstOn = @($seen | Where-Object { [int]$_.active_tab -eq $k })[0]
        $firstMs = if ($firstOn) { [Math]::Round([double]$firstOn.at_ms - $t0, 1) }
        $keptSays = @($kept | ForEach-Object { "frame $($_.frame) with tab id $(@($_.tabs)[[int]$_.active_tab].id) open (index $($_.active_tab)) drew tab id $($_.panes_tab)'s panes, the rail's open row $(if ($null -ne $_.rail.open_row) { @($_.rail.rows)[[int]$_.rail.open_row].tab_index }), animating $(@($_.animations | ForEach-Object name) -join ' ')" })
        # A switch that waited painted as it would meanwhile: its rail on the busy tab's row, the
        # rail's ring and bar gliding and the strip's words dissolving, nothing of the panes
        # dissolving until the busy tab's own are drawn (a frame the recorder missed, a gap in the
        # frames' numbers, leaves it unjudged)
        if ($wait -and [int]$wait.waits -ge 1) {
          $nums = @($seen | ForEach-Object { [long]$_.frame })
          $gap = $nums.Count -and ($nums[0] -gt $beforeW + 1 -or @(for ($i = 1; $i -lt $nums.Count; $i++) { if ($nums[$i] -gt $nums[$i - 1] + 1) { $i } }).Count)
          if (-not $kept.Count -and -not $gap) { Add-Failure "busy switch to tab ${k} waited for its push ($($wait.max_ms) ms) and no frame drew meanwhile: the window's paint waited with its panes" }
          foreach ($kf in $kept) {
            $names = @($kf.animations | ForEach-Object name)
            $row = if ($null -ne $kf.rail.open_row) { @($kf.rail.rows)[[int]$kf.rail.open_row] }
            $why = @()
            if (-not $row -or [int]$row.tab_index -ne $k) { $why += "the rail's open row wasn't tab ${k}'s" }
            if ($names -notcontains 'rail.marker') { $why += "the rail's ring and bar didn't glide" }
            if ($names -notcontains 'strip.swap') { $why += "the strip's words didn't dissolve" }
            if ($names -contains 'pane.dissolve') { $why += 'panes dissolved' }
            if ($why) { Add-Failure "busy switch to tab ${k}, frame $($kf.frame) of its wait: $($why -join '; ')" }
          }
          if ($f -and @($f.Window.animations | ForEach-Object name) -notcontains 'pane.dissolve') { Add-Failure "busy switch to tab ${k}: the tab before's panes didn't dissolve as its own drew" }
        }
        $busySwitches += [ordered]@{ Tab = $k; Ms = $ms; FirstMs = $firstMs; OwnMs = $ownMs; Lag = $lag; Wait = $wait; Kept = $kept.Count; KeptSays = $keptSays }
        "busy switch to tab ${k}: $(if ($null -ne $ms) { "drawn $ms ms after the key, its first frame $firstMs (the window's own time $ownMs ms), $lag lines behind the screen it had written at the key" } else { 'never drawn' })$(if ($wait) { "; $($wait.waits) frame waits, $($wait.pushed) ended by its push and $($wait.bounded) by the bound, $($wait.max_ms) ms at most" }); $($kept.Count) frames kept another tab's panes$(if ($keptSays) { ": $($keptSays -join '; ')" })"
        Send-Key $w F1
        Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $ticker }).Count -eq 1 -and -not $s.animating } -What 'the ticker in view again' -TimeoutSec 10 | Out-Null
        Start-Sleep -Milliseconds 300
      }
    } finally { $rec.Dispose() }
    $results.BusySwitches = $busySwitches
    $times = @($busySwitches | Where-Object { $null -ne $_.Ms } | ForEach-Object { $_.Ms })
    $results.BusySwitchP50 = Get-Pct $times 0.5; $results.BusySwitchMax = ($times | Measure-Object -Maximum).Maximum
    $results.BusySwitchFirstP50 = Get-Pct @($busySwitches | Where-Object { $null -ne $_.FirstMs } | ForEach-Object { $_.FirstMs }) 0.5
    $results.BusySwitchOwnP50 = Get-Pct @($busySwitches | Where-Object { $null -ne $_.OwnMs } | ForEach-Object { $_.OwnMs }) 0.5
    "busy switches: drawn $($results.BusySwitchP50) ms after the key at the median, $($results.BusySwitchMax) at most, their first frames $($results.BusySwitchFirstP50) ms at the median; the window's own time $($results.BusySwitchOwnP50) ms at the median"

    # 4. polls: the tab in view printing
    Get-Metrics $w -Reset | Out-Null
    Set-Event "$Event-ticker"
    Start-Sleep ($PollSec + 1)
    $m = Get-Metrics $w
    $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out 'polls-metrics.json')
    $p = [ordered]@{
      Seconds = [Math]::Round($m.span_ms / 1000, 2)
      Polls = Get-Pdus $m 'GetPaneRenderChanges' 'sent'
      Liveness = Get-Pdus $m 'LivenessResponse' 'received'
      Pushes = Get-Pdus $m 'GetPaneRenderChangesResponse' 'received'
      Frames = $m.totals.frames
    }
    $results.Polls = $p
    "polls (the tab in view printing a line every 100 ms beside them, $($p.Seconds) s): $($p.Polls) GetPaneRenderChanges sent, $($p.Liveness) LivenessResponse back, $($p.Pushes) pushes, $($p.Frames) frames"

    # The pushes a second the GUI may take with one busy tab shown and the rest held: that tab's
    # lines (a push a line, the status block in the same write, or two where ConPTY splits the
    # write) and what -MaxHeldPushes allows; with nothing held it's every busy tab's, Busy + 1 times
    # one tab's lines
    $oneTab = 2 * 1000 / $EveryMs + $MaxHeldPushes

    # 6. two windows: a second window of the same GUI on the domain, with a busy tab of its own in
    # view there, the ticker in the first: each gets what it shows, the busy tabs neither shows
    # stay held
    $twoTag = $Busy + 1
    Send-Key $w F12
    $clock = [Diagnostics.Stopwatch]::StartNew()
    do {
      Start-Sleep -Milliseconds 50
      $found = @(Get-ProcessWindows $w | Where-Object { [long]$_ -ne [long]$w.Handle })
    } while (-not $found -and $clock.Elapsed.TotalSeconds -lt 10)
    if (-not $found) { throw 'F12 opened no second window' }
    $second = [long]$found[0]
    Wait-State { param($entry, $snap) @($snap.windows | Where-Object { [long]$_.hwnd -eq $second -and $_.frame -gt 0 -and @($_.panes | Where-Object { $null -ne $_.server_id }).Count -gt 0 }).Count -gt 0 } -What 'the second window drawing its tab' -TimeoutSec 15 | Out-Null
    $bSid = @((Get-StateWindow (Read-State) -Hwnd $second).panes)[0].server_id
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Read-Count $twoTag) -and $clock.Elapsed.TotalSeconds -lt 10) { Start-Sleep -Milliseconds 50 }
    Start-Sleep 1
    $two = [ordered]@{ Seconds = $null; PushesPerSec = $null; PaneHeld = $null; Lags = @(); Unread = 0 }
    $rec = Start-FrameRecorder -Window $w
    try {
      Get-Metrics $w -Reset | Out-Null
      $clock = [Diagnostics.Stopwatch]::StartNew()
      while ($clock.Elapsed.TotalSeconds -lt $WindowSec) {
        [void]$rec.Take()
        $bFrame = [long](Get-StateWindow (Read-State) -Hwnd $second).frame
        $count = Read-Count $twoTag
        $shows = { param($sw) [long]$sw.frame -gt $bFrame -and @($sw.panes | Where-Object { $_.server_id -eq $bSid -and $_.drawn_cursor }).Count -gt 0 }.GetNewClosure()
        try {
          $f = Wait-Recorded -Recorder $rec $shows -Hwnd $second -TimeoutSec 5 -What "the second window's tab drawn"
          $p = @($f.Window.panes | Where-Object server_id -eq $bSid)[0]
          if ($count) { $two.Lags += Get-Lag $p.drawn_cursor $count } else { $two.Unread++ }
        } catch { $two.Unread++ }
        Start-Sleep -Milliseconds 150
      }
      $m = Get-Metrics $w
      $two.Seconds = [Math]::Round($m.span_ms / 1000, 2)
      $two.PushesPerSec = [Math]::Round((Get-Pdus $m 'GetPaneRenderChangesResponse' 'received') / $two.Seconds, 1)
      $two.PaneHeld = Get-Pdus $m 'PaneHeld' 'received'
      if ($results.Said) { $results.Said.SwitchShown += Get-Pdus $m 'SetShownPanes' 'sent' }
    } finally { $rec.Dispose() }
    $two.LagMax = ($two.Lags | Measure-Object -Maximum).Maximum
    $results.TwoWindows = $two
    "two windows ($($two.Seconds) s, the second showing a busy tab the first hasn't): $($two.Lags.Count) of its frames read, at most $($two.LagMax) lines behind its program ($($two.Unread) unread); the GUI took $($two.PushesPerSec) pushes a second, $($two.PaneHeld) PaneHeld"
    Invoke-Cli kill-pane --pane-id $bSid -Server -Check | Out-Null
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (@(Get-ProcessWindows $w | Where-Object { [long]$_ -eq $second }).Count -and $clock.Elapsed.TotalSeconds -lt 10) { Start-Sleep -Milliseconds 100 }

    # 7. minimized: busy tab 1 in view, the window minimized and restored without activating it
    $sid1 = $busyIds[0]
    Send-Key $w F2
    Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $sid1 }).Count -eq 1 -and -not $s.animating } -What 'busy tab 1 in view' -TimeoutSec 10 | Out-Null
    Start-Sleep -Milliseconds 500
    $min = [ordered]@{ Seconds = $null; PushesPerSec = $null; Frames = $null; Ms = $null; Lag = $null; Waits = $null }
    if (-not (Set-WindowMinimized $w)) { throw "the window didn't minimize" }
    Get-Metrics $w -Reset | Out-Null
    Start-Sleep $WindowSec
    $m = Get-Metrics $w
    $min.Seconds = [Math]::Round($m.span_ms / 1000, 2)
    $min.PushesPerSec = [Math]::Round((Get-Pdus $m 'GetPaneRenderChangesResponse' 'received') / $min.Seconds, 1)
    $min.Frames = $m.totals.frames
    $rec = Start-FrameRecorder -Window $w
    try {
      [void]$rec.Take()
      $aFrame = [long](Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).frame
      $count = Read-Count 1
      $t0 = Get-UnixMs
      Set-WindowMinimized $w -Restore | Out-Null
      $shows = { param($sw) [long]$sw.frame -gt $aFrame -and @($sw.panes | Where-Object { $_.server_id -eq $sid1 -and $_.drawn_cursor }).Count -gt 0 }.GetNewClosure()
      try {
        $f = Wait-Recorded -Recorder $rec $shows -Hwnd ([long]$w.Handle) -TimeoutSec 10 -What 'the first frame after the restore'
        $min.Ms = [Math]::Round($f.TimeMs - $t0, 1)
        $p = @($f.Window.panes | Where-Object server_id -eq $sid1)[0]
        if ($count) { $min.Lag = Get-Lag $p.drawn_cursor $count }
      } catch { }
    } finally { $rec.Dispose() }
    $fw = (Get-Metrics $w).frame_wait
    if ($fw) { $min.Waits = [int]$fw.waits }
    $results.Minimized = $min
    "minimized ($($min.Seconds) s, busy tab 1 in view): the GUI took $($min.PushesPerSec) pushes a second, $($min.Frames) frames; restored: $(if ($null -ne $min.Ms) { "its first frame $($min.Ms) ms on, $($min.Lag) lines behind the screen written at the restore" } else { 'never drawn' }), $($min.Waits) frame waits"
    Send-Key $w F1
    Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $ticker }).Count -eq 1 -and -not $s.animating } -What 'the ticker in view again' -TimeoutSec 10 | Out-Null
  }

  # 5. switch: every busy tab on its final screen, out of view, then each brought into view
  Set-Content -LiteralPath $StopFile 'stop'
  Start-Sleep 2
  $h = Resolve-Handle $w
  $rec = Start-FrameRecorder -Window $w
  $switches = @()
  try {
    for ($k = 1; $k -le $Busy; $k++) {
      $sid = $busyIds[$k - 1]
      $c = Get-FinalCursor $k
      $wantCursor = @{ X = $c.Col - 1; Y = $c.Row - 1 }
      $before = (Read-State).frame
      if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
      [void]$rec.Take()
      $cpu0 = Get-CpuSnapshot $w
      $t0 = Get-UnixMs
      Send-Key $w "F$($k + 1)"
      $shows = { param($sw) @($sw.panes | Where-Object { $_.server_id -eq $sid -and $_.drawn_cursor }).Count -gt 0 }.GetNewClosure()
      $queue = [Collections.Generic.List[object]]::new()
      $stale = @()
      $right = $null
      try {
        while (-not $right) {
          $f = Wait-Recorded -Recorder $rec $shows -Hwnd ([long]$w.Handle) -AfterFrame $before -Queue $queue -TimeoutSec 10 -What "tab $k's pane drawn"
          $p = @($f.Window.panes | Where-Object server_id -eq $sid)[0]
          if ([int]$p.drawn_cursor.x -eq $wantCursor.X -and [int]$p.drawn_cursor.y -eq $wantCursor.Y) { $right = $f }
          else { $stale += "frame $($f.Frame) drew its cursor at $($p.drawn_cursor.x),$($p.drawn_cursor.y)"; $before = $f.Frame }
        }
      } catch { $stale += "never drawn right: $($_.Exception.Message)" }
      $ms = if ($right) { [Math]::Round($right.TimeMs - $t0, 1) } else { $null }
      Wait-State { param($s) -not $s.animating } -What 'the switch settled' -TimeoutSec 10 | Out-Null
      # What the switch cost the GUI thread to the window settled, and its own time for it (its
      # counters' tab action, from the key to the frame it showed in); what it fetched to show
      # it, before anything here asks it for lines
      $cpu1 = Get-CpuSnapshot $w
      $threadMs = [Math]::Round(($cpu1.ThreadCycles - $cpu0.ThreadCycles) / $rate, 2)
      $fetched = $null; $ownMs = $null; $waits = $null
      if ($hasMetrics) {
        $sm = Get-Metrics $w
        $fetched = Get-Pdus $sm 'GetLines' 'sent'
        $tab = @($sm.budget.actions | Where-Object { $_.action -eq 'tab' -and $_.count -gt 0 })[0]
        if ($tab) { $ownMs = [Math]::Round([double]$tab.max_ms, 1) }
        if ($sm.frame_wait) { $waits = [int]$sm.frame_wait.waits }
      }
      # What the window holds of it, from its own socket: its lines and their links
      $guiPane = @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).panes | Where-Object server_id -eq $sid)[0].id
      $text = @(Invoke-Cli get-text --pane-id $guiPane)
      $escaped = (Invoke-Cli get-text --escapes --pane-id $guiPane) -join "`n"
      $wrong = @()
      for ($r = 1; $r -le $FinalRows; $r++) {
        $line = if ($r -le $text.Count) { $text[$r - 1].TrimEnd() } else { '' }
        if ($line -ne (Get-FinalLine $k $r)) { $wrong += $r }
      }
      $links = ([regex]::Matches($escaped, "\x1b\]8;[^;]*;https://example\.com/hidden/$k/")).Count
      $switches += [ordered]@{ Tab = $k; Ms = $ms; OwnMs = $ownMs; ThreadMs = $threadMs; Stale = $stale; WrongRows = $wrong; Links = $links; Fetched = $fetched; Waits = $waits }
      "switch to tab ${k}: $(if ($null -ne $ms) { "its first right frame $ms ms after the key (the window's own time $ownMs ms, its GUI thread $threadMs ms to settled)" } else { 'never drawn right' })$(if ($stale) { ", before it: $($stale -join '; ')" }); $(if ($wrong) { "rows $($wrong -join ', ') not what it wrote" } else { "all $FinalRows rows what it wrote" }), $links of its URLs links, $fetched GetLines, $waits frame waits"
      Send-Key $w F1
      Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $ticker }).Count -eq 1 -and -not $s.animating } -What 'the ticker in view again' -TimeoutSec 10 | Out-Null
      Start-Sleep -Milliseconds 300
    }
  } finally { $rec.Dispose() }
  $results.Switches = $switches
  $times = @($switches | Where-Object { $null -ne $_.Ms } | ForEach-Object { $_.Ms })
  $results.SwitchP50 = Get-Pct $times 0.5; $results.SwitchMax = ($times | Measure-Object -Maximum).Maximum
  $results.SwitchOwnP50 = Get-Pct @($switches | Where-Object { $null -ne $_.OwnMs } | ForEach-Object { $_.OwnMs }) 0.5
  $results.SwitchThreadP50 = Get-Pct @($switches | ForEach-Object { $_.ThreadMs }) 0.5
  "switches: the first right frame $($results.SwitchP50) ms after the key at the median, $($results.SwitchMax) at most; the window's own time $($results.SwitchOwnP50) ms and its GUI thread $($results.SwitchThreadP50) ms a switch at the median"
  # Whatever ended the window's connection over the run to here (a PDU it didn't know, PITFALLS
  # 188); step 8 ends it on purpose
  # From the window's own first line: a GUI given a pid an earlier one had appends to its log (PITFALLS 240)
  $results.Connection = @(Get-ConnectionLines $w $w.LogStart)
  "the connection ($Pairing): $(if ($results.Connection.Count) { "ended: $($results.Connection -join ' | ')" } else { 'never ended' })"

  # 8. reconnect (PITFALLS 201): a tab printing out of view, held, finishes and the server hangs
  # up on the window before the hold's quiet push, so the hold outlives the connection that said
  # it. The new connection holds and owes nothing for the tab, quiet since: the window asks its
  # push at the reattach, so a switch to it draws its final screen, with no wait the bound ends
  $re = [ordered]@{ Tag = $Busy + 2; Reconnected = $false; Asked = $false; Cursor = $null; Waits = $null; Bounded = $null; PaneHeld = $null }
  $rStop = Join-Path $T.Out 'stop-reconnect'
  $logAt = Get-LogLength $w
  $rid = New-Tab -From $ticker -Program (New-BusyProgram $re.Tag -Go "$Event-reconnect" -Stop $rStop -Hangup $HangupFile) -NoActivate -Server
  Wait-State { param($s) @($s.tabs).Count -eq $want + 1 } -What "step 8's tab in the window" -TimeoutSec 30 | Out-Null
  if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
  Set-Event "$Event-reconnect"
  Start-Sleep -Milliseconds 1500
  if ($hasMetrics) { $re.PaneHeld = Get-Pdus (Get-Metrics $w) 'PaneHeld' 'received' }
  Set-Content -LiteralPath $rStop 'stop'
  $re.Reconnected = [bool](Wait-Until { @(Get-ConnectionLines $w $logAt | Where-Object { $_ -match 'Reconnected!' }).Count -gt 0 } -TimeoutSec 30 -IntervalMs 100)
  Start-Sleep -Milliseconds 1000
  # The window's word that a pane was still held at the reattach and its push asked
  # (ClientPane::lbterm_reconnected, at info): the hold outlived the connection, so this step
  # judged the case it's for
  $re.Asked = [bool](Test-LogSince $w $logAt "the server's $rid) was held: its push asked of the new connection")
  $rc = Get-FinalCursor $re.Tag
  $before = (Read-State).frame
  if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
  $rec = Start-FrameRecorder -Window $w
  try {
    [void]$rec.Take()
    Send-Key $w "F$($Busy + 2)"
    $shows = { param($sw) @($sw.panes | Where-Object { $_.server_id -eq $rid -and $_.drawn_cursor }).Count -gt 0 }.GetNewClosure()
    try {
      $f = Wait-Recorded -Recorder $rec $shows -Hwnd ([long]$w.Handle) -AfterFrame $before -Queue ([Collections.Generic.List[object]]::new()) -TimeoutSec 10 -What "step 8's tab drawn"
      $p = @($f.Window.panes | Where-Object server_id -eq $rid)[0]
      $re.Cursor = "$($p.drawn_cursor.x),$($p.drawn_cursor.y)"
    } catch { $re.Cursor = "never drawn: $($_.Exception.Message)" }
  } finally { $rec.Dispose() }
  if ($hasMetrics -and ($fw = (Get-Metrics $w).frame_wait)) { $re.Waits = [int]$fw.waits; $re.Bounded = [int]$fw.bounded }
  $results.Reconnect = $re
  "reconnect: $(if ($re.Reconnected) { 'the server hung up and the window reconnected' } else { 'no reconnect in 30 s' }); the tab's PaneHeld before $($re.PaneHeld), its push asked at the reattach $($re.Asked); its first frame after drew its cursor at $($re.Cursor) (its final screen's $($rc.Col - 1),$($rc.Row - 1)); $($re.Waits) frame waits, $($re.Bounded) ended by the bound"
  $results.Guard = Get-MemoryGuard
  [pscustomobject]$results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-hidden.json')
  Stop-TestWindow $w
  Stop-TestServers

  if (-not $Measure) {
    foreach ($s in $switches) {
      if ($s.Stale) { Add-Failure "switching to tab $($s.Tab): $($s.Stale -join '; ') before its first right frame" }
      if ($s.WrongRows) { Add-Failure "switching to tab $($s.Tab): the window held rows $($s.WrongRows -join ', ') other than it wrote" }
      if ($s.Links -lt $FinalRows) { Add-Failure "switching to tab $($s.Tab): $($s.Links) of its $FinalRows URLs were links in the window" }
      if ($s.Fetched) { Add-Failure "switching to tab $($s.Tab): the window fetched lines for it ($($s.Fetched) GetLines), which it should hold" }
      if ($s.Waits) { Add-Failure "switching to tab $($s.Tab), quiet out of view: its first frame waited for a push ($($s.Waits)), which its quiet push should have brought before the switch" }
    }
    foreach ($s in @($results.BusySwitches | Where-Object { $_ })) {
      if ($null -eq $s.Lag) { Add-Failure "switching to busy tab $($s.Tab): its first frame's screen couldn't be read" }
      elseif ($s.Lag -gt 1) { Add-Failure "switching to busy tab $($s.Tab): its first frame drew a screen $($s.Lag) lines behind the one it had written at the key" }
    }
    if (-not $Compat) {
      foreach ($n in @($results.Names)) {
        if ($null -eq $n.Ms -or $n.Ms -gt $NamesMs) { Add-Failure "busy tab $($n.Tab), out of view: its title, user var and progress reached the window $(if ($null -eq $n.Ms) { "not all within 5 s (title $($n.TitleMs), progress $($n.ProgressMs), user var $($n.UserVarMs) ms)" } else { "$($n.Ms) ms" }) after it was told, over $NamesMs" }
      }
      if ($results.NamesBells -lt 4 * $Busy) { Add-Failure "the window took $($results.NamesBells) NotifyAlerts while the $Busy busy tabs set their names and rang, under the $(4 * $Busy) they sent" }
      # The second window's tab drawn as its program wrote it, the first's busy tabs still held;
      # the minimized window's tab pushed and drawn at once on the restore, nothing else pushed
      $two = $results.TwoWindows
      if (-not $two.Lags.Count) { Add-Failure "two windows: none of the second window's frames of its tab could be read ($($two.Unread) tries)" }
      elseif ($two.LagMax -gt 1) { Add-Failure "two windows: the second window drew its tab $($two.LagMax) lines behind its program, which it shows" }
      $min = $results.Minimized
      if ($null -eq $min.Ms) { Add-Failure 'minimized: no frame drew busy tab 1 after the restore' }
      elseif ($null -eq $min.Lag -or $min.Lag -gt 1) { Add-Failure "minimized: the first frame after the restore drew busy tab 1 $($min.Lag) lines behind the screen written at the restore" }
      if ($min.Waits) { Add-Failure "minimized: the first frame after the restore waited for a push ($($min.Waits)), of a tab the window still showed" }
    }
    if ($OwnPair -and -not $Compat -and ($null -eq $results.BusySwitchP50 -or $results.BusySwitchP50 -gt $BusySwitchMs)) { Add-Failure "a switch to a tab at work drew it $($results.BusySwitchP50) ms after the key at the median, over $BusySwitchMs" }
    if ($null -eq $results.SwitchP50 -or $results.SwitchP50 -gt $SwitchMs) { Add-Failure "a switch to a busy tab showed it right $($results.SwitchP50) ms after the key at the median, over $SwitchMs" }
    # What was said: this build's window says what it shows, and says no more to a server that
    # answers it doesn't know it; another build's window says nothing and is told nothing
    $said = $results.Said
    if ($said) {
      $heldAny = $said.SetupHeld + [int]$results.Hidden.PaneHeld + [int]$results.TwoWindows.PaneHeld
      if ($GuiBin) {
        if ($said.SetupShown -or $said.SwitchShown) { Add-Failure "another build's window sent SetShownPanes ($($said.SetupShown) at its start, $($said.SwitchShown) at its switches): -GuiBin isn't an older window" }
        if ($heldAny) { Add-Failure "the server sent another build's window $heldAny PaneHeld, which a window that never said what it shows must never get (PITFALLS 188)" }
      }
      else {
        if (-not $said.SetupShown) { Add-Failure "this build's window sent no SetShownPanes at its start" }
        if ($said.SetupLacks) {
          # The server doesn't know it: said once, and no more on the connection
          if ($said.SwitchShown) { Add-Failure "the server answered SetShownPanes with an error, yet the window sent $($said.SwitchShown) more at its switches" }
          if ($heldAny) { Add-Failure "a server that doesn't know SetShownPanes sent $heldAny PaneHeld" }
        }
        elseif (-not $Compat -and -not $said.SwitchShown) { Add-Failure "the window's switches sent no SetShownPanes, on a server that knows it" }
      }
    }
    if ($results.Connection.Count) { Add-Failure "the window's connection ended ($Pairing): $($results.Connection -join ' | ')" }
    $re = $results.Reconnect
    if (-not $re.Reconnected) { Add-Failure "unmeasured: step 8's server never hung up on the window, or it never said Reconnected!, in 30 s" }
    else {
      $rc = Get-FinalCursor $re.Tag
      if ($re.Cursor -ne "$($rc.Col - 1),$($rc.Row - 1)") { Add-Failure "reconnect: the first frame of the tab that finished out of view as the connection ended drew its cursor at $($re.Cursor), not its final screen's $($rc.Col - 1),$($rc.Row - 1) (PITFALLS 201)" }
      if ($re.Bounded) { Add-Failure "reconnect: the switch to the tab that finished out of view waited out the bound $($re.Bounded) times for a push the new connection never sent (PITFALLS 201)" }
      if ($OwnPair -and -not $Compat -and -not $re.PaneHeld) { Add-Failure "unmeasured: step 8's tab wasn't held before the hang-up (no PaneHeld), so the reconnect's hold wasn't judged" }
      elseif ($OwnPair -and -not $Compat -and -not $re.Asked) { Add-Failure "unmeasured: step 8's tab's hold ended before the hang-up (the window's log never said it asked its push at the reattach), so the reconnect's hold wasn't judged" }
    }
    if (-not $Compat) {
      $holds = [bool]$results.Hidden.Holds
      if ($OwnPair -and -not $holds) { Add-Failure "this build's server didn't hold the tabs out of view (no PaneHeld came)" }
      if (($GuiBin -or ($said -and $said.SetupLacks)) -and $holds) { Add-Failure "the server held tabs for a window that didn't say what it shows ($Pairing)" }
      if ($holds) {
        if ($results.Hidden.PushesPerSec -gt $MaxHeldPushes) { Add-Failure "the server holds the tabs out of view, yet the window took $($results.Hidden.PushesPerSec) pushes a second of them, over $MaxHeldPushes" }
        if ($results.TwoWindows.PushesPerSec -gt $oneTab) { Add-Failure "two windows: the GUI took $($results.TwoWindows.PushesPerSec) pushes a second, over the $oneTab one shown busy tab brings: tabs neither window shows were pushed" }
        if ($results.Minimized.PushesPerSec -gt $oneTab) { Add-Failure "minimized: the GUI took $($results.Minimized.PushesPerSec) pushes a second, over the $oneTab its one shown busy tab brings" }
      }
      elseif ($null -eq $results.Hidden.PushUs) { Add-Failure "no pushes of the tabs out of view reached the window, from a server that doesn't hold them" }
      elseif ($OwnPair -and $results.Hidden.PushUs -gt $PushUs) { Add-Failure "a push of a tab out of view cost the GUI thread $($results.Hidden.PushUs) us, over $PushUs" }
      if ($OwnPair -and $results.Polls.Polls -gt $MaxPolls) { Add-Failure "the tab in view printing brought $($results.Polls.Polls) polls of the server in $($results.Polls.Seconds) s, over ${MaxPolls}: its pushes had every change" }
    }
  }
} -Passed "mux-hidden ($Pairing): $Busy busy tabs out of view on a mux domain $(if ($Compat) { 'against another build: each shows its screen at once when switched to' } else { 'held or pushed as the pair should, their names at once, two windows and a minimized one each drawing what it shows, and each tab its screen at once when switched to' })"
