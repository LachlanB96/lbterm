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
-Setup runs `lbterm cli` commands before the recording, ';' between them (a second tab to switch
to: -Setup 'spawn,--pane-id,0;activate-tab,--tab-index,0'). A fade has no edge: -Fade X,Y,W,H
plots how much of the first frame still shows in that rectangle (sheet.py --fade, fade.png) and
-FadeMs MS (before slowdown) fits it to tab_switch's curve, its duration MS:

  pwsh -File lbterm\motion\capture.ps1 -Name tab-switch -Setup 'spawn,--pane-id,0;activate-tab,--tab-index,0' `
      -Cli activate-tab,--tab-index,1 -Fade 300,300,400,200 -FadeMs 200

A step of -Cli or -Then can be posted input instead of a cli command, as the harness posts it
(never real input, never focus): post,rclick,row,N right-clicks the tab panel's row N's pad (the
tab menu), post,click,row,N left-clicks it, post,move,item,N puts the pointer on the open menu's
item N, post,key,KEY posts a key (Escape, Down). A menu has no edge: -Fade menu reads the fade over
the menu's card, which the script finds by opening the menu once with -Cli before recording and
closing it with Escape (lbterm/NEW-TAB.md, section 6: the menus fade in on pane_enter and out on
pane_exit). -Prime is the step that opens it for that, when -Cli doesn't (a menu closed and
opened again as it fades out: -Prime post,rclick,row,0 -Setup post,rclick,row,0 -Cli post,key,Escape
-Then post,rclick,row,0). -MotionOff records the window with lbterm_motion off:
  pwsh -File lbterm\motion\capture.ps1 -Name menu -Cli post,rclick,row,0 -Fade menu
  pwsh -File lbterm\motion\capture.ps1 -Name menu-back -Cli post,rclick,row,0 -Then post,key,Escape `
      -ThenMs 300 -Fade menu
