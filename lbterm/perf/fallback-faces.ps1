#Requires -Version 7
# Covers: wezterm-font/src/lib.rs, wezterm-font/src/lbterm.rs, wezterm-font/src/locator/gdi.rs, config/src/lbterm.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/mod.rs
<#
Which fallback face draws a character a font meets, held to not turning on what another font met
first (lbterm/PERF.md, "What font setup costs"; PITFALLS 54). Pixels, not counts: font-setup.ps1
counts searches, which can pass while a glyph is drawn from another face. Each window is its own
process (the faces a process's fonts found are shared within it: wezterm-font's lbterm.rs Found),
with the same config and the same steps, which end in a regular line of symbols none of the
bundled fonts has (★ ☆ ※ ① ② ◆ ◇ ♠ ♣ ♥ ♪ √ ∴ ≒ 〓), drawn after the screen is cleared and the
fonts are made again by a zoom a size bigger and back. Its pane is captured and compared.

1. Alone: nothing before the symbols.
2. After Hangul: a bold line of Hangul first, its search finding a Korean face that has some of
   the symbols too. The pane must be the first window's, pixel for pixel. perf/fonts-core's first
   build gave every font made after a search every face found so far, so after the zoom the
   regular font drew ※ ① ② ≒ 〓 from the Korean face the bold font had found: 718 pixels.
3. Searched together (said, not judged without -Exact): the bold line has the Hangul and the
   symbols in one run, so one search picks a face for each as a set, and the regular font meeting
   the symbols later takes the faces that search picked, where its own search, of the symbols
   alone, may pick others. The pixels that differ from the first window are said, with both
   captures, until Lachlan says whether a font may take the face another font's search chose for
   a character (PERF.md, "What font setup costs"); main's build searches again and draws them as
   the first window does. -Exact fails on any difference.

Captures, so best unlocked; a locked PC's PrintWindow still has the pane's text.

  pwsh -File lbterm\perf\fallback-faces.ps1 [-Bin <build>] [-Exact]
#>
param([string]$Bin, [switch]$Exact)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'fallback-faces' -Bin $Bin

function ConvertTo-Text([int[]]$CodePoints) { -join ($CodePoints | ForEach-Object { [char]$_ }) }
# The symbols: none in JetBrains Mono, Noto Color Emoji or Symbols Nerd Font Mono; a Korean face
# has ※ ① ② ≒ 〓 among them
$SymbolPoints = @(0x2605, 0x20, 0x2606, 0x20, 0x203B, 0x20, 0x2460, 0x20, 0x2461, 0x20, 0x25C6, 0x20, 0x25C7, 0x20, 0x2660, 0x20, 0x2663, 0x20, 0x2665, 0x20, 0x266A, 0x20, 0x221A, 0x20, 0x2234, 0x20, 0x2252, 0x20, 0x3013)
$HangulPoints = @(0xD55C, 0xAD6D, 0xC5B4, 0x20, 0xD14C, 0xC2A4, 0xD2B8)
# Written by the pane's pwsh from code points, so the typed command stays ASCII
function Get-WriteCommand([int[]]$CodePoints, [switch]$Bold) {
  $chars = ($CodePoints | ForEach-Object { '0x{0:X}' -f $_ }) -join ','
  if ($Bold) { '[Console]::Write("`e[1m" + (-join [char[]](' + $chars + ')) + "`e[0m`r`n")' }
  else { '[Console]::Write((-join [char[]](' + $chars + ')) + "`r`n")' }
}

$cfg = Join-Path $T.Out 'config\fallback-faces.lua'

