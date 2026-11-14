#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/context_bar.rs, wezterm-gui/src/context_edits.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/termwindow/control.rs, lbterm-extras/src/context_edits.rs, lbterm-extras/src/conversation.rs, lbterm-extras/src/context.rs, lbterm-extras/src/context_bar.rs, lbterm-extras/src/drafts.rs
<#
The context bar's Remove and Compact now against real Claude Code (lbterm/CONTEXT.md, "The
buttons"), where perf\context-edits.ps1 has a stand-in draw Claude Code's screen to the code's own
idea of it. The Claude Code installed here (`claude` on PATH) runs in a test window with a config
folder of its own (fullscreen, onboarding done, a made-up API key) and talks to a stand-in for
Anthropic's API on a loopback port (ANTHROPIC_BASE_URL), as perf\claude-working.ps1 runs it: no
account is used and nothing leaves the PC. The stand-in answers every request at once, its usage
the request's characters over four, so the bar has something in use. Its work folder has a
CLAUDE.md of the test's own importing a rule beside it with an `@` on a line with words round it.

1. One turn answered: the bar has the CLAUDE.md and the rule it imports, the rule with Remove.
2. Remove: only the import's `@` comes off the test's CLAUDE.md, the line's words and the path
   staying as plain text, which Claude Code doesn't import (step 4 holds it to that: the rule isn't
   loaded again with the turn after the compaction); the rule's section stays until a compaction,
   its row saying `Import off in CLAUDE.md: a compaction can attach it again`.
3. Compact now, lit at Claude Code's empty prompt (no placeholder there taken for words typed):
   a press types /compact and Enter at once and writes nothing else (Lachlan's 1b of 2026-11-10:
   no rule anywhere); Claude Code compacts (its summary request to the stand-in, held while the
   test looks, a compact_boundary record in the transcript), and the bar follows: the rule's
   import section is gone. How long the request took to come after the press is said.
4. What Claude Code attached after the summary: the rule, whole, as every compaction does while
   it's among the five files it restores (lbterm/CONTEXT.md, "What Claude Code restores after a
   compaction"), which the bar shows as a section of its own, `attached`, hatched, its row the
   import's Undo saying `Attached again after the summary, @ off in CLAUDE.md`; and the slot's
   small CLAUDE.md, `attached` too. One more turn: the CLAUDE.md files loaded again, the rule not
   imported among them, its attached section still there.
   No settings file was written: every `settings*.json` under the test's config folder
   (CLAUDE_CONFIG_DIR) and its work folder is as it was before the press, byte for byte, while it
   compacts, after it and after the turn, and the config folder's `settings.json` is still the
   test's own.
5. Claude Code's own /context, typed at its prompt: the files it lists against the bar's, by path
   (each file /context counts must be a section of the bar), and its total and categories beside
   the bar's in use and sections, said rather than judged, since against the stand-in both count
   characters over four (lbterm/CONTEXT.md, "Measured, against Claude Code's own /context").

Claude Code reads the CLAUDE.md files above its folder, inside this tree, as claude-working's
does: the test presses only the row whose path is its own rule, and fails before pressing anything
else. Every screen is kept in the output folder's `captures`, with the API's requests.

  pwsh -File lbterm\perf\claude-compact.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'claude-compact' -Bin $Bin

