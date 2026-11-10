#Requires -Version 7
# Covers: wezterm-client/src/client.rs, wezterm-client/src/lbterm_start.rs, mux/src/lbterm_serve.rs, wezterm-mux-server/src/main.rs, wezterm-mux-server-impl/src/local.rs, config/src/lbterm.rs
<#
How a GUI starts its mux domain's server (lbterm/MUX.md, "Starting the server"; lbterm/PERF.md,
finding 25), in test windows of their own class, on the test's own socket, so Lachlan's server and
socket are never touched. Upstream tried the socket ten times over 2.25 s before it started the
server, though nothing could be listening; a connect refused can't say whether a server is on its
way, so the server's own lock on its socket says it.

1. No socket file: the GUI starts the server at once (its log's "Will try spawning the server"
   within 1.5 s of its process starting, where upstream's tries alone took 2.25 s), its tab comes
   from the server, and the server has one tab, not a second the GUI opened over an empty list.
2. A socket file a killed server left behind: the same.
3. Two GUIs opened together, their server slow to listen (its own config holds it about a second
   first): one server started, by one of them (one "Running:" between their logs), the other
   saying it waited for that one, both with the server's tab.
4. A server running whose socket file was removed (it binds it again at its next look, every
   3 s): a GUI opened then waits for that server, starts none, and connects once it's back.
5. A server started by hand as a GUI starts its own, both slow to listen: one of them refuses
   ("another mux server is starting or running", naming the pid of the one that serves), and the
   GUI has the other's tab. A client can't see a server before it has loaded its config, so a
   second process can start here: it never serves.
6. A server holding the socket that never answers (held still, its socket file removed so it
   can't bind it again): a GUI opened then waits for it, starts none, and gives up after its 7 s
   naming that server's pid, still running, for Lachlan to end (the review of perf/key-latency,
   2026-10-31: a mutex can't say who holds it, so the server says its pid beside it).

  pwsh -File lbterm\perf\server-start.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'server-start' -Bin $Bin
$sock = $T.Sock
$domain = 'perfstart'
$dir = Join-Path $T.Out 'config'
New-Item -ItemType Directory -Force $dir | Out-Null
$idle = New-PwshProgram -Script 'Start-Sleep 600'

# The GUI's config, and the server's: the same domain, the server's maybe slow to load (a busy
# loop of $Slow seconds of its CPU before it returns), so it listens that much later
function New-Configs([string]$Name, [double]$Slow = 0) {
  $gui = Join-Path $dir "$Name-gui.lua"
  $server = Join-Path $dir "$Name-server.lua"
  $domains = @"
  unix_domains = { {
    name = '$domain',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $server)', '--daemonize' },
  } },
"@
  $body = @"
return {
  automatically_reload_config = false,
  wsl_domains = {},
  front_end = 'WebGpu',
  initial_cols = 90,
  window_close_confirmation = 'NeverPrompt',
  -- The windows are killed between the steps: no crash restore asking over the next one's tab
  lbterm_crash_restore = false,
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-Command', 'Start-Sleep 600' },
  default_cwd = 'C:/Windows',
$domains
}
"@
  $hold = if ($Slow) { "local t0 = os.clock()`nwhile os.clock() - t0 < $Slow do end`n" } else { '' }
  [IO.File]::WriteAllText($gui, "local wezterm = require 'wezterm'`n$body")
  [IO.File]::WriteAllText($server, "local wezterm = require 'wezterm'`n$hold$body")
  [pscustomobject]@{ Gui = $gui; Server = $server }
}

# The test's servers that serve (not a --daemonize copy on its way out)
function Get-Serving { @(Get-TestServers | Where-Object { $_.CommandLine -notmatch '--daemonize' }) }

function Stop-All {
  foreach ($w in @($T.Windows)) { if (Test-WindowAlive $w) { Stop-TestWindow $w } }
  Stop-TestServers
  if (-not (Wait-Until { -not (Get-TestServers) } -TimeoutSec 5)) { throw 'the test''s mux servers didn''t end' }
}

# A GUI process's own log lines
function Get-Lines($Window, [string]$Pattern) { @(Get-LogLines $Window $Pattern) }

# When a log line was written, on today's date (lines carry the time of day)
function Get-LineTime([string]$Line) {
  if ($Line -match '^(\d\d:\d\d:\d\d\.\d{3})') { [datetime]::ParseExact("$((Get-Date).ToString('yyyy-MM-dd')) $($Matches[1])", 'yyyy-MM-dd HH:mm:ss.fff', $null) }
}

# Waits for a window's state file to show a pane of the server's
function Wait-ServerPane($Window, [string]$What) {
  Wait-State -Window $Window { param($s) @($s.panes | Where-Object { $null -ne $_.server_id }).Count -ge 1 } -What $What -TimeoutSec 20 | Out-Null
}

# A test server held still and let go (step 6): a server that holds its socket and never answers
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class LbtermHeldServer {
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("ntdll.dll")] static extern int NtSuspendProcess(IntPtr h);
  [DllImport("ntdll.dll")] static extern int NtResumeProcess(IntPtr h);
  static bool On(int pid, bool hold) {
    IntPtr h = OpenProcess(0x0800, false, (uint)pid);
    if (h == IntPtr.Zero) return false;
    try { return (hold ? NtSuspendProcess(h) : NtResumeProcess(h)) == 0; } finally { CloseHandle(h); }
  }
  public static bool Hold(int pid) { return On(pid, true); }
  public static bool Let(int pid) { return On(pid, false); }
}
'@

