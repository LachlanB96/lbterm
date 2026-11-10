#Requires -Version 7
# The heaviest stacks through a frame matching -Match: the -Up frames above it (callers) and -Down below, grouped
param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Match, [int]$Up = 8, [int]$Down = 4, [int]$Top = 8, [ValidateSet('cpu', 'wall')][string]$By = 'cpu')
$groups = @{}; $tot = 0.0; $all = 0.0
foreach ($l in [IO.File]::ReadAllLines($Path)) {
  $p = $l.Split("`t"); if ($p.Count -lt 3) { continue }
  $w = if ($By -eq 'cpu') { [double]$p[1] } else { [double]$p[2] }
  $all += $w
  $f = $p[0].Split(';')
  $i = -1; for ($k = 0; $k -lt $f.Count; $k++) { if ($f[$k] -match $Match) { $i = $k; break } }
  if ($i -lt 0) { continue }
  $tot += $w
  $a = [Math]::Max(0, $i - $Up); $b = [Math]::Min($f.Count - 1, $i + $Down)
  $key = ($f[$a..$b] | ForEach-Object { if ($_.Length -gt 110) { $_.Substring(0, 110) } else { $_ } }) -join "`n    "
  $groups[$key] = $w + [double]$groups[$key]
}
"$([math]::Round(100 * $tot / [math]::Max(1.0, $all), 1))% of the thread's $By through /$Match/"
$groups.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First $Top | ForEach-Object { '{0,6:N1}%' -f (100 * $_.Value / [math]::Max(1.0, $all)); "    $($_.Key)" }
