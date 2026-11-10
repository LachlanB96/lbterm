# What the benchmark scripts share (bench.ps1, key-latency.ps1): launching each terminal
# unfocused on the right-most screen, finding the processes that are its own, timing and
# sampling them, and closing only what was opened. lbterm/BENCH.md says what is measured and how.
#
#   Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
#   . (Join-Path $PSScriptRoot 'bench-lib.ps1')
#
# The terminals ("kinds"):
#   lbterm      LBTerm's GUI with its tabs in its own process (the local domain)
#   lbterm-mux  LBTerm's GUI on a mux domain of the benchmark's own: every tab in lbterm-mux-server,
#               which the GUI starts (serve_command), as Lachlan runs it
#   wt          Windows Terminal, started through its wt.exe alias: a WindowsTerminal.exe of its
#               own (one that wasn't running before the launch), never one that was
#
# Upstream WezTerm was a kind too until 2026-11-04, when it was uninstalled from the PC (BENCH.md,
# "WezTerm, the baseline that stays"): its numbers from earlier runs stay in BENCH.md.
#
# Nothing here sends input, changes focus or touches a window or process it didn't start: every
# process it ends was created after the benchmark began (Protect-Existing), and is found by pid.
Set-StrictMode -Off

$script:BenchRoot = $PSScriptRoot
$script:BenchPwsh = 'C:\Program Files\PowerShell\7\pwsh.exe'
$script:WtAlias = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\wt.exe'

