#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/inbox.rs, wezterm-gui/src/termwindow/render/screen_line.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/inbox.rs, lbterm-extras/src/inbox.rs
<#
Links to one InMail message (`lbterm://inmail/<project>?message=<file>`; lbterm/INBOX.md, "Links to a
message") end to end, in a test window that looks like Lachlan's LBTerm (New-TestConfig), its
`lbterm_inbox` folders two projects' InMail folders in target\perf\inbox-links\projects. Its program
prints four lines of the same words: plain, a link (OSC 8) to a message waiting in One's folder, a
link to one that isn't there, and a link to a message for Three, a project whose InMail the window
doesn't look in (one made after the config was read), whose message may well wait.

1. The waiting message's link is in the waiting orange, the missing one's grey with a line through
   it, and neither the plain line nor Three's link has either (links.png).
2. The waiting message archived (moved out): within a couple of seconds no orange is left, and both
   of One's links are struck through, Three's still not (archived.png).
3. A message of the other link's name arriving in One's folder: that link turns orange and loses
   its line (arrived.png).

When the folders have been looked in comes from the InMail button's count in the window's state
file (lbterm/STATE.md), the same look the links are drawn from; the pictures say how they look.
Panes of a window without the keyboard are drawn at 85%, so the colours are looked for in ranges
wide enough for that.

  pwsh -File lbterm\perf\inbox-links.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'inbox-links' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
Remove-Item -Recurse -Force $projects -ErrorAction SilentlyContinue
$folders = foreach ($p in 'One', 'Two') { Join-Path $projects "$p\InMail" }
New-Item -ItemType Directory -Force $folders | Out-Null

function Send-Message([string]$To, [string]$Name) {
  $file = Join-Path $projects "$To\InMail\$Name"
  [IO.File]::WriteAllText($file, "---`nfrom: Two`nto: $To`nsent: 2026-10-23T22:00`nsubject: `"A message`"`n---`n`nThe body.`n")
  $file
}
$waits = Send-Message -To 'One' -Name '2026-10-23-2200-from-Two-waits.md'
$later = '2026-10-23-2201-from-Two-later.md'

# The waiting orange (#FF8A3D), dimmed or not, and the read grey (#717171) a strike is drawn in
$orange = New-ColourRange 180 255 90 160 20 95
$grey = New-ColourRange 80 130 80 130 80 130
$luaFolders = ($folders | ForEach-Object { "[[$_]]" }) -join ', '
$script = @"
`$e = [char]27
function Link([string]`$uri, [string]`$words) { "`$e]8;;`$uri`$e\`$words`$e]8;;`$e\" }
[Console]::WriteLine('')
[Console]::WriteLine('    the same words, over and over')
[Console]::WriteLine('')
[Console]::WriteLine('    ' + (Link 'lbterm://inmail/One?message=$(Split-Path $waits -Leaf)' 'the same words, over and over'))
[Console]::WriteLine('')
[Console]::WriteLine('    ' + (Link 'lbterm://inmail/One?message=$later' 'the same words, over and over'))
[Console]::WriteLine('')
[Console]::WriteLine('    ' + (Link 'lbterm://inmail/Three?message=$later' 'the same words, over and over'))
Start-Sleep 600
"@

# What a capture of the pane shows: orange pixels, and the bands of rows a strike runs across
function Measure-Links($w, $s, [string]$Name) {
  $pane = @($s.panes)[0]
  $box = New-Rect $pane.rect.x $pane.rect.y $pane.rect.w $pane.rect.h
  # A strike runs the words' length; a glyph's stroke, a cell at most
  $cell = $pane.rect.w / [Math]::Max(1, $pane.cols)
  $shot = Get-Shot $w -Name $Name
  $rows = @([LbtermE2E.Px]::RowsWithRun($shot, $box, $grey, [int](12 * $cell)))
  $bands = 0
  for ($i = 0; $i -lt $rows.Count; $i++) { if ($i -eq 0 -or $rows[$i] - $rows[$i - 1] -gt 2) { $bands++ } }
  $found = [pscustomobject]@{ Orange = [LbtermE2E.Px]::CountRange($shot, $box, $orange); Struck = $bands; Rows = $rows -join ',' }
  $shot.Dispose()
  $found
}
function Format-Links([string]$Name, $m) { "${Name}: $($m.Orange) orange pixels, $($m.Struck) struck through (rows $($m.Rows))" }

Invoke-Test {
  $w = Open-TestWindow -Program (New-PwshProgram $script) -Lua @"
rawset(config, 'lbterm_inbox', { folders = { $luaFolders } })
config.initial_cols = 100
config.initial_rows = 14
"@
  # 1. Looked in once, the words printed: one orange, one struck
  $s = Wait-Change { param($s) $s.strip.inbox -eq 1 -and @($s.panes).Count -eq 1 } -What 'the folders looked in'
  Wait-Still $w -Region (New-Rect @($s.panes)[0].rect.x @($s.panes)[0].rect.y @($s.panes)[0].rect.w @($s.panes)[0].rect.h) | Out-Null
  $m = Measure-Links $w $s 'links'
  Format-Links 'links' $m
  if ($m.Orange -lt 30) { Add-Failure "the waiting message's link isn't orange ($($m.Orange) pixels; links.png)" }
  if ($m.Struck -ne 1) { Add-Failure "$($m.Struck) lines struck through, not the one link to a message that isn't there: Three's, for a folder not looked in, mustn't be (links.png)" }

  # 2. Archived: grey, One's both struck, Three's still not
  Remove-Item $waits
  $s = Wait-Change { param($s) $s.strip.inbox -eq 0 } -What 'the button with none' -TimeoutSec 5
  $m = Measure-Links $w $s 'archived'
  Format-Links 'archived' $m
  if ($m.Orange -gt 0) { Add-Failure "orange left once the message was archived ($($m.Orange) pixels; archived.png)" }
  if ($m.Struck -ne 2) { Add-Failure "$($m.Struck) lines struck through once the message was archived, not One's two links (archived.png)" }

  # 3. A message of the other link's name: that one waits now
  Send-Message -To 'One' -Name $later | Out-Null
  $s = Wait-Change { param($s) $s.strip.inbox -eq 1 } -What 'the button saying 1 again' -TimeoutSec 5
  $m = Measure-Links $w $s 'arrived'
  Format-Links 'arrived' $m
  if ($m.Orange -lt 30) { Add-Failure "the link to the message that arrived isn't orange ($($m.Orange) pixels; arrived.png)" }
  if ($m.Struck -ne 1) { Add-Failure "$($m.Struck) lines struck through, not the one to the archived message (arrived.png)" }
} -Passed 'inbox links passed: orange while the message waits, grey and struck through once it''s gone, back when one of its name arrives, and a link to a project not looked in drawn as any link'
