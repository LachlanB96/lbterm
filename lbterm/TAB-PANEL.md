# Tab panel

A card down the left of the window, the colour of the panes and ringed with a grey hairline, holding a Claude Code status pad for each tab, with a label under it saying what the tab is for, the open tab ringed in red with a red bar beside it down the rail's edge. The moment the pointer reaches it, its ring starts to light. Rest the pointer on it and it opens into a list of the tabs (title, project, status, how long since Claude stopped), its ring lights up red-orange and casts light onto the panes, a light runs once round the ring, and the panes' cards press over to make room, each card's own outline shrinking with it. With the panel listing the tabs, the tab bar keeps only the window's buttons. Click a row (or a pad on the closed rail) to switch to that tab. It's LBTerm's own take on Terminal's TabPanel.exe, which stays as WezTerm's panel.

The look is "edge light", the fourth of four designs Lachlan was shown on 2026-10-20 (red outline, Material 3 Expressive shape morph, cards with gutters, edge light), picked for its modern feel and the way the line glows when the panel opens, casting a little light onto the black pane. Its shape follows LBTerm's design system (`lbterm/DESIGN.md`): a hairline ring, 12 px corners, and the 4 px gutter between it, the window's edges, the tab bar and the panes' cards.

## Turning it on

```lua
config.lbterm_tab_panel = {
  enabled = true,
  rail_width = '51pt',        -- the closed panel, ring included (68 px at 96 DPI)
  width = '217.5pt',          -- open (290 px at 96 DPI)
  open_delay_ms = 150,        -- the pointer rests this long before it opens (its ring lights at once)
  close_delay_ms = 300,       -- and it waits this long after the pointer leaves (its light dims at once)
  idle_full_minutes = 60,     -- the idle timer goes all red after this, like TabPanel.exe's idleFullMin
  labels = true,              -- a label under each pad: the tab's name, else its Claude session's
                              -- label from lbterm_summaries, else its title
  label_lines = 2,            -- lines a label may take (1 to 3)
  label_font_size = 7.0,      -- points, in the window frame's font
  tab_bar = false,            -- the tabs in the tab bar too; off, the bar is a slim strip the panes'
                              -- colour holding only the window's buttons, to drag the window by
}
```

The motion is `lbterm_motion.panel_open` (`spring(220ms, 0)`, the edge and the cards it presses over), `panel_close` (`150ms cubic-bezier(0.23, 1, 0.32, 1)`), `panel_glow` (`spring(400ms, 0)`, the ring's light as the edge moves), `hover` (`spring(250ms, 0)`, the ring's light answering the pointer while the edge waits) and `panel_sweep` (`1400ms cubic-bezier(0.33, 0.33, 0.4, 1)`, the light running round the ring); the words fade on `pane_enter` and `pane_exit`. All of it follows `lbterm_motion.enabled` and `slowdown`. The tokens and why they are what they are: the `lbterm-motion` skill.

## The delays hold back the edge, not the answer

Every action answers at once (`lbterm/DESIGN.md`, principle 1; Lachlan's picks 1a to 4a, 2026-10-21, after the panel took 150 ms to show anything, played its open to the end when he'd already left, and showed nothing when he left and came straight back). `lbterm_panel::Hover` keeps the timing and says how lit the ring is (`Light`):

| The pointer | The edge | The light |
|---|---|---|
| reaches the closed rail | waits `open_delay_ms`, then opens | pending at once, on `hover`; the open carries it on to full on `panel_glow` |
| passes over the rail and off | stays shut | up and back, on `hover` |
| leaves the open panel | waits `close_delay_ms`, then closes | pending at once, on `hover` |
| comes back within the grace | stays open | full again, on `hover` |
| leaves while the edge is still opening (`panel_open`'s 220 ms) | turns round at once, from where it is | back to rest with the edge, on `panel_glow` |
| comes back onto the rail or what's left of the panel while the edge is still closing (`panel_close`'s 150 ms) | opens again at once, from where it is | full with the edge |
| clicks a pane the panel presses over | shuts at once; coming back waits on the rail as usual | to rest |

