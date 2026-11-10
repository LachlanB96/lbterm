# Pester 5+, run by `lbterm.ps1 check`. budgets.psm1's parts that need no window (lbterm/PERF.md,
# "The budget runner"): reading budgets.json, a span's numbers from two snapshots and a metrics
# reply, judging a scenario, the report, its table and the history line, and what a scenario's run
# left in its folder. Nothing here opens a window.

BeforeAll {
  Import-Module (Join-Path $PSScriptRoot '..\budgets.psm1') -Force -DisableNameChecking
  $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $real = Join-Path $repo 'lbterm\perf\budgets.json'

  # A budgets file with the rest section given (or a plain one) and these scenarios
  function New-BudgetsFile([string]$Dir, [object[]]$Scenarios, [hashtable]$Rest, [int]$Version = 1) {
    New-Item -ItemType Directory -Force $Dir | Out-Null
    if (-not $Rest) { $Rest = @{ seconds = 5; runs = 3; settle_ms = 2000; budgets = @{ frames = @{ max = 0.2; why = 'nothing paints' }; cpu_ms = @{ max = 5 } } } }
    $path = Join-Path $Dir 'budgets.json'
    [IO.File]::WriteAllText($path, (@{ version = $Version; rest = $Rest; scenarios = @($Scenarios) } | ConvertTo-Json -Depth 8))
    $path
  }

  # Two snapshots as Get-CpuSnapshot makes them, -Seconds apart at a clock of 1000 ticks a second
  function New-Snapshots {
    param([double]$Seconds = 5, [long]$Cycles = 0, [long]$ThreadCycles = 0, [long]$StateCycles = 0, [object[]]$ServersBefore = @(), [object[]]$ServersAfter = @(), [object[]]$Conpty = @())
    $before = [pscustomobject]@{
      At = 1000; Cycles = 1000000; ThreadCycles = 500000; CpuMs = 100; ThreadMs = 50; PrivateBytes = 400MB; Handles = 800; Threads = 40
      ThreadList = @([pscustomobject]@{ Tid = 1; Name = 'main'; Cycles = 500000 }, [pscustomobject]@{ Tid = 2; Name = 'lbterm-state'; Cycles = 10 }, [pscustomobject]@{ Tid = 3; Name = ''; Cycles = 7 })
      Servers = @($ServersBefore); Conpty = @($Conpty | ForEach-Object { [pscustomobject]@{ Pid = $_.Pid; Cycles = $_.Before } })
    }
    $after = [pscustomobject]@{
      At = 1000 + 1000 * $Seconds; Cycles = 1000000 + $Cycles; ThreadCycles = 500000 + $ThreadCycles; CpuMs = 115.625; ThreadMs = 50; PrivateBytes = 401MB; Handles = 801; Threads = 40
      ThreadList = @([pscustomobject]@{ Tid = 1; Name = 'main'; Cycles = 500000 + $ThreadCycles }, [pscustomobject]@{ Tid = 2; Name = 'lbterm-state'; Cycles = 10 + $StateCycles }, [pscustomobject]@{ Tid = 3; Name = ''; Cycles = 7 })
      Servers = @($ServersAfter); Conpty = @($Conpty | ForEach-Object { [pscustomobject]@{ Pid = $_.Pid; Cycles = $_.After } })
    }
    $before, $after
  }

  # A metrics reply as the control pipe gives it, its counts given, over -SpanMs
  function New-Metrics([int]$Requests = 0, [int]$Timers = 0, [int]$Tasks = 0, [double]$TaskMs = 0, [int]$Messages = 0, [int]$Wakes = 0, [double]$SpanMs = 5000) {
    [pscustomobject]@{
      span_ms = $SpanMs
      totals = [pscustomobject]@{ requests = $Requests; timers_fired = $Timers; tasks = $Tasks; task_ms = $TaskMs; messages = $Messages }
      gui = [pscustomobject]@{ wakes = $Wakes }
      requests = @([pscustomobject]@{ site = 'render/conductor_view.rs:338'; frames = 300; requests = 310 }, [pscustomobject]@{ site = 'render/lights.rs:120'; frames = 12; requests = 12 })
      tasks = @([pscustomobject]@{ site = 'window.rs:1819'; count = 600; total_ms = 9.5 })
      timers = @([pscustomobject]@{ site = 'window.rs:1821'; fired = 300 })
      messages = @([pscustomobject]@{ site = 'WM_PAINT'; count = 300; total_ms = 1 })
      threads = @([pscustomobject]@{ tid = 1; name = 'main'; cpu_ms = 4000 })
      budget = [pscustomobject]@{ at_rest = [pscustomobject]@{ frames = 290; timers = 300; tasks = 600; task_ms = 9.5 }; lines = 2; on_purpose = @() }
    }
  }

  # A scenario as Read-Budgets gives one
  function New-Scenario([hashtable]$Budgets = @{ frames = @{ max = 0.2 }; gui_thread_ms = @{ max = 2 }; cpu_ms = @{ max = 5 } }, [string]$Bug) {
    $b = [ordered]@{}
    foreach ($k in $Budgets.Keys) { $b[$k] = @{ max = [double]$Budgets[$k].max; why = $Budgets[$k].why } }
    [pscustomobject]@{ Name = 'conductor-view'; Kind = 'rest'; Domain = 'mux'; What = 'the view open'; Bug = $Bug; Seconds = 5; Budgets = $b }
  }
}

