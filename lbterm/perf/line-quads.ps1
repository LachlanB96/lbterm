#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/line_key.rs, wezterm-gui/src/termwindow/render/mod.rs, wezterm-gui/src/termwindow/render/screen_line.rs, wezterm-gui/src/quad.rs, wezterm-gui/src/renderstate.rs, wezterm-gui/src/termwindow/render/kept_quads.rs, lbterm-motion/src/content.rs
<#
A pane's lines kept by what they show, not where (the line quad cache: lbterm/FLOWS.md rows 85 to
93, and the list under segment 4's table of what a line takes from its row). Scrolling the open
tab lays out only the lines the scroll brings, every line is drawn at its row, and what a line
takes from its row (the cursor, a selection, the link under the pointer) is drawn where it is.

Three programs, each in a window of its own like Lachlan's (New-TestConfig), on the GUI's own tab
(the line quad cache is the same for a mux pane: render/pane.rs's render_line takes both), a
line scrolled in for each key typed, each scroll one frame (DECSET 2026 round it):

- region: a Claude Code-like fullscreen view on the alternate screen, a header row and a prompt
  row, the conversation between them in a scrolling region (DECSTBM) moved up a row by SU and a
  new line written into the last row: its lines go to other stable rows, as Claude Code's do.
- shell: the main screen printing a line and a new line, scrolling the screen into scrollback:
  its lines keep their stable rows and change their places on the screen.
- lone: the region's header and prompt with one line among blank rows, moved up a row by SU each
  key. Blank rows make no quads, so the line's quads keep their place in the layer's buffer while
  they move, with the same row-0 hashes: only the offset folded into them (content::moved) tells
  the compositor and the upload it moved (PITFALLS 129 and 131). In region and shell every index
  of the buffer holds other content each scroll, so they'd pass with the fold taken out; this one
  fails then.

The lines are coloured, with a background run; every fifth is the same rule (U+2500), so several
rows show the same line and share its entry, and every third other one starts with a link. For
each program:

1. Work: the lines laid out again (the state file's work.lines) for each scroll's frame. Only the
   line it brings is new: at most 2 a scroll in all (main's key, which held the row's place, laid
   out every row of the view each scroll), and in lone none but the one line the first scroll lays
   out in every mode, which row it is not traced. The first screen's count is said too: its rules
   share one entry.
2. Every line at its row: a capture after each scroll against the one before it, row by row
   across the pane's text (its first and last columns left out, where the card's corners and the
   scrollbar are): each row that moved shows, pixel for pixel, what the row below it showed, the
   rows that didn't move show what they did, and in region and shell the row the scroll wrote in
   is new.
3. In region, then, on rows that moved: the cursor shown on one, a posted drag selecting across
   three (bound to CompleteSelection 'PrimarySelection', which with the state file on goes to its
   `copied`, never Lachlan's clipboard; a build from before feat/new-tab-menus wrote it, PITFALLS
   143), and the pointer on a link (LBTERM_POSTED_POINTER). Each is laid out again (work)
   and shows something new on its rows.
4. A second window, drawn by another build (-Reference: the installed one, copied, unless a build
   is named), shows the screen the scrolls ended on straight from its text, never scrolled, with
   the same cursor, selection (its release bound to Nop, so a build from before feat/new-tab-menus
   copies nothing to Lachlan's clipboard) and pointer: every row of the two is the same, pixel for pixel. The
   other build lays each line out at its own row, so whatever this one takes wrongly from a row,
   or leaves out, shows (PITFALLS 56: a reference drawn by the build under test would hide it).
5. No quad left stale on the GPU: LBTERM_CHECK_UPLOADS in every window, the frames' visuals.stale
   0 (PITFALLS 38 and 129).

The captures need the desktop unlocked: with the PC locked the script still counts the work, and
fails saying the captures couldn't be taken.

  pwsh -File lbterm\perf\line-quads.ps1 [-Bin <build>] [-Reference installed|<build>] [-Scrolls 20] [-Only region|shell|lone]
#>
param([string]$Bin, [string]$Reference = 'installed', [int]$Scrolls = 20, [ValidateSet('region', 'shell', 'lone')][string]$Only)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
$T = Start-Test 'line-quads' -Bin $Bin

# The build the fresh screens are drawn with: the installed one copied into the run's folder
# (its own folder is the daily LBTerm's, which the Warden and the log reader take it for), or one
# named. Never the build under test
$refBin = if ($Reference -eq 'installed') {
  $to = Join-Path $T.Out 'reference'
  Copy-Build -From (Join-Path $env:LOCALAPPDATA 'Programs\LBTerm') -To $to | Out-Null
  $to
} else { (Resolve-Path $Reference).Path }
$refGui = Join-Path $refBin 'lbterm-gui.exe'
if (-not (Test-Path $refGui)) { throw "no build in $refBin (lbterm-gui.exe) to draw the fresh screens with" }
if ((Resolve-Path $refGui).Path -eq (Resolve-Path $T.Gui).Path) { throw "-Reference is the build under test ($refBin): the fresh screens would be drawn by the same code" }
$refVersion = (& (Join-Path $refBin 'lbterm.exe') --version) -join ' '
"the fresh screens drawn by $refVersion ($refBin)"

# Where the cursor goes, the rows selected and the column the pointer rests on, 0-based, in
# region's step 3: rows of the conversation, which every scroll moved
$CursorRow = 9; $CursorCol = 12
$SelRows = @(12, 14); $SelCols = @(4, 30)
$LinkCol = 8

# @MODE@ region, shell or lone, @START@ the first line shown (in lone, 1 + how many rows above the
# region's bottom the line is). Draws when it reads `g`, scrolls a line in for each `j`, shows the
# cursor at @CROW@, @CCOL@ (1-based) for `c`; its title says what it has drawn
$Program = @'
$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$mode = '@MODE@'; $start = @START@
function Line([int]$n) {
  if ($mode -eq 'lone') { return "$e[38;5;179m  the one line among blank rows $e[48;5;236m moved a row a key $e[0m" }
  if ($n % 5 -eq 0) { return "$e[38;5;244m  " + ([string][char]0x2500 * 64) + "$e[0m" }
  if ($n % 3 -eq 0) { return "  https://example.invalid/runs/$n $e[38;5;$(18 + $n % 200)m output line $n$e[0m and its link" }
  "$e[38;5;$(18 + $n % 200)m  $n.$e[0m output line $n of the run $e[48;5;236m$e[38;5;179m tag $($n % 7) $e[0m and its words"
}
while ([Console]::ReadKey($true).KeyChar -ne 'g') {}
$rows = [Console]::WindowHeight
$sb = [Text.StringBuilder]::new()
# 1-based: the header in row 1, the conversation in rows 2 to rows - 1, the prompt in the last
$top = 2; $bottom = $rows - 1
$prompt = "$e[$rows;1H$e[2K$e[38;5;250m  > the prompt box$e[0m$e[$rows;1H"
if ($mode -eq 'shell') {
  [void]$sb.Append("$e[?25l$e[2J$e[H")
  for ($i = 0; $i -lt $rows - 1; $i++) { [void]$sb.Append((Line ($start + $i))).Append("`r`n") }
  $next = $start + $rows - 1
} else {
  [void]$sb.Append("$e[?1049h$e[?25l$e[2J$e[$top;${bottom}r")
  [void]$sb.Append("$e[1;1H$e[1;38;5;75m  a fullscreen view's header$e[0m")
  if ($mode -eq 'region') {
    for ($r = $top; $r -le $bottom; $r++) { [void]$sb.Append("$e[$r;1H").Append((Line ($start + $r - $top))) }
    $next = $start + $bottom - $top + 1
  } else {
    [void]$sb.Append("$e[$($bottom - $start + 1);1H").Append((Line 0))
  }
  [void]$sb.Append($prompt)
}
$k = 0
[void]$sb.Append("$e]2;lq $k$e\")
[Console]::Write($sb.ToString())
while ($true) {
  $key = [Console]::ReadKey($true).KeyChar
  if ($key -eq 'c') {
    [Console]::Write("$e[?2026h$e[@CROW@;@CCOL@H$e[?25h$e]2;lq c$e\$e[?2026l")
    continue
  }
  if ($key -ne 'j') { continue }
  $k++
  $frame = if ($mode -eq 'region') {
    "$e[?2026h$e[S$e[$bottom;1H$e[2K" + (Line $next) + $prompt + "$e]2;lq $k$e\$e[?2026l"
  } elseif ($mode -eq 'lone') {
    "$e[?2026h$e[S" + $prompt + "$e]2;lq $k$e\$e[?2026l"
  } else {
    "$e[?2026h" + (Line $next) + "`r`n$e]2;lq $k$e\$e[?2026l"
  }
  $next++
  [Console]::Write($frame)
}
'@

# Selecting as WezTerm's defaults do, but copying where Lachlan's clipboard never sees it
# (perf/pane-feedback.ps1), and no click opening a link
$lua = @"
config.initial_cols = 100
config.initial_rows = 30
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.disable_default_mouse_bindings = true
config.mouse_bindings = {
  { event = { Down = { streak = 1, button = 'Left' } }, mods = 'NONE', action = wezterm.action.SelectTextAtMouseCursor 'Cell' },
  { event = { Drag = { streak = 1, button = 'Left' } }, mods = 'NONE', action = wezterm.action.ExtendSelectionToMouseCursor 'Cell' },
  { event = { Up = { streak = 1, button = 'Left' } }, mods = 'NONE', action = wezterm.action.CompleteSelection 'PrimarySelection' },
}
"@
# The reference window (step 4) is another build's, which may be from before feat/new-tab-menus
# and put a copy on Lachlan's clipboard (lbterm/PITFALLS.md, 151): its release copies nothing,
# the selection left drawn as the drag made it, the same cells as a completed one once the copy's
# light has faded in the window under test
$refLua = $lua.Replace("wezterm.action.CompleteSelection 'PrimarySelection'", 'wezterm.action.Nop')
if ($refLua -eq $lua) { throw "line-quads: the reference window's release still copies: its binding wasn't found in the config" }
$windowEnv = @{ LBTERM_CHECK_UPLOADS = '1' }
$script:Stale = 0
$script:StaleFrames = 0
$onState = { param($sw, $s) if ($sw.visuals -and [int]$sw.visuals.stale -gt 0) { $script:Stale += [int]$sw.visuals.stale; $script:StaleFrames++ } }

function Get-Counts($w) {
  $s = Read-State -Window $w
  [pscustomobject]@{ Lines = [long]$s.work.lines; Frames = [long]$s.frame }
}

function Get-Pane($w) { @((Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).panes)[0] }

# A window (of -Exe's build, else the one under test) running the program in $Mode from line
# $Start, drawn: its first screen's lines laid out
function Open-Program([string]$Mode, [int]$Start, [string]$Exe) {
  $code = $Program.Replace('@MODE@', $Mode).Replace('@START@', [string]$Start).Replace('@CROW@', [string]($CursorRow + 1)).Replace('@CCOL@', [string]($CursorCol + 1))
  $open = @{ Lua = $(if ($Exe) { $refLua } else { $lua }); Environment = $windowEnv; Program = (New-PwshProgram $code); PostedPointer = $true }
  if ($Exe) { $open.Exe = $Exe }
  $w = Open-TestWindow @open
  $sw = Wait-State { param($s) @($s.panes).Count -eq 1 -and @($s.panes)[0].rows -gt 0 } -Window $w -What 'the pane'
  # A build from before feat/new-tab-menus puts a selection's copy on Lachlan's clipboard (on
  # Windows every kind of clipboard is his: lbterm/PITFALLS.md, 151); one whose state file has
  # `copied` puts it there instead. The reference window's release copies nothing ($refLua), so
  # only the window under test is asked
  if (-not $Exe -and $sw.PSObject.Properties.Name -notcontains 'copied') { throw "the build in $Bin writes no copied to its state file, so this script's selection would overwrite Lachlan's clipboard (lbterm/PITFALLS.md, 151): run it against a build from feat/new-tab-menus on" }
  Send-Focus $w
  Start-Sleep -Milliseconds 800
  Wait-Settled -Window $w | Out-Null
  $before = Get-Counts $w
  Send-Key $w 'g'
  Wait-State { param($x) @($x.panes)[0].title -eq 'lq 0' } -Window $w -AfterFrame $before.Frames -What "the first screen of $Mode" -OnState $onState | Out-Null
  Wait-Settled -Window $w -OnState $onState | Out-Null
  $after = Get-Counts $w
  [pscustomobject]@{ Window = $w; FirstLines = $after.Lines - $before.Lines }
}

# The pane's cells as its text is drawn (lbterm/STATE.md, `cells`): where the first starts and a
# cell's size, in the window's client pixels. Never from `rect`, the pane's card: a lone pane's
# card holds padding and what's left over of a cell, so `rect.h / rows` (20.5 for 31 rows of 20)
# put the last row's band past the capture's edge and every band a little further off its row.
# The fresh window, of an older build whose state file may have no `cells`, uses the grid of the
# build under test: it's held to the same card and the same cells count first
function Get-Grid($Pane) {
  if (-not $Pane.cells -or -not $Pane.cells.w) { throw "the state file gives the pane no cells (lbterm/STATE.md): the build under test is older than the field" }
  [pscustomobject]@{ X = [int]$Pane.cells.x; Y = [int]$Pane.cells.y; W = [int]($Pane.cells.w / $Pane.cols); H = [int]($Pane.cells.h / $Pane.rows); Cols = [int]$Pane.cols }
}

# The middle of the cell ($Col, $Row), 0-based, as a point in the window's client area
function Get-CellPoint($Grid, [int]$Col, [int]$Row) {
  @([int]($Grid.X + ($Col + 0.5) * $Grid.W), [int]($Grid.Y + ($Row + 0.5) * $Grid.H))
}

# Step 3 in window $w: the cursor shown, the selection dragged, the pointer on the link in row
# $LinkRow; the lines each laid out
function Invoke-RowSteps($w, $Grid, [int]$LinkRow) {
  $laid = [ordered]@{}
  $before = Get-Counts $w
  Send-Key $w 'c'
  Wait-Change { param($x) @($x.panes)[0].title -eq 'lq c' } -Window $w -AfterFrame $before.Frames -What 'the cursor shown' -OnState $onState | Out-Null
  $laid.cursor = (Get-Counts $w).Lines - $before.Lines
  $before = Get-Counts $w
  $a = Get-CellPoint $Grid $SelCols[0] $SelRows[0]; $b = Get-CellPoint $Grid $SelCols[1] $SelRows[1]
  Send-Drag $w $a[0] $a[1] $b[0] $b[1]
  Wait-Settled -Window $w -AfterFrame $before.Frames -OnState $onState -What 'the selection drawn' | Out-Null
  $laid.selection = (Get-Counts $w).Lines - $before.Lines
  $before = Get-Counts $w
  $h = Get-CellPoint $Grid $LinkCol $LinkRow
  Send-Mouse $w Move $h[0] $h[1]
  Wait-Change { param($x) $true } -Window $w -AfterFrame $before.Frames -What 'the link under the pointer drawn' -OnState $onState | Out-Null
  $laid.link = (Get-Counts $w).Lines - $before.Lines
  $laid
}

# The pane's row $Row of $Shot as a bitmap of its own, its first and last columns left out
function Get-Band($Shot, $Grid, [int]$Row) {
  $rect = [Drawing.Rectangle]::new($Grid.X + $Grid.W, $Grid.Y + $Row * $Grid.H, ($Grid.Cols - 2) * $Grid.W, $Grid.H)
  $Shot.Clone($rect, $Shot.PixelFormat)
}

# How many pixels differ between row $RowA of $A and row $RowB of $B, both on $Grid
function Compare-Rows($Grid, $A, [int]$RowA, $B, [int]$RowB) {
  $a = Get-Band $A $Grid $RowA; $b = Get-Band $B $Grid $RowB
  try { [LbtermE2E.Px]::Differing($a, $b, [LbtermE2E.Px]::All($a)) } finally { $a.Dispose(); $b.Dispose() }
}

Invoke-Test {
  $locked = Test-WorkstationLocked
  if ($locked) { $T.Notes.Add('the PC is locked: the work is counted, the captures not compared') }
  foreach ($mode in @('region', 'shell', 'lone') | Where-Object { -not $Only -or $_ -eq $Only }) {
    $script:Stale = 0; $script:StaleFrames = 0
    $start = 1
    $o = Open-Program $mode $start
    $w = $o.Window
    $p = Get-Pane $w
    $grid = Get-Grid $p
    $rows = [int]$p.rows
    # The rows that move a scroll, 0-based: the region's, or every row but the cursor's blank one
    $moving = if ($mode -eq 'shell') { 0..($rows - 2) } else { 1..($rows - 2) }
    $still = if ($mode -eq 'shell') { @($rows - 1) } else { @(0, ($rows - 1)) }
    $first = $moving[0]; $last = $moving[-1]
    "${mode}: $rows rows, the first screen laid out $($o.FirstLines) lines"

    $shot = if (-not $locked) { Get-Shot $w }
    $perScroll = [Collections.Generic.List[long]]::new()
    $misplaced = [Collections.Generic.List[string]]::new()
    for ($n = 1; $n -le $Scrolls; $n++) {
      $before = Get-Counts $w
      Send-Key $w 'j'
      Wait-State { param($x) @($x.panes)[0].title -eq "lq $n" } -Window $w -AfterFrame $before.Frames -What "the frame of scroll $n" -OnState $onState | Out-Null
      Wait-Settled -Window $w -OnState $onState | Out-Null
      $perScroll.Add((Get-Counts $w).Lines - $before.Lines)
      if ($locked) { continue }
      $now = Get-Shot $w
      if ($n -eq 1) { Save-Shot $now "$mode-scrolled-1" }
      # Each row that moved shows what the row below it showed; the rest what they showed
      for ($r = $first; $r -lt $last; $r++) {
        $d = Compare-Rows $grid $now $r $shot ($r + 1)
        if ($d -gt 0) { $misplaced.Add("scroll ${n}: row $r differs from row $($r + 1) before it in $d pixels") }
      }
      foreach ($r in $still) {
        $d = Compare-Rows $grid $now $r $shot $r
        if ($d -gt 0) { $misplaced.Add("scroll ${n}: row $r, which doesn't move, changed in $d pixels") }
      }
      if ($mode -ne 'lone' -and (Compare-Rows $grid $now $last $shot $last) -eq 0) { $misplaced.Add("scroll ${n}: the row it writes ($last) shows what it did before") }
      $shot.Dispose(); $shot = $now
    }
    $laid = ($perScroll | Measure-Object -Sum).Sum
    "${mode}: $Scrolls scrolls laid out $laid lines ($($perScroll -join ' ')), where the view has $($moving.Count) rows that move"
    if ($mode -eq 'lone') {
      # The first scroll lays out one line more than it brings, here and in region alike (1, then
      # 0 a scroll, on 2026-11-04): which row isn't traced (lbterm/FLOWS.md, "Not traced yet")
      if ($laid -gt 1) { Add-Failure "lone: $Scrolls scrolls of one line among blank rows laid out $laid lines ($($perScroll -join ' ')), where a line that only moved is laid out never and the first scroll lays out one (main's key laid out 2 a scroll)" }
    } else {
      if ($laid -gt 2 * $Scrolls) { Add-Failure "${mode}: $Scrolls scrolls laid out $laid lines ($($perScroll -join ' ')): a line that only moved was laid out again, where each scroll brings one" }
      # Every fifth line is the rule, whose entry the first screen made
      if ($laid -lt $Scrolls - [Math]::Ceiling($Scrolls / 5.0) - 1) { Add-Failure "${mode}: $Scrolls scrolls laid out $laid lines: the lines they brought weren't all laid out (a new line drawn from another line's entry?)" }
    }
    $count = $misplaced.Count
    if ($count) { Add-Failure "${mode}: $count rows not where their lines are: $(@($misplaced | Select-Object -First 6) -join '; ')" }
    elseif (-not $locked) { "${mode}: after each of $Scrolls scrolls every row that moved shows what the row below it showed, pixel for pixel" }

    # Region: what a line takes from its row, on rows that moved. The link row: the first row
    # from the cursor's on whose line starts with a link (every third line, rules aside)
    $linkRow = -1
    if ($mode -eq 'region') {
      $linkRow = @(($CursorRow + 1)..$last | Where-Object { $_ -notin $SelRows[0]..$SelRows[1] } | Where-Object { $m = $start + $Scrolls + $_ - 1; $m % 3 -eq 0 -and $m % 5 -ne 0 })[0]
      if ($null -eq $linkRow) { throw "region: no row from $($CursorRow + 1) to $last outside the selection shows a link after $Scrolls scrolls" }
      $laidSteps = Invoke-RowSteps $w $grid $linkRow
      "region: laid out for the cursor $($laidSteps.cursor), the selection $($laidSteps.selection), the link under the pointer $($laidSteps.link) lines"
      if ($laidSteps.cursor -lt 1) { Add-Failure "region: the cursor shown on a moved row laid out no line: its row was drawn from an entry without it" }
      if ($laidSteps.selection -lt $SelRows[1] - $SelRows[0] + 1) { Add-Failure "region: a selection across $($SelRows[1] - $SelRows[0] + 1) moved rows laid out $($laidSteps.selection) lines: a selected row was drawn from an entry without it" }
      if ($laidSteps.link -lt 1) { Add-Failure "region: the pointer on a link on a moved row laid out no line: its row was drawn from an entry without the link under the pointer" }
      if (-not $locked) {
        $now = Get-Shot $w
        Save-Shot $now 'region-steps'
        $news = @([pscustomobject]@{ What = 'the cursor'; Row = $CursorRow }) +
          @($SelRows[0]..$SelRows[1] | ForEach-Object { [pscustomobject]@{ What = 'the selection'; Row = $_ } }) +
          @([pscustomobject]@{ What = 'the link under the pointer'; Row = $linkRow })
        foreach ($nw in $news) {
          if ((Compare-Rows $grid $now $nw.Row $shot $nw.Row) -eq 0) { Add-Failure "region: $($nw.What) shows nothing new on row $($nw.Row)" }
        }
        $shot.Dispose(); $shot = $now
      }
    }

    # The same screen drawn by the other build straight from its text in a window that never
    # scrolled, with the same cursor, selection and pointer
    if (-not $locked) {
      $final = $shot
      Save-Shot $final "$mode-scrolled"
      Stop-TestWindow $w
      $fresh = Open-Program $mode ($start + $Scrolls) -Exe $refGui
      $fp = Get-Pane $fresh.Window
      # The same card at the same place and the same cells count: with the same config, the same grid
      $same = $fp.rows -eq $p.rows -and $fp.cols -eq $p.cols -and $fp.rect.x -eq $p.rect.x -and $fp.rect.y -eq $p.rect.y -and $fp.rect.w -eq $p.rect.w -and $fp.rect.h -eq $p.rect.h
      if (-not $same) {
        Add-Failure "${mode}: the window drawing the text fresh has a pane of $($fp.cols) by $($fp.rows) (card $($fp.rect.w) by $($fp.rect.h) at $($fp.rect.x),$($fp.rect.y)), not $($p.cols) by $($p.rows) (card $($p.rect.w) by $($p.rect.h) at $($p.rect.x),$($p.rect.y)): not compared"
      } else {
        if ($mode -eq 'region') { Invoke-RowSteps $fresh.Window $grid $linkRow | Out-Null }
        $ref = Get-Shot $fresh.Window -Name "$mode-fresh"
        $differ = @(0..($rows - 1) | ForEach-Object { $d = Compare-Rows $grid $final $_ $ref $_; if ($d -gt 0) { "row $_ in $d pixels" } })
        if ($differ) { Add-Failure "${mode}: the scrolled screen and the same text drawn fresh by $refVersion differ: $($differ.Count) rows ($(@($differ | Select-Object -First 6) -join ', '))" }
        else { "${mode}: the scrolled screen is the same text drawn fresh by $refVersion, every row pixel for pixel" }
        $ref.Dispose()
      }
      $final.Dispose()
      Stop-TestWindow $fresh.Window
    } else {
      Stop-TestWindow $w
    }
    if ($script:Stale -gt 0) { Add-Failure "${mode}: $($script:StaleFrames) frames left $($script:Stale) quads stale on the GPU: hashes the same as what it held, vertices not (PITFALLS 129)" }
  }
  if ($locked) { Add-Failure 'the PC is locked: the captures that hold every line at its row could not be taken (the work was counted; run it again unlocked)' }
} -Passed 'line-quads: a scroll of a fullscreen region, of a shell or of one line among blank rows lays out only the line it brings, every line shows at its row after each scroll, the cursor, a selection and the link under the pointer on moved rows are laid out again, the screens are the same text drawn fresh by another build, and no quad is left stale on the GPU'
