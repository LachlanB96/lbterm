#Requires -Version 7
# Variant: -Mux
# Covers: lbterm-extras/src/counters.rs, lbterm-extras/src/slow.rs, lbterm-extras/src/control.rs, wezterm-gui/src/counters.rs, wezterm-gui/src/control.rs, wezterm-gui/src/stats.rs, wezterm-gui/src/main.rs, wezterm-gui/src/termwindow/render/paint.rs, window/src/os/windows/window.rs, window/src/os/windows/connection.rs, window/src/spawn.rs, wezterm-client/src/client.rs, codec/src/lib.rs, wezterm/src/control.rs, lbterm/harness.psm1
<#
The counters (lbterm/PERF.md) end to end, through the control pipe's `metrics` (Get-Metrics), in a
test window of its own class that never takes focus, drawn as Lachlan's are (WebGpu with the
compositor, max_fps 120; -Mux puts its tabs in a mux server of their own, as his are). Every wait
is on the state file, never a capture: PrintWindow paints the window itself, a frame of the
test's own.

1. The first reply: the window's pid and build, a span, the process's CPU and the GUI thread's,
   its memory, handles and threads, the frames it has painted since it started, and upstream's
   paint histogram (gui.paint.impl).
2. At rest, settled, for 4 s between two resets: no frame, no repaint asked for, no paint. What
   did run (tasks, timers, messages, wakes) is printed; holding those to a budget is
   budgets.ps1's.
3. A frame: `echo` sent to the pane. The frames the state file counts are the frames metrics
   counts, no more and no fewer, and the requests behind them are named, with the frames they
   asked for.
4. Keys posted to the window: each WM_KEYDOWN counted.
5. A split, the GUI's own (a Lua event, sent through the control pipe's `emit`): its tasks, its
   frames and who asked for them (its frames are paced by wm_paint's hold on the message loop's
   own timer since perf/frame-loop, which no counted timer stands for); with -Mux, the SplitPane PDU
   the GUI's mux client sent and the server's PDUs it received, by type, with their bytes.
6. A reset straight after a reset covers nothing, and `lbterm control metrics` answers from a
   shell.