Describe 'budgets.json' {
  It 'reads the repo''s, every scenario with a setup in budgets.ps1: at rest a budget for frames and the GUI thread, an action for its time, its 95th percentile and its GUI thread' {
    $b = Read-Budgets $real
    $b.Runs | Should -BeGreaterThan 0
    @($b.Scenarios | Where-Object Kind -eq 'rest').Count | Should -BeGreaterThan 20
    @($b.Scenarios | Where-Object Kind -eq 'action').Count | Should -BeGreaterThan 15
    $script = Get-Content -Raw (Join-Path $repo 'lbterm\perf\budgets.ps1')
    foreach ($s in $b.Scenarios) {
      if ($s.Kind -eq 'action') {
        $script | Should -Match ([regex]::Escape("`$Actions['$($s.Name)']")) -Because "budgets.json names the action $($s.Name)"
        foreach ($m in 'ms', 'p95_ms', 'gui_thread_ms') { $s.Budgets.Contains($m) | Should -BeTrue -Because "$($s.Name) wants a budget for $m" }
        # A first time measured alone (picker-first: one cold open a window, no warm-up) has one
        # run, its rounds' windows the runs; every other action more than one
        if ($s.Name -like '*-first') { ($s.Runs, $s.Warmup) | Should -Be @(1, 0) -Because "$($s.Name) measures a first time alone" }
        else { $s.Runs | Should -BeGreaterThan 1 }
      } else {
        $script | Should -Match ([regex]::Escape("`$Setups['$($s.Name)']")) -Because "budgets.json names $($s.Name)"
        $s.Budgets.Contains('frames') | Should -BeTrue
        $s.Budgets.Contains('gui_thread_ms') | Should -BeTrue
      }
    }
    # The throughputs are held to a least, not a most
    foreach ($s in @($b.Scenarios | Where-Object Name -like 'throughput*')) { $s.Budgets.mb_per_s.min | Should -BeGreaterThan 0 }
  }

  It 'reads an action''s runs, warm-ups, GUI action and budgets, a min among them, its kind''s defaults under its own' {
    $dir = Join-Path $TestDrive 'actions'
    New-Item -ItemType Directory -Force $dir | Out-Null
    $path = Join-Path $dir 'budgets.json'
    [IO.File]::WriteAllText($path, (@{
          version = 1
          rest = @{ seconds = 5; runs = 3; settle_ms = 2000; budgets = @{ frames = @{ max = 0.2 } } }
          actions = @{ runs = 12; warmup = 2; budgets = @{ frames = @{ max = 10; why = 'any action' } } }
          scenarios = @(
            @{ name = 'key-echo'; kind = 'action'; gui = 'key'; what = 'a key'; budgets = @{ ms = @{ max = 30; why = 'its echo' } } }
            @{ name = 'throughput'; kind = 'action'; runs = 4; warmup = 0; what = 'a writer'; budgets = @{ mb_per_s = @{ min = 20 } } }
            @{ name = 'one-tab'; what = 'at rest' }
          )
        } | ConvertTo-Json -Depth 8))
    $b = Read-Budgets $path
    $key, $tp, $rest = $b.Scenarios
    $key.Kind | Should -Be 'action'
    $key.Gui | Should -Be 'key'
    $key.Runs | Should -Be 12
    $key.Warmup | Should -Be 2
    $key.Budgets.ms.max | Should -Be 30
    $key.Budgets.frames.why | Should -Be 'any action'
    $key.Budgets.Contains('cpu_ms') | Should -BeFalse
    $tp.Runs | Should -Be 4
    $tp.Warmup | Should -Be 0
    $tp.Budgets.mb_per_s.min | Should -Be 20
    $tp.Budgets.mb_per_s.max | Should -BeNullOrEmpty
    $tp.Gui | Should -BeNullOrEmpty
    $rest.Kind | Should -Be 'rest'
    $rest.Budgets.Contains('ms') | Should -BeFalse
    $b.ActionRuns | Should -Be 12
  }

  It 'refuses an action it can''t hold a run to' {
    $dir = Join-Path $TestDrive 'badactions'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; kind = 'action'; what = 'x'; budgets = @{ fps = @{ max = 1 } } })) } | Should -Throw '*a.budgets.fps isn''t something an action measures*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; kind = 'action'; what = 'x'; budgets = @{ ms = @{ max = 1; min = 0 } } })) } | Should -Throw '*both a max and a min*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; kind = 'action'; what = 'x'; gui = 'scroll' })) } | Should -Throw '*gui ''scroll'' isn''t an action the GUI times*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x'; gui = 'key' })) } | Should -Throw '*only an action scenario has one*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; kind = 'action'; what = 'x'; runs = 0 })) } | Should -Throw '*runs is ''0''*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'action'; what = 'x' })) } | Should -Throw '*a kind''s name*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x'; budgets = @{ ms = @{ max = 1 } } })) } | Should -Throw '*a.budgets.ms isn''t something a span measures*'
  }

  It 'lays a scenario''s own budgets over the defaults, and its domain is mux unless it says' {
    $path = New-BudgetsFile (Join-Path $TestDrive 'merge') @(
      @{ name = 'blink'; what = 'a blinking cursor'; budgets = @{ frames = @{ max = 2; why = 'its blink' } } }
      @{ name = 'plain'; what = 'one tab'; domain = 'local'; bug = 'a known one' }
    )
    $b = Read-Budgets $path
    $blink, $plain = $b.Scenarios
    $blink.Budgets.frames.max | Should -Be 2
    $blink.Budgets.frames.why | Should -Be 'its blink'
    $blink.Budgets.cpu_ms.max | Should -Be 5
    $blink.Domain | Should -Be 'mux'
    $blink.Seconds | Should -Be 5
    $plain.Budgets.frames.max | Should -Be 0.2
    $plain.Domain | Should -Be 'local'
    $plain.Bug | Should -Be 'a known one'
    @($blink.Budgets.Keys) | Should -Be @('frames', 'cpu_ms')
  }

  It 'gives a busy scenario the busy section''s budgets over the rest''s, and its own over those' {
    $dir = Join-Path $TestDrive 'busy'
    New-Item -ItemType Directory -Force $dir | Out-Null
    $path = Join-Path $dir 'budgets.json'
    [IO.File]::WriteAllText($path, (@{
          version = 1
          rest = @{ seconds = 5; runs = 3; settle_ms = 2000; budgets = @{ frames = @{ max = 0.2 }; cpu_ms = @{ max = 5 }; timers = @{ max = 0.5 } } }
          busy = @{ budgets = @{ frames = @{ max = 30; why = 'a line every 50 ms' }; cpu_ms = @{ max = 200 } } }
          scenarios = @(
            @{ name = 'ticker'; busy = $true; what = 'a line every 50 ms' }
            @{ name = 'ticker-card'; busy = $true; what = 'the same, a card open'; budgets = @{ words = @{ max = 1 }; frame_ms = @{ max = 4 } } }
            @{ name = 'one-tab'; what = 'at rest' }
          )
        } | ConvertTo-Json -Depth 8))
    $b = Read-Budgets $path
    $ticker, $card, $rest = $b.Scenarios
    $ticker.Busy | Should -BeTrue
    $ticker.Budgets.frames.max | Should -Be 30
    $ticker.Budgets.frames.why | Should -Be 'a line every 50 ms'
    $ticker.Budgets.cpu_ms.max | Should -Be 200
    $ticker.Budgets.timers.max | Should -Be 0.5 -Because 'what the busy section doesn''t say is the rest''s'
    $card.Budgets.words.max | Should -Be 1
    $card.Budgets.frame_ms.max | Should -Be 4
    $card.Budgets.frames.max | Should -Be 30
    $rest.Busy | Should -BeFalse
    $rest.Budgets.frames.max | Should -Be 0.2
    $rest.Budgets.Contains('words') | Should -BeFalse
    { Read-Budgets (New-BudgetsFile (Join-Path $TestDrive 'nobusy') @(@{ name = 'a'; busy = $true; what = 'x' })) } | Should -Throw '*no busy section*'
    { Read-Budgets (New-BudgetsFile (Join-Path $TestDrive 'busyaction') @(@{ name = 'a'; kind = 'action'; busy = $true; what = 'x' })) } | Should -Throw '*only a scenario at rest*'
  }

  It 'refuses a file it can''t hold a run to, saying what''s wrong' {
    $dir = Join-Path $TestDrive 'bad'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x'; budgets = @{ fps = @{ max = 1 } } })) } | Should -Throw '*a.budgets.fps isn''t something a span measures*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x'; budgets = @{ frames = @{ why = 'no max' } } })) } | Should -Throw '*a.budgets.frames has no max*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x' }, @{ name = 'a'; what = 'y' })) } | Should -Throw '*two scenarios named a*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x'; domain = 'ssh' })) } | Should -Throw '*domain ''ssh''*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a'; what = 'x'; kind = 'throughput' })) } | Should -Throw '*kind ''throughput''*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'A b'; what = 'x' })) } | Should -Throw '*isn''t lower-case words*'
    { Read-Budgets (New-BudgetsFile $dir @(@{ name = 'a' })) } | Should -Throw '*says nothing of what it sets up*'
    { Read-Budgets (New-BudgetsFile $dir @() -Version 2) } | Should -Throw '*version 2*'
    { Read-Budgets (New-BudgetsFile $dir @() -Rest @{ seconds = 0; runs = 3; settle_ms = 1 }) } | Should -Throw '*rest.seconds*'
    { Read-Budgets (Join-Path $dir 'none.json') } | Should -Throw '*no budgets file*'
  }

  It 'picks scenarios by name or wildcard, comma-separated or not, in the file''s order, and throws for one none matches' {
    $s = @('one-tab', 'one-tab-local', 'rail-open', 'conductor-view') | ForEach-Object { [pscustomobject]@{ Name = $_ } }
    @(Select-BudgetScenarios $s @('rail-open,one-tab*')).Name | Should -Be @('one-tab', 'one-tab-local', 'rail-open')
    @(Select-BudgetScenarios $s @('conductor-view', 'one-tab')).Name | Should -Be @('one-tab', 'conductor-view')
    @(Select-BudgetScenarios $s @()).Count | Should -Be 4
    { Select-BudgetScenarios $s @('nope') } | Should -Throw '*no budget scenario named nope*'
  }

  It 'picks every scenario of a kind by its name' {
    $s = @(@{ n = 'one-tab'; k = 'rest' }, @{ n = 'key-echo'; k = 'action' }, @{ n = 'split'; k = 'action' }) | ForEach-Object { [pscustomobject]@{ Name = $_.n; Kind = $_.k } }
    @(Select-BudgetScenarios $s @('action')).Name | Should -Be @('key-echo', 'split')
    @(Select-BudgetScenarios $s @('rest,split')).Name | Should -Be @('one-tab', 'split')
  }
}

Describe 'A span''s numbers' {
  It 'takes medians and nearest-rank percentiles' {
    Get-Median @(3, 1, 2) | Should -Be 2
    Get-Median @(4, 1, 3, 2) | Should -Be 2.5
    Get-Median @() | Should -BeNullOrEmpty
    Get-Percentile @(1..20) 95 | Should -Be 19
    Get-Percentile @(5) 95 | Should -Be 5
  }

  It 'adds up a list of processes'' cycles, those alive at both ends only' {
    $before = @([pscustomobject]@{ Pid = 1; Cycles = 100 }, [pscustomobject]@{ Pid = 2; Cycles = 50 }, [pscustomobject]@{ Pid = 3; Cycles = 7 })
    $after = @([pscustomobject]@{ Pid = 1; Cycles = 400 }, [pscustomobject]@{ Pid = 2; Cycles = -1 }, [pscustomobject]@{ Pid = 4; Cycles = 999 })
    Get-CycleDelta $before $after | Should -Be 300
    Get-CycleDelta @() @() | Should -BeNullOrEmpty
  }

  It 'makes a second of each from two snapshots, the state file''s frames and a metrics reply' {
    # 15 ms of the GUI thread and 30 ms of the process, of which 6 ms the state file's writer, in 5 s, at 3000 cycles a ms
    $b, $a = New-Snapshots -Seconds 5 -Cycles 90000 -ThreadCycles 45000 -StateCycles 18000 `
      -ServersBefore @([pscustomobject]@{ Pid = 9; Cycles = 0 }) -ServersAfter @([pscustomobject]@{ Pid = 9; Cycles = 30000 }) `
      -Conpty @(@{ Pid = 7; Before = 0; After = 15000 })
    $m = New-Metrics -Requests 10 -Timers 5 -Tasks 20 -TaskMs 4 -Messages 50 -Wakes 25 -SpanMs 5000
    $s = ConvertTo-RestSample -Before $b -After $a -Frames 3 -Metrics $m -CycleRate 3000 -Frequency 1000 -Loops 2
    $s.seconds | Should -Be 5
    $s.loops | Should -Be 2
    $s.frames | Should -Be 0.6
    $s.requests | Should -Be 2
    $s.timers | Should -Be 1
    $s.tasks | Should -Be 4
    $s.task_ms | Should -Be 0.8
    $s.messages | Should -Be 10
    $s.wakes | Should -Be 5
    $s.gui_thread_ms | Should -Be 3
    $s.cpu_ms | Should -Be 4.8
    $s.state_ms | Should -Be 1.2
    $s.server_cpu_ms | Should -Be 2
    $s.conpty_cpu_ms | Should -Be 1
    $s.private_mb | Should -Be 401
    $s.handles | Should -Be 801
    $s.ticks.cpu_ms | Should -Be 3.125
    $s.at_rest.frames | Should -Be 290
    $s.top.requests[0].site | Should -Be 'render/conductor_view.rs:338'
    @($s.threads.name) | Should -Be @('main', 'lbterm-state')
    $s.threads[0].ms | Should -Be 3
  }

  It 'says what a frame costs, its paint pass and the words laid out, and nothing of a frame for a span that painted none' {
    # 60 ms of the GUI thread in 5 s for 20 frames: 3 ms a frame
    $b, $a = New-Snapshots -Seconds 5 -ThreadCycles 180000
    $m = New-Metrics -SpanMs 5000
    $m | Add-Member paints @(
      [pscustomobject]@{ site = 'window-0/pass'; count = 16; total_ms = 24 }
      [pscustomobject]@{ site = 'window-0/draw'; count = 16; total_ms = 99 }
      [pscustomobject]@{ site = 'window-1/pass'; count = 4; total_ms = 4 }
    )
    $m | Add-Member words ([pscustomobject]@{ laid_out = 35; kept = 4000 })
    $s = ConvertTo-RestSample -Before $b -After $a -Frames 20 -Metrics $m -CycleRate 3000 -Frequency 1000
    $s.frame_ms | Should -Be 3
    $s.pass_ms | Should -Be 1.4 -Because 'every window''s passes, 28 ms over 20, and none of the draws'
    $s.words | Should -Be 7
    $none = ConvertTo-RestSample -Before $b -After $a -Frames 0 -Metrics $null -CycleRate 3000 -Frequency 1000
    $none.frame_ms | Should -BeNullOrEmpty
    $none.pass_ms | Should -BeNullOrEmpty
    $none.words | Should -BeNullOrEmpty -Because 'a build without metrics (or without its words) says none'
  }

  It 'says the lines laid out again a second, from the state file''s count, and nothing for a build without it' {
    $b, $a = New-Snapshots -Seconds 5
    $s = ConvertTo-RestSample -Before $b -After $a -Frames 100 -Metrics $null -CycleRate 3000 -Frequency 1000 -Lines 2700
    $s.lines | Should -Be 540
    $none = ConvertTo-RestSample -Before $b -After $a -Frames 100 -Metrics $null -CycleRate 3000 -Frequency 1000
    $none.lines | Should -BeNullOrEmpty -Because 'a state file without work counts no lines'
  }

  It 'counts metrics over the reply''s own span, and leaves them out for a build without metrics or a server' {
    $b, $a = New-Snapshots -Seconds 5
    $s = ConvertTo-RestSample -Before $b -After $a -Frames 0 -Metrics (New-Metrics -Tasks 22 -SpanMs 5500) -CycleRate 3000 -Frequency 1000
    $s.tasks | Should -Be 4
    $none = ConvertTo-RestSample -Before $b -After $a -Frames 0 -Metrics $null -CycleRate 3000 -Frequency 1000
    $none.tasks | Should -BeNullOrEmpty
    $none.requests | Should -BeNullOrEmpty
    $none.top | Should -BeNullOrEmpty
    $none.server_cpu_ms | Should -BeNullOrEmpty
    $none.cpu_ms | Should -Be 0
    $none.frames | Should -Be 0
    $none.loops | Should -BeNullOrEmpty
    (ConvertTo-RestSample -Before $b -After $a -Frames 0 -Metrics $null -CycleRate 3000 -Frequency 1000 -Loops 0).loops | Should -Be 0
  }
}

