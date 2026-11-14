# The budget runner's library (lbterm/PERF.md, "The budget runner"): reading lbterm\perf\budgets.json,
# a measured span's numbers at rest and an action's run's, judging a scenario's spans or runs
# against its budgets, the report, its table and the history line, and the run itself behind
# `lbterm.ps1 budgets`. perf\budgets.ps1 sets each scenario up in a test window and measures it;
# this judges what it measured.
#
#   Import-Module (Join-Path $PSScriptRoot 'harness.psm1') -Force -DisableNameChecking   # the run needs it
#   Import-Module (Join-Path $PSScriptRoot 'budgets.psm1') -Force -DisableNameChecking
#
# lbterm\tests\budgets.Tests.ps1 tests everything here but the run, which opens windows.
Set-StrictMode -Off
$ErrorActionPreference = 'Stop'

$script:Repo = Split-Path $PSScriptRoot -Parent
$script:BudgetsFile = Join-Path $PSScriptRoot 'perf\budgets.json'
$script:BudgetsScript = Join-Path $PSScriptRoot 'perf\budgets.ps1'

# What a span at rest is measured by, in the order the table and the history give them: each one's
# unit and what it is. budgets.json gives a budget (`max`) to some; the rest are said, not judged.
$script:RestMetrics = [ordered]@{
  frames        = @{ unit = '/s'; what = 'frames painted, every window''s (the state file''s count)' }
  requests      = @{ unit = '/s'; what = 'repaints asked for (metrics: requests)' }
  timers        = @{ unit = '/s'; what = 'timers fired (metrics: timers_fired)' }
  tasks         = @{ unit = '/s'; what = 'main-thread task runs (metrics: tasks)' }
  task_ms       = @{ unit = 'ms/s'; what = 'the time those took (metrics: task_ms)' }
  messages      = @{ unit = '/s'; what = 'window messages (metrics: messages)' }
  wakes         = @{ unit = '/s'; what = 'the GUI thread coming back from waiting (metrics: gui.wakes)' }
  gui_thread_ms = @{ unit = 'ms/s'; what = 'the GUI thread''s CPU, by its cycles' }
  cpu_ms        = @{ unit = 'ms/s'; what = 'the GUI process''s CPU, by its cycles, less the state file''s writer (a test window''s alone)' }
  state_ms      = @{ unit = 'ms/s'; what = 'the state file''s writer thread (lbterm-state), which only a test window has: left out of cpu_ms' }
  server_cpu_ms = @{ unit = 'ms/s'; what = 'the mux server''s CPU, by its cycles (a mux domain''s)' }
  conpty_cpu_ms = @{ unit = 'ms/s'; what = 'the conpty hosts'' CPU (OpenConsole.exe, one a pane), by their cycles' }
  frame_ms      = @{ unit = 'ms'; what = 'the GUI thread''s CPU a frame painted (gui_thread_ms over frames): what a frame costs with what the scenario has open, against the same with nothing open (a busy scenario''s)' }
  pass_ms       = @{ unit = 'ms'; what = 'the paint pass a frame, building it (metrics: paints, each window''s pass, its time over its count)' }
  words         = @{ unit = '/s'; what = 'words the cards, lists and the rail laid out (metrics: words.laid_out; lbterm_extras::kept): none while what they show stays the same' }
  lines         = @{ unit = '/s'; what = 'panes'' lines laid out as quads again, missing the line quad cache (the state file''s work.lines): only the lines that changed, not every line holding a link or a trigger''s match' }
  private_mb    = @{ unit = 'MB'; what = 'the GUI process''s private bytes at the span''s end' }
  handles       = @{ unit = ''; what = 'the GUI process''s handles at the span''s end' }
  loops         = @{ unit = ''; what = 'animations DWM plays for ever in the window as the span ends (the state file''s loops: a waiting pad''s arc, the Conductor''s turning hues and light), each keeping DWM composing every refresh (lbterm/COMPOSITOR.md, "Loops")' }
}

# What an action is measured by: each run gives a value of each `field` (sample), and the scenario
# is judged on `stat` of those over its runs: the median, the 95th percentile (nearest rank), or
# the first, a warm-up run in a window that hasn't done it yet, left out of the others. Every time
# is from the posted message, the cli call or the process's start; every cost is one run's, from
# just before it to the window quiet again after.
$script:ActionMetrics = [ordered]@{
  ms            = @{ unit = 'ms'; field = 'ms'; stat = 'median'; what = 'from the posted message (or the cli call, the resize, the process''s start) to the first frame the state file says shows it' }
  p95_ms        = @{ unit = 'ms'; field = 'ms'; stat = 'p95'; what = 'the same, its 95th percentile' }
  gui_ms        = @{ unit = 'ms'; field = 'gui_ms'; stat = 'median'; what = 'the GUI''s own time for it (metrics'' budget.actions: lbterm.action), from its message''s arrival to the end of the frame that shows it' }
  gui_p95_ms    = @{ unit = 'ms'; field = 'gui_ms'; stat = 'p95'; what = 'the same, its 95th percentile' }
  paint_ms      = @{ unit = 'ms'; field = 'paint_ms'; stat = 'median'; what = 'the GUI''s own, from its message''s arrival to the paint of the frame that shows it starting (metrics'' present.actions)' }
  screen_ms     = @{ unit = 'ms'; field = 'screen_ms'; stat = 'median'; what = 'the GUI''s own, from its message''s arrival to the refresh the frame that shows it went up at, by DXGI''s frame statistics on the swapchain that shows it (metrics'' present.actions)' }
  screen_p95_ms = @{ unit = 'ms'; field = 'screen_ms'; stat = 'p95'; what = 'the same, its 95th percentile' }
  post_screen_ms = @{ unit = 'ms'; field = 'post_screen_ms'; stat = 'median'; what = 'from the posted message to that refresh, the runner''s clock against the GUI''s (screen_unix_ms)' }
  after_present_ms = @{ unit = 'ms'; field = 'after_present_ms'; stat = 'median'; what = 'from the frame''s presents to that refresh: what DWM adds' }
  settled_ms    = @{ unit = 'ms'; field = 'settled_ms'; stat = 'median'; what = 'to the first frame with nothing animating: its motion over' }
  shown_frame   = @{ unit = ''; field = 'shown_frame'; stat = 'p95'; what = 'which frame after it showed it, 1 the first (its 95th percentile, the slowest of 15 runs): a count of frames, which a loaded PC doesn''t move the way it moves milliseconds' }
  unread_frames = @{ unit = ''; field = 'unread_frames'; stat = 'p95'; what = 'frames the GUI painted straight before the one that showed it whose snapshots the runner never read (its 95th percentile): above 0, one of those may have shown it first, and ms and shown_frame can be that many frames long' }
  missed        = @{ unit = ''; field = 'missed'; stat = 'median'; what = 'refreshes of the window''s screen while its motion played that came without a new frame, by the compositor clock''s statistics (metrics'' present.motion), the refreshes counted by the screen''s vblank times' }
  refreshes     = @{ unit = ''; field = 'refreshes'; stat = 'median'; what = 'the refreshes its motion played over, from its first frame up to its settling, by the screen''s vblank times a refresh apart' }
  refreshes_counted = @{ unit = ''; field = 'refreshes_counted'; stat = 'median'; what = 'the same span by the screen''s own refresh counter, which jumps at a present on some screens (PITFALLS 166): beside refreshes to show when they disagree, never a budget' }
  motion_first_ms = @{ unit = 'ms'; field = 'motion_first_ms'; stat = 'median'; what = 'from the frame that started its motion to the motion''s first frame on screen' }
  slow_paints   = @{ unit = ''; field = 'slow_paints'; stat = 'median'; what = 'the GUI''s paints while its motion played that took longer than a refresh' }
  motion_paint_ms = @{ unit = 'ms'; field = 'motion_paint_ms'; stat = 'median'; what = 'the longest paint while its motion played' }
  ready_ms      = @{ unit = 'ms'; field = 'ready_ms'; stat = 'median'; what = 'a start: to the first frame with its tab''s pane in it' }
  writer_ms     = @{ unit = 'ms'; field = 'writer_ms'; stat = 'median'; what = 'a writer''s own time for its writes: how long the console host took them' }
  writer_mb_per_s = @{ unit = 'MB/s'; field = 'writer_mb_per_s'; stat = 'median'; what = 'a writer at full speed beside the pane: what it wrote in its last second, which the console host takes as fast as LBTerm reads it' }
  echo_ms       = @{ unit = 'ms'; field = 'echo_ms'; stat = 'median'; what = 'keys typed one after another (a burst, a key held): each from its own post to the first frame that drew its echo, the run''s median, then the median of the runs' }
  echo_max_ms   = @{ unit = 'ms'; field = 'echo_max_ms'; stat = 'p95'; what = 'the same keys: the run''s slowest echo, its 95th percentile over the runs' }
  first_ms      = @{ unit = 'ms'; field = 'ms'; stat = 'first'; what = 'the first run, a warm-up in a window that hadn''t done it yet, left out of the rest' }
  gui_thread_ms = @{ unit = 'ms'; field = 'gui_thread_ms'; stat = 'median'; what = 'the GUI thread''s CPU for one, by its cycles, from before the message to the window quiet again' }
  cpu_ms        = @{ unit = 'ms'; field = 'cpu_ms'; stat = 'median'; what = 'the GUI process''s CPU for one, by its cycles, less the state file''s writer (a test window''s alone)' }
  server_cpu_ms = @{ unit = 'ms'; field = 'server_cpu_ms'; stat = 'median'; what = 'the mux server''s CPU for one, by its cycles' }
  frames        = @{ unit = ''; field = 'frames'; stat = 'median'; what = 'frames painted for one, to the window quiet again' }
  messages      = @{ unit = ''; field = 'messages'; stat = 'median'; what = 'window messages for one (metrics: messages)' }
  mb_per_s      = @{ unit = 'MB/s'; field = 'mb_per_s'; stat = 'median'; what = 'a writer''s output shown, in megabytes a second' }
  private_mb    = @{ unit = 'MB'; field = 'private_mb'; stat = 'median'; what = 'the GUI process''s private bytes once the run is quiet' }
  server_private_mb = @{ unit = 'MB'; field = 'server_private_mb'; stat = 'median'; what = 'the mux servers'' private bytes once the run is quiet (a mux domain''s)' }
  font_loads    = @{ unit = ''; field = 'font_loads'; stat = 'median'; what = 'fonts made for one: a style found through the font lookup, its shaper and metrics (metrics: fonts.fonts)' }
  font_files    = @{ unit = ''; field = 'font_files'; stat = 'median'; what = 'system font files read whole and parsed for one (metrics: fonts.files_read)' }
  fallback_searches = @{ unit = ''; field = 'fallback_searches'; stat = 'median'; what = 'searches for a fallback face for characters no font had, for one (metrics: fonts.fallback_searches)' }
  reshapes      = @{ unit = ''; field = 'reshapes'; stat = 'median'; what = 'times a window''s shapes were all cleared at once for one (metrics: fonts.reshapes)' }
  atlas_rebuilds = @{ unit = ''; field = 'atlas_rebuilds'; stat = 'median'; what = 'glyph atlases made for one, every glyph in it rasterized again (metrics: fonts.atlas_made)' }
  lines_laid_out = @{ unit = ''; field = 'lines_laid_out'; stat = 'median'; what = 'lines laid out as quads again for one, not found in the line quad cache (metrics: caches.line_quad_cache.misses)' }
}

# The GUI's own actions (lbterm-extras/src/budget.rs, Action::name): what an action scenario's
# `gui` may name, so metrics' budget.actions times it too
$script:GuiActions = @('key', 'tab', 'new-tab', 'close-tab', 'split', 'rail', 'inbox', 'agenda', 'conductor-view', 'picker', 'pick', 'reload', 'paint')

# The thread that writes the state file (lbterm-extras/src/state.rs), a test window's alone
$script:StateThread = 'lbterm-state'

# The setups perf\budgets.ps1 has, by kind: what budgets.json may name. A kind's name in -Only
# picks every scenario of it, so no scenario may be called that
$script:BudgetKinds = @('rest', 'action')
$script:BudgetDomains = @('local', 'mux')

# The metrics a kind is measured and judged by
function Get-BudgetMetrics([string]$Kind) { if ($Kind -eq 'action') { $script:ActionMetrics } else { $script:RestMetrics } }

#region budgets.json

