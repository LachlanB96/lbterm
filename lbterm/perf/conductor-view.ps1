#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/conductor_view.rs, wezterm-gui/src/seat_client.rs, lbterm-extras/src/seat/**, wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/keyevent.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, wezterm-gui/src/termwindow/render/composited.rs
<#
The Conductor's view (lbterm/TAB-PANEL.md, "The Conductor's view"; wezterm-gui/src/termwindow/
render/conductor_view.rs, seat_client.rs; lbterm-extras/src/seat) end to end, in a test window that
looks like Lachlan's LBTerm (New-TestConfig), against a seat played here on a pipe of its own: it
writes down every line LBTerm sends, pushes `seat.view`, `seat.items`, `seat.delta` and
`seat.show` to the subscriber, and answers every command with a `seat.reply` (ok, or a refusal when
asked).

1. LBTerm subscribes (`seat.subscribe`, client lbterm). The seat says the Conductor is on, reading,
   with an ask waiting: the pad goes orange (pad-asks.png).
2. A left click on the pad opens the view over the open tab, the pad carrying the open tab's ring
   (view.png): the conversation, the header, the box, and down the side the ask, two sessions'
   questions, the sessions, the events, a suggestion and a note.
3. The reply streams in (`seat.delta` twice, streaming.png), then the finished item replaces it.
4. The ask's Allow sends `seat.allow` for it; the seat's next view drops the ask and the pad's
   orange goes.
5. Typing goes in the box; Enter sends `seat.say` with the words, and the box empties (typed.png).
   The seat's echo and a message from the phone queued behind the turn under way show, and Stop
   sends `seat.interrupt` (said.png). A refusal puts the words back and says why under the box
   (refused.png).
6. "Accept most" in the header sends `seat.control` `mode most`.
7. A question in text's `1b`
   chip goes in its box, each on a line of its own, a second press takes one out, and Send
   answers it with the lines (answered.png).
