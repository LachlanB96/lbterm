#Requires -Version 7
# Variant: -Only pointer,write -ServerBin installed -Compat
# Covers: wezterm-client/src/pane/*.rs, wezterm-gui/src/termwindow/render/pane_lines.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/mod.rs, wezterm-gui/src/termwindow/mouseevent.rs, term/src/terminalstate/mod.rs, mux/src/localpane.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
What the window's own side of a mux domain's pane costs (lbterm/MUX.md, "A frame of a pane on the
mux domain" and "The pointer over a pane"): a frame, a pointer move, a write, a line fetched and
a resize, each part in a test mux server of its own (a socket under the test's folder; never
Lachlan's), its window drawn as Lachlan's (New-TestConfig). Each part opens its own window:

1. frame: a screen of -Rows lines of 24-bit colour text, a URL and a path on each, its last row's
   spinner drawn again every -SpinMs, for -Seconds, in a window on the mux domain and then in one
   whose pane is its own: the frames, the GUI thread's time a frame, metrics' paint_pane.lines (the
   lines' part of a pane's paint) and the line state cache's hits a frame: a row whose shape key
   (render/mod.rs, line_shape_hash) is kept on its line is a hit, twice a frame (once where it's
   held, once on its copy), and one hashed again isn't. Until perf/mux-client every row of a mux
   pane in view was hashed again on every frame (its key went with a copy of the line): no hits.
2. pointer: -Moves pointer moves posted across a pane -MoveEveryMs apart, the window told it has
   the keyboard (a move goes to a pane only then), over a shell's pane and over a program that has
   the mouse (1000, 1002, 1003 and 1006, as Claude Code's fullscreen view asks), across its cells
   and then inside one of them: the SendMouseEvent PDUs the window sent for each, its GUI thread's
   time and the server's main thread's, and the reports the program read. Until perf/mux-client
   every move over a shell's pane went to the server, which dropped it; every move over a program
   with the mouse still goes, those inside one cell too (MUX.md, "The pointer over a pane").
3. write: an echoing tab in view and -Busy tabs printing without a pause behind it, so the server
   is busy: -Writes keys bound to SendString (F5, 'b'), then as many letters ('a', a key), each
   -KeyEveryMs apart: what each key's WM_KEYDOWN held the GUI thread for (metrics' messages, its
   time handled, which counts any wait in it); then -Pairs pairs of a letter and F5 posted with
   no gap between them, which the echo must read in order ('ab' each). Until perf/mux-client a
   write to a mux pane (SendString, Lachlan's Shift+Enter and Alt+Enter, a paste of an image, the
   scroll strip's keys) waited for the server's answer on the GUI thread, and went ahead of a key
   still waiting to be sent.
4. fetch: a tab in view printing a line of 200 characters every -FetchEveryMs (5; 0 without a
   pause) behind -History lines of
   scrollback, for -Seconds: the GetLines the window sent (metrics), its frames and the pushes it
   took. A line the server pushes can be newer than the push, and until perf/mux-client the
   window fetched such a line, threw away what came back and fetched it again.
5. resize: -Busy tabs each printing -History lines, then the window made four columns narrower
   and wider again -Steps times: the time its WM_WINDOWPOSCHANGED held the GUI thread (metrics),
   where every pane of every tab is resized, which makes every
   line every pane's cache holds stale. Until perf/mux-client that freed and made a node for every
   line held.

It fails, unless -Measure, when a frame of the mux pane finds fewer than 1/-HashRatio of the shape
keys kept that the window's own pane's frame does (the same screen and spinner), when a move over
the shell's pane is sent at all, when a pane isn't back to its columns once the window is, when the program with
the mouse reads none of the moves over it, when a SendString key holds the GUI thread over
-WriteMs at the mean (without its longest), or when the pairs' echo isn't 'ab' each in order (with
this build's server only: whether a write keeps its place behind a key at the program is the server's
doing, PITFALLS 137). -Compat (another build's server: -ServerBin) judges the pointer and the writes
alone. mux-gui.json in the output
folder has every number.

  pwsh -File lbterm\perf\mux-gui.ps1 [-Only frame,pointer,write,fetch,resize] [-Measure] [-Compat] [-ServerBin <build>|installed] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string[]]$Only = @('frame', 'pointer', 'write', 'fetch', 'resize'),
  [int]$Cols = 120,
  [int]$Rows = 50,
  [int]$Seconds = 10,
  [int]$SpinMs = 62,
  [int]$Moves = 600,
  [int]$MoveEveryMs = 2,
  [int]$Writes = 60,
  [int]$Pairs = 40,
  [int]$KeyEveryMs = 40,
  [int]$Busy = 2,
  [int]$History = 20000,
  [int]$FetchEveryMs = 5,
  [int]$Steps = 10,
  [switch]$Measure,
  [switch]$Compat,
  [double]$MaxMemGB = 2,
  # Bounds: a local frame's shape keys kept over a mux frame's; a SendString key's WM_KEYDOWN, ms
  [double]$HashRatio = 3,
  [double]$WriteMs = 2
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-gui' -Bin $Bin
$Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
if ($ServerBin -eq 'installed') {
  $dir = Join-Path $T.Out 'installed-server'
  New-Item -ItemType Directory -Force $dir | Out-Null
  foreach ($f in 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe') { Copy-Item (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $dir }
  $ServerBin = $dir
}
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$rate = Get-CycleRate
# Get-CycleRate is cycles a millisecond
$cyclesMs = { param($c) [Math]::Round($c / $rate, 2) }

# The frame's screen: -Rows - 1 lines of 24-bit colour words, a URL and a path on each, then the
# last row's spinner drawn again every -SpinMs, as a working Claude Code session's is
$FrameProgram = New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$h = [Console]::WindowHeight; $w = [Console]::WindowWidth
$sb = [Text.StringBuilder]::new()
[void]$sb.Append("$e[?25l$e[2J")
for ($r = 1; $r -lt $h; $r++) {
  [void]$sb.Append("$e[$r;1H")
  $line = ''
  for ($k = 0; $line.Length -lt $w - 60; $k++) {
    $c = "$e[38;2;$((40 + $r * 7 + $k * 13) % 256);$((200 - $r * 3) % 256);$((90 + $k * 29) % 256)m"
    [void]$sb.Append("${c}word$k ")
    $line += "word$k "
  }
  [void]$sb.Append("$e[0m see https://example.com/frame/$r and C:/work/frame/notes-$r.md")
}
[Console]::Out.Write($sb.ToString())
$spin = [char[]]@(0xB7, 0x2722, 0x2733, 0x2736, 0x273B, 0x273D)
for ($i = 0; ; $i++) {
  [Console]::Out.Write("$e[$h;1H$e[38;5;174m$($spin[$i % 6]) Working$e[0m")
  [Threading.Thread]::Sleep(XSPINX)
}
'@ -replace 'XSPINX', $SpinMs)

# A program that has the mouse (as Claude Code's fullscreen view asks for it) and counts the
# reports it reads, saying the count in its first row every 200 ms
$MouseProgram = New-PwshProgram -Script @'
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public static class LbtermCon {
  [DllImport("kernel32.dll")] public static extern IntPtr GetStdHandle(int n);
  [DllImport("kernel32.dll")] public static extern bool SetConsoleMode(IntPtr h, uint m);
}
"@
$e = [char]27
# Input as VT, nothing echoed or cooked: the reports come as ESC [ < b ; x ; y M
[void][LbtermCon]::SetConsoleMode([LbtermCon]::GetStdHandle(-10), 0x200)
[Console]::Out.Write("$e[?1000h$e[?1002h$e[?1003h$e[?1006h$e[?25l$e[2J$e[1;1Hmouse reports: 0")
$in = [Console]::OpenStandardInput()
$buf = [byte[]]::new(8192); $n = 0; $text = ''
$said = [Diagnostics.Stopwatch]::StartNew()
while ($true) {
  $got = $in.Read($buf, 0, $buf.Length)
  if ($got -le 0) { continue }
  $text += [Text.Encoding]::ASCII.GetString($buf, 0, $got)
  $found = [regex]::Matches($text, "\x1b\[<\d+;\d+;\d+[Mm]")
  if ($found.Count) { $n += $found.Count; $last = $found[$found.Count - 1]; $text = $text.Substring($last.Index + $last.Length) }
  if ($text.Length -gt 64) { $text = $text.Substring($text.Length - 64) }
  if ($said.ElapsedMilliseconds -ge 200) { [Console]::Out.Write("$e[1;1Hmouse reports: $n   "); $said.Restart() }
}
'@

# A shell's screen: a page of text and a prompt, nothing asked of the mouse
$ShellProgram = New-PwshProgram -Script @'
$sb = [Text.StringBuilder]::new()
for ($i = 1; $i -le 30; $i++) { [void]$sb.Append("a shell's line $i, as a build or a log leaves it`n") }
[Console]::Out.Write($sb.ToString() + 'PS C:\work> ')
Start-Sleep 900
'@

# Each key it's given, written back
$EchoProgram = New-PwshProgram -Script @'
[Console]::Out.Write('> ')
while ($true) {
  $k = [Console]::ReadKey($true)
  if ($k.KeyChar -eq [char]13) { [Console]::Out.Write("`r`n> ") } else { [Console]::Out.Write($k.KeyChar) }
}
'@

# Without a pause, until killed
$FloodProgram = New-PwshProgram -Script @'
$s = (('x' * 100) + "`n") * 400
while ($true) { [Console]::Out.Write($s) }
'@

# -History lines, then a file saying so, then a line of 200 characters every -FetchEveryMs (fetch) or
# nothing more (resize)
function Get-HistoryProgram([int]$Lines, [string]$Done, [switch]$Print) {
  New-PwshProgram -Script (@'
$sb = [Text.StringBuilder]::new()
for ($i = 1; $i -le XLINESX; $i++) { [void]$sb.Append("history line $i of XLINESX, as a build or a log leaves it`n") }
[Console]::Out.Write($sb.ToString())
Set-Content -LiteralPath 'XDONEX' 'printed'
if (XPRINTX) {
  $line = 'x' * 200
  for ($i = 0; ; $i++) { [Console]::Out.WriteLine("$i $line"); if (XEVERYX) { [Threading.Thread]::Sleep(XEVERYX) } }
}
Start-Sleep 900
'@ -replace 'XLINESX', $Lines -replace 'XDONEX', $Done -replace 'XPRINTX', $(if ($Print) { '$true' } else { '$false' }) -replace 'XEVERYX', $FetchEveryMs)
}

function New-GuiConfig([string]$Name) {
  $cfg = Join-Path $T.Out "config-$Name\mux-gui.lua"
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.enable_scroll_bar = false
rawset(config, 'lbterm_scroll_strip', nil)
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F5', mods = 'NONE', action = wezterm.action.SendString 'b' })
config.unix_domains = { {
  name = 'guimux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  $cfg
}

# A window on the test domain whose one tab runs $Program, with -Behind tabs out of view: the
# window, its tab's server pane, the server's pid and its main thread
function Open-GuiWindow([string]$Name, [object[]]$Program, [object[][]]$Behind = @()) {
  $w = Open-TestWindow -ConfigFile (New-GuiConfig $Name) -Domain 'guimux' -Socket $T.Sock -TimeoutSec 30
  Wait-State { param($s) $s.frame -gt 0 } -What "the $Name window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $serverPid = [int]$server[0].ProcessId
  Start-MemoryGuard -Pids $w.Pid, $serverPid -LimitGB $MaxMemGB
  $shell = $panes[0].pane_id
  $pane = New-Tab -From $shell -Program $Program -Server
  foreach ($b in $Behind) { New-Tab -From $pane -Program $b -NoActivate -Server | Out-Null }
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  $want = 1 + $Behind.Count
  Wait-State { param($s) @($s.tabs).Count -eq $want } -What "the $want tabs in the $Name window" -TimeoutSec 30 | Out-Null
  Invoke-Cli activate-pane --pane-id $pane -Server -Check | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $pane }).Count -eq 1 -and -not $s.animating } -What "the $Name tab in view" -TimeoutSec 15 | Out-Null
  $w | Add-Member -NotePropertyName Pane -NotePropertyValue $pane
  $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue $serverPid
  $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue (Get-MainThread $serverPid)
  $w
}

function Close-GuiWindow($w) {
  Stop-TestWindow $w
  Stop-TestServers
  $guard = Get-MemoryGuard
  if ($guard) { throw "the memory guard stopped the test: $guard" }
}

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
  $s = ($b.At - $a.At) / [Diagnostics.Stopwatch]::Frequency
  [ordered]@{
    Seconds = [Math]::Round($s, 2)
    GuiThreadMs = & $cyclesMs ($b.Thread - $a.Thread); GuiMs = & $cyclesMs ($b.Gui - $a.Gui)
    ServerMs = & $cyclesMs ($b.Server - $a.Server); ServerMainMs = & $cyclesMs ($b.Main - $a.Main)
  }
}

# A PDU's count in a metrics reply, sent or received
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}

# How many reports the program with the mouse says it has read (its first row), from the server;
# its screen saved as <Save>.txt
function Get-MouseReports([int]$Pane, [string]$Save) {
  $text = (Invoke-Cli get-text --pane-id $Pane -Server) -join "`n"
  if ($Save) { Set-Content -LiteralPath (Join-Path $T.Out "$Save.txt") $text }
  if ($text -match 'mouse reports: (\d+)') { [int]$Matches[1] } else { $null }
}

# What's measured so far, kept after each part so a part that fails keeps the ones before it
function Save-Results { [pscustomobject]$results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-gui.json') }

# The window's active pane as its last frame drew it
function Get-ActivePane($w) { @((Get-StateWindow (Read-State) -Hwnd ([long](Resolve-Handle $w))).panes | Where-Object active)[0] }

# A histogram of a metrics reply by name
function Get-Hist($m, [string]$Name) { @($m.histograms | Where-Object name -eq $Name)[0] }

function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

$results = [ordered]@{ Window = $T.Gui; Server = $serverExe; Compat = [bool]$Compat; Locked = (Test-WorkstationLocked); Frame = $null; Pointer = $null; Write = $null; Fetch = $null; Resize = $null }

Invoke-Test {
  # 1. frame: the same screen and spinner in a mux pane and in the window's own
  if ('frame' -in $Only -and -not $Compat) {
    $frame = [ordered]@{}
    foreach ($where in 'mux', 'local') {
      if ($where -eq 'mux') {
        $w = Open-GuiWindow 'frame' $FrameProgram
      } else {
        $w = Open-TestWindow -ConfigFile (New-GuiConfig 'frame-local') -Program $FrameProgram -TimeoutSec 30
        Wait-State { param($s) $s.frame -gt 0 } -What 'the local window''s first frame' -TimeoutSec 30 | Out-Null
        $w | Add-Member -NotePropertyName ServerPid -NotePropertyValue $w.Pid
        $w | Add-Member -NotePropertyName ServerMain -NotePropertyValue (Get-MainThread $w.Pid)
      }
      Start-Sleep 2
      Get-Metrics $w -Reset | Out-Null
      $a = Get-Mark $w
      Start-Sleep $Seconds
      $b = Get-Mark $w
      $m = Get-Metrics $w
      $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out "frame-$where-metrics.json")
      $spent = Get-Spent $a $b
      $frames = [int]$m.totals.frames
      $lines = Get-Hist $m 'paint_pane.lines'
      # A line whose shape key is kept on it is a hit of the line state cache; a line hashed
      # again isn't counted (the cache is only asked for a key a line holds), so the hits a frame
      # against the rows say it. metrics' caches count them since the reset (since perf/fonts-core,
      # 2026-11-03, which took the caches' hit and miss histograms out)
      $kept = $m.caches.line_state_cache
      $perSec = $frames / $spent.Seconds
      $r = [ordered]@{
        Frames = $frames; FramesPerSec = [Math]::Round($perSec, 1)
        GuiThreadMsPerFrame = if ($frames) { [Math]::Round($spent.GuiThreadMs / $frames, 3) } else { $null }
        GuiMsPerFrame = if ($frames) { [Math]::Round($spent.GuiMs / $frames, 3) } else { $null }
        LinesMsMean = if ($lines) { $lines.mean } else { $null }; LinesMsP50 = if ($lines) { $lines.p50 } else { $null }; LinesCount = if ($lines) { $lines.count } else { 0 }
        Hits = if ($kept) { [long]$kept.hits } else { $null }
        HitsPerSec = if ($kept -and $spent.Seconds) { [Math]::Round([double]$kept.hits / $spent.Seconds, 1) } else { $null }; Rows = $Rows
      }
      $r.HitsPerFrame = if ($kept -and $frames) { [Math]::Round([double]$kept.hits / $frames, 1) } else { $null }
      $frame[$where] = $r
      "frame ($where, $($spent.Seconds) s): $frames frames ($($r.FramesPerSec)/s), the GUI thread $($r.GuiThreadMsPerFrame) ms a frame ($($r.GuiMsPerFrame) ms of the process), paint_pane.lines $($r.LinesMsMean) ms at the mean over $($r.LinesCount); $($r.HitsPerFrame) lines a frame found their shape key kept ($($r.HitsPerSec) a second; each row is looked up twice a frame, so twice its $Rows rows when none is hashed again)"
      if ($where -eq 'mux') { Close-GuiWindow $w } else { Stop-TestWindow $w }
    }
    $results.Frame = $frame
    Save-Results
  }

  # 2. pointer: moves across a shell's pane and across a program with the mouse
  if ('pointer' -in $Only) {
    $pointer = [ordered]@{}
    $w = Open-GuiWindow 'pointer' $ShellProgram @(, $MouseProgram)
    $h = Resolve-Handle $w
    foreach ($over in 'shell', 'mouse', 'cell') {
      if ($over -eq 'mouse') {
        Invoke-Cli activate-tab --tab-index 1 -Server -Check | Out-Null
        Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.mouse_reporting }).Count -eq 1 -and -not $s.animating } -What 'the program with the mouse in view, its mouse reporting known' -TimeoutSec 15 | Out-Null
      }
      $p = (Get-ActivePane $w)
      $mousePane = $p.server_id
      # The window told it has the keyboard: a move goes to a pane only then
      Send-Focus $w
      Start-Sleep -Milliseconds 300
      $x0 = [int]$p.rect.x + 4; $x1 = [int]($p.rect.x + $p.rect.w) - 8
      $y0 = [int]$p.rect.y + 40; $y1 = [int]($p.rect.y + $p.rect.h) - 40
      # cell: every move inside one cell, the middle of row 10, column 40, a pixel either way
      $cw = $p.rect.w / $p.cols; $ch = $p.rect.h / $p.rows
      $cx = [int]($p.rect.x + ($p.cols / 3 + 0.5) * $cw); $cy = [int]($p.rect.y + 10.5 * $ch)
      $reportsBefore = Get-MouseReports $p.server_id
      Get-Metrics $w -Reset | Out-Null
      $a = Get-Mark $w
      $clock = [Diagnostics.Stopwatch]::StartNew()
      for ($k = 0; $k -lt $Moves; $k++) {
        if ($over -eq 'cell') {
          $x = $cx + ($k % 3) - 1; $y = $cy + ([Math]::Floor($k / 3) % 3) - 1
        } else {
          $f = $k / [Math]::Max(1, $Moves - 1)
          $x = [int]($x0 + ($x1 - $x0) * $f)
          $y = [int]($y0 + ($y1 - $y0) * (0.5 + 0.5 * [Math]::Sin($f * 12)))
        }
        [LbtermE2E.Win]::Mouse($h, 0x0200, 0, $x, $y)
        $due = ($k + 1) * $MoveEveryMs
        while ($clock.Elapsed.TotalMilliseconds -lt $due) { }
      }
      Start-Sleep -Milliseconds 500
      $b = Get-Mark $w
      $m = Get-Metrics $w
      $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out "pointer-$over-metrics.json")
      Send-Focus $w -Off
      $spent = Get-Spent $a $b
      $sent = Get-Pdus $m 'SendMouseEvent' 'sent'
      $reports = $null
      if ($over -ne 'shell') {
        Start-Sleep -Milliseconds 400
        $after = Get-MouseReports $mousePane "pointer-$over"
        if ($null -ne $after -and $null -ne $reportsBefore) { $reports = $after - $reportsBefore }
      }
      $r = [ordered]@{ Moves = $Moves; Sent = $sent; Reports = $reports; Frames = [int]$m.totals.frames } + $spent
      $pointer[$over] = $r
      "pointer over the $over pane: $Moves moves in $([Math]::Round($clock.Elapsed.TotalMilliseconds)) ms, $sent SendMouseEvent sent$(if ($null -ne $reports) { ", $reports reports read by the program" }); the GUI thread $($spent.GuiThreadMs) ms, the server's main thread $($spent.ServerMainMs) ms and the server $($spent.ServerMs) ms over $($spent.Seconds) s, $($r.Frames) frames"
    }
    Close-GuiWindow $w
    $results.Pointer = $pointer
    Save-Results
  }

  # 3. write: SendString keys and letters into an echo with the server busy behind it
  if ('write' -in $Only) {
    $write = [ordered]@{}
    $w = Open-GuiWindow 'write' $EchoProgram @(1..$Busy | ForEach-Object { , $FloodProgram })
    Start-Sleep 2
    foreach ($kind in 'sendstring', 'letter') {
      $key = if ($kind -eq 'sendstring') { 'F5' } else { 'A' }
      Get-Metrics $w -Reset | Out-Null
      $a = Get-Mark $w
      for ($k = 0; $k -lt $Writes; $k++) { Send-Key $w $key; Start-Sleep -Milliseconds $KeyEveryMs }
      Start-Sleep -Milliseconds 500
      $b = Get-Mark $w
      $m = Get-Metrics $w
      $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out "write-$kind-metrics.json")
      $down = @($m.messages | Where-Object site -eq 'WM_KEYDOWN')[0]
      $r = [ordered]@{
        Keys = $Writes; KeyDowns = if ($down) { [int]$down.count } else { 0 }
        KeyDownMsMean = if ($down -and $down.count) { [Math]::Round([double]$down.total_ms / [double]$down.count, 3) } else { $null }
        KeyDownMsMax = if ($down) { [double]$down.max_ms } else { $null }
        # The mean without the longest, which one stall of the PC (another program's load) can make
        KeyDownMsTrimmed = if ($down -and $down.count -gt 1) { [Math]::Round(([double]$down.total_ms - [double]$down.max_ms) / ([double]$down.count - 1), 3) } else { $null }
        WritesSent = Get-Pdus $m 'WriteToPane' 'sent'; KeysSent = Get-Pdus $m 'SendKeyDown' 'sent'
      } + (Get-Spent $a $b)
      $write[$kind] = $r
      "write ($kind keys, $Busy tabs printing behind): $($r.KeyDowns) WM_KEYDOWN held the GUI thread $($r.KeyDownMsMean) ms at the mean, $($r.KeyDownMsTrimmed) without the longest, $($r.KeyDownMsMax) at most; $($r.WritesSent) WriteToPane and $($r.KeysSent) SendKeyDown sent"
      # A new line for the next part, each key's echo on its own line
      Send-Key $w 'Enter'
      Start-Sleep -Milliseconds 300
    }
    # The pairs: a letter and F5 with no gap, which must reach the program in that order
    for ($k = 0; $k -lt $Pairs; $k++) {
      Send-Key $w 'A'
      Send-Key $w 'F5'
      Start-Sleep -Milliseconds $KeyEveryMs
    }
    Start-Sleep -Milliseconds 1500
    # The echo's last line: the pairs, after the Enter that ended the letters' line
    $screen = @(Invoke-Cli get-text --pane-id $w.Pane -Server)
    Set-Content -LiteralPath (Join-Path $T.Out 'write-pairs.txt') ($screen -join "`n")
    $last = @($screen | Where-Object { $_ -match '^> ' })[-1]
    $got = if ($last) { ($last.Substring(2) -replace '[^ab]', '') } else { '' }
    $want = 'ab' * $Pairs
    $write.Pairs = [ordered]@{ Pairs = $Pairs; Read = $got; InOrder = ($got -eq $want); Swapped = ([regex]::Matches($got, 'ba')).Count - ([regex]::Matches($want, 'ba')).Count }
    "write (pairs): $Pairs letters each followed at once by F5's SendString read as $(if ($got -eq $want) { "'ab' each, in order" } else { "'$got', $($write.Pairs.Swapped) out of order" })"
    Close-GuiWindow $w
    $results.Write = $write
    Save-Results
  }

  # 4. fetch: a tab in view printing a long line every 5 ms
  if ('fetch' -in $Only -and -not $Compat) {
    $done = Join-Path $T.Out 'fetch.done'
    $w = Open-GuiWindow 'fetch' (Get-HistoryProgram $History $done -Print)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath $done) -and $clock.Elapsed.TotalSeconds -lt 120) { Start-Sleep -Milliseconds 100 }
    Start-Sleep 2
    Get-Metrics $w -Reset | Out-Null
    $a = Get-Mark $w
    Start-Sleep $Seconds
    $b = Get-Mark $w
    $m = Get-Metrics $w
    $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out 'fetch-metrics.json')
    $spent = Get-Spent $a $b
    $r = [ordered]@{
      GetLines = Get-Pdus $m 'GetLines' 'sent'; Pushes = Get-Pdus $m 'GetPaneRenderChangesResponse' 'received'; Frames = [int]$m.totals.frames
    } + $spent
    $r.GetLinesPerSec = [Math]::Round($r.GetLines / $spent.Seconds, 1)
    $results.Fetch = $r
    Save-Results
    "fetch ($($spent.Seconds) s, a line of 200 every $FetchEveryMs ms behind $History lines): $($r.GetLines) GetLines sent ($($r.GetLinesPerSec)/s), $($r.Pushes) pushes, $($r.Frames) frames; the GUI thread $($spent.GuiThreadMs) ms, the server's main thread $($spent.ServerMainMs) ms"
    Close-GuiWindow $w
  }

  # 5. resize: every pane's cache full of lines, then the window four columns narrower and back
  if ('resize' -in $Only -and -not $Compat) {
    $dones = @(1..($Busy + 1) | ForEach-Object { Join-Path $T.Out "resize-$_.done" })
    $w = Open-GuiWindow 'resize' (Get-HistoryProgram $History $dones[0]) @(1..$Busy | ForEach-Object { , (Get-HistoryProgram $History $dones[$_]) })
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (@($dones | Where-Object { -not (Test-Path -LiteralPath $_) }).Count -and $clock.Elapsed.TotalSeconds -lt 120) { Start-Sleep -Milliseconds 100 }
    Start-Sleep 3
    $rect = Get-WindowRect $w
    $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
    $p = (Get-ActivePane $w)
    # Narrower by a few columns' width and back, as budgets.ps1's resize does
    $cell = 4 * [Math]::Max(4, [int][Math]::Ceiling($p.rect.w / $p.cols))
    Get-Metrics $w -Reset | Out-Null
    $a = Get-Mark $w
    for ($k = 0; $k -lt $Steps; $k++) {
      # SetWindowPos returns once the window has taken its new size (WM_WINDOWPOSCHANGED, where
      # every pane is resized), so nothing here waits on a frame: one in a few didn't come within
      # 10 s on any build while a game ran, the window's size changed and its state file not
      Set-WindowSize $w ($width - $cell) $height
      Start-Sleep -Milliseconds 150
      Set-WindowSize $w $width $height
      Start-Sleep -Milliseconds 150
    }
    Start-Sleep -Milliseconds 500
    $b = Get-Mark $w
    $m = Get-Metrics $w
    $m | ConvertTo-Json -Depth 16 | Set-Content (Join-Path $T.Out 'resize-metrics.json')
    $back = Wait-Until { (Get-ActivePane $w).cols -eq $p.cols } -TimeoutSec 15
    # The window's new size is taken in WM_WINDOWPOSCHANGED (every pane of every tab resized there)
    $size = @($m.messages | Where-Object site -eq 'WM_WINDOWPOSCHANGED')[0]
    $spent = Get-Spent $a $b
    $r = [ordered]@{
      Steps = 2 * $Steps; Tabs = $Busy + 1; HistoryLines = $History
      SizeMessages = if ($size) { [int]$size.count } else { 0 }
      SizeMsTotal = if ($size) { [double]$size.total_ms } else { $null }; SizeMsMax = if ($size) { [double]$size.max_ms } else { $null }
      Back = $back; GetLines = Get-Pdus $m 'GetLines' 'sent'; GuiPrivateMB = [Math]::Round((Get-Process -Id $w.Pid).PrivateMemorySize64 / 1MB, 1)
    } + $spent
    $r.SizeMsPerStep = if ($size -and $size.count) { [Math]::Round($r.SizeMsTotal / $size.count, 2) } else { $null }
    $results.Resize = $r
    Save-Results
    "resize ($($r.Steps) steps, $($r.Tabs) tabs each after $History lines): WM_WINDOWPOSCHANGED held the GUI thread $($r.SizeMsTotal) ms in all over $($r.SizeMessages), $($r.SizeMsPerStep) ms a step, $($r.SizeMsMax) at most; the GUI thread $($spent.GuiThreadMs) ms over $($spent.Seconds) s; the window holds $($r.GuiPrivateMB) MB"
    Close-GuiWindow $w
  }

  Save-Results

  if (-not $Measure) {
    if ($results.Frame) {
      $mux = $results.Frame.mux; $local = $results.Frame.local
      if (-not $mux.Frames -or -not $local.Frames) { Add-Failure "frame: unmeasured, a window painted no frames ($($mux.Frames) and $($local.Frames))" }
      elseif (-not $local.HitsPerFrame) { Add-Failure "frame: unmeasured, the window's own pane's frames found no shape key kept ($($local.HitsPerFrame))" }
      elseif ($mux.HitsPerFrame * $HashRatio -lt $local.HitsPerFrame) { Add-Failure "frame: the mux pane's frame found $($mux.HitsPerFrame) lines' shape keys kept against the window's own pane's $($local.HitsPerFrame): the rest hashed again" }
    }
    if ($results.Pointer) {
      if ($results.Pointer.shell.Sent -gt 0) { Add-Failure "pointer: $($results.Pointer.shell.Sent) moves over the shell's pane went to the server, which drops them" }
      if (-not $results.Pointer.mouse.Sent) { Add-Failure 'pointer: no move over the program with the mouse went to the server' }
      if (-not $results.Pointer.mouse.Reports) { Add-Failure "pointer: the program with the mouse read no reports ($($results.Pointer.mouse.Reports))" }
    }
    if ($results.Resize -and -not $results.Resize.Back) { Add-Failure "resize: the pane wasn't back to its columns 15 s after the window was" }
    if ($results.Write) {
      $s = $results.Write.sendstring
      if ($null -eq $s.KeyDownMsTrimmed) { Add-Failure 'write: unmeasured, no WM_KEYDOWN counted' }
      elseif ($s.KeyDownMsTrimmed -gt $WriteMs) { Add-Failure "write: a SendString key held the GUI thread $($s.KeyDownMsTrimmed) ms at the mean (without the longest), over $WriteMs" }
      if ($s.WritesSent -ne $Writes) { Add-Failure "write: $($s.WritesSent) of the $Writes F5 keys wrote to the pane (a modifier held on the PC turns a posted F5 into another key)" }
      # The order at the program is the server's (its terminal's writer thread): an older one's
      # may swap them whatever the window does
      if (-not $ServerBin -and -not $results.Write.Pairs.InOrder) { Add-Failure "write: the pairs read '$($results.Write.Pairs.Read)', not 'ab' each in order" }
    }
  }
} -Passed "mux-gui: a mux pane's frame hashes only the lines that changed, a move over a shell's pane stays in the window, a write holds the GUI thread no round trip and keeps its place behind keys"
