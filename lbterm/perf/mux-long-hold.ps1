#Requires -Version 7
# Covers: term/src/lbterm_changes.rs, term/src/screen.rs, mux/src/lbterm_push_read.rs, wezterm-mux-server-impl/src/lbterm_flow.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-gui/src/termwindow/frame_wait.rs, lbterm-extras/src/frame_wait.rs
<#
The push that ends a long hold (lbterm/MUX.md, "Tabs out of view: output held at the server",
"What the server holds"; perf/hidden-output's review, finding 10): a tab out of view prints more
batches than its screen keeps notes of where its viewport's top was (term/src/lbterm_changes.rs,
KEPT, 256), so the push after the hold reads every line's sequence number in the scrollback once,
-Scrollback lines (50,000 is Lachlan's), where a push after a shorter hold reads only the rows
below the oldest note it needs. What that push costs the server's main thread, and what it costs a
switch to the tab, whose first frames keep the tab before's panes until it comes (Lachlan's 1a and
2b, bounded at 50 ms, config::lbterm::SWITCH_WAIT).

One window on a test mux server of its own (a socket under the test's folder; never Lachlan's),
-Scrollback lines kept, two tabs built on the server (PITFALLS 202): a quiet one in view, and a
printer out of view that prints when the script says, -BatchLines lines a write with -BatchGapMs
between writes so that each write is a parser batch of its own, then its trial's line and its
cursor at a place of the trial's own, and writes when it did. First it fills its scrollback (the
fill, whose push after its hold is reported too). Then -Trials times each:
1. quiet-long: -LongBatches writes out of view, then quiet: the server's push once the tab has been
   quiet for config::lbterm::HELD_QUIET (250 ms), alone in its second.
2. switch-long: -LongBatches writes out of view, and the tab brought into view by a key (F2,
   ActivateTab) as soon as it says it's done, within the quiet span, so the switch asks for the
   push: the key to the first frame drawing the tab, whether that frame drew its last screen (its
   cursor), and the window's frame waits (the counters' frame_wait). Then back by F1.
3. switch-short: the same with -ShortBatches writes, fewer than the notes kept, for comparison.
Each trial's figures: the print's span and the server's main thread over it (cycles over the
clock's span), the printer's pushes from the server's push log (`lbterm::push`, its line for the
pane over the trial: `pushed`, `rows`, `stale`, `us` and the `reads` step, microseconds), and the
server's main thread from the print's end to a second and a half after (the push, alone).