if (-not ('LbtermBench.Native' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.IO; using System.Text;
using System.Runtime.InteropServices; using System.Threading;
namespace LbtermBench {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }

  public class ProcInfo { public uint Pid, Parent; public string Exe; public long Created; }
  public class WindowHit { public long Hwnd; public uint Pid; public string Exe; public string Class; public long At; public RECT Rect; }
  public class FgChange { public long At; public long Hwnd; public uint Pid; public uint IdleMs; public uint PrevPid; public int X, Y; public bool PointerOn; public bool Killed; }
  public class ProcStart { public long At; public uint Pid, Parent; public string Exe, Path; }
  public class Mem { public uint Pid; public long Private, WorkingSet, PeakWorkingSet; }
  public class ThreadTime { public uint Tid; public string Name; public long Cpu; }

  // Each thread of a process: its id, its name (SetThreadDescription, which Rust's named threads
  // set) and its kernel plus user time in 100 ns units
  public static class Threads {
    [StructLayout(LayoutKind.Sequential)] struct TE32 { public uint dwSize, cntUsage, th32ThreadID, th32OwnerProcessID; public int tpBasePri, tpDeltaPri; public uint dwFlags; }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll")] static extern bool Thread32First(IntPtr snap, ref TE32 e);
    [DllImport("kernel32.dll")] static extern bool Thread32Next(IntPtr snap, ref TE32 e);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
    [DllImport("kernel32.dll")] static extern bool GetThreadTimes(IntPtr h, out long c, out long e, out long k, out long u);
    [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr h, out IntPtr name);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
    public static ThreadTime[] Of(uint pid) {
      var r = new List<ThreadTime>();
      IntPtr snap = CreateToolhelp32Snapshot(4, 0);
      if (snap == IntPtr.Zero || snap == new IntPtr(-1)) return r.ToArray();
      try {
        var e = new TE32(); e.dwSize = (uint)Marshal.SizeOf(typeof(TE32));
        if (Thread32First(snap, ref e)) do {
          if (e.th32OwnerProcessID != pid) continue;
          IntPtr h = OpenThread(0x0800, false, e.th32ThreadID);
          if (h == IntPtr.Zero) continue;
          try {
            long c, x, k, u; if (!GetThreadTimes(h, out c, out x, out k, out u)) continue;
            string name = ""; IntPtr p;
            if (GetThreadDescription(h, out p) >= 0 && p != IntPtr.Zero) { name = Marshal.PtrToStringUni(p); LocalFree(p); }
            r.Add(new ThreadTime { Tid = e.th32ThreadID, Name = name, Cpu = k + u });
          } finally { CloseHandle(h); }
        } while (Thread32Next(snap, ref e));
      } finally { CloseHandle(snap); }
      return r.ToArray();
    }
  }

  public static class Native {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int v, int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT v, int size);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr h, int flags, StringBuilder s, ref int size);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long create, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr h, out uint code);
    [DllImport("psapi.dll")] static extern bool GetProcessMemoryInfo(IntPtr h, out PMCEX c, int size);
    [DllImport("winmm.dll")] public static extern uint timeBeginPeriod(uint p);
    [DllImport("winmm.dll")] public static extern uint timeEndPeriod(uint p);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr snap, ref PE32 e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr snap, ref PE32 e);
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUTINFO { public uint cbSize; public uint dwTime; }
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUTINFO i);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct PE32 {
      public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID;
      public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags;
      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }
    [StructLayout(LayoutKind.Sequential)] struct PMCEX {
      public uint cb; public uint PageFaultCount; public UIntPtr PeakWorkingSetSize; public UIntPtr WorkingSetSize;
      public UIntPtr QuotaPeakPagedPoolUsage; public UIntPtr QuotaPagedPoolUsage; public UIntPtr QuotaPeakNonPagedPoolUsage;
      public UIntPtr QuotaNonPagedPoolUsage; public UIntPtr PagefileUsage; public UIntPtr PeakPagefileUsage; public UIntPtr PrivateUsage;
    }
    const uint QueryLimited = 0x1000, VmRead = 0x0010;

    // DateTime.UtcNow.Ticks: GetSystemTimePreciseAsFileTime, the clock the programs inside the
    // terminals write too
    public static long Now() { return DateTime.UtcNow.Ticks; }
    public static uint IdleMs() {
      var i = new LASTINPUTINFO(); i.cbSize = (uint)Marshal.SizeOf(i); GetLastInputInfo(ref i);
      return (uint)Environment.TickCount - i.dwTime;
    }
    public static uint ForegroundPid() { uint p; GetWindowThreadProcessId(GetForegroundWindow(), out p); return p; }

    // When a process was created, as DateTime ticks (UTC); 0 when it can't be opened
    public static long Created(uint pid) {
      IntPtr h = OpenProcess(QueryLimited, false, pid);
      if (h == IntPtr.Zero) return 0;
      try { long c, e, k, u; return GetProcessTimes(h, out c, out e, out k, out u) ? DateTime.FromFileTimeUtc(c).Ticks : 0; }
      finally { CloseHandle(h); }
    }
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr p, uint access, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr t, int cls, out int info, int size, out int ret);
    // 1 elevated, 0 not, -1 its token can't be read from here
    public static int Elevated(uint pid) {
      IntPtr h = OpenProcess(QueryLimited, false, pid);
      if (h == IntPtr.Zero) return -1;
      try {
        IntPtr t; if (!OpenProcessToken(h, 0x0008, out t)) return -1;
        try { int e, n; return GetTokenInformation(t, 20, out e, 4, out n) ? (e != 0 ? 1 : 0) : -1; } finally { CloseHandle(t); }
      } finally { CloseHandle(h); }
    }
    public static bool Alive(uint pid) {
      IntPtr h = OpenProcess(QueryLimited, false, pid);
      if (h == IntPtr.Zero) return false;
      try { uint code; return GetExitCodeProcess(h, out code) && code == 259; } finally { CloseHandle(h); }
    }
    static readonly Dictionary<uint, string> exeCache = new Dictionary<uint, string>();
    public static string ExePath(uint pid) {
      IntPtr h = OpenProcess(QueryLimited, false, pid);
      if (h == IntPtr.Zero) return null;
      try { var s = new StringBuilder(1024); int n = s.Capacity; return QueryFullProcessImageName(h, 0, s, ref n) ? s.ToString() : null; }
      finally { CloseHandle(h); }
    }

    // Every process now: pid, parent, file name, creation time
    public static ProcInfo[] Snapshot() {
      var list = new List<ProcInfo>();
      IntPtr snap = CreateToolhelp32Snapshot(2, 0);
      if (snap == IntPtr.Zero || snap == new IntPtr(-1)) return list.ToArray();
      try {
        var e = new PE32(); e.dwSize = (uint)Marshal.SizeOf(typeof(PE32));
        if (Process32FirstW(snap, ref e)) {
          do { list.Add(new ProcInfo { Pid = e.th32ProcessID, Parent = e.th32ParentProcessID, Exe = e.szExeFile }); } while (Process32NextW(snap, ref e));
        }
      } finally { CloseHandle(snap); }
      foreach (var p in list) p.Created = Created(p.Pid);
      return list.ToArray();
    }

    // The descendants of `roots` in a snapshot: a child counts only when it was created after its
    // parent, so a parent pid used again by an unrelated process links nothing
    public static ProcInfo[] Descendants(ProcInfo[] snap, uint[] roots) {
      var byPid = new Dictionary<uint, ProcInfo>(); foreach (var p in snap) byPid[p.Pid] = p;
      var inSet = new HashSet<uint>(roots); var result = new List<ProcInfo>();
      bool grew = true;
      while (grew) {
        grew = false;
        foreach (var p in snap) {
          if (inSet.Contains(p.Pid) || !inSet.Contains(p.Parent)) continue;
          ProcInfo parent; if (!byPid.TryGetValue(p.Parent, out parent)) continue;
          if (p.Created != 0 && parent.Created != 0 && p.Created < parent.Created) continue;
          inSet.Add(p.Pid); result.Add(p); grew = true;
        }
      }
      return result.ToArray();
    }

    // Kernel plus user time of each process, in 100 ns units; -1 for one that can't be read
    public static long[] Cpu(uint[] pids) {
      var r = new long[pids.Length];
      for (int i = 0; i < pids.Length; i++) {
        IntPtr h = OpenProcess(QueryLimited, false, pids[i]);
        if (h == IntPtr.Zero) { r[i] = -1; continue; }
        try { long c, e, k, u; r[i] = GetProcessTimes(h, out c, out e, out k, out u) ? k + u : -1; } finally { CloseHandle(h); }
      }
      return r;
    }

    // Private bytes (commit) and working set of each process
    public static Mem[] Memory(uint[] pids) {
      var r = new List<Mem>();
      foreach (var pid in pids) {
        IntPtr h = OpenProcess(QueryLimited | VmRead, false, pid);
        if (h == IntPtr.Zero) h = OpenProcess(QueryLimited, false, pid);
        if (h == IntPtr.Zero) continue;
        try {
          PMCEX c; c = new PMCEX(); c.cb = (uint)Marshal.SizeOf(typeof(PMCEX));
          if (GetProcessMemoryInfo(h, out c, (int)c.cb))
            r.Add(new Mem { Pid = pid, Private = (long)c.PrivateUsage.ToUInt64(), WorkingSet = (long)c.WorkingSetSize.ToUInt64(), PeakWorkingSet = (long)c.PeakWorkingSetSize.ToUInt64() });
        } finally { CloseHandle(h); }
      }
      return r.ToArray();
    }

    static bool Cloaked(IntPtr h) { int v; return DwmGetWindowAttribute(h, 14, out v, 4) == 0 && v != 0; }
    // The window's bounds without the invisible resize borders (DWMWA_EXTENDED_FRAME_BOUNDS)
    public static RECT Frame(IntPtr h) {
      RECT r; if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) == 0) return r;
      GetWindowRect(h, out r); return r;
    }
    public static string ClassOf(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
    public static string TitleOf(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }

    // A top-level window someone can see: visible, not cloaked, not owned (a tooltip, a popup),
    // with an area
    static bool Seen(IntPtr h) {
      if (!IsWindowVisible(h) || GetWindow(h, 4) != IntPtr.Zero || Cloaked(h)) return false;
      RECT r; return GetWindowRect(h, out r) && r.Right > r.Left && r.Bottom > r.Top;
    }

    // The visible top-level windows of process `pid`
    public static long[] WindowsOf(uint pid) {
      var l = new List<long>();
      EnumWindows((h, x) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && Seen(h)) l.Add(h.ToInt64()); return true; }, IntPtr.Zero);
      return l.ToArray();
    }

    // Waits for the first visible top-level window of process `pid` (or, with pid 0, of any process
    // whose file is `exe` and isn't in `exclude`), of class `cls` when one is given, looking every
    // millisecond: when it was seen
    public static WindowHit WaitWindow(uint pid, string exe, string cls, uint[] exclude, int timeoutMs) {
      var skip = new HashSet<uint>(exclude ?? new uint[0]);
      var names = new Dictionary<uint, string>();
      var clock = System.Diagnostics.Stopwatch.StartNew();
      timeBeginPeriod(1);
      try {
        while (clock.ElapsedMilliseconds < timeoutMs) {
          WindowHit hit = null;
          EnumWindows((h, x) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (pid != 0 ? p != pid : skip.Contains(p)) return true;
            if (!Seen(h)) return true;
            string c = ClassOf(h);
            if (!string.IsNullOrEmpty(cls) && c != cls) return true;
            if (pid == 0) {
              string n;
              if (!names.TryGetValue(p, out n)) { var path = ExePath(p); n = path == null ? "" : Path.GetFileName(path); names[p] = n; }
              if (!string.Equals(n, exe, StringComparison.OrdinalIgnoreCase)) return true;
            }
            RECT r; GetWindowRect(h, out r);
            hit = new WindowHit { Hwnd = h.ToInt64(), Pid = p, Exe = exe, Class = c, At = Now(), Rect = r };
            return false;
          }, IntPtr.Zero);
          if (hit != null) return hit;
          Thread.Sleep(1);
        }
      } finally { timeEndPeriod(1); }
      return null;
    }
  }

  // Waits on a thread of its own for a window's title to hold `marker`, reading it every
  // millisecond with GetWindowText (which reads another process's caption without sending its
  // window a message, so the terminal's GUI thread does nothing for it): when it first did
  public class TitleWatch {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    Thread thread; long seen; volatile bool stop;
    public static TitleWatch Start(long hwnd, string marker, int timeoutMs) {
      var w = new TitleWatch(); var h = new IntPtr(hwnd);
      w.thread = new Thread(() => {
        Native.timeBeginPeriod(1);
        try {
          var clock = System.Diagnostics.Stopwatch.StartNew(); var sb = new StringBuilder(1024);
          while (!w.stop && clock.ElapsedMilliseconds < timeoutMs) {
            sb.Clear(); GetWindowText(h, sb, 1024);
            if (sb.ToString().Contains(marker)) { Interlocked.Exchange(ref w.seen, Native.Now()); return; }
            Thread.Sleep(1);
          }
        } finally { Native.timeEndPeriod(1); }
      });
      w.thread.IsBackground = true; w.thread.Start();
      return w;
    }
    // When the title held the marker (DateTime ticks), or 0 if it didn't within `ms` more
    public long Wait(int ms) { thread.Join(ms); return Interlocked.Read(ref seen); }
    public void Cancel() { stop = true; }
  }

  // Who has the foreground, every 20 ms on a thread of its own: each change, with how long the
  // real keyboard and mouse had been idle then, the process that had it before, and whether the
  // pointer was on the window that took it (a click of Lachlan's gives a window the foreground).
  // A guarded process (a benchmark terminal's) that gets the foreground is ended there and then,
  // so whatever Lachlan types next can't go to it for more than those 20 ms.
  public static class ForegroundWatch {
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr h, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    static Timer timer; static readonly List<FgChange> seen = new List<FgChange>(); static long last = -1; static uint lastPid;
    static readonly HashSet<uint> guard = new HashSet<uint>();
    public static void Guard(uint pid) { lock (seen) guard.Add(pid); }
    public static void Start() {
      lock (seen) { seen.Clear(); last = -1; }
      if (timer != null) timer.Dispose();
      timer = new Timer(_ => {
        IntPtr h = Native.GetForegroundWindow(); long hv = h.ToInt64();
        lock (seen) {
          if (hv == last) return;
          last = hv; uint p = Native.ForegroundPid();
          var c = new FgChange { At = Native.Now(), Hwnd = hv, Pid = p, IdleMs = Native.IdleMs(), PrevPid = lastPid };
          lastPid = p;
          POINT pt; RECT r;
          if (GetCursorPos(out pt)) { c.X = pt.X; c.Y = pt.Y; if (Native.GetWindowRect(h, out r)) c.PointerOn = pt.X >= r.Left && pt.X < r.Right && pt.Y >= r.Top && pt.Y < r.Bottom; }
          if (guard.Contains(p)) {
            IntPtr ph = OpenProcess(0x0001, false, p);
            if (ph != IntPtr.Zero) { c.Killed = TerminateProcess(ph, 1); CloseHandle(ph); }
          }
          seen.Add(c);
        }
      }, null, 0, 20);
    }
    public static FgChange[] Since(long ticks) { lock (seen) return seen.FindAll(c => c.At >= ticks).ToArray(); }
    public static FgChange[] All() { lock (seen) return seen.ToArray(); }
    public static void Stop() { if (timer != null) { timer.Dispose(); timer = null; } }
  }

  // New processes whose names are in a list (wsl.exe and the like), polled every 20 ms: a
  // process that lives under 20 ms can be missed
  public static class ProcessWatch {
    static Timer timer; static readonly List<ProcStart> seen = new List<ProcStart>(); static HashSet<uint> known = new HashSet<uint>();
    static HashSet<string> names;
    public static void Start(string[] watch) {
      names = new HashSet<string>(watch, StringComparer.OrdinalIgnoreCase);
      lock (seen) { seen.Clear(); known.Clear(); foreach (var p in Native.Snapshot()) known.Add(p.Pid); }
      if (timer != null) timer.Dispose();
      timer = new Timer(_ => Poll(), null, 0, 20);
    }
    static int polling;
    static void Poll() {
      if (Interlocked.Exchange(ref polling, 1) == 1) return;
      try {
        foreach (var p in SnapshotLight()) {
          lock (seen) {
            if (known.Contains(p.Pid)) continue;
            known.Add(p.Pid);
            if (names.Contains(p.Exe)) seen.Add(new ProcStart { At = Native.Now(), Pid = p.Pid, Parent = p.Parent, Exe = p.Exe, Path = Native.ExePath(p.Pid) });
          }
        }
      } finally { Interlocked.Exchange(ref polling, 0); }
    }
    // Without creation times: cheaper, polled often
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr snap, ref PE e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr snap, ref PE e);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] struct PE {
      public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID;
      public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags;
      [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }
    static List<ProcInfo> SnapshotLight() {
      var list = new List<ProcInfo>();
      IntPtr snap = CreateToolhelp32Snapshot(2, 0);
      if (snap == IntPtr.Zero || snap == new IntPtr(-1)) return list;
      try {
        var e = new PE(); e.dwSize = (uint)Marshal.SizeOf(typeof(PE));
        if (Process32FirstW(snap, ref e)) { do { list.Add(new ProcInfo { Pid = e.th32ProcessID, Parent = e.th32ParentProcessID, Exe = e.szExeFile }); } while (Process32NextW(snap, ref e)); }
      } finally { CloseHandle(snap); }
      return list;
    }
    public static ProcStart[] All() { lock (seen) return seen.ToArray(); }
    public static void Stop() { if (timer != null) { timer.Dispose(); timer = null; } }
  }

  // The benchmark's text, written once into files the producer sends as they are. Every pattern
  // is deterministic, so every terminal gets the same bytes. Each returns bytes and line feeds.
  public static class Data {
    static void Line(Stream s, StringBuilder sb, ref long bytes, ref long lines) {
      var b = Encoding.UTF8.GetBytes(sb.ToString()); s.Write(b, 0, b.Length); bytes += b.Length;
      foreach (var c in b) if (c == 10) lines++;
      sb.Clear();
    }
    static FileStream Open(string path) { return new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.Read, 1 << 20); }

    // `width` printable ASCII characters a line (spaces too), CRLF
    public static long[] Plain(string path, long target, int width) {
      long bytes = 0, lines = 0; var sb = new StringBuilder();
      using (var f = Open(path)) {
        while (bytes < target) {
          for (int i = 0; i < width; i++) sb.Append((char)(32 + (int)((lines * 7 + i * 3) % 95)));
          sb.Append("\r\n"); Line(f, sb, ref bytes, ref lines);
        }
      }
      return new[] { bytes, lines };
    }

    // `cells` cells a line, each with its own SGR: a 256-colour foreground and background and, in
    // turn, none, bold, italic or underline; reset at the end of the line
    public static long[] Sgr(string path, long target, int cells) {
      long bytes = 0, lines = 0; var sb = new StringBuilder(); int[] attrs = { 0, 1, 3, 4 };
      using (var f = Open(path)) {
        while (bytes < target) {
          for (int i = 0; i < cells; i++) {
            long n = lines * cells + i;
            sb.Append("\x1b[").Append(attrs[n % 4]).Append(";38;5;").Append((n * 7) % 256).Append(";48;5;").Append((n * 13 + 100) % 256).Append('m');
            sb.Append((char)(33 + (int)(n % 94)));
          }
          sb.Append("\x1b[0m\r\n"); Line(f, sb, ref bytes, ref lines);
        }
      }
      return new[] { bytes, lines };
    }

    // Lines `width` columns wide of wide CJK and Hangul, emoji (each two columns wide in every
    // Unicode version since 9), accented and combining Latin, Greek, Cyrillic and box drawing
    public static long[] Unicode(string path, long target, int width) {
      string[] wide = { "漢", "字", "表", "示", "中", "文", "日", "本", "語", "한", "국", "어", "テ", "ス", "ト" };
      string[] emoji = { "\U0001F600", "\U0001F680", "\U0001F389", "\U0001F30F", "\U0001F525", "\U0001F4BB", "\U0001F40D", "\U0001F355" };
      string[] narrow = { "α", "β", "γ", "δ", "Ж", "З", "И", "é", "ü", "ñ", "ç", "e\u0301", "a\u0308", "─", "│", "┌", "┐", "└", "┘", "├", "┤", "€", "£" };
      long bytes = 0, lines = 0; var sb = new StringBuilder(); long k = 0;
      using (var f = Open(path)) {
        while (bytes < target) {
          int col = 0;
          while (col < width) {
            long pick = (k * 2654435761L) % 7; k++;
            if (pick < 2 && col + 2 <= width) { sb.Append(wide[k % wide.Length]); col += 2; }
            else if (pick == 2 && col + 2 <= width) { sb.Append(emoji[k % emoji.Length]); col += 2; }
            else if (pick == 3) { sb.Append(' '); col += 1; }
            else { sb.Append(narrow[k % narrow.Length]); col += 1; }
          }
          sb.Append("\r\n"); Line(f, sb, ref bytes, ref lines);
        }
      }
      return new[] { bytes, lines };
    }

    // vtebench's kind of load: a scrolling region (DECSTBM) set inside the screen and lines
    // scrolled through it from its bottom, then the region reset and a burst of cursor moves (CUP)
    // to places across the screen, each writing a few characters; again and again
    public static long[] Vte(string path, long target, int cols, int rows) {
      long bytes = 0, lines = 0; var sb = new StringBuilder(); long seed = 12345; int top = 3, bottom = rows - 2;
      Func<int, int> next = m => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return (int)(seed % m); };
      using (var f = Open(path)) {
        while (bytes < target) {
          sb.Append("\x1b[").Append(top).Append(';').Append(bottom).Append('r');
          sb.Append("\x1b[").Append(bottom).Append(";1H");
          for (int i = 0; i < 40; i++) {
            for (int c = 0; c < cols - 20; c++) sb.Append((char)(33 + (int)((lines + c) % 94)));
            sb.Append("\r\n"); Line(f, sb, ref bytes, ref lines);
          }
          sb.Append("\x1b[r");
          for (int i = 0; i < 60; i++) {
            sb.Append("\x1b[").Append(1 + next(rows)).Append(';').Append(1 + next(cols - 8)).Append('H').Append("vtebench");
          }
          Line(f, sb, ref bytes, ref lines);
        }
        sb.Append("\x1b[r\x1b[").Append(rows).Append(";1H\r\n"); Line(f, sb, ref bytes, ref lines);
      }
      return new[] { bytes, lines };
    }
  }
}
'@
}

