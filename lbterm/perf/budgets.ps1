#Requires -Version 7
# Helper: one budget scenario a run, for `lbterm.ps1 budgets` (lbterm/PERF.md, "The budget runner"); not an e2e test, since it fails until what's over its budget is fixed
# Covers: lbterm/perf/budgets.json, lbterm/budgets.psm1, lbterm/harness.psm1
<#
One scenario of lbterm\perf\budgets.json, set up in a test window and measured: at rest, what
LBTerm costs while nothing happens to it, or an action, how long it takes to show and what it costs
(lbterm/PERF.md, "The budget runner"). `lbterm.ps1 budgets` runs every scenario this way, one
process each, and judges and reports them all; run by hand it judges the one and prints its row.

An action (budgets.json's kind "action") is done -Runs times after its warm-ups, each from a
window at rest: what it does posted (a key, a click, a move, a resize, a cli call, a process
started), timed from just before that to the time_ms of the first frame the state file says shows
it (Start-FrameRecorder keeps every frame, so none is missed between reads), then to the first with
nothing animating, then the window left until it's quiet (no frame for 300 ms), what it cost read
from the GUI thread's, the process's and the mux server's cycles, the frames painted and metrics
(reset before each run: the GUI's own time for it, its messages and tasks). Then it's undone (the
tab closed, the menu shut, the pointer gone) and the window waits at rest for the next. Its window
times the GUI's own action (`gui`) against the scenario's own gui_ms budget, so a run over it has
the GUI say in its log what the time went on, kept in measure.json.

Every window looks like Lachlan's (New-TestConfig: his lbterm.lua, WebGpu with the compositor,
max_fps 120, the rail, the strip, the objectives panel and the scroll strip), on a mux domain of the
test's own as his tabs are (budgets.json's domain says when it isn't), with USERPROFILE a scratch
folder, so the summaries folder, the prompt history and anything his handlers write are the
test's, and no projects card of his. The scenario's setup does what it needs (tabs, a pointer
resting somewhere, a menu, a played hub or seat), gives the window the keyboard with a posted
WM_SETFOCUS unless it's about not having it, and waits for the state file (lbterm/STATE.md) to say
it's there. Then it settles (the state file settled, within 8 s, and settle_ms more, so the
GUI's own budget, whose quiet_ms is 1 s, takes the window as at rest) and is measured -Runs times
for -Seconds each, nothing sent to the window meanwhile:

- frames: the state file's frame count (every window of the process), so a build without metrics
  is measured the same way;
- the GUI process's CPU and its GUI thread's by their cycles (Get-CpuSnapshot: Windows' times come in
  15.625 ms ticks), the mux server's and the conpty hosts' too, and the GUI's memory and handles;
- from metrics (lbterm/CONTROL.md), reset before each span: repaints asked for, timers fired, task
  runs and their time, messages, wakes, who ran, and what the GUI's own budget counted at rest.

Around every span and every run, what the rest of the PC did (Start-PcWatch, Stop-PcWatch,
ConvertTo-PcSpan): what others took of the CPUs the test runs on, how busy the whole PC was, and
those CPUs' clock, laid on its numbers (Add-PcSample), which scales what's measured by cycles to the
nominal clock. One measured on a PC busier than budgets.json's pc rules is taken again, up to
pc.retakes more a scenario, while fewer than -Runs were calm (lbterm/PERF.md, "The PC while it
measured").

-Log gives the window's log a WEZTERM_LOG of its own (module=level, comma-separated), for a
trace: `-Log 'info,lbterm::present=debug,lbterm::present::walk=trace'` has each episode of motion's
line and every compositor frame its walk read (wezterm-gui/src/present.rs), in the same log as
-SlowEvery's. Like -SlowEvery, a run with it isn't a measurement.

-SlowEvery traces instead of measuring: the window logs every paint part, task and message over
1 ms to its log (LBTERM_SLOW_EVERY with lbterm_slow_ms = 1, lbterm-extras/src/slow.rs), each paint's
pass, draw and present in turn, so an action's paints can be told apart one by one (which costs
what), as the per-run metrics, a sum over the run, can't. Its numbers aren't a measurement: the
logging runs on the GUI thread. The log is ~/.local/share/lbterm/lbterm-gui.exe-log-<pid>.txt, the
pid in the run's pids.txt (`lbterm.ps1 logs -Pid <pid> -Slow`).

After each span the scenario must still be what it set up (a menu still open, the pointer still
there), or the run fails as not measuring it. measure.json in the output folder has every span,
the lbterm::budget lines the window logged, and notes; result.json fails on what's over budget
("over budget: ...") and on a setup that threw.

  pwsh -File lbterm\perf\budgets.ps1 -List
  pwsh -File lbterm\perf\budgets.ps1 -Scenario one-tab [-Runs 3] [-Seconds 5] [-Bin <build>]
  pwsh -File lbterm\perf\budgets.ps1 -Scenario key-echo [-Runs 15] [-Bin <build>] [-GuiMs 1]

-GuiMs gives an action's window that time for the GUI's own action in place of the scenario's
gui_ms budget, so every run over it has the GUI say in its log what its time went on (-GuiMs 1:
every run), kept in measure.json's action_lines: a way to see where a slow action's time goes.
Each run's sample also says which frame after it showed it (shown_frame, 1 the first) and when
the first frame after it ended (first_frame_ms).
#>
param([string]$Bin, [string]$Scenario, [int]$Runs = 0, [double]$Seconds = 0, [double]$GuiMs = 0, [switch]$List, [switch]$SlowEvery, [string]$Log)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
Import-Module (Join-Path $PSScriptRoot '..\budgets.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')

$Budgets = Read-Budgets (Join-Path $PSScriptRoot 'budgets.json')
if ($List) {
  foreach ($s in $Budgets.Scenarios) { '{0,-24} {1,-6} {2,-5}  {3}' -f $s.Name, $s.Kind, $s.Domain, $s.What }
  exit 0
}
if (-not $Scenario) { throw 'budgets.ps1 runs one scenario: -Scenario <name> (-List lists them); lbterm.ps1 budgets runs them all' }
$Sc = @($Budgets.Scenarios | Where-Object Name -eq $Scenario)[0]
if (-not $Sc) { throw "no scenario $Scenario in budgets.json: $(($Budgets.Scenarios | ForEach-Object Name) -join ', ')" }
$IsAction = $Sc.Kind -eq 'action'
# Spans at rest; an action's runs, its own unless -Runs says, which the judging then holds it to
if (-not $Runs) { $Runs = $Sc.Runs }
$Sc.Runs = $Runs
if (-not $Seconds) { $Seconds = $Sc.Seconds }
# Spans or runs that may be taken again in place of ones measured on a busy PC
$Retakes = $Budgets.Pc.Retakes

$T = Start-Test "budgets-$Scenario" -Bin $Bin
$Mux = $Sc.Domain -eq 'mux'
$HomeDir = Join-Path $T.Out 'home'
# Lachlan's lbterm.lua puts the summaries and the prompt history under USERPROFILE, a scratch
# folder here
$Sums = Join-Path $HomeDir '.claude\summaries'
$Transcripts = Join-Path $T.Out 'transcripts'
$Projects = Join-Path $T.Out 'projects'
$Cfg = Join-Path $T.Out 'config\budgets.lua'
$DemoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
New-Item -ItemType Directory -Force $Sums, $Transcripts, $Projects, (Split-Path $Cfg) | Out-Null
$Notes = [Collections.Generic.List[string]]::new()
$script:FirstPane = $null

# Where test windows open (the harness's $TestWindowPosition, a variable of its module)
$x0, $y0 = (Get-OffscreenPosition -Screens @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object Bounds) -Desktop ([System.Windows.Forms.SystemInformation]::VirtualScreen)) -split ','

# An action's window: a key for each action a key does (Lachlan's config binds none of F2 to F8),
# and the GUI's own time for the scenario's action set to the scenario's gui_ms budget, so a run
# over it has the GUI say what the time went on (lbterm/PERF.md, "Expected times"; its name in
# config's lbterm_budget.actions has _ for -)
$ActionLua = if ($IsAction) {
  $guiMs = if (-not $Sc.Gui) { $null } elseif ($GuiMs -gt 0) { $GuiMs } elseif ($Sc.Budgets.Contains('gui_ms') -and $null -ne $Sc.Budgets.gui_ms.max) { $Sc.Budgets.gui_ms.max }
  # Only to a build that has the budget: one without it (the installed build, measured as the
  # before) opens its configuration error window for a field it doesn't know, a second window of
  # the test's beside the one measured. config_builder's strict mode asks without saying anything
  $guiLua = if ($guiMs) {
    "if pcall(function() local probe = wezterm.config_builder(); probe:set_strict_mode(true); probe.lbterm_budget = { enabled = true } end) then rawset(config, 'lbterm_budget', { actions = { $($Sc.Gui -replace '-', '_') = $([string]::Format([cultureinfo]::InvariantCulture, '{0}', $guiMs)) } }) end"
  }
  @"
config.keys = config.keys or {}
for _, k in ipairs({
  { key = 'F2', mods = 'NONE', action = act.TogglePaneZoomState },
  { key = 'F3', mods = 'NONE', action = act.CloseCurrentPane { confirm = false } },
  { key = 'F4', mods = 'NONE', action = act.ActivateTabRelative(1) },
  { key = 'F5', mods = 'NONE', action = act.ReloadConfiguration },
  { key = 'F6', mods = 'NONE', action = act.CloseCurrentTab { confirm = false } },
  { key = 'F7', mods = 'NONE', action = act.SplitHorizontal { args = pwsh } },
  { key = 'F8', mods = 'NONE', action = act.SpawnCommandInNewTab { args = pwsh } },
  { key = 'F9', mods = 'NONE', action = act.IncreaseFontSize },
  { key = 'F10', mods = 'NONE', action = act.DecreaseFontSize },
  -- A new window, on the screen test windows open on, a little down and across
  { key = 'F11', mods = 'NONE', action = wezterm.action_callback(function(window, pane)
      wezterm.mux.spawn_window {
        args = pwsh,
        position = { x = $x0 + 60, y = $y0 + 40, origin = 'ScreenCoordinateSystem' },
        $(if ($Mux) { "domain = { DomainName = 'budgets' }," })
      }
    end) },
}) do table.insert(config.keys, k) end
$guiLua
"@
} else { '' }

# What every scenario's config adds to Lachlan's: its size, a shell of no profile in the test's
# folder, none of his projects, the scroll strip's transcripts the test's, and the Lua events the
# scenarios run through the control pipe's emit
$BaseLua = @"
config.initial_cols = 110
config.initial_rows = 30
config.window_close_confirmation = 'NeverPrompt'
config.default_cwd = [[$($T.Out)]]
local pwsh = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
config.default_prog = pwsh
rawset(config, 'lbterm_projects', nil)
rawset(config, 'lbterm_scroll_strip', { transcripts = [[$Transcripts]] })
local act = wezterm.action
wezterm.on('budgets-search', function(window, pane) window:perform_action(act.Search 'CurrentSelectionOrEmptyString', pane) end)
wezterm.on('budgets-copy', function(window, pane) window:perform_action(act.ActivateCopyMode, pane) end)
wezterm.on('budgets-palette', function(window, pane) window:perform_action(act.ActivateCommandPalette, pane) end)
wezterm.on('budgets-split', function(window, pane) window:perform_action(act.SplitHorizontal { args = pwsh }, pane) end)
wezterm.on('budgets-zoom', function(window, pane) window:perform_action(act.TogglePaneZoomState, pane) end)
-- Three more windows, on the screen test windows open on, a little down and across each
wezterm.on('budgets-windows', function(window, pane)
  for i = 1, 3 do
    wezterm.mux.spawn_window {
      args = pwsh,
      position = { x = $x0 + 70 * i, y = $y0 + 50 * i, origin = 'ScreenCoordinateSystem' },
      $(if ($Mux) { "domain = { DomainName = 'budgets' }," })
    }
  end
end)
$ActionLua
"@

# The scenario's test window, its config Lachlan's with the base and -Lua added, on the test's mux
# domain when budgets.json says so, its one tab running -Program (a shell without), up and its pane
# in the state file. A mux server started for a connecting GUI opens a tab of its own with the
# default program and never runs the one `connect <domain> -- <program>` names, so there the
# program gets a tab of its own and the server's first tab is closed.
function Open-BudgetWindow {
  param([string]$Lua = '', [object[]]$Program, [switch]$PostedPointer)
  $domain = if ($Mux) { @"
config.unix_domains = { {
  name = 'budgets',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $Cfg)', '--daemonize' },
} }
"@ }
  $slow = if ($SlowEvery) { "`nconfig.lbterm_slow_ms = 1" } else { '' }
  New-TestConfig -Path $Cfg -Lua ($BaseLua + "`n" + $Lua + "`n" + $domain + $slow)
  $open = @{ ConfigFile = $Cfg; UserProfile = $HomeDir; PostedPointer = $PostedPointer; TimeoutSec = 30 }
  # An action is timed to the screen too (lbterm_extras::present, off unless asked): at rest it
  # stays off, so what's measured there is what Lachlan's window spends
  $open.Environment = @{}
  if ($IsAction) { $open.Environment.LBTERM_PRESENT_TIMING = '1' }
  # -SlowEvery: every paint part, task and message over 1 ms in the window's log, to trace
  if ($SlowEvery) { $open.Environment.LBTERM_SLOW_EVERY = '1' }
  # -Log: the window's WEZTERM_LOG (module=level), for a trace such as the walk's frames
  # (`lbterm::present=debug,lbterm::present::walk=trace`, PITFALLS 166)
  if ($Log) { $open.Log = $Log }
  if (-not $Mux) { $open.Program = if ($Program) { $Program } else { New-PwshProgram } }
  if ($Mux) { $open.Domain = 'budgets'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  # What it draws reaches the screen only where it can be seen (lbterm/PERF.md, "What's hard to
  # time"): moved on its screen to where nothing over it overlaps it, if there's such a place,
  # never raised or activated; what's over it said once
  if (-not $script:CoverSaid) {
    $before = (Get-WindowCover $w).Said
    $clear = Move-TestWindowClear $w
    $after = Get-WindowCover $w
    $script:CoverSaid = $true
    $script:Hidden = $after.Covered -ge 0.95
    if ($after.Covered -gt 0.02) { $Notes.Add("over the window: $($after.Said)$(if (-not $clear) { '; nowhere clear on its screen' }), so what reached the screen is measured only where the GUI says it could be seen") }
    elseif ($before -ne 'nothing') { $Notes.Add("moved clear of $before") }
  }
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 20)) { throw "the test's mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) @($s.panes).Count -ge 1 -and $s.rail } -What 'the first pane and the rail' -TimeoutSec 30 | Out-Null
  $script:FirstPane = (Get-Panes -Server:$Mux)[0].pane_id
  if ($Mux -and $Program) {
    $own = New-Tab -From $script:FirstPane -Program $Program -Server
    # Both in the window before the first goes: a window left without a tab closes
    Wait-State { param($s) @($s.tabs).Count -eq 2 } -What "the program's tab beside the server's first" -TimeoutSec 15 | Out-Null
    Invoke-Cli activate-pane --pane-id $own -Server -Check | Out-Null
    Invoke-Cli kill-pane --pane-id $script:FirstPane -Server -Check | Out-Null
    $script:FirstPane = $own
    Wait-State { param($s) @($s.tabs).Count -eq 1 -and @($s.panes)[0].server_id -eq $own } -What "the program's own tab alone" -TimeoutSec 15 | Out-Null
  }
  $w
}

# A tab beside the first, not opened, running $Program: its pane id (the server's on a mux domain)
function Add-BudgetTab([object[]]$Program) { New-Tab -From $script:FirstPane -NoActivate -Program $Program -Server:$Mux }

# A stand-in Claude tab (motion/demo-tab.ps1): its title, state and how long ago that began, and
# its session when given
function New-DemoProgram([string]$Title, [string]$State = 'idle', [double]$AgeMinutes = 5, [string]$Session) {
  $a = @('-Title', $Title, '-State', $State, '-AgeMinutes', $AgeMinutes)
  if ($Session) { $a += @('-Session', $Session) }
  New-PwshProgram -File $DemoTab -ArgumentList $a
}

# The window as the state file has it
function Get-Win { Get-StateWindow (Read-State) -Hwnd ([long](Get-Test).Current.Handle) }

# Waits for the window to show what the scenario set up: -Until (param($s), the window's state entry)
function Wait-Shown([scriptblock]$Until, [string]$What, [double]$TimeoutSec = 20) {
  Wait-State $Until -What $What -TimeoutSec $TimeoutSec | Out-Null
}

# The pointer posted to (x, y) once and left there (LBTERM_POSTED_POINTER keeps it hovering): no
# hold, which would post a move every 16 ms and keep the window busy
function Set-RestingPointer($w, [int]$X, [int]$Y) { Send-Mouse $w Move $X $Y }

# A session id made from a number, as Claude Code's look
function Get-Sid([int]$N) { 'b0d6e7{0:x2}-0000-4000-8000-{0:x12}' -f $N }

# One tab with the keyboard, then an overlay a Lua event opens (-Emit), shown when -Until says so
function Open-Overlay([string]$Emit, [scriptblock]$Until, [string]$What) {
  $r = & $Setups['one-tab']
  Send-Focus $r.Window
  Start-Sleep -Milliseconds 500
  Send-Emit $Emit
  Wait-Shown $Until $What
  $r.Holds = $Until
  $r
}

#region The scenarios: each opens its window, sets it up and returns what to hold it to

# Each returns @{ Window; Holds = param($s, $w): of the window's state entry, whether the scenario is still what it set up;
# NoSettle = it never settles on purpose; Focus = $false to leave the keyboard alone }
$Setups = @{}

