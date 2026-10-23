#Requires -Version 7
# Covers: wezterm-gui/src/scripting/mod.rs
<#
Checks wezterm.gui.clipboard_has_image() against what Windows PowerShell's Get-Clipboard -Format
Image says about the same clipboard, in a test window, and times both. It only reads the clipboard,
so it tests whichever state the clipboard is in: copy an image first (Win+Shift+S) to test the
true side, some text to test the false side. -WithImage puts a small image on the clipboard first,
which replaces what's there.

  pwsh -File lbterm\perf\clipboard-image.ps1 [-WithImage]
#>
param([string]$Bin, [switch]$WithImage)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'clipboard-image' -Bin $Bin

if ($WithImage) {
  # Windows PowerShell for the clipboard's image API, as wezterm.lua's fallback uses it
  & powershell.exe -NoProfile -NonInteractive -Command @'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = New-Object System.Drawing.Bitmap 8, 8
[System.Windows.Forms.Clipboard]::SetImage($b)
'@
}

# The answer Get-Clipboard gives, and how long it takes, the way wezterm.lua asked before
$watch = [Diagnostics.Stopwatch]::StartNew()
$probe = & powershell.exe -NoProfile -NonInteractive -Command 'if (Get-Clipboard -Format Image) { "img" }'
$probeMs = $watch.ElapsedMilliseconds
$expected = [bool]($probe -match 'img')

$lua = Join-Path $T.Out 'config\clipboard-image.lua'
New-Item -ItemType Directory -Force (Split-Path $lua) | Out-Null
@'
local wezterm = require 'wezterm'
local started = os.clock()
local has = wezterm.gui.clipboard_has_image()
wezterm.log_warn(string.format('clipboard_has_image=%s in %.3f ms', tostring(has), (os.clock() - started) * 1000))
return { lbterm_crash_restore = false, automatically_reload_config = false, front_end = 'WebGpu' }
'@ | Set-Content -Path $lua -Encoding utf8NoBOM

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $lua -Program (New-PwshProgram)
  # The config logs its answer as it runs, before the window shows (was a fixed 1 s, then 1 s
  # after the window went)
  Wait-LogLine $w 'clipboard_has_image=' -TimeoutSec 10 | Out-Null
  Stop-TestWindow $w
  if (-not (Test-Path $w.Log)) { throw "no log for pid $($w.Pid)" }
  $line = Get-LogLines $w 'clipboard_has_image=(\w+) in ([\d.]+) ms' | Select-Object -First 1
  if (-not $line -or $line -notmatch 'clipboard_has_image=(\w+) in ([\d.]+) ms') { Add-Failure 'the test window never logged clipboard_has_image'; return }
  $got = $Matches[1] -eq 'true'
  "Get-Clipboard -Format Image: $expected in $probeMs ms"
  "clipboard_has_image():      $got in $($Matches[2]) ms"
  if ($got -ne $expected) { Add-Failure 'they disagree' }
} -Passed "clipboard-image: they agree ($(if ($expected) { 'an image' } else { 'no image' }) on the clipboard)"