# budgets.json, read and checked: its rest settings (seconds, runs, settle_ms) and budgets, its
# actions' (runs, warmup, and budgets every action has), and every scenario with its own budgets
# laid over its kind's defaults. Throws, saying what's wrong, for a file this can't hold a run to:
# a version it doesn't know, a budget for a metric its kind doesn't measure or without a max or a
# min, a name used twice or a kind's, a kind or domain it has no setup for, a GUI action the GUI
# doesn't time.
function Read-Budgets([string]$Path = $script:BudgetsFile) {
  if (-not (Test-Path -LiteralPath $Path)) { throw "no budgets file at $Path" }
  $j = try { Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -AsHashtable } catch { throw "budgets file ${Path}: $($_.Exception.Message)" }
  if ($j.version -ne 1) { throw "budgets file ${Path}: version $($j.version); this reads version 1" }
  if (-not $j.rest) { throw "budgets file ${Path}: no rest section" }
  $rest = $j.rest
  foreach ($k in 'seconds', 'runs', 'settle_ms') {
    if (-not ($rest[$k] -is [ValueType]) -or [double]$rest[$k] -le 0) { throw "budgets file ${Path}: rest.$k is '$($rest[$k])', not a number above 0" }
  }
  $defaults = ConvertTo-BudgetSet $rest.budgets 'rest.budgets' $Path $script:RestMetrics
  # A busy scenario's (one that paints on purpose, a line printed in its open tab every so often):
  # the rest's, with the busy section's laid over them
  $busyOwn = ConvertTo-BudgetSet $(if ($j.busy) { $j.busy.budgets }) 'busy.budgets' $Path $script:RestMetrics
  $busyDefaults = [ordered]@{}
  foreach ($m in $script:RestMetrics.Keys) {
    if ($busyOwn.Contains($m)) { $busyDefaults[$m] = $busyOwn[$m] }
    elseif ($defaults.Contains($m)) { $busyDefaults[$m] = $defaults[$m] }
  }
  # The actions' section: how many runs and warm-ups each, when a scenario says nothing
  $act = if ($j.actions) { $j.actions } else { @{ runs = 15; warmup = 1 } }
  if (-not ($act.runs -is [ValueType]) -or [int]$act.runs -lt 1) { throw "budgets file ${Path}: actions.runs is '$($act.runs)', not a number of 1 or more" }
  if ($null -ne $act.warmup -and (-not ($act.warmup -is [ValueType]) -or [int]$act.warmup -lt 0)) { throw "budgets file ${Path}: actions.warmup is '$($act.warmup)', not a number of 0 or more" }
  $actWarmup = if ($null -ne $act.warmup) { [int]$act.warmup } else { 1 }
  $actDefaults = ConvertTo-BudgetSet $act.budgets 'actions.budgets' $Path $script:ActionMetrics
  $pc = ConvertTo-PcRules $j.pc $Path
  $names = [Collections.Generic.HashSet[string]]::new()
  $scenarios = foreach ($s in @($j.scenarios)) {
    $name = [string]$s.name
    if ($name -notmatch '^[a-z0-9][a-z0-9-]*$') { throw "budgets file ${Path}: a scenario's name '$name' isn't lower-case words joined by -" }
    if ($name -in $script:BudgetKinds) { throw "budgets file ${Path}: a scenario named $name, which is a kind's name (-Only $name picks every scenario of it)" }
    if (-not $names.Add($name)) { throw "budgets file ${Path}: two scenarios named $name" }
    $kind = if ($s.kind) { [string]$s.kind } else { 'rest' }
    if ($kind -notin $script:BudgetKinds) { throw "budgets file ${Path}: $name's kind '$kind' isn't one of $($script:BudgetKinds -join ', ')" }
    $domain = if ($s.domain) { [string]$s.domain } else { 'mux' }
    if ($domain -notin $script:BudgetDomains) { throw "budgets file ${Path}: $name's domain '$domain' isn't local or mux" }
    if (-not $s.what) { throw "budgets file ${Path}: $name says nothing of what it sets up (what)" }
    $metrics = Get-BudgetMetrics $kind
    $own = ConvertTo-BudgetSet $s.budgets "$name.budgets" $Path $metrics
    $busy = [bool]$s.busy
    if ($busy -and $kind -ne 'rest') { throw "budgets file ${Path}: $name is busy, but only a scenario at rest can be" }
    if ($busy -and -not $j.busy) { throw "budgets file ${Path}: $name is busy, but the file has no busy section" }
    $kindDefaults = if ($kind -eq 'action') { $actDefaults } elseif ($busy) { $busyDefaults } else { $defaults }
    $budgets = [ordered]@{}
    foreach ($m in $metrics.Keys) {
      if ($own.Contains($m)) { $budgets[$m] = $own[$m] }
      elseif ($kindDefaults.Contains($m)) { $budgets[$m] = $kindDefaults[$m] }
    }
    $gui = if ($s.gui) { [string]$s.gui } else { $null }
    if ($gui -and $kind -ne 'action') { throw "budgets file ${Path}: $name names a GUI action ($gui), but only an action scenario has one" }
    if ($gui -and $gui -notin $script:GuiActions) { throw "budgets file ${Path}: $name's gui '$gui' isn't an action the GUI times ($($script:GuiActions -join ', '))" }
    if ($kind -eq 'action' -and $null -ne $s.runs -and (-not ($s.runs -is [ValueType]) -or [int]$s.runs -lt 1)) { throw "budgets file ${Path}: $name's runs is '$($s.runs)', not a number of 1 or more" }
    [pscustomobject]@{
      Name    = $name
      Kind    = $kind
      Domain  = $domain
      What    = [string]$s.what
      Bug     = $(if ($s.bug) { [string]$s.bug } else { $null })
      Seconds = $(if ($s.seconds) { [double]$s.seconds } else { [double]$rest.seconds })
      # An action's: how many runs are judged, after how many warm-ups, and the GUI's own name for it
      Runs    = $(if ($kind -eq 'action') { if ($null -ne $s.runs) { [int]$s.runs } else { [int]$act.runs } } else { [int]$rest.runs })
      Warmup  = $(if ($kind -eq 'action') { if ($null -ne $s.warmup) { [int]$s.warmup } else { $actWarmup } } else { 0 })
      Gui     = $gui
      Busy    = $busy
      Budgets = $budgets
    }
  }
  [pscustomobject]@{
    Path      = $Path
    Seconds   = [double]$rest.seconds
    Runs      = [int]$rest.runs
    SettleMs  = [int]$rest.settle_ms
    Defaults  = $defaults
    BusyDefaults = $busyDefaults
    ActionRuns = [int]$act.runs
    ActionWarmup = $actWarmup
    ActionDefaults = $actDefaults
    Pc        = $pc
    Scenarios = @($scenarios)
  }
}

# budgets.json's pc section, checked: when a span or a run counts as measured on a busy PC (others
# took more than others_max of its CPUs, or the whole PC was busier than pc_max, each a share from 0
# to 1) and how many spans or runs a scenario may take again in place of busy ones (retakes). Without
# the section nothing counts as busy and nothing is taken again; the PC is still said and the
# cycles still scaled to the nominal clock
function ConvertTo-PcRules($Pc, [string]$Path) {
  if ($null -eq $Pc) { return [pscustomobject]@{ OthersMax = $null; PcMax = $null; Retakes = 0 } }
  if ($Pc -isnot [Collections.IDictionary]) { throw "budgets file ${Path}: pc isn't an object" }
  foreach ($k in $Pc.Keys) { if ($k -notin 'others_max', 'pc_max', 'retakes', 'why') { throw "budgets file ${Path}: pc.$k isn't a rule this knows (others_max, pc_max, retakes)" } }
  $share = { param($k)
    $v = $Pc[$k]
    if ($null -eq $v) { return $null }
    if (-not ($v -is [ValueType]) -or [double]$v -le 0 -or [double]$v -gt 1) { throw "budgets file ${Path}: pc.$k is '$v', not a share above 0 and up to 1" }
    [double]$v
  }
  $retakes = $Pc.retakes
  if ($null -ne $retakes -and (-not ($retakes -is [ValueType]) -or [int]$retakes -lt 0)) { throw "budgets file ${Path}: pc.retakes is '$retakes', not a number of 0 or more" }
  [pscustomobject]@{ OthersMax = (& $share 'others_max'); PcMax = (& $share 'pc_max'); Retakes = $(if ($null -ne $retakes) { [int]$retakes } else { 0 }) }
}

# A budgets object's entries checked: each a metric of -Metrics (its kind's), with a max of 0 or
# more or, for what should be at least so much (a throughput), a min (and a why when it says one):
# name -> @{ max; min; why }
function ConvertTo-BudgetSet($Set, [string]$Where, [string]$Path, [Collections.IDictionary]$Metrics = $script:RestMetrics) {
  $out = [ordered]@{}
  if ($null -eq $Set) { return $out }
  $what = if ([object]::ReferenceEquals($Metrics, $script:ActionMetrics)) { 'an action measures' } else { 'a span measures' }
  foreach ($k in $Set.Keys) {
    if (-not $Metrics.Contains($k)) { throw "budgets file ${Path}: $Where.$k isn't something $what ($($Metrics.Keys -join ', '))" }
    $b = $Set[$k]
    $isNum = { param($x) $x -is [ValueType] -and [double]$x -ge 0 }
    if ($b -isnot [Collections.IDictionary] -or -not ((& $isNum $b.max) -or (& $isNum $b.min))) { throw "budgets file ${Path}: $Where.$k has no max (or min) of 0 or more" }
    if ($null -ne $b.max -and $null -ne $b.min) { throw "budgets file ${Path}: $Where.$k has both a max and a min" }
    $out[$k] = @{
      max = $(if ($null -ne $b.max) { [double]$b.max } else { $null })
      min = $(if ($null -ne $b.min) { [double]$b.min } else { $null })
      why = $(if ($b.why) { [string]$b.why } else { $null })
    }
  }
  $out
}

# The scenarios -Only names (a name or a wildcard, comma-separated or not, or a kind's name for
# every scenario of it: rest, action), in the file's order; throws for a name none matches
function Select-BudgetScenarios([object[]]$Scenarios, [string[]]$Only) {
  $names = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
  if (-not $names) { return @($Scenarios) }
  $picks = { param($s, $n) $s.Name -like $n -or ($n -in $script:BudgetKinds -and $(if ($s.Kind) { $s.Kind } else { 'rest' }) -eq $n) }
  $unknown = @($names | Where-Object { $n = $_; -not ($Scenarios | Where-Object { & $picks $_ $n }) })
  if ($unknown) { throw "no budget scenario named $($unknown -join ', ') in budgets.json: $(($Scenarios | ForEach-Object Name) -join ', ')" }
  @($Scenarios | Where-Object { $s = $_; $names | Where-Object { & $picks $s $_ } })
}

#endregion

#region A span's numbers

# The middle value (the mean of the two middle ones for an even count); nothing for none
function Get-Median([double[]]$Values) {
  $v = @($Values | Sort-Object)
  if (-not $v.Count) { return $null }
  $mid = [int][Math]::Floor($v.Count / 2)
  if ($v.Count % 2) { [double]$v[$mid] } else { ([double]$v[$mid - 1] + [double]$v[$mid]) / 2 }
}

# The nearest-rank percentile (95 for p95); nothing for none
function Get-Percentile([double[]]$Values, [double]$P) {
  $v = @($Values | Sort-Object)
  if (-not $v.Count) { return $null }
  $rank = [int][Math]::Ceiling($P / 100 * $v.Count)
  [double]$v[[Math]::Min([Math]::Max($rank, 1), $v.Count) - 1]
}

function Round3($x) { if ($null -eq $x) { $null } else { [Math]::Round([double]$x, 3) } }

# The cycles a list of processes ran between two snapshots' lists ([{Pid, Cycles}]): those in both
# and alive at both (Cycles -1 is gone); nothing when neither list has any
function Get-CycleDelta([object[]]$Before, [object[]]$After) {
  if (-not @($Before).Count -and -not @($After).Count) { return $null }
  $was = @{}
  foreach ($b in @($Before)) { if ($b -and [long]$b.Cycles -ge 0) { $was[[int]$b.Pid] = [long]$b.Cycles } }
  [long]$sum = 0
  foreach ($a in @($After)) {
    if ($a -and [long]$a.Cycles -ge 0 -and $was.ContainsKey([int]$a.Pid)) { $sum += [Math]::Max([long]0, [long]$a.Cycles - $was[[int]$a.Pid]) }
  }
  $sum
}

