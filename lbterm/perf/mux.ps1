#Requires -Version 7
# Covers: wezterm-mux-server-impl/src/sessionhandler.rs, codec/src/lib.rs, wezterm-client/src/domain.rs, wezterm-client/src/pane/clientpane.rs, mux/src/lib.rs, wezterm-gui/src/frontend.rs, wezterm-gui/src/restore.rs, wezterm-mux-server/src/main.rs
<#
The mux domain end to end (lbterm/MUX.md), in test windows of their own class, on a domain and
socket of their own (target\perf\mux), so the real lbterm-mux-server and its tabs are never
touched:

1. A test GUI connects to the test domain, which starts a test lbterm-mux-server. A second tab
   reports what Claude Code would: the claude_session and claude_status user vars, an OSC 7501
   "blocked on a question" and OSC 9;4 progress. The GUI must see all of it (its Lua writes what
   it sees to report.json every half second).
2. The GUI is killed, as a crash would end it. The server must still hold both tabs.
3. A second GUI connects. It must show both tabs with what they had, though nothing has been
   sent again since (captured: mux-reattached.png), and offer no crash restore.
4. It's closed as a window is (WM_CLOSE). The tabs must end with it, and the server after them.
5. A third GUI, quit with QuitApplication (F12 here): the same.

  pwsh -File lbterm\perf\mux.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux' -Bin $Bin
