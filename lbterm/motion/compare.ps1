#Requires -Version 7
<#
Checks that the compositor draws what upstream's renderer draws (lbterm/COMPOSITOR.md, "Checking it").

  pwsh -File lbterm\motion\compare.ps1 [-Bin <build>]

Opens target\release\lbterm-gui.exe (or -Bin's) twice, looking like Lachlan's LBTerm
(New-TestConfig), with lbterm_compositor on and off (motion off, so layouts land at once), splits
both into the same three panes, checks from their state files that both have them in the same
places with the same one active, captures each client area with PrintWindow(PW_RENDERFULLCONTENT),
and compares them ([LbtermE2E.Px]::Compare) into target\motion\compare: compositor.png,
upstream.png, and diff.png (the compositor's at half, soft pixels yellow, others red).

The two draw the panes without the keyboard differently on purpose (lbterm/DESIGN.md, "Panes"):
with the compositor such a pane is its visual at 85% (`inactive`) over the window background, so
its card keeps the surface colour and its text goes to 85%; without it upstream's
inactive_pane_hsb dims card and text alike in linear light (by default its brightness 0.8, the card
#121212 to #0F0F0F, white text to 231). Until 2026-10-30 that difference was 642,985 of the 649,419
pixels this found, burying anything else. So the window without the compositor is given an
inactive_pane_hsb that changes nothing, and its panes without the keyboard are shown as DWM shows
them with it, at 85% over the window background (read in the gutter between the first two panes),
before the two are compared.

What may still differ is the soft pixels at the edges of one visual lying over another: the
active pane's outline (its own visual over the pane) and the tab panel's rail (its ring, the open
tab's ring and bar, each its own visual). DWM blends a visual's partly covered pixel over the one
under it in the stored, gamma-encoded values; the GPU without the compositor blends it in linear
light, so the same coverage comes out a few levels brighter with the compositor. Such a pixel lies
between two neighbours both shots agree on (or one step round a rounded corner), its colour in
both within theirs, and the compositor's is what DWM's blend makes of the coverage the plain
shot's says, within 4 levels a channel ([LbtermE2E.Px]::BlendError): an edge a pixel wider or
narrower in one shot isn't (until 2026-10-30 being between its neighbours was enough, and a line
a pixel wider passed as 30 soft pixels). Any other pixel differing, a soft one anywhere but those
edges, or one more than 70 levels apart summed (59 was the most measured), fails it:
something drawn only one way, moved, recoloured or cut off.
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
# On the right-most screen, without taking focus: the harness's test windows
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $repo 'target\motion\compare'
$T = Start-Test 'motion-compare' -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.compare'

# lbterm/DESIGN.md's `inactive`: a pane without the keyboard at 85% (lbterm_panel::palette::INACTIVE)
$Inactive = 0.85
# A pixel differs when its channels' differences add up to more than this
$Tolerance = 6
# A soft pixel is what DWM's blend makes of the plain shot's coverage within this many levels a
# channel (4 was the most on 2026-10-30, at the open tab's red ring), and no further apart than
# this summed (59 was)
$BlendSlack = 4
$MaxSoft = 70

function Open([bool]$compositor) {
  $on = if ($compositor) { 'true' } else { 'false' }
  $set = @("lbterm_compositor=$on", "lbterm_motion={enabled='off'}")
  # Every pane drawn undimmed, as each pane's visual is drawn with the compositor
  if (-not $compositor) { $set += 'inactive_pane_hsb={hue=1.0,saturation=1.0,brightness=1.0}' }
  Open-TestWindow -WorkingDirectory $env:USERPROFILE -TimeoutSec 10 -Set $set
}

# Three panes, each split settled as the window's state file says (lbterm/STATE.md)
function Layout($w) {
  Use-TestWindow $w
  Wait-Change { param($s) $s.frame -gt 0 -and @($s.panes).Count -eq 1 } -What 'the window' | Out-Null
  $right = Invoke-Cli split-pane --pane-id 0 --right --percent 40
  Wait-Change { param($s) @($s.panes).Count -eq 2 } -What 'the first split' | Out-Null
  Invoke-Cli split-pane --pane-id $right --bottom --percent 30 | Out-Null
  Wait-Change { param($s) @($s.panes).Count -eq 3 } -What 'the second split' | Out-Null
}

# A pane's place and whether it has the keyboard, to set the two windows side by side
function Format-Pane($p) { "pane $($p.id) at $($p.rect.x),$($p.rect.y) $($p.rect.w)x$($p.rect.h)$(if ($p.active) { ', active' })" }

# Where a card's outline and its soft pixels lie: bands $By either side of its edges, and its
# corners, round which the outline curves $Corner in (the card's radius, lbterm/DESIGN.md's 12 px)
function Get-EdgeBands($r, [int]$By, [int]$Corner) {
  $c = $Corner + $By
  @(
    (New-Rect ($r.x - $By) ($r.y - $By) ($r.w + 2 * $By) (2 * $By)),
    (New-Rect ($r.x - $By) ($r.y + $r.h - $By) ($r.w + 2 * $By) (2 * $By)),
    (New-Rect ($r.x - $By) ($r.y - $By) (2 * $By) ($r.h + 2 * $By)),
    (New-Rect ($r.x + $r.w - $By) ($r.y - $By) (2 * $By) ($r.h + 2 * $By)),
    (New-Rect ($r.x - $By) ($r.y - $By) $c $c),
    (New-Rect ($r.x + $r.w + $By - $c) ($r.y - $By) $c $c),
    (New-Rect ($r.x - $By) ($r.y + $r.h + $By - $c) $c $c),
    (New-Rect ($r.x + $r.w + $By - $c) ($r.y + $r.h + $By - $c) $c $c)
  )
}

function Format-Colour([int]$c) { '#{0:X6}' -f ($c -band 0xFFFFFF) }

Invoke-Test {
  $comp = Open $true
  $plain = Open $false
  Layout $comp
  Layout $plain
  # The shells' prompts drawn: what they print, which the state file can't say
  $a = Wait-Still $comp -Name 'compositor'
  $b = Wait-Still $plain -Name 'upstream'
  try {
    $sa = Get-StateWindow (Read-State $comp)
    $sb = Get-StateWindow (Read-State $plain)
    # The same layout in both, or a pixel comparison says nothing
    $placesA = @($sa.panes | ForEach-Object { Format-Pane $_ })
    $placesB = @($sb.panes | ForEach-Object { Format-Pane $_ })
    if ("$($sa.size.width)x$($sa.size.height)" -ne "$($sb.size.width)x$($sb.size.height)") {
      throw "the windows are different sizes: $($sa.size.width)x$($sa.size.height) with the compositor, $($sb.size.width)x$($sb.size.height) without"
    }
    if (($placesA -join '; ') -ne ($placesB -join '; ')) {
      throw "the panes aren't in the same places: with the compositor $($placesA -join '; '); without it $($placesB -join '; ')"
    }
    $panes = @($sa.panes)
    $active = @($panes | Where-Object active)
    if ($active.Count -ne 1) { throw "$($active.Count) panes have the keyboard, not one: $($placesA -join '; ')" }
    $dimmed = [System.Drawing.Rectangle[]]@($panes | Where-Object { -not $_.active } | ForEach-Object { ConvertTo-Rect $_.rect })
    # The window background, in the gutter between the left pane and the right ones
    $left = $panes | Where-Object id -eq 0
    $right = $panes | Where-Object { $_.rect.x -gt $left.rect.x } | Select-Object -First 1
    $gx = [int][Math]::Floor(($left.rect.x + $left.rect.w + $right.rect.x) / 2)
    $gy = [int]($right.rect.y + $right.rect.h / 2)
    $under = $b.GetPixel($gx, $gy).ToArgb()
    if ($a.GetPixel($gx, $gy).ToArgb() -ne $under) {
      throw "the gutter at $gx,$gy isn't the same colour in both: $(Format-Colour $a.GetPixel($gx, $gy).ToArgb()) with the compositor, $(Format-Colour $under) without"
    }
    # Where edges of one visual lie over another: the active pane's outline, and the rail
    if (-not $sa.rail.rect) { throw 'the window with the compositor has no tab rail: New-TestConfig is meant to look like Lachlan''s LBTerm' }
    $rail = $sa.rail.rect
    $radius = [int][Math]::Ceiling(12 * $sa.size.dpi / 96)
    $softIn = [System.Drawing.Rectangle[]]@((Get-EdgeBands $active[0].rect 3 $radius) + @(New-Rect ($rail.x - 3) ($rail.y - 3) ($rail.w + 6) ($rail.h + 6)))
    $raw = [LbtermE2E.Px]::Compare($a, $b, @(), $Inactive, $under, $Tolerance, $softIn, $BlendSlack)
    $c = [LbtermE2E.Px]::Compare($a, $b, $dimmed, $Inactive, $under, $Tolerance, $softIn, $BlendSlack)
    $diff = Join-Path $out 'diff.png'
    $c.Marked.Save($diff); $c.Marked.Dispose(); $raw.Marked.Dispose()
    Add-Capture $diff
    $all = $c.Width * $c.Height
    "the panes: $($placesA -join '; '); the window background $(Format-Colour $under)"
    "with the panes without the keyboard as each window draws them: $($raw.Differing) of $all pixels differ"
    "with the $($dimmed.Count) panes without the keyboard shown at $([int]($Inactive * 100))% over the window background ($($c.Dimmed) pixels): $($c.Differing) differ"
    "  soft, at edges DWM blends in stored values: $($c.Soft) ($($c.SoftArc) round corners), at most $($c.MaxSoft) levels apart, $($c.SoftBrighter) brighter with the compositor, each within $('{0:N1}' -f $c.MaxBlendError) levels a channel of DWM's blend"
    "  other: $($c.Other)$(if ($c.SoftOutside) { " ($($c.SoftOutside) of them soft, but away from the outline and the rail)" })"
    if ($c.MaxSoft -gt $MaxSoft) { Add-Failure "a soft pixel is $($c.MaxSoft) levels apart summed, more than the $MaxSoft an edge's blend has come to" }
    if ($c.Other) {
      foreach ($p in $c.OtherAt) { "    ($($p[0]), $($p[1])): $(Format-Colour $p[2]) with the compositor, $(Format-Colour $p[3]) without" }
      Add-Failure "$($c.Other) pixels differ that aren't soft pixels at an edge of a visual over another, within x $($c.OtherBox[0])..$($c.OtherBox[2]), y $($c.OtherBox[1])..$($c.OtherBox[3]): the two draw something differently (diff.png, in red)"
    }
  } finally {
    $a.Dispose(); $b.Dispose()
  }
} -Passed 'compare: the compositor draws what upstream draws, but for its soft pixels at the active outline and the rail'
