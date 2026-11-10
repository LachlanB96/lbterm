#Requires -Version 7
# Variant: -GuiBin installed
# Variant: -ServerBin installed
# Covers: wezterm-gui/src/termwindow/resize.rs, wezterm-gui/src/termwindow/mod.rs, lbterm-extras/src/resize.rs, window/src/os/windows/window.rs, wezterm-client/src/pane/clientpane.rs, wezterm-client/src/pane/lbterm_sizes.rs, wezterm-client/src/domain.rs, wezterm-client/src/client.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/lbterm_resize.rs, mux/src/tab.rs, mux/src/localpane.rs, term/src/screen.rs, term/src/terminalstate/mod.rs, wezterm-surface/src/line/line.rs, wezterm-surface/src/line/clusterline.rs
<#
A window resized with every tab's scrollback full, on a mux domain of its own (lbterm/MUX.md,
"A window's resize"; the audit's server10 and core-9): -Tabs tabs, each after -History lines of
scrollback (a third of them longer than the window is wide, so they wrap, and coloured). Each
size a tab takes goes to each of its panes: on the server the pane's terminal fits its whole
scrollback to the new width (rewrap), and the window and the server tell each other about it.

1. drag: the window's frame dragged, as Windows' sizing loop does it (WM_ENTERSIZEMOVE, then the
   sizes, then WM_EXITSIZEMOVE): one column narrower -Steps times, -EveryMs apart, then one
   column wider -Steps times back to where it was.
2. jump: -JumpCols columns narrower in one step, outside a drag, as maximizing, restoring or
   snapping a window does: every tab takes it at once.
3. back: the window's first size again, the same way.
-NoDrag leaves the drag out: a build whose drag keeps its server busy for minutes (the installed
build of 2026-11-05) would have its jumps measured behind it.

For each, from its first size to every pane settled at its last, and 2 s on:
- the server's main thread and its CPU, the window's GUI thread and its CPU (by their cycles);
- the PDUs the window sent and took (metrics): Resize, ListPanes (each a resync of every tab),
  TabResized, GetLines, the pushes;
