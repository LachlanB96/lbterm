#Requires -Version 7
# Helper: the stand-in Claude Code strip-prompt.ps1, scroll-strip.ps1 and conversation-cost.ps1 run in their pane
<#
A stand-in for Claude Code's fullscreen view, drawn the way Claude Code 2.1.312 draws it, as
measured in a test window on 2026-10-28 (lbterm/SCROLL-STRIP.md, "What Claude Code draws"); the
real screens are lbterm-extras' fixtures/claude-2.1.312. On the alternate screen:

- the banner at the top: a blank row, three of logo, two blank;
- each message sent on the grey band after "❯ ", white; its answer after "● " on the first line
  and two spaces on the rest; "✻ Worked for 12s · done 09:00" after it; a blank row between;
- at the bottom, eight rows: the effort line, the input box between two rules, a three-line
  status and the mode line. The conversation is the rows above them;
- PgUp and PgDn scroll half the conversation's rows, Ctrl+Home to the top, Ctrl+End to the
  latest. While scrolled up, the sticky header covers the first row, naming the last message
  above the view on the band in dim grey (when no message starts the view), and "Jump to bottom
  (ctrl+End) ↓" covers the middle of the last.

It sets the pane's claude_session and claude_prompt user vars as claudehook.exe does.

With -Reads, each message's answer comes after a run of that many file reads, which Claude Code
folds into one line ("Read 5 files"), as lbterm-extras' long stand-in transcript has them
(conversation/long.rs). W starts and stops Claude Code working: a spinner on the effort line over
the input box, drawn every 100 ms, which changes nothing of the conversation's rows.

  pwsh -NoProfile -File fake-claude.ps1 -Sid <session id> [-Messages 12] [-Long <last message>] [-Reads 5]
#>
param(
  [Parameter(Mandatory)][string]$Sid,
  [int]$Messages = 12,
  [string]$Long,
  [int]$Reads = 0
)
$ErrorActionPreference = 'Stop'
$e = [char]27

function UserVar([string]$name, [string]$value) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($value))
  [Console]::Write("$e]1337;SetUserVar=$name=$b64$([char]7)")
}

# The conversation: Kind is banner, user, first (an answer's first line), text, turn or blank
$lines = [Collections.Generic.List[object]]::new()
foreach ($t in '', ' LOGO   Claude Code (stand-in)', ' LOGO   Haiku', ' LOGO   ~\work\demo', '') { $lines.Add([pscustomobject]@{ Kind = 'banner'; Text = $t }) }
for ($m = 1; $m -le $Messages; $m++) {
  $text = if ($m -eq $Messages -and $Long) { $Long } else { "message number $m, about part $m of the work" }
  $lines.Add([pscustomobject]@{ Kind = 'blank'; Text = '' })
  $lines.Add([pscustomobject]@{ Kind = 'user'; Text = $text })
  $lines.Add([pscustomobject]@{ Kind = 'blank'; Text = '' })
  if ($Reads -gt 0) {
    $lines.Add([pscustomobject]@{ Kind = 'text'; Text = "Read $Reads files" })
    $lines.Add([pscustomobject]@{ Kind = 'blank'; Text = '' })
  }
  for ($j = 1; $j -le 14; $j++) { $lines.Add([pscustomobject]@{ Kind = $(if ($j -eq 1) { 'first' } else { 'text' }); Text = "answer $m, line $j" }) }
  $lines.Add([pscustomobject]@{ Kind = 'blank'; Text = '' })
  $lines.Add([pscustomobject]@{ Kind = 'turn'; Text = "Worked for 12s $([char]0xb7) done 09:00" })
}
$last = if ($Long) { $Long } else { "message number $Messages, about part $Messages of the work" }
$bottomRows = 8

# Rows scrolled up from the latest
$offset = 0

