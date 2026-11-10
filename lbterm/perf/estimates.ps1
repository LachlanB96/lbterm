#Requires -Version 7
# Covers: wezterm-gui/src/conductor_hub.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/taskbar.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm-extras/src/conductor.rs, lbterm-extras/src/taskbar.rs, lbterm-extras/src/state.rs, lbterm-panel/src/claude.rs, lbterm-panel/src/layout.rs, lbterm-panel/src/words.rs
<#
The Conductor hub's guesses of how far along each Claude session is (`conductor.estimates`, from
Conductor's estimate.ts; conductor_hub.rs, render/tab_panel.rs, taskbar.rs) end to end, in a test
window that looks like Lachlan's LBTerm (New-TestConfig), with its state file in
target\perf\estimates and its hub played here, on a pipe of its own.

1. Three tabs hold Claude sessions (claudehook.exe's `claude_session` user var), each working by
   OSC 7501: the first gives no progress of its own, the second its own 70%, the third has handed
   back (done) with a subagent still at it. The hub says its guesses: the first 40% with 4 minutes
   left and two subagents at 50% and 10%; the second 30%, which its own 70% beats; the third's
   subagent at 80%.
2. The state file: the first row's bar is the guess (estimated, "~4 min") with its subagents' two
   bars; the second's is its own 70%, not a guess, with no time; the third has no turn bar, only its
   subagent's.
3. Open, the pointer below the rows: the guesses in their dimmer green on the first and third rows'
   bars, none on the second's (estimates.png).
4. The first tab open, with nothing reporting OSC 9;4: the taskbar button shows the guess, 40% (the
   GUI's log).
5. The hub guesses the first is further on, 70%: its bar moves on (further.png). Then the hub hangs
   up: every bar goes, and the taskbar button clears.

The pointer arrives as posted messages with LBTERM_POSTED_POINTER, as in the other perf scripts;
the window must never be the foreground one. When the rail has answered comes from the window's
state file (lbterm/STATE.md); the pictures say how it looks.

  pwsh -File lbterm\perf\estimates.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'estimates' -Bin $Bin
$pipeName = "lbterm-perf-estimates-$PID"

# The hub, played: a connection whose first line subscribes is pushed to; HangUp closes them all
if (-not ('LbtermEstimatesHub' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
public static class LbtermEstimatesHub {
  static string pipe;
  static readonly List<StreamWriter> subscribers = new List<StreamWriter>();
  static readonly List<NamedPipeServerStream> open = new List<NamedPipeServerStream>();
  public static int Subscribed;
  public static void Serve(string name) {
    pipe = name;
    new Thread(Accept) { IsBackground = true }.Start();
  }
  static void Accept() {
    while (true) {
      var s = new NamedPipeServerStream(pipe, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances,
        PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
      s.WaitForConnection();
      lock (open) open.Add(s);
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
        if (!subscribed && line.Contains("\"lbterm.subscribe\"")) {
          subscribed = true;
          Interlocked.Increment(ref Subscribed);
          lock (subscribers) subscribers.Add(writer);
        }
      }
    } catch (Exception) { }
    lock (subscribers) subscribers.Remove(writer);
  }
  public static int Push(string line) {
    int n = 0;
    lock (subscribers) foreach (var w in subscribers.ToArray()) { try { w.WriteLine(line); n++; } catch (Exception) { } }
    return n;
  }
  public static void HangUp() {
    lock (open) { foreach (var s in open) { try { s.Dispose(); } catch (Exception) { } } open.Clear(); }
  }
}
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread
}

# A tab holding Claude session `sid`, whose program says `record` with OSC 7501
function Claude-Tab([string]$sid, [string]$record) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sid))
  New-PwshProgram "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7 + [char]27 + ']7501;$record' + [char]27 + '\'); Start-Sleep 600"
}
$sids = @('e5e5e5e5-0000-4000-8000-000000000001', 'e5e5e5e5-0000-4000-8000-000000000002', 'e5e5e5e5-0000-4000-8000-000000000003')
function Bars([int]$first) {
  $until = [DateTimeOffset]::Now.ToUnixTimeMilliseconds() + 120000
  '{"type":"conductor.estimates","until":' + $until + ',"sessions":[' +
  '{"sessionId":"' + $sids[0] + '","percent":' + $first + ',"left":240,"agents":[{"id":"a1","what":"Survey the dashboards","percent":50,"left":120},{"id":"a2","what":"Check the log","percent":10,"left":600}]},' +
  '{"sessionId":"' + $sids[1] + '","percent":30,"left":420,"agents":[]},' +
  '{"sessionId":"' + $sids[2] + '","agents":[{"id":"a3","what":"Long job","percent":80,"left":60}]}]}'
}
# The rows by the tab each holds: the window's first tab holds no session
function Row($s, [int]$k) { @($s.rail.rows | Where-Object tab_index -eq ($k + 1))[0] }
function Same($a, $b) { (@($a) -join ',') -eq (@($b) -join ',') }

$lua = @"
rawset(config, 'lbterm_conductor', { state_file = '$(ConvertTo-LuaPath (Join-Path $T.Out 'button.json'))', hub_pipe = [[\\.\pipe\$pipeName]] })
config.front_end = 'WebGpu'
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true, open_delay_ms = 0 }
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@