#region Screens and the clock

# The right-most screen's bounds (the far-right portrait screen here), where every window opens
function Get-BenchScreen {
  $s = [System.Windows.Forms.Screen]::AllScreens | Sort-Object { $_.Bounds.Right } | Select-Object -Last 1
  $s.Bounds
}

# Where an LBTerm window opens (`--position`, which counts from the virtual desktop's top-left: the
# harness's $TestWindowPosition), and Windows Terminal's `--pos` (screen coordinates): both 20 px
# in and 120 px down from the right-most screen's top-left
function Get-BenchPositions {
  $b = Get-BenchScreen
  $v = [System.Windows.Forms.SystemInformation]::VirtualScreen
  [pscustomobject]@{
    Screen   = $b
    Lbterm   = '{0},{1}' -f ($b.X - $v.X + 20), ($b.Y - $v.Y + 120)
    Wt       = '{0},{1}' -f ($b.X + 20), ($b.Y + 120)
  }
}

function Get-Ticks { [LbtermBench.Native]::Now() }
function ConvertTo-Ms([long]$Ticks) { [Math]::Round($Ticks / 10000.0, 1) }

#endregion

#region What existed before: never ended, never measured

$script:Protected = $null

# Every process running when the benchmark starts, by pid and creation time: nothing here ends
# one of them (Stop-BenchTerminal refuses), whatever a pid lookup says later
function Protect-Existing {
  $script:Protected = @{}
  foreach ($p in [LbtermBench.Native]::Snapshot()) { $script:Protected[[uint32]$p.Pid] = [long]$p.Created }
}

