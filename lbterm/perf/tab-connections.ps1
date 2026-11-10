#Requires -Version 7
# Covers: lbterm-extras/src/links.rs, lbterm-extras/src/glow.rs, lbterm-extras/src/menu.rs, lbterm-extras/src/state.rs, lbterm-panel/src/palette.rs, lbterm-panel/src/layout.rs, lbterm-panel/src/words.rs, lbterm-motion/src/spans.rs, wezterm-gui/src/termwindow/render/connections.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/render/link_drag.rs, wezterm-gui/src/termwindow/render/tab_menu.rs, wezterm-gui/src/termwindow/tab_links.rs, wezterm-gui/src/termwindow/held.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/state.rs, wezterm-gui/src/state_files.rs, lbterm/motion/demo-tab.ps1
<#
Tab connections (lbterm/LINKS.md, "Connections"): what the rail draws and what Lachlan does to
them. A test window looking like Lachlan's LBTerm (New-TestConfig) with the tab panel,
lbterm_summaries and lbterm_links pointed at scratch files, and a handler of its own for
lbterm-tab-link that writes each event to <out>\events.jsonl. The test plays the links file's
program (Terminal's links.ts): it writes the file and reads the events. Five stand-in Claude tabs:
Scroll strip, Tab Wave, Heat steps (waiting on Lachlan), Usage chart and InMail.

1. The file connects Scroll strip, Heat steps and InMail (rows 0, 2 and 4), suggests Tab Wave with
   Usage chart (rows 1 and 3), has Tab Wave waiting on Scroll strip (a link) and a declined
   suggestion. The link keeps lane 0, the suggested pair takes lane 1 and the group of three lane 2
   (the state file's rail.connections); the declined one shows nowhere; the three rows say
   they're in the group (connected) and keep their own pads. In the capture (tab-connections.png)
   the group's line is cyan down its lane from row 0 to row 4 with a solid mark at each member;
   the pair's line is faint cyan with hollow marks. Where a row ends the link too (Scroll strip,
   waited on, and Tab Wave, waiting), the link's mark is drawn over the connection's, purple, with
   the cyan round it.
2. The suggested line's light (lbterm_extras::glow, Lachlan's 1a): carried by DWM through its
   three laps as the suggestion comes (loops says connection.light), seen at two places along the
   line at least 4 px apart in captures taken over 3.2 s (tab-connections-light-1.png, -2.png);
   once they're done, a frame saying no loop, and the light still and glowing at the line's middle
   in three captures 600 ms apart (tab-connections-light-still.png). The pointer coming onto the
   rail from the pane runs them again, seen in two places once the panel it opens has settled
   (its own light crosses the line's band as it opens); the window minimized while they run,
   the light still within 3 s, and restored, running the laps it was cut off from, as the rail's
   other loops do (lbterm_extras::work::loops_play); done again, the file saying a new why of the
   pair runs them again. And the window's own frame count flat: the counters' frames over
   -IdleSeconds with the light running, against a span as long with no suggestion. Since
   feat/dcomp-recorder where the light rests (the line's middle, after its laps and with motion
   off) is compositor/imp/tests/connections.rs's, with its laps' values (three, down to the
   line's bottom and up to its top and back each, ending at the middle) from what DWM was handed;
   this step keeps the light glowing, seen moving in captures (DWM playing it) and the loops.
3. Without the compositor (a second window, lbterm_compositor = false): no light, and the
   suggested line is dashed, its lane broken into runs (tab-connections-flat.png); the group's
   line runs past Tab Wave's row with nothing across to its mark, and across to Heat steps'.
4. Motion off (a third window, lbterm_motion's enabled = 'off'): no loop, and the light still at the
   suggested line's middle in captures 600 ms apart (tab-connections-still.png); since
   feat/dcomp-recorder the light glowing is judged here and where it is by
   compositor/imp/tests/connections.rs.

What he does, in a fourth window: the group with a note and five messages, the pair suggested,
and Tab Wave saying it waits for Scroll strip's objective 1.
5. Resting on the pair's line, the panel closed: its card beside the rail, "Tab Wave and Usage
   chart could be talking", the why and "A click on the line connects them" in cyan
   (tab-connections-card-suggested.png), the panel staying shut.
6. The tab menu on Usage chart: "Connect with Tab Wave, as suggested" and "Decline the suggested
   connection with Tab Wave", the menu widened past its 260 px for them; Decline fires decline
   with the pair's id, and the pair's line goes at once.
7. Suggested again (a new id): a left click on its line fires accept with its id, and it's drawn
   connected at once.
8. Resting on the group's line: "Connected: Scroll strip, Heat steps, InMail", the why, the note
   in red and the last three of the five messages, each "Scroll strip to Heat steps, 14:02: …"
   (tab-connections-card.png).
9. The tab menu on Scroll strip: "Leave the connection with Heat steps and InMail" fires leave
   with the group's id and Scroll strip's session, and the group's line shrinks to its other two
   at once.
10. Shift held over the pane first: the state file's tint says shift and the active pane's outline
    is green along its card's bottom edge (tab-connections-shift.png). Tab Wave right-dragged onto
    Scroll strip with Shift held: the drag says it connects, held still says shift but tint
    doesn't, and no green along the outline (Lachlan's 2a), the line cyan, both cards objectives
    (Tab Wave's and Scroll strip's), nothing lit though Tab Wave's
    wait matches (tab-connections-drag.png), side by side; the window made too narrow for that,
    Scroll strip's card under Tab Wave's or over it, not across it (tab-connections-drag-narrow.png),
    and the window as wide again; Shift let go mid-drag, it's a wait link again, the
    match lit green; held again (still no tint) and let go, connect fires with the two sessions,
    and Scroll strip is drawn joining Tab Wave's connection at once (his E6); the drag over with
    Shift still held, tint says shift and the outline is green again
    (tab-connections-drag-after.png).
11. Usage chart Shift-dragged onto Scroll strip, connected together now: connect fires, and Usage
    chart is drawn out of the connection at once.
12. Usage chart dragged onto Heat steps with Shift, let go of Shift, then of the button: toggle
    fires, a wait link, drawn at once.

The Conductor's pad, in a fifth window with lbterm_conductor (its pad atop the rail, the tabs a
slot lower), its remote control on, so its mark shows on the pad's top right corner:
13. The file suggests the pair (both on this rail), Usage chart with Night build (a session in no
    tab of this window, its label from the summaries) and Night build with a second session
    elsewhere, beside the group and the declined one: the pad's cyan badge counts 2, the two with a
    member here (the state file's rail.conductor.suggested and badge), on the pad's top left
    corner, cyan with the count in the pads' numbers' font (auto-hinted) dark on it, 8 of its pixels
    or more 0x60 or darker (tab-connections-badge.png, and six times as large -badge-zoom.png). Ten more
    suggestions of Usage chart's with sessions elsewhere: 12, the badge wider to the right, its
    left, top and height as they were (-badge-12.png, -badge-12-zoom.png). With the pair and
    the ten gone it counts 1, where the 2 was, its count's dark pixels not the 2's
    (-badge-1.png, -badge-1-zoom.png). At 1 and 12, every row of the 1's stem has a pixel 0x60 or
    darker, its thinnest stroke (Measure-Stem). At each, the badge stays left of the remote mark
    (rail.conductor.mark) and no cyan is in the mark's square. Usage chart's menu
    "Connect with Night build, as suggested" fires accept with its id, and the badge goes at once,
    before the file says so; the file saying it, none is drawn (tab-connections-badge-none.png).
14. Scroll strip connected with Tab Wave and suggested with InMail, no link, read on the panel
    opened under the pointer on its pad (step 13's pick shuts it): its two marks in one
    place read connected, solid cyan in the middle, with the suggestion's rim round it (more cyan
    round it than round Tab Wave's mark alone), InMail's hollow (tab-connections-two-marks.png,
    -two-marks-zoom.png six times as large).

-Rest measures instead, for before and after: one window, the same file, moved clear of what
covers it on its screen where it can be (Move-TestWindowClear; what covers it is said), settled,
then three spans: -RestSeconds with the light still once its laps are done (a build that runs it
for ever is still running it after -StillWait s), -LapsSeconds of its laps run again by the file
saying a new why of the pair (shorter than the laps, or it fails), and -RestSeconds with no
suggestion. Each: the window's frames (its counters and the state file's) and wakes, the GUI
process's and its GUI thread's CPU (their cycles), and what DWM did (the harness's
Measure-DwmSpan: the compositor clock's frames a second, the share of 50 ms steps it completed
none in, dwm.exe's CPU and GPU in ms a second, the frames it presented a second to each screen and
to the window's own; all of it the whole desktop's, so other windows' changes count too), written
to rest.json and printed.

-Environment NAME=value,... goes into every window's environment, and -Config 'name=value',... into
every window's config after the test's own settings (-Config 'dpi=144' for the badge at 1.5x).
A build without connections ignores the key and draws the link alone.

Its USERPROFILE is a scratch folder. Fails with exit 1.

-Acts runs 5 to 12 alone, and -Badge 13 and 14 alone.

-Pointer measures instead what a pointer move over the rail's lines costs, as inbox-mouse.ps1 does
for its list: one window with the file of 1 to 4, and -Rounds rounds of -Moves moves posted along
the link's lane in Scroll strip's row, below its mark, with its card open, along the group's lane
in Tab Wave's row, which it passes beside the pair's lane and mark, with its card open (a build
without connections has no line there), and over the pane, each measured by the GUI thread's
cycles once it has gone quiet, with the frames painted. Each sweep stays inside one row: the row
under the pointer changing paints a frame, which the first try's sweeps across rows were mostly
measuring. It fails if any frame is painted. Written to pointer.json and printed, for a build
before and after.

-Paint measures instead what a frame costs with the Conductor's badge in it: 13's window, the pad
counting two, and a sixth tab, the open one, printing a line every 16 ms for -PaintSeconds, -Rounds
times after a screenful unmeasured, as paint-cost.ps1 prints; the GUI thread's and the process's
CPU a frame by their cycles, written to paint.json and printed, for a build before and after (one
without connections ignores the key, so its rail has the link and the pad, no badge).

  pwsh -File lbterm\perf\tab-connections.ps1 [-Bin <dir>] [-Rest [-RestSeconds 10] [-LapsSeconds 5] [-StillWait 15]] [-Environment NAME=value,...] [-Config 'name=value',...] [-Acts] [-Badge] [-Pointer [-Moves 400] [-Rounds 3]] [-Paint [-PaintSeconds 6] [-Rounds 3]]
#>
param([string]$Bin, [switch]$Rest, [int]$RestSeconds = 10, [double]$LapsSeconds = 5, [int]$StillWait = 15, [string[]]$Environment = @(), [string[]]$Config = @(), [int]$IdleSeconds = 4, [switch]$Acts, [switch]$Badge, [switch]$Pointer, [int]$Moves = 400, [int]$Rounds = 3, [switch]$Paint, [double]$PaintSeconds = 6)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'tab-connections' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
$linksFile = Join-Path $T.Out 'tab-links.json'
$events = Join-Path $T.Out 'events.jsonl'
New-Item -ItemType Directory -Force $summaries, $home2 | Out-Null
Remove-Item -LiteralPath $events -ErrorAction SilentlyContinue

# The sessions. PowerShell's names ignore case, so no other variable here may be $a to $e:
# a `$c = ...` in the test's scope once put a connection where Heat steps' session was
$A = 'aaaaaaaa-1111-4111-8111-111111111111'
$B = 'bbbbbbbb-2222-4222-8222-222222222222'
$C = 'cccccccc-3333-4333-8333-333333333333'
$D = 'dddddddd-4444-4444-8444-444444444444'
$E = 'eeeeeeee-5555-4555-8555-555555555555'
$tabs = @(
  @{ sid = $A; title = 'Scroll strip'; state = 'idle' }
  @{ sid = $B; title = 'Tab Wave'; state = 'idle' }
  @{ sid = $C; title = 'Heat steps'; state = 'waiting' }
  @{ sid = $D; title = 'Usage chart'; state = 'idle' }
  @{ sid = $E; title = 'InMail'; state = 'idle' }
)
function Link([string]$Waiter, [string]$On, [string]$State = 'waiting', [string]$What = '') {
  [ordered]@{ id = "$Waiter-$On"; waiter = $Waiter; on = $On; at = 1; by = 'test'; what = $What; state = $State; stateAt = 1; note = '' }
}
function Connection([string]$Id, [string[]]$Members, [string]$State, [string]$By = 'files', [string]$Why = '', [string]$Note = '', $Messages = @()) {
  [ordered]@{ id = $Id; members = @($Members); state = $State; at = 1793250000000; stateAt = 1793250000000; by = $By; why = $Why; note = $Note; messages = @($Messages) }
}
function Write-Links($Links, $Connections = @(), $Waits = @()) {
  $json = [ordered]@{ updatedAt = 1; links = @($Links); waits = @($Waits); connections = @($Connections) } | ConvertTo-Json -Depth 6 -Compress
  Write-FileWhole -Path $linksFile -Text $json
}
function Objective([int]$Id, [string]$Status, [string]$Text) {
  [ordered]@{ id = $Id; text = $Text; from = 'prompt'; at = 1; status = $Status; evidence = ''; statusAt = 1 }
}
function Write-Objectives([string]$Sid, [int]$Current, $List) {
  $json = [ordered]@{ sessionId = $Sid; updatedAt = 1; current = $Current; objectives = @($List) } | ConvertTo-Json -Depth 5 -Compress
  [IO.File]::WriteAllText((Join-Path $summaries "$Sid.objectives.json"), $json)
}
function Read-Events { @(if (Test-Path $events) { Get-Content -LiteralPath $events | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } }) }
function Wait-Event([int]$Count, [string]$What) {
  $deadline = [DateTime]::Now.AddSeconds(10)
  while ([DateTime]::Now -lt $deadline) {
    $all = Read-Events
    if ($all.Count -ge $Count) { return $all[$Count - 1] }
    Start-Sleep -Milliseconds 100
  }
  Add-Failure "no lbterm-tab-link event for $What"
  $null
}
# Whether an event is what was asked, field by field (a field not named must be absent)
function Test-Event($Fired, [hashtable]$Want, [string]$What) {
  if (-not $Fired) { return }
  $got = [ordered]@{}; foreach ($p in $Fired.PSObject.Properties) { $got[$p.Name] = $p.Value }
  $bad = @(foreach ($k in @($Want.Keys) + @($got.Keys) | Select-Object -Unique) { if ("$($got[$k])" -ne "$($Want[$k])") { $k } })
  "$What fired $($Fired | ConvertTo-Json -Compress)"
  if ($bad.Count) { Add-Failure "$What fired $($Fired | ConvertTo-Json -Compress), not $($Want | ConvertTo-Json -Compress)" }
}
$handler = @"
wezterm.on('lbterm-tab-link', function(window, pane, l)
  local f = io.open([[$events]], 'a')
  f:write(wezterm.json_encode({ action = l.action, waiter = l.waiter, on = l.on, objective = l.objective, connection = l.connection, session = l.session }) .. '\n')
  f:close()
  return false
end)
"@
$group = Connection 'c-group' @($A, $C, $E) connected -By lbterm -Why 'all three draw on the rail'
$pair = Connection 'c-pair' @($B, $D) suggested -Why 'both write lbterm/render/thumbnails.rs'
$declined = Connection 'c-no' @($A, $D) declined -By digest -Why 'both read the rail'
$wait = Link $B $A -What 'the rail drawn'
function Demo($tab) { New-PwshProgram -File $demoTab -ArgumentList @('-Title', $tab.title, '-State', $tab.state, '-Session', $tab.sid) }
# lbterm_panel::palette::CONNECTED, and any cyan over the rail's dark card: the faint suggested
# line (55% of it), the full one, the light (its white end)
$cyan = 0x33CCE8
$cyanish = New-ColourRange 0 120 80 255 100 255
$lit = New-ColourRange 140 255 205 255 215 255
# lbterm_panel::palette::LINK_WAITING, LINK_DONE and LINK_ERROR
$purple = 0x9F6EEB; $green = 0x3DDC84; $red = 0xFF5449

# -Environment and -Config go to every window the test opens
$envs = @{}
foreach ($kv in @($Environment | ForEach-Object { $_ -split '[ ,]+' } | Where-Object { $_ })) { $k, $v = $kv -split '=', 2; $envs[$k] = $v }

# A window with the five tabs and the file, its rail drawn: what it says
function Open-Rail([string[]]$Set) {
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo -Lua $handler -Environment $envs `
    -Set (@('lbterm_tab_panel={enabled=true,open_delay_ms=0}', "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'", "lbterm_links={file='$(ConvertTo-LuaPath $linksFile)'}") + $Set + $Config) `
    -Program (Demo $tabs[0])
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  $panes = @(0)
  foreach ($tab in $tabs | Select-Object -Skip 1) { $panes += [int](Invoke-Cli spawn --window-id 0 -Program (Demo $tab)) }
  for ($i = 0; $i -lt $tabs.Count; $i++) { Invoke-Cli set-tab-title --pane-id $panes[$i] "$([char]0x2733) $($tabs[$i].title)" | Out-Null }
  Invoke-Cli activate-tab --pane-id 0 --tab-index 0 | Out-Null
  $w
}

# The pixels of a lane between two ys that are in range $k, and their runs down it
function Measure-Lane($shot, [double]$X, [double]$Top, [double]$Bottom, $k) {
  $r = New-Rect ([int]$X - 2) ([int]$Top + 4) 4 ([int]($Bottom - $Top) - 8)
  [pscustomobject]@{ Pixels = [LbtermE2E.Px]::CountRange($shot, $r, $k); Length = $r.Height }
}
# A mark at ($X, $Y): how many cyan pixels round it, and whether its middle is cyan (solid), the
# link's purple (a link's mark over it) or neither (hollow)
function Measure-Mark($shot, [double]$X, [double]$Y) {
  $box = New-Rect ([int]$X - 5) ([int]$Y - 5) 10 10
  $middle = New-Rect ([int][Math]::Floor($X) - 1) ([int][Math]::Floor($Y) - 1) 2 2
  [pscustomobject]@{
    Ring = [LbtermE2E.Px]::CountRange($shot, $box, $cyanish)
    Middle = [LbtermE2E.Px]::CountRange($shot, $middle, $cyanish)
    Purple = [LbtermE2E.Px]::CountNear($shot, $middle, $purple, 40)
  }
}
# Whether every row is in: the group on rows 0, 2 and 4, the pair on 1 and 3, the link (a tab's
# session can reach the rail after its row)
function Test-AllIn($s) {
  $seen = @{}; foreach ($x in @($s.rail.connections)) { $seen[$x.id] = (@($x.rows) -join ',') }
  @($s.rail.rows).Count -eq 5 -and $seen['c-group'] -eq '0,2,4' -and $seen['c-pair'] -eq '1,3' -and @($s.rail.links).Count -eq 1
}

if ($Rest) {
  # The pair as it is, and as the file says it again with another why: its light runs its laps
  # again (lbterm_extras::glow)
  $pairAgain = Connection 'c-pair' @($B, $D) suggested -Why 'both write lbterm/render/thumbnails.rs and lbterm/LINKS.md'
  Write-Links @($wait) @($group, $pair, $declined)
  $spans = [Collections.Generic.List[object]]::new()
  Invoke-Test {
    $w = Open-Rail @('lbterm_compositor=true')
    Wait-Change { param($s) @($s.rail.rows).Count -eq 5 -and @($s.rail.links).Count -eq 1 } -What 'five rows and the link' -Animating | Out-Null
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    $connections = @($sw.rail.connections).Count
    $p = @($sw.rail.connections | Where-Object id -eq 'c-pair')[0]
    # Its laps' length (lbterm_extras::links::glow_path: the line's length at 70 px a second each way,
    # 0.9 s at least, there and back; at this window's DPI)
    $scale = [double]$sw.size.dpi / 96
    $lapS = if ($p) { 2 * [Math]::Max([Math]::Abs(@($p.ys)[-1] - @($p.ys)[0]) / (70 * $scale), 0.9) } else { 0 }
    "build $($T.Bin)$(if ($envs.Count) { " with $(($envs.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')" }): $connections connections drawn, loops $(@($sw.loops) -join ', '), a lap $([Math]::Round($lapS, 2)) s"
    if ($LapsSeconds -gt 3 * $lapS - 1) { Add-Failure "-LapsSeconds $LapsSeconds is longer than the laps ($([Math]::Round(3 * $lapS, 1)) s) less a second" }
    # What it draws reaches the screen only where nothing covers it: moved clear on its screen where
    # it can be, without raising or activating it, and what covers it said
    $cover = Get-WindowCover $w
    if ($cover.Covered -gt 0.02 -and -not (Move-TestWindowClear $w)) { "nowhere on its screen is clear of $($cover.Said)" }
    $cover = Get-WindowCover $w
    "covered by $($cover.Said)"
    $meter = New-DwmMeter -Window $w
    $rate = Get-CycleRate
    $frameNow = { [long](Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).frame }
    $span = { param([string]$Name, [double]$Seconds)
      $m0 = Get-Metrics -Reset
      $c0 = Get-CpuSnapshot -Window $w; $f0 = & $frameNow
      $dwm = Measure-DwmSpan $meter $Seconds
      $m1 = Get-Metrics
      $c1 = Get-CpuSnapshot -Window $w; $f1 = & $frameNow
      $row = [ordered]@{
        span = $Name; seconds = $dwm.seconds; loops = @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).loops)
        frames = $m1.totals.frames; state_frames = $f1 - $f0; wakes = $m1.totals.wakes
        gui_ms = [Math]::Round(($c1.Cycles - $c0.Cycles) / $rate, 1); gui_thread_ms = [Math]::Round(($c1.ThreadCycles - $c0.ThreadCycles) / $rate, 1)
        dwm_frames = $dwm.frames; dwm_idle = $dwm.idle; dwm_cpu_ms = $dwm.cpu_ms; dwm_gpu_ms = $dwm.gpu_ms
        screen_presents = $dwm.screen_presents; presents = $dwm.presents; screens = $dwm.screens
      }
      $spans.Add([pscustomobject]$row)
      "${Name}: $($row | ConvertTo-Json -Compress)"
    }
    # 1. Still: once the laps it came with are done (a build that runs it for ever never is)
    try {
      Wait-State { param($s) @($s.loops) -notcontains 'connection.light' } -What 'the light still' -TimeoutSec $StillWait | Out-Null
      'the light holds still'
    } catch { "the light still runs after $StillWait s" }
    Start-Sleep -Seconds 1
    & $span 'still' $RestSeconds
    # 2. Its laps, the file saying something new of the pair, for -LapsSeconds of them
    $f = & $frameNow
    Write-Links @($wait) @($group, $pairAgain, $declined)
    try { Wait-State { param($s) @($s.loops) -contains 'connection.light' } -What 'the light running its laps' -AfterFrame $f -TimeoutSec 5 | Out-Null }
    catch { "no frame said the light runs after the file changed" }
    & $span 'laps' $LapsSeconds
    # 3. No suggestion at all
    Write-Links @($wait) @($group, $declined)
    Wait-Change { param($s) @($s.rail.connections).Count -eq 1 } -What 'the suggestion gone' -Animating | Out-Null
    Start-Sleep -Seconds 1
    & $span 'none' $RestSeconds
    [ordered]@{ bin = $T.Bin; environment = $envs; lap = $lapS; screen = $meter.Screen; covered = $cover.Covered; covered_by = $cover.Said; spans = $spans; locked = [bool](Get-Process LogonUI -ErrorAction SilentlyContinue) } |
      ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'rest.json')
  } -Passed "measured the window at rest with a suggested connection: still, running its laps and with none"
  return
}

if ($Pointer) {
  Write-Links @($wait) @($group, $pair, $declined)
  Initialize-CpuReader
  $rate = Get-CycleRate
  $results = [Collections.Generic.List[object]]::new()
  Invoke-Test {
    $w = Open-Rail @('lbterm_compositor=true')
    Wait-Change { param($s) @($s.rail.rows).Count -eq 5 -and @($s.rail.links).Count -eq 1 } -What 'five rows and the link' -Animating | Out-Null
    $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    $tid = Get-GuiThreadId $w
    $frameNow = { [long](Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).frame }
    # The GUI thread's cycles once it has gone quiet: under 0.05 ms in 300 ms
    $quietCycles = {
      $last = [LbtermE2E.Cpu]::ThreadCycles($tid)
      $clock = [Diagnostics.Stopwatch]::StartNew()
      while ($clock.Elapsed.TotalSeconds -lt 10) {
        Start-Sleep -Milliseconds 300
        $now = [LbtermE2E.Cpu]::ThreadCycles($tid)
        if ($now - $last -lt 0.05 * $rate) { return $now }
        $last = $now
      }
      throw 'the GUI thread never went quiet after the moves'
    }
    # Moves posted down and up between y0 and y1 at x, a few at a time so none waits long
    $measure = { param([string]$What, [int]$X, [int]$Y0, [int]$Y1)
      Send-Mouse $w Move $X $Y0
      Start-Sleep -Milliseconds 600
      Get-QuietFrame -QuietMs 500 -Window $w | Out-Null
      $card = (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).rail.line_card
      $c0 = & $quietCycles; $f0 = & $frameNow
      for ($i = 0; $i -lt $Moves; $i++) {
        $t = ($i % 40) / 40.0
        $y = if (($i / 40) % 2 -lt 1) { $Y0 + ($Y1 - $Y0) * $t } else { $Y1 - ($Y1 - $Y0) * $t }
        Send-Mouse $w Move $X ([int]$y)
        if ($i % 20 -eq 19) { Start-Sleep -Milliseconds 2 }
      }
      $c1 = & $quietCycles; $f1 = & $frameNow
      [pscustomobject]@{ What = $What; Moves = $Moves; GuiMs = [Math]::Round(($c1 - $c0) / $rate, 3); UsAMove = [Math]::Round(($c1 - $c0) / $rate * 1000 / $Moves, 1); Frames = $f1 - $f0; Card = [bool]$card }
    }
    # Inside one row's slot, so the row under the pointer stays the same: a row hovered changing
    # paints a frame
    $within = { param($i, [double]$from) $r = @($s.rail.rows)[$i].rect; @([int]($r.y + $r.h * $from), [int]($r.y + $r.h - 3)) }
    $link = @($s.rail.links)[0]
    $pane = @($s.panes)[0].rect
    # The group's lane where it passes Tab Wave's row, beside the pair's lane and its mark there
    $groupX = if ($s.rail.connections) { [int]@($s.rail.connections | Where-Object id -eq 'c-group')[0].x } else { [int]$link.x - 6 }
    $linkYs = & $within 0 0.55; $groupYs = & $within 1 0.1
    for ($round = 1; $round -le $Rounds; $round++) {
      $onLink = & $measure 'along the link' ([int]$link.x) $linkYs[0] $linkYs[1]
      $onGroup = & $measure 'along the group' $groupX $groupYs[0] $groupYs[1]
      $onPane = & $measure 'over the pane' ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + 40)) ([int]($pane.y + 140))
      $results.Add([pscustomobject]@{ Round = $round; Link = $onLink; Group = $onGroup; Pane = $onPane })
      "round ${round}: along the link $($onLink.UsAMove) us a move ($($onLink.Frames) frames, card $($onLink.Card)); along the group $($onGroup.UsAMove) us ($($onGroup.Frames) frames, card $($onGroup.Card)); over the pane $($onPane.UsAMove) us ($($onPane.Frames) frames)"
      foreach ($m in $onLink, $onGroup, $onPane) { if ($m.Frames -gt 0) { Add-Failure "round ${round}: $($m.Frames) frames painted for moves $($m.What), which change nothing on the screen" } }
    }
    [pscustomobject]@{ Bin = $T.Bin; Moves = $Moves; Rounds = $results; Locked = [bool](Get-Process LogonUI -ErrorAction SilentlyContinue) } |
      ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'pointer.json')
  } -Passed "measured $Rounds rounds of $Moves pointer moves along the rail's lines"
  return
}

