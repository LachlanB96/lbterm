#Requires -Version 7
# Covers: wezterm-gui/src/let_go.rs, wezterm-gui/src/inbox.rs, lbterm-extras/src/let_go.rs, lbterm-extras/src/watch.rs, config/src/lbterm.rs
<#
Letting go of a folder on request (lbterm/LET-GO.md) end to end, in a test window that looks like
Lachlan's LBTerm (New-TestConfig), its `lbterm_inbox` the InMail folder of a project of its own,
target\perf\let-go\projects\One, and its `lbterm_let_go` a folder of requests of its own beside it.
Windows won't rename a folder while a handle inside it is open, and the InMail watch holds one.

1. With the window watching One's InMail (its message counted on the button), renaming One is
   refused.
2. A request to let go of One written into the requests' folder (through a rename, as a program
   writes one): One can be renamed, and how long that took from the request's landing is said. It's
   renamed back while the request holds.
3. The request removed: the watch is back in One's InMail (a message written there is counted
   within 5 s, which a watch from above it wouldn't see), and renaming One is refused again.
4. A request that runs out (until 1.5 s on): One can be renamed while it holds; once its time has
   come, with the file still there, the watch is back (a message counted) and a rename is refused.

  pwsh -File lbterm\perf\let-go.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'let-go' -Bin $Bin
$projects = Join-Path $T.Out 'projects'
$requests = Join-Path $T.Out 'let-go'
Remove-Item -Recurse -Force $projects, $requests -ErrorAction SilentlyContinue
$one = Join-Path $projects 'One'
$inbox = Join-Path $one 'InMail'
$renamed = Join-Path $projects 'Uno'
New-Item -ItemType Directory -Force $inbox, $requests | Out-Null

$sent = 0
function Send-Message {
  $script:sent++
  $file = Join-Path $inbox ("2026-11-03-2300-from-Two-message-{0}.md" -f $script:sent)
  [IO.File]::WriteAllText($file, "---`nfrom: Two`nto: One`nsent: 2026-11-03T23:00`nsubject: `"Message $($script:sent)`"`n---`n")
}
Send-Message

# A request, written as a program writes one: a temporary file renamed into place
function Write-Request([string]$Name, [long]$UntilMs) {
  $file = Join-Path $requests "$Name.json"
  $json = @{ folder = $one; until = $UntilMs; by = 'let-go.ps1' } | ConvertTo-Json -Compress
  Write-FileWhole -Path $file -Text $json -Temp "$file.$PID.tmp"
  $file
}
function Get-NowMs { [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }

# Tries to rename One to Uno every 10 ms until it goes through or $Seconds pass: the time it took,
# or $null when it never did
function Measure-Rename([double]$Seconds) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
    try { [IO.Directory]::Move($one, $renamed); return $clock.Elapsed.TotalMilliseconds }
    catch [IO.IOException] { Start-Sleep -Milliseconds 10 }
    catch [UnauthorizedAccessException] { Start-Sleep -Milliseconds 10 }
  }
  $null
}
# One rename tried: whether it went through (and then One is put back)
function Test-Renames {
  try { [IO.Directory]::Move($one, $renamed) } catch { return $false }
  [IO.Directory]::Move($renamed, $one)
  $true
}

Invoke-Test {
  $w = Open-TestWindow -Lua @"
rawset(config, 'lbterm_inbox', { folders = { [[$inbox]] } })
rawset(config, 'lbterm_let_go', { folder = [[$requests]] })
"@
  # 1. Watched: refused
  Wait-Change { param($s) $s.strip.inbox -eq 1 } -What 'the message counted' | Out-Null
  if (Test-Renames) { Add-Failure 'One was renamed with the window watching its InMail: the test shows nothing (is the inbox on?)' }
  else { 'watched: the rename refused, as before' }

  # 2. Let go of: renamed, timed from the request's landing
  $request = Write-Request 'rename' ((Get-NowMs) + 60000)
  $took = Measure-Rename 5
  if ($null -eq $took) { Add-Failure 'One still not renamed 5 s after the request to let go of it' }
  else {
    'let go of: renamed {0:N0} ms after the request landed' -f $took
    if ($took -gt 1000) { Add-Failure ('the rename went through {0:N0} ms after the request, past 1 s' -f $took) }
    [IO.Directory]::Move($renamed, $one)
  }

  # 3. Taken back: a message in its InMail counted, and the rename refused again
  Remove-Item -LiteralPath $request
  Start-Sleep -Milliseconds 300
  Send-Message
  Wait-Change { param($s) $s.strip.inbox -eq 2 } -What 'a message counted once the request went' -TimeoutSec 5 | Out-Null
  if (Test-Renames) { Add-Failure 'One renamed after its request was removed: the watch never came back' }
  else { 'taken back: the watch is in its InMail again (a message counted), the rename refused' }

  # 4. A request that runs out
  $request = Write-Request 'short' ((Get-NowMs) + 1500)
  $took = Measure-Rename 1.2
  if ($null -eq $took) { Add-Failure 'One not renamed while a request with time left held' }
  else { [IO.Directory]::Move($renamed, $one) }
  Start-Sleep -Milliseconds 1700
  Send-Message
  Wait-Change { param($s) $s.strip.inbox -eq 3 } -What 'a message counted once the request ran out' -TimeoutSec 5 | Out-Null
  if (Test-Renames) { Add-Failure "One renamed after its request's time came: a request that runs out was held past it" }
  else { "ran out: the watch is back with the request's file still there, the rename refused" }
  Remove-Item -LiteralPath $request
} -Passed 'let-go passed: a watched project refused a rename, went through once let go of, and was watched again once the request went or ran out'
