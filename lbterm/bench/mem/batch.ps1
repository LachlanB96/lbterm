# Runs experiments from a list, one window at a time, each a pwsh of its own (run.ps1 exits)
param([Parameter(Mandatory)][string]$List, [int]$Repeat = 1)
$s = Split-Path $PSScriptRoot -Parent
$bin = Join-Path $s 'bin'
$outRoot = Join-Path $s 'out'
$pwsh = 'C:/Program Files/PowerShell/7/pwsh.exe'
$experiments = Get-Content -Raw (Join-Path $PSScriptRoot $List) | ConvertFrom-Json
for ($rep = 1; $rep -le $Repeat; $rep++) {
  foreach ($e in $experiments) {
    $name = if ($Repeat -gt 1) { "$($e.name)-r$rep" } else { $e.name }
    $a = @('-NoProfile', '-File', (Join-Path $PSScriptRoot 'run.ps1'), '-Name', $name, '-Bin', $bin, '-OutRoot', $outRoot, '-Config', $e.config, '-Scan')
    if ($e.mux) { $a += '-Mux' }
    if ($e.lua) { $a += @('-Lua', $e.lua) }
    if ($e.scrollback) { $a += @('-Scrollback', $e.scrollback) }
    if ($e.log) { $a += @('-Log', $e.log) }
    if ($e.steps) { $a += '-Steps'; $a += ($e.steps -join ',') }
    "=== $name $(Get-Date -Format 'HH:mm:ss')"
    & $pwsh @a 2>&1 | Where-Object { $_ -notmatch '^NOTE: another LBTerm test window' } | ForEach-Object { "  $_" }
  }
}
"=== done $(Get-Date -Format 'HH:mm:ss')"
