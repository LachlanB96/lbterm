#Requires -Version 7
# Covers: mux/src/localpane.rs, wezterm/src/cli/list.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
Times `lbterm cli list` against a test window with many tabs: how long a cli request holds up
LBTerm's GUI thread, which also takes every keypress.

  pwsh -File lbterm\perf\cli-list.ps1                                   # this tree's build
  pwsh -File lbterm\perf\cli-list.ps1 -Bin "$env:LOCALAPPDATA\Programs\LBTerm"   # the installed one

Opens <Bin>\lbterm-gui.exe without a config (-n), with Start-Unfocused (no console, outside
Claude's job, never taking focus) on the right-most screen (offscreen.ps1). It spawns -Tabs tabs, each
a pwsh running a node child like a Claude session, so every pane has a process tree to walk and
none reports its folder by OSC 7. Then it runs `cli list` -Calls times, -GapMs apart (longer than
the 300 ms process-list cache), prints the median, 90th percentile and worst, and closes the window.
#>
param(
  [string]$Bin,
  [int]$Tabs = 18,
  [int]$Calls = 20,
  [int]$GapMs = 500
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'cli-list' -Bin $Bin
$node = 'C:\Program Files\nodejs\node.exe'

Invoke-Test {
  # Open-TestWindow points `lbterm cli` at this GUI, not the terminal this shell runs in
  $w = Open-TestWindow -NoConfig -Program (New-PwshProgram)
  # Painted: its pane is there to spawn beside (was 1 s)
  Wait-State { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -What 'the window' | Out-Null
  # The window never has focus (LBTERM_NO_ACTIVATE), so spawn has to be told which window
  $first = (Get-Panes)[0].pane_id
  for ($n = 1; $n -lt $Tabs; $n++) {
    New-Tab -From $first -Program (New-PwshProgram "& '$node' -e 'setInterval(()=>{},1e9)'") | Out-Null
  }
  # Every tab in the window, and each one's node running under its pwsh, so every pane has its
  # whole tree to walk (was 3 s)
  Wait-State { param($x) @($x.tabs).Count -eq $Tabs } -What "$Tabs tabs" | Out-Null
  Wait-Until {
    $shells = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($w.Pid) AND Name = 'pwsh.exe'" | ForEach-Object ProcessId)
    $nodes = @(Get-CimInstance Win32_Process -Filter "Name = 'node.exe'" | Where-Object { $_.ParentProcessId -in $shells })
    $nodes.Count -ge $Tabs - 1
  } -TimeoutSec 15 -IntervalMs 250 | Out-Null
  $panes = (Get-Panes).Count

  # The cli itself, timed, as a caller would run it (Invoke-Cli's own work left out)
  $ms = foreach ($c in 1..$Calls) {
    Start-Sleep -Milliseconds $GapMs
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $T.Cli cli --no-auto-start list --format json | Out-Null
    $sw.Stop()
    $sw.Elapsed.TotalMilliseconds
  }
  # The cli's own start-up, for comparison: a request the GUI answers without touching panes
  $baseline = foreach ($c in 1..5) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & $T.Cli cli --no-auto-start list-clients | Out-Null
    $sw.Stop()
    $sw.Elapsed.TotalMilliseconds
  }

  # Answering from the cache mustn't lose anything: every pane still has its folder
  $withCwd = @(Get-Panes | Where-Object { $_.cwd }).Count

  $sorted = @($ms | Sort-Object)
  $pick = { param($q) [Math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int][Math]::Floor($q * $sorted.Count))], 1) }
  [pscustomobject]@{
    Bin            = $T.Bin
    Panes          = $panes
    PanesWithCwd   = $withCwd
    MedianMs       = & $pick 0.5
    P90Ms          = & $pick 0.9
    WorstMs        = [Math]::Round(($sorted | Select-Object -Last 1), 1)
    ListClientsMs  = [Math]::Round((@($baseline | Sort-Object))[2], 1)
  }
}
