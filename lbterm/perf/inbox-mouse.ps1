#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/inbox.rs, lbterm-extras/src/projects.rs, lbterm-extras/src/kept.rs, wezterm-gui/src/termwindow/render/words.rs
<#
What the pointer costs while the InMail button's list is open (lbterm/INBOX.md, "What a change
costs"; lbterm/PERF.md, "What the cards cost"), in a test window with projects the size of
Lachlan's: 47 of them (three repos nested in others' folders, as Trainer is in Musica's) and 70
messages waiting in the InMail folders of 38. While the list is open it takes every mouse event,
and each used to lay the whole list out again, asking which project every message's folder is in
by making every project's folder key again for each (70 messages by 47 projects, each key built
anew), for a move that changes nothing on the screen.

1. The list is opened from a click on the button, and the pointer rests on its third row.
2. -Moves moves are posted along that row, left and right within it, so what's under the pointer
   stays the same row and nothing is painted. Over them: the GUI thread's CPU (its cycles, exact),
   a move's share of it, and the frames painted. The same moves over the window with the list
   shut, beside it, are what any move costs, for the record.
3. The pointer goes down the rows one at a time, and the state file's menu focus has to follow it
   to each, as before.
4. It fails when the moves within a row paint a frame, when the focus doesn't follow, or when a
   move within the row costs the GUI thread more than -BudgetUs above a move with the list shut,
   at the median of -Rounds rounds: 100 us, a tenth of a millisecond. Measured on 2026-10-31,
   two runs of each build interleaved: before the list kept its layout, 8,562 to 9,194 us a move
   (3.4 to 3.7 s for the 400), against 37 to 40 with it shut; after, 34 to 49 us open and 38 to
   44 shut, a median difference of -2.6 and -1.8 us (lbterm/PERF.md, "What the cards cost").

moves.json in target\perf\inbox-mouse has every number.

  pwsh -File lbterm\perf\inbox-mouse.ps1 [-Bin <build>] [-Moves 400] [-Rounds 3] [-BudgetUs 100]
#>
param([string]$Bin, [int]$Moves = 400, [int]$Rounds = 3, [double]$BudgetUs = 100)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'inbox-mouse' -Bin $Bin
Initialize-CpuReader
$rate = Get-CycleRate

# Projects the size of Lachlan's, as the budget runner's busy scenarios make them (perf/budgets.ps1,
# New-ProjectsFixture): the last three are repos in the first three's folders
$projects = Join-Path $T.Out 'projects'
Remove-Item -Recurse -Force $projects -ErrorAction SilentlyContinue
$count = 47; $mail = 70; $withMail = 38
$names = @(1..$count | ForEach-Object { 'Project{0:d2}' -f $_ })
$dirs = @($names | ForEach-Object { Join-Path $projects $_ })
for ($r = 0; $r -lt 3; $r++) { $dirs[$count - 1 - $r] = Join-Path $dirs[$r] "code\$($names[$count - 1 - $r])" }
foreach ($d in $dirs) { New-Item -ItemType Directory -Force (Join-Path $d 'InMail') | Out-Null }
$start = (Get-Date).AddDays(-9)
for ($m = 0; $m -lt $mail; $m++) {
  $p = $m % $withMail
  New-InMailMessage -Folder (Join-Path $dirs[$p] 'InMail') -To $names[$p] -From 'Home' -Subject "Message $m about part $m of the work in $($names[$p])" -At $start.AddMinutes(173 * $m) | Out-Null
}
$listed = for ($i = 0; $i -lt $count; $i++) {
  $parent = if ($i -ge $count - 3) { " parent = '$($names[$count - 1 - $i])'," } else { '' }
  "{ name = '$($names[$i])', dir = [[$($dirs[$i])]],$parent }"
}
$folders = @($dirs | ForEach-Object { "[[$(Join-Path $_ 'InMail')]]" }) -join ', '
$cfg = Join-Path $T.Out 'config\inbox-mouse.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 30
rawset(config, 'lbterm_projects', { projects = { $($listed -join ', ') } })
rawset(config, 'lbterm_inbox', { folders = { $folders } })
"@