Pending is 35% of the open light and glow (`palette::PENDING`): the red starting to show in the grey ring, faintly on the panes. The light's level rides in the plan (`PanelPlan::light`), and `PanelLayers::show` moves the light alone when the edge doesn't move, so the edge's motion is never restarted. How long the edge takes on screen comes from the motion settings (`Hover::set_motion`); with motion off or no compositor there's no turning round, and the light is drawn at its level. The window strip's detail card and prompt tooltip use the same `Hover`: coming back while the card fades out (`pane_exit`) opens it again at once, but leaving while it fades in waits out its grace, since the pointer crosses the prompt row on its way down to it.

## What a row says

Nothing is polled and no file is read on LBTerm's GUI thread:

- **What the program says**, first of all. Claude Code (2.1.295 on) reports its own state with OSC 7501, the Program Status Protocol, to a terminal that answers its query, and LBTerm answers (below). The newest report from the program itself (no `id`) sets the row's pad and words: **Working**; blocked, an orange pad with **Permission**, **Question** or **Login** from its `kind`, or **Blocked**; **Done**, with a white dot on the pad while the result is unread (it arrived after the tab was last on screen); **Your turn**; or **Failed** on a red pad. When it sends a `msg` that fits whole, the message takes the status word's place, in the same colour; cut short it would say less than the word. Claude Code's are often longer than the open panel's room ("Sleeping for 4 seconds", "answer: Do you prefer red or blue? (Red · Blue)"). Its `title` replaces the tab title, and its `progress` draws a thin bar along the foot of the row while it works or waits. Its parts, Claude Code's subagents and background tasks (records with an `id`), show as "2 agents" where the age goes while it works. The age counts from the report. Watched with a real Claude Code 2.1.304 session on 2026-10-20: idle, working, working with a message, blocked on a question, done.
- **State**, when a pane has no reports, from the `claude_status` user var Terminal's claudehook.exe sets in the pane (`006400` working, `ff8c00` blocked on you, `default` stopped, each with the FILETIME the hook ran; the newest in the tab wins, as in Terminal's configs), and from the glyph Claude Code puts before its title (a braille spinner while it works, ✳ while it waits). The light wins; the glyph covers a pane without hooks and tells a finished session from an exited one.
- **Title**: the tab's own title, else the active pane's without Claude's glyph, else the program's name.
- **Waiting on its own tasks**: a program that has handed the turn back (done or idle) while parts of it still work gets a green arc turning on its pad's corner, and its row says **Waiting on N tasks** in green: it will carry on by itself when they finish, so it isn't stopped (Lachlan's pick, 2026-10-21, over dots, a ring, a colour of its own and an hourglass). Claude Code doesn't report its background tasks itself: watched on 2026-10-21, its own state went to `done` and stayed there for the 38 s a background command ran. So Terminal's statusline counts them from the transcript (a start's "running in background with ID", a `<task-notification>` or a stop ending it) and reports each as an OSC 7501 `working` record under `bg/<id>` through `claudehook tasks`, clearing it when it ends. Without the compositor the arc is drawn still.
- **Label**, under the pad on the closed rail, so the tabs can be told apart without opening the panel: the name the tab was given (Rename in the tab menu, `set-tab-title`), else the label Terminal's summary worker wrote for its Claude session, else the title. The worker (Terminal's `shared/claude/summary-worker.ts`) asks Haiku for a word or three on what the session is for, mostly from its first request, on the session's first Stop and every fourth after, and writes it to `<session>.label.txt` in the folder `lbterm_summaries` names; the session is the active pane's `claude_session` user var. LBTerm reads those files on a thread of its own and repaints when one changes (`wezterm-gui/src/summaries.rs`). Two lines of up to seven characters on the 68 px rail, broken between words, a word too long cut with an ellipsis (`text::wrap`); a wider `rail_width` fits more. Lachlan picked labels under the pads on the rail as it is (2026-10-20) over a wider rail with the title beside each pad.
- **Project**: from the folder the tab's Claude session is in now, which Terminal's statusline keeps in `<session>.cwd.txt` in the `lbterm_summaries` folder; else from the active pane's working directory, as TabPanel.exe names it (the folder under `Life\Projects`, a worktree as `repo · tree`). Without OSC 7 from the shell, WezTerm finds a directory by walking the pane's processes, so it's looked up on a thread of its own, only while the panel is open, at most every 15 s. The pane's folder alone named most of Lachlan's Claude tabs "Projects" (2026-10-21): a tab opens in `Life\Projects` and its shell says so, then Claude is started in, or moves to, a project the shell never reports.
- **Age and idle timer**: since the status last changed, for blocked and finished sessions; three bars above the pad, green, then yellow, then red over `idle_full_minutes`. The panel repaints when an age or timer next changes, not on a clock.

