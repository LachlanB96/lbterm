#Requires -Version 7
# Variant: -Local
# Covers: lbterm-extras/src/conversation.rs, lbterm-extras/src/conversation/*.rs, lbterm-extras/src/summaries.rs, lbterm-extras/src/watch.rs, wezterm-gui/src/conversations.rs, wezterm-gui/src/summaries.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, lbterm/perf/fake-claude.ps1, lbterm/perf/threads.ps1
<#
What the scroll strip's conversations and the sessions' summaries cost (lbterm/AT-REST.md, "The
scroll strip and the summaries"), in a test window that looks like Lachlan's (New-TestConfig) on a
mux domain of its own (-Local: its own tabs), against a transcript the size of his longest:
lbterm-extras' long stand-in (src/conversation/long.rs, written the same way here), 276 messages
and 46 MB, 5,801 rows, which fake-claude.ps1 -Reads 5 draws; and a short one of three messages
beside it, 68 rows, as full a screen. Never a real transcript: the strip's transcripts and the
summaries are folders of the test's own.

1. The long one opened: its tab activated from a shell's, the strip shows all its rows within
   -OpenBudgetMs. What reading it cost lbterm-conversations.
2. At rest on it for -RestSeconds: no frame, and neither lbterm-conversations nor lbterm-summaries
   wakes more often than upstream's update_checker, which sleeps a day at a time: what wakes that
   is the PC's (a process suspended and resumed, as the warden does to heavy ones, wakes
   every thread of it once; 4 times in a minute on 2026-10-30).
