#Requires -Version 7
# Covers: wezterm-client/src/lbterm_current.rs, wezterm-client/src/client.rs, wezterm-client/src/lbterm_backlog.rs, wezterm-client/src/pane/clientpane.rs, mux/src/pane.rs, lua-api-crates/mux/src/pane.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-gui/src/control.rs, wezterm-gui/src/termwindow/control.rs, lbterm-extras/src/drafts.rs, config/src/lbterm.rs
<#
Reads of a tab out of view whose server holds its output (Lachlan's 6b of 2026-11-08; lbterm/MUX.md,
"What reads a held pane's lines"): each asks the server for the pane's push first, so it reads the
pane as its program has it, not the window's copy as last pushed.

Every tab is in a test mux server of its own (a socket under the test's folder; never Lachlan's):
a quiet tab in view, a busy one out of view printing `held line <n>` every -EveryMs and writing
<n> to a file after each, so its server holds it from its first push on (PaneHeld), and a
stand-in Claude Code out of view (fake-prompt.ps1) showing a permission prompt.

1. Lua: an event run through the control pipe (`lbterm control emit`) reads the busy tab with
   `pane:get_dimensions()`, `get_lines_as_text()` and `get_cursor_position()` and logs what it
   read: its newest line must be no older than the line before the one the tab's file said as the
   event was asked (the server's parse can trail the program's write by a line), and the cursor
   on the row after it. The window's log says the read asked and how long it waited.
2. cli: `lbterm cli get-text` through the window's own socket (gui-sock) reads the busy tab, held
   again by then: its newest line, the same way.
3. A voice draft: the stand-in shows its permission prompt and goes quiet, so the window gets that
   screen; then its input box, with its status line turning every 50 ms as Claude Code's does at
   work, so its server holds it there and the window's copy still shows the permission prompt. A
   text draft sent to it by id through the control pipe (`draft --pane <id> send`) must be
   checked against the input box and typed, its keys reaching the stand-in: on a window that read
   the copy, the draft was refused as the prompt not showing.

