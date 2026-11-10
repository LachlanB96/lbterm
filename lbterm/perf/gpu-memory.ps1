#Requires -Version 7
# Covers: wezterm-gui/src/termwindow/webgpu.rs, wezterm-gui/src/termwindow/gpu_work.rs
<#
What a window drawn as Lachlan's holds for its GPU (lbterm/PERF.md, findings 95 and 96), in test
windows with his config (New-TestConfig: WebGpu with the compositor, so D3D12) on the right-most
screen without focus:

1. At rest with one tab, settled: its private allocations read with VirtualQueryEx, its private
   bytes, and its GPU memory from Windows' GPU Process Memory counters. With wgpu's default
   MemoryHints::Performance its D3D12 allocator (gpu-allocator) committed a 256 MB block of the
   GPU's memory and a 64 MB upload block in RAM a window, for the few MB a terminal draws with;
   LBTerm's blocks are 128 MB and 64 MB (config/src/lbterm.rs, GPU_MEMORY_BLOCK). And wgpu's
   default view heap, 1,000,000 views of 32 bytes made whole with the device, was 30.5 MB of the
   GPU's own memory mapped into the process (in its private bytes, not its working set);
   LBTerm's is 65,536 views (GPU_VIEW_DESCRIPTORS), 2 MB. So no private allocation may have more
   than 128 MB committed, GPU dedicated memory stays under 235 MB and private bytes under 400 MB
   (on 2026-11-01 with both of wgpu's defaults 373.8 MB and 495 MB; with 128 MB blocks 245.8 MB
   and 366 MB; on 2026-11-04 with the view heap too 217.2 MB and 336 MB. 64 MB blocks held
   153.1 MB and 240 MB, but put a screen of new glyphs 5% later: PERF.md, finding 95).
