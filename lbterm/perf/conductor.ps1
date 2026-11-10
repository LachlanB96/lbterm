#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/conductor_hub.rs, wezterm-gui/src/state_files.rs, lbterm-extras/src/conductor.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/lights.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/compositor.rs
<#
The Conductor on the tab rail (`lbterm_conductor`; wezterm-gui/src/termwindow/render/conductor.rs,
conductor_hub.rs, state_files.rs; lbterm-extras/src/conductor.rs) end to end, in a test window that
looks like Lachlan's LBTerm (New-TestConfig), with its state file in target\perf\conductor and its
hub played here, on a pipe of its own: it writes down every line LBTerm sends and pushes focus
lines when asked. Two more tabs say they hold Claude sessions (claudehook.exe's `claude_session`
user var).

1. The hub: LBTerm subscribes, and says which tab and pane holds each session.
2. No state file: the pad atop the rail is dim, "off" (off.png). On, read only: a grey outline and
   the mode, in the slot alone (read.png). Accept most and Accept all at rest (Lachlan's 7a of
   2026-11-01): a still hairline in the mode's colour, yellow and red, measured in a band left of
   the pad inside its pill, with none of the spectrum's blues there, nothing moving between two
   pictures 700 ms apart (most.png, most-later.png, all.png, all-later.png), the state file's
   outline `mode` and no `conductor.outline` among the window's loops.
3. The hub says it's thinking about the first session: that tab's pad gets the iridescent outline,
   moving (thinking.png), and the Conductor's pad stays still: thinking isn't acting. Acting on the
   second: its pad too, a light going round it (acting.png), and the Conductor's pad lights, its
   spectrum fading in over the still outline (`conductor.outline.fade` in flight), its hues
   turning; lit in Accept all too, and in read only the grey outline, the ring gone at once. The
   hub lets go of both: they linger a lap at most, then go, and the pad's spectrum fades out to the
   still outline, the rail as it was before (let-go.png), no loop left. Since feat/dcomp-recorder
   what the compositor does with the pad's ring (its fades on their curve, its hues' angle a loop
   only while it shows, `conductor.outline` in `loops` then and gone with it, in read only gone at
   once) is compositor/imp/tests/rail.rs's; this step keeps the fades' names (the hub's push
   reaching the compositor), no loop at rest or once let go (render dropping the ring), the read
   only's no fade, and the pixels: the outline's colours, the hues seen turning, the rail as it was.