# The stand-in API: every request written down, each answered at once, its usage its body's
# characters over four
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System; using System.IO; using System.Net; using System.Net.Sockets; using System.Text; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public sealed class LbtermCompactApi {
  TcpListener listener; public int Port;
  public readonly List<string> Log = new List<string>();
  public int Turns = 0; public int Summaries = 0;
  // HoldNext: the next streamed request (the compaction's, once Compact now is pressed) waits for
  // Release, so the test can look at Claude Code and the bar while it compacts
  public volatile bool HoldNext = false; public int Held = 0;
  public readonly ManualResetEventSlim Release = new ManualResetEventSlim(true);
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
          if (first.StartsWith("POST /v1/messages/count_tokens")) { Json(s, "{\"input_tokens\":" + Math.Max(1, text.Length / 4) + "}"); continue; }
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
  void Messages(Stream s, string body) {
    var model = Regex.Match(body, "\"model\":\"([^\"]+)\"").Groups[1].Value;
    int input = Math.Max(1, body.Length / 4);
    if (!body.Contains("\"stream\":true")) {
      Json(s, "{\"id\":\"msg_x\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"" + model + "\",\"content\":[{\"type\":\"text\",\"text\":\"Compact check\"}],\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":" + input + ",\"output_tokens\":4}}");
      return;
    }
    var tools = body.Contains("\"tools\":[{");
    // The compaction's request asks for a summary of the conversation so far
    var summary = body.Contains("summary") && !tools;
    if (HoldNext) { HoldNext = false; Release.Reset(); Interlocked.Increment(ref Held); Note("held"); Release.Wait(); Note("let go"); }
    if (tools) Interlocked.Increment(ref Turns);
    if (summary) Interlocked.Increment(ref Summaries);
    Note("messages: " + (tools ? "a turn" : summary ? "a summary" : "other") + ", " + input + " tokens in");
    var h = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
    s.Write(h, 0, h.Length);
    Ev(s, "message_start", "{\"type\":\"message_start\",\"message\":{\"id\":\"msg_" + Guid.NewGuid().ToString("N") + "\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"" + model + "\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":" + input + ",\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":1}}}");
    var words = summary ? "<summary>The user asked for a check of the context bar; it was answered.</summary>" : "The context bar check is answered.";
    Ev(s, "content_block_start", "{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}");
    Ev(s, "content_block_delta", "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":" + Quote(words) + "}}");
    Ev(s, "content_block_stop", "{\"type\":\"content_block_stop\",\"index\":0}");
    Ev(s, "message_delta", "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":40}}");
    Ev(s, "message_stop", "{\"type\":\"message_stop\"}");
  }
}
'@

$claude = (Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $claude) { throw 'claude-compact needs Claude Code installed: no claude on PATH' }
$api = [LbtermCompactApi]::new()
$api.Start()

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
$settingsText = [IO.File]::ReadAllText((Join-Path $configDir 'settings.json'))
[ordered]@{
  hasCompletedOnboarding = $true; lastOnboardingVersion = $version; lastReleaseNotesSeen = $version; theme = 'dark'
  customApiKeyResponses = @{ approved = @($key.Substring($key.Length - 20)); rejected = @() }
  projects = @{ ($work -replace '\\', '/') = @{ hasTrustDialogAccepted = $true; allowedTools = @() } }
} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8NoBOM (Join-Path $configDir '.claude.json')

# The work folder's own CLAUDE.md and the rule it imports
$md = Join-Path $work 'CLAUDE.md'
$ruleFile = Join-Path $work 'check-rule.md'
$mdText = "# Compact check`n`nWords about the check.`n`nRead @check-rule.md before the check.`n"
[IO.File]::WriteAllText($md, $mdText)
[IO.File]::WriteAllText($ruleFile, "A rule the check imports.`n" + ('r' * 2000) + "`n")

$sid = [guid]::NewGuid().ToString()
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($sid))
[IO.File]::WriteAllText((Join-Path $summaries "$sid.usage.json"), '{"usd":0.1,"window":200000}')
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
& '$claude' --session-id $sid --model claude-haiku-4-5 --name 'Compact Check'
"@
$lua = @"
config.initial_cols = 120
config.initial_rows = 40
rawset(config, 'lbterm_summaries', '$(ConvertTo-LuaPath $summaries)')
rawset(config, 'lbterm_tab_panel', { enabled = true, open_delay_ms = 0 })
rawset(config, 'lbterm_context', { transcripts = '$(ConvertTo-LuaPath (Join-Path $configDir 'projects'))' })
rawset(config, 'lbterm_context_bar', {})
"@

