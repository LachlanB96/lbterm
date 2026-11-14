# The state file

A test window says what it shows. With `LBTERM_STATE_FILE` set to an absolute path in a GUI's environment, the GUI writes a JSON snapshot of all its windows to that path after every frame a window paints, so a test waits on what LBTerm says instead of on screenshots and fixed sleeps. The end-to-end scripts in `lbterm/perf` learnt everything by PrintWindow and pixel sampling, with 178 `Start-Sleep`s between them, and some were flaky for it.

Test windows only (Lachlan's pick, 2026-10-21): nothing sets the variable for his daily LBTerm; the test harness (`lbterm/harness.psm1`) sets it for every test window it opens, to `<the test's output folder>\state.json`. Unset, nothing is gathered and nothing is written: the variable is read once, the first time a window asks, and every hook returns at once after that.

The pure part is `lbterm-extras/src/state.rs` (the snapshot's types, what's in flight, the writer), tested with `cargo test -p lbterm-extras state`. The GUI side is `wezterm-gui/src/state_probe.rs` (the process's windows and the writer) and `wezterm-gui/src/termwindow/state.rs` (what one window shows), with a small accessor in each feature's own file. `lbterm\perf\state.ps1` checks it end to end.

## How a test uses it

```powershell
$w = Open-TestWindow ...                     # the harness sets LBTERM_STATE_FILE
Invoke-Cli spawn --pane-id 0 --no-activate | Out-Null
$e = Wait-Change { param($s) @($s.rail.rows).Count -eq 2 } -What 'the new row'   # the change, then settled
$x, $y = Get-RectCenter $e.rail.rows[1].pad  # where to post the pointer
```

The harness's `Read-State`, `Wait-State`, `Wait-Settled`, `Wait-Change` and `Get-Frames -Until` (`lbterm/harness.psm1`) do what follows; a predicate gets the window's entry, found by its HWND, and the whole snapshot.

- **Read it shared for delete.** The GUI writes `<path>.tmp` and renames it over `<path>`, so a reader sees the old file or the new one, whole, never half of one. A reader that opens it without `FILE_SHARE_DELETE` (`Get-Content`, `[IO.File]::ReadAllText`) holds that rename up until it lets go; the writer tries again for 250 ms, so it isn't lost, but open it as the harness's `Get-StateSnapshot` does: `[IO.File]::Open($path, 'Open', 'Read', 'ReadWrite, Delete')`.
- **Find your window** by `hwnd` (what `Wait-Window` returns) or `id`.
- **Wait on a change, then on settling.** After a `lbterm cli` call or a posted message, poll the file (every 10 to 20 ms) until it shows what the action does (two rows, two panes, `rail.open`), then until `animating` is false (`Wait-Change`). A frame painted before the action took effect can say `animating: false` too, so waiting for `animating: false` alone right after the action can return at once.
- **Frames only go up**: `frame` at the top is every frame this process has painted, a window's `frame` its own. A test can note a window's `frame` before it acts and wait for a later one.
- **`settled`** is `animating` false and no `waits` under way: nothing will change on screen by itself. A notice ("Config reloaded", 4 s) or a landed mark (1.6 s) is a wait, so a test that doesn't care about those waits on `animating` instead.

## When it's written

- After every frame any window paints, at the end of `paint_impl` (`render/paint.rs`), so it describes what that frame drew.
- Once when a window is made, before its first frame (`frame: 0`), and again when it goes (`WindowEvent::Destroyed`), without it.
- By a thread of its own (`lbterm-state`): the GUI thread builds the snapshot and hands it over; the thread serialises and writes it. A snapshot handed over while one is being written replaces any still waiting: the newest wins, and a burst of frames costs one write after the one under way.
- **The frame an animation ends on.** Most of LBTerm's motion runs in DWM (`compositor.rs`): the GUI sets targets and paints nothing while the panes slide, the rail opens or the strip's card fades. So the compositor keeps every animation it starts, by name, from its start to its end (`drive` and `play`, where every animation that ends is made; the ones that never end are `loops`; in every window, not only a test window, since the budget takes a window as busy while one plays: lbterm/PERF.md), and a window with any in flight asks for one frame more 20 ms after the last of them ends. That frame's snapshot says `animating: false`. It comes after the compositor's own wake 2 ms past a fade's end, which puts what faded away with a commit and no frame (lbterm/COMPOSITOR.md): this frame's `prepare` would otherwise put them away first, and no test could tell whether the wake does, which Lachlan's windows, with no state file, rely on (2 ms until 2026-10-30). Without the compositor (OpenGL, `lbterm_compositor = false`) or with motion off, nothing animates in DWM and everything is there in the frame that shows it.

## What it holds (version 1)

A field taken away or changed in meaning bumps `version`; one added doesn't. Absent things are `null`, never missing, so `$w.rail -eq $null` means the tab panel is off, not a typo.

### Top level

| Field | |
|---|---|
| `version` | 1 |
| `pid` | the GUI's process id |
| `build` | the version, as `lbterm --version` says it |
| `frame` | frames painted by this process, all windows; only ever goes up |
| `time_ms` | when the snapshot was taken, Unix milliseconds |
| `windows` | each window, by id |
| `pace` | how the GUI thread paces its frames, all windows, since the process started (below) |
| `work` | what the GUI thread does besides painting, all windows, since the process started (below) |

### `pace`

What the message loop and wm_paint did, counted always (a relaxed atomic add each) and read here: `lbterm-extras/src/pace.rs` has the rules, `window/src/os/windows/pace.rs` the Windows side. A window paints, then holds its next frame back one period from that paint's start, a frame of `max_fps` and never less than a refresh of its screen (its exact rate, QueryDisplayConfig's); a frame asked for during the hold is painted when it ends, on a high-resolution waitable timer the loop waits on. A test takes two snapshots and subtracts. `perf/frame-pace.ps1` reads them.

| Field | |
|---|---|
| `turns` | turns of the message loop: each runs the queued tasks, ends the holds that are up, then handles a message or waits |
| `paints` | WM_PAINTs that painted a frame |
| `held` | WM_PAINTs that came while a frame was held back: about one for each time something asked for a frame during a hold. Before 2026-10-30 the window wasn't validated then and Windows sent it again on every turn, hundreds a frame |
| `paint_cycles` | the GUI thread's cycles in those paints (QueryThreadCycleTime), what painting cost the thread; a wait on DWM inside a paint isn't counted, as it isn't the thread's |
| `holds` | holds that ended with a frame asked for during them |
| `late_us` | how much later than due those ended, in all, in microseconds |
| `late` | those that ended over 2 ms late (on Windows' 15.6 ms tick most did, until 2026-10-30) |

### `work`

What the GUI thread did besides painting, counted always (a relaxed atomic add each) and read here: `lbterm-extras/src/work.rs` has the rules, `window/src/os/windows/work.rs` the Windows side. Each turn of the message loop drains the main thread's task queue first; every 0.5 ms of a drain it asks Windows whether a message waits for the window (GetQueueStatus: a key, the pointer, a posted message, which is how a test's keys and moves come), and stops for one, which the loop takes before it drains the rest (since 2026-10-30; the sizing loop's pumps still drain it all). A test takes two snapshots and subtracts. `perf/gui-thread.ps1` reads them.

| Field | |
|---|---|
| `tasks` | tasks run from the main thread's queue |
| `drains` | drains of the queue that ran at least one task |
| `drain_us` | their time in all, in microseconds |
| `long_drains` | those over 2 ms |
| `waited` | drains in which a message was seen waiting for the window before they ended |
| `waited_us` | how long those messages waited in their drains after they were seen, in all, in microseconds |
| `long_waits` | those that waited over 2 ms |
| `ime` | times the IME's window was moved to the text cursor (since 2026-10-30 only when the caret moved, or the window's keyboard or size changed) |
| `cursors` | times the mouse cursor was set (since 2026-10-30 only to another shape, or after the pointer was over the frame or left the window) |
| `accent` | times ColorPrevalence or the accent colour was read for the window's border (the registry, a UISettings object): since 2026-10-30 once, and again after Windows says a setting or the colorization colour changed |
| `lines` | panes' lines laid out as quads again, missing the line quad cache |
| `line_shapes` | lines turned into shaped runs again: panes' lines missing the line-to-element cache (those of the `lines` whose words no cached line had), and the retro tab bar's line each time it's drawn (since 2026-10-30) |
| `shapes` | runs of a pane's text (or the retro tab bar's) the font shaped again, missing the shape cache (since 2026-10-30) |
| `cut` | drains that stopped with tasks left for a message waiting |
| `unseen` | frames painted while the window couldn't be seen (minimized, or the session locked or the screens off, which a window with a state file goes by only with `LBTERM_UNSEEN=skip`): one a second at most |
| `keys` | key presses (WM_KEYDOWN, WM_SYSKEYDOWN) the message loop took off the queue |
| `key_qpc` | when it took the last, as QueryPerformanceCounter's count: a test that reads the same clock as it posts a key (.NET's `Stopwatch.GetTimestamp()`) has all the key waited in the queue, behind a paint, a task or a drain, where `waited_us` is only what a drain kept it after it was seen (`perf/gui-thread.ps1`'s keys) |

### A window

| Field | |
|---|---|
| `id` | the GUI's window id (Lua's `window:window_id()`; on a mux domain, not the server's) |
| `hwnd` | its HWND, to post messages to |
| `frame` | frames it has painted, 0 before its first |
| `focused` | Windows gave it the keyboard (a test window never has it) |
| `keyboard` | it shows as having the keyboard: focused, or a test window (`LBTERM_NO_ACTIVATE`) that no posted `WM_KILLFOCUS` has told otherwise |
| `held` | the modifiers it takes as held, `["ctrl", "shift", "alt"]` or fewer, in that order: what its last key or pointer message carried, none without the keyboard; they colour the active outline (`termwindow/held.rs`). A posted key leaves Windows' key state alone, so tests hold them with a pointer message's `MK_CONTROL` and `MK_SHIFT` |
| `tint` | those of `held` the active outline's colour shows: all of them, but Shift while a tab is right-dragged on the rail, where it says the drag connects tabs and the drag is cyan for it (`lbterm_extras::links::outline_held`, lbterm/LINKS.md, "Connections"); Shift still held as the drag ends, it's back |
| `animating` | true exactly while it has asked for another frame to carry an animation, fade or glow on: something in `animations` |
| `animations` | `[{name, progress, left_ms, played}]`, soonest to end first: what's moving (below), how far through its time (0 to 1), how long it has left, and what it plays as numbers: `played`, `[{from, to, low, high, ms}]`, one for each number the compositor gave DWM under that name in this animation (a pane's x, y, width and height are four), the newest 8 in the order they started: where it starts and ends, the least and most it reaches on the way (read every millisecond of its cubics, `lbterm_motion::Segments::span`) and how long it lasts. Left out for a name nothing gave numbers for (the hearts the GUI draws, upstream's frames). A test checks a look's values here, not in pixels (Lachlan's 3a of 2026-11-09): perf/menu-keys.ps1's flare reaching 1 and going back to 0. Worked out only in a window with a state file, so Lachlan's pays nothing for it (`lbterm_extras::state::InFlight::played`, compositor.rs's `record`) |
| `waits` | `[{name, left_ms}]`: delays under way that will change what shows when they're up (below) |
| `settled` | `animating` false and no `waits` |
| `loops` | animations that never end in the window's visual tree as the frame ends, carried by DWM (below), read from the compositor's own spinners, outlines and lamps (what DWM was given, not what the panel's plan asked for): `rail.spinner`, `rail.starting` (the Starting row's arc, only while a new tab is on its way: lbterm/NEW-TAB.md), `conductor.outline`, `conductor.light`, `menu.spinner` (only while a menu item waits), `link.lamp` (only while a tab link drag matches), `connection.light` (while a light along a tab connection suggested on the rail runs its laps: three when the suggestion comes, when the file says something new of it and when the pointer comes onto the rail, none between them, when it's still; a frame comes to say so once they end, lbterm/LINKS.md); they don't count as `animating`. None while the window can't be seen (minimized; a window without a state file, or with `LBTERM_UNSEEN=skip`, also with the session locked or the screens off): they hold still then (lbterm/COMPOSITOR.md, "Loops") |
| `size` | `{width, height, dpi}`: the client area in pixels |
| `pointer` | `{x, y, over, row, item, pane, button}`: where the pointer last was (posted or real), and what's there; `null` once it has left the window |
| `active_tab` | the window's index of the open tab |
| `active_pane` | the pane with the keyboard (an overlay's, while one is up) |
| `tabs` | `[{index, id, title, active, panes, zoomed, colour}]`; `title` is the tab's own, empty until one is set (the rail's row says what it shows); `colour` is the tab menu's, `"#rrggbb"` |
| `panes` | the open tab's panes as painted (a zoomed tab's one): `{id, server_id, rect, cells, cols, rows, active, zoomed, title, alt_screen, mouse_reporting, cursor: {x, y}, drawn_cursor, scroll_top, session, progress, weights}`. `weights` (only in a pane with a Claude session, and only when it has one) is each option's share of 100 its last frame drew in its colour, top first: `{row, col, share}`, the row from the pane's top as drawn and `col` its digits' first column (lbterm/QUESTIONS.md, "The weights"). `rect` is the pane's card, `cells` its cells as its text is drawn (a cell is `cells.w / cols` by `cells.h / rows` exactly, from `cells.x`, `cells.y`). Find a row or a column by `cells`, never by `rect`: a card holds padding and what's left of a cell at its edges, so `rect.h / rows` is no row's height (a lone pane in a 1024 by 698 window: card 908 by 637 at 76,57, cells 900 by 620 at 80,65, 31 rows of 20) |
| `panes_tab` | the tab whose panes `panes` are, by its id (`tabs[].id`): the open tab's, but while a switch's held pane hasn't had its push (lbterm/MUX.md, "The switch's first frame"; Lachlan's 2b of 2026-11-08) the tab drawn before it, whose panes the frame draws in the open tab's place as they were, the rail and the strip already the open tab's; `null` when nothing was drawn before such a wait and no pane is (`panes` empty) |
| `fading_panes` | the ids of panes whose visuals are still on screen after leaving it, fading out: a closed pane's under the rest, the panes of a tab switched from or closed over the next tab's (`pane.dissolve`). Empty once every fade is done: each is put away just after its fade ends, without a frame, so the frame that says `animating: false` lists none |
| `fading_sides` | how many pictures of the strips down the right (the scroll strip and the objectives strip) as a tab switched from or closed showed them are still on screen over the next tab's, fading out with its panes (`sides.dissolve`). 0 once each fade is done: they're put away with the panes |
| `fading_swaps` | how many of the strip's words in flight are older than the newest, still on screen fading out over it: a tab switched through while the swap before it still faded keeps its words fading with its panes (`strip.swap`), six in flight at most (lbterm/COMPOSITOR.md, "Motion"). 0 once each fade is done: they're put away with the panes |
| `spare_panes` | how many pane surfaces the compositor keeps for panes to come. Once nothing fades, no more than the most panes the window has had on screen at once (one at least, four at most): a run of quick switches makes one for each tab still fading, and they're let go after |
| `scrollbar` | `{enabled, shown, marks}`: whether it shows beside the active pane (not while its program has the whole screen), and its prompt marks |
| `rail` | the tab panel, with `lbterm_tab_panel` on (below) |
| `strip` | the window strip's words, while the tab panel hides the tabs (below) |
| `objectives` | the objectives panel down the right, with `lbterm_objectives` on (below) |
| `context_bar` | the context bar in the objectives panel's room, with `lbterm_context_bar` on (below); while it's on, `objectives` is `null` |
| `scroll_strip` | the scroll strip, the open tab's scrollbar left of the objectives panel, with `lbterm_scroll_strip` on (below) |
| `projects` | the projects pad on the strip's second row and its card, with `lbterm_projects` set (below); `objectives.projects` until 2026-11-08, when the pad left the objectives strip |
| `cost` | the cost pane the context pill's press opens, while the pill shows (below; since 2026-11-09) |
| `menu` | the menu that's open (below) |
| `copied` | what the window last copied, here in place of the clipboard: the tab menu's Copy link (`[<label>](lbterm://session/<id>) in <Project>`, lbterm/TAB-MOUSE.md), a selection's let-go, `CopyTo`, the Conductor's view's copy; `null` until something is copied. With the state file on, every copy writes here and never to the clipboard (termwindow/clipboard.rs's `copy_to_clipboard` asks render/tab_menu.rs's `lbterm_copy_to_state` first), since a test window shares Lachlan's (lbterm/PITFALLS.md, 151). A paste into the new-tab picker's box (a key bound to `PasteFrom` while it's open, his 11a) reads it back from here, never from the clipboard, so a test copies its words first (Lua's `window:copy_to_clipboard` lands here too) and pastes them; a pane's paste still reads the clipboard |
| `new_tab` | `{rect, zone, hover, open, hidden, picker, start, starts, keys, closed}`: the new-tab pad at the rail's foot (lbterm/NEW-TAB.md), with `lbterm_tab_panel.new_tab` on, else `null`. `rect` its pill, `zone` where a press lands on it (its slot across the rail, on into the screen's corner while the window is maximized), `hover` the pointer on it, `open` its picker or its + menu open (it shows lit), `hidden` how many tabs the rail has no room for ("+N" over it; the Starting row's slot isn't one). `picker` while it's open: `{mode, rect, input, query, rows, highlight, message, none, started, footer}`, `mode` `claude` or `shell` (the + menu's Shell in a project…), `input` the box and `query` what's typed in it, `rows` the whole list, row 0 nearest the box, each `{name, dir, group, repo, marks, rect}` (`dir` where a pick opens, in Windows' spelling: backslashes, as the config spells its folders) (`group` `here`, `home`, `recent` or `all` with nothing typed, `null` once something is; `marks` the name's characters the first word matched; `rect` `null` scrolled away), `highlight` the highlighted row, `message` the first message the words after the name make, `none` "No project called …", `started` "Started …" after a start behind, `footer` what the keys do, `asked` the last frame painted when it opened and `shown` the first that drew it (`null` until one has), so a test judges "in the next frame" from the record. `start` the last tab asked for in the window, kept after it's done: `{id, label, asked, asked_ms, shown, ended, ended_frame, ended_ms, tab, behind}`, `id` which start it is (from 1 in the window), `asked` the last frame painted when it was asked for and `asked_ms` when, in Unix milliseconds by the GUI's clock, `shown` the first that drew its Starting row (`null` where the tab came before the next frame), `ended` `arrived` (with `tab`, the tab that came) or `folded` (20 s up, or Lua's `window:lbterm_expect_tab(nil)`) and `ended_frame` the frame that showed it and `ended_ms` when it ended by the GUI's clock (a fold is never before `asked_ms` + 20,000, and the strip's notice it puts up goes never before `ended_ms` + 4,000), `behind` opened behind. `starts` every tab asked for lately, oldest first, `start` the last of them (the eight newest): several can be on their way at once after Shift+Enter, the rail showing the oldest's Starting row, and each tab that comes is the oldest waiting's. `keys` `{picker, shell, reopen}`: the keys the key table binds to `LbtermNewTab`'s three, as the pad's row and the + menu name them ("Ctrl+T"), `null` where nothing's bound. `closed` the tabs Lachlan closed by hand lately, newest first, that Reopen and Ctrl+Shift+T open again (ten at most, in memory alone): `[{label, dir, project, sessions, at}]`, `dir` where Reopen opens it (its session's folder as cwd.txt says it, else its pane's; `""` when neither said one), `project` the picker's name for the project that's in (`""` for none), `sessions` its panes' Claude sessions, the active pane's first, `at` when it closed in Unix seconds. The + menu is `menu`'s kind `plus` |
| `overlay` | `"modal"` (command palette, character select, pane select), else the title of an overlay with a pane id of its own over the active pane (the launcher, the tab navigator, the close confirmations), else `null`. Copy mode, search and quick select report the id of the pane under them (CODEMAP.md, "A frame", the two kinds of overlay), so they're `null` here: tell them by the pane in `panes`, whose `server_id` is `null` under one on a mux domain (an overlay has no `remote_pane_id`) and whose `title` is `Copy mode: ...` under copy mode or search |
| `readout` | the size readout while it shows or fades: `"120 × 32"`, `"110% · 109 × 29"` |
| `landed` | `{pane, row, left_ms}`: the row a prompt jump, a scrollbar mark or a search landed on, while it's marked |
| `questions` | the pane whose question colours show |
| `questions_pane` | the questions bar while a hold shows it (QUESTIONS.md, "The questions bar"): `{rect, rows: [{label, colour, answered, age, arrow}], more}`, `rect` the bar's whole height, `arrow` `up` or `down` while the question's words are off the screen that way, else none, each row shown top to bottom, `more` the rows its "+N" slot counts; left out while it isn't shown |
| `hearts` | bursts of hearts in the air |
| `conductor_view` | the Conductor's view while it's open in this window (below), else `null` |
| `tasks` | `{row, rect, lines}`: the card of what a tab's Claude session has running, while it's open beside row `row` (lbterm/TAB-PANEL.md, "What's running"). Each line as it reads, its parts a space apart: `"Agent Review the card 12m"`, the subagent's `"Edit tasks_card.rs · 31 tools · 84k tokens"` under it, `"+3 more"` |
| `thumbnails` | `[{pane, path, state, rect, hovered, version, places}]`: each picture a pane drew of an image it links to (lbterm/THUMBNAILS.md); `state` is `"shown"`, `"loading"`, `"gone"` or `"unreadable"`, `rect` the picture as drawn (its box when there's no picture), the part in view; `version` which decoding of its file is drawn (0 while none is), `places` how many places in the atlas it has taken (1 while its file is rewritten at the same size) |
| `drafts` | `[{pane, text, rect}]`: each voice draft's card over its pane (lbterm/CONTROL.md, "Drafts"); `pane` the mux server's id, `text` what it says, new lines and all |
| `ticked` | the questions a voice draft has ticked on the pane whose question colours show, by number |
| `next` | `{label, off, rect}`: the red ring round the next question that voice draft hasn't answered (lbterm/QUESTIONS.md), `null` while there's none; `label` its marker's words (`"Q2"`), `off` true when the ring's round its marker at the screen's edge, `rect` the ring as drawn |
| `visuals` | `{drawn, kept, which, made, bind_groups, uploads, upload_bytes, built, passes, submits, upload_us, record_us, submit_us, stale}`: with the compositor, how many of its visuals the frame drew and presented, how many it left as they were because their swapchains showed what it would have drawn already (lbterm/COMPOSITOR.md, "Only what changed is drawn"), and which it drew, in the order it drew them, as the compositor's `Target` names them (`Over`, `Swap`, `Swatch`, `Pane(3)`, `Panel(Rows)`); `null` without the compositor. An InMail change's frame draws 2 or 3 of a four-pane window's 24, `Over` and `Swap` (and the lights' `Swatch` the first time), the frame after DWM's dissolve none (`perf/inbox-cost.ps1`). What drawing them took on the GPU (COMPOSITOR.md, "What a frame costs"): `made`, the buffers and bind groups it made (a layer grown or let go, a new atlas, a visual's new place); `bind_groups`, the bind groups the window keeps after it (termwindow/gpu_work.rs, `kept`: the atlas's two and a uniform's for each place a visual was drawn at in the last 600 frames at the window's size and the one before, at most `config::lbterm::GPU_UNIFORMS_KEPT`), a uniform's each holding a buffer of 64 KB of the GPU's memory, and with D3D12 each taking a view in the device's view heap, the atlas's two each (lbterm/PERF.md, finding 96; `perf/gpu-memory.ps1` resizes a window drawing every visual and counts them); `uploads` and `upload_bytes`, the runs of quads it wrote to the layers' GPU copies; `built`, the templates it made again (`card-mask-wide`, `card-mask-tall`, `card-line-wide`, `card-line-tall`, `ring-spectrum`, `ring-iris`, `ring-light`, `panel-glow`, `panel-lit`, `panel-spot`, `panel-spot-core`, `connection-light` while a tab connection is suggested, lbterm/LINKS.md, `menu-glow-tab`, `-prompt`, `-conductor` or `-list` as a menu opens, moves or changes size in its slot, and `picker-glow` the picker's), each only when what it's made from changed. A frame of a pane's output makes nothing and builds nothing (`perf/paint-cost.ps1`). How it drew them: `passes`, the render passes it recorded, one a visual drawn; `submits`, the submits it made of them, one for each eight visuals (D3D12's limit on the swapchains one may write to), none when it drew none; and in microseconds how long it took to gather what each visual draws, the layers' GPU copies written as it went (`upload_us`), to record the passes (`record_us`, each ended as it's recorded, which is when wgpu does most of a pass's work) and to finish and submit them (`submit_us`). `stale`, in a test window with `LBTERM_CHECK_UPLOADS` in its environment, the quads a write to a layer's GPU copy left out because their hashes matched what it held though their vertices didn't, a write to a quad its hash doesn't fold in (lbterm/PITFALLS.md, "Caches and memory"): always 0, and `null` where it isn't checked |

Rectangles are `{x, y, w, h}` in the window's client pixels, which `WM_MOUSEMOVE`'s `lParam` and a client-area capture count in: post a move to a row's pad with `x + w/2, y + h/2`. A pane's `rect` is its card where it's laid out; while the tab panel is open, DWM shows it pressed to the right without moving it. `server_id` is the mux server's pane id on a mux domain (what `lbterm cli` takes there), `null` when it's the GUI's own. `cursor.y` counts from the top of the pane's screen. `cursor` is the pane's as the snapshot is taken, just after the frame; `drawn_cursor` is where this frame drew it (`null` when the frame didn't draw the pane), which differs when output came while the frame was being drawn: to know that a frame shows a key's echo, go by `drawn_cursor` (lbterm/PERF.md, finding 34). `scroll_top` is the stable row at the top of the view while it's scrolled back, `null` at the bottom. `session` is the pane's `claude_session` user var. `progress` is OSC 9;4's: `"42%"`, `"error 42%"`, `"indeterminate"`, `"paused"`.

### `rail`

| Field | |
|---|---|
| `open` | open, or still open while it waits out its grace |
| `phase` | `closed`, `opening` (waiting out `open_delay_ms`), `open`, `closing` (waiting out `close_delay_ms`), as the panel shows: while `held`, the held state's (closed with its light pending is `opening`), not the one underneath, whose waits `waits` still counts (`rail.open`, `rail.close`). Until 2026-11-08 it was the one underneath, so a held rail read `open: false` with `phase: open` (`lbterm_panel::Hover::shown_phase`) |
| `held` | the panel held still under a menu's card, open or fading out (`Hover::hold`; FLOWS.md, "A menu, from what opens it to the end of its fade", step 10): `open`, `phase` and `light` say what shows, the pointer and its delays going on underneath. Since 2026-11-08 (Lachlan's 7b) the rows, the marker, the pads' arcs and glints and the Conductor's outlines hold too, with the compositor: `rows` say the rail as it is now, which the screen shows only once the card has gone, its moves (`rail.row.exit`, `rail.row.slide`, `rail.marker`, `light.glint`, `light.send`) starting in that frame |
| `light` | the ring's light: `rest`, `pending`, `full` |
| `rect` | the panel's card, open or closed as `open` says |
| `labels` | the labels under the pads: `shown`, or `faded` while the panel is open, where each open row's title says the same name (Lachlan's 12a, lbterm/NEW-TAB.md, "Landing 4"): the open panel's words cover them, so with the compositor they fade out as `rail.words` fades in, and back as it fades out; without it, at once |
| `rows` | the rail's rows, in order (below) |
| `hovered` | the row under the pointer |
| `open_row` | the row with the open tab's ring and bar; `null` while they're on the Conductor's pad |
| `conductor` | `{level, label, remote, tab, open, hovered, outline, acting, pad, asks, suggested, badge, mark}`: the Conductor's pad, with `lbterm_conductor` set; `level` is `off`, `read`, `most` or `all`; `outline` what's round its pad (`lbterm_extras::conductor::outline`): `none` (off), `grey` (read only), `mode` (Accept most or all at rest, a still hairline in the mode's colour) or `spectrum` (while it acts there, or under the pointer; with the compositor its ring fades in and out on `panel_glow`, `conductor.outline.fade` among the animations while it does, Lachlan's 7a, NEW-TAB.md, "Landing 4"); `acting` its hub naming a session it acts on, any, its least lap and linger included (`conductor_hub::acting`; false without a hub); `open` is its tab the open one or its view open (the open tab's ring and bar on the pad); `asks` how many of its asks wait on him (the pad orange while any do); `suggested` how many tab connections are suggested with at least one member on this rail, the count on its cyan badge (lbterm/LINKS.md, "Connections"), changes he's made that the links file doesn't say yet included; `badge` where that badge is drawn, sized for its count in the pads' numbers' font, `null` at 0; `mark` where the remote control's mark is drawn on the pad's other corner while `remote`, else `null`: the badge stays clear of it |
| `dragging` | the row of a tab being dragged along the rail |
| `drop_hint` | a tab dragged from another window is over this one, and the rail shows where it would land |
| `links` | the tab links' lines between rows (lbterm/LINKS.md), `[{waiter, on, lane, look, x, top, bottom}]`: the waiting row and the row it waits on, its lane (0 nearest the pills), `look` `waiting`, `done`, `error` or `suggested`, `x` its lane's middle, and `top` and `bottom` the ys of its two ends |
| `connections` | the tab connections' lines between rows (lbterm/LINKS.md, "Connections"), `[{id, state, lane, rows, x, ys, mark_x}]`: one for each connection suggested or made with two members or more on this rail (a declined one is none), `state` `suggested` or `connected`, its lane among the links' (0 nearest the pills), its members' rows on this rail top to bottom, `x` its lane's middle, `ys` the y of each of those rows' marks and `mark_x` their middle across |
| `link_drag` | a tab being right-dragged onto another, `{row, over, matched, connect, waits, aims, waits_match, aims_match, waits_words, aims_words}`: the row picked up, the row under the pointer it would wait on (or connect with), whether that tab has the objective its wait matches, whether Shift is held so that letting go connects the two (lbterm/LINKS.md, "Connections": the drag cyan, nothing matched), the card of what it waits for (connecting, of its own objectives) and the card of that tab's objectives, the matched block in each, and each card's lines of words |
| `line_card` | the card of the line the pointer rests on, `{line, connection, rect, lines}`: a link's line by its place in `links` (`connection` null), or a tab connection's by its id (`line` null), the card, and its lines of words |

A row: `{row, tab, tab_index, visible, rect, pad, label, title, project, state, status, message, progress, estimated, left, agent_bars, agents, waiting, extras, asks, quiet, link, connected, unread, age, bars, cost, prompted, active, hovered, colour, conductor, pending, hidden, hidden_pending, parked}`. `bars` is the idle timer's bars drawn over its pad, bottom first (`green`, `yellow`, `red`): only the lit ones are drawn since Lachlan's 12a (lbterm/NEW-TAB.md, "Landing 4"), so it's `[]` for a tab at work or without Claude, and on a row too short for the timer. `hidden` is the row looking hidden from the Conductor (lbterm/NEW-TAB.md, section 8): every one of its tab's Claude sessions on the Conductor's button.json `hidden`, or a Hide from Conductor picked on its tab menu still waiting for that; `hidden_pending` that what it shows is such a pick still waiting, for up to 5 s (a Show waiting too, with `hidden` false), after which it says what the file says and the strip's `notice` says `The Conductor didn't hide <label>` (or `didn't show <label> again`). A test judges a pick from these, by frames. `parked` is the mux tab's flag (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing 4"): the row dimmed on the rail and a "Parked" chip on the open panel; on a mux domain it changes at the server's word, never at the window's own (lbterm/MUX.md, "Parked tabs"). `pending` marks the Starting row (lbterm/NEW-TAB.md, "The Starting row"): a tab asked for in this window and not come yet, after the tabs, its `label` the project's or the shell's, `state` `starting`, `tab` `null` (it has none yet, and an id of 0 is a real tab's); the oldest start's while several wait; it goes as the first tab the window didn't have comes (that tab's row grows out of it), or folds after 20 s or when Lua says `window:lbterm_expect_tab(nil)`, the strip's `notice` saying `Nothing opened for <label>`. `rect` is its slot across the open panel, `pad` its pad on the rail. `state` is `none`, `working`, `waiting` (blocked on you), `idle`, `error`, `linked` (waiting on another tab, lbterm/LINKS.md) or `stuck` (working, but its reply has written nothing for 7 minutes with no tool running: the amber pad, lbterm/TAB-PANEL.md); `quiet` that it's working with nothing from its reply for 3 minutes (the amber ring round its green pad); `link` where its tab link stands while the link decides its pad (`waiting`, `telling`, `told`, `stuck`, `failed`, `closed`) and `asks` its orange ring, linked and asking him something too; `connected` the ids of the tab connections made that its session is in (its other members on this rail or not), which leave its pad and words as they are, not one the rail draws under `pending` before the file has it; `status` the word its pad says ("Working", "Question", "Extras", "Your turn"); `waiting` how many of its program's own tasks still run after it handed the turn back (the turning arc); `extras` that it's finished with only extras on offer (`idle`, its blue pad ringed in orange); `progress` is its program's, else the Conductor hub's guess (`estimated` true, `left` the time left as the open row says it, "~4 min"); `agent_bars` its subagents' guessed percents, oldest first (lbterm/TAB-PANEL.md, "The Conductor's guesses"); `conductor` is `thinking` or `acting` while the Conductor is on its session. `cost` is what its Claude conversation has cost, as the open row's second line ends with it (`$12.34`), empty until it's known (lbterm/CONTEXT.md). `prompted` is how long ago Lachlan last sent its Claude session a prompt, as the open row's second line starts with it (`just now`, `4 min`, `2 h`, `3 d`), empty while it isn't known, as for a tab without Claude (lbterm/TAB-PANEL.md, "Since his last prompt"); it's the row's words whether the panel shows them or not, and a narrow panel can leave them out. The Conductor's own tab has no row.

### `strip`

`{title, summary, caption, notice, prompt, note, badge, card, restart, inbox, agenda, context, mic, jumping, lit, items}`: the words as `lbterm/DESIGN.md`'s "The window strip" names them. `card` is `summary` or `prompt` while the card under the strip is open. `restart` is the "Restart to update" pill showing. `inbox` is how many messages the InMail button says wait, `null` without the button (lbterm/INBOX.md). `agenda` is what the agenda's button counts, `{urgent, failing}`, `null` without it (lbterm/AGENDA.md). `context` is what the context pill under the InMail button says, the open tab's Claude cost (`$12.34`, or `$–` before it's known), `null` while it doesn't show (lbterm/CONTEXT.md); a click on it does nothing, and it's never in `lit`. `mic` is the mic mark while it shows, `listening`, `hearing` or `muted` (lbterm/CONTROL.md, "The mic mark"). A notice is a wait under way (`strip.notice`), so a window isn't `settled` while one shows: wait on the state alone for it. `jumping` is a jump back to a prompt under way (Claude Code's view being scrolled to it). `lit` is what's lit under the pointer (`close`, `maximize`, `minimize`, `restart`, `words`, `prompt`, `arrow`, `inbox`, `agenda`, `projects`), and `items` where each of those is, with `context` (the pill) too, `[{name, rect}]`.