A build that holds nothing (main's, the installed one) pushes the printer as it prints, so its
print span carries the pushes and its switch waits for nothing: the same script measures both.
Judged: every switch's tab drawn, its last screen within -SettleMs of the key, and on a build that
holds, the push after every long hold and switch made (a pushed count in the log). -Measure judges
nothing on cost (nothing else is judged on cost either: this is a measurement).

mux-long-hold.json in the output folder has every number.

  pwsh -File lbterm\perf\mux-long-hold.ps1 [-Trials 3] [-Scrollback 50000] [-LongBatches 800] [-ShortBatches 60] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [int]$Trials = 3,
  [int]$Scrollback = 50000,
  [int]$LongBatches = 800,
  [int]$ShortBatches = 60,
  [int]$BatchLines = 10,
  [int]$BatchGapMs = 8,
  [int]$Cols = 120,
  [int]$Rows = 40,
  [int]$SettleMs = 2000,
  [double]$MaxMemGB = 3,
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-long-hold' -Bin $Bin
$rate = Get-CycleRate

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermHold {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  public static ulong ThreadCycles(int tid) {
    IntPtr h = OpenThread(0x0800, false, (uint)tid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryThreadCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  // The process's first thread, its main one
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      int best = 0; DateTime first = DateTime.MaxValue;
      foreach (ProcessThread t in p.Threads) {
        try { if (t.StartTime < first) { first = t.StartTime; best = t.Id; } } catch (Exception) { }
      }
      return best;
    }
  }
}
'@

$Dir = $T.Out
# The printer: each go-<k> file says "<writes> <lines a write> <ms between>"; it prints them, then
# its trial's line with its cursor at a place of the trial's own, and writes done-<k> with when
$Printer = New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27; $bel = [char]7
[Console]::Out.Write("$e]2;hold-printer$bel")
$sb = [Text.StringBuilder]::new()
$n = 0; $k = 0
while ($true) {
  $k++
  $go = "XDIRX\go-$k"
  while (-not (Test-Path $go)) { Start-Sleep -Milliseconds 10 }
  $spec = ([IO.File]::ReadAllText($go)).Trim() -split ' '
  $writes = [int]$spec[0]; $lines = [int]$spec[1]; $gap = [int]$spec[2]
  for ($b = 0; $b -lt $writes; $b++) {
    [void]$sb.Clear()
    for ($i = 0; $i -lt $lines; $i++) { $n++; [void]$sb.Append("hold $k line $n the quick brown fox jumps over the lazy dog 0123456789`r`n") }
    [Console]::Out.Write($sb.ToString())
    if ($gap -gt 0) { Start-Sleep -Milliseconds $gap }
  }
  $row = 5 + ($k % 20); $col = 3 + ($k % 30)
  [Console]::Out.Write("hold $k end after $n lines$e[${row};${col}H")
  [IO.File]::WriteAllText("XDIRX\done-$k", "$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())")
}
'@ -replace 'XDIRX', $Dir)
$Quiet = New-PwshProgram -Script "[Console]::Out.Write([char]27 + ']2;hold-quiet' + [char]7); Start-Sleep 3600"
function Get-TrialCursor([int]$K) { @{ X = 2 + ($K % 30); Y = 4 + ($K % 20) } }

$cfg = Join-Path $T.Out 'config\mux-long-hold.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.scrollback_lines = $Scrollback
config.enable_scroll_bar = false
rawset(config, 'lbterm_scroll_strip', nil)
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F1', mods = 'NONE', action = wezterm.action.ActivateTab(0) })
table.insert(config.keys, { key = 'F2', mods = 'NONE', action = wezterm.action.ActivateTab(1) })
config.unix_domains = { {
  name = 'holdmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

# A log's text from byte $From to byte $To
function Get-LogSlice([string]$Path, [long]$From, [long]$To) {
  if (-not $Path -or -not (Test-Path -LiteralPath $Path) -or $To -le $From) { return '' }
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); $text = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  if ($text.Length -gt ($To - $From)) { $text.Substring(0, [int]($To - $From)) } else { $text }
}
function Get-LogLength([string]$Path) { if ($Path -and (Test-Path -LiteralPath $Path)) { (Get-Item -LiteralPath $Path).Length } else { 0 } }
# Pane $Rid's pushes in the server's push log between two lengths of it, its counts summed
# (lbterm_pushstats.rs's counts_text: asked, alone, pushed, rows, stale, us, held, then each step;
# a build from before the flood build says no alone, one from before perf/hidden-output no held)
function Get-PanePushes([string]$Path, [long]$From, [long]$To, [int]$Rid) {
  $r = [ordered]@{ Lines = 0; Asked = 0; Alone = 0; Pushed = 0; Rows = 0; Stale = 0; Us = 0; Held = 0; ReadsUs = 0 }
  foreach ($l in ((Get-LogSlice $Path $From $To) -split "`r?`n")) {
    if ($l -notmatch 'lbterm::push') { continue }
    if ($l -match "; pane $Rid(?: shown)? asked (\d+)(?: alone (\d+))? pushed (\d+) rows (\d+) stale (\d+) pdus \d+ bytes \d+ us (\d+)(?: held (\d+) names \d+)? reads (\d+)") {
      $r.Lines++; $r.Asked += [int]$Matches[1]; $r.Alone += [int]$Matches[2]; $r.Pushed += [int]$Matches[3]
      $r.Rows += [int]$Matches[4]; $r.Stale += [long]$Matches[5]; $r.Us += [long]$Matches[6]; $r.Held += [int]$Matches[7]; $r.ReadsUs += [long]$Matches[8]
    }
  }
  $r
}
function Get-Pct([double[]]$Values, [double]$P) {
  if ($null -eq $Values -or @($Values).Count -eq 0) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

$results = [ordered]@{ Bin = $T.Gui; Locked = (Test-WorkstationLocked); Scrollback = $Scrollback; LongBatches = $LongBatches; ShortBatches = $ShortBatches; Trials = @() }

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Domain 'holdmux' -Socket $T.Sock -TimeoutSec 30
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $sp = [int]$server[0].ProcessId
  Start-MemoryGuard -Pids $w.Pid, $sp -LimitGB $MaxMemGB
  # The tabs are built on the server, whose first tab runs its own default program (PITFALLS 202)
  $shell = $panes[0].pane_id
  $quietId = New-Tab -From $shell -Program $Quiet -Server
  $rid = New-Tab -From $shell -Program $Printer -Server
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 2 } -What 'the two tabs once the shell has gone' -TimeoutSec 30 | Out-Null
  Send-Key $w F1
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $quietId }).Count -eq 1 -and -not $s.animating }.GetNewClosure() -What 'the quiet tab in view' -TimeoutSec 15 | Out-Null
  $serverMain = [LbtermHold]::MainThread($sp)
  $serverLog = Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$sp.txt"
  $hasMetrics = $true
  try { Get-Metrics $w -Reset -TimeoutMs 4000 | Out-Null } catch { $hasMetrics = $false }
  $h = Resolve-Handle $w
  $script:k = 0

  # One trial: the printer prints $Writes writes out of view, then (-Switch) is brought into view
  # by F2 as soon as it's done, or left to go quiet; then a round of the connection (a key to the
  # tab in view, or F1 back) logs the second's pushes
  # (it prints its line and keeps its figures in $results: what a function returns, a caller that
  # keeps it takes the printed lines too)
  function Invoke-Trial([string]$Kind, [int]$Writes, [int]$Lines, [int]$Gap, [switch]$Switch) {
    # The tabs' ids as locals of its own: a closure (GetNewClosure) takes a function's own
    # variables, not those it sees in its caller's (PITFALLS 206)
    $rid = $rid; $quietId = $quietId
    $script:k++
    $tr = [ordered]@{ Kind = $Kind; Trial = $script:k; Writes = $Writes; Lines = $Writes * $Lines }
    $logAt = Get-LogLength $serverLog
    $c0 = [Diagnostics.Stopwatch]::GetTimestamp(); $s0 = [LbtermHold]::ThreadCycles($serverMain)
    [IO.File]::WriteAllText((Join-Path $Dir "go-$($script:k)"), "$Writes $Lines $Gap")
    $doneFile = Join-Path $Dir "done-$($script:k)"
    if (-not (Wait-Until { Test-Path -LiteralPath $doneFile } -TimeoutSec 180 -IntervalMs 2)) { throw "trial $($script:k) ($Kind): the printer never said it was done" }
    $c1 = [Diagnostics.Stopwatch]::GetTimestamp(); $s1 = [LbtermHold]::ThreadCycles($serverMain)
    $tr.PrintSec = [Math]::Round(($c1 - $c0) / [Diagnostics.Stopwatch]::Frequency, 2)
    $tr.PrintServerMainMs = [Math]::Round(($s1 - $s0) / $rate, 1)
    if ($Switch) {
      $want = Get-TrialCursor $script:k
      if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
      $rec = Start-FrameRecorder -Window $w
      try {
        $before = (Read-State).frame
        [void]$rec.Take()
        $t0 = Get-UnixMs
        Send-Key $w F2
        $seen = [Collections.Generic.List[object]]::new()
        $drawn = { param($sw, $s) $seen.Add($sw); @($sw.panes | Where-Object { $_.server_id -eq $rid -and $_.drawn_cursor }).Count -gt 0 }.GetNewClosure()
        $f = $null
        try { $f = Wait-Recorded -Recorder $rec $drawn -Hwnd ([long]$h) -AfterFrame $before -TimeoutSec 10 -What "trial $($script:k)'s tab drawn" } catch { }
        if ($f) {
          $tr.KeyToFrameMs = [Math]::Round($f.TimeMs - $t0, 1)
          $p = @($f.Window.panes | Where-Object server_id -eq $rid)[0]
          $tr.FirstCursor = "$($p.drawn_cursor.x),$($p.drawn_cursor.y)"
          $tr.FirstCurrent = [int]$p.drawn_cursor.x -eq $want.X -and [int]$p.drawn_cursor.y -eq $want.Y
          $tr.KeptFrames = @($seen | Where-Object { $_.PSObject.Properties.Name -contains 'panes_tab' -and $null -ne $_.active_tab -and $_.panes_tab -ne @($_.tabs)[[int]$_.active_tab].id }).Count
        }
        # Its last screen, however many frames it took
        $cur = { param($sw) @($sw.panes | Where-Object { $_.server_id -eq $rid -and $_.drawn_cursor -and [int]$_.drawn_cursor.x -eq $want.X -and [int]$_.drawn_cursor.y -eq $want.Y }).Count -gt 0 }.GetNewClosure()
        if ($tr.FirstCurrent) { $tr.KeyToCurrentMs = $tr.KeyToFrameMs }
        else {
          try { $g = Wait-Recorded -Recorder $rec $cur -Hwnd ([long]$h) -AfterFrame $before -TimeoutSec ([Math]::Max(2, $SettleMs / 1000 + 1)) -What "trial $($script:k)'s last screen"; $tr.KeyToCurrentMs = [Math]::Round($g.TimeMs - $t0, 1) } catch { }
        }
      } finally { $rec.Dispose() }
      if ($hasMetrics) { try { $tr.FrameWait = (Get-Metrics $w).frame_wait } catch { } }
      Start-Sleep -Milliseconds 1500
      $s2 = [LbtermHold]::ThreadCycles($serverMain)
      $tr.AfterServerMainMs = [Math]::Round(($s2 - $s1) / $rate, 1)
      Send-Key $w F1
      Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $quietId }).Count -eq 1 -and -not $s.animating }.GetNewClosure() -What 'the quiet tab in view again' -TimeoutSec 15 | Out-Null
    } else {
      Start-Sleep -Milliseconds 1500
      $s2 = [LbtermHold]::ThreadCycles($serverMain)
      $tr.AfterServerMainMs = [Math]::Round(($s2 - $s1) / $rate, 1)
      # A key to the quiet tab: its answer ends a round of the connection, which logs the second's pushes
      Send-Chars $w 'x'
    }
    Start-Sleep -Milliseconds 400
    $pp = Get-PanePushes $serverLog $logAt (Get-LogLength $serverLog) $rid
    foreach ($n in $pp.Keys) { $tr["Log$n"] = $pp[$n] }
    $tr.ReadsUsAPush = if ($pp.Pushed) { [Math]::Round($pp.ReadsUs / $pp.Pushed) } else { $null }
    $tr.UsAPush = if ($pp.Pushed) { [Math]::Round($pp.Us / $pp.Pushed) } else { $null }
    $script:results.Trials += [pscustomobject]$tr
    $fw = $tr.FrameWait
    "$Kind (trial $($script:k), $($tr.Lines) lines in $Writes writes, $($tr.PrintSec) s): the server's main thread $($tr.PrintServerMainMs) ms over the print and $($tr.AfterServerMainMs) ms in the 1.5 s after; the printer's pushes $($pp.Pushed) ($($pp.Alone) alone, $($pp.Held) PaneHeld), $($pp.Rows) rows and $($pp.Stale) stale, $($pp.Us) us ($($pp.ReadsUs) reading)$(if ($pp.Pushed) { ", $($tr.UsAPush) us a push ($($tr.ReadsUsAPush) reading)" })$(if ($Switch) { "; the key to its first frame $($tr.KeyToFrameMs) ms ($(if ($tr.FirstCurrent) { 'its last screen' } else { "cursor $($tr.FirstCursor), not its last screen" }), $($tr.KeptFrames) frames kept the tab before's panes), to its last screen $($tr.KeyToCurrentMs) ms$(if ($fw) { "; frame waits $($fw.waits), $($fw.pushed) ended by the push and $($fw.bounded) by the bound, $($fw.max_ms) ms at most" })" })"
    $guard = Get-MemoryGuard
    if ($guard) { throw "the memory guard stopped the test: $guard" }
  }

  # The fill: the scrollback full, the printer out of view
  $fillWrites = [int][Math]::Ceiling(($Scrollback + $Rows + 200) / 500)
  Invoke-Trial 'fill' $fillWrites 500 0
  # (not $t: PowerShell's names are case-insensitive, and $T is the test's: PITFALLS 206)
  for ($n = 1; $n -le $Trials; $n++) {
    Invoke-Trial 'quiet-long' $LongBatches $BatchLines $BatchGapMs
    Invoke-Trial 'switch-long' $LongBatches $BatchLines $BatchGapMs -Switch
    Invoke-Trial 'switch-short' $ShortBatches $BatchLines $BatchGapMs -Switch
  }
  foreach ($x in @($results.Trials | Where-Object { $_.Kind -like 'switch-*' })) {
    if ($null -eq $x.KeyToFrameMs) { Add-Failure "$($x.Kind) trial $($x.Trial): the tab was never drawn after its key" }
    elseif ($null -eq $x.KeyToCurrentMs) { Add-Failure "$($x.Kind) trial $($x.Trial): the tab's last screen (cursor $((Get-TrialCursor $x.Trial).X),$((Get-TrialCursor $x.Trial).Y)) wasn't drawn within $SettleMs ms of its key; its first frame had $($x.FirstCursor)" }
    elseif ($x.KeyToCurrentMs -gt $SettleMs) { Add-Failure "$($x.Kind) trial $($x.Trial): its last screen drawn $($x.KeyToCurrentMs) ms after the key, over $SettleMs" }
  }
  # Each kind's figures at the median of its trials
  foreach ($kind in 'quiet-long', 'switch-long', 'switch-short') {
    $ts = @($results.Trials | Where-Object Kind -eq $kind)
    $line = "$kind, the median of $($ts.Count): the server's main thread $([Math]::Round((Get-Pct @($ts | ForEach-Object { $_.PrintServerMainMs / $_.PrintSec }) 0.5), 1)) ms a second of the print, $(Get-Pct @($ts | ForEach-Object AfterServerMainMs) 0.5) ms after it; $(Get-Pct @($ts | ForEach-Object LogPushed) 0.5) pushes, $(Get-Pct @($ts | ForEach-Object LogUs) 0.5) us of them ($(Get-Pct @($ts | ForEach-Object LogReadsUs) 0.5) reading)"
    if ($kind -ne 'quiet-long') { $line += "; the key to its first frame $(Get-Pct @($ts | ForEach-Object KeyToFrameMs) 0.5) ms, to its last screen $(Get-Pct @($ts | ForEach-Object KeyToCurrentMs) 0.5) ms" }
    $line
  }
  $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-long-hold.json')
} -Passed "mux-long-hold: every switch to a tab that printed out of view drew its last screen within $SettleMs ms"
