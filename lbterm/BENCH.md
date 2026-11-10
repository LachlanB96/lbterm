# Benchmarks

LBTerm against other terminals on this PC, measured the same way for each: startup, output throughput, CPU at rest and while printing, and memory (`lbterm/bench/bench.ps1`, tables by `summarize.ps1`), and key-to-screen latency (`key-latency.ps1`, which needs the keyboard and runs only while Lachlan is away from the PC). The scripts live in `lbterm/bench`, not `lbterm/perf`, because they open other terminals and the e2e runner collects `lbterm/perf`.

## How it measures

- Every terminal opens unfocused on the right-most screen through the benchmark's launcher (`BenchLaunch.cs`, `SW_SHOWNOACTIVATE`), and a watcher checks the foreground every 20 ms from the launch to the close: a terminal whose window takes the foreground is closed at once and not used again in the run, with no retry.
- The same font (Cascadia Mono 12 pt, Windows Terminal's default), columns and rows (100x30), scrollback (9001 lines, Windows Terminal's default) and shell (pwsh 7, `-NoProfile`) in each. LBTerm runs one config of the benchmark's own (WebGpu, no WSL domains, no update check), as WezTerm did while it ran (below); Windows Terminal its own settings, untouched.
- Startup: from the launch to the window showing, to the tab's program being created, and to it running (`ready.ps1` writes the time).
- Throughput: `producer.ps1` writes 32 MB of a pattern (plain 80-column lines, long lines that wrap, dense SGR colour, wide characters and emoji, a vtebench-like scrolling pattern) to its console with WriteFile and times itself; then it sets the window title, which `bench.ps1` reads from outside to know when the terminal has caught up. "MB/s taken" is how fast ConPTY took the writes, "shown" how fast the terminal showed them, "to quiet" until its CPU settled.
- CPU and memory: the terminal's whole tree (GUI, LBTerm's mux server, ConPTY's OpenConsole), from process cycle times and private bytes, at rest for 60 s with 1 and with 8 idle tabs, and after 100,000 lines of scrollback.
- Every round runs the terminals interleaved, so drift in the PC's load falls on all of them, and logs what else was busy as each round began.

## WezTerm, the baseline that stays

Since 2026-11-04 the benchmark runs LBTerm (on each domain) and Windows Terminal, and no longer upstream WezTerm: WezTerm was uninstalled from the PC that day, after a security review found that `wezterm --config "<Lua>" ls-fonts` runs any Lua with no window, and nothing but this benchmark used it. Lachlan's 2a of 2026-11-04: drop the column, and keep its speeds as the baseline to look back to. That baseline is the WezTerm rows of the run of 2026-10-30 below (startup, throughput, CPU at rest, memory and scrollback), measured beside LBTerm the same way. Read them knowing what they were: WezTerm 20260610-150805-891bed31, a nightly of 2026-06-10, so upstream as it was then, not the upstream LBTerm's `main` is built on at any later date.

`bench.ps1` and `key-latency.ps1` take `-Kinds lbterm, lbterm-mux, wt`; `summarize.ps1` still names a `wezterm` row, so the results of a run from before then can be tabled again.

## The run of 2026-10-30 (20261030-215243)

LBTerm 20261030-191144-301f18e6 (the installed build), WezTerm 20260610-150805-891bed31, Windows Terminal 1.24.11321.0. Intel Core i9-9980XE, 18 cores / 36 threads, every core at 3.0 GHz (turbo off by Lachlan's choice since 2026-10-24). Other Claude sessions' builds and tests ran beside it (the load lines below), and Call of Duty's launcher was open (`cod23-cod`, under half a core).

Not independently reviewed: the run was stopped at Lachlan's word after the at-rest phase's first round (and part of its second), before the review that would have rerun its numbers. Windows Terminal isn't in it: in the first smoke test (21:37:19, 2.3 s after Lachlan's last input) its window took the foreground as it opened, and the watcher closed it and kept it out of the run. Its numbers come with the key timing, while Lachlan is away.

