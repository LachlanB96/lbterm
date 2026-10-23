//! LBTerm's triggers: rules that watch the text a pane shows, after iTerm2's triggers (a
//! pattern and what to do when it appears). There's no standard for this; iTerm2's is the one
//! terminals copy, and WezTerm's `hyperlink_rules` are the same idea limited to links.
//!
//! A rule matches a regex against each line as the pane draws it, and can:
//! - **style** what it matched: a tint towards a colour, or a rainbow ([`Style`]), applied while
//!   drawing, so scrollback is styled too and the text itself is never changed;
//! - play an **effect** once per new occurrence, at most once per cooldown ([`Effect`]);
//! - tell Lua, through the `lbterm-trigger` event, once per new occurrence.
//!
//! Without `lbterm_triggers` in the config, LBTerm has two built in ([`easter_eggs`]): "love"
//! goes a little pink and sends hearts up from the pane's corner, and "LB" goes rainbow.
//!
//! What's here is the logic, without the GUI: [`Rules`] finds matches in a line of cells,
//! [`Seen`] decides which are new, and [`colour`] works out the colours. Tested with
//! `cargo test -p lbterm-triggers`.

pub mod colour;
pub mod hearts;
pub mod rules;
pub mod seen;

pub use colour::Rgb;
pub use rules::{easter_eggs, Cell, Effect, Hit, Rule, Rules, Spec, Style};
pub use seen::{Fire, Seen};