function Test-Protected([uint32]$ProcessId) {
  if (-not $script:Protected) { throw 'Protect-Existing first' }
  if (-not $script:Protected.ContainsKey($ProcessId)) { return $false }
  # The same pid made again since is a new process
  $created = [LbtermBench.Native]::Created($ProcessId)
  $created -eq 0 -or $created -eq $script:Protected[$ProcessId]
}

#endregion

#region The build, the launcher

# A copy of a build's top-level files in $To (not its folder: the installed one is Lachlan's daily
# LBTerm to Warden and the log reader), without the .old files an install leaves; files already
# there and unchanged are kept. Its folder.
function Copy-BenchBuild([Parameter(Mandatory)][string]$From, [Parameter(Mandatory)][string]$To) {
  New-Item -ItemType Directory -Force $To | Out-Null
  foreach ($f in Get-ChildItem -LiteralPath $From -File | Where-Object Name -notlike '*.old') {
    $dest = Join-Path $To $f.Name
    if (-not (Test-Path $dest) -or (Get-Item $dest).Length -ne $f.Length -or (Get-Item $dest).LastWriteTimeUtc -ne $f.LastWriteTimeUtc) { Copy-Item -LiteralPath $f.FullName $dest -Force }
  }
  (Resolve-Path $To).Path
}

