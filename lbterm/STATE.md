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
- **The frame an animation ends on.** Most of LBTerm's motion runs in DWM (`compositor.rs`): the GUI sets targets and paints nothing while the panes slide, the rail opens or the strip's card fades. So the compositor keeps every animation it starts, by name, from its start to its end (`drive` and `play`, where every animation that ends is made; the ones that never end are `loops`), and a window with any in flight asks for one frame more 2 ms after the last of them ends. That frame's snapshot says `animating: false`. Without the compositor (OpenGL, `lbterm_compositor = false`) or with motion off, nothing animates in DWM and everything is there in the frame that shows it.

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

### A window

| Field | |
|---|---|
| `id` | the GUI's window id (Lua's `window:window_id()`; on a mux domain, not the server's) |
| `hwnd` | its HWND, to post messages to |
| `frame` | frames it has painted, 0 before its first |
| `focused` | Windows gave it the keyboard (a test window never has it) |
| `keyboard` | it shows as having the keyboard: focused, or a test window (`LBTERM_NO_ACTIVATE`) that no posted `WM_KILLFOCUS` has told otherwise |
| `animating` | true exactly while it has asked for another frame to carry an animation, fade or glow on: something in `animations` |
| `animations` | `[{name, progress, left_ms}]`, soonest to end first: what's moving (below), how far through its time (0 to 1), how long it has left |
| `waits` | `[{name, left_ms}]`: delays under way that will change what shows when they're up (below) |
| `settled` | `animating` false and no `waits` |
| `loops` | animations that never end while what they show is there, carried by DWM (below); they don't count as `animating` |
| `size` | `{width, height, dpi}`: the client area in pixels |
| `pointer` | `{x, y, over, row, item, pane, button}`: where the pointer last was (posted or real), and what's there; `null` once it has left the window |
| `active_tab` | the window's index of the open tab |
| `active_pane` | the pane with the keyboard (an overlay's, while one is up) |
| `tabs` | `[{index, id, title, active, panes, zoomed, colour}]`; `title` is the tab's own, empty until one is set (the rail's row says what it shows); `colour` is the tab menu's, `"#rrggbb"` |
| `panes` | the open tab's panes as painted (a zoomed tab's one): `{id, server_id, rect, cols, rows, active, zoomed, title, alt_screen, mouse_reporting, cursor: {x, y}, scroll_top, session, progress}` |
| `scrollbar` | `{enabled, shown, marks}`: whether it shows beside the active pane (not while its program has the whole screen), and its prompt marks |
| `rail` | the tab panel, with `lbterm_tab_panel` on (below) |
| `strip` | the window strip's words, while the tab panel hides the tabs (below) |
| `menu` | the menu that's open (below) |
| `overlay` | `"modal"` (command palette, character select, pane select), else an overlay pane's own title (launcher, search, copy mode, confirm close) |
| `readout` | the size readout while it shows or fades: `"120 × 32"`, `"110% · 109 × 29"` |
| `landed` | `{pane, row, left_ms}`: the row a prompt jump, a scrollbar mark or a search landed on, while it's marked |
| `questions` | the pane whose question colours show |
| `hearts` | bursts of hearts in the air |

Rectangles are `{x, y, w, h}` in the window's client pixels, which `WM_MOUSEMOVE`'s `lParam` and a client-area capture count in: post a move to a row's pad with `x + w/2, y + h/2`. A pane's `rect` is its card where it's laid out; while the tab panel is open, DWM shows it pressed to the right without moving it. `server_id` is the mux server's pane id on a mux domain (what `lbterm cli` takes there), `null` when it's the GUI's own. `cursor.y` counts from the top of the pane's screen. `scroll_top` is the stable row at the top of the view while it's scrolled back, `null` at the bottom. `session` is the pane's `claude_session` user var. `progress` is OSC 9;4's: `"42%"`, `"error 42%"`, `"indeterminate"`, `"paused"`.

### `rail`

