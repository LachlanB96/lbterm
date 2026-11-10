using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;

namespace LbMem {
  public class Alloc {
    public ulong Base;
    public ulong Reserved;      // whole allocation's span of MEM_COMMIT + MEM_RESERVE
    public ulong Committed;
    public ulong Resident;      // pages in the working set
    public ulong ResidentShared; // of those, shared (image/mapped not yet written)
    public ulong ResidentPrivate;
    public uint Type;           // MEM_IMAGE 0x1000000, MEM_MAPPED 0x40000, MEM_PRIVATE 0x20000
    public string Kind = "";
    public string File = "";
    public string Detail = "";
    public ulong WriteCopyCommitted;
    public ulong WritableCommitted;
    public ulong ExecCommitted;
    public ulong GuardCommitted;
    public ulong ZeroResident;  // resident pages that read all zero (Scan)
    public ulong Scanned;
    public List<ulong[]> Ranges = new List<ulong[]>(); // committed ranges [start, size, protect]
  }

  public class Font { public ulong Address; public ulong Size; public string Tag; public string Owner; public int Tables; public string Name = ""; }
  public class Ref { public ulong Target; public string From; public int Count; }

  public class Snap {
    public uint Pid;
    public ulong PrivateUsage, WorkingSet, PeakWorkingSet, PagefileUsage, PeakPagefileUsage, PagedPool, NonPagedPool;
    public List<Alloc> Allocs = new List<Alloc>();
    public List<ulong> Heaps = new List<ulong>();
    public List<ulong> HeapFree = new List<ulong>();
    public List<Mem.Block> Blocks = new List<Mem.Block>();
    public ulong HeapBusy, HeapBusyCount, HeapFreeWalked, HeapWalkStops;
    public ulong[] SmallBusy = new ulong[4];
    public ulong ProcessHeap;
    public List<string> Notes = new List<string>();
    public List<Font> Fonts = new List<Font>();
    public List<Ref> Refs = new List<Ref>();
    public Dictionary<uint, string> ThreadNames = new Dictionary<uint, string>();
  }

  public static class Native {
    [StructLayout(LayoutKind.Sequential)] public struct MBI {
      public ulong BaseAddress; public ulong AllocationBase; public uint AllocationProtect; public uint _a1;
      public ulong RegionSize; public uint State; public uint Protect; public uint Type; public uint _a2;
    }
    [StructLayout(LayoutKind.Sequential)] public struct WSX { public ulong VirtualAddress; public ulong Attributes; }
    [StructLayout(LayoutKind.Sequential)] public struct PMC {
      public uint cb; public uint PageFaultCount; public ulong PeakWorkingSetSize; public ulong WorkingSetSize;
      public ulong QuotaPeakPagedPoolUsage; public ulong QuotaPagedPoolUsage; public ulong QuotaPeakNonPagedPoolUsage;
      public ulong QuotaNonPagedPoolUsage; public ulong PagefileUsage; public ulong PeakPagefileUsage; public ulong PrivateUsage;
    }
    [StructLayout(LayoutKind.Sequential)] public struct PBI { public ulong ExitStatus; public ulong Peb; public ulong Affinity; public ulong BasePriority; public ulong Pid; public ulong Parent; }
    [StructLayout(LayoutKind.Sequential)] public struct TBI { public int ExitStatus; public int _p; public ulong Teb; public ulong Pid; public ulong Tid; public ulong Affinity; public int Priority; public int BasePriority; }

