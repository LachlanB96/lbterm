#Requires -Version 7
# Covers: lbterm-panel/src/claude.rs, lbterm-panel/src/palette.rs, wezterm-gui/src/termwindow/render/tab_panel.rs
<#
What only Terminal's hooks know about a Claude reply, shown over Claude Code's own OSC 7501 done
(lbterm/TAB-PANEL.md, "What a row says"): a reply that asks Lachlan something in text lights the
claude_status user var orange (ff8c00), and one whose only question is the extras it offers blue
(5b7cff). Three stand-in tabs print Claude Code's done record and that light themselves: one
asking, one offering extras, one plainly done.

The state file must say the first is waiting on a Question, the second idle with Extras and
extras true, the third idle and Done. The closed rail is captured (pad-question.png): beside the
second pad, and only there, a faint orange ring (the waiting pad's orange 60% of the way from
what's under it). Then the panel opens for the words (pad-question-open.png). Fails with exit 1.

  pwsh -File lbterm\perf\pad-question.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'pad-question' -Bin $Bin
$home2 = Join-Path $T.Out 'home'

# Claude Code's done, then claudehook's light, as each stand-in prints them
$lights = @{ asks = 'ff8c00'; extras = '5b7cff'; done = 'default' }
function Stand-In([string]$name) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("$($lights[$name])|$([DateTime]::UtcNow.ToFileTimeUtc())"))
  New-PwshProgram "`$e = [char]27; [Console]::Write(`"`$e]7501;state=done:app=claude`$e\`" + `"`$e]1337;SetUserVar=claude_status=$b64`" + [char]7); Write-Host '$name'; Start-Sleep 600"
}

# Pixels of the ring's orange in the strips just left and right of a pad, where only a ring is drawn
$ringOrange = [int[]]@(100, 255, 45, 160, 0, 100)
function Ring-Pixels($Shot, $Pad, [double]$Scale) {
  $near = ConvertTo-Px 1 $Scale
  $far = ConvertTo-Px 4 $Scale
  $y = [int]$Pad.y; $h = [int]$Pad.h
  $left = New-Rect ([int]$Pad.x - $far) $y ($far - $near) $h
  $right = New-Rect ([int]($Pad.x + $Pad.w) + $near) $y ($far - $near) $h
  [LbtermE2E.Px]::CountRange($Shot, $left, $ringOrange) + [LbtermE2E.Px]::CountRange($Shot, $right, $ringOrange)
}

Invoke-Test {
  $w = Open-TestWindow -UserProfile $home2 -WorkingDirectory $T.Repo -PostedPointer -Program (Stand-In asks) `
    -Set 'lbterm_tab_panel={enabled=true,open_delay_ms=0}'
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  foreach ($name in 'extras', 'done') {
    Invoke-Cli spawn --window-id 0 -Program (Stand-In $name) | Out-Null
  }
  # Each tab on screen once, so a done result counts as seen and wears no unread mark, then the first back
  foreach ($i in 1, 2, 0) {
    Invoke-Cli activate-tab --pane-id 0 --tab-index $i | Out-Null
    Wait-Change { param($s) $s.active_tab -eq $i } -What "tab $i open" -Animating | Out-Null
  }
  $e = Wait-Change { param($s) @($s.rail.rows).Count -eq 3 -and @($s.rail.rows)[1].extras -and @($s.rail.rows)[0].state -eq 'waiting' } `
    -What 'the first pad asking, the second offering extras' -Animating
  $rows = @($e.rail.rows)
  $want = @(
    @{ state = 'waiting'; status = 'Question'; extras = $false }
    @{ state = 'idle'; status = 'Extras'; extras = $true }
    @{ state = 'idle'; status = 'Done'; extras = $false }
  )
  for ($i = 0; $i -lt 3; $i++) {
    $r = $rows[$i]; $x = $want[$i]
    "row ${i}: $($r.state), $($r.status), extras $($r.extras)"
    if ($r.state -ne $x.state -or $r.status -ne $x.status -or [bool]$r.extras -ne $x.extras) {
      Add-Failure "row $i is $($r.state), $($r.status), extras $($r.extras); expected $($x.state), $($x.status), extras $($x.extras)"
    }
  }

  $shot = Get-Shot $w -Name 'pad-question'
  $ring = 0..2 | ForEach-Object { Ring-Pixels $shot $rows[$_].pad $w.Scale }
  $shot.Dispose()
  "ring pixels beside each pad: $($ring -join ', ')"
  if ($ring[1] -lt [Math]::Round(20 * $w.Scale)) { Add-Failure "no orange ring beside the extras pad ($($ring[1]) pixels)" }
  foreach ($i in 0, 2) { if ($ring[$i] -gt 2) { Add-Failure "orange beside pad $i, which has no ring ($($ring[$i]) pixels)" } }

  # Open: the words
  Set-PointerHold $w (ConvertTo-Px 40 $w.Scale) (ConvertTo-Px 160 $w.Scale)
  Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating | Out-Null
  (Get-Shot $w -Name 'pad-question-open').Dispose()
  Clear-PointerHold
} -Passed 'pad-question: a question asked in text waits on you, and extras alone are blue in a faint orange ring, over Claude Code''s done'
