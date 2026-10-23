#Requires -Version 7
<#
The slow-frame log (lbterm-extras/src/slow.rs) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig) with lbterm_slow_ms at its default (33, read from
config/src/lbterm.rs) and F9 bound to an
action_callback that holds the GUI thread for 60 ms, as a slow Lua handler would. Keys are posted
messages, never real input.

The GUI's first key probes the keyboard layout's AltGr and dead keys (KeyboardLayoutInfo in
window.rs): about 10 ms of WM_KEYDOWN on an idle PC, 40 ms and more on a busy one or while the
window is still starting. So a Shift press pays for that while the window settles; its line, when
it has one, is shown. The probe is the GUI thread's, once per layout (lbterm-extras/src/layouts.rs),
and the log names each one it makes: `keyboard layout 0x4090c09 probed in 9.4 ms: ...`.

1. At rest, once the window has settled, nothing is logged under lbterm::slow for 6 s.
2. F9: `slow task wezterm-gui/src/termwindow/mod.rs:<n>: <ms> ms`, <n> the line in
   schedule_window_event that spawns a window event's Lua, <ms> at least 50.
3. F9 three times more, in the same 10 s: nothing more at once, then as those 10 s end, with the
   window at rest, `... : 4 times over 33 ms in the last 10 s, max <ms> ms` (the summary's timer).
4. No other message or task went over: the key's own messages and the event's other tasks are
   charged their own time, not the handler's. Paint lines are shown but not failed on: now and
   then a present waits on DWM for 50 ms or more (perf -Measure). Then at rest again for 6 s,
   nothing.
5. F8 opens a second window of the same GUI, on the test screen (SpawnCommandInNewWindow with a
   position), and a Shift goes to it: its first key takes the first window's probe, so the log
   names one probe of the layout, not two.

-Measure instead takes the window through ordinary things, with every time anything goes over
-Threshold ms logged (LBTERM_SLOW_EVERY, no 10 s windows), and prints what went over in each, by
kind and site, with how often it went over 8, 16, 33 and 50 ms: at rest, the first key (a Shift),
typing (posted keys into pwsh), output scrolling past, a split, the tab rail opening and closing
(posted pointer moves, LBTERM_POSTED_POINTER), a resize, a new tab, a second window (F8) and its
first key, and the layout probes the log names. -Mux puts its tabs in a mux server of their own, as
Lachlan's are (lbterm/MUX.md): the GUI is then that server's client, and what the panes print
reaches it as the client's PDUs.

  pwsh -File lbterm\perf\slow.ps1
  pwsh -File lbterm\perf\slow.ps1 -Measure [-Threshold 1] [-Mux] [-Log <WEZTERM_LOG filter>]
#>
param([string]$Bin, [switch]$Measure, [int]$Threshold = 1, [switch]$Mux, [string]$Log)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\motion\offscreen.ps1')

$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $Bin) { $Bin = Join-Path $repo 'target\release' }
$gui = Join-Path $Bin 'lbterm-gui.exe'
$cliExe = Join-Path $Bin 'lbterm.exe'
$out = Join-Path $repo 'target\perf\slow'
# In a folder of its own, with reloading off (New-TestConfig): LBTerm reloads on any change in
# its config's folder
$cfg = Join-Path $out 'config\slow.lua'
$sock = Join-Path $out 'sock'
New-Item -ItemType Directory -Force $out, (Split-Path $cfg) | Out-Null
$lua = { param($p) $p -replace '\\', '/' }

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class LbtermSlow {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static uint ForegroundPid() { uint p; GetWindowThreadProcessId(GetForegroundWindow(), out p); return p; }
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  // The process's visible top-level windows other than h
  public static IntPtr[] OtherWindows(uint pid, IntPtr h) {
    var found = new System.Collections.Generic.List<IntPtr>();
    EnumWindows((w, l) => { uint p; GetWindowThreadProcessId(w, out p); if (p == pid && w != h && IsWindowVisible(w)) found.Add(w); return true; }, IntPtr.Zero);
    return found.ToArray();
  }
  // A press and release of virtual key vk, with its scan code, as the keyboard would send them
  public static void Key(IntPtr h, int vk) {
    long scan = MapVirtualKey((uint)vk, 0) << 16;
    PostMessage(h, 0x0100, (IntPtr)vk, (IntPtr)(scan | 1));
    PostMessage(h, 0x0101, (IntPtr)vk, unchecked((IntPtr)(int)(0xC0000001 | scan)));
  }
  public static void Move(IntPtr h, int x, int y) {
    PostMessage(h, 0x0200, IntPtr.Zero, (IntPtr)(((y & 0xFFFF) << 16) | (x & 0xFFFF)));
  }
  public static void Leave(IntPtr h) { PostMessage(h, 0x02A3, IntPtr.Zero, IntPtr.Zero); }
}
'@
$VK_F9 = 0x78; $VK_F8 = 0x77; $VK_RETURN = 0x0D; $VK_SHIFT = 0x10
# Where F8's second window opens: beside the first, on the same screen (--position's numbers, from
# the top-left of the whole desktop)
$posX, $posY = $TestWindowPosition -split ',' | ForEach-Object { [int]$_ }