function Draw {
  $h = [Console]::WindowHeight
  $w = [Console]::WindowWidth
  $area = [Math]::Max(1, $h - $bottomRows)
  $max = [Math]::Max(0, $lines.Count - $area)
  $script:offset = [Math]::Min([Math]::Max(0, $script:offset), $max)
  $first = $lines.Count - $area - $script:offset
  if ($first -lt 0) { $first = 0 }
  $scrolled = $script:offset -gt 0
  $sb = [Text.StringBuilder]::new()
  [void]$sb.Append("$e[H")
  $cut = { param($t, $n) if ($t.Length -gt $n) { $t.Substring(0, $n) } else { $t } }
  for ($r = 0; $r -lt $area; $r++) {
    $i = $first + $r
    [void]$sb.Append("$e[$($r + 1);1H$e[0m$e[2K")
    if ($r -eq 0 -and $scrolled -and $i -lt $lines.Count -and $lines[$i].Kind -ne 'user') {
      # The sticky header: the last message above the view, dim on the band
      $above = $null
      for ($k = $i - 1; $k -ge 0; $k--) { if ($lines[$k].Kind -eq 'user') { $above = $lines[$k]; break } }
      if ($above) {
        [void]$sb.Append("$e[38;2;80;80;80m$e[48;2;55;55;55m$([char]0x276f) $(& $cut $above.Text ($w - 3))$e[K$e[0m")
        continue
      }
    }
    if ($r -eq $area - 1 -and $scrolled) {
      $jump = " Jump to bottom (ctrl+End) $([char]0x2193) "
      $left = if ($i -lt $lines.Count) { & $cut ("  " + $lines[$i].Text) ([Math]::Max(0, [int](($w - $jump.Length) / 2))) } else { '' }
      [void]$sb.Append($left.PadRight([Math]::Max(0, [int](($w - $jump.Length) / 2))))
      [void]$sb.Append("$e[38;2;255;255;255m$e[48;2;55;55;55m$jump$e[0m")
      continue
    }
    if ($i -ge $lines.Count) { continue }
    $line = $lines[$i]
    switch ($line.Kind) {
      'user' { [void]$sb.Append("$e[48;2;55;55;55m$([char]0x276f) $e[38;2;255;255;255m$(& $cut $line.Text ($w - 3))$e[39m$e[K$e[0m") }
      'first' { [void]$sb.Append("$e[38;2;255;255;255m$([char]0x25cf)$e[39m $(& $cut $line.Text ($w - 3))") }
      'text' { [void]$sb.Append("  $(& $cut $line.Text ($w - 3))") }
      'turn' { [void]$sb.Append("$e[38;2;153;153;153m$([char]0x273b) $(& $cut $line.Text ($w - 3))$e[0m") }
      default { [void]$sb.Append((& $cut $line.Text ($w - 1))) }
    }
  }
  $rule = [string][char]0x2500 * $w
  $row = $area + 1
  [void]$sb.Append("$e[$row;1H$e[0m$e[2K"); $row++
  [void]$sb.Append("$e[$row;1H$e[0m$e[2K$e[38;2;136;136;136m$rule$e[0m"); $row++
  [void]$sb.Append("$e[$row;1H$e[0m$e[2K$([char]0x276f) "); $row++
  [void]$sb.Append("$e[$row;1H$e[0m$e[2K$e[38;2;136;136;136m$rule$e[0m"); $row++
  foreach ($t in "  $([char]0x25b8) a summary of the session", '  haiku | demo | main', '  toggles') { [void]$sb.Append("$e[$row;1H$e[0m$e[2K$e[38;2;153;153;153m$t$e[0m"); $row++ }
  [void]$sb.Append("$e[$row;1H$e[0m$e[2K  accept edits on")
  [Console]::Write($sb.ToString())
}

# Claude Code working: its spinner and time on the effort line, the row over the input box's rules
$working = $false
$spun = 0
function Spin {
  $script:spun++
  $glyph = @([char]0x00b7, [char]0x2722, [char]0x2733, [char]0x2736, [char]0x273b, [char]0x273d)[$script:spun % 6]
  $row = [Math]::Max(1, [Console]::WindowHeight - $bottomRows) + 1
  [Console]::Write("$e[$row;1H$e[2K$e[38;2;215;119;87m$glyph$e[0m Working$([char]0x2026) ($([int]($script:spun / 10))s $([char]0xb7) esc to interrupt)")
}

[Console]::OutputEncoding = [Text.Encoding]::UTF8
[Console]::Write("$e[?1049h$e[?25l")
UserVar 'claude_session' $Sid
UserVar 'claude_prompt' $last
Draw
while ($true) {
  while ($working -and -not [Console]::KeyAvailable) {
    Start-Sleep -Milliseconds 100
    Spin
  }
  $k = [Console]::ReadKey($true)
  if ($k.Key -eq 'W') {
    $working = -not $working
    Draw
    continue
  }
  $half = [Math]::Max(1, [int](([Console]::WindowHeight - $bottomRows) / 2))
  switch ($k.Key) {
    'PageUp' { $offset += $half }
    'PageDown' { $offset -= $half }
    'End' { if ($k.Modifiers -band [ConsoleModifiers]::Control) { $offset = 0 } }
    'Home' { if ($k.Modifiers -band [ConsoleModifiers]::Control) { $offset = $lines.Count } }
    'Q' { [Console]::Write("$e[?1049l$e[?25h"); exit }
  }
  Draw
}