# The top -Top of a metrics reply's lists, for naming who ran: who asked for frames, the tasks,
# the timers, the messages and the threads, most first. Every message's site and every PDU the
# window sent or took, not just the top: an action's messages are a dozen, and a count told apart
# between two builds by one message or a server's CPU by a few PDUs a second is traced by which
# (the review of perf/mux-client, whose budget comparison had zoom 8 messages to 9 and nothing
# to say which)
function Get-MetricsTop($Metrics, [int]$Top = 5) {
  if (-not $Metrics) { return $null }
  $take = { param($list, $key) @(@($list) | Where-Object { $_ } | Sort-Object { - [double]$_.$key } | Select-Object -First $Top) }
  $all = { param($list, $key) @(@($list) | Where-Object { $_ -and [double]$_.$key -gt 0 } | Sort-Object { - [double]$_.$key }) }
  [ordered]@{
    requests = @(& $take $Metrics.requests 'frames' | ForEach-Object { [ordered]@{ site = $_.site; frames = $_.frames; requests = $_.requests } })
    tasks    = @(& $take $Metrics.tasks 'count' | ForEach-Object { [ordered]@{ site = $_.site; count = $_.count; ms = $_.total_ms } })
    timers   = @(& $take $Metrics.timers 'fired' | ForEach-Object { [ordered]@{ site = $_.site; fired = $_.fired } })
    messages = @(& $all $Metrics.messages 'count' | ForEach-Object { [ordered]@{ site = $_.site; count = $_.count; ms = $_.total_ms } })
    threads  = @(& $take $Metrics.threads 'cpu_ms' | ForEach-Object { [ordered]@{ name = $(if ($_.name) { $_.name } else { '(unnamed)' }); tid = $_.tid; ms = $_.cpu_ms } })
    mux      = @(@($Metrics.mux) | Where-Object { $_ -and ([double]$_.sent + [double]$_.received) -gt 0 } | Sort-Object { - ([double]$_.sent + [double]$_.received) } |
        ForEach-Object { [ordered]@{ pdu = $_.pdu; sent = $_.sent; received = $_.received } })
  }
}

# A span at rest as numbers a second: from two Get-CpuSnapshot snapshots (-Before, -After), the
# frames the state file counted between them (-Frames) and the metrics reply that covers the span
# (-Metrics, nothing for a build without metrics, whose counts are then nothing too), the lines
# laid out again between them (-Lines, the state file's work.lines; nothing from a build without it)
# and how many loops the state file said as it ended (-Loops). -CycleRate is cycles a millisecond
# (Get-CycleRate), -Frequency the snapshots' clock (Stopwatch.Frequency). What the GUI's own budget
# counted at rest and who ran are kept beside the numbers.
function ConvertTo-RestSample {
  param([Parameter(Mandatory)]$Before, [Parameter(Mandatory)]$After, [AllowNull()]$Frames, [AllowNull()]$Metrics,
    [Parameter(Mandatory)][double]$CycleRate, [double]$Frequency = [Diagnostics.Stopwatch]::Frequency, [AllowNull()]$Lines,
    [AllowNull()]$Loops)
  $s = ([double]$After.At - [double]$Before.At) / $Frequency
  if ($s -le 0) { throw "a span of $s s" }
  $per = { param($n, $over = $s) if ($null -eq $n) { $null } else { Round3 ([double]$n / $over) } }
  $cpu = { param($cycles) if ($null -eq $cycles) { $null } else { Round3 ([double]$cycles / $CycleRate / $s) } }
  $t = if ($Metrics) { $Metrics.totals } else { $null }
  # The reply's counts are over its own span, from its reset to its answer, a little longer
  $ms = if ($Metrics -and [double]$Metrics.span_ms -gt 0) { [double]$Metrics.span_ms / 1000 } else { $s }
  $sample = [ordered]@{ seconds = Round3 $s }
  $sample.frames = & $per $Frames
  $sample.requests = & $per $(if ($t) { $t.requests }) $ms
  $sample.timers = & $per $(if ($t) { $t.timers_fired }) $ms
  $sample.tasks = & $per $(if ($t) { $t.tasks }) $ms
  $sample.task_ms = & $per $(if ($t) { $t.task_ms }) $ms
  $sample.messages = & $per $(if ($t) { $t.messages }) $ms
  $sample.wakes = & $per $(if ($Metrics) { $Metrics.gui.wakes }) $ms
  $sample.gui_thread_ms = & $cpu ([Math]::Max([long]0, [long]$After.ThreadCycles - [long]$Before.ThreadCycles))
  # The state file (LBTERM_STATE_FILE) is a test window's: its writer's cycles are left out of the
  # process's, so what's held to a budget is what Lachlan's window spends
  $threads = @(Get-ThreadCycles $Before.ThreadList $After.ThreadList)
  $state = ($threads | Where-Object name -eq $script:StateThread | Measure-Object cycles -Sum).Sum
  $sample.cpu_ms = & $cpu ([Math]::Max([long]0, [long]$After.Cycles - [long]$Before.Cycles - [long]$state))
  $sample.state_ms = & $cpu ([long]$state)
  $sample.server_cpu_ms = & $cpu (Get-CycleDelta $Before.Servers $After.Servers)
  $sample.conpty_cpu_ms = & $cpu (Get-CycleDelta $Before.Conpty $After.Conpty)
  # What a frame costs: the GUI thread's over the frames, and the paint pass's own (each window's
  # `pass`, from metrics' paints); nothing for a span that painted none
  $sample.frame_ms = if ([double]$Frames -gt 0) { Round3 ([double]$sample.gui_thread_ms * $s / [double]$Frames) } else { $null }
  $passes = @($(if ($Metrics) { $Metrics.paints }) | Where-Object { $_ -and ([string]$_.site) -like '*/pass' })
  $passCount = ($passes | Measure-Object count -Sum).Sum
  $sample.pass_ms = if ($passCount -gt 0) { Round3 (($passes | Measure-Object total_ms -Sum).Sum / $passCount) } else { $null }
  $sample.words = & $per $(if ($Metrics -and $Metrics.words) { $Metrics.words.laid_out }) $ms
  $sample.lines = & $per $Lines
  $sample.private_mb = Round3 ([double]$After.PrivateBytes / 1MB)
  $sample.handles = [int]$After.Handles
  # The state file's loops as the span ended, by count (a build without them says nothing)
  $sample.loops = $(if ($null -ne $Loops) { [int]$Loops })
  # Windows' own, in its clock ticks: a check on the cycles
  $sample.ticks = [ordered]@{
    cpu_ms = Round3 (([double]$After.CpuMs - [double]$Before.CpuMs) / $s)
    gui_thread_ms = $(if ($null -ne $After.ThreadMs -and $null -ne $Before.ThreadMs) { Round3 (([double]$After.ThreadMs - [double]$Before.ThreadMs) / $s) })
  }
  if ($Metrics -and $Metrics.budget) {
    $r = $Metrics.budget.at_rest
    $sample.at_rest = [ordered]@{ frames = $r.frames; timers = $r.timers; tasks = $r.tasks; task_ms = $r.task_ms; lines = $Metrics.budget.lines }
    $sample.on_purpose = @($Metrics.budget.on_purpose | Where-Object { $_ })
  }
  $sample.top = Get-MetricsTop $Metrics
  # Which of the GUI's threads ran, by their cycles: what the process's CPU went on, whatever the
  # build (metrics' threads count in ticks)
  $sample.threads = @($threads | Sort-Object { - $_.cycles } | Select-Object -First 5 |
      ForEach-Object { [ordered]@{ tid = $_.tid; name = $(if ($_.name) { $_.name } else { '(unnamed)' }); ms = Round3 ($_.cycles / $CycleRate / $s) } })
  # And the mux servers', by name: where a server's CPU went
  $sample.server_threads = @(Get-TopThreads $Before.ServerThreads $After.ServerThreads ($CycleRate * $s) -Whole (Get-CycleDelta $Before.Servers $After.Servers))
  $sample
}

# The threads that ran most between two lists of threads' cycles ([{Tid, Name, Cycles}]), the
# first -Top, each as its tid, name and cycles over -Per (cycles a millisecond, times the span's
# seconds for ms a second). With -Whole, the process's cycles over the same time: what's left of
# them once every thread alive at both ends is taken off went on threads that started and ended
# between, said as '(threads gone by the end)' when it's a tenth of a unit or more
function Get-TopThreads([object[]]$Before, [object[]]$After, [double]$Per, [int]$Top = 5, [AllowNull()]$Whole) {
  if (-not $Before -or -not $After -or $Per -le 0) { return @() }
  $all = @(Get-ThreadCycles $Before $After)
  $list = @($all | Sort-Object { - $_.cycles } | Select-Object -First $Top |
      ForEach-Object { [ordered]@{ tid = $_.tid; name = $(if ($_.name) { $_.name } else { '(unnamed)' }); ms = Round3 ($_.cycles / $Per) } })
  if ($null -ne $Whole) {
    $gone = [double]$Whole - [double](($all | Measure-Object cycles -Sum).Sum)
    if ($gone / $Per -ge 0.1) { $list += [ordered]@{ tid = 0; name = '(threads gone by the end)'; ms = Round3 ($gone / $Per) } }
  }
  @($list | Sort-Object { - $_.ms })
}

# Each thread's cycles between two snapshots' thread lists ([{Tid, Name, Cycles}]), those in both
# that ran at all
function Get-ThreadCycles([object[]]$Before, [object[]]$After) {
  $was = @{}
  foreach ($b in @($Before)) { if ($b -and [long]$b.Cycles -ge 0) { $was[[int]$b.Tid] = [long]$b.Cycles } }
  foreach ($a in @($After)) {
    if (-not $a -or [long]$a.Cycles -lt 0 -or -not $was.ContainsKey([int]$a.Tid)) { continue }
    $d = [long]$a.Cycles - $was[[int]$a.Tid]
    if ($d -gt 0) { [pscustomobject]@{ tid = [int]$a.Tid; name = [string]$a.Name; cycles = [double]$d } }
  }
}

