#Requires -Version 7
<#
Records the Conductor reading the other tabs and answering (lbterm/TAB-PANEL.md, "The Conductor"):
a demo, staged from end to end, so nothing in it is one of Lachlan's real sessions.

  pwsh -File lbterm\motion\conductor-demo.ps1              # demo.gif and the frames, the pad accepting most
  pwsh -File lbterm\motion\conductor-demo.ps1 -Mode read   # the pad read only: no spectrum turning on it
  pwsh -File lbterm\motion\conductor-demo.ps1 -Live        # the window left open, nothing played

Opens target\release\lbterm-gui.exe (or -Bin's) looking like Lachlan's LBTerm (New-TestConfig)
with the Conductor on and its hub played here on a pipe of its own, as perf/conductor.ps1 plays
it. The first tab is the Conductor's (conductor-tab.ps1, a stand-in for its Claude Code session),
the state file naming its session, so its pad atop the rail is that tab's. Five more tabs say they
hold sessions in safe, made-up work (demo-tab.ps1, labels from a summaries folder of the demo's
own). Then it plays the conversation: Lachlan asks what everyone's up to, the Conductor reads four
sessions at once (the hub says it's thinking about them: their pads' iridescent outlines), says
who needs him, and is told to answer one: it acts on that session (the light going round its pad),
types into its pane (the pad's glint), the session goes back to work (its state dissolving in) and
the strip's summary dissolves into a new one.

The window's USERPROFILE is a scratch folder, so lbterm.lua's handlers write nothing real. It
opens with Start-Unfocused on the right-most screen and never takes focus. -Dpi renders it larger
than the screen's own (120: a quarter up, for the web's 2x screens) at -Cols by -Rows.

Frames go to target\motion\conductor\frames (frame000.png..., frames.csv: frame, ms), every
-StepMs as near as the captures allow, cropped to the client area; gif.py makes demo.gif from them
at real speed. Needs Python with Pillow.
#>
param(
  [ValidateSet('read', 'most', 'all')]
  [string]$Mode = 'most',
  [int]$Dpi = 120,
  [int]$Cols = 76,
  [int]$Rows = 26,
  [int]$StepMs = 33,
  [switch]$Live,
  [switch]$Keep,
  [string]$Bin
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
if ($Live) { $Keep = $true }
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $repo 'target\motion\conductor'
$T = Start-Test 'motion-conductor' -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.conductor-demo'
$home2 = Join-Path $out 'home'
$sums = Join-Path $out 'summaries'
$frames = Join-Path $out 'frames'
$cues = Join-Path $out 'cues.jsonl'
$state = Join-Path $out 'button.json'
$pipeName = "lbterm-conductor-demo-$PID"
New-Item -ItemType Directory -Force $home2, $sums, $frames | Out-Null
New-Item -ItemType File -Force $cues | Out-Null

# The hub, played as perf/conductor.ps1 plays it: what LBTerm sends is kept, and a connection
# whose first line subscribes is pushed to
if (-not ('LbtermConductorDemoHub' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
public static class LbtermConductorDemoHub {
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

# The sessions: made-up work in the projects the portfolio page names, nothing of Lachlan's own
$own = 'c0c0c0c0-0000-4000-8000-0000000000c0'
$sessions = [ordered]@{
  website    = @{ sid = 'c0c0c0c0-0000-4000-8000-000000000001'; label = 'Sea heath'; title = 'Sea heath'; state = 'waiting'; age = 4 }
  spirescape = @{ sid = 'c0c0c0c0-0000-4000-8000-000000000002'; label = 'Rat drops'; title = 'Rat drop table'; state = 'waiting'; age = 1 }
  gloomfell  = @{ sid = 'c0c0c0c0-0000-4000-8000-000000000003'; label = 'Mage v4'; title = 'Mage staff, v4'; state = 'working'; age = 0 }
  qualitylab = @{ sid = 'c0c0c0c0-0000-4000-8000-000000000004'; label = 'Pytest run'; title = 'Nightly pytest run'; state = 'working'; age = 0 }
  lbterm     = @{ sid = 'c0c0c0c0-0000-4000-8000-000000000005'; label = 'Rail labels'; title = 'Rail labels'; state = 'idle'; age = 22 }
}
foreach ($k in $sessions.Keys) {
  $s = $sessions[$k]
  [IO.File]::WriteAllText((Join-Path $sums "$($s.sid).label.txt"), $s.label)
  $s.status = Join-Path $out "status-$k.txt"
  [IO.File]::WriteAllText($s.status, $s.state)
}
# The strip's words for the Conductor's tab, before and after
$summary = Join-Path $sums "$own.txt"
[IO.File]::WriteAllText($summary, "Watching five sessions`n")
[IO.File]::WriteAllText($state, (@{ state = 'on'; mode = $Mode; sessionId = $own; tooltip = 'Conductor is on' } | ConvertTo-Json -Compress))

$modeName = @{ read = 'read only'; most = 'accept most'; all = 'accept all' }[$Mode]
$lua = @"
rawset(config, 'lbterm_conductor', {
  state_file = '$(ConvertTo-LuaPath $state)',
  hub_pipe = [[\\.\pipe\$pipeName]],
  menu = {},
})
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.dpi = $Dpi
config.initial_cols = $Cols
config.initial_rows = $Rows
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
"@

$demoTab = Join-Path $PSScriptRoot 'demo-tab.ps1'
$conductorTab = Join-Path $PSScriptRoot 'conductor-tab.ps1'
[LbtermConductorDemoHub]::Serve($pipeName)
try {
  $w = Open-TestWindow -Lua $lua -UserProfile $home2 -WorkingDirectory $repo `
    -Program (New-PwshProgram -File $conductorTab -ArgumentList @('-Sid', $own, '-Cues', $cues, '-Status', $modeName))
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail.conductor } -What 'the window and the Conductor''s pad' | Out-Null
  $first = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $first "$([char]0x2733) Conductor" | Out-Null
  foreach ($k in $sessions.Keys) {
    $s = $sessions[$k]
    $a = @('-Title', $s.title, '-State', $s.state, '-AgeMinutes', $s.age, '-Session', $s.sid, '-StatusFile', $s.status)
    # Not renamed: a tab's title would take the place of its label on the rail
    $s.pane = New-Tab -From $first -NoActivate -Program (New-PwshProgram -File $demoTab -ArgumentList $a)
  }
  # The hub hears where every session is
  $ok = Wait-Until {
    $line = [LbtermConductorDemoHub]::Heard() | Where-Object { $_ -match '"lbterm.panes"' } | Select-Object -Last 1
    $line -and @(($line | ConvertFrom-Json).panes | Where-Object sessionId -like 'c0c0c0c0-*').Count -ge 6
  } -TimeoutSec 15
  if (-not $ok) { throw "the hub never heard all six sessions: $([LbtermConductorDemoHub]::Heard() -join ' | ')" }
  Wait-State { param($s) @($s.rail.rows).Count -eq 5 -and $s.rail.conductor.open } -What 'five rows, the Conductor''s tab open' | Out-Null
  if ($Live) {
    "Conductor demo window open (pid $($w.Pid)); push focus lines to \\.\pipe\$pipeName from this shell"
    return
  }
  # Renames glint their pads and new tabs grow in: all of that over before the recording starts
  Start-Sleep 2

  $sid = @{}; foreach ($k in $sessions.Keys) { $sid[$k] = $sessions[$k].sid }
  $cue = { param($o) [IO.File]::AppendAllText($cues, ($o | ConvertTo-Json -Compress) + "`n") }
  $focus = { param([object[]]$pairs)
    $list = @($pairs | ForEach-Object { @{ sessionId = $sid[$_[0]]; doing = $_[1] } })
    [void][LbtermConductorDemoHub]::Push((@{ type = 'conductor.focus'; sessions = $list } | ConvertTo-Json -Compress -Depth 4))
  }
  # Typed into a session's pane as Conductor's answer types it, without waiting on the cli
  $sendText = { param([int]$pane, [string]$text)
    $p = [Diagnostics.ProcessStartInfo]::new($T.Cli)
    foreach ($a in @('cli', '--no-auto-start', 'send-text', '--pane-id', $pane, '--no-paste', $text)) { $p.ArgumentList.Add([string]$a) }
    $p.CreateNoWindow = $true; $p.UseShellExecute = $false
    [void][Diagnostics.Process]::Start($p)
  }
  $reads = @('website', 'spirescape', 'gloomfell', 'qualitylab')
  $names = @{ website = 'website-a6'; spirescape = 'spirescape-k2'; gloomfell = 'gloomfell-m4'; qualitylab = 'quality-lab-p9'; lbterm = 'lbterm-c3' }
  $said = 'yes, on a miss too'
  # The conversation, each step at its time in ms
  $steps = @(
    @{ at = 500; do = { & $cue @{ type = 'type'; text = "what's everyone up to?" } } }
    @{ at = 1350; do = { & $cue @{ type = 'send'; spin = 'Reading the fleet' } } }
    @{ at = 1600; do = { & $cue @{ type = 'tool'; id = 'f'; name = 'fleet'; arg = '' } } }
    @{ at = 2000; do = { & $cue @{ type = 'result'; id = 'f'; text = '5 sessions: 2 working, 2 waiting on you, 1 idle' } } }
    @{ at = 2250; do = { & $cue @{ type = 'spin'; text = 'Reading 4 sessions' }; & $cue @{ type = 'tool'; id = 'website'; name = 'read'; arg = $names.website }; & $focus @(, @('website', 'thinking')) } }
    @{ at = 2450; do = { & $cue @{ type = 'tool'; id = 'spirescape'; name = 'read'; arg = $names.spirescape }; & $focus @(@('website', 'thinking'), @('spirescape', 'thinking')) } }
    @{ at = 2650; do = { & $cue @{ type = 'tool'; id = 'gloomfell'; name = 'read'; arg = $names.gloomfell }; & $focus @(@('website', 'thinking'), @('spirescape', 'thinking'), @('gloomfell', 'thinking')) } }
    @{ at = 2850; do = { & $cue @{ type = 'tool'; id = 'qualitylab'; name = 'read'; arg = $names.qualitylab }; & $focus ($reads | ForEach-Object { , @($_, 'thinking') }) } }
    @{ at = 4700; do = { & $cue @{ type = 'group'; ids = $reads; text = 'Read 4 sessions' }; & $focus @() } }
    @{ at = 4900; do = { & $cue @{ type = 'say'; lines = @(
            "Two need you. $($names.spirescape) asks whether a miss should still drop the rat's bones, and $($names.website) has the heath shaded and wants to push. A push is outward, so that one's yours."
            "$($names.gloomfell) is rendering mage v4, and $($names.qualitylab) is 31 of 48 tests in with nothing failing.") } } }
    @{ at = 7300; do = { & $cue @{ type = 'type'; text = "tell spirescape $said" } } }
    @{ at = 8450; do = { & $cue @{ type = 'send'; spin = 'Answering' } } }
    @{ at = 8700; do = { & $cue @{ type = 'tool'; id = 'a'; name = 'answer'; arg = "$($names.spirescape), said: `"$said`"" }; & $focus @(, @('spirescape', 'acting')) } }
    @{ at = 9500; do = { & $sendText $sessions.spirescape.pane $said } }
    @{ at = 10100; do = { & $cue @{ type = 'result'; id = 'a'; text = 'Typed into its prompt' }; [IO.File]::WriteAllText($sessions.spirescape.status, 'working') } }
    @{ at = 10500; do = { & $focus @(); & $cue @{ type = 'say'; lines = @("Done. $($names.spirescape) is working again, and I've left $($names.website)'s push for you.") } } }
    @{ at = 10900; do = { [IO.File]::WriteAllText($summary, "Spirescape answered`n") } }
  )
  $endMs = 12800

  # Frames every $StepMs, saved as they're taken: a whole run's bitmaps don't fit in memory
  $h = Resolve-Handle $w
  $csv = [Collections.Generic.List[string]]::new(); $csv.Add('frame,ms')
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $next = 0; $n = 0
  while ($clock.ElapsedMilliseconds -lt $endMs) {
    while ($next -lt $steps.Count -and $clock.ElapsedMilliseconds -ge $steps[$next].at) { & $steps[$next].do; $next++ }
    $shot = [LbtermE2E.Win]::Capture($h)
    $ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 1)
    if ($shot) {
      $shot.Save((Join-Path $frames ('frame{0:D3}.png' -f $n)))
      $shot.Dispose()
      $csv.Add("$n,$ms"); $n++
    }
    $wait = [int]($n * $StepMs - $clock.ElapsedMilliseconds)
    if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
  }
  Watch-Foreground
  $csv | Set-Content (Join-Path $frames 'frames.csv')
  "$n frames in $([Math]::Round($endMs / 1000, 1)) s to $frames (DPI scale $($w.Scale))"
} finally {
  Close-Test -Keep:$Keep
}
if ($Live) { return }
python (Join-Path $PSScriptRoot 'gif.py') $frames --out (Join-Path $out 'demo.gif') --step 40 --hold 400
