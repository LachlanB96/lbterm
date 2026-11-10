#Requires -Version 7
# Variant: -Unnamed
# Covers: wezterm-gui/src/termwindow/render/scroll_strip.rs, lbterm-extras/src/conversation.rs, lbterm-extras/src/watch.rs, wezterm-gui/src/conversations.rs, lbterm-extras/src/questions.rs, lbterm-extras/src/drafts.rs, wezterm-gui/src/drafts.rs
<#
The scroll strip against real Claude Code working (lbterm/SCROLL-STRIP.md, "What Claude Code
draws"), where every other test has a stand-in draw what the code expects: fake-claude.ps1 was
written to the code's own idea of Claude Code, and passed while real Claude Code's spinner had the
strip read the screen for every one of its ticks (2026-10-31). The Claude Code installed here
(`claude` on PATH) runs in a test window with a config folder of its own (fullscreen, onboarding
done, a made-up API key) and talks to a stand-in for Anthropic's API played here on a loopback port
(ANTHROPIC_BASE_URL): no account is used and nothing leaves the PC. It answers the first prompt at
once with a long answer, taller than the window, and holds the second open, sending nothing but
pings, so Claude Code's spinner turns until the test lets it go.

1. The long answer shows on the strip: its rows, a thumb.
2. The second turn working, at the latest, for -HoldSec: Claude Code draws its spinner several times
   a second, and the strip reads the screen at most once in all (the state file's
   `scroll_strip.looks`). Until 2026-10-31 it read it for each tick.
3. PgUp: the view scrolled up half the area, the strip's view above where it was.
4. The turn let go and done while scrolled up: Claude Code says "1 new message (ctrl+End) ↓" where
   it said "Jump to bottom", and the strip's view stays where Claude Code's is. Until 2026-10-31
   it went to the latest.
5. A voice draft of two lines (`lbterm control draft add`, then `send`): sent into the prompt, and
   the next turn's request holds both lines, a new line between them.

The conversation is named (`claude --name`), as every one of Lachlan's is: Claude Code 2.1.314 on
writes the name into the input box's upper rule, which LBTerm took for no input box until
2026-11-01, so the strip lost Claude Code's view and voice send was refused. -Unnamed runs it
without the name.

Every screen is kept in the output folder's `captures` (`lbterm cli get-text --escapes`, and the
words alone), with the API's requests and the counts: what lbterm-extras' fixtures
(src/conversation/fixtures/claude-<version>-working) are made from when Claude Code draws
differently, its names swapped for others of the same length first. Claude Code reads the CLAUDE.md
files above its folder, inside this tree: an external-imports prompt is answered with its default,
No.

  pwsh -File lbterm\perf\claude-working.ps1 [-Bin <build>] [-HoldSec 6] [-Unnamed]