4. A right click on the pad drops its menu. A click on an item that waits for its answer runs it
   (its row flashing white, menu.conductor.flash, Lachlan's 9b),
   and the menu stays, the item's arc turning and the others out of reach (pending.png,
   pending-later.png), until the state file changes; the arc is among the window's loops while it
   waits and gone from them once the menu closes, where it went on turning, hidden, for good
   (the compositor's own, lbterm/STATE.md: perf/paint). An item that doesn't wait, picked with the
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
  pwsh -File lbterm\perf\conductor.ps1 -Capture   # step 4a's answer recorded, then it stops

-Capture runs the window with motion at -Slowdown (5) and, at step 4a's answer, records it frame
by frame as motion\capture.ps1 does: the menu closing while Accept most waits, its card fading out
on pane_exit with the waiting arc in it, held still at its angle (MenuSpinner::hold; the
compositor's Conductor slot), to target\motion\conductor-spinner with its contact sheet. It ends
there: what follows is checked at real speed only.

-CapturePad records the Conductor's pad in Accept most as the hub starts acting on a session and
lets go (Lachlan's 7a): the spectrum's ring fading in over the still yellow outline on
`panel_glow`, its hues turning, and fading back out to it, at -Slowdown (1 for real speed),
-MotionOff for none (there and gone in the next frame), each frame saved as it's taken to
target\motion\<CaptureName>, with two sheets from timesheet.py, fade-in.png and fade-out.png, of the
pad three times as big at real-speed times from each push. It says how long the PC had had no
real input first (PITFALLS 239: with the displays asleep DWM plays nothing into a capture). It ends
there, after step 2. With -Interrupt the hub acts again 150 ms (of real speed) into the fade out
and lets go after, so the ring turns round from where it is and fades back in, never from nothing
(turn-back.png, from the push, and fade-out-again.png, the last fade).

  pwsh -File lbterm\perf\conductor.ps1 -CapturePad -Slowdown 1 -CaptureName conductor-pad-1x
#>
param([string]$Bin, [switch]$Capture, [double]$Slowdown = 5, [double]$Seconds = 2,
  [switch]$CapturePad, [string]$CaptureName = 'conductor-pad', [switch]$MotionOff, [switch]$Interrupt)
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
if ($MotionOff) { $lua += "`nconfig.lbterm_motion = { enabled = 'off' }`n" }
elseif ($Capture -or $CapturePad) { $lua += "`nconfig.lbterm_motion = { enabled = 'on', slowdown = $Slowdown }`n" }

# The pad's outline, read in a band left of the pad, inside its pill, where only the outline is
# drawn (its label is under the pad, the tabs below its slot): the mode's still hairline, or the
# spectrum's ring with its glow
$yellow = New-ColourRange 200 255 180 255 40 170   # palette::MODE_MOST, #F2E86D, and its soft edge
$red = New-ColourRange 200 255 30 150 20 140       # palette::MODE_ALL, #FF5449
$cool = New-ColourRange 0 160 0 255 170 255        # the spectrum's blues, cyans and violets

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
  # The band left of the pad, inside its pill, where its outline is drawn and nothing else
  $band = New-Rect ([int][Math]::Max(0, $padRect.x - (ConvertTo-Px 14 $win.Scale))) ([int]$padRect.y) (ConvertTo-Px 13 $win.Scale) ([int]$padRect.h)
  $edge = { param($shot, $range) [LbtermE2E.Px]::CountRange($shot, $band, $range) }
  # And the spectrum's blues anywhere round the pad, over its slot: its hues run once round the
  # pill and DWM turns them, so a band on one side can be all reds for a moment, never the whole
  # slot; at rest nothing there is blue (the pad's dark, its glyph white, its word and outline
  # the mode's colour)
  $top = [int][Math]::Max(0, $padRect.y - (ConvertTo-Px 16 $win.Scale))
  $slotBox = New-Rect 0 $top (ConvertTo-Px 90 $win.Scale) ([int]$tabs - 1 - $top)
  $blues = { param($shot) [LbtermE2E.Px]::CountRange($shot, $slotBox, $cool) }
  "read: in the band left of the pad, $(& $edge $read $yellow) yellow and $(& $edge $read $red) red pixels; $(& $blues $read) of the spectrum's blues round it"
  if ($script:entry.rail.conductor.outline -ne 'grey') { Add-Failure "read only, the state file's outline is '$($script:entry.rail.conductor.outline)', not grey" }
  # Accept most and Accept all at rest (his 7a): the mode's colour, still, no loop for DWM. Its
  # words go out as the report's lines; its picture is left in $script:restShot
  $rest = { param([string]$mode, $colour, [string]$colourName)
    [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "' + $mode + '", "tooltip": "Conductor is on (accept ' + $mode + '), here only since 09:05" }')
    $shot = Shows { param($s) $s.rail.conductor.level -eq $mode }.GetNewClosure() $mode "the pad accepting $mode"
    $c = $script:entry.rail.conductor
    if ($c.outline -ne 'mode' -or $c.acting) { Add-Failure "accept $mode at rest: the state file's outline is '$($c.outline)', acting $($c.acting), not a still 'mode'" }
    if (@($script:entry.loops) -contains 'conductor.outline') { Add-Failure "accept $mode at rest: conductor.outline among the loops ($(@($script:entry.loops) -join ', ')), DWM composing every refresh for it" }
    Start-Sleep -Milliseconds 700
    $later = Get-Shot $win -Name "$mode-later"
    $n = & $edge $shot $colour
    $cold = & $blues $shot
    "${mode}: at rest, $n $colourName pixels in the band left of the pad and $cold of the spectrum's blues round it; loops $(@($script:entry.loops) -join ', ')"
    if ($n -lt (ConvertTo-Px 8 $win.Scale)) { Add-Failure "accept $mode at rest: only $n $colourName pixels in the band left of the pad, no outline in the mode's colour" }
    if ($cold -gt 0) { Add-Failure "accept $mode at rest: $cold of the spectrum's blues round the pad" }
    $d = Changed $shot $later 0 0 90 $tabs
    if ($d) { Add-Failure "accept $mode at rest: the pad moved in 700 ms at $(Format-Box $d)" } else { "${mode}: still in 700 ms" }
    if (Changed $shot $later 0 $tabs 90 100000) { Add-Failure "accept ${mode}: the tabs changed at rest" }
    $script:restShot = $shot
  }
  & $rest 'most' $yellow 'yellow'
  $most = $script:restShot
  $d = Changed $read $most 0 0 100000 $tabs
  if (-not $d) { Add-Failure 'accept most never showed' } else { "most: the outline and the word at $(Format-Box $d)" }
  & $rest 'all' $red 'red'
  # Back to most, the pad as step 3 starts from
  & $rest 'most' $yellow 'yellow'
  $most = $script:restShot

  if ($CapturePad) {
    # The pad as the hub starts acting and lets go, frame by frame, saved as they're taken
    $out = Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) "target\motion\$CaptureName"
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $out | Out-Null
    $slow = if ($MotionOff) { 1 } else { $Slowdown }
    "capture: $([LbtermE2E.Win]::IdleMs()) ms since the last real key or mouse input; locked: $(Test-WorkstationLocked)"
    $acts = '{"type":"conductor.focus","sessions":[{"sessionId":"' + $sids[1] + '","doing":"acting"}]}'
    $inMs = 300
    # Let go once the fade in is long done; it lights for its lap at least and lingers a moment
    # (lbterm_extras::conductor, ACTING_AT_LEAST, LINGER), then fades out
    $letGoMs = $inMs + [Math]::Max(1500, 900 * $slow)
    $outMs = [Math]::Max($letGoMs + 400, $inMs + 1400)
    # -Interrupt: acting again while it fades out, and let go again after (lbterm_extras::conductor's
    # PadRing: lit again while it fades, it fades back in from where it is)
    $backMs = $outMs + 150 * $slow
    $againMs = $backMs + [Math]::Max(1500, 900 * $slow)
    $out2Ms = $againMs + 400
    $endMs = $(if ($Interrupt) { $out2Ms } else { $outMs }) + 1000 * $slow + 600
    $h = Resolve-Handle $win
    $csv = [Collections.Generic.List[string]]::new(); $csv.Add('frame,ms')
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $pushed = 0; $n = 0
    while ($clock.ElapsedMilliseconds -lt $endMs) {
      if ($pushed -eq 0 -and $clock.ElapsedMilliseconds -ge $inMs) { [void][LbtermConductorHub]::Push($acts); $pushed = 1 }
      if ($pushed -eq 1 -and $clock.ElapsedMilliseconds -ge $letGoMs) { [void][LbtermConductorHub]::Push('{"type":"conductor.focus","sessions":[]}'); $pushed = 2 }
      if ($Interrupt -and $pushed -eq 2 -and $clock.ElapsedMilliseconds -ge $backMs) { [void][LbtermConductorHub]::Push($acts); $pushed = 3 }
      if ($pushed -eq 3 -and $clock.ElapsedMilliseconds -ge $againMs) { [void][LbtermConductorHub]::Push('{"type":"conductor.focus","sessions":[]}'); $pushed = 4 }
      $shot = [LbtermE2E.Win]::Capture($h)
      $ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 1)
      if ($shot) {
        $shot.Save((Join-Path $out ('frame{0:D3}.png' -f $n)))
        $shot.Dispose()
        $csv.Add("$n,$ms"); $n++
      }
    }
    Watch-Foreground
    $csv | Set-Content (Join-Path $out 'frames.csv')
    "capture: $n frames in $([Math]::Round($endMs / 1000, 1)) s to $out (acting at $inMs ms, let go at $letGoMs, fading out from about $outMs$(if ($Interrupt) { ", acting again at $backMs, let go at $againMs, fading out from about $out2Ms" }))"
    # Sheets of the pad, three times as big, at times of real speed from each push
    $x = [int][Math]::Max(0, $padRect.x - (ConvertTo-Px 18 $win.Scale)); $y = [int][Math]::Max(0, $padRect.y - (ConvertTo-Px 14 $win.Scale))
    $w = [int]$padRect.w + (ConvertTo-Px 36 $win.Scale); $hh = [int]$padRect.h + (ConvertTo-Px 34 $win.Scale)
    $steps = @(-60, 0, 40, 80, 120, 160, 200, 260, 320, 400, 500, 700)
    $sheets = @(@('fade-in.png', $inMs), @('fade-out.png', $outMs))
    if ($Interrupt) { $sheets += , @('turn-back.png', $backMs); $sheets += , @('fade-out-again.png', $out2Ms) }
    foreach ($sheet in $sheets) {
      $at = ($steps | ForEach-Object { [Math]::Round($sheet[1] / $slow + $_) }) -join ','
      python (Join-Path $PSScriptRoot '..\motion\timesheet.py') $out (Join-Path $out $sheet[0]) $x $y $w $hh 3 $slow $at 6
    }
    return
  }

  # 3. Thinking about the first session, acting on the second, then neither
  $focus = { param($list) '{"type":"conductor.focus","sessions":[' + (($list | ForEach-Object { '{"sessionId":"' + $_[0] + '","doing":"' + $_[1] + '"}' }) -join ',') + ']}' }
  $n = [LbtermConductorHub]::Push((& $focus @(, @($sids[0], 'thinking'))))
  if ($n -lt 1) { Add-Failure 'no subscriber to push to' }
  $thinking = Shows { param($s) @($s.rail.rows | Where-Object conductor -eq 'thinking').Count -eq 1 } 'thinking' 'a row the Conductor thinks about'
  $c = $script:entry.rail.conductor
  if ($c.outline -ne 'mode' -or $c.acting) { Add-Failure "thinking about a session lit the Conductor's pad: outline '$($c.outline)', acting $($c.acting)" }
  if (Changed $most $thinking 0 0 90 $tabs) { Add-Failure 'thinking about a session changed the Conductor''s pad' } else { 'thinking: the Conductor''s pad stays still' }
  $d = Changed $most $thinking 0 $tabs 90 100000
  if (-not $d) { Add-Failure 'thinking: no outline on the first session''s tab' } else { "thinking: an outline at $(Format-Box $d)" }
  $thinkingTop = if ($d) { $d[1] } else { 0 }
  Start-Sleep -Milliseconds 700
  if (-not (Changed $thinking (Get-Shot $win -Name 'thinking-later') 0 $tabs 90 100000)) { Add-Failure 'thinking: the outline never moved' }
  # What plays as the pad lights and goes out, frame by frame from the state file
  $played = [System.Collections.Generic.HashSet[string]]::new()
  $collect = { param($s) foreach ($a in @($s.animations)) { [void]$played.Add($a.name) } }
  [void][LbtermConductorHub]::Push((& $focus @(@($sids[0], 'thinking'), @($sids[1], 'acting'))))
  Wait-State { param($s) $s.rail.conductor.outline -eq 'spectrum' } -What 'the Conductor''s pad lit as it acts' -OnState $collect | Out-Null
  $acting = Shows { param($s) @($s.rail.rows | Where-Object conductor -eq 'acting').Count -eq 1 -and $s.rail.conductor.outline -eq 'spectrum' } 'acting' 'a row the Conductor acts on, its pad lit'
  $c = $script:entry.rail.conductor
  if (-not $c.acting) { Add-Failure 'acting on a session: the state file''s conductor.acting is false' }
  if (-not $played.Contains('conductor.outline.fade')) { Add-Failure "the pad's spectrum came without its fade in: played [$(@($played) -join ', ')]" } else { 'acting: the pad''s spectrum faded in (conductor.outline.fade)' }
  # Its hues handed to DWM as a loop while it shows (the hue rotation's angle once round every
  # HUE_PERIOD, `loops` naming conductor.outline) is the compositor's given the ring and spin:
  # compositor/imp/tests/rail.rs (feat/dcomp-recorder). Here the loops are said, and the hues seen
  # turning below (DWM playing them)
  "acting: loops $(@($script:entry.loops) -join ', ')"
  $cold = & $blues $acting
  if ($cold -lt (ConvertTo-Px 4 $win.Scale)) { Add-Failure "acting: only $cold of the spectrum's blues round the pad" } else { "acting: $cold of the spectrum's blues round the pad" }
  Start-Sleep -Milliseconds 700
  if (-not (Changed $acting (Get-Shot $win -Name 'acting-later') 0 0 90 $tabs)) { Add-Failure "acting: the pad's hues never turned" } else { 'acting: the pad''s hues turn' }
  $d = Changed $thinking $acting 0 ($thinkingTop + 40) 90 100000
  if (-not $d) { Add-Failure 'acting: no outline on the second session''s tab' } else { "acting: an outline and its light at $(Format-Box $d)" }
  # The second session's row, for a click on it later (the third row)
  $actingRow = @($script:entry.rail.rows | Where-Object conductor -eq 'acting')[0]
  # Each mode while it acts: Accept all lights too; read only takes no notice, its grey outline
  # there and the ring gone at once
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "all", "tooltip": "Conductor is on (accept all)" }')
  $all = Shows { param($s) $s.rail.conductor.level -eq 'all' -and $s.rail.conductor.outline -eq 'spectrum' } 'acting-all' 'the pad accepting all, lit as it acts'
  if ((& $blues $all) -lt (ConvertTo-Px 4 $win.Scale)) { Add-Failure 'acting in accept all: none of the spectrum round the pad' } else { 'acting: lit in accept all too' }
  $played.Clear()
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)" }')
  Wait-State { param($s) $s.rail.conductor.level -eq 'read' } -What 'the pad read only' -OnState $collect | Out-Null
  $readActing = Shows { param($s) $s.rail.conductor.level -eq 'read' -and $s.rail.conductor.outline -eq 'grey' -and $s.rail.conductor.acting } 'acting-read' 'the pad read only while it acts'
  if ($played.Contains('conductor.outline.fade')) { Add-Failure 'read only: the ring faded out, where it goes at once' }
  $cold = & $blues $readActing
  if ($cold -gt 0) { Add-Failure "read only while it acts: $cold of the spectrum's blues round the pad" } else { 'acting: read only keeps its grey outline' }
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "most", "tooltip": "Conductor is on (accept most), here only since 09:05" }')
  Shows { param($s) $s.rail.conductor.level -eq 'most' -and $s.rail.conductor.outline -eq 'spectrum' } 'acting-most' 'the pad accepting most again, lit' | Out-Null
  $played.Clear()
  [void][LbtermConductorHub]::Push((& $focus @()))
  # They linger a lap at most, the pad's spectrum fading out after: the wait is what's tested
  Wait-State { param($s) $s.rail.conductor.outline -eq 'mode' -and -not $s.rail.conductor.acting } -What 'the pad no longer lit' -TimeoutSec 5 -OnState $collect | Out-Null
  Wait-State { param($s) @($s.animations | Where-Object name -eq 'conductor.outline.fade').Count -eq 0 } -What 'the pad''s fade out over' -TimeoutSec 5 -OnState $collect | Out-Null
  if (-not $played.Contains('conductor.outline.fade')) { Add-Failure "the pad's spectrum went without its fade out: played [$(@($played) -join ', ')]" } else { 'let go: the pad''s spectrum faded out (conductor.outline.fade)' }
  Start-Sleep 2.5
  $letGo = Get-Shot $win -Name 'let-go'
  $d = Changed $most $letGo 0 $tabs 90 100000
  if ($d) { Add-Failure "the outlines stayed after the hub let go: $(Format-Box $d)" } else { 'let go: both outlines went' }
  $d = Changed $most $letGo 0 0 90 $tabs
  if ($d) { Add-Failure "the pad isn't as it was at rest after the hub let go: $(Format-Box $d)" } else { 'let go: the pad is its still outline again' }
  $loops = @((Read-State).windows[0].loops)
  if ($loops -contains 'conductor.outline') { Add-Failure "let go: conductor.outline still among the loops ($($loops -join ', '))" }

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
  $flashed = [System.Collections.Generic.HashSet[string]]::new()
  Send-Click $win $x $y
  & $leave
  $waiting = Wait-State { param($s) $s.menu.waiting -ne $null } -What 'the item to wait for its answer' -TimeoutSec 5 -OnState { param($s) foreach ($a in @($s.animations)) { [void]$flashed.Add($a.name) } }
  # The row picked flashes white, as the picker's rows do (Lachlan's 9b)
  if (-not $flashed.Contains('menu.conductor.flash')) { Add-Failure "the click on 'Accept most' didn't flash its row: no menu.conductor.flash in [$(@($flashed) -join ', ')]" } else { 'menu: the row picked flashed (menu.conductor.flash)' }
  $p1 = Get-Shot $win -Name 'pending'
  Start-Sleep -Milliseconds 500
  $p2 = Get-Shot $win -Name 'pending-later'
  [void](& $lastRun 'mode most')
  if (((Reported) -join ',') -ne 'mode most') { Add-Failure "a click on the first item ran: $((Reported) -join ', ')" } else { 'menu: a click ran "Accept most"' }
  if (-not (Read-State).windows[0].menu) { Add-Failure 'the menu closed while its item waited' } else { "menu: it stays open while ""Accept most"" waits (waiting: $($waiting.menu.waiting))" }
  if (-not (Changed $p1 $p2 $arc[0] $arc[1] $arc[2] $arc[3])) { Add-Failure "the waiting item's arc never turned" } else { 'menu: its arc turns' }
  # What DWM was given, from the compositor's tree: the arc turns while the item waits
  try { Wait-State { param($s) @($s.loops) -contains 'menu.spinner' } -What 'the waiting item''s arc among the loops' -TimeoutSec 5 | Out-Null; 'menu: its arc is among the loops while it waits' }
  catch { Add-Failure "while its item waited, the arc wasn't among the window's loops: $(@((Read-State).windows[0].loops) -join ', ')" }
  & $keys Down, Enter
  if ((Reported).Count -ne 1) { Add-Failure "while it waited, Down and Enter ran: $((Reported) -join ', ')" }
  # The answer: the file written again in the same words, only its updatedAt new (as Conductor's
  # off does when it's off already; the mode stays read so the pad holds still), and the menu goes
  $answer = { [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)", "updatedAt": 2 }') }
  if ($Capture) {
    # The card fading out with its arc, frame by frame (motion\capture.ps1's way), then no more
    $out = Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'target\motion\conductor-spinner'
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $out | Out-Null
    $answered = @{ Done = $false }
    $frames = Get-Frames $win ([int]($Seconds * 1000)) -Raw -Each { param($ms) if (-not $answered.Done) { & $answer; $answered.Done = $true } }
    Save-Frames $frames -Dir $out -Window $win
    Remove-Frames $frames
    "capture: $($frames.Count) frames in $Seconds s to $out"
    python (Join-Path $PSScriptRoot '..\motion\sheet.py') $out --row 200
    return
  }
  & $answer
  if (-not (& $menuGoes 'answered')) { Add-Failure 'the menu stayed after the state file changed' } else { 'menu: it closes when the state file changes' }
  # Hidden, the arc holds still: a turn DWM plays for ever keeps it composing every refresh
  try { Wait-State { param($s) -not $s.menu -and @($s.loops) -notcontains 'menu.spinner' } -What 'the arc gone from the loops' -TimeoutSec 5 | Out-Null; 'menu: closed, its arc no longer turns' }
  catch { Add-Failure "the menu closed, but its hidden arc still turns: loops $(@((Read-State).windows[0].loops) -join ', ')" }

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
