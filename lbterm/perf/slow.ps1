#Requires -Version 7
# Covers: lbterm-extras/src/slow.rs, config/src/lbterm.rs, promise/src/spawn.rs, window/src/spawn.rs, window/src/os/windows/window.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/paint.rs, lbterm-extras/src/layouts.rs
<#
The slow-frame log (lbterm-extras/src/slow.rs) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig) with lbterm_slow_ms at its default (33, read from
config/src/lbterm.rs) and F9 bound to an
action_callback that holds the GUI thread for 60 ms, as a slow Lua handler would. Keys are posted
messages, never real input.

The GUI's first key probes the keyboard layout's AltGr and dead keys (KeyboardLayoutInfo in
window.rs): about 10 ms of WM_KEYDOWN on an idle PC, 40 ms and more on a busy one or while the
window is still starting. So a Shift press pays for that while the window settles; its line, when
it has one, is shown. The probe is the GUI thread's, once per layout (lbterm-extras/src/layouts.rs),
and the log names each one it makes: `keyboard layout 0x4090c09 probed in 9.4 ms: ...`.

1. At rest, once the window has settled, nothing is logged under lbterm::slow for 6 s.
2. F9: `slow task wezterm-gui/src/termwindow/mod.rs:<n>: <ms> ms`, <n> the line in
   schedule_window_event that spawns a window event's Lua, <ms> at least 50.
