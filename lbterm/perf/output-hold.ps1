#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/render/pane_lines.rs, wezterm-gui/src/termwindow/render/pane.rs, wezterm-gui/src/termwindow/render/marks.rs, wezterm-gui/src/termwindow/render/scroll_strip.rs, wezterm-surface/src/line/line.rs, wezterm-surface/src/line/clusterline.rs, mux/src/lbterm_prompts.rs
<#
A frame holds a printing pane's terminal only to copy the rows it shows, and the scroll strip's
prompt marks read each new row cheaply, so the pane's parser goes on taking in output while the
window paints (lbterm/PERF.md, finding 37; render/pane_lines.rs, render/marks.rs).

Until perf/key-latency a frame laid out the rows it showed inside the pane's terminal lock (3.8 ms
a frame of 30 rows at full speed), and the strip's prompt marks worked out every new row's semantic
zones a cell at a time there too (11.7 ms a frame): the parser, which takes the same lock for each
batch of output, waited 6.7 ms a batch at the mean, and 16 MB showed at 7.3 to 7.8 MB/s.

In a local test window like Lachlan's (New-TestConfig: the scroll strip, scrollback_lines 50,000),
its tab a writer that prints -Megabytes of text once a key reaches it: plain lines first, then
-Prompts shell prompts marked with OSC 133, each followed by -PromptEvery lines, all of those in
the scrollback. With lbterm::lines at debug (WEZTERM_LOG) each frame says how many rows it copied
and how long it held the terminal, and each look for prompt marks how many rows it read and how
long it took.

1. The output shown: its time and MB a second, said, not judged (lbterm.ps1 budgets judges
   throughput-local).
2. Every copy held the terminal under -HeldMs at the 95th percentile.
3. The marks' looks over 500 rows or more took under -RowUs a row at the median, waits for the
   terminal included.
4. The strip shows each prompt as a mark of its own, -Prompts of them: a prompt is still found
   among output and in the scrollback, where rows are kept as runs of attributes.
5. The keyboard taken away and given back, which repaints the window and changes no row: those
   frames copy no row anew, each taken from the frame before.
6. A second tab opened, then closed: a pane's copy goes as the pane does (lbterm::lines's `its
   copy dropped`), the second tab's as it closes, while the first tab's stays to be copied from
   when it's shown again, so a long-lived window keeps copies of its open panes alone, where it
   kept a closed pane's until some new pane was copied (the review of perf/key-latency,
   2026-10-31).

A build without lbterm::lines's lines (one from before this) fails 2, 3 and 5 for saying nothing.

  pwsh -File lbterm\perf\output-hold.ps1 [-Megabytes 16] [-Prompts 5] [-PromptEvery 5000] [-HeldMs 2] [-RowUs 1]
#>
param([string]$Bin, [int]$Megabytes = 16, [int]$Prompts = 5, [int]$PromptEvery = 5000, [double]$HeldMs = 2, [double]$RowUs = 1)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'output-hold' -Bin $Bin
$cfg = Join-Path $T.Out 'config\output-hold.lua'
$transcripts = Join-Path $T.Out 'transcripts'
New-Item -ItemType Directory -Force (Split-Path $cfg), $transcripts | Out-Null

