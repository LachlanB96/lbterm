# The context pill

A small pill on the window strip's second row, right under the InMail button, saying what the open tab's Claude Code conversation has cost. A click drops a list of the files in that conversation's context: every CLAUDE.md, import, rule and memory index Claude Code loaded, how each came to be loaded and how big it is. Asked for on 2026-11-07: "LBTerm should be able to show me what context files this agent has loaded currently. There should also be a $ amount of how much this agents tab has cost. It should go under the inmail button." The open tab panel's rows say each tab's dollars too (his yes to that, the same day).

## Turning it on

```lua
rawset(config, 'lbterm_context', {
  -- transcripts = [[C:\Users\me\.claude\projects]],   -- Claude Code's; this is the default
})
```

`rawset`, as for the InMail button: a build that doesn't know the key would refuse it. It needs the tab panel (`lbterm_tab_panel.enabled`, with the tab bar holding only the window's buttons) and the strip's second row (`lbterm_prompt_history`), since the pill sits on that row over the rail. The dollars come from the folder `lbterm_summaries` names; the files from the session's transcript, in `transcripts` (or `lbterm_scroll_strip.transcripts`, which wins, since one reader serves both). The pill shows while the open tab is a Claude Code session (its pane's `claude_session` user var), and not while the Conductor's view stands over the tabs.

## What it says

**The dollars** are the conversation's cost at list price, to the cent: every run of it (a resumed conversation appends to the same transcript) and every subagent's, priced request by request from the transcripts. Claude Code's own `total_cost_usd` (what `/cost` says) isn't it: that counts one process, and since it no longer restores a session's cost on resume, a resumed conversation would start again at $0. Another program writes the number, and LBTerm shows it as written; in Lachlan's setup that's his statusline (below). The pill reads `$0.42` and `$12.34` with cents under $100, `$148` and `$2304` in whole dollars, then `$12.3k` (`lbterm_panel::text::dollars`), never rounded up past the cent spent; `$–` before the file says anything.

**The list** hangs under the strip from the pill, in the InMail list's slot (render/menu_fade.rs, `Slot::List`: z-layers 77 to 79, its card a group of the compositor's `menus` that fades in as it opens and out as it closes, with every menu's light and hairline; only one of the lists in the slot is ever open; lbterm/NEW-TAB.md, section 6). While it's open it has every key, those bound in the config too (Lachlan's 7b of 2026-11-07): the arrows, Page Up and Down, Home and End scroll it, Esc closes it, and any other key flares its card's edge once (`lbterm_motion::menu::bump`, Lachlan's 9a of 2026-11-08), Enter among them, since its rows are to read. No row is lit and nothing flashes:

- A heading: how many files, the tokens they take together, and their share of the context window once it's known: `9 context files · ≈31k tokens · 3.1% of the 1M window`.
- The dollars again, with what they count: `$12.34 at list price: every run of it and its subagents`.
- Each file, in the order Claude Code loaded it: how it came to be loaded (`user`, `project`, `local`, `managed`, `memory`, or `pulled in`), its path with the home folder as `~` (cut from its start when it's long, keeping the file's name), and its tokens at the right end, `≈6.1k`.

A `pulled in` file is a CLAUDE.md that a file read in its folder brought in after the start. One from a folder that isn't the conversation's own or one above it, another project's, is drawn in amber, kind and path: that's the expensive kind, a whole project's instructions loaded because one of its files was read. The conversation's own folder is where it started, the first `cwd` its transcript records, which neither a `cd` nor a resume changes.

Rows are to read, not to press. The wheel, the arrow keys, Page Up and Down, Home and End scroll; Esc, the pill again or a click elsewhere closes it. It closes by itself when the open tab stands for another session.

**On the open tab panel** each row's second line ends with its tab's dollars, in the project's grey, the status cut short before it when the line is full. The closed rail doesn't show them.

## Where the files come from

Claude Code records what it puts in a conversation's context in the transcript (`~/.claude/projects/<folder>/<session>.jsonl`), as attachment records:

- `instructions`, when a session starts or resumes and again after every compaction: `files`, each with its `path`, its `type` (`User`, `Project`, `Local`, `Managed`, `AutoMem` for the memory index) and its whole `content`.
- `nested_memory`, for each CLAUDE.md pulled in later: its `path`, and its text under `content.content`.
- A `compact_boundary` system record for each compaction, which drops everything before it, the files pulled in included; the `instructions` record written right after it loads the start's files again.

So what's loaded now is every file recorded since the last compaction, by path, the newest record of a path giving its size. A resume writes another `instructions` record but drops nothing: what was pulled in before is still in the conversation's history. A subagent's records (`isSidechain`) are its own context, not the conversation's. Claude Code's `InstructionsLoaded` hook reports the same loads as they happen (with why: `session_start`, `nested_traversal`, `include`, `path_glob_match`, `compact`), but not the memory index, and it would need a program of its own to keep what it says; the transcript has all of it already, sizes included.

A file's tokens are its content's UTF-16 units over four, rounded, as Claude Code estimates a memory file's size; the heading adds them up. They're the files' share of the context, not the context's whole size.

## How it's built

**Nothing is read on the GUI thread.**

- The files are read by the scroll strip's transcript reader (`lbterm_extras::conversation`, on its `lbterm-conversations` thread): it parses the `instructions` and `nested_memory` attachments as it parses the rest (`WANTED`, the records it reads at all, has both), keeps the files in `lbterm_extras::context::Context`, and shares them with every snapshot it hands out (an `Arc`, copied only when a file changes, the revision moving so a new snapshot is taken). The reader follows the sessions windows say they show (`crate::conversations::shows`), now per window and per reader: the scroll strip says its own (`Reader::ScrollStrip`), and the context list says its session while it's open (`Reader::Context`), so neither undoes the other. Opening the list on a session nothing followed reads its transcript from the start on that thread; the list says `Reading the conversation…` until it's in.
- The dollars are read by the summaries' thread (`lbterm_extras::summaries`), from `<session>.usage.json` beside the session's summary: `{ "usd": 12.34, "window": 1000000 }`, the cost and the context window's size in tokens (`null` before it's known). The writer rewrites it as often as a working session's cost moves a cent, so a change to it alone wakes the GUI with `Changed::Usage` and the sessions it was for, and only a window showing one of them repaints: one whose pill shows it, or whose open tab panel has its row (`TermWindow::lbterm_shows_usage`). Any other change to a summary still repaints every window. A usage-only change doesn't move the summaries' `generation` either, which keys what's laid out from them (the question colours, the drafts), none of which shows the dollars.
- The pill (`render/context.rs`, `lbterm_context_pill`) is built into the strip's second row by `render/strip.rs`'s `lbterm_prompt_row`, centred on the rail's column as the InMail button is on the first row, with the strip between it and the prompt; a pill wider than the room pushes the prompt on, a column after it. It's the InMail button's pill (`lbterm_strip_pill`), sitting back (hairline white 14%, words 70%) and lit under the pointer or while its list is open (50%, white). It fades on hover with the strip's buttons (`render/hover_fade.rs`). Each paint, `lbterm_context_refresh` (after the strip's content, whose look at the tab's panes it reuses, `StripState::session`) builds the strip again only when the pill's words, its session or whether its list is open change.
- The list (`lbterm_context_list_layout`) is laid out again only when the context (by its `Arc`), the dollars, the scroll, the fonts or where it goes change, and its words are laid out once and kept (render/words.rs).

## The state file

`strip.context` is the pill's words while it shows (`$12.34`, `$–`), `null` without it, and `strip.items` names it `context`. The open list is `menu` with `kind` `context`: its lines top first, the heading and the dollars as they read, each file as `<kind> · <path> · ≈<tokens>`, `checked` for one drawn in amber. Each rail row has `cost`, its open line's dollars, empty until known (lbterm/STATE.md).

## Lachlan's setup

His Terminal project's statusline writes `<session>.usage.json` (`rememberUsage`), with the conversation's cost from the Claude project's count of its transcripts, every subagent's included, at list price (the same prices the Claude dashboard uses), and Claude Code's `context_window_size`. It runs at each new message and every 10 s, so a subagent's spending shows while the main conversation is quiet. His `lbterm.lua` turns the pill on.

## Checking it

- `lbterm.ps1 test -Package lbterm-extras,lbterm-panel,config`: the files from the records (`context`, `conversation::tests`), the dollars read and the usage-only wake (`summaries`), the dollars' words and the row's place for them (`lbterm-panel`'s `text` and `words`), the config key.
- `perf/context.ps1`: a test window with a stand-in Claude session, its transcript and its usage file: the pill under the InMail button with the dollars, the list's heading, files, kinds, sizes and an amber pulled-in file, the dollars changing on the pill without the window's other tabs repainting it, a compaction emptying the pulled-in file from the list, and the open tab panel's row with its cost.