# The line that spawns a window event's Lua: what an action_callback's task is named by
$modRs = Join-Path $repo 'wezterm-gui\src\termwindow\mod.rs'
$src = Get-Content $modRs
$fn = ($src | Select-String -Pattern 'fn schedule_window_event' | Select-Object -First 1).LineNumber
$spawnLine = ($src | Select-Object -Skip $fn | Select-String -Pattern 'promise::spawn::spawn\(config::with_lua_config_on_main_thread' |
  Select-Object -First 1).LineNumber + $fn
$site = "wezterm-gui/src/termwindow/mod.rs:$spawnLine"

# The check runs at lbterm_slow_ms's default; -Measure at -Threshold
$slowLua = if ($Measure) { "config.lbterm_slow_ms = $Threshold" } else { '' }
$default = [int][regex]::Match((Get-Content -Raw (Join-Path $repo 'config\src\lbterm.rs')),
  'fn default_slow_ms\(\) -> u64 \{\s*(\d+)').Groups[1].Value
if (-not $default) { throw "no default_slow_ms in config\src\lbterm.rs" }
New-TestConfig -Path $cfg -Lua @"
$slowLua
config.initial_cols = 120
config.initial_rows = 30
config.keys = config.keys or {}
-- Holds the GUI thread 60 ms, as a slow Lua handler would. MSVC's clock() is wall time
table.insert(config.keys, { key = 'F9', mods = 'NONE', action = wezterm.action_callback(function(window, pane)
  local t = os.clock()
  while os.clock() - t < 0.060 do end
end) })
-- A second window of the same GUI, on the test screen
table.insert(config.keys, { key = 'F8', mods = 'NONE', action = wezterm.action.SpawnCommandInNewWindow {
  args = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' },
  position = { x = $($posX + 60), y = $($posY + 60), origin = 'ScreenCoordinateSystem' },
} })
$(if ($Mux) { @"
config.unix_domains = { {
  name = 'slowmux',
  socket_path = '$(& $lua $sock)',
  serve_command = { '$(& $lua (Join-Path $Bin 'lbterm-mux-server.exe'))', '--config-file', '$(& $lua $cfg)', '--daemonize' },
} }
"@ })
"@
# New-TestConfig's loader has no wezterm in scope of its own
(Get-Content -Raw $cfg) -replace '^local config', "local wezterm = require 'wezterm'`nlocal config" | Set-Content -NoNewline $cfg

$environment = @{}
if ($Log) { $environment.WEZTERM_LOG = $Log }
if ($Measure) { $environment.LBTERM_SLOW_EVERY = '1'; $environment.LBTERM_POSTED_POINTER = '1' }
$launch = if ($Mux) { 'connect slowmux' } else { 'start --always-new-process' }
$procId = Start-Unfocused -Exe $gui -Environment $environment -Arguments "--config-file `"$cfg`" $launch --position $TestWindowPosition --class com.lachlanb.lbterm.perf.slow -- pwsh -NoProfile"
# The test's own mux server, by the config in its command line (as Lua has it, forward slashes)
function Get-Server {
  @(Get-CimInstance Win32_Process -Filter "Name = 'lbterm-mux-server.exe'" |
    Where-Object { ($_.CommandLine -replace '/', '\') -like "*$cfg*" })
}
# With -Mux, cli talks to the server, by its pane ids, as Lachlan's tools do
$cli = @(if ($Mux) { 'cli', '--no-auto-start' } else { 'cli' })
$logPath = "$env:USERPROFILE\.local\share\lbterm\lbterm-gui.exe-log-$procId.txt"
# The lbterm::slow lines logged so far: time, kind, site, ms (a first line) or the whole message
function Get-Slow {
  if (-not (Test-Path $logPath)) { return @() }
  @(Get-Content $logPath | ForEach-Object {
    if ($_ -match '^(\S+)\s+WARN\s+lbterm::slow\s+> (slow (\w+) (\S+): (.+))$') {
      $rest = $Matches[5]
      [pscustomobject]@{
        At = $Matches[1]; Message = $Matches[2]; Kind = $Matches[3]; Site = $Matches[4]
        Ms = if ($rest -match '^([\d.]+) ms$') { [double]$Matches[1] } else { $null }
        Summary = if ($rest -match '^(\d+) times over (\d+) ms in the last 10 s, max ([\d.]+) ms$') {
          [pscustomobject]@{ Count = [int]$Matches[1]; Threshold = [int]$Matches[2]; Max = [double]$Matches[3] }
        } else { $null }
      }
    }
  })
}

# The keyboard layout probes the log names (window.rs, KeyboardLayoutInfo::update)
function Get-Probes {
  if (-not (Test-Path $logPath)) { return @() }
  @(Select-String -Path $logPath -Pattern '> (keyboard layout \S+ probed in [\d.]+ ms.*)$' | ForEach-Object { $_.Matches[0].Groups[1].Value })
}
# F8 in the first window: the second window's handle, once it's there and has had time to start
function Open-SecondWindow {
  [LbtermSlow]::Key($h, $VK_F8)
  $until = (Get-Date).AddSeconds(10)
  do {
    Start-Sleep -Milliseconds 100
    $found = @([LbtermSlow]::OtherWindows([uint32]$procId, $h))
  } while (-not $found -and (Get-Date) -lt $until)
  if (-not $found) { throw 'F8 opened no second window' }
  Start-Sleep 4
  $found[0]
}

$fail = [Collections.Generic.List[string]]::new()
$foreground = [Collections.Generic.HashSet[uint32]]::new()
try {
  $h = Wait-Window -Id $procId -Seconds 20
  if (-not $h -or $h -eq [IntPtr]::Zero) { throw "no window from $gui" }
  [void]$foreground.Add([LbtermSlow]::ForegroundPid())
  $env:WEZTERM_UNIX_SOCKET = if ($Mux) { $sock } else { "$env:USERPROFILE\.local\share\lbterm\gui-sock-$procId" }
  Remove-Item Env:WEZTERM_PANE -ErrorAction SilentlyContinue
  if (-not $Measure) {
    # The first key's layout probe, paid while settling (VK_SHIFT)
    Start-Sleep 1
    [LbtermSlow]::Key($h, 0x10)
  }
  # Settled: started, the shell's first prompt drawn, the strip's notices gone
  Start-Sleep 5
  $first = (@(& $cliExe @cli list --format json | ConvertFrom-Json))[0].pane_id
  $bad = Select-String -Path $logPath -Pattern 'Configuration Error.*' -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($bad) { throw "the test config didn't load: $($bad.Matches[0].Value)" }

  if ($Measure) {
    $s = [LbtermSlow]::GetDpiForWindow($h) / 96.0
    $px = { param($v) [int][Math]::Round($v * $s) }
    $cr = New-Object LbtermSlow+RECT; [void][LbtermSlow]::GetClientRect($h, [ref]$cr)
    $phases = [Collections.Generic.List[object]]::new()
    $phase = { param($name) $phases.Add([pscustomobject]@{ Name = $name; From = (Get-Slow).Count }) }

    & $phase 'startup'
    $phases[0].From = 0
    & $phase 'rest (6 s)'
    Start-Sleep 6
    # A Shift types nothing, but meets the keyboard layout's probe all the same
    & $phase 'first key (Shift)'
    [LbtermSlow]::Key($h, $VK_SHIFT)
    Start-Sleep 1
    & $phase 'typing (60 keys, 80 ms apart)'
    $word = 'echo the quick brown fox jumps over the lazy dog again and again'
    foreach ($c in $word.ToCharArray()[0..59]) {
      $vk = if ($c -eq ' ') { 0x20 } else { [int][char]::ToUpper($c) }
      [LbtermSlow]::Key($h, $vk); Start-Sleep -Milliseconds 80
    }
    [LbtermSlow]::Key($h, $VK_RETURN)
    Start-Sleep 2
    & $phase 'output (2000 lines scrolling past)'
    & $cliExe @cli send-text --pane-id $first --no-paste "1..2000 | ForEach-Object { `"line `$_ of the output scrolling past, long enough to wrap the odd time: `$('x' * (`$_ % 90))`" }`r"
    Start-Sleep 4
    & $phase 'split'
    $second = [int](& $cliExe @cli split-pane --pane-id $first --right -- 'C:/Program Files/PowerShell/7/pwsh.exe' -NoProfile)
    Start-Sleep 3
    & $phase 'tab rail open and close'
    [LbtermSlow]::Move($h, (& $px 24), (& $px 160))
    Start-Sleep 2
    [LbtermSlow]::Move($h, [int]($cr.Right * 0.7), [int]($cr.Bottom / 2))
    Start-Sleep 2
    [LbtermSlow]::Leave($h)
    Start-Sleep 1
    & $phase 'resize (narrower, then back)'
    $wr = New-Object LbtermSlow+RECT; [void][LbtermSlow]::GetWindowRect($h, [ref]$wr)
    $w = $wr.Right - $wr.Left; $hh = $wr.Bottom - $wr.Top
    # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
    [void][LbtermSlow]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, ($w - (& $px 120)), $hh, 0x0002 -bor 0x0004 -bor 0x0010)
    Start-Sleep 2
    [void][LbtermSlow]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, $w, $hh, 0x0002 -bor 0x0004 -bor 0x0010)
    Start-Sleep 2
    & $phase 'new tab'
    & $cliExe @cli spawn --pane-id $second -- 'C:/Program Files/PowerShell/7/pwsh.exe' -NoProfile | Out-Null
    Start-Sleep 3
    & $phase 'second window (F8)'
    $h2 = Open-SecondWindow
    & $phase 'second window, first key (Shift)'
    [LbtermSlow]::Key($h2, $VK_SHIFT)
    Start-Sleep 1
    & $phase 'rest again (6 s)'
    Start-Sleep 6
    $all = Get-Slow
    $phases.Add([pscustomobject]@{ Name = 'end'; From = $all.Count })
    for ($i = 0; $i -lt $phases.Count - 1; $i++) {
      $p = $phases[$i]
      $lines = @($all | Select-Object -Skip $p.From -First ($phases[$i + 1].From - $p.From))
      "== $($p.Name): $($lines.Count) over $Threshold ms"
      $lines | Group-Object Kind, Site | Sort-Object { ($_.Group | Measure-Object Ms -Maximum).Maximum } -Descending | ForEach-Object {
        $ms = @($_.Group.Ms)
        $stat = $ms | Measure-Object -Maximum -Average
        $over = foreach ($t in 8, 16, 33, 50) { "$(@($ms | Where-Object { $_ -gt $t }).Count) >$t" }
        '  {0,-58} {1,4}x  avg {2,5:N1}  max {3,6:N1} ms   {4}' -f $_.Name, $ms.Count, $stat.Average, $stat.Maximum, ($over -join ', ')
      }
    }
    $probes = Get-Probes
    "== keyboard layout probes: $($probes.Count)"
    $probes | ForEach-Object { "  $_" }
    return
  }

  # What starting and the first key logged
  Get-Slow | ForEach-Object { "settling: $($_.Message)" }

  # 1. At rest: nothing
  $mark = (Get-Slow).Count
  Start-Sleep 6
  $rest = @(Get-Slow | Select-Object -Skip $mark)
  if ($rest) { $fail.Add("logged at rest: $($rest.Message -join ' | ')") }

  # 2. F9: the task that held the thread, by where it was spawned
  $mark = (Get-Slow).Count
  [LbtermSlow]::Key($h, $VK_F9)
  Start-Sleep -Milliseconds 800
  $now = @(Get-Slow | Select-Object -Skip $mark)
  $now.Message
  $held = @($now | Where-Object { $_.Kind -eq 'task' -and $_.Site -eq $site -and $_.Ms })
  if (-not $held) { $fail.Add("no 'slow task ${site}: <ms> ms' after F9") }
  elseif ($held[0].Ms -lt 50) { $fail.Add("F9 held the thread 60 ms, but the line says $($held[0].Ms) ms") }

  # 3. Three more in the same 10 s: counted, then summed up when they end
  foreach ($i in 1..3) { [LbtermSlow]::Key($h, $VK_F9); Start-Sleep -Milliseconds 400 }
  Start-Sleep 1
  $soon = @(Get-Slow | Select-Object -Skip $mark | Where-Object Site -eq $site)
  if ($soon.Count -ne 1) { $fail.Add("$($soon.Count) lines for $site within its 10 s, not 1: $($soon.Message -join ' | ')") }
  Start-Sleep 9
  $after = @(Get-Slow | Select-Object -Skip $mark)
  $after | Select-Object -Skip $now.Count | ForEach-Object Message
  $sum = @($after | Where-Object { $_.Site -eq $site -and $_.Summary })
  if (-not $sum) { $fail.Add("no summary for $site as its 10 s ended") }
  else {
    $want = "4 times over $default ms"
    if ($sum[0].Summary.Count -ne 4 -or $sum[0].Summary.Threshold -ne $default) { $fail.Add("the summary says '$($sum[0].Message)', wanted $want") }
    if ($sum[0].Summary.Max -lt 50) { $fail.Add("the summary's max is $($sum[0].Summary.Max) ms") }
  }

  # 4. No other message or task went over (a present can wait on DWM: shown, not failed on), and
  # at rest again nothing more
  $others = @($after | Where-Object { $_.Site -ne $site -and $_.Kind -ne 'paint' })
  if ($others) { $fail.Add("other lines: $($others.Message -join ' | ')") }
  $after | Where-Object Kind -eq 'paint' | ForEach-Object { "noted: $($_.Message)" }
  $mark = (Get-Slow).Count
  Start-Sleep 6
  $rest = @(Get-Slow | Select-Object -Skip $mark)
  if ($rest) { $fail.Add("logged at rest after: $($rest.Message -join ' | ')") }

  # 5. A second window's first key: the first window's probe, not one of its own
  $h2 = Open-SecondWindow
  [LbtermSlow]::Key($h2, $VK_SHIFT)
  Start-Sleep 1
  $probes = Get-Probes
  $probes | ForEach-Object { "probe: $_" }
  if ($probes.Count -ne 1) { $fail.Add("$($probes.Count) probes of the keyboard layout after two windows' first keys, not 1") }

  [void]$foreground.Add([LbtermSlow]::ForegroundPid())
  if ($foreground.Contains([uint32]$procId)) { $fail.Add('the test window took the foreground') }
} finally {
  Stop-Process -Id $procId -ErrorAction SilentlyContinue
  foreach ($s in Get-Server) { Stop-Process -Id $s.ProcessId -Force -ErrorAction SilentlyContinue }
}
if ($fail.Count) { $fail | ForEach-Object { "FAIL $_" }; exit 1 }
"slow-frame log passed: $site named, summed up after 10 s, nothing at rest, one keyboard layout probe for two windows (log: $logPath)"
