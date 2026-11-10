#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/hop.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/park.rs, lbterm-extras/src/hop.rs, lbterm-panel/src/claude.rs, config/src/lbterm.rs, config/src/keyassignment.rs
<#
The side buttons' hops as LBTerm's own action (Lachlan's 8a and 9a; lbterm/NEW-TAB.md, "Landing
4"), end to end: a test window whose config binds the buttons as his lbterm.lua does, Back and
Forward to `LbtermTab 'Back'` and `'Forward'`, and with Shift to `'Last'` and `'Waiting'`, gets
WM_XBUTTONDOWN and WM_XBUTTONUP posted, Shift in their wParam as a mouse sends it, and the state
file must show each hop. Five tabs of the GUI's own: one (working, open), two (a question two
minutes old), three (finished twenty minutes ago), four (a question ten minutes old), five
(working).

1. Back from one wraps to five; Forward back to one, then on to two.
2. The event test-park performs `LbtermPark 'Toggle'` on two (as lbterm-park-toggle does), and its
   row says parked; Forward goes to three, Back from three skips two to one, Forward from one skips
   it to three.
3. Shift+Forward from three (finished, so in the list) goes to the next waiting, wrapping: four
   (the question oldest of those not parked, first in the list) and again three, the only finished.
   From one, which waits on nothing, to four, the first of them.
4. Shift+Back goes to the tab he was on last: from four to one, and again to four, as Alt+Tab does;
   then from three with two parked between, the tab before it that isn't parked.
5. One, three and five parked too, from five Shift+Back passes the parked tabs it was on to four;
   from four, the one tab not parked, Back, Forward and Shift+Forward go nowhere.

Its USERPROFILE is a scratch folder; the GUI's own parked list it writes beside its socket
(gui-sock-<pid>.parked.json) is removed after. Fails with exit 1.

  pwsh -File lbterm\perf\hops.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'hops' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $home2 | Out-Null

function Demo([string]$Title, [string]$State, [double]$Age = 0) {
  New-PwshProgram -File $demoTab -ArgumentList @('-Title', $Title, '-State', $State, '-AgeMinutes', $Age)
}

$lua = @"
config.window_close_confirmation = 'NeverPrompt'
config.cursor_blink_rate = 0
config.lbterm_tab_panel = { enabled = true }
local act = wezterm.action
config.mouse_bindings = config.mouse_bindings or {}
for _, b in ipairs({
  { 'Back', 'NONE', 'Back' }, { 'Forward', 'NONE', 'Forward' },
  { 'Back', 'SHIFT', 'Last' }, { 'Forward', 'SHIFT', 'Waiting' },
}) do
  table.insert(config.mouse_bindings, {
    event = { Down = { streak = 1, button = b[1] } }, mods = b[2], alt_screen = 'Any', action = act.LbtermTab(b[3]),
  })
end
-- The action as Earshot's lbterm-park-toggle reaches it, through the control pipe
wezterm.on('test-park', function(window, pane) window:perform_action(act.LbtermPark 'Toggle', pane) end)
"@