2. Resized: every visual drawn in every frame (LBTERM_DRAW_ALL), four panes, the window made a
   pixel wider -Resizes times and then its first size again, so each frame draws every visual at
   a size it hasn't been at and makes a uniform's bind group for each place one is drawn at
   (termwindow/gpu_work.rs; the state file's `visuals.bind_groups`). A uniform's projection is
   made from the window's size, so only the size it's at and the one before are kept, whatever
   the sizes: never more than the start's and two sizes' places, and after them two sizes' places
   and the atlas's two. Kept by age, 150 sizes kept 1,703, a buffer of 64 KB of the GPU's memory
   each, and the window's GPU dedicated memory rose 68 MB and its private bytes 100 to 105 MB
   (2026-11-04). So read settled before the sizes and after them, back at its first size, GPU
   dedicated memory may rise at most -MaxGpuRiseMB and private bytes -MaxPrivateRiseMB; and the
   window made the size before again, as a maximize after its restore makes it, makes no bind
   group. It draws on, with no GPU error in its log.
3. The oldest go: the same four panes with LBTERM_TEST_UNIFORMS_KEPT=16 and the window's size
   left alone, the divider between the panes moved a cell -Moves times (`cli adjust-pane-size`),
   so each move draws the panes at places they haven't been at, at the same size: the bind groups
   kept pass 16 and fall back to about half, never past 16, the atlas's two and a frame's visuals,
   and the window draws on.

-Measure prints the numbers and judges nothing.

  pwsh -File lbterm\perf\gpu-memory.ps1 [-Bin <build>] [-Measure] [-Resizes 150] [-Moves 20]
#>
param([string]$Bin, [switch]$Measure, [int]$Resizes = 150, [int]$Moves = 20,
  [double]$MaxGpuRiseMB = 16, [double]$MaxPrivateRiseMB = 64)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\harness.psm1') -Force -DisableNameChecking
$T = Start-Test 'gpu-memory' -Bin $Bin

# Each private allocation of a process (VirtualQueryEx's regions grouped by their allocation
# base), most committed first, in bytes: what it reserves and what of that is committed. Judged by
# what's committed: the allocator's heaps are committed whole, where a GUI also reserves a 1 GB
# range it commits almost nothing of
if (-not ('LbtermE2E.Regions' -as [type])) {
  Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
namespace LbtermE2E {
  public static class Regions {
    [StructLayout(LayoutKind.Sequential)] struct Info {
      public ulong BaseAddress, AllocationBase; public uint AllocationProtect, __a; public ulong RegionSize;
      public uint State, Protect, Type, __b;
    }
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern UIntPtr VirtualQueryEx(IntPtr h, UIntPtr address, out Info info, UIntPtr size);
    const uint MEM_COMMIT = 0x1000, MEM_FREE = 0x10000, MEM_PRIVATE = 0x20000;
    // [base, reserved, committed] of each private allocation
    public static List<ulong[]> Private(uint pid) {
      IntPtr h = OpenProcess(0x0400 | 0x0010, false, pid);
      if (h == IntPtr.Zero) throw new InvalidOperationException("can't open process " + pid);
      var byBase = new Dictionary<ulong, ulong[]>();
      try {
        ulong at = 0; Info info;
        while (VirtualQueryEx(h, (UIntPtr)at, out info, (UIntPtr)Marshal.SizeOf(typeof(Info))) != UIntPtr.Zero) {
          if (info.State != MEM_FREE && info.Type == MEM_PRIVATE) {
            ulong[] a;
            if (!byBase.TryGetValue(info.AllocationBase, out a)) { a = new ulong[] { info.AllocationBase, 0, 0 }; byBase[info.AllocationBase] = a; }
            a[1] += info.RegionSize;
            if (info.State == MEM_COMMIT) a[2] += info.RegionSize;
          }
          ulong next = info.BaseAddress + info.RegionSize;
          if (next <= at) break;
          at = next;
        }
      } finally { CloseHandle(h); }
      var list = new List<ulong[]>(byBase.Values);
      list.Sort((x, y) => y[2].CompareTo(x[2]));
      return list;
    }
  }
}
'@
}

# The process's GPU memory by Windows' counters, in MB: dedicated (the GPU's own) and shared
function Get-GpuMemory([int]$ProcessId) {
  $r = [ordered]@{ dedicated = 0.0; shared = 0.0 }
  foreach ($s in (Get-Counter -Counter "\GPU Process Memory(pid_${ProcessId}_*)\*" -ErrorAction Stop).CounterSamples) {
    if ($s.Path -like '*\dedicated usage') { $r.dedicated += $s.CookedValue / 1MB }
    elseif ($s.Path -like '*\shared usage') { $r.shared += $s.CookedValue / 1MB }
  }
  $r.dedicated = [Math]::Round($r.dedicated, 1); $r.shared = [Math]::Round($r.shared, 1)
  [pscustomobject]$r
}

$pwshExe = 'C:/Program Files/PowerShell/7/pwsh.exe'
$cfg = Join-Path $T.Out 'gpu-memory.lua'
New-TestConfig -Path $cfg
# config/src/lbterm.rs's GPU_UNIFORMS_KEPT: 32 MB of uniforms, 64 KB each
$UniformsKept = 512
$mb = { param($b) [Math]::Round([double]$b / 1MB, 1) }

# Settled: no frame for 2 s (the prompt drawn, the rail's fades over), 30 s at most, so a PC busy
# beside the test only makes it wait longer
function Wait-Settled($w) {
  $last = -1; $quiet = [Diagnostics.Stopwatch]::StartNew(); $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.Elapsed.TotalSeconds -lt 30 -and $quiet.Elapsed.TotalSeconds -lt 2) {
    $f = Get-StateFrame $w
    if ($f -ne $last) { $last = $f; $quiet.Restart() }
    Start-Sleep -Milliseconds 100
  }
}
# What the window holds, settled: private bytes, GPU dedicated and shared in MB, and the bind
# groups it keeps (the state file's visuals.bind_groups)
function Read-Held($w) {
  Wait-Settled $w
  $p = Get-Process -Id $w.Pid
  $gpu = Get-GpuMemory $w.Pid
  $sw = Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)
  [pscustomobject]@{
    Private = & $mb $p.PrivateMemorySize64; Dedicated = $gpu.dedicated; Shared = $gpu.shared
    BindGroups = $(if ($sw.visuals) { $sw.visuals.bind_groups } else { $null }); Frame = [long]$sw.frame
  }
}
function Format-Held($h) { "private $($h.Private) MB, GPU dedicated $($h.Dedicated) MB and shared $($h.Shared), $($h.BindGroups) bind groups" }

