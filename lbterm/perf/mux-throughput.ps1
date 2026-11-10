#Requires -Version 7
# Variant: -Local
# Variant: -Kind rgb
# Covers: wezterm-client/src/client.rs, wezterm-client/src/lbterm_backlog.rs, wezterm-client/src/pane/renderable.rs, wezterm-client/src/pane/clientpane.rs, window/src/spawn.rs, window/src/os/windows/connection.rs, window/src/os/windows/work.rs, wezterm-mux-server-impl/src/dispatch.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
Output at full speed in the tab in view, on a mux domain of its own or (-Local) in the window's own
tab (lbterm/PERF.md, findings 24 and 33; lbterm/MUX.md, "Output at full speed"): a writer prints
16 MB as fast as it can each time a key reaches it, -Runs times, as budgets.ps1's throughput does
(-Kind plain: lines of 104 bytes; rgb: Claude Code's look, 24-bit colours on the text and on a
diff's rows, box rules and file paths), with -Behind tabs out of view printing without a pause
beside it (a mux domain's only). For each run, from the key to the first frame with the
writer's title saying it's done: the time, the frames, the window's GUI thread and process and
the server's CPU (by their cycles). Through all the runs, how long the window's thread went
without taking a message (Start-HangWatch): Windows calls a window that goes 5 s without one not
responding, and DWM put its ghost over a test window's three times in 54 runs on 2026-10-30
(finding 33), and the longest drain of the main thread's task queue the window counted (metrics'
gui.longest_drain_ms, from builds that count it: the longest the GUI thread ran tasks without a
look at its messages).

It fails, unless -Measure, when the window's thread went -HangMs without taking a message.
mux-throughput.json in the output folder has every number.

  pwsh -File lbterm\perf\mux-throughput.ps1 [-Kind plain|rgb] [-Runs 6] [-Local] [-Behind 0] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [ValidateSet('plain', 'rgb')][string]$Kind = 'plain',
  [int]$Runs = 6,
  [switch]$Local,
  # Tabs out of view printing without a pause through every run (a mux domain's only), as
  # perf/mux-flood.ps1's: the most the GUI thread is given to apply
  [int]$Behind = 0,
  [double]$HangMs = 1000,
  [double]$MaxMemGB = 4,
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-throughput' -Bin $Bin
$rate = Get-CycleRate
$cyclesMs = { param($c) [Math]::Round($c / $rate, 1) }

# The 1 MB the writer writes 16 times a run, made once into a file it reads
function Get-Chunk {
  $sb = [Text.StringBuilder]::new()
  if ($Kind -eq 'plain') {
    $line = ' the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk'
    for ($i = 0; $i -lt 10082; $i++) { [void]$sb.Append($i.ToString().PadLeft(6)).Append($line).Append("`r`n") }
  } else {
    $e = "$([char]27)["
    $dim = "${e}38;2;153;153;153m"; $white = "${e}38;2;255;255;255m"; $border = "${e}38;2;136;136;136m"
    $green = "${e}38;2;78;186;101m"; $accent = "${e}38;2;177;185;249m"; $added = "${e}48;2;34;92;43m"; $removed = "${e}48;2;122;41;54m"
    $rule = [string]::new([char]0x2500, 96)
    for ($k = 0; $sb.Length -lt 1MB; $k++) {
      switch ($k % 8) {
        0 { [void]$sb.Append($border).Append([char]0x256D).Append($rule).Append([char]0x256E).Append("${e}39m") }
        1 { [void]$sb.Append($green).Append("$([char]0x25CF) ").Append("${e}1m").Append($white).Append('Read').Append("${e}22m").Append($dim).Append("(C:/Users/demo/project/src/module$($k % 997)/notes$k.md)").Append("${e}39m") }
        2 { [void]$sb.Append($dim).Append("  $([char]0x2514)  Read $k lines (ctrl+o to expand)").Append("${e}39m") }
        { $_ -in 3, 4 } { [void]$sb.Append($added).Append($dim).Append(($k % 99991).ToString().PadLeft(6)).Append(' ').Append($white).Append(("+   let value$k = compute(input, &options[$($k % 97)]);").PadRight(90)).Append("${e}39m${e}49m") }
        5 { [void]$sb.Append($removed).Append($dim).Append(($k % 99991).ToString().PadLeft(6)).Append(' ').Append($white).Append(("-   let value$k = compute(input, options[$($k % 97)]);").PadRight(90)).Append("${e}39m${e}49m") }
        6 { [void]$sb.Append($white).Append('The change in ').Append($accent).Append('C:/Users/demo/project/CLAUDE.md').Append($white).Append(" keeps line $k, as ${e}3mnoted${e}23m before.").Append("${e}39m") }
        7 { [void]$sb.Append($border).Append([char]0x2570).Append($rule).Append([char]0x256F).Append("${e}39m") }
      }
      [void]$sb.Append("`r`n")
    }
  }
  [Text.Encoding]::UTF8.GetBytes($sb.ToString())
}
$data = Join-Path $T.Out "$Kind.bin"
[IO.File]::WriteAllBytes($data, (Get-Chunk))

# The writer: its chunk 16 times once a key reaches it, then its title saying so (OSC 0 ended by
# BEL on the same stream). The first title a program sets doesn't reach the pane, so a title of no
# use first, then 'ready'
$Writer = New-PwshProgram -Script (@'
$e = [char]27
$chunk = [IO.File]::ReadAllBytes('XDATAX')
$out = [Console]::OpenStandardOutput()
function Title([string]$t) { $b = [Text.Encoding]::UTF8.GetBytes("$e]0;$t$([char]7)"); $out.Write($b, 0, $b.Length); $out.Flush() }
$n = 0
$b = [Text.Encoding]::UTF8.GetBytes("ready`r`n"); $out.Write($b, 0, $b.Length)
Title 'starting'
Start-Sleep -Milliseconds 200
Title 'ready'
while ($true) {
  [void][Console]::ReadKey($true)
  $n++
  $clock = [Diagnostics.Stopwatch]::StartNew()
  for ($k = 0; $k -lt 16; $k++) { $out.Write($chunk, 0, $chunk.Length) }
  $out.Flush()
  $ms = [int]$clock.Elapsed.TotalMilliseconds
  Title "wrote $n $ms $(16 * $chunk.Length)"
}
'@ -replace 'XDATAX', $data)

function Get-ActivePane { @((Get-StateWindow (Read-State) -Hwnd ([long](Resolve-Handle $script:W))).panes | Where-Object active)[0] }

function Get-MainThread([int]$ProcessId) {
  $p = Get-Process -Id $ProcessId
  $first = $null
  foreach ($t in $p.Threads) {
    try { if (-not $first -or $t.StartTime -lt $first.StartTime) { $first = $t } } catch { }
  }
  [int]$first.Id
}

function Get-Mark($w) {
  # (an `if` that gives an empty array gives $null, which Get-CpuSnapshot can't take)
  $snap = if ($w.ServerPid) { Get-CpuSnapshot $w -Servers @($w.ServerPid) } else { Get-CpuSnapshot $w }
  $main = if ($w.ServerPid) { @($snap.ServerThreads | Where-Object { $_.Tid -eq $w.ServerMain })[0] }
  [pscustomobject]@{
    At = $snap.At; Thread = $snap.ThreadCycles; Gui = $snap.Cycles
    Server = if ($w.ServerPid) { @($snap.Servers)[0].Cycles } else { 0 }; Main = if ($main) { $main.Cycles } else { 0 }
  }
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

$results = [ordered]@{ Window = $T.Gui; Locked = (Test-WorkstationLocked); Kind = $Kind; Local = [bool]$Local; Behind = $Behind; Runs = @() }

Invoke-Test {
  if ($Local) {
    $w = Open-TestWindow -Program $Writer -TimeoutSec 30
  } else {
    $cfg = Join-Path $T.Out 'config\mux-throughput.lua'
    New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
    New-TestConfig -Path $cfg -Lua @"
config.unix_domains = { {
  name = 'throughputmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
    $w = Open-TestWindow -ConfigFile $cfg -Domain 'throughputmux' -Socket $T.Sock -TimeoutSec 30
  }
  $script:W = $w
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  if (-not $Local) {
    $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
      $panes = @(Get-Panes -Server)
      if (-not $panes) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
    $server = @(Get-TestServers)
    if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
    $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue ([int]$server[0].ProcessId)
    $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue (Get-MainThread $w.ServerPid)
    $shell = $panes[0].pane_id
    $pane = New-Tab -From $shell -Program $Writer -Server
    Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
    $flood = New-PwshProgram -Script @'
$s = (('x' * 100) + "`n") * 400
while ($true) { [Console]::Out.Write($s) }
'@
    for ($i = 0; $i -lt $Behind; $i++) { New-Tab -From $pane -Program $flood -NoActivate -Server | Out-Null }
    Invoke-Cli activate-pane --pane-id $pane -Server -Check | Out-Null
    Start-MemoryGuard -Pids $w.Pid, $w.ServerPid -LimitGB $MaxMemGB
  } else {
    Start-MemoryGuard -Pids $w.Pid -LimitGB $MaxMemGB
  }
  Wait-State { param($s) $p = @($s.panes | Where-Object active)[0]; $p -and $p.title -eq 'ready' -and @($s.tabs).Count -eq 1 + $Behind } -What 'the writer ready' -TimeoutSec 30 | Out-Null
  Start-Sleep 1

  $hang = Start-HangWatch $w
  for ($n = 1; $n -le $Runs; $n++) {
    Wait-ModifiersUp | Out-Null
    Get-Metrics $w -Reset | Out-Null
    $a = Get-Mark $w
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Send-Key $w G
    $want = "^wrote $n "
    Wait-State { param($s) $p = @($s.panes | Where-Object active)[0]; $p -and $p.title -match $want }.GetNewClosure() -What "run $n's title" -TimeoutSec 120 -IntervalMs 5 | Out-Null
    $ms = $clock.Elapsed.TotalMilliseconds
    $b = Get-Mark $w
    $m = Get-Metrics $w
    # (not $t, which is $T, the test, to PowerShell)
    $title = (Get-ActivePane).title -split ' '
    $run = [ordered]@{
      Ms = [Math]::Round($ms, 0); WriterMs = [double]$title[2]; Bytes = [double]$title[3]
      MBps = [Math]::Round([double]$title[3] / 1MB / ($ms / 1000), 2)
      Frames = [int]$m.totals.frames
      GuiThreadMs = & $cyclesMs ($b.Thread - $a.Thread); GuiMs = & $cyclesMs ($b.Gui - $a.Gui)
      ServerMs = & $cyclesMs ($b.Server - $a.Server); ServerMainMs = & $cyclesMs ($b.Main - $a.Main)
      Pushes = [int](@($m.mux | Where-Object pdu -eq 'GetPaneRenderChangesResponse')[0].received)
      GetLines = [int](@($m.mux | Where-Object pdu -eq 'GetLines')[0].sent)
      LongestDrainMs = if ($null -ne $m.gui.longest_drain_ms) { [double]$m.gui.longest_drain_ms } else { $null }
      LongestTaskMs = [double](@($m.tasks | Sort-Object { [double]$_.max_ms } -Descending)[0].max_ms)
      LongestTask = [string](@($m.tasks | Sort-Object { [double]$_.max_ms } -Descending)[0].site)
      LongestMessageMs = [double](@($m.messages | Sort-Object { [double]$_.max_ms } -Descending)[0].max_ms)
    }
    $results.Runs += $run
    "run ${n}: $($run.Ms) ms ($($run.MBps) MB/s; the writer $($run.WriterMs)), $($run.Frames) frames, the GUI thread $($run.GuiThreadMs) ms (its process $($run.GuiMs)), the server $($run.ServerMs) ms (main thread $($run.ServerMainMs)), $($run.Pushes) pushes, $($run.GetLines) GetLines; the longest drain $($run.LongestDrainMs) ms, task $($run.LongestTaskMs) ms ($($run.LongestTask)), message $($run.LongestMessageMs) ms" | Out-Host
    Start-Sleep -Milliseconds 1500
  }
  $results.Hang = Stop-HangWatch $hang
  $results.LongestDrainMs = ($results.Runs | ForEach-Object LongestDrainMs | Measure-Object -Maximum).Maximum
  # (not $runs, which is -Runs to PowerShell)
  $done = $results.Runs
  $results.Median = [ordered]@{
    Ms = Get-Pct @($done | ForEach-Object Ms) 0.5; MBps = Get-Pct @($done | ForEach-Object MBps) 0.5
    Frames = Get-Pct @($done | ForEach-Object Frames) 0.5; GuiThreadMs = Get-Pct @($done | ForEach-Object GuiThreadMs) 0.5
    ServerMs = Get-Pct @($done | ForEach-Object ServerMs) 0.5
  }
  $results.LongestTaskMs = ($done | ForEach-Object LongestTaskMs | Measure-Object -Maximum).Maximum
  $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-throughput.json')
  $md = $results.Median; $h = $results.Hang
  "$(if ($Local) { 'local' } else { 'mux' }) $Kind, $Runs runs: $($md.Ms) ms at the median ($($md.MBps) MB/s), $($md.Frames) frames, the GUI thread $($md.GuiThreadMs) ms, the server $($md.ServerMs) ms; the longest wait for a message $($h.LongestMs) ms (p99 $($h.P99Ms), $($h.Over100Ms) over 100 ms, $($h.Over1s) over 1 s), the longest drain the window counted $($results.LongestDrainMs) ms, the longest task $($results.LongestTaskMs) ms"
  if (-not $Measure -and $h.LongestMs -gt $HangMs) { Add-Failure "the window's thread went $($h.LongestMs) ms without taking a message (over $HangMs)" }
} -Passed 'the window took its messages through every run'
