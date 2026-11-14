//! LBTerm: the tab panel (lbterm/TAB-PANEL.md).
//!
//! A card down the left of the window, the colour of the panes and ringed
//! with a grey hairline, holding a Claude status pad per tab. While the
//! pointer rests on it, it opens into a list of the tabs, its ring lights
//! red and glows, and the panes' cards press over to make room. What each
//! row says, when the panel opens and where things go are `lbterm_panel`'s;
//! this reads the tabs from the mux, draws them and takes the mouse events
//! over the panel.
//!
//! The panel draws into z-layers of its own: [`GLOW_Z`], [`BODY_Z`] and
//! [`WORDS_Z`]. Without the compositor they're drawn over everything but
//! modals, at the panel's size of the moment. With the compositor they're
//! the panel's visuals, always drawn open, and DWM opens and closes the
//! panel ([`PanelPlan`]), so the motion keeps going whatever LBTerm's GUI
//! thread is doing. Only the compositor has the light that runs round the
//! ring as it opens ([`SPOT_Z`], [`SPOT_CORE_Z`]): DWM carries it.

use crate::quad::{QuadTrait, TripleLayerQuadAllocator, TripleLayerQuadAllocatorTrait};
use crate::glyphcache::CachedGlyph;
use crate::termwindow::box_model::{
    ComputedElement, ComputedElementContent, Element, ElementCell, ElementContent, LayoutContext,
};
use crate::termwindow::compositor::{MotionSettings, PanelPlan, RingFade, RingLight, RingPlan, RowMotion, RowPlan, WavePlan};
use crate::termwindow::render::conductor::{self as conductor_ring, RingStyle};
use lbterm_extras::conductor::{self, Doing, Level, PaneFact};
use lbterm_panel::words::HeadSays;
use crate::termwindow::render::card::Room;
use crate::termwindow::render::kept_quads::{bits, layout_bits, KeptQuads};
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, RectF, WindowOps};
use config::{ConfigHandle, DimensionContext};
use lbterm_motion::{Curve, PanelPart, PixelRect};
use lbterm_panel::words::{self, Placed};
use lbterm_extras::links;
use lbterm_panel::{palette, Area, Blocked, Hover, Layout, LinkState, Program, ProgramState, Row, RowState, Status, Tab, TabLink};
use mux::pane::{CachePolicy, Pane, PaneId};
use mux::tab::TabId;
use crate::termwindow::titles::DrawnRows;
use wezterm_term::program_status::{self as osc7501, ProgramStatusRecord};
use mux::Mux;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use wezterm_font::LoadedFont;
use window::color::LinearRgba;

/// The panel's z-layers are this and the nine above it. Modals (100 and up)
/// stay over it; the tab bar (10, 11) goes under it.
pub const PANEL_ZINDEX: i8 = 50;
/// The light round the ring
const GLOW_Z: i8 = PANEL_ZINDEX;
/// The grey ring, the card inside it, the pads and their numbers
const BODY_Z: i8 = PANEL_ZINDEX + 1;
/// What only the open panel shows: titles and the rows' long highlights
const WORDS_Z: i8 = PANEL_ZINDEX + 2;
/// The strip's background, the panes' colour: in `over`, under the panel
const STRIP_Z: i8 = PANEL_ZINDEX - 1;
/// The running light's halo and its bright middle, drawn where it starts
const SPOT_Z: i8 = PANEL_ZINDEX + 4;
const SPOT_CORE_Z: i8 = PANEL_ZINDEX + 5;
/// The ring lit: the red gradient, over the grey ring while the panel is open
const LIT_Z: i8 = PANEL_ZINDEX + 6;
/// The turning arc's template, with the compositor: drawn once, shown on
/// each pad waiting on tasks
const SPIN_Z: i8 = PANEL_ZINDEX + 7;
/// The Conductor's outlines' templates and its light's, with the compositor: drawn once, shown
/// round each pad it's on and its own (render/conductor.rs)
const RING_SPECTRUM_Z: i8 = PANEL_ZINDEX + 3;
const RING_IRIS_Z: i8 = PANEL_ZINDEX + 8;
const RING_LIGHT_Z: i8 = PANEL_ZINDEX + 9;
/// The white arc's template on the Starting row's pad while a new tab is on its way, with the
/// compositor: drawn once, shown turning on that pad (lbterm/NEW-TAB.md, "The Starting row"). The
/// panel's ten layers from PANEL_ZINDEX are full, so it's one of the few free above them
const START_SPIN_Z: i8 = 98;

/// The tabs' rows on the rail (pills, pads, numbers, labels) and the open tab's ring and bar,
/// with the compositor: each in a surface of its own, the rows shown a row at a time so DWM
/// can slide, bring in, fade out and dissolve them, the marker gliding to the open tab
/// (compositor.rs, `PanelLayers::sync_rows`). Without it they're drawn in the body.
const ROWS_Z: i8 = 46;
const MARKER_Z: i8 = 47;
/// The rows' parts on the open panel (the long highlight and its ring, the pad drawn again on
/// it, the words, the chips, the progress track), with the compositor: shown a row at a time
/// too, so they move with their row, under the words' fade. Without it they're in the words.
const ROW_WORDS_Z: i8 = 48;
/// Each row's progress fill, whole, with the compositor: the open panel shows as much of it as
/// the program says, gliding when that changes. Without it the fill is drawn as long as that.
const PROGRESS_Z: i8 = 45;
/// The wave washing out ahead of the edge as it opens, and the spray it throws, with the
/// compositor: each cell's cover in the first of a layer's three, its character in the second
/// (lbterm_panel::wave)
const WAVE_Z: i8 = 83;
const SPRAY_Z: i8 = 84;

/// Whether a z-layer's quads are the panel's.
pub fn owns_zindex(zindex: i8) -> bool {
    (PANEL_ZINDEX..PANEL_ZINDEX + 10).contains(&zindex)
        || [ROWS_Z, ROW_WORDS_Z, MARKER_Z, PROGRESS_Z, WAVE_Z, SPRAY_Z, START_SPIN_Z].contains(&zindex)
}

/// Whether the panel's visual `part` draws z-layer `zindex`. The glow and
/// the ring are drawn once, open, and seen through two visuals each: the
/// body's and the cap's.
pub fn draws(zindex: i8, part: PanelPart) -> bool {
    matches!(
        (zindex, part),
        (GLOW_Z, PanelPart::Glow | PanelPart::CapGlow)
            | (BODY_Z, PanelPart::Body | PanelPart::Cap)
            | (WORDS_Z, PanelPart::Words)
            | (LIT_Z, PanelPart::Lit | PanelPart::CapLit)
            | (SPOT_Z, PanelPart::Spot)
            | (SPOT_CORE_Z, PanelPart::SpotCore)
            | (SPIN_Z, PanelPart::Spinner)
            | (START_SPIN_Z, PanelPart::StartSpinner)
            | (RING_SPECTRUM_Z, PanelPart::RingSpectrum)
            | (RING_IRIS_Z, PanelPart::RingIris)
            | (RING_LIGHT_Z, PanelPart::RingLight)
            | (ROWS_Z, PanelPart::Rows)
            | (ROW_WORDS_Z, PanelPart::RowWords)
            | (MARKER_Z, PanelPart::Marker)
            | (PROGRESS_Z, PanelPart::Progress)
            | (WAVE_Z, PanelPart::Wave)
            | (SPRAY_Z, PanelPart::Spray)
    )
}

/// The words wait this long for the edge before fading in
const WORDS_DELAY: f64 = 0.06;
/// How long the projects looked up for the panes hold before another look
const CWD_TTL: Duration = Duration::from_secs(15);

/// The panel's layout with no rows, for its sizes.
fn bare_layout(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> Layout {
    let context = DimensionContext {
        dpi: dimensions.dpi as f32,
        pixel_max: dimensions.pixel_width as f32,
        pixel_cell: metrics.cell_size.width as f32,
    };
    let settings = &config.lbterm_tab_panel;
    Layout {
        left: 0.0,
        body_top: 0.0,
        bottom: 0.0,
        right: 0.0,
        rail: settings.rail_width.evaluate_as_pixels(context).round().max(0.0),
        width: settings.width.evaluate_as_pixels(context).round().max(0.0),
        scale: dimensions.dpi as f32 / 96.0,
        rows: 0,
        label: 0.0,
        label_line: 0.0,
        head: false,
        foot: false,
    }
}

/// The pane id `lbterm cli` takes for `pane`: the mux server's, for a pane that lives there
pub(crate) fn server_pane_id(pane: &Arc<dyn Pane>) -> u64 {
    use wezterm_dynamic::Value;
    let remote = match pane.get_metadata() {
        Value::Object(obj) => match obj.get_by_str("remote_pane_id") {
            Some(Value::U64(id)) => Some(*id),
            Some(Value::I64(id)) => Some(*id as u64),
            _ => None,
        },
        _ => None,
    };
    remote.unwrap_or(pane.pane_id() as u64)
}

/// The strip the window keeps down its left for the closed panel (gutter,
/// rail), 0 with the panel off. The window treats it as border, so
/// everything that places panes and splits leaves room for it; the tab bar
/// takes it back (`fancy_tab_bar.rs`).
pub fn rail_width(
    config: &ConfigHandle,
    dimensions: &crate::Dimensions,
    metrics: &RenderMetrics,
) -> f32 {
    if !config.lbterm_tab_panel.enabled {
        return 0.0;
    }
    bare_layout(config, dimensions, metrics).strip()
}

/// The middle of the closed rail, where its pads line up, from the window's left edge inside any
/// OS border: the strip's InMail button is centred over it (render/inbox.rs). 0 with the panel off.
pub fn rail_column(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    if !config.lbterm_tab_panel.enabled {
        return 0.0;
    }
    bare_layout(config, dimensions, metrics).column()
}

/// The words last laid out, and what they were laid out from.
#[derive(PartialEq)]
struct WordsKey {
    placed: Placed,
    shape_generation: usize,
    font: usize,
    label_font: usize,
    count_font: usize,
    /// The marks' font (`tab_panel_mark_font`), 0 while no row is hidden from the Conductor
    mark_font: usize,
    /// The highlighted rows, whose numbers and labels the open panel draws
    /// again over their long highlight
    raised: Vec<usize>,
}

struct LaidOut {
    key: WordsKey,
    numbers: Vec<ComputedElement>,
    labels: Vec<ComputedElement>,
    words: Vec<ComputedElement>,
    raised: Vec<ComputedElement>,
}

pub struct TabPanelState {
    /// Whether it's open; the tasks card goes with it (render/tasks_card.rs)
    pub(crate) hover: Hover,
    /// The row under the pointer
    hovered: Option<usize>,
    /// A button went down on the panel: its release is the panel's too
    pub(crate) pressed: bool,
    /// The compositor's part this frame; `None` with the panel off or no
    /// compositor
    pub plan: Option<PanelPlan>,
    /// Where things were last drawn, for the mouse (and a tab being dragged
    /// along the rows, tab_mouse.rs)
    pub(crate) layout: Option<Layout>,
    laid_out: Option<LaidOut>,
    /// Every word of the last lay-out, by what it was laid out from, for the next (render/words.rs)
    words: std::cell::RefCell<super::words::Words>,
    /// How many times the words have been laid out, and their quads as last made, a z-layer each
    laid_count: u64,
    /// The Conductor's badge's count's font, with its metrics and size, and the shape generation
    /// and DPI it was looked up at (`tab_panel_count_font`)
    count_font: Option<((usize, usize), Rc<LoadedFont>, RenderMetrics, words::Font)>,
    /// The marks' font, for the eye-off mark of a tab hidden from the Conductor, with its metrics
    /// and size, and the shape generation and DPI it was looked up at (`tab_panel_mark_font`)
    mark_font: Option<((usize, usize), Rc<LoadedFont>, RenderMetrics, words::Font)>,
    word_quads: std::cell::RefCell<super::kept_quads::KeptLayers>,
    /// The parts that change only with its place, its colours and how lit it is, as last made
    chrome: PanelChrome,
    /// When a repaint is already on its way
    wake: Option<Instant>,
    /// What the last paint waited for besides the pointer: an age reading otherwise, and the
    /// hub's outlines going or its bars running out
    due: (Option<Instant>, Option<Instant>),
    /// The Starting row's time running out, while it waits for its tab (render/new_tab.rs)
    starting_due: Option<Instant>,
    /// The last menu card fading out gone, while the panel is held for it (`Hover::hold`)
    held_due: Option<Instant>,
    /// Hide from Conductor and Show to Conductor picked from a tab menu in this window, each
    /// until the Conductor's file says it or 5 s go by (render/conductor_hide.rs)
    pub(crate) hides: lbterm_extras::conductor::hide::Picks,
    /// The first of those running out, for the paint's wake
    pub(crate) hide_due: Option<Instant>,
    /// Each tab's active pane's working directory
    cwds: HashMap<PaneId, String>,
    cwds_fetched: Option<Instant>,
    fetching: bool,
    /// When each tab was last on screen, in Unix seconds: results that
    /// arrived since are unread
    seen: HashMap<TabId, f64>,
    /// The tabs this window showed, newest first, for Shift+Back (`LbtermTab 'Last'`,
    /// termwindow/hop.rs): the open tab put in front at each paint that shows another
    recent: lbterm_extras::hop::Recent,
    /// Tabs whose pads glint at the next paint: a program typed into them, or they were
    /// renamed (lbterm_panel_glint), or a tab opened behind came (render/new_tab.rs)
    pub(crate) glints: Vec<TabId>,
    /// Tabs whose pads send a light up the rail to the Conductor's at the next paint that has the
    /// rail to itself: a Hide from Conductor or Show to Conductor picked (render/conductor_hide.rs,
    /// `Light::Send`)
    pub(crate) sends: Vec<TabId>,
    /// A tab dragged from another window is over this one: the rail shows where it would
    /// land, after the last tab
    drop_hint: bool,
    /// The Conductor's tab, as the last paint found it: its pad stands for it, so it has no row
    /// (lbterm_extras::conductor::tab_of)
    pub(crate) conductor_tab: Option<TabId>,
    /// The pointer is on the Conductor's pad
    head_hovered: bool,
    /// The Conductor acts on a session, as its hub said at the last paint (conductor_hub::acting):
    /// its pad's spectrum in Accept most and Accept all (Lachlan's 7a)
    head_acting: bool,
    /// The spectrum's ring on its pad as it comes and goes with the compositor, and when one
    /// fading out will have gone, for a paint then (lbterm_extras::conductor::PadRing)
    pad_ring: conductor::PadRing,
    ring_due: Option<Instant>,
    /// What the last paint showed, with a state file (lbterm/STATE.md)
    pub(crate) said: Option<lbterm_extras::state::Rail>,
    /// Row by row, the tab whose Claude session has parts still running that its statusline
    /// has listed, as the last paint found them: resting on the row shows the list in a card
    /// beside it (render/tasks_card.rs)
    pub(crate) tasks_rows: Vec<Option<TabId>>,
    /// The tab links as the last paint drew them (lbterm/LINKS.md)
    pub(crate) links: RailLinks,
    /// With the compositor, the lights along the suggested tab connections the last paint drew
    /// (render/connections.rs); their template is kept in its own layer (render/kept.rs)
    pub(crate) glow: Option<crate::termwindow::compositor::GlowPlan>,
    /// The pointer is on the rail (or the open panel): coming onto it runs the suggested lines'
    /// lights' laps again (lbterm_extras::glow, Lachlan's 1a)
    on_rail: bool,
    /// Each tab on the rail as the last paint made its row from it, its link included: whether
    /// a program's OSC 7501 report changes what the row shows is asked of it
    /// (tab_panel_status_moves_row), and a title's, by the window's mux subscription too, which
    /// is left the same map as the paint makes it (titles.rs, TitleWatch)
    drawn: DrawnRows,
    /// The tab the last paint drew on each row, row by row: what the pointer's row is on until
    /// the next paint (`tab_panel_drawn_tab`, PITFALLS 156)
    drawn_rows: Vec<TabId>,
}

/// The panel's parts kept as quads and copied into layers they share (render/kept_quads.rs): the
/// rows, the card in its grey ring, the open tab's ring and bar. The light round the panel, the
/// ring lit and the running light's two discs have layers of their own, which keep them
/// (render/kept.rs)
#[derive(Default)]
struct PanelChrome {
    /// The rows on the rail, and their parts on the open panel and their bars' fills
    rows: std::cell::RefCell<KeptQuads>,
    open: std::cell::RefCell<KeptQuads>,
    progress: std::cell::RefCell<KeptQuads>,
    body: std::cell::RefCell<KeptQuads>,
    marker: std::cell::RefCell<KeptQuads>,
}

/// The tab links on the rail as the last paint found them: each row's Claude session, the lines
/// between rows (the links' and the tab connections'), and what each row's slot draws of them
#[derive(Default)]
pub(crate) struct RailLinks {
    pub(crate) sessions: Vec<Option<String>>,
    pub(crate) lines: Vec<links::Line>,
    /// The tab connections' lines, in the lanes the links' lines leave them
    pub(crate) joins: Vec<links::Join>,
    pub(crate) pieces: Vec<Vec<links::Piece>>,
    /// What the links file said, with changes asked of it that it doesn't say yet
    pub(crate) said: Arc<links::Links>,
    /// Each session on the rail by its row's label, for the menu and the drag's cards
    pub(crate) labels: HashMap<String, String>,
    /// How many tab connections are suggested with a member on this rail: the count on the
    /// Conductor's pad (render/conductor.rs, `links::Links::suggested_on`)
    pub(crate) suggested: usize,
}

/// Every Claude session in any tab of any window: a link whose session waited on is in none
/// says it closed
fn open_sessions() -> HashSet<String> {
    let mux = Mux::get();
    let mut open = HashSet::new();
    for id in mux.iter_windows() {
        let tabs: Vec<Arc<mux::tab::Tab>> = match mux.get_window(id) {
            Some(window) => window.iter_tabs().cloned().collect(),
            None => continue,
        };
        for tab in tabs {
            for pos in tab.iter_panes_ignoring_zoom() {
                if let Some(sid) = pos.pane.copy_user_vars().remove("claude_session").filter(|s| !s.is_empty()) {
                    open.insert(sid);
                }
            }
        }
    }
    open
}

/// The panel's word for where a link stands
fn link_state(standing: links::Standing) -> LinkState {
    match standing {
        links::Standing::Stuck => LinkState::Stuck,
        links::Standing::Failed => LinkState::Failed,
        links::Standing::Closed => LinkState::Closed,
        links::Standing::Waiting => LinkState::Waiting,
        links::Standing::Telling => LinkState::Telling,
        links::Standing::Told => LinkState::Told,
    }
}

/// A link's line, in its colour: purple waiting, green once what it waits on finished, red gone
/// wrong, and a suggested one faint purple over `under`; a tab connection's cyan, faint over
/// `under` while it's suggested
pub(crate) fn link_colour(look: links::Look, under: u32) -> u32 {
    match look {
        links::Look::Waiting => palette::LINK_WAITING,
        links::Look::Done => palette::LINK_DONE,
        links::Look::Error => palette::LINK_ERROR,
        links::Look::Suggested => palette::blend(under, palette::LINK_WAITING, palette::LINK_SUGGESTED_ALPHA),
        links::Look::ConnectionSuggested => palette::blend(under, palette::CONNECTED, palette::LINK_SUGGESTED_ALPHA),
        links::Look::Connected => palette::CONNECTED,
    }
}

impl Default for TabPanelState {
    fn default() -> Self {
        Self {
            hover: Hover::new(Duration::from_millis(150), Duration::from_millis(300)),
            hovered: None,
            pressed: false,
            plan: None,
            layout: None,
            laid_out: None,
            words: Default::default(),
            laid_count: 0,
            count_font: None,
            mark_font: None,
            word_quads: Default::default(),
            chrome: Default::default(),
            wake: None,
            due: (None, None),
            starting_due: None,
            held_due: None,
            hides: Default::default(),
            hide_due: None,
            cwds: HashMap::new(),
            cwds_fetched: None,
            fetching: false,
            seen: HashMap::new(),
            recent: Default::default(),
            glints: vec![],
            sends: vec![],
            drop_hint: false,
            conductor_tab: None,
            head_hovered: false,
            head_acting: false,
            pad_ring: Default::default(),
            ring_due: None,
            said: None,
            tasks_rows: vec![],
            links: RailLinks::default(),
            glow: None,
            on_rail: false,
            drawn: DrawnRows::default(),
            drawn_rows: vec![],
        }
    }
}

impl TabPanelState {
    /// LBTerm: the laid-out words point into a glyph atlas that's gone (gpu_lost.rs)
    pub(crate) fn forget_laid_out(&mut self) {
        self.laid_out = None;
        self.words.get_mut().forget();
        self.word_quads.get_mut().forget();
        let c = &mut self.chrome;
        for kept in [&mut c.rows, &mut c.open, &mut c.progress, &mut c.body, &mut c.marker] {
            kept.get_mut().forget();
        }
    }
}

/// What a pane's program reported (OSC 7501), as the panel takes it.
fn program(record: ProgramStatusRecord) -> Program {
    Program {
        state: match record.state {
            osc7501::ProgramState::Working => ProgramState::Working,
            osc7501::ProgramState::Done => ProgramState::Done,
            osc7501::ProgramState::Blocked => ProgramState::Blocked,
            osc7501::ProgramState::Error => ProgramState::Error,
            osc7501::ProgramState::Idle | osc7501::ProgramState::Clear => ProgramState::Idle,
        },
        kind: record.kind.map(|kind| match kind {
            osc7501::BlockedKind::Permission => Blocked::Permission,
            osc7501::BlockedKind::Question => Blocked::Question,
            osc7501::BlockedKind::Auth => Blocked::Auth,
        }),
        progress: record.progress,
        title: record.title,
        msg: record.msg,
        updated: record
            .updated
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64()),
        id: record.id,
    }
}

