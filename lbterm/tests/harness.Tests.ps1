# Pester 5+, run by `lbterm.ps1 check`. harness.psm1's parts that need no window: the runner's
# Covers mapping, verdicts and report, the result format, geometry and pixel probes, and the
# state file's reading and waiting against fake files. Nothing here opens a window.

BeforeAll {
  Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
  $repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent

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
}

Describe 'LBTerm''s own test scripts' {
  BeforeAll {
    $real = @(Get-E2eEntries (Join-Path $repo 'lbterm\perf'))
    $tracked = @(git -C $repo ls-files)
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
}

Describe 'Geometry and messages' {
  It 'packs a point into a mouse message''s lParam, negatives as 16-bit two''s complement' {
    [LbtermE2E.Win]::LParam(10, 20).ToInt64() | Should -Be 0x0014000A
    ([LbtermE2E.Win]::LParam(-1, 5).ToInt64() -band 0xFFFFFFFF) | Should -Be 0x0005FFFF
    # y past 32767 comes out negative as a signed 32-bit lParam, as GET_Y_LPARAM reads it
    ([LbtermE2E.Win]::LParam(0, 0xFFFF).ToInt64() -band 0xFFFFFFFF) | Should -Be 0xFFFF0000
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
