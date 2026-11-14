//! LBTerm: the Conductor's view (lbterm/TAB-PANEL.md, "The Conductor's view"; Lachlan's design,
//! 2026-10-24): the Conductor runs in the background in its seat (Conductor's SEAT.md), and LBTerm
//! draws its conversation and everything around it in the window's tab area. A left click on the
//! Conductor's pad opens it (`config.lbterm_conductor.seat_pipe` set), and so does the seat's
//! `seat.show` and `LbtermConductorView`. While it's open it stands over the tab that was open, the
//! pad carrying the open tab's ring and bar; a click on a tab's row, a tab opened any other way,
//! or Esc with the box empty leaves it.
//!
//! Where everything goes is lbterm_extras::seat::layout's; the seat's lines are crate::seat_client's.
//! This draws the layout's rectangles and words into z-layers 12 to 15, which go to the
//! compositor's `over` visual (over the panes, under the tab panel), takes the mouse and the keys
//! while it's open, and sends the seat what's pressed and typed.
//!
//! At rest it costs nothing (Lachlan, 2026-10-29: about 80% of a core while it was open, the
//! panes under it repainting the window and the view laid out whole on every frame). The scene is
//! kept, and laid out again only when something it's made from changes ([`LaidFrom`]: the room,
//! the DPI, the fonts, the seat's word; the UI's state, marked `dirty` by whatever changes it; the
//! clock coming round, which wakes it once for that); its words are placed once a scene; Windows
//! is handed the caret only when it moves; and while it's open the panes under it don't repaint
//! the window (termwindow/mod.rs, `mux_pane_output_event`), since nothing of them shows. Closed,
//! it costs nothing either: what the seat says repaints only the windows it's open in
//! (seat_client.rs), and the pad's asks are the state file's.

