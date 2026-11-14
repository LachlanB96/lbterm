# Pester 5+, run by `lbterm.ps1 check`. harness.psm1's parts that need no window: the runner's
# Covers mapping, verdicts and report, the result format, geometry and pixel probes, and the
# state file's reading and waiting against fake files. Nothing here opens a window.

BeforeAll {
  Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
  $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  # The three tests that can't pass while the workstation is locked (LogonUI.exe) skip then, saying
  # so, as e2e stops on a lock (Lachlan's E2 of 2026-11-14: a check run on 2026-11-13 failed only
  # these while the PC was locked, and passed unchanged once it was unlocked)
  $lockedSkip = 'the workstation is locked (LogonUI.exe): this needs the desktop; rerun the check unlocked to cover it'

  # A test script with the header lines given, in $Dir
  function New-FakeScript([string]$Dir, [string]$Name, [string[]]$Header) {
    New-Item -ItemType Directory -Force $Dir | Out-Null
    $path = Join-Path $Dir $Name
    [IO.File]::WriteAllText($path, ((@('#Requires -Version 7') + $Header + @('<#', 'Help.', '#>', 'param([string]$Bin)', '# Covers: after-param/never.rs', '"ran"')) -join "`n"))
    $path
  }

  # A state file as the GUI writes it
  function Write-FakeState([string]$Path, [long]$Frame, [object[]]$Windows, [int]$ProcessId = 4242, [int]$Version = 1) {
    $s = [ordered]@{ version = $Version; pid = $ProcessId; frame = $Frame; windows = @($Windows) }
    [IO.File]::WriteAllText($Path, ($s | ConvertTo-Json -Depth 5))
  }

  function New-Bitmap([int]$W, [int]$H, [System.Drawing.Color]$Fill) {
    $b = [System.Drawing.Bitmap]::new($W, $H, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($b); $g.Clear($Fill); $g.Dispose()
    $b
  }
}

Describe 'Covers globs' {
  It 'takes * within a folder and ** across folders' {
    Test-Covers @('wezterm-gui/src/termwindow/render/strip*.rs') 'wezterm-gui/src/termwindow/render/strip_swap.rs' | Should -BeTrue
    Test-Covers @('wezterm-gui/src/*.rs') 'wezterm-gui/src/termwindow/render/strip.rs' | Should -BeFalse
    Test-Covers @('lbterm-panel/**') 'lbterm-panel/src/layout.rs' | Should -BeTrue
    Test-Covers @('wezterm-gui/**/strip.rs') 'wezterm-gui/src/termwindow/render/strip.rs' | Should -BeTrue
    Test-Covers @('wezterm-gui/**/strip.rs') 'wezterm-gui/strip.rs' | Should -BeTrue
  }

  It 'takes ? as one character, and a folder ending in / as everything under it' {
    Test-Covers @('mux/src/pane?.rs') 'mux/src/panes.rs' | Should -BeTrue
    Test-Covers @('mux/src/pane?.rs') 'mux/src/pane.rs' | Should -BeFalse
    Test-Covers @('lbterm-extras/') 'lbterm-extras/src/strip.rs' | Should -BeTrue
  }

  It 'reads either slash, and matches whole paths only' {
    Test-Covers @('lbterm-extras\src\strip.rs') 'lbterm-extras/src/strip.rs' | Should -BeTrue
    Test-Covers @('lbterm-extras/src/strip.rs') 'lbterm-extras\src\strip.rs' | Should -BeTrue
    Test-Covers @('src/strip.rs') 'lbterm-extras/src/strip.rs' | Should -BeFalse
    Test-Covers @('lbterm-extras/src/strip.rs') 'lbterm-extras/src/strip.rs.orig' | Should -BeFalse
  }

  It 'escapes what regexes would read' {
    Test-Covers @('a+b/(x).rs') 'a+b/(x).rs' | Should -BeTrue
    Test-Covers @('a.b/x.rs') 'aXb/x.rs' | Should -BeFalse
  }
}

Describe 'Script headers' {
  It 'reads Covers, Args, Variant and Helper lines before the param block' {
    $p = New-FakeScript (Join-Path $TestDrive 'h') 'one.ps1' @('# Covers: a/b.rs, c/**', '# Covers: d/*.rs e/f.rs', '# Args: -ClickMark 10', '# Variant: -Mux', '# Variant: -Off')
    $h = Get-ScriptHeader $p
    $h.Covers | Should -Be @('a/b.rs', 'c/**', 'd/*.rs', 'e/f.rs')
    $h.HasCovers | Should -BeTrue
    $h.Args | Should -Be '-ClickMark 10'
    $h.Variants | Should -Be @('-Mux', '-Off')
    $h.Helper | Should -BeFalse
  }

  It 'says when a script has no Covers line, and marks helpers' {
    $p = New-FakeScript (Join-Path $TestDrive 'h') 'two.ps1' @('# Helper: a stand-in')
    $h = Get-ScriptHeader $p
    $h.HasCovers | Should -BeFalse
    $h.Covers.Count | Should -Be 0
    $h.Helper | Should -BeTrue
  }

  It 'names variants after their arguments' {
    Get-VariantSuffix '-Mux' | Should -Be 'mux'
    Get-VariantSuffix '-Slowdown 2' | Should -Be 'slowdown-2'
    Get-VariantSuffix '-NoCompositor' | Should -Be 'nocompositor'
  }
}

Describe 'The runner''s entries' {
  BeforeAll {
    $dir = Join-Path $TestDrive 'perf'
    New-FakeScript $dir 'alpha.ps1' @('# Covers: lbterm-panel/**', '# Variant: -Mux') | Out-Null
    New-FakeScript $dir 'beta.ps1' @('# Covers: wezterm-gui/src/termwindow/render/strip.rs') | Out-Null
    New-FakeScript $dir 'gamma.ps1' @() | Out-Null
    New-FakeScript $dir 'fake-thing.ps1' @('# Helper: run by alpha') | Out-Null
    $entries = @(Get-E2eEntries $dir)
  }

  It 'has one per script and one per variant, and no helpers' {
    ($entries | ForEach-Object Name) | Should -Be @('alpha', 'alpha-mux', 'beta', 'gamma')
    ($entries | Where-Object Name -eq 'alpha-mux').Args | Should -Be @('-Mux')
    ($entries | Where-Object Name -eq 'alpha-mux').Script | Should -Be 'alpha'
  }

  It 'picks by name, by script (with its variants) and by wildcard, and refuses an unknown name' {
    (Select-E2eOnly $entries @('beta') | ForEach-Object Name) | Should -Be @('beta')
    (Select-E2eOnly $entries @('alpha') | ForEach-Object Name) | Should -Be @('alpha', 'alpha-mux')
    (Select-E2eOnly $entries @('alpha-mux,gamma') | ForEach-Object Name) | Should -Be @('alpha-mux', 'gamma')
    (Select-E2eOnly $entries @('g*') | ForEach-Object Name) | Should -Be @('gamma')
    { Select-E2eOnly $entries @('delta') } | Should -Throw '*delta*'
  }

  It 'picks what a change covers, and lists the scripts without a Covers line' {
    $pick = Select-E2eChanged -Entries $entries -Files @('lbterm-panel/src/layout.rs')
    ($pick.Selected | ForEach-Object Name) | Should -Be @('alpha', 'alpha-mux')
    ($pick.Uncovered | ForEach-Object Name) | Should -Be @('gamma')
    (Select-E2eChanged -Entries $entries -Files @('wezterm-gui\src\termwindow\render\strip.rs')).Selected.Name | Should -Be 'beta'
    (Select-E2eChanged -Entries $entries -Files @('README.md')).Selected.Count | Should -Be 0
  }

  It 'picks a script whose own file changed, Covers or not' {
    (Select-E2eChanged -Entries $entries -Files @('lbterm/perf/gamma.ps1')).Selected.Name | Should -Be 'gamma'
  }

  It 'picks everything when the harness changed' {
    (Select-E2eChanged -Entries $entries -Files @('lbterm/harness.psm1')).Selected.Count | Should -Be 4
    (Select-E2eChanged -Entries $entries -Files @('lbterm/motion/start-unfocused.ps1')).Selected.Count | Should -Be 4
  }

  It 'takes the smoke set by exact run name, and refuses a name no script runs' {
    $file = Join-Path $TestDrive 'smoke.json'
    '{ "scripts": [ { "name": "alpha", "why": "x" }, { "name": "beta", "why": "y" } ] }' | Set-Content $file
    (Get-E2eSmoke -Entries $entries -File $file | ForEach-Object Name) | Should -Be @('alpha', 'beta')
    '{ "scripts": [ { "name": "alpha-mux", "why": "x" } ] }' | Set-Content $file
    (Get-E2eSmoke -Entries $entries -File $file | ForEach-Object Name) | Should -Be @('alpha-mux')
    '{ "scripts": [ { "name": "delta", "why": "x" } ] }' | Set-Content $file
    { Get-E2eSmoke -Entries $entries -File $file } | Should -Throw '*delta*'
  }

  It 'pairs a look''s captures side by side, each at its own size under its label' {
    Add-Type -AssemblyName System.Drawing
    $mk = { param($path, $w, $h, $color)
      $bmp = [Drawing.Bitmap]::new($w, $h)
      $bmp.SetResolution(144, 144)
      $g = [Drawing.Graphics]::FromImage($bmp); $g.Clear($color); $g.Dispose()
      $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    }
    $before = Join-Path $TestDrive 'look\before'; $after = Join-Path $TestDrive 'look\after'
    New-Item -ItemType Directory -Force $before, $after | Out-Null
    & $mk (Join-Path $before 'menu.png') 40 30 ([Drawing.Color]::FromArgb(255, 255, 0, 0))
    & $mk (Join-Path $after 'menu.png') 50 20 ([Drawing.Color]::FromArgb(255, 0, 0, 255))
    & $mk (Join-Path $before 'only-before.png') 4 4 ([Drawing.Color]::Black)
    Get-LookCaptures $before $after | Should -Be @('menu.png')

    $out = New-LookPair -Before (Join-Path $before 'menu.png') -After (Join-Path $after 'menu.png') -Out (Join-Path $TestDrive 'look\pairs\menu.png') -Gap 10 -Band 20
    $pair = [Drawing.Bitmap]::FromFile($out)
    try {
      $pair.Width | Should -Be (40 + 10 + 50)
      $pair.Height | Should -Be (20 + 30)
      $pair.GetPixel(20, 35).ToArgb() | Should -Be ([Drawing.Color]::FromArgb(255, 255, 0, 0).ToArgb())
      $pair.GetPixel(39, 49).ToArgb() | Should -Be ([Drawing.Color]::FromArgb(255, 255, 0, 0).ToArgb())
      $pair.GetPixel(75, 30).ToArgb() | Should -Be ([Drawing.Color]::FromArgb(255, 0, 0, 255).ToArgb())
      $pair.GetPixel(99, 39).ToArgb() | Should -Be ([Drawing.Color]::FromArgb(255, 0, 0, 255).ToArgb())
      # Under the shorter one, the background
      $pair.GetPixel(75, 45).ToArgb() | Should -Be ([Drawing.Color]::FromArgb(255, 24, 24, 30).ToArgb())
    } finally { $pair.Dispose() }
  }

  It 'runs a look''s script on both builds and pairs what both saved' {
    # Invoke-LookRun refuses while the workstation is locked, by design (its captures would show nothing)
    if (Test-WorkstationLocked) { Set-ItResult -Skipped -Because $lockedSkip; return }
    $perf = Join-Path $TestDrive 'look-run\perf'
    New-Item -ItemType Directory -Force $perf | Out-Null
    # A stand-in script: saves card.png red on the build named before, blue on the other, and
    # fails its own checks on the before build, which judges nothing here
    [IO.File]::WriteAllText((Join-Path $perf 'card.ps1'), @'
#Requires -Version 7
# Covers: never.rs
param([string]$Bin)
Add-Type -AssemblyName System.Drawing
$bmp = [Drawing.Bitmap]::new(8, 6)
$g = [Drawing.Graphics]::FromImage($bmp)
$g.Clear($(if ((Split-Path $Bin -Leaf) -eq 'before') { [Drawing.Color]::Red } else { [Drawing.Color]::Blue }))
$g.Dispose()
$bmp.Save((Join-Path $env:LBTERM_E2E_OUT 'card.png'), [Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
if ((Split-Path $Bin -Leaf) -eq 'before') { 'FAIL the card was red'; exit 1 }
'passed'
'@)
    $builds = foreach ($n in 'before', 'after') {
      $d = Join-Path $TestDrive "look-run\$n"
      New-Item -ItemType Directory -Force $d | Out-Null
      New-Item -ItemType File (Join-Path $d 'lbterm-gui.exe') | Out-Null
      $d
    }
    $root = Join-Path $TestDrive 'look-run\target'
    $said = @(Invoke-LookRun -Bin $builds[1] -Against $builds[0] -Only card -Root $root -PerfDir $perf -BeforeLabel 'before: x' -AfterLabel 'after: y')
    ($said -join "`n") | Should -Match 'card, before: its checks failed \(the card was red\)'
    ($said -join "`n") | Should -Match 'card, after: its checks passed'
    ($said -join "`n") | Should -Match '1 pairs\. Show them to Lachlan:'
    $pairFile = Get-ChildItem $root -Recurse -Filter 'card-card.png' | Select-Object -First 1
    $pairFile | Should -Not -BeNullOrEmpty
    ($said[-1]) | Should -Match ('show-image ".*card-card\.png"')
    Add-Type -AssemblyName System.Drawing
    $pair = [Drawing.Bitmap]::FromFile($pairFile.FullName)
    try {
      $pair.GetPixel(2, 30).ToArgb() | Should -Be ([Drawing.Color]::Red.ToArgb())
      $pair.GetPixel(8 + 16 + 2, 30).ToArgb() | Should -Be ([Drawing.Color]::Blue.ToArgb())
    } finally { $pair.Dispose() }
    { Invoke-LookRun -Bin $builds[1] -Against (Join-Path $TestDrive 'nowhere') -Only card -Root $root -PerfDir $perf } | Should -Throw '*no build in*'
  }

  It 'picks the scripts the branch changed, with their variants, and never for the harness' {
    (Select-E2eOwn -Entries $entries -Files @('lbterm/perf/alpha.ps1', 'lbterm-panel/src/layout.rs') | ForEach-Object Name) | Should -Be @('alpha', 'alpha-mux')
    (Select-E2eOwn -Entries $entries -Files @('lbterm\perf\gamma.ps1') | ForEach-Object Name) | Should -Be @('gamma')
    @(Select-E2eOwn -Entries $entries -Files @('lbterm/harness.psm1')).Count | Should -Be 0
  }
}

Describe 'LBTerm''s own test scripts' {
  BeforeAll {
    $real = @(Get-E2eEntries (Join-Path $repo 'lbterm\perf'))
    $tracked = @(git -C $repo ls-files)
  }

  It 'the smoke set names runs that exist, each with why' {
    $file = Join-Path $repo 'lbterm\perf\smoke.json'
    @(Get-E2eSmoke -Entries $real -File $file).Count | Should -Be @((Get-Content $file -Raw | ConvertFrom-Json).scripts).Count
    @((Get-Content $file -Raw | ConvertFrom-Json).scripts | Where-Object { -not $_.why }) | Should -BeNullOrEmpty
  }

  It 'each is on the harness' {
    @($real | Where-Object { (Get-Content $_.Path -Raw) -notmatch 'harness\.psm1' } | ForEach-Object Name) | Should -BeNullOrEmpty
  }

  It 'each on the harness has a Covers line' {
    $onHarness = @($real | Where-Object { (Get-Content $_.Path -Raw) -match 'harness\.psm1' })
    @($onHarness | Where-Object { -not $_.Header.HasCovers } | ForEach-Object Name) | Should -BeNullOrEmpty
  }

  It 'covers files that exist: every glob matches one' {
    $dead = foreach ($e in ($real | Where-Object { $_.Name -eq $_.Script })) {
      foreach ($g in $e.Header.Covers) {
        $rx = ConvertTo-GlobRegex $g
        if (-not ($tracked | Where-Object { $_ -match $rx } | Select-Object -First 1)) { "$($e.Name): $g" }
      }
    }
    @($dead) | Should -BeNullOrEmpty
  }
}

Describe 'The result format' {
  It 'passes with no failures' {
    $r = ConvertTo-TestResult -Name 'x' -Failures @() -Seconds 12.345 -Captures @('a.png')
    $r.pass | Should -BeTrue
    $r.seconds | Should -Be 12.3
    $r.failures.Count | Should -Be 0
    $r.captures | Should -Be @('a.png')
  }

  It 'says why a test judged nothing, only when it didn''t fail' {
    $r = ConvertTo-TestResult -Name 'x' -Seconds 1 -Unjudged 'its window was covered'
    $r.pass | Should -BeTrue
    $r.unjudged | Should -Be 'its window was covered'
    (ConvertTo-TestResult -Name 'x' -Failures @('broke') -Seconds 1 -Unjudged 'covered').Contains('unjudged') | Should -BeFalse
    (ConvertTo-TestResult -Name 'x' -Seconds 1).Contains('unjudged') | Should -BeFalse
  }

  It 'fails with one, and keeps every key in its JSON' {
    $r = ConvertTo-TestResult -Name 'x' -Failures @('it broke', '') -Seconds 1
    $r.pass | Should -BeFalse
    $r.failures | Should -Be @('it broke')
    $path = Join-Path $TestDrive 'result.json'
    Write-TestResult $r $path
    $back = Get-Content $path -Raw | ConvertFrom-Json
    $back.PSObject.Properties.Name | Should -Be @('name', 'pass', 'failures', 'seconds', 'captures')
    $back.name | Should -Be 'x'
    @($back.failures) | Should -Be @('it broke')
  }
}

Describe 'Invoke-Test' {
  BeforeAll {
    # A build that's only its name, so Start-Test takes it; nothing here opens a window
    $bin = Join-Path $TestDrive 'bin'
    New-Item -ItemType Directory -Force $bin | Out-Null
    Set-Content (Join-Path $bin 'lbterm-gui.exe') ''
    $module = Join-Path $PSScriptRoot '..\harness.psm1'
    function Invoke-FakeTest([string]$Name, [string]$Body) {
      $out = Join-Path $TestDrive "out-$Name"
      $script = Join-Path $TestDrive "$Name.ps1"
      [IO.File]::WriteAllText($script, @"
`$ErrorActionPreference = 'Stop'
Import-Module '$module' -Force -DisableNameChecking
`$T = Start-Test '$Name' -Bin '$bin' -Out '$out'
Invoke-Test {
$Body
} -Passed 'all well'
"@)
      $printed = & 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File $script
      [pscustomobject]@{ Exit = $LASTEXITCODE; Printed = @($printed); Result = (Get-Content (Join-Path $out 'result.json') -Raw | ConvertFrom-Json) }
    }
  }

  It 'passes a body without failures: exit 0, the -Passed line, result.json saying so' {
    $r = Invoke-FakeTest 'fine' "'step one'"
    $r.Exit | Should -Be 0
    $r.Printed | Should -Contain 'step one'
    $r.Printed[-1] | Should -Be 'all well'
    $r.Result.pass | Should -BeTrue
    $r.Result.name | Should -Be 'fine'
  }

  It 'fails with what Add-Failure kept, carrying on past it' {
    $r = Invoke-FakeTest 'kept' "Add-Failure 'the pad never lit'`n'after'"
    $r.Exit | Should -Be 1
    $r.Printed | Should -Contain 'after'
    $r.Printed | Should -Contain 'FAIL the pad never lit'
    @($r.Result.failures) | Should -Be @('the pad never lit')
  }

  It 'fails on a throw, naming the test script''s line even when the library threw it' {
    $r = Invoke-FakeTest 'threw' "'before'`nGet-Shot ([IntPtr]::Zero)`n'not reached'"
    $r.Exit | Should -Be 1
    $r.Printed | Should -Not -Contain 'not reached'
    $r.Result.failures[0] | Should -Match '^no window to capture .*\(threw\.ps1:6\)$'
  }

  It "refuses a config that doesn't set wsl_domains, before opening anything (wsl.exe's stub opens WSL's installer)" {
    # Open-TestWindow wakes the displays first, and its mouse move of no pixels is refused while locked
    if (Test-WorkstationLocked) { Set-ItResult -Skipped -Because $lockedSkip; return }
    $cfg = Join-Path $TestDrive 'nowsl.lua'
    Set-Content $cfg 'return { lbterm_crash_restore = false }'
    $r = Invoke-FakeTest 'nowsl' "Open-TestWindow -ConfigFile '$cfg'`n'not reached'"
    $r.Exit | Should -Be 1
    $r.Printed | Should -Not -Contain 'not reached'
    $r.Result.failures[0] | Should -Match "doesn't set wsl_domains = \{\}"
    Test-Path (Join-Path $TestDrive 'out-nowsl\pids.txt') | Should -BeFalse -Because 'nothing was started'
  }
}

Describe 'The memory guard' {
  BeforeAll {
    # A process of no window that waits, holding a pwsh's few tens of MB
    function Start-Holder {
      $si = [Diagnostics.ProcessStartInfo]::new('C:\Program Files\PowerShell\7\pwsh.exe', '-NoProfile -NonInteractive -Command Start-Sleep 60')
      $si.UseShellExecute = $false
      $si.CreateNoWindow = $true
      [Diagnostics.Process]::Start($si)
    }
  }

  It 'kills what it watches once one holds more than its limit, and says what and when' {
    $p = Start-Holder
    try {
      Start-MemoryGuard -Pids $p.Id -LimitGB 0.01 -PeriodMs 50
      $clock = [Diagnostics.Stopwatch]::StartNew()
      while (-not (Get-MemoryGuard) -and $clock.Elapsed.TotalSeconds -lt 10) { Start-Sleep -Milliseconds 50 }
      Get-MemoryGuard | Should -Match "^pwsh \(pid $($p.Id)\) held \d+ MB private, \d+ MB working set, [\d.]+ s after the guard started, over its 0\.0 GB: stopped$"
      $p.WaitForExit(5000) | Should -BeTrue -Because 'the guard killed it'
      Stop-MemoryGuard | Should -Be (Get-MemoryGuard) -Because 'stopping it says what it tripped on'
    } finally { if (-not $p.HasExited) { $p.Kill() } }
  }

  It 'leaves alone what stays under its limit, and stopping it says nothing' {
    $p = Start-Holder
    try {
      Start-MemoryGuard -Pids $p.Id, $p.Id -LimitGB 8 -PeriodMs 20
      Start-Sleep -Milliseconds 400
      Get-MemoryGuard | Should -BeNullOrEmpty
      Stop-MemoryGuard | Should -BeNullOrEmpty
      $p.HasExited | Should -BeFalse
    } finally { if (-not $p.HasExited) { $p.Kill() } }
  }

  It 'fails a test it stopped, whether the body saw it or not' {
    $bin = Join-Path $TestDrive 'guard-bin'
    New-Item -ItemType Directory -Force $bin | Out-Null
    Set-Content (Join-Path $bin 'lbterm-gui.exe') ''
    $out = Join-Path $TestDrive 'out-guard'
    $script = Join-Path $TestDrive 'guard.ps1'
    [IO.File]::WriteAllText($script, @"
`$ErrorActionPreference = 'Stop'
Import-Module '$(Join-Path $PSScriptRoot '..\harness.psm1')' -Force -DisableNameChecking
`$T = Start-Test 'guard' -Bin '$bin' -Out '$out'
Invoke-Test {
  `$si = [Diagnostics.ProcessStartInfo]::new('C:\Program Files\PowerShell\7\pwsh.exe', '-NoProfile -NonInteractive -Command Start-Sleep 60')
  `$si.UseShellExecute = `$false
  `$si.CreateNoWindow = `$true
  `$p = [Diagnostics.Process]::Start(`$si)
  Start-MemoryGuard -Pids `$p.Id -LimitGB 0.01 -PeriodMs 20
  `$p.WaitForExit(10000) | Out-Null
  'after'
} -NoWindowCheck -Passed 'all well'
"@)
    $printed = @(& 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File $script)
    $LASTEXITCODE | Should -Be 1
    $printed | Should -Contain 'after'
    $result = Get-Content (Join-Path $out 'result.json') -Raw | ConvertFrom-Json
    @($result.failures)[0] | Should -Match '^the memory guard stopped the test: pwsh \(pid \d+\) held'
  }
}

Describe 'A test window on its screen' {
  BeforeAll {
    Add-Type -AssemblyName System.Drawing
    function Rect([int]$Left, [int]$Top, [int]$Right, [int]$Bottom) { [pscustomobject]@{ Left = $Left; Top = $Top; Right = $Right; Bottom = $Bottom } }
    # The right-most portrait screen of 2026-11-10: x 4920 to 6000, y -177 to 1743
    $screen = [System.Drawing.Rectangle]::new(4920, -177, 1080, 1920)
  }
  It 'leaves a window that is all on its screen where it is' {
    $to = Get-OnScreenRect -Window (Rect 4940 -57 5940 649) -Screen $screen
    $to.On | Should -BeTrue
    $to.Resized | Should -BeFalse
    @($to.X, $to.Y, $to.Width, $to.Height) | Should -Be @(4940, -57, 1000, 706)
  }
  It 'narrows a window wider than its screen, as budgets.ps1''s 110 columns were, and keeps it 20 px in' {
    $to = Get-OnScreenRect -Window (Rect 4940 -57 6070 649) -Screen $screen
    $to.On | Should -BeFalse
    $to.Off.Right | Should -Be 70
    $to.Off.Left | Should -Be 0
    @($to.X, $to.Y, $to.Width, $to.Height) | Should -Be @(4940, -57, 1040, 706)
    $to.Resized | Should -BeTrue
  }
  It 'moves a window that fits back in, its size kept' {
    $to = Get-OnScreenRect -Window (Rect 5100 1500 6050 1900) -Screen $screen
    @($to.X, $to.Y, $to.Width, $to.Height) | Should -Be @(5030, 1323, 950, 400)
    $to.Resized | Should -BeFalse
    $to.Off.Right | Should -Be 50
    $to.Off.Bottom | Should -Be 157
  }
}

Describe 'Mux sockets' {
  It "gives each test's output folder a socket short enough for Windows' AF_UNIX, and its own" {
    $long = 'C:\Users\someone\src\terminal\code\lbterm\.claude\worktrees\slot-3\target\perf\spawn-no-activate-old-server'
    $sock = Get-TestSocket $long
    $sock.Length | Should -BeLessThan 108
    Get-TestSocket $long | Should -Be $sock
    # Another checkout's run of the same test
    Get-TestSocket ($long -replace 'slot-3', 'slot-1') | Should -Not -Be $sock
    # The same folder named in another case
    Get-TestSocket $long.ToUpperInvariant() | Should -Be $sock
  }
}

Describe 'The daily socket' {
  It 'tells a socket made again under the same name from the one before' {
    $dir = Join-Path ([IO.Path]::GetTempPath()) "lbterm-socket-stamp-$PID"
    New-Item -ItemType Directory -Force $dir | Out-Null
    $path = Join-Path $dir 'sock'
    function New-Listener {
      $s = [Net.Sockets.Socket]::new([Net.Sockets.AddressFamily]::Unix, [Net.Sockets.SocketType]::Stream, [Net.Sockets.ProtocolType]::Unspecified)
      $s.Bind([Net.Sockets.UnixDomainSocketEndPoint]::new($path))
      $s
    }
    try {
      Get-SocketStamp $path | Should -BeNullOrEmpty
      $first = New-Listener
      $stamp = Get-SocketStamp $path
      $stamp | Should -Not -BeNullOrEmpty
      Get-SocketStamp $path | Should -Be $stamp
      # What a server taking the path does: the file removed, another bound in its place (.NET
      # removes it itself as the socket goes)
      $first.Dispose()
      Remove-Item -LiteralPath $path -ErrorAction SilentlyContinue
      Start-Sleep -Milliseconds 20
      $second = New-Listener
      Get-SocketStamp $path | Should -Not -Be $stamp
      $second.Dispose()
    }
    finally { Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue }
  }
}

Describe 'Test windows and WSL' {
  It 'every script that writes a config of its own sets wsl_domains = {} in it' {
    # start-unfocused.ps1 is the launcher: its --config-file is a usage example in its header
    $scripts = Get-ChildItem (Join-Path $repo 'lbterm\perf'), (Join-Path $repo 'lbterm\motion') -Filter *.ps1 |
      Where-Object Name -ne 'start-unfocused.ps1'
    $own = @($scripts | Where-Object { (Get-Content $_.FullName -Raw) -match '-ConfigFile|serve_command|--config-file' })
    $own.Count | Should -BeGreaterThan 10
    @($own | Where-Object {
        $text = Get-Content $_.FullName -Raw
        $text -notmatch 'wsl_domains\s*=\s*\{\}' -and $text -notmatch 'New-TestConfig'
      } | ForEach-Object Name) | Should -BeNullOrEmpty
  }

  It "New-TestConfig leaves out what shows or acts on Lachlan's own files: the Conductor, the InMail and agenda buttons, the links, the picker's recent picks" {
    $cfg = Join-Path $TestDrive 'lookalike.lua'
    New-TestConfig -Path $cfg
    $text = Get-Content -Raw $cfg
    foreach ($key in 'lbterm_conductor', 'lbterm_inbox', 'lbterm_agenda', 'lbterm_close_marks', 'lbterm_links') {
      $text | Should -Match ([regex]::Escape("rawset(config, '$key', nil)"))
    }
    $text | Should -Match ([regex]::Escape("config.lbterm_projects.recent_file = nil"))
  }

  It 'New-TestConfig and -NoConfig say no WSL domains' {
    (Get-Content (Join-Path $repo 'lbterm\motion\offscreen.ps1') -Raw) | Should -Match 'config\.wsl_domains = \{\}'
    (Get-Content (Join-Path $repo 'lbterm\harness.psm1') -Raw) | Should -Match ([regex]::Escape("'-n --config lbterm_crash_restore=false --config wsl_domains={}'"))
  }
}

Describe 'Reading an attempt' {
  It 'takes result.json when the script wrote one' {
    $d = Join-Path $TestDrive 'a1'; New-Item -ItemType Directory $d | Out-Null
    Write-TestResult (ConvertTo-TestResult -Name 'a' -Failures @('no ring') -Seconds 3 -Captures @('x.png')) (Join-Path $d 'result.json')
    $a = Read-E2eAttempt -Dir $d -Exit 1 -Seconds 3.04 -TimedOut $false
    $a.Pass | Should -BeFalse
    $a.Failures | Should -Be @('no ring')
    $a.Captures | Should -Be @('x.png')
  }

  It 'fails a script that exits 0 but wrote a failing result, and passes one that wrote a passing result' {
    $d = Join-Path $TestDrive 'a2'; New-Item -ItemType Directory $d | Out-Null
    Write-TestResult (ConvertTo-TestResult -Name 'a' -Failures @('odd') -Seconds 1) (Join-Path $d 'result.json')
    (Read-E2eAttempt -Dir $d -Exit 0 -Seconds 1 -TimedOut $false).Pass | Should -BeFalse
    Write-TestResult (ConvertTo-TestResult -Name 'a' -Seconds 1) (Join-Path $d 'result.json')
    (Read-E2eAttempt -Dir $d -Exit 0 -Seconds 1 -TimedOut $false).Pass | Should -BeTrue
    (Read-E2eAttempt -Dir $d -Exit 0 -Seconds 1 -TimedOut $false).Unjudged | Should -BeNullOrEmpty
    Write-TestResult (ConvertTo-TestResult -Name 'a' -Seconds 1 -Unjudged 'covered') (Join-Path $d 'result.json')
    $u = Read-E2eAttempt -Dir $d -Exit 0 -Seconds 1 -TimedOut $false
    $u.Pass | Should -BeTrue
    $u.Unjudged | Should -Be 'covered'
  }

  It 'reads FAIL lines from the output of a script without a result' {
    $d = Join-Path $TestDrive 'a3'; New-Item -ItemType Directory $d | Out-Null
    Set-Content (Join-Path $d 'output.txt') @('step one', 'FAIL: the pad never lit', 'FAIL the ring is gone')
    $a = Read-E2eAttempt -Dir $d -Exit 1 -Seconds 2 -TimedOut $false
    $a.Failures | Should -Be @('the pad never lit', 'the ring is gone')
  }

  It 'says what a script that threw printed, and that one timed out' {
    $d = Join-Path $TestDrive 'a4'; New-Item -ItemType Directory $d | Out-Null
    Set-Content (Join-Path $d 'output.txt') @('Exception: no window from lbterm-gui.exe')
    (Read-E2eAttempt -Dir $d -Exit 1 -Seconds 2 -TimedOut $false).Failures[0] | Should -Match 'exit 1: Exception: no window'
    $t = Read-E2eAttempt -Dir $d -Exit -1 -Seconds 600 -TimedOut $true
    $t.Pass | Should -BeFalse
    $t.Failures[-1] | Should -Be 'timed out after 600 s'
  }
}

Describe 'An attempt''s folder kept for the next' {
  It 'renames it once a process of the last attempt lets go of a file in it' {
    $d = Join-Path $TestDrive 'held'; New-Item -ItemType Directory $d | Out-Null
    $f = [IO.File]::Open((Join-Path $d 'log.txt'), 'Create', 'ReadWrite', 'None')
    # Let go of on a thread of its own in 700 ms, as a process on its way out does
    Add-Type -ErrorAction SilentlyContinue -TypeDefinition 'public static class LbtermTestLater { public static void Dispose(System.IDisposable d, int ms) { System.Threading.Tasks.Task.Delay(ms).ContinueWith(_ => d.Dispose()); } }'
    [LbtermTestLater]::Dispose($f, 700)
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Rename-AttemptFolder $d 'held.1' -TimeoutSec 10
    $clock.Elapsed.TotalMilliseconds | Should -BeGreaterThan 500
    Test-Path (Join-Path $TestDrive 'held.1') | Should -BeTrue
    Test-Path $d | Should -BeFalse
  }

  It 'says so when the folder stays held' {
    $d = Join-Path $TestDrive 'kept'; New-Item -ItemType Directory $d | Out-Null
    $f = [IO.File]::Open((Join-Path $d 'log.txt'), 'Create', 'ReadWrite', 'None')
    try { { Rename-AttemptFolder $d 'kept.1' -TimeoutSec 0.6 } | Should -Throw '*still held after 0.6 s*' }
    finally { $f.Dispose() }
  }
}

Describe 'Verdicts and the report' {
  BeforeAll {
    $ok = [pscustomobject]@{ Pass = $true; Exit = 0; Seconds = 10; TimedOut = $false; Dir = 'd'; Failures = @(); Captures = @() }
    $bad = [pscustomobject]@{ Pass = $false; Exit = 1; Seconds = 12; TimedOut = $false; Dir = 'd.1'; Failures = @('broke'); Captures = @() }
  }

  It 'passes a first-time pass, calls a pass on the rerun flaky, and fails the rest' {
    Get-E2eVerdict @($ok) | Should -Be 'pass'
    Get-E2eVerdict @($bad, $ok) | Should -Be 'flaky'
    Get-E2eVerdict @($bad, $bad) | Should -Be 'fail'
    Get-E2eVerdict @() | Should -Be 'fail'
    $none = [pscustomobject]@{ Pass = $true; Unjudged = 'covered'; Exit = 0; Seconds = 10; TimedOut = $false; Dir = 'd'; Failures = @(); Captures = @() }
    Get-E2eVerdict @($none) | Should -Be 'unjudged'
  }

  It 'counts a run that judged nothing apart from the passes, and says why in its row' {
    $none = [pscustomobject]@{ Pass = $true; Unjudged = 'covered'; Exit = 0; Seconds = 10; TimedOut = $false; Dir = 'd'; Failures = @(); Captures = @() }
    $results = @(
      [pscustomobject]@{ Name = 'a'; Script = 'a'; Args = @(); Attempts = @($ok); Result = 'pass' }
      [pscustomobject]@{ Name = 'p'; Script = 'p'; Args = @(); Attempts = @($none); Result = 'unjudged' }
    )
    $r = New-E2eReport -Run 'r' -Bin 'b' -Commit 'abc' -Started (Get-Date).AddSeconds(-30) -Results $results
    $r.summary.pass | Should -Be 1
    $r.summary.unjudged | Should -Be 1
    ($r.scripts | Where-Object name -eq 'p').unjudged | Should -Be 'covered'
    ($r.scripts | Where-Object name -eq 'a').unjudged | Should -BeNullOrEmpty
    $table = @(Format-E2eTable $r)
    $table[2] | Should -Match '^p\s+unjudged\s+10\.0\s+covered$'
    $table[-1] | Should -Match '^1 passed, 1 unjudged \(judged nothing: not passes\), 0 flaky, 0 failed'
  }

  It 'counts each result, keeps the first failure of a flaky one, and prints a row each' {
    $results = @(
      [pscustomobject]@{ Name = 'a'; Script = 'a'; Args = @(); Attempts = @($ok); Result = 'pass' }
      [pscustomobject]@{ Name = 'b'; Script = 'b'; Args = @(); Attempts = @($bad, $ok); Result = 'flaky' }
      [pscustomobject]@{ Name = 'c-mux'; Script = 'c'; Args = @('-Mux'); Attempts = @($bad, $bad); Result = 'fail' }
    )
    $r = New-E2eReport -Run 'r' -Bin 'b' -Commit 'abc' -Started (Get-Date).AddSeconds(-90) -Results $results
    $r.summary.pass | Should -Be 1
    $r.summary.flaky | Should -Be 1
    $r.summary.fail | Should -Be 1
    ($r.scripts | Where-Object name -eq 'b').flaky | Should -BeTrue
    ($r.scripts | Where-Object name -eq 'b').failures | Should -Be @('broke')
    ($r.scripts | Where-Object name -eq 'c-mux').attempts.Count | Should -Be 2
    $table = @(Format-E2eTable $r)
    $table.Count | Should -Be 5
    $table[0] | Should -Match '^script\s+result\s+seconds\s+flaky$'
    $table[2] | Should -Match '^b\s+flaky\s+10\.0\s+flaky$'
    $table[-1] | Should -Match '^1 passed, 1 flaky, 1 failed'
  }

  It 'stops at a locked workstation before a script runs, and the report says where' {
    Mock -ModuleName harness Test-WorkstationLocked { $true }
    $perf = Join-Path $TestDrive 'perf-locked'
    $bin = Join-Path $TestDrive 'bin-locked'
    New-Item -ItemType Directory $perf, $bin | Out-Null
    Set-Content (Join-Path $perf 'a.ps1') "# Covers: x`nexit 0"
    Set-Content (Join-Path $bin 'lbterm-gui.exe') ''
    $root = Join-Path $TestDrive 'e2e-locked'
    $out = @(Invoke-E2eRun -Bin $bin -Root $root -PerfDir $perf)
    @($out -match '^STOPPED: the workstation was locked \(LogonUI\.exe\) before a$').Count | Should -Be 1
    $report = Get-ChildItem $root -Recurse -Filter report.json | Get-Content -Raw | ConvertFrom-Json
    $report.stopped | Should -Match 'before a$'
    @($report.scripts).Count | Should -Be 0
  }

  It 'stops when the displays are asleep, before a script runs, and the report says why' {
    Mock -ModuleName harness Test-WorkstationLocked { $false }
    Mock -ModuleName harness Get-DisplaysAsleep { 'the displays are asleep: no input for 4000 s, and the power plan turns them off after 3600 s' }
    $perf = Join-Path $TestDrive 'perf-asleep'
    $bin = Join-Path $TestDrive 'bin-asleep'
    New-Item -ItemType Directory $perf, $bin | Out-Null
    Set-Content (Join-Path $perf 'a.ps1') "# Covers: x`nexit 0"
    Set-Content (Join-Path $bin 'lbterm-gui.exe') ''
    $root = Join-Path $TestDrive 'e2e-asleep'
    $out = @(Invoke-E2eRun -Bin $bin -Root $root -PerfDir $perf)
    @($out -match '^STOPPED: the displays are asleep: .*, before a$').Count | Should -Be 1
    $report = Get-ChildItem $root -Recurse -Filter report.json | Get-Content -Raw | ConvertFrom-Json
    $report.stopped | Should -Match '^the displays are asleep'
    @($report.scripts).Count | Should -Be 0
  }

  It 'reads the display-off time from powercfg, for the words' {
    ConvertFrom-PowercfgIndex "    Current AC Power Setting Index: 0x00000e10`n    Current DC Power Setting Index: 0x0000012c" | Should -Be 3600
    ConvertFrom-PowercfgIndex 'Current AC Power Setting Index: 0x00000000' | Should -Be 0
    ConvertFrom-PowercfgIndex 'nothing like it' | Should -Be 0
  }

  It 'calls the displays asleep only when Windows says they are off, whatever the idle time' {
    Get-DisplaysAsleep -Power off | Should -Match '^the displays are asleep: Windows says they''re off \(no input for \d+ s; the power plan turns them off after \d+ s, and a mouse move of no pixels didn''t bring them back\)$'
    Get-DisplaysAsleep -Power on | Should -BeNullOrEmpty
    # Dimmed is on: DWM still composes and presents
    Get-DisplaysAsleep -Power dimmed | Should -BeNullOrEmpty
  }

  It 'sends no input to displays that are on, and a mouse move of no pixels only to ones that are off (2a)' {
    Mock -ModuleName harness Send-NoMove { }
    # On: the hold alone, nothing said, no input
    Mock -ModuleName harness Get-DisplayPower { 'on' }
    Request-DisplaysAwake | Should -BeNullOrEmpty
    Should -Invoke -ModuleName harness Send-NoMove -Times 0 -Exactly
    # Off, then on after the move: one move, and the run says it woke them
    $script:reads = 0
    Mock -ModuleName harness Get-DisplayPower { if ($script:reads++ -lt 1) { 'off' } else { 'on' } }
    Request-DisplaysAwake | Should -Match '^the displays were off: a mouse move of no pixels turned them back on in [\d.]+ s$'
    Should -Invoke -ModuleName harness Send-NoMove -Times 1 -Exactly
    # Off for good: one move, then the words a run refuses with
    Mock -ModuleName harness Get-DisplayPower { 'off' }
    Request-DisplaysAwake -WaitSec 0 | Should -Be 'the displays stayed off for 0 s after a mouse move of no pixels'
    Should -Invoke -ModuleName harness Send-NoMove -Times 2 -Exactly
  }

  It 'reads the displays'' power from Windows, and holds them on for the process (E4)' {
    # Windows gives no display state to a locked session
    if (Test-WorkstationLocked) { Set-ItResult -Skipped -Because $lockedSkip; return }
    # This PC's displays are on while the check runs, which is when anyone is there to run it
    Get-DisplayPower | Should -BeIn @('on', 'dimmed', 'off')
    # In a process of its own, so the hold ends with it and the test's process keeps none
    $m = (Get-Module harness).Path
    $out = pwsh -NoProfile -Command "Import-Module '$m' -DisableNameChecking; Use-DisplaysType; `$h = [LbtermE2E.Displays]::Holding; `$null = Request-DisplaysAwake; `$null = Request-DisplaysAwake; '{0} {1} {2}' -f `$h, [LbtermE2E.Displays]::SystemState(), [LbtermE2E.Displays]::Holding"
    $holdingBefore, $after, $holding = "$out".Trim() -split ' '
    $holdingBefore | Should -Be 'False'
    # ES_DISPLAY_REQUIRED (2) is in force once the process holds it, a second Hold changing nothing
    ([uint32]$after -band 2) | Should -Be 2
    $holding | Should -Be 'True'
  }

  It 'puts where a run stopped in the table' {
    $r = New-E2eReport -Run 'r' -Bin 'b' -Commit 'abc' -Started (Get-Date) -Results @([pscustomobject]@{ Name = 'a'; Script = 'a'; Args = @(); Attempts = @($ok); Result = 'pass' }) -Stopped 'the workstation was locked (LogonUI.exe) before b'
    $table = @(Format-E2eTable $r)
    $table[-2] | Should -Be 'STOPPED: the workstation was locked (LogonUI.exe) before b'
    (New-E2eReport -Run 'r' -Started (Get-Date) -Results @()).stopped | Should -BeNullOrEmpty
  }
}

Describe 'Geometry and messages' {
  It 'packs a point into a mouse message''s lParam, negatives as 16-bit two''s complement' {
    [LbtermE2E.Win]::LParam(10, 20).ToInt64() | Should -Be 0x0014000A
    ([LbtermE2E.Win]::LParam(-1, 5).ToInt64() -band 0xFFFFFFFF) | Should -Be 0x0005FFFF
    # y past 32767 comes out negative as a signed 32-bit lParam, as GET_Y_LPARAM reads it
    ([LbtermE2E.Win]::LParam(0, 0xFFFF).ToInt64() -band 0xFFFFFFFF) | Should -Be 0xFFFF0000
  }

  It 'builds a side button''s and a click''s messages as Send-SideButton and Send-Click post them' {
    $fwd = @(Get-SideButtonMessages 10 20 -Forward)
    $fwd.Count | Should -Be 2
    $fwd[0].Msg | Should -Be 0x020B
    $fwd[0].W | Should -Be ((2 -shl 16) -bor 0x40)
    $fwd[1].Msg | Should -Be 0x020C
    $fwd[1].W | Should -Be (2 -shl 16)
    $fwd[0].L | Should -Be 0x0014000A
    (@(Get-SideButtonMessages 10 20)[0].W) | Should -Be ((1 -shl 16) -bor 0x20)
    $click = @(Get-ClickMessages 10 20 -Leave)
    @($click | ForEach-Object Msg) | Should -Be @(0x0200, 0x0201, 0x0202, 0x02A3)
    @($click | ForEach-Object W) | Should -Be @(0, 1, 0, 0)
    @(Get-ClickMessages 10 20).Count | Should -Be 3
  }

  It 'posts each set on a clock of its own, in order and never early, whatever the script''s thread does' {
    # To the poster's own thread (no window): what's checked is when they went
    $posts = @(0, 40, 80, 120) | ForEach-Object { @{ AtMs = $_; Messages = @(Get-SideButtonMessages 1 1 -Forward) } }
    $posts += @{ AtMs = 160; Messages = @(Get-ClickMessages 1 1 -Leave) }
    $p = Start-TimedPosts ([IntPtr]::Zero) $posts
    # The script's thread busy and then blocked long past the last post's time, as Get-Frames'
    # captures would be: every post goes while it's blocked, so none waited on it. Held by that,
    # not by how late a post may be, which a loaded PC moves by hundreds of ms (PITFALLS 132)
    $x = 0; foreach ($i in 1..100000) { $x += $i }
    Start-Sleep -Milliseconds 2000
    $blockedTill = (Get-UnixMs) - $p.StartUnixMs
    $p.Wait(5000) | Should -BeTrue
    $p.Done | Should -BeTrue
    $p.Sent.Count | Should -Be 5
    for ($i = 0; $i -lt 5; $i++) {
      $p.Sent[$i] | Should -BeGreaterOrEqual $posts[$i].AtMs
      if ($i) { $p.Sent[$i] | Should -BeGreaterOrEqual $p.Sent[$i - 1] }
      $p.Sent[$i] | Should -BeLessThan $blockedTill
    }
    $p.StartUnixMs | Should -BeGreaterThan ((Get-UnixMs) - 60000)
  }

  It 'packs a wheel turn''s delta into the high word' {
    ([LbtermE2E.Win]::WheelWParam(120, 0).ToInt64() -band 0xFFFFFFFF) | Should -Be 0x00780000
    ([LbtermE2E.Win]::WheelWParam(-120, 0).ToInt64() -band 0xFFFFFFFF) | Should -Be 0xFF880000
  }

  It 'builds a key''s lParam: repeat 1, the scan code, extended, and an up''s transition bits' {
    ([LbtermE2E.Win]::KeyLParam(0x1C, $false, $false).ToInt64() -band 0xFFFFFFFF) | Should -Be 0x001C0001
    ([LbtermE2E.Win]::KeyLParam(0x1C, $false, $true).ToInt64() -band 0xFFFFFFFF) | Should -Be 0xC01C0001
    ([LbtermE2E.Win]::KeyLParam(0x48, $true, $false).ToInt64() -band 0xFFFFFFFF) | Should -Be 0x01480001
  }

  It 'scales 96-DPI lengths to a window''s pixels' {
    ConvertTo-Px 38 1 | Should -Be 38
    ConvertTo-Px 38 1.5 | Should -Be 57
    ConvertTo-Px 13 1.25 | Should -Be 16
  }

  It 'opens test windows on the right-most screen, counted from the desktop''s top-left' {
    $screens = @(
      [System.Drawing.Rectangle]::new(-1920, 0, 1920, 1080)
      [System.Drawing.Rectangle]::new(0, 0, 2560, 1440)
      [System.Drawing.Rectangle]::new(2560, -400, 1440, 2560)
    )
    $desktop = [System.Drawing.Rectangle]::new(-1920, -400, 5920, 2560)
    Get-OffscreenPosition -Screens $screens -Desktop $desktop | Should -Be '4500,120'
  }

  It 'quotes command-line arguments as CommandLineToArgvW reads them back' {
    ConvertTo-CommandLineArg 'plain' | Should -Be 'plain'
    ConvertTo-CommandLineArg "front_end='WebGpu'" | Should -Be "front_end='WebGpu'"
    ConvertTo-CommandLineArg 'C:/Program Files/PowerShell/7/pwsh.exe' | Should -Be '"C:/Program Files/PowerShell/7/pwsh.exe"'
    ConvertTo-CommandLineArg 'say "hi"' | Should -Be '"say \"hi\""'
    ConvertTo-CommandLineArg 'C:\a b\' | Should -Be '"C:\a b\\"'
    ConvertTo-CommandLineArg '' | Should -Be '""'
  }

  It 'names keys by their virtual-key codes' {
    ConvertTo-Vk 'Enter' | Should -Be 0x0D
    ConvertTo-Vk 'F12' | Should -Be 0x7B
    ConvertTo-Vk 'y' | Should -Be ([uint32][char]'Y')
    ConvertTo-Vk 0x28 | Should -Be 0x28
    { ConvertTo-Vk 'Hyper' } | Should -Throw
  }

  It 'writes the sequences a stand-in tab prints' {
    Get-UserVarSequence 'claude_session' 'abc' | Should -Be "$([char]27)]1337;SetUserVar=claude_session=YWJj$([char]7)"
    $p = New-PwshProgram 'Start-Sleep 600'
    $p[0] | Should -Be 'C:/Program Files/PowerShell/7/pwsh.exe'
    [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($p[3])) | Should -Be 'Start-Sleep 600'
  }
}

Describe 'Pixel probes' {
  BeforeAll {
    $black = New-Bitmap 40 30 ([System.Drawing.Color]::Black)
    $marked = New-Bitmap 40 30 ([System.Drawing.Color]::Black)
    foreach ($x in 10..14) { foreach ($y in 5..7) { $marked.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, 0x39, 0xA5, 0x9C)) } }
  }

  It 'boxes what differs, and nothing for the same picture' {
    [LbtermE2E.Px]::DiffBox($black, $marked, [LbtermE2E.Px]::All($black)) | Should -Be @(10, 5, 14, 7)
    [LbtermE2E.Px]::DiffBox($black, $black, [LbtermE2E.Px]::All($black)) | Should -BeNullOrEmpty
    [LbtermE2E.Px]::DiffBox($black, $marked, (New-Rect 20 0 20 30)) | Should -BeNullOrEmpty
    $d = Get-DiffBox $black $marked
    $d | Should -Be @(10, 5, 14, 7)
    Get-DiffBox $black $black | Should -BeNullOrEmpty
    Format-Box @(10, 5, 14, 7) | Should -Be 'x 10..14, y 5..7'
    Format-Box $null | Should -Be 'nothing'
  }

  It 'counts a colour by its channels'' ranges, and finds where it is' {
    $teal = New-ColourRange 28 86 136 194 127 185
    [LbtermE2E.Px]::CountRange($marked, [LbtermE2E.Px]::All($marked), $teal) | Should -Be 15
    [LbtermE2E.Px]::CountNear($marked, [LbtermE2E.Px]::All($marked), 0x39A59C, 0) | Should -Be 15
    [LbtermE2E.Px]::MeanYInRange($marked, [LbtermE2E.Px]::All($marked), $teal) | Should -Be 6
    [LbtermE2E.Px]::MeanYInRange($black, [LbtermE2E.Px]::All($black), $teal) | Should -Be -1
    [LbtermE2E.Px]::RunStartsDown($marked, 12, $teal) | Should -Be @(5)
  }

  It 'finds the rows with a long run of a colour, as a strike across words makes' {
    $teal = New-ColourRange 28 86 136 194 127 185
    $struck = New-Bitmap 40 30 ([System.Drawing.Color]::Black)
    foreach ($x in 4..33) { $struck.SetPixel($x, 20, [System.Drawing.Color]::FromArgb(255, 0x39, 0xA5, 0x9C)) }
    foreach ($x in 4..9) { $struck.SetPixel($x, 24, [System.Drawing.Color]::FromArgb(255, 0x39, 0xA5, 0x9C)) }
    [LbtermE2E.Px]::RowsWithRun($struck, [LbtermE2E.Px]::All($struck), $teal, 20) | Should -Be @(20)
    [LbtermE2E.Px]::RowsWithRun($struck, [LbtermE2E.Px]::All($struck), $teal, 6) | Should -Be @(20, 24)
    [LbtermE2E.Px]::RowsWithRun($marked, [LbtermE2E.Px]::All($marked), $teal, 6) | Should -BeNullOrEmpty -Because 'a glyph-sized block runs only 5'
    [LbtermE2E.Px]::RowsWithRun($struck, (New-Rect 0 0 20 30), $teal, 20) | Should -BeNullOrEmpty -Because 'only what lies in the box counts'
  }

  It 'measures brightness and how far apart two pictures are' {
    [LbtermE2E.Px]::Light($black, [LbtermE2E.Px]::All($black)) | Should -Be 0
    [LbtermE2E.Px]::LumDistance($black, $black, [LbtermE2E.Px]::All($black)) | Should -Be 0
    [LbtermE2E.Px]::LumDistance($black, $marked, (New-Rect 10 5 5 3)) | Should -BeGreaterThan 100
    [LbtermE2E.Px]::Changed($black, $marked, [LbtermE2E.Px]::All($black), 10) | Should -Be 15
    $small = New-Bitmap 10 10 ([System.Drawing.Color]::Black)
    [LbtermE2E.Px]::LumDistance($black, $small, [LbtermE2E.Px]::All($black)) | Should -Be 255
  }

  It 'tells a glide (each pixel old or new) from a dissolve (a blend)' {
    $old = New-Bitmap 10 1 ([System.Drawing.Color]::Black)
    $new = New-Bitmap 10 1 ([System.Drawing.Color]::White)
    $glide = New-Bitmap 10 1 ([System.Drawing.Color]::Black)
    foreach ($x in 0..4) { $glide.SetPixel($x, 0, [System.Drawing.Color]::White) }
    $blend = New-Bitmap 10 1 ([System.Drawing.Color]::Gray)
    $g = [LbtermE2E.Px]::Glide($old, $new, $glide, [LbtermE2E.Px]::All($old))
    $g | Should -Be @(0.5, 1, 10)
    $d = [LbtermE2E.Px]::Glide($old, $new, $blend, [LbtermE2E.Px]::All($old))
    $d[1] | Should -Be 0
  }

  It 'says how much of each of several pictures a cross-fade of them shows' {
    # Three pictures each with a block of its own, as tabs in flight each with its block, mixed
    # as DWM lays them over each other: each pixel the pictures weighted by their shares
    $parts = foreach ($at in 0, 10, 20) {
      $b = New-Bitmap 30 6 ([System.Drawing.Color]::FromArgb(255, 18, 18, 18))
      foreach ($x in $at..($at + 7)) { foreach ($y in 1..4) { $b.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, 220, 200, 180)) } }
      $b
    }
    $shares = @(0.2, 0.5, 0.3)
    $mixed = New-Bitmap 30 6 ([System.Drawing.Color]::Black)
    foreach ($x in 0..29) {
      foreach ($y in 0..5) {
        $c = foreach ($ch in 'R', 'G', 'B') { [int][Math]::Round((0..2 | ForEach-Object { $shares[$_] * $parts[$_].GetPixel($x, $y).$ch } | Measure-Object -Sum).Sum) }
        $mixed.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, $c[0], $c[1], $c[2]))
      }
    }
    $got = [LbtermE2E.Px]::Mix($mixed, [System.Drawing.Bitmap[]]$parts, [LbtermE2E.Px]::All($mixed))
    $got.Count | Should -Be 3
    foreach ($k in 0..2) { [Math]::Abs($got[$k] - $shares[$k]) | Should -BeLessThan 0.01 }
    # One picture whole is all of it; pictures alike can't be told apart; sizes must match
    $one = [LbtermE2E.Px]::Mix($parts[1], [System.Drawing.Bitmap[]]$parts, [LbtermE2E.Px]::All($mixed))
    [Math]::Abs($one[1] - 1) | Should -BeLessThan 1e-9
    [LbtermE2E.Px]::Mix($mixed, [System.Drawing.Bitmap[]]@($parts[0], $parts[0]), [LbtermE2E.Px]::All($mixed)).Count | Should -Be 0
    [LbtermE2E.Px]::Mix($mixed, [System.Drawing.Bitmap[]]@((New-Bitmap 10 10 ([System.Drawing.Color]::Black)), $parts[0]), [LbtermE2E.Px]::All($mixed)).Count | Should -Be 0
  }
}

Describe 'A shot with the compositor against one without (motion\compare.ps1)' {
  BeforeAll {
    function ConvertTo-Linear([double]$s) { if ($s -le 0.04045) { $s / 12.92 } else { [Math]::Pow(($s + 0.055) / 1.055, 2.4) } }
    function ConvertTo-Srgb([double]$l) { if ($l -le 0.0031308) { $l * 12.92 } else { 1.055 * [Math]::Pow($l, 1 / 2.4) - 0.055 } }
    # How much of each pixel an outline covers as LBTerm strokes one (render/card.rs,
    # lbterm_stroke): a rounded square's edge, $Inset in from the picture's, corners of $Radius,
    # opaque $Core px inward from it (1, a hairline) and soft 1 px either side, sampled at each
    # pixel's centre: [y, x] in 0..1
    function Get-RingCover([int]$Size, [double]$Inset, [double]$Radius, [int]$Core = 1) {
      $cover = [double[,]]::new($Size, $Size)
      $half = $Size / 2 - $Inset
      for ($y = 0; $y -lt $Size; $y++) {
        for ($x = 0; $x -lt $Size; $x++) {
          # How far outside the rounded square the pixel's centre is (inside when negative)
          $qx = [Math]::Abs($x + 0.5 - $Size / 2) - ($half - $Radius)
          $qy = [Math]::Abs($y + 0.5 - $Size / 2) - ($half - $Radius)
          $d = [Math]::Sqrt([Math]::Pow([Math]::Max($qx, 0), 2) + [Math]::Pow([Math]::Max($qy, 0), 2)) + [Math]::Min([Math]::Max($qx, $qy), 0) - $Radius
          $cover[$y, $x] = if ($d -ge 1) { 0 } elseif ($d -ge 0) { 1 - $d } elseif ($d -ge -$Core) { 1 } elseif ($d -ge -$Core - 1) { $d + $Core + 1 } else { 0 }
        }
      }
      , $cover
    }
    # The ring in grey $Line over grey $Back, each pixel blended as the compositor's DWM does it
    # (-Dwm: the covered share of the line, encoded, over the back in stored values) or as the GPU
    # does without it (in linear light). -Shift moves it right.
    function New-RingShot($Cover, [int]$Line, [int]$Back, [switch]$Dwm, [int]$Shift = 0) {
      $n = $Cover.GetLength(0)
      $b = New-Bitmap $n $n ([System.Drawing.Color]::FromArgb(255, $Back, $Back, $Back))
      $ll = ConvertTo-Linear ($Line / 255); $bl = ConvertTo-Linear ($Back / 255)
      for ($y = 0; $y -lt $n; $y++) {
        for ($x = 0; $x -lt $n - $Shift; $x++) {
          $c = $Cover[$y, $x]
          if ($c -eq 0) { continue }
          $v = if ($Dwm) { 255 * (ConvertTo-Srgb ($c * $ll)) + (1 - $c) * $Back } else { 255 * (ConvertTo-Srgb ($c * $ll + (1 - $c) * $bl)) }
          $v = [int][Math]::Round($v)
          $b.SetPixel($x + $Shift, $y, [System.Drawing.Color]::FromArgb(255, $v, $v, $v))
        }
      }
      $b
    }
    # Its edge through the pixels' centres, so round its corners some pixels have no two
    # neighbours both pictures agree on (the tab panel's pill had eight, 2026-10-30)
    $cover = Get-RingCover 40 6.5 5.5
    $back = 18
  }

  It 'finds nothing between the same picture' {
    $a = New-RingShot $cover 200 $back -Dwm
    $c = [LbtermE2E.Px]::Compare($a, $a, @(), 0.85, 0xFF121212, 6, @())
    $c.Differing | Should -Be 0
    $c.Other | Should -Be 0
    $c.Marked.Width | Should -Be 40
  }

  It 'takes a ring''s edges blended in stored values for soft, round its corners too' {
    $comp = New-RingShot $cover 200 $back -Dwm
    $plain = New-RingShot $cover 200 $back
    $c = [LbtermE2E.Px]::Compare($comp, $plain, @(), 0.85, 0xFF121212, 6, @())
    $c.Differing | Should -BeGreaterThan 20 -Because 'DWM''s blend in stored values lifts a partly covered pixel by several levels'
    $c.Other | Should -Be 0
    $c.Soft | Should -Be $c.Differing
    $c.SoftArc | Should -BeGreaterThan 0 -Because 'round a corner no two neighbours of some pixels agree'
    $c.SoftBrighter | Should -Be $c.Soft
  }

  It 'counts soft pixels only where it''s told edges lie over another visual' {
    $comp = New-RingShot $cover 255 $back -Dwm
    $plain = New-RingShot $cover 255 $back
    $all = [LbtermE2E.Px]::Compare($comp, $plain, @(), 0.85, 0xFF121212, 6, @())
    $left = [System.Drawing.Rectangle[]]@((New-Rect 0 0 20 40))
    $c = [LbtermE2E.Px]::Compare($comp, $plain, @(), 0.85, 0xFF121212, 6, $left)
    $c.SoftOutside | Should -BeGreaterThan 0
    $c.Soft + $c.SoftOutside | Should -Be $all.Soft
    $c.Other | Should -Be $c.SoftOutside
    @($c.OtherAt | Where-Object { $_[0] -lt 20 }) | Should -BeNullOrEmpty
  }

  It 'calls a ring moved a pixel other' {
    $comp = New-RingShot $cover 200 $back -Dwm
    $plain = New-RingShot $cover 200 $back -Shift 1
    $c = [LbtermE2E.Px]::Compare($comp, $plain, @(), 0.85, 0xFF121212, 6, @())
    $c.Other | Should -BeGreaterThan 20
    $c.OtherBox | Should -Not -BeNullOrEmpty
    $c.OtherAt.Count | Should -BeLessOrEqual 40
  }

  It 'calls an edge a pixel wider or narrower in one shot other, though it lies between its neighbours' {
    # The ring a pixel thicker, blended DWM's way, against the hairline the GPU's way, and the
    # other way round: the pixel that's the line in one and what's under it in the other
    $thick = Get-RingCover 40 6.5 5.5 -Core 2
    $wider = [LbtermE2E.Px]::Compare((New-RingShot $thick 200 $back -Dwm), (New-RingShot $cover 200 $back), @(), 0.85, 0xFF121212, 6, @())
    $wider.Other | Should -BeGreaterThan 20
    $narrower = [LbtermE2E.Px]::Compare((New-RingShot $cover 200 $back -Dwm), (New-RingShot $thick 200 $back), @(), 0.85, 0xFF121212, 6, @())
    $narrower.Other | Should -BeGreaterThan 20
    # A hard line, nothing soft about it, a pixel wider: the extra column lies between the line
    # and what's under it, as a soft pixel does, but no coverage blends to it (the review of
    # 2026-10-30: 30 such pixels passed as soft)
    $one = New-Bitmap 20 20 ([System.Drawing.Color]::FromArgb(255, $back, $back, $back))
    $two = New-Bitmap 20 20 ([System.Drawing.Color]::FromArgb(255, $back, $back, $back))
    foreach ($y in 2..17) {
      $one.SetPixel(9, $y, [System.Drawing.Color]::FromArgb(255, 200, 200, 200))
      foreach ($x in 9, 10) { $two.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, 200, 200, 200)) }
    }
    $hard = [LbtermE2E.Px]::Compare($two, $one, @(), 0.85, 0xFF121212, 6, @())
    $hard.Differing | Should -Be 16
    $hard.Soft | Should -Be 0
    $hard.Other | Should -Be 16
    # The ring's own soft pixels are each within a level or so of DWM's blend
    $soft = [LbtermE2E.Px]::Compare((New-RingShot $cover 200 $back -Dwm), (New-RingShot $cover 200 $back), @(), 0.85, 0xFF121212, 6, @())
    $soft.MaxBlendError | Should -BeLessOrEqual 1.5
    # Half of grey 200 over 18: the GPU's 147 in linear light, DWM's 155 in stored values
    [LbtermE2E.Px]::BlendError(0xFF9B9B9B, 0xFF939393, 0xFFC8C8C8, 0xFF121212) | Should -BeLessOrEqual 1
    [LbtermE2E.Px]::BlendError(0xFFC8C8C8, 0xFF121212, 0xFFC8C8C8, 0xFF121212) | Should -BeGreaterThan 100
  }

  It 'calls a ring drawn in another colour, or not at all, other' {
    $comp = New-RingShot $cover 200 $back -Dwm
    ([LbtermE2E.Px]::Compare($comp, (New-RingShot $cover 150 $back), @(), 0.85, 0xFF121212, 6, @())).Other | Should -BeGreaterThan 20
    $none = New-Bitmap 40 40 ([System.Drawing.Color]::FromArgb(255, $back, $back, $back))
    ([LbtermE2E.Px]::Compare($comp, $none, @(), 0.85, 0xFF121212, 6, @())).Other | Should -BeGreaterThan 20
  }

  It 'shows a pane without the keyboard at 85% over the window background before comparing' {
    [LbtermE2E.Px]::Over(0xFFFFFFFF, 0.85, 0xFF121212) | Should -Be 0xFFDBDBDB
    [LbtermE2E.Px]::Over(0xFF121212, 0.85, 0xFF121212) | Should -Be 0xFF121212
    $plain = New-Bitmap 40 30 ([System.Drawing.Color]::FromArgb(255, 18, 18, 18))
    $comp = New-Bitmap 40 30 ([System.Drawing.Color]::FromArgb(255, 18, 18, 18))
    foreach ($x in 5..9) { foreach ($y in 5..9) { $plain.SetPixel($x, $y, [System.Drawing.Color]::White); $comp.SetPixel($x, $y, [System.Drawing.Color]::FromArgb(255, 219, 219, 219)) } }
    $pane = [System.Drawing.Rectangle[]]@((New-Rect 2 2 20 20))
    $c = [LbtermE2E.Px]::Compare($comp, $plain, $pane, 0.85, 0xFF121212, 6, @())
    $c.Dimmed | Should -Be 400
    $c.Differing | Should -Be 0
    $c = [LbtermE2E.Px]::Compare($comp, $plain, @(), 0.85, 0xFF121212, 6, @())
    $c.Other | Should -Be 25 -Because 'undimmed, the text is 36 levels brighter in every channel'
    { [LbtermE2E.Px]::Compare($comp, (New-Bitmap 10 10 ([System.Drawing.Color]::Black)), @(), 0.85, 0xFF121212, 6, @()) } | Should -Throw '*differ in size*'
  }
}

