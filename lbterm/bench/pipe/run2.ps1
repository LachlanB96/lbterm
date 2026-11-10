#Requires -Version 7
# One test window: -Rounds measured rounds of -File through the producer, then (-Sample) one more
# round with the busiest threads sampled. Writes rounds.jsonl and the samples into runs\<Name>.
param(
  [Parameter(Mandatory)][string]$Bin,
  [Parameter(Mandatory)][string]$Name,
  [string]$Tag,
  [switch]$Mux,
  [ValidateSet('bench', 'lachlan')][string]$Config = 'bench',
  [string]$File = 'plain',
  [int]$Rounds = 2,
  [switch]$Sample,
  [int]$Hz = 1000,
  [int]$SampleThreads = 5,
  [int]$Cols = 100,
  [int]$Rows = 30,
  [string]$SampleFile = 'plain'
)
$ErrorActionPreference = 'Stop'
$Here = $PSScriptRoot
Import-Module (Join-Path $PSScriptRoot '..\..\harness.psm1') -Force -DisableNameChecking
Add-Type -Path (Join-Path $Here 'data\Probe2.dll')
$Out = Join-Path $Here "runs\$Name"
$T = Start-Test "pipe-$Name" -Bin $Bin -Out $Out
$Hs = Join-Path $Out 'hs'; New-Item -ItemType Directory -Force $Hs | Out-Null
$Cfg = Join-Path $Out 'config\cfg.lua'; New-Item -ItemType Directory -Force (Split-Path $Cfg) | Out-Null
$HomeDir = Join-Path $Out 'home'
$Data = Join-Path $Here 'data'
$rate = Get-CycleRate
$roundNames = @(@(1..$Rounds | ForEach-Object { "$File-$_" }) + @(if ($Sample) { "$SampleFile-$($Rounds + 1)" }))
$lp = { param($s) $s -replace '\\', '/' }
$prog = @('C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-File', (& $lp (Join-Path $Here 'producer.ps1')), '-Dir', (& $lp $Hs), '-Data', (& $lp $Data), '-Rounds', ($roundNames -join ','))
$progLua = '{ ' + (($prog | ForEach-Object { "'$_'" }) -join ', ') + ' }'
$domainLua = "{ { name = 'pipemux', socket_path = '$(& $lp $T.Sock)', serve_command = { '$(& $lp $T.MuxServer)', '--config-file', '$(& $lp $Cfg)', '--daemonize' } } }"
if ($Config -eq 'bench') {
  [IO.File]::WriteAllText($Cfg, @"
local wezterm = require 'wezterm'
return {
  font = wezterm.font('Cascadia Mono'),
  font_size = 12.0,
  initial_cols = $Cols,
  initial_rows = $Rows,
  scrollback_lines = 9001,
  front_end = 'WebGpu',
  check_for_updates = false,
  automatically_reload_config = false,
  wsl_domains = {},
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  default_prog = $progLua,
  default_cwd = '$(& $lp $Out)',
  unix_domains = $(if ($Mux) { $domainLua } else { '{}' }),
}
"@)
} else {
  New-TestConfig -Path $Cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.window_close_confirmation = 'NeverPrompt'
config.default_cwd = [[$Out]]
config.default_prog = $progLua
rawset(config, 'lbterm_projects', nil)
$(if ($Mux) { "config.unix_domains = $domainLua" })
"@
}

$results = [Collections.Generic.List[object]]::new()
function Get-Pids($w) {
  $servers = @(Get-TestServers | ForEach-Object { [int]$_.ProcessId })
  $conpty = @(Get-ConptyIds -Parents (@($w.Pid) + $servers))
  @{ Servers = $servers; Conpty = $conpty }
}
function Get-TreeCycles($w, $p) {
  $c = [PipeProbe.Native]::ProcessCycles([uint32]$w.Pid)
  foreach ($id in @($p.Servers) + @($p.Conpty)) { $x = [PipeProbe.Native]::ProcessCycles([uint32]$id); if ($x -gt 0) { $c += $x } }
  $c
}
# Until the tree spends under 2% of a core over 500 ms
function Wait-TreeQuiet($w, $p, [double]$TimeoutSec = 90) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $hist = [Collections.Generic.List[object]]::new()
  while ($clock.Elapsed.TotalSeconds -lt $TimeoutSec) {
    $hist.Add([pscustomobject]@{ T = $clock.Elapsed.TotalMilliseconds; C = (Get-TreeCycles $w $p) })
    $old = @($hist | Where-Object { $_.T -le $hist[-1].T - 500 }) | Select-Object -Last 1
    if ($old) { $pct = ($hist[-1].C - $old.C) / $rate / ($hist[-1].T - $old.T) * 100; if ($pct -lt 2) { return [Math]::Round($clock.Elapsed.TotalMilliseconds) } }
    Start-Sleep -Milliseconds 100
  }
  -1
}
function Get-Delta($a, $b) {
  $ms = { param($c) [Math]::Round($c / $rate, 1) }
  $gt = @{}; foreach ($t in $a.ThreadList) { $gt[$t.Tid] = $t }
  $threads = @(foreach ($t in $b.ThreadList) { $c0 = if ($gt.ContainsKey($t.Tid)) { $gt[$t.Tid].Cycles } else { 0 }; [pscustomobject]@{ proc = 'gui'; tid = $t.Tid; name = $t.Name; ms = (& $ms ($t.Cycles - $c0)) } })
  $st = @{}; foreach ($t in $a.ServerThreads) { $st["$($t.Pid)/$($t.Tid)"] = $t }
  $sthreads = @(foreach ($t in $b.ServerThreads) { $k = "$($t.Pid)/$($t.Tid)"; $c0 = if ($st.ContainsKey($k)) { $st[$k].Cycles } else { 0 }; [pscustomobject]@{ proc = 'server'; tid = $t.Tid; name = $t.Name; ms = (& $ms ($t.Cycles - $c0)) } })
  $srv = 0; foreach ($s in $b.Servers) { $s0 = @($a.Servers | Where-Object Pid -eq $s.Pid)[0]; if ($s0 -and $s.Cycles -gt 0) { $srv += $s.Cycles - $s0.Cycles } }
  $con = 0; foreach ($s in $b.Conpty) { $s0 = @($a.Conpty | Where-Object Pid -eq $s.Pid)[0]; if ($s0 -and $s.Cycles -gt 0) { $con += $s.Cycles - $s0.Cycles } }
  [ordered]@{
    gui_ms = (& $ms ($b.Cycles - $a.Cycles)); gui_thread_ms = (& $ms ($b.ThreadCycles - $a.ThreadCycles)); server_ms = (& $ms $srv); conpty_ms = (& $ms $con)
    threads = @(@($threads) + @($sthreads) | Where-Object { $_.ms -gt 1 } | Sort-Object ms -Descending | Select-Object -First 14)
  }
}
function Get-MetricsSummary($m) {
  if (-not $m) { return $null }
  $t = $m.totals
  [ordered]@{
    span_ms = $m.span_ms; frames = $t.frames; unasked = $t.unasked; requests = $t.requests; tasks = $t.tasks; task_ms = $t.task_ms; messages = $t.messages; message_ms = $t.message_ms
    paints = $t.paints; paint_ms = $t.paint_ms; timers = $t.timers_fired; wakes = $t.wakes; pdus_sent = $t.pdus_sent; pdus_received = $t.pdus_received
    gui_busy_ms = $m.gui.busy_ms; gui_waited_ms = $m.gui.waited_ms
    paint_parts = @($m.paints | Select-Object -First 6 | ForEach-Object { "$($_.site) $($_.count) $($_.total_ms) ms max $($_.max_ms)" })
    top_tasks = @($m.tasks | Select-Object -First 8 | ForEach-Object { "$($_.site) $($_.count) $($_.total_ms) ms max $($_.max_ms)" })
    top_messages = @($m.messages | Select-Object -First 5 | ForEach-Object { "$($_.site) $($_.count) $($_.total_ms) ms" })
    requests_by = @($m.requests | Select-Object -First 6 | ForEach-Object { "$($_.site) req $($_.requests) frames $($_.frames)" })
    mux = @($m.mux | Select-Object -First 10 | ForEach-Object { "$($_.pdu) sent $($_.sent) ($($_.sent_bytes) B) recv $($_.received) ($($_.received_bytes) B)" })
    histograms = @($m.histograms | ForEach-Object { "$($_.name) [$($_.unit)] n=$($_.count) mean=$($_.mean) p50=$($_.p50) p95=$($_.p95) max=$($_.max)" })
    counters = @($m.upstream_counters | Select-Object -First 20 | ForEach-Object { "$($_.name)=$($_.value)" })
    caches = $m.caches
  }
}

