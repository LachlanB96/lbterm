#Requires -Version 7
# Variant: -Mux
# Covers: lbterm-extras/src/present.rs, wezterm-gui/src/present.rs, wezterm-gui/src/termwindow/budget.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/counters.rs, lbterm-extras/src/budget.rs, lbterm-extras/src/state.rs
<#
What's hard to time (lbterm/PERF.md, "What's hard to time"; lbterm-extras/src/present.rs,
wezterm-gui/src/present.rs) end to end, in a test window that looks like Lachlan's LBTerm
(New-TestConfig: WebGpu with the compositor, max_fps 120, the rail and his motion), read through
the control pipe's `metrics` (its `present` part), the timing to the screen turned on as it starts
(LBTERM_PRESENT_TIMING; off, which Lachlan's window is unless asked, nothing is watched, looked at
or walked: checked first). Waits are on the state file; keys are posted messages, never real
input. -Mux puts its tabs in a mux server of their own, as his are.

0. Off costs nothing: `lbterm control metrics --present off`, keys and a split, and nothing is
   watched or walked (present.cost all zeros); then on again.

1. Keys echoed at a pwsh prompt, one at a time: each key's frame seen on screen by DXGI's frame
   statistics (`available.frame_statistics`), its three times in order (to its frame's paint, to
   its presents, to the refresh it went up at), the refresh no earlier than the presents and no
   later than a few refreshes after, and the time the GUI gives it (`screen_unix_ms`) after the
   key was posted and before the test read the reply. Every frame's own paint is timed to the
   screen too.
2. Motion: a split (F7), its close (F3), a new tab (F8) and a switch back (F4), the rail opening on
   the pointer's rest. Each opens an episode the compositor clock's statistics are walked over
   (`available.compositor_clock`): measured, with refreshes, its names (pane.layout for a split,
   rail.edge for the rail), refreshes no fewer than presents, and when its first frame went up.
   How many refreshes went without a frame is printed, not held to anything here (budgets.ps1's
   `missed`), since another program's load can take one.
3. Latency under load: a writer at full speed in a second pane, keys echoed in the first; their
   times printed beside the keys' without it (budgets.ps1's key-echo-load holds them).
4. What it costs: each look at a swapchain's statistics and each compositor frame walked, from the
   reply's `cost`.

A window others hide shows its frames to nobody, so none is seen going up and its motion isn't
walked: what that leaves unchecked is said in result.json's notes, and the pass line says only
what was checked. When every key's window was covered, or Windows' frame statistics never said a
present of the window's went up though nothing covered it (as on this PC on 2026-11-09, every
read all zeros: lbterm/PERF.md, finding 98), the run is unjudged (Set-Unjudged), neither a pass
on nothing (PITFALLS 283) nor the build's failure; some keys seen and some lost still fails.

  pwsh -File lbterm\perf\present.ps1 [-Mux] [-Bin <build>]