# One GUI from nothing (or a dead server's file): it starts the server at once
function Test-StartsAtOnce([string]$When) {
  $c = New-Configs 'plain'
  $w = Open-TestWindow -ConfigFile $c.Gui -Domain $domain -Socket $sock -TimeoutSec 20
  Wait-ServerPane $w "${When}: the server's tab"
  # When it was let run, after the handoff to the test's job
  $started = $w.Ran
  $spawn = @(Get-Lines $w 'Will try spawning the server')
  if ($spawn.Count -ne 1) { Add-Failure "${When}: the GUI said $($spawn.Count) times that it starts the server, not once"; return }
  $after = ((Get-LineTime $spawn[0]) - $started).TotalMilliseconds
  $answered = @(Get-Lines $w 'lbterm::serve.*the server started answered after') | Select-Object -Last 1
  "${When}: the server started $([Math]::Round($after)) ms after the GUI's process; $(if ($answered) { ($answered -split '> ', 2)[1] } else { 'no word of its answer' })"
  if ($after -gt 1500) { Add-Failure "${When}: the server was started $([Math]::Round($after)) ms after the GUI's process, not at once" }
  if (@(Get-Lines $w 'lbterm::serve.*waiting for that one|waited .* for another client').Count) { Add-Failure "${When}: the GUI waited for a server, though none was there" }
  $serving = Get-Serving
  if ($serving.Count -ne 1) { Add-Failure "${When}: $($serving.Count) mux servers serve the socket" }
  $panes = @(Get-Panes -Server)
  if ($panes.Count -ne 1) { Add-Failure "${When}: the server has $($panes.Count) panes, not its one" }
}

