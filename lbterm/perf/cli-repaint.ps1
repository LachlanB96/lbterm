#Requires -Version 7
# Variant: -Mux
# Covers: mux/src/lbterm_repaint.rs, wezterm-gui/src/termwindow/mod.rs, mux/src/tab.rs, mux/src/window.rs, mux/src/lib.rs, wezterm-client/src/domain.rs, wezterm-client/src/client.rs, wezterm-client/src/pane/clientpane.rs, wezterm-mux-server-impl/src/sessionhandler.rs
<#
What `lbterm cli` changes in a window shows there at once, with nothing else to paint it. The
Conductor, AI Dashboard, Relay and terminal.ahk drive LBTerm this way, often while it's in the
background. A focus, a zoom or a split moved from the cli used to change the mux and leave the
window on its old frame until something else painted it (a program's output, the pointer, a
capture: PrintWindow sends WM_PAINT, which is how it hid from the capture-based tests).

In a test window that looks like Lachlan's LBTerm (New-TestConfig: the cursor doesn't blink, as
his doesn't) whose panes run programs that draw nothing, so no output paints it. With -Mux its
tabs live in a test lbterm-mux-server, on a domain and socket of its own
(target\perf\cli-repaint-mux), and every call goes to the server, as the tools above reach
Lachlan's LBTerm. Each step waits until the window is idle (settled, and no frame for -QuietMs),
makes one call, and waits on the state file alone (lbterm/STATE.md) for a later frame showing it;
never a capture, which would paint the window itself.

1. split-pane: the new pane is laid out (its program's first output paints it, so this always
   showed)
2. activate-pane back to the first pane, then activate-pane-direction down to the second
3. zoom-pane --zoom: the tab zoomed, with the strip's badge; --unzoom: back
4. adjust-pane-size: the second pane four rows taller
5. set-tab-title: the tab's name, on the strip
6. With the keyboard in the window (a posted WM_SETFOCUS): activate-pane and a zoom and back
7. kill-pane: one pane left
8. spawn --no-activate, activate-tab to it, activate-pane back to the first tab

A step that doesn't show within -TimeoutSec is a failure; a capture then paints the window, so
the next step starts from what it shows.

  pwsh -File lbterm\perf\cli-repaint.ps1 [-Mux] [-TimeoutSec 3]
