#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/thumbnails.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/screen_line.rs, wezterm-gui/src/thumbnails.rs, lbterm-extras/src/thumbnails.rs
<#
Thumbnails of the images a pane links to (lbterm/THUMBNAILS.md) end to end, in a test window of the
script's own config, whose open-uri handler logs a link and opens nothing. Its program turns on SGR
mouse reporting, as Claude Code does, and prints three pictures' links, as an agent's reply holds
them: rows 3 and 4 of a teal picture whose top is above the view, then side by side a magenta
picture's caption and five rows, and the same for a file that isn't there, then an SVG's caption
and four rows.

1. The magenta and teal pictures show (their state "shown"), each its colour where the state file
   says it's drawn, the teal one the two rows in view only; the missing file's box is "gone" and its
   caption struck through; a hairline in the rail's grey round the magenta one (shown.png).
2. The pointer over the magenta picture: it says hovered, and its hairline brightens (hovered.png).
3. A click on the picture opens its row's link, past the program's mouse.
4. The magenta file deleted: within a few seconds it's "gone", no magenta left, its caption struck
   (deleted.png); written again, it shows again (back.png).
5. The SVG, drawn at its box's size in yellow (svg.png), rewritten 30 times as a live tile is,
   moved into place blue and yellow in turn: each time the picture follows it, in the new colour
   (svg-<n>.png), drawn over its last decoding's place in the atlas, so it has taken one place
   (`places` 1) when the 30th is drawn.

  pwsh -File lbterm\perf\thumbnails.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'thumbnails' -Bin $Bin
$pics = Join-Path $T.Out 'pics'
Remove-Item -Recurse -Force $pics -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $pics | Out-Null

Add-Type -AssemblyName System.Drawing
function New-Png([string]$Path, [int]$W, [int]$H, [System.Drawing.Color]$Colour) {
  $bmp = New-Object System.Drawing.Bitmap $W, $H
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear($Colour); $g.Dispose()
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
$magenta = Join-Path $pics 'magenta one.png'
$teal = Join-Path $pics 'teal.png'
$gone = Join-Path $pics 'gone.png'
$chart = Join-Path $pics 'chart.svg'
# A live tile's SVG, written whole and moved into place as the Dashboards server writes one
function Write-Svg([string]$Fill) {
  Write-FileWhole -Path $chart -Text "<svg xmlns=`"http://www.w3.org/2000/svg`" viewBox=`"0 0 300 60`"><rect width=`"300`" height=`"60`" fill=`"$Fill`"/></svg>"
}
Write-Svg '#ffdc00'
New-Png $magenta 400 300 ([System.Drawing.Color]::FromArgb(255, 0, 255))
New-Png $teal 300 300 ([System.Drawing.Color]::FromArgb(0, 128, 128))
function Address([string]$Path) { 'lbterm://image/' + (($Path -replace '\\', '/') -replace ' ', '%20') }

$cfg = Join-Path $T.Out 'config\thumbnails.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
wezterm.on('open-uri', function(window, pane, uri)
  wezterm.log_warn('LBTERM-TEST open-uri ' .. uri)
  return false
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  initial_cols = 100, initial_rows = 16,
  window_close_confirmation = 'NeverPrompt',
  enable_tab_bar = false,
  cursor_blink_rate = 0,
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_cwd = 'C:/Windows',
}
"@)

$script = @"
`$e = [char]27; `$b = [char]0x2800
[Console]::OutputEncoding = [Text.UTF8Encoding]::new(`$false)
function Link([string]`$uri, [string]`$words) { "`$e]8;;`$uri`$e\`$words`$e]8;;`$e\" }
function Blank([int]`$n) { [string]::new(`$b, `$n) }
[Console]::Write("`$e[?1000h`$e[?1006h")
[Console]::WriteLine('  ' + (Link '$(Address $teal)?row=3&of=5' (Blank 12)))
[Console]::WriteLine('  ' + (Link '$(Address $teal)?row=4&of=5' (Blank 12)))
[Console]::WriteLine('')
[Console]::WriteLine('  ' + (Link '$(Address $magenta)' 'magenta one.png') + (Blank 8) + (Link '$(Address $gone)' 'gone.png'))
for (`$r = 0; `$r -lt 5; `$r++) {
  [Console]::WriteLine('  ' + (Link "$(Address $magenta)?row=`$r&of=5" (Blank 17)) + (Blank 6) + (Link "$(Address $gone)?row=`$r&of=5" (Blank 12)))
}
[Console]::WriteLine('')
[Console]::WriteLine('  ' + (Link '$(Address $chart)' 'chart.svg'))
for (`$r = 0; `$r -lt 4; `$r++) { [Console]::WriteLine('  ' + (Link "$(Address $chart)?row=`$r&of=4" (Blank 40))) }
Start-Sleep 600
"@

