//! LBTerm: what a tab's right-drag onto another shows (lbterm/LINKS.md). A line from its pad to
//! the pointer and a ring round the tab it would wait on; past the panel, level with the pointer,
//! a card of what its session says it waits for (Lachlan's E5); beside that, level with the row
//! under the pointer, a card of that tab's objectives. When what it waits for matches one of them
//! (the matcher's word, lbterm_extras::links::Links::suggestion), both blocks light green, the
//! line and the ring too, and with the compositor a light runs round each block, carried by DWM
//! (his E6; compositor.rs, `LinkLamps`). The cards sit beside the panel rather than under the
//! pointer, so the rows it's dragged over stay in sight.
//!
//! With Shift held the drag connects the two tabs instead (lbterm/LINKS.md, "Connections"): the
//! line and the ring are cyan (`palette::CONNECTED`), and the cards are both tabs' objectives, the
//! dragged one's past the panel level with the pointer and the other's beside it, with nothing lit:
//! a connection matches no wait. Shift is read live (tab_links.rs), so letting it go mid-drag
//! brings the link's look back.
//!
//! With no drag, resting the pointer on a link's line opens its card in the same place: who waits
//! on whom, where it stands in the line's colour, what it waits for and the objective of the tab
//! waited on that it matched (his E3), the panel left as it is. On a connection's line, the
//! connection's card: who could be talking and why, or who's connected, why, the note in red and
//! the last three messages they sent each other (`links::connection_card`).
//!
//! Drawn in z-layers of their own (88 to 91), which the compositor's `top` shows over the panel
//! as it does the tab menu's, and the light's template in 92 (`Target::LinkLamp`).

use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::compositor::LampPlan;
use crate::termwindow::render::tab_panel::{light_bands, link_colour, srgb};
use crate::termwindow::tab_links::RailLine;
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use config::{DimensionContext, SrgbaTuple};
use lbterm_extras::links::{self, Card, Tone};
use lbterm_extras::objectives::status_colour;
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Area};
use std::time::Instant;
use window::color::LinearRgba;

/// The line to the pointer and the ring round the tab under it
const LINE_Z: i8 = 88;
const SHADOW_Z: i8 = 89;
const CARD_Z: i8 = 90;
const TEXT_Z: i8 = 91;
/// The light's template, with the compositor: drawn once a frame, shown round each matched block
pub const LAMP_Z: i8 = 92;

/// Whether a z-layer is the drag's (the compositor's `top` shows them)
pub fn owns_zindex(zindex: i8) -> bool {
    (LINE_Z..=TEXT_Z).contains(&zindex)
}

/// Each card's width, at 96 DPI: a connection's line card wider than a link's, for its messages
const WAITS_WIDTH: f64 = 240.0;
const AIMS_WIDTH: f64 = 320.0;
const LINE_WIDTH: f64 = 300.0;
const CONNECTION_WIDTH: f64 = 360.0;
/// The light round a matched block: a lap in this many seconds, the Conductor's acting lap's
/// pace, its reach either side of where it is, and where it is this many times a lap
const LAMP_LAP: f64 = 1.4;
const LAMP_REACH: f32 = 7.0;
const LAMP_STEPS: usize = 90;
/// The light's colour: the links' green, lifted towards white so it reads as light
const LAMP: u32 = 0xC9FFDF;
/// How much of the green fills a matched block
const MATCH_FILL: f64 = 0.16;
/// The card's ring and its words over the surface, as the tasks card's (render/tasks_card.rs)
const RING_ALPHA: f32 = 0.14;
const TITLE_ALPHA: f32 = 0.92;
const TEXT_ALPHA: f32 = 0.85;
const DIM_ALPHA: f32 = 0.5;

/// What the last paint showed of a drag: for the compositor's lights and the state file
#[derive(Default)]
pub(crate) struct Shown {
    pub(crate) row: Option<usize>,
    pub(crate) over: Option<usize>,
    pub(crate) matched: bool,
    /// Shift held: let go, it connects the two tabs
    pub(crate) connect: bool,
    pub(crate) waits: Option<Card>,
    pub(crate) aims: Option<Card>,
    pub(crate) lamps: Option<LampPlan>,
}

fn over(surface: SrgbaTuple, alpha: f32) -> LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

fn area(r: Rect) -> Area {
    Area::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
}

