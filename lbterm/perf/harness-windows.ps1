#Requires -Version 7
# Covers: lbterm/harness.psm1, lbterm/motion/start-unfocused.ps1
<#
The harness's own window check end to end (Invoke-Test in lbterm/harness.psm1 fails a test when a
top-level window that isn't its own comes up while it runs, and notes another test's). Three small
tests, each a script of its own run in a child pwsh as the runner runs tests:

1. own: opens one test window (Open-TestWindow) and waits for its first frame. It must pass: a
   test's own windows never count against it.
2. noted: opens its own window, then a second LBTerm with a test window's class that isn't this
   test's, as another test running beside it would (Start-Unfocused, on the right-most screen,
   never focused). It must pass, with a note naming lbterm-gui.exe and that window's pid.
3. stray: the same, but the second LBTerm's class is no test's (com.lachlanb.lbterm.stray-...), as
   a window coming up from nowhere would be. It must fail, and its failure must name
   lbterm-gui.exe and that window's pid.

The extra windows are closed here whatever happens; this test's own check lets the children's
windows through (by their classes).

  pwsh -File lbterm\perf\harness-windows.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'harness-windows' -Bin $Bin
$module = Join-Path $PSScriptRoot '..\harness.psm1'
$strayClass = 'com.lachlanb.lbterm.stray-harness-windows'
$notedClass = 'com.lachlanb.lbterm.perf-harness-windows-other'

# A child test: $body runs in its Invoke-Test, its output in <out>\<name>
function Write-Child([string]$Name, [string]$Body) {
  $path = Join-Path $T.Out "$Name.ps1"
  [IO.File]::WriteAllText($path, @"
`$ErrorActionPreference = 'Stop'
Import-Module '$module' -Force -DisableNameChecking
`$T = Start-Test 'harness-windows-$Name' -Bin '$($T.Bin)' -Out '$(Join-Path $T.Out $Name)'
Invoke-Test {
$Body
} -Passed '$Name passed'
"@)
  $path
}

# A child test that opens its own window, then an LBTerm of class $Class that isn't one of its
# windows (its pid in <out>\<name>.pid), and closes that one after the watch has seen it
function Write-ExtraChild([string]$Name, [string]$Class) {
  Write-Child $Name @"
  `$w = Open-TestWindow -NoConfig -Program (New-PwshProgram)
  Wait-State { param(`$s) `$s.frame -gt 0 } -What 'the first frame' | Out-Null
  `$position = & (Get-Module harness) { `$TestWindowPosition }
  `$id = Start-Unfocused -Exe `$T.Gui -Arguments "-n --config lbterm_crash_restore=false --config wsl_domains={} start --always-new-process --position `$position --class $Class -- pwsh -NoProfile" -Job
  [IO.File]::WriteAllText('$(Join-Path $T.Out "$Name.pid")', "`$id")
  if ((Wait-Window -Id `$id -Seconds 20) -eq [IntPtr]::Zero) { throw 'the extra window never showed' }
  # The watch polls every 100 ms
  Start-Sleep -Milliseconds 500
  Stop-Process -Id `$id -Force -ErrorAction SilentlyContinue
"@
}

# Runs a child test: its exit code, what it printed, its result.json and its extra window's pid
function Invoke-Child([string]$Path, [string]$Name) {
  $printed = @(& 'C:\Program Files\PowerShell\7\pwsh.exe' -NoProfile -File $Path 2>&1 | ForEach-Object { "$_" })
  $code = $LASTEXITCODE
  $result = Get-Content (Join-Path $T.Out "$Name\result.json") -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json
  $printed | ForEach-Object { "  ${Name}: $_" }
  [pscustomobject]@{ Exit = $code; Result = $result; Extra = (Get-Content (Join-Path $T.Out "$Name.pid") -ErrorAction SilentlyContinue) }
}

$own = Write-Child 'own' @'
  $w = Open-TestWindow -NoConfig -Program (New-PwshProgram)
  Wait-State { param($s) $s.frame -gt 0 } -What 'the first frame' | Out-Null
  Start-Sleep -Milliseconds 500
'@
$noted = Write-ExtraChild 'noted' $notedClass
$stray = Write-ExtraChild 'stray' $strayClass

Invoke-Test {
  # 1. Its own window only: passes
  $r = Invoke-Child $own 'own'
  if ($r.Exit -ne 0 -or -not $r.Result.pass) { Add-Failure "a test with only its own window failed (exit $($r.Exit)): $(@($r.Result.failures) -join ' | ')" }
  else { 'own: passed, its window its own' }

  # 2. Another test's window: passes, noted
  $r = Invoke-Child $noted 'noted'
  $named = @($r.Result.notes | Where-Object { $_ -match "^another LBTerm test window appeared \(not counted\): lbterm-gui\.exe \(pid $($r.Extra)\b" })
  if (-not $r.Extra) { Add-Failure 'the noted child never started its second window' }
  elseif ($r.Exit -ne 0 -or -not $r.Result.pass) { Add-Failure "a test that saw another test's window failed (exit $($r.Exit)): $(@($r.Result.failures) -join ' | ')" }
  elseif (-not $named) { Add-Failure "the noted test passed, but no note names lbterm-gui.exe, pid $($r.Extra): $(@($r.Result.notes) -join ' | ')" }
  else { "noted: passed, with $($named[0])" }

  # 3. A window that isn't the test's nor any test's: fails, naming it
  $r = Invoke-Child $stray 'stray'
  $named = @($r.Result.failures | Where-Object { $_ -match "^a window that isn't the test's appeared: lbterm-gui\.exe \(pid $($r.Extra)\b" })
  if (-not $r.Extra) { Add-Failure 'the stray child never started its stray window' }
  elseif ($r.Exit -ne 1 -or $r.Result.pass) { Add-Failure "a test whose run saw a window not its own passed (exit $($r.Exit))" }
  elseif (-not $named) { Add-Failure "the stray test failed, but not naming lbterm-gui.exe, pid $($r.Extra): $(@($r.Result.failures) -join ' | ')" }
  else { "stray: failed as it should: $($named[0])" }
} -AllowWindow { param($w) $w.Class -like 'com.lachlanb.lbterm.*harness-windows*' } -Finally {
  foreach ($name in 'noted', 'stray') {
    $id = Get-Content (Join-Path $T.Out "$name.pid") -ErrorAction SilentlyContinue
    if ($id) { Stop-Process -Id ([int]$id) -Force -ErrorAction SilentlyContinue }
  }
  foreach ($c in $strayClass, $notedClass) { Remove-Item (Join-Path $env:APPDATA "lbterm\session-$c.json") -ErrorAction SilentlyContinue }
} -Passed 'harness windows: a test''s own window passes, another test''s is noted, any other window fails the test and is named'
