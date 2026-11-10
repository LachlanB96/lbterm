#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/keyevent.rs, wezterm-gui/src/termwindow/echo.rs, lbterm-extras/src/echo.rs, config/src/lbterm.rs, window/src/os/windows/window.rs, wezterm-client/src/pane/renderable.rs, wezterm-client/src/pane/clientpane.rs, mux/src/lbterm_wait.rs, mux/src/lib.rs
<#
A key's echo shows in the first frame after it (lbterm/PERF.md, findings 35 and 68). A key
typed into a pane asks for no frame of its own unless that frame draws something new
(lbterm_extras::echo), so the frame its echo asks for isn't held a pacing period behind one that
drew nothing; on a mux domain the server's answer to each key, which changes nothing before the
echo, asks for none either (wezterm-client's renderable.rs, said on lbterm::echo); and the
parser's wait for the rest of a frame ends when it's due, not on Windows' 15.6 ms tick
(mux/src/lbterm_wait.rs). A key whose own frame does draw something still paints at once.

In a test window like Lachlan's (New-TestConfig), its tab a pwsh without a profile at a short
prompt; with -Mux its tab lives in a test lbterm-mux-server (target\perf\echo-frame-mux). Every
frame the GUI writes is kept (Start-FrameRecorder), so "the first frame after it" is exact. Each
step starts from a window at rest (no frame for 300 ms) and is read from the frames and metrics.

1. -Keys characters typed at the prompt, one at a time: each one's echo (the drawn cursor on one)
   in the first frame after it, and no frame asked for by keyevent.rs. On a mux domain, the
   server's answers said to change nothing, at least one.
2. A key bound to SendString (F9, sending "y"), as Lachlan's Shift+Enter is: the same.
3. Scrolled back a page (F10, ScrollByPage -1), a character typed: the first frame after it shows
   the pane back at the bottom, asked for by keyevent.rs, the key's own frame.
4. Copy mode (F11, an overlay), k: the first frame after it shows copy mode's cursor a row up,
   asked for by keyevent.rs. Escape leaves it.
5. An IME's composition (a dead key's goes the same way): its end, which Windows sends with the
   composed character right behind it, paints with that key's echo, the first frame after them
   both, where its own frame came first and held the echo's a pacing period (the review of
   perf/key-latency, 2026-10-31); ended alone (cancelled), its own frame a while on
   (COMPOSITION_CLEAR_WAIT, 50 ms), asked for by termwindow/echo.rs. A composition can't be typed
   from here, so it's posted as Windows sends one: WM_IME_COMPOSITION with GCS_COMPSTR (the
   window's IME holds no text, so the composing text is empty, which LBTerm takes as composing)
   and WM_IME_ENDCOMPOSITION.

Each step's times from the key's post to the frame that showed it are said, not judged: the
budget runner holds them (key-echo, key-burst, key-held and their -local scenarios). A build
without drawn_cursor in its state file can't say which frame drew an echo, so it fails saying so.

  pwsh -File lbterm\perf\echo-frame.ps1 [-Mux] [-Keys 8] [-Bin <build>]
