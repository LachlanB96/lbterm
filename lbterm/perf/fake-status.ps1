#Requires -Version 7
# Helper: the stand-in status-storm.ps1 runs in each of its tabs
<#
A tab's program that sends the OSC 7501 records a working Claude Code session and claudehook send,
while the line in its control file says so (read every 20 ms; the script writes it). What they
send, from Claude Code 2.1.312's bundle and Terminal's statusline.ts:

- Claude Code's own record, `state=working:app=claude-code`, with `msg` its task summary and
  `progress` its to-do list's, sent when any of them changes (it keeps what it last sent of each
  record and sends only one that differs); a record for each of its running tasks (subagents),
  `id=<task>` with `title` the task's description, working, or blocked while paused.
- claudehook's `state=working:id=bg/<id>` for each background task still running, sent again every
  minute while it runs (TASK_RESEND_MS), word for word.

Lines:

- `rest`: nothing.
- `hello`: its title and folder (OSC 0 and OSC 7) again. What it writes as it starts doesn't reach
  the terminal (ConPTY's first frame isn't out yet), and a pane whose title is still `wezterm`
  takes one from its foreground process, so the server walked every process on the PC each time
  it printed, 9 ms a report (measured 2026-10-30): Claude Code sets its title, and claudehook says
  the folder, all the time.
- `base`: `hello`, then its records once: its own (working, message A), a subagent `agent-a1`
  ("Explore the code"), and claudehook's `bg/t1` and `bg/t2`.
- `resend`: all four again every 250 ms, word for word: claudehook's minute resend, sped up.
- `parts`: every 250 ms its subagent turns between working and blocked (paused), which its row
  counts the same ("1 agent"): a change no row shows.
- `words`: every 2.5 s its own message turns between A and B: a change its row shows.
- `flip`: every 2.5 s its own state turns between working and blocked on a permission prompt.
- `state:<working|blocked|done>`: its own record in that state once, the rest as `base` left them.
- `agents:<n>`: n subagents running (`agent-a1` to `agent-a<n>`), the others cleared, once.
- `print:<text>`: <text> written once, `\e` in it as ESC and `\a` as BEL (config-save.ps1 has a
  program set its own colours with it).

Each time the line changes it writes how many reports it has sent so far to <control>.sent, so
the script knows how many came in a phase.

  pwsh -NoProfile -File fake-status.ps1 -Name <name> -Control <file> [-PhaseMs 0]
#>
param(
  [Parameter(Mandatory)][string]$Name,
  [Parameter(Mandatory)][string]$Control,
  [int]$PhaseMs = 0
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$bel = [char]7
$st = "$e\"
$b64 = { param($s) [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($s)) }
$msgA = & $b64 'Reading the files'
$msgB = & $b64 'Editing the code'
$task = & $b64 'Explore the code'
$own = { param($state, $msg) $k = if ($state -eq 'blocked') { ':kind=permission' } else { '' }; "$e]7501;state=$state$($k):app=claude-code:msg=$msg$st" }
$part = { param($id, $state) "$e]7501;state=$($state):app=claude-code:id=$($id):title=$task$st" }
$bg = { param($id) "$e]7501;state=working:id=bg/$id$st" }

# Its folder, as claudehook says it (OSC 7), and its title: without them the server walks every
# process on the PC for the pane's folder and title each time it has printed (mux/src/localpane.rs).
# Sent again by `hello`, since these go before ConPTY passes anything on
$hello = "$e]7;file://localhost/$((Get-Location).Path -replace '\\', '/')$bel$e]0;$Name$bel"
[Console]::Write($hello)
Write-Host "$Name, waiting on $Control"
$clock = [Diagnostics.Stopwatch]::StartNew()
$seen = $null
$mode = 'rest'
$sent = 0
$turn = 0
$next = 0
$ownState = 'working'
$ownMsg = $msgA
$partState = 'working'
function Send([string[]]$Records) { [Console]::Write(($Records -join '')); $script:sent += $Records.Count }
function Base { Send @((& $own $ownState $ownMsg), (& $part 'agent-a1' $partState), (& $bg 't1'), (& $bg 't2')) }
while ($true) {
  $line = $null
  try { $line = [IO.File]::ReadAllText($Control).Trim() } catch { }
  if ($null -ne $line -and $line -ne $seen) {
    $seen = $line
    try { [IO.File]::WriteAllText("$Control.sent", "$sent") } catch { }
    $now = $clock.ElapsedMilliseconds
    if ($line -eq 'hello') {
      [Console]::Write($hello)
      $mode = 'rest'
    } elseif ($line -eq 'base') {
      [Console]::Write($hello)
      $ownState = 'working'; $ownMsg = $msgA; $partState = 'working'
      Base
      $mode = 'rest'
    } elseif ($line -like 'state:*') {
      $ownState = $line.Substring(6)
      Send @(& $own $ownState $ownMsg)
      $mode = 'rest'
    } elseif ($line -like 'print:*') {
      [Console]::Write($line.Substring(6).Replace('\e', "$e").Replace('\a', "$bel"))
      $mode = 'rest'
    } elseif ($line -like 'agents:*') {
      $n = [int]$line.Substring(7)
      Send @(1..4 | ForEach-Object { if ($_ -le $n) { & $part "agent-a$_" 'working' } else { "$e]7501;state=clear:id=agent-a$_$st" } })
      $mode = 'rest'
    } else {
      $mode = $line
      $next = $now + $PhaseMs
    }
  }
  $now = $clock.ElapsedMilliseconds
  if ($now -ge $next) {
    switch ($mode) {
      'resend' { Base; $next += 250 }
      'parts' {
        $partState = if ($partState -eq 'working') { 'blocked' } else { 'working' }
        Send @(& $part 'agent-a1' $partState); $next += 250
      }
      'words' {
        $ownMsg = if ($ownMsg -eq $msgA) { $msgB } else { $msgA }
        Send @(& $own $ownState $ownMsg); $next += 2500
      }
      'flip' {
        $ownState = if ($ownState -eq 'working') { 'blocked' } else { 'working' }
        Send @(& $own $ownState $ownMsg); $next += 2500
      }
    }
  }
  Start-Sleep -Milliseconds 20
}
