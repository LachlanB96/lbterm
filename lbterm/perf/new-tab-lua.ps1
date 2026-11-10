#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/termwindow/render/new_tab.rs, lua-api-crates/mux/src/lib.rs, wezterm-client/src/domain.rs, wezterm-gui/src/termwindow/last_tab.rs, wezterm-gui/src/scripting/guiwin.rs
<#
Lachlan's own lbterm.lua with the new-tab pad (lbterm/NEW-TAB.md, landing 1, section 9: the keys,
and his lbterm-project-action and lbterm-reopen-tab handlers) in a test window of this build, of its
own class on the right-most screen, never taking focus. new-tab.ps1 tests LBTerm's side with a
handler of its own; this one tests his file's side against it, as it is (-Config: Get-LookalikeConfig,
his live lbterm.lua) or a copy of it (-Config <path>, a branch of Terminal's in a worktree).

His file runs as New-TestConfig runs it (no summon key, mux domain, Conductor, InMail or agenda
button, close marks, links, recent picks' file, reloading), through a stand-in for the wezterm
module that keeps what his handlers would start from starting: wezterm.background_child_process
writes its arguments to ran.jsonl instead (Conductor's record new, the Claude project's ai open),
his window's toast_notification to toasts.jsonl, and the tab his handler asks spawn_tab for is
written to spawned.jsonl and opened with `claude` swapped for a stand-in on the pinned Bun that
writes the arguments it was given to argv.jsonl, so his command line goes through the real
PowerShell to a program's argv and no Claude session starts. His handlers step aside in a test
window (LBTERM_STATE_FILE, which every one has): the stand-in hides it from them while they run,
until the test says the window is a test window again (step 8), when they must step aside.
Posted keys carry no Ctrl or Shift, so the actions his file binds to Ctrl+T, Ctrl+Alt+T,
Ctrl+Shift+T and Ctrl+W are written down (keys.json) and done through the control pipe's emit
(newtab-lua-picker, -shell, -reopen, -close: perform_action with his action), which keys Lachlan
holds down while it runs can't turn into others, and Shift+Enter is a Shift+click.

Every wait is on the state file or a file the stand-ins write, with deadlines far past what a
loaded PC takes; what's judged is the order things came in and what they said, never milliseconds
(Lachlan, 2026-11-03).

0. In a build without the pad (-Bin a copy of the one installed before landing 1; its state file has
   no new_tab), his file loads as it always has, with no Configuration Error window: Ctrl+T's action
   is SpawnTab and opens a tab, Ctrl+Alt+T and Ctrl+Shift+T are unbound, and that's all it checks.
1. His keys: Ctrl+T the picker, Ctrl+Alt+T a shell, Ctrl+Shift+T Reopen (LbtermNewTab).
2. His Ctrl+T's action opens the picker; "terminal fix it" and Enter: his handler opens Claude's tab in
   front in Terminal's folder, the Starting row going into it, "fix it" its first message as
   Claude's argv has it, and Conductor's record new with --by 'new-tab picker' and his pick as
   --said, the same session id as the tab's.
2b. The open tab a PowerShell in a folder inside his Terminal project (shared\claude), as a pane's
   file:// URL says it: Enter on the Here row, highlighted as the picker opens, and "terminal"
   typed while it's Here each make his handler open Claude there, its project found by its own
   folder, and Conductor's record told that folder (lbterm/PITFALLS.md, 115: his handler had looked
   the folder up as text and opened LBTerm's PowerShell instead of Claude).
3. Shift+click on Warden's row: the tab opens behind (spawn_tab's activate false), the window
   staying on its tab, and Claude's argv with no first message.
4. Ctrl+click: a shell, which his handler leaves to LBTerm: PowerShell in Warden's folder, nothing
   of his run.
5. A first message with both kinds of quote (fired as the picker fires it): Claude's argv has it
   as typed.
6. His Ctrl+Alt+T's: a tab in the open tab's folder, nothing of his run.
7. Reopen: a tab whose pane says a Claude session, closed with his Ctrl+W's action: his
   Ctrl+Shift+T's runs ai open with its session id alone and opens no tab of its own, its Starting
   row folding once its 20 s are up (ai never runs here); a shell's tab closed and reopened: his
   handler leaves it to LBTerm, which opens it in its folder.
8. The window a test window again: a pick of Terminal opens LBTerm's default in its folder and
   nothing of his runs.

  pwsh -File lbterm\perf\new-tab-lua.ps1 [-Bin <build>] [-Config <lbterm.lua>]        # tabs in the GUI
  pwsh -File lbterm\perf\new-tab-lua.ps1 [-Bin <build>] [-Config <lbterm.lua>] -Mux   # tabs in a test lbterm-mux-server
#>
param([string]$Bin, [string]$Config, [switch]$Mux)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('new-tab-lua' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin
$sock = $T.Sock
if (-not $Config) { $Config = Get-LookalikeConfig }
if (-not (Test-Path -LiteralPath $Config)) { throw "no lbterm.lua at $Config" }

# His projects, as his file names them (names.lua), for the folders his picks open in
$names = Get-Content -Raw -LiteralPath (Join-Path $env:LOCALAPPDATA 'Megaproject\names.json') | ConvertFrom-Json
$bun = $names.bun
$dirOf = @{}
foreach ($p in $names.projects) { $dirOf[$p.name] = $p.dir }
foreach ($n in 'Terminal', 'Warden') { if (-not $dirOf[$n]) { throw "names.json names no project $n" } }

$spawned = Join-Path $T.Out 'spawned.jsonl'
$ran = Join-Path $T.Out 'ran.jsonl'
$toasts = Join-Path $T.Out 'toasts.jsonl'
$argvFile = Join-Path $T.Out 'argv.jsonl'
$keysFile = Join-Path $T.Out 'keys.json'
# The stand-in for claude: its arguments as its argv has them, one line a run
$standIn = Join-Path $T.Out 'claude-stand-in.ts'
[IO.File]::WriteAllText($standIn, @"
import { appendFileSync } from "node:fs";
appendFileSync($((ConvertTo-Json $argvFile)), JSON.stringify(process.argv.slice(2)) + "\n");
"@)

$cfg = Join-Path $T.Out 'config\new-tab-lua.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$domain = if ($Mux) {
  @"
config.unix_domains = { {
  name = 'perfnewtablua',
  socket_path = '$(ConvertTo-LuaPath $sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
[IO.File]::WriteAllText($cfg, @"
local real = require 'wezterm'
local encode = (real.serde and real.serde.json_encode) or real.json_encode
local function record(file, value)
  local f = io.open(file, 'a')
  if f then f:write(encode(value) .. '\n'); f:close() end
end
-- His handlers for the new tab, kept here and called through as_his (below); every other one his own
local his = {}
package.loaded['wezterm'] = setmetatable({
  on = function(name, fn)
    if name == 'lbterm-project-action' or name == 'lbterm-reopen-tab' then his[name] = fn; return end
    return real.on(name, fn)
  end,
  background_child_process = function(args) record([[$ran]], args) end,
}, { __index = real })
local config = dofile([[$Config]])
package.loaded['wezterm'] = real

-- As New-TestConfig leaves out what reaches outside the window
config.lbterm_summon = nil
config.unix_domains = nil
config.default_gui_startup_args = nil
config.lbterm_crash_restore = false
config.wsl_domains = {}
rawset(config, 'lbterm_conductor', nil)
rawset(config, 'lbterm_inbox', nil)
rawset(config, 'lbterm_agenda', nil)
rawset(config, 'lbterm_close_marks', nil)
rawset(config, 'lbterm_links', nil)
if rawget(config, 'lbterm_projects') then config.lbterm_projects.recent_file = nil end
config.automatically_reload_config = false
config.window_close_confirmation = 'NeverPrompt'

-- His keys for the new tab, written down, and their actions done when the test emits their event: a
-- posted key carries no Ctrl or Shift
local found = {}
local by = { ['t CTRL'] = 'picker', ['t CTRL|ALT'] = 'shell', ['t CTRL|SHIFT'] = 'reopen', ['w CTRL'] = 'close' }
for _, k in ipairs(config.keys or {}) do
  local name = tostring(k.key) .. ' ' .. tostring(k.mods)
  if by[name] then
    local a = k.action
    found[name] = type(a) == 'table' and ((a.LbtermNewTab and ('LbtermNewTab ' .. a.LbtermNewTab))
      or (a.SpawnTab and 'SpawnTab') or (a.CloseCurrentTab and 'CloseCurrentTab')) or tostring(a)
    real.on('newtab-lua-' .. by[name], function(window, pane) window:perform_action(a, pane) end)
  end
end
local f = io.open([[$keysFile]], 'w')
if f then f:write(encode(found)); f:close() end

-- What his handler asks spawn_tab for, written down and opened with claude swapped for the stand-in
local STAND_IN = "& '$(ConvertTo-LuaPath $bun)' '$(ConvertTo-LuaPath $standIn)'"
local function stand_in(args)
  local out = {}
  for i, a in ipairs(args or {}) do
    out[i] = (type(a) == 'string' and a:match('^claude ')) and (STAND_IN .. a:sub(#'claude' + 1)) or a
  end
  return out
end
local function proxy(window)
  return setmetatable({
    mux_window = function()
      local mw = window:mux_window()
      return setmetatable({
        spawn_tab = function(_, t)
          record([[$spawned]], t)
          local copy = {}
          for k, v in pairs(t) do copy[k] = v end
          copy.args = stand_in(t.args)
          return mw:spawn_tab(copy)
        end,
      }, { __index = function(_, k) return function(_, ...) return mw[k](mw, ...) end end })
    end,
    toast_notification = function(_, title, text) record([[$toasts]], { title = title, text = text }) end,
  }, { __index = function(_, k) return function(_, ...) return window[k](window, ...) end end })
end
-- His handler as it runs in his own window, LBTERM_STATE_FILE hidden from it, until a test window again
local mine = true
local function as_his(name, window, pane, p)
  local fn = his[name]
  if not fn then return end
  if not mine then return fn(window, pane, p) end
  local getenv = os.getenv
  os.getenv = function(k) if k == 'LBTERM_STATE_FILE' then return nil end return getenv(k) end
  local ok, r = pcall(fn, proxy(window), pane, p)
  os.getenv = getenv
  if not ok then error(r) end
  return r
end
real.on('lbterm-project-action', function(window, pane, p) return as_his('lbterm-project-action', window, pane, p) end)
real.on('lbterm-reopen-tab', function(window, pane, r) return as_his('lbterm-reopen-tab', window, pane, r) end)
real.on('newtab-lua-test-window', function() mine = false end)
-- A pick behind of Terminal with a first message of both kinds of quote, as the picker fires it
real.on('newtab-lua-quotes', function(window, pane)
  return as_his('lbterm-project-action', window, pane, { name = 'Terminal', dir = [[$($dirOf['Terminal'])]], action = 'new',
    prompt = 'say "hi" and don\'t', behind = true, source = 'picker', mail = 0, sessions = 0 })
end)
$domain
return config
"@)

# A file's lines, each its JSON, as one array never unrolled: a line can be an array itself (an
# argv), and one line alone would otherwise come back as that line's array
function Read-Lines([string]$Path) {
  $lines = [Collections.Generic.List[object]]::new()
  if (Test-Path $Path) { foreach ($l in Get-Content $Path) { if ($l) { $lines.Add(($l | ConvertFrom-Json -NoEnumerate)) } } }
  return , $lines.ToArray()
}
function Wait-Lines([string]$Path, [int]$Count, [string]$What) {
  if (-not (Wait-Until { (Read-Lines $Path).Count -ge $Count } -TimeoutSec 30)) { throw "$What never came ($((Read-Lines $Path).Count) in $(Split-Path -Leaf $Path))" }
  return , ((Read-Lines $Path)[$Count - 1])
}
function Send-Words($Window, [string]$Text) {
  foreach ($c in $Text.ToCharArray()) { if ($c -eq ' ') { Send-Key $Window Space } else { Send-Key $Window ([string]$c) } }
}
function ConvertTo-Folder([string]$p) { ([uri]::UnescapeDataString(($p -replace '^file:///', '')) -replace '/', '\').TrimEnd('\').ToLowerInvariant() }
# A start asked for once it's ended, later than the last one waited for (new-tab.ps1's Wait-Start)
$script:LastAsked = -1
function Wait-Start([string]$Label, [string]$What, [int]$TimeoutSec = 60) {
  $floor = $script:LastAsked
  $start = (Wait-State { param($s) $s.new_tab.start -and $s.new_tab.start.label -eq $Label -and $s.new_tab.start.ended -and [long]$s.new_tab.start.asked -gt $floor } -What $What -TimeoutSec $TimeoutSec).new_tab.start
  $script:LastAsked = [long]$start.asked
  $start
}
$uuid = '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'

Invoke-Test {
  $w = if ($Mux) { Open-TestWindow -ConfigFile $cfg -Domain perfnewtablua -Socket $sock -PostedPointer -WaitIdle -TimeoutSec 30 }
  else { Open-TestWindow -ConfigFile $cfg -PostedPointer -WaitIdle -TimeoutSec 30 }
  $s = Wait-State { param($s) $s.frame -gt 0 -and $s.rail -and @($s.rail.rows).Count -eq 1 } -What 'the window drawn, one row on its rail' -TimeoutSec 60
  if ($Mux) { Wait-State { param($s) @($s.panes | Where-Object { $null -ne $_.server_id }).Count -ge 1 } -What 'the server''s tab attached' -TimeoutSec 60 | Out-Null }
  $tabCount = { @((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).tabs).Count }

  # 0. A build from before landing 1 (no new_tab in its state file: the installed one until this
  # landing is): his file must load as it always has, Ctrl+T a PowerShell tab and the other two unbound
  if ($s.PSObject.Properties.Name -notcontains 'new_tab') {
    $keys = Get-Content -Raw $keysFile | ConvertFrom-Json
    "a build without the new-tab pad (its state file says no new_tab); his keys: $($keys | ConvertTo-Json -Compress)"
    if ($keys.'t CTRL' -ne 'SpawnTab') { Add-Failure "in a build without the picker his Ctrl+T is '$($keys.'t CTRL')', not SpawnTab" }
    foreach ($k in 't CTRL|ALT', 't CTRL|SHIFT') { if ($keys.$k) { Add-Failure "in a build without the picker his $k is bound, to $($keys.$k)" } }
    if (@((Get-Test).Notes | Where-Object { $_ -like '*Configuration Error*' }).Count -or @(Get-Panes -Window $w | Where-Object { Test-ConfigErrorPane $_ }).Count) {
      Add-Failure "this build put up a Configuration Error window for his file"
    }
    $tabs0 = & $tabCount
    Send-Emit 'newtab-lua-picker' -Window $w
    Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What "his Ctrl+T's action (SpawnTab) opening a tab" -TimeoutSec 60 | Out-Null
    'his Ctrl+T: a PowerShell tab, as before'
    if ((Read-Lines $spawned).Count -or (Read-Lines $ran).Count) { Add-Failure 'his handlers acted in a build without the picker' }
    (Get-Test).PassedLine = 'new-tab-lua: a build without the new-tab pad runs his file as before: no configuration error, Ctrl+T a PowerShell tab, Ctrl+Alt+T and Ctrl+Shift+T unbound'
    return
  }
  Wait-State { param($s) $null -ne $s.new_tab } -What 'the pad' -TimeoutSec 30 | Out-Null
  $inFolder = { param($dir) @(Get-Panes -Window $w -Server:$Mux | Where-Object { (ConvertTo-Folder ([string]$_.cwd)) -eq (ConvertTo-Folder $dir) }).Count }
  $openPicker = { param([string]$What)
    Wait-Settled -What "the window, before $What" | Out-Null
    Send-Emit 'newtab-lua-picker' -Window $w
    Wait-Change { param($s) $s.new_tab.picker -and $null -ne $s.new_tab.picker.shown } -What "his Ctrl+T's action opening the picker for $What" -TimeoutSec 30
  }

  # 1. His keys
  $keys = Get-Content -Raw $keysFile | ConvertFrom-Json
  "his keys: $($keys | ConvertTo-Json -Compress)"
  foreach ($want in @(@('t CTRL', 'LbtermNewTab Picker'), @('t CTRL|ALT', 'LbtermNewTab Shell'), @('t CTRL|SHIFT', 'LbtermNewTab Reopen'), @('w CTRL', 'CloseCurrentTab'))) {
    if ($keys.($want[0]) -ne $want[1]) { Add-Failure "his $($want[0]) is '$($keys.($want[0]))', not $($want[1])" }
  }

  # 2. A pick in front with a first message: his handler's tab, Claude's argv, Conductor's record
  $s = & $openPicker 'Terminal'
  Send-Words $w 'terminal fix it'
  $s = Wait-Change { param($s) $s.new_tab.picker.message -eq 'fix it' } -What 'the first message "fix it"'
  $h = @($s.new_tab.picker.rows)[$s.new_tab.picker.highlight]
  if ($h.name -ne 'Terminal') { Add-Failure "typed ""terminal fix it"", the highlight is $($h.name)" }
  $active0 = $s.active_tab
  Send-Key $w Enter
  $t = Wait-Lines $spawned 1 'his spawn_tab for Terminal'
  $start = Wait-Start 'Terminal' 'the Starting row for Terminal to end'
  if ($start.ended -ne 'arrived') { Add-Failure "the start for Terminal $($start.ended)" }
  Wait-State { param($s) $s.active_tab -ne $active0 -and -not (@($s.rail.rows) | Where-Object pending) } -What 'his tab in front' -TimeoutSec 60 | Out-Null
  "spawned: $($t | ConvertTo-Json -Compress -Depth 4)"
  if ((ConvertTo-Folder $t.cwd) -ne (ConvertTo-Folder $dirOf['Terminal'])) { Add-Failure "his tab asked for folder $($t.cwd), not Terminal's" }
  if ($null -ne $t.activate) { Add-Failure "a pick in front asked spawn_tab for activate = $($t.activate)" }
  $argv = Wait-Lines $argvFile 1 "Claude's arguments for Terminal"
  "argv: $($argv | ConvertTo-Json -Compress)"
  if ($argv.Count -ne 4 -or $argv[0] -ne '--session-id' -or $argv[1] -notmatch $uuid -or $argv[2] -ne '--' -or $argv[3] -ne 'fix it') { Add-Failure "Claude was given $($argv | ConvertTo-Json -Compress)" }
  $rec = Wait-Lines $ran 1 "Conductor's record new"
  $tail = @($rec)[2..($rec.Count - 1)]
  "ran: $($tail -join ' ')"
  $want = @('record', 'new', $dirOf['Terminal'], '--pane', '*', '--session-id', $argv[1], '--by', 'new-tab picker', '--said', "picked Terminal in the new-tab picker in LBTerm, first message 'fix it'", '--prompt', 'fix it')
  if ($tail.Count -ne $want.Count) { Add-Failure "Conductor's record was $($tail -join ' ')" }
  else { for ($i = 0; $i -lt $want.Count; $i++) { if ($want[$i] -ne '*' -and $tail[$i] -ne $want[$i]) { Add-Failure "Conductor's record's $i is '$($tail[$i])', not '$($want[$i])'" } } }
  if (-not (Wait-Until { (& $inFolder $dirOf['Terminal']) -ge 1 } -TimeoutSec 30)) { Add-Failure "no pane in Terminal's folder" }

  # 2b. The Here row in a folder inside a project, and the project typed while it's Here
  $sub = Join-Path $dirOf['Terminal'] 'shared\claude'
  if (-not (Test-Path -LiteralPath $sub)) { throw "no folder $sub in his Terminal project" }
  $first = @(Get-Panes -Window $w -Server:$Mux)[0].pane_id
  $subPane = New-Tab -From $first -Cwd $sub -Window $w -Server:$Mux
  Invoke-Cli activate-pane --pane-id $subPane -Window $w -Server:$Mux | Out-Null
  if (-not (Wait-Until { (& $inFolder $sub) -ge 1 } -TimeoutSec 60)) { throw "the tab never said it's in $sub" }
  foreach ($typed in '', 'terminal') {
    $what = if ($typed) { """$typed"" typed while it's Here" } else { 'the Here row' }
    # Opened until its Here row is the sub folder's, as the window last heard the tab's folder (30 s)
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    while ($true) {
      $s = & $openPicker $what
      $here = @($s.new_tab.picker.rows)[0]
      if (($here.group -eq 'here' -and (ConvertTo-Folder $here.dir) -eq (ConvertTo-Folder $sub)) -or $deadline.Elapsed.TotalSeconds -gt 30) { break }
      Send-Key $w Escape
      Wait-Change { param($s) -not $s.new_tab.picker } -What 'Esc closing the picker' | Out-Null
    }
    if ($here.group -ne 'here' -or $here.name -ne 'Terminal') { Add-Failure "${what}: the first row is $($here.name) ($($here.group)), not Here: Terminal" }
    if ($here.dir -cne $sub) { Add-Failure "${what}: Here's folder is '$($here.dir)', not '$sub' as Windows spells it" }
    if ($typed) {
      Send-Words $w $typed
      $s = Wait-Change { param($s) $s.new_tab.picker.query -eq $typed } -What """$typed"" in the box"
    }
    $h = @($s.new_tab.picker.rows)[$s.new_tab.picker.highlight]
    if ($h.name -ne 'Terminal') { Add-Failure "${what}: the highlight is $($h.name), not Terminal" }
    $n0 = (Read-Lines $spawned).Count, (Read-Lines $ran).Count, (Read-Lines $argvFile).Count
    $active0 = $s.active_tab
    Send-Key $w Enter
    $t = Wait-Lines $spawned ($n0[0] + 1) "his spawn_tab for $what"
    if ((ConvertTo-Folder $t.cwd) -ne (ConvertTo-Folder $sub)) { Add-Failure "${what}: his tab asked for folder $($t.cwd), not $sub" }
    if (-not (@($t.args) -match '^claude --session-id ')) { Add-Failure "${what}: his tab isn't Claude's: $(@($t.args) -join ' ')" }
    $argv = Wait-Lines $argvFile ($n0[2] + 1) "Claude's arguments for $what"
    $rec = Wait-Lines $ran ($n0[1] + 1) "Conductor's record of $what"
    $tail = @($rec)[2..($rec.Count - 1)]
    if ($tail[0] -ne 'record' -or $tail[1] -ne 'new' -or (ConvertTo-Folder $tail[2]) -ne (ConvertTo-Folder $sub) -or $tail[6] -ne $argv[1]) { Add-Failure "${what}: Conductor's record was $($tail -join ' ')" }
    else { "${what}: Claude in $($t.cwd), Conductor told $($tail[2])" }
    Wait-Start 'Terminal' "the Starting row for $what to end" | Out-Null
    Wait-State { param($s) $s.active_tab -ne $active0 -and -not (@($s.rail.rows) | Where-Object pending) } -What "his tab in front for $what" -TimeoutSec 60 | Out-Null
    Invoke-Cli activate-pane --pane-id $subPane -Window $w -Server:$Mux | Out-Null
  }

  # 3. Shift+click: behind
  $s = & $openPicker 'Warden behind'
  Send-Words $w 'warden'
  $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'warden' -and @($s.new_tab.picker.rows)[0].rect } -What '"warden" in the box'
  $tabs0 = & $tabCount
  $active0 = $s.active_tab
  $rx, $ry = Get-RectCenter @($s.new_tab.picker.rows)[0].rect
  Send-Mouse $w Move $rx $ry
  Send-Mouse $w LDown $rx $ry -Keys L, Shift
  Send-Mouse $w LUp $rx $ry -Keys Shift
  $t = Wait-Lines $spawned 4 'his spawn_tab behind'
  if ($t.activate -ne $false) { Add-Failure "Shift+click asked spawn_tab for activate = $($t.activate), not false" }
  if ((ConvertTo-Folder $t.cwd) -ne (ConvertTo-Folder $dirOf['Warden'])) { Add-Failure "the tab behind asked for folder $($t.cwd)" }
  $s = Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What 'the tab behind' -TimeoutSec 60
  if ($s.active_tab -ne $active0) { Add-Failure "his tab behind moved the window from tab $active0 to $($s.active_tab)" } else { 'behind: the window stayed on its tab' }
  $argv = Wait-Lines $argvFile 4 "Claude's arguments for Warden"
  if ($argv.Count -ne 2 -or $argv[0] -ne '--session-id' -or $argv[1] -notmatch $uuid) { Add-Failure "Claude behind was given $($argv | ConvertTo-Json -Compress)" }
  $rec = Wait-Lines $ran 4 "Conductor's record of the tab behind"
  if (@($rec)[-1] -ne 'picked Warden in the new-tab picker in LBTerm') { Add-Failure "the record behind said '$(@($rec)[-1])'" }
  Wait-Start 'Warden' 'the start behind to end' | Out-Null

  # 4. Ctrl+click: a shell, LBTerm's
  $s = Wait-Settled -What 'the picker after the tab behind'
  if (-not $s.new_tab.picker) { throw 'the picker closed after Shift+click' }
  Send-Words $w 'warden'
  $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'warden' -and @($s.new_tab.picker.rows)[0].rect } -What '"warden" again'
  $tabs0 = & $tabCount
  $n0 = (Read-Lines $spawned).Count, (Read-Lines $ran).Count
  $warden0 = & $inFolder $dirOf['Warden']
  $rx, $ry = Get-RectCenter @($s.new_tab.picker.rows)[0].rect
  Send-Mouse $w Move $rx $ry
  Send-Mouse $w LDown $rx $ry -Keys L, Ctrl
  Send-Mouse $w LUp $rx $ry -Keys Ctrl
  Wait-Start 'Warden' 'the shell in Warden' | Out-Null
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What 'the shell in Warden' -TimeoutSec 60 | Out-Null
  if (-not (Wait-Until { (& $inFolder $dirOf['Warden']) -gt $warden0 } -TimeoutSec 30)) { Add-Failure "the shell didn't open in Warden's folder" }
  if ((Read-Lines $spawned).Count -ne $n0[0] -or (Read-Lines $ran).Count -ne $n0[1]) { Add-Failure 'his handler acted on a shell' } else { 'Ctrl+click: LBTerm''s shell in Warden''s folder, nothing of his run' }

  # 5. Quotes in the first message, through PowerShell to Claude's argv
  $tabs0 = & $tabCount
  Send-Emit 'newtab-lua-quotes' -Window $w
  $argv = Wait-Lines $argvFile 5 "Claude's arguments with quotes"
  "argv: $($argv | ConvertTo-Json -Compress)"
  if ($argv.Count -ne 4 -or $argv[3] -ne 'say "hi" and don''t') { Add-Failure "a first message with quotes reached Claude as $($argv | ConvertTo-Json -Compress)" }
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What 'the quoted tab' -TimeoutSec 60 | Out-Null

  # 6. His Ctrl+Alt+T's action: PowerShell in the open tab's folder (Warden's: step 4's shell came in
  # front, and step 5's tab behind)
  $s = Wait-Settled -What 'the window before his Ctrl+Alt+T'
  $tabs0 = @($s.tabs).Count
  $active0 = $s.active_tab
  $warden0 = & $inFolder $dirOf['Warden']
  $n0 = (Read-Lines $spawned).Count, (Read-Lines $ran).Count
  Send-Emit 'newtab-lua-shell' -Window $w
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 -and $s.active_tab -ne $active0 } -What 'his Ctrl+Alt+T''s tab in front' -TimeoutSec 60 | Out-Null
  if (-not (Wait-Until { (& $inFolder $dirOf['Warden']) -gt $warden0 } -TimeoutSec 30)) { Add-Failure "his Ctrl+Alt+T didn't open in the open tab's folder, Warden's: $((Get-Panes -Window $w -Server:$Mux | ForEach-Object cwd) -join ', ')" }
  else { 'his Ctrl+Alt+T: PowerShell in the open tab''s folder' }
  if ((Read-Lines $spawned).Count -ne $n0[0] -or (Read-Lines $ran).Count -ne $n0[1]) { Add-Failure 'his handler acted on Ctrl+Alt+T' }

  # 7. Reopen. A tab whose pane says a Claude session (the user var claudehook sets), closed with his Ctrl+W's action
  $sid = [guid]::NewGuid().ToString()
  $first = @(Get-Panes -Window $w -Server:$Mux)[0].pane_id
  $claudePane = New-Tab -From $first -Cwd $dirOf['Warden'] -Program (New-ClaudeSessionProgram -Sid $sid) -Window $w -Server:$Mux
  # In front: on a mux domain the server's window goes to the cli's new tab and the GUI's needn't
  Invoke-Cli activate-pane --pane-id $claudePane -Window $w -Server:$Mux | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $_.session -eq $sid }).Count -ge 1 } -What 'the Claude stand-in tab in front, saying its session' -TimeoutSec 60 | Out-Null
  $s = Wait-Settled -What 'the window with the Claude tab in front'
  $tabs0 = @($s.tabs).Count
  Send-Emit 'newtab-lua-close' -Window $w
  $s = Wait-Change { param($s) @($s.tabs).Count -eq $tabs0 - 1 -and @($s.new_tab.closed).Count -ge 1 } -What 'the Claude tab closed and kept'
  $kept = @($s.new_tab.closed)[0]
  "closed: $($kept | ConvertTo-Json -Compress)"
  if (@($kept.sessions)[0] -ne $sid) { Add-Failure "the tab closed was kept with sessions $(@($kept.sessions) -join ', '), not $sid" }
  $tabs0 = @($s.tabs).Count
  $n0 = (Read-Lines $ran).Count
  Send-Emit 'newtab-lua-reopen' -Window $w
  $r = Wait-Lines $ran ($n0 + 1) 'ai open for the Claude tab'
  "ran: $(@($r)[2..($r.Count - 1)] -join ' ')"
  if ($r.Count -ne 4 -or (Split-Path -Leaf $r[1]) -ne 'ai.ts' -or $r[2] -ne 'open' -or $r[3] -ne $sid) { Add-Failure "Reopen ran $($r -join ' ')" }
  $start = Wait-Start $kept.label "the Starting row for '$($kept.label)' to fold (ai never runs here)" 90
  if ($start.ended -ne 'folded') { Add-Failure "the reopen of a Claude tab $($start.ended): his handler should open no tab of its own" }
  $s = Wait-Settled -What 'the window after the fold'
  if (@($s.tabs).Count -ne $tabs0) { Add-Failure "a tab opened for the Claude tab's reopen: $tabs0 tabs before, $(@($s.tabs).Count) after" }
  else { 'Reopen of a Claude tab: ai open with its id, no tab of LBTerm''s, the row folded' }

  # A shell's tab (whichever comes in front once the Claude tab's gone) closed and reopened: LBTerm's
  $s = Wait-Settled -What 'the window before the shell closes'
  $tabs0 = @($s.tabs).Count
  Send-Emit 'newtab-lua-close' -Window $w
  $s = Wait-Change { param($s) @($s.tabs).Count -eq $tabs0 - 1 -and @($s.new_tab.closed).Count -ge 1 } -What 'a shell''s tab closed and kept'
  $kept = @($s.new_tab.closed)[0]
  $n0 = (Read-Lines $ran).Count
  $terminal0 = & $inFolder $dirOf['Terminal']
  Send-Emit 'newtab-lua-reopen' -Window $w
  $start = Wait-Start $kept.label "the shell's tab reopening"
  if ($start.ended -ne 'arrived') { Add-Failure "the shell's reopen $($start.ended)" }
  if (-not (Wait-Until { (& $inFolder $kept.dir) -ge 1 } -TimeoutSec 30)) { Add-Failure "the shell's tab didn't come back in $($kept.dir)" }
  if ((Read-Lines $ran).Count -ne $n0) { Add-Failure 'his handler ran something for a shell''s reopen' } else { "Reopen of a shell's tab: LBTerm's, in $($kept.dir)" }

  # 8. A test window again: his handler steps aside
  Send-Emit 'newtab-lua-test-window' -Window $w
  $s = & $openPicker 'Terminal in a test window'
  Send-Words $w 'terminal'
  $s = Wait-Change { param($s) $s.new_tab.picker.query -eq 'terminal' } -What '"terminal" in the box'
  $tabs0 = @($s.tabs).Count
  $n0 = (Read-Lines $spawned).Count, (Read-Lines $ran).Count, (Read-Lines $argvFile).Count
  $terminal0 = & $inFolder $dirOf['Terminal']
  Send-Key $w Enter
  Wait-Start 'Terminal' 'the pick in a test window' | Out-Null
  Wait-State { param($s) @($s.tabs).Count -gt $tabs0 } -What 'LBTerm''s tab for the pick' -TimeoutSec 60 | Out-Null
  if (-not (Wait-Until { (& $inFolder $dirOf['Terminal']) -gt $terminal0 } -TimeoutSec 30)) { Add-Failure "LBTerm's default didn't open in Terminal's folder" }
  if ((Read-Lines $spawned).Count -ne $n0[0] -or (Read-Lines $ran).Count -ne $n0[1] -or (Read-Lines $argvFile).Count -ne $n0[2]) { Add-Failure 'his handler acted in a test window' }
  else { 'a test window: his handler stepped aside, LBTerm opened PowerShell in Terminal''s folder' }
  if ((Read-Lines $toasts).Count) { Add-Failure "his handlers told Windows: $((Read-Lines $toasts | ForEach-Object { $_.title }) -join '; ')" }
} -Passed 'new-tab-lua: his Ctrl+T, Ctrl+Alt+T and Ctrl+Shift+T are the picker, a shell and Reopen; his handler opens a pick in front with its first message as Claude gets it and Conductor told who asked, behind without moving the window, leaves a shell to LBTerm, reopens a Claude tab with ai open alone and a shell''s through LBTerm, and steps aside in a test window'
