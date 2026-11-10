# One measured window drawn as Lachlan's (New-TestConfig), local domain: memory at rest with one
# tab, then after each step (tabs:<n>, lines:<n> of varied text, glyphs: a burst of glyphs the atlas
# hasn't seen, CJK, emoji and more, a line every 20 ms; rest:<s>), the GUI's breakdown
# (memtools.ps1) and the paints since the step before (metrics, reset at each). lbterm/PERF.md,
# "What a window's GPU memory holds" (finding 95), was measured with it. Run it as a pwsh of its own
# (Invoke-Test exits), one window at a time, builds interleaved; -Env sets the window's environment.
#
#   pwsh -File lbterm\bench\mem\gpumem.ps1 -Name r1-main -Bin <build> -OutRoot <folder> [-Steps tabs:8,lines:100000,glyphs,glyphs] [-Scan]
param(
  [Parameter(Mandatory)][string]$Name,
  [Parameter(Mandatory)][string]$Bin,
  [Parameter(Mandatory)][string]$OutRoot,
  [string[]]$Steps = @('tabs:8', 'lines:100000', 'glyphs'),
  [int]$SettleSec = 6,
  [string]$Log = '',
  [switch]$Shots,
  # Read every resident private page too, for the ones all zeros (slower: the 64 MB blocks and the
  # heap are read through)
  [switch]$Scan,
  # KEY=VALUE;KEY=VALUE for the window's environment
  [string]$Env = ''
)
$ErrorActionPreference = 'Stop'
$Steps = @($Steps | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
Import-Module (Join-Path $PSScriptRoot '..\..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot 'memtools.ps1')

$out = Join-Path $OutRoot $Name
$T = Start-Test "gpumem-$Name" -Bin $Bin -Out $out
$pwshExe = 'C:/Program Files/PowerShell/7/pwsh.exe'
$report = [ordered]@{ name = $Name; bin = $Bin; env = $Env; steps = @(); started = (Get-Date).ToString('o'); locked = [bool](Get-Process LogonUI -ErrorAction SilentlyContinue) }

function Get-Load {
  $a = Get-Process | Select-Object Id, ProcessName, @{ n = 'Cpu'; e = { $_.TotalProcessorTime.TotalMilliseconds } }
  Start-Sleep -Milliseconds 1000
  $b = @{}; Get-Process | ForEach-Object { $b[$_.Id] = $_ }
  $busy = $a | ForEach-Object {
    $p = $b[$_.Id]; if ($p) { [pscustomobject]@{ Name = "$($_.ProcessName) $($_.Id)"; Cores = [Math]::Round(($p.TotalProcessorTime.TotalMilliseconds - $_.Cpu) / 1000, 2) } }
  } | Where-Object Cores -gt 0.2 | Sort-Object Cores -Descending
  $cores = [Math]::Round((@($busy) | Measure-Object Cores -Sum).Sum, 2)
  $counts = @{}
  foreach ($n in 'lbterm-gui', 'lbterm-mux-server', 'cargo', 'rustc') { $counts[$n] = @(Get-Process -Name $n -ErrorAction SilentlyContinue).Count }
  "$cores cores busy; lbterm-gui $($counts['lbterm-gui']), mux servers $($counts['lbterm-mux-server']), cargo $($counts.cargo), rustc $($counts.rustc); top: $((@($busy) | Select-Object -First 5 | ForEach-Object { "$($_.Name) $($_.Cores)" }) -join ', ')"
}

# 100,000 varied lines (words, numbers, hex, some coloured), then the title 'fill done'
function New-FillProgram([int]$N) {
  New-PwshProgram -Script (@'
$e = [char]27
$w = [Console]::WindowWidth - 1
$r = [Random]::new(7)
$words = -split 'cache line pane tab shape glyph quad frame window rail strip fade build check merge branch commit test slot warden summary session prompt output render layout switch hop surface visual config default memory thread GUI mux server socket state file JSON lbterm term cell row column font colour cursor scroll text error warning passed failed ok the a to of and in is it for on that with'
$b = [Text.StringBuilder]::new()
for ($i = 0; $i -lt __LINES__; $i++) {
  $want = $r.Next(8, $w); $n = 0
  while ($true) {
    $k = $r.Next(10)
    $t = if ($k -lt 6) { $words[$r.Next($words.Count)] } elseif ($k -lt 8) { [string]$r.Next(0, 100000) } else { '{0:x8}' -f $r.Next() }
    if ($n + $t.Length + 1 -gt $want) { break }
    if ($r.Next(5) -eq 0) { [void]$b.Append("$e[3$($r.Next(1, 8))m$t$e[0m ") } else { [void]$b.Append("$t ") }
    $n += $t.Length + 1
  }
  [void]$b.Append("`n")
  if ($i % 200 -eq 199) { [Console]::Out.Write($b.ToString()); [void]$b.Clear() }
}
[Console]::Out.Write($b.ToString() + "$e]2;fill done$([char]7)")
Start-Sleep 3600
'@ -replace '__LINES__', $N)
}

# Glyphs the atlas hasn't seen, a line at a time with a pause so each screen is drawn: CJK
# ideographs (wide, a fallback font), emoji (wide, coloured), maths and arrows, Greek and Cyrillic,
# in the eight colours and bold, then the title 'fill done'
function New-GlyphProgram {
  New-PwshProgram -Script @'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$e = [char]27
$cols = [Console]::WindowWidth - 2
$runs = @(
  @{ From = 0x4E00; Count = 4000; Wide = $true },
  @{ From = 0x1F300; Count = 0x350; Wide = $true },
  @{ From = 0x2190; Count = 0x170; Wide = $false },
  @{ From = 0x0391; Count = 0x60; Wide = $false },
  @{ From = 0x0410; Count = 0x40; Wide = $false },
  @{ From = 0x2500; Count = 0x80; Wide = $false }
)
$line = 0
foreach ($run in $runs) {
  $per = if ($run.Wide) { [int][Math]::Floor($cols / 2) } else { $cols }
  for ($i = 0; $i -lt $run.Count; $i += $per) {
    $b = [Text.StringBuilder]::new()
    $line++
    [void]$b.Append("$e[$(if ($line % 3 -eq 0) { '1;' })3$($line % 7 + 1)m")
    for ($j = $i; $j -lt [Math]::Min($run.Count, $i + $per); $j++) { [void]$b.Append([char]::ConvertFromUtf32($run.From + $j)) }
    [void]$b.Append("$e[0m`n")
    [Console]::Out.Write($b.ToString())
    Start-Sleep -Milliseconds 20
  }
}
[Console]::Out.Write("$e]2;fill done$([char]7)")
Start-Sleep 3600
'@
}

function Get-PaintStats($m) {
  $paint = @($m.budget.actions | Where-Object action -eq 'paint') | Select-Object -First 1
  $h = @($m.histograms | Where-Object name -eq 'gui.paint.impl') | Select-Object -First 1
  [ordered]@{
    paints = $m.totals.paints; paint_ms = $m.totals.paint_ms; frames = $m.totals.frames
    p50 = $paint.p50_ms; p95 = $paint.p95_ms; p99 = $paint.p99_ms; max = $paint.max_ms; count = $paint.count
    impl = $h
  }
}

function Measure-Step([string]$Label, $w) {
  $last = -1; $since = [Diagnostics.Stopwatch]::StartNew(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt 30) {
    $s = Get-StateSnapshot $w.StateFile
    $f = if ($s) { [long]$s.frame } else { -1 }
    if ($f -ne $last) { $last = $f; $since.Restart() }
    if ($since.Elapsed.TotalSeconds -ge 2) { break }
    Start-Sleep -Milliseconds 100
  }
  Start-Sleep -Seconds 1
  $m = Get-Metrics $w -Reset
  $gui = Get-MemReport -ProcessId $w.Pid -Scan:$Scan
  $tabs = $null; $st = Get-StateSnapshot $w.StateFile
  if ($st) { $sw = Get-StateWindow $st -Hwnd ([long]$w.Handle); if ($sw) { $tabs = @($sw.tabs).Count } }
  $paint = Get-PaintStats $m
  $big = @($gui.BigPrivate | Select-Object -First 6 | ForEach-Object { "$($_.ReservedMB)/$($_.CommittedMB)c/$($_.ResidentMB)r$(if ($Scan) { "/$($_.ZeroMB)z" })" })
  $step = [ordered]@{ label = $Label; at = (Get-Date).ToString('HH:mm:ss'); tabs = $tabs; frame = $last; gui = $gui; paint = $paint }
  $script:report.steps += $step
  "[$Label] tabs $tabs; private $($gui.PrivateUsageMB) MB, WS $($gui.WorkingSetMB) MB, private resident $($gui.PrivateResidentMB); GPU dedicated $($gui.Gpu.DedicatedMB) shared $($gui.Gpu.SharedMB) local $($gui.Gpu.LocalMB) non-local $($gui.Gpu.NonLocalMB); biggest private (reserved/committed/resident$(if ($Scan) { '/zero' }) MB): $($big -join ', '); paints $($paint.paints) p50 $($paint.p50) p95 $($paint.p95) p99 $($paint.p99) max $($paint.max) ms"
}

$script:glyphRuns = 0
$cfgPath = Join-Path $out 'config\mem.lua'
New-Item -ItemType Directory -Force (Split-Path $cfgPath) | Out-Null
New-TestConfig -Path $cfgPath -Lua "config.scrollback_lines = 50000`nconfig.default_prog = { '$pwshExe', '-NoProfile' }"
$report.loadBefore = Get-Load

Invoke-Test {
  $open = @{ ConfigFile = $cfgPath; Program = @($pwshExe, '-NoProfile'); TimeoutSec = 30 }
  if ($Log) { $open.Log = $Log }
  if ($Env) { $e = @{}; foreach ($kv in $Env -split ';') { $k, $v = $kv -split '=', 2; $e[$k] = $v }; $open.Environment = $e }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What 'the first frame' -TimeoutSec 30 | Out-Null
  $first = $null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($null -eq $first -and $clock.Elapsed.TotalSeconds -lt 20) {
    $panes = @(Get-Panes)
    if ($panes) { $first = $panes[0].pane_id } else { Start-Sleep -Milliseconds 200 }
  }
  if ($null -eq $first) { throw 'no pane found' }
  Start-Sleep -Seconds $SettleSec
  Measure-Step 'start' $w
  foreach ($step in $Steps) {
    $p = $step -split ':'
    switch ($p[0]) {
      'tabs' {
        $want = [int]$p[1]
        $have = @(Get-Tabs).Count
        for ($i = $have; $i -lt $want; $i++) { New-Tab -From $first -Program @($pwshExe, '-NoProfile') | Out-Null }
        Wait-State { param($s) @($s.tabs).Count -ge $want } -What "$want tabs" -TimeoutSec 60 | Out-Null
        Start-Sleep -Seconds $SettleSec
        Measure-Step "tabs$want" $w
      }
      'lines' {
        $n = [int]$p[1]
        $pane = New-Tab -From $first -Program (New-FillProgram $n)
        Wait-State { param($s) @($s.panes | Where-Object { $_.title -eq 'fill done' }).Count -gt 0 } -What "$n lines" -TimeoutSec 600 | Out-Null
        Start-Sleep -Seconds $SettleSec
        Measure-Step "lines$n" $w
      }
      'glyphs' {
        $pane = New-Tab -From $first -Program (New-GlyphProgram)
        if ($Shots) { Start-Sleep -Seconds 3; (Get-Shot $w -Name "glyphs$($script:glyphRuns + 1)-3s").Dispose() }
        Wait-State { param($s) @($s.panes | Where-Object { $_.title -eq 'fill done' }).Count -gt 0 } -What 'the glyphs' -TimeoutSec 300 | Out-Null
        Start-Sleep -Seconds $SettleSec
        $script:glyphRuns++
        Measure-Step "glyphs$($script:glyphRuns)" $w
      }
      'rest' { Start-Sleep -Seconds ([int]$p[1]); Measure-Step "rest$($p[1])" $w }
    }
  }
  $report.loadAfter = Get-Load
  if ($Log) { $atlas = @(Get-LogLines $w -Pattern 'texture atlas'); $report.atlas = @($atlas | ForEach-Object { "$_" }); "atlas: $($atlas.Count) lines: $((@($atlas) | Select-Object -Last 6) -join ' | ')" }
} -Finally {
  [IO.File]::WriteAllText((Join-Path $out 'report.json'), ($report | ConvertTo-Json -Depth 8))
} -Passed "gpumem-$($Name): measured"