# The launcher (BenchLaunch.cs) built in $Data with the .NET Framework's csc.exe, once: a
# GUI-subsystem exe, so Start-Unfocused starts it from WMI with no console. Its path.
function Install-BenchLauncher([Parameter(Mandatory)][string]$Data) {
  $exe = Join-Path $Data 'BenchLaunch.exe'
  $src = Join-Path $script:BenchRoot 'BenchLaunch.cs'
  if (-not (Test-Path $exe) -or (Get-Item $exe).LastWriteTimeUtc -lt (Get-Item $src).LastWriteTimeUtc) {
    $csc = Join-Path $env:SystemRoot 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    $said = & $csc /nologo /target:winexe "/out:$exe" $src 2>&1
    if ($LASTEXITCODE) { throw "csc couldn't build the launcher: $said" }
  }
  $exe
}

#endregion

#region Configs

# The benchmark's LBTerm config: Windows Terminal's default font and size (Cascadia Mono, 12 pt),
# the same columns and rows, the same scrollback, WebGpu, no update check, no WSL or mux domains
# (a mux domain only in -Mux's), no reloading, no crash restore.
function New-BenchConfig {
  param(
    [Parameter(Mandatory)][string]$Path,
    [int]$Cols = 100, [int]$Rows = 30, [int]$Scrollback = 9001,
    [Parameter(Mandatory)][string]$Cwd,
    [switch]$Mux, [string]$Socket, [string]$MuxServer, [string]$Domain = 'benchmux',
    # What the first tab runs (default_prog): a mux server starts its first tab itself, with this
    [string[]]$Program = @($script:BenchPwsh, '-NoProfile'),
    # More fields, as Lua table entries (`lbterm_triggers = {},`): bench.ps1's -LbtermLua
    [string]$Lua = ''
  )
  New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
  # Not $lua: PowerShell's names ignore case, and the [string]$Lua parameter would take it as text
  $toLuaPath = { param($s) $s -replace '\\', '/' }
  $lines = [Collections.Generic.List[string]]::new()
  $lines.Add("local wezterm = require 'wezterm'")
  $lines.Add('return {')
  $lines.Add("  font = wezterm.font('Cascadia Mono'),")
  $lines.Add('  font_size = 12.0,')
  $lines.Add("  initial_cols = $Cols,")
  $lines.Add("  initial_rows = $Rows,")
  $lines.Add("  scrollback_lines = $Scrollback,")
  $lines.Add("  front_end = 'WebGpu',")
  $lines.Add('  check_for_updates = false,')
  $lines.Add('  automatically_reload_config = false,')
  $lines.Add('  wsl_domains = {},')
  $lines.Add("  window_close_confirmation = 'NeverPrompt',")
  $lines.Add("  default_prog = { $(($Program | ForEach-Object { "'$(& $toLuaPath $_)'" }) -join ', ') },")
  $lines.Add("  default_cwd = '$(& $toLuaPath $Cwd)',")
  $lines.Add('  lbterm_crash_restore = false,')
  if ($Lua) { foreach ($l in $Lua -split "`n") { if ($l.Trim()) { $lines.Add("  $($l.Trim())") } } }
  if ($Mux) {
    $lines.Add("  unix_domains = { { name = '$Domain', socket_path = '$(& $toLuaPath $Socket)',")
    $lines.Add("    serve_command = { '$(& $toLuaPath $MuxServer)', '--config-file', '$(& $toLuaPath $Path)', '--daemonize' } } },")
  } else {
    $lines.Add('  unix_domains = {},')
  }
  $lines.Add('}')
  [IO.File]::WriteAllText($Path, ($lines -join "`n") + "`n")
  $Path
}

#endregion

#region Launching, finding and closing a terminal

# Quotes one argument for a command line (the harness's rules)
function Join-BenchArgs([string[]]$Items) { ($Items | ForEach-Object { ConvertTo-CommandLineArg $_ }) -join ' ' }