Describe 'An action''s runs' {
  BeforeAll {
    # A scenario as Read-Budgets gives an action
    function New-Action([hashtable]$Budgets = @{ ms = @{ max = 30 }; p95_ms = @{ max = 50 }; gui_thread_ms = @{ max = 10 } }, [string]$Gui = 'key') {
      $b = [ordered]@{}
      foreach ($k in $Budgets.Keys) { $b[$k] = @{ max = $Budgets[$k].max; min = $Budgets[$k].min; why = $Budgets[$k].why } }
      [pscustomobject]@{ Name = 'key-echo'; Kind = 'action'; Domain = 'mux'; What = 'a key'; Bug = $null; Gui = $Gui; Runs = 5; Warmup = 1; Budgets = $b }
    }
    # A run's numbers as ConvertTo-ActionSample makes them
    $run = { param($ms, $gui = $null, [switch]$Warmup) [ordered]@{ warmup = [bool]$Warmup; ms = $ms; gui_ms = $gui; settled_ms = $ms + 100; gui_thread_ms = $ms / 2; frames = 2; top = $null } }
  }

  It 'makes a run''s numbers from its times, its cycles, its frames and the metrics reply' {
    $before = [pscustomobject]@{ Thread = 1000; Process = 5000; State = 100; Servers = @([pscustomobject]@{ Pid = 9; Cycles = 0 }) }
    # 12 ms of the GUI thread, 30 ms of the process of which 3 the state file's writer, 6 of the server's, at 3000 cycles a ms
    $after = [pscustomobject]@{ Thread = 1000 + 36000; Process = 5000 + 90000; State = 100 + 9000; Servers = @([pscustomobject]@{ Pid = 9; Cycles = 18000 }) }
    $m = [pscustomobject]@{
      totals = [pscustomobject]@{ messages = 412; tasks = 9 }
      messages = @([pscustomobject]@{ site = 'WM_PAINT'; count = 400; total_ms = 30 }, [pscustomobject]@{ site = 'WM_KEYDOWN'; count = 1; total_ms = 12 })
      budget = [pscustomobject]@{ actions = @([pscustomobject]@{ action = 'paint'; count = 2; max_ms = 9 }, [pscustomobject]@{ action = 'key'; count = 1; max_ms = 41.25 }) }
      requests = @(); tasks = @(); timers = @(); threads = @()
      mux = @([pscustomobject]@{ pdu = 'ListPanes'; sent = 1; received = 0 }, [pscustomobject]@{ pdu = 'Pong'; sent = 0; received = 0 },
        [pscustomobject]@{ pdu = 'Resize'; sent = 3; received = 0 })
    }
    $s = ConvertTo-ActionSample -StartMs 1000.5 -ShownMs 1042 -SettledMs 1100 -Before $before -After $after -Frames 3 -Metrics $m -Gui key -CycleRate 3000
    $s.warmup | Should -BeFalse
    $s.ms | Should -Be 41.5
    $s.settled_ms | Should -Be 99.5
    $s.gui_ms | Should -Be 41.25
    $s.gui_thread_ms | Should -Be 12
    $s.cpu_ms | Should -Be 27
    $s.state_ms | Should -Be 3
    $s.server_cpu_ms | Should -Be 6
    $s.frames | Should -Be 3
    $s.messages | Should -Be 412
    $s.wm_paint | Should -Be 400
    $s.top.messages[0].site | Should -Be 'WM_PAINT'
    # Every message's site and every PDU that went, so a run's one message more is traced by which
    @($s.top.messages | ForEach-Object { $_.site }) | Should -Be @('WM_PAINT', 'WM_KEYDOWN')
    @($s.top.mux | ForEach-Object { "$($_.pdu) $($_.sent)" }) | Should -Be @('Resize 3', 'ListPanes 1')
  }

  It 'reads its frame to the screen and its motion from the reply''s present part' {
    $m = [pscustomobject]@{
      totals = [pscustomobject]@{ messages = 10; tasks = 2 }
      budget = [pscustomobject]@{ actions = @([pscustomobject]@{ action = 'split'; count = 1; max_ms = 60 }) }
      present = [pscustomobject]@{
        actions = @(
          [pscustomobject]@{ action = 'paint'; count = 7; hidden = 0; overtaken = 0; lost = 0; last = [pscustomobject]@{ paint_ms = 0; present_ms = 5; screen_ms = 20; screen_unix_ms = 1100 } },
          [pscustomobject]@{ action = 'split'; count = 1; hidden = 0; overtaken = 0; lost = 0; last = [pscustomobject]@{ paint_ms = 40.5; present_ms = 58; screen_ms = 75.25; screen_unix_ms = 1080.75 } }
        )
        motion = [pscustomobject]@{ episodes = 2; measured = 2; covered = 0; missed = 3; refreshes = 30; counted = 2160; slow_paints = 1; longest_paint_ms = 21.5; first_ms = [pscustomobject]@{ mean = 33.3; max = 40 }; names = @([pscustomobject]@{ name = 'pane.layout' }, [pscustomobject]@{ name = 'pane.enter' }) }
      }
      requests = @(); tasks = @(); timers = @(); threads = @()
    }
    $s = ConvertTo-ActionSample -StartMs 1000.5 -ShownMs 1062 -Metrics $m -Gui split
    $s.paint_ms | Should -Be 40.5
    $s.screen_ms | Should -Be 75.25
    $s.after_present_ms | Should -Be 17.25
    $s.post_screen_ms | Should -Be 80.25
    $s.screen_hidden | Should -Be 0
    $s.missed | Should -Be 3
    $s.refreshes | Should -Be 30
    # The screen's own counter beside them, which jumped (PITFALLS 166), kept and judged by nothing
    $s.refreshes_counted | Should -Be 2160
    $s.slow_paints | Should -Be 1
    $s.motion_paint_ms | Should -Be 21.5
    $s.motion_first_ms | Should -Be 33.3
    $s.motion | Should -Be @('pane.layout', 'pane.enter')
  }

  It 'leaves out what the GUI couldn''t see go up: a hidden window''s frame and motion' {
    $m = [pscustomobject]@{
      totals = [pscustomobject]@{ messages = 10; tasks = 2 }
      budget = [pscustomobject]@{ actions = @() }
      present = [pscustomobject]@{
        actions = @([pscustomobject]@{ action = 'key'; count = 1; hidden = 1; overtaken = 0; lost = 0; last = [pscustomobject]@{ paint_ms = 12; present_ms = 18; screen_ms = $null; screen_unix_ms = $null } })
        motion = [pscustomobject]@{ episodes = 1; measured = 0; covered = 1; missed = 0; refreshes = 0; slow_paints = 0; longest_paint_ms = 4; first_ms = [pscustomobject]@{ mean = 0; max = 0 }; names = @() }
      }
    }
    $s = ConvertTo-ActionSample -StartMs 0 -ShownMs 20 -Metrics $m -Gui key
    $s.paint_ms | Should -Be 12
    $s.screen_ms | Should -BeNullOrEmpty
    $s.post_screen_ms | Should -BeNullOrEmpty
    $s.screen_hidden | Should -Be 1
    $s.motion_hidden | Should -Be 1
    $s.missed | Should -BeNullOrEmpty
    # Not judged: a budget on missed or screen_ms has nothing to hold
    $sc = [pscustomobject]@{ Name = 'k'; Kind = 'action'; Domain = 'mux'; What = 'k'; Bug = $null; Gui = 'key'; Runs = 1; Warmup = 0; Budgets = [ordered]@{ missed = @{ max = 0 }; screen_ms = @{ max = 30 } } }
    $r = Test-BudgetScenario -Scenario $sc -Samples @($s)
    $r.metrics.missed.Contains('pass') | Should -BeFalse
    $r.metrics.screen_ms.Contains('pass') | Should -BeFalse
    $r.result | Should -Be 'pass'
  }

  It 'reads what font setup did from the reply''s fonts part, and holds a budget on it' {
    $count = { param($n) [pscustomobject]@{ count = $n; ms = 0.5 * $n } }
    $m = [pscustomobject]@{
      totals = [pscustomobject]@{ messages = 10; tasks = 2 }
      fonts = [pscustomobject]@{ fonts = (& $count 3); files_read = (& $count 0); fallback_searches = (& $count 1); reshapes = (& $count 2); atlas_made = (& $count 1); atlas_side = 1024 }
    }
    $s = ConvertTo-ActionSample -StartMs 0 -ShownMs 20 -Metrics $m
    $s.font_loads | Should -Be 3
    $s.font_files | Should -Be 0
    $s.fallback_searches | Should -Be 1
    $s.reshapes | Should -Be 2
    $s.atlas_rebuilds | Should -Be 1
    $sc = [pscustomobject]@{ Name = 'reload'; Kind = 'action'; Domain = 'mux'; What = 'r'; Bug = $null; Gui = $null; Runs = 1; Warmup = 0; Budgets = [ordered]@{ font_loads = @{ max = 0 }; font_files = @{ max = 0 } } }
    $r = Test-BudgetScenario -Scenario $sc -Samples @($s)
    $r.result | Should -Be 'fail'
    @($r.over) -join ' ' | Should -Match 'font_loads'
    # A build from before fonts were counted measures none of them, so none is judged
    $old = ConvertTo-ActionSample -StartMs 0 -ShownMs 20 -Metrics ([pscustomobject]@{ totals = [pscustomobject]@{ messages = 1; tasks = 1 } })
    $old.Contains('font_loads') | Should -BeFalse
    (Test-BudgetScenario -Scenario $sc -Samples @($old)).result | Should -Be 'pass'
  }

  It 'reads the lines laid out again from the reply''s caches, none when the line cache did nothing, and holds a budget on them' {
    $m = [pscustomobject]@{
      totals = [pscustomobject]@{ messages = 4; tasks = 1 }
      caches = [pscustomobject]@{
        line_quad_cache = [pscustomobject]@{ hits = 120; misses = 3; evictions = 3 }
        shape_cache = [pscustomobject]@{ hits = 7; misses = 1; evictions = 0 }
      }
    }
    $s = ConvertTo-ActionSample -StartMs 0 -ShownMs 10 -Metrics $m
    $s.lines_laid_out | Should -Be 3
    $quiet = ConvertTo-ActionSample -StartMs 0 -ShownMs 10 -Metrics ([pscustomobject]@{ totals = [pscustomobject]@{ messages = 1; tasks = 1 }; caches = [pscustomobject]@{} })
    $quiet.lines_laid_out | Should -Be 0
    $sc = [pscustomobject]@{ Name = 'repaint'; Kind = 'action'; Domain = 'mux'; What = 'r'; Bug = $null; Gui = $null; Runs = 1; Warmup = 0; Budgets = [ordered]@{ lines_laid_out = @{ max = 0 } } }
    $r = Test-BudgetScenario -Scenario $sc -Samples @($s)
    $r.result | Should -Be 'fail'
    @($r.over) -join ' ' | Should -Match 'lines_laid_out'
    (Test-BudgetScenario -Scenario $sc -Samples @($quiet)).result | Should -Be 'pass'
    # A build from before the caches were counted measures none, so none is judged
    $old = ConvertTo-ActionSample -StartMs 0 -ShownMs 10 -Metrics ([pscustomobject]@{ totals = [pscustomobject]@{ messages = 1; tasks = 1 } })
    $old.Contains('lines_laid_out') | Should -BeFalse
    (Test-BudgetScenario -Scenario $sc -Samples @($old)).result | Should -Be 'pass'
  }

  It 'says a writer''s megabytes a second, and nothing from a build without metrics' {
    $s = ConvertTo-ActionSample -StartMs 0 -ShownMs 2000 -Bytes (16MB) -Warmup
    $s.warmup | Should -BeTrue
    $s.mb_per_s | Should -Be 8
    $s.gui_ms | Should -BeNullOrEmpty
    $s.messages | Should -BeNullOrEmpty
    $s.gui_thread_ms | Should -BeNullOrEmpty
    # A call's run is timed to its return; the shown time before the start is no time
    (ConvertTo-ActionSample -StartMs 100 -ShownMs 90).ms | Should -Be 0
  }

  It 'says what the GUI and its servers hold at a run''s end, and where its time went by the timed histograms, and holds a budget on the memory' {
    $c = { param($priv, $server) [pscustomobject]@{ Thread = 0; Process = 0; State = 0; Servers = @(); ServerThreads = @(); Threads = @(); PrivateBytes = $priv; ServerPrivateBytes = $server } }
    $m = [pscustomobject]@{
      totals = [pscustomobject]@{ messages = 3; tasks = 1 }
      histograms = @(
        [pscustomobject]@{ name = 'paint_pane.lines'; unit = 'ms'; count = 10; mean = 2.5 }
        [pscustomobject]@{ name = 'shape.harfbuzz'; unit = 'ms'; count = 4; mean = 0.5 }
        [pscustomobject]@{ name = 'pdu.size'; unit = 'value'; count = 9; mean = 100 }
      )
    }
    $s = ConvertTo-ActionSample -StartMs 0 -ShownMs 900 -Before (& $c 0 0) -After (& $c (300MB) (40MB)) -Metrics $m
    $s.private_mb | Should -Be 300
    $s.server_private_mb | Should -Be 40
    @($s.spent_ms.Keys) -join ',' | Should -Be 'paint_pane.lines,shape.harfbuzz'
    $s.spent_ms['paint_pane.lines'] | Should -Be 25
    # Locally there's no server: none is said
    $local = ConvertTo-ActionSample -StartMs 0 -ShownMs 900 -Before (& $c 0 0) -After (& $c (300MB) 0)
    $local.Contains('server_private_mb') | Should -BeFalse
    $sc = [pscustomobject]@{ Name = 'scrollback-local'; Kind = 'action'; Domain = 'local'; What = 's'; Bug = $null; Gui = $null; Runs = 1; Warmup = 0; Budgets = [ordered]@{ private_mb = @{ max = 280 } } }
    $r = Test-BudgetScenario -Scenario $sc -Samples @($s)
    $r.result | Should -Be 'fail'
    @($r.over) -join ' ' | Should -Match 'private_mb'
  }

  It 'judges the medians and 95th percentiles of the runs, the warm-ups only in first_ms' {
    $runs = @((& $run 300 -Warmup)) + @(10, 12, 14, 16, 40 | ForEach-Object { & $run $_ ($_ - 1) })
    $r = Test-BudgetScenario -Scenario (New-Action) -Samples $runs
    $r.result | Should -Be 'pass'
    $r.spans | Should -Be 5
    $r.warmup | Should -Be 1
    $r.gui | Should -Be 'key'
    $r.metrics.ms.value | Should -Be 14
    $r.metrics.ms.stat | Should -Be 'median'
    $r.metrics.p95_ms.value | Should -Be 40
    $r.metrics.p95_ms.stat | Should -Be 'p95'
    $r.metrics.gui_ms.value | Should -Be 13
    $r.metrics.first_ms.value | Should -Be 300
    $r.metrics.settled_ms.value | Should -Be 114
    $r.metrics.ms.Contains('median') | Should -BeFalse
    $r.metrics.ms.pass | Should -BeTrue
    $r.metrics.gui_thread_ms.value | Should -Be 7
  }

  It 'fails what''s over its most or under its least, saying which' {
    $runs = @(60, 70, 80 | ForEach-Object { & $run $_ })
    $r = Test-BudgetScenario -Scenario (New-Action) -Samples $runs
    $r.result | Should -Be 'fail'
    $r.over | Should -Be @('ms 70 ms (its budget 30)', 'p95_ms 80 ms (its budget 50)', 'gui_thread_ms 35 ms (its budget 10)')
    $tp = [ordered]@{ warmup = $false; ms = 4000; mb_per_s = 4.1 }
    $slow = Test-BudgetScenario -Scenario (New-Action -Budgets @{ mb_per_s = @{ min = 20 } } -Gui '') -Samples @($tp)
    $slow.result | Should -Be 'fail'
    $slow.over | Should -Be @('mb_per_s 4.1 MB/s (its budget at least 20)')
    $slow.metrics.mb_per_s.budget_min | Should -Be 20
    $fast = Test-BudgetScenario -Scenario (New-Action -Budgets @{ mb_per_s = @{ min = 20 } } -Gui '') -Samples @([ordered]@{ warmup = $false; ms = 400; mb_per_s = 41 })
    $fast.result | Should -Be 'pass'
  }

  It 'is an error with only warm-ups' {
    (Test-BudgetScenario -Scenario (New-Action) -Samples @((& $run 300 -Warmup))).result | Should -Be 'error'
  }

  It 'times keys typed one after another each to the first frame that drew its echo, the last key''s as the run''s' {
    # Three keys 15 ms apart from column 4; frames drawing the cursor at 4, 5, 7 (two echoes in
    # one) and, a row down, the line wrapped
    $seen = @(
      [pscustomobject]@{ T = 1005; X = 4; Y = 2 },
      [pscustomobject]@{ T = 1009; X = 5; Y = 2 },
      [pscustomobject]@{ T = 1041; X = 7; Y = 2 }
    )
    $e = ConvertTo-KeyEchoes -Posts @(1000, 1015, 1030) -Seen $seen -X 4 -Y 2
    $e.echoes | Should -Be @(9, 26, 11)
    $e.echo_ms | Should -Be 11
    $e.echo_max_ms | Should -Be 26
    $e.ms | Should -Be 11
    # An even count's median is the middle two's mean; a row down is past every key
    $w = ConvertTo-KeyEchoes -Posts @(1000, 1015) -Seen @([pscustomobject]@{ T = 1020.5; X = 0; Y = 3 }) -X 4 -Y 2
    $w.echoes | Should -Be @(20.5, 5.5)
    $w.echo_ms | Should -Be 13
    # A key no frame shows is left out; none shown is nothing
    (ConvertTo-KeyEchoes -Posts @(1000, 1015) -Seen @([pscustomobject]@{ T = 1009; X = 5; Y = 2 }) -X 4 -Y 2).echoes | Should -Be @(9)
    (ConvertTo-KeyEchoes -Posts @(1000) -Seen @() -X 4 -Y 2).Count | Should -Be 0
  }

  It 'judges keys typed one after another on their echoes'' median and the 95th percentile of each run''s slowest' {
    $runs = @(
      [ordered]@{ warmup = $false; ms = 20; echo_ms = 10; echo_max_ms = 20 },
      [ordered]@{ warmup = $false; ms = 22; echo_ms = 12; echo_max_ms = 22 },
      [ordered]@{ warmup = $false; ms = 40; echo_ms = 30; echo_max_ms = 40 }
    )
    $r = Test-BudgetScenario -Scenario (New-Action -Budgets @{ echo_ms = @{ max = 20 }; echo_max_ms = @{ max = 35 } } -Gui '') -Samples $runs
    $r.metrics.echo_ms.value | Should -Be 12
    $r.metrics.echo_ms.stat | Should -Be 'median'
    $r.metrics.echo_max_ms.value | Should -Be 40
    $r.metrics.echo_max_ms.stat | Should -Be 'p95'
    $r.over | Should -Contain 'echo_max_ms 40 ms (its budget 35)'
    $r.over | Should -Not -Contain 'echo_ms 12 ms (its budget 20)'
  }
}

