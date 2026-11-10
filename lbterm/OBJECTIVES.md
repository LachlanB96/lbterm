# Objectives panel

A narrow strip down the right of the window, ringed with the tab rail's grey hairline, with a mark level with each tab's pad on the rail saying what became of what its Claude session was started for. Rest the pointer on it and a card opens over the panes beside it, listing each tab's first objective on its row, level with its pad, with how many of its objectives are done. Rest on a row there and a second card opens beside that with the session's whole list: each objective's status, its words, and what showed the status. A click on a row opens its tab.

It's Lachlan's ask (2026-10-24): with sixteen Claude sessions open he couldn't tell what each was started for, or whether it got there. A session often sets out on one thing, goes off on another he asked for on the way, finishes that, and the first is forgotten; the rail's label follows whatever the session is doing now, so it doesn't help. He picked a right-hand pane with hover for detail over putting it in the tab rail, and Haiku judging what's done with its evidence over marking each by hand.

## Turning it on

```lua
config.lbterm_objectives = {
  width = '10.5pt',       -- the closed strip, its ring included (14 px at 96 DPI)
  card_width = '270pt',   -- each card (360 px at 96 DPI), less when the window hasn't the room
  open_delay_ms = 300,    -- the pointer rests on the strip this long before the card opens
  close_delay_ms = 200,   -- and the cards close this long after it leaves them
}
```

It needs `lbterm_tab_panel` (its marks line up with the rail's rows) and `lbterm_summaries` (where the files are). Without either the strip takes no room.

## Where the objectives come from

Terminal's `shared/claude/objectives.ts` keeps `<session>.objectives.json` in the `lbterm_summaries` folder, beside the session's label and summary, in the same Haiku call that writes those: Conductor's hub's digest of each finished step, or Terminal's summary worker while no hub writes them (Terminal's `shared/claude/README.md`, "objectives.ts"). The file, read off the GUI thread by `lbterm_extras::summaries` like the labels and parsed by `lbterm_extras::objectives`:

```json
{ "sessionId": "…", "updatedAt": 1792820000000, "current": 2,
  "objectives": [{ "id": 1, "text": "Find why Defender uses 30% of the CPU", "from": "first", "status": "done", "evidence": "Nightly checks explained it" },
                 { "id": 2, "text": "Have the warden know the overclocked cores", "from": "prompt", "status": "open", "evidence": "" }] }
```

`from` is `first` (the session's first request), `inmail` (a message it was told to deal with, its subject as its words) or `prompt` (something asked for later); `status` is `open`, `done`, `waiting` (on Lachlan) or `dropped`; `current` is the objective its last step was on. A status or source LBTerm doesn't know reads as open, or as asked for later.

## The marks

What the session was started for is its `first` and `inmail` objectives, those not dropped (the rest when there are none of those). The mark (`Objectives::mark`):

| Mark | When |
|---|---|
| green dot | everything it was asked for is done |
| green ring | what it was started for is done; something asked for later is still open |
| yellow ring | what it was started for is still open, and its last step was on something asked for later: it went off on a detour, finished or not |
| orange ring | what it was started for waits on Lachlan |
| grey ring | what it was started for is still open, and it's on it |

A tab with no Claude session, or whose session has no file yet, has no mark. The colours are the rail's: its green, the idle timer's yellow, the waiting pad's orange, the labels' grey.

## The cards

The list card's right edge is a gutter short of the strip, over the panes' last card, and runs from the first visible row to the last. Each row is level with its tab's pad: the mark, the first objective (a message's as "InMail: subject"), cut with an ellipsis to fit, and "2/3" at the right when it has more than one. A tab without objectives shows its name, dimmer, so the rows still line up with the rail. The row under the pointer is lit (white 6%).

The detail card sits a gutter left of the list, its top level with the row it's for, moved up to fit above the window's bottom: the tab's name, then each objective's status word in its colour (Done green, Waiting orange, Open, Dropped dim), its words wrapped beside it, and its evidence dimmer under them; "now" under the status of the objective the last step was on.

Moving the pointer from the strip onto the list, along it, and onto the detail card keeps both open; the detail follows the row the pointer is on, on the list or level with it on the strip. Leaving them all closes both after `close_delay_ms`.