-Measure instead measures what two things cost, for comparing builds (lbterm/PERF.md, "What the
counters cost"): -Keys keys typed into a pwsh prompt 8 ms apart, and a pane printing as fast as
pwsh can write for -Seconds, in a tab of its own. It prints one JSON line: the GUI process's CPU
and its GUI thread's (Windows' own numbers, so a build without metrics is measured the same way),
the frames each painted (the state file's count), how long each took and how much the writer
wrote. -Counts adds what metrics counted in each, for a build that has it; leave it off when
comparing with one that hasn't. The GUI thread's CPU is said by its cycles too (`*_thread_cycles_ms`),
which Windows' times, in 15.6 ms ticks, can't resolve. -Present turns the timing to the screen on as
the window starts (LBTERM_PRESENT_TIMING, lbterm-extras/src/present.rs), for what it costs; without
it, a build that has it leaves it off, as Lachlan's window does.

  pwsh -File lbterm\perf\metrics.ps1 [-Mux]
  pwsh -File lbterm\perf\metrics.ps1 -Measure [-Bin <build>] [-Mux] [-Counts] [-Present] [-Keys 400] [-Seconds 8]
#>
param([string]$Bin, [switch]$Mux, [switch]$Measure, [switch]$Counts, [switch]$Present, [int]$Keys = 400, [int]$Seconds = 8)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$name = 'metrics' + $(if ($Measure) { '-measure' } else { '' }) + $(if ($Mux) { '-mux' } else { '' })
$T = Start-Test $name -Bin $Bin
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\metrics.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

$domain = if ($Mux) {
  @"
  unix_domains = { {
    name = 'perfmetrics',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@
} else { '' }
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
-- The split step's: the GUI splits the pane itself, as a key would
wezterm.on('metrics-split', function(window, pane)
  pane:split { direction = 'Right', args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' } }
end)
return {
  automatically_reload_config = false,
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  wsl_domains = {},
  initial_cols = 120,
  initial_rows = 32,
  -- As Lachlan's lbterm.lua has them: the compositor, and frames up to 120 a second
  front_end = 'WebGpu',
  max_fps = 120,
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
$domain}
"@)

# The process's frames, as the state file counts them, once nothing has painted for $QuietMs
function Get-QuietFrame([int]$QuietMs = 700, [double]$TimeoutSec = 10) {
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

# The test window's tabs, as its state file has them
function Get-TabCount { @((Get-StateWindow (Read-State) -Hwnd ([long](Get-Test).Current.Handle)).tabs).Count }

# The window settled, its shell's prompt drawn and nothing painting
function Wait-Quiet([string]$What) {
  Wait-Settled -What $What -TimeoutSec 20 | Out-Null
  Get-QuietFrame
}

Invoke-Test {
  $presentEnv = if ($Present) { @{ LBTERM_PRESENT_TIMING = '1' } } else { @{} }
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfmetrics -Socket $sock -TimeoutSec 20 -Environment $presentEnv }
  else { Open-TestWindow -ConfigFile $cfg -Cwd 'C:\Windows' -TimeoutSec 20 -Environment $presentEnv }
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }
  Wait-Change { param($s) @($s.panes).Count -ge 1 } -What 'the first pane' -TimeoutSec 20 | Out-Null
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  # The shell's first prompt: a line it prints once it's up, and quiet after
  Invoke-Cli send-text --pane-id $first --no-paste "'shell up'`r" -Server:$Mux -Check | Out-Null
  Start-Sleep -Milliseconds 1500
  Wait-Quiet 'the shell to start' | Out-Null

  if ($Measure) {
    $result = [ordered]@{ build = (Get-Content (Join-Path $T.Bin 'version.txt') -ErrorAction SilentlyContinue) ?? $T.Bin; mux = [bool]$Mux; present = [bool]$Present }
    Initialize-CpuReader
    $rate = Get-CycleRate
    $tid = Get-GuiThreadId $w
    $cycles = { [LbtermE2E.Cpu]::ThreadCycles([uint32]$tid) }
    $count = { param($m) if ($m) { [ordered]@{ frames = $m.totals.frames; requests = $m.totals.requests; tasks = $m.totals.tasks; messages = $m.totals.messages; paints = $m.totals.paints; timers = $m.totals.timers_fired; wakes = $m.totals.wakes; pdus = $m.totals.pdus_sent + $m.totals.pdus_received } } }

    # Typing: letters into the prompt, each echoed by the shell
    if ($Counts) { Get-Metrics -Reset | Out-Null }
    $frames0 = (Read-State).frame
    $cpu0 = Get-GuiCpu $w
    $c0 = & $cycles
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $letters = 'abcdefghijklmnopqrstuvwxyz'
    Send-Key $w @(0..($Keys - 1) | ForEach-Object { [string]$letters[$_ % 26] }) -GapMs 8
    $typedMs = $clock.ElapsedMilliseconds
    Get-QuietFrame | Out-Null
    $cpu1 = Get-GuiCpu $w
    $c1 = & $cycles
    $result.keys = $Keys
    $result.typing_ms = $typedMs
    $result.typing_frames = (Read-State).frame - $frames0
    $result.typing_cpu_ms = [Math]::Round($cpu1.Process - $cpu0.Process, 1)
    $result.typing_thread_ms = [Math]::Round($cpu1.Thread - $cpu0.Thread, 1)
    $result.typing_thread_cycles_ms = [Math]::Round(($c1 - $c0) / $rate, 2)
    if ($Counts) { $result.typing_counts = & $count (Get-Metrics -Reset) }
    Send-Key $w Escape
    Get-QuietFrame | Out-Null

    # Output at full speed: a tab of its own printing for -Seconds as fast as pwsh can write, then
    # saying how much it wrote. Not a file through cmd's type: ConPTY takes 40 MB of that in a
    # quarter of a second and hands the GUI three frames of it
    $wrote = Join-Path $T.Out 'wrote.txt'
    $writer = @"
`$e = [char]27
`$lines = foreach (`$i in 0..199) { if (`$i % 10 -eq 0) { "`$e[32mline `$i`$e[0m the quick brown fox jumps over the lazy dog `$('x' * (`$i % 60))" } else { "line `$i the quick brown fox jumps over the lazy dog `$('y' * (`$i % 70))" } }
`$chunk = (`$lines -join "``r``n") + "``r``n"
`$out = [Console]::Out
`$clock = [Diagnostics.Stopwatch]::StartNew()
`$n = 0
while (`$clock.ElapsedMilliseconds -lt $($Seconds * 1000)) { `$out.Write(`$chunk); `$n++ }
[IO.File]::WriteAllText('$wrote', [string](`$n * `$chunk.Length))
"@
    $tabs0 = Get-TabCount
    $frames0 = (Read-State).frame
    if ($Counts) { Get-Metrics -Reset | Out-Null }
    $cpu0 = Get-GuiCpu $w
    $c0 = & $cycles
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $writerPane = New-Tab -From $first -Program (New-PwshProgram $writer) -Server:$Mux
    # On a mux domain the window doesn't follow a spawn to its tab, and a tab out of sight isn't
    # painted: open it
    if ($Mux) { Invoke-Cli activate-pane --pane-id $writerPane -Server -Check | Out-Null }
    # Its tab comes and goes; the state file says so without asking the GUI anything
    if (-not (Wait-Until { (Get-TabCount) -gt $tabs0 } -TimeoutSec 10 -IntervalMs 20)) { throw 'the writer''s tab never opened' }
    if (-not (Wait-Until { (Get-TabCount) -le $tabs0 } -TimeoutSec ($Seconds + 30) -IntervalMs 50)) { throw 'the writer''s tab never ended' }
    $outMs = $clock.ElapsedMilliseconds
    $cpu1 = Get-GuiCpu $w
    $c1 = & $cycles
    $frames1 = (Read-State).frame
    $chars = [long](Get-Content -Raw $wrote)
    $result.output_s = $Seconds
    $result.output_mb = [Math]::Round($chars / 1MB, 1)
    $result.output_ms = $outMs
    $result.output_frames = $frames1 - $frames0
    $result.output_cpu_ms = [Math]::Round($cpu1.Process - $cpu0.Process, 1)
    $result.output_thread_ms = [Math]::Round($cpu1.Thread - $cpu0.Thread, 1)
    $result.output_thread_cycles_ms = [Math]::Round(($c1 - $c0) / $rate, 2)
    if ($Counts) { $result.output_counts = & $count (Get-Metrics -Reset) }
    $line = $result | ConvertTo-Json -Compress -Depth 5
    [IO.File]::WriteAllText((Join-Path $T.Out 'measure.json'), $line)
    $line
    return
  }

  # 1. The first reply
  $m = Get-Metrics
  $p = $m.process
  if ($m.pid -ne $w.Pid) { Add-Failure "metrics came from pid $($m.pid), the window's is $($w.Pid)" }
  if (-not ($m.span_ms -gt 0)) { Add-Failure "the first reply's span is $($m.span_ms) ms" }
  if (-not ($p.cpu_ms -gt 0 -and $p.gui_thread_ms -gt 0 -and $p.gui_thread_cycles -gt 0)) { Add-Failure "the process's CPU: $($p | ConvertTo-Json -Compress)" }
  if (-not ($p.private_bytes -gt 10MB -and $p.handles -gt 50 -and $p.threads -gt 3 -and $p.gdi_objects -gt 0 -and $p.user_objects -gt 0)) { Add-Failure "what the process holds: $($p | ConvertTo-Json -Compress)" }
  if (-not ($m.totals.frames -gt 0)) { Add-Failure "no frames since it started: $($m.totals | ConvertTo-Json -Compress)" }
  $paint = @($m.histograms | Where-Object name -eq 'gui.paint.impl')
  if (-not $paint -or -not ($paint[0].count -gt 0)) { Add-Failure "no gui.paint.impl histogram: $(@($m.histograms.name) -join ', ')" }
  "since it started: $(Format-Metrics $m -Top 2 | Out-String)".TrimEnd()
  "  process: CPU $($p.cpu_ms) ms ($($p.cpu_percent)% of a core), GUI thread $($p.gui_thread_ms) ms, $([Math]::Round($p.private_bytes / 1MB, 1)) MB private, $($p.handles) handles, $($p.threads) threads, $($p.gdi_objects) GDI and $($p.user_objects) USER objects"
  "  histograms: $(@($m.histograms).Count), gui.paint.impl $($paint[0].count) frames, p50 $($paint[0].p50) ms, p95 $($paint[0].p95) ms"

  # 2. At rest
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds 4
  $rest = Get-Metrics -Reset
  "at rest: $(Format-Metrics $rest -Top 5 | Out-String)".TrimEnd()
  if ($rest.totals.frames -ne 0) { Add-Failure "$($rest.totals.frames) frames at rest, asked by $(@($rest.requests | ForEach-Object { "$($_.site) ($($_.frames))" }) -join ', ')" }
  if ($rest.totals.requests -ne 0) { Add-Failure "$($rest.totals.requests) repaints asked for at rest: $(@($rest.requests | ForEach-Object { "$($_.site) ($($_.requests))" }) -join ', ')" }
  if ($rest.totals.paints -ne 0) { Add-Failure "$($rest.totals.paints) paint parts at rest" }

  # 3. A frame: what the shell prints, counted as the state file counts it
  $f0 = Get-QuietFrame
  Get-Metrics -Reset | Out-Null
  Invoke-Cli send-text --pane-id $first --no-paste "echo metrics-frame`r" -Server:$Mux -Check | Out-Null
  Wait-State { param($sw, $s) $s.frame -gt $f0 } -What 'a frame for the echo' | Out-Null
  $f1 = Wait-Quiet 'the echo to settle'
  $echo = Get-Metrics -Reset
  "a frame (echo): $(Format-Metrics $echo | Out-String)".TrimEnd()
  $painted = $f1 - $f0
  if ($echo.totals.frames -ne $painted) { Add-Failure "the state file counted $painted frames for the echo, metrics $($echo.totals.frames)" }
  $win = @($echo.windows)
  if ($win.Count -ne 1 -or $win[0].frames -ne $painted) { Add-Failure "the echo's frames by window: $($echo.windows | ConvertTo-Json -Compress)" }
  $credited = (@($echo.requests) | Measure-Object frames -Sum).Sum
  if (-not $echo.requests -or $credited -lt $echo.totals.frames - $echo.totals.unasked) { Add-Failure "the echo's $($echo.totals.frames) frames ($($echo.totals.unasked) unasked) have $credited credited to requests: $($echo.requests | ConvertTo-Json -Compress)" }
  if ($echo.totals.unasked -ne 0) { Add-Failure "$($echo.totals.unasked) of the echo's frames had nobody asking" }

  # 4. Keys
  Get-Metrics -Reset | Out-Null
  $f0 = Get-QuietFrame
  Send-Chars $w 'abc'
  Wait-State { param($sw, $s) $s.frame -gt $f0 } -What 'the keys'' echo' | Out-Null
  Send-Key $w Escape
  Wait-Quiet 'the keys to settle' | Out-Null
  $keyed = Get-Metrics -Reset
  $down = (ConvertTo-MetricsTable $keyed.messages)['WM_KEYDOWN']
  if (-not $down -or $down.count -lt 4) { Add-Failure "four keys posted, WM_KEYDOWN counted $($down.count)" }
  else { "keys: WM_KEYDOWN $($down.count) times, $($down.total_ms) ms, the longest $($down.max_ms) ms; $($keyed.totals.frames) frames" }

  # 5. A split
  $f0 = Get-QuietFrame
  Get-Metrics -Reset | Out-Null
  $out = @(& $T.Cli control --class $T.Class emit metrics-split 2>&1)
  if ($LASTEXITCODE) { throw "emit metrics-split: $($out -join ' ')" }
  Wait-Change { param($s) @($s.panes).Count -eq 2 } -What 'the split' | Out-Null
  $f1 = Wait-Quiet 'the split to settle'
  $split = Get-Metrics -Reset
  "a split: $(Format-Metrics $split -Top 4 | Out-String)".TrimEnd()
  if (-not ($split.totals.tasks -gt 0)) { Add-Failure 'the split ran no tasks' }
  if ($split.totals.frames -ne $f1 - $f0) { Add-Failure "the state file counted $($f1 - $f0) frames for the split, metrics $($split.totals.frames)" }
  if (-not $split.requests) { Add-Failure 'nobody asked for the split''s frames' }
  $pdus = ConvertTo-MetricsTable $split.mux -Key pdu
  if ($Mux) {
    if (-not $pdus['SplitPane'] -or $pdus['SplitPane'].sent -lt 1) { Add-Failure "no SplitPane sent to the server: $($split.mux | ConvertTo-Json -Compress)" }
    if (-not ($split.totals.pdus_received -gt 0)) { Add-Failure 'nothing received from the server for the split' }
    $bytes = (@($split.mux) | Measure-Object received_bytes -Sum).Sum
    "  the mux client: $($split.totals.pdus_sent) PDUs sent, $($split.totals.pdus_received) received ($bytes bytes)"
  } elseif ($split.mux) { Add-Failure "PDUs counted with no mux client: $($split.mux | ConvertTo-Json -Compress)" }

  # 6. A reset straight after a reset, and the cli
  Get-Metrics -Reset | Out-Null
  $again = Get-Metrics -Reset
  if ($again.totals.frames -ne 0 -or $again.totals.paints -ne 0 -or -not ($again.span_ms -lt 1000)) { Add-Failure "a reset straight after a reset: $($again.totals | ConvertTo-Json -Compress), span $($again.span_ms) ms" }
  $out = @(& $T.Cli control --class $T.Class metrics 2>&1)
  $code = $LASTEXITCODE
  $cli = try { ($out -join "`n") | ConvertFrom-Json -Depth 16 } catch { $null }
  if ($code -ne 0 -or -not $cli.ok -or $cli.pid -ne $w.Pid) { Add-Failure "lbterm control metrics: exit $code, $($out -join ' ')" }
  else { "lbterm control metrics: ok, pid $($cli.pid), $(@($cli.tasks).Count) task sites" }
} -Passed "metrics$(if ($Measure) { ' measured' })$(if ($Mux) { ' (mux)' }): the process, nothing at rest, every frame counted with who asked, keys, a split's tasks$(if ($Mux) { ' and PDUs' }), resets and the cli"