if ($Paint) {
  # 13's window, the Conductor's pad counting two, and a sixth tab that prints: what a frame costs
  # the GUI thread with the badge drawn in it, for a build before and after
  $Night = 'ffffffff-6666-4666-8666-666666666666'
  $Far = '12121212-7777-4777-8777-777777777777'
  [IO.File]::WriteAllText((Join-Path $summaries "$Night.label.txt"), 'Night build')
  $conductorFile = Join-Path $T.Out 'conductor.json'
  [IO.File]::WriteAllText($conductorFile, '{ "state": "on", "mode": "read", "tooltip": "Conductor is on (read only)" }')
  Write-Links @($wait) @($group, $pair, (Connection 'c-half' @($D, $Night) suggested), (Connection 'c-far' @($Night, $Far) suggested), $declined)
  $writer = '$e = [char]27; $sw = [Diagnostics.Stopwatch]::StartNew(); $i = 0; while ($sw.ElapsedMilliseconds -lt {0}) {{ $i++; [Console]::Out.Write("$e[38;5;$(17 + $i % 214)mline $i$e[0m the quick brown fox jumps over the lazy dog $e[1;34m$($sw.ElapsedMilliseconds) ms$e[0m`n"); Start-Sleep -Milliseconds 16 }}; "writer done after $i lines"' -f [int]($PaintSeconds * 1000)
  Initialize-CpuReader
  $rate = Get-CycleRate
  $results = [Collections.Generic.List[object]]::new()
  Invoke-Test {
    $w = Open-Rail @('lbterm_compositor=true', "lbterm_conductor={state_file='$(ConvertTo-LuaPath $conductorFile)'}")
    Wait-Change { param($s) @($s.rail.rows).Count -eq 5 -and $s.rail.conductor -and @($s.rail.links).Count -eq 1 } -What "five rows, the link and the Conductor's pad" -Animating | Out-Null
    $printer = [int](Invoke-Cli spawn --window-id 0 -Program (New-PwshProgram))
    $s = Wait-Change { param($s) @($s.rail.rows).Count -eq 6 -and @($s.rail.rows)[5].active } -What 'the printing tab open' -Animating
    "build $($T.Bin): the pad's badge $($s.rail.conductor.badge | ConvertTo-Json -Compress), $($s.rail.conductor.suggested) suggested here, $(@($s.rail.connections).Count) connections drawn"
    Invoke-Cli send-text --pane-id $printer --no-paste "function prompt { 'PS> ' }; cls`r" -Check | Out-Null
    Start-Sleep -Milliseconds 800
    # A screenful first, unmeasured, as paint-cost.ps1 does: the layers grow and the glyphs go in
    Invoke-Cli send-text --pane-id $printer --no-paste "$writer`r" -Check | Out-Null
    Start-Sleep -Milliseconds ([int]($PaintSeconds * 1000) + 500)
    Get-QuietFrame -QuietMs 1000 -TimeoutSec 20 | Out-Null
    for ($run = 1; $run -le $Rounds; $run++) {
      $f0 = Get-QuietFrame -QuietMs 700 -TimeoutSec 20
      $c0 = Get-CpuSnapshot -Window $w
      Invoke-Cli send-text --pane-id $printer --no-paste "$writer`r" -Check | Out-Null
      Start-Sleep -Milliseconds ([int]($PaintSeconds * 1000) + 500)
      $f1 = Get-QuietFrame -QuietMs 700 -TimeoutSec 20
      $c1 = Get-CpuSnapshot -Window $w
      $frames = $f1 - $f0
      $row = [pscustomobject]@{
        Run = $run; Frames = $frames
        GuiThreadMsFrame = [Math]::Round(($c1.ThreadCycles - $c0.ThreadCycles) / $rate / [Math]::Max(1, $frames), 3)
        ProcessMsFrame = [Math]::Round(($c1.Cycles - $c0.Cycles) / $rate / [Math]::Max(1, $frames), 3)
      }
      $results.Add($row)
      "run ${run}: $($row.Frames) frames, the GUI thread $($row.GuiThreadMsFrame) ms a frame, the process $($row.ProcessMsFrame) ms"
      if ($frames -lt 50) { Add-Failure "run ${run} painted $frames frames: the printing didn't paint" }
    }
    [pscustomobject]@{ Bin = $T.Bin; Seconds = $PaintSeconds; Runs = $results; Badge = $s.rail.conductor.badge; Locked = [bool](Get-Process LogonUI -ErrorAction SilentlyContinue) } |
      ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'paint.json')
  } -Passed "measured $Rounds runs of $PaintSeconds s printing with the Conductor's pad"
  return
}