The logic is the `lbterm-panel` crate (`cargo test -p lbterm-panel`): rows, the hover timing, the layout and where every word goes.

## How it's built

**Room for it.** The window keeps a strip down its left (gutter, rail) as though it were border (`render/borders.rs`), so every calculation that places panes, splits and the mouse leaves room for it, and the first card sits a gutter past it, as cards do from every edge of their room (`render/card.rs`). The fancy tab bar takes the strip back and runs the full width above the panel (`render/fancy_tab_bar.rs`). The strip is filled with the panes' colour, in `over`. Upstream's `window_content_alignment = 'Center'` (which Lachlan's `lbterm.lua` sets) worked out the leftover pixels without taking the border off, which shifted the panes' text right by half the strip and put their last columns off the window; `padding_left_top` takes it off now (`render/mod.rs`).

**Drawing.** The panel has z-layers of its own: 50 for the light, 51 for the grey hairline ring, the card inside it, the pads and their numbers, 56 for the ring lit (a red-coral-pink gradient down its sides, over the grey one while open), and 52 for what only the open panel shows (titles, ages, projects, statuses, and each row's long highlight, which starts just past the pad column so the pad shows through, the pill under it covering the rest in the same colour). Modals stay over it; the tab bar goes under it.

**The light** took three goes, and the reasons matter for anything faint drawn into a visual:

- Stacked rounded rectangles of even, faint alpha read as a flat band with a hard edge. The visual's surface is premultiplied and blended in linear light, and DWM lays it over the panes with its stored, gamma-encoded values, so over a dark pane a faint light looks far stronger than its alpha. The alpha at each distance is worked back from what should be seen instead (`palette::glow_alpha`: a Gaussian from 30% of the way to the light's colour at the ring to nothing 26 px out).
- Stacking still lost the outer light: each layer's contribution rounded to zero in the 8-bit surface before the next arrived. So every pixel is drawn once: bands along the sides and fans of segments round the corners, each quad with its own colour at each corner (`QuadImpl::set_corners`, LBTerm's one addition to `quad.rs`).
- The GPU rounds a pixel's alpha to 1/255 before blending, and near black each step showed as a ring. So a band keeps one alpha, rounded up to a step, and the falloff rides in its colour, which isn't rounded.

The tab bar's drawn height is what the panel sits under (the fancy bar can draw a couple of pixels taller than it says it is).

**Motion, with the compositor.** The panel is always drawn open, and DWM shows as much of it as the moment calls for, so the motion runs whatever LBTerm's GUI thread is doing (`compositor.rs`, `PanelLayers`):

| Visual | Draws | Opening |
|---|---|---|
| body group: glow, body, lit, words | z-layers 50, 51, 56, 52 | the group's clip moves right with the edge; the glow and the lit ring fade in on `panel_glow`, from wherever the pending light had them |
| cap group: glow, cap, lit | z-layers 50, 51, 56, seen at the ring's right end | the group moves right with the edge |
| spinners: one per waiting pad, in the body group | z-layer 57, the arc drawn once round the first waiting pad's corner into a surface never shown itself; each pad's visual shows its swapchain, offset to that pad and turned about its middle by a rotate transform on a repeating animation (a turn every 1.4 s), so DWM spins it; held still with motion off. The disc under the arc is drawn on the pad in the colour under it, since it looks the same at every angle | the arc turns from the moment its pad starts waiting until it stops |
| light group: halo, middle | z-layers 54, 55, drawn round where the light starts | carried once round the ring's middle line on `panel_sweep`, fading in as it sets off and out as it gets back |
| rows: one per visible tab, in a group over the body | z-layer 46 drawn into one of two surfaces never shown themselves; each row's visual shows its slot of the front one (`Layout::slot`: the row's whole height across the open panel) | see below |
| row words: one per visible tab, in a group in the words' visual | z-layer 48, the row's open part (its long highlight and the ring round it, the pad drawn again on it, its words, chips and progress track), drawn and shown the same way; a second visual per row that moves with the first and fades with the words. Each progress bar rides in its row's | see below |
| marker | z-layer 47: the open tab's ring and bar, in a visual of their own over the rows | see below |
| progress: one per bar | z-layer 45: every row's progress fill drawn whole into a surface never shown itself; each bar's visual shows its own fill, clipped to how far along it is | see below |

