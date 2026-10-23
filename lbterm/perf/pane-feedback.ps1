#Requires -Version 7
# Covers: lbterm-extras/src/arrival.rs, lbterm-extras/src/lights.rs, lbterm-motion/src/**, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/termwindow/mouseevent.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/overlay/copy.rs, wezterm-gui/src/termwindow/resize.rs, mux/src/lbterm_repaint.rs
<#
The panes' answers to actions (lbterm/FEEDBACK.md), end to end in a test window of its own class
with the compositor on, at slowdown -Slowdown so PrintWindow sees each one. Nothing here is real
input: focus, keys and clicks are posted messages, and the window never takes the foreground.

1. Keyboard: a posted WM_KILLFOCUS puts the active outline out (dim); a posted WM_SETFOCUS brings
   it back with a pulse brighter than its rest (arrive_in, arrive_out), which settles at rest.
2. Arrival: `cli activate-pane` with no key in the window pulses the outline on the new pane; the
   same just after a posted Shift is the key's, and glides without a pulse. Each move must paint
   by itself (its frame in the state file) before the captures that record it start.
3. Ripple: a posted right click lights a circle round where it landed, growing as it fades, and
   the pane ends as it was (press).
4. Copy: a posted drag over the prompt, let go (bound to CompleteSelection 'PrimarySelection',
   which never touches Lachlan's clipboard), lights the selection brighter than its highlight,
   which it settles to (copy_in, copy_out).
5. A link in a program that has the mouse: a posted click on a URL a program prints after turning
   on SGR mouse reporting opens it (the test config's open-uri handler logs it and opens nothing),
   and the program gets none of that click; a click off the link still reaches it.
6. Readout: the window resized by SetWindowPos (no activation) shows a card in the middle of the
   panes, gone a moment after; F5 (IncreaseFontSize in the test config) shows it again.
7. Landed mark: F7 opens a search for "needle", Enter moves to a match, which is marked in the
   panel's coral.

Captures go to target\perf\pane-feedback.

  pwsh -File lbterm\perf\pane-feedback.ps1 [-Slowdown 4]
#>
param([double]$Slowdown = 4, [string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'pane-feedback' -Bin $Bin
$cfg = Join-Path $T.Out 'config\feedback.lua'
$programLog = Join-Path $T.Out 'program-input.txt'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The panel's coral (RING_MIDDLE #FF8A5C, the landed mark's bar)
$coral = New-ColourRange 181 255 91 169 0 129

[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local act = wezterm.action
-- Opening a link logs it and opens nothing
wezterm.on('open-uri', function(window, pane, uri)
  wezterm.log_warn('LBTERM-TEST open-uri ' .. uri)
  return false
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  lbterm_compositor = true,
  lbterm_motion = { enabled = 'on', slowdown = $Slowdown },
  initial_cols = 100, initial_rows = 26,
  window_close_confirmation = 'NeverPrompt',
  enable_tab_bar = false,
  cursor_blink_rate = 0,
  default_cursor_style = 'SteadyBlock',
  lbterm_crash_restore = false,
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
  -- Selecting as WezTerm's defaults do, but copying where Lachlan's clipboard never sees it
  disable_default_mouse_bindings = true,
  mouse_bindings = {
    { event = { Down = { streak = 1, button = 'Left' } }, mods = 'NONE',
      action = act.SelectTextAtMouseCursor 'Cell' },
    { event = { Drag = { streak = 1, button = 'Left' } }, mods = 'NONE',
      action = act.ExtendSelectionToMouseCursor 'Cell' },
    { event = { Up = { streak = 1, button = 'Left' } }, mods = 'NONE',
      action = act.CompleteSelectionOrOpenLinkAtMouseCursor 'PrimarySelection' },
  },
  keys = {
    { key = 'F5', action = act.IncreaseFontSize },
    { key = 'F6', action = act.ResetFontSize },
    { key = 'F7', action = act.Search { CaseSensitiveString = 'needle' } },
  },
}
"@)

function Trace($values) { Format-Trace $values '{0:N1}' }
# Settled: two shots 700 ms apart match by brightness, at the same size (a window still changing
# size isn't still)
function Settled($w) { Wait-Still $w -Lum -NoThrow }
# `cli activate-pane`, then the frame the window paints for it by itself, before any capture
# (which would paint it too: perf\cli-repaint.ps1)
function Move-Keyboard([int]$to) {
  $before = Get-StateFrame $w
  Invoke-Cli activate-pane --pane-id $to | Out-Null
  Wait-State { param($s) $s.active_pane -eq $to } -AfterFrame $before -TimeoutSec 3 -PresentMs 0 -What "pane $to given the keyboard by the cli, painted" | Out-Null
}

$slow = { param($ms) [int]($ms * $Slowdown) }
Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -SameProcess -Log 'warn'
  Start-Sleep 2
  $first = (Get-Panes)[0].pane_id
  Invoke-Cli send-text --pane-id $first --no-paste "echo needle`r" | Out-Null
  $second = [int](Invoke-Cli split-pane --pane-id $first --right -Program (New-PwshProgram))
  $rest = Settled $w
  Save-Shot $rest 'rest'
  $winW = $rest.Width; $winH = $rest.Height
  # The lower half: the outlines and nothing else that changes (the prompts and cursors are on top)
  $lower = New-Rect 0 ([int]($winH / 2)) $winW ([int]($winH / 2) - 2)
  $brighter = { param($a, $b, $r) [LbtermE2E.Px]::Brighter($a, $b, $r) }

  # 1. Keyboard away and back
  Send-Focus $w -Off
  Start-Sleep -Milliseconds (& $slow 900)
  $off = Get-Shot $w -Name 'keyboard-away'
  $atRest = & $brighter $off $rest $lower
  "outline at rest: $('{0:N2}' -f $atRest) brighter than with no keyboard, over the lower half"
  if ($atRest -le 0.01) { Add-Failure 'the outline at rest shows no different from the window without the keyboard: it never went out (keyboard-away.png)' }
  Send-Focus $w
  $frames = Get-Frames $w (& $slow 700)
  $light = @($frames | ForEach-Object { & $brighter $off $_.Shot $lower })
  "keyboard back: $(Trace $light)"
  $peak = ($light | Measure-Object -Maximum).Maximum
  if ($peak -lt 2.5 * $atRest) { Add-Failure "the outline didn't pulse as the keyboard came back: peak $('{0:N2}' -f $peak) against rest $('{0:N2}' -f $atRest)" }
  if ([math]::Abs($light[-1] - $atRest) -gt [math]::Max(0.02, 0.25 * $atRest)) { Add-Failure "the outline didn't settle back at rest: $('{0:N2}' -f $light[-1]) against $('{0:N2}' -f $atRest)" }
  Save-Shot $frames[([array]::IndexOf($light, $peak))].Shot 'keyboard-back-peak'
  Remove-Frames $frames

  # 2. Arrival from outside, and a key's own move. The pane that has the keyboard after the split
  # is the new one; the key goes first, then the cli moves the keyboard back to the first pane.
  Start-Sleep -Milliseconds 1200
  Send-Key $w Shift
  Start-Sleep -Milliseconds 100
  Move-Keyboard $first
  $frames = Get-Frames $w (& $slow 600)
  $keyed = @($frames | ForEach-Object { & $brighter $off $_.Shot $lower })
  "after a key: $(Trace $keyed)"
  $keyedPeak = ($keyed | Measure-Object -Maximum).Maximum
  if ($keyedPeak -gt 1.8 * $atRest) { Add-Failure "a move just after a key pulsed (peak $('{0:N2}' -f $keyedPeak) against rest $('{0:N2}' -f $atRest)): it's the key's" }
  Remove-Frames $frames
  Start-Sleep -Milliseconds 1500
  Move-Keyboard $second
  $frames = Get-Frames $w (& $slow 600)
  $outside = @($frames | ForEach-Object { & $brighter $off $_.Shot $lower })
  "from outside: $(Trace $outside)"
  $outsidePeak = ($outside | Measure-Object -Maximum).Maximum
  if ($outsidePeak -lt 2.5 * $atRest) { Add-Failure "activating a pane from outside didn't pulse: peak $('{0:N2}' -f $outsidePeak) against rest $('{0:N2}' -f $atRest)" }
  Save-Shot $frames[([array]::IndexOf($outside, $outsidePeak))].Shot 'arrival-peak'
  Remove-Frames $frames
  Move-Keyboard $first
  Wait-Change { param($s) $s.active_pane -eq $first } -What 'the first pane given the keyboard, settled' -Animating | Out-Null
  $base = Get-Shot $w

  # 3. A right click's ripple, low in the first pane
  $cx = [int]($winW / 4); $cy = [int]($winH * 0.7)
  Send-Click $w $cx $cy -Right
  $frames = Get-Frames $w (& $slow 400)
  $reach = @($frames | ForEach-Object { [LbtermE2E.Px]::Reach($base, $_.Shot, $cx, $cy, 160, 6) })
  "ripple's reach: $(Trace $reach)"
  $lit = @($reach | Where-Object { $_ -gt 0 })
  if ($lit.Count -lt 2) { Add-Failure 'a right click lit nothing round where it landed' }
  elseif ($lit[-1] -le $lit[0]) { Add-Failure "the ripple didn't grow: $(Trace $lit)" }
  if ($lit.Count) { Save-Shot $frames[[array]::IndexOf($reach, $lit[0])].Shot 'ripple' }
  Remove-Frames $frames
  Start-Sleep -Milliseconds (& $slow 300)
  $after = Get-Shot $w
  $whole = New-Rect 0 0 $winW $winH
  $left = [LbtermE2E.Px]::Changed($base, $after, $whole, 2)
  if ($left -gt 0) { Add-Failure "the ripple left $left pixels changed behind it (ripple-after.png)"; Save-Shot $after 'ripple-after' }

  # 4. A copy lights what's copied: a drag over the first pane's prompt
  $leftPane = New-Rect 0 0 ([int]($winW / 2) - 8) ([int]($winH / 2))
  $prompt = [LbtermE2E.Px]::FirstBrightRow($base, $leftPane, 150)
  if (-not $prompt) { throw 'no prompt found in the first pane (rest.png)' }
  $py = $prompt[0] + 6; $px0 = $prompt[1] + 2; $px1 = [int](($prompt[1] + $prompt[2]) / 2)
  Send-Mouse $w LDown $px0 $py -Keys L
  Send-Mouse $w Move (($px0 + $px1) / 2) $py -Keys L
  Send-Mouse $w Move $px1 $py -Keys L
  Send-Mouse $w LUp $px1 $py
  $frames = Get-Frames $w (& $slow 600)
  $band = New-Rect $px0 ($py - 6) ($px1 - $px0) 12
  $copyLight = @($frames | ForEach-Object { & $brighter $base $_.Shot $band })
  "copy: $(Trace $copyLight)"
  $copyPeak = ($copyLight | Measure-Object -Maximum).Maximum
  if ($copyPeak -lt $copyLight[-1] + 8) { Add-Failure "the copied text didn't light up past its highlight: peak $('{0:N1}' -f $copyPeak), settled $('{0:N1}' -f $copyLight[-1])" }
  Save-Shot $frames[([array]::IndexOf($copyLight, $copyPeak))].Shot 'copy-peak'
  Remove-Frames $frames
  Send-Click $w ([int]($winW / 4)) ([int]($winH * 0.8))

  # 7. Search's landed mark (before the link's tab: it searches the first pane)
  Start-Sleep -Milliseconds 400
  Send-Key $w F7
  Start-Sleep -Milliseconds 600
  Send-Key $w Enter
  Start-Sleep -Milliseconds 300
  $marked = Get-Shot $w -Name 'search-mark'
  $n = [LbtermE2E.Px]::CountRange($marked, $whole, $coral)
  "search: $n coral pixels"
  if ($n -lt 10) { Add-Failure 'moving to a search match marked nothing in coral (search-mark.png)' }
  Send-Key $w Escape
  Start-Sleep -Milliseconds 2000

  # 6. The readout: resized, then the font
  $before = Settled $w
  $wr = Get-WindowRect $w
  Set-WindowSize $w ($wr.Right - $wr.Left - 90) ($wr.Bottom - $wr.Top)
  Start-Sleep -Milliseconds 250
  $shown = Get-Shot $w -Name 'readout'
  # Over the active pane, the first, on the left
  $middle = New-Rect ([int]($shown.Width / 4 - 50)) ([int]($shown.Height / 2 - 20)) 100 40
  $settled = Settled $w
  $readout = [LbtermE2E.Px]::Changed($settled, $shown, $middle, 12)
  "readout after a resize: $readout pixels in the middle"
  if ($readout -lt 30) { Add-Failure 'resizing the window showed no readout in the middle (readout.png)' }
  Send-Key $w F5
  Start-Sleep -Milliseconds 300
  $font = Get-Shot $w -Name 'readout-font'
  Send-Key $w F6
  $settled2 = Settled $w
  $middle2 = New-Rect ([int]($font.Width / 4 - 50)) ([int]($font.Height / 2 - 20)) 100 40
  if ($font.Width -eq $settled2.Width -and $font.Height -eq $settled2.Height) {
    $fontReadout = [LbtermE2E.Px]::Changed($settled2, $font, $middle2, 12)
    "readout after F5: $fontReadout pixels in the middle"
    if ($fontReadout -lt 30) { Add-Failure 'a bigger font showed no readout in the middle (readout-font.png)' }
  } else {
    "readout after F5: the window changed size with the font; see readout-font.png"
  }

  # 5. A link in a program that has the mouse, in a tab of its own
  $script = @"
`$e = [char]27
# VT input (prompt-mouse.ps1's way): ConPTY then passes mouse reports through as typed characters
Add-Type -Namespace W -Name C -MemberDefinition '[DllImport("kernel32.dll")] public static extern System.IntPtr GetStdHandle(int h); [DllImport("kernel32.dll")] public static extern bool GetConsoleMode(System.IntPtr h, out uint m); [DllImport("kernel32.dll")] public static extern bool SetConsoleMode(System.IntPtr h, uint m);'
`$in = [W.C]::GetStdHandle(-10); `$mode = 0; [void][W.C]::GetConsoleMode(`$in, [ref]`$mode)
[void][W.C]::SetConsoleMode(`$in, `$mode -bor 0x200)
[Console]::Write("`$e[?1000h`$e[?1006h")
[Console]::WriteLine('https://example.com/lbterm-feedback-test')
while (`$true) { `$k = [Console]::ReadKey(`$true); [IO.File]::AppendAllText('$($programLog -replace "'", "''")', [string]`$k.KeyChar) }
"@
  New-Tab -From $first -Program (New-PwshProgram $script) | Out-Null
  $tab = Settled $w
  Save-Shot $tab 'link-tab'
  $url = [LbtermE2E.Px]::FirstBrightRow($tab, (New-Rect 0 0 $tab.Width ([int]($tab.Height / 2))), 150)
  if (-not $url) { throw 'the link never showed (link-tab.png)' }
  $ux = [int](($url[1] + $url[2]) / 2); $uy = $url[0] + 6
  Send-Click $w $ux $uy -Move
  $linkFrames = Get-Frames $w (& $slow 300)
  Start-Sleep -Milliseconds 800
  $opened = @(Get-LogLines $w 'LBTERM-TEST open-uri (\S+)')
  $got = if (Test-Path $programLog) { Get-Content $programLog -Raw } else { '' }
  "link: opened $(@($opened | ForEach-Object { if ($_ -match 'LBTERM-TEST open-uri (\S+)') { $Matches[1] } }) -join ', '); the program got $($got.Length) characters"
  if (-not ($opened | Where-Object { $_ -match 'lbterm-feedback-test' })) { Add-Failure 'a click on the link in a program with the mouse opened nothing' }
  if ($got -match '\[<0;') { Add-Failure "the program got the click on the link too: $($got -replace [char]27, '\e')" }
  $linkBand = New-Rect $url[1] ($url[0] - 4) ($url[2] - $url[1]) 16
  $linkLight = @($linkFrames | ForEach-Object { & $brighter $tab $_.Shot $linkBand })
  "link light: $(Trace $linkLight)"
  if (($linkLight | Measure-Object -Maximum).Maximum -lt 6) { Add-Failure 'the link didn''t light up as it opened' }
  Remove-Frames $linkFrames
  Send-Click $w $ux ([int]($tab.Height * 0.75))
  Start-Sleep -Milliseconds 800
  $got = if (Test-Path $programLog) { Get-Content $programLog -Raw } else { '' }
  if ($got -notmatch '\[<0;') { Add-Failure 'a click off the link never reached the program: it wasn''t reporting the mouse, so the link test proves nothing' }
} -Passed "pane feedback: the outline goes out and pulses back, outside arrivals pulse and keys' moves don't, ripples grow and leave nothing, copies and links light up, links open past a program's mouse, the readout and the search mark show"
