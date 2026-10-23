#Requires -Version 7
<#
Records a pane animation frame by frame, for checking motion (lbterm/COMPOSITOR.md, and the check in
.claude/skills/lbterm-motion).

  pwsh -File lbterm\motion\capture.ps1 -Name split -Cli split-pane,--pane-id,0,--right
  pwsh -File lbterm\motion\capture.ps1 -Name retarget -Cli split-pane,--pane-id,0,--right `
      -Then split-pane,--pane-id,1,--bottom -ThenMs 450 -Expect 1096,651,220,0

Opens target\release\lbterm-gui.exe (or -Bin's; with Start-Unfocused, so it leaves Claude's job
object and takes no focus) looking like Lachlan's LBTerm (New-TestConfig: his lbterm.lua without
its summon key or mux domain, plus any -Config) with the compositor on, motion on and -Slowdown,
runs the `lbterm cli` command -Cli against it (and -Then -ThenMs later, to interrupt the motion),
and grabs the window with PrintWindow(PW_RENDERFULLCONTENT) as fast as it can for -Seconds.
PrintWindow renders DirectComposition content whatever covers the window. Frames go to
target\motion\<Name>, then sheet.py tiles them into contact.png and plots the first split line on
-Row in edge.png; -Expect FROM,TO,MS,BOUNCE (MS before slowdown) fits that line to the spring it
should follow. -Still is for an edge that's on screen before it moves (the active pane's outline
gliding onto a new pane): the motion starts when it moves. Pass -ProcessId to use a running dev
instance instead.
Needs Python with Pillow and numpy for the sheet.
#>
param(
  [string]$Name = 'capture',
  # Comma-separated: pwsh -File passes a list as one string
  [string]$Cli,
  [string]$Then,
  [int]$ThenMs = 300,
  [double]$Seconds = 2,
  [double]$Slowdown = 5,
  [int]$ProcessId,
  [int]$Row = 200,
  # Where along -Row the edge is looked for from (sheet.py --from)
  [int]$From = 0,
  [string]$Expect,
  [switch]$Still,
  [switch]$Keep,
  # More settings for the window, each name=value as --config takes them
  [string[]]$Config = @(),
  [string]$Bin
)
$ErrorActionPreference = 'Stop'
# On the right-most screen, without taking focus: the harness's test windows
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$cliArgs = if ($Cli) { @($Cli -split ',') } else { @() }
$thenArgs = if ($Then) { @($Then -split ',') } else { @() }
$expectArgs = if ($Expect) { @($Expect -split ',' | ForEach-Object { [double]$_ }) } else { @() }
if ($expectArgs -and $expectArgs.Count -ne 4) { throw "-Expect takes FROM,TO,MS,BOUNCE" }

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $repo "target\motion\$Name"
$T = Start-Test "motion-$Name" -Bin $Bin -Out $out -Class 'com.lachlanb.lbterm.motion'

try {
  if ($ProcessId) {
    # A running dev instance: its window and socket, never closed here
    $p = Get-Process -Id $ProcessId
    $w = New-WindowInfo -ProcessId $ProcessId -Handle $p.MainWindowHandle -Class $T.Class
    Use-TestWindow $w
  } else {
    $w = Open-TestWindow -WorkingDirectory $env:USERPROFILE `
      -Set (@($Config) + 'lbterm_compositor=true' + "lbterm_motion={enabled='on',slowdown=$Slowdown}")
    Start-Sleep 3  # let the shell start and the first frames settle
  }

  # The commands run as their own processes, so the frames go on while the GUI answers them
  $started = @{ First = $null; Second = $null }
  $frames = Get-Frames $w ([int]($Seconds * 1000)) -Raw -Each {
    param($ms)
    if ($cliArgs -and -not $started.First) { $started.First = Start-Process $T.Cli -ArgumentList (@('cli') + $cliArgs) -NoNewWindow -PassThru }
    if ($thenArgs -and -not $started.Second -and $ms -ge $ThenMs) {
      $started.Second = Start-Process $T.Cli -ArgumentList (@('cli') + $thenArgs) -NoNewWindow -PassThru
    }
  }
  foreach ($p in $started.First, $started.Second) { if ($p) { $p.WaitForExit() } }
  Save-Frames $frames -Dir $out -Window $w
  Remove-Frames $frames
  "$($frames.Count) frames in $Seconds s to $out"
} finally {
  # A dev instance passed in isn't the test's to close
  Close-Test -Keep:($Keep -or [bool]$ProcessId)
}

$sheet = @((Join-Path $PSScriptRoot 'sheet.py'), $out, '--row', $Row, '--from', $From)
if ($Still) { $sheet += '--still' }
if ($expectArgs) {
  # FROM TO MS BOUNCE, with MS stretched by the slowdown the frames were taken at
  $sheet += @('--expect', $expectArgs[0], $expectArgs[1], ($expectArgs[2] * $Slowdown), $expectArgs[3])
}
python @sheet
