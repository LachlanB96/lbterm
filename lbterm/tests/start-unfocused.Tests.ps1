# Pester 5+, run by `lbterm.ps1 check`. Start-Unfocused's -Job (motion\start-unfocused.ps1): a test
# window handed to the job the test runs in before it runs, from the PC's dev settings' "job".
# The settings are a file of each test's own (LBTERM_DEV), the job's variable one nothing else
# sets, and the handoff a stand-in that records what it saw and exits as told. The processes are
# cmd.exe /c exit 7, detached (no console, nothing started under it), so each one that ran says so
# by its exit code.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\motion\start-unfocused.ps1')
  $script:dir = Join-Path ([IO.Path]::GetTempPath()) "lbterm-start-unfocused-$PID"
  New-Item -ItemType Directory -Force $dir | Out-Null
  $script:pwsh = 'C:\Program Files\PowerShell\7\pwsh.exe'
  $script:cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
  $script:var = "LBTERM_TEST_JOB_$PID"
  # The handoff's stand-in: says whether every thread of the process was suspended when it was
  # asked, then exits with the code in its file (0 taken, else not)
  $script:stub = Join-Path $dir 'adopt.ps1'
  [IO.File]::WriteAllText($stub, @'
param([int]$Id, [string]$Out, [int]$Code)
$p = Get-Process -Id $Id -ErrorAction SilentlyContinue
$held = $p -and @($p.Threads | Where-Object { $_.ThreadState -ne 'Wait' -or $_.WaitReason -ne 'Suspended' }).Count -eq 0
[IO.File]::WriteAllText($Out, "$Id $held")
if ($Code) { [Console]::Error.WriteLine("[stand-in] pid $Id isn't this job's") }
exit $Code
'@)
  # Settings naming the stand-in with exit $Code; their path, set as LBTERM_DEV
  function Use-Settings([int]$Code, [switch]$NoAdopt) {
    $out = Join-Path $dir "adopt-$Code.txt"
    Remove-Item -LiteralPath $out -ErrorAction SilentlyContinue
    $job = [ordered]@{ env = $var; run = 'queue-run -- {script}' }
    if (-not $NoAdopt) { $job.adopt = @($pwsh, '-NoProfile', '-File', $stub, '{pid}', $out, "$Code") }
    $file = Join-Path $dir 'dev.json'
    [IO.File]::WriteAllText($file, (@{ job = $job } | ConvertTo-Json -Depth 4))
    $env:LBTERM_DEV = $file
    $out
  }
  # A detached cmd.exe /c exit 7, started with $More's switches
  function Start-Seven([hashtable]$More = @{}) {
    Start-Unfocused -Exe $cmd -Arguments '/c exit 7' -NoConsole @More
  }
}

AfterAll {
  Remove-Item Env:LBTERM_DEV -ErrorAction SilentlyContinue
  Remove-Item "Env:$var" -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
}

Describe 'Get-JobHandoff' {
  AfterEach { Remove-Item "Env:$var" -ErrorAction SilentlyContinue }

  It 'is nothing without the file, a job or an adopt command' {
    Get-JobHandoff -File (Join-Path $dir 'no-such.json') | Should -BeNullOrEmpty
    $file = Join-Path $dir 'plain.json'
    [IO.File]::WriteAllText($file, '{ "config": "x" }')
    Get-JobHandoff -File $file | Should -BeNullOrEmpty
    Use-Settings 0 -NoAdopt | Out-Null
    Get-JobHandoff | Should -BeNullOrEmpty
  }

  It 'refuses outside a job, naming its variable' {
    Use-Settings 0 | Out-Null
    { Get-JobHandoff } | Should -Throw "*only inside a job, and $var isn't set*"
  }

  It 'gives the command inside one' {
    Use-Settings 0 | Out-Null
    Set-Item "Env:$var" 'job-1'
    $h = Get-JobHandoff
    $h[0] | Should -Be $pwsh
    $h | Should -Contain '{pid}'
  }
}

