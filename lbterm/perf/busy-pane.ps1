#Requires -Version 7
# Covers: mux/src/localpane.rs, mux/src/lib.rs, mux/src/lbterm_output.rs, wezterm-mux-server-impl/src/dispatch.rs, wezterm-mux-server-impl/src/lbterm_flow.rs, wezterm-mux-server-impl/src/sessionhandler.rs, wezterm-client/src/client.rs, wezterm-client/src/lbterm_backlog.rs, term/src/terminalstate/performer.rs, term/src/terminal.rs, vtparse/src/lib.rs, wezterm-escape-parser/src/parser/mod.rs
# Variant: -Local
<#
A pane printing as fast as it can doesn't hold up the others (lbterm/PERF.md, finding 91): a test
window on a mux domain of its own (-Local: the window's own tab), a pwsh prompt and beside it a
writer at full speed (lines of 104 bytes, 64 KB a write, no pause, the budget runner's
key-echo-load's), then -Keys keys typed at the prompt one at a time, each timed to its echo in the
GUI (the state file's cursor), and on a mux domain `lbterm cli get-text` of the prompt's pane, which
the server answers from its main thread, timed beside the writer.

Once the parser read a run of ASCII in a few ns a byte (2026-10-31, perf/fonts-core's printing
stage), a mux server's main thread waited on the pane's lock behind its parser and queued a push of
the pane's changes for every batch of its output: the prompt's echo never showed and the cli went
unanswered for minutes. A mux server's connection now owes each client one push a pane, worked out
when the client can take it (perf/mux-server, lbterm/MUX.md, "A window that can't keep up holds the
server to what it takes").

It fails when an echo takes over -EchoMs, a get-text over -CliMs, or never comes.

  pwsh -File lbterm\perf\busy-pane.ps1 [-Local] [-Keys 5] [-Bin <build>]
#>
param([string]$Bin, [switch]$Local, [int]$Keys = 5, [int]$EchoMs = 2000, [int]$CliMs = 3000)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('busy-pane' + $(if ($Local) { '-local' } else { '' })) -Bin $Bin
$mux = -not $Local

$load = @'
$line = ' the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk'
$sb = [Text.StringBuilder]::new()
for ($i = 0; $i -lt 630; $i++) { [void]$sb.Append($i.ToString().PadLeft(6)).Append($line).Append("`r`n") }
$chunk = [Text.Encoding]::UTF8.GetBytes($sb.ToString())
$out = [Console]::OpenStandardOutput()
while ($true) { $out.Write($chunk, 0, $chunk.Length) }
'@

# `lbterm cli` with a time limit: a server that doesn't answer is killed off, not waited on for good.
# Its output, and how long it took (or $null when it didn't end in time)
function Invoke-BoundedCli([string[]]$Arguments, [int]$TimeoutMs) {
  $psi = [Diagnostics.ProcessStartInfo]::new($T.Cli)
  foreach ($a in @('cli', '--no-auto-start') + $Arguments) { $psi.ArgumentList.Add($a) }
  $psi.Environment['WEZTERM_UNIX_SOCKET'] = $(if ($mux) { (Resolve-Window $null).ServerSocket } else { (Resolve-Window $null).Socket })
  [void]$psi.Environment.Remove('WEZTERM_PANE')
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $p = [Diagnostics.Process]::Start($psi)
  $read = $p.StandardOutput.ReadToEndAsync()
  if (-not $p.WaitForExit($TimeoutMs)) {
    try { $p.Kill() } catch { }
    return [pscustomobject]@{ Ms = $null; Text = '' }
  }
  [pscustomobject]@{ Ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 1); Text = $read.Result }
}

function Get-Prompt($Window) { @((Get-StateWindow (Read-State) -Hwnd ([long]$Window.Handle)).panes | Where-Object active)[0] }

Invoke-Test {
  $cfg = Join-Path $T.Out 'config\busy-pane.lua'
  New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
  $domain = if ($mux) {
    @"
config.unix_domains = { {
  name = 'busypane',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
  } else { '' }
  New-TestConfig -Path $cfg -Lua @"
config.window_close_confirmation = 'NeverPrompt'
config.cursor_blink_rate = 0
config.default_cursor_style = 'SteadyBlock'
$domain
"@
  $open = @{ ConfigFile = $cfg; Program = (New-PwshProgram); TimeoutSec = 20 }
  if ($mux) { $open.Domain = 'busypane'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  if ($mux -and -not (Wait-Until { @(Get-Panes -Server).Count -ge 1 } -TimeoutSec 15 -IntervalMs 250)) { throw "the test mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window''s first frame' | Out-Null
  $prompt = (Get-Panes -Server:$mux)[0].pane_id
  Invoke-Cli send-text --pane-id $prompt --no-paste "function prompt { 'PS> ' }`r" -Server:$mux -Check | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object active)[0].cursor.x -eq 4 } -What 'the short prompt' -TimeoutSec 15 | Out-Null
  $got = (Invoke-Cli split-pane --pane-id $prompt --right -Program (New-PwshProgram $load) -Server:$mux -Merge) -join ' '
  if ($got.Trim() -notmatch '^\d+$') { throw "split-pane printed: $got" }
  Invoke-Cli activate-pane --pane-id $prompt -Server:$mux -Check | Out-Null
  Wait-State { param($s) @($s.panes).Count -eq 2 -and @($s.panes | Where-Object active)[0].cursor.x -eq 4 } -What 'the writer beside the prompt' -TimeoutSec 20 | Out-Null
  Send-Focus $w
  # The writer at full speed
  Start-Sleep -Seconds 3

  $echoes = [Collections.Generic.List[object]]::new()
  for ($k = 1; $k -le $Keys; $k++) {
    $c0 = (Get-Prompt $w).cursor
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Send-Key $w X
    $ms = $null
    while ($clock.ElapsedMilliseconds -lt [Math]::Max(10000, $EchoMs * 3)) {
      $c = (Get-Prompt $w).cursor
      if ($c -and ($c.x -ne $c0.x -or $c.y -ne $c0.y)) { $ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 1); break }
      Start-Sleep -Milliseconds 10
    }
    $echoes.Add($ms)
    if ($null -eq $ms) { Add-Failure "key $k's echo never showed in $([Math]::Max(10000, $EchoMs * 3)) ms beside the writer"; break }
    if ($ms -gt $EchoMs) { Add-Failure "key $k's echo took $ms ms beside the writer, over $EchoMs" }
    Start-Sleep -Milliseconds 300
  }
  $cli = $null
  if ($mux) {
    $r = Invoke-BoundedCli @('get-text', '--pane-id', "$prompt") ([Math]::Max(15000, $CliMs * 3))
    $cli = $r.Ms
    if ($null -eq $cli) { Add-Failure "the server never answered cli get-text beside the writer" }
    elseif ($cli -gt $CliMs) { Add-Failure "the server took $cli ms to answer cli get-text beside the writer, over $CliMs" }
    elseif ($r.Text -notmatch 'PS> X') { Add-Failure "cli get-text of the prompt's pane didn't have the keys typed: $($r.Text.Trim() -replace '\s+', ' ')" }
  }
  "beside a writer at full speed$(if ($mux) { ', on a mux domain' }): echoes in $(($echoes | ForEach-Object { if ($null -eq $_) { 'never' } else { "$_ ms" } }) -join ', ')$(if ($mux) { "; cli get-text answered in $(if ($null -eq $cli) { 'never' } else { "$cli ms" })" })"
  [IO.File]::WriteAllText((Join-Path $T.Out 'busy.json'), ([pscustomobject]@{ mux = $mux; echoes_ms = @($echoes); cli_ms = $cli } | ConvertTo-Json))
} -Passed "busy-pane$(if ($Local) { ' -Local' }): a pane printing at full speed holds up neither another pane's echo nor the cli"