| Field | |
|---|---|
| `open` | open, or still open while it waits out its grace |
| `phase` | `closed`, `opening` (waiting out `open_delay_ms`), `open`, `closing` (waiting out `close_delay_ms`) |
| `light` | the ring's light: `rest`, `pending`, `full` |
| `rect` | the panel's card, open or closed as `open` says |
| `rows` | the rail's rows, in order (below) |
| `hovered` | the row under the pointer |
| `open_row` | the row with the open tab's ring and bar; `null` while they're on the Conductor's pad |
| `conductor` | `{level, label, remote, tab, open, hovered, pad}`: the Conductor's pad, with `lbterm_conductor` set; `level` is `off`, `read`, `most` or `all` |
| `dragging` | the row of a tab being dragged along the rail |
| `drop_hint` | a tab dragged from another window is over this one, and the rail shows where it would land |

A row: `{row, tab, tab_index, visible, rect, pad, label, title, project, state, status, message, progress, agents, waiting, unread, age, active, hovered, colour, conductor}`. `rect` is its slot across the open panel, `pad` its pad on the rail. `state` is `none`, `working`, `waiting` (blocked on you), `idle` or `error`; `status` the word its pad says ("Working", "Question", "Your turn"); `waiting` how many of its program's own tasks still run after it handed the turn back (the turning arc); `conductor` is `thinking` or `acting` while the Conductor is on its session. The Conductor's own tab has no row.

### `strip`

`{title, summary, caption, notice, prompt, note, badge, card, restart, jumping, lit, items}`: the words as `lbterm/DESIGN.md`'s "The window strip" names them. `card` is `summary` or `prompt` while the card under the strip is open. `restart` is the "Restart to update" pill showing. `jumping` is a jump back to a prompt under way (Claude Code's view being scrolled to it). `lit` is what's lit under the pointer (`close`, `maximize`, `minimize`, `restart`, `words`, `prompt`, `arrow`), and `items` where each of those is, `[{name, rect}]`.

### `menu`

