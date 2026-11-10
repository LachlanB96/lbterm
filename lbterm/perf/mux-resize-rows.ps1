#Requires -Version 7
# Variant: -ServerBin installed
# Variant: -GuiBin installed
# Covers: wezterm-client/src/pane/clientpane.rs, wezterm-client/src/pane/renderable.rs, wezterm-client/src/pane/lbterm_lent.rs, wezterm-client/src/pane/lbterm_poll.rs, wezterm-client/src/pane/lbterm_sizes.rs, wezterm-client/src/lbterm_backlog.rs, wezterm-gui/src/termwindow/resize.rs, lbterm-extras/src/resize.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/lbterm_resize.rs, term/src/screen.rs, wezterm-surface/src/line/line.rs
<#
The rows a window holds of each tab after a resize, against the server's, on a mux domain of its
own (lbterm/MUX.md, "A window's resize", "The rows a resize changed"): a tab in view that echoes
keys and -Hidden tabs out of view, each after a screen's worth of lines of mixed lengths (some
longer than the window is wide, so the server's rewrap joins and cuts them), then quiet. A window
takes the server's word for which rows a resize changed (the push it asks for once the server has
answered the resize) and makes none stale itself, so a tab out of view, which no frame draws and
whose pushes go through the window's backlog, must hold the server's rows by the time it's shown.