3. Claude working (fake-claude's spinner, ten frames a second) while -Appends of Claude's words
   are appended to the transcript -AppendMs apart: each shows on the strip (its rows) within
   -ShowBudgetMs, the screen read once for each at most; then the same in the short one's tab. The
   GUI thread costs the long one's at most -LengthRatio times what it costs the short one's and a
   millisecond a second more, and lbterm-conversations half a millisecond more: until 2026-10-30
   each append redid and compared the whole conversation and laid it out again.
4. The spinner alone for -SpinSeconds in each: the strip reads the screen not once (the state
   file's `looks`; it reads it only when the conversation's rows or the input box's rules change),
   and the window paints a frame for each of the spinner's and no more, -SpinFps a second at most
   (fake-claude draws ten). The GUI thread's CPU a frame is reported, the long one's and the short
   one's. Until 2026-10-30 every frame of the spinner read and located the screen, at most every
   120 ms with a frame of its own to catch up.
5. Away and back: the shell's tab shown while -Appends more are written to the long one, which
   isn't read while nothing shows it (lbterm-conversations doesn't wake); with -AwaySeconds, the
   short one's tab for the rest of that time, as another Claude tab is, one of its words appended
   at the end; back on the long one, only what was added is read, and the rows show within
   -ShowBudgetMs. With -AwaySeconds 620: until 2026-10-30 the reading thread, going round for
   any session, forgot those no strip had asked for in ten minutes, and read them again from
   their start when shown, 32 MB a look.
6. The statusline writing the long session's folder (`<sid>.cwd.txt`) -CwdWrites times: each is
   that file read alone (two read operations, its words and its end), where it was all seven of the
   session's files (the five there, ten read operations).

It fails on any of those. cost.json has every number; -Measure judges nothing.

  pwsh -File lbterm\perf\conversation-cost.ps1 [-Local] [-AwaySeconds 620] [-Bin <build>] [-Measure]
#>
param(
  [string]$Bin,
  [switch]$Local,
  [int]$RestSeconds = 20,
  [int]$Appends = 30,
  [int]$AppendMs = 500,
  [int]$SpinSeconds = 8,
  [int]$CwdWrites = 20,
  [int]$AwaySeconds = 0,
  [double]$OpenBudgetMs = 1000,
  [double]$ShowBudgetMs = 250,
  [double]$LengthRatio = 1.5,
  [double]$SpinFps = 12,
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('conversation-cost' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin
$mux = -not $Local
. (Join-Path $PSScriptRoot 'threads.ps1')

# The long stand-in, record for record as lbterm-extras' long.rs writes it: each message a
# prompt, five file reads (their call, a hook's progress record, their result with the file in it
# twice), a 14-line answer and the turn's end
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.IO;
using System.Text;
public static class LbtermTranscript {
  static string Stamp(long second) {
    return DateTimeOffset.FromUnixTimeSeconds(1793000000 + second).UtcDateTime.ToString("yyyy-MM-ddTHH:mm:ss.fffZ");
  }
  static string Head(string sid, long n) {
    return "{\"parentUuid\":\"00000000-0000-4000-8000-" + Math.Max(0, n - 1).ToString("D12") + "\",\"isSidechain\":false,\"userType\":\"external\",\"cwd\":\"C:\\\\work\\\\demo\",\"sessionId\":\"" + sid + "\",\"version\":\"2.1.312\",\"gitBranch\":\"main\",";
  }
  static string Tail(string type, long n, long second) {
    return ",\"type\":\"" + type + "\",\"uuid\":\"00000000-0000-4000-8000-" + n.ToString("D12") + "\",\"timestamp\":\"" + Stamp(second) + "\"}\n";
  }
  // A file's lines, as the Read tool returns them, JSON-escaped
  static string File(int m, int r, int lines) {
    var sb = new StringBuilder();
    for (int i = 1; i <= lines; i++)
      sb.Append(i.ToString().PadLeft(6)).Append('\u2192').Append("    let value_").Append(m).Append('_').Append(r).Append('_').Append(i)
        .Append(" = compute(\\\"part ").Append(m).Append("\\\", ").Append(i).Append(") + offset; // step ").Append(i).Append("\\n");
    return sb.ToString();
  }
  public static long Write(string path, string sid, int messages, int reads, int fileLines) {
    long n = 0;
    using (var w = new StreamWriter(path, false, new UTF8Encoding(false))) {
      for (int m = 1; m <= messages; m++) {
        long s = m * 60L;
        n++; w.Write(Head(sid, n) + "\"message\":{\"role\":\"user\",\"content\":\"message number " + m + ", about part " + m + " of the work\"},\"origin\":{\"kind\":\"human\"}" + Tail("user", n, s));
        for (int r = 1; r <= reads; r++) {
          string id = "toolu_" + m.ToString("D4") + "_" + r;
          string file = "C:\\\\work\\\\demo\\\\src\\\\part" + m + "\\\\file" + r + ".rs";
          n++; w.Write(Head(sid, n) + "\"message\":{\"model\":\"claude-opus-4\",\"id\":\"msg_" + m.ToString("D4") + "_" + r + "\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"" + id + "\",\"name\":\"Read\",\"input\":{\"file_path\":\"" + file + "\"}}],\"stop_reason\":\"tool_use\",\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":40000,\"output_tokens\":90}},\"requestId\":\"req_" + m.ToString("D4") + "_" + r + "\"" + Tail("assistant", n, s + r));
          n++; w.Write(Head(sid, n) + "\"data\":{\"type\":\"hook_progress\",\"hookEvent\":\"PostToolUse\",\"hookName\":\"PostToolUse:Read\",\"command\":\"claudehook.exe posttool\"},\"parentToolUseID\":\"" + id + "\",\"toolUseID\":\"" + id + "\"" + Tail("progress", n, s + r));
          string content = File(m, r, fileLines);
          n++; w.Write(Head(sid, n) + "\"message\":{\"role\":\"user\",\"content\":[{\"tool_use_id\":\"" + id + "\",\"type\":\"tool_result\",\"content\":\"" + content + "\"}]},\"toolUseResult\":{\"type\":\"text\",\"file\":{\"filePath\":\"" + file + "\",\"content\":\"" + content + "\",\"numLines\":" + fileLines + ",\"startLine\":1,\"totalLines\":" + fileLines + "}}" + Tail("user", n, s + r));
        }
        var answer = new StringBuilder();
        for (int j = 1; j <= 14; j++) { if (j > 1) answer.Append("\\n"); answer.Append("answer " + m + ", line " + j); }
        n++; w.Write(Head(sid, n) + "\"message\":{\"model\":\"claude-opus-4\",\"id\":\"msg_" + m.ToString("D4") + "_t\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"" + answer + "\"}],\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":40000,\"output_tokens\":300}},\"requestId\":\"req_" + m.ToString("D4") + "_t\"" + Tail("assistant", n, s + 30));
        n++; w.Write(Head(sid, n) + "\"subtype\":\"turn_duration\",\"durationMs\":12000,\"isMeta\":false" + Tail("system", n, s + 31));
      }
    }
    return new FileInfo(path).Length;
  }
  // One more of Claude's words, as a record of its own
  public static string MoreWords(string sid, int i) {
    long n = 1000000 + i;
    return Head(sid, n) + "\"message\":{\"model\":\"claude-opus-4\",\"id\":\"msg_more_" + i + "\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"more words " + i + ", written while the test watched\"}],\"usage\":{\"input_tokens\":1,\"output_tokens\":9}}" + Tail("assistant", n, 1000000 + i);
  }
}
'@

$projects = Join-Path $T.Out 'projects'
$folder = Join-Path $projects 'C--work-demo'
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $folder, $summaries, $home2 | Out-Null
$long = '6a1f0c3e-2b7d-4e95-8c41-9d3b5f2e7a60'
$short = '1c9e4b7a-5d2f-4a83-9b60-7e1d3c8f2a45'
$longFile = Join-Path $folder "$long.jsonl"
$shortFile = Join-Path $folder "$short.jsonl"
$clock = [Diagnostics.Stopwatch]::StartNew()
$longBytes = [LbtermTranscript]::Write($longFile, $long, 276, 5, 200)
$shortBytes = [LbtermTranscript]::Write($shortFile, $short, 3, 5, 200)
"the stand-ins: $longBytes bytes and $shortBytes, written in $([Math]::Round($clock.Elapsed.TotalSeconds, 1)) s"
# Rows as fake-claude.ps1 -Reads 5 draws them: its banner and 21 a message
$longRows = 5 + 276 * 21
$shortRows = 5 + 3 * 21
# A summary for each, every file the summary worker, the statusline and the question worker keep
foreach ($sid in $long, $short) {
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.label.txt"), "Part $sid".Substring(0, 12))
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.txt"), 'Working through the parts of the demo, one message at a time')
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.detail.txt"), "Goal: every part`nDone: most of them`nNext: the rest`nThen: the check`nLast: land it`n")
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.cwd.txt"), 'C:\work\demo')
  [IO.File]::WriteAllText((Join-Path $summaries "$sid.objectives.json"), '{"current":1,"objectives":[{"id":1,"text":"Every part of the demo","from":"first","status":"open"}]}')
}

$fake = Join-Path $PSScriptRoot 'fake-claude.ps1'
$shell = New-PwshProgram "Write-Host 'a shell'; Start-Sleep 3600"
$cfg = Join-Path $T.Out 'config\conversation-cost.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$domain = if ($mux) {
  @"
config.unix_domains = { {
  name = 'convcost',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 51
config.window_close_confirmation = 'NeverPrompt'
config.lbterm_tab_panel = { enabled = true, open_delay_ms = 0 }
rawset(config, 'lbterm_summaries', '$(ConvertTo-LuaPath $summaries)')
rawset(config, 'lbterm_scroll_strip', { transcripts = '$(ConvertTo-LuaPath $projects)' })
$domain
"@

$cost = [ordered]@{ mux = $mux; long_bytes = $longBytes; short_bytes = $shortBytes; open = $null; rest = $null; working = @(); spin = @(); away = $null; back = $null; cwd = $null }
Invoke-Test {
  $open = @{ ConfigFile = $cfg; Program = $shell; UserProfile = $home2; WorkingDirectory = $T.Repo; TimeoutSec = 30 }
  if ($env:LBTERM_COST_LOG) { $open.Log = $env:LBTERM_COST_LOG }
  if ($mux) { $open.Domain = 'convcost'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  if ($mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $first = (Get-Panes -Server:$mux)[0].pane_id
  if ($mux) {
    # `connect` runs the server's default program in the first tab: the shell goes in a tab of its
    # own, and that one goes
    $default = $first
    $first = New-Tab -From $default -Program $shell -Server
    Wait-Change { param($s) @($s.tabs).Count -eq 2 } -What 'the shell' | Out-Null
    Invoke-Cli kill-pane --pane-id $default -Server -Check | Out-Null
    Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'the default tab gone' | Out-Null
  }
  $longPane = New-Tab -From $first -NoActivate -Program (New-PwshProgram -File $fake -ArgumentList '-Sid', $long, '-Messages', 276, '-Reads', 5) -Server:$mux
  $shortPane = New-Tab -From $first -NoActivate -Program (New-PwshProgram -File $fake -ArgumentList '-Sid', $short, '-Messages', 3, '-Reads', 5) -Server:$mux
  Wait-Change { param($s) @($s.tabs).Count -eq 3 -and @($s.rail.rows | Where-Object { $_.label -like 'Part *' }).Count -eq 2 } -What 'three tabs, two of them named by their sessions' -TimeoutSec 30 | Out-Null
  Start-Sleep 3
  Wait-Settled -What 'the window, the shell open' | Out-Null
  $sw = { Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle) }
  $frameNow = { [long](& $sw).frame }
  $named = { param($span, [string]$name) [pscustomobject]@{
      wakes = [long](@($span.threads | Where-Object { $_.name -eq $name }) | Measure-Object wakes -Sum).Sum
      cpu_ms = [Math]::Round([double](@($span.threads | Where-Object { $_.name -eq $name }) | Measure-Object cpu_ms -Sum).Sum, 2)
    } }
  # How long from now until the window's state says $Shows, polled every 5 ms; $null after 15 s
  function Wait-Shows([scriptblock]$Shows, [long]$AfterFrame) {
    $c = [Diagnostics.Stopwatch]::StartNew()
    while ($c.Elapsed.TotalSeconds -lt 15) {
      $snap = Get-StateSnapshot $w.StateFile
      $s = if ($snap) { Get-StateWindow $snap -Hwnd ([long]$w.Handle) }
      if ($s -and [long]$s.frame -gt $AfterFrame -and (& $Shows $s)) { return [Math]::Round($c.Elapsed.TotalMilliseconds) }
      Start-Sleep -Milliseconds 5
    }
    $null
  }
  $stripRows = { param($s) if ($s.scroll_strip -and $s.scroll_strip.mode -eq 'claude') { [long]$s.scroll_strip.rows } else { -1 } }
  # Not before 15 s in: 10 s after a GUI starts, upstream's update_checker sweeps every GUI socket
  # in the runtime folder, which wakes every thread of the process once
  $left = 15 - ((Get-Date) - $w.Opened).TotalSeconds
  if ($left -gt 0) { Start-Sleep -Milliseconds ([int]($left * 1000)) }

  # 1. The long one opened
  $f0 = & $frameNow
  $a = [LbtermRest]::Take($w.Pid)
  Invoke-Cli activate-pane --pane-id $longPane -Server:$mux -Check | Out-Null
  $openMs = Wait-Shows { param($s) (& $stripRows $s) -eq $longRows } $f0
  Wait-Settled -What 'the long one shown' | Out-Null
  Start-Sleep -Milliseconds 300
  $b = [LbtermRest]::Take($w.Pid)
  $span = Measure-Span $a $b 0
  $cost.open = [ordered]@{ shown_ms = $openMs; frames = (& $frameNow) - $f0; conversations = & $named $span 'lbterm-conversations'; main = & $named $span 'main'; reads = $span.reads }
  "the long one opened: its $longRows rows on the strip in $openMs ms, $($cost.open.frames) frames; lbterm-conversations $($cost.open.conversations.cpu_ms) ms, the GUI thread $($cost.open.main.cpu_ms) ms"
  if ($null -eq $openMs) { Add-Failure "the long conversation's $longRows rows never showed on the strip (it said $(& $stripRows (& $sw)))" }
  elseif (-not $Measure -and $openMs -gt $OpenBudgetMs) { Add-Failure "the long conversation took $openMs ms to show, over $OpenBudgetMs" }

  # 2. At rest on it
  $f0 = & $frameNow
  $restFrom = Get-Date
  $a = [LbtermRest]::Take($w.Pid)
  $c = [Diagnostics.Stopwatch]::StartNew()
  Start-Sleep $RestSeconds
  $b = [LbtermRest]::Take($w.Pid)
  $rest = Measure-Span $a $b ([Math]::Round($c.Elapsed.TotalSeconds, 1))
  $rest | Add-Member frames ((& $frameNow) - $f0)
  $rest | Add-Member span "$($restFrom.ToString('HH:mm:ss.fff')) to $((Get-Date).ToString('HH:mm:ss.fff'))"
  $cost.rest = $rest
  "at rest on it, $($rest.span): $($rest.frames) frames"
  Format-Span $rest 'the GUI'
  if (-not $Measure) {
    if ($rest.frames -gt 0) { Add-Failure "at rest on the long conversation the window painted $($rest.frames) frames" }
    $pc = (& $named $rest 'update_checker').wakes
    foreach ($name in 'lbterm-conversations', 'lbterm-summaries') {
      $n = (& $named $rest $name).wakes
      if ($n -gt $pc) { Add-Failure "at rest on the long conversation $name woke $n times, where update_checker (the PC's wakes) woke $pc" }
    }
  }

  # 3 and 4. Claude working in each, its words appended; then its spinner alone
  # The pane is the server's on a mux domain, so which conversation shows is told by its rows
  function Measure-Working([string]$Which, [int]$Pane, [string]$File, [string]$Sid, [scriptblock]$Shown) {
    if (-not (& $Shown (& $stripRows (& $sw)))) {
      $f = & $frameNow
      Invoke-Cli activate-pane --pane-id $Pane -Server:$mux -Check | Out-Null
      if ($null -eq (Wait-Shows { param($s) & $Shown (& $stripRows $s) } $f)) { throw "the $Which conversation never showed on the strip" }
      Wait-Settled -What "the $Which one shown" | Out-Null
    }
    Invoke-Cli send-text --pane-id $Pane --no-paste 'W' -Server:$mux -Check | Out-Null
    Start-Sleep 1
    $looks = { $l = (& $sw).scroll_strip.looks; if ($null -eq $l) { $null } else { [long]$l } }
    $l0 = & $looks
    $shows = [Collections.Generic.List[object]]::new()
    $f0 = & $frameNow
    $a = [LbtermRest]::Take($w.Pid)
    $c = [Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt $Appends; $i++) {
      $was = & $stripRows (& $sw)
      $at = $c.ElapsedMilliseconds
      [IO.File]::AppendAllText($File, ([LbtermTranscript]::MoreWords($Sid, $script:appended++)))
      if ($Sid -eq $long) { $script:longAppended++ }
      $shows.Add((Wait-Shows { param($s) (& $stripRows $s) -gt $was } 0))
      $left = $AppendMs - ($c.ElapsedMilliseconds - $at)
      if ($left -gt 0) { Start-Sleep -Milliseconds $left }
    }
    $b = [LbtermRest]::Take($w.Pid)
    $secs = [Math]::Round($c.Elapsed.TotalSeconds, 1)
    $span = Measure-Span $a $b $secs
    $frames = (& $frameNow) - $f0
    $looked = if ($null -ne $l0) { (& $looks) - $l0 }
    $main = & $named $span 'main'; $conv = & $named $span 'lbterm-conversations'
    $times = @($shows | Where-Object { $null -ne $_ } | Sort-Object)
    $row = [ordered]@{
      which = $Which; seconds = $secs; frames = $frames; appends = $Appends; never = @($shows | Where-Object { $null -eq $_ }).Count
      shown_ms = [ordered]@{ median = $(if ($times) { $times[[int]($times.Count / 2)] }); max = $(if ($times) { $times[-1] }) }
      main_ms_a_second = [Math]::Round($main.cpu_ms / $secs, 2); conversations_ms_a_second = [Math]::Round($conv.cpu_ms / $secs, 2)
      conversations_wakes = $conv.wakes; cpu_ms = $span.cpu_ms; looks = $looked
    }
    $script:cost.working += [pscustomobject]$row
    $said = if ($null -eq $looked) { "the screen's reads unknown (the build doesn't say)" } else { "the screen read $looked times" }
    "$Which, working, $Appends appended in $secs s: $frames frames, $said; shown in $($row.shown_ms.median) ms at the median, $($row.shown_ms.max) at most ($($row.never) never); the GUI thread $($row.main_ms_a_second) ms a second, lbterm-conversations $($row.conversations_ms_a_second) ($($conv.wakes) wakes)"
    if ($row.never) { Add-Failure "$Which, working: $($row.never) of $Appends appends never showed on the strip" }
    elseif (-not $Measure -and $row.shown_ms.max -gt $ShowBudgetMs) { Add-Failure "$Which, working: an append showed $($row.shown_ms.max) ms after it was written, over $ShowBudgetMs" }
    if (-not $Measure) {
      if ($null -eq $looked) { Add-Failure "$Which, working: the build's state file doesn't say how often the strip reads the screen (scroll_strip.looks)" }
      elseif ($looked -gt $Appends) { Add-Failure "$Which, working: the strip read the screen $looked times for $Appends appends" }
    }
    # The spinner alone
    $f0 = & $frameNow
    $l0 = & $looks
    $a = [LbtermRest]::Take($w.Pid)
    $c = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep $SpinSeconds
    $b = [LbtermRest]::Take($w.Pid)
    $span = Measure-Span $a $b ([Math]::Round($c.Elapsed.TotalSeconds, 1))
    $frames = (& $frameNow) - $f0
    $looked = if ($null -ne $l0) { (& $looks) - $l0 }
    $main = & $named $span 'main'
    $spin = [pscustomobject]@{ which = $Which; seconds = $span.seconds; frames = $frames; fps = [Math]::Round($frames / $span.seconds, 1); looks = $looked; main_ms = $main.cpu_ms; main_ms_a_frame = $(if ($frames) { [Math]::Round($main.cpu_ms / $frames, 3) }) }
    $script:cost.spin += $spin
    $said = if ($null -eq $looked) { "the screen's reads unknown" } else { "the screen read $looked times" }
    "$Which, the spinner alone for $($span.seconds) s: $frames frames ($($spin.fps) a second), $said, the GUI thread $($main.cpu_ms) ms ($($spin.main_ms_a_frame) ms a frame)"
    if (-not $Measure) {
      if ($looked -gt 0) { Add-Failure "$Which, the spinner alone: the strip read the screen $looked times, where nothing of the conversation's rows changed" }
      if ($spin.fps -gt $SpinFps) { Add-Failure "$Which, the spinner alone: $($spin.fps) frames a second, over $SpinFps (it draws ten)" }
    }
    Invoke-Cli send-text --pane-id $Pane --no-paste 'W' -Server:$mux -Check | Out-Null
    Start-Sleep -Milliseconds 300
    Wait-Settled -What "$Which, done working" | Out-Null
  }
  $script:appended = 0
  $script:longAppended = 0
  Measure-Working -Which 'the long one' -Pane $longPane -File $longFile -Sid $long -Shown { param($rows) $rows -ge $longRows }
  Measure-Working -Which 'the short one' -Pane $shortPane -File $shortFile -Sid $short -Shown { param($rows) $rows -ge $shortRows -and $rows -lt 1000 }
  $lw, $sw2 = $cost.working[0], $cost.working[1]
  if (-not $Measure) {
    foreach ($k in @(@('main_ms_a_second', 1), @('conversations_ms_a_second', 0.5))) {
      $name, $more = $k
      if ($lw.$name -gt $LengthRatio * $sw2.$name + $more) { Add-Failure "working, $name was $($lw.$name) with the long conversation and $($sw2.$name) with the short one: over $LengthRatio times and $more more" }
    }
  }

  # 5. Away and back: the shell shown while the long one grows
  $f = & $frameNow
  Invoke-Cli activate-pane --pane-id $first -Server:$mux -Check | Out-Null
  if ($null -eq (Wait-Shows { param($s) $s.scroll_strip -and $s.scroll_strip.mode -eq 'shell' } $f)) { throw 'the shell never showed' }
  Wait-Settled -What 'the shell shown' | Out-Null
  $before = (Get-Item -LiteralPath $longFile).Length
  $a = [LbtermRest]::Take($w.Pid)
  $c = [Diagnostics.Stopwatch]::StartNew()
  for ($i = 0; $i -lt $Appends; $i++) {
    [IO.File]::AppendAllText($longFile, ([LbtermTranscript]::MoreWords($long, $script:appended++)))
    $script:longAppended++
    Start-Sleep -Milliseconds 100
  }
  Start-Sleep -Milliseconds 500
  $b = [LbtermRest]::Take($w.Pid)
  $span = Measure-Span $a $b ([Math]::Round($c.Elapsed.TotalSeconds, 1))
  $cost.away = [ordered]@{ seconds = $span.seconds; appended_bytes = (Get-Item -LiteralPath $longFile).Length - $before; conversations = & $named $span 'lbterm-conversations' }
  $pc = (& $named $span 'update_checker').wakes
  "away: $Appends appended ($($cost.away.appended_bytes) bytes) while the shell showed; lbterm-conversations woke $($cost.away.conversations.wakes) times (update_checker $pc)"
  if (-not $Measure -and $cost.away.conversations.wakes -gt $pc) { Add-Failure "the long conversation, shown nowhere, woke lbterm-conversations $($cost.away.conversations.wakes) times as it grew, where update_checker (the PC's wakes) woke $pc" }
  # Away longer: on the short one's tab for the rest of -AwaySeconds, as Lachlan is on another
  # Claude tab, which at the end shows one more of Claude's words (until 2026-10-30 the reading
  # thread forgot, as it went round for any session, those no strip had asked for in ten minutes)
  $left = $AwaySeconds - $c.Elapsed.TotalSeconds
  if ($left -gt 0) {
    $f = & $frameNow
    Invoke-Cli activate-pane --pane-id $shortPane -Server:$mux -Check | Out-Null
    $shortShown = { param($s) $r = & $stripRows $s; $r -ge $shortRows -and $r -lt 1000 }
    if ($null -eq (Wait-Shows $shortShown $f)) { throw 'the short conversation never showed' }
    Wait-Settled -What 'the short one shown' | Out-Null
    Start-Sleep -Milliseconds ([int](($AwaySeconds - $c.Elapsed.TotalSeconds) * 1000))
    $was = & $stripRows (& $sw)
    [IO.File]::AppendAllText($shortFile, ([LbtermTranscript]::MoreWords($short, $script:appended++)))
    if ($null -eq (Wait-Shows { param($s) (& $stripRows $s) -gt $was } 0)) { Add-Failure "after $AwaySeconds s away, the short one's new words never showed" }
    Wait-Settled -What 'the short one grown' | Out-Null
    $cost.away.seconds = [Math]::Round($c.Elapsed.TotalSeconds, 1)
    "away for $($cost.away.seconds) s in all, the short one's tab shown since $([Math]::Round($span.seconds, 1)) s, and one of its words shown at the end"
  }
  $want = $longRows + 2 * $script:longAppended
  $f0 = & $frameNow
  $a = [LbtermRest]::Take($w.Pid)
  Invoke-Cli activate-pane --pane-id $longPane -Server:$mux -Check | Out-Null
  $backMs = Wait-Shows { param($s) (& $stripRows $s) -eq $want } $f0
  Wait-Settled -What 'back on the long one' | Out-Null
  $b = [LbtermRest]::Take($w.Pid)
  $span = Measure-Span $a $b 0
  $cost.back = [ordered]@{ shown_ms = $backMs; rows = & $stripRows (& $sw); want = $want; conversations = & $named $span 'lbterm-conversations'; reads = $span.reads }
  "back on it: its $want rows in $backMs ms; lbterm-conversations $($cost.back.conversations.cpu_ms) ms (opening it from nothing took $($cost.open.conversations.cpu_ms))"
  if ($null -eq $backMs) { Add-Failure "back on the long conversation its $want rows never showed (the strip said $($cost.back.rows))" }
  elseif (-not $Measure -and $backMs -gt $ShowBudgetMs) { Add-Failure "back on the long conversation its rows showed $backMs ms after, over $ShowBudgetMs" }

  # 6. The statusline writing the session's folder
  $cwdFile = Join-Path $summaries "$long.cwd.txt"
  Start-Sleep 1
  $a = [LbtermRest]::Take($w.Pid)
  for ($i = 0; $i -lt $CwdWrites; $i++) {
    [IO.File]::WriteAllText($cwdFile, "C:\work\demo\part$i")
    Start-Sleep -Milliseconds 300
  }
  Start-Sleep -Milliseconds 500
  $b = [LbtermRest]::Take($w.Pid)
  $span = Measure-Span $a $b 0
  $sums = & $named $span 'lbterm-summaries'
  $cost.cwd = [ordered]@{ writes = $CwdWrites; reads = $span.reads; reads_a_write = [Math]::Round($span.reads / $CwdWrites, 2); summaries = $sums }
  "the session's folder written $CwdWrites times: $($span.reads) file reads ($($cost.cwd.reads_a_write) a write), lbterm-summaries $($sums.wakes) wakes, $($sums.cpu_ms) ms"
  if (-not $Measure -and $cost.cwd.reads_a_write -gt 2.5) { Add-Failure "each write of the session's folder file cost $($cost.cwd.reads_a_write) read operations, over one file's 2" }

  $cost | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "conversation-cost: a 46 MB conversation shows at once, costs nothing at rest, each append costs what it adds (as much as a one-message conversation's), the spinner reads no screen, a return reads only what was added, and a summary's one file changing is one read"
