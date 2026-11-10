#Requires -Version 7
# Covers: lbterm-panel/src/claude.rs, lbterm-panel/src/palette.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/mod.rs
<#
A working reply gone quiet or stuck, shown over Claude Code's own OSC 7501 working
(lbterm/TAB-PANEL.md, "A reply gone quiet or stuck"): Terminal's statusline sets the claude_quiet
user var through claudehook, quiet at 3 minutes with nothing written and no tool running, stuck at
7. Four stand-in tabs print Claude Code's working record, claudehook's working light and then a
claude_quiet of their own: one quiet, one stuck since 8 minutes ago, one with none, and one stuck
but older than its light (left over from before, so not believed).

The state file must say the first is working with quiet true, the second stuck with "Stuck? 8m",
the third and fourth working with quiet false. The closed rail is captured (pad-quiet.png): an
amber ring beside the first pad and no other, and the second pad filled with amber. Then the
panel opens for the words (pad-quiet-open.png). Fails with exit 1.

  pwsh -File lbterm\perf\pad-quiet.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'pad-quiet' -Bin $Bin
$home2 = Join-Path $T.Out 'home'

function Var([string]$name, [string]$value) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($value))
  "`$e]1337;SetUserVar=$name=$b64`" + [char]7 + `""
}
# Claude Code's working, claudehook's light, then the statusline's claude_quiet, as each stand-in
# prints them. after is how many FILETIME ticks the quiet value's stamp is after the light's
function Stand-In([string]$name, [string]$level, [int]$minutes, [long]$after) {
  $light = [DateTime]::UtcNow.ToFileTimeUtc()
  $since = [DateTimeOffset]::UtcNow.AddMinutes(-$minutes).ToUnixTimeMilliseconds()
  if ($level -eq 'none') { $since = 0 }
  $vars = (Var 'claude_status' "006400|$light") + (Var 'claude_quiet' "$level|$since|$($light + $after)")
  New-PwshProgram "`$e = [char]27; [Console]::Write(`"`$e]7501;state=working:app=claude`$e\`" + `"$vars`"); Write-Host '$name'; Start-Sleep 600"
}

# Amber (#FFBF00 and its blends over the dark rail): red high, green a good way under it, no blue
$amber = [int[]]@(150, 255, 105, 205, 0, 70)
function Ring-Pixels($Shot, $Pad, [double]$Scale) {
  $near = ConvertTo-Px 1 $Scale
  $far = ConvertTo-Px 4 $Scale
  $y = [int]$Pad.y; $h = [int]$Pad.h
  $left = New-Rect ([int]$Pad.x - $far) $y ($far - $near) $h
  $right = New-Rect ([int]($Pad.x + $Pad.w) + $near) $y ($far - $near) $h
  [LbtermE2E.Px]::CountRange($Shot, $left, $amber) + [LbtermE2E.Px]::CountRange($Shot, $right, $amber)
}
function Fill-Pixels($Shot, $Pad) {
  $inset = [int]($Pad.w / 4)
  [LbtermE2E.Px]::CountRange($Shot, (New-Rect ([int]$Pad.x + $inset) ([int]$Pad.y + $inset) ([int]$Pad.w - 2 * $inset) ([int]$Pad.h - 2 * $inset)), $amber)
}

Invoke-Test {
  $w = Open-TestWindow -UserProfile $home2 -WorkingDirectory $T.Repo -PostedPointer -Program (Stand-In quiet quiet 4 10) `
    -Set 'lbterm_tab_panel={enabled=true,open_delay_ms=0}'
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  Invoke-Cli spawn --window-id 0 -Program (Stand-In stuck stuck 8 10) | Out-Null
  Invoke-Cli spawn --window-id 0 -Program (Stand-In none none 0 10) | Out-Null
  Invoke-Cli spawn --window-id 0 -Program (Stand-In stale stuck 9 -10) | Out-Null
  # The first tab open, so the others are in the background, which a light alone repaints
  $e = Wait-Change { param($s) @($s.rail.rows).Count -eq 4 -and @($s.rail.rows)[1].state -eq 'stuck' -and @($s.rail.rows)[0].quiet } `
    -What 'the first pad quiet, the second stuck' -Animating
  $rows = @($e.rail.rows)
  $want = @(
    @{ state = 'working'; status = 'Working'; quiet = $true }
    @{ state = 'stuck'; status = 'Stuck? 8m'; quiet = $false }
    @{ state = 'working'; status = 'Working'; quiet = $false }
    @{ state = 'working'; status = 'Working'; quiet = $false }
  )
  for ($i = 0; $i -lt 4; $i++) {
    $r = $rows[$i]; $x = $want[$i]
    "row ${i}: $($r.state), $($r.status), quiet $($r.quiet)"
    if ($r.state -ne $x.state -or $r.status -ne $x.status -or [bool]$r.quiet -ne $x.quiet) {
      Add-Failure "row $i is $($r.state), $($r.status), quiet $($r.quiet); expected $($x.state), $($x.status), quiet $($x.quiet)"
    }
  }

  $shot = Get-Shot $w -Name 'pad-quiet'
  $ring = 0..3 | ForEach-Object { Ring-Pixels $shot $rows[$_].pad $w.Scale }
  $fill = 0..3 | ForEach-Object { Fill-Pixels $shot $rows[$_].pad }
  $shot.Dispose()
  "amber ring pixels beside each pad: $($ring -join ', '); amber inside each pad: $($fill -join ', ')"
  if ($ring[0] -lt [Math]::Round(20 * $w.Scale)) { Add-Failure "no amber ring beside the quiet pad ($($ring[0]) pixels)" }
  foreach ($i in 2, 3) { if ($ring[$i] -gt 2) { Add-Failure "amber beside pad $i, which has no ring ($($ring[$i]) pixels)" } }
  if ($fill[1] -lt [Math]::Round(40 * $w.Scale)) { Add-Failure "the stuck pad isn't amber ($($fill[1]) pixels)" }
  foreach ($i in 0, 2, 3) { if ($fill[$i] -gt 2) { Add-Failure "amber inside pad $i, which is green ($($fill[$i]) pixels)" } }

  # Open: the words
  Set-PointerHold $w (ConvertTo-Px 40 $w.Scale) (ConvertTo-Px 160 $w.Scale)
  Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating | Out-Null
  (Get-Shot $w -Name 'pad-quiet-open').Dispose()
  Clear-PointerHold
} -Passed 'pad-quiet: a quiet reply rings its green pad in amber and a stuck one turns amber saying its minutes, over Claude Code''s working'