#>
param([string]$Bin, [switch]$Mux, [double]$TimeoutSec = 3, [int]$QuietMs = 300)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('cli-repaint' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = Join-Path $T.Out 'sock'
$cfg = Join-Path $T.Out 'config\repaint.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The server reads the same file (serve_command), so both know the test domain
$domain = if ($Mux) {
  @"
config.unix_domains = { {
  name = 'perfrepaint',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 100
config.initial_rows = 24
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.window_close_confirmation = 'NeverPrompt'
$domain
"@

# A pane that draws nothing once it's up
$quiet = New-PwshProgram 'Start-Sleep 600'

# The GUI's id for a pane the cli names: the same for a window's own panes; on a mux domain the
# state file has the server's beside it
$script:ids = @{}
function Update-Ids($sw) { foreach ($p in @($sw.panes)) { if ($null -ne $p.server_id) { $script:ids[[int]$p.server_id] = [int]$p.id } } }
function Gui([int]$id) {
  if (-not $Mux) { return $id }
  if (-not $script:ids.ContainsKey($id)) { throw "no pane in the window has server id $id" }
  $script:ids[$id]
}
function Active($sw) { @($sw.tabs | Where-Object active)[0] }
function Pane($sw, [int]$id) { @($sw.panes | Where-Object { $_.id -eq (Gui $id) })[0] }

# The window idle: settled, and no frame painted for -QuietMs after, so the next frame is the next
# call's. Its entry, in $script:x.
function Wait-Quiet {
  for ($i = 0; $i -lt 20; $i++) {
    $frame = (Wait-Settled -TimeoutSec 10 -What 'the window to settle').frame
    Start-Sleep -Milliseconds $QuietMs
    $script:x = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    if ($script:x.frame -eq $frame) { Update-Ids $script:x; return }
  }
  throw "the window never went $QuietMs ms without painting: it last said $(Format-StateWindow $script:x)"
}

# One cli call made to the idle window, then a frame after it showing -Shows, waited on in the
# state file alone. The window, idle again after, in $script:x.
function Step {
  param([string]$Name, [scriptblock]$Call, [scriptblock]$Shows)
  Wait-Quiet
  $before = $script:x.frame
  $clock = [Diagnostics.Stopwatch]::StartNew()
  & $Call | Out-Null
  try {
    $shown = Wait-Change $Shows -AfterFrame $before -TimeoutSec $TimeoutSec -What $Name -Animating
    Write-Host "${Name}: shown and settled $($clock.ElapsedMilliseconds) ms after the call, frame $before to $($shown.frame)"
  } catch {
    Add-Failure "$Name didn't show in $TimeoutSec s, nothing else painting the window: $($_.Exception.Message)"
    Write-Host "${Name}: not shown in $TimeoutSec s, still frame $before"
    # What the capture-based tests did without knowing it: PrintWindow paints the window
    (Get-Shot $w).Dispose()
    Wait-Change $Shows -TimeoutSec 10 -What "$Name, once a capture painted the window" -Animating | Out-Null
  }
  Wait-Quiet
}

Invoke-Test {
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfrepaint -Socket $sock -Program $quiet -TimeoutSec 15 }
  else { Open-TestWindow -ConfigFile $cfg -Program $quiet }
  if ($Mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $sock didn't answer" }
  Wait-Change { param($sw) $sw.frame -gt 0 -and @($sw.panes).Count -eq 1 } -What 'the first pane' | Out-Null
  Wait-Quiet
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  if ($Mux -and -not $script:ids.ContainsKey([int]$first)) {
    throw "the window's pane isn't the server's pane ${first}: $(@($script:x.panes | ForEach-Object { "$($_.id)/$($_.server_id)" }) -join ', ')"
  }

  # 1. A split
  $script:second = $null
  Step 'split-pane' { $script:second = [int](Invoke-Cli split-pane --pane-id $first --bottom -Program $quiet -Server:$Mux -Check) } {
    param($sw) @($sw.panes).Count -eq 2 -and @($sw.panes | Where-Object { $_.id -ne (Gui $first) }).Count -eq 1
  }
  $second = $script:second
  "panes: first $first, second $second$(if ($Mux) { " (the window's $(Gui $first) and $(Gui $second))" })"

  # 2. The keyboard moved between the panes
  Step 'activate-pane' { Invoke-Cli activate-pane --pane-id $first -Server:$Mux -Check } { param($sw) $sw.active_pane -eq (Gui $first) }
  Step 'activate-pane-direction' { Invoke-Cli activate-pane-direction --pane-id $first Down -Server:$Mux -Check } { param($sw) $sw.active_pane -eq (Gui $second) }

  # 3. Zoomed and back
  Step 'zoom-pane --zoom' { Invoke-Cli zoom-pane --pane-id $second --zoom -Server:$Mux -Check } {
    param($sw) (Active $sw).zoomed -and @($sw.panes).Count -eq 1 -and (-not $sw.strip -or $sw.strip.badge -match 'Zoomed')
  }
  Step 'zoom-pane --unzoom' { Invoke-Cli zoom-pane --pane-id $second --unzoom -Server:$Mux -Check } {
    param($sw) -not (Active $sw).zoomed -and @($sw.panes).Count -eq 2 -and (-not $sw.strip -or -not $sw.strip.badge)
  }

  # 4. A split moved
  $rows = (Pane $script:x $second).rows
  Step 'adjust-pane-size' { Invoke-Cli adjust-pane-size --pane-id $second --amount 4 Up -Server:$Mux -Check } {
    param($sw) $p = Pane $sw $second; $p -and $p.rows -eq $rows + 4
  }

  # 5. The tab named
  Step 'set-tab-title' { Invoke-Cli set-tab-title --pane-id $first 'Repaint test' -Server:$Mux -Check } {
    param($sw) (Active $sw).title -eq 'Repaint test' -and (-not $sw.strip -or $sw.strip.title -eq 'Repaint test')
  }

  # 6. With the keyboard: a window that has it paints no more often, its cursor not blinking
  Send-Focus $w
  Wait-Change { param($sw) $sw.focused } -What 'the keyboard in the window' | Out-Null
  Step 'activate-pane, focused' { Invoke-Cli activate-pane --pane-id $first -Server:$Mux -Check } { param($sw) $sw.active_pane -eq (Gui $first) }
  Step 'zoom-pane --zoom, focused' { Invoke-Cli zoom-pane --pane-id $first --zoom -Server:$Mux -Check } { param($sw) (Active $sw).zoomed -and @($sw.panes).Count -eq 1 }
  Step 'zoom-pane --unzoom, focused' { Invoke-Cli zoom-pane --pane-id $first --unzoom -Server:$Mux -Check } { param($sw) -not (Active $sw).zoomed -and @($sw.panes).Count -eq 2 }
  Send-Focus $w -Off
  Wait-Change { param($sw) -not $sw.focused } -What 'the keyboard gone from the window' | Out-Null

  # 7. A pane closed
  Step 'kill-pane' { Invoke-Cli kill-pane --pane-id $second -Server:$Mux -Check } { param($sw) @($sw.panes).Count -eq 1 -and @($sw.tabs).Count -eq 1 }

  # 8. Tabs: one added in the background, opened, and left again
  Step 'spawn --no-activate' { Invoke-Cli spawn --pane-id $first --no-activate -Program $quiet -Server:$Mux -Check } {
    param($sw) @($sw.tabs).Count -eq 2 -and $sw.active_tab -eq 0
  }
  Step 'activate-tab' { Invoke-Cli activate-tab --pane-id $first --tab-index 1 -Server:$Mux -Check } { param($sw) $sw.active_tab -eq 1 }
  Step 'activate-pane, another tab' { Invoke-Cli activate-pane --pane-id $first -Server:$Mux -Check } { param($sw) $sw.active_tab -eq 0 -and $sw.active_pane -eq (Gui $first) }
} -Passed "cli-repaint$(if ($Mux) { ' -Mux' }): every cli change showed in the window by itself"
