#Requires -Version 7
<#
Start-Unfocused: starts a GUI (a test LBTerm window, a demo) the way nothing an agent opens should
ever start: without a console window and without taking focus. Dot-source it, then

  $id = Start-Unfocused -Exe $gui -Arguments '--config-file x start --position 6860,120 -- pwsh' `
          -Environment @{ USERPROFILE = $sandbox } -WorkingDirectory $dir

returns the new process's id.

Why not `explorer.exe launch.cmd`, as the scripts here used to do to get out of Claude's sandboxed
job: Explorer runs a .cmd in a console, and on Windows 11 that console is a Windows Terminal window
that comes up in front, takes the keyboard for the second the script runs, and swallows whatever
Lachlan is typing. Win32_Process.Create starts the process from the WMI service instead: outside the
job, with no console of ours, and told to show without activating. The environment it gets is
exactly the one given, so this hands over the usual one: this shell's, minus what belongs to this
session's terminal and Claude (a test window must not think it runs inside them), plus
LBTERM_NO_ACTIVATE=1, which LBTerm reads to show its windows without activating them, and
-Environment on top.
#>
function Start-Unfocused {
  param(
    [Parameter(Mandatory)][string]$Exe,
    [string]$Arguments = '',
    [hashtable]$Environment = @{},
    [string]$WorkingDirectory,
    # A console program left running in the background (sccache's server): started detached, with
    # no console at all, since WMI would otherwise give it a console window of its own
    [switch]$NoConsole
  )
  $skip = '^(WEZTERM_|TERM_PROGRAM|CLAUDE|ANTHROPIC_|LBTERM_|TERM$|COLORTERM$|CLAUDECODE$)'
  $vars = [ordered]@{}
  foreach ($e in [Environment]::GetEnvironmentVariables().GetEnumerator()) {
    if ($e.Key -notmatch $skip) { $vars[$e.Key] = $e.Value }
  }
  $vars['LBTERM_NO_ACTIVATE'] = '1'
  foreach ($k in $Environment.Keys) { $vars[$k] = $Environment[$k] }
  $props = @{
    # SW_SHOWNOACTIVATE, for whatever looks at STARTUPINFO
    ShowWindow           = [uint16]4
    EnvironmentVariables = [string[]]@($vars.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" })
  }
  # SW_HIDE and DETACHED_PROCESS
  if ($NoConsole) { $props.ShowWindow = [uint16]0; $props.CreateFlags = [uint32]8 }
  $startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property $props
  $create = @{ CommandLine = "`"$Exe`" $Arguments"; ProcessStartupInformation = $startup }
  if ($WorkingDirectory) { $create.CurrentDirectory = $WorkingDirectory }
  $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments $create
  if ($result.ReturnValue -ne 0) { throw "Win32_Process.Create failed ($($result.ReturnValue)) for $Exe" }
  [int]$result.ProcessId
}

# Waits up to -Seconds for process -Id to show its window; its handle, or 0.
function Wait-Window {
  param([Parameter(Mandatory)][int]$Id, [double]$Seconds = 10)
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt $Seconds) {
    $p = Get-Process -Id $Id -ErrorAction SilentlyContinue
    if (-not $p) { return [IntPtr]::Zero }
    if ($p.MainWindowHandle -ne 0) { return $p.MainWindowHandle }
    Start-Sleep -Milliseconds 100
  }
  [IntPtr]::Zero
}
