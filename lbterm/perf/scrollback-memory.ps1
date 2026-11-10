#Requires -Version 7
# Covers: wezterm-gui/src/quad.rs, wezterm-surface/src/line/*.rs, wezterm-cell/src/lib.rs, term/src/screen.rs, term/src/terminalstate/performer.rs
<#
What long output leaves the GUI holding (lbterm/PERF.md, "What printing costs"), held by its
private bytes: a test window with a config like lbterm/bench's (WebGpu, 110 by 30, a scrollback of
9,001 lines, none of Lachlan's), its own tab writing 100,032 lines of 80 columns at a time, eight
times over, its private bytes read before the first and after each once the window is quiet. The
scrollback holds 9,001 lines whatever is printed; what grows past it is the GUI's caches of what
it drew (each line's quads, its shapes), which fill to their size (line_quad_cache_size and the
rest, 1,024 lines each) once enough frames have drawn enough lines. The installed build before
these fixes grew 64 to 70 MB here, upstream WezTerm 35 (lbterm/BENCH.md), this build 39: a cached
line's quads were 2.6 times upstream's size (BoxedQuad's corners and gradient inline). Fails
when the growth passes -MaxGrowthMb; says when the window drew too few lines to fill its caches,
so the bound held less than it should.

  pwsh -File lbterm\perf\scrollback-memory.ps1 [-Bin <build>] [-Writes 8] [-MaxGrowthMb 50]
#>
param([string]$Bin, [int]$Writes = 8, [double]$MaxGrowthMb = 50)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'scrollback-memory' -Bin $Bin

$rows = [Collections.Generic.List[object]]::new()
Invoke-Test {
  # lbterm/bench's scrollback lines: 80 printable columns, a pattern 95 lines long
  $data = Join-Path $T.Out 'lines.bin'
  $sb = [Text.StringBuilder]::new()
  for ($line = 0; $line -lt 12504; $line++) {
    for ($i = 0; $i -lt 80; $i++) { [void]$sb.Append([char](32 + (($line * 7 + $i * 3) % 95))) }
    [void]$sb.Append("`r`n")
  }
  [IO.File]::WriteAllBytes($data, [Text.Encoding]::UTF8.GetBytes($sb.ToString()))
  $writer = @"
`$chunk = [IO.File]::ReadAllBytes('$data')
`$out = [Console]::OpenStandardOutput()
`$e = [char]27
function Title([string]`$t) { `$b = [Text.Encoding]::UTF8.GetBytes("`$e]0;`$t`$([char]7)"); `$out.Write(`$b, 0, `$b.Length); `$out.Flush() }
Title 'starting'; Start-Sleep -Milliseconds 200; Title 'ready'
`$n = 0
while (`$true) { [void][Console]::ReadKey(`$true); `$n++; for (`$k = 0; `$k -lt 8; `$k++) { `$out.Write(`$chunk, 0, `$chunk.Length) }; `$out.Flush(); Title "wrote `$n" }
"@
  $cfg = Join-Path $T.Out 'config\scrollback-memory.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  [IO.File]::WriteAllText($cfg, @'
local wezterm = require 'wezterm'
local config = {}
config.automatically_reload_config = false
config.front_end = 'WebGpu'
config.lbterm_crash_restore = false
config.wsl_domains = {}
config.check_for_updates = false
config.initial_cols = 110
config.initial_rows = 30
config.scrollback_lines = 9001
config.window_close_confirmation = 'NeverPrompt'
return config
'@)
  $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram $writer)
  Wait-State { param($s) @($s.panes).Count -ge 1 -and @($s.panes)[0].title -eq 'ready' } -What 'the writer ready' -TimeoutSec 30 | Out-Null
  Get-QuietFrame -QuietMs 1000 -TimeoutSec 20 -Window $w | Out-Null
  Send-Focus $w
  Start-Sleep -Seconds 1
  $p0 = (Get-Process -Id $w.Pid).PrivateMemorySize64
  $f0 = [long](Read-State).frame
  foreach ($i in 1..$Writes) {
    Send-Key $w G
    Wait-State { param($s) @($s.panes)[0].title -eq "wrote $i" } -What "the writer's write $i" -TimeoutSec 120 | Out-Null
    Get-QuietFrame -QuietMs 1000 -TimeoutSec 30 -Window $w | Out-Null
    Start-Sleep -Seconds 1
    $p = (Get-Process -Id $w.Pid).PrivateMemorySize64
    $rows.Add([pscustomobject]@{ write = $i; lines = $i * 100032; private_mb = [math]::Round($p / 1MB, 1); grown_mb = [math]::Round(($p - $p0) / 1MB, 1); frames = [long](Read-State).frame - $f0 })
    Write-Host ("write {0}: {1} MB private, {2} MB grown, {3} frames" -f $i, $rows[-1].private_mb, $rows[-1].grown_mb, $rows[-1].frames)
  }
  [IO.File]::WriteAllText((Join-Path $T.Out 'memory.json'), (@{ before_mb = [math]::Round($p0 / 1MB, 1); writes = $rows } | ConvertTo-Json -Depth 4))
  $last = $rows[-1]
  if ($last.grown_mb -gt $MaxGrowthMb) {
    Add-Failure "after $($last.lines) lines the GUI's private bytes grew $($last.grown_mb) MB, over $MaxGrowthMb MB: what the line caches hold a line has grown (BoxedQuad's size, wezterm-gui/src/quad.rs) or something keeps every line it saw"
  }
  # 30 rows a frame: the caches (1,024 lines) are full once about 35 frames have drawn new lines
  $filled = $last.frames -ge 35
  (Get-Test).PassedLine = "after $($last.lines) lines the GUI grew $($last.grown_mb) MB (at most $MaxGrowthMb) over $($last.frames) frames$(if (-not $filled) { ', too few to fill its caches, so the bound held less than it should' })"
} -Passed 'scrollback memory within its bound'
