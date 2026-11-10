#Requires -Version 7
# Compiles the probe's helpers and writes the data once
$ErrorActionPreference = 'Stop'
$Here = $PSScriptRoot
$Data = Join-Path $Here 'data'
New-Item -ItemType Directory -Force $Data | Out-Null
$bench = Join-Path $PSScriptRoot '..\BenchCon.cs'
if (-not (Test-Path (Join-Path $Data 'BenchCon.dll'))) { Add-Type -TypeDefinition (Get-Content -Raw $bench) -OutputAssembly (Join-Path $Data 'BenchCon.dll') -OutputType Library }
$dll = Join-Path $Data 'Probe.dll'
if (Test-Path $dll) { Remove-Item $dll }
Add-Type -TypeDefinition (Get-Content -Raw (Join-Path $Here 'lib.cs')) -OutputAssembly $dll -OutputType Library -CompilerOptions '/unsafe' -ReferencedAssemblies @('System.Collections', 'System.Linq', 'System.Runtime.InteropServices', 'System.Threading', 'System.Threading.Thread', 'System.Console', 'System.Diagnostics.Process')
Add-Type -Path $dll
if (-not (Test-Path (Join-Path $Data 'plain.txt'))) { $r = [PipeProbe.Data]::Plain((Join-Path $Data 'plain.txt'), 32MB, 80); "plain: $($r[0]) bytes, $($r[1]) lines" }
Get-ChildItem $Data | Select-Object Name, Length