    [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr OpenThread(uint access, bool inherit, uint tid);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] public static extern IntPtr VirtualQueryEx(IntPtr h, IntPtr addr, out MBI mbi, IntPtr len);
    [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, IntPtr size, out IntPtr read);
    [DllImport("psapi.dll", CharSet = CharSet.Unicode)] public static extern uint GetMappedFileNameW(IntPtr h, IntPtr addr, StringBuilder sb, uint size);
    [DllImport("psapi.dll")] public static extern bool QueryWorkingSetEx(IntPtr h, [In, Out] WSX[] pv, uint cb);
    [DllImport("psapi.dll")] public static extern bool GetProcessMemoryInfo(IntPtr h, out PMC c, uint cb);
    [DllImport("ntdll.dll")] public static extern int NtQueryInformationProcess(IntPtr h, int cls, out PBI pbi, int len, out int ret);
    [DllImport("ntdll.dll")] public static extern int NtQueryInformationThread(IntPtr h, int cls, out TBI tbi, int len, out int ret);
    [DllImport("kernel32.dll")] public static extern int GetThreadDescription(IntPtr h, out IntPtr desc);
    [DllImport("kernel32.dll")] public static extern IntPtr LocalFree(IntPtr p);
  }

  public static class Mem {
    const uint MEM_COMMIT = 0x1000, MEM_RESERVE = 0x2000, MEM_FREE = 0x10000;
    public const uint MEM_PRIVATE = 0x20000, MEM_MAPPED = 0x40000, MEM_IMAGE = 0x1000000;
    const ulong PAGE = 4096;

    static ulong ReadU64(IntPtr h, ulong addr) {
      var b = new byte[8]; IntPtr n;
      if (!Native.ReadProcessMemory(h, (IntPtr)(long)addr, b, (IntPtr)8, out n) || (long)n != 8) return 0;
      return BitConverter.ToUInt64(b, 0);
    }
    static uint ReadU32(IntPtr h, ulong addr) {
      var b = new byte[4]; IntPtr n;
      if (!Native.ReadProcessMemory(h, (IntPtr)(long)addr, b, (IntPtr)4, out n) || (long)n != 4) return 0;
      return BitConverter.ToUInt32(b, 0);
    }
    static byte[] Read(IntPtr h, ulong addr, int len) {
      var b = new byte[len]; IntPtr n;
      if (!Native.ReadProcessMemory(h, (IntPtr)(long)addr, b, (IntPtr)len, out n)) return null;
      if ((long)n != len) Array.Resize(ref b, (int)(long)n);
      return b;
    }

    public static Snap Take(uint pid, bool scan) {
      // PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
      IntPtr h = Native.OpenProcess(0x0400 | 0x0010, false, pid);
      if (h == IntPtr.Zero) throw new Exception("OpenProcess " + pid + " failed: " + Marshal.GetLastWin32Error());
      try { return TakeImpl(h, pid, scan); } finally { Native.CloseHandle(h); }
    }

