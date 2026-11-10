//! LBTerm: the objectives panel down the right of the window (lbterm/OBJECTIVES.md). Lachlan's
//! ask, 2026-10-24: with sixteen Claude sessions open he couldn't tell what each was started for,
//! or whether it got there before it went off on something else.
//!
//! Closed, it's a strip the window gives up like a border (borders.rs), ringed with the tab
//! rail's grey hairline, with a mark level with each tab's pad on the rail: what became of what
//! the tab's Claude session was started for (lbterm_extras::objectives::Mark), from the
//! `<sid>.objectives.json` Terminal's objectives.ts keeps beside its summary, read off this thread
//! (crate::summaries). Resting the pointer on the strip opens a card over the panes beside it,
//! listing each tab's first objective on its row, level with its pad; resting on a row there
//! opens a second card beside that with the session's whole list, each objective's status and
//! what showed it. A click on a row opens its tab.
//!
//! The strip and its marks are drawn in `over` and never move: as shapes for render/sides.rs,
//! which draws them there and, as a tab is switched from or closed, fades the strip as that tab
//! showed it out over the next tab's with its panes (the marks of the rows below a closed tab
//! move up). The cards fade, the strip's card's way (render/strip.rs): with the compositor each
//! is a visual of its own, over the panes and under the tab panel, faded in on `pane_enter` as it
//! opens and out on `pane_exit` as it closes, drawn as it was until it has faded; without it they
//! come and go at once.

use crate::termwindow::render::card::Room;
use crate::termwindow::render::sides;
use crate::termwindow::render::tab_panel::srgb;
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::{ConfigHandle, DimensionContext, SrgbaTuple};
use lbterm_extras::objectives::{self, Detail, List, Mark, Objectives, RowIn, Tone};
use lbterm_extras::strip::{self as strip_card, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Hover, Layout, Shape};
use std::sync::Arc;
use std::time::{Duration, Instant};
use window::color::LinearRgba;

/// The closed strip: the panes' colour, its ring, the marks and the projects pad, in `over`, and
/// the scroll strip beside it (render/sides.rs draws them all)
pub(crate) const STRIP_Z: i8 = 38;
/// Each card's shadow, card and words: the list's, the detail's, the projects card's
/// (render/projects.rs). With the compositor each card's are its visual's alone
/// (`Target::Objectives`)
pub(crate) const CARD: [[i8; 3]; 4] = [[39, 40, 41], [42, 43, 44], [27, 28, 29], [16, 17, 18]];

/// Which card a z-layer is: 0 the list, 1 the detail, 2 the projects card, 3 the scroll strip's
/// (render/scroll_strip.rs)
pub fn card_of(zindex: i8) -> Option<usize> {
    CARD.iter().position(|c| c.contains(&zindex))
}

/// White over the card, as the eye sees it (render/strip.rs's way)
pub(crate) const TEXT_ALPHA: f32 = 0.85;
pub(crate) const NAME_ALPHA: f32 = 0.45;
const TALLY_ALPHA: f32 = 0.50;
pub(crate) const TITLE_ALPHA: f32 = 0.92;
pub(crate) const EVIDENCE_ALPHA: f32 = 0.55;
const NOW_ALPHA: f32 = 0.50;
const RING_ALPHA: f32 = 0.14;
/// A row under the pointer: white this far over the card
pub(crate) const ROW_HOVER_ALPHA: f32 = 0.06;
/// The strip's ring under the pointer or open: its grey this far to white
const STRIP_LIT: f64 = 0.35;

/// Whether the panel is on: asked for, with the tab panel to line up with and summaries to read
fn on(config: &ConfigHandle) -> bool {
    config.lbterm_objectives.is_some() && config.lbterm_tab_panel.enabled && config.lbterm_summaries.is_some()
}

/// The strip's own width, its ring included, in pixels
fn width_px(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    config.lbterm_objectives.as_ref().map_or(0.0, |o| {
        o.width
            .evaluate_as_pixels(DimensionContext {
                dpi: dimensions.dpi as f32,
                pixel_max: dimensions.pixel_width as f32,
                pixel_cell: metrics.cell_size.width as f32,
            })
            .round()
    })
}

