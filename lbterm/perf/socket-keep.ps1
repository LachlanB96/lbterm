#Requires -Version 7
# Covers: wezterm-mux-server-impl/src/local.rs, wezterm-mux-server/src/main.rs, mux/src/lbterm_serve.rs
<#
A mux server keeps its socket (lbterm/MUX.md, "The server keeps its socket"). On 2026-10-29 a
test's mux server whose config failed to load fell back to the default domain and took Lachlan's
daily socket by removing the file there; when it ended with its test, nothing new could reach his
tabs. In a test window of its own class, on a domain and socket of its own:

1. The server's socket is taken the way an older server takes one: the file removed and another
   socket bound there, still listening. The server must bind its own again within a few seconds
   (it looks every 3 s), and the cli reach it.
2. The file is removed outright: bound again the same way.
3. A second server started on the live path must refuse, exit, and leave the socket as it was
   (it finds the first's claim on the socket, mux/src/lbterm_serve.rs, before it looks at the file).

What a server whose config file fails to load does (it stops rather than start on the default
domain) isn't here: against a build without that, the server would take the daily socket.

  pwsh -File lbterm\perf\socket-keep.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'socket-keep' -Bin $Bin
$cfg = Join-Path $T.Out 'config\keep.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
New-TestConfig -Path $cfg -Lua @"
config.unix_domains = { {
  name = 'keepmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
$idle = New-PwshProgram -Script 'Start-Sleep 600'
function Test-Answers { try { @(Invoke-Cli list --format json -Json -Server).Count -gt 0 } catch { $false } }

Invoke-Test {
  Open-TestWindow -ConfigFile $cfg -Domain keepmux -Socket $T.Sock -Program $idle | Out-Null
  Wait-State { param($s) @($s.panes | Where-Object { $null -ne $_.server_id }).Count -eq 1 } -What 'the first tab, on the server' -TimeoutSec 30 | Out-Null
  if (-not (Test-Answers)) { throw 'the server never answered on its socket' }

  # 1. Taken and held, as an older server does it
  $thief = [Net.Sockets.Socket]::new([Net.Sockets.AddressFamily]::Unix, [Net.Sockets.SocketType]::Stream, [Net.Sockets.ProtocolType]::Unspecified)
  Remove-Item -LiteralPath $T.Sock
  $thief.Bind([Net.Sockets.UnixDomainSocketEndPoint]::new($T.Sock))
  $thief.Listen(4)
  $taken = Get-SocketStamp $T.Sock
  $clock = [Diagnostics.Stopwatch]::StartNew()
  # Not the cli while the thief has it: it would wait on a socket that never answers
  while ((Get-SocketStamp $T.Sock) -eq $taken -and $clock.ElapsedMilliseconds -lt 10000) { Start-Sleep -Milliseconds 100 }
  $back = $clock.ElapsedMilliseconds
  if ((Get-SocketStamp $T.Sock) -ne $taken -and (Test-Answers)) { "taken and held: the server bound its own again after $back ms" }
  else { Add-Failure 'the server never took its socket back from another listener' }
  $thief.Dispose()

  # 2. Removed (.NET removes the file a socket it bound had, as it goes: the server's, by now)
  if (Test-Path -LiteralPath $T.Sock) { Remove-Item -LiteralPath $T.Sock }
  $clock.Restart()
  while (-not (Test-Answers) -and $clock.ElapsedMilliseconds -lt 10000) { Start-Sleep -Milliseconds 250 }
  if (Test-Answers) { "removed: bound again after $($clock.ElapsedMilliseconds) ms" } else { Add-Failure 'the server never bound its removed socket again' }

  # 3. A second server on the live path
  $before = Get-SocketStamp $T.Sock
  $err = Join-Path $T.Out 'second-err.txt'
  $second = Start-Process -FilePath $T.MuxServer -ArgumentList '--config-file', $cfg -PassThru -WindowStyle Hidden -RedirectStandardError $err
  if (-not $second.WaitForExit(15000)) { $second.Kill(); Add-Failure 'a second server on a live socket kept running' }
  elseif ((Get-Content $err -Raw) -notmatch 'not taking its socket') { Add-Failure "a second server ended without saying it left the socket alone: $(Get-Content $err -Raw)" }
  else { 'a second server refused the live socket' }
  if ((Get-SocketStamp $T.Sock) -ne $before) { Add-Failure 'a second server changed the live socket' }
  if (-not (Test-Answers)) { Add-Failure 'the first server stopped answering after a second one tried its socket' }
} -Passed 'socket-keep: a taken or removed socket is bound again within seconds, and a second server leaves a live one alone'
