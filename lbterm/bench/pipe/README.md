# Where LBTerm's time goes on plain output (2026-10-31)

The groundwork track's speed report, parked part way for usage. The scripts beside it are its probe: a 32 MB plain-output producer, CPU by thread, and a sampling profiler (lib.cs) that walks stacks against the build's PDB. Paths below to the session scratchpad were where it ran; the scripts are here now, the runs were not kept.

Nothing was edited outside my scratchpad, nothing was committed, built or run through cargo, and Lachlan's own LBTerm, its mux server and its socket weren't touched. The build copies and the large data files are deleted (2.0 GB freed). The probe scripts are copied here; the raw results stayed in the session's scratchpad (5.4 MB) and aren't kept.

## How I measured

- **Builds compared.** Each build was copied whole (top-level files and mesa) into the scratchpad and opened as harness test windows (`Start-Test`, `Open-TestWindow`, never focused).
  - installed: 20261031-125609-01659fc5
  - main: slot-3, ac81c1eb6
  - fonts: perf/fonts-core, slot-5, c9852a973
  - layers: perf/lbterm-layers, slot-6, a0616065f
  - paint: perf/paint, slot-4, 01205d6bf
  - muxclient: perf/mux-client, slot-1, d2e36a653
- **Workload.** The producer is lbterm/bench's, modified so rounds can repeat. Each round writes 32 MB of bench's `plain` pattern (80 printable ASCII columns, CRLF) in 64 KB WriteFile calls. "Shown" runs from the first write until the title marker appears in the window title. A 100x30 window.
- **Two configs.**
  - "bench": lbterm/bench's own config (Cascadia 12, WebGpu, scrollback 9001).
  - "lachlan": `New-TestConfig`, which is his `lbterm.lua` with the rail, the strips, the triggers and his hyperlink rules.
- **CPU.** Taken from the harness's `Get-CpuSnapshot`, by cycles: the GUI process, its GUI thread, every thread by name, the mux server and its threads, and the conpty host.
- **Counts.** Context switches came from NtQuerySystemInformation and I/O counts from GetProcessIoCounters. The `metrics` replies were taken with `-Reset` before each round.
- **Sampling profiler.** My own sampler (`pipe/lib.cs`, `PipeProbe.Sampler`). It suspends a thread, copies its stack, resumes it, then walks the copy with StackWalk64 against the build's PDB. Each sample is weighted by the cycles the thread ran since its last sample. Effective rate about 236 Hz a thread with 5 threads; each suspension is about 48 µs. Sampled rounds used 128 MB and ran slower than unsampled ones (main local 6.6 against 8.9 MB/s), so treat profile shares as indicative.
- **Caveat on every number here.** Test windows write the state file (`LBTERM_STATE_FILE`); lbterm/bench's own runs don't.
  - Its writer thread `lbterm-state` cost 241 to 414 ms per 32 MB, 35% of its samples spent in rename.
  - On the GUI thread, `lbterm_state_window` → `LocalPane::get_dimensions` took 21% of the main build's sampled GUI thread, because it takes the pane lock.
  - So GUI costs here are inflated over Lachlan's real window.
- **What else ran.**
  - At every round's start, 4 to 9 lbterm-gui and 1 to 6 mux-server processes were running: other sessions' test windows (budgets-key-echo, budgets-search, mux-hidden, mem-try1) and Lachlan's daily window.
  - For most of the later rounds, 2 to 4 cargo and 1 to 4 rustc as well.
  - The window check failed on Lachlan's own windows: Chrome at 14:52:39 and 14:53:00, explorer's "85% complete" copy dialog and its pop-ups. These aren't mine.
  - One dwm.exe Ghost was ours: at 14:55:34 the fonts-core mux window with Lachlan's config went "not responding" mid-burst (below).

## Measured: the bench config, interleaved

Three cycles of 2 rounds each (n=6), 14:47 to 14:52. Values are the median, with the range in brackets. Times are ms of CPU per 32 MB.

| Build, domain | MB/s shown | MB/s taken | CPU s/MB | GUI process | GUI thread | Server | ConPTY | Frames |
|---|---|---|---|---|---|---|---|---|
| installed, local | 9.8 (7.5-11.0) | 11.1 | 0.153 | 4,592 | 1,032 | - | 299 | 176 |
| installed, mux | 8.1 (5.4-9.6) | 9.2 | 0.240 | 2,647 | 1,824 | 4,604 | 414 | 217 |
| main, local | 8.9 (8.4-10.3) | 10.4 | 0.173 | 5,180 | 1,098 | - | 339 | 187 |
| main, mux | 10.7 (9.9-11.8) | 12.2 | 0.172 | 1,735 | 1,039 | 3,444 | 276 | 178 |
| fonts-core, local | 61.9 (41.0-73.9) | 70.6 (46-83) | 0.033 | 764 | 206 | - | 293 | 30 |
| fonts-core, mux | 54.4 (38.5-76.6) | 60.8 | 0.069 | 832 | 437 | 906 | 414 | 27 |