Describe 'Judging a scenario' {
  BeforeAll {
    $at = { param($frames, $gui, $cpu) [ordered]@{ frames = $frames; gui_thread_ms = $gui; cpu_ms = $cpu; tasks = $null; top = $null; threads = @([ordered]@{ name = 'main'; tid = 1; ms = $gui }) } }
  }

  It 'passes when every median is at or under its budget' {
    $r = Test-BudgetScenario -Scenario (New-Scenario) -Samples @((& $at 0 0.1 1.6), (& $at 0.2 0.3 2.1), (& $at 0 0.2 9))
    $r.result | Should -Be 'pass'
    $r.metrics.frames.median | Should -Be 0
    $r.metrics.frames.max | Should -Be 0.2
    $r.metrics.cpu_ms.median | Should -Be 2.1
    $r.metrics.cpu_ms.pass | Should -BeTrue
    $r.spans | Should -Be 3
    $r.over | Should -BeNullOrEmpty
  }

  It 'fails saying what''s over and by how much, and keeps the bug budgets.json names' {
    $r = Test-BudgetScenario -Scenario (New-Scenario -Bug 'the view paints every refresh') -Samples @((& $at 118 812 840), (& $at 119 790 820), (& $at 120 800 850)) -Lines @('window-0 painted 1190 frames in 10 s at rest')
    $r.result | Should -Be 'fail'
    $r.over | Should -Be @('frames 119 /s (its budget 0.2)', 'gui_thread_ms 800 ms/s (its budget 2)', 'cpu_ms 840 ms/s (its budget 5)')
    $r.bug | Should -Be 'the view paints every refresh'
    $r.lines | Should -Be @('window-0 painted 1190 frames in 10 s at rest')
    $r.top.threads[0].name | Should -Be 'main'
    $r.top.threads[0].n | Should -Be 2402
  }

  It 'doesn''t judge what a build doesn''t measure, and is an error without spans or with a setup''s failure' {
    $r = Test-BudgetScenario -Scenario (New-Scenario -Budgets @{ tasks = @{ max = 2 }; frames = @{ max = 0.2 } }) -Samples @((& $at 0 0 0))
    $r.result | Should -Be 'pass'
    $r.metrics.tasks.Contains('pass') | Should -BeFalse
    $r.metrics.tasks.budget | Should -Be 2
    # Said, not passed: a budget nothing measured is named
    $r.metrics.tasks.unmeasured | Should -BeTrue
    @($r.unmeasured) | Should -Be @('tasks')
    $r.metrics.frames.Contains('unmeasured') | Should -BeFalse
    (Test-BudgetScenario -Scenario (New-Scenario) -Samples @() -Errors @('the window didn''t open')).result | Should -Be 'error'
    (Test-BudgetScenario -Scenario (New-Scenario) -Samples @((& $at 0 0 0)) -Errors @('after span 1 it isn''t what it set up')).result | Should -Be 'error'
  }

  It 'adds up the mux server''s threads across the runs, and takes a run''s busiest threads from two lists' {
    $before = @([pscustomobject]@{ Tid = 7; Name = 'main'; Cycles = 1000 }, [pscustomobject]@{ Tid = 8; Name = ''; Cycles = 10 }, [pscustomobject]@{ Tid = 9; Name = 'gone'; Cycles = 5 })
    $after = @([pscustomobject]@{ Tid = 7; Name = 'main'; Cycles = 61000 }, [pscustomobject]@{ Tid = 8; Name = ''; Cycles = 3010 })
    $top = @(Get-TopThreads $before $after 3000)
    @($top.name) | Should -Be @('main', '(unnamed)')
    @($top.ms) | Should -Be @(20, 1)
    @(Get-TopThreads @() $after 3000).Count | Should -Be 0
    # The process spent 51 ms where its threads alive at both ends spent 21: 30 on threads that came and went
    $whole = @(Get-TopThreads $before $after 3000 -Whole 153000)
    $whole[0].name | Should -Be '(threads gone by the end)'
    $whole[0].ms | Should -Be 30
    @(Get-TopThreads $before $after 3000 -Whole 63100).Count | Should -Be 2
    $runs = @(1..2 | ForEach-Object { [ordered]@{ ms = 20; top = $null; server_threads = @([ordered]@{ name = 'main'; tid = 7; ms = 19.5 }, [ordered]@{ name = '(ntdll.dll)'; tid = 8; ms = 0.5 }) } })
    $t = Get-BudgetTopSites $runs
    $t.server_threads[0].name | Should -Be 'main'
    $t.server_threads[0].n | Should -Be 39
  }

  It 'adds up who ran across the spans, most first' {
    $s1 = [ordered]@{ frames = 1; top = [ordered]@{ requests = @(@{ site = 'a.rs:1'; frames = 2 }, @{ site = 'b.rs:2'; frames = 5 }); tasks = @(@{ site = 't.rs:3'; count = 4; ms = 1.5 }); timers = @(); messages = @() } }
    $s2 = [ordered]@{ frames = 1; top = [ordered]@{ requests = @(@{ site = 'a.rs:1'; frames = 4 }); tasks = @(@{ site = 't.rs:3'; count = 1; ms = 0.5 }); timers = @(); messages = @() } }
    $t = Get-BudgetTopSites @($s1, $s2)
    @($t.requests.site) | Should -Be @('a.rs:1', 'b.rs:2')
    $t.requests[0].n | Should -Be 6
    $t.tasks[0].n | Should -Be 5
    $t.tasks[0].ms | Should -Be 2
    @($t.pdus_sent).Count | Should -Be 0 -Because 'samples from before the PDUs were kept have none'
    $s1.top.mux = @(@{ pdu = 'Resize'; sent = 3; received = 0 }, @{ pdu = 'TabResized'; sent = 0; received = 2 })
    $s2.top.mux = @(@{ pdu = 'Resize'; sent = 1; received = 0 })
    $t = Get-BudgetTopSites @($s1, $s2)
    $t.pdus_sent[0].pdu | Should -Be 'Resize'
    $t.pdus_sent[0].n | Should -Be 4
    @($t.pdus_received.pdu) | Should -Be @('TabResized')
  }
}

