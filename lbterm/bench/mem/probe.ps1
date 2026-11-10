# Looks at a live test window's heap blocks of one size: dumps the first of them and finds every
# qword pointing into any of them, across all committed private memory and writable image pages
param([Parameter(Mandatory)][int]$ProcessId, [int]$Kb = 129, [int]$Dump = 512, [int]$MaxBlocks = 400)
. (Join-Path $PSScriptRoot 'memtools.ps1')
if (-not ('LbProbe' -as [type])) {
  Add-Type -ReferencedAssemblies 'System.Collections', 'System.Linq', 'System.Runtime.InteropServices' -TypeDefinition @'
using System; using System.Collections.Generic; using System.Linq; using System.Runtime.InteropServices; using System.Text;
public static class LbProbe {
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, uint p);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern bool ReadProcessMemory(IntPtr h, IntPtr a, byte[] b, IntPtr s, out IntPtr r);
  [StructLayout(LayoutKind.Sequential)] struct WSX { public ulong Va; public ulong Attr; }
  [DllImport("psapi.dll")] static extern bool QueryWorkingSetEx(IntPtr h, [In, Out] WSX[] pv, uint cb);
  public static byte[] Read(uint pid, ulong addr, int len) {
    IntPtr h = OpenProcess(0x0410, false, pid); try { var b = new byte[len]; IntPtr n; ReadProcessMemory(h, (IntPtr)(long)addr, b, (IntPtr)len, out n); return b; } finally { CloseHandle(h); }
  }
  // Every 8-byte-aligned qword in [ranges] that falls in one of the [lo, hi) targets: "where -> target+offset"
  public static List<string> Refs(uint pid, ulong[] rangeStart, ulong[] rangeLen, ulong[] lo, ulong[] hi, int max) {
    var res = new List<string>();
    IntPtr h = OpenProcess(0x0410, false, pid);
    try {
      var buf = new byte[4096];
      for (int r = 0; r < rangeStart.Length; r++) {
        for (ulong off = 0; off < rangeLen[r]; off += (ulong)buf.Length) {
          int len = (int)Math.Min((ulong)buf.Length, rangeLen[r] - off);
          var q = new WSX[1]; q[0].Va = rangeStart[r] + off;
          if (!QueryWorkingSetEx(h, q, 16) || (q[0].Attr & 1) == 0) continue;
          IntPtr n; if (!ReadProcessMemory(h, (IntPtr)(long)(rangeStart[r] + off), buf, (IntPtr)len, out n)) continue;
          int got = (int)(long)n;
          for (int o = 0; o + 8 <= got; o += 8) {
            ulong v = BitConverter.ToUInt64(buf, o);
            int i = Array.BinarySearch(lo, v); if (i < 0) i = ~i - 1;
            if (i < 0 || v >= hi[i]) continue;
            ulong at = rangeStart[r] + off + (ulong)o;
            if (at >= lo[i] && at < hi[i]) continue;
            string ctx = "";
            for (int k = -3; k <= 3; k++) { int p = o + 8 * k; ctx += (p >= 0 && p + 8 <= got) ? BitConverter.ToUInt64(buf, p).ToString("x") + (k == 0 ? "* " : " ") : "- "; }
            res.Add(string.Format("{0:x} -> #{1} +0x{2:x}: {3}", at, i, v - lo[i], ctx));
            if (res.Count >= max) return res;
          }
        }
      }
    } finally { CloseHandle(h); }
    return res;
  }
}
'@
}
$snap = [LbMem.Mem]::Take([uint32]$ProcessId, $true)
$blocks = @($snap.Blocks | Where-Object { $_.Busy -and [Math]::Round($_.Size / 1KB) -eq $Kb } | Sort-Object Address | Select-Object -First $MaxBlocks)
"$($blocks.Count) busy blocks of $Kb KB; zero $([Math]::Round((($blocks | Measure-Object Zero -Sum).Sum) / 1MB, 1)) MB"
if (-not $blocks.Count) { return }
$b0 = $blocks[0]
$bytes = [LbProbe]::Read([uint32]$ProcessId, $b0.Address + 16, $Dump)
"first block 0x{0:x}, its data:" -f ($b0.Address + 16)
for ($i = 0; $i -lt $bytes.Length; $i += 32) { '{0,5:x}: {1}' -f $i, (($i..([Math]::Min($i + 31, $bytes.Length - 1)) | ForEach-Object { '{0:x2}' -f $bytes[$_] }) -join '') }
# Where non-zero data sits in each block: the last non-zero byte's offset
$ends = foreach ($b in $blocks | Select-Object -First 30) {
  $d = [LbProbe]::Read([uint32]$ProcessId, $b.Address + 16, [int]$b.Size - 16)
  $last = -1; for ($k = $d.Length - 1; $k -ge 0; $k--) { if ($d[$k] -ne 0) { $last = $k; break } }
  $last
}
"last non-zero byte in the first 30 blocks: $($ends -join ', ')"
$ranges = @($snap.Allocs | Where-Object { $_.Committed -gt 0 -and ($_.Type -eq [LbMem.Mem]::MEM_PRIVATE -or $_.WritableCommitted -gt 0) } | ForEach-Object { $_.Ranges } | Where-Object { ($_[2] -band 0x100) -eq 0 -and ($_[2] -band 0xFF) -ne 1 })
$lo = [uint64[]]@($blocks | ForEach-Object { $_.Address + 16 })
$hi = [uint64[]]@($blocks | ForEach-Object { $_.Address + $_.Size })
$refs = [LbProbe]::Refs([uint32]$ProcessId, [uint64[]]@($ranges | ForEach-Object { $_[0] }), [uint64[]]@($ranges | ForEach-Object { $_[1] }), $lo, $hi, 60)
"$($refs.Count) references into them (first 60):"
$refs
# Which allocation each reference sits in
$refs | ForEach-Object { [uint64]('0x' + ($_ -split ' ')[0]) } | ForEach-Object {
  $a = $_; $al = $snap.Allocs | Where-Object { $a -ge $_.Base -and $a -lt $_.Base + $_.Reserved } | Select-Object -First 1
  if ($al) { "$($al.Kind) $($al.Detail) $([IO.Path]::GetFileName($al.File))" }
} | Group-Object | ForEach-Object { "$($_.Count) in $($_.Name)" }
