#Requires -Version 7
# Helper: the stand-in Claude Code reply questions.ps1 runs in its pane
<#
A stand-in for Claude Code's fullscreen renderer showing one reply that asks two questions, for
perf\questions.ps1. It draws on the alternate screen with mouse reporting on, as Claude Code
does: the reply's mark, its text two columns in, then the input box between two rules and a
status line. It scrolls the reply itself, PgUp and PgDn half a screen, Home to the top, End to
the bottom, and sets the pane's claude_session user var as claudehook.exe does.

  pwsh -NoProfile -File fake-reply.ps1 -Sid <session id>
#>
param([Parameter(Mandatory)][string]$Sid)
$ErrorActionPreference = 'Stop'
$e = [char]27

# The reply as Claude Code draws it, its markdown gone, wrapped under 100 columns: the findings
# the questions are about at the top, notes, then the questions at the bottom
$lines = [Collections.Generic.List[string]]::new()
$lines.Add("$([char]0x25cf) Here's what I found in the fleet and the rail.")
$lines.Add('')
$lines.Add('  - The fleet still lists two old background agents that aren''t running: "Keep going"')
$lines.Add('    (lamp-sketch, 16 days old) and "daily task planner" (162 days old).')
$lines.Add('  - The tab rail''s labels come from lbterm_summaries, read on a thread of its own, so a')
$lines.Add('    try window''s labels are the daily LBTerm''s.')
$lines.Add('  - The check passes on main, with the new tests in it.')
$lines.Add('')
for ($n = 1; $n -le 12; $n++) { $lines.Add("  Note $n`: more of the work, to fill the screen so that it scrolls.") }
$lines.Add('')
$lines.Add('  Questions')
$lines.Add('')
$lines.Add('  Q1. Should the rail show labels for try windows too?')
$lines.Add('  - 1a. Yes, the same way')
$lines.Add('  - 1b. No')
$lines.Add('')
$lines.Add('  Q2. Remove the two old background agents with claude rm? That deletes them, so I''ll only do')
$lines.Add('  it on your yes.')

# Rows scrolled up from the bottom
$offset = 0

function Draw {
  $h = [Console]::WindowHeight
  $w = [Console]::WindowWidth
  $view = [Math]::Max(1, $h - 4)
  $max = [Math]::Max(0, $lines.Count - $view)
  $script:offset = [Math]::Min([Math]::Max(0, $script:offset), $max)
  $first = [Math]::Max(0, $lines.Count - $view - $script:offset)
  $sb = [Text.StringBuilder]::new()
  for ($r = 0; $r -lt $view; $r++) {
    $i = $first + $r
    [void]$sb.Append("$e[$($r + 1);1H$e[0m$e[2K")
    if ($i -lt $lines.Count) {
      $t = $lines[$i]
      if ($t.Length -gt $w - 1) { $t = $t.Substring(0, $w - 1) }
      [void]$sb.Append($t)
    }
  }
  $rule = ([string][char]0x2500) * $w
  [void]$sb.Append("$e[$($h - 3);1H$e[2K$e[38;2;90;90;90m$rule$e[0m")
  [void]$sb.Append("$e[$($h - 2);1H$e[2K$([char]0x276f) ")
  [void]$sb.Append("$e[$($h - 1);1H$e[2K$e[38;2;90;90;90m$rule$e[0m")
  [void]$sb.Append("$e[$h;1H$e[2K  [fake] Projects | main")
  [Console]::Write($sb.ToString())
}

[Console]::OutputEncoding = [Text.Encoding]::UTF8
# The alternate screen, no cursor, and the mouse reported as Claude Code asks for it
[Console]::Write("$e[?1049h$e[?25l$e[?1000h$e[?1002h$e[?1006h")
[Console]::Write("$e]1337;SetUserVar=claude_session=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Sid)))$([char]7)")
Draw
while ($true) {
  $k = [Console]::ReadKey($true)
  $half = [Math]::Max(1, [int](([Console]::WindowHeight - 4) / 2))
  switch ($k.Key) {
    'PageUp' { $offset += $half }
    'PageDown' { $offset -= $half }
    'Home' { $offset = $lines.Count }
    'End' { $offset = 0 }
    'Q' { [Console]::Write("$e[?1000l$e[?1002l$e[?1006l$e[?1049l$e[?25h"); exit }
  }
  Draw
}