Lachlan, 2026-10-30, on the WezTerm column: LBTerm is WezTerm plus his commits, so beating upstream isn't the point. The WezTerm column is a baseline (what LBTerm's own changes cost or saved), not a rival; the comparisons that count are against other terminals. It's the only run with a WezTerm column ("WezTerm, the baseline that stays", above).

### Startup (ms from the launch)

| Terminal | n | Window shown | Program created | Program running |
|---|---|---|---|---|
| LBTerm (local) | 8 | 1,269 (1,203-1,345) | 118 (112-130) | 581 (567-644) |
| LBTerm (mux) | 8 | 1,234 (1,153-1,306) | 2,510 (2,477-2,585) | 2,975 (2,962-3,197) |
| WezTerm | 8 | 1,287 (1,264-1,372) | 121 (116-164) | 611 (573-653) |

### Throughput (median of the rounds, range in brackets)

| Pattern | Terminal | n | MB/s shown | MB/s to quiet | MB/s taken | Lines/s shown | Terminal CPU s/MB | Cores while printing | GUI / server / ConPTY CPU s |
|---|---|---|---|---|---|---|---|---|---|
| plain | LBTerm (local) | 3 | 11.4 (11.1-11.6) | 11.1 (10.9-11.4) | 12.8 (12.6-13.1) | 145,872 (142,092-148,851) | 0.124 (0.122-0.127) | 1.42 (1.39-1.45) | 3.64 / 0.00 / 0.31 |
| plain | LBTerm (mux) | 3 | 11.7 (11.7-11.8) | 11.3 (10.8-11.4) | 13.2 (13.2-13.4) | 150,120 (149,508-150,948) | 0.166 (0.164-0.168) | 1.94 (1.93-1.94) | 1.52 / 3.53 / 0.28 |
| plain | WezTerm | 3 | 11.3 (10.9-11.4) | 10.7 (9.7-10.9) | 12.5 (12.1-12.6) | 144,111 (140,043-145,422) | 0.169 (0.167-0.169) | 1.90 (1.83-1.92) | 5.12 / 0.00 / 0.27 |
| wrap | LBTerm (local) | 3 | 9.4 (8.9-9.5) | 9.2 (7.6-9.3) | 10.6 (10.1-10.7) | 25,769 (24,397-26,057) | 0.143 (0.138-0.147) | 1.30 (1.30-1.35) | 4.33 / 0.00 / 0.23 |
| wrap | LBTerm (mux) | 3 | 10.6 (10.6-10.6) | 9.9 (9.5-9.9) | 12.1 (11.9-12.1) | 28,972 (28,958-29,164) | 0.180 (0.172-0.183) | 1.89 (1.82-1.92) | 1.69 / 3.84 / 0.19 |
| wrap | WezTerm | 3 | 10.0 (10.0-10.2) | 9.8 (9.8-10.0) | 11.4 (11.2-11.5) | 27,515 (27,382-27,894) | 0.180 (0.170-0.185) | 1.81 (1.73-1.85) | 5.53 / 0.00 / 0.23 |
| sgr | LBTerm (local) | 3 | 23.6 (23.2-23.8) | 22.3 (22.2-22.4) | 25.7 (25.6-26.4) | 13,934 (13,719-14,030) | 0.094 (0.089-0.096) | 2.23 (2.11-2.23) | 1.77 / 0.00 / 1.20 |
| sgr | LBTerm (mux) | 3 | 26.7 (25.9-26.8) | 25.1 (25.0-25.4) | 27.1 (26.3-27.2) | 15,739 (15,290-15,827) | 0.129 (0.124-0.132) | 3.42 (3.33-3.45) | 1.23 / 1.77 / 1.16 |
| sgr | WezTerm | 3 | 25.8 (25.8-26.6) | 24.2 (17.0-25.0) | 26.0 (25.9-26.7) | 15,232 (15,199-15,666) | 0.107 (0.103-0.108) | 2.75 (2.74-2.79) | 2.19 / 0.00 / 1.23 |
| unicode | LBTerm (local) | 3 | 4.6 (4.5-4.7) | 4.6 (4.5-4.6) | 4.8 (4.7-4.8) | 32,356 (31,907-32,594) | 0.255 (0.255-0.259) | 1.18 (1.18-1.19) | 7.66 / 0.00 / 0.53 |
| unicode | LBTerm (mux) | 3 | 14.3 (11.6-14.4) | 7.3 (6.8-7.8) | 17.8 (13.4-18.4) | 100,601 (81,221-101,158) | 0.238 (0.234-0.285) | 2.57 (2.53-2.65) | 4.44 / 2.69 / 0.55 |
| unicode | WezTerm | 3 | 4.7 (4.7-4.7) | 4.6 (4.5-4.6) | 4.9 (4.8-4.9) | 32,843 (32,819-32,912) | 0.280 (0.276-0.284) | 1.31 (1.29-1.33) | 8.36 / 0.00 / 0.56 |
| vte | LBTerm (local) | 3 | 9.1 (9.0-9.3) | 8.9 (8.8-9.1) | 10.2 (10.2-10.4) | 90,242 (88,816-91,730) | 0.176 (0.175-0.180) | 1.61 (1.59-1.63) | 4.61 / 0.00 / 1.06 |
| vte | LBTerm (mux) | 3 | 10.3 (9.9-10.5) | 10.0 (9.6-10.2) | 11.6 (11.2-11.9) | 101,829 (97,914-104,265) | 0.205 (0.196-0.209) | 2.06 (2.03-2.15) | 1.69 / 3.88 / 1.08 |
| vte | WezTerm | 3 | 9.7 (9.6-9.9) | 9.5 (9.1-9.6) | 10.9 (10.9-11.1) | 96,148 (95,463-97,848) | 0.211 (0.211-0.216) | 2.09 (2.03-2.09) | 5.72 / 0.00 / 1.08 |

