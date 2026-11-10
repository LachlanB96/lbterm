# Pester 5+, run by `lbterm.ps1 check`. One set of features for the build and the check's tests
# (lbterm-unify): cargo tree's lines read, the host side told from the target side, the libraries
# two selections build differently found, and lbterm-unify's Cargo.toml written from them. Also
# Pester's own run beside cargo. Nothing here runs cargo.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')

  # A Split-CargoFeatures result from target and full tree lines
  function New-Resolve([string[]]$Target, [string[]]$Full) {
    Split-CargoFeatures (ConvertFrom-CargoTree $Target) (ConvertFrom-CargoTree $Full)
  }
}

Describe 'ConvertFrom-CargoTree' {
  It 'reads registry, path, git and proc-macro lines, and the (*) of a repeat' {
    $m = ConvertFrom-CargoTree @(
      'serde v1.0.229|alloc,default,std'
      'serde v1.0.229|alloc,default,std (*)'
      'wezterm-blob-leases v0.1.1 (C:\src\lbterm\wezterm-blob-leases)|default,simple_tempdir'
      'xcb v1.2.0 (https://github.com/x/xcb?rev=abc#abcdef12)|'
      'serde_derive v1.0.229 (proc-macro)|default'
      'warning: something cargo said'
    )
    @($m.Keys) | Should -Be @('serde v1.0.229', 'wezterm-blob-leases v0.1.1', 'xcb v1.2.0', 'serde_derive v1.0.229')
    $m['serde v1.0.229'].Source | Should -Be 'registry'
    @($m['serde v1.0.229'].Features) | Should -Be @('alloc', 'default', 'std')
    $m['wezterm-blob-leases v0.1.1'].Source | Should -Be 'path'
    $m['wezterm-blob-leases v0.1.1'].Path | Should -Be 'C:\src\lbterm\wezterm-blob-leases'
    $m['xcb v1.2.0'].Source | Should -Be 'git'
    @($m['xcb v1.2.0'].Features).Count | Should -Be 0
    $m['serde_derive v1.0.229'].Source | Should -Be 'registry'
  }

  It 'keeps each node of a library as a variant, and the union of their features' {
    $m = ConvertFrom-CargoTree @('syn v2.0.119|derive,parsing', 'syn v2.0.119|full,parsing')
    @($m['syn v2.0.119'].Features) | Should -Be @('derive', 'full', 'parsing')
    $m['syn v2.0.119'].Variants.Count | Should -Be 2
  }
}

Describe 'Split-CargoFeatures' {
  It 'puts on the host side what the full tree has besides the target side' {
    $r = New-Resolve -Target @('serde v1.0.229|std', 'syn v2.0.119|parsing') -Full @(
      'serde v1.0.229|std', 'syn v2.0.119|parsing', 'syn v2.0.119|derive,full,parsing', 'cc v1.2.0|parallel')
    @($r.Target.Keys) | Should -Be @('serde v1.0.229', 'syn v2.0.119')
    @($r.Host.Keys) | Should -Be @('syn v2.0.119', 'cc v1.2.0')
    @($r.Host['syn v2.0.119'].Features) | Should -Be @('derive', 'full', 'parsing')
  }
}

Describe 'Compare-CargoFeatures' {
  It 'finds a library two selections build with different features, with the union and who lacks it' {
    $sel = [ordered]@{
      build = New-Resolve -Target @('smallvec v1.15.2|union', 'serde v1.0.229|std', 'only-here v1.0.0|a') -Full @()
      check = New-Resolve -Target @('smallvec v1.15.2|', 'serde v1.0.229|alloc,std') -Full @()
    }
    $d = @(Compare-CargoFeatures $sel)
    @($d.Name) | Should -Be @('smallvec', 'serde')
    $s = $d | Where-Object Name -eq 'serde'
    $s.Side | Should -Be 'Target'
    @($s.Features) | Should -Be @('alloc', 'std')
    @($s.Missing) | Should -Be @('build')
    @(($d | Where-Object Name -eq 'smallvec').Missing) | Should -Be @('check')
  }

  It 'compares the host side on its own, and finds nothing when the selections agree' {
    $same = @('serde v1.0.229|std')
    $sel = [ordered]@{
      build = New-Resolve -Target $same -Full ($same + 'syn v2.0.119|parsing')
      check = New-Resolve -Target $same -Full ($same + 'syn v2.0.119|full,parsing')
    }
    $d = @(Compare-CargoFeatures $sel)
    $d.Count | Should -Be 1
    $d[0].Side | Should -Be 'Host'
    $d[0].Name | Should -Be 'syn'
    $agree = [ordered]@{ build = New-Resolve -Target $same -Full $same; check = New-Resolve -Target $same -Full $same }
    @(Compare-CargoFeatures $agree).Count | Should -Be 0
  }
}

