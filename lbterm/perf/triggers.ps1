#Requires -Version 7
# Variant: -MotionOff
# Covers: lbterm-triggers/**, wezterm-gui/src/termwindow/render/triggers.rs, wezterm-gui/src/termwindow/compositor.rs
<#
Shows the triggers' easter eggs (lbterm/TRIGGERS.md): a test window (no config, so the built-in
rules, with the WebGpu front end and the compositor, on the right-most screen, without focus)
gets a pane that prints a line with "love" and "LBTerm" in it, and the window is captured
(PrintWindow, which sees DirectComposition content) at four moments as the hearts rise:
target\perf\triggers\triggers-<ms>.png. Motion is on (lbterm_motion.enabled = 'on'), so DWM
plays the hearts; -MotionOff turns it off, so the GUI draws them frame by frame, as it does under
the default 'system' on a PC with Windows' animation effects off (Lachlan's is one).

Then, while they're still rising, the tab is switched away and back: in the frame with the other
tab the hearts are gone from its pane's corner, and back over theirs once it shows again, as
the GUI's own drawing has them (lbterm_triggers::hearts::follow, with DWM).

It fails when a capture in the hearts' first 900 ms has no pink in the pane's bottom right
corner (where the hearts rise, clear of the line's own pink "love"), when the other tab shows
any, or when the tab switched back to shows none. A locked PC leaves DirectComposition out of
captures: then it says so and checks no pixels.

  pwsh -File lbterm\perf\triggers.ps1 [-MotionOff]
#>
param([string]$Bin, [switch]$MotionOff, [int[]]$At = @(150, 450, 900, 1500), [int]$MinPink = 10)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('triggers' + $(if ($MotionOff) { '-motion-off' } else { '' })) -Bin $Bin
$Motion = if ($MotionOff) { 'off' } else { 'on' }

# The pane's bottom right, where the hearts rise: its right 160 px (at 96 DPI) from its fifth row
# down, clear of the lines printed at its top
function Get-Corner($sw, $pane) {
  $scale = $sw.size.dpi / 96
  $r = $pane.rect
  $rowH = $r.h / [math]::Max(1, $pane.rows)
  $left = [int]($r.x + $r.w - 160 * $scale)
  $top = [int]($r.y + 5 * $rowH)
  [Drawing.Rectangle]::new($left, $top, [int]($r.x + $r.w - $left), [int]($r.y + $r.h - $top))
}

# Pixels in $rect pink enough to be a heart's, at any of its opacities over the dark card: red
# well over green and over blue
function Get-Pink($shot, $rect) {
  $px = [LbtermE2E.Px]::Pixels($shot)
  $n = 0
  for ($y = [math]::Max(0, $rect.Top); $y -lt [math]::Min($shot.Height, $rect.Bottom); $y++) {
    for ($x = [math]::Max(0, $rect.Left); $x -lt [math]::Min($shot.Width, $rect.Right); $x++) {
      $c = $px[$y * $shot.Width + $x]
      $r = ($c -shr 16) -band 255; $g = ($c -shr 8) -band 255; $b = $c -band 255
      if ($r -ge 60 -and $r - $g -ge 30 -and $r - $b -ge 12) { $n++ }
    }
  }
  $n
}

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'", 'lbterm_compositor=true', "lbterm_motion={enabled='$Motion'}", 'initial_rows=16', 'initial_cols=70' -Program (New-PwshProgram)
  Wait-State { param($s) $s.frame -gt 0 -and $s.panes.Count -eq 1 } -What 'the window' | Out-Null
  $first = (Get-Panes)[0].pane_id
  $script = @'
'Welcome to LBTerm.'
'I love this terminal. Lovely, but not a glove or MLB.'
'cd C:\Code\LBTerm'
while ($true) { [void][Console]::ReadKey($true); 'love, again' }
'@
  $pane = New-Tab -From $first -Program (New-PwshProgram $script)
  Invoke-Cli activate-pane --pane-id $pane | Out-Null
  # The hearts start when the line is first drawn: the state file says when they're in the air
  Wait-State { param($s) $s.hearts -gt 0 } -What 'the hearts' | Out-Null
  $shown = [Diagnostics.Stopwatch]::StartNew()
  $sw = Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)
  $corner = Get-Corner $sw (@($sw.panes | Where-Object { $_.id -eq $pane -or $_.active })[0])
  $locked = Test-WorkstationLocked
  $pinks = [ordered]@{}
  foreach ($ms in $At) {
    $wait = $ms - $shown.ElapsedMilliseconds
    if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
    $shot = Get-Shot $w -Name "triggers-$ms"
    $pinks["$ms"] = Get-Pink $shot $corner
    $shot.Dispose()
  }
  "captured triggers-{$($At -join ',')}.png (motion $Motion); pink in the pane's corner: $(($pinks.GetEnumerator() | ForEach-Object { "$($_.Key) ms $($_.Value)" }) -join ', ')"

  # Away and back while a second burst rises: a key has the program print another love line,
  # after the easter egg's cooldown of 4 s
  $wait = 4300 - $shown.ElapsedMilliseconds
  if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
  Send-Key $w L
  $risen = Wait-Until { (Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).hearts -gt 0 } -TimeoutSec 5
  $switch = [ordered]@{}
  if ($risen) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Milliseconds 100
    $shot = Get-Shot $w -Name 'switch-before'; $switch.before = Get-Pink $shot $corner; $shot.Dispose()
    Invoke-Cli activate-pane --pane-id $first | Out-Null
    Wait-State { param($s) @($s.panes | Where-Object active)[0].id -eq $first } -Window $w -What 'the other tab' -TimeoutSec 5 | Out-Null
    Start-Sleep -Milliseconds 80
    $shot = Get-Shot $w -Name 'switch-away'; $switch.away = Get-Pink $shot $corner; $shot.Dispose()
    Invoke-Cli activate-pane --pane-id $pane | Out-Null
    Wait-State { param($s) @($s.panes | Where-Object active)[0].id -eq $pane } -Window $w -What 'the hearts'' tab again' -TimeoutSec 5 | Out-Null
    Start-Sleep -Milliseconds 80
    $shot = Get-Shot $w -Name 'switch-back'; $switch.back = Get-Pink $shot $corner; $shot.Dispose()
    $switch.back_at_ms = $clock.ElapsedMilliseconds
    "switched away and back as a second burst rose: pink in the corner $($switch.before) before, $($switch.away) on the other tab, $($switch.back) back ($($switch.back_at_ms) ms into the burst)"
  } else {
    Add-Failure "a second love line brought no hearts"
  }
  [ordered]@{ motion = $Motion; locked = $locked; corner = "$corner"; pinks = $pinks; switch = $switch } |
    ConvertTo-Json -Depth 4 | Set-Content (Join-Path $T.Out 'pink.json') -Encoding utf8

  if ($locked -or (Test-WorkstationLocked)) {
    (Get-Test).PassedLine = "triggers: captured as the hearts rise (motion $Motion), the PC locked, so no pixel checked"
    return
  }
  foreach ($ms in $At | Where-Object { $_ -le 900 }) {
    if ($pinks["$ms"] -lt $MinPink) { Add-Failure "$ms ms into the hearts the pane's corner had $($pinks["$ms"]) pink pixels (motion $Motion), under $MinPink" }
  }
  if ($risen) {
    if ($switch.before -lt $MinPink) { Add-Failure "the second burst showed $($switch.before) pink pixels before the switch, under $MinPink" }
    if ($switch.away -gt 0) { Add-Failure "the other tab showed $($switch.away) pink pixels where the hearts rose over the tab switched from" }
    # Its hearts are going by 1.6 s, so a switch back that slow says nothing
    if ($switch.back_at_ms -gt 1600) {
      "the tab came back $($switch.back_at_ms) ms into the burst, too late to judge its hearts"
      (Get-Test).PassedLine = "triggers: the hearts in the pane's corner as they rise (motion $Motion) and gone from the other tab; the switch back came too late to judge"
    }
    elseif ($switch.back -lt $MinPink) { Add-Failure "the tab switched back to showed $($switch.back) pink pixels in its corner, under $MinPink" }
  }
} -Passed "triggers: the hearts in the pane's corner as they rise (motion $Motion), gone from the other tab and back over theirs"