### CPU at rest (60 s, % of one core)

| Terminal | Tabs | n | Terminal | GUI | Mux server | ConPTY | The shells |
|---|---|---|---|---|---|---|---|
| LBTerm (local) | 1 | 2 | 0.42 (0.39-0.44) | 0.42 (0.39-0.44) | 0.00 | 0.00 | 0.12 (0.10-0.13) |
| LBTerm (local) | 8 | 2 | 0.41 (0.26-0.55) | 0.40 (0.26-0.55) | 0.00 | 0.00 | 1.13 (1.02-1.25) |
| LBTerm (mux) | 1 | 1 | 0.31 | 0.29 | 0.03 | 0.00 | 0.13 |
| LBTerm (mux) | 8 | 1 | 0.70 | 0.68 | 0.00 | 0.03 | 1.30 |
| WezTerm | 1 | 1 | 0.26 | 0.23 | 0.00 | 0.03 | 0.13 |
| WezTerm | 8 | 1 | 0.39 | 0.39 | 0.00 | 0.00 | 1.04 |

### Memory (MB: private bytes / working set of the GUI, mux server and ConPTY together)

| Terminal | n | 1 tab | 8 tabs | 100,000 lines, scrollback alike | 100,000 lines, all kept | GUI alone, 1 tab |
|---|---|---|---|---|---|---|
| LBTerm (local) | 2 | 303 (302-304) / 299 (299-299) | 342 (342-342) / 402 (402-402) | 359 / 348 | 389 / 378 | 300 (300-301) / 286 (286-287) |
| LBTerm (mux) | 1 | 321 / 323 | 359 / 424 | 380 / 374 | 409 / 403 | 312 / 290 |
| WezTerm | 1 | 302 / 297 | 342 / 401 | 337 / 322 | 366 / 353 | 300 / 285 |

### What else ran (busy cores over a second as each round began, of 36 threads)

