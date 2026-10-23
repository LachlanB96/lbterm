#Requires -Version 7
<#
A stand-in tab for panel-demo.ps1: sets the title and the claude_status user var a Claude Code
session would have (as Terminal's claudehook.exe sets it), prints a line saying it's a demo, and
waits. -State working | waiting | idle | none; -AgeMinutes backdates the status so the idle timer
shows. -Box draws a box out to the pane's edges, as Claude Code draws its prompt, so a card's
edges and what's cut off at them show. -Session sets the claude_session user var, as claudehook.exe
does at SessionStart, so the tab's label comes from <session>.label.txt in lbterm_summaries.
-StatusFile names a file to watch: whenever it says another state, the tab takes it, as a session
does when it starts working again (conductor-demo.ps1).
#>
param(
  [string]$Title = 'demo',
  [ValidateSet('working', 'waiting', 'idle', 'none')]
  [string]$State = 'none',
  [double]$AgeMinutes = 0,
  [switch]$Box,
  [string]$Session,
  [string]$StatusFile
)
$e = [char]27
$bel = [char]7
if ($Session) {
  [Console]::Write("$e]1337;SetUserVar=claude_session=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Session)))$bel")
}
function Set-Status([string]$State, [double]$AgeMinutes = 0) {
  if ($State -eq 'none') { return }
  $hex = @{ working = '006400'; waiting = 'ff8c00'; idle = 'default' }[$State]
  $stamp = [DateTime]::UtcNow.AddMinutes(-$AgeMinutes).ToFileTimeUtc()
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("$hex|$stamp"))
  [Console]::Write("$e]1337;SetUserVar=claude_status=$b64$bel")
}
Set-Status $State $AgeMinutes
# The title (with Claude Code's glyph) is set on the tab by panel-demo.ps1 through `lbterm cli
# set-tab-title`: a title a script sets here doesn't get through ConPTY to the terminal
$header = "$e[2mLBTerm tab panel demo tab: $Title ($State)$e[0m"
if ($StatusFile) {
  Write-Host $header
  $shown = $State
  while ($true) {
    $want = "$(Get-Content $StatusFile -Raw -ErrorAction SilentlyContinue)".Trim()
    if ($want -in 'working', 'waiting', 'idle' -and $want -ne $shown) { Set-Status $want; $shown = $want }
    Start-Sleep -Milliseconds 100
  }
}
if (-not $Box) {
  Write-Host $header
  Start-Sleep -Seconds 86400
  return
}
# The box, drawn again whenever the pane's width changes (a split, a close)
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$drawn = 0
while ($true) {
  $w = [Console]::WindowWidth
  if ($w -ne $drawn -and $w -gt 8) {
    $line = { param($text) $text = $text.Substring(0, [Math]::Min($text.Length, $w - 4)); "│ $text$(' ' * ($w - 4 - $text.Length)) │" }
    [Console]::Write("$e[2J$e[H")
    Write-Host $header
    Write-Host ("╭" + ("─" * ($w - 2)) + "╮")
    Write-Host (& $line "> $Title")
    Write-Host (& $line "  the box runs out to the pane's edges")
    Write-Host ("╰" + ("─" * ($w - 2)) + "╯")
    Write-Host ("0123456789" * [Math]::Ceiling($w / 10)).Substring(0, $w)
    $drawn = $w
  }
  Start-Sleep -Milliseconds 200
}
