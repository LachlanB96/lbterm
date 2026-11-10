# A row per step of every report: the GUI's memory split into the pieces this investigation found
param([string[]]$Names = @('*'), [switch]$Server, [switch]$Csv)
$s = Split-Path $PSScriptRoot -Parent
$rows = foreach ($d in Get-ChildItem (Join-Path $s 'out') -Directory | Where-Object { $n = $_.Name; @($Names | Where-Object { $n -like $_ }).Count }) {
  $f = Join-Path $d.FullName 'report.json'
  if (-not (Test-Path $f)) { continue }
  $r = Get-Content -Raw $f | ConvertFrom-Json
  foreach ($st in $r.steps) {
    $targets = if ($Server) { @($st.servers) } else { @($st.gui) }
    foreach ($g in $targets) {
      if (-not $g) { continue }
      $kind = @{}; foreach ($k in $g.ByKind) { $kind[$k.Kind] = $k }
      $big = @($g.BigPrivate)
      $pick = { param($pred) @($big | Where-Object $pred) }
      $dev = & $pick { $_.Kind -eq 'private-other' -and $_.ReservedMB -eq 256 }
      $host64 = & $pick { $_.Kind -eq 'private-other' -and $_.ReservedMB -eq 64 }
      $c32 = & $pick { $_.Kind -eq 'private-other' -and $_.ReservedMB -ge 30 -and $_.ReservedMB -le 32.1 }
      $sum = { param($set, $p) [Math]::Round((@($set) | Measure-Object $p -Sum).Sum, 1) }
      $heap = @($g.ByKind | Where-Object { $_.Kind -like 'heap-segment#*' })
      $heapFree = if ($g.Notes) { ($g.Notes | Where-Object { $_ -match 'TotalFreeSize (\d+)' } | ForEach-Object { [double]([regex]::Match($_, 'TotalFreeSize (\d+)').Groups[1].Value) / 1MB } | Measure-Object -Sum).Sum } else { $null }
      $other = @($g.ByKind | Where-Object { $_.Kind -eq 'private-other' })
      [pscustomobject]@{
        Run = $d.Name; Step = $st.label; Tabs = $st.tabs
        Private = $g.PrivateUsageMB; WS = $g.WorkingSetMB
        PrivRes = $g.PrivateResidentMB
        Dev256 = & $sum $dev 'CommittedMB'
        Host64 = & $sum $host64 'ResidentMB'
        Drv32 = & $sum $c32 'CommittedMB'; Drv32Res = & $sum $c32 'ResidentMB'
        OtherPriv = [Math]::Round((& $sum $other 'CommittedMB') - (& $sum $dev 'CommittedMB') - (& $sum $host64 'CommittedMB') - (& $sum $c32 'CommittedMB'), 1)
        Heap = & $sum $heap 'CommittedMB'; HeapZero = & $sum $heap 'ZeroMB'; HeapFree = $(if ($null -ne $heapFree) { [Math]::Round($heapFree, 1) })
        ImgW = $g.ImageWritableMB; ImgPrivRes = $g.ImageResidentPrivateMB
        Stacks = $kind['stack'].CommittedMB
        GpuDed = $g.Gpu.DedicatedMB; GpuShr = $g.Gpu.SharedMB
        Conpty = $st.conpty.privateMB
      }
    }
  }
}
if ($Csv) { $rows | ConvertTo-Csv -NoTypeInformation } else { $rows | Format-Table -AutoSize | Out-String -Width 400 }
