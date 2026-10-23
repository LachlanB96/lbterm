# Feedback

Every action answers at once (`lbterm/DESIGN.md`, principle 1). This is everything you can do in LBTerm, what answers it today, and what's still silent. Lachlan asked for the rule to be enforced on 2026-10-21, after the tab panel kept him waiting: "once everything has feedback, the terminal feels fluid and smooth."

The inventory was taken on 2026-10-21 from `main` (af39a0561) with the tab panel and strip fixes from `feat/panel-feedback`, before the Conductor moved from the strip to the rail (aab8990b0): its pad and outlines aren't in it yet. A separate agent is researching new animations and feedback. Whatever Lachlan picks from that research lands in `DESIGN.md`'s tokens, and the matching line here changes from gap to planned.

## The rule

- **The next frame.** Whatever the action is (a key, a click, the pointer arriving on something or leaving it, a program asking for you), something on screen changes in the next frame. Motion can follow, but the answer can't wait for it.
- **A delay guards; it never silences.** A rest before something opens, or a grace before it closes, holds back only what it guards. Light shows the wait meanwhile: the tab panel's ring goes to its pending light the moment the pointer reaches the rail or leaves the panel, and back the moment it changes its mind.
- **A change of mind turns things round.** Leaving while something opens closes it from where it is; coming back while it closes opens it again from there. Nothing plays out to the end first.
- **A key that does nothing still says so.** A key with nowhere to go gets a small answer where it would have acted. Silence there reads as a dropped key.
- **New actions come with an answer.** A change that adds an action adds its line here, with what answers it.

Status: **answers** (built), **planned** (in `DESIGN.md`, not built yet), **gap** (no answer planned; the proposal given is for Lachlan to pick or change).

## Panes, from the keyboard

