#Requires -Version 7
# Covers: term/src/terminalstate/mod.rs, term/src/terminalstate/performer.rs
<#
A shell's prompt turns off the mouse and focus reporting a dead program left on (LBTerm's
TerminalState::release_mouse_and_focus). Without it, a Claude Code killed mid-session leaves its
tab's pwsh with every mouse move typed onto its prompt as an escape code.

A test window (no config, on the right-most screen, without focus) runs a pwsh script that turns
on what a TUI asks for (any-event mouse in SGR, focus reports) and counts what arrives as input
for a few seconds while this script turns the mouse wheel over the pane (posted WM_MOUSEWHEELs:
a window without focus takes no other mouse event to a pane, and these test windows never have
it). Then it prints the prompt
mark (OSC 133 A), as Terminal's pwsh profile does at each prompt, and counts again under the same
turns: nothing should arrive the second time. Fails with exit 1.

  pwsh -File lbterm\perf\prompt-mouse.ps1
#>
param([string]$Bin)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'prompt-mouse' -Bin $Bin
$report = Join-Path $T.Out 'report.txt'
$go = Join-Path $T.Out 'go.txt'

# In the pane: the modes on, input counted; then the prompt mark, input counted again. Each count
# waits for this script to say the moves have started (go.txt), and the next phase for it to say
# they've stopped
$script = @"
`$e = [char]27
# VT input, as PSReadLine asks for: ConPTY then passes mouse reports through as typed characters
# (without it they become mouse records, which ReadKey never sees)
Add-Type -Namespace W -Name C -MemberDefinition '[DllImport("kernel32.dll")] public static extern System.IntPtr GetStdHandle(int h); [DllImport("kernel32.dll")] public static extern bool GetConsoleMode(System.IntPtr h, out uint m); [DllImport("kernel32.dll")] public static extern bool SetConsoleMode(System.IntPtr h, uint m);'
`$in = [W.C]::GetStdHandle(-10); `$mode = 0; [void][W.C]::GetConsoleMode(`$in, [ref]`$mode)
[void][W.C]::SetConsoleMode(`$in, `$mode -bor 0x200)
function Count([string]`$phase) {
  while (-not (Test-Path '$go')) { Start-Sleep -Milliseconds 50 }
  `$n = 0; `$deadline = (Get-Date).AddSeconds(3)
  while ((Get-Date) -lt `$deadline) {
    while ([Console]::KeyAvailable) { [void][Console]::ReadKey(`$true); `$n++ }
    Start-Sleep -Milliseconds 20
  }
  Add-Content '$report' "`$phase `$n"
  while (Test-Path '$go') { Start-Sleep -Milliseconds 50 }
  while ([Console]::KeyAvailable) { [void][Console]::ReadKey(`$true) }
}
[Console]::Write("`$e[?1000h`$e[?1002h`$e[?1003h`$e[?1006h`$e[?1004h")
Count 'on'
[Console]::Write("`$e]133;A`$e\PS> ")
Count 'prompt'
Start-Sleep 60
"@

# The wheel turned over the pane, a notch up then down, for 3.5 s while the pane counts
function Move-Over($w) {
  New-Item -ItemType File $go | Out-Null
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $i = 0
  while ($clock.ElapsedMilliseconds -lt 3500) {
    Send-Wheel $w 200 200 -Delta $(if ($i % 2) { -120 } else { 120 })
    $i++
    Start-Sleep -Milliseconds 60
  }
  Remove-Item $go
}

# The pane with the keyboard, in a window's state
function Get-ActivePane($x) { @($x.panes | Where-Object active)[0] }

Invoke-Test {
  $w = Open-TestWindow -NoConfig -Set "front_end='WebGpu'" -Program (New-PwshProgram $script)
  # The pane's script has turned on what a TUI asks for
  Wait-State { param($x) (Get-ActivePane $x).mouse_reporting } -What 'mouse reporting on in the pane' | Out-Null
  Move-Over $w
  # The pane has counted, and printed the prompt mark and its "PS> " (the mark itself is what
  # turns reporting off, which the second count tests)
  Wait-State { param($x) (Get-ActivePane $x).cursor.x -ge 4 } -What 'the prompt after the mark' | Out-Null
  Move-Over $w
  Wait-Until { @(Get-Content $report -ErrorAction SilentlyContinue).Count -ge 2 } -TimeoutSec 10 -IntervalMs 200 | Out-Null
  $counts = @{}
  foreach ($line in @(Get-Content $report -ErrorAction SilentlyContinue)) { $k, $v = $line -split ' '; $counts[$k] = [int]$v }
  "mouse reporting on: $($counts['on']) characters arrived from the wheel; after the prompt mark: $($counts['prompt'])"
  if (-not $counts.ContainsKey('on') -or $counts['on'] -eq 0) { Add-Failure 'no mouse reports arrived with the modes on, so the test proves nothing' }
  if (-not $counts.ContainsKey('prompt')) { Add-Failure 'the pane never counted after the prompt mark' }
  elseif ($counts['prompt'] -ne 0) { Add-Failure "$($counts['prompt']) characters still arrived after the prompt mark" }
} -Passed 'prompt-mouse: a prompt turns off the mouse reporting a program left on'
