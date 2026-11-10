#Requires -Version 7
# Variant: -ServerBin installed
# Variant: -GuiBin installed
# Covers: wezterm-gui/src/termwindow/panes_drawn.rs, lbterm-extras/src/overlays.rs, wezterm-gui/src/termwindow/mod.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/overlay/**, wezterm-gui/src/termwindow/render/conductor_view.rs, wezterm-client/src/lbterm_drawn.rs, wezterm-client/src/pane/clientpane.rs, wezterm-client/src/pane/renderable.rs, wezterm-mux-server-impl/src/lbterm_flow.rs
<#
A shown pane's pace on a mux domain on every path where no frame reads the pane as it prints
(lbterm/MUX.md, "A shown pane's pace", "What the window does for it, as built"; PITFALLS 246). A
server of this build waits, with no bound, for the window's word that a frame read a shown pane's
last push before it pushes the next (codec's PanesDrawn), so every way a window can show a pane has
to say it: perf/mux-pace.ps1 judges the open tab's frames, and this script the rest, each over a
tab in view printing a numbered line every -EveryMs with a user var a line (`paths`, OSC 1337).

Phases, each -Samples samples long:
1. normal: the tab in view, nothing over it (the baseline).
2. copy mode, 3. search, 4. quick select: pane overlays that read the pane under them
   (`assign_overlay_for_pane`; they report its id and their frames read its lines through them),
   each opened through a Lua emit; the frame says the pane under it (panes_drawn.rs,
   `lbterm_under_overlay`). Open when the frame's pane is the overlay (the state file's
   `server_id` null: an overlay has no `remote_pane_id`).
5. the close-pane confirmation (CloseCurrentPane with confirm): a pane overlay of its own, with its
   own id; the window says each push as it comes (`Shows::Overlaid`). Open when the state file
   names an `overlay`; closed by a posted Escape, which answers no.
6. the launcher: a tab overlay; said as it comes (`Shows::Covered`). Closed by a posted Escape.
7. the Conductor's view, its config naming a seat no one answers on; said as it comes
   (`lbterm_view_is_open`). Open while the state file's `conductor_view` isn't null.
8. minimized: a window that can't be seen; said as it comes (Lachlan's 4a). Minimized and restored
   with ShowWindow's no-activate shows (Set-WindowMinimized).

