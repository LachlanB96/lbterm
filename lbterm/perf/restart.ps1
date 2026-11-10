#Requires -Version 7
# Covers: wezterm-gui/src/restart.rs, lbterm-extras/src/restart.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/fancy_tab_bar.rs, wezterm-client/src/domain.rs, wezterm-gui/src/placement.rs, lbterm-extras/src/placement.rs, window/src/os/windows/window.rs
<#
The "Restart to update" pill (lbterm-extras/src/restart.rs, wezterm-gui/src/restart.rs) end to
end, in a test window of its own class, on a mux domain and socket of its own, run from a copy
of this tree's build in target\perf\restart\bin that has a version.txt, as an install does:

1. version.txt names this build: no pill (no-pill.png).
2. version.txt names another build: as it's written the GUI logs that the pill shows, and it
   does (pill.png). Everything that changed lies in the strip, at its right end, so the strip
   kept its height and the panes' cards didn't move.
3. With the launcher open (F8 here), a click on the pill (posted messages, never real input):
   the GUI starts the copy's lbterm-gui.exe and ends. The launcher is an overlay, a pane the GUI
   holds itself; until 2026-10-23 the restart counted it as a tab that would end with the GUI, and
   one left behind by a closed tab refused every restart of Lachlan's LBTerm. A new GUI from the copy attaches to the same server, which still
   holds the same tab (restarted.png), and neither GUI ever had the foreground. The window was
   given a size of its own first: the new one comes back at the same place and size, its place
   having been kept (LBTERM_PLACEMENT_DIR, lbterm-extras/src/placement.rs), and the tab is
   never resized.

Not here: the pill brightening under the pointer. A posted WM_MOUSEMOVE makes the window ask for
TME_LEAVE, and with the real pointer elsewhere Windows answers with WM_MOUSELEAVE at once, so
the hover is gone before the next frame; its colours are tested in lbterm-extras. Ctrl+Shift+F5
is the same action as the click (`LbtermRestart`); its default binding is in show-keys.

  pwsh -File lbterm\perf\restart.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
$T = Start-Test 'restart' -Bin $Bin
# The copy this runs from: the restart rewrites nothing in the build itself, only version.txt here
$bin = Join-Path $T.Out 'bin'
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\restart.lua'
$placeDir = Join-Path $T.Out 'placement'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

Remove-OldBuilds $bin
Copy-Build -From $T.Bin -To $bin | Out-Null
$version = Get-BuildVersion $T.Bin
Set-BuildVersion $bin $version