/// The mux's tab `idx` of its window as a row sees it, from what's in memory: titles, user vars,
/// program reports and its Claude session's label (wezterm-gui/src/summaries.rs, read off this
/// thread). Its working directory and when it was last seen are the panel's to add. The window
/// strip names the active tab with it too (render/strip.rs).
pub(crate) fn panel_tab(tab: &mux::tab::Tab, idx: usize, active: bool) -> Tab {
    let active_pane = tab.get_active_pane();
    let active_id = active_pane.as_ref().map(|p| p.pane_id());
    let panes = tab.iter_panes_ignoring_zoom();
    let pane_titles = pane_titles(active_pane.as_ref(), &panes);
    let mut statuses = vec![];
    let mut quiets = vec![];
    let mut programs = vec![];
    // Its Claude session, for its label: the active pane's, else any
    let mut session = None;
    for pos in panes {
        let is_active = Some(pos.pane.pane_id()) == active_id;
        let vars = pos.pane.copy_user_vars();
        if let Some(value) = vars.get("claude_status") {
            statuses.extend(Status::parse(value));
        }
        // A working reply gone quiet or stuck, as Terminal's statusline judges it
        if let Some(value) = vars.get("claude_quiet") {
            quiets.extend(lbterm_panel::claude::Quiet::parse(value));
        }
        if let Some(sid) = vars.get("claude_session").filter(|s| !s.is_empty()) {
            if session.is_none() || is_active {
                session = Some(sid.clone());
            }
        }
        programs.extend(pos.pane.get_program_status().into_iter().map(program));
    }
    // How far along its session is, as the Conductor's hub guesses it
    let estimate = session.as_deref().and_then(crate::conductor_hub::estimate).map(|e| lbterm_panel::claude::Estimate {
        percent: e.percent,
        left: e.left_secs,
        agents: e.agents.iter().map(|a| a.percent).collect(),
    });
    let summary = session.and_then(|sid| crate::summaries::get(&sid));
    Tab {
        index: idx + 1,
        tab_title: tab.get_title(),
        pane_titles,
        // Where its Claude session is now, as its statusline says: the
        // folder the tab's shell last reported is where the tab opened
        cwd: summary.as_ref().and_then(|s| s.cwd.clone()).unwrap_or_default(),
        statuses,
        programs,
        seen: 0.0,
        active,
        // What its conversation has cost, as the statusline last wrote it, for the open row
        // (Lachlan's E6, 2026-11-07)
        usd: summary.as_ref().and_then(|s| s.usage).map(|u| u.usd),
        // When Lachlan last prompted it, for the open row's first line (his ask of 2026-11-11):
        // from the summary already in hand, by the one call a harness session's time will come
        // through too, so the strip's every paint and the menus pay no second lookup for it
        prompted: summary
            .as_ref()
            .and_then(|s| s.last_prompt())
            .and_then(|at| at.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64()),
        label: summary.and_then(|s| s.label.clone()),
        estimate,
        link: None,
        quiets,
        // Hidden from the Conductor: the rail's paint says, from the window's picks and the
        // Conductor's file (render/conductor_hide.rs); the menu, the strip and the projects card
        // read rows without it
        hidden: false,
        // Parked: the mux tab's flag, an atomic load (park.rs; Lachlan's 8a)
        parked: tab.lbterm_parked(),
    }
}

/// The name tab `tab`, on the rail's row `idx` (the Conductor's tab not counted, as the rail
/// numbers them), shows under its pad: the name it was given, else its
/// Claude session's label, else its title as its row reads it (a program's own, else its active
/// pane's program, "pwsh" for "pwsh.exe"), as `lbterm_panel::claude::row` settles it. The tab
/// menu's Rename starts from it and its Copy link names the tab by it (render/tab_menu.rs). Read
/// once a pick, from what the rail keeps: no file
pub(crate) fn tab_label(tab: &mux::tab::Tab, idx: usize) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    // The idle timer's length changes nothing of the label
    lbterm_panel::row(&panel_tab(tab, idx, false), now, 3600.0).0.label
}

/// Whether `tab`'s row, as a paint drew it (`drawn`, its idle timer filling over `idle_full`
/// seconds), would read otherwise with its titles now (lbterm_panel::claude::titles_move_row):
/// asked for a pane's title alert by the window (lbterm_panel_title_changed) and, before any
/// task, by its mux subscription (titles.rs, lbterm_title_alert_dropped). Reads the mux's tab and
/// its panes' titles (each pane's lock), nothing of the window's
pub(crate) fn titles_move_row(tab: &mux::tab::Tab, drawn: &Tab, idle_full: f64) -> bool {
    let active = tab.get_active_pane();
    let titles = pane_titles(active.as_ref(), &tab.iter_panes_ignoring_zoom());
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    lbterm_panel::claude::titles_move_row(drawn, &tab.get_title(), &titles, now, idle_full)
}

/// Every pane's title, the active pane's first, as a row reads them (lbterm_panel::claude::Tab)
fn pane_titles(active: Option<&Arc<dyn Pane>>, panes: &[mux::tab::PositionedPane]) -> Vec<String> {
    let active_id = active.map(|p| p.pane_id());
    active
        .map(|pane| pane.get_title())
        .into_iter()
        .chain(
            panes
                .iter()
                .filter(|pos| Some(pos.pane.pane_id()) != active_id)
                .map(|pos| pos.pane.get_title()),
        )
        .collect()
}

/// The Claude session a tab stands for, as its row's label takes it: the active pane's
/// `claude_session` user var, else any pane's.
pub(crate) fn tab_session(tab: &mux::tab::Tab) -> Option<String> {
    let active = tab.get_active_pane().map(|p| p.pane_id());
    let mut session = None;
    for pos in tab.iter_panes_ignoring_zoom() {
        let sid = pos.pane.copy_user_vars().remove("claude_session").filter(|s| !s.is_empty());
        if let Some(sid) = sid {
            if Some(pos.pane.pane_id()) == active {
                return Some(sid);
            }
            session.get_or_insert(sid);
        }
    }
    session
}

pub(crate) fn srgb(rgb: u32) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, 255)
}

fn rect(a: Area) -> RectF {
    euclid::rect(a.x, a.y, a.w, a.h)
}

fn pixels(a: Area) -> PixelRect {
    PixelRect::new(
        a.x.round() as i32,
        a.y.round() as i32,
        a.w.round().max(0.0) as u32,
        a.h.round().max(0.0) as u32,
    )
}

/// Columns a character takes in the panel's monospaced font.
/// One band of a light or a stroke: from `near` to `far` pixels out, its
/// colour at each end.
pub(crate) type Band = (f32, LinearRgba, f32, LinearRgba);

/// A light of colour `light` falling off over `reach` pixels in `steps`
/// bands, `want(d)` its alpha `d` pixels out. The GPU rounds a pixel's
/// alpha to 1/255 before blending, and near black each step shows. So a
/// band keeps one alpha, rounded up to a step, and the falloff rides in its
/// colour, which isn't rounded: what's stored is the light times the alpha
/// wanted, either way.
pub(crate) fn light_bands(light: u32, reach: f32, steps: usize, want: impl Fn(f64) -> f64) -> Vec<Band> {
    let (red, green, blue) = palette::bytes(light);
    let light = [red, green, blue].map(|c| palette::decode(c as f64 / 255.0));
    let knots: Vec<(f32, f64)> = (0..=steps)
        .map(|j| {
            let d = reach * j as f32 / steps as f32;
            (d, want(d as f64))
        })
        .collect();
    knots
        .windows(2)
        .filter_map(|pair| {
            let ((near, want_near), (far, want_far)) = (pair[0], pair[1]);
            let alpha = (want_near.max(want_far) * 255.0).ceil() / 255.0;
            if alpha <= 0.0 {
                return None;
            }
            let colour = |want: f64| {
                let k = (want / alpha) as f32;
                LinearRgba::with_components(
                    light[0] as f32 * k,
                    light[1] as f32 * k,
                    light[2] as f32 * k,
                    alpha.min(1.0) as f32,
                )
            };
            Some((near, colour(want_near), far, colour(want_far)))
        })
        .collect()
}

/// Where the Conductor's light goes round `path` (corners of `radius`), `steps` times a lap, as
/// its visual's offsets from a template `reach` round it: worked out once for each, and kept for
/// the next frames while they still ask for it (the GUI thread's own; a few pills at most)
fn light_offsets(path: Area, radius: f32, reach: f32, steps: usize) -> Vec<(f32, f32)> {
    use lbterm_motion::content::{fold, pair};
    thread_local! {
        static KEPT: std::cell::RefCell<Vec<(u64, Vec<(f32, f32)>)>> = Default::default();
    }
    let mut key = 0;
    for word in [pair(path.x, path.y), pair(path.w, path.h), pair(radius, reach), steps as u64] {
        fold(&mut key, word);
    }
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        if let Some((_, offsets)) = kept.iter().find(|(k, _)| *k == key) {
            return offsets.clone();
        }
        let offsets: Vec<(f32, f32)> = (0..steps)
            .map(|k| {
                let (x, y) = lbterm_panel::layout::outline_point(path, radius, k as f64 / steps as f64);
                (x - reach, y - reach)
            })
            .collect();
        // A few rows' pills at a time: older ones go first
        if kept.len() >= 16 {
            kept.remove(0);
        }
        kept.push((key, offsets.clone()));
        offsets
    })
}

fn column_width(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        let mut buf = [0u8; 4];
        termwiz::cell::grapheme_column_width(c.encode_utf8(&mut buf), None)
    }
}

impl crate::TermWindow {
    /// Draw the panel for this frame and, with the compositor, say what its
    /// visuals should show. Called by the paint pass after everything else
    /// but modals.
    pub fn paint_tab_panel(&mut self) -> anyhow::Result<()> {
        self.lbterm_panel.plan = None;
        self.lbterm_panel.said = None;
        self.lbterm_panel.glow = None;
        // LBTerm: the Conductor's word on a Hide or Show picked from a tab menu, before anything
        // reads the rows: a glint for one it says, the strip's notice for one 5 s went by without
        // (render/conductor_hide.rs). Panel or not, as the tab bar's menu offers it too
        self.lbterm_panel.hide_due = self.lbterm_hides_settle();
        let settings = self.config.lbterm_tab_panel.clone();
        if !settings.enabled {
            self.lbterm_panel.layout = None;
            self.lbterm_panel.hovered = None;
            self.lbterm_panel.hover.close();
            self.lbterm_panel.tasks_rows.clear();
            if !self.lbterm_panel.drawn.is_empty() {
                self.lbterm_panel.drawn = DrawnRows::default();
                self.lbterm_title_watch.drew(DrawnRows::default(), 0.0);
            }
            // No rail to play a glint or a light on: what was asked of it goes, the Conductor's
            // answer to a pick from the tab bar's menu included, rather than all playing at once,
            // stale, when a reload brings the panel back
            self.lbterm_panel.glints.clear();
            self.lbterm_panel.sends.clear();
            self.tab_panel_wake(self.lbterm_panel.hide_due);
            return Ok(());
        }
        self.lbterm_panel.hover.set_delays(
            Duration::from_millis(settings.open_delay_ms),
            Duration::from_millis(settings.close_delay_ms),
        );
        let composited = self
            .webgpu
            .as_ref()
            .map_or(false, |w| w.compositor.is_some());
        let motion = self.lbterm_motion();
        // Leaving while it opens turns it round, and coming back while it
        // closes opens it again, for as long as each takes on screen
        let on_screen = |curve: &Curve| {
            if composited && motion.enabled {
                Duration::from_secs_f64(curve.duration())
            } else {
                Duration::ZERO
            }
        };
        self.lbterm_panel
            .hover
            .set_motion(on_screen(&motion.panel_open), on_screen(&motion.panel_close));

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let mut rows = vec![];
        let mut changes = lbterm_panel::Changes::default();
        // The rows whose words move on each second as the rail shows them now, open or closed:
        // the clock's rate for the budget
        // LBTerm: the rail holds still while a menu's card shows over the window, open or fading
        // out: it neither opens, closes nor washes through or over the card, and goes where the
        // pointer has taken it once the card has gone (lbterm_panel::hover's `hold`; FLOWS.md, "A
        // menu, from what opens it to the end of its fade", step 10). A wake waits for a fading
        // card's end only while it holds the panel
        let (cards, fading) = self.lbterm_menu_cards_show(Instant::now());
        self.lbterm_panel.hover.hold(cards);
        self.lbterm_panel.held_due = fading.filter(|_| self.lbterm_panel.hover.held());
        let shown_open = self.lbterm_panel.hover.is_open();
        let mut ticking = 0;
        let (mut tabs, sessions, own) = self.tab_panel_tabs();
        let rail = self.tab_panel_rail();
        // The rail's tabs, row by row, once a paint: what draws, plans and marks the rows asks it
        let rail_ids: Vec<TabId> = rail.iter().map(|t| t.tab_id()).collect();
        // LBTerm: each row hidden from the Conductor or not, as a pick waiting for its word asked
        // or its file says (render/conductor_hide.rs), in the row's looks from this paint
        let hides = self.lbterm_hide_looks(&rail_ids, &sessions);
        for (tab, look) in tabs.iter_mut().zip(&hides) {
            tab.hidden = look.hidden;
        }
        for tab in &tabs {
            let (row, change) = lbterm_panel::row(tab, now, settings.idle_full_minutes * 60.0);
            ticking += change.next(shown_open).is_some_and(|c| c <= 1.0) as usize;
            changes = changes.min(change);
            rows.push(row);
        }
        // The tab links: a waiting tab's row again with its link, and the lines between rows
        let linked = self.tab_panel_links(&mut rows, &mut tabs, &rail, now, settings.idle_full_minutes * 60.0);
        ticking += linked.next(shown_open).is_some_and(|c| c <= 1.0) as usize;
        changes = changes.min(linked);
        self.lbterm_budget.borrow_mut().rail_ticking = ticking;
        let drawn: DrawnRows = Arc::new(rail.iter().map(|tab| tab.tab_id()).zip(tabs).collect());
        // ...and for the window's mux subscription, which decides a title alert against the rows
        // as drawn, not as the next paint will draw them (titles.rs, lbterm_title_alert_dropped)
        self.lbterm_title_watch
            .drew(Arc::clone(&drawn), settings.idle_full_minutes * 60.0);
        self.lbterm_panel.drawn = drawn;
        self.lbterm_panel.drawn_rows = rail_ids.clone();
        // The rows whose tab has parts still running that its statusline has listed, for the
        // card resting on the row shows (render/tasks_card.rs). Only while the pane says parts
        // run: a session that ended with tasks listed leaves its file, and its pane's records go
        self.lbterm_panel.tasks_rows = rows
            .iter()
            .zip(rail.iter())
            .map(|(row, tab)| {
                let listed = row.agents > 0
                    && tab_session(tab)
                        .and_then(|sid| crate::summaries::get(&sid))
                        .map_or(false, |s| s.tasks.is_some());
                listed.then(|| tab.tab_id())
            })
            .collect();

        // The Conductor (render/conductor.rs): its slot atop the rail, and what it's doing with
        // each tab's Claude sessions, as its hub says
        let head = self.lbterm_conductor_head();
        let hub = self
            .config
            .lbterm_conductor
            .as_ref()
            .map_or(false, |c| c.hub_pipe.is_some());
        let doing: Vec<Option<Doing>> = sessions
            .iter()
            .map(|tab| {
                tab.iter()
                    .filter(|_| hub)
                    .filter_map(|(sid, _)| crate::conductor_hub::doing(sid))
                    .max()
            })
            .collect();
        // Whether it acts on any session, here or not: its own pad's spectrum in Accept most and
        // Accept all (Lachlan's 7a), still in its mode's colour otherwise. Once a paint, as `doing`
        // is asked a session a paint; the hub's word of a change repaints, and next_change wakes
        // the rail when the last acting outline goes
        self.lbterm_panel.head_acting = hub && head.is_some() && crate::conductor_hub::acting();
        if hub {
            // The rail's tabs numbered from 1 as it shows them, the Conductor's own as 0. The
            // sessions' ids are moved in, not copied: the hub is told only when they've moved
            let own = own.into_iter().flat_map(|(tab, active)| {
                tab.into_iter().map(move |(sid, pane)| PaneFact {
                    session_id: sid,
                    pane,
                    tab: 0,
                    active,
                })
            });
            let facts: Vec<PaneFact> = own
                .chain(sessions.into_iter().enumerate().flat_map(|(i, tab)| {
                    let active = rows.get(i).map_or(false, |r| r.active);
                    tab.into_iter().map(move |(sid, pane)| PaneFact {
                        session_id: sid,
                        pane,
                        tab: i + 1,
                        active,
                    })
                }))
                .collect();
            crate::conductor_hub::panes(self.mux_window_id, facts);
        }

        // LBTerm: the Starting row at the end of the tabs, while a tab asked for is on its way: the
        // tab that came takes its place, and past its time it folds (render/new_tab.rs)
        let (arrived, starting_due) = self.lbterm_new_tab_starting();
        self.lbterm_panel.starting_due = starting_due;
        let starting = self.lbterm_starting_label().map(str::to_string);
        let layout = self.tab_panel_layout(rows.len() + starting.is_some() as usize, head.is_some())?;
        self.lbterm_panel.layout = Some(layout);
        let open = self.lbterm_panel.hover.is_open();
        let light = self.lbterm_panel.hover.light().level();
        // With the compositor the visuals are always drawn open and lit,
        // and DWM shows as much as the moment calls for
        let (drawn_open, glow) = if composited { (true, 1.0) } else { (open, light) };
        self.tab_panel_draw(&rows, &doing, head.as_ref(), &layout, &rail_ids, starting.as_deref(), drawn_open, glow)?;
        // The Starting row in this frame, for what a test judges it by (render/new_tab.rs)
        if starting.is_some() && layout.row_visible(rows.len()) {
            self.lbterm_new_tab_row_drawn();
        }

        if composited {
            self.tab_panel_light(&layout)?;
            self.tab_panel_glints(&layout, &rail_ids);
            // The lights along the suggested tab connections (render/connections.rs)
            self.tab_panel_connection_glow(&layout)?;
            let level = head.as_ref().map(|(_, level)| *level);
            let glow_curve = if motion.enabled { motion.panel_glow.clone() } else { Curve::Instant };
            let (rings, ring_template, light_template) = self.tab_panel_rings(&layout, &doing, level, &glow_curve)?;
            // The turning arc's template, from the first row whose arc turns: a parked row's is
            // drawn still and dimmed with its pad, never from this undimmed template (Row::arc_turns)
            if let Some(i) = (0..rows.len())
                .take_while(|&i| layout.row_visible(i))
                .find(|&i| rows[i].arc_turns())
            {
                let gl_state = self.render_state.as_ref().unwrap();
                let layer = gl_state.layer_for_zindex(SPIN_Z)?;
                let mut quads = layer.quad_allocator();
                let area = layout.spinner(i);
                let snapped = pixels(area);
                let area = Area::new(snapped.x as f32, snapped.y as f32, snapped.w as f32, snapped.h as f32);
                // The arc alone: the disc under it stays on each pad
                self.tab_panel_spinner(&mut quads, area, &layout, None, true)?;
            }
            // The Starting row's white arc, turning on its pad's corner (render/new_tab.rs)
            let start_spinner = starting
                .as_ref()
                .filter(|_| layout.row_visible(rows.len()))
                .map(|_| layout.spinner(rows.len()));
            if let Some(area) = start_spinner {
                let gl_state = self.render_state.as_ref().unwrap();
                let layer = gl_state.layer_for_zindex(START_SPIN_Z)?;
                let mut quads = layer.quad_allocator();
                let snapped = pixels(area);
                let area = Area::new(snapped.x as f32, snapped.y as f32, snapped.w as f32, snapped.h as f32);
                self.tab_panel_arc(&mut quads, area, &layout, None, true, palette::UNREAD)?;
            }
            let pick = |curve: &Curve| {
                if motion.enabled {
                    curve.clone()
                } else {
                    Curve::Instant
                }
            };
            // The wave runs only as the edge moves, so with motion off there's none
            let wave = if motion.enabled && settings.wave.enabled {
                self.tab_panel_wave(&layout, &settings.wave, &motion)?
            } else {
                None
            };
            self.lbterm_panel.plan = Some(PanelPlan {
                surface: pixels(layout.surface()),
                cap: pixels(layout.cap_surface()),
                seam: layout.seam(true).round() as i32,
                travel: layout.travel().round() as u32,
                panes_left: layout.panes_left().round() as i32,
                panes_right: layout.panes_right().round() as i32,
                open,
                edge: pick(if open { &motion.panel_open } else { &motion.panel_close }),
                glow: pick(&motion.panel_glow),
                light,
                hover: pick(&motion.hover),
                words: pick(if open { &motion.enter } else { &motion.exit }),
                words_delay: WORDS_DELAY * self.config.lbterm_motion.slowdown,
                spot: pixels(layout.spot_surface()),
                spot_core: pixels(layout.spot_core_surface()),
                sweep: pick(&motion.panel_sweep),
                layout,
                spinners: (0..rows.len())
                    .take_while(|&i| layout.row_visible(i))
                    .filter(|&i| rows[i].arc_turns())
                    .map(|i| pixels(layout.spinner(i)))
                    .collect(),
                // Still while the window can't be seen: a loop keeps DWM composing every refresh
                spin: lbterm_extras::work::loops_play(motion.enabled, self.lbterm_unseen()),
                start_spinner: start_spinner.map(pixels),
                rows: self.tab_panel_row_plans(&rows, &layout, &rail_ids, starting.as_deref(), arrived),
                row_motion: RowMotion {
                    slide: pick(&motion.layout),
                    enter: pick(&motion.enter),
                    exit: pick(&motion.exit),
                    swap: pick(&motion.swap),
                    hover: pick(&motion.hover),
                    marker: pick(&motion.focus_move),
                },
                marker: self.tab_panel_marker(&rows, &layout, &rail_ids),
                progress: self.tab_panel_progress(&rows, &layout, &rail_ids),
                dragged: self.tab_panel_dragged(&layout),
                rings,
                ring_template,
                light_template,
                wave,
                cards,
            });
        } else {
            // Only the composited paint plays glints and lights (`tab_panel_glints`): without it
            // they'd wait for one that never comes
            self.lbterm_panel.glints.clear();
            self.lbterm_panel.sends.clear();
            // Nor fades the Conductor's pad's ring: its spectrum is drawn still in the body
            self.lbterm_panel.pad_ring = Default::default();
            self.lbterm_panel.ring_due = None;
        }

        // What it showed, for the state file (lbterm/STATE.md)
        if crate::state_probe::on() {
            self.lbterm_panel.said = Some(self.tab_panel_said(&rows, &doing, &hides, head.as_ref(), &layout, starting.as_deref()));
        }

        // Ages and timers move on with the clock, not with output: the idle bars' steps always,
        // the ages only while the open panel shows them (a paint as it opens has them right). An
        // outline the hub let go of goes once it has lingered
        let aged = changes
            .next(open)
            .map(|next| Instant::now() + Duration::from_secs_f64(next + 0.05));
        let outlines = if hub {
            crate::conductor_hub::next_change()
        } else {
            None
        };
        self.lbterm_panel.due = (aged, outlines);
        let due = [aged, outlines, self.lbterm_panel.hover.deadline(), self.lbterm_links.line_hover.deadline(), starting_due, self.lbterm_panel.held_due, self.lbterm_panel.hide_due, self.lbterm_panel.ring_due]
            .iter()
            .flatten()
            .min()
            .copied();
        // A clock in view repaints at rest on purpose: once a second for each row counting
        // seconds, and the minutes and idle steps besides (termwindow/budget.rs)
        if let Some(at) = aged.filter(|at| Some(*at) == due) {
            let per_second = self.lbterm_budget.borrow().clock_rate();
            self.lbterm_budget_tick(lbterm_extras::budget::Purpose::Clock, at, per_second);
        }
        self.tab_panel_wake(due);
        Ok(())
    }

