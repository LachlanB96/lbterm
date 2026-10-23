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

use crate::quad::{QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::box_model::{
    BorderColor, ComputedElement, Element, ElementColors, ElementContent, LayoutContext,
};
use crate::termwindow::compositor::{MotionSettings, PanelPlan, RingLight, RingPlan, RowMotion, RowPlan};
use crate::termwindow::render::conductor::{self as conductor_ring, RingStyle};
use lbterm_extras::conductor::{self, Doing, Level, PaneFact};
use lbterm_panel::words::HeadSays;
use crate::termwindow::render::card::Room;
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
use lbterm_panel::{palette, Area, Blocked, Hover, Layout, Program, ProgramState, Row, Status, Tab};
use mux::pane::{CachePolicy, Pane, PaneId};
use mux::tab::TabId;
use wezterm_term::program_status::{self as osc7501, ProgramStatusRecord};
use mux::Mux;
use std::collections::HashMap;
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

/// Whether a z-layer's quads are the panel's.
pub fn owns_zindex(zindex: i8) -> bool {
    (PANEL_ZINDEX..PANEL_ZINDEX + 10).contains(&zindex)
        || [ROWS_Z, ROW_WORDS_Z, MARKER_Z, PROGRESS_Z].contains(&zindex)
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
            | (RING_SPECTRUM_Z, PanelPart::RingSpectrum)
            | (RING_IRIS_Z, PanelPart::RingIris)
            | (RING_LIGHT_Z, PanelPart::RingLight)
            | (ROWS_Z, PanelPart::Rows)
            | (ROW_WORDS_Z, PanelPart::RowWords)
            | (MARKER_Z, PanelPart::Marker)
            | (PROGRESS_Z, PanelPart::Progress)
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
        head: false,
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

/// The words last laid out, and what they were laid out from.
#[derive(PartialEq)]
struct WordsKey {
    placed: Placed,
    shape_generation: usize,
    font: usize,
    label_font: usize,
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
    hover: Hover,
    /// The row under the pointer
    hovered: Option<usize>,
    /// A button went down on the panel: its release is the panel's too
    pressed: bool,
    /// The compositor's part this frame; `None` with the panel off or no
    /// compositor
    pub plan: Option<PanelPlan>,
    /// Where things were last drawn, for the mouse (and a tab being dragged
    /// along the rows, tab_mouse.rs)
    pub(crate) layout: Option<Layout>,
    laid_out: Option<LaidOut>,
    /// When a repaint is already on its way
    wake: Option<Instant>,
    /// Each tab's active pane's working directory
    cwds: HashMap<PaneId, String>,
    cwds_fetched: Option<Instant>,
    fetching: bool,
    /// When each tab was last on screen, in Unix seconds: results that
    /// arrived since are unread
    seen: HashMap<TabId, f64>,
    /// Tabs whose pads glint at the next paint: a program typed into them, or they were
    /// renamed (lbterm_panel_glint)
    glints: Vec<TabId>,
    /// A tab dragged from another window is over this one: the rail shows where it would
    /// land, after the last tab
    drop_hint: bool,
    /// The Conductor's tab, as the last paint found it: its pad stands for it, so it has no row
    /// (lbterm_extras::conductor::tab_of)
    pub(crate) conductor_tab: Option<TabId>,
    /// The pointer is on the Conductor's pad
    head_hovered: bool,
    /// What the last paint showed, with a state file (lbterm/STATE.md)
    pub(crate) said: Option<lbterm_extras::state::Rail>,
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
            wake: None,
            cwds: HashMap::new(),
            cwds_fetched: None,
            fetching: false,
            seen: HashMap::new(),
            glints: vec![],
            drop_hint: false,
            conductor_tab: None,
            head_hovered: false,
            said: None,
        }
    }
}

