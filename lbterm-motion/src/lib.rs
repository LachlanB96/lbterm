//! LBTerm's motion (lbterm/COMPOSITOR.md, and the rules in
//! `.claude/skills/lbterm-motion`).
//!
//! Motion is data: a [`Curve`] parsed from CSS-style text in the config, a
//! [`Motion`] moving one number along it, sampled into the cubic segments
//! that DirectComposition plays on its own thread, a point carried along a
//! path ([`path::along`]), and a [`plan`] of which panes move when the
//! layout changes. [`QuadSpans`] records which of a
//! frame's quads belong to which visual. Nothing here touches Windows, so
//! all of it is tested with `cargo test -p lbterm-motion`.

pub mod curve;
pub mod motion;
pub mod path;
pub mod plan;
pub mod spans;

pub use curve::{Curve, Easing, Spring};
pub use motion::{Chain, Cubic, Motion, Segments};
pub use plan::{dim_level, focus, plan, Change, Focus, Rect};
pub use spans::{
    command_list_for, command_lists, CardPart, PanelPart, PixelRect, QuadSpans, Target,
    SWAPCHAINS_PER_COMMAND_LIST,
};
