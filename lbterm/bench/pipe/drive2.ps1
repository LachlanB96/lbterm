#Requires -Version 7
# Interleaved runs: each cycle, each build, each domain, one window of -Rounds rounds
param([int]$Cycles = 3, [string[]]$Builds = @('installed', 'main', 'fonts'), [string[]]$Domains = @('local', 'mux'), [string]$Config = 'bench', [int]$Rounds = 2, [string]$Prefix = 'm', [switch]$Sample, [string]$Script = 'run.ps1', [string]$File = 'plain', [string]$SampleFile = 'plain4', [int]$SampleThreads = 5, [int]$Hz = 1000)
$Here = $PSScriptRoot
if ($Prefix -eq 'SL') { exit 0 }
$Builds = @($Builds | ForEach-Object { $_ -split ',' }); $Domains = @($Domains | ForEach-Object { $_ -split ',' })
$log = Join-Path $Here "drive-$Prefix.log"
foreach ($c in 1..$Cycles) {
  foreach ($b in $Builds) {
    foreach ($d in $Domains) {
      $name = "$Prefix-$Config-$b-$d-c$c"
      $args = @('-NoProfile', '-File', (Join-Path $Here $Script), '-Bin', (Join-Path $Here $b), '-Name', $name, '-Tag', $b, '-Config', $Config, '-Rounds', $Rounds, '-File', $File)
      if ($d -eq 'mux') { $args += '-Mux' }
      if ($Sample) { $args += @('-Sample', '-SampleFile', $SampleFile, '-SampleThreads', $SampleThreads, '-Hz', $Hz) }
      Add-Content $log "== $(Get-Date -Format 'HH:mm:ss') $name"
      $out = & 'C:/Program Files/PowerShell/7/pwsh.exe' @args 2>&1 | ForEach-Object { "$_" }
      Add-Content $log ($out | Where-Object { $_ -notmatch '^NOTE' })
      $notes = @($out | Where-Object { $_ -match '^NOTE' }).Count
      if ($notes) { Add-Content $log "  ($notes other test windows noted)" }
    }
  }
}
Add-Content $log "== done $(Get-Date -Format 'HH:mm:ss')"