# One tab at a pwsh prompt
function Open-OneTab([switch]$PostedPointer) {
  $w = Open-BudgetWindow -PostedPointer:$PostedPointer
  Invoke-Cli send-text --pane-id $script:FirstPane --no-paste "'shell up'`r" -Server:$Mux -Check | Out-Null
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}
$Setups['one-tab'] = { Open-OneTab }
$Setups['one-tab-local'] = $Setups['one-tab']
$Setups['unfocused'] = {
  $r = & $Setups['one-tab']
  Send-Focus $r.Window
  Wait-Shown { param($s) $s.keyboard } 'the keyboard given'
  Send-Focus $r.Window -Off
  Wait-Shown { param($s) -not $s.keyboard } 'the keyboard gone'
  $r.Focus = $false
  $r.Holds = { param($s, $w) -not $s.keyboard }
  $r
}
$Setups['minimized'] = {
  $r = & $Setups['one-tab']
  Send-Focus $r.Window
  Wait-Settled -TimeoutSec 10 | Out-Null
  if (-not (Set-WindowMinimized $r.Window)) { throw 'the window didn''t minimize' }
  $r.Focus = $false
  # Minimized, it paints nothing, so the state file says what it said before
  $r.Holds = { param($s, $w) [LbtermE2E.Show]::IsIconic((Resolve-Handle $w)) }
  $r
}

# Twelve tabs, eleven Claude stand-ins with labels and a shell last, the first open: -PostedPointer
# for a scenario whose pointer rests on the rail (tab-menu-opening). -Hidden (the numbers of
# Get-Sid, none for an empty list) gives the window the Conductor's pad, from a stand-in
# button.json as the Conductor writes it (`@Conductor:button`) with those sessions on its `hidden`
# list, and no hub or seat: a build with Hide from Conductor (lbterm/NEW-TAB.md, section 8) offers
# the tab menu's Hide row on every Claude tab and marks the hidden ones' pads, one without reads
# the same file and draws neither, so the two can be held against each other. -Mode is the pad's
# mode in that file (most, as the Conductor's default; all for Accept all). -Parked parks the tab
# of that number once the tabs are up (Lachlan's 8a; lbterm/NEW-TAB.md, "Landing 4"): F7 bound to
# open it, park it and open the first again, on a build with LbtermPark, whose state file says
# `parked` on each row; one without has neither, so its F7 is never sent and nothing is parked
function Open-TwelveTabs([switch]$PostedPointer, [int[]]$Hidden, [string]$Mode = 'most', [int]$Parked) {
  $states = 'working', 'waiting', 'idle'
  $lua = ''
  if ($PSBoundParameters.ContainsKey('Hidden')) {
    $state = Join-Path $T.Out 'button.json'
    $file = [ordered]@{ state = 'on'; mode = $Mode; listening = $false; busy = $false; tooltip = "Conductor is on (accept $Mode)"; hidden = @($Hidden | ForEach-Object { Get-Sid $_ }); updatedAt = '2026-11-09T10:00:00.000Z' }
    [IO.File]::WriteAllText($state, ($file | ConvertTo-Json -Compress))
    $lua = "rawset(config, 'lbterm_conductor', { state_file = [[$state]], menu = {} })"
  }
  if ($Parked) {
    $act = 'wezterm.action'
    $lua += @"

if wezterm.has_action and wezterm.has_action('LbtermPark') then
  config.keys = config.keys or {}
  table.insert(config.keys, { key = 'F7', action = $act.Multiple { $act.ActivateTab($($Parked - 1)), $act.LbtermPark 'Park', $act.ActivateTab(0) } })
end
"@
  }
  Write-SessionFiles -Summaries $Sums -Sid (Get-Sid 1) -Label 'Tab 1'
  $w = Open-BudgetWindow -Lua $lua -PostedPointer:$PostedPointer -Program (New-DemoProgram 'tab 1' 'idle' 5 (Get-Sid 1))
  for ($i = 2; $i -le 12; $i++) {
    $sid = Get-Sid $i
    if ($i -lt 12) { Write-SessionFiles -Summaries $Sums -Sid $sid -Label "Tab $i" }
    $program = if ($i -lt 12) { New-DemoProgram "tab $i" $states[$i % 3] 5 $sid } else { New-PwshProgram }
    Add-BudgetTab $program | Out-Null
  }
  Wait-Shown { param($s) @($s.tabs).Count -eq 12 -and @($s.rail.rows | Where-Object label).Count -ge 11 } 'twelve tabs, eleven with labels'
  $script:ParkedRow = if ($Parked) { $Parked - 1 } else { -1 }
  if ($Parked -and @((Get-Win).rail.rows)[0].PSObject.Properties['parked']) {
    Send-Focus $w
    # Sent again while nothing came of it: in one round of five (2026-11-09 18:13, Lachlan at the
    # PC) the GUI took the posted F7 (its keyboard layout probed, as at a window's first key) and
    # ran no binding, its log with no "parked:" line. Each press asks for 'Park' and opens the
    # first tab, so a second one changes nothing a first did
    for ($try = 1; ; $try++) {
      Send-Key $w F7
      try {
        Wait-Shown { param($s) (Test-ParkedRow $s) -and $s.active_tab -eq 0 -and $s.settled } "tab $Parked parked and the first open again" -TimeoutSec 5
        break
      } catch { if ($try -ge 3) { throw } }
    }
    if ($try -gt 1) { $Notes.Add("F7 sent $try times before tab $Parked was parked") }
  }
  if ($PSBoundParameters.ContainsKey('Hidden')) {
    $script:HiddenRows = @($Hidden | ForEach-Object { $_ - 1 })
    Wait-Shown { param($s) $s.rail.conductor -and (Test-HiddenRows $s) } "the Conductor's pad$(if ($Hidden) { ", rows $($script:HiddenRows -join ' and ') hidden where the build reads hidden" })"
    return @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 12 -and $s.rail.conductor -and (Test-HiddenRows $s) -and (Test-ParkedRow $s) } }
  }
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 12 -and (Test-ParkedRow $s) } }
}
# Whether the one row Open-TwelveTabs -Parked parked is the only one parked, on a build whose state
# file says `parked` on a row (8a's); true on one without, and with nothing asked parked
function Test-ParkedRow($s) {
  $rows = @($s.rail.rows)
  if (-not $rows.Count -or -not $rows[0].PSObject.Properties['parked']) { return $true }
  $parked = @(for ($i = 0; $i -lt $rows.Count; $i++) { if ($rows[$i].parked) { $i } })
  ($parked -join ',') -eq $(if ($script:ParkedRow -ge 0) { "$($script:ParkedRow)" } else { '' })
}
# Whether the rail's rows hidden from the Conductor are the ones Open-TwelveTabs -Hidden asked for,
# on a build whose state file says `hidden` on a row (Hide from Conductor's); true on one without
function Test-HiddenRows($s) {
  $rows = @($s.rail.rows)
  if (-not $rows.Count -or -not $rows[0].PSObject.Properties['hidden']) { return $true }
  $hidden = @(for ($i = 0; $i -lt $rows.Count; $i++) { if ($rows[$i].hidden) { $i } })
  ($hidden -join ',') -eq ($script:HiddenRows -join ',') -and -not @($rows | Where-Object hidden_pending).Count
}
$Setups['twelve-tabs'] = { Open-TwelveTabs }
# The same twelve tabs with the Conductor's pad, the first two hidden from it (lbterm/NEW-TAB.md,
# section 8): the closed rail with their eye-off marks on a build that draws them, the open tab's
# among them
$Setups['rail-hidden'] = { Open-TwelveTabs -Hidden 1, 2 }
# The same, the pointer resting on the first tab's pad: the panel open, that row lit, the two
# hidden rows' "Hidden" chips and marks
$Setups['rail-hidden-open'] = {
  $r = Open-TwelveTabs -PostedPointer -Hidden 1, 2
  $x, $y = Get-RectCenter (@((Get-Win).rail.rows)[0].pad)
  Set-RestingPointer $r.Window $x $y
  Wait-Shown { param($s) $s.rail.open -and $s.rail.hovered -eq 0 } 'the panel open, the first row lit'
  $r.Holds = { param($s, $w) $s.rail.open -and $s.rail.hovered -eq 0 -and @($s.tabs).Count -eq 12 -and $s.rail.conductor -and (Test-HiddenRows $s) }
  $r
}
# Landing 4's rail at rest (lbterm/NEW-TAB.md, "Landing 4"): the twelve tabs with the Conductor's
# pad accepting all from a stand-in button.json, no hub acting (Lachlan's 7a: the pad a still red
# hairline, where a build before it turned the spectrum's hues, a loop of DWM's), the second tab
# parked (8a: its row dimmed) and only the idle timer's lit bars over the pads (12a)
$Setups['rail-parked'] = { Open-TwelveTabs -Hidden @() -Mode all -Parked 2 }
# The same, the pointer resting on the first tab's pad: the panel open, that row lit, the parked
# row's chip, and the labels under the pads covered by the open words (12a)
$Setups['rail-parked-open'] = {
  $r = Open-TwelveTabs -PostedPointer -Hidden @() -Mode all -Parked 2
  $x, $y = Get-RectCenter (@((Get-Win).rail.rows)[0].pad)
  Set-RestingPointer $r.Window $x $y
  Wait-Shown { param($s) $s.rail.open -and $s.rail.hovered -eq 0 } 'the panel open, the first row lit'
  $r.Holds = { param($s, $w) $s.rail.open -and $s.rail.hovered -eq 0 -and @($s.tabs).Count -eq 12 -and $s.rail.conductor -and (Test-ParkedRow $s) }
  $r
}

$Setups['cursor-blink'] = {
  $w = Open-BudgetWindow -Lua "config.default_cursor_style = 'BlinkingBlock'`nconfig.cursor_blink_rate = 800"
  Invoke-Cli send-text --pane-id $script:FirstPane --no-paste "'shell up'`r" -Server:$Mux -Check | Out-Null
  @{ Window = $w; NoSettle = $true; Holds = { param($s, $w) $s.keyboard } }
}

$Setups['rail-open'] = {
  $w = Open-BudgetWindow -PostedPointer
  $s = Get-Win
  # Under the pads, on the rail: the panel opens, no row lit
  $last = @($s.rail.rows)[-1].pad
  Set-RestingPointer $w ([int]($last.x + $last.w / 2)) ([int]($last.y + $last.h + 80))
  Wait-Shown { param($s) $s.rail.open -and $s.pointer } 'the panel open'
  @{ Window = $w; Holds = { param($s, $w) $s.rail.open } }
}
$Setups['rail-pad'] = {
  $w = Open-BudgetWindow -PostedPointer
  $x, $y = Get-RectCenter (@((Get-Win).rail.rows)[0].pad)
  Set-RestingPointer $w $x $y
  Wait-Shown { param($s) $s.rail.open -and $s.rail.hovered -eq 0 } 'the panel open, the first row lit'
  @{ Window = $w; Holds = { param($s, $w) $s.rail.open -and $s.rail.hovered -eq 0 } }
}

$Setups['objectives'] = {
  $marks = @(
    @{ n = 21; label = 'Defender CPU'; objectives = @(New-Objective 1 first open 'Find why Defender uses 30% of the CPU') }
    @{ n = 22; label = 'Tab rail'; current = 2; objectives = @((New-Objective 1 first open 'Fix the tab rail'), (New-Objective 2 prompt done 'Rename the dashboard' 'Landed; check passed')) }
    @{ n = 23; label = 'Usage chart'; objectives = @(New-Objective 1 inmail done 'Build the usage chart' 'Archived 2026-10-24 09:20') }
  )
  foreach ($m in $marks) { Write-SessionFiles -Summaries $Sums -Sid (Get-Sid $m.n) -Label $m.label -Objectives $m.objectives -Current $(if ($m.current) { $m.current } else { 1 }) }
  $w = Open-BudgetWindow -Program (New-DemoProgram one idle 5 (Get-Sid 21))
  Add-BudgetTab (New-DemoProgram two idle 5 (Get-Sid 22)) | Out-Null
  Add-BudgetTab (New-DemoProgram three idle 5 (Get-Sid 23)) | Out-Null
  Add-BudgetTab (New-DemoProgram plain) | Out-Null
  Wait-Shown { param($s) @($s.objectives.marks).Count -eq 4 -and @($s.objectives.marks | Where-Object mark).Count -eq 3 } 'four rows, three marked'
  @{ Window = $w; Holds = { param($s, $w) @($s.objectives.marks | Where-Object mark).Count -eq 3 } }
}

$Setups['scroll-strip'] = {
  $sid = Get-Sid 31
  $folder = Join-Path $Transcripts 'C--work-demo'
  New-Item -ItemType Directory -Force $folder | Out-Null
  $start = [DateTime]::new(2026, 10, 28, 9, 0, 0, [DateTimeKind]::Utc)
  $records = for ($m = 1; $m -le 12; $m++) {
    $at = { param($k) $start.AddSeconds($m * 30 + $k).ToString('yyyy-MM-ddTHH:mm:ss.fffZ') }
    @{ type = 'user'; origin = @{ kind = 'human' }; timestamp = (& $at 0); isSidechain = $false; message = @{ role = 'user'; content = "message number $m, about part $m of the work" } } | ConvertTo-Json -Compress -Depth 8
    @{ type = 'assistant'; timestamp = (& $at 5); isSidechain = $false; message = @{ role = 'assistant'; content = @(@{ type = 'text'; text = ((1..14 | ForEach-Object { "answer $m, line $_" }) -join "`n") }) } } | ConvertTo-Json -Compress -Depth 8
    @{ type = 'system'; subtype = 'turn_duration'; durationMs = 12000; timestamp = (& $at 10); isSidechain = $false } | ConvertTo-Json -Compress -Depth 8
  }
  [IO.File]::WriteAllText((Join-Path $folder "$sid.jsonl"), ($records -join "`n") + "`n")
  $w = Open-BudgetWindow -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-claude.ps1') -ArgumentList '-Sid', $sid, '-Messages', 12)
  Wait-Shown { param($s) $s.scroll_strip.mode -eq 'claude' -and $s.scroll_strip.thumb -and @($s.scroll_strip.marks | Where-Object kind -eq 'prompt').Count -eq 12 } 'the conversation on the scroll strip' 30
  @{ Window = $w; Holds = { param($s, $w) $s.scroll_strip.mode -eq 'claude' } }
}

$Setups['conductor-pad'] = {
  $state = Join-Path $T.Out 'button.json'
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "most", "tooltip": "Conductor is on (accept most)" }')
  $hub = Start-PlayedPipe -Name "lbterm-perf-budgets-hub-$PID" -Subscribe '"lbterm.subscribe"'
  $sids = (Get-Sid 41), (Get-Sid 42)
  $w = Open-BudgetWindow -Lua "rawset(config, 'lbterm_conductor', { state_file = [[$state]], hub_pipe = [[\\.\pipe\$($hub.Name)]], menu = {} })"
  foreach ($sid in $sids) { Add-BudgetTab (New-ClaudeSessionProgram $sid) | Out-Null }
  if (-not (Wait-Until { $hub.Subscribers -ge 1 -and @(Get-PlayedLines $hub 'lbterm.panes' | Select-Object -Last 1 | ForEach-Object { @($_.panes).Count }) -ge 2 } -TimeoutSec 20)) {
    throw "LBTerm never subscribed to the played hub with both sessions' panes: $($hub.Heard() -join ' | ')"
  }
  Send-PlayedLine $hub ('{"type":"conductor.focus","sessions":[{"sessionId":"' + $sids[0] + '","doing":"thinking"},{"sessionId":"' + $sids[1] + '","doing":"acting"}]}') | Out-Null
  Wait-Shown { param($s) $s.rail.conductor.level -eq 'most' -and @($s.rail.rows | Where-Object conductor -eq 'thinking').Count -eq 1 -and @($s.rail.rows | Where-Object conductor -eq 'acting').Count -eq 1 } 'the pad accepting most, a row thought about and a row acted on'
  $script:Hub = $hub
  @{ Window = $w; Holds = { param($s, $w) @($s.rail.rows | Where-Object conductor).Count -eq 2 } }
}

# The same, its first tab's program waiting on a task of its own (its pad's arc turning too), and
# the window then minimized: its loops hold still, as they do while it can't be seen (lbterm/COMPOSITOR.md,
# "Loops"), where each kept DWM composing every refresh
$Setups['conductor-pad-minimized'] = {
  $r = & $Setups['conductor-pad']
  Invoke-Cli send-text --pane-id $script:FirstPane --no-paste "[Console]::Write(`"`$([char]27)]7501;state=done:app=claude`$([char]27)\`$([char]27)]7501;state=working:id=bg/b1`$([char]27)\`")`r" -Server:$Mux -Check | Out-Null
  Wait-Shown { param($s) @($s.rail.rows)[0].waiting -ge 1 -and @($s.loops) -contains 'rail.spinner' } 'the first pad waiting on its task, its arc turning'
  Send-Focus $r.Window
  Wait-Settled -TimeoutSec 10 | Out-Null
  if (-not (Set-WindowMinimized $r.Window)) { throw 'the window didn''t minimize' }
  $r.Focus = $false
  $r.Holds = { param($s, $w) [LbtermE2E.Show]::IsIconic((Resolve-Handle $w)) -and @($s.rail.rows | Where-Object conductor).Count -eq 2 }
  $r
}