# Starts one terminal of $Kind, unfocused, on the right-most screen, its first tab running
# $Program (Windows Terminal's further tabs, -More, through its own command line; LBTerm's are
# spawned by the cli once it shows, bench.ps1's Add-BenchTabs). Each launch writes a
# config of its own (New-BenchConfig, -Scrollback lines). The launcher (BenchLaunch.cs) starts it;
# meanwhile this waits for its window, every millisecond. What it returns says when the launch
# began (Launch: DateTime ticks just before CreateProcess), when the process was created
# (LaunchCreated), when the window showed (Visible), the GUI's pid and window, and where it is.
function Start-BenchTerminal {
  param(
    [Parameter(Mandatory)][ValidateSet('lbterm', 'lbterm-mux', 'wt')][string]$Kind,
    [Parameter(Mandatory)][object]$Setup,
    [Parameter(Mandatory)][string[]]$Program,
    [object[]]$More = @(),
    [int]$Scrollback = 0,
    [int]$TimeoutSec = 30
  )
  $pos = Get-BenchPositions
  $cwd = $Setup.Cwd
  $script:LaunchCount++
  $n = $script:LaunchCount
  $sb = if ($Scrollback) { $Scrollback } else { $Setup.Scrollback }
  $common = @{ Cols = $Setup.Cols; Rows = $Setup.Rows; Scrollback = $sb; Cwd = $cwd }
  $lbtermLua = if ($Setup.PSObject.Properties['LbtermLua']) { $Setup.LbtermLua } else { '' }
  switch ($Kind) {
    'lbterm' {
      $exe = Join-Path $Setup.Bin 'lbterm-gui.exe'
      $cfg = New-BenchConfig -Path (Join-Path $Setup.ConfigDir "lbterm-$n.lua") @common -Lua $lbtermLua
      $arguments = "--config-file $(ConvertTo-CommandLineArg $cfg) start --always-new-process --position $($pos.Lbterm) --class $($Setup.Class) --cwd $(ConvertTo-CommandLineArg $cwd) -- $(Join-BenchArgs $Program)"
      $guiExe = 'lbterm-gui.exe'; $class = $Setup.Class
    }
    'lbterm-mux' {
      if (@(Get-BenchServers $Setup).Count) { throw 'a benchmark mux server is still running before a lbterm-mux launch' }
      $exe = Join-Path $Setup.Bin 'lbterm-gui.exe'
      # The server starts its first tab itself, with default_prog, and the GUI shows it
      $cfg = New-BenchConfig -Path (Join-Path $Setup.ConfigDir "lbterm-mux-$n.lua") @common -Mux -Socket $Setup.Socket -MuxServer (Join-Path $Setup.Bin 'lbterm-mux-server.exe') -Program $Program -Lua $lbtermLua
      $arguments = "--config-file $(ConvertTo-CommandLineArg $cfg) connect benchmux --position $($pos.Lbterm) --class $($Setup.Class)"
      $guiExe = 'lbterm-gui.exe'; $class = $Setup.Class
    }
    'wt' {
      $exe = $script:WtAlias
      # Only a WindowsTerminal.exe of the benchmark's own may be measured: one already running
      # unelevated would take the new window into its process
      $running = @(Get-Process WindowsTerminal -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
      $unelevated = @($running | Where-Object { -not (Test-ElevatedProcess $_) })
      if ($unelevated.Count) { throw "a Windows Terminal runs unelevated already (pid $($unelevated -join ', ')): a new wt.exe would open its window in that process, so this launch is skipped" }
      $tabs = @("new-tab --profile $(ConvertTo-CommandLineArg $Setup.WtProfile) --title bench --startingDirectory $(ConvertTo-CommandLineArg $cwd) -- $(Join-BenchArgs $Program)")
      foreach ($m in $More) { $tabs += "new-tab --profile $(ConvertTo-CommandLineArg $Setup.WtProfile) --startingDirectory $(ConvertTo-CommandLineArg $cwd) -- $(Join-BenchArgs $m)" }
      $arguments = "-w new --pos $($pos.Wt) --size $($Setup.Cols),$($Setup.Rows) " + ($tabs -join ' ; ')
      $guiExe = 'WindowsTerminal.exe'; $class = 'CASCADIA_HOSTING_WINDOW_CLASS'; $cfg = $null
    }
  }
  # Every process of the GUI's file running now: none of them is this launch's
  $pre = [uint32[]]@(Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($guiExe)) -ErrorAction SilentlyContinue | ForEach-Object { [uint32]$_.Id })
  $req = Join-Path $Setup.Launches "launch-$n.txt"
  [IO.File]::WriteAllLines($req, [string[]]@($exe, $cwd, $arguments))
  $fgBefore = [LbtermBench.Native]::ForegroundPid()
  $since = Get-TicksNow
  $launcher = Start-Unfocused -Exe $Setup.Launcher -Arguments (ConvertTo-CommandLineArg $req) -WorkingDirectory $cwd
  $hit = [LbtermBench.Native]::WaitWindow(0, $guiExe, $class, $pre, $TimeoutSec * 1000)
  # What the launcher says; a window that showed is still this launch's to close when it says nothing
  $said = if (Wait-BenchFile "$req.out" 10) { (Get-Content -Raw "$req.out").Trim() } else { 'error 0 the launcher wrote nothing' }
  $f = if ($said -like 'error*') { @(0, 0, 0, 0) } else { $said -split ' ' }
  $term = [pscustomobject]@{
    Kind = $Kind; N = $n; LaunchPid = [uint32]$f[0]; Launch = [long]$f[1]; LaunchReturned = [long]$f[2]; LaunchCreated = [long]$f[3]
    Since = $since; Visible = $(if ($hit) { $hit.At } else { 0 }); GuiPid = $(if ($hit) { $hit.Pid } else { 0 }); Hwnd = $(if ($hit) { $hit.Hwnd } else { 0 })
    WindowClass = $(if ($hit) { $hit.Class } else { '' }); ServerPid = 0; Pids = [Collections.Generic.HashSet[uint32]]::new()
    ForegroundBefore = $fgBefore; Violation = $null; Placement = $null; Arguments = $arguments; Config = $cfg
    Error = $(if ($said -like 'error*') { "the launcher: $said" })
  }
  if ($term.LaunchPid) { [void]$term.Pids.Add($term.LaunchPid); [void]$global:BenchPids.Add($term.LaunchPid) }
  [void]$global:BenchPids.Add([uint32]$launcher)
  if (-not $hit) {
    # The GUI it started, if it's running: closed with the rest
    if ($Kind -ne 'wt') { $term.GuiPid = $term.LaunchPid }
    $term.Violation = "no window within $TimeoutSec s"
    return $term
  }
  [void]$term.Pids.Add([uint32]$hit.Pid)
  [void]$global:BenchPids.Add([uint32]$hit.Pid)
  # Ended by the watcher the moment it gets the foreground, unless the caller gives it the
  # foreground itself (key-latency.ps1 -Run sets $script:BenchGuard to $false)
  if ($script:BenchGuard -ne $false -and -not (Test-Protected ([uint32]$hit.Pid))) { [LbtermBench.ForegroundWatch]::Guard([uint32]$hit.Pid) }
  if ($Kind -ne 'wt' -and $hit.Pid -ne $term.LaunchPid) {
    $term.Violation = "the window that showed is process $($hit.Pid)'s, not the GUI the launcher started ($($term.LaunchPid))"
    return $term
  }
  if ($Kind -eq 'wt' -and (Test-Protected $hit.Pid)) {
    $term.Violation = "the Windows Terminal window showed in a process that was running before the benchmark (pid $($hit.Pid))"
    $term.GuiPid = 0
    return $term
  }
  if ($Kind -like 'lbterm*') {
    # The harness's own list: its window check takes the window for the test's, and it closes it
    Add-TestWindow -ProcessId $hit.Pid -Like ([pscustomobject]@{ Class = $Setup.Class; Config = $cfg; Domain = $(if ($Kind -eq 'lbterm-mux') { 'benchmux' }); ServerSocket = $Setup.Socket; StateFile = $null }) -TimeoutSec 5 | Out-Null
  }
  Test-BenchPlacement $term
  $term
}

# DateTime ticks now (the name keeps it apart from the harness's)
function Get-TicksNow { [LbtermBench.Native]::Now() }

# Whether a process runs elevated: its token's elevation; one whose token can't be read from
# here counts as elevated (an unelevated process can't read an elevated one's)
function Test-ElevatedProcess([uint32]$ProcessId) { [LbtermBench.Native]::Elevated($ProcessId) -ne 0 }

# Whether the window is on the right-most screen and nowhere else, and whether the foreground
# moved to any process of the terminal: checked as the window shows and again 500 ms after. The
# first thing wrong is the terminal's Violation, and bench.ps1 stops using that kind of terminal.
function Test-BenchPlacement($Term) {
  $screen = Get-BenchScreen
  $others = @([System.Windows.Forms.Screen]::AllScreens | Where-Object { $_.Bounds -ne $screen } | ForEach-Object Bounds)
  foreach ($when in 'shown', '500 ms after') {
    if ($when -ne 'shown') { Start-Sleep -Milliseconds 500 }
    $h = [IntPtr][long]$Term.Hwnd
    if (-not [LbtermBench.Native]::IsWindow($h)) { $Term.Violation = "its window went ($when)"; return }
    $r = [LbtermBench.Native]::Frame($h)
    $rect = [System.Drawing.Rectangle]::FromLTRB($r.Left, $r.Top, $r.Right, $r.Bottom)
    $Term.Placement = [pscustomobject]@{ Left = $r.Left; Top = $r.Top; Width = $r.Right - $r.Left; Height = $r.Bottom - $r.Top }
    $off = @($others | Where-Object { $_.IntersectsWith($rect) })
    if (-not $screen.Contains([System.Drawing.Point]::new([int](($r.Left + $r.Right) / 2), [int](($r.Top + $r.Bottom) / 2))) -or $off.Count) {
      $Term.Violation = "its window is at $($r.Left),$($r.Top) $($rect.Width)x$($rect.Height) ($when), not on the right-most screen alone ($($screen.X),$($screen.Y) $($screen.Width)x$($screen.Height))"
      return
    }
    # Not while the guard is off: the caller gives the terminal the foreground itself
    # (key-latency.ps1 -Run) or lets it take it (bench.ps1 -Away)
    if ($script:BenchGuard -ne $false) {
      $fg = Test-BenchForeground $Term
      if ($fg) { $Term.Violation = $fg; return }
    }
  }
}

