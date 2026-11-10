#Requires -Version 7
# Covers: mux/src/lbterm_ptyrec.rs, mux/src/lib.rs, mux/tests/lbterm_pty_frames.rs
<#
How real Claude Code writes its frames, as a mux server's pty reader sees them (lbterm/FLOWS.md, the
pane-output flow, rows 1 to 3 and its "Not traced yet"): the Claude Code installed here (`claude` on
PATH) runs in the first tab of a mux domain of the test's own, in a window as big as Lachlan's (1080
by 1920, his look), with a config folder of its own (fullscreen, onboarding done, a made-up API key),
talking to a stand-in for Anthropic's API on a loopback port (ANTHROPIC_BASE_URL): no account is
used and nothing leaves the PC. The window and so its server get LBTERM_PTY_RECORD, and the server
records every read of every pane's pty (mux/src/lbterm_ptyrec.rs) in the output folder's `ptyrec`.

1. Claude Code started, its input box drawn at the window's size.
2. A prompt typed, and a long answer streamed: -Lines lines of markdown (headings, paragraphs that
   wrap, lists, a code block), a few words a text delta every -DeltaMs, as the API streams one.
3. A prompt typed whose turn is -Tools tool calls, as most of a working session's turns are: each
   answer a line of text and a Read of a short file in the work folder, -ToolMs after its request
   came, and after the last result a short answer. Each call and its result join the conversation
   at its foot, which moves it up.
4. A prompt typed and its turn held open (pings only) for -HoldSec, Claude Code's spinner alone, then
   let go.
5. A probe tab whose pwsh writes DECRQM 2026, then two frames in DECSET and DECRST 2026, one in a
   single write and one in two: whether ConPTY passes them through to the reader, whatever Claude
   Code does with them.

Each phase's start goes to `ptyrec\marks.tsv` (nanoseconds since the Unix epoch, a tab, its name),
the stand-in's own times for the stream's first and last deltas, the tool turn's first request
and its last, and the held turn's request. Over
each phase, the pane's conpty host's and Claude Code's I/O counters (the conpty host's writes are its
pipe writes, one a console call it passes on, and its "other" I/O its console driver messages, one a
console call made of it) and the PC's load go to cost.json, with the screens in `captures` and the
API's requests.

It fails when nothing was recorded, when Claude Code's pane recorded under five reads in the
streaming, tool or spinner phases, or when the probe's pane recorded nothing. What the recording says, frame by frame,
is lbterm/bench/pty-frames.ps1's: run it on the output folder's `ptyrec` (it runs
mux/tests/lbterm_pty_frames.rs, through cargo, so in a warden job of its own, not this GPU one).

  pwsh -File lbterm\perf\claude-output.ps1 [-Bin <build>] [-Lines 160] [-DeltaMs 30] [-Tools 10] [-ToolMs 400] [-HoldSec 8]
#>
param(
  [string]$Bin,
  # The long answer's lines
  [int]$Lines = 160,
  # A text delta every -DeltaMs (Thread.Sleep, so on Windows' 15.6 ms tick: 30 is about 31)
  [int]$DeltaMs = 30,
  [int]$WordsPerDelta = 4,
  # The tool turn's calls, each answered -ToolMs after its request came
  [int]$Tools = 10,
  [int]$ToolMs = 400,
  [int]$HoldSec = 8,
  [int]$Width = 1080,
  [int]$Height = 1920
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $PSScriptRoot '..\budgets.psm1') -Force -DisableNameChecking
$T = Start-Test 'claude-output' -Bin $Bin