# The view open over the tab against a seat played here. Its view is what the real seat's would be
# with two sessions: an ask, a picker and a question in text waiting, an event, a suggestion, a
# note. With -Live the seat sends it again every 250 ms, as the real one does whenever any of it
# changes (Conductor's SEAT.md: at most every 250 ms) while the hub's guesses move: the first
# session's time left counting down a second a push, which changes nothing the view shows ("~4
# min" for a minute at a time)
function Open-ConductorView([switch]$Live, [switch]$NoOpen) {
  $state = Join-Path $T.Out 'button.json'
  [IO.File]::WriteAllText($state, '{ "state": "on", "mode": "read", "listening": true, "busy": false, "asks": 1 }')
  $seat = Start-PlayedPipe -Name "lbterm-perf-budgets-seat-$PID" -Subscribe '"seat.subscribe"' -Reply
  $sidA, $sidB = (Get-Sid 51), (Get-Sid 52)
  $w = Open-BudgetWindow -PostedPointer -Lua "rawset(config, 'lbterm_conductor', { state_file = [[$state]], seat_pipe = [[\\.\pipe\$($seat.Name)]], menu = {} })"
  Add-BudgetTab (New-ClaudeSessionProgram $sidA) | Out-Null
  if (-not (Wait-Until { $seat.Subscribers -ge 1 } -TimeoutSec 20)) { throw "LBTerm never subscribed to the played seat: $($seat.Heard() -join ' | ')" }
  $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  # What conductor-view.ps1's seat says, an ask and two questions waiting
  $view = @"
{"type":"seat.view","seq":@n@,"at":$now,
 "conductor":{"state":"on","mode":"read","listening":true,"pausedAt":null,"catchUp":false,"busy":false,"queued":0,"sessionId":"c0","startedAt":$($now - 600000),"model":"claude-opus-5-5","error":null},
 "asks":[{"id":"ask-3","at":$($now - 90000),"tool":"SendMessage","title":"Message Publishing the page in Website","detail":"Push the site now.","session":"website-a6","label":"Publishing the page","project":"Website"}],
 "sessions":[{"sessionId":"$sidA","name":"website-a6","project":"Website","status":"waiting","label":"Publishing the page","summary":"","question":"Which layout?","pane":null,"percent":40,"left":@left@,"agents":[],"conductor":"thinking"},
             {"sessionId":"$sidB","name":"terminal-b1","project":"Terminal","status":"working","label":"Pad colours","summary":"","question":null,"pane":null,"agents":[],"conductor":null}],
 "waiting":[{"id":"$($sidA):toolu_1","sessionId":"$sidA","session":"website-a6","label":"Publishing the page","project":"Website","kind":"picker","extras":false,"gist":"Which layout?","at":$($now - 60000),
   "questions":[{"question":"Which layout for the page?","header":"Layout","multiSelect":false,"options":[{"label":"Two columns","description":"Side by side"},{"label":"One column","description":"Easier on a phone"}]}],"text":null,"answerable":true,"why":null},
  {"id":"$($sidB):$now","sessionId":"$sidB","session":"terminal-b1","label":"Pad colours","project":"Terminal","kind":"text","extras":false,"gist":"Q1","at":$($now - 30000),"questions":[],"text":"Q1. Which colour for the pad?\n  1a. Orange\n  1b. Blue","answerable":true,"why":null}],
 "events":[{"seq":811,"at":$($now - 120000),"kind":"finished","session":"terminal-b1","label":"Pad colours","project":"Terminal","text":"Built the pad","wake":false}],
 "suggestions":[{"id":"g1","day":"2026-10-23","project":"Terminal","kind":"tooling","title":"Keep the worktree's target warm","change":"Build each new worktree once.","evidence":"Six builds waited.","saves":"Ten minutes a branch","status":"open","rating":null}],
 "notes":[{"id":"note-4","at":$($now - 45000),"text":"Publishing the page needs your OK to push"}]}
"@
  Send-PlayedLine $seat (($view -replace '@n@', '1') -replace '@left@', '240') | Out-Null
  Send-PlayedLine $seat @"
{"type":"seat.items","reset":true,"items":[
 {"id":"u1","at":$($now - 50000),"who":"you","from":"lbterm","text":"what's waiting on me?","queued":false},
 {"id":"t1","at":$($now - 49000),"who":"tool","tool":{"name":"fleet","text":"fleet: 2 sessions, 1 waiting","status":"done"}},
 {"id":"r1","at":$($now - 48000),"who":"conductor","text":"Two things:\n- **website-a6** asks which layout the page gets.\n- terminal-b1 asks which colour the pad takes."},
 {"id":"n1","at":$($now - 10000),"who":"notice","text":"Read only from now on"}]}
"@ | Out-Null
  $s = Wait-State { param($s) $s.rail.conductor.asks -eq 1 } -What 'the pad orange with an ask'
  $script:Seat = $seat
  # The action opens it itself (conductor-view-opening)
  if ($NoOpen) { return @{ Window = $w; Holds = { param($s, $w) $true } } }
  $pad = Get-RectCenter $s.rail.conductor.pad
  Send-Click $w $pad[0] $pad[1] -Move
  Send-Leave $w
  Wait-Shown { param($s) $s.conductor_view -and $s.conductor_view.connected -and @($s.conductor_view.items).Count -eq 4 } 'the view open'
  # The seat's guess of the first session's time left counts down from 240 s, a second a push
  if ($Live) { Start-PlayedRepeat $seat ($view -replace '@left@', '@down240@') -EveryMs 250 }
  $script:Seat = $seat
  @{ Window = $w; Holds = { param($s, $w) [bool]$s.conductor_view } }
}
$Setups['conductor-view'] = { Open-ConductorView -Live }
$Setups['conductor-view-quiet'] = { Open-ConductorView }

# The InMail button with one message waiting in a folder of the test's own, and with -Open its
# list open from a click on it
function Open-InMail([switch]$Open) {
  $folder = Join-Path $Projects 'One\InMail'
  New-InMailMessage -Folder $folder -To One -From Two -Subject 'A message' -At (Get-Date).AddHours(-3) | Out-Null
  $w = Open-BudgetWindow -PostedPointer -Lua "rawset(config, 'lbterm_inbox', { folders = { [[$folder]] } })"
  $s = Wait-State { param($s) $s.strip.inbox -eq 1 -and @($s.strip.items | Where-Object name -eq 'inbox') } -What 'the InMail button saying 1'
  if (-not $Open) { return @{ Window = $w; Holds = { param($s, $w) $true } } }
  $c = Get-RectCenter (@($s.strip.items | Where-Object name -eq 'inbox')[0].rect)
  Send-Click $w $c[0] $c[1] -Move
  Wait-Shown { param($s) $s.menu.kind -eq 'inbox' } 'the InMail list'
  @{ Window = $w; Holds = { param($s, $w) $s.menu.kind -eq 'inbox' } }
}
$Setups['inbox-list'] = { Open-InMail -Open }

# Two projects of the test's own, one with mail, a stand-in session in each, and with -Open the
# card open, the pointer resting on its pad
function Open-ProjectsCard([switch]$Open) {
  $alpha = Join-Path $Projects 'Alpha'
  $beta = Join-Path $Projects 'Beta'
  New-InMailMessage -Folder (Join-Path $alpha 'InMail') -To Alpha -From Beta -Subject 'A message' -At (Get-Date).AddHours(-2) | Out-Null
  New-Item -ItemType Directory -Force (Join-Path $beta 'InMail') | Out-Null
  Write-SessionFiles -Summaries $Sums -Sid (Get-Sid 61) -Label 'In Alpha' -Cwd $alpha
  Write-SessionFiles -Summaries $Sums -Sid (Get-Sid 62) -Label 'In Beta' -Cwd $beta
  $w = Open-BudgetWindow -PostedPointer -Program (New-DemoProgram one idle 5 (Get-Sid 61)) -Lua @"
rawset(config, 'lbterm_projects', { projects = { { name = 'Alpha', dir = [[$alpha]] }, { name = 'Beta', dir = [[$beta]] } } })
rawset(config, 'lbterm_inbox', { folders = { [[$alpha\InMail]], [[$beta\InMail]] } })
"@
  Add-BudgetTab (New-DemoProgram two idle 5 (Get-Sid 62)) | Out-Null
  $s = Wait-State { param($s) @($s.tabs).Count -eq 2 -and $s.objectives.projects.pad } -What 'two tabs and the projects pad'
  if (-not $Open) { return @{ Window = $w; Holds = { param($s, $w) $true } } }
  $x, $y = Get-RectCenter $s.objectives.projects.pad
  Set-RestingPointer $w $x $y
  Wait-Shown { param($s) $s.objectives.projects.open -and @($s.objectives.projects.rows).Count -eq 2 } 'the projects card open with two projects'
  @{ Window = $w; Holds = { param($s, $w) $s.objectives.projects.open } }
}
$Setups['projects-card'] = { Open-ProjectsCard -Open }

