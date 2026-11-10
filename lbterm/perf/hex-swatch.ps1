#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/hex.rs, wezterm-gui/src/termwindow/render/screen_line.rs, lbterm-extras/src/hex.rs
<#
The #RRGGBB bars (lbterm/HEX.md): a code in a pane's text gets a bar in its own colour along the
foot of its seven cells, every character staying as it is, judged by the pixels of a test window
that looks like Lachlan's LBTerm (New-TestConfig, the pane's background #121212).

A program prints, on rows of its own (0-based):

  1  #FFBF00, (#8a8a9e) color: #F2E86D; #40c0ff           four codes: a row's start, brackets,
                                                            a CSS value, lowercase
  3  abc#FF00FF #00FF00AA #1234567 &#FF0080; #fff ##FF8000 #C000FF_x 9#00FFC0
                                                            none of them a code
  5  #121212 near the background, #402020 not              the first too near the background
                                                            (outlined), the second dark but not
  7  日本 #00C0FF 漢字#FF6060 색#FF2080                     after two wide characters (cells
                                                            5 to 11); glued to a Chinese word,
                                                            a bar (no spaces in Chinese); glued
                                                            to a Korean one, none
  9  #FF4080 (underlined) #40FF80 (an OSC 8 link) a link    the program's underline wins; a link
 11  select #A060FF here                                    selected below