## The projects card

A pad at the top of the strip, four small grey squares level with the Conductor's pad on the tab rail (at the strip's foot when there's no Conductor), opens a card of every project while the pointer rests on it (Lachlan's ask, 2026-10-24; his picks: at the top of this strip, all projects busiest first, sessions with their subagents and tasks, start, read InMail or resume, the new tab in front). A left press on the pad opens it at once, without waiting out the rest (his E5, 2026-10-29; `Hover::open_now`), and the pointer leaving closes it as usual.

```lua
config.lbterm_projects = {
  projects = {   -- every project, any order
    { name = 'Terminal', dir = [[C:\…\AI\Terminal]] },
    { name = 'Shell', dir = [[C:\…\AI\Terminal\code\Shell]], parent = 'Terminal' },  -- a repo of Terminal's
    …
  },
  card_width = '360pt',   -- 480 px at 96 DPI
}
```

It needs `lbterm_objectives`, and takes the strip's delays. Each row is a project, the busiest first (the most sessions, then subagents and tasks, then mail, then by name), a quiet one (nothing open, no mail) dimmer. A project with a `parent` is a repo of that one (Lachlan's ask, 2026-10-28: "things like Trainer project should be shown under Musica project, maybe it has a blue icon"): its row comes straight under its project's, its name indented past a repo mark (nf-oct-repo) in a blue of its own (`#4FB3FF`, lighter and greener than the rail's "your turn" blue, which the idle dots on this card use), with its own sessions, mail and buttons. A project goes by what's going on in it and its repos together, so a repo with sessions or mail brings its project up with it, and its repos follow it the busiest first. It nests one level: a `parent` that isn't in the list, or is a repo itself, leaves the row a project of its own. Each row has:

