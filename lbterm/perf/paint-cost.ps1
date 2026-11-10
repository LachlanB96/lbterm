#Requires -Version 7
# Variant: -Local
# Variant: -Spinner -IntervalMs 100
# Variant: -Spinner -IntervalMs 100 -DrawAll
# Covers: wezterm-gui/src/termwindow/render/card.rs, wezterm-gui/src/termwindow/render/composited.rs, wezterm-gui/src/termwindow/render/draw.rs, wezterm-gui/src/termwindow/render/paint.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/termwindow/render/conductor.rs, wezterm-gui/src/termwindow/webgpu.rs, wezterm-gui/src/termwindow/compositor.rs, wezterm-gui/src/renderstate.rs, wezterm-gui/src/quad.rs, wezterm-gui/src/termwindow/state.rs, lbterm-extras/src/state.rs, wezterm-gui/src/termwindow/gpu_work.rs, lbterm-motion/src/spans.rs, wezterm-gui/src/termwindow/render/kept.rs, wezterm-gui/src/termwindow/render/kept_quads.rs, lbterm-motion/src/content.rs
<#
What painting a frame costs in a window like Lachlan's (lbterm/COMPOSITOR.md, "What a frame
costs"): his look (New-TestConfig), 1080 by 1920 at 96 DPI as his window is on his portrait
screen, three tabs on a mux domain of the test's own (-Local: the GUI's own), the Conductor's pad
accepting most against a hub played here thinking about one tab's session and acting on
another's, so three of its outlines show, and the open tab printing a line of colour every
-IntervalMs for -Seconds, -Runs times, after a screenful printed first. With -Spinner the open tab
writes its last line again in place every -IntervalMs instead, as Claude Code's spinner and timer
do while it works: one row changes, nothing scrolls. With -DrawAll (LBTERM_DRAW_ALL in the
window's environment) every visual is drawn in every frame, kept or not, as a frame that changes
them all draws them (a resize, a new palette, the first): what drawing them costs, frame after
frame.

For each run: the frames it painted (the state file's count); the GUI thread's and the process's
CPU by their cycles, a frame; with metrics (a build since perf/budgets), the paint's parts a frame
(pass: building every quad; draw: into the visuals' swapchains; present); from every frame the
state file wrote (Start-FrameRecorder), the visuals each drew and kept, and from a build that says
so, what each frame made on the GPU (`visuals.made`: buffers and bind groups), the vertex buffers
it wrote (`uploads`, `upload_bytes`) and the templates it built (`built`), and the render passes
it recorded, the submits it made of them and how long recording and submitting took (`passes`,
`submits`), and how long gathering what each visual draws (writing the layers' GPU copies),
recording the passes and submitting them took (`upload_us`, `record_us`, `submit_us`); and at its end the GUI's dedicated GPU memory and private bytes.

It fails, on a build that says what its frames made and built, when a frame builds a template
again (the card templates, the Conductor's outlines and their light, the panel's glow, lit ring
and running light change only with the window's size, its DPI, the palette, the rail's layout or
where an outline goes: render/kept.rs); when more than two frames a run make GPU buffers or bind
groups, or one makes more than eight (each layer keeps one vertex buffer, made again only when
what's on screen outgrows it, and the bind groups are kept while what they hold is the same:
termwindow/gpu_work.rs); when a frame writes a vertex buffer and draws no visual; or, on a build
that says what it recorded, when a frame records other than one render pass a visual it draws, or
other than a submit for each eight of them, none when it draws none (render/composited.rs); or, on
a build that checks (`visuals.stale`, with LBTERM_CHECK_UPLOADS in the window's environment, which
it sets unless -Measure), when a write to the GPU leaves out a quad whose hash matched what the
GPU's copy held but whose vertices didn't (a write its hash doesn't fold in: quad.rs). With
-Spinner it fails when more than five frames a run write over 64 KB to the GPU, a layer made again
aside: one row changes, so a frame writes that row's quads, not a layer's
(lbterm_motion::content::dirty_runs); the command typed and its Enter scrolling the full screen
once are the few that write every line.

After the runs it changes what the templates are made from, one thing at a time, and fails when
the frames that show a change don't make again every template made from it (a key short of an
input keeps the old look, and "built nothing" above passes it best): a fourth tab (the rail's
layout: the panel's templates and the outlines'), the window a pixel taller (all of them: each
is placed from the window's middle), and another background by a reload (F5, ReloadConfiguration:
all of them, the cards' masks filled with it and the panel's card made of it). A build that
doesn't say is measured, not judged. -Measure only measures.

cost.json in target\perf\paint-cost has every number, every frame's included.

  pwsh -File lbterm\perf\paint-cost.ps1 [-Local] [-Spinner] [-DrawAll] [-Measure] [-Bin <build>] [-Runs 3] [-Seconds 6] [-IntervalMs 16]
#>
param([string]$Bin, [switch]$Local, [switch]$Spinner, [switch]$DrawAll, [switch]$Measure, [int]$Runs = 3, [double]$Seconds = 6, [int]$IntervalMs = 16, [int]$Width = 1080, [int]$Height = 1920)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$Mux = -not $Local
$T = Start-Test ('paint-cost' + $(if ($Local) { '-local' } else { '' }) + $(if ($Spinner) { '-spinner' } else { '' }) + $(if ($DrawAll) { '-drawall' } else { '' })) -Bin $Bin
$cfg = Join-Path $T.Out 'config\paint-cost.lua'
$homeDir = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force (Split-Path $cfg), $homeDir | Out-Null

# The Conductor's pad on and accepting most: its own outline in every hue while the hub acts on a
# session, as it does here (Lachlan's 7a: a still hairline in the mode's colour at rest; until
# 2026-11-09 the spectrum always)
$button = Join-Path $T.Out 'button.json'
[IO.File]::WriteAllText($button, '{ "state": "on", "mode": "most", "tooltip": "Conductor is on (accept most)" }')
$hub = Start-PlayedPipe -Name "lbterm-perf-paint-cost-hub-$PID" -Subscribe '"lbterm.subscribe"'
$sids = 'b0d6e791-0000-4000-8000-000000000091', 'b0d6e792-0000-4000-8000-000000000092'

$domain = if ($Mux) {
  @"
config.unix_domains = { {
  name = 'paintcost',
  socket_path = '$(ConvertTo-LuaPath $T.Sock)',
  serve_command = { '$(ConvertTo-LuaPath $T.MuxServer)', '--config-file', '$(ConvertTo-LuaPath $cfg)', '--daemonize' },
} }
"@
} else { '' }
$lua = @"
config.initial_cols = 110
config.initial_rows = 30
config.window_close_confirmation = 'NeverPrompt'
config.default_cwd = [[$($T.Out)]]
config.default_prog = { 'C:/Program Files/PowerShell/7/pwsh.exe', '-NoProfile' }
rawset(config, 'lbterm_projects', nil)
rawset(config, 'lbterm_conductor', { state_file = [[$button]], hub_pipe = [[\\.\pipe\$($hub.Name)]], menu = {} })
-- F5 reloads it, for the step that changes the background (reloading on a save is off)
config.keys = config.keys or {}
table.insert(config.keys, { key = 'F5', mods = 'NONE', action = wezterm.action.ReloadConfiguration })
$domain
"@
New-TestConfig -Path $cfg -Lua $lua

# What the open tab runs to print: a line of colour every -IntervalMs for -Seconds, then a line
# saying so
$writer = '$e = [char]27; $sw = [Diagnostics.Stopwatch]::StartNew(); $i = 0; while ($sw.ElapsedMilliseconds -lt {0}) {{ $i++; [Console]::Out.Write("$e[38;5;$(17 + $i % 214)mline $i$e[0m the quick brown fox jumps over the lazy dog $e[1;34m$($sw.ElapsedMilliseconds) ms$e[0m and on to the end of the line`n"); Start-Sleep -Milliseconds {1} }}; "writer done after $i lines"' -f [int]($Seconds * 1000), $IntervalMs
# With -Spinner, the last line written again in place instead, as Claude Code's spinner and timer
# are while it works: one row changes, nothing scrolls
$spinLine = '$e = [char]27; $f = ''|/-\''; $sw = [Diagnostics.Stopwatch]::StartNew(); $i = 0; while ($sw.ElapsedMilliseconds -lt {0}) {{ $i++; [Console]::Out.Write("`r$e[2K$e[38;5;208m$($f[$i % 4])$e[0m Thinking... $e[2m($([int]($sw.ElapsedMilliseconds / 1000))s, $i)$e[0m"); Start-Sleep -Milliseconds {1} }}; "`nspinner done after $i"' -f [int]($Seconds * 1000), $IntervalMs
$measured = if ($Spinner) { $spinLine } else { $writer }

$variant = (@($(if ($Local) { 'local' }), $(if ($Spinner) { 'spinner' }), $(if ($DrawAll) { 'every visual drawn' })) | Where-Object { $_ }) -join ', '
if ($variant) { $variant = " ($variant)" }
$median = { param($xs) $v = @($xs | Where-Object { $null -ne $_ } | Sort-Object); if ($v) { $v[[int][Math]::Floor(($v.Count - 1) / 2)] } else { $null } }
$runsOut = [Collections.Generic.List[object]]::new()
Invoke-Test {
  $open = @{ ConfigFile = $cfg; UserProfile = $homeDir; TimeoutSec = 30 }
  if ($Mux) { $open.Domain = 'paintcost'; $open.Socket = $T.Sock } else { $open.Program = New-PwshProgram }
  $envs = @{}
  if ($DrawAll) { $envs.LBTERM_DRAW_ALL = '1' }
  # Judging, every write to the GPU is checked against a shadow of what it holds (gpu_work.rs):
  # a copy of every write, so not while measuring
  if (-not $Measure) { $envs.LBTERM_CHECK_UPLOADS = '1' }
  if ($envs.Count) { $open.Environment = $envs }
  $w = Open-TestWindow @open
  if ($Mux -and -not (Wait-Until { (Get-Panes -Server).Count -ge 1 } -TimeoutSec 20)) { throw "the test's mux server at $($T.Sock) didn't answer" }
  Wait-State { param($s) @($s.panes).Count -ge 1 -and $s.rail } -What 'the first pane and the rail' -TimeoutSec 30 | Out-Null
  $first = (Get-Panes -Server:$Mux)[0].pane_id
  foreach ($sid in $sids) { New-Tab -From $first -NoActivate -Program (New-ClaudeSessionProgram $sid) -Server:$Mux | Out-Null }
  Wait-State { param($s) @($s.tabs).Count -eq 3 } -What 'three tabs' | Out-Null

  # As tall as Lachlan's window: its client area -Width by -Height, the frame round it kept
  $outer = Get-WindowRect $w; $client = Get-ClientRect $w
  Set-WindowSize $w ($Width + ($outer.Right - $outer.Left) - $client.Width) ($Height + ($outer.Bottom - $outer.Top) - $client.Height)
  Wait-State { param($s) $s.size.width -eq $Width -and $s.size.height -eq $Height } -What "the window at $Width by $Height" -TimeoutSec 15 | Out-Null
  Send-Focus $w

  # The hub thinking about one session's tab and acting on the other's: two outlines with lights
  if (-not (Wait-Until { $hub.Subscribers -ge 1 -and @(Get-PlayedLines $hub 'lbterm.panes' | Select-Object -Last 1 | ForEach-Object { @($_.panes).Count }) -ge 2 } -TimeoutSec 20)) {
    throw "LBTerm never subscribed to the played hub with both sessions' panes: $($hub.Heard() -join ' | ')"
  }
  $focus = '{"type":"conductor.focus","sessions":[{"sessionId":"' + $sids[0] + '","doing":"thinking"},{"sessionId":"' + $sids[1] + '","doing":"acting"}]}'
  $ringed = { param($s) $s.rail.conductor.level -eq 'most' -and @($s.rail.rows | Where-Object conductor -eq 'thinking').Count -eq 1 -and @($s.rail.rows | Where-Object conductor -eq 'acting').Count -eq 1 }
  Send-PlayedLine $hub $focus | Out-Null
  Wait-State $ringed -What 'the pad accepting most, a row thought about and a row acted on' | Out-Null
  Invoke-Cli send-text --pane-id $first --no-paste "function prompt { 'PS> ' }; cls`r" -Server:$Mux -Check | Out-Null
  Start-Sleep -Milliseconds 800
  # A screenful printed first, unmeasured: the window's layers grow to hold it, and its glyphs go
  # into the atlas, as they have in a window that's been printing
  Invoke-Cli send-text --pane-id $first --no-paste "$writer`r" -Server:$Mux -Check | Out-Null
  Start-Sleep -Milliseconds ([int]($Seconds * 1000) + 500)
  Get-QuietFrame -QuietMs 1000 -TimeoutSec 20 | Out-Null

  $metricsOn = $true
  try { Get-Metrics -Reset | Out-Null } catch { $metricsOn = $false; $T.Notes.Add("no metrics from this build ($($_.Exception.Message)): the paint's parts aren't measured") }
  $rate = Get-CycleRate
  # Get-CycleRate is cycles a millisecond
  $toMs = { param([double]$cycles) [Math]::Round($cycles / $rate, 2) }
  $gpuMb = { try { [Math]::Round(((Get-Counter "\GPU Process Memory(pid_$($w.Pid)_*)\Dedicated Usage" -ErrorAction Stop).CounterSamples | Measure-Object CookedValue -Sum).Sum / 1MB, 1) } catch { $null } }
  $recorder = Start-FrameRecorder -Window $w
  $judged = $false
  for ($r = 1; $r -le $Runs; $r++) {
    # The outlines are still there: the hub's word lingers, so it's said again when it's gone
    if (-not (& $ringed (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)))) {
      Send-PlayedLine $hub $focus | Out-Null
      Wait-State $ringed -What 'the outlines back' | Out-Null
      Get-QuietFrame -QuietMs 1000 -TimeoutSec 20 | Out-Null
    }
    [void]$recorder.Take()
    $f0 = [long](Read-State).frame
    if ($metricsOn) { Get-Metrics -Reset | Out-Null }
    $c0 = Get-CpuSnapshot -Window $w
    Invoke-Cli send-text --pane-id $first --no-paste "$measured`r" -Server:$Mux -Check | Out-Null
    Start-Sleep -Milliseconds ([int]($Seconds * 1000) + 500)
    $f1 = Get-QuietFrame -QuietMs 700 -TimeoutSec 20
    $c1 = Get-CpuSnapshot -Window $w
    $m = if ($metricsOn) { Get-Metrics }
    $frames = $f1 - $f0
    $recorder.Look()
    $seen = @($recorder.Take() | Where-Object { $_.Frame -gt $f0 -and $_.Frame -le $f1 } | ForEach-Object {
      $s = try { $_.Text | ConvertFrom-Json -Depth 32 } catch { $null }
      if ($s) { $sw = Get-StateWindow $s -Hwnd ([long]$w.Handle); if ($sw) { [pscustomobject]@{ Frame = $_.Frame; Visuals = $sw.visuals } } }
    })
    $vis = @($seen | Where-Object { $_.Visuals })
    $says = @($vis | Where-Object { $null -ne $_.Visuals.made })
    $passed = @($vis | Where-Object { $null -ne $_.Visuals.passes })
    $part = { param($name) if ($m) { $p = @($m.paints | Where-Object { $_.site -like "*/$name" }); if ($p) { [Math]::Round((($p | Measure-Object total_ms -Sum).Sum) / [Math]::Max(1, $frames), 3) } } }
    $row = [pscustomobject]@{
      Run = $r; Frames = $frames; Recorded = $seen.Count
      GuiThreadMsFrame = [Math]::Round((& $toMs ($c1.ThreadCycles - $c0.ThreadCycles)) / [Math]::Max(1, $frames), 3)
      ProcessMsFrame = [Math]::Round((& $toMs ($c1.Cycles - $c0.Cycles)) / [Math]::Max(1, $frames), 3)
      GuiThreadMs = & $toMs ($c1.ThreadCycles - $c0.ThreadCycles)
      ProcessMs = & $toMs ($c1.Cycles - $c0.Cycles)
      PassMsFrame = & $part 'pass'; DrawMsFrame = & $part 'draw'; PresentMsFrame = & $part 'present'
      DrawnFrame = $(if ($vis) { [Math]::Round(($vis | ForEach-Object { $_.Visuals.drawn } | Measure-Object -Average).Average, 2) })
      KeptFrame = $(if ($vis) { [Math]::Round(($vis | ForEach-Object { $_.Visuals.kept } | Measure-Object -Average).Average, 2) })
      Which = @($vis | ForEach-Object { @($_.Visuals.which) -join '+' } | Group-Object | Sort-Object Count -Descending | Select-Object -First 4 | ForEach-Object { "$($_.Name) x$($_.Count)" })
      MadeFrame = $(if ($says) { [Math]::Round(($says | ForEach-Object { $_.Visuals.made } | Measure-Object -Average).Average, 2) })
      UploadsFrame = $(if ($says) { [Math]::Round(($says | ForEach-Object { $_.Visuals.uploads } | Measure-Object -Average).Average, 2) })
      UploadKbFrame = $(if ($says) { [Math]::Round(($says | ForEach-Object { $_.Visuals.upload_bytes } | Measure-Object -Average).Average / 1024, 1) })
      Built = @($says | ForEach-Object { @($_.Visuals.built) } | Group-Object | ForEach-Object { "$($_.Name) x$($_.Count)" })
      PassesFrame = $(if ($passed) { [Math]::Round(($passed | ForEach-Object { $_.Visuals.passes } | Measure-Object -Average).Average, 2) })
      SubmitsFrame = $(if ($passed) { [Math]::Round(($passed | ForEach-Object { $_.Visuals.submits } | Measure-Object -Average).Average, 2) })
      UploadUsFrame = $(if ($passed) { [Math]::Round(($passed | ForEach-Object { $_.Visuals.upload_us } | Measure-Object -Average).Average, 1) })
      RecordUsFrame = $(if ($passed) { [Math]::Round(($passed | ForEach-Object { $_.Visuals.record_us } | Measure-Object -Average).Average, 1) })
      SubmitUsFrame = $(if ($passed) { [Math]::Round(($passed | ForEach-Object { $_.Visuals.submit_us } | Measure-Object -Average).Average, 1) })
      BindGroups = $(if ($says) { ($says | ForEach-Object { $_.Visuals.bind_groups } | Measure-Object -Maximum).Maximum })
      GpuMb = & $gpuMb
      PrivateMb = [Math]::Round($c1.PrivateBytes / 1MB, 1)
      FramesSeen = $seen
    }
    $runsOut.Add($row)
    "run ${r}: $frames frames; a frame $($row.GuiThreadMsFrame) ms of the GUI thread, $($row.ProcessMsFrame) ms of the process$(if ($null -ne $row.PassMsFrame) { "; pass $($row.PassMsFrame), draw $($row.DrawMsFrame), present $($row.PresentMsFrame) ms" }); visuals drawn $($row.DrawnFrame), kept $($row.KeptFrame) ($($row.Which -join ', '))$(if ($says) { "; made $($row.MadeFrame), uploads $($row.UploadsFrame) ($($row.UploadKbFrame) KB), built $(if ($row.Built) { $row.Built -join ', ' } else { 'nothing' })" })$(if ($passed) { "; passes $($row.PassesFrame), submits $($row.SubmitsFrame); upload $($row.UploadUsFrame), record $($row.RecordUsFrame), submit $($row.SubmitUsFrame) us" })$(if ($null -ne $row.BindGroups) { "; bind groups kept $($row.BindGroups) at most" }); GPU $($row.GpuMb) MB, private $($row.PrivateMb) MB"

    # What a frame of output may do
    if ($Measure) { continue }
    if ($frames -lt [Math]::Max(5, $Seconds * 5)) { Add-Failure "run ${r}: only $frames frames for $Seconds s of a line every $IntervalMs ms" }
    if (-not $says) { if ($r -eq 1) { $T.Notes.Add('this build doesn''t say what its frames made, wrote and built (visuals.made): measured, not judged') }; continue }
    $judged = $true
    $built = @($says | Where-Object { @($_.Visuals.built).Count })
    if ($built) { Add-Failure "run ${r}: $($built.Count) frames built templates again, though nothing they're made from changed (the window's size, DPI and palette, where the outlines go): $(@($built | Select-Object -First 3 | ForEach-Object { "frame $($_.Frame) $(@($_.Visuals.built) -join ', ')" }) -join '; ')" }
    # A layer that needs more room than it has is made again, half as big again at least: a frame
    # or two a run, while what's on screen grows. More is buffers made again for nothing
    $made = @($says | Where-Object { $_.Visuals.made -gt 0 })
    if ($made.Count -gt 2 -or @($made | Where-Object { $_.Visuals.made -gt 8 })) { Add-Failure "run ${r}: $($made.Count) frames made GPU buffers or bind groups, $(($made | ForEach-Object { $_.Visuals.made } | Measure-Object -Sum).Sum) in all (frames $(@($made | Select-Object -First 5 | ForEach-Object Frame) -join ', ')): a frame of output makes none, but for a layer grown when what's on screen outgrows it" }
    elseif ($made) { $T.Notes.Add("run ${r}: $(@($made | ForEach-Object { "frame $($_.Frame) made $($_.Visuals.made)" }) -join ', '): a layer grown") }
    # Every quad a write left out held what the GPU's copy holds: its hash covers every byte of its
    # vertices (quad.rs), or the screen would keep the old ones
    $checked = @($says | Where-Object { $null -ne $_.Visuals.stale })
    $stale = @($checked | Where-Object { $_.Visuals.stale -gt 0 })
    if ($stale) { Add-Failure "run ${r}: $($stale.Count) frames left quads out of a write to the GPU whose hashes matched what it held but whose vertices didn't, $(($stale | ForEach-Object { $_.Visuals.stale } | Measure-Object -Sum).Sum) in all (frames $(@($stale | Select-Object -First 5 | ForEach-Object Frame) -join ', ')): a write to a quad its hash doesn't fold in" }
    elseif ($checked) { if ($r -eq 1) { $T.Notes.Add("every write checked against what the GPU holds: no quad left stale") } }
    elseif ($r -eq 1) { $T.Notes.Add('this build doesn''t check its writes to the GPU (visuals.stale): unchecked') }
    $idle = @($says | Where-Object { $_.Visuals.uploads -gt 0 -and $_.Visuals.drawn -eq 0 })
    if ($idle) { Add-Failure "run ${r}: $($idle.Count) frames wrote vertex buffers and drew no visual (frames $(@($idle | Select-Object -First 5 | ForEach-Object Frame) -join ', '))" }
    # One render pass a visual drawn, and a submit for each eight of them (D3D12's limit on the
    # swapchains one may write to), none for a frame that draws nothing (render/composited.rs)
    $extra = @($passed | Where-Object { $_.Visuals.passes -ne $_.Visuals.drawn -or $_.Visuals.submits -ne [Math]::Ceiling($_.Visuals.drawn / 8) })
    if ($extra) { Add-Failure "run ${r}: $($extra.Count) of $($passed.Count) frames recorded other than a render pass a visual drawn and a submit for each eight: $(@($extra | Select-Object -First 3 | ForEach-Object { "frame $($_.Frame) drew $($_.Visuals.drawn) in $($_.Visuals.passes) passes and $($_.Visuals.submits) submits" }) -join '; ')" }
    # A spinner changes one row a frame, so a frame writes that row's quads to the GPU, not its
    # layer's (lbterm_motion::content::dirty_runs). The command typed and its Enter, which scrolls
    # the full screen once, write more; a layer made again writes all it holds
    if ($Spinner) {
      $wrote = @($says | Where-Object { $_.Visuals.uploads -gt 0 })
      $big = @($wrote | Where-Object { $_.Visuals.made -eq 0 -and $_.Visuals.upload_bytes -gt 64KB })
      if (-not $wrote) { Add-Failure "run ${r}: no frame wrote anything to the GPU while the spinner turned: unmeasured" }
      elseif ($big.Count -gt 5) { Add-Failure "run ${r}: $($big.Count) of $($wrote.Count) frames wrote over 64 KB to the GPU while one row changed (frames $(@($big | Select-Object -First 5 | ForEach-Object { "$($_.Frame) $([Math]::Round($_.Visuals.upload_bytes / 1KB)) KB" }) -join ', ')): a frame writes only the quads that changed" }
    }
  }

  # What the templates are made from, changed one thing at a time: the frames showing each change
  # must make again every template made from it (render/kept.rs). A key short of an input keeps
  # the old look, and "built nothing" in the runs passes a key that never changes best
  # (lbterm/PITFALLS.md, "Caches and memory")
  if (-not $Measure -and $judged) {
    $cards = 'card-mask-wide', 'card-mask-tall', 'card-line-wide', 'card-line-tall'
    $panel = 'panel-glow', 'panel-lit', 'panel-spot', 'panel-spot-core'
    $rings = 'ring-spectrum', 'ring-iris', 'ring-light'
    $remade = { param([string]$What, [string[]]$Want, [scriptblock]$Change, [scriptblock]$Done)
      if (-not (& $ringed (Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)))) {
        Send-PlayedLine $hub $focus | Out-Null
        Wait-State $ringed -What 'the outlines back' | Out-Null
      }
      Get-QuietFrame -QuietMs 700 -TimeoutSec 20 | Out-Null
      [void]$recorder.Take()
      $g0 = [long](Read-State).frame
      & $Change
      Wait-State $Done -What $What -TimeoutSec 15 | Out-Null
      $g1 = Get-QuietFrame -QuietMs 700 -TimeoutSec 20
      $recorder.Look()
      $built = @($recorder.Take() | Where-Object { $_.Frame -gt $g0 -and $_.Frame -le $g1 } | ForEach-Object {
          $s = try { $_.Text | ConvertFrom-Json -Depth 32 } catch { $null }
          if ($s) { $sw = Get-StateWindow $s -Hwnd ([long]$w.Handle); if ($sw -and $sw.visuals) { @($sw.visuals.built) } }
        } | Where-Object { $_ } | Sort-Object -Unique)
      $missing = @($Want | Where-Object { $_ -notin $built })
      if ($missing) { Add-Failure "${What}: the frames after it never made $($missing -join ', ') again (they made $(if ($built) { $built -join ', ' } else { 'nothing' })): a template kept past a change in what it's made from" }
      else { "${What}: made again $($built -join ', ')" }
    }
    # The rail's layout, which the panel's templates and the outlines' are placed from
    & $remade 'a fourth tab' ($panel + $rings) { New-Tab -From $first -NoActivate -Server:$Mux | Out-Null } { param($s) @($s.tabs).Count -eq 4 }
    # The window's size: every template is placed from its middle
    $outer = Get-WindowRect $w
    & $remade 'the window a pixel taller' ($cards + $panel + $rings) { Set-WindowSize $w ($outer.Right - $outer.Left) ($outer.Bottom - $outer.Top + 1) } { param($s) $s.size.height -eq $Height + 1 }
    # The palette's background: the cards' masks are filled with it, the panel's card is it
    $bg = "config.colors = config.colors or {}`nconfig.colors.background = '#1b2a3a'"
    & $remade 'another background, by a reload' ($cards + $panel + $rings) { New-TestConfig -Path $cfg -Lua ($lua + "`n" + $bg); Send-Key $w 'F5' } { param($s) $s.strip.notice -match 'reload' }
  }
  $recorder.Dispose()
  $summary = [ordered]@{
    Mux = $Mux; Width = $Width; Height = $Height; Seconds = $Seconds; IntervalMs = $IntervalMs
    Build = $T.Bin
    GuiThreadMsFrame = & $median ($runsOut | ForEach-Object GuiThreadMsFrame)
    ProcessMsFrame = & $median ($runsOut | ForEach-Object ProcessMsFrame)
    PassMsFrame = & $median ($runsOut | ForEach-Object PassMsFrame)
    DrawMsFrame = & $median ($runsOut | ForEach-Object DrawMsFrame)
    PresentMsFrame = & $median ($runsOut | ForEach-Object PresentMsFrame)
    PassesFrame = & $median ($runsOut | ForEach-Object PassesFrame)
    SubmitsFrame = & $median ($runsOut | ForEach-Object SubmitsFrame)
    UploadUsFrame = & $median ($runsOut | ForEach-Object UploadUsFrame)
    RecordUsFrame = & $median ($runsOut | ForEach-Object RecordUsFrame)
    SubmitUsFrame = & $median ($runsOut | ForEach-Object SubmitUsFrame)
    Frames = & $median ($runsOut | ForEach-Object Frames)
    GpuMb = & $median ($runsOut | ForEach-Object GpuMb)
    PrivateMb = & $median ($runsOut | ForEach-Object PrivateMb)
    Runs = $runsOut
  }
  "median: a frame $($summary.GuiThreadMsFrame) ms of the GUI thread, $($summary.ProcessMsFrame) ms of the process$(if ($null -ne $summary.PassMsFrame) { "; pass $($summary.PassMsFrame), draw $($summary.DrawMsFrame), present $($summary.PresentMsFrame) ms" })$(if ($null -ne $summary.PassesFrame) { "; passes $($summary.PassesFrame), submits $($summary.SubmitsFrame); upload $($summary.UploadUsFrame), record $($summary.RecordUsFrame), submit $($summary.SubmitUsFrame) us" }); $($summary.Frames) frames a run; GPU $($summary.GpuMb) MB, private $($summary.PrivateMb) MB"
  [pscustomobject]$summary | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $T.Out 'cost.json')
  if (-not $Measure -and -not $judged) { (Get-Test).PassedLine = "paint-cost${variant}: measured; this build doesn't say what its frames built, made and wrote, so nothing was judged" }
} -Passed "paint-cost${variant}: $(if ($Measure) { 'measured' } else { "a frame of output builds no template again, makes GPU buffers or bind groups only for a layer growing, writes no vertex buffer it draws nothing from$(if ($Spinner) { ' and only the changed row''s quads to the GPU' }), leaves no quad stale on the GPU, and draws each visual in one render pass, a submit for each eight; a new tab, a pixel more and a new background each make again the templates made from them" })"
