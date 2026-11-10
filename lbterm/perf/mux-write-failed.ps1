#Requires -Version 7
# Covers: wezterm-client/src/lbterm_link.rs, wezterm-client/src/client.rs, wezterm-client/src/pane/clientpane.rs, mux/src/lbterm_writes.rs, lbterm-extras/src/scroll_note.rs, lbterm-extras/src/state.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, lbterm/perf/fake-claude.ps1
<#
While a mux pane's connection is down, the strip's own seek ends at once and says why, rather
than wait out its burst's time, and a write waits for the reconnect as a key does, saying nothing
(Lachlan's 15b of 2026-11-07; lbterm/MUX.md, "A write to a pane"; lbterm/PITFALLS.md, 178). In a test window on a mux domain and socket of its own
(target\perf\mux-write-failed), so the real lbterm-mux-server and its tabs are never touched.
Its one tab runs fake-claude.ps1 (a stand-in for Claude Code's fullscreen view, twelve messages,
as perf/scroll-strip.ps1 has it) on the test's server, with a transcript of the same twelve.

1. With the server up, a click on the third prompt's mark scrolls the conversation to message
   3: the strip's seek works on a mux pane, and the time it took is the baseline.
2. The test's server is killed. The window's client says in its log that its connection ended
   and keeps the tab while it tries to reconnect (about a minute; wezterm-client's
   after_disconnect).
3. A click on the eighth prompt's mark: the seek finds the connection down before its first
   burst of keys and sends none, which would wait out the reconnect and scroll the conversation
   after it. The strip's note says "Not sent" and why, the seek has ended (the state file's
   `seeking`), both well inside the 0.4 s the burst's wait starts at (scroll_strip.rs,
   `start_max`).
4. Once that note has gone, a key, F5 (bound to SendString, a write to the pane) and a key: the
   write waits for the reconnect with the keys round it, so no note comes and the log has no
   failed write (a write that failed at once while the keys waited would reach the program as a
   hole in what was typed). Before the failed-writes review (2026-11-08) it noted "Not sent".

