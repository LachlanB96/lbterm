#Requires -Version 7
# Helper: a sampling profiler for a test window's process (lbterm/PERF.md, "What printing costs"), run by hand beside a test or a budget scenario; not a test
<#
Where a process's threads spend their time, by sampling: waits for the lbterm-gui (or -Exe, a
mux server) whose command line names -Class (a test window's class) or takes -ProcessId, finds
its busiest threads over -PickSec (or takes -Tid), then for -Seconds suspends each -Hz times a
second, walks its stack (StackWalk64, on the x64 unwind data every function has) and resumes it.
Then it names every frame from the build's PDB through DbgHelp (the exe's folder unless -SymPath:
a copied build needs its .pdb beside it), demangles Rust's names, and says for each thread where
its samples were (self, the function at the top of the stack) and what they were under
(inclusive). A thread waiting (on a lock, a read, a message) is sampled too, in the wait: what
the thread does with its time, not only its CPU. Inlined functions show as the function they were
inlined into. Never touches a window: it reads and briefly suspends threads of a process of the
test's, which needs no elevation.

It refuses Lachlan's own LBTerm: a process run from the installed folder
(%LOCALAPPDATA%/Programs/LBTerm, his GUI and mux server), or an lbterm-gui started without a
class, as the harness starts every test window with one. Suspending his GUI's threads 1000 times
a second would stall his typing for the whole run. -Force samples it anyway; don't.

  pwsh -File lbterm\perf\sample.ps1 -Class perf-budgets-throughput-unicode-local -StartAfterSec 12 -Seconds 10 -Threads 3 -Out <file>
#>
param([string]$Class, [int]$ProcessId, [string]$Exe = 'lbterm-gui', [int[]]$Tid, [int]$Seconds = 10, [int]$Hz = 1000,
  [int]$Threads = 2, [double]$PickSec = 2, [string]$SymPath, [int]$Depth = 48, [int]$Top = 40, [string]$Out,
  [int]$WaitSec = 120, [double]$StartAfterSec = 0, [switch]$Force)

# Rust's legacy mangling (ZN <len><segment>... 17h<hash> E) as a path: `wezterm_term::terminalstate::
# performer::Performer::flush_print`, its escapes ($LT$, $u20$, ..) written out and the hash gone.
# Anything else comes back as it is
function ConvertFrom-RustSymbol([string]$Name) {
  $m = [regex]::Match($Name, '^_?ZN(?<body>.*)E(\.llvm\.\d+)?$')
  if (-not $m.Success) { return $Name }
  $body = $m.Groups['body'].Value
  $parts = [Collections.Generic.List[string]]::new()
  $i = 0
  while ($i -lt $body.Length) {
    $j = $i
    while ($j -lt $body.Length -and [char]::IsDigit($body[$j])) { $j++ }
    if ($j -eq $i) { return $Name }
    $len = [int]$body.Substring($i, $j - $i)
    if ($j + $len -gt $body.Length) { return $Name }
    $parts.Add($body.Substring($j, $len))
    $i = $j + $len
  }
  if ($parts.Count -gt 1 -and $parts[$parts.Count - 1] -match '^h[0-9a-f]{16}$') { $parts.RemoveAt($parts.Count - 1) }
  $escapes = [ordered]@{ '$LT$' = '<'; '$GT$' = '>'; '$RF$' = '&'; '$BP$' = '*'; '$C$' = ','; '$u20$' = ' '; '$u27$' = "'"; '$u5b$' = '['; '$u5d$' = ']'; '$u7b$' = '{'; '$u7d$' = '}'; '$u7e$' = '~'; '..' = '::' }
  $out = ($parts | ForEach-Object {
      $s = $_
      if ($s.StartsWith('_$')) { $s = $s.Substring(1) }
      foreach ($k in $escapes.Keys) { $s = $s.Replace($k, $escapes[$k]) }
      $s
    }) -join '::'
  $out
}

