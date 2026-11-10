#Requires -Version 7
# Variant: -Local
# Covers: wezterm-gui/src/state_files.rs, wezterm-gui/src/thumbnails.rs, wezterm-gui/src/restart.rs, wezterm-gui/src/stats.rs, wezterm-gui/src/inbox.rs, wezterm-gui/src/summaries.rs, deps/notify/src/windows.rs, lbterm-extras/src/watch.rs, lbterm-extras/src/inbox.rs, lbterm-extras/src/tasks.rs, lbterm-extras/src/summaries.rs, lbterm-panel/src/claude.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/tasks_card.rs, wezterm-gui/src/seat_client.rs, wezterm-gui/src/conductor_hub.rs, wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/termwindow/render/conductor_view.rs, wezterm-gui/src/termwindow/render/projects.rs, wezterm-gui/src/termwindow/render/questions.rs, lbterm-extras/src/conductor.rs, lbterm-extras/src/seat/mod.rs, lbterm-extras/src/seat/link.rs, lbterm/perf/threads.ps1, mux/src/lbterm_procs.rs, mux/src/lbterm_proclist.rs, mux/src/localpane.rs, lbterm-extras/src/newtab.rs, wezterm-gui/src/termwindow/render/new_tab.rs
<#
What a window like Lachlan's does while nothing happens (lbterm/AT-REST.md): no thread waking, no
frame and no file read unless something it shows changed, and when something does, it shows at
once. A test window that looks like his (New-TestConfig) on a mux domain of its own (-Local: its
own tabs), running a copy of the build with a version.txt beside it as an install has, with what
his has running all day, each against files of the test's own:

- ten tabs: the open one with coloured text and a thumbnail of an image it links to (show-image's
  links), seven stand-ins for Claude Code sessions that handed the turn back between 1 and 46
  minutes ago, each with its label (the rail's ages read differently by the minute while it's
  open, and no idle step comes during the test), one whose session has three tasks still going
  (its pad's arc turns), listed in its tasks.json, a subagent among them with a transcript of its
  own, and one that names itself and says its folder (OSC 7), as Claude Code does once its hooks
  have, and starts short programs (1e);
- the InMail button on -Folders project folders holding -Messages messages;
- the Conductor's pad on a state file (button.json, in a folder of its own inside a folder that
  stands for the machine state, as in Lachlan's), its seat and its hub played on pipes of the
  test's own (the seat sends its view and conversation on subscribe and answers commands; the hub
  hears where the sessions are and says its guesses), and the tab links on a links file in the
  machine state's folder.

1. At rest, the rail closed, for -RestSeconds: the window paints nothing; no thread of LBTerm's
   named lbterm-*, nor a notify watcher's, wakes at all; LBTerm's own threads but the GUI thread
   (those it started: async-io's timers, the mux client's, the GUI socket's) wake -OwnWakes times
   a second at most, and twice more for each other LBTerm connecting to its GUI socket as it
   starts (the counters name each), and the GUI thread, which other programs' messages wake too (a
   WM_GETTEXTLENGTH now and then), -MainWakes; and the process reads and writes no file and does
   -OtherIo other I/O operations a second at most. What the GPU driver's threads do (NVIDIA's
   D3D12 driver wakes two of its own 87 times a second here, whatever LBTerm draws) is reported
   and not judged.
   The GUI's own counters over the span (perf/budgets' metrics, from a build that answers them)
   say no timer fired on the GUI thread but those that tick on purpose, and no task ran there but
   another LBTerm connecting to its GUI socket (each one's update_checker does, 10 s after it
   starts).
1b. The seat talking with the Conductor's view closed, for -SeatSeconds: a new seat.view every
   250 ms (its first session going between working and idle), and for the second half the reply
   streaming (seat.delta every 100 ms), then the finished item. Nothing the window shows changes,
   so it paints nothing, its GUI thread wakes -MainWakes times a second at most, and no thread of
   LBTerm's named lbterm-* but the seat's own (which reads each line) wakes. Until 2026-10-30 each
   line painted every window: 182 frames for 181 lines.
1c. The hub saying its guesses again with a later end and the same sessions, as it does every
   45 s (here every 9 s, an end 12 s on), for -HubSeconds, three rows showing its bars: no frame,
   no thread of LBTerm's named lbterm-* but the hub's own waking. Until 2026-10-31 the rail's wake
   at each old end painted a frame for nothing.
1e. The tab that names itself and says its folder starting a short program every -ChurnEvery s,
   printing nothing, as Claude Code starts its statusline and hooks, for -ChurnSeconds: its job
   hears each start and end (lbterm::procs's `its processes moved: ... (shown: false)`, at debug
   in the window's log, or its server's on a mux domain), and nothing walks or reads its
   processes (no lbterm-procs thread starts); nothing the window shows changes, so it paints
   nothing, no task of LBTerm's runs on the GUI thread for it (its counters), and no thread of
   LBTerm's named lbterm-* but lbterm-jobs, which hears the job, wakes (the window's with -Local;
   on a mux domain the panes' jobs are the server's, whose threads are reported, not judged).
   ConPTY sends the pane's title again as each program ends, which the parser and the window take
   as upstream does (a title set again: a task for the output and one for the held alerts on the
   GUI thread, mux/src/lib.rs and localpane.rs, and the window's subscription's), so those tasks
   and the GUI thread's wakes are reported, not judged. The review of perf/key-latency asked for
   it (2026-10-31): the stand-ins elsewhere start no programs at rest.
1d. Neighbours writing beside the files watched, for -NeighbourSeconds, as Lachlan's machine state
   has them (measured on 2026-10-31): a file renamed into place in its folder (-MachineWrites a
   second), files in a folder of it nothing here watches (-SubfolderWrites), and the Conductor's
   seat.json beside button.json (-ConductorWrites). Windows watches folders, so notify's threads
   wake for each, -NotifyWakesAWrite times at most, and pass nothing on: no frame, no thread of
   LBTerm's named lbterm-* waking (lbterm/AT-REST.md, "What still wakes").
2. The pointer rests on the tasks tab's row (one posted move, no more): the panel opens and the
   tasks card drops beside it. Then at rest again for -CardSeconds: the transcripts' thread
   (lbterm-transcripts) never wakes, and the window paints only for an age the panel or the card
   shows reading otherwise: two frames for each row or line whose age changed in the span (its
   paint and the frame after its dissolve), and one more.
