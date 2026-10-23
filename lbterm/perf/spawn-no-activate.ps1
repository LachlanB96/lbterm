#Requires -Version 7
# Covers: wezterm/src/cli/spawn_command.rs, codec/src/lib.rs, wezterm-client/src/client.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
`lbterm cli spawn --no-activate` adds a tab without switching the window to it, so nothing
Lachlan types lands in a tab an agent opened (Conductor's sessions, AI Dashboard's launches).

In test windows of their own class, never the daily LBTerm:

1. A window whose tabs are its own, spawned into through its GUI socket. A plain spawn switches
   the window to the new tab, as upstream's does. A spawn with --no-activate leaves it where it
   was.
2. A window on a mux domain of its own (target\perf\spawn-no-activate), as Lachlan's LBTerm is.
   The server's Lua logs each change of the tab its window has active, looking every 50 ms
   (server.json, a line each); the GUI's state file says which pane it shows. With --no-activate
   neither ever moves to the new tab. A plain spawn moves the server's record but not the GUI:
   the cli's fallback for an older server relies on that.
3. With -OldServer <folder of an older build>: the mux server is that build, which doesn't know
   the SpawnNoActivate PDU. --no-activate still opens the tab (spawned the usual way, with a line
   on stderr saying so) and the GUI stays where it was.

  pwsh -File lbterm\perf\spawn-no-activate.ps1 [-Bin <build>] [-OldServer <build>]
#>
param([string]$Bin, [string]$OldServer)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'spawn-no-activate' -Bin $Bin
if ($OldServer) { $OldServer = (Resolve-Path $OldServer).Path }
$note = Join-Path $env:APPDATA "lbterm\session-$($T.Class).json"
# The configs and the server's report share a folder, as they always have here: see
# Write-Configs on why the report must sit beside the config. The socket doesn't, and each
# domain has the same one in turn: a socket's path must be under 108 characters (AF_UNIX), and an
# e2e run's folder in a worktree is most of that
$dir = Join-Path $T.Out 'config'
$sock = Join-Path $T.Out 'sock'
New-Item -ItemType Directory -Force $dir | Out-Null
$serverReport = Join-Path $dir 'server.json'
$pwshExe = 'C:/Program Files/PowerShell/7/pwsh.exe'
$sleeper = @($pwshExe, '-NoProfile', '-Command', 'Start-Sleep 600')

# The GUI's config, and with -Domain the server's too (serve_command names it, so the server
# writes server.json and the GUI doesn't)
function Write-Configs([string]$Domain, [string]$ServerExe) {
  $guiCfg = Join-Path $dir "$($Domain ? $Domain : 'local')-gui.lua"
  $serverCfg = Join-Path $dir "$Domain-server.lua"
  # Reloading stays on here, unlike every other test window: the server's report is written
  # beside its config, each write reloads it, and the reload's fresh Lua state is what keeps the
  # server's call_after tick sampling its window (with reloading off it never reports)
  $common = @"
  front_end = 'WebGpu',
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  default_prog = { '$pwshExe', '-NoProfile', '-Command', 'Start-Sleep 600' },
  default_cwd = 'C:/Windows',
"@
  $domains = $Domain ? @"
  unix_domains = { {
    name = '$Domain',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $ServerExe)', '--config-file', '$(ConvertTo-LuaPath $serverCfg)', '--daemonize' },
  } },
