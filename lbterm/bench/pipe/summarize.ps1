#Requires -Version 7
# The rounds of runs\<Prefix>-* grouped by config, build, domain and file: medians and ranges
param([Parameter(Mandatory)][string]$Prefix, [switch]$Threads, [switch]$Io)
$Here = $PSScriptRoot
$rows = foreach ($d in Get-ChildItem (Join-Path $Here 'runs') -Directory -Filter "$Prefix-*") {
  $f = Join-Path $d.FullName 'rounds.jsonl'
  if (Test-Path $f) { Get-Content $f | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { -not $_.sampled } }
}
function Med([double[]]$v) { $s = @($v | Sort-Object); if (-not $s.Count) { return $null }; if ($s.Count % 2) { $s[[int][math]::Floor($s.Count / 2)] } else { ($s[$s.Count / 2 - 1] + $s[$s.Count / 2]) / 2 } }
function Rng([double[]]$v, [string]$fmt = '{0:N1}') { $s = @($v | Sort-Object); "$($fmt -f (Med $v)) ($($fmt -f $s[0])-$($fmt -f $s[-1]))" }
$groups = $rows | Group-Object { "$($_.config) $($_.tag) $(if ($_.mux) { 'mux' } else { 'local' }) $($_.round -replace '-\d+$', '')" }
foreach ($g in ($groups | Sort-Object Name)) {
  $r = $g.Group
  [pscustomobject]@{
    group = $g.Name; n = $r.Count
    mb_shown = Rng ($r.mb_shown); mb_taken = Rng ($r.mb_taken)
    cpu_s_mb = Rng ($r.cpu_s_per_mb) '{0:N3}'
    gui_ms = Rng ($r | ForEach-Object { $_.run.gui_ms }) '{0:N0}'
    gui_thread = Rng ($r | ForEach-Object { $_.run.gui_thread_ms }) '{0:N0}'
    server_ms = Rng ($r | ForEach-Object { $_.run.server_ms }) '{0:N0}'
    conpty_ms = Rng ($r | ForEach-Object { $_.run.conpty_ms }) '{0:N0}'
    frames = Rng ($r.frames) '{0:N0}'
  }
  if ($Threads) {
    # each round's threads by role: name, and the unnamed by rank
    $byName = @{}
    foreach ($x in $r) {
      $rank = @{}
      foreach ($t in @($x.run.threads)) {
        $k = "$($t.proc) $($t.name)"
        $rank[$k] = 1 + [int]$rank[$k]
        $key = if ($rank[$k] -gt 1) { "$k #$($rank[$k])" } else { $k }
        if (-not $byName.ContainsKey($key)) { $byName[$key] = [Collections.Generic.List[double]]::new() }
        $byName[$key].Add([double]$t.ms)
        if ($t.PSObject.Properties['csw']) { $ck = "$key csw"; if (-not $byName.ContainsKey($ck)) { $byName[$ck] = [Collections.Generic.List[double]]::new() }; $byName[$ck].Add([double]$t.csw) }
      }
    }
    $byName.GetEnumerator() | Where-Object { $_.Key -notmatch 'csw$' } | Sort-Object { - (Med $_.Value) } | Select-Object -First 8 | ForEach-Object {
      $c = $byName["$($_.Key) csw"]
      "    {0,-34} {1}{2}" -f $_.Key, (Rng $_.Value '{0:N0}'), $(if ($c) { "  csw $(Rng $c '{0:N0}')" })
    } | Out-Host
  }
  if ($Io) {
    foreach ($kind in 'gui', 'server', 'conpty') {
      $iox = @($r | ForEach-Object { @($_.io) | Where-Object { $_.proc -eq $kind } })
      if ($iox.Count) { "    io $kind reads $(Rng ($iox.read_ops) '{0:N0}') ($(Rng ($iox.read_bytes) '{0:N0}') B) writes $(Rng ($iox.write_ops) '{0:N0}') ($(Rng ($iox.write_bytes) '{0:N0}') B) other $(Rng ($iox.other_ops) '{0:N0}')" | Out-Host }
      $cs = @($r | ForEach-Object { @($_.csw) | Where-Object { $_.proc -eq $kind } })
      if ($cs.Count) { "    csw $kind $(Rng ($cs | ForEach-Object { [double]$_.csw }) '{0:N0}')" | Out-Host }
    }
  }
}