3. Each change once, timed from the write to the first frame that shows it, each within
   -ShowBudgetMs: the subagent's transcript growing (the card's line), a message arriving and
   being archived (the button's count), the Conductor's state file asking (the pad's asks), its
   folder removed and made again with an ask, then renamed away and a new one made (the pad off,
   then the new file's asks: until 2026-10-31 the watch went with the folder and nothing after
   it showed), the links file linking two tabs (the rail's line), the image deleted and put back
   (the thumbnail gone and shown), and version.txt naming another build and this one again (the
   strip's pill).
   The Conductor asking writes button.json and sends the seat's view 250 ms later, as the seat
   does (its view timer); the pad's asks are the file's.
4. The open tab printing a line every 8 ms for -PrintSeconds: the hub hears nothing (no session
   moved), and the GUI thread's CPU a frame is reported for comparing builds.

It fails on any of those. cost.json has every number: threads by name with their wakes (context
switches), CPU (cycles) and the module they started in, the process's I/O operations, frames, and
the mux server's threads, reported and not judged (the server is its own). With a build that
answers the control pipe's `metrics`, its counters over the span at rest too (rest_metrics).
Measured on 2026-10-30, three runs of each build interleaved: lbterm/AT-REST.md.

  pwsh -File lbterm\perf\at-rest.ps1 [-Local] [-RestSeconds 60] [-SeatSeconds 20] [-HubSeconds 20] [-NeighbourSeconds 20] [-ChurnSeconds 20] [-Bin <build>] [-Measure]
#>
param(
  [string]$Bin,
  [switch]$Local,
  [int]$RestSeconds = 60,
  [int]$CardSeconds = 30,
  [int]$SeatSeconds = 20,
  [int]$PrintSeconds = 8,
  [int]$HubSeconds = 20,
  [int]$NeighbourSeconds = 20,
  [int]$ChurnSeconds = 20,
  [double]$ChurnEvery = 2,
  # lbterm-jobs's wakes a program may cost at most: its start and its end, each a message on the
  # job's port
  [double]$JobWakesAProgram = 4,
  [int]$Folders = 47,
  [int]$Messages = 80,
  # A second, at most, at rest: LBTerm's threads but the GUI thread, the GUI thread, other I/O
  # (measured on 2026-10-31: 0.1 to 0.3, 0.2 to 3.2 and 0.5)
  [double]$OwnWakes = 0.6,
  [double]$MainWakes = 5,
  [double]$OtherIo = 2,
  # The neighbours' writes a second, as measured in Lachlan's machine state on 2026-10-31 (7.26
  # changes a second in it, 3.26 in the Conductor's folder): files renamed into place in it, files
  # written in a folder of its LBTerm doesn't watch (the Warden's), and the Conductor's seat.json
  # renamed into place beside button.json; and the notify threads' wakes each may cost at most
  # (a wake for each change Windows reports, about four for a file renamed into place: 4.79 a
  # write measured on 2026-10-31)
  [double]$MachineWrites = 0.85,
  [double]$SubfolderWrites = 1.4,
  [double]$ConductorWrites = 0.82,
  [double]$NotifyWakesAWrite = 6,
  [double]$ShowBudgetMs = 250,
  # Measure and report, judging nothing
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
$T = Start-Test ('at-rest' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin
$mux = -not $Local

# Each thread's wakes and CPU, the process's I/O ([LbtermRest], Measure-Span, Format-Span)
. (Join-Path $PSScriptRoot 'threads.ps1')

# The Conductor's seat and its hub, played on pipes of the test's own (never the real ones): every
# line any connection sends is kept; a connection whose first line has `Marker` (`seat.subscribe`,
# `lbterm.subscribe`) is pushed to, and `Greeting` sent to it first, as the seat sends its view and
# conversation to a new subscriber; with `Answer`, any other seat line with an id is answered ok on
# its connection, as the seat answers a command
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public sealed class LbtermRestPipe {
  readonly string name, marker; readonly bool answer;
  readonly List<StreamWriter> subscribers = new List<StreamWriter>();
  readonly List<string> lines = new List<string>();
  public string[] Greeting = new string[0];
  public LbtermRestPipe(string name, string marker, bool answer) { this.name = name; this.marker = marker; this.answer = answer; }
  public void Serve() { new Thread(Accept) { IsBackground = true }.Start(); }
  void Accept() {
    while (true) {
      var s = new NamedPipeServerStream(name, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances,
        PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
      s.WaitForConnection();
      new Thread(() => Talk(s)) { IsBackground = true }.Start();
    }
  }
  void Talk(NamedPipeServerStream s) {
    var reader = new StreamReader(s);
    var writer = new StreamWriter(s) { AutoFlush = true, NewLine = "\n" };
    bool subscribed = false;
    try {
      string line;
      while ((line = reader.ReadLine()) != null) {
        lock (lines) lines.Add(line);
        if (!subscribed && line.Contains(marker)) {
          subscribed = true;
          lock (subscribers) { foreach (var g in Greeting) writer.WriteLine(g); subscribers.Add(writer); }
          continue;
        }
        var id = Regex.Match(line, "\"id\":\"([^\"]+)\"");
        if (answer && id.Success && line.Contains("\"type\":\"seat."))
          writer.WriteLine("{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":true}");
      }
    } catch (IOException) { }
    lock (subscribers) subscribers.Remove(writer);
  }
  public int Push(string line) {
    int n = 0;
    lock (subscribers) foreach (var w in subscribers.ToArray()) { try { w.WriteLine(line); n++; } catch (IOException) { } }
    return n;
  }
  // As the seat sends a view: `ms` after the change, on a timer of its own
  public void PushAfter(string line, int ms) { new Thread(() => { Thread.Sleep(ms); Push(line); }) { IsBackground = true }.Start(); }
  // As the hub says its guesses again: `head` + an end `aheadMs` from now + `tail`, every
  // `everyMs`, `times` times, the first at once; how many it sent so far in Resent
  int resent;
  public int Resent { get { return resent; } }
  public void Resend(string head, string tail, int aheadMs, int everyMs, int times) {
    new Thread(() => {
      for (int i = 0; i < times; i++) {
        Push(head + (DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() + aheadMs) + tail);
        Interlocked.Increment(ref resent);
        Thread.Sleep(everyMs);
      }
    }) { IsBackground = true }.Start();
  }
  public int Subscribers { get { lock (subscribers) return subscribers.Count; } }
  public int Count(string type) {
    int n = 0; string t = "\"type\":\"" + type + "\"";
    lock (lines) foreach (var l in lines) if (l.Contains(t)) n++;
    return n;
  }
}
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread, System.Text.RegularExpressions

# The GUI's own counters over a span (perf/budgets' counters), from a build that has them:
# its control pipe answers `metrics`, reset at the span's start. Nothing from one that doesn't
function Get-ControlMetrics($Window, [switch]$Reset) {
  $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', "lbterm-control-$($Window.Class)", [IO.Pipes.PipeDirection]::InOut)
  try {
    $pipe.Connect(3000)
    $writer = [IO.StreamWriter]::new($pipe, [Text.UTF8Encoding]::new($false)); $writer.AutoFlush = $true
    $reader = [IO.StreamReader]::new($pipe, [Text.UTF8Encoding]::new($false))
    $writer.WriteLine((ConvertTo-Json -Compress @{ id = 'at-rest'; op = 'metrics'; reset = [bool]$Reset }))
    $task = $reader.ReadLineAsync()
    if (-not $task.Wait(5000)) { return $null }
    $reply = $task.Result | ConvertFrom-Json -Depth 16
    if ($reply.ok -eq $false) { return $null }
    $reply
  } catch { $null } finally { $pipe.Dispose() }
}

# Who asked for a span's frames, from its counters (Get-ControlMetrics reset at its start)
function Format-Askers($Metrics) {
  if (-not $Metrics) { return 'not answered' }
  $asked = @($Metrics.requests | Where-Object { $_.frames } | ForEach-Object { "$($_.site) ($($_.frames))" })
  if ($Metrics.totals.unasked) { $asked += "nobody ($($Metrics.totals.unasked))" }
  if ($asked.Count) { $asked -join ', ' } else { 'nobody' }
}

# The build, copied with a version.txt naming itself beside it, as an install has
$bin = Join-Path $T.Out 'bin'
New-Item -ItemType Directory -Force $bin | Out-Null
Get-ChildItem -LiteralPath $T.Bin -File | Where-Object { $_.Name -notlike '*.old' -and $_.Name -ne 'version.txt' -and $_.Extension -in '.exe', '.dll' } |
  ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $bin }
$version = ((& (Join-Path $bin 'lbterm.exe') --version) -replace '^lbterm\s+', '').Trim()
$versionFile = Join-Path $bin 'version.txt'
[IO.File]::WriteAllText($versionFile, $version)
$T.Gui = Join-Path $bin 'lbterm-gui.exe'
$T.Cli = Join-Path $bin 'lbterm.exe'
$T.MuxServer = Join-Path $bin 'lbterm-mux-server.exe'

$home2 = Join-Path $T.Out 'home'
$summaries = Join-Path $T.Out 'summaries'
$inmail = Join-Path $T.Out 'inmail'
# As in Lachlan's machine state (%LOCALAPPDATA%\Megaproject): the links file in it, the
# Conductor's state file in a folder of its own inside it
$machine = Join-Path $T.Out 'megaproject'
$conductorDir = Join-Path $machine 'conductor'
$images = Join-Path $T.Out 'images'
New-Item -ItemType Directory -Force $home2, $summaries, $inmail, $conductorDir, $images | Out-Null

# Every project's InMail folder, the messages spread over them, the first folder empty for the test
$folderList = foreach ($i in 1..$Folders) { Join-Path $inmail ('P{0:D2}\InMail' -f $i) }
foreach ($f in $folderList) { New-Item -ItemType Directory -Force $f | Out-Null }
for ($m = 0; $m -lt $Messages; $m++) {
  $f = $folderList[1 + $m % ($Folders - 1)]
  $at = [DateTime]::new(2026, 10, 29, 8, 0, 0).AddMinutes($m * 7)
  [IO.File]::WriteAllText((Join-Path $f ('{0:yyyy-MM-dd-HHmm}-from-Home-message-{1}.md' -f $at, $m)), "---`nfrom: Home`nto: P`nsent: $($at.ToString('yyyy-MM-ddTHH:mm'))`nsubject: `"Message $m`"`n---`n`nThe body of message $m.`n")
}
$newMessage = Join-Path $folderList[0] '2026-10-30-0900-from-Home-the-test.md'

# The Conductor's state file and the links file, as their writers leave them
$button = Join-Path $conductorDir 'button.json'
function Write-Button([int]$Asks) {
  $json = [ordered]@{ state = 'on'; mode = 'read'; listening = $true; busy = $false; asks = $Asks; label = 'Conductor'; tooltip = 'on'; updatedAt = 1 } | ConvertTo-Json -Compress
  Write-FileWhole -Path $button -Text $json
}
Write-Button 0
$linksFile = Join-Path $machine 'tab-links.json'
function Write-Links($Links) {
  $json = [ordered]@{ updatedAt = 1; links = @($Links); waits = @() } | ConvertTo-Json -Depth 5 -Compress
  Write-FileWhole -Path $linksFile -Text $json
}
Write-Links @()

# The seat's view as the real seat sends it on subscribe and on each change: the Conductor on and
# idle, `$Asks` asks waiting, and the stand-in sessions, the first `$Status`
function Seat-View([int]$Seq, [string]$Status = 'idle', [int]$Asks = 0) {
  $rows = @(for ($i = 0; $i -lt $sessions.Count; $i++) {
    [ordered]@{ sessionId = $sessions[$i].Sid; name = "session-$($i + 1)"; project = 'Terminal'; status = $(if ($i -eq 0) { $Status } else { 'idle' }); label = "Session $($i + 1)"; summary = ''; question = $null; pane = $null; agents = @(); conductor = $null }
  })
  $askRows = @(for ($i = 0; $i -lt $Asks; $i++) {
    [ordered]@{ id = "ask-$i"; at = $seatAt - 60000; tool = 'SendMessage'; title = "Message session $($i + 1)"; detail = 'Push it'; session = "session-$($i + 1)"; label = "Session $($i + 1)"; project = 'Terminal' }
  })
  [ordered]@{
    type = 'seat.view'; seq = $Seq; at = $seatAt
    conductor = [ordered]@{ state = 'on'; mode = 'read'; listening = $true; pausedAt = $null; catchUp = $false; busy = $false; queued = 0; sessionId = 'c0'; startedAt = $seatAt - 600000; model = 'claude-opus-5-5'; error = $null }
    asks = $askRows; sessions = $rows; waiting = @(); events = @(); suggestions = @(); notes = @()
  } | ConvertTo-Json -Compress -Depth 6
}
$seatPipe = "lbterm-perf-atrest-seat-$PID"
$hubPipe = "lbterm-perf-atrest-hub-$PID"
$seat = [LbtermRestPipe]::new($seatPipe, '"seat.subscribe"', $true)
$hub = [LbtermRestPipe]::new($hubPipe, '"lbterm.subscribe"', $false)

# The image the open tab links to
Add-Type -AssemblyName System.Drawing
$image = Join-Path $images 'teal.png'
function New-Png([string]$Path) {
  $bmp = [System.Drawing.Bitmap]::new(300, 200)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::FromArgb(0, 128, 128)); $g.Dispose()
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
New-Png $image
$imageBytes = [IO.File]::ReadAllBytes($image)
$address = 'lbterm://image/' + ($image -replace '\\', '/')
# While this file is there the open tab prints a line every 8 ms, as a working program does
$printGo = Join-Path $T.Out 'print.go'
Remove-Item -LiteralPath $printGo -ErrorAction SilentlyContinue
$openTab = New-PwshProgram -Script @"
`$e = [char]27; `$b = [char]0x2800
[Console]::OutputEncoding = [Text.UTF8Encoding]::new(`$false)
`$w = 'alpha','bravo','charlie','delta','echo','foxtrot','golf','hotel'
foreach (`$r in 1..20) { `$line = ''; foreach (`$i in 1..14) { `$line += "`$e[38;5;`$((`$r * 7 + `$i * 13) % 230 + 17)m`$(`$w[(`$r + `$i) % 8]) " }; Write-Host (`$line + "`$e[0m") }
function Link([string]`$uri, [string]`$words) { "`$e]8;;`$uri`$e\`$words`$e]8;;`$e\" }
[Console]::WriteLine('  ' + (Link '$address' 'teal.png'))
for (`$r = 0; `$r -lt 4; `$r++) { [Console]::WriteLine('  ' + (Link "$($address)?row=`$r&of=4" ([string]::new(`$b, 16)))) }
`$n = 0
while (`$true) {
  if (Test-Path -LiteralPath '$($printGo -replace "'", "''")') { [Console]::WriteLine("`$e[38;5;`$(`$n % 230 + 17)mline `$n `$(`$w[`$n % 8]) `$(`$w[(`$n + 3) % 8])`$e[0m"); `$n++; Start-Sleep -Milliseconds 8 }
  else { Start-Sleep -Milliseconds 100 }
}
"@

# Seven sessions that handed back between 1 and 46 minutes ago, their minutes' ends spread over
# the spans at rest, each at least six minutes short of its idle bars' next step (an eighth of
# Lachlan's idle_full_minutes, 60), which the closed rail shows: none comes in the test
$demo = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$ages = 1.1, 8.25, 15.4, 23.55, 30.7, 37.85, 45.95
$sessions = for ($i = 0; $i -lt $ages.Count; $i++) {
  $sid = '{0}{0}{0}{0}{0}{0}{0}{0}-0000-4000-8000-00000000000{0}' -f ($i + 1)
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.label.txt"), "Session $($i + 1)")
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.txt"), "Handed back $($ages[$i]) minutes ago")
  [pscustomobject]@{ Sid = $sid; Age = $ages[$i]; Program = (New-PwshProgram -File $demo -ArgumentList @('-Title', "Session $($i + 1)", '-State', 'idle', '-AgeMinutes', $ages[$i], '-Session', $sid)) }
}

# The session with tasks: a subagent (its transcript of its own), a command and a Monitor, all
# over a minute in, so their ages read differently by the minute
$taskSid = '99999999-9999-4999-8999-999999999999'
$subagents = Join-Path $T.Out "projects\$taskSid\subagents"
New-Item -ItemType Directory -Force $subagents | Out-Null
$transcript = Join-Path $subagents 'agent-a1b2c3d4e5f6a7b8c.jsonl'
function Tool-Call([string]$Name, [hashtable]$ToolInput, [int]$Read) {
  @{
    type = 'assistant'; isSidechain = $true
    message = @{
      role = 'assistant'
      content = @(@{ type = 'tool_use'; id = 'toolu_x'; name = $Name; input = $ToolInput })
      usage = @{ input_tokens = 2; cache_creation_input_tokens = 400; cache_read_input_tokens = $Read; output_tokens = 180 }
    }
  } | ConvertTo-Json -Compress -Depth 8
}
[IO.File]::WriteAllText($transcript, ((@(
  (@{ type = 'user'; message = @{ role = 'user'; content = 'Review the card' } } | ConvertTo-Json -Compress -Depth 5)
  (Tool-Call 'Read' @{ file_path = 'C:\src\lbterm\TAB-PANEL.md' } 30000)
  (Tool-Call 'Edit' @{ file_path = 'C:\src\render\tasks_card.rs' } 52000)
)) -join "`n") + "`n")
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
# The seat's times, from now (phase 2 takes $now for its own)
$seatAt = $now
$taskList = @{
  sessionId = $taskSid
  tasks = @(
    @{ id = 'a1b2c3d4e5f6a7b8c'; kind = 'agent'; description = 'Review the tasks card'; agentType = 'Explore'; startedAt = $now - 12 * 60000 - 20000; transcript = $transcript }
    @{ id = 'b9x8y7z6w'; kind = 'command'; description = 'Run the LBTerm check'; startedAt = $now - 3 * 60000 - 40000 }
    @{ id = 'm5n4b3v2c'; kind = 'monitor'; description = 'Wait for the build queue'; startedAt = $now - 5 * 60000 - 10000 }
  )
}
[IO.File]::WriteAllText((Join-Path $summaries "$taskSid.tasks.json"), ($taskList | ConvertTo-Json -Compress -Depth 5))
[IO.File]::WriteAllText((Join-Path $summaries "$taskSid.label.txt"), 'Tasks')
$records = @('state=done:app=claude') + @($taskList.tasks | ForEach-Object { "state=working:id=bg/$($_.id)" })
$seq = (Get-UserVarSequence claude_session $taskSid) + (($records | ForEach-Object { "$([char]27)]7501;$_$([char]27)\" }) -join '')
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($seq))
$taskTab = New-PwshProgram "[Console]::Write([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$b64'))); Write-Host 'tasks'; Start-Sleep 3600"

# The tab that names itself (OSC 0) and says its folder (OSC 7), as Claude Code's session does once
# its hooks have, and while churn.go is there starts a short program every -ChurnEvery s, printing
# nothing (ping's output taken by pwsh), as Claude Code starts its statusline and hooks. ConPTY
# passes a title on only once a program has run (its folder at once), so one runs as it starts:
# till then the pane is titled by its process, and walked for its programs as it should be
$churnGo = Join-Path $T.Out 'churn.go'
Remove-Item -LiteralPath $churnGo -ErrorAction SilentlyContinue
$churnFolder = 'file://localhost/' + ($T.Out -replace '\\', '/')
$churnTab = New-PwshProgram -Script @"
`$e = [char]27
[Console]::Write("`$e]0;Churn`$e\`$e]7;$churnFolder`$e\")
Write-Host 'churn'
`$null = & "`$env:SystemRoot\System32\PING.EXE" -n 1 127.0.0.1
`$ran = 0
while (`$true) {
  if (Test-Path -LiteralPath '$($churnGo -replace "'", "''")') {
    `$null = & "`$env:SystemRoot\System32\PING.EXE" -n 1 127.0.0.1
    `$ran++
    [IO.File]::WriteAllText('$($churnGo -replace "'", "''").ran', "`$ran")
    Start-Sleep -Milliseconds $([int]($ChurnEvery * 1000))
  } else { Start-Sleep -Milliseconds 100 }
}
"@

$cfg = Join-Path $T.Out 'config\at-rest.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$domain = if ($mux) {
  @"
config.unix_domains = { {
  name = 'atrest',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
$inboxFolders = ($folderList | ForEach-Object { "'$(ConvertTo-LuaPath $_)'" }) -join ', '
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 140
config.initial_rows = 40
config.window_close_confirmation = 'NeverPrompt'
config.lbterm_compositor = true
config.lbterm_tab_panel = { enabled = true, open_delay_ms = 0 }
rawset(config, 'lbterm_summaries', '$(ConvertTo-LuaPath $summaries)')
rawset(config, 'lbterm_inbox', { folders = { $inboxFolders } })
rawset(config, 'lbterm_conductor', { state_file = '$(ConvertTo-LuaPath $button)', hub_pipe = [[\\.\pipe\$hubPipe]], seat_pipe = [[\\.\pipe\$seatPipe]] })
rawset(config, 'lbterm_links', { file = '$(ConvertTo-LuaPath $linksFile)' })
$domain
"@

$cost = [ordered]@{ build = $version; mux = $mux; rest = $null; seat = $null; hub = $null; neighbours = $null; churn = $null; card = $null; changes = @(); print = $null; server = $null; rest_metrics = $null }
$seat.Greeting = @(
  (Seat-View 1),
  ('{"type":"seat.items","reset":true,"items":[{"id":"u1","at":' + ($seatAt - 50000) + ',"who":"you","from":"lbterm","text":"what is waiting on me?","queued":false},{"id":"r1","at":' + ($seatAt - 48000) + ',"who":"conductor","text":"Nothing is waiting on you."}]}')
)
$seat.Serve()
$hub.Serve()
Invoke-Test {
  # lbterm::procs at debug, for 1e: at rest nothing moves, so it says nothing (1 judges the writes)
  $open = @{ ConfigFile = $cfg; Program = $openTab; UserProfile = $home2; PostedPointer = $true; WorkingDirectory = $T.Repo; TimeoutSec = 30; Log = 'info,lbterm::procs=debug' }
  if ($mux) { $open.Domain = 'atrest'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  if ($mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $first = (Get-Panes -Server:$mux)[0].pane_id
  if ($mux) {
    # `connect` runs the server's default program in the first tab: the open tab's goes in a tab
    # of its own, and that one goes
    $default = $first
    $first = New-Tab -From $default -Program $openTab -Server
    Wait-Change { param($s) @($s.tabs).Count -eq 2 } -What 'the open tab' | Out-Null
    Invoke-Cli kill-pane --pane-id $default -Server -Check | Out-Null
    Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'the default tab gone' | Out-Null
  }
  # Claude Code's ✳ before each title, as it titles a session that has handed back
  foreach ($s in $sessions) {
    $pane = New-Tab -From $first -NoActivate -Program $s.Program -Server:$mux
    Invoke-Cli set-tab-title --pane-id $pane "$([char]0x2733) Session $($sessions.IndexOf($s) + 1)" -Server:$mux | Out-Null
  }
  New-Tab -From $first -NoActivate -Program $taskTab -Server:$mux | Out-Null
  $churnPane = New-Tab -From $first -NoActivate -Program $churnTab -Server:$mux
  $tabs = $sessions.Count + 3
  $s = Wait-Change { param($s)
    $rows = @($s.rail.rows)
    @($s.tabs).Count -eq $tabs -and @($rows | Where-Object { $_.label -like 'Session *' -and $_.state -eq 'idle' -and $_.age }).Count -eq $sessions.Count -and
      @($rows | Where-Object { $_.waiting -eq 3 }).Count -eq 1 -and $s.strip.inbox -eq $Messages -and
      @($s.thumbnails | Where-Object { $_.state -eq 'shown' }).Count -ge 1 -and $null -ne $s.rail.conductor -and
      @($rows | Where-Object { $_.title -eq 'Churn' }).Count -eq 1
  } -What "$tabs tabs with their labels, the tasks row, the churning tab's own title, $Messages messages and the thumbnail" -TimeoutSec 30 -Animating
  $taskRow = @($s.rail.rows | Where-Object { $_.waiting -eq 3 })[0]
  # The seat subscribed to, and the hub told where every session is
  if (-not (Wait-Until { $seat.Subscribers -ge 1 } -TimeoutSec 15)) { throw 'LBTerm never subscribed to the played seat' }
  if (-not (Wait-Until { $hub.Count('lbterm.subscribe') -ge 1 -and $hub.Count('lbterm.panes') -ge 1 } -TimeoutSec 15)) { throw 'LBTerm never told the played hub where its sessions are' }
  # Long enough for every thread's first look to be over
  Start-Sleep 4
  Wait-Settled -What 'the window at rest' | Out-Null
  $server = if ($mux) { [int]@(Get-TestServers)[0].ProcessId } else { $null }
  if ($mux -and -not $server) { throw 'no test mux server running' }
  $frameNow = { [long](Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).frame }

  # 1. At rest, the rail closed. Not before 15 s in: 10 s after a GUI starts, upstream's
  # update_checker sweeps every GUI socket in the runtime folder for dead ones, reading the whole
  # folder (10,567 entries on 2026-10-30, about 490 I/O operations and 16 to 24 ms) and
  # connecting to every live GUI, once
  $left = 15 - ((Get-Date) - $w.Opened).TotalSeconds
  if ($left -gt 0) { Start-Sleep -Milliseconds ([int]($left * 1000)) }
  $f0 = & $frameNow
  Get-ControlMetrics $w -Reset | Out-Null
  $a = [LbtermRest]::Take($w.Pid); $sa = if ($server) { [LbtermRest]::Take($server) }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Start-Sleep $RestSeconds
  $b = [LbtermRest]::Take($w.Pid); $sb = if ($server) { [LbtermRest]::Take($server) }
  $cost.rest_metrics = Get-ControlMetrics $w
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $rest = Measure-Span $a $b $secs
  $rest | Add-Member frames ((& $frameNow) - $f0)
  $cost.rest = $rest
  "at rest, the rail closed: $($rest.frames) frames"
  Format-Span $rest 'the GUI'
  if ($server) { $cost.server = Measure-Span $sa $sb $secs; Format-Span $cost.server 'its mux server (not judged)' }
  $named = @($rest.threads | Where-Object { ($_.name -like 'lbterm-*' -or $_.name -like 'notify-rs*') -and $_.wakes -gt 0 })
  $main = [long](@($rest.threads | Where-Object { $_.name -eq 'main' }) | Measure-Object wakes -Sum).Sum
  $perSec = { param([double]$n) [Math]::Round($n / $secs, 2) }
  "LBTerm's threads but the GUI thread: $($rest.own_wakes) wakes ($(& $perSec $rest.own_wakes)/s); the GUI thread $main ($(& $perSec $main)/s)"
  if (-not $Measure) {
    if ($rest.frames -gt 0) { Add-Failure "at rest the window painted $($rest.frames) frames in $secs s" }
    foreach ($th in $named) { Add-Failure "at rest $($th.name) woke $($th.wakes) times in $secs s" }
    if ((& $perSec $main) -gt $MainWakes) { Add-Failure "at rest the GUI thread woke $(& $perSec $main) times a second, over $MainWakes" }
    if ($rest.reads -gt 0 -or $rest.writes -gt 0) { Add-Failure "at rest the process read $($rest.reads) times and wrote $($rest.writes) in $secs s" }
    if ((& $perSec $rest.others) -gt $OtherIo) { Add-Failure "at rest the process did $(& $perSec $rest.others) other I/O operations a second, over $OtherIo" }
  }
  # The GUI's own word on the span (perf/budgets' counters, lbterm/PERF.md), from a build that
  # has them: no timer fired at rest but those that tick on purpose (the budget leaves those out,
  # as upstream's status update), and no task ran on the GUI thread but another program
  # connecting to its GUI socket (each LBTerm's update_checker, 10 s after it starts, connects to
  # every live one: the other sessions' test windows starting)
  $atRest = if ($cost.rest_metrics) { $cost.rest_metrics.budget.at_rest }
  if ($atRest) {
    $allowedTasks = @('wezterm-mux-server-impl/src/local.rs:*')
    $timerSites = @($atRest.timer_sites | ForEach-Object { "$($_.site) ($($_.fired))" })
    $taskSites = @($atRest.task_sites | Where-Object { $site = $_.site; -not ($allowedTasks | Where-Object { $site -like $_ }) } | ForEach-Object { "$($_.site) ($($_.runs))" })
    "its counters at rest: $($atRest.frames) frames, $($atRest.timers) timers fired, $($atRest.tasks) tasks ($(@($atRest.task_sites | ForEach-Object { "$($_.site) $($_.runs)" }) -join ', '))"
    if (-not $Measure) {
      if ($atRest.frames -gt 0 -or $atRest.unasked -gt 0) { Add-Failure "at rest the GUI counted $($atRest.frames) frames ($($atRest.unasked) unasked)" }
      if ($atRest.timers -gt 0) { Add-Failure "at rest timers fired on the GUI thread: $($timerSites -join ', ')" }
      if ($taskSites.Count) { Add-Failure "at rest tasks ran on the GUI thread: $($taskSites -join ', ')" }
    }
  } else { "its counters at rest: not answered (a build without perf/budgets' metrics), not judged" }
  # Another LBTerm starting connects to this one's GUI socket (its update_checker), which wakes the
  # socket's listener and async-io: about 1.5 wakes a connection measured. Those are the PC's, as
  # other programs' messages to the GUI thread are, and each is allowed two
  $connections = if ($atRest) { [long](@($atRest.task_sites | Where-Object { $_.site -like 'wezterm-mux-server-impl/src/local.rs:*' }) | Measure-Object runs -Sum).Sum } else { 0 }
  $ownAllowed = $OwnWakes * $secs + 2 * $connections
  $rest | Add-Member connections $connections
  if (-not $Measure -and $rest.own_wakes -gt $ownAllowed) { Add-Failure "at rest LBTerm's threads but the GUI thread woke $($rest.own_wakes) times in $secs s, over $([Math]::Round($ownAllowed)) ($OwnWakes a second, and two for each of $connections connections to its GUI socket)" }

  # 1b. The seat talking with the Conductor's view closed: a new view every 250 ms, its first
  # session going between working and idle as a session's status does, and for the second half
  # the Conductor's reply streaming (seat.delta every 100 ms, the whole text so far each time),
  # for -SeatSeconds. Nothing the window shows changes (the pad's asks are button.json's), so it
  # paints nothing and its GUI thread wakes no more than at rest; the seat's own thread reads
  # each line (lbterm-seat), and no other thread of LBTerm's wakes
  $f0 = & $frameNow
  $a = [LbtermRest]::Take($w.Pid)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $views = 0; $deltas = 0; $seq = 2; $reply = ''
  $nextView = 0.0; $nextDelta = $SeatSeconds / 2
  while ($clock.Elapsed.TotalSeconds -lt $SeatSeconds) {
    $elapsed = $clock.Elapsed.TotalSeconds
    if ($elapsed -ge $nextView) {
      [void]$seat.Push((Seat-View $seq $(if ($seq % 2) { 'working' } else { 'idle' })))
      $seq++; $views++; $nextView += 0.25
    }
    if ($elapsed -ge $nextDelta) {
      $reply += "word$deltas "
      [void]$seat.Push((@{ type = 'seat.delta'; id = 'r2'; text = $reply } | ConvertTo-Json -Compress))
      $deltas++; $nextDelta += 0.1
    }
    Start-Sleep -Milliseconds 10
  }
  # The reply finished, as the seat ends one
  [void]$seat.Push(('{"type":"seat.items","items":[{"id":"r2","at":' + $seatAt + ',"who":"conductor","text":"' + $reply.Trim() + '"}]}'))
  Start-Sleep -Milliseconds 500
  $b = [LbtermRest]::Take($w.Pid)
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $talk = Measure-Span $a $b $secs
  $talk | Add-Member frames ((& $frameNow) - $f0)
  $talk | Add-Member lines ($views + $deltas + 1)
  $cost.seat = $talk
  "the seat talking, the view closed: $views views, $deltas deltas and the finished item in $secs s, $($talk.frames) frames"
  Format-Span $talk 'the GUI'
  $seatThread = [long](@($talk.threads | Where-Object { $_.name -eq 'lbterm-seat' }) | Measure-Object wakes -Sum).Sum
  $mainTalk = [long](@($talk.threads | Where-Object { $_.name -eq 'main' }) | Measure-Object wakes -Sum).Sum
  "the seat's thread woke $seatThread times for $($talk.lines) lines; the GUI thread $mainTalk ($(& $perSec $mainTalk)/s)"
  if (-not $Measure) {
    if ($talk.frames -gt 0) { Add-Failure "with the view closed the seat's $($talk.lines) lines painted $($talk.frames) frames" }
    foreach ($th in @($talk.threads | Where-Object { ($_.name -like 'lbterm-*' -or $_.name -like 'notify-rs*') -and $_.name -ne 'lbterm-seat' -and $_.wakes -gt 0 })) { Add-Failure "with the view closed the seat's lines woke $($th.name) $($th.wakes) times" }
    if ((& $perSec $mainTalk) -gt $MainWakes) { Add-Failure "with the view closed the seat's lines woke the GUI thread $(& $perSec $mainTalk) times a second, over $MainWakes" }
  }

  # 1c. The hub's guesses said again, as the hub does every 45 s with a later end and the same
  # sessions: three sessions' subagents' bars (the rows of sessions that handed back show only
  # those), sent with an end 12 s on and again every 9 s (the hub's 45 s
  # against an end 50 s on, made shorter), for -HubSeconds. Nothing the window shows changes, so
  # it paints nothing; until 2026-10-31 the rail's wake at the old end painted a frame each time
  $guessed = @($sessions[0..2] | ForEach-Object { $_.Sid })
  $bars = '"sessions":[' + (($guessed | ForEach-Object { '{"sessionId":"' + $_ + '","percent":40,"left":240,"agents":[{"id":"a1","kind":"agent","what":"Survey the dashboards","percent":50,"left":120}]}' }) -join ',') + ']}'
  $hub.Resend('{"type":"conductor.estimates","until":', ",$bars", 12000, 9000, 1)
  Wait-Change { param($s) @($s.rail.rows | Where-Object { @($_.agent_bars).Count -gt 0 }).Count -eq $guessed.Count } -What "the hub's bars on $($guessed.Count) rows" | Out-Null
  Start-Sleep 1
  $f0 = & $frameNow
  Get-ControlMetrics $w -Reset | Out-Null
  $a = [LbtermRest]::Take($w.Pid)
  $sent = $hub.Resent
  $clock = [Diagnostics.Stopwatch]::StartNew()
  # Said again from 9 s after the first, as each says it before its end
  Start-Sleep -Milliseconds 8000
  $hub.Resend('{"type":"conductor.estimates","until":', ",$bars", 12000, 9000, [int][Math]::Ceiling(($HubSeconds - 8) / 9))
  Start-Sleep -Milliseconds ([int](($HubSeconds - 8) * 1000))
  $b = [LbtermRest]::Take($w.Pid)
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $again = Measure-Span $a $b $secs
  $again | Add-Member frames ((& $frameNow) - $f0)
  $again | Add-Member lines ($hub.Resent - $sent)
  $again | Add-Member asked (Format-Askers (Get-ControlMetrics $w))
  $cost.hub = $again
  $still = @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).rail.rows | Where-Object { @($_.agent_bars).Count -gt 0 }).Count
  "the hub's guesses said again $($again.lines) times in $secs s, the bars still on $still rows: $($again.frames) frames"
  Format-Span $again 'the GUI'
  $hubThread = [long](@($again.threads | Where-Object { $_.name -eq 'lbterm-hub' }) | Measure-Object wakes -Sum).Sum
  $mainHub = [long](@($again.threads | Where-Object { $_.name -eq 'main' }) | Measure-Object wakes -Sum).Sum
  "the hub's thread woke $hubThread times for $($again.lines) lines; the GUI thread $mainHub ($(& $perSec $mainHub)/s)"
  if (-not $Measure) {
    if ($still -ne $guessed.Count) { Add-Failure "the hub's bars went from $($guessed.Count - $still) rows while it said them again" }
    if ($again.frames -gt 0) { Add-Failure "the hub saying its guesses again with a later end painted $($again.frames) frames in $secs s, asked by $($again.asked)" }
    foreach ($th in @($again.threads | Where-Object { ($_.name -like 'lbterm-*' -or $_.name -like 'notify-rs*') -and $_.name -ne 'lbterm-hub' -and $_.wakes -gt 0 })) { Add-Failure "the hub's guesses said again woke $($th.name) $($th.wakes) times" }
    if ((& $perSec $mainHub) -gt $MainWakes) { Add-Failure "the hub's guesses said again woke the GUI thread $(& $perSec $mainHub) times a second, over $MainWakes" }
  }
  # Its bars gone, as the hub ends them
  [void]$hub.Push('{"type":"conductor.estimates","until":' + ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() + 60000) + ',"sessions":[]}')
  Wait-Change { param($s) @($s.rail.rows | Where-Object { @($_.agent_bars).Count -gt 0 }).Count -eq 0 } -What "the hub's bars gone" | Out-Null

  # 1d. Neighbours writing beside the files watched, at the rates measured in Lachlan's machine
  # state, each as their programs write theirs (a file beside it renamed over it): Windows
  # watches folders, so each wakes notify's thread, which passes nothing on (lbterm/AT-REST.md,
  # "What still wakes": the cost goes once the Conductor and Terminal give these files folders of
  # their own). Nothing the window shows changes: no frame, no thread of LBTerm's named lbterm-*
  # waking, and notify's threads -NotifyWakesAWrite a write at most
  $f0 = & $frameNow
  Get-ControlMetrics $w -Reset | Out-Null
  $a = [LbtermRest]::Take($w.Pid)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $writes = 0; $nextMachine = 0.0; $nextSub = 0.0; $nextConductor = 0.0
  $sub = Join-Path $machine 'warden'
  New-Item -ItemType Directory -Force $sub | Out-Null
  # A bare rename on purpose: nothing opens these files (LBTerm's watch only wakes on the folder's
  # change), so Windows never refuses it, and the write is the programs' own way of writing theirs
  function Write-Neighbour([string]$File) {
    [IO.File]::WriteAllText("$File.$PID.tmp", "{`"at`":$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())}"); Move-Item -Force -LiteralPath "$File.$PID.tmp" -Destination $File
  }
  while ($clock.Elapsed.TotalSeconds -lt $NeighbourSeconds) {
    $elapsed = $clock.Elapsed.TotalSeconds
    if ($elapsed -ge $nextMachine) { Write-Neighbour (Join-Path $machine 'apc-state.json'); $writes++; $nextMachine += 1 / $MachineWrites }
    if ($elapsed -ge $nextSub) { Write-Neighbour (Join-Path $sub 'status.json'); $writes++; $nextSub += 1 / $SubfolderWrites }
    if ($elapsed -ge $nextConductor) { Write-Neighbour (Join-Path $conductorDir 'seat.json'); $writes++; $nextConductor += 1 / $ConductorWrites }
    Start-Sleep -Milliseconds 5
  }
  Start-Sleep -Milliseconds 300
  $b = [LbtermRest]::Take($w.Pid)
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $near = Measure-Span $a $b $secs
  $near | Add-Member frames ((& $frameNow) - $f0)
  $near | Add-Member neighbour_writes $writes
  $near | Add-Member asked (Format-Askers (Get-ControlMetrics $w))
  $notifyWakes = [long](@($near.threads | Where-Object { $_.name -like 'notify-rs*' }) | Measure-Object wakes -Sum).Sum
  $near | Add-Member notify_wakes $notifyWakes
  $cost.neighbours = $near
  "neighbours writing beside the files watched: $writes writes in $secs s, $($near.frames) frames, notify's threads $notifyWakes wakes ($([Math]::Round($notifyWakes / [Math]::Max(1, $writes), 2)) a write)"
  Format-Span $near 'the GUI'
  if (-not $Measure) {
    if ($near.frames -gt 0) { Add-Failure "the neighbours' $writes writes painted $($near.frames) frames, asked by $($near.asked)" }
    foreach ($th in @($near.threads | Where-Object { $_.name -like 'lbterm-*' -and $_.wakes -gt 0 })) { Add-Failure "the neighbours' $writes writes woke $($th.name) $($th.wakes) times" }
    if ($notifyWakes -gt $NotifyWakesAWrite * $writes) { Add-Failure "the neighbours' $writes writes woke notify's threads $notifyWakes times, over $NotifyWakesAWrite a write" }
  }

  # 1e. The tab that names itself and says its folder starting a short program every -ChurnEvery
  # s, printing nothing, for -ChurnSeconds: its job hears each, and nothing walks or reads
  # (lbterm::procs's lines, the GUI's log, or the server's on a mux domain), no frame, no task on
  # the GUI thread, and no lbterm-* thread but lbterm-jobs waking
  $procsLog = if ($server) { Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$server.txt" } else { $w.Log }
  $procsMark = if (Test-Path -LiteralPath $procsLog) { (Get-Item -LiteralPath $procsLog).Length } else { 0 }
  $f0 = & $frameNow
  Get-ControlMetrics $w -Reset | Out-Null
  $a = [LbtermRest]::Take($w.Pid); $sa = if ($server) { [LbtermRest]::Take($server) }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  [IO.File]::WriteAllText($churnGo, 'go')
  Start-Sleep -Milliseconds ([int]($ChurnSeconds * 1000))
  Remove-Item -LiteralPath $churnGo
  # The last program's end heard
  Start-Sleep -Milliseconds 1000
  $b = [LbtermRest]::Take($w.Pid); $sb = if ($server) { [LbtermRest]::Take($server) }
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $ran = if (Test-Path -LiteralPath "$churnGo.ran") { [int](Get-Content -LiteralPath "$churnGo.ran" -Raw) } else { 0 }
  $churn = Measure-Span $a $b $secs
  $churn | Add-Member frames ((& $frameNow) - $f0)
  $churn | Add-Member programs $ran
  $churnMetrics = Get-ControlMetrics $w
  $churn | Add-Member asked (Format-Askers $churnMetrics)
  $procLines = @(Read-LogLines $procsLog $procsMark | Where-Object { $_ -match 'lbterm::procs\s+> pane \d+: ' } | ForEach-Object { ($_ -split 'lbterm::procs\s+> ', 2)[1] })
  $heard = @($procLines | Where-Object { $_ -match "^pane $($churnPane): its processes moved: .*\(shown: false\)" }).Count
  $brought = @($procLines | Where-Object { $_ -match ': (walk|cwd read)' })
  $shownMoves = @($procLines | Where-Object { $_ -match '\(shown: true\)' })
  $churn | Add-Member heard $heard
  $churn | Add-Member brought_up_to_date $brought.Count
  # The panes' jobs are heard where the panes are: the window's with -Local, its server's
  $jobSpan = if ($server) { Measure-Span $sa $sb $secs } else { $churn }
  $jobs = [long](@($jobSpan.threads | Where-Object { $_.name -eq 'lbterm-jobs' }) | Measure-Object wakes -Sum).Sum
  $churn | Add-Member job_wakes $jobs
  if ($server) { $churn | Add-Member server $jobSpan }
  $cost.churn = $churn
  "the tab starting a program every $ChurnEvery s: $ran programs in $secs s, its job heard $heard moves, $($brought.Count) walks or reads, $($churn.frames) frames, lbterm-jobs woke $jobs times ($(if ($server) { "the server's" } else { "the window's" }))"
  Format-Span $churn 'the GUI'
  if ($server) { Format-Span $churn.server 'its mux server (not judged)' }
  $mainChurn = [long](@($churn.threads | Where-Object { $_.name -eq 'main' }) | Measure-Object wakes -Sum).Sum
  # ConPTY sends the title again as each program ends: the output's notice, the held alerts and
  # the window's subscription, as upstream takes a title set again (reported, not judged)
  $conptySites = 'mux/src/lib.rs:*', 'mux/src/localpane.rs:*', 'wezterm-gui/src/termwindow/mod.rs:*'
  $sites = if ($churnMetrics) { @($churnMetrics.budget.at_rest.task_sites | Where-Object { $_.site -notlike 'wezterm-mux-server-impl/src/local.rs:*' }) } else { @() }
  $isConpty = { param($site) @($conptySites | Where-Object { $site -like $_ }).Count -gt 0 }
  $churnTasks = @($sites | Where-Object { -not (& $isConpty $_.site) } | ForEach-Object { "$($_.site) ($($_.runs))" })
  $titleAgain = @($sites | Where-Object { & $isConpty $_.site } | ForEach-Object { "$($_.site) ($($_.runs))" })
  $churn | Add-Member title_again_tasks $titleAgain
  "  the title ConPTY sent again: tasks $(if ($titleAgain) { $titleAgain -join ', ' } else { 'none' }); the GUI thread woke $(& $perSec $mainChurn) times a second (not judged)"
  if (-not $Measure) {
    if ($ran -lt 3) { Add-Failure "the churning tab ran $ran programs in $secs s, too few to judge" }
    if ($heard -lt $ran) { Add-Failure "the churning tab's job heard $heard moves for $ran programs: $($procLines -join '; ')" }
    if ($brought.Count) { Add-Failure "a tab that names itself and says its folder had its processes walked or read $($brought.Count) times for $ran programs: $($brought -join '; ')" }
    if ($shownMoves.Count) { Add-Failure "a move was taken as shown: $($shownMoves -join '; ')" }
    if ($churn.frames -gt 0) { Add-Failure "the churning tab's $ran programs painted $($churn.frames) frames, asked by $($churn.asked)" }
    if ($churnTasks.Count) { Add-Failure "the churning tab's programs ran tasks of LBTerm's on the GUI thread: $($churnTasks -join ', ')" }
    foreach ($th in @($churn.threads | Where-Object { $_.name -like 'lbterm-*' -and $_.name -ne 'lbterm-jobs' -and $_.wakes -gt 0 })) { Add-Failure "the churning tab's programs woke $($th.name) $($th.wakes) times" }
    if (-not $server -and $jobs -gt $JobWakesAProgram * [Math]::Max(1, $ran)) { Add-Failure "the churning tab's $ran programs woke lbterm-jobs $jobs times, over $JobWakesAProgram a program" }
  }

  # 2. The tasks card open, at rest
  $pad = Get-RectCenter $taskRow.pad
  Send-Mouse $w Move $pad[0] $pad[1]
  $s = Wait-Change { param($s) $s.rail.open -and $s.tasks -and $s.tasks.row -eq $taskRow.row } -What 'the tasks card open' -Animating -TimeoutSec 20
  "the card says: $(@($s.tasks.lines) -join ' | ')"
  Start-Sleep 1
  $before = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $f0 = & $frameNow
  $a = [LbtermRest]::Take($w.Pid)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Start-Sleep $CardSeconds
  $b = [LbtermRest]::Take($w.Pid)
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $card = Measure-Span $a $b $secs
  $card | Add-Member frames ((& $frameNow) - $f0)
  # The ages that read differently over the span, the rows' and the card's lines': each is a
  # paint, and the dissolve of what changed a frame after it
  $after = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $was = @($before.rail.rows | ForEach-Object { "$($_.tab)" + ' ' + $_.age }) + @($before.tasks.lines)
  $now = @($after.rail.rows | ForEach-Object { "$($_.tab)" + ' ' + $_.age }) + @($after.tasks.lines)
  $aged = @($now | Where-Object { $_ -notin $was }).Count
  $card | Add-Member aged $aged
  $card | Add-Member allowed (2 * $aged + 1)
  $cost.card = $card
  "the card open, at rest: $($card.frames) frames, $aged ages read differently (allowed $($card.allowed))"
  Format-Span $card 'the GUI'
  $transcripts = @($card.threads | Where-Object { $_.name -eq 'lbterm-transcripts' -and $_.wakes -gt 0 })
  if (-not $Measure) {
    if ($card.frames -gt $card.allowed) { Add-Failure "with the card open at rest the window painted $($card.frames) frames in $secs s, where $aged ages reading differently needed $($card.allowed) at most" }
    foreach ($th in $transcripts) { Add-Failure "with the card open at rest the transcripts' thread woke $($th.wakes) times in $secs s" }
  }

  # 3. Each change once, timed to the first frame that shows it
  function Show-Change([string]$Name, [scriptblock]$Do, [scriptblock]$Shows) {
    $f = & $frameNow
    $clock = [Diagnostics.Stopwatch]::StartNew()
    & $Do
    $shownAt = $null
    while ($clock.Elapsed.TotalSeconds -lt 15) {
      $snap = Get-StateSnapshot $w.StateFile
      $sw = if ($snap) { Get-StateWindow $snap -Hwnd ([long]$w.Handle) }
      if ($sw -and [long]$sw.frame -gt $f -and (& $Shows $sw)) { $shownAt = [Math]::Round($clock.Elapsed.TotalMilliseconds); break }
      Start-Sleep -Milliseconds 5
    }
    $row = [pscustomobject]@{ change = $Name; shown_ms = $shownAt }
    $script:cost.changes += $row
    if ($null -eq $shownAt) { Add-Failure "$Name never showed (15 s)" }
    elseif ($shownAt -gt $ShowBudgetMs -and -not $Measure) { Add-Failure "$Name showed $shownAt ms after it, over $ShowBudgetMs" }
    "{0}: {1}" -f $Name, $(if ($null -eq $shownAt) { 'never' } else { "$shownAt ms" })
    Wait-Settled -What "the window after $Name" | Out-Null
  }
  Show-Change "the subagent's transcript growing" {
    [IO.File]::AppendAllText($transcript, (Tool-Call 'Write' @{ file_path = 'C:\src\notes.md' } 61000) + "`n")
  } { param($sw) $sw.tasks -and @($sw.tasks.lines)[1] -like 'Write notes.md*' }
  # The pointer off the window: the card and the panel close
  Send-Leave $w
  Wait-Change { param($s) -not $s.tasks -and -not $s.rail.open } -What 'the card and the panel closed' -Animating | Out-Null
  Show-Change 'a message arriving' {
    [IO.File]::WriteAllText($newMessage, "---`nfrom: Home`nto: P01`nsent: 2026-10-30T09:00`nsubject: `"The test`"`n---`n`nThe body.`n")
  } { param($sw) $sw.strip.inbox -eq $Messages + 1 }
  Show-Change 'a message archived' { Remove-Item -LiteralPath $newMessage } { param($sw) $sw.strip.inbox -eq $Messages }
  # As the seat does: button.json written at once, and the view to its subscribers 250 ms later
  # (seat.ts `changed`, its view timer)
  Show-Change 'the Conductor asking' { Write-Button 1; $seat.PushAfter((Seat-View 900 'idle' 1), 250) } { param($sw) $sw.rail.conductor.asks -eq 1 }
  Show-Change 'the Conductor answered' { Write-Button 0; $seat.PushAfter((Seat-View 901 'idle' 0), 250) } { param($sw) $sw.rail.conductor.asks -eq 0 }
  # The Conductor's folder going and coming back, as a program making its state folder again
  # does: removed (Windows ends its watch) and made again, then renamed away (its watch follows
  # it to its new name, Windows telling it nothing) and a new one made. Until 2026-10-31 the
  # watch went with the folder, nothing written after showed, and nothing was said. The seat's
  # view follows the file 250 ms later, as the seat sends it
  Show-Change 'the Conductor folder removed' { Remove-Item -Recurse -LiteralPath $conductorDir } { param($sw) $sw.rail.conductor.level -eq 'off' }
  Show-Change 'the Conductor folder made again' { New-Item -ItemType Directory $conductorDir | Out-Null; Write-Button 2; $seat.PushAfter((Seat-View 902 'idle' 2), 250) } { param($sw) $sw.rail.conductor.level -ne 'off' -and $sw.rail.conductor.asks -eq 2 }
  $away = "$conductorDir.old"
  Show-Change 'the Conductor folder renamed away' { Rename-Item -LiteralPath $conductorDir -NewName (Split-Path $away -Leaf) } { param($sw) $sw.rail.conductor.level -eq 'off' }
  Show-Change 'a new Conductor folder' { New-Item -ItemType Directory $conductorDir | Out-Null; Write-Button 0; $seat.PushAfter((Seat-View 903 'idle' 0), 250) } { param($sw) $sw.rail.conductor.level -ne 'off' -and $sw.rail.conductor.asks -eq 0 }
  Remove-Item -Recurse -LiteralPath $away
  $waiter = $sessions[0].Sid; $on = $sessions[1].Sid
  Show-Change 'two tabs linked' {
    Write-Links @([ordered]@{ id = "$waiter-$on"; waiter = $waiter; on = $on; at = 1; by = 'test'; what = 'the test'; state = 'waiting'; stateAt = 1; note = '' })
  } { param($sw) @($sw.rail.links).Count -eq 1 }
  Show-Change 'the link gone' { Write-Links @() } { param($sw) @($sw.rail.links).Count -eq 0 }
  Show-Change 'the image deleted' { Remove-Item -LiteralPath $image } { param($sw) @($sw.thumbnails | Where-Object { $_.state -eq 'gone' }).Count -ge 1 }
  Show-Change 'the image back' { [IO.File]::WriteAllBytes($image, $imageBytes) } { param($sw) @($sw.thumbnails | Where-Object { $_.state -eq 'shown' }).Count -ge 1 -and @($sw.thumbnails | Where-Object { $_.state -eq 'gone' }).Count -eq 0 }
  Show-Change 'another build installed' { [IO.File]::WriteAllText($versionFile, '20990101-000000-00000000') } { param($sw) $sw.strip.restart }
  Show-Change 'this build again' { [IO.File]::WriteAllText($versionFile, $version) } { param($sw) -not $sw.strip.restart }

  # 4. The open tab printing a line every 8 ms for -PrintSeconds, the rail closed: every frame
  # paints the rail, which tells the hub where the sessions are only when that changed, and it
  # doesn't here, so the hub hears nothing. The GUI thread's CPU a frame is reported for comparing
  # builds (each frame's bookkeeping: the hub's panes, the Conductor's state, the projects card)
  $panesBefore = $hub.Count('lbterm.panes')
  $f0 = & $frameNow
  $a = [LbtermRest]::Take($w.Pid)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  [IO.File]::WriteAllText($printGo, '')
  Start-Sleep $PrintSeconds
  Remove-Item -LiteralPath $printGo
  Start-Sleep -Milliseconds 300
  $b = [LbtermRest]::Take($w.Pid)
  $secs = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  $print = Measure-Span $a $b $secs
  $print | Add-Member frames ((& $frameNow) - $f0)
  $mainMs = [double](@($print.threads | Where-Object { $_.name -eq 'main' }) | Measure-Object cpu_ms -Sum).Sum
  $print | Add-Member main_ms_a_frame ($(if ($print.frames) { [Math]::Round($mainMs / $print.frames, 3) } else { $null }))
  $print | Add-Member panes_lines ($hub.Count('lbterm.panes') - $panesBefore)
  $cost.print = $print
  "the open tab printing for $PrintSeconds s: $($print.frames) frames, the GUI thread $([Math]::Round($mainMs, 1)) ms ($($print.main_ms_a_frame) ms a frame), $($print.panes_lines) panes lines to the hub"
  if (-not $Measure -and $print.panes_lines -gt 0) { Add-Failure "printing in the open tab sent the hub $($print.panes_lines) panes lines, with no session moving" }

  $cost | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "at-rest: at rest nothing of LBTerm's wakes, paints or reads, its counters naming no timer or task; the seat talking with the view closed, the hub saying its guesses again and neighbours writing beside its files paint nothing; the tasks card open paints only for its ages; every change showed within $ShowBudgetMs ms, its folder gone and back or renamed away included; printing tells the hub nothing"