Each read must have been of a held pane (the window's log says it asked), so a run where the
server never held the tabs fails as unmeasured rather than passing. The time each read waited is
in the log line, and in the results.

  pwsh -File lbterm\perf\held-reads.ps1 [-Bin <dir>] [-EveryMs 30]
#>
param(
  [string]$Bin,
  [int]$EveryMs = 30,
  [string]$Log
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'held-reads' -Bin $Bin

$Count = Join-Path $T.Out 'busy-count.txt'
$Mode = Join-Path $T.Out 'mode.txt'
$Keys = Join-Path $T.Out 'keys.txt'
[IO.File]::WriteAllText($Mode, 'permission')
[IO.File]::WriteAllText($Keys, '')
$Sid = '7c2d9e41-5b3a-4f60-8e17-2a9d4c6b1f03'
# Not $Draft: PowerShell's names are case-insensitive, and step 3's $draft is its results (PITFALLS 206)
$DraftText = 'yes, go ahead'

$Busy = New-PwshProgram -Script (@'
[Console]::Out.Write("$([char]27)]2;held-busy$([char]7)")
for ($i = 0; ; $i++) {
  [Console]::Out.Write("held line $i`r`n")
  [IO.File]::WriteAllText('XCOUNTX', "$i")
  [Threading.Thread]::Sleep(XEVERYX)
}
'@ -replace 'XCOUNTX', $Count -replace 'XEVERYX', $EveryMs)
$Quiet = New-PwshProgram -Script 'Start-Sleep 900'

$cfg = Join-Path $T.Out 'config\held-reads.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
-- 1. Lua: the busy tab read as Lua reads a pane, and what it read in the window's log
wezterm.on('held-reads-lua', function(window, pane)
  for _, tab in ipairs(window:mux_window():tabs()) do
    local p = tab:active_pane()
    if p:get_title() == 'held-busy' then
      local dims = p:get_dimensions()
      local text = p:get_lines_as_text()
      local c = p:get_cursor_position()
      local lines = {}
      for l in (text .. '\n'):gmatch('(.-)\n') do table.insert(lines, l) end
      local last, at = '', -1
      for i = #lines, 1, -1 do
        if lines[i]:match('%S') then last = lines[i]; at = i - 1; break end
      end
      wezterm.log_error('held-reads lua: top=' .. dims.physical_top .. ' cursor=' .. c.x .. ',' .. c.y .. ' at=' .. at .. ' last=' .. last)
      return
    end
  end
  wezterm.log_error('held-reads lua: no held-busy tab')
end)
config.initial_cols = 100
config.initial_rows = 30
config.enable_scroll_bar = false
config.unix_domains = { {
  name = 'heldmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@

# A PDU's count in a metrics reply, sent or received
function Get-Pdus($m, [string]$Name, [string]$Way) {
  $e = @($m.mux | Where-Object pdu -eq $Name)[0]
  if ($e) { [int]$e.$Way } else { 0 }
}
function Get-LogLength($w) { if ($w.Log -and (Test-Path -LiteralPath $w.Log)) { (Get-Item -LiteralPath $w.Log).Length } else { 0 } }
# The window's log from $From (its length then), as lines
function Get-LogSince($w, [long]$From) {
  if (-not $w.Log -or -not (Test-Path -LiteralPath $w.Log)) { return @() }
  $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete')
  try {
    [void]$fs.Seek($From, 'Begin')
    $text = [IO.StreamReader]::new($fs).ReadToEnd()
  } finally { $fs.Dispose() }
  @($text -split "`r?`n")
}
# The window's word on a read of the server's pane $Rid since $From: how it ended and in how many ms
function Get-ReadLine($w, [long]$From, [int]$Rid) {
  $l = @(Get-LogSince $w $From | Where-Object { $_ -match "a read of held pane \d+ \(the server's $Rid\): (.+) in ([\d.]+) ms" })[0]
  if ($l -and $l -match "a read of held pane \d+ \(the server's $Rid\): (.+) in ([\d.]+) ms") { [ordered]@{ How = $Matches[1]; Ms = [double]$Matches[2] } }
}
# The program's count, opened letting others write it (ReadAllText lets others only read, so it
# throws while the program has the file open to write it: the e2e of 2026-11-09 05:44), and read
# again while it's half written
function Get-Count {
  for ($k = 0; $k -lt 50; $k++) {
    try {
      $fs = [IO.File]::Open($Count, 'Open', 'Read', 'ReadWrite, Delete')
      try { $t = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
      if ($t.Trim() -match '^\d+$') { return [int]$t.Trim() }
    } catch { }
    Start-Sleep -Milliseconds 2
  }
  throw "the busy tab's count in $Count couldn't be read"
}
# `lbterm control --class <its class> <args>`: its exit code and its reply
function Invoke-Control {
  [CmdletBinding(PositionalBinding = $false)]
  param([Parameter(ValueFromRemainingArguments)][string[]]$Rest)
  $errFile = Join-Path $T.Out 'control-stderr.txt'
  $out = @(& $T.Cli control --class $T.Class @Rest 2>$errFile)
  $text = ($out | Out-String).Trim()
  [pscustomobject]@{
    Code  = $LASTEXITCODE
    Reply = $(if ($text) { $text | ConvertFrom-Json } else { $null })
    Error = ((Get-Content $errFile -Raw -ErrorAction SilentlyContinue) ?? '').Trim()
  }
}
# The stand-in's keys from the file, as text (a CR as <enter>)
function Get-Typed {
  (@(Get-Content -LiteralPath $Keys -ErrorAction SilentlyContinue | Where-Object { $_ }) | ForEach-Object {
    $k = [int]($_ -replace '^M-', '')
    if ($k -eq 13) { '<enter>' } else { [string][char]$k }
  }) -join ''
}

$results = [ordered]@{ Window = $T.Gui; Locked = (Test-WorkstationLocked); Lua = $null; Cli = $null; Draft = $null }

Invoke-Test {
  $logOpen = if ($Log) { @{ Log = "info,$Log" } } else { @{} }
  $w = Open-TestWindow @logOpen -ConfigFile $cfg -Domain 'heldmux' -Socket $T.Sock -TimeoutSec 30
  Wait-State { param($s) $s.frame -gt 0 } -What "the window's first frame" -TimeoutSec 30 | Out-Null
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list on the test server has no panes; the window's log: $($w.Log)" }
  # The tabs built on the server, the first tab's shell gone (PITFALLS 202)
  $shell = $panes[0].pane_id
  $quietId = New-Tab -From $shell -Program $Quiet -Server
  $busyId = New-Tab -From $shell -Program $Busy -NoActivate -Server
  $promptId = New-Tab -From $shell -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-prompt.ps1') -ArgumentList '-Sid', $Sid, '-Mode', $Mode, '-Keys', $Keys, '-TickMs', '50') -NoActivate -Server
  Invoke-Cli kill-pane --pane-id $shell -Server -Check | Out-Null
  Wait-State { param($s) @($s.tabs).Count -eq 3 } -What 'the three tabs in the window' -TimeoutSec 30 | Out-Null
  Invoke-Cli activate-pane --pane-id $quietId -Server -Check | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.active -and $_.server_id -eq $quietId }).Count -eq 1 -and -not $s.animating } -What 'the quiet tab in view' -TimeoutSec 15 | Out-Null
  if (-not (Wait-Until { Test-Path -LiteralPath $Count } -TimeoutSec 15)) { throw 'the busy tab never wrote its count' }
  $setup = Get-Metrics $w -Reset
  # Its permission prompt quiet long enough for its push (HELD_QUIET), then the busy tab well on
  Start-Sleep 2
  $m = Get-Metrics $w -Reset
  $held = (Get-Pdus $setup 'PaneHeld' 'received') + (Get-Pdus $m 'PaneHeld' 'received')
  "held at the start: $held PaneHeld taken"

  # 1. Lua
  $at = Get-LogLength $w
  $before = Get-Count
  $r = Invoke-Control emit held-reads-lua
  if ($r.Code -ne 0) { throw "emit held-reads-lua: exit $($r.Code) $($r.Error)" }
  $line = $null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $line -and $clock.Elapsed.TotalSeconds -lt 10) {
    $line = @(Get-LogSince $w $at | Where-Object { $_ -match 'held-reads lua: ' })[0]
    if (-not $line) { Start-Sleep -Milliseconds 100 }
  }
  if ($line) {
    $lua = [ordered]@{ CountAtAsk = $before; Line = ($line -replace '^.*held-reads lua: ', '') }
    if ($line -match 'top=(-?\d+) cursor=(\d+),(-?\d+) at=(-?\d+) last=held line (\d+)') {
      $lua.Top = [int]$Matches[1]; $lua.CursorX = [int]$Matches[2]; $lua.CursorY = [int]$Matches[3]; $lua.At = [int]$Matches[4]; $lua.Newest = [int]$Matches[5]
    }
    $lua.Read = Get-ReadLine $w $at $busyId
    $results.Lua = $lua
    "lua: the tab's file said line $before as it was asked; Lua read $($lua.Line); the window: $(if ($lua.Read) { "$($lua.Read.How) in $($lua.Read.Ms) ms" } else { 'no read of a held pane logged' })"
  }
  else { "lua: no line from the event in 10 s" }

  # 2. cli get-text through the window's own socket, the tab held again by now
  Start-Sleep -Milliseconds 400
  $guiBusy = @(Get-Panes | Where-Object title -eq 'held-busy')[0].pane_id
  $at = Get-LogLength $w
  $before = Get-Count
  $text = @(Invoke-Cli get-text --pane-id $guiBusy)
  $newest = @($text | Where-Object { $_ -match 'held line (\d+)' } | ForEach-Object { [int]([regex]::Match($_, 'held line (\d+)').Groups[1].Value) } | Sort-Object)[-1]
  Start-Sleep -Milliseconds 300
  $results.Cli = [ordered]@{ CountAtAsk = $before; Newest = $newest; Read = Get-ReadLine $w $at $busyId }
  "cli get-text: the tab's file said line $before as it was asked; get-text's newest was line $newest; the window: $(if ($results.Cli.Read) { "$($results.Cli.Read.How) in $($results.Cli.Read.Ms) ms" } else { 'no read of a held pane logged' })"

  # 3. A voice draft to the stand-in out of view, its input box held at its server
  if (-not (Wait-Until { (Invoke-Control draft --pane $promptId get).Code -eq 0 } -TimeoutSec 10)) { throw 'the window never answered draft get for the stand-in' }
  $m = Get-Metrics $w -Reset
  [IO.File]::WriteAllText($Mode, 'prompt')
  Start-Sleep -Milliseconds 800
  $m = Get-Metrics $w -Reset
  $draft = [ordered]@{ HeldAfterTheBox = Get-Pdus $m 'PaneHeld' 'received' }
  $add = Invoke-Control draft --pane $promptId add $DraftText
  $at = Get-LogLength $w
  $send = Invoke-Control draft --pane $promptId send
  $draft.Added = $add.Code -eq 0
  $draft.SendCode = $send.Code
  $draft.Sent = $send.Reply.sent
  $draft.Refused = if ($send.Code -ne 0) { if ($send.Reply.error) { $send.Reply.error } else { $send.Error } }
  Wait-Until { (Get-Typed) -like "*$DraftText<enter>*" } -TimeoutSec 5 | Out-Null
  $draft.Typed = Get-Typed
  $draft.Read = Get-ReadLine $w $at $promptId
  $results.Draft = $draft
  "draft: $(if ($draft.Sent) { "sent '$($draft.Sent)'" } else { "refused: $($draft.Refused)" }); the stand-in got '$($draft.Typed)'; the window: $(if ($draft.Read) { "$($draft.Read.How) in $($draft.Read.Ms) ms" } else { 'no read of a held pane logged' })"

  $results | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $T.Out 'results.json')

  $lua = $results.Lua
  if (-not $lua) { Add-Failure "Lua's read logged nothing" }
  elseif ($null -eq $lua.Newest) { Add-Failure "Lua's read wasn't of the busy tab's lines: $($lua.Line)" }
  else {
    if (-not $lua.Read) { Add-Failure "unmeasured: Lua's read of the busy tab didn't ask its server (the window logged no read of a held pane): was it held?" }
    elseif ($lua.Read.How -ne 'its push brought') { Add-Failure "Lua's read of the held tab: $($lua.Read.How) in $($lua.Read.Ms) ms" }
    if ($lua.Newest -lt $lua.CountAtAsk - 1) { Add-Failure "Lua read line $($lua.Newest) as the newest, where the tab had written line $($lua.CountAtAsk) when it was asked: the window's copy as last pushed" }
    if ($lua.CursorX -ne 0 -or $lua.CursorY -ne $lua.Top + $lua.At + 1) { Add-Failure "Lua's cursor $($lua.CursorX),$($lua.CursorY) isn't at the start of the row after its newest line (row $($lua.Top + $lua.At + 1))" }
  }
  $cli = $results.Cli
  if ($null -eq $cli.Newest) { Add-Failure "cli get-text through the window's socket read no line of the busy tab" }
  else {
    if (-not $cli.Read) { Add-Failure "unmeasured: cli get-text's read didn't ask the server (the window logged no read of a held pane): was it held?" }
    elseif ($cli.Read.How -ne 'its push brought') { Add-Failure "cli get-text's read of the held tab: $($cli.Read.How) in $($cli.Read.Ms) ms" }
    if ($cli.Newest -lt $cli.CountAtAsk - 1) { Add-Failure "cli get-text through the window's socket read line $($cli.Newest) as the newest, where the tab had written line $($cli.CountAtAsk) when it was asked: the window's copy as last pushed" }
  }
  if (-not $draft.HeldAfterTheBox) { Add-Failure "unmeasured: no PaneHeld came once the stand-in drew its input box, so the draft's read wasn't of a held pane" }
  if (-not $draft.Added) { Add-Failure "draft add to the stand-in failed: $($add.Error)" }
  elseif (-not $draft.Sent) { Add-Failure "the draft to the stand-in out of view was refused ($($draft.Refused)): it was checked against the window's copy, the permission prompt, not the input box its server had" }
  elseif ($draft.Typed -notlike "*$DraftText<enter>*") { Add-Failure "the draft sent, but the stand-in got '$($draft.Typed)'" }
  if ($draft.Sent -and -not $draft.Read) { Add-Failure "the draft's read didn't ask the server (the window logged no read of a held pane)" }
} -Passed "held-reads: Lua, cli get-text through the window's socket and a voice draft each read a tab its server holds as its program has it, after asking for its push"