$Setups['tasks-card'] = {
  $sid = Get-Sid 71
  $subagents = Join-Path $Transcripts "$sid\subagents"
  New-Item -ItemType Directory -Force $subagents | Out-Null
  $transcript = Join-Path $subagents 'agent-a1b2c3d4e5f6a7b8c.jsonl'
  $call = { param($name, $in, $read) @{ type = 'assistant'; isSidechain = $true; message = @{ role = 'assistant'; content = @(@{ type = 'tool_use'; id = 'toolu_x'; name = $name; input = $in }); usage = @{ input_tokens = 2; cache_creation_input_tokens = 400; cache_read_input_tokens = $read; output_tokens = 180 } } } | ConvertTo-Json -Compress -Depth 8 }
  [IO.File]::WriteAllText($transcript, ((@(
          (@{ type = 'user'; message = @{ role = 'user'; content = 'Review the card' } } | ConvertTo-Json -Compress -Depth 5)
          (& $call 'Read' @{ file_path = 'C:\src\lbterm\TAB-PANEL.md' } 30000)
          (& $call 'Edit' @{ file_path = 'C:\src\render\tasks_card.rs' } 52000)
        )) -join "`n") + "`n")
  $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
  $tasks = @(
    @{ id = 'a1b2c3d4e5f6a7b8c'; kind = 'agent'; description = 'Review the tasks card'; agentType = 'Explore'; startedAt = $now - 12 * 60000; transcript = $transcript }
    @{ id = 'b9x8y7z6w'; kind = 'command'; description = 'Run the LBTerm check'; startedAt = $now - 3 * 60000 }
    @{ id = 'm5n4b3v2c'; kind = 'monitor'; description = 'Wait for the build queue'; startedAt = $now - 5 * 60000 }
  )
  [IO.File]::WriteAllText((Join-Path $Sums "$sid.tasks.json"), (@{ sessionId = $sid; tasks = $tasks } | ConvertTo-Json -Compress -Depth 5))
  $records = @('state=done:app=claude') + @($tasks | ForEach-Object { "state=working:id=bg/$($_.id)" })
  $seq = (Get-UserVarSequence claude_session $sid) + (($records | ForEach-Object { "$([char]27)]7501;$_$([char]27)\" }) -join '')
  $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($seq))
  $w = Open-BudgetWindow -PostedPointer -Program (New-PwshProgram "[Console]::Write([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$b64'))); Write-Host 'claude'; Start-Sleep 600")
  $s = Wait-State { param($s) @($s.rail.rows)[0].waiting -eq 3 } -What 'the first row waiting on three tasks'
  $x, $y = Get-RectCenter (@($s.rail.rows)[0].pad)
  Set-RestingPointer $w $x $y
  # The card follows moves over the open panel: a move made before it opened starts no rest
  Wait-Shown { param($s) $s.rail.open -and $s.settled } 'the panel open'
  Set-RestingPointer $w $x $y
  Wait-Shown { param($s) $s.rail.open -and $s.tasks -and $s.tasks.row -eq 0 } 'the card beside the first row'
  @{ Window = $w; Holds = { param($s, $w) $s.tasks -and $s.tasks.row -eq 0 } }
}

$Setups['thumbnail'] = {
  Add-Type -AssemblyName System.Drawing
  $pic = Join-Path $T.Out 'pics\magenta.png'
  New-Item -ItemType Directory -Force (Split-Path $pic) | Out-Null
  $bmp = [System.Drawing.Bitmap]::new(400, 300)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::FromArgb(255, 0, 255)); $g.Dispose()
  $bmp.Save($pic, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  $uri = 'lbterm://image/' + ($pic -replace '\\', '/')
  $w = Open-BudgetWindow -Program (New-PwshProgram @"
`$e = [char]27; `$b = [char]0x2800
[Console]::OutputEncoding = [Text.UTF8Encoding]::new(`$false)
function Link([string]`$uri, [string]`$words) { "`$e]8;;`$uri`$e\`$words`$e]8;;`$e\" }
[Console]::Write("`$e[?1000h`$e[?1006h")
[Console]::WriteLine('  ' + (Link '$uri' 'magenta.png'))
for (`$r = 0; `$r -lt 5; `$r++) { [Console]::WriteLine('  ' + (Link "$($uri)?row=`$r&of=5" ([string]::new(`$b, 17)))) }
Start-Sleep 600
"@)
  Wait-Shown { param($s) @($s.thumbnails | Where-Object state -eq 'shown').Count -ge 1 } 'the thumbnail shown'
  @{ Window = $w; Holds = { param($s, $w) @($s.thumbnails | Where-Object state -eq 'shown').Count -ge 1 } }
}

$Setups['strip-card'] = {
  $sid = Get-Sid 81
  Write-SessionFiles -Summaries $Sums -Sid $sid -Label 'Strip card' -Summary 'Budgets: the strip card at rest' -Detail "Goal: measure what the strip's card costs at rest.`nState: pinned open.`n"
  [IO.File]::WriteAllText((Join-Path $Sums 'expanded.flag'), '')
  $w = Open-BudgetWindow -Program (New-ClaudeSessionProgram $sid)
  Wait-Shown { param($s) $s.strip.summary -match 'the strip card at rest' -and $s.strip.card -eq 'summary' } 'the summary and its card'
  @{ Window = $w; Holds = { param($s, $w) $s.strip.card -eq 'summary' } }
}

$Setups['link-hover'] = {
  $w = Open-BudgetWindow -PostedPointer -Program (New-PwshProgram @"
`$e = [char]27
[Console]::Write("`$e[2J`$e[3;5H")
for (`$i = 0; `$i -lt 3; `$i++) { [Console]::WriteLine("    `$e]8;;https://example.com/budgets`$e\a link a pane printed, under the pointer`$e]8;;`$e\") }
Start-Sleep 600
"@)
  $s = Wait-State { param($s) @($s.panes).Count -eq 1 -and @($s.panes)[0].cols -gt 0 } -What 'the pane'
  Start-Sleep -Milliseconds 800
  $p = @((Get-Win).panes)[0]
  $cw = $p.rect.w / $p.cols; $ch = $p.rect.h / $p.rows
  # The middle of the three links' rows, half way along
  Set-RestingPointer $w ([int]($p.rect.x + 24 * $cw)) ([int]($p.rect.y + 3.5 * $ch))
  Wait-Shown { param($s) $s.pointer -and $null -ne $s.pointer.pane } 'the pointer on the pane'
  @{ Window = $w; Holds = { param($s, $w) $s.pointer -and $null -ne $s.pointer.pane } }
}

# Search is copy mode's overlay with its search line open; the state file's overlay stays null for
# it, but the pane under it is the overlay's, titled "Copy mode: <the pane's title>"
$Setups['search'] = { Open-Overlay 'budgets-search' { param($s) ($s.overlay -and $s.overlay -ne 'modal') -or @($s.panes | Where-Object title -like 'Copy mode: *').Count } 'search open' }
$Setups['copy-mode'] = { Open-Overlay 'budgets-copy' { param($s) ($s.overlay -and $s.overlay -ne 'modal') -or @($s.panes | Where-Object title -like 'Copy mode: *').Count } 'copy mode on' }
$Setups['palette'] = { Open-Overlay 'budgets-palette' { param($s) $s.overlay -eq 'modal' } 'the command palette open' }

$Setups['zoomed'] = {
  $r = & $Setups['one-tab']
  Send-Emit 'budgets-split'
  Wait-Shown { param($s) @($s.panes).Count -eq 2 } 'the split'
  Send-Emit 'budgets-zoom'
  Wait-Shown { param($s) @($s.tabs | Where-Object zoomed).Count -eq 1 } 'a pane zoomed'
  $r.Holds = { param($s, $w) @($s.tabs | Where-Object zoomed).Count -eq 1 }
  $r
}

$Setups['four-windows'] = {
  $r = & $Setups['one-tab']
  Send-Emit 'budgets-windows'
  if (-not (Wait-Until { @((Read-State).windows).Count -eq 4 } -TimeoutSec 20)) { throw "four windows never showed: the state file has $(@((Read-State).windows).Count)" }
  # Where they opened: on the test windows' screen, never Lachlan's
  $screen = [System.Windows.Forms.Screen]::AllScreens | Sort-Object { $_.Bounds.Right } | Select-Object -Last 1
  foreach ($h in @((Read-State).windows | ForEach-Object { [IntPtr][long]$_.hwnd })) {
    $wr = Get-WindowRect $h
    if (-not $screen.Bounds.Contains($wr.Left + 10, $wr.Top + 10)) { $Notes.Add("a window opened at $($wr.Left),$($wr.Top), off the test windows' screen") }
  }
  $r.Holds = { param($s, $w) @((Read-State).windows).Count -eq 4 }
  $r
}

#endregion

#region Busy: what a frame costs with a card, a list or the rail open while the open tab paints

# The open tab's program for a busy scenario: a line rewritten in place every -EveryMs (about 16 a
# second at Windows' timer's resolution), as Claude Code's spinner and its count of seconds turn
# while it works, so the window paints a frame for each and what's open over the panes is drawn
# again with it. With -Sid it says first that it holds that Claude session (claude_session), so
# the strip has its summary. -Lines is pwsh run first, printing the lines it shows above the
# ticker's (a screen of links, of a trigger's matches)
function New-TickerProgram([int]$EveryMs = 50, [string]$Sid, [string]$Lines = '') {
  $var = if ($Sid) { "[Console]::Write([char]27 + ']1337;SetUserVar=claude_session=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Sid)))' + [char]7); " } else { '' }
  New-PwshProgram @"
$var$Lines
`$i = 0; `$sw = [Diagnostics.Stopwatch]::StartNew()
[Console]::WriteLine('a line rewritten every $EveryMs ms, as a working session''s spinner')
while (`$true) {
  `$i++
  [Console]::Write("``r  * Working (`$i)   ")
  `$wait = `$i * $EveryMs - `$sw.ElapsedMilliseconds
  if (`$wait -gt 0) { Start-Sleep -Milliseconds `$wait }
}
"@
}

# Projects the size of Lachlan's (47 of them, 70 messages waiting in the InMail folders of 38,
# three repos nested in projects), and the config that names them: lbterm_projects and the InMail
# button's folders
function New-ProjectsFixture([int]$Count = 47, [int]$Mail = 70, [int]$WithMail = 38) {
  $names = @(1..$Count | ForEach-Object { 'Project{0:d2}' -f $_ })
  $dirs = @($names | ForEach-Object { Join-Path $Projects $_ })
  # The last three are repos in the first three's folders, as Trainer is in Musica's
  for ($r = 0; $r -lt 3; $r++) { $dirs[$Count - 1 - $r] = Join-Path $dirs[$r] "code\$($names[$Count - 1 - $r])" }
  foreach ($d in $dirs) { New-Item -ItemType Directory -Force (Join-Path $d 'InMail') | Out-Null }
  $start = (Get-Date).AddDays(-9)
  for ($m = 0; $m -lt $Mail; $m++) {
    $p = $m % $WithMail
    New-InMailMessage -Folder (Join-Path $dirs[$p] 'InMail') -To $names[$p] -From 'Home' -Subject "Message $m about part $m of the work in $($names[$p])" -At $start.AddMinutes(173 * $m) | Out-Null
  }
  $listed = for ($i = 0; $i -lt $Count; $i++) {
    $parent = if ($i -ge $Count - 3) { " parent = '$($names[$Count - 1 - $i])'," } else { '' }
    "{ name = '$($names[$i])', dir = [[$($dirs[$i])]],$parent }"
  }
  $folders = @($dirs | ForEach-Object { "[[$(Join-Path $_ 'InMail')]]" }) -join ', '
  [pscustomobject]@{
    Dirs = $dirs
    Lua = "rawset(config, 'lbterm_projects', { projects = { $($listed -join ', ') } })`nrawset(config, 'lbterm_inbox', { folders = { $folders } })"
  }
}

# The ticker in the open tab beside 14 stand-in Claude tabs (working, waiting and idle, each in one
# of the projects), the projects and their mail: what the projects card, the InMail list and the
# rail have to show in a window like Lachlan's, nothing of it open yet
function Open-BusyTabs {
  $fix = New-ProjectsFixture
  $states = 'working', 'waiting', 'idle'
  for ($i = 1; $i -le 14; $i++) { Write-SessionFiles -Summaries $Sums -Sid (Get-Sid (100 + $i)) -Label "Tab $i" -Cwd $fix.Dirs[($i * 3) % $fix.Dirs.Count] }
  $w = Open-BudgetWindow -PostedPointer -Program (New-TickerProgram) -Lua $fix.Lua
  for ($i = 1; $i -le 14; $i++) { Add-BudgetTab (New-DemoProgram "tab $i" $states[$i % 3] 5 (Get-Sid (100 + $i))) | Out-Null }
  Wait-Shown { param($s) @($s.tabs).Count -eq 15 -and @($s.rail.rows | Where-Object label -like 'Tab *').Count -ge 10 -and $s.strip.inbox -eq 70 } 'fifteen tabs, the stand-ins labelled, 70 messages waiting' 60
  $w
}

$Setups['busy-one-tab'] = {
  $w = Open-BudgetWindow -Program (New-TickerProgram)
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}
$Setups['busy-tabs'] = {
  $w = Open-BusyTabs
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 15 -and -not $s.rail.open } }
}
$Setups['busy-projects-card'] = {
  $w = Open-BusyTabs
  $x, $y = Get-RectCenter (Get-Win).objectives.projects.pad
  Set-RestingPointer $w $x $y
  Wait-Shown { param($s) $s.objectives.projects.open -and @($s.objectives.projects.rows).Count -ge 10 } 'the projects card open'
  @{ Window = $w; Holds = { param($s, $w) $s.objectives.projects.open } }
}
$Setups['busy-inbox-list'] = {
  $w = Open-BusyTabs
  $c = Get-RectCenter (@((Get-Win).strip.items | Where-Object name -eq 'inbox')[0].rect)
  Send-Click $w $c[0] $c[1] -Move
  Wait-Shown { param($s) $s.menu.kind -eq 'inbox' -and @($s.menu.items).Count -eq 70 } 'the InMail list of 70'
  @{ Window = $w; Holds = { param($s, $w) $s.menu.kind -eq 'inbox' } }
}
$Setups['busy-rail-open'] = {
  $w = Open-BusyTabs
  $x, $y = Get-RectCenter (@((Get-Win).rail.rows)[1].pad)
  Set-RestingPointer $w $x $y
  Wait-Shown { param($s) $s.rail.open -and $s.rail.hovered -eq 1 } 'the panel open, the second row lit'
  @{ Window = $w; Holds = { param($s, $w) $s.rail.open } }
}
$Setups['busy-strip-card'] = {
  $sid = Get-Sid 181
  Write-SessionFiles -Summaries $Sums -Sid $sid -Label 'Strip card' -Summary 'Budgets: the strip card while the tab works' `
    -Detail "Goal: measure what the strip's card costs a frame while the tab under it paints.`nState: pinned open by expanded.flag, as Alt+S pins it.`nNext: hold it to what a frame costs with nothing open.`n"
  [IO.File]::WriteAllText((Join-Path $Sums 'expanded.flag'), '')
  $w = Open-BudgetWindow -Program (New-TickerProgram -Sid $sid)
  Wait-Shown { param($s) $s.strip.summary -match 'while the tab works' -and $s.strip.card -eq 'summary' } 'the summary and its card'
  @{ Window = $w; Holds = { param($s, $w) $s.strip.card -eq 'summary' } }
}
$Setups['busy-objectives'] = {
  $marks = @(
    @{ n = 121; label = 'Defender CPU'; objectives = @((New-Objective 1 first open 'Find why Defender uses 30% of the CPU'), (New-Objective 2 prompt open 'Then turn its scan of the build folders off', 'Asked at 09:12')) }
    @{ n = 122; label = 'Tab rail'; current = 2; objectives = @((New-Objective 1 first open 'Fix the tab rail'), (New-Objective 2 prompt done 'Rename the dashboard' 'Landed; check passed'), (New-Objective 3 prompt open 'Measure what a row costs')) }
    @{ n = 123; label = 'Usage chart'; objectives = @(New-Objective 1 inmail done 'Build the usage chart' 'Archived 2026-10-24 09:20') }
  )
  foreach ($m in $marks) { Write-SessionFiles -Summaries $Sums -Sid (Get-Sid $m.n) -Label $m.label -Objectives $m.objectives -Current $(if ($m.current) { $m.current } else { 1 }) }
  $w = Open-BudgetWindow -PostedPointer -Program (New-TickerProgram -Sid (Get-Sid 121))
  Add-BudgetTab (New-DemoProgram two idle 5 (Get-Sid 122)) | Out-Null
  Add-BudgetTab (New-DemoProgram three idle 5 (Get-Sid 123)) | Out-Null
  $s = Wait-State { param($s) @($s.objectives.marks | Where-Object mark).Count -eq 3 } -What 'three rows marked'
  # Resting on the strip level with the second row: the list opens, then that row's detail
  $x, $y = Get-RectCenter (@($s.objectives.marks)[1].rect)
  Set-RestingPointer $w $x $y
  Wait-Shown { param($s) $s.objectives.list -and $s.objectives.detail } 'the objectives list and its detail'
  @{ Window = $w; Holds = { param($s, $w) $s.objectives.list -and $s.objectives.detail } }
}

#endregion

#region Lines: what a frame costs with lines LBTerm looks at as they're drawn, while a tab paints

# 26 lines above the ticker, each holding a link printed as OSC 8 (as Claude Code links every path
# it names, FORCE_HYPERLINK=1 in Lachlan's lbterm.lua) and a URL hyperlink_rules make a link of
$LinkLines = @'
$e = [char]27
function Link([string]$uri, [string]$words) { "$e]8;;$uri$e\$words$e]8;;$e\" }
for ($n = 1; $n -le 26; $n++) {
  [Console]::WriteLine(('  {0,2}. ' -f $n) + (Link "file:///C:/work/project/src/module$n/file$n.rs" "src/module$n/file$n.rs:12$n") + " is where it's read, see https://example.com/docs/module$n/page$n")
}
'@
# 26 lines above the ticker, each holding four of the easter eggs' rainbow matches (\bLB), as a
# Claude session about LBTerm shows on most lines; no "love", whose hearts are their own scenario
$TriggerLines = @'
for ($n = 1; $n -le 26; $n++) {
  [Console]::WriteLine(('  {0,2}. LBTerm builds in C:\Code\LBTerm\target, LBTERM_TRY=1 in a try window, LBTerm''s slot {0}' -f $n))
}
'@
# 26 lines above the ticker as long as the others, with no link and nothing a trigger matches: what
# a screen of text costs a frame, for busy-links and busy-triggers to be held against
$PlainLines = @'
for ($n = 1; $n -le 26; $n++) {
  [Console]::WriteLine(('  {0,2}. Xbterm builds in C:\Code\Xbterm\target, XBTERM_TRY=1 in a try window, Xbterm''s slot {0}' -f $n))
}
'@
# Claude Code's fullscreen view scrolling: every row drawn again one line on every -EveryMs, from a
# conversation of 200 lines, each with -Word four times (the easter eggs' LB, or not), each line
# indented by a little more than the one before so a word's columns move as it scrolls
function New-ScrollProgram([int]$EveryMs = 50, [string]$Word = 'LBTerm') {
  New-PwshProgram @"
`$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new(`$false)
`$lines = @(1..200 | ForEach-Object { '  {0,3}. {1}$Word line {0}: cd C:\Code\$Word, then $($Word.ToUpper())_TRY=1 for a try window of $Word' -f `$_, ''.PadLeft(`$_ % 9) })
[Console]::Write("`$e[?1049h`$e[?25l")
`$i = 0; `$sw = [Diagnostics.Stopwatch]::StartNew()
while (`$true) {
  `$rows = [Console]::WindowHeight - 1
  `$sb = [Text.StringBuilder]::new()
  for (`$r = 0; `$r -lt `$rows; `$r++) { [void]`$sb.Append("`$e[`$(`$r + 1);1H`$e[2K").Append(`$lines[(`$i + `$r) % `$lines.Count]) }
  [Console]::Write(`$sb.ToString())
  `$i++
  `$wait = `$i * $EveryMs - `$sw.ElapsedMilliseconds
  if (`$wait -gt 0) { Start-Sleep -Milliseconds `$wait }
}
"@
}

$Setups['busy-lines'] = {
  $w = Open-BudgetWindow -Program (New-TickerProgram -Lines $PlainLines)
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}
$Setups['busy-links'] = {
  $w = Open-BudgetWindow -Program (New-TickerProgram -Lines $LinkLines)
  Wait-Shown { param($s) @($s.panes).Count -eq 1 } 'the links and the ticker'
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}
$Setups['busy-triggers'] = {
  $w = Open-BudgetWindow -Program (New-TickerProgram -Lines $TriggerLines)
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}
$Setups['busy-scroll'] = {
  $w = Open-BudgetWindow -Program (New-ScrollProgram -Word 'Xbterm')
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}
$Setups['busy-triggers-scroll'] = {
  $w = Open-BudgetWindow -Program (New-ScrollProgram -Word 'LBTerm')
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 } }
}

# A voice draft over a Claude stand-in (perf/fake-prompt.ps1: a reply asking Q1 and Q2 over its
# input box, the session's questions as Terminal's question worker writes them) answering Q1:
# its card and the question colours shown with no button held, Q1 ticked and Q2 ringed. -Own
# makes the stand-in's status line turn every 50 ms, as Claude Code's does while it works; without,
# the ticker turns in a pane split beside it, so what the draft's pane costs a frame it didn't
# change shows. -NoDraft leaves the draft out: the same panes, for the draft to be held against
function Open-Draft([switch]$Own, [switch]$NoDraft) {
  $sid = Get-Sid 191
  [IO.File]::WriteAllText((Join-Path $Sums "$sid.questions.json"), (@{
    sessionId = $sid
    messages = @(@{ id = 'reply'; at = 1; questions = @(
      @{ label = 'Q1'; number = 1; text = 'Q1. Should the rail show labels for try windows too?'; pos = 10; about = @() }
      @{ label = 'Q2'; number = 2; text = 'Q2. Remove the two old background agents with claude rm?'; pos = 30; about = @() }
    ) })
  } | ConvertTo-Json -Depth 8))
  $mode = Join-Path $T.Out 'mode.txt'
  $keys = Join-Path $T.Out 'keys.txt'
  [IO.File]::WriteAllText($mode, 'prompt')
  [IO.File]::WriteAllText($keys, '')
  $standIn = @('-Sid', $sid, '-Mode', $mode, '-Keys', $keys)
  if ($Own) { $standIn += @('-TickMs', 50) }
  $w = Open-BudgetWindow -Program (New-PwshProgram -File (Join-Path $PSScriptRoot 'fake-prompt.ps1') -ArgumentList $standIn)
  if (-not $Own) {
    $got = (Invoke-Cli split-pane --pane-id $script:FirstPane --right -Program (New-TickerProgram) -Server:$Mux -Merge) -join ' '
    if ($got.Trim() -notmatch '^\d+$') { throw "split-pane printed: $got" }
    Invoke-Cli activate-pane --pane-id $script:FirstPane -Server:$Mux -Check | Out-Null
    Wait-Shown { param($s) @($s.panes).Count -eq 2 } 'the ticker beside the stand-in'
  }
  if (-not (Wait-Until { (& $T.Cli control --class $T.Class focus 2>$null | Out-String | ConvertFrom-Json).session -eq $sid } -TimeoutSec 15)) { throw "the stand-in's pane never named session $sid" }
  if ($NoDraft) { return @{ Window = $w; Holds = { param($s, $w) @($s.panes).Count -eq 2 -and @($s.drafts).Count -eq 0 } } }
  $said = & $T.Cli control --class $T.Class draft add '1a' 2>&1 | Out-String
  if ($LASTEXITCODE -ne 0) { throw "draft add 1a: $said" }
  Wait-Shown { param($s) @($s.drafts).Count -eq 1 -and ($s.ticked -join ',') -eq '1' -and $s.next.label -eq 'Q2' } 'the draft, Q1 ticked and Q2 ringed'
  @{ Window = $w; Holds = { param($s, $w) @($s.drafts).Count -eq 1 -and ($s.ticked -join ',') -eq '1' } }
}
$Setups['busy-split'] = { Open-Draft -NoDraft }
$Setups['busy-draft'] = { Open-Draft }
$Setups['busy-draft-own'] = { Open-Draft -Own }

#endregion

#region A shell's scrollback: what a frame costs with the scroll strip's prompt marks over a long one

# A build's log at a shell: -Count lines with a shell's prompt (OSC 133) every -Every, file -Done
# written once they're out, then a line every -EveryMs with a prompt every 40th, the scrollback
# moving as a build's does (and once full, its first rows going)
function New-LogTickerProgram([int]$Count, [int]$Every = 500, [int]$EveryMs = 50, [Parameter(Mandatory)][string]$Done) {
  New-PwshProgram @"
`$e = [char]27
[Console]::OutputEncoding = [Text.UTF8Encoding]::new(`$false)
function Mark([long]`$n) { "`$e]133;A`$e\PS C:\work> `$e]133;B`$e\Invoke-Step `$n``r``n`$e]133;C`$e\" }
`$sb = [Text.StringBuilder]::new()
for (`$i = 1; `$i -le $Count; `$i++) {
  if (`$i % $Every -eq 0) { [void]`$sb.Append((Mark `$i)) } else { [void]`$sb.Append("  line `$i of the build's log, compiling one thing or another``r``n") }
  if (`$sb.Length -ge 262144) { [Console]::Write(`$sb.ToString()); [void]`$sb.Clear() }
}
[Console]::Write(`$sb.ToString())
[IO.File]::WriteAllText('$Done', 'done')
`$i = 0; `$sw = [Diagnostics.Stopwatch]::StartNew()
while (`$true) {
  `$i++
  if (`$i % 40 -eq 0) { [Console]::Write((Mark (1000000 + `$i))) } else { [Console]::WriteLine("tick `$i") }
  `$wait = `$i * $EveryMs - `$sw.ElapsedMilliseconds
  if (`$wait -gt 0) { Start-Sleep -Milliseconds `$wait }
}
"@
}

# The log's tab, its -Count lines printed (it's the window's tab from the start, so the strip has
# looked at them as they came) and the strip at its shell
function Open-LogTicker([int]$Count) {
  $done = Join-Path $T.Out 'history.done'
  $w = Open-BudgetWindow -Program (New-LogTickerProgram -Count $Count -Done $done)
  if (-not (Wait-Until { Test-Path $done } -TimeoutSec 120 -IntervalMs 250)) { throw "the tab's $Count lines weren't printed within 120 s" }
  Wait-Shown { param($s) $s.scroll_strip.mode -eq 'shell' } 'the scroll strip at the shell'
  @{ Window = $w; Holds = { param($s, $w) @($s.tabs).Count -eq 1 -and $s.scroll_strip.mode -eq 'shell' } }
}
$Setups['busy-shell'] = { Open-LogTicker 100 }
$Setups['busy-shell-local'] = $Setups['busy-shell']
$Setups['busy-scrollback'] = { Open-LogTicker 50000 }
$Setups['busy-scrollback-local'] = $Setups['busy-scrollback']

#endregion

#region The actions: each sets its window up once, then does what it times, run after run

# Each is @{ Setup = what opens the window (as a scenario at rest's: @{ Window; Focus }); Before =
# param($r, $i): what the run needs first (a tab to close), before the window is left to rest;
# Mark = param($r, $i): what the run starts from, read off the window at rest; Do = param($r, $i):
# what starts it, posted, timed from just before; Shown = param($w, $s, $r): of a frame's state,
# whether it shows it; After = param($r, $i): puts the window back as it was. Call = $true when Do
# is the whole of it (a cli call, timed to its return, no frame showing it). Extra = param($shown):
# more numbers from the frame that showed it (a writer's bytes). Run = param($r, $i, $warmup): a
# run measured its own way, returning its sample (a start).
$Actions = @{}

function Get-ActivePane($sw) { @($sw.panes | Where-Object active)[0] }
function Get-ActiveTab($sw) { @($sw.tabs | Where-Object active)[0] }

# A typed character's echo at pwsh's prompt: the cursor moved on. The prompt is made short (the
# test's folder, its default, runs to over 100 columns, so a few keys wrapped the line), and the
# line is cleared every ten
$Actions['key-echo'] = @{
  Setup = {
    $r = Open-OneTab
    Invoke-Cli send-text --pane-id $script:FirstPane --no-paste "function prompt { 'PS> ' }`r" -Server:$Mux -Check | Out-Null
    Wait-Shown { param($s) (Get-ActivePane $s).cursor.x -eq 4 } 'the short prompt'
    $r
  }
  Before = { param($r, $i) if ($i -and $i % 10 -eq 0) { Send-Key $r.Window Escape } }
  Mark = { param($r, $i) $c = Get-DrawnCursor (Get-ActivePane (Get-Win)); $r.X = $c.x; $r.Y = $c.y }
  Do = { param($r, $i) Send-Key $r.Window X }
  Shown = { param($w, $s, $r) $c = Get-DrawnCursor (Get-ActivePane $w); $c.x -ne $r.X -or $c.y -ne $r.Y }
}
# The cursor as the frame drew it (the state file's drawn_cursor), not as the pane had it when the
# snapshot was taken: an echo that came while a frame was drawn moves that one, though the frame
# didn't draw it (PERF.md, finding 34). A build without drawn_cursor has only the other
function Get-DrawnCursor($p) { if ($null -ne $p.drawn_cursor) { $p.drawn_cursor } else { $p.cursor } }
$Actions['key-echo-local'] = $Actions['key-echo']

# Latency under load: the same echo, with a writer at full speed in a pane beside it, its lines
# 104 bytes each, written 64 KB at a time without a pause, and its title saying every second how
# much it wrote, in MB a second (what the console host took, so how fast LBTerm read it)
$LoadScript = @'
$e = [char]27
$line = ' the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk'
$sb = [Text.StringBuilder]::new()
for ($i = 0; $i -lt 630; $i++) { [void]$sb.Append($i.ToString().PadLeft(6)).Append($line).Append("`r`n") }
$chunk = [Text.Encoding]::UTF8.GetBytes($sb.ToString())
$out = [Console]::OpenStandardOutput()
function Title([string]$t) { $b = [Text.Encoding]::UTF8.GetBytes("$e]0;$t$([char]7)"); $out.Write($b, 0, $b.Length); $out.Flush() }
Title 'starting'
$clock = [Diagnostics.Stopwatch]::StartNew()
$n = 0; $last = 0; $lastN = 0
while ($true) {
  $out.Write($chunk, 0, $chunk.Length)
  $n += $chunk.Length
  $ms = $clock.ElapsedMilliseconds
  if ($ms - $last -ge 1000) {
    Title ('load {0:F2}' -f (($n - $lastN) / 1MB / (($ms - $last) / 1000)))
    $last = $ms; $lastN = $n
  }
}
'@
$Actions['key-echo-load'] = @{
  Setup = {
    $r = & $Actions['key-echo'].Setup
    $got = (Invoke-Cli split-pane --pane-id $script:FirstPane --right -Program (New-PwshProgram $LoadScript) -Server:$Mux -Merge) -join ' '
    if ($got.Trim() -notmatch '^\d+$') { throw "split-pane printed: $got" }
    Invoke-Cli activate-pane --pane-id $script:FirstPane -Server:$Mux -Check | Out-Null
    Wait-Shown { param($s) @($s.panes).Count -eq 2 -and (Get-ActivePane $s).cursor.x -eq 4 -and @($s.panes | Where-Object { $_.title -match '^load ' }).Count -eq 1 } 'the writer writing beside the prompt' 30
    $r
  }
  Before = $Actions['key-echo'].Before
  Mark = $Actions['key-echo'].Mark
  Do = $Actions['key-echo'].Do
  Shown = $Actions['key-echo'].Shown
  # The writer's last second, from its title in the frame that showed the echo
  Extra = { param($shown)
    $t = @($shown.Window.panes | Where-Object { $_.title -match '^load ' })[0].title
    if ($t) { @{ writer_mb_per_s = [double]($t -split ' ')[1] } } else { @{} }
  }
  # The writer never lets the window go quiet: a run's cost is to its echo's frame and 300 ms on
  Busy = $true
}
$Actions['key-echo-load-local'] = $Actions['key-echo-load']

# Keys one after another, each timed from its own post to the first frame that drew its echo (the
# cursor on past it): a burst typed faster than a refresh of the test windows' screen, and a key
# held down at Lachlan's repeat rate. Do posts them at their times by the clock (a spin, since a
# sleep here ends on Windows' 15.6 ms tick), Shown notes every frame it's handed until the last
# echo is drawn, and Extra works out each key's echo from those (budgets.psm1's
# ConvertTo-KeyEchoes): the run's median (echo_ms) and slowest (echo_max_ms). A run's ms is its
# last key's, from its post to the frame that drew it; its settled_ms is from the first key's. The
# line is cleared before each run.
function Send-TimedKeys($r, [double[]]$AtMs, [switch]$Held) {
  $h = Resolve-Handle $r.Window
  $vk = [uint32](ConvertTo-Vk X)
  $scan = [LbtermE2E.Win]::MapVirtualKey($vk, 0)
  $down = [LbtermE2E.Win]::KeyLParam($scan, $false, $false)
  # A repeat's WM_KEYDOWN, as Windows posts it for a key held: its previous state down (bit 30)
  $repeat = [IntPtr]([long]$down -bor 0x40000000L)
  $up = [LbtermE2E.Win]::KeyLParam($scan, $false, $true)
  $r.Posts = [Collections.Generic.List[double]]::new()
  $clock = [Diagnostics.Stopwatch]::StartNew()
  for ($k = 0; $k -lt $AtMs.Count; $k++) {
    while ($clock.Elapsed.TotalMilliseconds -lt $AtMs[$k]) { [Threading.Thread]::SpinWait(64) }
    $r.Posts.Add((Get-UnixMs))
    if ($Held) { [LbtermE2E.Win]::Post($h, 0x0100, [IntPtr][long]$vk, $(if ($k) { $repeat } else { $down })) }
    else { [LbtermE2E.Win]::Key($h, $vk, $false) }
  }
  if ($Held) { [LbtermE2E.Win]::Post($h, 0x0101, [IntPtr][long]$vk, $up) }
  Watch-Foreground
}
$KeyRun = @{
  Setup = $Actions['key-echo'].Setup
  Before = { param($r, $i) Send-Key $r.Window Escape }
  Mark = { param($r, $i)
    $c = Get-DrawnCursor (Get-ActivePane (Get-Win))
    $r.X = $c.x; $r.Y = $c.y
    $r.Seen = [Collections.Generic.List[object]]::new()
  }
  Shown = { param($w, $s, $r)
    $c = Get-DrawnCursor (Get-ActivePane $w)
    $r.Seen.Add([pscustomobject]@{ T = [double]$s.time_ms; X = [int]$c.x; Y = [int]$c.y })
    $c.y -ne $r.Y -or $c.x -ge $r.X + $r.Posts.Count
  }
  Extra = { param($shown, $r) ConvertTo-KeyEchoes -Posts $r.Posts -Seen $r.Seen -X $r.X -Y $r.Y }
}
# Ten keys 15 ms apart: 67 a second, a run typed faster than the 60 Hz screen's refresh
$KeyBurstAt = @(0..9 | ForEach-Object { $_ * 15.0 })
$Actions['key-burst'] = $KeyRun.Clone()
$Actions['key-burst'].Do = { param($r, $i) Send-TimedKeys $r $KeyBurstAt }
$Actions['key-burst-local'] = $Actions['key-burst']
# A key held: its press, then after Lachlan's repeat delay (KeyboardDelay 1, 500 ms) fifteen
# repeats at his rate (KeyboardSpeed 31, about 30 a second, 33.3 ms apart), then let go
$KeyHeldAt = @(0.0) + @(0..14 | ForEach-Object { 500.0 + $_ * 33.3 })
$Actions['key-held'] = $KeyRun.Clone()
$Actions['key-held'].Do = { param($r, $i) Send-TimedKeys $r $KeyHeldAt -Held }
$Actions['key-held-local'] = $Actions['key-held']

# Two tabs, the second opened by the cli and not switched to
function Open-TwoTabs([switch]$PostedPointer) {
  $r = Open-OneTab -PostedPointer:$PostedPointer
  Add-BudgetTab (New-PwshProgram) | Out-Null
  Wait-Shown { param($s) @($s.tabs).Count -eq 2 -and $s.settled } 'two tabs'
  $r
}
# A tab switch by a key (ActivateTabRelative), to the other tab and back
$Actions['tab-by-key'] = @{
  Setup = { Open-TwoTabs }
  Mark = { param($r, $i) $r.Was = (Get-Win).active_tab }
  Do = { param($r, $i) Send-Key $r.Window F4 }
  Shown = { param($w, $s, $r) $w.active_tab -ne $r.Was }
}
# A tab switch by a click on the other tab's pad on the closed rail, the pointer gone straight
# after (a posted WM_MOUSELEAVE behind the click), so the panel doesn't open on its rest: what's
# timed and costed is the switch
$Actions['tab-by-click'] = @{
  Setup = { Open-TwoTabs -PostedPointer }
  Mark = { param($r, $i)
    $sw = Get-Win
    $r.Was = $sw.active_tab
    $r.X, $r.Y = Get-RectCenter (@($sw.rail.rows | Where-Object { -not $_.active })[0].pad)
  }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y; Send-Leave $r.Window }
  Shown = { param($w, $s, $r) $w.active_tab -ne $r.Was }
  After = { param($r, $i) Send-Leave $r.Window; Wait-Shown { param($s) $s.rail.phase -eq 'closed' -and $s.settled } 'the rail shut' }
}

