#Requires -Version 7
<#
Records the tab panel opening and closing (lbterm/TAB-PANEL.md): the motion check, and a demo.

  pwsh -File lbterm\motion\panel-demo.ps1                       # real speed: demo.gif
  pwsh -File lbterm\motion\panel-demo.ps1 -Slowdown 5 -OpenOnly # slowed: the edge against the spring
  pwsh -File lbterm\motion\panel-demo.ps1 -Interrupt              # out while it opens, back while it closes
  pwsh -File lbterm\motion\panel-demo.ps1 -Live                 # a window to try it in by hand
  pwsh -File lbterm\motion\panel-demo.ps1 -Split                # the first tab split in three

-Live opens the same window with the panel's usual delays, plays nothing, records nothing and
leaves the window open. -Split splits the first tab into three panes, each with a box out to its
edges, so the cards and their outlines show as the panel presses them over.

Opens target\release\lbterm-gui.exe (or -Bin's; with Start-Unfocused, so it leaves Claude's job
object and takes no focus) looking like Lachlan's LBTerm (New-TestConfig: his lbterm.lua without
its summon key or mux domain) with the compositor, motion and the panel on, and six stand-in tabs
(demo-tab.ps1) in different Claude states and projects. Its USERPROFILE is a scratch folder, so
lbterm.lua's handlers write nothing into the real ~\.claude. Then it plays the pointer as posted
WM_MOUSEMOVEs, with the panel's real delays: onto the rail (the ring's pending light at once, the
open 150 ms later), down to another row, off onto the panes and straight back within the close
delay (the light dips and comes back), then off for good. -Interrupt plays the turns instead: off
the panel while it's still opening (it turns round at once), and back on while it's still closing
(it opens again at once). The real mouse never moves.

A posted move would make Windows answer with WM_MOUSELEAVE at once, because the real cursor isn't
over the window. LBTERM_POSTED_POINTER (window/src/os/windows/window.rs) stops the window asking
for it, so the pointer stays where the last posted move put it.

Frames go to target\motion\panel-x<slowdown>. sheet.py measures the right edge of the panel's red
ring on an empty row of its card; -OpenOnly fits it to lbterm_motion.panel_open. gif.py makes
demo.gif at real speed. Needs Python with Pillow and numpy.
#>
param(
  [double]$Slowdown = 1,
  [switch]$OpenOnly,
  [switch]$Interrupt,
  [switch]$Keep,
  [switch]$Live,
  [switch]$Split,
  # More settings for the window, each name=value as --config takes them
  [string[]]$Config = @(),
  # WEZTERM_LOG for the window, as module=level
  [string]$Log,
  [string]$Bin
)
$ErrorActionPreference = 'Stop'
# On the right-most screen, without taking focus: the harness's test windows
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
if ($Live) { $Keep = $true }
# Measuring the edge, the panel opens on the first posted move; otherwise it waits as usual
$panelConfig = if ($OpenOnly) { 'lbterm_tab_panel={enabled=true,open_delay_ms=0}' } else { 'lbterm_tab_panel={enabled=true}' }
# Measuring the edge, the running light stays off: its halo crossing the measured row reads as the
# ring's edge
$sweep = if ($OpenOnly) { ",panel_sweep='none'" } else { '' }
# By hand it has to fit the portrait screen it opens on; recordings capture the whole window anyway
$size = if ($Live) { @('initial_cols=100') } else { @() }

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$demoTab = Join-Path $PSScriptRoot 'demo-tab.ps1'
$out = Join-Path $repo "target\motion\panel-x$Slowdown"
$T = Start-Test "motion-panel-x$Slowdown" -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.panel'
$home2 = Join-Path $out 'home'

function Demo([string]$Title, [string]$State = 'none', [double]$Age = 0, [switch]$Box) {
  $a = @('-Title', $Title, '-State', $State, '-AgeMinutes', $Age)
  if ($Box) { $a += '-Box' }
  New-PwshProgram -File $demoTab -ArgumentList $a
}

