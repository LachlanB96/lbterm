#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/screen_line.rs, wezterm-gui/src/termwindow/render/mod.rs, wezterm-gui/src/termwindow/render/triggers.rs, wezterm-gui/src/termwindow/render/thumbnails.rs, wezterm-gui/src/termwindow/compositor.rs, lbterm-extras/src/link_looks.rs, lbterm-triggers/**
<#
What LBTerm's own looks at the lines a pane draws cost (lbterm/PERF.md, "What a line costs"),
from the state file's count of lines laid out again (work.lines), its frames, the GUI's log and
metrics: nothing here rests on a capture, so it runs with the PC locked. Three test windows that
look like Lachlan's LBTerm (New-TestConfig: WebGpu with the compositor, the easter eggs' triggers),
their tabs the GUI's own, keys and the pointer posted (LBTERM_POSTED_POINTER, so a move hovers).

1. Links. A pane of 20 lines, each holding a link printed as OSC 8, and a key counter under them.
   Keys typed with the pointer on no link, then on one of the links, then off it again: each
   key's frame lays out its counter's line and nothing else, and the pointer coming onto the link
   or going off it lays out the line holding it and nothing else. Until 2026-10-31 every line
   holding a link was laid out again on every frame unless the pointer was on a link, and all of
   them each time it came onto one or went.
2. Triggers. A Claude Code-like fullscreen view of lines holding the easter eggs' rainbow match,
   two to a line at columns that differ from line to line, scrolled a line for each key, with an
   lbterm-trigger handler in its config logging each new match. The first screen's matches are
   new; after that each key's scroll brings one new line, and its two matches are all that's new:
   a line moved a row with its text isn't new, and a new line whose match is at a column another
   line's was in the same row is (-Scrolls goes past every column the lines' matches take). Then a line with "love" brings a burst of
   hearts, which DWM plays: the burst paints a few frames, not one every refresh for two seconds.
   Then the window rests 11 s, longer than a match is remembered once it's gone, and a key that
   changes another line brings no burst and no new match for the love still on the screen
   (until 2026-10-31 a match was remembered for 10 s from the last frame that drew it). Last, the
   love line printed again in another row while the first is on the screen: one new match, and its
   hearts.