# A new tab by a key (SpawnCommandInNewTab, a pwsh), closed again after
$Actions['new-tab'] = @{
  Setup = { Open-OneTab }
  Mark = { param($r, $i) $r.N = @((Get-Win).tabs).Count }
  Do = { param($r, $i) Send-Key $r.Window F8 }
  Shown = { param($w, $s, $r) @($w.tabs).Count -eq $r.N + 1 }
  After = { param($r, $i) Send-Key $r.Window F6; Wait-Shown { param($s) @($s.tabs).Count -eq $r.N -and $s.settled } 'the new tab closed' }
}
$Actions['new-tab-local'] = $Actions['new-tab']
# A tab closed by a key (CloseCurrentTab), a new one opened for it first
$Actions['close-tab'] = @{
  Setup = { Open-OneTab }
  Before = { param($r, $i)
    $n = @((Get-Win).tabs).Count
    Send-Key $r.Window F8
    Wait-Shown { param($s) @($s.tabs).Count -eq $n + 1 -and $s.settled } 'a tab to close'
  }
  Mark = { param($r, $i) $r.N = @((Get-Win).tabs).Count }
  Do = { param($r, $i) Send-Key $r.Window F6 }
  Shown = { param($w, $s, $r) @($w.tabs).Count -eq $r.N - 1 }
}
# A split by a key (SplitHorizontal, a pwsh), closed again after (CloseCurrentPane)
$Actions['split'] = @{
  Setup = { Open-OneTab }
  Mark = { param($r, $i) $r.N = @((Get-Win).panes).Count }
  Do = { param($r, $i) Send-Key $r.Window F7 }
  Shown = { param($w, $s, $r) @($w.panes).Count -eq $r.N + 1 }
  After = { param($r, $i) Send-Key $r.Window F3; Wait-Shown { param($s) @($s.panes).Count -eq $r.N -and $s.settled } 'the split closed' }
}
# A pane closed by a key (CloseCurrentPane), split off for it first: its motion is the pane going
# and the one left growing into its room (pane.exit, pane.layout)
$Actions['close-pane'] = @{
  Setup = { Open-OneTab }
  Before = { param($r, $i)
    $n = @((Get-Win).panes).Count
    Send-Key $r.Window F7
    Wait-Shown { param($s) @($s.panes).Count -eq $n + 1 -and $s.settled } 'a pane to close'
  }
  Mark = { param($r, $i) $r.N = @((Get-Win).panes).Count }
  Do = { param($r, $i) Send-Key $r.Window F3 }
  Shown = { param($w, $s, $r) @($w.panes).Count -eq $r.N - 1 }
}
# A pane zoomed and back by a key (TogglePaneZoomState), in a tab split in two
$Actions['zoom'] = @{
  Setup = {
    $r = Open-OneTab
    Send-Focus $r.Window
    Send-Key $r.Window F7
    Wait-Shown { param($s) @($s.panes).Count -eq 2 -and $s.settled } 'a tab split in two'
    $r
  }
  Mark = { param($r, $i) $r.Was = [bool](Get-ActiveTab (Get-Win)).zoomed }
  Do = { param($r, $i) Send-Key $r.Window F2 }
  Shown = { param($w, $s, $r) [bool](Get-ActiveTab $w).zoomed -ne $r.Was }
}