impl TermWindow {
    /// Draw what a tab's right-drag shows this frame, if one is under way
    pub(crate) fn paint_lbterm_link_drag(&mut self) -> anyhow::Result<()> {
        self.lbterm_links.shown = Shown::default();
        let Some(drag) = self.lbterm_link_dragging() else {
            return self.paint_lbterm_link_line_card();
        };
        let Some(layout) = self.lbterm_panel.layout else {
            return Ok(());
        };
        let (tab, session, at, connect) = (drag.tab, drag.session.clone(), drag.at, drag.shift);
        let scale = self.dimensions.dpi as f64 / 96.0;
        let row = self.tab_panel_row_of(tab).filter(|&i| layout.row_visible(i));
        let target = self.lbterm_link_target();
        let rail = &self.lbterm_panel.links;
        let wait = session.as_deref().and_then(|s| rail.said.wait(s)).cloned();
        // With Shift held it connects them: a connection matches no wait
        let matched_objective = match (&wait, &target) {
            (Some(w), Some((_, on))) if !connect && w.on.as_deref() == Some(on.as_str()) => w.objective,
            _ => None,
        };
        let matched = matched_objective.is_some();
        let name = |sid: &str| rail.labels.get(sid).cloned().filter(|l| !l.is_empty()).unwrap_or_else(|| "This tab".into());
        let title = session.as_deref().map(name).unwrap_or_else(|| "This tab".into());
        let target_name = target.as_ref().map(|(_, on)| name(on));
        let colour = if connect {
            palette::CONNECTED
        } else if matched {
            palette::LINK_DONE
        } else {
            palette::LINK_WAITING
        };
        let open = self.lbterm_panel.hover.is_open();
        let gl_state = self.render_state.as_ref().unwrap();

        // The line from its pad to the pointer, and the ring round the tab it would wait on
        {
            let layer = gl_state.layer_for_zindex(LINE_Z)?;
            let mut quads = layer.quad_allocator();
            if let Some(r) = row {
                let from = layout.link_end(r);
                self.lbterm_link_line(&mut quads, from, (at.0 as f32, at.1 as f32), layout.link_width(), srgb(colour))?;
                let mark = layout.link_end_size();
                let disc = Area::new(from.0 - mark / 2.0, from.1 - mark / 2.0, mark, mark);
                self.lbterm_menu_rounded(&mut quads, rect_of(disc), (mark / 2.0) as f64, srgb(colour))?;
            }
            if let Some((i, _)) = &target {
                let ring = if open { layout.highlight(*i) } else { layout.pill(*i) };
                self.lbterm_stroke(
                    &mut quads,
                    ring.grow(layout.px(1.0)),
                    layout.pill_radius() + layout.px(1.0),
                    layout.px(2.0),
                    layout.px(lbterm_panel::layout::FEATHER),
                    |_| colour,
                )?;
            }
        }

        // The cards: what it waits for beside the panel, level with the pointer, and the objectives
        // of the tab under it beside that, level with its row. Connecting, its own objectives in
        // place of what it waits for, and the other tab's under or over it where the two don't fit
        // side by side
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (metrics.cell_size.width as f64, metrics.cell_size.height as f64);
        let (ww, wh) = (self.dimensions.pixel_width as f64, self.dimensions.pixel_height as f64);
        let margin = layout.gap() as f64;
        let left = layout.panel(open).right() as f64 + margin;
        let what = wait.as_ref().map(|w| w.what.as_str());
        let own = session
            .as_deref()
            .filter(|_| connect)
            .and_then(crate::summaries::get)
            .and_then(|s| s.objectives.clone());
        let card = |top: f64| {
            if connect {
                links::aims_card(&title, own.as_deref(), None, left, (top, f64::MAX), AIMS_WIDTH * scale, cell, scale)
            } else {
                links::waits_card(&title, what, (left, top), WAITS_WIDTH * scale, cell, scale, matched)
            }
        };
        let probe = card(0.0);
        let top = (at.1 - probe.rect.h / 2.0).clamp(margin, (wh - margin - probe.rect.h).max(margin));
        let waits = card(top);
        let aims = match (&target, &target_name) {
            (Some((i, on)), Some(name)) => {
                let objectives = crate::summaries::get(on).and_then(|s| s.objectives.clone());
                let width = AIMS_WIDTH * scale;
                let card = |x: f64, range: (f64, f64)| {
                    links::aims_card(name, objectives.as_deref(), matched_objective, x, range, width, cell, scale)
                };
                if connect && !links::fits_beside(&waits.rect, width, ww, margin) {
                    // Connecting, both cards are objectives cards, too wide to sit side by side in
                    // a narrow window: the tab's under the dragged one's, or over it
                    let h = card(waits.rect.x, (0.0, f64::MAX)).rect.h;
                    let top = links::stacked_top(&waits.rect, h, (margin, wh - margin), margin);
                    Some(card(waits.rect.x, (top, f64::MAX)))
                } else {
                    let x = (waits.rect.x + waits.rect.w + margin).min(ww - margin - width);
                    let slot = layout.slot(*i);
                    Some(card(x, (slot.y as f64, wh - margin)))
                }
            }
            _ => None,
        };
        let surface = self.palette().background;
        self.lbterm_link_card(&waits, surface)?;
        if let Some(aims) = &aims {
            self.lbterm_link_card(aims, surface)?;
        }

        // The light round each matched block, with the compositor
        let lamps = if matched && self.lbterm_composited() {
            let blocks: Vec<Rect> = waits
                .matched
                .iter()
                .chain(aims.as_ref().and_then(|a| a.matched.as_ref()))
                .cloned()
                .collect();
            self.lbterm_link_lamp_plan(&blocks, scale)?
        } else {
            None
        };

        self.lbterm_links.shown = Shown {
            row,
            over: target.map(|(i, _)| i),
            matched,
            connect,
            waits: Some(waits),
            aims,
            lamps,
        };
        // What it shows, for the state file, beside the rail's (lbterm/STATE.md)
        if crate::state_probe::on() {
            let said = self.lbterm_state_link_drag();
            if let Some(rail) = self.lbterm_panel.said.as_mut() {
                rail.link_drag = said;
            }
        }
        Ok(())
    }