    static Snap TakeImpl(IntPtr h, uint pid, bool scan) {
      var s = new Snap { Pid = pid };
      Native.PMC pmc; Native.GetProcessMemoryInfo(h, out pmc, (uint)Marshal.SizeOf(typeof(Native.PMC)));
      s.PrivateUsage = pmc.PrivateUsage; s.WorkingSet = pmc.WorkingSetSize; s.PeakWorkingSet = pmc.PeakWorkingSetSize;
      s.PagefileUsage = pmc.PagefileUsage; s.PeakPagefileUsage = pmc.PeakPagefileUsage;
      s.PagedPool = pmc.QuotaPagedPoolUsage; s.NonPagedPool = pmc.QuotaNonPagedPoolUsage;

      // The address space, by allocation
      var byBase = new Dictionary<ulong, Alloc>();
      ulong addr = 0;
      Native.MBI m;
      int mbiSize = Marshal.SizeOf(typeof(Native.MBI));
      while (addr < 0x7FFFFFFFFFFF) {
        if (Native.VirtualQueryEx(h, (IntPtr)(long)addr, out m, (IntPtr)mbiSize) == IntPtr.Zero) break;
        if (m.State != MEM_FREE) {
          Alloc a;
          if (!byBase.TryGetValue(m.AllocationBase, out a)) {
            a = new Alloc { Base = m.AllocationBase, Type = m.Type };
            byBase[m.AllocationBase] = a; s.Allocs.Add(a);
          }
          a.Reserved += m.RegionSize;
          if (m.State == MEM_COMMIT) {
            a.Committed += m.RegionSize;
            a.Ranges.Add(new ulong[] { m.BaseAddress, m.RegionSize, m.Protect });
            uint p = m.Protect & 0xFF;
            if (p == 0x08 || p == 0x80) a.WriteCopyCommitted += m.RegionSize;
            if (p == 0x04 || p == 0x08 || p == 0x40 || p == 0x80) a.WritableCommitted += m.RegionSize;
            if (p == 0x10 || p == 0x20 || p == 0x40 || p == 0x80) a.ExecCommitted += m.RegionSize;
            if ((m.Protect & 0x100) != 0) a.GuardCommitted += m.RegionSize;
          }
        }
        ulong next = m.BaseAddress + m.RegionSize;
        if (next <= addr) break;
        addr = next;
      }

      // Files behind image and mapped allocations
      var sb = new StringBuilder(1024);
      foreach (var a in s.Allocs) {
        if (a.Type == MEM_IMAGE || a.Type == MEM_MAPPED) {
          sb.Clear();
          if (Native.GetMappedFileNameW(h, (IntPtr)(long)a.Base, sb, 1024) > 0) a.File = sb.ToString();
          a.Kind = a.Type == MEM_IMAGE ? "image" : (a.File.Length > 0 ? "mapped-file" : "mapped-section");
        }
      }

      // Working set, page by page, for every committed range
      foreach (var a in s.Allocs) {
        foreach (var r in a.Ranges) {
          ulong pages = r[1] / PAGE;
          ulong done = 0;
          while (done < pages) {
            int n = (int)Math.Min(65536UL, pages - done);
            var arr = new Native.WSX[n];
            for (int i = 0; i < n; i++) arr[i].VirtualAddress = r[0] + (done + (ulong)i) * PAGE;
            if (Native.QueryWorkingSetEx(h, arr, (uint)(n * 16))) {
              for (int i = 0; i < n; i++) {
                ulong at = arr[i].Attributes;
                if ((at & 1) != 0) {
                  a.Resident += PAGE;
                  if ((at & (1UL << 15)) != 0) a.ResidentShared += PAGE; else a.ResidentPrivate += PAGE;
                }
              }
            }
            done += (ulong)n;
          }
        }
      }

      // Heaps from the PEB
      Native.PBI pbi; int ret;
      if (Native.NtQueryInformationProcess(h, 0, out pbi, Marshal.SizeOf(typeof(Native.PBI)), out ret) == 0) {
        s.ProcessHeap = ReadU64(h, pbi.Peb + 0x30);
        uint nh = ReadU32(h, pbi.Peb + 0xE8);
        ulong arr = ReadU64(h, pbi.Peb + 0xF0);
        for (uint i = 0; i < nh && i < 256; i++) s.Heaps.Add(ReadU64(h, arr + 8UL * i));
      }
      var heapIndex = new Dictionary<ulong, int>();
      for (int i = 0; i < s.Heaps.Count; i++) heapIndex[s.Heaps[i]] = i;

      foreach (var a in s.Allocs) {
        if (a.Type != MEM_PRIVATE || a.Committed == 0) continue;
        uint sig = ReadU32(h, a.Base + 0x10);
        if (sig == 0xFFEEFFEE) {
          ulong heap = ReadU64(h, a.Base + 0x28);
          int idx; a.Kind = heapIndex.TryGetValue(heap, out idx) ? "heap-segment#" + idx : "heap-segment?";
          if (heap == s.ProcessHeap) a.Detail = "process heap";
        } else if (sig == 0xDDEEDDEE) {
          int idx; a.Kind = heapIndex.TryGetValue(a.Base, out idx) ? "segment-heap#" + idx : "segment-heap?";
        }
      }

      // Each NT heap's header: Signature 0xEEFFEEFF at +0x98, TotalFreeSize (16-byte units) at
      // +0xC0, the decommit thresholds at +0xB0 and +0xB8, VirtualMemoryThreshold at +0x94
      for (int i = 0; i < s.Heaps.Count; i++) {
        ulong hb = s.Heaps[i];
        uint hsig = ReadU32(h, hb + 0x98);
        if (hsig == 0xEEFFEEFF) {
          ulong free = ReadU64(h, hb + 0xC0) * 16;
          s.HeapFree.Add(free);
          s.Notes.Add("heap " + i + " (0x" + hb.ToString("x") + "): NT heap, TotalFreeSize " + free + " bytes, DeCommitFreeBlockThreshold " + ReadU64(h, hb + 0xB0) * 16 + ", DeCommitTotalFreeThreshold " + ReadU64(h, hb + 0xB8) * 16 + ", VirtualMemoryThreshold " + (ulong)ReadU32(h, hb + 0x94) * 16 + ", FrontEndHeap 0x" + ReadU64(h, hb + 0x198).ToString("x"));
        } else {
          s.HeapFree.Add(0);
          s.Notes.Add("heap " + i + " (0x" + hb.ToString("x") + "): signature 0x" + hsig.ToString("x") + " at +0x98 (not an NT heap header as expected)");
        }
      }

      // Each NT heap's VirtualAllocdBlocks: a LIST_ENTRY in its header whose Flink is a private
      // allocation's base whose Blink points back at it
      var privBases = new HashSet<ulong>(s.Allocs.Where(x => x.Type == MEM_PRIVATE).Select(x => x.Base));
      for (int i = 0; i < s.Heaps.Count; i++) {
        ulong hb = s.Heaps[i];
        var hdr = Read(h, hb, 0x400);
        if (hdr == null) continue;
        for (int off = 0; off + 16 <= hdr.Length; off += 8) {
          ulong fl = BitConverter.ToUInt64(hdr, off), bl = BitConverter.ToUInt64(hdr, off + 8);
          ulong head = hb + (ulong)off;
          if (fl == head || !privBases.Contains(fl)) continue;
          if (ReadU64(h, fl + 8) != head) continue;
          // Walk it
          int count = 0; ulong e = fl; bool ok = true;
          var seen = new List<ulong>();
          while (e != head && count < 100000) {
            if (!privBases.Contains(e)) { ok = false; break; }
            seen.Add(e); e = ReadU64(h, e); count++;
          }
          if (!ok || e != head) continue;
          foreach (var b in seen) {
            var al = byBase[b];
            if (al.Kind == "") {
              al.Kind = "heap-large#" + i;
              ulong commit = ReadU64(h, b + 0x20);
              al.Detail = "commit " + commit + " reserve " + ReadU64(h, b + 0x28);
            }
          }
          s.Notes.Add("heap " + i + " (0x" + hb.ToString("x") + "): VirtualAllocdBlocks at +0x" + off.ToString("x") + ", " + seen.Count + " blocks");
          break;
        }
      }

      // Thread stacks and TEBs
      var proc = System.Diagnostics.Process.GetProcessById((int)pid);
      var tebs = new List<ulong>();
      foreach (System.Diagnostics.ProcessThread t in proc.Threads) {
        IntPtr th = Native.OpenThread(0x0040 | 0x0800, false, (uint)t.Id); // QUERY_INFORMATION | QUERY_LIMITED
        if (th == IntPtr.Zero) continue;
        try {
          string name = "";
          IntPtr d;
          if (Native.GetThreadDescription(th, out d) >= 0 && d != IntPtr.Zero) { name = Marshal.PtrToStringUni(d) ?? ""; Native.LocalFree(d); }
          s.ThreadNames[(uint)t.Id] = name;
          Native.TBI tbi;
          if (Native.NtQueryInformationThread(th, 0, out tbi, Marshal.SizeOf(typeof(Native.TBI)), out ret) == 0 && tbi.Teb != 0) {
            tebs.Add(tbi.Teb);
            ulong dealloc = ReadU64(h, tbi.Teb + 0x1478);
            Alloc st;
            if (dealloc != 0 && byBase.TryGetValue(dealloc, out st)) {
              st.Kind = "stack";
              st.Detail = (st.Detail.Length > 0 ? st.Detail + "; " : "") + t.Id + ":" + name;
            }
          }
        } finally { Native.CloseHandle(th); }
      }
      foreach (var teb in tebs) {
        foreach (var a in s.Allocs) {
          if (a.Kind == "" && a.Type == MEM_PRIVATE && teb >= a.Base && teb < a.Base + a.Reserved) { a.Kind = "teb"; break; }
        }
      }
      if (pbi.Peb != 0) foreach (var a in s.Allocs) if (a.Kind == "" && pbi.Peb >= a.Base && pbi.Peb < a.Base + a.Reserved) { a.Kind = "peb"; break; }
      foreach (var a in s.Allocs) {
        if (a.Kind == "") a.Kind = a.Type == MEM_PRIVATE ? "private-other" : (a.Type == MEM_MAPPED ? "mapped" : "image");
        if (a.Kind == "private-other" && a.Committed > 0) {
          // The first bytes, for telling who made it
          var head = Read(h, a.Ranges[0][0], 32);
          if (head != null && a.Detail == "") a.Detail = BitConverter.ToString(head).Replace("-", "");
        }
      }

      if (scan) { Scan(h, s); FindRefs(h, s); WalkHeap(h, s); }
      return s;
    }