Describe 'New-UnifyManifest' {
  BeforeAll {
    $dir = 'C:\src\lbterm\lbterm-unify'
    function Pin($side, $name, $ver, $feats, $src = 'registry', $path) {
      [pscustomobject]@{ Side = $side; Name = $name; Version = $ver; Source = $src; Path = $path; Features = $feats }
    }
  }

  It 'pins each library with exactly its features, two versions of one under names of their own' {
    $text = New-UnifyManifest -Dir $dir -Pins @(
      (Pin 'Target' 'serde' '1.0.229' @('alloc', 'std'))
      (Pin 'Target' 'windows-sys' '0.61.2' @('Win32_Foundation'))
      (Pin 'Target' 'windows-sys' '0.59.0' @('Win32_System_Console'))
      (Pin 'Target' 'wezterm-blob-leases' '0.1.1' @('simple_tempdir') 'path' 'C:\src\lbterm\wezterm-blob-leases'))
    $text | Should -Match '(?m)^name = "lbterm-unify"$'
    $text | Should -Match '(?m)^test = false$'
    $text | Should -Match '(?m)^serde = \{ version = "1.0.229", default-features = false, features = \["alloc", "std"\] \}$'
    $text | Should -Match '(?m)^windows-sys-0_59_0 = \{ package = "windows-sys", version = "0.59.0", default-features = false, features = \["Win32_System_Console"\] \}$'
    $text | Should -Match '(?m)^windows-sys-0_61_2 = \{ package = "windows-sys", version = "0.61.2",'
    $text | Should -Match '(?m)^wezterm-blob-leases = \{ path = "../wezterm-blob-leases", default-features = false, features = \["simple_tempdir"\] \}$'
    $text | Should -Not -Match 'build-dependencies'
  }

  It 'puts host-side pins under [build-dependencies], and refuses a git dependency' {
    $text = New-UnifyManifest -Dir $dir -Pins @((Pin 'Target' 'serde' '1.0.229' @('std')), (Pin 'Host' 'syn' '2.0.119' @('full')))
    $text | Should -Match '(?s)\[dependencies\]\nserde = .*\n\n\[build-dependencies\]\nsyn = \{ version = "2.0.119", default-features = false, features = \["full"\] \}\n$'
    { New-UnifyManifest -Dir $dir -Pins @((Pin 'Target' 'xcb' '1.2.0' @() 'git')) } | Should -Throw '*comes from git*'
  }

  It 'with no pins is an empty [dependencies]' {
    New-UnifyManifest -Dir $dir -Pins @() | Should -Match '\[dependencies\]\n$'
  }
}

Describe 'Get-UnifyKey' {
  It 'changes with a Cargo.toml or Cargo.lock, and not with other files' {
    $r = Join-Path $TestDrive 'repo'
    New-Item -ItemType Directory -Force (Join-Path $r 'a') | Out-Null
    git -C $r init -q
    Set-Content (Join-Path $r 'Cargo.lock') 'lock'
    Set-Content (Join-Path $r 'a\Cargo.toml') '[package]'
    Set-Content (Join-Path $r 'a\lib.rs') 'fn x() {}'
    $k1 = Get-UnifyKey -Repo $r -Cargo 'cargo 1.96.0'
    Set-Content (Join-Path $r 'a\lib.rs') 'fn y() {}'
    Get-UnifyKey -Repo $r -Cargo 'cargo 1.96.0' | Should -Be $k1
    Set-Content (Join-Path $r 'a\Cargo.toml') '[package] # changed'
    $k2 = Get-UnifyKey -Repo $r -Cargo 'cargo 1.96.0'
    $k2 | Should -Not -Be $k1
    Set-Content (Join-Path $r 'Cargo.lock') 'lock 2'
    Get-UnifyKey -Repo $r -Cargo 'cargo 1.96.0' | Should -Not -Be $k2
    Get-UnifyKey -Repo $r -Cargo 'cargo 1.97.0' | Should -Not -Be (Get-UnifyKey -Repo $r -Cargo 'cargo 1.96.0')
  }
}

Describe 'Start-PesterRun' {
  It 'runs a folder of tests in a pwsh of its own, its output in the log, and its exit code at the end' {
    $pass = Join-Path $TestDrive 'pass'; $fail = Join-Path $TestDrive 'fail'
    New-Item -ItemType Directory -Force $pass, $fail | Out-Null
    Set-Content (Join-Path $pass 'a.Tests.ps1') "Describe 'a' { It 'passes' { 1 | Should -Be 1 } }"
    Set-Content (Join-Path $fail 'b.Tests.ps1') "Describe 'b' { It 'fails on purpose' { 1 | Should -Be 2 } }"
    $p = Start-PesterRun -Tests $pass -Log (Join-Path $TestDrive 'logs\pass.log')
    $f = Start-PesterRun -Tests $fail -Log (Join-Path $TestDrive 'logs\fail.log')
    Wait-PesterRun $p -Quiet | Should -Be 0
    Wait-PesterRun $f -Quiet | Should -Be 1
    Get-Content (Join-Path $TestDrive 'logs\pass.log') -Raw | Should -Match 'Tests Passed: 1'
    Get-Content (Join-Path $TestDrive 'logs\fail.log') -Raw | Should -Match 'fails on purpose'
  }

  It 'leaves out the files it is told to, as the check does with the one that needs the build' {
    $both = Join-Path $TestDrive 'both'
    New-Item -ItemType Directory -Force $both | Out-Null
    Set-Content (Join-Path $both 'a.Tests.ps1') "Describe 'a' { It 'passes' { 1 | Should -Be 1 } }"
    Set-Content (Join-Path $both 'later.Tests.ps1') "Describe 'later' { It 'fails if it runs' { 1 | Should -Be 2 } }"
    $p = Start-PesterRun -Tests $both -Exclude (Join-Path $both 'later.Tests.ps1') -Log (Join-Path $TestDrive 'logs\both.log')
    Wait-PesterRun $p -Quiet | Should -Be 0
    Get-Content (Join-Path $TestDrive 'logs\both.log') -Raw | Should -Not -Match 'fails if it runs'
  }
}
