# Pester 5+, run by `lbterm.ps1 check`. What builds and e2e runs leave behind in a tree's target\
# (lbterm-lib.ps1, "What builds and test runs leave behind"), pruned by each build and e2e run and by
# lbterm.ps1 prune. Every folder is a scratch one in TestDrive, dated by hand.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
  $script:now = [datetime]'2026-11-08 13:00'

  # A folder at $Path with one file of $Bytes, last written $Ago before $now
  function New-Dated([string]$Path, [timespan]$Ago, [int]$Bytes = 10) {
    New-Item -ItemType Directory -Force $Path | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $Path 'f.bin'), [byte[]]::new($Bytes))
    (Get-Item -LiteralPath $Path).LastWriteTime = $script:now - $Ago
  }
  function Get-Names($items) { @($items | ForEach-Object Name | Sort-Object) }
}

Describe 'Get-StaleIncremental' {
  BeforeEach {
    $script:inc = Join-Path $TestDrive ([guid]::NewGuid().ToString('n').Substring(0, 8))
    New-Item -ItemType Directory -Force $script:inc | Out-Null
  }

  It 'keeps each crate''s newest folder however old it is' {
    New-Dated (Join-Path $inc 'lbterm_gui-aaa') ([timespan]::FromDays(20))
    New-Dated (Join-Path $inc 'term-bbb') ([timespan]::FromDays(9))
    Get-StaleIncremental -Dir $inc -Now $now | Should -BeNullOrEmpty
  }

  It 'takes a crate''s older folders no build wrote in two days and more than a day older than its newest' {
    New-Dated (Join-Path $inc 'lbterm_gui-new') ([timespan]::FromHours(1))
    New-Dated (Join-Path $inc 'lbterm_gui-old') ([timespan]::FromDays(4))
    New-Dated (Join-Path $inc 'lbterm_gui-older') ([timespan]::FromDays(8))
    Get-Names (Get-StaleIncremental -Dir $inc -Now $now) | Should -Be @('lbterm_gui-old', 'lbterm_gui-older')
  }

  It 'keeps the release build''s folder and the tests'' when they were built within a day of each other' {
    New-Dated (Join-Path $inc 'mux-release') ([timespan]::FromDays(5))
    New-Dated (Join-Path $inc 'mux-tests') ([timespan]::FromDays(5.5))
    Get-StaleIncremental -Dir $inc -Now $now | Should -BeNullOrEmpty
  }

  It 'keeps a folder written in the last two days, though its crate has a newer' {
    New-Dated (Join-Path $inc 'config-new') ([timespan]::FromMinutes(5))
    New-Dated (Join-Path $inc 'config-yesterday') ([timespan]::FromDays(1.5))
    Get-StaleIncremental -Dir $inc -Now $now | Should -BeNullOrEmpty
  }

  It 'tells crates apart by the whole name before the hash' {
    # wezterm_gui's and wezterm_gui_subcommands' folders are different crates
    New-Dated (Join-Path $inc 'wezterm_gui-new') ([timespan]::FromHours(1))
    New-Dated (Join-Path $inc 'wezterm_gui_subcommands-old') ([timespan]::FromDays(9))
    Get-StaleIncremental -Dir $inc -Now $now | Should -BeNullOrEmpty
  }

  It 'is nothing for a tree that has no incremental folder' {
    Get-StaleIncremental -Dir (Join-Path $TestDrive 'none') -Now $now | Should -BeNullOrEmpty
  }
}

Describe 'Get-StaleRuns' {
  BeforeEach {
    $script:e2e = Join-Path $TestDrive ([guid]::NewGuid().ToString('n').Substring(0, 8))
    New-Item -ItemType Directory -Force $script:e2e | Out-Null
  }

  It 'takes the runs older than three days and keeps the rest' {
    New-Dated (Join-Path $e2e '20261029-165521') ([timespan]::FromDays(10))
    New-Dated (Join-Path $e2e '20261104-120000') ([timespan]::FromDays(4))
    New-Dated (Join-Path $e2e '20261107-173551') ([timespan]::FromDays(1))
    Get-Names (Get-StaleRuns -Dir $e2e -Now $now) | Should -Be @('20261029-165521', '20261104-120000')
  }

  It 'never takes the newest run, however old' {
    New-Dated (Join-Path $e2e '20261020-100000') ([timespan]::FromDays(19))
    New-Dated (Join-Path $e2e '20261030-211000') ([timespan]::FromDays(9))
    Get-Names (Get-StaleRuns -Dir $e2e -Now $now) | Should -Be @('20261020-100000')
  }

  It 'leaves a folder that isn''t a run alone' {
    New-Dated (Join-Path $e2e 'captures') ([timespan]::FromDays(30))
    New-Dated (Join-Path $e2e '20261107-173551') ([timespan]::FromDays(1))
    Get-StaleRuns -Dir $e2e -Now $now | Should -BeNullOrEmpty
  }
}

