#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/control.rs, wezterm-gui/src/termwindow/control.rs, wezterm-gui/src/seat_client.rs, lbterm-extras/src/control.rs, wezterm/src/control.rs, config/src/lbterm.rs
<#
The control pipe end to end (lbterm/CONTROL.md), in a test window of its own class that never
takes focus, through `lbterm control --class <its class>` and through the pipe itself:

1. `focus` names the open tab's pane (the mux server's id, with -Mux), the Claude session in it
   (a program sets the claude_session user var, as claudehook does), and that LBTerm hasn't got
   the keyboard.
2. `emit lbterm-tab-back` and `lbterm-tab-forward` run the config's handlers, which hop tabs as
   Lachlan's lbterm.lua does for the side buttons: the state file shows each hop, round the ends
   too, and `focus` follows.
3. WezTerm's own events, and a class no LBTerm runs as, are refused: exit 1, the reason on stderr.
4. One connection carries several requests, answered in order with their ids, and a line that
   isn't JSON is refused without closing it.

  pwsh -File lbterm\perf\control.ps1          # this tree's build
  pwsh -File lbterm\perf\control.ps1 -Mux     # tabs in a test lbterm-mux-server, as LBTerm runs
#>
param([string]$Bin, [switch]$Mux)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('control' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = $T.Sock
$cfg = Join-Path $T.Out 'config\control.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

$domain = if ($Mux) {
  @"
  unix_domains = { {
    name = 'perfcontrol',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
"@
} else { '' }
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
local act = wezterm.action
wezterm.on('lbterm-tab-back', function(window, pane) window:perform_action(act.ActivateTabRelative(-1), pane) end)
wezterm.on('lbterm-tab-forward', function(window, pane) window:perform_action(act.ActivateTabRelative(1), pane) end)
return {
  automatically_reload_config = false,
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  wsl_domains = {},
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  default_cwd = 'C:/Windows',
$domain}
"@)

# The middle tab's program names a Claude session, as claudehook does at SessionStart
$session = 'perf-control-session'
$claude = @"
`$e = [char]27
`$v = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('$session'))
[Console]::Write("`$e]1337;SetUserVar=claude_session=`$v`$([char]7)")
Start-Sleep -Seconds 120
"@

# `lbterm control --class <class> <args>`: its exit code, its reply and what it said on stderr
function Invoke-Control {
  [CmdletBinding(PositionalBinding = $false)]
  param([Parameter(ValueFromRemainingArguments)][string[]]$Rest, [string]$Class = $T.Class)
  $errFile = Join-Path $T.Out 'control-stderr.txt'
  $out = @(& $T.Cli control --class $Class @Rest 2>$errFile)
  $code = $LASTEXITCODE
  $text = ($out | Out-String).Trim()
  [pscustomobject]@{
    Code  = $code
    Reply = $(if ($text) { $text | ConvertFrom-Json } else { $null })
    Error = ((Get-Content $errFile -Raw -ErrorAction SilentlyContinue) ?? '').Trim()
  }
}

# The open tab's active pane, by the id `lbterm cli` takes (the server's with -Mux)
function Get-OpenPane([int]$TabIndex) {
  $panes = @(Get-Panes -Server:$Mux)
  $tabs = [Collections.Generic.List[object]]::new()
  foreach ($p in $panes) { if (-not $tabs.Contains($p.tab_id)) { $tabs.Add($p.tab_id) } }
  @($panes | Where-Object { $_.tab_id -eq $tabs[$TabIndex] -and $_.is_active })[0].pane_id
}

Invoke-Test {
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfcontrol -Socket $sock -TimeoutSec 15 }
  else { Open-TestWindow -ConfigFile $cfg -Cwd 'C:\Windows' -TimeoutSec 15 }
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15)) { throw "the test mux server at $sock didn't answer" }
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  New-Tab -From $first -Program (New-PwshProgram $claude) -Server:$Mux | Out-Null
  $last = New-Tab -From $first -Program (New-PwshProgram) -Server:$Mux
  Wait-Change { param($x) @($x.tabs).Count -eq 3 } -What 'three tabs' | Out-Null
  # On the mux domain the window doesn't follow a spawn to its tab: open the last one
  Invoke-Cli activate-pane --pane-id $last -Server:$Mux -Check | Out-Null
  Wait-Change { param($x) $x.active_tab -eq 2 } -What 'the last tab open' | Out-Null

  # 1. Where he is
  $r = Invoke-Control focus
  $want = Get-OpenPane 2
  if ($r.Code -ne 0) { Add-Failure "focus failed ($($r.Code)): $($r.Error)" }
  elseif ($r.Reply.pane -ne $want) { Add-Failure "focus named pane $($r.Reply.pane), the open tab's is $want" }
  elseif ($null -ne $r.Reply.session) { Add-Failure "focus named a session in a plain pane: $($r.Reply.session)" }
  elseif ($r.Reply.foreground) { Add-Failure 'focus says LBTerm has the keyboard, but the test window never takes it' }
  else { "focus: pane $want, no session, not in front" }

  # 2. The hops, as the side buttons make them
  $steps = @(
    @{ Event = 'lbterm-tab-back'; Want = 1; Name = 'back' },
    @{ Event = 'lbterm-tab-back'; Want = 0; Name = 'back' },
    @{ Event = 'lbterm-tab-back'; Want = 2; Name = 'back round to the last tab' },
    @{ Event = 'lbterm-tab-forward'; Want = 0; Name = 'forward round to the first tab' },
    @{ Event = 'lbterm-tab-forward'; Want = 1; Name = 'forward' }
  )
  foreach ($step in $steps) {
    $tab = $step.Want
    $r = Invoke-Control emit $step.Event
    if ($r.Code -ne 0 -or $r.Reply.event -ne $step.Event) { Add-Failure "emit $($step.Event) failed ($($r.Code)): $($r.Error)"; continue }
    $landed = { param($x) $x.active_tab -eq $tab }.GetNewClosure()
    try { Wait-Change $landed -What $step.Name -TimeoutSec 5 | Out-Null; "$($step.Name): tab $tab" }
    catch { Add-Failure "$($step.Name): wanted tab $tab, $($_.Exception.Message)" }
  }

  # The Claude tab is open now: focus names its session (a remote pane's user vars arrive a
  # moment after the pane)
  $want = Get-OpenPane 1
  $named = Wait-Until { $script:last = Invoke-Control focus; $script:last.Reply.session -eq $session } -TimeoutSec 5
  if (-not $named) { Add-Failure "focus on the Claude tab named session '$($script:last.Reply.session)', not '$session'" }
  elseif ($script:last.Reply.pane -ne $want) { Add-Failure "focus on the Claude tab named pane $($script:last.Reply.pane), not $want" }
  else { "focus followed the hops: pane $want, session $session" }

  # 3. Refusals
  $r = Invoke-Control emit update-status
  if ($r.Code -ne 1 -or $r.Error -notmatch "WezTerm's own event") { Add-Failure "emit update-status: exit $($r.Code), '$($r.Error)'" }
  else { "emit update-status: refused ($($r.Error))" }
  $r = Invoke-Control focus -Class "com.lachlanb.lbterm.perf-nobody-$PID"
  if ($r.Code -ne 1 -or $r.Error -notmatch 'no LBTerm of class') { Add-Failure "a class nothing runs as: exit $($r.Code), '$($r.Error)'" }
  else { "a class nothing runs as: refused ($($r.Error))" }

  # 4. One connection, several requests
  $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', "lbterm-control-$($T.Class)", [IO.Pipes.PipeDirection]::InOut)
  try {
    $pipe.Connect(5000)
    $writer = [IO.StreamWriter]::new($pipe, [Text.UTF8Encoding]::new($false))
    $writer.AutoFlush = $true
    $reader = [IO.StreamReader]::new($pipe, [Text.UTF8Encoding]::new($false))
    $ask = {
      param([string]$Line)
      $writer.WriteLine($Line)
      $task = $reader.ReadLineAsync()
      if (-not $task.Wait(8000)) { throw "no reply to $Line in 8 s" }
      $task.Result | ConvertFrom-Json
    }
    $a = & $ask '{"id":"a","op":"focus"}'
    $b = & $ask 'not json'
    $c = & $ask '{"id":"c","op":"focus"}'
    if ($a.id -ne 'a' -or -not $a.ok) { Add-Failure "the first request on one connection: $($a | ConvertTo-Json -Compress)" }
    elseif ($b.ok -or $b.error -notmatch '^not a JSON object') { Add-Failure "a line that isn't JSON: $($b | ConvertTo-Json -Compress)" }
    elseif ($c.id -ne 'c' -or -not $c.ok -or $c.pane -ne $a.pane) { Add-Failure "the request after it: $($c | ConvertTo-Json -Compress)" }
    else { 'one connection: two requests answered in order, the line between refused' }
  } finally { $pipe.Dispose() }
} -Passed "control$(if ($Mux) { ' (mux)' }): focus names the pane and its session, emit hops tabs both ways, refusals say why, one connection carries several requests"
