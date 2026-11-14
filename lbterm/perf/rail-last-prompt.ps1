#Requires -Version 7
# Covers: lbterm-panel/src/**, lbterm-extras/src/summaries.rs, wezterm-gui/src/summaries.rs, wezterm-gui/src/termwindow/render/tab_panel.rs, wezterm-gui/src/conductor_hub.rs, lbterm/motion/demo-tab.ps1
<#
How long ago Lachlan last prompted each tab's Claude session, at the start of the open tab panel's
second line, before the project (Lachlan's 1c of 2026-11-14; lbterm/TAB-PANEL.md, "What a row says", "Since his last prompt"), end to end. A test
window looking like Lachlan's LBTerm (New-TestConfig), lbterm_summaries pointed at a scratch folder
(<out>\summaries), gets four tabs: a working session prompted three minutes ago, a question
prompted two hours ago, a Claude session with no prompt.json, and a shell. Each prompt.json is
written whole through a rename, as the statusline was asked to write it: { "at": <Unix ms> }. A hub
played here on a pipe of its own (Start-PlayedPipe, as estimates.ps1 plays one) guesses the
working session has four minutes left, so its row shows both sides Lachlan asked for: the time
since his last prompt on the left, the Conductor's "~4 min left" on the right.

1. Closed: rail-last-prompt.png. The state file's rows say "3 min", "2 h", "" and "" (`prompted`):
   the rows' words, which the closed rail doesn't draw; the first row's `left` says "~4 min".
2. Still closed, the question's prompt.json is written again, thirty minutes ago: the window must
   paint nothing for it in the next 1.5 s (its `frame` holds still), since only the open panel
   shows the time: it wakes as a cost does (`lbterm_shows_usage`), and the context pill shows the
   active tab's session, the working one, not the question's.
3. Open, the pointer posted onto the rail: rail-last-prompt-open.png. The rows say "3 min",
   "30 min", "" and "", the first still "~4 min" left.
4. Open, the working session's prompt.json is written again, now: its row must say "just now",
   with nothing done to the window (rail-last-prompt-new.png, taken whether or not it does, so a
   build without the time makes its pair for look too).

Its USERPROFILE is a scratch folder, so lbterm.lua's handlers write nothing into the real
~\.claude. Fails with exit 1.

  pwsh -File lbterm\perf\rail-last-prompt.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'rail-last-prompt' -Bin $Bin
$demoTab = Join-Path $T.Repo 'lbterm\motion\demo-tab.ps1'
$summaries = Join-Path $T.Out 'summaries'
$home2 = Join-Path $T.Out 'home'
New-Item -ItemType Directory -Force $summaries, $home2 | Out-Null

$sessions = @(
  # A whole 3 minutes: text::since counts down, so it reads "3 min" for the next 60 s, well past step 3
  @{ sid = '44444444-4444-4444-8444-444444444444'; label = 'Deploy'; state = 'working'; ago = 180 }
  @{ sid = '55555555-5555-4555-8555-555555555555'; label = 'Lodestone'; state = 'waiting'; age = 3; ago = 2 * 3600 + 600 }
  @{ sid = '66666666-6666-4666-8666-666666666666'; label = 'No file yet'; state = 'working' }
)
# A session's prompt.json as the statusline was asked to write it: whole, through a rename, `seconds` ago
function Write-Prompt([string]$Sid, [double]$Seconds) {
  $at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - [long]($Seconds * 1000)
  # Write-FileWhole tries the rename again while Windows refuses it, as it does while the window's
  # summaries thread has the file open to read it
  Write-FileWhole -Path (Join-Path $summaries "$Sid.prompt.json") -Text "{ `"at`": $at }"
}
foreach ($s in $sessions) {
  [IO.File]::WriteAllText((Join-Path $summaries "$($s.sid).label.txt"), $s.label)
  if ($s.ago) { Write-Prompt $s.sid $s.ago }
}
function Demo([string]$Title, [string]$State = 'none', [double]$Age = 0, [string]$Session) {
  $a = @('-Title', $Title, '-State', $State, '-AgeMinutes', $Age)
  if ($Session) { $a += @('-Session', $Session) }
  New-PwshProgram -File $demoTab -ArgumentList $a
}
function Said($state) { @($state.rail.rows | ForEach-Object { "$($_.prompted)" }) -join '|' }
# The Conductor's hub, played: its guess for the working session, 40% with four minutes left, good
# for five minutes (conductor.estimates' `until`)
$hub = Start-PlayedPipe -Name "lbterm-perf-rail-last-prompt-hub-$PID" -Subscribe '"lbterm.subscribe"'
$guess = '{"type":"conductor.estimates","until":' + ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() + 300000) +
  ',"sessions":[{"sessionId":"' + $sessions[0].sid + '","percent":40,"left":240,"agents":[]}]}'
$lua = "rawset(config, 'lbterm_conductor', { state_file = '$(ConvertTo-LuaPath (Join-Path $T.Out 'button.json'))', hub_pipe = [[\\.\pipe\$($hub.Name)]], menu = {} })"

Invoke-Test {
  $first = $sessions[0]
  # LBTERM_POSTED_POINTER: a posted move hovers, so the panel opens under the moves in step 3
  $w = Open-TestWindow -UserProfile $home2 -PostedPointer -WorkingDirectory $T.Repo -Lua $lua `
    -Set 'lbterm_compositor=true', 'lbterm_tab_panel={enabled=true,open_delay_ms=0}', "lbterm_summaries='$(ConvertTo-LuaPath $summaries)'" `
    -Program (Demo one $first.state -Session $first.sid)
  Wait-State { param($s) $s.frame -gt 0 -and $s.rail } -What 'the window and its rail' | Out-Null
  foreach ($s in $sessions | Select-Object -Skip 1) {
    Invoke-Cli spawn --window-id 0 -Program (Demo two $s.state $s.age $s.sid) | Out-Null
  }
  Invoke-Cli spawn --window-id 0 -Program (Demo shell) | Out-Null
  Invoke-Cli activate-tab --pane-id 0 --tab-index 0 | Out-Null
  if (-not (Wait-Until { $hub.Subscribers -ge 1 } -TimeoutSec 20)) { throw "LBTerm never subscribed to the played hub: $($hub.Heard() -join ' | ')" }
  if ((Send-PlayedLine $hub $guess) -lt 1) { throw 'no subscriber to push the guess to' }
  # The labels read, and with them each session's prompt.json (one read of all its files); the guess
  $ready = Wait-Change {
    param($s)
    @($s.rail.rows).Count -eq 4 -and @($s.rail.rows | Where-Object label -eq 'Lodestone').Count -and @($s.rail.rows)[0].left -eq '~4 min'
  } -What 'four rows, the labels read, the first with its guess' -Animating

  # 1. Closed: the rail as it was, the times only in the rows' words. Captured before anything is
  # judged, so look has its pair from a build without them too
  Get-Shot $w -Name 'rail-last-prompt' | ForEach-Object Dispose
  $want = '3 min|2 h||'
  "closed: $(Said $ready)"
  if ((Said $ready) -ne $want) { Add-Failure "closed, the rows say '$(Said $ready)', not '$want'" }

  # 2. Closed, a prompt.json written again: nothing paints for it
  $before = (Read-State).frame
  Write-Prompt $sessions[1].sid 1800
  Start-Sleep -Milliseconds 1500
  $after = Read-State
  "closed, the question's prompt.json written again: frame $before, then $($after.frame)"
  if ($after.frame -ne $before) { Add-Failure "the closed rail painted $($after.frame - $before) frames for a prompt.json it doesn't show" }

  # 3. Open: posted moves onto the rail, every frame, as rail-labels.ps1 does
  $px = { param($v) ConvertTo-Px $v $w.Scale }
  Set-PointerHold $w (& $px 40) (& $px 160)
  $opened = Wait-Change { param($s) $s.rail.open } -What 'the panel open' -Animating
  Get-Shot $w -Name 'rail-last-prompt-open' | ForEach-Object Dispose
  $want = '3 min|30 min||'
  $left = @($opened.rail.rows)[0].left
  "open: $(Said $opened); the first row's guess '$left' left"
  if ((Said $opened) -ne $want) { Add-Failure "open, the rows say '$(Said $opened)', not '$want'" }
  if ($left -ne '~4 min') { Add-Failure "open, the first row lost its guess: '$left' left" }

  # 4. Open, the working session prompted again: its row says so, nothing done to the window
  Write-Prompt $first.sid 0
  try {
    $new = Wait-Change { param($s) @($s.rail.rows)[0].prompted -eq 'just now' } -What 'the first row saying just now' -Animating
    "after its prompt.json was written again: $(Said $new)"
  } catch { Add-Failure "$_" }
  # Taken whether or not it moved on, so look has this pair from a build without the time too
  Get-Shot $w -Name 'rail-last-prompt-new' | ForEach-Object Dispose
  Clear-PointerHold
} -Passed 'rail-last-prompt: how long ago each session was last prompted, from its prompt.json, at the start of the open row''s second line with the Conductor''s guess at the first''s end; none for a shell or a session without one; a closed rail paints nothing for it'