function Screen-Text { (Invoke-Cli get-text --pane-id $script:pane) -join "`n" }
function Save-Screen([string]$name) {
  [IO.File]::WriteAllText((Join-Path $captures "$name.txt"), ((Invoke-Cli get-text --pane-id $script:pane --escapes) -join "`n"))
  $plain = Screen-Text
  [IO.File]::WriteAllText((Join-Path $captures "$name.plain.txt"), $plain)
  $plain
}
function Read-Bar { (Get-StateWindow (Read-State) -Hwnd ([long]$script:win.Handle)).context_bar }
function Type-Line([string]$words) {
  Invoke-Cli send-text --pane-id $script:pane --no-paste $words | Out-Null
  Start-Sleep -Milliseconds 300
  Invoke-Cli send-text --pane-id $script:pane --no-paste "`r" | Out-Null
}
function Get-Transcript { Get-ChildItem -LiteralPath (Join-Path $configDir 'projects') -Recurse -Filter "$sid.jsonl" | Select-Object -First 1 }
$rulePath = ($ruleFile -replace '\\', '/').ToLowerInvariant()
function Test-Rule($section) { $section.path -and (($section.path -replace '\\', '/').ToLowerInvariant() -eq $rulePath) }
# Every Claude Code settings file under the test's config folder and its work folder, with a hash
# of each: Compact now writes none (lbterm/CONTEXT.md, "Compact now")
function Get-SettingsFiles {
  @(Get-ChildItem -LiteralPath $configDir, $work -Recurse -Force -File -Filter 'settings*.json' -ErrorAction SilentlyContinue | Sort-Object FullName |
    ForEach-Object { "$($_.FullName) $((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash)" }) -join "`n"
}
function Test-Settings([string]$when) {
  $now = Get-SettingsFiles
  $own = [IO.File]::ReadAllText((Join-Path $configDir 'settings.json'))
  if ($now -cne $script:settingsBefore) { Add-Failure "$when the settings files aren't as they were before Compact now: was`n$($script:settingsBefore)`nnow`n$now" }
  elseif ($own -cne $settingsText) { Add-Failure "$when the config folder's settings.json isn't the test's own: $own" }
  else { "$when no settings file written, settings.json the test's own byte for byte" }
}
$found = [ordered]@{ claude = $version }

