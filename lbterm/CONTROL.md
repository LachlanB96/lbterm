# The control pipe

Lachlan's pick (2026-10-25): Earshot, his always-on voice listener, answers Claude sessions and moves between tabs by voice, and the terminal side of that belongs in LBTerm. Earshot keeps the ears (the mic, speech to text, its rules, and turning "one A, two yes" into the way he types answers); LBTerm knows which pane he's looking at, runs its own handlers for what he asks, and draws what's going on. The control pipe is how they talk, and any other program on the PC can use it too: APCommand, Relay, the Conductor.

Before it, Earshot drove LBTerm the way the Conductor does, through `lbterm cli` against the mux server: it guessed the pane he was in from the least idle client, and copied `lbterm.lua`'s tab hop into Python, parked tabs and all, so the two could drift apart.

## The pipe

Every GUI listens on `\\.\pipe\lbterm-control-<its window class>` (`config::lbterm::control_pipe`): the daily LBTerm's is `\\.\pipe\lbterm-control-com.lachlanb.lbterm`, a try window's `...-com.lachlanb.lbterm.try`, and each test window's its own, so nothing asked of one is done by another. Clients from other machines are refused.

One JSON object a line, each way. A request is `{"id": <anything>, "op": "<op>", ...}`; its reply carries the same `id` and `"ok": true` with what was asked for, or `"ok": false` with `error`, saying why. A connection carries as many requests as it likes, one at a time, each answered before the next is read. A line that isn't a request is refused and the connection stays open.

A request runs in the window he was last in (it had the keyboard, or was clicked in), else the first, on the GUI thread; the reply comes once it has, or after 5 s saying LBTerm didn't answer. Nothing it does takes the keyboard or brings a window forward.

## Requests

`focus`: where he is.

```json
{"id": 1, "op": "focus"}
{"id": 1, "ok": true, "window": 0, "pane": 14, "session": "ff2a9a76-…", "title": "✳ Answering by voice", "foreground": true}
```