# One run of an action as numbers: -StartMs just before it was posted (Unix milliseconds, with the
# fraction), -ShownMs the state file's time_ms of the first frame that showed it (or, for a call,
# when it returned) and -SettledMs of the first after with nothing animating; -Before and -After
# the cycles from just before it to the window quiet again ({Thread, Process, State, Servers}:
# the GUI thread's, the process's, the state file's writer's, and [{Pid, Cycles}] of the mux
# servers), the frames painted meanwhile, and the metrics reply covering it (nothing for a build
# without metrics). -Gui names the GUI's own action, whose time metrics' budget.actions has (its
# longest, if the run made more than one); -Bytes what a writer wrote, for its megabytes a second.
# -Warmup marks a run that's only a first time's.
function ConvertTo-ActionSample {
  param([Parameter(Mandatory)][double]$StartMs, [AllowNull()]$ShownMs, [AllowNull()]$SettledMs,
    [AllowNull()]$Before, [AllowNull()]$After, [AllowNull()]$Frames, [AllowNull()]$Metrics, [string]$Gui,
    [double]$CycleRate = 3000000, [double]$Bytes = 0, [switch]$Warmup)
  $since = { param($t) if ($null -eq $t) { $null } else { Round3 ([Math]::Max([double]0, [double]$t - $StartMs)) } }
  $cpu = { param($cycles) if ($null -eq $cycles) { $null } else { Round3 ([Math]::Max([double]0, [double]$cycles) / $CycleRate) } }
  $sample = [ordered]@{ warmup = [bool]$Warmup }
  $sample.ms = & $since $ShownMs
  $sample.settled_ms = & $since $SettledMs
  $sample.gui_ms = $null
  if ($Metrics -and $Gui) {
    $a = @($Metrics.budget.actions | Where-Object { $_ -and $_.action -eq $Gui -and [int]$_.count -ge 1 })[0]
    if ($a) { $sample.gui_ms = Round3 $a.max_ms; $sample.gui_count = [int]$a.count }
    # Its frame to the screen (lbterm-extras/src/present.rs): the last one the GUI saw go up
    $on = @($Metrics.present.actions | Where-Object { $_ -and $_.action -eq $Gui -and $_.last })[0]
    if ($on) {
      $sample.paint_ms = Round3 $on.last.paint_ms
      # Seen going up, unless its window was hidden or its frame overtaken or lost
      if ($null -ne $on.last.screen_ms) {
        $sample.screen_ms = Round3 $on.last.screen_ms
        $sample.after_present_ms = Round3 ([double]$on.last.screen_ms - [double]$on.last.present_ms)
        $sample.post_screen_ms = Round3 ([double]$on.last.screen_unix_ms - $StartMs)
      }
      $sample.screen_hidden = [int]$on.hidden
      $sample.screen_overtaken = [int]$on.overtaken
      $sample.screen_lost = [int]$on.lost
    }
  }
  # Its motion: every episode the run played, added up
  $mo = if ($Metrics -and $Metrics.present) { $Metrics.present.motion }
  if ($mo -and [int]$mo.episodes -ge 1) {
    $sample.episodes = [int]$mo.episodes
    $sample.slow_paints = [int]$mo.slow_paints
    $sample.motion_paint_ms = Round3 $mo.longest_paint_ms
    $sample.motion_hidden = [int]$mo.covered
    if ([int]$mo.measured -ge 1) {
      $sample.missed = [int]$mo.missed
      $sample.refreshes = [int]$mo.refreshes
      # The screen's own counter over the same span (a build from before 2026-11-08 has none, and
      # its refreshes are this counter's, which PITFALLS 166 says not to trust)
      if ($null -ne $mo.counted) { $sample.refreshes_counted = [int]$mo.counted }
      $sample.motion_first_ms = Round3 $mo.first_ms.mean
      $sample.motion = @($mo.names | ForEach-Object { $_.name })
    }
  }
  if ($Before -and $After) {
    $sample.gui_thread_ms = & $cpu ([long]$After.Thread - [long]$Before.Thread)
    $state = if ($null -ne $After.State -and $null -ne $Before.State -and [long]$After.State -ge 0 -and [long]$Before.State -ge 0) { [long]$After.State - [long]$Before.State } else { 0 }
    $sample.cpu_ms = & $cpu ([long]$After.Process - [long]$Before.Process - $state)
    $sample.state_ms = & $cpu $state
    $sample.server_cpu_ms = & $cpu (Get-CycleDelta $Before.Servers $After.Servers)
    $sample.server_threads = @(Get-TopThreads $Before.ServerThreads $After.ServerThreads $CycleRate -Whole (Get-CycleDelta $Before.Servers $After.Servers))
    $sample.threads = @(Get-TopThreads $Before.Threads $After.Threads $CycleRate -Whole ([long]$After.Process - [long]$Before.Process))
  }
  # What the GUI process and its servers hold once the run is quiet (a reader too old to say has none)
  if ($After -and $After.PrivateBytes) { $sample.private_mb = Round3 ([double]$After.PrivateBytes / 1MB) }
  if ($After -and $After.ServerPrivateBytes) { $sample.server_private_mb = Round3 ([double]$After.ServerPrivateBytes / 1MB) }
  $sample.frames = if ($null -ne $Frames) { [int]$Frames } else { $null }
  $sample.messages = if ($Metrics) { [int]$Metrics.totals.messages } else { $null }
  $sample.mb_per_s = if ($Bytes -gt 0 -and $sample.ms -gt 0) { Round3 ($Bytes / 1MB / ($sample.ms / 1000)) } else { $null }
  if ($Metrics) {
    $paint = @($Metrics.messages | Where-Object { $_ -and $_.site -eq 'WM_PAINT' })[0]
    $sample.wm_paint = if ($paint) { [int]$paint.count } else { 0 }
    $sample.tasks = [int]$Metrics.totals.tasks
  }
  # What font setup it did (wezterm-font's lbterm.rs and the GUI's counters.rs): a build from before
  # these were counted has no fonts, so none of them is measured
  if ($Metrics -and $Metrics.fonts) {
    $f = $Metrics.fonts
    $sample.font_loads = [int]$f.fonts.count
    $sample.font_files = [int]$f.files_read.count
    $sample.fallback_searches = [int]$f.fallback_searches.count
    $sample.reshapes = [int]$f.reshapes.count
    $sample.atlas_rebuilds = [int]$f.atlas_made.count
  }
  # What the caches did (lfucache's counts): a build from before they were counted has no caches,
  # so nothing of them is measured; a cache that did nothing in the span isn't in the reply
  if ($Metrics -and $Metrics.PSObject.Properties['caches']) {
    $q = if ($Metrics.caches) { $Metrics.caches.PSObject.Properties['line_quad_cache'] } else { $null }
    $sample.lines_laid_out = if ($q) { [int]$q.Value.misses } else { 0 }
  }
  # Where its time went by upstream's timed histograms (shape.harfbuzz, paint_pane.lines,
  # send_actions_to_mux.perform_actions.latency and the rest): each one's count times its mean,
  # the eight largest, said and not judged
  if ($Metrics -and $Metrics.histograms) {
    $spent = [ordered]@{}
    foreach ($h in @($Metrics.histograms | Where-Object { $_ -and $_.unit -eq 'ms' -and [double]$_.count -gt 0 } | Sort-Object { [double]$_.count * [double]$_.mean } -Descending | Select-Object -First 8)) {
      $spent[[string]$h.name] = Round3 ([double]$h.count * [double]$h.mean)
    }
    if ($spent.Count) { $sample.spent_ms = $spent }
  }
  $sample.top = Get-MetricsTop $Metrics
  $sample
}

# Keys typed one after another (budgets.ps1's key-burst and key-held): each key's echo, from its
# post (-Posts, Unix ms) to the first frame of -Seen ({T, X, Y}: each frame's time_ms and the
# cursor it drew, in order) whose cursor is past it, starting from the cursor at (-X, -Y) with the
# key k (from 0) moving it to X + k + 1, or off the row. Their median (the lower and upper middle's
# mean for an even count) as echo_ms, the slowest as echo_max_ms, and the last key's as ms: the
# run's own time, its last key to the frame that caught up with it. A key no frame shows is left
# out; none shown is nothing at all.
function ConvertTo-KeyEchoes([double[]]$Posts = @(), [object[]]$Seen = @(), [int]$X, [int]$Y) {
  $echoes = @(for ($k = 0; $k -lt $Posts.Count; $k++) {
    $f = @($Seen | Where-Object { [int]$_.Y -ne $Y -or [int]$_.X -ge $X + $k + 1 })[0]
    if ($f) { Round3 ([double]$f.T - $Posts[$k]) }
  })
  if (-not $echoes.Count) { return @{} }
  $sorted = @($echoes | Sort-Object)
  $n = $sorted.Count
  $median = if ($n % 2) { $sorted[($n - 1) / 2] } else { ($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2 }
  @{
    echoes      = $echoes
    echo_ms     = Round3 $median
    echo_max_ms = $sorted[-1]
    ms          = $echoes[-1]
  }
}

#endregion

#region Judging

# Who ran across a scenario's spans, added up by site, most first: what a failure names
function Get-BudgetTopSites([object[]]$Samples, [int]$Top = 3) {
  $sum = { param($list, $key, $count)
    $by = [ordered]@{}
    foreach ($e in @($list)) {
      if (-not $e) { continue }
      $k = [string]$e.$key
      if (-not $by.Contains($k)) { $by[$k] = [ordered]@{ $key = $k; n = 0.0; ms = 0.0 } }
      $by[$k].n += [double]$e.$count
      if ($null -ne $e.ms -and $count -ne 'ms') { $by[$k].ms += [double]$e.ms }
    }
    @($by.Values | Sort-Object { - $_.n } | Select-Object -First $Top)
  }
  $tops = @($Samples | ForEach-Object { $_.top } | Where-Object { $_ })
  $threads = @($Samples | ForEach-Object { $_.threads } | Where-Object { $_ })
  $serverThreads = @($Samples | ForEach-Object { $_.server_threads } | Where-Object { $_ })
  if (-not $tops -and -not $threads -and -not $serverThreads) { return $null }
  [ordered]@{
    requests = @(& $sum ($tops | ForEach-Object { $_.requests }) 'site' 'frames')
    tasks    = @(& $sum ($tops | ForEach-Object { $_.tasks }) 'site' 'count')
    timers   = @(& $sum ($tops | ForEach-Object { $_.timers }) 'site' 'fired')
    messages = @(& $sum ($tops | ForEach-Object { $_.messages }) 'site' 'count')
    # The PDUs the window sent and took, by kind (a build before Get-MetricsTop kept them, none)
    pdus_sent = @(& $sum ($tops | ForEach-Object { $_.mux } | Where-Object { $_ -and [double]$_.sent -gt 0 }) 'pdu' 'sent')
    pdus_received = @(& $sum ($tops | ForEach-Object { $_.mux } | Where-Object { $_ -and [double]$_.received -gt 0 }) 'pdu' 'received')
    # By name, their ms (a second, at rest) added up over the spans or runs
    threads  = @(& $sum $threads 'name' 'ms')
    server_threads = @(& $sum $serverThreads 'name' 'ms')
  }
}

# A number as the table says it: up to three figures, no trailing zeros
function Format-BudgetNumber($x) {
  if ($null -eq $x) { return '-' }
  $d = [double]$x
  $a = [Math]::Abs($d)
  $f = if ($a -ge 100) { '{0:0}' } elseif ($a -ge 10) { '{0:0.#}' } elseif ($a -ge 1) { '{0:0.##}' } else { '{0:0.###}' }
  [string]::Format([cultureinfo]::InvariantCulture, $f, $d)
}

# A statistic of some values: the median, the 95th percentile (nearest rank) or the first
function Get-BudgetStat([double[]]$Values, [string]$Stat) {
  if (-not @($Values).Count) { return $null }
  switch ($Stat) {
    'p95' { Get-Percentile $Values 95 }
    'first' { [double]@($Values)[0] }
    default { Get-Median $Values }
  }
}

# A scenario judged. At rest each metric's median over the spans; for an action each metric's
# statistic (ActionMetrics: the median, the 95th percentile, or the first) of its runs, the warm-up
# runs only in `first`. Each held to its budget: at or under its max, or at or over its min, and
# the scenario passes when every budget it has does. `value` is what's judged; at rest `median` is
# the same. A metric with a budget that nothing measured (in no span or run: a build without it,
# a window too hidden for anything to be seen going up) isn't judged, and is said: the scenario's
# `unmeasured` names each, and the run's summary counts the scenarios with any, so a pass is never
# taken for what it didn't check. Without spans or runs, or with -Errors (its setup threw, or it
# stopped being what it set up), it's an error: what was measured isn't the scenario.
#
# Spans and runs measured on a busy PC (their pc.busy, Add-PcSample's) are left out while at least
# half the scenario's -Runs were measured on a calm one; with fewer, every one is judged, and one
# over budget then is `busy`, not `fail`: over, but on a PC too busy to say it's LBTerm's. pc says
# how many were calm and busy, which were judged, and the judged ones' medians of what others took
# of their CPUs, the PC and the clock.
function Test-BudgetScenario {
  param([Parameter(Mandatory)]$Scenario, [object[]]$Samples = @(), [string[]]$Errors = @(), [string[]]$Lines = @(), [string[]]$Notes = @())
  $all = @($Samples | Where-Object { $_ })
  $isAction = $Scenario.Kind -eq 'action'
  # An action's warm-up runs say only what a first time costs
  $measured = @($all | Where-Object { -not ($isAction -and $_.warmup) })
  $warm = @($all | Where-Object { $isAction -and $_.warmup })
  $calm = @($measured | Where-Object { -not ($_.pc -and $_.pc.busy) })
  $need = [Math]::Max(1, [int][Math]::Ceiling([double]$(if ($Scenario.Runs) { $Scenario.Runs } else { 1 }) / 2))
  $onCalm = $calm.Count -ge $need
  $samples = if ($onCalm) { $calm } else { $measured }
  $pc = $null
  if (@($measured | Where-Object { $_.pc }).Count) {
    $med = { param($k) Round3 (Get-Median @($samples | Where-Object { $_.pc } | ForEach-Object { [double]$_.pc[$k] })) }
    $pc = [ordered]@{
      calm = $calm.Count; busy = $measured.Count - $calm.Count; judged = $(if ($onCalm) { 'calm' } else { 'all' })
      others = & $med 'others'; pc = & $med 'pc'; clock = & $med 'clock'; ghz = & $med 'ghz'
    }
    $why = @($measured | Where-Object { $_.pc -and $_.pc.busy } | ForEach-Object { $_.pc.why } | Select-Object -Unique)
    if ($why) { $pc.why = @($why) }
  }
  $defs = Get-BudgetMetrics $Scenario.Kind
  $metrics = [ordered]@{}
  $over = [Collections.Generic.List[string]]::new()
  $unmeasured = [Collections.Generic.List[string]]::new()
  foreach ($m in $defs.Keys) {
    $def = $defs[$m]
    $field = if ($def.field) { $def.field } else { $m }
    $stat = if ($def.stat) { $def.stat } else { 'median' }
    $from = if ($stat -eq 'first') { $warm } else { $samples }
    $values = @($from | ForEach-Object { $_.$field } | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ })
    $b = $Scenario.Budgets[$m]
    $entry = [ordered]@{ unit = $def.unit }
    if (-not $isAction) { $entry.median = $null }
    $entry.stat = $stat
    $entry.value = $null
    $entry.max = $null
    $entry.values = @($values)
    if ($values.Count) {
      $entry.value = Round3 (Get-BudgetStat $values $stat)
      if (-not $isAction) { $entry.median = $entry.value }
      $entry.max = Round3 (($values | Measure-Object -Maximum).Maximum)
    }
    if ($b) {
      if ($null -ne $b.max) { $entry.budget = $b.max }
      if ($null -ne $b.min) { $entry.budget_min = $b.min }
      if ($b.why) { $entry.why = $b.why }
      if ($values.Count) {
        $entry.pass = if ($null -ne $b.min) { $entry.value -ge $b.min } else { $entry.value -le $b.max }
        if (-not $entry.pass) {
          $its = if ($null -ne $b.min) { "at least $(Format-BudgetNumber $b.min)" } else { Format-BudgetNumber $b.max }
          $over.Add("$m $(Format-BudgetNumber $entry.value)$(if ($def.unit) { " $($def.unit)" }) (its budget $its)")
        }
      } else {
        $entry.unmeasured = $true
        $unmeasured.Add($m)
      }
    }
    $metrics[$m] = $entry
  }
  $errors = @($Errors | Where-Object { $_ })
  $result = if (-not $samples.Count -or $errors.Count) { 'error' } elseif ($over.Count) { if ($pc -and -not $onCalm) { 'busy' } else { 'fail' } } else { 'pass' }
  $judged = [ordered]@{
    name     = $Scenario.Name
    kind     = $Scenario.Kind
    domain   = $Scenario.Domain
    what     = $Scenario.What
    result   = $result
    over     = @($over)
    unmeasured = @($unmeasured)
    errors   = $errors
    bug      = $Scenario.Bug
    spans    = $samples.Count
    metrics  = $metrics
    top      = Get-BudgetTopSites $samples
    lines    = @($Lines | Where-Object { $_ })
    notes    = @($Notes | Where-Object { $_ })
  }
  if ($pc) { $judged.pc = $pc }
  if ($isAction) { $judged.gui = $Scenario.Gui; $judged.warmup = $warm.Count }
  $judged
}

