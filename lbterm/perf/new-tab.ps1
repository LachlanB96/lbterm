#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/render/new_tab.rs, lbterm-extras/src/newtab.rs, lbterm-panel/src/layout.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm-gui/src/termwindow/render/kept_quads.rs, wezterm-gui/src/scripting/guiwin.rs, wezterm-gui/src/termwindow/keyevent.rs, wezterm-gui/src/termwindow/render/menu_fade.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/termwindow/last_tab.rs
<#
The new-tab pad at the rail's foot, its picker and its + menu, and the Starting row
(lbterm/NEW-TAB.md, landing 1), in a test window of its own class on the right-most screen that
never takes focus. The pointer is posted mouse messages, the keys posted key messages; Shift and
Ctrl ride on a click's own message (MK_SHIFT, MK_CONTROL), since a posted key carries no modifier.
Where to click and when the window has answered come from its state file (lbterm/STATE.md): every
wait is on it, with deadlines far past what a loaded PC takes, and what's judged is the order
things came in and the frames they came in, never milliseconds (Lachlan, 2026-11-03: "the tests
should take into account that the system is sometimes busy or at a different speed"). "In the
next frame" is judged from the state file's records of the frames: the picker's `asked` and
`shown`, a start's `asked`, `shown` and `ended_frame`, kept after the fact, so no frame has to be
caught as it goes by.

Its config is its own, not Lachlan's (New-TestConfig would run his lbterm-project-action handler
for the test's projects, lbterm/PITFALLS.md 68): nine projects in scratch folders, a handler that
writes each lbterm-project-action event to events.jsonl and otherwise leaves the default to open
the default program in the project's folder, a project named Nothing whose handler takes the tab
back (window:lbterm_expect_tab(nil)) and says it handled it, Later whose handler opens its tab
behind itself 3 s after the pick, and Silent whose handler says it handled it and opens nothing.
Its first tab opens in Warden's folder's src, a folder inside a project, as a tab of his often is.
F7 is bound to LbtermNewTab 'Picker' and Ctrl+Shift+T to 'Reopen', and nothing to 'Shell'; F3 to
copy two lines and F4 to PasteFrom 'Clipboard', for step 14.

1. The pad: its pill and zone in the state file, below the rows, and lit under the pointer; the
   keys the pad names are the ones bound (F7, Ctrl+Shift+T, none for a shell).
2. A click on it opens the picker in the next frame, its first row the open tab's folder (Here,
   Warden, with src past its folder), highlighted.
3. "war" highlights Warden, its letters marked, in every frame that shows "war" in the box.
4. Esc closes it; LbtermNewTab 'Reopen' with nothing closed says so in the strip and opens
   nothing; LbtermNewTab 'Picker' (Ctrl+T in Lachlan's config, fired here through the control
   pipe's emit) opens it again.
5. "bargain hunter find a kettle": the first message says "find a kettle"; Enter fires the
   event with Bargain Hunter, its folder, action new, the prompt and source picker; the next frame
   shows the Starting row (or, where the tab came before it, the tab), the tab comes in
   Bargain Hunter's folder, in front, and the row has gone into it. The pick is kept. Every
   folder the event says is in Windows' spelling (backslashes), as the config spells it.
   Then Enter on the Here row, and "warden" typed while it's Here: each fires Warden with dir its
   src folder and project_dir Warden's own (lbterm/PITFALLS.md, 115), and opens there.
6. A digit picks its row with the box empty.
7. Shift+click on a row: the event says behind, the picker stays open saying "Started …" with
   its box cleared, and the tab comes behind: the window stays on its tab. Then Later
   Shift+clicked twice, each start waiting while the other does: each record ends with a tab of
   its own, the first's the first to come (lbterm/PITFALLS.md, 120).
8. Ctrl+click: action shell.
9. Nothing: the handler takes it back, the Starting row folds and the strip says "Nothing
   opened for Nothing". Silent: nothing opens, and the row folds, never before its 20 s by the
   GUI's clock (and the state file's frame that shows it folded never earlier), the strip's
   notice gone never before its 4 s; both may come late on a loaded PC.
10. A right click: the + menu, upward, its launch menu rows "here", Shell in a project…, Nothing
    closed yet, New window; PowerShell 7 here opens a tab.
11. Shell in a project…: its row flashes white as it's picked (menu.list.flash, Lachlan's 9b, where
    the window plays motion: its config leaves that to Windows, which plays none here on this PC,
    so it's judged where motion is on, in menu-keys.ps1's step 11), the picker titled for a shell;
    Esc closes it.
12. Reopen: a tab opened from the picker and closed by hand (F6, CloseCurrentTab, as Ctrl+W) is
    kept (the state file's new_tab.closed: its label, its folder, its project), the + menu offers
    "Reopen <label>", and LbtermNewTab 'Reopen' (Ctrl+Shift+T in Lachlan's config, fired through
    the control pipe) fires lbterm-reopen-tab with it; the handler leaves it to LBTerm, so the
    default program opens in its folder, in front, its Starting row in the next frame, and it's
    kept no more. Closed again, the + menu's Reopen row does the same.
13. Twenty tabs, the window 1,000 px tall: every one on the rail above the pad, none said hidden.
14. A paste key that isn't Ctrl+V (Lachlan's 4b of 2026-11-08: only Ctrl+V pastes into the
    picker's box, his 11a; a posted key can't carry Ctrl, so newtab::pastes_into_picker's test
    holds that): F3 copies "bargain" and "find a kettle" on two lines, which with the state file
    on goes to its copied, never the clipboard; the picker opened (F7), F4, bound to PasteFrom
    'Clipboard' as his Ctrl+Shift+V and Shift+Insert are, leaves the box empty and, as a key the
    picker has no use for, lights its box (his 3a): the state file's animations name light.glint
    after it (the window has motion on, as his has: a brief light plays only then; paste-key.png).
    Over the + menu F4 does nothing: the menu stays open, no tab opens and no box fills (his 7b).

  pwsh -File lbterm\perf\new-tab.ps1 [-Bin <build>]          # tabs in the GUI (the local domain)
  pwsh -File lbterm\perf\new-tab.ps1 [-Bin <build>] -Mux     # tabs in a test lbterm-mux-server, as LBTerm runs