- how long the window's thread went without taking a message (Start-HangWatch);
- how long after the last size every pane, on the server and in the window, had it (settled);
- the tab in view's echo of a key posted straight after the last size, to the frame that shows it
  (the state file's cursor): what the server's main thread had queued before it.

It fails, unless -Measure, when a pane on the server or in the window hasn't the window's size
-SettleSec after the last size, when the key isn't echoed by then, or when the window's thread
went -HangMs without a message. With -Measure nothing is judged, and its last line says so, with
each part that didn't settle, isn't a pass (PITFALLS 11). mux-resize.json in the output folder
has every number.

-ServerBin runs another build's lbterm-mux-server.exe for the domain, and -GuiBin another build's
lbterm-gui.exe for the window (installed: a copy of the daily LBTerm's, in the output folder, as
perf/mux-load.ps1 makes it), so each pairing of an older window or server with this build is
resized as this build's pair is, and judged the same: the wire and the habits of either side
have to hold with the other's (PITFALLS 159). The e2e runner runs both against the installed
build.

  pwsh -File lbterm\perf\mux-resize.ps1 [-Tabs 12] [-History 50000] [-Steps 20] [-EveryMs 16] [-JumpCols 30] [-Jumps 1] [-NoDrag] [-Measure] [-Bin <build>] [-ServerBin <build>|installed] [-GuiBin <build>|installed]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$Tabs = 12,
  [int]$History = 50000,
  [int]$Cols = 120,
  [int]$Rows = 50,
  [int]$Steps = 20,
  [int]$EveryMs = 16,
  [double]$SettleSec = 90,
  [double]$HangMs = 2000,
  [double]$MaxMemGB = 4,
  [int]$JumpCols = 30,
  # The jump and back this many times (the last pair's numbers kept), to keep the server busy
  # for a sampling profiler beside it (perf/sample.ps1)
  [int]$Jumps = 1,
  # The jumps alone, with no drag before them: a build whose drag leaves its server busy for
  # minutes would have its jumps measured behind it
  [switch]$NoDrag,
  # WEZTERM_LOG for the window (lbterm::resize=debug says what each size went to)
  [string]$Log,
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-resize' -Bin $Bin
# The installed build's $Files, copied into the test's folder: a server or window running from the
# install folder would be taken for the daily one (the Warden's registry and `lbterm.ps1 logs` go
# by the exe's folder)
function Copy-Installed([string]$Name, [string[]]$Files) {
  $dir = Join-Path $T.Out "installed-$Name"
  New-Item -ItemType Directory -Force $dir | Out-Null
  foreach ($f in $Files) { Copy-Item -Recurse (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $dir }
  $dir
}
if ($ServerBin -eq 'installed') { $ServerBin = Copy-Installed 'server' 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe' }
if ($GuiBin -eq 'installed') { $GuiBin = Copy-Installed 'gui' 'lbterm-gui.exe', 'conpty.dll', 'OpenConsole.exe', 'libEGL.dll', 'libGLESv2.dll', 'mesa' }
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$guiExe = if ($GuiBin) { Join-Path (Resolve-Path $GuiBin).Path 'lbterm-gui.exe' } else { $T.Gui }
if (-not (Test-Path $guiExe)) { throw "no lbterm-gui.exe in $GuiBin" }
$rate = Get-CycleRate
$cyclesMs = { param($c) [Math]::Round($c / $rate, 1) }

# -History lines, every third longer than the window and coloured, then a file saying so; the tab
# in view then echoes each key it's given, the others wait
function Get-HistoryProgram([string]$Done, [switch]$Echo) {
  New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$n = XLINESX
$long = 'y' * 140
$sb = [Text.StringBuilder]::new()
for ($i = 1; $i -le $n; $i++) {
  if ($i % 3 -eq 0) { [void]$sb.Append("$e[38;2;$(40 + $i % 200);180;$(90 + $i % 160)mhistory line $i of $n$e[0m, $long`n") }
  else { [void]$sb.Append("history line $i of $n, as a build or a log leaves it`n") }
  if ($sb.Length -gt 1MB) { [Console]::Out.Write($sb.ToString()); [void]$sb.Clear() }
}
[Console]::Out.Write($sb.ToString())
Set-Content -LiteralPath 'XDONEX' 'printed'
if (XECHOX) {
  [Console]::Out.Write("`r`n> ")
  while ($true) {
    $k = [Console]::ReadKey($true)
    if ($k.KeyChar -eq [char]13) { [Console]::Out.Write("`r`n> ") } else { [Console]::Out.Write($k.KeyChar) }
  }
}
Start-Sleep 3600
'@ -replace 'XLINESX', $History -replace 'XDONEX', $Done -replace 'XECHOX', $(if ($Echo) { '$true' } else { '$false' }))
}

$cfg = Join-Path $T.Out 'config\mux-resize.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.scrollback_lines = $([Math]::Max(1000, $History))
config.unix_domains = { {
  name = 'resizemux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

# A process's first thread, its main one
function Get-MainThread([int]$ProcessId) {
  $p = Get-Process -Id $ProcessId
  $first = $null
  foreach ($t in $p.Threads) {
    try { if (-not $first -or $t.StartTime -lt $first.StartTime) { $first = $t } } catch { }
  }
  [int]$first.Id
}

function Get-Mark($w) {
  $snap = Get-CpuSnapshot $w -Servers @($w.ServerPid)
  $main = @($snap.ServerThreads | Where-Object { $_.Tid -eq $w.ServerMain })[0]
  [pscustomobject]@{ At = $snap.At; Thread = $snap.ThreadCycles; Gui = $snap.Cycles; Server = @($snap.Servers)[0].Cycles; Main = if ($main) { $main.Cycles } else { 0 } }
}

function Get-Spent($a, $b) {
  [ordered]@{
    Seconds = [Math]::Round(($b.At - $a.At) / [Diagnostics.Stopwatch]::Frequency, 2)
    GuiThreadMs = & $cyclesMs ($b.Thread - $a.Thread); GuiMs = & $cyclesMs ($b.Gui - $a.Gui)
    ServerMs = & $cyclesMs ($b.Server - $a.Server); ServerMainMs = & $cyclesMs ($b.Main - $a.Main)
  }
}

function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

# Each pane's size as the server has it and as the window has it, against the size wanted: the
# panes that differ, named
function Get-Unsettled($w, [int]$WantCols, [int]$WantRows) {
  $off = @()
  foreach ($p in @(Get-Panes -Server)) {
    if ($p.size.cols -ne $WantCols -or $p.size.rows -ne $WantRows) { $off += "server pane $($p.pane_id) $($p.size.cols)x$($p.size.rows)" }
  }
  foreach ($p in @(Get-Panes -Window $w)) {
    if ($p.size.cols -ne $WantCols -or $p.size.rows -ne $WantRows) { $off += "window pane $($p.pane_id) $($p.size.cols)x$($p.size.rows)" }
  }
  , $off
}

function Get-ActivePane { @((Get-StateWindow (Read-State) -Hwnd ([long](Resolve-Handle $script:W))).panes | Where-Object active)[0] }

# The window given each of $Sizes (width, height) -EveryMs apart, inside Windows' sizing loop with
# -Live (WM_ENTERSIZEMOVE first and WM_EXITSIZEMOVE after, sent as the loop sends them, so the
# window takes them as a drag of its frame), then a key: how long after the last size the key was
# echoed and every pane, on the server and in the window, had the window's size (-WantCols by
# -WantRows, or with -WantCols 0 the size its tab in view has once the key is echoed), and what the server, the window and the PDUs between them did until then and 2 s on
function Measure-Sizes($w, [object[]]$Sizes, [switch]$Live, [string]$Name, [int]$WantCols, [int]$WantRows) {
  Wait-ModifiersUp | Out-Null
  $x0 = [int](Get-ActivePane).cursor.x
  Get-Metrics $w -Reset | Out-Null
  $a = Get-Mark $w
  $hang = Start-HangWatch $w
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $sizeMs = @()
  if ($Live) { Send-SizeMove $w }
  for ($k = 0; $k -lt $Sizes.Count; $k++) {
    while ($clock.Elapsed.TotalMilliseconds -lt $k * $EveryMs) { [Threading.Thread]::Sleep(1) }
    $t0 = $clock.Elapsed.TotalMilliseconds
    Set-WindowSize $w $Sizes[$k][0] $Sizes[$k][1]
    $sizeMs += $clock.Elapsed.TotalMilliseconds - $t0
  }
  if ($Live) { Send-SizeMove $w -End }
  # A key straight after the last size: how long until the tab in view echoes it, which waits for
  # whatever the server's main thread has queued before it
  $lastMs = $clock.Elapsed.TotalMilliseconds
  Send-Key $w X
  # The key's echo (the state file, which the window writes each frame), then settled: every pane
  # on the server and in the window at the window's size again (cli list on each, which takes
  # longer than a frame, so it's asked only once the echo is in)
  $echoMs = $null; $settled = $null; $off = @(); $atSettle = $null
  while ($clock.Elapsed.TotalMilliseconds - $lastMs -lt $SettleSec * 1000) {
    $p = Get-ActivePane
    if ($p -and [int]$p.cursor.x -gt $x0) { $echoMs = $clock.Elapsed.TotalMilliseconds - $lastMs; break }
    Start-Sleep -Milliseconds 5
  }
  # The window's size in cells, as its tab in view has it, when the caller doesn't know it
  if ($WantCols -le 0) {
    $p = Get-ActivePane
    $WantCols = [int]$p.cols; $WantRows = [int]$p.rows
  }
  do {
    $off = Get-Unsettled $w $WantCols $WantRows
    if (-not $off.Count) { $settled = $clock.Elapsed.TotalMilliseconds - $lastMs; $atSettle = Get-Mark $w; break }
    Start-Sleep -Milliseconds 50
  } while ($clock.Elapsed.TotalMilliseconds - $lastMs -lt $SettleSec * 1000)
  if (-not $atSettle) { $atSettle = Get-Mark $w }
  Start-Sleep 2
  $b = Get-Mark $w
  $watch = Stop-HangWatch $hang
  $m = Get-Metrics $w
  $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out "metrics-$Name.json")
  $posChanged = @($m.messages | Where-Object site -eq 'WM_WINDOWPOSCHANGED')[0]
  $d = [ordered]@{
    Sizes = $Sizes.Count; Live = [bool]$Live; SizingMs = [Math]::Round($lastMs, 0)
    SetWindowPosMsMax = [Math]::Round(($sizeMs | Measure-Object -Maximum).Maximum, 1)
    SetWindowPosMsP50 = [Math]::Round((Get-Pct $sizeMs 0.5), 1)
    EchoMs = if ($null -ne $echoMs) { [Math]::Round($echoMs, 0) } else { $null }
    SettledMs = if ($null -ne $settled) { [Math]::Round($settled, 0) } else { $null }
    Unsettled = $off
    ToSettle = Get-Spent $a $atSettle
    WithTail = Get-Spent $a $b
    WindowPosChangedMs = if ($posChanged) { [double]$posChanged.total_ms } else { $null }
    WindowPosChangedMaxMs = if ($posChanged) { [double]$posChanged.max_ms } else { $null }
    Resize = Get-Pdus $m 'Resize' 'sent'
    ListPanes = Get-Pdus $m 'ListPanes' 'sent'
    TabResized = Get-Pdus $m 'TabResized' 'received'
    GetLines = Get-Pdus $m 'GetLines' 'sent'
    Pushes = Get-Pdus $m 'GetPaneRenderChangesResponse' 'received'
    # The asks for a pane's push, a Resize's answered among them (FLOWS.md's resize flow, What could go 9)
    Asks = Get-Pdus $m 'GetPaneRenderChanges' 'sent'
    Frames = [int]$m.totals.frames
    Hang = $watch
  }
  "$Name ($($d.Sizes) sizes$(if ($Live) { " $EveryMs ms apart, a drag of the frame" }), $Tabs tabs each after $History lines): a key echoed $(if ($null -ne $d.EchoMs) { "$($d.EchoMs) ms" } else { 'never' }) after the last size, every pane settled $(if ($null -ne $d.SettledMs) { "$($d.SettledMs) ms" } else { 'never' }) after it; to then the server's main thread $($d.ToSettle.ServerMainMs) ms (its CPU $($d.ToSettle.ServerMs)), the GUI thread $($d.ToSettle.GuiThreadMs) ms (its process $($d.ToSettle.GuiMs)) over $($d.ToSettle.Seconds) s; 2 s on: server main $($d.WithTail.ServerMainMs), GUI thread $($d.WithTail.GuiThreadMs); WM_WINDOWPOSCHANGED $($d.WindowPosChangedMs) ms ($($d.WindowPosChangedMaxMs) at most), SetWindowPos $($d.SetWindowPosMsP50) ms at the median, $($d.SetWindowPosMsMax) at most; PDUs: $($d.Resize) Resize, $($d.ListPanes) ListPanes, $($d.TabResized) TabResized, $($d.GetLines) GetLines, $($d.Pushes) pushes, $($d.Asks) asked; $($d.Frames) frames; the longest wait for a message $($d.Hang.LongestMs) ms (p99 $($d.Hang.P99Ms), $($d.Hang.Over100Ms) over 100 ms)" | Out-Host
  $d
}

$results = [ordered]@{ Window = $guiExe; Server = $serverExe; Locked = (Test-WorkstationLocked); Tabs = $Tabs; History = $History; Steps = 2 * $Steps; EveryMs = $EveryMs }

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Domain 'resizemux' -Socket $T.Sock -TimeoutSec 30 -Log $Log -Exe $guiExe
  $script:W = $w
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue ([int]$server[0].ProcessId)
  $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue (Get-MainThread $w.ServerPid)
  Start-MemoryGuard -Pids $w.Pid, $w.ServerPid -LimitGB $MaxMemGB
  $shell = $panes[0].pane_id
  $dones = @(1..$Tabs | ForEach-Object { Join-Path $T.Out "tab-$_.done" })
  $echo = New-Tab -From $shell -Program (Get-HistoryProgram $dones[0] -Echo) -Server
  for ($i = 1; $i -lt $Tabs; $i++) { New-Tab -From $echo -Program (Get-HistoryProgram $dones[$i]) -NoActivate -Server | Out-Null }
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq $Tabs } -What "the $Tabs tabs" -TimeoutSec 60 | Out-Null
  Invoke-Cli activate-pane --pane-id $echo -Server -Check | Out-Null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (@($dones | Where-Object { -not (Test-Path -LiteralPath $_) }).Count -and $clock.Elapsed.TotalSeconds -lt 300) { Start-Sleep -Milliseconds 200 }
  $left = @($dones | Where-Object { -not (Test-Path -LiteralPath $_) }).Count
  if ($left) { throw "$left of the $Tabs tabs hadn't printed their $History lines in 300 s" }
  $results.PrintedSec = [Math]::Round($clock.Elapsed.TotalSeconds, 1)
  Wait-State { param($s) $p = @($s.panes | Where-Object active)[0]; $p -and $p.server_id -eq $echo -and -not $s.animating } -What 'the echoing tab in view' -TimeoutSec 15 | Out-Null
  Start-Sleep 3

  $p0 = Get-ActivePane
  $wantCols = [int]$p0.cols; $wantRows = [int]$p0.rows
  $off = Get-Unsettled $w $wantCols $wantRows
  if ($off.Count) { throw "before the drag the panes weren't all at the window's $($wantCols)x$($wantRows): $($off -join ', ')" }
  $rect = Get-WindowRect $w
  $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
  $cell = [Math]::Max(4, [int][Math]::Ceiling($p0.rect.w / $p0.cols))
  $sizes = @(1..$Steps | ForEach-Object { $width - $_ * $cell }) + @(($Steps - 1)..0 | ForEach-Object { $width - $_ * $cell })

  if (-not $NoDrag) {
    $results.Drag = Measure-Sizes $w ($sizes | ForEach-Object { , @($_, $height) }) -Live -Name 'drag' -WantCols $wantCols -WantRows $wantRows
  }
  # One size and back, outside a drag (as maximizing, restoring or snapping gives): every tab
  # takes each, so this is what a pane's resize costs, twelve times over
  for ($j = 0; $j -lt $Jumps; $j++) {
    $results.Jump = Measure-Sizes $w @(, @(($width - $JumpCols * $cell), $height)) -Name 'jump' -WantCols 0
    $results.Back = Measure-Sizes $w @(, @($width, $height)) -Name 'back' -WantCols $wantCols -WantRows $wantRows
  }
  $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-resize.json')
  # What each part would fail on: judged, unless -Measure, which says them and passes nothing
  $wrong = [Collections.Generic.List[string]]::new()
  foreach ($part in 'Drag', 'Jump', 'Back') {
    $d = $results[$part]
    if (-not $d) { continue }
    if ($null -eq $d.SettledMs) { $wrong.Add("$($part.ToLower()): $SettleSec s after its last size the panes weren't all at the window's size: $($d.Unsettled -join ', ')") }
    if ($null -eq $d.EchoMs) { $wrong.Add("$($part.ToLower()): a key posted after its last size wasn't echoed in $SettleSec s") }
    if ($d.Hang.LongestMs -gt $HangMs) { $wrong.Add("$($part.ToLower()): the window's thread went $($d.Hang.LongestMs) ms without taking a message (over $HangMs)") }
  }
  if (-not $Measure) {
    foreach ($x in $wrong) { Add-Failure $x }
  } else {
    (Get-Test).PassedLine = "mux-resize: measured, not judged (-Measure)$(if ($wrong.Count) { "; NOT SETTLED: $($wrong -join '; ')" } else { '; every part settled and echoed' })"
  }
} -Passed "every pane at the window's size after the drag and each jump"