    // The process heap's backend blocks, segment by segment: each busy block's size and how many
    // of its resident pages read all zero. Entries are 16 bytes; the second qword is encoded by
    // XOR with the heap's Encoding (+0x88): Size (16-byte units) in bits 0-15, Flags 16-23
    // (busy = 1), checksum 24-31 (XOR of the first three bytes), UnusedBytes 56-63.
    public class Block { public ulong Address; public ulong Size; public ulong Zero; public ulong Resident; public bool Busy; public string Head; public ulong Refs; public string RefFrom = ""; }
    static void WalkHeap(IntPtr h, Snap s) {
      if (s.ProcessHeap == 0) return;
      ulong key = ReadU64(h, s.ProcessHeap + 0x88);
      var segs = s.Allocs.Where(a => a.Kind.StartsWith("heap-segment#") && a.Detail == "process heap").ToList();
      foreach (var seg in segs) {
        foreach (var r in seg.Ranges) {
          ulong start = r[0], end = r[0] + r[1];
          ulong e = start;
          if (start == seg.Base) e = ReadU64(h, seg.Base + 0x40); // FirstEntry
          int guard = 0;
          var pageBuf = new Dictionary<ulong, bool>();
          while (e + 16 <= end && guard++ < 2000000) {
            ulong q = ReadU64(h, e + 8) ^ key;
            ulong size = (q & 0xFFFF) * 16;
            byte b0 = (byte)q, b1 = (byte)(q >> 8), b2 = (byte)(q >> 16), chk = (byte)(q >> 24);
            if (size == 0 || (byte)(b0 ^ b1 ^ b2) != chk) { s.HeapWalkStops++; break; }
            bool busy = (b2 & 1) != 0;
            var blk = new Block { Address = e, Size = size, Busy = busy };
            if (size >= 64 * 1024) {
              // zero and resident pages wholly inside the block's data
              ulong d0 = (e + 16 + PAGE - 1) & ~(PAGE - 1), d1 = (e + size) & ~(PAGE - 1);
              if (d1 > d0) {
                ulong pages = (d1 - d0) / PAGE;
                var arr = new Native.WSX[pages];
                for (ulong i = 0; i < pages; i++) arr[i].VirtualAddress = d0 + i * PAGE;
                if (Native.QueryWorkingSetEx(h, arr, (uint)(pages * 16))) {
                  for (ulong i = 0; i < pages; i++) {
                    if ((arr[i].Attributes & 1) == 0) continue;
                    blk.Resident += PAGE;
                    var pg = Read(h, d0 + i * PAGE, (int)PAGE);
                    bool z = pg != null;
                    if (z) for (int k = 0; k < pg.Length; k += 8) if (BitConverter.ToUInt64(pg, k) != 0) { z = false; break; }
                    if (z) blk.Zero += PAGE;
                  }
                }
              }
              var hd = Read(h, e + 16, 48);
              if (hd != null) blk.Head = BitConverter.ToString(hd).Replace("-", "");
            }
            if (busy) { s.HeapBusy += size; s.HeapBusyCount++; } else s.HeapFreeWalked += size;
            if (size >= 64 * 1024) s.Blocks.Add(blk);
            else {
              int bucket = size <= 256 ? 0 : size <= 4096 ? 1 : size <= 16384 ? 2 : 3;
              if (busy) s.SmallBusy[bucket] += size;
            }
            e += size;
          }
        }
      }
      // Who points at the big zero-heavy blocks: qwords equal to the block's data start (or inside
      // its first 64 bytes) in resident heap pages, with the 16 bytes either side of each hit
      var targets = s.Blocks.Where(b => b.Busy && b.Zero * 2 > b.Resident && b.Resident >= 256 * 1024).OrderBy(b => b.Address).ToList();
      if (targets.Count == 0) return;
      var starts = targets.Select(b => b.Address + 16).ToArray();
      foreach (var seg in segs) {
        foreach (var r in seg.Ranges) {
          ulong pages = r[1] / PAGE;
          var res = new Native.WSX[pages];
          for (ulong i = 0; i < pages; i++) res[i].VirtualAddress = r[0] + i * PAGE;
          if (!Native.QueryWorkingSetEx(h, res, (uint)(pages * 16))) continue;
          for (ulong i = 0; i < pages; i++) {
            if ((res[i].Attributes & 1) == 0) continue;
            var buf = Read(h, r[0] + i * PAGE, (int)PAGE);
            if (buf == null) continue;
            for (int o = 0; o + 8 <= buf.Length; o += 8) {
              ulong v = BitConverter.ToUInt64(buf, o);
              int idx = Array.BinarySearch(starts, v);
              if (idx < 0) continue;
              var t = targets[idx];
              t.Refs++;
              if (t.RefFrom.Length < 400) {
                ulong at = r[0] + i * PAGE + (ulong)o;
                ulong a0 = o >= 16 ? BitConverter.ToUInt64(buf, o - 16) : 0, a1 = o >= 8 ? BitConverter.ToUInt64(buf, o - 8) : 0;
                ulong c1 = o + 16 <= buf.Length ? BitConverter.ToUInt64(buf, o + 8) : 0, c2 = o + 24 <= buf.Length ? BitConverter.ToUInt64(buf, o + 16) : 0;
                t.RefFrom += string.Format("[{0:x}: {1:x} {2:x} *{3:x} {4:x}] ", at, a0, a1, c1, c2);
              }
            }
          }
        }
      }
    }