The ring's two right-hand corners (the cap) ride on the edge while the body, clipped at the seam where those corners begin, grows behind them, so the ring is whole and rounded at every width. Both move on the same curve in the same commit. The two glow visuals brighten from 45% to full on `panel_glow`, and the words fade in on `pane_enter` after 60 ms, so the edge goes first.

**The panes make room without resizing.** While the panel is open, each pane's visual shows pressed into the room left of it: everything from the first card's visual to the last's mapped onto what's left once the edge has moved (`PixelRect::squeeze`), so the first card lands a gutter past the open panel and the cards stay a gutter apart. Only the visuals' offsets and clips move, on the panel's curve; the panes' content keeps its size, anchored at its top-left, so the right of each pane is cut off while the panel is open and nothing is squashed. Each card's outline is nine pieces of two template cards riding on its visual's clip (`lbterm/COMPOSITOR.md`), so the cards stay whole and rounded as they narrow, their text cut off at their own right-hand edge. No PTY is resized, so a hover never makes Claude Code redraw. A click on a pane while it's pressed over would land where the pane really is, not where it shows, so it closes the panel at once and goes no further.

**The running light** came from Lachlan asking for "a negative animation for the main pane": as the ring lights up, a light runs round the ring, brighter where it is. (With it came a grey frame round the panes that shrank with them; the design system retired it on 2026-10-20, and the panes' own card outlines shrink instead.) The light is two discs of falling-off colour (`tab_panel_disc`), a warm halo 64 px across and a near-white middle, drawn where the light starts (the top of the ring's top-left corner) and carried round by DComp: `lbterm_motion::path::along` samples `Layout::sweep_point` every 1/120 s along `panel_sweep` into straight cubics for the group's x and y offsets. It goes down the left side first, which never moves, and reaches the right side after the edge has settled there. It runs only when the panel opens, and fades out at once if the panel closes on it.

**The open tab.** Its pill on the rail has a ring of the accent red round it, a hairline drawn by `lbterm_stroke`, and a short bar of the same red runs down the rail's left edge beside it, inside the panel's ring. Lachlan picked the two together (2026-10-20) when the faint red pill alone was too hard to see, over a stronger red pill and a white ring. With labels on, every pill is as wide as the closed rail lets it be, 42 px on the 68 px rail: nothing per tab may show right of the seam, where the ring's right-hand corners (the cap) begin. Open, a highlighted row's pill grows into its long highlight, drawn whole in the words layer, and the ring goes round all of it; the row's pad, timer, number and label are drawn again on top, so nothing of the pill underneath shows through.

**The rows move on their own** (built 2026-10-21, `PanelLayers::sync_rows`, `sync_marker`, `sync_progress`), each from wherever it is when a new change comes, so nothing restarts or jumps:

- A row is followed by its tab, not its place. A row whose slot changes (a tab closed above it, moved, dragged past) slides there on `pane_layout`. A new tab's row grows in: its clip opens from the top on `pane_layout` as the rows below make room, and it fades in on `pane_enter`. A closed tab's row keeps the old surface's picture and fades where it was on `pane_exit`.
- Each row's look is a hash of what it shows (its words, state, label, colour, whether it's the open tab, but not how far its progress is). When it changes the old picture is kept: the rows draw into one surface and show the other, and a row that changed gets a ghost visual showing the old surface, faded out over the new on `swap` (on `hover` when only its hover changed). The two surfaces swap each time any row changes.
- The marker (the open tab's ring and bar) glides from the old row to the new on `focus_move`; when its row moves, it moves with it on `pane_layout`.
- A progress bar's visual is clipped to how far along it is, and the clip's right edge glides on `pane_layout`.
- A row being dragged along the panel (`tab_mouse.rs`) is lifted out of the order: it's drawn with a soft shadow, its visual moved over the others, and it sits under the pointer at once on every move, while the rows it passes slide out of its way. Let go, it slides into its slot.
- A pad glints, the copy flash's white (`copy_in`, `copy_out`) round it, when a program types into its tab through `lbterm cli` and when the tab is renamed (`lbterm_panel_glint`). The session handler (the mux server's, or a window's own socket's) raises the `lbterm_typed` user var on the pane when the connection typing never said who it is: a window sends `SetClientId` as it attaches and the cli never does (`mux::lbterm_typed`), so the Conductor's, Relay's and AI Dashboard's typing shows and a GUI's own doesn't. It's a notification, not a stored var: nothing in Lua sees it.
- The window resizing, or motion off, puts every row, the marker and the bars where they belong at once.

**One outline.** Lachlan asked (2026-10-20) for the outline to be one solid colour at rest and to light up only when the panel is in use. The ring is a grey hairline (`palette::OUTLINE`) at rest, drawn by the routine every outline uses (`lbterm_stroke` in `render/card.rs`: a stroke soft a pixel either side, its colour free to change down its length), with the design system's 12 px corners. Opening fades the gradient ring and its glow in over the grey one on `panel_glow`. Until the design system it was a 3 px stroke with 18 px corners and up to 10 px of gap, paired with the grey frame round the panes, which needed Lachlan's `lbterm.lua` to pad the panes 12pt.

**The tab bar steps aside.** With the panel on, the fancy tab bar draws no tabs, no new-tab button and no status, only the window's buttons, on a strip the panes' colour that the window is dragged by (`fancy_tab_bar.rs`, `config::lbterm::hides_tabs`). `lbterm_tab_panel.tab_bar = true` brings them back. The retro tab bar doesn't know the setting.

**Without the compositor** (`lbterm_compositor = false`, or OpenGL), the panel is drawn at its size of the moment, over the panes, with no motion and no running light (DWM carries it).

## OSC 7501