# A thread's samples (each its frames' names, the top first) said as two lists, most first: self,
# the share of samples a function was at the top in; inclusive, the share it was anywhere in
function Get-SampleReport([string]$Thread, [object[]]$Samples, [int]$Top = 40) {
  $n = @($Samples).Count
  $self = @{}; $incl = @{}
  foreach ($s in $Samples) {
    $names = @($s)
    if (-not $names.Count) { continue }
    $self[$names[0]] = 1 + [int]$self[$names[0]]
    $seen = @{}
    foreach ($nm in $names) { if (-not $seen.ContainsKey($nm)) { $seen[$nm] = 1; $incl[$nm] = 1 + [int]$incl[$nm] } }
  }
  $lines = [Collections.Generic.List[string]]::new()
  $lines.Add("== thread $Thread, $n samples")
  $lines.Add('-- self')
  foreach ($e in ($self.GetEnumerator() | Sort-Object { -$_.Value }, Key | Select-Object -First $Top)) { $lines.Add(('{0,6:N1}%  {1}' -f (100.0 * $e.Value / [Math]::Max(1, $n)), $e.Key)) }
  $lines.Add('-- inclusive')
  foreach ($e in ($incl.GetEnumerator() | Sort-Object { -$_.Value }, Key | Select-Object -First ($Top * 2))) { $lines.Add(('{0,6:N1}%  {1}' -f (100.0 * $e.Value / [Math]::Max(1, $n)), $e.Key)) }
  $lines
}