#endregion

#region The PC while it measured (lbterm/PERF.md, "The PC while it measured")

# What the PC did over a stretch, from Start-PcWatch and Stop-PcWatch (harness.psm1): -Start and
# -Stop each {At, Busy, Own} (and -Start's Mask, -Stop's Clock), Own the cycles of the measurement's
# own processes by pid, and -CycleRate cycles a millisecond. As numbers:
# - seconds: how long the stretch was
# - cpus, of: how many CPUs the measured processes could run on (the warden job's) and how many
#   the PC has
# - own: how many CPUs' worth the measurement's own processes took (the runner, the GUI, its
#   servers and everything they started; one that started during the stretch counts whole)
# - others: the share of those CPUs' time that the rest of the PC took, the measurement's own taken
#   off: 0 is nothing else ran there, 1 is others ran on all of them throughout. A test window runs
#   at below-normal priority, so whatever else wants those CPUs comes first
# - pc: the share of the whole PC's time that was busy, everything included
# - clock: those CPUs' clock over the stretch, a share of the nominal one (1 is 3.0 GHz here, above
#   1 with turbo), each CPU's weighted by how busy it was, since an idle one's says little; ghz the
#   same in GHz
function ConvertTo-PcSpan {
  param([Parameter(Mandatory)]$Start, [Parameter(Mandatory)]$Stop,
    [double]$CycleRate = 3000000, [double]$Frequency = [Diagnostics.Stopwatch]::Frequency)
  $s = ([double]$Stop.At - [double]$Start.At) / $Frequency
  if ($s -le 0) { throw "a stretch of $s s" }
  $n = [Math]::Min(@($Start.Busy).Count, @($Stop.Busy).Count)
  if ($n -lt 1) { throw 'no processor''s busy time read' }
  $mask = [long]$Start.Mask
  $mine = @(for ($i = 0; $i -lt $n; $i++) { if ($mask -le 0 -or ($i -lt 63 -and ($mask -band ([long]1 -shl $i)))) { $i } })
  if (-not $mine.Count) { throw "the affinity 0x$($mask.ToString('x')) names none of the PC's $n processors" }
  $wall = $s * 1e7
  $delta = @(for ($i = 0; $i -lt $n; $i++) { [Math]::Max([double]0, [double]@($Stop.Busy)[$i] - [double]@($Start.Busy)[$i]) })
  $onMine = ($mine | ForEach-Object { $delta[$_] } | Measure-Object -Sum).Sum
  $all = ($delta | Measure-Object -Sum).Sum
  # The measurement's own: each process's cycles over the stretch, all of them for one that wasn't
  # there at its start
  $ownCycles = 0.0
  if ($Stop.Own) {
    foreach ($p in $Stop.Own.Keys) {
      $was = if ($Start.Own -and $Start.Own.ContainsKey($p)) { [double]$Start.Own[$p] } else { 0.0 }
      $ownCycles += [Math]::Max([double]0, [double]$Stop.Own[$p] - $was)
    }
  }
  $ownCores = $ownCycles / $CycleRate / ($s * 1000)
  $others = [Math]::Max([double]0, $onMine / $wall - $ownCores) / $mine.Count
  # The clock: PDH names group 0's processor i "0,i"
  $sum = 0.0; $weight = 0.0; $plain = [Collections.Generic.List[double]]::new()
  foreach ($i in $mine) {
    $c = $Stop.Clock["0,$i"]
    if ($null -eq $c) { throw "no clock read for processor $i (PDH's \Processor Information(0,$i)\% Processor Performance)" }
    $plain.Add([double]$c)
    $sum += [double]$c * $delta[$i]; $weight += $delta[$i]
  }
  $clock = if ($weight -gt 0) { $sum / $weight } else { ($plain | Measure-Object -Average).Average }
  [ordered]@{
    seconds = Round3 $s
    cpus    = $mine.Count
    of      = $n
    own     = Round3 $ownCores
    others  = Round3 ([Math]::Min([double]1, $others))
    pc      = Round3 ([Math]::Min([double]1, $all / $wall / $n))
    clock   = Round3 ($clock / 100)
    ghz     = Round3 ($clock / 100 * $CycleRate / 1e6)
  }
}

# What a span or run measures by its cycles, which count time at the nominal clock whatever the
# processor's runs at: scaled by the PC's clock (ConvertTo-PcSpan's), so each says what it would
# have taken at the nominal 3.0 GHz, and a run with turbo holds up against one without
$script:ClockScaled = @{
  rest   = @('gui_thread_ms', 'cpu_ms', 'state_ms', 'server_cpu_ms', 'conpty_cpu_ms', 'frame_ms')
  action = @('gui_thread_ms', 'cpu_ms', 'state_ms', 'server_cpu_ms')
}

# A sample (a span's or a run's numbers, an ordered dictionary) with the PC's stretch (-Pc,
# ConvertTo-PcSpan's) laid on it: kept as its pc, judged busy against -Rules (Read-Budgets' Pc: when
# others took more of its CPUs than others_max, or the PC was busier than pc_max, each said in
# pc.why), and what it measured by cycles scaled to the nominal clock, what was measured kept in raw
function Add-PcSample([Parameter(Mandatory)]$Sample, [Parameter(Mandatory)]$Pc, [string]$Kind = 'rest', $Rules) {
  $p = [ordered]@{}
  foreach ($k in $Pc.Keys) { $p[$k] = $Pc[$k] }
  $why = @(
    if ($Rules -and $null -ne $Rules.OthersMax -and [double]$p.others -gt $Rules.OthersMax) { "others took $([Math]::Round(100 * [double]$p.others))% of its CPUs (over $([Math]::Round(100 * $Rules.OthersMax))%)" }
    if ($Rules -and $null -ne $Rules.PcMax -and [double]$p.pc -gt $Rules.PcMax) { "the PC was $([Math]::Round(100 * [double]$p.pc))% busy (over $([Math]::Round(100 * $Rules.PcMax))%)" }
  )
  $p.busy = [bool]$why.Count
  if ($why.Count) { $p.why = $why -join ', ' }
  $Sample['pc'] = $p
  $raw = [ordered]@{}
  foreach ($m in $script:ClockScaled[$(if ($Kind -eq 'action') { 'action' } else { 'rest' })]) {
    if ($null -eq $Sample[$m]) { continue }
    $raw[$m] = $Sample[$m]
    $Sample[$m] = Round3 ([double]$Sample[$m] * [double]$p.clock)
  }
  $Sample['raw'] = $raw
  $Sample
}

# A sample's PC said in a few words for a run's line: what others took of its CPUs, the PC, the
# clock, and busy when it was
function Format-PcSample($Sample) {
  $p = $Sample.pc
  if (-not $p) { return '' }
  "; others $([Math]::Round(100 * [double]$p.others))% of its $($p.cpus) CPUs, the PC $([Math]::Round(100 * [double]$p.pc))%, clock $(Format-BudgetNumber $p.ghz) GHz$(if ($p.busy) { ", busy: $($p.why)" })"
}

#endregion

#region The report, its table and the history

# report.json's shape: the run, the build, and every scenario as judged
function New-BudgetReport {
  param([string]$Run, [string]$Bin, [string]$Build, [string]$Commit, [datetime]$Started, [object[]]$Results = @(), [int]$Runs, [double]$Seconds)
  $r = @($Results | Where-Object { $_ })
  [ordered]@{
    run      = $Run
    bin      = $Bin
    build    = $Build
    commit   = $Commit
    started  = $Started.ToString('o')
    seconds  = [Math]::Round(((Get-Date) - $Started).TotalSeconds, 1)
    runs     = $Runs
    span_s   = $Seconds
    summary  = [ordered]@{
      pass  = @($r | Where-Object { $_.result -eq 'pass' }).Count
      fail  = @($r | Where-Object { $_.result -eq 'fail' }).Count
      # Over budget, but measured on a PC too busy to say it's LBTerm's (Test-BudgetScenario)
      busy  = @($r | Where-Object { $_.result -eq 'busy' }).Count
      error = @($r | Where-Object { $_.result -eq 'error' }).Count
      # Of those that passed or failed, the ones with a budget nothing measured
      unmeasured = @($r | Where-Object { $_.result -ne 'error' -and @($_.unmeasured | Where-Object { $_ }).Count }).Count
      # Medians that moved past the drift's share since the last run of the same scenario
      drift = @($r | Where-Object { @($_.drift | Where-Object { $_ }).Count }).Count
    }
    scenarios = $r
  }
}