- its name, a repo's after its mark;
- its Claude sessions, in every LBTerm window's tabs, as dots in the rail's colours with a count: working green, waiting on you orange, idle blue; then `+N`, their subagents and background tasks (what the rail's row counts as "agents" and as tasks it waits on). A session is the project's whose folder it's in now (its statusline's `<session>.cwd.txt`, else its pane's folder), the deepest project folder above it, as `projects.ts` finds one, so one in `Terminal\code\LBTerm` is LBTerm's;
- the InMail waiting for it, `N mail` in the waiting orange, counted from the InMail button's folders (`lbterm_inbox`);
- on the row under the pointer, buttons: **New**, **InMail** (only with mail) and **Resume**.

A title line says "Projects", with what the dots mean beside it when the card is wide enough. The card opens down from the pad at the strip's top, and up from it at the strip's foot (`projects::place`). When the rows don't all fit in the window, the quietest are left out from the bottom (a repo never shows without its project) and the last line says how many.

A button fires the `lbterm-project-action` Lua event, `(window, pane, project)`, `project` being `{name, dir, action, mail, sessions, source}` with `action` `new`, `inmail` or `resume` and `source` `card`, and the card closes. The new-tab pad's picker fires the same event (lbterm/NEW-TAB.md), with `source` `picker`, `action` `shell` as well, and `prompt` and `behind`, and so does the tab menu's New session in <Project> (since 2026-11-05, NEW-TAB.md's landing 2), with `source` `tab menu`, `action` `new`, the tab's folder as `dir` and `behind` false. Unless a handler returns false, New opens a tab in the project's folder, in front. Terminal's `lbterm.lua` handles all three, each opening its tab in front with one `spawn_tab` over the window's own connection to the mux server (its `open_claude_tab`), no process started to open it, and names who asked in its `--by` (`projects card`, `new-tab picker`, `tab menu`): New and InMail with a session id it makes (InMail with Claude told to read its InMail and deal with each message), which Conductor's `record new` puts in its history after (Lachlan's 2a, 2026-10-29; they went through Conductor's `new --show` before), and Resume with the project's newest conversation that isn't open, picked from the Claude project's `@Claude:resumable`, which the handler reads at the press (his 1b, 2026-11-02; the Claude project's `ai.ts resume-last` before). Terminal's `docs/lbterm-lua.md` has each.

## How it's built

- **Room for it.** `render/objectives.rs`'s `strip_width` (the strip and a gutter) is added to the window's right border in `get_os_border_impl` (`render/borders.rs`), as the rail's is to the left, so the panes, the scrollbar and the mouse all leave room for it, and the fancy tab bar takes it back (`fancy_tab_bar.rs`) so the window's buttons stay at the edge. It never changes as the cards open, so no PTY is resized.
- **Drawing.** The strip, the panes' colour under it, its marks and the projects pad are z-layer 38, in `over`: they never move. They're drawn as a list of shapes (render/sides.rs, with the scroll strip's), and as a tab is switched from or closed the list the frame before drew goes over the next tab's strip in a visual of its own and fades out with that tab's panes, on `tab_switch` from the share they start from: a closed tab's rows below move up, and their old marks fade where they were over the new ones. A switch alone changes no mark, so the strip, drawn the same in both tabs, isn't drawn again. The list card's shadow, card and words are z-layers 39 to 41, the detail card's 42 to 44, the projects card's 27 to 29 (`objectives::card_of`), each card's only in its own visual with the compositor. `render/projects.rs` is the projects card's, `lbterm_extras::projects` where its words go.
- **What a frame costs.** A card's words are laid out once and kept by what each was laid out from (its text, colour, place, font and cell), and their quads kept while the words stay the same (`render/words.rs`): a frame painted for anything else lays out none of them. The projects card's folders are made once per list of projects (`lbterm_extras::projects::Folders`), its mail counted again only when the inbox's list of messages is another, and the card laid out again only when its counts or its place change. With the card open over fifteen tabs and a line changing 20 times a second in the open tab, a frame cost the GUI thread 18.1 ms before this (2026-10-31) and 5.9 after, against 5.4 to 5.5 with the card shut (lbterm/PERF.md, "What the cards cost").
- **Motion.** The cards fade as the strip's detail card does (`lbterm/DESIGN.md`; the `lbterm-motion` skill): a visual each in the compositor's `objectives` group, over the strip's card and under the tab panel, its opacity faded in on `pane_enter` as it opens and out on `pane_exit` as it closes, from wherever it is; a closing card is drawn as it was until it has faded (`Compositor::prepare_objectives`, `objectives_showing`). Nothing moves. Coming back while a card fades out opens it again at once; the rest before it opens and the grace before it closes are `lbterm_panel::Hover`'s. With motion off or no compositor the cards come and go at once.
- **Mouse.** `lbterm_objectives_mouse` runs after the strip's card and before the tab panel (`mouseevent.rs`). It takes every event on the strip (and the gutters either side of it) and on an open card, so nothing under a card gets a click.
- **The state file** (`lbterm/STATE.md`): `objectives` with the strip, each row's mark and where it is, the cards while open, the row under the pointer, and `projects`, the pad and its card; `pointer.over` names `objectives.strip`, `objectives.list`, `objectives.detail`, `projects.pad` and `projects.card`; the fades are `objectives.list`, `objectives.detail` and `objectives.projects`, the waits `objectives.open`, `objectives.close`, `projects.open` and `projects.close`.

## Checking it

- `cargo test -p lbterm-extras objectives` (the marks, the first objective, where every word on the cards goes), `projects` (which project a folder is in, which project a repo is under, the order, the card's rows, a repo's mark and the buttons) and `summaries` (the file read beside the label), and `cargo test -p config lbterm` (the settings), all in `lbterm.ps1 check`.
- `pwsh -File lbterm\perf\side-strips.ps1`: the strip fading with the panes as a tab closes, the third row's mark showing the closed tab's panes' share in every frame on the way, and the same strip in two tabs showing no change as they switch (lbterm/SCROLL-STRIP.md has the rest).
- `pwsh -File lbterm\perf\objectives.ps1`: three tabs with Claude sessions and objectives files in a scratch `lbterm_summaries` folder and one plain. The marks must sit level with the rail's pads in their colours, a file rewritten must change its mark, resting on the strip must open the list with each tab's first objective, resting on a row the detail card with its whole list, and leaving must close both (`target\perf\objectives`).
