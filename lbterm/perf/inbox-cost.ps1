#Requires -Version 7
# Variant: -Mux
# Covers: wezterm-gui/src/inbox.rs, wezterm-gui/src/termwindow/render/inbox.rs, lbterm-extras/src/inbox.rs, wezterm-gui/src/termwindow/render/strip_swap.rs, wezterm-gui/src/termwindow/render/lights.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/quad.rs, wezterm-gui/src/renderstate.rs, wezterm-gui/src/termwindow/webgpu.rs, lbterm-motion/src/content.rs, mux/src/localpane.rs
<#
What an InMail message arriving or being archived costs (lbterm/INBOX.md, "What a change costs"),
in a test window that looks like Lachlan's LBTerm (New-TestConfig) with four panes of coloured text
and an InMail folder of its own; with -Mux its tabs live in a test lbterm-mux-server, as his do.
Showing a change takes one frame, and that frame needs to draw only the visuals it changes: the
strip's (`over`), the swap's dissolve and, the first time, the lights' swatch.

1. At rest the window paints nothing, and the InMail thread (lbterm-inbox) doesn't run: it
   watches the folders and sleeps until something in them changes (it looked every second until
   2026-10-30, so a message showed up to a second late).
2. A message is written and removed -Changes times, -GapMs apart. Each change has to show the
   button's new count, then the window settles (the state file's frame after DWM's dissolve
   ends). For each: how soon the button said so, the frames painted, which visuals each drew and
   how many it kept (the state file's `visuals`), the GUI process's CPU over the gap
   (QueryProcessCycleTime, exact), the slow log's paints (LBTERM_SLOW_EVERY, lbterm_slow_ms = 1),
   and the threads that started and ran.
3. It fails when a change paints more than two frames (its own and the state file's settling one),
   when the change's frame draws more than four visuals or any but the strip's (`Over`), the
   swap's dissolve (`Swap`) and the lights' swatch (`Swatch`), a pane's among them, when the
   settling frame draws any, when a change after the first starts a thread that runs 5 ms or more
   (a process walk per pane: mux/src/localpane.rs), when the button shows the changes more than
   -ShowBudgetMs after them at the median, or when the changes after the first cost more than
   -CpuBudgetMs of GUI CPU at the median: 60 ms in a window of its own, 50 on a mux domain, from
   what's measured below. Measured on 2026-10-30 before these fixes: three frames an arrival and
   two a removal, every one drawing all 24 visuals, a walk of every process per pane, 125 to 440
   ms a change, a median of 186 (45 to 117 ms on a mux domain, a median of 50, 72 and 77 in three
   runs); after: 27 to 44 ms, medians of 30 and 36 (28 to 47, medians of 33 and 39); the button
   39 to 1042 ms after the file was written while the thread looked every second, a median of
   293 to 505, and 46 to 89 ms once it watched the folders, a median of 57 to 63.

cost.json in target\perf\inbox-cost has every number.

  pwsh -File lbterm\perf\inbox-cost.ps1 [-Mux] [-Changes 3] [-Bin <build>]
#>
param([string]$Bin, [switch]$Mux, [int]$Changes = 3, [int]$GapMs = 1500, [double]$CpuBudgetMs = 0, [double]$ShowBudgetMs = 200)
# A mux domain's GUI does less of a change's work itself (its panes are the server's)
if (-not $CpuBudgetMs) { $CpuBudgetMs = if ($Mux) { 50 } else { 60 } }
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test ('inbox-cost' + $(if ($Mux) { '-mux' } else { '' })) -Bin $Bin

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermCost {
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr h, out IntPtr desc);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
  const uint THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000;
  // Cycles a second, spinning on this thread for ms: the cycle counter's rate
  public static double Rate(int ms) {
    ulong a, b; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out a);
    while (sw.ElapsedMilliseconds < ms) { }
    QueryThreadCycleTime(GetCurrentThread(), out b);
    return (b - a) / sw.Elapsed.TotalSeconds;
  }
  // Every thread of the process, exited ones included
  public static ulong ProcessCycles(int pid) {
    IntPtr h = OpenProcess(PROCESS_QUERY_LIMITED, false, (uint)pid);
    if (h == IntPtr.Zero) return 0;
    try { ulong c; QueryProcessCycleTime(h, out c); return c; } finally { CloseHandle(h); }
  }
  // Each live thread's cycles
  public static Dictionary<int, ulong> Threads(int pid) {
    var d = new Dictionary<int, ulong>();
    using (var p = Process.GetProcessById(pid)) {
      foreach (ProcessThread t in p.Threads) {
        IntPtr h = OpenThread(THREAD_QUERY_LIMITED, false, (uint)t.Id);
        if (h == IntPtr.Zero) continue;
        try { ulong c; if (QueryThreadCycleTime(h, out c)) d[t.Id] = c; } finally { CloseHandle(h); }
      }
    }
    return d;
  }
  public static string Name(int id) {
    IntPtr h = OpenThread(THREAD_QUERY_LIMITED, false, (uint)id);
    if (h == IntPtr.Zero) return "";
    try {
      IntPtr d;
      if (GetThreadDescription(h, out d) < 0 || d == IntPtr.Zero) return "";
      string s = Marshal.PtrToStringUni(d); LocalFree(d); return s ?? "";
    } finally { CloseHandle(h); }
  }
}
'@

