#Requires -Version 7
# Args: -ClickMark 10
# Covers: wezterm-gui/src/scrollbar.rs, lbterm-extras/src/marks.rs, lbterm-extras/src/scrollbar.rs, wezterm-gui/src/termwindow/mouseevent.rs
<#
Shows the scrollbar's prompt marks: a test window (no config but the scrollbar, the WebGpu front
end and the compositor, on the right-most screen, without focus) gets a pane that prints 40 shell
prompts marked with OSC 133, each followed by a few lines of output, and a capture of the window
(PrintWindow, which sees DirectComposition content) goes to target\perf\scrollbar-marks\marks.png,
with the scrollbar's column cut out and enlarged in marks-bar.png. -ClickMark n then posts a click
on the n-th mark and captures again (marks-clicked.png): that prompt should be at the top.

  pwsh -File lbterm\perf\scrollbar-marks.ps1 -ClickMark 10
#>
param([string]$Bin, [int]$Prompts = 40, [int]$ClickMark = 0)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'scrollbar-marks' -Bin $Bin

# Where the marks are: coral runs down a column inside the scrollbar
$coral = New-ColourRange 201 255 101 169 91 159

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'", 'lbterm_compositor=true', 'enable_scroll_bar=true', 'initial_rows=30' -Program (New-PwshProgram)
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  # A prompt (A..B), a command, its output (C) and its end (D), $Prompts times, then wait
  $script = @"
`$e = [char]27
for (`$i = 1; `$i -le $Prompts; `$i++) {
  [Console]::Write("`$e]133;A`$e\PS C:\demo> `$e]133;B`$e\Get-Thing `$i`n`$e]133;C`$e\")
  for (`$j = 0; `$j -lt (`$i % 4 + 1); `$j++) { [Console]::WriteLine("output line `$j of thing `$i") }
  [Console]::Write("`$e]133;D;0`$e\")
}
Start-Sleep 60
"@
  $pane = New-Tab -From $first -Program (New-PwshProgram $script)
  Invoke-Cli activate-pane --pane-id $pane | Out-Null
  $e = Wait-Change { param($s) $s.scrollbar.marks -ge $Prompts } -What "$Prompts prompt marks on the scrollbar" -Animating

  $shot = Get-Shot $w -Name 'marks'
  # The right-most 40 px, four times as wide, so the marks can be seen
  Save-Zoomed $shot (New-Rect ($shot.Width - 40) 0 40 $shot.Height) 'marks-bar' -Zoom 4 -ZoomY 1
  # The scrollbar lies inside the card's outline (8 px in at the 96 DPI the test screen has) and is
  # a cell (9 px) wide
  $x = $shot.Width - 12
  $marks = @([LbtermE2E.Px]::RunStartsDown($shot, $x, $coral))
  $shot.Dispose()
  "captured marks.png and marks-bar.png: $($marks.Count) marks ($($e.scrollbar.marks) in the state file)"

  if ($ClickMark -gt 0 -and $marks.Count -ge $ClickMark) {
    # A click on mark n, posted, so the real mouse never moves
    $y = $marks[$ClickMark - 1] + 1
    Send-Click $w $x $y
    Wait-Change { param($s) $s.landed } -What 'the prompt the mark leads to' -Animating | Out-Null
    (Get-Shot $w -Name 'marks-clicked').Dispose()
    "clicked mark $ClickMark at y=${y}: marks-clicked.png"
  }
} -Passed 'scrollbar-marks: captured'
