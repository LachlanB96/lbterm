#Requires -Version 7
# Variant: -ServerBin installed
# Variant: -GuiBin installed
# Covers: wezterm-client/src/lbterm_drawn.rs, wezterm-client/src/client.rs, wezterm-client/src/pane/renderable.rs, wezterm-client/src/pane/clientpane.rs, wezterm-gui/src/termwindow/panes_drawn.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-mux-server-impl/src/lbterm_flow.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-mux-server-impl/src/lbterm_pushstats.rs, codec/src/lib.rs
<#
A shown pane's pace on a mux domain (lbterm/MUX.md, "A shown pane's pace"; FLOWS.md's pane-output
flow, What could go 12): a server of this build pushes a pane its window shows once a frame has
read the last push of it, as the window says (codec's PanesDrawn), where it pushed one a parser
batch whatever the frames took. The tab is in a test mux server of its own (a socket under the
test's folder; never Lachlan's), the window drawn as Lachlan's (New-TestConfig) without the
scrollbar's and the scroll strip's prompt marks, which read the whole scrollback of the tab in view.

-Rounds rounds, each:
1. flood: the tab in view prints numbered lines in blocks without a pause for -Seconds, and between
   blocks reads any key posted to it and sets its title to `key-<the key>` at once (OSC 2). A key
   posted every -KeyEveryMs, each timed from its post to the first frame whose pane has that title
   (the state file's time_ms): the echo of input to a flooding pane, which the server lets go
   unpaced (the pace's rule 6). Over the span: the frames the window painted (the state file's
   `frame`), the shown pane's pushes and how many of them waited for a frame's word and for how
   long (the server's push log, `lbterm::push`, its `pushed`, `waited` and `waited_us` on the
   pane's `shown` line), the PanesDrawn the window sent and the pushes it took (its counters), and
   the server's main thread and the window's GUI thread (cycles, over the clock's span).
2. final: the tab told to stop (a file it looks for between blocks) clears its screen to five
   lines of the round's own, its title `pace-final-<round>` and its cursor at a place of the
   round's, and writes when it wrote them. Within -SettleMs a frame of the window must draw it: a
   frame whose pane has that title and draws its cursor there. Its time from the write to that
   frame. The last output always reaches a frame: under the pace the push that carries it waits
   at most for the word of the frame that read the one before.

Each pairing with another build (PITFALLS 159): -ServerBin runs another build's
lbterm-mux-server.exe for the domain and -GuiBin another build's lbterm-gui.exe for the window
(`installed` for the installed build's, copied into the test's folder first). What's right is
judged in every pairing: every round's final screen drawn, the keys' titles shown, the connection
never ending. What it costs is judged in this build's pair alone: the shown pane's pushes a second
at most -PushRatio times its frames a second plus two a key, some push waiting for a frame's word
(the pace ran), and the keys' echo median at most -EchoMs. In another pairing no push may wait: an
older window never says PanesDrawn, so its server paces nothing, and an older server doesn't know
SetShownPanes, so the window never sends it PanesDrawn (it would end the connection: PITFALLS 227).

-Age: the newest line's age in a frame. Each block ends with a title saying when it was written
(`key-<the last key> t=<unix ms>`; the keys' titles are looked for in either form), and between
keys every frame the state file says is read: the snapshot's time_ms less its pane's title's time.
A title a block is a title change in every push, for every build alike: compare -Age runs only
with -Age runs.

mux-pace.json in the output folder has every number. -Measure reports and judges nothing on cost.

  pwsh -File lbterm\perf\mux-pace.ps1 [-Rounds 3] [-Seconds 4] [-KeyEveryMs 250] [-SettleMs 1500] [-Age] [-Measure] [-ServerBin <build>|installed] [-GuiBin <build>|installed] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$Rounds = 3,
  [int]$Seconds = 4,
  [int]$KeyEveryMs = 250,
  [int]$SettleMs = 1500,
  [int]$Cols = 120,
  [int]$Rows = 40,
  [int]$Scrollback = 5000,
  [double]$PushRatio = 1.2,
  [double]$EchoMs = 100,
  [double]$MaxMemGB = 2,
  [string]$Log,
  [switch]$Age,
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-pace' -Bin $Bin
# The installed build's $Files, copied into the test's folder: a server or window running from the
# install folder would be taken for the daily one (the Warden's registry and `lbterm.ps1 logs` go
# by the exe's folder)
function Copy-Installed([string]$Name, [string[]]$Files) {
  $dir = Join-Path $T.Out "installed-$Name"
  New-Item -ItemType Directory -Force $dir | Out-Null
  foreach ($f in $Files) { Copy-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $dir }
  $dir
}
if ($ServerBin -eq 'installed') { $ServerBin = Copy-Installed 'server' 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe' }
if ($GuiBin -eq 'installed') { $GuiBin = Copy-Installed 'gui' 'lbterm-gui.exe', 'conpty.dll', 'OpenConsole.exe', 'libEGL.dll', 'libGLESv2.dll', 'mesa' }
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$guiExe = if ($GuiBin) { Join-Path (Resolve-Path $GuiBin).Path 'lbterm-gui.exe' } else { $T.Gui }
if (-not (Test-Path $guiExe)) { throw "no lbterm-gui.exe in $GuiBin" }
$Pairing = if ($GuiBin -and $ServerBin) { 'other window, other server' } elseif ($GuiBin) { 'other window' } elseif ($ServerBin) { 'other server' } else { 'this build' }
$OwnPair = $Pairing -eq 'this build'
$rate = Get-CycleRate

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermPace {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  public static ulong ThreadCycles(int tid) {
    IntPtr h = OpenThread(0x0800, false, (uint)tid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryThreadCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  // The process's first thread, its main one
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      int best = 0; DateTime first = DateTime.MaxValue;
      foreach (ProcessThread t in p.Threads) {
        try { if (t.StartTime < first) { first = t.StartTime; best = t.Id; } } catch (Exception) { }
      }
      return best;
    }
  }
}
'@

# The tab: rounds of a flood, each ended by its stop file with a final screen of its own
$Dir = $T.Out
function Get-FinalCursor([int]$Round) { @{ Row = 6 + $Round; Col = 4 + 2 * $Round } }
$Flood = New-PwshProgram -Script (@'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27; $bel = [char]7
[Console]::Out.Write("$e]2;pace-flood$bel")
$sb = [Text.StringBuilder]::new()
$n = 0
$age = XAGEX
for ($r = 1; $r -le XROUNDSX; $r++) {
  while (-not (Test-Path "XDIRX\go-$r")) { Start-Sleep -Milliseconds 20 }
  $last = ''; $t = 0
  while (-not (Test-Path "XDIRX\stop-$r")) {
    [void]$sb.Clear()
    for ($i = 0; $i -lt 40; $i++) { $n++; [void]$sb.Append("pace $r line $n the quick brown fox jumps over the lazy dog 0123456789 abcdefghijklmnopqrstuvwxyz`r`n") }
    # -Age: the block's title says when it was written, after its last line
    if ($age) { $t = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); [void]$sb.Append("$e]2;key-$last t=$t$bel") }
    [Console]::Out.Write($sb.ToString())
    while ([Console]::KeyAvailable) { $k = [Console]::ReadKey($true); $last = $k.KeyChar; [Console]::Out.Write("$e]2;key-$($k.KeyChar)$(if ($age) { " t=$t" })$bel") }
  }
  $row = 6 + $r; $col = 4 + 2 * $r
  $final = "$e[2J$e[H"
  for ($j = 1; $j -le 5; $j++) { $final += "final $r row $j after $n lines`r`n" }
  $final += "$e]2;pace-final-$r$bel$e[${row};${col}H"
  [Console]::Out.Write($final)
  [IO.File]::WriteAllText("XDIRX\done-$r", "$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())")
}
Start-Sleep 900
'@ -replace 'XROUNDSX', $Rounds -replace 'XDIRX', $Dir -replace 'XAGEX', $(if ($Age) { '$$true' } else { '$$false' }))

$cfg = Join-Path $T.Out 'config\mux-pace.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = $Cols
config.initial_rows = $Rows
config.scrollback_lines = $Scrollback
config.enable_scroll_bar = false
config.unix_domains = { {
  name = 'pacemux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

# A PDU's count in a metrics reply, sent or received
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}
# A log's text from byte $From on
function Get-LogText([string]$Path, [long]$From = 0) {
  if (-not $Path -or -not (Test-Path -LiteralPath $Path)) { return '' }
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
}
function Get-LogLength([string]$Path) { if ($Path -and (Test-Path -LiteralPath $Path)) { (Get-Item -LiteralPath $Path).Length } else { 0 } }
# The server's push log for pane $Rid from byte $From to byte $To: its round pushes while shown,
# those of them that waited for a frame's word and for how long (a build without `waited` says
# none), and the seconds the lines cover (each line its connection's last second or so, its span
# first), which the pushes a second are counted over
function Get-PaneLog([string]$Path, [long]$From, [long]$To, [int]$Rid) {
  $r = [ordered]@{ Lines = 0; Pushed = 0; Waited = 0; WaitedUs = 0; Seconds = 0.0 }
  $text = Get-LogText $Path $From
  if ($To -gt $From -and $text.Length -gt ($To - $From)) { $text = $text.Substring(0, [int]($To - $From)) }
  foreach ($l in ($text -split "`r?`n")) {
    if ($l -notmatch 'lbterm::push') { continue }
    if ($l -match "([\d.]+) s: \d+ rounds.*; pane $Rid shown asked \d+ alone \d+ pushed (\d+)([^;]*)") {
      $r.Lines++; $r.Seconds += [double]$Matches[1]; $r.Pushed += [int]$Matches[2]
      $rest = $Matches[3]
      if ($rest -match ' waited (\d+) waited_us (\d+)') { $r.Waited += [int]$Matches[1]; $r.WaitedUs += [long]$Matches[2] }
    }
  }
  $r
}
# (from $From, the window's `LogStart`: a GUI given a pid an earlier one had appends to its log,
# whose lines aren't this window's, PITFALLS 240)
function Get-ConnectionLines([string]$Path, [long]$From = 0) {
  @((Get-LogText $Path $From) -split "`r?`n" | Where-Object { $_ -match 'the connection to the mux server ended|client disconnected|Reconnected!|reattaching after a reconnect|detached domain' })
}
function Get-Pct([double[]]$Values, [double]$P) {
  if (-not $Values) { return $null }
  $s = @($Values | Sort-Object); $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($P * ($s.Count - 1) + 0.5))]
}
# The flooding pane as the frame drew it
function Get-FloodPane($sw, [int]$Rid) { @($sw.panes | Where-Object { [int]$_.server_id -eq $Rid })[0] }

$results = [ordered]@{ Pairing = $Pairing; Window = $guiExe; Server = $serverExe; Locked = (Test-WorkstationLocked); Rounds = @() }

Invoke-Test {
  $open = @{ ConfigFile = $cfg; Domain = 'pacemux'; Socket = $T.Sock; TimeoutSec = 30; Exe = $guiExe }
  if ($Log) { $open.Log = $Log }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  $server = @(Get-TestServers)
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers for the window" }
  $sp = [int]$server[0].ProcessId
  Start-MemoryGuard -Pids $w.Pid, $sp -LimitGB $MaxMemGB
  # The tabs are built on the server, whose first tab runs its own default program (PITFALLS 202)
  $shell = $panes[0].pane_id
  $rid = New-Tab -From $shell -Program $Flood -Server
  Wait-State { param($s) @($s.tabs).Count -eq 2 } -What 'the flooding tab in the window' -TimeoutSec 30 | Out-Null
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 1 } -What "the one tab once the shell's gone" -TimeoutSec 30 | Out-Null
  Wait-State { param($s) [bool](Get-FloodPane $s $rid) }.GetNewClosure() -What 'the flooding tab drawn' -TimeoutSec 15 | Out-Null
  $guiMain = [LbtermPace]::MainThread($w.Pid); $serverMain = [LbtermPace]::MainThread($sp)
  $serverLog = Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$sp.txt"
  $hasMetrics = $true
  try { Get-Metrics $w -Reset -TimeoutMs 4000 | Out-Null } catch { $hasMetrics = $false }
  $h = Resolve-Handle $w

  for ($r = 1; $r -le $Rounds; $r++) {
    $round = [ordered]@{ Round = $r }
    if ($hasMetrics) { Get-Metrics $w -Reset | Out-Null }
    $logAt = Get-LogLength $serverLog
    $s0 = Read-State; $f0 = Get-StateWindow $s0 -Hwnd ([long]$h)
    $c0 = [Diagnostics.Stopwatch]::GetTimestamp()
    $gui0 = [LbtermPace]::ThreadCycles($guiMain); $srv0 = [LbtermPace]::ThreadCycles($serverMain)
    [IO.File]::WriteAllText((Join-Path $Dir "go-$r"), '')
    # 1. flood, with a key every -KeyEveryMs into it
    $times = [Collections.Generic.List[double]]::new(); $late = 0
    $ages = [Collections.Generic.List[double]]::new(); $lastAgeFrame = -1
    $span = [Diagnostics.Stopwatch]::StartNew(); $i = 0
    while ($span.Elapsed.TotalSeconds -lt $Seconds) {
      $ch = [string][char](97 + ($i % 26))
      $t0 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
      Send-Chars $w $ch
      Watch-Foreground
      $title = "key-$ch"
      try {
        Wait-State { param($sw) $p = Get-FloodPane $sw $rid; $p -and ($p.title -eq $title -or $p.title -like "$title t=*") }.GetNewClosure() -What "key $ch's title" -TimeoutSec 3 -IntervalMs 5 | Out-Null
        $times.Add([long](Get-Test).LastState.time_ms - $t0)
      } catch { $late++ }
      $i++
      $left = $KeyEveryMs - ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $t0)
      if ($Age) {
        # Till the next key, each frame the state file says: how old the newest block it drew of
        # the flood was, from its title's time to the snapshot's (taken just after the frame)
        $until = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() + $left
        while ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -lt $until) {
          $sa = Read-State; $wa = Get-StateWindow $sa -Hwnd ([long]$h)
          if ($wa -and [long]$wa.frame -ne $lastAgeFrame) {
            $lastAgeFrame = [long]$wa.frame
            $pa = Get-FloodPane $wa $rid
            if ($pa -and $pa.title -match ' t=(\d{13})$') { $ages.Add([long]$sa.time_ms - [long]$Matches[1]) }
          }
          Start-Sleep -Milliseconds 6
        }
      } elseif ($left -gt 0) { Start-Sleep -Milliseconds $left }
      $guard = Get-MemoryGuard
      if ($guard) { throw "the memory guard stopped the test: $guard" }
    }
    $c1 = [Diagnostics.Stopwatch]::GetTimestamp()
    $gui1 = [LbtermPace]::ThreadCycles($guiMain); $srv1 = [LbtermPace]::ThreadCycles($serverMain)
    $logTo = Get-LogLength $serverLog
    $s1 = Read-State; $f1 = Get-StateWindow $s1 -Hwnd ([long]$h)
    $sec = ($c1 - $c0) / [Diagnostics.Stopwatch]::Frequency
    $m = if ($hasMetrics) { try { Get-Metrics $w -TimeoutMs 4000 } catch { $null } }
    # 2. final: stopped, and its last screen drawn
    $want = Get-FinalCursor $r
    $wantX = $want.Col - 1; $wantY = $want.Row - 1
    $finalTitle = "pace-final-$r"
    [IO.File]::WriteAllText((Join-Path $Dir "stop-$r"), '')
    $doneFile = Join-Path $Dir "done-$r"
    Wait-Until { Test-Path -LiteralPath $doneFile } -TimeoutSec 10 -IntervalMs 5 | Out-Null
    $doneMs = if (Test-Path -LiteralPath $doneFile) { [long]([IO.File]::ReadAllText($doneFile).Trim()) } else { $null }
    $drawn = { param($sw)
      $p = Get-FloodPane $sw $rid
      $c = if ($p.drawn_cursor) { $p.drawn_cursor } else { $p.cursor }
      $p -and $p.title -eq $finalTitle -and $c -and [int]$c.x -eq $wantX -and [int]$c.y -eq $wantY
    }.GetNewClosure()
    $finalMs = $null
    try {
      Wait-State $drawn -What "round $r's final screen" -TimeoutSec ([Math]::Max(2, $SettleMs / 1000 + 1)) -IntervalMs 5 | Out-Null
      if ($doneMs) { $finalMs = [long](Get-Test).LastState.time_ms - $doneMs }
    } catch { }
    $last = Get-FloodPane (Get-StateWindow (Read-State) -Hwnd ([long]$h)) $rid
    $pl = Get-PaneLog $serverLog $logAt $logTo $rid
    $round.Seconds = [Math]::Round($sec, 2)
    $round.FramesPerSec = [Math]::Round(([long]$f1.frame - [long]$f0.frame) / (([long]$s1.time_ms - [long]$s0.time_ms) / 1000), 1)
    $round.PushLines = $pl.Lines
    $round.PushLogSeconds = [Math]::Round($pl.Seconds, 2)
    $round.PushesPerSec = if ($pl.Seconds) { [Math]::Round($pl.Pushed / $pl.Seconds, 1) } else { 0 }
    $round.Waited = $pl.Waited
    $round.WaitedMsMean = if ($pl.Waited) { [Math]::Round($pl.WaitedUs / $pl.Waited / 1000, 2) } else { $null }
    $round.WindowPushesPerSec = if ($m) { [Math]::Round((Get-Pdus $m 'GetPaneRenderChangesResponse' 'received') / $sec, 1) } else { $null }
    $round.PanesDrawnPerSec = if ($m) { [Math]::Round((Get-Pdus $m 'PanesDrawn' 'sent') / $sec, 1) } else { $null }
    $round.ServerMainMsPerSec = [Math]::Round(($srv1 - $srv0) / $rate / $sec, 1)
    $round.GuiMainMsPerSec = [Math]::Round(($gui1 - $gui0) / $rate / $sec, 1)
    $round.Keys = $times.Count + $late; $round.KeysLate = $late
    $round.KeysPerSec = [Math]::Round($round.Keys / $sec, 1)
    $round.EchoP50 = Get-Pct $times 0.5; $round.EchoP95 = Get-Pct $times 0.95; $round.EchoMax = ($times | Measure-Object -Maximum).Maximum
    $round.EchoMs = @($times)
    if ($Age) { $round.AgeP50 = Get-Pct $ages 0.5; $round.AgeP95 = Get-Pct $ages 0.95; $round.AgeMax = ($ages | Measure-Object -Maximum).Maximum; $round.AgeFrames = $ages.Count }
    $round.FinalMs = $finalMs
    $round.FinalDrawn = $null -ne $finalMs
    $round.LastFrame = if ($last) { "title '$($last.title)', cursor $($last.drawn_cursor.x),$($last.drawn_cursor.y)" } else { 'no pane' }
    $results.Rounds += [pscustomobject]$round
    "round ${r}: $($round.Seconds) s, $($round.FramesPerSec) frames/s, the shown pane $($round.PushesPerSec) pushes/s ($($round.Waited) waited for a frame's word, $($round.WaitedMsMean) ms each), the window took $($round.WindowPushesPerSec) pushes/s and sent $($round.PanesDrawnPerSec) PanesDrawn/s; server main $($round.ServerMainMsPerSec) ms/s, GUI $($round.GuiMainMsPerSec) ms/s; keys $($round.Keys) ($late late), echo $($round.EchoP50)/$($round.EchoP95)/$($round.EchoMax) ms;$(if ($Age) { " the newest block a frame drew $($round.AgeP50)/$($round.AgeP95)/$($round.AgeMax) ms old ($($round.AgeFrames) frames);" }) final screen $(if ($null -ne $finalMs) { "drawn $finalMs ms after it was written" } else { "NOT drawn: the last frame had $($round.LastFrame)" })"
    if ($null -eq $finalMs) { Add-Failure "round ${r}: the final screen (title $finalTitle, cursor $wantX,$wantY) wasn't drawn within $SettleMs ms of the flood's stop; the last frame had $($round.LastFrame)" }
    elseif ($finalMs -gt $SettleMs) { Add-Failure "round ${r}: the final screen was drawn $finalMs ms after it was written, over $SettleMs" }
    if ($late) { Add-Failure "round ${r}: $late of $($round.Keys) keys' titles never drawn within 3 s" }
    if (-not $Measure) {
      if ($OwnPair) {
        if (-not $pl.Lines) { Add-Failure "round ${r}: the server's push log has no line for the shown pane ${rid}: nothing to judge the pace by" }
        $bound = $PushRatio * $round.FramesPerSec + 2 * $round.KeysPerSec
        if ($round.PushesPerSec -gt $bound) { Add-Failure "round ${r}: the shown pane was pushed $($round.PushesPerSec) times a second against $($round.FramesPerSec) frames ($($round.KeysPerSec) keys a second): over $([Math]::Round($bound, 1))" }
        if (-not $pl.Waited) { Add-Failure "round ${r}: no push of the shown pane waited for a frame's word: the pace never ran" }
        if ($null -ne $round.EchoP50 -and $round.EchoP50 -gt $EchoMs) { Add-Failure "round ${r}: the keys' echo median $($round.EchoP50) ms, over $EchoMs" }
      } elseif ($pl.Waited) {
        Add-Failure "round ${r}: $($pl.Waited) pushes waited for a frame's word with $Pairing, where nothing is paced"
      }
    }
  }
  $results.Connection = @(Get-ConnectionLines $w.Log $w.LogStart)
  if ($results.Connection.Count) { Add-Failure "the window's connection ended: $($results.Connection -join ' | ')" }
  "pairing: $Pairing; the window's connection $(if ($results.Connection.Count) { 'ended' } else { 'never ended' })"
  $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-pace.json')
} -Passed "mux-pace ($Pairing): every round's last screen drawn once the flood in view stopped$(if ($OwnPair) { ', the shown pane pushed at most once a frame and a key, each push waiting for a frame to read the last' } else { ', nothing paced' })"
