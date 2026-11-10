param($S)
Import-Module (Join-Path $PSScriptRoot '..\..\harness.psm1') -Force -DisableNameChecking
Add-Type -Path (Join-Path $S 'pipe\data\Probe.dll')
$T = Start-Test 'pipe-walktest' -Bin (Join-Path $S 'pipe\fonts') -Out (Join-Path $S 'pipe\runs\walktest')
Invoke-Test {
  $w = Open-TestWindow -NoConfig -TimeoutSec 30
  Start-Sleep 2
  $p = Get-Process -Id $w.Pid
  $tids = @($p.Threads | Select-Object -First 4 | ForEach-Object { [uint32]$_.Id })
  $sm = [PipeProbe.Sampler]::Start([uint32[]]@($tids | ForEach-Object { [uint32]$w.Pid }), [uint32[]]$tids, [string[]]@($tids | ForEach-Object { "t$_" }), 1000, $T.Bin, 64)
  Start-Sleep 3; $sm.Recording = $true; Start-Sleep 2; $sm.Stop()
  "walks $($sm.Walks) walk_ms $([math]::Round($sm.WalkMs,1)) ctx_ms $([math]::Round($sm.CtxMs,1)) frames $($sm.Frames) per walk $([math]::Round($sm.WalkMs/$sm.Walks,3)) ms"
  foreach ($t in $sm.Targets) { "$($t.Label) $($t.Stacks.Count)" }
  $sm.Done()
} -Passed 'ok'