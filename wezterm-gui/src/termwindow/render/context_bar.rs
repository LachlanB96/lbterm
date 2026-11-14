//! LBTerm: the context bar down the window's right (`config.lbterm_context_bar`; lbterm/CONTEXT.md,
//! "The context bar"). Lachlan's ask of 2026-11-08: "lets not show the objectives bar ... We
//! should replace its space with a 'context bar'. The context bar will show how the context is
//! used ... memory files might be yellow, and will show a seperate section for each file."
//!
//! At rest it's a strip in the objectives strip's room, right of the scroll strip, the window
//! giving it up like a border (borders.rs, fancy_tab_bar.rs), ringed as the objectives strip is.
//! Inside the ring is the track, standing for the open tab's Claude Code conversation's whole
//! context window, top to bottom: each file loaded into it a section of its own (memory files
//! yellow, CLAUDE.md files grey, another project's pulled in amber), then Claude Code's own parts,
//! then the conversation, and below them free space, the track showing through. The sections are
//! lbterm_extras::context_bar's, made from the conversation reader's snapshot
//! (crate::conversations, `Reader::Bar`, off this thread) and the window's size from the session's
//! `usage.json` (crate::summaries), and laid out into runs of whole pixels
//! (`context_bar::runs`) only when the context's `Arc`, the window's size or the track change: a
//! frame painted for anything else lays out nothing and reads no file.
//!
//! Drawn in `over` as shapes for render/sides.rs with the scroll strip, so it fades with a tab's
//! panes as the strips do. With the pointer on it, its ring lights at once; after a rest the bar
//! itself widens to the closed rail's width over the terminal (Lachlan's 4b of 2026-11-09), its
//! sections laid out again with a line's room each and each one's tokens on it, a pane as wide as
//! it coming out beside it once it has widened with the numbers of sections too thin for theirs;
//! resting on a section slides the pane out from under the widened bar, every section listed with
//! its path, kind, tokens and button, its totals pinned at its foot when the rows don't all fit,
//! the scroll strip riding on the left edge of what's out (lbterm/CONTEXT.md, "At rest and
//! widened", "The pane"). The widened bar, the thin numbers' pane, the pane and the riding strip
//! are visuals of their own with the compositor (`Target::ContextBar`), slid by DWM (compositor.rs,
//! `prepare_context_bar`); the terminal keeps its size throughout, so no pane is resized. Their
//! places and words are laid out only when the sections, the bar's place or the font change (the
//! pane only once it's out), and their words kept (render/words.rs).

use crate::termwindow::render::objectives::{over, EVIDENCE_ALPHA, ROW_HOVER_ALPHA, TEXT_ALPHA, TITLE_ALPHA};
use crate::termwindow::render::sides;
use crate::termwindow::render::tab_panel::srgb;
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::{ConfigHandle, DimensionContext, SrgbaTuple};
use lbterm_extras::context::{self as ctx, Context};
use lbterm_extras::context_bar::{self, Bar, Run, Waiting, What};
use lbterm_extras::context_edits::{self, State as EditState};
use lbterm_extras::strip::{self as strip_card, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::{PixelRect, Target};
use lbterm_panel::{palette, Hover, Shape};
use mux::pane::PaneId;
use mux::Mux;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use window::color::LinearRgba;

// Its z-layers are its visuals' alone, and go to them by number before any owner after it in
// composited.rs's `lbterm_ranges` (lbterm/PITFALLS.md, 207): 101 to 107, past upstream's
// overlays' 100, where no one has a number and seven in a row are free. Stage 3 took 51 to 57, which are
// the tab panel's (`PANEL_ZINDEX` + 1 to + 7: the rail's ring and body, its words, its lights and
// its spinner), and the rail lost them to the bar's visuals: `the_context_bars_z_layers_are_its_own`
// holds them apart from every other owner's
/// The pane's shadow, card and words (`Target::ContextBar(0)`)
pub(crate) const PANE_Z: [i8; 3] = [101, 102, 103];
/// The scroll strip while it rides on the drawer's or the pane's left edge (`Target::ContextBar(2)`)
pub(crate) const RIDE_Z: i8 = 104;
/// The widened bar: its shadow, card with its track and sections, and its numbers
/// (`Target::ContextBar(1)`; the drawer beside the bar until the second round's 4b)
pub(crate) const DRAWER_Z: [i8; 3] = [105, 106, 107];
/// The pane beside the widened bar with the thin sections' numbers: its shadow, card and numbers
/// (`Target::ContextBar(3)`)
pub(crate) const THIN_Z: [i8; 3] = [108, 109, 110];

/// Which of the context bar's visuals draws z-layer `zindex`, when it's one of theirs
pub fn target_of(zindex: i8) -> Option<Target<PaneId>> {
    if PANE_Z.contains(&zindex) {
        Some(Target::ContextBar(0))
    } else if DRAWER_Z.contains(&zindex) {
        Some(Target::ContextBar(1))
    } else if zindex == RIDE_Z {
        Some(Target::ContextBar(2))
    } else if THIN_Z.contains(&zindex) {
        Some(Target::ContextBar(3))
    } else {
        None
    }
}

/// A card's ring: white this far over it
const RING_ALPHA: f32 = 0.14;
/// The hairlines from a moved label to its section
const LEADER_ALPHA: f32 = 0.30;
/// A button's pill
const BUTTON_ALPHA: f32 = 0.10;
/// A button under the pointer
const BUTTON_LIT: f32 = 0.22;

/// The bar's words while the transcript hasn't been read yet
const READING: &str = "Reading the conversation…";
/// The pane's foot: it types `/compact` into the tab (CONTEXT.md, "The buttons")
const COMPACT: &str = "Compact now";
/// Compact now dim, its pill and its words
const BUTTON_DIM: f32 = 0.05;
/// Compact now's words once it's typed `/compact`, until the reader has read the compaction or
/// a turn after it: what it waits for, so they're true after a cancel too (Lachlan's 11b of
/// 2026-11-14; CONTEXT.md, "A `/compact` cancelled with Esc")
const SENT: &str = "Sent /compact: waiting for its compaction or your next message";
/// Compact now with no Claude Code pane to type into
const NO_PANE: &str = "No Claude Code in this tab";

/// Whether the bar is on: asked for, with the transcripts it reads (`lbterm_context`), the
/// summaries the window's size comes from, and the tab panel whose rail it lines up with
pub(crate) fn on(config: &ConfigHandle) -> bool {
    config.lbterm_context_bar.is_some()
        && config.lbterm_context.is_some()
        && config.lbterm_summaries.is_some()
        && config.lbterm_tab_panel.enabled
}

/// The bar's own width at rest, its ring included, in pixels
fn width_px(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    config.lbterm_context_bar.as_ref().map_or(0.0, |b| {
        b.width
            .evaluate_as_pixels(DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: dimensions.pixel_width as f32,
                pixel_cell: metrics.cell_size.width as f32,
            })
            .round()
    })
}

/// What the window keeps down its right for the bar (the bar and a gutter between it and the
/// window's edge), 0 with it off: the objectives strip's room, which it takes (render/objectives.rs
/// keeps none while the bar is on). The window treats it as border (borders.rs), so the panes,
/// the scrollbar and the mouse leave room for it, and the tab bar takes it back
/// (fancy_tab_bar.rs). It never changes as the bar widens or its pane opens: they're drawn over
/// the terminal, so no PTY is resized (Lachlan's 7a).
pub fn strip_width(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    if !on(config) {
        return 0.0;
    }
    width_px(config, dimensions, metrics) + Shape::new(dimensions.dpi as f32 / 96.0).gutter()
}

/// What the sections were laid out from: the reader's context (a new snapshot's is the same `Arc`
/// until something in it changes), the window's size, and the track and its least heights in
/// whole pixels
struct LaidFrom {
    context: Option<Arc<Context>>,
    window: Option<u64>,
    track: [i64; 4],
    floor: u32,
    gap: u32,
    /// The pane its conversation runs in, and the removals' generation (crate::context_edits):
    /// a Remove or an Undo changes the section's row and hatches its run
    pane: Option<PaneId>,
    edits: u64,
}

impl PartialEq for LaidFrom {
    fn eq(&self, o: &Self) -> bool {
        let same = match (&self.context, &o.context) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        same && (self.window, self.track, self.floor, self.gap, self.pane, self.edits) == (o.window, o.track, o.floor, o.gap, o.pane, o.edits)
    }
}

/// The sections as laid out: the bar, or what it waits for, its runs, and their shapes
struct Laid {
    /// None while the transcript hasn't been read
    bar: Option<Result<Bar, Waiting>>,
    runs: Vec<Run>,
    filled: u32,
    scale: Option<f64>,
    shapes: Vec<sides::Shape>,
    /// Each section's button and second line, as its removal stands
    looks: Vec<Look>,
    /// The removals pressed in the pane whose section has gone from the bar (a compaction took
    /// it): each a row of the pane with its Undo, numbered on from the sections
    /// (lbterm_extras::context_edits::off_the_bar)
    gone: Vec<(context_bar::Removal, Look)>,
    /// The runs' least height and gap at rest, in pixels: the widened bar's gap is the same
    floor: (u32, u32),
}

/// A section's button and second line as its removal stands (crate::context_edits): Remove, or
/// Undo once its line is out, and what happened; a pulled-in file's note; nothing for the rest
#[derive(Clone, Default)]
struct Look {
    button: Option<&'static str>,
    note: Option<String>,
    removed: bool,
}

/// The looks of `bar`'s sections, from the removals pressed in its pane
fn looks(bar: &Bar, edits: &[(context_bar::Removal, EditState)]) -> Vec<Look> {
    bar.sections
        .iter()
        .map(|s| match context_edits::section_removal(s, edits) {
            Some(removal) => {
                let state = edits.iter().find(|(r, _)| r == removal).map(|(_, s)| s);
                // A file attached again after a compaction's summary: its row says that's what
                // keeps it in
                let attached = s.what == What::Attached;
                Look {
                    button: Some(state.map_or("Remove", EditState::button)),
                    note: state.map(|st| if attached { st.attached_note(removal) } else { st.note(removal) }),
                    removed: state.map_or(false, EditState::removed),
                }
            }
            None => Look { button: None, note: s.note.map(String::from), removed: false },
        })
        .collect()
}

/// The widened bar as laid out (Lachlan's 4b of 2026-11-09): the bar itself at the closed rail's
/// width, its sections laid out again for its track with a line's floor and each number on its
/// own section, and the pane beside it for the numbers of sections too thin for theirs
struct Drawer {
    /// The widened bar, its ring included, and its track
    rect: Rect,
    track: Rect,
    /// Its runs (`context_bar::runs` with a line and its gap the floor)
    runs: Vec<Run>,
    /// Each run's colour, and a removed section's hatching in the track's
    shapes: Vec<(Rect, u32)>,
    /// Each number on its section, or free space's on the track: its section, where its words go,
    /// its words, and whether they're dark (on a light colour)
    labels: Vec<(usize, Rect, String, bool)>,
    thin: Option<Thin>,
}

