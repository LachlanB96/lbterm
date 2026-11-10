#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/state.rs, window/src/os/windows/work.rs, window/src/os/windows/window.rs, lbterm-extras/src/work.rs
<#
A window's loops (lbterm/COMPOSITOR.md, "Loops"): the animations DWM plays for as long as what
they show is there, a waiting pad's arc and the Conductor's turning hues and the light going round
them. One in a window's visual tree keeps DWM composing a frame every refresh of the fastest
screen, minimized or not, so they hold still while the window can't be seen (minimized; for
Lachlan's windows the session locked or the screens off too, which a test window with a state file
doesn't go by) and start again once it can.

A test window in his look (New-TestConfig) with the Conductor's pad accepting most against a hub
played here thinking about one tab's session and acting on another's (three outlines, two with
their light), and its first tab's program done but waiting on a task of its own (its pad's arc):

1. The state file's loops say all three (rail.spinner, conductor.outline, conductor.light); with
   the workstation unlocked, two captures 350 ms apart differ on the rail, where DWM turns them.
2. Minimized (SW_SHOWMINNOACTIVE): within 3 s a frame says no loops, its rail still with the arc's
   pad waiting and both outlines (still, not gone); over the next 4 s it paints a frame a second at
   most (the rail's ages tick), and none of them sets a loop going.
3. Restored without being activated: within 3 s a frame says all three again, and unlocked the
   rail turns again.

-Measure then says what DWM did (harness's Measure-DwmSpan: the compositor clock's frames a
second, the share of 50 ms steps it went without one, dwm.exe's CPU and GPU in ms a second) over
-Seconds spans, -Rounds rounds of four interleaved: the loops shown, the window minimized with
them, none (read only, no focus, no task), none minimized. DWM's numbers are the whole desktop's,
so another session's test windows or Lachlan's own move them: each state's median is said, and
nothing is judged on them. cost.json in target\perf\dwm-loops has every span.

  pwsh -File lbterm\perf\dwm-loops.ps1 [-Mux] [-Measure [-Rounds 3] [-Seconds 10]] [-Bin <build>]
#>
param([string]$Bin, [switch]$Mux, [switch]$Measure, [int]$Rounds = 3, [double]$Seconds = 10)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
$T = Start-Test ('dwm-loops' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$cfg = Join-Path $T.Out 'config\dwm-loops.lua'
$homeDir = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force (Split-Path $cfg), $homeDir | Out-Null

# The Conductor's pad: accepting most for the loops, read only for none
$button = Join-Path $T.Out 'button.json'
$most = '{ "state": "on", "mode": "most", "tooltip": "Conductor is on (accept most)" }'
$read = '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)" }'
[IO.File]::WriteAllText($button, $most)
$hub = Start-PlayedPipe -Name "lbterm-perf-dwm-loops-hub-$PID" -Subscribe '"lbterm.subscribe"'
$sids = 'b0d6e791-0000-4000-8000-000000000071', 'b0d6e792-0000-4000-8000-000000000072'
$domain = if ($Mux) {
  @"
config.unix_domains = { {
  name = 'dwmloops',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 100
config.initial_rows = 30
config.window_close_confirmation = 'NeverPrompt'
config.default_cwd = [[$($T.Out)]]
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
rawset(config, 'lbterm_projects', nil)
rawset(config, 'lbterm_conductor', { state_file = [[$button]], hub_pipe = [[\\.\pipe\$($hub.Name)]], menu = {} })
$domain
"@

$all = @('rail.spinner', 'conductor.outline', 'conductor.light')
$loopsOf = { param($s) @($s.loops | Where-Object { $_ }) }
$median = { param($xs) $v = @($xs | Where-Object { $null -ne $_ } | Sort-Object); if ($v) { $v[[int][Math]::Floor(($v.Count - 1) / 2)] } else { $null } }
$spans = [Collections.Generic.List[object]]::new()

Invoke-Test {
  $open = @{ ConfigFile = $cfg; UserProfile = $homeDir; TimeoutSec = 30 }
  if ($Mux) { $open.Domain = 'dwmloops'; $open.Socket = $T.Sock } else { $open.Program = New-PwshProgram }
  $w = Open-TestWindow @open
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 20)) { throw "the test's mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) @($s.panes).Count -ge 1 -and $s.rail } -What 'the first pane and the rail' -TimeoutSec 30 | Out-Null
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  foreach ($sid in $sids) { New-Tab -From $first -NoActivate -Program (New-ClaudeSessionProgram $sid) -Server:$Mux | Out-Null }
  Wait-State { param($s) @($s.tabs).Count -eq 3 } -What 'three tabs' | Out-Null
  if (-not (Wait-Until { $hub.Subscribers -ge 1 -and @(Get-PlayedLines $hub 'lbterm.panes' | Select-Object -Last 1 | ForEach-Object { @($_.panes).Count }) -ge 2 } -TimeoutSec 20)) {
    throw "LBTerm never subscribed to the played hub with both sessions' panes: $($hub.Heard() -join ' | ')"
  }
  Invoke-Cli send-text --pane-id $first --no-paste "function prompt { 'PS> ' }; `$e = [char]27; cls`r" -Server:$Mux -Check | Out-Null
  Start-Sleep -Milliseconds 800

  # The loops on (the pad accepting most, two rows ringed, the first tab's program waiting on a
  # task of its own) or none
  $focusOn = '{"type":"conductor.focus","sessions":[{"sessionId":"' + $sids[0] + '","doing":"thinking"},{"sessionId":"' + $sids[1] + '","doing":"acting"}]}'
  $focusOff = '{"type":"conductor.focus","sessions":[]}'
  $shown = { param($s) $r = @($s.rail.rows); $s.rail.conductor.level -eq 'most' -and @($r | Where-Object conductor -eq 'thinking').Count -eq 1 -and @($r | Where-Object conductor -eq 'acting').Count -eq 1 -and $r[0].waiting -ge 1 }
  $none = { param($s) $r = @($s.rail.rows); $s.rail.conductor.level -eq 'read' -and -not @($r | Where-Object { $_.conductor }) -and -not ($r[0].waiting -ge 1) }
  $set = {
    param([bool]$on)
    if ($on) {
      [IO.File]::WriteAllText($button, $most)
      Send-PlayedLine $hub $focusOn | Out-Null
      Invoke-Cli send-text --pane-id $first --no-paste "[Console]::Write(`"`$e]7501;state=done:app=claude`$e\`$e]7501;state=working:id=bg/b1`$e\`")`r" -Server:$Mux -Check | Out-Null
      Wait-State $shown -What 'the outlines and the arc' -TimeoutSec 15 | Out-Null
    } else {
      [IO.File]::WriteAllText($button, $read)
      Send-PlayedLine $hub $focusOff | Out-Null
      Invoke-Cli send-text --pane-id $first --no-paste "[Console]::Write(`"`$e]7501;state=clear:id=bg`$e\`")`r" -Server:$Mux -Check | Out-Null
      Wait-State $none -What 'no outline or arc' -TimeoutSec 15 | Out-Null
    }
    Get-QuietFrame -QuietMs 1000 -TimeoutSec 20 | Out-Null
  }
  $locked = { Test-WorkstationLocked }
  # Two captures 350 ms apart: what changed on the rail, where DWM turns the arc and the hues
  $railTurns = {
    param([string]$name)
    $a = Get-Shot $w; Start-Sleep -Milliseconds 350; $b = Get-Shot $w
    Save-Shot $a "$name"; Save-Shot $b "$name-later"
    $n = [LbtermE2E.Px]::Differing($a, $b, (Get-Columns 0 ((ConvertTo-Px 80 $w.Scale) - 1) -Of $a))
    $a.Dispose(); $b.Dispose()
    $n
  }

  # 1. Shown: all three loops play
  & $set $true
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $missing = @($all | Where-Object { $_ -notin (& $loopsOf $s) })
  if ($missing) { Add-Failure "shown: the loops say $((& $loopsOf $s) -join ', '), not $($missing -join ', ')" } else { "shown: loops $((& $loopsOf $s) -join ', ')" }
  if (& $locked) { $T.Notes.Add('the workstation is locked: the rail''s turning isn''t captured') }
  else {
    $n = & $railTurns 'shown'
    if ($n -eq 0) { Add-Failure 'shown: nothing on the rail turned in 350 ms' } else { "shown: $n pixels of the rail changed in 350 ms" }
  }

  # 2. Minimized: still, and nothing painted after
  if (-not (Set-WindowMinimized $w)) { throw 'the window didn''t minimize' }
  try {
    $s = Wait-State { param($s) -not (& $loopsOf $s) } -What 'a frame with no loops' -TimeoutSec 3
    "minimized: a frame with no loops (frame $((Read-State).frame))"
    if (-not (& $shown $s)) { Add-Failure "minimized: the rail no longer shows the outlines and the waiting pad, held still: $(Format-StateWindow $s)" }
  } catch {
    $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    Add-Failure "minimized: no frame within 3 s said its loops were still; the last says $((& $loopsOf $s) -join ', ')"
  }
  # The rail's ages still tick (each second for a row's first minute), at most a frame a second
  # while it can't be seen (lbterm_extras::work::UNSEEN_PERIOD), and none of them sets them going
  $recorder = Start-FrameRecorder -Window $w
  try {
    $f0 = [long](Read-State).frame
    Start-Sleep -Seconds 4
    $recorder.Look()
    $seen = @($recorder.Take() | Where-Object { $_.Frame -gt $f0 } | ForEach-Object {
      $st = try { $_.Text | ConvertFrom-Json -Depth 32 } catch { $null }
      if ($st) { [pscustomobject]@{ Frame = $_.Frame; Loops = @(& $loopsOf (Get-StateWindow $st -Hwnd ([long]$w.Handle))) } }
    })
  } finally { $recorder.Dispose() }
  $playing = @($seen | Where-Object { $_.Loops })
  if ($seen.Count -gt 5) { Add-Failure "minimized: $($seen.Count) frames in 4 s, where it paints a frame a second at most" }
  if ($playing) { Add-Failure "minimized: $($playing.Count) of its $($seen.Count) frames in 4 s set loops going: $(@($playing | Select-Object -First 3 | ForEach-Object { "frame $($_.Frame) $($_.Loops -join ', ')" }) -join '; ')" }
  else { "minimized: $($seen.Count) frames in 4 s (the ages ticking), none with a loop" }

  # 3. Restored: playing again
  Set-WindowMinimized $w -Restore | Out-Null
  try {
    $s = Wait-State { param($s) -not @($all | Where-Object { $_ -notin (& $loopsOf $s) }) } -What 'a frame with every loop again' -TimeoutSec 3
    "restored: loops $((& $loopsOf $s) -join ', ')"
  } catch {
    $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    Add-Failure "restored: no frame within 3 s said its loops played again; the last says $((& $loopsOf $s) -join ', ')"
  }
  Get-QuietFrame -QuietMs 700 -TimeoutSec 10 | Out-Null
  if (-not (& $locked)) {
    $n = & $railTurns 'restored'
    if ($n -eq 0) { Add-Failure 'restored: nothing on the rail turned in 350 ms' } else { "restored: $n pixels of the rail changed in 350 ms" }
  }

  if ($Measure) {
    $meter = New-DwmMeter
    $rate = Get-CycleRate
    ''
    "what DWM did: $Rounds rounds of four $Seconds s spans, interleaved"
    for ($r = 1; $r -le $Rounds; $r++) {
      foreach ($state in 'shown', 'shown minimized', 'none', 'none minimized') {
        if ([LbtermE2E.Show]::IsIconic([IntPtr](Resolve-Handle $w))) {
          Set-WindowMinimized $w -Restore | Out-Null
          Get-QuietFrame -QuietMs 700 -TimeoutSec 10 | Out-Null
        }
        & $set ($state -like 'shown*')
        if ($state -like '*minimized') {
          Set-WindowMinimized $w | Out-Null
          Start-Sleep -Milliseconds 1500
        }
        $loops = @(& $loopsOf (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)))
        $f0 = [long](Read-State).frame
        $c0 = Get-CpuSnapshot -Window $w
        $d = Measure-DwmSpan $meter $Seconds
        $c1 = Get-CpuSnapshot -Window $w
        $f1 = [long](Read-State).frame
        $row = [pscustomobject]@{
          round = $r; state = $state; loops = $loops; at = (Get-Date).ToString('HH:mm:ss')
          dwm_frames = $d.frames; dwm_idle = $d.idle; dwm_cpu_ms = $d.cpu_ms; dwm_gpu_ms = $d.gpu_ms
          gui_frames = $f1 - $f0; gui_cpu_ms = [Math]::Round(($c1.Cycles - $c0.Cycles) / $rate / $d.seconds, 2)
        }
        $spans.Add($row)
        "  round $r, $($state): DWM $($d.frames) frames/s, idle $($d.idle), CPU $($d.cpu_ms) ms/s, GPU $($d.gpu_ms) ms/s; the GUI $($row.gui_frames) frames, $($row.gui_cpu_ms) ms/s; loops $(if ($loops) { $loops -join ', ' } else { 'none' })"
      }
    }
    if ([LbtermE2E.Show]::IsIconic([IntPtr](Resolve-Handle $w))) { Set-WindowMinimized $w -Restore | Out-Null }
    ''
    foreach ($g in $spans | Group-Object state) {
      "{0,-16} DWM frames/s {1,6}  idle {2,6}  CPU ms/s {3,7}  GPU ms/s {4,6}  the GUI's frames {5}" -f $g.Name, (& $median $g.Group.dwm_frames), (& $median $g.Group.dwm_idle), (& $median $g.Group.dwm_cpu_ms), (& $median $g.Group.dwm_gpu_ms), (($g.Group.gui_frames) -join ', ')
    }
    [IO.File]::WriteAllText((Join-Path $T.Out 'cost.json'), (@{ build = (Get-BuildVersion $T.Bin); seconds = $Seconds; rounds = $Rounds; spans = @($spans) } | ConvertTo-Json -Depth 6))
  }
} -Passed 'dwm-loops: the loops play while the window can be seen, hold still minimized and play again restored'