Each thread's share, in ms per 32 MB:

| Build, domain | Parser thread | GUI thread | State writer | Others |
|---|---|---|---|---|
| installed, local | 3,100 (2,753-3,913) | 1,032 | 241 | GL driver 100; reader 19 |
| fonts-core, local | 414 | 206 | 83 | reader 22 |
| installed, mux | 3,892 (server) | 1,824 | - | server main 312, client 307 |
| main, mux | 2,939 (server) | 1,039 | - | server main 235, client 222 |
| fonts-core, mux | 566 (server) | 437 | - | client 302, server main 253 |

The parser thread is unnamed; it is `mux::parse_buffered_data`. The reader thread, also unnamed, is `read_from_pane_pty`.

What it shows:

- **The parser is 65-75% of the terminal's CPU on installed and main**, about 95-120 ns a byte. fonts-core's printing changes cut it 7.5x and lift shown speed 5-7x: about 62 MB/s locally, within reach of Windows Terminal's 93 (78-118) from BENCH.md. That comparison isn't interleaved.
- **ConPTY takes 260-570 ms per 32 MB on every build** (8-17 ms a MB). On fonts-core that's about 28% of all the terminal's CPU; at 62-83 MB/s taken it's close to its own ceiling.
- **The reader thread is negligible** (about 20 ms per 32 MB). Measured on the GUI: 515 ReadFile calls per 32 MB, one per 64 KB write (`pipe/runs/L-*/rounds.jsonl`, `io`), 515 context switches, and its samples are waits (ReadFile, then the socketpair send). The hop through the socketpair costs the parser about 6% (`recv`).

## Measured: Lachlan's config, interleaved

Rounds L and T, n=4 each, 14:52 to 15:07.

| Build | Local MB/s | Local GUI thread (ms) | Mux MB/s | Mux GUI thread (ms) | Mux frames |
|---|---|---|---|---|---|
| installed | 5.8 (4.5-6.3) | 3,201 | 3.6 (2.5-5.1), 8.4 taken | 7,126 (4,871-11,615) | 6 (5-7) |
| main | 6.4 (4.7-7.5) | 3,043 | 4.0 (3.9-4.3) | 6,405 | 7 |
| fonts-core | 20.3 (17.6-23.5) and 19.9 (17.2-20.3) | 1,404 / 1,394 | 16.0 (2.4-28.8) and 5.3 (3.5-37.5), 71.5 taken | 3,367 / 5,292 | 3-5 |
| layers | 8.9 (5.9-10.2) | 1,774 | 9.5 (8.2-10.6) | 1,638 | 188 (176-203) |
| paint | 7.0 (6.2-7.5) | 2,454 | 4.3 (3.0-6.6) | 6,186 | 6 |
| muxclient | 6.6 (5.9-7.8) | 2,788 | 8.1 (7.1-9.8) | 1,205 (client thread 2,433) | 11 (8-14) |

Server CPU on the mux domain: installed 7,410 ms; main 6,070. fonts-core's two mux rounds were bimodal: the first about 26-37 MB/s, the second 2.4-5.5.

What it shows:

- **Locally, his config's paint is the cap once printing is cheap.**
  - On fonts-core the GUI thread spends about 35 ms a frame against about 7 ms with the bench config.
  - `gui.paint.impl` mean 36.3 ms, p95 55.6; `paint_pane.lines` only 6.4 ms of it (metrics of L-lachlan-fonts-local-c2).
  - The rest is LBTerm's layers under the pane lock: fonts-core's finding 69 names the scroll strip's prompt search, the triggers and the rail. perf/lbterm-layers takes the GUI thread from 3.2 to 1.8 s on a main-based build.
