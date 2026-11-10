#Requires -Version 7
<#
The program the startup and rest runs start in each tab (bench.ps1): the moment it runs, it writes
the time (DateTime ticks, UTC: the clock bench.ps1 reads too), its pid and the console's columns
and rows to -Out, then sleeps -Sleep seconds, or with pwsh's -NoExit before -File goes on as an
idle shell at its prompt.

  pwsh -NoProfile [-NoExit] -File ready.ps1 -Out <file> [-Sleep 60]
#>
param([Parameter(Mandatory)][string]$Out, [int]$Sleep = 0)
$t = [DateTime]::UtcNow.Ticks
# After the time is read, so the helper's load is in no measurement
. (Join-Path $PSScriptRoot '..\write-whole.ps1')
Write-FileWhole -Path $Out -Text "$t $PID $([Console]::WindowWidth) $([Console]::WindowHeight)"
if ($Sleep) { Start-Sleep -Seconds $Sleep }