- startup round 0: 7.21 cores; rustc 50972 2.42, rustc 11712 1.53, pwsh 45176 0.81, pwsh 38740 0.75, DisplayFusion.ui 23992 0.33
- startup round 3: 6.71 cores; rustc 11712 3.88, pwsh 38740 0.52, cod23-cod 47648 0.38, bun 52884 0.31
- startup round 6: 4.46 cores; pwsh 32824 0.89, pwsh 38740 0.73, bun 52884 0.48, cod23-cod 47648 0.22
- throughput round 1: 4.1 cores; pwsh 38740 0.59, chrome 32880 0.42, VBAudioMatrixCoconut_x64 27720 0.39
- throughput round 2: 4.86 cores; pwsh 38740 0.72, pwsh 46400 0.39, DisplayFusion.ui 23992 0.39
- throughput round 3: 3.16 cores; pwsh 38740 0.75, chrome 13892 0.56, VBAudioMatrixCoconut_x64 27720 0.36
- idle round 1: 3.23 cores; pwsh 38740 0.75, chrome 32880 0.53, VBAudioMatrixCoconut_x64 27720 0.34
- idle round 2: 5.08 cores; chrome 32880 0.69, pwsh 38740 0.69, chrome 13892 0.62, cod23-cod 47648 0.48

## What it shows

