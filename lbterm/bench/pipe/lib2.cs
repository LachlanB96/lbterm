// Throwaway probe: every thread's context switches and each process's I/O counts, unelevated
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace PipeProbe {
  public static class SysInfo {
    [DllImport("ntdll")] static extern int NtQuerySystemInformation(int cls, IntPtr buf, int len, out int ret);
    [DllImport("kernel32")] static extern bool GetProcessIoCounters(IntPtr h, out IO c);
    [StructLayout(LayoutKind.Sequential)] struct IO { public ulong ReadOps, WriteOps, OtherOps, ReadBytes, WriteBytes, OtherBytes; }
    static IntPtr buf = IntPtr.Zero; static int size = 0;
    // pid, tid, context switches, kernel and user time (100 ns), state, wait reason; the processes' own I/O
    public static List<long[]> Threads(uint[] pids) {
      var want = new HashSet<uint>(pids); var rows = new List<long[]>();
      if (size == 0) { size = 8 << 20; buf = Marshal.AllocHGlobal(size); }
      int ret; int st;
      while ((st = NtQuerySystemInformation(5, buf, size, out ret)) == unchecked((int)0xC0000004)) { Marshal.FreeHGlobal(buf); size *= 2; buf = Marshal.AllocHGlobal(size); }
      if (st != 0) throw new Exception("NtQuerySystemInformation " + st.ToString("X"));
      long off = 0;
      while (true) {
        IntPtr p = new IntPtr(buf.ToInt64() + off);
        int next = Marshal.ReadInt32(p, 0), n = Marshal.ReadInt32(p, 4);
        uint pid = (uint)Marshal.ReadInt64(p, 0x50);
        if (want.Contains(pid)) {
          for (int i = 0; i < n; i++) {
            IntPtr t = new IntPtr(p.ToInt64() + 0x100 + i * 0x50);
            rows.Add(new long[] { pid, Marshal.ReadInt64(t, 0x30), (uint)Marshal.ReadInt32(t, 0x40), Marshal.ReadInt64(t, 0x00), Marshal.ReadInt64(t, 0x08), Marshal.ReadInt32(t, 0x44), Marshal.ReadInt32(t, 0x48) });
          }
        }
        if (next == 0) break; off += next;
      }
      return rows;
    }
    public static long[] Io(uint pid) {
      IntPtr h = Native.OpenProcess(0x1000, false, pid); if (h == IntPtr.Zero) return null;
      try { IO c; if (!GetProcessIoCounters(h, out c)) return null; return new long[] { (long)c.ReadOps, (long)c.WriteOps, (long)c.OtherOps, (long)c.ReadBytes, (long)c.WriteBytes, (long)c.OtherBytes }; }
      finally { Native.CloseHandle(h); }
    }
  }
}