### `objectives`

`{strip, marks, list, detail, hovered}` (lbterm/OBJECTIVES.md). `strip` is the closed strip, its ring included. `marks` is a mark per visible rail row, `[{row, mark, rect}]`, level with its pad: `mark` is `done`, `first-done`, `drifted`, `waiting` or `open`, `null` for a tab without objectives. `list` is the card of each tab's first objective while it's open, `{rect, rows: [{row, text, tally, mark, rect}]}`, `text` the first objective (or the tab's name without one) and `tally` "2/3" (empty with one). `detail` is the card of one session's whole list while it's open, `{row, rect, lines}`, its lines top to bottom: the tab's name, then each objective's status, words and evidence. `hovered` is the rail row under the pointer, whose detail shows. The projects pad and card were `objectives.projects` until 2026-11-08; they're the window's `projects` now (below), the objectives strip on or off.

### `projects`

`{pad, open, card, rows, more, hovered}` (lbterm/OBJECTIVES.md, "The projects card"), while the pad shows on the strip's second row (`lbterm_projects` listing projects, the tab panel, the strip's second row), `null` otherwise. `pad` is the pad's pill, as the strip laid it out (the same rect as `strip.items`' `projects`), what the pointer rests on or presses to open the card. `open` is the card open and `card` where it is while it's open: under the strip, its left edge the pad's or the panes' left, whichever is further right. `rows` (while open, busiest first, each repo under its project) `[{name, parent, mail, working, waiting, idle, parts, rect, chips: [{action, rect}]}]` with `parent` the project a repo's row is listed under (`null` on a project's) and `action` `new`, `inmail` or `resume`, `more` the line saying how many didn't fit, `hovered` the row under the pointer by its place in `rows`.

### `cost`

`{pill, count, source, open, rect, note, total, rows, compactions, compacted, graph, steps, marks, words}` (lbterm/CONTEXT.md, "The cost pane"), while the context pill shows, `null` otherwise. `pill` is the pill's rect, what a press opens the pane from (the same as `strip.items`' `context`). `count` is which way the pane is: true with LBTerm's own count on (`lbterm_context.count_cost`); false (the default) with it off, when nothing of the count runs and the pane shows the Claude project's split (`@Claude:cost`'s `<session>.cost.json`, since round 4's stage 3 of 2026-11-10). `source` is whose split the open pane shows: `count` LBTerm's own, `claude` the Claude project's; `null` while it shows neither (being counted or read, or none to show), when `note` says why, `total` is `null`, `rows`, `compacted` and `marks` are empty, `graph` is `null` and `steps` and `compactions` are 0 (with the count off the pane then says the pill's dollars and the context window from `usage.json` above the note). `open` is the pane open (not while it fades out). `rect` is the pane's, `null` while it's shut. `note` is what it says in place of a count: `Counting what this conversation has cost…`, or why there's none (`No transcript for this conversation in …`), or with `count` false `Reading what spent it from the Claude project…` (the first opening on a conversation), `The Claude project hasn't published what spent it for this conversation yet` (no `cost.json` for it), `The Claude project's split is version 2, and this LBTerm reads version 1`, `The Claude project's split can't be read: …` or `No summaries folder (lbterm_summaries) to read the Claude project's split from`. `total` is the split's total in dollars: the own count's unrounded (the pill's dollars before they're cut to the cent), the Claude project's its file's `usd`, to the cent. `rows` are `turns`, `subagents` and `workflows` in that order, each `{spender, runs, usd, input, writes, reads, output, rect}`: `runs` the subagents or workflows that made a request (0 for his turns), `usd` its dollars, the four its tokens by type (cache writes and cache reads), `rect` its row. `runs` for workflows, from the Claude project's split, is its runs, however many agents each ran. `compactions` is how many the transcript recorded (no dollars: Claude Code records none), and `compacted` each one's name on the graph in time order, its sizes whether or not the graph had room for it (`545k → 55k`, the context it compacted and the context of the first request after it; `545k → …` until that request is made; `null` for one with no size recorded). `graph` is the graph's plot, `null` when the pane has no room for it or nothing to draw; `steps` how many requests it steps up at (from the Claude project's split, how many minutes something was spent in), `marks` each mark on it in time order (`subagents`, `workflows`). `words` are the words drawn, in order, for a test to read.

