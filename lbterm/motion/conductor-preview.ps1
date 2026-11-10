#Requires -Version 7
<#
A preview of the Conductor's view (lbterm/TAB-PANEL.md, "The Conductor's view") against what the
Conductor's seat holds right now: for checking a change to how the view looks against the real
conversation rather than a made-up one (Lachlan's E2, 2026-10-28).

  pwsh -File lbterm\motion\conductor-preview.ps1              # this tree's build, the shots in target\motion\conductor-preview
  pwsh -File lbterm\motion\conductor-preview.ps1 -Cards       # with a made-up ask and two questions on the side
  pwsh -File lbterm\motion\conductor-preview.ps1 -Busy        # as if a turn were under way, a message queued

It subscribes to the seat's pipe once, read only (`seat.subscribe`, client `preview`), keeps the
first `seat.view` and the conversation (`seat.items`, reset), and hangs up: it sends the seat no
command. Then it opens a test window of the build (Open-TestWindow: unfocused, on the right-most
screen) whose config names a pipe of its own, plays that view and conversation to it, opens the view
with a click on the pad, and captures it:

  newest     the view as it opens, at the newest
  scrolled   the conversation scrolled up six turns of the wheel
  side       the side column scrolled down
  top        the conversation's start, today's name over it
  folded     the side folded to a rail as wide as the tab rail
  sessions   the side opened again from the rail's working sessions
  review     the suggestions reviewed one at a time

The shots hold Lachlan's real conversation, so they stay in target\ (which git ignores) and go
nowhere else. Fails, saying so, when the seat doesn't answer.
#>
param(
  [string]$Bin,
  [string]$SeatPipe = 'conductor-seat',
  [switch]$Cards,
  [switch]$Busy,
  [int]$Cols = 112,
  [int]$Rows = 84
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$T = Start-Test 'conductor-preview' -Bin $Bin -Out (Join-Path $repo 'target\motion\conductor-preview')
$state = Join-Path $T.Out 'button.json'
$pipeName = "lbterm-preview-seat-$PID"

# The seat as it stands: its view and its conversation, from one read-only subscription
function Get-SeatNow([string]$name) {
  $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $name, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
  try {
    try { $pipe.Connect(5000) } catch { throw "the seat doesn't answer on \\.\pipe\$name ($($_.Exception.Message)): is the Conductor's seat running?" }
    $writer = [IO.StreamWriter]::new($pipe, [Text.UTF8Encoding]::new($false))
    $writer.NewLine = "`n"
    $writer.AutoFlush = $true
    $writer.WriteLine((@{ type = 'seat.subscribe'; id = "preview-$PID"; client = 'preview'; pid = $PID; version = 'conductor-preview' } | ConvertTo-Json -Compress))
    $reader = [IO.StreamReader]::new($pipe, [Text.UTF8Encoding]::new($false))
    $view = $null; $items = $null
    $deadline = (Get-Date).AddSeconds(10)
    while (-not ($view -and $items)) {
      $left = [int]($deadline - (Get-Date)).TotalMilliseconds
      if ($left -le 0) { throw "the seat sent no $(if (-not $view) { 'seat.view' } else { 'seat.items' }) within 10 s" }
      $read = $reader.ReadLineAsync()
      if (-not $read.Wait($left)) { continue }
      $line = $read.Result
      if ($null -eq $line) { throw 'the seat hung up before it sent its view and conversation' }
      if ($line -match '"type":"seat\.view"' -and -not $view) { $view = $line }
      elseif ($line -match '"type":"seat\.items"' -and $line -match '"reset":true' -and -not $items) { $items = $line }
    }
    [pscustomobject]@{ View = $view; Items = $items }
  } finally {
    $pipe.Dispose()
  }
}

# The seat played to the test window: a subscriber gets what's pushed, a command its ok
if (-not ('LbtermPreviewSeat' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public static class LbtermPreviewSeat {
  static string pipe;
  static readonly List<StreamWriter> subscribers = new List<StreamWriter>();
  public static int Subscribed = 0;
  public static void Serve(string name) { pipe = name; new Thread(Accept) { IsBackground = true }.Start(); }
  static void Accept() {
    while (true) {
      var s = new NamedPipeServerStream(pipe, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
      s.WaitForConnection();
      new Thread(() => Talk(s)) { IsBackground = true }.Start();
    }
  }
  static void Talk(NamedPipeServerStream s) {
    var reader = new StreamReader(s);
    var writer = new StreamWriter(s) { AutoFlush = true, NewLine = "\n" };
    try {
      string line;
      while ((line = reader.ReadLine()) != null) {
        if (line.Contains("\"seat.subscribe\"")) { lock (subscribers) subscribers.Add(writer); Subscribed++; continue; }
        var id = Regex.Match(line, "\"id\":\"([^\"]+)\"");
        if (id.Success) writer.WriteLine("{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":true}");
      }
    } catch (IOException) { }
    lock (subscribers) subscribers.Remove(writer);
  }
  public static int Push(string line) {
    int n = 0;
    lock (subscribers) foreach (var w in subscribers.ToArray()) { try { w.WriteLine(line); n++; } catch (IOException) { } }
    return n;
  }
}
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread, System.Text.RegularExpressions
}

$now = Get-SeatNow $SeatPipe
$view = $now.View | ConvertFrom-Json -Depth 64
$stamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
if ($Cards -and @($view.sessions).Count -ge 2) {
  # A made-up ask and two questions, put on two of the real sessions
  $s1 = $view.sessions[0]; $s2 = $view.sessions[1]
  $view.asks = @([pscustomobject]@{ id = 'ask-preview'; at = $stamp - 140000; tool = 'SendMessage'; title = "Message $($s1.label) in $($s1.project)"; detail = 'From the Conductor: step 2 is done; go on to step 3 and push when the check passes.'; session = $s1.name; label = $s1.label; project = $s1.project })
  $view.waiting = @(
    [pscustomobject]@{ id = "$($s1.sessionId):preview"; sessionId = $s1.sessionId; session = $s1.name; label = $s1.label; project = $s1.project; kind = 'picker'; extras = $false; gist = 'Which layout?'; at = $stamp - 360000
      questions = @([pscustomobject]@{ question = 'Which layout?'; header = 'Layout'; multiSelect = $false; options = @([pscustomobject]@{ label = 'Two columns'; description = 'Side by side' }, [pscustomobject]@{ label = 'One column'; description = 'Easier on a phone' }) }); text = $null; answerable = $true; why = $null },
    [pscustomobject]@{ id = "$($s2.sessionId):preview"; sessionId = $s2.sessionId; session = $s2.name; label = $s2.label; project = $s2.project; kind = 'text'; extras = $false; gist = 'Q1'; at = $stamp - 60000; questions = @()
      text = "Q1. Delete the old branch?`n  1a. Delete it`n  1b. Keep it`n`nExtras, yes or no each:`nE2. A test for it"; answerable = $true; why = $null })
}
if ($Busy) { $view.conductor.busy = $true; $view.conductor.queued = 1 }
$viewLine = $view | ConvertTo-Json -Depth 64 -Compress

$lua = @"
rawset(config, 'lbterm_conductor', {
  state_file = '$(ConvertTo-LuaPath $state)',
  seat_pipe = [[\\.\pipe\$pipeName]],
  menu = {},
})
config.initial_cols = $Cols
config.initial_rows = $Rows
config.cursor_blink_rate = 0
"@

[LbtermPreviewSeat]::Serve($pipeName)
Invoke-Test {
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "listening": true, "busy": false, "asks": 0 }')
  $script:win = Open-TestWindow -Lua $lua -PostedPointer -Program (New-PwshProgram 'Start-Sleep 900')
  $entry = Wait-State { param($s) $s.frame -gt 0 -and $s.rail.conductor -and $null -ne $s.active_pane } -What 'the window and the pad'
  if (-not (Wait-Until { [LbtermPreviewSeat]::Subscribed -gt 0 } -TimeoutSec 15)) { throw 'LBTerm never subscribed to the played seat' }
  [void][LbtermPreviewSeat]::Push($viewLine)
  [void][LbtermPreviewSeat]::Push($now.Items)
  $pad = Get-RectCenter $entry.rail.conductor.pad
  Send-Click $win $pad[0] $pad[1] -Move
  Send-Leave $win
  $entry = Wait-State { param($s) $s.conductor_view -and $s.conductor_view.connected -and @($s.conductor_view.items).Count -gt 0 } -What 'the view open with the conversation'
  # The view's fonts measure each character the first time it's drawn: a moment to settle
  Start-Sleep -Milliseconds 800
  # A button by its name in the state file, pressed where it is
  function Press([string]$name) {
    $b = @((Read-State).windows[0].conductor_view.buttons | Where-Object name -eq $name)[0]
    if (-not $b -or -not $b.rect) { throw "no $name in sight to press" }
    $p = Get-RectCenter $b.rect
    Send-Click $win $p[0] $p[1] -Move
    Send-Leave $win
    Start-Sleep -Milliseconds 600
  }
  Get-Shot $win -Name 'newest' | Out-Null
  $v = $entry.conductor_view
  $cx = [int]($v.conversation.x + $v.conversation.w / 2); $cy = [int]($v.conversation.y + $v.conversation.h / 2)
  for ($i = 0; $i -lt 6; $i++) { Send-Wheel $win $cx $cy -Delta 120; Start-Sleep -Milliseconds 40 }
  Start-Sleep -Milliseconds 500
  Get-Shot $win -Name 'scrolled' | Out-Null
  $sx = [int]($v.rect.x + $v.rect.w - 120); $sy = [int]($v.rect.y + $v.rect.h / 2)
  for ($i = 0; $i -lt 12; $i++) { Send-Wheel $win $sx $sy -Delta -120; Start-Sleep -Milliseconds 40 }
  Start-Sleep -Milliseconds 500
  Get-Shot $win -Name 'side' | Out-Null
  for ($i = 0; $i -lt 400; $i++) { Send-Wheel $win $cx $cy -Delta 120 }
  Start-Sleep -Milliseconds 700
  Get-Shot $win -Name 'top' | Out-Null
  Press 'fold-side'
  Get-Shot $win -Name 'folded' | Out-Null
  Press 'reveal:sessions:working'
  Get-Shot $win -Name 'sessions' | Out-Null
  if (@((Read-State).windows[0].conductor_view.buttons | Where-Object name -eq 'review:start').Count) {
    Press 'fold-side'
    Press 'reveal:suggestions'
    Press 'review:start'
    Get-Shot $win -Name 'review' | Out-Null
  } else {
    'review: no open suggestions to review'
  }
  "shots in $($T.Out)"
} -Passed 'the Conductor''s view previewed against the seat as it stands'