    // Who points at each private-other allocation of 1 MB or more: qwords inside it found in the
    // resident writable pages of images (by module) and of the heaps and other private memory
    static void FindRefs(IntPtr h, Snap s) {
      var targets = s.Allocs.Where(a => a.Kind == "private-other" && a.Committed >= (1UL << 20)).OrderBy(a => a.Base).ToList();
      if (targets.Count == 0) return;
      var bases = targets.Select(a => a.Base).ToArray();
      var ends = targets.Select(a => a.Base + a.Reserved).ToArray();
      var counts = new Dictionary<string, int>();
      foreach (var a in s.Allocs) {
        if (a.Committed == 0) continue;
        string from;
        if (a.Type == MEM_IMAGE) from = System.IO.Path.GetFileName(a.File);
        else if (a.Type == MEM_PRIVATE) from = a.Kind.StartsWith("heap") ? "heap" : (a.Kind == "private-other" ? "private@" + a.Base.ToString("x") : a.Kind);
        else continue;
        foreach (var r in a.Ranges) {
          uint p = (uint)r[2] & 0xFF;
          if ((r[2] & 0x100) != 0 || !(p == 0x04 || p == 0x08 || p == 0x40 || p == 0x80)) continue;
          ulong pages = r[1] / PAGE;
          var arr = new Native.WSX[pages];
          for (ulong i = 0; i < pages; i++) arr[i].VirtualAddress = r[0] + i * PAGE;
          if (!Native.QueryWorkingSetEx(h, arr, (uint)(pages * 16))) continue;
          for (ulong i = 0; i < pages; i++) {
            if ((arr[i].Attributes & 1) == 0) continue;
            var buf = Read(h, r[0] + i * PAGE, (int)PAGE);
            if (buf == null) continue;
            for (int o = 0; o + 8 <= buf.Length; o += 8) {
              ulong v = BitConverter.ToUInt64(buf, o);
              if (v < bases[0] || v >= ends[ends.Length - 1]) continue;
              int idx = Array.BinarySearch(bases, v);
              if (idx < 0) idx = ~idx - 1;
              if (idx < 0 || v >= ends[idx]) continue;
              if (targets[idx].Base == a.Base) continue;
              string key = bases[idx].ToString("x") + "|" + from;
              int c; counts.TryGetValue(key, out c); counts[key] = c + 1;
            }
          }
        }
      }
      foreach (var kv in counts) {
        var parts = kv.Key.Split('|');
        s.Refs.Add(new Ref { Target = Convert.ToUInt64(parts[0], 16), From = parts[1], Count = kv.Value });
      }
    }

