#Requires -Version 7
# lbterm/bench's producer.ps1, for this probe: rounds named <file>-<n> (plain-1, plain-2), each
# waiting for go-<round>, then the file written in 64 KiB WriteFile calls and the title set to
# the go file's marker.
param([Parameter(Mandatory)][string]$Dir, [Parameter(Mandatory)][string]$Data, [Parameter(Mandatory)][string]$Rounds, [int]$Chunk = 65536, [int]$HoldSec = 900)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\write-whole.ps1')
try {
  Add-Type -Path (Join-Path $Data 'BenchCon.dll')
  $console = [LbtermBenchCon.Con]::Setup() | ConvertFrom-Json
  $names = @($Rounds -split ',' | Where-Object { $_ })
  $files = @{}
  foreach ($n in $names) { $f = $n -replace '-\d+$', ''; if (-not $files.ContainsKey($f)) { $files[$f] = [IO.File]::ReadAllBytes((Join-Path $Data "$f.txt")) } }
  [IO.File]::WriteAllText((Join-Path $Dir 'ready.json'), ([ordered]@{ pid = $PID; console = $console; rounds = $names } | ConvertTo-Json -Compress -Depth 4))
  $e = [char]27
  foreach ($n in $names) {
    $go = Join-Path $Dir "go-$n"
    while (-not [IO.File]::Exists($go)) { Start-Sleep -Milliseconds 5 }
    $marker = ''
    for ($i = 0; $i -lt 50 -and -not $marker; $i++) { try { $marker = [IO.File]::ReadAllText($go).Trim() } catch { Start-Sleep -Milliseconds 5 } }
    [LbtermBenchCon.Con]::Write("$e[0m$e[r$e[H$e[2J$e[3J$e]0;bench running $n$([char]7)")
    $r = [LbtermBenchCon.Con]::Send($files[($n -replace '-\d+$', '')], $Chunk)
    [LbtermBenchCon.Con]::Write("$e]0;$marker$([char]7)")
    $done = [ordered]@{ round = $n; start = $r[0]; end = $r[1]; seconds = $r[2] / 1e7; bytes = $r[3]; writes = $r[4] }
    Write-FileWhole -Path (Join-Path $Dir "done-$n.json") -Text ($done | ConvertTo-Json -Compress) -Temp (Join-Path $Dir "done-$n.tmp")
  }
  [IO.File]::WriteAllText((Join-Path $Dir 'finished'), '')
} catch {
  [IO.File]::WriteAllText((Join-Path $Dir 'error.txt'), "$($_.Exception.Message)`n$($_.ScriptStackTrace)")
  throw
}
Start-Sleep -Seconds $HoldSec
