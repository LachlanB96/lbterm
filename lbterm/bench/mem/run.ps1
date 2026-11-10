# One measured window: opens a test window of the copied build with a config, then at each step
# (rest with one tab, more tabs, lines printed) takes the GUI's (and a mux server's) memory
# breakdown. Writes <Out>\report.json. Run as its own pwsh: Invoke-Test exits.
param(
  [Parameter(Mandatory)][string]$Name,
  [Parameter(Mandatory)][string]$Bin,
  [Parameter(Mandatory)][string]$OutRoot,
  [ValidateSet('bench', 'lachlan', 'none')][string]$Config = 'bench',
  [switch]$Mux,
  # More config lines (Lua): `name = value,` entries for bench, `config.x = y` for lachlan
  [string]$Lua = '',
  # Steps after the first rest: tabs:<n total>, lines:<n>[:plain|varied|wide], rest:<seconds>, close:<tabs>
  [string[]]$Steps = @(),
  [int]$Scrollback = 9001,
  [int]$SettleSec = 8,
  [switch]$Scan,
  [string]$Log = ''
)
$ErrorActionPreference = 'Stop'
$Steps = @($Steps | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Lua = ($Lua -split '[|]') -join "`n"
Import-Module (Join-Path $PSScriptRoot '..\..\harness.psm1') -Force -DisableNameChecking
. (Join-Path $PSScriptRoot 'memtools.ps1')

$out = Join-Path $OutRoot $Name
$T = Start-Test "mem-$Name" -Bin $Bin -Out $out
$pwshExe = 'C:/Program Files/PowerShell/7/pwsh.exe'
$report = [ordered]@{ name = $Name; config = $Config; mux = [bool]$Mux; lua = $Lua; steps = @(); started = (Get-Date).ToString('o'); load = @() }

function Get-Load {
  # What else was busy: lbterm-gui, cargo, rustc and the busiest processes by CPU over a second
  $a = Get-Process | Select-Object Id, ProcessName, @{ n = 'Cpu'; e = { $_.TotalProcessorTime.TotalMilliseconds } }
  Start-Sleep -Milliseconds 1000
  $b = @{}; Get-Process | ForEach-Object { $b[$_.Id] = $_ }
  $busy = $a | ForEach-Object {
    $p = $b[$_.Id]; if ($p) { [pscustomobject]@{ Name = "$($_.ProcessName) $($_.Id)"; Cores = [Math]::Round(($p.TotalProcessorTime.TotalMilliseconds - $_.Cpu) / 1000, 2) } }
  } | Where-Object Cores -gt 0.2 | Sort-Object Cores -Descending
  $cores = [Math]::Round((@($busy) | Measure-Object Cores -Sum).Sum, 2)
  $counts = @{}
  foreach ($n in 'lbterm-gui', 'lbterm-mux-server', 'cargo', 'rustc', 'link', 'rust-lld') { $counts[$n] = @(Get-Process -Name $n -ErrorAction SilentlyContinue).Count }
  "$cores cores busy; lbterm-gui $($counts['lbterm-gui']), mux servers $($counts['lbterm-mux-server']), cargo $($counts.cargo), rustc $($counts.rustc); top: $((@($busy) | Select-Object -First 5 | ForEach-Object { "$($_.Name) $($_.Cores)" }) -join ', ')"
}

function New-Config([string]$Path) {
  New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
  $sock = ConvertTo-LuaPath $T.Sock
  $srv = ConvertTo-LuaPath $T.MuxServer
  $cfg = ConvertTo-LuaPath $Path
  if ($Config -eq 'bench') {
    $lines = @(
      "local wezterm = require 'wezterm'",
      'return {',
      "  font = wezterm.font('Cascadia Mono'),",
      '  font_size = 12.0,',
      '  initial_cols = 100,',
      '  initial_rows = 30,',
      "  scrollback_lines = $Scrollback,",
      "  front_end = 'WebGpu',",
      '  check_for_updates = false,',
      '  automatically_reload_config = false,',
      '  wsl_domains = {},',
      "  window_close_confirmation = 'NeverPrompt',",
      "  default_prog = { '$pwshExe', '-NoProfile' },",
      '  lbterm_crash_restore = false,'
    )
    foreach ($l in $Lua -split "`n") { if ($l.Trim()) { $lines += "  $($l.Trim())" } }
    if ($Mux) { $lines += "  unix_domains = { { name = 'memmux', socket_path = '$sock', serve_command = { '$srv', '--config-file', '$cfg', '--daemonize' } } }," }
    else { $lines += '  unix_domains = {},' }
    $lines += '}'
    [IO.File]::WriteAllText($Path, ($lines -join "`n") + "`n")
  } elseif ($Config -eq 'lachlan') {
    $extra = "config.scrollback_lines = $Scrollback`nconfig.default_prog = { '$pwshExe', '-NoProfile' }`n$Lua"
    if ($Mux) { $extra += "`nconfig.unix_domains = { { name = 'memmux', socket_path = '$sock', serve_command = { '$srv', '--config-file', '$cfg', '--daemonize' } } }" }
    New-TestConfig -Path $Path -Lua $extra
  }
  $Path
}

# A tab's program printing $N lines then setting its title to 'fill done'
function New-FillProgram([int]$N, [string]$Kind) {
  New-PwshProgram -Script (@'
$e = [char]27
$w = [Console]::WindowWidth - 1
$r = [Random]::new(7)
$words = -split 'cache line pane tab shape glyph quad frame window rail strip fade build check merge branch commit test slot warden summary session prompt output render layout switch hop surface visual config default memory thread GUI mux server socket state file JSON lbterm term cell row column font colour cursor scroll text error warning passed failed ok the a to of and in is it for on that with'
$kind = '__KIND__'
$b = [Text.StringBuilder]::new()
for ($i = 0; $i -lt __LINES__; $i++) {
  if ($kind -eq 'plain') {
    [void]$b.Append(('{0:d7} ' -f $i) + 'The quick brown fox jumps over the lazy dog 0123456789 abcdefghijklmnopqrstu')
  } elseif ($kind -eq 'wide') {
    for ($c = 0; $c -lt $w; $c++) { if ($c % 8 -eq 0) { [void]$b.Append("$e[3$($r.Next(1, 8))m") }; [void]$b.Append([char](33 + $r.Next(90))) }
    [void]$b.Append("$e[0m")
  } else {
    $want = $r.Next(8, $w); $n = 0
    while ($true) {
      $k = $r.Next(10)
      $t = if ($k -lt 6) { $words[$r.Next($words.Count)] } elseif ($k -lt 8) { [string]$r.Next(0, 100000) } else { '{0:x8}' -f $r.Next() }
      if ($n + $t.Length + 1 -gt $want) { break }
      if ($r.Next(5) -eq 0) { [void]$b.Append("$e[3$($r.Next(1, 8))m$t$e[0m ") } else { [void]$b.Append("$t ") }
      $n += $t.Length + 1
    }
  }
  [void]$b.Append("`n")
  if ($i % 200 -eq 199) { [Console]::Out.Write($b.ToString()); [void]$b.Clear() }
}
[Console]::Out.Write($b.ToString() + "$e]2;fill done$([char]7)")
Start-Sleep 3600
'@ -replace '__LINES__', $N -replace '__KIND__', $Kind)
}

function Get-ConptyPids([int[]]$Parents) {
  @(Get-CimInstance Win32_Process -Filter "Name = 'OpenConsole.exe' OR Name = 'conhost.exe'" | Where-Object { $_.ParentProcessId -in $Parents } | ForEach-Object { [int]$_.ProcessId })
}

function Measure-Step([string]$Label, $w) {
  # Wait for it to be quiet: no frame for 2 s, up to 30 s
  $last = -1; $since = [Diagnostics.Stopwatch]::StartNew(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt 30) {
    $s = Get-StateSnapshot $w.StateFile
    $f = if ($s) { [long]$s.frame } else { -1 }
    if ($f -ne $last) { $last = $f; $since.Restart() }
    if ($since.Elapsed.TotalSeconds -ge 2) { break }
    Start-Sleep -Milliseconds 100
  }
  Start-Sleep -Seconds 1
  $servers = @(Get-TestServers | ForEach-Object { [int]$_.ProcessId })
  $gui = Get-MemReport -ProcessId $w.Pid -Scan:$Scan
  $srv = @($servers | ForEach-Object { Get-MemReport -ProcessId $_ -Scan:$Scan })
  $conpty = Get-ConptyPids (@($w.Pid) + $servers)
  $cp = @($conpty | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
  $tabs = $null; $st = Get-StateSnapshot $w.StateFile
  if ($st) { $sw = Get-StateWindow $st -Hwnd ([long]$w.Handle); if ($sw) { $tabs = @($sw.tabs).Count } }
  $step = [ordered]@{
    label = $Label; at = (Get-Date).ToString('HH:mm:ss'); tabs = $tabs; frame = $last
    gui = $gui; servers = $srv
    conpty = [ordered]@{ count = $cp.Count; privateMB = [Math]::Round((($cp | Measure-Object PrivateMemorySize64 -Sum).Sum) / 1MB, 1); workingSetMB = [Math]::Round((($cp | Measure-Object WorkingSet64 -Sum).Sum) / 1MB, 1) }
  }
  $script:report.steps += $step
  $srvLine = if ($srv) { "; server $($srv[0].PrivateUsageMB) MB private, $($srv[0].WorkingSetMB) WS" } else { '' }
  "[$Label] tabs $tabs; GUI $(Format-MemLine $gui)$srvLine; conpty x$($cp.Count) $($step.conpty.privateMB) MB"
}

$cfgPath = if ($Config -ne 'none') { New-Config (Join-Path $out "config\mem.lua") }
$report.loadBefore = Get-Load

Invoke-Test {
  $open = @{ TimeoutSec = 30 }; if ($Log) { $open.Log = $Log }
  if ($Config -eq 'none') { $open.NoConfig = $true; $open.Program = @($pwshExe, '-NoProfile') } else { $open.ConfigFile = $cfgPath }
  if ($Mux) { $open.Domain = 'memmux'; $open.Socket = $T.Sock } elseif ($Config -ne 'none') { $open.Program = @($pwshExe, '-NoProfile') }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What 'the first frame' -TimeoutSec 30 | Out-Null
  $first = $null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $first -and $clock.Elapsed.TotalSeconds -lt 20) {
    $panes = @(Get-Panes -Server:$Mux)
    if ($panes) { $first = $panes[0].pane_id } else { Start-Sleep -Milliseconds 200 }
  }
  if ($null -eq $first) { throw 'no pane found' }
  Start-Sleep -Seconds $SettleSec
  Measure-Step 'start' $w
  foreach ($step in $Steps) {
    $p = $step -split ':'
    switch ($p[0]) {
      'tabs' {
        $want = [int]$p[1]
        $have = @(Get-Tabs -Server:$Mux).Count
        for ($i = $have; $i -lt $want; $i++) { New-Tab -From $first -Program @($pwshExe, '-NoProfile') -Server:$Mux | Out-Null }
        Wait-State { param($s) @($s.tabs).Count -ge $want } -What "$want tabs" -TimeoutSec 60 | Out-Null
        Start-Sleep -Seconds $SettleSec
        Measure-Step "tabs$want" $w
      }
      'lines' {
        $n = [int]$p[1]; $kind = if ($p.Count -gt 2) { $p[2] } else { 'plain' }
        $pane = New-Tab -From $first -Program (New-FillProgram $n $kind) -Server:$Mux
        if ($Mux) {
          Invoke-Cli activate-pane --pane-id $pane -Server | Out-Null
          $ok = Wait-Until { @(Get-Panes -Server | Where-Object { $_.pane_id -eq $pane -and $_.title -eq 'fill done' }).Count -gt 0 } -TimeoutSec 600 -IntervalMs 1000
          if (-not $ok) { throw "the fill tab never said it was done" }
        } else {
          Wait-State { param($s) @($s.panes | Where-Object { $_.title -eq 'fill done' }).Count -gt 0 } -What "$n lines" -TimeoutSec 600 | Out-Null
        }
        Start-Sleep -Seconds $SettleSec
        Measure-Step "lines$n-$kind" $w
        $script:lastFill = $pane
      }
      'closefill' {
        Invoke-Cli kill-pane --pane-id $script:lastFill -Server:$Mux -Check | Out-Null
        Start-Sleep -Seconds $SettleSec
        Measure-Step 'closefill' $w
      }
      'rest' { Start-Sleep -Seconds ([int]$p[1]); Measure-Step "rest$($p[1])" $w }
    }
  }
  $report.loadAfter = Get-Load
  $report.log = $w.Log
} -Finally {
  [IO.File]::WriteAllText((Join-Path $out 'report.json'), ($report | ConvertTo-Json -Depth 8))
} -Passed "mem-$($Name): measured"