- **On a mux domain with his config, the window paints 3 to 9 frames per 32 MB on every build but layers, and blocks its GUI thread for half-second stretches.** fonts-core, mux, L c2, first round:
  - task `wezterm-client/src/pane/renderable.rs:525` ran 8 times for 499.9 ms in all, the longest 499.6;
  - `rpc{method=get_lines}` mean 490 ms;
  - a paint pass took up to 463 ms;
  - this is the round the DWM Ghost ("not responding") came from.
  - perf/lbterm-layers fixes it (188 frames, GUI thread 1.6 s). Its commit 6efd7fae5 ("a mux pane's never fetch its scrollback" for the strip's shell marks) matches.
  - This is the biggest problem in Lachlan's actual setup (mux plus his config).

## Measured: where each thread's samples went

Files: `pipe/runs/S-bench-*/sample-*.folded`. Lines are folded stacks with cycles and sample counts.

- **Main (installed code), local, parser thread** (17.9 s CPU over 128 MB):
  - self: `Performer::flush_print` 28.6%, `ClusteredLine::append_grapheme` 9.5%, `Action::append_to` 8.3%, `Line::set_cell_grapheme` 6.8%, `RtlAllocateHeap` 5.9%, `grapheme_column_width` 5.5%, vtparse 3.9 plus 2.8%;
  - inclusive: `perform_actions` 66.6%, `flush_print` 63.9%, `Parser::parse` 30.3%, `set_cell_grapheme` 23.4%, `append_to` 21.3% (the PrintString pushed a character at a time), `finish_grow`/realloc 9.2%, `scroll_up` 5.2%;
  - this is core-12 exactly, and fonts-core fixes it;
  - the mux server's parser thread on the main build has the same shape: `flush_print` 65.6% inclusive, parse 29.3%.
- **fonts-core, local, parser thread** (1.78 s CPU over 128 MB, about 13 ms a MB). What's left after its fix:
  - `RtlAllocateHeap` 14.1% plus `RtlFreeHeap` 9.9% self;
  - `TerminalState::scroll_up` 31.6% inclusive, of which `Screen::scroll_up` is 24.7% (dropping a `Line` 12.3%, `Line::new`);
  - `TermConfig::with_config` 12.7% inclusive, with `Configuration::get` 5.4% self;
  - waiting on the pane lock (`lock_slow`) 11.8%;
  - `set_cells_ascii` 11.2%, parse 18.8%.
  - The config lookups come from three callers, each once per line or per run: `normalize_output_to_unicode_nfc` in `flush_print` (5.9%), `scrollback_size` in `Screen::scroll_up`, and `bidi_mode` in `TerminalState::scroll_up`.
- **Main, local, GUI thread** (bench config, 5.57 s, about 88% under `wm_paint`):
  - `LocalPane::with_lines_mut` 28.2%, almost all of it `lock_slow` (27.6%): waiting for the parser;
  - `lbterm_state_window` → `get_dimensions` 21.4% (the test-only state file);
  - `apply_hyperlinks` 13.7%, of which `fancy_regex::vm::run` 7.5% self (default rules, every changed line);
  - `HeapQuadAllocator::apply_to` 7%;
  - `MotionSettings::from_config` with `SystemParametersInfoW` and curve parsing on every paint, 2.7 to 4.4%;
  - some frames name driver exports without PDBs (`vkGetInstanceProcAddr`) and labels like `shape_hash_for_line`/`assign_overlay` above paint look like folded symbols, so trust nothing at that level.
- **fonts-core, local, GUI thread** (0.86 s): 26% waiting for messages, `with_lines_mut` 29.9%.
- **Main, mux, GUI client thread:** 86% in IOCP waits, about 12.7% in `Pdu::decode_frame` (deserializing CellStorage).
- **Main, mux, server main thread:** 83% waiting; `push_pane_changes` 13%, encode 10.4%, zstd about 5%.

## Ranked fixes by what they'd buy

1. **Land perf/fonts-core's printing (core-12, the parser's runs).** Measured: local 9.8 → 61.9 MB/s and 0.153 → 0.033 CPU s/MB; mux 8.1 → 54.4 MB/s. It's that branch's work, not to be redone.
2. **Land perf/lbterm-layers for Lachlan's real setup.** Measured: mux plus his config goes from 3.6-4.0 MB/s and 5-7 frames to 9.5 MB/s and 188 frames, with GUI thread 7.1 → 1.6 s; local GUI thread 3.2 → 1.8 s. fonts-core plus layers together wasn't measured because no such build exists. My estimate is that local with his config then sits at the frame cost: the 35 ms frames are the ceiling at about 20 MB/s on fonts-core.
3. **What's left in the parser after fonts-core** (read from samples; the fixes aren't measured):
   - Reuse the line that scrolls off instead of `Line::new` plus a drop. In `term/src/screen.rs` around 785-830 (slot-5), the default-blank branch allocates a new `Line` and drops the removed one.
   - Cache `scrollback_size`, `bidi_mode` and `normalize_output_to_unicode_nfc` per config generation. Each call now takes TermConfig's std mutex, then the global CONFIG mutex and an Arc clone (`config/src/terminal.rs` 38-43, `config/src/lib.rs` 720-726).
   - Together about 35-40% of the parser's remaining time, roughly 150 ms per 32 MB: my estimate is +15-25% local speed while parser-bound.
   - This disagrees with fonts-core's finding 66, which put TermConfig's locks under 1%. That was in Unicode output; in plain ASCII after its own fix they're 12.7% inclusive here.