    // The font's full name (name table, name ID 4; Windows UTF-16BE first, else Mac Roman)
    static string FontName(IntPtr h, ulong origin, ulong dirAt) {
      var hd = Read(h, dirAt, 12);
      if (hd == null || hd.Length < 12) return "";
      int n = hd[4] << 8 | hd[5];
      var dir = Read(h, dirAt + 12, 16 * n);
      if (dir == null) return "";
      for (int i = 0; i < n; i++) {
        int o = 16 * i;
        if (Encoding.ASCII.GetString(dir, o, 4) != "name") continue;
        ulong off = (uint)(dir[o + 8] << 24 | dir[o + 9] << 16 | dir[o + 10] << 8 | dir[o + 11]);
        int len = (int)(uint)(dir[o + 12] << 24 | dir[o + 13] << 16 | dir[o + 14] << 8 | dir[o + 15]);
        var t = Read(h, origin + off, Math.Min(len, 1 << 20));
        if (t == null || t.Length < 6) return "";
        int count = t[2] << 8 | t[3], strOff = t[4] << 8 | t[5];
        string mac = "";
        for (int k = 0; k < count && 6 + 12 * k + 12 <= t.Length; k++) {
          int r = 6 + 12 * k;
          int plat = t[r] << 8 | t[r + 1], id = t[r + 6] << 8 | t[r + 7], l = t[r + 8] << 8 | t[r + 9], so = t[r + 10] << 8 | t[r + 11];
          if (id != 4 || strOff + so + l > t.Length) continue;
          if (plat == 3) return Encoding.BigEndianUnicode.GetString(t, strOff + so, l);
          if (plat == 1 && mac == "") mac = Encoding.ASCII.GetString(t, strOff + so, l);
        }
        return mac;
      }
      return "";
    }