Invoke-Test {
  $cfg = Join-Path $T.Out 'config\hops.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua $lua
  $w = Open-TestWindow -ConfigFile $cfg -UserProfile $home2 -Program (Demo one working) -TimeoutSec 20
  Wait-State { param($s) $s.frame -gt 0 -and @($s.rail.rows).Count -eq 1 } -What 'the window and its row' | Out-Null
  $first = (Get-Panes)[0].pane_id
  New-Tab -From $first -NoActivate -Program (Demo two waiting 2) | Out-Null
  $three = New-Tab -From $first -NoActivate -Program (Demo three idle 20)
  # A finished session's row needs Claude Code's glyph to start its title (demo-tab.ps1's header)
  Invoke-Cli set-tab-title --pane-id $three "$([char]0x2733) three" | Out-Null
  New-Tab -From $first -NoActivate -Program (Demo four waiting 10) | Out-Null
  New-Tab -From $first -NoActivate -Program (Demo five working) | Out-Null
  $s = Wait-Change {
    param($s) $r = @($s.rail.rows)
    $r.Count -eq 5 -and $s.active_tab -eq 0 -and $r[1].state -eq 'waiting' -and $r[2].state -eq 'idle' -and $r[3].state -eq 'waiting'
  } -What 'five rows: two and four asking, three finished, one open'
  "rows: $(@($s.rail.rows | ForEach-Object { "$($_.tab_index) $($_.state)" }) -join ', ')"
  $x, $y = Get-RectCenter @($s.panes)[0].rect

  # A press, and the tab it should land on; $null for one that goes nowhere, judged a while after:
  # nothing paints for a press that does nothing
  $press = { param([string]$name, [bool]$forward, [bool]$shift, $want)
    $keys = if ($shift) { @('Shift') } else { @() }
    $before = (Get-StateWindow (Read-State)).active_tab
    Send-SideButton $w $x $y -Forward:$forward -Keys $keys
    if ($null -eq $want) {
      Start-Sleep -Milliseconds 800
      $after = (Get-StateWindow (Read-State)).active_tab
      if ($after -ne $before) { Add-Failure "${name}: wanted nowhere, went from $before to $after" } else { "${name}: stayed on $before" }
      return
    }
    $landed = { param($sw) $sw.active_tab -eq $want }.GetNewClosure()
    try { Wait-Change $landed -What $name -TimeoutSec 5 | Out-Null; "${name}: $before to $want" }
    catch { Add-Failure "${name}: wanted tab $want from $before, $($_.Exception.Message)" }
  }
  $park = { param([int]$tab)
    Send-Emit 'test-park' -Window $w
    $is = { param($sw) @($sw.rail.rows)[$tab].parked }.GetNewClosure()
    Wait-Change $is -What "tab $tab parked" | Out-Null
    "tab $tab parked"
  }

  # 1. Back and Forward, wrapping
  & $press 'back from one, round to five' $false $false 4
  & $press 'forward round to one' $true $false 0
  & $press 'forward to two' $true $false 1

  # 2. Two parked: passed by both ways
  & $park 1
  & $press 'forward from two (parked) to three' $true $false 2
  & $press 'back from three past two to one' $false $false 0
  & $press 'forward from one past two to three' $true $false 2

  # 3. Shift+Forward: the waiting, walked. Not parked: four (asks, 10 min), then three (finished)
  & $press 'shift+forward from three to four' $true $true 3
  & $press 'shift+forward from four to three' $true $true 2
  & $press 'back from three past two to one' $false $false 0
  & $press 'shift+forward from one to four, the first waiting' $true $true 3

  # 4. Shift+Back: the last tab he was on, back and forth; two (parked) never
  & $press 'shift+back from four to one' $false $true 0
  & $press 'shift+back from one to four again' $false $true 3
  & $press 'back from four to three' $false $false 2
  & $press 'shift+back from three to four' $false $true 3

  # 5. Every other tab parked: nowhere to go
  & $press 'back from four to three' $false $false 2
  & $park 2
  & $press 'back from three past two to one' $false $false 0
  & $park 0
  & $press 'back from one round to five' $false $false 4
  & $park 4
  & $press 'shift+back from five past the parked to four' $false $true 3
  & $press 'back with every other tab parked' $false $false $null
  & $press 'forward with every other tab parked' $true $false $null
  & $press 'shift+forward with only the open tab waiting' $true $true $null
  $gui = "$($w.Socket).parked.json"
  Stop-TestWindow $w
  Remove-Item -LiteralPath $gui, "$gui.tmp" -ErrorAction SilentlyContinue
} -Passed 'hops: Back and Forward past parked tabs, Shift+Forward walking the waiting, Shift+Back to the last tab, nowhere with every other tab parked'
