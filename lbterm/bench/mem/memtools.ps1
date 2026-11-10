# Memory breakdown of a process: VirtualQueryEx by allocation, working set by page, heaps,
# stacks, mapped files and images, and the GPU's per-process counters. Dot-source it.
if (-not ('LbMem.Mem' -as [type])) {
  Add-Type -Path (Join-Path $PSScriptRoot 'LbMem.cs') -ReferencedAssemblies 'System.Diagnostics.Process', 'System.ComponentModel.Primitives', 'System.Collections', 'System.Linq', 'System.Runtime.InteropServices', 'System.Collections.NonGeneric'
}

function MB([double]$b) { [Math]::Round($b / 1MB, 1) }

function Get-GpuMem([int]$ProcessId) {
  $r = [ordered]@{ DedicatedMB = 0.0; SharedMB = 0.0; LocalMB = 0.0; NonLocalMB = 0.0; CommittedMB = 0.0 }
  try {
    $c = Get-Counter -Counter "\GPU Process Memory(pid_${ProcessId}_*)\*" -ErrorAction Stop
    foreach ($s in $c.CounterSamples) {
      $v = $s.CookedValue / 1MB
      switch -Wildcard ($s.Path) {
        '*\dedicated usage' { $r.DedicatedMB += $v }
        '*\shared usage' { $r.SharedMB += $v }
        '*\local usage' { $r.LocalMB += $v }
        '*\non local usage' { $r.NonLocalMB += $v }
        '*\total committed' { $r.CommittedMB += $v }
      }
    }
  } catch { $r.Error = $_.Exception.Message }
  foreach ($k in @($r.Keys)) { if ($r[$k] -is [double]) { $r[$k] = [Math]::Round($r[$k], 1) } }
  [pscustomobject]$r
}

function Get-KindGroup([string]$Kind) {
  if ($Kind -like 'heap-segment#*' -or $Kind -like 'heap-large#*' -or $Kind -like 'segment-heap#*') { return $Kind }
  $Kind
}