1. jump: -JumpCols columns narrower in one step, outside a drag (every tab takes it at once); once
   every pane on the server and in the window has it and -QuietMs on, each tab out of view: the
   rows the window holds of it (`cli get-text` from the window's own socket, which reads them as a
   frame does) against the server's (`cli get-text` from the server's), then the tab shown (the
   server's activate-pane) and its rows again once its first frame has drawn it.
2. at-once: the first width again, and each tab out of view shown straight after, before its push
   can have come: once its frame has drawn it, its rows against the server's until they match.
3. drag: the window's frame dragged -Steps columns narrower (Windows' sizing loop, WM_ENTERSIZEMOVE
   to WM_EXITSIZEMOVE, -EveryMs apart), so only the tab in view takes each size and the rest take
   the last at its end (lbterm-extras' resize.rs, Behind); then as 1.

For each, the PDUs the window sent and took (metrics): GetLines (a row fetched), GetPaneRenderChanges
(the pushes asked for, a resize's and the poll's), the pushes, Resize. The tab in view's rows are
checked against the server's too.

It fails, unless -Measure, when a tab's rows in the window don't match the server's -SettleSec after
it's shown, when the window held rows the server doesn't have once the resize had settled and
-QuietMs gone by (parts 1 and 3: the push its resize asked for never came), or when the window
fetched a row (GetLines) in a part: every row these tabs show is one the push brings or the window
held. With -GuiBin (another build's window, which made every row stale at a resize and fetched them
again) only the first is judged, the rest said; with -ServerBin the fetches are said, not judged
(an older server's pushes leave rows out that it brings into view later). mux-resize-rows.json in the output folder has every
number.

  pwsh -File lbterm\perf\mux-resize-rows.ps1 [-Hidden 3] [-JumpCols 30] [-Steps 12] [-QuietMs 1000] [-Measure] [-Bin <build>] [-ServerBin <build>|installed] [-GuiBin <build>|installed]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$Hidden = 3,
  [int]$Cols = 120,
  [int]$Rows = 40,
  [int]$JumpCols = 30,
  [int]$Steps = 12,
  [int]$EveryMs = 16,
  [int]$QuietMs = 1000,
  [double]$SettleSec = 15,
  [string]$Log,
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-resize-rows' -Bin $Bin
# The installed build's files, copied into the test's folder, as perf/mux-resize.ps1 does: one run
# from the install folder would be taken for the daily LBTerm
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

# Eight lines of mixed lengths (two and three times the window's width among them), each its own
# text, then a file saying so; the tab in view then echoes each key, the others wait. Fewer rows
# than the screen at every width the test gives it, so every row it shows was in view when it
# printed: one the window holds, or one the push after a resize brings
function Get-RowsProgram([int]$Tab, [string]$Done, [switch]$Echo) {
  New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$chars = 'abcdefghijklmnopqrstuvwxyz0123456789'
$i = 0
foreach ($len in 30, 200, 50, 150, 10, 260, 40, 100) {
  $i++
  $sb = [Text.StringBuilder]::new("tab XTABX line $i ")
  for ($c = 0; $sb.Length -lt $len; $c++) { [void]$sb.Append($chars[($c + 7 * $i + XTABX) % $chars.Length]) }
  [Console]::Out.Write($sb.ToString() + "`r`n")
}
[Console]::Out.Write("tab XTABX ready`r`n")
Set-Content -LiteralPath 'XDONEX' 'printed'
if (XECHOX) {
  [Console]::Out.Write('> ')
  while ($true) {
    $k = [Console]::ReadKey($true)
    if ($k.KeyChar -eq [char]13) { [Console]::Out.Write("`r`n> ") } else { [Console]::Out.Write($k.KeyChar) }
  }
}
Start-Sleep 3600
'@ -replace 'XTABX', $Tab -replace 'XDONEX', $Done -replace 'XECHOX', $(if ($Echo) { '$true' } else { '$false' }))
}

$cfg = Join-Path $T.Out 'config\mux-resize-rows.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.unix_domains = { {
  name = 'rowsmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

function Get-ActivePane { @((Get-StateWindow (Read-State) -Hwnd ([long](Resolve-Handle $script:W))).panes | Where-Object active)[0] }

function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}

function Get-PduCounts {
  $m = Get-Metrics $script:W
  [ordered]@{
    GetLines = Get-Pdus $m 'GetLines' 'sent'
    Asked = Get-Pdus $m 'GetPaneRenderChanges' 'sent'
    Pushes = Get-Pdus $m 'GetPaneRenderChangesResponse' 'received'
    Resize = Get-Pdus $m 'Resize' 'sent'
  }
}

# A pane's rows in view, each without its trailing blanks, the blank rows at the end left out. The
# last row asked for is the screen's last: get-text's default end is one row past it (its
# --end-line is inclusive and defaults to the screen's height), a row neither side has in view,
# which the window fetches the first time it's read
function Get-Rows([int]$Pane, [switch]$Server) {
  $last = [int]@(Get-Panes -Server | Where-Object pane_id -eq $script:Echo.Server)[0].size.rows - 1
  $lines = @(Invoke-Cli get-text --pane-id $Pane --start-line 0 --end-line $last -Server:$Server | ForEach-Object { "$_".TrimEnd() })
  $n = $lines.Count
  while ($n -gt 0 -and -not $lines[$n - 1]) { $n-- }
  , @($lines | Select-Object -First $n)
}

# Where the window's rows of a pane differ from the server's: $null when they match
function Compare-Rows([int]$Gui, [int]$ServerPane) {
  $mine = Get-Rows $Gui
  $theirs = Get-Rows $ServerPane -Server
  for ($r = 0; $r -lt [Math]::Max($mine.Count, $theirs.Count); $r++) {
    $a = if ($r -lt $mine.Count) { $mine[$r] } else { '' }
    $b = if ($r -lt $theirs.Count) { $theirs[$r] } else { '' }
    if ($a -ne $b) { return "row $r of $($theirs.Count): the window has '$a', the server '$b'" }
  }
  $null
}

# Every pane, on the server and in the window, at the size the tab in view has once its width is
# no longer $OldCols: how long it took, or $null
function Wait-Sized([int]$OldCols) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt $SettleSec) {
    $p = Get-ActivePane
    if ($p -and [int]$p.cols -ne $OldCols) {
      $off = @(@(Get-Panes -Server) + @(Get-Panes -Window $script:W) | Where-Object { $_.size.cols -ne [int]$p.cols -or $_.size.rows -ne [int]$p.rows })
      if (-not $off.Count) { return [Math]::Round($clock.Elapsed.TotalMilliseconds, 0) }
    }
    Start-Sleep -Milliseconds 20
  }
  $null
}

