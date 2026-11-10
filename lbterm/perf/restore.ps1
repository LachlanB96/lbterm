#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/restore.rs, wezterm-gui/src/overlay/restore.rs, lbterm-extras/src/restore.rs, wezterm-gui/src/main.rs, wezterm-gui/src/frontend.rs, wezterm-client/src/domain.rs, mux/src/connui.rs
<#
Crash restore end to end (lbterm/RESTORE.md), in test windows of their own class, so the real
note (session-com.lachlanb.lbterm.json) is never touched:

1. A test window gets two more tabs: one in C:\Windows\System32, which is parked (F7, bound to
   LbtermPark 'Toggle'; Lachlan's 8a) and noted parked, and one that sets the claude_session
   user var as claudehook does. LBTerm writes its note a couple of seconds later. With -Mux, no
   note written while the window's domain attaches may list its connection status as a tab.
2. The window is killed, as a crash would end it (with -Mux its test mux server too, as a
   restart onto a new build ends both), with a handle to its process held open until step 5,
   as a process monitor's would be. The note must say not clean, three tabs.
3. A new test window opens with LBTERM_RESTORE_CLAUDE naming a stub claude.cmd that echoes its
   arguments, so no real Claude Code starts (pwsh's profile would find the real one first on
   PATH). It must show the prompt. With -Mux, whose new server answers 3 s late as Lachlan's
   did, the prompt must come once the window's domain has attached, over the server's first
   tab, and still be there 3 s on: until 2026-10-30 it came up over the connection status the
   window shows until then, a tab of its own that the attach takes away, and went with it,
   unanswered, with nothing opened. The window writes its own note at once all the same, with
   the three tabs in it as pending (until 2026-10-30 it wrote none until the answer). Then, still
   with -Mux, the server hangs up on the window, which reconnects: the prompt stays, asked once.
   A second tab, and the prompt's own tab ended: the prompt is asked again over the one left,
   once, and the three are still pending.
4. The prompt put aside, never answered, so the three stay pending:
   - the launcher (F8) opened over it stays up 1.5 s on, with the prompt not asked over it (it
     took the launcher down at once); Esc on the launcher brings the prompt back, asked once more.
   - a workspace switch (F9) moves the window to another mux window, which takes every overlay
     down (the frontend's SwitchToMuxWindow): asked again there once, over the new workspace's
     tab, and once more back on the first (F6). The other workspace's tab is then ended.
   - the window closed with the prompt up (WM_CLOSE, as its X button does; with -Mux that ends
     its tabs and its server): its note still has the three pending.
5. A third window opens as the second did, and asks, once, about the three at least. Then it's
   given a tab in a folder of its own and killed with the prompt up, as a crash would end it
   (with -Mux its server too): its note, not clean, has that tab and the three pending, where
   until 2026-10-30 an unanswered window had written none. The window after it asks once, about
   all four (and any other tab of the killed window's own).
   Enter is posted to it: every offered tab comes back, the Claude one running
   `claude --resume <id>` in its folder (captured: restore-prompt.png before, restore-done.png
   after), the System32 tab parked again (the published <socket>.parked.json of the process
   holding it names its pane), and its note has nothing pending.
6. With -Mux, only the GUI is killed now, its server and the tabs running on. A fourth window
   attaches to them: it must offer nothing (they're all still open), and its note has nothing
   pending.

  pwsh -File lbterm\perf\restore.ps1         # tabs in the GUI (the local domain)
  pwsh -File lbterm\perf\restore.ps1 -Mux    # tabs in a test lbterm-mux-server, as LBTerm runs
  pwsh -File lbterm\perf\restore.ps1 -Mux -Log 'info,wezterm_client=debug,mux=debug'  # with the attach in the GUI's log
#>
param([string]$Bin, [switch]$Mux, [string]$Log)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('restore' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$stub = Join-Path $T.Out 'stub'
New-Item -ItemType Directory -Force $stub | Out-Null
$note = Join-Path $env:APPDATA "lbterm\session-$($T.Class).json"
Remove-Item $note -ErrorAction SilentlyContinue
$sid = '0b6d7c2e-6a51-4c1e-9d3a-2f1e8b7a9c40'
'@echo STUB claude %*' | Set-Content (Join-Path $stub 'claude.cmd')
$hangup = Join-Path $T.Out 'hangup'

# The test's own config, with no config of Lachlan's: crash restore on (what this tests), F8 for
# the launcher and F9 and F6 for two workspaces, to put the prompt aside. With -Mux, a domain of
# the test's own, which the server reads too (serve_command). Its server answers slowly, as
# Lachlan's daily one did on 2026-10-30 (attached about 3 s after it was started): mux-startup,
# which only a server fires, holds the server's main thread for 3 s, so the GUI connects at once
# but its list of panes waits. The window's first frame comes well before its domain has
# attached, while the window holds only the connection status (not a timeout: the GUI gives a
# server it starts about 2.5 s to listen, and this one listens at once)
$cfg = Join-Path $T.Out 'config\restore.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$slowServer = if ($Mux) { @'
wezterm.on('mux-startup', function()
  local t0 = os.clock()
  while os.clock() - t0 < 3 do end
end)
'@ } else { '' }
$domain = if ($Mux) { @"
  unix_domains = { {
    name = 'perfrestore',
    socket_path = '$(ConvertTo-LuaPath $T.Sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@ } else { '' }
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
$slowServer
return {
  automatically_reload_config = false,
  wsl_domains = {},
  front_end = 'WebGpu',
  initial_cols = 90,
  -- WM_CLOSE closes at once, so the prompt's window closes with it up
  window_close_confirmation = 'NeverPrompt',
  -- What this tests: a killed test window greets the next with the prompt
  lbterm_crash_restore = true,
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  keys = {
    { key = 'F8', action = wezterm.action.ShowLauncher },
    { key = 'F9', action = wezterm.action.SwitchToWorkspace { name = 'other' } },
    { key = 'F6', action = wezterm.action.SwitchToWorkspace { name = 'default' } },
    -- Parks the open tab, as Ctrl+Alt+D does in Lachlan's config: one of the noted tabs is parked
    { key = 'F7', action = wezterm.action.LbtermPark 'Toggle' },
  },
$domain}
"@)

# With -Mux, on the domain, the server's first tab in place of -Program
function Open-Window([hashtable]$environment = @{}) {
  if ($Mux) {
    Open-TestWindow -ConfigFile $cfg -Domain perfrestore -Socket $T.Sock -Environment $environment -Log $Log -TimeoutSec 15
  } else {
    Open-TestWindow -ConfigFile $cfg -Cwd 'C:\Windows' -Program (New-PwshProgram) -Environment $environment -Log $Log
  }
}

# A pane's folder as cli list has it (a file:// URL), as a path
function ConvertFrom-CwdUrl([string]$Url) {
  try { ([uri]$Url).LocalPath.TrimEnd('\') } catch { $Url }
}

# The note as it is now, or nothing
function Read-Note { try { Get-Content $note -Raw -ErrorAction Stop | ConvertFrom-Json } catch { $null } }

# The panes where the tabs live: the window's own, or with -Mux its server's
function Get-TabPanes { @(Get-Panes -Server:$Mux) }

# A window's entry in its state file, as it is now
function Get-WindowState($Window) { Get-StateWindow (Read-State $Window) -Hwnd ([long]$Window.Handle) }

# Each time a window put the prompt up, from its log: "asking about 3 tabs over tab 2 of window 0"
function Get-Asks($Window) { @(Get-LogLines $Window 'restore: asking about') }

# The mux window an ask went over
function Get-AskWindow([string]$Line) { if ($Line -match 'of window (\d+)') { [int]$Matches[1] } else { $null } }

# Nothing answered the prompt, so the note still offers the killed LBTerm's three tabs, the
# Claude one with its session: as `pending` in the new window's own note, which it writes from
# the start (until 2026-10-30 it wrote none until the answer, so a prompt left up, or orphaned
# as Lachlan's was at 11:09 that day, left the whole run unnoted)
function Test-NotePending([string]$When, [int]$ByPid) {
  $n = Read-Note
  $pending = @($n.pending | Where-Object { $_ })
  if (-not $n -or $n.pid -ne $ByPid) {
    Add-Failure "$When the note wasn't the window's own (pid $($n.pid), the window's $ByPid)"
  }
  if ($pending.Count -ne 3 -or -not ($pending | Where-Object { $_.claude_session -eq $sid })) {
    Add-Failure "$When the note had $($pending.Count) tabs pending, not the three killed ones with the Claude session ($(($pending | ForEach-Object { "'$($_.title)'" }) -join ', '))"
  }
}

# The window has written its own note, with the three tabs pending, 2 s after its tabs changed
function Wait-NotePending($Window) {
  Wait-Until { $n = Read-Note; $n -and $n.pid -eq $Window.Pid -and @($n.pending | Where-Object { $_ }).Count -eq 3 } -TimeoutSec 10 -IntervalMs 100 | Out-Null
}

# How many tabs an ask offered: "asking about 3 tabs over tab 2 of window 0"
function Get-AskCount([string]$Line) { if ($Line -match 'asking about (\d+) tab') { [int]$Matches[1] } else { $null } }

# The window's asks once it has settled: exactly $Want, none more
function Test-AskedTimes($Window, [int]$Want, [string]$When) {
  Start-Sleep -Milliseconds 1000
  $asks = @(Get-Asks $Window)
  if ($asks.Count -ne $Want) { Add-Failure "$When the window asked $($asks.Count) times in all, not $Want`: $($asks -join ' | ')" }
  $asks
}

Invoke-Test {
  $first = Open-Window
  if ($Mux) {
    # While its domain attaches the window holds only the connection status, a tab of the GUI's
    # own, which must never be noted as a tab to open again: every note written until the
    # server's tab shows (the slow server leaves seconds for one)
    $seen = [Collections.Generic.HashSet[string]]::new()
    $attached = Wait-Until {
      foreach ($t in @((Read-Note).windows.tabs)) { if ($t) { [void]$seen.Add("'$($t.title)' in $($t.cwd)") } }
      $w = try { Get-WindowState $first } catch { $null }
      $w -and @($w.panes | Where-Object { $_.title -and $_.title -notmatch 'Connecting' }).Count -gt 0
    } -TimeoutSec 20 -IntervalMs 100
    if (-not $attached) { throw "the window showed no tab of the test mux server's within 20 s" }
    $junk = @($seen | Where-Object { $_ -match 'Connecting' })
    "while attaching, the note listed: $(if ($seen.Count) { $seen -join ', ' } else { 'no tabs' })"
    if ($junk) { Add-Failure "a note listed the connection status as a tab to open again: $($junk -join ', ')" }
    if (-not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $($T.Sock) didn't answer" }
  }
  # Painted: its pane is there to spawn beside (was 1 s)
  Wait-State { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -What 'the window' | Out-Null
  $pane0 = (Get-TabPanes)[0].pane_id
  New-Tab -From $pane0 -Cwd 'C:\Windows\System32' -Program (New-PwshProgram) -Server:$Mux | Out-Null
  # Parked (Lachlan's 8a), so the note says so and the tab comes back parked in step 5. Opened by
  # hand: a tab the server's cli spawns isn't the window's open one
  Invoke-Cli activate-tab --pane-id $pane0 --tab-index 1 -Server:$Mux | Out-Null
  Wait-Change { param($x) $x.active_tab -eq 1 } -Window $first -What 'the System32 tab open' | Out-Null
  Send-Key $first 'F7'
  # A Claude tab, as claudehook marks one: the claude_session user var. In this tree's folder,
  # which no other tab is in. -Command, not New-PwshProgram's -EncodedCommand: procinfo reads
  # nothing of a process whose command line passes 520 characters, its folder included, and the
  # note would have none to restore the tab in
  $setVar = "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('$sid')) + [char]7); Start-Sleep 60"
  New-Tab -From $pane0 -Cwd $T.Repo -Program @('C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-Command', $setVar) -Server:$Mux | Out-Null
  # LBTerm has written the three tabs down, the Claude one with its session, 2 s after they
  # changed (was a fixed 4 s); the checks below say what's missing if it never does
  Wait-Until { $n = Read-Note; $n -and @($n.windows.tabs).Count -eq 3 -and @($n.windows.tabs | Where-Object { $_.claude_session -eq $sid }).Count -and @($n.windows.tabs | Where-Object parked).Count } -TimeoutSec 10 -IntervalMs 100 | Out-Null
  $parkedNoted = @((Read-Note).windows.tabs | Where-Object parked)
  "noted parked: $(@($parkedNoted | ForEach-Object cwd) -join ', ')"
  if ($parkedNoted.Count -ne 1 -or $parkedNoted[0].cwd -notmatch 'System32\\?$') { Add-Failure "the note doesn't say the System32 tab, and only it, is parked: $(@($parkedNoted | ForEach-Object cwd) -join ', ')" }
  # Held open across the next starts, as a process monitor's handle would be: an ended process
  # still opens by its pid, with its times, while anything holds a handle to it, and until
  # 2026-10-30 the next LBTerm took that for the LBTerm that wrote the note still running,
  # offered nothing and wrote its own note over it (a run of this test without the hold showed
  # just that, most likely with the handle the harness's kill opens not yet let go)
  $holder = Get-Process -Id $first.Pid
  [void]$holder.Handle
  # As a crash would end it
  Stop-TestWindow $first
  if ($Mux) {
    # The tabs outlive the GUI in its server, until the server is ended too: a restart onto a
    # new build, the way Lachlan's ended on 2026-10-30
    $held = @(Get-Panes -Server).Count
    "after the GUI's kill: its server holds $held panes"
    if ($held -ne 3) { Add-Failure "the server holds $held panes after the GUI died, not 3" }
    Stop-TestServers
    if (-not (Wait-Until { -not (Get-TestServers) } -TimeoutSec 5)) { throw 'the test mux server was still running 5 s after it was killed' }
  }

  if (-not (Test-Path $note)) { throw "no note at $note" }
  $saved = Get-Content $note -Raw | ConvertFrom-Json
  $tabs = @($saved.windows | ForEach-Object { $_.tabs })
  "note: clean=$($saved.clean), $($tabs.Count) tabs, Claude in $(@($tabs | Where-Object claude_session).Count)"
  if ($saved.clean) { Add-Failure 'the note says clean after a kill' }
  if ($tabs.Count -ne 3) { Add-Failure "the note has $($tabs.Count) tabs, not 3" }
  if (-not ($tabs | Where-Object { $_.claude_session -eq $sid })) { Add-Failure 'the Claude tab lost its session id' }

  $environment = @{ LBTERM_RESTORE_CLAUDE = Join-Path $stub 'claude.cmd' }
  # With -Mux the server it starts can hang up on it (mux-reconnect.ps1 has the same)
  $second = Open-Window ($environment + $(if ($Mux) { @{ LBTERM_TEST_HANGUP = $hangup } } else { @{} }))
  # The prompt is an overlay, there before the window's first frame (frame 0 names it): painted
  # once a frame has been (was 3 s). With -Mux, over the one tab the new server started with
  Wait-Change { param($x) $x.frame -gt 0 -and $x.overlay -and @($x.tabs).Count -eq 1 } -Window $second -Animating -What 'the restore prompt painted' -TimeoutSec 20 | Out-Null
  $promptUp = $true
  Wait-NotePending $second
  Test-NotePending 'once the prompt was up, unanswered,' $second.Pid
  if ($Mux) {
    # On the window's mux window after the attach: its tab is the server's, and the prompt stays
    # once the attach is past (the cancelled one went within a second of showing)
    $up = Get-Date
    # (A server still busy starting answers this only once it's done)
    $serverPanes = @(Get-Panes -Server)
    "the prompt is up $('{0:N1}' -f ($up - $second.Opened).TotalSeconds) s after the window showed, at $($up.ToString('HH:mm:ss.fff')); the server answers with $($serverPanes.Count) pane at $(Get-Date -Format 'HH:mm:ss.fff')"
    if ($serverPanes.Count -ne 1) { Add-Failure "the new server has $($serverPanes.Count) panes under the prompt, not 1" }
    if (Wait-Until { -not (Get-WindowState $second).overlay } -TimeoutSec 3 -IntervalMs 50) {
      "the prompt went at $(Get-Date -Format 'HH:mm:ss.fff'), unanswered"
      Add-Failure 'the prompt went by itself within 3 s of showing, with nothing answered'
      $promptUp = $false
    } else {
      # Never twice: the server hangs up on the window while the prompt is up, and the window
      # reconnects and lists the panes again. The one prompt stays, asked once
      [IO.File]::WriteAllText($hangup, 'now')
      if (-not (Wait-Until { -not (Test-Path $hangup) } -TimeoutSec 5 -IntervalMs 100)) { throw "the test server didn't take the hang-up file within 5 s" }
      if (-not (Wait-LogLine $second 'Reconnected!' -TimeoutSec 15)) { Add-Failure "the window's log has no 'Reconnected!' 15 s after its server hung up" }
      $gone = Wait-Until { -not (Get-WindowState $second).overlay } -TimeoutSec 2 -IntervalMs 50
      $asked = @(Get-Asks $second)
      "reconnected with the prompt up: the prompt $(if ($gone) { 'gone' } else { 'still up' }), asked $($asked.Count) time$(if ($asked.Count -ne 1) { 's' })"
      if ($gone) { Add-Failure 'the prompt went when the server hung up and the window reconnected'; $promptUp = $false }
      if ($asked.Count -ne 1) { Add-Failure "the window asked $($asked.Count) times: $($asked -join ' | ')" }

      # A prompt whose tab goes is no answer: it's asked again, over the tab that's left, once. A
      # second tab beside the prompt's, then the prompt's own tab ended in the server
      $under = (Get-Panes -Server)[0].pane_id
      New-Tab -From $under -NoActivate -Server | Out-Null
      Wait-State { param($w) @($w.tabs).Count -eq 2 } -Window $second -What 'a second tab beside the prompt' | Out-Null
      Invoke-Cli kill-pane --pane-id $under -Server | Out-Null
      $again = Wait-Until { @(Get-Asks $second).Count -ge 2 } -TimeoutSec 10 -IntervalMs 100
      $x = Wait-Change { param($w) $w.overlay -and @($w.tabs).Count -eq 1 } -Window $second -Animating -What 'the prompt over the tab that was left' -TimeoutSec 10
      "the prompt's tab ended: $(if ($again) { 'asked again' } else { 'not asked again' }) over the tab that's left ($(@($x.tabs).Count) tab)"
      if (-not $again) { Add-Failure "the prompt wasn't asked again when its tab went: $((Get-LogLines $second 'restore:') -join ' | ')" }
      Test-AskedTimes $second 2 'once its tab had gone and it had settled,' | Out-Null
      Test-NotePending 'when the prompt''s tab went,' $second.Pid
    }
  }

  if ($promptUp) {
    # The launcher opened over the prompt takes the prompt down: no answer, but the prompt mustn't
    # go back over the launcher, which took it down at once (and the X button's, Ctrl+Shift+W's
    # and the quit prompt's confirmations alike). It waits for the launcher to close
    $asks = @(Get-Asks $second).Count
    Send-Key $second 'F8'
    $launcher = try { Wait-Change { param($w) $w.overlay -eq 'Launcher' } -Window $second -Animating -What 'the launcher over the prompt' -TimeoutSec 10 } catch { $null }
    Start-Sleep -Milliseconds 1500
    $now = (Get-WindowState $second).overlay
    $asked = @(Get-Asks $second).Count
    "the launcher over the prompt: $(if ($launcher) { 'it came up' } else { 'it never showed' }), and 1.5 s on the overlay is '$now', asked $($asked - $asks) more"
    if (-not $launcher) { Add-Failure "the launcher (F8) never showed over the prompt; the window said '$now'" }
    elseif ($now -ne 'Launcher') { Add-Failure "the launcher opened over the prompt was gone 1.5 s on ('$now' over the tab): the prompt was asked again over it" }
    if ($asked -ne $asks) { Add-Failure "the prompt was asked $($asked - $asks) times more while the launcher was up: $((Get-Asks $second) -join ' | ')" }
    Test-NotePending 'when the launcher put the prompt aside,' $second.Pid
    # Esc closes the launcher, and the prompt is back over the tab, asked once more
    Send-Key $second 'Escape'
    $back = Wait-Until { @(Get-Asks $second).Count -ge $asks + 1 } -TimeoutSec 10 -IntervalMs 100
    $x = try { Wait-Change { param($w) $w.overlay -and $w.overlay -ne 'Launcher' } -Window $second -Animating -What 'the prompt back once the launcher closed' -TimeoutSec 10 } catch { $null }
    "the launcher closed: the prompt $(if ($back -and $x) { 'back' } else { 'not back' })"
    if (-not ($back -and $x)) { Add-Failure "the prompt didn't come back when the launcher closed: $((Get-LogLines $second 'restore:') -join ' | ')" }
    $asks = @(Test-AskedTimes $second ($asks + 1) 'once the launcher had closed,')

    # A workspace switch puts the window on another mux window: the frontend's SwitchToMuxWindow,
    # which takes every overlay down. Asked again there, once, over the new workspace's one tab,
    # and once more back on the first
    $homeWindow = Get-AskWindow $asks[-1]
    Send-Key $second 'F9'
    $there = Wait-Until { @(Get-Asks $second).Count -ge $asks.Count + 1 } -TimeoutSec 15 -IntervalMs 100
    $x = try { Wait-Change { param($w) $w.overlay -and @($w.tabs).Count -eq 1 } -Window $second -Animating -What 'the prompt in the other workspace' -TimeoutSec 10 } catch { $null }
    $inOther = @(Test-AskedTimes $second ($asks.Count + 1) 'in the other workspace,')
    $otherWindow = Get-AskWindow $inOther[-1]
    "switched to the other workspace: $(if ($there) { "asked again over mux window $otherWindow (the first's is $homeWindow)" } else { 'not asked again' })"
    if (-not ($there -and $x)) { Add-Failure "the prompt wasn't asked again in the other workspace: $((Get-LogLines $second 'restore:') -join ' | ')" }
    elseif ($otherWindow -eq $homeWindow) { Add-Failure "the prompt was asked again over mux window $homeWindow, the one the window left for the other workspace" }
    Test-NotePending 'when a workspace switch put the prompt aside,' $second.Pid
    Send-Key $second 'F6'
    $back = Wait-Until { @(Get-Asks $second).Count -ge $inOther.Count + 1 } -TimeoutSec 15 -IntervalMs 100
    $x = try { Wait-Change { param($w) $w.overlay -and @($w.tabs).Count -eq 1 } -Window $second -Animating -What 'the prompt back in the first workspace' -TimeoutSec 10 } catch { $null }
    $asks = @(Test-AskedTimes $second ($inOther.Count + 1) 'back in the first workspace,')
    "switched back: $(if ($back) { "asked again over mux window $(Get-AskWindow $asks[-1])" } else { 'not asked again' })"
    if (-not ($back -and $x)) { Add-Failure "the prompt wasn't asked again back in the first workspace: $((Get-LogLines $second 'restore:') -join ' | ')" }
    elseif ((Get-AskWindow $asks[-1]) -ne $homeWindow) { Add-Failure "back in the first workspace the prompt was asked over mux window $(Get-AskWindow $asks[-1]), not $homeWindow" }
    # The other workspace's tab, which no window shows now, ended: the prompt stays as it is
    foreach ($p in @(Get-TabPanes | Where-Object { $_.workspace -eq 'other' })) { Invoke-Cli kill-pane --pane-id $p.pane_id -Server:$Mux | Out-Null }
    if (-not (Wait-Until { -not @(Get-TabPanes | Where-Object { $_.workspace -eq 'other' }) } -TimeoutSec 5)) { Add-Failure "the other workspace's tab was still there 5 s after it was killed" }
    Test-AskedTimes $second $asks.Count 'once the other workspace''s tab had ended,' | Out-Null
  }

  # The prompt's window closed with it up, as its X button does: nothing answered, so the note
  # still has the killed LBTerm's tabs pending and the next window asks again. With -Mux, closing
  # the window ends its tabs, and its server goes with the last
  Send-Close $second
  if (-not (Wait-Until { -not (Test-WindowAlive $second) } -TimeoutSec 10)) { throw "the window was still running 10 s after WM_CLOSE" }
  $kept = Read-Note
  "closed with the prompt up: the note is pid $($kept.pid)'s (the window's $($second.Pid)), clean=$($kept.clean), $(@($kept.pending | Where-Object { $_ }).Count) tabs pending"
  Test-NotePending 'when the prompt''s window closed,' $second.Pid
  if ($Mux -and -not (Wait-Until { -not (Get-TestServers) } -TimeoutSec 10)) {
    Add-Failure 'the test mux server was still running 10 s after its window closed'
    Stop-TestServers
  }

  $third = Open-Window $environment
  Wait-Change { param($x) $x.frame -gt 0 -and $x.overlay -and @($x.tabs).Count -eq 1 } -Window $third -Animating -What 'the restore prompt painted again' -TimeoutSec 20 | Out-Null
  $asks = @(Test-AskedTimes $third 1 'opened after the prompt''s window closed,')
  "the next window: the prompt up, asked $($asks.Count) time$(if ($asks.Count -ne 1) { 's' }) about $(Get-AskCount $asks[-1]) tabs"
  if ((Get-AskCount $asks[-1]) -lt 3) { Add-Failure "the next window offered $(Get-AskCount $asks[-1]) tabs, not the three killed ones at least" }

  # Killed with the prompt up, as a crash would end it (with -Mux its server too): until
  # 2026-10-30 a window whose prompt was never answered wrote no note, so every tab opened in it
  # was lost. Now its own note, not clean, has a tab opened while the prompt was up (in a folder
  # no other tab is in) and the three still pending, and the next window offers all four
  $own = Join-Path $T.Out 'own-tab'
  New-Item -ItemType Directory -Force $own | Out-Null
  New-Tab -From (Get-TabPanes)[0].pane_id -Cwd $own -Program (New-PwshProgram) -NoActivate -Server:$Mux | Out-Null
  $noted = Wait-Until { $n = Read-Note; $n -and $n.pid -eq $third.Pid -and @($n.windows.tabs | Where-Object { $_.cwd -and $_.cwd.TrimEnd('\') -eq $own }).Count -and @($n.pending | Where-Object { $_ }).Count -eq 3 } -TimeoutSec 10 -IntervalMs 100
  if (-not $noted) { Add-Failure "a tab opened with the prompt up wasn't noted in the window's own note within 10 s: $((Read-Note | ConvertTo-Json -Depth 5 -Compress))" }
  Stop-TestWindow $third
  if ($Mux) {
    Stop-TestServers
    if (-not (Wait-Until { -not (Get-TestServers) } -TimeoutSec 5)) { throw 'the test mux server was still running 5 s after it was killed' }
  }
  $crashed = Read-Note
  "killed with the prompt up: the note is pid $($crashed.pid)'s (the window's $($third.Pid)), clean=$($crashed.clean), $(@($crashed.pending | Where-Object { $_ }).Count) tabs pending"
  Test-NotePending 'when the window was killed with the prompt up,' $third.Pid
  if ($crashed.clean) { Add-Failure 'the note says clean after the window with the prompt up was killed' }
  $third = Open-Window $environment
  Wait-Change { param($x) $x.frame -gt 0 -and $x.overlay -and @($x.tabs).Count -eq 1 } -Window $third -Animating -What 'the restore prompt after a crash with it up' -TimeoutSec 20 | Out-Null
  $asks = @(Test-AskedTimes $third 1 'opened after a crash with the prompt up,')
  $offered = Get-AskCount $asks[-1]
  "after a crash with the prompt up: asked $($asks.Count) time$(if ($asks.Count -ne 1) { 's' }) about $offered tabs"
  if ($offered -lt 4) { Add-Failure "after a crash with the prompt up the window offered $offered tabs, not the three still pending and the one opened with the prompt up" }
  # The window's own tab and every one offered
  $want = 1 + $offered
  $ownBack = { @(Get-TabPanes | Where-Object { $_.cwd -and (ConvertFrom-CwdUrl $_.cwd) -eq $own })[0] }
  $holder.Dispose()
  Get-Shot $third -Name 'restore-prompt' | ForEach-Object Dispose
  # Enter opens the picked tabs, all of them to start with
  Send-Key $third 'Enter'
  # The prompt gone and the tabs open (was 3 s); the count below says so if they never are
  try { Wait-Change { param($x) -not $x.overlay -and @($x.tabs).Count -eq $want } -Window $third -Animating -What 'the offered tabs restored' -TimeoutSec 10 | Out-Null } catch { }
  # The Claude tab's folder, from a walk of its processes, and what its stub printed (pwsh loads
  # its profile first)
  $inRepo = { @(Get-TabPanes | Where-Object { $_.cwd -and (ConvertFrom-CwdUrl $_.cwd) -eq $T.Repo })[0] }
  Wait-Until { & $inRepo } -TimeoutSec 10 | Out-Null
  $panes = Get-TabPanes
  "after Enter: $($panes.Count) tabs"
  if ($panes.Count -ne $want) { Add-Failure "$($panes.Count) tabs after restoring, not $want (the new one and the $offered offered)" }
  $claudePane = & $inRepo
  if ($claudePane) {
    $read = { (Invoke-Cli get-text --pane-id $claudePane.pane_id -Server:$Mux) -join "`n" }
    Wait-Until { (& $read) -match "STUB claude --resume $sid" } -TimeoutSec 10 -IntervalMs 250 | Out-Null
    $text = & $read
    if ($text -notmatch "STUB claude --resume $sid") { Add-Failure "the Claude tab ran: $(($text | Out-String).Trim())" }
  } else { Add-Failure 'no restored tab in the Claude tab''s folder' }
  if (-not (Wait-Until { & $ownBack } -TimeoutSec 10)) { Add-Failure "the tab opened with the prompt up didn't come back in $own" }
  # The parked tab came back parked: the process holding it names its pane in its published list
  # (the server's beside its socket with -Mux, else the GUI's own beside its)
  $published = if ($Mux) { "$($T.Sock).parked.json" } else { "$($third.Socket).parked.json" }
  $sys32 = { @(Get-TabPanes | Where-Object { $_.cwd -and (ConvertFrom-CwdUrl $_.cwd) -match 'System32\\?$' })[0] }
  $parkedBack = Wait-Until { $p = & $sys32; $f = if (Test-Path -LiteralPath $published) { Get-Content -LiteralPath $published -Raw | ConvertFrom-Json }; $p -and $f -and @($f.panes) -contains [int]$p.pane_id } -TimeoutSec 10 -IntervalMs 100
  "the System32 tab back: $(if ($parkedBack) { 'parked' } else { 'not parked' }) ($published)"
  if (-not $parkedBack) { Add-Failure "the System32 tab, parked when noted, didn't come back parked ($published)" }
  if (-not $Mux) { Remove-Item -LiteralPath $published, "$published.tmp" -ErrorAction SilentlyContinue }
  Invoke-Cli activate-tab --pane-id $panes[0].pane_id --tab-index 0 -Server:$Mux | Out-Null
  # The first tab open again (was 1 s)
  Wait-Change { param($x) $x.active_tab -eq 0 } -Window $third -Animating -What 'the first tab open' | Out-Null
  Get-Shot $third -Name 'restore-done' | ForEach-Object Dispose
  # Answered, so its note has nothing pending from 2 s after
  if (-not (Wait-Until { $n = Read-Note; $n.pid -eq $third.Pid -and -not @($n.pending | Where-Object { $_ }).Count } -TimeoutSec 5 -IntervalMs 100)) { Add-Failure "the answered window's note still has tabs pending, or isn't its own (pid $((Read-Note).pid), the window's $($third.Pid); $(@((Read-Note).pending | Where-Object { $_ }).Count) pending)" }

  if ($Mux) {
    # Only the GUI dies: the four tabs run on in the server, so there's nothing to offer
    Wait-Until { $n = Read-Note; $n -and $n.pid -eq $third.Pid -and @($n.windows.tabs).Count -eq $want } -TimeoutSec 10 -IntervalMs 100 | Out-Null
    Stop-TestWindow $third
    $held = @(Get-Panes -Server).Count
    if ($held -ne $want) { Add-Failure "the server holds $held panes after only the GUI died, not $want" }
    $fourth = Open-Window
    Wait-State { param($x) $x.frame -gt 0 -and @($x.tabs).Count -eq $want } -Window $fourth -What "the $want tabs reattached" -TimeoutSec 15 | Out-Null
    # Past the 2 s crash restore gives reattached panes' user vars, and then some
    if (Wait-Until { (Get-WindowState $fourth).overlay } -TimeoutSec 4 -IntervalMs 50) { Add-Failure "a prompt came up with every noted tab still open: $((Get-WindowState $fourth).overlay)" }
    # Answered by itself: its own note, with nothing pending
    if (-not (Wait-Until { $n = Read-Note; $n.pid -eq $fourth.Pid -and -not @($n.pending | Where-Object { $_ }).Count } -TimeoutSec 5 -IntervalMs 100)) { Add-Failure "the reattached window's note isn't its own with nothing pending (pid $((Read-Note).pid), the window's $($fourth.Pid))" }
    $shown = @((Get-WindowState $fourth).tabs).Count
    "only the GUI killed: $shown tabs reattached, no prompt, the note the new window's"
  }
} -Passed "restore$(if ($Mux) { ' (mux)' }): the note, the prompt and the restored tabs all as they should be"
