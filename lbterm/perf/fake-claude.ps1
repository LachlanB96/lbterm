#Requires -Version 7
# Helper: the stand-in Claude Code strip-prompt.ps1 runs in its pane
<#
A stand-in for Claude Code's fullscreen renderer, for perf\strip-prompt.ps1. It draws a
conversation on the alternate screen and scrolls it itself, as Claude Code does: PgUp and PgDn
half a screen, Ctrl+End back to the latest. Each message sent is on Claude Code's grey band after
"❯ ", each answer below it, and the input box and status sit at the bottom. While scrolled up, a
dim header at the top shows the last message scrolled out of view (this one doesn't jump when it's
clicked, so a jump has to carry on without it). It sets the pane's claude_session and
claude_prompt user vars as claudehook.exe does.

  pwsh -NoProfile -File fake-claude.ps1 -Sid <session id> [-Messages 12] [-Long <last message>]
#>
param(
  [Parameter(Mandatory)][string]$Sid,
  [int]$Messages = 12,
  [string]$Long
)
$ErrorActionPreference = 'Stop'
$e = [char]27

function UserVar([string]$name, [string]$value) {
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($value))
  [Console]::Write("$e]1337;SetUserVar=$name=$b64$([char]7)")
}

# The conversation: each message, then fourteen lines of answer and a blank
$lines = [Collections.Generic.List[object]]::new()
for ($m = 1; $m -le $Messages; $m++) {
  $text = if ($m -eq $Messages -and $Long) { $Long } else { "message number $m, about part $m of the work" }
  $lines.Add([pscustomobject]@{ User = $true; Text = $text })
  for ($j = 1; $j -le 14; $j++) { $lines.Add([pscustomobject]@{ User = $false; Text = "$([char]0x25cf) answer $m, line $j" }) }
  $lines.Add([pscustomobject]@{ User = $false; Text = '' })
}
$last = if ($Long) { $Long } else { "message number $Messages, about part $Messages of the work" }

# Rows scrolled up from the latest
$offset = 0

function Draw {
  $h = [Console]::WindowHeight
  $w = [Console]::WindowWidth
  $view = [Math]::Max(1, $h - 3)
  $max = [Math]::Max(0, $lines.Count - $view)
  $script:offset = [Math]::Min([Math]::Max(0, $script:offset), $max)
  $first = $lines.Count - $view - $script:offset
  $sb = [Text.StringBuilder]::new()
  [void]$sb.Append("$e[H")
  for ($r = 0; $r -lt $view; $r++) {
    $i = $first + $r
    [void]$sb.Append("$e[$($r + 1);1H$e[0m$e[2K")
    if ($r -eq 0 -and $script:offset -gt 0) {
      # The header: the last message above the view, dim, no band
      $above = $null
      for ($k = $first - 1; $k -ge 0; $k--) { if ($lines[$k].User) { $above = $lines[$k]; break } }
      if ($above) {
        $t = "$([char]0x276f) $($above.Text)"
        if ($t.Length -gt $w - 1) { $t = $t.Substring(0, $w - 1) }
        [void]$sb.Append("$e[38;2;120;120;120m$t$e[0m")
        continue
      }
    }
    if ($i -lt 0 -or $i -ge $lines.Count) { continue }
    $line = $lines[$i]
    if ($line.User) {
      $t = $line.Text
      if ($t.Length -gt $w - 3) { $t = $t.Substring(0, $w - 3) }
      [void]$sb.Append("$e[38;2;153;153;153m$e[48;2;55;55;55m$([char]0x276f) $e[38;2;255;255;255m$t$e[K$e[0m")
    } else {
      $t = $line.Text
      if ($t.Length -gt $w - 1) { $t = $t.Substring(0, $w - 1) }
      [void]$sb.Append($t)
    }
  }
  [void]$sb.Append("$e[$($h - 1);1H$e[0m$e[2K$e[38;2;153;153;153m$([char]0x276f) $e[0m")
  [void]$sb.Append("$e[$h;1H$e[2K  [fake] Projects | main")
  [Console]::Write($sb.ToString())
}

[Console]::OutputEncoding = [Text.Encoding]::UTF8
[Console]::Write("$e[?1049h$e[?25l")
UserVar 'claude_session' $Sid
UserVar 'claude_prompt' $last
Draw
while ($true) {
  $k = [Console]::ReadKey($true)
  $half = [Math]::Max(1, [int](([Console]::WindowHeight - 3) / 2))
  switch ($k.Key) {
    'PageUp' { $offset += $half }
    'PageDown' { $offset -= $half }
    'End' { if ($k.Modifiers -band [ConsoleModifiers]::Control) { $offset = 0 } }
    'Q' { [Console]::Write("$e[?1049l$e[?25h"); exit }
  }
  Draw
}