# A report object for $ProcessId: totals, by kind, the biggest of each, modules, mapped files, fonts, GPU
function Get-MemReport {
  param([Parameter(Mandatory)][int]$ProcessId, [switch]$Scan, [int]$Top = 25)
  $gpu = Get-GpuMem $ProcessId
  $s = [LbMem.Mem]::Take([uint32]$ProcessId, [bool]$Scan)
  $allocs = @($s.Allocs)
  $byKind = $allocs | Group-Object { Get-KindGroup $_.Kind } | ForEach-Object {
    $g = $_.Group
    [pscustomobject]@{
      Kind = $_.Name; Count = $g.Count
      CommittedMB = MB (($g | Measure-Object Committed -Sum).Sum)
      ReservedMB = MB (($g | Measure-Object Reserved -Sum).Sum)
      ResidentMB = MB (($g | Measure-Object Resident -Sum).Sum)
      ResidentPrivateMB = MB (($g | Measure-Object ResidentPrivate -Sum).Sum)
      WritableMB = MB (($g | Measure-Object WritableCommitted -Sum).Sum)
      ZeroMB = MB (($g | Measure-Object ZeroResident -Sum).Sum)
    }
  } | Sort-Object CommittedMB -Descending
  $priv = @($allocs | Where-Object { $_.Type -eq [LbMem.Mem]::MEM_PRIVATE })
  $img = @($allocs | Where-Object { $_.Type -eq [LbMem.Mem]::MEM_IMAGE })
  $map = @($allocs | Where-Object { $_.Type -eq [LbMem.Mem]::MEM_MAPPED })
  $sum = { param($set, $f) MB (($set | Measure-Object $f -Sum).Sum) }
  $modules = $img | ForEach-Object {
    [pscustomobject]@{
      Module = [IO.Path]::GetFileName($_.File); CommittedMB = MB $_.Committed; ResidentMB = MB $_.Resident
      ResidentPrivateMB = MB $_.ResidentPrivate; WritableMB = MB $_.WritableCommitted
    }
  } | Sort-Object ResidentMB -Descending
  $mapped = $map | ForEach-Object {
    [pscustomobject]@{
      File = $(if ($_.File) { $_.File -replace '^\\Device\\HarddiskVolume\d+', '' } else { '(section)' }); Base = '0x{0:x}' -f $_.Base
      CommittedMB = MB $_.Committed; ResidentMB = MB $_.Resident; ResidentPrivateMB = MB $_.ResidentPrivate; WritableMB = MB $_.WritableCommitted
    }
  } | Sort-Object CommittedMB -Descending
  $bigPrivate = $priv | Sort-Object Committed -Descending | Select-Object -First $Top | ForEach-Object {
    [pscustomobject]@{
      Base = '0x{0:x}' -f $_.Base; Kind = $_.Kind; CommittedMB = MB $_.Committed; ReservedMB = MB $_.Reserved
      ResidentMB = MB $_.Resident; ZeroMB = MB $_.ZeroResident; Detail = $_.Detail
    }
  }
  [pscustomobject]@{
    Pid = $ProcessId
    PrivateUsageMB = MB $s.PrivateUsage; WorkingSetMB = MB $s.WorkingSet; PeakWorkingSetMB = MB $s.PeakWorkingSet
    PeakPagefileMB = MB $s.PeakPagefileUsage
    PrivateCommittedMB = & $sum $priv 'Committed'; PrivateResidentMB = & $sum $priv 'Resident'; PrivateReservedMB = & $sum $priv 'Reserved'
    ImageCommittedMB = & $sum $img 'Committed'; ImageResidentMB = & $sum $img 'Resident'; ImageResidentPrivateMB = & $sum $img 'ResidentPrivate'; ImageWritableMB = & $sum $img 'WritableCommitted'
    MappedCommittedMB = & $sum $map 'Committed'; MappedResidentMB = & $sum $map 'Resident'; MappedResidentPrivateMB = & $sum $map 'ResidentPrivate'; MappedWritableMB = & $sum $map 'WritableCommitted'
    Heaps = @($s.Heaps | ForEach-Object { '0x{0:x}' -f $_ }); ProcessHeap = '0x{0:x}' -f $s.ProcessHeap
    Threads = $s.ThreadNames.Count
    ThreadNames = @($s.ThreadNames.Values | Group-Object | Sort-Object Count -Descending | ForEach-Object { "$($_.Name) x$($_.Count)" })
    Gpu = $gpu
    ByKind = @($byKind)
    BigPrivate = @($bigPrivate)
    Modules = @($modules)
    Mapped = @($mapped)
    Fonts = @($s.Fonts | ForEach-Object { [pscustomobject]@{ Address = '0x{0:x}' -f $_.Address; SizeMB = [Math]::Round($_.Size / 1MB, 2); Name = $_.Name; Tag = $_.Tag; Owner = $_.Owner; Tables = $_.Tables } })
    Refs = @($s.Refs | Sort-Object Count -Descending | ForEach-Object { [pscustomobject]@{ Target = '0x{0:x}' -f $_.Target; From = $_.From; Count = $_.Count } })
    ModuleMap = @($img | ForEach-Object { [pscustomobject]@{ Module = [IO.Path]::GetFileName($_.File); Base = '0x{0:x}' -f $_.Base; End = '0x{0:x}' -f ($_.Base + $_.Reserved) } })
    Notes = @($s.Notes)
    HeapWalk = [pscustomobject]@{
      BusyMB = MB $s.HeapBusy; BusyCount = $s.HeapBusyCount; FreeMB = MB $s.HeapFreeWalked; Stops = $s.HeapWalkStops
      SmallBusyMB = @($s.SmallBusy | ForEach-Object { MB $_ })
      BigBusyMB = MB ((@($s.Blocks | Where-Object Busy) | Measure-Object Size -Sum).Sum)
      BigZeroMB = MB ((@($s.Blocks | Where-Object Busy) | Measure-Object Zero -Sum).Sum)
      Big = @($s.Blocks | Where-Object Busy | Sort-Object Size -Descending | Select-Object -First 40 | ForEach-Object {
        [pscustomobject]@{ Address = '0x{0:x}' -f $_.Address; KB = [Math]::Round($_.Size / 1KB); ZeroKB = [Math]::Round($_.Zero / 1KB); ResKB = [Math]::Round($_.Resident / 1KB); Refs = $_.Refs; Head = $_.Head; RefFrom = $_.RefFrom }
      })
      SizeHistogram = @($s.Blocks | Where-Object Busy | Group-Object { [Math]::Round($_.Size / 1KB) } | Sort-Object { [double]$_.Name } -Descending | ForEach-Object { "$($_.Name) KB x$($_.Count) (zero $([Math]::Round((($_.Group | Measure-Object Zero -Sum).Sum) / 1MB, 1)) MB)" })
    }
  }
}

# One line for the console
function Format-MemLine($r) {
  "private $($r.PrivateUsageMB) MB (private allocs committed $($r.PrivateCommittedMB), resident $($r.PrivateResidentMB); image writable $($r.ImageWritableMB), mapped writable $($r.MappedWritableMB)), WS $($r.WorkingSetMB) MB (image $($r.ImageResidentMB), mapped $($r.MappedResidentMB)); GPU dedicated $($r.Gpu.DedicatedMB) shared $($r.Gpu.SharedMB) committed $($r.Gpu.CommittedMB)"
}
