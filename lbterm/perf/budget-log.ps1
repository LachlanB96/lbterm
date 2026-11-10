#Requires -Version 7
# Variant: -Mux
# Covers: lbterm-extras/src/budget.rs, lbterm-extras/src/slow.rs, lbterm-extras/src/counters.rs, wezterm-gui/src/termwindow/budget.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/state.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/spawn.rs, wezterm-gui/src/termwindow/last_tab.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/tasks_card.rs, wezterm-gui/src/counters.rs, config/src/lbterm.rs, config/src/lib.rs, lbterm/lbterm-lib.ps1, lbterm/lbterm.ps1
<#
The budget's warnings in the daily build (lbterm/PERF.md, "Warnings in the daily build";
lbterm-extras/src/budget.rs, wezterm-gui/src/termwindow/budget.rs) end to end, in a test window that
looks like Lachlan's LBTerm (New-TestConfig: WebGpu with the compositor, max_fps 120, the rail),
with lbterm_budget at its defaults but a tab switch's time, 40 ms. Whatever the window logs on
target lbterm::budget is read from its log; what it counted, from the control pipe's `metrics`
(its `budget` part, each action's counts and percentiles among them). Waits are on the state file;
keys and events are posted messages and the control pipe's `emit`, never real input. -Mux puts its
tabs in a mux server of their own, as Lachlan's are. The cursor is a blinking block
(cursor_blink_rate 800) at Lachlan's animation_fps 1, so it blinks only while the window is told
it has the keyboard (step 3).

1. At rest, nothing going on, for 12 s: no lbterm::budget line, and metrics counts no frame, timer
   or task at rest (the cli listener's few on a mux domain are under rest_tasks).
2. A clock in view, as a config shows one: update-status each second (status_update_interval 1000)
   setting the right status to the time, for 12 s. On purpose at its rate: no line, the status
   purpose's ticks and frames counted, and at most the emit's own first frame at rest.
3. A blinking cursor, the window told it has the keyboard (a posted WM_SETFOCUS), for 12 s:
   upstream's frames turning the cursor on and off every 800 ms, 1.25 a second where
   animation_fps says 1. On purpose at the rate it really comes: no line, the animation purpose's
   ticks counted with at most one past its rate, at most two frames, no timer and at most
   rest_tasks tasks counted at rest.
4. Painting at rest on purpose, the test's way: a Lua timer (wezterm.time.call_after) setting the
   right status 30 times a second, which nothing in LBTerm says is on purpose. Within 25 s,
   `window-<id> painted <n> frames in 10 s at rest, asked by <site> (<n>)`, its first site the one
   metrics says asked most, and at least three in four of the frames it painted counted at rest
   (the line's span starts at the first frame at rest, which can be the clock's last; how fast
   the Lua timer runs depends on the PC); and `tasks ran <n> times in 10 s at rest ...` for the
   Lua timer's tasks and each frame's own. A timers line comes only from a counted timer: the
   paint throttle's, which each frame armed, went with perf/frame-loop's pacing, whose hold is
   the message loop's own timer, so none is asked for; one said is checked against metrics. `lbterm.ps1 logs -Pid <pid> -Budget`
   reads them back, naming the site. Then it stops, and at rest again for 12 s nothing more is
   counted at rest.
5. A tab switch over its time: F9's Lua handler holds the GUI thread 60 ms, then switches tabs.
   `tab took <ms> ms, <over> ms over its 40 ms: task wezterm-gui/src/termwindow/mod.rs:<n> ...`,
   <n> the line in schedule_window_event that spawns a window event's Lua, at least 50 ms of it:
   the handler ran before the switch was begun, and its time is still what the line says it went
   on (the thread's recent runs).
6. Every action timed and shown: a new tab (F8), tab switches (F4), a split (F7), a closed tab
   (F6), a reload (F5), keys echoed by pwsh, paints. Each is counted in metrics' budget.actions with
   its percentiles; their times are printed, not held to anything here (budgets.ps1's).

-Measure instead does each action -Runs times (the rail and the InMail list too, with posted
pointer moves and a message of the test's own) and prints each one's p50, p95 and longest from
metrics' budget.actions, the numbers config/src/lbterm.rs's defaults come from (lbterm/PERF.md,
"Expected times"), into target\perf\budget-log-measure\measure.json.

  pwsh -File lbterm\perf\budget-log.ps1 [-Mux]
  pwsh -File lbterm\perf\budget-log.ps1 -Measure [-Runs 15] [-Bin <build>] [-Mux]
#>
param([string]$Bin, [switch]$Mux, [switch]$Measure, [int]$Runs = 15)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
$name = 'budget-log' + $(if ($Measure) { '-measure' } else { '' }) + $(if ($Mux) { '-mux' } else { '' })
$T = Start-Test $name -Bin $Bin
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\budget.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$inbox = Join-Path $T.Out 'projects\One\InMail'
Remove-Item -Recurse -Force (Join-Path $T.Out 'projects') -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $inbox | Out-Null
$at = (Get-Date).AddHours(-1)
[IO.File]::WriteAllText((Join-Path $inbox ('{0:yyyy-MM-dd-HHmm}-from-Two-a-message.md' -f $at)),
  "---`nfrom: Two`nto: One`nsent: {0:yyyy-MM-ddTHH:mm}`nsubject: `"A message`"`n---`n`nThe body.`n" -f $at)

# The line that spawns a window event's Lua: what an action_callback's task is named by
$src = Get-Content (Join-Path $T.Repo 'wezterm-gui\src\termwindow\mod.rs')
$fn = ($src | Select-String -Pattern 'fn schedule_window_event' | Select-Object -First 1).LineNumber
$spawnLine = ($src | Select-Object -Skip $fn | Select-String -Pattern 'promise::spawn::spawn\(config::with_lua_config_on_main_thread' |
  Select-Object -First 1).LineNumber + $fn
$luaTask = "wezterm-gui/src/termwindow/mod.rs:$spawnLine"
$tabMs = 40

New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 28
-- A blinking cursor at Lachlan's animation_fps: it blinks while the window has the keyboard
config.animation_fps = 1
config.default_cursor_style = 'BlinkingBlock'
config.cursor_blink_rate = 800
rawset(config, 'lbterm_inbox', { folders = { [[$inbox]] } })
-- A clock in view, as a config shows one: update-status each second
config.status_update_interval = 1000
local clock = false
wezterm.on('update-status', function(window, pane)
  if clock then window:set_right_status(wezterm.strftime('%H:%M:%S')) end
end)
wezterm.on('budget-clock-on', function(window, pane) clock = true; window:set_right_status(wezterm.strftime('%H:%M:%S')) end)
wezterm.on('budget-clock-off', function(window, pane) clock = false; window:set_right_status('') end)
-- The test's own painting at rest: the right status changed 30 times a second from a Lua timer
local painting = false
local n = 0
local function paint(window)
  if not painting then return end
  n = n + 1
  window:set_right_status('frame ' .. n)
  wezterm.time.call_after(0.033, function() paint(window) end)
end
wezterm.on('budget-paint-on', function(window, pane) painting = true; paint(window) end)
wezterm.on('budget-paint-off', function(window, pane) painting = false; window:set_right_status('') end)
$(if (-not $Measure) { "rawset(config, 'lbterm_budget', { actions = { tab = $tabMs } })" })
config.keys = config.keys or {}
local pwsh = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
for _, k in ipairs({
  -- Holds the GUI thread 60 ms, as a slow Lua handler would, then switches tabs. MSVC's clock() is wall time
  { key = 'F9', mods = 'NONE', action = wezterm.action_callback(function(window, pane)
      local t = os.clock()
      while os.clock() - t < 0.060 do end
      window:perform_action(wezterm.action.ActivateTabRelative(1), pane)
    end) },
  { key = 'F8', mods = 'NONE', action = wezterm.action.SpawnCommandInNewTab { args = pwsh } },
  { key = 'F7', mods = 'NONE', action = wezterm.action.SplitHorizontal { args = pwsh } },
  { key = 'F6', mods = 'NONE', action = wezterm.action.CloseCurrentTab { confirm = false } },
  { key = 'F5', mods = 'NONE', action = wezterm.action.ReloadConfiguration },
  { key = 'F4', mods = 'NONE', action = wezterm.action.ActivateTabRelative(1) },
  { key = 'F3', mods = 'NONE', action = wezterm.action.CloseCurrentPane { confirm = false } },
}) do table.insert(config.keys, k) end
$(if ($Mux) { @"
config.unix_domains = { {
  name = 'budgetmux',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@ })
"@

# The process's frames, as the state file counts them, once nothing has painted for $QuietMs
function Get-QuietFrame([int]$QuietMs = 700, [double]$TimeoutSec = 15) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $frame = (Read-State).frame
  $since = [Diagnostics.Stopwatch]::StartNew()
  while ($since.ElapsedMilliseconds -lt $QuietMs) {
    if ($clock.Elapsed.TotalSeconds -gt $TimeoutSec) { throw "the window kept painting for $TimeoutSec s (frame $frame)" }
    Start-Sleep -Milliseconds 50
    $now = (Read-State).frame
    if ($now -ne $frame) { $frame = $now; $since.Restart() }
  }
  [long]$frame
}

# The window as the state file has it
function Get-Win { Get-StateWindow (Read-State) -Hwnd ([long](Get-Test).Current.Handle) }

# The budget's lines the window has logged, read as lbterm.ps1 logs reads them; -AtRest only what
# ran at rest (frames, timers, tasks), not actions over their time (a window's start-up's)
function Get-BudgetLines([switch]$AtRest) {
  if ($AtRest) { return @(Get-BudgetLines | Where-Object Kind -ne 'action') }
  @(Get-LogLines $w 'lbterm::budget' | ForEach-Object {
      $message = ($_ -split ' > ', 2)[1]
      $b = ConvertFrom-LbtermBudgetLine $message
      if ($b) { $b | Add-Member -NotePropertyName Message -NotePropertyValue $message -PassThru }
    })
}

function Send-Emit([string]$Event) {
  $out = @(& $T.Cli control --class $T.Class emit $Event 2>&1)
  if ($LASTEXITCODE) { throw "emit ${Event}: $($out -join ' ')" }
}

# An action's counts and percentiles in a metrics reply (budget.actions), as count, p50, p95, max
# and mean in milliseconds
function Get-ActionHistogram($M, [string]$Action) {
  $a = @($M.budget.actions | Where-Object action -eq $Action)[0]
  if ($a) { [pscustomobject]@{ count = [int]$a.count; p50 = $a.p50_ms; p95 = $a.p95_ms; max = $a.max_ms; mean = $a.mean_ms } }
}

function Get-ActionCount($M, [string]$Action) {
  $a = @($M.budget.actions | Where-Object action -eq $Action)[0]
  if ($a) { [int]$a.count } else { 0 }
}

# A key posted and the window answering it: a change in the state file, then quiet
function Invoke-Step([string]$Key, [scriptblock]$Until, [string]$What) {
  Send-Key $w $Key
  Wait-Change $Until -What $What -TimeoutSec 15 | Out-Null
}

Invoke-Test {
  $open = @{ ConfigFile = $cfg; Program = (New-PwshProgram); PostedPointer = $true; TimeoutSec = 30 }
  if ($Mux) { $open.Domain = 'budgetmux'; $open.Socket = $sock }
  $w = Open-TestWindow @open
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }
  Wait-Change { param($s) @($s.panes).Count -ge 1 -and $s.rail } -What 'the first pane and the rail' -TimeoutSec 30 | Out-Null
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  Invoke-Cli send-text --pane-id $first --no-paste "'shell up'`r" -Server:$Mux -Check | Out-Null
  Start-Sleep -Milliseconds 1500
  Wait-Settled -What 'the shell to start' -TimeoutSec 20 | Out-Null
  Get-QuietFrame | Out-Null
  $windowId = (Get-Win).id

  if ($Measure) {
    $result = [ordered]@{ build = (Get-Content (Join-Path $T.Bin 'version.txt') -ErrorAction SilentlyContinue) ?? $T.Bin; mux = [bool]$Mux; runs = $Runs }
    Get-Metrics -Reset | Out-Null
    # Keys: one at a time into the prompt, each echoed, the line cleared now and then
    for ($i = 0; $i -lt $Runs; $i++) {
      $f0 = (Read-State).frame
      Send-Key $w 'x'
      Wait-State { param($sw, $s) $s.frame -gt $f0 } -What 'an echo' -TimeoutSec 10 | Out-Null
      Start-Sleep -Milliseconds 120
      if ($i % 10 -eq 9) { Send-Key $w Escape; Start-Sleep -Milliseconds 200 }
    }
    Send-Key $w Escape
    Get-QuietFrame | Out-Null
    # A new tab and its close, then tab switches between two
    for ($i = 0; $i -lt $Runs; $i++) {
      Invoke-Step F8 { param($s) @($s.tabs).Count -eq 2 } 'a new tab'
      Get-QuietFrame -QuietMs 400 | Out-Null
      Invoke-Step F6 { param($s) @($s.tabs).Count -eq 1 } 'the tab closed'
      Get-QuietFrame -QuietMs 400 | Out-Null
    }
    Invoke-Step F8 { param($s) @($s.tabs).Count -eq 2 } 'a second tab'
    Get-QuietFrame | Out-Null
    for ($i = 0; $i -lt $Runs; $i++) {
      $was = (Get-Win).active_tab
      Invoke-Step F4 { param($s) $s.active_tab -ne $was } 'a tab switch'
      Get-QuietFrame -QuietMs 300 | Out-Null
    }
    Invoke-Step F6 { param($s) @($s.tabs).Count -eq 1 } 'back to one tab'
    Get-QuietFrame | Out-Null
    # A split and its close
    for ($i = 0; $i -lt $Runs; $i++) {
      Invoke-Step F7 { param($s) @($s.panes).Count -eq 2 } 'a split'
      Get-QuietFrame -QuietMs 400 | Out-Null
      Invoke-Step F3 { param($s) @($s.panes).Count -eq 1 } 'the split closed'
      Get-QuietFrame -QuietMs 400 | Out-Null
    }
    # The rail: the pointer resting on the first pad, then away
    for ($i = 0; $i -lt $Runs; $i++) {
      $s = Get-Win
      $x, $y = Get-RectCenter $s.rail.rows[0].pad
      Send-Mouse $w Move $x $y
      Wait-State { param($sw) $sw.rail.open -and -not $sw.animating } -What 'the rail open' -TimeoutSec 10 | Out-Null
      Send-Leave $w
      Wait-State { param($sw) $sw.rail.phase -eq 'closed' -and $sw.settled } -What 'the rail closed' -TimeoutSec 10 | Out-Null
    }
    # The InMail list: a click on the button, Esc
    for ($i = 0; $i -lt $Runs; $i++) {
      $sw = Wait-State { param($sw) $sw.strip.inbox -ge 1 -and @($sw.strip.items | Where-Object name -eq 'inbox') } -What 'the InMail button' -TimeoutSec 10
      $b = @($sw.strip.items | Where-Object name -eq 'inbox')[0]
      $c = Get-RectCenter $b.rect
      Send-Click $w $c[0] $c[1]
      Wait-State { param($sw) $sw.menu.kind -eq 'inbox' } -What 'the list' -TimeoutSec 10 | Out-Null
      Send-Key $w Escape
      Wait-State { param($sw) -not $sw.menu -and $sw.settled } -What 'the list closed' -TimeoutSec 10 | Out-Null
      Send-Leave $w
    }
    # Reloads: each puts "Config reloaded" in the strip for 4 s
    for ($i = 0; $i -lt [Math]::Min($Runs, 6); $i++) {
      $n0 = Get-ActionCount (Get-Metrics) 'reload'
      Send-Key $w F5
      if (-not (Wait-Until { (Get-ActionCount (Get-Metrics) 'reload') -gt $n0 } -TimeoutSec 15)) { throw 'a reload never showed' }
      Start-Sleep -Milliseconds 300
    }
    $m = Get-Metrics
    $table = foreach ($a in 'key', 'tab', 'new-tab', 'close-tab', 'split', 'rail', 'inbox', 'reload', 'paint') {
      $h = Get-ActionHistogram $m $a
      $c = @($m.budget.actions | Where-Object action -eq $a)[0]
      [ordered]@{ action = $a; count = $h.count; p50 = $h.p50; p95 = $h.p95; max = $h.max; mean = $h.mean; budget_ms = $c.budget_ms; over = $c.over; unshown = $c.unshown }
    }
    $result.actions = @($table)
    $result.lines = @(Get-BudgetLines | ForEach-Object Message)
    foreach ($r in $table) { '{0,-14} {1,4} runs  p50 {2,7} ms  p95 {3,7} ms  max {4,7} ms  (its {5} ms, {6} over, {7} not shown)' -f $r.action, $r.count, $r.p50, $r.p95, $r.max, $r.budget_ms, $r.over, $r.unshown }
    $json = $result | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText((Join-Path $T.Out 'measure.json'), $json)
    return
  }

  # 1. At rest, nothing going on
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds 12
  $rest = Get-Metrics -Reset
  $b = $rest.budget
  "1. at rest 12 s: $(Format-Metrics $rest -Top 3 | Out-String)".TrimEnd()
  "   budget: at rest $($b.at_rest.frames) frames, $($b.at_rest.timers) timers, $($b.at_rest.tasks) tasks ($($b.at_rest.task_ms) ms); $($b.lines) lines"
  if (-not $b.enabled) { Add-Failure "the budget is off: $($b | ConvertTo-Json -Compress -Depth 5)" }
  $lines = @(Get-BudgetLines -AtRest)
  if ($lines) { Add-Failure "at rest with nothing going on, it said: $(@($lines.Message) -join ' | ')" }
  foreach ($l in @(Get-BudgetLines | Where-Object Kind -eq 'action')) { "   (start-up: $($l.Message))" }
  if ($b.at_rest.frames -ne 0 -or $b.at_rest.timers -ne 0) { Add-Failure "at rest: $($b.at_rest | ConvertTo-Json -Compress -Depth 4)" }
  if ($b.at_rest.tasks -gt 20) { Add-Failure "at rest, $($b.at_rest.tasks) tasks: $($b.at_rest.task_sites | ConvertTo-Json -Compress)" }
  if ($rest.totals.frames -ne 0) { Add-Failure "$($rest.totals.frames) frames at rest" }

  # 2. A clock in view, on purpose at its rate
  Send-Emit 'budget-clock-on'
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds 12
  $clock = Get-Metrics -Reset
  Send-Emit 'budget-clock-off'
  $status = @($clock.budget.on_purpose | Where-Object purpose -eq 'status')[0]
  "2. a clock in view 12 s: $($clock.totals.frames) frames; status ticks $($status.ticks) ($($status.past_rate) past its rate), $($status.frames) frames, $($status.timers) timers and $($status.tasks) tasks on purpose; at rest $($clock.budget.at_rest.frames) frames, $($clock.budget.at_rest.tasks) tasks"
  if (-not $status -or $status.ticks -lt 9 -or $status.frames -lt 8) { Add-Failure "the clock's ticks weren't counted on purpose: $($clock.budget.on_purpose | ConvertTo-Json -Compress)" }
  if ($status.past_rate -gt 0) { Add-Failure "$($status.past_rate) of the clock's ticks were past its rate" }
  if ($clock.budget.at_rest.frames -gt 2) { Add-Failure "a clock's frames counted at rest: $($clock.budget.at_rest | ConvertTo-Json -Compress -Depth 4)" }
  $lines = @(Get-BudgetLines -AtRest)
  if ($lines) { Add-Failure "a clock in view, it said: $(@($lines.Message) -join ' | ')" }
  Get-QuietFrame | Out-Null

  # 3. A blinking cursor at animation_fps 1, the window with the keyboard (as far as it knows)
  $before = @(Get-BudgetLines -AtRest).Count
  Send-Focus $w
  Start-Sleep -Milliseconds 1500
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds 12
  $blink = Get-Metrics -Reset
  Send-Focus $w -Off
  $anim = @($blink.budget.on_purpose | Where-Object purpose -eq 'animation')[0]
  $r = $blink.budget.at_rest
  "3. a blinking cursor 12 s: $($blink.totals.frames) frames; animation ticks $($anim.ticks) ($($anim.past_rate) past its rate), $($anim.frames) frames, $($anim.timers) timers and $($anim.tasks) tasks on purpose; at rest $($r.frames) frames, $($r.timers) timers, $($r.tasks) tasks"
  if ([int]$blink.totals.frames -lt 10) { Add-Failure "the cursor painted $($blink.totals.frames) frames in 12 s: it isn't blinking" }
  if (-not $anim -or [int]$anim.ticks -lt 10 -or [int]$anim.frames -lt 10) { Add-Failure "the blink's ticks weren't counted on purpose: $($blink.budget.on_purpose | ConvertTo-Json -Compress)" }
  if ($anim -and [int]$anim.past_rate -gt 1) { Add-Failure "$($anim.past_rate) of the blink's ticks were past its rate" }
  if ([int]$r.frames -gt 2) { Add-Failure "a blinking cursor's frames counted at rest: $($r | ConvertTo-Json -Compress -Depth 4)" }
  if ([int]$r.timers -gt 0 -or [int]$r.tasks -gt 20) { Add-Failure "a blinking cursor's timers or tasks counted at rest: $($r | ConvertTo-Json -Compress -Depth 4)" }
  $lines = @(Get-BudgetLines -AtRest | Select-Object -Skip $before)
  if ($lines) { Add-Failure "a blinking cursor, it said: $(@($lines.Message) -join ' | ')" }
  Get-QuietFrame | Out-Null

  # 4. Painting at rest on purpose, the test's way
  Get-Metrics -Reset | Out-Null
  $painting = [Diagnostics.Stopwatch]::StartNew()
  Send-Emit 'budget-paint-on'
  # The frames, the timers and the tasks each have their 10 s span from their first at rest: all
  # said within two spans. No counted timer runs for a frame since perf/frame-loop: its pacing
  # holds on the message loop's own timer
  $kinds = 'frames', 'tasks'
  $said = Wait-Until { $l = @(Get-BudgetLines -AtRest); -not @($kinds | Where-Object { $k = $_; -not @($l | Where-Object Kind -eq $k) }) } -TimeoutSec 25
  $paint = Get-Metrics
  Send-Emit 'budget-paint-off'
  $saidAfter = [Math]::Round($painting.Elapsed.TotalSeconds, 1)
  $frames = @(Get-BudgetLines | Where-Object Kind -eq 'frames')
  $tasks = @(Get-BudgetLines | Where-Object Kind -eq 'tasks')
  $timers = @(Get-BudgetLines | Where-Object Kind -eq 'timers')
  $top = @($paint.requests)[0]
  "4. painting at rest: said after $saidAfter s; metrics $($paint.totals.frames) frames, asked most by $($top.site) ($($top.frames)); the budget counted $($paint.budget.at_rest.frames) frames, $($paint.budget.at_rest.timers) timers and $($paint.budget.at_rest.tasks) tasks at rest"
  foreach ($l in Get-BudgetLines -AtRest) { "   $($l.Message)" }
  if ($timers -and -not (@($paint.timers.site) -contains $timers[0].Top)) { Add-Failure "the timers line's $($timers[0].Top) isn't among metrics' timers" }
  if (-not $frames) { Add-Failure "no frames line in 25 s of painting at rest" }
  else {
    $f = $frames[0]
    if ($f.What -ne "window-$windowId") { Add-Failure "the line names $($f.What), the window is window-$windowId" }
    # The line's span started at the first frame at rest, which can be the clock's last, seconds
    # before the painting: what the budget counted says how much of it was at rest
    if ($f.Count -le 5) { Add-Failure "it said $($f.Count) frames, no more than rest_frames" }
    # How fast the Lua timer runs depends on Windows' timer resolution and the PC's load (19 frames
    # a second in one run, 7 in another): what's held is that the frames it painted, nothing else
    # going on, were counted at rest, but those the status ticks it causes may take
    $atRest = [int]$paint.budget.at_rest.frames
    if ($atRest -lt 30 -or $atRest -lt 0.75 * $paint.totals.frames) { Add-Failure "the budget counted $atRest of the $($paint.totals.frames) frames painted in $saidAfter s at rest" }
    if ($f.Top -ne $top.site) { Add-Failure "the line says $($f.Top) asked most, metrics $($top.site)" }
  }
  if (-not $tasks) { Add-Failure 'no tasks line for the Lua timer''s tasks at rest' }
  elseif (-not (@($paint.tasks.site) -contains $tasks[0].Top)) { Add-Failure "the tasks line's $($tasks[0].Top) isn't among metrics' tasks" }
  $read = & 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File (Join-Path $T.Repo 'lbterm\lbterm.ps1') logs -Pid $w.Pid -Budget 2>&1 | Out-String
  if ($frames -and $read -notmatch [regex]::Escape("most asked by $($frames[0].Top)")) { Add-Failure "lbterm.ps1 logs -Budget didn't name $($frames[0].Top): $read" }
  else { "   lbterm.ps1 logs -Budget: $((($read -split "`r?`n") | Where-Object { $_ -match 'frames  at rest' } | Select-Object -First 1).Trim())" }
  Get-QuietFrame | Out-Null
  $before = @(Get-BudgetLines -AtRest).Count
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds 12
  $after = Get-Metrics -Reset
  $more = @(Get-BudgetLines -AtRest | Select-Object -Skip $before)
  # The paint's spans may end, or a held summary be said, after the painting stopped; nothing new
  # at rest may be counted
  if ($after.budget.at_rest.frames -ne 0) { Add-Failure "at rest again, $($after.budget.at_rest.frames) frames counted at rest" }
  if (@($more | Where-Object { $_.Kind -ne 'frames' -and $_.Kind -ne 'tasks' })) { Add-Failure "at rest again, it said: $(@($more.Message) -join ' | ')" }
  "   at rest again 12 s: $($after.totals.frames) frames, $(@($more).Count) lines more (the painting's own, ended)"

  # 5. A tab switch over its time, its Lua handler before it
  Invoke-Step F8 { param($s) @($s.tabs).Count -eq 2 } 'a second tab'
  Get-QuietFrame | Out-Null
  $was = (Get-Win).active_tab
  Invoke-Step F9 { param($s) $s.active_tab -ne $was } 'the slow tab switch'
  $said = Wait-Until { @(Get-BudgetLines | Where-Object { $_.Kind -eq 'action' -and $_.What -eq 'tab' -and $_.Spans -eq 1 }).Count -ge 1 } -TimeoutSec 5
  $tab = @(Get-BudgetLines | Where-Object { $_.Kind -eq 'action' -and $_.What -eq 'tab' -and $_.Spans -eq 1 })[0]
  if (-not $said -or -not $tab) { Add-Failure 'no line for the tab switch over its time' }
  else {
    "5. $($tab.Message)"
    if ($tab.Budget -ne $tabMs) { Add-Failure "the line's budget is $($tab.Budget) ms, the config's $tabMs" }
    if ($tab.Ms -lt 60) { Add-Failure "the switch took $($tab.Ms) ms, with a 60 ms handler before it" }
    $lua = @($tab.Sites | Where-Object Site -eq "task $luaTask")[0]
    if (-not $lua -or $lua.Ms -lt 50) { Add-Failure "the line doesn't say the time went on the handler's task ($luaTask): $($tab.Message)" }
  }

  # 6. Every action, shown and timed
  Get-QuietFrame | Out-Null
  $was = (Get-Win).active_tab
  Invoke-Step F4 { param($s) $s.active_tab -ne $was } 'a plain tab switch'
  Get-QuietFrame | Out-Null
  Invoke-Step F7 { param($s) @($s.panes).Count -eq 2 } 'a split'
  Get-QuietFrame | Out-Null
  Invoke-Step F6 { param($s) @($s.tabs).Count -eq 1 } 'the tab closed'
  Get-QuietFrame | Out-Null
  $f0 = (Read-State).frame
  Send-Chars $w 'ab'
  Wait-State { param($sw, $s) $s.frame -gt $f0 } -What 'the keys'' echo' | Out-Null
  Get-QuietFrame | Out-Null
  Send-Key $w Escape
  Send-Key $w F5
  if (-not (Wait-Until { (Get-ActionCount (Get-Metrics) 'reload') -ge 1 } -TimeoutSec 15)) { Add-Failure 'the reload was never shown' }
  $m = Get-Metrics
  foreach ($a in @{ action = 'new-tab'; least = 1 }, @{ action = 'tab'; least = 2 }, @{ action = 'split'; least = 1 }, @{ action = 'close-tab'; least = 1 }, @{ action = 'reload'; least = 1 }, @{ action = 'key'; least = 1 }, @{ action = 'paint'; least = 10 }) {
    $h = Get-ActionHistogram $m $a.action
    $c = Get-ActionCount $m $a.action
    $row = @($m.budget.actions | Where-Object action -eq $a.action)[0]
    "6. $($a.action): $c shown, $($row.over) over its $($row.budget_ms) ms; p50 $($h.p50) ms, max $($h.max) ms"
    if ($c -lt $a.least -or -not $h -or $h.count -lt $a.least) { Add-Failure "$($a.action): $c shown and a histogram of $($h.count), at least $($a.least) expected" }
  }
} -Passed "budget-log$(if ($Measure) { ' measured' })$(if ($Mux) { ' (mux)' }): nothing said at rest, a clock and a blinking cursor on purpose, painting at rest named by who asked, a slow tab switch named by its handler, every action timed"