# The table the run prints: a row a scenario at rest with its medians, then a row an action with
# its times and costs, then for each failure what's over, who ran most, and the bug when
# budgets.json names it
function Format-BudgetTable($Report) {
  $tables = @(
    @{ kind = 'rest'; head = 'scenario'; cols = [ordered]@{ frames = 'frames/s'; gui_thread_ms = 'gui ms/s'; cpu_ms = 'cpu ms/s'; server_cpu_ms = 'server'; conpty_cpu_ms = 'conpty'; tasks = 'tasks/s'; timers = 'timers/s'; frame_ms = 'frame ms'; words = 'words/s'; lines = 'lines/s' } }
    @{ kind = 'action'; head = 'action'; cols = [ordered]@{ ms = 'p50 ms'; p95_ms = 'p95 ms'; gui_ms = 'gui ms'; screen_ms = 'screen'; settled_ms = 'settled'; first_ms = 'first'; gui_thread_ms = 'gui cpu'; cpu_ms = 'cpu'; server_cpu_ms = 'server'; frames = 'frames'; missed = 'missed'; mb_per_s = 'MB/s' } }
  )
  $w = [Math]::Max(8, (@($Report.scenarios | ForEach-Object { $_.name.Length }) + 8 | Measure-Object -Maximum).Maximum)
  $first = $true
  foreach ($t in $tables) {
    $rows = @($Report.scenarios | Where-Object { $(if ($_.kind) { $_.kind } else { 'rest' }) -eq $t.kind })
    if (-not $rows) { continue }
    if (-not $first) { '' }
    $first = $false
    $head = $t.head.PadRight($w) + '  result'
    foreach ($c in $t.cols.Values) { $head += '  ' + $c.PadLeft(8) }
    $head += '  ' + 'others'.PadLeft(6) + '  ' + 'GHz'.PadLeft(4)
    $head
    foreach ($s in $rows) {
      $row = $s.name.PadRight($w) + '  ' + $s.result.PadRight(6)
      foreach ($m in $t.cols.Keys) {
        $e = $s.metrics[$m]
        $v = if ($e) { if ($e.Contains('value')) { $e.value } else { $e.median } }
        $cell = Format-BudgetNumber $v
        if ($e -and $e.Contains('pass') -and -not $e.pass) { $cell = "$cell!" }
        $row += '  ' + $cell.PadLeft(8)
      }
      # What others took of its CPUs and their clock, the judged spans' or runs' medians
      $p = $s.pc
      $row += '  ' + $(if ($p) { "$([Math]::Round(100 * [double]$p.others))%" } else { '-' }).PadLeft(6) + '  ' + $(if ($p) { Format-BudgetNumber $p.ghz } else { '-' }).PadLeft(4)
      $row
    }
  }
  foreach ($s in @($Report.scenarios | Where-Object { $_.result -ne 'pass' -or ($_.pc -and $_.pc.busy) })) {
    ''
    "$($s.name): $($s.result)$(if ($s.over) { ', over: ' + ($s.over -join '; ') })"
    $p = $s.pc
    if ($p -and $p.busy) {
      $unit = if ($s.kind -eq 'action') { 'runs' } else { 'spans' }
      $judged = if ($p.judged -eq 'calm') { "judged on the $($p.calm) calm ones" } else { "judged on all of them: too few calm ones, so over budget is busy, not fail" }
      "  the PC: $($p.busy) of its $unit busy ($(@($p.why) -join '; ')), $judged"
    }
    foreach ($e in $s.errors) { "  error: $e" }
    if ($s.bug) { "  known: $($s.bug)" }
    $t = $s.top
    if ($t) {
      $say = { param($list, $key, $what) if (@($list).Count) { "  ${what}: " + (@($list | ForEach-Object { "$($_.$key) ($(Format-BudgetNumber $_.n)$(if ($_.ms) { ", $(Format-BudgetNumber $_.ms) ms" }))" }) -join ', ') } }
      & $say $t.requests 'site' 'frames asked by'
      & $say $t.tasks 'site' 'tasks'
      & $say $t.timers 'site' 'timers'
      & $say $t.threads 'name' 'threads (ms/s, the spans added)'
      & $say $t.server_threads 'name' 'the server''s threads (ms, added up)'
    }
    foreach ($l in @($s.lines | Select-Object -First 4)) { "  said: $l" }
    if (@($s.lines).Count -gt 4) { "  said $(@($s.lines).Count - 4) more lines (report.json)" }
  }
  # What passed without all of it being measured, and what moved since the last run
  $partly = @($Report.scenarios | Where-Object { $_.result -ne 'error' -and @($_.unmeasured | Where-Object { $_ }).Count })
  if ($partly) {
    ''
    'budgets nothing measured (not judged):'
    foreach ($s in $partly) { "  $($s.name): $(@($s.unmeasured) -join ', ')" }
  }
  # Actions whose time may be a frame or more long: the runner's recorder never read a frame
  # straight before the one that showed it (unread_frames; lbterm/PITFALLS.md, 355)
  $unread = @($Report.scenarios | Where-Object { $_.result -ne 'error' -and $_.metrics.unread_frames -and [double]$_.metrics.unread_frames.max -gt 0 })
  if ($unread) {
    ''
    'frames the runner never read straight before the one that showed it (its ms can be that many frames long):'
    foreach ($s in $unread) {
      $v = @($s.metrics.unread_frames.values)
      "  $($s.name): $(@($v | Where-Object { [double]$_ -gt 0 }).Count) of $($v.Count) runs, up to $([long]$s.metrics.unread_frames.max)"
    }
  }
  $moved =@($Report.scenarios | Where-Object { @($_.drift | Where-Object { $_ }).Count })
  if ($moved) {
    ''
    "moved since the last run of each (over $($script:DriftShare * 100)% and $($script:DriftFloor) of its unit):"
    foreach ($s in $moved) { "  $($s.name): $(@($s.drift | ForEach-Object { "$($_.metric) $(Format-BudgetNumber $_.was) to $(Format-BudgetNumber $_.now) ($($_.run))" }) -join '; ')" }
  }
  ''
  $m = $Report.summary
  "$($m.pass) passed, $($m.fail) over budget, $(if ($m.busy) { "$($m.busy) over on a busy PC (run again when it's quieter), " })$($m.error) couldn't be measured$(if ($m.unmeasured) { ", $($m.unmeasured) with budgets nothing measured" })$(if ($m.drift) { ", $($m.drift) moved since their last run" }), in $([Math]::Round($Report.seconds / 60, 1)) min"
}

# How far a median may move from the last run of the same scenario before the run says so: a
# share of the last, and at least so much of its unit (a frame a second, a millisecond), since
# what's near nothing doubles for nothing. Said, not judged: a budget is what fails a run
$script:DriftShare = 0.25
$script:DriftFloor = 1.0

# Where every tree's runs keep their history: the main checkout's target\budgets, the same file
# whichever slot or worktree ran, so a drift across branches and builds shows in one place
function Get-BudgetHistoryPath([string]$Repo = $script:Repo) {
  $common = git -C $Repo rev-parse --path-format=absolute --git-common-dir 2>$null
  $main = if ($LASTEXITCODE -eq 0 -and $common) { Split-Path ($common -replace '/', '\') -Parent } else { $Repo }
  Join-Path $main 'target\budgets\history.jsonl'
}

# The history's last line for each scenario, newest first by file order: name -> @{ run; metrics }
function Read-BudgetHistory([string]$Path) {
  $last = @{}
  if (-not (Test-Path -LiteralPath $Path)) { return $last }
  foreach ($line in [IO.File]::ReadAllLines($Path)) {
    if (-not $line.Trim()) { continue }
    $h = try { $line | ConvertFrom-Json -AsHashtable } catch { $null }
    if (-not $h -or -not $h.scenarios) { continue }
    foreach ($name in $h.scenarios.Keys) { $last[$name] = @{ run = [string]$h.run; build = [string]$h.build; clock = [string]$h.clock; metrics = $h.scenarios[$name] } }
  }
  $last
}

# A scenario's medians against its last run's (-Last, Read-BudgetHistory's): each that moved over
# the drift's share and floor, as metric, was, now and the run it's from. What's measured by cycles
# isn't held to a line from before they were scaled to the nominal clock (no clock in it)
function Get-BudgetDrift($Judged, $Last) {
  if (-not $Last -or $Judged.result -eq 'error') { return @() }
  $scaled = $script:ClockScaled[$(if ($Judged.kind -eq 'action') { 'action' } else { 'rest' })]
  @(foreach ($m in @($Judged.metrics.Keys)) {
      if ($Last.clock -ne 'nominal' -and $m -in $scaled) { continue }
      $e = $Judged.metrics[$m]
      $now = if ($e.Contains('value')) { $e.value } else { $e.median }
      $was = $Last.metrics[$m]
      if ($null -eq $now -or $null -eq $was -or -not ($was -is [ValueType])) { continue }
      $d = [Math]::Abs([double]$now - [double]$was)
      if ($d -gt $script:DriftFloor -and $d -gt $script:DriftShare * [Math]::Abs([double]$was)) {
        [ordered]@{ metric = $m; was = [double]$was; now = [double]$now; run = $Last.run }
      }
    })
}

# The run as one line of history.jsonl: the build, when, and every scenario's result and medians,
# so a slow drift shows from one run to the next
function ConvertTo-BudgetHistoryLine($Report) {
  $scen = [ordered]@{}
  foreach ($s in $Report.scenarios) {
    $e = [ordered]@{ result = $s.result }
    if ($s.kind -eq 'action') { $e.kind = 'action' }
    # The PC it was measured on, so a step in the history can be told from a busier PC or a faster clock
    if ($s.pc) { $e.pc = [ordered]@{ others = $s.pc.others; pc = $s.pc.pc; clock = $s.pc.clock; busy = $s.pc.busy } }
    foreach ($m in (Get-BudgetMetrics $s.kind).Keys) {
      $x = if ($s.metrics) { $s.metrics[$m] }
      $v = if ($x) { if ($x.Contains('value')) { $x.value } else { $x.median } }
      if ($null -ne $v) { $e[$m] = $v }
    }
    $scen[$s.name] = $e
  }
  # clock: what the CPU it measured by cycles is in. "nominal" since 2026-11-03 (Add-PcSample's
  # scaling); a line without it is as measured, at whatever clock the processor ran
  [ordered]@{
    run = $Report.run; date = $Report.started; build = $Report.build; commit = $Report.commit; bin = $Report.bin
    runs = $Report.runs; span_s = $Report.span_s; clock = 'nominal'; summary = $Report.summary; scenarios = $scen
  } | ConvertTo-Json -Compress -Depth 6
}

# Appends the run's line to the history file (UTF-8, no BOM, a line each)
function Add-BudgetHistory([Parameter(Mandatory)]$Report, [Parameter(Mandatory)][string]$Path) {
  New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
  [IO.File]::AppendAllText($Path, (ConvertTo-BudgetHistoryLine $Report) + "`n", [Text.UTF8Encoding]::new($false))
}

# What one scenario's run of perf\budgets.ps1 left in its folder, judged: its measure.json's spans,
# lines and notes against the scenario's budgets, and its result.json's failures that aren't the
# budget's own (a setup that threw) as errors
function Read-BudgetAttempt([Parameter(Mandatory)]$Scenario, [Parameter(Mandatory)]$Attempt) {
  $file = Join-Path $Attempt.Dir 'measure.json'
  $m = if (Test-Path -LiteralPath $file) { try { Get-Content -Raw -LiteralPath $file | ConvertFrom-Json -AsHashtable } catch { $null } }
  $samples = if ($m) { @($m.samples) } else { @() }
  $errors = @($Attempt.Failures | Where-Object { $_ -and $_ -notmatch '^over budget: ' })
  if (-not $samples.Count -and -not $errors) { $errors = @("nothing measured (exit $($Attempt.Exit))") }
  $judged = Test-BudgetScenario -Scenario $Scenario -Samples $samples -Errors $errors -Lines @($m.lines) -Notes (@($m.notes) + @($Attempt.Notes))
  $judged.out = $Attempt.Dir
  $judged.seconds = $Attempt.Seconds
  $judged
}

#endregion

#region The run: lbterm.ps1 budgets

# lbterm.ps1 budgets: each scenario of budgets.json (-Only some) run by perf\budgets.ps1 in a
# process of its own (as e2e runs a script: Invoke-E2eAttempt, in harness.psm1), its output in
# <Root>\<run>\<scenario>, judged as it ends; one that couldn't be measured (its setup failed)
# runs again up to -Retry times, one over budget never does (it was measured -Runs times already).
# report.json and the table at the end, and the run's line appended to <Root>\history.jsonl. A
# locked workstation doesn't stop it, as it stops e2e, since nothing here rests on a capture: each
# scenario measured while it was says so in its notes. Get-BudgetLastReport has the report.
function Invoke-BudgetRun {
  param(
    [Parameter(Mandatory)][string]$Bin,
    [string]$Build,
    [string[]]$Only,
    [int]$Runs,
    [double]$Seconds,
    [int]$Retry = 1,
    [int]$TimeoutMin = 10,
    [switch]$WaitIdle,
    [string]$Root = (Join-Path $script:Repo 'target\budgets'),
    [string]$File = $script:BudgetsFile,
    [string]$Script = $script:BudgetsScript,
    [string]$History = (Get-BudgetHistoryPath)
  )
  if (-not (Test-Path (Join-Path $Bin 'lbterm-gui.exe'))) { throw "no build in $Bin (lbterm-gui.exe): run lbterm.ps1 build first, or pass -Bin" }
  $Bin = (Resolve-Path $Bin).Path
  $b = Read-Budgets $File
  $scenarios = @(Select-BudgetScenarios $b.Scenarios $Only)
  # -Runs, when given, is every scenario's: spans at rest, runs of an action, and what it's judged by
  $given = $Runs
  if (-not $Runs) { $Runs = $b.Runs }
  if ($given) { foreach ($sc in $scenarios) { $sc.Runs = $given } }
  $started = Get-Date
  $run = $started.ToString('yyyyMMdd-HHmmss')
  $runDir = Join-Path $Root $run
  New-Item -ItemType Directory -Force $runDir | Out-Null
  $commit = (git -C $script:Repo rev-parse --short=8 HEAD 2>$null)
  $actions = @($scenarios | Where-Object Kind -eq 'action').Count
  "budgets run $run against $Bin ($Build), $($scenarios.Count - $actions) scenarios at rest, $Runs spans of $(if ($Seconds) { $Seconds } else { $b.Seconds }) s each, and $actions actions, $(if ($given) { "$given runs" } else { 'their runs' }) each, output in $runDir"
  $results = [Collections.Generic.List[object]]::new()
  $past = Read-BudgetHistory $History
  foreach ($sc in $scenarios) {
    $judged = $null
    for ($n = 0; $n -le $Retry; $n++) {
      $locked = Test-WorkstationLocked
      $dir = Join-Path $runDir $sc.Name
      if ($n -gt 0 -and (Test-Path $dir)) { Rename-AttemptFolder $dir "$($sc.Name).$n" }
      $argList = @('-Scenario', $sc.Name)
      if ($given -or $sc.Kind -ne 'action') { $argList += @('-Runs', $Runs) }
      if ($Seconds) { $argList += @('-Seconds', $Seconds) }
      $entry = [pscustomobject]@{ Name = $sc.Name; Script = 'budgets'; Path = $Script; Args = $argList }
      $a = Invoke-E2eAttempt -Entry $entry -Dir $dir -Bin $Bin -TimeoutSec ($TimeoutMin * 60) -WaitIdle:$WaitIdle
      $judged = Read-BudgetAttempt -Scenario $sc -Attempt $a
      if ($locked -or (Test-WorkstationLocked)) { $judged.notes = @($judged.notes) + 'measured with the workstation locked (LogonUI.exe): DWM shows nothing on the screen while it is, so a present may cost otherwise' }
      "  $($sc.Name)$(if ($n) { " (again, $n)" }): $($judged.result) in $($a.Seconds) s$(if ($judged.over) { ': ' + ($judged.over -join '; ') } elseif ($judged.errors) { ': ' + $judged.errors[0] })"
      if ($judged.result -ne 'error') { break }
    }
    $judged.drift = @(Get-BudgetDrift $judged $past[$sc.Name])
    $results.Add($judged)
    $report = New-BudgetReport -Run $run -Bin $Bin -Build $Build -Commit $commit -Started $started -Results $results -Runs $Runs -Seconds $(if ($Seconds) { $Seconds } else { $b.Seconds })
    $report | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $runDir 'report.json') -Encoding utf8NoBOM
  }
  $script:LastBudgetReport = $null
  if (-not $results.Count) { 'nothing to run'; return }
  $report = New-BudgetReport -Run $run -Bin $Bin -Build $Build -Commit $commit -Started $started -Results $results -Runs $Runs -Seconds $(if ($Seconds) { $Seconds } else { $b.Seconds })
  $report | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $runDir 'report.json') -Encoding utf8NoBOM
  Add-BudgetHistory $report $History
  $script:LastBudgetReport = $report
  ''
  Format-BudgetTable $report
  "report: $(Join-Path $runDir 'report.json'); history: $History"
}

# The report of the last Invoke-BudgetRun in this session, or nothing
function Get-BudgetLastReport { $script:LastBudgetReport }

#endregion

#region Two builds against each other: lbterm.ps1 budgets -Against

# The spans or runs a scenario's attempt left in its folder's measure.json, none when it left none
function Read-BudgetSamples([Parameter(Mandatory)][string]$Dir) {
  $file = Join-Path $Dir 'measure.json'
  if (-not (Test-Path -LiteralPath $file)) { return @() }
  @((Get-Content -Raw -LiteralPath $file | ConvertFrom-Json -AsHashtable).samples | Where-Object { $_ })
}

# Student's t's 99.5th percentile by degrees of freedom: a two-sided 99% interval's. Between two
# rows the lower row's (the wider interval), so a fraction of a degree never narrows it
$script:T995 = @(@(1, 63.657), @(2, 9.925), @(3, 5.841), @(4, 4.604), @(5, 4.032), @(6, 3.707), @(7, 3.499), @(8, 3.355), @(9, 3.25), @(10, 3.169), @(12, 3.055), @(15, 2.947), @(20, 2.845), @(30, 2.75), @(60, 2.66), @(120, 2.617))
function Get-T995([double]$Df) {
  $t = 2.576
  for ($i = $script:T995.Count - 1; $i -ge 0; $i--) { if ($script:T995[$i][0] -le [Math]::Max(1, [Math]::Floor($Df + 1e-9))) { return $script:T995[$i][1] } }
  $t
}

# What a difference under which can't be told from nothing: half a count for what counts whole
# events (frames, messages: in a span at rest, half an event over the span's -Seconds), else 0
$script:CountMetrics = @('frames', 'requests', 'timers', 'tasks', 'messages', 'wakes', 'words', 'lines', 'handles', 'loops', 'missed', 'refreshes', 'refreshes_counted', 'slow_paints')
function Get-BudgetFloor([string]$Metric, [string]$Kind, [double]$Seconds = 5) {
  if ($Metric -notin $script:CountMetrics) { return 0 }
  if ($Kind -ne 'action' -and $Metric -notin 'handles', 'loops') { return 0.5 / $Seconds }
  0.5
}

# Two builds' rounds of a scenario held against each other. -Before and -After are the scenario as
# judged in each round (Test-BudgetScenario's), in round order; -BeforeAll and -AfterAll as judged on
# every round's spans or runs together. For each metric both builds measured: each one's value over
# all its rounds, the difference (after less before, and as a share of before), and Welch's test on
# the rounds' own values: the difference of their means and its 99% interval (`within`, the half
# width: what a difference has to pass to be told from how each build's rounds vary). A difference
# whose interval leaves out nothing is `higher` or `lower`, else `same`; one no wider than the
# metric's floor (half a count of what counts events, Get-BudgetFloor, a span at rest -Seconds long)
# is `same` whatever the rounds say. With fewer than two rounds of either a difference is `unknown`.
# On identical builds the test calls a metric different under 1% of the time at any number of
# rounds; it sees a shift of three of a round's standard deviations 80% of the time at 5 rounds, and
# of two at 8 (lbterm/PERF.md, "Two builds against each other").
# Beside it, never deciding, the rounds paired: each round's two builds, which ran back to back, by
# the round each was judged in (its `round`), a t-test at 99% on their differences (`paired_diff`,
# `paired_within`, `paired_verdict`, with the floor and the sign rule as Welch's), how many rounds
# paired (`paired_n`) and in how many the after came higher (`paired_later`). It sees a lean the
# PC's swings hide from Welch's when both builds' rounds swing together, and is wider than Welch's
# when they don't: on this PC's first 19 comparisons it told apart 69 metrics of 400 where Welch's
# told apart 73 (PERF.md, "The rounds paired"; Lachlan's 1a of 2026-11-04, PITFALLS 113).
function Compare-BudgetRounds {
  param([Parameter(Mandatory)]$Scenario, [object[]]$Before = @(), [object[]]$After = @(),
    [Parameter(Mandatory)]$BeforeAll, [Parameter(Mandatory)]$AfterAll, [double]$Seconds = 5)
  $defs = Get-BudgetMetrics $Scenario.Kind
  $roundValues = { param($list, $m) @($list | Where-Object { $_ -and $_.metrics } | ForEach-Object { $x = $_.metrics[$m]; if ($x) { $x.value } } | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ }) }
  $meanOf = { param($v) ($v | Measure-Object -Average).Average }
  $varOf = { param($v) $mv = & $meanOf $v; (($v | ForEach-Object { ($_ - $mv) * ($_ - $mv) }) | Measure-Object -Sum).Sum / ($v.Count - 1) }
  $valueIn = { param($j, $m) if ($j -and $j.metrics) { $x = $j.metrics[$m]; if ($x -and $null -ne $x.value) { [double]$x.value } } }
  $pairedRounds = { param($before, $after, $m)
    $byRound = @{}
    foreach ($j in $before) { if ($j -and $null -ne $j.round) { $v = & $valueIn $j $m; if ($null -ne $v) { $byRound[[int]$j.round] = $v } } }
    @(foreach ($j in $after) {
        if (-not $j -or $null -eq $j.round -or -not $byRound.ContainsKey([int]$j.round)) { continue }
        $v = & $valueIn $j $m
        if ($null -ne $v) { $v - $byRound[[int]$j.round] }
      })
  }
  @(foreach ($m in $defs.Keys) {
      $b = $BeforeAll.metrics[$m]; $a = $AfterAll.metrics[$m]
      if (-not $b -or -not $a -or $null -eq $b.value -or $null -eq $a.value) { continue }
      $bv = & $roundValues $Before $m
      $av = & $roundValues $After $m
      $diff = [double]$a.value - [double]$b.value
      $floor = Get-BudgetFloor $m $Scenario.Kind $Seconds
      $meanDiff = $null; $within = $null
      if ($bv.Count -ge 2 -and $av.Count -ge 2) {
        $meanDiff = (& $meanOf $av) - (& $meanOf $bv)
        $sb = (& $varOf $bv) / $bv.Count; $sa = (& $varOf $av) / $av.Count
        $se = [Math]::Sqrt($sb + $sa)
        if ($se -gt 0) {
          $df = ($sb + $sa) * ($sb + $sa) / (($sb * $sb) / ($bv.Count - 1) + ($sa * $sa) / ($av.Count - 1))
          $within = (Get-T995 $df) * $se
        } else { $within = 0.0 }
      }
      $judge = { param($d, $w)
        if ([Math]::Abs($diff) -le $floor) { 'same' }
        elseif ($null -eq $w) { 'unknown' }
        elseif ([Math]::Abs($d) -le $w -or [Math]::Sign($d) -ne [Math]::Sign($diff)) { 'same' }
        elseif ($diff -gt 0) { 'higher' } else { 'lower' }
      }
      $verdict = & $judge $meanDiff $within
      # The rounds paired: each round's after less its before, for the rounds both builds were
      # judged in with this metric; none for rounds that don't say which they were
      $paired = & $pairedRounds $Before $After $m
      $pairedDiff = $null; $pairedWithin = $null; $pairedVerdict = $null; $pairedLater = $null
      if ($paired.Count -ge 2) {
        $pairedDiff = & $meanOf $paired
        $pairedWithin = (Get-T995 ($paired.Count - 1)) * [Math]::Sqrt((& $varOf $paired) / $paired.Count)
        $pairedVerdict = & $judge $pairedDiff $pairedWithin
        $pairedLater = @($paired | Where-Object { $_ -gt 0 }).Count
      }
      [ordered]@{
        metric = $m; unit = $defs[$m].unit; before = $b.value; after = $a.value; diff = Round3 $diff
        share = $(if ([double]$b.value -ne 0) { Round3 ($diff / [Math]::Abs([double]$b.value)) })
        mean_diff = Round3 $meanDiff; within = Round3 $within; floor = Round3 $floor; verdict = $verdict
        paired_n = $paired.Count; paired_diff = Round3 $pairedDiff; paired_within = Round3 $pairedWithin
        paired_verdict = $pairedVerdict; paired_later = $pairedLater
        before_rounds = @($bv); after_rounds = @($av)
      }
    })
}