impl Drawer {
    /// The pointer's on the widened bar or the pane beside it
    fn contains(&self, x: f64, y: f64) -> bool {
        self.rect.contains(x, y) || self.thin.as_ref().map_or(false, |t| t.rect.contains(x, y))
    }
}

/// The pane beside the widened bar, as wide as it, its right ring on the bar's left: the numbers
/// of sections too thin for their own, each level with its section as far as they fit, pushed
/// apart, a hairline from a moved one to its section (`context_bar::numbers`' `thin`)
struct Thin {
    rect: Rect,
    /// Each label: its section, where its words go, its words
    labels: Vec<(usize, Rect, String)>,
    /// The hairlines from moved labels to their sections
    leaders: Vec<Rect>,
}

/// One of the pane's rows, a section's
struct PaneRow {
    section: usize,
    rect: Rect,
    swatch: Rect,
    colour: u32,
    /// The words' top
    text_y: f64,
    kind: (f64, String),
    path: Option<(f64, f64, String)>,
    tokens: (f64, String),
    /// Its pill, its words and how far in from the pill's left they start
    button: Option<(Rect, &'static str, f64)>,
    note: Option<(f64, f64, String)>,
}

/// The pane as laid out
struct PaneCard {
    rect: Rect,
    /// What's in use of the window, or what the bar waits for
    heading: (f64, f64, String),
    rows: Vec<PaneRow>,
    /// Rows scrolled out above by the wheel (the first shown is that section), counted on a line
    /// over the first shown
    above: usize,
    /// The furthest the wheel can scroll them: the first row of the last page
    most: usize,
    /// Rows left out below for want of room, counted under the last shown
    left_out: usize,
    /// The totals pinned at the foot under a rule while only the files scroll, which they are when
    /// the rows don't all fit (Lachlan's 3a of 2026-11-09): `rows[..scrolled]` are the rows the
    /// wheel scrolls (every row when nothing's pinned), the pinned rows after them, and `above`,
    /// `most` and `left_out` count the scrolled ones
    pinned: bool,
    scrolled: usize,
    rule: Option<Rect>,
    /// Compact now at its foot, once there's a bar
    compact: Option<CompactAt>,
}

/// Where Compact now goes: its pill, its words' place, and the line under it for why it's dim
struct CompactAt {
    rect: Rect,
    text: (f64, f64),
    why: (f64, f64),
}

/// What the drawer and the pane were laid out from: the sections' layout, the bar's place, the
/// font's cell, and the room to the left with the pane's width as asked
#[derive(Clone, Copy, PartialEq)]
struct CardsFrom {
    layouts: u64,
    bar: [i64; 4],
    cell: [i64; 2],
    room: [i64; 2],
    /// The font and its shape generation the words were measured in
    font: (usize, usize),
    /// The pane's rows as far as the wheel scrolled them: only the pane is laid out again for it
    scroll: usize,
}

/// The scroll strip's ride this frame (render/scroll_strip.rs reads it): how far left of its own
/// place it's drawn, to the drawer's or the pane's left edge once they're out, and whether it's in
/// a visual of its own for DWM to glide there. Riding the thin numbers' pane, it glides in the
/// bar's slide with no wait, as the thin pane comes out in it (Lachlan's E9 of 2026-11-10; until
/// round 4's stage 6 it waited for the bar's slide to be done, `after_bar`)
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Ride {
    pub offset: f64,
    pub own: bool,
}

/// Where one of the bar's sliding visuals goes this frame, for the compositor (compositor.rs,
/// `prepare_context_bar`): its surface at its place once out (`bounds`), the window's part it's
/// seen through (`clip`), whether it's coming out or going back, the offset it comes out from and
/// goes back to (`home`; none for out of sight past the clip's right edge), and whether its slide
/// back waits for the thin numbers' pane's, started in the same commit (`after_thin`, the widened
/// bar's; lbterm_extras::context_bar::slide_wait). The thin numbers' pane's `clip` is where it's
/// seen once the bar is out: the compositor moves that clip with the bar's slide (`Slid::follow`),
/// so the thin pane comes out from under the bar's moving edge in the bar's own slide (Lachlan's
/// E9 of 2026-11-10)
#[derive(Clone, Copy, PartialEq)]
pub struct Slide {
    pub bounds: PixelRect,
    pub clip: PixelRect,
    pub open: bool,
    pub home: Option<f64>,
    pub after_thin: bool,
}

/// The pane's, the widened bar's and the thin numbers' pane's slides this frame
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Visuals {
    pub pane: Option<Slide>,
    pub bar: Option<Slide>,
    pub thin: Option<Slide>,
}

pub struct ContextBarState {
    /// The bar at rest as last drawn, its ring included, and its track inside the ring
    rect: Option<Rect>,
    track: Option<Rect>,
    /// The conversation it stands for: the open tab's, and the pane it runs in
    session: Option<String>,
    pane_id: Option<PaneId>,
    laid: Option<(LaidFrom, Laid)>,
    /// How many times the sections have been laid out, for the state file
    layouts: u64,
    /// The pointer is on the bar, the gutter beside it, the drawer or the pane
    pointer_on: bool,
    /// The bar widening (`widen_delay_ms`) and the pane opening (`open_delay_ms`), each closing
    /// `close_delay_ms` after the pointer leaves them all
    widen: Hover,
    pane: Hover,
    /// The section under the pointer, on the bar, the drawer or the pane: its row is lit
    lit: Option<usize>,
    /// The section whose button is under the pointer: its pill lights
    hot: Option<usize>,
    /// How far the wheel scrolled the pane's rows, as the last layout held it (the first shown),
    /// back to the top once the pane has closed; and where the pointer was when it turned, so the
    /// layout after it lights the row now under it
    scroll: usize,
    wheel_at: Option<(f64, f64)>,
    /// The widened bar and the pane as last laid out, and what from: the pane only once it's out
    cards: Option<(CardsFrom, Rc<Drawer>, Option<Rc<PaneCard>>)>,
    /// While open: what the pointer can be on
    drawer: Option<Rc<Drawer>>,
    pane_card: Option<Rc<PaneCard>>,
    /// The ones last drawn, which a closing one slides back showing
    last_drawer: Option<Rc<Drawer>>,
    last_pane: Option<Rc<PaneCard>>,
    /// Where the pane's, the widened bar's and the thin numbers' pane's visuals go this frame and
    /// whether each is coming out, for the compositor
    visuals: Visuals,
    /// The scroll strip's ride, and where it was drawn riding in its own visual, with its own place
    ride: Ride,
    rode: Option<(PixelRect, i32)>,
    wake: Option<Instant>,
    /// A button went down on the bar: its release is the bar's too
    pressed: bool,
    /// Compact now's look this frame while the pane shows: may it type `/compact`, or why not
    compact: Option<Result<(), String>>,
    /// What the last look at the pane's screen was made from (the pane, its seqno, its
    /// `claude_status`) and what it said: looked at again only when one of them moves
    compact_seen: Option<((PaneId, usize, String), Result<(), String>)>,
    /// Why `/compact` didn't get to a pane, said until the pane closes, and only for that pane;
    /// that it was typed is every window's, kept by pane till the compaction (crate::context_edits)
    compact_failed: Option<(PaneId, String)>,
    /// The pointer's on Compact now
    compact_hot: bool,
    /// The widened bar's, the pane's and the thin numbers' pane's words as their last frame laid
    /// them out (render/words.rs)
    words: [std::cell::RefCell<super::words::Words>; 3],
    /// The widths of the words the last layout measured, so the next measures only what's new
    widths: std::cell::RefCell<Widths>,
}

/// Words' widths as `compute_element` lays them out, for the font and shape generation they were
/// measured in (`font`, as CardsFrom has it): `now` holds the layout under way's, found in `last`
/// (the layout before's) or measured, and `last` is the layout under way's once it's done, so
/// what's kept is one layout's words. A reply while the pane is out lays the drawer and the pane
/// out again with only the heading, the tokens and the labels new; a word met twice in one layout
/// (a kind's, a path's cut tried again) is measured once
#[derive(Default)]
struct Widths {
    font: Option<(usize, usize)>,
    last: std::collections::HashMap<String, f64>,
    now: std::collections::HashMap<String, f64>,
}

impl Default for ContextBarState {
    fn default() -> Self {
        Self {
            rect: None,
            track: None,
            session: None,
            pane_id: None,
            laid: None,
            layouts: 0,
            pointer_on: false,
            widen: Hover::new(Duration::from_millis(150), Duration::from_millis(250)),
            pane: Hover::new(Duration::from_millis(300), Duration::from_millis(250)),
            lit: None,
            hot: None,
            scroll: 0,
            wheel_at: None,
            cards: None,
            drawer: None,
            pane_card: None,
            last_drawer: None,
            last_pane: None,
            visuals: Visuals::default(),
            ride: Ride::default(),
            rode: None,
            wake: None,
            pressed: false,
            compact: None,
            compact_seen: None,
            compact_failed: None,
            compact_hot: false,
            words: Default::default(),
            widths: Default::default(),
        }
    }
}

impl crate::TermWindow {
    /// Each paint, after the tab panel (whose rail it lines up with) and before the scroll strip:
    /// the bar at rest, its sections laid out again only when what they're made from changed
    pub(crate) fn paint_lbterm_context_bar(&mut self) -> anyhow::Result<()> {
        let layout = match self.lbterm_panel.layout {
            Some(layout) if on(&self.config) => layout,
            _ => {
                let state = std::mem::take(&mut self.lbterm_context_bar);
                if state.session.is_some() {
                    crate::conversations::shows(self.mux_window_id, crate::conversations::Reader::Bar, None);
                }
                self.lbterm_context_bar.layouts = state.layouts;
                return Ok(());
            }
        };
        let room = self.lbterm_room()?;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let panel = layout.panel(false);
        // Right of the scroll strip, when it's on (render/scroll_strip.rs)
        let bar = Rect {
            x: room.right as f64 + super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64,
            y: panel.y as f64,
            w: width_px(&self.config, &self.dimensions, &self.render_metrics) as f64,
            h: panel.h as f64,
        };
        let stroke = shape.hairline() as f64;
        let radius = (shape.radius() as f64).min(bar.w / 2.0);
        // The track starts as far in as the scroll strip's does (its marks' inset), and clear of
        // the ring's round ends
        let inset = stroke + (2.0 * scale).round();
        let ends = radius.max(inset);
        let track = Rect {
            x: bar.x + inset,
            y: (bar.y + ends).round(),
            w: (bar.w - 2.0 * inset).max(1.0),
            h: (bar.h - 2.0 * ends).max(1.0).round(),
        };

        // The open tab's conversation; none while the Conductor's view stands over the tabs, as
        // the context pill shows none then
        let (sid, pane) = if self.lbterm_view_is_open() {
            (None, None)
        } else {
            self.lbterm_active_tab()
                .and_then(|(_, tab)| super::prompt::target(&tab))
                .map_or((None, None), |t| (Some(t.sid), Some(t.pane.pane_id())))
        };
        // The reading thread follows it for as long as the bar shows it
        crate::conversations::shows(self.mux_window_id, crate::conversations::Reader::Bar, sid.as_deref());
        if sid != self.lbterm_context_bar.session {
            self.lbterm_context_bar.session = sid.clone();
        }
        self.lbterm_context_bar.pane_id = pane;
        let (context, window) = match sid.as_deref() {
            Some(sid) => (
                crate::conversations::get(sid).map(|s| s.context.clone()),
                crate::summaries::get(sid).and_then(|s| s.usage).and_then(|u| u.window),
            ),
            None => (None, None),
        };
        let from = LaidFrom {
            context,
            window,
            track: [track.x, track.y, track.w, track.h].map(|v| v.round() as i64),
            floor: (config::lbterm::CONTEXT_SECTION_MIN * scale).round().max(1.0) as u32,
            gap: (config::lbterm::CONTEXT_SECTION_GAP * scale).round().max(1.0) as u32,
            pane,
            edits: crate::context_edits::generation(),
        };
        if self.lbterm_context_bar.laid.as_ref().map_or(true, |(f, _)| *f != from) {
            let edits = pane.map(crate::context_edits::states).unwrap_or_default();
            let laid = lay_out(&from, sid.is_some(), track, &edits);
            self.lbterm_context_bar.laid = Some((from, laid));
            self.lbterm_context_bar.layouts += 1;
        }

        let surface = self.palette().background;
        // Under it, from the panes' room (past the scroll strip's) to the window's edge (inside
        // any border of the frame's own), under the tab bar: the panes' colour over the border's
        let from_x = bar.x;
        let edge = room.right as f64
            + super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64
            + strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let under = Rect {
            x: from_x,
            y: layout.body_top as f64,
            w: edge - from_x,
            h: (layout.bottom - layout.body_top) as f64,
        };
        let outline = if self.lbterm_context_bar.pointer_on {
            palette::blend(palette::OUTLINE, 0xFFFFFF, palette::PENDING)
        } else {
            palette::OUTLINE
        };
        let inner = Rect { x: bar.x + stroke, y: bar.y + stroke, w: bar.w - 2.0 * stroke, h: bar.h - 2.0 * stroke };
        let mut shapes = Vec::with_capacity(4 + self.lbterm_context_bar.laid.as_ref().map_or(0, |(_, l)| l.shapes.len()));
        shapes.push(sides::Shape::new(under, 0.0, surface.to_linear()));
        shapes.push(sides::Shape::new(bar, radius, srgb(outline)));
        shapes.push(sides::Shape::new(inner, radius - stroke, surface.to_linear()));
        // The track is the free space: what the sections don't fill shows it
        shapes.push(sides::Shape::new(track, 0.0, srgb(palette::TRACK)));
        if let Some((_, laid)) = self.lbterm_context_bar.laid.as_ref() {
            shapes.extend_from_slice(&laid.shapes);
        }
        self.lbterm_sides_draw(shapes);
        let state = &mut self.lbterm_context_bar;
        state.rect = Some(bar);
        state.track = Some(track);
        self.paint_lbterm_context_widened(bar, track, panel.w as f64, room.left as f64, stroke, scale)
    }

