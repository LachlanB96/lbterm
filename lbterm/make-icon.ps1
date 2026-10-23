#Requires -Version 7
<#
Draws LBTerm's icon and writes it as a multi-size .ico (PNG entries, 16 to 256 px).
wezterm-gui's build.rs embeds assets\windows\lbterm.ico; Windows takes the taskbar,
Alt+Tab and title-bar icons from it.

  pwsh -File lbterm\make-icon.ps1                      write assets\windows\lbterm.ico
  pwsh -File lbterm\make-icon.ps1 -Preview <png>       also write a strip of every size to look at

Each size is drawn on its own canvas rather than scaled down from 256, so the small ones stay sharp.
#>
param(
  [string]$Out = (Join-Path $PSScriptRoot '..\assets\windows\lbterm.ico'),
  [string]$Preview
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$sizes  = 16, 24, 32, 48, 64, 128, 256
$top    = [Drawing.Color]::FromArgb(0x2a, 0x2e, 0x38)
$bottom = [Drawing.Color]::FromArgb(0x14, 0x16, 0x1b)
$ink    = [Drawing.Color]::FromArgb(0xf0, 0xf0, 0xf0)
$accent = [Drawing.Color]::FromArgb(0xff, 0x9e, 0x3b)   # the orange of the "waiting on you" status light

function New-Frame([int]$s) {
  $bmp = [Drawing.Bitmap]::new($s, $s, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'
  $g.TextRenderingHint = 'AntiAliasGridFit'
  $g.Clear([Drawing.Color]::Transparent)

  # Rounded square
  $r = [Math]::Max(2, [int]($s * 0.22))
  $path = [Drawing.Drawing2D.GraphicsPath]::new()
  $path.AddArc(0, 0, $r, $r, 180, 90)
  $path.AddArc($s - $r - 1, 0, $r, $r, 270, 90)
  $path.AddArc($s - $r - 1, $s - $r - 1, $r, $r, 0, 90)
  $path.AddArc(0, $s - $r - 1, $r, $r, 90, 90)
  $path.CloseFigure()
  $fill = [Drawing.Drawing2D.LinearGradientBrush]::new([Drawing.Point]::new(0, 0), [Drawing.Point]::new(0, $s), $top, $bottom)
  $g.FillPath($fill, $path)

  # "LB", and a block cursor under it. The taskbar and title-bar sizes have no room
  # for the cursor, so the letters fill the square instead.
  $small = $s -le 24
  $font = [Drawing.Font]::new('Cascadia Mono', [single]($s * $(if ($small) { 0.60 } else { 0.46 })), [Drawing.FontStyle]::Bold, [Drawing.GraphicsUnit]::Pixel)
  $fmt = [Drawing.StringFormat]::new()
  $fmt.Alignment = 'Center'; $fmt.LineAlignment = 'Center'
  $text = [Drawing.RectangleF]::new(0, $(if ($small) { 0 } else { -$s * 0.06 }), $s, $s)
  $g.DrawString('LB', $font, [Drawing.SolidBrush]::new($ink), $text, $fmt)
  if (-not $small) {
    $cw = [int]($s * 0.20); $ch = [Math]::Max(2, [int]($s * 0.08))
    $g.FillRectangle([Drawing.SolidBrush]::new($accent), [int](($s - $cw) / 2), [int]($s * 0.76), $cw, $ch)
  }

  $g.Dispose()
  $bmp
}

$frames = foreach ($s in $sizes) {
  $bmp = New-Frame $s
  $ms = [IO.MemoryStream]::new()
  $bmp.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
  [pscustomobject]@{ Size = $s; Png = $ms.ToArray(); Bitmap = $bmp }
}

# ICONDIR, one ICONDIRENTRY per size, then the PNGs
$ico = [IO.MemoryStream]::new()
$w = [IO.BinaryWriter]::new($ico)
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
foreach ($f in $frames) {
  $dim = if ($f.Size -ge 256) { 0 } else { $f.Size }
  $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([uint16]1); $w.Write([uint16]32)
  $w.Write([uint32]$f.Png.Length); $w.Write([uint32]$offset)
  $offset += $f.Png.Length
}
foreach ($f in $frames) { $w.Write($f.Png) }
$w.Flush()
[IO.File]::WriteAllBytes([IO.Path]::GetFullPath($Out), $ico.ToArray())
"wrote $([IO.Path]::GetFullPath($Out)) ($($frames.Count) sizes, $($ico.Length) bytes)"

if ($Preview) {
  $width = [int](($sizes | Measure-Object -Sum).Sum + 10 * ($sizes.Count + 1))
  $strip = [Drawing.Bitmap]::new($width, 276)
  $g = [Drawing.Graphics]::FromImage($strip)
  $g.Clear([Drawing.Color]::FromArgb(0xdd, 0xdd, 0xdd))
  $x = 10
  foreach ($f in $frames) { $g.DrawImage($f.Bitmap, $x, 10, $f.Size, $f.Size); $x += $f.Size + 10 }
  $g.Dispose()
  $strip.Save($Preview, [Drawing.Imaging.ImageFormat]::Png)
  "preview $Preview"
}
