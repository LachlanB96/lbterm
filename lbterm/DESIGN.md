# LBTerm design system

What LBTerm looks like and how it answers you, in one place. Lachlan set it on 2026-10-20 by choosing between options, building on the "edge light" tab panel he picked earlier that day. This document is the source: the code's constants (`lbterm-panel/src/palette.rs`, `layout.rs`, `config/src/lbterm.rs`) and the motion tokens in the `lbterm-motion` skill follow it, and a change starts here.

Status: agreed 2026-10-20 (margins 4 px everywhere, the grey frame retired, the closed ring a grey hairline). Steps 1 and 2 of the build order are built: the shape (2026-10-20) and focus (2026-10-21).

## Principles

1. **Every action answers at once.** Moving focus, switching tabs, clicking, copying, a bell, Claude needing you, the pointer arriving on something or leaving it: each gets a small response on screen in the next frame, so nothing happens silently. Once everything answers, the terminal feels fluid (Lachlan, 2026-10-21). A delay may hold back what it guards (the rest before the tab panel opens, the grace before it closes), never the answer: light shows the wait, and a pointer that changes its mind turns things round from where they are. A key with nowhere to go still says so. `lbterm/FEEDBACK.md` lists every action, what answers it, and the gaps still open.
2. **Light is the language.** Outlines are hairlines. Brightness says what's near the cursor, what has the keyboard, and what needs you, more than colour or thickness do.
3. **No wasted space.** A gap exists only if it has a job: it carries the light, it's where you resize, it's where a dragged tab lands.
4. **Fluid and soft.** Critically damped springs of about 250 to 400 ms. Things glide and settle; nothing overshoots or bounces.
5. **Animations compose.** The cursor's light, the panel opening, focus gliding and an alert all play at once without cancelling each other. Each property has one owner, and a new target starts from wherever it is now.
6. **Calm by default.** Neutral white-grey for everything the panes do. The red-orange accent belongs to the panel while it's in use. Status colours mean status and nothing else.
7. **Never in the way.** Motion never delays a keystroke's effect: the content changes at once and only light and opacity animate. Nothing LBTerm or an agent opens takes the keyboard (`lbterm/motion/start-unfocused.ps1`).

## Tokens

### Shape

| Token | Value | For |
|---|---|---|
| hairline | 1 px (DPI-scaled, never under 1), soft one pixel either side | every outline: panes, the panel ring, menus |
| radius | 12 px | panes, the panel, menus. Pills and pads inside keep their own |
| gutter | 4 px | between panes, and between the panes, the panel and the window's edges |
| reach | 120 px | how far from an edge the cursor starts to light it |

### Colour