8. "Useful" on the suggestion sends `seat.rsi` rate useful.
   Then the seat hangs: -Hung (10) messages it reads and never answers, their connections left
   open. Each is said to have had no answer once its 15 s are over, and the GUI has no more
   threads (2 at most, for others' comings and goings) or handles (4) after than before them
   (hung.json). Until 2026-10-30 each command started two threads, and the one reading its reply
   stayed blocked, with its handle, for as long as the seat kept the connection.
9. A session's card opens its tab: the view leaves for it, and the panes that were under it go
   at once, with no `pane.dissolve`: they never showed (until 2026-10-30 they came back whole for
   a frame and faded out over the tab that opened).
10. `seat.show` opens the view again; a click on the first tab's row leaves it for that tab, no
    `pane.dissolve` either; the pad opens it again. A workspace switch (F9) leaves it, as
    any tab opened another way does, and the state file says the window once; back in the first
    workspace (F6) the pad opens it again, and Esc with the box empty leaves it (closed.png).
11. In a window 860 px across the view keeps its side column (narrow.png).

At rest, after 2: the seat says the same again with every time hours old but one question asked
40 s ago, and for -RestSec (10 s) nothing is sent or pressed. The view open at rest paints one
frame, when that question's "just now" turns to "1 min ago" at 45 s, and nothing else: a view that
paints on its own (it laid itself out and drew every frame, about 80% of a core, until 2026-10-29)
fails, and so does one whose clock never repaints. The frames, frames a second and the GUI process's
CPU over the rest are printed, with the view closed too, for measuring a change.

Then, still with nothing else to paint the window:
- Two refusals 3 s apart: the second notice goes 12 s after it came, by itself (until 2026-10-30 a
  notice that came while an earlier one's end was due got no repaint of its own, and stayed).
- The tab under the view, then a tab behind it, light claude_status orange as claudehook.exe does
  for a reply that asks in text, printing nothing else: each pad says so within a second (the
  panes under the view don't repaint the window, and until 2026-10-30 the light didn't either).
- The scroll strip isn't drawn beside the view: it's the covered pane's scrollbar.

-Measure does only that: the window, the view closed at rest, the view open at rest (every time
hours old, so nothing at all should paint), and the numbers, -Runs times in one window. Run it
against two builds in turn (-Bin) to compare them.

The pointer arrives as posted presses and keys as posted messages, as in the other perf scripts,
with LBTERM_POSTED_POINTER; the window must never be the foreground one. Where the buttons are and
when the view has answered come from the window's state file (lbterm/STATE.md, `conductor_view`);
the pictures say how it looks.

  pwsh -File lbterm\perf\conductor-view.ps1
  pwsh -File lbterm\perf\conductor-view.ps1 -Measure -Runs 3 -Bin <build>
#>
param([string]$Bin, [switch]$Measure, [int]$Runs = 1, [int]$RestSec = 10, [int]$Hung = 10)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'conductor-view' -Bin $Bin
$state = Join-Path $T.Out 'button.json'
$pipeName = "lbterm-perf-seat-$PID"

# The seat, played: every line any connection sends is kept; a connection whose first line
# subscribes is pushed to; any other line with an id is a command, answered on its connection
if (-not ('LbtermFakeSeat' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public static class LbtermFakeSeat {
  static string pipe;
  static readonly List<StreamWriter> subscribers = new List<StreamWriter>();
  public static readonly List<string> Lines = new List<string>();
  // The next command of this type is refused
  public static string Refuse = "";
  // While set, commands are read and never answered, their connections left open: a seat that hangs
  public static bool Silent = false;
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
        var type = Regex.Match(line, "\"type\":\"([^\"]+)\"").Groups[1].Value;
        if (type == "seat.subscribe") {
          if (!subscribed) { subscribed = true; lock (subscribers) subscribers.Add(writer); }
          continue;
        }
        var id = Regex.Match(line, "\"id\":\"([^\"]+)\"");
        if (!id.Success || Silent) continue;
        string reply;
        lock (Lines) {
          if (Refuse == type) {
            Refuse = "";
            reply = "{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":false,\"error\":\"Refused by the test seat\"}";
          } else {
            reply = "{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":true}";
          }
        }
        writer.WriteLine(reply);
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
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread, System.Text.RegularExpressions
}

# The commands LBTerm sent of a type, as objects
function Sent([string]$type) {
  @([LbtermFakeSeat]::Heard() | Where-Object { $_ -match ('"type":"' + [regex]::Escape($type) + '"') } | ForEach-Object { $_ | ConvertFrom-Json })
}
# The last command of a type, once one more than `$before` has come, within 5 s
function Next-Sent([string]$type, [int]$before) {
  $deadline = (Get-Date).AddSeconds(5)
  do {
    $all = Sent $type
    if ($all.Count -gt $before) { return $all[-1] }
    Start-Sleep -Milliseconds 50
  } while ((Get-Date) -lt $deadline)
  $null
}
function Push([string]$line) { [void][LbtermFakeSeat]::Push(($line -replace '\r?\n\s*', ' ')) }
function View { (Read-State).windows[0].conductor_view }
function Button($v, [string]$name) {
  $b = @($v.buttons | Where-Object name -eq $name)[0]
  if (-not $b) { throw "no button $name on the view: $(@($v.buttons | ForEach-Object name) -join ', ')" }
  $b
}
# A button on the side column scrolled into sight: the wheel over the side, down and then up
function Reveal([string]$name) {
  for ($turn = 0; $turn -lt 40; $turn++) {
    $v = View
    $b = Button $v $name
    if ($b.rect) { return $b }
    $delta = if ($turn -lt 15) { -120 } else { 120 }
    $sx = [int]($v.rect.x + $v.rect.w - 120); $sy = [int]($v.rect.y + $v.rect.h / 2)
    $frame = Get-StateFrame $win
    Send-Wheel $win $sx $sy -Delta $delta
    try { Wait-State { param($s) $true } -AfterFrame $frame -What 'the side scrolled' -TimeoutSec 2 | Out-Null } catch { }
  }
  throw "button $name never came into sight"
}
function Press([string]$name) {
  $b = Reveal $name
  $x, $y = Get-RectCenter $b.rect
  Send-Click $win $x $y -Move
}
# What the window shows once `what` is so, settled but for loops and notices, then its picture
function Shows([scriptblock]$what, [string]$name, [string]$desc) {
  $script:entry = Wait-Change $what -What $desc -Animating
  Get-Shot $win -Name $name
}
# Whether words are drawn in a rectangle the state file names: pixels at least `floor` bright,
# which the view's fills (the card, a bubble, a button's tint, a hairline) never are. Every word
# went missing once, when another feature's visual took the view's z-layers, while the fills
# and every state-file check still passed.
function Test-Words($shot, $rect, [string]$what, [int]$floor = 140) {
  if (-not $rect) { Add-Failure "no rectangle for $what in the state file"; return }
  if (-not [LbtermE2E.Px]::BrightBox($shot, (ConvertTo-Rect $rect), $floor)) { Add-Failure "$what shows no words (no pixel $floor bright in $(($rect | ConvertTo-Json -Compress)))" }
  else { "words: $what drawn" }
}

$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sidA = 'b1c2d3e4-0000-4000-8000-00000000000a'
$sidB = 'b1c2d3e4-0000-4000-8000-00000000000b'
# The seat's view: times count back from -At (now when the script started), and the second
# question was asked at -Asked (30 s before that)
$viewWith = {
  param([bool]$ask, [string]$mode = 'read', [bool]$waiting = $true, [long]$At = $now, [long]$Asked = $now - 30000)
  $asks = if ($ask) { '[{"id":"ask-3","at":' + ($At - 90000) + ',"tool":"SendMessage","title":"Message Publishing the résumé in Website","detail":"Push the site now: the résumé is ready.\nThen tell me when it is live.","session":"website-a6","label":"Publishing the résumé","project":"Website"}]' } else { '[]' }
  $wait = if ($waiting) { @"
[{"id":"$($sidA):toolu_1","sessionId":"$sidA","session":"website-a6","label":"Publishing the résumé","project":"Website","kind":"text","extras":false,"gist":"Which layout?","at":$($At - 60000),
  "questions":null,"text":"Which layout for the résumé page, two columns like the printed one or one column?","answerable":true,"why":null},
 {"id":"$($sidB):$now","sessionId":"$sidB","session":"terminal-b1","label":"Pad colours","project":"Terminal","kind":"text","extras":false,"gist":"Q1","at":$Asked,
  "questions":[],"text":"Q1. Which colour for the pad?\n  1a. Orange\n  1b. Blue","answerable":true,"why":null}]
"@ } else { '[]' }
  @"
{"type":"seat.view","seq":1,"at":$At,
 "conductor":{"state":"on","mode":"$mode","listening":true,"pausedAt":null,"catchUp":false,"busy":false,"queued":0,"sessionId":"c0","startedAt":$($At - 600000),"model":"claude-opus-5-5","error":null},
 "asks":$asks,
 "sessions":[{"sessionId":"$sidA","name":"website-a6","project":"Website","status":"waiting","label":"Publishing the résumé","summary":"","question":"Which layout?","pane":null,"percent":40,"left":240,"agents":[],"conductor":"thinking"},
             {"sessionId":"$sidB","name":"terminal-b1","project":"Terminal","status":"working","label":"Pad colours","summary":"","question":null,"pane":null,"agents":[],"conductor":null}],
 "waiting":$wait,
 "events":[{"seq":811,"at":$($At - 120000),"kind":"finished","session":"terminal-b1","label":"Pad colours","project":"Terminal","text":"Built the pad","wake":false},
           {"seq":812,"at":$($At - 60000),"kind":"waiting","session":"website-a6","label":"Publishing the résumé","project":"Website","text":"asks which layout","wake":true}],
 "suggestions":[{"id":"g1","day":"2026-10-23","project":"Terminal","kind":"tooling","title":"Keep the worktree's target warm","change":"Build each new worktree once in the background as it's made.","evidence":"Six builds waited 11 minutes each.","saves":"About ten minutes a branch","status":"open","rating":null}],
 "notes":[{"id":"note-4","at":$($At - 45000),"text":"Publishing the résumé in Website needs your OK to push"}]}
"@
}

# What the window costs while nothing happens: once it has settled, the frames it paints and the
# GUI process's CPU over -RestSec, nothing sent or pressed meanwhile. The state file is read every
# 100 ms (reading it paints nothing) for when the last frame was painted.
function Measure-Rest([string]$what) {
  Wait-Settled -Animating -What "$what, settled" | Out-Null
  $p = Get-Process -Id $win.Pid
  $first = Read-State
  $frame0 = [long](Get-StateWindow $first -Hwnd ([long]$win.Handle)).frame
  $cpu0 = $p.TotalProcessorTime
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $last = $first
  while ($clock.ElapsedMilliseconds -lt $RestSec * 1000) {
    Start-Sleep -Milliseconds 100
    $s = Get-StateSnapshot $win.StateFile
    if ($s) { $last = $s }
  }
  $p.Refresh()
  $cpu = ($p.TotalProcessorTime - $cpu0).TotalSeconds
  $secs = $clock.Elapsed.TotalSeconds
  $frames = [long](Get-StateWindow $last -Hwnd ([long]$win.Handle)).frame - $frame0
  $lastView = (Get-StateWindow $last -Hwnd ([long]$win.Handle)).conductor_view
  $r = [pscustomobject]@{
    What = $what; Seconds = [Math]::Round($secs, 2); Frames = $frames; Fps = [Math]::Round($frames / $secs, 1)
    CpuSeconds = [Math]::Round($cpu, 3); CpuPercent = [Math]::Round(100 * $cpu / $secs, 1); LastFrameAt = [long]$last.time_ms
    Clock = if ($lastView) { $lastView.clock } else { $null }
  }
  "rest: $what, $($r.Frames) frames in $($r.Seconds) s ($($r.Fps) a second), GUI CPU $($r.CpuSeconds) s ($($r.CpuPercent)% of a core)" | Out-Host
  $r
}

$lua = @"
rawset(config, 'lbterm_conductor', {
  state_file = '$(ConvertTo-LuaPath $state)',
  seat_pipe = [[\\.\pipe\$pipeName]],
  menu = {
    { label = 'Open its view', action = wezterm.action.LbtermConductorView 'Show' },
  },
})
config.initial_cols = 150
config.initial_rows = 44
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
-- Workspace switches, as restore.ps1 has them: the view takes typing and editing keys, not these
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', action = wezterm.action.SwitchToWorkspace { name = 'other' } })
table.insert(config.keys, { key = 'F6', action = wezterm.action.SwitchToWorkspace { name = 'default' } })
"@

# The open tab's program: a spinner, as a working Claude session's is, while spin.go is there,
# and once light-under.go is there, claude_status lit orange (blocked on you) as claudehook.exe lights it
$spinGo = Join-Path $T.Out 'spin.go'
$lightUnder = Join-Path $T.Out 'light-under.go'
$lightBehind = Join-Path $T.Out 'light-behind.go'
Remove-Item -LiteralPath $spinGo, $lightUnder, $lightBehind -ErrorAction SilentlyContinue
$light = @'
$b = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('ff8c00|' + [DateTime]::UtcNow.ToFileTimeUtc()))
[Console]::Write([char]27 + ']1337;SetUserVar=claude_status=' + $b + [char]7)
'@
$spinner = @'
$i = 0; $lit = $false
while ($true) {
  if (-not $lit -and (Test-Path -LiteralPath 'LIGHT_GO')) { LIGHT; $lit = $true }
  if (Test-Path -LiteralPath 'SPIN_GO') { [Console]::Write("`r" + '|/-\'[$i % 4] + " Working $i "); $i++; Start-Sleep -Milliseconds 50 }
  else { Start-Sleep -Milliseconds 100 }
}
'@ -replace 'SPIN_GO', ($spinGo -replace "'", "''") -creplace 'LIGHT_GO', ($lightUnder -replace "'", "''") -creplace 'LIGHT', $light.Replace('$', '$$')
# A tab behind the view's that lights its claude_status the same once light-behind.go is there
$behind = ("while (-not (Test-Path -LiteralPath '{0}')) {{ Start-Sleep -Milliseconds 100 }}`n{1}`nStart-Sleep 900" -f ($lightBehind -replace "'", "''"), $light)
# The spinner on or off, and a moment for it to start or stop
function Set-Spinner([bool]$on) {
  if ($on) { [IO.File]::WriteAllText($spinGo, '') } else { Remove-Item -LiteralPath $spinGo -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 400
}

[LbtermFakeSeat]::Serve($pipeName)
Invoke-Test {
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "listening": true, "busy": false, "asks": 1 }')
  $script:win = Open-TestWindow -Lua $lua -PostedPointer -Program (New-PwshProgram $spinner)
  Start-PointerWatch $win
  # The test window's own pane, from its state: a config this build doesn't know all of opens a
  # window of its own saying so, whose pane `cli list` can name first
  $first = (Wait-State { param($s) $s.frame -gt 0 -and $s.rail.conductor -and $null -ne $s.active_pane } -What 'the window and the Conductor''s pad').active_pane
  # Two more tabs, one holding website-a6's session, as claudehook.exe says so, and one that
  # lights its claude_status when it's told to
  $paneA = New-Tab -From $first -NoActivate -Program (New-ClaudeSessionProgram $sidA)
  New-Tab -From $first -NoActivate -Program (New-PwshProgram $behind) | Out-Null

  # 1. LBTerm subscribes; the seat says it's on with an ask waiting: the pad goes orange
  if (-not (Wait-Until { (Sent 'seat.subscribe').Count -gt 0 } -TimeoutSec 15)) { throw 'LBTerm never subscribed to the seat' }
  $hello = (Sent 'seat.subscribe')[0]
  if ($hello.client -ne 'lbterm') { Add-Failure "it subscribed as $($hello.client), not lbterm" } else { "seat: subscribed as $($hello.client), pid $($hello.pid)" }
  Push (& $viewWith $true)
  Push (@"
{"type":"seat.items","reset":true,"items":[
 {"id":"u1","at":$($now - 50000),"who":"you","from":"lbterm","text":"what's waiting on me?","queued":false},
 {"id":"t1","at":$($now - 49000),"who":"tool","tool":{"name":"fleet","text":"fleet: 2 sessions, 1 waiting","status":"done"}},
 {"id":"r1","at":$($now - 48000),"who":"conductor","text":"Two things:\n- **website-a6** asks which layout the résumé page gets.\n- terminal-b1 asks which colour the pad takes: ``1a`` or ``1b``.\n\nI'd like to message website-a6 to push once you've picked."},
 {"id":"h1","at":$($now - 20000),"who":"hub","text":"Conductor hub: 1 thing since you last looked.\n- terminal-b1 finished a step"},
 {"id":"p1","at":$($now - 15000),"who":"peer","from":"website-a6","label":"Publishing the résumé","project":"Website","text":"The résumé page is built. May I push it?"},
 {"id":"n1","at":$($now - 10000),"who":"notice","text":"Read only from now on (Lachlan, LBTerm)"}]}
"@)
  $asks = Shows { param($s) $s.rail.conductor.asks -eq 1 } 'pad-asks' 'the pad orange with an ask'
  'pad: orange with 1 ask'

  if ($Measure) {
    # The view closed, then open, at rest, every time hours old so nothing on it changes for
    # hours; then both again with the spinner going in the open tab, under the view
    $pad = Get-RectCenter $script:entry.rail.conductor.pad
    $old = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - 3 * 3600000
    Push (& $viewWith $true 'read' $true $old ($old - 30000))
    $results = [Collections.Generic.List[object]]::new()
    for ($run = 1; $run -le $Runs; $run++) {
      foreach ($spin in $false, $true) {
        Set-Spinner $spin
        $under = if ($spin) { ', a spinner in the tab under it' } else { '' }
        $results.Add((Measure-Rest "the view closed$under"))
        Send-Click $win $pad[0] $pad[1] -Move
        Send-Leave $win
        Wait-State { param($s) $s.conductor_view -and @($s.conductor_view.items).Count -eq 6 -and $s.rail.conductor.open } -What 'the view open' | Out-Null
        $open = Measure-Rest "the view open$under"
        $results.Add($open)
        if ($open.Frames -gt 0) { Add-Failure "run $($run): the view open$under painted $($open.Frames) frames in $($open.Seconds) s, with nothing on it to change for hours" }
        Send-Key $win Escape
        Wait-State { param($s) -not $s.conductor_view } -What 'the view closed' | Out-Null
      }
      Set-Spinner $false
    }
    $results | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $T.Out 'rest.json') -Encoding utf8NoBOM
    return
  }

  # 2. A left click on the pad opens the view over the open tab
  $pad = Get-RectCenter $script:entry.rail.conductor.pad
  Send-Click $win $pad[0] $pad[1] -Move
  Send-Leave $win
  $view = Shows { param($s) $s.conductor_view -and $s.conductor_view.connected -and @($s.conductor_view.items).Count -eq 6 -and $s.rail.conductor.open } 'view' 'the view open, the pad carrying the open tab''s ring'
  $v = $script:entry.conductor_view
  if ($script:entry.rail.open_row -ne $null) { Add-Failure "a tab's row still has the open tab's ring: row $($script:entry.rail.open_row)" }
  $kinds = @($v.cards | ForEach-Object kind) -join ','
  if ($kinds -ne 'ask,waiting,waiting,session,session,events,suggestion,notes') { Add-Failure "the side's cards: $kinds" } else { "view: open, its cards $kinds" }
  if ($v.state -ne 'on' -or $v.mode -ne 'read') { Add-Failure "the view says $($v.state), $($v.mode)" }
  if ($v.focus -ne 'main') { Add-Failure "typing goes to $($v.focus), not the box" }
  foreach ($name in 'allow:ask-3', 'deny:ask-3', 'control:mode most', 'send', 'rsi:g1:useful', "chip:$($sidB):$($now):1b") { [void](Button $v $name) }
  # Its words are drawn, not only its fills: the Conductor's reply, his words, a header button,
  # a card's button and the ask's card
  Test-Words $view (@($v.items | Where-Object id -eq 'r1')[0].rect) 'the Conductor''s reply'
  Test-Words $view (@($v.items | Where-Object id -eq 'u1')[0].rect) 'his words in their bubble'
  Test-Words $view (Button $v 'control:mode read').rect 'the header''s Read only'
  Test-Words $view (Button $v 'allow:ask-3').rect 'the ask''s Allow'
  Test-Words $view (@($v.cards | Where-Object kind -eq 'ask')[0].rect) 'the ask''s card'

  # At rest: the same again, every time hours old but the second question, asked 40 s ago, and
  # the spinner going in the tab under the view. The view's clock (lbterm/STATE.md,
  # conductor_view.clock) is when that question's "just now" turns to "1 min ago", 45 s after it
  # was asked; that's the one frame the view may paint, the spinner's none
  Set-Spinner $true
  $restAt = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  $asked = $restAt - 40000
  Push (& $viewWith $true 'read' $true ($restAt - 3 * 3600000) $asked)
  $tick = $asked + 45000
  # The days' names change at midnight too, which comes first once a day
  $midnight = [DateTimeOffset]::new([DateTime]::Today.AddDays(1)).ToUnixTimeMilliseconds()
  if ($midnight -lt $asked + 120000) {
    "rest: midnight comes before the question's clock would, so its check waits for another run"
  } else {
    Wait-State { param($s) $s.conductor_view.clock -eq $tick } -What "the view's clock at the question's 45 s ($tick)" | Out-Null
    $rest = Measure-Rest 'the view open, a question turning 45 s old'
    if ($rest.Frames -ne 1) { Add-Failure "the view at rest painted $($rest.Frames) frames in $($rest.Seconds) s: one was due, the clock's at 45 s" }
    if ($rest.LastFrameAt -lt $tick) { Add-Failure "the view's last frame at rest was at $($rest.LastFrameAt), before its clock's ${tick}: 'just now' never turned to '1 min ago'" }
    elseif ($rest.Clock -ne $asked + 90000) { Add-Failure "after the tick the view's clock says $($rest.Clock), not the question's 90 s ($($asked + 90000))" }
    else { "rest: one frame, the clock's, then the clock at the question's 90 s; the spinner under it, none" }
  }
  Set-Spinner $false
  if ((Read-State).windows[0].scroll_strip) { Add-Failure "the scroll strip is drawn beside the view: it's the scrollbar of the pane the view covers" }

  # Two refusals 3 s apart, with nothing else to paint the window: the second notice goes 12 s
  # after it came, by itself. Its end was never woken while the first's was still due, so it
  # stayed until something else painted (until 2026-10-30)
  $before = (Sent 'seat.say').Count
  foreach ($n in 1, 2) {
    [LbtermFakeSeat]::Refuse = 'seat.say'
    if ($n -eq 1) {
      Send-Chars $win 'twice'
      Wait-State { param($s) $s.conductor_view.boxes[0].text -eq 'twice' } -What 'the words to be refused' | Out-Null
    }
    $f = Get-StateFrame $win
    Send-Key $win Enter
    if (-not (Next-Sent 'seat.say' ($before + $n - 1))) { Add-Failure "refusal ${n}: Enter sent no seat.say"; break }
    Wait-State { param($s) $s.conductor_view.boxes[0].text -eq 'twice' -and $s.conductor_view.notice -match 'Refused' } -AfterFrame $f -What "refusal ${n}: the words back and the notice" | Out-Null
    $noticeAt = [Diagnostics.Stopwatch]::StartNew()
    if ($n -eq 1) { Start-Sleep -Milliseconds 3000 }
  }
  try {
    Wait-State { param($s) -not $s.conductor_view.notice } -What 'the second notice gone by itself' -TimeoutSec 15 | Out-Null
    $went = $noticeAt.Elapsed.TotalSeconds
    if ($went -lt 11) { Add-Failure "the second notice went $([Math]::Round($went, 1)) s after it came: the first's end took it, 12 s were due" }
    else { "notices: two 3 s apart, the second gone $([Math]::Round($went, 1)) s after it came, by itself" }
  } catch { Add-Failure "the second of two notices 3 s apart was still on the view 15 s after it came, with nothing else to paint the window" }
  Send-Key $win Back, Back, Back, Back, Back
  Wait-State { param($s) $s.conductor_view.boxes[0].text -eq '' } -What 'the box empty again' | Out-Null

  # claudehook's light beside the view: the tab under it, then a tab behind it, light
  # claude_status orange and print nothing else, and each pad says so within a second, with
  # nothing else to paint the window (the panes under the view don't repaint it, and until
  # 2026-10-30 the light didn't either: the tab under the view showed "Your turn" for a question)
  foreach ($case in @(@{ Flag = $lightUnder; Tab = 0; What = 'the tab under the view' }, @{ Flag = $lightBehind; Tab = 2; What = 'a tab behind the view' })) {
    $row = @(@((Read-State).windows[0].rail.rows) | Where-Object tab_index -eq $case.Tab)[0]
    if (-not $row) { Add-Failure "no rail row for $($case.What) (tab $($case.Tab))"; continue }
    if ($row.state -eq 'waiting') { Add-Failure "$($case.What)'s pad was orange before its light" }
    [IO.File]::WriteAllText($case.Flag, '')
    $lit = [Diagnostics.Stopwatch]::StartNew()
    try {
      Wait-State { param($s) @(@($s.rail.rows) | Where-Object { $_.tab_index -eq $case.Tab -and $_.state -eq 'waiting' }).Count -gt 0 } -What "$($case.What)'s pad orange" -TimeoutSec 1 -PresentMs 0 | Out-Null
      "light: $($case.What)'s pad orange $($lit.ElapsedMilliseconds) ms after its claude_status lit"
    } catch { Add-Failure "$($case.What)'s pad wasn't orange a second after its claude_status lit, the view open" }
  }

  # 3. The reply streams in, then the item replaces it
  Push '{"type":"seat.delta","id":"r2","text":"Looking at"}'
  Push '{"type":"seat.delta","id":"r2","text":"Looking at **website-a6** now: it has"}'
  $stream = Shows { param($s) $s.conductor_view.streaming -eq 'r2' -and (@($s.conductor_view.items | Where-Object id -eq 'r2')[0].text -match 'it has$') } 'streaming' 'the reply streaming'
  'stream: the reply so far shows, marked as being written'
  Push ('{"type":"seat.items","items":[{"id":"r2","at":' + $now + ',"who":"conductor","text":"Looking at **website-a6** now: it has the résumé ready."}]}')
  Wait-State { param($s) -not $s.conductor_view.streaming -and (@($s.conductor_view.items | Where-Object id -eq 'r2')[0].text -match 'ready\.$') } -What 'the finished reply' | Out-Null
  'stream: the finished item replaced it'

  # 4. Allow sends seat.allow; the next view drops the ask and the pad's orange
  $before = (Sent 'seat.allow').Count
  Press 'allow:ask-3'
  $allow = Next-Sent 'seat.allow' $before
  if (-not $allow -or $allow.ask -ne 'ask-3' -or $allow.by -ne 'lbterm') { Add-Failure "Allow sent: $($allow | ConvertTo-Json -Compress)" } else { "allow: seat.allow for ask-3, by lbterm" }
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "listening": true, "busy": false, "asks": 0 }')
  Push (& $viewWith $false)
  Wait-State { param($s) $s.rail.conductor.asks -eq 0 -and -not @($s.conductor_view.cards | Where-Object kind -eq 'ask').Count } -What 'the ask gone' | Out-Null
  'allow: the ask went, and the pad''s orange with it'

  # 5. Typing and Enter: seat.say with the words, the box emptied
  Send-Chars $win 'hello'
  Send-Key $win Space
  Send-Chars $win 'conductor'
  $typed = Shows { param($s) $s.conductor_view.boxes[0].text -eq 'hello conductor' } 'typed' 'the words in the box'
  Test-Words $typed $script:entry.conductor_view.boxes[0].rect 'the box''s typed words'
  $before = (Sent 'seat.say').Count
  Send-Key $win Enter
  $say = Next-Sent 'seat.say' $before
  if (-not $say -or $say.text -ne 'hello conductor' -or $say.from -ne 'lbterm') { Add-Failure "Enter sent: $($say | ConvertTo-Json -Compress)" } else { 'say: Enter sent "hello conductor", from lbterm' }
  Wait-State { param($s) $s.conductor_view.boxes[0].text -eq '' } -What 'the box emptied' | Out-Null
  # The seat's echo, the turn under way, and words from the phone queued after it
  Push ('{"type":"seat.items","items":[{"id":"u2","at":' + $now + ',"who":"you","from":"lbterm","text":"hello conductor","queued":false},' +
    '{"id":"t3","at":' + $now + ',"who":"tool","tool":{"name":"read","text":"read website-a6: 40 lines","status":"running"}},' +
    '{"id":"u3","at":' + $now + ',"who":"you","from":"phone","text":"and push it when it is ready","queued":true}]}')
  Push ((& $viewWith $false) -replace '"busy":false,"queued":0', '"busy":true,"queued":1')
  Shows { param($s) $s.conductor_view.busy -and @($s.conductor_view.items | Where-Object queued).Count -eq 1 } 'said' 'his words in, the turn running, a message queued' | Out-Null
  'say: his words in the conversation, a message queued after the turn, Stop showing'
  $before = (Sent 'seat.interrupt').Count
  Press 'stop'
  $stop = Next-Sent 'seat.interrupt' $before
  if (-not $stop -or $stop.by -ne 'lbterm') { Add-Failure "Stop sent: $($stop | ConvertTo-Json -Compress)" } else { 'stop: seat.interrupt, by lbterm' }
  # The seat refuses the next: the words go back in the box and the notice says why
  [LbtermFakeSeat]::Refuse = 'seat.say'
  Send-Chars $win 'again'
  Wait-State { param($s) $s.conductor_view.boxes[0].text -eq 'again' } -What 'the second words' | Out-Null
  Send-Key $win Enter
  Shows { param($s) $s.conductor_view.boxes[0].text -eq 'again' -and $s.conductor_view.notice -match 'Refused' } 'refused' 'the refused words back, the notice saying why' | Out-Null
  'say: refused, the words came back and the notice says why'
  # Emptied again for what follows
  Send-Key $win Back, Back, Back, Back, Back
  Wait-State { param($s) $s.conductor_view.boxes[0].text -eq '' } -What 'the box empty' | Out-Null

  # 6. A control in the header
  $before = (Sent 'seat.control').Count
  Press 'control:mode most'
  $control = Next-Sent 'seat.control' $before
  if (-not $control -or $control.action -ne 'mode most') { Add-Failure "Accept most sent: $($control | ConvertTo-Json -Compress)" } else { 'control: seat.control mode most' }

  # 7. The waiting cards: a chip into the text question's box, then Send
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "most", "listening": true, "busy": false, "asks": 0 }')
  Push (& $viewWith $false 'most')
  Wait-State { param($s) $s.conductor_view.mode -eq 'most' } -What 'accept most' | Out-Null
  # Each chip's code on a line of its own, as Relay's phone puts them, and out again with its line
  $box = "answer:$($sidB):$now"
  foreach ($step in @(@('1b', '1b'), @('1a', "1b`n1a"), @('1b', '1a'), @('1b', "1a`n1b"))) {
    Press "chip:$($sidB):$($now):$($step[0])"
    $want = $step[1]
    Wait-State { param($s) @($s.conductor_view.boxes | Where-Object name -eq $box)[0].text -eq $want } -What "the box holding '$($want -replace "`n", ' / ')' after $($step[0])" | Out-Null
  }
  $before = (Sent 'seat.answer').Count
  Press "answer:$($sidB):$now"
  $answer = Next-Sent 'seat.answer' $before
  if (-not $answer -or $answer.text -ne "1a`n1b" -or $answer.question -ne "$($sidB):$now") { Add-Failure "the text answer: $($answer | ConvertTo-Json -Compress -Depth 5)" } else { 'answer: the text question, 1a and 1b on lines of their own' }
  Get-Shot $win -Name 'answered' | Out-Null

  # 8. A suggestion's rating
  $before = (Sent 'seat.rsi').Count
  Press 'rsi:g1:useful'
  $rsi = Next-Sent 'seat.rsi' $before
  if (-not $rsi -or $rsi.action -ne 'rate' -or $rsi.rating -ne 'useful' -or $rsi.suggestion -ne 'g1') { Add-Failure "Useful sent: $($rsi | ConvertTo-Json -Compress)" } else { 'rsi: rate useful for g1' }

  # 8b. A seat that hangs: it reads -Hung messages and never answers, their connections left open.
  # Each says it got no answer once its wait (15 s) is over, and nothing of LBTerm's is left waiting
  # on it: the GUI has no more threads or handles than before (until 2026-10-30 each command started
  # two threads, and the one reading its reply stayed blocked for as long as the seat kept the
  # connection, with its handle)
  [LbtermFakeSeat]::Silent = $true
  $gui = Get-Process -Id $win.Pid
  $threads0 = $gui.Threads.Count; $handles0 = $gui.HandleCount
  $before = (Sent 'seat.say').Count
  for ($n = 1; $n -le $Hung; $n++) {
    Send-Chars $win 'q'
    Wait-State { param($s) $s.conductor_view.boxes[0].text -eq 'q' } -What "hung message $($n): the word typed" | Out-Null
    Send-Key $win Enter
    if (-not (Next-Sent 'seat.say' ($before + $n - 1))) { Add-Failure "hung message $($n): Enter sent no seat.say"; break }
    Wait-State { param($s) $s.conductor_view.boxes[0].text -eq '' } -What "hung message $($n): the box emptied" | Out-Null
  }
  $lastSent = [Diagnostics.Stopwatch]::StartNew()
  $gui.Refresh()
  $threadsWaiting = $gui.Threads.Count; $handlesWaiting = $gui.HandleCount
  # Every wait over, and a moment for what ended to go
  try { Wait-State { param($s) $s.conductor_view.notice -match "didn't answer" } -What 'the hung messages'' notice' -TimeoutSec 20 | Out-Null }
  catch { Add-Failure "$Hung messages to a seat that never answers said nothing 20 s on" }
  $left = 17000 - $lastSent.ElapsedMilliseconds
  if ($left -gt 0) { Start-Sleep -Milliseconds $left }
  $gui.Refresh()
  $threads1 = $gui.Threads.Count; $handles1 = $gui.HandleCount
  $script:hungCost = [pscustomobject]@{ messages = $Hung; threads_before = $threads0; threads_waiting = $threadsWaiting; threads_after = $threads1; handles_before = $handles0; handles_waiting = $handlesWaiting; handles_after = $handles1 }
  $script:hungCost | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $T.Out 'hung.json') -Encoding utf8NoBOM
  "hung: $Hung messages, threads $threads0, $threadsWaiting while they waited, $threads1 after; handles $handles0, $handlesWaiting, $handles1"
  if ($threads1 - $threads0 -gt 2) { Add-Failure "a seat that never answered $Hung messages left the GUI $($threads1 - $threads0) threads more than before them" }
  if ($handles1 - $handles0 -gt 4) { Add-Failure "a seat that never answered $Hung messages left the GUI $($handles1 - $handles0) handles more than before them" }
  [LbtermFakeSeat]::Silent = $false
  # The first message refused by no answer came back in the box (it was empty then): emptied again
  Send-Key $win Back, Back
  Wait-State { param($s) $s.conductor_view.boxes[0].text -eq '' } -What 'the box empty after the hung messages' | Out-Null

  # 9. A session's card opens its tab, the view leaving for it. Nothing of the tab under the view
  # showed, so its panes go at once, not dissolving over the tab that opens (until 2026-10-30 they
  # showed whole for a frame and faded out)
  $script:dissolved = $false
  $watchDissolve = { param($sw) if (@($sw.animations | ForEach-Object name) -contains 'pane.dissolve') { $script:dissolved = $true } }
  Press "session:$sidA"
  $opened = Wait-Change { param($s) -not $s.conductor_view -and @($s.panes | Where-Object { $_.session -eq $sidA }).Count } -What 'website-a6''s tab open, the view gone' -Animating -OnState $watchDissolve
  'session: its card opened its tab and the view left'
  if ($script:dissolved) { Add-Failure 'session: the panes under the view dissolved over the tab its card opened, though they never showed' }

  # 10. seat.show opens it again; a click on a tab's row leaves it for that tab; opened again, Esc
  # with the box empty leaves it for the tab under it
  Push '{"type":"seat.show"}'
  $shown = Wait-State { param($s) $s.conductor_view -and $s.rail.conductor.open } -What 'the view open from seat.show'
  'show: seat.show opened the view'
  $row = @($shown.rail.rows)[0]
  $x, $y = Get-RectCenter $row.pad
  $script:dissolved = $false
  Send-Click $win $x $y -Move
  Send-Leave $win
  Wait-Change { param($s) -not $s.conductor_view -and $s.active_tab -eq $row.tab_index } -What 'the first row''s tab open, the view gone' -Animating -OnState $watchDissolve | Out-Null
  'row: a click on the first row left the view for its tab'
  if ($script:dissolved) { Add-Failure 'row: the panes under the view dissolved over the tab its row opened, though they never showed' }
  Send-Click $win $pad[0] $pad[1] -Move
  Send-Leave $win
  Wait-State { param($s) $s.conductor_view } -What 'the view open again from the pad' | Out-Null
  # 10b. A workspace switch (the frontend's SwitchToMuxWindow: the window shows another mux
  # window) leaves the view, as a tab opened any other way does, so the view is never open over a
  # mux window it wasn't listed under for the seat's repaints (seat_client.rs); and the state file
  # says the window as it is, not also as it was. Back in the first workspace, the pad opens the
  # view again
  Send-Key $win F9
  Wait-State { param($s) -not $s.conductor_view -and @($s.tabs).Count -eq 1 } -What 'the other workspace, the view left' | Out-Null
  $listed = @((Read-State).windows | Where-Object { [long]$_.hwnd -eq [long]$win.Handle }).Count
  if ($listed -ne 1) { Add-Failure "workspace: the state file says the window $listed times after a workspace switch" }
  'workspace: a workspace switch left the view'
  Send-Key $win F6
  Wait-State { param($s) @($s.tabs).Count -gt 1 } -What 'back in the first workspace' | Out-Null
  Wait-Settled -Animating -What 'back in the first workspace, settled' | Out-Null
  Send-Click $win $pad[0] $pad[1] -Move
  Send-Leave $win
  Wait-State { param($s) $s.conductor_view } -What 'the view open again from the pad, back in the first workspace' | Out-Null
  Send-Key $win Escape
  $closed = Shows { param($s) -not $s.conductor_view -and $s.rail.open_row -ne $null } 'closed' 'the view closed, a tab''s row open again'
  'esc: the box empty, Esc left the view'
  if (-not $script:entry.scroll_strip) { Add-Failure "the scroll strip didn't come back once the view left" } else { 'scroll strip: back once the view left' }

  # 11. A narrow window keeps its side column
  Set-WindowSize $win 860 760
  # The pane's size readout from the resize gone first: it's the pane's, not the view's
  Wait-State { param($s) $s.size.width -lt 900 } -What 'the window narrower' | Out-Null
  try { Wait-State { param($s) $s.readout } -What 'the readout' -TimeoutSec 2 | Out-Null } catch { }
  Wait-State { param($s) -not $s.readout } -What 'the readout gone' | Out-Null
  Send-Click $win $pad[0] $pad[1] -Move
  Send-Leave $win
  Shows { param($s) $s.conductor_view -and $s.size.width -lt 900 -and @($s.conductor_view.cards | Where-Object { $_.rect }).Count -gt 0 } 'narrow' 'the view in a narrow window, its side column there' | Out-Null
  "narrow: $($script:entry.size.width) px across, the side column's first card at x $(@($script:entry.conductor_view.cards | Where-Object { $_.rect })[0].rect.x)"
} -Passed 'conductor-view: the view opens from the pad, shows what the seat says, streams the reply, and sends what''s pressed and typed'
