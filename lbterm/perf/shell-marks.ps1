#Requires -Version 7
# Variant: -Local
# Covers: wezterm-gui/src/termwindow/render/marks.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, lbterm-extras/src/marks.rs, mux/src/lbterm_prompts.rs, wezterm-client/src/pane/lbterm_held.rs, wezterm-client/src/pane/renderable.rs, wezterm-client/src/client.rs, wezterm-mux-server-impl/src/sessionhandler.rs, codec/src/lib.rs
<#
What a shell's prompt marks cost on the scroll strip (lbterm/SCROLL-STRIP.md, "A shell's marks"),
with a scrollback as long as Lachlan's (scrollback_lines = 50000), from the state file, the
counters (metrics) and the processes' cycles: nothing here rests on a capture, so it runs with the
PC locked. A test window that looks like Lachlan's LBTerm (New-TestConfig: the strip on), on a mux
domain of its own as his tabs are (-Local: the tabs the GUI's own), with three tabs:

  A, a pwsh at its prompt, open first;
  B, -Lines lines of a build's log with a shell's prompt (OSC 133) every -Every lines, printed
     while it's in the background;
  C, the same with 100 lines, for B to be held against;
  D, a prompt, then on a file the test writes the same as B.

1. First look. B is opened (a key). Its strip shows a mark for each of its -Lines/-Every prompts,
   all of them, within -ShowMs; what it took (the switch's frame, the marks', the GUI thread, the
   longest task, the server's CPU and the GetLines the window asked for, the GUI's memory) is said.
   Until 2026-10-31 a mux pane's first look asked the server for every line of its scrollback,
   blank lines made in the GUI for each until they came, and marked none of them.
2. Output. B prints a line every 50 ms for -Seconds, a prompt every 40th, then C does the same:
   what a frame costs the GUI thread with 50,000 rows behind it against 100, the frames, the
   server's CPU and the GetLines; the new prompts marked. Until 2026-10-31 each frame with output
   went through every row of the scrollback.
3. The window made wider: a new width rewraps the scrollback and numbers its rows again, so every
   row is read again, and B's marks are all there again.
4. (Mux only) The GUI is killed and a new one attaches, as after a "Restart to update": B's marks,
   all of them again, with what it took.
5. Looked at, then printed in the background: D is opened (its one prompt marked) and left for A,
   prints -Lines lines in the background, and is opened again: every prompt marked, and nothing
   held over -HoldMs. Until 2026-10-31 that look read every row that went into the scrollback
   in the frame that showed the tab (80 ms locally with 50,000 rows, 17 ms on a mux domain).

It fails when a first look, the wider window, the attach or the look after output in the
background marks fewer prompts than were printed, when the output's prompts aren't marked, when
a first look or the look after output in the background holds the GUI thread over -HoldMs in one
go (a frame or a task), when a frame with 50,000 rows behind it costs the GUI thread more than
-FrameMs over one with 100, or when the window asks the server for more than -MaxFetchKb of lines
for a look or the output (a viewport's worth is what drawing it needs). cost.json in
target\perf\shell-marks has every number.

  pwsh -File lbterm\perf\shell-marks.ps1 [-Local] [-Lines 50000] [-Every 500] [-Seconds 10] [-Bin <build>]
#>
param([string]$Bin, [switch]$Local, [int]$Lines = 50000, [int]$Every = 500, [double]$Seconds = 10,
  [double]$FrameMs = 0.3, [double]$MaxFetchKb = 512, [double]$ShowMs = 2000, [double]$HoldMs = 30, [switch]$NoChecks)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$Mux = -not $Local
$T = Start-Test ('shell-marks' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin
$HomeDir = Join-Path $T.Out 'home'
$Cfg = Join-Path $T.Out 'config\shell-marks.lua'
New-Item -ItemType Directory -Force $HomeDir, (Split-Path $Cfg), (Join-Path $T.Out 'transcripts') | Out-Null

# A build's log of -Lines lines with a shell's prompt every -Every, file $Done written when it's
# all out, then, on g, a line every 50 ms with a prompt every 40th until s. With $Go, a prompt
# first, and the log once file $Go is there
function New-LogProgram([int]$Count, [int]$Each, [string]$Done, [string]$Go) {
  $wait = if ($Go) { "[Console]::Write((Mark 0)); while (-not (Test-Path '$Go')) { Start-Sleep -Milliseconds 100 }" } else { '' }
  New-PwshProgram @"
`$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new(`$false)
function Mark([long]`$n) { "`$e]133;A`$e\PS C:\work> `$e]133;B`$e\Invoke-Step `$n``r``n`$e]133;C`$e\" }
$wait
`$sb = [Text.StringBuilder]::new()
for (`$i = 1; `$i -le $Count; `$i++) {
  if (`$i % $Each -eq 0) { [void]`$sb.Append((Mark `$i)) } else { [void]`$sb.Append("  line `$i of the build's log, compiling one thing or another``r``n") }
  if (`$sb.Length -ge 262144) { [Console]::Write(`$sb.ToString()); [void]`$sb.Clear() }
}
[Console]::Write(`$sb.ToString())
[IO.File]::WriteAllText('$Done', 'done')
while (`$true) {
  `$k = [Console]::ReadKey(`$true)
  if (`$k.KeyChar -ne 'g') { continue }
  `$i = 0; `$sw = [Diagnostics.Stopwatch]::StartNew()
  while (`$true) {
    if ([Console]::KeyAvailable -and [Console]::ReadKey(`$true).KeyChar -eq 's') { break }
    `$i++
    if (`$i % 40 -eq 0) { [Console]::Write((Mark (1000000 + `$i))) } else { [Console]::WriteLine("tick `$i") }
    `$wait = `$i * 50 - `$sw.ElapsedMilliseconds
    if (`$wait -gt 0) { Start-Sleep -Milliseconds `$wait }
  }
}
"@
}

$domain = if ($Mux) { @"
config.unix_domains = { {
  name = 'shellmarks',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $Cfg)', '--daemonize' },
} }
"@ }
New-TestConfig -Path $Cfg -Lua (@"
config.initial_cols = 110
config.initial_rows = 30
config.window_close_confirmation = 'NeverPrompt'
config.default_cwd = [[$($T.Out)]]
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
config.scrollback_lines = 50000
rawset(config, 'lbterm_projects', nil)
rawset(config, 'lbterm_scroll_strip', { transcripts = [[$(Join-Path $T.Out 'transcripts')]] })
config.keys = config.keys or {}
for i, key in ipairs({ 'F2', 'F3', 'F4', 'F5' }) do
  table.insert(config.keys, { key = key, mods = 'NONE', action = wezterm.action.ActivateTab(i - 1) })
end
"@ + "`n" + $domain)

function Open-Window {
  $open = @{ ConfigFile = $Cfg; UserProfile = $HomeDir; TimeoutSec = 30 }
  if ($Mux) { $open.Domain = 'shellmarks'; $open.Socket = $T.Sock } else { $open.Program = New-PwshProgram }
  $w = Open-TestWindow @open
  if ($Mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 20)) { throw "the test's mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) @($s.panes).Count -ge 1 } -Window $w -What 'the first pane' -TimeoutSec 30 | Out-Null
  $w
}

function Get-Win($w) { Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle) }
function Get-PromptMarks($sw) { @($sw.scroll_strip.marks | Where-Object kind -eq 'prompt').Count }
# metrics, or nothing from a build without them (the installed one before perf/budgets)
function Get-MetricsOrNot($w, [switch]$Reset) { try { Get-Metrics -Window $w -Reset:$Reset -TimeoutMs 3000 } catch { $null } }
# The GUI's cycles, and the test's mux server's
function Get-Cpu($w) {
  if ($Mux) { Get-CpuSnapshot -Window $w -Servers @(Get-TestServers | ForEach-Object { [int]$_.ProcessId }) } else { Get-CpuSnapshot -Window $w }
}
# Cycles a millisecond
$Rate = Get-CycleRate
function Get-Span($w, $a, $b) {
  [pscustomobject]@{
    GuiThreadMs = ($b.ThreadCycles - $a.ThreadCycles) / $Rate
    GuiMs       = ($b.Cycles - $a.Cycles) / $Rate
    ServerMs    = (($b.Servers | Measure-Object Cycles -Sum).Sum - ($a.Servers | Measure-Object Cycles -Sum).Sum) / $Rate
    PrivateMb   = $b.PrivateBytes / 1MB
    Seconds     = ($b.At - $a.At) / [Diagnostics.Stopwatch]::Frequency
  }
}
# What the window asked of the server and the longest task, from metrics
function Get-Asked($m) {
  if (-not $m) { return [pscustomobject]@{ GetLines = $null; LinesKb = $null; Prompts = $null; LongestTaskMs = $null; Pass = $null } }
  $gl = @($m.mux | Where-Object { $_.pdu -eq 'GetLines' -or $_.name -eq 'GetLines' })[0]
  $glr = @($m.mux | Where-Object { $_.pdu -eq 'GetLinesResponse' -or $_.name -eq 'GetLinesResponse' })[0]
  $pr = @($m.mux | Where-Object { ($_.pdu, $_.name) -match 'PromptRows' })
  $passes = @($m.paints | Where-Object { ($_.name, $_.site) -match '/pass$' })
  [pscustomobject]@{
    GetLines      = [int]$(if ($gl) { $gl.sent } else { 0 })
    LinesKb       = [math]::Round($(if ($glr) { $glr.received_bytes / 1KB } else { 0 }), 1)
    Prompts       = [int](($pr | Measure-Object sent -Sum).Sum)
    LongestTaskMs = [math]::Round((@($m.tasks | ForEach-Object max_ms) + 0 | Measure-Object -Maximum).Maximum, 2)
    Pass          = $(if ($passes) { [pscustomobject]@{ Count = ($passes | Measure-Object count -Sum).Sum; Ms = [math]::Round(($passes | Measure-Object total_ms -Sum).Sum, 1); MaxMs = [math]::Round(($passes | Measure-Object max_ms -Maximum).Maximum, 2) } })
  }
}

$Expected = [int][math]::Floor($Lines / $Every)
$Cost = [ordered]@{ lines = $Lines; every = $Every; mux = $Mux; build = $null }

# Opens tab $Key and follows its marks for 15 s or until it has $Want and nothing more comes: the
# time to its first frame and to the first with $Want marks (else the marks' last change), and
# what the look cost
function Invoke-Look($w, [string]$Key, [int]$Want) {
  Start-Sleep -Milliseconds 300
  Get-MetricsOrNot $w -Reset | Out-Null
  $c0 = Get-Cpu $w
  $rec = Start-FrameRecorder -Window $w
  $queue = [Collections.Generic.List[object]]::new()
  $t0 = Get-UnixMs
  Send-Key $w $Key
  $shown = Wait-Recorded -Recorder $rec -Queue $queue { param($s) $s.scroll_strip.mode -eq 'shell' -and @($s.tabs | Where-Object active)[0].index -eq ([int]$Key.Substring(1) - 2) } -Hwnd ([long]$w.Handle) -What "tab $Key open" -TimeoutSec 15
  $shownMs = $shown.TimeMs - $t0
  $last = $shown; $count = Get-PromptMarks $shown.Window
  $marksMs = if ($count -ge $Want) { $shownMs } else { $null }
  $changedMs = $shownMs
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt 15) {
    $f = try { Wait-Recorded -Recorder $rec -Queue $queue { param($s) $true } -Hwnd ([long]$w.Handle) -AfterFrame $last.Frame -TimeoutSec 1.5 -What 'a frame' } catch { $null }
    if (-not $f) { if ($count -ge $Want) { break } else { continue } }
    $n = Get-PromptMarks $f.Window
    if ($n -ne $count) { $count = $n; $changedMs = $f.TimeMs - $t0 }
    if ($null -eq $marksMs -and $count -ge $Want) { $marksMs = $f.TimeMs - $t0 }
    $last = $f
  }
  if ($null -eq $marksMs) { $marksMs = $changedMs }
  $rec.Dispose()
  Wait-Settled -Window $w -TimeoutSec 20 | Out-Null
  Start-Sleep -Milliseconds 500
  $c1 = Get-Cpu $w
  $m1 = Get-MetricsOrNot $w
  $span = Get-Span $w $c0 $c1
  [pscustomobject]@{
    ShownMs = [math]::Round($shownMs, 1); MarksMs = [math]::Round($marksMs, 1); Marks = $count; Want = $Want
    GuiThreadMs = [math]::Round($span.GuiThreadMs, 1); GuiMs = [math]::Round($span.GuiMs, 1); ServerMs = [math]::Round($span.ServerMs, 1)
    PrivateMbBefore = [math]::Round($c0.PrivateBytes / 1MB, 1); PrivateMbAfter = [math]::Round($c1.PrivateBytes / 1MB, 1)
    Asked = Get-Asked $m1
  }
}

# Tab $Key prints for -Seconds: what a frame costs
function Invoke-Output($w, [string]$Key) {
  Send-Key $w $Key
  Start-Sleep -Milliseconds 800
  Wait-Settled -Window $w -TimeoutSec 15 | Out-Null
  $marks0 = Get-PromptMarks (Get-Win $w)
  Send-Key $w G
  Start-Sleep -Milliseconds 1000
  $f0 = (Get-Win $w).frame
  $m0 = Get-MetricsOrNot $w -Reset
  $c0 = Get-Cpu $w
  Start-Sleep -Milliseconds ([int]($Seconds * 1000))
  $c1 = Get-Cpu $w
  $m1 = Get-MetricsOrNot $w
  $sw = Get-Win $w
  $frames = $sw.frame - $f0
  Send-Key $w S
  Start-Sleep -Milliseconds 600
  Wait-Settled -Window $w -TimeoutSec 15 | Out-Null
  $marks1 = Get-PromptMarks (Get-Win $w)
  $span = Get-Span $w $c0 $c1
  [pscustomobject]@{
    Frames = $frames; FramesPerSecond = [math]::Round($frames / $span.Seconds, 1)
    GuiThreadMsPerSecond = [math]::Round($span.GuiThreadMs / $span.Seconds, 1)
    GuiThreadMsPerFrame = [math]::Round($span.GuiThreadMs / [math]::Max(1, $frames), 3)
    ServerMsPerSecond = [math]::Round($span.ServerMs / $span.Seconds, 1)
    MarksBefore = $marks0; MarksAfter = $marks1
    Asked = Get-Asked $m1
  }
}

Invoke-Test {
  $w = Open-Window
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  $doneB = Join-Path $T.Out 'b.done'; $doneC = Join-Path $T.Out 'c.done'
  $doneD = Join-Path $T.Out 'd.done'; $goD = Join-Path $T.Out 'd.go'
  Remove-Item $doneD, $goD -ErrorAction SilentlyContinue
  $b = New-Tab -From $first -NoActivate -Program (New-LogProgram $Lines $Every $doneB) -Server:$Mux
  $c = New-Tab -From $first -NoActivate -Program (New-LogProgram 100 $Every $doneC) -Server:$Mux
  $d = New-Tab -From $first -NoActivate -Program (New-LogProgram $Lines $Every $doneD $goD) -Server:$Mux
  if (-not (Wait-Until { (Test-Path $doneB) -and (Test-Path $doneC) } -TimeoutSec 120 -IntervalMs 250)) { throw "tab B's $Lines lines weren't printed within 120 s" }
  Wait-Settled -Window $w -TimeoutSec 20 | Out-Null
  Start-Sleep -Seconds 2
  $m = Get-MetricsOrNot $w
  $Cost.build = if ($m) { $m.build } else { 'no metrics' }

  # 1. First look at B
  $look = Invoke-Look $w 'F3' $Expected
  $Cost.first_look = $look
  "first look at $Lines rows: shown in $($look.ShownMs) ms, $($look.Marks) of $Expected marks by $($look.MarksMs) ms; GUI thread $($look.GuiThreadMs) ms, server $($look.ServerMs) ms, GetLines $($look.Asked.GetLines) ($($look.Asked.LinesKb) KB), prompt-row asks $($look.Asked.Prompts), longest task $($look.Asked.LongestTaskMs) ms; private $($look.PrivateMbBefore) -> $($look.PrivateMbAfter) MB"

  # 2. Output, with 50,000 rows behind and with 100
  $long = Invoke-Output $w 'F3'
  $short = Invoke-Output $w 'F4'
  $Cost.output_long = $long
  $Cost.output_short = $short
  foreach ($o in @(@('long', $long), @('short', $short))) {
    $n, $x = $o
    "output, $n history: $($x.FramesPerSecond) frames/s, GUI thread $($x.GuiThreadMsPerSecond) ms/s ($($x.GuiThreadMsPerFrame) ms a frame), server $($x.ServerMsPerSecond) ms/s, GetLines $($x.Asked.GetLines) ($($x.Asked.LinesKb) KB), prompt-row asks $($x.Asked.Prompts), marks $($x.MarksBefore) -> $($x.MarksAfter)"
  }
  $extra = $long.GuiThreadMsPerFrame - $short.GuiThreadMsPerFrame
  $Cost.extra_ms_per_frame = [math]::Round($extra, 3)
  "a frame with $Lines rows behind it costs $([math]::Round($extra, 3)) ms more of the GUI thread than with 100"

  # 3. A new width: every row read again
  Send-Key $w F3
  Wait-Settled -Window $w -TimeoutSec 15 | Out-Null
  Start-Sleep -Milliseconds 500
  $before = Get-PromptMarks (Get-Win $w)
  $cols = @((Get-Win $w).panes)[0].cols
  $r = Get-WindowRect $w
  Set-WindowSize $w ($r.Right - $r.Left + 240) ($r.Bottom - $r.Top)
  if (-not (Wait-Until { @((Get-Win $w).panes)[0].cols -gt $cols } -TimeoutSec 10)) { throw "the window made wider kept $cols columns" }
  Start-Sleep -Seconds 2
  Wait-Settled -Window $w -TimeoutSec 15 | Out-Null
  $wider = Get-PromptMarks (Get-Win $w)
  $Cost.wider = [ordered]@{ cols_before = $cols; cols = @((Get-Win $w).panes)[0].cols; marks_before = $before; marks = $wider }
  "made wider ($cols to $($Cost.wider.cols) columns): $wider of the $before marks B had"

  # 4. A new GUI attaching to the same tabs
  if ($Mux) {
    $had = $wider
    Send-Key $w F2
    Wait-Settled -Window $w -TimeoutSec 15 | Out-Null
    Stop-TestWindow $w
    Start-Sleep -Seconds 1
    $w = Open-Window
    Wait-Settled -Window $w -TimeoutSec 20 | Out-Null
    Start-Sleep -Seconds 2
    $again = Invoke-Look $w 'F3' $had
    $Cost.attach = $again
    $Cost.attach_had = $had
    "attached again: $($again.Marks) of the $had marks B had, by $($again.MarksMs) ms; GUI thread $($again.GuiThreadMs) ms, server $($again.ServerMs) ms, GetLines $($again.Asked.GetLines) ($($again.Asked.LinesKb) KB), prompt-row asks $($again.Asked.Prompts), longest task $($again.Asked.LongestTaskMs) ms; private $($again.PrivateMbBefore) -> $($again.PrivateMbAfter) MB"
  }

  # 5. D looked at, left, printing in the background, looked at again
  $seen = Invoke-Look $w 'F5' 1
  Send-Key $w F2
  Wait-Settled -Window $w -TimeoutSec 15 | Out-Null
  Set-Content $goD 'go'
  if (-not (Wait-Until { Test-Path $doneD } -TimeoutSec 120 -IntervalMs 250)) { throw "tab D's $Lines lines weren't printed within 120 s" }
  Wait-Settled -Window $w -TimeoutSec 20 | Out-Null
  Start-Sleep -Seconds 1
  $back = Invoke-Look $w 'F5' ($seen.Marks + $Expected)
  $Cost.background = [ordered]@{ first = $seen; back = $back; want = $seen.Marks + $Expected }
  "looked at after $Lines rows printed in the background: shown in $($back.ShownMs) ms, $($back.Marks) of $($seen.Marks + $Expected) marks by $($back.MarksMs) ms; GUI thread $($back.GuiThreadMs) ms, server $($back.ServerMs) ms, prompt-row asks $($back.Asked.Prompts), longest task $($back.Asked.LongestTaskMs) ms, longest paint pass $(if ($back.Asked.Pass) { $back.Asked.Pass.MaxMs }) ms"

  $Cost | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'cost.json') -Encoding utf8
  if ($NoChecks) { return }
  if ($look.Marks -lt $Expected) { Add-Failure "the first look at $Lines rows marked $($look.Marks) of its $Expected prompts" }
  elseif ($look.MarksMs -gt $ShowMs) { Add-Failure "the first look's $Expected marks took $($look.MarksMs) ms to show, over $ShowMs" }
  foreach ($l in @(@('the first look', $look), @('attaching again', $again), @('the look after output in the background', $back))) {
    $name, $x = $l
    if (-not $x -or $null -eq $x.Asked.LongestTaskMs) { continue }
    $held = [math]::Max($x.Asked.LongestTaskMs, $(if ($x.Asked.Pass) { $x.Asked.Pass.MaxMs } else { 0 }))
    if ($held -gt $HoldMs) { Add-Failure "$name held the GUI thread $held ms in one go (a frame or a task), over $HoldMs" }
  }
  if ($null -ne $look.Asked.LinesKb -and $look.Asked.LinesKb -gt $MaxFetchKb) { Add-Failure "the first look asked the server for $($look.Asked.LinesKb) KB of lines ($($look.Asked.GetLines) GetLines), over $MaxFetchKb" }
  if ($wider -lt $before) { Add-Failure "the window made wider marked $wider of B's $before prompts" }
  if ($back.Marks -lt $Cost.background.want) { Add-Failure "the look after $Lines rows printed in the background marked $($back.Marks) of D's $($Cost.background.want) prompts" }
  if ($long.MarksAfter -le $long.MarksBefore) { Add-Failure "the output's new prompts weren't marked: $($long.MarksBefore) marks before, $($long.MarksAfter) after" }
  if ($extra -gt $FrameMs) { Add-Failure "a frame with $Lines rows behind it cost the GUI thread $([math]::Round($extra, 3)) ms more than with 100, over $FrameMs" }
  if ($null -ne $long.Asked.LinesKb -and $long.Asked.LinesKb -gt $MaxFetchKb) { Add-Failure "the output asked the server for $($long.Asked.LinesKb) KB of lines, over $MaxFetchKb" }
  if ($Mux) {
    if ($again.Marks -lt $Cost.attach_had) { Add-Failure "a new GUI attaching marked $($again.Marks) of B's $($Cost.attach_had) prompts" }
    if ($null -ne $again.Asked.LinesKb -and $again.Asked.LinesKb -gt $MaxFetchKb) { Add-Failure "attaching again asked the server for $($again.Asked.LinesKb) KB of lines, over $MaxFetchKb" }
  }
} -Passed "shell-marks: every prompt of $Lines rows marked on a first look$(if ($Mux) { ', on attaching again' }) and after printing in the background, new ones as they come, a frame costing the same with them behind it as with 100, and no scrollback asked for"