    /// The widened bar's drawer and the pane, while they're out or sliding back, and the scroll
    /// strip's ride on their left edge
    fn paint_lbterm_context_widened(&mut self, bar: Rect, track: Rect, rail: f64, left: f64, stroke: f64, scale: f64) -> anyhow::Result<()> {
        let settings = self.config.lbterm_context_bar.clone().unwrap_or_default();
        let motion = self.lbterm_motion();
        let composited = self.lbterm_composited();
        let moves = composited && motion.enabled;
        let now = Instant::now();
        {
            let state = &mut self.lbterm_context_bar;
            let close = Duration::from_millis(settings.close_delay_ms);
            state.widen.set_delays(Duration::from_millis(settings.widen_delay_ms), close);
            state.pane.set_delays(Duration::from_millis(settings.open_delay_ms), close);
            let (opening, closing) = if moves {
                (Duration::from_secs_f64(motion.panel_open.duration()), Duration::from_secs_f64(motion.panel_close.duration()))
            } else {
                (Duration::ZERO, Duration::ZERO)
            };
            state.widen.set_motion(opening, closing);
            state.pane.set_motion(opening, closing);
            state.widen.tick(now);
            state.pane.tick(now);
        }
        // What still slides, from the compositor as the last frame left it. Only with motion: with
        // it off nothing slides, and the compositor's word lags this frame's by one, which no
        // wake would come to put right (the bar stayed wide once the pane had closed)
        let (showing, settles) = if moves {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(([false; 4], None), |c| c.context_bar_showing(now))
        } else {
            ([false; 4], None)
        };
        let pane_open = self.lbterm_context_bar.pane.is_open();
        // The pane of thin sections' numbers stays out while the pane is out or sliding back, and
        // the bar stays wide while that's out too, narrowing once it's in (CONTEXT.md, "The pane"):
        // both go back in the one commit `held` ends in, the bar's slide waiting for the thin
        // pane's (lbterm_extras::context_bar::out; FLOWS.md, "The context bar's widening")
        let held = self.lbterm_context_bar.widen.is_open() || pane_open || showing[0];
        let parts = lbterm_extras::context_bar::out(held, showing[1], showing[3]);

        // Laid out again only when the sections, the bar's place, the font or the room changed
        let (font, metrics, cell) = self.lbterm_objectives_font()?;
        let pane_w = settings
            .pane_width
            .evaluate_as_pixels(DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.dimensions.pixel_width as f32,
                pixel_cell: self.render_metrics.cell_size.width as f32,
            })
            .round() as f64;
        let from = CardsFrom {
            layouts: self.lbterm_context_bar.layouts,
            bar: [bar.x, bar.y, bar.w, bar.h].map(|v| v.round() as i64),
            cell: [cell.0 * 64.0, cell.1].map(|v| v.round() as i64),
            room: [left.round() as i64, pane_w as i64],
            font: (Rc::as_ptr(&font) as *const () as usize, self.shape_generation),
            scroll: self.lbterm_context_bar.scroll,
        };
        // The pane is laid out only once it's out: the widening's frame lays out the widened bar
        // alone, where until the second round it laid out the pane it didn't show as well (about
        // forty words measured and the paths cut, on the frame that starts the widening's slide)
        let stale = self.lbterm_context_bar.cards.as_ref().map_or(true, |(f, _, _)| *f != from);
        let no_pane = pane_open && self.lbterm_context_bar.cards.as_ref().map_or(true, |(_, _, p)| p.is_none());
        if parts.lay_out && (stale || no_pane) {
            // Each word measured as it lays out: a few dozen, only when what they're laid out
            // from changed, never for a frame of its own; the wheel's scroll alone lays out the
            // pane and keeps the widened bar
            {
                let mut widths = self.lbterm_context_bar.widths.borrow_mut();
                if widths.font != Some(from.font) {
                    *widths = Widths { font: Some(from.font), ..Default::default() };
                }
            }
            let widths = &self.lbterm_context_bar.widths;
            let measure = |text: &str| {
                let known = {
                    let mut w = widths.borrow_mut();
                    let w = &mut *w;
                    match w.now.get(text) {
                        Some(width) => Some(*width),
                        None => w.last.remove(text).inspect(|width| {
                            w.now.insert(text.to_string(), *width);
                        }),
                    }
                };
                known.unwrap_or_else(|| {
                    let width = self.lbterm_context_width(&font, &metrics, text);
                    widths.borrow_mut().now.insert(text.to_string(), width);
                    width
                })
            };
            let laid = self.lbterm_context_bar.laid.as_ref().map(|(_, l)| l);
            let kept = self
                .lbterm_context_bar
                .cards
                .as_ref()
                .filter(|(f, _, _)| CardsFrom { scroll: from.scroll, ..*f } == from)
                .map(|(_, d, _)| d.clone());
            let drawer = kept.unwrap_or_else(|| Rc::new(lay_out_drawer(laid, bar, track, rail, stroke, cell.1, scale, &measure)));
            let home = config::HOME_DIR.to_string_lossy().to_string();
            let session = self.lbterm_context_bar.session.is_some();
            let pane = pane_open.then(|| lay_out_pane(laid, session, &drawer, left, pane_w, stroke, cell.1, scale, &home, from.scroll, &measure));
            {
                let mut w = widths.borrow_mut();
                w.last = std::mem::take(&mut w.now);
            }
            let state = &mut self.lbterm_context_bar;
            // Held where the layout clamped it, so a wheel turned on past the end keeps nothing in
            // hand; the key with it, so that's no layout again
            let scroll = pane.as_ref().map_or(from.scroll, |p| p.above);
            state.scroll = scroll;
            let from = CardsFrom { scroll, ..from };
            // The row now under the pointer the wheel turned at lights, and its button
            if let (Some((x, y)), Some(pane)) = (state.wheel_at, pane.as_ref()) {
                state.wheel_at = None;
                let row = pane.rows.iter().find(|r| r.rect.contains(x, y));
                state.lit = row.map(|r| r.section).or(state.lit);
                state.hot = row.filter(|r| r.button.map_or(false, |(b, _, _)| b.contains(x, y))).map(|r| r.section);
            }
            state.cards = Some((from, drawer, pane.map(Rc::new)));
        }
        let fresh = self.lbterm_context_bar.cards.as_ref().map(|(_, d, p)| (d.clone(), p.clone()));
        let state = &mut self.lbterm_context_bar;
        let drawer = match parts.bar {
            CardDraw::Fresh => fresh.as_ref().map(|(d, _)| d.clone()),
            CardDraw::Last => state.last_drawer.clone(),
            CardDraw::None => None,
        };
        let pane = match strip_card::card_draw(pane_open, showing[0]) {
            CardDraw::Fresh => fresh.as_ref().and_then(|(_, p)| p.clone()),
            CardDraw::Last => state.last_pane.clone(),
            CardDraw::None => {
                // In and gone: it opens again at the top (laid out again when next it's out)
                state.scroll = 0;
                None
            }
        };
        // The pane of thin sections' numbers: out while held, sliding back after, from the widened
        // bar it's part of
        let thin = match parts.thin {
            CardDraw::Fresh => fresh.as_ref().map(|(d, _)| d.clone()),
            CardDraw::Last => state.last_drawer.clone(),
            CardDraw::None => None,
        }
        .filter(|d| d.thin.is_some());
        // Only open ones take the pointer
        state.drawer = drawer.clone().filter(|_| parts.takes_pointer);
        state.pane_card = pane.clone().filter(|_| pane_open);