    /// The tab links on this paint's rail (lbterm/LINKS.md): each row of a tab that waits on
    /// another made again with its link, which names the tab it waits on by that tab's own row
    /// label (else its session's label), and the lines between rows laid out. When those rows
    /// next read differently
    fn tab_panel_links(
        &mut self,
        rows: &mut [Row],
        tabs: &mut [Tab],
        rail: &[Arc<mux::tab::Tab>],
        now: f64,
        idle_full: f64,
    ) -> lbterm_panel::Changes {
        let Some(file) = self.config.lbterm_links.as_ref().map(|l| l.file.clone()) else {
            self.lbterm_panel.links = RailLinks::default();
            return lbterm_panel::Changes::default();
        };
        // What it asks of the file that it doesn't say yet shows already (tab_links.rs)
        let (said, pending) = self.lbterm_links_pending(crate::state_files::links(&file));
        // A link asked of the file shows on the rail open or closed, as a step does
        let mut next = lbterm_panel::Changes {
            age: None,
            step: pending.map(|due| due.saturating_duration_since(Instant::now()).as_secs_f64()),
        };
        let sessions: Vec<Option<String>> = rail.iter().map(|t| tab_session(t)).collect();
        let mut labels: HashMap<String, String> = sessions
            .iter()
            .zip(rows.iter())
            .filter_map(|(s, r)| Some((s.clone()?, r.label.clone())))
            .collect();
        // Those waited on that aren't on this rail, and the members of its connections that
        // aren't, by their sessions' labels
        let away = said.links.iter().map(|l| &l.on).chain(
            said.connections
                .iter()
                .filter(|c| c.state != links::ConnectionState::Declined)
                .flat_map(|c| c.members.iter()),
        );
        for sid in away {
            if !labels.contains_key(sid) {
                if let Some(label) = crate::summaries::get(sid).and_then(|s| s.label.clone()) {
                    labels.insert(sid.clone(), label);
                }
            }
        }
        if !said.links.is_empty() {
            let open = open_sessions();
            let label = |sid: &str| -> String { labels.get(sid).cloned().unwrap_or_default() };
            for (i, sid) in sessions.iter().enumerate() {
                let (Some(sid), Some(tab)) = (sid, tabs.get_mut(i)) else {
                    continue;
                };
                let Some(link) = links::row_link(&said, sid, |s| open.contains(s)) else {
                    continue;
                };
                tab.link = Some(TabLink {
                    state: link_state(link.standing),
                    on: label(&link.on),
                    what: link.what,
                    note: link.note,
                });
                let (row, change) = lbterm_panel::row(tab, now, idle_full);
                rows[i] = row;
                next = next.min(change);
            }
        }
        let refs: Vec<Option<&str>> = sessions.iter().map(|s| s.as_deref()).collect();
        // The links' lines and the connections' share the lanes
        let laid = links::lay_out(&refs, &said, lbterm_panel::layout::LINK_LANES);
        let pieces = (0..rows.len()).map(|i| laid.pieces(i)).collect();
        let links::Lanes { lines, joins } = laid;
        // The Conductor's count: a suggestion with one member here counts, line or not
        let suggested = said.suggested_on(&sessions);
        self.lbterm_panel.links = RailLinks { sessions, lines, joins, pieces, said, labels, suggested };
        next
    }

