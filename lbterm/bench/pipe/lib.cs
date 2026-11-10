// Throwaway probe: data, a title watch and a cycle-weighted sampling profiler with inline frames.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace PipeProbe {
  public static class Native {
    [DllImport("winmm")] public static extern uint timeBeginPeriod(uint p);
    [DllImport("winmm")] public static extern uint timeEndPeriod(uint p);
    [DllImport("kernel32")] public static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
    [DllImport("kernel32")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32")] public static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32")] public static extern bool QueryThreadCycleTime(IntPtr h, out ulong cycles);
    [DllImport("kernel32")] public static extern bool QueryProcessCycleTime(IntPtr h, out ulong cycles);
    [DllImport("kernel32")] public static extern int GetThreadDescription(IntPtr h, out IntPtr desc);
    [DllImport("kernel32")] public static extern IntPtr LocalFree(IntPtr p);
    public static long ThreadCycles(uint tid) {
      IntPtr h = OpenThread(0x0800, false, tid); if (h == IntPtr.Zero) return -1;
      try { ulong c; return QueryThreadCycleTime(h, out c) ? (long)c : -1; } finally { CloseHandle(h); }
    }
    public static long ProcessCycles(uint pid) {
      IntPtr h = OpenProcess(0x1000, false, pid); if (h == IntPtr.Zero) return -1;
      try { ulong c; return QueryProcessCycleTime(h, out c) ? (long)c : -1; } finally { CloseHandle(h); }
    }
    public static string ThreadName(uint tid) {
      IntPtr h = OpenThread(0x0800, false, tid); if (h == IntPtr.Zero) return "";
      try { IntPtr p; if (GetThreadDescription(h, out p) < 0 || p == IntPtr.Zero) return ""; try { return Marshal.PtrToStringUni(p) ?? ""; } finally { LocalFree(p); } }
      finally { CloseHandle(h); }
    }
  }

  public static class Data {
    static FileStream Open(string path) { return new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.Read, 1 << 20); }
    // lbterm/bench's plain: `width` printable ASCII a line, CRLF
    public static long[] Plain(string path, long target, int width) {
      long bytes = 0, lines = 0; var sb = new StringBuilder();
      using (var f = Open(path)) {
        while (bytes < target) {
          for (int i = 0; i < width; i++) sb.Append((char)(32 + (int)((lines * 7 + i * 3) % 95)));
          sb.Append("\r\n");
          var b = Encoding.ASCII.GetBytes(sb.ToString()); f.Write(b, 0, b.Length); bytes += b.Length; lines++; sb.Clear();
        }
      }
      return new[] { bytes, lines };
    }
  }

  public class TitleWatch {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    Thread thread; long seen; volatile bool stop;
    public static TitleWatch Start(long hwnd, string marker, int timeoutMs) {
      var w = new TitleWatch(); var h = new IntPtr(hwnd);
      w.thread = new Thread(() => {
        Native.timeBeginPeriod(1);
        try {
          var clock = Stopwatch.StartNew(); var sb = new StringBuilder(1024);
          while (!w.stop && clock.ElapsedMilliseconds < timeoutMs) {
            sb.Clear(); GetWindowText(h, sb, 1024);
            if (sb.ToString().Contains(marker)) { Interlocked.Exchange(ref w.seen, DateTime.UtcNow.Ticks); return; }
            Thread.Sleep(1);
          }
        } finally { Native.timeEndPeriod(1); }
      });
      w.thread.IsBackground = true; w.thread.Start();
      return w;
    }
    public long Wait(int ms) { thread.Join(ms); return Interlocked.Read(ref seen); }
    public void Cancel() { stop = true; }
  }

  // Suspends each target thread hz times a second, walks its stack (StackWalk64 on x64 unwind
  // data) and resumes it; each sample is weighted by the thread's cycles since its last sample, so
  // a sample in a wait weighs nothing in the CPU view and one in the wall view.
  public class Sampler {
    [DllImport("kernel32")] static extern uint SuspendThread(IntPtr h);
    [DllImport("kernel32")] static extern uint ResumeThread(IntPtr h);
    [DllImport("kernel32")] static extern bool GetThreadContext(IntPtr h, IntPtr ctx);
    [DllImport("kernel32", CharSet = CharSet.Ansi)] static extern IntPtr GetProcAddress(IntPtr mod, string name);
    [DllImport("kernel32", CharSet = CharSet.Unicode)] static extern IntPtr LoadLibraryW(string name);
    [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymInitializeW(IntPtr hProcess, string path, bool invade);
    [DllImport("dbghelp")] static extern bool SymCleanup(IntPtr hProcess);
    [DllImport("dbghelp")] static extern uint SymSetOptions(uint opts);
    [DllImport("dbghelp")] static extern bool StackWalk64(uint machine, IntPtr hProcess, IntPtr hThread, IntPtr frame, IntPtr ctx, IntPtr readMem, IntPtr funcTable, IntPtr modBase, IntPtr translate);
    [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymFromAddrW(IntPtr hProcess, ulong addr, out ulong disp, IntPtr symbol);
    [DllImport("dbghelp")] static extern uint SymAddrIncludeInlineTrace(IntPtr hProcess, ulong addr);
    [DllImport("dbghelp")] static extern bool SymQueryInlineTrace(IntPtr hProcess, ulong start, uint startCtx, ulong startRet, ulong cur, out uint curCtx, out uint curFrameIndex);
    [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymFromInlineContextW(IntPtr hProcess, ulong addr, uint ctx, out ulong disp, IntPtr symbol);
    [DllImport("dbghelp", CharSet = CharSet.Unicode)] static extern bool SymGetModuleInfoW64(IntPtr hProcess, ulong addr, IntPtr info);

    public class Target {
      public uint Pid, Tid; public string Label; public IntPtr Thread, Process; public ulong Last;
      public List<ulong[]> Stacks = new List<ulong[]>(); public List<long> Weights = new List<long>();
      public long Skipped;
    }
    public List<Target> Targets = new List<Target>();
    Dictionary<uint, IntPtr> procs = new Dictionary<uint, IntPtr>();
    Thread worker; volatile bool stop; int hz, depth;
    public volatile bool Recording; public long Walks; public double WalkMs; public double CtxMs; public long Frames; public double RecordedMs; long recStart = -1;
    public double ElapsedMs;

    public static Sampler Start(uint[] pids, uint[] tids, string[] labels, int hz, string symPath, int depth) {
      var s = new Sampler { hz = hz, depth = depth };
      SymSetOptions(0x2 | 0x4 | 0x10); // UNDNAME | DEFERRED_LOADS | LOAD_LINES
      for (int i = 0; i < tids.Length; i++) {
        IntPtr p;
        if (!s.procs.TryGetValue(pids[i], out p)) {
          p = Native.OpenProcess(0x0410 | 0x0008, false, pids[i]);
          if (p == IntPtr.Zero) throw new Exception("OpenProcess " + pids[i] + " failed: " + Marshal.GetLastWin32Error());
          if (!SymInitializeW(p, symPath, true)) throw new Exception("SymInitialize failed: " + Marshal.GetLastWin32Error());
          s.procs[pids[i]] = p;
        }
        var t = new Target { Pid = pids[i], Tid = tids[i], Label = labels[i], Process = p, Thread = Native.OpenThread(0x0002 | 0x0008 | 0x0040 | 0x0800, false, tids[i]) };
        if (t.Thread != IntPtr.Zero) { ulong c; Native.QueryThreadCycleTime(t.Thread, out c); t.Last = c; s.Targets.Add(t); }
      }
      s.worker = new Thread(s.Loop) { IsBackground = true, Priority = ThreadPriority.Highest };
      s.worker.Start();
      return s;
    }

    [DllImport("kernel32", SetLastError = true)] static extern bool ReadProcessMemory(IntPtr p, IntPtr addr, IntPtr buf, IntPtr size, out IntPtr read);
    [DllImport("kernel32")] static extern IntPtr VirtualQueryEx(IntPtr p, IntPtr addr, IntPtr info, IntPtr len);
    [UnmanagedFunctionPointer(CallingConvention.Winapi)] delegate bool ReadMemProc(IntPtr hProcess, ulong addr, IntPtr buf, uint size, IntPtr read);
    ReadMemProc readDelegate; IntPtr readPtr;
    IntPtr stackBuf = Marshal.AllocHGlobal(256 * 1024); ulong stackBase; long stackLen;
    Dictionary<string, IntPtr> pages = new Dictionary<string, IntPtr>();
    Dictionary<uint, ulong> stackTop = new Dictionary<uint, ulong>();
    public long PageMisses, ReadFails;
    IntPtr Page(IntPtr hp, ulong page) {
      string k = hp.ToInt64() + ":" + page; IntPtr b;
      if (pages.TryGetValue(k, out b)) return b;
      b = Marshal.AllocHGlobal(4096); IntPtr got;
      if (!ReadProcessMemory(hp, new IntPtr((long)page), b, new IntPtr(4096), out got)) { Marshal.FreeHGlobal(b); b = IntPtr.Zero; }
      PageMisses++; pages[k] = b; return b;
    }
    bool ReadMem(IntPtr hp, ulong addr, IntPtr buf, uint size, IntPtr read) {
      if (addr >= stackBase && addr + size <= stackBase + (ulong)stackLen) {
        unsafe { Buffer.MemoryCopy((void*)(stackBuf.ToInt64() + (long)(addr - stackBase)), (void*)buf, size, size); }
        if (read != IntPtr.Zero) Marshal.WriteInt32(read, (int)size); return true;
      }
      uint done = 0;
      while (done < size) {
        ulong a = addr + done, page = a & ~4095UL; IntPtr pb = Page(hp, page);
        if (pb == IntPtr.Zero) { ReadFails++; if (read != IntPtr.Zero) Marshal.WriteInt32(read, (int)done); return done > 0; }
        uint off = (uint)(a - page), n = Math.Min(size - done, 4096 - off);
        unsafe { Buffer.MemoryCopy((void*)(pb.ToInt64() + off), (void*)(buf.ToInt64() + done), n, n); }
        done += n;
      }
      if (read != IntPtr.Zero) Marshal.WriteInt32(read, (int)done); return true;
    }
    ulong StackTop(Target t, ulong rsp) {
      ulong top; if (stackTop.TryGetValue(t.Tid, out top) && rsp < top) return top;
      IntPtr info = Marshal.AllocHGlobal(48);
      try { if (VirtualQueryEx(t.Process, new IntPtr((long)rsp), info, new IntPtr(48)) == IntPtr.Zero) return rsp + 65536; top = (ulong)Marshal.ReadInt64(info, 0) + (ulong)Marshal.ReadInt64(info, 24); }
      finally { Marshal.FreeHGlobal(info); }
      stackTop[t.Tid] = top; return top;
    }

    void Loop() {
      IntPtr dbg = LoadLibraryW("dbghelp.dll");
      IntPtr fta = GetProcAddress(dbg, "SymFunctionTableAccess64"), gmb = GetProcAddress(dbg, "SymGetModuleBase64");
      readDelegate = ReadMem; readPtr = Marshal.GetFunctionPointerForDelegate(readDelegate);
      IntPtr ctxRaw = Marshal.AllocHGlobal(1232 + 32), ctx = new IntPtr((ctxRaw.ToInt64() + 15) & ~15L), frame = Marshal.AllocHGlobal(512);
      var pcs = new List<ulong>(depth);
      Native.timeBeginPeriod(1);
      var clock = Stopwatch.StartNew(); long period = Stopwatch.Frequency / hz, next = 0;
      try {
        while (!stop) {
          foreach (var t in Targets) {
            ulong c; if (!Native.QueryThreadCycleTime(t.Thread, out c)) continue;
            long w0 = clock.ElapsedTicks;
            if (SuspendThread(t.Thread) == 0xFFFFFFFF) continue;
            pcs.Clear();
            bool ok = false;
            try {
              Native.QueryThreadCycleTime(t.Thread, out c);
              for (int b = 0; b < 1232; b += 8) Marshal.WriteInt64(ctx, b, 0);
              Marshal.WriteInt32(ctx, 0x30, 0x100003);
              if (GetThreadContext(t.Thread, ctx)) {
                ulong rsp0 = (ulong)Marshal.ReadInt64(ctx, 0x98);
                ulong top = StackTop(t, rsp0);
                long len = (long)Math.Min(256UL * 1024, top - rsp0);
                IntPtr got;
                if (ReadProcessMemory(t.Process, new IntPtr((long)rsp0), stackBuf, new IntPtr(len), out got) || got.ToInt64() > 0) { stackBase = rsp0; stackLen = got.ToInt64() > 0 ? got.ToInt64() : len; ok = true; }
              } else t.Skipped++;
            } finally { ResumeThread(t.Thread); CtxMs += (clock.ElapsedTicks - w0) * 1000.0 / Stopwatch.Frequency; }
            if (ok) {
              ulong rip = (ulong)Marshal.ReadInt64(ctx, 0xF8), rsp = (ulong)Marshal.ReadInt64(ctx, 0x98), rbp = (ulong)Marshal.ReadInt64(ctx, 0xA0);
              for (int b = 0; b < 512; b += 8) Marshal.WriteInt64(frame, b, 0);
              Marshal.WriteInt64(frame, 0, (long)rip); Marshal.WriteInt32(frame, 12, 3);
              Marshal.WriteInt64(frame, 32, (long)rbp); Marshal.WriteInt32(frame, 44, 3);
              Marshal.WriteInt64(frame, 48, (long)rsp); Marshal.WriteInt32(frame, 60, 3);
              for (int d = 0; d < depth; d++) {
                if (!StackWalk64(0x8664, t.Process, IntPtr.Zero, frame, ctx, readPtr, fta, gmb, IntPtr.Zero)) break;
                ulong pc = (ulong)Marshal.ReadInt64(frame, 0); if (pc == 0) break; pcs.Add(pc); Frames++;
              }
            }
            Walks++; WalkMs += (clock.ElapsedTicks - w0) * 1000.0 / Stopwatch.Frequency;
            long w = (long)(c - t.Last); t.Last = c;
            if (Recording && pcs.Count > 0) { if (recStart < 0) recStart = clock.ElapsedTicks; t.Stacks.Add(pcs.ToArray()); t.Weights.Add(w); }
          }
          next += period;
          long wait = next - clock.ElapsedTicks;
          if (wait > 0) Thread.Sleep((int)Math.Max(1, wait * 1000 / Stopwatch.Frequency));
          else if (wait < -10 * period) next = clock.ElapsedTicks;
        }
      } finally {
        ElapsedMs = clock.Elapsed.TotalMilliseconds; if (recStart >= 0) RecordedMs = (clock.ElapsedTicks - recStart) * 1000.0 / Stopwatch.Frequency;
        Native.timeEndPeriod(1); Marshal.FreeHGlobal(ctxRaw); Marshal.FreeHGlobal(frame);
      }
    }
    public void Stop() { stop = true; worker.Join(); foreach (var t in Targets) Native.CloseHandle(t.Thread); }

    Dictionary<string, string[]> names = new Dictionary<string, string[]>();
    string Sym(IntPtr process, ulong pc, uint inlineCtx, bool inl) {
      IntPtr sym = Marshal.AllocHGlobal(88 + 2 * 1024);
      try {
        for (int b = 0; b < 88; b += 4) Marshal.WriteInt32(sym, b, 0);
        Marshal.WriteInt32(sym, 0, 88); Marshal.WriteInt32(sym, 80, 1000);
        ulong disp;
        bool ok = inl ? SymFromInlineContextW(process, pc, inlineCtx, out disp, sym) : SymFromAddrW(process, pc, out disp, sym);
        if (ok) { int len = Marshal.ReadInt32(sym, 76); return Marshal.PtrToStringUni(new IntPtr(sym.ToInt64() + 84), Math.Min(len, 1000)); }
      } finally { Marshal.FreeHGlobal(sym); }
      if (inl) return null;
      IntPtr info = Marshal.AllocHGlobal(4096);
      try {
        for (int b = 0; b < 4096; b += 4) Marshal.WriteInt32(info, b, 0);
        Marshal.WriteInt32(info, 0, 3264);
        return SymGetModuleInfoW64(process, pc, info) ? "(" + Marshal.PtrToStringUni(new IntPtr(info.ToInt64() + 44)) + ")" : "?";
      } finally { Marshal.FreeHGlobal(info); }
    }
    // The frames at pc, innermost (inlined) first, then the function they're in
    public string[] Names(IntPtr process, ulong pc) {
      string key = process.ToInt64() + ":" + pc;
      string[] r; if (names.TryGetValue(key, out r)) return r;
      var list = new List<string>();
      uint n = SymAddrIncludeInlineTrace(process, pc);
      if (n > 0) {
        uint ctx, idx;
        if (SymQueryInlineTrace(process, pc, 0, pc, pc, out ctx, out idx)) {
          for (uint i = 0; i < n; i++) { var s = Sym(process, pc, ctx + i, true); if (s != null) list.Add(s + " [inl]"); }
        }
      }
      list.Add(Sym(process, pc, 0, false));
      r = list.ToArray(); names[key] = r; return r;
    }

    // Rust's legacy mangling (ZN <len><seg>... 17h<hash> E) as a path; anything else as it is
    public static string Clean(string name) {
      if (name == null) return "?";
      string tail = "";
      if (name.EndsWith(" [inl]")) { tail = " [inl]"; name = name.Substring(0, name.Length - 6); }
      int z = name.IndexOf("ZN");
      if (!(name.StartsWith("_ZN") || name.StartsWith("ZN"))) return name + tail;
      string body = name.Substring(z + 2);
      int e = body.LastIndexOf('E'); if (e < 0) return name + tail;
      body = body.Substring(0, e);
      var parts = new List<string>(); int i = 0;
      while (i < body.Length) {
        int j = i; while (j < body.Length && char.IsDigit(body[j])) j++;
        if (j == i) return name + tail;
        int len = int.Parse(body.Substring(i, j - i)); if (j + len > body.Length) return name + tail;
        parts.Add(body.Substring(j, len)); i = j + len;
      }
      if (parts.Count > 1 && parts[parts.Count - 1].Length == 17 && parts[parts.Count - 1][0] == 'h') parts.RemoveAt(parts.Count - 1);
      var sb = new StringBuilder();
      for (int k = 0; k < parts.Count; k++) {
        string s = parts[k]; if (s.StartsWith("_$")) s = s.Substring(1);
        s = s.Replace("$LT$", "<").Replace("$GT$", ">").Replace("$RF$", "&").Replace("$BP$", "*").Replace("$C$", ",").Replace("$u20$", " ").Replace("$u27$", "'").Replace("$u5b$", "[").Replace("$u5d$", "]").Replace("$u7b$", "{").Replace("$u7d$", "}").Replace("$u7e$", "~").Replace("..", "::");
        if (k > 0) sb.Append("::"); sb.Append(s);
      }
      return sb.ToString() + tail;
    }

    // Per target: folded stacks (root first, ';'-joined) with their cycles and sample counts
    public Dictionary<string, long[]> Folded(Target t) { return Folded(t, Clean); }
    public Dictionary<string, long[]> Folded(Target t, Func<string, string> clean) {
      var d = new Dictionary<string, long[]>();
      for (int i = 0; i < t.Stacks.Count; i++) {
        var st = t.Stacks[i]; var parts = new List<string>();
        for (int k = st.Length - 1; k >= 0; k--) {
          ulong pc = k == 0 ? st[k] : st[k] - 1;
          var ns = Names(t.Process, pc);
          for (int j = ns.Length - 1; j >= 0; j--) parts.Add(clean(ns[j]));
        }
        string key = string.Join(";", parts);
        long[] v; if (!d.TryGetValue(key, out v)) { v = new long[2]; d[key] = v; }
        v[0] += t.Weights[i]; v[1] += 1;
      }
      return d;
    }
    public void Done() { foreach (var p in procs.Values) { SymCleanup(p); Native.CloseHandle(p); } }
  }
}