# Whether the foreground ever went to the terminal's processes since it was launched: a line
# saying when and from what, or nothing. It starts CLICKED when the pointer was on the window and
# real input came within 250 ms before (Lachlan clicked it), else VIOLATION (the terminal took it,
# or Windows gave it the foreground as his own window went). Either way the watcher has ended it.
function Test-BenchForeground($Term) {
  if ($Term.GuiPid -and [LbtermBench.Native]::Alive([uint32]$Term.GuiPid)) { Update-BenchPids $Term }
  foreach ($c in [LbtermBench.ForegroundWatch]::Since($Term.Since)) {
    if ($Term.Pids.Contains([uint32]$c.Pid)) {
      $prev = if ($c.PrevPid) { (Get-Process -Id $c.PrevPid -ErrorAction SilentlyContinue).ProcessName } else { '?' }
      $what = "the foreground went to its process $($c.Pid) at $(([datetime]::new($c.At, 'Utc')).ToLocalTime().ToString('HH:mm:ss.fff')) from $prev ($($c.PrevPid)), $($c.IdleMs) ms after the last real key or mouse input, the pointer at $($c.X),$($c.Y)$(if ($c.PointerOn) { ' on' } else { ' off' }) the window$(if ($c.Killed) { '; ended at once' })"
      if ($c.PointerOn -and $c.IdleMs -lt 250) { return "CLICKED: $what" }
      return "VIOLATION: $what"
    }
  }
  $now = [LbtermBench.Native]::ForegroundPid()
  if ($Term.Pids.Contains([uint32]$now)) { return "VIOLATION: its process $now has the foreground" }
  $null
}

