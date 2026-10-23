#Requires -Version 7
<#
Records the keyboard moving between panes (lbterm/DESIGN.md, step 2): the active pane's outline
gliding to the next pane on lbterm_motion.focus_move, and the panes dimming to 85% as they lose the
keyboard and back on lbterm_motion.dim.

  pwsh -File lbterm\motion\focus-demo.ps1                      # real speed: demo.gif
  pwsh -File lbterm\motion\focus-demo.ps1 -Slowdown 5 -Measure # one move, the outline against the spring

Opens target\release\lbterm-gui.exe (or -Bin's) with Start-Unfocused (no console, no focus, on the
right-most screen), looking like Lachlan's LBTerm (New-TestConfig) with the compositor and motion
on, and splits it into three panes, each a stand-in tab with a box out to its edges (demo-tab.ps1
-Box): left, right, and below right. Then it moves the keyboard with `lbterm cli activate-pane`,
left, right, below, left, every 900 ms (times -Slowdown), grabbing the window with PrintWindow as
it goes. Frames go to target\motion\focus-x<slowdown>, with demo.gif at real speed.

-Measure moves the keyboard once, from the left pane to the right one, and has sheet.py follow the
outline's top edge (the only line on the row it sits on: the other cards have no outline) and fit
it to focus_move. Needs Python with Pillow and numpy.
#>
param(
  [double]$Slowdown = 1,
  [switch]$Measure,
  [switch]$Keep,
  # More settings for the window, each name=value as --config takes them
  [string[]]$Config = @(),
  [string]$Bin
)
$ErrorActionPreference = 'Stop'
# On the right-most screen, without taking focus: the harness's test windows
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$demoTab = Join-Path $PSScriptRoot 'demo-tab.ps1'
$out = Join-Path $repo "target\motion\focus-x$Slowdown"
$T = Start-Test "motion-focus-x$Slowdown" -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.focus'
$home2 = Join-Path $out 'home'

function Demo([string]$Title) { New-PwshProgram -File $demoTab -ArgumentList '-Title', $Title, '-Box' }

try {
  # The running light stays off: its halo would cross the measured row
  $w = Open-TestWindow -UserProfile $home2 -WorkingDirectory $repo -Cwd $repo -Program (Demo Left) `
    -Set (@($Config) + 'lbterm_compositor=true' + "lbterm_motion={enabled='on',slowdown=$Slowdown,panel_sweep='none'}")
  # Every cli call names its pane, since a window that never has focus has no focused pane
  Start-Sleep 2
  $panes = @{ left = 0 }
  $panes.right = [int](Invoke-Cli split-pane --pane-id 0 --right --percent 45 -Program (Demo Right))
  $panes.below = [int](Invoke-Cli split-pane --pane-id $panes.right --bottom -Program (Demo Below))
  Invoke-Cli activate-pane --pane-id $panes.left | Out-Null
  Start-Sleep 2

  $moves = @(if ($Measure) { 'right' } else { 'right', 'below', 'left', 'right' })
  $gap = 900 * $Slowdown
  $tEnd = 400 + $moves.Count * $gap + 300
  $next = @{ I = 0 }
  # Each move is its own process, so the frames go on while the GUI answers it
  $frames = Get-Frames $w ([int]$tEnd) -Raw -Each {
    param($ms)
    if ($next.I -lt $moves.Count -and $ms -ge 400 + $next.I * $gap) {
      Start-Process $T.Cli -ArgumentList @('cli', '--no-auto-start', 'activate-pane', '--pane-id', $panes[$moves[$next.I]]) -NoNewWindow
      $next.I++
    }
  }
  Save-Frames $frames -Dir $out -Window $w
  Remove-Frames $frames
  "$($frames.Count) frames in $([Math]::Round($tEnd / 1000, 1)) s to $out"
} finally {
  Close-Test -Keep:$Keep
}

if ($Measure) {
  # The outline's top edge: the row under the tab bar with the longest run of its grey in the
  # first frame. Along it, the outline starts at the first of its pixels past the tab panel: on the
  # left pane before the move, on the right one after
  $probe = @'
import sys
import numpy as np
from PIL import Image
def grey(path):
    return np.asarray(Image.open(path).convert("L")).astype(int)
x0 = int(sys.argv[3])
a = grey(sys.argv[1])
line = lambda img, y: (img[y, x0:] > 26) & (img[y, x0:] < 40)
row = max(range(20, 80), key=lambda y: int(line(a, y).sum()))
start = lambda img: x0 + int(np.nonzero(line(img, row))[0][0])
print(row, start(a), start(grey(sys.argv[2])))
'@
  $frames = @(Get-ChildItem $out -Filter 'frame*.png' | Sort-Object Name)
  $row, $from, $to = (python -c $probe $frames[0].FullName $frames[-1].FullName 80) -split ' '
  "the outline's top edge is row ${row}: from x $from on the left pane to $to on the right"
  python (Join-Path $PSScriptRoot 'sheet.py') $out --row $row --from 80 --bright 26 --still --expect $from $to (300 * $Slowdown) 0
} else {
  python (Join-Path $PSScriptRoot 'gif.py') $out --slowdown $Slowdown --scale 0.6
}