/// The strip the window keeps down its right for the closed panel (the strip, and a gutter
/// between it and the window's edge), 0 with the panel off. The window treats it as border, so
/// the panes' last card sits a gutter short of it; the tab bar takes it back (fancy_tab_bar.rs).
/// It never changes as the cards open, so nothing is resized.
pub fn strip_width(config: &ConfigHandle, dimensions: &crate::Dimensions, metrics: &RenderMetrics) -> f32 {
    if !on(config) {
        return 0.0;
    }
    width_px(config, dimensions, metrics) + Shape::new(dimensions.dpi as f32 / 96.0).gutter()
}

pub struct ObjectivesState {
    hover: Hover,
    /// The list's row under the pointer, whose detail the second card shows
    row: Option<usize>,
    /// The strip, as last drawn, ring included
    strip: Option<Rect>,
    /// Each visible rail row's mark, as last drawn, and where it goes
    marks: Vec<(usize, Option<Mark>, Rect)>,
    /// The cards while they're open, as last drawn: what the pointer can be on
    list: Option<List>,
    detail: Option<Detail>,
    /// The cards last drawn, which a closing card fades out showing
    last_list: Option<List>,
    last_detail: Option<Detail>,
    /// Where each card's visual goes this frame, and whether it's opening
    visuals: [Option<(PixelRect, bool)>; 2],
    /// When a repaint is already on its way
    wake: Option<Instant>,
    /// A button went down on the panel: its release is the panel's too
    pressed: bool,
    /// The projects pad at the top of the strip, and its card (render/projects.rs)
    pub(crate) projects: super::projects::ProjectsState,
    /// Each card's words as its last frame laid them out, in `CARD`'s order (render/words.rs)
    pub(crate) words: [std::cell::RefCell<super::words::Words>; 4],
}

impl Default for ObjectivesState {
    fn default() -> Self {
        Self {
            hover: Hover::new(Duration::from_millis(300), Duration::from_millis(200)),
            row: None,
            strip: None,
            marks: vec![],
            list: None,
            detail: None,
            last_list: None,
            last_detail: None,
            visuals: [None, None],
            wake: None,
            pressed: false,
            projects: Default::default(),
            words: Default::default(),
        }
    }
}

/// `alpha` of the way from `surface` to white, as one solid colour
pub(crate) fn over(surface: SrgbaTuple, alpha: f32) -> LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