#>
param(
  [string]$Bin,
  [int]$HoldSec = 6,
  # The most looks at the screen while only the spinner turns
  [int]$SpinnerLooks = 1,
  # The conversation without a name, so its upper rule is all line
  [switch]$Unnamed
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'claude-working' -Bin $Bin

# The stand-in API: every request written down; /v1/messages answered as a stream, the first turn's
# at once with a long answer, every later one held open (pings each second) until Release
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System; using System.IO; using System.Net; using System.Net.Sockets; using System.Text; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public sealed class LbtermStandInApi {
  TcpListener listener; public int Port;
  public readonly List<string> Log = new List<string>();
  // Every turn's request body, as it came
  public readonly List<string> Bodies = new List<string>();
  public readonly ManualResetEvent Release = new ManualResetEvent(false);
  public int Turns = 0; public int Held = 0; public int Answered = 0;
  public void Start() { listener = new TcpListener(IPAddress.Loopback, 0); listener.Start(); Port = ((IPEndPoint)listener.LocalEndpoint).Port; new Thread(Accept) { IsBackground = true }.Start(); }
  void Accept() { while (true) { var c = listener.AcceptTcpClient(); new Thread(() => Serve(c)) { IsBackground = true }.Start(); } }
  static string ReadHead(Stream s) {
    var sb = new StringBuilder(); int b; int state = 0;
    while ((b = s.ReadByte()) >= 0) { sb.Append((char)b); if (b == '\r' || b == '\n') { state++; if (state == 4) return sb.ToString(); } else state = 0; }
    return null;
  }
  void Note(string line) { lock (Log) Log.Add(DateTime.Now.ToString("HH:mm:ss.fff") + " " + line); }
  void Serve(TcpClient c) {
    try {
      using (c) { var s = c.GetStream();
        while (true) {
          var head = ReadHead(s); if (head == null) return;
          var m = Regex.Match(head, "content-length:\\s*(\\d+)", RegexOptions.IgnoreCase);
          int len = m.Success ? int.Parse(m.Groups[1].Value) : 0;
          var body = new byte[len]; int got = 0; while (got < len) { int n = s.Read(body, got, len - got); if (n <= 0) return; got += n; }
          var text = Encoding.UTF8.GetString(body);
          var first = head.Split('\n')[0].Trim();
          Note(first + " " + len + " B");
          if (first.StartsWith("POST /v1/messages/count_tokens")) { Json(s, "{\"input_tokens\":120}"); continue; }
          if (first.StartsWith("POST /v1/messages")) { Messages(s, text); return; }
          Json(s, "{}");
        }
      }
    } catch (Exception e) { Note("error " + e.Message); }
  }
  static void Json(Stream s, string json) {
    var b = Encoding.UTF8.GetBytes(json);
    var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: " + b.Length + "\r\n\r\n");
    s.Write(h, 0, h.Length); s.Write(b, 0, b.Length); s.Flush();
  }
  static void Ev(Stream s, string type, string data) { var b = Encoding.UTF8.GetBytes("event: " + type + "\ndata: " + data + "\n\n"); s.Write(b, 0, b.Length); s.Flush(); }
  static string Quote(string t) { return "\"" + t.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\n", "\\n") + "\""; }
  static void Text(Stream s, string t) {
    Ev(s, "content_block_start", "{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}");
    Ev(s, "content_block_delta", "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":" + Quote(t) + "}}");
  }
  static void Finish(Stream s) {
    Ev(s, "content_block_stop", "{\"type\":\"content_block_stop\",\"index\":0}");
    Ev(s, "message_delta", "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":40}}");
    Ev(s, "message_stop", "{\"type\":\"message_stop\"}");
  }
  void Messages(Stream s, string body) {
    var model = Regex.Match(body, "\"model\":\"([^\"]+)\"").Groups[1].Value;
    if (!body.Contains("\"stream\":true")) {
      Json(s, "{\"id\":\"msg_x\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"" + model + "\",\"content\":[{\"type\":\"text\",\"text\":\"Scroll strip check\"}],\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":4}}");
      return;
    }
    var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
    s.Write(h, 0, h.Length);
    Ev(s, "message_start", "{\"type\":\"message_start\",\"message\":{\"id\":\"msg_" + Guid.NewGuid().ToString("N") + "\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"" + model + "\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":120,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":1}}}");
    // A turn's request offers the tools; a title's or a summary's doesn't
    if (!body.Contains("\"tools\":[{")) { Text(s, "Scroll strip check"); Finish(s); return; }
    lock (Bodies) Bodies.Add(body);
    if (Interlocked.Increment(ref Turns) == 1) {
      var sb = new StringBuilder("Here is a long answer, line by line, so the conversation is taller than the window.\n\n");
      for (int i = 1; i <= 70; i++) sb.Append("- Line " + i + " of the long answer about where the spinner turns.\n");
      Text(s, sb.ToString()); Finish(s); Interlocked.Increment(ref Answered); return;
    }
    Interlocked.Increment(ref Held);
    Text(s, "");
    while (!Release.WaitOne(1000)) Ev(s, "ping", "{\"type\":\"ping\"}");
    Ev(s, "content_block_delta", "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"The spinner turned.\"}}");
    Finish(s);
  }
}
'@

