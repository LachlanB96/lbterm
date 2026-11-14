//! LBTerm: the scroll strip down the right of the window, left of the objectives panel
//! (lbterm/SCROLL-STRIP.md). Lachlan's ask, 2026-10-28: the scrollbar of the conversation in the
//! open tab, with a mark where each of his prompts is and where Claude did something worth
//! finding again, a click on a mark taking him there; it looks like the objectives strip beside
//! it.
//!
//! It's the active pane's scrollbar. At a shell it's the scrollback's: the thumb is the
//! viewport, a coral bar marks each prompt (OSC 133, render/marks.rs), and a click or a drag
//! scrolls the viewport. In Claude Code's fullscreen view (the pane's `claude_session` user var,
//! on the alternate screen) it's the conversation's, read from the session's transcript
//! (crate::conversations, lbterm_extras::conversation): each block Claude Code draws takes rows
//! at the pane's width, each mark sits at its block's place, and the thumb is where the screen
//! says Claude Code is scrolled to, found from the messages and words on it. A click there scrolls
//! Claude Code with the keys it knows, reading the screen after each (`seek`), and marks the row
//! it lands on, as the strip's prompt menu does (render/prompt.rs). With it on, the panes' own
//! scrollbar goes (`panes_keep_scroll_bar`).
//!
//! What it costs (lbterm/AT-REST.md, "The scroll strip"): each paint says which session's
//! conversation the window shows (crate::conversations::shows), which is all the transcript's
//! reading thread hears from it. A grown conversation is laid out from the first block that
//! changed (Snapshot::layout_from), and the screen is read again only when the conversation's
//! rows or the input box's rules changed since the last look (Claude Code's spinner and status
//! under them change many times a second while it works and say nothing of where it's scrolled
//! to). Until 2026-10-30 each grown snapshot was laid out whole, and every change to the pane
//! read and located the screen, at most every 120 ms with a repaint of its own to catch up.
//!
//! The strip, its thumb and marks are drawn in `over` (z-layer 38, the objectives strip's) and
//! never move: as shapes for render/sides.rs, which draws them there and, as a tab is switched
//! from or closed, fades the strip as that tab showed it out over the next tab's with its panes.
//! Resting the pointer on a mark opens a card beside it saying what each mark there
//! is; it fades as the objectives' cards do, the fourth of their visuals (render/objectives.rs,
//! `CARD`).

use crate::scrollbar::ScrollHit;
use crate::termwindow::render::objectives::{over, CARD, ROW_HOVER_ALPHA, TEXT_ALPHA};
use crate::termwindow::render::prompt;
use crate::termwindow::render::sides::Shape;
use crate::termwindow::render::tab_panel::srgb;
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::{ConfigHandle, DimensionContext};
use lbterm_extras::conversation::{self as conv, Card, CardItem, CardTone, Kind, Layout, Seek, Shape as MarkShape, Snapshot, Step, View};
use lbterm_extras::scroll_note::{Note, Why};
use lbterm_extras::strip::{self as strip_card, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Hover};
use mux::pane::{Pane, PaneId};
use mux::Mux;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wezterm_term::StableRowIndex;
use window::color::LinearRgba;

/// Which of the objectives' cards is this strip's (render/objectives.rs, `CARD`)
pub(crate) const WHICH: usize = 3;
/// The thumb: white this far over the panes' colour, and while the pointer's on the strip or
/// it's being dragged
const THUMB_ALPHA: f32 = 0.24;
const THUMB_LIT: f32 = 0.36;
/// Black over the part of a conversation above its last compaction, which Claude Code can't
/// scroll to
const DIM_ALPHA: f32 = 0.45;
/// How far from a mark the pointer may be and still be on it, at 96 DPI
const SLOP: f64 = 4.0;
/// The most marks one card lists
const MAX_ITEMS: usize = 6;
/// How long a click that didn't get there says why
const NOTE_FOR: Duration = Duration::from_secs(4);
/// How far the pointer moves on the thumb before it's a drag, at any DPI
const DRAG_SLOP: f64 = 3.0;

/// As the GUI starts: a write to a pane that didn't get there (mux::lbterm_writes) goes to the
/// window holding the pane, whose strip says so (`lbterm_scroll_write_failed`). On the GUI
/// thread, where a window is found, whichever thread heard it
pub fn start() {
    mux::lbterm_writes::on_failed(|pane, why, reached| {
        let maybe = reached == mux::lbterm_writes::Reached::Maybe;
        promise::spawn::spawn_into_main_thread(async move {
            let Some((_, window, _)) = Mux::get().resolve_pane_id(pane) else {
                return;
            };
            let Some(gui) = crate::frontend::try_front_end().and_then(|f| f.gui_window_for_mux_window(window)) else {
                return;
            };
            gui.window.notify(TermWindowNotif::Apply(Box::new(move |tw| tw.lbterm_scroll_write_failed(pane, why, maybe))));
        })
        .detach();
    });
}

/// Whether the strip is on: asked for, with the tab panel to line up with
fn on(config: &ConfigHandle) -> bool {
    config.lbterm_scroll_strip.is_some() && config.lbterm_tab_panel.enabled
}

/// Whether the panes keep their own scrollbar, and its room: not with the strip on, which is
/// their scrollbar then
pub fn panes_keep_scroll_bar(config: &ConfigHandle) -> bool {
    !on(config)
}

/// The strip's own width, its ring included, in pixels
fn width_px(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    config.lbterm_scroll_strip.as_ref().map_or(0.0, |s| {
        s.width
            .evaluate_as_pixels(DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: dimensions.pixel_width as f32,
                pixel_cell: metrics.cell_size.width as f32,
            })
            .round()
    })
}

/// The strip the window keeps down its right for it (the strip and a gutter after it), 0 with
/// it off. The window treats it as border, as it does the objectives strip's, so the panes'
/// last card sits a gutter short of it; the tab bar takes it back (fancy_tab_bar.rs).
pub fn strip_width(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    if !on(config) {
        return 0.0;
    }
    width_px(config, dimensions, metrics) + lbterm_panel::Shape::new(dimensions.dpi as f32 / 96.0).gutter()
}

/// What the strip is the scrollbar of
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    None,
    /// The pane's scrollback
    Shell,
    /// Claude Code's conversation
    Claude,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::None => "none",
            Mode::Shell => "shell",
            Mode::Claude => "claude",
        }
    }
}

/// Where a mark leads
#[derive(Debug, Clone, Copy, PartialEq)]
enum Goes {
    /// The conversation's event, by its place in the snapshot's events
    Event(usize),
    /// A row of the pane's scrollback: a shell prompt's
    Row(StableRowIndex),
}

/// A mark as drawn
#[derive(Debug, Clone)]
struct Drawn {
    kind: Kind,
    rect: Rect,
    /// Its y on the strip, for the pointer
    middle: f64,
    goes: Goes,
}

/// A seek under way, shared with its task: the block it's going to now, which a drag moves,
/// and whether Lachlan has taken over (a key or the wheel in the pane)
struct SeekCtl {
    pane: PaneId,
    /// The block and the line of it
    target: Cell<(usize, usize)>,
    cancelled: Cell<bool>,
    /// A write to the pane failed (`lbterm_scroll_write_failed`): it ends as gone at its next
    /// look rather than wait out its burst's time for a screen that won't move
    gone: Cell<bool>,
}

/// How a seek ended
enum Sought {
    Landed(usize),
    NotThere,
    /// Something of Claude Code's in its input box's place (a dialog, a permission prompt)
    Covered,
    TooFar,
    Gone,
    /// The pane's connection to its mux server is down: its keys would wait out the reconnect
    /// and scroll the conversation after it (wezterm-client's lbterm_link.rs)
    Down,
    /// Lachlan took over
    Cancelled,
}

/// Whether `pane`'s connection to its mux server is down, so a burst sent now would wait for
/// the reconnect (wezterm-client's lbterm_link.rs); a local pane's never is
fn link_down(pane: &Arc<dyn Pane>) -> bool {
    pane.downcast_ref::<wezterm_client::pane::ClientPane>().map_or(false, |c| c.lbterm_link_down())
}