Invoke-Test {
  $script:win = Open-TestWindow -PostedPointer -Lua $lua -Program (New-PwshProgram $launch) -WorkingDirectory $work -TimeoutSec 30
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window' | Out-Null
  $script:pane = (Get-Panes)[0].pane_id
  $ready = Wait-Until {
    $t = Screen-Text
    if ($t -match 'Allow external CLAUDE.md file imports' -and $t -match '(?m)^\s*❯ No, disable external imports') { Invoke-Cli send-text --pane-id $script:pane --no-paste "`r" | Out-Null; Start-Sleep 1; return $false }
    ([regex]::Matches($t, '(?m)^─{20,}')).Count -ge 2 -and $t -notmatch 'Enter to confirm'
  } -TimeoutSec 40 -IntervalMs 500
  Save-Screen 'start' | Out-Null
  if (-not $ready) { throw "Claude Code never drew its input box: captures\start.plain.txt" }

  # 1. One turn: the bar has the rule, with Remove
  Type-Line 'Check the context bar please'
  if (-not (Wait-Until { $api.Turns -ge 1 -and (Screen-Text) -match 'context bar check is answered' } -TimeoutSec 30 -IntervalMs 250)) { Save-Screen 'stuck' | Out-Null; throw "the turn never came back: $($api.Log -join ' | ')" }
  $bar = $null
  $ok = Wait-Until { $b = Read-Bar; $script:bar = $b; $b -and $b.in_use -and @($b.sections | Where-Object { Test-Rule $_ }).Count -eq 1 } -TimeoutSec 20 -IntervalMs 250
  $bar = $script:bar
  $found.sections = @($bar.sections | ForEach-Object { "$($_.kind) $($_.tokens) $($_.path)" })
  "the bar: in use $($bar.in_use) of $($bar.window), scale $($bar.scale); $(@($bar.sections | ForEach-Object { "$($_.kind) $($_.tokens)" }) -join ', ')"
  if (-not $ok) { throw "the bar never showed the rule check-rule.md: waiting '$($bar.waiting)', sections $($found.sections -join '; ')" }
  $ruleSection = @($bar.sections | Where-Object { Test-Rule $_ })[0]
  if ($ruleSection.kind -ne 'import' -or $ruleSection.button -ne 'remove') { Add-Failure "the rule is a '$($ruleSection.kind)' section with button '$($ruleSection.button)', not an import with Remove" }
  Save-Screen 'answered' | Out-Null

  # 2. Remove, on the rule's row and nothing else
  $c = Get-RectCenter $bar.rect
  Send-Mouse $script:win Move $c[0] $c[1]
  if (-not (Wait-Until { (Read-Bar).pane.compact } -TimeoutSec 10 -IntervalMs 100)) { throw 'the pane never opened' }
  $b = Read-Bar
  $i = [array]::IndexOf(@($b.sections | ForEach-Object { Test-Rule $_ }), $true)
  $row = @($b.pane.rows | Where-Object section -eq $i)[0]
  if (-not $row -or -not (Test-Rule $b.sections[$row.section]) -or $row.button -ne 'Remove') { throw "the rule's row isn't there with Remove: $($b.pane.rows | ConvertTo-Json -Compress)" }
  Send-Click $script:win ($row.rect.x + $row.rect.w - 30) ((Get-RectCenter $row.rect)[1]) -Move
  if (-not (Wait-Until { (Read-Bar).sections[$i].removed } -TimeoutSec 10 -IntervalMs 100)) { Add-Failure "Remove didn't take the rule's @ off: $((Read-Bar).pane.rows[$i].note)" }
  $note = @((Read-Bar).pane.rows | Where-Object section -eq $i)[0].note
  $found.removed_note = $note
  if ($note -cne 'Import off in CLAUDE.md: a compaction can attach it again') { Add-Failure "the removed rule's row says '$note'" } else { "the removed rule's row: '$note'" }
  $now = [IO.File]::ReadAllText($md)
  if ($now -ne $mdText.Replace("@check-rule.md", 'check-rule.md')) { Add-Failure "the CLAUDE.md after Remove is '$now'" } else { "Remove: only the @ off the import in the test's CLAUDE.md, its words and the path kept" }

  # 3. Compact now at the empty prompt
  $b = Read-Bar
  $found.compact = $b.pane.compact
  "Compact now: enabled $($b.pane.compact.enabled), '$($b.pane.compact.why)'"
  Save-Screen 'before-compact' | Out-Null
  if (-not $b.pane.compact.enabled) { throw "Compact now isn't lit at Claude Code's prompt: '$($b.pane.compact.why)' (captures\before-compact.plain.txt)" }
  $at = Get-RectCenter $b.pane.compact.rect
  $script:settingsBefore = Get-SettingsFiles
  $found.settings = @($script:settingsBefore -split "`n" | ForEach-Object { Split-Path -Leaf ($_ -replace ' [0-9A-F]{64}$', '') })
  $api.HoldNext = $true
  $pressed = [Diagnostics.Stopwatch]::StartNew()
  Send-Click $script:win $at[0] $at[1] -Move
  # While it compacts, its request held: Claude Code's working line over the box (what the first
  # round's Compact now read; Terminal's PreCompact hook sets claude_status for it since, and no
  # hook runs in a test window), and the pane closed and opened again keeps Compact now dim, its
  # sent kept by pane till the compaction (lbterm/PITFALLS.md, 230)
  if (Wait-Until { $api.Held -ge 1 } -TimeoutSec 30 -IntervalMs 100) {
    # Typed at once: how long the compaction's request took to come, said (the two keys are
    # written 150 ms apart, then Claude Code starts its compaction)
    $found.press_to_request_ms = $pressed.ElapsedMilliseconds
    "compacting: its request came $($pressed.ElapsedMilliseconds) ms after the press"
    Test-Settings 'compacting:'
    $spin = Wait-Until { (Screen-Text) -match 'esc to interrupt' } -TimeoutSec 10 -IntervalMs 200
    Save-Screen 'compacting' | Out-Null
    if (-not $spin) { Add-Failure "Claude Code's screen while it compacts says no 'esc to interrupt' (captures\compacting.plain.txt): Compact now's look at its working line has nothing to see" } else { 'compacting: its working line over the box says esc to interrupt' }
    Send-Mouse $script:win Move 120 200
    if (-not (Wait-Until { -not (Read-Bar).pane } -TimeoutSec 10 -IntervalMs 100)) { Add-Failure 'the pane never closed while Claude Code compacted' }
    Send-Mouse $script:win Move $c[0] $c[1]
    if (-not (Wait-Until { (Read-Bar).pane.compact } -TimeoutSec 10 -IntervalMs 100)) { Add-Failure 'the pane never opened again while Claude Code compacted' }
    $b = Read-Bar
    $found.compacting = [ordered]@{ enabled = $b.pane.compact.enabled; why = $b.pane.compact.why }
    # Not $t: PowerShell's names are case-insensitive, and $T is the test (Start-Test)
    $tr = Get-Transcript
    if ($tr -and (Select-String -LiteralPath $tr.FullName -SimpleMatch '"compact_boundary"' -Quiet)) { 'compacting: not judged, the compaction was in before the pane opened again' }
    elseif ($b.pane.compact.enabled) { Add-Failure "Compact now lit again while Claude Code compacted, the pane opened again: a press would queue a second /compact" }
    else { "compacting: Compact now dim with the pane opened again, '$($b.pane.compact.why)'" }
  } else { Add-Failure "no request came to hold after Compact now: $($api.Log -join ' | ')" }
  $api.Release.Set()
  $compacted = Wait-Until { $t = Get-Transcript; $t -and (Select-String -LiteralPath $t.FullName -SimpleMatch '"compact_boundary"' -Quiet) } -TimeoutSec 60 -IntervalMs 500
  Save-Screen 'compacted' | Out-Null
  $found.summaries = $api.Summaries
  if (-not $compacted) { throw "Claude Code didn't compact in 60 s after Compact now: $($api.Log -join ' | ') (captures\compacted.plain.txt)" }
  'Compact now: Claude Code compacted, a compact_boundary in the transcript'
  Send-Mouse $script:win Move 120 200
  # The rule's import section goes with the compaction, and the rule comes back as a file attached
  # again after the summary, a section of its own (the records after the boundary may be read a
  # wake after it)
  $gone = Wait-Until { $b = Read-Bar; $script:bar = $b; $b -and $b.sections -and @($b.sections | Where-Object { (Test-Rule $_) -and $_.kind -ne 'attached' }).Count -eq 0 } -TimeoutSec 20 -IntervalMs 250
  $found.after = @($script:bar.sections | ForEach-Object { "$($_.kind) $($_.tokens) $($_.path)" })
  "after the compaction: estimate $($script:bar.estimate), $(@($script:bar.sections | ForEach-Object { "$($_.kind) $($_.tokens)" }) -join ', ')"
  if (-not $gone) { Add-Failure "the rule's import is still on the bar after the compaction: $($found.after -join '; ')" } else { "the bar followed: the rule's import gone" }
  Wait-Until { $b = Read-Bar; $script:bar = $b; @($b.sections | Where-Object { (Test-Rule $_) -and $_.kind -eq 'attached' }).Count -gt 0 } -TimeoutSec 10 -IntervalMs 250 | Out-Null
  $attachedNow = @($script:bar.sections | Where-Object kind -eq 'attached' | ForEach-Object { Split-Path -Leaf $_.path })
  $found.attached = $attachedNow
  if (-not $attachedNow.Count) { Add-Failure "no section of the bar is a file attached again after the summary: $($found.after -join '; ')" } else { "attached again after the summary, on the bar: $($attachedNow -join ', ')" }
  # The rule's attached section: hatched, its row the import's Undo saying what's true of it
  $ruleAt = [array]::IndexOf(@($script:bar.sections | ForEach-Object { (Test-Rule $_) -and $_.kind -eq 'attached' }), $true)
  if ($ruleAt -lt 0) { Add-Failure "the removed rule wasn't attached again after the summary, on the bar: $($found.after -join '; ')" }
  else {
    if (-not $script:bar.sections[$ruleAt].removed) { Add-Failure "the rule attached again isn't hatched as removed" }
    Send-Mouse $script:win Move $c[0] $c[1]
    $open = Wait-Until { $p = (Read-Bar).pane; $p -and @($p.rows | Where-Object section -eq $ruleAt).Count } -TimeoutSec 10 -IntervalMs 100
    $r = @((Read-Bar).pane.rows | Where-Object section -eq $ruleAt)[0]
    $found.attached_row = [ordered]@{ button = $r.button; note = $r.note }
    if (-not $open) { Add-Failure "the pane never showed the attached rule's row" }
    elseif ($r.button -ne 'Undo' -or $r.note -cne 'Attached again after the summary, @ off in CLAUDE.md') { Add-Failure "the attached rule's row is '$($r.button)', '$($r.note)'" }
    else { "the attached rule's row: $($r.button), '$($r.note)'" }
    Send-Mouse $script:win Move 120 200
    Wait-Until { -not (Read-Bar).pane } -TimeoutSec 10 -IntervalMs 100 | Out-Null
  }
  Test-Settings 'compacted:'

  # 4. The turn after: what Claude Code records after a compaction, and what the bar shows then.
  # Claude Code 2.1.319 attaches the files it had read again after the summary (a `file` record
  # with the content, or a `compact_file_reference` naming it), the removed import among them, and
  # loads the CLAUDE.md files again only with the next turn (CONTEXT.md, "Measured")
  $turns = $api.Turns
  Type-Line 'One more turn please'
  if (-not (Wait-Until { $api.Turns -gt $turns } -TimeoutSec 30 -IntervalMs 250)) { Add-Failure "Claude Code asked for no turn after the compaction: $($api.Log -join ' | ')" }
  Start-Sleep 2
  Save-Screen 'next-turn' | Out-Null
  $records = @(Get-Content -LiteralPath (Get-Transcript).FullName | ForEach-Object { try { $_ | ConvertFrom-Json -Depth 50 } catch { } })
  $at = [array]::FindIndex([object[]]$records, [Predicate[object]] { param($r) $r.subtype -eq 'compact_boundary' })
  $found.after_compaction = @($records[($at + 1)..($records.Count - 1)] | ForEach-Object {
    $a = $_.attachment
    if ($a) { "attachment:$($a.type)$(if ($a.filename) { " $(Split-Path -Leaf $a.filename)" })$(if ($a.type -eq 'instructions') { ' ' + (@($a.files | ForEach-Object { Split-Path -Leaf $_.path }) -join ',') })" } else { $_.type }
  })
  $b = Read-Bar
  $found.next_turn = @($b.sections | ForEach-Object { "$($_.kind) $($_.tokens) $($_.path)" })
  "the turn after: in use $($b.in_use), $(@($b.sections | ForEach-Object { "$($_.kind) $($_.tokens)" }) -join ', ')"
  "recorded after the compaction: $($found.after_compaction -join '; ')"
  $restored = @($records[($at + 1)..($records.Count - 1)] | Where-Object { $_.attachment.type -eq 'file' -and (Test-Rule @{ path = $_.attachment.filename }) })
  if (-not $restored) { Add-Failure "the removed import wasn't attached again after the summary as a file attachment: Claude Code's restore isn't as lbterm/CONTEXT.md says, so the bar's words aren't true ($($found.after_compaction -join '; '))" } else { "the removed import attached again after the summary ($($restored.Count)), as Claude Code does with no rule" }
  # The CLAUDE.md files loaded again with the turn, read from disk: the work folder's with its
  # import's @ off, the path left as plain text, and no import; the attachment still in
  if (@($b.sections | Where-Object { (Test-Rule $_) -and $_.kind -ne 'attached' }).Count) { Add-Failure "the rule is imported again with the turn after the compaction: $($found.next_turn -join '; ')" }
  if (-not @($b.sections | Where-Object { (Test-Rule $_) -and $_.kind -eq 'attached' }).Count) { Add-Failure "the rule's attached section went with the turn after the compaction: $($found.next_turn -join '; ')" }
  Test-Settings 'the turn after:'
  $mdPath = ($md -replace '\\', '/').ToLowerInvariant()
  if (-not @($b.sections | Where-Object { $_.path -and (($_.path -replace '\\', '/').ToLowerInvariant() -eq $mdPath) }).Count) { Add-Failure "the work folder's CLAUDE.md isn't on the bar with the turn after the compaction: $($found.next_turn -join '; ')" }
  [IO.File]::WriteAllLines((Join-Path $captures 'api.log'), [string[]]$api.Log)

  # 5. Claude Code's own /context beside the bar, for the same conversation: the files it counts
  # against the bar's file sections by path, and its numbers beside the bar's. What it said is read
  # from the transcript: Claude Code 2.1.312 kept it as a local_command record's contextUsage, and
  # 2.1.319 keeps the ANSI picture in a local_command record and the same as Markdown in a user
  # record ("## Context Usage": the tokens, a table by category, one of memory files), which is
  # read here. The pulled-in files, the memory files Claude read and the files attached again
  # after a summary are the conversation's to
  # /context (its Messages), so only the files loaded at the start are held to each other
  $seen = @(Get-Content -LiteralPath (Get-Transcript).FullName).Count
  Type-Line '/context'
  $script:contextLine = $null
  $said = Wait-Until {
    $hit = @(Get-Content -LiteralPath (Get-Transcript).FullName | Select-Object -Skip $seen | Where-Object { $_ -like '*"contextUsage"*' -or $_ -like '*## Context Usage*' }) | Select-Object -Last 1
    if ($hit) { $script:contextLine = $hit; $true } else { $false }
  } -TimeoutSec 40 -IntervalMs 250
  if (-not $said) {
    Save-Screen 'context-stuck' | Out-Null
    Add-Failure "/context kept nothing in the transcript: $(@($api.Log | Select-Object -Last 4) -join ' | ')"
  } else {
    Start-Sleep 1
    Save-Screen 'context' | Out-Null
    $rec = $script:contextLine | ConvertFrom-Json -Depth 50
    # Its numbers as written (`5.3k`, `31`, `~360`) in tokens
    $tokens = { param([string]$t) $t = $t.Trim().TrimStart('~'); if ($t -match '^([\d.]+)k$') { [int]([double]$Matches[1] * 1000) } elseif ($t -match '^[\d.]+$') { [int][double]$t } else { $null } }
    if ($rec.contextUsage) {
      $u = $rec.contextUsage
      $total = $u.total_tokens; $window = $u.raw_max_tokens
      $categories = @($u.categories | ForEach-Object { [pscustomobject]@{ name = $_.name; tokens = $_.tokens } })
      $files = @($u.memory_files | ForEach-Object { [pscustomobject]@{ path = $_.path; tokens = $_.tokens } })
    } else {
      $md = [string]$rec.message.content
      if ($md -match '\*\*Tokens:\*\*\s*([\d.]+k?)\s*/\s*([\d.]+[kM]?)') { $total = & $tokens $Matches[1]; $window = $Matches[2] }
      $table = { param([string]$heading) $part = ($md -split '(?m)^### ')
        $body = @($part | Where-Object { $_.StartsWith($heading) })[0]
        @($body -split "`n" | Where-Object { $_ -match '^\|' -and $_ -notmatch '^\|\s*-' } | Select-Object -Skip 1 | ForEach-Object { , @($_.Trim().Trim('|') -split '\|' | ForEach-Object { $_.Trim() }) }) }
      $categories = @(& $table 'Estimated usage by category' | ForEach-Object { [pscustomobject]@{ name = $_[0]; tokens = & $tokens $_[1] } })
      $files = @(& $table 'Memory Files' | ForEach-Object { [pscustomobject]@{ path = $_[1]; tokens = & $tokens $_[2] } })
    }
    $b = Read-Bar
    $norm = { param($p) ($p -replace '\\', '/').ToLowerInvariant() }
    $loaded = @($b.sections | Where-Object { $_.path -and $_.kind -notin 'pulled_in', 'elsewhere', 'memory', 'attached' })
    "/context ($(if ($rec.contextUsage) { 'contextUsage' } else { 'its Markdown' })): $total of $window; the bar: in use $($b.in_use) of $($b.window)$(if ($b.estimate) { ' (an estimate)' })"
    foreach ($c in $categories) { "  /context's $($c.name): $($c.tokens)" }
    foreach ($sec in @($b.sections | Where-Object { -not $_.path })) { "  the bar's $($sec.kind): $($sec.tokens)" }
    if (-not $files.Count) { Add-Failure "/context named no memory files: $($script:contextLine.Substring(0, [Math]::Min(300, $script:contextLine.Length)))" }
    $pairs = foreach ($m in $files) {
      $sec = @($loaded | Where-Object { (& $norm $_.path) -eq (& $norm $m.path) })[0]
      "  $(Split-Path -Leaf $m.path) in $(Split-Path -Leaf (Split-Path $m.path)): /context $($m.tokens), the bar $(if ($sec) { "$($sec.kind) $($sec.tokens)" } else { 'none' })"
      if (-not $sec) { Add-Failure "/context counts $($m.path) ($($m.tokens) tokens), which isn't a section of the bar" }
    }
    $pairs
    $theirs = @($files | ForEach-Object { & $norm $_.path })
    $more = @($loaded | Where-Object { $theirs -notcontains (& $norm $_.path) } | ForEach-Object { "$($_.kind) $(Split-Path -Leaf $_.path)" })
    if ($more) { Add-Failure "on the bar and not in /context's files: $($more -join '; ')" }
    # What's in use, held to /context's total within its rounding: its Markdown's figure to the
    # unit of its last digit, rounded or cut (51.6k is 51,600 give or take 100); contextUsage's is the count itself,
    # which on his transcripts was the bar's to 8 tokens in 55,339 (CONTEXT.md, "Measured")
    $round = if ($rec.contextUsage) { [Math]::Ceiling($total * 0.001) } elseif ($md -match '\*\*Tokens:\*\*\s*\d+\.(\d+)k') { 1000 / [Math]::Pow(10, $Matches[1].Length) } elseif ($md -match '\*\*Tokens:\*\*\s*\d+k') { 1000 } else { 0 }
    if ($null -eq $total) { Add-Failure "/context's total couldn't be read" }
    elseif ([Math]::Abs([double]$b.in_use - [double]$total) -gt $round) { Add-Failure "the bar's in use $($b.in_use) isn't /context's $total within its rounding ($round)" }
    else { "in use: the bar's $($b.in_use), /context's $total, within its rounding ($round)" }
    $found.context = [ordered]@{ total = $total; window = $window; categories = @($categories | ForEach-Object { "$($_.name) $($_.tokens)" }); files = @($pairs); in_use = $b.in_use; only_bar = $more }
  }
  $found | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'found.json')
} -Passed "claude-compact: real Claude Code $version, its rule's @ taken off its line in the test's CLAUDE.md, the path left as plain text not imported again, Compact now typing /compact at once and writing no settings file, the rule attached again after the summary as its own hatched section with the import's Undo and true words, and the bar followed the compaction"