### `scroll_strip`

`{strip, track, mode, pane, thumb, marks, rows, view_top, card, seeking, note, note_head, looks}` (lbterm/SCROLL-STRIP.md). `strip` is the strip, its ring included, and `track` where the thumb and the marks go. `mode` is `claude` (the conversation in Claude Code's fullscreen view), `shell` (the pane's scrollback) or `none`, and `pane` the pane it's the scrollbar of. `thumb` is where the view is, `null` while Claude Code's isn't known yet. `marks` is every mark drawn, top to bottom, `[{kind, rect}]`, `kind` one of `prompt`, `question`, `permission`, `denied`, `error`, `compaction`, `agent`, `commit`, `edit`, `mail`, `turn-end` and `interrupt` (a shell's are all `prompt`). `rows` is the conversation's rows at the pane's width and `view_top` the row at the top of the view (Claude only). `card` is the card on a mark while it's open, `{rect, lines}`, each mark's kind and when, then its words. `seeking` is a click's scroll through Claude Code's view under way, and `note` why the last didn't get there, or that a write to this pane didn't (lbterm/SCROLL-STRIP.md, "Notes"), while that's said, and `note_head` its head: `Not there` for a click, `Not sent` for a write, `May not have arrived` for a write whose connection ended before the server answered. A write's note is only in the state of the window whose active tab holds its pane. `looks` is how many times the window has read a Claude pane's screen for where its view is, since it opened: once when the conversation, its layout or the conversation's rows on screen (or the input box's rules) changed, never for Claude Code's spinner or status alone (`perf/conversation-cost.ps1` counts them).


### `context_bar`

`{rect, track, session, waiting, in_use, window, estimate, scale, filled, sections, layouts, widened, pane, lit, ride}` (lbterm/CONTEXT.md, "The context bar"). `rect` is the bar at rest, its ring included, and `track` inside it, where the sections go: the open tab's conversation's whole context window, top to bottom. `session` is that conversation's, `null` while the tab has none (or the Conductor's view is open). `waiting` says, in the pane's words, what the bar waits for while it draws only its track: `Reading the conversation…` before the reader has the transcript, `Waiting for Claude's first reply`, `Waiting for the context window's size` or `This transcript doesn't record Claude Code's first request`; `null` once it has its sections. `in_use` and `window` are tokens, `estimate` true after a compaction until the next reply, `scale` the first request's real size over its characters over four. `filled` is how far down the track the filled part ends, in pixels: `in_use` of `window` of the track's height, rounded. `sections` is every section in the bar's order, free last, `[{kind, path, tokens, rect, colour, button, removed, note}]`: `kind` one of `user`, `project`, `import`, `local`, `managed`, `memory_index`, `memory`, `pulled_in`, `elsewhere` (another project's pulled-in CLAUDE.md), `attached` (a file Claude Code attached again after a compaction's summary, beside its own section once the next turn loads it again; since the second round's stage 3), `system`, `tools`, `skills`, `conversation`, `free`; `path` a file's as Claude Code wrote it; `rect` where its run is drawn, its gap included, the same for neighbours of one colour drawn as one, `null` for free and a section with no tokens; `colour` `#RRGGBB`; `button` `remove` or `null`; `removed` true while its line is out of the file that loads it (its Remove pressed and not undone; its run hatched); `note` its row's second line: a pulled-in file's `Leaves at the next compaction`, or where its removal stands (`Off MEMORY.md: a compaction can attach it again`, `Back in MEMORY.md`, a refusal's why). `layouts` counts the times the sections were laid out, which is only when the context, the window's size or the track changed: a script can tell a frame that laid them out from one that didn't. `widened` is the bar widened to the closed rail's width while it's out or sliding back in (the drawer left of the bar until the second round's 4b), `{rect, labels, thin}`, each label `{section, text, rect}` (`section` its index in `sections`, `text` its tokens as written, `1.5k`), a number on its own section (its rect inside `rect`) or, for a section too thin for its own, in the pane beside the bar, `thin`, that pane's rect, as wide as `rect` and left of it, absent when every number is on its section (the smallest of those left out when they don't all fit), the labels in the sections' order; `null` at rest. `pane` is the pane while it's out or sliding back, `{rect, heading, rows, above, left_out, pinned, compact}` (`pinned`: the totals, every section without a path, are the last rows, pinned at the foot under a rule while only the files scroll, which they are when the rows don't all fit; `above` and `left_out` then count files): `heading` what's in use of the window (`40k of 200k in use · 20%`, `≈` first while it's an estimate) or what it waits for; `rows` one per section in the bar's order, `[{section, kind, path, tokens, button, note, rect, lit}]`, and one for each removal whose section has gone from the bar (a compaction took it), after the files, its `section` past the last of `sections`, `tokens` empty, `button` `Undo` and `note` `Off MEMORY.md: Undo puts it back` (or `Import off in CLAUDE.md: Undo puts its @ back`, then `Back in …` or `Import back on in …` once undone), `kind` the pane's word (`user`, `memory index`, `pulled in`, `system prompt` ...), `path` as shown (home as `~`, cut from its start with `…`), `tokens` as shown (`≈1.5k`; free's exact while what's in use is), `button` `Remove`, `Undo` once its line is out, or `null`, `note` its second line, `lit` for the section under the pointer; `above` how many rows the wheel has scrolled out above the first shown (0 at the top, and each time the pane opens), `left_out` how many below didn't fit; `compact` Compact now at its foot once the bar has its sections, `{rect, enabled, why}`: `enabled` while a press would type `/compact` into the tab, `why` the words under it when it wouldn't (`Claude is working`, `Something is typed in the box`, `A dialog has the box's place`, `No Claude Code in this tab`) or what a press did: `Sent /compact: …` from the press (Compact now types at once and writes nothing else, CONTEXT.md, "Compact now writes no rule") until the reader has read that session's compaction (or a reply with none: a cancelled or failed one, then a turn), in every window showing the pane, and `Not sent: …` in the window that pressed until the pane closes. `lit` is the section under the pointer on the bar, the drawer or the pane while either is out, else `null`. `ride` is how far left of its own place the scroll strip is drawn, in pixels: to the drawer's left edge while only it's out, to the pane's while that's out, 0 at rest (`scroll_strip.strip` is where it's drawn).
### `menu`

`{kind, tab, rect, items, focus, renaming, waiting}`, for whichever is open: `kind` is `tab` (the tab menu, with `tab` its tab and `renaming` the name being typed), `plus` (the new-tab pad's + menu, top to bottom: `PowerShell 7 here` and each other launch menu entry, `Shell in a project…`, up to five `Reopen …` or `Nothing closed yet`, `New window`), `prompts` (the list of prompts, newest first, or the one row saying why there are none), `conductor` (the Conductor's, with `waiting` the item waiting for its answer), `inbox` (the InMail button's list, newest first, each `project: subject`, `checked` for one a conversation has taken, or the one row saying none wait) or `agenda` (the agenda's list, top to bottom: a section's heading `Urgent (1)`, a row `project: title`, a folded group `title (2)`, an opened row's `detail: …` and its buttons an item each, `button: Tick`, and the agents' line `Agents' to-dos (1)`; `checked` for an opened group or row, the agents' line while it shows them, and a button asking first). The kind `context`, the context pill's list of the files in the conversation's context, went on 2026-11-08 with the list (Lachlan's 5a): the context bar's pane lists them, `context_bar.pane.rows` (lbterm/CONTEXT.md). Each item is `{label, checked, rect, key, detail}`; `rect` is `null` for a prompt scrolled out of view, and `key` is the one letter the row shows dim at its right and is picked by (the tab menu's `R`, `L`, `N`, `S`, `H`, `W`, `M`, `F`, `O`, `X`, `menu::key`; its items are the rows `menu::items` gives the tab's facts as the menu opened, so a Claude tab has `Copy link` and a tab whose folder is in one of `lbterm_projects` has `New session in <Project>`, a Claude tab while `lbterm_conductor`'s button.json has a `hidden` list (and it isn't the Conductor's own tab) `Hide from Conductor`, or `Show to Conductor` when every session of the tab is on the list, and while a pick for the tab still waits the same row greyed, `Hiding from Conductor…` or `Showing to Conductor…` with `inert: true` and no `key` (PITFALLS 244; Lachlan's 11b of 2026-11-09: every row the card shows is listed, `menu::shown`, an inert one included, and `inert` is left out when false) (lbterm/NEW-TAB.md, section 8), a Claude tab with `lbterm_links` and another Claude tab on the rail `Wait on…`, and any tab while one is finished `Close finished tabs (N)`; on Wait on…'s page the items are `‹ Back` and the other Claude tabs by label, with no keys; the + menu's, `newtab::plus_key`), left out where a row has none. `detail` is a row's second line as drawn, left out where it has none: Close finished tabs' list of the tabs it closes by label, cut to the card's width. `focus` is the item under the pointer or picked with the arrow keys.

### `conductor_view`

The Conductor's view (lbterm/TAB-PANEL.md, "The Conductor's view"), as its last paint laid it out:

| Field | |
|---|---|
| `connected` | the seat answers on its pipe |
| `state` | the Conductor's: `off`, `starting`, `on`, `stopping`, `error`; `unknown` without the seat |
| `mode`, `listening`, `busy`, `queued`, `error` | as the seat's `seat.view` says |
| `rect` | the view's card |
| `conversation` | the conversation's room |
| `scroll`, `scroll_max` | how far it's scrolled up from the newest, and how far it can be, in pixels |
| `items` | `[{id, who, text, queued, streaming, tool, rect}]`: the conversation's items in the order they show; `who` is `you`, `conductor`, `tool`, `hub`, `peer` or `notice`, `text` the first 200 characters, `tool` a tool call's status, `rect` what of it shows (`null` scrolled away) |
| `streaming` | the item whose reply is being written |
| `boxes` | `[{name, rect, text, caret, focused}]`: every box, `main` at the bottom first, then `answer:<waiting>`, a question in text's; `caret` in characters |
| `focus` | the box typing goes to |
| `cards` | `[{kind, id, title, rect}]`: the side's cards in order; `kind` is `ask`, `waiting`, `session`, `hidden` (a session hidden from the Conductor, in the Sessions card's Hidden group while it's open: lbterm/NEW-TAB.md, section 8), `events`, `suggestion` or `notes` |
| `buttons` | `[{name, label, rect, on, enabled, pending}]`: every button, `rect` what of it shows (`null` scrolled away), `on` lit (the mode in force, an option picked, a rating given), `pending` pressed and waiting on the seat's reply. The names: `send`, `stop`, `newest`, `control:<action>` (`control:on`, `control:mode most`, `control:listen off`, ...), `allow:<ask>`, `deny:<ask>`, `pick:<waiting>:<question>:<option>` (from 0), `other:<waiting>:<question>`, `chip:<waiting>:<code>`, `answer:<waiting>`, `session:<sessionId>`, `show:<sessionId>` (Show on a session in the Hidden group, which fires `lbterm-conductor-hide` with `hide` false), `section:<name>` (a section's or a sessions group's heading, `on` while it's open: `section:sessions:hidden` starts folded), `rsi:<suggestion>:<send, dismiss, useful or not>` |
| `notice` | under the box for a moment: what the seat answered a command with |
| `selection` | the conversation's words selected |
| `hovered` | the button under the pointer, by name |
| `clock` | when what it says about time next changes, Unix milliseconds: an ask's or a waiting question's age ("just now" to "1 min ago" at 45 s, and on), or the next midnight, when the days' names change. The view paints then, and otherwise only when something it shows changes |