| Action | What answers today | Status |
|---|---|---|
| Focus to another pane | The outline glides there on `focus_move` and the panes dim on `dim`, in DWM, the next frame | answers |
| Focus with no pane that way | Nothing: upstream does nothing | gap: the active outline brightens along the edge it tried to cross and fades back (`hover` in, `pane_exit` out) |
| Split | The new card comes in on `pane_layout` and `pane_enter`, once the mux server has made the pane | answers (after a round trip to the server) |
| Close a pane | The confirm overlay if it asks, then a `pane_exit` fade as the others grow | answers |
| Zoom | The planner grows the clip and fades the others (`lbterm-motion/src/plan.rs`). While it's zoomed, a badge after the strip's title says which pane shows: "Zoomed · 2 of 3" | answers (2026-10-21) |
| Switch tab | The content and the strip's title swap at once; the rail's ring and bar glide to the new row on `focus_move` | answers on the rail (2026-10-21); planned: `tab_switch` cross-fade |
| Switch tab with only one | Nothing | gap: the rail's open-tab bar pulses once (light only) |
| New tab | Once the server has it, its row grows into the rail (its slot opening on `pane_layout`, fading in on `pane_enter`), then an empty card until the shell draws | answers once the server has it (2026-10-21); gap: nothing in the round trip before, a dim pad saying "Starting" |
| Close a tab | Its row fades where it was on `pane_exit` and the rows below slide up on `pane_layout`; its panes vanish | answers on the rail (2026-10-21); gap: the panes fading on `pane_exit` |
| Copy | What's copied lights to white 22% and fades (`copy_in`, `copy_out`): a drag let go, Ctrl+Shift+C, a quick-select pick (render/lights.rs) | answers (2026-10-21) |
| Paste | Only the program's echo; an empty clipboard is silent | gap (minor) |
| Shift+right-click smart paste | A ripple where it landed, and no wait: Terminal's `lbterm.lua` asks `wezterm.gui.clipboard_has_image()`, which asks Windows at once (250 to 310 ms of powershell.exe before) | answers (2026-10-21) |
| A plain right-click in a pane | A ripple from where it landed (`press`), though lbterm.lua swallows the click so Claude Code doesn't paste | answers (2026-10-21) |
| The window losing the keyboard | The active outline goes out on `dim` | answers (2026-10-21) |
| The keyboard arriving from outside: the window taking it back (summon, a toast's click, Alt+Tab), a pane activated with no key or click in the window (AI Dashboard, Conductor, a pane closing by itself) | The outline comes back with a pulse to white 35% (`arrive_in`, `arrive_out`); a move a key or click made glides without one (`lbterm_extras::arrival`) | answers (2026-10-21) |
| Resizing the window, maximising, snapping | A card over the active pane says its size in cells, at once, fading 700 ms after the last change | answers (2026-10-21) |
| Font bigger, smaller, back (Ctrl+=, Ctrl+-, Ctrl+0) | The same card, with the font's size: "110% · 109 × 29", for 1.2 s | answers (2026-10-21) |
| Search's next or previous match | The row it lands on is marked in the panel's coral for 1.6 s, as a prompt jump's (not while the pattern is typed) | answers (2026-10-21) |
| Search, quick select, launcher, command palette | The overlay or modal the next frame, no motion | answers |
| Copy mode | Only the cursor. Its "Copy mode:" title never reaches the strip, which names the tab | gap: a "Copy mode" chip in the strip, as for zoom |
| Scroll past the end of scrollback | Nothing | gap (minor) |
| The bell | A system beep; `visual_bell` is off | planned: `bell`, a light running along the pane's outline |

## Panes, with the mouse

| Action | What answers today | Status |
|---|---|---|
| The pointer on a pane | Nothing | planned: the hover outline and the light near the cursor (build step 3) |
| The pointer on a gutter | The resize cursor | planned: both edges lit (build step 4) |
| Dragging a gutter | The split follows, retargeted on every move | answers |
| Click a pane | Focus glides there | answers |
| Click a pane while the panel presses it over | The panel closes at once and the click goes no further | answers (the click's intent is dropped, by design: the pane isn't where it shows) |
| Select, hover a link | Upstream draws them at once | answers |
| Click a link | It lights up and fades as it opens (`copy_in`, `copy_out`), in a program that has the mouse too (Claude Code), where the click used to go to the program and nothing opened | answers (2026-10-21) |
| Click a prompt mark on the scrollbar | The prompt scrolls to the top, marked in the panel's coral for 1.6 s | answers (2026-10-21) |

## The tab panel

| Action | What answers today | Status |
|---|---|---|
| The pointer onto the closed rail | The ring's light comes up to pending at once on `hover`; the edge opens after `open_delay_ms` (150) and the light carries on to full. A pad under the pointer also gets its highlight | answers (fixed 2026-10-21: before, 150 ms of nothing, and nothing at all over the open tab's pad or between pads) |
| Passing over the rail | The light comes up and goes back; nothing moves | answers |
| Leaving the open panel | The light drops to pending at once; the edge closes after `close_delay_ms` (300) | answers (fixed 2026-10-21) |
| Out and back within the grace | The light dips and comes back up; the panel stays | answers (fixed 2026-10-21: before, nothing at all) |
| Leaving while it opens | It turns round at once, from where the edge is | answers (fixed 2026-10-21: before, it opened all the way first) |
| Coming back while it closes | It opens again at once, from where the edge is | answers (fixed 2026-10-21: before, it finished closing and waited 150 ms) |
| The pointer on a row | Its highlight, at once | planned: the highlight fading on `hover` |
| Click a row or a pad | The tab switches at once | planned: `press` ripple |
| Right-click a row | The tab menu the next frame | answers |
| Drag a tab | Past 6 px the grabbing cursor; the row lifts, a shadow under it, and follows the pointer while the others slide out of its way | answers (lifted 2026-10-21) |
| Drag a tab off the panel or onto another window | Past the tear-off distance a card with the tab's name follows the pointer, never taking the keyboard; another LBTerm window under it rings an empty slot after its last tab in the accent | answers (2026-10-21) |
| Rename a tab | Its label dissolves into the new name on `swap` and its pad glints (`copy_in`, `copy_out`) | answers (2026-10-21) |
| Colour a tab | Its row dissolves into the new colour on `swap` | answers (2026-10-21) |

## The window strip