# Tab $Tab shown, and how long after its first frame its rows match the server's: $null with what
# still differed after -SettleSec
function Show-Tab($Tab) {
  $sid = $Tab.Server
  Invoke-Cli activate-pane --pane-id $sid -Server -Check | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $sid -and $_.drawn_cursor }).Count -eq 1 } -What "tab $($Tab.N) in view" -TimeoutSec 15 | Out-Null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $diff = Compare-Rows $Tab.Gui $sid
  $first = $diff
  while ($diff -and $clock.Elapsed.TotalSeconds -lt $SettleSec) {
    Start-Sleep -Milliseconds 50
    $diff = Compare-Rows $Tab.Gui $sid
  }
  [ordered]@{ Tab = $Tab.N; AtFirstFrame = $first; MatchedMs = if ($diff) { $null } else { [Math]::Round($clock.Elapsed.TotalMilliseconds, 0) }; Left = $diff }
}

function Show-Echo {
  Invoke-Cli activate-pane --pane-id $script:Echo.Server -Server -Check | Out-Null
  Wait-State { param($s) $p = @($s.panes | Where-Object active)[0]; $p -and $p.server_id -eq $script:Echo.Server -and -not $s.animating } -What 'the echoing tab in view' -TimeoutSec 15 | Out-Null
}

# One part: $Resize sizes the window, then each tab out of view is looked at and shown
function Measure-Part([string]$Name, [scriptblock]$Resize, [switch]$AtOnce) {
  Show-Echo
  Start-Sleep -Milliseconds 300
  $old = [int](Get-ActivePane).cols
  Get-Metrics $script:W -Reset | Out-Null
  & $Resize
  $d = [ordered]@{ Name = $Name; AtOnce = [bool]$AtOnce; SizedMs = $null; Held = @(); Shown = @() }
  if (-not $AtOnce) {
    $d.SizedMs = Wait-Sized $old
    Start-Sleep -Milliseconds $QuietMs
    $d.InView = Compare-Rows $script:Echo.Gui $script:Echo.Server
    foreach ($tab in $script:Tabs) { $d.Held += [ordered]@{ Tab = $tab.N; Diff = Compare-Rows $tab.Gui $tab.Server } }
    $d.AfterHeld = Get-PduCounts
  }
  foreach ($tab in $script:Tabs) { $d.Shown += Show-Tab $tab }
  if ($AtOnce) { $d.SizedMs = Wait-Sized $old }
  Show-Echo
  $d.InViewAfter = Compare-Rows $script:Echo.Gui $script:Echo.Server
  $d.Pdus = Get-PduCounts
  $held = @($d.Held | Where-Object Diff).Count
  $late = @($d.Shown | Where-Object { $null -eq $_.MatchedMs }).Count
  $firstOff = @($d.Shown | Where-Object AtFirstFrame).Count
  "$Name$(if ($AtOnce) { ' (each tab shown at once)' }): sized in $($d.SizedMs) ms; $(if (-not $AtOnce) { "$held of $Hidden tabs out of view held rows the server hasn't, $QuietMs ms on; " })$firstOff of $Hidden showed a row unlike the server's at their first frame, $late never matched; PDUs: $($d.Pdus.GetLines) GetLines, $($d.Pdus.Asked) GetPaneRenderChanges, $($d.Pdus.Pushes) pushes, $($d.Pdus.Resize) Resize" | Out-Host
  $d
}