More places a posted step can aim at: post,rclick,newtab (or click) the new-tab pad at the rail's
foot (the + menu, the picker), post,click,strip,NAME the window strip's item NAME (agenda, inbox,
context), and post,click,item,N the item N of the menu the setup left open, as the state file
placed it once the setup settled (without -Fade menu). -Lua adds Lua to the window's config (key
bindings, a stand-in agenda file), -PostedPointer lets the posted pointer hover (the harness's
LBTERM_POSTED_POINTER). The new-tab-menus round's captures of 2026-11-08 (lbterm/FLOWS.md, "A menu,
from what opens it", Seen on screen) used them: a shortcut's pick, an agenda line's flash.
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
  # cli commands before the recording, ';' between them, each comma-separated
  [string]$Setup,
  # X,Y,W,H: plot a fade there instead of an edge; -FadeMs fits it (ms before slowdown)
  [string]$Fade,
  [double]$FadeMs,
  [switch]$Keep,
  # More settings for the window, each name=value as --config takes them
  [string[]]$Config = @(),
  [string]$Bin,
  # The step -Fade menu opens the menu with to find its card (else -Cli's)
  [string]$Prime,
  # Motion off in the window: what's recorded is the end state at once
  [switch]$MotionOff,
  # The window's log, as WEZTERM_LOG
  [string]$Log,
  # Lua added to the window's config (Open-TestWindow -Lua)
  [string]$Lua = '',
  # The posted pointer hovers (Open-TestWindow -PostedPointer)
  [switch]$PostedPointer
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
    $w = Open-TestWindow -Log $Log -WorkingDirectory $env:USERPROFILE -Lua $Lua -PostedPointer:$PostedPointer `
      -Set (@($Config) + 'lbterm_compositor=true' + $(if ($MotionOff) { "lbterm_motion={enabled='off'}" } else { "lbterm_motion={enabled='on',slowdown=$Slowdown}" }))
    # The shell started and the window settled, as its state file says (lbterm/STATE.md)
    Wait-Change { param($s) $s.frame -gt 0 -and $s.panes.Count -ge 1 } -What 'the window to settle' | Out-Null
  }
  # Where a posted step lands, from the state file: a rail row's pad, or the open menu's item
  $s0 = Wait-Settled -What 'the rail'
  if (-not @($s0.rail.rows).Count) { $s0 = Wait-Change { param($s) @($s.rail.rows).Count -ge 1 } -What 'a row on the rail' }
  $menuSeen = $null
  $place = { param([string[]]$parts)
    switch ($parts[2]) {
      'row' { Get-RectCenter $s0.rail.rows[[int]$parts[3]].pad }
      'newtab' {
        if (-not $s0.new_tab) { throw "post,$($parts[1]),newtab: the rail has no new-tab pad" }
        Get-RectCenter $s0.new_tab.rect
      }
      'strip' {
        $item = @($s0.strip.items | Where-Object name -eq $parts[3])[0]
        if (-not $item) { throw "post,$($parts[1]),strip,$($parts[3]): the strip shows no such item ($(@($s0.strip.items | ForEach-Object name) -join ', '))" }
        Get-RectCenter $item.rect
      }
      'item' {
        # The menu -Fade menu found, else the one the setup left open
        $menu = if ($menuSeen) { $menuSeen } else { $s0.menu }
        if (-not $menu) { throw "post,$($parts[1]),item needs a menu: give -Fade menu, or open one in -Setup" }
        Get-RectCenter $menu.items[[int]$parts[3]].rect
      }
      default { @([int]$parts[2], [int]$parts[3]) }
    }
  }
  # A step: posted input at once (post,...), or a cli command as a process of its own, so the frames
  # go on while the GUI answers it
  $step = { param([string[]]$parts)
    if ($parts[0] -ne 'post') { return Start-Process $T.Cli -ArgumentList (@('cli', '--no-auto-start') + $parts) -NoNewWindow -PassThru }
    switch ($parts[1]) {
      'key' { Send-Key $w $parts[2] }
      'rclick' { $x, $y = & $place $parts; Send-Click $w $x $y -Right -Move }
      'click' { $x, $y = & $place $parts; Send-Click $w $x $y -Move }
      'move' { $x, $y = & $place $parts; Send-Mouse $w Move $x $y }
      default { throw "post,$($parts[1]): rclick, click, move or key" }
    }
    $null
  }
  if ($Fade -eq 'menu') {
    # The menu once, to find its card, then closed and settled before the recording
    & $step $(if ($Prime) { @($Prime -split ',') } else { $cliArgs }) | Out-Null
    $menuSeen = (Wait-Change { param($s) $null -ne $s.menu } -What 'the menu to open').menu
    $r = $menuSeen.rect
    # Inside its card, clear of its corners and its rows' words: the card's fill and its glow's edge
    $Fade = '{0},{1},{2},{3}' -f [int]($r.x + 4), [int]($r.y + 4), [int]($r.w - 8), [int]($r.h - 8)
    Send-Key $w Escape
    Wait-Change { param($s) $null -eq $s.menu } -What 'the menu to close' | Out-Null
    Wait-Settled -Animating | Out-Null
    "the menu's card: $Fade"
  }
  if ($Setup) {
    foreach ($command in $Setup -split ';') {
      # What follows a `--` is the program, which Invoke-Cli takes apart (a function drops a bare --)
      $parts = @($command -split ',')
      $at = [Array]::IndexOf($parts, '--')
      # Posted input, as a step of -Cli is, onto the rail as the steps before have left it (a
      # tab a spawn added is a row to click)
      if ($parts[0] -eq 'post') { $s0 = Wait-Settled -What 'the rail'; & $step $parts | Out-Null; continue }
      if ($at -gt 0) { Invoke-Cli @($parts[0..($at - 1)]) -Program @($parts[($at + 1)..($parts.Count - 1)]) | Out-Null }
      else { Invoke-Cli @parts | Out-Null }
    }
    Wait-Settled -Animating | Out-Null
    # What the setup's shells print
    Start-Sleep -Milliseconds 1500
    # The rows -Cli and -Then click, as the setup left them
    $s0 = Wait-Settled -What 'the rail'
  }


  $started = @{ First = $null; Second = $null; FirstDone = $false; SecondDone = $false }
  $frames = Get-Frames $w ([int]($Seconds * 1000)) -Raw -Each {
    param($ms)
    if ($cliArgs -and -not $started.FirstDone) { $started.First = & $step $cliArgs; $started.FirstDone = $true }
    if ($thenArgs -and -not $started.SecondDone -and $ms -ge $ThenMs) { $started.Second = & $step $thenArgs; $started.SecondDone = $true }
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
if ($Fade) {
  $sheet += @('--fade') + @($Fade -split ',')
  if ($FadeMs) { $sheet += @('--fade-ms', ($FadeMs * $Slowdown)) }
}
if ($expectArgs) {
  # FROM TO MS BOUNCE, with MS stretched by the slowdown the frames were taken at
  $sheet += @('--expect', $expectArgs[0], $expectArgs[1], ($expectArgs[2] * $Slowdown), $expectArgs[3])
}
python @sheet