What the strip shows is what the state file says (lbterm/STATE.md, `scroll_strip`'s `note`,
`note_head` and `seeking`). Mouse and keys are posted messages, never real input. A server that
refuses a write (no such pane) reaches the strip from the write's task, which no test window can
make happen on demand: wezterm-client's lbterm_link.rs tests what its answer says.

  pwsh -File lbterm\perf\mux-write-failed.ps1 [-Bin <dir>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-write-failed' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$sums = Join-Path $T.Out 'summaries'
$folder = Join-Path $projects 'C--work-demo'
$cfg = Join-Path $T.Out 'config\mux-write-failed.lua'
New-Item -ItemType Directory -Force $folder, $sums, (Split-Path $cfg) | Out-Null

# The twelve messages fake-claude.ps1 shows, as a transcript
$sid = '7c1d4e2a-5b3f-4a8e-9d60-2f4b8c1e7a35'
$start = [DateTime]::new(2026, 11, 7, 9, 0, 0, [DateTimeKind]::Utc)
$records = [Collections.Generic.List[string]]::new()
for ($m = 1; $m -le 12; $m++) {
  foreach ($r in @(
      @{ type = 'user'; origin = @{ kind = 'human' }; message = @{ role = 'user'; content = "message number $m, about part $m of the work" } },
      @{ type = 'assistant'; message = @{ role = 'assistant'; content = @(@{ type = 'text'; text = ((1..14 | ForEach-Object { "answer $m, line $_" }) -join "`n") }) } },
      @{ type = 'system'; subtype = 'turn_duration'; durationMs = 12000 })) {
    $r.timestamp = $start.AddSeconds(5 * $records.Count).ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    $r.isSidechain = $false
    $records.Add(($r | ConvertTo-Json -Compress -Depth 8))
  }
}
[IO.File]::WriteAllText((Join-Path $folder "$sid.jsonl"), ($records -join "`n") + "`n")

# Lachlan's config (New-TestConfig) on a domain of the test's own, the strip reading the test's
# transcripts, F5 bound to a SendString. The server reads the same file (serve_command)
New-TestConfig -Path $cfg -Lua @"
config.lbterm_summaries = '$(ConvertTo-LuaPath $sums)'
config.lbterm_scroll_strip = { transcripts = '$(ConvertTo-LuaPath $projects)' }
config.initial_cols = 110
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F5', mods = 'NONE', action = wezterm.action.SendString 'x' })
config.unix_domains = { {
  name = 'failmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

# The pane's text, until a line below the top one matches `$match` (the top is the header)
function Wait-Text([string]$match, [double]$seconds = 10) {
  Wait-Until { @(Invoke-Cli get-text --pane-id $pane | Select-Object -Skip 1) -match $match } -TimeoutSec $seconds -IntervalMs 250
}
function Marks($s) { @($s.scroll_strip.marks | Where-Object kind -eq 'prompt' | Sort-Object { $_.rect.y }) }
# A posted click on a prompt's mark, then the first state in which `$done` holds: how long after
# the click, and that state
function Measure-Click($at, [scriptblock]$done, [string]$what) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Send-Click $w $at[0] $at[1]
  $s = Wait-State $done -What $what -TimeoutSec 10
  [pscustomobject]@{ Ms = [Math]::Round($clock.Elapsed.TotalMilliseconds); State = $s }
}

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Domain 'failmux' -Socket $T.Sock -TimeoutSec 30
  Wait-State { param($s) $s.frame -gt 0 } -What 'the first frame' -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "the test server has no panes; the window's log: $($w.Log)" }
  if (@(Get-TestServers).Count -ne 1) { throw "$(@(Get-TestServers).Count) test mux servers for the window" }
  $fake = Join-Path $PSScriptRoot 'fake-claude.ps1'
  $shell = $panes[0].pane_id
  $remote = New-Tab -From $shell -Program (New-PwshProgram -File $fake -ArgumentList '-Sid', $sid, '-Messages', 12) -Server
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 1 } -What 'the one tab' -TimeoutSec 30 | Out-Null
  $pane = (Get-Panes)[0].pane_id

  # 1. The server up: a seek on a mux pane gets there
  $e = Wait-Change { param($s) $s.scroll_strip.mode -eq 'claude' -and $s.scroll_strip.thumb -and (Marks $s).Count -eq 12 } -What 'the conversation on the strip' -TimeoutSec 30
  $up = Measure-Click (Get-RectCenter (Marks $e)[2].rect) { param($s) -not $s.scroll_strip.seeking -and ($s.landed -or $s.scroll_strip.note) } 'the seek to message 3'
  if ($up.State.scroll_strip.note) { Add-Failure "with the server up the seek to message 3 ended with the note '$($up.State.scroll_strip.note_head): $($up.State.scroll_strip.note)'" }
  elseif (-not (Wait-Text '^\s*❯ message number 3,')) { Add-Failure 'with the server up a click on the third prompt''s mark never brought message 3 into view' }
  else { "server up: the seek to message 3 landed in $($up.Ms) ms" }
  Wait-Settled -What 'the jump to settle' | Out-Null
  Send-Leave $w
  Wait-State { param($s) -not $s.scroll_strip.card -and $s.settled } -What 'the card to close' | Out-Null

  # 2. The server gone: the window keeps its tab while it tries to reconnect
  Stop-TestServers
  # This build's client says so as the connection ends; one from before perf/mux-client's
  # failed-writes stage only once its first try to connect again has failed, a second on
  if (-not (Wait-LogLine $w 'connection to the mux server ended|problem reconnecting' -TimeoutSec 10)) { throw "the window's log never said its connection ended, 10 s after its server was killed" }
  if (-not (Test-WindowAlive $w)) { throw 'the window closed as its server was killed' }
  $marks = Marks (Wait-State { param($s) (Marks $s).Count -eq 12 } -What 'the strip after the kill')

  # 3. The seek sends nothing with the connection down: the strip says so and the seek has ended
  $down = Measure-Click (Get-RectCenter $marks[7].rect) { param($s) $s.scroll_strip.note -and -not $s.scroll_strip.seeking } 'a note on the strip and the seek ended'
  $strip = $down.State.scroll_strip
  "server gone: the click's note in $($down.Ms) ms, '$($strip.note_head): $($strip.note)'"
  if ($strip.note_head -ne 'Not sent') { Add-Failure "the seek into a pane whose server has gone noted '$($strip.note_head)', not 'Not sent': '$($strip.note)'" }
  if ($strip.note -notmatch "isn't answering") { Add-Failure "the note doesn't say the mux server isn't answering: '$($strip.note)'" }
  # A seek that waited out its burst's time for the screen to move would take 0.4 s at least
  if ($down.Ms -ge 400) { Add-Failure "the seek ended $($down.Ms) ms after the click, not at once: a burst's wait starts at 400 ms" }
  if (Get-LogLines $w "writing to remote pane $remote") { Add-Failure "the window's log has a failed write to pane ${remote}: the seek wrote with the connection down" }
  (Get-Shot $w -Name 'not-sent').Dispose()

  # 4. A key, a binding's SendString and a key, once the seek's note has gone: all three wait
  Wait-State { param($s) -not $s.scroll_strip.note } -What 'the seek''s note to go' -TimeoutSec 10 | Out-Null
  Send-Key $w A
  Send-Key $w F5
  Send-Key $w B
  $quiet = $true
  try {
    $k = Wait-State { param($s) $s.scroll_strip.note } -What 'a note for F5''s SendString' -TimeoutSec 3
    $quiet = $false
    Add-Failure "F5's SendString between two keys noted '$($k.scroll_strip.note_head): $($k.scroll_strip.note)' with the connection down: it should wait with the keys"
  } catch { }
  if (Get-LogLines $w "writing to remote pane $remote") { Add-Failure "the window's log has a failed write to pane ${remote}: F5's SendString didn't wait with the keys round it" }
  if ($quiet) { "F5's SendString between two keys: no note in 3 s, waiting for the reconnect with them" }
} -Passed 'mux-write-failed: with a pane''s server gone the strip''s seek ends at once and says so, and a write waits for the reconnect with the keys round it'
