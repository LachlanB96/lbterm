# Pester 5+, run by `lbterm.ps1 check`. How lbterm.ps1 runs cargo: sccache found and set up.
# Nothing here runs cargo or sccache. (The thermal guard's tests went with the guard, 2026-10-25:
# cargo runs straight inside the warden job.)

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')

  # An empty file standing in for an exe
  function New-Exe([string]$Path) {
    New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
    Set-Content $Path ''
    $Path
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