# Lines of 104 bytes, about a megabyte a write; the prompts' blocks at the end, so every prompt is
# in the 50,000 rows of scrollback. The title goes as OSC 0 ended by BEL (one ended by ST the
# console host doesn't pass on); OSC 133 as Lachlan's profile and scrollbar-marks.ps1 send it
$writer = @"
`$e = [char]27
`$line = ' the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk'
function Lines([int]`$n) { `$sb = [Text.StringBuilder]::new(); for (`$i = 0; `$i -lt `$n; `$i++) { [void]`$sb.Append(`$i.ToString().PadLeft(6)).Append(`$line).Append("``r``n") }; [Text.Encoding]::UTF8.GetBytes(`$sb.ToString()) }
`$chunk = Lines 10082
`$block = Lines $PromptEvery
`$out = [Console]::OpenStandardOutput()
function Put([byte[]]`$b) { `$out.Write(`$b, 0, `$b.Length) }
function Say([string]`$s) { Put ([Text.Encoding]::UTF8.GetBytes(`$s)) }
function Title([string]`$t) { Say "`$e]0;`$t`$([char]7)"; `$out.Flush() }
Say "ready``r``n"
Title 'starting'
Start-Sleep -Milliseconds 200
Title 'ready'
[void][Console]::ReadKey(`$true)
`$clock = [Diagnostics.Stopwatch]::StartNew()
`$bytes = 0
`$plain = [Math]::Max(0, $Megabytes * 1MB - $Prompts * `$block.Length)
while (`$bytes -lt `$plain) { Put `$chunk; `$bytes += `$chunk.Length }
for (`$p = 1; `$p -le $Prompts; `$p++) {
  Say "`$e]133;A`$e\PS C:\demo> `$e]133;B`$e\run `$p``r``n`$e]133;C`$e\"
  Put `$block; `$bytes += `$block.Length
  Say "`$e]133;D;0`$e\"
}
`$out.Flush()
Title "done `$([int]`$clock.Elapsed.TotalMilliseconds) `$bytes"
Start-Sleep 600
"@

New-TestConfig -Path $cfg -Lua @"
config.initial_cols = 110
config.initial_rows = 30
config.scrollback_lines = 50000
config.window_close_confirmation = 'NeverPrompt'
config.default_cwd = [[$($T.Out)]]
rawset(config, 'lbterm_scroll_strip', { transcripts = [[$transcripts]] })
"@

function Get-Pane($sw) { @($sw.panes | Where-Object active)[0] }
# lbterm::lines's lines since $From in the GUI's log, and its copies among them
function Get-Lines($w, [long]$From) { @(Read-LogLines $w.Log $From | Where-Object { $_ -match 'lbterm::lines\s+> pane \d+: ' } | ForEach-Object { ($_ -split 'lbterm::lines\s+> ', 2)[1] }) }
function ConvertTo-Copies($lines) { @($lines | ForEach-Object { if ($_ -match ': (\d+) rows copied, (\d+) anew, its terminal held ([\d.]+) ms, waited for ([\d.]+) ms') { [pscustomobject]@{ Rows = [int]$Matches[1]; Anew = [int]$Matches[2]; Held = [double]$Matches[3]; Waited = [double]$Matches[4] } } }) }
function Get-PromptMarks($sw) { @($sw.scroll_strip.marks | Where-Object kind -eq 'prompt') }
function Get-Percentile([double[]]$Values, [double]$P) {
  $s = @($Values | Sort-Object)
  if (-not $s.Count) { return $null }
  $s[[int][Math]::Min($s.Count - 1, [Math]::Floor($P * ($s.Count - 1) + 0.5))]
}

$counts = [ordered]@{}
Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Program (New-PwshProgram $writer) -Log 'info,lbterm::lines=debug' -TimeoutSec 30
  Send-Focus $w
  Wait-State { param($s) (Get-Pane $s).title -eq 'ready' -and $s.scroll_strip.mode -eq 'shell' } -What 'the writer ready, the strip the scrollback''s' -TimeoutSec 30 | Out-Null
  Start-Sleep -Milliseconds 500
  $mark = (Get-Item -LiteralPath $w.Log).Length

  # 1. The output, from the key that starts it to the frame with its title saying it's done
  $frame = Get-StateFrame
  $t0 = Get-UnixMs
  Send-Key $w G
  $done = Wait-State { param($s) (Get-Pane $s).title -match '^done ' } -AfterFrame $frame -What 'the writer done' -TimeoutSec 120
  $shownMs = [double](Get-Test).LastState.time_ms - $t0
  $title = ((Get-Pane $done).title -split ' ')
  $bytes = [double]$title[2]
  $counts.output = [ordered]@{ bytes = $bytes; shown_ms = [Math]::Round($shownMs, 1); mb_per_s = [Math]::Round($bytes / 1MB / ($shownMs / 1000), 2); writer_ms = [double]$title[1]; frames = [long]$done.frame - $frame }
  "1. $([Math]::Round($bytes / 1MB, 1)) MB shown in $([Math]::Round($shownMs)) ms, $($counts.output.mb_per_s) MB/s, in $($counts.output.frames) frames; the writer's writes $($title[1]) ms"

  # The marks once the window is quiet
  $final = try { Wait-State { param($s) (Get-PromptMarks $s).Count -ge $Prompts -and $s.settled } -What "$Prompts prompt marks on the strip" -TimeoutSec 15 }
  catch { Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle) }
  Start-Sleep -Milliseconds 300
  $lines = Get-Lines $w $mark
  $copies = @(ConvertTo-Copies $lines)
  $looks = @($lines | ForEach-Object { if ($_ -match ': prompt marks: (\d+) rows looked at in ([\d.]+) ms') { [pscustomobject]@{ Rows = [int]$Matches[1]; Ms = [double]$Matches[2] } } })

  # 2. Each copy held the terminal briefly
  $counts.copies = [ordered]@{ count = $copies.Count; held_p50_ms = (Get-Percentile $copies.Held 0.5); held_p95_ms = (Get-Percentile $copies.Held 0.95); waited_p50_ms = (Get-Percentile $copies.Waited 0.5); waited_p95_ms = (Get-Percentile $copies.Waited 0.95) }
  if (-not $copies.Count) { Add-Failure 'no frame said it copied the pane''s rows (lbterm::lines): a build from before render/pane_lines.rs lays them out inside the pane''s terminal' }
  else {
    "2. $($copies.Count) copies of $((Get-Percentile $copies.Rows 0.5)) rows, $((Get-Percentile $copies.Anew 0.5)) of them anew at the median: the terminal held $($counts.copies.held_p50_ms) ms at the median, $($counts.copies.held_p95_ms) at the 95th percentile; waited for $($counts.copies.waited_p50_ms) and $($counts.copies.waited_p95_ms)"
    if ($counts.copies.held_p95_ms -gt $HeldMs) { Add-Failure "a frame's copy held the pane's terminal $($counts.copies.held_p95_ms) ms at the 95th percentile, over $HeldMs" }
  }

  # 3. The marks' looks read each row cheaply
  $big = @($looks | Where-Object Rows -ge 500)
  $perRow = @($big | ForEach-Object { $_.Ms * 1000 / $_.Rows })
  $counts.marks_looks = [ordered]@{ count = $looks.Count; rows = ($looks | Measure-Object Rows -Sum).Sum; ms = [Math]::Round(($looks | Measure-Object Ms -Sum).Sum, 3); over_500_rows = $big.Count; us_a_row_p50 = $(if ($perRow.Count) { [Math]::Round((Get-Percentile $perRow 0.5), 3) }); ms_p50 = (Get-Percentile $looks.Ms 0.5); ms_p95 = (Get-Percentile $looks.Ms 0.95) }
  if (-not $looks.Count) { Add-Failure 'no look for prompt marks said how long it took (lbterm::lines): a build from before this' }
  elseif (-not $big.Count) { Add-Failure "no look for prompt marks read 500 rows or more ($($looks.Count) looks, $($counts.marks_looks.rows) rows): nothing to judge a row's cost by" }
  else {
    "3. $($looks.Count) looks for prompt marks read $($counts.marks_looks.rows) rows in $($counts.marks_looks.ms) ms; those of 500 rows or more $($counts.marks_looks.us_a_row_p50) us a row at the median; a look $($counts.marks_looks.ms_p50) ms at the median, $($counts.marks_looks.ms_p95) at the 95th percentile"
    if ($counts.marks_looks.us_a_row_p50 -gt $RowUs) { Add-Failure "the prompt marks' looks took $($counts.marks_looks.us_a_row_p50) us a row at the median, over $RowUs" }
  }

  # 4. Each prompt a mark of its own
  $found = Get-PromptMarks $final
  $counts.prompt_marks = $found.Count
  "4. $($found.Count) prompt marks on the strip, at y $(@($found | ForEach-Object { $_.rect.y }) -join ', ')"
  if ($found.Count -ne $Prompts) { Add-Failure "the strip shows $($found.Count) prompt marks, where $Prompts prompts were printed among the output" }

  # 5. Frames that change no row copy none anew
  $mark = (Get-Item -LiteralPath $w.Log).Length
  $frame = Get-StateFrame
  Send-Focus $w -Off
  Wait-State { param($s) $true } -AfterFrame $frame -What 'a frame without the keyboard' -TimeoutSec 5 | Out-Null
  $frame = Get-StateFrame
  Send-Focus $w
  Wait-State { param($s) $true } -AfterFrame $frame -What 'a frame with it again' -TimeoutSec 5 | Out-Null
  Start-Sleep -Milliseconds 300
  $still = @(ConvertTo-Copies (Get-Lines $w $mark))
  $counts.unchanged = [ordered]@{ copies = $still.Count; rows = ($still | Measure-Object Rows -Sum).Sum; anew = ($still | Measure-Object Anew -Sum).Sum; held_ms = @($still.Held) }
  "5. the keyboard away and back: $($still.Count) copies of $($counts.unchanged.rows) rows, $($counts.unchanged.anew) anew, the terminal held $(@($still.Held) -join ', ') ms"
  if (-not $still.Count) { Add-Failure 'no copy said for the frames of the keyboard going and coming back' }
  elseif ($counts.unchanged.anew) { Add-Failure "frames that changed no row copied $($counts.unchanged.anew) rows anew, where the last frame's copies are the same" }

  # 6. A pane's copy goes with the pane: the second tab's as it closes, the first tab's kept while
  # the second is shown
  $first = @(Get-Panes | Where-Object { $_.is_active })[0].pane_id
  $mark = (Get-Item -LiteralPath $w.Log).Length
  $second = New-Tab -From $first -Program (New-PwshProgram 'Write-Host second; Start-Sleep 600')
  Wait-Change { param($s) @($s.tabs).Count -eq 2 -and (Get-Pane $s).title -notmatch '^done ' } -What 'the second tab shown' -Animating -TimeoutSec 15 | Out-Null
  Invoke-Cli kill-pane --pane-id $second -Check | Out-Null
  Wait-Change { param($s) @($s.tabs).Count -eq 1 } -What 'the second tab closed' -Animating -TimeoutSec 15 | Out-Null
  Start-Sleep -Milliseconds 300
  $dropped = @(Get-Lines $w $mark | Where-Object { $_ -match ': its copy dropped' } | ForEach-Object { [int]([regex]::Match($_, '^pane (\d+):').Groups[1].Value) })
  $counts.dropped = [ordered]@{ first = $first; second = $second; dropped = $dropped }
  "6. a second tab shown and closed: copies dropped of panes $(if ($dropped) { $dropped -join ', ' } else { 'none' }) (the first $first, the second $second)"
  if ($dropped -contains [int]$first) { Add-Failure "the first tab's copy was dropped though its pane is open: dropped $($dropped -join ', ')" }
  if ($dropped -notcontains [int]$second) { Add-Failure "the second tab's copy wasn't dropped once it closed: dropped $($dropped -join ', ')" }

  $counts | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'counts.json')
} -Passed "output-hold: the rows copied, the marks read cheaply and found, unchanged rows not copied again, and a closed pane's copy dropped"