| Action | What answers today | Status |
|---|---|---|
| The pointer on a window button or the restart pill | Its look fades in on `button_hover`, in DWM | answers |
| Clicking any of them | No pressed look | planned: `press` |
| The pointer on the title and summary | The summary brightens from 60% to 85% at once, fading in on `button_hover` on a hover slot; the detail card opens after 300 ms | answers (fixed 2026-10-21: before, nothing until the card opened) |
| Coming back to the words while the card fades out | It opens again at once, from where it is | answers (fixed 2026-10-21: before, it waited out the 300 ms rest again) |
| Leaving while the card fades in | It waits out its 200 ms grace, then closes | answers: the pointer crosses the prompt row on its way down to the card, so this one doesn't turn round at once |
| The pointer on the prompt | It brightens from 80% to 100% at once; all of it in the card after 300 ms when it's cut short | answers |
| Click the prompt (jump) | Nothing until Claude Code's view has scrolled there, which can take several steps of up to 1.2 s each; a second click while it runs is dropped | gap: the prompt says "Jumping…" in the panel's coral until it lands, and a second click retargets the jump |
| Click the arrow | Its chip brightens at once; the list opens the next frame, "Reading your prompts…" until it has them | answers |
| Click the restart pill, Ctrl+Shift+F5 | The pill says "↻ Restarting…" and the strip "Restarting…" before the window ends; the new window's strip says "Updated to <version> · N tabs back" | answers (2026-10-21) |
| The GUI started again after a crash | Its strip says "Reattached N tabs", the tabs it found running in the mux server | answers (2026-10-21) |
| Saving the config (`lbterm.lua`) | "Config reloaded" in the strip's caption for 4 s | answers (2026-10-21) |
| A new summary, the last prompt, a notice, the badge | The old words dissolve into the new on `swap`, and a new summary or prompt brightens its row once | answers (2026-10-21) |
| Alt+S | The card pins about 60 ms later (the summaries thread); a tab with no detail stays silent | gap (minor): with nothing to show, the summary flashes once |

## Menus (the tab menu, the prompt list, the Conductor's menu)

| Action | What answers today | Status |
|---|---|---|
| Open | The next frame, no motion | answers; later (Lachlan, 2026-10-21): the strip card's fade, `pane_enter` in and `pane_exit` out (`DESIGN.md`, "Menus") |
| A row under the pointer, the arrow keys | Its highlight, at once | planned: `hover`; later (2026-10-21): the highlight gliding between rows |
| Upstream's overlays (launcher, command palette, confirm-close, search, quick select, debug) | The next frame, in WezTerm's style | later (Lachlan, 2026-10-21): redrawn as cards in LBTerm's look, with the menus' fade |
| Esc, a click away | Closes the next frame | answers |
| Enter with no row picked | Nothing | gap: the first row lights, as though picked |
| Other keys | Swallowed silently | gap (minor) |
| Colour, close others | At once | answers |
| Duplicate | Nothing until the server has the new tab | gap: as for a new tab |

## Events

| Event | What shows today | Status |
|---|---|---|
| A tab's state, words or label changing | Its row dissolves from the old look into the new on `swap`, the next frame | answers (2026-10-21) |
| A program's progress | Its bar on the open panel glides to the new length on `pane_layout` | answers (2026-10-21) |
| A program typing into a tab through `lbterm cli` (the Conductor, Relay, AI Dashboard) | That tab's pad glints (`copy_in`, `copy_out`) | answers (2026-10-21) |
| Claude blocked on you | The pad dissolves into orange with its word on `swap` | planned: `alert`, a pulse and then a glow until you look |
| ...while LBTerm isn't the foreground window | No taskbar flash; a toast only when Claude Code sends OSC 99 | gap: the taskbar button flashes once |
| Claude done in a tab you're not on | A white unread dot on the pad | planned: `alert` |
| Output in a tab you're not on | Nothing (`has_unseen_output` exists, the panel doesn't use it) | planned: a soft glow until you look |
| A bell in a tab you're not on | Nothing on the panel | gap: that tab's pad glows, as for output |
| A program waiting on its own tasks | The arc turning on its pad, in DWM | answers |
| A newer build installed | The restart pill appears within 10 s, at once | gap (minor): the pill fades in on `pane_enter` |

## The gaps by how often they'd be felt

On a keyboard-heavy day with 8 or more Claude tabs:

1. New tabs: Ctrl+T shows nothing until the server answers (a closed tab's row now fades where it was, and a new one grows in once it exists).
2. Keys with nowhere to go: Alt+Up and Alt+Down in a tab with one pane (most Claude tabs), and the tab keys with one tab.
3. Attention from elsewhere: a bell in another tab, Claude blocked while LBTerm is behind another window.
4. Modes with the tabs hidden: zoom and copy mode show nowhere.
5. The prompt jump: no sign it's working, and a second click is lost.
6. Menus: Enter with nothing picked.
7. Restarting: the window goes with nothing bridging to the new one.
8. ~~Dragging a tab out: nothing shows where it will land.~~ Built 2026-10-21: the ghost card and the other window's slot.
9. The smart-paste probe (Terminal's config).
