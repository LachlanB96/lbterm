//! LBTerm's smaller additions to the GUI, as logic that's tested without a window
//! (`cargo test -p lbterm-extras`). The GUI side of each lives in wezterm-gui.

pub mod arrival;
pub mod conductor;
pub mod gpu;
pub mod hover;
pub mod layouts;
pub mod lights;
pub mod marks;
pub mod menu;
pub mod prompts;
pub mod questions;
pub mod restart;
pub mod restore;
pub mod scrollbar;
pub mod slow;
pub mod state;
pub mod strip;
pub mod summaries;
pub mod summon;
pub mod tabs;
pub mod taskbar;