$claude = (Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $claude) { throw 'claude-working needs Claude Code installed: no claude on PATH' }
$api = [LbtermStandInApi]::new()
$api.Start()

# Short folders: the transcript's path, under a folder named after the work folder's, must stay
# under Windows' 260 characters
$configDir = Join-Path $T.Out 'c'
$work = Join-Path $T.Out 'w'
$summaries = Join-Path $T.Out 'summaries'
$captures = Join-Path $T.Out 'captures'
Remove-Item -Recurse -Force -LiteralPath $configDir, $work, $captures -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $configDir, $work, $summaries, $captures | Out-Null
$key = 'sk-ant-api03-' + (-join ((1..40) | ForEach-Object { 'abcdefghijklmnopqrstuvwxyz0123456789'[(Get-Random -Maximum 36)] }))
$version = ((& $claude --version) -split ' ')[0]
"Claude Code $version, the stand-in API on 127.0.0.1:$($api.Port)"
@{ tui = 'fullscreen' } | ConvertTo-Json | Set-Content -Encoding utf8NoBOM (Join-Path $configDir 'settings.json')
[ordered]@{
  hasCompletedOnboarding = $true; lastOnboardingVersion = $version; lastReleaseNotesSeen = $version; theme = 'dark'
  customApiKeyResponses = @{ approved = @($key.Substring($key.Length - 20)); rejected = @() }
  projects = @{ ($work -replace '\\', '/') = @{ hasTrustDialogAccepted = $true; allowedTools = @() } }
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8NoBOM (Join-Path $configDir '.claude.json')

$sid = [guid]::NewGuid().ToString()
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sid))
$name = 'Strip Check'
$nameArg = if ($Unnamed) { '' } else { "--name '$name'" }
# claude_session as Terminal's claudehook sets it, which the strip follows the session by
$launch = @"
`$env:CLAUDE_CONFIG_DIR = '$configDir'
`$env:ANTHROPIC_BASE_URL = 'http://127.0.0.1:$($api.Port)'
`$env:ANTHROPIC_API_KEY = '$key'
`$env:DISABLE_AUTOUPDATER = '1'
`$env:CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1'
`$env:DISABLE_TELEMETRY = '1'
`$env:DISABLE_ERROR_REPORTING = '1'
[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7)
Set-Location -LiteralPath '$work'
& '$claude' --session-id $sid --model claude-haiku-4-5 $nameArg
"@
$lua = @"
config.initial_cols = 120
config.initial_rows = 40
rawset(config, 'lbterm_summaries', '$(ConvertTo-LuaPath $summaries)')
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath (Join-Path $configDir 'projects'))' }
"@

function Screen-Text { (Invoke-Cli get-text --pane-id $script:pane) -join "`n" }
function Save-Screen([string]$name) {
  [IO.File]::WriteAllText((Join-Path $captures "$name.txt"), ((Invoke-Cli get-text --pane-id $script:pane --escapes) -join "`n"))
  $plain = Screen-Text
  [IO.File]::WriteAllText((Join-Path $captures "$name.plain.txt"), $plain)
  $plain
}
function Strip { (Get-StateWindow (Read-State) -Hwnd ([long]$script:win.Handle)).scroll_strip }
function Frame { [long](Get-StateWindow (Read-State) -Hwnd ([long]$script:win.Handle)).frame }
function Type-Line([string]$words) {
  Invoke-Cli send-text --pane-id $script:pane --no-paste $words | Out-Null
  Start-Sleep -Milliseconds 300
  Invoke-Cli send-text --pane-id $script:pane --no-paste "`r" | Out-Null
}
# `lbterm control --class <its class> <args>`: its exit code, its reply and what it said on stderr
function Invoke-Control {
  [CmdletBinding(PositionalBinding = $false)]
  param([Parameter(ValueFromRemainingArguments)][string[]]$Rest)
  $errFile = Join-Path $T.Out 'control-stderr.txt'
  $out = @(& $T.Cli control --class $T.Class @Rest 2>$errFile)
  $code = $LASTEXITCODE
  $text = ($out | Out-String).Trim()
  [pscustomobject]@{
    Code  = $code
    Reply = $(if ($text) { $text | ConvertFrom-Json } else { $null })
    Error = ((Get-Content $errFile -Raw -ErrorAction SilentlyContinue) ?? '').Trim()
  }
}
$cost = [ordered]@{ claude = $version; named = -not $Unnamed; spinner = $null; scrolled = $null; done = $null; voice = $null }

