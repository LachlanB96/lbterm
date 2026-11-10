#Requires -Version 7
# Variant: -Local
# Covers: wezterm-client/src/pane/clientpane.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/frontend.rs, wezterm-gui/src/main.rs, config/src/lib.rs, lua-api-crates/window-funcs/src/lib.rs, lbterm-extras/src/appearance.rs
<#
What a config save and a GUI's start cost (lbterm/MUX.md, "What a config save costs"): a window that
looks like Lachlan's LBTerm (New-TestConfig, its reloads on) with -Tabs tabs on a mux domain of the
test's own (-Local: the window's own panes), each running a stand-in (fake-status.ps1) that has
printed its line and waits. Each process that runs the config writes a line to a file of its own.

1. The start: how many times the GUI, its mux server and each `lbterm cli spawn` ran the config,
   and how long the GUI took from its process starting to its first frame (the state file's).
2. A save of the config, word for word, as an editor's or another session's: the GUI's CPU and its
   main thread's, the mux server's and the frames in the -Settle seconds after it; then in a second
   window with WEZTERM_LOG=codec=debug and lbterm::palette=debug, the SetPalette PDUs each side
   sent, the GUI's GetLines (lines it had made stale and fetched again) and, from builds that say,
   the times the GUI cleared its shapes for a pane's palette or found it had that palette already.
3. What must still change: from captures, so not on a locked desktop (result.json and the last
   line say "colours unchecked" then), the config's background changed in a save reaches the open
   pane, and changed back, again; and by counts in the second window, locked or not, a save
   changing the background has the GUI send the server each pane's new palette once and the
   server send each back at most once, and again for the save changing it back. (A program's own
   colours, OSC 10 and 11, never reach the terminal through ConPTY: in the installed build of
   2026-10-30 the pane kept the config's background after one, so a program's palette can't be
   tested here.)

It fails when the GUI's start runs the config more than once, when a save sends any pane's palette
back to the GUI, when it makes the GUI fetch lines again or clear its shapes, when a save doesn't
say "Config reloaded" on the strip, when a colour doesn't show as it should, or when a changed
background isn't sent once for each pane or comes back more than once. cost.json in
target\perf\config-save has every number.

Measured on 2026-10-30, three runs of each build in turn: before these fixes (main), the GUI ran
the config twice as it started, and a save word for word sent 12 palettes up and 24 back, fetched
the open pane's lines again and cost 156 to 206 ms of the GUI (100 to 133 ms of its thread) and 52
to 67 ms of the server. After: once; no palette either way, no lines fetched, 98 to 129 ms (58 to
84) and 26 to 53 ms. The first save after a window opens sometimes resizes every pane, in both
builds (upstream's apply_dimensions), so it isn't held to fetching no lines.

  pwsh -File lbterm\perf\config-save.ps1 [-Local] [-Tabs 12] [-Settle 3] [-NoCount] [-Measure] [-Bin <build>]
#>
param(
  [string]$Bin,
  [switch]$Local,
  [int]$Tabs = 12,
  [double]$Settle = 3,
  # Only the cost window, not the counting one (and no budgets that need its counts)
  [switch]$NoCount,
  # Only measure: no budgets
  [switch]$Measure
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('config-save' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermSaveCost {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  const uint THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000;
  public static double Rate(int ms) {
    ulong a, b; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out a);
    while (sw.ElapsedMilliseconds < ms) { }
    QueryThreadCycleTime(GetCurrentThread(), out b);
    return (b - a) / sw.Elapsed.TotalSeconds;
  }
  public static ulong ProcessCycles(int pid) {
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, (uint)pid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryProcessCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  public static ulong ThreadCycles(int id) {
    IntPtr h = OpenThread(THREAD_QUERY_LIMITED, false, (uint)id);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryThreadCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  public static int MainThread(int pid) {
    using (var p = Process.GetProcessById(pid)) {
      ProcessThread first = null;
      foreach (ProcessThread t in p.Threads) {
        try { if (first == null || t.StartTime < first.StartTime) first = t; } catch { }
      }
      return first == null ? 0 : first.Id;
    }
  }
}
'@

$ctl = Join-Path $T.Out 'ctl'
New-Item -ItemType Directory -Force $ctl | Out-Null
$helper = Join-Path $PSScriptRoot 'fake-status.ps1'
$names = @('open') + @(1..($Tabs - 1) | ForEach-Object { "tab $_" })
function Get-Control([string]$Name) { Join-Path $ctl (($Name -replace ' ', '-') + '.txt') }
function Set-Control([string]$Name, [string]$Line) {
  $f = Get-Control $Name
  for ($i = 0; ; $i++) {
    try { [IO.File]::WriteAllText($f, $Line); return } catch { if ($i -ge 100) { throw }; Start-Sleep -Milliseconds 5 }
  }
}
function Get-Program([string]$Name) {
  Set-Control $Name 'rest'
  New-PwshProgram -File $helper -ArgumentList '-Name', $Name, '-Control', (Get-Control $Name)
}

# The config, in a folder of its own (WezTerm watches the folder too), its reloads on, writing a
# line to a file named by the pid of whatever runs it (one file each: a GUI and its server running
# it at once lost a line appending to one). The background is the one thing a save changes, when
# it does
$cfg = Join-Path $T.Out 'config\config-save.lua'
$runs = Join-Path $T.Out 'runs'
New-Item -ItemType Directory -Force (Split-Path $cfg), $runs | Out-Null
$base = '#102040'
function Write-Config([string]$Background) {
  $domain = if (-not $Local) {
    @"
config.unix_domains = { {
  name = 'configsave',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  } else { '' }
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 140
config.initial_rows = 40
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.window_close_confirmation = 'NeverPrompt'
config.colors = config.colors or {}
config.colors.background = '$Background'
config.automatically_reload_config = true
do
  local f = io.open('$(ConvertTo-LuaPath $runs)/' .. tostring(wezterm.procinfo.pid()) .. '.txt', 'a')
  if f then f:write('ran\n') f:close() end
end
$domain
"@
}
Write-Config $base

$rate = [LbtermSaveCost]::Rate(300)
$ms = { param([double]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }
$mux = -not $Local

# Every run so far, as the pid of the process that ran it, once a run
function Get-Runs {
  @(Get-ChildItem -LiteralPath $runs -Filter '*.txt' -ErrorAction SilentlyContinue | ForEach-Object {
    $id = [int]$_.BaseName
    @(Get-Content -LiteralPath $_.FullName | Where-Object { $_ }) | ForEach-Object { $id }
  })
}
function Count-Runs([int[]]$All, [int]$ProcessId) { @($All | Where-Object { $_ -eq $ProcessId }).Count }

# A window with every tab running its stand-in, settled, and what its start ran
function Open-SaveWindow([string]$Log) {
  Get-ChildItem -LiteralPath $runs -Filter '*.txt' -ErrorAction SilentlyContinue | Remove-Item -ErrorAction SilentlyContinue
  $open = @{ ConfigFile = $cfg; Program = (Get-Program 'open'); TimeoutSec = 20 }
  if ($Log) { $open.Log = $Log }
  if ($mux) { $open.Domain = 'configsave'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window''s first frame' -TimeoutSec 20 | Out-Null
  # When it was let run, after the handoff to the test's job
  $started = $w.Ran
  $firstFrameMs = [int]((Get-Date) - $started).TotalMilliseconds
  if ($mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  $server = if ($mux) { @(Get-TestServers)[0] } else { $null }
  $serverPid = if ($server) { [int]$server.ProcessId } else { 0 }
  $atStart = Get-Runs
  $start = [ordered]@{
    Gui = Count-Runs $atStart $w.Pid
    Server = $(if ($serverPid) { Count-Runs $atStart $serverPid } else { $null })
    Other = @($atStart | Where-Object { $_ -ne $w.Pid -and $_ -ne $serverPid }).Count
    FirstFrameMs = $firstFrameMs
  }
  $first = (Get-Panes -Server:$mux)[0].pane_id
  if ($mux) {
    $default = $first
    $first = New-Tab -From $default -Program (Get-Program 'open') -Server
    Wait-Change { param($s) @($s.tabs).Count -eq 2 } -What 'the open tab''s stand-in' | Out-Null
    Invoke-Cli kill-pane --pane-id $default -Server -Check | Out-Null
    Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'the default tab gone' | Out-Null
  }
  for ($i = 1; $i -lt $names.Count; $i++) { New-Tab -From $first -NoActivate -Program (Get-Program $names[$i]) -Server:$mux | Out-Null }
  Wait-Change { param($s) @($s.tabs).Count -eq $Tabs } -What "$Tabs tabs" | Out-Null
  $spawned = Get-Runs
  $start.Spawns = @($spawned | Where-Object { $_ -ne $w.Pid -and $_ -ne $serverPid }).Count - $start.Other
  $start.GuiAfterTabs = Count-Runs $spawned $w.Pid
  Start-Sleep 2
  # Every tab's title and folder, as a Claude tab's are (fake-status.ps1's `hello`)
  foreach ($n in $names) { Set-Control $n 'hello' }
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.title -eq 'open' }).Count -eq 1 } -What 'the open tab''s title' -TimeoutSec 10 | Out-Null
  Wait-Settled -What "$Tabs tabs at rest" -TimeoutSec 15 | Out-Null
  Write-Host "start: the GUI ran the config $($start.Gui) times$(if ($mux) { ", its server $($start.Server)" }), $($start.Spawns) for $($Tabs - 1) cli spawns; first frame $firstFrameMs ms after the GUI started"
  [pscustomobject]@{ Window = $w; First = $first; Server = $serverPid; Main = [LbtermSaveCost]::MainThread($w.Pid); Start = [pscustomobject]$start }
}

function Get-Frame($w) { $s = Read-State -Window $w; [long]$s.frame }
function Get-Size([string]$Path) { if (Test-Path -LiteralPath $Path) { (Get-Item -LiteralPath $Path).Length } else { 0 } }
function Read-From([string]$Path, [long]$From) {
  if (-not (Test-Path -LiteralPath $Path)) { return '' }
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
}
function Get-ServerLog([int]$ProcessId) { Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$ProcessId.txt" }

# The config saved with $Background, and what the -Settle seconds after cost
function Measure-Save($o, [string]$Background, [switch]$Count) {
  $w = $o.Window
  $guiLog = $w.Log; $srvLog = if ($o.Server) { Get-ServerLog $o.Server } else { $null }
  # The last save's notice gone (it shows 4 s), so this one's is its own
  Wait-State { param($s) -not $s.strip.notice } -What 'the last notice gone' -TimeoutSec 6 | Out-Null
  $r0 = Get-Runs
  $f0 = Get-Frame $w
  $c0 = [LbtermSaveCost]::ProcessCycles($w.Pid); $m0 = [LbtermSaveCost]::ThreadCycles($o.Main)
  $sv0 = if ($o.Server) { [LbtermSaveCost]::ProcessCycles($o.Server) } else { 0 }
  $g0 = Get-Size $guiLog; $s0 = if ($srvLog) { Get-Size $srvLog } else { 0 }
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Write-Config $Background
  $said = $true
  try { Wait-State { param($s) $s.strip.notice -match 'Config reloaded' } -What '"Config reloaded" on the strip' -TimeoutSec 5 | Out-Null } catch { $said = $false }
  $saidMs = $clock.ElapsedMilliseconds
  $left = [int]($Settle * 1000 - $clock.ElapsedMilliseconds)
  if ($left -gt 0) { Start-Sleep -Milliseconds $left }
  $r1 = Get-Runs
  $row = [ordered]@{
    Background = $Background; Said = $said; SaidMs = $saidMs
    GuiRuns = (Count-Runs $r1 $w.Pid) - (Count-Runs $r0 $w.Pid)
    ServerRuns = $(if ($o.Server) { (Count-Runs $r1 $o.Server) - (Count-Runs $r0 $o.Server) } else { $null })
    Frames = (Get-Frame $w) - $f0
    GuiMs = & $ms ([LbtermSaveCost]::ProcessCycles($w.Pid) - $c0)
    MainMs = & $ms ([LbtermSaveCost]::ThreadCycles($o.Main) - $m0)
    ServerMs = $(if ($o.Server) { & $ms ([LbtermSaveCost]::ProcessCycles($o.Server) - $sv0) } else { $null })
  }
  if ($Count) {
    $g = Read-From $guiLog $g0
    $s = if ($srvLog) { Read-From $srvLog $s0 } else { '' }
    $row.ServerSetPalette = [regex]::Matches($s, 'encode(?:_async)? SetPalette size=').Count
    $row.GuiSetPalette = [regex]::Matches($g, 'encode(?:_async)? SetPalette size=').Count
    $row.GuiGetLines = [regex]::Matches($g, 'encode(?:_async)? GetLines size=').Count
    $row.GuiResize = [regex]::Matches($g, 'encode(?:_async)? Resize size=').Count
    $row.ShapesCleared = [regex]::Matches($g, 'lbterm::palette\s+> palette pane \d+ window \d+: shapes cleared').Count
    $row.HadIt = [regex]::Matches($g, 'lbterm::palette\s+> palette pane \d+: the one it has').Count
  }
  $line = "save$(if ($Count) { ' (counted)' }) to $($Background): $(if ($said) { "said in $saidMs ms" } else { 'never said' }); the GUI ran the config $($row.GuiRuns) times$(if ($o.Server) { ", the server $($row.ServerRuns)" }); $($row.Frames) frames; GUI $($row.GuiMs) ms (main thread $($row.MainMs))$(if ($o.Server) { ", server $($row.ServerMs) ms" })"
  if ($Count) { $line += "; SetPalette from the server $($row.ServerSetPalette), from the GUI $($row.GuiSetPalette); GetLines $($row.GuiGetLines); Resize $($row.GuiResize); shapes cleared $($row.ShapesCleared), palettes it had $($row.HadIt)" }
  Write-Host $line
  [pscustomobject]$row
}

# The open pane's background in a capture: the mean colour of a block at its bottom right, where
# the stand-in printed nothing
function Get-PaneColour($w) {
  $s = Read-State -Window $w
  $p = @($s.panes | Where-Object active)[0]
  if (-not $p) { $p = @($s.panes)[0] }
  $shot = Get-Shot $w
  try {
    $x0 = [int]($p.rect.x + $p.rect.w * 0.7); $y0 = [int]($p.rect.y + $p.rect.h * 0.6)
    $r = 0; $g = 0; $b = 0; $n = 0
    for ($y = $y0; $y -lt $y0 + 20; $y += 2) { for ($x = $x0; $x -lt $x0 + 40; $x += 2) { $c = $shot.GetPixel($x, $y); $r += $c.R; $g += $c.G; $b += $c.B; $n++ } }
    [pscustomobject]@{ R = [int]($r / $n); G = [int]($g / $n); B = [int]($b / $n) }
  } finally { $shot.Dispose() }
}
function Format-Colour($c) { "rgb($($c.R), $($c.G), $($c.B))" }

$saves = [Collections.Generic.List[object]]::new()
$counted = [Collections.Generic.List[object]]::new()
$changes = [Collections.Generic.List[object]]::new()
$colours = [Collections.Generic.List[object]]::new()
$starts = [Collections.Generic.List[object]]::new()
# Captures leave out what DirectComposition shows while it's locked: the colours go unchecked
# then, and only the counts say a changed background went where it should
$locked = Test-WorkstationLocked
Invoke-Test {
  # 1 and 2. The start, and a save word for word
  # The first reload after a window opens resizes every pane (upstream's apply_dimensions, the
  # palette's no part of it): kept apart, as `First`
  $o = Open-SaveWindow
  $starts.Add($o.Start)
  $first = Measure-Save $o $base
  $first | Add-Member First $true
  $saves.Add($first)
  $saves.Add((Measure-Save $o $base))
  $saves.Add((Measure-Save $o $base))

  # 3. What must still change
  $w = $o.Window
  if ($locked) {
    $note = 'colours unchecked: the workstation is locked, and captures leave out what DirectComposition shows (a changed background is held to its counts alone)'
    Write-Host "NOTE: $note"
    (Get-Test).Notes.Add($note)
  } else {
    $look = { param($what) $c = Get-PaneColour $w; $colours.Add([pscustomobject]@{ What = $what; Colour = Format-Colour $c }); Write-Host "$($what): $(Format-Colour $c)"; $c }
    $blue = & $look "the config's background $base"
    Measure-Save $o '#402010' | Out-Null
    $red = & $look "the config's background changed to #402010"
    if (-not ($red.R -gt $red.B + 15 -and $blue.B -gt $blue.R + 15)) { Add-Failure "the config's new background didn't reach the open pane: $(Format-Colour $blue) before the save, $(Format-Colour $red) after" }
    Measure-Save $o $base | Out-Null
    $back = & $look "the config's background back to $base"
    if (-not ($back.B -gt $back.R + 15)) { Add-Failure "the config's background changed back didn't reach the open pane: $(Format-Colour $back)" }
  }

  # 2. Counts
  if (-not $NoCount) {
    Stop-TestWindow $w
    Stop-TestServers
    Start-Sleep 1
    $o = Open-SaveWindow -Log 'info,codec=debug,lbterm::palette=debug'
    $starts.Add($o.Start)
    $first = Measure-Save $o $base -Count
    $first | Add-Member First $true
    $counted.Add($first)
    $counted.Add((Measure-Save $o $base -Count))
    $counted.Add((Measure-Save $o $base -Count))
    # 3, by counts, whatever the lock: a changed background goes to the server once for each
    # pane, and comes back at most once each; then changed back
    $changes.Add((Measure-Save $o '#402010' -Count))
    $changes.Add((Measure-Save $o $base -Count))
  }

  if (-not $Measure) {
    foreach ($s in $starts) {
      if ($s.Gui -ne 1) { Add-Failure "the GUI ran the config $($s.Gui) times as it started: once is all it needs" }
    }
    foreach ($r in @($saves) + @($counted)) {
      if (-not $r.Said) { Add-Failure "a save of the config didn't say ""Config reloaded"" on the strip within 5 s" }
      if ($r.GuiRuns -gt 1) { Add-Failure "a save ran the config $($r.GuiRuns) times in the GUI" }
    }
    foreach ($r in $counted) {
      if ($r.ServerSetPalette) { Add-Failure "a save word for word had the server send the GUI SetPalette $($r.ServerSetPalette) times: every pane already has its palette" }
      if ($r.GuiSetPalette) { Add-Failure "a save word for word had the GUI send the server SetPalette $($r.GuiSetPalette) times: the server has every pane's already" }
      if ($r.ShapesCleared) { Add-Failure "a save word for word had the GUI clear its shapes $($r.ShapesCleared) times for panes' palettes" }
      # The first, which resizes every pane, fetches the open pane's lines again for that
      if ($r.GuiGetLines -and -not $r.First) { Add-Failure "a save word for word had the GUI fetch lines again $($r.GuiGetLines) times" }
    }
    foreach ($r in $changes) {
      if (-not $r.Said) { Add-Failure "a save changing the background to $($r.Background) didn't say ""Config reloaded"" on the strip within 5 s" }
      # Without a server there's nothing to send: the window's own panes take the config
      if ($mux) {
        if ($r.GuiSetPalette -ne $Tabs) { Add-Failure "a save changing the background to $($r.Background) had the GUI send the server SetPalette $($r.GuiSetPalette) times: once for each of the $Tabs panes" }
        if ($r.ServerSetPalette -gt $Tabs) { Add-Failure "a save changing the background to $($r.Background) had the server send the GUI SetPalette $($r.ServerSetPalette) times: at most once for each of the $Tabs panes" }
      }
    }
  }
  [pscustomobject]@{ Mux = $mux; Tabs = $Tabs; Locked = $locked; Starts = $starts; Saves = $saves; Counted = $counted; Changes = $changes; Colours = $colours } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "config-save$(if ($Local) { ' (local)' }): the GUI runs the config once as it starts, a save sends no palette back and refetches nothing, and $(if ($locked) { 'colours unchecked (the workstation was locked)' } else { 'colours still change' })$(if ($mux -and -not $NoCount) { '; a changed background goes up once a pane and back at most once' })"