#>
param([string]$Bin, [switch]$Mux, [int]$Keys = 12)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$name = 'present' + $(if ($Mux) { '-mux' } else { '' })
$T = Start-Test $name -Bin $Bin
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\present.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 28
config.keys = config.keys or {}
local pwsh = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
for _, k in ipairs({
  { key = 'F8', mods = 'NONE', action = wezterm.action.SpawnCommandInNewTab { args = pwsh } },
  { key = 'F7', mods = 'NONE', action = wezterm.action.SplitHorizontal { args = pwsh } },
  { key = 'F6', mods = 'NONE', action = wezterm.action.CloseCurrentTab { confirm = false } },
  { key = 'F4', mods = 'NONE', action = wezterm.action.ActivateTabRelative(1) },
  { key = 'F3', mods = 'NONE', action = wezterm.action.CloseCurrentPane { confirm = false } },
}) do table.insert(config.keys, k) end
$(if ($Mux) { @"
config.unix_domains = { {
  name = 'presentmux',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@ })
"@

function Get-Win { Get-StateWindow (Read-State) -Hwnd ([long](Get-Test).Current.Handle) }
# A note in result.json, said as it's made
function Add-Note([string]$Note) { (Get-Test).Notes.Add($Note); "note: $Note" }
function Get-ActivePane($sw) { @($sw.panes | Where-Object active)[0] }
function Get-Drawn($p) { if ($null -ne $p.drawn_cursor) { $p.drawn_cursor } else { $p.cursor } }

# A key posted at the prompt and its echo drawn, then the window quiet; the time it was posted
function Send-EchoedKey([string]$Key = 'x') {
  $c0 = Get-Drawn (Get-ActivePane (Get-Win))
  $t0 = Get-UnixMs
  Send-Key $w $Key
  Wait-State { param($sw) $c = Get-Drawn (Get-ActivePane $sw); $c.x -ne $c0.x -or $c.y -ne $c0.y } -What 'the echo' -TimeoutSec 10 | Out-Null
  Start-Sleep -Milliseconds 150
  $t0
}

# Keys typed one at a time, each read back from metrics: the times the GUI gives it, every key's
# row (Key is null for one the GUI didn't see on screen)
function Measure-Keys([int]$N) {
  for ($i = 0; $i -lt $N; $i++) {
    if ($i -and $i % 8 -eq 0) { Send-Key $w Escape; Start-Sleep -Milliseconds 300 }
    Get-Metrics -Reset | Out-Null
    $t0 = Send-EchoedKey
    $m = Get-Metrics
    $read = Get-UnixMs
    $k = @($m.present.actions | Where-Object action -eq 'key')[0]
    [pscustomobject]@{ I = $i; T0 = $t0; Read = $read; Key = $k; Available = $m.present.available }
  }
}

# What the rows say, a line a key
function Format-Keys([object[]]$Rows, [string]$What) {
  $seen = @($Rows | Where-Object { $_.Key -and $_.Key.last -and $null -ne $_.Key.last.screen_ms })
  "${What}: $($seen.Count) of $(@($Rows).Count) keys seen on screen; overtaken $(@($Rows | ForEach-Object { [int]$_.Key.overtaken } | Measure-Object -Sum).Sum), lost $(@($Rows | ForEach-Object { [int]$_.Key.lost } | Measure-Object -Sum).Sum); the frame statistics: $(@($Rows)[-1].Available | ConvertTo-Json -Compress)"
  foreach ($r in @($Rows | Where-Object { $_.Key -and $_.Key.last })) {
    $l = $r.Key.last
    $up = if ($null -ne $l.screen_ms) { ", on screen $($l.screen_ms) ms; $([Math]::Round($l.screen_unix_ms - $r.T0, 2)) ms after it was posted" } else { ', not seen going up' }
    "  key $($r.I): from its arrival, paint $($l.paint_ms) ms, presented $($l.present_ms) ms$up"
  }
}

Invoke-Test {
  $open = @{ ConfigFile = $cfg; Program = (New-PwshProgram); PostedPointer = $true; TimeoutSec = 30; Environment = @{ LBTERM_PRESENT_TIMING = '1' } }
  if ($Mux) { $open.Domain = 'presentmux'; $open.Socket = $sock }
  $script:w = Open-TestWindow @open
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }
  Wait-Change { param($s) @($s.panes).Count -ge 1 -and $s.rail } -What 'the first pane and the rail' -TimeoutSec 30 | Out-Null
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  Invoke-Cli send-text --pane-id $first --no-paste "function prompt { 'PS> ' }`r" -Server:$Mux -Check | Out-Null
  Wait-State { param($sw) (Get-ActivePane $sw).cursor.x -eq 4 } -What 'the short prompt' -TimeoutSec 20 | Out-Null
  Send-Focus $w
  Wait-Settled -What 'the shell to start' -TimeoutSec 20 | Out-Null
  Start-Sleep -Milliseconds 500
  # What it draws reaches the screen only where it can be seen: moved where nothing covers it, if
  # there's such a place on its screen (never raised over anything, never activated)
  $before = (Get-WindowCover $w).Said
  $clear = Move-TestWindowClear $w
  $cover = Get-WindowCover $w
  "over the window: $before; $(if ($clear) { "moved clear, now $($cover.Said)" } else { 'nowhere clear on its screen' })"
  $hidden = $cover.Covered -ge 0.95
  $checked = [Collections.Generic.List[string]]::new()
  $unchecked = [Collections.Generic.List[string]]::new()

  # 0. Off, nothing is done: no look, no walk, no watch
  $out = @(& $T.Cli control --class $T.Class metrics --present off 2>&1)
  if ($LASTEXITCODE) { throw "metrics --present off: $($out -join ' ')" }
  Get-Metrics -Reset | Out-Null
  $x0 = Send-EchoedKey
  Send-Key $w F7
  Wait-Change { param($s) @($s.panes).Count -eq 2 } -What 'a split with the timing off' -TimeoutSec 20 | Out-Null
  Send-Key $w F3
  Wait-Change { param($s) @($s.panes).Count -eq 1 } -What 'its close' -TimeoutSec 20 | Out-Null
  $offM = (Get-Metrics).present
  $offCost = $offM.cost
  "off: on $($offM.on); $($offCost.polls) looks, $($offCost.walks) walks, $($offCost.covers) cover looks, $(@($offM.actions).Count) actions, $($offM.motion.episodes) episodes"
  if ($offM.on) { Add-Failure 'metrics --present off left it on' }
  if ([int]$offCost.polls -or [int]$offCost.walks -or [int]$offCost.covers -or @($offM.actions).Count -or [int]$offM.motion.episodes) {
    Add-Failure "with the timing off it still watched or walked: $($offM | ConvertTo-Json -Compress -Depth 6)"
  }
  $out = @(& $T.Cli control --class $T.Class metrics --present on 2>&1)
  if ($LASTEXITCODE) { throw "metrics --present on: $($out -join ' ')" }
  $checked.Add('nothing watched or walked with it off')

  # 1. Keys to the screen
  $rows = @(Measure-Keys $Keys)
  Format-Keys $rows 'keys'
  $seen = @($rows | Where-Object { $_.Key -and $_.Key.last -and $null -ne $_.Key.last.screen_ms })
  $avail = $rows[-1].Available
  # Keys the GUI found its window hidden for went up to nobody: counted, not watched
  $hid = @($rows | Where-Object { $_.Key -and [int]$_.Key.hidden -gt 0 })
  $lost = @($rows | Where-Object { $_.Key -and [int]$_.Key.lost -gt 0 })
  if ($hid.Count) { Add-Note ("$($hid.Count) of $Keys keys went to a window others hid (over it: $($cover.Said)), so their times to the screen couldn't be checked") }
  $overtaken = (@($rows | ForEach-Object { [int]$_.Key.overtaken }) | Measure-Object -Sum).Sum
  # Nothing to time to the screen by (Lachlan's E5 of 2026-11-09): every key's window covered, or
  # Windows' frame statistics never saying one of the window's presents went up though nothing
  # covered it, which it did on this PC for every swapchain on 2026-11-09 (PERF.md, finding 98).
  # Unjudged, not a pass on nothing (PITFALLS 283) and not the build's failure. Some keys seen and
  # some lost is still a failure
  if ($hid.Count -eq @($rows).Count) {
    Set-Unjudged "every key's window was covered (over it: $($cover.Said)), so nothing was timed to the screen"
  } elseif ($seen.Count -eq 0 -and $overtaken -eq 0 -and $hid.Count -eq 0 -and $lost.Count) {
    Set-Unjudged "Windows' frame statistics said none of the window's presents went up though nothing covered it, so nothing could be timed to the screen (PERF.md, finding 98)"
  } else {
    if ($lost.Count) { Add-Failure "$($lost.Count) of $Keys keys were watched and never seen on screen" }
    if ($seen.Count + $hid.Count -lt [Math]::Ceiling($Keys * 0.75)) { Add-Failure "only $($seen.Count) of $Keys keys were seen on screen and $($hid.Count) hidden" }
  }
  if (@($rows | Where-Object { -not $_.Key }).Count) { Add-Failure "$(@($rows | Where-Object { -not $_.Key }).Count) of $Keys keys weren't in metrics' present.actions at all" }
  if ($seen.Count -and -not $avail.frame_statistics) { Add-Failure 'keys were seen on screen, but metrics says DXGI''s frame statistics aren''t available' }
  foreach ($r in @($rows | Where-Object { $_.Key -and $_.Key.last })) {
    $l = $r.Key.last
    if (-not ($l.paint_ms -le $l.present_ms)) { Add-Failure "key $($r.I): presented ($($l.present_ms) ms) before its paint ($($l.paint_ms) ms)" }
  }
  $checked.Add('keys timed to their paints and presents in order')
  foreach ($r in $seen) {
    $l = $r.Key.last
    if (-not ($l.paint_ms -le $l.present_ms -and $l.present_ms -le $l.screen_ms)) { Add-Failure "key $($r.I): its times aren't in order: paint $($l.paint_ms), presented $($l.present_ms), on screen $($l.screen_ms)" }
    if ($l.screen_ms - $l.present_ms -gt 100) { Add-Failure "key $($r.I): on screen $([Math]::Round($l.screen_ms - $l.present_ms, 1)) ms after its presents, more than six refreshes of a 60 Hz screen" }
    if ($l.screen_unix_ms -lt $r.T0 -or $l.screen_unix_ms -gt $r.Read) { Add-Failure "key $($r.I): on screen at $($l.screen_unix_ms), outside the $($r.T0) to $($r.Read) it was posted and read in" }
  }
  if ($seen.Count) { $checked.Add("$($seen.Count) of $Keys keys timed to the screen") }
  else { $unchecked.Add('no key was seen going up, so nothing was timed to the screen') }
  $paint = @((Get-Metrics).present.actions | Where-Object action -eq 'paint')[0]
  "every frame's paint: $(if ($paint) { "$($paint.count) shown, $($paint.seen) seen on screen, $($paint.hidden) hidden, $($paint.overtaken) overtaken, $($paint.lost) lost$(if ([int]$paint.seen) { "; after its presents $($paint.after_present_ms.mean) ms on average" })" } else { 'none' })"

  # 2. Motion
  function Measure-Motion([string]$What, [scriptblock]$Do, [scriptblock]$Until, [string[]]$Names) {
    Get-QuietFrame -QuietMs 400 -TimeoutSec 20 -Window $w | Out-Null
    Get-Metrics -Reset | Out-Null
    & $Do
    Wait-Change $Until -What $What -TimeoutSec 20 | Out-Null
    Get-QuietFrame -QuietMs 400 -TimeoutSec 20 -Window $w | Out-Null
    $m = (Get-Metrics).present
    $mo = $m.motion
    $last = $mo.last
    "${What}: $($mo.episodes) episodes, $($mo.measured) measured; $($mo.refreshes) refreshes, $($mo.presents) presents, $($mo.missed) missed; first frame up at $($mo.first_ms.mean) ms; $($mo.paints) paints, $($mo.slow_paints) over a refresh, the longest $($mo.longest_paint_ms) ms; names $(@($mo.names | ForEach-Object name) -join ', ')"
    if ($last) { "  the last: $($last | ConvertTo-Json -Compress)" }
    if (-not $m.available.compositor_clock) { Add-Failure "${What}: metrics says the compositor clock's statistics aren't available"; return }
    if ([int]$mo.episodes -lt 1) { Add-Failure "${What}: no episode of motion"; return }
    # Motion in a window others hid went up to nobody: said, never counted as missed
    if ([int]$mo.covered -ge 1) {
      if ([int]$mo.missed -ne 0 -or [int]$mo.refreshes -ne 0) { Add-Failure "${What}: hidden, yet counted $($mo.refreshes) refreshes and $($mo.missed) missed" }
      Add-Note ("${What}: the window was hidden (over it: $((Get-WindowCover $w).Said)), so whether every refresh got a frame couldn't be checked")
      $script:motionHidden++
      return
    }
    if ([int]$mo.measured -lt 1) { Add-Failure "${What}: no present of its motion seen on its screen"; return }
    $script:motionWalked++
    if ([int]$mo.refreshes -lt 1) { Add-Failure "${What}: no refreshes counted" }
    if ([int]$mo.presents -gt [int]$mo.refreshes) { Add-Failure "${What}: more presents ($($mo.presents)) than refreshes ($($mo.refreshes))" }
    if ([int]$mo.missed -ne [int]$mo.refreshes - [int]$mo.presents) { Add-Failure "${What}: missed $($mo.missed) isn't refreshes less presents" }
    $have = @($mo.names | ForEach-Object name)
    foreach ($n in $Names) { if ($n -notin $have) { Add-Failure "${What}: no $n among its names ($($have -join ', '))" } }
  }
  $script:motionHidden = 0
  $script:motionWalked = 0
  Measure-Motion 'a split' { Send-Key $w F7 } { param($s) @($s.panes).Count -eq 2 } @('pane.layout')
  Measure-Motion 'its close' { Send-Key $w F3 } { param($s) @($s.panes).Count -eq 1 } @('pane.layout')
  Measure-Motion 'a new tab' { Send-Key $w F8 } { param($s) @($s.tabs).Count -eq 2 } @()
  $was = (Get-Win).active_tab
  Measure-Motion 'a tab switch' { Send-Key $w F4 } { param($s) $s.active_tab -ne $was } @('pane.dissolve')
  $pad = (Get-Win).rail.rows[0].pad
  $x, $y = Get-RectCenter $pad
  Measure-Motion 'the rail opening' { Send-Mouse $w Move $x $y } { param($s) $s.rail.open } @('rail.edge')
  Send-Leave $w
  Wait-State { param($sw) $sw.rail.phase -eq 'closed' -and $sw.settled } -What 'the rail closed' -TimeoutSec 15 | Out-Null
  Send-Key $w F6
  Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'back to one tab' -TimeoutSec 15 | Out-Null
  if ($script:motionWalked) { $checked.Add("$script:motionWalked motions' refreshes walked") }
  if ($script:motionHidden) { $unchecked.Add("$script:motionHidden motions went to a hidden window, so no refresh of theirs was walked") }

  # 3. Under load: a writer beside the prompt
  $writer = @'
$line = ' the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk'
$sb = [Text.StringBuilder]::new()
for ($i = 0; $i -lt 630; $i++) { [void]$sb.Append($i.ToString().PadLeft(6)).Append($line).Append("`r`n") }
$chunk = [Text.Encoding]::UTF8.GetBytes($sb.ToString())
$out = [Console]::OpenStandardOutput()
while ($true) { $out.Write($chunk, 0, $chunk.Length) }
'@
  $pane = Get-ActivePane (Get-Win)
  $id = if ($Mux) { $pane.server_id } else { $pane.id }
  Invoke-Cli split-pane --pane-id $id --right -Program (New-PwshProgram $writer) -Server:$Mux -Check | Out-Null
  Invoke-Cli activate-pane --pane-id $id -Server:$Mux -Check | Out-Null
  # The prompt's pane active again, by its id (the writer's cursor goes anywhere)
  Wait-State { param($sw) $a = Get-ActivePane $sw; @($sw.panes).Count -eq 2 -and $(if ($Mux) { $a.server_id -eq $id } else { $a.id -eq $id }) } -What 'the writer beside the prompt' -TimeoutSec 20 | Out-Null
  Start-Sleep -Milliseconds 1000
  Send-Key $w Escape
  Start-Sleep -Milliseconds 300
  $loadedRows = @(Measure-Keys ([Math]::Max(6, [int]($Keys / 2))))
  Format-Keys $loadedRows 'keys with a writer at full speed beside them'
  $loaded = @($loadedRows | Where-Object { $_.Key -and $_.Key.last -and $null -ne $_.Key.last.screen_ms })
  $med = { param($rows, $f) $v = @($rows | Where-Object { $_.Key.last -and $null -ne $_.Key.last.$f } | ForEach-Object { [double]$_.Key.last.$f } | Sort-Object); if ($v.Count) { $v[[int][Math]::Floor(($v.Count - 1) / 2)] } else { '-' } }
  foreach ($f in 'paint_ms', 'present_ms', 'screen_ms') { "median $f`: $(& $med $rows $f) alone, $(& $med $loadedRows $f) with the writer" }

  # 4. What it cost
  $cost = (Get-Metrics).present.cost
  "cost since the last reset: $($cost.polls) looks at a swapchain, $($cost.poll_ms) ms; $($cost.walks) walks over $($cost.frames_walked) compositor frames, $($cost.walk_ms) ms"
  if ([int]$cost.polls -gt 0 -and [double]$cost.poll_ms / [int]$cost.polls -gt 0.2) { Add-Failure "a look at a swapchain's statistics cost $([Math]::Round([double]$cost.poll_ms / [int]$cost.polls, 3)) ms" }
  if ([int]$cost.frames_walked -gt 0 -and [double]$cost.walk_ms / [int]$cost.frames_walked -gt 0.05) { Add-Failure "a compositor frame walked cost $([Math]::Round([double]$cost.walk_ms / [int]$cost.frames_walked, 4)) ms" }
  "  and $($cost.covers) looks at what's over the window, $($cost.cover_ms) ms"
  if ([int]$cost.covers -gt 0 -and [double]$cost.cover_ms / [int]$cost.covers -gt 2) { Add-Failure "a look at what's over the window cost $([Math]::Round([double]$cost.cover_ms / [int]$cost.covers, 3)) ms" }
  $checked.Add('what it costs')
  # Only what was checked passes; what wasn't is said, in the line and the notes
  foreach ($u in $unchecked) { Add-Note "unchecked: $u" }
  (Get-Test).PassedLine = "present$(if ($Mux) { ' (mux)' }): $($checked -join ', ')$(if ($unchecked.Count) { "; UNCHECKED: $($unchecked -join '; ')" })"
} -Passed "present$(if ($Mux) { ' (mux)' })"