/// When a Claude pane's screen was last read for where its view is, and what it said
#[derive(Debug, Clone)]
struct Looked {
    /// The pane's sequence number then, from which a change since is asked for
    seqno: usize,
    /// The snapshot (its address) and layout it was looked at with, and the pane's size
    with: (usize, usize, usize, usize),
    /// Where its conversation's rows ended and its input box's rules were: a change elsewhere
    /// (Claude Code's status) says nothing new
    area: Option<(usize, usize, usize)>,
    /// Each row's words then ([`row_key`]): a row written again with the same words didn't
    /// change, and Claude Code writes its whole bottom block again for each tick of its spinner
    texts: Vec<u64>,
}

/// A row's words, as a look keeps them to tell whether the row changed since
fn row_key(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The marks as drawn for a conversation at a place, kept until either changes: the snapshot's
/// and the layout's addresses, the track and the scale
type MarksKey = (usize, usize, [u64; 4], u64);

pub struct ScrollStripState {
    hover: Hover,
    strip: Option<Rect>,
    track: Option<Rect>,
    thumb: Option<Rect>,
    mode: Mode,
    pane: Option<PaneId>,
    snapshot: Option<Arc<Snapshot>>,
    /// The layout and the snapshot it's for: a snapshot of the conversation grown is laid out
    /// from where the two part
    layout: Option<(Arc<Snapshot>, Arc<Layout>)>,
    /// Where each Claude pane's view was last found
    views: HashMap<PaneId, View>,
    /// What each Claude pane's screen last said: how many rows its conversation has
    screens: HashMap<PaneId, conv::Screen>,
    /// When each Claude pane's screen was last read
    looked: HashMap<PaneId, Looked>,
    /// The marks, and the order they're drawn in (quietest first); a conversation's are made
    /// again only when it or where it's drawn changes
    marks: Rc<Vec<Drawn>>,
    order: Rc<Vec<usize>>,
    marks_for: Option<(MarksKey, Rc<Vec<Drawn>>, Rc<Vec<usize>>, Option<f64>)>,
    /// The pointer is on the strip
    pointer_on: bool,
    /// The marks under the pointer, best first, by their place in `marks`
    hovered: Vec<usize>,
    /// The card while open, and what each of its marks leads to
    card: Option<Card>,
    card_goes: Vec<(Goes, f64)>,
    last_card: Option<Card>,
    /// The card's item under the pointer
    card_row: Option<usize>,
    visual: Option<(PixelRect, bool)>,
    /// A drag of the thumb: how far down the thumb the pointer took it, and where its top is now
    drag: Option<(f64, f64)>,
    /// Where the press on the thumb was, whether the pointer has moved since, and the mark under
    /// it: a press and release that doesn't move is a click on that mark
    drag_from: f64,
    drag_moved: bool,
    drag_mark: Option<(Goes, f64)>,
    seek: Option<Rc<SeekCtl>>,
    /// Why the last click didn't get there, or that a write to a pane didn't
    /// (lbterm_extras::scroll_note), until when, level with what it's about
    note: Option<Note>,
    wake: Option<Instant>,
    pressed: bool,
    /// The conversation's rows and the view's top, and how many times the screen's been read for
    /// where the view is, for the state file
    rows: Option<usize>,
    view_top: Option<usize>,
    looks: u64,
}

impl Default for ScrollStripState {
    fn default() -> Self {
        Self {
            hover: Hover::new(Duration::from_millis(150), Duration::from_millis(200)),
            strip: None,
            track: None,
            thumb: None,
            mode: Mode::None,
            pane: None,
            snapshot: None,
            layout: None,
            views: HashMap::new(),
            screens: HashMap::new(),
            looked: HashMap::new(),
            marks: Rc::default(),
            order: Rc::default(),
            marks_for: None,
            pointer_on: false,
            hovered: vec![],
            card: None,
            card_goes: vec![],
            last_card: None,
            card_row: None,
            visual: None,
            drag: None,
            drag_from: 0.0,
            drag_moved: false,
            drag_mark: None,
            seek: None,
            note: None,
            wake: None,
            pressed: false,
            rows: None,
            view_top: None,
            looks: 0,
        }
    }
}

impl ScrollStripState {
    fn reset(&mut self) {
        self.hover.close();
        (self.strip, self.track, self.thumb, self.card, self.visual) = (None, None, None, None, None);
        self.marks = Rc::default();
        self.order = Rc::default();
        self.hovered.clear();
        self.mode = Mode::None;
        self.drag = None;
    }
}

/// `rgb` at `alpha`
fn tint(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A key Claude Code scrolls by, pressed `n` times in its pane in one write. Claude Code takes
/// a burst written at once in one go and draws once at the end; the same keys written one by
/// one, it draws as it goes, so a look between its frames saw a screen it was still moving
/// past (measured on 2.1.312: 120 PgDns in one write, one frame after 530 ms).
fn press(pane: &Arc<dyn Pane>, key: conv::Key, n: u32) -> bool {
    let sequence = match key {
        conv::Key::PageUp => "\x1b[5~",
        conv::Key::PageDown => "\x1b[6~",
        conv::Key::Top => "\x1b[1;5H",
        conv::Key::Bottom => "\x1b[1;5F",
    };
    let burst = sequence.repeat(n.clamp(1, MAX_BURST) as usize);
    pane.writer().write_all(burst.as_bytes()).is_ok()
}

/// The most keys one burst presses
const MAX_BURST: u32 = 200;
/// How long a burst can take Claude Code before it draws: on a long conversation about 250 ms
/// for 40 PgUps and 530 ms for 120 (a 5,800-row session, 2.1.312), so a base and a part for
/// each key, with room to spare; a burst at an end draws nothing at all
fn start_max(n: u32) -> Duration {
    Duration::from_millis(400 + 6 * n.min(MAX_BURST) as u64)
}
const START_POLL: Duration = Duration::from_millis(20);
/// How long the screen must hold still after a burst to count as drawn: Claude Code can draw
/// the conversation moved and "Jump to bottom" over it a frame later, over 80 ms apart on a long
/// conversation, and a look between them takes a scrolled screen for the latest
const SEEK_QUIET: Duration = Duration::from_millis(160);
/// The longest a seek waits for the screen to hold still
const SEEK_STILL_MAX: Duration = Duration::from_millis(1500);
/// How many times a seek that's found its line looks again before it trusts it, while the
/// screen keeps changing (Claude writing at the latest)
const CONFIRMS: u32 = 3;

type Rows = Vec<lbterm_extras::prompts::Row>;

/// The conversation's rows of a screen: what a seek waits on, not the spinner or the input box
fn area(rows: &[lbterm_extras::prompts::Row]) -> &[lbterm_extras::prompts::Row] {
    &rows[..conv::Screen::read(rows).map_or(rows.len(), |s| s.area).min(rows.len())]
}

/// The screen once its conversation has held still for [`SEEK_QUIET`] (or [`SEEK_STILL_MAX`]
/// gone by)
async fn held_still(pane: &Arc<dyn Pane>, ctl: &SeekCtl) -> Rows {
    let start = Instant::now();
    let mut rows = prompt::read_rows(pane);
    let mut still = Instant::now();
    loop {
        lbterm_extras::counters::timer(smol::Timer::after(START_POLL)).await;
        let now = prompt::read_rows(pane);
        if area(&now) != area(&rows) {
            rows = now;
            still = Instant::now();
        } else if still.elapsed() >= SEEK_QUIET || start.elapsed() >= SEEK_STILL_MAX || ctl.cancelled.get() || ctl.gone.get() {
            return now;
        }
    }
}

/// The screen once Claude Code has drawn what a burst of `n` keys asked for: the conversation's
/// rows moved from `before` (or [`start_max`] gone by without, at an end), then holding still.
/// A key or wheel of Lachlan's stops the wait, and so does a write to the pane failing (the
/// burst's own, whose answer came after it was sent), which ends the seek as gone
async fn after_burst(pane: &Arc<dyn Pane>, before: &[lbterm_extras::prompts::Row], n: u32, ctl: &SeekCtl) -> Rows {
    let start = Instant::now();
    loop {
        lbterm_extras::counters::timer(smol::Timer::after(START_POLL)).await;
        let now = prompt::read_rows(pane);
        if ctl.gone.get() {
            return now;
        }
        if area(&now) != area(before) || ctl.cancelled.get() {
            break;
        }
        if start.elapsed() >= start_max(n) {
            return now;
        }
    }
    held_still(pane, ctl).await
}

/// Scroll Claude Code's conversation in pane `pane_id` until block `ctl.target` is on screen, a
/// burst of keys and a look at a time; a drag moving the target sends the next burst towards
/// the new one, and any key or wheel of Lachlan's in the pane stops it (`ctl.cancelled`)
async fn seek(pane_id: PaneId, snapshot: Arc<Snapshot>, layout: Arc<Layout>, ctl: Rc<SeekCtl>, mut near: Option<usize>) -> Sought {
    let Some(pane) = Mux::get().get_pane(pane_id) else {
        return Sought::Gone;
    };
    let mut rows = prompt::read_rows(&pane);
    let mut seek: Option<Seek> = None;
    let mut confirms = 0;
    loop {
        if ctl.gone.get() {
            return Sought::Gone;
        }
        if ctl.cancelled.get() {
            return Sought::Cancelled;
        }
        let target = ctl.target.get();
        if seek.as_ref().map(|s| s.target()) != Some(target) {
            match Seek::new(&snapshot, target.0, target.1) {
                Some(s) => seek = Some(s),
                None => return Sought::NotThere,
            }
        }
        let Some(s) = seek.as_mut() else {
            return Sought::NotThere;
        };
        if conv::Screen::read(&rows).is_none() {
            return Sought::Covered;
        }
        let step = s.look(&snapshot, &layout, &rows, near);
        log::debug!(
            "scroll strip: seek {target:?}, round {}, view near {near:?}: {step:?}; screen {:?}",
            s.rounds(),
            conv::Screen::read(&rows)
        );
        match step {
            Step::Found(row) => {
                // Found on a screen Claude Code may not have finished: once it's held still a
                // while longer, the same screen, else look again at the one it came to
                let again = held_still(&pane, &ctl).await;
                confirms += 1;
                if area(&again) == area(&rows) || confirms >= CONFIRMS {
                    return Sought::Landed(row);
                }
                rows = again;
                near = snapshot.locate(&layout, &rows, near).map(|v| v.top).or(near);
            }
            Step::Press(key, n) => {
                // No burst while the connection is down: it would wait out the reconnect, with
                // nothing to steer by, and scroll the conversation after it. A write a local pane
                // fails at once ends it here; one the server refuses ends it at the next look,
                // once the failure's heard (`ctl.gone`)
                if link_down(&pane) {
                    return Sought::Down;
                }
                if !press(&pane, key, n) {
                    return Sought::Gone;
                }
                rows = after_burst(&pane, &rows, n, &ctl).await;
                near = snapshot.locate(&layout, &rows, near).map(|v| v.top).or(near);
            }
            Step::NotThere => return Sought::NotThere,
            Step::TooFar => return Sought::TooFar,
        }
    }
}

impl crate::TermWindow {
    /// Each paint, after the objectives panel: the strip beside the active pane's scrolled rows
    /// (Claude Code's conversation, above its input box; a shell's whole screen), its thumb and
    /// marks, and the card while it's open or fading
    pub(crate) fn paint_lbterm_scroll_strip(&mut self) -> anyhow::Result<()> {
        let layout = match self.lbterm_panel.layout {
            // Not while the Conductor's view covers the pane it's the scrollbar of: it would say
            // where a pane nobody can see is scrolled to, which nothing repaints while the view is
            // open (termwindow/mod.rs, mux_pane_output_event), and a click would scroll it unseen.
            // Leaving the view repaints, and it's back
            Some(layout) if on(&self.config) && !self.lbterm_view_is_open() => layout,
            _ => {
                self.lbterm_scroll_strip.reset();
                crate::conversations::shows(self.mux_window_id, crate::conversations::Reader::ScrollStrip, None);
                self.lbterm_context_bar_rode(None);
                return Ok(());
            }
        };
        let settings = self.config.lbterm_scroll_strip.clone().unwrap_or_default();
        let motion = self.lbterm_motion();
        let composited = self.lbterm_composited();
        let closing = if composited && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        let hover = &mut self.lbterm_scroll_strip.hover;
        hover.set_delays(
            Duration::from_millis(settings.open_delay_ms),
            Duration::from_millis(settings.close_delay_ms),
        );
        hover.set_motion(Duration::ZERO, closing);

        let room = self.lbterm_room()?;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let panel = layout.panel(false);
        let surface = self.palette().background;

        // What it's the scrollbar of, and the active pane's rows it stands beside: of the panes
        // drawn, whose picture it fades with (render/sides.rs), so while a held pane's push
        // hasn't come, the kept tab's beside its screen (termwindow/frame_wait.rs)
        let pane = self.lbterm_pane_to_paint();
        let pos = self.lbterm_panes_to_paint().into_iter().find(|p| p.is_active);
        let sid = pane.as_ref().and_then(|p| {
            p.copy_user_vars()
                .remove("claude_session")
                .filter(|s| !s.is_empty() && p.is_alt_screen_active())
        });
        let mode = match (&pane, &sid) {
            (None, _) => Mode::None,
            (Some(_), Some(_)) => Mode::Claude,
            (Some(_), None) => Mode::Shell,
        };
        // Which conversation this window shows: the reading thread follows the ones shown
        crate::conversations::shows(self.mux_window_id, crate::conversations::Reader::ScrollStrip, sid.as_deref());
        let conversation = match (&pane, &sid) {
            (Some(pane), Some(sid)) => self.lbterm_scroll_look(pane, sid),
            _ => None,
        };
        let span = match (mode, &pane, &pos) {
            (Mode::Claude, Some(pane), Some(pos)) => {
                self.lbterm_scroll_strip.screens.get(&pane.pane_id()).map_or(pos.height, |s| s.area.min(pos.height))
            }
            (_, _, Some(pos)) => pos.height,
            _ => 0,
        };
        let stroke = shape.hairline() as f64;
        let width = width_px(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let radius = (shape.radius() as f64).min(width / 2.0);
        let inset = stroke + (2.0 * scale).round();
        let ends = radius.max(inset);
        let (top, bottom) = match &pos {
            Some(pos) if span > 0 => {
                let (a, b) = self.lbterm_scroll_rows_y(pos, span)?;
                ((a - ends).max(panel.y as f64), (b + ends).min((panel.y + panel.h) as f64))
            }
            _ => (panel.y as f64, (panel.y + panel.h) as f64),
        };
        // While the context bar's drawer or pane is out, the strip rides on its left edge
        // (render/context_bar.rs): drawn there, and with the compositor in a visual of its own
        // that DWM glides there from where it showed
        let ride = self.lbterm_context_bar_ride();
        let strip = Rect { x: room.right as f64 + ride.offset, y: top, w: width, h: (bottom - top).max(2.0 * ends + 1.0) };
        let track = Rect {
            x: strip.x + inset,
            y: strip.y + ends,
            w: (strip.w - 2.0 * inset).max(1.0),
            h: (strip.h - 2.0 * ends).max(1.0),
        };

        let (thumb, marks, dim_to) = match (&pane, conversation) {
            (Some(pane), Some((snapshot, conv_layout))) => self.lbterm_scroll_claude(pane, &snapshot, &conv_layout, track, scale),
            (Some(pane), None) if mode == Mode::Shell => {
                self.lbterm_scroll_strip.snapshot = None;
                let (thumb, marks) = self.lbterm_scroll_shell(pane, track, scale);
                let order = Rc::new((0..marks.len()).collect());
                (Some(thumb), (Rc::new(marks), order), None)
            }
            _ => (None, (Rc::default(), Rc::default()), None),
        };
        let state = &mut self.lbterm_scroll_strip;
        state.mode = mode;
        state.pane = pane.as_ref().map(|p| p.pane_id());
        // A dragged thumb is where the pointer has it until Claude Code catches up
        let thumb = match (state.drag, thumb, mode) {
            (Some((_, top)), Some(t), Mode::Claude) => Some(Rect { y: top.clamp(track.y, track.y + track.h - t.h), ..t }),
            _ => thumb,
        };
        state.strip = Some(strip);
        state.track = Some(track);
        state.thumb = thumb;
        (state.marks, state.order) = marks;
        // The marks under the pointer may have moved under it
        if state.pointer_on {
            if let Some(y) = self.current_mouse_event.as_ref().map(|e| e.coords.y as f64) {
                let hovered = self.lbterm_scroll_marks_at(y);
                self.lbterm_scroll_strip.hovered = hovered;
            }
        }
        let lit = self.lbterm_scroll_strip.pointer_on || self.lbterm_scroll_strip.drag.is_some();
        let mut shapes = self.lbterm_scroll_draw(&layout, room, strip, track, dim_to, surface, lit, scale);
        if ride.own {
            // The panes' colour under its own place stays; the strip goes to its visual
            let riding = shapes.split_off(1.min(shapes.len()));
            self.lbterm_sides_draw(shapes);
            self.lbterm_shapes(super::context_bar::RIDE_Z, &riding)?;
            let (x0, y0) = (strip.x.floor(), strip.y.floor());
            let (x1, y1) = ((strip.x + strip.w).ceil(), (strip.y + strip.h).ceil());
            let bounds = PixelRect::new(x0 as i32, y0 as i32, (x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32);
            self.lbterm_context_bar_rode(Some((bounds, (x0 - ride.offset) as i32)));
        } else {
            self.lbterm_sides_draw(shapes);
            self.lbterm_context_bar_rode(None);
        }

        // The card: laid out afresh while open, and with the compositor drawn as it was while
        // its visual fades out
        let now = Instant::now();
        if self.lbterm_scroll_strip.note.as_ref().map_or(false, |n| n.gone(now)) {
            self.lbterm_scroll_strip.note = None;
        }
        let noted = self.lbterm_scroll_shown_note(now).is_some();
        let open = self.lbterm_scroll_strip.hover.is_open() || noted;
        let (fading, settles) = if composited {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(([false; super::objectives::CARDS], None), |c| c.objectives_showing(now))
        } else {
            ([false; super::objectives::CARDS], None)
        };
        let card = match strip_card::card_draw(open, fading[WHICH]) {
            CardDraw::Fresh => self.lbterm_scroll_lay_out_card(&layout, strip, &settings, scale)?,
            CardDraw::Last => self.lbterm_scroll_strip.last_card.clone(),
            CardDraw::None => None,
        };
        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        self.lbterm_scroll_strip.visual = card.as_ref().and_then(|c| {
            strip_card::visual_bounds(&c.rect, scale, window).map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open))
        });
        self.lbterm_scroll_strip.card = if open { card.clone() } else { None };
        if let Some(card) = card {
            self.lbterm_scroll_draw_card(&card, surface, shape.radius() as f64, stroke, scale)?;
            self.lbterm_scroll_strip.last_card = Some(card);
        }
        self.lbterm_scroll_wake(settles);
        let note_due = self.lbterm_scroll_strip.note.as_ref().map(|n| n.until);
        let due = [self.lbterm_scroll_strip.hover.deadline(), note_due].iter().flatten().min().copied();
        self.lbterm_scroll_wake(due);
        Ok(())
    }

    /// Where rows 0 and `rows` of pane `pos` start, in the window's pixels, as its text is drawn
    fn lbterm_scroll_rows_y(&self, pos: &crate::termwindow::PositionedPane, rows: usize) -> anyhow::Result<(f64, f64)> {
        let (_, padding_top) = self.padding_left_top();
        let border = self.get_os_border();
        let tab_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height()? as f64
        } else {
            0.0
        };
        let cell = self.render_metrics.cell_size.height as f64;
        let y0 = border.top.get() as f64 + tab_bar + padding_top as f64 + pos.top as f64 * cell;
        Ok((y0, y0 + rows as f64 * cell))
    }

    /// A shell's scrollback: the viewport's thumb, a bar at each prompt
    fn lbterm_scroll_shell(&mut self, pane: &Arc<dyn Pane>, track: Rect, scale: f64) -> (Rect, Vec<Drawn>) {
        self.lbterm_scroll_strip.rows = None;
        self.lbterm_scroll_strip.view_top = None;
        let dims = pane.get_dimensions();
        let viewport = self.get_viewport(pane.pane_id());
        let min = self.min_scroll_bar_height() as usize;
        let info = ScrollHit::thumb(&**pane, viewport, track.h as usize, min);
        let thumb = Rect { x: track.x, y: track.y + info.top as f64, w: track.w, h: info.height as f64 };
        let rows = self.lbterm_prompt_marks(pane);
        let bar = (2.0 * scale).round().max(1.0);
        let room = (track.h - bar).max(0.0);
        let total = dims.scrollback_rows.max(1) as f64;
        let mut marks: Vec<Drawn> = vec![];
        for &row in rows.iter() {
            let offset = row - dims.scrollback_top;
            if offset < 0 || offset as usize >= dims.scrollback_rows {
                continue;
            }
            let y = (track.y + offset as f64 / total * room).round();
            // A burst of short prompts is one bar, not a solid block
            if marks.last().map_or(false, |m| y < m.rect.y + bar) {
                continue;
            }
            marks.push(Drawn {
                kind: Kind::Prompt,
                rect: Rect { x: track.x, y, w: track.w, h: bar },
                middle: y + bar / 2.0,
                goes: Goes::Row(row),
            });
        }
        (thumb, marks)
    }

    /// The conversation's layout at `cols`, made again only when the snapshot or the width
    /// changes, and then from the first block that changed: a snapshot of the conversation
    /// grown shares every block before what was added with the one before it
    fn lbterm_scroll_layout(&mut self, snapshot: &Arc<Snapshot>, cols: usize) -> Arc<Layout> {
        let layout = match &self.lbterm_scroll_strip.layout {
            Some((was, layout)) if Arc::ptr_eq(was, snapshot) && layout.cols == cols => return Arc::clone(layout),
            Some((was, layout)) if layout.cols == cols => {
                Arc::new(snapshot.layout_from(layout, snapshot.blocks.first_difference(&was.blocks)))
            }
            _ => Arc::new(snapshot.layout(cols)),
        };
        self.lbterm_scroll_strip.layout = Some((Arc::clone(snapshot), Arc::clone(&layout)));
        layout
    }

    /// Whether `pane`'s screen changed since `looked` where it says where Claude Code's view
    /// is: the conversation's rows, or the input box's rules (which say where they end), or a
    /// row anywhere else that's become a rule. Its spinner and status, which change many times a
    /// second while Claude Code works, say nothing new.
    fn lbterm_scroll_moved(pane: &Arc<dyn Pane>, looked: &Looked) -> bool {
        let dims = pane.get_dimensions();
        let top = dims.physical_top;
        let changed = pane.get_changed_since(top..top + dims.viewport_rows as StableRowIndex, looked.seqno);
        let Some((area, upper, lower)) = looked.area else {
            return !changed.is_empty();
        };
        // A row of the conversation's or a rule whose words differ from the last look's, but
        // Claude Code's spinner turning: 2.1.314 draws it under the conversation, in its area,
        // and writes its whole bottom block again for each tick, about six a second, each of
        // which was a look at the screen (in a test window against real Claude Code, 2026-10-31)
        let moved = changed.iter_values().any(|row| {
            let i = (row - top).max(0) as usize;
            let (_, lines) = pane.get_lines(row..row + 1);
            let text = lines.first().map(|l| l.as_str().into_owned()).unwrap_or_default();
            if looked.texts.get(i) == Some(&row_key(&text)) {
                return false;
            }
            if i == upper || i == lower {
                return true;
            }
            if i < area {
                return !conv::is_spinner(&text);
            }
            conv::is_rule(&lbterm_extras::prompts::Row { text, band: false, dim: false })
        });
        moved
    }

    /// Session `sid`'s conversation in `pane`, laid out at its width, and a fresh look at the
    /// screen when the conversation, its layout or what the screen shows of it has changed:
    /// where Claude Code's view is, and how many rows its conversation has above the input box
    fn lbterm_scroll_look(&mut self, pane: &Arc<dyn Pane>, sid: &str) -> Option<(Arc<Snapshot>, Arc<Layout>)> {
        let Some(snapshot) = crate::conversations::get(sid) else {
            self.lbterm_scroll_strip.snapshot = None;
            return None;
        };
        self.lbterm_scroll_strip.snapshot = Some(Arc::clone(&snapshot));
        let dims = pane.get_dimensions();
        let layout = self.lbterm_scroll_layout(&snapshot, dims.cols);
        let id = pane.pane_id();
        let seqno = pane.get_current_seqno();
        let with = (Arc::as_ptr(&snapshot) as usize, Arc::as_ptr(&layout) as usize, dims.cols, dims.viewport_rows);
        let look = match self.lbterm_scroll_strip.looked.get(&id) {
            None => true,
            Some(last) if last.with != with => true,
            Some(last) if last.seqno == seqno => false,
            Some(last) => Self::lbterm_scroll_moved(pane, last),
        };
        if look {
            self.lbterm_scroll_strip.looks += 1;
            let rows = prompt::read_rows(pane);
            let screen = conv::Screen::read(&rows);
            if let Some(screen) = screen {
                self.lbterm_scroll_strip.screens.insert(id, screen);
            }
            let near = self.lbterm_scroll_strip.views.get(&id).map(|v| v.top);
            if let Some(view) = snapshot.locate(&layout, &rows, near) {
                self.lbterm_scroll_strip.views.insert(id, view);
            }
            let area = screen.zip(conv::Screen::rules(&rows)).map(|(s, (upper, lower))| (s.area, upper, lower));
            let texts = rows.iter().map(|r| row_key(&r.text)).collect();
            self.lbterm_scroll_strip.looked.insert(id, Looked { seqno, with, area, texts });
        } else if let Some(last) = self.lbterm_scroll_strip.looked.get_mut(&id) {
            // Nothing that matters changed up to now: changes are asked for from here on
            last.seqno = seqno;
        }
        // Forget panes that have closed
        if self.lbterm_scroll_strip.views.len() > 8 {
            let mux = Mux::get();
            let state = &mut self.lbterm_scroll_strip;
            state.views.retain(|id, _| mux.get_pane(*id).is_some());
            state.looked.retain(|id, _| mux.get_pane(*id).is_some());
            state.screens.retain(|id, _| mux.get_pane(*id).is_some());
        }
        Some((snapshot, layout))
    }

    /// Claude Code's conversation on the track: the thumb where its view is, a mark for each
    /// event (in the order they're drawn), and where the part above the last compaction ends.
    /// The marks are made again only when the conversation, its layout or the track changes.
    fn lbterm_scroll_claude(
        &mut self,
        pane: &Arc<dyn Pane>,
        snapshot: &Arc<Snapshot>,
        layout: &Arc<Layout>,
        track: Rect,
        scale: f64,
    ) -> (Option<Rect>, (Rc<Vec<Drawn>>, Rc<Vec<usize>>), Option<f64>) {
        let view = self.lbterm_scroll_strip.views.get(&pane.pane_id()).copied();
        self.lbterm_scroll_strip.rows = Some(layout.total);
        self.lbterm_scroll_strip.view_top = view.map(|v| v.top);
        let min = self.min_scroll_bar_height() as f64;
        let thumb = view.map(|v| {
            let (y, h) = conv::thumb(layout, v, track.h, min);
            Rect { x: track.x, y: track.y + y, w: track.w, h }
        });
        let key: MarksKey = (
            Arc::as_ptr(snapshot) as usize,
            Arc::as_ptr(layout) as usize,
            [track.x.to_bits(), track.y.to_bits(), track.w.to_bits(), track.h.to_bits()],
            scale.to_bits(),
        );
        if let Some((k, marks, order, dim_to)) = &self.lbterm_scroll_strip.marks_for {
            if *k == key {
                return (thumb, (Rc::clone(marks), Rc::clone(order)), *dim_to);
            }
        }
        let bar = (2.0 * scale).round().max(2.0);
        let dot = (4.0 * scale).round().max(3.0);
        let hair = scale.round().max(1.0);
        let marks: Vec<Drawn> = snapshot
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let y = (track.y + conv::mark_y(layout, e.block, track.h)?).round();
                let rect = match e.kind.shape() {
                    MarkShape::Bar => Rect { x: track.x, y: y - bar / 2.0, w: track.w, h: bar },
                    MarkShape::Dot => Rect { x: (track.x + (track.w - dot) / 2.0).round(), y: y - dot / 2.0, w: dot, h: dot },
                    MarkShape::Tick => Rect { x: track.x, y, w: (track.w * 0.45).round().max(2.0), h: hair },
                    MarkShape::Line => Rect { x: track.x - 1.0, y, w: track.w + 2.0, h: hair },
                };
                Some(Drawn { kind: e.kind, rect, middle: y, goes: Goes::Event(i) })
            })
            .collect();
        let dim_to = snapshot
            .compacted_at
            .and_then(|c| conv::mark_y(layout, c, track.h))
            .map(|y| (track.y + y).round());
        let mut order: Vec<usize> = (0..marks.len()).collect();
        order.sort_by_key(|&i| marks[i].kind.rank());
        let (marks, order) = (Rc::new(marks), Rc::new(order));
        self.lbterm_scroll_strip.marks_for = Some((key, Rc::clone(&marks), Rc::clone(&order), dim_to));
        (thumb, (marks, order), dim_to)
    }

    /// The strip: the panes' colour, its ring, lit while the pointer is on it, the part above a
    /// compaction dimmed, the thumb, the marks quietest first, as shapes for `over`
    /// (render/sides.rs)
    fn lbterm_scroll_draw(
        &self,
        layout: &lbterm_panel::Layout,
        room: crate::termwindow::render::card::Room,
        strip: Rect,
        track: Rect,
        dim_to: Option<f64>,
        surface: config::SrgbaTuple,
        lit: bool,
        scale: f64,
    ) -> Vec<Shape> {
        let shape = self.lbterm_shape();
        // Under it, from the panes' room to the gutter after it, under the tab bar
        let under = Rect {
            x: room.right as f64,
            y: layout.body_top as f64,
            w: strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64,
            h: (layout.bottom - layout.body_top) as f64,
        };
        let mut shapes = vec![Shape::new(under, 0.0, surface.to_linear())];
        let outline = if lit {
            palette::blend(palette::OUTLINE, 0xFFFFFF, palette::OUTLINE_LIT)
        } else {
            palette::OUTLINE
        };
        let stroke = shape.hairline() as f64;
        let radius = (shape.radius() as f64).min(strip.w / 2.0);
        shapes.push(Shape::new(strip, radius, srgb(outline)));
        let inner = Rect { x: strip.x + stroke, y: strip.y + stroke, w: strip.w - 2.0 * stroke, h: strip.h - 2.0 * stroke };
        shapes.push(Shape::new(inner, radius - stroke, surface.to_linear()));
        if let Some(to) = dim_to.filter(|to| *to > track.y) {
            let dim = Rect { x: track.x, y: track.y, w: track.w, h: to - track.y };
            shapes.push(Shape::new(dim, scale * 2.0, LinearRgba::with_components(0., 0., 0., DIM_ALPHA)));
        }
        let state = &self.lbterm_scroll_strip;
        if let Some(thumb) = state.thumb {
            let alpha = if lit { THUMB_LIT } else { THUMB_ALPHA };
            shapes.push(Shape::new(thumb, thumb.w / 2.0, over(surface, alpha)));
        }
        for m in state.order.iter().filter_map(|&i| state.marks.get(i)) {
            let colour = match m.kind.shape() {
                MarkShape::Tick => tint(m.kind.colour(), 0.75),
                MarkShape::Line => tint(m.kind.colour(), 0.6),
                _ => tint(m.kind.colour(), 0.95),
            };
            let radius = match m.kind.shape() {
                MarkShape::Dot => m.rect.w / 2.0,
                _ => 0.0,
            };
            shapes.push(Shape::new(m.rect, radius, colour));
        }
        shapes
    }

    /// The marks within reach of `y`, nearest first, the weightier first at the same place
    fn lbterm_scroll_marks_at(&self, y: f64) -> Vec<usize> {
        let slop = SLOP * self.dimensions.dpi as f64 / 96.0;
        let marks = &self.lbterm_scroll_strip.marks;
        let mut near: Vec<usize> = (0..marks.len())
            .filter(|&i| (marks[i].middle - y).abs() <= slop + marks[i].rect.h / 2.0)
            .collect();
        near.sort_by(|&a, &b| {
            let d = |i: usize| ((marks[i].middle - y).abs() * 2.0).round() as i64;
            d(a).cmp(&d(b)).then(marks[b].kind.rank().cmp(&marks[a].kind.rank()))
        });
        near.truncate(MAX_ITEMS);
        near
    }

    /// The card's width in pixels, at most what the room leaves
    fn lbterm_scroll_card_width(&self, settings: &config::lbterm::LbtermScrollStrip, room: f64) -> f64 {
        let w = settings.card_width.evaluate_as_pixels(DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.dimensions.pixel_width as f32,
            pixel_cell: self.render_metrics.cell_size.width as f32,
        }) as f64;
        w.min(room).max(0.0).round()
    }

    /// The card: what the marks under the pointer are (or why the last click didn't get there),
    /// a gutter left of the strip, level with them
    fn lbterm_scroll_lay_out_card(
        &mut self,
        layout: &lbterm_panel::Layout,
        strip: Rect,
        settings: &config::lbterm::LbtermScrollStrip,
        scale: f64,
    ) -> anyhow::Result<Option<Card>> {
        let (_, _, cell) = self.lbterm_objectives_font()?;
        let gutter = self.lbterm_shape().gutter() as f64;
        let right = strip.x - gutter;
        let room = self.lbterm_room()?;
        let width = self.lbterm_scroll_card_width(settings, right - room.left as f64 - gutter);
        let gap = layout.gap() as f64;
        let bounds = |at: f64| (at, layout.body_top as f64 + gap, layout.bottom as f64 - gap);
        if let Some(note) = self.lbterm_scroll_shown_note(Instant::now()).cloned() {
            let colour = match note.why {
                Why::NotThere => palette::LABEL,
                // A write that may have arrived is still one the server never confirmed: the
                // failed red, its head saying which (Lachlan's 4a of 2026-11-08)
                Why::NotSent | Why::MayNotHaveArrived => palette::FAILED,
            };
            let item = CardItem { colour, head: note.why.head().into(), text: note.text, lines: 3 };
            self.lbterm_scroll_strip.card_goes.clear();
            return Ok(conv::card(&[item], right, bounds(note.at), width, cell, scale));
        }
        let state = &self.lbterm_scroll_strip;
        // Off the marks, on the card or on the way to it: it shows what it showed
        if state.hovered.is_empty() {
            return Ok(state.card.clone());
        }
        let now = chrono::Local::now().naive_local();
        let mut items = vec![];
        let mut goes = vec![];
        for &i in &state.hovered {
            let Some(m) = state.marks.get(i) else {
                continue;
            };
            let item = match m.goes {
                Goes::Event(e) => {
                    let Some(event) = state.snapshot.as_ref().and_then(|s| s.events.get(e)) else {
                        continue;
                    };
                    let stamp = conv::when(event.at_ms, now);
                    CardItem {
                        colour: event.kind.colour(),
                        head: if stamp.is_empty() { event.kind.label().into() } else { format!("{} \u{b7} {stamp}", event.kind.label()) },
                        text: event.text.clone(),
                        lines: if event.kind == Kind::Prompt { 5 } else { 3 },
                    }
                }
                Goes::Row(row) => CardItem {
                    colour: Kind::Prompt.colour(),
                    head: "Prompt".into(),
                    text: self.lbterm_scroll_row_text(row),
                    lines: 2,
                },
            };
            items.push(item);
            goes.push((m.goes, m.middle));
        }
        let at = goes.iter().map(|(_, y)| *y).sum::<f64>() / goes.len().max(1) as f64;
        self.lbterm_scroll_strip.card_goes = goes;
        Ok(conv::card(&items, right, bounds(at), width, cell, scale))
    }

    /// The words on a row of the active pane's scrollback: a shell prompt's
    fn lbterm_scroll_row_text(&self, row: StableRowIndex) -> String {
        let Some(pane) = self.lbterm_scroll_strip.pane.and_then(|id| Mux::get().get_pane(id)) else {
            return String::new();
        };
        let (_, lines) = pane.get_lines(row..row + 1);
        lines.first().map(|l| l.as_str().trim().to_string()).unwrap_or_default()
    }

    fn lbterm_scroll_draw_card(&self, card: &Card, surface: config::SrgbaTuple, radius: f64, stroke: f64, scale: f64) -> anyhow::Result<()> {
        self.lbterm_objectives_card(WHICH, card.rect, surface, radius, stroke, scale)?;
        if let Some(i) = self.lbterm_scroll_strip.card_row {
            if let Some(r) = card.items.get(i) {
                let gl_state = self.render_state.as_ref().unwrap();
                let layer = gl_state.layer_for_zindex(CARD[WHICH][1])?;
                let mut quads = layer.quad_allocator();
                let inset = 4.0 * scale;
                let band = Rect { x: r.x + inset, y: r.y, w: r.w - 2.0 * inset, h: r.h };
                self.lbterm_menu_rounded(&mut quads, band, radius / 2.0, over(surface, ROW_HOVER_ALPHA))?;
            }
        }
        let right = card.rect.x + card.rect.w;
        let words: Vec<_> = card
            .lines
            .iter()
            .map(|l| {
                let colour = match l.tone {
                    CardTone::Head(rgb) => srgb(rgb),
                    CardTone::Text => over(surface, TEXT_ALPHA),
                };
                (l.x, l.y, right - l.x, l.text.clone(), colour)
            })
            .collect();
        self.lbterm_objectives_words(WHICH, &words)
    }

    /// Where the card's visual goes this frame and whether it's opening, for the compositor
    pub(crate) fn lbterm_scroll_strip_visual(&self) -> Option<(PixelRect, bool)> {
        self.lbterm_scroll_strip.visual
    }

    /// The pointer moved, pressed or released over the strip or its card: the card opens once
    /// it rests on a mark; a click on a mark, or on its line on the card, goes there; a press on
    /// the thumb drags it; one elsewhere on the track goes to that point. True when the strip
    /// took the event: nothing under it gets it.
    pub(crate) fn lbterm_scroll_strip_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(strip) = self.lbterm_scroll_strip.strip else {
            return false;
        };
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        // A drag of the thumb has every event until it's let go
        if self.lbterm_scroll_strip.drag.is_some() {
            match event.kind {
                WMEK::Move => {
                    // A hand's tremble isn't a drag
                    if self.lbterm_scroll_strip.drag_moved || (y - self.lbterm_scroll_strip.drag_from).abs() > DRAG_SLOP {
                        self.lbterm_scroll_strip.drag_moved = true;
                        self.lbterm_scroll_drag_to(y);
                    }
                }
                WMEK::Release(_) => {
                    self.lbterm_scroll_strip.drag = None;
                    self.lbterm_scroll_strip.pressed = false;
                    let mark = self.lbterm_scroll_strip.drag_mark.take();
                    if let (false, Some((goes, at))) = (self.lbterm_scroll_strip.drag_moved, mark) {
                        self.lbterm_scroll_go(goes, at);
                    }
                }
                _ => {}
            }
            context.lbterm_invalidate();
            return true;
        }
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut self.lbterm_scroll_strip.pressed) {
                return true;
            }
        }
        let busy = self.current_mouse_capture.is_some() || self.window_drag_position.is_some() || self.dragging.is_some();
        let gutter = self.lbterm_shape().gutter() as f64;
        // The strip and the gutter before it, to the objectives strip's (whose handler runs first)
        let on_strip = !busy && x >= strip.x - gutter && y >= strip.y && y < strip.y + strip.h;
        let on_card = !busy && self.lbterm_scroll_strip.card.as_ref().map_or(false, |c| c.rect.contains(x, y));
        let now = Instant::now();
        let mut changed = on_strip != self.lbterm_scroll_strip.pointer_on;
        self.lbterm_scroll_strip.pointer_on = on_strip;
        if on_strip {
            let hovered = self.lbterm_scroll_marks_at(y);
            if hovered != self.lbterm_scroll_strip.hovered {
                self.lbterm_scroll_strip.hovered = hovered;
                changed = true;
            }
        } else if !on_card && !self.lbterm_scroll_strip.hovered.is_empty() {
            // Left for the card or beyond: the card keeps what it shows while it's open
            if !self.lbterm_scroll_strip.hover.is_open() {
                self.lbterm_scroll_strip.hovered.clear();
                changed = true;
            }
        }
        let card_row = if on_card {
            self.lbterm_scroll_strip.card.as_ref().and_then(|c| c.items.iter().position(|r| r.contains(x, y)))
        } else {
            None
        };
        if card_row != self.lbterm_scroll_strip.card_row {
            self.lbterm_scroll_strip.card_row = card_row;
            changed = true;
        }
        let inside = (on_strip && !self.lbterm_scroll_strip.hovered.is_empty()) || on_card;
        changed |= self.lbterm_scroll_strip.hover.pointer(inside, now);
        if !self.lbterm_scroll_strip.hover.is_open() && !inside && !on_card {
            self.lbterm_scroll_strip.card_row = None;
        }
        if changed {
            context.lbterm_invalidate();
        }
        let due = self.lbterm_scroll_strip.hover.deadline();
        self.lbterm_scroll_wake(due);
        if !(on_strip || on_card) {
            return false;
        }
        match &event.kind {
            WMEK::Press(MousePress::Left) => {
                self.lbterm_scroll_strip.pressed = true;
                let state = &self.lbterm_scroll_strip;
                if on_card {
                    if let Some((goes, at)) = card_row.and_then(|i| state.card_goes.get(i)).copied() {
                        self.lbterm_scroll_go(goes, at);
                    }
                } else if let Some(thumb) = state.thumb.filter(|t| y >= t.y && y < t.y + t.h) {
                    // On the thumb: a drag, or a click on the mark under it once let go unmoved
                    let mark = state.hovered.first().and_then(|&i| state.marks.get(i)).map(|m| (m.goes, m.middle));
                    let s = &mut self.lbterm_scroll_strip;
                    (s.drag, s.drag_from, s.drag_moved, s.drag_mark) = (Some((y - thumb.y, thumb.y)), y, false, mark);
                } else if let Some(m) = state.hovered.first().and_then(|&i| state.marks.get(i)) {
                    let (goes, at) = (m.goes, m.middle);
                    self.lbterm_scroll_go(goes, at);
                } else {
                    self.lbterm_scroll_track_click(y);
                }
                context.lbterm_invalidate();
            }
            WMEK::Press(_) => self.lbterm_scroll_strip.pressed = true,
            WMEK::Move => {
                let pointing = (on_strip && !self.lbterm_scroll_strip.hovered.is_empty()) || card_row.is_some();
                context.set_cursor(Some(if pointing { CursorIcon::Pointer } else { CursorIcon::Default }));
            }
            _ => {}
        }
        true
    }

    /// The pointer left the window
    pub(crate) fn lbterm_scroll_strip_mouse_leave(&mut self) {
        let state = &mut self.lbterm_scroll_strip;
        state.pointer_on = false;
        state.card_row = None;
        let changed = state.hover.left_window(Instant::now());
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        let due = self.lbterm_scroll_strip.hover.deadline();
        self.lbterm_scroll_wake(due);
    }

    /// Go where a mark leads: a shell prompt's row, scrolled to the top; a conversation's
    /// event, sought in Claude Code's view. `at` is the mark's y, for a note
    fn lbterm_scroll_go(&mut self, goes: Goes, at: f64) {
        let Some(pane) = self.lbterm_scroll_strip.pane.and_then(|id| Mux::get().get_pane(id)) else {
            return;
        };
        match goes {
            Goes::Row(row) => {
                let dims = pane.get_dimensions();
                self.set_viewport(pane.pane_id(), Some(row), dims);
                self.lbterm_mark_landed(pane.pane_id(), row);
            }
            Goes::Event(e) => {
                let Some(snapshot) = self.lbterm_scroll_strip.snapshot.clone() else {
                    return;
                };
                let Some(event) = snapshot.events.get(e) else {
                    return;
                };
                if snapshot.before_compaction(event.block) {
                    self.lbterm_scroll_note("It's above the last compaction: Claude Code doesn't scroll back that far", at);
                    return;
                }
                let cols = self.lbterm_scroll_strip.layout.as_ref().map_or(pane.get_dimensions().cols, |l| l.1.cols);
                match snapshot.seek_point(event.block, cols) {
                    Some(point) => self.lbterm_scroll_seek(pane.pane_id(), point),
                    None => self.lbterm_scroll_note("Nothing on Claude Code's screen to find it by", at),
                }
            }
        }
    }

    /// A press on the track off the thumb and the marks: go to that point, the thumb's middle
    /// there
    fn lbterm_scroll_track_click(&mut self, y: f64) {
        let (Some(track), Some(thumb)) = (self.lbterm_scroll_strip.track, self.lbterm_scroll_strip.thumb) else {
            return;
        };
        self.lbterm_scroll_to(y - track.y - thumb.h / 2.0);
    }

    /// The thumb dragged to `y`, where the pointer is
    fn lbterm_scroll_drag_to(&mut self, y: f64) {
        let (Some(track), Some((grab, _))) = (self.lbterm_scroll_strip.track, self.lbterm_scroll_strip.drag) else {
            return;
        };
        self.lbterm_scroll_strip.drag = Some((grab, y - grab));
        self.lbterm_scroll_to(y - grab - track.y);
    }

    /// Scroll so the thumb's top is `top` down the track: a shell's viewport at once, Claude
    /// Code's view by a seek to the words there
    fn lbterm_scroll_to(&mut self, top: f64) {
        let state = &self.lbterm_scroll_strip;
        let (Some(track), Some(thumb)) = (state.track, state.thumb) else {
            return;
        };
        let Some(pane) = state.pane.and_then(|id| Mux::get().get_pane(id)) else {
            return;
        };
        match state.mode {
            Mode::Shell => {
                let room = (track.h - thumb.h).max(0.0);
                let top = top.clamp(0.0, room) as usize;
                let min = self.min_scroll_bar_height() as usize;
                let viewport = self.get_viewport(pane.pane_id());
                let row = ScrollHit::thumb_top_to_scroll_top(top, &*pane, viewport, track.h as usize, min);
                let dims = pane.get_dimensions();
                self.set_viewport(pane.pane_id(), Some(row), dims);
            }
            Mode::Claude => {
                let (Some(snapshot), Some((_, layout))) = (state.snapshot.clone(), state.layout.clone()) else {
                    return;
                };
                let Some(rows) = state.screens.get(&pane.pane_id()).map(|s| s.area) else {
                    return;
                };
                let row = conv::row_at(&layout, rows, top, track.h, thumb.h);
                if let Some(point) = snapshot.point_at(&layout, row) {
                    self.lbterm_scroll_seek(pane.pane_id(), point);
                }
            }
            Mode::None => {}
        }
    }

    /// Scroll Claude Code in `pane` to `target`, a block and the line of it, or send a seek under
    /// way there instead
    fn lbterm_scroll_seek(&mut self, pane: PaneId, target: (usize, usize)) {
        if let Some(ctl) = self.lbterm_scroll_strip.seek.as_ref().filter(|c| c.pane == pane) {
            ctl.target.set(target);
            return;
        }
        let state = &mut self.lbterm_scroll_strip;
        let (Some(snapshot), Some((_, layout))) = (state.snapshot.clone(), state.layout.clone()) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let ctl = Rc::new(SeekCtl { pane, target: Cell::new(target), cancelled: Cell::new(false), gone: Cell::new(false) });
        state.seek = Some(Rc::clone(&ctl));
        state.note = None;
        let near = state.views.get(&pane).map(|v| v.top);
        promise::spawn::spawn(async move {
            let ended_for = Rc::clone(&ctl);
            let outcome = seek(pane, snapshot, layout, ctl, near).await;
            // The block it went to: a drag may have moved on since
            let ended_for = ended_for.target.get();
            let landed = match outcome {
                Sought::Landed(row) => Ok(row),
                Sought::NotThere => Err(Some("Couldn't find it in Claude Code's view")),
                Sought::Covered => Err(Some("Claude Code has a dialog open: answer it, then try again")),
                Sought::TooFar => Err(Some("Too far to scroll to")),
                Sought::Gone | Sought::Cancelled | Sought::Down => Err(None),
            };
            let down = matches!(outcome, Sought::Down);
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.lbterm_scroll_sought(pane, landed, ended_for);
                if down {
                    // Not sent, in the failed red, as a write that didn't get there is
                    tw.lbterm_scroll_write_failed(pane, wezterm_client::lbterm_link::Down.to_string(), false);
                }
            })));
        })
        .detach();
    }

    /// Lachlan pressed a key or turned the wheel in a pane: a seek under way there stops where
    /// it is, so it never fights him
    pub(crate) fn lbterm_scroll_cancel(&mut self) {
        if let Some(ctl) = self.lbterm_scroll_strip.seek.take() {
            ctl.cancelled.set(true);
        }
    }

    fn lbterm_scroll_sought(&mut self, pane: PaneId, landed: Result<usize, Option<&'static str>>, ended_for: (usize, usize)) {
        // A drag moved on after the seek had ended: on to where it is now
        let moved_on = self
            .lbterm_scroll_strip
            .seek
            .as_ref()
            .filter(|c| c.pane == pane && !c.cancelled.get() && !c.gone.get() && c.target.get() != ended_for)
            .map(|c| c.target.get());
        self.lbterm_scroll_strip.seek = None;
        if let Some(target) = moved_on {
            self.lbterm_scroll_strip.looked.remove(&pane);
            self.lbterm_scroll_seek(pane, target);
            return;
        }
        // Read the screen again for where it is now
        self.lbterm_scroll_strip.looked.remove(&pane);
        match landed {
            Ok(row) => self.lbterm_mark_landed_screen(pane, row),
            Err(Some(why)) => {
                let at = self.lbterm_scroll_strip.thumb.map_or(0.0, |t| t.y + t.h / 2.0);
                self.lbterm_scroll_note(why, at);
            }
            Err(None) => {}
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Say why a click didn't get there, on the card, level with `at`, for a few seconds
    fn lbterm_scroll_note(&mut self, why: &str, at: f64) {
        self.lbterm_scroll_noted(Note::not_there(why, at, Instant::now() + NOTE_FOR));
    }

    fn lbterm_scroll_noted(&mut self, note: Note) {
        let until = note.until;
        self.lbterm_scroll_strip.note = Some(note);
        self.lbterm_scroll_wake(Some(until));
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// The note the strip shows now: a write's only while its pane's tab is the active one, the
    /// tab asked of the mux only while there's such a note
    fn lbterm_scroll_shown_note(&self, now: Instant) -> Option<&Note> {
        let note = self.lbterm_scroll_strip.note.as_ref()?;
        let tab = note
            .tab
            .and_then(|_| Mux::get().get_active_tab_for_window(self.mux_window_id))
            .map(|tab| tab.tab_id() as u64);
        note.shows(tab, now).then_some(note)
    }

    /// A write to `pane` didn't get there (mux::lbterm_writes, from wezterm-client's
    /// lbterm_link.rs): a seek under way in it ends as gone, and the strip says why, in the
    /// failed red, on the strip of the pane's tab for as long as a click's note (Lachlan's 15b
    /// of 2026-11-07), whichever of its panes the strip stands by. A seek that found the pane's
    /// connection down says so here too (`Sought::Down`). A pane the mux no longer holds (its
    /// domain detached when the client gave up) has no strip to say it on: the log has it.
    /// `maybe`: the connection ended before the server answered, so it may have got there, and
    /// the note's head says so ("May not have arrived", Lachlan's 4a of 2026-11-08)
    pub(crate) fn lbterm_scroll_write_failed(&mut self, pane: PaneId, why: String, maybe: bool) {
        if let Some(ctl) = self.lbterm_scroll_strip.seek.as_ref().filter(|c| c.pane == pane) {
            ctl.gone.set(true);
        }
        let Some((_, _, tab)) = Mux::get().resolve_pane_id(pane) else {
            return;
        };
        let state = &self.lbterm_scroll_strip;
        let at = match (state.thumb, state.track) {
            (Some(t), _) => t.y + t.h / 2.0,
            (None, Some(t)) => t.y + t.h / 2.0,
            (None, None) => 0.0,
        };
        self.lbterm_scroll_noted(Note::not_sent(tab as u64, &why, maybe, at, Instant::now() + NOTE_FOR));
    }

    /// Repaint at `at`: when the card's wait is up, a note goes or a fade ends
    pub(crate) fn lbterm_scroll_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        if matches!(self.lbterm_scroll_strip.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_scroll_strip.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_scroll_strip.wake == Some(at) {
                    tw.lbterm_scroll_strip.wake = None;
                }
                tw.lbterm_scroll_strip.hover.tick(Instant::now());
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// What the strip shows, for the state file (lbterm/STATE.md); `None` with it off
    pub(crate) fn lbterm_state_scroll_strip(&self) -> Option<lbterm_extras::state::ScrollStrip> {
        use lbterm_extras::state as S;
        let at = |r: &Rect| S::Rect::round(r.x, r.y, r.w, r.h);
        let state = &self.lbterm_scroll_strip;
        let strip = state.strip?;
        let mut marks: Vec<&Drawn> = state.marks.iter().collect();
        marks.sort_by(|a, b| a.middle.total_cmp(&b.middle));
        Some(S::ScrollStrip {
            strip: at(&strip),
            track: at(&state.track.unwrap_or(strip)),
            mode: state.mode.name().to_string(),
            pane: state.pane.map(|p| p as u64),
            thumb: state.thumb.as_ref().map(at),
            marks: marks
                .into_iter()
                .map(|m| S::ScrollMark { kind: m.kind.name().to_string(), rect: at(&m.rect) })
                .collect(),
            rows: state.rows,
            view_top: state.view_top,
            card: state.card.as_ref().map(|c| S::ScrollCard {
                rect: at(&c.rect),
                lines: c.lines.iter().map(|l| l.text.clone()).collect(),
            }),
            seeking: state.seek.is_some(),
            note: self.lbterm_scroll_shown_note(Instant::now()).map(|n| n.text.clone()),
            note_head: self.lbterm_scroll_shown_note(Instant::now()).map(|n| n.why.head().to_string()),
            looks: state.looks,
        })
    }

    /// What of the strip is at `x`, `y`, for the state file's pointer: its card, a mark, the
    /// thumb or the strip
    pub(crate) fn lbterm_scroll_strip_under(&self, x: f64, y: f64) -> Option<&'static str> {
        let state = &self.lbterm_scroll_strip;
        if state.card.as_ref().map_or(false, |c| c.rect.contains(x, y)) {
            return Some("scroll.card");
        }
        let strip = state.strip?;
        let gutter = self.lbterm_shape().gutter() as f64;
        if !(x >= strip.x - gutter && x < strip.x + strip.w && y >= strip.y && y < strip.y + strip.h) {
            return None;
        }
        if !self.lbterm_scroll_marks_at(y).is_empty() {
            return Some("scroll.mark");
        }
        if state.thumb.map_or(false, |t| y >= t.y && y < t.y + t.h) {
            return Some("scroll.thumb");
        }
        Some("scroll.strip")
    }

    /// The card's waits under way at `now`: its rest before it opens and grace before it closes
    pub(crate) fn lbterm_scroll_strip_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let hover = &self.lbterm_scroll_strip.hover;
        let name = match hover.phase() {
            lbterm_panel::Phase::Opening => Some("scroll.open"),
            lbterm_panel::Phase::Closing => Some("scroll.close"),
            _ => None,
        };
        name.zip(hover.deadline())
            .map(|(name, due)| lbterm_extras::state::Wait {
                name: name.to_string(),
                left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
            })
            .into_iter()
            .collect()
    }
}
