# Triggers

Rules that watch the text a pane shows and do something about what they match: colour it, play an effect, tell Lua. There's no standard for this. iTerm2's triggers (a regex plus a list of actions) are what terminals copy, and WezTerm's `hyperlink_rules` are the same idea limited to links, so LBTerm's follow iTerm2's shape.

## The easter eggs

Without `lbterm_triggers` in the config, LBTerm has two rules built in:

- **love** (the whole word, any case: `(?i)\blove\b`) goes a little pink, 45% of the way from the text's own colour to `#ff7eb6`, and sends seven hearts floating up from the bottom-right corner of the pane, swaying and fading over two seconds. At most once every 4 seconds per pane.
- **LB** in capitals at the start of a word (`\bLB`) goes rainbow, red to violet across the two letters. So LBTerm's own name does, in a path like `C:\Code\LBTerm` too, and MLB or LachlanB don't.

## Your own

```lua
config.lbterm_triggers = {
  { name = 'love', regex = [[(?i)\blove\b]], tint = '#ff7eb6', tint_amount = 0.45, effect = 'hearts', cooldown_ms = 4000 },
  { name = 'lb', regex = [[\bLB]], rainbow = true },
  { name = 'error', regex = [[(?i)\berror\b]], tint = '#ff5555', tint_amount = 0.6 },
}
-- config.lbterm_triggers = {}   -- none, not even the easter eggs
```

| Key | | |
|---|---|---|
| `name` | required | what the Lua event calls the rule |
| `regex` | required | a Rust regex, matched against each line |
| `tint`, `tint_amount` | | `#rrggbb`, and how far (0 to 1, default 0.45) to move the text's colour towards it, mixed in linear light |
| `rainbow` | | red to violet across the match |
| `effect` | | `'hearts'` |
| `cooldown_ms` | default 4000 | the effect plays at most once in this long, per pane |

A rule that doesn't compile is left out and logged; the rest still work.

Every new match fires a Lua event, effect or not, when the config has a handler for it (without one, nothing is run for it):

```lua
wezterm.on('lbterm-trigger', function(window, pane, name, text)
  wezterm.log_info('trigger ' .. name .. ' saw ' .. text)
end)
```

## How it works

- **Matching** happens as a line is drawn new or changed (`wezterm-gui/src/termwindow/render/triggers.rs`): its text is made once from its cells (`LineText`), all the rules run as one `RegexSet` over it, so a line nothing matches costs one pass, and what it matched is kept until the line's sequence number changes or its row goes out of view. A frame drawing a line it drew before costs a lookup. Only drawn lines are matched: a background tab is matched when it's shown, and output that scrolls past between two frames isn't seen.
- **Styles** colour a copy of the line that is drawn instead of it, kept with what it matched, so its shape is hashed once (on its first frame) and served from the line caches after. A tint is worked out from the colour the text has in the pane's palette, so a program changing the palette (OSC 4, 10) has the copy coloured again, nothing new about it. The terminal's own cells never change, so copying text, scrollback search and the line caches (keyed by a hash of the line, colours included) all behave as before, and scrollback is styled when it's scrolled to.
- **New** is decided once a frame has drawn its panes (`Seen` in `lbterm-triggers`). Each match is in two places: its line (that text, without the blanks after it, that rule, that column) and its row (the text before it in that row, the rule, the column), and each place counts how many of the pane's lines on the screen have it. A match is new when, over the frame, both its places' counts went up. A place stays seen while it's on the screen, however long and however seldom the window paints, and for 10 seconds after it goes (its line changed or was cleared, or its row went out of view): one that comes back within that is the same, one after it new. So a line Claude Code's fullscreen view scrolls a row isn't new at its new row (one row has its text in place of another), a reply streaming on after a match in its row doesn't make it new, a screen cleared and drawn again isn't new, the next line scrolled into a row with its match where another's was is new, and the same line printed again while the first is still on the screen (`echo love` twice) is new, each occurrence once. A line printed again just as its twin leaves the screen, in the same frame or within 10 seconds of it, keeps the count where it was and isn't. Effects also have their cooldown.
- **Hearts** are vector shapes (a path of quadratic curves in `triggers.rs`, drawn through the glyph cache like the tab panel's rounded corners) in z-layer 20: over the panes and the tab bar, under the tab panel and modals. Each heart's path is `lbterm_triggers::hearts::Path`. With the compositor and motion on DWM plays them (since 2026-10-31): their picture is drawn once into a sheet of one heart in each pink, a template never seen itself, and each heart is a visual showing its pink's square of it, its rise, sway, growth and fade handed to DWM as it rises (`Path::tracks`: the rise, growth and fade exactly, the sway a sine sampled into cubics within a tenth of a pixel), a burst's hearts cut off at their pane's top (`prepare_hearts` in `compositor.rs`). Each frame says where each pane with a burst in the air is, so a burst moves with its pane (a split, a zoom, the window resized) and is cut to nothing while its pane isn't shown (its tab switched from, another pane zoomed), coming back where it is if the pane shows again before it's over, as the GUI's own drawing has them (`lbterm_triggers::hearts::follow`). A burst paints a frame or two where it painted the whole window every 16 ms for two seconds (lbterm/PERF.md, "What a line costs"). Without the compositor, or with motion off (the default `lbterm_motion.enabled = 'system'` on a PC with Windows' animation effects off, Lachlan's among them), the GUI draws them, worked out from the burst's age every 16 ms on a timer of their own, because the GUI's animation frames stop while the window is unfocused, over each pane as it is in that frame; with the compositor into `over`.

The logic, without the GUI: `cargo test -p lbterm-triggers` (rules and the easter eggs, colours, what's new, the hearts' paths).

## Checking it

`pwsh -File lbterm\perf\line-work.ps1` holds what the triggers cost without a capture: a fullscreen view scrolled a line at a time tells Lua only of the new line's matches, a burst of hearts paints a few frames, love on the screen through 11 seconds at rest isn't new at the next frame, the love line printed again in another row is new once, and without a handler no Lua event runs.

`pwsh -File lbterm\perf\triggers.ps1 [-MotionOff]` opens a test window (no config, so the easter eggs; on the right-most screen, without focus; motion on, so DWM plays the hearts, or with `-MotionOff` off, so the GUI draws them) that prints a line with "love" and "LBTerm" in it, and captures it four times over the first second and a half: `target\perf\triggers\triggers-<ms>.png`. It fails when a capture in the first 900 ms has no pink in the pane's bottom right corner, where the hearts rise. Then a key prints love again, and the tab is switched away and back while those hearts rise: none may show on the other tab, and they must show again over theirs. A locked PC leaves DirectComposition out of captures, so then it checks no pixels and says so.