Invoke-Test {
  $open = @{ ConfigFile = $Cfg; TimeoutSec = 40 }
  if ($Config -eq 'lachlan') { $open.UserProfile = $HomeDir }
  if ($Mux) { $open.Domain = 'pipemux'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  $ready = Join-Path $Hs 'ready.json'
  if (-not (Wait-Until { Test-Path $ready } -TimeoutSec 90 -IntervalMs 200)) { throw "the producer never started: $(Get-Content -Raw (Join-Path $Hs 'error.txt') -ErrorAction SilentlyContinue)" }
  Start-Sleep -Milliseconds 300
  $console = (Get-Content -Raw $ready | ConvertFrom-Json).console
  $p = Get-Pids $w
  "window $($w.Pid), servers $($p.Servers -join ','), conpty $($p.Conpty -join ','), console $($console.cols)x$($console.rows)"
  $hasMetrics = $true
  $prevDeltas = @()
  for ($i = 0; $i -lt $roundNames.Count; $i++) {
    $rn = $roundNames[$i]
    $sampled = $Sample -and $i -eq $roundNames.Count - 1
    $sampler = $null
    if ($sampled) {
      # the busiest threads of the rounds before, the GUI thread always among them; started early so the PDB loads before the round
      $pick = @($prevDeltas | Sort-Object ms -Descending | Select-Object -First $SampleThreads)
      $gtid = Get-GuiThreadId $w
      if (-not ($pick | Where-Object { $_.proc -eq 'gui' -and $_.tid -eq $gtid })) { $pick += [pscustomobject]@{ proc = 'gui'; tid = $gtid; name = 'main'; ms = 0 } }
      $pids = @($pick | ForEach-Object { if ($_.proc -eq 'gui') { [uint32]$w.Pid } else { [uint32]$p.Servers[0] } })
      $sampler = [PipeProbe.Sampler]::Start([uint32[]]$pids, [uint32[]]@($pick | ForEach-Object { [uint32]$_.tid }), [string[]]@($pick | ForEach-Object { "$($_.proc) $($_.tid) $($_.name)" }), $Hz, $T.Bin, 64)
      Start-Sleep -Seconds 4
    }
    $q0 = Wait-TreeQuiet $w $p
    $load = @(Get-Process | Where-Object { $_.ProcessName -match '^(cargo|rustc|link|rust-lld|lbterm-gui|lbterm-mux-server|cl)$' } | Group-Object ProcessName | ForEach-Object { "$($_.Name) x$($_.Count)" }) -join ', '
    if ($hasMetrics) { try { Get-Metrics -Window $w -Reset | Out-Null } catch { $hasMetrics = $false } }
    $f0 = (Read-State -Window $w).frame
    $s0 = Get-CpuSnapshot -Window $w -Servers $p.Servers -Conpty $p.Conpty
    $marker = "pipe-done-$rn-$([guid]::NewGuid().ToString('N').Substring(0, 6))"
    $watch = [PipeProbe.TitleWatch]::Start([long]$w.Handle, $marker, 600000)
    if ($sampler) { $sampler.Recording = $true }
    $allPids = [uint32[]]@(@($w.Pid) + @($p.Servers) + @($p.Conpty) | ForEach-Object { [uint32]$_ })
    $x0 = [PipeProbe.SysInfo]::Threads($allPids); $io0 = @{}; foreach ($q in $allPids) { $io0[$q] = [PipeProbe.SysInfo]::Io($q) }
    [IO.File]::WriteAllText((Join-Path $Hs "go-$rn"), $marker)
    $done = Join-Path $Hs "done-$rn.json"
    if (-not (Wait-Until { Test-Path $done } -TimeoutSec 600 -IntervalMs 20)) { throw "the producer didn't finish $rn" }
    $seen = $watch.Wait(600000)
    if ($sampler) { $sampler.Stop() }
    $x1 = [PipeProbe.SysInfo]::Threads($allPids); $io1 = @{}; foreach ($q in $allPids) { $io1[$q] = [PipeProbe.SysInfo]::Io($q) }
    $s1 = Get-CpuSnapshot -Window $w -Servers $p.Servers -Conpty $p.Conpty
    $m1 = if ($hasMetrics) { Get-Metrics -Window $w } else { $null }
    $f1 = (Read-State -Window $w).frame
    if (-not $seen) { throw "the title never showed $marker" }
    $q1 = Wait-TreeQuiet $w $p
    $s2 = Get-CpuSnapshot -Window $w -Servers $p.Servers -Conpty $p.Conpty
    $d = Get-Content -Raw $done | ConvertFrom-Json
    $run = Get-Delta $s0 $s1
    $tail = Get-Delta $s1 $s2
    if (-not $sampled) { $prevDeltas = $run.threads }
    $shown = ($seen - $d.start) / 1e7
    $csw0 = @{}; foreach ($r0 in $x0) { $csw0["$($r0[0])/$($r0[1])"] = $r0[2] }
    $csw = @(foreach ($r1 in $x1) { $k = "$($r1[0])/$($r1[1])"; $b0 = if ($csw0.ContainsKey($k)) { $csw0[$k] } else { 0 }; $dd = $r1[2] - $b0; if ($dd -gt 20) { [pscustomobject]@{ pid = $r1[0]; tid = $r1[1]; csw = $dd } } })
    foreach ($th in $run.threads) { $m = @($csw | Where-Object tid -eq $th.tid)[0]; $th | Add-Member -NotePropertyName csw -NotePropertyValue $(if ($m) { $m.csw } else { 0 }) -Force }
    $kind = { param($q) if ($q -eq $w.Pid) { 'gui' } elseif ($q -in $p.Servers) { 'server' } else { 'conpty' } }
    $io = @(foreach ($q in $allPids) { if ($io0[$q] -and $io1[$q]) { $a = $io0[$q]; $b = $io1[$q]; [ordered]@{ proc = (& $kind $q); pid = $q; read_ops = $b[0] - $a[0]; write_ops = $b[1] - $a[1]; other_ops = $b[2] - $a[2]; read_bytes = $b[3] - $a[3]; write_bytes = $b[4] - $a[4]; other_bytes = $b[5] - $a[5] } } })
    $cswByProc = @(foreach ($q in $allPids) { [ordered]@{ proc = (& $kind $q); pid = $q; csw = (($csw | Where-Object pid -eq $q | Measure-Object csw -Sum).Sum) } })
    $row = [ordered]@{
      name = $Name; tag = $Tag; mux = [bool]$Mux; config = $Config; round = $rn; sampled = $sampled; at = (Get-Date -Format 'HH:mm:ss'); load = $load
      bytes = $d.bytes; writes = $d.writes; writes_s = [Math]::Round($d.seconds, 3); shown_s = [Math]::Round($shown, 3); behind_s = [Math]::Round(($seen - $d.end) / 1e7, 3)
      mb_taken = [Math]::Round($d.bytes / 1MB / $d.seconds, 2); mb_shown = [Math]::Round($d.bytes / 1MB / $shown, 2)
      frames = $f1 - $f0; quiet_before_ms = $q0; tail_ms = $q1; io = $io; csw = $cswByProc; csw_threads = @($csw | Sort-Object csw -Descending | Select-Object -First 12)
      run = $run; tail = [ordered]@{ gui_ms = $tail.gui_ms; gui_thread_ms = $tail.gui_thread_ms; server_ms = $tail.server_ms; conpty_ms = $tail.conpty_ms }
      cpu_s_per_mb = [Math]::Round(($run.gui_ms + $run.server_ms + $run.conpty_ms) / 1000 / ($d.bytes / 1MB), 4)
      metrics = (Get-MetricsSummary $m1)
    }
    if ($sampler) {
      $row.sample = [ordered]@{ hz = $Hz; elapsed_ms = $sampler.ElapsedMs; recorded_ms = $sampler.RecordedMs; walks = $sampler.Walks; walk_ms = [Math]::Round($sampler.WalkMs, 1); ctx_ms = [Math]::Round($sampler.CtxMs, 1); frames_walked = $sampler.Frames; threads = @() }
      foreach ($tg in $sampler.Targets) {
        $f = $sampler.Folded($tg)
        $file = Join-Path $Out ("sample-{0}.folded" -f ($tg.Label -replace '[^\w]+', '-'))
        $sb = [Text.StringBuilder]::new()
        foreach ($k in $f.Keys) { [void]$sb.Append("$k`t$($f[$k][0])`t$($f[$k][1])`n") }
        [IO.File]::WriteAllText($file, $sb.ToString())
        $row.sample.threads += [ordered]@{ label = $tg.Label; samples = $tg.Stacks.Count; skipped = $tg.Skipped; file = $file }
      }
      $sampler.Done()
    }
    $results.Add($row)
    Add-Content (Join-Path $Out 'rounds.jsonl') ($row | ConvertTo-Json -Compress -Depth 8)
    "{0} {1}: {2} MB/s shown ({3} taken), {4} s; gui {5} ms (thread {6}), server {7}, conpty {8}; frames {9}; {10} s/MB" -f $Name, $rn, $row.mb_shown, $row.mb_taken, $row.shown_s, $run.gui_ms, $run.gui_thread_ms, $run.server_ms, $run.conpty_ms, $row.frames, $row.cpu_s_per_mb
  }
} -Passed "pipe $Name done"