# The rail opening as the pointer rests on the first pad (open_delay_ms, 150, is in what's timed;
# the GUI's own time starts as it ends), the pointer gone after
$Actions['rail-opening'] = @{
  Setup = { Open-OneTab -PostedPointer }
  Mark = { param($r, $i) $r.X, $r.Y = Get-RectCenter (@((Get-Win).rail.rows)[0].pad) }
  Do = { param($r, $i) Send-Mouse $r.Window Move $r.X $r.Y }
  Shown = { param($w, $s, $r) $w.rail.open -and $w.rail.phase -eq 'open' }
  After = { param($r, $i) Send-Leave $r.Window; Wait-Shown { param($s) $s.rail.phase -eq 'closed' -and $s.settled } 'the rail shut' }
}
# The InMail list opening from a click on its button, shut by Esc after
$Actions['inbox-opening'] = @{
  Setup = { Open-InMail }
  Mark = { param($r, $i) $r.X, $r.Y = Get-RectCenter (@((Get-Win).strip.items | Where-Object name -eq 'inbox')[0].rect) }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y }
  Shown = { param($w, $s, $r) $w.menu.kind -eq 'inbox' }
  After = { param($r, $i)
    Send-Key $r.Window Escape
    Wait-Shown { param($s) -not $s.menu } 'the list shut'
    Send-Leave $r.Window
    Wait-Shown { param($s) $s.settled } 'the button at rest'
  }
}
# The list of a session's prompts dropping from a click on the arrow before the strip's last one,
# Claude Code's history.jsonl in the test's home as long as Lachlan's (2 MB, 6,600 lines of 1,200
# sessions on 2026-10-31), the session's 40 prompts among them; shown once they're all listed
# (until they're read the list says so in its one row)
$Actions['prompts-opening'] = @{
  Setup = {
    $sid = Get-Sid 201
    $sb = [Text.StringBuilder]::new()
    $words = 'about one part of the work or another, ' * 5
    for ($i = 0; $i -lt 6600; $i++) {
      $who = if ($i % 165 -eq 0) { $sid } else { Get-Sid (1000 + $i % 1200) }
      [void]$sb.Append('{"display":"prompt ').Append($i).Append(' ').Append($words).Append('","pastedContents":{},"timestamp":').Append(1790000000000 + $i * 1000).Append(',"project":"C:\\work","sessionId":"').Append($who).Append("`"}`n")
    }
    New-Item -ItemType Directory -Force (Join-Path $HomeDir '.claude') | Out-Null
    [IO.File]::WriteAllText((Join-Path $HomeDir '.claude\history.jsonl'), $sb.ToString())
    $vars = (Get-UserVarSequence claude_session $sid) + (Get-UserVarSequence claude_prompt "prompt 6435 $words")
    $w = Open-BudgetWindow -PostedPointer -Program (New-PwshProgram "[Console]::Write('$vars'); [Console]::WriteLine('a Claude stand-in, its last prompt on the strip'); while (`$true) { Start-Sleep -Seconds 60 }")
    Wait-Shown { param($s) $s.strip.prompt -and @($s.strip.items | Where-Object name -eq 'arrow') } 'the last prompt on the strip'
    @{ Window = $w }
  }
  Mark = { param($r, $i) $r.X, $r.Y = Get-RectCenter (@((Get-Win).strip.items | Where-Object name -eq 'arrow')[0].rect) }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y }
  Shown = { param($w, $s, $r) $w.menu.kind -eq 'prompts' -and @($w.menu.items).Count -eq 40 }
  After = { param($r, $i)
    Send-Key $r.Window Escape
    Wait-Shown { param($s) -not $s.menu } 'the list shut'
    Send-Leave $r.Window
    Wait-Shown { param($s) $s.settled } 'the strip at rest'
  }
}
# The projects card opening as the pointer rests on its pad, the pointer gone after
$Actions['projects-opening'] = @{
  Setup = { Open-ProjectsCard }
  Mark = { param($r, $i) $r.X, $r.Y = Get-RectCenter (Get-Win).objectives.projects.pad }
  Do = { param($r, $i) Send-Mouse $r.Window Move $r.X $r.Y }
  Shown = { param($w, $s, $r) [bool]$w.objectives.projects.open }
  After = { param($r, $i) Send-Leave $r.Window; Wait-Shown { param($s) -not $s.objectives.projects.open -and $s.settled } 'the card shut' }
}
# The tab menu opening from a right click on the first tab's pad (lbterm/TAB-MOUSE.md; NEW-TAB.md,
# section 7), to the first frame that draws it, Esc shutting it after. Twelve tabs, eleven of them
# Claude stand-ins with labels, so it gathers its facts as it does in a window like Lachlan's: the
# open tab's session, and every rail row's (render/tab_panel.rs, tab_panel_menu_rail), its words
# measured once. The pointer rests on the pad, the rail open and the row lit, before each run, as a
# hand's is before it clicks; the right click itself moves nothing
$Actions['tab-menu-opening'] = @{
  Setup = { Open-TwelveTabs -PostedPointer }
  Before = { param($r, $i)
    $s = Get-Win
    $r.X, $r.Y = Get-RectCenter (@($s.rail.rows)[0].pad)
    if (-not ($s.rail.open -and $s.rail.hovered -eq 0)) { Set-RestingPointer $r.Window $r.X $r.Y }
    Wait-Shown { param($s) $s.rail.open -and $s.rail.hovered -eq 0 -and -not $s.menu -and $s.settled } 'the pointer resting on the first pad, the rail open'
  }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y -Right }
  Shown = { param($w, $s, $r) $w.menu.kind -eq 'tab' }
  After = { param($r, $i)
    # A build that parks (Lachlan's 8a) offers the first tab's menu Park tab, whether it can found
    # as the menu opens (park.rs, can_park: the tab's domain asked)
    $s = Get-Win
    if (@($s.rail.rows)[0].PSObject.Properties['parked'] -and -not @($s.menu.items | Where-Object key -eq 'P').Count) {
      throw "the first tab's menu has no Park tab row: $(@($s.menu.items | ForEach-Object label) -join ', ')"
    }
    Send-Key $r.Window Escape
    Wait-Shown { param($s) -not $s.menu -and $s.settled } 'the tab menu shut'
  }
}
# The same with the Conductor's pad and its button.json's `hidden` list, empty (lbterm/NEW-TAB.md,
# section 8): a build with Hide from Conductor offers the first tab's menu its Hide row, the offer
# found as the menu opens (render/conductor_hide.rs, lbterm_hide_offer: the file's list and every
# pane's session in the tab), so this is what the row adds to the opening; one without opens the
# menu it always did. Each run's menu is checked for the row where the build reads `hidden`
$Actions['tab-menu-hide-opening'] = @{
  Setup = { Open-TwelveTabs -PostedPointer -Hidden @() }
  Before = $Actions['tab-menu-opening'].Before
  Do = $Actions['tab-menu-opening'].Do
  Shown = $Actions['tab-menu-opening'].Shown
  After = { param($r, $i)
    $s = Get-Win
    if (@($s.rail.rows)[0].PSObject.Properties['hidden'] -and -not @($s.menu.items | Where-Object key -eq 'H').Count) {
      throw "the first tab's menu has no Hide from Conductor row: $(@($s.menu.items | ForEach-Object label) -join ', ')"
    }
    & $Actions['tab-menu-opening'].After $r $i
  }
}
# The Conductor's view opening from a click on its pad, against a seat played here, left by Esc
$Actions['conductor-view-opening'] = @{
  Setup = { Open-ConductorView -NoOpen }
  Mark = { param($r, $i) $r.X, $r.Y = Get-RectCenter (Get-Win).rail.conductor.pad }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y }
  Shown = { param($w, $s, $r) [bool]$w.conductor_view }
  After = { param($r, $i)
    Send-Leave $r.Window
    Send-Key $r.Window Escape
    Wait-Shown { param($s) -not $s.conductor_view -and $s.settled } 'the view left'
  }
}
# The new-tab pad at the rail's foot (lbterm/NEW-TAB.md) with two projects of the test's own to
# pick from, in folders of its own: Lachlan's lbterm-project-action handler acts only on his own
# projects' folders, so a pick here opens LBTerm's default, a pwsh in the project's folder
# (lbterm/PITFALLS.md, 68). His recent picks' file isn't the test's: New-TestConfig clears it, and
# the projects set here name none, so the picks are kept in memory alone.
function Open-NewTabPad {
  $alpha = Join-Path $Projects 'Alpha'
  $beta = Join-Path $Projects 'Beta'
  New-Item -ItemType Directory -Force $alpha, $beta | Out-Null
  $w = Open-BudgetWindow -PostedPointer -Lua "rawset(config, 'lbterm_projects', { projects = { { name = 'Alpha', dir = [[$alpha]] }, { name = 'Beta', dir = [[$beta]] } } })"
  Wait-Shown { param($s) $s.new_tab -and $s.new_tab.rect } 'the + pad'
  @{ Window = $w }
}
# The pointer resting on the pad, lit and still, before a run's click on it
function Set-PointerOnPad($r) {
  $s = Get-Win
  $r.X, $r.Y = Get-RectCenter $s.new_tab.rect
  if (-not $s.new_tab.hover) { Set-RestingPointer $r.Window $r.X $r.Y }
  Wait-Shown { param($s) $s.new_tab.hover -and $s.settled } 'the pointer resting on the + pad, lit'
}
# The picker opening from a click on the pad, to the first frame that draws it; Esc shuts it after
$Actions['picker'] = @{
  Setup = { Open-NewTabPad }
  Before = { param($r, $i) Set-PointerOnPad $r }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y }
  Shown = { param($w, $s, $r) $w.new_tab.picker -and $null -ne $w.new_tab.picker.shown }
  After = { param($r, $i)
    Send-Key $r.Window Escape
    Wait-Shown { param($s) -not $s.new_tab.picker -and $s.settled } 'the picker shut'
  }
}
# The picker's first open in a GUI's life, cold: its words laid out, measured and drawn for the first
# time and its visual made (lbterm/NEW-TAB.md, "What the picker costs"). budgets.json gives it one run
# and no warm-up, so each round's window (budgets -Against -Rounds) is one cold open; `cold` says a
# run was the window's first open, 0 for a retake on a busy PC, which is warm and isn't this case
$Actions['picker-first'] = @{
  Setup = { Open-NewTabPad }
  Before = { param($r, $i) Set-PointerOnPad $r; $r.Opens = [int]$r.Opens + 1 }
  Do = { param($r, $i) Send-Click $r.Window $r.X $r.Y }
  Shown = { param($w, $s, $r) $w.new_tab.picker -and $null -ne $w.new_tab.picker.shown }
  Extra = { param($shown, $r) @{ cold = [int]($r.Opens -eq 1) } }
  After = { param($r, $i)
    Send-Key $r.Window Escape
    Wait-Shown { param($s) -not $s.new_tab.picker -and $s.settled } 'the picker shut'
  }
}
# A pick: Enter in the picker, opened before the run, to the first frame its start's record says drew
# its Starting row (new_tab.start's shown), or ended it, had the tab come first: the start the run
# asked for, by its id, not a row the rail happens to show (lbterm/PITFALLS.md, 52); the tab, a pwsh
# in Alpha's folder, is waited for and closed after (CloseCurrentTab: it opened in front)
$Actions['pick'] = @{
  Setup = { Open-NewTabPad }
  Before = { param($r, $i)
    Set-PointerOnPad $r
    $r.N = @((Get-Win).tabs).Count
    $last = (Get-Win).new_tab.start
    $r.Last = if ($last) { [long]$last.id } else { 0 }
    Send-Click $r.Window $r.X $r.Y
    Wait-Shown { param($s) $s.new_tab.picker -and $null -ne $s.new_tab.picker.shown -and $s.settled } 'the picker open'
  }
  Do = { param($r, $i) Send-Key $r.Window Enter }
  Shown = { param($w, $s, $r)
    $st = $w.new_tab.start
    $st -and [long]$st.id -gt $r.Last -and ($null -ne $st.shown -or $st.ended) -and [long]$w.frame -ge [long]$(if ($null -ne $st.shown) { $st.shown } else { $st.ended_frame })
  }
  After = { param($r, $i)
    Wait-Shown { param($s) @($s.tabs).Count -eq $r.N + 1 -and -not @($s.rail.rows | Where-Object pending) } 'the picked tab' 60
    Send-Key $r.Window F6
    Wait-Shown { param($s) @($s.tabs).Count -eq $r.N -and $s.settled } 'the picked tab closed' 30
  }
}
# Lines like a Claude Code session's, in the pane and its tab's title, with characters LBTerm's
# bundled fonts don't have, so they're drawn from fallbacks found among Windows' fonts: what a
# reload, a zoom or a new window in Lachlan's windows meets (lbterm/PERF.md, "What font setup
# costs"). Made of code points, so the file's encoding can't change them
$SymbolsLine = -join (@(0x23FA, 0x20, 0x52, 0x65, 0x61, 0x64, 0x20, 0x23BF, 0x20, 0x34, 0x32, 0x20, 0x273B, 0x20, 0x26A0, 0x20, 0x6F22, 0x5B57, 0x20, 0xD55C, 0xAD6D, 0x20, 0x2605) | ForEach-Object { [char]$_ })
function Open-SymbolsTab {
  $r = Open-OneTab
  # In UTF-8, or the console's code page makes them '?'; and a program's first title doesn't
  # reach the pane (the writer below says so), so a title of no use first
  $cmd = '[Console]::OutputEncoding = [Text.Encoding]::UTF8; 1..20 | % {{ ''{0} '' + $_ }}; [Console]::Write("`e]0;symbols`a"); Start-Sleep -Milliseconds 200; [Console]::Write("`e]0;{0}`a")' -f $SymbolsLine
  Invoke-Cli send-text --pane-id $script:FirstPane --no-paste "$cmd`r" -Server:$Mux -Check | Out-Null
  Wait-Shown { param($s) (Get-ActivePane $s).title -eq $SymbolsLine } 'the lines and the title with symbols' 20
  $r
}