3. The same view in a window with no lbterm-trigger handler (Lachlan's config has none): the
   scrolling's new matches run no Lua event's task (metrics' tasks by site).

  pwsh -File lbterm\perf\line-work.ps1 [-Bin <build>]
#>
param([string]$Bin, [int]$Keys = 10, [int]$Scrolls = 24)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'line-work' -Bin $Bin

$LinksProgram = @'
$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::Write("$e[2J")
for ($n = 1; $n -le 20; $n++) {
  [Console]::Write("$e[$($n + 1);3H$e]8;;file:///C:/work/src/file$n.rs$e\src/file$n.rs$e]8;;$e\ is where part $n is read")
}
$i = 0
while ($true) { [void][Console]::ReadKey($true); $i++; [Console]::Write("$e[24;3H$e[2Kkeys $i") }
'@

# Rows 0 to 21 of a conversation of 200 lines, each with LB twice at columns of its own; a key
# scrolls it a line, `l` puts a line with love in the last row, `x` counts in the first row, `d`
# prints the love line again in the row above it
$ScrollProgram = @'
$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$lines = @(1..200 | ForEach-Object { '{0}{1}. LBTerm line, then LBTERM_TRY for line {1}' -f ''.PadLeft($_ % 11 + 2), $_ })
[Console]::Write("$e[?1049h$e[?25l")
$top = 0; $x = 0; $love = $false
function Draw {
  $sb = [Text.StringBuilder]::new()
  for ($r = 0; $r -lt 20; $r++) { [void]$sb.Append("$e[$($r + 2);1H$e[2K").Append($lines[($top + $r) % $lines.Count]) }
  [void]$sb.Append("$e[23;1H$e[2K").Append($(if ($love) { '  I love this terminal' } else { '' }))
  [Console]::Write($sb.ToString())
}
Draw
while ($true) {
  $k = [Console]::ReadKey($true)
  switch -CaseSensitive ($k.KeyChar) {
    'l' { $love = $true; Draw }
    'x' { $x++; [Console]::Write("$e[1;1H$e[2K  counter $x") }
    'd' { [Console]::Write("$e[22;1H$e[2K  I love this terminal") }
    default { $top++; Draw }
  }
}
'@

$Handler = @"
wezterm.on('lbterm-trigger', function(window, pane, name, text)
  wezterm.log_info('line-work trigger ' .. name .. ' ' .. text)
end)
"@

function Get-Counts($w) {
  $s = Read-State -Window $w
  [pscustomobject]@{ Lines = [long]$s.work.lines; Frames = [long]$s.frame }
}

# Each key typed, its frame waited for, then the window settled: the lines and frames it brought
function Send-Keys($w, [string[]]$Chars) {
  $before = Get-Counts $w
  foreach ($c in $Chars) {
    $f = (Get-Counts $w).Frames
    Send-Key $w $c
    Wait-State { param($s) $true } -Window $w -AfterFrame $f -What "the frame after key $c" | Out-Null
  }
  Wait-Settled -Window $w | Out-Null
  $after = Get-Counts $w
  [pscustomobject]@{ Lines = $after.Lines - $before.Lines; Frames = $after.Frames - $before.Frames }
}

# The pointer posted to (x, y), and the window settled after the frame it brought, if any
function Move-Pointer($w, [int]$X, [int]$Y) {
  $before = Get-Counts $w
  Send-Mouse $w Move $X $Y
  Start-Sleep -Milliseconds 300
  Wait-Settled -Window $w | Out-Null
  $after = Get-Counts $w
  [pscustomobject]@{ Lines = $after.Lines - $before.Lines; Frames = $after.Frames - $before.Frames }
}

function Get-TriggerLines($w) { @(Get-LogLines $w 'line-work trigger ').Count }

Invoke-Test {
  $notes = [Collections.Generic.List[string]]::new()
  $base = "config.initial_cols = 110`nconfig.initial_rows = 26`nconfig.cursor_blink_rate = 0"

  # 1. Links
  $w = Open-TestWindow -PostedPointer -Lua $base -Program (New-PwshProgram $LinksProgram)
  Wait-State { param($s) @($s.panes).Count -eq 1 -and @($s.panes)[0].cols -gt 0 } -Window $w -What 'the links pane' | Out-Null
  Send-Focus $w
  Start-Sleep -Milliseconds 800
  Wait-Settled -Window $w | Out-Null
  $p = @((Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)).panes)[0]
  $cw = $p.rect.w / $p.cols; $ch = $p.rect.h / $p.rows
  # Link 5 is on row 5 from column 2; nothing is on row 5 past column 40
  $onX = [int]($p.rect.x + 6.5 * $cw); $offX = [int]($p.rect.x + 80.5 * $cw); $rowY = [int]($p.rect.y + 5.5 * $ch)
  $chars = @(1..$Keys | ForEach-Object { 'X' })
  $off = Send-Keys $w $chars
  $onto = Move-Pointer $w $onX $rowY
  $on = Send-Keys $w $chars
  $away = Move-Pointer $w $offX $rowY
  $again = Send-Keys $w $chars
  $said = "pointer on no link: $($off.Lines) lines in $($off.Frames) frames; onto a link: $($onto.Lines) in $($onto.Frames); on it: $($on.Lines) in $($on.Frames); off it: $($away.Lines) in $($away.Frames); off again: $($again.Lines) in $($again.Frames)"
  $said
  foreach ($run in @(@('on no link', $off), @('on a link', $on), @('on no link again', $again))) {
    $name, $c = $run
    if ($c.Frames -lt $Keys) { Add-Failure "keys with the pointer $name painted $($c.Frames) frames for $Keys keys: they didn't each show" }
    elseif ($c.Lines -gt $c.Frames + 2) { Add-Failure "keys with the pointer $name laid out $($c.Lines) lines in $($c.Frames) frames, where only the counter's line changes" }
  }
  if ($onto.Frames -lt 1) { Add-Failure 'the pointer coming onto a link painted nothing: the link never showed as under it' }
  foreach ($move in @(@('onto the link', $onto), @('off the link', $away))) {
    $name, $c = $move
    if ($c.Lines -gt 2) { Add-Failure "the pointer moving $name laid out $($c.Lines) lines, where only the line holding it looks different" }
  }

  # 2. Triggers, with a handler
  $w2 = Open-TestWindow -PostedPointer -Lua ($base + "`n" + $Handler) -Program (New-PwshProgram $ScrollProgram)
  Wait-State { param($s) @($s.panes).Count -eq 1 } -Window $w2 -What 'the scrolling pane' | Out-Null
  Send-Focus $w2
  if (-not (Wait-Until { (Get-TriggerLines $w2) -ge 40 } -TimeoutSec 15)) { Add-Failure "the first screen's 40 matches brought $(Get-TriggerLines $w2) lbterm-trigger events" }
  Wait-Settled -Window $w2 | Out-Null
  Start-Sleep -Milliseconds 500
  $first = Get-TriggerLines $w2
  $scroll = Send-Keys $w2 @(1..$Scrolls | ForEach-Object { 'J' })
  Start-Sleep -Milliseconds 500
  $events = (Get-TriggerLines $w2) - $first
  "scrolling $Scrolls lines: $events new matches told Lua (2 a line brought in), $($scroll.Lines) lines laid out in $($scroll.Frames) frames; the first screen's: $first"
  if ($first -ne 40) { Add-Failure "the first screen of 20 lines told Lua of $first matches, not 40" }
  if ($events -gt 2 * $Scrolls) { Add-Failure "scrolling $Scrolls lines told Lua of $events new matches, where each scroll brings one line of 2: a line moved a row with its text was taken for new" }
  elseif ($events -lt 2 * $Scrolls) { Add-Failure "scrolling $Scrolls lines told Lua of $events new matches, where each scroll brings one line of 2: a new line's match at a column another had in that row was taken for the same" }

  # Love: a burst of hearts, played by DWM
  $f0 = (Get-Counts $w2).Frames
  Send-Key $w2 l
  $s = Wait-State { param($x) [int]$x.hearts -gt 0 } -Window $w2 -What 'the hearts in the air'
  $burstFrom = (Get-Counts $w2).Frames
  Wait-Settled -Window $w2 -TimeoutSec 10 | Out-Null
  $burst = (Get-Counts $w2).Frames - $f0
  "a burst of hearts: $burst frames from the key to the window at rest"
  if ($burst -gt 5) { Add-Failure "a burst of hearts painted $burst frames: the GUI moved them, where DWM plays them" }
  # At rest longer than a gone match is remembered, then another line changed
  Start-Sleep -Seconds 11
  $loves = @(Get-LogLines $w2 'line-work trigger love').Count
  $f1 = (Get-Counts $w2).Frames
  Send-Key $w2 x
  Wait-State { param($x) $true } -Window $w2 -AfterFrame $f1 -What 'the frame after x' | Out-Null
  Start-Sleep -Milliseconds 300
  $after = Read-State -Window $w2
  $hearts = [int](Get-StateWindow $after -Hwnd ([long]$w2.Handle)).hearts
  $lovesAfter = @(Get-LogLines $w2 'line-work trigger love').Count
  "11 s at rest with love on the screen, then another line: $hearts bursts in the air, $($lovesAfter - $loves) new love matches"
  if ($loves -ne 1) { Add-Failure "the love line told Lua $loves times, not once" }
  if ($hearts -gt 0 -or $lovesAfter -ne $loves) { Add-Failure "a frame after 11 s at rest took the love still on the screen for new: $hearts bursts, $($lovesAfter - $loves) events" }
  # The same love line printed again in another row while the first is still on the screen: a
  # new occurrence, told once, with its hearts (the cooldown long over)
  $f2 = (Get-Counts $w2).Frames
  Send-Key $w2 d
  Wait-State { param($x) $true } -Window $w2 -AfterFrame $f2 -What 'the frame after d' | Out-Null
  Start-Sleep -Milliseconds 300
  $twice = @(Get-LogLines $w2 'line-work trigger love').Count - $lovesAfter
  $hearts2 = [int](Get-StateWindow (Read-State -Window $w2) -Hwnd ([long]$w2.Handle)).hearts
  "the love line printed again in another row: $twice new love matches, $hearts2 bursts in the air"
  if ($twice -ne 1 -or $hearts2 -lt 1) { Add-Failure "the love line printed again while the first was on the screen told Lua $twice times and brought $hearts2 bursts, not once and one" }

  # 3. No handler: no Lua event's task. The others go first: metrics answers on a pipe named by
  # the test's window class, which the first window still open holds
  Stop-TestWindow $w
  Stop-TestWindow $w2
  $w3 = Open-TestWindow -PostedPointer -Lua $base -Program (New-PwshProgram $ScrollProgram)
  Wait-State { param($s) @($s.panes).Count -eq 1 } -Window $w3 -What 'the scrolling pane without a handler' | Out-Null
  Send-Focus $w3
  Start-Sleep -Milliseconds 800
  Wait-Settled -Window $w3 | Out-Null
  Get-Metrics -Window $w3 -Reset | Out-Null
  Send-Keys $w3 @(1..$Scrolls | ForEach-Object { 'J' }) | Out-Null
  $m = Get-Metrics -Window $w3
  $tasks = @($m.tasks | Where-Object { $_ -and ([string]$_.site) -match 'triggers\.rs' })
  $count = ($tasks | Measure-Object count -Sum).Sum
  "without a handler, scrolling $Scrolls lines ran $([int]$count) tasks from render/triggers.rs"
  if ($count -gt 0) { Add-Failure "without an lbterm-trigger handler the scrolling ran $count tasks from render/triggers.rs ($(@($tasks | ForEach-Object site) -join ', ')): a Lua event for nobody" }
} -Passed 'line-work: a key lays out its own line alone whether the pointer is on a link or not, the pointer onto or off a link lays out only the line holding it, a scroll tells Lua only of the line it brings, a burst of hearts paints a few frames, a match on screen through 11 s at rest stays seen, the same line printed again is new, and without a handler no Lua event runs'