fn grow(r: Rect, by: f64, down: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by + down,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

/// A mark at `at`: filled, or a ring round the surface's colour
fn mark_shapes(at: Rect, mark: Mark, surface: SrgbaTuple, scale: f64) -> Vec<sides::Shape> {
    let mut shapes = vec![sides::Shape::new(at, at.w / 2.0, srgb(mark.colour()))];
    if !mark.filled() {
        let ring = (2.0 * scale).round().max(1.0);
        let inner = grow(at, -ring, 0.0);
        shapes.push(sides::Shape::new(inner, inner.w / 2.0, surface.to_linear()));
    }
    shapes
}

/// A tab's Claude session, as the rail names its label from it: its active pane's, else any
/// pane's (render/tab_panel.rs, `panel_tab`). Terminal's claudehook empties it as the session ends
pub(crate) fn session_of(tab: &mux::tab::Tab) -> Option<String> {
    let active = tab.get_active_pane().map(|p| p.pane_id());
    let mut session = None;
    for pos in tab.iter_panes_ignoring_zoom() {
        let vars = pos.pane.copy_user_vars();
        if let Some(sid) = vars.get("claude_session").filter(|s| !s.is_empty()) {
            if session.is_none() || Some(pos.pane.pane_id()) == active {
                session = Some(sid.clone());
            }
        }
    }
    session
}

/// One rail row's tab, as the panel uses it
struct RowTab {
    row: usize,
    middle: f64,
    objectives: Option<Arc<Objectives>>,
    /// What the rail calls it: its name, else its session's label, else its title
    name: String,
}

impl crate::TermWindow {
    /// Each paint, after the tab panel (whose rows the marks line up with): the strip, its marks,
    /// and the cards while they're open or fading
    pub(crate) fn paint_lbterm_objectives(&mut self) -> anyhow::Result<()> {
        let layout = match self.lbterm_panel.layout {
            Some(layout) if on(&self.config) => layout,
            _ => {
                let state = &mut self.lbterm_objectives;
                state.hover.close();
                (state.strip, state.list, state.detail, state.row) = (None, None, None, None);
                state.marks.clear();
                state.visuals = [None, None];
                state.projects.reset();
                return Ok(());
            }
        };
        let settings = self.config.lbterm_objectives.clone().unwrap_or_default();
        let motion = self.lbterm_motion();
        let composited = self.lbterm_composited();
        let closing = if composited && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        let hover = &mut self.lbterm_objectives.hover;
        hover.set_delays(
            Duration::from_millis(settings.open_delay_ms),
            Duration::from_millis(settings.close_delay_ms),
        );
        // Coming back while a card fades out opens it again at once (lbterm_panel::hover)
        hover.set_motion(Duration::ZERO, closing);

        let room = self.lbterm_room()?;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let panel = layout.panel(false);
        // Right of the scroll strip, when it's on (render/scroll_strip.rs)
        let strip = Rect {
            x: room.right as f64 + super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64,
            y: panel.y as f64,
            w: width_px(&self.config, &self.dimensions, &self.render_metrics) as f64,
            h: panel.h as f64,
        };
        let rows = self.lbterm_objectives_rows(&layout);
        let marks: Vec<(usize, Option<Mark>, Rect)> = rows
            .iter()
            .map(|r| {
                let at = objectives::strip_mark(strip.x, strip.w, r.middle, scale);
                (r.row, r.objectives.as_ref().and_then(|o| o.mark()), at)
            })
            .collect();
        let surface = self.palette().background;
        let open = self.lbterm_objectives.hover.is_open();
        let lit = open || self.lbterm_objectives.hover.light() != lbterm_panel::Light::Rest;
        let shapes = self.lbterm_objectives_draw_strip(&layout, room, strip, &marks, surface, lit, scale);
        self.lbterm_sides_draw(shapes);
        self.lbterm_objectives.strip = Some(strip);
        self.lbterm_objectives.marks = marks;

        // The cards: laid out afresh while open, and with the compositor drawn as they were while
        // their visuals fade out
        let (fading, settles) = if composited {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(([false; 4], None), |c| c.objectives_showing(Instant::now()))
        } else {
            ([false; 4], None)
        };
        // The projects pad on the strip, and its card (render/projects.rs)
        self.paint_lbterm_projects(&layout, strip, surface, fading[2], scale)?;
        if !open {
            self.lbterm_objectives.row = None;
        }
        let list = match strip_card::card_draw(open, fading[0]) {
            CardDraw::Fresh => self.lbterm_objectives_lay_out_list(&rows, room, &settings, scale)?,
            CardDraw::Last => self.lbterm_objectives.last_list.clone(),
            CardDraw::None => None,
        };
        // The list opens on a timer, with no move to say which row the pointer's on: the row
        // level with where it last was, on the list or the strip
        if let (true, None, Some(list), Some(event)) = (open, self.lbterm_objectives.row, list.as_ref(), self.current_mouse_event.as_ref()) {
            let (x, y) = (event.coords.x as f64, event.coords.y as f64);
            if x >= list.rect.x {
                self.lbterm_objectives.row = list.rows.iter().find(|r| y >= r.rect.y && y < r.rect.y + r.rect.h).map(|r| r.row);
            }
        }
        let row = self.lbterm_objectives.row;
        let detail_open = open && row.is_some();
        let detail = match strip_card::card_draw(detail_open, fading[1]) {
            CardDraw::Fresh => match (&list, row) {
                (Some(list), Some(row)) => self.lbterm_objectives_lay_out_detail(&rows, list, row, &layout, room, &settings, scale)?,
                _ => None,
            },
            CardDraw::Last => self.lbterm_objectives.last_detail.clone(),
            CardDraw::None => None,
        };
        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        let visual = |rect: &Rect, opening: bool| {
            strip_card::visual_bounds(rect, scale, window).map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), opening))
        };
        self.lbterm_objectives.visuals = [
            list.as_ref().and_then(|l| visual(&l.rect, open)),
            detail.as_ref().and_then(|d| visual(&d.rect, detail_open && d.rect.w > 0.0)),
        ];
        // Only open cards take the pointer
        self.lbterm_objectives.list = if open { list.clone() } else { None };
        self.lbterm_objectives.detail = if detail_open { detail.clone() } else { None };
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        if let Some(list) = list {
            self.lbterm_objectives_draw_list(&list, surface, radius, stroke, scale)?;
            self.lbterm_objectives.last_list = Some(list);
        }
        if let Some(detail) = detail {
            self.lbterm_objectives_draw_detail(&detail, surface, radius, stroke, scale)?;
            self.lbterm_objectives.last_detail = Some(detail);
        }
        // A card closing is drawn until it has faded, then not
        self.lbterm_objectives_wake(settles);
        let due = [self.lbterm_objectives.hover.deadline(), self.lbterm_objectives.projects.deadline()]
            .iter()
            .flatten()
            .min()
            .copied();
        self.lbterm_objectives_wake(due);
        Ok(())
    }

    /// The rail's visible rows, each with its pad's middle, its session's objectives and its name
    fn lbterm_objectives_rows(&self, layout: &Layout) -> Vec<RowTab> {
        self.tab_panel_rail()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < layout.rows && layout.row_visible(*i))
            .map(|(i, tab)| {
                let summary = session_of(tab).and_then(|sid| crate::summaries::get(&sid));
                let pad = layout.pad(i);
                // As the rail names it: the name it was given, else its session's label, else
                // its active pane's title
                let given = tab.get_title();
                let name = if !given.is_empty() {
                    given
                } else if let Some(label) = summary.as_ref().and_then(|s| s.label.clone()).filter(|l| !l.is_empty()) {
                    label
                } else {
                    tab.get_active_pane().map(|p| p.get_title()).unwrap_or_default()
                };
                RowTab {
                    row: i,
                    middle: (pad.y + pad.h / 2.0) as f64,
                    objectives: summary.and_then(|s| s.objectives.clone()),
                    name,
                }
            })
            .collect()
    }

    /// The strip: the panes' colour, its ring, lit while the pointer is on it or the cards are
    /// open, and each row's mark, as shapes for `over` (render/sides.rs)
    fn lbterm_objectives_draw_strip(
        &self,
        layout: &Layout,
        room: Room,
        strip: Rect,
        marks: &[(usize, Option<Mark>, Rect)],
        surface: SrgbaTuple,
        lit: bool,
        scale: f64,
    ) -> Vec<sides::Shape> {
        let shape = self.lbterm_shape();
        // Under it, from the panes' room (past the scroll strip's) to the window's edge (inside any
        // border of the frame's own), under the tab bar: the panes' colour over the border's
        let from = room.right + super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics);
        let edge = from as f64 + strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let under = Rect {
            x: from as f64,
            y: layout.body_top as f64,
            w: edge - from as f64,
            h: (layout.bottom - layout.body_top) as f64,
        };
        let outline = if lit {
            palette::blend(palette::OUTLINE, 0xFFFFFF, STRIP_LIT)
        } else {
            palette::OUTLINE
        };
        let stroke = shape.hairline() as f64;
        let radius = (shape.radius() as f64).min(strip.w / 2.0);
        let mut shapes = vec![
            sides::Shape::new(under, 0.0, surface.to_linear()),
            sides::Shape::new(strip, radius, srgb(outline)),
            sides::Shape::new(grow(strip, -stroke, 0.0), radius - stroke, surface.to_linear()),
        ];
        for (_, mark, at) in marks {
            if let Some(mark) = mark {
                shapes.extend(mark_shapes(*at, *mark, surface, scale));
            }
        }
        shapes
    }

    /// The title font, and a column's width and a line's height in it
    pub(crate) fn lbterm_objectives_font(&self) -> anyhow::Result<(std::rc::Rc<wezterm_font::LoadedFont>, RenderMetrics, (f64, f64))> {
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (font.metrics().cell_width.get(), metrics.cell_size.height as f64);
        Ok((font, metrics, cell))
    }

    /// A card's width in pixels, at most what the room leaves
    fn lbterm_objectives_card_width(&self, settings: &config::lbterm::LbtermObjectives, room: f64) -> f64 {
        let w = settings.card_width.evaluate_as_pixels(DimensionContext {
            dpi: self.dimensions.dpi as f32,
            pixel_max: self.dimensions.pixel_width as f32,
            pixel_cell: self.render_metrics.cell_size.width as f32,
        }) as f64;
        w.min(room).max(0.0).round()
    }

    /// The list card, its right edge a gutter short of the strip, over the panes' last card
    fn lbterm_objectives_lay_out_list(
        &self,
        rows: &[RowTab],
        room: Room,
        settings: &config::lbterm::LbtermObjectives,
        scale: f64,
    ) -> anyhow::Result<Option<List>> {
        let (_, _, cell) = self.lbterm_objectives_font()?;
        let gutter = self.lbterm_shape().gutter() as f64;
        let right = room.right as f64 - gutter;
        let width = self.lbterm_objectives_card_width(settings, right - room.left as f64 - gutter);
        let height = self.lbterm_panel.layout.map_or(0.0, |l| l.row_height() as f64);
        let input: Vec<RowIn> = rows
            .iter()
            .map(|r| RowIn {
                row: r.row,
                middle: r.middle,
                height,
                objectives: r.objectives.as_deref(),
                name: &r.name,
            })
            .collect();
        Ok(objectives::list(&input, right, width, cell, scale))
    }

    /// The detail card for rail row `row`, a gutter left of the list, level with the row
    fn lbterm_objectives_lay_out_detail(
        &self,
        rows: &[RowTab],
        list: &List,
        row: usize,
        layout: &Layout,
        room: Room,
        settings: &config::lbterm::LbtermObjectives,
        scale: f64,
    ) -> anyhow::Result<Option<Detail>> {
        let Some(tab) = rows.iter().find(|r| r.row == row) else {
            return Ok(None);
        };
        let Some(o) = tab.objectives.as_ref() else {
            return Ok(None);
        };
        let Some(at) = list.rows.iter().find(|r| r.row == row) else {
            return Ok(None);
        };
        let (_, _, cell) = self.lbterm_objectives_font()?;
        let gutter = self.lbterm_shape().gutter() as f64;
        let right = list.rect.x - gutter;
        let width = self.lbterm_objectives_card_width(settings, right - room.left as f64 - gutter);
        if width < 20.0 * cell.0 {
            return Ok(None);
        }
        let gap = layout.gap() as f64;
        Ok(objectives::detail(
            &tab.name,
            o,
            right,
            (at.rect.y, layout.body_top as f64 + gap, layout.bottom as f64 - gap),
            width,
            cell,
            scale,
        ))
    }

    /// A card's shadow, its hairline and fill, in card `which`'s z-layers
    pub(crate) fn lbterm_objectives_card(&self, which: usize, rect: Rect, surface: SrgbaTuple, radius: f64, stroke: f64, scale: f64) -> anyhow::Result<()> {
        let gl_state = self.render_state.as_ref().unwrap();
        {
            let layer = gl_state.layer_for_zindex(CARD[which][0])?;
            let mut quads = layer.quad_allocator();
            // Out to strip::shadow_reach, which the card's visual holds
            let (reach, down) = strip_card::shadow_reach(scale);
            for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                let shadow = LinearRgba::with_components(0., 0., 0., *alpha);
                self.lbterm_menu_rounded(&mut quads, grow(rect, by, down), radius + by, shadow)?;
            }
        }
        let layer = gl_state.layer_for_zindex(CARD[which][1])?;
        let mut quads = layer.quad_allocator();
        self.lbterm_menu_rounded(&mut quads, grow(rect, stroke, 0.), radius + stroke, over(surface, RING_ALPHA))?;
        self.lbterm_menu_rounded(&mut quads, rect, radius, surface.to_linear())?;
        Ok(())
    }

    /// Words at `x`, `y`, `w` wide, in `colour`, in card `which`'s words' z-layer: each laid out
    /// only when the card's last frame didn't have it (render/words.rs)
    pub(crate) fn lbterm_objectives_words(&self, which: usize, words: &[(f64, f64, f64, String, LinearRgba)]) -> anyhow::Result<()> {
        let (font, metrics, cell) = self.lbterm_objectives_font()?;
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(
            &self.lbterm_objectives.words[which],
            &font,
            &metrics,
            room,
            CARD[which][2],
            words.iter().map(|(x, y, w, text, colour)| {
                (text.clone(), euclid::rect(*x as f32, *y as f32, w.max(cell.0) as f32, cell.1 as f32), *colour)
            }),
        )
    }

    fn lbterm_objectives_draw_list(&self, list: &List, surface: SrgbaTuple, radius: f64, stroke: f64, scale: f64) -> anyhow::Result<()> {
        self.lbterm_objectives_card(0, list.rect, surface, radius, stroke, scale)?;
        let gl_state = self.render_state.as_ref().unwrap();
        let (_, _, cell) = self.lbterm_objectives_font()?;
        {
            let layer = gl_state.layer_for_zindex(CARD[0][1])?;
            let mut quads = layer.quad_allocator();
            for r in &list.rows {
                if Some(r.row) == self.lbterm_objectives.row {
                    let inset = 4.0 * scale;
                    let band = Rect { x: r.rect.x + inset, y: r.rect.y + 1.0, w: r.rect.w - 2.0 * inset, h: r.rect.h - 2.0 };
                    self.lbterm_menu_rounded(&mut quads, band, radius / 2.0, over(surface, ROW_HOVER_ALPHA))?;
                }
                if let Some(mark) = r.mark {
                    let d = (objectives::MARK * scale).round();
                    let at = Rect { x: (r.mark_x - d / 2.0).round(), y: (r.text_y + (cell.1 - d) / 2.0).round(), w: d, h: d };
                    let under = if Some(r.row) == self.lbterm_objectives.row {
                        // the ring's middle is the row's highlight
                        let c = surface;
                        let a = ROW_HOVER_ALPHA;
                        SrgbaTuple(c.0 + (1. - c.0) * a, c.1 + (1. - c.1) * a, c.2 + (1. - c.2) * a, 1.)
                    } else {
                        surface
                    };
                    self.lbterm_shapes_into(&mut quads, &mark_shapes(at, mark, under, scale))?;
                }
            }
        }
        let mut words = vec![];
        for r in &list.rows {
            let alpha = if r.has_objectives { TEXT_ALPHA } else { NAME_ALPHA };
            words.push((r.text_x, r.text_y, r.tally_x - r.text_x, r.text.clone(), over(surface, alpha)));
            if !r.tally.is_empty() {
                let right = list.rect.x + list.rect.w;
                words.push((r.tally_x, r.text_y, right - r.tally_x, r.tally.clone(), over(surface, TALLY_ALPHA)));
            }
        }
        self.lbterm_objectives_words(0, &words)
    }

    fn lbterm_objectives_draw_detail(&self, detail: &Detail, surface: SrgbaTuple, radius: f64, stroke: f64, scale: f64) -> anyhow::Result<()> {
        self.lbterm_objectives_card(1, detail.rect, surface, radius, stroke, scale)?;
        let right = detail.rect.x + detail.rect.w;
        let words: Vec<_> = detail
            .lines
            .iter()
            .map(|l| {
                let colour = match l.tone {
                    Tone::Title => over(surface, TITLE_ALPHA),
                    Tone::Status(status) => srgb(objectives::status_colour(status)),
                    Tone::Now => over(surface, NOW_ALPHA),
                    Tone::Text => over(surface, TEXT_ALPHA),
                    Tone::Evidence => over(surface, EVIDENCE_ALPHA),
                };
                (l.x, l.y, right - l.x, l.text.clone(), colour)
            })
            .collect();
        self.lbterm_objectives_words(1, &words)
    }

    /// Where each card's visual goes this frame and whether it's opening, for the compositor: the
    /// list, the detail, the projects card, the scroll strip's card (render/scroll_strip.rs)
    pub(crate) fn lbterm_objectives_visuals(&self) -> [Option<(PixelRect, bool)>; 4] {
        let [list, detail] = self.lbterm_objectives.visuals;
        [list, detail, self.lbterm_objectives.projects.visual(), self.lbterm_scroll_strip_visual()]
    }

    /// The pointer moved, pressed or released: the list opens once it has rested on the strip
    /// and stays while it's on the strip or a card; a row under it shows its detail; a click on
    /// one opens its tab. True when the panel took the event: nothing under it gets it.
    pub(crate) fn lbterm_objectives_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(strip) = self.lbterm_objectives.strip else {
            return false;
        };
        // A drag that began elsewhere carries on over the panel without opening it
        let busy = self.current_mouse_capture.is_some() || self.window_drag_position.is_some() || self.dragging.is_some();
        // The projects pad at the top of the strip, and its card, first (render/projects.rs): the
        // objectives' cards close while the pointer's there
        if self.lbterm_projects_mouse(event, context, busy) {
            if self.lbterm_objectives.hover.pointer(false, Instant::now()) {
                context.lbterm_invalidate();
            }
            let due = self.lbterm_objectives.hover.deadline();
            self.lbterm_objectives_wake(due);
            return true;
        }
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut self.lbterm_objectives.pressed) {
                return true;
            }
        }
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        // The strip and the gutters either side of it, to the window's edge, but the pad's zone;
        // while the list is open, the scroll strip too, which the pointer crosses on its way to
        // the list (render/scroll_strip.rs)
        let gutter = self.lbterm_shape().gutter() as f64;
        let bridge = if self.lbterm_objectives.hover.is_open() {
            super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64
        } else {
            0.0
        };
        let on_pad = self.lbterm_objectives.projects.zone().map_or(false, |z| y >= z.y && y < z.y + z.h);
        let on_strip = !on_pad && x >= strip.x - gutter - bridge && y >= strip.y && y < strip.y + strip.h;
        let on_list = self.lbterm_objectives.list.as_ref().map_or(false, |l| l.rect.contains(x, y));
        let on_detail = self.lbterm_objectives.detail.as_ref().map_or(false, |d| d.rect.contains(x, y));
        let inside = !busy && (on_strip || on_list || on_detail);
        let now = Instant::now();
        let mut changed = self.lbterm_objectives.hover.pointer(inside, now);
        let open = self.lbterm_objectives.hover.is_open();
        // The row under the pointer, on the list or level with it on the strip; on the detail
        // card it stays the one it's for
        let row = if !open {
            None
        } else if on_detail {
            self.lbterm_objectives.row
        } else {
            self.lbterm_objectives
                .list
                .as_ref()
                .filter(|_| on_list || on_strip)
                .and_then(|l| l.rows.iter().find(|r| y >= r.rect.y && y < r.rect.y + r.rect.h))
                .map(|r| r.row)
                .or(if on_list || on_strip { None } else { self.lbterm_objectives.row })
        };
        if row != self.lbterm_objectives.row {
            self.lbterm_objectives.row = row;
            changed = true;
        }
        if changed {
            context.lbterm_invalidate();
        }
        let due = self.lbterm_objectives.hover.deadline();
        self.lbterm_objectives_wake(due);
        if !inside {
            return false;
        }
        if let WMEK::Press(press) = &event.kind {
            self.lbterm_objectives.pressed = true;
            if matches!(press, MousePress::Left) && on_list {
                if let Some(idx) = row.and_then(|r| self.tab_panel_tab_index(r)) {
                    if let Err(err) = self.activate_tab(idx as isize) {
                        log::error!("objectives: {err:#}");
                    }
                    context.lbterm_invalidate();
                }
            }
        }
        if matches!(event.kind, WMEK::Move) {
            context.set_cursor(Some(if on_list && row.is_some() { CursorIcon::Pointer } else { CursorIcon::Default }));
        }
        true
    }

    /// The pointer left the window
    pub(crate) fn lbterm_objectives_mouse_leave(&mut self) {
        let changed = self.lbterm_objectives.hover.left_window(Instant::now());
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        let due = self.lbterm_objectives.hover.deadline();
        self.lbterm_objectives_wake(due);
        self.lbterm_projects_mouse_leave();
    }

    /// Repaint at `at`: when a hover's wait is up (the objectives' or the projects card's), or a
    /// card has faded
    pub(crate) fn lbterm_objectives_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        if matches!(self.lbterm_objectives.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_objectives.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_objectives.wake == Some(at) {
                    tw.lbterm_objectives.wake = None;
                }
                let now = Instant::now();
                tw.lbterm_objectives.hover.tick(now);
                tw.lbterm_objectives.projects.tick(now);
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// What the panel shows, for the state file (lbterm/STATE.md); `None` with it off
    pub(crate) fn lbterm_state_objectives(&self) -> Option<lbterm_extras::state::Objectives> {
        use lbterm_extras::state as S;
        let at = |r: &Rect| S::Rect::round(r.x, r.y, r.w, r.h);
        let state = &self.lbterm_objectives;
        let strip = state.strip?;
        Some(S::Objectives {
            strip: at(&strip),
            marks: state
                .marks
                .iter()
                .map(|(row, mark, rect)| S::ObjectiveMark {
                    row: *row,
                    mark: mark.map(|m| m.name().to_string()),
                    rect: at(rect),
                })
                .collect(),
            list: state.list.as_ref().map(|l| S::ObjectivesList {
                rect: at(&l.rect),
                rows: l
                    .rows
                    .iter()
                    .map(|r| S::ObjectivesRow {
                        row: r.row,
                        text: r.text.clone(),
                        tally: r.tally.clone(),
                        mark: r.mark.map(|m| m.name().to_string()),
                        rect: at(&r.rect),
                    })
                    .collect(),
            }),
            detail: state.detail.as_ref().map(|d| S::ObjectivesDetail {
                row: state.row,
                rect: at(&d.rect),
                lines: d.lines.iter().map(|l| l.text.clone()).collect(),
            }),
            hovered: state.row,
            projects: self.lbterm_state_projects(),
        })
    }

    /// What of the panel is at `x`, `y`, for the state file's pointer: the projects card or pad, a
    /// card (with the list's row) or the strip
    pub(crate) fn lbterm_objectives_under(&self, x: f64, y: f64) -> Option<(&'static str, Option<usize>)> {
        if let Some(under) = self.lbterm_projects_under(x, y) {
            return Some(under);
        }
        let state = &self.lbterm_objectives;
        if state.detail.as_ref().map_or(false, |d| d.rect.contains(x, y)) {
            return Some(("objectives.detail", state.row));
        }
        if let Some(list) = state.list.as_ref().filter(|l| l.rect.contains(x, y)) {
            let row = list.rows.iter().find(|r| r.rect.contains(x, y)).map(|r| r.row);
            return Some(("objectives.list", row));
        }
        let strip = state.strip?;
        let gutter = self.lbterm_shape().gutter() as f64;
        (x >= strip.x - gutter && y >= strip.y && y < strip.y + strip.h).then_some(("objectives.strip", None))
    }

    /// The panel's waits under way at `now`: the list's rest before it opens and grace before it
    /// closes
    pub(crate) fn lbterm_objectives_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let hover = &self.lbterm_objectives.hover;
        let name = match hover.phase() {
            lbterm_panel::Phase::Opening => Some("objectives.open"),
            lbterm_panel::Phase::Closing => Some("objectives.close"),
            _ => None,
        };
        let mut waits: Vec<_> = name
            .zip(hover.deadline())
            .map(|(name, due)| lbterm_extras::state::Wait {
                name: name.to_string(),
                left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
            })
            .into_iter()
            .collect();
        waits.extend(self.lbterm_projects_waits(now));
        waits
    }
}