Describe 'Start-Unfocused -Job' {
  AfterEach { Remove-Item "Env:$var" -ErrorAction SilentlyContinue }

  It 'refuses outside a job before it starts anything' {
    $out = Use-Settings 0
    { Start-Seven @{ Job = $true } } | Should -Throw '*only inside a job*'
    Test-Path $out | Should -BeFalse
  }

  It 'hands the process over while it is suspended, and lets it run once it is taken' {
    $out = Use-Settings 0
    Set-Item "Env:$var" 'job-1'
    $s = Start-Seven @{ Job = $true; PassThru = $true }
    $id, $held = (Get-Content -Raw $out).Trim() -split ' '
    [int]$id | Should -Be $s.Id
    $held | Should -Be 'True'
    $s.Ran | Should -BeOfType [datetime]
    # Let run: it ends by itself (one left suspended never would)
    $p = Get-Process -Id $s.Id -ErrorAction SilentlyContinue
    if ($p) { $p.WaitForExit(10000) | Should -BeTrue }
  }

  It 'with -Held, hands it over and leaves it suspended for the caller, which it then runs for' {
    $out = Use-Settings 0
    Set-Item "Env:$var" 'job-1'
    $id = Start-Seven @{ Job = $true; Held = $true }
    $p = [Diagnostics.Process]::GetProcessById($id)
    $null = $p.Handle
    try {
      (Get-Content -Raw $out).Trim() | Should -Be "$id True"
      $p.HasExited | Should -BeFalse
      Resume-StartedProcess $id | Out-Null
      $p.WaitForExit(10000) | Should -BeTrue
      # Its own exit code: it ran to its end, not ended
      $p.ExitCode | Should -Be 7
    } finally { if (-not $p.HasExited) { $p.Kill() } }
  }

  It 'ends one the job will not take, before it ran, and says why' {
    $out = Use-Settings 1
    Set-Item "Env:$var" 'job-1'
    { Start-Seven @{ Job = $true } } | Should -Throw "*the job didn't take*ended before it ran*isn't this job's*"
    $id, $held = (Get-Content -Raw $out).Trim() -split ' '
    $held | Should -Be 'True'
    # Ended: gone at once
    Get-Process -Id ([int]$id) -ErrorAction SilentlyContinue | Should -BeNullOrEmpty
  }

  It 'changes nothing on a PC without an adopt command' {
    Use-Settings 0 -NoAdopt | Out-Null
    $s = Start-Seven @{ Job = $true; PassThru = $true }
    $s.Id | Should -BeGreaterThan 0
    Test-Path (Join-Path $dir 'adopt-0.txt') | Should -BeFalse
  }
}

Describe 'Test windows join their job' {
  It 'every Start-Unfocused in the harness, perf and motion scripts asks -Job' {
    # Not bench's: it times rival terminals' starts, each the same way, and Windows Terminal's
    # window comes from a process of its own that no job would hold. Nor lbterm.ps1's sccache
    # server and try window, which outlive the job on purpose.
    $lbterm = Join-Path $PSScriptRoot '..'
    $files = @(Get-Item (Join-Path $lbterm 'harness.psm1')) +
      @(Get-ChildItem (Join-Path $lbterm 'perf'), (Join-Path $lbterm 'motion') -Filter *.ps1 | Where-Object Name -ne 'start-unfocused.ps1')
    $calls = 0
    $without = foreach ($f in $files) {
      $ast = [System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$null, [ref]$null)
      foreach ($c in $ast.FindAll({ param($a) $a -is [System.Management.Automation.Language.CommandAst] -and $a.GetCommandName() -eq 'Start-Unfocused' }, $true)) {
        $calls++
        $job = $c.CommandElements | Where-Object { $_ -is [System.Management.Automation.Language.CommandParameterAst] -and $_.ParameterName -eq 'Job' }
        # Splatted: the hashtable it's given asks it
        $splat = $c.CommandElements | Where-Object { $_ -is [System.Management.Automation.Language.VariableExpressionAst] -and $_.Splatted }
        if (-not $job -and -not ($splat -and (Get-Content -Raw $f.FullName) -match '\bJob = \$true\b')) { "$($f.Name):$($c.Extent.StartLineNumber)" }
      }
    }
    $calls | Should -BeGreaterThan 3
    @($without) | Should -BeNullOrEmpty
  }
}

Describe 'Start-Unfocused -Held and Resume-StartedProcess' {
  BeforeEach { Use-Settings 0 -NoAdopt | Out-Null }

  It 'leaves it suspended until it is resumed, and it then runs to its own end' {
    $id = Start-Seven @{ Held = $true }
    $p = [Diagnostics.Process]::GetProcessById($id)
    $null = $p.Handle
    try {
      Start-Sleep -Milliseconds 300
      $p.HasExited | Should -BeFalse
      @($p.Threads | Where-Object { $_.WaitReason -ne 'Suspended' }).Count | Should -Be 0
      $ran = Resume-StartedProcess $id
      $ran | Should -BeOfType [datetime]
      $p.WaitForExit(10000) | Should -BeTrue
      $p.ExitCode | Should -Be 7
    } finally { if (-not $p.HasExited) { $p.Kill() } }
  }

  It 'says so when there is nothing to resume' {
    { Resume-StartedProcess 0 } | Should -Throw '*couldn''t be resumed*'
  }
}