# The benchmark's mux servers: lbterm-mux-server.exe whose command line names its config folder
function Get-BenchServers($Setup) {
  @(Get-CimInstance Win32_Process -Filter "Name = 'lbterm-mux-server.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -and ($_.CommandLine -replace '/', '\') -like "*$($Setup.ConfigDir)\*" })
}

# The terminal's processes now: the GUI and everything it started, and on a mux domain the
# server and everything it started. Each with its role: gui, server, conpty (OpenConsole.exe),
# shell (whatever runs in a tab) or other.
function Get-BenchProcesses($Term, [object]$Setup) {
  $snap = [LbtermBench.Native]::Snapshot()
  $roots = [Collections.Generic.List[uint32]]::new()
  if ($Term.GuiPid) { $roots.Add([uint32]$Term.GuiPid) }
  if ($Term.Kind -eq 'lbterm-mux') {
    if (-not $Term.ServerPid -and $Setup) {
      $s = @(Get-BenchServers $Setup | Where-Object { -not (Test-Protected ([uint32]$_.ProcessId)) })
      if ($s.Count -eq 1) { $Term.ServerPid = [uint32]$s[0].ProcessId }
    }
    if ($Term.ServerPid) { $roots.Add([uint32]$Term.ServerPid) }
  }
  $all = @($snap | Where-Object { $roots -contains $_.Pid }) + @([LbtermBench.Native]::Descendants($snap, [uint32[]]$roots.ToArray()))
  foreach ($p in $all) {
    if (Test-Protected $p.Pid) { continue }
    $role = if ($p.Pid -eq $Term.GuiPid) { 'gui' }
    elseif ($p.Pid -eq $Term.ServerPid) { 'server' }
    elseif ($p.Exe -ieq 'OpenConsole.exe' -or $p.Exe -ieq 'conhost.exe') { 'conpty' }
    elseif ($p.Exe -ieq 'pwsh.exe' -or $p.Exe -ieq 'cmd.exe') { 'shell' }
    else { 'other' }
    [void]$Term.Pids.Add([uint32]$p.Pid)
    [pscustomobject]@{ Pid = [uint32]$p.Pid; Exe = $p.Exe; Role = $role; Created = $p.Created }
  }
}

function Update-BenchPids($Term) { if ($Term.GuiPid) { Get-BenchProcesses $Term $script:BenchSetup | Out-Null } }

# Ends the terminal: every process of its own, children first, by pid; never one that was
# running before the benchmark. Waits for them to go.
function Stop-BenchTerminal($Term, [object]$Setup) {
  if (-not $Term) { return }
  $procs = @(Get-BenchProcesses $Term $Setup)
  $pids = @($procs | Sort-Object { if ($_.Role -in 'gui', 'server') { 1 } else { 0 } } | ForEach-Object Pid) + @($Term.Pids)
  foreach ($p in ($pids | Select-Object -Unique)) {
    if ($p -eq 0 -or (Test-Protected $p)) { continue }
    Stop-Process -Id $p -Force -ErrorAction SilentlyContinue
  }
  $deadline = (Get-Date).AddSeconds(10)
  foreach ($p in ($pids | Select-Object -Unique)) {
    while ((Get-Date) -lt $deadline -and -not (Test-Protected $p) -and [LbtermBench.Native]::Alive($p)) { Start-Sleep -Milliseconds 50 }
  }
  if ($Term.Kind -eq 'lbterm-mux' -and $Setup) {
    foreach ($s in Get-BenchServers $Setup) { if (-not (Test-Protected ([uint32]$s.ProcessId))) { Stop-Process -Id $s.ProcessId -Force -ErrorAction SilentlyContinue } }
  }
}

#endregion

#region Measuring

# CPU time of each of the terminal's processes, now: role and 100 ns units by pid
function Get-BenchCpu($Term, [object]$Setup) {
  $procs = @(Get-BenchProcesses $Term $Setup)
  $times = [LbtermBench.Native]::Cpu([uint32[]]@($procs | ForEach-Object Pid))
  $r = @{}
  for ($i = 0; $i -lt $procs.Count; $i++) { if ($times[$i] -ge 0) { $r[$procs[$i].Pid] = [pscustomobject]@{ Role = $procs[$i].Role; Exe = $procs[$i].Exe; Cpu = $times[$i] } } }
  [pscustomobject]@{ At = Get-TicksNow; ByPid = $r }
}

# The CPU used between two Get-BenchCpu readings, in seconds, by role (gui, server, conpty, shell,
# other) and terminal (gui + server + conpty: what the terminal costs, the programs in it apart).
# A process that started in between counts from nothing; one gone by the second isn't counted.
function Measure-BenchCpu($A, $B) {
  $roles = [ordered]@{ gui = 0.0; server = 0.0; conpty = 0.0; shell = 0.0; other = 0.0 }
  foreach ($k in $B.ByPid.Keys) {
    $before = if ($A.ByPid.ContainsKey($k)) { $A.ByPid[$k].Cpu } else { 0 }
    $roles[$B.ByPid[$k].Role] += ($B.ByPid[$k].Cpu - $before) / 1e7
  }
  $secs = ($B.At - $A.At) / 1e7
  $terminal = $roles.gui + $roles.server + $roles.conpty
  [pscustomobject]@{
    Seconds = [Math]::Round($secs, 3)
    Terminal = [Math]::Round($terminal, 4); Gui = [Math]::Round($roles.gui, 4); Server = [Math]::Round($roles.server, 4)
    Conpty = [Math]::Round($roles.conpty, 4); Shell = [Math]::Round($roles.shell, 4); Other = [Math]::Round($roles.other, 4)
    # Of one core, over the span
    TerminalPercent = $(if ($secs -gt 0) { [Math]::Round(100 * $terminal / $secs, 2) } else { 0 })
  }
}

# The GUI's and the mux server's threads now (Get-BenchThreads), and the busiest of them between
# two readings: "<process> <thread name or id> <seconds>", most first
function Get-BenchThreads($Term) {
  $r = @{}
  foreach ($p in @(@('gui', $Term.GuiPid), @('server', $Term.ServerPid))) {
    if (-not $p[1]) { continue }
    foreach ($t in [LbtermBench.Threads]::Of([uint32]$p[1])) { $r["$($p[0]):$($t.Tid)"] = [pscustomobject]@{ Who = $p[0]; Name = $t.Name; Tid = $t.Tid; Cpu = $t.Cpu } }
  }
  $r
}
function Measure-BenchThreads($A, $B, [int]$Top = 5) {
  @($B.Keys | ForEach-Object {
      $before = if ($A.ContainsKey($_)) { $A[$_].Cpu } else { 0 }
      [pscustomobject]@{ Who = $B[$_].Who; Name = $(if ($B[$_].Name) { $B[$_].Name } else { "#$($B[$_].Tid)" }); Seconds = ($B[$_].Cpu - $before) / 1e7 }
    } | Sort-Object Seconds -Descending | Select-Object -First $Top | Where-Object Seconds -gt 0.01 |
    ForEach-Object { '{0} {1} {2:N2}' -f $_.Who, $_.Name, $_.Seconds })
}

# Private bytes and working set of the terminal's processes, in MB, by role and summed for the
# terminal (gui + server + conpty)
function Measure-BenchMemory($Term, [object]$Setup) {
  $procs = @(Get-BenchProcesses $Term $Setup)
  $mem = [LbtermBench.Native]::Memory([uint32[]]@($procs | ForEach-Object Pid))
  $byRole = @{}
  foreach ($m in $mem) {
    $role = ($procs | Where-Object Pid -eq $m.Pid | Select-Object -First 1).Role
    if (-not $byRole.ContainsKey($role)) { $byRole[$role] = [pscustomobject]@{ Private = 0.0; WorkingSet = 0.0; Count = 0 } }
    $byRole[$role].Private += $m.Private / 1MB; $byRole[$role].WorkingSet += $m.WorkingSet / 1MB; $byRole[$role].Count++
  }
  $sum = { param($field) $t = 0.0; foreach ($r in 'gui', 'server', 'conpty') { if ($byRole.ContainsKey($r)) { $t += $byRole[$r].$field } }; [Math]::Round($t, 1) }
  $round = { param($r, $f) if ($byRole.ContainsKey($r)) { [Math]::Round($byRole[$r].$f, 1) } else { 0 } }
  [pscustomobject]@{
    PrivateMB = & $sum 'Private'; WorkingSetMB = & $sum 'WorkingSet'
    GuiPrivateMB = & $round 'gui' 'Private'; GuiWorkingSetMB = & $round 'gui' 'WorkingSet'
    ServerPrivateMB = & $round 'server' 'Private'; ServerWorkingSetMB = & $round 'server' 'WorkingSet'
    ConptyPrivateMB = & $round 'conpty' 'Private'; ConptyWorkingSetMB = & $round 'conpty' 'WorkingSet'; Conptys = $(if ($byRole.ContainsKey('conpty')) { $byRole.conpty.Count } else { 0 })
    ShellPrivateMB = & $round 'shell' 'Private'; Shells = $(if ($byRole.ContainsKey('shell')) { $byRole.shell.Count } else { 0 })
  }
}

# Waits until the terminal's processes have used under $Percent of a core over $WindowMs (read
# every 100 ms), up to $TimeoutSec: how long it took, and whether it got there
function Wait-BenchQuiet($Term, [object]$Setup, [double]$Percent = 2, [int]$WindowMs = 500, [double]$TimeoutSec = 30) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  $readings = [Collections.Generic.List[object]]::new()
  while ($clock.Elapsed.TotalSeconds -lt $TimeoutSec) {
    $readings.Add((Get-BenchCpu $Term $Setup))
    $last = $readings[$readings.Count - 1]
    for ($i = $readings.Count - 2; $i -ge 0; $i--) {
      if (($last.At - $readings[$i].At) / 1e4 -ge $WindowMs) {
        $c = Measure-BenchCpu $readings[$i] $last
        if ($c.TerminalPercent -lt $Percent) { return [pscustomobject]@{ Ms = [Math]::Round($clock.Elapsed.TotalMilliseconds); Quiet = $true } }
        break
      }
    }
    Start-Sleep -Milliseconds 100
  }
  [pscustomobject]@{ Ms = [Math]::Round($clock.Elapsed.TotalMilliseconds); Quiet = $false }
}

# Waits for a file to exist, every 10 ms, up to $TimeoutSec: whether it did
function Wait-BenchFile([string]$Path, [double]$TimeoutSec = 30) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt $TimeoutSec) {
    if (Test-Path -LiteralPath $Path) { return $true }
    Start-Sleep -Milliseconds 10
  }
  Test-Path -LiteralPath $Path
}

# What else is running: the CPU the whole PC used over a second (every process but the idle
# ones), the processes that used most of it, and the PC's own clock (turbo off: 3.0 GHz)
function Get-BenchLoad {
  $a = @{}; foreach ($p in Get-Process) { try { $a[$p.Id] = @($p.ProcessName, $p.TotalProcessorTime.TotalSeconds) } catch { } }
  Start-Sleep -Seconds 1
  $use = foreach ($p in Get-Process) {
    try { if ($a.ContainsKey($p.Id)) { [pscustomobject]@{ Name = $p.ProcessName; Pid = $p.Id; Cores = [Math]::Round($p.TotalProcessorTime.TotalSeconds - $a[$p.Id][1], 2) } } } catch { }
  }
  $top = @($use | Where-Object Cores -gt 0.05 | Sort-Object Cores -Descending | Select-Object -First 8)
  [pscustomobject]@{
    At = (Get-Date).ToString('o')
    BusyCores = [Math]::Round((($use | Measure-Object Cores -Sum).Sum), 2)
    Top = @($top | ForEach-Object { "$($_.Name) $($_.Pid) $($_.Cores)" })
  }
}

#endregion