### `pointer.over`

| `over` | with |
|---|---|
| `menu.item`, `menu` | `item` |
| `rail.conductor`, `rail.row`, `rail` | `row` |
| `view.button`, `view.box` | `button`: the button's or the box's name |
| `view.conversation`, `view` | (the Conductor's view: its conversation, elsewhere on it) |
| `strip.card` | |
| `objectives.detail`, `objectives.list`, `objectives.strip` | `row`: the rail row the card's row or the detail is for |
| `projects.card`, `projects.pad` | `row`: the card's row, by its place |
| `cost.pane` | the cost pane (the pill itself is `strip.context`) |
| `context.button`, `context.compact`, `context.pane`, `context.bar` | (the context bar's pane: a row's Remove or Undo button, Compact now, elsewhere on it; the bar, the gutter left of it, or its drawer while it's out, with `context_bar.lit` the section under it) |
| `scroll.card`, `scroll.mark`, `scroll.thumb`, `scroll.strip` | (the scroll strip: its card, a mark, the thumb, elsewhere on it) |
| `rail.tasks` | the card of what a tab is running |
| `picker` | `row`: the new-tab picker's row under it, by its place in the list (lbterm/NEW-TAB.md) |
| `rail.new_tab` | the new-tab pad's zone |
| `strip.button`, `strip.pill`, `strip.words`, `strip.prompt`, `strip.arrow`, `strip.inbox`, `strip.agenda`, `strip.context` | `button` (`close`, `maximize`, `minimize`, `restart`, `words`, `prompt`, `arrow`, `inbox`, `agenda`, `context`). The projects pad on the strip is `projects.pad`, named before the strip's items |
| `tab`, `tab.close` | `item`: the tab's index, with the tab bar showing tabs |
| `strip` | elsewhere on the strip |
| `scrollbar`, `split` | |
| `pane` | `pane` |
| `window` | none of those (the gutters) |

### Animation names

What's moving, and the `lbterm_motion` token it plays on (`lbterm/DESIGN.md`):

| Name | What moves | Token |
|---|---|---|
| `pane.layout` | panes sliding and resizing to a new layout; a split's new pane riding in | `pane_layout` |
| `pane.enter`, `pane.exit` | a pane fading in, a closed one fading out | `pane_enter`, `pane_exit` |
| `pane.dissolve` | the panes of a tab switched from or closed fading out over the next tab's | `tab_switch` |
| `sides.dissolve` | the strips down the right as that tab showed them, fading out with its panes over the next tab's | `tab_switch` |
| `pane.dim` | a pane going to 85% as it loses the keyboard, and back | `dim` |
| `outline.move` | the active outline gliding to the pane that takes the keyboard, or with its pane | `focus_move` |
| `outline.arrive` | the outline's pulse as the keyboard arrives from outside | `arrive_in`, `arrive_out` |
| `outline.dim` | the outline going out as the window loses the keyboard, and back | `dim` |
| `outline.held` | the outline going to full in the held modifiers' colour, and back to white at rest once they're let go (`held`) | `arrive_in`, `arrive_out` |
| `outline.dissolve` | what of a tab's active outline the next tab's doesn't draw too, fading out with that tab's panes as it's switched from or closed | `tab_switch` |
| `outline.return` | the active outline of a tab switched back to while that fade ran, coming back up from nothing as its panes come back, what showed on its lines going as it comes | `tab_switch`, or with other tabs still in flight as those go (sampled) |
| `rail.edge` | the tab panel's edge opening or closing | `panel_open`, `panel_close` |
| `questions.pane` | the questions bar sliding out from under the window's right edge as a right hold starts, or back under it as the hold ends, turned round from where it is by a release or a press mid-slide (QUESTIONS.md, "The questions bar"); none with motion off | `panel_open`, `panel_close` |
| `questions.push` | what the questions bar pushes left as it comes out (the panes, their outline, `over` under the tab bar, the strips, the objectives' and context bar's visuals, the tab panel), as far as the bar comes out, and back as it goes, from the bar's commit on its curve (QUESTIONS.md, "The questions bar"); none with motion off or no rail | `panel_open`, `panel_close` |
| `rail.light` | the ring's light and glow brightening or dimming | `panel_glow`, `hover` |
| `rail.words` | the open panel's words fading in or out | `pane_enter`, `pane_exit` |
| `rail.sweep` | the light running round the ring as it opens | `panel_sweep` |
| `rail.wave` | the wave washing out ahead of the edge as it opens (`lbterm_tab_panel.wave`), breaking, its spray scattering; fading where it is when turned round | `panel_open`, `panel_wash`, `copy_in`, `panel_splash`, `pane_exit` |
| `rail.row.grow`, `rail.row.enter` | a new tab's slot opening, and its row fading in | `pane_layout`, `pane_enter` |
| `rail.row.exit` | a closed tab's row fading where it was | `pane_exit` |
| `rail.row.slide` | rows sliding to new slots; a dragged row let go | `pane_layout` |
| `rail.row.swap`, `rail.row.hover` | a row's old look dissolving into its new one; a hover's | `swap`, `hover` |
| `rail.marker` | the open tab's ring and bar gliding to the new row, or moving with its own row; when the open tab closes as the row taking its slot slides up into it, riding up with that row (`lbterm_motion::rail`, Lachlan's 3b), ending as that row's `rail.row.slide` does | `focus_move` (a glide), `pane_layout` (with a row) |
| `rail.progress` | a progress bar gliding to its new length | `pane_layout` |
| `strip.hover` | a window button's or the strip's look fading under the pointer | `button_hover` |
| `strip.card` | the strip's card fading in or out | `pane_enter`, `pane_exit` |
| `objectives.list`, `objectives.detail`, `objectives.projects` | the objectives panel's cards and the projects card fading in or out (the projects card is still the objectives' third card in the compositor, with the objectives strip on or off) | `pane_enter`, `pane_exit` |
| `context.cost` | the cost pane fading in or out (the objectives' fifth card in the compositor) | `pane_enter`, `pane_exit` |
| `context.widen`, `context.thin`, `context.pane` | the context bar widening or narrowing (the widened bar's content sliding out from where the bar is, or back: since feat/context-bar's round 3 its narrowing is started in the same commit as the thin numbers' pane's slide back and delayed by what's left of it, so `context.widen` is listed from that commit, its start `now` plus the delay as the compositor's `drive` records any delayed slide, its `progress` 0 until the delay is up and its `left_ms` counting the delay too), the pane of its thin sections' numbers sliding out from under the widened bar's moving left edge in the bar's own slide, started in the same commit with no delay (since round 4's stage 6, Lachlan's E9 of 2026-11-10: its clip rides the bar's one animation, `Slid::follow`, which isn't listed apart; until then its start was delayed by what was left of the widening), or back, and its pane sliding out from under it or back (lbterm/CONTEXT.md) | `panel_open`, `panel_close` |
| `scroll.ride` | the scroll strip gliding to the context bar's drawer's or pane's left edge, or home | `panel_open` going, `panel_close` coming back |
| `scroll.card` | the scroll strip's card fading in or out | `pane_enter`, `pane_exit` |
| `rail.tasks` | the card of what a tab is running fading in or out | `pane_enter`, `pane_exit` |
| `newtab.picker` | the new-tab picker fading in or out (lbterm/NEW-TAB.md) | `pane_enter`, `pane_exit` |
| `menu.tab`, `menu.prompt`, `menu.conductor`, `menu.list` | a menu's card fading in or out: the tab menu, the prompt list, the Conductor's menu, and the InMail list, the agenda or the + menu (their slot; render/menu_fade.rs) | `pane_enter`, `pane_exit` |
| `menu.tab.glide`, `menu.prompt.glide`, `menu.conductor.glide`, `menu.list.glide` | its highlight gliding to the row lit now | `hover` |
| `menu.tab.light`, `menu.prompt.light`, `menu.conductor.light`, `menu.list.light` | its highlight's light coming up on a row or going out as none is lit | `hover` |
| `menu.tab.hairline`, `menu.prompt.hairline`, `menu.conductor.hairline`, `menu.list.hairline` | its edge flaring once for a key no row takes: the hairline all the way to white and the light round the card white under its red, one animation on both, and back (the compositor's `menu_lamp`; none with motion off). Named for the hairline, which until 2026-11-08 was all that brightened, 35% of the way to white on `arrive_out`'s fall | `arrive_in`, `panel_glow` |
| `menu.tab.flash`, `menu.prompt.flash`, `menu.conductor.flash`, `menu.list.flash` | a row picked in its menu flashing white as the pick goes, up to the press's white and back (Lachlan's 9b; the compositor's `Light::Row`, in `menu_lights` over the card, not faded with it; none with motion off). The picker's picked row is `light.glint` | `copy_in`, `copy_out` |
| `menu.tab.swap` (and `menu.prompt.swap`, `menu.conductor.swap`, `menu.list.swap`, which nothing plays yet) | the card's page turning (the tab menu's Wait on… and ‹ Back): the old page's last frame fading out over the new (the compositor's `menu_turn`; none with motion off) | `swap` |
| `rail.plus.hover` | the new-tab pad's look under the pointer, or while its picker or menu is open, fading in or out | `button_hover` |
| `strip.swap` | the strip's old words dissolving into the new | `swap` |
| `light.press` | a right-click's ripple | `press` |
| `light.copy` | a copy's or an opened link's flash | `copy_in`, `copy_out` |
| `light.glint` | a pad's glint (typed into from `lbterm cli`, renamed, Copy link, the Conductor's word on a hide or a show), and the picker's picked row and its box's brightening | `copy_in`, `copy_out` |
| `light.send` | the light a Hide from Conductor or Show to Conductor sends up the rail from the tab's pad to the Conductor's (Lachlan's 6a; the compositor's `Light::Send`, lbterm/NEW-TAB.md, section 8), starting in the frame that lets the rail go when a menu's card held it; two entries while it runs, its climb (`send`, 600 ms) and its light (up on `copy_in`, held, out on `copy_out` once it's there: 860 ms in all); none with motion off | `send`, `copy_in`, `copy_out` |
| `readout` | the size readout fading out | `pane_exit` |
| `hearts` | the hearts rising: DWM's with the compositor and motion on (each heart's rise, sway, growth and fade, `compositor.rs`'s `prepare_hearts`), else drawn by the GUI frame by frame | |
| `wezterm` | upstream's own next frame (a blinking cursor or text, an animated image), which it asks for only while the window is focused | |