13-14  a code a wrap splits (#E0 | FF40)                    none on either part
16-17  a code ending at a wrapped row's last cell, AA on the next row: #60E0FFAA, none
 18  #20FFA0 at the last cell of a row that doesn't wrap    a bar
 20  #FF2020 (SGR 8) #20FF20 (in its background's colour)    hidden by the program: none
     #C0A0FF (black on a background of #C0A0FF)             the program's own swatch: none

1. At rest: each code's bar, in its colour (each channel within 4), fills the foot of its seven
   cells, max(1, round(2 * dpi / 96)) px tall, and nothing of its colour is anywhere else on its
   row; no colour of a non-code is on its row (or rows) at all. The near code's bar is outlined:
   the outline's bottom and top pixel rows across the code and its two side columns are the
   text's colour (#ffffff), the bar inside them the code's; #402020's foot has none of the text's
   colour. The program's own swatch has none of its text's colour (black) along its foot.
2. The pointer on the link over #40FF80 (LBTERM_POSTED_POINTER): the link is still found (its row
   shows something new, the hover's underline) and the bar shows as at rest (HEX.md, question 2).
3. A posted drag across row 11 (bound to CompleteSelection, which with the state file on copies to
   its `copied`, never Lachlan's clipboard): the bar shows in the selection as at rest, and the copy
   is the text the program printed, `select #A060FF here`.
4. The program scrolls its rows up one (a newline at the bottom row): every code's bar and the near
   code's outline are whole at its new row, from the quads kept for what the row shows, copied to
   its new place (PITFALLS 128), and the scroll's frames laid out 3 lines at most (the state file's
   work.lines: the row it brings, the cursor's, and the selection's if the scroll cleared it), not
   the 19 rows that moved.
5. The program shows its cursor (a steady block, as Lachlan's) on the third cell of #FFBF00: the
   other six cells keep their bar, and the cursor covers the bar on its own cell (HEX.md, the
   cursor; cursor.png for Lachlan).
6. A second window with `lbterm_hex_colours = false`, the same program: no code's colour on its row,
   and the rows with no code the same as the first window's, pixel for pixel; on the rows with codes,
   the two differ only in the codes' feet (row 20's hidden codes and swatch the same with it on).

Captures for Lachlan in the run's folder (target/perf/hex-swatch): codes.png (the pane's rows at
1x), codes-x3.png, near-x6.png (the outlined code), hidden-x3.png (row 20: the hidden codes and the
program's swatch), hover.png, selected.png, cursor-x3.png and off.png (1x).

The captures need the desktop unlocked: with the PC locked the script fails saying so.

  pwsh -File lbterm\perf\hex-swatch.ps1 [-Bin <build>]
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'hex-swatch' -Bin $Bin

# The program's ASCII rows, where a code's first cell is its place in the text
$Row1 = '#FFBF00, (#8a8a9e) color: #F2E86D; #40c0ff'
$Row3 = 'abc#FF00FF #00FF00AA #1234567 &#FF0080; #fff ##FF8000 #C000FF_x 9#00FFC0'
$Row5 = '#121212 near the background, #402020 not'
$Row11 = 'select #A060FF here'

# Draws when it reads `g` (the window's size settled by then, so the wrapped rows wrap where it
# says), the cursor hidden and put out of the way; its title says it's drawn
$Program = @'
$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
while ([Console]::ReadKey($true).KeyChar -ne 'g') {}
$cols = [Console]::WindowWidth
$sb = [Text.StringBuilder]::new()
[void]$sb.Append("$e[?25l$e[2J")
function At([int]$Row, [string]$Text) { [void]$sb.Append("$e[$($Row + 1);1H").Append($Text) }
At 1 '@ROW1@'
At 3 '@ROW3@'
At 5 '@ROW5@'
At 7 "$([char]0x65E5)$([char]0x672C) #00C0FF $([char]0x6F22)$([char]0x5B57)#FF6060 $([char]0xC0C9)#FF2080"
At 9 "$e[4m#FF4080$e[24m $e]8;;https://example.invalid/swatch$e\#40FF80$e]8;;$e\ a link"
At 11 '@ROW11@'
At 13 ((' ' * ($cols - 3)) + '#E0FF40 split')
At 16 (('.' * ($cols - 8)) + ' #60E0FFAA more')
[void]$sb.Append("$e[19;$($cols - 6)H#20FFA0")
At 20 "$e[8m#FF2020$e[28m $e[38;2;64;64;64;48;2;64;64;64m#20FF20$e[0m $e[38;2;0;0;0;48;2;192;160;255m#C0A0FF$e[0m end"
[void]$sb.Append("$e[23;1H$e]2;hex drawn$e\")
[Console]::Write($sb.ToString())
while ($true) {
  switch ([Console]::ReadKey($true).KeyChar) {
    # A newline at the bottom row: every row up one, the cursor hidden still
    's' { [Console]::Write("$e[$([Console]::WindowHeight);1H`n$e]2;hex scrolled$e\") }
    # The cursor shown on #FFBF00's third cell (row 1, a row up after the scroll)
    'c' { [Console]::Write("$e[1;3H$e[?25h$e]2;hex cursor$e\") }
  }
}
'@
$Program = $Program.Replace('@ROW1@', $Row1).Replace('@ROW3@', $Row3).Replace('@ROW5@', $Row5).Replace('@ROW11@', $Row11)

# Selecting as WezTerm's defaults do, copying to the state file's `copied` (perf/line-quads.ps1)
$Lua = @'
config.initial_cols = 100
config.initial_rows = 24
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.disable_default_mouse_bindings = true
config.mouse_bindings = {
  { event = { Down = { streak = 1, button = 'Left' } }, mods = 'NONE', action = wezterm.action.SelectTextAtMouseCursor 'Cell' },
  { event = { Drag = { streak = 1, button = 'Left' } }, mods = 'NONE', action = wezterm.action.ExtendSelectionToMouseCursor 'Cell' },
  { event = { Up = { streak = 1, button = 'Left' } }, mods = 'NONE', action = wezterm.action.CompleteSelection 'PrimarySelection' },
}
'@

function Get-Pane($w) { @((Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).panes)[0] }

# The pane's cells as its text is drawn (lbterm/STATE.md, `cells`), in client pixels
function Get-Grid($Pane) {
  if (-not $Pane.cells -or -not $Pane.cells.w) { throw "the state file gives the pane no cells (lbterm/STATE.md): the build under test is older than the field" }
  [pscustomobject]@{ X = [int]$Pane.cells.x; Y = [int]$Pane.cells.y; W = [int]($Pane.cells.w / $Pane.cols); H = [int]($Pane.cells.h / $Pane.rows); Cols = [int]$Pane.cols; Rows = [int]$Pane.rows }
}

# Cells $Col0..$Col1 of row $Row, the bottom $Foot pixel rows of them with -Foot
function Get-CellRect($Grid, [int]$Row, [int]$Col0, [int]$Col1, [int]$Foot = 0) {
  $x = $Grid.X + $Col0 * $Grid.W; $w = ($Col1 - $Col0 + 1) * $Grid.W
  $y = $Grid.Y + $Row * $Grid.H
  if ($Foot) { New-Rect $x ($y + $Grid.H - $Foot) $w $Foot } else { New-Rect $x $y $w $Grid.H }
}
function Get-RowRect($Grid, [int]$Row) { Get-CellRect $Grid $Row 0 ($Grid.Cols - 1) }

# The middle of cell ($Col, $Row) in client pixels
function Get-CellPoint($Grid, [int]$Col, [int]$Row) { @([int]($Grid.X + ($Col + 0.5) * $Grid.W), [int]($Grid.Y + ($Row + 0.5) * $Grid.H)) }

# A colour's range, each channel within $Tol
function Get-Range([string]$Hex, [int]$Tol = 4) {
  $c = @(0, 2, 4 | ForEach-Object { [Convert]::ToInt32($Hex.Substring($_, 2), 16) })
  New-ColourRange ([Math]::Max(0, $c[0] - $Tol)) ([Math]::Min(255, $c[0] + $Tol)) ([Math]::Max(0, $c[1] - $Tol)) ([Math]::Min(255, $c[1] + $Tol)) ([Math]::Max(0, $c[2] - $Tol)) ([Math]::Min(255, $c[2] + $Tol))
}
function Get-Count($Shot, $Rect, [string]$Hex) { [LbtermE2E.Px]::CountRange($Shot, $Rect, (Get-Range $Hex)) }

function Save-Crop($Shot, $Rect, [string]$Name) {
  $c = $Shot.Clone($Rect, $Shot.PixelFormat)
  try { Save-Shot $c $Name } finally { $c.Dispose() }
}

# A window running the program, drawn and settled: the window, its pane's grid and its DPI
function Open-Program([string[]]$Set = @()) {
  $w = Open-TestWindow -Lua $Lua -Set $Set -Program (New-PwshProgram $Program) -PostedPointer
  Wait-State { param($s) @($s.panes).Count -eq 1 -and @($s.panes)[0].rows -gt 0 } -Window $w -What 'the pane' | Out-Null
  Send-Focus $w
  Start-Sleep -Milliseconds 800
  Wait-Settled -Window $w | Out-Null
  $f = Get-StateFrame $w
  Send-Key $w 'g'
  $sw = Wait-State { param($s) @($s.panes)[0].title -eq 'hex drawn' } -Window $w -AfterFrame $f -What 'the program''s rows drawn'
  Wait-Settled -Window $w | Out-Null
  $sw = Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)
  if ($sw.PSObject.Properties.Name -notcontains 'copied') { throw "the build in $Bin writes no copied to its state file, so the selection's copy would go to Lachlan's clipboard (lbterm/PITFALLS.md, 151)" }
  $pane = @($sw.panes)[0]
  [pscustomobject]@{ Window = $w; Pane = $pane; Grid = (Get-Grid $pane); Dpi = [int]$sw.size.dpi }
}

Invoke-Test {
  if (Test-WorkstationLocked) { throw 'the PC is locked: the bars are judged by their pixels, which a locked desktop doesn''t draw (run it again unlocked)' }
  $on = Open-Program
  $g = $on.Grid; $w = $on.Window
  $cols = $g.Cols
  $scale = $on.Dpi / 96.0
  $thick = [int][Math]::Min($g.H, [Math]::Max(1, [Math]::Round(2 * $scale, [MidpointRounding]::AwayFromZero)))
  $edge = [int][Math]::Max(1, [Math]::Round($scale, [MidpointRounding]::AwayFromZero))
  $foot = [int][Math]::Min($g.H, $thick + 2 * $edge)
  "the pane: $cols by $($g.Rows) cells of $($g.W) by $($g.H) px at $($g.X),$($g.Y), $($on.Dpi) DPI: a bar $thick px tall, $foot outlined"
  if ($cols -lt 80) { throw "the pane is $cols cells wide, too narrow for the rows (80 at least)" }
  if ($g.Rows -lt 24) { throw "the pane is $($g.Rows) rows tall, too short for the rows (24 at least)" }

  # Every code, by its row and first cell; every non-code by the colour it would be and its rows
  $codes = @(
    [pscustomobject]@{ Row = 1; Col = $Row1.IndexOf('#FFBF00'); Hex = 'FFBF00'; What = '#FFBF00, at a row''s start' }
    [pscustomobject]@{ Row = 1; Col = $Row1.IndexOf('#8a8a9e'); Hex = '8a8a9e'; What = '(#8a8a9e)' }
    [pscustomobject]@{ Row = 1; Col = $Row1.IndexOf('#F2E86D'); Hex = 'F2E86D'; What = 'color: #F2E86D;' }
    [pscustomobject]@{ Row = 1; Col = $Row1.IndexOf('#40c0ff'); Hex = '40c0ff'; What = '#40c0ff, lowercase, at the row''s end' }
    [pscustomobject]@{ Row = 5; Col = $Row5.IndexOf('#402020'); Hex = '402020'; What = '#402020, dark but not too near' }
    [pscustomobject]@{ Row = 7; Col = 5; Hex = '00C0FF'; What = '#00C0FF after two wide characters' }
    [pscustomobject]@{ Row = 7; Col = 17; Hex = 'FF6060'; What = '#FF6060 glued to a Chinese word (Lachlan''s 7b)' }
    [pscustomobject]@{ Row = 9; Col = 8; Hex = '40FF80'; What = '#40FF80 under an OSC 8 link' }
    [pscustomobject]@{ Row = 11; Col = $Row11.IndexOf('#A060FF'); Hex = 'A060FF'; What = '#A060FF' }
    [pscustomobject]@{ Row = 18; Col = $cols - 7; Hex = '20FFA0'; What = '#20FFA0 at the last cell of a row that doesn''t wrap' }
  )
  $near = [pscustomobject]@{ Row = 5; Col = 0; Hex = '121212'; What = '#121212 on the #121212 background' }
  $nonCodes = @(
    [pscustomobject]@{ Rows = @(3); Hex = 'FF00FF'; What = 'abc#FF00FF' }
    [pscustomobject]@{ Rows = @(3); Hex = '00FF00'; What = '#00FF00AA' }
    [pscustomobject]@{ Rows = @(3); Hex = '123456'; What = '#1234567' }
    [pscustomobject]@{ Rows = @(3); Hex = 'FF0080'; What = '&#FF0080;' }
    [pscustomobject]@{ Rows = @(3); Hex = 'FF8000'; What = '##FF8000' }
    [pscustomobject]@{ Rows = @(3); Hex = 'C000FF'; What = '#C000FF_x' }
    [pscustomobject]@{ Rows = @(3); Hex = '00FFC0'; What = '9#00FFC0' }
    [pscustomobject]@{ Rows = @(7); Hex = 'FF2080'; What = '#FF2080 glued to a Korean word' }
    [pscustomobject]@{ Rows = @(9); Hex = 'FF4080'; What = '#FF4080, which the program underlined' }
    [pscustomobject]@{ Rows = @(13, 14); Hex = 'E0FF40'; What = '#E0FF40, split by a wrap' }
    [pscustomobject]@{ Rows = @(16, 17); Hex = '60E0FF'; What = '#60E0FF ending a wrapped row, AA on the next' }
    [pscustomobject]@{ Rows = @(20); Hex = 'FF2020'; What = '#FF2020, which the program hid (SGR 8)' }
    [pscustomobject]@{ Rows = @(20); Hex = '20FF20'; What = '#20FF20, drawn in its background''s colour' }
  )
  # The program's own swatch: black on #C0A0FF, which a bar would only outline
  $swatch = [pscustomobject]@{ Row = 20; Col = 16; Hex = 'C0A0FF'; What = '#C0A0FF on the program''s own background of it' }

  # A code's bar: its colour fills the code's seven cells' foot, $thick px, and nowhere else on its row
  function Test-Bar($Shot, $Code, [string]$When) {
    $cells = Get-CellRect $g $Code.Row $Code.Col ($Code.Col + 6)
    $want = 7 * $g.W * $thick
    $inCells = Get-Count $Shot $cells $Code.Hex
    $inFoot = Get-Count $Shot (Get-CellRect $g $Code.Row $Code.Col ($Code.Col + 6) -Foot $thick) $Code.Hex
    $inRow = Get-Count $Shot (Get-RowRect $g $Code.Row) $Code.Hex
    $line = "$When, $($Code.What): $inFoot of $want foot pixels in #$($Code.Hex)"
    if ($inFoot -lt [Math]::Floor(0.9 * $want)) { Add-Failure "$line, where its bar fills them" }
    elseif ($inCells -ne $inFoot) { Add-Failure "$line, and $($inCells - $inFoot) more above the foot in its cells" }
    elseif ($inRow -ne $inCells) { Add-Failure "$line, and $($inRow - $inCells) more elsewhere on row $($Code.Row)" }
    else { $line }
  }

  # 1. At rest
  $rest = Get-Shot $w
  $bg = $rest.GetPixel($g.X + 60 * $g.W + 2, $g.Y + 2)
  "the pane's background as drawn: #{0:X2}{1:X2}{2:X2}" -f $bg.R, $bg.G, $bg.B
  Save-Crop $rest (New-Rect $g.X $g.Y ($cols * $g.W) (20 * $g.H)) 'codes'
  Save-Zoomed $rest (New-Rect $g.X ($g.Y + $g.H) (45 * $g.W) (11 * $g.H)) 'codes-x3' -Zoom 3
  foreach ($c in $codes) { Test-Bar $rest $c 'at rest' }
  foreach ($n in $nonCodes) {
    $found = ($n.Rows | ForEach-Object { Get-Count $rest (Get-RowRect $g $_) $n.Hex } | Measure-Object -Sum).Sum
    if ($found) { Add-Failure "at rest, $($n.What) isn't a code, but $found pixels of row(s) $($n.Rows -join ', ') are #$($n.Hex)" }
    else { "at rest, $($n.What): no bar" }
  }
  # The near code: its outline (bottom and top pixel rows across it, a column at each side) in the
  # text's colour, the bar inside it in the code's; #402020's foot has none of the text's colour
  $bottom = { param($Shot, $Code, [string]$Hex = 'FFFFFF') Get-Count $Shot (New-Rect ($g.X + $Code.Col * $g.W) ($g.Y + ($Code.Row + 1) * $g.H - 1) (7 * $g.W) 1) $Hex }
  function Test-Outline($Shot, $Code, [string]$When) {
    $x = $g.X + $Code.Col * $g.W; $right = $x + 7 * $g.W; $top = $g.Y + ($Code.Row + 1) * $g.H - $foot
    $across = 7 * $g.W; $inside = ($across - 2 * $edge) * $thick
    $parts = [ordered]@{
      bottom = @((& $bottom $Shot $Code), $across)
      top = @((Get-Count $Shot (New-Rect $x $top $across 1) 'FFFFFF'), $across)
      left = @((Get-Count $Shot (New-Rect $x $top 1 $foot) 'FFFFFF'), $foot)
      right = @((Get-Count $Shot (New-Rect ($right - 1) $top 1 $foot) 'FFFFFF'), $foot)
      fill = @((Get-Count $Shot (New-Rect ($x + $edge) ($top + $edge) ($across - 2 * $edge) $thick) $Code.Hex), $inside)
    }
    $line = "$When, $($Code.What): outline " + (($parts.Keys | ForEach-Object { "$_ $($parts[$_][0]) of $($parts[$_][1])" }) -join ', ')
    $short = @($parts.Keys | Where-Object { $parts[$_][0] -lt [Math]::Floor(0.9 * $parts[$_][1]) })
    if ($short) { Add-Failure "$line, where its $($short -join ' and ') should be whole" } else { $line }
  }
  Save-Zoomed $rest (Get-CellRect $g $near.Row 0 15) 'near-x6' -Zoom 6
  Test-Outline $rest $near 'at rest'
  $dark = $codes | Where-Object Hex -eq '402020'
  $notOutlined = & $bottom $rest $dark
  if ($notOutlined) { Add-Failure "at rest, $($dark.What): $notOutlined pixels along its foot's bottom are the text's colour: outlined, where it's far enough from the background" }
  # The program's swatch: its foot its own background, with no outline of its black text round it
  Save-Zoomed $rest (Get-CellRect $g $swatch.Row 0 30) 'hidden-x3' -Zoom 3
  $swatchFoot = Get-Count $rest (Get-CellRect $g $swatch.Row $swatch.Col ($swatch.Col + 6) -Foot $thick) $swatch.Hex
  $swatchLine = & $bottom $rest $swatch '000000'
  if ($swatchLine) { Add-Failure "at rest, $($swatch.What): $swatchLine pixels along its foot's bottom are its text's black: an outlined bar on the program's own swatch, where it gets none" }
  elseif ($swatchFoot -lt [Math]::Floor(0.9 * 7 * $g.W * $thick)) { Add-Failure "at rest, $($swatch.What): $swatchFoot of $(7 * $g.W * $thick) foot pixels are its background: the program's swatch isn't drawn as the script means" }
  else { "at rest, $($swatch.What): no bar or outline, its foot the program's own background" }

  # 2. The pointer on the link over #40FF80: the link found (its row shows the hover), the bar as at rest
  $link = $codes | Where-Object Hex -eq '40FF80'
  $p = Get-CellPoint $g ($link.Col + 3) $link.Row
  $f = Get-StateFrame $w
  Send-Mouse $w Move $p[0] $p[1]
  Wait-Change { param($s) $true } -Window $w -AfterFrame $f -What 'the link under the pointer drawn' | Out-Null
  Wait-Settled -Window $w | Out-Null
  $hover = Get-Shot $w
  if (-not (Get-DiffBox $rest $hover -Region (Get-CellRect $g $link.Row $link.Col ($link.Col + 6)))) { Add-Failure "the pointer on the link over $($link.What) shows nothing new on its cells: the link isn't found over the code" }
  else { 'hover: the link over #40FF80 is found and drawn hovered' }
  Test-Bar $hover $link 'under the pointer'
  Save-Crop $hover (Get-CellRect $g $link.Row 0 30) 'hover'
  Send-Leave $w
  Wait-Settled -Window $w | Out-Null

  # 3. A selection across row 11: the bar as at rest, the copy what the program printed. The drag
  # ends on the blank cell after the text: a cell joins a forward selection only once the pointer
  # is past its middle, so a drag ending on the text's last cell's middle left it out (PITFALLS 281)
  $sel = $codes | Where-Object Hex -eq 'A060FF'
  $a = Get-CellPoint $g 0 $sel.Row; $b = Get-CellPoint $g $Row11.Length $sel.Row
  $f = Get-StateFrame $w
  Send-Drag $w $a[0] $a[1] $b[0] $b[1]
  Wait-Settled -Window $w -AfterFrame $f -What 'the selection drawn' | Out-Null
  $selected = Get-Shot $w
  Test-Bar $selected $sel 'selected'
  Save-Crop $selected (Get-CellRect $g $sel.Row 0 30) 'selected'
  $copied = (Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).copied
  if ($copied -ne $Row11) { Add-Failure "the selection across row $($sel.Row) copied '$copied', not the text printed, '$Row11'" }
  else { "selected: copied '$copied', as printed" }
  $selected.Dispose(); $hover.Dispose()

  # 4. The rows scrolled up one: every bar whole at its new row, from the quads kept for what the
  # row shows, and only the row the scroll brings laid out, with the cursor's and the selection's
  # when the scroll clears it (ConPTY may send a scroll as rows written again, which clears a
  # selection on them), never the rows that moved
  $before = [long](Read-State -Window $w).work.lines
  $f = Get-StateFrame $w
  Send-Key $w 's'
  Wait-State { param($s) @($s.panes)[0].title -eq 'hex scrolled' } -Window $w -AfterFrame $f -What 'the rows scrolled' | Out-Null
  Wait-Settled -Window $w | Out-Null
  $laid = [long](Read-State -Window $w).work.lines - $before
  $up = { param($c) [pscustomobject]@{ Row = $c.Row - 1; Col = $c.Col; Hex = $c.Hex; What = $c.What } }
  $scrolled = Get-Shot $w
  foreach ($c in $codes) { Test-Bar $scrolled (& $up $c) 'scrolled up one' }
  Test-Outline $scrolled (& $up $near) 'scrolled up one'
  if ($laid -gt 3) { Add-Failure "scrolled up one: the scroll's frames laid out $laid lines, where only the row it brings, the cursor's and the selection's are new: a row that only moved was laid out again" }
  else { "scrolled up one: $laid lines laid out, every bar from its row's kept quads" }

  # 5. The cursor, a steady block, on #FFBF00's third cell: the cursor over the bar there, the bar
  # whole under the other six
  $amber = & $up ($codes | Where-Object Hex -eq 'FFBF00')
  $f = Get-StateFrame $w
  Send-Key $w 'c'
  Wait-State { param($s) @($s.panes)[0].title -eq 'hex cursor' } -Window $w -AfterFrame $f -What 'the cursor shown' | Out-Null
  Wait-Settled -Window $w | Out-Null
  $cursorShot = Get-Shot $w
  Save-Zoomed $cursorShot (Get-CellRect $g $amber.Row 0 15) 'cursor-x3' -Zoom 3
  $underCursor = Get-Count $cursorShot (Get-CellRect $g $amber.Row ($amber.Col + 2) ($amber.Col + 2) -Foot $thick) $amber.Hex
  $others = (@(0, 1, 3, 4, 5, 6) | ForEach-Object { Get-Count $cursorShot (Get-CellRect $g $amber.Row ($amber.Col + $_) ($amber.Col + $_) -Foot $thick) $amber.Hex } | Measure-Object -Sum).Sum
  $line = "the cursor on $($amber.What)'s third cell: $underCursor of $($g.W * $thick) foot pixels under it in #$($amber.Hex), $others of $(6 * $g.W * $thick) under the other six"
  if ($others -lt [Math]::Floor(0.9 * 6 * $g.W * $thick)) { Add-Failure "$line, where the other six keep their bar" }
  elseif ($underCursor -ge $g.W * $thick) { Add-Failure "$line, where the cursor is drawn over the bar on its cell" }
  else { $line }
  $scrolled.Dispose(); $cursorShot.Dispose()
  Stop-TestWindow $w

  # 6. lbterm_hex_colours = false: no bars; the rows with no code as the first window drew them,
  # and those with codes different only in the codes' feet
  $off = Open-Program -Set 'lbterm_hex_colours=false'
  $og = $off.Grid
  if ($og.X -ne $g.X -or $og.Y -ne $g.Y -or $og.W -ne $g.W -or $og.H -ne $g.H -or $og.Cols -ne $g.Cols) {
    Add-Failure "the window with the switch off has a grid of $($og.Cols) cells of $($og.W) by $($og.H) at $($og.X),$($og.Y), not the first's: not compared"
  } else {
    $offShot = Get-Shot $off.Window
    Save-Crop $offShot (New-Rect $g.X $g.Y ($cols * $g.W) (20 * $g.H)) 'off'
    foreach ($c in $codes) {
      $n = Get-Count $offShot (Get-RowRect $g $c.Row) $c.Hex
      if ($n) { Add-Failure "switched off, $($c.What) still has $n pixels of #$($c.Hex) on its row" }
    }
    $o = & $bottom $offShot $near
    if ($o -ge [Math]::Floor(0.9 * 7 * $g.W)) { Add-Failure "switched off, $($near.What) is still outlined ($o pixels)" }
    'switched off: no code''s colour on its row'
    foreach ($r in 0..20) {
      $here = @($codes + $near | Where-Object Row -eq $r)
      $all = [LbtermE2E.Px]::Differing($rest, $offShot, (Get-RowRect $g $r))
      $inFeet = ($here | ForEach-Object { [LbtermE2E.Px]::Differing($rest, $offShot, (Get-CellRect $g $r $_.Col ($_.Col + 6) -Foot $foot)) } | Measure-Object -Sum).Sum
      if ($all -ne $inFeet) { Add-Failure "row ${r}: on and off differ in $($all - $inFeet) pixels outside the codes' feet" }
    }
    'switched off: the rows differ only in the codes'' feet'
    $offShot.Dispose()
  }
  $rest.Dispose()
} -Passed 'hex-swatch: every #RRGGBB code shows a bar in its colour along its cells'' foot, at rest, under a link''s hover, in a selection and moved by a scroll, the near one outlined, no non-code, hidden code or program''s swatch gets one, a block cursor covers the bar on its cell, a selection copies the text printed, and with lbterm_hex_colours off nothing shows'
