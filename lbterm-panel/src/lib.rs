//! LBTerm's tab panel (lbterm/TAB-PANEL.md): a rail of Claude Code status
//! pads down the left of the window that opens into a list of tabs while
//! the pointer rests on it.
//!
//! It shows what Terminal's TabPanel.exe and APCommand's Terminal mode show,
//! in their colours, but reads it from inside the terminal: each tab's
//! Claude state from the `claude_status` user var that claudehook.exe sets
//! in its pane, and from the glyph Claude Code puts in front of its title.
//! Nothing is polled and no file is read.
//!
//! What's here is the logic, without the GUI: [`row`] turns a tab into what
//! its row shows, [`Hover`] decides when the panel opens and closes and how lit it is,
//! [`Layout`] says where every part of it goes, and [`wave`] what washes ahead of its edge as it
//! opens. Tested with `cargo test -p lbterm-panel`.

pub mod claude;
pub mod hover;
pub mod idle;
pub mod layout;
pub mod look;
pub mod names;
pub mod palette;
pub mod shape;
pub mod text;
pub mod wave;
pub mod words;

pub use claude::{row, Blocked, Changes, LinkState, Program, ProgramState, Quiet, QuietLevel, Row, RowState, Status, Tab, TabLink, waits_on_you};
pub use hover::{Hover, Light, Phase};
pub use idle::Bar;
pub use layout::{Area, Layout};
pub use shape::{Shape, Side, Template};
pub use words::{Font, Placed, Word};