# The stand-in API: every request written down; /v1/messages answered as a stream, by Mode: "long",
# a long answer streamed a few words at a time; "tools", a line of text and a Read of the next part
# until -Tools results have come back, then a short answer; "hold", held open (pings each second)
# until Release. Its times are nanoseconds since the Unix epoch, the recording's clock.
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System; using System.IO; using System.Net; using System.Net.Sockets; using System.Text; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public sealed class LbtermOutputApi {
  TcpListener listener; public int Port;
  public readonly List<string> Log = new List<string>();
  public readonly ManualResetEvent Release = new ManualResetEvent(false);
  public int Turns = 0, Held = 0, Answered = 0, Deltas = 0;
  public long StreamStartNs = 0, StreamEndNs = 0, HeldNs = 0;
  public int Lines = 160, DeltaMs = 30, WordsPerDelta = 4;
  public volatile string Mode = "long";
  public string WorkDir = "";
  public int Tools = 10, ToolMs = 400, ToolCalls = 0, ToolsDone = 0;
  public long ToolsStartNs = 0, ToolsEndNs = 0;
  public static long UnixNs() { return (DateTime.UtcNow.Ticks - 621355968000000000L) * 100; }
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
  static void Delta(Stream s, string t) { Ev(s, "content_block_delta", "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":" + Quote(t) + "}}"); }
  static void Begin(Stream s) { Ev(s, "content_block_start", "{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}"); }
  static void Finish(Stream s) {
    Ev(s, "content_block_stop", "{\"type\":\"content_block_stop\",\"index\":0}");
    Ev(s, "message_delta", "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":40}}");
    Ev(s, "message_stop", "{\"type\":\"message_stop\"}");
  }
  // The long answer: markdown as a long answer has it, sections of a heading, a paragraph that
  // wraps, a list and now and then a code block
  public string Answer() {
    var sb = new StringBuilder();
    string[] words = { "the", "pane", "output", "flow", "reads", "each", "write", "from", "its", "console", "and", "parses", "it", "into", "actions", "that", "change", "lines", "of", "terminal", "model", "before", "server", "pushes", "them", "to", "window", "which", "draws", "frame" };
    int n = 0, w = 0;
    Func<int, string> sentence = k => { var p = new List<string>(); for (int i = 0; i < k; i++) p.Add(words[(w++ * 7 + i) % words.Length]); var t = string.Join(" ", p); return char.ToUpper(t[0]) + t.Substring(1) + "."; };
    while (n < Lines) {
      int section = n / 20 + 1;
      sb.Append("## Part " + section + ": how the " + words[section % words.Length] + " works\n\n"); n += 2;
      sb.Append(sentence(14) + " " + sentence(12) + " " + sentence(16) + " " + sentence(10) + "\n\n"); n += 4;
      for (int i = 1; i <= 6 && n < Lines; i++) { sb.Append("- **Step " + i + "**: " + sentence(9) + " See `step_" + i + "()` in `mux/src/lib.rs`.\n"); n++; }
      sb.Append("\n"); n++;
      if (section % 2 == 0) {
        sb.Append("```rust\n"); n++;
        for (int i = 0; i < 6 && n < Lines; i++) { sb.Append("    let step_" + i + " = pane.read(&mut buf[" + i + "..])?; // " + sentence(5) + "\n"); n++; }
        sb.Append("```\n\n"); n += 2;
      }
      sb.Append(sentence(18) + " " + sentence(11) + "\n\n"); n += 3;
    }
    sb.Append("That is the whole of the long answer.\n");
    return sb.ToString();
  }
  void Messages(Stream s, string body) {
    var model = Regex.Match(body, "\"model\":\"([^\"]+)\"").Groups[1].Value;
    if (!body.Contains("\"stream\":true")) {
      Json(s, "{\"id\":\"msg_x\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"" + model + "\",\"content\":[{\"type\":\"text\",\"text\":\"Output check\"}],\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":4}}");
      return;
    }
    var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
    s.Write(h, 0, h.Length);
    Ev(s, "message_start", "{\"type\":\"message_start\",\"message\":{\"id\":\"msg_" + Guid.NewGuid().ToString("N") + "\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"" + model + "\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":120,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":1}}}");
    // A turn's request offers the tools; a title's or a summary's doesn't
    if (!body.Contains("\"tools\":[{")) { Begin(s); Delta(s, "Output check"); Finish(s); return; }
    Interlocked.Increment(ref Turns);
    if (Mode == "tools") {
      // This turn's results so far: the request holds the turn's whole history
      int results = Regex.Matches(body, "\"type\":\"tool_result\"").Count;
      if (results == 0 && ToolsStartNs == 0) ToolsStartNs = UnixNs();
      if (results >= Tools && ToolsEndNs == 0) ToolsEndNs = UnixNs();
      Thread.Sleep(ToolMs);
      if (results < Tools) {
        int n = results + 1;
        var path = (WorkDir + "/part-" + n + ".txt").Replace("\\", "/");
        Begin(s); Delta(s, "Reading part " + n + " of " + Tools + ".");
        Ev(s, "content_block_stop", "{\"type\":\"content_block_stop\",\"index\":0}");
        Ev(s, "content_block_start", "{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_" + n.ToString("D2") + Guid.NewGuid().ToString("N").Substring(0, 20) + "\",\"name\":\"Read\",\"input\":{}}}");
        Ev(s, "content_block_delta", "{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":" + Quote("{\"file_path\":\"" + path + "\"}") + "}}");
        Ev(s, "content_block_stop", "{\"type\":\"content_block_stop\",\"index\":1}");
        Ev(s, "message_delta", "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":30}}");
        Ev(s, "message_stop", "{\"type\":\"message_stop\"}");
        Interlocked.Increment(ref ToolCalls);
        return;
      }
      var sb = new StringBuilder("All " + Tools + " parts read. What each says:\n\n");
      for (int i = 1; i <= Tools; i++) sb.Append("- Part " + i + " says where step " + i + " of the flow runs and what it costs.\n");
      sb.Append("\nThat is all of the parts.\n");
      Begin(s); Delta(s, sb.ToString()); Finish(s);
      Interlocked.Increment(ref ToolsDone);
      return;
    }
    if (Mode == "long") {
      Begin(s);
      var pieces = Regex.Matches(Answer(), "\\S+\\s*");
      var chunk = new StringBuilder();
      StreamStartNs = UnixNs();
      for (int i = 0; i < pieces.Count; i++) {
        chunk.Append(pieces[i].Value);
        if ((i + 1) % WordsPerDelta == 0 || i == pieces.Count - 1) {
          Delta(s, chunk.ToString()); chunk.Clear(); Interlocked.Increment(ref Deltas);
          Thread.Sleep(DeltaMs);
        }
      }
      StreamEndNs = UnixNs();
      Finish(s); Interlocked.Increment(ref Answered); return;
    }
    HeldNs = UnixNs();
    Interlocked.Increment(ref Held);
    Begin(s); Delta(s, "");
    while (!Release.WaitOne(1000)) Ev(s, "ping", "{\"type\":\"ping\"}");
    Delta(s, "The spinner turned.");
    Finish(s);
  }
}
// A process's I/O counters and cycles: reads, writes, others, their bytes, cycles
public static class LbtermOutputIo {
  [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern bool GetProcessIoCounters(IntPtr h, out IO c);
  [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)] public struct IO { public ulong Reads, Writes, Others, ReadBytes, WriteBytes, OtherBytes; }
  public static ulong[] Sample(int pid) {
    var r = new ulong[7];
    IntPtr h = OpenProcess(0x1000, false, (uint)pid);
    if (h == IntPtr.Zero) return null;
    try {
      IO io; if (GetProcessIoCounters(h, out io)) { r[0] = io.Reads; r[1] = io.Writes; r[2] = io.Others; r[3] = io.ReadBytes; r[4] = io.WriteBytes; r[5] = io.OtherBytes; }
      ulong c; if (QueryProcessCycleTime(h, out c)) r[6] = c;
      return r;
    } finally { CloseHandle(h); }
  }
}
'@

