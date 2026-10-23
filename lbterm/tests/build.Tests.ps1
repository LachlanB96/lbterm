# Pester 5+, run by `lbterm.ps1 check`. How lbterm.ps1 runs cargo: the thermal guard's refusals
# told from cargo's failures and tried again (against fake guards run by Git's bash, the way
# guarded.sh is), and sccache found and set up. Nothing here runs cargo, the real guard or sccache.

BeforeDiscovery {
  $bash = 'C:\Program Files\Git\bin\bash.exe'
  $hasBash = Test-Path $bash
}

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
  $bash = 'C:\Program Files\Git\bin\bash.exe'

  # A fake guarded.sh: refuses (as cputemp.ps1 -Check does, exit 1) on its first $Refusals runs,
  # counted in a file beside it, then runs the command it's given, as the real one does
  function New-FakeGuard([int]$Refusals) {
    $dir = Join-Path $TestDrive "guard-$([guid]::NewGuid())"
    New-Item -ItemType Directory $dir | Out-Null
    $script = @(
      '#!/usr/bin/env bash'
      'count="$(dirname "$0")/runs"'
      'n=$(( $(cat "$count" 2>/dev/null || echo 0) + 1 )); echo $n > "$count"'
      "if [ `$n -le $Refusals ]; then echo `"cputemp: CPU is at 84 C; wait until it's below 80 C`"; exit 1; fi"
      '"$@"'
    ) -join "`n"
    [IO.File]::WriteAllText((Join-Path $dir 'guarded.sh'), "$script`n")
    # With slashes, as lbterm.ps1 names the real one: bash's dirname doesn't split at backslashes
    (Join-Path $dir 'guarded.sh') -replace '\\', '/'
  }

  function Get-Runs([string]$Guard) {
    $f = Join-Path (Split-Path $Guard) 'runs'
    if (Test-Path $f) { [int](Get-Content $f) } else { 0 }
  }

  # An empty file standing in for an exe
  function New-Exe([string]$Path) {
    New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
    Set-Content $Path ''
    $Path
  }
}

Describe 'Get-DevSettings' {
  It 'reads the guard, its stop file and the lookalike config' {
    $f = Join-Path $TestDrive 'dev.json'
    Set-Content $f '{ "guard": "C:\\lab\\guarded.sh", "thermalStop": "C:/lab/STOP", "config": "C:/cfg/lbterm.lua" }'
    $s = Get-DevSettings -Path $f
    $s.guard | Should -Be 'C:/lab/guarded.sh'
    $s.thermalStop | Should -Be 'C:/lab/STOP'
    $s.config | Should -Be 'C:/cfg/lbterm.lua'
  }

  It 'has no guard without the file, so a build refuses unless -NoGuard' {
    $s = Get-DevSettings -Path (Join-Path $TestDrive 'none.json')
    $s.guard | Should -BeNullOrEmpty
    $s.thermalStop | Should -BeNullOrEmpty
    $s.config | Should -BeNullOrEmpty
  }

  It 'leaves out what the file leaves out' {
    $f = Join-Path $TestDrive 'guard-only.json'
    Set-Content $f '{ "guard": "C:/lab/guarded.sh" }'
    $s = Get-DevSettings -Path $f
    $s.guard | Should -Be 'C:/lab/guarded.sh'
    $s.config | Should -BeNullOrEmpty
  }
}

