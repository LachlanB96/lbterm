#Requires -Version 7
# Covers: wezterm-client/src/domain.rs, lua-api-crates/mux/src/pane.rs, mux/src/tab.rs
<#
A split made from Lua on the mux domain leaves the keyboard where Lua puts it. Terminal's
summary pane is opened this way (lbterm.lua's claude_spawn_summary handler): a 1-row split
under the Claude pane, then pane:activate() on the Claude pane, so nothing typed lands in the
summary. With the tabs in lbterm-mux-server, the server made the new pane active without the
GUI's client knowing, so the activate() never reached it and the next resync handed the
keyboard to the summary pane (fixed 2026-10-20 in wezterm-client/src/domain.rs).

In a test window of its own class, on a domain and socket of its own (target\perf\split-focus),
so the real server is never touched:

1. The first pane sets the user var split_back three times; the handler splits a row under it
   and activates it again, as lbterm.lua does. The first pane must stay active, on the server
   and in the GUI (the active pane its state file names, by the server's id).
2. It sets split_only: a split with no activate() after it. The new pane must be active, as
   any split leaves it.

  pwsh -File lbterm\perf\split-focus.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'split-focus' -Bin $Bin
$sock = Join-Path $T.Out 'sock'
$cfg = Join-Path $T.Out 'config\split.lua'
$emit = Join-Path $T.Out 'emit.ps1'
$ask = Join-Path $T.Out 'ask.txt'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null

# The server reads the same file (serve_command), so both know the test domain
[IO.File]::WriteAllText($cfg, @"
local wezterm = require 'wezterm'
wezterm.on('user-var-changed', function(window, pane, name, value)
  if name ~= 'split_back' and name ~= 'split_only' then return end
  pcall(function()
    pane:split { direction = 'Bottom', size = 1,
                 args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-Command', 'Start-Sleep 600' } }
  end)
  -- What lbterm.lua's summary-pane handler does: focus straight back, in the same callback
  if name == 'split_back' then pcall(function() pane:activate() end) end
end)
return {
  automatically_reload_config = false,
  front_end = 'WebGpu',
  window_close_confirmation = 'NeverPrompt',
  lbterm_crash_restore = false,
  default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile', '-File', '$(ConvertTo-LuaPath $emit)' },
  default_cwd = 'C:/Windows',
  unix_domains = { {
    name = 'perfsplit',
    socket_path = '$(ConvertTo-LuaPath $sock)',
    serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
  } },
}
"@)

# The first pane: asks for a split by setting a user var each time a file names one
[IO.File]::WriteAllText($emit, @'
$e = [char]27; $bel = [char]7
$ask = Join-Path $PSScriptRoot 'ask.txt'
$n = 0
while ($true) {
  if (Test-Path $ask) {
    $name = (Get-Content $ask -Raw).Trim(); Remove-Item $ask
    $n++
    [Console]::Write("$e]1337;SetUserVar=$name=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("$n")))$bel")
    "asked for $name ($n)"
  }
  Start-Sleep -Milliseconds 100
}
'@)

# The pane the GUI gives the keyboard, by the server's id, as its state file has it
function Get-GuiActive {
  $x = Get-StateWindow (Read-State) -Hwnd ([long]$T.Current.Handle)
  @($x.panes | Where-Object { $_.id -eq $x.active_pane })[0].server_id
}

# Asks the first pane for a split, then waits until the server and the GUI both have $count
# panes and a moment more for the resyncs to settle
function Invoke-Split([string]$name, [int]$count) {
  [IO.File]::WriteAllText($ask, $name)
  if (-not (Wait-Until { @(Get-Panes -Server).Count -ge $count } -TimeoutSec 10 -IntervalMs 250)) {
    throw "$name`: the server has $(@(Get-Panes -Server).Count) panes, not $count"
  }
  # The GUI's tab with them all, laid out and still (was polled from its Lua's report.json)
  try { Wait-Change { param($x) @($x.panes).Count -ge $count } -Animating -What "$count panes in the GUI" -TimeoutSec 10 | Out-Null } catch { }
  # A resync handing the keyboard on would come after: the time the bug took to show
  Start-Sleep 2
  $panes = Get-Panes -Server
  [pscustomobject]@{ server = @($panes | Where-Object is_active)[0].pane_id; gui = Get-GuiActive; panes = $panes }
}

Invoke-Test {
  Open-TestWindow -ConfigFile $cfg -Domain perfsplit -Socket $sock -TimeoutSec 15 | Out-Null
  Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250 | Out-Null
  $first = (Get-Panes -Server)[0].pane_id
  # The GUI showing it (was 1 s)
  Wait-Change { param($x) $x.frame -gt 0 -and @($x.panes).Count -eq 1 } -Animating -What 'the first pane in the GUI' | Out-Null

  # 1. Split and focus straight back, three times over
  for ($i = 1; $i -le 3; $i++) {
    $r = Invoke-Split 'split_back' ($i + 1)
    "split_back $i`: server's active pane $($r.server), the GUI's $($r.gui) (the first is $first)"
    if ($r.server -ne $first) { Add-Failure "split_back $i`: the server's active pane is $($r.server), not the first ($first)" }
    if ($r.gui -ne $first) { Add-Failure "split_back $i`: the GUI's active pane is $($r.gui), not the first ($first)" }
  }

  # 2. A split with nothing after it leaves the new pane active
  $before = @($r.panes.pane_id)
  $r = Invoke-Split 'split_only' 5
  $new = @($r.panes.pane_id | Where-Object { $_ -notin $before })[0]
  "split_only: server's active pane $($r.server), the GUI's $($r.gui) (the new one is $new)"
  if ($r.server -ne $new) { Add-Failure "split_only: the server's active pane is $($r.server), not the new one ($new)" }
  if ($r.gui -ne $new) { Add-Failure "split_only: the GUI's active pane is $($r.gui), not the new one ($new)" }
} -Passed 'split-focus: a Lua split keeps the keyboard where Lua puts it, on the server and in the GUI'