Describe 'Remove-StaleBuildFiles' {
  It 'deletes what the two pick in a tree, says how much, and leaves the rest' {
    $tree = Join-Path $TestDrive 'tree'
    $inc = Join-Path $tree 'target\release\incremental'
    $e2e = Join-Path $tree 'target\e2e'
    New-Dated (Join-Path $inc 'term-new') ([timespan]::FromHours(2)) 100
    New-Dated (Join-Path $inc 'term-old') ([timespan]::FromDays(6)) 1000
    New-Dated (Join-Path $e2e '20261101-193740') ([timespan]::FromDays(7)) 500
    New-Dated (Join-Path $e2e '20261108-011356') ([timespan]::FromHours(12)) 50

    $r = Remove-StaleBuildFiles -Tree $tree -Now $now
    $r.Folders | Should -Be 2
    $r.Bytes | Should -Be 1500
    $r.Kept | Should -BeNullOrEmpty
    Get-Names (Get-ChildItem -LiteralPath $inc) | Should -Be @('term-new')
    Get-Names (Get-ChildItem -LiteralPath $e2e) | Should -Be @('20261108-011356')
    Format-StaleBuildFiles $r 'stale' | Should -Be @("pruned 2 stale folder(s) (0.0 GB) from $tree")
  }

  It 'prunes look''s runs as e2e''s, the newest kept' {
    $tree = Join-Path $TestDrive 'tree-look'
    $look = Join-Path $tree 'target\look'
    New-Dated (Join-Path $look '20261101-193740') ([timespan]::FromDays(7)) 300
    New-Dated (Join-Path $look '20261102-100000') ([timespan]::FromDays(6)) 30

    $r = Remove-StaleBuildFiles -Tree $tree -What runs -Now $now
    $r.Folders | Should -Be 1
    Get-Names (Get-ChildItem -LiteralPath $look) | Should -Be @('20261102-100000')
  }

  It 'with -What incremental leaves the e2e runs alone' {
    $tree = Join-Path $TestDrive 'tree2'
    New-Dated (Join-Path $tree 'target\release\incremental\term-new') ([timespan]::FromHours(2))
    New-Dated (Join-Path $tree 'target\release\incremental\term-old') ([timespan]::FromDays(6))
    New-Dated (Join-Path $tree 'target\e2e\20261101-193740') ([timespan]::FromDays(7))
    New-Dated (Join-Path $tree 'target\e2e\20261108-011356') ([timespan]::FromHours(12))

    (Remove-StaleBuildFiles -Tree $tree -What incremental -Now $now).Folders | Should -Be 1
    Test-Path (Join-Path $tree 'target\e2e\20261101-193740') | Should -BeTrue
  }

  It 'keeps a folder it can''t delete, says why, and goes on' {
    $tree = Join-Path $TestDrive 'tree3'
    $inc = Join-Path $tree 'target\release\incremental'
    New-Dated (Join-Path $inc 'gui-new') ([timespan]::FromHours(2))
    New-Dated (Join-Path $inc 'gui-held') ([timespan]::FromDays(6))
    New-Dated (Join-Path $inc 'mux-new') ([timespan]::FromHours(2))
    New-Dated (Join-Path $inc 'mux-old') ([timespan]::FromDays(6))
    $held = [IO.File]::Open((Join-Path $inc 'gui-held\f.bin'), 'Open', 'Read', 'None')
    try {
      (Get-Item -LiteralPath (Join-Path $inc 'gui-held')).LastWriteTime = $now - [timespan]::FromDays(6)
      $r = Remove-StaleBuildFiles -Tree $tree -Now $now
    } finally { $held.Dispose() }
    $r.Folders | Should -Be 1
    @($r.Kept).Count | Should -Be 1
    $r.Kept[0] | Should -BeLike "*gui-held*"
    Test-Path (Join-Path $inc 'mux-old') | Should -BeFalse
    (Format-StaleBuildFiles $r 'incremental')[-1] | Should -BeLike "couldn't prune *gui-held*"
  }

  It 'is nothing in a tree with no target' {
    $r = Remove-StaleBuildFiles -Tree (Join-Path $TestDrive 'empty') -Now $now
    $r.Folders | Should -Be 0
    Format-StaleBuildFiles $r 'incremental' | Should -BeNullOrEmpty
  }
}
