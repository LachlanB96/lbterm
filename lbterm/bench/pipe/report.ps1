#Requires -Version 7
# A folded sample file as self and inclusive shares, by CPU (cycles) and by wall (samples).
# -Match keeps only stacks containing a frame matching it; -Under gives inclusive shares only of frames below a frame matching it.
param([Parameter(Mandatory)][string]$Path, [int]$Top = 30, [string]$Match, [ValidateSet('cpu', 'wall')][string]$By = 'cpu', [switch]$NoInline, [int]$Width = 160)
$rows = foreach ($l in [IO.File]::ReadAllLines($Path)) {
  $p = $l.Split("`t"); if ($p.Count -lt 3) { continue }
  [pscustomobject]@{ Stack = $p[0]; Cycles = [double]$p[1]; Count = [int]$p[2] }
}
if ($Match) { $rows = @($rows | Where-Object { $_.Stack -match $Match }) }
$tot = 0.0; $self = @{}; $incl = @{}
foreach ($r in $rows) {
  $w = if ($By -eq 'cpu') { $r.Cycles } else { $r.Count }
  $tot += $w
  $frames = $r.Stack.Split(';')
  if ($NoInline) { $frames = @($frames | Where-Object { -not $_.EndsWith(' [inl]') }) }
  $leaf = $frames[-1]
  $self[$leaf] = $w + [double]$self[$leaf]
  $seen = @{}
  foreach ($f in $frames) { if (-not $seen.ContainsKey($f)) { $seen[$f] = 1; $incl[$f] = $w + [double]$incl[$f] } }
}
$allCount = ($rows | Measure-Object Count -Sum).Sum
$allCyc = ($rows | Measure-Object Cycles -Sum).Sum
"$Path`n  samples $allCount, cpu $([math]::Round($allCyc / 3e6, 1)) ms; by $By"
$cut = { param($s) if ($s.Length -gt $Width) { $s.Substring(0, $Width) } else { $s } }
'-- self'
$self.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First $Top | ForEach-Object { '{0,6:N1}%  {1}' -f (100 * $_.Value / [math]::Max(1.0, $tot)), (& $cut $_.Key) }
'-- inclusive'
$incl.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First ($Top * 2) | ForEach-Object { '{0,6:N1}%  {1}' -f (100 * $_.Value / [math]::Max(1.0, $tot)), (& $cut $_.Key) }
