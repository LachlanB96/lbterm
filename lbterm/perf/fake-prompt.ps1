#Requires -Version 7
# Helper: the stand-in Claude Code draft.ps1 runs in its pane
<#
A stand-in for Claude Code for perf\draft.ps1: a reply asking two questions, on the alternate
screen, and under it what the file -Mode names: `prompt` (the input box between its two rules,
and the status line), `permission` (a permission prompt in its place), `picker` (an
AskUserQuestion picker at its first question) or `newsession` (the prompt, with another Claude
session in the pane). It draws again when the file changes, sets the pane's claude_session user
var as claudehook.exe does, and writes each key it's sent to -Keys, one a line: the character's
code, with `M-` before it when Alt came with it (ESC then CR can arrive as Alt+Enter). With
-TickMs, its status line under the prompt turns that often, a count going up, as Claude Code's
does while it works (perf/budgets.ps1's busy-draft-own).

  pwsh -NoProfile -File fake-prompt.ps1 -Sid <session id> -Mode <file> -Keys <file> [-TickMs 50]
#>
param(
  [Parameter(Mandatory)][string]$Sid,
  [Parameter(Mandatory)][string]$Mode,
  [Parameter(Mandatory)][string]$Keys,
  [int]$TickMs = 0
)
$ErrorActionPreference = 'Stop'
$e = [char]27
$arrow = [char]0x276f

$reply = @(
  "$([char]0x25cf) Two things before I land it.",
  '',
  '  Q1. Should the rail show labels for try windows too?',
  '  - 1a. Yes, the same way',
  '  - 1b. No',
  '',
  '  Q2. Remove the two old background agents with claude rm?'
)

function Set-Session([string]$id) {
  [Console]::Write("$e]1337;SetUserVar=claude_session=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($id)))$([char]7)")
}

function Draw([string]$what) {
  $h = [Console]::WindowHeight
  $w = [Console]::WindowWidth
  $rule = ([string][char]0x2500) * $w
  # The prompt's upper rule as Claude Code 2.1.314 on draws a named conversation's (every one of
  # Lachlan's): the name near its right end, one of the line after it
  $title = ' Draft Check '
  $named = ([string][char]0x2500) * ($w - $title.Length - 1) + $title + [char]0x2500
  $below = switch ($what) {
    'permission' {
      @($rule, ' Bash command', '   rm -rf build', ' Do you want to proceed?', " $arrow 1. Yes", '   2. No, and tell Claude what to do differently (esc)')
    }
    'picker' {
      @($rule, " $([char]0x2190)  $([char]0x2610) Colour  $([char]0x2610) Ring  $([char]0x2714) Submit  $([char]0x2192)", '',
        'Which colour should the pad take?', '', "$arrow 1. Teal", '  2. Amber', '  3. Type something.', $rule,
        '  4. Chat about this', '', 'Enter to select · Tab/Arrow keys to navigate · Esc to cancel')
    }
    default { @($named, "$arrow ", $rule, '  [fake] LBTerm | main') }
  }
  $sb = [Text.StringBuilder]::new()
  [void]$sb.Append("$e[0m$e[2J")
  for ($r = 0; $r -lt $reply.Count; $r++) { [void]$sb.Append("$e[$($r + 1);1H$($reply[$r])") }
  $top = $h - $below.Count + 1
  for ($i = 0; $i -lt $below.Count; $i++) { [void]$sb.Append("$e[$($top + $i);1H$e[2K$($below[$i])") }
  [Console]::Write($sb.ToString())
}

[Console]::OutputEncoding = [Text.Encoding]::UTF8
[Console]::Write("$e[?1049h$e[?25l")
Set-Session $Sid
$shown = $null
$tick = 0
$clock = [Diagnostics.Stopwatch]::StartNew()
while ($true) {
  $now = if (Test-Path $Mode) { (Get-Content $Mode -Raw).Trim() } else { 'prompt' }
  if ($now -ne $shown) {
    if ($now -eq 'newsession') { Set-Session "$Sid-next" }
    Draw $now
    $shown = $now
  }
  while ([Console]::KeyAvailable) {
    $k = [Console]::ReadKey($true)
    $alt = ($k.Modifiers -band [ConsoleModifiers]::Alt) -ne 0
    Add-Content -LiteralPath $Keys -Value ($(if ($alt) { 'M-' } else { '' }) + [int]$k.KeyChar)
  }
  if ($TickMs -le 0) { Start-Sleep -Milliseconds 40; continue }
  # The status line, the screen's last row under the prompt, turning
  if ($shown -eq 'prompt' -and $clock.ElapsedMilliseconds -ge ($tick + 1) * $TickMs) {
    $tick++
    [Console]::Write("$e[$([Console]::WindowHeight);1H$e[2K  [fake] LBTerm | main | working $tick")
  }
  Start-Sleep -Milliseconds ([Math]::Max(1, [Math]::Min(40, ($tick + 1) * $TickMs - $clock.ElapsedMilliseconds)))
}
