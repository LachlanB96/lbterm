//! LBTerm's motion (lbterm/COMPOSITOR.md, and the rules in
//! `.claude/skills/lbterm-motion`).
//!
//! Motion is data: a [`Curve`] parsed from CSS-style text in the config, a
//! [`Motion`] moving one number along it, sampled into the cubic segments
//! that DirectComposition plays on its own thread, a point carried along a
//! path ([`path::along`]), a [`plan`] of which panes move when the
//! layout changes, the tabs in flight as one is left while others still fade
//! ([`flight`]), and what of the active outline fades with a tab left
//! ([`outline::switch`]), the menus' fade and their highlight's glide ([`menu`]), and the light sent
//! along the rail to the Conductor's pad on a hide or a show ([`send`]). [`QuadSpans`] records which of a
//! frame's quads belong to which visual, and a [`content::Key`] what a frame
//! would draw into one, so a swapchain that shows it already is left alone.
//! Nothing here touches Windows, so all of it is tested with
//! `cargo test -p lbterm-motion`.

pub mod content;
pub mod curve;
pub mod flight;
pub mod menu;
pub mod motion;
pub mod outline;
pub mod path;
pub mod plan;
pub mod rail;
pub mod send;
pub mod spans;
pub mod wash;

pub use curve::{Curve, Easing, Spring};
pub use flight::{Flight, Leaving, Switched};
pub use motion::{sample, Chain, Cubic, Motion, Segments, Span};
pub use plan::{dim_level, dim_tint, dissolve, dissolve_from, focus, gave_way, plan, Change, Focus, Rect};
pub use spans::{
    command_list_for, command_lists, CardPart, PanelPart, Passes, PixelRect, QuadSpans, Target,
    SWAPCHAINS_PER_COMMAND_LIST,
};
