# Pester 5+, run by `lbterm.ps1 check`. Install, the build kept for rollback and the swap back, on
# folders under TestDrive standing in for target\release, the install and the previous build.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')

  # A build whose every file says which build it is
  function New-Build([string]$Dir, [string]$Version, [switch]$NoVersionFile) {
    foreach ($f in $LbtermExes + $LbtermExtras) {
      $path = Join-Path $Dir $f
      New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
      Set-Content $path $Version -NoNewline
    }
    if (-not $NoVersionFile) { Set-BuildVersion $Dir $Version }
  }

  function Get-Says([string]$Dir, [string]$File = 'lbterm-gui.exe') { Get-Content (Join-Path $Dir $File) -Raw }

  # Holds a file the way a running exe or loaded dll is held: it can be read and renamed, not
  # overwritten
  function Lock-File([string]$Path) {
    [IO.File]::Open($Path, 'Open', 'Read', [IO.FileShare]'Read, Delete')
  }
}

Describe 'The scripts' {
  # A parse error in lbterm.ps1 only shows when a command runs; the check should say first
  It '<_> parses' -ForEach @(Get-ChildItem (Join-Path $PSScriptRoot '..') -Recurse -Filter *.ps1 | ForEach-Object { $_.FullName }) {
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($_, [ref]$null, [ref]$errors)
    $errors | ForEach-Object { "$($_.Extent.StartLineNumber): $($_.Message)" } | Should -BeNullOrEmpty
  }
}

Describe 'Copy-Build' {
  It 'copies every file of a build, mesa\opengl32.dll included' {
    New-Build "$TestDrive\cb1\from" 'v2'
    Copy-Build -From "$TestDrive\cb1\from" -To "$TestDrive\cb1\to" | Should -Be 0
    foreach ($f in $LbtermExes + $LbtermExtras) { Get-Says "$TestDrive\cb1\to" $f | Should -Be 'v2' }
    Get-BuildVersion "$TestDrive\cb1\to" | Should -Be 'v2'
  }

  It 'renames a file in use aside and puts the new one in its place' {
    New-Build "$TestDrive\cb2\from" 'v2'
    New-Build "$TestDrive\cb2\to" 'v1'
    $held = Lock-File "$TestDrive\cb2\to\lbterm-gui.exe"
    try {
      Copy-Build -From "$TestDrive\cb2\from" -To "$TestDrive\cb2\to" -Stamp '20261020' | Should -Be 1
      Get-Says "$TestDrive\cb2\to" | Should -Be 'v2'
      Get-Content "$TestDrive\cb2\to\lbterm-gui.exe.20261020.old" -Raw | Should -Be 'v1'
    } finally { $held.Dispose() }
    Remove-OldBuilds "$TestDrive\cb2\to"
    Test-Path "$TestDrive\cb2\to\lbterm-gui.exe.20261020.old" | Should -BeFalse
  }

  It 'refuses a build missing a file' {
    New-Build "$TestDrive\cb3\from" 'v2'
    Remove-Item "$TestDrive\cb3\from\conpty.dll"
    { Copy-Build -From "$TestDrive\cb3\from" -To "$TestDrive\cb3\to" } | Should -Throw '*conpty.dll is missing*'
  }
}

Describe 'Remove-OldBuilds' {
  It 'leaves an old file something still holds' {
    New-Item -ItemType Directory -Force "$TestDrive\rob" | Out-Null
    Set-Content "$TestDrive\rob\lbterm-gui.exe.1.old" 'held'
    # Without Delete sharing: a running exe can be renamed but not deleted, which a plain handle
    # can only stand in for by refusing both
    $held = [IO.File]::Open("$TestDrive\rob\lbterm-gui.exe.1.old", 'Open', 'Read', [IO.FileShare]::Read)
    try {
      Remove-OldBuilds "$TestDrive\rob"
      Test-Path "$TestDrive\rob\lbterm-gui.exe.1.old" | Should -BeTrue
    } finally { $held.Dispose() }
  }
}