Describe 'The report, its table and the history' {
  BeforeAll {
    $sample = { param($frames, $gui) [ordered]@{ frames = $frames; gui_thread_ms = $gui; cpu_ms = $gui; top = [ordered]@{ requests = @(@{ site = 'render/conductor_view.rs:338'; frames = 300 }); tasks = @(); timers = @(); messages = @() } } }
    $pass = Test-BudgetScenario -Scenario ((New-Scenario) | ForEach-Object { $_.Name = 'one-tab'; $_ }) -Samples @((& $sample 0 0.1))
    $fail = Test-BudgetScenario -Scenario (New-Scenario -Bug 'paints every refresh') -Samples @((& $sample 120 800)) -Lines @('window-0 painted 1190 frames in 10 s at rest, asked by render/conductor_view.rs:338 (1190)')
    $broken = Test-BudgetScenario -Scenario ((New-Scenario) | ForEach-Object { $_.Name = 'minimized'; $_ }) -Samples @() -Errors @('the window didn''t minimize')
    $report = New-BudgetReport -Run '20261030-120000' -Bin 'C:\b' -Build '20261030-1-abc' -Commit 'abc' -Started (Get-Date).AddMinutes(-3) -Results @($pass, $fail, $broken) -Runs 3 -Seconds 5
  }

  It 'counts the results' {
    $report.summary.pass | Should -Be 1
    $report.summary.fail | Should -Be 1
    $report.summary.error | Should -Be 1
    $report.runs | Should -Be 3
    $report.build | Should -Be '20261030-1-abc'
  }

  It 'tables a row a scenario, what''s over marked, then each failure with what''s over, the bug, who asked and what the GUI said' {
    $t = @(Format-BudgetTable $report)
    $t[0] | Should -Match '^scenario\s+result\s+frames/s\s+gui ms/s\s+cpu ms/s'
    $t[1] | Should -Match '^one-tab\s+pass\s+0\s+0\.1\s+0\.1'
    $t[2] | Should -Match '^conductor-view\s+fail\s+120!\s+800!\s+800!'
    $text = $t -join "`n"
    $text | Should -Match 'conductor-view: fail, over: frames 120 /s \(its budget 0\.2\)'
    $text | Should -Match 'known: paints every refresh'
    $text | Should -Match 'frames asked by: render/conductor_view\.rs:338 \(300\)'
    $text | Should -Match 'said: window-0 painted 1190 frames'
    $text | Should -Match 'minimized: error'
    $text | Should -Match 'error: the window didn''t minimize'
    $t[-1] | Should -Match '^1 passed, 1 over budget, 1 couldn''t be measured'
  }

  It 'writes the run as one line of history with every scenario''s result and medians, appended' {
    $line = ConvertTo-BudgetHistoryLine $report
    $line | Should -Not -Match "`n"
    $h = $line | ConvertFrom-Json
    $h.build | Should -Be '20261030-1-abc'
    $h.scenarios.'conductor-view'.result | Should -Be 'fail'
    $h.scenarios.'conductor-view'.frames | Should -Be 120
    $h.scenarios.'one-tab'.gui_thread_ms | Should -Be 0.1
    $h.scenarios.minimized.result | Should -Be 'error'
    $file = Join-Path $TestDrive 'h\history.jsonl'
    Add-BudgetHistory $report $file
    Add-BudgetHistory $report $file
    @(Get-Content $file).Count | Should -Be 2
    [IO.File]::ReadAllBytes($file)[0] | Should -Be ([byte][char]'{')
  }

  It 'tables the actions apart, under their own heading, and writes their values to the history' {
    $sc = [pscustomobject]@{ Name = 'split'; Kind = 'action'; Domain = 'mux'; What = 'a split'; Bug = 'twice the local'; Gui = 'split'; Runs = 3; Warmup = 0
      Budgets = [ordered]@{ ms = @{ max = 100 }; p95_ms = @{ max = 120 }; gui_thread_ms = @{ max = 50 } } }
    $samples = @(70, 72, 74 | ForEach-Object { [ordered]@{ warmup = $false; ms = $_; gui_ms = $_ - 1; settled_ms = 620; gui_thread_ms = 160; cpu_ms = 180; server_cpu_ms = 115; frames = 7; top = $null } })
    $act = Test-BudgetScenario -Scenario $sc -Samples $samples -Lines @('split took 74.0 ms, 4.0 ms over its 70 ms: paint window-0/draw 20.0 ms')
    $both = New-BudgetReport -Run 'r' -Bin 'C:\b' -Build 'b' -Commit 'c' -Started (Get-Date) -Results @($pass, $act) -Runs 3 -Seconds 5
    $t = @(Format-BudgetTable $both)
    $t[0] | Should -Match '^scenario\s+result'
    $t[1] | Should -Match '^one-tab\s+pass'
    $t[2] | Should -Be ''
    $t[3] | Should -Match '^action\s+result\s+p50 ms\s+p95 ms\s+gui ms\s+screen\s+settled\s+first\s+gui cpu'
    $t[4] | Should -Match '^split\s+fail\s+72\s+74\s+71\s+-\s+620\s+-\s+160!'
    $text = $t -join "`n"
    $text | Should -Match 'split: fail, over: gui_thread_ms 160 ms \(its budget 50\)'
    $text | Should -Match 'known: twice the local'
    $text | Should -Match 'said: split took 74\.0 ms'
    $h = (ConvertTo-BudgetHistoryLine $both) | ConvertFrom-Json
    $h.scenarios.split.kind | Should -Be 'action'
    $h.scenarios.split.ms | Should -Be 72
    $h.scenarios.split.p95_ms | Should -Be 74
    $h.scenarios.split.server_cpu_ms | Should -Be 115
    $h.scenarios.'one-tab'.frames | Should -Be 0
  }

  It 'says what has a budget nothing measured: in the summary, the table and the scenario, never as over' {
    $sc = [pscustomobject]@{ Name = 'split'; Kind = 'action'; Domain = 'mux'; What = 'a split'; Bug = $null; Gui = 'split'; Runs = 2; Warmup = 0
      Budgets = [ordered]@{ ms = @{ max = 100 }; missed = @{ max = 0 } } }
    # A hidden window: no run says how many refreshes went without a frame
    $hid = Test-BudgetScenario -Scenario $sc -Samples @(70, 72 | ForEach-Object { [ordered]@{ warmup = $false; ms = $_; top = $null } })
    $hid.result | Should -Be 'pass'
    @($hid.unmeasured) | Should -Be @('missed')
    $seen = Test-BudgetScenario -Scenario $sc -Samples @(70, 72 | ForEach-Object { [ordered]@{ warmup = $false; ms = $_; missed = 0; top = $null } })
    @($seen.unmeasured).Count | Should -Be 0
    $r = New-BudgetReport -Run 'r' -Bin 'C:\b' -Build 'b' -Commit 'c' -Started (Get-Date) -Results @($hid, $seen) -Runs 2 -Seconds 5
    $r.summary.pass | Should -Be 2
    $r.summary.unmeasured | Should -Be 1
    $t = @(Format-BudgetTable $r)
    ($t -join "`n") | Should -Match 'budgets nothing measured \(not judged\):\s+split: missed'
    $t[-1] | Should -Match '^2 passed, 0 over budget, 0 couldn''t be measured, 1 with budgets nothing measured'
  }

  It 'says the medians that moved since the last run of each scenario, from one history for every tree' {
    $file = Join-Path $TestDrive 'drift\history.jsonl'
    $old = Test-BudgetScenario -Scenario (New-Scenario) -Samples @([ordered]@{ frames = 0; gui_thread_ms = 4; cpu_ms = 4.5; top = $null })
    Add-BudgetHistory (New-BudgetReport -Run 'first' -Bin 'C:\b' -Build 'b1' -Commit 'c' -Started (Get-Date) -Results @($old) -Runs 1 -Seconds 5) $file
    $last = Read-BudgetHistory $file
    $last['conductor-view'].run | Should -Be 'first'
    # gui_thread_ms 4 to 1.5 moved 62% and over 1 ms; cpu_ms 4.5 to 4.9 is 9%, and frames 0 to 0.2 under the floor
    $now = Test-BudgetScenario -Scenario (New-Scenario) -Samples @([ordered]@{ frames = 0.2; gui_thread_ms = 1.5; cpu_ms = 4.9; top = $null })
    $d = @(Get-BudgetDrift $now $last['conductor-view'])
    @($d.metric) | Should -Be @('gui_thread_ms')
    $d[0].was | Should -Be 4
    $d[0].now | Should -Be 1.5
    $d[0].run | Should -Be 'first'
    @(Get-BudgetDrift $now $null).Count | Should -Be 0
    $now.drift = $d
    $r = New-BudgetReport -Run 'second' -Bin 'C:\b' -Build 'b2' -Commit 'c' -Started (Get-Date) -Results @($now) -Runs 1 -Seconds 5
    $r.summary.drift | Should -Be 1
    (@(Format-BudgetTable $r) -join "`n") | Should -Match 'conductor-view: gui_thread_ms 4 to 1\.5 \(first\)'
    # Every slot's runs go to the main checkout's target
    $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
    $main = Split-Path ((git -C $repo rev-parse --path-format=absolute --git-common-dir) -replace '/', '\') -Parent
    Get-BudgetHistoryPath $repo | Should -Be (Join-Path $main 'target\budgets\history.jsonl')
    Get-BudgetHistoryPath $TestDrive | Should -Be (Join-Path $TestDrive 'target\budgets\history.jsonl')
  }

  It 'says numbers in a few figures' {
    Format-BudgetNumber 0 | Should -Be '0'
    Format-BudgetNumber 0.0004 | Should -Be '0'
    Format-BudgetNumber 0.1234 | Should -Be '0.123'
    Format-BudgetNumber 2.345 | Should -Be '2.35'
    Format-BudgetNumber 24.77 | Should -Be '24.8'
    Format-BudgetNumber 812.4 | Should -Be '812'
    Format-BudgetNumber $null | Should -Be '-'
  }
}

Describe 'What a scenario''s run left' {
  It 'judges its measure.json, the budget''s own failures not errors, a setup''s throw an error' {
    $dir = Join-Path $TestDrive 'attempt'
    New-Item -ItemType Directory -Force $dir | Out-Null
    $m = [ordered]@{ scenario = 'conductor-view'; samples = @([ordered]@{ frames = 120; gui_thread_ms = 800; cpu_ms = 800 }); lines = @('a line'); notes = @('a note') }
    [IO.File]::WriteAllText((Join-Path $dir 'measure.json'), ($m | ConvertTo-Json -Depth 6))
    $over = [pscustomobject]@{ Dir = $dir; Exit = 1; Seconds = 30; Failures = @('over budget: frames 120 /s (its budget 0.2)'); Notes = @('another test''s window') }
    $r = Read-BudgetAttempt -Scenario (New-Scenario) -Attempt $over
    $r.result | Should -Be 'fail'
    $r.errors | Should -BeNullOrEmpty
    $r.lines | Should -Be @('a line')
    $r.notes | Should -Be @('a note', 'another test''s window')
    $r.out | Should -Be $dir
    $threw = [pscustomobject]@{ Dir = $dir; Exit = 1; Seconds = 30; Failures = @('over budget: frames 120 /s (its budget 0.2)', 'the window didn''t open (budgets.ps1:114)'); Notes = @() }
    $e = Read-BudgetAttempt -Scenario (New-Scenario) -Attempt $threw
    $e.result | Should -Be 'error'
    $e.errors | Should -Be @('the window didn''t open (budgets.ps1:114)')
    $empty = Join-Path $TestDrive 'empty'
    New-Item -ItemType Directory -Force $empty | Out-Null
    $n = Read-BudgetAttempt -Scenario (New-Scenario) -Attempt ([pscustomobject]@{ Dir = $empty; Exit = -1; Seconds = 600; Failures = @(); Notes = @() })
    $n.result | Should -Be 'error'
    $n.errors | Should -Be @('nothing measured (exit -1)')
  }
}

Describe 'The PC while it measured' {
  BeforeAll {
    # Two readings 2 s apart at a clock of 1000 ticks a second, on a PC of four CPUs whose test may
    # run on the first two (mask 3): CPU 0 busy half the time, CPU 1 all of it, CPU 2 a fifth, CPU 3
    # not at all, in 100 ns. The measurement's own took half a CPU (3e9 cycles at 3e6 a ms over 2 s):
    # pid 10 two thirds of it, pid 12, which started in between, the rest; pid 11 nothing
    function New-PcReadings([long]$Mask = 3, [hashtable]$Clock = @{ '0,0' = 120.0; '0,1' = 90.0; '0,2' = 50.0; '0,3' = 40.0 }) {
      $start = [pscustomobject]@{ At = 0; Busy = [long[]]@(5e7, 5e7, 5e7, 5e7); Mask = $Mask; Own = @{ 10 = [long]1000; 11 = [long]500 } }
      $stop = [pscustomobject]@{ At = 2000; Busy = [long[]]@(6e7, 7e7, 5.4e7, 5e7); Own = @{ 10 = [long](1000 + 2e9); 11 = [long]500; 12 = [long]1e9 }; Clock = $Clock }
      $start, $stop
    }
  }

  It 'says what others took of the test''s CPUs, the whole PC''s share busy and the clock, weighted by how busy each CPU was' {
    $start, $stop = New-PcReadings
    $p = ConvertTo-PcSpan -Start $start -Stop $stop -CycleRate 3e6 -Frequency 1000
    $p.seconds | Should -Be 2
    $p.cpus | Should -Be 2
    $p.of | Should -Be 4
    $p.own | Should -Be 0.5
    # 1.5 CPUs busy on its two, 0.5 its own: one CPU's worth of others over two
    $p.others | Should -Be 0.5
    # 3.4 CPUs' worth busy over the PC's four
    $p.pc | Should -Be 0.425
    # (120 x 1 + 90 x 2) / 3: CPU 1 ran twice as long as CPU 0; CPUs 2 and 3 aren't its
    $p.clock | Should -Be 1
    $p.ghz | Should -Be 3
  }

  It 'takes every CPU without a mask, an idle set''s plain average, and never less than nothing from others' {
    $start, $stop = New-PcReadings -Mask 0
    $p = ConvertTo-PcSpan -Start $start -Stop $stop -CycleRate 3e6 -Frequency 1000
    $p.cpus | Should -Be 4
    # 1.7 CPUs busy over the four, 0.5 its own
    $p.others | Should -Be 0.3
    $idle = [pscustomobject]@{ At = 0; Busy = [long[]]@(1, 1); Mask = 3; Own = @{} }
    $still = [pscustomobject]@{ At = 1000; Busy = [long[]]@(1, 1); Own = @{ 10 = [long]3e9 }; Clock = @{ '0,0' = 50.0; '0,1' = 70.0 } }
    $q = ConvertTo-PcSpan -Start $idle -Stop $still -CycleRate 3e6 -Frequency 1000
    $q.clock | Should -Be 0.6
    $q.others | Should -Be 0
  }

  It 'throws for a stretch of no time, a mask naming no CPU, and a CPU whose clock wasn''t read' {
    $start, $stop = New-PcReadings
    { ConvertTo-PcSpan -Start $start -Stop $start -Frequency 1000 } | Should -Throw '*a stretch of 0 s*'
    $none = [pscustomobject]@{ At = 0; Busy = [long[]]@(1, 1, 1, 1); Mask = [long]16; Own = @{} }
    { ConvertTo-PcSpan -Start $none -Stop $stop -Frequency 1000 } | Should -Throw '*names none of*'
    $start, $stop = New-PcReadings -Clock @{ '0,0' = 100.0 }
    { ConvertTo-PcSpan -Start $start -Stop $stop -Frequency 1000 } | Should -Throw '*no clock read for processor 1*'
  }

  It 'scales what''s measured by cycles to the nominal clock, keeps it raw, and says busy against the rules' {
    $pc = [ordered]@{ seconds = 5; cpus = 4; of = 36; own = 0.1; others = 0.5; pc = 0.3; clock = 1.5; ghz = 4.5 }
    $rules = [pscustomobject]@{ OthersMax = 0.4; PcMax = 0.75; Retakes = 2 }
    $s = Add-PcSample ([ordered]@{ seconds = 5; frames = 4; gui_thread_ms = 2; cpu_ms = 4; server_cpu_ms = $null; frame_ms = 0.5; tasks = 3 }) $pc 'rest' $rules
    $s.gui_thread_ms | Should -Be 3
    $s.cpu_ms | Should -Be 6
    $s.frame_ms | Should -Be 0.75
    $s.frames | Should -Be 4
    $s.tasks | Should -Be 3
    $s.server_cpu_ms | Should -BeNullOrEmpty
    $s.raw.gui_thread_ms | Should -Be 2
    $s.raw.Contains('server_cpu_ms') | Should -BeFalse
    $s.pc.busy | Should -BeTrue
    $s.pc.why | Should -Be 'others took 50% of its CPUs (over 40%)'
    (Format-PcSample $s) | Should -Be '; others 50% of its 4 CPUs, the PC 30%, clock 4.5 GHz, busy: others took 50% of its CPUs (over 40%)'
    # An action's time isn't scaled, its CPU is; with no rules nothing is busy
    $a = Add-PcSample ([ordered]@{ warmup = $false; ms = 12; gui_thread_ms = 4; state_ms = 1 }) ([ordered]@{ others = 0.9; pc = 0.9; clock = 0.5; ghz = 1.5; cpus = 4 }) 'action' $null
    $a.ms | Should -Be 12
    $a.gui_thread_ms | Should -Be 2
    $a.state_ms | Should -Be 0.5
    $a.pc.busy | Should -BeFalse
    $b = Add-PcSample ([ordered]@{ ms = 1 }) ([ordered]@{ others = 0.1; pc = 0.9; clock = 1; ghz = 3; cpus = 4 }) 'action' $rules
    $b.pc.why | Should -Be 'the PC was 90% busy (over 75%)'
  }

  It 'reads budgets.json''s pc rules, none without the section, and refuses rules it can''t hold a run to' {
    $repoPc = (Read-Budgets $real).Pc
    $repoPc.OthersMax | Should -BeGreaterThan 0
    $repoPc.OthersMax | Should -BeLessOrEqual 1
    $repoPc.PcMax | Should -BeGreaterThan 0
    $repoPc.Retakes | Should -BeGreaterOrEqual 1
    $plain = Read-Budgets (New-BudgetsFile (Join-Path $TestDrive 'nopc') @(@{ name = 'one-tab'; what = 'a tab' }))
    $plain.Pc.OthersMax | Should -BeNullOrEmpty
    $plain.Pc.Retakes | Should -Be 0
    $write = { param($pc)
      $dir = Join-Path $TestDrive ('pc' + [guid]::NewGuid().ToString('n'))
      $path = New-BudgetsFile $dir @(@{ name = 'one-tab'; what = 'a tab' })
      $j = Get-Content -Raw $path | ConvertFrom-Json -AsHashtable
      $j.pc = $pc
      [IO.File]::WriteAllText($path, ($j | ConvertTo-Json -Depth 8))
      $path
    }
    (Read-Budgets (& $write @{ others_max = 0.5; retakes = 2; why = 'w' })).Pc.Retakes | Should -Be 2
    { Read-Budgets (& $write @{ others_max = 1.5 }) } | Should -Throw '*pc.others_max is ''1.5''*'
    { Read-Budgets (& $write @{ pc_max = 0 }) } | Should -Throw '*pc.pc_max*'
    { Read-Budgets (& $write @{ retakes = -1 }) } | Should -Throw '*pc.retakes*'
    { Read-Budgets (& $write @{ clock_min = 0.8 }) } | Should -Throw '*pc.clock_min isn''t a rule*'
  }

  It 'judges on the calm spans while half its runs were calm, else on all, over budget then busy' {
    $calm = { param($gui) [ordered]@{ frames = 0; gui_thread_ms = $gui; cpu_ms = 1; top = $null; pc = [ordered]@{ others = 0.1; pc = 0.2; clock = 1.1; ghz = 3.3; busy = $false } } }
    $busy = { param($gui) [ordered]@{ frames = 0; gui_thread_ms = $gui; cpu_ms = 1; top = $null; pc = [ordered]@{ others = 0.7; pc = 0.5; clock = 0.9; ghz = 2.7; busy = $true; why = 'others took 70% of its CPUs (over 50%)' } } }
    $sc = New-Scenario | Add-Member -NotePropertyName Runs -NotePropertyValue 3 -PassThru
    $r = Test-BudgetScenario -Scenario $sc -Samples @((& $calm 1), (& $busy 9), (& $calm 1.5))
    $r.result | Should -Be 'pass'
    $r.spans | Should -Be 2
    $r.pc.calm | Should -Be 2
    $r.pc.busy | Should -Be 1
    $r.pc.judged | Should -Be 'calm'
    $r.pc.others | Should -Be 0.1
    @($r.pc.why) | Should -Be @('others took 70% of its CPUs (over 50%)')
    $over = Test-BudgetScenario -Scenario $sc -Samples @((& $busy 9), (& $busy 8), (& $calm 1))
    $over.pc.judged | Should -Be 'all'
    $over.result | Should -Be 'busy'
    $over.spans | Should -Be 3
    (Test-BudgetScenario -Scenario $sc -Samples @((& $busy 1), (& $busy 1), (& $calm 1))).result | Should -Be 'pass'
    # Over budget on calm spans is a failure, however busy the others were
    (Test-BudgetScenario -Scenario $sc -Samples @((& $calm 9), (& $busy 1), (& $calm 9))).result | Should -Be 'fail'
    $report = New-BudgetReport -Run 'r' -Bin 'C:\b' -Build 'b' -Commit 'c' -Started (Get-Date) -Results @($r, $over) -Runs 3 -Seconds 5
    $report.summary.busy | Should -Be 1
    $report.summary.fail | Should -Be 0
    $t = @(Format-BudgetTable $report)
    $t[0] | Should -Match 'others\s+GHz$'
    $t[1] | Should -Match '10%\s+3\.3$'
    $text = $t -join "`n"
    $text | Should -Match 'conductor-view: busy, over: gui_thread_ms 8 ms/s'
    $text | Should -Match 'the PC: 2 of its spans busy \(others took 70% of its CPUs \(over 50%\)\), judged on all of them'
    $text | Should -Match 'the PC: 1 of its spans busy .*judged on the 2 calm ones'
    $t[-1] | Should -Match '^1 passed, 0 over budget, 1 over on a busy PC \(run again when it''s quieter\), 0 couldn''t be measured'
  }

  It 'writes the PC and the clock to the history, and holds no scaled metric to a line from before the scaling' {
    $sc = New-Scenario | Add-Member -NotePropertyName Runs -NotePropertyValue 1 -PassThru
    $judged = Test-BudgetScenario -Scenario $sc -Samples @([ordered]@{ frames = 0; gui_thread_ms = 1; cpu_ms = 1; top = $null; pc = [ordered]@{ others = 0.2; pc = 0.3; clock = 0.5; ghz = 1.5; busy = $false } })
    $h = ConvertTo-BudgetHistoryLine (New-BudgetReport -Run 'r' -Bin 'C:\b' -Build 'b' -Commit 'c' -Started (Get-Date) -Results @($judged) -Runs 1 -Seconds 5) | ConvertFrom-Json
    $h.clock | Should -Be 'nominal'
    $h.scenarios.'conductor-view'.pc.others | Should -Be 0.2
    $h.scenarios.'conductor-view'.pc.clock | Should -Be 0.5
    $raw = @{ run = 'old'; clock = ''; metrics = @{ gui_thread_ms = 9; frames = 9 } }
    @((Get-BudgetDrift $judged $raw).metric) | Should -Be @('frames')
    $raw.clock = 'nominal'
    @((Get-BudgetDrift $judged $raw).metric) | Should -Be @('frames', 'gui_thread_ms')
  }
}

Describe 'Two builds against each other' {
  BeforeAll {
    # The comparison runs each attempt through the harness's Invoke-E2eAttempt
    Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking -Global
    $judgedOf = { param([hashtable]$values) [ordered]@{ metrics = [ordered]@{ ms = [ordered]@{ value = $values.ms }; gui_thread_ms = [ordered]@{ value = $values.gui }; frames = [ordered]@{ value = $values.frames } } } }
    $action = [pscustomobject]@{ Name = 'spinner-tall'; Kind = 'action' }
  }

  It 'tells a difference apart by Welch''s test on the rounds at 99%, else calls it the same' {
    $before = @((& $judgedOf @{ ms = 10; gui = 2; frames = 1 }), (& $judgedOf @{ ms = 11; gui = 2.1; frames = 1 }), (& $judgedOf @{ ms = 10.5; gui = 2; frames = 1 }))
    $after = @((& $judgedOf @{ ms = 10.6; gui = 3; frames = 1 }), (& $judgedOf @{ ms = 11.2; gui = 3.1; frames = 1 }), (& $judgedOf @{ ms = 11.4; gui = 3; frames = 1 }))
    $c = @(Compare-BudgetRounds -Scenario $action -Before $before -After $after -BeforeAll (& $judgedOf @{ ms = 10.5; gui = 2; frames = 1 }) -AfterAll (& $judgedOf @{ ms = 11.2; gui = 3; frames = 1 }))
    @($c.metric) | Should -Be @('ms', 'gui_thread_ms', 'frames')
    $ms = $c | Where-Object metric -eq 'ms'
    # The rounds' means 10.5 and 11.067: 0.567 apart, against a 99% interval of +-2.19 (3.87
    # degrees of freedom, so 3's 5.841 times the difference's standard error of 0.376)
    $ms.diff | Should -Be 0.7
    $ms.mean_diff | Should -Be 0.567
    $ms.within | Should -Be 2.194
    $ms.verdict | Should -Be 'same'
    @($ms.before_rounds) | Should -Be @(10, 11, 10.5)
    # Rounds that don't say which they were pair with nothing
    $ms.paired_n | Should -Be 0
    $ms.paired_within | Should -BeNullOrEmpty
    $ms.paired_verdict | Should -BeNullOrEmpty
    $gui = $c | Where-Object metric -eq 'gui_thread_ms'
    # 1 apart against +-0.217 (4 degrees, 4.604 times 0.047)
    $gui.within | Should -Be 0.217
    $gui.verdict | Should -Be 'higher'
    $gui.share | Should -Be 0.5
    ($c | Where-Object metric -eq 'frames').verdict | Should -Be 'same'
    $lower = @(Compare-BudgetRounds -Scenario $action -Before $after -After $before -BeforeAll (& $judgedOf @{ ms = 11.2; gui = 3; frames = 1 }) -AfterAll (& $judgedOf @{ ms = 10.5; gui = 2; frames = 1 }))
    ($lower | Where-Object metric -eq 'gui_thread_ms').verdict | Should -Be 'lower'
    # One round of a build has nothing to hold a difference to
    $one = @(Compare-BudgetRounds -Scenario $action -Before @($before[0]) -After $after -BeforeAll $before[0] -AfterAll $after[0])
    ($one | Where-Object metric -eq 'gui_thread_ms').verdict | Should -Be 'unknown'
    ($one | Where-Object metric -eq 'frames').verdict | Should -Be 'same'
  }

  It 'shows the rounds paired beside Welch''s test, which still decides, and says what they alone tell apart' {
    # Both builds swing together round by round, as with the PC's load: each round's after 5 to 7
    # ms over its before, the rounds 480 to 566 apart
    $rounded = { param($r, $ms) $j = & $judgedOf @{ ms = $ms; gui = 2; frames = 1 }; $j.round = $r; $j }
    $bms = @(500, 540, 480, 560, 520); $ams = @(506, 545, 487, 566, 525)
    $before = @(1..5 | ForEach-Object { & $rounded $_ $bms[$_ - 1] })
    $after = @(1..5 | ForEach-Object { & $rounded $_ $ams[$_ - 1] })
    $c = @(Compare-BudgetRounds -Scenario $action -Before $before -After $after -BeforeAll (& $judgedOf @{ ms = 520; gui = 2; frames = 1 }) -AfterAll (& $judgedOf @{ ms = 525.8; gui = 2; frames = 1 }))
    $ms = $c | Where-Object metric -eq 'ms'
    # Welch's: 5.8 apart against +-69.466 (8.0 degrees, so 7's 3.499, times 19.853)
    $ms.within | Should -Be 69.466
    $ms.verdict | Should -Be 'same'
    # Paired: 5.8 apart on average against +-1.723 (4 degrees, 4.604 times 0.374), later in all 5
    $ms.paired_n | Should -Be 5
    $ms.paired_diff | Should -Be 5.8
    $ms.paired_within | Should -Be 1.723
    $ms.paired_verdict | Should -Be 'higher'
    $ms.paired_later | Should -Be 5
    ($c | Where-Object metric -eq 'gui_thread_ms').paired_verdict | Should -Be 'same'
    # A round one build lost pairs with nothing
    $lost = @(Compare-BudgetRounds -Scenario $action -Before $before -After @($after[0..3]) -BeforeAll (& $judgedOf @{ ms = 520; gui = 2; frames = 1 }) -AfterAll (& $judgedOf @{ ms = 526; gui = 2; frames = 1 }))
    ($lost | Where-Object metric -eq 'ms').paired_n | Should -Be 4
    # The table: a paired column, and what the rounds paired alone tell apart, never a verdict
    $report = [ordered]@{
      after = @{ build = 'a1' }; before = @{ build = 'b1' }; rounds = 5; seconds = 60
      scenarios = @([ordered]@{ name = 'atlas-refill'; kind = 'action'; before = @{ result = 'pass' }; after = @{ result = 'pass' }; comparison = $c })
      summary = @{ pass = 1; fail = 0; busy = 0; error = 0; higher = 0; lower = 0 }
    }
    $text = (Format-BudgetComparison $report) -join "`n"
    $text | Should -Match 'metric\s+before\s+after\s+diff\s+\+-99%\s+paired\s+verdict'
    $text | Should -Match 'ms\s+520\s+526\s+\+5\.8\s+69\.5\s+\+-1\.72 higher\s+same \(ms\)'
    $text | Should -Match 'nothing told apart \(99%\)'
    $text | Should -Match 'the rounds paired only \(99%, not a verdict\):\s+atlas-refill ms: 520 to 526 ms, higher in 5 of 5 rounds: 5\.8 apart on average, past \+-1\.72; Welch''s \+-69\.5 calls it the same'
    $text | Should -Not -Match 'nothing measured'
    # A metric no round measured on a build (its window covered) is named, and counted in the
    # last line, never left to read as a pass (PITFALLS 182)
    $report.scenarios[0].before = @{ result = 'pass'; unmeasured = @('missed', 'screen_ms') }
    $report.scenarios[0].after = @{ result = 'pass'; unmeasured = @('missed') }
    $text = (Format-BudgetComparison $report) -join "`n"
    $text | Should -Match 'budgets nothing measured \(not judged\):\s+atlas-refill before: missed, screen_ms\s+atlas-refill after: missed'
    $text | Should -Match "after: 1 passed, 0 over budget, 0 couldn't be measured, 2 with budgets nothing measured;"
  }

  It 'calls rounds that never vary different on any difference, and no difference of under half a count' {
    $rounds = { param($frames, $ms) @(1..3 | ForEach-Object { & $judgedOf @{ ms = $ms; gui = 1; frames = $frames } }) }
    $c = @(Compare-BudgetRounds -Scenario $action -Before (& $rounds 1 10) -After (& $rounds 2 10.001) -BeforeAll (& $judgedOf @{ ms = 10; gui = 1; frames = 1 }) -AfterAll (& $judgedOf @{ ms = 10.001; gui = 1; frames = 2 }))
    $f = $c | Where-Object metric -eq 'frames'
    $f.within | Should -Be 0
    $f.floor | Should -Be 0.5
    $f.verdict | Should -Be 'higher'
    ($c | Where-Object metric -eq 'ms').verdict | Should -Be 'higher'
    # At rest a count is a rate over a span: half a message over 5 s is 0.1 a second
    $rest = [pscustomobject]@{ Name = 'one-tab'; Kind = 'rest' }
    $msg = { param($v) [ordered]@{ metrics = [ordered]@{ messages = [ordered]@{ value = $v } } } }
    $r = @(Compare-BudgetRounds -Scenario $rest -Before @((& $msg 0.68), (& $msg 0.69)) -After @((& $msg 0.78), (& $msg 0.78)) -BeforeAll (& $msg 0.688) -AfterAll (& $msg 0.78) -Seconds 5)
    $r[0].floor | Should -Be 0.1
    $r[0].verdict | Should -Be 'same'
    Get-BudgetFloor 'gui_thread_ms' 'rest' 5 | Should -Be 0
    Get-BudgetFloor 'handles' 'rest' 5 | Should -Be 0.5
    Get-T995 3.87 | Should -Be 5.841
    Get-T995 0.4 | Should -Be 63.657
    Get-T995 11 | Should -Be 3.169
    Get-T995 500 | Should -Be 2.617
  }

  It 'takes the builds in turn, judges each on its rounds together, compares them and writes both to the history' {
    foreach ($b in 'before', 'after') { New-Item -ItemType Directory -Force (Join-Path $TestDrive "bins\$b") | Out-Null; Set-Content (Join-Path $TestDrive "bins\$b\lbterm-gui.exe") '' }
    $file = New-BudgetsFile (Join-Path $TestDrive 'vs') @(@{ name = 'one-tab'; what = 'a tab' }) -Rest @{ seconds = 5; runs = 2; settle_ms = 2000; budgets = @{ gui_thread_ms = @{ max = 2 } } }
    # A stand-in for budgets.ps1: says which build it ran, and leaves spans whose GUI thread is
    # 1 ms/s on the before build and 3 on the after
    $script = Join-Path $TestDrive 'stand-in.ps1'
    Set-Content $script @'
param([string]$Bin, [string]$Scenario, [int]$Runs, [double]$Seconds)
Add-Content $env:BUDGETS_TEST_ORDER (Split-Path $Bin -Leaf)
$base = if ((Split-Path $Bin -Leaf) -eq 'after') { 3 } else { 1 }
$samples = @(1..$Runs | ForEach-Object { [ordered]@{ seconds = 5; frames = 0; gui_thread_ms = $base + 0.01 * $_; cpu_ms = 2 * $base; top = $null; pc = [ordered]@{ others = 0.1; pc = 0.2; clock = 1; ghz = 3; busy = $false } } })
[IO.File]::WriteAllText((Join-Path $env:LBTERM_E2E_OUT 'measure.json'), (@{ scenario = $Scenario; samples = $samples } | ConvertTo-Json -Depth 6))
'@
    $env:BUDGETS_TEST_ORDER = Join-Path $TestDrive 'order.txt'
    $history = Join-Path $TestDrive 'vs\history.jsonl'
    try {
      $out = @(Invoke-BudgetComparison -Bin (Join-Path $TestDrive 'bins\after') -Build 'a1' -Against (Join-Path $TestDrive 'bins\before') -AgainstBuild 'b1' -Rounds 2 -Root (Join-Path $TestDrive 'vs\runs') -File $file -Script $script -History $history -TimeoutMin 1)
    } finally { Remove-Item Env:BUDGETS_TEST_ORDER }
    # The order turned in the second round
    @(Get-Content (Join-Path $TestDrive 'order.txt')) | Should -Be @('before', 'after', 'after', 'before')
    $r = Get-BudgetLastReport
    $r.compare | Should -BeTrue
    $r.rounds | Should -Be 2
    $s = $r.scenarios[0]
    $s.before.result | Should -Be 'pass'
    $s.after.result | Should -Be 'fail'
    $s.after.spans | Should -Be 4
    $s.after.rounds | Should -Be 2
    $gui = $s.comparison | Where-Object metric -eq 'gui_thread_ms'
    $gui.before | Should -Be 1.015
    $gui.after | Should -Be 3.015
    $gui.verdict | Should -Be 'higher'
    $r.summary.fail | Should -Be 1
    $r.summary.higher | Should -Be 2
    $lines = @(Get-Content $history | ConvertFrom-Json)
    @($lines.build) | Should -Be @('b1', 'a1')
    $text = $out -join "`n"
    $text | Should -Match 'round 2 after: fail'
    $text | Should -Match 'told apart \(99%\):\s+one-tab gui_thread_ms: 1\.02 to 3\.02 ms/s, higher'
    $text | Should -Match 'one-tab cpu_ms: 2 to 6 ms/s, higher: the rounds'' means 4 apart, past \+-0'
    $text | Should -Match 'after: 0 passed, 1 over budget, 0 couldn''t be measured; 2 metrics higher and 0 lower, told apart at 99%'
    { Invoke-BudgetComparison -Bin (Join-Path $TestDrive 'bins\after') -Against (Join-Path $TestDrive 'bins\before') -Rounds 1 -File $file } | Should -Throw '*two rounds or more*'
    { Invoke-BudgetComparison -Bin (Join-Path $TestDrive 'bins\after') -Against (Join-Path $TestDrive 'bins\after') -File $file } | Should -Throw '*the build itself*'
  }
}