$results = [Collections.Generic.List[object]]::new()
Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram) -PostedPointer -TimeoutSec 30
  $s = Wait-State { param($s) $s.frame -gt 0 -and $s.strip -and [int]$s.strip.inbox -eq $mail } -What "the button saying $mail" -TimeoutSec 30
  $tid = Get-GuiThreadId $w
  $ms = { param([long]$cycles) $cycles / $rate }
  $frameNow = { [long](Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).frame }
  # The GUI thread's cycles once it has gone quiet: no change over 300 ms
  $quietCycles = {
    $last = [LbtermE2E.Cpu]::ThreadCycles($tid)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt 10) {
      Start-Sleep -Milliseconds 300
      $now = [LbtermE2E.Cpu]::ThreadCycles($tid)
      # Less than 0.05 ms in 300 ms: what the thread does at rest
      if ($now - $last -lt 0.05 * $rate) { return $now }
      $last = $now
    }
    throw 'the GUI thread never went quiet after the moves'
  }
  # Moves posted along y from x0 to x1 and back, -Moves of them, a few at a time so none waits long
  $sweep = { param([int]$x0, [int]$x1, [int]$y)
    for ($i = 0; $i -lt $Moves; $i++) {
      $t = ($i % 40) / 40.0
      $x = if (($i / 40) % 2 -lt 1) { $x0 + ($x1 - $x0) * $t } else { $x1 - ($x1 - $x0) * $t }
      Send-Mouse $w Move ([int]$x) $y
      if ($i % 20 -eq 19) { Start-Sleep -Milliseconds 2 }
    }
  }
  $measure = { param([string]$what, [int]$x0, [int]$x1, [int]$y)
    Get-QuietFrame -QuietMs 500 -Window $w | Out-Null
    $c0 = & $quietCycles; $f0 = & $frameNow
    & $sweep $x0 $x1 $y
    $c1 = & $quietCycles; $f1 = & $frameNow
    [pscustomobject]@{ What = $what; Moves = $Moves; GuiMs = [Math]::Round((& $ms ($c1 - $c0)), 3); UsAMove = [Math]::Round((& $ms ($c1 - $c0)) * 1000 / $Moves, 1); Frames = $f1 - $f0 }
  }

  # The button, and a row of the pane under where the list opens, for the moves with it shut
  $button = @($s.strip.items | Where-Object name -eq 'inbox')[0].rect
  $bx, $by = Get-RectCenter $button

  for ($round = 1; $round -le $Rounds; $round++) {
    # Shut: the same moves over the pane, where the list's third row will be
    if ($round -gt 1) {
      Send-Key $w Escape
      Wait-State { param($s) $s.menu.kind -ne 'inbox' } -What 'the list shut' | Out-Null
    }
    Send-Click $w $bx $by -Move
    $s = Wait-State { param($s) $s.menu.kind -eq 'inbox' -and @($s.menu.items).Count -eq $mail -and $s.menu.rect } -What "the InMail list of $mail"
    $row = @($s.menu.items)[2].rect
    if (-not $row) { throw "the list's third row has no rect: $(@($s.menu.items)[2] | ConvertTo-Json -Compress)" }
    # The row's part clear of the tab rail: with the list shut, moves over the rail open the panel,
    # whose open area then takes the next click on the button to close it (lbterm/TAB-PANEL.md).
    # Until 2026-11-07 they didn't, only because the click that opened the list left the window
    # holding the pointer for a drag, which shut the panel to it (lbterm/PITFALLS.md, 135)
    $rail = $s.rail.rect
    $clear = if ($rail) { [int]($rail.x + $rail.w + 8) } else { 0 }
    $y = [int]($row.y + $row.h / 2); $x0 = [Math]::Max([int]($row.x + 12), $clear); $x1 = [int]($row.x + $row.w - 12)
    Send-Mouse $w Move $x0 $y
    Wait-State { param($s) $s.menu.focus -eq 2 } -What "the third row under the pointer" | Out-Null
    $open = & $measure 'within a row of the open list' $x0 $x1 $y
    $after = Read-State
    $focus = (Get-StateWindow $after -Hwnd ([long]$w.Handle)).menu.focus
    if ($focus -ne 2) { Add-Failure "round ${round}: after the moves within the third row the focus says $focus" }
    Send-Key $w Escape
    Wait-State { param($s) $s.menu.kind -ne 'inbox' } -What 'the list shut' | Out-Null
    $shut = & $measure 'the same place with the list shut' $x0 $x1 $y
    $results.Add([pscustomobject]@{ Round = $round; Open = $open; Shut = $shut })
    "round ${round}: within a row of the open list $($open.UsAMove) us a move ($($open.GuiMs) ms for $Moves, $($open.Frames) frames); with it shut $($shut.UsAMove) us ($($shut.GuiMs) ms, $($shut.Frames) frames)"
    if ($open.Frames -gt 0) { Add-Failure "round ${round}: $($open.Frames) frames painted for moves within one row, which change nothing on the screen" }
  }

  # Down the rows one at a time: the focus follows
  Send-Click $w $bx $by -Move
  $s = Wait-State { param($s) $s.menu.kind -eq 'inbox' -and @($s.menu.items).Count -eq $mail } -What "the InMail list again"
  $items = @($s.menu.items)
  $shown = @(for ($i = 0; $i -lt $items.Count; $i++) { if ($items[$i].rect) { $i } })
  foreach ($i in $shown) {
    $cx, $cy = Get-RectCenter $items[$i].rect
    Send-Mouse $w Move $cx $cy
    $ok = Wait-Until { (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).menu.focus -eq $i } -TimeoutSec 3
    if (-not $ok) { Add-Failure "the pointer on row $i ($($items[$i].label)): the focus says $((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).menu.focus)"; break }
  }
  "the focus followed the pointer down $($shown.Count) rows"

  $extra = @($results | ForEach-Object { [Math]::Round($_.Open.UsAMove - $_.Shut.UsAMove, 1) } | Sort-Object)
  $median = $extra[[int][Math]::Floor(($extra.Count - 1) / 2)]
  "a move within a row of the open list costs $median us more than with it shut at the median ($($extra -join ', ') us; budget $BudgetUs)"
  if ($median -gt $BudgetUs) { Add-Failure "a move within a row of the open list costs the GUI thread $median us more than with it shut, over $BudgetUs us: is the list laid out again for every mouse event?" }
  [pscustomobject]@{ Moves = $Moves; Rounds = $results; MedianExtraUs = $median; BudgetUs = $BudgetUs; Rows = $shown.Count } |
    ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'moves.json')
} -Passed 'inbox-mouse: moving the pointer within a row of the open InMail list costs next to nothing and paints nothing, and the focus follows it down the rows'