# The colours, dimmed (a window without the keyboard draws at 85%) or not
$pink = New-ColourRange 180 255 0 70 180 255
$cyan = New-ColourRange 0 60 85 150 85 150
$rest = New-ColourRange 75 115 75 115 88 128      # the hairline, #5E5E6E
$lit = New-ColourRange 115 160 115 160 115 160    # under the pointer, #898989
$struck = New-ColourRange 80 130 80 130 80 130    # a gone caption's grey, #717171
$yellow = New-ColourRange 180 255 150 235 0 60    # the SVG's #ffdc00
$blue = New-ColourRange 0 50 50 115 180 255       # and its #005aff

function Get-Thumb($s, [string]$Path) { @($s.thumbnails) | Where-Object { $_.path -eq ($Path -replace '\\', '/') } | Select-Object -First 1 }
function Get-Box($t, [int]$Grow = 0) { New-Rect ($t.rect.x - $Grow) ($t.rect.y - $Grow) ($t.rect.w + 2 * $Grow) ($t.rect.h + 2 * $Grow) }
function Count-In($w, $s, [string]$Name, $Box, $Range) {
  $shot = Get-Shot $w -Name $Name
  try { [LbtermE2E.Px]::CountRange($shot, $Box, $Range) } finally { $shot.Dispose() }
}
# The pixels of a hairline round a box: those in the band a pixel or two either side of its edge
function Count-Outline($w, [string]$Name, $t, $Range) {
  $shot = Get-Shot $w -Name $Name
  try {
    $outer = Get-Box $t 1
    $inner = Get-Box $t -2
    [LbtermE2E.Px]::CountRange($shot, $outer, $Range) - [LbtermE2E.Px]::CountRange($shot, $inner, $Range)
  } finally { $shot.Dispose() }
}
# Whether the caption over a picture is struck: a row of grey run across it
function Test-Struck($w, [string]$Name, $t, [double]$CellH) {
  $shot = Get-Shot $w -Name $Name
  try {
    $band = New-Rect $t.rect.x ([int]($t.rect.y - $CellH - 4)) 60 ([int]$CellH)
    @([LbtermE2E.Px]::RowsWithRun($shot, $band, $struck, 40)).Count -gt 0
  } finally { $shot.Dispose() }
}

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -SameProcess -Log 'warn' -PostedPointer -Program (New-PwshProgram $script)
  # 1. Shown, the teal one's two rows, the missing file gone
  $s = Wait-Change { param($s) (Get-Thumb $s $magenta).state -eq 'shown' -and (Get-Thumb $s $teal).state -eq 'shown' -and (Get-Thumb $s $gone).state -eq 'gone' } -What 'the three pictures drawn'
  $pane = @($s.panes)[0]
  $cellH = $pane.rect.h / [Math]::Max(1, $pane.rows)
  $m = Get-Thumb $s $magenta; $t = Get-Thumb $s $teal; $g = Get-Thumb $s $gone
  "magenta: $($m.rect | ConvertTo-Json -Compress); teal: $($t.rect | ConvertTo-Json -Compress); gone: $($g.rect | ConvertTo-Json -Compress)"
  $area = $m.rect.w * $m.rect.h
  $pinkIn = Count-In $w $s 'shown' (Get-Box $m) $pink
  if ($area -lt 2000 -or $pinkIn -lt 0.8 * $area) { Add-Failure "the magenta picture isn't drawn where the state says ($pinkIn magenta pixels of $area; shown.png)" }
  # 400 x 300, so it's as tall as it's wide by 3 to 4
  if ([Math]::Abs($m.rect.w / [Math]::Max(1, $m.rect.h) - 4 / 3) -gt 0.08) { Add-Failure "the magenta picture lost its shape: $($m.rect.w) x $($m.rect.h)" }
  $cyanIn = Count-In $w $s 'teal' (Get-Box $t) $cyan
  if ($t.rect.h -gt 2 * $cellH + 1 -or $cyanIn -lt 0.8 * $t.rect.w * $t.rect.h) { Add-Failure "the teal picture's two rows in view aren't drawn as they should be ($cyanIn teal pixels, $($t.rect.h) px tall; teal.png)" }
  if ($t.rect.y -gt $pane.rect.y + $cellH) { Add-Failure "the teal picture isn't at the top of the view: y $($t.rect.y)" }
  $outline = Count-Outline $w 'outline' $m $rest
  if ($outline -lt $m.rect.w) { Add-Failure "no hairline round the magenta picture ($outline pixels of the rail's grey; outline.png)" }
  if (-not (Test-Struck $w 'gone' $g $cellH)) { Add-Failure "the missing file's caption isn't struck through (gone.png)" }

  # 2. Hovered
  $cx = [int]($m.rect.x + $m.rect.w / 2); $cy = [int]($m.rect.y + $m.rect.h / 2)
  Send-Mouse $w Move $cx $cy
  $s = Wait-Change { param($s) (Get-Thumb $s $magenta).hovered } -What 'the magenta picture hovered'
  $litOutline = Count-Outline $w 'hovered' (Get-Thumb $s $magenta) $lit
  if ($litOutline -lt $m.rect.w) { Add-Failure "the hairline didn't brighten under the pointer ($litOutline pixels; hovered.png)" }
  # The row's link runs on past a picture narrower than its cells: no underline shows there
  $past = New-Rect ($m.rect.x + $m.rect.w + 2) $m.rect.y 40 $m.rect.h
  $underline = Count-In $w $s 'hovered-past' $past (New-ColourRange 60 255 60 255 60 255)
  if ($underline -gt 0) { Add-Failure "something's drawn past the hovered picture's right edge ($underline pixels; hovered-past.png)" }

  # 3. A click on it opens its row's link, past the program's mouse. A window that's never had the
  # keyboard takes no clicks, so it's given it first, without the foreground moving
  Send-Focus $w
  Start-Sleep -Milliseconds 300
  Send-Click $w $cx $cy -Move
  if (-not (Wait-LogLine $w 'LBTERM-TEST open-uri' -TimeoutSec 5)) { Add-Failure 'a click on the picture opened nothing' }
  $opened = @(Get-LogLines $w 'LBTERM-TEST open-uri (\S+)' | ForEach-Object { if ($_ -match 'LBTERM-TEST open-uri (\S+)') { $Matches[1] } })
  "opened: $($opened -join ', ')"
  if (-not ($opened | Where-Object { $_ -like "$(Address $magenta)?row=*" })) { Add-Failure "the click opened something else: $($opened -join ', ')" }
  Send-Leave $w

  # 4. Deleted, then back
  Remove-Item $magenta
  $s = Wait-Change { param($s) (Get-Thumb $s $magenta).state -eq 'gone' } -What 'the deleted picture gone' -TimeoutSec 8
  $m = Get-Thumb $s $magenta
  $pinkLeft = Count-In $w $s 'deleted' (Get-Box $m 2) $pink
  if ($pinkLeft -gt 0) { Add-Failure "magenta left once its file was deleted ($pinkLeft pixels; deleted.png)" }
  if (-not (Test-Struck $w 'deleted-caption' $m $cellH)) { Add-Failure "the deleted picture's caption isn't struck through (deleted-caption.png)" }
  New-Png $magenta 400 300 ([System.Drawing.Color]::FromArgb(255, 0, 255))
  $s = Wait-Change { param($s) (Get-Thumb $s $magenta).state -eq 'shown' } -What 'the picture back' -TimeoutSec 8
  $m = Get-Thumb $s $magenta
  $pinkBack = Count-In $w $s 'back' (Get-Box $m) $pink
  if ($pinkBack -lt 0.8 * $m.rect.w * $m.rect.h) { Add-Failure "the picture didn't come back with its file ($pinkBack pixels; back.png)" }

  # 5. The SVG at its box's size, then rewritten as a live tile is: one place in the atlas
  $s = Wait-Change { param($s) (Get-Thumb $s $chart).state -eq 'shown' } -What 'the SVG drawn'
  $c = Get-Thumb $s $chart
  "svg: $($c.rect | ConvertTo-Json -Compress), version $($c.version), places $($c.places)"
  # 300 x 60 across four rows of forty cells: as wide as the cells allow or as tall as the rows,
  # bigger than the 300 x 60 it says, its shape kept
  if ($c.rect.w -lt 300 -or [Math]::Abs($c.rect.w / [Math]::Max(1, $c.rect.h) - 5) -gt 0.15) { Add-Failure "the SVG isn't drawn at its box's size with its shape: $($c.rect.w) x $($c.rect.h)" }
  $yellowIn = Count-In $w $s 'svg' (Get-Box $c) $yellow
  if ($yellowIn -lt 0.8 * $c.rect.w * $c.rect.h) { Add-Failure "the SVG isn't drawn yellow where the state says ($yellowIn pixels; svg.png)" }
  $last = $c.version
  for ($n = 1; $n -le 30; $n++) {
    $fill = if ($n % 2) { '#005aff' } else { '#ffdc00' }
    Write-Svg $fill
    $s = Wait-Change { param($s) $t = Get-Thumb $s $chart; $t.state -eq 'shown' -and $t.version -gt $last } -What "the SVG's rewrite $n drawn" -TimeoutSec 8
    $c = Get-Thumb $s $chart
    $last = $c.version
    if ($n % 10 -eq 0 -or $n -eq 1) {
      $range = if ($n % 2) { $blue } else { $yellow }
      $in = Count-In $w $s "svg-$n" (Get-Box $c) $range
      if ($in -lt 0.8 * $c.rect.w * $c.rect.h) { Add-Failure "rewrite $n isn't drawn in its colour, $fill ($in pixels; svg-$n.png)" }
    }
  }
  "svg after 30 rewrites: version $($c.version), places $($c.places)"
  if ($c.places -ne 1) { Add-Failure "the SVG took $($c.places) places in the atlas over 30 rewrites, not one" }
} -Passed 'thumbnails passed: drawn in their rows, half of one at the top, a hairline that brightens under the pointer, a click opening it, gone and back with its file, an SVG at its box''s size rewritten 30 times in one place in the atlas'