- **Throughput is ConPTY's, not the terminal's.** What ConPTY took ("taken") and what the terminal showed are within about 10% for every terminal and pattern: on Windows a console program's output goes through ConPTY, and that's where the speed is set (about 11 MB/s of plain lines, 26 of SGR-heavy ones). Shown speed isn't where LBTerm can pull ahead while every terminal sits behind ConPTY.
- **LBTerm takes less CPU for the same output.** On its local domain, CPU per MB against WezTerm's: plain 0.124 against 0.169 (27% less), wrap 21% less, SGR 12%, Unicode 9%, vte 17%; the GUI alone 3.64 s against 5.12 s for 32 MB of plain lines. Most likely main's own work (frame pacing, only changed visuals drawn) showing; not measured stage by stage. On the mux domain the total is about WezTerm's local cost, split between the server (3.5 s) and a GUI that does a third of what a local one does (1.5 s), so the GUI thread stays free for keys.
- **Unicode is shaping-bound in the GUI.** Locally, wide characters and emoji show at 4.6 MB/s with 7.7 s of GUI CPU for 32 MB, the slowest pattern by far. On the mux domain the server takes them three times as fast (14.3 MB/s shown, 17.8 taken) and the GUI catches up afterwards (7.3 MB/s to quiet). A lead for the fonts track: shaping with no cache (the audit's fonts-2).
- **A cold start on the mux domain loses 2.4 s.** The tab's program starts 2,510 ms after the launch against 118 ms locally: the client spends about 2.25 s retrying a socket no server listens on before it starts the server, then sleeps 200 ms (the audit's client11, in the mux-client group). Lachlan's GUI usually attaches to a server that's running, so it's every start after a reboot or a server restart that pays it.
- **The window takes 1.2-1.3 s to show, in LBTerm and WezTerm alike.** Upstream's cost, LBTerm's own changes neither adding nor saving any. A lead: font setup starting from nothing through the system locator (the audit's fonts-3) and the GPU's first adapter.
- **At rest, all of them are near nothing**: 0.3 to 0.7% of a core with 1 or 8 idle tabs.
- **Memory: about 300 MB for one tab, in the GUI**, the same as WezTerm, so upstream's (the GPU device and its buffers, glyph atlases, font data). 100,000 lines of scrollback add about 56 MB to LBTerm against about 35 MB to WezTerm with the same scrollback setting: something of LBTerm's own keeps about 20 MB more per 100,000 lines, not looked into yet.

## Still to run

- Windows Terminal, and key-to-screen latency for all of them: `key-latency.ps1 -Run -Away` (its header says how) and `bench.ps1` with `-Away`, both only once Lachlan has said he's away from the PC. Any real key or mouse input ends the run at once, closes its window and gives the foreground back. About 20 to 30 minutes, an estimate until the first real run.
- The at-rest phase's other rounds (n is 1 or 2 above).

## ConPTY 1.22, 1.24 and 1.25 (2026-10-31, 04:38 to 04:48)

LBTerm ships its own ConPTY (`assets/windows/conhost`: `conpty.dll` and `OpenConsole.exe`, 1.22.2502.04002, the one upstream WezTerm ships). The same installed build (20261030-191144-301f18e6) with Microsoft's ConPTY packages (`Microsoft.Windows.Console.ConPTY` from NuGet, signed by Microsoft) swapped in beside it: 1.24.2610.01001 and 1.25.2609.30003. `bench.ps1 -Phase throughput -Kinds lbterm,lbterm-mux -Rounds 1 -SizeMB 32 -From <copy> -Variant conpty-<v>`, three cycles of the three interleaved; the PC locked, six other sessions' builds and tests running. Medians of the three:

| Pattern | Domain | MB/s shown 1.22 / 1.24 / 1.25 | MB/s taken by ConPTY | ConPTY's CPU s for 32 MB | Terminal CPU s/MB |
|---|---|---|---|---|---|
| plain | local | 11.9 / 11.9 / 11.9 | 13.3 / 13.5 / 13.6 | 0.27 / 0.25 / 0.22 | 0.097 / 0.095 / 0.094 |
| plain | mux | 11.0 / 11.6 / 11.7 | 12.4 / 13.3 / 13.3 | 0.30 / 0.25 / 0.27 | 0.140 / 0.134 / 0.134 |
| wrap | local | 10.8 / 10.7 / 10.7 | 12.2 / 12.1 / 12.1 | 0.22 / 0.14 / 0.22 | 0.103 / 0.100 / 0.103 |
| wrap | mux | 10.4 / 10.6 / 10.9 | 11.7 / 12.0 / 12.4 | 0.19 / 0.16 / 0.17 | 0.148 / 0.147 / 0.140 |
| sgr | local | 26.5 / 29.2 / 28.2 | 26.7 / 32.8 / 31.8 | 1.19 / 0.91 / 0.89 | 0.075 / 0.065 / 0.066 |
| sgr | mux | 26.1 / 25.9 / 29.4 | 26.6 / 29.1 / 33.0 | 1.17 / 0.95 / 0.83 | 0.110 / 0.114 / 0.100 |
| unicode | local | 4.9 / 4.9 / 4.8 | 5.0 / 5.1 / 5.0 | 0.52 / 0.52 / 0.47 | 0.230 / 0.226 / 0.228 |
| unicode | mux | 14.1 / 14.4 / 14.2 | 17.9 / 17.8 / 18.2 | 0.52 / 0.50 / 0.47 | 0.238 / 0.242 / 0.229 |
| vte | local | 10.2 / 10.4 / 10.4 | 11.4 / 11.6 / 11.8 | 1.03 / 1.08 / 1.00 | 0.136 / 0.134 / 0.131 |
| vte | mux | 10.4 / 9.6 / 10.2 | 11.7 / 10.8 / 11.4 | 1.05 / 1.17 / 1.03 | 0.163 / 0.182 / 0.168 |

The newer ConPTY takes dense SGR colour (the pattern nearest Claude Code's output) 19 to 24% faster, with about a quarter less of its own CPU; the rest is the same within the runs' spread. 1.25 is a little ahead of 1.24 on the mux domain, as Lachlan runs it. The ceiling is still ConPTY's: shown speed follows what it takes.

## Key to screen (2026-10-31, 11:26 to 11:29)

`key-latency.ps1 -Run -Away -Kinds lbterm,lbterm-mux,wt -Samples 100 -Rounds 2`, with Lachlan's word that it could take the keyboard ("you can also take input over until I say stop") and after 65 s without his input. Real keys (SendInput) into each terminal in turn, with the foreground, on the right-most screen (60 Hz), each sample timed from the SendInput call to the probe cell changing on the screen as DWM composed it (GDI BitBlt; a read took 6.2 to 10.5 ms, so a sample is that coarse). LBTerm is the installed build, 20261030-191144-301f18e6: before perf/budgets, perf/at-rest and perf/key-latency. The PC was busy: six other sessions' builds and tests, 8 to 12 cores.

| Terminal | Round | Median | p90 | p99 | Fastest | Slowest |
|---|---|---|---|---|---|---|
| LBTerm (local) | 1 | 33.6 ms | 48.3 | 77.0 | 16.6 | 77.0 |
| LBTerm (local) | 2 | 29.9 ms | 38.7 | 111.8 | 15.3 | 111.8 |
| LBTerm (mux) | 1 | 34.3 ms | 49.5 | 72.3 | 25.7 | 72.3 |
| LBTerm (mux) | 2 | 34.5 ms | 50.8 | 564.3 | 27.5 | 564.3 |
| Windows Terminal | 1 | 23.3 ms | 37.3 | 158.5 | 14.2 | 158.5 |
| Windows Terminal | 2 | 21.1 ms | 29.6 | 35.4 | 12.9 | 35.4 |

Windows Terminal shows a key 10 to 13 ms sooner at the median. That's about what lbterm/PERF.md's findings 35 and 49 account for: every key asks for a frame of its own before its echo has come, which draws nothing and holds the echo's frame back a refresh (16.7 ms on this screen), and each echo walks every process on the PC (about 18 ms of CPU, on the mux server on a mux domain). perf/key-latency fixes both; this table is its before. LBTerm on the mux domain is a further 0.5 to 4.6 ms behind its local domain at the median and has a slow tail (564 ms once).

## Windows Terminal (2026-10-31, 11:31 to 11:42)

`bench.ps1 -Away -Kinds wt -Phase smoke,startup,throughput,idle -Rounds 3 -SizeMB 32`, Windows Terminal 1.24.11321.0 (its own ConPTY, OpenConsole 1.24.2605.12001, in-process), after 65 s without Lachlan's input and with his word that it could take the keyboard; the PC busy with six other sessions' builds and tests (8 to 12 cores). LBTerm's numbers beside it are from the runs above (the installed build on 2026-10-30, and ConPTY 1.25 in the same build this morning), not interleaved with these, so read small gaps as nothing.

| | Windows Terminal | LBTerm local | LBTerm mux |
|---|---|---|---|
| Window shown | 1,077 ms | 1,269 | 1,234 |
| The shell running | 1,551 ms | 581 | 2,975 |
| Plain lines, MB/s shown | 92.9 (78 to 118) | 11.9 | 11.7 |
| Lines that wrap | 146.3 | 10.7 | 10.9 |
| Dense SGR colour | 8.5 | 28.2 | 29.4 |
| Wide characters and emoji | 15.7 | 4.8 | 14.2 |
| vtebench-like scrolling | 26.2 | 10.4 | 10.2 |
| Terminal CPU per MB, plain | 0.023 s | 0.094 | 0.134 |
| At rest, 1 tab / 8 tabs (% of a core) | 0.39 / 0.52 | 0.42 / 0.41 | 0.31 / 0.70 |
| Memory, 1 tab (private MB) | 74 | 303 | 321 |
| Memory, 8 tabs | 202 | 342 | 359 |
| 100,000 lines of scrollback, same setting | +5 MB | +56 | +59 |

**This corrects "Throughput is ConPTY's" above.** "MB/s taken" is how fast the producer's writes were taken, and that's bounded by the whole pipeline, the terminal reading ConPTY's output included. Windows Terminal's ConPTY takes plain lines at 78 to 121 MB/s, so ConPTY isn't the ceiling at 12 MB/s: LBTerm is, taking about 0.1 s of CPU for each MB of plain output (upstream WezTerm the same) against Windows Terminal's 0.023. Where the time goes (the parser, the terminal model's per-grapheme bookkeeping, the audit's core-12, line storage) is the lead for making plain output several times faster.

Where LBTerm is ahead: dense SGR colour, the output nearest Claude Code's, about 3.3 times Windows Terminal's speed; a shell running 1 s sooner on its local domain; and Unicode on the mux domain. Where it's behind: plain, wrapping and scrolling output (2.5 to 14 times), key to screen (10 to 13 ms, above), and memory (4 times for one tab, and 11 times the scrollback's cost).