Invoke-Test {
  # 1 to 4, what the rail draws, unless -Acts or -Badge
  if (-not $Acts -and -not $Badge) {
  # 1. The group of three, the suggested pair beside the link, the declined one nowhere
  Write-Links @($wait) @($group, $pair, $declined)
  $w = Open-Rail @('lbterm_compositor=true')
  $s = Wait-Change { param($s) Test-AllIn $s } -What 'five rows, two connections and the link' -Animating
  $rows = @($s.rail.rows)
  "rows: $(($rows | ForEach-Object { "$($_.label)=$($_.state)$(if (@($_.connected).Count) { '[' + (@($_.connected) -join ',') + ']' })" }) -join ', ')"
  $line = @($s.rail.links)[0]
  $byId = @{}; foreach ($x in @($s.rail.connections)) { $byId[$x.id] = $x }
  "lines: link $($line.waiter)->$($line.on) lane $($line.lane); $((@($s.rail.connections) | ForEach-Object { "$($_.id) $($_.state) rows $(@($_.rows) -join ',') lane $($_.lane)" }) -join '; ')"
  if ($line.waiter -ne 1 -or $line.on -ne 0 -or $line.lane -ne 0 -or $line.look -ne 'waiting') { Add-Failure "the link is $($line.waiter)->$($line.on) in lane $($line.lane), $($line.look)" }
  $g = $byId['c-group']; $p = $byId['c-pair']
  if (-not $g -or $g.state -ne 'connected' -or (@($g.rows) -join ',') -ne '0,2,4' -or $g.lane -ne 2) { Add-Failure "the group is $($g | ConvertTo-Json -Compress)" }
  if (-not $p -or $p.state -ne 'suggested' -or (@($p.rows) -join ',') -ne '1,3' -or $p.lane -ne 1) { Add-Failure "the pair is $($p | ConvertTo-Json -Compress)" }
  if ($byId['c-no']) { Add-Failure 'the declined connection is drawn' }
  foreach ($i in 0, 2, 4) { if ((@($rows[$i].connected) -join ',') -ne 'c-group') { Add-Failure "row $i says it's in '$(@($rows[$i].connected) -join ',')'" } }
  foreach ($i in 1, 3) { if (@($rows[$i].connected).Count) { Add-Failure "row $i says it's connected: $(@($rows[$i].connected) -join ',')" } }
  # A connection leaves each pad as it is: Heat steps waits on Lachlan, Tab Wave on Scroll strip
  if ($rows[0].state -ne 'idle' -or $rows[2].state -ne 'waiting' -or $rows[4].state -ne 'idle' -or $rows[1].state -ne 'linked') { Add-Failure "the pads changed: $(($rows | ForEach-Object state) -join ', ')" }
  $shot = Get-Shot $w -Name 'tab-connections'
  if ($g) {
    $n = Measure-Lane $shot $g.x @($g.ys)[0] @($g.ys)[-1] $cyanish
    "group: $($n.Pixels) cyan pixels down its lane over $($n.Length) rows"
    if ($n.Pixels -lt $n.Length) { Add-Failure "the group's line isn't drawn whole ($($n.Pixels) cyan pixels over $($n.Length) rows)" }
    for ($k = 0; $k -lt @($g.ys).Count; $k++) {
      $y = @($g.ys)[$k]; $row = @($g.rows)[$k]
      $mk = Measure-Mark $shot $g.mark_x $y
      "group mark on row $row at y $y`: $($mk.Ring) cyan round it, middle $($mk.Middle) of 4 cyan and $($mk.Purple) purple"
      if ($row -eq 0) {
        # Scroll strip is waited on by Tab Wave: the link's solid purple mark over the group's, rimmed in cyan
        if ($mk.Purple -lt 3 -or $mk.Ring -lt 8) { Add-Failure "row 0's marks aren't the link's purple rimmed in cyan (purple $($mk.Purple), cyan $($mk.Ring))" }
      } elseif ($mk.Ring -lt 16 -or $mk.Middle -lt 3) { Add-Failure "the group's mark at y $y isn't solid cyan (ring $($mk.Ring), middle $($mk.Middle))" }
    }
  }
  if ($p) {
    $n = Measure-Lane $shot $p.x @($p.ys)[0] @($p.ys)[-1] $cyanish
    "pair: $($n.Pixels) cyan pixels down its lane over $($n.Length) rows"
    if ($n.Pixels -lt $n.Length) { Add-Failure "the suggested line isn't drawn ($($n.Pixels) cyan pixels over $($n.Length) rows)" }
    for ($k = 0; $k -lt @($p.ys).Count; $k++) {
      $y = @($p.ys)[$k]; $row = @($p.rows)[$k]
      $mk = Measure-Mark $shot $p.mark_x $y
      "pair mark on row $row at y $y`: $($mk.Ring) cyan round it, middle $($mk.Middle) of 4 cyan and $($mk.Purple) purple"
      # On Tab Wave's row the link's hollow mark is over the pair's, rimmed in faint cyan
      if ($mk.Ring -lt 4 -or $mk.Middle -gt 1) { Add-Failure "the suggested mark on row $row isn't hollow with cyan round it (ring $($mk.Ring), middle $($mk.Middle))" }
    }
  }
  $shot.Dispose()

  # 2. The light, carried by DWM, running its laps since the suggestion came: in two places, the
  # window painting nothing for it
  $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  "loops: $(@($sw.loops) -join ', ')"
  if (@($sw.loops) -notcontains 'connection.light') { Add-Failure "DWM isn't carrying the light (loops: $(@($sw.loops) -join ', '))" }
  # Where the light is in captures over -Ms, along the pair's line: whether it was seen running
  $runs = { param([string]$What, [int]$Ms, [string]$Shots)
    $band = New-Rect ([int]$p.x - 4) ([int]@($p.ys)[0] - 12) 8 ([int](@($p.ys)[-1] - @($p.ys)[0]) + 24)
    $frames = Get-Frames $w $Ms
    $seen = @($frames | ForEach-Object { [pscustomobject]@{ Ms = $_.Ms; Y = [LbtermE2E.Px]::MeanYInRange($_.Shot, $band, $lit); Shot = $_.Shot } })
    $found = @($seen | Where-Object { $_.Y -ge 0 })
    "${What}: the light in $($found.Count) of $($seen.Count) captures, at y $((@($found | ForEach-Object { [int]$_.Y }) | Select-Object -Unique) -join ' ')"
    if ($found.Count -lt [Math]::Max(2, $seen.Count / 2)) { Add-Failure "${What}: the light shows in $($found.Count) of $($seen.Count) captures" }
    else {
      $lo = @($found | Sort-Object Y)[0]; $hi = @($found | Sort-Object Y)[-1]
      "${What}: lowest at y $([int]$hi.Y) ($($hi.Ms) ms), highest at y $([int]$lo.Y) ($($lo.Ms) ms)"
      if ($hi.Y - $lo.Y -lt 4) { Add-Failure "${What}: the light stays at y $([int]$lo.Y) to $([int]$hi.Y): it isn't running along the line" }
      if ($Shots) {
        Save-Shot $lo.Shot "$Shots-1"
        Save-Shot $hi.Shot "$Shots-2"
      }
    }
    Remove-Frames $frames
  }
  # Still and glowing at the line's middle, in three captures 600 ms apart
  # (Not $Shot: PowerShell's names ignore case, and the capture below is $shot)
  $still = { param([string]$What, [string]$SaveAs)
    $band = New-Rect ([int]$p.x - 4) ([int]@($p.ys)[0] - 12) 8 ([int](@($p.ys)[-1] - @($p.ys)[0]) + 24)
    $middle = (@($p.ys)[0] + @($p.ys)[-1]) / 2
    $ys = @(foreach ($k in 1..3) {
        $shot = Get-Shot $w -Name $(if ($k -eq 1) { $SaveAs })
        [LbtermE2E.Px]::MeanYInRange($shot, $band, $lit)
        $shot.Dispose()
        if ($k -lt 3) { Start-Sleep -Milliseconds 600 }
      })
    "${What}: the light at y $(($ys | ForEach-Object { [Math]::Round($_, 1) }) -join ', '), the line's middle $middle"
    # Where it rests (the line's middle, by value, nothing animated) is the compositor's given the
    # plan, which compositor/imp/tests/connections.rs holds (feat/dcomp-recorder): here, that it
    # glows, printed with where, and the capture for lbterm.ps1 look
    if (@($ys | Where-Object { $_ -lt 0 }).Count) { Add-Failure "${What}: no light glowing" }
  }
  # Its laps' length (lbterm_extras::links::glow_path), and a frame saying they're done
  $lapS = if ($p) { 2 * [Math]::Max([Math]::Abs(@($p.ys)[-1] - @($p.ys)[0]) / (70 * $s.size.dpi / 96), 0.9) } else { 1 }
  $done = { param([string]$What)
    try {
      Wait-State { param($s) @($s.loops) -notcontains 'connection.light' } -What "$What, the laps done" -TimeoutSec (3 * $lapS + 3) | Out-Null
      $true
    } catch { Add-Failure "${What}: the light still ran $([Math]::Round(3 * $lapS + 3, 1)) s on, past its three laps of $([Math]::Round($lapS, 2)) s"; $false }
  }
  $running = { param([string]$What, [long]$After = -1)
    try {
      Wait-State { param($s) @($s.loops) -contains 'connection.light' } -What "$What, the light running" -AfterFrame $After -TimeoutSec 3 | Out-Null
      $true
    } catch { Add-Failure "${What}: no frame within 3 s said the light runs"; $false }
  }
  if ($p) {
    & $runs 'its laps as it came' 3200 'tab-connections-light'
    # 2b. Then still, glowing at the middle, and the loop gone from the state file (lbterm_extras::glow)
    if (& $done 'as it came') {
      $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
      "after its laps, loops: $(@($sw.loops) -join ', ')"
      & $still 'after its laps' 'tab-connections-light-still'
    }
    # 2c. The pointer coming onto the rail runs them again
    $pane = @($s.panes)[0].rect
    Send-Mouse $w Move ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h / 2))
    Start-Sleep -Milliseconds 300
    $f = Get-StateFrame $w
    # On InMail's pad: a row's rect is its slot on the open panel, most of it over the pane while
    # the panel is shut
    $onPad = Get-RectCenter @($s.rail.rows)[4].pad
    Send-Mouse $w Move $onPad[0] $onPad[1]
    if (& $running 'the pointer onto the rail' $f) {
      'the pointer onto the rail: the light runs its laps again'
      # The pointer on the rail opens the panel too (open_delay_ms=0), and as it opens a light of
      # its own crosses the pair's band, faster than this one ever goes: the laps are judged once
      # the panel is open and settled, when only the connection light moves there (PITFALLS 65)
      try {
        Wait-State { param($sw) $sw.rail.open -and -not $sw.animating } -What 'the panel open and settled' -AfterFrame $f -TimeoutSec 5 | Out-Null
        & $runs 'its laps with the pointer on the rail, the panel open and settled' 1600 ''
      } catch { Add-Failure "the pointer onto the rail: the panel didn't open and settle within 5 s" }
    }
    Send-Mouse $w Move ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h / 2))
  }
  # Minimized while they run, the light holds still as the rail's other loops do
  # (lbterm_extras::work::loops_play: a loop keeps DWM composing every refresh, seen or not), and
  # restored it runs the laps it was cut off from again
  if (-not (Set-WindowMinimized $w)) { throw 'the window didn''t minimize' }
  try {
    Wait-State { param($s) @($s.loops) -notcontains 'connection.light' } -What 'a frame with the light still' -TimeoutSec 3 | Out-Null
    "minimized: the light still (loops $(@((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).loops) -join ', '))"
  } catch { Add-Failure "minimized: no frame within 3 s said the light was still" }
  Set-WindowMinimized $w -Restore | Out-Null
  try {
    Wait-State { param($s) @($s.loops) -contains 'connection.light' } -What 'a frame with the light running again' -TimeoutSec 3 | Out-Null
    'restored: the light runs again'
  } catch { Add-Failure "restored: no frame within 3 s said the light ran again" }
  if ($p -and (& $done 'restored')) {
    # 2d. The file saying something new of the pair runs them again
    $f = Get-StateFrame $w
    Write-Links @($wait) @($group, (Connection 'c-pair' @($B, $D) suggested -Why 'both write lbterm/render/thumbnails.rs and lbterm/LINKS.md'), $declined)
    if (& $running 'the file saying a new why' $f) { 'the file saying a new why: the light runs its laps again' }
  }
  Get-QuietFrame -QuietMs 700 -TimeoutSec 10 | Out-Null
  # The window's own frames while it runs, against as long with no suggestion
  Start-Sleep -Milliseconds 500
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds $IdleSeconds
  $with = (Get-Metrics).totals.frames
  Write-Links @($wait) @($group, $declined)
  Wait-Change { param($s) @($s.rail.connections).Count -eq 1 } -What 'the suggestion gone' -Animating | Out-Null
  $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  if (@($sw.loops) -contains 'connection.light') { Add-Failure 'the light still runs with no suggestion' }
  Start-Sleep -Milliseconds 500
  Get-Metrics -Reset | Out-Null
  Start-Sleep -Seconds $IdleSeconds
  $without = (Get-Metrics).totals.frames
  "frames in $IdleSeconds s: $with with the light running, $without with no suggestion"
  if ($with -gt $without + 1) { Add-Failure "the window painted $with frames in $IdleSeconds s with the light running, $without without" }
  Stop-TestWindow $w

  # 3. Without the compositor: no light, the suggestion dashed
  Write-Links @($wait) @($group, $pair, $declined)
  $w = Open-Rail @('lbterm_compositor=false')
  $s = Wait-Change { param($s) Test-AllIn $s } -What 'the connections without the compositor' -Animating
  if (@($s.loops) -contains 'connection.light') { Add-Failure 'a light without the compositor' }
  $p = @($s.rail.connections | Where-Object id -eq 'c-pair')[0]
  $shot = Get-Shot $w -Name 'tab-connections-flat'
  $starts = @([LbtermE2E.Px]::RunStartsDown($shot, [int]$p.x, $cyanish) | Where-Object { $_ -gt @($p.ys)[0] + 4 -and $_ -lt @($p.ys)[-1] - 4 })
  "without the compositor: the suggested line in $($starts.Count) dashes between its marks"
  if ($starts.Count -lt 4) { Add-Failure "the suggested line isn't dashed without the compositor ($($starts.Count) runs)" }
  $g = @($s.rail.connections | Where-Object id -eq 'c-group')[0]
  $n = Measure-Lane $shot $g.x @($g.ys)[0] @($g.ys)[-1] $cyanish
  if ($n.Pixels -lt $n.Length) { Add-Failure "the group's line isn't whole without the compositor ($($n.Pixels) over $($n.Length) rows)" }
  # Past Tab Wave's row with nothing across, across to Heat steps': the pixel between the group's
  # lane and the pair's, level with each row's mark (the pair's across starts right of it). Here,
  # where no light runs along the pair's lane beside it
  $rowY = { param($i) $pad = @($s.rail.rows)[$i].pad; [int][Math]::Round($pad.y + $pad.h / 2) }
  $gap = [int]$g.x + 1
  $passed = [LbtermE2E.Px]::CountRange($shot, (New-Rect $gap ((& $rowY 1) - 1) 1 2), $cyanish)
  $across = [LbtermE2E.Px]::CountRange($shot, (New-Rect $gap ((& $rowY 2) - 1) 1 2), $cyanish)
  "the group's lane beside Tab Wave's mark: $passed cyan (passed by); beside Heat steps': $across (across to its mark)"
  if ($passed -gt 0 -or $across -lt 1) { Add-Failure "the group's line doesn't pass Tab Wave by and reach Heat steps (beside them $passed and $across cyan)" }
  $shot.Dispose()
  Stop-TestWindow $w

  # 4. Motion off: the light still, at the middle of the line, and no loop
  $w = Open-Rail @('lbterm_compositor=true', "lbterm_motion={enabled='off'}")
  $s = Wait-Change { param($s) Test-AllIn $s } -What 'the connections with motion off' -Animating
  "motion off, loops: $(@($s.loops) -join ', ')"
  if (@($s.loops) -contains 'connection.light') { Add-Failure 'the light turns with motion off' }
  $p = @($s.rail.connections | Where-Object id -eq 'c-pair')[0]
  $band = New-Rect ([int]$p.x - 4) ([int]@($p.ys)[0] - 12) 8 ([int](@($p.ys)[-1] - @($p.ys)[0]) + 24)
  $middle = (@($p.ys)[0] + @($p.ys)[-1]) / 2
  $ys = @(foreach ($k in 1..3) {
      $shot = Get-Shot $w -Name $(if ($k -eq 1) { 'tab-connections-still' })
      [LbtermE2E.Px]::MeanYInRange($shot, $band, $lit)
      $shot.Dispose()
      Start-Sleep -Milliseconds 600
    })
  "motion off: the light at y $(($ys | ForEach-Object { [Math]::Round($_, 1) }) -join ', '), the line's middle $middle"
  # Still at the line's middle with motion off: compositor/imp/tests/connections.rs, from what
  # DWM was handed; here, that it glows and no loop runs (the window's motion reaching the plan)
  if (@($ys | Where-Object { $_ -lt 0 }).Count) { Add-Failure 'no light with motion off' }
  Stop-TestWindow $w
  }

  # 5 to 12: what Lachlan does, each event through the window's handler, unless -Badge. The group
  # has a note and five messages, and Tab Wave says it waits for Scroll strip's objective 1
  if (-not $Badge) {
  Write-Objectives $A 1 @(Objective 1 open "Draw the rail's lines")
  Write-Objectives $B 2 @((Objective 1 done 'Build the tab wave'), (Objective 2 open 'Land feat/tab-links in LBTerm'))
  Write-Objectives $C 1 @(Objective 1 waiting 'Lower the heat steps')
  Write-Objectives $D 1 @(Objective 1 open 'Build the usage chart')
  Write-Objectives $E 1 @(Objective 1 open 'Read the InMail')
  $sent = @(1..5 | ForEach-Object {
      $from, $to = if ($_ % 2) { $A, $C } else { $C, $A }
      [ordered]@{ from = $from; to = $to; at = 1793250000000 + $_ * 60000; text = "message $_ about the rail" }
    })
  $note = "Couldn't tell InMail: its session isn't running"
  $talking = Connection 'c-group' @($A, $C, $E) connected -By lbterm -Why 'all three draw on the rail' -Note $note -Messages $sent
  $waitsOn = [ordered]@{ session = $B; what = "the rail's lines drawn"; at = 1; on = $A; objective = 1; matchedAt = 2 }
  Write-Links @($wait) @($talking, $pair, $declined) @($waitsOn)
  $w = Open-Rail @('lbterm_compositor=true')
  try { $s = Wait-Change { param($s) Test-AllIn $s } -What 'the rail for what he does' -Animating }
  catch {
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    "the rail said: rows $(@($sw.rail.rows).Count), links $(($sw.rail.links | ConvertTo-Json -Compress -Depth 4)), connections $(($sw.rail.connections | ForEach-Object { "$($_.id) $(@($_.rows) -join ',')" }) -join '; ')"
    throw
  }
  $pane = @($s.panes)[0].rect
  $away = @([int]($pane.x + $pane.w / 2), [int]($pane.y + $pane.h / 2))
  $rowY = { param($s, $i) $pad = @($s.rail.rows)[$i].pad; [int][Math]::Round($pad.y + $pad.h / 2) }
  $rowsOf = { param($s, [string]$Id) @(@($s.rail.connections | Where-Object id -eq $Id) | ForEach-Object { @($_.rows) -join ',' })[0] }
  # The pointer resting on a connection's lane between its first two marks, the panel shut first
  function Watch-Card([string]$Id) {
    Send-Mouse $w Move $away[0] $away[1]
    $s = Wait-Change { param($s) -not $s.rail.open -and -not $s.rail.line_card } -What 'the panel shut' -Animating
    $line = @($s.rail.connections | Where-Object id -eq $Id)[0]
    $at = @([int]$line.x, [int]((@($line.ys)[0] + @($line.ys)[1]) / 2))
    Send-Mouse $w Move $at[0] $at[1]
    try { Wait-State { param($s) $s.rail.line_card -and $s.rail.line_card.connection -eq $Id } -What "the card of $Id's line" }
    catch {
      $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
      Write-Host "resting at $($at -join ', ') on $Id ($($line | ConvertTo-Json -Compress)): the rail open $($sw.rail.open), hovered $($sw.rail.hovered), card $($sw.rail.line_card | ConvertTo-Json -Compress), connections $(($sw.rail.connections | ForEach-Object { "$($_.id) $($_.state) lane $($_.lane) x $($_.x)" }) -join '; ')"
      Get-Shot $w -Name "tab-connections-no-card-$Id" | ForEach-Object Dispose
      throw
    }
  }
  # A right press on row $From's pad dragged to row $To's slot, 70% across the open panel, with
  # -Keys held besides the button; where it ends
  function Send-RailDrag([int]$From, [int]$To, [string[]]$Keys = @()) {
    Send-Mouse $w Move $away[0] $away[1]
    $s = Wait-Change { param($s) -not $s.rail.link_drag -and -not $s.menu } -What 'no drag or menu' -Animating
    $start = Get-RectCenter @($s.rail.rows)[$From].pad
    $r = @($s.rail.rows)[$To].rect
    $end = @([int]($r.x + $r.w * 0.7), (& $rowY $s $To))
    Send-Mouse $w Move $start[0] $start[1]
    Send-Mouse $w RDown $start[0] $start[1] -Keys (@('R') + $Keys)
    foreach ($k in 1..10) {
      Send-Mouse $w Move ([int]($start[0] + ($end[0] - $start[0]) * $k / 10)) ([int]($start[1] + ($end[1] - $start[1]) * $k / 10)) -Keys (@('R') + $Keys)
      Start-Sleep -Milliseconds 30
    }
    $end
  }
  # How many of five places along the drag's line, from row $Row's mark to $To, are $Colour
  function Measure-DragLine($shot, $s, [int]$Row, $To, [int]$Colour) {
    $x0 = @($s.rail.connections)[0].mark_x; $y0 = & $rowY $s $Row
    @(foreach ($t in 0.3, 0.4, 0.5, 0.6, 0.7) {
        $x = $x0 + ($To[0] - $x0) * $t; $y = $y0 + ($To[1] - $y0) * $t
        if ([LbtermE2E.Px]::CountNear($shot, (New-Rect ([int]$x - 3) ([int]$y - 3) 7 7), $Colour, 50) -ge 2) { $t }
      }).Count
  }

  # 5. Resting on the suggested pair's line: its card, the panel shut
  $s = Watch-Card 'c-pair'
  $card = $s.rail.line_card
  $words = @($card.lines) -join ' '
  "the pair's card: $(@($card.lines) -join ' / ')"
  foreach ($want in 'Tab Wave and Usage chart could be talking', 'both write lbterm/render/thumbnails.rs', 'A click on the line connects them') {
    if (-not $words.Contains($want)) { Add-Failure "the suggestion's card hasn't '$want'" }
  }
  if ($s.rail.open) { Add-Failure 'resting on a connection opened the panel' }
  if ($card.rect.x -lt $s.rail.rect.x + $s.rail.rect.w) { Add-Failure "the suggestion's card covers the rail" }
  $shot = Get-Shot $w -Name 'tab-connections-card-suggested'
  $n = [LbtermE2E.Px]::CountRange($shot, (ConvertTo-Rect $card.rect), $cyanish)
  "the pair's card: $n cyan pixels (how to connect them)"
  if ($n -lt 10) { Add-Failure "the suggestion's card has no cyan ($n pixels)" }
  $shot.Dispose()

  # 6. Usage chart's menu: its suggestion's rows, the menu widened for them; Decline fires decline
  Send-Mouse $w Move $away[0] $away[1]
  $s = Wait-Change { param($s) -not $s.rail.line_card } -What 'the card gone' -Animating
  $p3 = Get-RectCenter @($s.rail.rows)[3].pad
  Send-Click $w $p3[0] $p3[1] -Right -Move
  $s = Wait-Change { param($s) $s.menu -and $s.menu.kind -eq 'tab' } -What "Usage chart's menu"
  $labels = @($s.menu.items | ForEach-Object label)
  "Usage chart's menu: $($labels -join ' | ')"
  foreach ($want in 'Connect with Tab Wave, as suggested', 'Decline the suggested connection with Tab Wave') {
    if ($labels -notcontains $want) { Add-Failure "Usage chart's menu hasn't '$want'" }
  }
  if (@($labels | Where-Object { $_ -match 'Scroll strip' }).Count) { Add-Failure "Usage chart's menu offers its declined connection with Scroll strip" }
  $wide = 260 * $s.size.dpi / 96
  "the menu: $($s.menu.rect.w) px wide (260 at 96 DPI is $wide here)"
  if ($s.menu.rect.w -le $wide) { Add-Failure "the menu didn't widen for its connection rows ($($s.menu.rect.w) px)" }
  $decline = @($s.menu.items | Where-Object label -eq 'Decline the suggested connection with Tab Wave')[0]
  if ($decline) {
    $mid = Get-RectCenter $decline.rect
    Send-Click $w $mid[0] $mid[1] -Move
    Test-Event (Wait-Event 1 'Decline') @{ action = 'decline'; connection = 'c-pair' } 'Decline'
    Wait-Change { param($s) -not $s.menu -and -not (& $rowsOf $s 'c-pair') } -What 'the declined pair gone at once' -Animating | Out-Null
  }
  # As links.ts would have it: declined; then suggested again, as a new one
  $pairNo = Connection 'c-pair' @($B, $D) declined -Why 'both write lbterm/render/thumbnails.rs'
  $pair2 = Connection 'c-pair-2' @($B, $D) suggested -Why 'both write lbterm/render/thumbnails.rs'
  Write-Links @($wait) @($talking, $pairNo, $pair2, $declined) @($waitsOn)

  # 7. A left click on the suggested line: accept, drawn connected at once
  $s = Wait-Change { param($s) (& $rowsOf $s 'c-pair-2') -eq '1,3' } -What 'the pair suggested again' -Animating
  $p = @($s.rail.connections | Where-Object id -eq 'c-pair-2')[0]
  Send-Click $w ([int]$p.x) ([int]((@($p.ys)[0] + @($p.ys)[1]) / 2)) -Move
  Test-Event (Wait-Event 2 'the suggested line clicked') @{ action = 'accept'; connection = 'c-pair-2' } 'A click on the suggested line'
  Wait-Change { param($s) @($s.rail.connections | Where-Object { $_.id -eq 'c-pair-2' -and $_.state -eq 'connected' }).Count -eq 1 } -What 'the pair connected at once' -Animating | Out-Null
  $pair2.state = 'connected'
  Write-Links @($wait) @($talking, $pairNo, $pair2, $declined) @($waitsOn)

  # 8. Resting on the group's line: who's in it, why, the note in red, the last three messages
  $s = Watch-Card 'c-group'
  $card = $s.rail.line_card
  $words = @($card.lines) -join ' '
  "the group's card: $(@($card.lines) -join ' / ')"
  foreach ($want in 'Connected: Scroll strip, Heat steps, InMail', 'all three draw on the rail', $note, 'message 3 about the rail', 'message 4 about the rail', 'message 5 about the rail') {
    if (-not $words.Contains($want)) { Add-Failure "the group's card hasn't '$want'" }
  }
  foreach ($old in 'message 1 ', 'message 2 ') { if ($words.Contains($old)) { Add-Failure "the group's card shows '$old', older than the last three" } }
  if ($words -notmatch 'Scroll strip to Heat steps, \d\d:\d\d: message 3' -or $words -notmatch 'Heat steps to Scroll strip, \d\d:\d\d: message 4') { Add-Failure "the messages aren't '<from> to <to>, hh:mm: <text>'" }
  $shot = Get-Shot $w -Name 'tab-connections-card'
  $n = [LbtermE2E.Px]::CountNear($shot, (ConvertTo-Rect $card.rect), $red, 40)
  "the group's card: $n red pixels (the note)"
  if ($n -lt 10) { Add-Failure "the group's note isn't red ($n pixels)" }
  $shot.Dispose()

  # 9. Scroll strip's menu: Leave fires leave, the group shrinking to its other two at once
  Send-Mouse $w Move $away[0] $away[1]
  $s = Wait-Change { param($s) -not $s.rail.line_card } -What 'the card gone' -Animating
  $p0 = Get-RectCenter @($s.rail.rows)[0].pad
  Send-Click $w $p0[0] $p0[1] -Right -Move
  $s = Wait-Change { param($s) $s.menu -and $s.menu.kind -eq 'tab' } -What "Scroll strip's menu"
  "Scroll strip's menu: $(@($s.menu.items | ForEach-Object label) -join ' | ')"
  $leave = @($s.menu.items | Where-Object label -eq 'Leave the connection with Heat steps and InMail')[0]
  if (-not $leave) { Add-Failure "Scroll strip's menu hasn't 'Leave the connection with Heat steps and InMail'" }
  else {
    $mid = Get-RectCenter $leave.rect
    Send-Click $w $mid[0] $mid[1] -Move
    Test-Event (Wait-Event 3 'Leave') @{ action = 'leave'; connection = 'c-group'; session = $A } 'Leave'
    Wait-Change { param($s) -not $s.menu -and (& $rowsOf $s 'c-group') -eq '2,4' } -What 'the group shrunk at once' -Animating | Out-Null
  }
  $talking.members = @($C, $E)
  Write-Links @($wait) @($talking, $pairNo, $pair2, $declined) @($waitsOn)
  Wait-State { param($s) (& $rowsOf $s 'c-group') -eq '2,4' } -What 'the group shrunk in the file' | Out-Null

  # 10. Tab Wave Shift-dragged onto Scroll strip: cyan, both tabs' objectives, nothing lit
  # The active pane's outline first, Shift held over the pane: green (held.rs), as the drag's
  # outline is judged against. Measured along the right half of its card's bottom edge, clear of
  # the open panel and the drag's cards, as modifier-light.ps1 measures it
  $heldGreen = New-ColourRange 0 100 170 255 60 160
  $edge = New-Rect ([int]($pane.x + $pane.w / 2)) ([int]($pane.y + $pane.h) - 5) ([int]($pane.w / 2) - 30) 10
  $long = [int]($edge.Width / 4)
  $outline = { param($shot) [LbtermE2E.Px]::CountRange($shot, $edge, $heldGreen) }
  Send-Mouse $w Move $away[0] $away[1] -Keys Shift
  $s = Wait-Change { param($s) (@($s.held) -join ',') -eq 'shift' -and (@($s.tint) -join ',') -eq 'shift' } -What 'Shift held over the pane, the outline tinted'
  $shot = Get-Shot $w -Name 'tab-connections-shift'
  $base = & $outline $shot
  $shot.Dispose()
  "Shift held over the pane: the outline green at $base of the $($edge.Width) pixels along its edge"
  if ($base -lt $long) { Add-Failure "with Shift held the outline isn't green ($base pixels along its edge, want $long), so the drag's can't be judged" }
  $to = Send-RailDrag 1 0 @('Shift')
  $s = Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.over -eq 0 -and $s.rail.link_drag.connect } -What 'the Shift-drag over Scroll strip' -Animating
  # Shift held through the drag connects, and the drag is cyan for it: the outline isn't tinted
  # (Lachlan's 2a), once the tint has gone
  $s = Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.connect -and (@($s.held) -join ',') -eq 'shift' } -What 'the Shift-drag settled'
  if (@($s.tint).Count) { Add-Failure "while the Shift-drag is under way the outline shows $(@($s.tint) -join ', ')" }
  $drag = $s.rail.link_drag
  "Shift-drag: row $($drag.row) over $($drag.over), connect $($drag.connect), matched $($drag.matched); cards: $(@($drag.waits_words) -join ' / ') || $(@($drag.aims_words) -join ' / ')"
  $mine = @($drag.waits_words) -join ' '; $theirs = @($drag.aims_words) -join ' '
  if ($drag.row -ne 1 -or $drag.matched -or $drag.waits_match -or $drag.aims_match) { Add-Failure "the Shift-drag is row $($drag.row), matched $($drag.matched)" }
  if (@($drag.waits_words)[0] -ne 'Tab Wave' -or -not $mine.Contains('Land feat/tab-links in LBTerm') -or -not $mine.Contains('Build the tab wave')) { Add-Failure "the dragged tab's card isn't its objectives: $mine" }
  if (@($drag.aims_words)[0] -ne 'Scroll strip' -or -not $theirs.Contains("Draw the rail's lines")) { Add-Failure "the card beside Scroll strip isn't its objectives: $theirs" }
  $shot = Get-Shot $w -Name 'tab-connections-drag'
  $n = Measure-DragLine $shot $s 1 $to $cyan
  "Shift-drag: the line cyan at $n of 5 places"
  if ($n -lt 4) { Add-Failure "the Shift-drag's line isn't cyan ($n of 5 places)" }
  $during = & $outline $shot
  "Shift-drag: the outline green at $during pixels along its edge (held $(@($s.held) -join ','), tint $(@($s.tint) -join ','))"
  if ($during -gt 5) { Add-Failure "while the Shift-drag is under way the outline is green ($during pixels along its edge)" }
  $shot.Dispose()
  # Side by side here. In a window too narrow for two objectives cards beside each other, Scroll
  # strip's goes under Tab Wave's (or over it), not across it, and stays in the window
  $mineRect = ConvertTo-Rect $drag.waits; $theirsRect = ConvertTo-Rect $drag.aims
  if ($mineRect.IntersectsWith($theirsRect)) { Add-Failure "the Shift-drag's cards overlap ($($drag.waits | ConvertTo-Json -Compress), $($drag.aims | ConvertTo-Json -Compress))" }
  $outer = Get-WindowRect $w; $client = Get-ClientRect $w
  $wideWidth = [int]$s.size.width
  $narrow = [int]($mineRect.Right + 160 * $s.size.dpi / 96)
  Set-WindowSize $w ($narrow + ($outer.Right - $outer.Left) - $client.Width) ($outer.Bottom - $outer.Top)
  $s = Wait-Change { param($s) $s.size.width -eq $narrow -and $s.rail.link_drag -and $s.rail.link_drag.connect -and $s.rail.link_drag.aims } -What "the Shift-drag in a window $narrow px wide" -Animating
  $drag = $s.rail.link_drag
  $mineRect = ConvertTo-Rect $drag.waits; $theirsRect = ConvertTo-Rect $drag.aims
  "Shift-drag $narrow px wide: Tab Wave's card $($drag.waits | ConvertTo-Json -Compress), Scroll strip's $($drag.aims | ConvertTo-Json -Compress)"
  if ($mineRect.IntersectsWith($theirsRect)) { Add-Failure "in a window $narrow px wide the Shift-drag's cards overlap ($($drag.waits | ConvertTo-Json -Compress), $($drag.aims | ConvertTo-Json -Compress))" }
  if ($theirsRect.Right -gt $narrow -or $theirsRect.Bottom -gt $s.size.height -or $theirsRect.Top -lt 0) { Add-Failure "in a window $narrow px wide Scroll strip's card $($drag.aims | ConvertTo-Json -Compress) runs out of it" }
  Get-Shot $w -Name 'tab-connections-drag-narrow' | ForEach-Object Dispose
  Set-WindowSize $w ($outer.Right - $outer.Left) ($outer.Bottom - $outer.Top)
  Wait-Change { param($s) $s.size.width -eq $wideWidth -and $s.rail.link_drag -and $s.rail.link_drag.connect } -What 'the window as wide as it was' -Animating | Out-Null
  # Shift let go mid-drag: a wait link's drag again, its match lit green
  Send-Mouse $w Move ($to[0] + 2) $to[1] -Keys R
  $s = Wait-Change { param($s) $s.rail.link_drag -and -not $s.rail.link_drag.connect } -What 'Shift let go mid-drag' -Animating
  $drag = $s.rail.link_drag
  "Shift let go: connect $($drag.connect), matched $($drag.matched), card: $(@($drag.waits_words) -join ' / ')"
  if (-not $drag.matched -or @($drag.waits_words)[1] -ne 'waits for') { Add-Failure "let go of Shift, the drag isn't a link's matched again (matched $($drag.matched))" }
  $shot = Get-Shot $w
  $n = Measure-DragLine $shot $s 1 @(($to[0] + 2), $to[1]) $green
  "Shift let go: the line green at $n of 5 places"
  if ($n -lt 4) { Add-Failure "let go of Shift, the drag's line isn't the match's green ($n of 5 places)" }
  $shot.Dispose()
  # Held again and let go: connect, Scroll strip drawn joining Tab Wave's connection
  Send-Mouse $w Move $to[0] $to[1] -Keys R, Shift
  Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.connect } -What 'Shift held again' -Animating | Out-Null
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  if (@($s.tint).Count) { Add-Failure "Shift held again mid-drag, the outline shows $(@($s.tint) -join ', ')" }
  Send-Mouse $w RUp $to[0] $to[1] -Keys Shift
  Test-Event (Wait-Event 4 'the Shift-drag let go') @{ action = 'connect'; waiter = $B; on = $A } 'The Shift-drag'
  Wait-Change { param($s) -not $s.rail.link_drag -and (& $rowsOf $s 'c-pair-2') -eq '0,1,3' } -What "Scroll strip joining Tab Wave's connection at once" -Animating | Out-Null
  # The drag over, Shift still held: the outline green again
  $s = Wait-Change { param($s) -not $s.rail.link_drag -and (@($s.tint) -join ',') -eq 'shift' } -What 'the drag let go with Shift held, the outline tinted again'
  $shot = Get-Shot $w -Name 'tab-connections-drag-after'
  $after = & $outline $shot
  $shot.Dispose()
  "the drag let go with Shift held: the outline green at $after pixels along its edge"
  if ($after -lt $long) { Add-Failure "the drag let go with Shift still held, the outline isn't green again ($after pixels along its edge, want $long)" }
  $pair2.members = @($B, $D, $A)
  Write-Links @($wait) @($talking, $pairNo, $pair2, $declined) @($waitsOn)
  Wait-State { param($s) (& $rowsOf $s 'c-pair-2') -eq '0,1,3' } -What 'the three in the file' | Out-Null

  # 11. Usage chart Shift-dragged onto Scroll strip, connected together: it's taken out
  $to = Send-RailDrag 3 0 @('Shift')
  Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.over -eq 0 -and $s.rail.link_drag.connect } -What 'Usage chart over Scroll strip' -Animating | Out-Null
  Send-Mouse $w RUp $to[0] $to[1] -Keys Shift
  Test-Event (Wait-Event 5 'the Shift-drag between two connected') @{ action = 'connect'; waiter = $D; on = $A } 'The Shift-drag onto a tab it was connected with'
  Wait-Change { param($s) -not $s.rail.link_drag -and (& $rowsOf $s 'c-pair-2') -eq '0,1' } -What 'Usage chart out of the connection at once' -Animating | Out-Null
  $pair2.members = @($B, $A)
  Write-Links @($wait) @($talking, $pairNo, $pair2, $declined) @($waitsOn)

  # 12. Shift let go before the button: a wait link's toggle
  $to = Send-RailDrag 3 2 @('Shift')
  Wait-Change { param($s) $s.rail.link_drag -and $s.rail.link_drag.over -eq 2 -and $s.rail.link_drag.connect } -What 'Usage chart over Heat steps' -Animating | Out-Null
  Send-Mouse $w Move ($to[0] + 2) $to[1] -Keys R
  Wait-Change { param($s) $s.rail.link_drag -and -not $s.rail.link_drag.connect } -What 'Shift let go' -Animating | Out-Null
  Send-Mouse $w RUp ($to[0] + 2) $to[1]
  Test-Event (Wait-Event 6 'the drag let go without Shift') @{ action = 'toggle'; waiter = $D; on = $C } 'The drag let go without Shift'
  Wait-Change { param($s) -not $s.rail.link_drag -and @($s.rail.links | Where-Object { $_.waiter -eq 3 -and $_.on -eq 2 }).Count -eq 1 } -What 'the wait link drawn at once' -Animating | Out-Null
  if ((Read-Events).Count -ne 6) { Add-Failure "$((Read-Events).Count) events fired, not 6" }
  Stop-TestWindow $w
  }

  # 13. The Conductor's pad counting the suggestions with a member on this rail, unless -Acts. Night
  # build is a session in no tab of this window, named by its label in the summaries, and Far
  # another; the conductor's state file says it's on, reading only
  if (-not $Acts) {
  $Night = 'ffffffff-6666-4666-8666-666666666666'
  $Far = '12121212-7777-4777-8777-777777777777'
  [IO.File]::WriteAllText((Join-Path $summaries "$Night.label.txt"), 'Night build')
  $conductorFile = Join-Path $T.Out 'conductor.json'
  # With its remote control's mark on the pad's other corner, which the badge stays clear of
  [IO.File]::WriteAllText($conductorFile, '{ "state": "remote", "mode": "read", "tooltip": "Conductor is on (read only), with remote control" }')
  $half = Connection 'c-half' @($D, $Night) suggested -Why 'both read the agenda'
  $gone = Connection 'c-far' @($Night, $Far) suggested -Why 'both elsewhere'
  Write-Links @($wait) @($group, $pair, $half, $gone, $declined)
  $w = Open-Rail @('lbterm_compositor=true', "lbterm_conductor={state_file='$(ConvertTo-LuaPath $conductorFile)'}")
  try { $s = Wait-Change { param($s) (Test-AllIn $s) -and $s.rail.conductor -and $s.rail.conductor.suggested -eq 2 -and $s.rail.conductor.mark } -What "the Conductor's pad counting the pair and Usage chart's, its remote mark on" -Animating }
  catch {
    $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
    "the rail said: rows $(@($sw.rail.rows).Count), conductor $($sw.rail.conductor | ConvertTo-Json -Compress), connections $(($sw.rail.connections | ForEach-Object { "$($_.id) $(@($_.rows) -join ',')" }) -join '; ')"
    throw
  }
  $head = $s.rail.conductor
  "the Conductor's pad: $($head.suggested) suggested here, its badge $($head.badge | ConvertTo-Json -Compress) on the pad $($head.pad | ConvertTo-Json -Compress), its remote mark $($head.mark | ConvertTo-Json -Compress)"
  $chip = ConvertTo-Rect $head.badge
  $padRect = ConvertTo-Rect $head.pad
  $markRect = ConvertTo-Rect $head.mark
  $zoomOn = New-Rect ($padRect.X - 14) ($padRect.Y - 14) ($padRect.Width + 28) ($padRect.Height + 26)
  if (-not ($chip.Left -lt $padRect.Left -and $chip.Right -gt $padRect.Left -and $chip.Top -lt $padRect.Top -and $chip.Bottom -gt $padRect.Top)) { Add-Failure "the badge $($head.badge | ConvertTo-Json -Compress) isn't on the pad's top left corner" }
  # The count, in the pads' numbers' font (his "use a bigger one"): inside the badge, short of its
  # soft edge (all cyan without a count), the pixels well darker than the cyan, by their green
  # (under 0x90 the ink, 0x60 or under dark). Its shape is a disc for one digit and a pill for two,
  # its ends round. What reads is how many are dark, and the 1's stem (Measure-Stem): the 9 pt
  # count as the title font hints it had 10 dark pixels in its 2, 16 in its 12 and 6 in its 1
  # drawn once, 19, 28 and 9 drawn twice over itself, its 1's stem half-toned; auto-hinted and
  # drawn twice, 19, 35 and 16 (2026-11-02; the second coat darkens half-done edges, so more pixels
  # pass 0x90 and the middle of the ink's greens says less). The 7 pt labels' digit it was: its strokes' greens 111 drawn once, 83
  # twice, and a 1's stem 130. Which pixels they are is the digit's shape: a 1 has to differ from
  # a 2 in them, not only in the state file (PITFALLS 37)
  function Measure-Count($Shot, $Chip) {
    $r = $Chip.Height / 2; $cy = $Chip.Y + $r; $reach = $r - 1.5
    $x0 = $Chip.X + $r; $x1 = $Chip.Right - $r
    $inside = 0; $greens = [Collections.Generic.List[int]]::new(); $where = [Collections.Generic.HashSet[string]]::new()
    for ($y = $Chip.Top; $y -lt $Chip.Bottom; $y++) {
      for ($x = $Chip.Left; $x -lt $Chip.Right; $x++) {
        $nx = [Math]::Min([Math]::Max($x + 0.5, $x0), $x1)
        if ([Math]::Sqrt([Math]::Pow($x + 0.5 - $nx, 2) + [Math]::Pow($y + 0.5 - $cy, 2)) -gt $reach) { continue }
        $inside++
        $px = $Shot.GetPixel($x, $y)
        if ($px.G -lt 0x90) { $greens.Add([int]$px.G); [void]$where.Add("$($x - $Chip.Left),$($y - $Chip.Top)") }
      }
    }
    $sorted = @($greens | Sort-Object)
    $ink = $sorted.Count
    [pscustomobject]@{
      Inside = $inside; Ink = $ink; Pixels = $where; Dark = @($sorted | Where-Object { $_ -le 0x60 }).Count
      Low = if ($ink) { $sorted[0] }; High = if ($ink) { $sorted[-1] }
      Median = if ($ink) { $sorted[[int][Math]::Floor(($ink - 1) / 2)] } else { 255 }
    }
  }
  # The thinnest stroke, a 1's stem (PITFALLS 64): a count of dark pixels passed a 1 whose base and
  # flag were dark while its stem was two columns of green 102 and 105 on the cyan's 204 at 96
  # DPI, a pixel-wide stroke falling across two columns, which a second coat darkens without
  # making dark. Its columns are the one with the most ink in the 1 alone (left of the ink's
  # middle for 12) and its fuller neighbour when that has half as much; its rows are those whose
  # ink lies within a column of them, the flag's and the base's left out; the darkest pixel in
  # each must be 0x60 or darker. -Rows is each row's darkest green, top to bottom
  function Measure-Stem($Shot, $Chip, [switch]$LeftDigit) {
    $r = $Chip.Height / 2; $cy = $Chip.Y + $r; $reach = $r - 1.5
    $x0 = $Chip.X + $r; $x1 = $Chip.Right - $r
    $ink = @{}
    for ($y = $Chip.Top; $y -lt $Chip.Bottom; $y++) {
      for ($x = $Chip.Left; $x -lt $Chip.Right; $x++) {
        $nx = [Math]::Min([Math]::Max($x + 0.5, $x0), $x1)
        if ([Math]::Sqrt([Math]::Pow($x + 0.5 - $nx, 2) + [Math]::Pow($y + 0.5 - $cy, 2)) -gt $reach) { continue }
        $g = [int]$Shot.GetPixel($x, $y).G
        if ($g -lt 0x90) { $ink["$x,$y"] = [pscustomobject]@{ X = $x; Y = $y; G = $g } }
      }
    }
    $pixels = @($ink.Values)
    if ($LeftDigit -and $pixels.Count) {
      $xs = @($pixels | ForEach-Object X | Sort-Object)
      $middle = ($xs[0] + $xs[-1]) / 2
      $pixels = @($pixels | Where-Object { $_.X -lt $middle })
    }
    if (-not $pixels.Count) { return [pscustomobject]@{ Columns = @(); Rows = @(); Pale = 0 } }
    # The column with the most ink, and beside it the fuller of its neighbours when that has half
    # as much at least: a stem fitted to the pixels is one column, one falling across two is two
    $counts = @{}; foreach ($col in $pixels | Group-Object X) { $counts[[int]$col.Name] = $col.Count }
    $top = @($counts.GetEnumerator() | Sort-Object Value -Descending)[0]
    $side = @(($top.Key - 1), ($top.Key + 1) | Where-Object { $counts.ContainsKey($_) } | Sort-Object { $counts[$_] } -Descending)
    $columns = @($top.Key) + @($side | Select-Object -First 1 | Where-Object { $counts[$_] * 2 -ge $top.Value }) | Sort-Object
    $lo = $columns[0] - 1; $hi = $columns[-1] + 1
    $rows = @($pixels | Group-Object Y | Where-Object { @($_.Group | Where-Object { $_.X -lt $lo -or $_.X -gt $hi }).Count -eq 0 } |
        Sort-Object { [int]$_.Name } | ForEach-Object { ($_.Group | Measure-Object G -Minimum).Minimum })
    [pscustomobject]@{ Columns = $columns; Rows = $rows; Pale = @($rows | Where-Object { $_ -gt 0x60 }).Count }
  }
  # One capture of the badge: cyan, its count dark and read clearly, clear of the remote mark (no
  # cyan in the mark's square)
  function Measure-Badge([string]$Name, $Chip, [string]$Count) {
    $shot = Get-Shot $w -Name $Name
    $fill = [LbtermE2E.Px]::CountNear($shot, $Chip, $cyan, 40)
    $inMark = [LbtermE2E.Px]::CountNear($shot, $markRect, $cyan, 40)
    $m = Measure-Count $shot $Chip
    $stem = if ($Count -match '^1') { Measure-Stem $shot $Chip -LeftDigit:($Count.Length -gt 1) }
    Save-Zoomed $shot $zoomOn "$Name-zoom" -Zoom 6
    $shot.Dispose()
    Write-Host "the badge at ${Count}: $($Chip.Width) by $($Chip.Height), $fill of its $($Chip.Width * $Chip.Height) pixels cyan; inside it $($m.Ink) of $($m.Inside) the count's, their green from $($m.Low) to $($m.High), the middle one $($m.Median), $($m.Dark) of them 0x60 or darker (the cyan's is $(0xCC), the ink's $(0x1C)); $inMark cyan pixels in the remote mark's square"
    if ($fill -lt $Chip.Width * $Chip.Height / 3) { Add-Failure "the badge at $Count isn't cyan ($fill of $($Chip.Width * $Chip.Height) pixels)" }
    if ($m.Dark -lt 8) { Add-Failure "$Count doesn't read on the badge ($($m.Dark) of its $($m.Ink) pixels 0x60 or darker)" }
    if ($stem) {
      Write-Host "the 1's stem at ${Count}: columns $($stem.Columns -join ' and ') from the badge's left $($Chip.Left), $($stem.Rows.Count) rows, each one's darkest green $($stem.Rows -join ' '), $($stem.Pale) of them paler than 0x60"
      if ($stem.Rows.Count -lt 3) { Add-Failure "the 1's stem at $Count wasn't found ($($stem.Rows.Count) rows in its columns $($stem.Columns -join ', '))" }
      elseif ($stem.Pale) { Add-Failure "the 1's stem at $Count is half-toned: $($stem.Pale) of its $($stem.Rows.Count) rows have nothing 0x60 or darker (their darkest greens $($stem.Rows -join ' '))" }
    }
    if ($Chip.Right -gt $markRect.Left -or $inMark -gt 2) { Add-Failure "the badge at $Count reaches the remote mark (its right $($Chip.Right), the mark's left $($markRect.Left), $inMark cyan pixels in it)" }
    $m
  }
  $two = Measure-Badge 'tab-connections-badge' $chip '2'
  # Twelve: ten more suggestions of Usage chart's with sessions elsewhere. Two digits widen it to
  # the right, its left and its height as they were, still clear of the remote mark
  $many = @(1..10 | ForEach-Object { Connection "c-many-$_" @($D, ('{0:x8}-9999-4999-8999-999999999999' -f (0x90000000 + $_))) suggested -Why "one of many, $_" })
  Write-Links @($wait) (@($group, $pair, $half, $gone, $declined) + $many)
  $s = Wait-Change { param($s) $s.rail.conductor.suggested -eq 12 -and $s.rail.conductor.badge } -What 'the count twelve' -Animating
  $chip12 = ConvertTo-Rect $s.rail.conductor.badge
  "twelve: $($s.rail.conductor.suggested) suggested here, the badge $($s.rail.conductor.badge | ConvertTo-Json -Compress)"
  if ($chip12.Left -ne $chip.Left -or $chip12.Top -ne $chip.Top -or $chip12.Height -ne $chip.Height -or $chip12.Width -le $chip.Width) { Add-Failure "the badge for 12 $($s.rail.conductor.badge | ConvertTo-Json -Compress) isn't the 2's widened to the right ($($head.badge | ConvertTo-Json -Compress))" }
  $twelve = Measure-Badge 'tab-connections-badge-12' $chip12 '12'
  if ($twelve.Dark -le $two.Dark) { Add-Failure "12 has no more dark pixels than 2 ($($twelve.Dark) and $($two.Dark))" }
  # The pair and the ten gone: one, Usage chart's with Night build, and the digit drawn is a 1: its
  # dark pixels aren't the 2's
  Write-Links @($wait) @($group, $half, $gone, $declined)
  $s = Wait-Change { param($s) $s.rail.conductor.suggested -eq 1 -and $s.rail.conductor.badge -and -not (@($s.rail.connections) | Where-Object id -eq 'c-pair') } -What 'the count one with the pair gone' -Animating
  "the pair gone: $($s.rail.conductor.suggested) suggested here, the badge $($s.rail.conductor.badge | ConvertTo-Json -Compress)"
  $chip1 = ConvertTo-Rect $s.rail.conductor.badge
  if ($chip1 -ne $chip) { Add-Failure "the badge for 1 is at $($s.rail.conductor.badge | ConvertTo-Json -Compress), not where the 2's was" }
  $one = Measure-Badge 'tab-connections-badge-1' $chip1 '1'
  $moved = @($two.Pixels | Where-Object { -not $one.Pixels.Contains($_) }).Count + @($one.Pixels | Where-Object { -not $two.Pixels.Contains($_) }).Count
  "the 1 against the 2: $moved of the count's pixels differ ($($two.Ink) then $($one.Ink)), $($one.Dark) of the 1's 0x60 or darker (the 2's $($two.Dark))"
  if ($moved -lt 4) { Add-Failure "the badge's count drew the same at 1 as at 2 ($moved pixels differ)" }
  # Accepted from Usage chart's menu: the badge goes at once, before the file says so
  $base = (Read-Events).Count
  $p3 = Get-RectCenter @($s.rail.rows)[3].pad
  Send-Click $w $p3[0] $p3[1] -Right -Move
  $s = Wait-Change { param($s) $s.menu -and $s.menu.kind -eq 'tab' } -What "Usage chart's menu"
  $labels = @($s.menu.items | ForEach-Object label)
  "Usage chart's menu: $($labels -join ' | ')"
  $connect = @($s.menu.items | Where-Object label -eq 'Connect with Night build, as suggested')[0]
  if (-not $connect) { Add-Failure "Usage chart's menu hasn't 'Connect with Night build, as suggested'" }
  else {
    $mid = Get-RectCenter $connect.rect
    Send-Click $w $mid[0] $mid[1] -Move
    Test-Event (Wait-Event ($base + 1) 'Connect with Night build') @{ action = 'accept'; connection = 'c-half' } 'Connect with Night build, as suggested'
    $s = Wait-Change { param($s) -not $s.menu -and $s.rail.conductor.suggested -eq 0 -and -not $s.rail.conductor.badge } -What 'the badge gone at once' -Animating
    "accepted: $($s.rail.conductor.suggested) suggested here, badge $($s.rail.conductor.badge | ConvertTo-Json -Compress)"
  }
  $half.state = 'connected'
  Write-Links @($wait) @($group, $half, $gone, $declined)
  $s = Wait-State { param($s) @(@($s.rail.rows)[3].connected) -contains 'c-half' } -What "Usage chart connected with Night build in the file"
  $s = Wait-Change { param($s) $s.rail.conductor.suggested -eq 0 -and -not $s.rail.conductor.badge } -What 'no badge with the file saying it' -Animating
  $shot = Get-Shot $w -Name 'tab-connections-badge-none'
  $left = [LbtermE2E.Px]::CountNear($shot, $chip, $cyan, 40)
  "none suggested: $left cyan pixels where the badge was"
  if ($left -gt 2) { Add-Failure "a badge is still drawn with none suggested ($left cyan pixels)" }
  $shot.Dispose()

  # 14. A tab in a connection made and in a suggestion, its two marks in one place: Scroll strip
  # connected with Tab Wave a row down and suggested with InMail four rows down, no link. The
  # suggestion is the longer, so laid out after the connection; drawn in lane order its hollow mark
  # cut a hole through the connection's solid one (PITFALLS 53)
  $made = Connection 'c-made' @($A, $B) connected -By lbterm -Why 'both draw the rail'
  $longSug = Connection 'c-far-sug' @($A, $E) suggested -Why 'both read the rail'
  # Read on the open panel, the pointer on Scroll strip's pad: step 13's pick left the pointer
  # where its row was, off the rail, and since the rail holds under a menu's card and closes once
  # the card has gone with the pointer off both (lbterm/TAB-PANEL.md, the hover table; Lachlan's
  # 7b and 10a), the panel is shut by now, where before it stayed open until the pointer moved.
  # Shut, the open tab's ring round the pad and its label covers the marks' left side, the
  # suggestion's rim with it (2026-11-08: 36 cyan pixels round Scroll strip's mark, as round Tab
  # Wave's alone)
  $s = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  $onPad = Get-RectCenter @($s.rail.rows)[0].pad
  Send-Mouse $w Move $onPad[0] $onPad[1]
  Wait-Change { param($s) $s.rail.phase -eq 'open' } -What 'the panel open under the pointer, for the marks' -Animating | Out-Null
  Write-Links @() @($made, $longSug)
  $rowsIn = { param($s, [string]$Id) @(@($s.rail.connections | Where-Object id -eq $Id) | ForEach-Object { @($_.rows) -join ',' })[0] }
  $s = Wait-Change { param($s) (& $rowsIn $s 'c-made') -eq '0,1' -and (& $rowsIn $s 'c-far-sug') -eq '0,4' } -What 'Scroll strip connected and suggested' -Animating
  $m = @($s.rail.connections | Where-Object id -eq 'c-made')[0]; $ls = @($s.rail.connections | Where-Object id -eq 'c-far-sug')[0]
  $shot = Get-Shot $w -Name 'tab-connections-two-marks'
  $both = Measure-Mark $shot $m.mark_x @($m.ys)[0]
  $made1 = Measure-Mark $shot $m.mark_x @($m.ys)[1]
  $sug4 = Measure-Mark $shot $ls.mark_x @($ls.ys)[1]
  "two marks: Scroll strip's (lanes $($m.lane) and $($ls.lane)) $($both.Ring) cyan round it, middle $($both.Middle) of 4; Tab Wave's, connected alone, $($made1.Ring) and $($made1.Middle); InMail's, suggested alone, $($sug4.Ring) and $($sug4.Middle)"
  Save-Zoomed $shot (New-Rect ([int]$m.mark_x - 14) ([int]@($m.ys)[0] - 12) 40 ([int](@($m.ys)[1] - @($m.ys)[0]) + 24)) 'tab-connections-two-marks-zoom' -Zoom 6
  $shot.Dispose()
  if ($both.Middle -lt 3) { Add-Failure "Scroll strip, connected and suggested, reads hollow: its mark's middle $($both.Middle) of 4 cyan" }
  if ($both.Ring -lt $made1.Ring + 4) { Add-Failure "Scroll strip's mark has no suggestion's rim round it ($($both.Ring) cyan pixels round it, Tab Wave's alone $($made1.Ring))" }
  if ($made1.Middle -lt 3 -or $sug4.Middle -gt 1) { Add-Failure "the marks alone aren't solid and hollow (Tab Wave's middle $($made1.Middle), InMail's $($sug4.Middle))" }
  }
} -Passed 'tab connections: a group of three and a suggested pair in lanes beside a link, solid and hollow marks in cyan, the declined one nowhere, the light carried along the suggestion by DWM with the window painting nothing for it, still while minimized and running again restored, still at its middle with motion off, dashed without the compositor; a suggestion''s card and a connection''s with its note in red and last three messages, decline and leave from the tab menu and accept by a click, each drawn at once, a Shift-drag cyan with both tabs'' objectives connecting two tabs with the active pane''s outline left white while it drags and green again after, taking one out, and a wait link again once Shift is let go; and the Conductor''s pad counting the suggestions with a member on its rail on a cyan badge in the pads'' numbers'' font at 2, 12 and 1, clear of its remote mark, gone at once when the last is accepted'
