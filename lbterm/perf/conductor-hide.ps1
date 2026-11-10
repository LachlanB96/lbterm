#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/conductor_hide.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/state_files.rs, lbterm-extras/src/conductor/hide.rs, lbterm-extras/src/conductor.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/state.rs, lbterm-panel/src/look.rs, lbterm-panel/src/words.rs, lbterm-panel/src/layout.rs, lbterm-panel/src/palette.rs, lbterm-extras/src/seat/layout.rs, lbterm-extras/src/seat/protocol.rs, wezterm-gui/src/termwindow/render/conductor_view.rs, lbterm-motion/src/send.rs, wezterm-gui/src/termwindow/compositor.rs
<#
The tab menu's Hide from Conductor and Show to Conductor (lbterm/NEW-TAB.md, section 8; landing
3), end to end in a test window with a config of its own: no Lachlan's lbterm.lua, so no handler of
his ever runs, and the Conductor's button.json is a stand-in in target\perf\conductor-hide that
this script writes, playing the Conductor. The window's `lbterm-conductor-hide` handler only
writes each pick down (hide.jsonl); nothing here runs `conductor`, reads the real button.json or
hides a real session.

Three tabs: a shell, "Tab Wave" (one stand-in Claude session, A, which lights its claude_status
orange once the script says so, as claudehook.exe does for a question) and "Pair" (two panes,
sessions B and C, as claudehook.exe's `claude_session` user var says). The Conductor's seat is
played on a pipe of its own for the view (step 6), as perf/conductor-view.ps1 plays it.

1. button.json without `hidden` (a Conductor from before 2026-11-02): no Hide row on the Claude
   tab's menu, and H goes nowhere (the card's edge flares, the menu stays).
2. With `hidden: []`: "Hide from Conductor (H)" on Tab Wave's menu, before Wait on…'s place;
   none on the shell's.
3. H on Tab Wave's menu: the event fires with `{sessions = {A}, label = 'Tab Wave', hide = true}`
   and the tab's own pane, and from the next frames the row is `hidden`, `hidden_pending`. A light
   runs up the rail from Tab Wave's pad to the Conductor's (`light.send`, Lachlan's 6a), setting
   off only once the menu's card has gone (the rail no longer `held`, his 7b), with its whole run
   (`send`'s 600 ms and `copy_out`'s 260) ahead of it. The script writes A into `hidden`: the row
   stays hidden, no longer pending, and the pad glints (`light.glint`).
   The look (NEW-TAB.md, section 8), from captures: on the closed rail the eye-off mark on Tab
   Wave's pad's bottom-right corner, a disc of the rail's surface cut out of the pad with a
   neutral grey glyph (white at 60%) centred on it, and no mark on the shell's pad
   (hidden-rail.png against shown-rail.png); Tab Wave lit orange as waiting on him still has its
   orange pad, the mark on it (hidden-waiting.png); on the open panel the "Hidden" chip on its row
   in the project chip's fill, its word white at 45%, none on the shell's row (hidden-open.png), and the
   mark still there with the row under the pointer, over its highlight (hidden-hovered.png).
4. Tab Wave's menu again says "Show to Conductor"; a click on it fires `hide = false`, the row
   shows at once (`hidden` false, `hidden_pending`), the light runs up the rail as for the hide,
   and the list emptied confirms it with a glint.
5. Pair: Hide fires for both sessions, its active pane's first, one light up the rail. The script writes nothing: the
   row is hidden and pending until 5 s after the key (never sooner; a loaded PC's later is
   allowed), then goes back, and the strip says "The Conductor didn't hide Pair", which goes.
   While it waits Pair's menu has the row greyed, "Hiding from Conductor…", with no key
   (menu-waiting.png), and H or a click on it does nothing, the menu staying open (Lachlan's 11b
   of 2026-11-09): a second
   request would race the first to the Conductor (PITFALLS 244).
6. The Conductor's view (the seat's `seat.view`, its sessions' `hidden`): Tab Wave (A, waiting on
   him) and "Elsewhere" (X, no tab in this window) hidden, a working session not. The Sessions
   card's Hidden group is at its foot, folded, its heading saying in orange "1 waiting on you"
   (view-hidden-folded.png); a press opens it, each hidden session with a Show button
   (view-hidden-open.png). Show on Tab Wave fires `{sessions = {A}, label = 'Tab Wave', hide =
   false, source = 'conductor view'}` with Tab Wave's own pane, and its rail row shows at once,
   pending, a light running up from its pad, until the script's write confirms it; Show on it
   again while it waits fires nothing (PITFALLS 244); Show on Elsewhere fires for X with the
   window's active pane, no row waits and no light runs.

