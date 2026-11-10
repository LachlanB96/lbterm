# Pester 5+, run by `lbterm.ps1 check`. Cargo only inside a job (lbterm-lib.ps1): the refusal a
# PC's "job" setting makes outside one, the line it gives, and lbterm.ps1 refusing before anything
# runs. The settings are a file of the test's own (LBTERM_DEV), naming a variable nothing sets.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\lbterm-lib.ps1')
  $settings = [pscustomobject]@{
    job = [pscustomobject]@{
      env = 'LBTERM_TEST_NO_SUCH_JOB'
      run = 'queue-run --name lbterm-{command} -- pwsh -NoProfile -File {script} {command}{args}'
    }
  }
}

Describe 'Get-JobRefusal' {
  It 'asks nothing without the setting' {
    Get-JobRefusal -Settings $null -Command build -Script 'C:\x\lbterm.ps1' | Should -BeNullOrEmpty
    Get-JobRefusal -Settings ([pscustomobject]@{ config = 'x' }) -Command build -Script 'C:\x\lbterm.ps1' | Should -BeNullOrEmpty
  }

  It 'lets it run in a job, its variable set' {
    Get-JobRefusal -Settings $settings -Command build -Script 'C:\x\lbterm.ps1' -Value '20261101-1' -InJob $true | Should -BeNullOrEmpty
  }

  It 'refuses outside one, with the line that starts it in one' {
    $r = Get-JobRefusal -Settings $settings -Command test -Script 'C:\x\lbterm\lbterm.ps1' -Arguments ' -Package lbterm-extras' -InJob $true
    $r | Should -Match "LBTERM_TEST_NO_SUCH_JOB isn't set"
    $r | Should -Match ([regex]::Escape('queue-run --name lbterm-test -- pwsh -NoProfile -File C:/x/lbterm/lbterm.ps1 test -Package lbterm-extras'))
  }

  It 'refuses the test-window runs outside one too, saying why' {
    $r = Get-JobRefusal -Settings $settings -Command e2e -Script 'C:\x\lbterm\lbterm.ps1' -Arguments ' -Only mux'
    $r | Should -Match 'opens test windows'
    $r | Should -Match ([regex]::Escape('queue-run --name lbterm-e2e -- pwsh -NoProfile -File C:/x/lbterm/lbterm.ps1 e2e -Only mux'))
    # with the warden's own line, a GPU job that only draws, and a measuring run on the measurements' lock
    $w = [pscustomobject]@{ job = [pscustomobject]@{ env = 'LBTERM_TEST_NO_SUCH_JOB'; run = 'warden-run --name lbterm-{command} --cpus 4 -- pwsh -File {script} {command}{args}' } }
    Get-JobRefusal -Settings $w -Command budgets -Script 'C:\x\lbterm.ps1' | Should -Match ([regex]::Escape('warden-run --gpu-draw --lock lbterm-measure --name lbterm-budgets --cpus 4'))
    Get-JobRefusal -Settings $w -Command e2e -Script 'C:\x\lbterm.ps1' | Should -Match ([regex]::Escape('warden-run --gpu-draw --name lbterm-e2e --cpus 4'))
    Get-JobRefusal -Settings $w -Command e2e -Script 'C:\x\lbterm.ps1' | Should -Not -Match -- '--lock'
    Get-JobRefusal -Settings $w -Command look -Script 'C:\x\lbterm.ps1' | Should -Match ([regex]::Escape('warden-run --gpu-draw --name lbterm-look --cpus 4'))
    Get-JobRefusal -Settings $w -Command look -Script 'C:\x\lbterm.ps1' | Should -Not -Match -- '--lock'
    Get-JobRefusal -Settings $w -Command build -Script 'C:\x\lbterm.ps1' | Should -Not -Match -- '--gpu'
    (Get-JobRefusal -Settings $settings -Command budgets -Script 'C:\x\lbterm.ps1') | Should -Match 'opens test windows'
  }

  It 'refuses a process that left its job: the variable kept, no job' {
    $r = Get-JobRefusal -Settings $settings -Command check -Script 'C:\x\lbterm.ps1' -Value '20261101-1' -InJob $false
    $r | Should -Match "is set but this process isn't in a job"
  }
}