# The comparison's table: for each scenario its rounds' PC and each metric either build measured
# something of, before, after, the difference, the 99% interval's half width and the verdict; then
# the metrics that came out higher or lower, all together
function Format-BudgetComparison($Report) {
  "$($Report.after.build) (after) against $($Report.before.build) (before), $($Report.rounds) rounds each, the order turned each round"
  foreach ($s in $Report.scenarios) {
    ''
    "$($s.name) ($($s.kind)): before $($s.before.result), after $($s.after.result)"
    foreach ($side in 'before', 'after') {
      $p = $s.$side.pc
      if ($p) { "  ${side}: others took $([Math]::Round(100 * [double]$p.others))% of its CPUs, the PC $([Math]::Round(100 * [double]$p.pc))% busy, clock $(Format-BudgetNumber $p.ghz) GHz (medians); $($p.calm) calm, $($p.busy) busy" }
    }
    $rows = @($s.comparison | Where-Object { [double]$_.before -ne 0 -or [double]$_.after -ne 0 })
    if (-not $rows) { '  every metric 0 on both'; continue }
    $w = [Math]::Max(10, (@($rows | ForEach-Object { $_.metric.Length }) | Measure-Object -Maximum).Maximum)
    # The paired column: the rounds paired's 99% half width and what it would call it (it never
    # decides: Compare-BudgetRounds), '-' with fewer than two rounds paired
    $pairedCell = { param($c) if ($null -eq $c.paired_within) { '-' } else { "+-$(Format-BudgetNumber $c.paired_within) $($c.paired_verdict)" } }
    $pw = [Math]::Max(6, (@($rows | ForEach-Object { (& $pairedCell $_).Length }) | Measure-Object -Maximum).Maximum)
    '  ' + 'metric'.PadRight($w) + '  ' + 'before'.PadLeft(8) + '  ' + 'after'.PadLeft(8) + '  ' + 'diff'.PadLeft(8) + '  ' + '+-99%'.PadLeft(8) + '  ' + 'paired'.PadRight($pw) + '  verdict'
    foreach ($c in $rows) {
      $d = Format-BudgetNumber $c.diff
      if ([double]$c.diff -gt 0) { $d = "+$d" }
      '  ' + $c.metric.PadRight($w) + '  ' + (Format-BudgetNumber $c.before).PadLeft(8) + '  ' + (Format-BudgetNumber $c.after).PadLeft(8) + '  ' + $d.PadLeft(8) + '  ' + (Format-BudgetNumber $c.within).PadLeft(8) + '  ' + (& $pairedCell $c).PadRight($pw) + "  $($c.verdict)$(if ($c.unit) { " ($($c.unit))" })"
    }
  }
  $moved = @(foreach ($s in $Report.scenarios) { foreach ($c in @($s.comparison | Where-Object { $_.verdict -in 'higher', 'lower' })) { "  $($s.name) $($c.metric): $(Format-BudgetNumber $c.before) to $(Format-BudgetNumber $c.after)$(if ($c.unit) { " $($c.unit)" }), $($c.verdict): the rounds' means $(Format-BudgetNumber $c.mean_diff) apart, past +-$(Format-BudgetNumber $c.within)" } })
  ''
  if ($moved) { 'told apart (99%):'; $moved } else { 'nothing told apart (99%): every difference is within how each build''s rounds vary' }
  # What the rounds paired tell apart that Welch's doesn't: a lean the PC's swings hide, when both
  # builds' rounds swung together. Not a verdict (PITFALLS 113): run it again, or more rounds
  $leans = @(foreach ($s in $Report.scenarios) { foreach ($c in @($s.comparison | Where-Object { $_.verdict -eq 'same' -and $_.paired_verdict -in 'higher', 'lower' })) { "  $($s.name) $($c.metric): $(Format-BudgetNumber $c.before) to $(Format-BudgetNumber $c.after)$(if ($c.unit) { " $($c.unit)" }), $($c.paired_verdict) in $(if ($c.paired_verdict -eq 'higher') { $c.paired_later } else { $c.paired_n - $c.paired_later }) of $($c.paired_n) rounds: $(Format-BudgetNumber $c.paired_diff) apart on average, past +-$(Format-BudgetNumber $c.paired_within); Welch's +-$(Format-BudgetNumber $c.within) calls it the same" } })
  if ($leans) { ''; 'the rounds paired only (99%, not a verdict):'; $leans }
  # What no round measured, on either build: the table above leaves such a metric out, and its
  # budget is passed without being judged (a window others covered has no screen times and no
  # missed), so it's named here and counted in the last line, never read as a pass (PITFALLS 182)
  $unjudged = @(foreach ($s in $Report.scenarios) { foreach ($side in 'before', 'after') { $u = @($s.$side.unmeasured | Where-Object { $_ }); if ($u.Count) { "  $($s.name) $($side): $($u -join ', ')" } } })
  if ($unjudged) { ''; 'budgets nothing measured (not judged):'; $unjudged }
  $m = $Report.summary
  "after: $($m.pass) passed, $($m.fail) over budget, $(if ($m.busy) { "$($m.busy) over on a busy PC, " })$($m.error) couldn't be measured$(if ($unjudged) { ", $($unjudged.Count) with budgets nothing measured" }); $($m.higher) metrics higher and $($m.lower) lower, told apart at 99%, in $([Math]::Round($Report.seconds / 60, 1)) min"
}