    // Reads the resident pages of every private and mapped-section allocation: zero pages, and the
    // OpenType/TrueType fonts held there (an sfnt header with a table directory that checks out)
    static void Scan(IntPtr h, Snap s) {
      foreach (var a in s.Allocs) {
        if (a.Type == MEM_IMAGE || a.Committed == 0) continue;
        if (a.Type == MEM_MAPPED && a.Kind == "mapped-file") continue;
        foreach (var r in a.Ranges) {
          if ((r[2] & 0x100) != 0 || (r[2] & 0xFF) == 0x01) continue; // guard or noaccess
          ulong pages = r[1] / PAGE;
          var arr = new Native.WSX[pages];
          for (ulong i = 0; i < pages; i++) arr[i].VirtualAddress = r[0] + i * PAGE;
          if (!Native.QueryWorkingSetEx(h, arr, (uint)(pages * 16))) continue;
          ulong i0 = 0;
          while (i0 < pages) {
            if ((arr[i0].Attributes & 1) == 0) { i0++; continue; }
            ulong i1 = i0;
            while (i1 < pages && (arr[i1].Attributes & 1) != 0 && i1 - i0 < 256) i1++;
            ulong start = r[0] + i0 * PAGE; int len = (int)((i1 - i0) * PAGE);
            var buf = Read(h, start, len);
            if (buf != null) {
              for (int p = 0; p + (int)PAGE <= buf.Length; p += (int)PAGE) {
                bool zero = true;
                for (int k = 0; k < (int)PAGE; k += 8) if (BitConverter.ToUInt64(buf, p + k) != 0) { zero = false; break; }
                if (zero) a.ZeroResident += PAGE;
                a.Scanned += PAGE;
              }
              for (int o = 0; o + 12 <= buf.Length; o += 8) {
                uint tag = (uint)(buf[o] << 24 | buf[o + 1] << 16 | buf[o + 2] << 8 | buf[o + 3]);
                if (tag != 0x00010000 && tag != 0x4F54544F && tag != 0x74727565 && tag != 0x74746366) continue;
                CheckFont(h, s, a, start + (ulong)o, tag);
              }
            }
            i0 = i1;
          }
        }
      }
    }

