#Requires -Version 7
# Helper: what a span cost a process, thread by thread, for at-rest.ps1 and conversation-cost.ps1
<#
Dot-sourced by the perf scripts that judge what LBTerm costs (lbterm/AT-REST.md, "Checking it"):
[LbtermRest]::Take(<pid>) snapshots every thread of a process (its context switches, each a wake,
its cycles and its name or the module it started in) and the process's I/O operations and handles,
from one NtQuerySystemInformation; Measure-Span subtracts two of them, Format-Span says one.

  . (Join-Path $PSScriptRoot 'threads.ps1')
#>

Add-Type -ErrorAction SilentlyContinue -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class LbtermRest {
  [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int cls, IntPtr buf, int len, out int ret);
  [DllImport("ntdll.dll")] static extern int NtQueryInformationThread(IntPtr h, int cls, out IntPtr info, int len, IntPtr ret);
  [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool QueryThreadCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern bool QueryProcessCycleTime(IntPtr h, out ulong c);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentThread();
  [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr h, out IntPtr desc);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
  const uint THREAD_QUERY_INFORMATION = 0x0040, THREAD_QUERY_LIMITED = 0x0800, PROCESS_QUERY_LIMITED = 0x1000;

  public sealed class Thread { public int Id; public long Switches; public ulong Cycles; public string Name; public string Module; }
  public sealed class Snap {
    public int Pid; public long Reads, Writes, Others, ReadBytes, WriteBytes, OtherBytes, Handles; public ulong Cycles;
    public Dictionary<int, Thread> Threads = new Dictionary<int, Thread>();
  }

  // Cycles a second, spinning on this thread for ms: the cycle counter's rate
  public static double Rate(int ms) {
    ulong a, b; var sw = Stopwatch.StartNew();
    QueryThreadCycleTime(GetCurrentThread(), out a);
    while (sw.ElapsedMilliseconds < ms) { }
    QueryThreadCycleTime(GetCurrentThread(), out b);
    return (b - a) / sw.Elapsed.TotalSeconds;
  }

  // The module a thread's start address is in
  static string Module(IntPtr h, List<Tuple<long, long, string>> modules) {
    IntPtr start;
    if (NtQueryInformationThread(h, 9, out start, IntPtr.Size, IntPtr.Zero) == 0) {
      long a = start.ToInt64();
      foreach (var m in modules) if (a >= m.Item1 && a < m.Item1 + m.Item2) return m.Item3;
    }
    return "";
  }
  // A thread's name: its description, else its start's module in brackets
  static string Name(IntPtr h, string module) {
    IntPtr d;
    if (GetThreadDescription(h, out d) >= 0 && d != IntPtr.Zero) {
      string s = Marshal.PtrToStringUni(d); LocalFree(d);
      if (!string.IsNullOrEmpty(s)) return s;
    }
    return module.Length > 0 ? "(" + module + ")" : "(unnamed)";
  }

  // Every thread of process pid now: its context switches (each a wake), cycles and name, and the
  // process's I/O operations and handles, from one system snapshot
  public static Snap Take(int pid) {
    int len = 16 << 20; IntPtr buf = Marshal.AllocHGlobal(len);
    try {
      int ret, st;
      while ((st = NtQuerySystemInformation(5, buf, len, out ret)) == unchecked((int)0xC0000004)) {
        Marshal.FreeHGlobal(buf); len = Math.Max(len * 2, ret + (1 << 20)); buf = Marshal.AllocHGlobal(len);
      }
      if (st != 0) throw new Exception("NtQuerySystemInformation: 0x" + st.ToString("x"));
      long off = 0;
      while (true) {
        IntPtr p = new IntPtr(buf.ToInt64() + off);
        int next = Marshal.ReadInt32(p, 0);
        int count = Marshal.ReadInt32(p, 4);
        if (Marshal.ReadInt64(p, 0x50) == pid) {
          var s = new Snap { Pid = pid };
          s.Handles = (uint)Marshal.ReadInt32(p, 0x60);
          s.Reads = Marshal.ReadInt64(p, 0xd0); s.Writes = Marshal.ReadInt64(p, 0xd8); s.Others = Marshal.ReadInt64(p, 0xe0);
          s.ReadBytes = Marshal.ReadInt64(p, 0xe8); s.WriteBytes = Marshal.ReadInt64(p, 0xf0); s.OtherBytes = Marshal.ReadInt64(p, 0xf8);
          for (int i = 0; i < count; i++) {
            IntPtr t = new IntPtr(p.ToInt64() + 0x100 + i * 0x50);
            int tid = (int)Marshal.ReadInt64(t, 0x30);
            s.Threads[tid] = new Thread { Id = tid, Switches = (uint)Marshal.ReadInt32(t, 0x40) };
          }
          var modules = new List<Tuple<long, long, string>>();
          try {
            using (var proc = Process.GetProcessById(pid))
              foreach (ProcessModule m in proc.Modules) modules.Add(Tuple.Create(m.BaseAddress.ToInt64(), (long)m.ModuleMemorySize, m.ModuleName));
          } catch (Exception) { }
          foreach (var th in s.Threads.Values) {
            IntPtr h = OpenThread(THREAD_QUERY_INFORMATION | THREAD_QUERY_LIMITED, false, (uint)th.Id);
            if (h == IntPtr.Zero) { th.Name = "(gone)"; continue; }
            try { ulong c; if (QueryThreadCycleTime(h, out c)) th.Cycles = c; th.Module = Module(h, modules); th.Name = Name(h, th.Module); } finally { CloseHandle(h); }
          }
          IntPtr ph = OpenProcess(PROCESS_QUERY_LIMITED, false, (uint)pid);
          if (ph != IntPtr.Zero) { try { ulong c; QueryProcessCycleTime(ph, out c); s.Cycles = c; } finally { CloseHandle(ph); } }
          return s;
        }
        if (next == 0) return null;
        off += next;
      }
    } finally { Marshal.FreeHGlobal(buf); }
  }
}
'@

$rate = [LbtermRest]::Rate(300)
$ms = { param([double]$cycles) [Math]::Round($cycles / $rate * 1000, 2) }

# What a span cost process $Pid: its threads by name (several of a name added up), each's wakes
# and CPU, the threads that came and went, the process's I/O operations and CPU
function Measure-Span($Before, $After, [double]$Seconds) {
  $rows = @{}
  foreach ($th in $After.Threads.Values) {
    $was = $Before.Threads[$th.Id]
    $wakes = if ($was) { $th.Switches - $was.Switches } else { $th.Switches }
    $cycles = if ($was) { [double]($th.Cycles - $was.Cycles) } else { [double]$th.Cycles }
    $name = if ($was -and $was.Name -and $was.Name -ne '(gone)') { $was.Name } else { $th.Name }
    if (-not $rows[$name]) { $rows[$name] = [ordered]@{ name = $name; module = $th.Module; threads = 0; wakes = 0; cpu_ms = 0.0; started = 0 } }
    $r = $rows[$name]; $r.threads++; $r.wakes += $wakes; $r.cpu_ms += (& $ms $cycles)
    if (-not $was) { $r.started++ }
  }
  $ended = @($Before.Threads.Keys | Where-Object { -not $After.Threads.ContainsKey($_) }).Count
  $threads = @($rows.Values | ForEach-Object { [pscustomobject]$_ } | Sort-Object -Property @{ e = 'wakes'; Descending = $true }, name)
  [pscustomobject]@{
    seconds   = $Seconds
    wakes     = [long](($threads | Measure-Object wakes -Sum).Sum)
    # LBTerm's own threads (started in its exe) but the GUI thread, which other programs' messages
    # wake too
    own_wakes = [long](($threads | Where-Object { $_.module -like 'lbterm*' -and $_.name -ne 'main' } | Measure-Object wakes -Sum).Sum)
    cpu_ms    = & $ms ([double]($After.Cycles - $Before.Cycles))
    reads     = $After.Reads - $Before.Reads
    writes    = $After.Writes - $Before.Writes
    others    = $After.Others - $Before.Others
    handles   = $After.Handles - $Before.Handles
    started   = [int](($threads | Measure-Object started -Sum).Sum)
    ended     = $ended
    threads   = $threads
  }
}
function Format-Span($Span, [string]$Who) {
  "${Who}: $($Span.wakes) wakes in $($Span.seconds) s ($([Math]::Round($Span.wakes / $Span.seconds, 2))/s), CPU $($Span.cpu_ms) ms, I/O $($Span.reads) reads $($Span.writes) writes $($Span.others) other, threads +$($Span.started) -$($Span.ended)"
  foreach ($th in $Span.threads | Where-Object { $_.wakes -gt 0 -or $_.cpu_ms -gt 0.05 }) {
    "  {0,6} wakes {1,9:N2} ms  {2}{3}" -f $th.wakes, $th.cpu_ms, $th.name, $(if ($th.threads -gt 1) { " (x$($th.threads))" } else { '' })
  }
}
