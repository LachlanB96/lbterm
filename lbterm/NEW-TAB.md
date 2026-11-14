# The new-tab pad, the picker and the menus

Lachlan, 2026-10-31: "There should be a button for new tab, and right clicking it should show a
context menu. The new tab should ask where you want to open the claude agent. The context menu will
show things like open a new shell. ... Right clicking a tab, should give you the option to hide the
tab from conductor too. Also come up with other context menu options."

Three designs (speed, motion and subtraction lenses) and a synthesis settled what to build. This file
is the synthesis: the build spec is what feat/new-tab builds. Status: **landing 1 landed on
2026-11-04** and is in Lachlan's installed build; landing 2 (the menus and the tab menu's items, with his 10a) was built on feat/new-tab-menus (2026-11-05, his go that day; an lbterm-track run in build slot-5, in six stages each sized to finish well under 250k of context, his 1a of 2026-11-05, a trial) and **landed on 2026-11-09** (4debc8950); landing 3, Hide from Conductor with his 6a's send light, was built on feat/conductor-hide from 2026-11-09 and **landed on 2026-11-09** (f6cbaa328) (an lbterm-track run in slot-5, his go of 2026-11-07), the Conductor's side having shipped on 2026-11-02 (section 8, "What the Conductor shipped"): its stage 1 reads button.json's `hidden`, its stage 2 the tab menu's row, the event and the pick's wait for the Conductor's word (section 8, "The event", "Pending" and "The row as built"), its stage 3 the look (the mark on the closed rail, the chip on the open panel) and the Conductor's view's folded Hidden group with its Show buttons (section 8, "The look" and "The Conductor's view", as built), its stage 4 his 6a's send light, a new token `send` (section 8, "The send light", as built; DESIGN.md's motion table), its stage 5 Terminal's `lbterm-conductor-hide` handler, on Terminal's own feat/conductor-hide beside the lbterm-motion skill's copy (section 9, as built), and its stage 6 measured it against main's build, the mark, the chip and the menu's row costing nothing at rest and nothing measurable on the tab menu's opening (PERF.md, "Hide from Conductor, by budgets -Against"), and ran the e2e scripts the branch picks (section 11, "Landing 3 as measured"). Its review (2026-11-09) found Terminal's handler starting one `conductor hide|show` a session side by side, which the Conductor's unlocked read-modify-write of its hidden.json can lose one of (PITFALLS 243), and the menu offering the reverse while a pick waited, which let two opposite requests race (244); its fix-up made the handler one call a pick with every id, asked the Conductor by InMail to take several ids in one call with one write and one result, and a lock (section 8, "What the Conductor shipped"), offered no row while a pick waits, and kept no light or glint while the panel is off. Until the Conductor takes several ids, a pick on a tab with two Claude sessions hides nothing and says so, and whether Terminal's branch lands before then is Lachlan's call. Still to be seen: the send light on the screen, every capture of it so far taken while the displays slept (section 8, "The send light"; PITFALLS 239). Landing order (PITFALLS 121): Terminal's feat/conductor-hide first, which is inert on the installed build (it never fires the event; `await_result`'s new `worked` is heard only by the new handler), then this branch, installed and the GUI restarted. The other way round this build's row is offered on every Claude tab with no handler to run it, so every pick runs out after 5 s with the strip's "The Conductor didn't hide <label>" and nothing hidden. The session that lands this branch lands both. In it every menu fades in and out and its highlight glides (section 6; render/menu_fade.rs and the compositor's `menus`; DESIGN.md, "Menus"), one falloff is every menu's light, each row's key shows dim at its right, a key no row takes flares the card's edge (since his 9a of 2026-11-08; it brightened the hairline before) and Enter with nothing lit lights the first row (`menu::answer`; perf/menu-keys.ps1), and the tab menu is section 7's (lbterm-extras's menu.rs, `Facts` gathered once as it opens; TAB-MOUSE.md, "The tab menu's model"): Rename from the label under the pad, Copy link (to the state file's `copied` in a test window, as every copy there is since stage 6, never the clipboard; PITFALLS 151) with the pad's glint, New session in <Project> firing `lbterm-project-action` with `source` 'tab menu', Shell in its folder, Wait on… (a page of the other Claude tabs in their pads' colours, dissolving in on `swap`, a pick firing `lbterm-tab-link` toggle), Close finished tabs (its second line naming them, every one still finished closed in one turn, kept to reopen), and his 10a, a middle click on a rail row closing that tab (perf/tab-mouse.ps1, steps 9 to 16; budgets.json's `tab-menu-opening`, the menu in the frame after the click). It needed no Lua: landing 1's lbterm.lua already handles `source` 'tab menu' and `lbterm-tab-link`. The review's fix-up (2026-11-05): Close finished tabs leaves a tab that waits on another through a tab link (PITFALLS 153), a middle click closes the tab its row was drawn for (156), the Conductor menu's turning arc fades with its card (155), the menus' light is kept rather than made each paint, the hairline's lamp looks up no swap chain on a quiet frame, and the marker's "landing at once" was traced: it stayed when the new open tab took the closed one's row, as Ctrl+W on main does (FLOWS.md, "The tab menu's facts", step 10). The follow-up track of 2026-11-07, after his answers that day (Lachlan's decisions, below): the tab menu's slow openings were the test window's timing to the screen spinning on a locked PC, main's code, fixed (FLOWS.md, "Opening, measured whole"); the marker rides up with the row that slides into a closed open tab's slot (3b, `lbterm_motion::rail`); a row picked in any menu flashes white as the picker's do (9b, `menu.<slot>.flash`); New session from the tab menu is a recent pick (8b); and an open menu has every key, a bound one doing nothing but flare its card's edge until the menu has closed, but for a shortcut the + menu shows beside a row, which picks it, and a paste key over the picker (7b, 5a, 11a; FLOWS.md, "A key while a menu is open"). Its last stage merged main's context pill, whose list drew its own card at the List slot's z-layers and took keys after the bindings: it's in the List slot now, fading as the menus do, with their light and hairline, and in the key gate (PITFALLS 175). Its third round (2026-11-08, after his answers that day, "Lachlan's decisions" below; six stages in slot-5) merged main again (perf/mux-client, then feat/write-failed-notes and chore/slot-prune, at 4eb88ca95), traced `missed` to the screen's refresh counter jumping at presents and counts it by vblank times since (8b; PITFALLS 166), and built the rest: the + menu's shortcuts pick their rows (5a), a paste key over the picker pastes into its box (11a), nothing of the rail plays under a card (7b), the agenda's presses flash (6), the highlight covers the Colour row (12) and a key no row takes flares the card's edge (9a), the rail kept open under a menu opened from it (10a, as built). Its review's fix-up (2026-11-08, evening) merged main again (feat/caption-pin-gap, docs/inmail-files and feat/inmail-taken, at 5bf86b777), bounded the picker box's cost by the box wherever the caret is (`newtab::box_span`, PITFALLS 209), put a menu's lamp in the tree only while its flare plays (COMPOSITOR.md, `menus`), and made the 7b checks judge every name the hold drives under both cards (PITFALLS 210); `missed` stayed unjudged, the right-most screen still 98% under the DashboardsBox and Discord at 19:25. Seen as it lands (FLOWS.md, "A menu, from what opens it", Seen on screen, "The round of 2026-11-08"), and measured as it lands against the installed main it merged (PERF.md, "The menus' fade, by `budgets -Against`", "And as it lands after its third round"): the five openings it touched cost what they did after the follow-up, the prompt list's state file frame and all, and `missed` is still unjudged, the right-most screen under the DashboardsBox and Discord at every round. Seen unlocked on 2026-11-07 (FLOWS.md, "A menu, from what opens it to the end of its fade", Seen on screen): the hairline's pulse (35% toward white on a one-pixel line, about 150 ms at real speed), which his 9a of 2026-11-08 found too faint: since then a key no row takes flares the card's edge, the hairline white and the light round the card white, above half for about 170 ms and a tenth for about 300 at real speed (Lachlan's decisions, 9a), the flash on a pick outliving its card by about 170 ms, Wait on…'s dissolve, the Conductor's arc fading with its card, the marker riding (3b), and the rail holding still under a card, open or closed, nothing of it crossing the card as it fades in or out. Measured once as it lands (PERF.md, "The menus' fade, by `budgets -Against`", "The branch as it lands", against the context pill's main, unlocked): the InMail list, the picker and the rail open as main's do in every metric the rounds measured but the list's fade (`missed` and the screen's times went unjudged, the test window covered, PITFALLS 182); the tab menu shows in the same frame and time, 1.2 ms more of the GUI thread a right click, of which 0.19 ms is its facts read as it opens and the rest the state file's frame only test windows paint; the prompt list 8.9 ms more, the most of it one more paint of the open list that only a test window makes (the state file's frame), then a first paint dearer by a millisecond or two, which his window pays once an opening, and about one shaping of the list not yet put in a paint (FLOWS.md, "Opening, measured whole"). Not in it: Hide from Conductor (row 6 of section 7, and section 8) is landing 3, after the Conductor ships hide and show; Interrupt stays out (his 11a); Park tab, the Conductor pad's outline, Shift with the side buttons and the quieter rail (his 7a, 8a, 9a, 12a) are landing 4, built on 2026-11-09 on feat/rail-landing-4 (an lbterm-track run in slot-5 in five stages; "Landing 4" below has its design, written before its code, what each stage built, what its last stage measured against main's build and the questions for Lachlan), with Terminal's half on Terminal's own feat/rail-landing-4, neither landed yet. Landing 1 was built from 2026-11-03 (Lachlan said to start
it then, as an lbterm-track run in build slot-3, his 1a of 2026-11-01), after it was parked on
2026-10-31 part way through, for usage. In it the pad, the picker, the + menu and the Starting row
are drawn and driven
(render/new_tab.rs, perf/new-tab.ps1), a tab closed by hand is kept and reopened (Ctrl+Shift+T's
action and the + menu's rows), Lua's spawn_tab takes `activate`, and budgets.json has the picker's
and the pick's actions (2026-11-04). The lbterm.lua part (section 9) is built on Terminal's own branch
feat/new-tab, tested there by lbterm.Tests.ps1 and here by perf/new-tab-lua.ps1, which runs his file
(or a copy) in a window of this build. It binds Ctrl+T, Ctrl+Alt+T and Ctrl+Shift+T and names the
recent picks' file and the root's other names only where `wezterm.has_action('LbtermNewTab')`, so
with a build from before this landing it loads exactly as before (every value the same, Ctrl+T a
PowerShell tab), and its handler's new paths act only on events that build never sends.

Landing order (the review of 2026-11-04, PITFALLS 121): Terminal's feat/new-tab first, which is
inert on the installed build, then this branch, installed and the GUI restarted. Not the other way:
this build with his live lbterm.lua opens nothing for a 'shell' pick (his handler returns false for
it), opens Shift+Enter in front with the picker still saying "Started", drops the first message,
records the Conductor's start as the projects card's, and leaves Ctrl+T a plain PowerShell. Both
landed in that order on 2026-11-04.

The review's fix-up (2026-11-04) merged main again (perf/budgets-busy-pc, fix/measure-lock,
perf/fonts-core and perf/gpu-memory among it) and changed:
- The folder a pick says. `newtab::folder_of` gives a drive's path with backslashes, as the config
  spells its folders, whoever said it (a pane's `file://` URL has forward slashes), and the event
  says `project_dir`, the project's own folder, beside `dir`, where to open: the Here row's `dir` is
  the open tab's folder, often one inside a project. His handler (Terminal's branch) finds the
  project by `project_dir` (else `dir`, the card's) in one spelling and opens a new session in
  `dir` when it's in the project, telling Conductor's `record new` that folder. Before, '+' then
  Enter in a tab in Warden/src opened PowerShell where Claude was asked for (PITFALLS 115).
- The root's other names. A project in `lbterm_projects` may have `aliases`, taken whole as its
  name, never as a prefix: his lbterm.lua gives the root `AI` and `~`, as Blueprint's `resolve`
  (`INDEX_NAMES`) and at-names take them, so "ai" finds Root ahead of Bargain Hunter and Trainer,
  which have it inside their names (PITFALLS 118). Home and index are old names since 2026-11-03
  (Blueprint's renames.json), which `resolve` refuses, so "home" finds nothing, as before.
- Several tabs on their way at once. Shift+Enter keeps the picker open, so a second start can come
  before the first's tab: they wait in order (`newtab::Starts`), the rail shows the oldest's
  Starting row, each tab that comes is the oldest's, a tab one took counting as had for every
  later one, and each ends on its own time. `expect_tab(nil)` takes back the newest. The state file
  keeps every start's record (`new_tab.starts`, `start` the last), each with its `id` and when it
  was asked for and ended by the GUI's clock (`asked_ms`, `ended_ms`), so a test holds the fold to
  never before its 20 s and the strip's notice to never before its 4 s (PITFALLS 120).
- The keys the pad names: from the key table (`InputMap::locate_app_wide_key_assignment`, again
  whenever it's made), "Ctrl+T" as his lbterm.lua binds it, nothing where nothing's bound, for the
  open panel's pad row and the + menu's hints (PITFALLS 119). The state file says them
  (`new_tab.keys`).
- The Starting row in the state file has no tab (`rail.rows[].tab` null), where it said 0, a real
  tab's id (PITFALLS 52), and "+N" leaves its slot out. budgets.json's pick is judged by its start's
  record (`new_tab.start.shown`), not by any pending row.
- What opened nothing now says so: a Resume or InMail pick no handler acted on (LBTerm has no
  default for them) folds its row at once with "Nothing opened for <project>"; Reopen with nothing
  kept says "Nothing closed to reopen"; the picker asked for with the pad off says "The new-tab
  picker is off".
- Reopen with no handler acting opens the default program in the tab's folder, whatever the tab
  ran: a Git Bash or cmd tab comes back as the default program. LBTerm doesn't keep what program a
  tab ran (a pane on a mux domain has no command line the GUI knows), so that's what landing 1
  does; Lachlan's handler resumes a Claude tab's conversation, the one case that has a session.
- The GUI's own budgets for the picker and a pick come from the warm measurements, 10 and 15 ms
  ("What the picker costs", below), where they were 100 ms each.

The fix-up's code was written while the warden refused builds for disk (C: under its 20 GB floor
from 02:40 on 2026-11-04, the build slots' stale incremental folders: .claude/docs/building.md), and
built after 28.9 GB of them were cleared. It compiled as written; its first runs of perf/new-tab.ps1
found two things, fixed: the pad named Reopen "Ctrl+T" where Ctrl+Shift+T was bound (the key table
keeps a shifted letter as its capital, PITFALLS 122), and three of the script's own waits were
wrong (a notice is never settled, PITFALLS 123; `time_ms` is the state file's root's; a mux tab's
title comes after it settles, PITFALLS 124). Then, on 87d95ee07's build: the check, wezterm-gui's
tests, perf/new-tab.ps1 and new-tab-lua.ps1 (with Terminal's worktree copy) with and without -Mux,
and spawn-no-activate.ps1 all passed (2026-11-04, 03:33 to 03:59, the desktop locked).

Held against a slot's build of main's code (2404cfc7f, `budgets -Against`, five rounds, the order
turned each round, one warden GPU job holding the lock, 04:00 to 04:27, the desktop locked, others
on 7 to 16% of its CPUs): key-echo on a mux domain 6.69 to 7.04 ms (+0.34, +-0.56, `same`; the
rounds 6.54 to 6.78 before and 6.76 to 7.27 after, three of five above every before, the GUI
thread's CPU a key 2.69 to 2.80 ms, `same`), locally 6.20 to 6.17 ms, rail-opening 161 to 159 ms
(its GUI's own 10.7 to 9.27), new-tab 40.2 to 35.2 ms and one-tab at rest, all `same`; the one told
apart was one-tab's handles, 794 to 803 (+9 a window, not traced). The review's gap of 1 to 2 ms a
key (PITFALLS 116) was against the installed build on the branch's older main and doesn't show
against main's own build in a slot: what's left is a lean of 0.4 ms on a mux domain that neither
test tells apart, and none locally. new-tab's p95 was over its 120 ms budget on both builds (192
and 210 ms), main's as much as the branch's.

Where the build so far differs from the spec, for the review and Lachlan:
- Enter with nothing named brightens the box with a pad's glint (`copy_in`, `copy_out`, the
  compositor's `Light::Pad` over it) rather than the hairline alone on `arrive_in`, `arrive_out`:
  the glint is the light the compositor already plays over the picker's visual.
- The pointer moving over the picker's rows moves its highlight, as a menu's focus follows it; the
  keys move it from there.
- The + menu's launch rows take the open tab's folder and the open pane's domain, and keep the
  entry's own command line and environment.
- A tab opened behind with no handler (the default) goes through `spawn_tab_or_window_activating`
  with `activate` false, as Lua's `spawn_tab { activate = false }` does. On a mux domain that goes to
  the server as SpawnNoActivate (`Domain::lbterm_spawn_activating`, which `ClientDomain` answers),
  so the server's window stays on its tab too. Before 2026-11-04 the GUI's window stayed (perf/
  new-tab.ps1 -Mux, 2026-11-03), but the GUI asked the server with SpawnV2, which the server's
  session handler answers by going to the new tab (as perf/spawn-no-activate.ps1's plain spawn
  shows), so a reconnect, `cli list` and the Conductor would have seen the new tab as the open one.
- Reopen keeps a tab closed with Ctrl+Shift+W's Yes only where that pane was its tab's last: a pane
  closed out of a split isn't a tab, and isn't kept. Close finished tabs, which keeps its tabs too,
  is landing 2's.
- The projects card's `lbterm-project-action` now says `source = 'card'`, so a handler tells the
  card's presses from the picker's without guessing from what's missing.
- Lachlan's lbterm.lua (section 9, on Terminal's feat/new-tab):
  - His `lbterm-project-action` and `lbterm-reopen-tab` handlers return nothing in a window with
    `LBTERM_STATE_FILE` (every test window), as his `open-uri` does: a test window running his file
    lists his real projects in its picker, and a pick there would have started a real Claude
    session and written Conductor's history (PITFALLS 68). The card's handler steps aside there
    too, which it didn't before.
  - Reopen doesn't call `expect_tab('Reopening <label>')`: LBTerm puts that Starting row up before
    the event. `ai open` says nothing back, so a resume that fails shows as the row folding at 20 s.
  - A pick from the picker that opens nothing (the spawn failed, Resume found nothing) takes its
    Starting row back at once with `expect_tab(nil)` before its notification, rather than leaving it
    for 20 s. The card's presses put up no row and take none back.
  - The picker's InMail (Alt+M) says `--said "picked <Project>'s InMail in the new-tab picker in
    LBTerm"`; the spec named only New's.
  - The recent picks' file is Terminal's `@Terminal:new-tab-recent`
    (`%LOCALAPPDATA%/Megaproject/new-tab/recent.json`, `recent-try.json` in a try window), named to
    LBTerm once `names.lua` has it (Project health's refresh writes `names.lua` again).

What the picker costs. Warm, its open to the frame that draws it was 2.1 to 2.3 ms in a test window
listing two projects (budgets.json's picker, two rounds), and 1.6 to 2.4 ms listing Lachlan's 49
by a click on the pad and by a key bound to `LbtermNewTab 'Picker'` (the review's probes,
2026-11-04). The first open in a GUI's life is cold: its words are shaped, measured and put in the
glyph atlas and its visual made for the first time, 42 to 95 ms in the review's probes (the paint
pass about 21 ms, the draw 13 to 44, the present 5 to 16), with 2 projects or 49; he sees it on his
first Ctrl+T after each start. The budget runner's warm-up had dropped it; budgets.json's
picker-first measures it alone, one cold open a round's window: with main merged in (perf/fonts-core
among it) and its two projects, 26.8, 24.5 and 26.8 ms to the frame, the GUI's own 14.3 to 16.3, the
GUI thread 17.2 to 19.3 ms, where a warm open took 1.89 to 2.07 ms and a pick 4.79 to 5.16 (three
rounds, 2026-11-04 04:31 to 04:35, the lock held), and its budgets are 1.5 times those. Lachlan's 49
projects weren't measured cold again on that build. Nothing warms it ahead yet: doing
the same work at config load or start would move it to another moment on the same GUI thread, and
which is better is a design question for Lachlan (the report of the fix-up asks it). The GUI's own
budget for the picker is 10 ms, from the warm numbers, so the cold first open is logged as over it
once each GUI start.

Measured as the pad, the picker and the Starting row went in (2026-11-03, 23:00 to 23:04, the
desktop unlocked, other agents' CPU jobs running beside, this the only GPU job): paint-cost.ps1
-Measure, Lachlan's look (New-TestConfig, so the pad shows), 1080 by 1920, three tabs on a mux
domain, a line of colour every 16 ms for 6 s, three runs a build, the installed build
(20261103-011219-de955813) and this branch's at bc0b63716 interleaved. The median a frame of the GUI
thread: before 11.351 and 12.074 ms, after 11.375 and 10.995; of the process: before 14.598 and
15.601, after 14.833 and 14.059. Every run of both built no template and made no GPU buffer a
frame, and kept the same visuals a frame (21.03, 21.32, 21.66 by run): the pad's look under the
pointer isn't drawn, hashed or kept at rest. GPU memory 470.0 MB before, 470.2 after; private bytes
643.7 to 649.1 MB before, 640.5 to 643.8 after. perf/new-tab.ps1 passed with and without -Mux on
the same build.

Measured as Reopen, spawn_tab{activate} and the budgets went in (2026-11-04, 00:46 to 01:13, the
desktop unlocked at each round's start and end, Lachlan at the PC: Git for Windows' askpass and a
Chrome popup came up during the first round; beside it other sessions' CPU jobs (Relay's and
ITMan's checks, another LBTerm track's builds) and that track's GPU jobs from 00:56 on, the warden
running up to three at once): `lbterm.ps1 budgets` over one-tab, twelve-tabs, rail-open, rail-pad,
key-echo, tab-by-click, new-tab, new-tab-local, close-tab and rail-opening, the installed build
(20261103-011219-de955813, copied) and this branch's (at 05af76e2e's code; its version string says
f3f3276f, the commit it was built over) interleaved, before, after, before, after. At rest every
round of both painted no frame; the GUI thread spent 0.047 to 0.099 ms a second before and 0.056
to 0.096 after, the process 1.27 to 1.52 before and 1.28 to 1.51 after. Medians of the actions,
before (two rounds) against after (two rounds), in ms: key-echo 7.41, 7.22 against 8.59, 9.19;
tab-by-click 6.30, 6.27 against 6.18, 5.65; new-tab 35.2, 35.5 against 35.8, 38.4 (its GUI thread
23.2, 22.8 against 22.0, 23.3 ms; the server 31.6, 32.1 against 31.7, 34.0); new-tab-local 67.7,
75.6 against 67.8, 63.8; close-tab 8.95 (the first round's errored on the Chrome popup) against
9.38, 8.86; rail-opening 162.9, 162.5 against 165.0, 164.6 (the GUI's own 12.9, 13.1 against 14.7,
14.4). Every action's frame count and the frame that showed it were the same in all four rounds. That
wasn't "no regression", as the stage's commit said: every after round of key-echo was above every
before round, and the review's 8 interleaved rounds (02:17 to 02:22, the lbterm-measure lock held,
the desktop locked) gave 7.07, 6.99, 6.67 and 6.80 ms before against 7.97, 7.66, 9.43 and 8.26
after, p95 9.1 to 14.3 against 17.2 to 21.4, with the same GUI-thread CPU a key (2.36 to 2.5 against
2.4 to 2.55 ms) and the same with the pad on or off: waiting, not work (PITFALLS 116). Its before
was the installed build, not a slot's build of main's code (PITFALLS 78).
The new actions on this branch's build, two rounds: the picker drawn in the frame after the click
in all 30 runs, a median of 2.29 and 2.34 ms (the GUI's own 2.09, 2.15), the GUI thread 3.9 and 3.7
ms; a pick's Starting row in the frame after Enter in all 30, 4.58 and 5.73 ms (the GUI's own 4.69,
5.53), the GUI thread 28.2 and 29.1 ms with the tab it opens, 8 and 9 frames. budgets.json holds
both to the frame after, and to 1.5 times the slower round as the others are.

## Summary

Don't redesign the tab bar. All three designers came to that on their own, from speed, from motion and from subtraction. The rail down the left and the strip across the top suit a window 1,080 px wide and about 1,880 px tall running 8 to 18 Claude tabs: width is what's short and height isn't (16 tabs leave about 470 px of rail empty), and tabs across the top would lose their labels and status words. The problem is where the actions are. There's no new-tab button. Ctrl+T opens a bare PowerShell, even though history.jsonl shows 32.6 Claude starts a day across 70 folders. The only way to pick a project is a 14 px pad on the far edge of the window, behind a 300 ms rest, listing about 60 projects busiest first with no search. The Conductor's log shows it used for 7 new sessions in about 5 days, against about 160 starts.

What gets built:

1. **The + pad.** It's fixed at the foot of the rail. With your window maximized on the portrait screen, that's the bottom-left corner of the screen, so a flick down-left lands on it.
2. **Left click, and Ctrl+T: the picker.** You type the way @names resolve ("war", "lbt", "barg") and Enter opens Claude there, in front. The open tab's project is already highlighted, so + then Enter starts another session where you are. Words after the name become Claude's first message. Shift+Enter starts it in the background and keeps the picker open. Ctrl+Enter opens a PowerShell there instead.
3. **Right click: a menu.** It lists the shells from launch_menu in the open tab's folder, "Shell in a project…", up to five recently closed tabs to reopen (Ctrl+Shift+T reopens the last one), and New window.
4. **A "Starting" row.** The moment any new tab is asked for, a dim row grows in at the end of the tabs, so the click answers in the next frame. That closes the top gap in FEEDBACK.md.
5. **The tab menu's new items:**
   - New session in <project>
   - Copy link
   - Hide from Conductor
   - Wait on…, for tab links without the right-drag
   - Close finished tabs (N)

   Other changes in the menus:
   - Duplicate tab becomes "Shell in its folder", since it never duplicated a Claude tab.
   - Move to new window moves further down.
   - Every row gets a one-letter key.
   - Every menu fades in and out and its highlight glides between rows, as DESIGN.md has planned since 2026-10-21.

Hide from Conductor depends on the Conductor. It has to add hide, show and a list of hidden sessions in its button.json, so LBTerm offers the item only once that file has the list. The InMail asking for it is ready.

Where the designs disagreed, I picked:
- The + stays fixed at the foot rather than moving along after the last tab.
- The picker opens beside the rail without opening the panel. Your wave is on, so opening the panel on each click would play it 30 times a day.
- Below the open tab's project, the picker's rows are in a stable order: Home, your last six picks, then A to Z.

Some choices are yours, listed under decisions. The main ones:
- Ctrl+T, which is built as the picker. Going back is a one-line binding.
- Whether the projects card goes now that the picker does its job.
- What hiding a session also stops, and whether a hidden session's questions still reach you in the view and on your phone.
- The Conductor pad's always-on spectrum.
- Parking.
- A few extras: Shift with the side buttons, middle-click close, and Interrupt.

## The build spec

BUILD SPEC: new-tab pad, picker, + menu, tab menu, Hide from Conductor. Repo: LBTerm's private repository. Each landing is a slot branch (`lbterm.ps1 slot`), checked with `lbterm.ps1 check` through warden-run, then `land`. Each landing also has a part in Terminal's lbterm/lbterm.lua, tested in lbterm/lbterm.Tests.ps1 and written up in docs/lbterm-lua.md.
- Landing 1: the + pad, the picker, the + menu, Reopen, the Starting row, and Lua spawn_tab{activate}.
- Landing 2: the menus (fade, glide, keys, glow) and the tab menu's items.
- Landing 3: Hide from Conductor. It ships after the Conductor ships hide and show (conductorAsk). Until its button.json has `hidden`, the item isn't offered: no half version.

Calls that are his taste, which I've picked:
- The + is fixed at the rail's foot.
- The picker uses the menus' look.
- The hidden mark is eye-off, on the pad's bottom-right corner.
- The new rows are named "Shell in its folder" and "New session".

Ctrl+T ships opening the picker (decision Q2). It's one binding in lbterm.lua to change.

1. THE + PAD
- Slot:
  - A foot slot, `Layout::foot`, the mirror of `Layout::head`.
  - A pill 42×30 px at 96 DPI, with 10 px corners (the label pill's), centred on `Layout::column()`. Its bottom sits LIST_PAD above the ring's inner edge.
  - A hairline rule in the head rule's grey sits HEAD_GAP (7 px) above it, 80% of the pill's width.
  - It never moves. It counts as a slot in `row_height` (rows + head + foot), so it can't fall off.
- Look: the InMail button's family (DESIGN.md, "The window strip"). At rest there's no fill, a hairline at white 14% and a + of two 1.5 px strokes 12 px across at white 45%. Under the pointer, or while its picker or menu is open, the hairline goes to 50% and the + to white 100%. That look is on a hover slot faded on `button_hover`, as render/hover_fade.rs does the strip's buttons. The closed rail has no label and no colour for it. On the open panel its row reads "New session" at white 85%, with "Ctrl+T" at 35% right-aligned.
- Hit zone: the slot across the closed rail. While the window is maximized it also runs to the window's left edge and down to its last pixel row, so a flick into the screen corner hits. His window is maximized on the 1080×1920 portrait screen, and its bottom-left is a true screen corner. When the window isn't maximized, the zone stops at the resize border.
- The pointer on the pad lights the ring to pending at once on `hover`, as anywhere on the rail.
- Room: rows give up label lines before any tab leaves the rail. When rows at ROW_MIN with two-line labels don't fit, labels drop to one line (44 px rows), then to none (28 px rows, with the label still on the open panel). As a line goes it fades where it was on `pane_exit`, while the rows shrink on `pane_layout`. Past 28 px rows, the foot rule shows a dim "+N" at white 50%, so no tab is ever cut silently. Today `row_visible` drops them without a word. That point is about 60 rows on his window, so it's practically unreachable.
- Config: `lbterm_tab_panel.new_tab`, default true (false means no pad and no slot), in config/src/lbterm.rs with its tests.
- State file: `new_tab {rect, hover, open}`.

2. LEFT CLICK (and Ctrl+T): THE PICKER
- Opening:
  - It opens in the next frame, with a ripple from the press point clipped to the pill (`press`).
  - While it's open the panel holds where it is. Closed stays closed: no opening after a rest while the pointer sits on the +. Open stays open: the picker counts as the panel, as the tasks card does. So no edge moves under it and his wave (on in lbterm.lua) isn't thrown on each click.
- Place: the card's left edge is a gutter past the closed rail (x 76 at 96 DPI) or past the open panel. Its bottom is on the panel's bottom and it grows upward, at most 70% of the window's height, 360 px wide.
- Look: the menus' family, the tab menu's card. That's the #262634 fill at 55%, 12 px corners, the hairline and the panel's accent light. DESIGN.md puts the InMail list there: a click, a list, a pick that opens a tab. It gets z-layers of its own over the panel, checked against every owns_zindex: 93 to 95, with 96 for the pad's look under the pointer (render/new_tab.rs, whose test holds them apart from every other owner; 85 to 87, which looked free on 2026-10-31, were taken since).
- The box: the card's last row, 36 px, level with the pill.
  - Fill white 6%, a hairline at white 12% that goes to 24% while the box has the keyboard, and a 2 px caret at white 90%. This is the Conductor view's box. Typing and IME use the view's box editor.
  - Placeholder "Open Claude in…" at white 35%.
  - Under it, a footer at white 35%: "Enter open · Shift+Enter behind · Ctrl+Enter shell".
  - When there's a first message, a line above the footer at white 60%: "First message: <text>".
- Rows: 28 px, above the box, row 1 nearest it. Each row has:
  - a 16×16 key chip 1–9 at white 7%, on the first nine rows, shown only while the box is empty;
  - the project's name at white 92%, or 60% when it's quiet (no session, no mail);
  - for a repo, an indent under its project past the blue repo mark (#4FB3FF, nf-oct-repo);
  - on the right, its sessions' dots with counts (working green, waiting orange, idle blue), "+N" for subagents and tasks, and "N mail" in the waiting orange. That's the projects card's row data (`lbterm_extras::projects`).

  On the highlighted row the right side gives way to the chips "Resume" and "InMail N" (the second only with mail), fading on `button_hover`. Up to 16 rows show, then the wheel scrolls them, with no motion.
- Order: each project once, with a 9 px rule between groups.
  1. "Here": the open tab's folder (its session's cwd.txt, else the pane's), named by the deepest project above it. The path shows dim when it isn't the project's root. It starts highlighted.
  2. Home (the root of the projects folder), unless it's Here.
  3. The last 6 projects picked in the picker, newest first.
  4. Every other project A to Z, repos under their project.

  Sessions and mail never move a row. The recent picks are kept in memory and written through a rename by a thread of its own to `lbterm_projects.recent_file`. His lbterm.lua sets that to a file in Terminal's machine-state folder (a try name in try windows), and New-TestConfig clears it. When unset, the picks live in memory only. They're read at start off the GUI thread, and nothing does file I/O in a GUI callback.
- Matching: the first match is highlighted in the same frame as the key.
  - The first word resolves the way @names do: a leading @ dropped, any case, spaces optional. The order is the whole folder name, then the one project it's the start of, then the start of any word in the name ("hunt" finds Bargain Hunter), then a substring.
  - A repo matches by its own name ("lbt" finds LBTerm).
  - Matched letters show at white 100%.
  - A following word joins the name only if it's the next whole word of the matched name. "bargain hunter find a kettle" opens Bargain Hunter with the first message "find a kettle". "bargain hunt for kettles" opens Bargain Hunter with "hunt for kettles".
  - Everything after the name is the first message.
  - Tab completes the highlighted name into the box, with a space after it.
  - With no match, the list says "No project called xyz", and Enter brightens the box's hairline once (`arrive_in`, `arrive_out`).
- Keys:
  - Up and Down move the highlight.
  - 1 to 9 pick a row while the box is empty. Once it isn't, digits are text (BTD2).
  - Enter opens Claude in front.
  - Shift+Enter opens Claude behind: the window stays on its tab, the picker stays open with the box cleared, and "Started <name>" shows under the box.
  - Ctrl+Enter opens a PowerShell 7 tab in that folder, in front.
  - Alt+R resumes the project's newest conversation that isn't open. Alt+M reads its InMail, only with mail.
  - PgUp and PgDn scroll.
  - The picker closes on Esc, on a click away, when a menu opens, when the open tab changes under it (a side button, the Conductor activating a pane), or when the window loses the keyboard.
  - It takes every key while open, as the tab menu does in keyevent.rs: since 2026-11-07 (his 7b) those bound in the config too. The picker isn't one of the compositor's menu slots, so it has no lamp to flare for a key it has no use for: since his 3a of 2026-11-08 such a key (one that neither moves, picks nor edits the box; a modifier on its own excepted) lights round its box, as Enter with nothing named does (`lbterm_picker_light`; his answers, below).
- Mouse: a click on a row is Enter, Shift+click is Shift+Enter, and a click on a chip does that chip.
- The keyboard's light: while it's open, the pane loses the active outline and dims to 85% on `dim`, as when the window loses the keyboard, and the box's hairline is at 24%. Closing gives both back on `dim`, with no arrival pulse, since a key or a click did it.
- What it fires: the projects card's event, `lbterm-project-action (window, pane, p)`, extended to `p = {name, dir, action = 'new'|'inmail'|'resume'|'shell', prompt?, behind?, source = 'picker'|'card'|'tab menu'}`. With no handler returning false, LBTerm opens default_prog in `dir` for new and shell, in front unless `behind`. The projects card fires it as today.
- Shell mode: the + menu's "Shell in a project…" opens the same picker titled "Shell in…". There Enter opens PowerShell 7, Ctrl+Enter opens Claude, and there's no first message.
- Actions:
  - `LbtermNewTab 'Picker'` on Ctrl+T.
  - `LbtermNewTab 'Shell'` on Ctrl+Alt+T: default_prog in the open pane's folder, what Ctrl+T does today.
  - `LbtermNewTab 'Reopen'` on Ctrl+Shift+T. It replaces WezTerm's default binding there, which was a second SpawnTab.
- Without `lbterm_projects`, a left click opens default_prog in the open pane's folder, as upstream's + did: there's nothing to pick from.

3. RIGHT CLICK ON THE +: THE + MENU
It opens at the pointer as the button comes up, with a ripple on the press. Since the + is at the bottom, it opens upward. It's in the menu family, 260 px wide, with 30 px rows and key hints right-aligned at white 35%.
  1. The `config.launch_menu` entries in order, each "<label> here" in the open tab's folder, through SpawnCommand in that pane's domain:
     - PowerShell 7 here (P, with the hint Ctrl+Alt+T)
     - Windows PowerShell 5.1 here (W)
     - Command Prompt here (C)
     - Git Bash here (G)

     A row's key is its first letter where that's unique. The list stays his editable config, and lbterm.lua's "Right-click the + button" comment becomes true again.
  2. Shell in a project… (I).
  Then a rule.
  3. Up to 5 Reopen rows, newest first. Each reads "Reopen <label>" at white 85%, with "<age> · <project>" at 50%. The first is R and carries the hint Ctrl+Shift+T. With none, one dim row says "Nothing closed yet".
  Then a rule.
  4. New window (N): SpawnWindow on the same domain.

4. REOPEN
- What's kept: LBTerm keeps the last 10 tabs Lachlan closed himself, in memory, with no file read. These are the closes that write close marks (Close tab, Close other tabs, Ctrl+W, Ctrl+Shift+W's Yes) plus Close finished tabs. For each it keeps every pane's claude_session, the row's label, the folder (cwd.txt, else the pane's), the project and the time.
- A pick (or Ctrl+Shift+T for the newest) fires `lbterm-reopen-tab (window, pane, {session?, dir, label})` and takes that tab off the list. With no handler returning false, LBTerm opens default_prog in `dir`.

5. THE STARTING ROW
- What starts it: every tab asked for in this window. That covers:
  - the picker, the + menu and the tab menu's New session and Shell rows;
  - Ctrl+T then a pick, and Ctrl+Alt+T;
  - Reopen;
  - anything that calls `window:lbterm_expect_tab` (the InMail button, links).
- Look: a row grows in at the end of the tabs.
  - A dim pad (#24242C) with the white waiting arc turning on its corner, the menu spinner's arc at a turn every 1.4 s, in DWM.
  - Its label is the project or the shell's name.
  - On the open panel it says "Starting…" at white 50%.
  - The strip keeps its own rule: "Opening …" after a quarter second.
- Arrival: the first tab the window didn't have becomes it.
  - The row dissolves into the real one.
  - In front: the marker glides to it and the switch cross-fades.
  - Behind: the pad glints.
- No arrival within 20 s, or `lbterm_expect_tab(nil)`: the row fades where it is and the rows close up, and the strip says "Nothing opened for <what>" for 4 s.
- State file: the row with `pending: true` and its label.
- This also closes FEEDBACK.md's Duplicate gap.

6. EVERY MENU (the tab menu, the + menu, the Conductor's, the InMail list, the prompt list)
- Motion: menus fade in on `pane_enter` and out on `pane_exit`, from wherever they are. The highlight glides between rows on `hover`. This is DESIGN.md's planned item of 2026-10-21, built once in the faded-card path.
- Glow: the four stacked glow rings (16, 9, 5 and 2.5%, in render/tab_menu.rs) band at 2×. They're replaced by one falloff from `palette::glow_alpha` at today's strength, each pixel drawn once, as the panel's light is (TAB-PANEL.md, "The light").
- Keys:
  - Every row can carry a one-letter key, shown dim at its right: white at 35% blended into the card, as the + menu's. Given as a colour's alpha from landing 2, the keys and Close finished tabs' second line drew full white until 2026-11-09, when landing 3's greyed row (his 11b) was measured (PITFALLS 248).
  - A key that no row takes flares the card's edge once, its hairline and the light round it going white (`arrive_in`, then `panel_glow`; his 9a of 2026-11-08, where the hairline alone brightened on `arrive_in` and `arrive_out`).
  - Enter with no row highlighted highlights the first (FEEDBACK.md gap 6).
- DESIGN.md's Menus line gets the menus' real fill: #262634 at 55%.

7. THE TAB MENU, in order
  1. Rename tab (R). Fix: the box starts from the label the row shows, as TAB-MOUSE.md already says. A test window started it from "pwsh.exe".
  2. Colour, the swatches. New: 0 to 8 pick one.
  3. Copy link (L). New, on Claude tabs only. It puts `[<label>](lbterm://session/<full id>) in <Project>` on the clipboard, leaving the project off when the label ends with it, and the pad glints.
  Then a rule.
  4. New session in <Project> (N). New. It fires `lbterm-project-action` with `{action='new', source='tab menu'}`. It's absent when the tab's folder is in no project.
  5. Shell in its folder (S). This was Duplicate tab and does the same: PowerShell in the tab's folder and domain, in front.
  Then a rule.
  6. Hide from Conductor, or Show to Conductor (H). New, on Claude tabs only, and only when button.json has `hidden` (section 8).
  7. Wait on… (W). New. It shows on a Claude tab with `lbterm_links` set and another Claude tab open. The card's rows dissolve on `swap` into a first row "‹ Back", then the other Claude tabs by label, each with its pad's colour. Picking one fires `lbterm-tab-link {action='toggle', waiter, on}`, as a right-drag's drop does. Left or Esc goes back.
  8. The tab's link rows, under their own rule, as today.
  Then a rule.
  9. Move to new window (M), as today, moved down.
  Then a rule.
  10. Close finished tabs (N) (F). New. It closes, writing close marks, every Claude tab (not the Conductor's) that meets all of these:
      - its row says Done or Your turn, not Question, Extras, Permission, Login, Blocked or Failed;
      - it has no unread white dot;
      - no agents or tasks are running in it;
      - its objectives mark is the green dot.

      A second line at white 50% lists those tabs by label, cut to the card's width. The row is absent when there are none.
  11. Close other tabs (O), as today.
  12. Close tab (X or Delete), as today, last.

On a tab with no Claude session, rows 3, 6 and 7 are absent.

8. HIDE FROM CONDUCTOR, LBTerm's part
- When it's offered: only when `lbterm_conductor.state_file` (Conductor's button.json) has a `hidden` array, on a tab with a Claude session. It acts on every claude_session in the tab's panes. It reads "Show to Conductor" when all of them are hidden. The Conductor's button.json has had `hidden` on every write since 2026-11-02 (below), so with today's Conductor the item is on every tab with a Claude session; a button.json without it (a Conductor from before) still offers nothing, and neither does no file, or no `lbterm_conductor` in the config.
- What the Conductor shipped (2026-11-02, its feat/hide-sessions, a694394, from the InMail below; read from its contract on 2026-11-09, `api Conductor hide`, `api Conductor button`, `api Conductor results`, `api Conductor hidden`, and its SEAT.md, `@Conductor:seat-protocol`):
  - The commands: `conductor hide <session id> --by <who> [--said "<his words>"] [--result <id>]`, `conductor show` the same, and `conductor hidden [--json]`. They work with the Conductor on or off and the seat running or not, and each call writes button.json again with a fresh `updatedAt`, the session already hidden or not, so LBTerm's watch always sees an answer.
  - button.json (`@Conductor:button`): `{ state, mode, listening, busy, asks, sessionId, label, tooltip, hidden: [session id, ...], updatedAt }`, `hidden` always there, `[]` when none, whether the Conductor is on or off. That's the one file LBTerm reads for it (below).
  - Its own list (`@Conductor:hidden`, `hidden.json`): `{ sessions: { <session id>: { at, by, said, label, project } }, updatedAt }`, keyed by Claude session id, so a hide holds through LBTerm restarts, crash restores, a move to another tab and `claude --resume`; a fork or a new session starts visible. Written only by `conductor hide` and `show` and the seat's hourly prune, which drops a session gone for good (not running, no transcript to resume). LBTerm doesn't read it: button.json's `hidden` says all the rail needs, and is watched already.
  - How a call ended (`@Conductor:results`, one `<id>.json` each, deleted by the reader, gone after a day): `{ id, ok: true, action: hide|show, sid, hidden, at }` or `{ id, ok: false, action, sid, error, at }`. Only the handler in Lachlan's lbterm.lua reads it (section 9), as it reads `new`'s.
  - What hiding stops: a hidden session gets no wakes, notices, outlines (`conductor.focus`), bars (`conductor.estimates`), reviews or place in the RSI report, and every tool of the Conductor's aimed at it is refused; its events are still logged, with `hidden` true and `wake` and `notice` false. The hub's digests, labels, objectives and questions go on, and `conductor status`, `important` and the Conductor's dashboard still show it, marked (his 4a and 5a, below).
  - What it didn't do until 2026-11-09 (found by landing 3's review, 2026-11-09, from conductor.ts): `hide` and `show` take one id a call (`args.length !== 1` is a usage error, which writes no result even with `--result`), and `setHidden` reads hidden.json, changes its one id and writes hidden.json and button.json back with no lock (`writeHidden`, `writeButton`, `writeAtomic`), as the seat's hourly `pruneHidden` does. Two calls at once lose one: both read the same list, the last write wins, and both results say `ok: true` (PITFALLS 243); two renames over one target at the same moment can also fail with EPERM. So LBTerm's handler makes one call a pick (section 9), and LBTerm's InMail of 2026-11-09 asked the Conductor for `conductor hide|show <sid> [<sid>...]` (every id checked first, all or nothing, one read-modify-write, one write of each file, one result `{ id, ok, action, sids, hidden, at }`), a lock round `setHidden`'s and `pruneHidden`'s read-modify-write, a usage refusal that still writes its result, a test of two hides at once in a sandbox, and its own figure for how long a hide takes to its write. Until it ships them a pick on a tab with two Claude sessions is refused with a usage error and no result: nothing is hidden, the row goes back after 5 s with the strip's notice, and the handler's notification says after 20 s that the Conductor didn't answer. A tab with one session is one id, as before. Shipped on 2026-11-09 (its answer that day, merge 95d0f34, commit bde5991, feat/hide-several): `conductor hide|show <sid> [<sid>...]` checks every id first and is all or nothing (with several, "None of the N sessions was hidden from the Conductor, as one was refused: <why>; ..."), then one read-modify-write under a lock, one write of hidden.json and of button.json, and one result, `{ id, ok: true, action, sids, hidden, at }` or `{ id, ok: false, action, sids, error, at }`, sids in the order given, lower-cased, and button.json's `hidden` listing newly hidden ids in that order, the active pane's first; the result has no `sid` any more, which the handler never read. The lock is `hidden.lock` beside hidden.json (not in button/, so LBTerm's watch on that folder isn't woken), taken by hide, show, the seat's prune and every write of button.json; a waiter tries every 5 ms, takes over a dead holder or one past 5 s, and gives up after 10 s with an error in the result. A cause the review didn't name: on Bun a reader in another process makes a rename over the file fail with EPERM (Bun's readFileSync opens without share-delete; 3,756 of 4,194 renames failed against one reader in a loop), so its writeAtomic tries the rename again every 2 ms for up to 2 s; LBTerm reads button.json through Rust's File::open, which shares delete, so it never holds the rename back (Lua's io.open would). A usage refusal with `--result` now writes its result. Its test runs six hides, the prune, 30 pad writes and two readers as separate processes released together (passed 6 of 6, failed 3 of 3 with the lock off). From the command's start to button.json's write, about 100 ms (medians of 6 warm runs: 97 for one session, 103 for two), the process ending at about 115. Until the seat running at the time restarts, its own pad writes don't take the lock; hide and show do, each a new process. So a pick on a tab with two Claude sessions now hides both in one call, and nothing is refused for being two.
  - The seat's view says it: each of `seat.view`'s `sessions` has `hidden` (true for one he hid, which never has a guess, `percent` and `agents`, or `conductor`), and each of its `waiting` has `hidden` as for its session, a hidden session's question staying there for his own answer (`seat.answer` goes through for it, his 5a). So the view's Hidden group below is built from what the seat sends, and nothing was asked of the Conductor for it. An ask the Conductor's Claude aims at a hidden session never reaches the view: the seat denies it at once.
- The event: a pick fires `lbterm-conductor-hide (window, pane, {sessions, label, hide})`. LBTerm hides nothing by itself. Built on 2026-11-09 (stage 2 of landing 3, render/conductor_hide.rs's `lbterm_conductor_hide`): `sessions` is a Lua list of every `claude_session` in the tab's panes, each once, the active pane's first (`tab_sessions`); `label` the tab's label as its row shows it (render/tab_panel.rs's `tab_label`); `hide` true for Hide from Conductor, false for Show to Conductor; `source` where it was picked, `tab menu` or `conductor view` (added in stage 3, beyond the spec's three fields, so the handler's `--said` can say which: the view's Show has no tab menu); `pane` the tab's own active pane, not the window's (the window's is where the menu was opened, which can be another tab's when the menu came from a row). Fired on the GUI thread through `config::with_lua_config_on_main_thread`, as `lbterm-tab-link` is; no handler, or one that does nothing, leaves the pick to run out (below). A test window's handler must be the test's own: Lachlan's lbterm.lua runs in every window `New-TestConfig` makes, so Terminal's handler has to step aside in a test window (PITFALLS 68: `LBTERM_STATE_FILE` in the GUI's environment, which every test window has), and perf/conductor-hide.ps1 writes a config of its own.
- Pending: the row dissolves into its hidden look at once and stays pending for 5 s, the tab links' PENDING.
  - Confirmed when button.json's `hidden` lists every session: the pad glints.
  - Not confirmed within 5 s: the row dissolves back, and the strip says "The Conductor didn't hide <label>" for 4 s.
  - As built (2026-11-09, `lbterm_extras::conductor::hide`): each window keeps its picks (`TabPanelState::hides`, a `hide::Picks`, at most one a tab: a second pick on the same tab takes the first's place). From the paint after the pick the row looks as asked (`Picks::look`: a waiting pick's `hide`, else hidden when the file's list names every session of the tab), which sets `Tab::hidden` and so `Row::hidden`, in both of the row's looks (lbterm-panel's `look.rs`), so the row dissolves on `swap` as any change of look does, once a menu's card no longer holds the rail. While a pick for the tab waits, nothing asks for that tab again: its menu's row is there greyed, saying "Hiding from Conductor…" or "Showing to Conductor…", with no key, and picks nothing (`Picks::offer` gives `Offer::Waiting`, `menu::inert`: the keys and the pointer pass over it and H is a key no row takes, so the card's edge flares; Lachlan's 11b of 2026-11-09, where the review's fix-up had taken the row away; a menu still open as the pick settles brings the row alive in its place, `lbterm_menu_hide_again`), the view's Show for one of its sessions does nothing (`lbterm_view_show`, logged), and a pick from a menu opened before the first began is refused (`lbterm_conductor_hide`, logged), 5 s at most. Stage 2 built the row to follow the pick's look (a Hide waiting offered Show), but the Show it offered started a second `conductor` beside the first, which the Conductor carries out in whatever order the two finish, so a Show taken as done from its own write could see the Hide's land after it and leave the tab hidden with nothing pending (PITFALLS 244; the review's fix-up of 2026-11-09). Each paint of the panel settles the picks first, at the top of `paint_tab_panel`, panel on or off (`lbterm_hides_settle`): a pick is done when the file has been written since it (its `updatedAt` isn't the one the pick saw, `Pick::then`) and `conductor::confirmed` says it, and its tab's pad glints in that paint (`TabPanelState::glints`, played after the rows are drawn); a pick 5 s old isn't done: it goes, the row looks as the file says again, and `lbterm_strip_notice` says `hide::not_done` for the strip's 4 s (`strip::NOTICE_FOR`). The notice needs the window strip, which is there only with the fancy tab bar shown and the panel hiding the tabs (DESIGN.md, "The window strip"; render/strip.rs, `lbterm_strip_content`): with the tab bar off, a pick that runs out is said only by its row going back (perf/conductor-hide.ps1's config keeps the tab bar on for it). The answer is looked for before the time, so a write the paint at 5 s finds still counts. The first pick still waiting sets `TabPanelState::hide_due`, which `tab_panel_wake` waits for with the rest, so nothing polls: a write of button.json repaints every window anyway (FLOWS.md, "The Conductor's state file, from its write to the paint"), and the end wakes one paint.
  - Why a write since the pick: the Conductor writes button.json on every `hide` and `show` with a fresh `updatedAt`, hidden already or not. Without that test, a Show picked while the Hide before it hadn't been answered would read as done from the file from before both (it hides nothing yet, which is what a show asks for), glint, and then the row would flick to hidden and back as the Conductor carried out the two in turn (PITFALLS 236).
  - The words when a Show isn't done: "The Conductor didn't show <label> again" (`hide::not_done`). The spec named only the hide's; this is a question for Lachlan.
- The look:
  - Closed rail: nf-md-eye_off at 10 px, white 60%, on a 12 px disc of the rail's surface at the pad's bottom-right corner. The top-right is the arc's and the unread dot's, and link marks sit left of the pad. It's never a status colour.
  - Open panel: a "Hidden" chip in the project chip's look, at white 45%.
  - The rail still turns orange when the tab waits on him, since that light is claudehook's.
  - As built (2026-11-09, stage 3 of landing 3), from `Row::hidden` (a pick waiting or the file's list, "Pending" above), which is in both of the row's looks, so a hide or a show dissolves the row on `swap`:
    - The disc is `lbterm_panel::Layout::hidden_mark`, a 12 px square centred 3 px in from the pad's bottom right corner, as the arc's square (`spinner`) is from its top right, so the two are mirror images and neither covers the other; the links' marks end left of the pad. `tab_panel_pad` fills it in what's under the pad (the card, or the row's hover or open fill), after the rings and the arc or unread dot, and again over an open row's highlight. Like the arc's disc it covers a corner of the pad and of a ring round it (the tab's colour, a link's question, a quiet reply, extras), and the bottom right of a two-digit number.
    - The glyph is `words::HIDDEN_GLYPH`, nf-md-eye_off (U+F0209), placed by `words::marks` in the middle of the disc, white at 60% over the disc's colour (`palette::hidden_mark`), in the marks' font: the title font's style at 7.5 pt (`tab_panel_mark_font`), which is 10 px at 96 DPI and as much more as the DPI is; the glyph comes from the Symbols Nerd Font every font falls back on, as the Conductor's glyph on its pad does. It's laid out with the pads' numbers and lifted with them over an open highlighted row. Looked up only while a row is hidden.
    - The chip: `words::place` puts "Hidden" (`HIDDEN_CHIP`) after the project's chip on the row's second line, in the same `Placed::chips` (the project chip's fill, `CHIP`, and its 6 px corners), its word white at 45% over it (`palette::hidden_word`), and only when it fits whole: on a panel too narrow for it, the status gets the room and the mark still says it.
    - perf/conductor-hide.ps1 checks each from captures of a card colour it sets: the disc cut out of the pad's corner and a neutral grey glyph in it, centred within 2 px, none on a shell's pad; the pad still orange with the mark on it while the hidden tab waits on him; the chip's fill and its word on the open row, none on the shell's.
- The Conductor's view: sessions whose `seat.view` entry says `hidden: true` go in a folded "Hidden · N" group at the foot of Sessions. Each has a Show button that fires the same event with `hide = false`. The folded header shows, in orange, how many of them wait on him.
  - As built (2026-11-09, stage 3 of landing 3; lbterm-extras's `seat::layout`): the seat's `sessions[].hidden` is read into `protocol::Session::hidden` (false when the seat doesn't say, as a seat from before 2026-11-02 doesn't). Hidden sessions are in no status group (Waiting, Working, Idle, Other) and are counted in none of the side's head or the folded side's buttons (`sessions_in`), but in the Sessions heading's total; they go in the group `sessions:hidden` after Other, with its own heading, "Hidden" then "· N", and, open or folded, "N waiting on you" in orange for those whose status is the Waiting group's (`waiting` or `blocked`). It starts folded (`FOLDED_AT_FIRST`; `Ui::is_folded`, PITFALLS 238) and a press on its heading opens it for as long as the window has the view's state. Open, each is a line: its status's dot, its label and project, its status in its colour, and Show (a words-only button in Material's blue, `Target::Show`) at the line's right end, with "Asks: …" under it in orange when it asks something; a press on the line opens its tab as any session's card does, and on Show runs `lbterm_view_show` (render/conductor_hide.rs; PITFALLS 237 for why Show is drawn after its row). The state file says the group's heading as the button `section:sessions:hidden`, each Show as `show:<session id>`, and each hidden session's card with kind `hidden` (STATE.md).
  - The press: where this window has a tab holding the session, that tab's row waits for the Conductor's word as a pick from its menu does (a pick for that one session: its row visible from the next paint, the glint, or after 5 s the strip's "The Conductor didn't show <label> again"), and the event's `pane` is the tab's own; where it hasn't, nothing waits and `pane` is the window's active tab's. While a pick for that tab waits (a Show from its menu, or this same Show pressed again), the press does nothing and says so in the log: a second request would race the first (PITFALLS 244). The view stays open; the seat's next view, without `hidden` on it, moves the session out of the group.
- The send light (Lachlan's 6a of 2026-11-01): on a hide or a show, a light runs along the rail from the tab's pad to the Conductor's pad, on a new 600 ms token, `send`.
  - As built (2026-11-09, stage 4 of landing 3): the pad's glint sent on, the request going to the Conductor, whose word comes back later as the tab's own pad's glint ("Pending" above). A piece of the lights' white swatch the glint's size (the pad grown 2 px, 6 px corners) rises to the press's white (35%) over the tab's pad on `copy_in` as it sets off, is carried up the rail's column on `send` (`600ms cubic-bezier(0.33, 0.33, 0.4, 1)`: `panel_sweep`'s curve, an even pace from the start slowing to a soft stop, so it runs rather than jumping most of the way and creeping as an ease-out would), held at that white all the way and over the pads it passes, and goes out over the Conductor's pad on `copy_out`: 860 ms in all at real speed. Every pad is the same size in the same column (`Layout::pad_in`), so only its offset moves.
  - Where it's built: `lbterm_hide_pick` (both a tab menu pick and Show in the view where this window has the tab) puts the tab on `TabPanelState::sends`, once a tab while it waits; `tab_panel_sends`, from `tab_panel_glints` once no menu's card holds the rail (his 7b: under the card it waits with the rows and sets off in the frame the row dissolves into its new look), turns each into the compositor's `Light::Send` from its pad to the Conductor's; `prepare_lights` plays it with `lbterm_motion::send`'s plan, one visual in `panel_lights` whose offsets and opacity DWM carries, so the GUI thread runs nothing for it while it plays (FLOWS.md, "A Hide from Conductor pick", step 8). The state file names it `light.send` (STATE.md).
  - When there's none: motion off (`lbterm_motion::send` gives no plan: the row's look is the whole answer, as for every end state); without the compositor; a rail without the Conductor's pad; a tab with no row in view (past the rail's last); Show in the view on a session with no tab in this window (no pad to send from); the panel off (a pick from the tab bar's menu): nothing is kept for it, and a paint with the panel off, or without the compositor, drops every glint and light asked for, the Conductor's answer's glint included, since only the composited panel's paint plays them and a kept one would play stale with the rest when the panel came back (the review's fix-up). Each pick sends its own light: a second pick on the same tab while one runs sends a second, never turning the first round, since it's a second request; two picks of one tab under one card send one.
  - perf/conductor-hide.ps1 checks it from the state file: `light.send` in a frame after each pick (the hide and the show from the menu, Pair's hide, the view's Show on Tab Wave), never while the rail is held, with its whole run (500 to 870 ms) left as it sets off, and none for Show on a session with no tab here. Its look is motion/send-light.ps1's (capture.ps1 with a stand-in Conductor, slowed and at real speed). Its first capture (2026-11-09 01:55, slowed five times) ran with the displays asleep, no input for 10,870 s, and showed nothing DWM plays, the menu's own fade and flash included, as main's build showed none in the same conditions (PITFALLS 239). Captured again with Lachlan at the PC (2026-11-09 11:27 to 11:28, the displays on, the branch's build at 8a9bffe9: slowed five times, at real speed, with motion off, and interrupted by a tab switch 2 s on): at real speed the light sets off from pad 4 once the menu's card has gone, climbs past pads 3, 2 and 1 and goes out over the Conductor's pad in about half a second, a small grey square that reads against the dark rail. Lachlan watched the real-speed capture as a video (the window, and the rail at 2x; ImageView shows a GIF's first frame only) and kept it as built: his 16a of 2026-11-09.
- Reading the list: button.json's `hidden` is read on the thread that reads the file now (state_files.rs, `lbterm_extras::conductor`), with no new watcher. Built on 2026-11-09 (stage 1 of landing 3): `conductor::read` keeps it as `Said::hidden`, a `conductor::Hidden` (a set of session ids, so the same ids in another order say the same), `None` when the file has no list there, and `None` too, with a warning in the log, when `hidden` is there but isn't a list of strings (null, a word, an object, a list with a number in it), since the Conductor's contract says it's always one; a blank id names no session and is passed over. A hide or a show is one more write of a file the `lbterm-state-files` thread already follows, so it costs what any write of it does (FLOWS.md, "The Conductor's state file, from its write to the paint"): a read and a parse on that thread, every window repainted, and each paint's `state_files::said` a lock and an `Arc` clone. What the rest of the landing asks of it:
  - The row (section 7, row 6): `conductor::offer(said, sessions)`, with `sessions` the tab's Claude sessions (every pane's `claude_session`): `None` with no list or no session, `Show` when the list names every one of them, else `Hide`. The Conductor's own tab never has a rail row, so it never has a tab menu to offer it in.
  - The mark: on a tab whose every session is hidden (`Hidden::cover` is `AllOf`), when the row says Show, so the mark and the row always agree. A tab with some hidden but not all (a hidden session moved in beside one that isn't, or `conductor hide` run on one of them by hand) has no mark, and its row says Hide, which hides the rest; as the Conductor sees it, part of that tab is still its to watch.
  - Pending: a hide is confirmed once the list names every session it was for, a show once it names none (`conductor::confirmed`), and nothing is confirmed while the file has no list.
- State file: `hidden` and `hidden_pending` on rail rows. Built (2026-11-09): `hidden` is the row's look (`Row::hidden`: every session on the list, or a Hide waiting), `hidden_pending` that a pick still waits (true for a Show waiting too, with `hidden` false); the Starting row has neither. The menu's row is `menu.items[]` with key `H`, its label "Hide from Conductor" or "Show to Conductor".
- The row as built (2026-11-09): `lbterm_extras::menu`'s `Item::Hide`, key H, in Wait on…'s group and before it (so with Wait on… absent it's a group of its own under a rule), there only on a Claude tab whose `Facts::conductor` is set. `lbterm_open_tab_menu` sets it once as the menu opens from render/conductor_hide.rs's `lbterm_hide_offer`: none without `lbterm_conductor`, on the Conductor's own tab (the tab bar's menu can reach it, the rail's can't), on a tab with no `claude_session` or while the file has no `hidden`; greyed, `Offer::Waiting`, while a pick for the tab waits ("Pending" above; his 11b). The mark on the rail and the Hidden chip on the open panel draw from `Row::hidden` ("The look", as built).
- No filter: LBTerm doesn't filter the hub's outlines or estimates for hidden sessions. Doing that would hide a Conductor bug.

9. TERMINAL'S lbterm.lua
- `open_claude_tab` takes `o.by` and `o.behind`.
  - `--by` comes from `p.source`: "new-tab picker", "tab menu" or "projects card".
  - `--said` reads "picked <Project> in the new-tab picker in LBTerm", plus ", first message '<text>'" when there is one.
  - `p.prompt` goes into `o.prompt`.
  - `behind` passes `activate = false` to `spawn_tab`.
- 'shell' is left to LBTerm's default.
- Keys, each bound only where `wezterm.has_action('LbtermNewTab')`: Ctrl+T to Picker, Ctrl+Alt+T to Shell, Ctrl+Shift+T to Reopen.
- `lbterm-reopen-tab`: a session runs `command('ai','open',sid)` through `background_child_process`, with `expect_tab('Reopening <label>')`. A shell returns nothing, so LBTerm's default handles it.
- `lbterm-conductor-hide`: for each session it runs `conductor hide|show <sid> --by lbterm --said "picked Hide from Conductor on <label>'s tab in LBTerm" --result <id>` through `background_child_process`. It reads the result with `conductor_new`'s timer, generalised, and a refusal comes up as a notification with its reason (his 1a of 2026-10-25).
  - As built (2026-11-09, stage 5 of landing 3, Terminal's feat/conductor-hide, beside the Conductor's pad in lbterm.lua): in a GUI with `LBTERM_STATE_FILE` (a test window) it returns nothing, so a test's own handler gets the event and no made-up session reaches the Conductor (PITFALLS 68). Otherwise every one of `sessions` that's a full session id (lower-cased, each once; anything else logged and left out) goes in one call, `conductor hide` or `show` `<sid> [<sid>...] --by lbterm --said "<his pick>" --result <id>`, one process a pick: never one a session, which `background_child_process` would start side by side and the Conductor's unlocked read-modify-write of hidden.json can lose one of, both results saying ok (PITFALLS 243; stage 5 built one a session, and the review's fix-up of 2026-11-09 made it one). Today's Conductor takes one id a call, so on a tab with two sessions the call is refused with a usage error and no result until it ships several (section 8, "What the Conductor shipped"): nothing hidden, the row back after 5 s, the notification after 20 s; `--said` is "picked Hide from Conductor on <label>'s tab in LBTerm" (Show to Conductor for a show) from the tab menu, or for an event with no `source` (a build before stage 3), and "pressed Show on <label> in the Conductor's view in LBTerm" from the view. An event without a boolean `hide` asks nothing and is logged. `conductor_new`'s timer was already general, `await_result` (it serves the agenda's presses too), and reads the one result: a refusal or failure, or no result in 20 s (looks at 4, 10 and 20 s), is one Windows notification, titled as LBTerm's strip says it ("The Conductor didn't hide <label>", "The Conductor didn't show <label> again"), with the reason, the silent one "Conductor didn't answer in 20 s. conductor hidden lists the sessions it has hidden."; a call that worked says nothing, the pad's glint saying it (stage 5's `o.worked`, which only the calls a session needed, went with them). A config reload in those 20 s loses the notification (PITFALLS 241); the hide is still done and the strip still says one that wasn't. Terminal's lbterm.Tests.ps1, "Hide from Conductor" (11 tests: a two-session pick one call with both ids, a refusal one notification, a session said twice named once), runs it with the wezterm module stood in for and a sandboxed results folder, nothing started; docs/lbterm-lua.md there has it, and Terminal's api.json names the commands and the results it reads.
- Comments: the launch_menu comment and the projects card's are updated.

LBTerm also gets `activate` (default true) in lua-api-crates/mux's SpawnTab. It goes through `spawn_tab_or_window_activating` and, on a mux domain, the SpawnNoActivate PDU that `cli spawn --no-activate` sends.

10. MOTION (every token from DESIGN.md and the skill; one new, `send`, for Lachlan's 6a of 2026-11-01, landing 3's send light)
- Pointer onto the +: the hairline goes from 14% to 50% and the + from 45% to 100%, and back from wherever they are: `button_hover`. The ring goes to pending: `hover`.
- A press on the +, either button: the ripple, clipped to the pill: `press`.
- The picker opening and closing: opacity only, from wherever it is: `pane_enter` and `pane_exit`. The pane's outline and the 85% text: `dim`.
- Typing, matching, Tab and digits: no motion (typing).
- The wheel in the picker: no motion (scrolling).
- The highlight moving: `hover`. The row's chips: `button_hover`.
- Enter with no match: `arrive_in`, `arrive_out` on the box's hairline.
- A pick in front:
  - The picked row flashes white 22%: `copy_in`, `copy_out`.
  - The card goes: `pane_exit`.
  - The Starting row's slot opens from the top: `pane_layout`. It fades in: `pane_enter`. The arc turns at 1.4 s a turn.
  - When the tab arrives, the row dissolves into the real one: `swap`. The marker glides: `focus_move`. The old tab fades over the new: `tab_switch`. The strip's words change: `swap`.
- A pick behind: the same Starting row with no switch. "Started <name>": `swap`. The pad glints on arrival: `copy_in`, `copy_out`.
- A start that fails or times out: the row fades where it is (`pane_exit`), the rows close up (`pane_layout`), and the strip's caption changes (`swap`).
- Any menu: in on `pane_enter`, out on `pane_exit`, and its highlight on `hover`. Wait on…'s page change: `swap`.
- A row picked in any menu flashes white as the picker's rows do, up to the press's 35% on `copy_in` and back on `copy_out`, over the card as it goes, not faded with it (his 9b of 2026-11-07; `menu.<slot>.flash`).
- The open tab closing as the row taking its slot slides up into it: the marker rides up with that row on its spring, `pane_layout`; it glides on `focus_move` when the open row doesn't move (his 3b).
- A key with nowhere to go in a menu: `arrive_in`, then `panel_glow` (his 9a of 2026-11-08; `arrive_in`, `arrive_out` before).
- New session, Shell or Reopen from a menu: the menu goes on `pane_exit`, then as a pick.
- Copy link: the pad glints: `copy_in`, `copy_out`.
- Rename: as built.
- Hide:
  - The menu goes: `pane_exit`.
  - The row takes its hidden look: `swap`.
  - Confirmed: the pad glints: `copy_in`, `copy_out`.
  - Not confirmed in 5 s: the row goes back and the strip says why: `swap`.
  - The light running up the rail from the tab's pad to the Conductor's, lit on `copy_in`, carried on `send`, out on `copy_out` (his 6a; a new token, built 2026-11-09).
- Show: `swap` and the light up the rail, then the glint when confirmed.
- Close finished tabs: each row fades where it was (`pane_exit`) while the rows below slide on one spring in one commit (`pane_layout`). If the open tab was among them: `tab_switch` and `focus_move`.
- Label lines dropping: `pane_layout`, with the lines fading on `pane_exit`.
- Motion off: every end state lands in the next frame. The Starting row, the marks and the pending looks still appear, since they're state. The arc holds still.

11. TESTS
- Cargo:
  - lbterm-panel layout:
    - the foot slot at 96 and 144 DPI;
    - `row_height` counting head and foot;
    - label lines dropping before `row_visible` fails (20 tabs in 900 px and 16 in 978 px all drawn);
    - "+N" only past 28 px rows;
    - the hit zone maximized and not.
  - lbterm-extras picker:
    - the match ranking (war, barg, hunt, lbt, btd);
    - the two Bargain Hunter first-message cases;
    - the order (Here, Home, recent, A to Z) without duplicates;
    - recent picks capped at 6;
    - digits only with an empty box;
    - Tab and no-match.
  - lbterm-extras menu:
    - the rows for a Claude tab, for a shell tab, without `hidden`, and with links off;
    - unique keys;
    - Close finished tabs, one case per exclusion;
    - the + menu from a launch_menu of 4 entries and of 0;
    - Reopen rows.
  - conductor: `hidden` parsed; pending, confirmed and expired.
  - lbterm-motion: plans for the Starting row (in, dissolve, fold away), the picker's fade, the menus' fade and the highlight's glide.
  - config: `new_tab`.
- Loaded or slow PCs (Lachlan, 2026-11-03, starting this landing: "the tests should take into account that the system is sometimes busy or at a different speed"): other agents' builds and test windows share the PC, and a warden job runs on 4 cores whatever their clock. So every test here judges by order and by frames, not by milliseconds: "in the next frame" is the frame the state file says came after the input (its frame number and what it shows), "in the same frame as the key" is the frame that answers the key, and a wait is on the state file or the counters (`Wait-State`, `Wait-Change`) with a deadline far past what a loaded PC takes, never a sleep followed by a look. Where time itself is what's tested (the Starting row's 20 s, the strip's 4 s notice), the test makes the time short through a test-only setting or allows it to come late, never early (PITFALLS 59), and the budgets' one-frame holds are counted in frames. A cargo test with a thread (the recent picks' reader and writer) waits for what the thread does with a generous deadline, never a fixed sleep (PITFALLS 60).
- e2e, with posted input and never the foreground:
  - New perf/new-tab.ps1 [-Mux]:
    - the pad from the state file, and its hover;
    - a click opening the picker in the next frame;
    - "war" highlighting Warden in the same frame;
    - "bargain hunter find a kettle" firing the event with the right name and prompt (the test handler writes it to a file);
    - a digit pick;
    - Enter putting the Starting row in the next frame, then the tab;
    - Shift+Enter leaving the window on its tab and the picker open;
    - Ctrl+Enter firing shell;
    - Esc giving keys back (send-text echo);
    - a handler calling `lbterm_expect_tab(nil)`: the row folds and the strip says so;
    - the + menu's 4 shell rows, with "Git Bash here" opening in the right folder;
    - Close tab, then Ctrl+Shift+T firing `lbterm-reopen-tab`;
    - 20 tabs all on the rail with the pad.
  - perf/tab-mouse.ps1 extended:
    - the new rows and keys;
    - Rename starting from the label;
    - Copy link's clipboard text;
    - Close finished tabs against stand-in sessions and objectives files, with an unread tab left open;
    - Wait on… firing `lbterm-tab-link`.
  - perf/conductor.ps1 extended (built on 2026-11-09 as perf/conductor-hide.ps1 instead, a config of its own so Lachlan's handler never runs, PITFALLS 68: every bullet but the view's group, with the mark judged from the state file's `hidden` until it's drawn):
    - button.json played without `hidden` (no item) and with `[]` (the item on a Claude tab, absent on a shell tab);
    - a pick firing the event;
    - the mark at once, then confirmed with a glint;
    - an unconfirmed hide going back after 5 s with the strip's notice;
    - Show;
    - the view's Hidden group from a played `seat.view`.
  - perf/spawn-no-activate.ps1: Lua `spawn_tab{activate=false}` on a mux domain.
  - perf/new-tab-lua.ps1 [-Mux] [-Config <lbterm.lua>] (built with section 9): his lbterm.lua in a
    window of the build, what his handlers would start written down instead and Claude a stand-in
    that writes its argv: his keys, a pick in front with a first message, behind, a shell, both
    kinds of quote, Reopen of a Claude tab and of a shell's, and his handlers stepping aside in a
    test window.
  - motion/capture.ps1 for the picker's and a menu's fade at slowdown 5: interrupted, and with motion off. motion/compare.ps1 still passes.
  - budgets.json gets two actions, each held to one frame:
    - "picker": from the press to the frame that shows it.
    - "pick": from Enter to the frame that shows the Starting row.
  - Landing 3 as measured (2026-11-09, its stage 6):
    - budgets.json's `rail-hidden` and `rail-hidden-open` at rest and `tab-menu-hide-opening` (`twelve-tabs` and `tab-menu-opening` with a stand-in Conductor's button.json and its `hidden` list, `Open-TwelveTabs -Hidden` in perf/budgets.ps1), with `twelve-tabs` and `tab-menu-opening`, held against main's build by `budgets -Against`, 5 rounds: every metric the same but `twelve-tabs`' task_ms (0.011 against 0.016 ms a second, the same tasks), the menu with its Hide row in the frame after the click in every run (PERF.md, "Hide from Conductor, by budgets -Against"). So the mark, the chip and the row cost nothing at rest and nothing measurable on the opening.
    - `lbterm.ps1 e2e` on the 78 scripts the branch's code picks (`-Changed` picked all 149, the harness having changed; perf-scripts.md), 02:39 to 04:10 with the PC unlocked and the displays asleep: 50 passed, perf/conductor-hide.ps1 among them; 5 flaky (at-rest on the control thread's one wake in 60 s, budget-log, conductor-view, rail-tasks-nocompositor, slow), each passing on its rerun; 23 failed, and main's build failed every one of the 23 the same way run straight after (04:10 to 04:57): pixel checks of what DWM plays, presents never seen on the screen, gui-thread's lock frames, and title-storm's 4% (PITFALLS 239). None was the branch's; what DWM plays, the send light's look with it, is still to be seen with the displays on.
    - Again with Lachlan at the PC and the displays on (2026-11-09 11:28 to 12:37, the right-most screen clear, one job at a time): the 28 scripts that judge what DWM plays, or failed asleep, on the branch's build (8a9bffe9) and then main's (0faee56b) straight after. The branch: 24 passed, perf/conductor-hide.ps1 among them, and 4 failed, present and present-mux (7 to 9 of 12 keys watched and never seen on screen) and title-storm and title-storm-local (a background tab's title 4.8 to 5.4% of a frame, budget 4%), each failing the same way on main's build, which also failed conductor-hide (main has no Hide row) and three scripts on a Chrome window the window check caught. So the branch has no failure of its own awake either; present and title-storm fail on main as they do here.
- Terminal, lbterm.Tests.ps1:
  - the picker's `new`: args, quoting, `--by`, `--said` and `activate=false`;
  - reopen running `ai open`;
  - the hide handler's command line (built 2026-11-09 on Terminal's feat/conductor-hide: lbterm.Tests.ps1's "lbterm.lua's Hide from Conductor", 12 tests, section 9; the whole file, 154, passed again in its worktree at stage 6, 02:35);
  - the keys only where `has_action` is true.

12. DOCS
- LBTerm: DESIGN.md (the + pad, the picker, the Starting row, the hidden mark, the menus' fill and fade), FEEDBACK.md (the new-tab and menu gaps closed), TAB-MOUSE.md (the menu), TAB-PANEL.md, STATE.md (the new fields) and .github/README.md.
- OBJECTIVES.md: correct the line that says New goes through Conductor's `new --show`. Since 2026-10-29 it's `open_claude_tab`.
- Terminal: docs/lbterm-lua.md.

## Landing 4: a quieter rail, parking in LBTerm, and Shift with the side buttons

Lachlan's 7a, 8a, 9a and 12a of 2026-11-01 ("Lachlan's decisions", below, with the questions they answer), which change the rail outside the new-tab work. Built from 2026-11-09 on feat/rail-landing-4, an lbterm-track run in build slot-5 in five stages: (1) this design and 12a, (2) 7a, (3) 8a's flag, its PDUs, what it publishes, the rail's look and the tab menu's row, (4) the hops (8a's skipping and 9a) and Terminal's half on its own feat/rail-landing-4, (5) measured against main's build and the e2e scripts run. Written before any of its code, from the code as main had it at f6cbaa328; each part says what changes, where, on which thread and how often, its motion (DESIGN.md's tokens: none new) and what the state file says. Where a choice is his, the obvious one is built and the question goes in the stage's report.

**12a. A quieter rail** (stage 1, built)
- The idle timer's bars are drawn only once they light. Until now each pad with room for the timer (`Layout::has_timer`) had its three bars drawn whatever they said, an unlit one in the dim `#24242C` (`palette::bar(Bar::Off)`, `UNLIT`): with 8 tabs at work, 24 grey stripes saying nothing. `lbterm_panel::idle::lit` gives the bars to draw, bottom first, the unlit ones left out, and render/tab_panel.rs's `tab_panel_pad` draws only those: none for a tab at work or without Claude (level 0), the bottom one green the moment Claude stops (level 1), two at level 2, all three from level 3 as the colours climb. It runs on the GUI thread inside the rows' kept quads (`chrome.rows`, and `chrome.open` for a row the open panel raises), made again only when a row's look changes (`rows_from`), so nothing more is spent a frame and a little less when they're made. A bar lighting changes the row's closed look (`look::looks` holds `bars`), so the row dissolves into it on `swap`, as it did from grey; with motion off it's there in the next frame.
- The pads' labels fade out while the panel is open. The open row leads with the same name (its title is the tab's label since 2026-10-25), so open, each name showed twice side by side. With the compositor the panel is always drawn open: its rows' pills, pads, numbers and labels in the rows' surface (z-layer 46, `ROWS_Z`), and its open parts in the words' surface (48, `ROW_WORDS_Z`), whose visual DWM fades in on `pane_enter` after `words_delay` as the panel opens and out on `pane_exit` from wherever it is as it closes (`rail.words`). So the words' surface covers each label: a rectangle of the pill's colour over the label's room (`Layout::label_cover`: the room under the pad, out to the pill's sides) for each visible row the open panel doesn't raise, and for the Starting row (`lbterm_panel::words::label_covers`), drawn in the open parts' kept quads (`chrome.open`) before the highlights. A raised row (the open tab, the row under the pointer: `fill`) has its long highlight drawn there whole already, over its label, and its label is no longer drawn again on it, where its pad, bars, number and mark still are. Each cover rides in its row's words visual, so it slides with its row and fades with the words: the labels go as the titles come in, on the words' own fade, and come back as they go, with nothing new to animate and no visual added. Without the compositor the words' layer (52) is drawn over the body (51) only while the panel's open, so the labels are gone while it is, at once, as everything there is. With motion off, at once. The Conductor's mode word under its pad and the foot's "+N" aren't a tab's name and stay. Made with the open parts' quads, only when the rows change: nothing a frame.
- His call, built the obvious way: the labels fade all the way out, not to a dim level; a lifted row being dragged is covered in the hover colour its pill has.
- The state file: each row's `bars`, the bars drawn bottom first (`"green"`, `"yellow"`, `"red"`; `[]` when none are lit or the row has no room for the timer), and the rail's `labels`, `shown` or `faded` (faded while `open`) (STATE.md).
- Tests: idle.rs's `lit`, layout.rs's `label_cover`, words.rs's `label_covers`; perf/rail-labels.ps1's new steps: no bar drawn over a working pad or a shell's, one over a pad two minutes into a question and three over one twenty minutes in, the labels' rooms bright closed and only the pill's colour open, the state file saying both. Passed on the branch's build (f935a5d76's code, 2026-11-09 15:49); the installed build (its exe written 2026-11-08 13:55) fails them, with 132 to 198 pixels of unlit bar over every pad and four labels left on the open panel.
- Seen (2026-11-09, 15:45 to 15:47, the PC unlocked and in use, input 0.1 s before; the right-most screen 98% under the DashboardsBox, which PrintWindow draws past): motion/panel-demo.ps1 at real speed and slowed five times on the branch's build, and at real speed on the installed build, kept in slot-5's `target/motion/quiet-rail-branch-1x`, `quiet-rail-branch-5x` and `quiet-rail-installed-1x`, with sheets from timesheet.py in the first: `rest-both.png` (the rail at rest three times as big, installed left: three grey stripes over the working pad and two over the question's one green, none on the branch), `open-branch.png` against `open-installed.png` (the opening at real speed, 300 to 1000 ms: the labels going from about 690 ms as the titles come and gone by 760, the installed build's staying), `open-x5-branch.png` (slowed: label and title cross over 190 to 360 ms of real time, the raised open row's label under its highlight as it comes up) and `close-branch.png` (closing: the labels back at half by 3,373 ms and whole by 3,414, as the words go on `pane_exit`). It reads as the label's name moving over into the title, nothing jumping; motion off isn't captured: the words' fade lands in the next frame with motion off, as it did.

**7a. The Conductor's pad, still in Accept most and Accept all** (stage 2, built)
- Before (main at f6cbaa328): `conductor::spectrum(level, hovered)` was true always in Accept most and Accept all (only under the pointer in read only), so render/tab_panel.rs's `tab_panel_rings` asked every paint for the spectrum's ring on the Conductor's pill, and the compositor showed it with a hue rotation DWM turns once round every 6 s for ever (`sync_rings`, `new_ring`, `HUE_PERIOD`; `conductor.outline` in the state file's `loops`). It was the brightest thing on the rail at rest, and while the window could be seen DWM composed a frame every refresh for it (PITFALLS 36; AT-REST.md).
- 7a, as built: at rest a still hairline in the mode's colour round the pill, yellow for most and red for all (`palette::MODE_MOST`, `MODE_ALL`, its label's colours), drawn in the body as read only's grey is (`tab_panel_conductor`); the spectrum's ring, with its glow and its turning hues, only while the Conductor acts, and, as in read only, while the pointer is on the pad. `lbterm_extras::conductor::outline(level, hovered, acting)` gives `Outline::None` (off), `Grey` (read only), `Mode` (most or all at rest) or `Spectrum`, in place of `spectrum(level, hovered)`; `hairline(level, outline)` the still line drawn in the body, kept in the mode's colour under the spectrum in most and all so the ring fades in over it and back out to it, and none in read only while the pointer's spectrum stands in its grey's place, as before.
- What acting is: its hub naming any session it acts on (`conductor.focus`'s `doing: "acting"`), here or in another window, with the least lap (`ACTING_AT_LEAST`, 1.4 s) and the linger (400 ms) the tabs' outlines keep, so a quick action still shows: `Focus::acting(now)`, read by `conductor_hub::acting()` under FOCUS's mutex once a paint of the rail (with a hub configured and the pad on the rail) into `TabPanelState::head_acting`. Thinking about a session (the iridescent outline) isn't acting. Nor is a turn of the Conductor's own under way: button.json has a `busy` (written since 2026-11-02) and the seat's `seat.view` one (the view's "working" word, `seat::protocol`), and neither lights the pad, so it lights rarely and then means it (his call, Q2 of stage 1's report). Read only takes no notice of acting: only what may act on its own lights by itself.
- Motion: the ring fades in over the still outline on `panel_glow` and out on `panel_glow` from wherever it is, its hues turning only while it shows (the ring visual's opacity, `Ring::fade_to` in compositor.rs, `conductor.outline.fade` among the state file's animations while it plays). The compositor removes a ring the plan stops naming at once, and the ring templates are drawn only while one is named, so `PadRing` (lbterm-extras) keeps the pad's ring named, lit or fading (`RingPlan::fade`), until a fade out on the paint's curve would have settled (`compositor::fade_out_lasts`: about 540 ms at real speed, worked out from the spring) and the rail's wake comes then (`ring_due`), whose paint takes it out. A ring made again for a change of `spin` (the window seen or not) starts from as much of the old one as showed; lit again while it fades, it fades back in from where it is. Read only's ring comes and goes with the pointer at once, as it did, and so do the tabs' (`ring_fades`). Motion off: there or gone in the next frame, still (`Curve::Instant`). Unseen: still (`loops_play`). Without the compositor the spectrum is drawn still in the hairline's place while it shows. No new token.
- The hub letting go: before this stage the hub's line letting go of a session changed nothing shown at once (it lingers), so nothing repainted and no wake was set, and an outline let go of stayed until some other paint came; the pad's spectrum would have stayed lit in DWM. `set_focus` now sets every rail's wake for the new end when only the end moved (`tab_panel_hub_wake`; PITFALLS 249; FLOWS.md, "The hub's word of what the Conductor is on, to the outlines"). That fixes the tabs' outlines too.
- At rest it costs DWM nothing: no `conductor.outline` in `loops` in Accept most or all while nothing is acted on (perf/conductor.ps1 holds it), and the GUI thread one more lock and a scan of the hub's few entries a paint of the rail. Measured at stage 5 (PERF.md, "Landing 4, by `budgets -Against`"): at rest with the pad accepting all and no hub, main's build kept one loop and the branch's none, the window about 7 MB and 7 handles lighter, and the GUI thread, the process and the server the same. Of the scripts that set Accept most, perf/conductor.ps1, dwm-loops.ps1, paint-cost.ps1 and budgets.ps1's `conductor-pad` setup (and the scenarios on it) have the hub acting on a session, so their pad lights and their loops are as on main; budgets.ps1's `Open-TwelveTabs -Hidden` (the `rail-hidden` scenarios) writes `most` with no hub, so its pad is now still where main's turned, and a before-and-after of those scenarios differs by that loop as well as by what it measures.
- The state file: the Conductor's `outline`, `none`, `grey`, `mode` or `spectrum`, and `acting` (STATE.md).
- Tests: conductor.rs's `the_spectrum_shows_while_it_acts_or_under_the_pointer_and_the_mode_is_still_at_rest` (`outline`, `hairline`, `ring_fades` in every mode), `the_pads_ring_stays_in_the_plan_until_it_has_faded_out` (`PadRing`: lit, the fade's end fixed when it lets go, lit again while fading, motion off) and `it_acts_while_any_session_it_acts_on_shows_and_thinking_isnt_acting` (`Focus::acting`, its least lap and `until`). perf/conductor.ps1's steps 2 and 3: most and all at rest a hairline in the mode's colour in a band left of the pad (20 yellow, 20 red pixels at 1x), none of the spectrum's blues round the pad, nothing moving in 700 ms, `outline` `mode` and no `conductor.outline` loop; thinking leaving the pad as it was; acting, the spectrum fading in (`conductor.outline.fade` seen), 353 of its blues round the pad and its hues turning, lit in all too, read only's grey kept with no fade; once the hub lets go, the fade out and the pad as it was at rest, no loop. Passed on the branch's build of 09c706174's code (2026-11-09 16:30). Its first run (16:13, on the build before `tab_panel_hub_wake`) failed two checks, each worked out before changing anything: the blues were counted in a band on the pad's left side, which the spectrum's hues, running once round the pill and turned by DWM, can leave all reds and yellows for a moment (the script's check: counted round the whole slot since), and the pad never went back to its still outline after the hub let go (the code's: PITFALLS 249).
- Seen (2026-11-09, 16:33 to 16:36, the PC unlocked and in use: 0 to 297 ms since real input as each capture began, but 7.4 s before the motion-off one; the displays awake): perf/conductor.ps1 `-CapturePad` on the branch's build, Accept most, at real speed, slowed five times and with motion off, and `-Interrupt` at real speed and slowed, kept in slot-5's `target/motion/conductor-pad-1x`, `-5x`, `-off`, `-turn-1x` and `-turn-5x`, each with `fade-in.png` and `fade-out.png` (the pad three times as big at real-speed times from each push), the turns with `turn-back.png` and `fade-out-again.png`. At real speed the hub's push at 300 ms shows from about 400 ms, the spectrum and its glow coming up over the yellow hairline and whole by about 520, its hues turning; let go at 1800, it holds its least lap and linger to 2200 and fades back to the yellow hairline between about 2260 and 2510. Slowed, both fades are a smooth cross-fade between the hairline and the spectrum, nothing jumping, no blank frame, the yellow hairline showing through as the ring goes. Acting again 150 ms into the fade out, the ring dips no further than about half and comes back up from there, slowed and at real speed, never from nothing. Motion off: the spectrum there at once between two frames (306 and 358 ms) and still, and gone at once (2213 to 2250). It reads at real speed as the pad lighting up while the Conductor does something and settling back.

**8a. Parking, a flag on the tab** (stage 3 built the flag, its PDUs, the published file, the rail's look, the tab menu's row and the action; stage 4 the hops, crash restore's note and Terminal's half)
- Today (Terminal's lbterm.lua, "side buttons, parking, Alt+S"): an empty file in `~/.claude/parked` named by the tab's active pane's server id (`lbterm-<id>`, `lbterm-try-<id>` in a try window), made or removed by Ctrl+Alt+D (`lbterm-park-toggle`). The side buttons' hops (`lbterm-tab-back`, `lbterm-tab-forward`) open one such file a tab, on the GUI thread, to skip the parked, and Terminal's statusline.ts looks for the same file for its box (`[✓] 💻 Enabled ^!D`, dim `[ ] 💻 Disabled` while parked). Nothing else reads it: the Conductor's "parked" is work parked in Later. A file named by a pane follows the pane, not the tab (a split moves it to whichever pane is active), and after a server restart, which numbers panes from 0 again, old files name the new server's panes. The rail doesn't show it.
- **Where the flag lives**: on the mux tab, in the process that holds its panes. `mux::tab::Tab` gets an `AtomicBool` beside its title (an upstream file: one field and two methods, `lbterm_parked()` and `lbterm_set_parked(bool)`, which notifies `MuxNotification::LbtermTabParked { tab_id, parked }` when it changes). Every window of a GUI shares its mux, so every window sees it, and with `MUX = true` the tab that holds it is the mux server's, so a GUI started again sees it too, mirrored on each window's client tab the way a tab's title is.
- **Between a window and its server**, as `TabTitleChanged` goes, compatible both ways as PITFALLS 188 and 159 ask: three PDUs from 7508 in codec's table. `GetParkedTabs` (a window to its server, nothing in it) and its answer `ParkedTabs { tabs }`, every parked tab of the server's; `TabParked { tab_id, parked }` both ways: a window's park or unpark (answered with UnitResponse, sent by `ClientDomain::lbterm_park` from wezterm-gui's `park.rs`; the window never sets its own copy, the server's push back does, as it does every other window's: this design first said the domain's subscriber would send the local tab's change on as TabTitleChanged's is, which for a flag that flips is a loop, PITFALLS 250), and the server's word of every change to every connection whose window asked `GetParkedTabs` (dispatch.rs; the connection's `Pending` notes it asked, as it notes SetShownPanes). A client domain asks after each `process_pane_list` (an attach, a reconnect, a resync) and sets its local tabs' flags from the answer (`remote_to_local_tab_id`), then hears each change. An older server answers ErrorResponse (`Pdu::Invalid { ident: 7508 }`, which a `codec::lacks_parked_tabs` reads as `lacks_shown_panes` does): that domain can't park, so the tab menu offers no Park tab on its tabs and the park action does nothing there but log it once; no file, no second way, and Lachlan restarts the server to have it. An older window never asks, so it's never sent a TabParked, a push it doesn't know, which would end its connection. Arms for `LbtermTabParked` in the four exhaustive matches on `MuxNotification` (wezterm-gui's frontend.rs and termwindow/mod.rs twice, the server's dispatch.rs; wezterm-client's domain.rs subscriber has a catch-all and leaves it alone), each beside TabTitleChanged's, and in mux's `lbterm_repaint::changed` (`Changed::Tab`), which paints the window holding the tab.
- **What ends it**: closing the tab, or unparking it. A GUI restart keeps it (the server's tab). A server restart ends every tab, and their flags with them. Crash restore (built in stage 4): the GUI's note (`session-<class>.json`, restore.rs's `note`) records `parked` with each tab (`TabNote::parked`, left out of the note while false, so a note from before 8a reads unparked), restore.rs's subscriber counting `LbtermTabParked` as a change of the tabs, and a tab it reopens from a parked note comes back parked: `lbterm_restore_tabs` opens it through `TermWindow::lbterm_spawn_tab_then` (termwindow/spawn.rs), which hands back the tab `spawn_command_internal` opened (it returns it since stage 4: upstream's returned nothing, its callers only looked for an error), and parks it with `park::set_parked` where `park::can_park` says its domain can, else logs that it reopened unparked. A tab still waiting from an earlier offer pairs with one of the note's own whether parked or not (`TabNote::same`), the note's own, the newer, saying which. (His call, built the obvious way: a tab reopened by crash restore comes back parked.) A tab dragged to another window keeps it (the same tab); a pane moved to a tab of its own starts unparked.
- **How the keys and lbterm.lua reach it**: an action, nothing read from a file: `LbtermPark` `'Toggle'`, `'Park'` or `'Unpark'`, on the window's open tab, bound to Ctrl+Alt+D in lbterm.lua where `wezterm.has_action('LbtermPark')`. `lbterm-park-toggle` stays an event, which LyreBird fires through the control pipe (CONTROL.md), its handler performing the action. Lua's `MuxTab` gets `lbterm_parked()` (lua-api-crates/mux's tab.rs) for a handler that wants it. No file under `~/.claude/parked` is written or read any more; the old ones are left where they are (his files).
- **Built in stage 3** (2026-11-09): the flag (`Tab::lbterm_parked`, `lbterm_set_parked`), `MuxNotification::LbtermTabParked`, the PDUs and their handling (MUX.md, "Parked tabs", has each step and function), the published file (mux's `lbterm_parked.rs`, started by the server's `spawn_listener` and the GUI's `async_run_terminal_gui`), the action and the menu's row (wezterm-gui's `park.rs`: `can_park`, `set_parked`, `lbterm_park_action`), Lua's `MuxTab:lbterm_parked()`, the rail's look and the state file's `parked`. Not in stage 3: crash restore's note, the hops, and Terminal's lbterm.lua binding Ctrl+Alt+D and `lbterm-park-toggle` to the action; stage 4 built all three (crash restore above, the hops and Terminal's half in 9a below).
- **The rail**: a parked tab's row is dimmed, its pad, bars, number, label and marks blended 40% of the way from what's under them and drawn opaque (PITFALLS 248), from `Tab::parked` (`panel_tab`, an atomic load a row a paint) into `Row::parked`, which is in the closed rail's look, so parking or unparking dissolves the row on `swap`; on the open panel a "Parked" chip after the project's (and Hidden's), as Hidden's is. The open tab parked keeps its ring and bar. Motion off: at once. (His call: how dim, and the chip.) As built: `palette::PARKED` (0.40) and `palette::parked(under, colour)`; render/tab_panel.rs's `tab_panel_pad` blends every colour it draws for the row (the lit bars, the tab's colour ring, the pad, the asks, quiet and extras rings, the spinner's still arc, the unread dot) toward `under`, the row's pill (its fill when open or hovered, else the rail's colour), and `lbterm_panel::words::dim_parked` does the same to the row's number, label and eye-off mark after `words::place`, before they're laid out; the chip is `words::PARKED_CHIP` in `palette::parked_word()` (Hidden's look). `parked` is in both of `look::looks`' hashes, so the closed row dissolves on `swap` and the words are laid out again. Nothing more a frame: the dimmed colours are in the rows' kept quads and the laid-out words, made only when a row changes. What the compositor draws for the rows is another matter (PITFALLS 255), as the review of 2026-11-09 found: the turning arc of "Waiting on N tasks" is one template in the Working green, placed by the plan's `spinners` on every row whose arc turns, and stage 3's dim reached only the arc drawn still without the compositor, so on his composited rail a parked tab with background tasks kept a full-strength arc turning on its dimmed pad, and a loop of DWM's for it. Since the fix-up a parked row's arc doesn't turn (`Row::arc_turns`): it's left out of `spinners` and of the template's choice and drawn still and dimmed in the body, with or without the compositor, so a parked row adds no loop. Of the plan's other per-row visuals: the progress fills on the open panel (drawn per row into their own kept quads, `chrome.progress`, and without the compositor in the body) are dimmed too, toward the row's fill or the card, the track under them as it is; the marker (the open tab's ring and bar) isn't the row's and stays lit; and the Conductor's outlines round the tabs it acts on or thinks about (`rings`, iridescent, from one template; `tab_panel_rainbow` without the compositor) are the Conductor's, not the row's, and stay as they are on a parked row.
- **The tab menu**: "Park tab", or "Unpark tab" on a parked tab, key P (free since landing 3), first in the section with Hide from Conductor and Wait on…, on every tab but the Conductor's, shells' too, and absent where the tab's domain can't park (menu.rs, `Item::Park`, `Facts::park`: `None` where it can't be parked, else whether it is, set by render/tab_menu.rs's `lbterm_open_tab_menu` from `park::can_park` and the Conductor's tab; `menu::park_words`). A pick: the menu goes on `pane_exit`, then `park::set_parked`, and the row dissolves on `swap` once the flag changes (at once on the GUI's own tab, at the server's word on a mux domain's).
- **What statusline.ts's box reads**: what the process holding the tabs publishes beside its socket, `<socket>.parked.json` (the daily server's `~/.local/share/lbterm/sock.parked.json`, a try or test window's server's its own, `gui-sock-<pid>.parked.json` for a GUI's own tabs): `{"pid": <its pid>, "panes": [<its id of every pane in a parked tab>]}`, written whole through a rename (retried for 250 ms while a reader holds the file, as `state::write_atomic`'s, and a write that still fails leaves the next look to write its list whatever it is: PITFALLS 256) by a thread of its own whenever the list changes (a park, an unpark, a pane added to or closed in a parked tab, a parked tab closed), and once, empty, as a server starts serving, so a crashed server's file never names the next one's panes; a GUI writes one only once one of its own tabs has been parked (its subscriber's `may_change` lets only a park through while the file names no pane, so with nothing parked a notification costs one atomic load; the list is worked out on the main thread, `look`, and written by the thread `lbterm-parked`). The file is left behind when its process ends: the server's is written over, empty, by the next server on that socket, and a GUI's `gui-sock-<pid>.parked.json` goes with its socket, at the next sweep after the GUI has ended. That sweep is upstream's: every GUI's update checker lists RUNTIME_DIR 10 s after it starts and daily (AT-REST.md, `update_checker`) and deletes any `gui-sock-*` over a second old that takes no connect. Until the fix-up it took the GUI's own file as a dead socket too, so the file went 10 s after any GUI started, its tab still parked, and wasn't written again (the review's probe: there at 4,068 ms, gone at 13,167); his daily `sock.parked.json` was never at risk. Since the fix-up the sweep counts `gui-sock-` and a pid alone as a socket (wezterm-client's discovery.rs, `lbterm_is_gui_sock`; PITFALLS 254), and takes a file LBTerm put beside one, `gui-sock-<pid>.<more>` (`lbterm_beside_gui_sock`), only once that socket is gone, swept now or before: a GUI's file lasts while the GUI runs and goes after it. statusline.ts reads `${WEZTERM_UNIX_SOCKET}.parked.json` and looks for `WEZTERM_PANE` in `panes` (both are the server's inside a pane): one small file read a run, no process walked, nothing asked of the GUI or the server. No file, no box: a server that publishes nothing can't say. The box keeps its words and `^!D` (his call whether it stays, now the rail shows it).
- Tests (stage 3): mux's `lbterm_parked` (the file's name and words, which notifications look again, a write replacing the file whole) and `lbterm_repaint` (a park paints the window holding the tab); codec's `lbterm_parked_tabs_round_trip` (the three PDUs under 7508 to 7510, TabParked naming no pane, `lacks_parked_tabs`); the server's `parks_are_told_only_to_a_client_that_asked` (lbterm_flow.rs, through dispatch's `notification_pdu`); config's `the_park_action_comes_from_lua_and_toggles`; the menu's `park_tab_is_first_in_its_group_on_every_tab_that_can_park`, and the every-mix test with Park among its bits; lbterm-panel's look (parked in both looks) and words (`a_parked_rows_chip_follows_hiddens_and_its_rail_words_are_dimmed`). perf/park.ps1 on the branch's build (2026-11-09 17:05, the PC unlocked and in use): the server's file empty as it served; Park tab (P) on two's menu; P parked it, the file `[1]`, two's pad 35,99,64, which is 40% of the way from the rail's 18,18,18 to its green 61,220,132, three's pad unchanged; a split in two `[1,3]`; the action parked the open tab, `[0,1,3]`, its bar's 171 accent pixels kept and its pad 48,32,34 as blended into its lit pill's 56,29,27; Unpark tab (P) unparked two, `[0]`, its pad back to 61,220,132; closing the open tab `[]`; a window with no mux domain wrote `gui-sock-<pid>.parked.json` naming its tab's pane 0. Seen in park-two-rail.png (two's pad, number and label dimmed, the others bright) and park-panel.png (the open panel, Parked chips on one and two after the project's). Again at 17:12 with no other window about: passed, the same numbers. With `-ServerBin installed` (a server from 2026-11-08, before 8a, 17:13): no file published, no Park tab on the menu, P going nowhere with the menu left open, the action doing nothing and the window's three rows still there, connected; a window of the installed build on this branch's server (it never asks, so it's never pushed) is held by the server's unit test only, not run (PITFALLS 159 asks for both pairings). Seen (2026-11-09 17:12, the PC unlocked and in use, 4.2 s since real input as the captures began): motion/capture.ps1 with F7 bound to `LbtermPark 'Toggle'` on the open tab of two plain shells, at real speed, slowed five times and with motion off, kept in slot-5's `target/motion/park-1x`, `-5x` and `-off`, with `park-sheet.png`, `park-x5-sheet.png` and `park-off-sheet.png` (timesheet.py, the rail twice as big) in the first. At real speed the open row's pad, number and label go dim from about 110 ms after the key and are settled by about 170, and come back between 1207 and 1266 ms after the second press at 1200: the row's `swap` dissolve. Slowed, a smooth cross-fade both ways (half way at about 60 ms of real time), the open tab's red ring and bar never changing. Motion off, dim or bright between one frame and the next. It reads at real speed as the tab stepping back, not a flash.
- Threads and cost: a park is an atomic set and one PDU sent from a task; the rail reads the flag at its paint, an atomic load a row; the server's publisher writes a few bytes on a thread when the list changes, rarely; nothing at rest.

**9a. Shift with the side buttons, and the hops in LBTerm** (stage 4, built)
- The hops moved out of Lua into LBTerm, so they read the flag rather than files: the action `LbtermTab` (config's `LbtermTab`: `'Back'`, `'Forward'`, `'Waiting'`, `'Last'`), performed by `TermWindow::lbterm_tab_action` (wezterm-gui's termwindow/hop.rs). At the press, on the GUI thread, `tab_panel_hops` (render/tab_panel.rs) gathers the mux window's tabs in its order, the Conductor's included: each one's parked flag (an atomic load), whether it's the Conductor's, and for a tab the last paint drew a row for, whether and since when that row waits on him (`lbterm_panel::waits_on_you`, the row worked out again from what it was drawn from, `lbterm_panel.drawn`, at now); with the open tab's place and the tabs the window showed, newest first. `lbterm_extras::hop::pick(tabs, open, how, recent)` names the tab, tested without a window, and `activate_tab` switches to it, the same switch a click on a pad makes. A few microseconds a press for a dozen tabs, no I/O, where each hop opened a file a tab; nothing at rest. FLOWS.md, "A side button's hop, from the press to the tab switch", has each step.
- `'Back'` and `'Forward'`: the nearest tab that way, wrapping, not parked, from the open one (parked or not); nothing when every other tab is, as the Lua hop did. The Conductor's tab, which can't be parked, is a stop as it was.
- Shift+Forward, `'Waiting'`: the tab that's waited on him longest by the rail's status times. `waits_on_you` says a row waits on him blocked (`Waiting`, the orange pad: Permission, Question, Login, Blocked; and a row waiting on another tab that asks him something too, `Linked` with `asks`) or finished (`Idle` with none of its own tasks still running, so Your turn, Done, Extras; and `Error`: Failed, or its link gone wrong), with its `since`, the time its status last changed, which its age and idle timer count from (`row_since`, the body of `row`, hands it back; `now` when there's none). Not at work, stuck, without Claude, waiting on another tab without asking, or handed back with its own background tasks running ("Waiting on N tasks", which carries on by itself). `since` is kept out of `Row`, whose equality decides whether a program's record repaints (PITFALLS 251). `hop::waiting` lists them, parked ones and the Conductor's left out, blocked first, oldest first, then finished, oldest first, a tie in the window's order (his call: strictly by age instead). From a tab in that list a press goes to the next one in it, so presses walk it and wrap; from anywhere else (a working tab, a parked one), to its first; with nothing waiting, or only the open tab, nothing happens, with no cue (his call whether there should be one). A window whose rail is off has no rows drawn, so nothing waits there.
- Shift+Back, `'Last'`: the tab he was on last that's still open and not parked. Each window keeps the tabs it showed, newest first (`hop::Recent`, `TabPanelState::recent`), the open tab put in front by `tab_panel_tabs` at each paint that shows another (one compare a paint while it's the same tab; the window's tabs walked, and those no longer in it dropped, only when it isn't), beside `seen`. So a second press goes back again, as Alt+Tab does, and a parked or closed tab is passed over for the one before it. Upstream's `ActivateLastTab` keeps one tab, which may since be parked or closed. Kept only while the rail paints, as `seen` is.
- His lbterm.lua (Terminal's feat/rail-landing-4, stage 4) binds the side buttons straight to the action where `wezterm.has_action('LbtermTab')` and `has_mouse_button('Back')`: Back and Forward with no modifiers, and with `mods = 'SHIFT'` (`lbterm_side_button` looks the binding up with the event's modifiers, and Windows' `MK_SHIFT` in the button's wParam gives Shift, so a posted press carries it too); Ctrl+Alt+D to `LbtermPark 'Toggle'` where `has_action('LbtermPark')`, and to `act.Nop` where it isn't (the review's find: unbound, on a rollback or with Terminal landed first, the key would have reached the pane as ESC and ^D; on main it was always bound, to the event). `lbterm-tab-back`, `lbterm-tab-forward`, `lbterm-tab-waiting`, `lbterm-tab-last` and `lbterm-park-toggle` stay events, each performing its action, for LyreBird (CONTROL.md). Its `hop` function, its marker files, its test-window guard on Ctrl+Alt+D (a test window parks its own tab now) and `PANE_PREFIX` are gone. Terminal's statusline.ts reads the published `<socket>.parked.json` (`parkedHere`), and its contract uses `@Terminal/code/LBTerm:parked`, which LBTerm's `api.json` publishes.
- Motion: a tab switch's own (`tab_switch`, `focus_move`); nothing new. The state file: nothing new (`active_tab` says where a hop went), and each row's `parked`.
- Tests: hop.rs's `back_and_forward_take_the_nearest_tab_not_parked_and_wrap`, `every_other_tab_parked_goes_nowhere`, `waiting_goes_to_the_oldest_blocked_on_him_then_the_oldest_finished`, `waiting_ties_go_in_the_windows_order_and_nothing_waiting_goes_nowhere`, `last_goes_to_the_newest_tab_shown_still_open_and_not_parked` (the last tab closed too) and `recent_keeps_the_tabs_shown_newest_first_so_last_goes_back_and_forth`; claude.rs's `a_row_waits_on_you_blocked_or_finished_since_its_status_changed`; config's `the_tab_hop_action_comes_from_lua`; restore.rs's `a_parked_tab_is_noted_parked_and_pairs_with_itself_unparked`. perf/hops.ps1 on the branch's build (2026-11-09 17:45, the PC unlocked and in use): five tabs of the GUI's own, one working and open, two a question two minutes old, three finished twenty minutes ago, four a question ten minutes old, five working; Back from one wrapped to five, Forward to one and two; two parked, Forward from it went to three, Back from three to one and Forward from one to three past it; Shift+Forward from three to four and again to three, and from one to four; Shift+Back from four to one and back to four; with three, one and five parked too, Shift+Back from five passed them to four, and from four Back, Forward and Shift+Forward stayed. Terminal's lbterm.Tests.ps1 (150 passed in its worktree, against the installed LBTerm, with `has_action` and the two actions stood in for a build that has them and one that doesn't) and statusline.test.ts (80 passed, `WEZTERM_UNIX_SOCKET` kept out of the child's environment so no test reads his file).

**Measured and run, as it lands** (stage 5, 2026-11-09; main merged in at 310b75e42, its e2e rule of naming the scripts with it, and Terminal's main into Terminal's branch, Landing below):
- Against main's build (PERF.md, "Landing 4, by `budgets -Against`"; 18:09 to 18:27, unlocked, Lachlan at the PC): `rail-parked` and `rail-parked-open`, new for it (the twelve tabs with the pad accepting all and no hub, the second tab parked, closed and with the pointer on the first pad), and `tab-menu-opening`, 5 rounds. At rest the GUI thread, the process and the server cost the same, the pad's loop is gone (`loops` 1 to 0, lower) and the window holds about 7 MB less (412 to 405 closed, 413 to 405 open, lower) and 7 handles fewer open (lower), the spectrum's ring out of its visual tree the likeliest reading (not traced). That a park costs nothing at rest is read from the code, not from these numbers: main's build can't park, so its rounds parked nothing (no F7 sent) and turned the pad's spectrum, and the comparison is main with no park and a turning pad against the branch with a park and a still pad, which isolates neither. In the code a park adds, at rest, an atomic load a row a paint (`panel_tab`) and nothing else; the publisher runs only on a notification. The tab menu's opening, its Park row on the branch's, is the same in every metric (ms 1.42 and 1.53, +-0.518; GUI thread 3.09 and 3.03, +-0.382). `missed` went unmeasured, the right-most screen under the DashboardsBox.
- e2e, the 20 scripts picked from `e2e -Changed`'s 134 candidates as judging what landing 4 touches (rail-labels, conductor, park and park-serverbin-installed, hops, restore and restore-mux, menu-keys, conductor-hide, tab-mouse-mux, dwm-loops-mux, pad-waiting with its no-compositor variant, mux-reconnect, tab-switch-mux and both its installed pairings, new-tab-mux, spawn-no-activate, last-tab-mux, state), on the branch's build in 23.5 min: 17 passed, tab-switch-mux-serverbin-installed flaky (with motion off tab two differed from motion on over x 75 to 1181, y 701 to 734, the window's last rows, not the rail; passed again), 3 failed. state and tab-mouse-mux listed the tab menu's rows without Park tab: both scripts now expect it on a build that parks (`Park-Row` in tab-mouse.ps1), and state, tab-mouse and tab-mouse-mux passed on the rerun. restore-mux failed twice at the tab opened with the prompt up not reaching the window's note in 10 s; main's script on main's build fails it the same way (once in two runs each, interleaved), so it's main's, not traced (perf-scripts.md, restore). An installed (older) window on this build's server, the pairing stage 3 didn't run, passed (tab-switch-mux-guibin-installed: the server never pushes a park to a window that didn't ask, PITFALLS 188), and an older server under this build's window passed (park-serverbin-installed: no Park tab, nothing published, the window still connected; tab-switch-mux-serverbin-installed, flaky as above).

**Questions for Lachlan** (gathered at stage 5 from the stages' reports; each is built the first way until he answers):
- 12a: the labels fade all the way out while the panel is open (built), or dim to about 35%?
- 7a, what lights the pad: only the hub acting on a session (built), or also while it thinks about one, or while a turn of its own runs (button.json's `busy`)?
- 7a, the pointer on the pad: in Accept most and Accept all it now fades the spectrum in on `panel_glow`, where read only's comes and goes at once as it always did (built), or should read only's fade too, or Accept's come at once?
- 8a, the look: a parked row dimmed 40% of the way to its pill's colour, with a Parked chip on the open panel (built): how dim, and the chip or not?
- 8a, the tab menu: Park tab first in the group with Hide from Conductor and Wait on…, on shells' tabs too (built), or somewhere else?
- 8a, crash restore: a tab reopened from a parked note comes back parked (built), or unparked?
- 8a, the statusline's box (`[✓] 💻 Enabled ^!D`, dim while parked), now read from LBTerm's file: keep it (built), or drop it now that the rail shows parking?
- 9a, Shift+Forward's order: tabs blocked on him first, oldest first, then finished ones, oldest first, rows waiting on their own background tasks and plain linked rows left out (built), or strictly by age?
- 9a, nothing waiting: Shift+Forward does nothing (built), or a cue that says so?

Changes he'd notice that he didn't ask for: a tab's Conductor outline goes on time, about 0.4 s after the hub lets go, where it stayed until some other paint came (PITFALLS 249); a tab parked in one window is parked in every window of the server and stays parked across a GUI restart (the markers followed a pane id); Ctrl+Alt+D in a test window parks that window's own tab (it did nothing there); and on a build without the actions the side buttons and Ctrl+Alt+D do nothing, with no file-based hop left to fall back on (Landing, below).

**Landing** (PITFALLS 121). LBTerm first, installed and the GUI restarted, then Terminal's branch: its lbterm.lua binds the new actions only where the build has them, but its file-based hop is gone, so on the installed build of today the side buttons would hop nowhere. Park tab needs the mux server restarted (an older server can't park, and the menu offers nothing until it can), and until then the statusline shows no box. Restarting the daily mux server ends every tab (MUX.md), so it waits for Lachlan's word, and crash restore brings back what it can. Terminal's main gained the status line's first stage on 2026-11-09 (claudehook's `statusline` verb: statusline.ts's last line served while nothing it reads has changed, the old park markers among what it looked at), so stage 5 merged Terminal's main into its feat/rail-landing-4 and put the published `<socket>.parked.json` in that signature in their place (`statusline::parked_file`; without it a park showed in the box only once the minute or the transcript moved), with claudehook.exe rebuilt on the branch: a later claudehook.exe on Terminal's main meets it as a binary conflict, which is settled by building again from the merged source (`shared/claude/build.ps1`).

## Where the designs agreed

All three designs agreed on these:

**The layout**
- No wholesale redesign. Keep the rail, its pads and labels, the strip's two rows, the scroll and objectives strips, and the Conductor's pad at the rail's head.

**What's wrong today**
- There's no new-tab button. The tab bar's + is hidden, and lbterm.lua's comment about right-clicking it points at nothing.
- Ctrl+T opens a bare PowerShell, while most new tabs are for Claude.
- The only way to choose a project is the projects card, at the window's far edge, behind a rest, with no search.
- A new tab shows nothing until the server answers (FEEDBACK.md's top gap).
- On a Claude tab, "Duplicate tab" opens a shell and duplicates nothing.
- Parked tabs look the same as every other tab.

**The new-tab button**
- A + on the rail in the InMail button's hairline look. Left click opens a picker of the projects, reusing the projects card's rows (session dots, mail) and its lbterm-project-action event and handler (open_claude_tab, then Conductor's record new).
- Typing filters the way @names resolve. Enter opens Claude in front. "Here" (the open tab's project) is highlighted first. Ctrl+Enter opens a shell there instead. Esc closes it.
- Right click opens a menu with the shells from config.launch_menu, Reopen closed tab (from what LBTerm knows as it writes close marks, resumed through the Claude project's ai open), and New window.
- A dim "Starting" row grows in at the end of the tabs the moment a tab is asked for.

**The tab menu**
- New: "New session in <project>", Hide from Conductor, and "Close finished tabs" (in two of the three).
- Kept: Rename, Colour, the link rows, Close other tabs and Close tab.

**Hide from Conductor**
- Keyed by Claude session id, so it survives restarts and resume, and a fork starts visible.
- The Conductor stops being woken by the session and stops acting on it. Its tools refuse it, and it gets no outline.
- Undone from the same menu item, from a folded Hidden group in the Conductor's view, or with conductor show.
- Tab links aren't affected, and the rail still shows orange when the session waits on him.
- The Conductor builds it, asked by InMail. LBTerm offers nothing until the Conductor publishes it.
- The mark shows at once and stays only when the Conductor confirms. Otherwise it goes and the strip says why.

**Motion**
- The picker fades in and out on opacity only, and typing animates nothing.
- Menus fade on pane_enter and pane_exit, with the highlight gliding on hover (DESIGN.md's planned item).
- The Starting row grows in on pane_layout and pane_enter, then dissolves into the real tab on swap, focus_move and tab_switch.
- Hiding dissolves the row on swap.
- The press ripple is on the +.
- With motion off, every end state lands in the next frame.

**Keys**
- Ctrl+Shift+T reopens the last closed tab.
- Shift+Enter in the picker starting a tab behind needs Lua's spawn_tab to take activate = false (proposed in two of the three).

## Where they disagreed, and what was picked

Each point says what each design proposed and what I chose.

1. **Where the + sits.** Speed and Subtraction: fixed at the rail's foot. Motion: right after the last tab, in the drop hint's slot.
   - Chose the foot. It never moves, and on his maximized portrait window it's the true bottom-left corner of the screen.
   - Motion's "the + becomes the tab" survives as the Starting row growing in at the end of the tabs. The drop hint stays where it is.

2. **Whether the panel opens on a + click.** Motion: open the panel at once and slide the picker out of its edge. Speed and Subtraction: put the picker beside the rail.
   - Chose beside the rail, with the panel held as it is while the picker is open.
   - lbterm.lua turns his wave on (wave.enabled = true). Opening the panel on every click would play the wave and press the panes over about 30 times a day.

3. **The picker's look.** Speed and Subtraction: the menus' look. Motion: the cards' look.
   - Chose the menus' look. DESIGN.md puts the InMail list there, and it's the same thing: a click, a list, a pick that opens a tab.

4. **The order of the picker's rows.** Speed: Here, Home, recent, then A to Z. Motion: busiest groups first. Subtraction: only projects with mail or sessions, at most 12 rows.
   - Chose Here, Home, the last 6 picks, then A to Z, with dots and mail shown but never moving a row.
   - The projects card reordering itself between looks is one of the complaints. Speed's data shows the 5 most recent projects cover only 39% of starts, so typing is the fast path and positions should hold still.

5. **Where the box sits.** Subtraction: at the top. Speed and Motion: level with the +.
   - Chose the bottom, beside the pointer, with the list growing upward and row 1 nearest the box.

6. **Reaching a row's InMail and Resume.** Subtraction: Left and Right across the row's chips. That clashes with the caret in the box.
   - Chose Alt+M and Alt+R, with the chips shown on the highlighted row.

7. **First message typed after the project name.** Only Speed proposed it.
   - Kept it. The mechanism is already there (open_claude_tab's prompt).
   - A strict rule decides where the name ends and the message starts: a following word joins the name only if it's the next whole word of the name. A preview line shows the message before Enter.

8. **Ctrl+T.** Speed (70) and Motion (60): open the picker. Subtraction: ask him.
   - Built as the picker, because his own words were "the new tab should ask where... the context menu will show things like open a new shell".
   - Ctrl+Alt+T keeps PowerShell one key away. Kept as decision Q2.

9. **The hide mark.** Speed: eye-off at the bottom-left. Subtraction: eye-off at the bottom-right. Motion: a struck Conductor glyph at the top-left.
   - Chose eye-off at the bottom-right. Motion's own notes say a struck glyph is barely legible at 10 px. The top-right is taken by the arc and the unread dot, and link marks sit left of the pad.
   - On the open panel it's a "Hidden" chip (Motion and Subtraction), not words on the second line (Speed).

10. **Where the hidden list lives.** Speed: a new published @Conductor:hidden file. Motion and Subtraction: a field in button.json.
    - Chose button.json. LBTerm already watches it, and the field being there is how LBTerm knows the Conductor can hide sessions.

11. **How long the mark waits for the Conductor.** Speed and Subtraction: 5 s. Motion: 10 s.
    - Chose 5 s, the tab links' PENDING. Refusals come back with their reason through --result.

12. **Hide on a shell tab.** Speed and Motion: shown dim with a reason. Subtraction: absent.
    - Chose absent. Claude-only rows don't belong on a shell tab's menu.

13. **Filtering on LBTerm's side.** Speed: LBTerm drops the hub's outlines for hidden sessions.
    - Not built. It would hide a Conductor bug, and his no-fallback rule applies. The Conductor's tests hold that promise instead.

14. **The light that runs along the rail on hide and show.** Only Motion proposed it.
    - Left out of the build and asked as taste (decision Q6).

15. **What hiding also stops.** Speed and Motion: estimates and reviews too. Subtraction: keep them.
    - Built as stopping them. Asked as decision Q4.

16. **A hidden session's questions.** Subtraction and Motion: still shown to him.
    - Built that way, answerable by his own press. Asked as decision Q5.

17. **Parking.** Motion and Subtraction: draw parked tabs and add "Park tab" to the menu. Speed: drop parking or move it into LBTerm.
    - Left out of the build. It's unused and file-based, so it's decision Q8.

18. **Close finished tabs.** Speed and Subtraction: add it.
    - Built beside Close other tabs. It's absent when there are none, and it also leaves out tabs with an unread result, so nothing he hasn't read gets closed.

19. **Extras only one design proposed.** Speed: Interrupt, middle-click close, the "needs you" pill, Shift with the side buttons. Subtraction: the idle bars and duplicate labels. Motion: the calmer Conductor pad (Subtraction agreed).
    - None of these are in the build. Each is a decision.

20. **Rail overflow.** Subtraction: drop label lines 2, then 1, then 0. Motion: then scroll. Speed: not needed.
    - Built the label-dropping, since the + takes a slot and tabs must never vanish silently.
    - Past that, a "+N" count instead of scrolling. On his window that point is never reached.

21. **Showing the keyboard went to the picker.** Motion: the active outline glides into the box, which would be new compositor work.
    - Chose the pane dimming on dim and the box's hairline going to 24%, as in the Conductor's view. Only existing tokens.

22. **The + menu's contents.** Motion: Claude in <project>, Resume a conversation…, one Reopen row. Speed: a chip row of other shells, five Reopen rows.
    - Dropped "Claude in <project>": + then Enter does it.
    - Dropped "Resume a conversation…": Claude Code's list shows Claude's own titles, not his labels.
    - Shells are full rows from launch_menu, not chips.
    - Up to five Reopen rows.

23. **Menu glow banding and the rename bug.** Found only by Motion.
    - Both fixed in the build: the glow at its current strength, and Rename starting from the row's label as TAB-MOUSE.md already says.

## Lachlan's decisions (asked 2026-10-31, answered 2026-11-01)

His answers, on 2026-11-01: 1a, 2a, 3b, 4a, 5a, 6a, 7a, 8a, 9a, 10a, 11a, 12a, 13a. Shares are the synthesis's.

Beyond the build spec, they add:
- 6a: the light running along the rail from the tab to the Conductor's pad on a hide or a show, a new 600 ms motion token (`send`). With landing 3: built on 2026-11-09, its stage 4 (section 8, "The send light").
- 10a: a middle-click on a rail row closes that tab without asking (Ctrl+Shift+T reopens it). With landing 2's tab menu.
- 7a: the Conductor's pad in Accept most and Accept all a still outline in the mode's colour at rest, its spectrum and glow only while it acts. With landing 4: built on 2026-11-09, its stage 2 ("Landing 4", 7a).
- 8a: parking kept, moved into LBTerm as a flag on the tab (dimmed on the rail, Park tab in the tab menu, no files under ~/.claude/parked); the side buttons skip parked tabs from the flag. With landing 4: built on 2026-11-09, its stages 3 and 4 ("Landing 4", 8a).
- 9a: Shift+Forward side button to the tab that's waited on him longest, Shift+Back to the tab he was on last. With landing 4: built on 2026-11-09, its stage 4 ("Landing 4", 9a).
- 12a: the idle timer's bars drawn only once they light, and the pads' labels faded while the panel is open. With landing 4: built on 2026-11-09, its stage 1 ("Landing 4", 12a).
7a, 8a, 9a and 12a change the rail outside the new-tab work: a landing 4 of their own after landing 2, or wherever the build finds them cheapest, each with its lbterm.lua part where it has one. 11a and 13a keep things as they are, and 1a, 2a and 3b were what the build already did.

1. Should the tab bar's layout be redesigned?
   - 1a. Keep the rail and the strip, and move the actions (what the build does) (80): Your window is short on width, not height: 16 tabs leave about 470 px of rail empty. All three designers came to this on their own.
   - 1b. Tabs across the top, Chrome-style (10): Familiar, but 16 tabs across 1,080 px get about 60 px each and lose their labels and status words.
   - 1c. A wider rail with each title beside its pad (10): Names readable in full (labels get 7 characters a line today), but it costs pane width all day. On 2026-10-20 you picked labels under the pads over this.
2. Ctrl+T is built to open the picker. That's one binding in lbterm.lua to change. Keep it?
   - 2a. Ctrl+T opens the picker, Ctrl+Alt+T opens PowerShell here (70): Most new tabs run Claude (32.6 starts a day in history.jsonl). Ctrl+T then Enter opens Claude where you are.
   - 2b. Ctrl+T stays a plain PowerShell, and the picker moves to Ctrl+Alt+T (30): Nothing to relearn. I read only history.jsonl, not how often you open plain shells.
3. The projects card and its pad (the four squares at the top of the objectives strip), now that the picker does the same job beside the tabs
   - 3a. Retire it, so the + is the only way to start a session (50): It's about 1,000 px from where your pointer is, opens only after a hover, has no search and reorders itself. The Conductor's log shows 7 New presses in about 5 days, against about 160 starts.
   - 3b. Keep it as it is, as a second way in (30): You picked it on 2026-10-24, and its view of every project's sessions and mail stays one hover away.
   - 3c. Keep it as an overview only, without its buttons (20): Keeps the at-a-glance view and drops the second launcher.
4. What else hiding a session from the Conductor stops. The build and the InMail say all three.
   - 4a. Estimates, Sonnet's reviews and the RSI report skip it too (60): Hidden reads as not watched and not spent on. That tab's guessed progress bar goes.
   - 4b. Only wakes and acting stop, and estimates, reviews and RSI carry on (40): The guessed bar stays, and the reviewer still catches a broken WATCHDOG rule.
5. A hidden session's questions in the Conductor's view and on Relay's phone screen
   - 5a. Still shown to you, in the Hidden group, and you can answer them (60): They're your questions. Only the Conductor's Claude is kept away, and hiding a session shouldn't stop you answering it from the phone.
   - 5b. Not shown: hidden means gone from the Conductor's screens too (40): The simpler meaning, and at the PC the rail's orange still tells you it's waiting.
6. A light running along the rail between the tab and the Conductor's pad when you hide or show a session (a new 600 ms motion token, send; not in the build). This one's your call.
   - 6a. Add it (50): Your call, since it's taste. It shows a request going from the tab to the Conductor. It's 600 ms because you couldn't see a 120 ms wave and kept a 0.8 s one.
   - 6b. Leave it out: the mark and the confirming glint are enough (50): Your call, since it's taste. Hiding is rare, and the mark and glint already show it worked.
7. The Conductor's pad in Accept most and Accept all
   - 7a. A still outline in the mode's colour at rest, with the spectrum and glow only while it acts (60): In the screenshots it's the brightest thing on the rail, brighter than a tab waiting on you. DESIGN.md's principle 6 is calm by default.
   - 7b. The spectrum all the time, as now (40): You picked it on 2026-10-21, as a constant reminder that it may act on its own.
8. Parking (Ctrl+Alt+D, and the side buttons skip parked tabs). This turns on whether you still park tabs: ~/.claude/parked is empty or missing, and every side-button hop opens a file per tab on the GUI thread.
   - 8a. Keep it, moved into LBTerm as a flag on the tab: parked tabs dimmed on the rail, Park tab in the tab menu, no files (50): If you park tabs, you can then see what the side buttons skip, and hops stop touching the disk.
   - 8b. Drop it (50): If you don't park tabs, the side buttons lose a hidden rule and the file reads go.
9. Getting to the tab that needs you when 16 are open
   - 9a. Shift+Forward side button goes to the tab that's waited on you longest, Shift+Back to the tab you were on last (50): You switch tabs with the side buttons. This takes no screen space, and Shift with them does nothing today.
   - 9b. Those, plus a 'needs you' pill (orange dot and count) in the strip beside the InMail button (20): A fixed one-click target, but it's one more thing on the strip saying what the orange pads already say.
   - 9c. Neither (30): Spotting orange works at 16 tabs, with the side buttons stepping one tab at a time.
10. Should a middle-click on a rail row close that tab without asking (Ctrl+Shift+T reopens it)?
   - 10a. Yes (60): Clearing finished tabs takes one click each, as in Chrome.
   - 10b. No, only the menu's Close tab (40): A slip of the wheel button would end a running Claude session.
11. Interrupt Claude in the tab menu (sends Esc to a working tab without switching to it)
   - 11a. Leave it out (60): The rail's state can be a moment out of date, so the Esc can arrive after the session has already stopped. The Conductor's stop already exists.
   - 11b. Add it (40): Stops a session that's gone off course from the rail, without leaving the tab you're in.
12. A quieter rail (from the subtraction design)
   - 12a. Draw the idle timer's bars only once they light, and fade the pads' labels while the panel is open (40): With 8 tabs that's 24 grey stripes saying nothing at rest. With the panel open, each name shows twice side by side.
   - 12b. Only the idle bars (30): Removes the clutter at rest and leaves the panel as it is.
   - 12c. Leave both as they are (30): You picked the three-bar timer and the labels, and neither costs anything.
13. A tab opened from the InMail button
   - 13a. In the background, as now (60): You picked this (1b) on 2026-10-23: you can queue InMail reads without leaving your tab.
   - 13b. In front, through the picker's handler, so every start goes the same way (40): Right now the two routes behave and fail differently: Conductor's new with a 20 s result check, against a single spawn.

Two more, on landing 1's fix-up (asked and answered 2026-11-04):
- The picker's first open after a GUI start is cold, 24.5 to 26.8 ms with two projects (42 to 95
  ms in the review's probes with his 49, before main's fonts-core came in): left as it is (his 3a
  of 60 against warming it once the GUI goes idle, 40), since it's once a start, measured on its
  own (`picker-first`), and warming it would put the same work into every window, those that never
  open the picker too.
- "home" and "index" typed in the picker say "No project called home", where Blueprint's resolve
  says they're old names for Root: left as it is (his 4a of 60 against saying the name to use, 40,
  which needs `names.lua` to carry Blueprint's renames).

Landing 2's, on its track's questions (asked 2026-11-05, answered 2026-11-07): 1a, 3b, 7b, 8b, 9b, and as built 4a, 5a, 6a, 10a and 11a; 2a made short stages every track's default (lbterm-track's, not this feature's). They mean:
- 1a: the slow tab-menu opening was traced with the runs' own metrics and fixed before landing: 20 of the branch's 75 openings had spent 40 to 172 ms of the GUI thread within the same 2 frames where main's never passed 6. It was the test window's timing to the screen arming a timer for a time already past (present.rs, main's code, off in his window), fixed by `Episode::next_look` (FLOWS.md, "Opening, measured whole"; PITFALLS 164).
- 3b: when the open tab closes and the next row slides up into its slot, the marker rides up with that row on its spring, so the ring stays on it the whole way, by Ctrl+W, the middle click and Close finished tabs alike; it had waited in the slot, as main's did for Ctrl+W. When the open tab is the last row it still glides up to the new open row (`lbterm_motion::rail::marker`; FLOWS.md, "The tab menu's facts", step 10).
- 7b: an open menu takes every key. A key bound in his config waits until the menu is closed, and a key the menu doesn't use brightens its hairline once, as a key no row takes does; every bound key acts again once the menu has closed, a fading-out one included (FLOWS.md, "A key while a menu is open", step 1; PITFALLS 150).
- 8b: New session from the tab menu counts as a recent pick in the picker, as a pick in the picker does (`newtab::pick`).
- 9b: a picked menu row flashes white, as the picker's rows do, in every menu: the tab menu, the + menu, the Conductor's, the InMail list and the prompt list (`lbterm_menu_flash`, `menu.<slot>.flash`). The agenda's rows, which open in place, and its chips' presses flashed nothing: that was the build's reading of "in every menu", not his word; asked, he said they flash too (his 6 of 2026-11-08, below).
- As built: 4a (the lit row's highlight over its words), 5a (the InMail list, the agenda and the + menu share one fade slot, and from the merge of 2026-11-07 until it went on 2026-11-08 the context pill's list too), 6a (Enter with nothing lit lights the first row), 10a (Close finished tabs closes only the tabs its line named that are still finished), 11a (Wait on…'s small square in the pad's colour).

Landing 2's follow-up, on its review's questions (asked 2026-11-08, answered 2026-11-08): 5a, 6 yes, 7b, 8b, 9a, 10a, 11a, 12 yes. They send the branch back once more before it lands, and mean:
- 5a: a key an open menu shows beside a row as that row's shortcut picks that row while the menu is open, with the row's flash, so the menu does what it says. Today that's the + menu's two (`newtab::plus_hint`: Ctrl+Alt+T beside the first launch entry, PowerShell 7 here, and Ctrl+Shift+T beside Reopen's newest row); no other menu names a bound key. Only the key the row shows: a second key bound to the same action, and every other bound key over an open menu, stays as his 7b has it, a key no row takes (brightening the hairline when he answered, flaring the card's edge since his 9a). Built on 2026-11-08 (stage 3): the key's binding is looked up in the default key table by the forms the binding passes use, and when it's `LbtermNewTab` 'Shell' or 'Reopen' and the key, named as the menu names keys, is what a row of the open + menu shows, that row is picked as its letter would be (`newtab::plus_by_shortcut`; render/menu_fade.rs `lbterm_menus_binding`, render/new_tab.rs `lbterm_new_tab_binding`; FLOWS.md, "A key while a menu is open", step 2; PITFALLS 185).
- 6, yes: the agenda's rows and its chips flash white when pressed, as every other menu's pick does (9b), from the same motion token. Built on 2026-11-08 (stage 4): every line a press acts on (a row opening or closing, a group folding, the agents' line) flashes where it shows once the press has done it, and every button where it was pressed, or, for one that asks first, where it shows asking; `menu.list.flash`, over the list's card, which stays open (render/agenda.rs, `lbterm_agenda_activate`, `lbterm_agenda_flash_line`, `lbterm_agenda_press`; FLOWS.md, "A menu, from what opens it to the end of its fade", step 9; perf/agenda.ps1).
- 7b: nothing of the rail plays under a menu's card. Besides the panel's edge, light and wave held since 2026-11-07, the rows' own motion and the pads' wait under the card and play once it has gone: the row exit, the slide and the marker's ride that Close tab or Close finished tabs start while the tab menu's card fades out over the open rail, a pad's glint (Copy link's), a status word's dissolve (FLOWS.md, "A menu, from what opens it to the end of its fade", step 10). Built on 2026-11-08 (stage 4): the compositor applies none of the rail's rows, marker, progress fills, pads' arcs or the Conductor's outlines and draws none of their surfaces while a menu's card shows, by the same cards the edge's hold reads, and the first frame after the card has gone takes it all as one change from what the screen last showed, so Close tab's row fades and the rows below slide up, the marker riding, once the card has gone; a pad's glint waits in the GUI's queue and glints at that paint; with motion off it all lands in that frame (`lbterm_motion::rail::Hold`; PITFALLS 193 and 194; perf/tab-mouse.ps1 steps 11, 13 and 15, perf/menu-keys.ps1 step 5). Not held: the lights along suggested tab connections, which a pointer coming onto the rail starts, and the rail without the compositor, which has no motion to hold.
- 8b: `missed` on the menus' openings is traced before landing, on main's build and the branch's, so the budget says something true. Traced on 2026-11-08 (stage 2): every miss judged since 2026-11-07 was the counter's jumps (the screen's refresh counter jumping at a present); whether DWM dropped any refresh in those fades is unmeasured, and the walk counts refreshes by the screen's vblank times now (PITFALLS 166; FLOWS.md, "A test window's motion, from an episode's opening to the budget's `missed`"). A measurement on a window that can be seen is still to run: the right-most screen was covered at the trace, and again at every round of the before-and-after as it lands (2026-11-08, 16:37 to 17:14, the DashboardsBox and Discord 100% each; PERF.md, "And as it lands after its third round"), so `missed` went unmeasured on both builds there too. It needs DISPLAY1, the right-most screen, clear of his windows (`perf/screen-cover.ps1` says what covers it; 98% each under the DashboardsBox and Discord still at 19:31 on 2026-11-08, after the review's fix-up): `warden-run --name ntm-missed --cpus 4 --mem 8 --gpu-draw --lock lbterm-measure --max 2h -- pwsh -NoProfile -File <tree>/lbterm/lbterm.ps1 budgets -Only tab-menu-opening,prompts-opening,inbox-opening -Bin <a copy of the branch's build>`, then `missed`, `refreshes` and `refreshes_counted` read in its report; with `perf/budgets.ps1 -Scenario tab-menu-opening -Runs 6 -Bin <that copy> -Log 'info,lbterm::present=debug,lbterm::present::walk=trace'` for each frame the walk read. Main's build can't be compared on it: it still counts by the counter. Measured on 2026-11-08 at 22:16 with DISPLAY1 clear (his 8a: he moved the DashboardsBox and Discord off it, and the run waited until no other test job ran and no test window was there), on the branch's build with his 3a and 4b (20261108-200000-0faee56b): in every one of 15 runs of each opening, the tab menu, the prompt list and the InMail list, exactly 1 of its 13 or 14 refreshes was missed (`refreshes_counted` 2,962 to 3,536, the counter's jumps), and so was 1 in each of the walk's 6 traced tab-menu openings and in the rail's motion in the same windows: the motion's second refresh, 11 to 29 ms after its episode opens, comes without a present, and every refresh after it has one. So it's the start of every motion LBTerm plays, not the menus': main's build (the installed 63467391b, 2026-11-09 00:09) has no fade on the tab menu or the prompt list to judge, and its walk still counts by the counter (its rail's motion 30,680 refreshes in 2.9 s). The budget of 0 holds no motion; why a motion's second refresh shows nothing new is open (PERF.md, "Measured on a window that could be seen").
- 9a: the key bump, the hairline a key no row takes brightens (any key 7b holds, now), made plainly visible at real speed: one pixel of the card's edge about a fifth brighter for about 150 ms is too faint by his rule that decoration he asked for must read at real speed. Designed from DESIGN.md's tokens and the lbterm-motion skill, captured at slowdown 1 and 5 for him to judge. Built on 2026-11-08 (stage 5): the card's edge flares, its hairline going all the way to white and the light round the card going white under its red, white 50% at the card's edge (the `light` token's most) falling away with the red out to its 11 px, rising on `arrive_in` and fading on `panel_glow`, the light that lingers; the fill, rows and words stay as they are, since a flare of the whole card would read as a pick (DESIGN.md, "Menus"). Nothing is drawn for it: the compositor shows the card's own surface again just under the card through a colour matrix that makes it white at its alpha scaled, and the card, opaque inside its hairline, hides all of it but the light round it (`MenuLamp`, `menu_lamp`, `lbterm_motion::menu::bump`, `palette::menu_bump_light_scale`; PITFALLS 198). At real speed the hairline goes from luma 159.6 to white (+95.4, where the pulse went +33.4) and is above half of that for 168 ms and a tenth for 306 (the pulse 108 and 193), and the light 2 px out lifts 71 levels of luma, 6 px out 23 (FLOWS.md, "A menu, from what opens it to the end of its fade", Seen on screen, "The key's flare"). For him to judge, in slot-5's target/motion: `bump-after-1x/flare.gif` against `bump-before-1x/flare.gif` at real speed, `bump-after-5x/flare.gif` at slowdown 5, and each folder's `flare-card.png` and `flare-corner.png`, the frames nearest 0 to 500 ms after the key. perf/menu-keys.ps1's step 16 reads it in captures at slowdown 10.
- 10a: kept as built: a menu opened from the rail keeps the rail open until its card has gone, where it used to close under the card after 300 ms.
- 11a: Ctrl+V while the picker is open pastes the clipboard's text into its search box, as typed words, so the search runs; over an open menu it does nothing, as before (a bound key no row takes). No test touches his clipboard. Built on 2026-11-08 (stage 3; Ctrl+V alone since his 4b that evening, Ctrl+Shift+V and Shift+Insert, which the defaults bind to a paste too, being keys the picker has no use for): Ctrl+V, bound to `PasteFrom 'Clipboard'` in his config, over the picker puts the clipboard's first line with something in it, trimmed, in the box (`newtab::pasted`, the box holding one line), the clipboard read off the GUI thread as a pane's paste is; in a test window it's the state file's `copied`, what the window last copied, never the clipboard (PITFALLS 151; STATE.md, `copied`). The box measures and draws only what it holds of a long line, galloped out from its caret both ways (`newtab::box_span`), where it measured the whole line again at each character (PITFALLS 192), and then, in its first fix, still the whole line before the caret and its whole tail (PITFALLS 209, the review of 2026-11-08).
- 12, yes: the highlight covers the Colour row too when Down or Up lands on it, as on every other row. Built on 2026-11-08 (stage 4): the row lights whenever a swatch has the keys or the pointer (`menu::same_row`), the highlight gliding onto it and off it as between any rows, the swatch's ring still saying which; the pointer on the row's label, off the swatches, lights nothing, as the row does nothing there (FLOWS.md, "A menu, from what opens it to the end of its fade", the Colour row; perf/menu-keys.ps1 step 3).

His answers of 2026-11-08 (evening) to what the follow-up's stages and their review left him, with the facts each turned on:
- 1a, the flare's length kept as built: about 300 ms at real speed (above half its lift for 168 ms, above a tenth for 306), on the existing tokens (`arrive_in` up, `panel_glow` down): it's at the card he's looking at, and a key's answer stays short. A longer fall token, about 0.5 to 0.6 s, was the other choice.
- 2a, the flare's look kept as built: the hairline fully white and the light round the card white 50% at its edge under its red, so pink-white (brighter, white 100% at the edge, and whiter, white over the red, were the others).
- 3a, a key the picker has no use for lights round its box: the picker's card has the menus' hairline and accent light (DESIGN.md), but it isn't one of the compositor's menu slots, so it has no lamp, and a bound key over it showed nothing. Built that evening: a key that neither moves, picks nor edits the box (a key the config binds, since 7b every key while it's open; a modifier on its own excepted) brings the `Light::Pad` round the box that Enter with nothing named does (`lbterm_picker_light`, render/new_tab.rs; perf/new-tab.ps1 step 14).
- 4b, only Ctrl+V pastes into the picker: it was built as any key bound to `PasteFrom` (his Ctrl+V, and the defaults' Ctrl+Shift+V and Shift+Insert, which his config leaves on). Built that evening: `newtab::pastes_into_picker`, the key's words as the + menu names them; Ctrl+Shift+V and Shift+Insert over the picker are keys it has no use for (3a). A posted key can't carry Ctrl, so the window's test checks a paste key that isn't Ctrl+V (perf/new-tab.ps1 step 14), and the unit test Ctrl+V.
- 5a, kept: the Colour row lights under the pointer on a swatch as well as when the keys land on one, as every other row lights under the pointer.
- 6a, kept: the rail moves at the true end of the card's fade, about 120 ms after the key at real speed.
- 7a, kept: after a pick in a menu opened from the rail, the rail closes once the card has gone if the pointer isn't on it (his 7b and 10a together); on main it stayed open until the pointer next moved.
- 8a: the right-most screen (DISPLAY1, x 4920), where test windows open, was under his DashboardsBox and Discord at nearly every run (98% each at 18:47, 19:25 and 19:31 on 2026-11-08), so `missed` and the screen times went unjudged in every round since 2026-11-07 but two (PITFALLS 182). He moves those windows off it for the 8b measurement; other LBTerm tracks' test windows open there too (his words), so it runs when no other test job does.

## The InMail for the Conductor (sent 2026-11-01, with his 4a and 5a)

Subject: Hide a session from the Conductor (LBTerm's tab menu): hide, show and a hidden list in button.json

From Terminal, for LBTerm. Lachlan asked on 2026-10-31 for a "Hide from Conductor" item in the tab menu, for a Claude session he wants to handle himself. LBTerm will draw the item and offer it. What hiding means is the Conductor's, so LBTerm won't offer the item until your button.json says you can do it. There's no half version and no fallback.

What hidden means: the Conductor's Claude doesn't hear about the session and doesn't act on it. Lachlan's own presses still work.

Please add:

1. Commands in your contract, under sessions or in a topic of their own:
   conductor hide <session id> --by <who> [--said "<his words>"] [--result <id>]
   conductor show <session id> --by <who> [--said "<his words>"] [--result <id>]
   conductor hidden [--json]
   - They have to work whether the Conductor is on or off and whether the seat is running. A hide is a choice he makes once and expects to stick while you're off.
   - --result writes how it ended to @Conductor:results, as new does, so LBTerm can tell him why a hide was refused.
   - Hiding a session that's already hidden, or showing one that isn't, still rewrites the file with a fresh updatedAt, as your modes do. That way LBTerm always sees an answer.

2. The list itself.
   - Keep it in your own state across restarts, keyed by Claude session id.
   - A hidden session stays hidden through an LBTerm restart, crash restore, a move to another tab and claude --resume. A fork or a new session starts visible.
   - Keep when it was hidden, who did it and his words, for your history.
   - Prune an entry only once you know the session is gone for good.

3. Publish it in two places.
   - button.json: "hidden": ["<session id>", ...]. It should always be there ([] when none), written with the rest of the file through a rename, with updatedAt bumped on every change. LBTerm already watches button.json on a thread of its own, and this field being there is how it knows you can hide sessions.
   - seat.view: "hidden": true on each hidden session. LBTerm's view uses it to fold them into a Hidden group with a Show button. Relay's Conductor screen could use it too, which is Relay's call; please tell Relay.

4. The hub, for a hidden session:
   - No wakes and no events delivered. Logging them is fine, with wake false.
   - No conductor.focus entries, so it gets no thinking or acting outline.
   - No conductor.estimates entries, no reviewer turns, and nothing about it in the RSI report's suggestions (Lachlan's answer, below).
   - Keep doing the work you do in Terminal's place: summaries, labels, objectives with tab links, and question files. Those draw his rail and strip, not yours.

5. The Conductor's Claude, through the seat and its MCP tools:
   - Leave the session out of what it's given of the fleet and status.
   - Refuse read, say, answer, resume and stop on it with "hidden from the Conductor by Lachlan", logged in actions.
   - Whatever Accept most and Accept all let it do to sessions on its own, it never does to a hidden one. Its permission prompts wait for him.
   - Those refusals are for its Claude only. A press from Lachlan in LBTerm's view or in Relay still goes through.
   - conductor status (his own) still lists a hidden session, marked hidden. conductor important still counts it. Both are his, not the Conductor's.

6. Tests:
   - A hidden session's hook events never reach the Conductor's Claude and never wake it.
   - It never gets an outline or an estimate.
   - The tools refuse it.
   - The list survives a restart.
   - show undoes all of it.

What LBTerm does (built in Terminal's code/LBTerm; nothing for you to build):
- The tab menu offers "Hide from Conductor", or "Show to Conductor", on a tab with a Claude session once button.json has hidden.
- Lachlan's lbterm.lua runs, through background_child_process, once for each session in the tab:
  conductor hide|show <session id> --by lbterm --said "picked Hide from Conductor on <label>'s tab in LBTerm" --result <id>
- The tab shows an eye-off mark at once and keeps it once your hidden list names the session. If the list doesn't name it within 5 s, the mark goes and the strip says so. Your --result reason comes up as a notification.
- The Show button in the view's Hidden group runs show the same way.
- LBTerm doesn't filter your outlines or estimates for a hidden session itself. If one arrives anyway, that's a bug on your side, and the tests above should catch it.

Lachlan's answers (2026-11-01), so build these as described:
- Estimates, Sonnet's reviews and the RSI report skip a hidden session (his 4a), and the rail's guessed bar goes for that tab.
- A hidden session's questions stay in the view and on Relay's phone screen, in the Hidden group, and he can answer them with his own press (his 5a). They're his questions, and the Conductor's Claude never sees them.

Why: with 8 to 18 sessions open, he wants some left entirely to himself. That means no wakes, no Haiku or Sonnet spent on them, and nothing the Conductor types into them.
