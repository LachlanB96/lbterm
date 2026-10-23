# Pester 5+, run by `lbterm.ps1 check`. The config `lbterm.ps1 try` writes, loaded by this tree's
# lbterm.exe through a harness that returns what it set as a SendString key binding, which
# `lbterm show-keys --lua` prints (as Terminal's lbterm.Tests.ps1 does). Skipped before a build.

BeforeDiscovery {
  $lbterm = Join-Path $PSScriptRoot '..\..\target\release\lbterm.exe'
  $built = Test-Path $lbterm
  $userConfig = Join-Path $env:USERPROFILE '.lbterm.lua'
}

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
  $lbterm = Join-Path $PSScriptRoot '..\..\target\release\lbterm.exe'
  $lua = { param($p) $p -replace '\\', '/' }

  # Writes try.lua over $UserConfig, then a harness reporting what try.lua's config holds
  function Get-TryConfig([string]$UserConfig) {
    $try = Join-Path $TestDrive 'try.lua'
    [IO.File]::WriteAllText($try, (New-TryConfig -UserConfig $UserConfig -Self $try `
      -Socket 'C:\lb\target\try\sock' -Server 'C:\lb\target\try\bin\lbterm-mux-server.exe'))
    $harness = Join-Path $TestDrive 'harness.lua'
    [IO.File]::WriteAllText($harness, @"
local wezterm = require 'wezterm'
local c = dofile('$(& $lua $try)')
local d = c.unix_domains[1]
return { keys = { { key = 'F1', mods = 'NONE', action = wezterm.action.SendString(
  'summon=' .. tostring(c.lbterm_summon) .. '|restore=' .. tostring(c.lbterm_crash_restore)
  .. '|domains=' .. #c.unix_domains .. '|name=' .. d.name .. '|sock=' .. d.socket_path
  .. '|serve=' .. table.concat(d.serve_command, ',')
  .. '|args=' .. table.concat(c.default_gui_startup_args, ',')) } } }
"@)
    $out = & $lbterm --config-file $harness show-keys --lua 2>&1 | Out-String
    $m = [regex]::Match($out, 'SendString\s*[\(\{]?\s*[''"]([^''"]*summon=[^''"]*)[''"]')
    if (-not $m.Success) { throw "no report from the harness:`n$out" }
    $report = @{}
    foreach ($pair in $m.Groups[1].Value -split '\|') { $k, $v = $pair -split '=', 2; $report[$k] = $v }
    $report
  }
}

Describe 'New-TryConfig' -Skip:(-not $built) {
  BeforeAll {
    # A config like Lachlan's: the summon key, crash restore left on, the daily mux domain
    $stub = Join-Path $TestDrive 'user.lua'
    [IO.File]::WriteAllText($stub, @"
local wezterm = require 'wezterm'
local c = wezterm.config_builder()
c.lbterm_summon = { key = 'PageUp' }
c.unix_domains = { { name = 'unix' } }
c.default_gui_startup_args = { 'connect', 'unix' }
return c
"@)
    $report = Get-TryConfig $stub
  }

  It 'puts the window on a domain and socket of its own, not the daily one' {
    $report.domains | Should -Be '1'
    $report.name | Should -Be 'try'
    $report.sock | Should -Be 'C:/lb/target/try/sock'
    $report.args | Should -Be 'connect,try'
  }

  It 'has the server it starts read the same file' {
    $report.serve | Should -Be "C:/lb/target/try/bin/lbterm-mux-server.exe,--config-file,$(& $lua (Join-Path $TestDrive 'try.lua')),--daemonize"
  }

  It 'leaves the summon key to the daily LBTerm, and keeps no crash note' {
    $report.summon | Should -Be 'nil'
    $report.restore | Should -Be 'false'
  }
}

Describe "Lachlan's config under a try window" -Skip:(-not ($built -and (Test-Path $userConfig))) {
  It 'loads' {
    $report = Get-TryConfig (Join-Path $env:USERPROFILE '.lbterm.lua')
    $report.name | Should -Be 'try'
    $report.summon | Should -Be 'nil'
  }
}