    /// The card of the line the pointer rests on, if its rest is up (Lachlan's E3): beside the
    /// panel as it is, level with where the pointer came onto the line
    fn paint_lbterm_link_line_card(&mut self) -> anyhow::Result<()> {
        let Some((line, at_y)) = self.lbterm_link_line_card(Instant::now()) else {
            return Ok(());
        };
        let k = match line {
            RailLine::Link(k) => k,
            RailLine::Join(k) => return self.paint_lbterm_connection_card(k, at_y),
        };
        let Some(layout) = self.lbterm_panel.layout else {
            return Ok(());
        };
        let rail = &self.lbterm_panel.links;
        let Some(line) = rail.lines.get(k).cloned() else {
            return Ok(());
        };
        let (Some(Some(waiter)), Some(Some(on))) = (rail.sessions.get(line.waiter), rail.sessions.get(line.on)) else {
            return Ok(());
        };
        // A made link's own words, else what the session says it waits for, for a suggested one
        let link = rail.said.between(waiter, on).cloned();
        let (what, objective) = match &link {
            Some(l) => (l.what.clone(), l.objective),
            None => rail.said.wait(waiter).map(|w| (w.what.clone(), w.objective)).unwrap_or_default(),
        };
        let name = |sid: &str| rail.labels.get(sid).cloned().filter(|l| !l.is_empty()).unwrap_or_else(|| "This tab".into());
        let (waiter_name, on_name) = (name(waiter), name(on));
        let aim = objective.and_then(|id| {
            let aims = crate::summaries::get(on).and_then(|s| s.objectives.clone())?;
            aims.list.iter().find(|x| x.id == id).map(|x| (id, x.text.clone()))
        });
        let stands = links::stands(link.as_ref());
        let says = links::LineSays {
            waiter: &waiter_name,
            on: &on_name,
            look: line.look,
            stands: &stands,
            what: &what,
            objective: aim.as_ref().map(|(id, text)| (*id, text.as_str())),
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (metrics.cell_size.width as f64, metrics.cell_size.height as f64);
        let wh = self.dimensions.pixel_height as f64;
        let margin = layout.gap() as f64;
        let left = layout.panel(self.lbterm_panel.hover.is_open()).right() as f64 + margin;
        let card = links::line_card(&says, left, at_y, (margin, wh - margin), LINE_WIDTH * scale, cell, scale);
        let surface = self.palette().background;
        self.lbterm_link_card(&card, surface)?;
        // What it shows, for the state file, beside the rail's (lbterm/STATE.md)
        self.lbterm_state_line_card(&card, Some(k), None);
        Ok(())
    }

    /// The card resting on tab connection `k`'s line opens (lbterm/LINKS.md, "Connections"): a
    /// suggestion's who could be talking and why, a connection's who's in it, why, the note in red
    /// and the last three messages, each "Tab Wave to Scroll strip, 14:02: …"
    fn paint_lbterm_connection_card(&mut self, k: usize, at_y: f64) -> anyhow::Result<()> {
        let Some(layout) = self.lbterm_panel.layout else {
            return Ok(());
        };
        let rail = &self.lbterm_panel.links;
        let Some((join, c)) = rail.joins.get(k).and_then(|j| Some((j.clone(), rail.said.connections.get(j.connection)?.clone()))) else {
            return Ok(());
        };
        let name = |sid: &str| rail.labels.get(sid).cloned().filter(|l| !l.is_empty()).unwrap_or_else(|| "another tab".into());
        let members: Vec<String> = c.members.iter().map(|m| name(m)).collect();
        let messages: Vec<String> = c
            .messages
            .iter()
            .map(|m| links::message_line(&name(&m.from), &name(&m.to), &clock(m.at), &m.text))
            .collect();
        let says = links::ConnectionSays { members: &members, look: join.look, why: &c.why, note: &c.note, messages: &messages };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (metrics.cell_size.width as f64, metrics.cell_size.height as f64);
        let wh = self.dimensions.pixel_height as f64;
        let margin = layout.gap() as f64;
        let left = layout.panel(self.lbterm_panel.hover.is_open()).right() as f64 + margin;
        let card = links::connection_card(&says, left, at_y, (margin, wh - margin), CONNECTION_WIDTH * scale, cell, scale);
        let surface = self.palette().background;
        self.lbterm_link_card(&card, surface)?;
        self.lbterm_state_line_card(&card, None, Some(c.id));
        Ok(())
    }

    /// What a line's card shows, for the state file, beside the rail's (lbterm/STATE.md)
    fn lbterm_state_line_card(&mut self, card: &Card, line: Option<usize>, connection: Option<String>) {
        if !crate::state_probe::on() {
            return;
        }
        if let Some(rail) = self.lbterm_panel.said.as_mut() {
            let r = card.rect;
            rail.line_card = Some(lbterm_extras::state::LineCard {
                line,
                connection,
                rect: lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h),
                lines: card.lines.iter().map(|l| l.text.clone()).collect(),
            });
        }
    }

