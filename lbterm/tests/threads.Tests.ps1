# Pester 5+, run by `lbterm.ps1 check`. perf/threads.ps1, which at-rest.ps1 and
# conversation-cost.ps1 measure a test window's threads with: a snapshot of a process's threads
# (this test's own pwsh, never an LBTerm), and what a span between two snapshots says, on
# snapshots made up here.

BeforeAll {
  . (Join-Path $PSScriptRoot '..\perf\threads.ps1')

  # A snapshot of a made-up process: threads as (id, name, module, switches, cycles), and its I/O
  function New-Snap {
    param([object[]]$Threads, [long]$Reads = 0, [long]$Writes = 0, [long]$Others = 0, [ulong]$Cycles = 0)
    $s = [LbtermRest+Snap]::new()
    $s.Pid = 1234
    ($s.Reads, $s.Writes, $s.Others, $s.Cycles) = ($Reads, $Writes, $Others, $Cycles)
    foreach ($t in $Threads) {
      $th = [LbtermRest+Thread]::new()
      ($th.Id, $th.Name, $th.Module, $th.Switches, $th.Cycles) = ($t[0], $t[1], $t[2], $t[3], [ulong]$t[4])
      $s.Threads[$th.Id] = $th
    }
    $s
  }
}

Describe 'A snapshot of a process' {
  It 'has every thread of the process, with its switches and cycles, and the process''s I/O' {
    $s = [LbtermRest]::Take($PID)
    $s | Should -Not -BeNullOrEmpty
    $s.Pid | Should -Be $PID
    $s.Threads.Count | Should -BeGreaterThan 0
    # Every thread has a name: its description, or the module it started in
    @($s.Threads.Values | Where-Object { -not $_.Name }).Count | Should -Be 0
    @($s.Threads.Values | Where-Object { $_.Switches -gt 0 }).Count | Should -BeGreaterThan 0
    $s.Cycles | Should -BeGreaterThan 0
  }

  It 'is none for a process that isn''t there' {
    [LbtermRest]::Take(-7) | Should -BeNullOrEmpty
  }
}

Describe 'What a span cost' {
  It 'adds up each name''s wakes, counts threads that came and went, and the I/O' {
    $before = New-Snap @(
      @(1, 'main', 'lbterm-gui.exe', 100, 1000),
      @(2, 'lbterm-conversations', 'lbterm-gui.exe', 10, 500),
      @(3, '(nvwgf2umx.dll)', 'nvwgf2umx.dll', 1000, 9000),
      @(4, '(nvwgf2umx.dll)', 'nvwgf2umx.dll', 50, 100),
      @(5, 'lbterm-placement', 'lbterm-gui.exe', 3, 30)
    ) -Reads 10 -Writes 2 -Others 7 -Cycles 100000
    $after = New-Snap @(
      @(1, 'main', 'lbterm-gui.exe', 112, 1000),
      @(2, 'lbterm-conversations', 'lbterm-gui.exe', 10, 500),
      @(3, '(nvwgf2umx.dll)', 'nvwgf2umx.dll', 1300, 9000),
      @(4, '(nvwgf2umx.dll)', 'nvwgf2umx.dll', 80, 100),
      @(6, 'lbterm-prompts', 'lbterm-gui.exe', 4, 40)
    ) -Reads 13 -Writes 2 -Others 9 -Cycles 100000
    $span = Measure-Span $before $after 20
    $span.seconds | Should -Be 20
    $span.wakes | Should -Be (12 + 0 + 300 + 30 + 4)
    # LBTerm's own threads but the GUI thread: the one that came, the reading thread that slept
    $span.own_wakes | Should -Be 4
    ($span.reads, $span.writes, $span.others) | Should -Be @(3, 0, 2)
    ($span.started, $span.ended) | Should -Be @(1, 1)
    $nv = @($span.threads | Where-Object name -eq '(nvwgf2umx.dll)')
    $nv.Count | Should -Be 1
    ($nv[0].threads, $nv[0].wakes) | Should -Be @(2, 330)
    (@($span.threads | Where-Object name -eq 'lbterm-conversations')[0]).wakes | Should -Be 0
    # Busiest first
    $span.threads[0].name | Should -Be '(nvwgf2umx.dll)'
  }

  It 'names a thread by what it was called at the start, should it go before the end' {
    $before = New-Snap @(, @(9, 'lbterm-seat', 'lbterm-gui.exe', 5, 0))
    $after = New-Snap @(, @(9, '(gone)', '', 8, 0))
    $span = Measure-Span $before $after 1
    $span.threads[0].name | Should -Be 'lbterm-seat'
    $span.threads[0].wakes | Should -Be 3
  }

  It 'says what it measured, a thread to a line' {
    $before = New-Snap @(, @(1, 'main', 'lbterm-gui.exe', 1, 0))
    $after = New-Snap @(, @(1, 'main', 'lbterm-gui.exe', 6, 0)) -Reads 2
    $lines = @(Format-Span (Measure-Span $before $after 5) 'the GUI')
    $lines[0] | Should -BeLike 'the GUI: 5 wakes in 5 s (1/s)*I/O 2 reads 0 writes 0 other*'
    $lines[1] | Should -BeLike '*5 wakes*main'
  }
}