Describe 'Get-GuardOutcome' {
  It 'passes a run that passed' {
    Get-GuardOutcome -ExitCode 0 -Started $true -Attempt 1 -Attempts 5 | Should -Be 'done'
  }

  It "tries again when the guard refused before starting the command" {
    Get-GuardOutcome -ExitCode 1 -Started $false -Attempt 1 -Attempts 5 | Should -Be 'retry'
    Get-GuardOutcome -ExitCode 1 -Started $false -Attempt 4 -Attempts 5 | Should -Be 'retry'
  }

  It 'gives up on the last attempt' {
    Get-GuardOutcome -ExitCode 1 -Started $false -Attempt 5 -Attempts 5 | Should -Be 'refused'
    Get-GuardOutcome -ExitCode 1 -Started $false -Attempt 1 -Attempts 1 | Should -Be 'refused'
  }

  It 'never runs again a command that started and failed, even with exit 1 (cargo on a bad argument)' {
    Get-GuardOutcome -ExitCode 1 -Started $true -Attempt 1 -Attempts 5 | Should -Be 'failed'
    Get-GuardOutcome -ExitCode 101 -Started $true -Attempt 1 -Attempts 5 | Should -Be 'failed'
  }

  It 'takes any other exit before the start for something else gone wrong, not a refusal' {
    Get-GuardOutcome -ExitCode 127 -Started $false -Attempt 1 -Attempts 5 | Should -Be 'failed'
    Get-GuardOutcome -ExitCode 2 -Started $false -Attempt 1 -Attempts 5 | Should -Be 'failed'
  }

  It "doesn't retry THERMAL-STOP, which stays until Lachlan deletes it" {
    Get-GuardOutcome -ExitCode 1 -Started $false -ThermalStop $true -Attempt 1 -Attempts 5 | Should -Be 'stop'
  }
}

Describe 'Get-GuardedCommand' {
  It 'writes the marker, then hands over to the command with its arguments as they are' {
    $c = Get-GuardedCommand -Marker 'C:\lb\target\lbterm-cargo-1.started' -Command cargo -Arguments 'build', '-p', 'wezterm'
    $c[0..2] | Should -Be @('bash', '-c', ': > "$0" && exec "$@"')
    $c[3] | Should -Be 'C:/lb/target/lbterm-cargo-1.started'
    $c[4..7] | Should -Be @('cargo', 'build', '-p', 'wezterm')
  }
}

Describe 'Invoke-Guarded' -Skip:(-not $hasBash) {
  BeforeEach {
    $marker = Join-Path $TestDrive "started-$([guid]::NewGuid())"
    $script:waits = 0
    $wait = { $script:waits++ }
  }

  It 'runs the command once when the guard lets it start' {
    $guard = New-FakeGuard 0
    $out = Invoke-Guarded -Bash $bash -Guard $guard -Command sh -Arguments '-c', 'echo "ran [$0]"', 'one two' -Marker $marker -Wait $wait
    $out | Should -Contain 'ran [one two]'
    Get-Runs $guard | Should -Be 1
    $script:waits | Should -Be 1
    Test-Path $marker | Should -BeFalse
  }

  It 'waits and tries again after refusals, then runs it' {
    $guard = New-FakeGuard 2
    $out = Invoke-Guarded -Bash $bash -Guard $guard -Command sh -Arguments '-c', 'echo ran' -Marker $marker -Attempts 5 -Wait $wait
    $out | Should -Contain 'ran'
    @($out | Where-Object { $_ -like '*refused to start cargo (try * of 5)*' }).Count | Should -Be 2
    Get-Runs $guard | Should -Be 3
    $script:waits | Should -Be 3
  }

  It 'fails after the last refusal' {
    $guard = New-FakeGuard 99
    { Invoke-Guarded -Bash $bash -Guard $guard -Command cargo -Arguments 'build' -Marker $marker -Attempts 3 -Wait $wait } |
      Should -Throw '*refused to start cargo build 3 times*'
    Get-Runs $guard | Should -Be 3
  }

  It 'never runs again a command that started and failed with exit 1' {
    $guard = New-FakeGuard 0
    { Invoke-Guarded -Bash $bash -Guard $guard -Command sh -Arguments '-c', 'exit 1' -Marker $marker -Attempts 5 -Wait $wait } |
      Should -Throw '*failed (exit 1)*'
    Get-Runs $guard | Should -Be 1
  }

  It 'passes on the exit code of a command that failed otherwise' {
    $guard = New-FakeGuard 0
    { Invoke-Guarded -Bash $bash -Guard $guard -Command sh -Arguments '-c', 'exit 101' -Marker $marker -Wait $wait } |
      Should -Throw '*failed (exit 101)*'
  }

  It 'stops at once for THERMAL-STOP, saying what it says' {
    $guard = New-FakeGuard 99
    $stop = Join-Path $TestDrive 'THERMAL-STOP'
    Set-Content $stop '2026-10-22 01:00:00: guard read 91 C'
    { Invoke-Guarded -Bash $bash -Guard $guard -Command cargo -Marker $marker -StopFile $stop -Attempts 5 -Wait $wait } |
      Should -Throw '*THERMAL-STOP is there (2026-10-22 01:00:00: guard read 91 C)*'
    Get-Runs $guard | Should -Be 1
  }
}