The Program Status Protocol (Mitchell Hashimoto's; spec at superlogical.com/rex/docs/build/program-status) lets a program tell the terminal what it's doing. LBTerm speaks it in every pane, whatever the config, because a program only reports once a terminal has answered its query:

- `wezterm-escape-parser/src/program_status.rs` parses a report or the query `ESC ] 7501 ; ? ST`, with every limit in the spec: keys and values from their character sets, a bad pair skipped, a bad `state` or `id` dropping the report, an oversized sequence or bad base64 dropping all of it.
- `term/src/program_status.rs` keeps up to 256 records per pane, by `id`, evicting the least recently updated. `state=clear` removes a record and its children; OSC 133 A (a new prompt, or N, which starts one too) drops the working and blocked ones, so a crashed program doesn't stay "Working"; RIS drops them all. `app` is inherited from the nearest record above.
- `performer.rs` answers the query with `ESC ] 7501 ; ? ESC \`, which comes back through ConPTY's input intact (checked 2026-10-20), and raises `Alert::ProgramStatus` on every change. The GUI repaints the panel on it.
- `pane:get_program_status()` in Lua returns the records as tables (`id`, `state`, `kind`, `progress`, `app`, `title`, `msg`, `updated` in Unix seconds), so Terminal's `lbterm.lua` can use them too.

`cargo test -p wezterm-escape-parser program_status` and `cargo test -p wezterm-term program_status` cover the parse and the records.

## The Conductor

The Conductor (Lachlan's `Life\Projects\Conductor`) is one Claude Code session that watches and directs the others. LBTerm shows it on the rail (picked 2026-10-21; the look is `lbterm/DESIGN.md`'s, under "The tab panel"):

```lua
rawset(config, 'lbterm_conductor', {
  state_file = [[...\ObsidianDash\conductor\button.json]],  -- { "state": "off"|"on"|"remote", "mode": "read"|"most"|"all", "sessionId": ..., "tooltip": ... }
  hub_pipe = [[\\.\pipe\conductor-hub]],                     -- unset: no outlines on the tabs
  glyph = '\u{f075a}',                                       -- on its pad; a note by default
  remote_glyph = '\u{f0003}',                                -- on the pad's corner while state is "remote"
  menu = {
    { label = 'Accept most', when = { state = { 'on', 'remote' } }, checked = { mode = 'most' },
      wait = true, action = wezterm.action_callback(function(window, pane) ... end) },
  },
})
```

- **Its pad** is a slot above the tabs (`Layout::head`), a tab's height, with a hairline rule under it; the tabs go down a slot. Its state file is read on a thread of its own when its size or time changes (`state_files.rs`, every 250 ms), and no file is off. On the open panel its row says its label (else "Conductor") over the file's `tooltip`. Read only, its outline is grey, and the spectrum while the pointer is on the pad (`conductor::spectrum`). A right click on it drops its menu from where it was clicked, and so does a left click while there's no tab of its to open (`render/conductor.rs`): the items whose `when` the file matches (a field the file hasn't got is `''`, but `state` is `off`), ticked where `checked` does. An item with `wait = true` keeps the menu open once picked, its arc turning where its tick goes and the others dimmed and out of reach, until the state file says anything new or 45 s pass (`conductor::answered`). A new `updatedAt` counts, so a rewrite in the same words is an answer too (Conductor's `off` when it's off already). A ticked item doesn't wait (`conductor::waits`): it asks for what's so already, the mode the Conductor is in, and Conductor writes nothing for that; Esc or a click away stops waiting. The arc is drawn into a template (z-layer 76) that a visual in the compositor's `top`, over the menu, shows turning a turn every 1.4 s (`Compositor::prepare_menu_spinner`); without the compositor it's drawn over the menu, still.
- **Its tab is the pad's** (Lachlan, 2026-10-21: the Conductor takes no tab space). While the file's `state` isn't off, its `sessionId` names the Conductor's own Claude session (`conductor::session`), and the first tab of the window with a pane whose `claude_session` user var is that session is its tab (`conductor::tab_of`). That tab has no row: the rail's rows are the window's other tabs, in order (`conductor::rail`, `TermWindow::tab_panel_rail`), and everything that turns a row into a tab goes through that list (a click, the tab menu, a drag, the rows' visuals, the glints). It's kept first in the window (`conductor::keep_first`; mux's `Window::lbterm_move_tab` moves it, the open tab staying open with no focus change sent), so Alt+Left from the first tab on the rail goes up into it, as the pad sits above that tab, and a drag along the rail can't put a tab above it. A left click on the pad opens it, and while it's the open tab the open tab's ring and bar are on the pad (`Layout::head_slot`, `head_edge_bar`) and its pill has the open tab's fill. A program typing into it glints the pad. The tab menu's "Close other tabs" leaves it alone. Conductor's `off` ends the session; its tab is found again whenever the file names a session a tab holds.
- **The tabs it's on** come from its hub (`conductor_hub.rs`), over the named pipe. LBTerm keeps a connection of its own open, the hub's lines and LBTerm's one JSON object a line:
  - LBTerm, first: `{"type":"lbterm.subscribe","pid":…,"version":…,"terminal":"lbterm"}` (`lbterm-try` in a try window), then the panes line below.
  - The hub, whenever the list changes, all of it each time: `{"type":"conductor.focus","sessions":[{"sessionId":"…","doing":"thinking"|"acting","until":<epoch ms>}]}`. A tab whose panes hold a session in the list gets the outline (acting outranks thinking). An outline shows at least a lap (1.4 s acting, 0.6 s thinking) and lingers 0.4 s after the hub lets go. `until` is optional: past it the outline goes even while the hub still names it, so a process that dies mid-action, or an action whose end nobody sees (Claude Code's SendMessage), can't leave a pad lit; naming it again with a later one keeps it going. Anything else in an entry (Conductor's action names) is left alone. When the connection closes they all go, and LBTerm tries again after 2 s, doubling to 30 s while the hub hangs up at once.
  - LBTerm, whenever it changes, on a connection of its own as claudehook's relay sends hook events: `{"type":"lbterm.panes","terminal":"lbterm","panes":[{"sessionId":"…","pane":17,"tab":3,"active":false}]}`. `pane` is the mux server's id, which `lbterm cli` takes; `tab` counts from 1 as the rail does, and the Conductor's own tab is 0. Nothing is sent without `hub_pipe`.
- **The outlines, with the compositor**, are drawn once a frame as templates into surfaces never seen themselves, round one of the pills (they're all one size): the spectrum's (z-layer 53), the iridescent (58) and the light's disc (59). Each pad that shows one has a visual of its own, offset to its pill, in a group over the body and the cap that the seam doesn't clip (an outline's glow reaches past it, and the pads never move), showing the template's swapchain with a DComp hue rotation whose angle DWM turns once round every 6 s; a tab acted on has a second visual, the disc, carried round the pill's outline lap after lap (`Compositor::lap`, 90 points a lap), and a tab thought about the same at 45% every 3 s. An outline kept from one frame to the next keeps its visuals, so its colours and light carry on where they were. Motion off: still. Without the compositor the outlines are drawn still in the body, and there's no light.

## Checking it

- `cargo test -p lbterm-panel` and `cargo test -p lbterm-motion` (the squeeze), both in `lbterm.ps1 check`.
- `pwsh -File lbterm\perf\conductor.ps1`: the Conductor on the rail against a hub the script plays on a pipe of its own: LBTerm subscribes and says where two Claude sessions are; the pad off, read only and accepting most, its hues turning; a tab thought about and one acted on, their outlines moving, gone once the hub lets go; the menu run by a right click and by the keys, closed by Esc, and offering to turn on to a left click while off; the Conductor's own tab moved first and off the rail (tab 0 to the hub), the read-only spectrum under the pointer, a left click on the pad opening its tab with the open tab's marker, a click on the third row opening the third row's tab.
- `pwsh -File lbterm\perf\rail-labels.ps1`: five tabs, three with Claude sessions and label files in a scratch `lbterm_summaries` folder, one renamed, one plain. The open tab must have its bar and ring closed, the rail must change when a label file does, and open, the ring must run along the open tab's row (`target\perf\rail-labels`).
- `pwsh -File lbterm\perf\rail-feedback.ps1 [-Slowdown 4]`: the rows' motion, captured frame by frame. The marker glides between rows and never past them, a new state and a new name dissolve in, a pad typed into by `lbterm cli send-text` and a renamed one glint and leave nothing behind, a progress bar glides (every pixel the old bar or the new on the way, not a blend), a tab grows in and another folds away, a dragged row is lifted under the pointer and lands where it's let go (`target\perf\rail-feedback`).
- `pwsh -File lbterm\motion\panel-demo.ps1` opens the build with six stand-in tabs (`demo-tab.ps1`) in different states and projects, plays the pointer with posted mouse moves (the real mouse never moves; `LBTERM_POSTED_POINTER` keeps it where the last move put it) and records it: frames, a contact sheet and `demo.gif` at real speed in `target\motion\panel-x1`. It plays the real delays: onto the rail (pending light, then the open), down a row, off and straight back within the grace (the dip), then off for good. `-Interrupt` leaves while it opens and comes back while it closes instead. `-Slowdown 5 -OpenOnly` measures the ring's right edge against `panel_open`. The stand-ins' titles are set with `lbterm cli set-tab-title`, because a title a script sets inside a pane doesn't get through ConPTY (in WezTerm either); Claude Code's own titles do.
- `LBTERM_DUMP_FRAMES=<dir>` saves each panel visual's texture too (`frame<n>-panel-glow.png` and so on): what was stored, before DWM lays it over the panes.

Measured 2026-10-20 at 96 DPI: the ring's edge follows `panel_open` with 0.5 px RMS error (2.9 px at worst) over 54 frames at `slowdown = 5`; the light runs from 89 at the ring to the pane's 18 over 26 px, smooth all the way and round the corners; without the compositor the panel draws at its size of the moment, ring and light included.

## Not done yet

- A key to open the panel, and the panel following keyboard focus.
- The orange "blocked on you" pad pulsing, as on the APC.
