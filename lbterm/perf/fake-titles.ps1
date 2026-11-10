#Requires -Version 7
# Helper: the stand-in title-storm.ps1 runs in each of its tabs
<#
A tab's program that sends what a working Claude Code session sends besides its screen, at its
rates, while the line in its control file says so (read every 20 ms; the script writes it):

- `rest`: nothing.
- `titles`: its title as Claude Code 2.1.312 sets it while it works, `◐ <name>` and `◑ <name>`
  in turn every 960 ms (its bundle's `vee=["◐","◑"]` and `tbt=960`), which ConPTY passes on as
  OSC 0. -PhaseMs puts each tab at another point in the cycle, as sessions started apart are.
- `repeat`: the title it last set (`<name>` if none) again every 100 ms: a program setting a title
  it already has.
- `mix`: `titles`, and every 2.5 s an OSC 7501 report of its own, working, its message turning
  between two, with OSC 9;4;3 (indeterminate progress) beside it.
- `draw`: a spinner line at the foot of its screen redrawn every 100 ms, as Claude Code's is while
  it works: output for a pane that's on screen to paint.
- `title:<text>`: its title set to <text> once (OSC 0).
- `progress:<n>`: OSC 9;4;1;<n> once (`progress:0` takes it off).

Each time the line changes it writes how many titles it has sent so far to <control>.sent, so
the script knows how many came in a phase.

  pwsh -NoProfile -File fake-titles.ps1 -Name <name> -Control <file> [-PhaseMs 0]
#>
param(
  [Parameter(Mandatory)][string]$Name,
  [Parameter(Mandatory)][string]$Control,
  [int]$PhaseMs = 0
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$bel = [char]7
$glyphs = [char]0x25d0, [char]0x25d1
$spinner = [char]0x2722, [char]0x2733, [char]0x2736, [char]0x273b, [char]0x273d
$b64 = { param($s) [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($s)) }
$messages = (& $b64 'Reading the files'), (& $b64 'Editing the code')

# Its folder, as Terminal's claudehook says it with each status change (OSC 7): without one the
# server works the pane's folder out by walking every process on the PC, a walk each time the pane
# has printed since the last (mux/src/localpane.rs), which no Claude tab of Lachlan's does
[Console]::Write("$e]7;file://localhost/$((Get-Location).Path -replace '\\', '/')$bel$e]0;$Name$bel")
Write-Host "$Name, waiting on $Control"
$clock = [Diagnostics.Stopwatch]::StartNew()
$seen = $null
$mode = 'rest'
# The title it last set, which `repeat` sends again
$title = $Name
$sent = 0
$turn = 0
$nextTitle = 0
$nextStatus = 0
$nextDraw = 0
while ($true) {
  $line = $null
  try { $line = [IO.File]::ReadAllText($Control).Trim() } catch { }
  if ($null -ne $line -and $line -ne $seen) {
    $seen = $line
    try { [IO.File]::WriteAllText("$Control.sent", "$sent") } catch { }
    $now = $clock.ElapsedMilliseconds
    if ($line -like 'title:*') {
      $title = $line.Substring(6)
      [Console]::Write("$e]0;$title$bel"); $sent++
      $mode = 'rest'
    } elseif ($line -like 'progress:*') {
      $n = [int]$line.Substring(9)
      [Console]::Write($(if ($n) { "$e]9;4;1;$n$bel" } else { "$e]9;4;0;0$bel" }))
      $mode = 'rest'
    } else {
      $mode = $line
      $nextTitle = $now + $PhaseMs
      $nextStatus = $now + $PhaseMs
      $nextDraw = $now
    }
  }
  $now = $clock.ElapsedMilliseconds
  switch ($mode) {
    { $_ -in 'titles', 'mix' } {
      if ($now -ge $nextTitle) {
        $title = "$($glyphs[$turn % 2]) $Name"
        [Console]::Write("$e]0;$title$bel"); $sent++; $turn++
        $nextTitle += 960
      }
    }
    'repeat' {
      if ($now -ge $nextTitle) {
        [Console]::Write("$e]0;$title$bel"); $sent++
        $nextTitle += 100
      }
    }
    'draw' {
      if ($now -ge $nextDraw) {
        $h = [Console]::WindowHeight
        [Console]::Write("$e[$h;1H$e[2K$($spinner[$turn % $spinner.Count]) Working on it ($([int]($now / 1000))s)"); $turn++
        $nextDraw += 100
      }
    }
  }
  if ($mode -eq 'mix' -and $now -ge $nextStatus) {
    [Console]::Write("$e]7501;state=working:app=claude:msg=$($messages[[int]($now / 2500) % 2])$e\$e]9;4;3;0$bel")
    $nextStatus += 2500
  }
  Start-Sleep -Milliseconds 20
}