Invoke-Test {
  $script:win = Open-TestWindow -Lua $lua -Program (New-PwshProgram $launch) -WorkingDirectory $work -TimeoutSec 30
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window' | Out-Null
  $script:pane = (Get-Panes)[0].pane_id
  $ready = Wait-Until {
    $t = Screen-Text
    if ($t -match 'Allow external CLAUDE.md file imports' -and $t -match '(?m)^\s*❯ No, disable external imports') { Invoke-Cli send-text --pane-id $script:pane --no-paste "`r" | Out-Null; Start-Sleep 1; return $false }
    ([regex]::Matches($t, '(?m)^─{20,}')).Count -ge 2 -and $t -notmatch 'Enter to confirm'
  } -TimeoutSec 40 -IntervalMs 500
  $start = Save-Screen 'start'
  if (-not $ready) { throw "Claude Code never drew its input box: captures\start.plain.txt" }
  if (-not $Unnamed) {
    if ($start -match "(?m)^─{20,} $([regex]::Escape($name)) ─+\s*$") { "named: the upper rule carries '$name'" }
    else { Add-Failure "Claude Code $version didn't write the name '$name' into the input box's upper rule, so this run tests what -Unnamed does: captures\start.plain.txt" }
  }

  # 1. The long answer on the strip
  Type-Line 'Write a long answer please'
  if (-not (Wait-Until { $api.Answered -ge 1 -and (Screen-Text) -match 'Line 70 of the long answer' } -TimeoutSec 30 -IntervalMs 250)) { Save-Screen 'stuck' | Out-Null; throw "the long answer never showed: $($api.Log -join ' | ')" }
  Start-Sleep 2
  Save-Screen 'answered' | Out-Null
  $strip = Strip
  "answered: the strip $($strip.mode), $($strip.rows) rows, the view at $($strip.view_top)"
  if (-not $strip.rows -or -not $strip.thumb) { Add-Failure "the long answer isn't on the strip: $($strip | ConvertTo-Json -Compress -Depth 4)" }

  # 2. The second turn held, the spinner alone at the latest
  Type-Line 'Check the scroll strip please'
  if (-not (Wait-Until { $api.Held -ge 1 } -TimeoutSec 30 -IntervalMs 200)) { throw "Claude Code never asked for the second turn: $($api.Log -join ' | ')" }
  Start-Sleep 2
  Save-Screen 'working-a' | Out-Null; Start-Sleep -Milliseconds 130; Save-Screen 'working-b' | Out-Null; Start-Sleep -Milliseconds 300; Save-Screen 'working-c' | Out-Null
  $a = Strip; $fa = Frame
  Start-Sleep -Seconds $HoldSec
  $b = Strip; $fb = Frame
  $latest = $b.view_top
  $cost.spinner = [ordered]@{ seconds = $HoldSec; frames = $fb - $fa; looks = [long]$b.looks - [long]$a.looks; view_top = $latest }
  "working at the latest, the spinner alone for $HoldSec s: $($fb - $fa) frames, $($cost.spinner.looks) looks at the screen"
  if ($fb - $fa -lt $HoldSec) { Add-Failure "the spinner drew $($fb - $fa) frames in $HoldSec s: too few to say what its ticks cost" }
  if ($cost.spinner.looks -gt $SpinnerLooks) { Add-Failure "the spinner alone had the strip read the screen $($cost.spinner.looks) times in $HoldSec s, over $SpinnerLooks" }

  # 3. Scrolled up while it works
  Invoke-Cli send-text --pane-id $script:pane --no-paste "$([char]27)[5~" | Out-Null
  $up = Wait-Until { (Strip).view_top -lt $latest } -TimeoutSec 5 -IntervalMs 100
  Start-Sleep 1
  Save-Screen 'up-a' | Out-Null; Start-Sleep -Milliseconds 130; Save-Screen 'up-b' | Out-Null; Start-Sleep -Milliseconds 300; Save-Screen 'up-c' | Out-Null
  $scrolled = (Strip).view_top
  $cost.scrolled = [ordered]@{ view_top = $scrolled }
  "PgUp: the view at $scrolled, from $latest"
  if (-not $up) { Add-Failure "PgUp didn't move the strip's view from $latest" }

  # 4. Done while scrolled up
  $api.Release.Set() | Out-Null
  $done = Wait-Until { (Screen-Text) -match 'new message' } -TimeoutSec 15 -IntervalMs 200
  Start-Sleep 1
  Save-Screen 'done-up' | Out-Null
  $strip = Strip
  $cost.done = [ordered]@{ view_top = $strip.view_top; rows = $strip.rows }
  "done while scrolled up: the view at $($strip.view_top) of $($strip.rows) rows"
  if (-not $done) { Add-Failure "Claude Code never said a new message came while scrolled up: captures\done-up.plain.txt" }
  elseif ([Math]::Abs([long]$strip.view_top - [long]$scrolled) -gt 2) { Add-Failure "the turn done while scrolled up moved the strip's view from $scrolled to $($strip.view_top), where Claude Code's stayed" }

  # 5. A voice draft of two lines, sent into the prompt
  $turns = $api.Bodies.Count
  $added = @((Invoke-Control draft add 'Voice line one'), (Invoke-Control draft add '\nVoice line two'))
  $bad = @($added | Where-Object Code -ne 0)
  if ($bad) { Add-Failure "draft add: $($bad | ForEach-Object { "exit $($_.Code) $($_.Error)" })" }
  $sent = Invoke-Control draft send
  $got = Wait-Until { $api.Bodies.Count -gt $turns } -TimeoutSec 15 -IntervalMs 200
  Save-Screen 'voice' | Out-Null
  $body = if ($got) { $api.Bodies[$api.Bodies.Count - 1] } else { '' }
  $both = $body.Contains('Voice line one\nVoice line two')
  $cost.voice = [ordered]@{ send = $sent.Code; error = $sent.Error; turn = [bool]$got; lines = $both }
  if ($sent.Code -ne 0) { Add-Failure "draft send at the prompt was refused: '$($sent.Error)' (captures\done-up.plain.txt)" }
  elseif (-not $got) { Add-Failure "draft send said it sent, but Claude Code asked for no turn in 15 s: captures\voice.plain.txt" }
  elseif (-not $both) { Add-Failure "the voice draft's turn doesn't hold its two lines with a new line between them: captures\voice.plain.txt" }
  else { 'voice: a two-line draft sent at the prompt, its turn holding both lines' }

  [IO.File]::WriteAllLines((Join-Path $captures 'api.log'), [string[]]$api.Log)
  $cost | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "claude-working: real Claude Code$(if ($Unnamed) { '' } else { " (a named conversation)" }): its long answer on the strip, its spinner alone reading the screen $SpinnerLooks times at most, scrolled up and a turn done there followed, a two-line voice draft sent"