use crate::seat_client;
use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator};
use crate::termwindow::box_model::{BorderColor, ComputedElement, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::compositor::Light;
use crate::termwindow::{TermWindow, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::{Clipboard, CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::keyassignment::ClipboardCopyDestination;
use config::lbterm::LbtermConductorView;
use config::{DimensionContext, FontAttributes, FontWeight, TextStyle};
use lbterm_extras::seat::input;
use lbterm_extras::seat::layout::{self, BoxKey, Font, Layer, Metrics, Model, Prim, Scene, Target, Ui};
use lbterm_extras::seat::protocol::{Command, Reply, Waiting};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Area, Side};
use mux::tab::TabId;
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use wezterm_font::LoadedFont;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// The view's z-layers: over the panes (0) and the fancy tab bar (10, 11), under everything else
/// LBTerm draws: the tab panel (45 and up) and the cards that drop over the panes (the strip's,
/// the objectives panel's, the tasks card) included. In the compositor's `over`, which
/// composited.rs gives them before any other owner. 38 to 41 were the view's until the
/// objectives panel took 38 to 44 for its strip and its cards' own visuals, which hid every
/// word and button the view drew in 39 to 41.
const BACK_Z: i8 = 12;
const MID_Z: i8 = 13;
const TEXT_Z: i8 = 14;
const TOP_Z: i8 = 15;

/// Whether a z-layer is the view's: the compositor draws it into `over` and nowhere else
pub fn owns_zindex(zindex: i8) -> bool {
    (BACK_Z..=TOP_Z).contains(&zindex)
}

/// The marker's key while the view is open: no tab's
pub const VIEW_KEY: TabId = TabId::MAX;

/// How long what the seat answered stays under the box: a refusal longer
const NOTICE_FOR: Duration = Duration::from_secs(6);
const REFUSAL_FOR: Duration = Duration::from_secs(12);

/// A command sent from a button: its id, the button's name (showing "…" until the reply), and
/// a message's words, put back in the box if the seat refuses them
struct Sent {
    id: String,
    button: String,
    words: Option<String>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct WordsKey {
    font: Font,
    text: String,
    rgb: u32,
    z: i8,
}

/// How late after the clock comes round its repaint is asked for, so the frame finds it come
const CLOCK_LATE: Duration = Duration::from_millis(5);

/// The layers the scene's fills and outlines go in (its words are drawn as elements)
const SHAPE_LAYERS: [Layer; 3] = [Layer::Back, Layer::Mid, Layer::Top];

/// What the scene was laid out from, besides the UI's state and the clock: laid out again when
/// any of it changes
#[derive(Debug, Clone, Copy, PartialEq)]
struct LaidFrom {
    room: Rect,
    dpi: usize,
    rail: f64,
    surface: u32,
    /// [`ViewState::measure_generation`]
    fonts: u64,
    /// `seat_client::generation`: the seat's view, the conversation, its replies, whether it answers
    seat: u64,
}

#[derive(Default)]
pub struct ViewState {
    open: bool,
    /// The mux window it was opened in, as the seat's client has it: what the seat says repaints
    /// this window while it's open (seat_client.rs, `view_opened`)
    listed: Option<MuxWindowId>,
    /// The frame last presented showed the view over the panes (render/composited.rs): what was
    /// under it never showed, so it goes at once when a tab opened from it takes its place
    presented: bool,
    /// The tab that was open as it opened: it stands over it, and a tab opened any other way
    /// leaves it
    tab: Option<TabId>,
    ui: Ui,
    scene: Option<Scene>,
    /// What `scene` was laid out from
    laid: Option<LaidFrom>,
    /// The UI's state changed since the scene was laid out (typed, scrolled, picked, hovered)
    dirty: bool,
    /// When what the scene says about time next changes, epoch milliseconds: laid out again then
    clock: Option<u64>,
    /// The clock's repaint asked for, by when it's for
    clock_wake: Option<u64>,
    /// The scene's words placed where they go, for the shape generation they were made in
    placed: Option<(usize, Vec<ComputedElement>)>,
    /// The scene's fills and outlines as quads, one allocator a layer of `SHAPE_LAYERS`, for the
    /// shape generation, quad generation and window size they were made at
    shapes: Option<((usize, usize, usize, usize), [HeapQuadAllocator; 3])>,
    sent: Vec<Sent>,
    /// A drag selecting the conversation's words is under way
    selecting: bool,
    /// The notice under the box goes then
    notice_until: Option<Instant>,
    /// Words laid out, for the shape generation they were laid out in
    words: HashMap<WordsKey, ComputedElement>,
    words_generation: usize,
    /// The view's fonts as last laid out, and the widths measured in them
    measure: Rc<Measure>,
    /// Goes up each time `measure` is replaced
    measure_generation: u64,
    /// The shape generation and DPI the fonts were last looked up at
    measured: Option<(usize, usize)>,
    /// When a repaint is already on its way
    wake: Option<Instant>,
}

impl ViewState {
    /// The view closed: nothing laid out is kept for it, and its clock asks for no repaint
    fn forget_scene(&mut self) {
        self.scene = None;
        self.laid = None;
        self.placed = None;
        self.shapes = None;
        self.clock = None;
        self.clock_wake = None;
        self.dirty = true;
    }
}

/// The font and size (in points) each of the view's type roles is drawn in: Roboto, which LBTerm
/// carries, in Material's roles (Lachlan, 2026-10-28: "like something Google would've
/// released"), and code in the panes' font
fn view_font(font: Font, config: &config::ConfigHandle) -> (TextStyle, f64) {
    let roboto = |weight: FontWeight| TextStyle {
        font: vec![FontAttributes {
            weight,
            ..FontAttributes::new("Roboto")
        }],
        foreground: None,
    };
    match font {
        Font::Text => (roboto(FontWeight::REGULAR), 10.5),
        Font::Strong => (roboto(FontWeight::MEDIUM), 10.5),
        Font::Small => (roboto(FontWeight::REGULAR), 9.0),
        Font::SmallStrong => (roboto(FontWeight::MEDIUM), 9.0),
        Font::Title => (roboto(FontWeight::MEDIUM), 12.0),
        Font::Code => (config.font.clone(), 9.0),
    }
}

/// Characters below this have their widths in a table, one row a font; the rest in a map
const TABLE: usize = 0x500;

/// The view's fonts, and each character's width in each as the shaper gives it, measured the
/// first time it's needed. A layout asks for a width tens of thousands of times, so the
/// characters below 0x500 (Latin, Greek, Cyrillic) are a table lookup, not a hash
#[derive(Default)]
pub struct Measure {
    fonts: HashMap<Font, (Rc<LoadedFont>, RenderMetrics)>,
    /// `Font::ALL.len()` rows of `TABLE`, NaN where not measured yet; empty until the first
    table: RefCell<Vec<f64>>,
    widths: RefCell<HashMap<(Font, char), f64>>,
}

impl Measure {
    /// Whether these are the fonts it measured in
    fn same(&self, fonts: &HashMap<Font, (Rc<LoadedFont>, RenderMetrics)>) -> bool {
        self.fonts.len() == fonts.len() && fonts.iter().all(|(f, (l, _))| self.fonts.get(f).map_or(false, |(k, _)| k.id() == l.id()))
    }

    /// `c`'s width in `font`, in pixels
    fn width(&self, font: Font, c: char) -> f64 {
        let i = c as usize;
        if i < TABLE {
            let slot = font as usize * TABLE + i;
            if let Some(w) = self.table.borrow().get(slot).copied().filter(|w| !w.is_nan()) {
                return w;
            }
            let w = self.shaped_width(font, c);
            let mut table = self.table.borrow_mut();
            if table.is_empty() {
                table.resize(Font::ALL.len() * TABLE, f64::NAN);
            }
            table[slot] = w;
            return w;
        }
        if let Some(w) = self.widths.borrow().get(&(font, c)) {
            return *w;
        }
        let w = self.shaped_width(font, c);
        self.widths.borrow_mut().insert((font, c), w);
        w
    }

    /// `c`'s width in `font` as the shaper gives it
    fn shaped_width(&self, font: Font, c: char) -> f64 {
        let Some((loaded, metrics)) = self.fonts.get(&font) else {
            return 0.0;
        };
        match loaded.blocking_shape(&c.to_string(), None, wezterm_bidi::Direction::LeftToRight, None, None) {
            Ok(glyphs) => glyphs.iter().map(|g| g.x_advance.get()).sum(),
            Err(err) => {
                log::error!("conductor view: measuring {c:?} in {font:?}: {err:#}");
                metrics.cell_size.width as f64
            }
        }
    }

    /// How tall `font`'s words are as box_model lays them out
    fn height(&self, font: Font) -> f64 {
        self.fonts.get(&font).map_or(0.0, |(_, m)| m.cell_size.height as f64)
    }

    /// A character's width in units of the boxes' font, as the layout counts them
    fn box_units(&self, c: char) -> usize {
        (self.width(Font::Text, c) * layout::UNIT).round().max(0.0) as usize
    }
}

fn srgb(rgb: u32) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, 255)
}

fn area(r: Rect) -> Area {
    Area::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Epoch milliseconds as the time of day here, "14:02"
fn clock(ms: u64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// The next midnight here after epoch milliseconds `ms`, when the conversation's days change
/// their names ("Today" to "Yesterday"); none where midnight doesn't come that day (a clock
/// change over it)
fn next_midnight(ms: u64) -> Option<u64> {
    use chrono::TimeZone;
    let today = chrono::Local.timestamp_millis_opt(ms as i64).single()?.date_naive();
    let midnight = today.succ_opt()?.and_hms_opt(0, 0, 0)?;
    let at = chrono::Local.from_local_datetime(&midnight).earliest()?;
    let ms = at.timestamp_millis();
    (ms >= 0).then(|| ms as u64)
}

/// The open suggestions' ids, in the order the view shows them
fn open_suggestions(view: Option<&lbterm_extras::seat::protocol::View>) -> Vec<String> {
    view.map_or(vec![], |v| v.suggestions.iter().filter(|s| s.status == "open").map(|s| s.id.clone()).collect())
}

/// The day epoch milliseconds `ms` fall on here, as he'd say it at `now`: "Today", "Yesterday",
/// "Monday" within the week, else "Monday 19 October"
fn day(ms: u64, now: u64) -> String {
    use chrono::TimeZone;
    let local = |ms: u64| chrono::Local.timestamp_millis_opt(ms as i64).single();
    let (Some(t), Some(n)) = (local(ms), local(now)) else {
        return String::new();
    };
    let (d, today) = (t.date_naive(), n.date_naive());
    match (today - d).num_days() {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        2..=6 => t.format("%A").to_string(),
        _ => t.format("%A %-d %B").to_string(),
    }
}

impl TermWindow {
    /// Whether the view can open: the config names a seat
    pub(crate) fn lbterm_view_on(&self) -> bool {
        self.config
            .lbterm_conductor
            .as_ref()
            .and_then(|c| c.seat_pipe.as_deref())
            .map_or(false, |p| !p.is_empty())
    }

    pub(crate) fn lbterm_view_is_open(&self) -> bool {
        self.lbterm_view.open
    }

    /// Whether the view stood over the panes in the frame last presented, or stands over them in
    /// this one: their visuals showed nothing then, so panes that give way to others under it go
    /// at once rather than dissolving. A tab opened from the view (a session's card, a row, a key,
    /// the cli) would otherwise bring back the tab under it at full for a frame and fade it out,
    /// where it never showed (the review of 2026-10-30); a dissolve under the view, as the last
    /// tab gives way to a new one with the view over it, shows nothing either
    pub(crate) fn lbterm_view_covers_panes(&self) -> bool {
        self.lbterm_view.presented || self.lbterm_view.open
    }

    /// The frame is presented: whether it showed the view (the paint pass closed it if a tab
    /// opened from it, before the frame was drawn)
    pub(crate) fn lbterm_view_presented(&mut self) {
        self.lbterm_view.presented = self.lbterm_view.open;
    }

    fn lbterm_active_tab_id(&self) -> Option<TabId> {
        Mux::get()
            .get_window(self.mux_window_id)
            .and_then(|w| w.get_active_tab().map(|t| t.tab_id()))
    }

    /// Open the view over the tab that's open, its box taking the typing
    pub(crate) fn lbterm_view_open(&mut self) {
        if !self.lbterm_view_on() {
            log::info!("conductor view: no seat_pipe in the config");
            return;
        }
        if !self.lbterm_view.open {
            log::info!("conductor view: open");
        }
        self.lbterm_view.open = true;
        self.lbterm_view.tab = self.lbterm_active_tab_id();
        self.lbterm_view.ui.focus = BoxKey::Main;
        // What the seat says repaints this window from now on; the connection started if it hasn't
        if self.lbterm_view.listed.is_none() {
            seat_client::view_opened(self.mux_window_id);
            self.lbterm_view.listed = Some(self.mux_window_id);
        }
        seat_client::listen();
        self.lbterm_view_changed();
    }

    /// Leave the view for the tab under it
    pub(crate) fn lbterm_view_close(&mut self) {
        if self.lbterm_view.open {
            log::info!("conductor view: closed");
        }
        let view = &mut self.lbterm_view;
        view.open = false;
        if let Some(window) = view.listed.take() {
            seat_client::view_closed(window);
        }
        view.selecting = false;
        view.ui.selection = None;
        view.ui.hovered = None;
        view.forget_scene();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Something the view shows changed in what this window keeps of it (what's typed, scrolled,
    /// picked, under the pointer, waiting on the seat): it's laid out again at the next paint,
    /// which this asks for
    fn lbterm_view_changed(&mut self) {
        self.lbterm_view.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// `LbtermConductorView`
    pub(crate) fn lbterm_view_action(&mut self, mode: LbtermConductorView) {
        match mode {
            LbtermConductorView::Show => self.lbterm_view_open(),
            LbtermConductorView::Hide => self.lbterm_view_close(),
            LbtermConductorView::Toggle if self.lbterm_view.open => self.lbterm_view_close(),
            LbtermConductorView::Toggle => self.lbterm_view_open(),
        }
    }

    /// The window strip's words while the view is open: what the Conductor is doing, shortly
    pub(crate) fn lbterm_view_strip_words(&self) -> Option<String> {
        let seat = seat_client::seat();
        let seat = seat.as_ref()?;
        if !seat.connected {
            return Some("the seat isn't running".to_string());
        }
        let view = seat.view.as_ref()?;
        let c = &view.conductor;
        let mut words = vec![c.state.clone()];
        if c.state == "on" {
            words.push(
                match c.mode.as_str() {
                    "most" => "accept most",
                    "all" => "accept all",
                    _ => "read only",
                }
                .to_string(),
            );
        }
        if c.busy {
            words.push("working".to_string());
        }
        if !view.asks.is_empty() {
            words.push(format!("{} ask{}", view.asks.len(), if view.asks.len() == 1 { "" } else { "s" }));
        }
        if !view.waiting.is_empty() {
            words.push(format!("{} waiting on you", view.waiting.len()));
        }
        Some(words.join(" · "))
    }

    /// How many asks wait on him, as the pad's state file says (`asks`): the seat writes it as each
    /// comes and goes (Conductor's SEAT.md, "Lifecycle"), and the file is watched, so the pad
    /// needs nothing of what the seat sends, and a closed view nothing of the seat at all. Until
    /// 2026-10-30 it took the seat's word while the seat answered, and the file's otherwise.
    pub(crate) fn lbterm_conductor_asks(&self) -> usize {
        self.lbterm_conductor_said()
            .and_then(|said| said.get("asks").and_then(|n| n.parse::<usize>().ok()))
            .unwrap_or(0)
    }

    /// Repaint at `at`: a notice going
    fn lbterm_view_wake(&mut self, at: Instant) {
        if matches!(self.lbterm_view.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_view.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_view.wake == Some(at) {
                    tw.lbterm_view.wake = None;
                }
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    fn lbterm_view_notice(&mut self, words: String, refusal: bool) {
        let until = Instant::now() + if refusal { REFUSAL_FOR } else { NOTICE_FOR };
        self.lbterm_view.ui.notice = Some((words, refusal));
        self.lbterm_view.notice_until = Some(until);
        self.lbterm_view_wake(until);
        // Laid out again by the paint under way (a reply) or the one the press or key asks for
        self.lbterm_view.dirty = true;
    }

    /// A repaint when the view's clock comes round (epoch milliseconds `at`), the one timer for
    /// it: a clock that moves on before it's due leaves the old timer doing nothing
    fn lbterm_view_clock_wake(&mut self, at: u64) {
        if self.lbterm_view.clock_wake == Some(at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_view.clock_wake = Some(at);
        let due = Instant::now() + Duration::from_millis(at.saturating_sub(now_ms())) + CLOCK_LATE;
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(due)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_view.clock_wake == Some(at) {
                    tw.lbterm_view.clock_wake = None;
                    if let Some(window) = tw.window.as_ref() {
                        window.invalidate();
                    }
                }
            })));
        })
        .detach();
    }

    /// Send the seat `command` for the button `button`, which says "…" until it answers
    fn lbterm_view_send(&mut self, command: Command, button: &str, words: Option<String>) {
        let id = seat_client::send(command);
        self.lbterm_view.ui.pending.insert(button.to_string());
        self.lbterm_view.sent.push(Sent {
            id,
            button: button.to_string(),
            words,
        });
        self.lbterm_view_changed();
    }

    /// The seat's answers to what was sent: a button stops waiting, a refusal is said under
    /// the box, and a message it refused goes back in the box when that's empty. Every command
    /// gets one, the seat's or, once its wait is over or the seat has gone, seat_client's
    fn lbterm_view_replies(&mut self) {
        let sent = std::mem::take(&mut self.lbterm_view.sent);
        let mut left = vec![];
        let mut answered: Vec<(Sent, Reply)> = vec![];
        for s in sent {
            match seat_client::reply(&s.id) {
                Some(reply) => answered.push((s, reply)),
                None => left.push(s),
            }
        }
        self.lbterm_view.sent = left;
        if !answered.is_empty() {
            self.lbterm_view.dirty = true;
        }
        for (s, reply) in answered {
            self.lbterm_view.ui.pending.remove(&s.button);
            if !reply.ok {
                if let Some(words) = s.words.filter(|_| self.lbterm_view.ui.main.is_empty()) {
                    self.lbterm_view.ui.main.set(&words);
                }
            }
            if let Some(words) = reply.words() {
                self.lbterm_view_notice(words.to_string(), !reply.ok);
            }
        }
        if let Some(until) = self.lbterm_view.notice_until {
            if Instant::now() >= until {
                self.lbterm_view.ui.notice = None;
                self.lbterm_view.notice_until = None;
                self.lbterm_view.dirty = true;
            }
        }
    }

    /// The view's room: the panes', a gutter in from it, as a pane's card
    fn lbterm_view_area(&self) -> anyhow::Result<Rect> {
        let room = self.lbterm_room()?;
        let card = self.lbterm_shape().card(
            Side::Outer(room.left),
            Side::Outer(room.top),
            Side::Outer(room.right),
            Side::Outer(room.bottom),
        );
        Ok(Rect {
            x: card.x as f64,
            y: card.y as f64,
            w: card.w as f64,
            h: card.h as f64,
        })
    }

    /// The view's fonts at this window's DPI, looked up again when the shape generation or the DPI
    /// changes (a config reload, a scale change, a new atlas), and measured again when any of them
    /// is another font
    fn lbterm_view_measure(&mut self) -> anyhow::Result<Rc<Measure>> {
        let at = (self.shape_generation, self.dimensions.dpi);
        if self.lbterm_view.measured == Some(at) {
            return Ok(Rc::clone(&self.lbterm_view.measure));
        }
        let mut fonts = HashMap::new();
        for font in Font::ALL {
            let (style, size) = view_font(font, &self.config);
            let loaded = self.fonts.lbterm_sized_font(&style, size)?;
            let metrics = RenderMetrics::with_font_metrics(&loaded.metrics());
            fonts.insert(font, (loaded, metrics));
        }
        if !self.lbterm_view.measure.same(&fonts) {
            self.lbterm_view.measure = Rc::new(Measure {
                fonts,
                ..Default::default()
            });
            self.lbterm_view.measure_generation += 1;
            // What was wrapped in the old fonts is the wrong width in these
            self.lbterm_view.ui.forget_measures();
        }
        self.lbterm_view.measured = Some(at);
        Ok(Rc::clone(&self.lbterm_view.measure))
    }

    /// Draw the view, if it's open: called by the paint pass after the panes, before the panel.
    /// The scene is laid out again only when what it's made from has changed since it was.
    pub(crate) fn paint_lbterm_conductor_view(&mut self) -> anyhow::Result<()> {
        if !self.lbterm_view.open {
            if self.lbterm_view.scene.is_some() {
                self.lbterm_view.forget_scene();
            }
            return Ok(());
        }
        if !self.lbterm_view_on() {
            self.lbterm_view_close();
            return Ok(());
        }
        // A tab opened any other way (a key, a program, a new tab) leaves the view for it
        let active = self.lbterm_active_tab_id();
        if active != self.lbterm_view.tab {
            log::info!("conductor view: tab {active:?} opened, leaving the view");
            self.lbterm_view_close();
            return Ok(());
        }
        self.lbterm_view_replies();
        // The notice's end, asked again each paint: a notice that came while an earlier one's
        // repaint was still on its way got none of its own (`lbterm_view_wake` keeps the
        // earliest), so it stayed until something else painted
        if let Some(until) = self.lbterm_view.notice_until {
            self.lbterm_view_wake(until);
        }

        let room = self.lbterm_view_area()?;
        let measure = self.lbterm_view_measure()?;
        let surface = {
            let (r, g, b, _) = self.palette().background.to_srgb_u8();
            ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        };
        let composing = match &self.dead_key_status {
            ::window::DeadKeyStatus::Composing(s) => Some(s.as_str()),
            ::window::DeadKeyStatus::None => None,
        };
        if composing != self.lbterm_view.ui.composing.as_deref() {
            self.lbterm_view.ui.composing = composing.map(str::to_string);
            self.lbterm_view.dirty = true;
        }
        // The tab rail is the window's left border: the side column folds to its width
        let rail = self.get_os_border().left.get() as f64;
        let from = LaidFrom {
            room,
            dpi: self.dimensions.dpi,
            rail,
            surface,
            fonts: self.lbterm_view.measure_generation,
            seat: seat_client::generation(),
        };
        let now = now_ms();
        let view = &self.lbterm_view;
        let stale = view.scene.is_none() || view.dirty || view.laid != Some(from) || view.clock.map_or(false, |at| now >= at);
        if stale {
            self.lbterm_view_lay_out(room, &measure, surface, rail, now);
            self.lbterm_view.laid = Some(from);
            self.lbterm_view.dirty = false;
        }
        if let Some(at) = self.lbterm_view.clock {
            self.lbterm_view_clock_wake(at);
        }

        // Drawn from the scene kept, put back however the drawing went
        let scene = self.lbterm_view.scene.take();
        let drawn = match scene.as_ref() {
            Some(scene) => self.lbterm_view_draw(scene, &measure),
            None => Ok(()),
        };
        self.lbterm_view.scene = scene;
        drawn
    }

    /// The scene laid out afresh from what the seat has said, at `now` (epoch milliseconds), its
    /// clock set to when it next changes by itself, and its words to be placed again
    fn lbterm_view_lay_out(&mut self, room: Rect, measure: &Rc<Measure>, surface: u32, rail: f64, now: u64) {
        let width = |font: Font, c: char| measure.width(font, c);
        let height = |font: Font| measure.height(font);
        let metrics = Metrics {
            scale: self.dimensions.dpi as f64 / 96.0,
            width: &width,
            height: &height,
            rail,
        };
        let pipe = seat_client::pipe().unwrap_or_default();
        let mut scene = {
            let seat = seat_client::seat();
            let (connected, view, shown) = match seat.as_ref() {
                Some(s) => (s.connected, s.view.as_ref(), s.conversation.shown()),
                None => (false, None, vec![]),
            };
            if let Some(view) = view {
                self.lbterm_view.ui.forget_gone(view);
            }
            let model = Model {
                connected,
                pipe: &pipe,
                view,
                items: &shown,
                now_ms: now,
                clock,
                day,
            };
            layout::lay_out_settled(room, &metrics, surface, &model, &mut self.lbterm_view.ui)
        };
        // Its days' names change at midnight as well as its ages when the layout says
        let clock = scene.clock.into_iter().chain(next_midnight(now)).min();
        scene.said.clock = clock;
        self.lbterm_view.clock = clock;
        self.lbterm_view.scene = Some(scene);
        self.lbterm_view.placed = None;
        self.lbterm_view.shapes = None;
    }

    /// The scene's fills, outlines and words into the view's z-layers, and the caret to Windows
    /// when it has moved
    fn lbterm_view_draw(&mut self, scene: &Scene, measure: &Measure) -> anyhow::Result<()> {
        if self.lbterm_view.words_generation != self.shape_generation || self.lbterm_view.words.len() > 3000 {
            self.lbterm_view.words.clear();
            self.lbterm_view.words_generation = self.shape_generation;
        }
        let gl_state = self.render_state.as_ref().unwrap();
        let z = |layer: Layer| match layer {
            Layer::Back => BACK_Z,
            Layer::Mid => MID_Z,
            Layer::Text => TEXT_Z,
            Layer::Top => TOP_Z,
        };
        // The fills and outlines, made into quads once a scene (each corner a fan of 16, an
        // outline three bands of them: a real conversation's 250 took 2.7 ms to make, 1.2 ms to
        // copy) and copied into the layers each frame; made again for a new window size (the
        // quads are in its coordinates) or shape generation
        let shapes_at = (self.shape_generation, self.quad_generation, self.dimensions.pixel_width, self.dimensions.pixel_height);
        if !matches!(&self.lbterm_view.shapes, Some((at, _)) if *at == shapes_at) {
            let mut shapes: [HeapQuadAllocator; 3] = Default::default();
            for (layer, heap) in SHAPE_LAYERS.iter().copied().zip(shapes.iter_mut()) {
                let mut quads = TripleLayerQuadAllocator::Heap(heap);
                for prim in &scene.prims {
                    match prim {
                        Prim::Fill { rect, radius, rgb, layer: l } if *l == layer => {
                            if *radius >= 1.0 {
                                self.lbterm_fill_rounded(&mut quads, area(*rect), *radius as f32, srgb(*rgb), None)?;
                            } else {
                                self.filled_rectangle(
                                    &mut quads,
                                    0,
                                    euclid::rect(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32),
                                    srgb(*rgb),
                                )?;
                            }
                        }
                        Prim::Outline { rect, radius, rgb, layer: l } if *l == layer => {
                            let shape = self.lbterm_shape();
                            let rgb = *rgb;
                            self.lbterm_stroke(&mut quads, area(*rect), *radius as f32, shape.hairline(), shape.feather(), |_| rgb)?;
                        }
                        _ => {}
                    }
                }
            }
            self.lbterm_view.shapes = Some((shapes_at, shapes));
        }
        if let Some((_, shapes)) = &self.lbterm_view.shapes {
            for (layer, heap) in SHAPE_LAYERS.iter().copied().zip(shapes.iter()) {
                let layer_z = gl_state.layer_for_zindex(z(layer))?;
                heap.apply_to(&mut layer_z.quad_allocator())?;
            }
        }

        // The words, each laid out once and placed once a scene, where it goes
        if !matches!(&self.lbterm_view.placed, Some((generation, _)) if *generation == self.shape_generation) {
            let mut placed = vec![];
            for prim in &scene.prims {
                let Prim::Text {
                    x,
                    y,
                    w,
                    text,
                    font,
                    rgb,
                    layer,
                } = prim
                else {
                    continue;
                };
                let key = WordsKey {
                    font: *font,
                    text: text.clone(),
                    rgb: *rgb,
                    z: z(*layer),
                };
                if !self.lbterm_view.words.contains_key(&key) {
                    let Some((loaded, m)) = measure.fonts.get(font) else {
                        continue;
                    };
                    let element = Element::new(loaded, ElementContent::Text(text.clone())).colors(ElementColors {
                        border: BorderColor::default(),
                        bg: LinearRgba::TRANSPARENT.into(),
                        text: srgb(*rgb).into(),
                    });
                    let computed = self.compute_element(
                        &LayoutContext {
                            width: DimensionContext {
                                dpi: self.dimensions.dpi as f32,
                                pixel_max: f32::MAX / 4.0,
                                pixel_cell: m.cell_size.width as f32,
                            },
                            height: DimensionContext {
                                dpi: self.dimensions.dpi as f32,
                                pixel_max: self.dimensions.pixel_height as f32,
                                pixel_cell: m.cell_size.height as f32,
                            },
                            bounds: euclid::rect(0.0, 0.0, (*w as f32).max(1.0) + 64.0, m.cell_size.height as f32),
                            metrics: m,
                            gl_state,
                            zindex: z(*layer),
                        },
                        &element,
                    )?;
                    self.lbterm_view.words.insert(key.clone(), computed);
                }
                if let Some(computed) = self.lbterm_view.words.get(&key) {
                    let mut at = computed.clone();
                    at.translate(euclid::vec2(*x as f32, *y as f32));
                    placed.push(at);
                }
            }
            self.lbterm_view.placed = Some((self.shape_generation, placed));
        }
        if let Some((_, placed)) = &self.lbterm_view.placed {
            for computed in placed {
                self.render_element(computed, gl_state, None)?;
            }
        }

        // The IME's window goes by the caret of the box typing goes to, told when it moves
        let caret = scene.caret.map(|caret| {
            ::window::Rect::new(
                ::window::Point::new(caret.x as isize, caret.y as isize),
                self.render_metrics.cell_size,
            )
        });
        // (the panes' caret is told through the same, so either is told again after the other:
        // termwindow/mod.rs's lbterm_ime)
        if let (Some(caret), Some(window)) = (caret, self.window.as_ref()) {
            if self.lbterm_ime.tell((caret, self.focused.is_some())) {
                window.set_text_cursor_position(caret);
            }
        }
        Ok(())
    }

    /// While it's open, the view takes the mouse over it, and a drag selecting its words wherever
    /// it goes: true when it did
    pub(crate) fn lbterm_view_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if !self.lbterm_view.open {
            return false;
        }
        let Some(scene) = self.lbterm_view.scene.as_ref() else {
            return false;
        };
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        if self.lbterm_view.selecting {
            match event.kind {
                WMEK::Move => {
                    if let (Some(spot), Some((a, _))) = (scene.spot(x, y), self.lbterm_view.ui.selection) {
                        self.lbterm_view.ui.selection = Some((a, spot));
                        // Past the conversation's edge, it scrolls on
                        let conv = scene.conversation;
                        if y < conv.y {
                            self.lbterm_view.ui.scroll += (conv.y - y).min(40.0);
                        } else if y > conv.y + conv.h {
                            self.lbterm_view.ui.scroll = (self.lbterm_view.ui.scroll - (y - conv.y - conv.h).min(40.0)).max(0.0);
                        }
                        self.lbterm_view.dirty = true;
                        context.invalidate();
                    }
                }
                WMEK::Release(MousePress::Left) => {
                    self.lbterm_view.selecting = false;
                    self.lbterm_view_copy_selection(false);
                    // A click that selected nothing lets go of the selection
                    self.lbterm_view.dirty = true;
                    context.invalidate();
                }
                _ => {}
            }
            return true;
        }
        if !scene.card.contains(x, y) {
            if self.lbterm_view.ui.hovered.take().is_some() {
                self.lbterm_view.dirty = true;
                context.invalidate();
            }
            // The gutter round it is still over the panes under it: nothing reaches them
            let gutter = self.lbterm_shape().gutter() as f64 + 1.0;
            let room = Rect {
                x: scene.card.x - gutter,
                y: scene.card.y - gutter,
                w: scene.card.w + 2.0 * gutter,
                h: scene.card.h + 2.0 * gutter,
            };
            if room.contains(x, y) {
                context.set_cursor(Some(CursorIcon::Default));
                return true;
            }
            return false;
        }
        let target = scene.hit(x, y).cloned();
        match event.kind {
            WMEK::Move => {
                let hovered = target.clone().filter(Target::is_button);
                if hovered != self.lbterm_view.ui.hovered {
                    self.lbterm_view.ui.hovered = hovered;
                    self.lbterm_view.dirty = true;
                    context.invalidate();
                }
                context.set_cursor(Some(match &target {
                    Some(t) if t.is_button() => CursorIcon::Pointer,
                    Some(Target::Box(_)) => CursorIcon::Text,
                    Some(Target::Conversation) if scene.spot(x, y).is_some() => CursorIcon::Text,
                    _ => CursorIcon::Default,
                }));
            }
            WMEK::Press(MousePress::Left) => {
                let shift = event.modifiers.contains(::window::Modifiers::SHIFT);
                match target {
                    Some(Target::Conversation) => {
                        if let Some(spot) = scene.spot(x, y) {
                            let anchor = match (shift, self.lbterm_view.ui.selection) {
                                (true, Some((a, _))) => a,
                                _ => spot,
                            };
                            self.lbterm_view.ui.selection = Some((anchor, spot));
                            self.lbterm_view.selecting = true;
                        } else {
                            self.lbterm_view.ui.selection = None;
                        }
                        self.lbterm_view.dirty = true;
                        context.invalidate();
                    }
                    Some(Target::Box(key)) => {
                        self.lbterm_view_place_caret(&key, x, y, shift);
                        self.lbterm_view.dirty = true;
                        context.invalidate();
                    }
                    Some(target) => {
                        self.lbterm_light_press(x, y);
                        self.lbterm_view_press(target);
                        self.lbterm_view.dirty = true;
                        context.invalidate();
                    }
                    None => {}
                }
            }
            WMEK::VertWheel(n) => {
                let lines = 3.0 * self.render_metrics.cell_size.height as f64 * n as f64;
                let on_side = scene.side.map_or(false, |s| s.contains(x, y));
                if on_side {
                    self.lbterm_view.ui.side_scroll = (self.lbterm_view.ui.side_scroll - lines).max(0.0);
                } else {
                    self.lbterm_view.ui.scroll = (self.lbterm_view.ui.scroll + lines).max(0.0);
                }
                self.lbterm_view.dirty = true;
                context.invalidate();
            }
            _ => {}
        }
        true
    }

    /// The pointer left the window: nothing on the view stays lit
    pub(crate) fn lbterm_view_mouse_leave(&mut self) {
        if self.lbterm_view.ui.hovered.take().is_some() {
            self.lbterm_view_changed();
        }
    }

    /// The caret to where a click landed in box `key`, its typing going there
    fn lbterm_view_place_caret(&mut self, key: &BoxKey, x: f64, y: f64, select: bool) {
        let place = self.lbterm_view.scene.as_ref().and_then(|s| s.box_place(key)).cloned();
        let ui = &mut self.lbterm_view.ui;
        ui.focus = key.clone();
        ui.selection = None;
        let Some(place) = place else {
            return;
        };
        let line = place.first + ((y - place.text.y) / place.lh).floor().max(0.0) as usize;
        let col = ((x - place.text.x) / place.cw).round().max(0.0) as usize;
        let measure = Rc::clone(&self.lbterm_view.measure);
        let units = |c: char| measure.box_units(c);
        let input = ui.input(key);
        let at = input::index_at(input.text(), &place.lines, line, col, units);
        input.place(at, select);
    }

    /// A button pressed: what it does, at once, and what it asks the seat
    fn lbterm_view_press(&mut self, target: Target) {
        let name = target.name();
        let view = seat_client::seat().as_ref().and_then(|s| s.view.clone());
        match target {
            Target::Control(action) => self.lbterm_view_send(Command::Control { action }, &name, None),
            Target::Allow(ask) => self.lbterm_view_send(Command::Allow { ask }, &name, None),
            Target::Deny(ask) => self.lbterm_view_send(
                Command::Deny {
                    ask,
                    why: String::new(),
                },
                &name,
                None,
            ),
            Target::Send => self.lbterm_view_say(),
            Target::Stop => self.lbterm_view_send(Command::Interrupt, &name, None),
            Target::Chip { waiting, code } => {
                // In its box's words, or out again; an extra's yes replacing its no
                let key = BoxKey::Answer { waiting };
                let ui = &mut self.lbterm_view.ui;
                let input = ui.input(&key);
                let words = input::toggle_code(input.text(), &code);
                input.set(&words);
                ui.focus = key;
            }
            Target::Answer { waiting } => {
                let Some(w) = view.as_ref().and_then(|v| v.waiting.iter().find(|w| w.id == waiting)).cloned() else {
                    return;
                };
                if let Some(text) = self.lbterm_view.ui.answer(&w) {
                    self.lbterm_view_answer(&w, text);
                }
            }
            Target::Session { session_id, pane } => self.lbterm_view_open_session(&session_id, pane.as_deref()),
            // Show to Conductor on a hidden session: the tab menu's event, from here (render/
            // conductor_hide.rs); the view stays, and the seat's next view says it's shown
            Target::Show { session_id, label } => self.lbterm_view_show(&session_id, label),
            Target::Rsi { suggestion, action } => {
                // Reviewing, a suggestion sent or dismissed moves him on to the next open one
                if matches!(action.as_str(), "send" | "dismiss") && self.lbterm_view.ui.review.as_deref() == Some(suggestion.as_str()) {
                    let open = open_suggestions(view.as_ref());
                    let next = open
                        .iter()
                        .skip_while(|id| **id != suggestion)
                        .nth(1)
                        .or_else(|| open.iter().find(|id| **id != suggestion))
                        .cloned();
                    self.lbterm_view.ui.review = next;
                }
                let (action, rating) = match action.as_str() {
                    "useful" | "not" => ("rate".to_string(), Some(action.clone())),
                    other => (other.to_string(), None),
                };
                self.lbterm_view_send(
                    Command::Rsi {
                        action,
                        suggestion,
                        rating,
                    },
                    &name,
                    None,
                );
            }
            Target::More(id) => {
                let expanded = &mut self.lbterm_view.ui.expanded;
                if !expanded.remove(&id) {
                    expanded.insert(id);
                }
            }
            Target::FoldSide => {
                let ui = &mut self.lbterm_view.ui;
                ui.side_folded = !ui.side_folded;
                ui.hovered = None;
            }
            Target::Section(name) => {
                let folded = &mut self.lbterm_view.ui.folded;
                if !folded.remove(&name) {
                    folded.insert(name);
                }
            }
            Target::Reveal(name) => {
                // The side opened at that section, it and the list it's in unfolded
                let ui = &mut self.lbterm_view.ui;
                ui.side_folded = false;
                ui.hovered = None;
                ui.folded.remove(&name);
                if let Some((list, _)) = name.split_once(':') {
                    ui.folded.remove(list);
                }
                ui.side_scroll = ui.sections.iter().find(|(n, _)| *n == name).map_or(0.0, |(_, y)| *y);
            }
            Target::Review(step) => {
                let open = open_suggestions(view.as_ref());
                let ui = &mut self.lbterm_view.ui;
                let at = ui.review.as_ref().and_then(|id| open.iter().position(|o| o == id));
                ui.review = match (step.as_str(), at) {
                    ("start", _) => {
                        ui.folded.remove("suggestions");
                        ui.side_scroll = ui.sections.iter().find(|(n, _)| n == "suggestions").map_or(ui.side_scroll, |(_, y)| *y);
                        open.first().cloned()
                    }
                    (_, _) if open.is_empty() => None,
                    ("next", Some(i)) => Some(open[(i + 1) % open.len()].clone()),
                    ("previous", Some(i)) => Some(open[(i + open.len() - 1) % open.len()].clone()),
                    ("next" | "previous", None) => open.first().cloned(),
                    _ => None,
                };
            }
            Target::Newest => self.lbterm_view.ui.scroll = 0.0,
            Target::Box(_) | Target::Conversation | Target::Side => {}
        }
    }

    fn lbterm_view_answer(&mut self, w: &Waiting, text: String) {
        let button = Target::Answer { waiting: w.id.clone() }.name();
        self.lbterm_view_send(
            Command::Answer {
                question: w.id.clone(),
                text,
            },
            &button,
            None,
        );
        if matches!(&self.lbterm_view.ui.focus, BoxKey::Answer { waiting, .. } if *waiting == w.id) {
            self.lbterm_view.ui.focus = BoxKey::Main;
        }
    }

    /// His words in the box to the Conductor: the box empties at once, and the seat's echo puts
    /// them in the conversation
    fn lbterm_view_say(&mut self) {
        let takes = seat_client::seat()
            .as_ref()
            .map_or(false, |s| s.connected && s.view.as_ref().map_or(false, |v| v.conductor.takes_words()));
        let words = self.lbterm_view.ui.main.text().trim().to_string();
        if words.is_empty() {
            return;
        }
        if !takes {
            self.lbterm_view_notice("The Conductor is off: turn it on to talk to it".to_string(), true);
            return;
        }
        self.lbterm_view.ui.main.take();
        self.lbterm_view.ui.scroll = 0.0;
        self.lbterm_view_send(Command::Say { text: words.clone() }, "send", Some(words));
    }

    /// A session's card clicked: the tab holding it opens, the view leaving for it
    fn lbterm_view_open_session(&mut self, session_id: &str, pane: Option<&str>) {
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            return;
        };
        let found = window.iter_tabs().enumerate().find_map(|(i, tab)| {
            tab.iter_panes_ignoring_zoom().into_iter().any(|pos| {
                pos.pane.copy_user_vars().get("claude_session").map(String::as_str) == Some(session_id)
                    || pane.map_or(false, |p| super::tab_panel::server_pane_id(&pos.pane).to_string() == p)
            })
            .then_some(i)
        });
        drop(window);
        match found {
            Some(idx) => {
                self.lbterm_view_close();
                if let Err(err) = self.activate_tab(idx as isize) {
                    log::error!("conductor view: opening {session_id}'s tab: {err:#}");
                }
            }
            None => self.lbterm_view_notice("That session's tab isn't in this window".to_string(), true),
        }
    }

    /// Copy the conversation's selected words (with Ctrl+C, `always`; let go of a drag, only
    /// when it selected something), lighting them up as a pane's copy does
    fn lbterm_view_copy_selection(&mut self, always: bool) -> bool {
        let Some(scene) = self.lbterm_view.scene.as_ref() else {
            return false;
        };
        let Some((a, z)) = self.lbterm_view.ui.selection else {
            return false;
        };
        let words = scene.selected(a, z);
        if words.is_empty() {
            if !always {
                self.lbterm_view.ui.selection = None;
            }
            return false;
        }
        let (from, to) = (a.min(z), a.max(z));
        let rects: Vec<PixelRect> = scene
            .lines
            .iter()
            .enumerate()
            .take(to.line + 1)
            .skip(from.line)
            .filter(|(_, l)| l.y >= scene.conversation.y && l.y + l.h <= scene.conversation.y + scene.conversation.h)
            .map(|(i, l)| {
                let last = l.xs.len().saturating_sub(1);
                let c0 = if i == from.line { from.col.min(last) } else { 0 };
                let c1 = if i == to.line { to.col.min(last) } else { last };
                let (x0, x1) = (l.xs.get(c0).copied().unwrap_or(l.x), l.xs.get(c1).copied().unwrap_or(l.x));
                PixelRect::new(x0.round() as i32, l.y.round() as i32, (x1 - x0).max(1.0).round() as u32, l.h.round() as u32)
            })
            .collect();
        self.copy_to_clipboard(ClipboardCopyDestination::ClipboardAndPrimarySelection, words);
        if !rects.is_empty() {
            self.lbterm_light(Light::Text {
                rects,
                radius: 3.0 * self.dimensions.dpi as f64 / 96.0,
            });
        }
        true
    }

    /// Pasted words into the box typing goes to
    fn lbterm_view_paste(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let future = window.get_clipboard(Clipboard::Clipboard);
        promise::spawn::spawn(async move {
            if let Ok(clip) = future.await {
                window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    if tw.lbterm_view.open {
                        let key = tw.lbterm_view.ui.focus.clone();
                        tw.lbterm_view.ui.input(&key).insert(&clip);
                        tw.lbterm_view_changed();
                    }
                })));
            }
        })
        .detach();
    }

    /// Whether the view takes a key before the key bindings do: typing and editing keys, and
    /// Ctrl with A, C, V and X; Alt, the function keys and the tab keys (Ctrl+Tab, Ctrl+PageUp)
    /// still reach the bindings
    pub(crate) fn lbterm_view_takes(&self, key: &KeyCode, mods: KeyModifiers) -> bool {
        if !self.lbterm_view.open {
            return false;
        }
        let ctrl = mods.contains(KeyModifiers::CTRL);
        let alt = mods.contains(KeyModifiers::ALT);
        let shift = mods.contains(KeyModifiers::SHIFT);
        if alt {
            return false;
        }
        match key {
            KeyCode::Char(c) if ctrl => matches!(c.to_ascii_lowercase(), 'a' | 'c' | 'v' | 'x'),
            KeyCode::Char(_) => true,
            KeyCode::Tab | KeyCode::PageUp | KeyCode::PageDown if ctrl => false,
            KeyCode::UpArrow | KeyCode::DownArrow if ctrl => false,
            KeyCode::Insert => shift && !ctrl,
            KeyCode::Enter
            | KeyCode::Tab
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Escape
            | KeyCode::LeftArrow
            | KeyCode::RightArrow
            | KeyCode::UpArrow
            | KeyCode::DownArrow
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => true,
            _ => false,
        }
    }

    /// A key while the view is open: typing, editing, Enter to send, Esc to leave. True when
    /// it did something with it.
    pub(crate) fn lbterm_view_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        if !self.lbterm_view.open {
            return false;
        }
        let ctrl = mods.contains(KeyModifiers::CTRL);
        let shift = mods.contains(KeyModifiers::SHIFT);
        let focus = self.lbterm_view.ui.focus.clone();
        let place = self.lbterm_view.scene.as_ref().and_then(|s| s.box_place(&focus)).cloned();
        let lines = place.as_ref().map(|p| p.lines.clone()).unwrap_or_default();
        let measure = Rc::clone(&self.lbterm_view.measure);
        let units = |c: char| measure.box_units(c);
        let page = self
            .lbterm_view
            .scene
            .as_ref()
            .map_or(400.0, |s| (s.conversation.h - self.render_metrics.cell_size.height as f64 * 2.0).max(40.0));
        let mut handled = true;
        match key {
            KeyCode::Escape => {
                let ui = &mut self.lbterm_view.ui;
                if ui.selection.take().is_some() {
                } else if focus != BoxKey::Main {
                    ui.focus = BoxKey::Main;
                } else if ui.main.selection().is_some() {
                    let caret = ui.main.caret();
                    ui.main.place(caret, false);
                } else if ui.main.is_empty() {
                    self.lbterm_view_close();
                    return true;
                }
            }
            KeyCode::Enter if shift => self.lbterm_view.ui.input(&focus).insert("\n"),
            KeyCode::Enter => match &focus {
                BoxKey::Main => self.lbterm_view_say(),
                BoxKey::Answer { waiting, .. } => {
                    let waiting = waiting.clone();
                    self.lbterm_view_press(Target::Answer { waiting });
                }
            },
            KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
                'a' => self.lbterm_view.ui.input(&focus).select_all(),
                'c' => {
                    let selected = self.lbterm_view.ui.input(&focus).selected().map(str::to_string);
                    match selected {
                        Some(words) => self.copy_to_clipboard(ClipboardCopyDestination::ClipboardAndPrimarySelection, words),
                        None => {
                            self.lbterm_view_copy_selection(true);
                        }
                    }
                }
                'x' => {
                    let input = self.lbterm_view.ui.input(&focus);
                    if let Some(words) = input.selected().map(str::to_string) {
                        input.backspace();
                        self.copy_to_clipboard(ClipboardCopyDestination::ClipboardAndPrimarySelection, words);
                    }
                }
                'v' => self.lbterm_view_paste(),
                _ => handled = false,
            },
            KeyCode::Char(c) => {
                let mut buf = [0u8; 4];
                self.lbterm_view.ui.selection = None;
                self.lbterm_view.ui.input(&focus).insert(c.encode_utf8(&mut buf));
            }
            KeyCode::Insert if shift => self.lbterm_view_paste(),
            KeyCode::Tab => self.lbterm_view.ui.input(&focus).insert("    "),
            KeyCode::Backspace if ctrl => self.lbterm_view.ui.input(&focus).backspace_word(),
            KeyCode::Backspace => self.lbterm_view.ui.input(&focus).backspace(),
            KeyCode::Delete => self.lbterm_view.ui.input(&focus).delete(),
            KeyCode::LeftArrow if ctrl => self.lbterm_view.ui.input(&focus).word_left(shift),
            KeyCode::RightArrow if ctrl => self.lbterm_view.ui.input(&focus).word_right(shift),
            KeyCode::LeftArrow => self.lbterm_view.ui.input(&focus).left(shift),
            KeyCode::RightArrow => self.lbterm_view.ui.input(&focus).right(shift),
            KeyCode::Home if ctrl => self.lbterm_view.ui.scroll = f64::MAX / 4.0,
            KeyCode::End if ctrl => self.lbterm_view.ui.scroll = 0.0,
            KeyCode::Home => self.lbterm_view.ui.input(&focus).home(shift, &lines, &units),
            KeyCode::End => self.lbterm_view.ui.input(&focus).end(shift, &lines, &units),
            KeyCode::UpArrow | KeyCode::DownArrow => {
                let by = if *key == KeyCode::UpArrow { -1 } else { 1 };
                let input = self.lbterm_view.ui.input(&focus);
                if !input.is_empty() && !input.on_edge(by, &lines, &units) || shift {
                    input.vertical(by, shift, &lines, &units);
                } else {
                    // At the box's edge the conversation scrolls instead
                    let line = self.render_metrics.cell_size.height as f64;
                    let ui = &mut self.lbterm_view.ui;
                    ui.scroll = (ui.scroll - by as f64 * line * 2.0).max(0.0);
                }
            }
            KeyCode::PageUp => self.lbterm_view.ui.scroll += page,
            KeyCode::PageDown => self.lbterm_view.ui.scroll = (self.lbterm_view.ui.scroll - page).max(0.0),
            _ => handled = false,
        }
        if handled {
            self.lbterm_view_changed();
        }
        handled
    }

    /// Composed words (an IME's, a dead key's) into the box typing goes to
    pub(crate) fn lbterm_view_text(&mut self, words: &str) -> bool {
        if !self.lbterm_view.open {
            return false;
        }
        let focus = self.lbterm_view.ui.focus.clone();
        self.lbterm_view.ui.input(&focus).insert(words);
        self.lbterm_view_changed();
        true
    }
}