#>
param([string]$Bin, [switch]$Mux)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('new-tab' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = $T.Sock

# The projects, in folders of the test's own
$root = Join-Path $T.Out 'projects'
$names = [ordered]@{
  'Home'           = $root
  'Warden'         = Join-Path $root 'Warden'
  'Bargain Hunter' = Join-Path $root 'Bargain Hunter'
  'Terminal'       = Join-Path $root 'Terminal'
  'LBTerm'         = Join-Path $root 'Terminal\code\LBTerm'
  'Musica'         = Join-Path $root 'Musica'
  'Nothing'        = Join-Path $root 'Nothing'
  'Later'          = Join-Path $root 'Later'
  'Silent'         = Join-Path $root 'Silent'
}
foreach ($d in $names.Values) { New-Item -ItemType Directory -Force $d | Out-Null }
# The first tab's folder: one inside Warden's, as a pane's file:// URL will say it
$wardenSrc = Join-Path $names['Warden'] 'src'
New-Item -ItemType Directory -Force $wardenSrc | Out-Null
$events = Join-Path $T.Out 'events.jsonl'
$reopens = Join-Path $T.Out 'reopen.jsonl'
$recent = Join-Path $T.Out 'recent.json'
$listed = foreach ($n in $names.Keys) {
  $parent = if ($n -eq 'LBTerm') { " parent = 'Terminal'," } else { '' }
  "{ name = '$n', dir = [[$($names[$n])]],$parent }"
}
$pwshExe = 'C:/Program Files/PowerShell/7/pwsh.exe'
$cfg = Join-Path $T.Out 'config\new-tab.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$domain = if ($Mux) {
  @"
  unix_domains = { {
    name = 'perfnewtab',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@
} else { '' }
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
wezterm.on('lbterm-project-action', function(window, pane, p)
  local f = io.open([[$events]], 'a')
  if f then
    f:write(encode({ name = p.name, dir = p.dir, project_dir = p.project_dir, action = p.action, prompt = p.prompt, behind = p.behind, source = p.source }) .. '\n')
    f:close()
  end
  if p.name == 'Nothing' then
    window:lbterm_expect_tab(nil)
    return false
  end
  -- Its tab behind, 3 s after the pick: two picks of it wait at once
  if p.name == 'Later' then
    wezterm.time.call_after(3, function()
      window:mux_window():spawn_tab { domain = 'CurrentPaneDomain', cwd = p.dir, activate = false }
    end)
    return false
  end
  -- Handled, and nothing opens: its row waits its 20 s
  if p.name == 'Silent' then return false end
end)
-- Ctrl+T's action in Lachlan's config, fired through the control pipe: a posted key carries no Ctrl
wezterm.on('newtab-picker', function(window, pane)
  window:perform_action(wezterm.action.LbtermNewTab 'Picker', pane)
end)
-- Ctrl+Shift+T's, the same way
wezterm.on('newtab-reopen', function(window, pane)
  window:perform_action(wezterm.action.LbtermNewTab 'Reopen', pane)
end)
-- Each reopen written down, and left to LBTerm's default: the default program in its folder
wezterm.on('lbterm-reopen-tab', function(window, pane, r)
  local f = io.open([[$reopens]], 'a')
  if f then
    f:write(encode({ label = r.label, dir = r.dir, project = r.project, session = r.session, sessions = r.sessions }) .. '\n')
    f:close()
  end
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  -- Motion on, as Lachlan's is: the picker's light round its box (step 14, his 3a) is a brief
  -- light, which plays only with the compositor and motion on (render/lights.rs)
  lbterm_motion = { enabled = 'on' },
  lbterm_tab_panel = { enabled = true, open_delay_ms = 0, labels = false },
  initial_cols = 110,
  initial_rows = 40,
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_prog = { '$pwshExe', '-NoProfile' },
  -- The first tab opens in Warden's src (its server's too, with -Mux: lbterm connect takes no --cwd)
  default_cwd = [[$wardenSrc]],
  -- A tab closed by hand, as Ctrl+W closes it: a posted key carries no Ctrl. The picker on F7 and
  -- Reopen on Ctrl+Shift+T, for the keys the pad names; nothing on 'Shell'
  keys = {
    { key = 'F6', mods = 'NONE', action = wezterm.action.CloseCurrentTab { confirm = false } },
    { key = 'F7', mods = 'NONE', action = wezterm.action.LbtermNewTab 'Picker' },
    { key = 't', mods = 'CTRL|SHIFT', action = wezterm.action.LbtermNewTab 'Reopen' },
    -- A copy and a paste a posted key can carry (step 14): F3 copies two lines, here to the state
    -- file's copied, as every copy in a test window goes; F4 pastes, as Ctrl+V does in his config.
    -- F4 is pressed only over the picker or a menu: with neither open it would paste the clipboard,
    -- which is Lachlan's, into the pane
    { key = 'F3', mods = 'NONE', action = wezterm.action_callback(function(window, pane) window:copy_to_clipboard('bargain\r\nfind a kettle') end) },
    { key = 'F4', mods = 'NONE', action = wezterm.action.PasteFrom 'Clipboard' },
  },
  launch_menu = {
    { label = 'PowerShell 7', args = { '$pwshExe', '-NoProfile' } },
    { label = 'Command Prompt', args = { 'cmd.exe' } },
  },
  lbterm_projects = { projects = { $($listed -join ', ') }, recent_file = [[$recent]] },
$domain}
"@)

# The events the handler wrote, in order
function Get-Events { if (Test-Path $events) { @(Get-Content $events | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json }) } else { @() } }
# The next event once it's written: each a count on from the last waited for
$script:EventsSeen = 0
function Wait-Event([string]$What) {
  $count = $script:EventsSeen + 1
  if (-not (Wait-Until { (Get-Events).Count -ge $count } -TimeoutSec 30)) { throw "no lbterm-project-action event for $What ($((Get-Events).Count) written, $($count - 1) seen)" }
  $script:EventsSeen = $count
  (Get-Events)[$count - 1]
}
# A folder the event says, in Windows' spelling, as the config spells it: backslashes, the case
# kept, no slash at its end (lbterm/PITFALLS.md, 115). Compared as text, as a handler compares it
function Test-Spelled([string]$Said, [string]$Want, [string]$What) {
  if ($Said -cne $Want.TrimEnd('\')) { Add-Failure "$What is '$Said', not '$($Want.TrimEnd('\'))' as the config spells it" }
}
# Words typed as their keys, spaces as the space bar
function Send-Words($Window, [string]$Text) {
  foreach ($c in $Text.ToCharArray()) {
    if ($c -eq ' ') { Send-Key $Window Space } else { Send-Key $Window ([string]$c) }
  }
}
# The picker's row by name
function Get-PickerRow($s, [string]$Name) {
  $rows = @($s.new_tab.picker.rows)
  for ($i = 0; $i -lt $rows.Count; $i++) { if ($rows[$i].name -eq $Name) { return $i } }
  -1
}
# A folder as cli list or a path says it, to compare
function ConvertTo-Folder([string]$p) { ([uri]::UnescapeDataString(($p -replace '^file:///', '')) -replace '/', '\').TrimEnd('\').ToLowerInvariant() }
# A start asked for, once it's ended: the state file's record (lbterm/STATE.md, `new_tab.start`).
# Each is a start after the last one waited for (its `asked` frame later): two in a row with the same
# label (a tab reopened twice) would otherwise find the first's record still there, and on a mux
# domain did (2026-11-04)
$script:LastAsked = -1
function Wait-Start([string]$Label, [string]$What) {
  $floor = $script:LastAsked
  $start = (Wait-State { param($s) $s.new_tab.start -and $s.new_tab.start.label -eq $Label -and $s.new_tab.start.ended -and [long]$s.new_tab.start.asked -gt $floor } -What $What -TimeoutSec 60).new_tab.start
  $script:LastAsked = [long]$start.asked
  $start
}
# Its row in the next frame after the ask, or the tab itself there
function Test-StartFrames($start, [string]$What) {
  $next = [long]$start.asked + 1
  if ($null -ne $start.shown) {
    if ([long]$start.shown -ne $next) { Add-Failure "${What}: asked after frame $($start.asked), its Starting row first shown in frame $($start.shown), not the next" }
    elseif ([long]$start.ended_frame -le [long]$start.shown) { Add-Failure "${What}: the row ended in frame $($start.ended_frame), not after it showed ($($start.shown))" }
    else { "${What}: the Starting row in the next frame ($($start.shown)), $($start.ended) in frame $($start.ended_frame)" }
  } elseif ($start.ended -eq 'arrived' -and [long]$start.ended_frame -eq $next) {
    "${What}: the tab itself came in the next frame ($next), before its row had a frame to show in"
  } else {
    Add-Failure "${What}: asked after frame $($start.asked), its row never shown and it $($start.ended) in frame $($start.ended_frame)"
  }
}

Invoke-Test {
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfnewtab -Socket $sock -PostedPointer -WaitIdle -TimeoutSec 30 }
  else { Open-TestWindow -ConfigFile $cfg -PostedPointer -WaitIdle -TimeoutSec 30 }
  $s = Wait-State { param($s) $s.frame -gt 0 -and $s.new_tab -and $s.rail -and @($s.rail.rows).Count -eq 1 } -What 'the pad and one row on the rail' -TimeoutSec 60
  $tabCount = { @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).tabs).Count }

  # Where the pad is now: once the window has settled at the size it really is (a window on a mux
  # domain takes its tab's size from the server as it attaches, and the rail with it)
  $padAt = {
    for ($k = 0; $k -lt 10; $k++) {
      $sw = Wait-Settled -What 'the window, for the pad'
      $cr = Get-ClientRect $w
      if ($sw.new_tab -and $sw.size.width -eq $cr.Width -and $sw.size.height -eq $cr.Height) { return @((Get-RectCenter $sw.new_tab.rect), $sw) }
      # The next frame, if one comes: a resize brings one
      try { Wait-State { param($x) $x.frame -gt $sw.frame } -What 'a frame at the window''s size' -TimeoutSec 5 | Out-Null } catch { }
    }
    throw "the state file's size never matched the window's client area ($($cr.Width)x$($cr.Height))"
  }

  # The first tab in Warden's folder, as the window will say it for Here; with -Mux, the server's
  # tab attached first (the window's tab changes to it as it does, which closes an open picker)
  $warden = ConvertTo-Folder $wardenSrc
  if (-not (Wait-Until { @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq $warden }).Count -ge 1 } -TimeoutSec 60)) {
    throw "the first tab never said it's in Warden's src: $((Get-Panes -Window $w -Server:$Mux | ForEach-Object cwd) -join ', ')"
  }
  if ($Mux) { Wait-State { param($s) @($s.panes | Where-Object { $null -ne $_.server_id }).Count -ge 1 } -What 'the server''s tab attached' -TimeoutSec 60 | Out-Null }

  # 1. The pad, lit under the pointer
  $at, $s = & $padAt
  $pad = $s.new_tab.rect
  $px, $py = $at
  "pad: $($pad.x),$($pad.y) $($pad.w)x$($pad.h), zone $($s.new_tab.zone.x),$($s.new_tab.zone.y) $($s.new_tab.zone.w)x$($s.new_tab.zone.h)"
  if ($pad.y -le (@($s.rail.rows)[0].rect.y)) { Add-Failure "the pad at y $($pad.y) isn't below the first row at $(@($s.rail.rows)[0].rect.y)" }
  # The keys it names: the ones the key table binds, none for what nothing's bound to (PITFALLS 119)
  $k = $s.new_tab.keys
  if ($k.picker -ne 'F7' -or $k.reopen -ne 'Ctrl+Shift+T' -or $null -ne $k.shell) { Add-Failure "the pad names the keys $($k | ConvertTo-Json -Compress), not F7, Ctrl+Shift+T and none for a shell" }
  else { "keys: the picker $($k.picker), Reopen $($k.reopen), a shell none" }
  Send-Mouse $w Move $px $py
  $s = Wait-Change { param($s) $s.new_tab.hover } -What 'the pad lit under the pointer'

  # 2. A click opens the picker in the next frame. Its Here row is the open tab's folder as the
  # window last heard it (a mux domain's pane says it in its own time): opened again, Esc between,
  # until it names Warden, 30 s at most
  $deadline = [Diagnostics.Stopwatch]::StartNew()
  while ($true) {
    Send-Click $w $px $py
    $s = Wait-Change { param($s) $s.new_tab.picker -and $null -ne $s.new_tab.picker.shown } -What 'the picker open' -TimeoutSec 30
    $p = $s.new_tab.picker
    if ([long]$p.shown -ne [long]$p.asked + 1) { Add-Failure "the picker was opened after frame $($p.asked) and first drawn in frame $($p.shown), not the next" }
    else { "picker: opened after frame $($p.asked), drawn in the next, $($p.shown)" }
    if (@($p.rows)[0].group -eq 'here' -or $deadline.Elapsed.TotalSeconds -gt 30) { break }
    "picker: no Here row yet (the window hasn't heard the tab's folder), again"
    Send-Key $w Escape
    Wait-Change { param($s) -not $s.new_tab.picker } -What 'Esc closing the picker' | Out-Null
  }
  $rows = @($p.rows)
  if ($rows[0].group -ne 'here' -or $rows[0].name -ne 'Warden') { Add-Failure "the first row is $($rows[0].name) ($($rows[0].group)), not Here: Warden" }
  Test-Spelled $rows[0].dir $wardenSrc "Here's folder (from the pane's file:// URL)"
  if ($p.highlight -ne 0) { Add-Failure "the highlight is on row $($p.highlight), not Here's" }
  if (-not $s.new_tab.open) { Add-Failure "the pad isn't lit with its picker open" }
  (Get-Shot $w -Name 'picker').Dispose()

  # 3. "war": Warden highlighted in every frame that shows it in the box
  $wrong = [Collections.Generic.List[string]]::new()
  $check = { param($sw)
    $q = $sw.new_tab.picker
    if ($q -and $q.query -eq 'war') {
      $h = @($q.rows)[$q.highlight]
      if ($h.name -ne 'Warden' -or (@($h.marks) -join ',') -ne '0,1,2') { $wrong.Add("frame $($sw.frame): $($h.name) [$(@($h.marks) -join ',')]") }
    }
  }
  Send-Words $w 'war'
  $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'war' } -What '"war" in the box' -OnState $check
  & $check $s
  if ($wrong.Count) { Add-Failure "with ""war"" typed, the highlight wasn't Warden's: $($wrong -join '; ')" }
  else { 'war: Warden highlighted, its first three letters marked' }

  # 4. Esc closes it; the action opens it again
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.new_tab.picker -and -not $s.new_tab.open } -What 'Esc closing the picker' | Out-Null
  # Reopen with nothing closed yet: the strip says so, and nothing opens or fires
  $tabs0 = & $tabCount
  Send-Emit 'newtab-reopen' -Window $w
  # -Animating: a notice is a wait till it goes (4 s), so a window showing one is never settled (STATE.md)
  Wait-Change { param($s) $s.strip.notice -eq 'Nothing closed to reopen' } -Animating -What "Reopen with nothing kept, said in the strip" | Out-Null
  if ((& $tabCount) -ne $tabs0 -or (Test-Path $reopens)) { Add-Failure 'Reopen with nothing kept opened a tab or fired lbterm-reopen-tab' } else { 'Reopen with nothing kept: the strip said so' }
  Send-Emit 'newtab-picker' -Window $w
  $s = Wait-Change { param($s) $s.new_tab.picker -and $s.new_tab.picker.query -eq '' } -What "LbtermNewTab 'Picker' opening it"

  # 5. The first message, and Enter: the event, the Starting row in the next frame, then the tab
  Send-Words $w 'bargain hunter find a kettle'
  $s = Wait-Change { param($s) $s.new_tab.picker.message -eq 'find a kettle' } -What 'the first message'
  (Get-Shot $w -Name 'picker-typed').Dispose()
  $h = @($s.new_tab.picker.rows)[$s.new_tab.picker.highlight]
  if ($h.name -ne 'Bargain Hunter') { Add-Failure "typed ""bargain hunter find a kettle"", the highlight is $($h.name)" }
  $active0 = $s.active_tab
  Send-Key $w Enter
  $e = Wait-Event 'Enter'
  "event: $($e | ConvertTo-Json -Compress)"
  if ($e.name -ne 'Bargain Hunter' -or $e.action -ne 'new' -or $e.prompt -ne 'find a kettle' -or $e.behind -or $e.source -ne 'picker') { Add-Failure "Enter fired $($e | ConvertTo-Json -Compress)" }
  Test-Spelled $e.dir $names['Bargain Hunter'] "Enter's event's dir"
  Test-Spelled $e.project_dir $names['Bargain Hunter'] "Enter's event's project_dir"
  $start = Wait-Start 'Bargain Hunter' 'the Starting row for Bargain Hunter to end'
  Test-StartFrames $start 'Enter'
  if ($start.ended -ne 'arrived') { Add-Failure "the start for Bargain Hunter $($start.ended)" }
  $s = Wait-State { param($s) -not (@($s.rail.rows) | Where-Object pending) -and $s.active_tab -ne $active0 } -What 'the new tab in front' -TimeoutSec 60
  if ($s.new_tab.picker) { Add-Failure 'the picker stayed open after a pick in front' }
  $ok = Wait-Until { @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq (ConvertTo-Folder $names['Bargain Hunter']) }).Count -ge 1 } -TimeoutSec 30
  if (-not $ok) { Add-Failure "no pane opened in Bargain Hunter's folder: $((Get-Panes -Window $w -Server:$Mux | ForEach-Object cwd) -join ', ')" } else { 'the tab opened in Bargain Hunter''s folder' }
  if (-not (Wait-Until { (Test-Path $recent) -and ((Get-Content $recent -Raw) -match 'Bargain Hunter') } -TimeoutSec 15)) { Add-Failure "the pick wasn't kept in $recent" }

  # 5b. Enter on the Here row, highlighted as the picker opens, and Warden's name typed while it's
  # Here: each fires Warden with dir its src, in Windows' spelling, and project_dir Warden's own
  # folder, and opens there. The window's open tab is Bargain Hunter's now: back to the first
  $first = @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq $warden })[0].pane_id
  Invoke-Cli activate-pane --pane-id $first -Window $w -Server:$Mux | Out-Null
  foreach ($typed in '', 'warden') {
    $what = if ($typed) { "Enter on ""$typed"" while it's Here" } else { 'Enter on the Here row' }
    Wait-Settled -What "the window before $what" | Out-Null
    $at, $null = & $padAt
    $px, $py = $at
    Send-Mouse $w Move $px $py
    Send-Click $w $px $py
    $s = Wait-Change { param($s) $s.new_tab.picker -and @($s.new_tab.picker.rows)[0].group -eq 'here' } -What "the picker with its Here row, for $what" -TimeoutSec 30
    if ($typed) {
      Send-Words $w $typed
      $s = Wait-Change { param($s) $s.new_tab.picker.query -eq $typed } -What """$typed"" in the box"
    }
    $h = @($s.new_tab.picker.rows)[$s.new_tab.picker.highlight]
    if ($h.name -ne 'Warden') { Add-Failure "${what}: the highlight is $($h.name), not Warden" }
    $src0 = @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq $warden }).Count
    Send-Key $w Enter
    $e = Wait-Event $what
    if ($e.name -ne 'Warden' -or $e.action -ne 'new') { Add-Failure "$what fired $($e | ConvertTo-Json -Compress)" }
    Test-Spelled $e.dir $wardenSrc "${what}'s dir"
    Test-Spelled $e.project_dir $names['Warden'] "${what}'s project_dir"
    Wait-Start 'Warden' "the tab for $what" | Out-Null
    if (-not (Wait-Until { @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq $warden }).Count -gt $src0 } -TimeoutSec 30)) { Add-Failure "${what}: no tab opened in Warden's src" }
    else { "${what}: Warden, dir $($e.dir), project_dir $($e.project_dir), a tab there" }
  }

  # 6. A digit picks its row with the box empty
  Wait-Settled -What 'the window after the new tab' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py
  $s = Wait-Change { param($s) $s.new_tab.picker } -What 'the picker again'
  $second = @($s.new_tab.picker.rows)[1].name
  $tabs0 = & $tabCount
  Send-Key $w '2'
  $e = Wait-Event 'the digit 2'
  if ($e.name -ne $second -or $e.action -ne 'new') { Add-Failure "2 picked $($e.name) ($($e.action)), not the second row, $second" } else { "2: picked $second" }
  Wait-Start $second "the tab for $second" | Out-Null
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What "the tab for $second" -TimeoutSec 60 | Out-Null

  # 7. Shift+click: behind, the picker staying open
  Wait-Settled -What 'the window after the digit' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py
  Wait-Change { param($s) $s.new_tab.picker } -What 'the picker for Shift+click' | Out-Null
  Send-Words $w 'mus'
  $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'mus' -and @($s.new_tab.picker.rows)[0].rect } -What '"mus" in the box'
  $tabs0 = & $tabCount
  $active0 = $s.active_tab
  $rx, $ry = Get-RectCenter @($s.new_tab.picker.rows)[0].rect
  Send-Mouse $w Move $rx $ry
  Send-Mouse $w LDown $rx $ry -Keys L, Shift
  Send-Mouse $w LUp $rx $ry -Keys Shift
  $e = Wait-Event 'Shift+click'
  if ($e.name -ne 'Musica' -or -not $e.behind) { Add-Failure "Shift+click fired $($e | ConvertTo-Json -Compress)" }
  $start = Wait-Start 'Musica' 'the start behind to end'
  if (-not $start.behind) { Add-Failure "the start for Musica isn't marked behind" }
  $s = Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What 'the tab behind' -TimeoutSec 60
  if ($s.active_tab -ne $active0) { Add-Failure "a tab opened behind moved the window from tab $active0 to $($s.active_tab)" } else { 'behind: the window stayed on its tab' }
  if (-not $s.new_tab.picker) { Add-Failure 'the picker closed after Shift+click' }
  elseif ($s.new_tab.picker.query -ne '' -or $s.new_tab.picker.started -ne 'Started Musica') { Add-Failure "after Shift+click the picker says query '$($s.new_tab.picker.query)', '$($s.new_tab.picker.started)'" }

  # 7b. Two starts at once: Later Shift+clicked twice, its handler opening each tab 3 s after its
  # pick, so the second is asked while the first waits. Each record ends with a tab of its own,
  # the first's the first to come, and the window stays on its tab (PITFALLS 120)
  $s = Wait-Settled -What 'the picker before Later'
  $tabs0 = @($s.tabs).Count
  $active0 = $s.active_tab
  $last = if ($s.new_tab.start) { [long]$s.new_tab.start.id } else { 0 }
  foreach ($k in 1, 2) {
    Send-Words $w 'later'
    $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'later' -and @($s.new_tab.picker.rows)[0].rect } -What """later"" in the box ($k)"
    $rx, $ry = Get-RectCenter @($s.new_tab.picker.rows)[0].rect
    Send-Mouse $w Move $rx $ry
    Send-Mouse $w LDown $rx $ry -Keys L, Shift
    Send-Mouse $w LUp $rx $ry -Keys Shift
    $e = Wait-Event "Later's Shift+click ($k)"
    if ($e.name -ne 'Later' -or -not $e.behind) { Add-Failure "Later's Shift+click $k fired $($e | ConvertTo-Json -Compress)" }
    Wait-Change { param($s) $s.new_tab.picker.query -eq '' } -What "the box cleared after Later's Shift+click $k" | Out-Null
  }
  $s = Wait-State { param($s) @($s.new_tab.starts | Where-Object { [long]$_.id -gt $last -and $_.label -eq 'Later' -and $_.ended }).Count -ge 2 } -What 'both of Later''s starts ended' -TimeoutSec 60
  $both = @($s.new_tab.starts | Where-Object { [long]$_.id -gt $last -and $_.label -eq 'Later' } | Sort-Object { [long]$_.id })
  "two starts: $(@($both | ForEach-Object { "$($_.id) asked $($_.asked) $($_.ended) $($_.ended_frame) tab $($_.tab)" }) -join '; ')"
  if ([long]$both[1].asked -ge [long]$both[0].ended_frame) { Add-Failure "Later's second start was asked (frame $($both[1].asked)) after the first had ended (frame $($both[0].ended_frame)): they never waited at once" }
  if (@($both | Where-Object { $_.ended -ne 'arrived' }).Count) { Add-Failure "a start for Later $(@($both | ForEach-Object ended) -join ', '), not arrived" }
  elseif ($both[0].tab -eq $both[1].tab) { Add-Failure "both of Later's starts took tab $($both[0].tab)" }
  elseif ([long]$both[0].ended_frame -gt [long]$both[1].ended_frame) { Add-Failure "the second start's tab came first: frames $($both[0].ended_frame), $($both[1].ended_frame)" }
  else { "two starts at once: tabs $($both[0].tab) and $($both[1].tab), in the order asked" }
  $s = Wait-State { param($s) @($s.tabs).Count -ge $tabs0 + 2 } -What 'Later''s two tabs' -TimeoutSec 60
  if ($s.active_tab -ne $active0) { Add-Failure "Later's tabs behind moved the window from tab $active0 to $($s.active_tab)" }

  # 8. Ctrl+click: a shell
  $s = Wait-Settled -What 'the picker after the tab behind'
  $i = Get-PickerRow $s 'Terminal'
  $rect = @($s.new_tab.picker.rows)[$i].rect
  if (-not $rect) { throw "Terminal's row isn't shown: $(@($s.new_tab.picker.rows | ForEach-Object name) -join ', ')" }
  $rx, $ry = Get-RectCenter $rect
  $tabs0 = & $tabCount
  Send-Mouse $w Move $rx $ry
  Send-Mouse $w LDown $rx $ry -Keys L, Ctrl
  Send-Mouse $w LUp $rx $ry -Keys Ctrl
  $e = Wait-Event 'Ctrl+click'
  if ($e.name -ne 'Terminal' -or $e.action -ne 'shell') { Add-Failure "Ctrl+click fired $($e | ConvertTo-Json -Compress)" } else { 'Ctrl+click: shell in Terminal' }
  Wait-Start 'Terminal' 'the shell in Terminal' | Out-Null
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What 'the shell in Terminal' -TimeoutSec 60 | Out-Null

  # 9. Nothing: the handler takes the tab back
  Wait-Settled -What 'the window after the shell' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py
  Wait-Change { param($s) $s.new_tab.picker } -What 'the picker for Nothing' | Out-Null
  Send-Words $w 'nothing'
  Wait-Change { param($s) $s.new_tab.picker.query -eq 'nothing' } -What '"nothing" in the box' | Out-Null
  $tabs0 = & $tabCount
  Send-Key $w Enter
  Wait-Event 'Nothing' | Out-Null
  $start = Wait-Start 'Nothing' 'the Starting row for Nothing to fold'
  if ($start.ended -ne 'folded') { Add-Failure "the start for Nothing $($start.ended), not folded" }
  $s = Wait-State { param($s) $s.strip.notice -eq 'Nothing opened for Nothing' -and -not (@($s.rail.rows) | Where-Object pending) } -What "the strip's notice" -TimeoutSec 30
  if (@($s.tabs).Count -ne $tabs0) { Add-Failure "a tab opened for Nothing" } else { 'Nothing: the row folded, the strip said so' }

  # 9b. Silent: handled, and nothing opens. The row folds once its 20 s are up, never before, by the
  # GUI's clock, and the first frame the state file says shows it folded is no earlier; the strip's
  # notice, put up as it folds, is gone never before its 4 s. Both may come late on a loaded PC
  Wait-Settled -What 'the window after Nothing' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py
  Wait-Change { param($s) $s.new_tab.picker } -What 'the picker for Silent' | Out-Null
  Send-Words $w 'silent'
  Wait-Change { param($s) $s.new_tab.picker.query -eq 'silent' } -What '"silent" in the box' | Out-Null
  $tabs0 = & $tabCount
  Send-Key $w Enter
  Wait-Event 'Silent' | Out-Null
  $floor = $script:LastAsked
  # A frame's time is the state file's, not its window's (time_ms is the root's): a predicate gets
  # both, the window first
  $script:SeenAtMs = $null
  $seen = Wait-State { param($sw, $s)
    $ok = $sw.new_tab.start -and $sw.new_tab.start.label -eq 'Silent' -and $sw.new_tab.start.ended -and [long]$sw.new_tab.start.asked -gt $floor
    if ($ok) { $script:SeenAtMs = [long]$s.time_ms }
    $ok } -What "Silent's row to fold, at 20 s or later" -TimeoutSec 90
  $seen | Add-Member -NotePropertyName time_ms -NotePropertyValue $script:SeenAtMs -Force
  $start = $seen.new_tab.start
  $script:LastAsked = [long]$start.asked
  $waited = [long]$start.ended_ms - [long]$start.asked_ms
  if ($start.ended -ne 'folded') { Add-Failure "Silent's start $($start.ended), not folded" }
  elseif ($waited -lt 20000) { Add-Failure "Silent's row folded $waited ms after it was asked for, before its 20 s" }
  elseif ([long]$seen.time_ms -lt [long]$start.asked_ms + 20000) { Add-Failure "the frame showing Silent's row folded was at $($seen.time_ms), $([long]$seen.time_ms - [long]$start.asked_ms) ms after it was asked for" }
  else { "Silent: the row folded $waited ms after it was asked for (20 s at the least)" }
  $script:GoneAtMs = $null
  Wait-State { param($sw, $s)
    $ok = $sw.strip.notice -ne 'Nothing opened for Silent'
    if ($ok) { $script:GoneAtMs = [long]$s.time_ms }
    $ok } -What "the strip's notice for Silent gone" -TimeoutSec 30 | Out-Null
  $shownFor = $script:GoneAtMs - [long]$start.ended_ms
  if ($shownFor -lt 4000) { Add-Failure "the strip's notice for Silent was gone $shownFor ms after it went up, before its 4 s" }
  else { "Silent: the strip's notice gone $shownFor ms after it went up (4 s at the least)" }
  if ((& $tabCount) -ne $tabs0) { Add-Failure 'a tab opened for Silent' }

  # 10. The + menu, on a right click
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py -Right
  $s = Wait-Change { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu'
  $labels = @($s.menu.items | ForEach-Object label)
  "+ menu: $($labels -join ' | ')"
  foreach ($want in 'PowerShell 7 here', 'Command Prompt here', "Shell in a project$([char]0x2026)", 'Nothing closed yet', 'New window') {
    if ($labels -notcontains $want) { Add-Failure "the + menu has no '$want': $($labels -join ', ')" }
  }
  if ($s.menu.rect.y + $s.menu.rect.h -gt $py + 2) { Add-Failure "the + menu runs below the pointer: it opens upward" }
  (Get-Shot $w -Name 'plus-menu').Dispose()
  $tabs0 = & $tabCount
  $item = @($s.menu.items | Where-Object label -eq 'PowerShell 7 here')[0]
  $ix, $iy = Get-RectCenter $item.rect
  Send-Click $w $ix $iy -Move
  Wait-Start 'PowerShell 7' 'PowerShell 7 here' | Out-Null
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 -and -not $s.menu } -What 'PowerShell 7 here' -TimeoutSec 60 | Out-Null
  'PowerShell 7 here: a tab'

  # 11. Shell in a project…
  Wait-Settled -What 'the window after PowerShell 7 here' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py -Right
  $s = Wait-Change { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu again'
  $item = @($s.menu.items | Where-Object label -eq "Shell in a project$([char]0x2026)")[0]
  $ix, $iy = Get-RectCenter $item.rect
  $flashed = [System.Collections.Generic.HashSet[string]]::new()
  Send-Click $w $ix $iy -Move
  $s = Wait-Change { param($s) $s.new_tab.picker.mode -eq 'shell' } -What 'the picker titled for a shell' -OnState { param($s) foreach ($a in @($s.animations)) { [void]$flashed.Add($a.name) } }
  # The + menu's row picked flashes white as it goes, as the picker's rows do (Lachlan's 9b). This
  # config leaves motion to Windows (`system`), so where Windows' animations are off, or the PC is
  # locked (PITFALLS 152), nothing plays, the + menu's fade and the picker's included: the flash is
  # asked for only where the window played the + menu's fade out or the picker's in
  if (-not ($flashed.Contains('menu.list') -or $flashed.Contains('newtab.picker'))) { "+ menu: no motion played here (Windows' animations off or the PC locked), so no flash to see, and none judged here: menu-keys.ps1's step 11 judges the + menu's flash with motion on [$(@($flashed) -join ', ')]" }
  elseif (-not $flashed.Contains('menu.list.flash')) { Add-Failure "Shell in a project… didn't flash as it was picked: no menu.list.flash in [$(@($flashed) -join ', ')]" } else { '+ menu: the row picked flashed (menu.list.flash)' }
  if ($s.new_tab.picker.footer -notlike 'Enter shell*') { Add-Failure "the shell's picker says '$($s.new_tab.picker.footer)'" } else { "Shell in a project: '$($s.new_tab.picker.footer)'" }
  (Get-Shot $w -Name 'shell-picker').Dispose()
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.new_tab.picker } -What 'Esc closing it' | Out-Null

  # 12. Reopen. A tab opened in front from the picker (Bargain Hunter again), closed by hand (F6,
  # CloseCurrentTab, as Ctrl+W), is kept: the state file's new_tab.closed has its label, folder and
  # project, and the + menu offers it
  $bh = ConvertTo-Folder $names['Bargain Hunter']
  $inBh = { @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq $bh }).Count }
  $bh0 = & $inBh
  Wait-Settled -What 'the window after the shell picker' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py
  Wait-Change { param($s) $s.new_tab.picker } -What 'the picker for a tab to close' | Out-Null
  Send-Words $w 'barg'
  $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'barg' } -What '"barg" in the box'
  $active0 = $s.active_tab
  Send-Key $w Enter
  Wait-Event 'Enter on "barg"' | Out-Null
  Wait-Start 'Bargain Hunter' 'the second tab in Bargain Hunter' | Out-Null
  Wait-State { param($s) $s.active_tab -ne $active0 -and -not (@($s.rail.rows) | Where-Object pending) } -What 'the second Bargain Hunter tab in front' -TimeoutSec 60 | Out-Null
  # The tab's folder known to the window before it closes, as the pane says it
  if (-not (Wait-Until { (& $inBh) -gt $bh0 } -TimeoutSec 30)) { throw "the second tab never said it's in Bargain Hunter's folder" }
  # On a mux domain the tab's title comes with the server's push, after the window has settled
  # without it ('wezterm', the client pane's before it): wait for the shell's own
  Wait-State { param($s) @($s.rail.rows | Where-Object active)[0].label -eq 'pwsh' } -What "the tab to close titled pwsh" -TimeoutSec 30 | Out-Null
  $s = Wait-Settled -What 'the window with the tab to close'
  $label = @($s.rail.rows | Where-Object active)[0].label
  $tabs0 = @($s.tabs).Count
  Send-Key $w F6
  $s = Wait-Change { param($s) @($s.tabs).Count -eq $tabs0 - 1 -and @($s.new_tab.closed).Count -ge 1 } -What 'the tab closed and kept'
  $kept = @($s.new_tab.closed)[0]
  "closed: $($kept | ConvertTo-Json -Compress)"
  if ($kept.label -ne $label) { Add-Failure "the tab closed was '$label' on the rail, kept as '$($kept.label)'" }
  if ((ConvertTo-Folder $kept.dir) -ne $bh) { Add-Failure "the tab closed was kept in '$($kept.dir)', not Bargain Hunter's folder" }
  if ($kept.project -ne 'Bargain Hunter') { Add-Failure "the tab closed was kept in project '$($kept.project)'" }
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py -Right
  $s = Wait-Change { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu with a tab to reopen'
  $labels = @($s.menu.items | ForEach-Object label)
  if ($labels -notcontains "Reopen $label") { Add-Failure "the + menu doesn't offer 'Reopen $label': $($labels -join ' | ')" }
  if ($labels -contains 'Nothing closed yet') { Add-Failure "the + menu says nothing's closed with '$label' kept" }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'Esc closing the + menu' | Out-Null

  # LbtermNewTab 'Reopen' (Ctrl+Shift+T in Lachlan's config): lbterm-reopen-tab with it, the
  # Starting row in the next frame, the tab in its folder in front, and it's no longer kept
  $tabs0 = & $tabCount
  Send-Emit 'newtab-reopen' -Window $w
  if (-not (Wait-Until { (Test-Path $reopens) -and @(Get-Content $reopens | Where-Object { $_ }).Count -ge 1 } -TimeoutSec 30)) { throw 'no lbterm-reopen-tab event for LbtermNewTab ''Reopen''' }
  $r = @(Get-Content $reopens | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json })[0]
  "reopen: $($r | ConvertTo-Json -Compress)"
  if ($r.label -ne $label -or (ConvertTo-Folder $r.dir) -ne $bh -or $r.project -ne 'Bargain Hunter') { Add-Failure "LbtermNewTab 'Reopen' fired $($r | ConvertTo-Json -Compress)" }
  $start = Wait-Start $label "the Starting row reopening '$label'"
  Test-StartFrames $start 'Reopen'
  if ($start.ended -ne 'arrived') { Add-Failure "the reopen $($start.ended)" }
  $s = Wait-State { param($s) @($s.tabs).Count -eq $tabs0 + 1 -and @($s.new_tab.closed).Count -eq 0 } -What 'the tab reopened, no longer kept' -TimeoutSec 60
  if (-not (Wait-Until { (& $inBh) -gt $bh0 } -TimeoutSec 30)) { Add-Failure "the tab reopened isn't in Bargain Hunter's folder: $((Get-Panes -Window $w -Server:$Mux | ForEach-Object cwd) -join ', ')" }
  else { "Reopen: '$label' again in Bargain Hunter's folder" }

  # The + menu's Reopen row does the same: the reopened tab closed again, picked from the menu
  Wait-State { param($s) @($s.rail.rows | Where-Object active)[0].label -eq 'pwsh' } -What "the reopened tab titled pwsh" -TimeoutSec 30 | Out-Null
  $s = Wait-Settled -What 'the window with the tab reopened'
  $label = @($s.rail.rows | Where-Object active)[0].label
  $tabs0 = @($s.tabs).Count
  Send-Key $w F6
  Wait-Change { param($s) @($s.tabs).Count -eq $tabs0 - 1 -and @($s.new_tab.closed).Count -eq 1 } -What 'the reopened tab closed again' | Out-Null
  $at, $null = & $padAt
  $px, $py = $at
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py -Right
  $s = Wait-Change { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu for its Reopen row'
  $item = @($s.menu.items | Where-Object label -eq "Reopen $label")[0]
  if (-not $item) { throw "the + menu has no 'Reopen $label': $(@($s.menu.items | ForEach-Object label) -join ' | ')" }
  $ix, $iy = Get-RectCenter $item.rect
  Send-Click $w $ix $iy -Move
  if (-not (Wait-Until { @(Get-Content $reopens | Where-Object { $_ }).Count -ge 2 } -TimeoutSec 30)) { throw 'no lbterm-reopen-tab event for the + menu''s Reopen row' }
  $start = Wait-Start $label "the Starting row reopening '$label' from the menu"
  Test-StartFrames $start 'the + menu''s Reopen'
  Wait-State { param($s) @($s.tabs).Count -eq $tabs0 -and @($s.new_tab.closed).Count -eq 0 -and -not $s.menu } -What 'the tab reopened from the menu' -TimeoutSec 60 | Out-Null
  'the + menu''s Reopen row: the tab again'

  # 13. Twenty tabs, every one on the rail above the pad, none said hidden: the window made 1,000 px
  # tall (NEW-TAB.md's cargo test holds 20 in 900), the tabs added behind by the cli, cmd.exe in each
  $wr = Get-WindowRect $w
  Set-WindowSize $w ($wr.Right - $wr.Left) 1000
  $first = @(Get-Panes -Window $w -Server:$Mux)[0].pane_id
  for ($k = & $tabCount; $k -lt 20; $k++) { New-Tab -From $first -NoActivate -Program @('cmd.exe') -Window $w -Server:$Mux | Out-Null }
  Wait-State { param($s) @($s.tabs).Count -eq 20 -and @($s.rail.rows | Where-Object { -not $_.pending }).Count -eq 20 } -What 'twenty tabs on the rail' -TimeoutSec 60 | Out-Null
  $cr = Get-ClientRect $w
  $s = Wait-State { param($s) $s.size.height -eq $cr.Height -and (Test-StateSettled $s) } -What 'the window settled at its new size' -TimeoutSec 30
  $rows = @($s.rail.rows | Where-Object { -not $_.pending })
  $pad = $s.new_tab.rect
  $off = @($rows | Where-Object { -not $_.visible -or ($_.pad.y + $_.pad.h) -gt $pad.y })
  if ($off.Count) { Add-Failure "with 20 tabs in $($cr.Height) px, $($off.Count) rows aren't drawn above the pad (at y $($pad.y)): $(@($off | ForEach-Object { "row $($_.row) at $($_.pad.y)" }) -join ', ')" }
  elseif ($s.new_tab.hidden -ne 0) { Add-Failure "with 20 tabs all drawn, the pad says +$($s.new_tab.hidden)" }
  else { "20 tabs: every one on the rail, the last at y $(@($rows)[-1].pad.y), the pad at $($pad.y)" }
  (Get-Shot $w -Name 'twenty-tabs').Dispose()

  # 14. A paste into the picker's box, from what the window copied: with the state file on, its
  # copied is the window's clipboard both ways, so nothing here reads or writes Lachlan's
  $want = "bargain`r`nfind a kettle"
  Wait-Settled -What 'the window, for the copy' | Out-Null
  Send-Key $w F3
  Wait-State { param($s) $s.copied -eq $want } -What 'F3 copying two lines to the state file' -TimeoutSec 30 | Out-Null
  Send-Key $w F7
  $s = Wait-Change { param($s) $s.new_tab.picker -and $s.new_tab.picker.query -eq '' } -What 'F7 opening the picker, for the paste'
  # F4 is a paste key, but not Ctrl+V: since his 4b of 2026-11-08 only Ctrl+V pastes into the box
  # (a posted key can't carry Ctrl, so newtab::pastes_into_picker's test holds that), and F4 is a
  # key the picker has no use for, which lights its box (his 3a): the compositor's Light::Pad, a
  # white glint over the box on copy_in and copy_out, which the state file's animations name
  # light.glint while it plays (the box's caret blinks, so a capture can't tell it apart)
  Wait-Settled -What 'the picker, before F4' | Out-Null
  $f0 = Get-StateFrame $w
  Send-Key $w F4
  $glint = try {
    Wait-State { param($s) @($s.animations | ForEach-Object name) -contains 'light.glint' } -AfterFrame $f0 -What "F4's light on the picker's box" -TimeoutSec 5
  } catch { $null }
  (Get-Shot $w -Name 'paste-key').Dispose()
  Wait-Ms 200
  $q = (Read-State).windows[0].new_tab.picker
  "14. F4 over the picker: '$($q.query)' in the box$(if ($glint) { ', its box lit (light.glint)' })"
  if ($q.query -ne '') { Add-Failure "F4, a paste key that isn't Ctrl+V, pasted '$($q.query)' into the picker's box: only Ctrl+V does (his 4b)" }
  if (-not $glint) { Add-Failure "F4 over the picker didn't light its box (his 3a): no light.glint in the state file's animations within 5 s" }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.new_tab.picker -and -not $s.new_tab.open } -What 'Esc closing the picker after the paste' | Out-Null
  # Over the + menu: nothing
  $at, $sw = & $padAt
  $px, $py = $at
  $tabs0 = & $tabCount
  Send-Mouse $w Move $px $py
  Send-Click $w $px $py -Right
  Wait-Change { param($s) $s.menu.kind -eq 'plus' } -What 'the + menu, for F4' | Out-Null
  Send-Key $w F4
  $s = Wait-Settled -What 'the window after F4 over the + menu'
  if ($s.menu.kind -ne 'plus') { Add-Failure "F4 over the + menu closed it" }
  elseif ($s.new_tab.picker) { Add-Failure "F4 over the + menu opened the picker, its box '$($s.new_tab.picker.query)'" }
  elseif ((& $tabCount) -ne $tabs0) { Add-Failure "F4 over the + menu opened a tab: $tabs0 became $(& $tabCount)" }
  else { "   F4 over the + menu: nothing, the menu still open, still $tabs0 tabs" }
  Send-Key $w Escape
  Wait-Change { param($s) -not $s.menu } -What 'Esc closing the + menu after F4' | Out-Null
} -Passed 'new-tab: the pad lights, opens the picker in the next frame and its + menu upward; typing finds the project, Enter, a digit, Shift+click and Ctrl+click fire lbterm-project-action as they should, and the Starting row shows in the next frame until the tab comes into it, or folds with the strip saying so; a tab closed by hand is kept and Reopen opens it again in its folder, from the action and the + menu; twenty tabs all fit on the rail above the pad; a paste key pastes the copied text''s first line into the picker''s box and its search runs, and does nothing over the + menu'
