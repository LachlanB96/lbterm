#Requires -Version 7
# Variant: -Mux
# Covers: mux/src/lbterm_procs.rs, mux/src/lbterm_proclist.rs, mux/src/localpane.rs, procinfo/src/windows.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-gui/src/termwindow/titles.rs, wezterm-gui/src/termwindow/render/tab_panel.rs
<#
A pane's title and folder follow its processes, and typing walks none of them (lbterm/PERF.md,
finding 49; mux/src/lbterm_procs.rs). A pane whose program sets no title is titled by its
foreground process (pwsh.exe, then a program it runs), and one whose shell doesn't say its
folder (OSC 7) has its foreground process's. Both came from walking every process on the PC,
which each key's echo did once, as output; now the pane's processes are a job whose messages say
when one starts or ends, and only then is it walked.

In a test window like Lachlan's (New-TestConfig, the rail and the strip), its tab a pwsh without
a profile at a short prompt (no title of its own, no OSC 7). With -Mux its tab lives in a test
lbterm-mux-server, on a domain and socket of its own (target\perf\proc-title-mux). The walks and
reads are counted from the lines lbterm::procs says at debug (WEZTERM_LOG), in the GUI's log, or
the server's on a mux domain.

1. The tab titled pwsh.exe, its rail row and the strip pwsh.
2. -Keys keys typed at the prompt, each waited for its echo: no walk.
3. A program that prints nothing (ping to $null, -PingSec long): its name in the pane's title,
   the rail row and the strip within -ShowSec of its line's Enter, with nothing else typed, and
   pwsh's again within -ShowSec once it ends. Walked for its start and its end.
4. cmd.exe started at the prompt, then `cd` there: the pane's folder, as the GUI has it (its own,
   or on a mux domain the server's push), becomes the new one within -ShowSec, read alone,
   without a walk.
5. -Programs programs run from cmd, which names itself (its title its own) and doesn't say its
   folder: the pane's folder is cmd's, which other programs starting and ending don't change, so
   their job's messages (each heard) walk nothing, where each had the whole PC walked before (the
   review of perf/key-latency, 2026-10-31: a Claude Code session the Conductor starts is such a
   pane until claudehook's first OSC 7, and starts its statusline and hooks every few seconds).
   `exit`, and pwsh's title is back: cmd, the foreground, ended, which its message says, and the
   list is walked again.

A build without lbterm::procs's lines (one from before this) says nothing of its walks, so the
counts are checked only where the build says any: the titles and folders are checked either way.

  pwsh -File lbterm\perf\proc-title.ps1 [-Mux] [-Keys 10] [-PingSec 4] [-ShowSec 2] [-Programs 5]
