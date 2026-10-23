#Requires -Version 7
# Covers: wezterm-gui/src/restart.rs, lbterm-extras/src/restart.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/fancy_tab_bar.rs, wezterm-client/src/domain.rs
<#
The "Restart to update" pill (lbterm-extras/src/restart.rs, wezterm-gui/src/restart.rs) end to
end, in a test window of its own class, on a mux domain and socket of its own, run from a copy
of this tree's build in target\perf\restart\bin that has a version.txt, as an install does:

1. version.txt names this build: no pill (no-pill.png).
2. version.txt names another build: within the 10 s poll the GUI logs that the pill shows, and it
   does (pill.png). Everything that changed lies in the strip, at its right end, so the strip
   kept its height and the panes' cards didn't move.
3. A click on the pill (posted messages, never real input): the GUI starts the copy's
   lbterm-gui.exe and ends. A new GUI from the copy attaches to the same server, which still
   holds the same tab (restarted.png), and neither GUI ever had the foreground.

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
$sock = Join-Path $T.Out 'sock'
$cfg = Join-Path $T.Out 'config\restart.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

Remove-OldBuilds $bin
Copy-Build -From $T.Bin -To $bin | Out-Null
$version = Get-BuildVersion $T.Bin
Set-BuildVersion $bin $version

[IO.File]::WriteAllText($cfg, @"
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  initial_cols = 100, initial_rows = 24,
  window_close_confirmation = 'NeverPrompt',
  -- The window's own buttons in the strip, as Lachlan's lbterm.lua has them
  window_decorations = 'INTEGRATED_BUTTONS|RESIZE',
  -- Nothing moves but the pill, so two shots differ only by it
  cursor_blink_rate = 0,
  default_cursor_style = 'SteadyBlock',
  lbterm_crash_restore = false,
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
  $w = Open-TestWindow -ConfigFile $cfg -Domain perfrestart -Socket $sock -Exe (Join-Path $bin 'lbterm-gui.exe') -Log 'info'
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

  # 3. A click at the pill's middle
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
  $clients = @(Invoke-Cli list-clients --format json -Server -Json | Where-Object { $_.pid -eq $next.Id })
  if (-not $clients) { Add-Failure "the new GUI (pid $($next.Id)) isn't a client of the server" }
  $cmdLine = (Get-CimInstance Win32_Process -Filter "ProcessId=$($next.Id)").CommandLine
  if ($cmdLine -notmatch [regex]::Escape("--class $($T.Class)")) { Add-Failure "the new GUI didn't get the old one's arguments: $cmdLine" }
  # The new window can have no client area yet for a moment: a capture then has nothing to cut
  for ($i = 0; $i -lt 10; $i++) {
    try { Get-Shot $nw -Name 'restarted' | ForEach-Object Dispose; break }
    catch { if ($i -eq 9) { Add-Failure "couldn't capture the new window: $($_.Exception.Message)" } else { Start-Sleep -Milliseconds 300 } }
  }
} -Passed "restart: the pill showed in the strip without moving the panes, and a click restarted into the copy with the tab kept ($($T.Out))"
