#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/menu_fade.rs, wezterm-gui/src/termwindow/keyevent.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/termwindow/render/new_tab.rs, wezterm-gui/src/termwindow/render/prompt.rs, wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/termwindow/render/inbox.rs, wezterm-gui/src/termwindow/render/agenda.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/newtab.rs, lbterm-motion/src/menu.rs, lbterm-motion/src/rail.rs, lbterm-panel/src/palette.rs, wezterm-gui/src/termwindow/render/tab_panel.rs
<#
The menus' keys and light (lbterm/NEW-TAB.md, section 6; lbterm/TAB-MOUSE.md, "The tab menu's
model"), in test windows of their own class on the right-most screen that never take focus. Keys
are posted key messages and clicks posted mouse messages, and every wait is on the state file
(lbterm/STATE.md) with deadlines far past what a loaded PC takes: what's judged is what the
window did and in what order, never how many milliseconds it took.

The keys, in a window with the compositor and motion on:
1. A right click on the first tab's pad opens the tab menu: each row's key in the state file is
   the one it shows dim at its right (R, S, M, O, X; none on the Colour row).
2. A letter no row takes (Q) leaves the menu open with nothing lit, the pane's cursor where it
   was, and flares the card's edge once (menu.tab.hairline in the state file's animations, the
   name kept from before Lachlan's 9a: lbterm_motion::menu::bump, the hairline and the light).
3. Enter with no row lit lights the first (Rename) and leaves the menu open, the frame that
   lights it making the menu's light again (visuals.built's menu-glow-tab) only if the card moved,
   which it didn't: the light is kept in its layer; Enter again starts the rename; Esc leaves the
   name; Down then lights the Colour row, the highlight gliding there (menu.tab.glide) rather than
   going out (Lachlan's 12 of 2026-11-08), and Esc closes the menu.
4. A digit picks its swatch: 3 gives the tab the third colour and closes the menu, its swatch
   flashing white as it goes (menu.tab.flash in the state file's animations, Lachlan's 9b); 0
   takes it off.
5. Delete is Close tab: on a second tab, it closes that tab, its row flashing (menu.tab.flash).
   Its row on the rail fades out (rail.row.exit) once the menu's card has gone: no frame has any
   animation the rail's hold drives (the harness's Get-RailUnderCard: the rows', the marker's,
   the progress fills', a status word's dissolve, the panel's edge and light, a pad's glint)
   beside the card's fade out (menu.tab), Lachlan's 7b of 2026-11-08, nothing of the rail playing
   under a menu's card.
6. The + menu: Enter with no row lit lights the first and leaves it open; a letter none of its
   rows takes (Z) flares its card's edge (menu.list.hairline); Esc closes it.
7. With no menu open, a letter reaches the pane (its cursor moves): so the keys before were the
   menus', and none reached it.
8. A key bound in the config over an open menu (Lachlan's 7b of 2026-11-07): F9, bound to open a
   tab, stands in for his Ctrl+Shift+T, since a posted key carries no Ctrl. Over the tab menu it
   flares the card's edge (menu.tab.hairline) and does nothing else: the menu stays open and no
   tab opens. Esc then F9 at once, the menu closed and fading out: F9 opens its tab.
9. The rail holds still under a menu's card (2026-11-07): with the rail closed, a right click
   straight after the move onto a pad, before the rail's rest (open_delay_ms) is up, opens the
   menu, and through the 600 ms after it, every frame read, the rail is still closed under it and
   held (rail.held); Esc, and the rail opens once the card has gone, the pointer still on the pad.
   No snapshot shows the rail's edge moving (rail.edge) while a menu is open or its card fades
   (menu.tab). Each read is of the window's own entry (Get-StateWindow), never the state file's
   top level, where rail is null and any check passes (PITFALLS 184).
10. A wave washing as a menu's card comes stops where it is and fades (the window has
   lbterm_tab_panel.wave on): the rail opened by the pointer's rest, a right click on the pad while
   the wave still has most of its run to go, and at the card's first frame rail.wave has no more
   than its fade left (300 ms), where washing on it had most of its run: a stop cuts what DWM
   played of it (InFlight::cut; before 2026-11-08 a stopped wave stayed in flight to its old end,
   and the stop itself never ran under a held panel, PITFALLS 187).
11. A + menu row picked flashes white (Lachlan's 9b): P, PowerShell 7 here, opens a shell tab, its
   row's menu.list.flash seen (new-tab.ps1's window leaves motion to Windows, so the + menu's flash
   is judged here), and the new tab's row enters the rail (rail.row.enter) once the + menu's card
   has gone: no frame has a name the hold drives beside its fade out (menu.list), his 7b.
12. Step 9 again in a window with motion off, where no card fades: the rail held closed under the
   menu, and once Esc has closed it the rail opens at once, the pointer still on the pad (before
   2026-11-08 nothing painted to let the hold go, PITFALLS 183).

