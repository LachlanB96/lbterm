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
6. Claude Code starting in a shell tab just before a crash: a GUI and a second plain tab, noted as
   two shell tabs; then the second reports a Claude session and the GUI is killed at once, inside
   the note's 2 s (SAVE_AFTER), so the note still has it as a shell tab. The next GUI must offer
   nothing: the pane with a session no noted tab has stands for the shell tab it started in.

  pwsh -File lbterm\perf\mux.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux' -Bin $Bin
$note = Join-Path $env:APPDATA "lbterm\session-$($T.Class).json"
$sock = $T.Sock
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
  -- update-status comes again on its interval only after a status is set (PITFALLS.md, trap 48)
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

# Claude Code arriving in a shell tab: its session and nothing more
$sid2 = '2a7c9e1f-3b5d-4f60-8c1e-9d2b4a6f8e03'
$sessionOnly = Join-Path $T.Out 'session-only.ps1'
[IO.File]::WriteAllText($sessionOnly, @"
`$e = [char]27; `$bel = [char]7
[Console]::Write("`$e]1337;SetUserVar=claude_session=$(& $b64 $sid2)`$bel")
'session-only: said its session'
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
  # The crash-restore note has both tabs, the Claude-ish one with its session, 2 s after they
  # changed: killed sooner, the note still had it as a shell tab, which the session's pane can't
  # stand for, and the next GUI offered it
  $noted = Wait-Until {
    $n = try { Get-Content $note -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $null }
    $n -and @($n.windows.tabs).Count -eq 2 -and @($n.windows.tabs | Where-Object { $_.claude_session -eq $sid }).Count
  } -TimeoutSec 10 -IntervalMs 100
  if (-not $noted) { Add-Failure "the crash-restore note didn't list both tabs, the Claude-ish one with its session, within 10 s" }

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
  # Painted and still, with no prompt: both tabs are still open
  $settled = Wait-Settled -Window $second -What 'the reattached window'
  if ($settled.overlay) { Add-Failure "the reattached window put up '$($settled.overlay)' with both noted tabs still open (a crash-restore prompt)" }
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

  # 6. Claude Code starts in a noted shell tab and the GUI dies before the note says so
  Remove-Item $note -ErrorAction SilentlyContinue
  $fourth = Open-Window
  $panes = Wait-ServerPanes 1
  # In a folder of its own: a note offers identical tabs once (restore::to_offer), so two shell tabs
  # in default_cwd would be one, which the first tab's pane stands for whatever the second becomes
  $shellTab = New-Tab -From $panes[0].pane_id -Server -Cwd $T.Out
  $two = Wait-Until {
    $n = try { Get-Content $note -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $null }
    $n -and @($n.windows.tabs).Count -eq 2 -and -not @($n.windows.tabs | Where-Object { $_.claude_session }).Count
  } -TimeoutSec 10 -IntervalMs 100
  if (-not $two) { throw 'the crash-restore note never listed the two shell tabs' }
  Invoke-Cli send-text --pane-id $shellTab --no-paste "& '$sessionOnly'`r" -Server | Out-Null
  $seen = Wait-Until {
    $r = try { @(Get-Content $report -Raw -ErrorAction Stop | ConvertFrom-Json) } catch { @() }
    @($r | Where-Object { $_.user_vars.claude_session -eq $sid2 }).Count
  } -TimeoutSec 8 -IntervalMs 50
  if (-not $seen) { throw "the GUI never saw the shell tab's session" }
  Stop-TestWindow $fourth
  # The check only means something if the note still had it as a shell tab
  $left = Get-Content $note -Raw | ConvertFrom-Json
  if (@($left.windows.tabs).Count -ne 2 -or @($left.windows.tabs | Where-Object { $_.claude_session }).Count) {
    Add-Failure "the note had $(@($left.windows.tabs).Count) tabs, $(@($left.windows.tabs | Where-Object { $_.claude_session }).Count) with a session, when the GUI died, not two shell tabs: a kill inside SAVE_AFTER was not tested"
  } else { 'killed inside SAVE_AFTER: the note still had the Claude tab as a shell tab' }
  $fifth = Open-Window
  Start-Sleep 3  # past crash restore's 2 s wait for the attach
  $settled = Wait-Settled -Window $fifth -What 'the GUI after the kill'
  if ($settled.overlay) { Add-Failure "the next GUI put up '$($settled.overlay)' though both noted tabs were still open: the session that started in the shell tab didn't stand for it" }
  else { 'reattached: nothing offered' }
  Send-Close $fifth
  if (-not (Wait-Gone { Test-WindowAlive $fifth })) { Add-Failure 'the fifth GUI was still running 8 s after its window closed' }
  if (-not (Wait-Gone { Get-TestServers })) { Add-Failure 'the server was still running 8 s after the fifth GUI closed' }
} -Passed 'mux: tabs outlive a dead GUI with their state, and end with a closed or quit one'
