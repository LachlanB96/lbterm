// The benchmark's launcher (bench.ps1 compiles it once, with the .NET Framework's csc.exe, into a
// GUI-subsystem exe beside the data). Start-Unfocused starts it from WMI, outside any job and with
// no console; it starts the terminal a request file names with CreateProcess, shown without
// activating (STARTF_USESHOWWINDOW, SW_SHOWNOACTIVATE), and writes when it did.
//
// Why a launcher of its own: Windows Terminal is started through its app execution alias (wt.exe
// under %LOCALAPPDATA%\Microsoft\WindowsApps), which Win32_Process.Create can't start (it fails
// with 8), while CreateProcess from a process of the user's starts it as a shell would. A console
// program can't do it unseen: started by WMI without a console (DETACHED_PROCESS) pwsh never
// runs, WMI refuses CREATE_NO_WINDOW, and a console it made would open in front (Windows
// Terminal is the default terminal here). Every terminal goes through it, so "the launch" is the
// same moment for all of them: just before CreateProcess.
//
//   BenchLaunch.exe <request file>
//     request: line 1 the exe, line 2 the working folder, line 3 the arguments as they go on the
//              command line after the quoted exe
//     <request>.out: "<pid> <before> <after> <created>": the new process's id, DateTime ticks (UTC)
//              just before and after CreateProcess, and its creation time from GetProcessTimes;
//              or "error <code> <message>"
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

static class BenchLaunch {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct STARTUPINFO {
    public int cb; public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
  }
  [StructLayout(LayoutKind.Sequential)]
  struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
  [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long create, out long exit, out long kernel, out long user);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern void GetSystemTimePreciseAsFileTime(out long t);

  static long Now() { long t; GetSystemTimePreciseAsFileTime(out t); return DateTime.FromFileTimeUtc(t).Ticks; }

  static int Main(string[] args) {
    string request = args[0], output = request + ".out";
    try {
      string[] lines = File.ReadAllLines(request);
      string exe = lines[0], cwd = lines[1], arguments = lines.Length > 2 ? lines[2] : "";
      var si = new STARTUPINFO();
      si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
      si.dwFlags = 1;        // STARTF_USESHOWWINDOW
      si.wShowWindow = 4;    // SW_SHOWNOACTIVATE
      var cmd = new StringBuilder("\"" + exe + "\" " + arguments);
      PROCESS_INFORMATION pi;
      long before = Now();
      bool ok = CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, cwd, ref si, out pi);
      long after = Now();
      if (!ok) { File.WriteAllText(output, "error " + Marshal.GetLastWin32Error() + " CreateProcess failed for " + exe); return 1; }
      long created, exit, kernel, user;
      long createdTicks = GetProcessTimes(pi.hProcess, out created, out exit, out kernel, out user) ? DateTime.FromFileTimeUtc(created).Ticks : 0;
      CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
      File.WriteAllText(output + ".tmp", pi.dwProcessId + " " + before + " " + after + " " + createdTicks);
      if (File.Exists(output)) File.Delete(output);
      File.Move(output + ".tmp", output);
      return 0;
    } catch (Exception e) {
      File.WriteAllText(output, "error 0 " + e.Message);
      return 1;
    }
  }
}