All on the surface colour. Alphas are of white over the surface, as the eye sees them (drawn with the gamma-aware solver the panel's glow uses).

| Token | Value | For |
|---|---|---|
| surface | `#121212` | panes, their cards, the panel's card, the window strip |
| outline at rest | white, 6% | the active pane's outline when the cursor is elsewhere |
| outline on hover | white, 14% | a pane's outline while the cursor is over it |
| ring at rest | `#5E5E6E` | the tab panel's ring when closed |
| light | white, up to 50% | the light pooling on an edge near the cursor |
| accent | `#FF3B30` → `#FF8A5C` → `#FF2D6F`, glow `#FF3B30` | the panel's ring while open, its glow, its running light |
| status | green `#3DDC84` working, orange `#FF8A3D` waiting, blue `#5B7CFF` your turn, red `#FF5449` failed, dim `#24242C` none | the panel's pads only |
| press | white, 35% fading to 0 | the ripple from a click |
| inactive | 85% | text in panes without the keyboard |

### Motion

The existing tokens stay; these are added. All follow `lbterm_motion.enabled` and `slowdown`.

| Token | Value | For |
|---|---|---|
| `hover` | `spring(250ms, 0)` | a pane's outline fading in and out as the cursor arrives and leaves; a row's highlight; the tab panel's ring going to its pending light and back (built 2026-10-21) |
| `light_follow` | `spring(120ms, 0)` | the light gliding after the cursor along an edge, retargeted on every pointer move |
| `focus_move` | `spring(300ms, 0)` | the active outline gliding from the old pane to the new one |
| `tab_switch` | `200ms cubic-bezier(0.23, 1, 0.32, 1)` | the old tab's panes fading out as the new tab's fade in |
| `dim` | `spring(300ms, 0)` | panes going to 85% as they lose the keyboard, and back; the active outline going out as the window loses the keyboard (built 2026-10-21) |
| `arrive_in`, `arrive_out` | `80ms cubic-bezier(0.23, 1, 0.32, 1)`, then `spring(220ms, 0)` | the active outline's light rising to white 35% and settling back to rest as the keyboard arrives from outside the window (built 2026-10-21); 300 ms in all, since a key (the summon key, Alt+Tab) can start it |
| `press` | `320ms cubic-bezier(0.23, 1, 0.32, 1)` | a click's ripple growing and fading: a right-click in a pane (built 2026-10-21) |
| `alert` | `700ms` pulse, then held | a pad pulsing once when Claude needs you or finishes in a tab you're not on, then glowing until you look |
| `copy_in`, `copy_out` | `180ms cubic-bezier(0.23, 1, 0.32, 1)`, `260ms cubic-bezier(0.23, 1, 0.32, 1)` | a copied selection's soft flash, and an opened link's (built 2026-10-21) |
| `swap` | `200ms cubic-bezier(0.23, 1, 0.32, 1)` | words changing in place on the window strip: the strip as it was fading out over the new words (built 2026-10-21) |
| `bell` | `600ms` | a light pulse running along the pane's outline |
| `button_hover` | `120ms cubic-bezier(0.23, 1, 0.32, 1)` | the window buttons and the strip |

Existing: `pane_layout` `spring(220ms, 0)`, `pane_enter` `spring(160ms, 0)`, `pane_exit` `120ms` ease-out, `panel_open` `spring(220ms, 0)`, `panel_close` `150ms` ease-out, `panel_glow` `spring(400ms, 0)`, `panel_sweep` `1400ms cubic-bezier(0.33, 0.33, 0.4, 1)`.

## The pieces

### Panes

- Every pane is a card: the surface colour, 12 px corners, a 4 px gutter round it.
- The pane with the keyboard keeps a faint outline (6%). The others have none and their text sits at 85%; both change on `focus_move` and `dim` when focus moves, and the outline glides from the old pane to the new one.
- The outline says whether typing lands in this window (picked 2026-10-21): it goes out on `dim` when the window loses the keyboard, and comes back with a pulse when the keyboard arrives from outside the window: the window takes it back (the summon key, a toast's click, Alt+Tab, a click into it), or the pane with it changes with no key or click in the window to do it (AI Dashboard or Conductor activating a pane, a pane closing by itself). The light rises to white 35% on `arrive_in` and settles on `arrive_out`, opacity alone; a move a key or click made glides as before, without the pulse (`lbterm_extras::arrival`).
- With the cursor over a pane, its outline fades up to 14% (`hover`), and the light rule below applies.
- The gutter does three jobs. It's where the outlines and their light are. It's the resize handle: hovering it lights both edges beside it and shows the resize cursor, and dragging moves the split. And while a tab is being dragged it lights where dropping would split, as the new pane's outline.
- Across a side-by-side split the gutter takes most of WezTerm's one-cell divider, so the text there sits 1 to 2 px from the outline (9 px cells). Lachlan's call (2026-10-20): live with it for now; a wider divider would be a change to the mux, and those panes would lose a column.

### The scrollbar

Inside the card on the window's right, two pixels clear of its outline and two clear of the text (asked for 2026-10-21), its track running down the active pane's card clear of its rounded corners, so beside stacked panes it never crosses the gutter between them. The padding on the right is the thumb's, as in WezTerm; the card's gutter and outline, and the gap from the text, come on top of it while the scrollbar shows. Agreed 2026-10-20.

While the active pane's program has the whole screen (the alternate screen or mouse reporting on, as Claude Code's fullscreen mode has), the thumb and its prompt marks go, and they're back when the pane is at a shell again (`lbterm_hide_scroll_bar_in_apps`, on by default; Lachlan's call, 2026-10-21). Such a program scrolls its own view, so the window's viewport never moves and the thumb only led to the lines printed before it started. Its room stays: taking it back would resize the pane every time a program starts or ends.

### The light near the cursor

Within 120 px of an edge, light pools on that edge at the point nearest the cursor. It's brightest at the edge, fades as the cursor moves away, spreads about 60 px either way along the edge, and follows the curve round a corner. It glides after the cursor (`light_follow`). The same rule lights the panel's ring once it's open, in the accent colours, and a gutter's two edges together.

### The tab panel

