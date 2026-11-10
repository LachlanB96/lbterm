#Requires -Version 7
<#
The tables of a bench.ps1 run (lbterm/BENCH.md has the last ones): medians and ranges (min to max)
of every measurement in <run>\results.jsonl, by terminal, as Markdown. Prints them and writes
<run>\tables.md (in the first run's folder). Several runs' results go in one set of tables: a
variant's run (bench.ps1 -Variant) beside the run it varies. The settings and versions are the
first run's.

  pwsh -File lbterm\bench\summarize.ps1 -Run target\bench\<time>[,target\bench\<time>]
#>
param([Parameter(Mandatory)][string[]]$Run)
$ErrorActionPreference = 'Stop'
$runs = @($Run -split ',' | ForEach-Object Trim | Where-Object { $_ })
$first = $runs[0]

$rows = @(foreach ($r in $runs) { Get-Content -LiteralPath (Join-Path $r 'results.jsonl') | ForEach-Object { $_ | ConvertFrom-Json } })
# A variant's results (bench.ps1 -Variant) are a terminal of their own: lbterm/<variant>
foreach ($r in $rows) { if ($r.PSObject.Properties['variant'] -and $r.variant) { $r.kind = "$($r.kind)/$($r.variant)" } }
$envInfo = Get-Content -Raw -LiteralPath (Join-Path $first 'env.json') | ConvertFrom-Json
# wezterm stays for runs from before 2026-11-04, so their tables can still be made again
$names = @{ lbterm = 'LBTerm (local)'; 'lbterm-mux' = 'LBTerm (mux)'; wezterm = 'WezTerm'; wt = 'Windows Terminal' }
$order = @('lbterm', 'lbterm-mux', 'wezterm', 'wt')
foreach ($k in @($rows | Where-Object { $_.kind -like '*/*' } | ForEach-Object kind | Select-Object -Unique)) {
  $base, $v = $k -split '/', 2
  $label = $names[$base] -replace '\)$', ''
  $names[$k] = "$label, $v)"
  $order += $k
}

function Get-Median([double[]]$v) {
  $s = @($v | Sort-Object); $n = $s.Count
  if (-not $n) { return $null }
  if ($n % 2) { $s[($n - 1) / 2] } else { ($s[$n / 2 - 1] + $s[$n / 2]) / 2 }
}
# "median (min-max)" to the given places, or a dash with nothing
function Format-Stat($Values, [int]$Places = 0, [string]$Unit = '') {
  $v = @($Values | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ })
  if (-not $v.Count) { return '-' }
  $f = "{0:N$Places}"
  $m = $f -f (Get-Median $v)
  if ($v.Count -eq 1) { return "$m$Unit" }
  $lo = $f -f ($v | Measure-Object -Minimum).Minimum; $hi = $f -f ($v | Measure-Object -Maximum).Maximum
  "$m$Unit ($lo-$hi)"
}
function Get-Kinds($Of) { @($order | Where-Object { $k = $_; @($Of | Where-Object kind -eq $k).Count }) }

$out = [Collections.Generic.List[string]]::new()
function Add-Line([string]$l = '') { $out.Add($l) }

# WezTerm only in runs from before 2026-11-04, when the benchmark stopped running it
$wez = if ($envInfo.wezterm) { ", WezTerm $($envInfo.wezterm)" } else { '' }
Add-Line "Run $([IO.Path]::GetFileName($first.TrimEnd('\'))): LBTerm $($envInfo.lbterm)$wez, Windows Terminal $($envInfo.wt); $($envInfo.settings.cols)x$($envInfo.settings.rows), Cascadia Mono 12 pt, scrollback $($envInfo.settings.scrollback). $($envInfo.cpu)."
Add-Line

# Startup
$st = @($rows | Where-Object { $_.phase -eq 'startup' -and -not $_.warmup })
if ($st.Count) {
  Add-Line '### Startup (ms from the launch)'
  Add-Line
  Add-Line '| Terminal | n | Window shown | Program created | Program running |'
  Add-Line '|---|---|---|---|---|'
  foreach ($k in Get-Kinds $st) {
    $x = @($st | Where-Object kind -eq $k)
    Add-Line "| $($names[$k]) | $($x.Count) | $(Format-Stat $x.visibleMs) | $(Format-Stat $x.programCreatedMs) | $(Format-Stat $x.programRunningMs) |"
  }
  Add-Line
}