- `pane`: the window's open tab's active pane, by the id `lbterm cli` takes (the mux server's, on the mux domain). Never an overlay over it.
- `session`: the Claude Code session in that pane (claudehook's `claude_session` user var), null when there's none. Its open question is `%LOCALAPPDATA%\Megaproject\live-sessions\<session>.question`.
- `foreground`: an LBTerm window has the keyboard now.

`emit`, with `event`: runs a Lua event in that window with its active pane, as a key bound to `EmitEvent` would.

```json
{"id": 2, "op": "emit", "event": "lbterm-tab-forward"}
{"id": 2, "ok": true, "event": "lbterm-tab-forward", "window": 0, "pane": 14}
```

The reply comes once the event is handed to the window, not when its handler ends. Lachlan's `lbterm.lua` has `lbterm-tab-back` and `lbterm-tab-forward` (the side buttons' hops, parked tabs skipped), `lbterm-park-toggle` (Ctrl+Alt+D) and `lbterm-summary-detail` (Alt+S). Since Lachlan's 8a and 9a (NEW-TAB.md, "Landing 4") parking and the hops are LBTerm's own, the `LbtermPark` and `LbtermTab` actions, reading the tab's flag rather than files: the events keep their names for Earshot, and Terminal's lbterm.lua (its feat/rail-landing-4) has each perform its action, `lbterm-tab-back` and `lbterm-tab-forward` `LbtermTab 'Back'` and `'Forward'`, the new `lbterm-tab-waiting` and `lbterm-tab-last` `'Waiting'` and `'Last'` (what Shift with the side buttons does), and `lbterm-park-toggle` `LbtermPark 'Toggle'`, each only in a build that has the action; perf/park.ps1 and perf/hops.ps1 fire an event of their own that performs `LbtermPark 'Toggle'` through this pipe, as `lbterm-park-toggle` does. WezTerm's own events (`update-status`, `format-tab-title`, `open-uri`, `user-var-changed` and the rest it calls with arguments of its own, `BUILT_IN_EVENTS` in `lbterm-extras/src/control.rs`) are refused. Any other name is the config's to answer; one it has no handler for does nothing.

## Drafts

Lachlan's pick (2026-10-25, 1a): an answer he says to a Claude session isn't typed into its prompt as he says it. LBTerm holds it as a draft over the pane it's for and types it whole when he says "send". Earshot hears it and writes it the way he types answers (`1a`, `2. yes`, a blank line, `also, ...`); LBTerm keeps it, shows it and types it (`lbterm-extras/src/drafts.rs`).

- **The card.** Just above Claude Code's input box (or what has its place: a picker, a permission prompt), a card shows the draft's lines as they'll be typed, each answer with a bar in its question's colour, under a line saying what to say to it. While a draft is open the pane's question colours show too (lbterm/QUESTIONS.md), each question it answers has a tick at the right of its first row, and the next it doesn't a red ring. A line answers the question whose number it starts with, the way he types them (`1a`, `4ab`, `2. yes`, `3) no`).
- **What it costs.** The card's lines are made again only when the draft or the session's questions change, where it goes only when the pane's screen does (read again only when the pane has changed), and a frame that changes neither draws it, its shapes and its words as they were (lbterm/PERF.md, "What a line costs").
- **Where it belongs.** A draft is one pane's (by the mux server's id), so switching tabs while it's open can't type it anywhere else, and it's drawn wherever that pane is shown. It doesn't outlive the Claude session it began with: once the pane's `claude_session` is another, it's gone.
- **Send.** Text only into Claude Code's prompt: its two rules near the bottom with its `❯` starting the first row between them, and none of a picker's or a permission prompt's words under the upper rule. A named conversation's upper rule carries its name near the right end (`──…── LBTerm InMail ─`, Claude Code 2.1.314 on), and still counts as a rule (`questions::is_rule_text`, which the scroll strip and the question colours use too). Each new line goes as Shift+Enter makes it (ESC CR), the text in one write, then Enter in a write of its own 150 ms later: Claude Code takes a long write as a paste, where an Enter is a new line and not a send, so a long draft written whole with its Enter sat in the box with "Removed 1 invisible character" (Earshot's InMail, 2026-10-25). Measured 2026-10-27 in a Haiku session on 2.1.311: that draft written that way showed the same, and with Enter written on its own, even under 45 ms after, it and a 12-line draft of 1,061 characters were sent clean. A picker's picks only into the picker they're for, at its first question (its first question's words on screen), typed as their keys 150 ms apart, and only once every question has one. A permission prompt never gets anything. When it can't, nothing is typed, the draft stays, and the strip says why ("Voice draft: …"). The screen it's checked against is the pane's as its program has it: a draft sent or typed into a pane named by id whose tab is out of view, its output held at a mux server, has the pane's push asked and applied first, waited for up to 50 ms (`HELD_READ_WAIT`) while the GUI thread goes on with its other work (Lachlan's 6b of 2026-11-08; MUX.md, "What reads a held pane's lines"). Until then it was checked against the window's copy as last pushed, so a session that had moved on from a permission prompt to its input box out of view refused the draft (perf/held-reads.ps1's step 3).
- **A send that fails part way** (Lachlan's 3a of 2026-11-08: a lost dictation is the costliest thing to retype). The card goes as the keys start, and each key's answer is waited for before the next (`ClientPane::lbterm_write_answered` on a mux pane; a write's own error on the window's). A key that doesn't get there stops the rest and puts the draft back over the pane, unless he has begun another there since (`drafts::put_back`), and the pane's scroll strip says why, headed "Not sent", or "May not have arrived" when the connection ended before the server answered, its text saying the draft is held again and, for the second, to look at the prompt before sending it: the keys before the failed one got there, and the failed one may have. Keys wait through a reconnect as every other input does (MUX.md, "A write to a pane"), so what puts a draft back is a server that refuses the write (the pane's program gone) or a connection that ends while a key is unanswered. A send's keys go one after another's answer, a round trip each (under a millisecond to a few here), inside the 150 ms between a draft's keys.
- **Edit.** The text typed into the prompt without Enter, for him to finish by hand; the card goes. One that fails part way comes back as a send does.
- **Scratch.** The newest piece taken back (a picker's picks all at once).

```json
{"id": 3, "op": "draft.add", "text": "1a\n2. yes"}
{"id": 3, "ok": true, "draft": {"pane": 14, "session": "ff2a9a76-…", "kind": "text", "text": "1a\n2. yes", "pieces": 1, "answered": [1, 2], "missing": []}}
{"id": 4, "op": "draft.picker", "label": "1b 2a", "keys": ["2", "1", "1"], "questions": ["Which colour should the pad take?", "And its ring?"]}
{"id": 5, "op": "draft.send"}
{"id": 5, "ok": true, "draft": null, "sent": "1b 2a"}
```

Every `draft.*` request takes `pane` (the mux server's id; the open pane of the window he was last in when it's left out), and its reply carries the draft as it is after it, or null.

| op | takes | does |
|---|---|---|
| `draft.get` | | nothing: what's there |
| `draft.add` | `text` (`\n` for a new line), `fresh` | adds a piece; `fresh` starts again. A picker's picks give way to it. 8,000 characters at most |
| `draft.picker` | `label`, `keys`, `questions`, `missing` | picks for a picker, in place of what's there. `keys` are what's typed, in order (a digit a question, `1` on the review tab, as Terminal's `shared\claude\README.md` "The picker's keys" says); `questions` the picker's, the first by which it's known on screen; `missing` the questions without a pick yet |
| `draft.scratch` | | the newest piece taken back: `removed` |
| `draft.clear` | | the draft let go: `removed` |
| `draft.send` | | typed and sent: `sent` |
| `draft.edit` | | typed into the prompt, not sent: `typed` |

The `LbtermDraft` action does the same from a key, on the pane it's run in: `LbtermDraft 'Send'`, `'Edit'`, `'Clear'`. A config shared with older builds binds it only where `wezterm.has_action('LbtermDraft')`, as for `LbtermQuestions`.

## The mic mark

Lachlan's yes (2026-10-25): while Earshot listens for LBTerm, a mic on the window strip says so, left of the window's buttons (and the restart pill), in every window: dim while it listens, in the tab rail's active red while it hears him speak, struck through while it's paused (a phone call, Q&A, muted). Earshot tells LBTerm its state as it changes, and again every so often while it listens: each state holds for its `ttl` (30 s unless given, 600 at most) and then goes, so a listener that stopped without saying so leaves no mic behind. `heard`, what it heard and did with it ("typed 1a", "nothing typed: the tab isn't asking anything"), shows as the strip's notice for a moment, after the mic, in the window he was last in (`lbterm-extras/src/mic.rs`, `wezterm-gui/src/mic.rs`).

```json
{"id": 6, "op": "mic", "state": "hearing", "ttl": 20}
{"id": 6, "ok": true, "mic": "hearing"}
{"id": 7, "op": "mic", "state": "listening", "heard": "typed 1a"}
```

`state` is `off`, `listening`, `hearing` or `muted`; the reply says what the mark shows. The glyphs are the Symbols Nerd Font's mic (U+F130) and struck mic (U+F131), which every LBTerm falls back to.

## Metrics

`metrics`: what the GUI has done and what it cost (lbterm/PERF.md has every field and how to read it). The counters are always on, in every LBTerm, the daily one included.

```json
{"id": 8, "op": "metrics"}
{"id": 8, "ok": true, "pid": 26768, "build": "20261029-…", "reset": false, "span_ms": 4127.2,
 "totals": {"frames": 0, "unasked": 0, "requests": 0, "tasks": 3, "task_ms": 0.123, "messages": 3, "message_ms": 0.002, "paints": 0, "paint_ms": 0.0, "timers_fired": 0, "wakes": 6, "pdus_sent": 0, "pdus_received": 0},
 "process": {"cpu_ms": 140.6, "cpu_percent": 3.4, "gui_thread_ms": 0.0, "private_bytes": 115867648, "handles": 376, "threads": 22, …},
 "threads": [{"tid": 4412, "name": "…", "cpu_ms": 125.0, "cpu_percent": 3.0}, …],
 "windows": [], "requests": [], "tasks": [{"site": "wezterm-mux-server-impl/src/local.rs:24", "count": 3, "total_ms": 0.123, "max_ms": 0.056}],
 "messages": […], "paints": [], "timers": [], "mux": [], "gui": {"wakes": 6, "waited_ms": 4127.0, "busy_ms": 0.2},
 "histograms": [{"name": "gui.paint.impl", "unit": "ms", "count": 5, "p50": 2.228, "p95": 15.27, …}, …], "upstream_counters": {},
 "budget": {"enabled": true, "quiet_ms": 1000, "at_rest": {"frames": 0, "unasked": 0, "timers": 0, "tasks": 3, "task_ms": 0.18, "frame_sites": [], "timer_sites": [], "task_sites": […]},
            "on_purpose": [{"purpose": "status", "ticks": 11, "past_rate": 0, "frames": 11, "timers": 22, "tasks": 132}],
            "news": {"frames": 4, "past": 0, "allowed": 30},
            "actions": [{"action": "tab", "budget_ms": 40.0, "count": 2, "over": 1, "unshown": 0, "mean_ms": 45.8, "p50_ms": 15.1, "p95_ms": 76.5, "p99_ms": 76.5, "max_ms": 76.5}, …], "lines": 3},
 "present": {"on": true, "available": {"frame_statistics": true, "frame_statistics_error": null, "compositor_clock": true},
             "actions": [{"action": "key", "count": 1, "seen": 1, "overtaken": 0, "lost": 0, "hidden": 0, "paint_ms": {"mean": 9.1, "max": 9.1}, "present_ms": {…}, "screen_ms": {…}, "after_present_ms": {…},
                          "last": {"paint_ms": 9.1, "present_ms": 21.4, "screen_ms": 38.0, "screen_unix_ms": 1793345523117.2}}, …],
             "motion": {"episodes": 1, "measured": 1, "partial": 0, "covered": 0, "refreshes": 31, "presents": 31, "missed": 0, "with_misses": 0, "paints": 4, "slow_paints": 0, "longest_paint_ms": 7.1,
                        "first_ms": {"mean": 24.9, "max": 24.9}, "names": [{"name": "pane.layout", "episodes": 1, "refreshes": 31, "missed": 0, …}, …], "last": {…}},
             "cost": {"polls": 2, "poll_ms": 0.02, "walks": 1, "walk_ms": 0.4, "frames_walked": 90, "covers": 1, "cover_ms": 0.01}},
 "fonts": {"configurations": {"count": 2, "ms": 0.0}, "fonts": {"count": 4, "ms": 13.9}, "lookups": {"count": 8, "ms": 4.3}, "files_read": {"count": 1, "ms": 1.4}, "substitutes": {"count": 14, "ms": 0.0},
           "built_in": {"count": 1, "ms": 1.6}, "fallback_searches": {"count": 0, "ms": 0.0}, "fallback_shared": {"count": 0, "ms": 0.0}, "shapers": {"count": 4, "ms": 0.0}, "kept": {"count": 0, "ms": 0.0},
           "atlas_made": {"count": 1, "ms": 0.0}, "atlas_cleared": {…}, "atlas_grown": {…}, "atlas_side": 1024, "reshapes": {"count": 0, "ms": 0.0}},
 "start": {"marks": {"bootstrap": 20.2, "config": 63.0, "mux": 64.1, "gpu_begun": 64.1, "frontend": 64.2, "gui_task": 64.4, "gui_startup": 69.3, "tab": 109.8, "first_paint": 546.0, "first_drawn": 727.0},
           "surfaces": {"count": 27, "ms": 126.5},
           "window": {"ms": 413.0, "at_ms": 112.8, "steps": {"fonts": 0.01, "metrics": 2.7, "state": 3.1, "os_window": 61.5, "gpu": 334.2, "render_state": 11.8, "show": 0.1}},
           "webgpu": {"ms": 334.0, "at_ms": 179.6, "steps": {"compositor": 0.8, "instance": 217.4, "surface": 0.1, "adapter": 0.02, "device": 80.4, "configure": 4.8, "shader": 1.3, "pipeline": 29.0}},
           "gpu-adapter": {"ms": 336.0, "at_ms": 62.9, "steps": {"instance": 25.9, "adapter": 308.2}}},
 "caches": {"glyph_cache": {"hits": 190, "misses": 0, "evictions": 0}, "line_quad_cache": {"hits": 9150, "misses": 61, "evictions": 61}, …},
 "words": {"laid_out": 0, "kept": 2140}, "quads": {"made": 0, "kept": 214}}
```

- It covers the span since the process started, or since the last `metrics` with `"reset": true`, which answers with the counts so far and then starts them again: ask with `reset` before something and again after it, and the second reply is what that something did. The counts are the process's, one set every reader shares, so a reset starts every reader's span again: the daily LBTerm (the default window class) refuses one unless the request says `"force": true` too, and a reader of it diffs two replies instead. A test window, of a class of its own, resets whenever it's asked.
- `budget` is what the budget judged over the span (lbterm/PERF.md, "Warnings in the daily build"): what ran at rest and who asked for it, what ran on purpose, the frames its background tabs' news brought (`news`: allowed, and past what's allowed), each action's count, how many were over their time and how many were given up on, with their mean, 50th, 95th and 99th percentiles and longest (from buckets of the budget's own, lbterm-extras/src/hist.rs), and how many lines it logged.
- `present` is what's hard to time (lbterm/PERF.md, "What's hard to time"): each action's times from what started it to its frame's paint, to its presents and to the refresh it went up at (`seen` of `count` went up; `hidden` went to a window others hid, `overtaken` and `lost` weren't seen), the last one's with when it went up in Unix milliseconds, and each episode of motion's refreshes, the ones that came without a frame (`missed`), its paints and those longer than a refresh. `available` says whether this PC gives the statistics it's read from; `cost` what reading them cost. It's measured only while `on`: off unless the window started with `LBTERM_PRESENT_TIMING` in its environment (a test window's) or a request turned it on with `"present": "on"` (`true` too; `"off"` or `false` turns it off), which takes effect before that request's reply.
- `fonts` is what font setup did over the span (lbterm/PERF.md, "What font setup costs"), each `{count, ms}`: font configurations made (a window's, and the throwaways a new window's size is worked out with), fonts made, lookups through the system's font locator, system font files read whole and parsed, families GDI put another face in place of and that were turned down by name before reading its file, the bundled fonts parsed (once a process), fallback searches and the characters answered from what another font found, shapers built, reloads that kept the fonts, glyph atlases made, cleared and grown, and every shape of a window cleared at once (`reshapes`). `atlas_side` is the largest side an atlas has had, a high-water mark a reset leaves.
- `start` is how the process's start and its last window's went (lbterm-extras/src/start.rs), never reset: `marks`, each in ms since `main` began, the first time the start got that far (its config read, the GPU's adapter begun once the start wasn't handed to a running LBTerm, the first tab, the first paint and its end); `window`, `webgpu` (that window's GPU state) and `gpu-adapter` (the process's instance and adapter, made once on a thread of their own), each its steps in ms, in all, and when it began (`at_ms`); `surfaces`, the compositor's surfaces made since the start and their time.
- `caches` is what the GUI's caches did over the span (lbterm/PERF.md, "What the caches keep"), by name: lookups that found their entry (`hits`), those that didn't (`misses`) and entries put out to make room (`evictions`), every window's caches of a name together: `line_quad_cache` (a line's quads; its misses are lines laid out again), `line_state_cache`, `line_to_ele_shape_cache`, `shape_cache`, `glyph_cache.image_cache` and `glyph_cache` (a glyph looked up as a line is laid out). A name that did nothing in the span is left out. They were upstream's `.hit.rate` and `.miss.rate` histograms until 2026-10-31.
- `words` is how many words the cards, the lists and the tab panel laid out over the span (`laid_out`) and how many they kept from the frame before (`kept`); `quads` how many times a part kept as quads was made again (`made`) and copied in as it was (`kept`): lbterm/PERF.md, "What the cards cost". While what they show stays the same, `laid_out` and `made` stay at 0 however many frames paint.
- It's answered on the pipe's own thread, not the GUI thread, unlike every other request: asking adds no task, no wake and no frame to what it measures, it answers while the GUI thread is held, and it isn't logged (a test asks it often). It needs no window.
- `reset` and `force` are `true` or `false`, `present` `on`, `off`, `true` or `false`; anything else is refused.

From a shell, `lbterm control metrics`, `lbterm control metrics --reset` (`--force` for the daily LBTerm's) and `lbterm control metrics --present on|off`; from a test script, the harness's `Get-Metrics [-Reset]`, `Format-Metrics` to say a reply in a few lines and `ConvertTo-MetricsTable` to look an entry up by site.

## From a shell

```powershell
lbterm control focus
lbterm control emit lbterm-tab-forward
lbterm control --class com.lachlanb.lbterm.try focus   # a try window
lbterm control draft add '1a\n2. yes'                   # \n is a new line
lbterm control draft --pane 14 send
lbterm control draft picker --label '1b 2a' --key 2 --key 1 --key 1 --question 'Which colour?' --question 'And its ring?'
lbterm control mic hearing --ttl 20
lbterm control mic listening --heard 'typed 1a'
lbterm control --class com.lachlanb.lbterm.try metrics --reset  # the counts so far, then from zero again
lbterm control metrics --present on                     # the daily LBTerm times its frames to the screen from now on
```

Each prints the reply as one JSON line. It exits 1 when LBTerm says no (the reason on stderr too), when no LBTerm of that class is running, or when there's no answer in 10 s.

## Checking it

`cargo test -p lbterm-extras` (through `lbterm.ps1 test -Package lbterm-extras,config`) covers reading requests, refusing what isn't one, and the replies, and for drafts their pieces, lines and keys and reading Claude Code's screen (the prompt, a permission prompt, a picker); `config`'s tests the pipe's name and `LbtermDraft`. `lbterm\perf\control.ps1 [-Mux]` runs the pipe end to end in a test window: `focus` naming the open tab's pane and the Claude session in it, `emit` hopping tabs both ways round the ends with handlers like Lachlan's, the refusals, and one connection carrying several requests. `lbterm\perf\draft.ps1` does drafts, against `perf\fake-prompt.ps1`, a stand-in for Claude Code that writes down every key it's sent: the card, its ticks and the red ring, scratch, send refused over a permission prompt, send at the prompt, edit from a key, a picker's picks, a new session. `lbterm\perf\mic.ps1` does the mic mark: listening, hearing lit red, what was heard on the strip, gone after its ttl, muted and off. `lbterm\perf\metrics.ps1 [-Mux]` does `metrics` (lbterm/PERF.md): the process, nothing painted at rest, every frame the state file counts counted with who asked for it, keys, a split's tasks and PDUs, resets and the cli. `lbterm\perf\budget-log.ps1 [-Mux]` reads its `budget` part, each action's percentiles among it, beside the lines the window logs. `lbterm\perf\present.ps1 [-Mux]` reads its `present` part for keys, motion and keys beside a writer at full speed, and turns it off and on again. `lbterm\perf\font-setup.ps1` reads its `fonts` and `start` parts through a start, a reload, a zoom, a font size changed and a new window. `lbterm\perf\line-cache.ps1` reads its `caches` part around a tab shown once the caches are full.

The code: `wezterm-gui/src/control.rs` (the pipe and its threads), `wezterm-gui/src/termwindow/control.rs` (what a window does for each request), `wezterm-gui/src/drafts.rs` (the drafts, one a pane), `wezterm-gui/src/termwindow/render/draft.rs` (the card) and `render/questions.rs` (the ticks), `lbterm-extras/src/control.rs` (the lines), `lbterm-extras/src/drafts.rs` (what a draft does, and reading the screen), `wezterm-gui/src/mic.rs` and `lbterm-extras/src/mic.rs` (the mic mark), `wezterm-gui/src/counters.rs` and `lbterm-extras/src/counters.rs` (`metrics`), `lbterm-extras/src/budget.rs` and `wezterm-gui/src/termwindow/budget.rs` (its `budget`), `lbterm-extras/src/present.rs` and `wezterm-gui/src/present.rs` (its `present`), `wezterm-font/src/lbterm.rs` and `wezterm-gui/src/counters.rs`'s `fonts` (its `fonts`), `lbterm-extras/src/start.rs` (its `start`), `lfucache/src/lib.rs`'s `counts` (its `caches`), `wezterm/src/control.rs` (`lbterm control`).
