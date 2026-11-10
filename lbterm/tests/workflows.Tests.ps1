# Pester 5+, run by `lbterm.ps1 check`. The tests in .claude/tests, on Bun: the saved workflow
# lbterm-track.js (the one copy of the track rules LBTerm's workflows run) and the tools beside it.
# .claude stays out of the public export, so there they're skipped, and Pester counts them as
# skipped.

BeforeDiscovery {
  $script:hasTests = Test-Path (Join-Path $PSScriptRoot '..\..\.claude\tests\lbterm-track.test.ts')
}

Describe 'Saved workflows and their tools' {
  It 'pass their tests' -Skip:(-not $hasTests) {
    $tests = Join-Path $PSScriptRoot '..\..\.claude\tests'
    $bun = (Get-Command bun -ErrorAction Stop).Source
    Push-Location $tests
    try { $out = & $bun test 2>&1 } finally { Pop-Location }
    $LASTEXITCODE | Should -Be 0 -Because ($out -join "`n")
    # bun test exits 0 having found no tests at all; there are at least the two files
    ($out -join "`n") -match 'across (\d+) files?' | Should -BeTrue
    [int]$Matches[1] | Should -BeGreaterOrEqual 2
  }
}
