#Requires -Version 7
# lbterm\bench\pty-frames.ps1: a recording's summary (mux/tests/lbterm_pty_frames.rs writes it) said
# phase by phase, with the conpty host's counters from claude-output.ps1's cost.json beside it

BeforeAll {
  $script:Script = Join-Path $PSScriptRoot '..\bench\pty-frames.ps1'
  $script:Dist = { param($p50, $p90, $max) [ordered]@{ n = 3; mean = 1.0; p50 = $p50; p90 = $p90; p99 = $max; max = $max } }
  $script:Rows = {
    param($n, $same, $inPlace, $scroll)
    [ordered]@{ n = $n; kinds = [ordered]@{ same = $same; 'in place' = $inPlace; scroll = $scroll }; rows = 920; changed = 40; blank = 3; seen_here = 1; moved = 30; seen_elsewhere = 1; new = 5; laid_out_by_row = 39; laid_out_by_content = 5; scroll_shifts = @{ '1' = $scroll }; in_scrolls = @{} }
  }
  $script:Phase = {
    [ordered]@{
      seconds = 12.5; reads = 400; bytes = 512000; reads_per_s = 32.0; bytes_per_s = 40960
      read_bytes = (& $Dist 900 4000 8192); read_gaps_us = [ordered]@{ '0-50' = 10; '50-100' = 0; '20000-50000' = 390 }
      frames = 380; frames_per_s = 30.4; frame_reads = (& $Dist 1 2 3); frame_bytes = (& $Dist 1000 3000 9000)
      frame_span_us = (& $Dist 0 80 400); frame_max_gap_us = (& $Dist 40 90 300); frame_gap_before_us = (& $Dist 31000 33000 90000)
      bracketed_frames = 370
      sync = [ordered]@{ set = 380; reset = 380; query = 1; frames = 380; frame_reads = (& $Dist 1 2 3); frame_span_us = (& $Dist 0 80 400); frame_max_gap_us = (& $Dist 0 80 400) }
      seen = [ordered]@{ line_feeds = 5; scroll_ops = 7; margins = 7; cursor = 900; erases = 300; sgr = 2000; printed = 90000; osc = 12; alt_screen = 1 }
      batches = (& $Rows 760 380 200 180); batch_wait_us = (& $Dist 0 3000 3600); batch_edges = 760; batch_dirty = (& $Dist 1 80 92)
      ticks = (& $Rows 300 10 150 140)
    }
  }
}

Describe 'pty-frames.ps1 -Print' {
  It 'says each phase of each recording, the conpty host''s counters beside its pane''s' {
    $out = Join-Path $TestDrive 'run'
    $rec = Join-Path $out 'ptyrec'
    New-Item -ItemType Directory -Force $rec | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $rec 'pane-9-0.ptyrec'), [byte[]]@(1, 2, 3))
    [ordered]@{
      pid = 9; pane = 0; start_unix_ns = 1; cut_short = $false; frame_gap_us = 2000; coalesce_us = 3000
      sizes_ms_rows_cols = @(, @(0.0, 92, 110))
      phases = [ordered]@{ all = (& $Phase); streaming = (& $Phase) }
    } | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $rec 'pane-9-0.summary.json')
    [ordered]@{
      pane = @{ id = 0 }
      phases = @{ streaming = @{ conpty = @{ writes = 401; write_bytes = 512000; others = 420; cpu_ms = 18.5 } } }
    } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $out 'cost.json')

    $lines = @(& $Script -Recording $rec -Print)
    $lines[0] | Should -Be '== pane-9-0: pane 0, 110 by 92 at 0 ms'
    $text = $lines -join "`n"
    $text | Should -Match '(?m)^  streaming: 12\.5 s, 400 reads \(32\.0 a second\), 512000 B'
    # The counters only where the cost file has the phase
    @($lines | Where-Object { $_ -match 'the conpty host: 401 pipe writes \(512000 B\), 420 console messages, 18\.5 ms' }).Count | Should -Be 1
    $text | Should -Match 'gaps between reads, us: 0-50 10, 20000-50000 390'
    $text | Should -Match 'frames \(reads under 2000 us apart\): 380, 30\.4 a second; reads 1 / 2 / 3'
    $text | Should -Match '380 DECSET 2026, 380 DECRST 2026, 1 DECRQM 2026; 380 frames bracketed'
    $text | Should -Match 'batches as the parser makes them: 760: 380 same, 200 in place, 180 scrolls; rows changed 40 of 920: blank 3, seen here 1, moved 30, seen elsewhere 1, new 5; laid out again keyed by row 39, by content 5'
    $text | Should -Match 'frames at most one a 16\.7 ms: 300: 10 same, 150 in place, 140 scrolls'
  }

  It 'refuses a folder with no recording in it' {
    $empty = Join-Path $TestDrive 'empty'
    New-Item -ItemType Directory -Force $empty | Out-Null
    { & $Script -Recording $empty -Print } | Should -Throw '*no .ptyrec*'
  }
}