#>
param([string]$Bin, [switch]$Mux, [int]$Keys = 8)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('echo-frame' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$cfg = Join-Path $T.Out 'config\echo-frame.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

$domain = if ($Mux) {
  @"
config.unix_domains = { {
  name = 'perfechoframe',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 100
config.initial_rows = 24
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
config.window_close_confirmation = 'NeverPrompt'
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-NoLogo', '-NoExit', '-Command', [[function prompt { 'PS> ' }]] }
config.default_cwd = 'C:/Users/Public'
local act = wezterm.action
config.keys = config.keys or {}
for _, k in ipairs({
  { key = 'F9', mods = 'NONE', action = act.SendString 'y' },
  { key = 'F10', mods = 'NONE', action = act.ScrollByPage(-1) },
  { key = 'F11', mods = 'NONE', action = act.ActivateCopyMode },
}) do table.insert(config.keys, k) end
$domain
"@

function Get-Pane($sw) { @($sw.panes | Where-Object active)[0] }
function Get-DrawnCursor($p) { $p.drawn_cursor }
$times = [ordered]@{}

# One key from a window at rest: posted by $Send, to the first frame $Shows says shows it. What
# frame after the key that was (1, the first), how long after its post, and the frames' requests
# by site from metrics, reset just before it
function Measure-Key([scriptblock]$Send, [scriptblock]$Shows, [string]$What) {
  Get-QuietFrame -QuietMs 300 -TimeoutSec 20 -Window $script:w | Out-Null
  $script:rec.Look(); [void]$script:rec.Take()
  Get-Metrics -Window $script:w -Reset | Out-Null
  $p0 = [long](Read-State -Window $script:w).frame
  $t0 = Get-UnixMs
  & $Send
  $shown = Wait-Recorded -Recorder $script:rec { param($x, $s) & $Shows $x } -Hwnd ([long]$script:w.Handle) -AfterFrame $p0 -TimeoutSec 10 -What $What
  Get-QuietFrame -QuietMs 300 -TimeoutSec 20 -Window $script:w | Out-Null
  $m = Get-Metrics -Window $script:w
  [pscustomobject]@{
    Nth = [long]$shown.Frame - $p0
    Ms = [Math]::Round([double]$shown.TimeMs - $t0, 1)
    State = $shown.Window
    Sites = @($m.requests | ForEach-Object { [string]$_.site })
  }
}
# The first frame after $Send, from a window at rest: how long after it and who asked for frames
function Measure-AnyFrame([scriptblock]$Send, [string]$What) {
  Get-QuietFrame -QuietMs 300 -TimeoutSec 20 -Window $script:w | Out-Null
  $script:rec.Look(); [void]$script:rec.Take()
  Get-Metrics -Window $script:w -Reset | Out-Null
  $p0 = [long](Read-State -Window $script:w).frame
  $t0 = Get-UnixMs
  & $Send
  $shown = Wait-Recorded -Recorder $script:rec { param($x, $s) $true } -Hwnd ([long]$script:w.Handle) -AfterFrame $p0 -TimeoutSec 10 -What $What
  Get-QuietFrame -QuietMs 300 -TimeoutSec 20 -Window $script:w | Out-Null
  $m = Get-Metrics -Window $script:w
  [pscustomobject]@{
    Ms = [Math]::Round([double]$shown.TimeMs - $t0, 1)
    Sites = @($m.requests | ForEach-Object { [string]$_.site })
  }
}
# An IME's composition as Windows posts it (step 5): composing, and its end
function Send-ImeComposing { [LbtermE2E.Win]::Post([IntPtr][long]$script:w.Handle, 0x010F, [IntPtr]::Zero, [IntPtr]8) }
function Send-ImeEnd { [LbtermE2E.Win]::Post([IntPtr][long]$script:w.Handle, 0x010E, [IntPtr]::Zero, [IntPtr]::Zero) }
function Test-KeySite($r) { @($r.Sites | Where-Object { $_ -match 'termwindow[\\/]keyevent\.rs' }).Count -gt 0 }
function Format-Ms($list) { $s = @($list | Sort-Object); "median $($s[[int][Math]::Floor(($s.Count - 1) / 2)]) ms, longest $($s[-1]) ms" }

Invoke-Test {
  $log = 'info,lbterm::echo=debug'
  $script:w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfechoframe -Socket $T.Sock -Log $log -TimeoutSec 30 }
  else { Open-TestWindow -ConfigFile $cfg -Log $log -TimeoutSec 30 }
  if ($Mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 20 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  $pane = (Get-Panes -Server:$Mux)[0].pane_id
  Send-Focus $script:w
  $sw = Wait-Change { param($s) (Get-Pane $s) -and $null -ne (Get-Pane $s).cursor -and (Get-Pane $s).cursor.x -eq 4 } -What 'pwsh at its short prompt' -TimeoutSec 30
  if ($null -eq (Get-Pane $sw).drawn_cursor) { throw 'this build''s state file has no drawn_cursor, so which frame drew an echo can''t be said (lbterm/STATE.md)' }
  # Something to scroll back over in step 3
  Invoke-Cli send-text --pane-id $pane --no-paste "1..60`r" -Server:$Mux -Check | Out-Null
  $sw = Wait-Change { param($s) (Get-DrawnCursor (Get-Pane $s)).x -eq 4 -and (Get-DrawnCursor (Get-Pane $s)).y -ge 20 } -What 'sixty lines and the prompt again' -TimeoutSec 15
  $script:rec = Start-FrameRecorder -Window $script:w
  $logMark = (Get-Item -LiteralPath $script:w.Log).Length
  try {
    # 1. Characters typed: each echo in the first frame after its key, none asked for by the key
    $echo = @()
    for ($i = 1; $i -le $Keys; $i++) {
      $c = Get-DrawnCursor (Get-Pane (Get-StateWindow (Read-State -Window $script:w) -Hwnd ([long]$script:w.Handle)))
      $r = Measure-Key { Send-Key $script:w X } { param($x) $d = Get-DrawnCursor (Get-Pane $x); $d.x -ne $c.x -or $d.y -ne $c.y } "key $i's echo"
      $echo += $r.Ms
      if ($r.Nth -ne 1) { Add-Failure "1. key $i's echo came in frame $($r.Nth) after it, not the first ($($r.Ms) ms)" }
      if (Test-KeySite $r) { Add-Failure "1. key $i asked for a frame of its own: $($r.Sites -join ', ')" }
    }
    $times['1. characters'] = Format-Ms $echo
    Write-Host "1. $Keys characters: each echo's frame $(Format-Ms $echo) after its key"
    if ($Mux) {
      $said = @(Read-LogLines $script:w.Log $logMark | Where-Object { $_ -match 'lbterm::echo\s+> pane \d+: a delta that changes nothing' })
      Write-Host "   the server's answers that changed nothing: $($said.Count)"
      if (-not $said.Count) { Add-Failure '1. no answer of the server''s to a key was said to change nothing (lbterm::echo): each key''s answer asked for a frame' }
    }

    # 2. A binding that sends text, as Shift+Enter's ESC CR does
    $c = Get-DrawnCursor (Get-Pane (Get-StateWindow (Read-State -Window $script:w) -Hwnd ([long]$script:w.Handle)))
    $r = Measure-Key { Send-Key $script:w F9 } { param($x) $d = Get-DrawnCursor (Get-Pane $x); $d.x -ne $c.x -or $d.y -ne $c.y } 'F9''s "y"'
    $times['2. SendString'] = "$($r.Ms) ms"
    Write-Host "2. F9's SendString: its echo in frame $($r.Nth) after it, $($r.Ms) ms"
    if ($r.Nth -ne 1) { Add-Failure "2. F9's echo came in frame $($r.Nth) after it, not the first" }
    if (Test-KeySite $r) { Add-Failure "2. F9 asked for a frame of its own: $($r.Sites -join ', ')" }

    # 3. Scrolled back, a key typed scrolls to the bottom: the key's own frame shows it
    Send-Key $script:w F10
    Wait-Change { param($s) $null -ne (Get-Pane $s).scroll_top } -What 'scrolled back a page' -TimeoutSec 10 | Out-Null
    $r = Measure-Key { Send-Key $script:w X } { param($x) $null -eq (Get-Pane $x).scroll_top } 'the pane back at the bottom'
    $times['3. scrolled back'] = "$($r.Ms) ms"
    Write-Host "3. scrolled back, a key: at the bottom in frame $($r.Nth) after it, $($r.Ms) ms, asked by $($r.Sites -join ', ')"
    if ($r.Nth -ne 1) { Add-Failure "3. the scroll to the bottom came in frame $($r.Nth) after the key, not the first" }
    if (-not (Test-KeySite $r)) { Add-Failure "3. the key that scrolled to the bottom asked for no frame of its own: $($r.Sites -join ', ')" }

    # 4. Copy mode: k moves its cursor up, drawn by LBTerm in the key's own frame
    Send-Key $script:w F11
    $sw = Wait-Change { param($s) (Get-Pane $s).title -like 'Copy mode: *' } -What 'copy mode on' -TimeoutSec 10
    $y = (Get-Pane $sw).cursor.y
    $r = Measure-Key { Send-Key $script:w K } { param($x) (Get-Pane $x).cursor.y -lt $y } 'copy mode''s cursor a row up'
    $times['4. copy mode'] = "$($r.Ms) ms"
    Write-Host "4. copy mode, k: its cursor up in frame $($r.Nth) after it, $($r.Ms) ms, asked by $($r.Sites -join ', ')"
    if ($r.Nth -ne 1) { Add-Failure "4. copy mode's cursor moved in frame $($r.Nth) after k, not the first" }
    if (-not (Test-KeySite $r)) { Add-Failure "4. k in copy mode asked for no frame of its own: $($r.Sites -join ', ')" }
    Send-Key $script:w Escape
    Wait-Change { param($s) (Get-Pane $s).title -notlike 'Copy mode: *' } -What 'copy mode left' -TimeoutSec 10 | Out-Null

    # 5. An IME's composition: its end and the key Windows sends with it, then its end alone
    Send-ImeComposing
    $c = Get-DrawnCursor (Get-Pane (Get-StateWindow (Read-State -Window $script:w) -Hwnd ([long]$script:w.Handle)))
    $r = Measure-Key { Send-ImeEnd; Send-Key $script:w X } { param($x) $d = Get-DrawnCursor (Get-Pane $x); $d.x -ne $c.x -or $d.y -ne $c.y } 'the echo of the key after a composition''s end'
    $times['5a. composition, its end and a key'] = "$($r.Ms) ms"
    Write-Host "5a. a composition's end and a key: the echo in frame $($r.Nth) after them, $($r.Ms) ms, asked by $($r.Sites -join ', ')"
    if ($r.Nth -ne 1) { Add-Failure "5a. the echo of the key after a composition's end came in frame $($r.Nth) after them, not the first: the end painted a frame of its own first" }
    if (@($r.Sites | Where-Object { $_ -match 'termwindow[\\/]echo\.rs' }).Count) { Add-Failure "5a. the composition's end asked for its own frame though the key's echo painted: $($r.Sites -join ', ')" }
    Send-ImeComposing
    $r = Measure-AnyFrame { Send-ImeEnd } 'a frame after a composition''s end alone'
    $times['5b. composition, its end alone'] = "$($r.Ms) ms"
    Write-Host "5b. a composition's end alone: its frame $($r.Ms) ms after it, asked by $($r.Sites -join ', ')"
    if (-not @($r.Sites | Where-Object { $_ -match 'termwindow[\\/]echo\.rs' }).Count) { Add-Failure "5b. a composition's end alone painted no frame asked for by termwindow/echo.rs: $($r.Sites -join ', ')" }
    if ($r.Ms -lt 40) { Add-Failure "5b. a composition's end alone painted $($r.Ms) ms after it, before its wait (50 ms) for a key's echo" }
  } finally { $script:rec.Dispose() }
  [IO.File]::WriteAllText((Join-Path $T.Out 'times.json'), ($times | ConvertTo-Json))
} -Passed "echo-frame$(if ($Mux) { ' -Mux' }): each echo in the first frame after its key, a SendString's too, with no frame of the key's own; a scroll back to the bottom and copy mode's cursor in the key's own frame; a composition's end with the next key's echo, and alone a while on"
