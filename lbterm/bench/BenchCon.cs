// The producer's console calls (producer.ps1): bench.ps1 compiles this once into BenchCon.dll
// beside the data, so a producer starting in a terminal loads it rather than compiling it there.
// Output goes through WriteFile on the console handle, as `type` or `cat` would send it: the
// least a program can add of its own.
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace LbtermBenchCon {
  public static class Con {
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int n);
    [DllImport("kernel32.dll")] static extern bool GetConsoleMode(IntPtr h, out uint m);
    [DllImport("kernel32.dll")] static extern bool SetConsoleMode(IntPtr h, uint m);
    [DllImport("kernel32.dll")] static extern bool SetConsoleOutputCP(uint cp);
    [DllImport("kernel32.dll")] static extern uint GetConsoleOutputCP();
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool WriteFile(IntPtr h, IntPtr buf, int n, out int written, IntPtr overlapped);
    [StructLayout(LayoutKind.Sequential)] struct COORD { public short X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct SMALL_RECT { public short Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct CSBI { public COORD Size; public COORD Cursor; public ushort Attr; public SMALL_RECT Window; public COORD Max; }
    [DllImport("kernel32.dll")] static extern bool GetConsoleScreenBufferInfo(IntPtr h, out CSBI info);

    static IntPtr Out { get { return GetStdHandle(-11); } }

    // UTF-8 output and VT processing on (ENABLE_PROCESSED_OUTPUT, ENABLE_WRAP_AT_EOL_OUTPUT,
    // ENABLE_VIRTUAL_TERMINAL_PROCESSING), the same in every terminal: the mode before and after,
    // the code page, and the window's columns and rows as the console reports them
    public static string Setup() {
      IntPtr h = Out; uint before, after;
      SetConsoleOutputCP(65001);
      GetConsoleMode(h, out before);
      SetConsoleMode(h, before | 0x1 | 0x2 | 0x4);
      GetConsoleMode(h, out after);
      CSBI i; GetConsoleScreenBufferInfo(h, out i);
      return string.Format("{{\"modeBefore\":{0},\"modeAfter\":{1},\"codePage\":{2},\"cols\":{3},\"rows\":{4}}}",
        before, after, GetConsoleOutputCP(), i.Window.Right - i.Window.Left + 1, i.Window.Bottom - i.Window.Top + 1);
    }

    public static void Write(string s) {
      var b = System.Text.Encoding.UTF8.GetBytes(s);
      Send(b, b.Length);
    }

    // Writes all of `data` in `chunk`-byte writes and times it: start and end (DateTime ticks,
    // UTC), the stopwatch's 100 ns units between, the bytes written and the writes
    public static long[] Send(byte[] data, int chunk) {
      IntPtr h = Out; long writes = 0, total = 0;
      var pin = GCHandle.Alloc(data, GCHandleType.Pinned);
      try {
        IntPtr p = pin.AddrOfPinnedObject();
        long start = DateTime.UtcNow.Ticks; var sw = Stopwatch.StartNew();
        while (total < data.Length) {
          int n = (int)Math.Min(chunk, data.Length - total), w;
          if (!WriteFile(h, p + (int)total, n, out w, IntPtr.Zero) || w <= 0) throw new Exception("WriteFile failed: " + Marshal.GetLastWin32Error());
          total += w; writes++;
        }
        sw.Stop(); long end = DateTime.UtcNow.Ticks;
        return new long[] { start, end, (long)(sw.Elapsed.TotalMilliseconds * 10000), total, writes };
      } finally { pin.Free(); }
    }
  }
}
