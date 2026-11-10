#Requires -Version 7
<#
What runs inside each terminal for the throughput and scrollback runs (bench.ps1): loads the
patterns' files whole, then for each, waits for bench.ps1's go-<pattern> file in -Dir, resets the
screen (SGR, scrolling region, screen and scrollback cleared, a title saying it's running:
untimed), and writes the file to its console in 64 KiB WriteFile calls, timing itself from the
first write to the last returning. A write returns once the console has taken it, and the console
(ConPTY) only takes more as fast as the terminal reads what it sends on, so the time is bounded by
the terminal, but buffers on the way let the last write return before the terminal has caught up.
So right after the data it sets the window title (OSC 0) to the marker the go file holds:
bench.ps1 watches the window's title for it, and the terminal shows it only once it has taken
everything before it. done-<pattern>.json says what the writes took. Then it waits (-HoldSec) for
bench.ps1 to close the window.

  pwsh -NoProfile -File producer.ps1 -Dir <handshake dir> -Data <data dir> -Patterns plain,wrap
#>
param(
  [Parameter(Mandatory)][string]$Dir,
  [Parameter(Mandatory)][string]$Data,
  [Parameter(Mandatory)][string]$Patterns,
  [int]$Chunk = 65536,
  [int]$HoldSec = 900
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..' 'write-whole.ps1')
try {
  Add-Type -Path (Join-Path $Data 'BenchCon.dll')
  $console = [LbtermBenchCon.Con]::Setup() | ConvertFrom-Json
  $names = @($Patterns -split ',' | Where-Object { $_ })
  $files = @{}
  foreach ($n in $names) { $files[$n] = [IO.File]::ReadAllBytes((Join-Path $Data "$n.txt")) }
  $ready = [ordered]@{ pid = $PID; console = $console; patterns = $names }
  [IO.File]::WriteAllText((Join-Path $Dir 'ready.json'), ($ready | ConvertTo-Json -Compress -Depth 4))
  $e = [char]27
  foreach ($n in $names) {
    $go = Join-Path $Dir "go-$n"
    while (-not [IO.File]::Exists($go)) { Start-Sleep -Milliseconds 5 }
    $marker = ''
    for ($i = 0; $i -lt 50 -and -not $marker; $i++) { try { $marker = [IO.File]::ReadAllText($go).Trim() } catch { Start-Sleep -Milliseconds 5 } }
    [LbtermBenchCon.Con]::Write("$e[0m$e[r$e[H$e[2J$e[3J$e]0;bench running $n$([char]7)")
    $r = [LbtermBenchCon.Con]::Send($files[$n], $Chunk)
    [LbtermBenchCon.Con]::Write("$e]0;$marker$([char]7)")
    $done = [ordered]@{ pattern = $n; start = $r[0]; end = $r[1]; seconds = $r[2] / 1e7; bytes = $r[3]; writes = $r[4] }
    Write-FileWhole -Path (Join-Path $Dir "done-$n.json") -Text ($done | ConvertTo-Json -Compress) -Temp (Join-Path $Dir "done-$n.tmp")
  }
  [LbtermBenchCon.Con]::Write("$e[0m$e[r")
  [IO.File]::WriteAllText((Join-Path $Dir 'finished'), '')
} catch {
  [IO.File]::WriteAllText((Join-Path $Dir 'error.txt'), "$($_.Exception.Message)`n$($_.ScriptStackTrace)")
  throw
}
Start-Sleep -Seconds $HoldSec