impl TabPanelState {
    /// LBTerm: the laid-out words point into a glyph atlas that's gone (gpu_lost.rs)
    pub(crate) fn forget_laid_out(&mut self) {
        self.laid_out = None;
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
    let mut pane_titles = vec![];
    if let Some(pane) = &active_pane {
        pane_titles.push(pane.get_title());
    }
    let mut statuses = vec![];
    let mut programs = vec![];
    // Its Claude session, for its label: the active pane's, else any
    let mut session = None;
    for pos in tab.iter_panes_ignoring_zoom() {
        let is_active = Some(pos.pane.pane_id()) == active_id;
        if !is_active {
            pane_titles.push(pos.pane.get_title());
        }
        let vars = pos.pane.copy_user_vars();
        if let Some(value) = vars.get("claude_status") {
            statuses.extend(Status::parse(value));
        }
        if let Some(sid) = vars.get("claude_session").filter(|s| !s.is_empty()) {
            if session.is_none() || is_active {
                session = Some(sid.clone());
            }
        }
        programs.extend(pos.pane.get_program_status().into_iter().map(program));
    }
    let summary = session.and_then(|sid| crate::summaries::get(&sid)).unwrap_or_default();
    Tab {
        index: idx + 1,
        tab_title: tab.get_title(),
        pane_titles,
        // Where its Claude session is now, as its statusline says: the
        // folder the tab's shell last reported is where the tab opened
        cwd: summary.cwd.unwrap_or_default(),
        statuses,
        programs,
        seen: 0.0,
        active,
        label: summary.label,
    }
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
        let settings = self.config.lbterm_tab_panel.clone();
        if !settings.enabled {
            self.lbterm_panel.layout = None;
            self.lbterm_panel.hovered = None;
            self.lbterm_panel.hover.close();
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
        let motion = MotionSettings::from_config(&self.config.lbterm_motion);
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
        let mut next_change = f64::INFINITY;
        let (tabs, sessions, own) = self.tab_panel_tabs();
        for tab in tabs {
            let (row, change) = lbterm_panel::row(&tab, now, settings.idle_full_minutes * 60.0);
            if let Some(change) = change {
                next_change = next_change.min(change);
            }
            rows.push(row);
        }

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
        if hub {
            // The rail's tabs numbered from 1 as it shows them, the Conductor's own as 0
            let own = own.iter().flat_map(|(tab, active)| {
                tab.iter().map(move |(sid, pane)| PaneFact {
                    session_id: sid.clone(),
                    pane: *pane,
                    tab: 0,
                    active: *active,
                })
            });
            let facts: Vec<PaneFact> = own
                .chain(sessions.iter().enumerate().flat_map(|(i, tab)| {
                    let active = rows.get(i).map_or(false, |r| r.active);
                    tab.iter().map(move |(sid, pane)| PaneFact {
                        session_id: sid.clone(),
                        pane: *pane,
                        tab: i + 1,
                        active,
                    })
                }))
                .collect();
            crate::conductor_hub::panes(&facts);
        }

        let layout = self.tab_panel_layout(rows.len(), head.is_some())?;
        self.lbterm_panel.layout = Some(layout);
        let open = self.lbterm_panel.hover.is_open();
        let light = self.lbterm_panel.hover.light().level();
        // With the compositor the visuals are always drawn open and lit,
        // and DWM shows as much as the moment calls for
        let (drawn_open, glow) = if composited { (true, 1.0) } else { (open, light) };
        self.tab_panel_draw(&rows, &doing, head.as_ref(), &layout, drawn_open, glow)?;

        if composited {
            self.tab_panel_light(&layout)?;
            self.tab_panel_glints(&layout);
            let level = head.as_ref().map(|(_, level)| *level);
            let (rings, ring_template, light_template) = self.tab_panel_rings(&layout, &doing, level)?;
            if let Some(i) = (0..rows.len())
                .take_while(|&i| layout.row_visible(i))
                .find(|&i| rows[i].waiting > 0)
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
            let pick = |curve: &Curve| {
                if motion.enabled {
                    curve.clone()
                } else {
                    Curve::Instant
                }
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
                    .filter(|&i| rows[i].waiting > 0)
                    .map(|i| pixels(layout.spinner(i)))
                    .collect(),
                spin: motion.enabled,
                rows: self.tab_panel_row_plans(&rows, &layout),
                row_motion: RowMotion {
                    slide: pick(&motion.layout),
                    enter: pick(&motion.enter),
                    exit: pick(&motion.exit),
                    swap: pick(&motion.swap),
                    hover: pick(&motion.hover),
                    marker: pick(&motion.focus_move),
                },
                marker: self.tab_panel_marker(&rows, &layout),
                progress: self.tab_panel_progress(&rows, &layout),
                dragged: self.tab_panel_dragged(&layout),
                rings,
                ring_template,
                light_template,
            });
        }

        // What it showed, for the state file (lbterm/STATE.md)
        if crate::state_probe::on() {
            self.lbterm_panel.said = Some(self.tab_panel_said(&rows, &doing, head.as_ref(), &layout));
        }

        // Ages and timers move on with the clock, not with output; an outline the hub let go of
        // goes once it has lingered
        let aged = next_change
            .is_finite()
            .then(|| Instant::now() + Duration::from_secs_f64(next_change + 0.05));
        let outlines = if hub {
            crate::conductor_hub::next_change()
        } else {
            None
        };
        let due = [aged, outlines, self.lbterm_panel.hover.deadline()]
            .iter()
            .flatten()
            .min()
            .copied();
        self.tab_panel_wake(due);
        Ok(())
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

    /// The pads asked to glint, lit over the panel (render/lights.rs)
    fn tab_panel_glints(&mut self, layout: &Layout) {
        let glints = std::mem::take(&mut self.lbterm_panel.glints);
        if glints.is_empty() {
            return;
        }
        let tabs = self.tab_panel_rail_ids();
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
            self.lbterm_light(crate::termwindow::compositor::Light::Pad {
                rect: pixels(pad.grow(layout.px(2.0))),
                radius: (layout.px(6.0)) as f64,
            });
        }
    }