# One window: -First written first (bold), the screen cleared, a zoom in and out, the symbols.
# Its pane captured as <Name>.png; the capture, the pane's rectangle and the fonts' counts
function Show-Symbols([string]$Name, [int[]]$First) {
  $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram) -TimeoutSec 30
  try {
    Wait-State { param($s) @($s.panes).Count -ge 1 } -Window $w -What 'its pane' -TimeoutSec 30 | Out-Null
    Get-QuietFrame -QuietMs 700 -TimeoutSec 20 -Window $w | Out-Null
    Send-Focus $w
    $pane = (Get-Panes -Window $w)[0].pane_id
    $say = {
      param([string]$Line)
      Invoke-Cli send-text --pane-id $pane --no-paste "$Line`r" -Window $w -Check | Out-Null
      Start-Sleep -Milliseconds 1500
      Get-QuietFrame -QuietMs 800 -TimeoutSec 20 -Window $w | Out-Null
    }
    # The prompt plain and the console in UTF-8 (its code page would make the symbols '?')
    & $say 'function prompt { "> " }; [Console]::OutputEncoding = [Text.Encoding]::UTF8; Clear-Host'
    if ($First) {
      & $say (Get-WriteCommand $First -Bold)
      & $say 'Clear-Host'
    }
    # The fonts made again at another size and back
    foreach ($k in 'F9', 'F10') {
      $was = (Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).size.width
      Send-Key $w $k
      Wait-State { param($s) $s.size.width -ne $was } -Window $w -What 'the window at its new size' -TimeoutSec 10 | Out-Null
      Start-Sleep -Milliseconds 1600
      Get-QuietFrame -QuietMs 800 -TimeoutSec 20 -Window $w | Out-Null
    }
    & $say 'Clear-Host'
    & $say (Get-WriteCommand $SymbolPoints)
    Get-QuietFrame -QuietMs 1000 -TimeoutSec 20 -Window $w | Out-Null
    $r = (Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).panes[0].rect
    $shot = Get-Shot $w -Name $Name
    $f = (Get-Metrics -Window $w).fonts
    $searches = if ($f) { [int]$f.fallback_searches.count } else { $null }
    $shared = if ($f) { [int]$f.fallback_shared.count } else { $null }
    Write-Host "${Name}: pane at $($r.x),$($r.y) $($r.w)x$($r.h); fallback searches $searches, answered from another font's $shared"
    [pscustomobject]@{ Shot = $shot; Rect = [System.Drawing.Rectangle]::new([int]$r.x, [int]$r.y, [int]$r.w, [int]$r.h) }
  } finally {
    Stop-TestWindow $w
  }
}

# How the pane of $B differs from $A's: nothing, or the count and the box
function Compare-Pane($A, $B) {
  if ($A.Rect -ne $B.Rect) { return "the panes differ in place or size ($($A.Rect) against $($B.Rect))" }
  $n = [LbtermE2E.Px]::Differing($A.Shot, $B.Shot, $A.Rect)
  if (-not $n) { return }
  $box = [LbtermE2E.Px]::DiffBox($A.Shot, $B.Shot, $A.Rect)
  "$n pixels differ, x $($box[0])..$($box[2]), y $($box[1])..$($box[3])"
}

Invoke-Test {
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 70
config.initial_rows = 10
config.window_close_confirmation = 'NeverPrompt'
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-NoLogo' }
local act = wezterm.action
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = act.IncreaseFontSize })
table.insert(config.keys, { key = 'F10', mods = 'NONE', action = act.DecreaseFontSize })
"@
  $alone = Show-Symbols 'symbols-alone'
  $after = Show-Symbols 'symbols-after-hangul' -First $HangulPoints
  $together = Show-Symbols 'symbols-after-searched-together' -First ($HangulPoints + 0x20 + $SymbolPoints)

  $d = Compare-Pane $alone $after
  if ($d) { Add-Failure "after a bold line of Hangul, the regular symbols were drawn from other faces than alone: $d (symbols-alone.png against symbols-after-hangul.png). A font took a face for characters it never met, found by another font (PITFALLS 54)" }
  else { Write-Host 'after Hangul: the pane as alone, pixel for pixel' }

  $d = Compare-Pane $alone $together
  if (-not $d) { Write-Host 'searched together: the pane as alone, pixel for pixel' }
  elseif ($Exact) { Add-Failure "after a bold line that searched for the Hangul and the symbols together, the regular symbols were drawn from other faces than alone: $d (symbols-alone.png against symbols-after-searched-together.png)" }
  else { Write-Host "searched together: $d (symbols-alone.png against symbols-after-searched-together.png): the faces another font's search picked for them as a set, which Lachlan is asked about; -Exact fails on it" }
  foreach ($s in $alone, $after, $together) { $s.Shot.Dispose() }
} -Passed 'fallback faces: the regular symbols drawn from the same faces after another font met Hangul and the fonts were made again as with nothing before them'
