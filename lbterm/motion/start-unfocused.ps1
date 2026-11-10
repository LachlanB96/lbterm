#Requires -Version 7
<#
Start-Unfocused: starts a GUI (a test LBTerm window, a demo) the way nothing an agent opens should
ever start: without a console window and without taking focus. Dot-source it, then

  $id = Start-Unfocused -Exe $gui -Arguments '--config-file x start --position 6860,120 -- pwsh' `
          -Environment @{ USERPROFILE = $sandbox } -WorkingDirectory $dir -Job

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

Outside the job means outside every job, the one a queue holds a test run to included: a test
window and all it starts (its mux server, its panes, the programs printing in them) ran on every CPU
at normal priority while the run that opened it was held to four (two runs at once took about 19
cores' worth, 2026-11-01). -Job is for what should end with the job it's started in, a test window:
on a PC whose dev settings say how to hand a process to its job (Get-JobHandoff, below) it starts
suspended, is handed over, and runs only once the job has it, so nothing it starts gets out first.
Not for a server other runs share (sccache's) or a window left open for Lachlan (lbterm.ps1 try).
The handoff takes 0.5 to 2 s, all before the process runs, so its creation time (Get-Process's
StartTime) is that much before it began: a test timing a start times it from -PassThru's Ran. A
test that needs several started together (two GUIs racing for one server) starts each -Held and
lets them all run at once with Resume-StartedProcess.
#>
function Start-Unfocused {
  param(
    [Parameter(Mandatory)][string]$Exe,
    [string]$Arguments = '',
    [hashtable]$Environment = @{},
    [string]$WorkingDirectory,
    # A console program left running in the background (sccache's server): started detached, with
    # no console at all, since WMI would otherwise give it a console window of its own
    [switch]$NoConsole,
    # A test window, which ends with the job it's started in (above)
    [switch]$Job,
    # Left suspended (once the job has it, with -Job) for the caller to let run with
    # Resume-StartedProcess, with others started the same way
    [switch]$Held,
    # Returns the id and when the process was let run (Ran: its resume, or its creation when it
    # wasn't suspended; a held one's is $null, Resume-StartedProcess's to say)
    [switch]$PassThru
  )
  # Before anything starts: outside a job, on a PC that runs test windows only inside one, it refuses
  $handoff = if ($Job) { Get-JobHandoff }
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
  $flags = 0
  # SW_HIDE and DETACHED_PROCESS
  if ($NoConsole) { $props.ShowWindow = [uint16]0; $flags = $flags -bor 8 }
  # CREATE_SUSPENDED, until the job has it or the caller lets it run
  if ($handoff -or $Held) { $flags = $flags -bor 4 }
  # WMI refuses CreateFlags 0, so it's given only with a flag in it
  if ($flags) { $props.CreateFlags = [uint32]$flags }
  $startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property $props
  $create = @{ CommandLine = "`"$Exe`" $Arguments"; ProcessStartupInformation = $startup }
  if ($WorkingDirectory) { $create.CurrentDirectory = $WorkingDirectory }
  $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments $create
  if ($result.ReturnValue -ne 0) { throw "Win32_Process.Create failed ($($result.ReturnValue)) for $Exe" }
  $id = [int]$result.ProcessId
  if ($handoff) { Join-StartedJob -Id $id -Handoff $handoff -Exe $Exe }
  $ran = if ($Held) { $null }
  elseif ($handoff) { Resume-StartedProcess $id }
  elseif ($PassThru) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($p) { $p.StartTime } else { throw "$Exe (pid $id) ended as it started" }
  }
  if ($PassThru) { [pscustomobject]@{ Id = $id; Ran = $ran } } else { $id }
}

# How -Job hands a process to the job this runs in: the command, its words with {pid} for the
# process's id, from the PC's dev settings (~\.config\lbterm\dev.json, or LBTERM_DEV's file, which
# never leave it; lbterm-lib.ps1's Get-JobRefusal reads the same "job"):
#   "job": { "env": "WARDEN_JOB", "adopt": ["warden-run", "adopt", "{pid}"], ... }
# `env` is the variable the queue gives every process it starts (Start-Unfocused passes it on, so
# the job can tell the process is its own); `adopt` exits 0 once the job has the process. Nothing
# without the file, "job" or "adopt": a PC that doesn't hold test runs to a job. With them, outside
# a job ($env not set) it refuses, since this PC opens test windows only inside one.
function Get-JobHandoff([string]$File = ($env:LBTERM_DEV ? $env:LBTERM_DEV : (Join-Path $HOME '.config\lbterm\dev.json'))) {
  if (-not (Test-Path -LiteralPath $File)) { return $null }
  $job = (Get-Content -Raw -LiteralPath $File | ConvertFrom-Json).job
  if (-not $job -or -not $job.adopt) { return $null }
  if (-not $job.env) { throw "$File's job has an adopt command but no env: the variable that says a process is in a job" }
  if (-not [Environment]::GetEnvironmentVariable([string]$job.env)) {
    throw "this PC opens test windows only inside a job, and $($job.env) isn't set here: start the run inside one ($File's job.run is the line)"
  }
  [string[]]@($job.adopt)
}

# Hands suspended process $Id to the job ($Handoff, Get-JobHandoff's). One the job won't take is
# ended before it ran at all, and this throws with what the command said.
function Join-StartedJob([Parameter(Mandatory)][int]$Id, [Parameter(Mandatory)][string[]]$Handoff, [string]$Exe) {
  $words = @($Handoff | ForEach-Object { $_.Replace('{pid}', "$Id") })
  $said = ''
  $code = -1
  try {
    $said = (& $words[0] @($words | Select-Object -Skip 1) 2>&1 | ForEach-Object { "$_" }) -join "`n"
    $code = $LASTEXITCODE
  }
  catch { $said = "$_" }
  if ($code -ne 0) {
    Stop-Process -Id $Id -Force -ErrorAction SilentlyContinue
    throw "the job didn't take $Exe (pid $Id, ended before it ran; $($words -join ' ') exit $code): $said"
  }
}

# Lets a process started suspended run (NtResumeProcess, which resumes every thread it has), and
# says when: the moment it could begin
function Resume-StartedProcess([Parameter(Mandatory)][int]$Id) {
  if (-not ('LbtermStartUnfocused.Resume' -as [type])) {
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
namespace LbtermStartUnfocused {
  public static class Resume {
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("ntdll.dll")] static extern int NtResumeProcess(IntPtr h);
    // PROCESS_SUSPEND_RESUME; 0 when it's resumed, else what failed
    public static string Process(int pid) {
      IntPtr h = OpenProcess(0x0800, false, pid);
      if (h == IntPtr.Zero) return "OpenProcess failed (" + Marshal.GetLastWin32Error() + ")";
      try { int s = NtResumeProcess(h); return s == 0 ? null : "NtResumeProcess 0x" + s.ToString("X8"); }
      finally { CloseHandle(h); }
    }
  }
}
'@
  }
  $failed = [LbtermStartUnfocused.Resume]::Process($Id)
  $ran = Get-Date
  if ($failed) { throw "pid $Id was started suspended and couldn't be resumed: $failed" }
  $ran
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