4. **Pane lock contention.** The parser waits on it 11-12%; the GUI waits on it 28-30% of its samples. fonts-core's finding 69 suggests drawing from a copy of the lines, or holding the lock for less. Read, not measured.
5. **The hyperlink scan on every changed local line at paint:** 13.7% of the GUI thread on main. perf/mux-client moves it to draw time for mux panes (measured: GUI thread 6.4 → 1.2 s with his config); local panes still pay it. Overlaps core-8 and the layers track.
6. **Motion settings rebuilt every paint:** 3-4% of the GUI thread. Already fixed by perf/paint 715ea0144.
7. **ConPTY:** about 300 ms per 32 MB and about 28% of the total after fonts-core. It's the next ceiling (about 100 MB/s); ConPTY 1.25 is already in main.
8. **The reader thread and the socketpair hop:** about 6% of the parser. Small.

## Where this overlaps perf/fonts-core

Items 1 and 3 overlap it (core-12, and TermConfig's locks from finding 66). Its findings 67 and 69 cover the link scan and the pane lock. Its core-3 (truecolor and hyperlink boxes) wasn't exercised: the plain pattern has no SGR.

## Not reached, and how I'd have gone on

1. **The wrap and vte patterns.** Generator code is in the transcript and `pipe/data` (Gen2 inline in a PowerShell call); the data is deleted. Run `drive2.ps1 -File wrap` and `-File vte` after regenerating with `PipeProbe.Data.Plain(path, 32MB, 380)` and bench's Vte(100, 30).
2. **Sampled runs with Lachlan's config on fonts-core and layers.** Cancelled before they started. To run them: `drive2.ps1 -Prefix SL -Config lachlan -Script run2.ps1 -Builds 'fonts,layers' -Rounds 1 -Sample -SampleFile plain4`. That needs the build copies again, and the `exit 0` guard for SL taken out of `drive2.ps1`.
3. **The state file's cost.** Rerun with the state writer off to get numbers like Lachlan's window: a harness change or a raw Start-Unfocused launch.
4. **A combined fonts-core plus layers build, and the parser fixes in item 3.** Both need a build slot.
5. **Windows Terminal interleaved.** It needs `-Away`, while Lachlan is away from the PC.
6. **ETW (wpr) for whole-system time and DWM's side.** Not tried; it may need elevation.

## Files

All in this folder, copied from the session's scratchpad:

- `run.ps1`: one window's rounds, CPU by thread and metrics.
- `run2.ps1`: the same, plus context switches and I/O counts.
- `drive.ps1`, `drive2.ps1`: the interleaved cycles.
- `summarize.ps1`: the tables above.
- `report.ps1`, `stacks.ps1`: read the folded samples.
- `lib.cs`, `lib2.cs`: the sampler, the title watch and the counters; `setup.ps1` compiles them.
- `producer.ps1`: the producer.
- `drive-*.log`: each run's log.
- `runs/<m|L|T|S>-*/rounds.jsonl`: every round's numbers.
- `runs/S-*/sample-*.folded`: the profiles.
## Addendum, after parking

- With Lachlan's config, fonts-core is only about 2.5x faster locally (17 to 20 MB/s against 6 to 7), against 5 to 7x with the bench config, and on a mux domain it's no faster at all (37.5 MB/s once, then 3.5 to 5.5, with 3 to 5 frames). In the slow fonts-core and paint mux runs the GUI thread is busy 5.2 to 8.6 s per 32 MB yet shows only 3 to 8 frames: the time goes somewhere other than presenting frames. A sampled run with his config (`drive2.ps1 -Prefix SL -Config lachlan -Script run2.ps1 -Builds 'fonts,layers' -Rounds 1 -Sample -SampleFile plain4`, after taking out drive2.ps1's `exit 0` guard for SL, which parked it) would say where. lbterm-layers' 188 frames suggest its fix for the strip's scrollback fetch is the cause.
- `Line::cells_mut_for_attr_changes_only` calls `coerce_vec_storage()` (`wezterm-surface/src/line/line.rs`, about line 1143 on main), so any caller turns a cluster-stored line into vec storage. No branch changes this.
- Runs taken while Lachlan used the desktop (rerun before trusting): S-fonts-local-c1 (Chrome, ImageView), T-layers-local-c1 (Explorer's delete dialog), T-paint-local-c1 (Explorer's copy window), T-paint-mux-c2 (ImageView), T-fonts-mux-c2 (Earshot's webview).
