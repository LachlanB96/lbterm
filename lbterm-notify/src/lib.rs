//! LBTerm's kitty desktop notifications: OSC 99
//! (<https://sw.kovidgoyal.net/kitty/desktop-notifications/>).
//!
//! A program sends `ESC ] 99 ; key=value:key=value ; payload ESC \`. The keys say what the
//! payload is (a title, a body, buttons), whether more chunks follow, what a click does
//! (focus the pane, report back to the program, or both), when to show it (always, only when
//! the window is unfocused, only when the pane can't be seen), how urgent it is, and when it
//! expires. A program can also close a notification, ask which are still open, and ask what
//! the terminal supports.
//!
//! Claude Code's `kitty` notification channel speaks it. WezTerm only has OSC 9 and 777: a
//! title and a body, and a click that does nothing in particular.
//!
//! What's here is the protocol without the GUI: [`parse`] reads one sequence, [`Assembler`]
//! puts chunks together into [`Command`]s, and [`reply`] writes what goes back to the
//! program. Tested with `cargo test -p lbterm-notify`.

pub mod assemble;
pub mod parse;
pub mod reply;

pub use assemble::{Assembler, Command, Notification};
pub use parse::{parse, Actions, Occasion, PayloadType, Request};