Describe 'Windows that aren''t the test''s' {
  BeforeAll {
    function New-Seen([uint32]$ProcessId, [uint32[]]$Ancestors = @(), [string]$Class = 'ConsoleWindowClass', [string]$Exe = 'conhost.exe', [string[]]$Names = @(), [string[]]$ProcessClasses = @()) {
      [pscustomobject]@{ Hwnd = 1; Pid = $ProcessId; Exe = $Exe; Ancestors = $Ancestors; AncestorExes = $Names; Title = 'C:\WINDOWS\system32\conhost.exe'
        Class = $Class; At = [datetime]'2026-10-22 02:10:11.123'; IdleMs = 4200; Width = 993; Height = 519
        ProcessClasses = $(if ($ProcessClasses) { $ProcessClasses } else { @($Class) }) }
    }
  }

  It 'takes an LBTerm window of a process the test opened, or of one it started, as the test''s own' {
    Test-OwnWindow (New-Seen 500 -Exe 'lbterm-gui.exe') @(500) | Should -BeTrue
    Test-OwnWindow (New-Seen 700 @(650, 500, 4) -Exe 'lbterm-gui.exe') @(500) | Should -BeTrue
    Test-OwnWindow (New-Seen 700 @(650, 4) -Exe 'lbterm-gui.exe') @(500) | Should -BeFalse
    Test-OwnWindow (New-Seen 700 -Exe 'lbterm-gui.exe') @() | Should -BeFalse
  }

  It 'never takes a console window as the test''s own, even one its own process opened' {
    Test-OwnWindow (New-Seen 900 @(800, 500)) @(500) | Should -BeFalse
    Test-OwnWindow (New-Seen 901 @(500) -Exe 'WindowsTerminal.exe' -Class 'CASCADIA_HOSTING_WINDOW_CLASS') @(500) | Should -BeFalse
  }

  It 'knows another LBTerm test window by its class, or by its process''s test window' {
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm.perf-tab-mouse') | Should -BeTrue
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm.try') | Should -BeTrue
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm.panel') | Should -BeTrue
    # A tab's ghost card, from a process showing a test window
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'LBTermTabGhost' -ProcessClasses @('com.lachlanb.lbterm.perf-tab-mouse', 'LBTermTabGhost')) | Should -BeTrue
    # Lachlan's own LBTerm, a ghost card of his, an LBTerm with a class of no test's
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm') | Should -BeFalse
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'LBTermTabGhost' -ProcessClasses @('com.lachlanb.lbterm', 'LBTermTabGhost')) | Should -BeFalse
    Test-OtherTestWindow (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm.stray-x') | Should -BeFalse
    # A test window's class on anything but LBTerm
    Test-OtherTestWindow (New-Seen 600 -Exe 'wezterm-gui.exe' -Class 'com.lachlanb.lbterm.perf-x') | Should -BeFalse
    Test-OtherTestWindow (New-Seen 600 -Exe 'conhost.exe' -ProcessClasses @('com.lachlanb.lbterm.perf-x')) | Should -BeFalse
  }

  It 'sorts each window: own, noise, allowed, another test''s, or foreign' {
    $allow = { param($w) $w.Exe -eq 'ShellExperienceHost.exe' }
    Get-WindowKind -Seen (New-Seen 500 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm.perf-x') -Allowed @(500) -Allow $allow | Should -Be 'own'
    Get-WindowKind -Seen (New-Seen 901 @(4) -Class 'tooltips_class32' -Exe 'explorer.exe') -Allowed @(500) -Allow $allow | Should -Be 'noise'
    Get-WindowKind -Seen (New-Seen 903 @(4) -Class 'Windows.UI.Core.CoreWindow' -Exe 'ShellExperienceHost.exe') -Allowed @(500) -Allow $allow | Should -Be 'allowed'
    Get-WindowKind -Seen (New-Seen 600 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm.perf-y') -Allowed @(500) -Allow $allow | Should -Be 'test'
    Get-WindowKind -Seen (New-Seen 900 @(800, 500)) -Allowed @(500) -Allow $allow | Should -Be 'foreign'
    Get-WindowKind -Seen (New-Seen 610 -Exe 'lbterm-gui.exe' -Class 'com.lachlanb.lbterm') -Allowed @(500) | Should -Be 'foreign'
  }

  It 'leaves out tooltips, menus and their shadows, and the input method''s windows' {
    Test-NoiseWindow (New-Seen 1 -Class 'tooltips_class32') | Should -BeTrue
    Test-NoiseWindow (New-Seen 1 -Class '#32768') | Should -BeTrue
    Test-NoiseWindow (New-Seen 1 -Class 'SysShadow') | Should -BeTrue
    Test-NoiseWindow (New-Seen 1 -Class 'MozillaDropShadowWindowClass' -Exe 'firefox.exe') | Should -BeTrue
    Test-NoiseWindow (New-Seen 1 -Class 'MozillaWindowClass' -Exe 'firefox.exe') | Should -BeFalse
    Test-NoiseWindow (New-Seen 1 -Class 'DFTitleBarWindow:01a160f6-430f-75cf-82e2-13456fd0cfd5' -Exe 'DisplayFusion.ui.exe') | Should -BeTrue
    Test-NoiseWindow (New-Seen 1 -Class 'DFTitleBar' -Exe 'DisplayFusion.ui.exe') | Should -BeFalse
    Test-NoiseWindow (New-Seen 1 -Class 'ConsoleWindowClass') | Should -BeFalse
    Test-NoiseWindow (New-Seen 1 -Class 'CASCADIA_HOSTING_WINDOW_CLASS') | Should -BeFalse
  }

  It 'leaves out a browser''s untitled windows (its status bubble and popups), and none with a title' {
    $bubble = New-Seen 1 -Class 'Chrome_WidgetWin_1' -Exe 'chrome.exe'
    $bubble.Title = ''; $bubble.Width = 270; $bubble.Height = 23
    Test-NoiseWindow $bubble | Should -BeTrue
    $bubble.Exe = 'msedge.exe'
    Test-NoiseWindow $bubble | Should -BeTrue
    # Its popups: the address bar's list, a menu (E3 of 2026-11-10: 409x600 and 270x58)
    $popup = New-Seen 1 -Class 'Chrome_WidgetWin_1' -Exe 'chrome.exe'
    $popup.Title = ''; $popup.Width = 409; $popup.Height = 600
    Test-NoiseWindow $popup | Should -BeTrue
    # A page a test opened: a title, and a window's size
    $page = New-Seen 1 -Class 'Chrome_WidgetWin_1' -Exe 'chrome.exe'
    $page.Title = 'Example Domain - Google Chrome'
    Test-NoiseWindow $page | Should -BeFalse
    $page.Title = 'Example Domain - Google Chrome'; $page.Height = 23
    Test-NoiseWindow $page | Should -BeFalse
    # Another Chromium program's strip is its own
    $code = New-Seen 1 -Class 'Chrome_WidgetWin_1' -Exe 'Code.exe'
    $code.Title = ''; $code.Height = 23
    Test-NoiseWindow $code | Should -BeFalse
    Get-WindowKind -Seen $bubble -Allowed @(500) | Should -Be 'noise'
  }

  It 'fails for the windows that are neither the test''s, nor noise, nor let through, nor another test''s, and notes those' {
    $seen = @(
      New-Seen 500 -Class 'com.lachlanb.lbterm.perf-x' -Exe 'lbterm-gui.exe'
      New-Seen 900 @(800, 500) -Exe 'conhost.exe'
      New-Seen 901 @(4) -Class 'tooltips_class32' -Exe 'explorer.exe'
      New-Seen 902 @(810, 4) -Class 'CASCADIA_HOSTING_WINDOW_CLASS' -Exe 'WindowsTerminal.exe'
      New-Seen 903 @(4) -Class 'Windows.UI.Core.CoreWindow' -Exe 'ShellExperienceHost.exe'
      New-Seen 904 @(42) -Class 'com.lachlanb.lbterm.perf-other' -Exe 'lbterm-gui.exe'
      New-Seen 905 @(42) -Class 'com.lachlanb.lbterm' -Exe 'lbterm-gui.exe'
    )
    $foreign = Get-ForeignWindows -Seen $seen -Allowed @(500)
    ($foreign | ForEach-Object Pid) | Should -Be @(900, 902, 903, 905)
    $allowed = Get-ForeignWindows -Seen $seen -Allowed @(500) -Allow { param($w) $w.Exe -eq 'ShellExperienceHost.exe' }
    ($allowed | ForEach-Object Pid) | Should -Be @(900, 902, 905)
    (Get-OtherTestWindows -Seen $seen -Allowed @(500) | ForEach-Object Pid) | Should -Be @(904)
    @(Get-ForeignWindows -Seen @() -Allowed @(500)).Count | Should -Be 0
    @(Get-OtherTestWindows -Seen @() -Allowed @(500)).Count | Should -Be 0
  }

  It 'says what opened a foreign window, what it said, when, and how long after the last real input' {
    $m = Format-ForeignWindow (New-Seen 902 @(810, 4) -Class 'CASCADIA_HOSTING_WINDOW_CLASS' -Exe 'WindowsTerminal.exe' -Names @('lbterm-mux-server.exe', 'System'))
    $m | Should -Match '^a window that isn''t the test''s appeared: WindowsTerminal\.exe \(pid 902, started by lbterm-mux-server\.exe 810 < System 4\)'
    $m | Should -Match "'C:\\WINDOWS\\system32\\conhost\.exe' \[CASCADIA_HOSTING_WINDOW_CLASS\], 993x519, at 02:10:11\.123, 4200 ms after the last real key or mouse input$"
    $n = Format-ForeignWindow (New-Seen 904 @(42) -Class 'com.lachlanb.lbterm.perf-other' -Exe 'lbterm-gui.exe' -Names @('WmiPrvSE.exe')) -Note
    $n | Should -Match '^another LBTerm test window appeared \(not counted\): lbterm-gui\.exe \(pid 904, started by WmiPrvSE\.exe 42\), .*\[com\.lachlanb\.lbterm\.perf-other\]'
  }

  It 'reads a window''s own lines from its log, not those of an earlier process with its pid' {
    $log = Join-Path $TestDrive 'lbterm-gui.exe-log-4242.txt'
    [IO.File]::WriteAllText($log, "04:16:03.779  INFO   x > control: listening`n04:16:14.590  WARN   lbterm::budget > tab took 26.5 ms, 6.5 ms over its 20 ms`n")
    $start = (Get-Item $log).Length
    [IO.File]::AppendAllText($log, "17:09:32.757  INFO   x > control: listening`r`n17:09:43.610  WARN   lbterm::budget > tab took 75.1 ms, 35.1 ms over its 40 ms`r`n")
    $w = [pscustomobject]@{ Pid = 4242; Log = $log; LogStart = $start }
    @(Get-LogLines $w 'lbterm::budget') | Should -Be @('17:09:43.610  WARN   lbterm::budget > tab took 75.1 ms, 35.1 ms over its 40 ms')
    @(Read-LogLines $log).Count | Should -Be 4
    @(Read-LogLines (Join-Path $TestDrive 'none.txt')).Count | Should -Be 0
    Wait-LogLine $w 'over its 40 ms' -TimeoutSec 0.2 | Should -BeTrue
    Wait-LogLine $w 'over its 20 ms' -TimeoutSec 0.2 | Should -BeFalse
    # A window record made without a start reads the whole file, as before
    @(Get-LogLines ([pscustomobject]@{ Pid = 4242; Log = $log; LogStart = 0 }) 'lbterm::budget').Count | Should -Be 2
  }

  It 'says when a test window took the foreground, what had it before and where it went' {
    $fg = { param([uint32]$P, [string]$Exe, [string]$Title, [string]$Class, [string]$At, [uint32]$Idle)
      [pscustomobject]@{ Hwnd = 1; Pid = $P; Exe = $Exe; Title = $Title; Class = $Class; At = [datetime]"2026-10-30 $At"; IdleMs = $Idle } }
    $log = @(
      (& $fg 700 'chrome.exe' 'Inbox' 'Chrome_WidgetWin_1' '12:28:50.100' 40),
      (& $fg 710 'ms-teams.exe' '' 'TeamsWebView' '12:28:53.000' 230),
      (& $fg 500 'lbterm-gui.exe' '[1/2] pwsh.exe' 'com.lachlanb.lbterm.perf-budgets-new-tab' '12:28:53.366' 594),
      (& $fg 500 'lbterm-gui.exe' '[2/2] pwsh.exe' 'com.lachlanb.lbterm.perf-budgets-new-tab' '12:28:54.000' 1200),
      (& $fg 700 'chrome.exe' 'Inbox' 'Chrome_WidgetWin_1' '12:28:56.250' 10)
    )
    $m = Format-ForegroundTaken -Pids @(500) -Log $log -Ours @(500, 501) -Stolen 'at 12:28:59.000, 2750 ms after the last real key or mouse input'
    $m | Should -Be "a test window took the foreground (pid 500), first at 12:28:53.366, 594 ms after the last real key or mouse input, from ms-teams.exe (pid 710), '' [TeamsWebView]; it went to chrome.exe (pid 700), 'Inbox' [Chrome_WidgetWin_1] at 12:28:56.250"
    # Kept to the end, and had as the test began
    $kept = Format-ForegroundTaken -Pids @(500) -Log @($log[0], $log[2]) -Ours @(500)
    $kept | Should -Match ", from chrome\.exe \(pid 700\), 'Inbox' \[Chrome_WidgetWin_1\]; a test window still had it at the test's end$"
    Format-ForegroundTaken -Pids @(500) -Log @($log[2]) -Ours @(500) | Should -Match ', which had it as the test began; '
    # The desktop's watch never saw it (no window check): the look as input was posted
    Format-ForegroundTaken -Pids @(500) -Log @($log[0]) -Ours @(500) -Stolen 'at 12:00:00.000, 5 ms after the last real key or mouse input' |
      Should -Be 'a test window took the foreground (pid 500), first at 12:00:00.000, 5 ms after the last real key or mouse input'
  }

  It 'puts notes in the result only when there are some, and carries them to the report' {
    $r = ConvertTo-TestResult -Name 'x' -Seconds 1 -Notes @('another LBTerm test window appeared (not counted): lbterm-gui.exe', '')
    $r.pass | Should -BeTrue
    $r.notes | Should -Be @('another LBTerm test window appeared (not counted): lbterm-gui.exe')
    (ConvertTo-TestResult -Name 'x' -Seconds 1 -Notes @()).Contains('notes') | Should -BeFalse
    $d = Join-Path $TestDrive 'noted'
    New-Item -ItemType Directory $d | Out-Null
    Write-TestResult $r (Join-Path $d 'result.json')
    $a = Read-E2eAttempt -Dir $d -Exit 0 -Seconds 1
    $a.Notes | Should -Be @('another LBTerm test window appeared (not counted): lbterm-gui.exe')
    $report = New-E2eReport -Run 'r' -Bin 'b' -Commit 'abc' -Started (Get-Date) -Results @([pscustomobject]@{ Name = 'a'; Script = 'a'; Args = @(); Attempts = @($a, $a); Result = 'pass' })
    $report.scripts[0].notes.Count | Should -Be 1
    $report.summary.notes | Should -Be 1
    $table = @(Format-E2eTable $report)
    $table[-2] | Should -Match '^notes: 1, windows of other tests \(not failures\), in report\.json$'
    $table[-1] | Should -Match '^1 passed, 0 flaky, 0 failed'
  }
}

Describe 'The state file' {
  BeforeEach {
    $state = Join-Path $TestDrive "state-$([guid]::NewGuid().ToString('N')).json"
  }

  It 'reads a snapshot, and finds a window by its HWND or id' {
    Write-FakeState $state 7 @(
      @{ id = 0; hwnd = 111; frame = 7; animating = $false; settled = $true; animations = @(); waits = @() }
      @{ id = 1; hwnd = 222; frame = 3; animating = $true; settled = $false; animations = @(@{ name = 'rail.edge'; progress = 0.5; left_ms = 90 }); waits = @() }
    )
    $s = Read-State -Path $state
    $s.frame | Should -Be 7
    (Get-StateWindow $s -Hwnd 222).id | Should -Be 1
    (Get-StateWindow $s -WindowId 0).hwnd | Should -Be 111
    (Get-StateWindow $s).id | Should -Be 0
    Get-StateWindow $s -Hwnd 999 | Should -BeNullOrEmpty
  }

  It 'says a build without the state file doesn''t write it' {
    { Read-State -Path $state } | Should -Throw "*doesn't write LBTERM_STATE_FILE*"
    { Wait-State { $true } -Path $state -TimeoutSec 2 -NoFileSec 0.3 } | Should -Throw "*doesn't write LBTERM_STATE_FILE*"
    { Wait-Settled -Path $state -TimeoutSec 0.3 } | Should -Throw "*doesn't write LBTERM_STATE_FILE*"
  }

  It 'refuses a version it doesn''t know' {
    Write-FakeState $state 1 @() -Version 2
    { Read-State -Path $state } | Should -Throw '*version 2*'
  }

  It 'settles on settled, or with -Animating on animating alone' {
    $calm = [pscustomobject]@{ animating = $false; settled = $true }
    $notice = [pscustomobject]@{ animating = $false; settled = $false; waits = @([pscustomobject]@{ name = 'strip.notice'; left_ms = 3000 }) }
    $busy = [pscustomobject]@{ animating = $true; settled = $false }
    Test-StateSettled $calm | Should -BeTrue
    Test-StateSettled $notice | Should -BeFalse
    Test-StateSettled $notice -Animating | Should -BeTrue
    Test-StateSettled $busy -Animating | Should -BeFalse
    Test-StateSettled $null | Should -BeFalse
  }

  It 'hands the predicate the window''s entry and the snapshot, and returns the entry' {
    Write-FakeState $state 3 @(@{ id = 0; hwnd = 5; frame = 3; animating = $false; settled = $true; rail = @{ rows = @(1, 2) } })
    $w = Wait-State { param($w, $s) $w.rail.rows.Count -eq 2 -and $s.frame -eq 3 } -Path $state -TimeoutSec 2
    $w.hwnd | Should -Be 5
  }

  It 'waits for a frame after -AfterFrame, and says what the window last said when none comes' {
    Write-FakeState $state 3 @(@{ id = 0; hwnd = 5; frame = 3; animating = $false; settled = $true; animations = @(); waits = @() })
    { Wait-State { $true } -Path $state -TimeoutSec 0.4 -AfterFrame 3 -What 'the menu' } | Should -Throw '*waiting for the menu*frame 3*'
  }

  It 'sees a snapshot written while it waits, over a half-written one, and shows each to -OnState' {
    Write-FakeState $state 1 @(@{ id = 0; hwnd = 5; frame = 1; animating = $true; settled = $false; animations = @(@{ name = 'pane.layout'; progress = 0.1; left_ms = 200 }); waits = @() })
    $job = Start-ThreadJob -ArgumentList $state -ScriptBlock {
      param($path)
      Start-Sleep -Milliseconds 200
      [IO.File]::WriteAllText($path, '{ "version": 1, "pid": 4242, "fra')
      Start-Sleep -Milliseconds 200
      $tmp = "$path.tmp"
      [IO.File]::WriteAllText($tmp, '{ "version": 1, "pid": 4242, "frame": 9, "windows": [ { "id": 0, "hwnd": 5, "frame": 9, "animating": false, "settled": true, "animations": [], "waits": [] } ] }')
      Move-Item -LiteralPath $tmp -Destination $path -Force
    }
    try {
      $frames = [Collections.Generic.List[long]]::new()
      $w = Wait-Settled -Path $state -TimeoutSec 5 -OnState { param($w) $frames.Add($w.frame) }
      $w.frame | Should -Be 9
      $frames | Should -Be @(1, 9)
    } finally { $job | Wait-Job | Remove-Job }
  }

  It 'waits for a change, then for it to settle on a later frame' {
    Write-FakeState $state 4 @(@{ id = 0; hwnd = 5; frame = 4; animating = $true; settled = $false; rail = @{ rows = @(1, 2) }; animations = @(@{ name = 'rail.row.grow'; progress = 0.3; left_ms = 100 }); waits = @() })
    $job = Start-ThreadJob -ArgumentList $state -ScriptBlock {
      param($path)
      Start-Sleep -Milliseconds 300
      [IO.File]::WriteAllText("$path.tmp", '{ "version": 1, "pid": 4242, "frame": 5, "windows": [ { "id": 0, "hwnd": 5, "frame": 5, "animating": false, "settled": true, "rail": { "rows": [1, 2] }, "animations": [], "waits": [] } ] }')
      Move-Item -LiteralPath "$path.tmp" -Destination $path -Force
    }
    try {
      $w = Wait-Change { param($s) @($s.rail.rows).Count -eq 2 } -Path $state -TimeoutSec 5
      $w.frame | Should -Be 5
    } finally { $job | Wait-Job | Remove-Job }
  }

  It 'records every frame as the GUI renames it into place, and goes through them for the first that shows it, the next wait carrying on after it' {
    $put = { param($n, [bool]$anim) [IO.File]::WriteAllText("$state.tmp", "{`"version`":1,`"pid`":4242,`"build`":`"x`",`"frame`":$n,`"time_ms`":$(1000 + 10 * $n),`"windows`":[{`"id`":0,`"hwnd`":5,`"frame`":$n,`"animating`":$($anim.ToString().ToLower()),`"settled`":true}]}"); Move-Item -LiteralPath "$state.tmp" -Destination $state -Force }
    & $put 1 $false
    $rec = Start-FrameRecorder -Path $state
    try {
      # Each frame is written once the recorder has kept the one before. It reads a snapshot when
      # its watcher's event is handled, on a pool thread, so a frame written over before then is
      # one it never saw, and no sleep between the writes is long enough in the check, beside
      # cargo: 4 ms apart lost frame 5, and 25 ms apart lost 3 and 4 (2026-11-12). Nothing here
      # calls Look(), so it's the watcher that has to read each one
      foreach ($n in 2..6) {
        & $put $n ($n -lt 5)
        $kept = [Diagnostics.Stopwatch]::StartNew()
        while ($rec.Last -lt $n) {
          if ($kept.Elapsed.TotalSeconds -gt 10) { throw "the recorder's watcher never read frame $n, 10 s after it was renamed into place: its last is frame $($rec.Last)" }
          Start-Sleep -Milliseconds 2
        }
      }
      $queue = [Collections.Generic.List[object]]::new()
      $shown = Wait-Recorded -Recorder $rec { param($w, $s) $w.frame -ge 3 } -Hwnd 5 -AfterFrame 1 -Queue $queue -TimeoutSec 3
      $shown.Frame | Should -Be 3
      $shown.TimeMs | Should -Be 1030
      $shown.Window.hwnd | Should -Be 5
      $shown.Missed | Should -Be 0
      $rec.Missed | Should -Be 0
      $settled = Wait-Recorded -Recorder $rec { param($w, $s) -not $w.animating } -Hwnd 5 -Queue $queue -TimeoutSec 3
      $settled.Frame | Should -Be 5
      { Wait-Recorded -Recorder $rec { param($w, $s) $w.frame -gt 99 } -Hwnd 5 -Queue $queue -TimeoutSec 0.3 -What 'frame 100' } | Should -Throw '*waiting for frame 100*frame 6*'
      # A frame no later than the last kept isn't kept again
      & $put 6 $false
      $rec.Look()
      $rec.Take().Count | Should -Be 0
    } finally { $rec.Dispose() }
  }

  It 'says how many frames it never read before the one that shows it, counting none from before -AfterFrame' {
    $put = { param($n) [IO.File]::WriteAllText("$state.tmp", "{`"version`":1,`"pid`":4242,`"build`":`"x`",`"frame`":$n,`"time_ms`":$(1000 + 10 * $n),`"windows`":[{`"id`":0,`"hwnd`":5,`"frame`":$n,`"animating`":false,`"settled`":true}]}"); Move-Item -LiteralPath "$state.tmp" -Destination $state -Force }
    & $put 1
    $rec = Start-FrameRecorder -Path $state
    try {
      # Frames 2, 5, 6 and 9, each read before the next is written: 3, 4, 7 and 8 are frames the
      # process painted that the recorder holds no snapshot of
      foreach ($n in 2, 5, 6, 9) { & $put $n; $rec.Look() }
      $rec.Missed | Should -Be 4
      $queue = [Collections.Generic.List[object]]::new()
      $shown = Wait-Recorded -Recorder $rec { param($w, $s) $w.frame -ge 3 } -Hwnd 5 -AfterFrame 1 -Queue $queue -TimeoutSec 3
      $shown.Frame | Should -Be 5
      $shown.Missed | Should -Be 2
      (Wait-Recorded -Recorder $rec { param($w, $s) $true } -Hwnd 5 -Queue $queue -TimeoutSec 3).Missed | Should -Be 0
      # 7 and 8 were never read, but only 8 came after the frame the action was posted on
      $late = Wait-Recorded -Recorder $rec { param($w, $s) $true } -Hwnd 5 -AfterFrame 7 -Queue $queue -TimeoutSec 3
      $late.Frame | Should -Be 9
      $late.Missed | Should -Be 1
    } finally { $rec.Dispose() }
  }

  It 'says now as the state file''s time_ms counts it' {
    $ms = Get-UnixMs
    [Math]::Abs($ms - [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()) | Should -BeLessThan 50
  }

  It 'waits out a window still animating, and says what' {
    Write-FakeState $state 1 @(@{ id = 0; hwnd = 5; frame = 1; animating = $true; settled = $false; animations = @(@{ name = 'rail.edge'; progress = 0.2; left_ms = 150 }); waits = @(@{ name = 'rail.open'; left_ms = 100 }) })
    { Wait-Settled -Path $state -TimeoutSec 0.4 } | Should -Throw '*animating True (rail.edge)*rail.open 100 ms*'
  }

  It 'gives the middle of a state rectangle, and the rectangle for the probes' {
    $r = [pscustomobject]@{ x = 27; y = 86; w = 22; h = 20 }
    Get-RectCenter $r | Should -Be @(38, 96)
    (ConvertTo-Rect $r).Right | Should -Be 49
  }
}

Describe 'The counters' {
  BeforeAll {
    # A metrics reply as the GUI writes it (lbterm/CONTROL.md), cut down
    $script:metrics = @'
{"id":"metrics","ok":true,"span_ms":3001.5,"reset":true,
 "totals":{"frames":4,"unasked":1,"requests":9,"messages":12,"message_ms":1.25,"tasks":30,"task_ms":8.5,"paints":12,"paint_ms":20.0,"timers_fired":4,"wakes":40,"pdus_sent":0,"pdus_received":0},
 "process":{"cpu_ms":45.0,"gui_thread_ms":31.25},
 "requests":[{"site":"wezterm-gui/src/termwindow/mod.rs:1690","requests":8,"frames":3},{"site":"window/src/os/windows/window.rs:2105","requests":1,"frames":1}],
 "tasks":[{"site":"wezterm-gui/src/termwindow/mod.rs:1754","count":20,"total_ms":6.0,"max_ms":1.5},{"site":"window/src/spawn.rs:52","count":10,"total_ms":2.5,"max_ms":0.5}],
 "messages":[{"site":"WM_PAINT","count":4,"total_ms":1.0,"max_ms":0.3}],
 "timers":[{"site":"window/src/os/windows/window.rs:1818","armed":4,"fired":4,"dropped":0}],
 "mux":[{"pdu":"GetPaneRenderChangesResponse","sent":0,"sent_bytes":0,"received":3,"received_bytes":900}],
 "threads":[{"tid":12,"name":"","cpu_ms":15.625,"cpu_percent":0.5}]}
'@ | ConvertFrom-Json -Depth 16
  }

  It 'says a reply in a few lines: the totals, then the top of each list' {
    $lines = @(Format-Metrics $metrics -Top 1)
    $lines[0] | Should -Be '3001.5 ms: 4 frames (1 unasked), 9 requests, 30 tasks (8.5 ms), 12 messages (1.25 ms), 12 paint parts (20 ms), 4 timers fired, 40 wakes; CPU 45 ms, the GUI thread''s 31.25 ms'
    $lines[1] | Should -Be '  asked by wezterm-gui/src/termwindow/mod.rs:1690: 3 frames, 8 requests'
    $lines | Should -Contain '  task wezterm-gui/src/termwindow/mod.rs:1754: 20 runs, 6 ms, the longest 1.5 ms'
    $lines | Should -Contain '  timer window/src/os/windows/window.rs:1818: 4 fired of 4 armed'
    $lines | Should -Contain '  pdu GetPaneRenderChangesResponse: 0 sent (0 B), 3 received (900 B)'
    $lines | Should -Contain '  thread (unnamed) 12: 15.625 ms'
    @($lines | Where-Object { $_ -like '  asked by*' }).Count | Should -Be 1
  }

  It 'makes a list a table by site, or by pdu' {
    $t = ConvertTo-MetricsTable $metrics.tasks
    $t['window/src/spawn.rs:52'].count | Should -Be 10
    $t.Count | Should -Be 2
    (ConvertTo-MetricsTable $metrics.mux -Key pdu)['GetPaneRenderChangesResponse'].received | Should -Be 3
    (ConvertTo-MetricsTable @()).Count | Should -Be 0
  }
}

Describe 'What DWM does' {
  It 'makes a second of each from two readings, and the share of steps the clock went without a frame' {
    $before = [pscustomobject]@{ At = 1000; Frame = 5000; Cpu = 2e7; Gpu = 1e6 }
    # 4 s on a clock of 1000 a second: 640 frames, 400 ms of CPU, 20 ms of GPU
    $after = [pscustomobject]@{ At = 5000; Frame = 5640; Cpu = 2e7 + 4e6; Gpu = 1e6 + 2e5 }
    $s = ConvertTo-DwmSpan $before $after @(80, 20) -Frequency 1000
    $s.seconds | Should -Be 4
    $s.frames | Should -Be 160
    $s.idle | Should -Be 0.25
    $s.cpu_ms | Should -Be 100
    $s.gpu_ms | Should -Be 5
  }

  It 'counts the frames presented to every screen and to the meter''s own, a 32-bit count coming round' {
    $before = [pscustomobject]@{ At = 1000; Frame = 0; Cpu = 0; Gpu = 0; Screen = [long]7; Presents = @{ [long]7 = 100.0; [long]9 = 4294967000.0 } }
    # 2 s: 120 presented to screen 7, 496 to screen 9 as its count went past 2^32
    $after = [pscustomobject]@{ At = 3000; Frame = 320; Cpu = 0; Gpu = 0; Screen = [long]7; Presents = @{ [long]7 = 220.0; [long]9 = 200.0; [long]11 = 5.0 } }
    $s = ConvertTo-DwmSpan $before $after -Frequency 1000
    $s.screen_presents | Should -Be 60
    $s.presents | Should -Be 308
    # Without the counts, or the screen, nothing is said of them
    $plain = ConvertTo-DwmSpan ([pscustomobject]@{ At = 0; Frame = 0; Cpu = 0; Gpu = 0 }) ([pscustomobject]@{ At = 1000; Frame = 1; Cpu = 0; Gpu = 0 }) -Frequency 1000
    $plain.presents | Should -BeNullOrEmpty
    $plain.screen_presents | Should -BeNullOrEmpty
    $after.Screen = [long]0
    (ConvertTo-DwmSpan $before $after -Frequency 1000).screen_presents | Should -BeNullOrEmpty
  }

  It 'says nothing of idle without steps, and refuses a span that isn''t one' {
    $at = [pscustomobject]@{ At = 1000; Frame = 1; Cpu = 0; Gpu = 0 }
    (ConvertTo-DwmSpan $at ([pscustomobject]@{ At = 2000; Frame = 1; Cpu = 0; Gpu = 0 }) -Frequency 1000).idle | Should -BeNullOrEmpty
    { ConvertTo-DwmSpan $at $at -Frequency 1000 } | Should -Throw '*a span of 0 s*'
  }
}

Describe 'Get-Panes' {
  BeforeEach {
    # The GUI's list with a Configuration Error window's pane first (mux/src/connui.rs's title)
    Mock -ModuleName harness Invoke-Cli { @(
      [pscustomobject]@{ pane_id = 3; window_id = 1; title = 'wezterm Configuration Error'; window_title = 'wezterm Configuration Error' }
      [pscustomobject]@{ pane_id = 0; window_id = 0; title = 'pwsh'; window_title = 'pwsh' }
      [pscustomobject]@{ pane_id = 1; window_id = 0; title = 'Configuration'; window_title = 'pwsh' }
    ) }
  }
  AfterEach { InModuleScope harness { $script:Test = $null } }

  It 'leaves a Configuration Error window''s pane out of the GUI''s list, so [0] is the test window''s' {
    $panes = @(Get-Panes)
    $panes.pane_id | Should -Be @(0, 1)
    (Get-Panes)[0].pane_id | Should -Be 0
  }

  It 'gives the server''s list as it is' {
    @(Get-Panes -Server).pane_id | Should -Be @(3, 0, 1)
  }

  It 'says once in the test''s notes that the window was there' {
    InModuleScope harness { $script:Test = [pscustomobject]@{ Notes = [Collections.Generic.List[string]]::new() } }
    Get-Panes | Out-Null
    Get-Panes | Out-Null
    $notes = @(InModuleScope harness { $script:Test.Notes })
    $notes.Count | Should -Be 1
    $notes[0] | Should -Match 'Configuration Error window'
  }
}

Describe 'Wait-ModifiersUp' {
  AfterEach { InModuleScope harness { $script:Test = $null } }

  It 'waits nothing and notes nothing with no modifier held' {
    InModuleScope harness { $script:Test = [pscustomobject]@{ Notes = [Collections.Generic.List[string]]::new() } }
    Wait-ModifiersUp -Held { $false } | Should -Be 0
    @(InModuleScope harness { $script:Test.Notes }).Count | Should -Be 0
  }

  It 'waits until a held modifier is let go, and notes it once with the longest wait' {
    InModuleScope harness { $script:Test = [pscustomobject]@{ Notes = [Collections.Generic.List[string]]::new() } }
    # Held for the first 60 ms of each wait
    $script:until = $null
    $held = { if (-not $script:until) { $script:until = [DateTime]::UtcNow.AddMilliseconds(60) }; [DateTime]::UtcNow -lt $script:until }
    $waited = Wait-ModifiersUp -Held $held
    $waited | Should -BeGreaterOrEqual 40
    $waited | Should -BeLessThan 1000
    $script:until = $null
    Wait-ModifiersUp -Held $held | Out-Null
    $notes = @(InModuleScope harness { $script:Test.Notes })
    $notes.Count | Should -Be 1
    $notes[0] | Should -Match '2 times'
    $notes[0] | Should -Not -Match 'still held'
  }

  It 'gives up after its time, posting anyway, and says the key went with one held' {
    InModuleScope harness { $script:Test = [pscustomobject]@{ Notes = [Collections.Generic.List[string]]::new() } }
    $waited = Wait-ModifiersUp -TimeoutMs 50 -Held { $true }
    $waited | Should -BeGreaterOrEqual 50
    @(InModuleScope harness { $script:Test.Notes })[0] | Should -Match 'still held'
  }

  It 'works outside a test' {
    Wait-ModifiersUp -TimeoutMs 20 -Held { $true } | Should -BeGreaterOrEqual 20
  }
}

Describe 'Stop-HangWatch' {
  It 'says the longest wait, when it began, the 99th percentile and the waits past each bound' {
    # A watch's two members Stop-HangWatch reads, without a window: its waits and its longest
    $watch = [pscustomobject]@{ Longest = 5200.0; LongestAtMs = 1234.4; Failed = 0 }
    $waits = [double[]]@(@(1..96 | ForEach-Object { 1.0 }) + @(150.0, 1500.0, 2000.0, 5200.0))
    $watch | Add-Member -MemberType ScriptMethod -Name Stop -Value { $waits }.GetNewClosure()
    $r = Stop-HangWatch $watch
    $r.Sent | Should -Be 100
    $r.LongestMs | Should -Be 5200
    $r.LongestAtMs | Should -Be 1234
    $r.P99Ms | Should -Be 2000
    $r.Over100Ms | Should -Be 4
    $r.Over1s | Should -Be 3
    $r.Over5s | Should -Be 1
  }

  It 'says nothing waited when no message was sent' {
    $watch = [pscustomobject]@{ Longest = 0.0; LongestAtMs = 0.0; Failed = 0 }
    $watch | Add-Member -MemberType ScriptMethod -Name Stop -Value { [double[]]@() }
    $r = Stop-HangWatch $watch
    $r.Sent | Should -Be 0
    $r.P99Ms | Should -BeNullOrEmpty
    $r.Over100Ms | Should -Be 0
  }
}

Describe 'The rail under a menu card' {
  # Every rail. and light. name the compositor gives an animation is sorted, held or exempt with a
  # reason, so a test judging Lachlan's 7b takes every name the hold drives, never a hand-picked
  # few, and a name the compositor gains fails here until it's sorted (PITFALLS 210)
  It 'sorts every rail and light animation the compositor names, and names none it lost' {
    $code = Get-Content -Raw (Join-Path $repo 'wezterm-gui/src/termwindow/compositor.rs')
    $named = @([regex]::Matches($code, '"((?:rail|light)\.[a-z_.]+)"') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
    $named.Count | Should -BeGreaterThan 10
    $sorted = Get-RailUnderCard
    $known = @($sorted.Held) + @($sorted.Exempt.Keys)
    @($named | Where-Object { $_ -notin $known }) | Should -BeNullOrEmpty -Because 'each name the compositor drives is held under a card or exempt with a reason'
    @($known | Where-Object { $_ -notin $named }) | Should -BeNullOrEmpty -Because 'a name the compositor no longer gives judges nothing'
    @($sorted.Held | Where-Object { $_ -in $sorted.Exempt.Keys }) | Should -BeNullOrEmpty
    foreach ($why in $sorted.Exempt.Values) { $why | Should -Not -BeNullOrEmpty }
  }

  It 'fails a frame with a card beside a held name, each frame once, and passes one without' {
    $t = [pscustomobject]@{ Failures = [System.Collections.Generic.List[string]]::new() }
    Mock -ModuleName harness Get-Test { $t }
    $u = New-UnderCardWatch -Cards 'menu.list'
    $frame = { param([long]$n, [string[]]$names) [pscustomobject]@{ frame = $n; animations = @($names | ForEach-Object { [pscustomobject]@{ name = $_ } }) } }
    $u.Watch((& $frame 1 @('menu.list', 'menu.list.flash')))
    $u.Watch((& $frame 2 @('rail.row.enter', 'rail.row.grow')))
    $u.Judge('clean') | Should -BeLike '*nothing of the rail moved*'
    $t.Failures.Count | Should -Be 0
    $u.Watch((& $frame 3 @('menu.list', 'rail.row.swap')))
    $u.Watch((& $frame 3 @('menu.list', 'rail.row.swap')))
    $u.Watch((& $frame 4 @('menu.tab', 'rail.marker')))
    $u.Watch((& $frame 5 @('menu.list', 'rail.wave')))
    $u.Judge('swap') | Should -BeNullOrEmpty
    $t.Failures.Count | Should -Be 1
    $t.Failures[0] | Should -BeLike '*frame 3: rail.row.swap beside menu.list'
  }
}
