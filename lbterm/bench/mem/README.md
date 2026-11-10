# Where an LBTerm window's memory goes (2026-10-31)

The groundwork track's memory report, parked part way for usage. The scripts beside it are its tools: LbMem.cs reads a process's regions, resident and zero pages, heaps, stacks and held fonts; run.ps1 and batch.ps1 drive test windows through lists of steps (list1.json to list3.json). Paths below to the session scratchpad were where it ran; the tools are here now, its out/ reports were not kept.

## How I measured
- **Build:** the installed one, copied to the scratchpad: 20261031-125609-01659fc5 (01659fc5d, the perf/mux-server merge). It is older than the feat/switch-calls merge (ac81c1eb6). So its line caches are 1024 by default and its quads are the unboxed 220-byte ones. The 4096-line-cache numbers below were set by config on this build, so they overstate main, where boxed quads roughly halve the quad part.
- **Windows:** opened through harness.psm1 (Start-Test, Open-TestWindow, -Out in the scratchpad) on the far-right screen, unfocused. No test window ever took the foreground.
- **What I read per process (my own tool, `LbMem.cs`):**
  - every allocation through VirtualQueryEx, split into private, image and mapped
  - which pages are resident (QueryWorkingSetEx, page by page) and which of those read all zero
  - the heaps (from the PEB, the NT heap header, then a walk of the process heap's blocks)
  - thread stacks (from each thread's TEB)
  - a scan for font files held in memory
  - what points into the big regions
- **GPU per process:** the `\GPU Process Memory(pid_*)` counters.
- **Timing and repeats:** each step was read after 2 s with no frame plus 1 s. Runs were on 2026-10-31 between 14:41 and 15:18. The first list ran twice, interleaved (r1, r2), and agreed within about 12 MB.
- **What ran beside me:**
  - another session's `lbterm.ps1 budgets` (perf-budgets-* windows from slot-5 and slot-2)
  - sibling agents' stats/ and pipe/ runs in the same scratchpad
  - Lachlan's Chrome, an Explorer copy dialog, Earshot's webview, ImageView, and a DWM ghost window of another session's bench window
  - Every window-check FAIL in my runs was one of those, never mine. Each report.json has a loadBefore and loadAfter line.

## Correction to the premise (measured)
- BENCH.md's config (bench-lib.ps1 New-BenchConfig) never sets `lbterm_compositor`, which defaults to off. So its 303 MB was WebGpu without DirectComposition. Lachlan's lbterm.lua line 173 turns it on. That mode costs about 200 MB more private, but less working set.
- In BENCH's mode, a log at wgpu info level showed only `wgpu_hal::vulkan::conv` warnings. That suggests the Vulkan backend, not D3D12, but I didn't confirm it.

## GUI private bytes with one tab (measured)

| Config | Private MB | Working set MB | GPU dedicated / shared MB |
|---|---|---|---|
| `-n`, no config (OpenGL) | 126.1, 115.9 | 107, 102 | 37 / 7 |
| Bench config, OpenGL | 135.0, 128.6 | | 40 / 7 |
| Bench config, Software | 129.1, 130.7 | | 40 / 7 |
| Bench config, WebGpu without compositor (BENCH's) | 333.5, 321.6 | 320, 308 | 55 / 172 |
| Bench config + compositor (D3D12, DComp) | 485.9 and 491.0, plus 9 more runs at 483–493 | 202–213 | 328 / 95 |
| Lachlan's lbterm.lua through New-TestConfig (compositor) | 529.3, 532.0 | 248, 250 | 374 / 110 |
| Mux domain, bench + compositor | GUI 476.7; server 41.2 (62.5 working set) | | |

So in Lachlan's mode, private bytes are about twice the RAM really used. PERF.md's finding 16 (525 MB, about 440 MB a window) is this same mode.

## Where the compositor window's memory goes (b-comp-r2, l-default-r2; sizes measured, owners inferred)
1. **A 256.00 MB private region:** committed but 0 resident. Private bytes rise by this and GPU dedicated goes from 55 to 328 MB.
   - *Inferred:* it is gpu-allocator 0.27's default 256 MB device block. wgpu's default MemoryHints::Performance asks for it at `wezterm-gui/src/termwindow/webgpu.rs:383` (`memory_hints: Default::default()`), going through `wgpu-hal-25.0.2/src/dx12/suballocation.rs:72` and `gpu-allocator-0.27.0/src/lib.rs` (AllocationSizes default, about line 327).
2. **A 64.00 MB region:** fully resident, 61–63 MB of it zero pages. *Inferred:* gpu-allocator's 64 MB host (upload) block. This is real RAM.
3. **A 30.6 MB region:** fully resident and all zero past a header.
   - *Inferred from the size:* it matches exactly a D3D12 shader-visible view heap of 1,000,000 descriptors at 32 bytes each.
   - Code: `wgpu-hal-25.0.2/src/dx12/device.rs:113` (`capacity_views = limits.max_non_sampler_bindings`) and `adapter.rs:603` (`1_000_000`).
   - The device's requested limits are at webgpu.rs:376–381. I didn't check whether they pass this value through.
4. **Other driver regions (not identified):** they sit in 32 MB reservations and come to about 45 MB committed and 33–47 MB resident, mostly zero (18.3, 23.7–31.7 and 10–11 MB). Lachlan's config adds about 14 MB here.
5. **The process heap:** 45–55 MB at start, 22–36 MB of it zero.
   - It holds 219 busy blocks of 129 KB, each with only its first 4,264 bytes used, 25.9 MB zero in all.
   - Their contents point into nvwgf2umx.dll, D3D12Core.dll and ntdll, and carry an "NVDA" tag, so they are NVIDIA driver allocations.
   - They are freed once the window does work (8 tabs or output), when the heap falls to 33–35 MB. Still there after 90 s idle.
6. **Images:** 348 MB committed, 57–58 MB resident, 32.6 MB writable (nvwgf2umx.dll alone 24.6 MB writable but under 1 MB resident). That counts against private bytes but isn't RAM.
7. **Smaller items:**
   - Thread stacks: 1.0–1.4 MB committed.
   - Fonts: the scan found only two copies of Segoe UI (0.92 MB each) on the heap. JetBrains Mono is built into the exe, shared, not private. Fonts are not where the memory goes.
   - Glyph atlas: it starts at 128×128 (`termwindow/mod.rs:102`, ATLAS_SIZE) and lives in GPU memory. Not measured on its own.

## Growth (measured, local domain, compositor, 100-column window, lines of 89 characters)
- **8 tabs:** GUI 492.0 → 477.3 MB (the driver blocks freed). Each tab's OpenConsole is 2.4 MB, 19.5 MB for 8.
  - On the mux domain: GUI 476.7 → 478.8, server 41.2 → 14.8, OpenConsole 18.9.
  - Tabs cost the GUI nothing.
- **Line caches, 100,000 lines printed, 9,001 lines of scrollback:**

  | Line cache size | Before → after (MB) | Heap (MB) |
  |---|---|---|
  | 16 | 490.5 → 481.2 | 35 |
  | 1024 (the build's default) | 483.0 → 525.8 | 79.7 |
  | 4096 | 491.7 → 665.5 | 218.8; 366 heap blocks of 264 KB |

  Against tiny caches, the caches cost about +45 MB at 1024 and +184 MB at 4096 on this unboxed build.
- **Scrollback:**

  | Run | At the fill (MB) | Closing the tab (MB) | Freed space the heap kept (MB) |
  |---|---|---|---|
  | 100,000 lines kept, plain | +68.0 (against +42.8 with 9,001 kept) | −35.9 | 18.3 |
  | 100,000 lines kept, varied | +54.1 | −31.8 | 23.6 |

  That's about 250–360 bytes for each scrollback line in the GUI. The heap's TotalFreeSize shows freed memory that is never handed back.
- **At rest:** private 491.0 → 490.9 → 491.1 MB at 0, 30 and 90 s. Working set rose 15 MB as the driver touched pages. Nothing grows.
- **Lachlan's config against bench + compositor:** about +40 MB private (heap +25, driver +14) and +46 MB GPU dedicated. Not split further.

## What could go without costing speed (ranked)
1. **The gpu-allocator block sizes:** `memory_hints: wgpu::MemoryHints::MemoryUsage` (8 MB device and 4 MB host blocks), or Manual, at webgpu.rs:383, a one-line change.
   - Expected per window: about −300 MB private, about −60 MB RAM, about −250 MB GPU dedicated.
   - Each window makes its own Instance and device (webgpu.rs:272), so this multiplies with windows.
   - A terminal's GPU buffers are small, so I expect no speed cost. That's inferred: check with `budgets` before and after.
2. **The descriptor heap:** a lower `max_non_sampler_bindings` in required_limits (for example 65,536) would save about 28 MB RAM per window. Inferred; check the limits pass through first.
3. **Freed memory the heap keeps:** 18–24 MB after a long tab closes. The options are the segment heap (manifest heapType SegmentHeap), HeapCompact, or another allocator. Speed effect unknown.
4. **Line caches:** cost measured above. Lachlan keeps 4096 for speed.
5. **Nothing to win:** stacks, fonts, writable image sections (accounting only), and the 129 KB driver blocks (transient).

## Not done, and how to pick it up
- **m-comp's 100,000-line fill on the mux domain never showed.** At 15:09 the state file still had one pane titled pwsh.exe, and the server held 20.4 MB. The likely cause is the long `-EncodedCommand` fill program spawned through the server, but I didn't check. Put the fill script in a .ps1 file and pass `-File`.
- **m-sb100k, l-local and l-mux in `list3.json` never ran.** l-local is Lachlan's config at rest, then 8 tabs, then 100,000 varied lines, then the close; l-mux is the same on the mux domain.
- **The client line cache on the mux domain is unmeasured.** It is sized `scrollback_lines.max(128)` per pane (`wezterm-client/src/pane/renderable.rs:130`), so up to 50,000 lines per pane with Lachlan's config. It could be large.
- **Confirm BENCH's backend:** run with `WEZTERM_LOG=lbterm_gui::termwindow::webgpu=trace`, which prints "Using adapter".
- **Pin down the driver regions and heap allocations by stack.** Either:
  - an elevated xperf (`C:/Program Files (x86)/Windows Kits/10/Windows Performance Toolkit/xperf.exe -on PROC_THREAD+LOADER+VIRT_ALLOC -stackwalk VirtualAlloc`), which Lachlan would run in an admin PowerShell, or
  - wezterm-gui's `dhat-heap` feature, in a build once a slot is free.
- **Confirm items 1 and 2 directly** with a build that changes them, measured with these tools, then `budgets`.
- **Lua state:** not measured. A test-config handler logging `collectgarbage('count')` would give it without a build.
- **Four windows in one GUI:** not measured. It would show the per-window device cost.

## Files
Everything was made in the groundwork session's scratchpad, beside its sibling agents' folders (stats/, pipe/ and others); the tools are copied here.
- Mine are `lbterm/bench/mem/` (`LbMem.cs`, `memtools.ps1`, `run.ps1`, `batch.ps1`, `list1.json` to `list3.json`, `summ.ps1`, `probe.ps1`) and `out/<run>/report.json`.
- `run.ps1` needs `-Bin` pointed at a fresh copy of the installed build.
- Debris: the killed m-comp run may have left its socket folder under `~/.local/share/lbterm/e2e/<hash>`. The next Start-Test with the same output folder clears it.

## Since (2026-11-01)
- **Item 1, the gpu-allocator blocks, is confirmed and done** (perf/gpu-memory; lbterm/PERF.md, finding 95 and "What a window's GPU memory holds"). On main's build of 2026-11-01 the window held the 256 MB region (committed, none resident) and the 64 MB one (all resident) as inferred. The blocks are 64 MB and 32 MB now (`config::lbterm::GPU_MEMORY_BLOCK`): about 225 MB less private a window, 192 MB less of the GPU's own and 32 MB less RAM. MemoryUsage's 8 MB and 4 MB would save 74 MB more but cost speed, so it isn't taken: each glyph the atlas takes is staged in an upload buffer of its own of 64 KB at least, and a refill made and let go dozens of 4 MB blocks.
- **Four windows in one GUI** were measured by the budget runner's `four-windows`: 1,781.9 MB private on main's build, 885.2 to 886.2 with the 64 MB blocks.
- `gpumem.ps1` is the script that measured it: a window drawn as Lachlan's on its own domain, taken through steps (tabs, lines, a burst of glyphs the atlas hasn't seen), each read with memtools.ps1 and metrics' paints. It finds the harness from its own folder.
- **Item 2, the descriptor heap, is confirmed and done** (perf/gpu-memory; lbterm/PERF.md, finding 96). The device's `max_non_sampler_bindings` does reach wgpu-hal's `capacity_views` (wgpu-core passes the required limits to `Adapter::open` unchanged, and `downlevel_defaults()` carried wgpu's 1,000,000). The heap is 65,536 views now (`config::lbterm::GPU_VIEW_DESCRIPTORS`). **The inference that it was RAM was wrong:** it was the GPU's own memory mapped into the process. Its pages read as resident to `QueryWorkingSetEx`, but the working set never held them: taking it out moved private bytes by 28.4 MB and GPU dedicated by 28.7, and the working set by 0.2 (lbterm/PITFALLS.md, trap 74). The driver put it in an allocation of its own of 32,047,104 bytes in most windows and inside a 32 MB one in others.
- **Where a compositor window's memory goes now**, after both items, is a table in PERF.md's "What a window's GPU memory holds" ("Where a window's memory goes now"), with what's left that nobody owns.
- `gpumem.ps1 -Scan` reads every resident private page too, for the ones all zeros, and each step's line says the GPU's local and non-local memory.
- **A window resized** (the review, 2026-11-04): the memory above held only for a window not resized lately. Each uniform kept is a buffer D3D12 places on 64 KB of the GPU's memory, and those of every size a window passed through were kept, so 600 one-pixel sizes took a window from 153.5 to 490.4 MB of GPU memory. Now only its size's and the one before's are kept (PERF.md, "A window resized"): 26 bind groups and 153.8 MB back at its first size after 150.
- **The block size held to a test** (`lbterm.ps1 budgets -Only atlas-refill -Against <build>`): on main's code, 64 MB blocks put a screen of glyphs the atlas doesn't hold 5.5% later than wgpu's default blocks, told apart only with the rounds paired; 128 MB blocks cost nothing measured and hold 336 MB private and 217 MB of the GPU's at rest. So LBTerm's blocks are 128 MB, under Lachlan's rule, speed over memory (PERF.md, "A screen of glyphs it hadn't drawn, held to a test").