"@ : ''
  # The GUI writes gui.json beside the configs every 250 ms, and nothing reads it: each write
  # reloads the server's config too, which is what starts the server's tick at all (without them
  # server.json stays empty). What the GUI shows comes from its state file.
  [IO.File]::WriteAllText($guiCfg, @"
local wezterm = require 'wezterm'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
wezterm.on('update-status', function(window, pane)
  local f = io.open('$(ConvertTo-LuaPath (Join-Path $dir 'gui.json'))', 'w')
  if f then f:write(encode({ tabs = #window:mux_window():tabs() })); f:close() end
end)
return {
  status_update_interval = 250,
$common
$domains
}
"@)
  if ($Domain) {
    [IO.File]::WriteAllText($serverCfg, @"
local wezterm = require 'wezterm'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
local last
local function tick()
  pcall(function()
    local w = wezterm.mux.all_windows()[1]
    if not w then return end
    local line = encode({ tab = w:active_tab():tab_id(), tabs = #w:tabs() })
    if line == last then return end
    last = line
    local f = io.open('$(ConvertTo-LuaPath $serverReport)', 'a')
    if f then f:write(line .. '\n'); f:close() end
  end)
  wezterm.time.call_after(0.05, tick)
end
wezterm.time.call_after(0.05, tick)
return {
$common
$domains
}
"@)
  }
  [pscustomobject]@{ Gui = $guiCfg; Server = $serverCfg; Sock = $sock }
}

# What the GUI shows, as its state file says: the pane with the keyboard (by the server's id on a
# domain, as the cli names it) and how many tabs (was its Lua's gui.json)
function Get-GuiView {
  $x = Get-StateWindow (Read-State) -Hwnd ([long]$T.Current.Handle)
  $p = @($x.panes | Where-Object { $_.id -eq $x.active_pane })[0]
  [pscustomobject]@{ active = $(if ($null -ne $p.server_id) { $p.server_id } else { $x.active_pane }); tabs = @($x.tabs).Count }
}

# Every change the server's window went through, oldest first
function Read-ServerLog {
  @(Get-Content $serverReport -ErrorAction SilentlyContinue | ForEach-Object {
      try { $_ | ConvertFrom-Json } catch { } })
}

# Spawns into the window of $From, waits for the GUI (and the server, when there is one) to
# count $Tabs tabs and a moment more for a late switch, then says what each shows, and every
# tab the server's window had active since the spawn. The cli talks to the server on a domain,
# else to the GUI.
function Invoke-Spawn([int]$From, [int]$Tabs, [switch]$NoActivate, [switch]$Server) {
  $flags = @('--pane-id', $From) + ($NoActivate ? @('--no-activate') : @())
  $before = (Read-ServerLog).Count
  # stdout (the new pane's id) and stderr (the older server's fallback) together, told apart after
  $all = @(Invoke-Cli spawn @flags -Program $sleeper -Server:$Server -Merge)
  $code = $LASTEXITCODE
  $new = $all | Where-Object { $_ -match '^\s*\d+\s*$' } | Select-Object -Last 1
  $stderr = ($all | Where-Object { $_ -notmatch '^\s*\d+\s*$' }) -join "`n"
  if ($code -ne 0) { throw "spawn $flags failed ($code): $stderr" }
  # The GUI painting the new tab and still, and the server counting it (the checks below say so if
  # they never do)
  try { Wait-Change { param($x) @($x.tabs).Count -eq $Tabs } -Animating -What "$Tabs tabs in the GUI" -TimeoutSec 10 | Out-Null } catch { }
  if ($Server) { Wait-Until { (Read-ServerLog | Select-Object -Last 1).tabs -eq $Tabs } -TimeoutSec 10 -IntervalMs 100 | Out-Null }
  # A late switch would come after: no state says it won't
  Start-Sleep 1.5
  $g = Get-GuiView
  $log = Read-ServerLog
  $s = $log | Select-Object -Last 1
  $panes = Get-Panes -Server:$Server
  [pscustomobject]@{
    New = [int]"$new".Trim(); Gui = $g.active; GuiTabs = $g.tabs; Stderr = $stderr.Trim()
    # The server can run its config more than once, each run logging, so repeats fold into one
    ServerTab = $s.tab; ServerTabs = $s.tabs; ServerSeen = @($log | Select-Object -Skip $before | ForEach-Object tab | Get-Unique)
    TabOf = { param($pane) ($panes | Where-Object pane_id -eq $pane | Select-Object -First 1).tab_id }.GetNewClosure()
  }
}

# Opens a test window, on the given domain or on its own, and waits for its first pane
function Open-Window($Cfg, [string]$Domain) {
  Remove-Item $note, $serverReport, $Cfg.Sock -ErrorAction SilentlyContinue
  $w = if ($Domain) { Open-TestWindow -ConfigFile $Cfg.Gui -Domain $Domain -Socket $Cfg.Sock -TimeoutSec 15 }
  else { Open-TestWindow -ConfigFile $Cfg.Gui -TimeoutSec 15 }
  $server = [bool]$Domain
  Wait-Until { @(Get-Panes -Server:$server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250 | Out-Null
  $panes = @(Get-Panes -Server:$server)
  if ($panes.Count -lt 1) { throw "no pane in the test window" }
  # The GUI showing it, and still
  Wait-Change { param($x) $x.frame -gt 0 -and @($x.tabs).Count -ge 1 } -Animating -What 'the window''s first tab' | Out-Null
  [pscustomobject]@{ Window = $w; First = $panes[0].pane_id; FirstTab = $panes[0].tab_id }
}

function Close-Window($w) {
  if ($w) { Stop-TestWindow $w.Window }
  Stop-TestServers
  Remove-Item $note -ErrorAction SilentlyContinue
}

Invoke-Test {
  # 1. The window's own tabs
  $w = Open-Window (Write-Configs) ''
  $r = Invoke-Spawn -From $w.First -Tabs 2
  "local, plain: the window shows $($r.Gui), the new pane is $($r.New)"
  if ($r.Gui -ne $r.New) { Add-Failure "local, plain: the window shows $($r.Gui), not the new pane $($r.New)" }
  $was = $r.Gui
  $r = Invoke-Spawn -From $w.First -Tabs 3 -NoActivate
  "local, --no-activate: the window shows $($r.Gui) ($was before), $($r.GuiTabs) tabs"
  if ($r.Gui -ne $was) { Add-Failure "local, --no-activate: the window moved to $($r.Gui) from $was" }
  if ($r.GuiTabs -ne 3) { Add-Failure "local, --no-activate: $($r.GuiTabs) tabs, not 3" }
  Close-Window $w

  # 2. A mux domain of its own
  $cfg = Write-Configs 'perfspawn' $T.MuxServer
  $w = Open-Window $cfg 'perfspawn'
  $r = Invoke-Spawn -From $w.First -Tabs 2 -NoActivate -Server
  $newTab = & $r.TabOf $r.New
  "mux, --no-activate: the GUI shows $($r.Gui); the server's window had tabs $($r.ServerSeen -join ', ') active since (the first is pane $($w.First), tab $($w.FirstTab); the new tab is $newTab)"
  if ($r.Gui -ne $w.First) { Add-Failure "mux, --no-activate: the GUI moved to $($r.Gui)" }
  if ($newTab -in $r.ServerSeen) { Add-Failure "mux, --no-activate: the server's window switched to the new tab $newTab" }
  if ($r.ServerTab -ne $w.FirstTab) { Add-Failure "mux, --no-activate: the server's window is on tab $($r.ServerTab)" }
  if ($r.ServerTabs -ne 2) { Add-Failure "mux, --no-activate: the server has $($r.ServerTabs) tabs, not 2" }
  # The other way round, so a switch is known to show in the server's log. The GUI's next
  # focus report can put the server back, so only that it switched counts
  $r = Invoke-Spawn -From $w.First -Tabs 3 -Server
  $newTab = & $r.TabOf $r.New
  "mux, plain: the GUI shows $($r.Gui); the server's window had tabs $($r.ServerSeen -join ', ') active since (the new tab is $newTab)"
  if ($newTab -notin $r.ServerSeen) { Add-Failure "mux, plain: the server's window never switched to the new tab $newTab, so the log can't show a switch" }
  if ($r.Gui -ne $w.First) { Add-Failure "mux, plain: the GUI followed the server to $($r.Gui), so the cli's fallback for an older server would take the keyboard" }
  Close-Window $w

  # 3. An older server, which doesn't know SpawnNoActivate
  if ($OldServer) {
    $cfg = Write-Configs 'perfspawnold' (Join-Path $OldServer 'lbterm-mux-server.exe')
    $w = Open-Window $cfg 'perfspawnold'
    $r = Invoke-Spawn -From $w.First -Tabs 2 -NoActivate -Server
    "old server, --no-activate: the GUI shows $($r.Gui), $($r.ServerTabs) tabs; stderr: $($r.Stderr)"
    if ($r.Stderr -notmatch 'older build') { Add-Failure "old server: no word of the fallback on stderr (does $OldServer know SpawnNoActivate?)" }
    if ($r.Gui -ne $w.First) { Add-Failure "old server: the GUI moved to $($r.Gui)" }
    if ($r.ServerTabs -ne 2) { Add-Failure "old server: the server has $($r.ServerTabs) tabs, not 2" }
    Close-Window $w
  }
} -Passed 'spawn-no-activate: --no-activate leaves the window and its server on the tab they were on'
