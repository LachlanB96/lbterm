#Requires -Version 7
# Covers: wezterm-client/src/client.rs, wezterm-client/src/domain.rs, wezterm-mux-server-impl/src/dispatch.rs
<#
A window whose mux server hangs up on it, as the daily server did four times on 2026-10-23 when
the PC hit its commit limit and it couldn't encode a PDU (wezterm-client's after_disconnect). In
a test window of its own class, on a domain and socket of its own (target\perf\mux-reconnect),
so the real lbterm-mux-server and its tabs are never touched:

1. A test GUI connects to the test domain, which starts a test lbterm-mux-server with
   LBTERM_TEST_HANGUP set. A second tab joins the first.
2. The hang-up file is written: the server takes it and drops the GUI's connection. The window
   must stay, say "Reconnected!" in its log, show both tabs again, be in the server's
   list-clients again, and get a third tab made in the server after the reconnect.
3. The server is killed. With no server to answer, the window must give up (its log says so)
   and close, as it did before for a dead server.

  pwsh -File lbterm\perf\mux-reconnect.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-reconnect' -Bin $Bin
$note = Join-Path $env:APPDATA "lbterm\session-$($T.Class).json"
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\mux-reconnect.lua'
$report = Join-Path $T.Out 'report.json'
$hangup = Join-Path $T.Out 'hangup'
Remove-Item $note, $hangup -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The server reads the same file (serve_command), so both know the test domain. The GUI's Lua
# writes the panes it has to report.json every half second. update-status comes again on its
# interval only after a set_right_status or set_left_status (upstream's rule too); else only after
# a title update or a modifier key, so the handler sets an empty right status every time. Without
# it nothing fired it after the reconnect unless some title update happened along (finding 71)
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local report = '$(ConvertTo-LuaPath $report)'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
wezterm.on('update-status', function(window, pane)
  local panes = {}
  for _, tab in ipairs(window:mux_window():tabs()) do
    for _, p in ipairs(tab:panes()) do
      local meta = p:get_metadata()
      panes[#panes + 1] = {
        pane_id = p:pane_id(),
        remote_pane_id = type(meta) == 'table' and meta.remote_pane_id or nil,
      }
    end
  end
  local f = io.open(report, 'w')
  if f then f:write(encode(panes)); f:close() end
  window:set_right_status('')
end)
return {
  automatically_reload_config = false,
  wsl_domains = {},
  front_end = 'WebGpu',
  initial_cols = 90,
  status_update_interval = 500,
  window_close_confirmation = 'NeverPrompt',
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  unix_domains = { {
    name = 'perfreconnect',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
}
"@)

# The panes the GUI's Lua last saw, once it has seen $count of them
function Read-Report([int]$count, [int]$seconds = 10) {
  $deadline = (Get-Date).AddSeconds($seconds)
  $panes = @()
  while ((Get-Date) -lt $deadline) {
    try { $panes = @(Get-Content $report -Raw -ErrorAction Stop | ConvertFrom-Json) } catch { $panes = @() }
    if ($panes.Count -ge $count) { break }
    Start-Sleep -Milliseconds 250
  }
  $panes
}

# Once the test server answers with at least $count panes
function Wait-ServerPanes([int]$count, [int]$seconds = 15) {
  $deadline = (Get-Date).AddSeconds($seconds)
  do {
    $panes = @(Get-Panes -Server)
    if ($panes.Count -ge $count) { return $panes }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  throw "the test mux server didn't answer with $count panes within $seconds s"
}

Invoke-Test {
  # 1. Connect, which starts the server with LBTERM_TEST_HANGUP (the GUI's environment)
  $win = Open-TestWindow -ConfigFile $cfg -Domain perfreconnect -Socket $sock -TimeoutSec 15 -Environment @{ LBTERM_TEST_HANGUP = $hangup }
  $panes = Wait-ServerPanes 1
  $server = Get-TestServers
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers running after the GUI connected" }
  New-Tab -From $panes[0].pane_id -Server | Out-Null
  $before = Read-Report 2
  if ($before.Count -ne 2) { throw "the GUI shows $($before.Count) panes before the hang-up, not 2" }
  $serverIds = @((Wait-ServerPanes 2).pane_id | Sort-Object)
  "connected: the GUI shows 2 panes (server's $($serverIds -join ', '))"

  # 2. The server hangs up on the GUI
  [IO.File]::WriteAllText($hangup, 'now')
  if (-not (Wait-Until { -not (Test-Path $hangup) } -TimeoutSec 5 -IntervalMs 100)) { throw "the test server didn't take the hang-up file within 5 s" }
  Remove-Item $report -ErrorAction SilentlyContinue
  if (-not (Wait-LogLine $win 'Reconnected!' -TimeoutSec 15)) { Add-Failure "the GUI's log has no 'Reconnected!' 15 s after the server hung up" }
  if (-not (Test-WindowAlive $win)) { throw 'the window closed when the server hung up on it' }
  $after = Read-Report 2
  $remote = @($after.remote_pane_id | Sort-Object)
  if (($remote -join ',') -ne ($serverIds -join ',')) { Add-Failure "after the reconnect the GUI shows the server's panes $($remote -join ','), not $($serverIds -join ',')" }
  else { "reconnected: the window stayed and shows the server's panes $($remote -join ', ')" }
  $clients = @(Invoke-Cli list-clients --format json -Json -Server)
  if (-not ($clients | Where-Object { $_.pid -eq $win.Pid })) { Add-Failure "the server's list-clients has no client with the GUI's pid $($win.Pid) after the reconnect ($($clients.pid -join ', '))" }
  else { 'list-clients has the window again' }
  # A tab the server makes now reaches the window over the new connection
  New-Tab -From $serverIds[0] -Server | Out-Null
  $third = Read-Report 3
  if ($third.Count -ne 3) { Add-Failure "a tab made after the reconnect didn't reach the window: it shows $($third.Count) panes" }
  else { 'a tab made after the reconnect reached the window' }
  Wait-Settled -Window $win -What 'the reconnected window' | Out-Null
  Get-Shot $win -Name 'mux-reconnected' | ForEach-Object Dispose

  # 3. A dead server: the window gives up and closes, as it always did. About a minute: eight
  # tries, a second apart and doubling to 10 s
  Stop-TestServers
  if (-not (Wait-LogLine $win 'giving up' -TimeoutSec 75)) { Add-Failure "the GUI's log has no 'giving up' 75 s after its server was killed" }
  if (-not (Wait-Until { -not (Test-WindowAlive $win) } -TimeoutSec 10 -IntervalMs 200)) { Add-Failure 'the window was still open 10 s after it gave up on a dead server' }
  else { 'dead server: the window gave up and closed' }
} -Passed 'mux-reconnect: a window keeps its tabs when its server hangs up on it, and closes once no server answers'