# A window like Lachlan's drawing every visual in every frame, split in four, settled
function Open-FourPanes([string]$Kind, [hashtable]$Environment) {
  $w = Open-TestWindow -ConfigFile $cfg -Program @($pwshExe, '-NoProfile') -Environment $Environment
  Wait-State { param($s) @($s.panes).Count -ge 1 -and $s.rail } -What "the first pane and the rail ($Kind)" -TimeoutSec 30 | Out-Null
  $first = (Get-Panes)[0].pane_id
  $split = { param($pane, $dir) $got = (Invoke-Cli split-pane --pane-id $pane $dir -Program @($pwshExe, '-NoProfile') -Merge) -join ' '; if ($got.Trim() -notmatch '^\d+$') { throw "split-pane printed: $got" }; [int]$got.Trim() }
  $right = & $split $first '--right'
  & $split $first '--bottom' | Out-Null
  & $split $right '--bottom' | Out-Null
  Wait-State { param($s) @($s.panes).Count -ge 4 } -What "four panes ($Kind)" -TimeoutSec 20 | Out-Null
  Get-QuietFrame -QuietMs 700 -TimeoutSec 30 | Out-Null
  [pscustomobject]@{ Window = $w; First = $first }
}

Invoke-Test {
  $w = Open-TestWindow -ConfigFile $cfg -Program @($pwshExe, '-NoProfile')
  Wait-State { param($s) $s.frame -gt 0 } -What 'the first frame' -TimeoutSec 30 | Out-Null
  Wait-Settled $w
  $p = Get-Process -Id $w.Pid
  $gpu = Get-GpuMemory $w.Pid
  $allocs = [LbtermE2E.Regions]::Private([uint32]$w.Pid)
  $biggest = @($allocs | Select-Object -First 5 | ForEach-Object { "$(& $mb $_[2]) MB committed of $(& $mb $_[1])" })
  "private $(& $mb $p.PrivateMemorySize64) MB, working set $(& $mb $p.WorkingSet64) MB; GPU dedicated $($gpu.dedicated) MB, shared $($gpu.shared) MB"
  "biggest private allocations: $($biggest -join ', ')"
  if (-not $Measure) {
    $big = @($allocs | Where-Object { $_[2] -gt 128MB })
    if ($big) { Add-Failure "a private allocation with more than 128 MB committed, as wgpu's default 256 MB block was: $(@($big | ForEach-Object { "$(& $mb $_[2]) MB at 0x$('{0:x}' -f $_[0])" }) -join ', ')" }
    # The view heap is the GPU's own memory: wgpu's default 1,000,000 views were 28.7 MB more of it
    if ($gpu.dedicated -ge 235) { Add-Failure "GPU dedicated memory is $($gpu.dedicated) MB, under 235 wanted (217.2 measured with 128 MB blocks and a view heap of 65,536 views, 245.8 with wgpu's default heap of 1,000,000, 373.8 with its default blocks too)" }
    if ($gpu.dedicated -le 0) { Add-Failure 'no GPU dedicated memory read for the window: the counters measured nothing' }
    if ($p.PrivateMemorySize64 -ge 400MB) { Add-Failure "private bytes are $(& $mb $p.PrivateMemorySize64) MB, under 400 wanted (336 measured with 128 MB blocks, 495 with wgpu's default)" }
  }
  Stop-TestWindow $w

  # 2 and 3: four panes drawing every visual in every frame, resized a pixel at a time, then with
  # the most kept made small and the panes' divider moved at one size
  foreach ($kind in 'resized', 'oldest go') {
    $envs = @{ LBTERM_DRAW_ALL = '1' }
    $cap = $UniformsKept
    if ($kind -eq 'oldest go') { $cap = 16; $envs.LBTERM_TEST_UNIFORMS_KEPT = "$cap" }
    $four = Open-FourPanes $kind $envs
    $w = $four.Window; $first = $four.First
    $seen = [Collections.Generic.List[object]]::new()
    $note = { param($sw, $s) if ($sw.visuals -and $null -ne $sw.visuals.bind_groups) { $seen.Add([pscustomobject]@{ Frame = [long]$sw.frame; Width = $sw.size.width; Drawn = [int]$sw.visuals.drawn; Made = [int]$sw.visuals.made; BindGroups = [int]$sw.visuals.bind_groups }) } }
    $held = Read-Held $w
    $clock = [Diagnostics.Stopwatch]::StartNew()
    if ($kind -eq 'resized') {
      $outer = Get-WindowRect $w; $client = Get-ClientRect $w
      $ow = $outer.Right - $outer.Left; $oh = $outer.Bottom - $outer.Top; $cw = $client.Width
      for ($i = 1; $i -le $Resizes; $i++) {
        Set-WindowSize $w ($ow + $i) $oh
        Wait-State { param($s) $s.size.width -eq $cw + $i } -OnState $note -PresentMs 0 -What "the window $($cw + $i) px wide ($kind)" -TimeoutSec 15 | Out-Null
      }
      # Then its first size again, so what it holds is held against what it held at that size:
      # wider, its swapchains hold more (600 px wider, 17 MB more of the GPU's memory)
      Set-WindowSize $w $ow $oh
      Wait-State { param($s) $s.size.width -eq $cw } -OnState $note -PresentMs 0 -What "the window back at $cw px ($kind)" -TimeoutSec 15 | Out-Null
      $steps = "$Resizes sizes and back"
    } else {
      $cols = [int](@((Get-StateWindow (Read-State) -Hwnd ([long]$w.Handle)).panes | Where-Object id -eq $first)[0].cols)
      if (-not $cols) { throw "the first pane ($first) isn't in the state file's panes" }
      for ($i = 1; $i -le $Moves; $i++) {
        Invoke-Cli adjust-pane-size --pane-id $first --amount 1 Right -Check | Out-Null
        Wait-State { param($s) [int](@($s.panes | Where-Object id -eq $first)[0].cols) -eq $cols + $i } -OnState $note -PresentMs 0 -What "the first pane $($cols + $i) columns wide ($kind)" -TimeoutSec 15 | Out-Null
      }
      $steps = "$Moves moves of the divider"
    }
    $took = $clock.Elapsed.TotalSeconds
    $heldAfter = Read-Held $w
    # Drawing on afterwards: a line typed into the first pane, its frame drawing visuals
    $f0 = Get-StateFrame $w
    Invoke-Cli send-text --pane-id $first --no-paste "'drawn on'`r" -Check | Out-Null
    $after = Wait-State { param($s) $s.visuals -and $s.visuals.drawn -gt 0 } -AfterFrame $f0 -OnState $note -What "a frame after the $steps ($kind)" -TimeoutSec 15
    $back = $null
    if ($kind -eq 'resized') {
      # The size before, again, as a maximize after its restore makes it: drawn with what's kept
      $backSeen = [Collections.Generic.List[int]]::new()
      $kept = (Read-Held $w).BindGroups
      Set-WindowSize $w ($ow + $Resizes) $oh
      Wait-State { param($s) $s.size.width -eq $cw + $Resizes } -OnState { param($sw, $s) if ($sw.visuals) { $backSeen.Add([int]$sw.visuals.made) } } -PresentMs 0 -What "the window $($cw + $Resizes) px wide again ($kind)" -TimeoutSec 15 | Out-Null
      $back = [pscustomobject]@{ Before = $kept; After = (Read-Held $w).BindGroups; Made = [int]($backSeen | Measure-Object -Sum).Sum }
    }
    $errors = @(Get-LogLines $w -Pattern 'GPU error|GPU device lost|Unable to allocate descriptors')
    $alive = Test-WindowAlive $w
    Stop-TestWindow $w
    if (-not $seen.Count) {
      if ($Measure) { "$($kind): this build doesn't say the bind groups it keeps (visuals.bind_groups)"; continue }
      Add-Failure "$($kind): no frame said the bind groups it keeps (visuals.bind_groups): this build doesn't count them"; continue
    }
    $peak = ($seen | Measure-Object BindGroups -Maximum).Maximum
    $drawnMost = ($seen | Measure-Object Drawn -Maximum).Maximum
    $drawnLeast = ($seen | Where-Object Drawn -gt 0 | Measure-Object Drawn -Minimum).Minimum
    $fell = 0; for ($j = 1; $j -lt $seen.Count; $j++) { if ($seen[$j].BindGroups -lt $seen[$j - 1].BindGroups) { $fell++ } }
    "$($kind): $steps in $([Math]::Round($took, 1)) s, $($seen.Count) frames seen drawing $drawnLeast to $drawnMost visuals; bind groups kept $($held.BindGroups) at the start, $peak at most, $($heldAfter.BindGroups) after, falling $fell times; the most kept $cap; $($errors.Count) GPU errors in its log"
    "$($kind): before, $(Format-Held $held); after, $(Format-Held $heldAfter): GPU dedicated $([Math]::Round($heldAfter.Dedicated - $held.Dedicated, 1)) MB, private $([Math]::Round($heldAfter.Private - $held.Private, 1)) MB"
    if ($back) { "$($kind): back at the size before, $($back.Before) bind groups kept, then $($back.After), its frames making $($back.Made) buffers and bind groups" }
    if ($Measure) { continue }
    if (-not $alive) { Add-Failure "$($kind): the window ended during the $steps" }
    if ($errors) { Add-Failure "$($kind): its log says $($errors.Count) GPU errors: $(@($errors | Select-Object -First 3) -join ' | ')" }
    if (-not $after.visuals -or $after.visuals.drawn -lt 4) { Add-Failure "$($kind): the frame after the $steps drew $($after.visuals.drawn) visuals, every one wanted (LBTERM_DRAW_ALL)" }
    # Never past the most kept, the atlas's two and a frame's visuals
    if ($peak -gt $cap + 2 + $drawnMost) { Add-Failure "$($kind): $peak bind groups kept, more than the most ($cap), the atlas's two and a frame's $drawnMost visuals" }
    if ($kind -eq 'resized') {
      # The sizes before the one before go: never more than the start's and two sizes' places,
      # and after them two sizes' and the atlas's two. Kept by age, 150 sizes kept 1,703
      if ($peak -gt [int]$held.BindGroups + 2 * $drawnMost) { Add-Failure "$($kind): $peak bind groups kept at most over $steps, more than the $($held.BindGroups) at the start and two sizes' of $drawnMost visuals: uniforms made at sizes the window has left are kept" }
      if ($heldAfter.BindGroups -gt 2 + 2 * $drawnMost) { Add-Failure "$($kind): $($heldAfter.BindGroups) bind groups kept after the $steps, more than two sizes' of $drawnMost visuals and the atlas's two" }
      # ...but kept: the four panes' at least, or each frame makes its uniforms again
      if ($heldAfter.BindGroups -lt 2 + 4) { Add-Failure "$($kind): $($heldAfter.BindGroups) bind groups kept after the $steps, the atlas's two and the four panes' at least wanted: the uniforms aren't kept" }
      # What they hold doesn't grow with the sizes: 150 sizes kept by age rose 68 MB of GPU
      # dedicated memory and 113 MB of private bytes (2026-11-04)
      $gpuRise = $heldAfter.Dedicated - $held.Dedicated
      $privateRise = $heldAfter.Private - $held.Private
      if ($gpuRise -gt $MaxGpuRiseMB) { Add-Failure "$($kind): GPU dedicated memory rose $([Math]::Round($gpuRise, 1)) MB over $steps ($($held.Dedicated) to $($heldAfter.Dedicated)), at most $MaxGpuRiseMB wanted" }
      if ($privateRise -gt $MaxPrivateRiseMB) { Add-Failure "$($kind): private bytes rose $([Math]::Round($privateRise, 1)) MB over $steps ($($held.Private) to $($heldAfter.Private)), at most $MaxPrivateRiseMB wanted" }
      # The size before is kept: going back to it makes nothing
      if ($back.After -gt $back.Before -or $back.Made -gt 0) { Add-Failure "$($kind): back at the size before, the bind groups kept went from $($back.Before) to $($back.After) and its frames made $($back.Made): the size before's uniforms weren't kept" }
    } else {
      if ($fell -lt 1) { Add-Failure "$($kind): the bind groups kept never fell, so none went, though $steps drew $drawnLeast visuals at new places past $cap" }
      if ($heldAfter.BindGroups -lt [Math]::Floor($cap / 2)) { Add-Failure "$($kind): $($heldAfter.BindGroups) bind groups kept after, under half the most ($cap): more went than the oldest half" }
    }
  }
} -Passed $(if ($Measure) { 'gpu-memory: measured' } else { "gpu-memory: no allocation over 128 MB, GPU dedicated memory under 235 MB and private bytes under 400 MB; a window drawing every visual resized $Resizes times keeping its size's and the one before's uniforms, its memory risen no more than $MaxGpuRiseMB MB of the GPU's and $MaxPrivateRiseMB private, drawing on; and with the most kept made 16 the oldest going" })
