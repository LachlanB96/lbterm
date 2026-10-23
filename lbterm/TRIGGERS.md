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

Every new match fires a Lua event, effect or not:

```lua
wezterm.on('lbterm-trigger', function(window, pane, name, text)
  wezterm.log_info('trigger ' .. name .. ' saw ' .. text)
end)
```

## How it works

- **Matching** happens as each line is drawn (`wezterm-gui/src/termwindow/render/triggers.rs`): all the rules run as one `RegexSet` over the line's text, so a line nothing matches costs one pass, and the result is kept until the line's sequence number changes. Only drawn lines are matched: a background tab is matched when it's shown, and output that scrolls past between two frames isn't seen.
- **Styles** colour a copy of the line that is drawn instead of it. The terminal's own cells never change, so copying text, scrollback search and the line caches (keyed by a hash of the line, colours included) all behave as before, and scrollback is styled when it's scrolled to.
- **New** means that rule hasn't matched that text at that place (row and column) in the last 10 seconds (`Seen` in `lbterm-triggers`). Claude Code redraws its lines many times a second, and a match staying on screen stays seen; one that scrolls away or is cleared is new again when it comes back. Effects also have their cooldown.
- **Hearts** are vector shapes (a path of quadratic curves in `triggers.rs`, drawn through the glyph cache like the tab panel's rounded corners) in z-layer 20: over the panes and the tab bar, under the tab panel and modals. Where each heart is at each moment is `lbterm_triggers::hearts::frame`, worked out from the burst's age, so a slow frame doesn't slow them. They're redrawn every 16 ms on a timer of their own, because the GUI's animation frames stop while the window is unfocused.

The logic, without the GUI: `cargo test -p lbterm-triggers` (rules and the easter eggs, colours, what's new, the hearts' paths).

## Checking it

`pwsh -File lbterm\perf\triggers.ps1` opens a test window (no config, so the easter eggs; on the right-most screen, without focus) that prints a line with "love" and "LBTerm" in it, and captures it four times over the first second: `target\perf\triggers\triggers-<ms>.png`.