[LbtermEstimatesHub]::Serve($pipeName)
Invoke-Test {
  $win = Open-TestWindow -Lua $lua -PostedPointer -Program (New-PwshProgram 'Start-Sleep 600') -Log 'lbterm_gui::termwindow::taskbar=debug,info'
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  $panes = @(
    New-Tab -From $first -NoActivate -Program (Claude-Tab $sids[0] 'state=working:app=claude')
    New-Tab -From $first -NoActivate -Program (Claude-Tab $sids[1] 'state=working:app=claude:progress=70')
    New-Tab -From $first -NoActivate -Program (Claude-Tab $sids[2] 'state=done:app=claude')
  )

  # 1. LBTerm subscribes to the hub; the hub says its guesses
  $deadline = (Get-Date).AddSeconds(15)
  while ([LbtermEstimatesHub]::Subscribed -lt 1 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  if ([LbtermEstimatesHub]::Subscribed -lt 1) { throw 'LBTerm never subscribed to the hub' }
  if ([LbtermEstimatesHub]::Push((Bars 40)) -lt 1) { throw 'no subscriber to push to' }

  # 2. What the rows say
  $s = Wait-Change {
    param($s)
    $a = Row $s 0; $b = Row $s 1; $c = Row $s 2
    $a -and $b -and $c -and $a.progress -eq 40 -and (Same $a.agent_bars @(50, 10)) -and $b.progress -eq 70 -and (Same $c.agent_bars @(80))
  } -What 'the three rows with the hub''s guesses'
  $a = Row $s 0; $b = Row $s 1; $c = Row $s 2
  "rows: first $($a.progress)% estimated=$($a.estimated) '$($a.left)' agents [$($a.agent_bars -join ', ')]; second $($b.progress)% estimated=$($b.estimated) '$($b.left)'; third $($c.progress) agents [$($c.agent_bars -join ', ')]"
  if (-not $a.estimated -or $a.left -ne '~4 min') { Add-Failure "the first row's bar should be the guess with ~4 min left: estimated=$($a.estimated), left '$($a.left)'" }
  if ($b.estimated -or $b.left) { Add-Failure "the second row's own 70% should win over the guess: estimated=$($b.estimated), left '$($b.left)'" }
  if ($null -ne $c.progress -or $c.estimated) { Add-Failure "the third row has handed back: no turn bar, but it has $($c.progress)" }

  # 3. Open, the pointer below the rows so none is hovered: how the bars look
  $px = { param($v) ConvertTo-Px $v $win.Scale }
  $cr = Get-ClientRect $win
  Set-PointerHold $win (& $px 36) ($cr.Bottom - (& $px 60))
  $s = Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating
  $open = Get-Shot $win -Name 'estimates'
  # palette::estimate(Working): the status green half sunk into the track
  $guess = 0x57975D
  $lines = foreach ($k in 0, 1, 2) {
    $r = (Row $s $k).rect
    $ys = @(for ($y = [int]$r.y; $y -lt [int]($r.y + $r.h); $y++) {
        if ([LbtermE2E.Px]::CountNear($open, (New-Rect ([int]$r.x) $y ([int]$r.w) 1), $guess, 12) -ge (& $px 6)) { $y }
      })
    # Runs of lines next to each other are one bar's height
    $runs = 0; $last = -10
    foreach ($y in $ys) { if ($y -gt $last + 1) { $runs++ }; $last = $y }
    $runs
  }
  "pixels: lines of guessed bars in each row: $($lines -join ', ')"
  if ($lines[0] -lt 2) { Add-Failure "the first row should show its guessed bar and its subagents' over it: $($lines[0]) lines of the guess's colour (estimates.png)" }
  if ($lines[1] -ne 0) { Add-Failure "the second row's bar is its own progress, not a guess, but $($lines[1]) lines are the guess's colour (estimates.png)" }
  if ($lines[2] -ne 1) { Add-Failure "the third row should show its subagent's bar alone: $($lines[2]) lines of the guess's colour (estimates.png)" }
  Clear-PointerHold
  Send-Leave $win

  # 4. The first tab open, nothing reporting OSC 9;4: the guess on the taskbar button
  $read = { @(Get-LogLines $win 'taskbar progress for window \d+: (.+)$' | ForEach-Object { if ($_ -match 'taskbar progress for window \d+: (.+)$') { $Matches[1] } }) }
  Invoke-Cli activate-tab --pane-id $first --tab-index 1 | Out-Null
  $deadline = (Get-Date).AddSeconds(10)
  while (-not ((& $read) -contains 'Normal(40)') -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  $shown = & $read
  "taskbar: $($shown -join ', ')"
  if (-not ($shown -contains 'Normal(40)')) { Add-Failure "the taskbar button never showed the first tab's guess, 40%: $($shown -join ', ')" }

  # 5. Further on: the bar moves; the hub hangs up: every bar goes, and the button clears
  [void][LbtermEstimatesHub]::Push((Bars 70))
  $s = Wait-Change { param($s) (Row $s 0).progress -eq 70 } -What 'the first row at 70%' -Animating
  Get-Shot $win -Name 'further' | Out-Null
  [LbtermEstimatesHub]::HangUp()
  $s = Wait-Change {
    param($s)
    $a = Row $s 0; $c = Row $s 2
    $null -eq $a.progress -and @($a.agent_bars).Count -eq 0 -and @($c.agent_bars).Count -eq 0
  } -What 'the bars gone with the hub'
  if ((Row $s 1).progress -ne 70) { Add-Failure "the second row's own progress went with the hub: $((Row $s 1).progress)" }
  $deadline = (Get-Date).AddSeconds(10)
  while (((& $read) | Select-Object -Last 1) -ne 'None' -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  $shown = & $read
  if (($shown | Select-Object -Last 1) -ne 'None') { Add-Failure "the taskbar button kept the guess after the hub went: $($shown -join ', ')" }
  else { 'hub gone: the bars and the taskbar button cleared' }
} -Passed 'estimates: the hub''s guesses on the rows (dimmer than a program''s own, which wins), its subagents'' bars, the time left, the taskbar button, all gone with the hub'
