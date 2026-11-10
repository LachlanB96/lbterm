#Requires -Version 7
# Covers: lbterm-extras/src/mic.rs, lbterm-extras/src/control.rs, wezterm-gui/src/mic.rs, wezterm-gui/src/control.rs, wezterm-gui/src/termwindow/render/fancy_tab_bar.rs, wezterm-gui/src/termwindow/render/strip.rs, wezterm/src/control.rs
<#
The mic mark end to end (lbterm/CONTROL.md, "The mic mark"), through `lbterm control mic`, in a
test window that looks like Lachlan's LBTerm (New-TestConfig), whose strip shows it:

1. listening: the mark shows, dim (listening.png).
2. hearing: it's lit (hearing.png: the tab rail's active red on the strip's right).
3. What was heard shows as the strip's notice for a moment.
4. A state not said again goes once its ttl runs out, as a listener that stopped would leave it.
5. muted, then off: struck through, then gone. A state that isn't one is refused.

  pwsh -File lbterm\perf\mic.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'mic' -Bin $Bin

# The tab rail's active red (lbterm_panel::palette::ACTIVE, #FF5449), give or take 40 a channel
$red = New-ColourRange 215 255 44 124 33 113

function Invoke-Control {
  [CmdletBinding(PositionalBinding = $false)]
  param([Parameter(ValueFromRemainingArguments)][string[]]$Rest)
  $errFile = Join-Path $T.Out 'control-stderr.txt'
  $out = @(& $T.Cli control --class $T.Class @Rest 2>$errFile)
  $code = $LASTEXITCODE
  $text = ($out | Out-String).Trim()
  [pscustomobject]@{
    Code  = $code
    Reply = $(if ($text) { $text | ConvertFrom-Json } else { $null })
    Error = ((Get-Content $errFile -Raw -ErrorAction SilentlyContinue) ?? '').Trim()
  }
}

Invoke-Test {
  $w = Open-TestWindow -Program (New-PwshProgram) -Lua @"
config.lbterm_prompt_history = nil
config.initial_cols = 100
config.initial_rows = 24
"@
  Wait-State { param($s) $s.frame -gt 0 -and $null -ne $s.strip } -What 'the window and its strip' | Out-Null
  $rest = Get-Shot $w -Name 'rest'

  # 1. Listening
  $r = Invoke-Control mic listening
  if ($r.Code -ne 0 -or $r.Reply.mic -ne 'listening') { Add-Failure "mic listening: exit $($r.Code), $($r.Reply | ConvertTo-Json -Compress) $($r.Error)" }
  Wait-Change { param($s) $s.strip.mic -eq 'listening' } -What 'the mark, listening' | Out-Null
  'listening: the mark shows'
  Get-Shot $w -Name 'listening' | Out-Null

  # 2. Hearing: lit red on the strip
  Invoke-Control mic hearing | Out-Null
  Wait-Change { param($s) $s.strip.mic -eq 'hearing' } -What 'the mark, hearing' | Out-Null
  $shot = Get-Shot $w -Name 'hearing'
  $strip = New-Rect ([int]($shot.Width / 2)) 0 ([int]($shot.Width / 2)) 40
  $lit = [LbtermE2E.Px]::CountRange($shot, $strip, $red)
  $before = [LbtermE2E.Px]::CountRange($rest, $strip, $red)
  if ($lit -le $before + 4) { Add-Failure "hearing: $lit red pixels on the strip's right, $before at rest" }
  else { "hearing: lit ($lit red pixels on the strip's right, $before at rest)" }

  # 3. What was heard, on the strip
  $r = Invoke-Control mic listening --heard 'typed 1a'
  if ($r.Code -ne 0) { Add-Failure "mic listening --heard: exit $($r.Code), '$($r.Error)'" }
  try {
    # Not Wait-Change: a notice is a wait under way, so the window doesn't settle while it shows
    Wait-State { param($s) $s.strip.notice -like '*typed 1a*' -and $s.strip.mic -eq 'listening' } -What 'the notice of what was heard' | Out-Null
    'heard: said on the strip'
  } catch {
    $s = Get-StateWindow (Read-State)
    Add-Failure "$($_.Exception.Message); the strip: $($s.strip | ConvertTo-Json -Compress -Depth 3)"
  }

  # 4. Not said again: gone after its ttl
  # The notice above may still be up, so these wait on the state alone, not on settling
  Invoke-Control mic hearing --ttl 1 | Out-Null
  Wait-State { param($s) $s.strip.mic -eq 'hearing' } -What 'hearing, for a second' | Out-Null
  try { Wait-State { param($s) $null -eq $s.strip.mic } -What 'the mark gone once its ttl ran out' -TimeoutSec 4 | Out-Null; 'ttl: gone a second on' }
  catch { Add-Failure $_.Exception.Message }

  # 5. Muted, off, and a state that isn't one
  Invoke-Control mic muted | Out-Null
  Wait-Change { param($s) $s.strip.mic -eq 'muted' } -What 'the mark, muted' | Out-Null
  $r = Invoke-Control mic off
  if ($r.Code -ne 0 -or $null -ne $r.Reply.mic) { Add-Failure "mic off: $($r.Reply | ConvertTo-Json -Compress)" }
  Wait-Change { param($s) $null -eq $s.strip.mic } -What 'the mark gone' | Out-Null
  $r = Invoke-Control mic loud
  if ($r.Code -ne 1 -or $r.Error -notmatch 'not "loud"') { Add-Failure "mic loud: exit $($r.Code), '$($r.Error)'" }
  else { 'muted, off: shown, then gone; loud refused' }
} -Passed 'mic: listening, hearing lit, what was heard on the strip, gone after its ttl, muted and off'