    /// A pane's OSC 7501 records changed (Alert::ProgramStatus). The rail, the window strip and
    /// the projects card read them as they paint (panel_tab), so a paint is all it takes, and
    /// only one that shows something: in the window holding the pane when its tab's row changes,
    /// and in any other only while its projects card, which counts every window's tabs, is open.
    /// Every window painted for every report, in full: each pane's lines walked, for a working
    /// session's subagent's words, which no row shows
    pub(crate) fn lbterm_program_status(&mut self, pane_id: PaneId) {
        let repaint = if self.lbterm_holds_pane(pane_id) {
            self.tab_panel_status_moves_row(pane_id)
        } else {
            self.lbterm_projects_open()
        };
        log::debug!(
            target: "lbterm::status",
            "program status pane {pane_id} window {}: {}",
            self.mux_window_id,
            if repaint { "repaint" } else { "nothing shown changed" }
        );
        if repaint {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// Whether the OSC 7501 records of the tab holding `pane_id` now would change its row as the
    /// last paint drew it (lbterm_panel::claude::programs_move_row). Yes for a tab the rail drew
    /// no row for: with the rail off, or the Conductor's own tab, the strip still reads them
    fn tab_panel_status_moves_row(&self, pane_id: PaneId) -> bool {
        let Some((tab, drawn)) = self.tab_panel_drawn(pane_id) else {
            return true;
        };
        // As panel_tab reads them: every pane's, in order
        let programs: Vec<Program> = tab
            .iter_panes_ignoring_zoom()
            .into_iter()
            .flat_map(|pos| pos.pane.get_program_status().into_iter().map(program))
            .collect();
        let (now, idle_full) = self.tab_panel_now();
        lbterm_panel::claude::programs_move_row(drawn, &programs, now, idle_full)
    }

    /// A pane's title, icon title or tab title changed (titles.rs). A background tab's reaches
    /// its rail row only by a paint, and nothing else paints for it (its pane's output repaints
    /// only the open tab, mux_pane_output_event), so the window repaints when the row, as the
    /// last paint drew it, would read otherwise with its titles now
    /// (lbterm_panel::claude::titles_move_row): a new title does, Claude Code's ◐ and ◑ turning
    /// every 960 ms don't. A tab the rail drew no row for (the rail off, the Conductor's own)
    /// shows its title only in the strip and the caption when it's the open one, which its
    /// output and the title update paint
    pub(crate) fn lbterm_panel_title_changed(&self, pane_id: PaneId) {
        let moved = self.tab_panel_drawn(pane_id).map_or(false, |(tab, drawn)| {
            let (_, idle_full) = self.tab_panel_now();
            titles_move_row(&tab, drawn, idle_full)
        });
        log::debug!(
            target: "lbterm::title",
            "title pane {pane_id} window {}: {}",
            self.mux_window_id,
            if moved { "repaint" } else { "nothing shown changed" }
        );
        if moved {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// The window's tab holding `pane_id`, and that tab as the last paint made its row: None
    /// when the rail drew no row for it
    fn tab_panel_drawn(&self, pane_id: PaneId) -> Option<(Arc<mux::tab::Tab>, &Tab)> {
        let tab = {
            let mux = Mux::get();
            let window = mux.get_window(self.mux_window_id)?;
            let tab = window.iter_tabs().find(|tab| tab.contains_pane(pane_id)).cloned()?;
            tab
        };
        let drawn = self.lbterm_panel.drawn.get(&tab.tab_id())?;
        Some((tab, drawn))
    }

    /// Now in Unix seconds, and the seconds the idle timer fills over, as paint_tab_panel takes
    /// them
    fn tab_panel_now(&self) -> (f64, f64) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        (now, self.config.lbterm_tab_panel.idle_full_minutes * 60.0)
    }

    /// The pad of the tab holding `pane_id` glints at the next paint: a program typed into it
    /// (mux::lbterm_typed), so what an agent does in a tab shows where it happened
    pub(crate) fn lbterm_panel_glint(&mut self, pane_id: PaneId) {
        if let Some((_, _, tab)) = Mux::get().resolve_pane_id(pane_id) {
            self.lbterm_panel_glint_tab(tab);
        }
    }

    /// Tab `tab`'s pad glints at the next paint: a program typed into it, or it was renamed
    pub(crate) fn lbterm_panel_glint_tab(&mut self, tab: TabId) {
        if !self.lbterm_panel.glints.contains(&tab) {
            self.lbterm_panel.glints.push(tab);
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A tab dragged from another window is over this one (`on`), or has gone
    pub(crate) fn lbterm_panel_drop_hint(&mut self, on: bool) {
        if self.lbterm_panel.drop_hint != on {
            self.lbterm_panel.drop_hint = on;
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// The pads asked to glint, lit over the panel (render/lights.rs), and the lights sent up the
    /// rail from a pad to the Conductor's (`sends`). `tabs` is the rail's tabs, row by row. Under a
    /// menu's card they wait, as the rows do (Lachlan's 7b of 2026-11-08: Copy link's glint asked
    /// as its card fades out, and Hide from Conductor's light, picked as it does), and start at the
    /// paint that lets the rail go, from where its pad is then, in the frame the row dissolves into
    /// its hidden look
    fn tab_panel_glints(&mut self, layout: &Layout, tabs: &[TabId]) {
        if self.lbterm_panel.hover.held() {
            return;
        }
        self.tab_panel_sends(layout, tabs);
        if self.lbterm_panel.glints.is_empty() {
            return;
        }
        let glints = std::mem::take(&mut self.lbterm_panel.glints);
        for tab in glints {
            // The Conductor's tab glints on its pad
            let pad = if Some(tab) == self.lbterm_panel.conductor_tab && layout.head {
                layout.head_pad()
            } else {
                let Some(i) = tabs.iter().position(|t| *t == tab).filter(|&i| layout.row_visible(i)) else {
                    continue;
                };
                layout.pad(i)
            };
            self.lbterm_light_this_frame(crate::termwindow::compositor::Light::Pad {
                rect: pixels(pad.grow(layout.px(2.0))),
                radius: (layout.px(6.0)) as f64,
            });
        }
    }

    /// Each light asked for by a hide or a show (`sends`), from its tab's pad, as its glint, to the
    /// Conductor's (lbterm_motion::send; Lachlan's 6a of 2026-11-01). None where the rail has no
    /// Conductor's pad, or the tab has no row in view (past the rail's last, or gone): the hide
    /// still shows in its row's look
    fn tab_panel_sends(&mut self, layout: &Layout, tabs: &[TabId]) {
        if self.lbterm_panel.sends.is_empty() {
            return;
        }
        let sends = std::mem::take(&mut self.lbterm_panel.sends);
        if !layout.head {
            return;
        }
        let glint = |pad: Area| pixels(pad.grow(layout.px(2.0)));
        let to = glint(layout.head_pad());
        for tab in sends {
            let Some(i) = tabs.iter().position(|t| *t == tab).filter(|&i| layout.row_visible(i)) else {
                continue;
            };
            self.lbterm_light_this_frame(crate::termwindow::compositor::Light::Send {
                from: glint(layout.pad(i)),
                to,
                radius: layout.px(6.0) as f64,
            });
        }
    }

    /// Each visible row for the compositor: its tab, its slot, and hashes of what it shows (the
    /// closed rail's part, with its tab's colour and its pieces of the links' lines, and the open
    /// panel's words: lbterm_panel::look), so a row that moves is followed and one whose look
    /// changes dissolves. `tabs` is the rail's tabs, row by row
    fn tab_panel_row_plans(
        &self,
        rows: &[Row],
        layout: &Layout,
        tabs: &[TabId],
        starting: Option<&str>,
        arrived: Option<TabId>,
    ) -> Vec<RowPlan> {
        let mut plans: Vec<RowPlan> = rows
            .iter()
            .enumerate()
            .take_while(|&(i, _)| layout.row_visible(i))
            .filter_map(|(i, row)| {
                let key = *tabs.get(i)?;
                let (rail, words_look) = lbterm_panel::look::looks(row);
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                rail.hash(&mut hash);
                crate::tabcolors::rgb(key).hash(&mut hash);
                // Its pieces of the tab links' lines are drawn in its slot
                self.lbterm_panel.links.pieces.get(i).hash(&mut hash);
                Some(RowPlan {
                    key,
                    slot: pixels(layout.slot(i)),
                    look: hash.finish(),
                    words_look,
                    hovered: self.lbterm_panel.hovered == Some(i),
                    // LBTerm: the tab the Starting row was waiting for grows out of it, dissolving
                    // from its look where it is (render/new_tab.rs)
                    was: (Some(key) == arrived).then_some(super::new_tab::STARTING_KEY),
                })
            })
            .collect();
        // LBTerm: the Starting row, after the tabs (render/new_tab.rs)
        if let Some(label) = starting.filter(|_| layout.row_visible(rows.len())) {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            label.hash(&mut hash);
            plans.push(RowPlan {
                key: super::new_tab::STARTING_KEY,
                slot: pixels(layout.slot(rows.len())),
                look: hash.finish(),
                words_look: hash.finish(),
                hovered: false,
                was: None,
            });
        }
        plans
    }

    /// Each visible row's bars on the open panel, its tab and which bar (0 the program's or the
    /// hub's guess of it, then each subagent's), where, and how far along. `tabs` is the rail's
    /// tabs, row by row
    fn tab_panel_progress(&self, rows: &[Row], layout: &Layout, tabs: &[TabId]) -> Vec<(usize, u8, PixelRect, f64)> {
        rows.iter()
            .enumerate()
            .take_while(|&(i, _)| layout.row_visible(i))
            .filter_map(|(i, row)| Some((i, row, *tabs.get(i)?)))
            .flat_map(|(i, row, key)| {
                let n = row.agent_bars.len();
                let main = row.progress.map(|p| (key, 0, pixels(layout.progress_bar(i)), p as f64 / 100.0));
                let agents = row.agent_bars.iter().enumerate().map(move |(k, p)| {
                    (key, k as u8 + 1, pixels(layout.agent_bar(i, k, n, row.progress.is_some())), *p as f64 / 100.0)
                });
                main.into_iter().chain(agents)
            })
            .collect()
    }

    /// The row being dragged along the panel and how far the pointer has it from its slot:
    /// it follows the pointer, lifted, while the others slide out of its way
    fn tab_panel_dragged(&self, layout: &Layout) -> Option<(usize, f64)> {
        let (tab, y) = self.lbterm_dragged_row()?;
        let i = self.tab_panel_row_of(tab)?;
        if !layout.row_visible(i) {
            return None;
        }
        let slot = layout.slot(i);
        Some((tab, y - (slot.y + slot.h / 2.0) as f64))
    }

    /// The open tab's row and the top of its slot, for its ring and bar to glide to: the
    /// Conductor's slot while its tab is the open one. `tabs` is the rail's tabs, row by row
    fn tab_panel_marker(&self, rows: &[Row], layout: &Layout, tabs: &[TabId]) -> Option<(usize, i32)> {
        if layout.head && self.tab_panel_conductor_open() {
            // Its view is no tab: a key of its own, so the marker glides there as to a tab
            let key = if self.lbterm_view_is_open() {
                super::conductor_view::VIEW_KEY
            } else {
                self.lbterm_panel.conductor_tab?
            };
            return Some((key, pixels(layout.head_slot()).y));
        }
        let i = (0..rows.len()).take_while(|&i| layout.row_visible(i)).find(|&i| rows[i].active)?;
        let key = *tabs.get(i)?;
        Some((key, pixels(layout.slot(i)).y))
    }

    /// What the panel shows this frame, for the state file (lbterm/STATE.md)
    fn tab_panel_said(
        &self,
        rows: &[Row],
        doing: &[Option<Doing>],
        hides: &[lbterm_extras::conductor::hide::Look],
        head: Option<&(HeadSays, Level)>,
        layout: &Layout,
        starting: Option<&str>,
    ) -> lbterm_extras::state::Rail {
        use lbterm_extras::state;
        let rect = |a: Area| state::Rect::round(a.x as f64, a.y as f64, a.w as f64, a.h as f64);
        let rail = self.tab_panel_rail_ids();
        // The window's index of each tab, the window's lock let go before anything else asks
        let index: HashMap<TabId, usize> = Mux::get()
            .get_window(self.mux_window_id)
            .map(|w| w.iter_tabs().enumerate().map(|(i, t)| (t.tab_id(), i)).collect())
            .unwrap_or_default();
        let conductor_open = layout.head && self.tab_panel_conductor_open();
        let mut said_rows: Vec<state::RailRow> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let tab = rail.get(i).copied();
                state::RailRow {
                    row: i,
                    tab: tab.map(|t| t as u64),
                    tab_index: tab.and_then(|t| index.get(&t).copied()).unwrap_or(i),
                    visible: layout.row_visible(i),
                    rect: rect(layout.slot(i)),
                    pad: rect(layout.pad(i)),
                    label: row.label.clone(),
                    title: row.title.clone(),
                    project: row.project.clone(),
                    state: match row.state {
                        lbterm_panel::RowState::NoClaude => "none",
                        lbterm_panel::RowState::Working => "working",
                        lbterm_panel::RowState::Waiting => "waiting",
                        lbterm_panel::RowState::Idle => "idle",
                        lbterm_panel::RowState::Error => "error",
                        lbterm_panel::RowState::Linked => "linked",
                        lbterm_panel::RowState::Stuck => "stuck",
                    }
                    .to_string(),
                    status: row.status.clone(),
                    message: row.message.clone(),
                    progress: row.progress,
                    estimated: row.estimated,
                    left: row.left.clone(),
                    agent_bars: row.agent_bars.clone(),
                    agents: row.agents,
                    waiting: row.waiting,
                    extras: row.extras,
                    asks: row.asks,
                    quiet: row.quiet,
                    link: row.link.map(|l| {
                        match l {
                            LinkState::Waiting => "waiting",
                            LinkState::Telling => "telling",
                            LinkState::Told => "told",
                            LinkState::Stuck => "stuck",
                            LinkState::Failed => "failed",
                            LinkState::Closed => "closed",
                        }
                        .to_string()
                    }),
                    connected: self
                        .lbterm_panel
                        .links
                        .sessions
                        .get(i)
                        .and_then(|s| s.as_deref())
                        .map(|sid| self.lbterm_panel.links.said.made_of(sid).map(|c| c.id.clone()).collect())
                        .unwrap_or_default(),
                    unread: row.unread,
                    age: row.age.clone(),
                    // As tab_panel_pad draws them: the lit ones, where the row has room for them
                    bars: if layout.has_timer(i) {
                        lbterm_panel::idle::lit(row.bars).map(|(_, bar)| bar.name().to_string()).collect()
                    } else {
                        vec![]
                    },
                    cost: row.cost.clone(),
                    prompted: row.prompted.clone(),
                    active: row.active,
                    hovered: self.lbterm_panel.hovered == Some(i),
                    colour: tab
                        .and_then(crate::tabcolors::rgb)
                        .map(|(r, g, b)| state::hex(u32::from_be_bytes([0, r, g, b]))),
                    conductor: doing.get(i).and_then(|d| d.as_ref()).map(|d| {
                        match d {
                            Doing::Thinking => "thinking",
                            Doing::Acting => "acting",
                        }
                        .to_string()
                    }),
                    pending: false,
                    hidden: row.hidden,
                    hidden_pending: hides.get(i).map_or(false, |h| h.pending),
                    parked: row.parked,
                }
            })
            .collect();
        // LBTerm: the Starting row, after the tabs (render/new_tab.rs)
        if let Some(label) = starting {
            let n = rows.len();
            said_rows.push(state::RailRow {
                row: n,
                // No tab yet: an id of 0 would be a real tab's (PITFALLS 52)
                tab: None,
                tab_index: n,
                visible: layout.row_visible(n),
                rect: rect(layout.slot(n)),
                pad: rect(layout.pad(n)),
                label: label.to_string(),
                title: lbterm_extras::newtab::STARTING.to_string(),
                project: String::new(),
                state: "starting".to_string(),
                status: String::new(),
                message: String::new(),
                progress: None,
                estimated: false,
                left: String::new(),
                agent_bars: vec![],
                agents: 0,
                waiting: 0,
                extras: false,
                asks: false,
                quiet: false,
                link: None,
                connected: vec![],
                unread: false,
                age: String::new(),
                bars: vec![],
                cost: String::new(),
                prompted: String::new(),
                active: false,
                hovered: false,
                colour: None,
                conductor: None,
                pending: true,
                hidden: false,
                hidden_pending: false,
                parked: false,
            });
        }
        let open_row = if conductor_open {
            None
        } else {
            (0..rows.len())
                .take_while(|&i| layout.row_visible(i))
                .find(|&i| rows[i].active)
        };
        let hover = &self.lbterm_panel.hover;
        state::Rail {
            open: hover.is_open(),
            // As the screen shows it: held under a menu's card, the held state's
            phase: hover.shown_phase().name().to_string(),
            held: hover.held(),
            light: match hover.light() {
                lbterm_panel::Light::Rest => "rest",
                lbterm_panel::Light::Pending => "pending",
                lbterm_panel::Light::Full => "full",
            }
            .to_string(),
            rect: rect(layout.panel(hover.is_open())),
            // The open panel's words cover the labels (tab_panel_draw's `chrome.open`)
            labels: if hover.is_open() { "faded" } else { "shown" }.to_string(),
            rows: said_rows,
            hovered: self.lbterm_panel.hovered,
            open_row,
            conductor: head.map(|(says, level)| state::ConductorPad {
                level: match level {
                    Level::Off => "off",
                    Level::ReadOnly => "read",
                    Level::AcceptMost => "most",
                    Level::AcceptAll => "all",
                }
                .to_string(),
                label: says.label.clone(),
                remote: says.mark.is_some(),
                tab: self.lbterm_panel.conductor_tab.map(|t| t as u64),
                open: conductor_open,
                hovered: self.lbterm_panel.head_hovered,
                outline: self.tab_panel_head_outline(*level).name().to_string(),
                acting: self.lbterm_panel.head_acting,
                pad: rect(layout.head_pad()),
                asks: self.lbterm_conductor_asks(),
                suggested: self.lbterm_panel.links.suggested,
                badge: self
                    .tab_panel_number_font()
                    .ok()
                    .and_then(|(_, _, font)| words::head_badge(says, layout, font, column_width))
                    .map(rect),
                mark: says.mark.is_some().then(|| rect(layout.head_mark())),
            }),
            dragging: self
                .lbterm_dragged_row()
                .and_then(|(tab, _)| self.tab_panel_row_of(tab)),
            drop_hint: self.lbterm_panel.drop_hint,
            links: self
                .lbterm_panel
                .links
                .lines
                .iter()
                .map(|l| state::RailLine {
                    waiter: l.waiter,
                    on: l.on,
                    lane: l.lane,
                    look: l.look.name().to_string(),
                    x: layout.link_lane(l.lane) as f64,
                    top: layout.link_end(l.waiter.min(l.on)).1 as f64,
                    bottom: layout.link_end(l.waiter.max(l.on)).1 as f64,
                })
                .collect(),
            connections: self
                .lbterm_panel
                .links
                .joins
                .iter()
                .filter_map(|j| {
                    let c = self.lbterm_panel.links.said.connections.get(j.connection)?;
                    Some(state::RailConnection {
                        id: c.id.clone(),
                        state: c.state.name().to_string(),
                        lane: j.lane,
                        rows: j.rows.clone(),
                        x: layout.link_lane(j.lane) as f64,
                        ys: j.rows.iter().map(|&r| layout.link_end(r).1 as f64).collect(),
                        mark_x: layout.link_end(j.rows[0]).0 as f64,
                    })
                })
                .collect(),
            // The drag's paint, after the panel's, fills these in (render/link_drag.rs)
            link_drag: None,
            line_card: None,
        }
    }

    /// The pointer at `x`, `y` on the panel, for the state file: on the Conductor's pad, on a
    /// row, or elsewhere on it (lbterm/STATE.md)
    pub(crate) fn tab_panel_under(&self, x: f32, y: f32) -> Option<(&'static str, Option<usize>)> {
        let layout = self.lbterm_panel.layout.as_ref()?;
        let open = self.lbterm_panel.hover.is_open();
        if layout.head && layout.head_at(x, y, open) {
            return Some(("rail.conductor", None));
        }
        if let Some(row) = layout.row_at(x, y, open) {
            return Some(("rail.row", Some(row)));
        }
        layout.inside(x, y, open).then_some(("rail", None))
    }

    /// The panel's hover waits under way at `now`: its rest before opening, or its grace
    /// before closing (lbterm/STATE.md)
    pub(crate) fn tab_panel_waits(&self, now: Instant) -> Option<lbterm_extras::state::Wait> {
        self.lbterm_panel.layout.as_ref()?;
        let hover = &self.lbterm_panel.hover;
        let due = hover.deadline()?;
        let name = match hover.phase() {
            lbterm_panel::Phase::Opening => "rail.open",
            lbterm_panel::Phase::Closing => "rail.close",
            _ => return None,
        };
        Some(lbterm_extras::state::Wait {
            name: name.to_string(),
            left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
        })
    }

    /// The rail's tabs, in order: the window's, all but the Conductor's, whose pad stands for it
    /// (lbterm_extras::conductor::rail). Row `i` is the rail's tab `i`.
    pub(crate) fn tab_panel_rail(&self) -> Vec<Arc<mux::tab::Tab>> {
        let mux = Mux::get();
        let tabs: Vec<_> = match mux.get_window(self.mux_window_id) {
            Some(window) => window.iter_tabs().cloned().collect(),
            None => return vec![],
        };
        let conductor = self
            .lbterm_panel
            .conductor_tab
            .and_then(|id| tabs.iter().position(|t| t.tab_id() == id));
        conductor::rail(tabs.len(), conductor)
            .into_iter()
            .map(|i| Arc::clone(&tabs[i]))
            .collect()
    }

    /// The rail's tabs as the tab menu takes them (`lbterm_extras::menu::RailTab`), for Wait on…
    /// and Close finished tabs (render/tab_menu.rs): each tab the last paint drew a row for, in the
    /// rail's order, read once as the menu opens and again when Close finished tabs is picked.
    /// Everything is the row as the rail draws it: its status word, unread dot, agents and tasks,
    /// its pad's colour, the label under its pad, and whether it waits on another tab (`linked`:
    /// a link the links file holds, or one asked of it the rail draws already, which a tab that
    /// waits isn't finished either way, PITFALLS 153); its mark is its objectives'. A tab the rail
    /// hasn't drawn (no panel) isn't one
    pub(crate) fn tab_panel_menu_rail(&self) -> Vec<lbterm_extras::menu::RailTab> {
        let (now, idle_full) = self.tab_panel_now();
        let conductor = self.lbterm_panel.conductor_tab;
        self.tab_panel_rail()
            .iter()
            .filter_map(|tab| {
                let id = tab.tab_id();
                let drawn = self.lbterm_panel.drawn.get(&id)?;
                let shown = lbterm_panel::row(drawn, now, idle_full).0;
                let session = tab_session(tab);
                let mark = session
                    .as_deref()
                    .and_then(crate::summaries::get)
                    .and_then(|s| s.objectives.as_ref().and_then(|o| o.mark()));
                Some(lbterm_extras::menu::RailTab {
                    tab: id,
                    label: shown.label.clone(),
                    claude: session.is_some(),
                    conductor: Some(id) == conductor,
                    linked: drawn.link.is_some(),
                    status: shown.status,
                    unread: shown.unread,
                    agents: shown.agents,
                    waiting: shown.waiting,
                    mark,
                    colour: palette::pad(shown.state),
                })
            })
            .collect()
    }

    /// The window's tabs as the side buttons' hops take them (`lbterm_extras::hop::Hop`,
    /// termwindow/hop.rs), in the window's order, the Conductor's included, read at the press: each
    /// one's parked flag (an atomic load) and, for a tab the last paint drew a row for, whether and
    /// since when that row waits on him (`lbterm_panel::waits_on_you`, the row worked out again from
    /// what it was drawn from, at `now`); with the open tab's place, and the tabs shown newest
    /// first. A window with no rail has no rows, so nothing waits
    pub(crate) fn tab_panel_hops(&self) -> (Vec<lbterm_extras::hop::Hop>, Option<usize>, Vec<TabId>) {
        let (now, idle_full) = self.tab_panel_now();
        let conductor = self.lbterm_panel.conductor_tab;
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            return (vec![], None, vec![]);
        };
        let open = window.get_active_tab_idx();
        let hops = window
            .iter_tabs()
            .map(|tab| {
                let id = tab.tab_id();
                lbterm_extras::hop::Hop {
                    tab: id,
                    parked: tab.lbterm_parked(),
                    conductor: Some(id) == conductor,
                    waits: self
                        .lbterm_panel
                        .drawn
                        .get(&id)
                        .and_then(|drawn| lbterm_panel::waits_on_you(drawn, now, idle_full))
                        .map(|(asks, since)| lbterm_extras::hop::Waits { asks, since }),
                }
            })
            .collect::<Vec<_>>();
        let open = (open < hops.len()).then_some(open);
        (hops, open, self.lbterm_panel.recent.tabs().to_vec())
    }

    /// The tab the last paint drew on row `row`, while it's still in this window: what the
    /// pointer is on, where `tab_panel_rail().get(row)` is whatever tab the window lists there
    /// now, which after a close and before the next paint is the one that slid up into the row
    pub(crate) fn tab_panel_drawn_tab(&self, row: usize) -> Option<TabId> {
        let id = *self.lbterm_panel.drawn_rows.get(row)?;
        let mux = Mux::get();
        let window = mux.get_window(self.mux_window_id)?;
        window.get_tab_idx_for_id(id).map(|_| id)
    }

    /// The rail's tabs' ids, row by row
    fn tab_panel_rail_ids(&self) -> Vec<TabId> {
        self.tab_panel_rail().iter().map(|t| t.tab_id()).collect()
    }

    /// The window's index of the tab on row `row`
    pub(crate) fn tab_panel_tab_index(&self, row: usize) -> Option<usize> {
        let id = self.tab_panel_rail().get(row)?.tab_id();
        Mux::get().get_window(self.mux_window_id)?.get_tab_idx_for_id(id)
    }

    /// The row of tab `tab`, if the rail shows it
    pub(crate) fn tab_panel_row_of(&self, tab: TabId) -> Option<usize> {
        self.tab_panel_rail().iter().position(|t| t.tab_id() == tab)
    }

    /// Whether the Conductor's tab is the window's open one, or its view is open: its pad has
    /// the open tab's ring
    pub(crate) fn tab_panel_conductor_open(&self) -> bool {
        if self.lbterm_view_is_open() {
            return true;
        }
        let Some(id) = self.lbterm_panel.conductor_tab else {
            return false;
        };
        Mux::get()
            .get_window(self.mux_window_id)
            .and_then(|w| w.get_active_tab().map(|t| t.tab_id()))
            == Some(id)
    }

    /// Keep the Conductor's tab first in the window, after this paint (conductor::keep_first)
    fn tab_panel_conductor_first(&self, tab: TabId) {
        let Some(window) = self.window.clone() else {
            return;
        };
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            let mux = Mux::get();
            let Some(mut window) = mux.get_window_mut(tw.mux_window_id) else {
                return;
            };
            if let Some(from) = window.get_tab_idx_for_id(tab).filter(|&i| i != 0) {
                window.lbterm_move_tab(from, 0);
                log::info!("conductor: its tab {tab} moved from {from} to first");
            }
        })));
    }

    /// Every tab of this window on the rail, as the panel needs it, and with the Conductor in
    /// the config each one's Claude sessions and their panes' server ids, and the Conductor's own
    /// tab's (whether it's the open tab). The Conductor's tab is found here and kept first.
    /// Titles and user vars are in memory; working directories come from
    /// [`Self::tab_panel_fetch_cwds`], off this thread.
    fn tab_panel_tabs(&mut self) -> (Vec<Tab>, Vec<Vec<(String, u64)>>, Option<(Vec<(String, u64)>, bool)>) {
        let mux = Mux::get();
        let (tabs, active) = match mux.get_window(self.mux_window_id) {
            Some(window) => (
                window.iter_tabs().cloned().collect::<Vec<_>>(),
                window.get_active_tab_idx(),
            ),
            None => return (vec![], vec![], None),
        };
        let conductor = self.config.lbterm_conductor.is_some();
        let mut sessions: Vec<Vec<(String, u64)>> = tabs
            .iter()
            .map(|tab| {
                if !conductor {
                    return vec![];
                }
                tab.iter_panes_ignoring_zoom()
                    .into_iter()
                    .filter_map(|pos| {
                        let sid = pos.pane.copy_user_vars().remove("claude_session")?;
                        (!sid.is_empty()).then(|| (sid, server_pane_id(&pos.pane)))
                    })
                    .collect()
            })
            .collect();
        // The Conductor's own tab: the one holding the session its state file names. With a seat
        // there's none: its view stands for it (render/conductor_view.rs)
        let seat = self.lbterm_view_on();
        let found = (!seat).then(|| self.lbterm_conductor_said()).flatten().and_then(|said| {
            let sid = conductor::session(&said)?.to_string();
            let ids: Vec<Vec<&str>> = sessions
                .iter()
                .map(|tab| tab.iter().map(|(s, _)| s.as_str()).collect())
                .collect();
            conductor::tab_of(&sid, &ids)
        });
        let found_id = found.map(|i| tabs[i].tab_id());
        if found_id != self.lbterm_panel.conductor_tab {
            log::info!("conductor: its tab is {found_id:?} (index {found:?})");
        }
        self.lbterm_panel.conductor_tab = found_id;
        if let (Some(_), Some(id)) = (conductor::keep_first(found), found_id) {
            self.tab_panel_conductor_first(id);
        }
        let own = found.map(|i| (std::mem::take(&mut sessions[i]), i == active));

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        if let Some(tab) = tabs.get(active) {
            self.lbterm_panel.seen.insert(tab.tab_id(), now);
            // One compare while it's the tab shown last; the window's tabs walked only when not
            self.lbterm_panel
                .recent
                .shown(tab.tab_id(), |id| tabs.iter().any(|t| t.tab_id() == id));
        }
        let rail = conductor::rail(tabs.len(), found);
        let sessions: Vec<Vec<(String, u64)>> = rail.iter().map(|&i| std::mem::take(&mut sessions[i])).collect();
        let mut out = Vec::with_capacity(rail.len());
        let mut wanted = vec![];
        // While the Conductor's view is open no tab's row is the open one: its pad is
        let view = self.lbterm_view_is_open();
        for (row_idx, &idx) in rail.iter().enumerate() {
            let tab = &tabs[idx];
            let active_pane = tab.get_active_pane();
            let mut row = panel_tab(tab, row_idx, idx == active && !view);
            if row.cwd.is_empty() {
                row.cwd = active_pane
                    .as_ref()
                    .and_then(|pane| self.lbterm_panel.cwds.get(&pane.pane_id()).cloned())
                    .unwrap_or_default();
            }
            row.seen = self.lbterm_panel.seen.get(&tab.tab_id()).copied().unwrap_or(0.0);
            if let Some(pane) = active_pane {
                wanted.push(pane);
            }
            out.push(row);
        }
        self.tab_panel_fetch_cwds(wanted);
        (out, sessions, own)
    }