The + menu's shortcuts (Lachlan's 5a of 2026-11-08), in a window with motion on whose config binds
F6 to LbtermNewTab 'Shell' and F7 to 'Reopen', standing in for his Ctrl+Alt+T and Ctrl+Shift+T,
since a posted key carries no Ctrl, and F9 to open a tab:
13. The + menu names F6 beside its first launch row and F7 for Reopen (the state file's
   new_tab.keys, what plus_hint draws); F6 over the open + menu picks that row as its letter would:
   the menu closes, one tab opens, not two (the binding didn't act beside the row), and the row
   flashes (menu.list.flash), and the new tab's row enters once the card has gone, never beside
   its fade out (as step 11). F9, bound but shown beside no row, only flares the card's edge
   (menu.list.hairline), the menu open and no tab opened (his 7b).
14. A tab closed with the tab menu's Close tab is kept to reopen, and F7 over the + menu picks its
   Reopen row: the tab back, one tab more, the row's flash, its row entering once the card has
   gone (as step 11; the Close tab before it judged the same under the tab menu's card). With
   nothing closed, that row is "Nothing closed yet" and shows no key, so F7 there is a key no row
   takes: the flare only.

The light, in a window at 2x (dpi=192) with motion off, so a locked PC shows the card at once
(PITFALLS 148):
15. The tab menu's light, read pixel by pixel out from its card's right side and bottom over the
   pane: it falls away with no step bigger than -MaxStep levels of red from one pixel to the
   next. The four stacked rings it replaced fell by 42 or 43 levels at each of their edges there,
   and the falloff by 12 at the most (2026-11-05).

The flare, in a window at 96 DPI with motion on at slowdown 10:
16. A key no row takes (Q) flares the tab menu's edge (Lachlan's 9a of 2026-11-08), at slowdown 10
   for its frames: the frames round the card's right edge, 4 times, are flare.png, with
   flare-rest.png and flare-after.png, for `lbterm.ps1 look` to show him (none while the PC is
   locked). It waits on menu.tab.hairline's played numbers and says them, and the menu stays open.
   What the flare plays, menu.tab.hairline's lamp up to 1 (MENU_BUMP_HAIRLINE: the hairline white
   and the light round the card at its most, which lbterm-panel's palette tests hold) and back to
   0 and out of the tree once done, the compositor's test holds since feat/dcomp-recorder
   (compositor/imp/tests/menus.rs); it was judged here from the state file's `played` from
   2026-11-09 (his 3a), and by luma in captures before, which failed on any locked PC.

Fails with exit 1. Captures in target\perf\menu-keys.

  pwsh -File lbterm\perf\menu-keys.ps1 [-Bin <dir>] [-MaxStep 20] [-Only keys|light]

-Only light reads the light and the flare alone, as a build from before 2026-11-05 can be (its
rings fail it).
#>
param([string]$Bin, [int]$MaxStep = 20, [ValidateSet('all', 'keys', 'light')][string]$Only = 'all')
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'menu-keys' -Bin $Bin

# The window's animations' names in a snapshot
function Get-Animations($s) { @($s.animations | ForEach-Object name) }

# The red channel along a line of a capture, from (x, y) stepping (dx, dy), n pixels
function Get-RedLine($Shot, [int]$X, [int]$Y, [int]$Dx, [int]$Dy, [int]$Count) {
  @(for ($i = 0; $i -lt $Count; $i++) {
      $px = $X + $i * $Dx; $py = $Y + $i * $Dy
      if ($px -lt 0 -or $py -lt 0 -or $px -ge $Shot.Width -or $py -ge $Shot.Height) { break }
      [int]$Shot.GetPixel($px, $py).R
    })
}

# A light read out from a card's edge: no rise past noise, and no step bigger than $MaxStep
function Test-Falloff([int[]]$Red, [string]$What) {
  $steps = @(for ($i = 1; $i -lt $Red.Count; $i++) { $Red[$i - 1] - $Red[$i] })
  $biggest = ($steps | Measure-Object -Maximum).Maximum
  $rise = ($steps | Measure-Object -Minimum).Minimum
  "  ${What}: $($Red -join ' ') (biggest step $biggest)"
  if ($Red.Count -lt 8) { Add-Failure "$What`: only $($Red.Count) pixels to read"; return }
  if ($Red[0] - $Red[-1] -lt 15) { Add-Failure "$What`: no light to read ($($Red[0]) to $($Red[-1]))" }
  if ($biggest -gt $MaxStep) { Add-Failure "$What`: a step of $biggest levels from one pixel to the next, a band" }
  if ($rise -lt -3) { Add-Failure "$What`: the light rises by $(-$rise) levels going out" }
}

Invoke-Test {
  # ---------------------------------------------------------------------------- the keys
  if ($Only -ne 'light') {
  # F9 bound to open a tab: a key bound in the config, as his Ctrl+Shift+T is (step 8)
  $bound = "config.keys = config.keys or {}`ntable.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.SpawnCommandInNewTab { args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' } } })"
  # The wave on, for step 10 (a table given here leaves out what it doesn't say: the panel is off
  # unless it says enabled)
  $w = Open-TestWindow -WorkingDirectory $env:USERPROFILE -PostedPointer -Program (New-PwshProgram) -Lua $bound `
    -Set @('lbterm_compositor=true', "lbterm_motion={enabled='on'}", 'lbterm_tab_panel={enabled=true,wave={enabled=true}}')
  # The shell's prompt drawn too (its cursor off column 0): the window can settle before pwsh has
  # written it, and a cursor read then moves when it comes, which read as Q reaching the pane
  $s = Wait-Change { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and @($s.rail.rows).Count -ge 1 -and $s.panes[0].cursor.x -gt 0 } -What 'the window, settled, with its rail and the prompt' -TimeoutSec 60
  $cursor = $s.panes[0].cursor
  $openMenu = {
    param([int]$Row = 0)
    $sw = Wait-Settled -What 'the rail, for the menu'
    $x, $y = Get-RectCenter @($sw.rail.rows)[$Row].pad
    Send-Click $w $x $y -Right -Move
    Wait-State { param($s) $s.menu.kind -eq 'tab' } -What "the tab menu on row $Row" -TimeoutSec 30
  }

  # 1. Its rows' keys
  $s = & $openMenu
  $keys = @($s.menu.items | ForEach-Object { if ($_.key) { "$($_.label)=$($_.key)" } else { "$($_.label)=" } })
  "1. tab menu: $($keys -join ', ')"
  $want = [ordered]@{ 'Rename tab' = 'R'; 'Shell in its folder' = 'S'; 'Move to new window' = 'M'; 'Close other tabs' = 'O'; 'Close tab' = 'X' }
  foreach ($label in $want.Keys) {
    $item = @($s.menu.items | Where-Object label -eq $label)[0]
    if (-not $item) { Add-Failure "the tab menu has no '$label'" }
    elseif ($item.key -ne $want[$label]) { Add-Failure "'$label' shows key '$($item.key)', not '$($want[$label])'" }
  }
  $colour = @($s.menu.items | Where-Object { $_.label -like 'Colour*' })[0]
  if ($colour -and $colour.key) { Add-Failure "the Colour row shows a key, '$($colour.key)': its swatches take the digits" }

  # 2. A letter no row takes: the edge's flare (menu.tab.hairline), and nothing else
  $f0 = $s.frame
  Send-Chars $w 'q'
  $seen = $false
  $s = Wait-State { param($s) Get-Animations $s -contains 'menu.tab.hairline' } -AfterFrame $f0 -What "Q's hairline" -TimeoutSec 30
  if ($s) { $seen = $true; "2. Q: menu.tab.hairline at frame $($s.frame)" }
  $s = Wait-Settled -What 'the hairline to settle'
  if ($s.menu.kind -ne 'tab') { Add-Failure "Q closed the menu" }
  if ($null -ne $s.menu.focus) { Add-Failure "Q lit row $($s.menu.focus)" }
  if ($s.panes[0].cursor.x -ne $cursor.x -or $s.panes[0].cursor.y -ne $cursor.y) { Add-Failure "Q reached the pane: its cursor moved from $($cursor.x),$($cursor.y) to $($s.panes[0].cursor.x),$($s.panes[0].cursor.y)" }
  if (-not $seen) { Add-Failure "Q didn't flare the card's edge (no menu.tab.hairline)" }

  # 3. Enter with nothing lit lights the first row, Enter again picks it. The frame that lights
  # it paints the menu again, and makes its light again only if the card moved or changed size:
  # it's kept in its layer (render/kept.rs), made as the menu opened
  $names = [System.Collections.Generic.HashSet[string]]::new()
  $collect = { param($s) foreach ($n in (Get-Animations $s)) { [void]$names.Add($n) } }
  Send-Key $w Enter
  $s = Wait-State { param($s) $s.menu.focus -eq 0 } -What 'Enter lighting the first row'
  "3. Enter: row $($s.menu.focus) lit ($(@($s.menu.items)[0].label)), the menu open: $($s.menu.kind)"
  if (@($s.visuals.built) -contains 'menu-glow-tab') { Add-Failure "lighting a row made the tab menu's light again (visuals.built: $(@($s.visuals.built) -join ', ')), where its kept layer should have it" }
  else { "   its light kept: the frame built [$(@($s.visuals.built) -join ', ')]" }
  Send-Key $w Enter
  $s = Wait-State { param($s) $null -ne $s.menu.renaming } -What 'Enter again starting the rename'
  "   Enter again: renaming from '$($s.menu.renaming)'"
  Send-Key $w Escape
  Wait-State { param($s) $s.menu.kind -eq 'tab' -and $null -eq $s.menu.renaming } -What 'Esc leaving the name' | Out-Null
  # The Colour row lit as every row is (Lachlan's 12 of 2026-11-08): Down from Rename lands on
  # its first swatch, and the highlight glides there (menu.tab.glide) where it went out
  # (menu.tab.light), the swatch's ring alone saying where the keys were
  $s = Wait-Settled -What 'the menu settled, Rename lit'
  if ($s.menu.focus -ne 0) { Add-Failure "Esc from the name left row $($s.menu.focus) lit, not Rename" }
  $names.Clear()
  Send-Key $w Down
  $s = Wait-Change { param($s) $s.menu.focus -eq 1 } -What 'Down onto the Colour row' -OnState $collect
  $row = @($s.menu.items)[1].label
  if ($row -notlike 'Colour*') { Add-Failure "Down from Rename lit '$row', not the Colour row" }
  elseif ($names.Contains('menu.tab.light') -or -not $names.Contains('menu.tab.glide')) { Add-Failure "the highlight didn't cover the Colour row: it played [$(@($names) -join ', ')], where it should glide there (menu.tab.glide) and not go out (menu.tab.light)" }
  else { "   Down: the Colour row lit, the highlight gliding there (menu.tab.glide)" }
  Send-Key $w Escape
  Wait-State { param($s) -not $s.menu } -What 'Esc closing the menu' | Out-Null

  # 4. A digit picks its swatch, which flashes as it goes
  & $openMenu | Out-Null
  Send-Chars $w '3'
  $s = Wait-State { param($s) -not $s.menu -and @($s.rail.rows)[0].colour } -What '3 colouring the tab' -OnState $collect
  "4. 3: the tab's colour $(@($s.rail.rows)[0].colour), the menu closed"
  if (-not $names.Contains('menu.tab.flash')) { Add-Failure "3 picked its swatch without a flash: no menu.tab.flash in [$(@($names) -join ', ')]" } else { '   its swatch flashed (menu.tab.flash)' }
  & $openMenu | Out-Null
  Send-Chars $w '0'
  $s = Wait-State { param($s) -not $s.menu -and -not @($s.rail.rows)[0].colour } -What '0 taking the colour off'
  '   0: no colour'

  # 5. Delete is Close tab, on a second tab
  New-Tab -From ([int]$s.panes[0].id) -NoActivate -Program (New-PwshProgram) -Window $w | Out-Null
  $s = Wait-Change { param($s) @($s.rail.rows).Count -eq 2 } -What 'the second tab on the rail' -TimeoutSec 30
  & $openMenu 1 | Out-Null
  # Settled, so menu.tab is the card's fade out alone (a fade in turned round counts to its own end)
  Wait-Settled -What 'the tab menu on the second tab, settled' | Out-Null
  $names.Clear()
  # Nothing of the rail plays under the card (Lachlan's 7b of 2026-11-08): the closed row's exit,
  # the marker's glide up to the first row and everything else the hold drives (the harness's
  # Get-RailUnderCard, PITFALLS 210) wait for the card's fade out (menu.tab) and play after
  $under = New-UnderCardWatch -Cards 'menu.tab'
  $closing = { param($s) & $collect $s; $under.Watch($s) }
  Send-Key $w Delete
  $s = Wait-Change { param($s) -not $s.menu -and @($s.rail.rows).Count -eq 1 } -What 'Delete closing the second tab' -TimeoutSec 30 -OnState $closing
  '5. Delete: the second tab closed'
  if (-not $names.Contains('menu.tab.flash')) { Add-Failure "Delete closed the tab without Close tab's row flashing: no menu.tab.flash in [$(@($names) -join ', ')]" }
  if (-not $names.Contains('rail.row.exit')) { Add-Failure "Delete's tab left the rail without its row fading out: no rail.row.exit in [$(@($names) -join ', ')]" }
  else { "   its row faded out (rail.row.exit)" }
  $under.Judge('   Delete')

  # 6. The + menu
  $sw = Wait-Settled -What 'the rail, for the + pad'
  if (-not $sw.new_tab) { Add-Failure 'no + pad in the state file' }
  else {
    $px, $py = Get-RectCenter $sw.new_tab.rect
    Send-Click $w $px $py -Right -Move
    $s = Wait-State { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu'
    "6. + menu: $(@($s.menu.items | ForEach-Object { "$($_.label)=$($_.key)" }) -join ', ')"
    Send-Key $w Enter
    $s = Wait-State { param($s) $s.menu.focus -eq 0 } -What 'Enter lighting its first row'
    if ($s.menu.kind -ne 'plus') { Add-Failure "Enter with nothing lit closed the + menu" }
    $f0 = $s.frame
    Send-Chars $w 'z'
    $s = Wait-State { param($s) Get-Animations $s -contains 'menu.list.hairline' } -AfterFrame $f0 -What "Z's hairline on the + menu" -TimeoutSec 30
    "   Z: menu.list.hairline at frame $($s.frame)"
    Send-Key $w Escape
    Wait-State { param($s) -not $s.menu } -What 'Esc closing the + menu' | Out-Null
  }

  # 7. No menu: a letter reaches the pane
  $s = Wait-Settled -What 'the window, with no menu'
  if ($s.panes[0].cursor.x -ne $cursor.x) { Add-Failure "the pane's cursor moved while the menus had the keys: $($cursor.x) to $($s.panes[0].cursor.x)" }
  Send-Chars $w 'a'
  $s = Wait-State { param($s) $s.panes[0].cursor.x -ne $cursor.x } -What 'A echoed in the pane' -TimeoutSec 30
  "7. with no menu open, A reached the pane: its cursor went from column $($cursor.x) to $($s.panes[0].cursor.x)"

  # 8. A bound key over an open menu waits until it's closed: over the tab menu F9 flares the
  # card's edge and opens no tab; straight after Esc, the menu fading out, it opens one
  $rows = @($s.rail.rows).Count
  $s = & $openMenu
  $f0 = $s.frame
  Send-Key $w F9
  $s = Wait-State { param($s) Get-Animations $s -contains 'menu.tab.hairline' } -AfterFrame $f0 -What "F9's hairline over the tab menu" -TimeoutSec 30
  if ($s) { "8. F9, bound, over the tab menu: menu.tab.hairline at frame $($s.frame)" }
  $s = Wait-Settled -What 'the window after F9 over the menu'
  if ($s.menu.kind -ne 'tab') { Add-Failure "F9 over the tab menu closed it" }
  if (@($s.rail.rows).Count -ne $rows) { Add-Failure "F9's binding acted over the open tab menu: $rows tabs became $(@($s.rail.rows).Count)" }
  else { "   the menu open, still $rows tab(s): its binding waited" }
  # Posted back to back, F9 comes within milliseconds of Esc, inside the card's 120 ms fade out:
  # the menu's state went at Esc, so the bindings are back
  Send-Key $w Escape, F9
  $s = Wait-Change { param($s) -not $s.menu -and @($s.rail.rows).Count -eq $rows + 1 } -What 'F9 after Esc opening its tab' -TimeoutSec 30
  "   Esc then F9 at once: the menu closed and F9 opened a tab ($rows to $(@($s.rail.rows).Count))"

  # 9. The rail holds still under a menu's card: it neither opens nor closes through it. Every
  # frame through the 600 ms past the right click is read (the rest up four times over), each the
  # window's own entry: Read-State gives the state file's top level, whose .rail is null (PITFALLS 184)
  $moved = [System.Collections.Generic.List[string]]::new()
  $watch = { param($s)
    $a = Get-Animations $s
    if (($s.menu -or $a -contains 'menu.tab') -and $a -contains 'rail.edge') { $moved.Add("frame $($s.frame)") }
  }
  $holdStep = {
    param($Win, [string]$Label)
    $moved.Clear()
    Send-Leave $Win
    Wait-State { param($s) -not $s.rail.open } -Window $Win -What "the rail closed, the pointer gone ($Label)" -TimeoutSec 30 | Out-Null
    $sw = Wait-Settled -Window $Win -What "the closed rail ($Label)"
    $x, $y = Get-RectCenter @($sw.rail.rows)[0].pad
    Send-Click $Win $x $y -Right -Move
    Wait-State { param($s) $s.menu.kind -eq 'tab' } -Window $Win -What "the tab menu on the closed rail ($Label)" -OnState $watch | Out-Null
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $last = -1
    $under = [System.Collections.Generic.List[string]]::new()
    while ($clock.ElapsedMilliseconds -lt 600) {
      $s = Get-StateWindow (Read-State $Win)
      if ($s -and $s.frame -ne $last) {
        $last = $s.frame
        & $watch $s
        if ($s.menu -and $s.rail.open) { $under.Add("frame $($s.frame)") }
      }
      Start-Sleep -Milliseconds 10
    }
    $s = Get-StateWindow (Read-State $Win)
    if ($null -eq $s.rail) { Add-Failure "${Label}: the window's state has no rail to judge" }
    elseif (-not $s.menu) { Add-Failure "${Label}: the tab menu closed by itself" }
    elseif ($under.Count -or $s.rail.open) { Add-Failure "${Label}: the rail opened under the open tab menu, its rest up ($(@($under) -join ', '))" }
    elseif (-not $s.rail.held) { Add-Failure "${Label}: the rail isn't held under the open menu (rail.held false, phase $($s.rail.phase))" }
    else { "${Label}: the rail closed and held under the open menu through 600 ms past its rest (phase $($s.rail.phase))" }
    Send-Key $Win Escape
    $s = Wait-State { param($s) -not $s.menu -and $s.rail.open } -Window $Win -What "the rail opening once the menu has gone ($Label)" -TimeoutSec 10 -OnState $watch
    "   Esc: the rail opened at frame $($s.frame), the menu gone, held $($s.rail.held)"
    if ($moved.Count) { Add-Failure "${Label}: the rail's edge moved while a menu's card showed: $($moved -join ', ')" }
  }
  & $holdStep $w '9. motion on'

  # 10. A wave washing as a menu's card comes stops where it is and fades as turned round
  # (PanelPlan::cards): the rail opened by the pointer's rest, a right click on its pad while the
  # wave has most of its run to go, and from the card's first frame it has no more than its fade
  Send-Leave $w
  Wait-State { param($s) -not $s.rail.open } -What 'the rail closed, for the wave' -TimeoutSec 30 | Out-Null
  $sw = Wait-Settled -What 'the closed rail, for the wave'
  $x, $y = Get-RectCenter @($sw.rail.rows)[0].pad
  Send-Mouse $w Move $x $y
  $s = Wait-State { param($s) @($s.animations | Where-Object name -eq 'rail.wave').Count -gt 0 } -What 'the wave washing as the rail opens' -TimeoutSec 30
  $toGo = (@($s.animations | Where-Object name -eq 'rail.wave') | Measure-Object left_ms -Maximum).Maximum
  $f0 = $s.frame
  Send-Click $w $x $y -Right
  $s = Wait-State { param($s) $s.menu.kind -eq 'tab' } -AfterFrame $f0 -What 'the tab menu over the washing wave'
  # A stop cuts what DWM played of it there (InFlight::cut), so what's left is its fade, if any
  $wave = @($s.animations | Where-Object name -eq 'rail.wave')
  $left = if ($wave.Count) { ($wave | Measure-Object left_ms -Maximum).Maximum } else { 0 }
  "10. the wave had $toGo ms to go at frame $f0; at the card's first frame ($($s.frame)) $left ms"
  if ($toGo -lt 450) { Add-Failure "the wave had only $toGo ms to go before the right click: too late to tell a stop from its end" }
  elseif ($left -gt 300) { Add-Failure "the wave washed on under the tab menu's card: $left ms to go at its first frame" }
  Send-Key $w Escape
  Wait-State { param($s) -not $s.menu } -What 'Esc closing the menu over the wave' | Out-Null

  # 11. A + menu row picked flashes white as it goes (9b): P opens a shell tab, whose row enters
  # the rail once the + menu's card has gone, never under it (7b)
  $sw = Wait-Settled -What 'the rail, for the + menu pick'
  $tabs = @($sw.rail.rows).Count
  $px, $py = Get-RectCenter $sw.new_tab.rect
  Send-Click $w $px $py -Right -Move
  $s = Wait-State { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu, for P'
  $row = @($s.menu.items | Where-Object key -eq 'P')[0]
  if (-not $row) { Add-Failure "the + menu has no row with the key P: $(@($s.menu.items | ForEach-Object { "$($_.label)=$($_.key)" }) -join ', ')" }
  else {
    # Settled, so menu.list is the card's fade out alone (PITFALLS 194)
    Wait-Settled -What 'the + menu, settled, for P' | Out-Null
    $names.Clear()
    $under = New-UnderCardWatch -Cards 'menu.list'
    Send-Chars $w 'p'
    $s = Wait-Change { param($s) -not $s.menu -and @($s.rail.rows).Count -eq $tabs + 1 } -What 'P opening a shell tab' -TimeoutSec 30 -OnState { param($s) & $collect $s; $under.Watch($s) }
    "11. P on the + menu ('$($row.label)'): a tab opened ($tabs to $(@($s.rail.rows).Count))"
    if (-not $names.Contains('menu.list.flash')) { Add-Failure "P picked '$($row.label)' without its row flashing: no menu.list.flash in [$(@($names) -join ', ')]" }
    else { '   its row flashed (menu.list.flash)' }
    if (-not $names.Contains('rail.row.enter')) { Add-Failure "P's tab came onto the rail without its row entering: no rail.row.enter in [$(@($names) -join ', ')]" }
    $under.Judge('   P')
  }
  Stop-TestWindow $w

  # 12. Step 9 with motion off: no card fades, and the hold is let go all the same
  $w3 = Open-TestWindow -WorkingDirectory $env:USERPROFILE -PostedPointer -Program (New-PwshProgram) `
    -Set @('lbterm_compositor=true', "lbterm_motion={enabled='off'}")
  Wait-Change { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and @($s.rail.rows).Count -ge 1 } -Window $w3 -What 'the window with motion off, settled' -TimeoutSec 60 | Out-Null
  & $holdStep $w3 '12. motion off'
  Stop-TestWindow $w3

  # 13 and 14. The + menu's shortcuts (Lachlan's 5a of 2026-11-08), in a window that binds them
  # to keys a posted message can carry: F6 to LbtermNewTab 'Shell' and F7 to 'Reopen', standing
  # in for his Ctrl+Alt+T and Ctrl+Shift+T, and F9 to open a tab, a bound key no row shows
  $shortcuts = @(
    'config.keys = config.keys or {}'
    "table.insert(config.keys, { key = 'F6', mods = 'NONE', action = wezterm.action.LbtermNewTab 'Shell' })"
    "table.insert(config.keys, { key = 'F7', mods = 'NONE', action = wezterm.action.LbtermNewTab 'Reopen' })"
    "table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action.SpawnCommandInNewTab { args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' } } })"
  ) -join "`n"
  $w4 = Open-TestWindow -WorkingDirectory $env:USERPROFILE -PostedPointer -Program (New-PwshProgram) -Lua $shortcuts `
    -Set @('lbterm_compositor=true', "lbterm_motion={enabled='on'}")
  Wait-Change { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and @($s.rail.rows).Count -ge 1 -and $s.new_tab } -Window $w4 -What 'the window with the shortcuts bound, settled' -TimeoutSec 60 | Out-Null
  $plusMenu = {
    param([string]$For)
    $sw = Wait-Settled -Window $w4 -What "the rail, for the + menu ($For)"
    $px, $py = Get-RectCenter $sw.new_tab.rect
    Send-Click $w4 $px $py -Right -Move
    Wait-State { param($s) $s.menu.kind -eq 'plus' } -Window $w4 -What "the + menu ($For)" -TimeoutSec 30
  }
  # A key over the open + menu that its rows don't show: the edge's flare, the menu still open, no tab
  $strayOver = {
    param([string]$Key, [string]$Why)
    $s = & $plusMenu $Key
    $tabs = @($s.rail.rows).Count
    $f0 = $s.frame
    Send-Key $w4 $Key
    $s = Wait-State { param($s) Get-Animations $s -contains 'menu.list.hairline' } -Window $w4 -AfterFrame $f0 -What "$Key's hairline over the + menu" -TimeoutSec 30
    $s = Wait-Settled -Window $w4 -What "the window after $Key over the + menu"
    if ($s.menu.kind -ne 'plus') { Add-Failure "$Key ($Why) over the + menu closed it" }
    elseif (@($s.rail.rows).Count -ne $tabs) { Add-Failure "$Key ($Why) acted over the open + menu: $tabs tabs became $(@($s.rail.rows).Count)" }
    else { "   ${Key} ($Why): the edge's flare (menu.list.hairline), the menu open, still $tabs tab(s)" }
    Send-Key $w4 Escape
    Wait-State { param($s) -not $s.menu } -Window $w4 -What "Esc closing the + menu after $Key" -TimeoutSec 30 | Out-Null
  }
  # A key a row shows beside it picks that row: the menu closed, one tab more, the row's flash,
  # and the new tab's row entering the rail once the + menu's card has gone, never under it (7b)
  $pickBy = {
    param([string]$Key, [string]$Want, [scriptblock]$Done)
    $s = & $plusMenu $Key
    $tabs = @($s.rail.rows).Count
    # Settled, so menu.list is the card's fade out alone (PITFALLS 194)
    Wait-Settled -Window $w4 -What "the + menu, settled, for $Key" | Out-Null
    $names.Clear()
    $under = New-UnderCardWatch -Cards 'menu.list'
    Send-Key $w4 $Key
    $s = Wait-Change { param($s) -not $s.menu -and @($s.rail.rows).Count -eq $tabs + 1 -and (& $Done $s) } -Window $w4 -What "$Key picking $Want" -TimeoutSec 30 -OnState { param($s) & $collect $s; $under.Watch($s) }
    $s = Wait-Settled -Window $w4 -What "the window after $Key's pick"
    if (@($s.rail.rows).Count -ne $tabs + 1) { Add-Failure "$Key over the + menu made $(@($s.rail.rows).Count - $tabs) tabs, not one: its binding acted beside the row" }
    if (-not $names.Contains('menu.list.flash')) { Add-Failure "$Key picked $Want without its row flashing: no menu.list.flash in [$(@($names) -join ', ')]" }
    else { "   ${Key}: $Want picked, its row flashed (menu.list.flash), $tabs to $(@($s.rail.rows).Count) tabs" }
    if (-not $names.Contains('rail.row.enter')) { Add-Failure "${Key}'s tab came onto the rail without its row entering: no rail.row.enter in [$(@($names) -join ', ')]" }
    $under.Judge("   $Key")
  }

  # 13. F6 is what the + menu shows beside its first launch row: it picks that row
  $s = & $plusMenu 'its keys'
  $k = $s.new_tab.keys
  $first = @($s.menu.items)[0]
  "13. + menu: the shortcuts it shows, shell $($k.shell), reopen $($k.reopen); first row '$($first.label)'"
  if ($k.shell -ne 'F6' -or $k.reopen -ne 'F7') { Add-Failure "the + menu names the keys $($k | ConvertTo-Json -Compress), not F6 for a shell and F7 for Reopen" }
  if ($first.label -notlike '* here') { Add-Failure "the + menu's first row is '$($first.label)', not a launch entry here" }
  Send-Key $w4 Escape
  Wait-State { param($s) -not $s.menu } -Window $w4 -What 'Esc closing the + menu' -TimeoutSec 30 | Out-Null
  & $pickBy 'F6' "'$($first.label)'" { param($s) $true }
  & $strayOver 'F9' 'bound to open a tab, shown beside no row'

  # 14. F7 is what Reopen's row shows once a tab is closed: it reopens that tab. With nothing
  # closed, the row says so and shows no key, and F7 over it is a key no row takes
  $sw = Wait-Settled -Window $w4 -What 'the rail, for Close tab'
  $tabs = @($sw.rail.rows).Count
  $x, $y = Get-RectCenter @($sw.rail.rows)[$tabs - 1].pad
  Send-Click $w4 $x $y -Right -Move
  Wait-State { param($s) $s.menu.kind -eq 'tab' } -Window $w4 -What 'the tab menu on the last tab' -TimeoutSec 30 | Out-Null
  Wait-Settled -Window $w4 -What 'the tab menu on the last tab, settled' | Out-Null
  $under = New-UnderCardWatch -Cards 'menu.tab'
  Send-Key $w4 Delete
  $s = Wait-Change { param($s) -not $s.menu -and @($s.rail.rows).Count -eq $tabs - 1 -and @($s.new_tab.closed).Count -ge 1 } -Window $w4 -What 'Close tab keeping the tab to reopen' -TimeoutSec 30 -OnState { param($s) $under.Watch($s) }
  $closed = @($s.new_tab.closed).Count
  "14. Close tab: $tabs to $(@($s.rail.rows).Count) tabs, $closed kept to reopen"
  $under.Judge('   Close tab')
  & $pickBy 'F7' 'Reopen' { param($s) @($s.new_tab.closed).Count -eq $closed - 1 }
  $s = Wait-Settled -Window $w4 -What 'the window with nothing closed'
  if (@($s.new_tab.closed).Count -eq 0) { & $strayOver 'F7' 'Reopen, with nothing closed: no row shows it' }
  else { Add-Failure "after the reopen $(@($s.new_tab.closed).Count) tab(s) are still kept to reopen" }
  Stop-TestWindow $w4
  }
  if ($Only -eq 'keys') { return }

  # ---------------------------------------------------------------------------- the light at 2x
  $w2 = Open-TestWindow -WorkingDirectory $env:USERPROFILE -PostedPointer -Program (New-PwshProgram) `
    -Set @('lbterm_compositor=true', "lbterm_motion={enabled='off'}", 'dpi=192')
  $s = Wait-Change { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and @($s.rail.rows).Count -ge 1 } -Window $w2 -What 'the 2x window, settled' -TimeoutSec 60
  $x, $y = Get-RectCenter @($s.rail.rows)[0].pad
  Send-Click $w2 $x $y -Right -Move
  $s = Wait-Change { param($s) $s.menu.kind -eq 'tab' } -Window $w2 -What 'the tab menu at 2x' -TimeoutSec 30
  $r = $s.menu.rect
  $scale = [double]$s.size.dpi / 96
  "15. the tab menu at $($s.size.dpi) DPI: $($r.w) by $($r.h) at $($r.x),$($r.y)"
  $shot = Get-Shot $w2 -Name 'glow-2x'
  try {
    # Past the hairline and its soft edge, out to past the light's 11 px at 96 DPI
    $from = [int][Math]::Ceiling(3 * $scale)
    $count = [int][Math]::Ceiling(12 * $scale) - $from
    $right = [int]($r.x + $r.w) - 1 + $from
    $middle = [int]($r.y + $r.h / 2)
    Test-Falloff (Get-RedLine $shot $right $middle 1 0 $count) 'out from its right side'
    $bottom = [int]($r.y + $r.h) - 1 + $from
    $centre = [int]($r.x + $r.w * 3 / 4)
    Test-Falloff (Get-RedLine $shot $centre $bottom 0 1 $count) 'out from its bottom'
  } finally { $shot.Dispose() }
  Stop-TestWindow $w2

  # ---------------------------------------------------------------------------- the flare, said
  # 16. A key no row takes flares the card's edge (Lachlan's 9a of 2026-11-08), judged by what the
  # window says it played (his 3a of 2026-11-09), not by pixels: the flare's lamp, the hairline's
  # group and the light under the card on one animation (compositor.rs's MenuLamp), from where it
  # was up to MENU_BUMP_HAIRLINE (1: the hairline white and the light round the card at its most,
  # which lbterm-panel's palette tests hold) and back to none, the menu left open. At slowdown
  # $flareSlow so the frames round the card's edge show it for `lbterm.ps1 look` (flare.png,
  # flare-rest.png, flare-after.png), judged by his eye; none while the PC is locked, whose
  # captures leave DirectComposition's content out, and the numbers are judged all the same
  $flareSlow = 10
  $w5 = Open-TestWindow -WorkingDirectory $env:USERPROFILE -PostedPointer -Program (New-PwshProgram) `
    -Set @('lbterm_compositor=true', "lbterm_motion={enabled='on',slowdown=$flareSlow}")
  $s = Wait-Change { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 -and @($s.rail.rows).Count -ge 1 -and $s.panes[0].cursor.x -gt 0 } -Window $w5 -What 'the flare''s window, settled, with its prompt' -TimeoutSec 60
  $x, $y = Get-RectCenter @($s.rail.rows)[0].pad
  Send-Click $w5 $x $y -Right -Move
  Wait-State { param($s) $s.menu.kind -eq 'tab' } -Window $w5 -What 'the tab menu, for the flare' -TimeoutSec 30 | Out-Null
  $s = Wait-Settled -Window $w5 -What 'the tab menu faded in, at slowdown 10'
  $r = $s.menu.rect
  $edge = [int]($r.x + $r.w); $mid = [int]($r.y + $r.h / 2)
  $locked = Test-WorkstationLocked
  if (-not $locked) { (Get-Shot $w5 -Name 'flare-rest').Dispose() }
  Send-Chars $w5 'q'
  $s = Wait-State { param($s) @($s.animations | Where-Object { $_.name -eq 'menu.tab.hairline' -and $_.played }).Count -gt 0 } -Window $w5 -What 'the flare playing, with its numbers (menu.tab.hairline''s played)' -TimeoutSec 10
  $flare = @(@($s.animations | Where-Object name -eq 'menu.tab.hairline')[0].played)[-1]
  if (-not $locked) {
    # The rise is arrive_in's 80 ms and the fall panel_glow's spring: 3 s at slowdown 10 takes
    # them in past the fall's half
    $frames = Get-Frames $w5 (300 * $flareSlow)
    $area = New-Rect ($edge - 24) ($mid - 30) 48 60
    Save-Sheet @(for ($i = 0; $i -lt $frames.Count; $i += [Math]::Max(1, [int]($frames.Count / 16))) { $frames[$i] }) $area 'flare' -Zoom 4
    Remove-Frames $frames
  }
  $s = Wait-Settled -Window $w5 -What 'the flare to play out' -TimeoutSec 30
  if (-not $locked) { (Get-Shot $w5 -Name 'flare-after').Dispose() }
  # Said, not judged: what the flare plays (up to MENU_BUMP_HAIRLINE and back to none, its lamp
  # out of the tree once it's done) is the compositor's test's since feat/dcomp-recorder
  # (compositor/imp/tests/menus.rs, a_key_with_nowhere_to_go_flares_the_card_s_edge_and_leaves_nothing_after)
  "16. Q at slowdown ${flareSlow}: the flare's lamp from $([Math]::Round($flare.from, 3)) up to $([Math]::Round($flare.high, 3)) and back to $([Math]::Round($flare.to, 3)) over $($flare.ms) ms$(if ($locked) { '; no frames for look, the PC is locked' })"
  if ($s.menu.kind -ne 'tab') { Add-Failure "16. the tab menu closed under the flare ($($s.menu.kind))" }
  Stop-TestWindow $w5
} -Passed 'menu-keys: a row''s letter or a digit picks it, Delete closes the tab, Enter with nothing lit lights the first row, a key no row takes flares the card''s edge, and none reaches the pane, a bound one included until the menu has closed but for a key the + menu shows beside a row, which picks that row with its flash, a + menu pick flashes, and the rail holds still under its card and is let go with motion off too, a washing wave stopping; the light round a menu falls away without bands at 2x'