$note = Join-Path $env:APPDATA "lbterm\session-$($T.Class).json"
$sock = Join-Path $T.Out 'sock'
$cfg = Join-Path $T.Out 'config\mux.lua'
$report = Join-Path $T.Out 'report.json'
$sid = '5f0c2d7e-1b3a-4e6f-9a8b-7c6d5e4f3a21'
Remove-Item $note -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The server reads the same file (serve_command), so both know the test domain
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
        domain = p:get_domain_name(),
        user_vars = p:get_user_vars(),
        program_status = p:get_program_status(),
        progress = p:get_progress(),
      }
    end
  end
  local f = io.open(report, 'w')
  if f then f:write(encode(panes)); f:close() end
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  initial_cols = 90,
  status_update_interval = 500,
  window_close_confirmation = 'NeverPrompt',
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  keys = { { key = 'F12', mods = 'NONE', action = wezterm.action.QuitApplication } },
  unix_domains = { {
    name = 'perfmux',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
}
"@)

# What Claude Code would say in its tab, then nothing more
$claudeish = Join-Path $T.Out 'claudeish.ps1'
$b64 = { param($s) [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($s)) }
[IO.File]::WriteAllText($claudeish, @"
`$e = [char]27; `$bel = [char]7
[Console]::Write("`$e]1337;SetUserVar=claude_session=$(& $b64 $sid)`$bel")
[Console]::Write("`$e]1337;SetUserVar=claude_status=$(& $b64 'orange|1790000000000')`$bel")
[Console]::Write("`$e]7501;state=blocked:kind=question:app=claude:title=$(& $b64 'Which branch?')`$e\")
[Console]::Write("`$e]9;4;1;40`$bel")
'claude-ish: said its piece'
Start-Sleep 600
"@)

function Open-Window {
  Remove-Item $report -ErrorAction SilentlyContinue
  Open-TestWindow -ConfigFile $cfg -Domain perfmux -Socket $sock -TimeoutSec 15
}

# What the GUI's Lua last saw, once it has seen $count panes
function Read-Report([int]$count, [int]$seconds = 8) {
  $deadline = (Get-Date).AddSeconds($seconds)
  $panes = @()
  while ((Get-Date) -lt $deadline) {
    try { $panes = @(Get-Content $report -Raw -ErrorAction Stop | ConvertFrom-Json) } catch { $panes = @() }
    if ($panes.Count -ge $count -and ($panes | Where-Object { $_.user_vars.claude_session })) { break }
    Start-Sleep -Milliseconds 250
  }
  $panes
}

# Once the server a new GUI started answers with at least $count panes
function Wait-ServerPanes([int]$count, [int]$seconds = 15) {
  $deadline = (Get-Date).AddSeconds($seconds)
  do {
    $panes = @(Get-Panes -Server)
    if ($panes.Count -ge $count) { return $panes }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  throw "the test mux server didn't answer with $count panes within $seconds s"
}

function Wait-Gone([scriptblock]$alive, [int]$seconds = 8) {
  Wait-Until { -not (& $alive) } -TimeoutSec $seconds -IntervalMs 200
}

function Test-Claudeish($panes, [string]$when) {
  $c = $panes | Where-Object { $_.user_vars.claude_session } | Select-Object -First 1
  if (-not $c) { Add-Failure "${when}: no pane with claude_session"; return }
  if ($c.user_vars.claude_session -ne $sid) { Add-Failure "${when}: claude_session is '$($c.user_vars.claude_session)'" }
  if ($c.user_vars.claude_status -ne 'orange|1790000000000') { Add-Failure "${when}: claude_status is '$($c.user_vars.claude_status)'" }
  $ps = @($c.program_status)[0]
  if ($ps.state -ne 'blocked' -or $ps.kind -ne 'question' -or $ps.title -ne 'Which branch?') {
    Add-Failure "${when}: OSC 7501 record is $($c.program_status | ConvertTo-Json -Compress)"
  }
  if ($c.progress.Percentage -ne 40) { Add-Failure "${when}: progress is $($c.progress | ConvertTo-Json -Compress)" }
  if ($null -eq $c.remote_pane_id) { Add-Failure "${when}: no remote_pane_id in the pane's metadata" }
  "${when}: pane $($c.pane_id) (server's $($c.remote_pane_id)) in $($c.domain): $($c.user_vars.claude_session), $($ps.state)/$($ps.kind), progress $($c.progress.Percentage)"
}

Invoke-Test {
  # 1. Connect, which starts the server; a Claude-ish second tab
  $first = Open-Window
  $panes = Wait-ServerPanes 1
  $server = Get-TestServers
  if ($server.Count -ne 1) { throw "$($server.Count) test mux servers running after the first GUI connected" }
  New-Tab -From $panes[0].pane_id -Server -Program (New-PwshProgram -File $claudeish) | Out-Null
  Test-Claudeish (Read-Report 2) 'live'

  # 2. As a crash would end it
  Stop-TestWindow $first
  Start-Sleep 1
  $panes = @(Get-Panes -Server)
  "after the kill: the server holds $($panes.Count) panes ($($panes.pane_id -join ', '))"
  if ($panes.Count -ne 2) { Add-Failure "the server holds $($panes.Count) panes after the GUI died, not 2" }
  $serverIds = @($panes.pane_id)

  # 3. Attach again: nothing has been sent since, so all of it comes from the attach
  $second = Open-Window
  $seen = Read-Report 2
  Test-Claudeish $seen 'reattached'
  if ($seen.Count -ne 2) { Add-Failure "the reattached GUI shows $($seen.Count) panes, not 2" }
  $remote = @($seen.remote_pane_id | Sort-Object)
  if (($remote -join ',') -ne (($serverIds | Sort-Object) -join ',')) { Add-Failure "remote_pane_id $($remote -join ',') isn't the server's $($serverIds -join ',')" }
  Start-Sleep 3  # past crash restore's 2 s wait for the attach, so a prompt would show by now
  # Painted and still
  Wait-Settled -Window $second -What 'the reattached window' | Out-Null
  Get-Shot $second -Name 'mux-reattached' | ForEach-Object Dispose

  # 4. Closing the window ends its tabs, and the server goes once it has none
  Send-Close $second
  if (-not (Wait-Gone { Test-WindowAlive $second })) { Add-Failure 'the GUI was still running 8 s after its window closed' }
  if (-not (Wait-Gone { Get-TestServers })) { Add-Failure 'the server was still running 8 s after the last window closed' }
  else { 'after WM_CLOSE: GUI and server gone' }
  if ((Get-Content $note -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json).clean -ne $true) { Add-Failure 'the crash-restore note is not clean after closing' }

  # 5. A new server for a new GUI, and QuitApplication ends it the same way
  $third = Open-Window
  $panes = Wait-ServerPanes 1
  New-Tab -From $panes[0].pane_id -Server | Out-Null
  $panes = Wait-ServerPanes 2 5
  "third GUI: its new server holds $($panes.Count) panes"
  Send-Key $third 'F12'
  if (-not (Wait-Gone { Test-WindowAlive $third })) { Add-Failure 'the GUI was still running 8 s after QuitApplication' }
  if (-not (Wait-Gone { Get-TestServers })) { Add-Failure 'the server was still running 8 s after QuitApplication' }
  else { 'after QuitApplication: GUI and server gone' }
} -Passed 'mux: tabs outlive a dead GUI with their state, and end with a closed or quit one'