    /// Each visible row for the compositor: its tab, its slot, and a hash of what it shows (the
    /// row, its tab's colour, whether it's the open tab), so a row that moves is followed and
    /// one whose look changes dissolves
    fn tab_panel_row_plans(&self, rows: &[Row], layout: &Layout) -> Vec<RowPlan> {
        let tabs = self.tab_panel_rail_ids();
        rows.iter()
            .enumerate()
            .take_while(|&(i, _)| layout.row_visible(i))
            .filter_map(|(i, row)| {
                let key = *tabs.get(i)?;
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                // How far along it is glides in a layer of its own (`tab_panel_progress`): only
                // whether the row has a bar is part of its look
                let look = Row {
                    progress: row.progress.map(|_| 0),
                    ..row.clone()
                };
                format!("{look:?}").hash(&mut hash);
                crate::tabcolors::rgb(key).hash(&mut hash);
                Some(RowPlan {
                    key,
                    slot: pixels(layout.slot(i)),
                    look: hash.finish(),
                    hovered: self.lbterm_panel.hovered == Some(i),
                })
            })
            .collect()
    }

    /// Each visible row's progress bar on the open panel, and how far along it is
    fn tab_panel_progress(&self, rows: &[Row], layout: &Layout) -> Vec<(usize, PixelRect, f64)> {
        let tabs = self.tab_panel_rail_ids();
        rows.iter()
            .enumerate()
            .take_while(|&(i, _)| layout.row_visible(i))
            .filter_map(|(i, row)| {
                let progress = row.progress?;
                Some((*tabs.get(i)?, pixels(layout.progress_bar(i)), progress as f64 / 100.0))
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
    /// Conductor's slot while its tab is the open one
    fn tab_panel_marker(&self, rows: &[Row], layout: &Layout) -> Option<(usize, i32)> {
        if layout.head && self.tab_panel_conductor_open() {
            return Some((self.lbterm_panel.conductor_tab?, pixels(layout.head_slot()).y));
        }
        let i = (0..rows.len()).take_while(|&i| layout.row_visible(i)).find(|&i| rows[i].active)?;
        let key = *self.tab_panel_rail_ids().get(i)?;
        Some((key, pixels(layout.slot(i)).y))
    }

    /// What the panel shows this frame, for the state file (lbterm/STATE.md)
    fn tab_panel_said(
        &self,
        rows: &[Row],
        doing: &[Option<Doing>],
        head: Option<&(HeadSays, Level)>,
        layout: &Layout,
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
        let said_rows = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let tab = rail.get(i).copied();
                state::RailRow {
                    row: i,
                    tab: tab.unwrap_or_default() as u64,
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
                    }
                    .to_string(),
                    status: row.status.clone(),
                    message: row.message.clone(),
                    progress: row.progress,
                    agents: row.agents,
                    waiting: row.waiting,
                    unread: row.unread,
                    age: row.age.clone(),
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
                }
            })
            .collect();
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
            phase: hover.phase().name().to_string(),
            light: match hover.light() {
                lbterm_panel::Light::Rest => "rest",
                lbterm_panel::Light::Pending => "pending",
                lbterm_panel::Light::Full => "full",
            }
            .to_string(),
            rect: rect(layout.panel(hover.is_open())),
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
                pad: rect(layout.head_pad()),
            }),
            dragging: self
                .lbterm_dragged_row()
                .and_then(|(tab, _)| self.tab_panel_row_of(tab)),
            drop_hint: self.lbterm_panel.drop_hint,
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

    /// Whether the Conductor's tab is the window's open one: its pad has the open tab's ring
    pub(crate) fn tab_panel_conductor_open(&self) -> bool {
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
        // The Conductor's own tab: the one holding the session its state file names
        let found = self.lbterm_conductor_said().and_then(|said| {
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
        }
        let rail = conductor::rail(tabs.len(), found);
        let sessions: Vec<Vec<(String, u64)>> = rail.iter().map(|&i| std::mem::take(&mut sessions[i])).collect();
        let mut out = Vec::with_capacity(rail.len());
        let mut wanted = vec![];
        for (row_idx, &idx) in rail.iter().enumerate() {
            let tab = &tabs[idx];
            let active_pane = tab.get_active_pane();
            let mut row = panel_tab(tab, row_idx, idx == active);
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
        let label = if settings.labels {
            settings.label_lines as f32 * self.tab_panel_label_font()?.1.line_height
        } else {
            0.0
        };
        Ok(Layout {
            left: left - strip,
            body_top: top,
            bottom,
            right,
            rows,
            label,
            head,
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

    /// The panes' background, as the panel's card takes it.
    fn tab_panel_card(&mut self) -> u32 {
        let (r, g, b, _) = self.palette().background.to_srgb_u8();
        ((r as u32) << 16) | ((g as u32) << 8) | b as u32
    }

    /// The panel's quads, drawn `open` or closed, the light at `glow`.
    fn tab_panel_draw(
        &mut self,
        rows: &[Row],
        doing: &[Option<Doing>],
        head: Option<&(HeadSays, Level)>,
        layout: &Layout,
        open: bool,
        glow: f64,
    ) -> anyhow::Result<()> {
        let card = self.tab_panel_card();
        let gl_state = self.render_state.as_ref().unwrap();
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let font_size = words::Font {
            cell_width: metrics.cell_size.width as f32,
            line_height: metrics.cell_size.height as f32,
        };
        let (label_font, label_size) = self.tab_panel_label_font()?;
        let mut placed = words::place(rows, layout, font_size, label_size, column_width);
        // The rows' numbers and labels go with the rows; the Conductor's, after them, stay in
        // the body
        let (row_numbers, row_labels, row_words) = (placed.numbers.len(), placed.labels.len(), placed.words.len());
        if let Some((says, _)) = head {
            let head = words::head(says, layout, font_size, label_size, column_width);
            placed.numbers.extend(head.numbers);
            placed.labels.extend(head.labels);
            placed.words.extend(head.words);
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
        let panel = layout.panel(open);
        let pill_radius = layout.pill_radius();

        {
            // The strip under the panel, the panes' colour, in `over`
            let layer = gl_state.layer_for_zindex(STRIP_Z)?;
            let mut quads = layer.quad_allocator();
            let strip = Area::new(layout.left, layout.body_top, layout.strip(), layout.bottom - layout.body_top);
            self.filled_rectangle(&mut quads, 0, rect(strip), srgb(card))?;
        }

        {
            let layer = gl_state.layer_for_zindex(GLOW_Z)?;
            let mut quads = layer.quad_allocator();
            self.tab_panel_glow(&mut quads, panel, layout, card, glow)?;
        }

        if glow > 0.0 {
            // The ring lit, over the grey one: with the compositor always,
            // and DWM fades it in; without, as lit as the moment calls for
            let layer = gl_state.layer_for_zindex(LIT_Z)?;
            let mut quads = layer.quad_allocator();
            let ring = |y: f32| {
                let lit = palette::ring(((y - panel.y) / panel.h.max(1.0)).clamp(0.0, 1.0) as f64);
                palette::blend(palette::OUTLINE, lit, glow)
            };
            self.tab_panel_ring(&mut quads, panel, layout, ring)?;
        }

        // LBTerm: each tab's colour from its menu, a ring round its pad (tab_menu.rs)
        let accents: Vec<Option<(u8, u8, u8)>> = self
            .tab_panel_rail_ids()
            .into_iter()
            .map(crate::tabcolors::rgb)
            .collect();

        let composited = self.lbterm_composited();
        let (rows_z, marker_z) = if composited { (ROWS_Z, MARKER_Z) } else { (BODY_Z, BODY_Z) };
        let row_words_z = if composited { ROW_WORDS_Z } else { WORDS_Z };
        // The row a drag has picked up, which lifts
        let lifted = self
            .lbterm_dragged_row()
            .and_then(|(tab, _)| self.tab_panel_row_of(tab));
        {
            let layer = gl_state.layer_for_zindex(BODY_Z)?;
            let mut quads = layer.quad_allocator();
            let r = layout.radius().min(panel.w / 2.0).min(panel.h / 2.0).floor();
            let stroke = layout.stroke();
            self.tab_panel_rounded(&mut quads, panel.grow(-stroke), r - stroke, true, true, srgb(card))?;
            self.tab_panel_ring(&mut quads, panel, layout, |_| palette::OUTLINE)?;
        }
        {
            // The rows: their pills and pads
            let layer = gl_state.layer_for_zindex(rows_z)?;
            let mut quads = layer.quad_allocator();
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
                        self.tab_panel_rounded(&mut quads, shadow, pill_radius + grow, true, true, LinearRgba::with_components(0., 0., 0., alpha))?;
                    }
                    self.tab_panel_rounded(&mut quads, pill, pill_radius, true, true, srgb(hover))?;
                }
                if let Some(colour) = fill(row, i) {
                    self.tab_panel_rounded(&mut quads, pill, pill_radius, true, true, srgb(colour))?;
                }
                let under = fill(row, i).unwrap_or(card);
                self.tab_panel_pad(&mut quads, row, i, layout, accents.get(i).copied().flatten(), under)?;
            }
        }
        {
            // The open tab: a ring round its pill, and a bar beside it down the rail's edge
            let layer = gl_state.layer_for_zindex(marker_z)?;
            let mut quads = layer.quad_allocator();
            // The Conductor's tab open: on its pad, which stands for it
            let open_at = if head.is_some() && self.tab_panel_conductor_open() {
                Some((layout.head_pill(), layout.head_edge_bar()))
            } else {
                (0..rows.len())
                    .take_while(|&i| layout.row_visible(i))
                    .find(|&i| rows[i].active)
                    .map(|i| (layout.pill(i), layout.edge_bar(i)))
            };
            if let Some((pill, bar)) = open_at {
                self.tab_panel_active_ring(&mut quads, pill, layout)?;
                self.tab_panel_rounded(&mut quads, bar, bar.w / 2.0, true, true, srgb(palette::ACTIVE))?;
            }
            // A tab from another window over this one: an empty pill ringed in the accent
            // where it would land, after the last tab
            if self.lbterm_panel.drop_hint {
                let mut room = *layout;
                room.rows = rows.len() + 1;
                if room.row_visible(rows.len()) {
                    self.tab_panel_active_ring(&mut quads, room.pill(rows.len()), layout)?;
                }
            }
        }
        {
            let layer = gl_state.layer_for_zindex(BODY_Z)?;
            let mut quads = layer.quad_allocator();
            // The Conductor's pad atop the rail (render/conductor.rs)
            if let Some((_, level)) = head {
                let open_fill = self.tab_panel_conductor_open().then_some(active);
                self.tab_panel_conductor(&mut quads, layout, *level, self.lbterm_panel.head_hovered, open_fill)?;
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
            let layer = gl_state.layer_for_zindex(row_words_z)?;
            let mut quads = layer.quad_allocator();
            for (i, row) in rows.iter().enumerate() {
                if !layout.row_visible(i) {
                    break;
                }
                // The pill grown into the row's long highlight, the same
                // colour, over the rail's own; the open tab's ring goes round
                // all of it. The pad, its timer and its label are drawn
                // again on top (their words too, below)
                if let Some(colour) = fill(row, i) {
                    let highlight = layout.highlight(i);
                    self.tab_panel_rounded(&mut quads, highlight, pill_radius, true, true, srgb(colour))?;
                    if row.active {
                        self.tab_panel_active_ring(&mut quads, highlight, layout)?;
                    }
                    self.tab_panel_pad(&mut quads, row, i, layout, accents.get(i).copied().flatten(), colour)?;
                }
            }
            for chip in &placed.chips {
                self.tab_panel_rounded(&mut quads, *chip, layout.px(6.0), true, true, srgb(palette::CHIP))?;
            }
            // How far along a program says it is
            for (i, row) in rows.iter().enumerate() {
                let Some(progress) = row.progress.filter(|_| layout.row_visible(i)) else {
                    continue;
                };
                let track = layout.progress_bar(i);
                let radius = track.h / 2.0;
                self.tab_panel_rounded(&mut quads, track, radius, true, true, srgb(palette::TRACK))?;
                if composited {
                    // Whole, in its own layer: the compositor shows as much as it says
                    let layer = gl_state.layer_for_zindex(PROGRESS_Z)?;
                    let mut fills = layer.quad_allocator();
                    self.tab_panel_rounded(&mut fills, track, radius, true, true, srgb(palette::status(row.state)))?;
                } else {
                    let fill = Area::new(track.x, track.y, (track.w * progress as f32 / 100.0).round(), track.h);
                    self.tab_panel_rounded(&mut quads, fill, radius, true, true, srgb(palette::status(row.state)))?;
                }
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
            raised,
        };
        let fresh = matches!(&self.lbterm_panel.laid_out, Some(l) if l.key == key);
        if !fresh {
            let label_metrics = RenderMetrics::with_font_metrics(&label_font.metrics());
            let compute = |list: &[&words::Word],
                           zindex: i8,
                           font: &Rc<LoadedFont>,
                           metrics: &RenderMetrics,
                           font_size: words::Font|
             -> anyhow::Result<Vec<ComputedElement>> {
                let mut computed = Vec::with_capacity(list.len());
                for word in list {
                    let element = Element::new(font, ElementContent::Text(word.text.clone()))
                        .colors(ElementColors {
                            border: BorderColor::default(),
                            bg: LinearRgba::TRANSPARENT.into(),
                            text: srgb(word.colour).into(),
                        });
                    computed.push(self.compute_element(
                        &LayoutContext {
                            width: DimensionContext {
                                dpi: self.dimensions.dpi as f32,
                                pixel_max: self.dimensions.pixel_width as f32,
                                pixel_cell: font_size.cell_width,
                            },
                            height: DimensionContext {
                                dpi: self.dimensions.dpi as f32,
                                pixel_max: self.dimensions.pixel_height as f32,
                                pixel_cell: font_size.line_height,
                            },
                            bounds: rect(word.area),
                            metrics,
                            gl_state,
                            zindex,
                        },
                        &element,
                    )?);
                }
                Ok(computed)
            };
            let placed = &key.placed;
            let all: [Vec<&words::Word>; 3] = [&placed.numbers, &placed.labels, &placed.words]
                .map(|list| list.iter().collect());
            let [numbers, labels, words] = all;
            let lifted: [Vec<&words::Word>; 2] = [&placed.numbers, &placed.labels]
                .map(|list| list.iter().filter(|w| key.raised.contains(&w.row)).collect());
            let [raised_numbers, raised_labels] = lifted;
            let rows_z = if self.lbterm_composited() { ROWS_Z } else { BODY_Z };
            let (head_numbers, head_labels) = (
                numbers.get(row_numbers..).unwrap_or(&[]).to_vec(),
                labels.get(row_labels..).unwrap_or(&[]).to_vec(),
            );
            let mut numbers = compute(numbers.get(..row_numbers).unwrap_or(&numbers), rows_z, &font, &metrics, font_size)?;
            numbers.extend(compute(&head_numbers, BODY_Z, &font, &metrics, font_size)?);
            let mut labels = compute(labels.get(..row_labels).unwrap_or(&labels), rows_z, &label_font, &label_metrics, label_size)?;
            labels.extend(compute(&head_labels, BODY_Z, &label_font, &label_metrics, label_size)?);
            // The rows' words go with the rows; the Conductor's stay in the words
            let row_words_z = if self.lbterm_composited() { ROW_WORDS_Z } else { WORDS_Z };
            let head_words = words.get(row_words..).unwrap_or(&[]).to_vec();
            let mut words = compute(words.get(..row_words).unwrap_or(&words), row_words_z, &font, &metrics, font_size)?;
            words.extend(compute(&head_words, WORDS_Z, &font, &metrics, font_size)?);
            let mut raised = compute(&raised_numbers, row_words_z, &font, &metrics, font_size)?;
            raised.extend(compute(&raised_labels, row_words_z, &label_font, &label_metrics, label_size)?);
            self.lbterm_panel.laid_out = Some(LaidOut {
                key,
                numbers,
                labels,
                words,
                raised,
            });
        }
        if let Some(laid_out) = &self.lbterm_panel.laid_out {
            for element in laid_out.numbers.iter().chain(&laid_out.labels) {
                self.render_element(element, gl_state, None)?;
            }
            if open {
                for element in laid_out.words.iter().chain(&laid_out.raised) {
                    self.render_element(element, gl_state, None)?;
                }
            }
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
    fn tab_panel_bands(
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
        let gl_state = self.render_state.as_ref().unwrap();
        let centre = layout.sweep_point(0.0);
        for (zindex, surface, light, peak) in [
            (SPOT_Z, layout.spot_surface(), palette::SPOT, palette::SPOT_PEAK),
            (SPOT_CORE_Z, layout.spot_core_surface(), palette::SPOT_CORE, palette::SPOT_CORE_PEAK),
        ] {
            let layer = gl_state.layer_for_zindex(zindex)?;
            let mut quads = layer.quad_allocator();
            let reach = (surface.w - 1.0) / 2.0;
            let bands = light_bands(light, reach, 12, |d| {
                palette::light_alpha(d, reach as f64, card, light, peak)
            });
            self.tab_panel_disc(&mut quads, centre, &bands)?;
        }
        Ok(())
    }

    /// The Conductor's outlines, with the compositor (render/conductor.rs): where each goes,
    /// round its own pad while it may act on its own and round each tab it's on, a light going
    /// round those it thinks about (a glint) or acts on; and their templates, drawn once round one
    /// of those pills (every pill is the same size) into z-layers only the outlines' visuals show.
    /// Also the templates' places: the outline's, and the light's.
    fn tab_panel_rings(
        &mut self,
        layout: &Layout,
        doing: &[Option<Doing>],
        level: Option<Level>,
    ) -> anyhow::Result<(Vec<RingPlan>, PixelRect, PixelRect)> {
        /// Where the light is, this many times a lap
        const LIGHT_STEPS: usize = 90;
        let margin = conductor_ring::ring_margin(layout);
        let reach = conductor_ring::light_reach(layout).ceil();
        let around = |pill: Area| pixels(pill.grow(margin));
        let mut pills = vec![];
        let mut rings = vec![];
        if level.map_or(false, |level| conductor::spectrum(level, self.lbterm_panel.head_hovered)) {
            pills.push(layout.head_pill());
            rings.push(RingPlan {
                rect: around(layout.head_pill()),
                style: RingStyle::Spectrum,
                light: None,
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
            let offsets = (0..LIGHT_STEPS)
                .map(|k| {
                    let (x, y) = lbterm_panel::layout::outline_point(path, radius, k as f64 / LIGHT_STEPS as f64);
                    (x - reach, y - reach)
                })
                .collect();
            pills.push(pill);
            rings.push(RingPlan {
                rect: around(pill),
                style: RingStyle::Iridescent,
                light: Some(RingLight { lap, opacity, offsets }),
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
        let gl_state = self.render_state.as_ref().unwrap();
        for (zindex, style) in [
            (RING_SPECTRUM_Z, RingStyle::Spectrum),
            (RING_IRIS_Z, RingStyle::Iridescent),
        ] {
            let layer = gl_state.layer_for_zindex(zindex)?;
            let mut quads = layer.quad_allocator();
            self.tab_panel_rainbow(&mut quads, layout, template, style)?;
        }
        let layer = gl_state.layer_for_zindex(RING_LIGHT_Z)?;
        let mut quads = layer.quad_allocator();
        self.tab_panel_conductor_light(&mut quads, layout, (cx, cy))?;
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
        for (b, bar) in row.bars.iter().enumerate() {
            let area = layout.bar(i, b);
            self.tab_panel_rounded(quads, area, layout.px(1.5), true, true, srgb(palette::bar(*bar)))?;
        }
        let pad = layout.pad(i);
        if let Some((r, g, b)) = accent {
            let ring = LinearRgba::with_srgba(r, g, b, 255);
            self.tab_panel_rounded(quads, pad.grow(layout.px(2.0)), layout.px(9.0), true, true, ring)?;
        }
        self.tab_panel_rounded(quads, pad, layout.px(7.0), true, true, srgb(palette::pad(row.state)))?;
        if row.waiting > 0 {
            // The disc, of what's under the pad, so the arc reads apart from
            // it; the arc on it turns with the compositor, in a visual of its
            // own (SPIN_Z), and is drawn here, still, without
            let area = layout.spinner(i);
            self.tab_panel_spinner(quads, area, layout, Some(under), !self.lbterm_composited())?;
        } else if row.unread {
            let dot = layout.unread_dot(i);
            self.tab_panel_rounded(quads, dot, dot.w / 2.0, true, true, srgb(palette::UNREAD))?;
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
        let green = palette::pad(lbterm_panel::RowState::Working);
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
        // While it closes, coming back onto what's left of it opens it again
        let area_open = was_open || self.lbterm_panel.hover.reopens(now);
        let inside = !busy && layout.inside(x, y, area_open);
        let changed = self.lbterm_panel.hover.pointer(inside, now);
        let hovered = if inside {
            layout.row_at(x, y, self.lbterm_panel.hover.is_open())
        } else {
            None
        };
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
            context.invalidate();
        }
        self.tab_panel_wake(self.lbterm_panel.hover.deadline());

        if !inside {
            let pressed = self
                .webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(false, |c| c.panes_pressed());
            if pressed && was_open && matches!(event.kind, WMEK::Press(_)) {
                self.lbterm_panel.hover.close();
                self.lbterm_panel.pressed = true;
                context.invalidate();
                return true;
            }
            return false;
        }

        match event.kind {
            // The Conductor's pad is its tab: a left click opens it, and drops its menu while
            // there's no tab to open; a right click drops the menu (render/conductor.rs)
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
                        context.invalidate();
                    }
                    _ => self.lbterm_conductor_click((x as f64, y as f64)),
                }
            }
            WMEK::Press(MousePress::Right) if on_head => {
                self.lbterm_panel.pressed = true;
                self.lbterm_conductor_click((x as f64, y as f64));
            }
            WMEK::Press(MousePress::Left) => {
                self.lbterm_panel.pressed = true;
                if let Some(idx) = hovered.and_then(|i| self.tab_panel_tab_index(i)) {
                    if let Err(err) = self.activate_tab(idx as isize) {
                        log::error!("tab panel: {err:#}");
                    }
                    // A press on a row may become a drag (tab_mouse.rs)
                    self.lbterm_tab_pressed(idx, event, crate::termwindow::tab_mouse::Source::Panel);
                    context.invalidate();
                }
            }
            // The tab menu (tab_menu.rs)
            WMEK::Press(MousePress::Right) => {
                self.lbterm_panel.pressed = true;
                if let Some(i) = hovered {
                    let tab = self.tab_panel_rail().get(i).map(|t| t.tab_id());
                    if let Some(tab) = tab {
                        self.lbterm_open_tab_menu(tab, (x as f64, y as f64));
                    }
                }
            }
            WMEK::Press(_) => self.lbterm_panel.pressed = true,
            _ => {}
        }
        context.set_cursor(Some(if hovered.is_some() || on_head {
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
            context.invalidate();
        }
        self.tab_panel_wake(self.lbterm_panel.hover.deadline());
    }

    /// The pointer left the window: the panel closes as if it had left
    /// the panel.
    pub fn tab_panel_mouse_leave(&mut self) {
        if self.lbterm_panel.layout.is_none() {
            return;
        }
        let changed = self.lbterm_panel.hover.left_window(Instant::now());
        let was_hovering = self.lbterm_panel.hovered.take().is_some()
            | std::mem::take(&mut self.lbterm_panel.head_hovered);
        if changed || was_hovering {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        self.tab_panel_wake(self.lbterm_panel.hover.deadline());
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
            smol::Timer::at(at).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_panel.wake == Some(at) {
                    tw.lbterm_panel.wake = None;
                }
                tw.lbterm_panel.hover.tick(Instant::now());
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }
}