    static void CheckFont(IntPtr h, Snap s, Alloc a, ulong at, uint tag) {
      if (tag == 0x74746366) { // ttcf: a collection
        var hd = Read(h, at, 16);
        if (hd == null || hd.Length < 16) return;
        uint n = (uint)(hd[8] << 24 | hd[9] << 16 | hd[10] << 8 | hd[11]);
        if (n == 0 || n > 64) return;
        var offs = Read(h, at + 12, (int)(4 * n));
        if (offs == null) return;
        ulong max = 0; int tables = 0;
        for (int i = 0; i < n; i++) {
          uint fo = (uint)(offs[4 * i] << 24 | offs[4 * i + 1] << 16 | offs[4 * i + 2] << 8 | offs[4 * i + 3]);
          ulong e; int t; if (!Directory(h, at + fo, out e, out t, at)) return;
          max = Math.Max(max, e); tables += t;
        }
        uint f0 = (uint)(offs[0] << 24 | offs[1] << 16 | offs[2] << 8 | offs[3]);
        s.Fonts.Add(new Font { Address = at, Size = max, Tag = "ttcf x" + n, Owner = a.Kind, Tables = tables, Name = FontName(h, at, at + f0) });
        return;
      }
      ulong end; int tc;
      if (Directory(h, at, out end, out tc, at)) s.Fonts.Add(new Font { Address = at, Size = end, Tag = tag == 0x4F54544F ? "OTTO" : "sfnt", Owner = a.Kind, Tables = tc, Name = FontName(h, at, at) });
    }

    // An sfnt table directory at `at`: numTables sane, searchRange right, every table's tag
    // printable and its offset and length inside 64 MB; the font's size counted from `origin`
    static bool Directory(IntPtr h, ulong at, out ulong end, out int tables, ulong origin) {
      end = 0; tables = 0;
      var hd = Read(h, at, 12);
      if (hd == null || hd.Length < 12) return false;
      int n = hd[4] << 8 | hd[5];
      int sr = hd[6] << 8 | hd[7];
      if (n < 4 || n > 80) return false;
      int p2 = 1; while (p2 * 2 <= n) p2 *= 2;
      if (sr != p2 * 16) return false;
      var dir = Read(h, at + 12, 16 * n);
      if (dir == null || dir.Length < 16 * n) return false;
      bool head = false, cmap = false;
      for (int i = 0; i < n; i++) {
        int o = 16 * i;
        for (int k = 0; k < 4; k++) if (dir[o + k] < 0x20 || dir[o + k] > 0x7E) return false;
        string t = Encoding.ASCII.GetString(dir, o, 4);
        if (t == "head") head = true; if (t == "cmap") cmap = true;
        ulong off = (ulong)((uint)(dir[o + 8] << 24 | dir[o + 9] << 16 | dir[o + 10] << 8 | dir[o + 11]));
        ulong len = (ulong)((uint)(dir[o + 12] << 24 | dir[o + 13] << 16 | dir[o + 14] << 8 | dir[o + 15]));
        if (off > 64UL << 20 || len > 64UL << 20) return false;
        end = Math.Max(end, off + len);
      }
      tables = n;
      return head && cmap;
    }
  }
}