Invoke-Test {
  # 1. No socket file
  Test-StartsAtOnce 'no socket file'

  # 2. The file a killed server left
  Stop-All
  if (-not (Get-SocketStamp $sock)) { throw 'the killed server left no socket file behind' }
  Test-StartsAtOnce 'a killed server''s file'
  Stop-All

  # 3. Two GUIs together, their server slow to listen
  $c = New-Configs 'slow' -Slow 0.8
  # Let run one straight after the other, not each once its window shows as Open-TestWindow does,
  # so the second asks for the server while the first is still starting it: both are made and
  # handed to the test's job held, since a handoff takes longer than the server's hold
  $like = @{ Class = $T.Class; Config = $c.Gui; Domain = $domain; ServerSocket = $sock }
  $position = & (Get-Module harness) { $TestWindowPosition }
  $two = foreach ($n in 'a', 'b') {
    $state = Join-Path $T.Out "state-$n.json"
    $id = Start-Unfocused -Exe $T.Gui -Arguments "--config-file $(ConvertTo-CommandLineArg $c.Gui) connect $domain --position $position --class $($T.Class)" -Environment @{ LBTERM_STATE_FILE = $state } -Job -Held
    Add-Content (Join-Path $T.Out 'pids.txt') $id
    # Where its lines start in its log: a GUI given an earlier one's pid appends to its log
    @{ Id = $id; LogStart = Get-LogStart $id; Like = [pscustomobject]($like + @{ StateFile = $state }) }
  }
  foreach ($g in $two) { $g.Ran = Resume-StartedProcess $g.Id }
  $windows = @($two | ForEach-Object { $w = Add-TestWindow -ProcessId $_.Id -Like $_.Like -Ran $_.Ran; $w.LogStart = $_.LogStart; $w })
  foreach ($w in $windows) { Wait-ServerPane $w 'two together: the server''s tab' }
  $running = @($windows | ForEach-Object { Get-Lines $_ 'Running: ' })
  $waited = @($windows | ForEach-Object { Get-Lines $_ 'lbterm::serve.*waited .* for another client' })
  "two together: $($running.Count) started the server; $(if ($waited) { ($waited[0] -split '> ', 2)[1] } else { 'none said it waited' })"
  if ($running.Count -ne 1) { Add-Failure "two together: $($running.Count) GUIs started the server, not one" }
  if ($waited.Count -ne 1) { Add-Failure "two together: $($waited.Count) GUIs said they waited for the other starting it, not one" }
  if ((Get-Serving).Count -ne 1) { Add-Failure "two together: $((Get-Serving).Count) mux servers serve the socket" }
  if (@(Get-Panes -Server).Count -ne 1) { Add-Failure "two together: the server has $(@(Get-Panes -Server).Count) panes, not its one" }
  Stop-All

  # 4. A live server whose file was removed
  $c = New-Configs 'plain'
  $one = Open-TestWindow -ConfigFile $c.Gui -Domain $domain -Socket $sock -TimeoutSec 20
  Wait-ServerPane $one 'removed: the first GUI''s tab'
  Remove-Item -LiteralPath $sock
  $removed = Get-Date
  $two = Open-TestWindow -ConfigFile $c.Gui -Domain $domain -Socket $sock -TimeoutSec 20
  Wait-ServerPane $two 'removed: the second GUI''s tab, once the server is back'
  $lines = @(Get-Lines $two 'lbterm::serve')
  $answered = $lines | Where-Object { $_ -match 'the server holding it answered after' } | Select-Object -Last 1
  "removed: $(if ($answered) { ($answered -split '> ', 2)[1] } else { 'no word of an answer' }), its tab $([Math]::Round(((Get-Date) - $removed).TotalMilliseconds)) ms after the file went"
  if (@(Get-Lines $two 'Running: ').Count) { Add-Failure 'removed: the second GUI started a server beside the live one' }
  if (-not ($lines | Where-Object { $_ -match 'refused while a server holds it' })) { Add-Failure 'removed: the second GUI never said it waited for the server holding the socket' }
  if ((Get-Serving).Count -ne 1) { Add-Failure "removed: $((Get-Serving).Count) mux servers serve the socket" }
  Stop-All

  # 5. A server started by hand as the GUI starts its own
  $c = New-Configs 'slow' -Slow 0.8
  $err = Join-Path $T.Out 'by-hand-err.txt'
  # The GUI made and handed to the test's job held first, so it runs straight after the hand's
  # server starts, as it did before the handoff (which takes longer than the server's hold)
  $state = Join-Path $T.Out 'state-by-hand.json'
  $gui = Start-Unfocused -Exe $T.Gui -Arguments "--config-file $(ConvertTo-CommandLineArg $c.Gui) connect $domain --position $position --class $($T.Class)" -Environment @{ LBTERM_STATE_FILE = $state } -Job -Held
  $guiLogStart = Get-LogStart $gui
  $since = Get-Date
  $byHand = Start-Process -FilePath $T.MuxServer -ArgumentList '--config-file', $c.Server -PassThru -WindowStyle Hidden -RedirectStandardError $err
  $ran = Resume-StartedProcess $gui
  $w = Add-TestWindow -ProcessId $gui -Like ([pscustomobject]@{ Class = $T.Class; Config = $c.Gui; Domain = $domain; ServerSocket = $sock; StateFile = $state }) -Ran $ran
  $w.LogStart = $guiLogStart
  Wait-ServerPane $w 'by hand: the GUI''s tab'
  # The one that refused has ended by now: the hand's, its stderr; the GUI's, its own log
  [void](Wait-Until { (Get-Serving).Count -le 1 } -TimeoutSec 5)
  $logs = @(Get-ChildItem (Join-Path $env:USERPROFILE '.local\share\lbterm') -Filter 'lbterm-mux-server.exe-log-*.txt' |
    Where-Object LastWriteTime -ge $since.AddSeconds(-1) |
    Where-Object { (Read-LogLines $_.FullName) -match [regex]::Escape(($sock -replace '\\', '/')) -or (Read-LogLines $_.FullName) -match [regex]::Escape($sock) })
  # Each server that refused, once (its log says it twice: spawn_listener's error, then main's)
  $refusals = @($logs | Where-Object { (Read-LogLines $_.FullName) -match 'another mux server is starting or running' } | ForEach-Object { "the GUI's ($($_.Name))" })
  if ($byHand.HasExited -and (Test-Path $err) -and (Get-Content $err -Raw) -match 'another mux server is starting or running') { $refusals += 'the hand''s' }
  "by hand: refused by $(if ($refusals) { $refusals -join ', ' } else { 'none' }); the hand's server $(if ($byHand.HasExited) { "ended ($($byHand.ExitCode))" } else { 'serves' })"
  if ((Get-Serving).Count -ne 1) { Add-Failure "by hand: $((Get-Serving).Count) mux servers serve the socket" }
  if ($refusals.Count -ne 1) { Add-Failure "by hand: $($refusals.Count) servers said they refused the socket another holds, not one" }
  # The refusal names the one that serves
  $serves = @(Get-Serving)[0]
  $named = @(@($logs | ForEach-Object { Read-LogLines $_.FullName }) + @(if (Test-Path $err) { Get-Content $err }) | Where-Object { $_ -match 'another mux server is starting or running' -and $_ -match 'pid (\d+)\)' } | ForEach-Object { [int]([regex]::Match($_, 'pid (\d+)\)').Groups[1].Value) } | Select-Object -Unique)
  "by hand: the refusal named pid $(if ($named) { $named -join ', ' } else { 'none' }); pid $($serves.ProcessId) serves"
  if ($serves -and $named -notcontains [int]$serves.ProcessId) { Add-Failure "by hand: the refusal named pid $($named -join ', '), not $($serves.ProcessId), the server that serves" }
  if (-not $byHand.HasExited) { $byHand.Kill() }
  Stop-All

  # 6. A server holding the socket that never answers: held still, its file removed
  $c = New-Configs 'plain'
  $one = Open-TestWindow -ConfigFile $c.Gui -Domain $domain -Socket $sock -TimeoutSec 20
  Wait-ServerPane $one 'never answers: the first GUI''s tab'
  $held = @(Get-Serving)[0]
  if (-not $held) { throw 'never answers: no test server serves the socket' }
  $heldPid = [int]$held.ProcessId
  if (-not [LbtermHeldServer]::Hold($heldPid)) { throw "never answers: server $heldPid couldn't be held still" }
  $late = $null
  try {
    Remove-Item -LiteralPath $sock
    $started = Start-Unfocused -Exe $T.Gui -Arguments "--config-file $(ConvertTo-CommandLineArg $c.Gui) connect $domain --position $position --class $($T.Class)" -Environment @{ LBTERM_STATE_FILE = (Join-Path $T.Out 'state-held.json') } -Job -PassThru
    $late = $started.Id
    # From when it was let run, after the handoff to the test's job
    $since = $started.Ran
    Add-Content (Join-Path $T.Out 'pids.txt') $late
    $lateLog = Get-GuiLogPath $late
    $lateStart = Get-LogStart $late
    $gaveUp = $null
    [void](Wait-Until { $script:gaveUp = @(Read-LogLines $lateLog $lateStart | Where-Object { $_ -match 'hasn''t answered on it' })[0]; $null -ne $script:gaveUp } -TimeoutSec 20 -IntervalMs 250)
    $after = [Math]::Round(((Get-Date) - $since).TotalSeconds, 1)
    "never answers: $(if ($script:gaveUp) { "gave up by $after s: $(($script:gaveUp -split '> ', 2)[-1])" } else { "no word in $after s" })"
    if (-not $script:gaveUp) { Add-Failure "never answers: the GUI said nothing of the server that holds the socket and never answered, in $after s" }
    elseif ($script:gaveUp -notmatch "pid $heldPid, still running") { Add-Failure "never answers: the GUI didn't name pid $heldPid, still running: $($script:gaveUp)" }
    if (@(Read-LogLines $lateLog $lateStart | Where-Object { $_ -match 'Running: ' }).Count) { Add-Failure 'never answers: the GUI started a server over the one holding the socket' }
    if (@(Get-TestServers | Where-Object { $_.ProcessId -ne $heldPid -and $_.CommandLine -notmatch '--daemonize' }).Count) { Add-Failure 'never answers: a second server serves beside the held one' }
  } finally {
    [void][LbtermHeldServer]::Let($heldPid)
    if ($late) { Stop-Process -Id $late -Force -ErrorAction SilentlyContinue }
  }
} -Passed 'server-start: a GUI starts its server at once when none holds the socket, two together start one, one held by a live server is waited for, never started over, and one that never answers is named by its pid'