Describe 'Save-Previous' {
  It 'keeps the installed build when a different one replaces it' {
    New-Build "$TestDrive\sp1\install" 'v1'
    Save-Previous -Install "$TestDrive\sp1\install" -Previous "$TestDrive\sp1\previous" -NewVersion 'v2' | Should -BeTrue
    Get-Says "$TestDrive\sp1\previous" | Should -Be 'v1'
    Get-BuildVersion "$TestDrive\sp1\previous" | Should -Be 'v1'
    Test-Path "$TestDrive\sp1\previous.partial" | Should -BeFalse
  }

  It 'keeps the build to go back to when the same build is installed again' {
    New-Build "$TestDrive\sp2\install" 'v2'
    New-Build "$TestDrive\sp2\previous" 'v1'
    Save-Previous -Install "$TestDrive\sp2\install" -Previous "$TestDrive\sp2\previous" -NewVersion 'v2' | Should -BeFalse
    Get-Says "$TestDrive\sp2\previous" | Should -Be 'v1'
  }

  It 'replaces an older kept build' {
    New-Build "$TestDrive\sp3\install" 'v2'
    New-Build "$TestDrive\sp3\previous" 'v1'
    Save-Previous -Install "$TestDrive\sp3\install" -Previous "$TestDrive\sp3\previous" -NewVersion 'v3' | Should -BeTrue
    Get-Says "$TestDrive\sp3\previous" | Should -Be 'v2'
  }

  It 'keeps nothing on a first install' {
    Save-Previous -Install "$TestDrive\sp4\install" -Previous "$TestDrive\sp4\previous" -NewVersion 'v1' | Should -BeFalse
    Test-Path "$TestDrive\sp4\previous" | Should -BeFalse
  }

  It 'copies an installed build whose files are in use' {
    New-Build "$TestDrive\sp5\install" 'v1'
    $held = Lock-File "$TestDrive\sp5\install\lbterm-gui.exe"
    try {
      Save-Previous -Install "$TestDrive\sp5\install" -Previous "$TestDrive\sp5\previous" -NewVersion 'v2' | Should -BeTrue
    } finally { $held.Dispose() }
    Get-Says "$TestDrive\sp5\previous" | Should -Be 'v1'
  }
}

Describe 'Switch-Build' {
  It 'swaps the installed and the kept build, and back again' {
    New-Build "$TestDrive\sw1\install" 'v2'
    New-Build "$TestDrive\sw1\previous" 'v1'
    Switch-Build -Install "$TestDrive\sw1\install" -Previous "$TestDrive\sw1\previous" | Should -Be 0
    Get-Says "$TestDrive\sw1\install" | Should -Be 'v1'
    Get-BuildVersion "$TestDrive\sw1\install" | Should -Be 'v1'
    Get-Says "$TestDrive\sw1\previous" | Should -Be 'v2'
    Get-BuildVersion "$TestDrive\sw1\previous" | Should -Be 'v2'
    Switch-Build -Install "$TestDrive\sw1\install" -Previous "$TestDrive\sw1\previous" | Out-Null
    Get-Says "$TestDrive\sw1\install" | Should -Be 'v2'
    Get-Says "$TestDrive\sw1\previous" | Should -Be 'v1'
    Test-Path "$TestDrive\sw1\previous.swap" | Should -BeFalse
  }

  It 'swaps in beside a running build' {
    New-Build "$TestDrive\sw2\install" 'v2'
    New-Build "$TestDrive\sw2\previous" 'v1'
    $held = Lock-File "$TestDrive\sw2\install\lbterm-gui.exe"
    try {
      Switch-Build -Install "$TestDrive\sw2\install" -Previous "$TestDrive\sw2\previous" | Should -Be 1
    } finally { $held.Dispose() }
    Get-Says "$TestDrive\sw2\install" | Should -Be 'v1'
    Get-Says "$TestDrive\sw2\previous" | Should -Be 'v2'
  }

  It 'refuses when no build is kept' {
    New-Build "$TestDrive\sw3\install" 'v2'
    { Switch-Build -Install "$TestDrive\sw3\install" -Previous "$TestDrive\sw3\previous" } | Should -Throw '*No build kept*'
    Get-Says "$TestDrive\sw3\install" | Should -Be 'v2'
  }
}