# A burst of hearts (lbterm/TRIGGERS.md): a key to a program that prints "love" on a line of its
# own for each, the same line as the run before's still on the screen (each a new occurrence), to
# the first frame with hearts in the air; its cost is to the window quiet again, the burst's two
# seconds in it. Each run waits out the easter egg's cooldown of 4 s first
$Actions['hearts'] = @{
  Setup = {
    $w = Open-BudgetWindow -Program (New-PwshProgram @'
[Console]::WriteLine('each key prints the easter egg''s word on a line of its own')
while ($true) { [void][Console]::ReadKey($true); [Console]::WriteLine('love') }
'@)
    Wait-Shown { param($s) @($s.panes).Count -eq 1 } 'the program'
    @{ Window = $w; Since = [Diagnostics.Stopwatch]::StartNew() }
  }
  Before = { param($r, $i)
    $left = 4300 - $r.Since.ElapsedMilliseconds
    if ($left -gt 0) { Start-Sleep -Milliseconds $left }
  }
  Do = { param($r, $i) $r.Since = [Diagnostics.Stopwatch]::StartNew(); Send-Key $r.Window X }
  Shown = { param($w, $s, $r) [int]$w.hearts -gt 0 }
}
# A config reload by a key (ReloadConfiguration), Lachlan's whole lbterm.lua run again, shown by
# the strip's "Config reloaded"; the next waits for that to go (4 s). Over lines with symbols
$Actions['reload'] = @{
  Setup = { Open-SymbolsTab }
  Before = { param($r, $i) Wait-Shown { param($s) -not $s.strip.notice -and $s.settled } 'no notice on the strip' 10 }
  Do = { param($r, $i) Send-Key $r.Window F5 }
  Shown = { param($w, $s, $r) $w.strip.notice -match 'reload' }
}
# The font a size bigger and back, a run each (IncreaseFontSize, DecreaseFontSize), over lines with
# symbols: to the first frame with the window at its new size, which a font size changes
# (adjust_window_size_when_changing_font_size keeps the rows and columns). The size readout it
# shows fades in DWM, so each waits for that to be over first
$Actions['font-zoom'] = @{
  Setup = { Open-SymbolsTab }
  Before = { param($r, $i) Start-Sleep -Milliseconds 1600 }
  Mark = { param($r, $i) $r.W = (Get-Win).size.width }
  Do = { param($r, $i) Send-Key $r.Window $(if ($i % 2) { 'F10' } else { 'F9' }) }
  Shown = { param($w, $s, $r) $w.size.width -ne $r.W }
}

# A new window by a key (wezterm.mux.spawn_window, on a mux domain on the test's), over a window
# with lines with symbols: to its first frame, the state file's windows' own frame counts; closed
# again after (CloseCurrentTab, its last). Its start step by step from metrics' start
$Actions['new-window'] = @{
  Setup = { Open-SymbolsTab }
  Mark = { param($r, $i) $r.Hwnd = [long]$r.Window.Handle }
  Do = { param($r, $i) Send-Key $r.Window F11 }
  Shown = { param($w, $s, $r) @($s.windows | Where-Object { [long]$_.hwnd -ne $r.Hwnd -and [long]$_.frame -ge 1 }).Count -ge 1 }
  Extra = { param($shown)
    $m = try { Get-Metrics -Window (Get-Test).Current } catch { $null }
    $o = @{}
    if ($m -and $m.start) {
      foreach ($k in @('window', 'webgpu')) { if ($m.start.$k) { $o["${k}_start_ms"] = $m.start.$k.ms; $o["${k}_steps"] = $m.start.$k.steps } }
    }
    $o
  }
  After = { param($r, $i)
    $new = @((Read-State).windows | Where-Object { [long]$_.hwnd -ne $r.Hwnd })
    foreach ($nw in $new) { Send-Key ([IntPtr][long]$nw.hwnd) F6 }
    Wait-Until { @((Read-State).windows).Count -eq 1 } -TimeoutSec 15 | Out-Null
    Wait-Shown { param($s) $s.settled } 'the new window gone'
  }
}

# A line of characters in a script of their own, a new one each run, that none of the window's
# fonts has: in the pane and in its tab's title (the strip's words, in the window frame's font),
# so two fonts meet them. To the first frame showing the title, and costed to the window quiet
# again: the fallback found and drawn
$FallbackScript = @'
# The console's code page would make the UTF-8 bytes below other characters
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$e = [char]27
$pool = @(
  @(0x6F22, 0x5B57, 0x6587, 0x5316, 0x96FB, 0x8166), @(0xD55C, 0xAD6D, 0xC5B4, 0xD14C, 0xC2A4, 0xD2B8),
  @(0x0E20, 0x0E32, 0x0E29, 0x0E32, 0x0E44, 0x0E17), @(0x0939, 0x093F, 0x0928, 0x094D, 0x0926, 0x0940),
  @(0x0627, 0x0644, 0x0639, 0x0631, 0x0628, 0x064A), @(0x12A2, 0x1275, 0x12EE, 0x1335, 0x12EB, 0x1209),
  @(0x13E3, 0x13B3, 0x13A9, 0x13A0, 0x13A1, 0x13A2), @(0x182E, 0x1823, 0x1829, 0x182D, 0x1823, 0x182F),
  @(0x0F56, 0x0F7C, 0x0F51, 0x0F61, 0x0F72, 0x0F42), @(0xA188, 0xA320, 0xA000, 0xA001, 0xA002, 0xA003),
  @(0x1019, 0x103C, 0x1014, 0x103A, 0x1019, 0x102C), @(0x10E5, 0x10D0, 0x10E0, 0x10D7, 0x10E3, 0x10DA),
  @(0x2803, 0x2817, 0x2801, 0x280A, 0x2807, 0x2811), @(0x16A0, 0x16A2, 0x16A6, 0x16A8, 0x16B1, 0x16B2),
  @(0x2D5C, 0x2D49, 0x2D3C, 0x2D4F, 0x2D30, 0x2D59), @(0xA997, 0xA9AE, 0xA9A4, 0xA98F, 0xA9AD, 0xA9A0),
  @(0x1681, 0x1682, 0x1683, 0x1684, 0x1685, 0x1686)
)
$out = [Console]::OpenStandardOutput()
function Say([string]$t) { $b = [Text.Encoding]::UTF8.GetBytes($t); $out.Write($b, 0, $b.Length); $out.Flush() }
Say "ready`r`n"
Say "$e]0;starting$([char]7)"
Start-Sleep -Milliseconds 200
Say "$e]0;ready$([char]7)"
$n = 0
while ($true) {
  [void][Console]::ReadKey($true)
  $chars = -join ($pool[$n % $pool.Count] | ForEach-Object { [char]$_ })
  $n++
  Say "line $n $chars`r`n$e]0;fallback $n $chars$([char]7)"
}
'@
$Actions['fallback-line'] = @{
  Setup = {
    $w = Open-BudgetWindow -Program (New-PwshProgram $FallbackScript)
    Wait-Shown { param($s) (Get-ActivePane $s).title -eq 'ready' } 'the line writer ready' 30
    @{ Window = $w }
  }
  Mark = { param($r, $i) $r.N = $i + 1 }
  Do = { param($r, $i) Send-Key $r.Window G }
  Shown = { param($w, $s, $r) (Get-ActivePane $w).title -match "^fallback $($r.N) " }
}

# A screen of glyphs the atlas doesn't hold, a new one each run: what the glyph atlas costs
# filling again. Each glyph is rasterized and put in the atlas with a queue.write_texture of its
# own, which wgpu stages in an upload buffer of 64 KB at least, so the GPU allocator's upload
# blocks' size shows here and nowhere else (lbterm/PITFALLS.md, trap 77). A screen of CJK
# ideographs, the window made 55 rows tall for it: about 3,200 glyphs, more than half the 1024 px
# atlas and less than all of it on the test screen, so every run fills it, has it cleared and draws
# its screen again (atlas_rebuilds 1 a run). With the window's 32 rows, a screen of 1,800 cleared
# it every other run, and a round's median fell on either side. Six screens from U+4E00 in turn,
# none the one before, so none is in the atlas when it's drawn. To the first frame with the
# writer's title, which it sets once the screen is written; costed to the window quiet again
$GlyphsScript = @'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$e = [char]27
$out = [Console]::OpenStandardOutput()
function Say([string]$t) { $b = [Text.Encoding]::UTF8.GetBytes($t); $out.Write($b, 0, $b.Length); $out.Flush() }
Say "ready`r`n"
Say "$e]0;starting$([char]7)"
Start-Sleep -Milliseconds 200
Say "$e]0;ready$([char]7)"
$n = 0
while ($true) {
  [void][Console]::ReadKey($true)
  $n++
  $per = [int][Math]::Floor(([Console]::WindowWidth - 1) / 2)
  $rows = [Console]::WindowHeight - 1
  $c = 0x4E00 + (($n - 1) % 6) * $per * $rows
  $b = [Text.StringBuilder]::new()
  for ($row = 0; $row -lt $rows; $row++) {
    for ($k = 0; $k -lt $per; $k++) { [void]$b.Append([char]$c); $c++ }
    [void]$b.Append("`r`n")
  }
  Say ($b.ToString() + "$e]0;glyphs $n$([char]7)")
}
'@
$GlyphRows = 55
$Actions['atlas-refill'] = @{
  Setup = {
    $w = Open-BudgetWindow -Program (New-PwshProgram $GlyphsScript)
    Wait-Shown { param($s) (Get-ActivePane $s).title -eq 'ready' } 'the glyph writer ready' 30
    $p = Get-ActivePane (Get-Win)
    $outer = Get-WindowRect $w
    Set-WindowSize $w ($outer.Right - $outer.Left) (($outer.Bottom - $outer.Top) + [int][Math]::Ceiling(($GlyphRows - $p.rows) * $p.rect.h / $p.rows))
    Wait-Shown { param($s) (Get-ActivePane $s).rows -ge $GlyphRows } "the window $GlyphRows rows tall"
    @{ Window = $w }
  }
  Mark = { param($r, $i) $r.N = $i + 1 }
  Do = { param($r, $i) Send-Key $r.Window G }
  Shown = { param($w, $s, $r) (Get-ActivePane $w).title -eq "glyphs $($r.N)" }
}

# `lbterm cli list`, as the Conductor, Relay and the hooks call it, against the mux server with
# twelve tabs: the call from start to exit
$Actions['cli-list'] = @{
  Setup = { & $Setups['twelve-tabs'] }
  Call = $true
  Do = { param($r, $i) Invoke-Cli list --format json -Server:$Mux -Check | Out-Null }
}
# A frame with nothing new in it: the window told to paint (InvalidateRect, as Windows does when
# it's uncovered: no input, no focus) over two panes of text it has drawn before, to that frame.
# What drawing a screen of lines found in the caches costs, every line looked up and its quads
# copied in (lbterm/PERF.md, "What upstream's metrics cost")
Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LbtermBudgetPaint {
  [DllImport("user32.dll")] static extern bool InvalidateRect(IntPtr h, IntPtr rect, bool erase);
  public static bool Paint(IntPtr h) { return InvalidateRect(h, IntPtr.Zero, false); }
}
'@
$Actions['repaint'] = @{
  Setup = {
    $r = Open-OneTab
    Send-Key $r.Window F7
    Wait-Shown { param($s) @($s.panes).Count -eq 2 -and $s.settled } 'a tab split in two'
    $text = "1..120 | % { '{0,4} the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ' -f `$_ }`r"
    foreach ($p in @(Get-Panes -Server:$Mux)) { Invoke-Cli send-text --pane-id $p.pane_id --no-paste $text -Server:$Mux -Check | Out-Null }
    Start-Sleep -Seconds 2
    $r
  }
  Mark = { param($r, $i) $r.F = [long](Get-Win).frame }
  Do = { param($r, $i) [void][LbtermBudgetPaint]::Paint([IntPtr][long]$r.Window.Handle) }
  Shown = { param($w, $s, $r) [long]$w.frame -gt $r.F }
}

# The window made 80 px wider or narrower (SetWindowPos, which returns once the window has its
# new size): to the first frame at the new size
$Actions['resize'] = @{
  Setup = { Open-OneTab }
  Mark = { param($r, $i)
    $r.W = (Get-Win).size.width
    $wr = Get-WindowRect $r.Window
    $r.H = $wr.Bottom - $wr.Top
    $r.To = ($wr.Right - $wr.Left) + $(if ($i % 2) { 80 } else { -80 })
  }
  Do = { param($r, $i) Set-WindowSize $r.Window $r.To $r.H }
  Shown = { param($w, $s, $r) $w.size.width -ne $r.W }
}

# A writer at full speed: pwsh writing 16 MB of text (lines of 104 bytes) to its console in 1 MB
# writes once a key reaches it, and saying in its title when it's done, how long its writes took
# and how many bytes. To the first frame with that title: what it wrote shown, in MB a second. The
# title goes as OSC 0 ended by BEL on the same stream (through Console.Out, or ended by ST, the
# console host doesn't pass it on)
$WriterScript = @'
$e = [char]27
$line = ' the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk'
$sb = [Text.StringBuilder]::new()
for ($i = 0; $i -lt 10082; $i++) { [void]$sb.Append($i.ToString().PadLeft(6)).Append($line).Append("`r`n") }
$chunk = [Text.Encoding]::UTF8.GetBytes($sb.ToString())
$out = [Console]::OpenStandardOutput()
function Title([string]$t) { $b = [Text.Encoding]::UTF8.GetBytes("$e]0;$t$([char]7)"); $out.Write($b, 0, $b.Length); $out.Flush() }
$n = 0
# The first title a program sets doesn't reach the pane (a probe's first never did, its second
# always): a line, a title of no use, then 'ready'
$b = [Text.Encoding]::UTF8.GetBytes("ready`r`n"); $out.Write($b, 0, $b.Length)
Title 'starting'
Start-Sleep -Milliseconds 200
Title 'ready'
while ($true) {
  [void][Console]::ReadKey($true)
  $n++
  $clock = [Diagnostics.Stopwatch]::StartNew()
  for ($k = 0; $k -lt 16; $k++) { $out.Write($chunk, 0, $chunk.Length) }
  $out.Flush()
  $ms = [int]$clock.Elapsed.TotalMilliseconds
  Title "wrote $n $ms $(16 * $chunk.Length)"
}
'@
$Actions['throughput'] = @{
  Setup = {
    $w = Open-BudgetWindow -Program (New-PwshProgram $WriterScript)
    Wait-Shown { param($s) $p = Get-ActivePane $s; $p.title -eq 'ready' -and $p.cursor.y -ge 1 } 'the writer ready' 30
    @{ Window = $w }
  }
  Mark = { param($r, $i) $r.N = $i + 1 }
  Do = { param($r, $i) Send-Key $r.Window G }
  Shown = { param($w, $s, $r) (Get-ActivePane $w).title -match "^wrote $($r.N) " }
  Extra = { param($shown)
    $t = (Get-ActivePane $shown.Window).title -split ' '
    @{ writer_ms = [double]$t[2]; bytes = [double]$t[3] }
  }
  TimeoutSec = 120
}
$Actions['throughput-local'] = $Actions['throughput']

# The output the other writers print, made once here and written to a file each writer reads:
# deterministic, so every build gets the same bytes. Unicode is lbterm/bench's pattern (lines 80
# columns wide of wide CJK and Hangul, emoji, accented and combining Latin, Greek, Cyrillic and box
# drawing); Rgb is Claude Code's look, its 24-bit colours (38;2 and 48;2: dim and bright text, a
# diff's rows on a row-wide background, the boxes' borders) and the file paths its tools print,
# which Lachlan's hyperlink rules make links of; Lines is plain 80-column lines, bench's scrollback
if (-not ('LbtermBudgets.Output' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Text;
namespace LbtermBudgets {
  public static class Output {
    static byte[] Bytes(StringBuilder sb) { return Encoding.UTF8.GetBytes(sb.ToString()); }
    public static byte[] Unicode(int width, int target) {
      string[] wide = { "漢", "字", "表", "示", "中", "文", "日", "本", "語", "한", "국", "어", "テ", "ス", "ト" };
      string[] emoji = { "\U0001F600", "\U0001F680", "\U0001F389", "\U0001F30F", "\U0001F525", "\U0001F4BB", "\U0001F40D", "\U0001F355" };
      string[] narrow = { "α", "β", "γ", "δ", "Ж", "З", "И", "é", "ü", "ñ", "ç", "e\u0301", "a\u0308", "─", "│", "┌", "┐", "└", "┘", "├", "┤", "€", "£" };
      var sb = new StringBuilder(); long k = 0;
      while (Encoding.UTF8.GetByteCount(sb.ToString()) < target) {
        for (int line = 0; line < 64; line++) {
          int col = 0;
          while (col < width) {
            long pick = (k * 2654435761L) % 7; k++;
            if (pick < 2 && col + 2 <= width) { sb.Append(wide[k % wide.Length]); col += 2; }
            else if (pick == 2 && col + 2 <= width) { sb.Append(emoji[k % emoji.Length]); col += 2; }
            else if (pick == 3) { sb.Append(' '); col += 1; }
            else { sb.Append(narrow[k % narrow.Length]); col += 1; }
          }
          sb.Append("\r\n");
        }
      }
      return Bytes(sb);
    }
    public static byte[] Rgb(int target) {
      const string e = "\x1b[";
      string dim = e + "38;2;153;153;153m", white = e + "38;2;255;255;255m", border = e + "38;2;136;136;136m";
      string green = e + "38;2;78;186;101m", accent = e + "38;2;177;185;249m", added = e + "48;2;34;92;43m", removed = e + "48;2;122;41;54m";
      string rule = new string('\u2500', 96);
      var sb = new StringBuilder(); int k = 0;
      while (sb.Length < target) {
        for (int line = 0; line < 64; line++, k++) {
          switch (k % 8) {
            case 0: sb.Append(border).Append('\u256D').Append(rule).Append('\u256E').Append(e).Append("39m"); break;
            case 1: sb.Append(green).Append("\u25CF ").Append(e).Append("1m").Append(white).Append("Read").Append(e).Append("22m").Append(dim)
                      .Append("(C:/Users/demo/project/src/module").Append(k % 997).Append("/notes").Append(k).Append(".md)").Append(e).Append("39m"); break;
            case 2: sb.Append(dim).Append("  \u2514  Read ").Append(k).Append(" lines (ctrl+o to expand)").Append(e).Append("39m"); break;
            case 3:
            case 4: sb.Append(added).Append(dim).Append((k % 99991).ToString().PadLeft(6)).Append(' ').Append(white)
                      .Append(("+   let value" + k + " = compute(input, &options[" + (k % 97) + "]);").PadRight(90)).Append(e).Append("39m").Append(e).Append("49m"); break;
            case 5: sb.Append(removed).Append(dim).Append((k % 99991).ToString().PadLeft(6)).Append(' ').Append(white)
                      .Append(("-   let value" + k + " = compute(input, options[" + (k % 97) + "]);").PadRight(90)).Append(e).Append("39m").Append(e).Append("49m"); break;
            case 6: sb.Append(white).Append("The change in ").Append(accent).Append("C:/Users/demo/project/CLAUDE.md").Append(white)
                      .Append(" keeps line ").Append(k).Append(", as ").Append(e).Append("3mnoted").Append(e).Append("23m before.").Append(e).Append("39m"); break;
            default: sb.Append(border).Append('\u2570').Append(rule).Append('\u256F').Append(e).Append("39m"); break;
          }
          sb.Append("\r\n");
        }
      }
      return Bytes(sb);
    }
    public static byte[] Lines(int width, int count) {
      var sb = new StringBuilder();
      for (long line = 0; line < count; line++) {
        for (int i = 0; i < width; i++) sb.Append((char)(32 + (int)((line * 7 + i * 3) % 95)));
        sb.Append("\r\n");
      }
      return Bytes(sb);
    }
  }
}
'@
}

# A writer of a file's bytes, $Times over a run, once a key reaches it: the writer above with its
# chunk read from the file, which the scenario's setup makes in the test's folder
function New-FileWriterScript([Parameter(Mandatory)][string]$Data, [int]$Times = 16) {
  @"
`$e = [char]27
`$chunk = [IO.File]::ReadAllBytes('$Data')
`$out = [Console]::OpenStandardOutput()
function Title([string]`$t) { `$b = [Text.Encoding]::UTF8.GetBytes("`$e]0;`$t`$([char]7)"); `$out.Write(`$b, 0, `$b.Length); `$out.Flush() }
`$n = 0
`$b = [Text.Encoding]::UTF8.GetBytes("ready``r``n"); `$out.Write(`$b, 0, `$b.Length)
Title 'starting'
Start-Sleep -Milliseconds 200
Title 'ready'
while (`$true) {
  [void][Console]::ReadKey(`$true)
  `$n++
  `$clock = [Diagnostics.Stopwatch]::StartNew()
  for (`$k = 0; `$k -lt $Times; `$k++) { `$out.Write(`$chunk, 0, `$chunk.Length) }
  `$out.Flush()
  `$ms = [int]`$clock.Elapsed.TotalMilliseconds
  Title "wrote `$n `$ms `$($Times * `$chunk.Length)"
}
"@
}
# The writer's window, its data made first: as throughput's, with the data's writer
function Open-FileWriter([string]$Name, [byte[]]$Bytes, [int]$Times = 16) {
  $data = Join-Path $T.Out "$Name.bin"
  [IO.File]::WriteAllBytes($data, $Bytes)
  $w = Open-BudgetWindow -Program (New-PwshProgram (New-FileWriterScript -Data $data -Times $Times))
  Wait-Shown { param($s) $p = Get-ActivePane $s; $p.title -eq 'ready' -and $p.cursor.y -ge 1 } 'the writer ready' 30
  @{ Window = $w }
}
function New-FileWriterAction([scriptblock]$Setup) {
  @{
    Setup = $Setup
    Mark = $Actions['throughput'].Mark
    Do = $Actions['throughput'].Do
    Shown = $Actions['throughput'].Shown
    Extra = $Actions['throughput'].Extra
    TimeoutSec = 180
  }
}
# 16 MB of wide characters and emoji: what shaping and the fallback fonts cost at full speed
$Actions['throughput-unicode'] = New-FileWriterAction { Open-FileWriter 'unicode' ([LbtermBudgets.Output]::Unicode(80, 1MB)) }
$Actions['throughput-unicode-local'] = $Actions['throughput-unicode']
# 16 MB of Claude Code's 24-bit colours and file paths: what a cell's attributes and the links cost
$Actions['throughput-rgb'] = New-FileWriterAction { Open-FileWriter 'rgb' ([LbtermBudgets.Output]::Rgb(1MB)) }
$Actions['throughput-rgb-local'] = $Actions['throughput-rgb']
# 100,032 lines of 80 columns, written 12,504 at a time, into Lachlan's scrollback of 50,000: the
# memory the scrollback and the GUI's caches of what it drew hold once it's full (the warm-up
# fills it; each run after prints it all again)
$Actions['scrollback'] = New-FileWriterAction { Open-FileWriter 'lines' ([LbtermBudgets.Output]::Lines(80, 12504)) -Times 8 }
$Actions['scrollback-local'] = $Actions['scrollback']
# A spinner's line written again in place, as Claude Code's spinner and timer are while it works,
# in a window like Lachlan's: his window's 1080 by 1920, the Conductor's pad accepting most with
# its outlines round two rows (conductor-pad's), a screenful of coloured lines printed first. A key
# reaching the program writes its last line again, a dot longer or back to none, so the cursor
# moves: to the frame with it where it went. What a frame of one changed row costs there, the
# rail's and the cards' templates and the layers' buffers kept (lbterm/COMPOSITOR.md, "What a
# frame costs")
$SpinnerScript = @'
$e = [char]27
for ($i = 1; $i -le 150; $i++) { [Console]::Out.Write("$e[38;5;$(17 + $i % 214)mline $i$e[0m the quick brown fox jumps over the lazy dog $e[1;34m$i ms$e[0m and on to the end of the line`n") }
$f = '|/-\'
$n = 0
[Console]::Out.Write("$e[38;5;208m|$e[0m Thinking... ")
while ($true) {
  [void][Console]::ReadKey($true)
  $n++
  [Console]::Out.Write("`r$e[2K$e[38;5;208m$($f[$n % 4])$e[0m Thinking... $e[2m$('.' * ($n % 7))$e[0m")
}
'@
$Actions['spinner-tall'] = @{
  Setup = {
    $r = & $Setups['conductor-pad']
    $script:SpinnerFocus = '{"type":"conductor.focus","sessions":[{"sessionId":"' + (Get-Sid 41) + '","doing":"thinking"},{"sessionId":"' + (Get-Sid 42) + '","doing":"acting"}]}'
    $outer = Get-WindowRect $r.Window; $client = Get-ClientRect $r.Window
    Set-WindowSize $r.Window (1080 + ($outer.Right - $outer.Left) - $client.Width) (1920 + ($outer.Bottom - $outer.Top) - $client.Height)
    Wait-Shown { param($s) $s.size.width -eq 1080 -and $s.size.height -eq 1920 } 'the window at 1080 by 1920'
    $file = Join-Path $T.Out 'spinner.ps1'
    [IO.File]::WriteAllText($file, $SpinnerScript)
    Invoke-Cli send-text --pane-id $script:FirstPane --no-paste "& '$file'`r" -Server:$Mux -Check | Out-Null
    Wait-Shown { param($s) $p = Get-ActivePane $s; $p.cursor.y -eq $p.rows - 1 -and $p.cursor.x -eq 14 } 'a screenful and the spinner' 30
    $r
  }
  # The hub's word lingers, then goes: said again when the outlines have
  Before = { param($r, $i)
    if (@((Get-Win).rail.rows | Where-Object conductor).Count -lt 2) {
      Send-PlayedLine $script:Hub $script:SpinnerFocus | Out-Null
      Wait-Shown { param($s) @($s.rail.rows | Where-Object conductor).Count -eq 2 } 'the outlines back'
    }
  }
  Mark = { param($r, $i) $c = Get-DrawnCursor (Get-ActivePane (Get-Win)); $r.X = $c.x }
  Do = { param($r, $i) Send-Key $r.Window X }
  Shown = { param($w, $s, $r) (Get-DrawnCursor (Get-ActivePane $w)).x -ne $r.X }
}

# The cycles a run's cost is read from: the GUI thread's, the process's, the state file's writer's
# (a test window's alone, taken off the process's) and the mux servers'. Cheap enough to read just
# before what's timed.
function Get-ActionCycles($w, [int[]]$Servers) {
  [pscustomobject]@{
    Thread  = [LbtermE2E.Cpu]::ThreadCycles([uint32]$script:GuiTid)
    Process = [LbtermE2E.Cpu]::ProcessCycles([uint32]$w.Pid)
    State   = $(if ($script:StateTid) { [LbtermE2E.Cpu]::ThreadCycles([uint32]$script:StateTid) } else { 0 })
    Servers = @($Servers | ForEach-Object { [pscustomobject]@{ Pid = $_; Cycles = [LbtermE2E.Cpu]::ProcessCycles([uint32]$_) } })
    # The servers' threads by name, so a server that spends on a run says where; and the GUI
    # process's, for what its threads besides the GUI thread spend
    ServerThreads = @(Get-ServerThreadCycles $Servers)
    Threads = @(Get-ServerThreadCycles @($w.Pid))
    # What the GUI process and the servers hold, at the run's end the memory one left behind
    PrivateBytes = (Get-Process -Id $w.Pid -ErrorAction SilentlyContinue).PrivateMemorySize64
    ServerPrivateBytes = [long](@($Servers | ForEach-Object { (Get-Process -Id $_ -ErrorAction SilentlyContinue).PrivateMemorySize64 }) | Measure-Object -Sum).Sum
  }
}

# The state file's writer thread of a window's process, once it has written: its id
function Get-StateThreadId($w) {
  @((Get-CpuSnapshot $w).ThreadList | Where-Object Name -eq 'lbterm-state')[0].Tid
}

# One run of action $A in the window $R set up: the window at rest first, then the action, timed
# by the frames the recorder kept, its cost to the window quiet again, then undone
function Invoke-ActionRun($A, $R, [int]$I, [bool]$Warmup) {
  $w = $R.Window
  if ($A.Before) { & $A.Before $R $I }
  # A window kept busy on purpose (a writer beside the pane) never goes quiet: it's given 300 ms
  if ($A.Busy) { Start-Sleep -Milliseconds 300 } else { Get-QuietFrame -QuietMs 300 -TimeoutSec 20 -Window $w | Out-Null }
  if ($A.Mark) { & $A.Mark $R $I }
  $hwnd = [long]$w.Handle
  $queue = [Collections.Generic.List[object]]::new()
  # What the recorder kept before this run isn't this run's
  $Recorder.Look()
  [void]$Recorder.Take()
  if ($hasMetrics) { Get-Metrics -Window $w -Reset | Out-Null }
  $p0 = [long](Read-State -Window $w).frame
  # What the rest of the PC does over the run, from here to its cycles read after
  $pcw = Start-PcWatch -ProcessId $w.Pid -Roots (@($w.Pid) + @($servers))
  $c0 = Get-ActionCycles $w $servers
  $t0 = Get-UnixMs
  & $A.Do $R $I
  $shown = $null
  # The first frame after it, whatever it shows: when it ended
  $first = @{ Ms = $null }
  if ($A.Call) { $shownMs = Get-UnixMs; $settledMs = $null }
  else {
    $timeout = if ($A.TimeoutSec) { $A.TimeoutSec } else { 15 }
    $shown = Wait-Recorded -Recorder $Recorder { param($x, $s) if ($null -eq $first.Ms) { $first.Ms = [double]$s.time_ms }; & $A.Shown $x $s $R } -Hwnd $hwnd -AfterFrame $p0 -Queue $queue -TimeoutSec $timeout -What "$($Sc.Name): the frame that shows it"
    $settled = if (-not $shown.Window.animating) { $shown } else {
      Wait-Recorded -Recorder $Recorder { param($x, $s) -not $x.animating } -Hwnd $hwnd -Queue $queue -TimeoutSec 15 -What "$($Sc.Name): its motion over"
    }
    $shownMs = [double]$shown.TimeMs
    $settledMs = [double]$settled.TimeMs
  }
  if ($A.Busy) { Start-Sleep -Milliseconds 300 } else { Get-QuietFrame -QuietMs 300 -TimeoutSec 30 -Window $w | Out-Null }
  $c1 = Get-ActionCycles $w $servers
  $pcs = Stop-PcWatch $pcw -Roots (@($w.Pid) + @($servers))
  $p1 = [long](Read-State -Window $w).frame
  $m = if ($hasMetrics) { Get-Metrics -Window $w }
  $extra = if ($A.Extra -and $shown) { & $A.Extra $shown $R } else { @{} }
  $sample = ConvertTo-ActionSample -StartMs $t0 -ShownMs $shownMs -SettledMs $settledMs -Before $c0 -After $c1 -Frames ($p1 - $p0) -Metrics $m -Gui $Sc.Gui -CycleRate $rate -Bytes $(if ($extra.bytes) { $extra.bytes } else { 0 }) -Warmup:$Warmup
  foreach ($k in $extra.Keys) { $sample[$k] = $extra[$k] }
  Add-PcSample $sample (ConvertTo-PcSpan -Start $pcw -Stop $pcs -CycleRate $rate) 'action' $Budgets.Pc | Out-Null
  # Which frame after it showed it (1, the first) and when the first ended: said, not judged
  if ($shown) {
    $sample.shown_frame = [long]$shown.Frame - $p0
    $sample.first_frame_ms = [Math]::Round($first.Ms - $t0, 3)
  }
  if ($A.After) { & $A.After $R $I }
  $sample
}

# A start, from nothing: the GUI process started (and on a mux domain its server, by the GUI), to
# its first frame and to the first with its tab's pane in it; its whole cost to the window quiet.
# Each run's window and server are ended before the next, which has a socket of its own.
$Actions['start'] = @{
  Setup = { @{ Window = $null; Focus = $false } }
  Run = { param($r, $i, $warmup)
    foreach ($old in @((Get-Test).Windows)) { if (Test-WindowAlive $old) { Stop-TestWindow $old } }
    Stop-TestServers
    $n = (Get-Test).Windows.Count
    $stateFile = Join-Path $T.Out "state$(if ($n) { "-$n" }).json"
    $sock = Join-Path (Split-Path $T.Sock) "s$i"
    $cfg = Join-Path $T.Out "config\start-$i.lua"
    $domain = if ($Mux) { @"
config.unix_domains = { {
  name = 'budgets',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@ }
    New-TestConfig -Path $cfg -Lua ($BaseLua + "`n" + $domain)
    $rec = Start-FrameRecorder -Path $stateFile
    try {
      $open = @{ ConfigFile = $cfg; UserProfile = $HomeDir; TimeoutSec = 30 }
      if ($Mux) { $open.Domain = 'budgets'; $open.Socket = $sock } else { $open.Program = New-PwshProgram }
      $w = Open-TestWindow @open
      # From when it was let run, after the handoff to the test's job (not its creation)
      $started = ($w.Ran.ToUniversalTime() - [DateTime]::UnixEpoch).TotalMilliseconds
      $queue = [Collections.Generic.List[object]]::new()
      $first = Wait-Recorded -Recorder $rec { param($x, $s) $x.frame -ge 1 } -Hwnd ([long]$w.Handle) -Queue $queue -TimeoutSec 30 -What 'its first frame'
      # Its tab's pane: on a mux domain the server's (a server_id), not the "Connecting..." pane
      # the GUI shows while it starts the server and connects
      $isReady = { param($x) @($x.panes | Where-Object { -not $Mux -or $null -ne $_.server_id }).Count -ge 1 }
      $ready = if (& $isReady $first.Window) { $first } else {
        Wait-Recorded -Recorder $rec { param($x, $s) & $isReady $x } -Hwnd ([long]$w.Handle) -Queue $queue -TimeoutSec 30 -What 'its tab''s pane'
      }
      Get-QuietFrame -QuietMs 500 -TimeoutSec 30 -Window $w | Out-Null
      $servers = @(Get-TestServers | ForEach-Object { [int]$_.ProcessId })
      $script:GuiTid = Get-GuiThreadId $w
      $script:StateTid = Get-StateThreadId $w
      $c1 = Get-ActionCycles $w $servers
      $c0 = [pscustomobject]@{ Thread = 0; Process = 0; State = 0; Servers = @($servers | ForEach-Object { [pscustomobject]@{ Pid = $_; Cycles = 0 } }) }
      $sample = ConvertTo-ActionSample -StartMs $started -ShownMs $first.TimeMs -SettledMs $null -Before $c0 -After $c1 -Frames ([long](Read-State -Window $w).frame) -Metrics $null -CycleRate $rate -Warmup:$warmup
      $sample.ready_ms = [Math]::Round([double]$ready.TimeMs - $started, 3)
      # Where its start went (metrics' start: the process's marks from main, its window's steps,
      # its GPU's), said and not judged; a build without them has none
      $m = try { Get-Metrics -Window $w } catch { $null }
      if ($m -and $m.start) {
        $sample.marks = $m.start.marks
        foreach ($k in @('window', 'webgpu', 'gpu-adapter')) { if ($m.start.$k) { $sample["$($k -replace '-', '_')_start"] = $m.start.$k } }
      }
      $sample
    } finally { $rec.Dispose() }
  }
}
$Actions['start-local'] = $Actions['start']

#endregion

# Every span, judged at the end; measure.json has them all
$samples = [Collections.Generic.List[object]]::new()
$lines = @()
$actionLines = @()
Invoke-Test {
  if ($IsAction) {
    $act = $Actions[$Sc.Name]
    if (-not $act) { throw "budgets.json names the action $($Sc.Name), which budgets.ps1 has no setup for" }
    Initialize-CpuReader
    $rate = Get-CycleRate
    $total = $Sc.Warmup + $Runs
    "$($Sc.Name): $($Sc.What)"
    $r = & $act.Setup
    $w = $r.Window
    # Runs go on until -Runs were measured on a calm PC, or Retakes more than -Runs were taken
    $more = { param($i, $calm, $judged) $i -lt $Sc.Warmup -or ($calm -lt $Runs -and $judged -lt $Runs + $Retakes) }
    $calm = 0; $judged = 0
    if ($act.Run) {
      "  $total runs ($($Sc.Warmup) to warm up)$(if ($Retakes) { ", up to $Retakes more in place of busy ones" })"
      for ($i = 0; (& $more $i $calm $judged); $i++) {
        # A start's window is the run's own: the PC watched from this process, its tree the start's
        $pcw = Start-PcWatch
        $sample = & $act.Run $r $i ($i -lt $Sc.Warmup)
        $pcs = Stop-PcWatch $pcw -Roots (@((Get-Test).Windows | Where-Object { Test-WindowAlive $_ } | ForEach-Object { [int]$_.Pid }) + @(Get-TestServers | ForEach-Object { [int]$_.ProcessId }))
        Add-PcSample $sample (ConvertTo-PcSpan -Start $pcw -Stop $pcs -CycleRate $rate) 'action' $Budgets.Pc | Out-Null
        if (-not $sample.warmup) { $judged++; if (-not $sample.pc.busy) { $calm++ } }
        $samples.Add($sample)
        "  run ${i}$(if ($sample.warmup) { ' (warm-up)' }): $($sample.ms) ms to its first frame, $($sample.ready_ms) ms to its pane; the GUI thread $($sample.gui_thread_ms) ms, the process $($sample.cpu_ms) ms, the server $($sample.server_cpu_ms) ms, $($sample.frames) frames$(Format-PcSample $sample)"
      }
      return
    }
    if ($r.Focus -ne $false) { Send-Focus $w }
    Start-PointerWatch $w
    $hasMetrics = $true
    try { Get-Metrics -Window $w | Out-Null } catch { $hasMetrics = $false; $Notes.Add("no metrics from this build ($($_.Exception.Message)): times, frames and CPU only") }
    try { Wait-Settled -Window $w -TimeoutSec 8 -What 'the window to settle' | Out-Null } catch { $Notes.Add("it never settled before the first run: $($_.Exception.Message)") }
    $servers = @(Get-TestServers | ForEach-Object { [int]$_.ProcessId })
    if ($Mux -and -not $servers) { throw 'no mux server of the test''s running' }
    $script:GuiTid = Get-GuiThreadId $w
    $script:StateTid = Get-StateThreadId $w
    if (-not $script:StateTid) { $Notes.Add('no lbterm-state thread found: the state file''s writing is in the process''s CPU') }
    $Recorder = Start-FrameRecorder -Window $w
    "  $total runs ($($Sc.Warmup) to warm up)$(if ($Retakes) { ", up to $Retakes more in place of busy ones" }); the GUI $($w.Pid), its servers $($servers -join ', '); $(if ($hasMetrics) { 'metrics' } else { 'no metrics' })"
    try {
      for ($i = 0; (& $more $i $calm $judged); $i++) {
        $sample = Invoke-ActionRun $act $r $i ($i -lt $Sc.Warmup)
        if (-not $sample.warmup) { $judged++; if (-not $sample.pc.busy) { $calm++ } }
        $samples.Add($sample)
        "  run ${i}$(if ($sample.warmup) { ' (warm-up)' }): $($sample.ms) ms$(if ($null -ne $sample.gui_ms) { " (the GUI's $($sample.gui_ms))" }), settled $($sample.settled_ms) ms; the GUI thread $($sample.gui_thread_ms) ms, the process $($sample.cpu_ms) ms, the server $($sample.server_cpu_ms) ms, $($sample.frames) frames$(if ($null -ne $sample.messages) { ", $($sample.messages) messages ($($sample.wm_paint) WM_PAINT)" })$(if ($sample.mb_per_s) { ", $($sample.mb_per_s) MB/s (the writer's writes $($sample.writer_ms) ms)" })$(Format-PcSample $sample)"
      }
    } finally { $Recorder.Dispose() }
    # What the GUI said of its own time for this action: the runs over the scenario's gui_ms,
    # with what the time went on
    $said = @(Get-LogLines $w 'lbterm::budget' | ForEach-Object { ($_ -split ' > ', 2)[1] } | Where-Object { $_ })
    $script:actionLines = $said
    $script:lines = @($said | Where-Object { $b = ConvertFrom-LbtermBudgetLine $_; $b -and $b.Kind -eq 'action' -and $Sc.Gui -and $b.What -eq $Sc.Gui })
    return
  }
  $setup = $Setups[$Sc.Name]
  if (-not $setup) { throw "budgets.json names $($Sc.Name), which budgets.ps1 has no setup for" }
  $r = & $setup
  $w = $r.Window
  if ($r.Focus -ne $false) { Send-Focus $w }
  Start-PointerWatch $w

  # Whether this build answers metrics (one from before the counters says no op called)
  $hasMetrics = $true
  try { Get-Metrics -Window $w | Out-Null } catch { $hasMetrics = $false; $Notes.Add("no metrics from this build ($($_.Exception.Message)): frames and CPU only") }

  # Settled, then settle_ms more, so the GUI's budget takes it as at rest
  if (-not $r.NoSettle) {
    try { Wait-Settled -Window $w -TimeoutSec 8 -What 'the scenario to settle' | Out-Null }
    catch { $Notes.Add("it never settled: $($_.Exception.Message)") }
  }
  Start-Sleep -Milliseconds $Budgets.SettleMs
  $servers = @(Get-TestServers | ForEach-Object { [int]$_.ProcessId })
  $conpty = Get-ConptyIds (@($w.Pid) + $servers)
  if ($Mux -and -not $servers) { throw 'no mux server of the test''s running' }
  $rate = Get-CycleRate
  "$($Sc.Name): $($Sc.What)"
  "  measuring $Runs spans of $Seconds s$(if ($Retakes) { ", up to $Retakes more in place of busy ones" }); the GUI $($w.Pid), its servers $($servers -join ', '), $($conpty.Count) conpty hosts; $(if ($hasMetrics) { 'metrics' } else { 'no metrics' })"
  # Spans go on until -Runs were measured on a calm PC, or Retakes more than -Runs were taken
  $calm = 0
  for ($i = 1; $calm -lt $Runs -and $i -le $Runs + $Retakes; $i++) {
    if ($hasMetrics) { Get-Metrics -Window $w -Reset | Out-Null }
    $s0 = Read-State -Window $w
    $f0 = [long]$s0.frame
    $pcw = Start-PcWatch -ProcessId $w.Pid -Roots (@($w.Pid) + @($servers))
    $before = Get-CpuSnapshot $w -Servers $servers -Conpty $conpty
    Start-Sleep -Milliseconds ([int]($Seconds * 1000))
    $after = Get-CpuSnapshot $w -Servers $servers -Conpty $conpty
    $pcs = Stop-PcWatch $pcw -Roots (@($w.Pid) + @($servers))
    $state = Read-State -Window $w
    $m = if ($hasMetrics) { Get-Metrics -Window $w }
    # The lines laid out again, from the state file's work (a build before it has none)
    $lines = if ($null -ne $s0.work -and $null -ne $state.work) { [long]$state.work.lines - [long]$s0.work.lines }
    $sw = Get-StateWindow $state -Hwnd ([long]$w.Handle)
    $loops = if ($sw -and $sw.PSObject.Properties['loops']) { @($sw.loops | Where-Object { $_ }).Count }
    $sample = ConvertTo-RestSample -Before $before -After $after -Frames ([long]$state.frame - $f0) -Metrics $m -CycleRate $rate -Lines $lines -Loops $loops
    Add-PcSample $sample (ConvertTo-PcSpan -Start $pcw -Stop $pcs -CycleRate $rate) 'rest' $Budgets.Pc | Out-Null
    if (-not $sample.pc.busy) { $calm++ }
    $samples.Add($sample)
    "  span ${i}: $($sample.frames) frames/s, the GUI thread $($sample.gui_thread_ms) ms/s, the process $($sample.cpu_ms) ms/s, the server $($sample.server_cpu_ms) ms/s, conpty $($sample.conpty_cpu_ms) ms/s$(if ($hasMetrics) { ", $($sample.tasks) tasks/s, $($sample.timers) timers/s" })$(if ($null -ne $sample.lines) { ", $($sample.lines) lines/s" })$(if ($null -ne $sample.frame_ms) { ", $($sample.frame_ms) ms a frame" })$(if ($null -ne $sample.pass_ms) { " (pass $($sample.pass_ms))" })$(if ($null -ne $sample.loops) { ", $($sample.loops) loops" })$(Format-PcSample $sample)"
    if (-not (& $r.Holds $sw $w)) { throw "after span $i it isn't what it set up any more: $(Format-StateWindow $sw)" }
  }
  $sw = Get-StateWindow (Read-State -Window $w) -Hwnd ([long]$w.Handle)
  if (@($sw.loops)) { $Notes.Add("DWM's loops while measured: $(@($sw.loops) -join ', ')") }
  if ($sw.animating) { $Notes.Add("animating while measured: $(@($sw.animations | ForEach-Object name) -join ', ')") }
  # What the GUI's own budget said: what ran at rest is this scenario's; an action over its time
  # (start-up's first paints, the setup's own) is kept apart
  $said = @(Get-LogLines $w 'lbterm::budget' | ForEach-Object { ($_ -split ' > ', 2)[1] } | Where-Object { $_ })
  $script:lines = @($said | Where-Object { $b = ConvertFrom-LbtermBudgetLine $_; $b -and $b.Kind -ne 'action' })
  $script:actionLines = @($said | Where-Object { $_ -notin $script:lines })
} -Finally {
  $judged = Test-BudgetScenario -Scenario $Sc -Samples $samples -Lines $script:lines -Notes $Notes -Errors @((Get-Test).Failures)
  $measure = [ordered]@{
    scenario = $Sc.Name; domain = $Sc.Domain; what = $Sc.What
    build = (Get-BuildVersion $T.Bin); bin = $T.Bin; runs = $Runs; seconds = $Seconds; settle_ms = $Budgets.SettleMs
    pc_rules = [ordered]@{ others_max = $Budgets.Pc.OthersMax; pc_max = $Budgets.Pc.PcMax; retakes = $Budgets.Pc.Retakes }
    samples = @($samples); lines = @($script:lines); action_lines = @($script:actionLines); notes = @($Notes)
  }
  [IO.File]::WriteAllText((Join-Path $T.Out 'measure.json'), ($measure | ConvertTo-Json -Depth 10))
  if ($samples.Count) {
    $report = New-BudgetReport -Run 'one' -Bin $T.Bin -Build $measure.build -Started (Get-Date) -Results @($judged) -Runs $Runs -Seconds $Seconds
    Format-BudgetTable $report | Select-Object -SkipLast 2
    foreach ($o in $judged.over) { Add-Failure "over budget: $o" }
  }
  foreach ($n in $Notes) { "note: $n" }
} -Passed "budgets $($Sc.Name): within its budgets"