    /// A straight line `width` wide from `from` to `to`: one quad, its corners set (quad.rs)
    fn lbterm_link_line(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        colour: LinearRgba,
    ) -> anyhow::Result<()> {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = (dx * dx + dy * dy).sqrt();
        if length < 1.0 {
            return Ok(());
        }
        let (nx, ny) = (-dy / length * width / 2.0, dx / length * width / 2.0);
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let at = |x: f32, y: f32| [x - half_w, y - half_h];
        let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), colour)?;
        q.set_corners(
            [
                at(from.0 + nx, from.1 + ny),
                at(to.0 + nx, to.1 + ny),
                at(from.0 - nx, from.1 - ny),
                at(to.0 - nx, to.1 - ny),
            ],
            [colour; 4],
        );
        Ok(())
    }

    /// One of the drag's cards in the tasks card's look: its soft shadow, a hairline, the surface,
    /// its matched block lit green, and its words
    fn lbterm_link_card(&self, card: &Card, surface: SrgbaTuple) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        let grow = |r: Rect, by: f64, down: f64| Rect { x: r.x - by, y: r.y - by + down, w: r.w + 2.0 * by, h: r.h + 2.0 * by };
        {
            let layer = gl_state.layer_for_zindex(SHADOW_Z)?;
            let mut quads = layer.quad_allocator();
            let (reach, down) = lbterm_extras::strip::shadow_reach(scale);
            for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                self.lbterm_menu_rounded(&mut quads, grow(card.rect, by, down), radius + by, LinearRgba::with_components(0., 0., 0., *alpha))?;
            }
        }
        {
            let layer = gl_state.layer_for_zindex(CARD_Z)?;
            let mut quads = layer.quad_allocator();
            self.lbterm_menu_rounded(&mut quads, grow(card.rect, stroke, 0.), radius + stroke, over(surface, RING_ALPHA))?;
            self.lbterm_menu_rounded(&mut quads, card.rect, radius, surface.to_linear())?;
            if let Some(m) = card.matched {
                let (r, g, b, _) = surface.to_srgb_u8();
                let under = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
                let block_radius = 6.0 * scale;
                self.lbterm_menu_rounded(&mut quads, m, block_radius, srgb(palette::blend(under, palette::LINK_DONE, MATCH_FILL)))?;
                self.lbterm_stroke(
                    &mut quads,
                    area(m),
                    block_radius as f32,
                    shape.hairline().max(1.0) * 1.5,
                    (scale as f32).max(1.0),
                    |_| palette::LINK_DONE,
                )?;
            }
        }
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let right = card.rect.x + card.rect.w;
        let (r, g, b, _) = surface.to_srgb_u8();
        let under = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        for line in &card.lines {
            let colour = match line.tone {
                Tone::Title => over(surface, TITLE_ALPHA),
                Tone::Text => over(surface, TEXT_ALPHA),
                Tone::Heading | Tone::Quiet => over(surface, DIM_ALPHA),
                Tone::Status(s) => srgb(status_colour(s)),
                Tone::Look(look) => srgb(link_colour(look, under)),
            };
            let element = Element::new(&font, ElementContent::Text(line.text.clone())).colors(ElementColors {
                border: BorderColor::default(),
                bg: LinearRgba::TRANSPARENT.into(),
                text: colour.into(),
            });
            let computed = self.compute_element(
                &LayoutContext {
                    width: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: metrics.cell_size.width as f32,
                    },
                    height: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_height as f32,
                        pixel_cell: metrics.cell_size.height as f32,
                    },
                    bounds: euclid::rect(line.x as f32, line.y as f32, (right - line.x).max(0.0) as f32, metrics.cell_size.height as f32),
                    metrics: &metrics,
                    gl_state,
                    zindex: TEXT_Z,
                },
                &element,
            )?;
            self.render_element(&computed, gl_state, None)?;
        }
        Ok(())
    }

    /// The light's template, drawn this frame, and where it goes round each of `blocks`
    fn lbterm_link_lamp_plan(&self, blocks: &[Rect], scale: f64) -> anyhow::Result<Option<LampPlan>> {
        let Some(first) = blocks.first() else {
            return Ok(None);
        };
        let reach = (LAMP_REACH * scale as f32).round();
        let (cx, cy) = (first.x.round() as f32, first.y.round() as f32);
        let template = PixelRect::new(
            (cx - reach) as i32,
            (cy - reach) as i32,
            (2.0 * reach + 1.0) as u32,
            (2.0 * reach + 1.0) as u32,
        );
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(LAMP_Z)?;
        let mut quads = layer.quad_allocator();
        let bands = light_bands(LAMP, reach, 8, |d| {
            let t = (1.0 - d / reach as f64).clamp(0.0, 1.0);
            0.95 * t * t
        });
        self.tab_panel_disc(&mut quads, (cx, cy), &bands)?;
        let radius = (6.0 * scale) as f32;
        let paths = blocks
            .iter()
            .map(|b| {
                (0..LAMP_STEPS)
                    .map(|k| {
                        let (x, y) = lbterm_panel::layout::outline_point(area(*b), radius, k as f64 / LAMP_STEPS as f64);
                        ((x - reach).round(), (y - reach).round())
                    })
                    .collect()
            })
            .collect();
        Ok(Some(LampPlan { template, lap: LAMP_LAP, paths }))
    }

    /// The lights round a drag's matched blocks, for the compositor (render/composited.rs)
    pub(crate) fn lbterm_link_lamps(&self) -> Option<LampPlan> {
        self.lbterm_links.shown.lamps.clone()
    }

    /// What a drag shows, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_state_link_drag(&self) -> Option<lbterm_extras::state::LinkDrag> {
        use lbterm_extras::state as S;
        let shown = &self.lbterm_links.shown;
        let row = shown.row?;
        let at = |r: &Rect| S::Rect::round(r.x, r.y, r.w, r.h);
        Some(S::LinkDrag {
            row,
            over: shown.over,
            matched: shown.matched,
            connect: shown.connect,
            waits: shown.waits.as_ref().map(|c| at(&c.rect)),
            aims: shown.aims.as_ref().map(|c| at(&c.rect)),
            waits_match: shown.waits.as_ref().and_then(|c| c.matched.as_ref()).map(at),
            aims_match: shown.aims.as_ref().and_then(|c| c.matched.as_ref()).map(at),
            waits_words: words(shown.waits.as_ref()),
            aims_words: words(shown.aims.as_ref()),
        })
    }
}

/// A card's lines of words, for the state file
fn words(card: Option<&Card>) -> Vec<String> {
    card.map(|c| c.lines.iter().map(|l| l.text.clone()).collect()).unwrap_or_default()
}

fn rect_of(a: Area) -> Rect {
    Rect { x: a.x as f64, y: a.y as f64, w: a.w as f64, h: a.h as f64 }
}

/// When a connection's message was sent, as the card says it: "14:02", local time, or nothing
/// when the file doesn't say
fn clock(at_ms: u64) -> String {
    use chrono::TimeZone;
    if at_ms == 0 {
        return String::new();
    }
    chrono::Local
        .timestamp_millis_opt(at_ms as i64)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}