`{kind, tab, rect, items, focus, renaming, waiting}`, for whichever is open: `kind` is `tab` (the tab menu, with `tab` its tab and `renaming` the name being typed), `prompts` (the list of prompts, newest first, or the one row saying why there are none) or `conductor` (the Conductor's, with `waiting` the item waiting for its answer). Each item is `{label, checked, rect}`; `rect` is `null` for a prompt scrolled out of view. `focus` is the item under the pointer or picked with the arrow keys.

### `pointer.over`

| `over` | with |
|---|---|
| `menu.item`, `menu` | `item` |
| `rail.conductor`, `rail.row`, `rail` | `row` |
| `strip.card` | |
| `strip.button`, `strip.pill`, `strip.words`, `strip.prompt`, `strip.arrow` | `button` (`close`, `maximize`, `minimize`, `restart`, `words`, `prompt`, `arrow`) |
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
| `pane.dim` | a pane going to 85% as it loses the keyboard, and back | `dim` |
| `outline.move` | the active outline gliding to the pane that takes the keyboard, or with its pane | `focus_move` |
| `outline.arrive` | the outline's pulse as the keyboard arrives from outside | `arrive_in`, `arrive_out` |
| `outline.dim` | the outline going out as the window loses the keyboard, and back | `dim` |
| `rail.edge` | the tab panel's edge opening or closing | `panel_open`, `panel_close` |
| `rail.light` | the ring's light and glow brightening or dimming | `panel_glow`, `hover` |
| `rail.words` | the open panel's words fading in or out | `pane_enter`, `pane_exit` |
| `rail.sweep` | the light running round the ring as it opens | `panel_sweep` |
| `rail.row.grow`, `rail.row.enter` | a new tab's slot opening, and its row fading in | `pane_layout`, `pane_enter` |
| `rail.row.exit` | a closed tab's row fading where it was | `pane_exit` |
| `rail.row.slide` | rows sliding to new slots; a dragged row let go | `pane_layout` |
| `rail.row.swap`, `rail.row.hover` | a row's old look dissolving into its new one; a hover's | `swap`, `hover` |
| `rail.marker` | the open tab's ring and bar gliding to the new row | `focus_move` |
| `rail.progress` | a progress bar gliding to its new length | `pane_layout` |
| `strip.hover` | a window button's or the strip's look fading under the pointer | `button_hover` |
| `strip.card` | the strip's card fading in or out | `pane_enter`, `pane_exit` |
| `strip.swap` | the strip's old words dissolving into the new | `swap` |
| `light.press` | a right-click's ripple | `press` |
| `light.copy` | a copy's or an opened link's flash | `copy_in`, `copy_out` |
| `light.glint` | a pad's glint (typed into from `lbterm cli`, renamed) | `copy_in`, `copy_out` |
| `readout` | the size readout fading out | `pane_exit` |
| `hearts` | the hearts rising (drawn by the GUI, frame by frame) | |
| `wezterm` | upstream's own next frame (a blinking cursor or text, an animated image), which it asks for only while the window is focused | |

Several numbers moving under one name (a pane's x, y, width and height) are one animation, from the first one's start to the last one's end. An animation turned round before it ended (the pointer leaving the rail while it opens) is still counted until the old one would have ended, so `animating` can stay true for at most what that one had left. It never goes false early.

### Wait names

| Name | Until |
|---|---|
| `rail.open`, `rail.close` | the tab panel opens after resting on the rail, or closes after the pointer left |
| `strip.card.open`, `strip.card.close` | the strip's card opens after the pointer rested on the words, or closes after it left |
| `strip.prompt.open`, `strip.prompt.close` | the same for a prompt cut short |
| `strip.notice` | a notice leaves the caption's place |
| `strip.note` | a jump's note leaves the prompt's place |
| `strip.jump` | a jump back to a prompt lands or gives up; how long isn't known ahead, so `left_ms` is 0 |
| `readout` | the size readout starts to fade |
| `landed` | a landed row's mark goes |
| `conductor.answer` | a Conductor item stops waiting for its answer (45 s at most) |

### Loop names

`rail.spinner` (a pad whose program waits on its own tasks), `conductor.outline` (the Conductor's turning hues), `conductor.light` (the light going round a tab it acts on), `menu.spinner` (a Conductor item waiting for its answer). With motion off they hold still and aren't listed.

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
      "cursor": { "x": 11, "y": 2 }, "scroll_top": null, "session": null, "progress": null
    }],
    "scrollbar": { "enabled": true, "shown": true, "marks": 0 },
    "rail": {
      "open": true, "phase": "open", "light": "full", "rect": { "x": 4, "y": 57, "w": 290, "h": 637 },
      "rows": [{
        "row": 0, "tab": 0, "tab_index": 0, "visible": true,
        "rect": { "x": 4, "y": 65, "w": 290, "h": 75 }, "pad": { "x": 27, "y": 86, "w": 22, "h": 20 },
        "label": "pwsh", "title": "pwsh", "project": "Projects", "state": "none", "status": "", "message": "",
        "progress": null, "agents": 0, "waiting": 0, "unread": false, "age": "", "active": true, "hovered": true,
        "colour": null, "conductor": null
      }],
      "hovered": 0, "open_row": 0, "conductor": null, "dragging": null, "drop_hint": false
    },
    "strip": {
      "title": "pwsh", "summary": null, "caption": null, "notice": null, "prompt": null, "note": null, "badge": null,
      "card": null, "restart": false, "jumping": false, "lit": [],
      "items": [{ "name": "close", "rect": { "x": 952, "y": 0, "w": 46, "h": 30 } },
                { "name": "words", "rect": { "x": 80, "y": 0, "w": 28, "h": 28 } }]
    },
    "menu": null, "overlay": null, "readout": null, "landed": null, "questions": null, "hearts": 0
  }]
}
```

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
