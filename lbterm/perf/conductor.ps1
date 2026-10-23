#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/conductor_hub.rs, wezterm-gui/src/state_files.rs, lbterm-extras/src/conductor.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/lights.rs, wezterm-gui/src/termwindow/render/tab_panel.rs
<#
The Conductor on the tab rail (`lbterm_conductor`; wezterm-gui/src/termwindow/render/conductor.rs,
conductor_hub.rs, state_files.rs; lbterm-extras/src/conductor.rs) end to end, in a test window that
looks like Lachlan's LBTerm (New-TestConfig), with its state file in target\perf\conductor and its
hub played here, on a pipe of its own: it writes down every line LBTerm sends and pushes focus
lines when asked. Two more tabs say they hold Claude sessions (claudehook.exe's `claude_session`
user var).

1. The hub: LBTerm subscribes, and says which tab and pane holds each session.
2. No state file: the pad atop the rail is dim, "off" (off.png). On, read only: a grey outline and
   the mode, in the slot alone (read.png). Accept most: the spectrum's outline, its hues turning
   (most.png, most-later.png).
3. The hub says it's thinking about the first session: that tab's pad gets the iridescent outline,
   moving (thinking.png). Acting on the second: its pad too, a light going round it (acting.png).
   The hub lets go of both: they linger a lap at most, then go.
4. A right click on the pad drops its menu. A click on an item that waits for its answer runs it,
   and the menu stays, the item's arc turning and the others out of reach (pending.png,
   pending-later.png), until the state file changes. An item that doesn't wait, picked with the
   keys, runs and closes it at once, and so does the ticked one, which asks for what's so already.
   Esc stops a wait. Off, a left click drops it too, offering to turn on, and waits.
5. The Conductor's own tab (its session the state file's sessionId), opened last: LBTerm moves it
   first in the window and off the rail, so the hub hears it as tab 0 and the others as 1 to 3.
   Read only, the pointer on the pad turns its outline to the spectrum, hues turning (hover.png,
   hover-later.png), and on a tab instead it holds still. A left click on the pad opens its tab,
   the open tab's ring and bar on the pad (conductor-open.png); a click on the third row opens the
   third row's tab, not the window's third; a right click on the pad still drops the menu.

The pointer arrives as posted presses and keys as posted messages, as in the other perf scripts,
with LBTERM_POSTED_POINTER so a posted move hovers until a posted WM_MOUSELEAVE; the window must
never be the foreground one. Where the pad, the rows and the menu's items are, and when the pad,
the outlines and the menu have answered, come from the window's state file (lbterm/STATE.md, not
the Conductor's own button.json); the pictures still say how they look.

  pwsh -File lbterm\perf\conductor.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'conductor' -Bin $Bin
$state = Join-Path $T.Out 'button.json'
$report = Join-Path $T.Out 'report.txt'
$pipeName = "lbterm-perf-conductor-$PID"