3. F9 three times more, in the same 10 s: nothing more at once, then as those 10 s end, with the
   window at rest, `... : 4 times over 33 ms in the last 10 s, max <ms> ms` (the summary's timer).
4. No other message or task went over: the key's own messages and the event's other tasks are
   charged their own time, not the handler's. Paint lines are shown but not failed on: now and
   then a present waits on DWM for 50 ms or more (perf -Measure). Then at rest again for 6 s,
   nothing.
5. F8 opens a second window of the same GUI, on the test screen (SpawnCommandInNewWindow with a
   position), and a Shift goes to it: its first key takes the first window's probe, so the log
   names one probe of the layout, not two.

When the window has settled comes from its state file (lbterm/STATE.md). -Measure instead takes
the window through ordinary things, with every time anything goes over -Threshold ms logged
(LBTERM_SLOW_EVERY, no 10 s windows), and prints what went over in each, by kind and site, with
how often it went over 8, 16, 33 and 50 ms: at rest, the first key (a Shift), typing (posted keys
into pwsh), output scrolling past, a split, the tab rail opening and closing (posted pointer
moves, LBTERM_POSTED_POINTER), a resize, a new tab, a second window (F8) and its first key, and
the layout probes the log names. -Mux puts its tabs in a mux server of their own, as
Lachlan's are (lbterm/MUX.md): the GUI is then that server's client, and what the panes print
reaches it as the client's PDUs.

  pwsh -File lbterm\perf\slow.ps1
  pwsh -File lbterm\perf\slow.ps1 -Measure [-Threshold 1] [-Mux] [-Log <WEZTERM_LOG filter>]
#>
param([string]$Bin, [switch]$Measure, [int]$Threshold = 1, [switch]$Mux, [string]$Log)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'slow' -Bin $Bin
# In a folder of its own, with reloading off (New-TestConfig): LBTerm reloads on any change in
# its config's folder
$cfg = Join-Path $T.Out 'config\slow.lua'
$sock = $T.Sock
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
# Where F8's second window opens: beside the first, on the same screen (--position's numbers, from
# the top-left of the whole desktop)
$posX, $posY = (& (Get-Module harness) { $TestWindowPosition }) -split ',' | ForEach-Object { [int]$_ }

# The line that spawns a window event's Lua: what an action_callback's task is named by
$modRs = Join-Path $T.Repo 'wezterm-gui\src\termwindow\mod.rs'
$src = Get-Content $modRs
$fn = ($src | Select-String -Pattern 'fn schedule_window_event' | Select-Object -First 1).LineNumber
$spawnLine = ($src | Select-Object -Skip $fn | Select-String -Pattern 'promise::spawn::spawn\(config::with_lua_config_on_main_thread' |
  Select-Object -First 1).LineNumber + $fn
$site = "wezterm-gui/src/termwindow/mod.rs:$spawnLine"

# The check runs at lbterm_slow_ms's default; -Measure at -Threshold
$slowLua = if ($Measure) { "config.lbterm_slow_ms = $Threshold" } else { '' }
$default = [int][regex]::Match((Get-Content -Raw (Join-Path $T.Repo 'config\src\lbterm.rs')),
  'fn default_slow_ms\(\) -> u64 \{\s*(\d+)').Groups[1].Value
if (-not $default) { throw "no default_slow_ms in config\src\lbterm.rs" }
New-TestConfig -Path $cfg -Lua @"
$slowLua
config.initial_cols = 120
config.initial_rows = 30
config.keys = config.keys or {}
-- Holds the GUI thread 60 ms, as a slow Lua handler would. MSVC's clock() is wall time
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action_callback(function(window, pane)
  local t = os.clock()
  while os.clock() - t < 0.060 do end
end) })
-- A second window of the same GUI, on the test screen
table.insert(config.keys, { key = 'F8', mods = 'NONE', action = wezterm.action.SpawnCommandInNewWindow {
  args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  position = { x = $($posX + 60), y = $($posY + 60), origin = 'ScreenCoordinateSystem' },
} })
$(if ($Mux) { @"
config.unix_domains = { {
  name = 'slowmux',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@ })
"@

$environment = @{}
if ($Measure) { $environment.LBTERM_SLOW_EVERY = '1' }

# The lbterm::slow lines logged so far: time, kind, site, ms (a first line) or the whole message
function Get-Slow {
  if (-not (Test-Path $w.Log)) { return @() }
  @(Get-Content $w.Log | ForEach-Object {
    if ($_ -match '^(\S+)\s+WARN\s+lbterm::slow\s+> (slow (\w+) (\S+): (.+))$') {
      $rest = $Matches[5]
      [pscustomobject]@{
        At = $Matches[1]; Message = $Matches[2]; Kind = $Matches[3]; Site = $Matches[4]
        Ms = if ($rest -match '^([\d.]+) ms$') { [double]$Matches[1] } else { $null }
        Summary = if ($rest -match '^(\d+) times over (\d+) ms in the last 10 s, max ([\d.]+) ms$') {
          [pscustomobject]@{ Count = [int]$Matches[1]; Threshold = [int]$Matches[2]; Max = [double]$Matches[3] }
        } else { $null }
      }
    }
  })
}

# The keyboard layout probes the log names (window.rs, KeyboardLayoutInfo::update)
function Get-Probes {
  if (-not (Test-Path $w.Log)) { return @() }
  @(Select-String -Path $w.Log -Pattern '> (keyboard layout \S+ probed in [\d.]+ ms.*)$' | ForEach-Object { $_.Matches[0].Groups[1].Value })
}

# F8 in the first window: the second window's handle, once it's there and the state file has it
# drawn and settled
function Open-SecondWindow {
  Send-Key $w F8
  $clock = [Diagnostics.Stopwatch]::StartNew()
  do {
    Start-Sleep -Milliseconds 50
    $found = @(Get-ProcessWindows $w | Where-Object { $_ -ne $w.Handle })
  } while (-not $found -and $clock.Elapsed.TotalSeconds -lt 10)
  if (-not $found) { throw 'F8 opened no second window' }
  $second = [long]$found[0]
  Wait-State { param($entry, $snap) @($snap.windows | Where-Object { [long]$_.hwnd -eq $second -and $_.frame -gt 0 -and $_.settled }).Count } -What 'the second window to draw and settle' | Out-Null
  $found[0]
}

Invoke-Test {
  $open = @{ ConfigFile = $cfg; Environment = $environment; Program = (New-PwshProgram); PostedPointer = [bool]$Measure; Log = $Log }
  if ($Mux) { $open.Domain = 'slowmux'; $open.Socket = $sock }
  $w = Open-TestWindow @open
  if (-not $Measure) {
    # The first key's layout probe, paid while settling (VK_SHIFT)
    Wait-State { param($s) $s.frame -gt 0 } -What 'the first frame' | Out-Null
    Send-Key $w Shift
  }
  # Settled: started, the shell's first prompt drawn (two captures alike), the strip's notices gone
  Wait-Change { param($s) $s.panes.Count -ge 1 } -What 'the window to settle' -TimeoutSec 20 | Out-Null
  (Wait-Still $w -NoThrow).Dispose()
  Wait-Settled -What 'the strip''s notices to go' | Out-Null
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  $bad = Select-String -Path $w.Log -Pattern 'Configuration Error.*' -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($bad) { throw "the test config didn't load: $($bad.Matches[0].Value)" }

  if ($Measure) {
    $px = { param($v) ConvertTo-Px $v $w.Scale }
    $cr = Get-ClientRect $w
    $phases = [Collections.Generic.List[object]]::new()
    $phase = { param($name) $phases.Add([pscustomobject]@{ Name = $name; From = (Get-Slow).Count }) }

    & $phase 'startup'
    $phases[0].From = 0
    & $phase 'rest (6 s)'
    Start-Sleep 6
    # A Shift types nothing, but meets the keyboard layout's probe all the same
    & $phase 'first key (Shift)'
    Send-Key $w Shift
    Start-Sleep 1
    & $phase 'typing (60 keys, 80 ms apart)'
    $word = 'echo the quick brown fox jumps over the lazy dog again and again'
    foreach ($c in $word.ToCharArray()[0..59]) {
      Send-Key $w $(if ($c -eq ' ') { 'Space' } else { [string]$c }); Start-Sleep -Milliseconds 80
    }
    Send-Key $w Enter
    Start-Sleep 2
    & $phase 'output (2000 lines scrolling past)'
    Invoke-Cli send-text --pane-id $first --no-paste "1..2000 | ForEach-Object { `"line `$_ of the output scrolling past, long enough to wrap the odd time: `$('x' * (`$_ % 90))`" }`r" -Server:$Mux | Out-Null
    Start-Sleep 4
    & $phase 'split'
    $second = [int](Invoke-Cli split-pane --pane-id $first --right -Program (New-PwshProgram) -Server:$Mux)
    Start-Sleep 3
    & $phase 'tab rail open and close'
    Send-Mouse $w Move (& $px 24) (& $px 160)
    Start-Sleep 2
    Send-Mouse $w Move ([int]($cr.Right * 0.7)) ([int]($cr.Bottom / 2))
    Start-Sleep 2
    Send-Leave $w
    Start-Sleep 1
    & $phase 'resize (narrower, then back)'
    $wr = Get-WindowRect $w
    $ww = $wr.Right - $wr.Left; $hh = $wr.Bottom - $wr.Top
    Set-WindowSize $w ($ww - (& $px 120)) $hh
    Start-Sleep 2
    Set-WindowSize $w $ww $hh
    Start-Sleep 2
    & $phase 'new tab'
    New-Tab -From $second -Program (New-PwshProgram) -Server:$Mux | Out-Null
    Start-Sleep 3
    & $phase 'second window (F8)'
    $h2 = Open-SecondWindow
    & $phase 'second window, first key (Shift)'
    Send-Key $h2 Shift
    Start-Sleep 1
    & $phase 'rest again (6 s)'
    Start-Sleep 6
    $all = Get-Slow
    $phases.Add([pscustomobject]@{ Name = 'end'; From = $all.Count })
    for ($i = 0; $i -lt $phases.Count - 1; $i++) {
      $p = $phases[$i]
      $lines = @($all | Select-Object -Skip $p.From -First ($phases[$i + 1].From - $p.From))
      "== $($p.Name): $($lines.Count) over $Threshold ms"
      $lines | Group-Object Kind, Site | Sort-Object { ($_.Group | Measure-Object Ms -Maximum).Maximum } -Descending | ForEach-Object {
        $ms = @($_.Group.Ms)
        $stat = $ms | Measure-Object -Maximum -Average
        $over = foreach ($limit in 8, 16, 33, 50) { "$(@($ms | Where-Object { $_ -gt $limit }).Count) >$limit" }
        '  {0,-58} {1,4}x  avg {2,5:N1}  max {3,6:N1} ms   {4}' -f $_.Name, $ms.Count, $stat.Average, $stat.Maximum, ($over -join ', ')
      }
    }
    $probes = Get-Probes
    "== keyboard layout probes: $($probes.Count)"
    $probes | ForEach-Object { "  $_" }
    return
  }

  # What starting and the first key logged
  Get-Slow | ForEach-Object { "settling: $($_.Message)" }

  # 1. At rest: nothing
  $mark = (Get-Slow).Count
  Start-Sleep 6
  $rest = @(Get-Slow | Select-Object -Skip $mark)
  if ($rest) { Add-Failure "logged at rest: $($rest.Message -join ' | ')" }

  # 2. F9: the task that held the thread, by where it was spawned
  $mark = (Get-Slow).Count
  Send-Key $w F9
  Start-Sleep -Milliseconds 800
  $now = @(Get-Slow | Select-Object -Skip $mark)
  $now.Message
  $held = @($now | Where-Object { $_.Kind -eq 'task' -and $_.Site -eq $site -and $_.Ms })
  if (-not $held) { Add-Failure "no 'slow task ${site}: <ms> ms' after F9" }
  elseif ($held[0].Ms -lt 50) { Add-Failure "F9 held the thread 60 ms, but the line says $($held[0].Ms) ms" }

  # 3. Three more in the same 10 s: counted, then summed up when they end
  foreach ($i in 1..3) { Send-Key $w F9; Start-Sleep -Milliseconds 400 }
  Start-Sleep 1
  $soon = @(Get-Slow | Select-Object -Skip $mark | Where-Object Site -eq $site)
  if ($soon.Count -ne 1) { Add-Failure "$($soon.Count) lines for $site within its 10 s, not 1: $($soon.Message -join ' | ')" }
  Start-Sleep 9
  $after = @(Get-Slow | Select-Object -Skip $mark)
  $after | Select-Object -Skip $now.Count | ForEach-Object Message
  $sum = @($after | Where-Object { $_.Site -eq $site -and $_.Summary })
  if (-not $sum) { Add-Failure "no summary for $site as its 10 s ended" }
  else {
    $want = "4 times over $default ms"
    if ($sum[0].Summary.Count -ne 4 -or $sum[0].Summary.Threshold -ne $default) { Add-Failure "the summary says '$($sum[0].Message)', wanted $want" }
    if ($sum[0].Summary.Max -lt 50) { Add-Failure "the summary's max is $($sum[0].Summary.Max) ms" }
  }

  # 4. No other message or task went over (a present can wait on DWM: shown, not failed on), and
  # at rest again nothing more
  $others = @($after | Where-Object { $_.Site -ne $site -and $_.Kind -ne 'paint' })
  if ($others) { Add-Failure "other lines: $($others.Message -join ' | ')" }
  $after | Where-Object Kind -eq 'paint' | ForEach-Object { "noted: $($_.Message)" }
  $mark = (Get-Slow).Count
  Start-Sleep 6
  $rest = @(Get-Slow | Select-Object -Skip $mark)
  if ($rest) { Add-Failure "logged at rest after: $($rest.Message -join ' | ')" }

  # 5. A second window's first key: the first window's probe, not one of its own
  $h2 = Open-SecondWindow
  Send-Key $h2 Shift
  Start-Sleep 1
  $probes = Get-Probes
  $probes | ForEach-Object { "probe: $_" }
  if ($probes.Count -ne 1) { Add-Failure "$($probes.Count) probes of the keyboard layout after two windows' first keys, not 1" }
} -Passed "slow-frame log passed: $site named, summed up after 10 s, nothing at rest, one keyboard layout probe for two windows"