[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
return {
  automatically_reload_config = false,
  keys = { { key = 'F8', mods = 'NONE', action = wezterm.action.ShowLauncher } },
  front_end = 'WebGpu',
  initial_cols = 100, initial_rows = 24,
  window_close_confirmation = 'NeverPrompt',
  -- The window's own buttons in the strip, as Lachlan's lbterm.lua has them
  window_decorations = 'INTEGRATED_BUTTONS|RESIZE',
  -- Nothing moves but the pill, so two shots differ only by it
  cursor_blink_rate = 0,
  default_cursor_style = 'SteadyBlock',
  lbterm_crash_restore = false,
  wsl_domains = {},
  lbterm_tab_panel = { enabled = true },
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  unix_domains = { {
    name = 'perfrestart',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath (Join-Path $bin 'lbterm-mux-server.exe'))', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
}
"@)

# The copy's GUIs: the test window and the one its restart starts
function Get-Guis { @(Get-Process lbterm-gui -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$bin\*" }) }

Invoke-Test {
  # Its windows' places kept in the test's own folder, as Lachlan's LBTerm keeps its own
  $w = Open-TestWindow -ConfigFile $cfg -Domain perfrestart -Socket $sock -Exe (Join-Path $bin 'lbterm-gui.exe') -Log 'info' -Environment @{ LBTERM_PLACEMENT_DIR = $placeDir }
  $first = $w.Pid
  # Attached: the server it started answers with its tab (until then the window shows a
  # connecting screen, which holds still too)
  $paneBefore = $null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt 30 -and $null -eq $paneBefore) {
    Start-Sleep -Milliseconds 500
    $paneBefore = (Get-Panes -Server).pane_id
  }
  if ($null -eq $paneBefore) { throw "the test server never answered with a tab" }
  # Settled: the shell has drawn its prompt (its cursor moved along the row), no pill, and
  # nothing moving (was two shots a second apart matching)
  Wait-Change { param($x) $p = @($x.panes | Where-Object active)[0]; $p.cursor.x -gt 0 -and -not $x.strip.restart } -What 'the prompt drawn, no pill' | Out-Null

  # The window at a size of its own, which it's to come back at: its place is kept as it changes
  $wasWide = (Get-ClientRect $w).Width
  Set-WindowSize $w ($(if ($wasWide -eq 960) { 1000 } else { 960 })) 700
  Wait-Change { param($x) $x.size.width -ne $wasWide -and -not $x.strip.restart } -What 'the window at its new size' | Out-Null
  $rectBefore = Get-WindowRect $w
  $paneSize = { $p = @(Get-Panes -Server)[0]; "$($p.size.cols)x$($p.size.rows)" }
  $sizeBefore = & $paneSize
  $keptFiles = { @(Get-ChildItem $placeDir -Filter 'window-*.txt' -ErrorAction SilentlyContinue) }
  if (Wait-Until { (& $keptFiles).Count } -TimeoutSec 5) {
    $kept = & $keptFiles
    "kept: $($kept.Name): $((Get-Content $kept[0].FullName -Raw).Trim()); the window is at $($rectBefore.Left),$($rectBefore.Top) to $($rectBefore.Right),$($rectBefore.Bottom), its tab $sizeBefore"
  } else { Add-Failure "no window's place was kept in $placeDir" }
  $noPill = Get-Shot $w -Name 'no-pill'

  # 2. Another build installed
  Set-BuildVersion $bin '20990101-000000-00000000'
  $clock.Restart()
  if (-not (Wait-LogLine $w 'the restart pill shows' -TimeoutSec 15)) { Add-Failure 'the GUI never logged that the pill shows' }
  "pill after {0:N1} s" -f $clock.Elapsed.TotalSeconds
  # Painted, and still (was 1 s)
  Wait-Change { param($x) $x.strip.restart } -What 'the pill in the strip' | Out-Null
  $pill = Get-Shot $w -Name 'pill'
  $box = Get-DiffBox $noPill $pill
  if (-not $box) { Add-Failure 'nothing on screen changed when another build was installed'; return }
  "changed: x $($box[0])..$($box[2]), y $($box[1])..$($box[3]) of $($pill.Width)x$($pill.Height)"
  # The strip is the band above the panel's ring: everything that changed must lie in it, at
  # the right
  if ($box[3] -gt $pill.Height * 0.12) { Add-Failure "the change reaches y=$($box[3]): below the strip, so the panes moved" }
  if ($box[0] -lt $pill.Width / 2) { Add-Failure "the change starts at x=$($box[0]): not at the strip's right end" }

  # 3. The launcher open, an overlay pane of the GUI's own, and a click at the pill's middle
  Send-Key $w F8
  Wait-Change { param($x) $x.overlay } -What 'the launcher open' | Out-Null
  $x = [int](($box[0] + $box[2]) / 2); $y = [int](($box[1] + $box[3]) / 2)
  Send-Mouse $w Move $x $y
  Start-Sleep -Milliseconds 300
  Send-Click $w $x $y
  (Get-Process -Id $first -ErrorAction SilentlyContinue) | ForEach-Object { $_.WaitForExit(5000) } | Out-Null
  if (Test-WindowAlive $w) { Add-Failure "the GUI (pid $first) didn't end after the click" }
  $clock.Restart()
  $next = $null
  while ($clock.Elapsed.TotalSeconds -lt 20 -and -not $next) {
    $next = Get-Guis | Where-Object { $_.Id -ne $first -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1
    Watch-Foreground
    if (-not $next) { Start-Sleep -Milliseconds 250 }
  }
  if (-not $next) { Add-Failure 'no new GUI came up after the click'; return }
  "restarted: pid $first -> $($next.Id) in {0:N1} s" -f $clock.Elapsed.TotalSeconds
  # The new GUI is one of the test's: closed with it, and watched for the foreground
  $nw = Add-TestWindow -ProcessId $next.Id -Like $w
  Use-TestWindow $nw
  # Attached and painted: the server's tab, its prompt drawn, still (was 3 s)
  Wait-Change { param($x) $p = @($x.panes | Where-Object active)[0]; $null -ne $p.server_id -and $p.cursor.x -gt 0 } -What 'the tab in the new window' | Out-Null
  $paneAfter = (Get-Panes -Server).pane_id
  if ("$paneAfter" -ne "$paneBefore") { Add-Failure "the server's panes were '$paneBefore' and are now '$paneAfter'" }
  # Back where it was, at the size it was, and its tab never resized (Lachlan, 2026-10-25: the
  # new window came up on another screen at another size)
  $rectAfter = Get-WindowRect $nw
  "placed: $($rectAfter.Left),$($rectAfter.Top) to $($rectAfter.Right),$($rectAfter.Bottom); its tab $(& $paneSize)"
  $off = [Math]::Max([Math]::Max([Math]::Abs($rectAfter.Left - $rectBefore.Left), [Math]::Abs($rectAfter.Top - $rectBefore.Top)),
    [Math]::Max([Math]::Abs($rectAfter.Right - $rectBefore.Right), [Math]::Abs($rectAfter.Bottom - $rectBefore.Bottom)))
  if ($off -gt 2) { Add-Failure "the new window is at $($rectAfter.Left),$($rectAfter.Top) to $($rectAfter.Right),$($rectAfter.Bottom), not where the old one was, $($rectBefore.Left),$($rectBefore.Top) to $($rectBefore.Right),$($rectBefore.Bottom)" }
  if ((& $paneSize) -ne $sizeBefore) { Add-Failure "the tab was $sizeBefore and is now $(& $paneSize): the restart resized it" }
  $clients = @(Invoke-Cli list-clients --format json -Server -Json | Where-Object { $_.pid -eq $next.Id })
  if (-not $clients) { Add-Failure "the new GUI (pid $($next.Id)) isn't a client of the server" }
  $cmdLine = (Get-CimInstance Win32_Process -Filter "ProcessId=$($next.Id)").CommandLine
  if ($cmdLine -notmatch [regex]::Escape("--class $($T.Class)")) { Add-Failure "the new GUI didn't get the old one's arguments: $cmdLine" }
  # The new window can have no client area yet for a moment: a capture then has nothing to cut
  for ($i = 0; $i -lt 10; $i++) {
    try { Get-Shot $nw -Name 'restarted' | ForEach-Object Dispose; break }
    catch { if ($i -eq 9) { Add-Failure "couldn't capture the new window: $($_.Exception.Message)" } else { Start-Sleep -Milliseconds 300 } }
  }
} -Passed "restart: the pill showed in the strip without moving the panes, and a click with the launcher open restarted into the copy with the tab kept ($($T.Out))"