- Closed: the ring a hairline at rest (`#5E5E6E`), no glow; the pads, with their timers, and under each a label of up to two short lines saying what the tab is for (7pt, dimmer than a title; the open tab's as bright as one). The open tab: a hairline ring of the accent round its pill and a short accent bar down the rail's left edge beside it (picked 2026-10-20, when the faint red pill alone was too hard to see). Open, the ring goes round the tab's whole row.
- Opening: the ring lights to the accent and its glow swells (`panel_glow`), the light runs once round it (`panel_sweep`), and then settles near the cursor and follows it like any edge.
- Waiting on the pointer: the ring comes up to a pending light (35% of its open light and glow) on `hover` the moment the pointer reaches the closed rail, and opening carries the same light on to full. Leaving the open panel drops it back to pending at once while the edge waits out its grace, and coming back brings it up again. Leaving while it still opens turns it round at once, and coming back while it closes opens it again at once (2026-10-21, Lachlan's picks 1a to 4a: the delays hold back the edge, never the answer).
- Rows: the hover highlight fades (`hover`); a click sends a ripple from where you clicked (`press`).
- Pads waiting on tasks: Claude has handed the turn back but its background tasks still run, so it will carry on by itself: a small green arc turns on the pad's corner, in DWM, and the row says "Waiting on N tasks" in green (2026-10-21).
- Pads: Claude needing you or finishing in a tab you're not on pulses once and glows until you look (`alert`); output in a tab you're not on glows softly until you look. The pad's tab-colour ring (from the tab menu) is drawn round the pad.
- While open, the panes press over, and their own card outlines are what shrink with them.
- The rail answers what happens to its tabs (built 2026-10-21). Each row is a visual of its own in DWM, so:
  - The open tab's ring and bar glide from the old row to the new one on `focus_move`, from wherever they are, and move with their row on `pane_layout` when it moves.
  - A new tab grows in: its slot opens from the top on `pane_layout` as the rows below make room, and it fades in on `pane_enter`. A closed tab's row fades where it was on `pane_exit` while the rows below slide up on `pane_layout`. Rows that change places slide too.
  - A row whose look changes (its state, words, label, colour, the open tab) dissolves from the old look into the new on `swap`: the old row is kept on a surface of its own and faded out over the new one. A change of hover alone fades on `hover`.
  - A program's progress glides along its bar on `pane_layout` (the bar's clip, in its own layer), and leaves the rest of the row alone.
  - A pad glints, the copy flash's light (`copy_in`, `copy_out`) round it, when a program types into its tab through `lbterm cli` (the Conductor, Relay, AI Dashboard: `mux::lbterm_typed`), so what an agent does shows where it happened; and when the tab is renamed.
  - A dragged row is lifted, a soft shadow under it, and follows the pointer, over the others, which slide out of its way. Past the tear-off distance a small card with the tab's name follows the pointer instead (its own layered popup: it never takes the keyboard and the mouse goes through it), and another LBTerm window under the pointer rings an empty slot after its last tab, in the accent, where the tab would land.
- The Conductor (picked 2026-10-21, over a pill on the window strip): its pad is the rail's top slot, a tab pad's shape, a hairline apart from the tabs below. Its glyph sits where a tab's number goes, and under it its mode, in the idle timer's colours: "read" green, "most" yellow, "all" red; off, glyph and "off" are dim. A small mark on the pad's corner while its remote control is on. Read only, a grey hairline round its pill, and the spectrum while the pointer is on it. Accepting most or all, so acting on its own, the spectrum: an outline of every hue, strong, with a soft glow, the colours running round it once every 6 s. The pad is the Conductor's tab (Lachlan, 2026-10-21): the tab holding its Claude session takes no row of its own, it's kept first in the window, so Alt+Left from the first tab goes up into it as the pad sits above that tab, and while it's the open tab the open tab's ring and bar are on the pad. A left click on the pad opens its tab; while it's off there's no tab, and a left click drops the menu. A right click drops its menu from where it was clicked: the tab menu's card and light, the items its state offers, a tick by the mode it's in. An item whose answer is the Conductor's new state (turning it on or off, a mode, remote control) keeps the menu open once picked, a small white arc turning where its tick goes and the other items dimmed, until the state comes back changed, at most 45 s (picked 2026-10-21); Esc stops waiting. The ticked mode picked again closes the menu at once: it's what the Conductor is in already, so there's no answer to wait for.
- Tabs the Conductor is on: while it thinks about a tab's Claude session, that tab's pill gets the iridescent outline, every hue pale as on foil, running round as the spectrum does, with a faint glint going round every 3 s. While it acts on it (a message, an answer, starting, resuming or stopping it), a bright light goes round instead, a lap every 1.4 s, the turning arc's pace. An outline shows at least a lap, and lingers a moment after the Conductor lets go. All of it in DWM (the hues by a hue rotation, the light along the outline); motion off, it holds still.

### The window strip

The panes' colour, only the window's buttons, dragged to move the window. The buttons fade on hover (`button_hover`), the close button's red included. Built 2026-10-20: the look under the pointer is drawn on a slot over the strip that DWM fades, two slots so one button fades out as the next fades in (`lbterm/COMPOSITOR.md`).

With `lbterm_summaries` set, the strip says what the active tab is doing (picked 2026-10-20 from four layouts, in place of Terminal's 1-row summary pane under each Claude session). Where the tabs would be, lined up with the panes' text: the tab's title, as the tab panel names it, at white 92%, then " · " at 35% and its Claude session's summary at 60%. By the buttons (left of the restart pill when that shows), when the summary was written and how long ago, "22:22 · 2m ago" at 50%, as the summary pane said it. All in the window frame's font, so the strip keeps its height. A tab with no Claude session shows its title alone. When room is short the caption goes first, then the summary, and a title longer than half the room is cut to half. The words stay part of the strip: they drag the window and a double-click maximises it.

The summary brightens to 85% the moment the pointer reaches the words, fading in on `button_hover` (2026-10-21), and resting there for 300 ms drops the session's detail down under them in a card: the surface colour, 12 px corners, a hairline at white 14%, a soft shadow so it reads over the pane, 12 px in, the labels ("Goal", "Finding", "State", "Next") at 50% lined up with the strip's words and the text at 85%, wrapped at 88 columns or the window's edge. The summary stays at 85% while it's open. It stays while the pointer is on the words or the card and closes 200 ms after it leaves both. Alt+S (Terminal's `expanded.flag`) pins it open in every window, as it expanded the summary pane. It sits over the panes and under the tab panel. It fades in on `pane_enter` as it opens and out on `pane_exit` as it closes, its opacity alone, in DWM, from wherever it is, so the pointer coming back as it fades turns it round at once, without waiting out the rest again (picked 2026-10-21). Motion off, it comes and goes at once.

With `lbterm_prompt_history` set (Claude Code's `history.jsonl`), the strip has a second row, the same in every tab so switching tabs never resizes the panes (picked 2026-10-21, layout 1c of three): an arrow in a small chip, white 7% filling to 18% under the pointer, then the Claude pane's last prompt on one line at white 80%, 100% under the pointer, in the frame's font, lined up with the title above it. A prompt cut short shows all of it in the strip's card when the pointer rests on it. A click on the prompt jumps Claude Code's conversation back to it; a click on the arrow drops a list of every prompt the session sent, newest first, each with when (the time today, the day this week, the date before that), in the tab menu's card and light, and a click on one, or the arrow keys and Enter, jumps to it. Esc or a click elsewhere closes it. Claude Code's fullscreen renderer keeps the conversation to itself, so a jump asks it to scroll with the keys it knows: Ctrl+End, then PgUp half a screen at a time, reading the screen after each, until the message is on its grey band, or in the header Claude Code keeps at the top, which it clicks once. The row it lands on is marked in the panel's coral for 1.6 s. A message from before a compaction can't be reached (Claude Code stops scrolling there), and a note says so in the prompt's place for 5 s. A tab with no Claude session leaves the row empty.

While a newer build is installed than the one running, a "↻ Restart to update" pill sits left of the buttons (picked 2026-10-20): a hairline with fully rounded ends, white 14% at rest and 35% under the pointer, its words at 75% and then 100%, its fill always the strip's. It's shorter than the buttons, so the strip keeps its height. A click on it, or Ctrl+Shift+F5, restarts into the installed build; the tabs stay. It fades on hover as the buttons do (`button_hover`).

The strip answers what happens to the window and to the words on it (built 2026-10-21):

- Words that change in the same tab dissolve: a new summary or its time, the last prompt, a notice coming or going, the badge. The strip as it was is drawn again over the rows that changed, on the strip's own colour, and fades out on `swap`; a tab switch swaps them at once. A new summary or prompt then brightens its row once, the copy flash's light (`copy_in`, `copy_out`).
- A notice takes the caption's place for 4 s, at white 85%, with or without a summary: "Config reloaded" when the config is saved; "Restarting…" in every window the moment a restart starts, the pill saying "↻ Restarting…" too, the window staying 150 ms to show it; and in the first window of the GUI that comes back, "Updated to <version> · 12 tabs back" ("Restarted" when the build is the same), or after a crash "Reattached 12 tabs", the tabs it found still running in the mux server.
- A zoomed tab wears a badge after its title, "Zoomed · 2 of 3", in a chip of the prompt arrow's shape: which of its panes shows, since the others are out of sight.

### Menus

The tab menu and anything like it: a card of the surface colour, 12 px corners, a hairline, the panel's accent light.

For later (Lachlan, 2026-10-21, items 26 and 27 of the feedback brainstorm; not built, and no session is on them as of that day): every menu (the tab menu, the prompt list, the Conductor's) opening with the strip card's fade, `pane_enter` in and `pane_exit` out from wherever it is, with the keyboard's highlight gliding between rows on `hover`; and upstream's overlays (the launcher, the command palette, confirm-close, search, quick select, the debug overlay), which still pop in WezTerm's style, redrawn as cards in this look with the same fade. The second is the bigger job.

### Other moments

- Switching tabs: the panes cross-fade (`tab_switch`) and the panel's ring and bar glide to the new row (`focus_move`, built 2026-10-21).
- Copying: what's copied flashes softly, white 22% at its brightest (`copy_in`, `copy_out`): a drag-select let go (WezTerm copies it), Ctrl+Shift+C, a quick-select pick (built 2026-10-21).
- Opening a link: the link flashes the same way, since what it opens is often on another screen. A plain click on a link opens it in a program that has the mouse too (Claude Code), as the pointer's hand says it will, and a click that wobbled half a cell is still a click on the link rather than a copy of one character (built 2026-10-21).
- A right-click in a pane: a ripple from where it landed, white 35% growing to 40 px as it fades (`press`). Terminal's `lbterm.lua` swallows a plain one so Claude Code doesn't paste, so before this it did nothing you could see (built 2026-10-21).
- The window's size: while its frame is dragged (or it's maximised or snapped), a card in the middle of the active pane says its size in cells, "120 × 32", there at once and fading on `pane_exit` 700 ms after the last change. The font getting bigger, smaller or back adds its size, "110% · 109 × 29", for 1.2 s (built 2026-10-21).
- Landing: a scrollbar mark's click and a search's next or previous match mark the row they land on in the panel's coral for 1.6 s, as a prompt jump does (built 2026-10-21; typing the search's pattern doesn't).
- The bell: a light pulse runs along the pane's outline (`bell`), no sound and no screen flash.

## Rules for building it

- Motion runs in DWM: only visuals' offsets, clips and opacity animate, through DirectComposition. LBTerm's GUI thread only sets targets, the cursor-following light included (one retarget per pointer move, never a per-frame loop).
- No ease-in, and no overshoot.
- Interruptible: a new target starts from the current value and keeps its velocity. Nothing queues, nothing restarts from the beginning.
- Frequent actions (focus, tab switch) take 300 ms or less and never hold back what the key did: the new pane has the keyboard at once, and the new tab's content is there at once, fading in.
- Motion off (`lbterm_motion.enabled`) means the end state at once.
- One owner per property: the hover outline, the active outline and the light are separate visuals, so they add up instead of fighting.

This replaces the `lbterm-motion` skill's rule that actions done 100 or more times a day (focus moves, tab switches) get no motion: Lachlan asked for them to glide and cross-fade. The skill has the new tokens and that change (2026-10-20).

## What changes from today

- Outlines go from 3 px to a hairline, corners from 18 px to 12.
- The gaps round the panel go from 8 to 10 px to the 4 px gutter, and panes become cards with gutters between them instead of sharing a split line.
- The separate grey frame round the panes while the panel is open gives way to the panes' own card outlines.
- `lbterm.lua`'s 12pt padding (room for the frame) comes back down; the card's padding is just room for the text.

## Build order

Each step on its own branch, with tests for the geometry and timing and a recorded demo for review:

1. Shape: hairlines, 12 px corners, the 4 px gutter, panes as cards, the frame retired.
2. Focus: the active outline and its glide, inactive panes at 85%.
3. Light: hover outlines, the light near the cursor on panes, gutters and the panel's ring (after its sweep).
4. Gutters: resizing by dragging, tab drop zones (with the tab drag's drop logic in `lbterm-extras/src/tabs.rs`).
5. Feedback: the click ripple, the tab cross-fade, pad alerts and background glow, the copy flash, the bell, the window buttons (built ahead of the rest, 2026-10-20). Built 2026-10-21: the ripple on a right-click in a pane, the copy and link flashes, the outline's keyboard light and arrival pulse, the size readout, landed marks; the strip's dissolves, notices and zoom badge; the rail's gliding marker, rows growing in, folding away and dissolving, progress gliding, glints, the lifted drag and its ghost.
