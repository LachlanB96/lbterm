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
| Switch tab | The new tab is there at once with the keyboard in it, and the old tab's last frame fades out over it on `tab_switch`, its active outline with it where the new tab's doesn't draw; the strip's words dissolve with it on `swap`, and the scroll strip's thumb and marks with it on `tab_switch`; the rail's ring and bar glide to the new row on `focus_move` as the two rows dissolve into their new looks. Switching on mid-fade starts from what shows, and a tab switched back to comes back with what it shows now; in a fast run of side-button presses or rail clicks every tab in flight keeps its share of the panes and the strip's words, never rising, and going back to the oldest moves nothing | answers (the rail 2026-10-21, the panes and the strip 2026-10-29, tabs in flight 2026-10-31) |
| Switch tab with only one | Nothing | gap: the rail's open-tab bar pulses once (light only) |
| New tab | The frame after it's asked for, a Starting row grows in at the end of the tabs (a dim pad with the white waiting arc turning on its corner, "Starting…" on the open panel); when the tab comes the row dissolves into it on `swap`, in front with the ring gliding to it and the switch's fade, behind with its pad's glint; nothing in 20 s, or Lua taking it back, and the row folds where it was while the strip says "Nothing opened for …" for 4 s. Every tab asked for in the window shows it: the + pad's picker and menu, Ctrl+Alt+T, Reopen, the tab menu's Shell in its folder (Duplicate tab until landing 2), and Lua's `window:lbterm_expect_tab` (the InMail button, links) (lbterm/NEW-TAB.md) | answers (the row once the server has it 2026-10-21, the Starting row 2026-11-04) |
| Click the + pad, or Ctrl+T | The picker beside the rail in the next frame, the pad's ripple held inside it, the panes dimming and the active outline going out while it has the keys; its first row, the open tab's project, already highlighted | answers (2026-11-04) |
| Reopen a tab closed (Ctrl+Shift+T, the + menu's Reopen rows) | Its Starting row in the next frame, then the tab in its folder, in front; the + menu says "Nothing closed yet" with none kept | answers (2026-11-04) |
| Close a tab | Its panes fade out where they were over the next tab's on `tab_switch`, its active outline with them where the next tab's doesn't draw, and the strip's words dissolve into the next tab's on `swap`, the strips down the right with them on `tab_switch` (the objectives strip's marks fading where they were as the rows move up); its row fades where it was on `pane_exit`, the rows below slide up on `pane_layout`, and the next tab's row dissolves into its open look as the ring and bar glide to it | built (2026-10-29) |
| Copy | What's copied lights to white 22% and fades (`copy_in`, `copy_out`): a drag let go, Ctrl+Shift+C, a quick-select pick (render/lights.rs) | answers (2026-10-21) |
| Paste | Only the program's echo; an empty clipboard is silent | gap (minor) |
| Shift+right-click smart paste | A ripple where it landed, and no wait: Terminal's `lbterm.lua` asks `wezterm.gui.clipboard_has_image()`, which asks Windows at once (250 to 310 ms of powershell.exe before) | answers (2026-10-21) |
| A plain right-click in a pane | A ripple from where it landed (`press`), though lbterm.lua swallows the click so Claude Code doesn't paste | answers (2026-10-21) |
| The window losing the keyboard | The active outline goes out on `dim` | answers (2026-10-21) |
| The keyboard arriving from outside: the window taking it back (summon, a toast's click, Alt+Tab), a pane activated with no key or click in the window (the Claude project, the Conductor, a pane closing by itself) | The outline comes back with a pulse to white 35% (`arrive_in`, `arrive_out`); a move a key or click made glides without one (`lbterm_extras::arrival`) | answers (2026-10-21) |
| Holding a modifier: Ctrl, Shift, Alt | The active outline goes to its full light in the modifier's colour, the next frame (`arrive_in`): Ctrl blue, Shift green, Alt red, added as light when several are held (Ctrl+Shift cyan, Ctrl+Alt magenta, Shift+Alt yellow, all three white); let go, it settles back to white at rest (`arrive_out`). What's held is what LBTerm reads off each key and pointer message, the modifiers it gives the next key, so a Ctrl that Windows never reported shows no blue (Lachlan, 2026-10-25, after Ctrl+T sometimes typed a "t"). The Win key isn't shown (`termwindow/held.rs`; `perf\modifier-light.ps1`) | answers (2026-10-25) |
| Resizing the window, maximising, snapping | A card over the active pane says its size in cells, at once, fading 700 ms after the last change; not while the Conductor's view is over the pane | answers (2026-10-21) |
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
| Click a link to an InMail message that's been dealt with | It lights up as any link does; it's grey and struck through already (lbterm/INBOX.md, "Links to a message"), and in Lachlan's config nothing opens | answers (2026-10-23) |
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
| Copy link (the tab menu, a Claude tab) | The menu fades and the tab's pad glints (`copy_in`, `copy_out`) | answers (2026-11-05) |
| New session in <Project> (the tab menu) | The menu fades, then as a pick in front: the Starting row from the next frame until the tab comes | answers (2026-11-05) |
| Wait on… (the tab menu, a Claude tab with `lbterm_links` and another Claude tab) | The card's rows dissolve on `swap` into a page of the other Claude tabs, each after a dot in its pad's colour; picking one makes the link as a right drag's drop does, Left or Esc turning back the same way | answers (2026-11-05) |
| Close finished tabs (the tab menu, while one qualifies) | Its second line names them as the menu opens; picked, every one still finished closes in one turn, each row fading where it was (`pane_exit`) while those below slide up on one spring (`pane_layout`), each kept to reopen. With the open tab among them the next tab dissolves in (`tab_switch`), but the marker lands on its row at once where `focus_move` should carry it (FLOWS.md, "The tab menu's facts, and a pick from it", What could go) | answers (2026-11-05); gap: the marker's glide |
| Middle-click a row (not the Conductor's or the + pad) | Its tab closes at once, as Close tab does: the row fades and the rows below close up, its close mark written, Ctrl+Shift+T bringing it back | answers (2026-11-05, his 10a) |

## The window strip

| Action | What answers today | Status |
|---|---|---|
| The pointer on a window button, the restart pill or the InMail button | Its look fades in on `button_hover`, in DWM | answers |
| Clicking any of them | No pressed look | planned: `press` |
| The pointer on the title and summary | The summary brightens from 60% to 85% at once, fading in on `button_hover` on a hover slot; the detail card opens after 300 ms | answers (fixed 2026-10-21: before, nothing until the card opened) |
| Coming back to the words while the card fades out | It opens again at once, from where it is | answers (fixed 2026-10-21: before, it waited out the 300 ms rest again) |
| Leaving while the card fades in | It waits out its 200 ms grace, then closes | answers: the pointer crosses the prompt row on its way down to the card, so this one doesn't turn round at once |
| The pointer on the prompt | It brightens from 80% to 100% at once; all of it in the card after 300 ms when it's cut short | answers |
| Click the prompt (jump) | Nothing until Claude Code's view has scrolled there, which can take several steps of up to 1.2 s each; a second click while it runs is dropped | gap: the prompt says "Jumping…" in the panel's coral until it lands, and a second click retargets the jump |
| Click the arrow | Its chip brightens at once; the list opens the next frame, "Reading your prompts…" until it has them | answers |
| Click the InMail button | It stays lit as under the pointer while its list is open; the list opens the next frame | answers (2026-10-23) |
| Pick a message in the InMail list | The list closes at once; the new tab grows in on the rail a moment later, once its program has started it (Lachlan's config: the Conductor's `new`, which doesn't switch to it) | answers (2026-10-23) |
| Click the restart pill, Ctrl+Shift+F5 | The pill says "↻ Restarting…" and the strip "Restarting…" before the window ends; the new window's strip says "Updated to <version> · N tabs back" | answers (2026-10-21) |
| The GUI started again after a crash | Its strip says "Reattached N tabs", the tabs it found running in the mux server | answers (2026-10-21) |
| Saving the config (`lbterm.lua`) | "Config reloaded" in the strip's caption for 4 s | answers (2026-10-21) |
| A new summary, the last prompt, a notice, the badge | The old words dissolve into the new on `swap`, and a new summary or prompt brightens its row once | answers (2026-10-21) |
| Alt+S | The card pins about 60 ms later (the summaries thread); a tab with no detail stays silent | gap (minor): with nothing to show, the summary flashes once |

## Menus (the tab menu, the prompt list, the Conductor's menu, the InMail list)

| Action | What answers today | Status |
|---|---|---|
| Open | The next frame, fading in on `pane_enter` (the tab menu, the prompt list, the Conductor's menu, the InMail list, the agenda and the + menu, each in its slot of the compositor's `menus`); with motion off, there at once | answers (2026-11-05, NEW-TAB.md landing 2, section 6) |
| A row under the pointer, the arrow keys | Its highlight glides from the lit row on `hover`, its light coming up or going out; a highlight where none showed, a card just opened or moved, and motion off place it at once | answers (2026-11-05) |
| Upstream's overlays (launcher, command palette, confirm-close, search, quick select, debug) | The next frame, in WezTerm's style | later (Lachlan, 2026-10-21): redrawn as cards in LBTerm's look, with the menus' fade |
| Esc, a click away | The card fades out on `pane_exit` from where it is, taking no click or key while it goes; with motion off, gone the next frame | answers (2026-11-05) |
| Enter with no row picked | The first row lights, in every menu; Enter again picks it | answers (2026-11-05, NEW-TAB.md landing 2) |
| A row's letter, a swatch's digit, Delete (the tab menu, the + menu) | The row's own answer: the rename, the colour, the tab closing; each row's letter shows dim at its right | answers (2026-11-05) |
| Other keys | The card's edge flares once, its hairline and the light round it going white (`arrive_in`, then `panel_glow`; his 9a of 2026-11-08, where the hairline alone, 35% of the way to white for about 150 ms, was too faint to see); nothing reaches the pane. A key bound in the config does nothing until the menu has closed (his 7b of 2026-11-07), but for a key the + menu shows beside a row, which picks it (his 5a) | answers (2026-11-05; seen at real speed 2026-11-08) |
| Colour, close others | At once | answers |
| Shell in its folder (Duplicate tab until landing 2) | The Starting row in the next frame, as for a new tab | answers (2026-11-04) |

## The Conductor's view (since 2026-10-24, lbterm/TAB-PANEL.md)

| Action | What answers today | Status |
|---|---|---|
| A left click on the Conductor's pad, `seat.show`, `LbtermConductorView` | The view the next frame, the open tab's ring and bar on the pad (gliding there with the compositor) | answers; gap: the view itself comes and goes at once, where the strip card's fade (`pane_enter`, `pane_exit`) would suit it |
| A click on a tab's row, a tab opened otherwise, Esc with the box empty | The view goes the next frame, the tab shows; a tab opened from the view is there at once, with nothing of the tab that was under it fading out over it, since it never showed (the strip's words still dissolve) | answers |
| Esc with words in the box | Nothing: the words stay | gap (minor): the box's hairline brightens once, as a key with nowhere to go |
| The pointer on a button, an option, a session's card | Its fill lightens at once | answers; planned: on `hover` |
| A press on a button | The press's ripple where it landed, and the button says "…" until the seat replies | answers |
| Typing, editing, a paste, IME | In the box the next frame; the box grows to eight lines | answers |
| Enter (Send) | The box empties at once; his words show once the seat echoes them (a local pipe, at once in practice); refused, they come back with the reason in orange under the box | answers |
| Enter or Send while the Conductor is off | "The Conductor is off: turn it on to talk to it" in orange under the box | answers |
| Allow, Deny, a control, a rating | "…" on the button until the reply; the card or the header changes when the next `seat.view` says so | answers |
| A picker's option (one question, one pick) | Sent at once, "…" on the card's options until the reply; the card goes with the next view | answers |
| A chip on a question in text | Its code in the card's box at once | answers |
| A session's card | Its tab opens, the view going | answers; gap: a session in another window or none says so in the notice, nothing more |
| A drag over the conversation | The selection at once; let go, it's copied with the copy flash | answers |
| The wheel, PageUp and PageDown | The conversation or the side column scrolls the next frame; "↓ Newest" shows while scrolled up | answers; gap: no motion, a jump a notch at a time |
| The reply being written | Grows as `seat.delta` comes, a caret after it | answers |
| The seat going | "the seat isn't running" in orange in the header, its controls gone | answers |

## Events

| Event | What shows today | Status |
|---|---|---|
| A tab's state, words or label changing | Its row dissolves from the old look into the new on `swap`, the next frame | answers (2026-10-21) |
| A program's progress | Its bar on the open panel glides to the new length on `pane_layout` | answers (2026-10-21) |
| A program typing into a tab through `lbterm cli` (the Conductor, Relay, the Claude project) | That tab's pad glints (`copy_in`, `copy_out`) | answers (2026-10-21) |
| Claude blocked on you | The pad dissolves into orange with its word on `swap` | planned: `alert`, a pulse and then a glow until you look |
| ...while LBTerm isn't the foreground window | No taskbar flash; a toast only when Claude Code sends OSC 99 | gap: the taskbar button flashes once |
| Claude done in a tab you're not on | A white unread dot on the pad | planned: `alert` |
| Output in a tab you're not on | Nothing (`has_unseen_output` exists, the panel doesn't use it) | planned: a soft glow until you look |
| A bell in a tab you're not on | Nothing on the panel | gap: that tab's pad glows, as for output |
| A program waiting on its own tasks | The arc turning on its pad, in DWM | answers |
| Resting on a row whose session has tasks running | The row's highlight at once; after 300 ms a card beside it listing each task, a subagent's latest tool call and tokens, fading in on `pane_enter` and out on `pane_exit` 200 ms after the pointer leaves the row and the card | answers (2026-10-24) |
| A newer build installed | The restart pill appears as version.txt is written (until 2026-10-30, within 10 s), at once | gap (minor): the pill fades in on `pane_enter` |
| An InMail message arriving or archived | Within a second the button's count dissolves into the new one on `swap`, and more mail brightens the button once | answers (2026-10-23) |
| An InMail message archived, with a link to it on screen | In the same frame as the count, the link goes from the waiting orange to grey, struck through | gap (minor): it changes at once rather than dissolving |
| The Conductor asking to do something that waits on him | Its pad turns orange the next frame, the ask's card on its view | answers (2026-10-24); gap: no pulse, as for a tab blocked on him (`alert`) |

## The gaps by how often they'd be felt

On a keyboard-heavy day with 8 or more Claude tabs:

1. ~~New tabs: Ctrl+T shows nothing until the server answers (a closed tab's row now fades where it was, and a new one grows in once it exists).~~ Built 2026-11-04 (lbterm/NEW-TAB.md, landing 1): the Starting row the frame after any tab is asked for.
2. Keys with nowhere to go: Alt+Up and Alt+Down in a tab with one pane (most Claude tabs), and the tab keys with one tab.
3. Attention from elsewhere: a bell in another tab, Claude blocked while LBTerm is behind another window.
4. Modes with the tabs hidden: zoom and copy mode show nowhere.
5. The prompt jump: no sign it's working, and a second click is lost.
6. ~~Menus: Enter with nothing picked.~~ Built 2026-11-05 (NEW-TAB.md, landing 2): the first row lights, and a key with nowhere to go brightens the hairline.
7. Restarting: the window goes with nothing bridging to the new one.
8. ~~Dragging a tab out: nothing shows where it will land.~~ Built 2026-10-21: the ghost card and the other window's slot.
9. The smart-paste probe (Terminal's config).