$inbox = Join-Path $T.Out 'inbox'
Remove-Item -Recurse -Force $inbox -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $inbox | Out-Null
$message = Join-Path $inbox '2026-10-30-0900-from-Home-the-cost-of-a-message.md'
# Sixty lines of coloured words, then quiet: what a busy pane leaves on its screen
$text = New-PwshProgram -Script "`$e = [char]27; `$w = 'alpha','bravo','charlie','delta','echo','foxtrot','golf','hotel'; foreach (`$r in 1..60) { `$line = ''; foreach (`$i in 1..22) { `$line += `"`$e[38;5;`$((`$r * 7 + `$i * 13) % 230 + 17)m`$(`$w[(`$r + `$i) % 8]) `" }; Write-Host (`$line + `"`$e[0m`") }; Start-Sleep 900"
$cfg = Join-Path $T.Out 'config\inbox-cost.lua'
New-Item -ItemType Directory -Force (Split-Path $cfg) | Out-Null
$domain = if ($Mux) {
  @"

config.unix_domains = { {
  name = 'costmux',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
New-TestConfig -Path $cfg -Lua @"
config.lbterm_inbox = { folders = { '$(ConvertTo-LuaPath $inbox)' } }
config.lbterm_slow_ms = 1
config.initial_cols = 160
config.initial_rows = 50
$domain
"@

$rows = [Collections.Generic.List[object]]::new()
Invoke-Test {
  $open = @{ ConfigFile = $cfg; Program = $text; Environment = @{ LBTERM_SLOW_EVERY = '1' }; TimeoutSec = 20 }
  if ($Mux) { $open.Domain = 'costmux'; $open.Socket = $T.Sock }
  $w = Open-TestWindow @open
  Wait-State { param($s) $s.frame -gt 0 } -What 'the window''s first frame' | Out-Null
  # Three more panes of the same, then quiet
  $panes = @(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not $panes -and $clock.Elapsed.TotalSeconds -lt 15) {
    $panes = @(Get-Panes -Server:$Mux)
    if (-not $panes) { Start-Sleep -Milliseconds 200 }
  }
  if (-not $panes) { throw "cli list$(if ($Mux) { ' on the test server' }) has no panes; the window's log: $($w.Log)" }
  $first = $panes[0].pane_id
  foreach ($dir in 'right', 'bottom', 'bottom') {
    Invoke-Cli split-pane --pane-id $first "--$dir" -Server:$Mux -Program $text -Check | Out-Null
  }
  Wait-Change { param($s) @($s.panes).Count -eq 4 } -What 'four panes' | Out-Null
  Start-Sleep 3
  Wait-Settled -What 'four panes at rest' | Out-Null
  # The window's first frame's draw, for the record: the slowest paint in the log, not a change's
  $firstPaint = @(Get-LogLines $w 'slow paint window-\d+/draw: [\d.]+ ms' | ForEach-Object { if ($_ -match 'draw: ([\d.]+) ms') { $Matches[1] } }) | Select-Object -First 1

  $rate = [LbtermCost]::Rate(300)
  $ms = { param([ulong]$cycles) [Math]::Round($cycles / $rate * 1000, 1) }
  $frameNow = { [long](Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).frame }
  $logAt = { (Get-Item $w.Log).Length }
  $logSince = { param($from) $fs = [IO.File]::Open($w.Log, 'Open', 'Read', 'ReadWrite, Delete'); try { [void]$fs.Seek($from, 'Begin'); [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() } }

  # 1. At rest, and the InMail thread asleep
  $inboxThread = @(([LbtermCost]::Threads($w.Pid)).Keys | Where-Object { [LbtermCost]::Name($_) -eq 'lbterm-inbox' })[0]
  if (-not $inboxThread) { Add-Failure 'no thread named lbterm-inbox in the GUI' }
  $f0 = & $frameNow; $c0 = [LbtermCost]::ProcessCycles($w.Pid); $t0 = ([LbtermCost]::Threads($w.Pid))[$inboxThread]
  Start-Sleep -Milliseconds ($GapMs * 2)
  $restFrames = (& $frameNow) - $f0; $restCpu = & $ms ([LbtermCost]::ProcessCycles($w.Pid) - $c0)
  $inboxRan = if ($inboxThread) { ([LbtermCost]::Threads($w.Pid))[$inboxThread] - $t0 } else { 0 }
  "rest: $restFrames frames in $($GapMs * 2) ms, GUI CPU $restCpu ms, the InMail thread $inboxRan cycles"
  if ($restFrames -gt 0) { Add-Failure "at rest the window painted $restFrames frames in $($GapMs * 2) ms" }
  if ($inboxRan -gt 0) { Add-Failure "at rest the InMail thread ran ($inboxRan cycles in $($GapMs * 2) ms): it's to sleep until a folder changes" }

  # 2. The changes
  $n = 0
  foreach ($i in 1..$Changes) {
    foreach ($kind in 'arrives', 'is archived') {
      $n++
      $want = if ($kind -eq 'arrives') { 1 } else { 0 }
      $f0 = & $frameNow; $l0 = & $logAt
      $c0 = [LbtermCost]::ProcessCycles($w.Pid)
      $before = [LbtermCost]::Threads($w.Pid); $seen = @{}; foreach ($k in $before.Keys) { $seen[$k] = $before[$k] }
      $clock = [Diagnostics.Stopwatch]::StartNew()
      if ($want) {
        [IO.File]::WriteAllText($message, "---`nfrom: Home`nto: Terminal`nsent: 2026-10-30T09:00`nsubject: `"The cost of a message $i`"`n---`n`nThe body.`n")
      } else { Remove-Item -LiteralPath $message }
      $shownAt = $null; $settledAt = $null; $last = $f0; $visuals = [Collections.Generic.List[object]]::new()
      while ($clock.ElapsedMilliseconds -lt $GapMs -or $null -eq $settledAt) {
        if ($clock.ElapsedMilliseconds -gt $GapMs + 4000) { break }
        foreach ($e in ([LbtermCost]::Threads($w.Pid)).GetEnumerator()) { $seen[$e.Key] = $e.Value }
        $s = Get-StateSnapshot $w.StateFile
        $sw = if ($s) { Get-StateWindow $s -Hwnd ([long]$w.Handle) }
        if ($sw -and $sw.frame -ne $last) {
          $last = $sw.frame
          $visuals.Add($sw.visuals)
          if ($null -eq $shownAt -and $null -ne $sw.strip -and [int]$sw.strip.inbox -eq $want) { $shownAt = $clock.ElapsedMilliseconds }
          if ($null -ne $shownAt -and $null -eq $settledAt -and -not $sw.animating) { $settledAt = $clock.ElapsedMilliseconds }
        }
        Start-Sleep -Milliseconds 5
      }
      $frames = (& $frameNow) - $f0
      $cpu = & $ms ([LbtermCost]::ProcessCycles($w.Pid) - $c0)
      $log = & $logSince $l0
      $paints = @([regex]::Matches($log, 'slow paint window-\d+/(\w+): ([\d.]+) ms') | ForEach-Object { [pscustomobject]@{ Part = $_.Groups[1].Value; Ms = [double]$_.Groups[2].Value } })
      # Threads that started during the change and ran 5 ms or more
      $started = @(foreach ($id in @($seen.Keys | Where-Object { -not $before.ContainsKey($_) })) {
        $ran = & $ms $seen[$id]
        if ($ran -ge 5) {
          $name = [LbtermCost]::Name($id)
          "$(if ($name) { $name } else { "thread $id" }) $ran ms"
        }
      })
      $drawn = @($visuals | ForEach-Object { if ($_) { "$($_.drawn)/$($_.drawn + $_.kept)$(if ($_.which) { ' (' + (@($_.which) -join ', ') + ')' })" } else { '?' } })
      $row = [pscustomobject]@{
        Change = $n; Kind = $kind; ShownMs = $shownAt; SettledMs = $settledAt; Frames = $frames
        Visuals = $drawn; CpuMs = $cpu; Threads = $started
        SlowestPass = ($paints | Where-Object Part -eq 'pass' | Measure-Object Ms -Maximum).Maximum
        SlowestDraw = ($paints | Where-Object Part -eq 'draw' | Measure-Object Ms -Maximum).Maximum
      }
      $rows.Add($row)
      "change ${n}: a message $kind; shown at $shownAt ms, settled at $settledAt ms; $frames frames, visuals drawn/all $($drawn -join ' '); GUI CPU $cpu ms; slowest pass $($row.SlowestPass) ms, draw $($row.SlowestDraw) ms$(if ($started) { "; threads started: $($started -join ', ')" })"

      # 3. What it may cost
      if ($null -eq $shownAt) { Add-Failure "change ${n}: the button never said $want"; continue }
      if ($frames -gt 2) { Add-Failure "change ${n} (a message $kind) painted $frames frames: one shows it, and the state file asks for one more when DWM's dissolve ends" }
      if ($n -gt 1 -and $started) { Add-Failure "change ${n} started threads that ran: $($started -join ', ') (a process walk per pane?)" }
      $known = @($visuals | Where-Object { $_ })
      if ($known.Count -lt $visuals.Count -or -not $known) { Add-Failure "change ${n}: the state file doesn't say which visuals its frames drew (a build before 'Only what changed is drawn')"; continue }
      $change = $known[0]
      if ($change.drawn -gt 4) { Add-Failure "change ${n}: its frame drew $($change.drawn) visuals of $($change.drawn + $change.kept): the strip's, the swap's and the lights' are all it changes" }
      if ($null -eq $change.which) { Add-Failure "change ${n}: the state file doesn't name the visuals its frame drew"; continue }
      $others = @(@($change.which) | Where-Object { $_ -notin 'Over', 'Swap', 'Swatch' })
      if ($others) { Add-Failure "change ${n}: its frame drew $($others -join ', ') as well: the strip's (Over), the swap's and the lights' swatch are all it changes" }
      if ('Over' -notin @($change.which)) { Add-Failure "change ${n}: its frame didn't draw the strip's visual (Over), which shows the button: $(@($change.which) -join ', ')" }
      foreach ($later in @($known | Select-Object -Skip 1)) {
        if ($later.drawn -gt 0) { Add-Failure "change ${n}: the frame after the dissolve drew $($later.drawn) visuals: nothing changed in it" }
      }
    }
  }
  $later = @($rows | Select-Object -Skip 1 | ForEach-Object CpuMs | Sort-Object)
  $median = if ($later) { $later[[int][Math]::Floor(($later.Count - 1) / 2)] } else { $null }
  "GUI CPU a change after the first: median $median ms (budget $CpuBudgetMs), $($later -join ', ') ms; the first $($rows[0].CpuMs) ms; the window's first draw $(if ($firstPaint) { "$firstPaint ms" } else { 'not logged' })"
  if ($null -ne $median -and $median -gt $CpuBudgetMs) { Add-Failure "a change costs $median ms of GUI CPU at the median, over the budget of $CpuBudgetMs ms" }
  $shown = @($rows | Where-Object { $null -ne $_.ShownMs } | ForEach-Object ShownMs | Sort-Object)
  $showMedian = if ($shown) { $shown[[int][Math]::Floor(($shown.Count - 1) / 2)] } else { $null }
  "the button showed a change $($shown -join ', ') ms after it: median $showMedian ms (budget $ShowBudgetMs)"
  if ($null -ne $showMedian -and $showMedian -gt $ShowBudgetMs) { Add-Failure "the button shows a change $showMedian ms after it at the median, over $ShowBudgetMs ms: is the InMail thread polling?" }
  [pscustomobject]@{
    Mux = [bool]$Mux; RestFrames = $restFrames; RestCpuMs = $restCpu; RestInboxCycles = $inboxRan; GapMs = $GapMs
    MedianCpuMs = $median; MedianShownMs = $showMedian; Changes = $rows
  } | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $T.Out 'cost.json')
} -Passed "inbox-cost$(if ($Mux) { ' (mux)' }): an InMail change shows at once and paints one frame, drawing only the strip's visuals, and costs what showing it needs; at rest nothing runs"