Describe 'Find-Sccache' {
  BeforeEach {
    $root = Join-Path $TestDrive "find-$([guid]::NewGuid())"
    $cargo = Join-Path $root 'cargo'
    $winget = Join-Path $root 'winget'
  }

  It 'takes the newest of winget package folders, which nothing puts on PATH' {
    $pkg = Join-Path $winget 'Packages\Mozilla.sccache_Microsoft.Winget.Source_8wekyb3d8bbwe'
    New-Exe (Join-Path $pkg 'sccache-v0.9.1-x86_64-pc-windows-msvc\sccache.exe') | Out-Null
    $new = New-Exe (Join-Path $pkg 'sccache-v0.18.0-x86_64-pc-windows-msvc\sccache.exe')
    New-Item -ItemType Directory (Join-Path $pkg 'sccache-v0.19.0-x86_64-pc-windows-msvc') | Out-Null   # no exe in it
    Find-Sccache -PathDirs @() -CargoHome $cargo -WingetRoots $winget | Should -Be $new
  }

  It 'takes one on PATH first, then cargo install''s, then winget''s link' {
    $onPath = New-Exe (Join-Path $root 'bin\sccache.exe')
    $installed = New-Exe (Join-Path $cargo 'bin\sccache.exe')
    $link = New-Exe (Join-Path $winget 'Links\sccache.exe')
    Find-Sccache -PathDirs 'Q:\no-such-drive', '', (Join-Path $root 'bin') -CargoHome $cargo -WingetRoots $winget | Should -Be $onPath
    Find-Sccache -PathDirs @() -CargoHome $cargo -WingetRoots $winget | Should -Be $installed
    Remove-Item $installed
    Find-Sccache -PathDirs @() -CargoHome $cargo -WingetRoots $winget | Should -Be $link
  }

  It 'finds nothing when it isn''t installed' {
    Find-Sccache -PathDirs @() -CargoHome $cargo -WingetRoots $winget | Should -BeNullOrEmpty
  }
}

Describe 'Get-SccacheEnvironment' {
  It 'runs cargo''s clients in client-side mode, compiling anyway when the server is lost' {
    $e = Get-SccacheEnvironment
    $e.SCCACHE_CLIENT_SIDE | Should -Be '1'
    $e.SCCACHE_IGNORE_SERVER_IO_ERROR | Should -Be '1'
    $e.SCCACHE_IDLE_TIMEOUT | Should -Be '3600'
    $e.Contains('SCCACHE_START_SERVER') | Should -BeFalse
  }

  It 'makes sccache.exe the server itself, in the foreground, and only that' {
    $e = Get-SccacheEnvironment -IdleSeconds 600 -Server
    $e.SCCACHE_START_SERVER | Should -Be '1'
    $e.SCCACHE_NO_DAEMON | Should -Be '1'
    $e.SCCACHE_IDLE_TIMEOUT | Should -Be '600'
    $e.Contains('SCCACHE_CLIENT_SIDE') | Should -BeFalse
  }
}

Describe 'Test-SccacheServer' {
  It 'finds something listening, and nothing once it stops' {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = $listener.LocalEndpoint.Port
    try { Test-SccacheServer $port | Should -BeTrue } finally { $listener.Stop() }
    Test-SccacheServer $port | Should -BeFalse
  }
}