#>
param([string]$Bin, [switch]$Mux, [int]$Keys = 10, [int]$PingSec = 4, [double]$ShowSec = 2, [int]$Programs = 5)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('proc-title' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$cfg = Join-Path $T.Out 'config\proc-title.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The server reads the same file (serve_command), so both know the test domain; its first tab
# runs default_prog, so the test's pwsh is that
$domain = if ($Mux) {
  @"
config.unix_domains = { {
  name = 'perfproctitle',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 100
config.initial_rows = 24
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.window_close_confirmation = 'NeverPrompt'
config.skip_close_confirmation_for_processes_named = { 'pwsh.exe', 'cmd.exe', 'PING.EXE', 'conhost.exe' }
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-NoLogo', '-NoExit', '-Command', [[function prompt { 'PS> ' }]] }
config.default_cwd = 'C:/Users/Public'
$domain
"@

function Get-Pane($sw) { @($sw.panes | Where-Object active)[0] }
function Get-Row($sw) { @($sw.rail.rows | Where-Object active)[0] }
function Get-DrawnCursor($p) { if ($null -ne $p.drawn_cursor) { $p.drawn_cursor } else { $p.cursor } }
# What the window shows the tab as: the pane's title, its rail row's and the strip's
function Get-Shown($sw) {
  $p = Get-Pane $sw; $r = Get-Row $sw
  [pscustomobject]@{ Pane = [string]$p.title; Row = [string]$r.title; Strip = [string]$sw.strip.title }
}
# The pane's title its program's name, or the path ConPTY gives it once a program that set a title
# of its own (cmd) has ended: the console's first title, its program's path, which is the
# terminal's own title from then on
function Test-Shows($sw, [string]$Name) {
  $s = Get-Shown $sw
  $s.Pane -match "(?i)(^|[\\/])$Name(\.exe)?$" -and $s.Row -match "(?i)^$Name" -and $s.Strip -match "(?i)^$Name"
}
# When the frame the last Wait-State took was painted (the snapshot's time_ms), less $From
function Get-ShownMs([double]$From) { [Math]::Round([double](Get-Test).LastState.time_ms - $From, 1) }

# lbterm::procs's lines (the walks, the cwd reads, the job's messages) since Set-Mark
function Set-Mark { $script:mark = if (Test-Path -LiteralPath $script:logPath) { (Get-Item -LiteralPath $script:logPath).Length } else { 0 } }
function Get-ProcLines { @(Read-LogLines $script:logPath $script:mark | Where-Object { $_ -match 'lbterm::procs\s+> pane \d+: ' } | ForEach-Object { ($_ -split 'lbterm::procs\s+> ', 2)[1] }) }
function Get-Walks($lines) { @($lines | Where-Object { $_ -match ': walk' }) }
function Get-Reads($lines) { @($lines | Where-Object { $_ -match ': cwd read' }) }
function Get-Moves($lines) { @($lines | Where-Object { $_ -match 'its processes moved' }) }
$counts = [ordered]@{}

# A line typed into the pane through the cli (its own input, as a paste would be)
function Send-Line([string]$Text) {
  Invoke-Cli send-text --pane-id $script:pane --no-paste "$Text`r" -Server:$Mux -Check | Out-Null
}

Invoke-Test {
  $log = 'info,lbterm::procs=debug,lbterm::title=debug'
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfproctitle -Socket $T.Sock -Log $log -TimeoutSec 30 }
  else { Open-TestWindow -ConfigFile $cfg -Log $log -TimeoutSec 30 }
  if ($Mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 20 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  $script:pane = (Get-Panes -Server:$Mux)[0].pane_id
  $script:logPath = if ($Mux) {
    $srv = @(Get-TestServers)[0]
    if (-not $srv) { throw 'no test mux server running' }
    Join-Path $env:USERPROFILE ".local\share\lbterm\lbterm-mux-server.exe-log-$($srv.ProcessId).txt"
  } else { $w.Log }
  Send-Focus $w

  # 1. pwsh at its prompt, titled by its process
  $sw = Wait-Change { param($s) (Get-Pane $s) -and (Get-DrawnCursor (Get-Pane $s)).x -eq 4 -and (Test-Shows $s 'pwsh') } -What 'pwsh.exe at its short prompt, in the title, the rail and the strip' -TimeoutSec 30
  Write-Host "1. at the prompt: $(Get-Shown $sw | ConvertTo-Json -Compress)"
  Start-Sleep -Milliseconds 500

  # 2. Keys typed at the prompt walk nothing
  Set-Mark
  $x0 = (Get-DrawnCursor (Get-Pane $sw)).x
  for ($i = 1; $i -le $Keys; $i++) {
    $frame = Get-StateFrame
    $want = $x0 + $i
    Send-Key $w X
    Wait-State { param($s) (Get-DrawnCursor (Get-Pane $s)).x -eq $want } -AfterFrame $frame -What "key $i's echo" -TimeoutSec 5 | Out-Null
    Start-Sleep -Milliseconds 120
  }
  # What they set off, done
  Start-Sleep -Milliseconds 700
  $lines = Get-ProcLines
  $counts.keys = [ordered]@{ keys = $Keys; walks = (Get-Walks $lines).Count; cwd_reads = (Get-Reads $lines).Count; lines = $lines.Count }
  Write-Host "2. $Keys keys typed: $((Get-Walks $lines).Count) walks, $((Get-Reads $lines).Count) cwd reads"
  if ((Get-Walks $lines).Count) { Add-Failure "typing $Keys keys at the prompt walked the pane's processes $((Get-Walks $lines).Count) times: $((Get-Walks $lines) -join '; ')" }
  Send-Key $w Escape
  Wait-State { param($s) (Get-DrawnCursor (Get-Pane $s)).x -eq 4 } -What 'the line cleared' -TimeoutSec 5 | Out-Null
  Start-Sleep -Milliseconds 500

  # 3. A program that prints nothing: in the title as it starts, pwsh again as it ends
  Set-Mark
  $frame = Get-StateFrame
  $sent = Get-UnixMs
  Send-Line "ping -n $($PingSec + 1) 127.0.0.1 > `$null"
  try {
    $sw = Wait-State { param($s) Test-Shows $s 'ping' } -AfterFrame $frame -What 'PING.EXE in the title, the rail and the strip' -TimeoutSec $ShowSec
    $shownMs = Get-ShownMs $sent
    Write-Host "3. ping shown $shownMs ms after its line: $(Get-Shown $sw | ConvertTo-Json -Compress)"
    $counts.program_shown_ms = $shownMs
  } catch {
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    Add-Failure "a program that prints nothing wasn't in the title within $ShowSec s of its line: the window shows $(Get-Shown $sw | ConvertTo-Json -Compress)"
  }
  $frame = Get-StateFrame
  try {
    $sw = Wait-State { param($s) Test-Shows $s 'pwsh' } -AfterFrame $frame -What 'pwsh.exe again' -TimeoutSec ($PingSec + 2 + $ShowSec)
    $counts.program_ended_ms = Get-ShownMs $sent
    Write-Host "3. pwsh again $($counts.program_ended_ms) ms after the line (ping runs about $PingSec s)"
  } catch {
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    Add-Failure "pwsh wasn't in the title again once ping ended: the window shows $(Get-Shown $sw | ConvertTo-Json -Compress)"
  }
  Start-Sleep -Milliseconds 500
  $lines = Get-ProcLines
  $counts.program = [ordered]@{ walks = (Get-Walks $lines).Count; cwd_reads = (Get-Reads $lines).Count; moved = @($lines | Where-Object { $_ -match 'moved' }).Count }
  Write-Host "3. ping's run: $(($counts.program | ConvertTo-Json -Compress))"
  if ($lines.Count -and (Get-Walks $lines).Count -lt 2) { Add-Failure "ping's start and end walked $((Get-Walks $lines).Count) times, not one each: $($lines -join '; ')" }

  # 4. cmd's cd: the folder read alone, in the GUI's view
  # Quoted for pwsh, which would take $G for a variable
  Send-Line 'cmd.exe /d /k ''prompt $G'''
  Wait-State { param($s) (Get-DrawnCursor (Get-Pane $s)).x -eq 1 } -What "cmd's prompt" -TimeoutSec 10 | Out-Null
  Start-Sleep -Milliseconds 700
  $cwd0 = [string]@(Get-Panes | Where-Object { $_.is_active })[0].cwd
  Set-Mark
  $sent = Get-UnixMs
  Send-Line 'cd C:\Windows'
  $got = $null
  $ok = Wait-Until {
    $script:got = [string]@(Get-Panes | Where-Object { $_.is_active })[0].cwd
    $script:got -match '(?i)/C:/Windows/?$'
  } -TimeoutSec $ShowSec -IntervalMs 50
  $cwdMs = [Math]::Round((Get-UnixMs) - $sent, 1)
  Start-Sleep -Milliseconds 300
  $lines = Get-ProcLines
  $counts.cwd = [ordered]@{ before = $cwd0; after = $script:got; shown_ms = $cwdMs; walks = (Get-Walks $lines).Count; cwd_reads = (Get-Reads $lines).Count }
  Write-Host "4. cmd's cd: $(($counts.cwd | ConvertTo-Json -Compress))"
  if (-not $ok) { Add-Failure "the pane's folder wasn't C:\Windows within $ShowSec s of cmd's cd: the GUI says '$($script:got)' (before it: '$cwd0')" }
  if ($lines.Count -and (Get-Walks $lines).Count) { Add-Failure "cmd's cd walked the pane's processes: $($lines -join '; ')" }
  if ($lines.Count -and -not (Get-Reads $lines).Count) { Add-Failure "cmd's cd was never read: $($lines -join '; ')" }

  # 5. Programs run from cmd, which names itself and doesn't say its folder: heard, not walked
  Start-Sleep -Milliseconds 500
  $y0 = (Get-DrawnCursor (Get-Pane (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)))).y
  Set-Mark
  Send-Line "for /l %i in (1,1,$Programs) do @ping -n 1 127.0.0.1 >nul"
  # cmd's prompt again, a line further on
  Wait-State { param($s) $c = Get-DrawnCursor (Get-Pane $s); $c.x -eq 1 -and $c.y -gt $y0 } -What "cmd's prompt after the programs" -TimeoutSec 15 | Out-Null
  # Their last end heard, and the thread done (it paces itself 300 ms)
  Start-Sleep -Milliseconds 1200
  $lines = Get-ProcLines
  $counts.programs = [ordered]@{ programs = $Programs; moved = (Get-Moves $lines).Count; walks = (Get-Walks $lines).Count; cwd_reads = (Get-Reads $lines).Count }
  Write-Host "5. $Programs programs from cmd: $(($counts.programs | ConvertTo-Json -Compress))"
  if ($lines.Count) {
    if ((Get-Walks $lines).Count) { Add-Failure "$Programs programs run from cmd, a pane that names itself, walked its processes $((Get-Walks $lines).Count) times: $($lines -join '; ')" }
    if ((Get-Moves $lines).Count -lt 2 * $Programs) { Add-Failure "$Programs programs run from cmd were heard $((Get-Moves $lines).Count) times, not their starts and ends: $($lines -join '; ')" }
  }

  $frame = Get-StateFrame
  Send-Line 'exit'
  try {
    Wait-State { param($s) Test-Shows $s 'pwsh' } -AfterFrame $frame -What 'pwsh.exe again after cmd' -TimeoutSec ($ShowSec + 2) | Out-Null
  } catch {
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    Add-Failure "pwsh wasn't in the title again after cmd's exit: the window shows $(Get-Shown $sw | ConvertTo-Json -Compress)"
  }
  if (-not (Read-LogLines $script:logPath 0 | Where-Object { $_ -match 'lbterm::procs' })) {
    (Get-Test).Notes.Add('this build says nothing on lbterm::procs (one from before it): the walks and reads were not counted')
  }
  $counts | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'counts.json')
} -Passed 'a pane titled by its processes shows a silent program as it starts and pwsh as it ends, its folder after cmd''s cd, programs run from cmd walk nothing, and typing walked nothing'