In each phase, every sample reads how many lines the program had written, then the window's copy of
the pane (`cli get-text` through the window's own socket) and the newest `paths` user var the
window's Lua heard (`user-var-changed`, written to a file): each may be at most -MaxLag lines behind.
Then the program is paused, its last line and a title of the phase's own (`paths-paused-<n>`)
written while the path is still open, the path is closed, and a frame must draw the pane itself
with that title within -SettleMs: the last output reaches a frame once the path ends. In this
build's pair each phase must have sent PanesDrawn (the word was said, by a frame or as the push
came); with an older build either side none may wait (perf/mux-pace.ps1 judges the pairings' pace).

mux-pace-paths.json in the output folder has every number.

  pwsh -File lbterm\perf\mux-pace-paths.ps1 [-EveryMs 30] [-Samples 10] [-MaxLag 20] [-SettleMs 1500] [-Only copy,search,...] [-ServerBin <build>|installed] [-GuiBin <build>|installed] [-Bin <build>]
#>
param(
  [string]$Bin,
  [string]$ServerBin,
  [string]$GuiBin,
  [int]$EveryMs = 30,
  [int]$Samples = 10,
  [int]$GapMs = 120,
  [int]$MaxLag = 20,
  [int]$SettleMs = 1500,
  [string[]]$Only,
  [string]$Log
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mux-pace-paths' -Bin $Bin
# The installed build's $Files, copied into the test's folder: a server or window running from the
# install folder would be taken for the daily one (the Warden's registry and `lbterm.ps1 logs` go
# by the exe's folder)
function Copy-Installed([string]$Name, [string[]]$Files) {
  $dir = Join-Path $T.Out "installed-$Name"
  New-Item -ItemType Directory -Force $dir | Out-Null
  foreach ($f in $Files) { Copy-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA "Programs\LBTerm\$f") $dir }
  $dir
}
if ($ServerBin -eq 'installed') { $ServerBin = Copy-Installed 'server' 'lbterm-mux-server.exe', 'conpty.dll', 'OpenConsole.exe' }
if ($GuiBin -eq 'installed') { $GuiBin = Copy-Installed 'gui' 'lbterm-gui.exe', 'conpty.dll', 'OpenConsole.exe', 'libEGL.dll', 'libGLESv2.dll', 'mesa' }
$serverExe = if ($ServerBin) { Join-Path (Resolve-Path $ServerBin).Path 'lbterm-mux-server.exe' } else { $T.MuxServer }
if (-not (Test-Path $serverExe)) { throw "no lbterm-mux-server.exe in $ServerBin" }
$guiExe = if ($GuiBin) { Join-Path (Resolve-Path $GuiBin).Path 'lbterm-gui.exe' } else { $T.Gui }
if (-not (Test-Path $guiExe)) { throw "no lbterm-gui.exe in $GuiBin" }
$Pairing = if ($GuiBin -and $ServerBin) { 'other window, other server' } elseif ($GuiBin) { 'other window' } elseif ($ServerBin) { 'other server' } else { 'this build' }
$OwnPair = $Pairing -eq 'this build'

# The tab: a numbered line and a user var each -EveryMs, its count in a file; paused at pause-<n>
# with its last line and the title paths-paused-<n> (the time written in paused-<n>), on again at
# resume-<n>
$Dir = $T.Out
$Count = Join-Path $Dir 'count.txt'
$Heard = Join-Path $Dir 'heard.txt'
$Busy = New-PwshProgram -Script (@'
$e = [char]27; $bel = [char]7
[Console]::Out.Write("$e]2;paths-busy$bel")
$k = 1
for ($i = 0; ; $i++) {
  if (Test-Path "XDIRX\pause-$k") {
    [Console]::Out.Write("paths paused $k at line $i`r`n$e]2;paths-paused-$k$bel")
    [IO.File]::WriteAllText("XDIRX\paused-$k", "$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())")
    while (-not (Test-Path "XDIRX\resume-$k")) { [Threading.Thread]::Sleep(10) }
    $k++
    continue
  }
  $v = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("$i"))
  [Console]::Out.Write("pace line $i`r`n$e]1337;SetUserVar=paths=$v$bel")
  [IO.File]::WriteAllText('XCOUNTX', "$i")
  [Threading.Thread]::Sleep(XEVERYX)
}
'@ -replace 'XDIRX', $Dir -replace 'XCOUNTX', $Count -replace 'XEVERYX', $EveryMs)

$cfg = Join-Path $T.Out 'config\mux-pace-paths.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
local act = wezterm.action
wezterm.on('paths-copy', function(window, pane) window:perform_action(act.ActivateCopyMode, pane) end)
wezterm.on('paths-search', function(window, pane) window:perform_action(act.Search 'CurrentSelectionOrEmptyString', pane) end)
wezterm.on('paths-copy-close', function(window, pane) window:perform_action(act.CopyMode 'Close', pane) end)
wezterm.on('paths-quick', function(window, pane) window:perform_action(act.QuickSelect, pane) end)
wezterm.on('paths-confirm', function(window, pane) window:perform_action(act.CloseCurrentPane { confirm = true }, pane) end)
wezterm.on('paths-launcher', function(window, pane) window:perform_action(act.ShowLauncher, pane) end)
wezterm.on('paths-view', function(window, pane) window:perform_action(act.LbtermConductorView 'Show', pane) end)
wezterm.on('paths-view-hide', function(window, pane) window:perform_action(act.LbtermConductorView 'Hide', pane) end)
-- The newest paths user var the window heard
wezterm.on('user-var-changed', function(window, pane, name, value)
  if name == 'paths' then
    local f = io.open([[$Heard]], 'w')
    if f then f:write(value) f:close() end
  end
end)
-- The Conductor's view opens over a seat no one answers on, the Conductor off
rawset(config, 'lbterm_conductor', {
  state_file = '$(ConvertTo-LuaPath (Join-Path $T.Out 'no-conductor.json'))',
  seat_pipe = [[\\.\pipe\lbterm-perf-noseat-$PID]],
})
config.initial_cols = 100
config.initial_rows = 30
config.enable_scroll_bar = false
config.unix_domains = { {
  name = 'pathsmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $serverExe)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

# A PDU's count in a metrics reply, sent or received
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}
# A small file another process writes as a whole, read as a number (null while it's being written)
function Read-Number([string]$Path) {
  for ($k = 0; $k -lt 50; $k++) {
    try {
      $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
      try { $t = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
      if ($t.Trim() -match '^\d+$') { return [long]$t.Trim() }
    } catch { }
    Start-Sleep -Milliseconds 2
  }
  $null
}
# How far behind the program the window's copy of the pane and the user var it heard are, in lines
function Get-Lag([int]$GuiId) {
  $written = Read-Number $Count
  $text = @(Invoke-Cli get-text --pane-id $GuiId)
  $lines = @($text | ForEach-Object { if ($_ -match 'pace line (\d+)') { [long]$Matches[1] } } | Sort-Object)
  $newest = if ($lines) { $lines[-1] } else { $null }
  $heard = Read-Number $Heard
  [pscustomobject]@{
    Written = $written
    Copy = $newest
    CopyLag = if ($null -ne $newest -and $null -ne $written) { $written - $newest } else { $null }
    Heard = $heard
    HeardLag = if ($null -ne $heard -and $null -ne $written) { $written - $heard } else { $null }
  }
}
function Get-ConnectionLines([string]$Path, [long]$From = 0) {
  if (-not $Path -or -not (Test-Path -LiteralPath $Path)) { return @() }
  $fs = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite, Delete')
  try { [void]$fs.Seek($From, 'Begin'); $text = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
  @($text -split "`r?`n" | Where-Object { $_ -match 'the connection to the mux server ended|client disconnected|Reconnected!|reattaching after a reconnect|detached domain' })
}
# The printing pane as a frame drew it: the pane itself, not an overlay in its place
function Get-BusyPane($sw, [int]$Rid) { @($sw.panes | Where-Object { $null -ne $_.server_id -and [int]$_.server_id -eq $Rid })[0] }

$results = [ordered]@{ Pairing = $Pairing; Window = $guiExe; Server = $serverExe; Locked = (Test-WorkstationLocked); Phases = @() }

Invoke-Test {
  $open = @{ ConfigFile = $cfg; Domain = 'pathsmux'; Socket = $T.Sock; TimeoutSec = 30; Exe = $guiExe }
  if ($Log) { $open.Log = $Log }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  # The tabs are built on the server, whose first tab runs its own default program (PITFALLS 202)
  $shell = $panes[0].pane_id
  $rid = New-Tab -From $shell -Program $Busy -Server
  Wait-State { param($s) @($s.tabs).Count -eq 2 } -What 'the printing tab in the window' -TimeoutSec 30 | Out-Null
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 1 } -What "the one tab once the shell's gone" -TimeoutSec 30 | Out-Null
  Wait-State { param($sw) [bool](Get-BusyPane $sw $rid) }.GetNewClosure() -What 'the printing tab drawn' -TimeoutSec 15 | Out-Null
  if (-not (Wait-Until { Test-Path -LiteralPath $Count } -TimeoutSec 15)) { throw 'the printing tab never wrote its count' }
  $guiId = @(Get-Panes | Where-Object title -like 'paths-*')[0].pane_id
  if ($null -eq $guiId) { throw "the window's own socket lists no paths-busy pane: $(@(Get-Panes) | ConvertTo-Json -Compress)" }
  $h = Resolve-Handle $w
  $hasMetrics = $true
  try { Get-Metrics $w -Reset -TimeoutMs 4000 | Out-Null } catch { $hasMetrics = $false }
  "the printing pane: $rid on the server, $guiId in the window; pairing: $Pairing"

  # Each phase: its name, how it opens and closes, and how a frame shows it open
  $overlayUp = { param($sw) $p = @($sw.panes)[0]; $p -and $null -eq $p.server_id }
  $phases = @(
    @{ Key = 'normal'; Name = 'the tab in view'; Open = {}; Close = {}; IsOpen = { param($sw) [bool](Get-BusyPane $sw $rid) }.GetNewClosure() }
    @{ Key = 'copy'; Name = 'copy mode (a pane overlay reading the pane)'; Open = { Send-Emit 'paths-copy' -Window $w }; Close = { Send-Emit 'paths-copy-close' -Window $w }; IsOpen = $overlayUp }
    @{ Key = 'search'; Name = 'search (a pane overlay reading the pane)'; Open = { Send-Emit 'paths-search' -Window $w }; Close = { Send-Emit 'paths-copy-close' -Window $w }; IsOpen = $overlayUp }
    @{ Key = 'quick'; Name = 'quick select (a pane overlay reading the pane)'; Open = { Send-Emit 'paths-quick' -Window $w }; Close = { Send-Key $w Escape }; IsOpen = $overlayUp }
    @{ Key = 'confirm'; Name = 'the close-pane confirmation (a pane overlay of its own)'; Open = { Send-Emit 'paths-confirm' -Window $w }; Close = { Send-Key $w Escape }; IsOpen = { param($sw) $null -ne $sw.overlay } }
    @{ Key = 'launcher'; Name = 'the launcher (a tab overlay)'; Open = { Send-Emit 'paths-launcher' -Window $w }; Close = { Send-Key $w Escape }; IsOpen = { param($sw) $null -ne $sw.overlay } }
    @{ Key = 'view'; Name = "the Conductor's view"; Open = { Send-Emit 'paths-view' -Window $w }; Close = { Send-Emit 'paths-view-hide' -Window $w }; IsOpen = { param($sw) $null -ne $sw.conductor_view } }
    @{ Key = 'minimized'; Name = 'a minimized window'; Open = { if (-not (Set-WindowMinimized $w)) { throw 'the window would not minimize' } }; Close = { Set-WindowMinimized $w -Restore | Out-Null }; IsOpen = $null }
  )
  if ($Only) { $keys = @($Only | ForEach-Object { $_ -split ',' }); $phases = @($phases | Where-Object { $_.Key -in $keys }) }

  $n = 0
  foreach ($ph in $phases) {
    $n++
    $phase = [ordered]@{ Phase = $ph.Key; Name = $ph.Name }
    & $ph.Open
    Watch-Foreground
    # Open: a frame shows it (a minimized window draws once a second, so it's asked of Windows)
    if ($ph.IsOpen) {
      try {
        Wait-State $ph.IsOpen -What "$($ph.Name) open" -TimeoutSec 5 | Out-Null
        $phase.Opened = $true
      } catch {
        $phase.Opened = $false
        Add-Failure "$($ph.Name): never opened (the last frame: $(Format-StateWindow (Get-StateWindow (Read-State) -Hwnd ([long]$h))))"
      }
    } else { $phase.Opened = $true }
    Start-Sleep -Milliseconds 300
    $m0 = if ($hasMetrics) { try { Get-Metrics $w -Reset -TimeoutMs 4000 } catch { $null } }
    $span = [Diagnostics.Stopwatch]::StartNew()
    $rows = @(for ($i = 0; $i -lt $Samples; $i++) { Get-Lag $guiId; Start-Sleep -Milliseconds $GapMs })
    $secs = $span.Elapsed.TotalSeconds
    $m = if ($hasMetrics) { try { Get-Metrics $w -TimeoutMs 4000 } catch { $null } }
    $copyLags = @($rows | ForEach-Object CopyLag)
    $heardLags = @($rows | ForEach-Object HeardLag)
    $phase.Seconds = [Math]::Round($secs, 2)
    $phase.CopyLag = $copyLags
    $phase.HeardLag = $heardLags
    $phase.PushesPerSec = if ($m) { [Math]::Round((Get-Pdus $m 'GetPaneRenderChangesResponse' 'received') / $secs, 1) } else { $null }
    $phase.PanesDrawn = if ($m) { Get-Pdus $m 'PanesDrawn' 'sent' } else { $null }
    $badCopy = @($copyLags | Where-Object { $null -eq $_ -or $_ -gt $MaxLag })
    $badHeard = @($heardLags | Where-Object { $null -eq $_ -or $_ -gt $MaxLag })
    if ($badCopy) { Add-Failure "$($ph.Name): the window's copy of the pane fell behind the program: lines behind at each look $($copyLags -join ' ') (at most $MaxLag)" }
    if ($badHeard) { Add-Failure "$($ph.Name): the paths user var the window's Lua heard fell behind: lines behind at each look $($heardLags -join ' ') (at most $MaxLag)" }
    if ($OwnPair -and $m -and -not $phase.PanesDrawn) { Add-Failure "$($ph.Name): the window sent no PanesDrawn while the pane printed: nothing said it read" }
    # The last output while it's open, then closed: a frame of the pane itself draws it
    [IO.File]::WriteAllText((Join-Path $Dir "pause-$n"), '')
    $pausedFile = Join-Path $Dir "paused-$n"
    Wait-Until { Test-Path -LiteralPath $pausedFile } -TimeoutSec 5 -IntervalMs 5 | Out-Null
    $pausedMs = Read-Number $pausedFile
    & $ph.Close
    Watch-Foreground
    $title = "paths-paused-$n"
    $phase.LastDrawnMs = $null
    try {
      Wait-State { param($sw) $p = Get-BusyPane $sw $rid; $p -and $p.title -eq $title }.GetNewClosure() -What "$($ph.Name): its last output in a frame" -TimeoutSec ([Math]::Max(2, $SettleMs / 1000 + 1)) -IntervalMs 5 | Out-Null
      if ($pausedMs) { $phase.LastDrawnMs = [long](Get-Test).LastState.time_ms - $pausedMs }
    } catch { }
    if ($null -eq $phase.LastDrawnMs) {
      $last = Get-BusyPane (Get-StateWindow (Read-State) -Hwnd ([long]$h)) $rid
      Add-Failure "$($ph.Name): once it closed, no frame drew the pane's last output (title $title) within $SettleMs ms; the last frame's pane: $(if ($last) { "title '$($last.title)'" } else { 'not the pane itself' })"
    }
    [IO.File]::WriteAllText((Join-Path $Dir "resume-$n"), '')
    $results.Phases += [pscustomobject]$phase
    "$($ph.Name): $($phase.Seconds) s, $($phase.PushesPerSec) pushes/s, $($phase.PanesDrawn) PanesDrawn sent; the copy behind by $($copyLags -join ' '); the user var heard behind by $($heardLags -join ' '); last output drawn $(if ($null -ne $phase.LastDrawnMs) { "$($phase.LastDrawnMs) ms after it was written (the path closed in between)" } else { 'NEVER' })"
    Start-Sleep -Milliseconds 200
  }
  $results.Connection = @(Get-ConnectionLines $w.Log $w.LogStart)
  if ($results.Connection.Count) { Add-Failure "the window's connection ended: $($results.Connection -join ' | ')" }
  $results | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'mux-pace-paths.json')
} -Passed "mux-pace-paths ($Pairing): over every path where no frame reads a printing pane in view (copy mode, search, quick select, the close confirmation, the launcher, the Conductor's view, a minimized window) the window's copy and the user vars it heard kept up, and each path's last output reached a frame once it closed"