# Throughput
$tp = @($rows | Where-Object phase -eq 'throughput')
if ($tp.Count) {
  Add-Line '### Throughput (median of the rounds, range in brackets)'
  Add-Line
  Add-Line '| Pattern | Terminal | n | MB/s shown | MB/s to quiet | MB/s taken | Lines/s shown | Terminal CPU s/MB | Cores while printing | GUI / server / ConPTY CPU s |'
  Add-Line '|---|---|---|---|---|---|---|---|---|---|'
  foreach ($p in @('plain', 'wrap', 'sgr', 'unicode', 'vte')) {
    foreach ($k in Get-Kinds ($tp | Where-Object pattern -eq $p)) {
      $x = @($tp | Where-Object { $_.pattern -eq $p -and $_.kind -eq $k })
      $cores = @($x | ForEach-Object { $_.cpuRun.Terminal / $_.endToEnd })
      # Until the terminal went quiet after the marker: the tail's wait less the 500 ms of quiet
      # Wait-BenchQuiet looks for (so to about 100 ms, its reading interval)
      $quiet = @($x | ForEach-Object { ($_.bytes / 1MB) / ($_.endToEnd + [Math]::Max(0, $_.tailMs - 500) / 1000) })
      $split = '{0:N2} / {1:N2} / {2:N2}' -f (Get-Median @($x | ForEach-Object { $_.cpuRun.Gui + $_.cpuTail.Gui })), (Get-Median @($x | ForEach-Object { $_.cpuRun.Server + $_.cpuTail.Server })), (Get-Median @($x | ForEach-Object { $_.cpuRun.Conpty + $_.cpuTail.Conpty }))
      Add-Line "| $p | $($names[$k]) | $($x.Count) | $(Format-Stat $x.mbpsShown 1) | $(Format-Stat $quiet 1) | $(Format-Stat $x.mbps 1) | $(Format-Stat $x.linesPerSecShown) | $(Format-Stat $x.cpuPerMB 3) | $(Format-Stat $cores 2) | $split |"
    }
  }
  Add-Line
}

# CPU at rest
$idle = @($rows | Where-Object phase -eq 'idle')
if ($idle.Count) {
  Add-Line "### CPU at rest ($($envInfo.settings.restSec) s, % of one core)"
  Add-Line
  Add-Line '| Terminal | Tabs | n | Terminal | GUI | Mux server | ConPTY | The shells |'
  Add-Line '|---|---|---|---|---|---|---|---|'
  foreach ($k in Get-Kinds $idle) {
    foreach ($tabs in 1, 8) {
      $x = @($idle | Where-Object { $_.kind -eq $k -and $_.tabs -eq $tabs })
      if (-not $x.Count) { continue }
      $pct = { param($f) @($x | ForEach-Object { 100 * $_.cpu.$f / $_.cpu.Seconds }) }
      Add-Line "| $($names[$k]) | $tabs | $($x.Count) | $(Format-Stat $x.cpu.TerminalPercent 2) | $(Format-Stat (& $pct 'Gui') 2) | $(Format-Stat (& $pct 'Server') 2) | $(Format-Stat (& $pct 'Conpty') 2) | $(Format-Stat (& $pct 'Shell') 2) |"
    }
  }
  Add-Line
}

# Memory
$sb = @($rows | Where-Object phase -eq 'scrollback')
if ($idle.Count -or $sb.Count) {
  Add-Line '### Memory (MB: private bytes / working set of the GUI, mux server and ConPTY together)'
  Add-Line
  Add-Line '| Terminal | n | 1 tab | 8 tabs | 100,000 lines, scrollback alike | 100,000 lines, all kept | GUI alone, 1 tab |'
  Add-Line '|---|---|---|---|---|---|---|'
  foreach ($k in Get-Kinds (@($idle) + @($sb))) {
    $one = @($idle | Where-Object { $_.kind -eq $k -and $_.tabs -eq 1 })
    $eight = @($idle | Where-Object { $_.kind -eq $k -and $_.tabs -eq 8 })
    $alike = @($sb | Where-Object { $_.kind -eq $k -and $_.scrollbackLines -eq $envInfo.settings.scrollback })
    $kept = @($sb | Where-Object { $_.kind -eq $k -and $_.scrollbackLines -eq $envInfo.settings.scrollLines })
    $pair = { param($x) if ($x.Count) { "$(Format-Stat $x.memory.PrivateMB 0) / $(Format-Stat $x.memory.WorkingSetMB 0)" } else { '-' } }
    Add-Line "| $($names[$k]) | $($one.Count) | $(& $pair $one) | $(& $pair $eight) | $(& $pair $alike) | $(& $pair $kept) | $(Format-Stat $one.memory.GuiPrivateMB 0) / $(Format-Stat $one.memory.GuiWorkingSetMB 0) |"
  }
  Add-Line
}

# What else ran
$load = @($rows | Where-Object phase -eq 'load')
if ($load.Count) {
  Add-Line "### What else ran (busy cores over a second as each round began, of $([Environment]::ProcessorCount) threads)"
  Add-Line
  foreach ($l in $load) { Add-Line "- $($l.what): $($l.busyCores) cores; $(@($l.top) -join ', ')" }
  Add-Line
}
$watch = @($rows | Where-Object phase -eq 'watch')
foreach ($w in $watch) {
  if (@($w.wsl).Count) { Add-Line "WSL processes seen: $(@($w.wsl) -join '; ')" } else { Add-Line 'No wsl.exe (or WSL bash.exe) started during the run.' }
  foreach ($b in $w.banned.PSObject.Properties) { Add-Line "Not run or stopped: $($b.Name): $($b.Value)" }
}

$text = $out -join "`n"
[IO.File]::WriteAllText((Join-Path $first 'tables.md'), $text + "`n")
$text