# Why a process mustn't be sampled, or nothing when it may: Lachlan's own LBTerm (its exe in
# -Installed, the installed folder) or an lbterm-gui without a class (each test window has one).
# -Path and -CommandLine are the process's (Win32_Process's ExecutablePath and CommandLine)
function Get-SampleRefusal([string]$Name, [string]$Path, [string]$CommandLine,
  [string]$Installed = (Join-Path $env:LOCALAPPDATA 'Programs\LBTerm')) {
  $norm = { param($p) ($p -replace '/', '\').TrimEnd('\') }
  if ($Path -and (& $norm $Path).StartsWith((& $norm $Installed) + '\', [StringComparison]::OrdinalIgnoreCase)) {
    return "$Name runs from the installed LBTerm ($Path): Lachlan's own, whose threads this would suspend $Hz times a second"
  }
  if ($Name -like 'lbterm-gui*' -and $CommandLine -notmatch '(^|\s)--class\s') {
    return "$Name has no --class in its command line, as every test window has: not a test's"
  }
}

# Dot-sourced (the tests), only the functions above
if ($MyInvocation.InvocationName -eq '.') { return }
$ErrorActionPreference = 'Stop'
if (-not ('LbtermSampler' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
public static class LbtermSampler {
  [DllImport("kernel32")] static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
  [DllImport("kernel32")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32")] static extern uint SuspendThread(IntPtr h);
  [DllImport("kernel32")] static extern uint ResumeThread(IntPtr h);
  [DllImport("kernel32")] static extern bool GetThreadContext(IntPtr h, IntPtr ctx);
  [DllImport("kernel32")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong cycles);
  [DllImport("kernel32", CharSet = CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr mod, string name);
  [DllImport("kernel32", CharSet = CharSet.Unicode)] static extern IntPtr LoadLibraryW(string name);
  [DllImport("winmm")] static extern uint timeBeginPeriod(uint p);
  [DllImport("winmm")] static extern uint timeEndPeriod(uint p);
  [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymInitializeW(IntPtr hProcess, string path, bool invade);
  [DllImport("dbghelp")] static extern bool SymCleanup(IntPtr hProcess);
  [DllImport("dbghelp")] static extern uint SymSetOptions(uint opts);
  [DllImport("dbghelp")] static extern bool StackWalk64(uint machine, IntPtr hProcess, IntPtr hThread, IntPtr frame, IntPtr ctx, IntPtr readMem, IntPtr funcTable, IntPtr modBase, IntPtr translate);
  [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymFromAddrW(IntPtr hProcess, ulong addr, out ulong disp, IntPtr symbol);
  [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymGetModuleInfoW64(IntPtr hProcess, ulong addr, IntPtr info);

  public static ulong Cycles(uint tid) {
    IntPtr h = OpenThread(0x0800, false, tid); // THREAD_QUERY_LIMITED_INFORMATION
    if (h == IntPtr.Zero) return 0;
    ulong c; QueryThreadCycleTime(h, out c); CloseHandle(h); return c;
  }

  // Samples: per thread, each sample's program counters, leaf first
  public static Dictionary<uint, List<ulong[]>> Run(uint pid, uint[] tids, int ms, int hz, string symPath, int depth, out IntPtr process) {
    process = OpenProcess(0x0410 | 0x0008, false, pid); // QUERY_INFORMATION | VM_READ | VM_OPERATION
    if (process == IntPtr.Zero) throw new Exception("OpenProcess failed: " + Marshal.GetLastWin32Error());
    SymSetOptions(0x2 | 0x4 | 0x10); // UNDNAME | DEFERRED_LOADS | LOAD_LINES
    if (!SymInitializeW(process, symPath, true)) throw new Exception("SymInitialize failed: " + Marshal.GetLastWin32Error());
    IntPtr dbg = LoadLibraryW("dbghelp.dll");
    IntPtr fta = GetProcAddress(dbg, "SymFunctionTableAccess64");
    IntPtr gmb = GetProcAddress(dbg, "SymGetModuleBase64");
    var handles = new IntPtr[tids.Length];
    for (int i = 0; i < tids.Length; i++) handles[i] = OpenThread(0x0002 | 0x0008 | 0x0040, false, tids[i]); // SUSPEND_RESUME | GET_CONTEXT | QUERY_INFORMATION
    var result = new Dictionary<uint, List<ulong[]>>();
    foreach (var t in tids) result[t] = new List<ulong[]>();
    IntPtr ctxRaw = Marshal.AllocHGlobal(1232 + 32);
    IntPtr ctx = new IntPtr((ctxRaw.ToInt64() + 15) & ~15L);
    IntPtr frame = Marshal.AllocHGlobal(512);
    var pcs = new List<ulong>(depth);
    timeBeginPeriod(1);
    var clock = Stopwatch.StartNew();
    long period = Stopwatch.Frequency / hz;
    long next = 0;
    try {
      while (clock.ElapsedMilliseconds < ms) {
        for (int i = 0; i < tids.Length; i++) {
          IntPtr h = handles[i];
          if (h == IntPtr.Zero) continue;
          if (SuspendThread(h) == 0xFFFFFFFF) continue;
          try {
            for (int b = 0; b < 1232; b += 8) Marshal.WriteInt64(ctx, b, 0);
            Marshal.WriteInt32(ctx, 0x30, 0x100003); // CONTEXT_AMD64 | CONTROL | INTEGER
            if (!GetThreadContext(h, ctx)) continue;
            ulong rip = (ulong)Marshal.ReadInt64(ctx, 0xF8), rsp = (ulong)Marshal.ReadInt64(ctx, 0x98), rbp = (ulong)Marshal.ReadInt64(ctx, 0xA0);
            for (int b = 0; b < 512; b += 8) Marshal.WriteInt64(frame, b, 0);
            Marshal.WriteInt64(frame, 0, (long)rip); Marshal.WriteInt32(frame, 12, 3);
            Marshal.WriteInt64(frame, 32, (long)rbp); Marshal.WriteInt32(frame, 44, 3);
            Marshal.WriteInt64(frame, 48, (long)rsp); Marshal.WriteInt32(frame, 60, 3);
            pcs.Clear();
            for (int d = 0; d < depth; d++) {
              if (!StackWalk64(0x8664, process, h, frame, ctx, IntPtr.Zero, fta, gmb, IntPtr.Zero)) break;
              ulong pc = (ulong)Marshal.ReadInt64(frame, 0);
              if (pc == 0) break;
              pcs.Add(pc);
            }
          } finally { ResumeThread(h); }
          if (pcs.Count > 0) result[tids[i]].Add(pcs.ToArray());
        }
        next += period;
        long wait = next - clock.ElapsedTicks;
        if (wait > 0) Thread.Sleep((int)Math.Max(1, wait * 1000 / Stopwatch.Frequency));
      }
    } finally {
      timeEndPeriod(1);
      foreach (var h in handles) if (h != IntPtr.Zero) CloseHandle(h);
      Marshal.FreeHGlobal(ctxRaw); Marshal.FreeHGlobal(frame);
    }
    return result;
  }

  static Dictionary<ulong, string> names = new Dictionary<ulong, string>();
  public static string Name(IntPtr process, ulong pc) {
    string n;
    if (names.TryGetValue(pc, out n)) return n;
    IntPtr sym = Marshal.AllocHGlobal(88 + 2 * 1024);
    try {
      for (int b = 0; b < 88; b += 4) Marshal.WriteInt32(sym, b, 0);
      Marshal.WriteInt32(sym, 0, 88); Marshal.WriteInt32(sym, 80, 1000);
      ulong disp;
      if (SymFromAddrW(process, pc, out disp, sym)) {
        int len = Marshal.ReadInt32(sym, 76);
        n = Marshal.PtrToStringUni(new IntPtr(sym.ToInt64() + 84), Math.Min(len, 1000));
      } else {
        IntPtr info = Marshal.AllocHGlobal(4096);
        try {
          for (int b = 0; b < 4096; b += 4) Marshal.WriteInt32(info, b, 0);
          Marshal.WriteInt32(info, 0, 3264); // IMAGEHLP_MODULEW64's size
          n = SymGetModuleInfoW64(process, pc, info) ? "(" + Marshal.PtrToStringUni(new IntPtr(info.ToInt64() + 44)) + ")" : "?";
        } finally { Marshal.FreeHGlobal(info); }
      }
    } finally { Marshal.FreeHGlobal(sym); }
    names[pc] = n;
    return n;
  }
  public static void Done(IntPtr process) { SymCleanup(process); CloseHandle(process); }
}
'@
}

function Find-Target {
  if ($ProcessId) { return Get-Process -Id $ProcessId }
  if (-not $Class) { throw 'sample.ps1 needs -Class <a test window''s class> or -ProcessId' }
  $deadline = (Get-Date).AddSeconds($WaitSec)
  while ((Get-Date) -lt $deadline) {
    foreach ($p in @(Get-Process $Exe -ErrorAction SilentlyContinue)) {
      $cmd = (Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue).CommandLine
      if ($cmd -and $cmd -match [regex]::Escape($Class)) { return $p }
    }
    Start-Sleep -Milliseconds 250
  }
  throw "no $Exe with $Class in its command line within $WaitSec s"
}
$p = Find-Target
$cim = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue
$refusal = Get-SampleRefusal -Name $p.Name -Path $cim.ExecutablePath -CommandLine $cim.CommandLine
if ($refusal -and -not $Force) { throw "sample.ps1 refuses: $refusal (-Force to sample it anyway)" }
"target: $($p.Name) $($p.Id)"
if ($StartAfterSec -gt 0) { Start-Sleep -Seconds $StartAfterSec }
$tids = if ($Tid) { @($Tid | ForEach-Object { [uint32]$_ }) } else {
  # The busiest threads over -PickSec, by their cycles
  $c0 = @{}; foreach ($t in $p.Threads) { $c0[[uint32]$t.Id] = [LbtermSampler]::Cycles([uint32]$t.Id) }
  Start-Sleep -Seconds $PickSec
  $p.Refresh()
  $busy = @($p.Threads | ForEach-Object { $id = [uint32]$_.Id; [pscustomobject]@{ Tid = $id; Cycles = ([LbtermSampler]::Cycles($id) - $(if ($c0.ContainsKey($id)) { $c0[$id] } else { 0 })) } } | Sort-Object Cycles -Descending | Select-Object -First $Threads)
  Write-Host "threads: $(($busy | ForEach-Object { "$($_.Tid) ($([math]::Round($_.Cycles / 3e6)) ms at 3 GHz)" }) -join ', ')"
  @($busy.Tid)
}
if (-not $SymPath) { $SymPath = Split-Path ($p.Path) }
$proc = [IntPtr]::Zero
$samples = [LbtermSampler]::Run([uint32]$p.Id, [uint32[]]$tids, $Seconds * 1000, $Hz, $SymPath, $Depth, [ref]$proc)
$report = [Collections.Generic.List[string]]::new()
try {
  foreach ($t in $samples.Keys) {
    $named = @(foreach ($s in $samples[$t]) { , @($s | ForEach-Object { ConvertFrom-RustSymbol ([LbtermSampler]::Name($proc, $_)) }) })
    foreach ($l in (Get-SampleReport -Thread "$t" -Samples $named -Top $Top)) { $report.Add($l) }
  }
} finally { [LbtermSampler]::Done($proc) }
$text = $report -join "`n"
if ($Out) { [IO.File]::WriteAllText($Out, $text + "`n") }
$text
