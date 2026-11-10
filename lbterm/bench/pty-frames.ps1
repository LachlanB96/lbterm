#Requires -Version 7
<#
What a folder of pty recordings says, frame by frame (lbterm/FLOWS.md, the pane-output flow): the
recordings a mux server makes with LBTERM_PTY_RECORD set (mux/src/lbterm_ptyrec.rs), as
lbterm/perf/claude-output.ps1 makes them of real Claude Code in its output folder's `ptyrec`.

It runs mux/tests/lbterm_pty_frames.rs on them (`lbterm.ps1 test -Package mux` with
LBTERM_PTY_FRAMES set, so start it in a warden job: cargo runs), which plays the mux parser
thread's batching over each recording's reads and performs every batch on a terminal of the pane's
size, and writes beside each recording `<name>.frames.tsv` (a line a frame: reads less than
-FrameGapUs apart), `<name>.batches.tsv` (a line a batch the parser would have performed, with what
it did to the screen) and `<name>.summary.json` (the whole recording and each phase the folder's
marks.tsv names). Then it prints each recording's phases, with the conpty host's counters from the
cost.json beside the folder when there is one. -Print prints the summaries already written, with no
cargo.

  warden-run --name pty-frames --cpus 4 --mem 16 --max 1h -- pwsh -NoProfile -File lbterm\bench\pty-frames.ps1 -Recording <folder> [-FrameGapUs 2000]
  pwsh -File lbterm\bench\pty-frames.ps1 -Recording <folder> -Print
#>
param(
  [Parameter(Mandatory)][string]$Recording,
  # Reads closer than this are one frame
  [int]$FrameGapUs = 2000,
  [switch]$Print
)
$ErrorActionPreference = 'Stop'
$Recording = (Resolve-Path -LiteralPath $Recording).Path
if (-not @(Get-ChildItem -LiteralPath $Recording -Filter '*.ptyrec')) { throw "no .ptyrec in $Recording" }

if (-not $Print) {
  $env:LBTERM_PTY_FRAMES = $Recording
  $env:LBTERM_PTY_FRAME_GAP_US = [string]$FrameGapUs
  try {
    & 'C:/Program Files/PowerShell/7/pwsh.exe' -NoProfile -File (Join-Path $PSScriptRoot '..\lbterm.ps1') test -Package mux
    if ($LASTEXITCODE) { throw "lbterm.ps1 test -Package mux exited $LASTEXITCODE" }
  } finally {
    Remove-Item Env:LBTERM_PTY_FRAMES, Env:LBTERM_PTY_FRAME_GAP_US -ErrorAction SilentlyContinue
  }
}

# A distribution as "p50 / p90 / max"
function Format-Dist($d) { if (-not $d -or -not $d.n) { '-' } else { "$($d.p50) / $($d.p90) / $($d.max)" } }
function Format-Rows($c) {
  if (-not $c -or -not $c.n) { return '-' }
  "$($c.n): $($c.kinds.same) same, $($c.kinds.'in place') in place, $($c.kinds.scroll) scrolls; rows changed $($c.changed) of $($c.rows): blank $($c.blank), seen here $($c.seen_here), moved $($c.moved), seen elsewhere $($c.seen_elsewhere), new $($c.new); laid out again keyed by row $($c.laid_out_by_row), by content $($c.laid_out_by_content)"
}

$cost = $null
$costFile = Join-Path (Split-Path $Recording) 'cost.json'
if (Test-Path -LiteralPath $costFile) { $cost = Get-Content -Raw -LiteralPath $costFile | ConvertFrom-Json }
foreach ($file in Get-ChildItem -LiteralPath $Recording -Filter '*.summary.json' | Sort-Object Name) {
  $s = Get-Content -Raw -LiteralPath $file.FullName | ConvertFrom-Json
  $sizes = @($s.sizes_ms_rows_cols | ForEach-Object { "$($_[2]) by $($_[1]) at $($_[0]) ms" }) -join ', '
  "== $($file.Name -replace '\.summary\.json$', ''): pane $($s.pane), $sizes$(if ($s.cut_short) { ', its last record cut short' })"
  foreach ($p in $s.phases.PSObject.Properties) {
    $v = $p.Value
    ''
    "  $($p.Name): $($v.seconds) s, $($v.reads) reads ($('{0:N1}' -f $v.reads_per_s) a second), $($v.bytes) B ($('{0:N0}' -f $v.bytes_per_s) B a second)"
    $c = if ($cost -and $cost.phases.PSObject.Properties[$p.Name]) { $cost.phases.($p.Name).conpty }
    if ($c -and $s.pane -eq $cost.pane.id) { "    the conpty host: $($c.writes) pipe writes ($($c.write_bytes) B), $($c.others) console messages, $($c.cpu_ms) ms of CPU" }
    "    reads, bytes p50 / p90 / max: $(Format-Dist $v.read_bytes)"
    "    gaps between reads, us: $(($v.read_gaps_us.PSObject.Properties | Where-Object Value | ForEach-Object { "$($_.Name) $($_.Value)" }) -join ', ')"
    "    frames (reads under $($s.frame_gap_us) us apart): $($v.frames), $('{0:N1}' -f $v.frames_per_s) a second; reads $(Format-Dist $v.frame_reads), bytes $(Format-Dist $v.frame_bytes), span us $(Format-Dist $v.frame_span_us), largest gap inside us $(Format-Dist $v.frame_max_gap_us), gap before us $(Format-Dist $v.frame_gap_before_us)"
    "    synchronized output: $($v.sync.set) DECSET 2026, $($v.sync.reset) DECRST 2026, $($v.sync.query) DECRQM 2026; $($v.sync.frames) frames bracketed, reads $(Format-Dist $v.sync.frame_reads), span us $(Format-Dist $v.sync.frame_span_us), largest gap us $(Format-Dist $v.sync.frame_max_gap_us); frames that open and close in it: $($v.bracketed_frames)"
    "    in the stream: $($v.seen.line_feeds) line feeds, $($v.seen.scroll_ops) scrolls asked (SU, SD, IL, DL, RI), $($v.seen.margins) DECSTBM, $($v.seen.cursor) cursor moves, $($v.seen.erases) erases, $($v.seen.sgr) SGR, $($v.seen.printed) characters, $($v.seen.osc) OSC, $($v.seen.alt_screen) alternate screen"
    "    batches as the parser makes them: $(Format-Rows $v.batches); waited us $(Format-Dist $v.batch_wait_us), $($v.batch_edges) at a synchronized edge, rows dirty $(Format-Dist $v.batch_dirty)"
    "    frames at most one a 16.7 ms: $(Format-Rows $v.ticks)"
  }
  ''
}
