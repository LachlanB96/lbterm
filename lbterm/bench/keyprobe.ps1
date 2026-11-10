#Requires -Version 7
<#
What runs in the terminal for key-latency.ps1: a row of ten solid cells (reverse-video spaces) for
finding the cell grid in a capture, then a ">" prompt with the cursor after it, read with the
console's own line input ([Console]::ReadLine, cooked mode). So a typed key is echoed by ConPTY
itself, as cmd's prompt is, and comes back to the terminal as output: the same path in every
terminal, with no shell of its own in it. -Ready is written once the prompt is up.

  pwsh -NoProfile -File keyprobe.ps1 -Ready <file>
#>
param([Parameter(Mandatory)][string]$Ready)
$e = [char]27
[Console]::Write("$e[0m$e[2J$e[3J$e[H$e[7m$(' ' * 10)$e[0m`r`n>")
[IO.File]::WriteAllText($Ready, "$PID")
while ($true) { $null = [Console]::ReadLine(); [Console]::Write("$e[2;1H$e[2K>") }