# lbterm.ps1 budgets -Against: the build -Bin (after) against the build -Against (before), each
# scenario (-Only some) run -Rounds times on each, the order turned each round (before first in the
# odd rounds, after first in the even), so whatever the PC did over the run falls on both alike.
# Each build is judged on its rounds' spans or runs together, and the two held against each other
# by Compare-BudgetRounds. report.json in <Root>\<run>, the table at the end, and a history line for
# each build. Get-BudgetLastReport has the report, its summary the after build's.
function Invoke-BudgetComparison {
  param(
    [Parameter(Mandatory)][string]$Bin,
    [string]$Build,
    [Parameter(Mandatory)][string]$Against,
    [string]$AgainstBuild,
    [string[]]$Only,
    [int]$Runs,
    [double]$Seconds,
    [int]$Rounds = 5,
    [int]$Retry = 1,
    [int]$TimeoutMin = 10,
    [switch]$WaitIdle,
    [string]$Root = (Join-Path $script:Repo 'target\budgets'),
    [string]$File = $script:BudgetsFile,
    [string]$Script = $script:BudgetsScript,
    [string]$History = (Get-BudgetHistoryPath)
  )
  foreach ($dir in $Bin, $Against) { if (-not (Test-Path (Join-Path $dir 'lbterm-gui.exe'))) { throw "no build in $dir (lbterm-gui.exe)" } }
  $Bin = (Resolve-Path $Bin).Path
  $Against = (Resolve-Path $Against).Path
  if ($Bin -eq $Against) { throw "-Against is the build itself ($Bin): give the build to hold it against" }
  if ($Rounds -lt 2) { throw "-Rounds ${Rounds}: a comparison needs two rounds or more, so each build's own spread can be measured" }
  $b = Read-Budgets $File
  $scenarios = @(Select-BudgetScenarios $b.Scenarios $Only)
  $given = $Runs
  if (-not $Runs) { $Runs = $b.Runs }
  if ($given) { foreach ($sc in $scenarios) { $sc.Runs = $given } }
  $started = Get-Date
  $run = $started.ToString('yyyyMMdd-HHmmss') + '-vs'
  $runDir = Join-Path $Root $run
  New-Item -ItemType Directory -Force $runDir | Out-Null
  $commit = (git -C $script:Repo rev-parse --short=8 HEAD 2>$null)
  $bins = @{ before = $Against; after = $Bin }
  "budgets comparison $run`: $Bin ($Build) after, $Against ($AgainstBuild) before, $($scenarios.Count) scenarios, $Rounds rounds each, output in $runDir"
  $results = [Collections.Generic.List[object]]::new()
  $past = Read-BudgetHistory $History
  $judgedAll = @{ before = [Collections.Generic.List[object]]::new(); after = [Collections.Generic.List[object]]::new() }
  foreach ($sc in $scenarios) {
    $byRound = @{ before = [Collections.Generic.List[object]]::new(); after = [Collections.Generic.List[object]]::new() }
    $samples = @{ before = [Collections.Generic.List[object]]::new(); after = [Collections.Generic.List[object]]::new() }
    $errors = @{ before = [Collections.Generic.List[string]]::new(); after = [Collections.Generic.List[string]]::new() }
    for ($r = 1; $r -le $Rounds; $r++) {
      $order = if ($r % 2) { 'before', 'after' } else { 'after', 'before' }
      foreach ($side in $order) {
        $judged = $null
        $dir = $null
        for ($n = 0; $n -le $Retry; $n++) {
          $dir = Join-Path $runDir "$($sc.Name)\$r-$side$(if ($n) { ".$n" })"
          $argList = @('-Scenario', $sc.Name)
          if ($given -or $sc.Kind -ne 'action') { $argList += @('-Runs', $Runs) }
          if ($Seconds) { $argList += @('-Seconds', $Seconds) }
          $entry = [pscustomobject]@{ Name = $sc.Name; Script = 'budgets'; Path = $Script; Args = $argList }
          $a = Invoke-E2eAttempt -Entry $entry -Dir $dir -Bin $bins[$side] -TimeoutSec ($TimeoutMin * 60) -WaitIdle:$WaitIdle
          $judged = Read-BudgetAttempt -Scenario $sc -Attempt $a
          "  $($sc.Name) round $r $side$(if ($n) { " (again, $n)" }): $($judged.result) in $($a.Seconds) s$(if ($judged.pc) { ", others $([Math]::Round(100 * [double]$judged.pc.others))% of its CPUs, clock $(Format-BudgetNumber $judged.pc.ghz) GHz" })$(if ($judged.errors) { ': ' + $judged.errors[0] })"
          if ($judged.result -ne 'error') { break }
        }
        if ($judged.result -eq 'error') { $errors[$side].Add("round ${r}: $(@($judged.errors)[0])"); continue }
        # Which round, so Compare-BudgetRounds can pair each build's with the other's
        $judged.round = $r
        $byRound[$side].Add($judged)
        foreach ($x in (Read-BudgetSamples $dir)) { $samples[$side].Add($x) }
      }
    }
    # Each build judged on its rounds together: as many spans or runs as its rounds had
    $pooled = [pscustomobject]@{}
    foreach ($p in $sc.PSObject.Properties) { $pooled | Add-Member -NotePropertyName $p.Name -NotePropertyValue $p.Value }
    $pooled.Runs = [int]$(if ($given) { $given } elseif ($sc.Runs) { $sc.Runs } else { $Runs }) * $Rounds
    $all = @{}
    foreach ($side in 'before', 'after') {
      $all[$side] = Test-BudgetScenario -Scenario $pooled -Samples @($samples[$side]) -Errors @($(if (-not $byRound[$side].Count) { @($errors[$side]) })) -Notes @($errors[$side] | ForEach-Object { "couldn't be measured in $_" })
      $all[$side].rounds = $byRound[$side].Count
      $judgedAll[$side].Add($all[$side])
    }
    $all.after.drift = @(Get-BudgetDrift $all.after $past[$sc.Name])
    $results.Add([ordered]@{
        name = $sc.Name; kind = $sc.Kind; before = $all.before; after = $all.after
        comparison = @(Compare-BudgetRounds -Scenario $sc -Before @($byRound.before) -After @($byRound.after) -BeforeAll $all.before -AfterAll $all.after -Seconds $(if ($Seconds) { $Seconds } else { $sc.Seconds }))
      })
  }
  $script:LastBudgetReport = $null
  if (-not $results.Count) { 'nothing to run'; return }
  $after = New-BudgetReport -Run $run -Bin $Bin -Build $Build -Commit $commit -Started $started -Results @($judgedAll.after) -Runs $Runs -Seconds $(if ($Seconds) { $Seconds } else { $b.Seconds })
  $before = New-BudgetReport -Run $run -Bin $Against -Build $AgainstBuild -Commit $null -Started $started -Results @($judgedAll.before) -Runs $Runs -Seconds $(if ($Seconds) { $Seconds } else { $b.Seconds })
  $verdicts = @($results | ForEach-Object { $_.comparison } | ForEach-Object { $_.verdict })
  $summary = [ordered]@{}
  foreach ($k in $after.summary.Keys) { $summary[$k] = $after.summary[$k] }
  $summary.higher = @($verdicts | Where-Object { $_ -eq 'higher' }).Count
  $summary.lower = @($verdicts | Where-Object { $_ -eq 'lower' }).Count
  # Told apart by the rounds paired alone (shown, never a verdict: Compare-BudgetRounds)
  $summary.paired_only = @($results | ForEach-Object { $_.comparison } | Where-Object { $_.verdict -eq 'same' -and $_.paired_verdict -in 'higher', 'lower' }).Count
  $report = [ordered]@{
    run = $run; compare = $true; rounds = $Rounds; commit = $commit
    before = [ordered]@{ bin = $Against; build = $AgainstBuild }
    after = [ordered]@{ bin = $Bin; build = $Build }
    started = $started.ToString('o'); seconds = [Math]::Round(((Get-Date) - $started).TotalSeconds, 1)
    runs = $Runs; span_s = $(if ($Seconds) { $Seconds } else { $b.Seconds })
    summary = $summary
    scenarios = @($results)
  }
  $report | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $runDir 'report.json') -Encoding utf8NoBOM
  # Both builds in the history, the before first, so the after is each scenario's last
  Add-BudgetHistory $before $History
  Add-BudgetHistory $after $History
  $script:LastBudgetReport = $report
  ''
  Format-BudgetComparison $report
  "report: $(Join-Path $runDir 'report.json'); history: $History"
}

#endregion