        // The scroll strip rides on the left edge of what's out: the pane's, else the thin
        // numbers' pane's (which comes out in the bar's own slide, so the strip glides with it from
        // the widening's commit), else the widened bar's while it's held out; going back from out
        // it goes home with the thin pane, under the widened bar until that narrows past it
        let thin_out = thin.as_ref().filter(|_| held).and_then(|d| d.thin.as_ref()).map(|t| t.rect.x);
        let card = state.drawer.as_ref().filter(|_| parts.ride_on_card);
        let edge = match (&state.pane_card, thin_out, card) {
            (Some(p), _, _) => p.rect.x,
            (None, Some(x), _) => x,
            (None, None, Some(d)) => d.rect.x,
            _ => bar.x,
        };
        let offset = if super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics) > 0.0 {
            (edge - bar.x).round()
        } else {
            0.0
        };
        let state = &mut self.lbterm_context_bar;
        let was_away = state.ride.own && state.ride.offset != 0.0;
        state.ride = Ride { offset, own: lbterm_extras::context_bar::ride_own(moves, offset, showing[2], was_away) };

        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        // Each visual's surface at its place once out, and its clip ending at its card's right
        // edge: its content slides out from under that edge. The widened bar's clip ends inside
        // the bar's own right ring, which shows through it the whole time, and its content comes
        // out from where the bar is (its left edge on the bar's) and goes back there: the bar
        // itself widens (Lachlan's 4b of 2026-11-09). The thin numbers' pane's clip ends on the
        // widened bar's left ring, and the compositor moves it with the bar's slide, so the thin
        // pane comes out from under the bar's moving edge (his E9 of 2026-11-10)
        let slide = |rect: &Rect, right: f64, open: bool, home: Option<f64>, after_thin: bool| {
            strip_card::visual_bounds(rect, scale, window).map(|(x, y, w, h)| {
                let right = right.round() as i32;
                Slide { bounds: PixelRect::new(x, y, w, h), clip: PixelRect::new(x, y, (right - x).max(0) as u32, h), open, home, after_thin }
            })
        };
        state.visuals = Visuals {
            pane: pane.as_ref().and_then(|p| slide(&p.rect, (p.rect.x + p.rect.w).ceil(), pane_open, None, false)),
            bar: drawer.as_ref().and_then(|d| slide(&d.rect, bar.x + bar.w - stroke, parts.bar_open, Some((bar.x - d.rect.x).round()), true)),
            thin: thin.as_ref().and_then(|d| d.thin.as_ref()).and_then(|t| slide(&t.rect, (t.rect.x + t.rect.w).ceil(), held, None, false)),
        };
        let surface = self.palette().background;
        let radius = self.lbterm_shape().radius() as f64;
        let lit = self.lbterm_context_bar.lit.filter(|_| pane_open);
        let hot = self.lbterm_context_bar.hot.filter(|_| pane_open);
        // Compact now's look at the tab's pane, only while the pane's out; what it said of a press
        // goes once the pane has closed
        if pane_open {
            let look = self.lbterm_context_compact_look(false);
            self.lbterm_context_bar.compact = Some(look);
        } else {
            let state = &mut self.lbterm_context_bar;
            state.compact_failed = None;
            if pane.is_none() {
                state.compact = None;
                state.compact_seen = None;
            }
        }
        if let Some(thin) = thin.as_ref().and_then(|d| d.thin.as_ref()) {
            self.lbterm_context_draw_thin(thin, &font, &metrics, cell, surface, radius, stroke, scale)?;
        }
        if let Some(drawer) = drawer {
            // Its ring the bar's, lit as the bar's is under the pointer
            let ring = if self.lbterm_context_bar.pointer_on {
                palette::blend(palette::OUTLINE, 0xFFFFFF, palette::PENDING)
            } else {
                palette::OUTLINE
            };
            self.lbterm_context_draw_drawer(&drawer, srgb(ring), &font, &metrics, cell, surface, radius, stroke, scale)?;
            self.lbterm_context_bar.last_drawer = Some(drawer);
        }
        if let Some(pane) = pane {
            let compact = self.lbterm_context_bar.compact.clone();
            let compact_hot = self.lbterm_context_bar.compact_hot && pane_open;
            self.lbterm_context_draw_pane(&pane, lit, hot, compact.as_ref(), compact_hot, &font, &metrics, cell, surface, radius, stroke, scale)?;
            self.lbterm_context_bar.last_pane = Some(pane);
        }
        // A wait that's up, and a slide that's done when something was going: a card sliding back,
        // drawn from its last frame until it's in (the bar narrows once the pane's in), or the
        // scroll strip riding home into its own place. One coming out is DWM's to finish, and its
        // end changes nothing drawn: a frame for it would draw the same again (two a pane's
        // opening, which the budget runner's context-bar-pane-opening found; lbterm/PITFALLS.md,
        // 219)
        let going = (showing[0] && !pane_open)
            || (showing[1] && !held)
            || (showing[3] && !held)
            || (showing[2] && self.lbterm_context_bar.ride.offset == 0.0);
        if going {
            self.lbterm_context_bar_wake(settles);
        }
        let due = [self.lbterm_context_bar.widen.deadline(), self.lbterm_context_bar.pane.deadline()]
            .iter()
            .flatten()
            .min()
            .copied();
        self.lbterm_context_bar_wake(due);
        Ok(())
    }

    /// The scroll strip's ride this frame, for render/scroll_strip.rs, which paints after the bar
    pub(crate) fn lbterm_context_bar_ride(&self) -> Ride {
        self.lbterm_context_bar.ride
    }

    /// The scroll strip drew itself riding in its own visual, at `bounds`; its own place is `home`
    pub(crate) fn lbterm_context_bar_rode(&mut self, rode: Option<(PixelRect, i32)>) {
        self.lbterm_context_bar.rode = rode;
    }

    /// Where the pane's, the widened bar's, the thin numbers' pane's and the riding scroll strip's
    /// visuals go this frame, for the compositor (compositor.rs, `prepare_context_bar`); the ride
    /// with the window as its clip and where it showed from
    pub(crate) fn lbterm_context_bar_visuals(&self) -> (Visuals, Option<(PixelRect, PixelRect, i32)>) {
        let state = &self.lbterm_context_bar;
        let window = PixelRect::new(0, 0, self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        let ride = state.rode.filter(|_| state.ride.own).map(|(bounds, home)| (bounds, window, home));
        (state.visuals, ride)
    }

    /// How wide `text` lays out in `font`, as compute_element lays out the words drawn: the
    /// frame's font needn't be monospaced (lbterm/PITFALLS.md, 76)
    pub(crate) fn lbterm_context_width(&self, font: &Rc<wezterm_font::LoadedFont>, metrics: &RenderMetrics, text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let Some(gl_state) = self.render_state.as_ref() else {
            return 0.0;
        };
        let cell = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        let bounds = euclid::rect(0.0, 0.0, self.dimensions.pixel_width as f32, metrics.cell_size.height as f32);
        let element = crate::termwindow::box_model::Element::new(font, crate::termwindow::box_model::ElementContent::Text(text.to_string()));
        self.compute_element(&self.lbterm_words_context(gl_state, metrics, cell, bounds, PANE_Z[2]), &element)
            .map_or(0.0, |c| c.content_rect.width() as f64)
    }

    /// A card in `z`'s shadow and card layers: its shadow, its ring (`ring`, else white
    /// `RING_ALPHA` over the panes' colour) and its fill
    #[allow(clippy::too_many_arguments)]
    fn lbterm_context_card(&self, z: [i8; 3], rect: Rect, ring: Option<LinearRgba>, surface: SrgbaTuple, radius: f64, stroke: f64, scale: f64) -> anyhow::Result<()> {
        let gl_state = self.render_state.as_ref().unwrap();
        let grow = |r: Rect, by: f64, down: f64| Rect { x: r.x - by, y: r.y - by + down, w: r.w + 2.0 * by, h: r.h + 2.0 * by };
        {
            let layer = gl_state.layer_for_zindex(z[0])?;
            let mut quads = layer.quad_allocator();
            let (reach, down) = strip_card::shadow_reach(scale);
            for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                self.lbterm_menu_rounded(&mut quads, grow(rect, by, down), radius + by, LinearRgba::with_components(0., 0., 0., *alpha))?;
            }
        }
        let layer = gl_state.layer_for_zindex(z[1])?;
        let mut quads = layer.quad_allocator();
        self.lbterm_menu_rounded(&mut quads, rect, radius, ring.unwrap_or_else(|| over(surface, RING_ALPHA)))?;
        let inner = grow(rect, -stroke, 0.0);
        self.lbterm_menu_rounded(&mut quads, inner, (radius - stroke).max(0.0), surface.to_linear())?;
        Ok(())
    }

    /// The widened bar: its card ringed as the bar is (`ring`, the bar's corners), its track, the
    /// sections' runs and hatching as laid out for it, and each number on its section, dark on the
    /// light colours
    #[allow(clippy::too_many_arguments)]
    fn lbterm_context_draw_drawer(
        &self,
        drawer: &Drawer,
        ring: LinearRgba,
        font: &Rc<wezterm_font::LoadedFont>,
        metrics: &RenderMetrics,
        cell: (f64, f64),
        surface: SrgbaTuple,
        radius: f64,
        stroke: f64,
        scale: f64,
    ) -> anyhow::Result<()> {
        // The bar's own corners, so its ring meets the bar's right ring line, which shows through
        // the widened bar's clip
        let radius = radius.min((drawer.rect.w.min(drawer.rect.h)) / 2.0).min(self.lbterm_context_bar.rect.map_or(radius, |b| b.w / 2.0));
        self.lbterm_context_card(DRAWER_Z, drawer.rect, Some(ring), surface, radius, stroke, scale)?;
        {
            let gl_state = self.render_state.as_ref().unwrap();
            let layer = gl_state.layer_for_zindex(DRAWER_Z[1])?;
            let mut quads = layer.quad_allocator();
            self.lbterm_menu_rounded(&mut quads, drawer.track, 0.0, srgb(palette::TRACK))?;
            for (at, colour) in &drawer.shapes {
                self.lbterm_menu_rounded(&mut quads, *at, 0.0, srgb(*colour))?;
            }
        }
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        let (dark, light) = (surface.to_linear(), over(surface, TEXT_ALPHA));
        let right = drawer.track.x + drawer.track.w;
        self.lbterm_draw_words(
            &self.lbterm_context_bar.words[0],
            font,
            metrics,
            room,
            DRAWER_Z[2],
            drawer.labels.iter().map(|(_, at, text, on_light)| {
                // Its rect to the track's right edge: the words stay on the bar (a glyph a hair
                // wider than the font's cell is cut there, never past it)
                let colour = if *on_light { dark } else { light };
                (text.clone(), euclid::rect(at.x as f32, at.y as f32, (right - at.x).max(0.0) as f32, cell.1 as f32), colour)
            }),
        )
    }

    /// The pane of thin sections' numbers beside the widened bar: its card, the hairlines from moved
    /// numbers to their sections, and the numbers
    #[allow(clippy::too_many_arguments)]
    fn lbterm_context_draw_thin(
        &self,
        thin: &Thin,
        font: &Rc<wezterm_font::LoadedFont>,
        metrics: &RenderMetrics,
        cell: (f64, f64),
        surface: SrgbaTuple,
        radius: f64,
        stroke: f64,
        scale: f64,
    ) -> anyhow::Result<()> {
        self.lbterm_context_card(THIN_Z, thin.rect, None, surface, radius, stroke, scale)?;
        {
            let gl_state = self.render_state.as_ref().unwrap();
            let layer = gl_state.layer_for_zindex(THIN_Z[1])?;
            let mut quads = layer.quad_allocator();
            for leader in &thin.leaders {
                self.lbterm_menu_rounded(&mut quads, *leader, 0.0, over(surface, LEADER_ALPHA))?;
            }
        }
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        let colour = over(surface, TEXT_ALPHA);
        self.lbterm_draw_words(
            &self.lbterm_context_bar.words[2],
            font,
            metrics,
            room,
            THIN_Z[2],
            thin.labels.iter().map(|(_, at, text)| {
                // A cell to spare past its end: a glyph a hair wider than the font's cell would be
                // cut off at the rect's edge (the label's own end is where it was placed)
                (text.clone(), euclid::rect(at.x as f32, at.y as f32, (at.w + cell.0) as f32, cell.1 as f32), colour)
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn lbterm_context_draw_pane(
        &self,
        pane: &PaneCard,
        lit: Option<usize>,
        hot: Option<usize>,
        compact: Option<&Result<(), String>>,
        compact_hot: bool,
        font: &Rc<wezterm_font::LoadedFont>,
        metrics: &RenderMetrics,
        cell: (f64, f64),
        surface: SrgbaTuple,
        radius: f64,
        stroke: f64,
        scale: f64,
    ) -> anyhow::Result<()> {
        self.lbterm_context_card(PANE_Z, pane.rect, None, surface, radius, stroke, scale)?;
        {
            let gl_state = self.render_state.as_ref().unwrap();
            let layer = gl_state.layer_for_zindex(PANE_Z[1])?;
            let mut quads = layer.quad_allocator();
            for row in &pane.rows {
                if Some(row.section) == lit {
                    let inset = 4.0 * scale;
                    let band = Rect { x: row.rect.x + inset, y: row.rect.y + 1.0, w: row.rect.w - 2.0 * inset, h: row.rect.h - 2.0 };
                    self.lbterm_menu_rounded(&mut quads, band, radius / 2.0, over(surface, ROW_HOVER_ALPHA))?;
                }
                self.lbterm_menu_rounded(&mut quads, row.swatch, 2.0 * scale, srgb(row.colour))?;
                if let Some((at, _, _)) = &row.button {
                    let alpha = if Some(row.section) == hot { BUTTON_LIT } else { BUTTON_ALPHA };
                    self.lbterm_menu_rounded(&mut quads, *at, at.h / 2.0, over(surface, alpha))?;
                }
            }
            // The rule over the pinned totals
            if let Some(rule) = pane.rule {
                self.lbterm_menu_rounded(&mut quads, rule, 0.0, over(surface, RING_ALPHA))?;
            }
            if let (Some(at), Some(look)) = (&pane.compact, compact) {
                let alpha = match (look.is_ok(), compact_hot) {
                    (false, _) => BUTTON_DIM,
                    (true, true) => BUTTON_LIT,
                    (true, false) => BUTTON_ALPHA,
                };
                self.lbterm_menu_rounded(&mut quads, at.rect, at.rect.h / 2.0, over(surface, alpha))?;
            }
        }
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        let right = pane.rect.x + pane.rect.w;
        let word = |x: f64, y: f64, w: f64, text: &str, colour: LinearRgba| {
            (text.to_string(), euclid::rect(x as f32, y as f32, w.max(cell.0) as f32, cell.1 as f32), colour)
        };
        let mut words = vec![word(pane.heading.0, pane.heading.1, right - pane.heading.0, &pane.heading.2, over(surface, TITLE_ALPHA))];
        for row in &pane.rows {
            words.push(word(row.kind.0, row.text_y, right - row.kind.0, &row.kind.1, over(surface, TEXT_ALPHA)));
            if let Some((x, w, text)) = &row.path {
                words.push(word(*x, row.text_y, *w, text, over(surface, EVIDENCE_ALPHA)));
            }
            words.push(word(row.tokens.0, row.text_y, right - row.tokens.0, &row.tokens.1, over(surface, TEXT_ALPHA)));
            if let Some((at, text, inset)) = &row.button {
                words.push(word(at.x + inset, row.text_y, at.w - inset, text, over(surface, TEXT_ALPHA)));
            }
            if let Some((x, y, text)) = &row.note {
                words.push(word(*x, *y, right - *x, text, over(surface, EVIDENCE_ALPHA)));
            }
        }
        if let (Some(at), Some(look)) = (&pane.compact, compact) {
            let alpha = if look.is_ok() { TEXT_ALPHA } else { EVIDENCE_ALPHA };
            words.push(word(at.text.0, at.text.1, at.rect.w, COMPACT, over(surface, alpha)));
            if let Err(why) = look {
                words.push(word(at.why.0, at.why.1, right - at.why.0, why, over(surface, EVIDENCE_ALPHA)));
            }
        }
        let scrolled = &pane.rows[..pane.scrolled.min(pane.rows.len())];
        if pane.above > 0 {
            if let Some(first) = scrolled.first() {
                let y = first.rect.y - cell.1;
                words.push(word(first.kind.0, y, right - first.kind.0, &format!("{} above", pane.above), over(surface, EVIDENCE_ALPHA)));
            }
        }
        if pane.left_out > 0 {
            if let Some(last) = scrolled.last() {
                let y = last.rect.y + last.rect.h;
                let more = if pane.pinned { format!("{} more files", pane.left_out) } else { format!("{} more", pane.left_out) };
                words.push(word(last.kind.0, y, right - last.kind.0, &more, over(surface, EVIDENCE_ALPHA)));
            }
        }
        self.lbterm_draw_words(&self.lbterm_context_bar.words[1], font, metrics, room, PANE_Z[2], words)
    }

    /// Compact now's look at the tab's pane: whether it may type `/compact` there, or why not.
    /// From what the window holds: the pane's `claude_status`, and only when Claude Code isn't
    /// working, its screen's last rows as a voice draft's send reads them (control.rs's
    /// `live_screen`), looked at again only when the pane's seqno or its status has moved since
    /// (`fresh` looks again whatever: a press)
    fn lbterm_context_compact_look(&mut self, fresh: bool) -> Result<(), String> {
        let Some(pane) = self.lbterm_context_bar.pane_id.and_then(|id| Mux::get().get_pane(id)) else {
            return Err(NO_PANE.to_string());
        };
        if let Some((_, why)) = self.lbterm_context_bar.compact_failed.as_ref().filter(|(p, _)| *p == pane.pane_id()) {
            return Err(why.clone());
        }
        // Typed into this pane for this conversation, from any window, and not compacted yet
        if let Some(sid) = self.lbterm_context_bar.session.as_deref() {
            let (compactions, replies) = lbterm_context_compactions(sid);
            if crate::context_edits::compact_waits(pane.pane_id(), sid, compactions, replies) {
                return Err(SENT.to_string());
            }
        }
        let status = pane.copy_user_vars().remove("claude_status").unwrap_or_default();
        let key = (pane.pane_id(), pane.get_current_seqno(), status);
        if !fresh {
            if let Some((seen, look)) = &self.lbterm_context_bar.compact_seen {
                if *seen == key {
                    return look.clone();
                }
            }
        }
        let working = lbterm_panel::claude::Status::parse(&key.2).map_or(false, |s| s.colour == Some(lbterm_panel::claude::WORKING));
        let look = if working {
            context_edits::may_compact(true, &Default::default())
        } else {
            context_edits::may_compact(false, &crate::termwindow::control::live_screen(&pane))
        }
        .map_err(String::from);
        self.lbterm_context_bar.compact_seen = Some((key, look.clone()));
        look
    }

    /// Compact now pressed: `/compact` and Enter typed into the tab's pane at once, as a voice draft
    /// is sent (control.rs's `type_keys`, Enter its own write), if a fresh look says Claude Code is
    /// at its prompt with nothing typed; the pane says why when it isn't, or that it's sent. It
    /// writes nothing else anywhere, removed files or not: a compaction attaches a removed file
    /// again while it's among the five Claude Code restores, which the bar then shows as a section
    /// of its own (lbterm/CONTEXT.md, "Compact now")
    fn lbterm_context_compact_press(&mut self) {
        let look = self.lbterm_context_compact_look(true);
        let pane = self.lbterm_context_bar.pane_id.and_then(|id| Mux::get().get_pane(id));
        let sid = self.lbterm_context_bar.session.clone();
        if let (Ok(()), Some(pane), Some(sid)) = (&look, pane, sid) {
            let (compactions, replies) = lbterm_context_compactions(&sid);
            crate::context_edits::compact_sent(pane.pane_id(), &sid, compactions, replies);
            log::info!("context bar: Compact now typed /compact into pane {}", pane.pane_id());
            let keys = vec!["/compact".to_string(), lbterm_extras::drafts::ENTER.to_string()];
            crate::termwindow::control::type_keys(self.window.clone(), &pane, keys, |tw, local, why, maybe| {
                tw.lbterm_context_compact_failed(local, why, maybe)
            });
        }
        self.lbterm_context_bar.compact = Some(self.lbterm_context_compact_look(false));
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A key of Compact now's `/compact` didn't get to `pane`: its pane says so until it closes,
    /// and the scroll strip's note too, as a voice draft's failed send does
    fn lbterm_context_compact_failed(&mut self, pane: PaneId, why: String, maybe: bool) {
        crate::context_edits::compact_unsent(pane);
        self.lbterm_context_bar.compact_failed = Some((pane, format!("Not sent: {why}")));
        self.lbterm_scroll_write_failed(pane, format!("{why}; Compact now's /compact may be half typed: look at the box first"), maybe);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Repaint at `at`: a wait that's up, or a slide that's done
    pub(crate) fn lbterm_context_bar_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        if matches!(self.lbterm_context_bar.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_context_bar.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_context_bar.wake == Some(at) {
                    tw.lbterm_context_bar.wake = None;
                }
                let now = Instant::now();
                tw.lbterm_context_bar.widen.tick(now);
                tw.lbterm_context_bar.pane.tick(now);
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// The section at `y` on the bar's track: its run's, split among a joined run's sections by
    /// their tokens, or free space below the filled part. While the bar is widened, its own runs
    /// (laid out again with a line's floor) on it, and the label under the pointer on the pane of
    /// thin sections' numbers beside it
    fn lbterm_context_section_at(&self, x: f64, y: f64) -> Option<usize> {
        let state = &self.lbterm_context_bar;
        let track = state.track?;
        let (_, laid) = state.laid.as_ref()?;
        let bar = laid.bar.as_ref()?.as_ref().ok()?;
        if let Some(widened) = state.drawer.as_ref() {
            if let Some((section, _, _)) = widened.thin.as_ref().and_then(|t| t.labels.iter().find(|(_, at, _)| at.contains(x, y))) {
                return Some(*section);
            }
            if widened.contains(x, y) {
                return section_in(bar, &widened.runs, laid.filled, widened.track, y);
            }
        }
        section_in(bar, &laid.runs, laid.filled, track, y)
    }

    /// The pointer moved, pressed or released: on the bar or the gutter left of it, the bar takes
    /// it, and its ring lights while the pointer's there. True when it took the event: nothing
    /// under it gets it.
    pub(crate) fn lbterm_context_bar_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(bar) = self.lbterm_context_bar.rect else {
            return false;
        };
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut self.lbterm_context_bar.pressed) {
                return true;
            }
        }
        // A drag that began elsewhere carries on over the bar
        let busy = self.current_mouse_capture.is_some() || self.window_drag_position.is_some() || self.dragging.is_some();
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let gutter = self.lbterm_shape().gutter() as f64;
        let on_bar = x >= bar.x - gutter && y >= bar.y && y < bar.y + bar.h;
        let on_drawer = self.lbterm_context_bar.drawer.as_ref().map_or(false, |d| d.contains(x, y));
        let row = self
            .lbterm_context_bar
            .pane_card
            .as_ref()
            .filter(|p| p.rect.contains(x, y))
            .map(|p| p.rows.iter().find(|r| r.rect.contains(x, y)).map(|r| (r.section, r.button.map_or(false, |(b, _, _)| b.contains(x, y)))));
        let on_pane = row.is_some();
        let on_compact = self
            .lbterm_context_bar
            .pane_card
            .as_ref()
            .and_then(|p| p.compact.as_ref())
            .map_or(false, |c| c.rect.contains(x, y));
        let inside = !busy && (on_bar || on_drawer || on_pane);
        // The section under the pointer: the pane stays as it is while the pointer moves along the
        // bar, its row lit (Lachlan's 8a)
        let lit = match row {
            Some(row) => row.map(|(s, _)| s),
            None if inside => self.lbterm_context_section_at(x, y),
            None => None,
        };
        let now = Instant::now();
        let state = &mut self.lbterm_context_bar;
        let mut changed = inside != state.pointer_on;
        state.pointer_on = inside;
        changed |= state.widen.pointer(inside, now);
        // It opens after a rest on a section; once out it stays while the pointer's on any of them
        let on_section = if state.pane.is_open() { inside } else { inside && lit.is_some() };
        changed |= state.pane.pointer(on_section, now);
        let hot = match row {
            Some(Some((s, true))) if inside => Some(s),
            _ => None,
        };
        if hot != state.hot {
            state.hot = hot;
            changed = true;
        }
        let compact_hot = inside && on_compact;
        if compact_hot != state.compact_hot {
            state.compact_hot = compact_hot;
            changed = true;
        }
        if lit != state.lit {
            // Lit only on the drawer and the pane: kept at rest, so a pane opened by its rest
            // lights the right row
            changed |= state.drawer.is_some() || state.pane_card.is_some();
            state.lit = lit;
        }
        // A left press on a row's button: Remove, or Undo once its line is out; on Compact now,
        // `/compact` typed into the tab (CONTEXT.md, "The buttons")
        let mut pressed_button = None;
        let mut pressed_compact = false;
        if inside {
            if let WMEK::Press(press) = &event.kind {
                state.pressed = true;
                // A left press on the bar opens it all at once, as the projects pad does
                if matches!(press, MousePress::Left) && (on_bar || on_drawer) {
                    changed |= state.widen.open_now(now);
                    changed |= state.pane.open_now(now);
                }
                if matches!(press, MousePress::Left) {
                    if let Some(Some((section, true))) = row {
                        pressed_button = Some(section);
                    }
                    pressed_compact = on_compact;
                }
            }
            // The wheel scrolls the pane's rows while it's out and they don't all fit, three a
            // notch as the lists' do, never past the last page
            if let (WMEK::VertWheel(n), Some(pane)) = (&event.kind, state.pane_card.as_ref()) {
                let to = if *n > 0 { state.scroll.saturating_sub(3) } else { (state.scroll + 3).min(pane.most) };
                if to != state.scroll {
                    state.scroll = to;
                    state.wheel_at = Some((x, y));
                    changed = true;
                }
            }
        }
        if let Some(section) = pressed_button {
            // A row past the sections is a removal whose section has gone (Laid::gone)
            let pressed = state.pane_id.map(crate::context_edits::states).unwrap_or_default();
            let removal = state.laid.as_ref().and_then(|(_, l)| {
                let sections = l.bar.as_ref().and_then(|b| b.as_ref().ok()).map(|b| &b.sections)?;
                match sections.get(section) {
                    Some(s) => context_edits::section_removal(s, &pressed).cloned(),
                    None => l.gone.get(section - sections.len()).map(|(r, _)| r.clone()),
                }
            });
            if let (Some(pane), Some(removal)) = (state.pane_id, removal) {
                crate::context_edits::press(pane, &removal);
            }
        }
        if pressed_compact {
            self.lbterm_context_compact_press();
        }
        let state = &mut self.lbterm_context_bar;
        if changed {
            context.lbterm_invalidate();
        }
        let due = [state.widen.deadline(), state.pane.deadline()].iter().flatten().min().copied();
        self.lbterm_context_bar_wake(due);
        if inside {
            let on_button = matches!(row, Some(Some((_, true)))) || on_compact;
            context.set_cursor(Some(if on_button { CursorIcon::Pointer } else { CursorIcon::Default }));
        }
        inside
    }

    /// The pointer left the window: the bar's ring goes back to rest, and the drawer and the pane
    /// close as if it had left them
    pub(crate) fn lbterm_context_bar_mouse_leave(&mut self) {
        let now = Instant::now();
        let state = &mut self.lbterm_context_bar;
        let mut changed = std::mem::take(&mut state.pointer_on);
        changed |= state.widen.left_window(now);
        changed |= state.pane.left_window(now);
        changed |= state.lit.take().is_some();
        changed |= state.hot.take().is_some();
        changed |= std::mem::take(&mut state.compact_hot);
        let due = [state.widen.deadline(), state.pane.deadline()].iter().flatten().min().copied();
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        self.lbterm_context_bar_wake(due);
    }

    /// The waits under way at `now`: the bar's rest before it widens, the pane's before it opens,
    /// and their grace before they close
    pub(crate) fn lbterm_context_bar_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        use lbterm_panel::Phase;
        let state = &self.lbterm_context_bar;
        let mut waits = vec![];
        for (hover, opening) in [(&state.widen, "context.widen"), (&state.pane, "context.open")] {
            let name = match hover.phase() {
                Phase::Opening => opening,
                Phase::Closing => "context.close",
                _ => continue,
            };
            if let Some(due) = hover.deadline() {
                if !waits.iter().any(|w: &lbterm_extras::state::Wait| w.name == name) {
                    waits.push(lbterm_extras::state::Wait {
                        name: name.to_string(),
                        left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
                    });
                }
            }
        }
        waits
    }

    /// The bar's conversation, while it shows one: crate::summaries repaints a window showing a
    /// session whose `usage.json` alone changed only when this, the pill's or an open rail row's
    /// is one of them (render/context.rs, `lbterm_shows_usage`), for the window's size
    pub(crate) fn lbterm_context_bar_session(&self) -> Option<&str> {
        self.lbterm_context_bar.session.as_deref()
    }

    /// What the state file's `pointer.over` names on the bar
    pub(crate) fn lbterm_context_bar_under(&self, x: f64, y: f64) -> Option<&'static str> {
        let state = &self.lbterm_context_bar;
        if let Some(pane) = state.pane_card.as_ref().filter(|p| p.rect.contains(x, y)) {
            let on_button = pane.rows.iter().any(|r| r.button.map_or(false, |(b, _, _)| b.contains(x, y)));
            let on_compact = pane.compact.as_ref().map_or(false, |c| c.rect.contains(x, y));
            return Some(if on_button {
                "context.button"
            } else if on_compact {
                "context.compact"
            } else {
                "context.pane"
            });
        }
        let bar = state.rect?;
        let gutter = self.lbterm_shape().gutter() as f64;
        let on_drawer = state.drawer.as_ref().map_or(false, |d| d.contains(x, y));
        (on_drawer || (x >= bar.x - gutter && y >= bar.y && y < bar.y + bar.h)).then_some("context.bar")
    }

    /// The bar for the state file (lbterm/STATE.md, `context_bar`)
    pub(crate) fn lbterm_state_context_bar(&self) -> Option<lbterm_extras::state::ContextBar> {
        use lbterm_extras::state as S;
        let at = |r: &Rect| S::Rect::round(r.x, r.y, r.w, r.h);
        let state = &self.lbterm_context_bar;
        let (bar, track) = (state.rect?, state.track?);
        let laid = state.laid.as_ref().map(|(_, l)| l);
        let made = laid.and_then(|l| l.bar.as_ref());
        let waiting = match (&state.session, made) {
            (None, _) => None,
            (Some(_), None) => Some(READING.to_string()),
            (Some(_), Some(Err(w))) => Some(w.words().to_string()),
            (Some(_), Some(Ok(_))) => None,
        };
        let shown = made.and_then(|b| b.as_ref().ok());
        let runs = laid.map(|l| l.runs.as_slice()).unwrap_or_default();
        let sections = shown
            .map(|b| {
                b.sections
                    .iter()
                    .enumerate()
                    .map(|(i, s)| S::ContextSection {
                        kind: s.what.name().to_string(),
                        path: s.path.clone(),
                        tokens: s.tokens,
                        rect: runs
                            .iter()
                            .find(|r| (r.first..=r.last).contains(&i))
                            .map(|r| at(&run_rect(track, r))),
                        colour: format!("#{:06X}", s.what.colour()),
                        button: (s.removal.is_some() || laid.and_then(|l| l.looks.get(i)).map_or(false, |l| l.button.is_some()))
                            .then(|| "remove".to_string()),
                        removed: laid.and_then(|l| l.looks.get(i)).map_or(false, |l| l.removed),
                        note: laid.and_then(|l| l.looks.get(i)).and_then(|l| l.note.clone()).or_else(|| s.note.map(String::from)),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(S::ContextBar {
            rect: at(&bar),
            track: at(&track),
            session: state.session.clone(),
            waiting,
            in_use: shown.map(|b| b.in_use),
            window: shown.map(|b| b.window),
            estimate: shown.map_or(false, |b| b.estimate),
            scale: laid.and_then(|l| l.scale),
            filled: laid.map_or(0, |l| l.filled),
            sections,
            layouts: state.layouts,
            widened: state.drawer.as_ref().or(state.last_drawer.as_ref().filter(|_| state.visuals.bar.is_some())).map(|d| {
                let thin = d.thin.as_ref().filter(|_| state.visuals.thin.is_some());
                let mut labels: Vec<S::ContextLabel> = d
                    .labels
                    .iter()
                    .map(|(section, rect, text, _)| S::ContextLabel { section: *section, text: text.clone(), rect: at(rect) })
                    .chain(thin.into_iter().flat_map(|t| {
                        t.labels.iter().map(|(section, rect, text)| S::ContextLabel { section: *section, text: text.clone(), rect: at(rect) })
                    }))
                    .collect();
                labels.sort_by_key(|l| l.section);
                S::ContextDrawer { rect: at(&d.rect), labels, thin: thin.map(|t| at(&t.rect)) }
            }),
            pane: state.pane_card.as_ref().or(state.last_pane.as_ref().filter(|_| state.visuals.pane.is_some())).map(|p| S::ContextPane {
                rect: at(&p.rect),
                heading: p.heading.2.clone(),
                rows: p
                    .rows
                    .iter()
                    .map(|r| S::ContextRow {
                        section: r.section,
                        kind: r.kind.1.clone(),
                        path: r.path.as_ref().map(|(_, _, t)| t.clone()),
                        tokens: r.tokens.1.clone(),
                        button: r.button.map(|(_, w, _)| w.to_string()),
                        note: r.note.as_ref().map(|(_, _, t)| t.clone()),
                        rect: at(&r.rect),
                        lit: state.pane_card.is_some() && state.lit == Some(r.section),
                    })
                    .collect(),
                above: p.above,
                left_out: p.left_out,
                pinned: p.pinned,
                compact: p.compact.as_ref().map(|c| S::ContextCompact {
                    rect: at(&c.rect),
                    enabled: matches!(state.compact, Some(Ok(()))),
                    why: state.compact.as_ref().and_then(|l| l.as_ref().err()).cloned(),
                }),
            }),
            lit: state.lit.filter(|_| state.drawer.is_some() || state.pane_card.is_some()),
            ride: state.ride.offset as i64,
        })
    }
}

/// Where a run is drawn in `track`, its gap included
/// How many compactions and replies session `sid`'s transcript has recorded, as the reader has
/// read it: Compact now's `/compact` waits until either has moved (crate::context_edits)
fn lbterm_context_compactions(sid: &str) -> (u32, u32) {
    crate::conversations::get(sid).map_or((0, 0), |s| (s.context.compactions, s.context.replies))
}

fn run_rect(track: Rect, run: &Run) -> Rect {
    Rect { x: track.x, y: track.y + run.y as f64, w: track.w, h: run.h as f64 }
}

/// The section at `y` on a track laid out into `runs`: its run's, split among a joined run's
/// sections by their tokens, or free space below the filled part
fn section_in(bar: &Bar, runs: &[Run], filled: u32, track: Rect, y: f64) -> Option<usize> {
    if y < track.y || y >= track.y + track.h {
        return None;
    }
    let down = y - track.y;
    if down >= filled as f64 {
        return bar.sections.iter().position(|s| s.what == context_bar::What::Free && s.tokens > 0);
    }
    let run = runs.iter().find(|r| down >= r.y as f64 && down < (r.y + r.h) as f64)?;
    let total: u64 = bar.sections[run.first..=run.last].iter().map(|s| s.tokens).sum();
    let mut at = (down - run.y as f64) / run.h.max(1) as f64 * total as f64;
    for i in run.first..=run.last {
        let t = bar.sections[i].tokens as f64;
        if at < t {
            return Some(i);
        }
        at -= t;
    }
    Some(run.last)
}

/// A removed section's share of each run it's in, hatched with the track's colour, a line every
/// other pixel: its line is out of the file, and it leaves at the next compaction
fn hatches(bar: &Bar, runs: &[Run], looks: &[Look], track: Rect) -> Vec<Rect> {
    let mut lines = vec![];
    for r in runs {
        let total: u64 = bar.sections[r.first..=r.last].iter().map(|s| s.tokens).sum::<u64>().max(1);
        let mut y = (track.y + r.y as f64).round();
        for i in r.first..=r.last {
            let h = r.coloured as f64 * bar.sections[i].tokens as f64 / total as f64;
            if looks.get(i).map_or(false, |l| l.removed) {
                let mut line = y.floor() + 1.0;
                while line < y + h {
                    lines.push(Rect { x: track.x, y: line, w: track.w, h: 1.0 });
                    line += 2.0;
                }
            }
            y += h;
        }
    }
    lines
}

/// The sections for what they're made from, laid out into runs in `track`, and their shapes: each
/// run its colour down to its gap, the track showing through the gap and below the filled part
fn lay_out(from: &LaidFrom, session: bool, track: Rect, edits: &[(context_bar::Removal, EditState)]) -> Laid {
    let bar = match (&from.context, session) {
        (Some(context), true) => Some(context_bar::sections(context, from.window)),
        _ => None,
    };
    let scale = from
        .context
        .as_ref()
        .and_then(|c| c.scale)
        .filter(|s| s.estimated > 0)
        .map(|s| s.real as f64 / s.estimated as f64);
    let (runs, filled) = match &bar {
        Some(Ok(bar)) => {
            let height = from.track[3].max(0) as u32;
            (context_bar::runs(bar, height, from.floor, from.gap), context_bar::filled(bar, height))
        }
        _ => (vec![], 0),
    };
    let looks = match &bar {
        Some(Ok(bar)) => looks(bar, edits),
        _ => vec![],
    };
    let gone = match &bar {
        Some(Ok(bar)) => context_edits::off_the_bar(bar, edits)
            .into_iter()
            .map(|(removal, state)| {
                let look = Look { button: Some(state.button()), note: Some(state.gone_note(removal)), removed: state.removed() };
                (removal.clone(), look)
            })
            .collect(),
        _ => vec![],
    };
    let mut shapes: Vec<sides::Shape> = runs
        .iter()
        .map(|r| {
            let at = Rect { h: r.coloured as f64, ..run_rect(track, r) };
            sides::Shape::new(at, 0.0, srgb(r.colour))
        })
        .collect();
    // A removed section's share of its run is hatched with the track's colour
    if let Some(Ok(bar)) = &bar {
        shapes.extend(hatches(bar, &runs, &looks, track).into_iter().map(|at| sides::Shape::new(at, 0.0, srgb(palette::TRACK))));
    }
    Laid { bar, runs, filled, scale, shapes, looks, gone, floor: (from.floor, from.gap) }
}


/// Whether words on `colour` are drawn dark (the panes' colour) rather than light: on the light
/// sections (the greys of the files, yellow, amber, the conversation's blue-grey), not on Claude
/// Code's parts' darker grey or the track
fn dark_on(colour: u32) -> bool {
    let [r, g, b] = [16, 8, 0].map(|s| ((colour >> s) & 0xFF) as f64 / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b > 0.45
}

/// The widened bar (Lachlan's 4b of 2026-11-09): the bar itself at the closed rail's width
/// (`rail`), its right edge the bar's, over the terminal; its track inset as the bar's is, the
/// sections laid out again for it with a line and its gap as their floor (`context_bar::runs`), so
/// the fill line stays where what's in use puts it and most sections have a line's room, and each
/// one's tokens written on it, from its left, by their laid-out widths (`measure`; the frame's
/// font needn't be monospaced, lbterm/PITFALLS.md 76; `context_bar::numbers`). The numbers of
/// sections too thin for theirs go in a pane as wide as the bar, left of it (`Thin`), right-aligned
/// against it, level with their sections, pushed apart with a hairline from a moved one to its
/// section (Lachlan's rule for panes: never a floating card)
#[allow(clippy::too_many_arguments)]
fn lay_out_drawer(
    laid: Option<&Laid>,
    bar: Rect,
    track: Rect,
    rail: f64,
    stroke: f64,
    line: f64,
    scale: f64,
    measure: &dyn Fn(&str) -> f64,
) -> Drawer {
    let x = (bar.x + bar.w - rail).round().min(bar.x - 2.0 * line);
    let rect = Rect { x, y: bar.y, w: bar.x + bar.w - x, h: bar.h };
    let inset = track.x - bar.x;
    let wide = Rect { x: rect.x + inset, y: track.y, w: (rect.w - 2.0 * inset).max(1.0), h: track.h };
    let mut drawer = Drawer { rect, track: wide, runs: vec![], shapes: vec![], labels: vec![], thin: None };
    let Some((laid, Some(Ok(sections)))) = laid.map(|l| (l, l.bar.as_ref())) else {
        return drawer;
    };
    let px = line.round().max(1.0) as u32;
    let height = wide.h.max(0.0) as u32;
    let gap = laid.floor.1;
    let runs = context_bar::runs(sections, height, px + gap, gap);
    for r in &runs {
        drawer.shapes.push((Rect { h: r.coloured as f64, ..run_rect(wide, r) }, r.colour));
    }
    drawer.shapes.extend(hatches(sections, &runs, &laid.looks, wide).into_iter().map(|at| (at, palette::TRACK)));
    let numbers = context_bar::numbers(sections, &runs, height, px);
    let pad = (4.0 * scale).round();
    for l in numbers.on {
        let colour = runs.iter().find(|r| (r.first..=r.last).contains(&l.section)).map(|r| r.colour);
        let w = measure(&l.text);
        let at = Rect { x: wide.x + pad, y: wide.y + l.y as f64, w: w.min(wide.w - pad), h: line };
        drawer.labels.push((l.section, at, l.text, colour.map_or(false, dark_on)));
    }
    if !numbers.thin.is_empty() {
        // As wide as the bar, its right ring on the bar's left ring
        let at = Rect { x: rect.x + stroke - rect.w, y: rect.y, w: rect.w, h: rect.h };
        let right = at.x + at.w - stroke - (6.0 * scale).round();
        let edge = (right + (2.0 * scale).round()).round();
        let mut thin = Thin { rect: at, labels: vec![], leaders: vec![] };
        for l in numbers.thin {
            let w = measure(&l.text);
            let place = Rect { x: (right - w).round().max(at.x + stroke), y: wide.y + l.y as f64, w, h: line };
            if l.moved(px) {
                let (from, to) = (place.y + line / 2.0, wide.y + l.to as f64);
                let (top, bottom) = (from.min(to).round(), from.max(to).round());
                thin.leaders.push(Rect { x: edge, y: top, w: stroke, h: (bottom - top).max(stroke) });
                thin.leaders.push(Rect { x: edge, y: to.round(), w: at.x + at.w - edge, h: stroke });
            }
            thin.labels.push((l.section, place, l.text));
        }
        drawer.thin = Some(thin);
    }
    drawer.runs = runs;
    drawer
}

/// `text` cut from its start to fit `room` pixels, keeping its end (a file's name and the folders
/// nearest it say the most), an ellipsis where it was cut: the longest end that fits, found by
/// halving, a handful of measures
fn fit_end(text: &str, room: f64, measure: &dyn Fn(&str) -> f64) -> String {
    if room <= 0.0 {
        return String::new();
    }
    if measure(text) <= room {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |k: usize| format!("…{}", chars[chars.len() - k..].iter().collect::<String>());
    let (mut fits, mut over) = (0, chars.len());
    while over - fits > 1 {
        let mid = (fits + over) / 2;
        if measure(&cut(mid)) <= room {
            fits = mid;
        } else {
            over = mid;
        }
    }
    if fits == 0 {
        String::new()
    } else {
        cut(fits)
    }
}

/// The pane: left of the drawer, `want` wide or what the room leaves, as tall as the bar, its
/// right ring on the drawer's left. A heading, then a row for every section in the bar's order: its
/// colour's swatch, its kind, its path with the home folder as `~` cut from its start, its tokens
/// and its button, with a dimmer second line where it has something to say; rows that don't fit
/// are counted under the last. Words are placed by their laid-out widths (`measure`, PITFALLS 76).
#[allow(clippy::too_many_arguments)]
fn lay_out_pane(
    laid: Option<&Laid>,
    session: bool,
    drawer: &Drawer,
    left: f64,
    want: f64,
    stroke: f64,
    line: f64,
    scale: f64,
    home: &str,
    scroll: usize,
    measure: &dyn Fn(&str) -> f64,
) -> PaneCard {
    let right = drawer.rect.x + stroke;
    let w = want.min(right - left).max(line * 12.0);
    let x = (right - w).round();
    let rect = Rect { x, y: drawer.rect.y, w: right - x, h: drawer.rect.h };
    let pad = (10.0 * scale).round();
    let gap = (8.0 * scale).round();
    let row_h = (line * 1.4).round();
    let inner = (rect.x + pad, rect.x + rect.w - pad);
    let made = laid.and_then(|l| l.bar.as_ref());
    let heading = match (session, made) {
        (false, _) => "No Claude Code conversation in this tab".to_string(),
        (true, None) => READING.to_string(),
        (true, Some(Err(w))) => w.words().to_string(),
        (true, Some(Ok(bar))) => context_bar::heading(bar),
    };
    let mut card = PaneCard {
        rect,
        heading: (inner.0, rect.y + pad, heading),
        rows: vec![],
        above: 0,
        most: 0,
        left_out: 0,
        pinned: false,
        scrolled: 0,
        rule: None,
        compact: None,
    };
    let (Some(Ok(bar)), Some(laid)) = (made, laid) else {
        return card;
    };
    let d = (line * 0.55).round();
    let kind_x = inner.0 + d + (6.0 * scale).round();
    // The rows in the bar's order, a removal whose section has gone (laid.gone) after the files,
    // where its file was, numbered on from the sections
    let len = bar.sections.len();
    let files = bar.sections.iter().position(|s| s.path.is_none()).unwrap_or(len);
    let order: Vec<usize> = (0..files).chain(len..len + laid.gone.len()).chain(files..len).collect();
    let what_of = |i: usize| if i < len { bar.sections[i].what } else { laid.gone[i - len].0.what() };
    let look_of = |i: usize| if i < len { laid.looks.get(i).cloned().unwrap_or_default() } else { laid.gone[i - len].1.clone() };
    // The kinds' column as wide as its widest word, each word measured once
    let mut kinds_seen: Vec<&'static str> = order.iter().map(|&i| what_of(i).word()).collect();
    kinds_seen.sort_unstable();
    kinds_seen.dedup();
    let kinds = kinds_seen.iter().map(|w| measure(w)).fold(0.0, f64::max);
    let path_x = (kind_x + kinds + gap).round();
    let words = ["Remove", "Undo"].map(|w| (w, measure(w)));
    let button_w = (words.iter().map(|(_, w)| *w).fold(0.0, f64::max) + 2.0 * gap).round();
    let button_x = inner.1 - button_w;
    let inset = (3.0 * scale).round();
    // Compact now at the foot, a line under it for why it can't be pressed; the rows stop above it
    let foot = rect.y + rect.h - pad - row_h - line;
    let compact_w = (measure(COMPACT) + 2.0 * gap).round();
    card.compact = Some(CompactAt {
        rect: Rect { x: inner.0, y: foot + inset, w: compact_w, h: row_h - 2.0 * inset },
        text: ((inner.0 + gap).round(), (foot + (row_h - line) / 2.0).round()),
        why: (inner.0, (foot + row_h).round()),
    });
    let bottom = foot - (4.0 * scale).round();
    let top = rect.y + pad + row_h;
    let heights: Vec<f64> = order.iter().map(|&i| if look_of(i).note.is_some() { row_h + line } else { row_h }).collect();
    let row_at = |i: usize, y: f64, h: f64| {
        let look = look_of(i);
        // A removal off the bar has no tokens: its section went with them
        let (section, path) = match bar.sections.get(i) {
            Some(s) => (Some(s), s.path.as_deref()),
            None => (None, Some(laid.gone[i - len].0.file())),
        };
        let tokens = section.map(|s| context_bar::row_tokens(bar, s)).unwrap_or_default();
        let tokens_x = (button_x - gap - if tokens.is_empty() { 0.0 } else { measure(&tokens) }).round();
        let text_y = (y + (row_h - line) / 2.0).round();
        let path = path.map(|p| {
            let room = tokens_x - gap - path_x;
            (path_x, room.max(0.0), fit_end(&ctx::short_path(p, home), room, measure))
        });
        let button = look.button.map(|word| (Rect { x: button_x, y: y + inset, w: button_w, h: row_h - 2.0 * inset }, word));
        PaneRow {
            section: i,
            rect: Rect { x: rect.x + stroke, y, w: rect.w - 2.0 * stroke, h },
            swatch: Rect { x: inner.0, y: (y + (row_h - d) / 2.0).round(), w: d, h: d },
            colour: what_of(i).colour(),
            text_y,
            kind: (kind_x, what_of(i).word().to_string()),
            path,
            tokens: (tokens_x, tokens),
            button: button.map(|(r, w)| {
                let width = words.iter().find(|(word, _)| *word == w).map_or(0.0, |(_, width)| *width);
                (r, w, ((r.w - width) / 2.0).round())
            }),
            note: look.note.map(|n| (kind_x, text_y + row_h - (row_h - line) / 2.0, n)),
        }
    };
    // When the rows don't all fit, the totals (Claude Code's parts, the conversation, free) are
    // pinned at the foot under a rule and only the files scroll (Lachlan's 3a of 2026-11-09): the
    // files and the removals off the bar come first in `order`, the totals after them
    let scrolled = files + laid.gone.len();
    let totals: f64 = heights[scrolled..].iter().sum();
    let rule_gap = (6.0 * scale).round();
    let pin = heights.iter().sum::<f64>() > bottom - top && scrolled > 0 && totals + 2.0 * rule_gap + line < bottom - top;
    let mut pinned = vec![];
    let room_bottom = if pin {
        let mut y = bottom - totals;
        card.rule = Some(Rect { x: inner.0, y: (y - rule_gap).round(), w: inner.1 - inner.0, h: stroke });
        for (at, &i) in order.iter().enumerate().skip(scrolled) {
            pinned.push(row_at(i, y, heights[at]));
            y += heights[at];
        }
        bottom - totals - 2.0 * rule_gap
    } else {
        bottom
    };
    card.pinned = pin;
    let (list, heights) = if pin { (&order[..scrolled], &heights[..scrolled]) } else { (&order[..], &heights[..]) };
    // The wheel's scroll, as far as fills the room (context_bar::first_shown); rows scrolled out
    // above are counted on a line over the first shown
    let mut y = top;
    card.above = context_bar::first_shown(heights, room_bottom - y, line, scroll);
    card.most = context_bar::first_shown(heights, room_bottom - y, line, usize::MAX);
    if card.above > 0 {
        y += line;
    }
    for (at, &i) in list.iter().enumerate().skip(card.above) {
        let h = heights[at];
        if y + h > room_bottom {
            card.left_out = list.len() - at;
            // The count of what's left out goes under the last row shown
            if y + line > room_bottom && card.rows.pop().is_some() {
                card.left_out += 1;
            }
            break;
        }
        card.rows.push(row_at(i, y, h));
        y += h;
    }
    card.scrolled = card.rows.len();
    card.rows.extend(pinned);
    card
}

#[cfg(test)]
mod test {
    use crate::termwindow::render::*;

    /// A z-layer goes to its visual by its number alone (lbterm/PITFALLS.md, 207): each of the
    /// bar's is routed to the bar and claimed by no other owner (render/composited.rs's
    /// `every_z_layer_has_one_owner_at_most` walks every number for every owner)
    #[test]
    fn the_context_bars_z_layers_are_its_own() {
        let ours = super::PANE_Z.iter().chain([super::RIDE_Z].iter()).chain(super::DRAWER_Z.iter()).chain(super::THIN_Z.iter()).copied();
        for z in ours {
            assert!(super::target_of(z).is_some(), "{} isn't routed to the bar", z);
            assert_eq!(composited::owners_of(z), vec!["the context bar"], "z-layer {}'s owners", z);
        }
    }
}