$results = [ordered]@{ Window = $guiExe; Server = $serverExe; Locked = (Test-WorkstationLocked); Hidden = $Hidden; Parts = @() }

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Domain 'rowsmux' -Socket $T.Sock -TimeoutSec 30 -Log $Log -Exe $guiExe
  $script:W = $w
  if ($Log) { "the window's log: $($w.Log)" | Out-Host }
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $shell = $panes[0].pane_id
  $dones = @(0..$Hidden | ForEach-Object { Join-Path $T.Out "tab-$_.done" })
  $echoId = New-Tab -From $shell -Program (Get-RowsProgram 0 $dones[0] -Echo) -Server
  $ids = @(1..$Hidden | ForEach-Object { New-Tab -From $echoId -Program (Get-RowsProgram $_ $dones[$_]) -NoActivate -Server })
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq $Hidden + 1 } -What "the $($Hidden + 1) tabs" -TimeoutSec 60 | Out-Null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (@($dones | Where-Object { -not (Test-Path -LiteralPath $_) }).Count -and $clock.Elapsed.TotalSeconds -lt 60) { Start-Sleep -Milliseconds 100 }
  if (@($dones | Where-Object { -not (Test-Path -LiteralPath $_) }).Count) { throw "the tabs hadn't printed their lines in 60 s" }
  # Each tab shown once, as Lachlan's have been, for the window's own id of its pane
  $script:Tabs = @()
  for ($i = 0; $i -lt $Hidden; $i++) {
    $sid = $ids[$i]
    Invoke-Cli activate-pane --pane-id $sid -Server -Check | Out-Null
    $s = Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $sid -and $_.drawn_cursor }).Count -eq 1 } -What "tab $($i + 1) in view" -TimeoutSec 15
    $gid = [int]@((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).panes | Where-Object server_id -eq $sid)[0].id
    $script:Tabs += [pscustomobject]@{ N = $i + 1; Server = $sid; Gui = $gid }
  }
  Invoke-Cli activate-pane --pane-id $echoId -Server -Check | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $echoId -and $_.drawn_cursor }).Count -eq 1 } -What 'the echoing tab in view' -TimeoutSec 15 | Out-Null
  $script:Echo = [pscustomobject]@{ N = 0; Server = $echoId; Gui = [int]@((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).panes | Where-Object server_id -eq $echoId)[0].id }
  Start-Sleep 1
  foreach ($tab in @($script:Echo) + $script:Tabs) {
    $diff = Compare-Rows $tab.Gui $tab.Server
    if ($diff) { throw "before any resize, tab $($tab.N)'s rows in the window weren't the server's: $diff" }
  }

  $rect = Get-WindowRect $w
  $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
  $p0 = Get-ActivePane
  $cell = [Math]::Max(4, [int][Math]::Ceiling($p0.rect.w / $p0.cols))
  $results.Parts += Measure-Part 'jump' { Set-WindowSize $script:W ($width - $JumpCols * $cell) $height }
  $results.Parts += Measure-Part 'at-once' { Set-WindowSize $script:W $width $height } -AtOnce
  $results.Parts += Measure-Part 'drag' {
    Send-SizeMove $script:W
    for ($k = 1; $k -le $Steps; $k++) { Set-WindowSize $script:W ($width - $k * $cell) $height; Start-Sleep -Milliseconds $EveryMs }
    Send-SizeMove $script:W -End
  }
  $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-resize-rows.json')

  $wrong = [Collections.Generic.List[string]]::new()
  $said = [Collections.Generic.List[string]]::new()
  foreach ($d in $results.Parts) {
    foreach ($s in $d.Shown) { if ($null -eq $s.MatchedMs) { $wrong.Add("$($d.Name): tab $($s.Tab)'s rows weren't the server's $SettleSec s after it was shown: $($s.Left)") } }
    if ($d.InViewAfter) { $wrong.Add("$($d.Name): the tab in view's rows weren't the server's: $($d.InViewAfter)") }
    # Not `$own = if (...) { $said } else { $wrong }`: an if hands an empty list on as nothing
    # (PITFALLS 143)
    $own = $wrong
    if ($GuiBin) { $own = $said }
    foreach ($h in $d.Held) { if ($h.Diff) { $own.Add("$($d.Name): tab $($h.Tab) out of view held rows the server hasn't $QuietMs ms after the resize settled: $($h.Diff)") } }
    # An older server's pushes are its own: it names every row a rewrap went over, and the rows
    # it leaves out of view and brings in later aren't this window's to have held
    if ($ServerBin) { $own = $said }
    if ($d.Pdus.GetLines) { $own.Add("$($d.Name): the window fetched rows $($d.Pdus.GetLines) times (GetLines) that the push brings or it held") }
  }
  foreach ($x in $said) { (Get-Test).Notes.Add("not judged with -GuiBin: $x") }
  if (-not $Measure) {
    foreach ($x in $wrong) { Add-Failure $x }
  } else {
    (Get-Test).PassedLine = "mux-resize-rows: measured, not judged (-Measure)$(if ($wrong.Count) { "; WOULD FAIL: $($wrong -join '; ')" } else { '; every tab shown with the server''s rows' })"
  }
} -Passed "every tab resized out of view shown with the server's rows, none fetched again"
