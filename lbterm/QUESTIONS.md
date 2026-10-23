# Question colours

Lachlan's idea (2026-10-21): "when I hold right click on the conversation, a lesser model will show the questions it's asking me in different colours, and highlight the relevant bits in the response that are related to the question in the same colour."

While the right button's held on a Claude pane, each question in Claude's reply gets a colour, Q1 blue, Q2 green, Q3 amber, Q4 violet, Q5 teal, Q6 pink (then round again), and so does every passage of the reply that the question is about: a band over the whole bullet or paragraph, a bar down its left. A question's options (1a, 1b) take its colour too, and a passage two questions are about carries both bars. When part of a question is scrolled out of Claude Code's view, a chip at that edge of the conversation says so: "Q5 ↑" at the top for a passage above, "Q2 ↓" at the bottom for a question below. Let go and it's all gone. The wheel still scrolls the conversation meanwhile, and the colours follow it.

## How it gets there

1. **After each reply** (Terminal's `shared/claude`): `claudehook stop` hands the reply that asks something (a line outside code, quotes, tables and headings that's labelled `Q1` or ends in `?`) to `question-worker.ts` on its stdin. The worker asks Haiku (`claude -p --model haiku`, as the summary worker does) for each question's first words and quotes of the passages it's about, keeps only what's word for word in the reply, and adds it to `~/.claude/summaries/<session>.questions.json`, the session's last 20 replies that asked something. This happens before the button's pressed, so a press shows the colours at once.
2. **On the press** (here): Terminal's `lbterm.lua` swallows a plain right press and release in panes that take the mouse (Claude Code would paste on them), and binds those to `LbtermQuestions 'Show'` and `'Hide'`, only where `wezterm.has_action('LbtermQuestions')`, since a build without the action refuses the whole config. The press that shows them holds them: they go when that button's let go anywhere, when a mouse event says it's no longer down (let go outside the window), or when the window loses the keyboard (`wezterm-gui/src/termwindow/render/questions.rs`).
3. **Finding them on screen** (`lbterm-extras/src/questions.rs`): the pane holds only Claude Code's screen, the reply as it drew it: its markdown gone, its lines wrapped to the pane, two columns in. So everything is matched by its words alone (runs of letters and digits, lowercased), across rows. Each match grows to its block: up to the row it starts on (a bullet, a number, a `Q` label, Claude Code's `●`) and down to a blank row, the next block, or a row indented less. Claude Code's input box (between the two rules at the bottom) and what's under it are left out. A passage cut off by the top or bottom edge still counts by the part that's on screen, if that part is at the edge and at least two fifths of it. The file's newest reply is laid out first, so where two claim the same rows it keeps them.
4. **Which way the rest is**: each question and passage carries where it starts in the reply, in words. What's on screen spans a range of those; a part of a question that's on screen that isn't found and lies before that range is above, after it below.

The layout is made again only when the pane's screen or the session's file changes, and only while the button's held. Bands are drawn over the text in z-layer 23 and the chips in 24 and 25, in the compositor's `over` visual beside the prompt mark.

## Settings

Nothing of its own: it needs `lbterm_summaries` (the folder the worker writes to, which `lbterm.lua` sets) and a binding.

```lua
-- In a pane that takes the mouse, a held right button (lbterm.lua does this for the ones it swallows)
{ event = { Down = { streak = 1, button = 'Right' } }, mods = 'NONE', mouse_reporting = true,
  action = wezterm.action.LbtermQuestions 'Show' },
{ event = { Up = { streak = 1, button = 'Right' } }, mods = 'NONE', mouse_reporting = true,
  action = wezterm.action.LbtermQuestions 'Hide' },
-- Or a key that turns them on and off
{ key = 'q', mods = 'CTRL|ALT', action = wezterm.action.LbtermQuestions 'Toggle' },
```

## What it can't do

- Colour what Claude Code hasn't drawn: a passage scrolled out of its view gets a chip, not a colour. No escape sequence tells a terminal where a fullscreen program is scrolled to.
- Questions asked with AskUserQuestion's picker: they aren't in the reply's text.
- A reply whose questions Haiku didn't find, or quoted wrongly: the worker drops what isn't word for word in the reply, so a wrong quote colours nothing rather than the wrong thing.

## Checking it

`cargo test -p lbterm-extras` (through `lbterm.ps1 test -Package lbterm-extras`) covers the matching, the blocks, the options, the markers and the file. `lbterm\perf\questions.ps1` runs it end to end in a test window against `perf\fake-reply.ps1`, a stand-in for Claude Code's fullscreen view with one reply. Terminal's check covers the worker (`question-worker.test.ts`), which replies claudehook hands it (`claudehook.Tests.ps1`) and the binding (`lbterm.Tests.ps1`).