$claude = (Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $claude) { throw 'claude-output needs Claude Code installed: no claude on PATH' }
$api = [LbtermOutputApi]::new()
$api.Lines = $Lines; $api.DeltaMs = $DeltaMs; $api.WordsPerDelta = $WordsPerDelta; $api.Tools = $Tools; $api.ToolMs = $ToolMs
$api.Start()

# Short folders: the transcript's path, under a folder named after the work folder's, must stay
# under Windows' 260 characters
$configDir = Join-Path $T.Out 'c'
$work = Join-Path $T.Out 'w'
$summaries = Join-Path $T.Out 'summaries'
$captures = Join-Path $T.Out 'captures'
$rec = Join-Path $T.Out 'ptyrec'
New-Item -ItemType Directory -Force $configDir, $work, $summaries, $captures, $rec | Out-Null
$api.WorkDir = $work
# The parts the tool turn reads, a dozen lines each
for ($i = 1; $i -le $Tools; $i++) {
  [IO.File]::WriteAllLines((Join-Path $work "part-$i.txt"), [string[]](1..12 | ForEach-Object { "Part $i, line ${_}: what step $i of the pane-output flow does, and on which thread." }))
}
$key = 'sk-ant-api03-' + (-join ((1..40) | ForEach-Object { 'abcdefghijklmnopqrstuvwxyz0123456789'[(Get-Random -Maximum 36)] }))
$version = ((& $claude --version) -split ' ')[0]
"Claude Code $version, the stand-in API on 127.0.0.1:$($api.Port)"
# Read allowed, so the tool turn asks nothing
@{ tui = 'fullscreen'; permissions = @{ allow = @('Read') } } | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8NoBOM (Join-Path $configDir 'settings.json')
[ordered]@{
  hasCompletedOnboarding = $true; lastOnboardingVersion = $version; lastReleaseNotesSeen = $version; theme = 'dark'
  customApiKeyResponses = @{ approved = @($key.Substring($key.Length - 20)); rejected = @() }
  projects = @{ ($work -replace '\\', '/') = @{ hasTrustDialogAccepted = $true; allowedTools = @() } }
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8NoBOM (Join-Path $configDir '.claude.json')

# Named, as every one of Lachlan's conversations is
$sid = [guid]::NewGuid().ToString()
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sid))
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
& '$claude' --session-id $sid --model claude-haiku-4-5 --name 'Output Check'
"@
$program = New-PwshProgram $launch
$cfg = Join-Path $T.Out 'config\claude-output.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
# The domain's first tab runs Claude Code: its server takes default_prog from the same file
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 40
config.window_close_confirmation = 'NeverPrompt'
config.default_prog = { $(($program | ForEach-Object { "'$_'" }) -join ', ') }
config.default_cwd = [[$work]]
rawset(config, 'lbterm_summaries', '$(ConvertTo-LuaPath $summaries)')
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath (Join-Path $configDir 'projects'))' }
config.unix_domains = { {
  name = 'claudeout',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

$marks = Join-Path $rec 'marks.tsv'
$counters = [ordered]@{}
$conpty = $null; $claudePid = $null
function Get-Counters {
  [ordered]@{
    conpty = if ($script:conpty) { [LbtermOutputIo]::Sample($script:conpty) }
    claude = if ($script:claudePid) { [LbtermOutputIo]::Sample($script:claudePid) }
  }
}
# A phase starts now, or at -At (nanoseconds since the Unix epoch, the stand-in's)
function Add-Mark([string]$Name, [long]$At = 0) {
  $ns = if ($At) { $At } else { [LbtermOutputApi]::UnixNs() }
  Add-Content -LiteralPath $marks "$ns`t$Name"
  # The counters are read now, whatever the phase's own time
  $script:counters[$Name] = [ordered]@{ ns = $ns; taken = [LbtermOutputApi]::UnixNs(); counters = (Get-Counters) }
}
function Screen-Text { (Invoke-Cli get-text --pane-id $script:pane -Server) -join "`n" }
function Save-Screen([string]$name) {
  [IO.File]::WriteAllText((Join-Path $captures "$name.txt"), ((Invoke-Cli get-text --pane-id $script:pane --escapes -Server) -join "`n"))
  $plain = Screen-Text
  [IO.File]::WriteAllText((Join-Path $captures "$name.plain.txt"), $plain)
  $plain
}
function Type-Line([string]$words) {
  Invoke-Cli send-text --pane-id $script:pane --no-paste $words -Server | Out-Null
  Start-Sleep -Milliseconds 300
  Invoke-Cli send-text --pane-id $script:pane --no-paste "`r" -Server | Out-Null
}
# A recording's reads (mux/src/lbterm_ptyrec.rs's format, little-endian: the header, then records
# of a kind byte, ns since the start, and a read's length and bytes or a size's rows and columns):
# each read's time in ns since the Unix epoch and its length. Read through a handle shared for
# writing, while the server still writes it; a record cut short ends it.
function Read-PtyReads([Parameter(Mandatory)][string]$Path) {
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
  try { $ms = [IO.MemoryStream]::new(); $fs.CopyTo($ms); $b = $ms.ToArray() } finally { $fs.Dispose() }
  $reads = [Collections.Generic.List[object]]::new()
  if ($b.Length -lt 28 -or [Text.Encoding]::ASCII.GetString($b, 0, 8) -ne 'LBPTYRC1') { return [pscustomobject]@{ Start = 0; Reads = $reads; Size = $b.Length } }
  $start = [BitConverter]::ToUInt64($b, 8)
  $at = 28
  while ($at + 9 -le $b.Length) {
    $kind = $b[$at]; $ns = [BitConverter]::ToUInt64($b, $at + 1); $body = $at + 9
    if ($kind -eq 1) {
      if ($body + 4 -gt $b.Length) { break }
      $len = [BitConverter]::ToUInt32($b, $body)
      if ($body + 4 + $len -gt $b.Length) { break }
      $reads.Add([pscustomobject]@{ Ns = $start + $ns; Bytes = $len })
      $at = $body + 4 + $len
    } elseif ($kind -eq 2) { $at = $body + 8 } else { break }
  }
  [pscustomobject]@{ Start = $start; Reads = $reads; Size = $b.Length }
}
$cost = [ordered]@{ claude = $version; lines = $Lines; delta_ms = $DeltaMs; words_per_delta = $WordsPerDelta; tools = $Tools; tool_ms = $ToolMs; hold_sec = $HoldSec }

Invoke-Test {
  $script:win = Open-TestWindow -ConfigFile $cfg -Domain 'claudeout' -Socket $T.Sock -WorkingDirectory $work -TimeoutSec 30 -Environment @{ LBTERM_PTY_RECORD = $rec }
  if (-not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 20)) { throw "the test's mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window' -TimeoutSec 30 | Out-Null
  $script:pane = (Get-Panes -Server)[0].pane_id
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $sp = [int]$server[0].ProcessId
  $cost.server_pid = $sp

  # As tall as Lachlan's window: its client area -Width by -Height, the frame round it kept
  $outer = Get-WindowRect $script:win; $client = Get-ClientRect $script:win
  Set-WindowSize $script:win ($Width + ($outer.Right - $outer.Left) - $client.Width) ($Height + ($outer.Bottom - $outer.Top) - $client.Height)
  Wait-State { param($s) $s.size.width -eq $Width -and $s.size.height -eq $Height } -What "the window at $Width by $Height" -TimeoutSec 15 | Out-Null

  $ready = Wait-Until {
    $t = Screen-Text
    if ($t -match 'Allow external CLAUDE.md file imports' -and $t -match '(?m)^\s*❯ No, disable external imports') { Invoke-Cli send-text --pane-id $script:pane --no-paste "`r" -Server | Out-Null; Start-Sleep 1; return $false }
    ([regex]::Matches($t, '(?m)^─{20,}')).Count -ge 2 -and $t -notmatch 'Enter to confirm'
  } -TimeoutSec 40 -IntervalMs 500
  Save-Screen 'start' | Out-Null
  if (-not $ready) { throw "Claude Code never drew its input box: captures\start.plain.txt" }
  $paneInfo = @(Get-Panes -Server | Where-Object pane_id -eq $script:pane)[0]
  $cost.pane = [ordered]@{ id = $script:pane; rows = $paneInfo.size.rows; cols = $paneInfo.size.cols }
  "Claude Code's pane $($script:pane): $($paneInfo.size.cols) by $($paneInfo.size.rows)"

  # Its conpty host and Claude Code's process: the server's one OpenConsole, and the claude under
  # the pane's pwsh
  $script:conpty = @(Get-ConptyIds -Parents $sp)[0]
  $shells = @(Get-CimInstance Win32_Process -Filter "Name = 'pwsh.exe'" | Where-Object { [int]$_.ParentProcessId -eq $sp } | ForEach-Object { [int]$_.ProcessId })
  $script:claudePid = @(Get-CimInstance Win32_Process -Filter "Name = 'claude.exe'" | Where-Object { [int]$_.ParentProcessId -in $shells } | ForEach-Object { [int]$_.ProcessId })[0]
  $cost.conpty_pid = $script:conpty; $cost.claude_pid = $script:claudePid
  $cost.conpty_exe = (Get-CimInstance Win32_Process -Filter "ProcessId = $($script:conpty)").ExecutablePath
  if (-not $script:conpty -or -not $script:claudePid) { Add-Failure "couldn't find the pane's conpty host ($($script:conpty)) or Claude Code's process ($($script:claudePid)): their counters are missing" }

  Start-Sleep 1
  $rate = Get-CycleRate
  $pcStart = Start-PcWatch -ProcessId $script:win.Pid -Roots @($script:win.Pid, $sp)
  $cost.locked_at_start = Test-WorkstationLocked
  Add-Mark 'ready'

  # 2. The long answer streamed
  Add-Mark 'typing'
  Type-Line 'Write a long answer please'
  $expected = [Math]::Ceiling($Lines * 12 / $WordsPerDelta) * ($DeltaMs + 16) / 1000
  if (-not (Wait-Until { $api.StreamStartNs -gt 0 } -TimeoutSec 30 -IntervalMs 100)) { Save-Screen 'stuck' | Out-Null; throw "Claude Code never asked for the first turn: $($api.Log -join ' | ')" }
  Add-Mark 'streaming' $api.StreamStartNs
  Start-Sleep -Milliseconds ([int]([Math]::Min(4, $expected / 2) * 1000))
  Save-Screen 'streaming' | Out-Null
  if (-not (Wait-Until { $api.Answered -ge 1 } -TimeoutSec ([Math]::Max(60, 4 * $expected)) -IntervalMs 200)) { Save-Screen 'stuck' | Out-Null; throw "the long answer never finished streaming: $($api.Deltas) deltas sent" }
  Add-Mark 'settling' $api.StreamEndNs
  $cost.deltas = $api.Deltas
  $cost.stream_seconds = [Math]::Round(($api.StreamEndNs - $api.StreamStartNs) / 1e9, 3)
  if (-not (Wait-Until { (Screen-Text) -match 'whole of the long answer' } -TimeoutSec 15 -IntervalMs 250)) { Add-Failure "the long answer's last line never showed: captures\answered.plain.txt" }
  Start-Sleep 2
  Save-Screen 'answered' | Out-Null

  # 3. A turn of tool calls, each call and its result joining the conversation at its foot
  $api.Mode = 'tools'
  Add-Mark 'typing-tools'
  Type-Line 'Read the parts please'
  if (-not (Wait-Until { $api.ToolsStartNs -gt 0 } -TimeoutSec 30 -IntervalMs 100)) { Save-Screen 'stuck' | Out-Null; throw "Claude Code never asked for the tool turn: $($api.Log -join ' | ')" }
  Add-Mark 'tools' $api.ToolsStartNs
  if (-not (Wait-Until { $api.ToolsDone -ge 1 } -TimeoutSec ([Math]::Max(60, $Tools * ($ToolMs / 1000 + 3))) -IntervalMs 200)) {
    Save-Screen 'stuck' | Out-Null
    throw "the tool turn never finished: $($api.ToolCalls) calls answered (captures\stuck.plain.txt; a permission prompt?)"
  }
  Add-Mark 'tools-answered' $api.ToolsEndNs
  $cost.tool_calls = $api.ToolCalls
  $cost.tools_seconds = [Math]::Round(($api.ToolsEndNs - $api.ToolsStartNs) / 1e9, 3)
  if (-not (Wait-Until { (Screen-Text) -match 'That is all of the parts' } -TimeoutSec 15 -IntervalMs 250)) { Add-Failure "the tool turn's answer never showed: captures\tools.plain.txt" }
  Start-Sleep 2
  Save-Screen 'tools' | Out-Null

  # 4. A turn held, the spinner alone
  $api.Mode = 'hold'
  Add-Mark 'typing-hold'
  Type-Line 'Check the spinner please'
  if (-not (Wait-Until { $api.Held -ge 1 } -TimeoutSec 30 -IntervalMs 100)) { throw "Claude Code never asked for the second turn: $($api.Log -join ' | ')" }
  Add-Mark 'spinner' $api.HeldNs
  Start-Sleep -Milliseconds 1500
  Save-Screen 'spinner' | Out-Null
  Start-Sleep -Milliseconds ([Math]::Max(0, $HoldSec * 1000 - 1500))
  Add-Mark 'released'
  $api.Release.Set() | Out-Null
  if (-not (Wait-Until { (Screen-Text) -match 'The spinner turned' } -TimeoutSec 15 -IntervalMs 200)) { Add-Failure "the held turn's answer never showed: captures\done.plain.txt" }
  Start-Sleep 2
  Save-Screen 'done' | Out-Null

  # 5. The probe: DECRQM 2026, a frame in one write, a frame in two
  Add-Mark 'probe'
  $probe = '$e = [char]27; [Console]::Out.Write("$e[?2026`$p"); [Console]::Out.Flush(); Start-Sleep -Milliseconds 300; [Console]::Out.Write("$e[?2026hprobe frame one$e[?2026l`r`n"); [Console]::Out.Flush(); Start-Sleep -Milliseconds 300; [Console]::Out.Write("$e[?2026hprobe frame two, "); [Console]::Out.Flush(); Start-Sleep -Milliseconds 50; [Console]::Out.Write("its second write$e[?2026l`r`n"); [Console]::Out.Flush(); "probe done"; Start-Sleep 30'
  $probePane = New-Tab -From $script:pane -NoActivate -Program (New-PwshProgram $probe) -Server
  $cost.probe_pane = $probePane
  $probed = Wait-Until { (Invoke-Cli get-text --pane-id $probePane -Server) -join "`n" -match 'probe done' } -TimeoutSec 20 -IntervalMs 200
  if (-not $probed) { Add-Failure "the probe tab never said it was done" }
  Add-Mark 'end'
  $pcStop = Stop-PcWatch $pcStart -Roots @($script:win.Pid, $sp)
  $cost.locked_at_end = Test-WorkstationLocked
  $cost.pc = ConvertTo-PcSpan -Start $pcStart -Stop $pcStop -CycleRate $rate

  # Each phase's counters: from its mark to the next
  $names = @($counters.Keys)
  $phases = [ordered]@{}
  for ($i = 0; $i -lt $names.Count - 1; $i++) {
    $a = $counters[$names[$i]]; $b = $counters[$names[$i + 1]]
    $p = [ordered]@{ seconds = [Math]::Round(($b.ns - $a.ns) / 1e9, 3); counted_seconds = [Math]::Round(($b.taken - $a.taken) / 1e9, 3) }
    foreach ($who in 'conpty', 'claude') {
      $x = $a.counters[$who]; $y = $b.counters[$who]
      if ($x -and $y) {
        $p[$who] = [ordered]@{
          reads = $y[0] - $x[0]; writes = $y[1] - $x[1]; others = $y[2] - $x[2]
          read_bytes = $y[3] - $x[3]; write_bytes = $y[4] - $x[4]; other_bytes = $y[5] - $x[5]
          cpu_ms = [Math]::Round(($y[6] - $x[6]) / $rate, 2)
        }
      }
    }
    $phases[$names[$i]] = $p
  }
  $cost.phases = $phases
  [IO.File]::WriteAllLines((Join-Path $captures 'api.log'), [string[]]$api.Log)
  $cost | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'cost.json')

  # What was recorded, read through a handle: the server still has the files open, and a folder's
  # listing says a file's size as of its last close (0 here) (PITFALLS 126)
  $files = @(Get-ChildItem -LiteralPath $rec -Filter '*.ptyrec')
  $read = @{}
  foreach ($f in $files) { $read[$f.Name] = Read-PtyReads $f.FullName }
  "recorded: $(@($files | ForEach-Object { "$($_.Name) $($read[$_.Name].Size) B, $($read[$_.Name].Reads.Count) reads" }) -join '; ')"
  if (-not $files) { Add-Failure "nothing was recorded in ${rec}: LBTERM_PTY_RECORD didn't reach the test's mux server, or its readers didn't record" }
  $mine = $read["pane-$sp-$($script:pane).ptyrec"]
  if (-not $mine) { Add-Failure "no recording of Claude Code's pane ($($script:pane)) from the server ($sp)" }
  else {
    # Its reads while the answer streamed, while the tools ran and while only the spinner went
    foreach ($span in @(@('streaming', 'settling'), @('tools', 'tools-answered'), @('spinner', 'released'))) {
      $from = $counters[$span[0]].ns; $to = $counters[$span[1]].ns
      $n = @($mine.Reads | Where-Object { $_.Ns -ge $from -and $_.Ns -lt $to }).Count
      "Claude Code's pane, $($span[0]): $n reads recorded"
      if ($n -lt 5) { Add-Failure "Claude Code's pane recorded $n reads in its $($span[0]) phase: too few to have seen it" }
    }
  }
  $probeRec = $read["pane-$sp-$probePane.ptyrec"]
  if (-not $probeRec -or -not $probeRec.Reads.Count) { Add-Failure "no reads recorded of the probe's pane ($probePane)" }
  foreach ($k in 'streaming', 'tools', 'spinner') {
    $c = $phases[$k].conpty
    if ($c) { "$k`: the conpty host wrote $($c.writes) times ($($c.write_bytes) B) and took $($c.others) console messages in $($phases[$k].counted_seconds) s" }
  }
  "the PC: $($cost.pc.others) of the job's CPUs taken by others, $($cost.pc.pc) of the whole PC busy; locked: $($cost.locked_at_start) at the start, $($cost.locked_at_end) at the end"
  "analyse it: warden-run --name pty-frames --cpus 4 --mem 16 --max 1h -- pwsh -NoProfile -File $(Join-Path $PSScriptRoot '..\bench\pty-frames.ps1') -Recording $rec"
} -Passed "claude-output: real Claude Code's pty reads recorded through a long answer streamed, a turn of $Tools tool calls, a turn held with its spinner, and a probe of synchronized output, in ptyrec"