    /// Look up the panes' working directories on another thread while the
    /// panel is open (only the open panel shows projects), at most every
    /// [`CWD_TTL`] or when a pane is new. Without OSC 7 from the shell,
    /// WezTerm finds a pane's directory by walking its processes, which
    /// can take milliseconds a pane.
    fn tab_panel_fetch_cwds(&mut self, panes: Vec<Arc<dyn Pane>>) {
        let state = &mut self.lbterm_panel;
        let stale = state.cwds_fetched.map_or(true, |t| t.elapsed() > CWD_TTL);
        let missing = panes.iter().any(|p| !state.cwds.contains_key(&p.pane_id()));
        if state.fetching || !state.hover.is_open() || panes.is_empty() || !(stale || missing) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        state.fetching = true;
        state.cwds_fetched = Some(Instant::now());
        let spawned = std::thread::Builder::new()
            .name("lbterm-panel-cwd".into())
            .spawn(move || {
                let found: Vec<(PaneId, String)> = panes
                    .iter()
                    .map(|p| {
                        let cwd = p.get_current_working_dir(CachePolicy::AllowStale);
                        (p.pane_id(), cwd.map(|u| u.to_string()).unwrap_or_default())
                    })
                    .collect();
                window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    let state = &mut tw.lbterm_panel;
                    state.fetching = false;
                    let changed = found.iter().any(|(id, cwd)| state.cwds.get(id) != Some(cwd));
                    state.cwds = found.into_iter().collect();
                    if changed {
                        if let Some(window) = tw.window.as_ref() {
                            window.invalidate();
                        }
                    }
                })));
            });
        if spawned.is_err() {
            self.lbterm_panel.fetching = false;
        }
    }

    /// The panel in the panes' room (render/card.rs), whose left edge is
    /// past the strip: the panel's own left is the window's.
    fn tab_panel_layout(&self, rows: usize, head: bool) -> anyhow::Result<Layout> {
        let strip = rail_width(&self.config, &self.dimensions, &self.render_metrics);
        let Room { left, top, right, bottom } = self.lbterm_room()?;
        let settings = &self.config.lbterm_tab_panel;
        let (label, label_line) = if settings.labels {
            let line = self.tab_panel_label_font()?.1.line_height;
            (settings.label_lines as f32 * line, line)
        } else {
            (0.0, 0.0)
        };
        Ok(Layout {
            left: left - strip,
            body_top: top,
            bottom,
            right,
            rows,
            label,
            // Rows give up their labels' lines one at a time before any tab leaves the rail, the
            // new-tab pad at its foot taking a slot (lbterm/NEW-TAB.md, "The + pad")
            label_line,
            head,
            foot: settings.new_tab,
            ..bare_layout(&self.config, &self.dimensions, &self.render_metrics)
        })
    }

    /// The labels' font, and its size as the words need it: the advance as
    /// it is, not rounded to a whole pixel as a cell is, since at this size
    /// that's a tenth of a character short and a label would run past its
    /// room into the open tab's ring.
    fn tab_panel_label_font(&self) -> anyhow::Result<(Rc<LoadedFont>, words::Font)> {
        let font = self.fonts.panel_label_font()?;
        let metrics = font.metrics();
        let size = words::Font {
            cell_width: metrics.cell_width.get() as f32,
            line_height: metrics.cell_height.get().ceil() as f32,
        };
        Ok((font, size))
    }

    /// The pads' numbers' font, the title font, with its metrics and its size as the words need
    /// it: the tabs' numbers, the Conductor's glyph and its badge's count are in it
    fn tab_panel_number_font(&self) -> anyhow::Result<(Rc<LoadedFont>, RenderMetrics, words::Font)> {
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let size = words::Font {
            cell_width: metrics.cell_size.width as f32,
            line_height: metrics.cell_size.height as f32,
        };
        Ok((font, metrics, size))
    }

    /// The Conductor's badge's count's font (`Placed::counts`): the pads' numbers' font, the
    /// title font, at its size and weight, with FreeType's auto-hinter, which fits its stems to
    /// the pixels across as well as down. Hinted as the title font is (FreeType's TrueType
    /// interpreter fits only heights), JetBrains Mono's 1 at 9 pt and 96 DPI has a stem a pixel
    /// wide falling across two columns, both half-toned however many coats it gets (their
    /// darkest greens 102 on the cyan's 204 drawn twice); bold falls across two as well (105
    /// once). Auto-hinted, it's one column, 31 drawn twice, the ink's own 28 at 1.5x and 2x
    /// (lbterm/LINKS.md, "The Conductor's count"; PITFALLS 64). Looked up again only when the
    /// shape generation or the DPI changes (a config reload, a scale change)
    fn tab_panel_count_font(&mut self) -> anyhow::Result<(Rc<LoadedFont>, RenderMetrics, words::Font)> {
        let at = (self.shape_generation, self.dimensions.dpi);
        if let Some((was, font, metrics, size)) = &self.lbterm_panel.count_font {
            if *was == at {
                return Ok((Rc::clone(font), *metrics, *size));
            }
        }
        let title = self.fonts.title_font()?;
        let mut style = title.style().clone();
        for attr in &mut style.font {
            attr.freetype_load_flags = Some(config::FreeTypeLoadFlags::FORCE_AUTOHINT);
            attr.freetype_load_target = Some(config::FreeTypeLoadTarget::Normal);
        }
        let font = self.fonts.lbterm_sized_font(&style, title.lbterm_font_size())?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let size = words::Font {
            cell_width: metrics.cell_size.width as f32,
            line_height: metrics.cell_size.height as f32,
        };
        self.lbterm_panel.count_font = Some((at, Rc::clone(&font), metrics, size));
        Ok((font, metrics, size))
    }

    /// The marks' font (`Placed::marks`): the title font's style at 7.5 pt, so 10 px at 96 DPI
    /// and as much more as the DPI is (lbterm_panel::layout::HIDDEN_GLYPH), nf-md-eye_off coming
    /// from the Symbols Nerd Font every font falls back on, as the Conductor's glyph on its pad
    /// does. Looked up only while a row is hidden from the Conductor, and again only when the
    /// shape generation or the DPI changes
    fn tab_panel_mark_font(&mut self) -> anyhow::Result<(Rc<LoadedFont>, RenderMetrics, words::Font)> {
        let at = (self.shape_generation, self.dimensions.dpi);
        if let Some((was, font, metrics, size)) = &self.lbterm_panel.mark_font {
            if *was == at {
                return Ok((Rc::clone(font), *metrics, *size));
            }
        }
        let title = self.fonts.title_font()?;
        let points = f64::from(lbterm_panel::layout::HIDDEN_GLYPH) * 72.0 / 96.0;
        let font = self.fonts.lbterm_sized_font(title.style(), points)?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let size = words::Font {
            cell_width: metrics.cell_size.width as f32,
            line_height: metrics.cell_size.height as f32,
        };
        self.lbterm_panel.mark_font = Some((at, Rc::clone(&font), metrics, size));
        Ok((font, metrics, size))
    }

    /// The wave for this frame (lbterm_panel::wave): the water drawn with its front where its run
    /// ends, `reach` times the edge's travel past the open ring (as far as leaves its spray room
    /// inside the panes), and the spray from just short of there, on the panes' rows. Drawn into
    /// [`WAVE_Z`] and [`SPRAY_Z`] only when their look isn't the one their surfaces hold, which
    /// keep it otherwise.
    fn tab_panel_wave(
        &mut self,
        layout: &Layout,
        settings: &config::lbterm::LbtermPanelWave,
        motion: &MotionSettings,
    ) -> anyhow::Result<Option<WavePlan>> {
        use lbterm_panel::wave::{self, DRIFT, RECEDE, SAG, SPRAY};
        let cell_w = self.render_metrics.cell_size.width as f32;
        let cell_h = self.render_metrics.cell_size.height as f32;
        let room = layout.wave_band();
        let travel = layout.travel();
        if cell_w <= 0.0 || cell_h <= 0.0 || room.h <= 0.0 || travel <= 0.0 {
            return Ok(None);
        }
        let stop = layout.panel(true).right().round();
        let drift = DRIFT as f32 * cell_w;
        let spray_w = (SAG + SPRAY) as f32 * cell_w;
        let panes_right = layout.panes_right();
        let beyond = (settings.reach as f32 * travel)
            .min(panes_right - stop - (spray_w - SAG as f32 * cell_w) - drift)
            .max(0.0)
            .round();
        let run_end = stop + beyond;
        let cols = wave::columns(travel + beyond, cell_w);
        if cols == 0 {
            return Ok(None);
        }
        // The panes' rows: every pane starts a whole number of rows below the top one
        let (_, padding_top) = self.padding_left_top();
        let top_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height().unwrap_or(0.0)
        } else {
            0.0
        };
        let grid = self.get_os_border().top.get() as f32 + top_bar + padding_top;
        let first = grid - ((grid - room.y) / cell_h).ceil() * cell_h;
        let rows = ((room.bottom() - first) / cell_h).ceil().max(0.0) as u32;
        let (y, h) = (room.y.round() as i32, room.h.round() as u32);
        let width = cols as f32 * cell_w;
        let rect = PixelRect::new((run_end - width) as i32, y, width as u32, h);
        let spray = PixelRect::new((run_end - SAG as f32 * cell_w) as i32, y, spray_w as u32, h);
        let closed = room.x.round() as i32;
        let band_right = ((spray.x + spray.w as i32) as f32 + drift).min(panes_right).round() as i32;
        let band = PixelRect::new(closed, y, (band_right - closed).max(0) as u32, h);
        let card = self.tab_panel_card();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (rect.x, rect.y, rect.w, rect.h, spray.x, spray.w, band.x, band.w).hash(&mut hasher);
        (cell_w.to_bits(), cell_h.to_bits(), first.to_bits(), card, settings.cover.to_bits()).hash(&mut hasher);
        (self.config.generation(), self.dimensions.dpi).hash(&mut hasher);
        let look = hasher.finish();
        let drawn = self
            .webgpu
            .as_ref()
            .and_then(|w| w.compositor.as_ref())
            .and_then(|c| c.panel_wave_drawn());
        let redraw = drawn != Some(look);
        if redraw {
            let panel = layout.panel(true);
            let water = wave::cells(cols, rows);
            self.tab_panel_wave_draw(WAVE_Z, &water, |col| run_end - (col + 1) as f32 * cell_w, first, card, settings.cover, panel)?;
            let drops = wave::spray(rows);
            let left = spray.x as f32;
            self.tab_panel_wave_draw(SPRAY_Z, &drops, |col| left + col as f32 * cell_w, first, card, settings.cover, panel)?;
        }
        Ok(Some(WavePlan {
            rect,
            spray,
            band,
            stop: stop as i32,
            cell: cell_w as f64,
            lead: settings.lead,
            beyond: beyond as f64,
            recede: (RECEDE as f32 * cell_w) as f64,
            drift: drift as f64,
            fade: motion.panel_wash.clone(),
            rise: motion.copy_in.clone(),
            splash: motion.panel_splash.clone(),
            look,
            redraw,
        }))
    }

    /// Cells of the wave or its spray into `zindex`, each column at `x_of(col)` and its rows down
    /// from `first`, in the panes' font: each cell's cover, the panes' colour hiding what's under
    /// it, and its character, coloured by its height down the open `panel`.
    fn tab_panel_wave_draw(
        &self,
        zindex: i8,
        cells: &[lbterm_panel::wave::Cell],
        x_of: impl Fn(u32) -> f32,
        first: f32,
        card: u32,
        cover: f64,
        panel: Area,
    ) -> anyhow::Result<()> {
        let gl_state = self.render_state.as_ref().unwrap();
        let font = self.fonts.default_font()?;
        let metrics = &self.render_metrics;
        let (cell_w, cell_h) = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        // Each character laid out once, for its glyph and its baseline in a cell
        let mut glyphs: HashMap<char, (Rc<CachedGlyph>, f32)> = HashMap::new();
        let dpi = self.dimensions.dpi as f32;
        for cell in cells {
            if cell.ch == ' ' || glyphs.contains_key(&cell.ch) {
                continue;
            }
            let element = Element::new(&font, ElementContent::Text(cell.ch.to_string()));
            let computed = self.compute_element(
                &LayoutContext {
                    width: DimensionContext {
                        dpi,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: cell_w,
                    },
                    height: DimensionContext {
                        dpi,
                        pixel_max: self.dimensions.pixel_height as f32,
                        pixel_cell: cell_h,
                    },
                    bounds: euclid::rect(0., 0., 4. * cell_w, cell_h),
                    metrics,
                    gl_state,
                    zindex,
                },
                &element,
            )?;
            if let ComputedElementContent::Text(laid) = &computed.content {
                if let Some(ElementCell::Glyph(glyph)) = laid.first() {
                    glyphs.insert(cell.ch, (Rc::clone(glyph), computed.baseline));
                }
            }
        }
        let layer = gl_state.layer_for_zindex(zindex)?;
        let mut quads = layer.quad_allocator();
        let (left, top) = (self.dimensions.pixel_width as f32 / -2.0, self.dimensions.pixel_height as f32 / -2.0);
        let under = srgb(card);
        // The covers a run at a time: a row's neighbouring cells that hide as much as each other
        let mut runs: Vec<(u32, u32, u32, f32)> = vec![];
        for cell in cells {
            let hides = (cell.cover as f64 * cover) as f32;
            match runs.last_mut() {
                Some((row, _, last, h)) if *row == cell.row && *last + 1 == cell.col && *h == hides => *last = cell.col,
                _ => runs.push((cell.row, cell.col, cell.col, hides)),
            }
        }
        for (row, from, to, hides) in runs {
            if hides > 0.0 {
                let x = x_of(from).min(x_of(to));
                let y = first + row as f32 * cell_h;
                let hide = LinearRgba::with_components(under.0, under.1, under.2, hides);
                let area = Area::new(x, y, (to - from + 1) as f32 * cell_w, cell_h);
                self.filled_rectangle(&mut quads, 0, rect(area), hide)?;
            }
        }
        for cell in cells {
            let x = x_of(cell.col);
            let y = first + cell.row as f32 * cell_h;
            let Some((glyph, baseline)) = glyphs.get(&cell.ch) else {
                continue;
            };
            let Some(texture) = glyph.texture.as_ref() else {
                continue;
            };
            let down = ((y + cell_h / 2.0 - panel.y) / panel.h.max(1.0)).clamp(0.0, 1.0) as f64;
            let colour = srgb(lbterm_panel::wave::colour(card, down, cell.light));
            // Where render_element puts a glyph in its cell
            let gx = x + left + (glyph.x_offset + glyph.bearing_x).get() as f32;
            let gy = y + top - (glyph.y_offset + glyph.bearing_y).get() as f32 + baseline;
            let w = texture.coords.size.width as f32 * glyph.scale as f32;
            let h = texture.coords.size.height as f32 * glyph.scale as f32;
            let mut quad = quads.allocate(1)?;
            quad.set_position(gx, gy, gx + w, gy + h);
            quad.set_fg_color(colour);
            quad.set_alt_color_and_mix_value(colour, 0.0);
            quad.set_texture(texture.texture_coords());
            quad.set_has_color(glyph.has_color);
            quad.set_hsv(None);
        }
        Ok(())
    }

    /// The panes' background, as the panel's card takes it.
    fn tab_panel_card(&mut self) -> u32 {
        let (r, g, b, _) = self.palette().background.to_srgb_u8();
        ((r as u32) << 16) | ((g as u32) << 8) | b as u32
    }

    /// The panel's quads, drawn `open` or closed, the light at `glow`.
    #[allow(clippy::too_many_arguments)]
    fn tab_panel_draw(
        &mut self,
        rows: &[Row],
        doing: &[Option<Doing>],
        head: Option<&(HeadSays, Level)>,
        layout: &Layout,
        tabs: &[TabId],
        starting: Option<&str>,
        open: bool,
        glow: f64,
    ) -> anyhow::Result<()> {
        let card = self.tab_panel_card();
        let (count_font, count_metrics, count_size) = self.tab_panel_count_font()?;
        // The marks' font, only while a visible row is hidden from the Conductor
        let hidden = rows.iter().enumerate().take_while(|&(i, _)| layout.row_visible(i)).any(|(_, r)| r.hidden);
        let mark_font = if hidden { Some(self.tab_panel_mark_font()?) } else { None };
        let gl_state = self.render_state.as_ref().unwrap();
        let (font, metrics, font_size) = self.tab_panel_number_font()?;
        let (label_font, label_size) = self.tab_panel_label_font()?;
        let mut placed = words::place(rows, layout, font_size, label_size, column_width);
        // LBTerm: the Starting row's label under its pad, and "Starting…" on the open panel, with
        // the rows (render/new_tab.rs)
        let n = rows.len();
        if let Some(label) = starting.filter(|_| layout.row_visible(n)) {
            let room = layout.label(n);
            if room.h > 0.0 {
                let columns = (room.w / label_size.cell_width).floor().max(0.0) as usize;
                let text = lbterm_panel::text::fit(label, columns, column_width);
                let w = text.chars().map(column_width).sum::<usize>() as f32 * label_size.cell_width;
                placed.labels.push(words::Word {
                    row: n,
                    area: Area::new((room.x + (room.w - w) / 2.0).round(), room.y, w + label_size.cell_width, label_size.line_height),
                    text,
                    colour: palette::LABEL,
                });
            }
            let room = layout.text(n);
            let columns = (room.w / font_size.cell_width).floor().max(0.0) as usize;
            let line = font_size.line_height;
            let first = (room.y + (room.h - line) / 2.0).round();
            placed.words.push(words::Word {
                row: n,
                text: lbterm_panel::text::fit(&format!("{} \u{b7} {label}", lbterm_extras::newtab::STARTING), columns, column_width),
                area: Area::new(room.x, first, room.w + font_size.cell_width, line),
                colour: super::new_tab::white_over(card, super::new_tab::STARTING_ALPHA),
            });
        }
        // The rows' numbers and labels go with the rows; the Conductor's, after them, stay in
        // the body
        let (row_numbers, row_labels, row_words) = (placed.numbers.len(), placed.labels.len(), placed.words.len());
        if let Some((says, _)) = head {
            let head = words::head(says, layout, font_size, label_size, column_width);
            placed.numbers.extend(head.numbers);
            placed.labels.extend(head.labels);
            placed.words.extend(head.words);
            placed.counts.extend(head.counts);
        }
        // LBTerm: the new-tab pad's row on the open panel, "New session" with its key right, and
        // "+N" over it for tabs the rail has no room for (render/new_tab.rs)
        let card_rgb = card;
        if layout.foot {
            let room = layout.foot_text();
            let line = font_size.line_height;
            let y = (room.y + (room.h - line) / 2.0).round();
            // The picker's key as the key table binds it, none where nothing is (PITFALLS 119)
            let key = self.lbterm_new_tab.keys.picker.clone();
            let key_w = key.as_ref().map_or(0.0, |k| k.chars().count() as f32 * font_size.cell_width);
            placed.words.push(words::Word {
                row: usize::MAX,
                text: super::new_tab::FOOT_WORDS.to_string(),
                area: Area::new(room.x, y, (room.w - key_w).max(0.0), line),
                colour: super::new_tab::white_over(card_rgb, super::new_tab::FOOT_WORDS_ALPHA),
            });
            if let Some(key) = key {
                placed.words.push(words::Word {
                    row: usize::MAX,
                    text: key,
                    area: Area::new(room.right() - key_w, y, key_w + font_size.cell_width, line),
                    colour: super::new_tab::white_over(card_rgb, super::new_tab::FOOT_KEY_ALPHA),
                });
            }
            let hidden = self.lbterm_hidden_tabs(layout);
            if hidden > 0 {
                let text = format!("+{hidden}");
                let w = text.chars().count() as f32 * label_size.cell_width;
                let rule = layout.foot_rule();
                placed.labels.push(words::Word {
                    row: usize::MAX,
                    area: Area::new(
                        (layout.column() - w / 2.0).round(),
                        (rule.y - label_size.line_height).round(),
                        w + label_size.cell_width,
                        label_size.line_height,
                    ),
                    text,
                    colour: super::new_tab::white_over(card_rgb, super::new_tab::HIDDEN_ALPHA),
                });
            }
        }
        let active = palette::blend(card, palette::ACTIVE, palette::ACTIVE_ALPHA);
        let hover = palette::blend(card, palette::HOVER, palette::HOVER_ALPHA);
        let fill = |row: &Row, i: usize| {
            if row.active {
                Some(active)
            } else if self.lbterm_panel.hovered == Some(i) {
                Some(hover)
            } else {
                None
            }
        };
        // LBTerm: the eye-off mark on each pad hidden from the Conductor, white at 60% over the
        // disc tab_panel_pad cuts for it in what's under the pad (lbterm/NEW-TAB.md, section 8)
        if let Some((_, _, mark_size)) = &mark_font {
            let glyph = layout.px(lbterm_panel::layout::HIDDEN_GLYPH);
            placed.marks = words::marks(rows, layout, *mark_size, glyph, |i| fill(&rows[i], i).unwrap_or(card));
        }
        // LBTerm: a parked row's number, label and mark dimmed into its pill, as tab_panel_pad
        // dims its pad (Lachlan's 8a)
        words::dim_parked(&mut placed, rows, |i| rows.get(i).and_then(|r| fill(r, i)).unwrap_or(card));
        let panel = layout.panel(open);
        let pill_radius = layout.pill_radius();

        {
            // The strip under the panel, the panes' colour, in `over`
            let layer = gl_state.layer_for_zindex(STRIP_Z)?;
            let mut quads = layer.quad_allocator();
            let strip = Area::new(layout.left, layout.body_top, layout.strip(), layout.bottom - layout.body_top);
            self.filled_rectangle(&mut quads, 0, rect(strip), srgb(card))?;
        }

        // LBTerm: the glow and the lit ring, made only when the layout, the card's colour or the
        // light change, and kept in their layers otherwise (render/kept.rs): with the compositor
        // they're drawn open and lit always, and were made again every frame, about 520 and 1,600
        // quads in a window 1,920 tall. Their layers are theirs alone, so nothing is copied in;
        // the body, the rows and the words below share theirs, and are copied in
        // (render/kept_quads.rs)
        let [l0, l1, l2, l3, l4] = super::kept::layout(layout);
        let light_key = |parts: &[u64]| {
            let mut all = vec![l0, l1, l2, l3, l4, open as u64, glow.to_bits()];
            all.extend_from_slice(parts);
            self.lbterm_template_key(&all)
        };
        let place = layout_bits(layout);
        let chrome = &self.lbterm_panel.chrome;
        self.lbterm_kept_layer(GLOW_Z, "panel-glow", light_key(&[card as u64]), |quads| {
            self.tab_panel_glow(quads, panel, layout, card, glow)
        })?;

        if glow > 0.0 {
            // The ring lit, over the grey one: with the compositor always,
            // and DWM fades it in; without, as lit as the moment calls for
            let ring = |y: f32| {
                let lit = palette::ring(((y - panel.y) / panel.h.max(1.0)).clamp(0.0, 1.0) as f64);
                palette::blend(palette::OUTLINE, lit, glow)
            };
            self.lbterm_kept_layer(LIT_Z, "panel-lit", light_key(&[]), |quads| {
                self.tab_panel_ring(quads, panel, layout, ring)
            })?;
        }

        // LBTerm: each tab's colour from its menu, a ring round its pad (tab_menu.rs)
        let accents: Vec<Option<(u8, u8, u8)>> = tabs.iter().copied().map(crate::tabcolors::rgb).collect();

        let composited = self.lbterm_composited();
        let (rows_z, marker_z) = if composited { (ROWS_Z, MARKER_Z) } else { (BODY_Z, BODY_Z) };
        let row_words_z = if composited { ROW_WORDS_Z } else { WORDS_Z };
        // The row a drag has picked up, which lifts
        let lifted = self
            .lbterm_dragged_row()
            .and_then(|(tab, _)| self.tab_panel_row_of(tab));
        self.lbterm_kept_quads(&chrome.body, BODY_Z, (place, open, card), |quads| {
            let r = layout.radius().min(panel.w / 2.0).min(panel.h / 2.0).floor();
            let stroke = layout.stroke();
            self.tab_panel_rounded(quads, panel.grow(-stroke), r - stroke, true, true, srgb(card))?;
            self.tab_panel_ring(quads, panel, layout, |_| palette::OUTLINE)?;
            // LBTerm: the new-tab pad at the rail's foot, at rest: a hairline pill with a + in
            // it, and the rule over it in the Conductor's rule's grey. Its look under the pointer
            // is render/new_tab.rs's, over it (lbterm/NEW-TAB.md, "The + pad")
            if layout.foot {
                let (r, g, b) = palette::bytes(palette::OUTLINE);
                let grey = LinearRgba::with_srgba(r, g, b, (0.6f32 * 255.0).round() as u8);
                self.filled_rectangle(quads, 0, rect(layout.foot_rule()), grey)?;
                let line = super::new_tab::white_over(card, super::new_tab::PAD_LINE);
                self.lbterm_stroke(
                    quads,
                    layout.foot_pill(),
                    layout.foot_radius(),
                    layout.stroke(),
                    layout.px(lbterm_panel::layout::FEATHER),
                    |_| line,
                )?;
                let plus = srgb(super::new_tab::white_over(card, super::new_tab::PAD_PLUS));
                for stroke in layout.plus() {
                    self.filled_rectangle(quads, 0, rect(stroke), plus)?;
                }
            }
            Ok(())
        })?;
        // What the rows' quads are made from, row by row: what each shows (its looks), its
        // highlight, its tab's colour and its pieces of the links' lines, and how far along its
        // bars are (drawn that long without the compositor). The rows on the rail and their parts
        // on the open panel are made again only when one of these changes, and copied in
        // otherwise (render/kept_quads.rs)
        let rows_from = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (place, card, composited, lifted, starting).hash(&mut hasher);
            for (i, row) in rows.iter().enumerate().take_while(|&(i, _)| layout.row_visible(i)) {
                (lbterm_panel::look::looks(row), fill(row, i), accents.get(i), self.lbterm_panel.links.pieces.get(i)).hash(&mut hasher);
                (row.progress, &row.agent_bars, row.estimated, row.state as u8).hash(&mut hasher);
            }
            hasher.finish()
        };
        // The rows: their pills and pads
        self.lbterm_kept_quads(&chrome.rows, rows_z, rows_from, |quads| {
            for (i, row) in rows.iter().enumerate() {
                if !layout.row_visible(i) {
                    break;
                }
                let pill = layout.pill(i);
                if lifted == Some(i) {
                    // Lifted while it's dragged: a soft shadow under it, inside its slot (what's
                    // past it would stay behind with the row next to it)
                    let slot = layout.slot(i);
                    for (by, alpha) in [(3.0f32, 0.30f32), (2.0, 0.22), (1.0, 0.16)] {
                        let grow = layout.px(by);
                        let top = (pill.y - grow + layout.px(1.0)).max(slot.y);
                        let bottom = (pill.bottom() + grow + layout.px(1.0)).min(slot.bottom());
                        let shadow = Area::new(pill.x - grow, top, pill.w + 2.0 * grow, bottom - top);
                        self.tab_panel_rounded(quads, shadow, pill_radius + grow, true, true, LinearRgba::with_components(0., 0., 0., alpha))?;
                    }
                    self.tab_panel_rounded(quads, pill, pill_radius, true, true, srgb(hover))?;
                }
                if let Some(colour) = fill(row, i) {
                    self.tab_panel_rounded(quads, pill, pill_radius, true, true, srgb(colour))?;
                }
                let under = fill(row, i).unwrap_or(card);
                // Its pieces of the tab links' lines, in its own slot, so they move with it
                self.tab_panel_link_pieces(quads, i, layout, card, under, false)?;
                self.tab_panel_pad(quads, row, i, layout, accents.get(i).copied().flatten(), under)?;
            }
            // LBTerm: the Starting row: a dim pad with the white waiting arc on its corner, the
            // arc turning in a visual of its own with the compositor (START_SPIN_Z) and drawn here,
            // still, without (render/new_tab.rs)
            let n = rows.len();
            if starting.is_some() && layout.row_visible(n) {
                // (kept quads: made only when the rows change, so the row's first frame is noted
                // in paint_tab_panel, whatever this closure does)
                self.tab_panel_rounded(quads, layout.pad(n), layout.px(7.0), true, true, srgb(super::new_tab::STARTING_PAD))?;
                self.tab_panel_arc(quads, layout.spinner(n), layout, Some(card), !composited, palette::UNREAD)?;
            }
            Ok(())
        })?;
        {
            // The open tab: a ring round its pill, and a bar beside it down the rail's edge. The
            // Conductor's tab open: on its pad, which stands for it
            let open_at = if head.is_some() && self.tab_panel_conductor_open() {
                Some((layout.head_pill(), layout.head_edge_bar()))
            } else {
                (0..rows.len())
                    .take_while(|&i| layout.row_visible(i))
                    .find(|&i| rows[i].active)
                    .map(|i| (layout.pill(i), layout.edge_bar(i)))
            };
            // A tab from another window over this one: an empty pill ringed in the accent
            // where it would land, after the last tab
            let hint = self.lbterm_panel.drop_hint.then(|| {
                let mut room = *layout;
                room.rows = rows.len() + 1;
                room.row_visible(rows.len()).then(|| room.pill(rows.len()))
            });
            let from = (place, open_at.map(|(pill, bar)| (bits(pill), bits(bar))), hint.map(|h| h.map(bits)));
            self.lbterm_kept_quads(&chrome.marker, marker_z, from, |quads| {
                if let Some((pill, bar)) = open_at {
                    self.tab_panel_active_ring(quads, pill, layout)?;
                    self.tab_panel_rounded(quads, bar, bar.w / 2.0, true, true, srgb(palette::ACTIVE))?;
                }
                if let Some(Some(pill)) = hint {
                    self.tab_panel_active_ring(quads, pill, layout)?;
                }
                Ok(())
            })?;
        }
        {
            let layer = gl_state.layer_for_zindex(BODY_Z)?;
            let mut quads = layer.quad_allocator();
            // The Conductor's pad atop the rail (render/conductor.rs), with its count of the
            // connections suggested here on a badge, rimmed in what's under it
            if let Some((says, level)) = head {
                let open_fill = self.tab_panel_conductor_open().then_some(active);
                let badge = words::head_badge(says, layout, font_size, column_width).map(|a| (a, open_fill.unwrap_or(card)));
                self.tab_panel_conductor(&mut quads, layout, *level, open_fill, badge)?;
            }
            // Without the compositor, its outlines round the tabs it's on, still
            if !self.lbterm_composited() {
                for (i, d) in doing.iter().enumerate() {
                    if d.is_some() && layout.row_visible(i) {
                        self.tab_panel_rainbow(&mut quads, layout, layout.pill(i), RingStyle::Iridescent)?;
                    }
                }
            }
        }

        if open {
            // Each visible row's bars: where, how far along and in what colour, a parked row's
            // fills dimmed toward what's under them as its pad and words are (Lachlan's 8a; the
            // track, the bar's ground, as it is). The fills are drawn per row, with or without the
            // compositor, so the dim reaches both (PITFALLS 255)
            let bars = |i: usize, row: &Row| -> Vec<(Area, u8, u32)> {
                let n = row.agent_bars.len();
                let under = fill(row, i).unwrap_or(card);
                let dim = |colour: u32| if row.parked { palette::parked(under, colour) } else { colour };
                let main = row.progress.map(|p| {
                    let colour = if row.estimated { palette::estimate(row.state) } else { palette::status(row.state) };
                    (layout.progress_bar(i), p, dim(colour))
                });
                let agents = row.agent_bars.iter().enumerate().map(|(k, &p)| {
                    (layout.agent_bar(i, k, n, row.progress.is_some()), p, dim(palette::estimate(RowState::Working)))
                });
                main.into_iter().chain(agents).collect()
            };
            let chips: Vec<[u32; 4]> = placed.chips.iter().map(|c| bits(*c)).collect();
            self.lbterm_kept_quads(&chrome.open, row_words_z, (rows_from, &chips), |quads| {
                // LBTerm: the labels under the pads covered where the open panel shows, each by
                // its pill's colour, so they fade out as the titles saying the same names fade in
                // with these words, and back as they go (Lachlan's 12a, lbterm/NEW-TAB.md,
                // "Landing 4"). A raised row's highlight, below, covers its own. The Starting
                // row's too, after the tabs' (rows_from has the rows, the layout, `lifted` and
                // `starting`, all these depend on)
                let shown = rows.len() + starting.is_some() as usize;
                let raised = |i: usize| rows.get(i).map_or(false, |row| fill(row, i).is_some());
                for (i, cover) in lbterm_panel::words::label_covers(layout, shown, raised) {
                    let under = if lifted == Some(i) { hover } else { card };
                    self.filled_rectangle(quads, 0, rect(cover), srgb(under))?;
                }
                for (i, row) in rows.iter().enumerate() {
                    if !layout.row_visible(i) {
                        break;
                    }
                    // The pill grown into the row's long highlight, the same
                    // colour, over the rail's own; the open tab's ring goes round
                    // all of it. The pad and its timer are drawn again on top
                    // (its number and mark too, below), not its label, which the
                    // title beside it says (Lachlan's 12a)
                    if let Some(colour) = fill(row, i) {
                        let highlight = layout.highlight(i);
                        self.tab_panel_rounded(quads, highlight, pill_radius, true, true, srgb(colour))?;
                        if row.active {
                            self.tab_panel_active_ring(quads, highlight, layout)?;
                        }
                        // The links' ends the highlight covers, and the pad, again over it
                        self.tab_panel_link_pieces(quads, i, layout, card, colour, true)?;
                        self.tab_panel_pad(quads, row, i, layout, accents.get(i).copied().flatten(), colour)?;
                    }
                }
                for chip in &placed.chips {
                    self.tab_panel_rounded(quads, *chip, layout.px(6.0), true, true, srgb(palette::CHIP))?;
                }
                // How far along a program says it is, or the Conductor's hub guesses it is (dimmer),
                // and the hub's guess for each subagent, side by side over it: the track, and
                // without the compositor the fill as long as it says
                for (i, row) in rows.iter().enumerate() {
                    if !layout.row_visible(i) {
                        break;
                    }
                    for (track, progress, colour) in bars(i, row) {
                        let radius = track.h / 2.0;
                        self.tab_panel_rounded(quads, track, radius, true, true, srgb(palette::TRACK))?;
                        if !composited {
                            let fill = Area::new(track.x, track.y, (track.w * progress as f32 / 100.0).round(), track.h);
                            self.tab_panel_rounded(quads, fill, radius, true, true, srgb(colour))?;
                        }
                    }
                }
                Ok(())
            })?;
            if composited {
                // The fills whole, in their own layer: the compositor shows as much as each says
                self.lbterm_kept_quads(&chrome.progress, PROGRESS_Z, rows_from, |fills| {
                    for (i, row) in rows.iter().enumerate() {
                        if !layout.row_visible(i) {
                            break;
                        }
                        for (track, _, colour) in bars(i, row) {
                            self.tab_panel_rounded(fills, track, track.h / 2.0, true, true, srgb(colour))?;
                        }
                    }
                    Ok(())
                })?;
            }
        }

        // Laying out text shapes it: only when the words change
        let raised = if open {
            (0..rows.len())
                .take_while(|&i| layout.row_visible(i))
                .filter(|&i| fill(&rows[i], i).is_some())
                .collect()
        } else {
            vec![]
        };
        let key = WordsKey {
            placed,
            shape_generation: self.shape_generation,
            font: Rc::as_ptr(&font) as *const () as usize,
            label_font: Rc::as_ptr(&label_font) as *const () as usize,
            count_font: Rc::as_ptr(&count_font) as *const () as usize,
            mark_font: mark_font.as_ref().map_or(0, |(f, ..)| Rc::as_ptr(f) as *const () as usize),
            raised,
        };
        let fresh = matches!(&self.lbterm_panel.laid_out, Some(l) if l.key == key);
        if !fresh {
            let label_metrics = RenderMetrics::with_font_metrics(&label_font.metrics());
            // Each word taken from the last lay-out's when it was laid out from the same (its
            // text, colour, place and font: render/words.rs), so one row's new status, the pointer
            // moving to the next row or an age ticking lays out those words alone, not every
            // number, label and word on the panel
            let mut kept = self.lbterm_panel.words.borrow_mut();
            kept.at(self.lbterm_words_at());
            let mut taken: Vec<(super::words::WordKey, ComputedElement)> = vec![];
            let mut compute = |list: &[&words::Word],
                               zindex: i8,
                               font: &Rc<LoadedFont>,
                               metrics: &RenderMetrics,
                               font_size: words::Font|
             -> anyhow::Result<Vec<ComputedElement>> {
                let mut computed = Vec::with_capacity(list.len());
                for word in list {
                    let (key, element) = self.lbterm_take_word(
                        &mut kept,
                        font,
                        metrics,
                        (font_size.cell_width, font_size.line_height),
                        zindex,
                        (word.text.clone(), rect(word.area), srgb(word.colour)),
                    )?;
                    computed.push(element.clone());
                    taken.push((key, element));
                }
                Ok(computed)
            };
            let placed = &key.placed;
            let all: [Vec<&words::Word>; 3] = [&placed.numbers, &placed.labels, &placed.words]
                .map(|list| list.iter().collect());
            let [numbers, labels, words] = all;
            // A raised row's number goes again over its highlight, not its label: the open
            // panel's labels are covered, the title beside each saying the same (Lachlan's 12a)
            let raised_numbers: Vec<&words::Word> =
                placed.numbers.iter().filter(|w| key.raised.contains(&w.row)).collect();
            let rows_z = if self.lbterm_composited() { ROWS_Z } else { BODY_Z };
            let (head_numbers, head_labels) = (
                numbers.get(row_numbers..).unwrap_or(&[]).to_vec(),
                labels.get(row_labels..).unwrap_or(&[]).to_vec(),
            );
            let mut numbers = compute(numbers.get(..row_numbers).unwrap_or(&numbers), rows_z, &font, &metrics, font_size)?;
            numbers.extend(compute(&head_numbers, BODY_Z, &font, &metrics, font_size)?);
            // The badge's count over the Conductor's pad, in its own font
            let counts: Vec<&words::Word> = placed.counts.iter().collect();
            numbers.extend(compute(&counts, BODY_Z, &count_font, &count_metrics, count_size)?);
            let mut labels = compute(labels.get(..row_labels).unwrap_or(&labels), rows_z, &label_font, &label_metrics, label_size)?;
            labels.extend(compute(&head_labels, BODY_Z, &label_font, &label_metrics, label_size)?);
            // The rows' words go with the rows; the Conductor's stay in the words
            let row_words_z = if self.lbterm_composited() { ROW_WORDS_Z } else { WORDS_Z };
            let head_words = words.get(row_words..).unwrap_or(&[]).to_vec();
            let mut words = compute(words.get(..row_words).unwrap_or(&words), row_words_z, &font, &metrics, font_size)?;
            words.extend(compute(&head_words, WORDS_Z, &font, &metrics, font_size)?);
            let mut raised = compute(&raised_numbers, row_words_z, &font, &metrics, font_size)?;
            // The eye-off marks with the rows' numbers, and again over an open row's highlight
            if let Some((mark_font, mark_metrics, mark_size)) = &mark_font {
                let marks: Vec<&words::Word> = placed.marks.iter().collect();
                numbers.extend(compute(&marks, rows_z, mark_font, mark_metrics, *mark_size)?);
                let lifted: Vec<&words::Word> = placed.marks.iter().filter(|w| key.raised.contains(&w.row)).collect();
                raised.extend(compute(&lifted, row_words_z, mark_font, mark_metrics, *mark_size)?);
            }
            kept.keep(taken);
            self.lbterm_panel.laid_count += 1;
            drop(kept);
            self.lbterm_panel.laid_out = Some(LaidOut {
                key,
                numbers,
                labels,
                words,
                raised,
            });
        }
        // Their quads made again only when they're laid out again or the open panel's words
        // come or go, copied in otherwise (render/kept_quads.rs)
        if let Some(laid_out) = &self.lbterm_panel.laid_out {
            let open_words: &[ComputedElement] = if open { &laid_out.words } else { &[] };
            let open_raised: &[ComputedElement] = if open { &laid_out.raised } else { &[] };
            let elements = laid_out.numbers.iter().chain(&laid_out.labels).chain(open_words).chain(open_raised);
            self.lbterm_draw_elements(&self.lbterm_panel.word_quads, (self.lbterm_panel.laid_count, open), elements)?;
        }
        Ok(())
    }

    /// The light round the ring, out to `layout.glow()` past it, at
    /// `strength`. Every pixel is drawn once: bands along the sides and
    /// fans round the corners, their colours blending from knot to knot.
    /// Faint layers stacked one over another would each round away to
    /// nothing in the 8-bit surface before the next arrived.
    fn tab_panel_glow(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        panel: Area,
        layout: &Layout,
        card: u32,
        strength: f64,
    ) -> anyhow::Result<()> {
        /// Knots out from the ring, and segments round each corner
        const KNOTS: usize = 10;
        const ARC: usize = 12;
        let reach = layout.glow();
        let r = layout.radius().min(panel.w / 2.0).min(panel.h / 2.0);
        if reach <= 0.0 || panel.h <= 2.0 * r {
            return Ok(());
        }
        let bands = light_bands(palette::GLOW, reach, KNOTS, |d| {
            palette::glow_alpha(d, reach as f64, card) * strength
        });
        self.tab_panel_bands(quads, panel, r, &bands, ARC)
    }

    /// Bands of colour following the rounded rectangle `outline` (corner
    /// radius `r`), each from `near` to `far` pixels out from it (inside
    /// when negative), blending from its inner colour to its outer one:
    /// the light round the ring. Straight
    /// bands down the sides, fans of `arc` quads round the corners, so
    /// every pixel is drawn once.
    pub(crate) fn tab_panel_bands(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        outline: Area,
        r: f32,
        bands: &[Band],
        arc: usize,
    ) -> anyhow::Result<()> {
        use std::f32::consts::{FRAC_PI_2, PI};
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let mut quad = |corners: [(f32, f32); 4], colours: [LinearRgba; 4]| -> anyhow::Result<()> {
            let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), colours[0])?;
            q.set_corners(corners.map(|(x, y)| [x - half_w, y - half_h]), colours);
            Ok(())
        };
        let (left, top, right, bottom) = (outline.x, outline.y, outline.right(), outline.bottom());
        for &(near, inner, far, outer) in bands {
            quad(
                [(left + r, top - far), (right - r, top - far), (left + r, top - near), (right - r, top - near)],
                [outer, outer, inner, inner],
            )?;
            quad(
                [(left + r, bottom + near), (right - r, bottom + near), (left + r, bottom + far), (right - r, bottom + far)],
                [inner, inner, outer, outer],
            )?;
            quad(
                [(left - far, top + r), (left - near, top + r), (left - far, bottom - r), (left - near, bottom - r)],
                [outer, inner, outer, inner],
            )?;
            quad(
                [(right + near, top + r), (right + far, top + r), (right + near, bottom - r), (right + far, bottom - r)],
                [inner, outer, inner, outer],
            )?;
            for (cx, cy, from) in [
                (left + r, top + r, PI),
                (right - r, top + r, 1.5 * PI),
                (right - r, bottom - r, 0.0),
                (left + r, bottom - r, FRAC_PI_2),
            ] {
                let at = |radius: f32, angle: f32| (cx + radius * angle.cos(), cy + radius * angle.sin());
                for i in 0..arc {
                    let a0 = from + FRAC_PI_2 * i as f32 / arc as f32;
                    let a1 = from + FRAC_PI_2 * (i + 1) as f32 / arc as f32;
                    quad(
                        [at(r + near, a0), at(r + near, a1), at(r + far, a0), at(r + far, a1)],
                        [inner, inner, outer, outer],
                    )?;
                }
            }
        }
        Ok(())
    }

    /// What only the compositor shows: the light that runs round the ring,
    /// drawn where it starts. DWM carries it.
    fn tab_panel_light(&mut self, layout: &Layout) -> anyhow::Result<()> {
        let card = self.tab_panel_card();
        let centre = layout.sweep_point(0.0);
        // LBTerm: made only when the layout or the card's colour change, and kept in their
        // layers otherwise (render/kept.rs): two discs of 576 wedges each, made again every frame
        // until 2026-10-31
        let mut parts = super::kept::layout(layout).to_vec();
        parts.push(card as u64);
        let key = self.lbterm_template_key(&parts);
        for (zindex, name, surface, light, peak) in [
            (SPOT_Z, "panel-spot", layout.spot_surface(), palette::SPOT, palette::SPOT_PEAK),
            (SPOT_CORE_Z, "panel-spot-core", layout.spot_core_surface(), palette::SPOT_CORE, palette::SPOT_CORE_PEAK),
        ] {
            let reach = (surface.w - 1.0) / 2.0;
            self.lbterm_kept_layer(zindex, name, key, |quads| {
                let bands = light_bands(light, reach, 12, |d| {
                    palette::light_alpha(d, reach as f64, card, light, peak)
                });
                self.tab_panel_disc(quads, centre, &bands)
            })?;
        }
        Ok(())
    }

    /// The Conductor's pad's outline at this paint (`conductor::outline`): the pointer on it, as
    /// the last move said, and whether it acts, as this paint asked its hub
    pub(crate) fn tab_panel_head_outline(&self, level: Level) -> conductor::Outline {
        conductor::outline(level, self.lbterm_panel.head_hovered, self.lbterm_panel.head_acting)
    }

    /// The Conductor's outlines, with the compositor (render/conductor.rs): where each goes,
    /// round its own pad while its outline is the spectrum (while it acts in Accept most and
    /// Accept all, or the pointer is on it: `conductor::outline`) and as that fades out, and round
    /// each tab it's on, a light going round those it thinks about (a glint) or acts on; and their
    /// templates, drawn once round one of those pills (every pill is the same size) into z-layers
    /// only the outlines' visuals show. Also the templates' places: the outline's, and the light's.
    /// `glow` is `panel_glow` as this paint plays it (`Curve::Instant` with motion off), which the
    /// pad's ring fades in and out on in Accept most and Accept all (Lachlan's 7a).
    fn tab_panel_rings(
        &mut self,
        layout: &Layout,
        doing: &[Option<Doing>],
        level: Option<Level>,
        glow: &Curve,
    ) -> anyhow::Result<(Vec<RingPlan>, PixelRect, PixelRect)> {
        /// Where the light is, this many times a lap
        const LIGHT_STEPS: usize = 90;
        let margin = conductor_ring::ring_margin(layout);
        let reach = conductor_ring::light_reach(layout).ceil();
        let around = |pill: Area| pixels(pill.grow(margin));
        let mut pills = vec![];
        let mut rings = vec![];
        // The pad's spectrum, kept in the plan until it has faded out once it's let go: the
        // compositor takes a ring the plan stops naming away at once, and the templates are drawn
        // only while one is named. Read only's comes and goes at once with the pointer, as it did
        let spectrum = level.map_or(false, |level| self.tab_panel_head_outline(level) == conductor::Outline::Spectrum);
        let curve = match level {
            Some(level) if conductor::ring_fades(level) => glow.clone(),
            _ => Curve::Instant,
        };
        let shows = self.lbterm_panel.pad_ring.at(
            spectrum,
            Instant::now(),
            crate::termwindow::compositor::fade_out_lasts(&curve),
        );
        self.lbterm_panel.ring_due = self.lbterm_panel.pad_ring.due();
        if shows != conductor::RingNow::Gone {
            pills.push(layout.head_pill());
            rings.push(RingPlan {
                rect: around(layout.head_pill()),
                style: RingStyle::Spectrum,
                light: None,
                fade: Some(RingFade {
                    lit: shows == conductor::RingNow::Lit,
                    curve,
                }),
            });
        }
        for (i, d) in doing.iter().enumerate() {
            let Some(d) = d else {
                continue;
            };
            if !layout.row_visible(i) {
                break;
            }
            let pill = layout.pill(i);
            let (lap, opacity) = match d {
                Doing::Acting => (conductor_ring::ACTING_LAP, conductor_ring::ACTING_LIGHT),
                Doing::Thinking => (conductor_ring::GLINT_LAP, conductor_ring::GLINT_LIGHT),
            };
            let (path, radius) = conductor_ring::light_path(layout, pill);
            // LBTerm: worked out once for a pill's place and size, not every frame
            let offsets = light_offsets(path, radius, reach, LIGHT_STEPS);
            pills.push(pill);
            rings.push(RingPlan {
                rect: around(pill),
                style: RingStyle::Iridescent,
                light: Some(RingLight { lap, opacity, offsets }),
                fade: None,
            });
        }
        let Some(&template) = pills.first() else {
            return Ok((rings, PixelRect::default(), PixelRect::default()));
        };
        let (cx, cy) = (template.x.round(), template.y.round());
        let light = PixelRect::new(
            (cx - reach) as i32,
            (cy - reach) as i32,
            (2.0 * reach + 1.0) as u32,
            (2.0 * reach + 1.0) as u32,
        );
        // LBTerm: the templates, made only when the pill they're drawn round, the layout or the
        // window change (render/kept.rs): about 900 quads every frame while an outline showed
        let mut parts = super::kept::layout(layout).to_vec();
        parts.extend(super::kept::area(template));
        let key = self.lbterm_template_key(&parts);
        for (zindex, name, style) in [
            (RING_SPECTRUM_Z, "ring-spectrum", RingStyle::Spectrum),
            (RING_IRIS_Z, "ring-iris", RingStyle::Iridescent),
        ] {
            self.lbterm_kept_layer(zindex, name, key, |quads| {
                self.tab_panel_rainbow(quads, layout, template, style)
            })?;
        }
        self.lbterm_kept_layer(RING_LIGHT_Z, "ring-light", key, |quads| {
            self.tab_panel_conductor_light(quads, layout, (cx, cy))
        })?;
        Ok((rings, around(template), light))
    }

    /// A round light at `centre`: each band a ring of wedges from `near` to
    /// `far` pixels out, blending from its inner colour to its outer one.
    pub(crate) fn tab_panel_disc(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        centre: (f32, f32),
        bands: &[Band],
    ) -> anyhow::Result<()> {
        use std::f32::consts::TAU;
        const WEDGES: usize = 48;
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let at = |radius: f32, angle: f32| {
            [centre.0 + radius * angle.cos() - half_w, centre.1 + radius * angle.sin() - half_h]
        };
        for &(near, inner, far, outer) in bands {
            for i in 0..WEDGES {
                let a0 = TAU * i as f32 / WEDGES as f32;
                let a1 = TAU * (i + 1) as f32 / WEDGES as f32;
                let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), inner)?;
                q.set_corners(
                    [at(near, a0), at(near, a1), at(far, a0), at(far, a1)],
                    [inner, inner, outer, outer],
                );
            }
        }
        Ok(())
    }

    /// The ring: a hairline round `outline`, soft either side, its colour
    /// at each point `colour(y)` (render/card.rs). Grey at rest; lit, red,
    /// coral and pink top to bottom.
    /// A row's pad and what goes with it: the timer above, the ring of the
    /// tab's colour round it, the mark of an unread result.
    fn tab_panel_pad(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        row: &Row,
        i: usize,
        layout: &Layout,
        accent: Option<(u8, u8, u8)>,
        under: u32,
    ) -> anyhow::Result<()> {
        // Parked (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing 4"): every colour of the row 40% of
        // the way from what's under it, blended and drawn opaque (PITFALLS 248), its number,
        // label and marks with it (words::dim_parked); the disc cut for a mark is `under` already
        let dim = |colour: u32| if row.parked { palette::parked(under, colour) } else { colour };
        // Only the bars that are lit: none over a pad at work or without Claude (Lachlan's 12a,
        // lbterm/NEW-TAB.md, "Landing 4"), where an unlit bar was a dim stripe
        for (b, bar) in lbterm_panel::idle::lit(row.bars) {
            let area = layout.bar(i, b);
            self.tab_panel_rounded(quads, area, layout.px(1.5), true, true, srgb(dim(palette::bar(bar))))?;
        }
        let pad = layout.pad(i);
        if let Some((r, g, b)) = accent {
            let ring = srgb(dim(u32::from_be_bytes([0, r, g, b])));
            self.tab_panel_rounded(quads, pad.grow(layout.px(2.0)), layout.px(9.0), true, true, ring)?;
        }
        self.tab_panel_rounded(quads, pad, layout.px(7.0), true, true, srgb(dim(palette::pad(row.state))))?;
        if row.asks {
            // Waiting on another tab, and asking him something too: an orange ring round the
            // purple (or red) pad (Lachlan's 4a, 2026-10-29)
            let ring = dim(palette::asks_ring());
            self.lbterm_stroke(
                quads,
                pad.grow(layout.px(2.0)),
                layout.px(9.0),
                layout.px(1.5),
                layout.px(lbterm_panel::layout::FEATHER),
                |_| ring,
            )?;
        }
        if row.quiet {
            // Working, but nothing from its reply for 3 minutes with no tool running: an amber
            // ring round the green pad (Lachlan's 2a, 2026-11-02), as strong as a link's question
            let ring = dim(palette::quiet_ring());
            self.lbterm_stroke(
                quads,
                pad.grow(layout.px(2.0)),
                layout.px(9.0),
                layout.px(1.5),
                layout.px(lbterm_panel::layout::FEATHER),
                |_| ring,
            )?;
        }
        if row.extras {
            // Finished, with only extras on offer: a faint orange hairline round the blue pad
            let ring = dim(palette::extras_ring(under));
            self.lbterm_stroke(
                quads,
                pad.grow(layout.px(2.0)),
                layout.px(9.0),
                layout.stroke(),
                layout.px(lbterm_panel::layout::FEATHER),
                |_| ring,
            )?;
        }
        if row.waiting > 0 {
            // The disc, of what's under the pad, so the arc reads apart from
            // it; the arc on it turns with the compositor, in a visual of its
            // own (SPIN_Z), and is drawn here, still, without, and on a parked
            // row, dimmed (Row::arc_turns)
            let area = layout.spinner(i);
            let arc = dim(palette::pad(lbterm_panel::RowState::Working));
            self.tab_panel_arc(quads, area, layout, Some(under), !self.lbterm_composited() || !row.arc_turns(), arc)?;
        } else if row.unread {
            let dot = layout.unread_dot(i);
            self.tab_panel_rounded(quads, dot, dot.w / 2.0, true, true, srgb(dim(palette::UNREAD)))?;
        }
        if row.hidden {
            // Hidden from the Conductor: a disc of what's under the pad on its bottom right
            // corner, cut out of it as the arc's is at the top right, for the eye-off glyph the
            // words put on it (words::marks, in the marks' font); never a status colour, so the
            // pad keeps its own (lbterm/NEW-TAB.md, section 8)
            let disc = layout.hidden_mark(i);
            self.tab_panel_rounded(quads, disc, disc.w / 2.0, true, true, srgb(under))?;
        }
        Ok(())
    }

    /// Row `i`'s pieces of the tab links' and connections' lines
    /// (lbterm_extras::links::Lanes::pieces), all inside its slot: down its lane from the slot's top
    /// or to its bottom where the line runs on, and where a line ends here, across to a mark left
    /// of the pad, hollow at the tab that waits and solid at the tab waited on, and at a
    /// connection's member hollow while it's suggested and solid once it's made. A suggested link
    /// is dashed, and so is a suggested connection without the compositor. `ends_only` draws the
    /// part the open panel's highlight covers, again over it. `card` is the rail's colour, `under`
    /// the pill's.
    fn tab_panel_link_pieces(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        i: usize,
        layout: &Layout,
        card: u32,
        under: u32,
        ends_only: bool,
    ) -> anyhow::Result<()> {
        let Some(pieces) = self.lbterm_panel.links.pieces.get(i) else {
            return Ok(());
        };
        let slot = layout.slot(i);
        let w = layout.link_width();
        let (end_x, end_y) = layout.link_end(i);
        let mark = layout.link_end_size();
        let dash = (layout.px(3.0), layout.px(3.0));
        // A suggested connection is a faint line a light runs along, with the compositor, and
        // dashed as a suggested link is without it (render/connections.rs)
        let composited = self.lbterm_composited();
        // Every mark on a row is in one place: a connection's member mark, drawn before the marks
        // over it (a suggestion's before a connection made's, a link's last), is a pixel bigger
        // all round for each of them, so its cyan rims the next
        let rims = links::Lanes::rims(pieces);
        for (p, rim) in pieces.iter().zip(rims) {
            let x = layout.link_lane(p.lane) - w / 2.0;
            let dashed = p.look == links::Look::Suggested || (p.look == links::Look::ConnectionSuggested && !composited);
            if !ends_only {
                let colour = srgb(link_colour(p.look, card));
                let top = if p.up { slot.y } else { end_y - w / 2.0 };
                let bottom = if p.down { slot.bottom() } else { end_y + w / 2.0 };
                if dashed {
                    for (a, b) in links::dashes(top, bottom, dash.0, dash.1) {
                        self.filled_rectangle(quads, 0, euclid::rect(x, a, w, b - a), colour)?;
                    }
                } else if bottom > top {
                    self.filled_rectangle(quads, 0, euclid::rect(x, top, w, bottom - top), colour)?;
                }
            }
            let Some(end) = p.end else {
                continue;
            };
            let colour = link_colour(p.look, under);
            // Across from the lane to the mark
            let from = if ends_only { layout.pill(i).x } else { x };
            let to = end_x - mark / 2.0;
            if to > from {
                self.filled_rectangle(quads, 0, euclid::rect(from, end_y - w / 2.0, to - from, w), srgb(colour))?;
            }
            let size = mark + 2.0 * layout.px(rim as f32);
            let disc = Area::new((end_x - size / 2.0).round(), (end_y - size / 2.0).round(), size, size);
            self.tab_panel_rounded(quads, disc, size / 2.0, true, true, srgb(colour))?;
            if p.look.hollow(end) {
                let hole = disc.grow(-layout.px(1.5));
                self.tab_panel_rounded(quads, hole, hole.w / 2.0, true, true, srgb(under))?;
            }
        }
        Ok(())
    }

    /// The waiting mark in `area`: a disc of `disc`'s colour (what's under
    /// the pad), so it reads apart from the pad it overhangs, and on it, with
    /// `arc`, three quarters of a ring in the working pad's green, soft at its
    /// edges.
    fn tab_panel_spinner(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Area,
        layout: &Layout,
        disc: Option<u32>,
        arc: bool,
    ) -> anyhow::Result<()> {
        self.tab_panel_arc(quads, area, layout, disc, arc, palette::pad(lbterm_panel::RowState::Working))
    }

    /// `tab_panel_spinner`'s mark with its arc in `arc_rgb`: the Starting row's is white
    /// (render/new_tab.rs)
    fn tab_panel_arc(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Area,
        layout: &Layout,
        disc: Option<u32>,
        arc: bool,
        arc_rgb: u32,
    ) -> anyhow::Result<()> {
        use std::f32::consts::{FRAC_PI_2, TAU};
        const SEGMENTS: usize = 36;
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let centre = (area.x + area.w / 2.0, area.y + area.h / 2.0);
        let at = |radius: f32, angle: f32| {
            [centre.0 + radius * angle.cos() - half_w, centre.1 + radius * angle.sin() - half_h]
        };
        let colour = |rgb: u32, alpha: f32| {
            let (r, g, b) = palette::bytes(rgb);
            LinearRgba::with_srgba(r, g, b, (alpha * 255.0).round() as u8)
        };
        let feather = layout.px(lbterm_panel::layout::FEATHER);
        let outer = area.w / 2.0;
        let width = layout.px(2.0);
        let ring = outer - feather - layout.px(1.0);
        // The disc, and the ring on it: (near, alpha there, far, alpha there,
        // colour, how much of the turn)
        let green = arc_rgb;
        let mut bands = vec![];
        if let Some(disc) = disc {
            bands.push((0.0, 1.0, outer - feather, 1.0, disc, 1.0));
            bands.push((outer - feather, 1.0, outer, 0.0, disc, 1.0));
        }
        if arc {
            bands.push((ring - width - feather, 0.0, ring - width, 1.0, green, 0.75));
            bands.push((ring - width, 1.0, ring, 1.0, green, 0.75));
            bands.push((ring, 1.0, ring + feather, 0.0, green, 0.75));
        }
        for (near, a_near, far, a_far, rgb, turn) in bands {
            let steps = ((SEGMENTS as f32) * turn).round() as usize;
            for k in 0..steps {
                // From the top, clockwise on screen
                let a0 = -FRAC_PI_2 + TAU * k as f32 / SEGMENTS as f32;
                let a1 = -FRAC_PI_2 + TAU * (k + 1) as f32 / SEGMENTS as f32;
                let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), colour(rgb, a_near))?;
                q.set_corners(
                    [at(near, a0), at(near, a1), at(far, a0), at(far, a1)],
                    [colour(rgb, a_near), colour(rgb, a_near), colour(rgb, a_far), colour(rgb, a_far)],
                );
            }
        }
        Ok(())
    }

    /// The open tab's ring: a hairline of the accent round its highlight.
    fn tab_panel_active_ring(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Area,
        layout: &Layout,
    ) -> anyhow::Result<()> {
        self.lbterm_stroke(
            quads,
            area,
            layout.pill_radius(),
            layout.stroke(),
            layout.px(lbterm_panel::layout::FEATHER),
            |_| palette::ACTIVE,
        )
    }

    fn tab_panel_ring(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        outline: Area,
        layout: &Layout,
        colour: impl Fn(f32) -> u32,
    ) -> anyhow::Result<()> {
        self.lbterm_stroke(
            quads,
            outline,
            layout.radius(),
            layout.stroke(),
            layout.px(lbterm_panel::layout::FEATHER),
            colour,
        )
    }

    /// A rectangle with its left and right ends rounded or square: square
    /// where it runs on from something of the same colour.
    fn tab_panel_rounded(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Area,
        radius: f32,
        round_left: bool,
        round_right: bool,
        colour: LinearRgba,
    ) -> anyhow::Result<()> {
        if area.w <= 0.0 || area.h <= 0.0 {
            return Ok(());
        }
        let r = radius.min(area.w / 2.0).min(area.h / 2.0).floor();
        if r < 1.0 {
            self.filled_rectangle(quads, 0, rect(area), colour)?;
            return Ok(());
        }
        let size = euclid::size2(r, r);
        let mut corners = vec![];
        if round_left {
            corners.push((TOP_LEFT_ROUNDED_CORNER, area.x, area.y));
            corners.push((BOTTOM_LEFT_ROUNDED_CORNER, area.x, area.bottom() - r));
        }
        if round_right {
            corners.push((TOP_RIGHT_ROUNDED_CORNER, area.right() - r, area.y));
            corners.push((BOTTOM_RIGHT_ROUNDED_CORNER, area.right() - r, area.bottom() - r));
        }
        for (poly, x, y) in corners {
            self.poly_quad(quads, 0, euclid::point2(x, y), poly, 1, size, colour)?
                .set_grayscale();
        }
        let (left, right) = (
            if round_left { r } else { 0.0 },
            if round_right { r } else { 0.0 },
        );
        self.filled_rectangle(
            quads,
            0,
            euclid::rect(area.x + left, area.y, area.w - left - right, area.h),
            colour,
        )?;
        for (x, w) in [(area.x, left), (area.right() - right, right)] {
            if w > 0.0 {
                self.filled_rectangle(quads, 0, euclid::rect(x, area.y + r, w, area.h - 2.0 * r), colour)?;
            }
        }
        Ok(())
    }

    /// Close the panel at once, as a click on the panes it presses over does: before a question
    /// shows in a pane, so its buttons are where they'll stay. Left open, the panel folded once the
    /// pointer was off it on the way, and the pane slid back left with the Yes under the pointer
    /// (Lachlan, 2026-10-25)
    pub(crate) fn tab_panel_fold(&mut self) {
        if self.lbterm_panel.hover.close() {
            log::debug!("tab panel: folded for a question");
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// The panel's share of a mouse event: true when the panel handled it.
    /// Moving onto the panel starts it opening, moving off the open panel
    /// starts it closing, and a left click on a row switches to that tab.
    /// While the open panel presses the panes over they aren't where they
    /// look, so a click on them closes the panel at once and goes no
    /// further.
    pub fn tab_panel_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(layout) = self.lbterm_panel.layout else {
            return false;
        };
        // LBTerm: a button pressed on the new-tab pad let go: the right one opens its + menu as it
        // comes up (render/new_tab.rs)
        if let WMEK::Release(press) = &event.kind {
            if self.lbterm_new_tab_pad_release(*press, event.coords.x as f64, event.coords.y as f64) {
                self.lbterm_panel.pressed = false;
                return true;
            }
        }
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut self.lbterm_panel.pressed) {
                return true;
            }
        }
        let now = Instant::now();
        let (x, y) = (event.coords.x as f32, event.coords.y as f32);
        // A drag that began elsewhere (a selection, a split, the window)
        // carries on over the panel without opening it
        let busy = self.current_mouse_capture.is_some()
            || self.window_drag_position.is_some()
            || self.dragging.is_some();
        let was_open = self.lbterm_panel.hover.is_open();
        // LBTerm: the new-tab pad's zone at the rail's foot, on into the screen's corner while the
        // window is maximized, past the rail (render/new_tab.rs): lit under the pointer, a left
        // press opens its picker, a right one its + menu as it comes up
        let on_pad = !busy && layout.foot_at(x, y, was_open, self.lbterm_new_tab_corner());
        if self.lbterm_new_tab_pad_pointer(on_pad) {
            context.lbterm_invalidate();
        }
        // While it closes, coming back onto what's left of it opens it again
        let area_open = was_open || self.lbterm_panel.hover.reopens(now);
        // The card of what a tab is running counts as the panel, so the pointer can go over to
        // it and the panel stays open; it takes the clicks on it (render/tasks_card.rs)
        let on_tasks = !busy && self.lbterm_tasks_card_at(x as f64, y as f64);
        let inside = !busy && (layout.inside(x, y, area_open) || on_tasks);
        // On a tab link's line: its card opens once the pointer rests there, and the closed panel
        // stays closed (Lachlan's E3), its rows and lines in sight beside the card
        let line = if busy || self.lbterm_links.drag.is_some() {
            None
        } else {
            self.lbterm_link_line_at(x, y)
        };
        let changed = self.lbterm_panel.hover.pointer(inside && (area_open || line.is_none()), now);
        // Onto the rail: the suggested lines' lights run their laps again, in a frame of their own
        if inside != std::mem::replace(&mut self.lbterm_panel.on_rail, inside) && inside && self.lbterm_connection_lights_owed() {
            context.lbterm_invalidate();
        }
        let hovered = if inside {
            layout.row_at(x, y, self.lbterm_panel.hover.is_open())
        } else {
            None
        };
        let (line_changed, line_due) = self.lbterm_link_line_pointer(line, y, now);
        if line_changed {
            context.lbterm_invalidate();
        }
        self.tab_panel_wake(line_due);
        self.lbterm_tasks_pointer(hovered.filter(|_| line.is_none()), on_tasks, now, context);
        if changed {
            log::debug!(
                "tab panel: pointer {}, open {}, light {:?}",
                if inside { "in" } else { "out" },
                self.lbterm_panel.hover.is_open(),
                self.lbterm_panel.hover.light()
            );
        }
        // On the Conductor's pad: read only, its outline turns to the spectrum
        let on_head = inside && layout.head_at(x, y, self.lbterm_panel.hover.is_open());
        if changed || hovered != self.lbterm_panel.hovered || on_head != self.lbterm_panel.head_hovered {
            self.lbterm_panel.hovered = hovered;
            self.lbterm_panel.head_hovered = on_head;
            context.lbterm_invalidate();
        }
        self.tab_panel_wake(self.lbterm_panel.hover.deadline());

        if on_pad {
            if let WMEK::Press(press @ (MousePress::Left | MousePress::Right)) = event.kind {
                self.lbterm_panel.pressed = true;
                self.lbterm_new_tab_pad_press(press, x as f64, y as f64, &layout);
                context.lbterm_invalidate();
                return true;
            }
            if !inside {
                context.set_cursor(Some(CursorIcon::Pointer));
                return true;
            }
        }

        if !inside {
            let pressed = self
                .webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(false, |c| c.panes_pressed());
            if pressed && was_open && matches!(event.kind, WMEK::Press(_)) {
                self.lbterm_panel.hover.close();
                self.lbterm_panel.pressed = true;
                context.lbterm_invalidate();
                return true;
            }
            return false;
        }

        match event.kind {
            // The Conductor's pad is its tab: a left click opens it, and drops its menu while
            // there's no tab to open; a right click drops the menu (render/conductor.rs)
            // With a seat, a left click opens the Conductor's view (render/conductor_view.rs),
            // on or off: there's no tab of its own
            WMEK::Press(MousePress::Left) if on_head && self.lbterm_view_on() => {
                self.lbterm_panel.pressed = true;
                // Timed to the frame that shows it (termwindow/budget.rs)
                if !self.lbterm_view_is_open() {
                    self.lbterm_budget_begin(
                        lbterm_extras::budget::Action::ConductorView,
                        crate::termwindow::budget::Shown::View,
                    );
                }
                self.lbterm_view_open();
                context.lbterm_invalidate();
            }
            WMEK::Press(MousePress::Left) if on_head => {
                self.lbterm_panel.pressed = true;
                let tab = self.lbterm_panel.conductor_tab.and_then(|id| {
                    Mux::get().get_window(self.mux_window_id)?.get_tab_idx_for_id(id)
                });
                match (conductor::left_click(tab.is_some()), tab) {
                    (conductor::PadClick::Open, Some(idx)) => {
                        if let Err(err) = self.activate_tab(idx as isize) {
                            log::error!("conductor: its tab: {err:#}");
                        }
                        context.lbterm_invalidate();
                    }
                    _ => self.lbterm_conductor_click((x as f64, y as f64)),
                }
            }
            WMEK::Press(MousePress::Right) if on_head => {
                self.lbterm_panel.pressed = true;
                self.lbterm_conductor_click((x as f64, y as f64));
            }
            // A suggested link's dashes down the rail's edge: a click makes it; a suggested
            // connection's line, a click connects them (lbterm/LINKS.md)
            WMEK::Press(MousePress::Left) if line.map_or(false, |k| self.lbterm_link_line_click(k)) => {
                self.lbterm_panel.pressed = true;
                context.lbterm_invalidate();
            }
            WMEK::Press(MousePress::Left) => {
                self.lbterm_panel.pressed = true;
                if let Some(idx) = hovered.and_then(|i| self.tab_panel_tab_index(i)) {
                    // The Conductor's view leaves for the tab clicked, the one under it too
                    self.lbterm_view_close();
                    if let Err(err) = self.activate_tab(idx as isize) {
                        log::error!("tab panel: {err:#}");
                    }
                    // A press on a row may become a drag (tab_mouse.rs)
                    self.lbterm_tab_pressed(idx, event, crate::termwindow::tab_mouse::Source::Panel);
                    context.lbterm_invalidate();
                }
            }
            // A right press on a row: dragged, it makes that tab wait on the one it's let go on;
            // let go where it started, the tab menu (tab_links.rs, tab_menu.rs)
            WMEK::Press(MousePress::Right) => {
                self.lbterm_panel.pressed = true;
                if let Some(i) = hovered {
                    let tab = self.tab_panel_rail().get(i).map(|t| t.tab_id());
                    if let Some(tab) = tab {
                        self.lbterm_link_pressed(tab, event);
                    }
                }
            }
            // A middle click on a row closes that tab without asking, as the menu's Close tab
            // does: its close marks written and the tab kept to reopen (Ctrl+Shift+T), the last tab
            // giving way to a new one while the Conductor's pad stays (Lachlan's 10a; lbterm/
            // NEW-TAB.md). Only the row under the pointer: on the Conductor's pad or the + pad it
            // does nothing
            WMEK::Press(MousePress::Middle) => {
                self.lbterm_panel.pressed = true;
                // The tab the row under the pointer was drawn for, not the live list's tab at
                // that position: between a close and the next paint another tab has slid into
                // it, and this close asks nothing (PITFALLS 156)
                let tab = hovered
                    .filter(|_| !on_pad && !on_head)
                    .and_then(|i| self.tab_panel_drawn_tab(i));
                if let Some(tab) = tab {
                    log::info!("tab panel: a middle click closes tab {tab}");
                    self.lbterm_mark_tab(tab);
                    self.lbterm_remove_tab(tab);
                    context.lbterm_invalidate();
                }
            }
            WMEK::Press(_) => self.lbterm_panel.pressed = true,
            _ => {}
        }
        let suggested = line.map_or(false, |k| self.lbterm_link_line_suggested(k));
        context.set_cursor(Some(if hovered.is_some() || on_head || suggested || on_pad {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }));
        true
    }

    /// While a menu holds the mouse, the pointer still reaches the panel's hover, the menu
    /// counting as the panel: on either it stays open, anywhere else it closes as it would
    /// (a Conductor item that waits keeps its menu up for as long as 45 s). The menu keeps
    /// the clicks.
    pub(crate) fn tab_panel_hover_under_menu(&mut self, event: &MouseEvent, context: &dyn WindowOps) {
        if !matches!(event.kind, WMEK::Move) {
            return;
        }
        let Some(layout) = self.lbterm_panel.layout else {
            return;
        };
        let cards = [self.lbterm_tab_menu_card(), self.lbterm_conductor_menu_card()];
        if cards.iter().all(Option::is_none) {
            return;
        }
        let now = Instant::now();
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let area_open = self.lbterm_panel.hover.is_open() || self.lbterm_panel.hover.reopens(now);
        let on_menu = cards.iter().flatten().any(|card| card.contains(x, y));
        let inside = on_menu || layout.inside(x as f32, y as f32, area_open);
        if self.lbterm_panel.hover.pointer(inside, now) {
            context.lbterm_invalidate();
        }
        self.tab_panel_wake(self.lbterm_panel.hover.deadline());
    }

    /// The pointer left the window: the panel closes as if it had left
    /// the panel.
    pub fn tab_panel_mouse_leave(&mut self) {
        if self.lbterm_panel.layout.is_none() {
            return;
        }
        self.lbterm_panel.on_rail = false;
        let changed = self.lbterm_panel.hover.left_window(Instant::now()) | self.lbterm_link_line_leave();
        self.lbterm_tasks_mouse_leave();
        let was_hovering = self.lbterm_panel.hovered.take().is_some()
            | std::mem::take(&mut self.lbterm_panel.head_hovered)
            | self.lbterm_new_tab_pad_pointer(false);
        if changed || was_hovering {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        self.tab_panel_wake(self.lbterm_panel.hover.deadline());
    }

    /// After a composited frame (render/composited.rs): while the panel is held under a menu's
    /// card (`Hover::hold`), the end of the fade out that frame may have just started, so a paint
    /// then lets it go. The paint where the menu closed saw the card still open: its fade starts
    /// in the frame after the paint. With no fade to wait for (motion off, or a card the frame
    /// settled at 0 at once) and no card showing any more, a paint now lets it go: nothing else
    /// would come to, and in an idle window the rail stayed as it was held, closed under the
    /// pointer that rested on it (PITFALLS 183; FLOWS.md, "A menu, from what opens it to the end
    /// of its fade", step 10)
    pub(crate) fn tab_panel_hold_wake(&mut self, fading: Option<Instant>) {
        if !self.lbterm_panel.hover.held() {
            return;
        }
        if let Some(at) = fading {
            self.lbterm_panel.held_due = Some(at);
            self.tab_panel_wake(Some(at));
        } else if !self.lbterm_menu_cards_show(Instant::now()).0 {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// The hub moved when an outline goes without changing what shows now (it let go of a
    /// session, which still shows its least lap and its linger: conductor_hub's `set_focus`): the
    /// rail's wake for then, and no paint now. The wake paints when it comes, as one the last paint
    /// asked for would (`due.1`, the hub's)
    pub(crate) fn tab_panel_hub_wake(&mut self) {
        if self.lbterm_panel.layout.is_none() {
            return;
        }
        let next = crate::conductor_hub::next_change();
        self.lbterm_panel.due.1 = next;
        self.tab_panel_wake(next);
    }

    /// Repaint at `at`: when the hover's wait is up, or a row's age or
    /// timer moves on.
    fn tab_panel_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        // One already due sooner repaints, and the paint asks again
        if matches!(self.lbterm_panel.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_panel.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                // One a sooner wake took the place of repaints nothing: that one's paint asked
                // for whatever was still to come (each hover over the rail left one behind)
                if tw.lbterm_panel.wake != Some(at) {
                    return;
                }
                tw.lbterm_panel.wake = None;
                // It paints only when something the last paint waited for is due now: an age, the
                // pointer's rest, or the hub's bars running out or an outline going. Otherwise it
                // goes to whatever's due next. The hub resends its guesses every 45 s with a later
                // end and the same sessions, which paints nothing, and the wake left at the old
                // end painted a frame for nothing; so did one a paint since had no more use for
                // (the bars gone before their end)
                let now = Instant::now();
                let (aged, hub) = tw.lbterm_panel.due;
                let pointer = [tw.lbterm_panel.hover.deadline(), tw.lbterm_links.line_hover.deadline()];
                let starting = tw.lbterm_panel.starting_due;
                let held = tw.lbterm_panel.held_due;
                // A Hide or Show picked from a tab menu running out (render/conductor_hide.rs)
                let hides = tw.lbterm_panel.hide_due;
                // The Conductor's pad's ring faded out, to take out of the plan (Lachlan's 7a)
                let ring = tw.lbterm_panel.ring_due;
                let due = aged.iter().chain(pointer.iter().flatten()).chain(starting.iter()).chain(held.iter()).chain(hides.iter()).chain(ring.iter()).any(|d| *d <= now)
                    || (hub.is_some_and(|h| h <= now) && crate::conductor_hub::changes_by(now));
                if !due {
                    let next = hub.and_then(|_| crate::conductor_hub::next_change());
                    tw.lbterm_panel.due.1 = next;
                    tw.tab_panel_wake([aged, next, pointer[0], pointer[1], starting, held, hides, ring].iter().flatten().min().copied());
                    return;
                }
                // The rail opening at the end of the pointer's rest, timed from then to the
                // frame that shows it (termwindow/budget.rs)
                let (was_open, rest_ends) = (tw.lbterm_panel.hover.is_open(), tw.lbterm_panel.hover.deadline());
                tw.lbterm_panel.hover.tick(Instant::now());
                if let (false, true, Some(from)) = (was_open, tw.lbterm_panel.hover.is_open(), rest_ends) {
                    tw.lbterm_budget_begin_at(
                        lbterm_extras::budget::Action::Rail,
                        from,
                        crate::termwindow::budget::Shown::Next,
                    );
                }
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }
}