Describe 'Format-LbtermArguments' {
  It 'gives the parameters after the command as they would be typed again' {
    $bound = [ordered]@{ Command = 'test'; Package = @('lbterm-extras', 'config'); NoSccache = [switch]$true; Jobs = 2; Latest = [switch]$false }
    Format-LbtermArguments $bound | Should -Be ' -Package lbterm-extras,config -NoSccache -Jobs 2'
    Format-LbtermArguments ([ordered]@{ Command = 'build' }) | Should -Be ''
  }
}

Describe 'Get-DevSettings' {
  It 'reads the file, or nothing without one' {
    Get-DevSettings (Join-Path $TestDrive 'none.json') | Should -BeNullOrEmpty
    $f = Join-Path $TestDrive 'dev.json'
    [IO.File]::WriteAllText($f, '{ "job": { "env": "X", "run": "y" } }')
    (Get-DevSettings $f).job.env | Should -Be 'X'
  }

  It 'says whether this process is in a job object' {
    Test-InJobObject | Should -BeOfType [bool]
  }
}

Describe 'lbterm.ps1 outside a job' {
  It 'refuses build before anything runs, with the line to use, and leaves the rest alone' {
    $f = Join-Path $TestDrive 'dev-guard.json'
    $settings | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $f
    $script = Join-Path $PSScriptRoot '..\lbterm.ps1'
    $err = Join-Path $TestDrive 'err.txt'
    $saved = $env:LBTERM_DEV
    try {
      $env:LBTERM_DEV = $f
      $out = & 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File $script build -NoSccache 2>$err
      $LASTEXITCODE | Should -Be 2
      $said = Get-Content -Raw $err
      $said | Should -Match "LBTERM_TEST_NO_SUCH_JOB isn't set"
      $said | Should -Match 'queue-run --name lbterm-build -- pwsh -NoProfile -File \S+/lbterm/lbterm.ps1 build -NoSccache'
      $out | Should -BeNullOrEmpty
    } finally {
      $env:LBTERM_DEV = $saved
    }
  }
}

Describe 'Get-RunRefusal' {
  It 'refuses a whole set unless -AllScripts says so, and names how to pick' {
    $e2e = Get-RunRefusal -Command e2e
    $e2e | Should -Match '^lbterm\.ps1 e2e runs no whole set: name the scripts with -Only, or run -Smoke'
    $e2e | Should -Match 'e2e -Changed lists'
    $e2e | Should -Match '-AllScripts, only when Lachlan asks'
    Get-RunRefusal -Command budgets | Should -Match '^lbterm\.ps1 budgets runs no whole set: name the scenarios with -Only'
    Get-RunRefusal -Command e2e -Only @('') | Should -Match 'runs no whole set'
    Get-RunRefusal -Command e2e -Only @('inbox', 'menu-keys') | Should -BeNullOrEmpty
    Get-RunRefusal -Command budgets -AllScripts | Should -BeNullOrEmpty
  }

  It 'wants named scripts and the build before for look' {
    Get-RunRefusal -Command look -Against 'C:/b' | Should -Match '^lbterm\.ps1 look shows named scripts only'
    Get-RunRefusal -Command look -Only menu-keys | Should -Match '^lbterm\.ps1 look needs the build before the change: -Against'
    Get-RunRefusal -Command look -Only menu-keys -Against 'C:/b' | Should -BeNullOrEmpty
    Get-RunRefusal -Command look -Only menu-keys -Against 'C:/b' -Asleep 'the displays are asleep' | Should -Match '^lbterm\.ps1 look waits for the displays'
  }

  It 'lets e2e -Smoke run with no names, and budgets not' {
    Get-RunRefusal -Command e2e -Smoke | Should -BeNullOrEmpty
    Get-RunRefusal -Command e2e -Smoke -Only inbox | Should -BeNullOrEmpty
    Get-RunRefusal -Command budgets -Smoke | Should -Match 'runs no whole set'
    Get-RunRefusal -Command e2e -Smoke -Asleep 'the displays are asleep' | Should -Match '^lbterm\.ps1 e2e waits for the displays'
  }

  It 'waits while the displays are asleep, named scripts or not' {
    $asleep = 'the displays are asleep: no input for 4000 s, and the power plan turns them off after 3600 s'
    Get-RunRefusal -Command e2e -Only inbox -Asleep $asleep | Should -Be "lbterm.ps1 e2e waits for the displays: $asleep. Nothing it measures or captures can be judged until they wake (PITFALLS 239)"
    Get-RunRefusal -Command budgets -AllScripts -Asleep $asleep | Should -Match '^lbterm\.ps1 budgets waits for the displays'
  }
}
