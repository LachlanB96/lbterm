#Requires -Version 7
<#
Records the light a Hide from Conductor sends up the rail (lbterm/NEW-TAB.md, section 8, "The send
light"; Lachlan's 6a of 2026-11-01, the `send` token) with capture.ps1: not a test, a capture for
judging the motion by eye (the lbterm-motion skill's check). A window that looks like Lachlan's
(capture.ps1's New-TestConfig) with four tabs, three shells and a stand-in Claude tab (row 3,
its claude_session user var set as claudehook.exe sets it), the Conductor's pad from a stand-in
button.json with `hidden: []` that this script writes under target\motion, and a
`lbterm-conductor-hide` handler of the window's own that does nothing: no Conductor is ever asked,
the real button.json is never read, and nothing is hidden. Lachlan's lbterm.lua, which
New-TestConfig loads, has a handler for the event (Terminal's, since landing 3), but it returns
nothing in a GUI with LBTERM_STATE_FILE, which Open-TestWindow (so capture.ps1) always sets: the
window's own do-nothing handler gets the event, and the stand-in session id never reaches the
Conductor (PITFALLS 68).
The tab menu is opened on row 3 before the recording and H is the recorded step: the menu's
card fades out (`pane_exit`), and once it has gone the light sets off from pad 4 to the
Conductor's pad (`copy_in`, `send`, `copy_out`: 860 ms at real speed).

  pwsh -File lbterm\motion\send-light.ps1                       # slowed five times, 6 s
  pwsh -File lbterm\motion\send-light.ps1 -Slowdown 1 -Seconds 1.5 -Name send-light-1x
  pwsh -File lbterm\motion\send-light.ps1 -MotionOff -Name send-light-off -Seconds 1.5

Then read target\motion\<Name>\contact.png, and the rail's column through the light with
timesheet.py (times of real speed): python lbterm\motion\timesheet.py target\motion\<Name>
target\motion\<Name>\rail.png 0 30 90 400 2 <slowdown> 0,100,140,180,240,300,360,420,480,540,600,660,720,780,840,900,960,1040 9

Judge it awake: with the displays asleep DWM plays no motion into these frames (lbterm/PITFALLS.md,
239), so nothing captured then shows the light.
#>
param(
  [string]$Name = 'send-light',
  [double]$Slowdown = 5,
  [double]$Seconds = 6,
  [switch]$MotionOff,
  # Another build (capture.ps1's -Bin); this tree's target\release by default
  [string]$Bin,
  # The recorded step: H on the tab menu's Hide row by default
  [string]$Cli = 'post,key,H',
  [string]$Then,
  [int]$ThenMs = 300,
  [switch]$PostedPointer
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$dir = Join-Path $repo "target\motion\$Name-conductor"
New-Item -ItemType Directory -Force $dir | Out-Null
$button = Join-Path $dir 'button.json'
[IO.File]::WriteAllText($button, '{"state":"on","mode":"read","listening":false,"busy":false,"tooltip":"Conductor is on (read only)","hidden":[],"updatedAt":"2026-11-09T10:00:01.000Z"}')
# The stand-in Claude tab: claude_session set as claudehook.exe sets it, then it waits
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('c0ffee00-0000-4000-8000-0000000000aa'))
$wait = "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$b64' + [char]7); Start-Sleep 600"
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($wait))
$lua = "rawset(config, 'lbterm_conductor', { state_file = [[$($button -replace '\\', '/')]] }) wezterm.on('lbterm-conductor-hide', function(window, pane, h) end)"
$setup = "spawn,--pane-id,0;spawn,--pane-id,0;spawn,--pane-id,0,--,C:/Program Files/PowerShell/7/pwsh.exe,-NoProfile,-EncodedCommand,$encoded;activate-tab,--tab-index,0;post,rclick,row,3"
$p = @{
  Name = $Name; Slowdown = $Slowdown; Seconds = $Seconds; Lua = $lua; Setup = $setup; Cli = $Cli
  Bin = $(if ($Bin) { $Bin } else { Join-Path $repo 'target\release' })
}
if ($MotionOff) { $p.MotionOff = $true }
if ($Then) { $p.Then = $Then; $p.ThenMs = $ThenMs }
if ($PostedPointer) { $p.PostedPointer = $true }
& (Join-Path $PSScriptRoot 'capture.ps1') @p