Keys and clicks are posted to the window, which never takes focus. Everything is judged from the
window's state file (lbterm/STATE.md: `rail.rows[].hidden` and `hidden_pending`, `menu.items`,
`strip.notice`, `animations`, `conductor_view.buttons` and `cards`), by frames and order, and the
look from captures; the one time tested (5 s) may come late, never early (PITFALLS 59).

  pwsh -File lbterm\perf\conductor-hide.ps1 [-Bin <dir>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'conductor-hide' -Bin $Bin
$cfg = Join-Path $T.Out 'lbterm.lua'
$state = Join-Path $T.Out 'button.json'
$events = Join-Path $T.Out 'hide.jsonl'
$lightGo = Join-Path $T.Out 'light.go'
Remove-Item -LiteralPath $lightGo -ErrorAction SilentlyContinue
$pipeName = "lbterm-perf-hide-seat-$PID"
$summaries = Join-Path $T.Out 'summaries'
New-Item -ItemType Directory -Force $summaries | Out-Null

[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local encode = (wezterm.serde and wezterm.serde.json_encode) or wezterm.json_encode
-- The test's own handler: each pick written down, and nothing asked of any Conductor. The script
-- plays the Conductor, writing its stand-in button.json, or leaving it as it is
wezterm.on('lbterm-conductor-hide', function(window, pane, h)
  local f = io.open([[$events]], 'a')
  if f then
    f:write(encode({ sessions = h.sessions, label = h.label, hide = h.hide, source = h.source, pane = pane:pane_id() }) .. '\n')
    f:close()
  end
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  -- On, not following Windows: with motion off nothing glints, and the glint is what confirms
  lbterm_motion = { enabled = 'on' },
  lbterm_tab_panel = { enabled = true, open_delay_ms = 0, labels = false },
  -- The fancy tab bar left on (the default), as Lachlan has it: with the panel hiding the tabs it's
  -- the window strip, where the unanswered pick's notice goes (no tab bar, no strip)
  initial_cols = 100,
  initial_rows = 30,
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  lbterm_summaries = '$(ConvertTo-LuaPath $summaries)',
  lbterm_conductor = { state_file = '$(ConvertTo-LuaPath $state)', seat_pipe = [[\\.\pipe\$pipeName]] },
  -- A card colour the script knows, for the look's captures: the rail's surface, under the pads
  colors = { background = '#14141c' },
}
"@)

# The stand-in button.json, as the Conductor writes it (`@Conductor:button`): `hidden` only when
# given (step 1 has none), and a fresh updatedAt each write, as every hide and show writes one
$script:writes = 0
function Write-Button([object]$Hidden) {
  $script:writes++
  $file = [ordered]@{ state = 'on'; mode = 'read'; listening = $false; busy = $false; tooltip = 'Conductor is on (read only)' }
  if ($null -ne $Hidden) { $file.hidden = @($Hidden) }
  $file.updatedAt = "2026-11-09T10:00:$('{0:D2}' -f $script:writes).000Z"
  [IO.File]::WriteAllText($state, ($file | ConvertTo-Json -Compress))
}
# The seat, played: a connection whose first line subscribes is pushed to; any other line with an
# id is a command, answered ok (the view's Show sends none: it's the Lua event's)
if (-not ('LbtermHideSeat' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.IO; using System.IO.Pipes; using System.Threading; using System.Collections.Generic;
using System.Text.RegularExpressions;
public static class LbtermHideSeat {
  static string pipe;
  static readonly List<StreamWriter> subscribers = new List<StreamWriter>();
  public static void Serve(string name) { pipe = name; new Thread(Accept) { IsBackground = true }.Start(); }
  static void Accept() {
    while (true) {
      var s = new NamedPipeServerStream(pipe, PipeDirection.InOut, NamedPipeServerStream.MaxAllowedServerInstances,
        PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
      s.WaitForConnection();
      new Thread(() => Talk(s)) { IsBackground = true }.Start();
    }
  }
  static void Talk(NamedPipeServerStream s) {
    var reader = new StreamReader(s);
    var writer = new StreamWriter(s) { AutoFlush = true, NewLine = "\n" };
    try {
      string line;
      while ((line = reader.ReadLine()) != null) {
        var type = Regex.Match(line, "\"type\":\"([^\"]+)\"").Groups[1].Value;
        if (type == "seat.subscribe") { lock (subscribers) subscribers.Add(writer); continue; }
        var id = Regex.Match(line, "\"id\":\"([^\"]+)\"");
        if (id.Success) writer.WriteLine("{\"type\":\"seat.reply\",\"id\":\"" + id.Groups[1].Value + "\",\"ok\":true}");
      }
    } catch (IOException) { }
    lock (subscribers) subscribers.Remove(writer);
  }
  public static int Push(string line) {
    int n = 0;
    lock (subscribers) foreach (var w in subscribers.ToArray()) { try { w.WriteLine(line); n++; } catch (IOException) { } }
    return n;
  }
}
'@ -ReferencedAssemblies System.IO.Pipes, System.Collections, System.Threading, System.Threading.Thread, System.Text.RegularExpressions
}
function Push-Seat([string]$line) { [LbtermHideSeat]::Push(($line -replace '\r?\n\s*', ' ')) }

# The look, from a capture: within the 12 px square of a pad's hidden mark (centred 3 px in from
# its bottom right corner, the pad 20 px tall at 1x), how many pixels are the pad's colour and how
# many are the mark's glyph, a neutral grey at least 100 bright, and where the glyph's middle is
function Measure-Mark($shot, $pad) {
  $scale = [double]$pad.h / 20
  $cx = $pad.x + $pad.w - 3 * $scale; $cy = $pad.y + $pad.h - 3 * $scale; $half = 6 * $scale
  $box = [System.Drawing.Rectangle]::new([int][Math]::Floor($cx - $half), [int][Math]::Floor($cy - $half), [int][Math]::Ceiling(2 * $half), [int][Math]::Ceiling(2 * $half))
  $padRgb = $shot.GetPixel([int]($pad.x + 3 * $scale), [int]($pad.y + $pad.h / 2)).ToArgb() -band 0xFFFFFF
  $padPx = [LbtermE2E.Px]::CountNear($shot, $box, $padRgb, 12)
  $glyph = 0; $sx = 0.0; $sy = 0.0
  for ($y = $box.Top; $y -lt $box.Bottom; $y++) {
    for ($x = $box.Left; $x -lt $box.Right; $x++) {
      $c = $shot.GetPixel($x, $y)
      $hi = [Math]::Max($c.R, [Math]::Max($c.G, $c.B)); $lo = [Math]::Min($c.R, [Math]::Min($c.G, $c.B))
      if ($lo -ge 100 -and $hi - $lo -le 16) { $glyph++; $sx += $x; $sy += $y }
    }
  }
  [pscustomobject]@{
    PadRgb = '{0:X6}' -f $padRgb; PadPx = $padPx; Glyph = $glyph
    Off = if ($glyph) { [Math]::Round([Math]::Sqrt([Math]::Pow($sx / $glyph + 0.5 - $cx, 2) + [Math]::Pow($sy / $glyph + 0.5 - $cy, 2)), 1) } else { $null }
  }
}

function Get-Events { if (Test-Path $events) { @(Get-Content $events | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json }) } else { @() } }
function Menu-Rows($m) { @($m.menu.items | ForEach-Object { if ($_.key) { "$($_.label) ($($_.key))" } else { $_.label } }) -join ', ' }

$A = 'c0ffee00-0000-4000-8000-00000000000a'
$B = 'c0ffee00-0000-4000-8000-00000000000b'
$C = 'c0ffee00-0000-4000-8000-00000000000c'

Write-Button $null
Invoke-Test {
  [LbtermHideSeat]::Serve($pipeName)
  $w = Open-TestWindow -ConfigFile $cfg -Cwd 'C:\Windows' -WaitIdle -TimeoutSec 15 -PostedPointer
  if (-not (Wait-Until { (Get-Panes).Count -ge 1 } -TimeoutSec 10)) { throw 'the test window has no pane' }
  $win1 = (Get-Panes)[0].window_id
  $first = (Get-Panes)[0].pane_id
  Invoke-Cli set-tab-title --pane-id $first 'shell' | Out-Null
  # Tab Wave: session A, and once light.go is there its claude_status orange (blocked on you), as
  # claudehook.exe lights it for a question
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($A))
  $go = $lightGo -replace "'", "''"
  $wave = "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7); " +
    "while (-not (Test-Path -LiteralPath '$go')) { Start-Sleep -Milliseconds 100 }; " +
    "`$v = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('ff8c00|' + [DateTime]::UtcNow.ToFileTimeUtc())); " +
    "[Console]::Write([char]27 + ']1337;SetUserVar=claude_status=' + `$v + [char]7); Start-Sleep 600"
  $wavePane = New-Tab -From $first -NoActivate -Program (New-PwshProgram $wave)
  Invoke-Cli set-tab-title --pane-id $wavePane 'Tab Wave' | Out-Null
  $pairPane = New-Tab -From $first -NoActivate -Program (New-ClaudeSessionProgram $B)
  $got = (Invoke-Cli split-pane --pane-id $pairPane --right -Program (New-ClaudeSessionProgram $C) -Merge) -join ' '
  if ($got.Trim() -notmatch '^\d+$') { throw "split-pane printed: $got" }
  $pairSecond = [int]$got.Trim()
  Invoke-Cli set-tab-title --pane-id $pairPane 'Pair' | Out-Null
  # Pair's first pane active again, so its sessions go active first: B, then C
  Invoke-Cli activate-pane --pane-id $pairPane | Out-Null
  $tabs = @(Get-Tabs $win1)
  if ($tabs.Count -ne 3) { throw "the window has $($tabs.Count) tabs, not 3" }
  $tabOf = @{}
  foreach ($t in $tabs) { $tabOf[$t.tab_title] = [long]$t.tab_id }

  # Each tab's row, as the rail draws it now
  $rowOf = { param($s, [string]$title) @($s.rail.rows | Where-Object { $_.tab -eq $tabOf[$title] })[0] }
  $s = Wait-Change { param($s) @($s.rail.rows).Count -eq 3 -and $s.rail.conductor } -What 'three rows and the Conductor''s pad'
  # The tab menu on a tab's row: a right click on its pad
  $menu = { param([string]$title)
    $script:e = Wait-Settled -What 'the rail'
    $x, $y = Get-RectCenter (& $rowOf $script:e $title).pad
    Send-Click $w $x $y -Right -Move
    Wait-Change { param($s) $s.menu.kind -eq 'tab' } -What "the tab menu on $title"
  }
  $closeMenu = {
    Send-Key $w Escape
    Wait-Change { param($s) -not $s.menu } -What 'the menu closed' | Out-Null
  }
  # The frame names every frame from here on showed, for the glint
  $script:seen = [Collections.Generic.HashSet[string]]::new()
  $watch = { param($sw) foreach ($a in @($sw.animations)) { [void]$script:seen.Add($a.name) } }
  # The light a pick sends up the rail to the Conductor's pad (his 6a, `light.send`): the first
  # frame after $f naming it, the rail let go (under a menu's card it waits, his 7b), with its
  # whole run still ahead of it, send's 600 ms and copy_out's 260
  $sent = { param([long]$f, [string]$what)
    $s = Wait-State { param($s) @($s.animations | Where-Object name -eq 'light.send').Count } -AfterFrame $f -What "the light sent up the rail for $what"
    $left = (@($s.animations | Where-Object name -eq 'light.send') | Measure-Object left_ms -Maximum).Maximum
    if ($s.rail.held) { Add-Failure "$what's light set off under the menu's card, the rail held (frame $($s.frame))" }
    if ($left -gt 870 -or $left -lt 500) { Add-Failure "$what's light had $left ms to run as it set off, not its 860" }
    [pscustomobject]@{ Frame = $s.frame; Left = $left; Held = [bool]$s.rail.held }
  }
  # The rail closed, the pointer gone from it, settled: for a capture of the closed rail
  $closed = {
    Send-Leave $w
    Wait-Change { param($s) -not $s.rail.open -and -not $s.menu } -What 'the rail closed' -Animating | Out-Null
  }
  # The rail open with the pointer on the shell's pad, settled
  $opened = {
    $x, $y = Get-RectCenter (& $rowOf (Read-State).windows[0] 'shell').pad
    Send-Mouse $w Move $x $y
    Wait-Change { param($s) $s.rail.open -and $s.rail.phase -eq 'open' } -What 'the rail open' -Animating | Out-Null
  }

  # Before anything is hidden: the closed rail, for the look's captures to be held against
  & $closed
  $before = Get-Shot $w -Name 'shown-rail'

  # 1. No `hidden` in the file: no row, and H goes nowhere
  $m = & $menu 'Tab Wave'
  $rows = Menu-Rows $m
  if (@($m.menu.items | Where-Object key -eq 'H').Count) { Add-Failure "a button.json without hidden offered: $rows" }
  else { "1. without hidden: $rows" }
  $f = $m.frame
  Send-Key $w H
  $after = Wait-State { param($s) $true } -AfterFrame $f -What 'a frame after H'
  if (-not $after.menu) { Add-Failure 'H with no Hide row closed the menu' }
  if (@(Get-Events).Count) { Add-Failure "H with no Hide row fired lbterm-conductor-hide: $(Get-Content $events -Raw)" }
  & $closeMenu

  # 2. With hidden: [] the row is on the Claude tab's menu, not the shell's
  Write-Button @()
  Wait-State { param($s) $true } -AfterFrame $s.frame -What 'a frame after the write' | Out-Null
  $m = & $menu 'shell'
  if (@($m.menu.items | Where-Object key -eq 'H').Count) { Add-Failure "the shell tab offered Hide: $(Menu-Rows $m)" }
  & $closeMenu
  $m = & $menu 'Tab Wave'
  $rows = Menu-Rows $m
  $hideRow = @($m.menu.items | Where-Object key -eq 'H')[0]
  if (-not $hideRow -or $hideRow.label -ne 'Hide from Conductor') { Add-Failure "with hidden: [] Tab Wave's menu has [$rows], no 'Hide from Conductor (H)'" }
  else { "2. with hidden []: $rows" }

  # 3. H: the event, the row at once, then the Conductor's write and the glint
  $f = $m.frame
  Send-Key $w H
  $pending = Wait-State { param($s) $r = & $rowOf $s 'Tab Wave'; $r.hidden -and $r.hidden_pending } -AfterFrame $f -What "Tab Wave's row hidden and pending"
  if ($pending.menu) { Add-Failure 'the menu stayed open after Hide' }
  if (-not (Wait-Until { @(Get-Events).Count -ge 1 } -TimeoutSec 15)) { Add-Failure 'Hide fired no lbterm-conductor-hide' }
  else {
    $ev = @(Get-Events)[-1]
    $said = "sessions [$(@($ev.sessions) -join ', ')], label '$($ev.label)', hide $($ev.hide), pane $($ev.pane)"
    if ((@($ev.sessions) -join ',') -ne $A -or $ev.label -ne 'Tab Wave' -or $ev.hide -ne $true -or $ev.pane -ne $wavePane) {
      Add-Failure "Hide fired $said, not sessions [$A], 'Tab Wave', hide true, pane $wavePane"
    } else { "3. hide fired: $said; the row hidden and pending from frame $($pending.frame)" }
  }
  $light = & $sent $f 'the hide'
  if ($light.Frame -lt $pending.frame) { Add-Failure "the hide's light set off in frame $($light.Frame), before its row was hidden (frame $($pending.frame))" }
  else { "3. the light up the rail (light.send) set off in frame $($light.Frame), the rail let go, $($light.Left) ms to run" }
  $script:seen.Clear()
  $f = (Wait-State { param($s) $true } -What 'a frame').frame
  Write-Button @($A)
  $done = Wait-Change { param($s) $r = & $rowOf $s 'Tab Wave'; $r.hidden -and -not $r.hidden_pending } -AfterFrame $f -OnState $watch -What "Tab Wave hidden as the Conductor says"
  if (-not $script:seen.Contains('light.glint')) { Add-Failure "the Conductor's word on the hide didn't glint Tab Wave's pad: [$(@($script:seen) -join ', ')]" }
  else { '3. confirmed: hidden, no longer pending, the pad glinted (light.glint)' }

  # 3. The look on the closed rail: the mark on Tab Wave's pad, none on the shell's
  & $closed
  $s = (Read-State).windows[0]
  $shot = Get-Shot $w -Name 'hidden-rail'
  $wavePad = (& $rowOf $s 'Tab Wave').pad
  $was = Measure-Mark $before $wavePad
  $now = Measure-Mark $shot $wavePad
  $shellPad = (& $rowOf $s 'shell').pad
  $shell = Measure-Mark $shot $shellPad
  $shellWas = Measure-Mark $before $shellPad
  "3. the mark: Tab Wave's corner $($was.PadPx) of the pad's #$($was.PadRgb) and $($was.Glyph) grey before, $($now.PadPx) and $($now.Glyph) hidden (the glyph's middle $($now.Off) px from the disc's); the shell's $($shellWas.Glyph) grey before, $($shell.Glyph) after"
  if ($was.PadPx - $now.PadPx -lt 25) { Add-Failure "no disc cut out of Tab Wave's pad's corner: $($was.PadPx) of its colour before, $($now.PadPx) hidden" }
  # The pad's number can reach into the corner's square, so the glyph is what the hide added
  if ($now.Glyph - $was.Glyph -lt 8) { Add-Failure "no eye-off glyph on Tab Wave's pad: $($was.Glyph) grey pixels before, $($now.Glyph) hidden" }
  elseif ($was.Glyph -le 2 -and $now.Off -gt 2) { Add-Failure "the eye-off glyph is $($now.Off) px off its disc's middle" }
  if ([Math]::Abs($shell.Glyph - $shellWas.Glyph) -gt 2) { Add-Failure "the shell's pad, not hidden, changed at its corner: $($shellWas.Glyph) grey pixels before, $($shell.Glyph) after" }

  # Tab Wave waiting on him: still orange, the mark on it (the light is claudehook's)
  [IO.File]::WriteAllText($lightGo, '')
  Wait-Change { param($s) $r = & $rowOf $s 'Tab Wave'; $r.state -eq 'waiting' -and $r.hidden } -What 'Tab Wave waiting, still hidden' -Animating | Out-Null
  $shot = Get-Shot $w -Name 'hidden-waiting'
  $lit = Measure-Mark $shot $wavePad
  $orange = [LbtermE2E.Px]::CountNear($shot, (ConvertTo-Rect $wavePad), 0xFF8A3D, 12)
  "3. waiting on him: $orange px of the pad orange, the mark's glyph $($lit.Glyph) grey"
  if ($orange -lt 150) { Add-Failure "Tab Wave waits on him but its pad isn't orange: $orange px" }
  if ($lit.Glyph -lt 8) { Add-Failure "the mark went when Tab Wave turned orange: $($lit.Glyph) grey pixels" }

  # The open panel: the Hidden chip on Tab Wave's row, in the project chip's fill, its word white
  # at 45% over it; none on the shell's row
  & $opened
  $s = (Read-State).windows[0]
  $shot = Get-Shot $w -Name 'hidden-open'
  # The second line of a row's words: the lower half of its slot, right of its pad. The shell's
  # has its project's chip ("Windows", its folder's), in the same fill, its word the project's
  # grey; only a hidden row's has a word white at 45% on it, its strokes' middles that colour within
  # 4 (antialiased in linear light, the project's grey crosses it within 10 at a word's edges)
  $words = { param($row) $r = $row.rect; $p = $row.pad; [System.Drawing.Rectangle]::new([int]($p.x + $p.w + 8), [int]($r.y + $r.h / 2), [int]($r.x + $r.w - $p.x - $p.w - 8), [int]($r.h / 2)) }
  $waveRow = & $rowOf $s 'Tab Wave'; $shellRow = & $rowOf $s 'shell'
  $chip = [LbtermE2E.Px]::CountNear($shot, (& $words $waveRow), 0x262634, 3)
  $ink = [LbtermE2E.Px]::CountNear($shot, (& $words $waveRow), 0x88888F, 4)
  $none = [LbtermE2E.Px]::CountNear($shot, (& $words $shellRow), 0x88888F, 4)
  "3. the chip: $chip px of the chip's fill and $ink of its word on Tab Wave's second line, $none of that word's colour on the shell's"
  if ($chip -lt 60) { Add-Failure "no Hidden chip on Tab Wave's open row: $chip px of the chip's fill" }
  if ($ink -lt 20) { Add-Failure "the Hidden chip has no words white at 45%: $ink px" }
  if ($none -gt 2) { Add-Failure "the shell's row, not hidden, has words white at 45% on its second line: $none px" }
  $open = Measure-Mark $shot $waveRow.pad
  if ($open.Glyph -lt 8) { Add-Failure "the open panel lost Tab Wave's mark: $($open.Glyph) grey pixels" }
  # Over its own highlight (the pointer on its pad): the pad drawn again on it, the disc in the
  # highlight's colour and the mark lifted with the number
  $px, $py = Get-RectCenter $waveRow.pad
  Send-Mouse $w Move $px $py
  Wait-Change { param($s) (& $rowOf $s 'Tab Wave').hovered } -What 'Tab Wave''s row under the pointer' -Animating | Out-Null
  $lifted = Measure-Mark (Get-Shot $w -Name 'hidden-hovered') $waveRow.pad
  "3. under the pointer: the mark's glyph $($lifted.Glyph) grey, $($lifted.Off) px off the disc's middle"
  if ($lifted.Glyph -lt 8) { Add-Failure "Tab Wave's highlight covers its mark: $($lifted.Glyph) grey pixels" }
  & $closed

  # 4. Show to Conductor, by a click: at once, then confirmed
  $m = & $menu 'Tab Wave'
  $show = @($m.menu.items | Where-Object key -eq 'H')[0]
  if (-not $show -or $show.label -ne 'Show to Conductor') { Add-Failure "a hidden tab's menu has [$(Menu-Rows $m)], no 'Show to Conductor (H)'" }
  else {
    $f = $m.frame
    $x, $y = Get-RectCenter $show.rect
    Send-Click $w $x $y -Move
    $shown = Wait-State { param($s) $r = & $rowOf $s 'Tab Wave'; -not $r.hidden -and $r.hidden_pending } -AfterFrame $f -What "Tab Wave's row shown and pending"
    if (-not (Wait-Until { @(Get-Events).Count -ge 2 } -TimeoutSec 15)) { Add-Failure 'Show fired no lbterm-conductor-hide' }
    elseif (@(Get-Events)[-1].hide -ne $false) { Add-Failure "Show fired hide = $(@(Get-Events)[-1].hide)" }
    else { "4. show fired hide false; the row visible and pending from frame $($shown.frame)" }
    $light = & $sent $f 'the show'
    "4. the light up the rail set off in frame $($light.Frame), $($light.Left) ms to run"
    $script:seen.Clear()
    $f = (Wait-State { param($s) $true } -What 'a frame').frame
    Write-Button @()
    Wait-Change { param($s) $r = & $rowOf $s 'Tab Wave'; -not $r.hidden -and -not $r.hidden_pending } -AfterFrame $f -OnState $watch -What 'Tab Wave shown as the Conductor says' | Out-Null
    if (-not $script:seen.Contains('light.glint')) { Add-Failure "the Conductor's word on the show didn't glint Tab Wave's pad: [$(@($script:seen) -join ', ')]" }
    else { '4. confirmed: visible, no longer pending, the pad glinted' }
  }

  # 5. Pair: both sessions, active pane's first; the Conductor says nothing
  $m = & $menu 'Pair'
  $f = $m.frame
  $asked = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  Send-Key $w H
  Wait-State { param($s) $r = & $rowOf $s 'Pair'; $r.hidden -and $r.hidden_pending } -AfterFrame $f -What "Pair's row hidden and pending" | Out-Null
  if (-not (Wait-Until { @(Get-Events).Count -ge 3 } -TimeoutSec 15)) { Add-Failure "Hide on Pair fired no lbterm-conductor-hide" }
  else {
    $ev = @(Get-Events)[-1]
    if ((@($ev.sessions) -join ',') -ne "$B,$C" -or $ev.pane -ne $pairPane) { Add-Failure "Hide on Pair fired sessions [$(@($ev.sessions) -join ', ')] from pane $($ev.pane), not [$B, $C] from pane $pairPane" }
    else { "5. hide on Pair fired both sessions, its active pane's first" }
  }
  $light = & $sent $f 'the hide on Pair'
  "5. one light up the rail from Pair's pad, frame $($light.Frame)"
  # While it waits, no second request: Pair's row is there greyed, "Hiding from Conductor…", with
  # no key, and neither H nor a click on it asks again (Lachlan's 11b of 2026-11-09)
  $m = & $menu 'Pair'
  $r = & $rowOf $m 'Pair'
  $greyed = @($m.menu.items | Where-Object { $_.label -eq "Hiding from Conductor$([char]0x2026)" })[0]
  if (-not $r.hidden_pending) { Add-Failure "Pair's menu opened after its pick ran out ($([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $asked) ms): the PC too slow to judge it" }
  elseif (@($m.menu.items | Where-Object key -eq 'H').Count) { Add-Failure "Pair's menu offered '$(@($m.menu.items | Where-Object key -eq 'H')[0].label)' while its Hide waited: [$(Menu-Rows $m)]" }
  elseif (-not $greyed -or -not $greyed.inert) { Add-Failure "Pair's menu has no greyed 'Hiding from Conductor…' row while its Hide waits: [$(Menu-Rows $m)]" }
  else {
    "5. while it waits, Pair's menu has the row greyed with no key: $(Menu-Rows $m)"
    # Its look (menu-waiting.png): the greyed words and every key are white at 50% and 35% over
    # the card, about 140 and 110, so nothing in them reaches 200, where a live row's words
    # (0xECECF4) do. A colour's alpha drew at full strength until 2026-11-09 (PITFALLS 248)
    $shot = Get-Shot $w -Name 'menu-waiting'
    $live = @($m.menu.items | Where-Object key -eq 'M')[0]
    $words = { param($item) $r = ConvertTo-Rect $item.rect; [System.Drawing.Rectangle]::new($r.X, $r.Y, [int]($r.Width * 0.75), $r.Height) }
    $keyOf = { param($item) $r = ConvertTo-Rect $item.rect; [System.Drawing.Rectangle]::new($r.Right - [int](32 * $w.Scale), $r.Y, [int](32 * $w.Scale), $r.Height) }
    if (-not [LbtermE2E.Px]::BrightBox($shot, (& $words $live), 200)) { Add-Failure "Move to new window's words have nothing over 200 in menu-waiting.png: the capture can't judge the greyed row" }
    elseif ($over = [LbtermE2E.Px]::BrightBox($shot, (& $words $greyed), 200)) { Add-Failure "the greyed row's words draw as bright as a live row's (over 200 from x $($over[0]), y $($over[1])): white at 50% would be about 140 (PITFALLS 248)" }
    elseif ($over = [LbtermE2E.Px]::BrightBox($shot, (& $keyOf $live), 200)) { Add-Failure "Move to new window's key M draws over 200 (from x $($over[0]), y $($over[1])): white at 35% would be about 110 (PITFALLS 248)" }
    else { '5. the greyed words and the keys are dim in menu-waiting.png, a live row''s words bright' }
    $f = $m.frame
    Send-Key $w H
    $after = Wait-State { param($s) $true } -AfterFrame $f -What 'a frame after H on the greyed row'
    $x, $y = Get-RectCenter $greyed.rect
    Send-Click $w $x $y -Move
    $after = Wait-State { param($s) $true } -AfterFrame $after.frame -What 'a frame after a click on the greyed row'
    if (-not $after.menu) { Add-Failure 'H or a click on the greyed row closed the menu' }
    elseif (@(Get-Events).Count -ne 3) { Add-Failure "H or a click on the greyed row asked again: $(@(Get-Events).Count) events" }
    else { '5. H and a click on the greyed row did nothing, the menu still open' }
  }
  & $closeMenu
  # The frame's time is the snapshot's (Unix ms by the GUI's clock), not the window entry's
  $script:backMs = 0
  Wait-State { param($sw, $cs) $r = & $rowOf $sw 'Pair'; if (-not $r.hidden -and -not $r.hidden_pending) { $script:backMs = [long]$cs.time_ms; $true } } -TimeoutSec 30 -What "Pair's row back after 5 s" | Out-Null
  $notice = Wait-State { param($s) $s.strip.notice } -TimeoutSec 10 -What "the strip's notice"
  $after = $script:backMs - $asked
  if ($after -lt 5000) { Add-Failure "Pair's row went back $after ms after the key: sooner than 5 s" }
  if ($notice.strip.notice -ne "The Conductor didn't hide Pair") { Add-Failure "the strip said '$($notice.strip.notice)', not 'The Conductor didn't hide Pair'" }
  else { "5. not confirmed: the row went back $after ms after the key, the strip saying '$($notice.strip.notice)'" }
  Wait-State { param($s) -not $s.strip.notice } -TimeoutSec 15 -What "the notice gone" | Out-Null
  '5. the notice went'

  # 6. The Conductor's view: the hidden sessions folded at the foot of Sessions, then Show
  $sidElse = 'c0ffee00-0000-4000-8000-0000000000ee'
  Write-Button @($A, $sidElse)
  Wait-Change { param($s) (& $rowOf $s 'Tab Wave').hidden -and -not (& $rowOf $s 'Tab Wave').hidden_pending } -What 'Tab Wave hidden again' | Out-Null
  $at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  $view = { param([bool]$aHidden, [bool]$xHidden) @"
{"type":"seat.view","seq":1,"at":$at,
 "conductor":{"state":"on","mode":"read","listening":true,"pausedAt":null,"catchUp":false,"busy":false,"queued":0,"sessionId":"c0","startedAt":$($at - 600000),"model":"claude-opus-5-5","error":null},
 "asks":[],
 "sessions":[{"sessionId":"$A","name":"terminal-a","project":"Terminal","status":"waiting","label":"Tab Wave","summary":"","question":"Which colour?","pane":null,"agents":[],"conductor":null,"hidden":$("$aHidden".ToLower())},
             {"sessionId":"$sidElse","name":"web-x","project":"Website","status":"idle","label":"Elsewhere","summary":"Built the page","question":null,"pane":null,"agents":[],"conductor":null,"hidden":$("$xHidden".ToLower())},
             {"sessionId":"$B","name":"terminal-b","project":"Terminal","status":"working","label":"Pair","summary":"Building","question":null,"pane":null,"agents":[],"conductor":null}],
 "waiting":[],"events":[],"suggestions":[],"notes":[]}
"@ }
  # The view pushed until LBTerm has subscribed and been told
  if (-not (Wait-Until { (Push-Seat (& $view $true $true)) -gt 0 } -TimeoutSec 15)) { throw 'LBTerm never subscribed to the seat' }
  $x, $y = Get-RectCenter (Read-State).windows[0].rail.conductor.pad
  Send-Click $w $x $y -Move
  Send-Leave $w
  $v = (Wait-Change { param($s) $s.conductor_view -and $s.conductor_view.connected -and @($s.conductor_view.buttons | Where-Object name -eq 'section:sessions:hidden').Count } -What 'the view open with the Hidden group' -Animating).conductor_view
  $group = @($v.buttons | Where-Object name -eq 'section:sessions:hidden')[0]
  $shot = Get-Shot $w -Name 'view-hidden-folded'
  $kinds = @($v.cards | ForEach-Object kind) -join ','
  if ($group.on) { Add-Failure 'the Hidden group started open' }
  if (@($v.buttons | Where-Object { $_.name -like 'show:*' }).Count) { Add-Failure 'Show buttons while the Hidden group is folded' }
  if (@($v.cards | Where-Object kind -eq 'hidden').Count) { Add-Failure "hidden sessions' cards while folded: $kinds" }
  $last = @($v.buttons | Where-Object { $_.name -like 'section:sessions:*' })[-1].name
  if ($last -ne 'section:sessions:hidden') { Add-Failure "the Hidden group isn't at the foot of Sessions: the last group is $last" }
  $orange = if ($group.rect) { [LbtermE2E.Px]::CountNear($shot, (ConvertTo-Rect $group.rect), 0xFF8A3D, 24) } else { 0 }
  if ($orange -lt 10) { Add-Failure "the folded Hidden group's heading doesn't say in orange how many wait on him: $orange px" }
  else { "6. the view: Hidden folded at the foot of Sessions ($kinds), $orange px of orange on its heading" }
  $x, $y = Get-RectCenter $group.rect
  Send-Click $w $x $y -Move
  $v = (Wait-Change { param($s) @($s.conductor_view.buttons | Where-Object { $_.name -like 'show:*' }).Count -eq 2 } -What 'the Hidden group open' -Animating).conductor_view
  Get-Shot $w -Name 'view-hidden-open' | Out-Null
  $hiddenCards = @($v.cards | Where-Object kind -eq 'hidden' | ForEach-Object title) -join ', '
  if ($hiddenCards -ne 'Tab Wave, Elsewhere') { Add-Failure "the open Hidden group has [$hiddenCards], not Tab Wave and Elsewhere" }
  else { "6. opened: $hiddenCards, each with Show" }

  # Show on Tab Wave: the event for A alone, from the view, its tab's own pane; its row waits
  $n = @(Get-Events).Count
  $f = (Read-State).windows[0].frame
  $btn = @($v.buttons | Where-Object name -eq "show:$A")[0]
  $x, $y = Get-RectCenter $btn.rect
  Send-Click $w $x $y -Move
  Wait-State { param($s) $r = & $rowOf $s 'Tab Wave'; -not $r.hidden -and $r.hidden_pending } -AfterFrame $f -What "Tab Wave's row shown and pending from the view" | Out-Null
  if (-not (Wait-Until { @(Get-Events).Count -gt $n } -TimeoutSec 15)) { Add-Failure "the view's Show fired no lbterm-conductor-hide" }
  else {
    $ev = @(Get-Events)[-1]
    $said = "sessions [$(@($ev.sessions) -join ', ')], label '$($ev.label)', hide $($ev.hide), source '$($ev.source)', pane $($ev.pane)"
    if ((@($ev.sessions) -join ',') -ne $A -or $ev.label -ne 'Tab Wave' -or $ev.hide -ne $false -or $ev.source -ne 'conductor view' -or $ev.pane -ne $wavePane) {
      Add-Failure "the view's Show fired $said, not sessions [$A], 'Tab Wave', hide false, 'conductor view', pane $wavePane"
    } else { "6. Show on Tab Wave fired $said; its row visible and pending" }
  }
  $light = & $sent $f "the view's Show"
  "6. a light up the rail from Tab Wave's pad, frame $($light.Frame)"
  # Show on it again while it waits: nothing fired, the first still waiting
  $n = @(Get-Events).Count
  $f = (Read-State).windows[0].frame
  Send-Click $w $x $y -Move
  try { Wait-State { param($s) $true } -AfterFrame $f -TimeoutSec 2 -What 'a frame after Show again' | Out-Null } catch { }
  if (Wait-Until { @(Get-Events).Count -gt $n } -TimeoutSec 1.5) { Add-Failure "Show on Tab Wave again while its Show waited fired another lbterm-conductor-hide" }
  elseif (-not (& $rowOf (Read-State).windows[0] 'Tab Wave').hidden_pending) { Add-Failure "Tab Wave's row stopped waiting before the script's write" }
  else { '6. Show on it again while it waits: nothing fired' }
  $f = (Read-State).windows[0].frame
  Write-Button @($sidElse)
  Push-Seat (& $view $false $true) | Out-Null
  Wait-Change { param($s) $r = & $rowOf $s 'Tab Wave'; -not $r.hidden -and -not $r.hidden_pending -and @($s.conductor_view.cards | Where-Object kind -eq 'hidden').Count -eq 1 } -AfterFrame $f -What 'Tab Wave shown, the view saying so' -Animating | Out-Null
  '6. confirmed: the row no longer pending, the view''s Hidden group down to Elsewhere'

  # Show on Elsewhere, which has no tab here: the event for X with the window's active pane
  $v = (Read-State).windows[0].conductor_view
  $btn = @($v.buttons | Where-Object name -eq "show:$sidElse")[0]
  if (-not $btn) { Add-Failure "no Show on Elsewhere: $(@($v.buttons | ForEach-Object name) -join ', ')" }
  else {
    $n = @(Get-Events).Count
    # The window's active tab's pane (Pair's since its pane was activated at the start), the
    # mux server's id as the state file says it
    $activePane = (Read-State).windows[0].active_pane
    $x, $y = Get-RectCenter $btn.rect
    Send-Click $w $x $y -Move
    if (-not (Wait-Until { @(Get-Events).Count -gt $n } -TimeoutSec 15)) { Add-Failure "Show on Elsewhere fired no lbterm-conductor-hide" }
    else {
      $ev = @(Get-Events)[-1]
      $now = (Read-State).windows[0]
      $pending = @($now.rail.rows | Where-Object hidden_pending).Count
      if ((@($ev.sessions) -join ',') -ne $sidElse -or $ev.label -ne 'Elsewhere' -or $ev.pane -ne $activePane -or $pending) {
        Add-Failure "Show on Elsewhere fired sessions [$(@($ev.sessions) -join ', ')], '$($ev.label)', pane $($ev.pane) with $pending rows pending, not [$sidElse], 'Elsewhere', pane $activePane, none"
      } else { "6. Show on Elsewhere fired for it alone with the window's pane ($activePane); no row waits" }
      # No row of it here, so no pad to send a light from: a frame after the event says none runs
      # (A window with nothing more to draw writes none: the frame the event came in is then the last)
      $after = try { Wait-State { param($s) $true } -AfterFrame $now.frame -TimeoutSec 2 -What 'a frame after Show on Elsewhere' } catch { $null }
      $named = @(@($now) + @($after) | Where-Object { $_ } | ForEach-Object { @($_.animations | Where-Object name -eq 'light.send') }).Count
      if ($named) { Add-Failure "Show on Elsewhere, with no tab here, sent a light up the rail" }
      else { "6. no light up the rail for it" }
    }
  }
} -Passed 'conductor-hide: the tab menu offers Hide from Conductor only with the Conductor''s list, a pick shows at once, fires the event and sends a light up the rail once the card has gone, the Conductor''s word confirms it with a glint, the mark and the chip show it, Show undoes it from the menu or the view''s folded Hidden group, and an unanswered pick goes back with the strip''s notice'