Several numbers moving under one name (a pane's x, y, width and height) are one animation, from the first one's start to the last one's end. An animation turned round before it ended (the pointer leaving the rail while it opens) is still counted until the old one would have ended, so `animating` can stay true for at most what that one had left. It never goes false early. One stopped where it is, its numbers set at once in place of what DWM played, is cut there (`InFlight::cut`, since 2026-11-08): the tab panel's wave stopped as a menu's card comes, `rail.wave` then counting only its fade (until then it stayed to its old end, PITFALLS 187).

### Wait names

| Name | Until |
|---|---|
| `rail.open`, `rail.close` | the tab panel opens after resting on the rail, or closes after the pointer left |
| `strip.card.open`, `strip.card.close` | the strip's card opens after the pointer rested on the words, or closes after it left |
| `rail.tasks.open`, `rail.tasks.close` | the card of what a tab is running opens after the pointer rested on its row, or closes after it left the row and the card |
| `strip.prompt.open`, `strip.prompt.close` | the same for a prompt cut short |
| `objectives.open`, `objectives.close` | the objectives panel's list opens after resting on its strip, or its cards close after the pointer left them |
| `context.widen`, `context.open`, `context.close` | the context bar widens after resting on it, its pane opens after resting on a section, or they close after the pointer left the bar, the drawer and the pane |
| `projects.open`, `projects.close` | the projects card opens after resting on its pad (`lbterm_projects.open_delay_ms`), or closes after the pointer left it and the card (`close_delay_ms`) |
| `cost.close` | the cost pane shuts after the pointer left the pill and the pane (`COST_CLOSE_MS`, 250); it opens only on a press, so it has no wait to open |
| `scroll.open`, `scroll.close` | the scroll strip's card opens after resting on a mark, or closes after the pointer left the marks and the card |
| `strip.notice` | a notice leaves the caption's place |
| `strip.note` | a jump's note leaves the prompt's place |
| `strip.jump` | a jump back to a prompt lands or gives up; how long isn't known ahead, so `left_ms` is 0 |
| `readout` | the size readout starts to fade |
| `landed` | a landed row's mark goes |
| `conductor.answer` | a Conductor item stops waiting for its answer (45 s at most) |
| `view.notice` | the Conductor's view's notice under its box goes (6 s, a refusal 12 s) |

### Loop names

`rail.spinner` (a pad whose program waits on its own tasks, not a parked row's, whose arc is drawn still: `Row::arc_turns`), `rail.starting` (the Starting row's arc while a new tab is on its way), `conductor.outline` (the Conductor's turning hues), `conductor.light` (the light going round a tab it acts on), `menu.spinner` (a Conductor item waiting for its answer), `link.lamp` (the lights round a tab link drag's matched blocks), `connection.light` (the light running along a suggested tab connection's line: lbterm/LINKS.md, "Connections"). With motion off, or while the window can't be seen, they hold still and aren't listed.

## A sample

From `state.ps1` (`target\perf\state\rail-open.json`), the rail open and settled with the pointer on the first row, after a split: one of the two tabs, rows and panes kept, and two of the strip's items.

```json
{
  "version": 1, "pid": 51012, "build": "20261021-233839-13b335c4", "frame": 14, "time_ms": 1792588042297,
  "windows": [{
    "id": 0, "hwnd": 16064854, "frame": 14, "focused": false, "keyboard": true,
    "animating": false, "animations": [], "waits": [], "settled": true, "loops": [],
    "size": { "width": 998, "height": 698, "dpi": 96 },
    "pointer": { "x": 38, "y": 96, "over": "rail.row", "row": 0, "item": null, "pane": null, "button": null },
    "active_tab": 0, "active_pane": 2,
    "tabs": [{ "index": 0, "id": 0, "title": "", "active": true, "panes": [0, 2], "zoomed": false, "colour": null }],
    "panes": [{
      "id": 2, "server_id": null, "rect": { "x": 527, "y": 57, "w": 467, "h": 637 }, "cols": 50, "rows": 31,
      "active": true, "zoomed": false, "title": "pwsh.exe", "alt_screen": false, "mouse_reporting": false,
      "cursor": { "x": 11, "y": 2 }, "drawn_cursor": { "x": 11, "y": 2 }, "scroll_top": null, "session": null, "progress": null
    }],
    "scrollbar": { "enabled": true, "shown": true, "marks": 0 },
    "rail": {
      "open": true, "phase": "open", "light": "full", "rect": { "x": 4, "y": 57, "w": 290, "h": 637 },
      "rows": [{
        "row": 0, "tab": 0, "tab_index": 0, "visible": true,
        "rect": { "x": 4, "y": 65, "w": 290, "h": 75 }, "pad": { "x": 27, "y": 86, "w": 22, "h": 20 },
        "label": "pwsh", "title": "pwsh", "project": "Projects", "state": "none", "status": "", "message": "",
        "progress": null, "estimated": false, "left": "", "agent_bars": [], "agents": 0, "waiting": 0, "unread": false, "age": "", "cost": "", "active": true, "hovered": true,
        "colour": null, "conductor": null
      }],
      "hovered": 0, "open_row": 0, "conductor": null, "dragging": null, "drop_hint": false
    },
    "strip": {
      "title": "pwsh", "summary": null, "caption": null, "notice": null, "prompt": null, "note": null, "badge": null,
      "card": null, "restart": false, "inbox": null, "agenda": null, "context": null, "mic": null, "jumping": false, "lit": [],
      "items": [{ "name": "close", "rect": { "x": 952, "y": 0, "w": 46, "h": 30 } },
                { "name": "words", "rect": { "x": 80, "y": 0, "w": 28, "h": 28 } }]
    },
    "menu": null, "overlay": null, "readout": null, "landed": null, "questions": null, "hearts": 0, "tasks": null,
    "thumbnails": [], "drafts": [], "ticked": [], "next": null
  }]
}
```

A build since 2026-10-30 has `pace` after `windows`, and `work` after it (above).

And what `state.ps1` saw of a split at slowdown 3 (`seen.txt`), a snapshot per frame: the frame that starts it, frames while the shells draw, and the one asked for when DWM's last animation ended.

```
   Ms Frame Animating Animations
 8997     4      True {pane.enter, pane.dim, pane.layout, outline.move}
 9049     5      True {pane.enter, pane.dim, pane.layout, outline.move}
 9211     6      True {pane.enter, pane.dim, pane.layout, outline.move}
 9578     7      True {pane.enter, pane.dim, pane.layout, outline.move}
10587     8     False {}
```

## Not covered

- Upstream overlays and modals are named, not described: the launcher's or the command palette's items aren't in the file.
- The rail's rows and the strip's words are what the last paint drew; the tab panel's project chips come from a lookup that runs only while the panel is open.
- Question colours say which pane shows them, not what they colour.