# The hub, played: every line any connection sends is kept; a connection whose first line
# subscribes is pushed to
if (-not ('LbtermConductorHub' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
public static class LbtermConductorHub {
  static string pipe;
  static readonly List<StreamWriter> subscribers = new List<StreamWriter>();
  public static readonly List<string> Lines = new List<string>();
  public static void Serve(string name) {
    pipe = name;
    new Thread(Accept) { IsBackground = true }.Start();
  }
  static void Accept() {
    while (true) {
      var s = new NamedPipeServerStream(pipe, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances,
        PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
      s.WaitForConnection();
      new Thread(() => Talk(s)) { IsBackground = true }.Start();
    }
  }
  static void Talk(NamedPipeServerStream s) {
    var reader = new StreamReader(s);
    var writer = new StreamWriter(s) { AutoFlush = true, NewLine = "\n" };
    bool subscribed = false;
    try {
      string line;
      while ((line = reader.ReadLine()) != null) {
        lock (Lines) Lines.Add(line);
        if (!subscribed && line.Contains("\"lbterm.subscribe\"")) {
          subscribed = true;
          lock (subscribers) subscribers.Add(writer);
        }
      }
    } catch (IOException) { }
    lock (subscribers) subscribers.Remove(writer);
  }
  public static int Push(string line) {
    int n = 0;
    lock (subscribers) foreach (var w in subscribers.ToArray()) { try { w.WriteLine(line); n++; } catch (IOException) { } }
    return n;
  }
  public static string[] Heard() { lock (Lines) return Lines.ToArray(); }
}
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread
}

function Reported { @(Get-Content $report -ErrorAction SilentlyContinue) }
function Changed($a, $b, [int]$left = 0, [int]$top = 0, [int]$right = 100000, [int]$bottom = 100000) {
  Get-DiffBox $a $b -Region (New-Rect $left $top ($right - $left) ($bottom - $top))
}
# What the window shows once `what` is so and nothing animates (the Conductor's hues and lights
# are loops, which don't count), then the picture of it
function Shows([scriptblock]$what, [string]$name, [string]$desc) {
  $script:entry = Wait-Change $what -What $desc -Animating
  Get-Shot $win -Name $name
}

$lua = @"
local function note(what)
  return wezterm.action_callback(function(window, pane)
    local f = io.open('$(ConvertTo-LuaPath $report)', 'a')
    if f then f:write(what .. '\n'); f:close() end
  end)
end
local on = { state = { 'on', 'remote' } }
rawset(config, 'lbterm_conductor', {
  state_file = '$(ConvertTo-LuaPath $state)',
  hub_pipe = [[\\.\pipe\$pipeName]],
  menu = {
    { label = 'Turn on', when = { state = { 'off' } }, wait = true, action = note('on') },
    { label = 'Accept most', when = on, checked = { mode = 'most' }, wait = true, action = note('mode most') },
    { label = 'Read only', when = on, checked = { mode = 'read' }, wait = true, action = note('mode read') },
    { label = 'Accept all', when = on, checked = { mode = 'all' }, wait = true, action = note('mode all') },
    { label = 'Turn off', when = on, wait = true, action = note('off') },
    { label = 'Say hello', when = on, action = note('hello') },
  },
})
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@

[LbtermConductorHub]::Serve($pipeName)
Invoke-Test {
  $win = Open-TestWindow -Lua $lua -PostedPointer -Program (New-PwshProgram 'Start-Sleep 600')
  Start-PointerWatch $win
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail.conductor } -What 'the window and the Conductor''s pad' | Out-Null
  $first = (Get-Panes)[0].pane_id

  # Two tabs holding Claude sessions, as claudehook.exe says so
  $sids = @('a1b2c3d4-0000-4000-8000-000000000001', 'a1b2c3d4-0000-4000-8000-000000000002')
  $panes = foreach ($sid in $sids) { New-Tab -From $first -NoActivate -Program (New-ClaudeSessionProgram $sid) }

  # 1. LBTerm subscribes, and says where the sessions are
  $deadline = (Get-Date).AddSeconds(15)
  $where = $null
  do {
    Start-Sleep -Milliseconds 300
    $heard = [LbtermConductorHub]::Heard()
    $where = $heard | Where-Object { $_ -match '"lbterm.panes"' } | Select-Object -Last 1 | ConvertFrom-Json
  } while ((Get-Date) -lt $deadline -and (-not ($heard -match '"lbterm.subscribe"') -or @($where.panes).Count -lt 2))
  if (-not ($heard -match '"lbterm.subscribe"')) { Add-Failure 'LBTerm never subscribed to the hub' }
  foreach ($k in 0, 1) {
    $p = @($where.panes) | Where-Object sessionId -eq $sids[$k]
    if (-not $p) { Add-Failure "the hub never heard where session $($k + 1) is: $($heard -join ' | ')" }
    elseif ($p.pane -ne $panes[$k] -or $p.tab -ne $k + 2) { Add-Failure "session $($k + 1) said to be in tab $($p.tab), pane $($p.pane): it's in tab $($k + 2), pane $($panes[$k])" }
  }
  if (-not $T.Failures.Count) { "hub: subscribed; sessions in tabs 2 and 3, panes $($panes -join ' and ')" }

  # 2. The pad: off, read only, accept most
  $off = Shows { param($s) @($s.rail.rows).Count -eq 3 -and $s.rail.conductor.level -eq 'off' } 'off' 'three rows, the pad off'
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only), here only since 09:05" }')
  $read = Shows { param($s) $s.rail.conductor.level -eq 'read' } 'read' 'the pad read only'
  $head = Changed $off $read
  if (-not $head) { Add-Failure 'the pad never showed the Conductor on' }
  elseif ($head[2] -gt 90 -or $head[3] -gt 200) { Add-Failure "turning on changed more than the pad's slot: $(Format-Box $head)" }
  else { "read: the pad changed at $(Format-Box $head)" }
  # The pad and the rows under it, where the file says
  $padRect = $script:entry.rail.conductor.pad
  $pad = Get-RectCenter $padRect
  # Below the Conductor's slot, the tabs start (the pad's outline glows a few pixels past its pill)
  $tabs = if ($head) { $head[3] + 12 } else { [int]($padRect.y + $padRect.h + 12) }
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "most", "tooltip": "Conductor is on (accept most), here only since 09:05" }')
  $most = Shows { param($s) $s.rail.conductor.level -eq 'most' } 'most' 'the pad accepting most'
  # Its hues turn: a loop, so two pictures apart
  Start-Sleep -Milliseconds 700
  $later = Get-Shot $win -Name 'most-later'
  $d = Changed $read $most 0 0 100000 $tabs
  if (-not $d) { Add-Failure 'accept most never showed' } else { "most: the outline at $(Format-Box $d)" }
  if (-not (Changed $most $later 0 0 90 $tabs)) { Add-Failure "the spectrum's hues never turned" } else { 'most: its hues turn' }
  if (Changed $most $later 0 $tabs 90 100000) { Add-Failure "the tabs changed while only the pad's outline should move" }

  # 3. Thinking about the first session, acting on the second, then neither
  $focus = { param($list) '{"type":"conductor.focus","sessions":[' + (($list | ForEach-Object { '{"sessionId":"' + $_[0] + '","doing":"' + $_[1] + '"}' }) -join ',') + ']}' }
  $n = [LbtermConductorHub]::Push((& $focus @(, @($sids[0], 'thinking'))))
  if ($n -lt 1) { Add-Failure 'no subscriber to push to' }
  $thinking = Shows { param($s) @($s.rail.rows | Where-Object conductor -eq 'thinking').Count -eq 1 } 'thinking' 'a row the Conductor thinks about'
  $d = Changed $most $thinking 0 $tabs 90 100000
  if (-not $d) { Add-Failure 'thinking: no outline on the first session''s tab' } else { "thinking: an outline at $(Format-Box $d)" }
  $thinkingTop = if ($d) { $d[1] } else { 0 }
  Start-Sleep -Milliseconds 700
  if (-not (Changed $thinking (Get-Shot $win -Name 'thinking-later') 0 $tabs 90 100000)) { Add-Failure 'thinking: the outline never moved' }
  [void][LbtermConductorHub]::Push((& $focus @(@($sids[0], 'thinking'), @($sids[1], 'acting'))))
  $acting = Shows { param($s) @($s.rail.rows | Where-Object conductor -eq 'acting').Count -eq 1 } 'acting' 'a row the Conductor acts on'
  $d = Changed $thinking $acting 0 ($thinkingTop + 40) 90 100000
  if (-not $d) { Add-Failure 'acting: no outline on the second session''s tab' } else { "acting: an outline and its light at $(Format-Box $d)" }
  # The second session's row, for a click on it later (the third row)
  $actingRow = @($script:entry.rail.rows | Where-Object conductor -eq 'acting')[0]
  [void][LbtermConductorHub]::Push((& $focus @()))
  # They linger a lap at most: the wait is what's tested
  Start-Sleep 2.5
  $d = Changed $most (Get-Shot $win -Name 'let-go') 0 $tabs 90 100000
  if ($d) { Add-Failure "the outlines stayed after the hub let go: $(Format-Box $d)" } else { 'let go: both outlines went' }

  # 4. The menu, by what its items run. Read only, the pad's outline holds still
  $menuUp = { param([switch]$Left)
    Send-Click $win $pad[0] $pad[1] -Right:(-not $Left)
    Wait-State { param($s) $s.menu.kind -eq 'conductor' } -What 'the Conductor''s menu'
  }
  $leave = { Send-Leave $win; Wait-State { param($s) -not $s.pointer } -What 'the pointer gone' | Out-Null }
  $keys = { param([string[]]$names) Send-Key $win $names -GapMs 80; Start-Sleep -Milliseconds 400 }
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)", "updatedAt": 1 }')
  Shows { param($s) $s.rail.conductor.level -eq 'read' } 'menu-closed' 'the pad read only again' | Out-Null
  # The arc, in the first row's tick
  $arc = @(($pad[0] + 4), ($pad[1] + 4), ($pad[0] + 36), ($pad[1] + 36))
  # Whether the menu goes within 5 s: the window answers a changed file at its next paint
  $menuGoes = { param([string]$name)
    try { Wait-State { param($s) -not $s.menu } -What 'the menu to close' -TimeoutSec 5 | Out-Null; (Get-Shot $win -Name $name).Dispose(); $true }
    catch { (Get-Shot $win -Name $name).Dispose(); $false }
  }
  # Whether the last item run is `what` within 5 s: under load the window answers keys late
  $lastRun = { param([string]$what)
    for ($i = 0; $i -lt 20; $i++) {
      if ((Reported | Select-Object -Last 1) -eq $what) { return $true }
      Start-Sleep -Milliseconds 250
    }
    $false
  }

  # a. "Accept most" waits: it runs, the menu stays, its arc turns, the others can't be picked
  $m = & $menuUp
  $item = @($m.menu.items | Where-Object label -eq 'Accept most')[0]
  if (-not $item) { throw "no 'Accept most' in the menu: $(@($m.menu.items | ForEach-Object label) -join ', ')" }
  $x, $y = Get-RectCenter $item.rect
  Send-Click $win $x $y
  & $leave
  $waiting = Wait-State { param($s) $s.menu.waiting -ne $null } -What 'the item to wait for its answer' -TimeoutSec 5
  $p1 = Get-Shot $win -Name 'pending'
  Start-Sleep -Milliseconds 500
  $p2 = Get-Shot $win -Name 'pending-later'
  [void](& $lastRun 'mode most')
  if (((Reported) -join ',') -ne 'mode most') { Add-Failure "a click on the first item ran: $((Reported) -join ', ')" } else { 'menu: a click ran "Accept most"' }
  if (-not (Read-State).windows[0].menu) { Add-Failure 'the menu closed while its item waited' } else { "menu: it stays open while ""Accept most"" waits (waiting: $($waiting.menu.waiting))" }
  if (-not (Changed $p1 $p2 $arc[0] $arc[1] $arc[2] $arc[3])) { Add-Failure "the waiting item's arc never turned" } else { 'menu: its arc turns' }
  & $keys Down, Enter
  if ((Reported).Count -ne 1) { Add-Failure "while it waited, Down and Enter ran: $((Reported) -join ', ')" }
  # The answer: the file written again in the same words, only its updatedAt new (as Conductor's
  # off does when it's off already; the mode stays read so the pad holds still), and the menu goes
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)", "updatedAt": 2 }')
  if (-not (& $menuGoes 'answered')) { Add-Failure 'the menu stayed after the state file changed' } else { 'menu: it closes when the state file changes' }

  # b. "Say hello" doesn't wait: Down five times and Enter run it, and the menu closes at once
  & $menuUp | Out-Null
  & $leave
  & $keys Down, Down, Down, Down, Down, Enter
  if (-not (& $lastRun 'hello')) { Add-Failure "the keys ran: $((Reported) -join ', ')" } else { 'menu: Down five times and Enter ran "Say hello"' }
  if (-not (& $menuGoes 'hello')) { Add-Failure "the menu stayed open after an item that doesn't wait" } else { "menu: an item that doesn't wait closes it" }

  # The ticked "Read only" waits for nothing: the Conductor is in it already, and Conductor writes
  # nothing for it. Down twice and Enter run it, and the menu closes at once
  & $menuUp | Out-Null
  & $leave
  & $keys Down, Down, Enter
  if (-not (& $lastRun 'mode read')) { Add-Failure "the keys ran: $((Reported) -join ', ')" } else { 'menu: Down twice and Enter ran the ticked "Read only"' }
  if (-not (& $menuGoes 'ticked')) { Add-Failure 'the menu waited on the item already ticked' } else { 'menu: the ticked item closes it at once' }

  # c. Esc stops a wait
  & $menuUp | Out-Null
  & $leave
  & $keys Down, Down, Down, Enter
  & $keys Escape
  if (-not (& $lastRun 'mode all')) { Add-Failure "the keys ran: $((Reported) -join ', ')" }
  if (-not (& $menuGoes 'escaped')) { Add-Failure 'Esc left the waiting menu open' } else { 'menu: Esc stops a wait' }

  # d. Off, a left click drops the menu too (there's no tab to open), offering to turn on, and
  # waits for it
  Remove-Item $state
  $d = Changed $off (Shows { param($s) $s.rail.conductor.level -eq 'off' } 'off-again' 'the pad off again') 0 0 90 $tabs
  if ($d) { Add-Failure "without its file the pad isn't off: $(Format-Box $d)" } else { 'off: the pad is off again' }
  & $menuUp -Left | Out-Null
  & $leave
  & $keys Down, Enter
  if (-not (& $lastRun 'on')) { Add-Failure "off, the menu ran: $((Reported) -join ', ')" } else { 'menu: off, it offers to turn on' }
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read" }')
  if (-not (& $menuGoes 'turned-on')) { Add-Failure 'the menu stayed after the Conductor came on' } else { 'menu: it closes when the Conductor comes on' }

  # 5. The Conductor's own tab, opened last as Conductor opens it (--no-activate); the state file
  # names its session
  $own = 'a1b2c3d4-0000-4000-8000-0000000000c0'
  $ownPane = New-Tab -From $first -NoActivate -Program (New-ClaudeSessionProgram $own)
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "sessionId": "' + $own + '" }')
  # The last panes line LBTerm sent, once it says what `$ok` wants, within 10 s
  $panesWhen = { param([scriptblock]$ok)
    $deadline = (Get-Date).AddSeconds(10)
    do {
      $line = [LbtermConductorHub]::Heard() | Where-Object { $_ -match '"lbterm.panes"' } | Select-Object -Last 1 | ConvertFrom-Json
      if ($line -and (& $ok @($line.panes))) { return @($line.panes) }
      Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    @($line.panes)
  }
  $fact = { param($ps, [string]$sid) $ps | Where-Object sessionId -eq $sid | Select-Object -First 1 }

  # a. First in the window, off the rail: the hub hears it as tab 0, the others as the rail shows them
  $ps = & $panesWhen { param($ps) (& $fact $ps $own).tab -eq 0 }
  $o = & $fact $ps $own
  if (-not $o -or $o.tab -ne 0 -or $o.pane -ne $ownPane) { Add-Failure "its tab wasn't heard as tab 0, pane ${ownPane}: $($ps | ConvertTo-Json -Compress)" }
  foreach ($k in 0, 1) {
    $p = & $fact $ps $sids[$k]
    if ($p.tab -ne $k + 2) { Add-Failure "with the Conductor's tab off the rail, session $($k + 1) is said to be in tab $($p.tab), not $($k + 2)" }
  }
  if ($o.active) { Add-Failure 'opening the Conductor''s tab with --no-activate made it the open one' }
  $list = @(Get-Panes)
  if ($list[0].pane_id -ne $ownPane) { Add-Failure "its tab isn't first in the window: the panes go $($list.pane_id -join ', ')" }
  if (-not $T.Failures.Count) { "its tab: first in the window (pane $ownPane), heard as tab 0; the others still tabs 1 to 3" }

  # b. Read only, the pointer on the pad turns its outline to the spectrum, its hues turning; on a
  # tab it holds still. The panel opens under it: settled first, then two shots apart
  $padBox = @(0, ($padRect.y - 4), 90, ($padRect.y + $padRect.h + 4))
  $hover = { param($x, $y, [string]$over) Send-Mouse $win Move $x $y; Wait-Change { param($s) $s.rail.open -and $s.pointer.over -eq $over } -What "the panel open, the pointer on $over" -Animating | Out-Null }
  & $hover $pad[0] $pad[1] 'rail.conductor'
  $h1 = Get-Shot $win -Name 'hover'
  Start-Sleep -Milliseconds 700
  $h2 = Get-Shot $win -Name 'hover-later'
  if (-not (Changed $h1 $h2 $padBox[0] $padBox[1] $padBox[2] $padBox[3])) { Add-Failure 'read only, the pointer on the pad: its outline never turned' } else { 'hover: on the pad, read only, its spectrum turns' }
  & $hover $pad[0] ($tabs + 30) 'rail.row'
  $t1 = Get-Shot $win -Name 'hover-tab'
  Start-Sleep -Milliseconds 700
  $d = Changed $t1 (Get-Shot $win -Name 'hover-tab-later') $padBox[0] $padBox[1] $padBox[2] $padBox[3]
  if ($d) { Add-Failure "read only, the pointer on a tab: the pad's outline moved at $(Format-Box $d)" } else { 'hover: on a tab, the pad holds still' }
  & $leave
  Wait-Change { param($s) -not $s.rail.open } -What 'the panel closed' -Animating | Out-Null

  # c. A left click on the pad opens its tab: the hub hears it's the open one, and the open
  # tab's ring and bar are on the pad
  $closedPad = Get-Shot $win -Name 'conductor-closed'
  $runs = (Reported).Count
  Send-Click $win $pad[0] $pad[1]
  & $leave
  $ps = & $panesWhen { param($ps) (& $fact $ps $own).active }
  if (-not (& $fact $ps $own).active) { Add-Failure 'a left click on the pad didn''t open the Conductor''s tab' } else { 'click: the pad opened its tab' }
  $d = Changed $closedPad (Shows { param($s) $s.rail.conductor.open } 'conductor-open' 'the Conductor''s tab open') $padBox[0] $padBox[1] $padBox[2] $padBox[3]
  if (-not $d) { Add-Failure 'its tab open, the pad shows nothing of it' } else { "click: the open tab's marker on the pad at $(Format-Box $d)" }
  if ((Reported).Count -ne $runs) { Add-Failure "the left click ran a menu item: $((Reported) -join ', ')" }

  # d. A click on the third row opens the third row's tab: the second session's, not the window's
  # third (the first session's, with the Conductor's first)
  if ($actingRow) {
    $third = Get-RectCenter $actingRow.pad
    Send-Click $win $third[0] $third[1]
    & $leave
    $ps = & $panesWhen { param($ps) (& $fact $ps $sids[1]).active }
    if (-not (& $fact $ps $sids[1]).active) {
      Add-Failure "a click on the third row opened: $(($ps | Where-Object active).sessionId)"
    } else { "click: the third row opened session 2's tab" }
  }

  # e. A right click on the pad still drops the menu
  $before = Get-Shot $win -Name 'right-before'
  $m = & $menuUp
  & $leave
  $d = ConvertTo-Rect $m.menu.rect
  $after = Get-Shot $win -Name 'right-menu'
  if (-not (Get-DiffBox $before $after -Region $d)) { Add-Failure 'a right click on the pad dropped no menu' } else { 'right click: the menu' }
  & $keys Escape
} -Passed 'conductor: the pad shows its state, the tabs show what it''s doing, and the menu runs its items'
