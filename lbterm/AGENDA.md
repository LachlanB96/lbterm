# The agenda button

A small button on the window strip, in its first row after the InMail button (or in its place without it), with two counts: what's urgent, in the waiting orange, and what's failing, in red. A click drops a list from it of what's to do, coming up and failing: what's urgent first, then what's failing, what's coming up and the to-dos that are yours, with the agents' to-dos behind a line that shows them. Lachlan's comes from his own list of everything across his projects (Lachlan's picks, 2026-11-01: the button beside the InMail one, its list hanging under it like the InMail list's, the urgent and failing first and the agents' to-dos behind a filter).

LBTerm keeps no list of its own. Another program writes the rows and the counts to a file, and the button shows what the file says: the counts are the program's, never counted again here.

## Turning it on

```lua
rawset(config, 'lbterm_agenda', {
  file = [[C:\Work\State\agenda\awake.json]],
  glyph = '\u{f076e}',   -- on the button, before the counts; nf-md-format_list_checks by default
})

-- A button under an opened row fires this. LBTerm does nothing more: the handler runs it
wezterm.on('lbterm-agenda-press', function(window, pane, p)
  -- p.file is the agenda's file as the config named it: a press from any other agenda (a test
  -- window that runs this config with a file of its own) isn't this handler's, so it returns
  -- nothing and the next handler gets it. Returning false stops every handler after it.
  if p.file ~= [[C:\Work\State\agenda\awake.json]] then return end
  -- p.what: 'do' (one of the row's own buttons, p.button its word), 'claude', 'snooze' (p.days) or 'open' (p.open)
  -- p.source and p.id name the row to its program; p.project and p.title say what it is
  wezterm.background_child_process({ 'agenda', p.what, p.source, p.id, tostring(p.button or p.days or '') })
  return false
end)
```

`rawset`, as for the InMail button: a build that doesn't know the key would refuse it. It needs the tab panel (`lbterm_tab_panel.enabled`, with the tab bar holding only the window's buttons), since the button sits over the rail. No file, no button.

## The file

```json
{ "changed": 1793499785871,
  "counts": { "urgent": 15, "failing": 53 },
  "rows": [ { "source": "Index", "id": "fail:docs:git", "kind": "failing", "who": "agent",
              "level": "normal", "title": "Git: no repo", "detail": "Not a git repo yet",
              "project": "docs", "whenText": "waiting 3 d", "open": "/d/projects",
              "group": "Unresolved incidents",
              "actions": [ { "label": "Recheck" }, { "label": "Drop", "confirm": "Drop this for good?" } ] } ] }
```

- `counts` is what the button shows. A file without it, or without `rows`, isn't an agenda: the button keeps what the last good one said.
- Each row: `kind` is `todo`, `upcoming` or `failing`; `who` is `you` or `agent`; `level` is `urgent`, `soon`, `normal` or `low`. `source` and `id` name it to its program. `title` is its line and `detail` a second shown when it's opened; `project` (the source without one) and `whenText` (the program's own words for when) go in their columns. `group` folds rows together. `actions` are the row's own buttons, `confirm` what one asks before it runs. A row LBTerm can't read (a kind, whose or level it doesn't know, no id or title) is left out.
- The rows come in the program's order, which each section keeps.
- The program writes it whole and renames it into place, and only when what's in it changes, in a folder of its own: Windows says when a folder changes, not a file, so a busy folder would wake LBTerm's watch for nothing.

## The list

- **Sections**, each under its heading with a number: Urgent (anything urgent, whatever it is), Failing, Coming up, Yours to do, and the agents' line at the end, which shows the Agents' to-dos under it at a click. Coming up goes before Yours to do because it's short and has dates in it, where the to-dos can run long. Urgent's and Failing's numbers are the file's counts, the ones on the button, never counted here: the program's failing count can take in the urgent failing rows listed under Urgent and leave out low ones, so the rows under Failing needn't add up to it. The other headings count their rows.
- **Groups.** Rows of one group and project fold into one line, `Unresolved incidents · Harbor` with its count, where the first of them was; a click opens it, its rows under it. A low row without a group folds into `Whenever`. A group of one is just its row, and urgent rows never fold.
- **A row**: an orange dot for urgent, a red one for failing, then when, the project and the title. A click opens it: its detail and its buttons under it, one row open at a time.
- **Buttons**: the row's own, then Claude, Snooze 1 d, 7 d, and Open when it has a page. Each fires `lbterm-agenda-press`. One that asks first says its question on the button, ringed in the waiting orange, at the first press, and runs at the second. Claude and Open close the list, since they take him elsewhere; the rest leave it open, the row dim until the file says something new.
- **Keys** while it's open: up and down over the rows, groups and the agents' line, Page Up, Page Down, Home and End; Enter opens or folds what's picked; Esc closes. The wheel scrolls.
- What's opened (the agents' to-dos shown, the groups opened) stays for the next time the list opens; an opened row closes with it.

## How it's built

**Nothing is read on the GUI thread.** The file is watched on the thread that watches the Conductor's state file and the tab links' (`wezterm-gui/src/state_files.rs`), through its folder, and read when Windows says it changed; when what it says changes, every window repaints. While nothing changes it sleeps.

**The logic** is `lbterm-extras/src/agenda.rs`, tested without a window: reading the file, the list's lines (`lines`), a row's buttons (`presses`) and the colours.

**The button** (`render/agenda.rs`, `lbterm_agenda_button`) is a pill like the InMail button's (`lbterm_strip_pill` in `render/inbox.rs`, which both use), in the strip's first row. `lbterm_strip_buttons` places them: the InMail button centred over the rail's pads, the agenda's half a column after it, and they push the strip's words on rather than go into them. It sits back (white 45%) with nothing to count, as the InMail button does with nothing waiting. New counts in the same window dissolve on `swap`, and more brightens it once (`render/strip_swap.rs`, `lbterm_strip_swap_button`).

**The list** hangs under the strip from the button's left, in the tab menu's card and light, in the InMail list's z-layers (77 to 79, in the compositor's `top`): only one of the two is open at a time. It's laid out again only when the file's rows, what's opened, the button asking first, the scroll, the fonts or where it goes change.

## The state file

`strip.agenda` is `{urgent, failing}` while the button shows, `null` without it, and `strip.items` names it `agenda`. The open list is `menu` with `kind` `agenda`: an item a line, top to bottom (`Urgent (1)`, `Ledger: Pay the car registration`, `Unresolved incidents · Harbor (2)`, `detail: …`, `Agents' to-dos (1)`), an opened row's buttons an item each (`button: Tick`, `button: Drop this for good?` while it asks), `checked` for an opened group or row, the agents' line while it shows them, and a button asking first (lbterm/STATE.md).

## Checking it

`pwsh -File lbterm\perf\agenda.ps1` (`[-Bin <build>]`), in a test window with the InMail button beside it and a stand-in file: the button after the InMail one with its counts in their colours, the list's sections and a folded group, a group and the agents' to-dos opened, a row's detail and buttons, a button that asks first, the file rewritten and the button and list following it, Snooze and Open heard by the handler, the keys, and the button gone with its file.
