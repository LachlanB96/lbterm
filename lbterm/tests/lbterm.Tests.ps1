# Pester 5+, run by `lbterm.ps1 check`. lbterm-lib.ps1's file handling, on folders under TestDrive:
# nothing here touches the real build, install or OpenSSL cache.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')

  # A built OpenSSL as openssl-src leaves it: headers naming the version, the two static libs
  function New-OpensslBuild([string]$Dir, [string]$Version) {
    New-Item -ItemType Directory -Force (Join-Path $Dir 'include\openssl'), (Join-Path $Dir 'lib') | Out-Null
    Set-Content (Join-Path $Dir 'include\openssl\opensslv.h') "# define OPENSSL_VERSION_STR `"$Version`""
    Set-Content (Join-Path $Dir 'lib\libssl.lib') 'ssl'
    Set-Content (Join-Path $Dir 'lib\libcrypto.lib') 'crypto'
  }
}

Describe 'Test-VersionNamesCommit' {
  It 'takes a version stamped at the commit' {
    Test-VersionNamesCommit '20261021-091512-97d757db' '97d757db' | Should -BeTrue
  }

  It 'turns down one stamped at another commit, or none' {
    Test-VersionNamesCommit '20261020-222558-f3b4ccda' '97d757db' | Should -BeFalse
    Test-VersionNamesCommit '' '97d757db' | Should -BeFalse
    Test-VersionNamesCommit $null '97d757db' | Should -BeFalse
  }
}

Describe 'Get-OpensslSrcVersion' {
  It 'reads the version Cargo.lock pins' {
    $lock = Join-Path $TestDrive 'Cargo.lock'
    Set-Content $lock "[[package]]`nname = `"openssl-probe`"`nversion = `"0.1.6`"`n`n[[package]]`nname = `"openssl-src`"`nversion = `"300.6.1+3.6.3`"`nsource = `"registry`""
    Get-OpensslSrcVersion $lock | Should -Be '300.6.1+3.6.3'
  }

  It 'finds one in the real Cargo.lock' {
    Get-OpensslSrcVersion (Join-Path $PSScriptRoot '..\..\Cargo.lock') | Should -Match '^\d+\.\d+\.\d+\+\d+\.\d+\.\d+$'
  }

  It 'says nothing without openssl-src' {
    $lock = Join-Path $TestDrive 'none.lock'
    Set-Content $lock "[[package]]`nname = `"serde`"`nversion = `"1.0.0`""
    Get-OpensslSrcVersion $lock | Should -BeNullOrEmpty
  }
}

Describe 'Test-OpensslBuild' {
  It 'takes a whole build of the version' {
    New-OpensslBuild (Join-Path $TestDrive 'whole') '3.6.3'
    Test-OpensslBuild (Join-Path $TestDrive 'whole') '300.6.1+3.6.3' | Should -BeTrue
  }

  It 'refuses another version' {
    New-OpensslBuild (Join-Path $TestDrive 'older') '3.5.0'
    Test-OpensslBuild (Join-Path $TestDrive 'older') '300.6.1+3.6.3' | Should -BeFalse
  }

  It 'refuses one missing a library' {
    New-OpensslBuild (Join-Path $TestDrive 'half') '3.6.3'
    Remove-Item (Join-Path $TestDrive 'half\lib\libcrypto.lib')
    Test-OpensslBuild (Join-Path $TestDrive 'half') '300.6.1+3.6.3' | Should -BeFalse
  }

  It 'refuses a folder that is not there' {
    Test-OpensslBuild (Join-Path $TestDrive 'nothing') '300.6.1+3.6.3' | Should -BeFalse
  }
}

Describe 'Save-OpensslCache' {
  BeforeEach {
    $build = Join-Path $TestDrive "build-$([guid]::NewGuid())"
    $cache = Join-Path $TestDrive "cache-$([guid]::NewGuid())\300.6.1+3.6.3"
  }

  It 'keeps the build of the right version, of the two openssl-sys left' {
    New-OpensslBuild (Join-Path $build 'openssl-sys-aaa\out\openssl-build\install') '3.5.0'
    New-OpensslBuild (Join-Path $build 'openssl-sys-bbb\out\openssl-build\install') '3.6.3'
    Save-OpensslCache -BuildDir $build -Cache $cache -SrcVersion '300.6.1+3.6.3' | Should -BeTrue
    Test-OpensslBuild $cache '300.6.1+3.6.3' | Should -BeTrue
    Test-Path "$cache.partial" | Should -BeFalse
  }

  It 'keeps nothing when no build has the version' {
    New-OpensslBuild (Join-Path $build 'openssl-sys-aaa\out\openssl-build\install') '3.5.0'
    Save-OpensslCache -BuildDir $build -Cache $cache -SrcVersion '300.6.1+3.6.3' | Should -BeFalse
    Test-Path $cache | Should -BeFalse
  }

  It 'leaves a cache it already has alone' {
    New-OpensslBuild $cache '3.6.3'
    Set-Content (Join-Path $cache 'lib\libssl.lib') 'kept'
    New-OpensslBuild (Join-Path $build 'openssl-sys-bbb\out\openssl-build\install') '3.6.3'
    Save-OpensslCache -BuildDir $build -Cache $cache -SrcVersion '300.6.1+3.6.3' | Should -BeTrue
    Get-Content (Join-Path $cache 'lib\libssl.lib') | Should -Be 'kept'
  }

  It 'replaces a cache left half-copied' {
    New-OpensslBuild $cache '3.6.3'
    Remove-Item (Join-Path $cache 'lib\libcrypto.lib')
    New-OpensslBuild (Join-Path $build 'openssl-sys-bbb\out\openssl-build\install') '3.6.3'
    Save-OpensslCache -BuildDir $build -Cache $cache -SrcVersion '300.6.1+3.6.3' | Should -BeTrue
    Test-OpensslBuild $cache '300.6.1+3.6.3' | Should -BeTrue
  }

  It 'copes with no build folder at all' {
    Save-OpensslCache -BuildDir (Join-Path $TestDrive 'no-target') -Cache $cache -SrcVersion '300.6.1+3.6.3' | Should -BeFalse
  }
}