/// LBTerm: the Conductor's view, for the state file (lbterm/STATE.md)
impl TermWindow {
    pub(crate) fn lbterm_state_view(&self) -> Option<lbterm_extras::state::ConductorView> {
        let scene = self.lbterm_view.scene.as_ref().filter(|_| self.lbterm_view.open)?;
        Some(scene.said.clone())
    }

    /// What of the view is at `x`, `y`: its name in the state file's `pointer.over`, and the
    /// button's, box's or card's name
    pub(crate) fn lbterm_view_under(&self, x: f64, y: f64) -> Option<(&'static str, Option<String>)> {
        let scene = self.lbterm_view.scene.as_ref().filter(|_| self.lbterm_view.open)?;
        if !scene.card.contains(x, y) {
            return None;
        }
        Some(match scene.hit(x, y) {
            Some(Target::Box(key)) => ("view.box", Some(key.name())),
            Some(Target::Conversation) => ("view.conversation", None),
            Some(Target::Side) | None => ("view", None),
            Some(t) => ("view.button", Some(t.name())),
        })
    }

    /// The view's notice going, while one shows
    pub(crate) fn lbterm_view_wait(&self, now: Instant) -> Option<lbterm_extras::state::Wait> {
        let until = self.lbterm_view.notice_until.filter(|_| self.lbterm_view.open)?;
        Some(lbterm_extras::state::Wait {
            name: "view.notice".to_string(),
            left_ms: lbterm_extras::state::millis(until.saturating_duration_since(now)),
        })
    }
}