try {
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -Log $Log -WorkingDirectory $repo -Cwd $repo `
    -Set (@($Config) + 'lbterm_compositor=true' + "lbterm_motion={enabled='on',slowdown=$Slowdown$sweep}" + $panelConfig + $size) `
    -Program (Demo 'Tab panel for LBTerm' working -Box:$Split)
  # Every cli call names its window or pane: a window that never has focus has no focused pane
  Start-Sleep 2
  # Made-up projects, in folders of the demo's own: the panel names a tab's project after the
  # folder under Life\Projects
  $vault = Join-Path $out 'Life\Projects'
  $tabs = @(
    @{ cwd = "$vault\Garden"; title = 'Watering schedule'; state = 'waiting'; age = 3 }
    @{ cwd = "$vault\Journal"; title = 'Weekly review'; state = 'idle'; age = 18 }
    @{ cwd = "$vault\Terminal"; title = 'Summary pane height'; state = 'idle'; age = 52 }
    @{ cwd = "$vault\Website"; title = 'npm run dev'; state = 'none'; age = 0 }
    @{ cwd = "$vault\Shopping"; title = 'Price watch: monitor arm'; state = 'working'; age = 0 }
  )
  $titled = @(@{ pane = 0; title = 'Tab panel for LBTerm'; state = 'working' })
  foreach ($t in $tabs) {
    New-Item -ItemType Directory -Force $t.cwd | Out-Null
    $pane = Invoke-Cli spawn --window-id 0 --cwd $t.cwd -Program (Demo $t.title $t.state $t.age)
    $titled += @{ pane = [int]$pane; title = $t.title; state = $t.state }
  }
  # Claude Code's glyph in front, as it titles its own sessions: a braille spinner frame while
  # it works, ✳ otherwise
  foreach ($t in $titled) {
    $glyph = switch ($t.state) { 'none' { '' } 'working' { [string][char]0x2810 + ' ' } default { [string][char]0x2733 + ' ' } }
    Invoke-Cli set-tab-title --pane-id $t.pane "$glyph$($t.title)" | Out-Null
  }
  Invoke-Cli activate-tab --pane-id 0 --tab-index 0 | Out-Null
  if ($Split) {
    # Side by side, then the right one stacked: every kind of edge a card can have
    $right = Invoke-Cli split-pane --pane-id 0 --right --percent 45 -Program (Demo 'Split right' -Box)
    Invoke-Cli split-pane --pane-id ([int]$right) --bottom -Program (Demo 'Split below' -Box) | Out-Null
  }
  if ($Live) {
    "LBTerm demo window open (pid $($w.Pid)): rest the pointer on the panel at its left"
    return
  }
  Start-Sleep 2

  $cr = Get-ClientRect $w
  $s = $w.Scale
  $px = { param($v) ConvertTo-Px $v $s }

  # Rows start 13 px under the tab bar (about 28 px) and are 46 px apart: row 3 on the rail,
  # row 5 on the open panel, then the panes
  $rail = @((& $px 40), (& $px 163))
  $lower = @((& $px 160), (& $px 255))
  $panes = @((& $px 720), (& $px 300))
  # Each move is posted once, at its time in ms. The delays (150 ms to open, 300 to close) are
  # real time; only the motion is slowed
  $tOpen = 400
  $moves = [Collections.Generic.List[object]]::new()
  $move = { param($at, $to) $moves.Add([pscustomobject]@{ At = $at; To = $to }) }
  & $move $tOpen $rail
  if ($Interrupt) {
    $tOut = $tOpen + 150 + 100 * $Slowdown   # 100 ms into the 220 ms open
    $tBack = $tOut + 70 * $Slowdown          # 70 ms into the 150 ms close
    & $move $tOut $panes
    & $move $tBack $rail
    & $move ($tBack + 800 * $Slowdown) $panes
  } elseif (-not $OpenOnly) {
    $tRow = $tOpen + 150 + 1000 * $Slowdown
    $tOut = $tRow + 700 * $Slowdown
    $tBack = $tOut + 150                     # inside the close delay
    & $move $tRow $lower
    & $move $tOut $panes
    & $move $tBack $lower
    & $move ($tBack + 600 * $Slowdown) $panes
  }
  $tEnd = if ($OpenOnly) { $tOpen + 1400 * $Slowdown } else { $moves[$moves.Count - 1].At + 300 + 900 * $Slowdown }
  foreach ($m in $moves) { "move at $($m.At) ms to $($m.To -join ',')" }

  $next = @{ I = 0 }
  $posted = [Collections.Generic.List[string]]::new()
  $frames = Get-Frames $w ([int]$tEnd) -Raw -Each {
    param($ms)
    while ($next.I -lt $moves.Count -and $ms -ge $moves[$next.I].At) {
      Send-Mouse $w Move $moves[$next.I].To[0] $moves[$next.I].To[1]
      $posted.Add("$([DateTime]::Now.ToString('HH:mm:ss.fff')) posted a move to $($moves[$next.I].To -join ',')")
      $next.I++
    }
  }
  Save-Frames $frames -Dir $out -Window $w
  Remove-Frames $frames
  # When each move was posted, by the clock the GUI's log keeps (-Log)
  $posted | Set-Content (Join-Path $out 'moves.txt')
  "$($frames.Count) frames in $([Math]::Round($tEnd / 1000, 1)) s to $out (DPI scale $s)"
} finally {
  Close-Test -Keep:$Keep
}
if ($Live) { return }

# The panel's sizes as LBTerm works them out: a 4 px gutter, a 51pt rail, 217.5pt open
$gapPx = 2 * [Math]::Max(1, [Math]::Round(2 * $s))
$railPx = [Math]::Round(51 * 96 * $s / 72)
$widthPx = [Math]::Round(217.5 * 96 * $s / 72)
$row = $cr.Bottom - (& $px 30)
$sheet = @((Join-Path $PSScriptRoot 'sheet.py'), $out, '--row', $row, '--ring')
if ($OpenOnly) {
  # The ring's right edge on panel_open, spring(220ms, 0)
  $sheet += @('--expect', ($gapPx + $railPx), ($gapPx + $widthPx), (220 * $Slowdown), 0)
}
python @sheet
python (Join-Path $PSScriptRoot 'gif.py') $out --slowdown $Slowdown --crop "0,0,$([Math]::Min($cr.Right, (& $px 760))),$($cr.Bottom)" --scale ([Math]::Round(0.8 / $s, 3))
