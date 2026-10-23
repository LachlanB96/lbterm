//! LBTerm: the Conductor on the tab rail (`config.lbterm_conductor`; lbterm/DESIGN.md, "The tab
//! panel"; lbterm/TAB-PANEL.md), whose logic is lbterm_extras::conductor and lbterm_panel's.
//!
//! The rail's top slot is the Conductor's pad, a tab pad's shape: its glyph where a tab's number
//! goes, its mode under it in the mode's colour, a mark on its corner while its remote control is
//! on, and round it a grey outline while it only reads, or the spectrum's colours running round
//! while it may act on its own. A click on it drops its menu, the items its state offers, the
//! mode it's in ticked. Tabs it's thinking about get the iridescent outline, and those it's acting
//! on a light running round it too, as its hub says (crate::conductor_hub).
//!
//! With the compositor the outlines are drawn once as templates and shown on each pad by visuals
//! of their own, whose hues DWM turns and whose light DWM carries round (compositor.rs, `Ring`).
//! Without it they're drawn still, in the panel's body.
//!
//! The menu is drawn in z-layers 66 to 68, beside the tab menu's and the prompt list's in the
//! compositor's `top` visual, over the tab panel.

use super::tab_panel::{light_bands, srgb};
use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::{TermWindow, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::DimensionContext;
use lbterm_extras::conductor::{self, Level, Menu, Said};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::words::HeadSays;
use lbterm_panel::{palette, Area, Layout};
use std::time::Instant;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// The menu's light, card and words: in the tab menu's range, so the compositor's `top`
const GLOW_Z: i8 = 66;
const CARD_Z: i8 = 67;
const TEXT_Z: i8 = 68;

/// The outlines' two looks (lbterm/DESIGN.md, picked 2026-10-21)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingStyle {
    /// Round the Conductor's pad while it may act on its own: every hue, strong
    Spectrum,
    /// Round a tab's pad while the Conductor is on it: every hue, pale
    Iridescent,
}

/// The hues go once round in this long; the light goes round in this long while the Conductor
/// acts (the turning arc's pace), and the glint while it thinks
pub const HUE_PERIOD: f64 = 6.0;
pub const ACTING_LAP: f64 = 1.4;
pub const GLINT_LAP: f64 = 3.0;
/// How bright the light is acting, and the glint thinking
pub const ACTING_LIGHT: f32 = 1.0;
pub const GLINT_LIGHT: f32 = 0.45;
/// The outline's stroke, its soft edges, and the glow past it, at 96 DPI
const RING_STROKE: f32 = 1.5;
const RING_GLOW: f32 = 5.0;
/// The light's reach either side of where it is
const LIGHT_REACH: f32 = 7.0;

/// The turning arc on an item that waits for its answer: its template's z-layer, its visual
/// over the menu in the compositor's `top` (compositor.rs, `MenuSpinner`), and drawn straight
/// over the menu, still, without the compositor
pub const MENU_SPIN_Z: i8 = 76;

/// The menu while it's open
pub struct OpenMenu {
    /// The items offered when it opened, as indexes into the config's `menu`
    items: Vec<usize>,
    /// The item under the pointer, or where the arrow keys put it, as a place in `items`
    focus: Option<usize>,
    /// Where it was clicked open
    at: (f64, f64),
    /// An item picked that waits for its answer
    pending: Option<Pending>,
}

/// An item waiting for its answer: the menu stays open, its arc turning, until the state file
/// says something new (lbterm_extras::conductor::answered)
struct Pending {
    /// Its place in `items`
    pos: usize,
    /// What the file said when it was picked
    said: Said,
    since: Instant,
}

#[derive(Default)]
pub struct ConductorState {
    menu: Option<OpenMenu>,
    /// Where the turning arc is this frame, for the compositor
    spinner: Option<PixelRect>,
    /// When a repaint is already on its way, to give up waiting
    wake: Option<Instant>,
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

fn srgb_alpha(rgb: u32, alpha: f32) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// How far past a pill its outline's template reaches, glow included
pub fn ring_margin(layout: &Layout) -> f32 {
    layout.px(RING_STROKE) + layout.px(RING_GLOW) + layout.px(1.0)
}

/// The light's template: a square round where it is
pub fn light_reach(layout: &Layout) -> f32 {
    layout.px(LIGHT_REACH)
}

/// Where the light goes round a pill: its outline's middle line
pub fn light_path(layout: &Layout, pill: Area) -> (Area, f32) {
    (pill, layout.pill_radius())
}

impl TermWindow {
    /// What the Conductor's state file says now: nothing without the Conductor in the config;
    /// no file reads as off
    pub(crate) fn lbterm_conductor_said(&self) -> Option<Said> {
        let conductor = self.config.lbterm_conductor.as_ref()?;
        Some(crate::state_files::said(&conductor.state_file).unwrap_or_default())
    }

    /// What its slot on the rail says, and how far it may act on its own
    pub(crate) fn lbterm_conductor_head(&self) -> Option<(HeadSays, Level)> {
        let conductor = self.config.lbterm_conductor.as_ref()?;
        let said = self.lbterm_conductor_said()?;
        let pad = conductor::pad(&said);
        let (glyph_colour, label_colour) = match pad.level {
            Level::Off => (palette::CONDUCTOR_OFF, palette::CONDUCTOR_OFF),
            Level::ReadOnly => (palette::CONDUCTOR_ON, palette::MODE_READ),
            Level::AcceptMost => (palette::CONDUCTOR_ON, palette::MODE_MOST),
            Level::AcceptAll => (palette::CONDUCTOR_ON, palette::MODE_ALL),
        };
        let says = HeadSays {
            glyph: conductor.glyph.clone(),
            glyph_colour,
            label: pad.label,
            label_colour,
            mark: pad.remote.then(|| conductor.remote_glyph.clone()),
            name: said.get("label").unwrap_or("Conductor").to_string(),
            says: said.tooltip().unwrap_or("").to_string(),
        };
        Some((says, pad.level))
    }

    /// The Conductor's pad on the rail, under its words: the pad, the outline round its pill (the
    /// spectrum's while it may act, or while it only reads and the pointer is on it, still when
    /// there's no compositor to move it; grey while it only reads), its pill filled as the open
    /// tab's while its tab is the open one (`open_fill`), and the hairline between it and the tabs
    pub(crate) fn tab_panel_conductor(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        layout: &Layout,
        level: Level,
        hovered: bool,
        open_fill: Option<u32>,
    ) -> anyhow::Result<()> {
        let pill = layout.head_pill();
        if let Some(fill) = open_fill {
            self.tab_panel_fill(quads, pill, layout.pill_radius(), srgb(fill))?;
        }
        if conductor::spectrum(level, hovered) {
            if !self.lbterm_composited() {
                self.tab_panel_rainbow(quads, layout, pill, RingStyle::Spectrum)?;
            }
        } else if level == Level::ReadOnly {
            self.lbterm_stroke(
                quads,
                pill,
                layout.pill_radius(),
                layout.stroke(),
                layout.px(lbterm_panel::layout::FEATHER),
                |_| palette::OUTLINE,
            )?;
        }
        let pad = layout.head_pad();
        self.tab_panel_fill(quads, pad, layout.px(7.0), srgb(palette::CONDUCTOR_PAD))?;
        let rule = layout.head_rule();
        self.tab_panel_fill(quads, rule, 0.0, srgb_alpha(palette::OUTLINE, 0.6))
    }

    /// A rounded rectangle filled with one colour
    fn tab_panel_fill(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Area,
        radius: f32,
        colour: LinearRgba,
    ) -> anyhow::Result<()> {
        if area.w <= 0.0 || area.h <= 0.0 {
            return Ok(());
        }
        self.lbterm_menu_rounded(
            quads,
            Rect {
                x: area.x as f64,
                y: area.y as f64,
                w: area.w as f64,
                h: area.h as f64,
            },
            radius as f64,
            colour,
        )
    }

    /// An outline of every hue round `pill`, `style`'s, its hue running from 0 to 1 once round
    /// from the top of its top-left corner, with a glow past it: stepped round a few pixels at a
    /// time, each step's colour its own, so DWM's hue rotation runs the colours round
    pub(crate) fn tab_panel_rainbow(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        layout: &Layout,
        pill: Area,
        style: RingStyle,
    ) -> anyhow::Result<()> {
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let radius = layout.pill_radius();
        let perimeter = 2.0 * (pill.w + pill.h);
        let steps = ((perimeter / layout.px(2.0)).ceil() as usize).max(24);
        let point = |k: usize| lbterm_panel::layout::outline_point(pill, radius, k as f64 / steps as f64);
        let hue = |k: usize| match style {
            RingStyle::Spectrum => palette::spectrum(k as f64 / steps as f64),
            RingStyle::Iridescent => palette::iridescent(k as f64 / steps as f64),
        };
        // Out from the outline: the stroke, soft either side, and the glow past it
        let stroke = layout.px(RING_STROKE);
        let feather = layout.px(1.0);
        let glow = layout.px(RING_GLOW);
        let half = stroke / 2.0;
        let bands = [
            (-half - feather, 0.0, -half, 1.0),
            (-half, 1.0, half, 1.0),
            (half, 1.0, half + feather, 0.35),
            (half + feather, 0.35, half + feather + glow, 0.0),
        ];
        let normal = |k: usize| {
            let (a, b) = (point((k + steps - 1) % steps), point((k + 1) % steps));
            let (tx, ty) = (b.0 - a.0, b.1 - a.1);
            let len = (tx * tx + ty * ty).sqrt().max(1e-3);
            // Round anticlockwise on screen: out is to the tangent's left
            (-ty / len, tx / len)
        };
        for k in 0..steps {
            let j = (k + 1) % steps;
            let (p0, p1) = (point(k), point(j));
            let (n0, n1) = (normal(k), normal(j));
            let (c0, c1) = (hue(k), hue(j));
            for &(near, a_near, far, a_far) in &bands {
                let at = |p: (f32, f32), n: (f32, f32), d: f32| [p.0 + n.0 * d - half_w, p.1 + n.1 * d - half_h];
                let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), srgb_alpha(c0, a_near))?;
                q.set_corners(
                    [at(p0, n0, near), at(p1, n1, near), at(p0, n0, far), at(p1, n1, far)],
                    [
                        srgb_alpha(c0, a_near),
                        srgb_alpha(c1, a_near),
                        srgb_alpha(c0, a_far),
                        srgb_alpha(c1, a_far),
                    ],
                );
            }
        }
        Ok(())
    }

    /// The light that runs round a pad's outline: a small white disc, soft to its edge, at
    /// `centre`
    pub(crate) fn tab_panel_conductor_light(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        layout: &Layout,
        centre: (f32, f32),
    ) -> anyhow::Result<()> {
        let reach = light_reach(layout);
        let bands = light_bands(0xFFFFFF, reach, 8, |d| {
            let t = (1.0 - d / reach as f64).clamp(0.0, 1.0);
            0.95 * t * t
        });
        self.tab_panel_disc(quads, centre, &bands)
    }

    /// The waiting item's arc in `area`: three quarters of a ring in the menu's white, soft at
    /// its edges, as the rail's turning arc is drawn
    fn lbterm_conductor_arc(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: PixelRect,
        scale: f64,
    ) -> anyhow::Result<()> {
        use std::f32::consts::{FRAC_PI_2, TAU};
        const SEGMENTS: usize = 36;
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let centre = (
            area.x as f32 + area.w as f32 / 2.0,
            area.y as f32 + area.h as f32 / 2.0,
        );
        let at = |radius: f32, angle: f32| {
            [centre.0 + radius * angle.cos() - half_w, centre.1 + radius * angle.sin() - half_h]
        };
        let px = |v: f64| (v * scale).round().max(1.0) as f32;
        let feather = px(1.0);
        let width = px(1.5);
        let ring = area.w as f32 / 2.0 - feather - px(0.5);
        let bands = [
            (ring - width - feather, 0.0, ring - width, 1.0),
            (ring - width, 1.0, ring, 1.0),
            (ring, 1.0, ring + feather, 0.0),
        ];
        let steps = (SEGMENTS as f32 * 0.75).round() as usize;
        for (near, a_near, far, a_far) in bands {
            for k in 0..steps {
                // From the top, clockwise on screen
                let a0 = -FRAC_PI_2 + TAU * k as f32 / SEGMENTS as f32;
                let a1 = -FRAC_PI_2 + TAU * (k + 1) as f32 / SEGMENTS as f32;
                let (c_near, c_far) = (srgb_alpha(palette::TITLE, a_near), srgb_alpha(palette::TITLE, a_far));
                let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), c_near)?;
                q.set_corners(
                    [at(near, a0), at(near, a1), at(far, a0), at(far, a1)],
                    [c_near, c_near, c_far, c_far],
                );
            }
        }
        Ok(())
    }

    pub(crate) fn lbterm_conductor_menu_is_open(&self) -> bool {
        self.lbterm_conductor.menu.is_some()
    }

    fn lbterm_conductor_menu_close(&mut self) {
        if self.lbterm_conductor.menu.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// A click on the Conductor's pad at `at`: drop its menu there, the items its state offers
    pub(crate) fn lbterm_conductor_click(&mut self, at: (f64, f64)) {
        let (Some(conductor), Some(said)) = (self.config.lbterm_conductor.as_ref(), self.lbterm_conductor_said())
        else {
            return;
        };
        let items: Vec<usize> = conductor
            .menu
            .iter()
            .enumerate()
            .filter(|(_, item)| conductor::offered(&item.when, &said))
            .map(|(n, _)| n)
            .collect();
        if items.is_empty() {
            log::info!("conductor: nothing offered in state {:?}", said.state());
            return;
        }
        self.lbterm_conductor.menu = Some(OpenMenu {
            items,
            focus: None,
            at,
            pending: None,
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// The open Conductor menu's card, for the tab panel behind it (render/tab_panel.rs)
    pub(crate) fn lbterm_conductor_menu_card(&self) -> Option<Rect> {
        let (menu, _) = self.lbterm_conductor_menu_layout().ok()??;
        Some(menu.card)
    }

    /// The open menu's items' words and ticks, and where it goes
    fn lbterm_conductor_menu_layout(&self) -> anyhow::Result<Option<(Menu, Vec<(String, bool)>)>> {
        let (Some(open), Some(conductor)) = (self.lbterm_conductor.menu.as_ref(), self.config.lbterm_conductor.as_ref())
        else {
            return Ok(None);
        };
        let said = self.lbterm_conductor_said().unwrap_or_default();
        let items: Vec<(String, bool)> = open
            .items
            .iter()
            .filter_map(|&n| conductor.menu.get(n))
            .map(|item| (item.label.clone(), conductor::ticked(&item.checked, &said)))
            .collect();
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let scale = self.dimensions.dpi as f64 / 96.0;
        let cell_w = font.metrics().cell_width.get();
        let row_h = (metrics.cell_size.height as f64 * 1.6).round();
        let columns = items.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        let window = (
            self.dimensions.pixel_width as f64,
            self.dimensions.pixel_height as f64,
        );
        Ok(conductor::menu(items.len(), columns, open.at, window, (row_h, cell_w), scale)
            .map(|menu| (menu, items)))
    }

    /// Run the open menu's item at `pos`. One that waits for its answer keeps the menu open, its
    /// arc turning, until the state file changes; any other closes it.
    fn lbterm_conductor_menu_pick(&mut self, pos: usize) {
        let Some(&n) = self.lbterm_conductor.menu.as_ref().and_then(|o| o.items.get(pos)) else {
            return;
        };
        let item = self
            .config
            .lbterm_conductor
            .as_ref()
            .and_then(|c| c.menu.get(n))
            .map(|item| (item.label.clone(), item.action.clone(), item.wait, item.checked.clone()));
        let Some((label, action, wait, checked)) = item else {
            return;
        };
        let said = self.lbterm_conductor_said().unwrap_or_default();
        if conductor::waits(wait, &checked, &said) {
            let since = Instant::now();
            if let Some(open) = self.lbterm_conductor.menu.as_mut() {
                open.pending = Some(Pending { pos, said, since });
                open.focus = None;
            }
            self.lbterm_conductor_wake(since + conductor::PENDING_AT_MOST);
        } else {
            self.lbterm_conductor.menu = None;
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        if let Err(err) = self.perform_key_assignment(&pane, &action) {
            log::error!("conductor menu: {label}: {err:#}");
        }
    }

    /// Repaint at `at`: when a waiting item gives up
    fn lbterm_conductor_wake(&mut self, at: Instant) {
        if matches!(self.lbterm_conductor.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_conductor.wake = Some(at);
        promise::spawn::spawn(async move {
            smol::Timer::at(at).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_conductor.wake == Some(at) {
                    tw.lbterm_conductor.wake = None;
                }
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// Where the turning arc is this frame, for the compositor: only with it, while an item waits
    pub(crate) fn lbterm_conductor_spinner(&self) -> Option<PixelRect> {
        self.lbterm_conductor.spinner
    }

    /// Whether the open menu's item waiting for its answer has it: then the menu closes
    fn lbterm_conductor_answered(&self) -> bool {
        let Some(pending) = self.lbterm_conductor.menu.as_ref().and_then(|o| o.pending.as_ref()) else {
            return false;
        };
        let now = self.lbterm_conductor_said().unwrap_or_default();
        conductor::answered(&pending.said, &now, pending.since.elapsed())
    }

    /// While the menu's open it takes every mouse event: true when it did
    pub(crate) fn lbterm_conductor_menu_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if self.lbterm_conductor.menu.is_none() {
            return false;
        }
        let Ok(Some((layout, _))) = self.lbterm_conductor_menu_layout() else {
            self.lbterm_conductor_menu_close();
            return false;
        };
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let waiting = self
            .lbterm_conductor
            .menu
            .as_ref()
            .map_or(false, |o| o.pending.is_some());
        // While an item waits, the others can't be picked: the card holds the pointer, and a
        // click away still closes it
        let hit = layout.hit(x, y).filter(|_| !waiting);
        match event.kind {
            WMEK::Move => {
                if let Some(open) = self.lbterm_conductor.menu.as_mut() {
                    if open.focus != hit && (hit.is_some() || layout.card.contains(x, y)) {
                        open.focus = hit;
                        context.invalidate();
                    }
                }
                context.set_cursor(Some(if hit.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                }));
            }
            WMEK::Press(MousePress::Left) => match hit {
                Some(pos) => self.lbterm_conductor_menu_pick(pos),
                None if layout.card.contains(x, y) => {}
                // The pad itself among them: a second click closes it
                None => self.lbterm_conductor_menu_close(),
            },
            WMEK::Press(_) if !layout.card.contains(x, y) => self.lbterm_conductor_menu_close(),
            _ => {}
        }
        true
    }

    /// While the menu's open it takes every key: up and down it, Enter to run the item, Esc to
    /// close it. True when it did.
    pub(crate) fn lbterm_conductor_menu_key(&mut self, key: &KeyCode, _mods: KeyModifiers) -> bool {
        let Some(open) = self.lbterm_conductor.menu.as_ref() else {
            return false;
        };
        let (focus, count) = (open.focus, open.items.len());
        // While an item waits, only Esc does anything: it stops waiting
        if open.pending.is_some() && *key != KeyCode::Escape {
            return true;
        }
        match key {
            KeyCode::Escape => self.lbterm_conductor_menu_close(),
            KeyCode::Enter => {
                if let Some(pos) = focus {
                    self.lbterm_conductor_menu_pick(pos);
                }
            }
            KeyCode::UpArrow | KeyCode::DownArrow => {
                if let Some(open) = self.lbterm_conductor.menu.as_mut() {
                    open.focus = conductor::step(focus, count, *key == KeyCode::DownArrow);
                }
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
            }
            _ => {}
        }
        true
    }

    /// Draw the open menu: the tab menu's card and light, a row an item, a tick by the one in
    /// force
    pub(crate) fn paint_lbterm_conductor_menu(&mut self) -> anyhow::Result<()> {
        self.lbterm_conductor.spinner = None;
        // An item waiting for its answer has it: the menu's done
        if self.lbterm_conductor_answered() {
            self.lbterm_conductor.menu = None;
            return Ok(());
        }
        let Some((layout, items)) = self.lbterm_conductor_menu_layout()? else {
            return Ok(());
        };
        let focus = self.lbterm_conductor.menu.as_ref().and_then(|o| o.focus);
        let pending = self
            .lbterm_conductor
            .menu
            .as_ref()
            .and_then(|o| o.pending.as_ref().map(|p| p.pos));
        let card_rgb = {
            let (r, g, b, _) = self.palette().background.to_srgb_u8();
            palette::blend(((r as u32) << 16) | ((g as u32) << 8) | b as u32, palette::CHIP, 0.55)
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_h = metrics.cell_size.height as f64;
        let gl_state = self.render_state.as_ref().unwrap();

        let mut words = vec![];
        let mut place = |text: &str, x: f64, y: f64, w: f64, colour: u32| -> anyhow::Result<()> {
            let element = Element::new(&font, ElementContent::Text(text.to_string())).colors(ElementColors {
                border: BorderColor::default(),
                bg: LinearRgba::TRANSPARENT.into(),
                text: srgb(colour).into(),
            });
            words.push(self.compute_element(
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
                    bounds: euclid::rect(x as f32, y as f32, w as f32, line_h as f32),
                    metrics: &metrics,
                    gl_state,
                    zindex: TEXT_Z,
                },
                &element,
            )?);
            Ok(())
        };
        for (i, (row, (label, ticked))) in layout.rows.iter().zip(&items).enumerate() {
            let y = row.y + (row.h - line_h) / 2.0;
            // While one waits, the others sit back: they can't be picked
            let colour = match pending {
                Some(p) if p != i => palette::PROJECT,
                _ => palette::TITLE,
            };
            if *ticked && pending != Some(i) {
                place("\u{2713}", layout.tick_x, y, layout.text_x - layout.tick_x, colour)?;
            }
            place(label, layout.text_x, y, row.x + row.w - layout.text_x, colour)?;
        }

        // The waiting item's arc, where its tick would go: drawn into a template the
        // compositor turns over the menu, and without the compositor straight over the menu, still
        if let Some(row) = pending.and_then(|p| layout.rows.get(p)) {
            let side = (row.h * 0.6).min(layout.text_x - layout.tick_x).round().max(4.0);
            let cx = ((layout.tick_x + layout.text_x) / 2.0 - 0.25 * (layout.text_x - layout.tick_x)).round();
            let cy = (row.y + row.h / 2.0).round();
            let half = (side / 2.0).round();
            let area = PixelRect::new(
                (cx - half) as i32,
                (cy - half) as i32,
                (2.0 * half) as u32,
                (2.0 * half) as u32,
            );
            let layer = gl_state.layer_for_zindex(MENU_SPIN_Z)?;
            let mut quads = layer.quad_allocator();
            self.lbterm_conductor_arc(&mut quads, area, scale)?;
            if self.lbterm_composited() {
                self.lbterm_conductor.spinner = Some(area);
            }
        }

        // The light round it, fading out, as the tab menu's
        {
            let layer = gl_state.layer_for_zindex(GLOW_Z)?;
            let mut quads = layer.quad_allocator();
            for (i, alpha) in [0.16f32, 0.09, 0.05, 0.025].iter().enumerate() {
                let by = (2.0 + 3.0 * i as f64) * scale;
                self.lbterm_menu_rounded(&mut quads, grow(layout.card, by), radius + by, srgb_alpha(palette::GLOW, *alpha))?;
            }
        }
        let layer = gl_state.layer_for_zindex(CARD_Z)?;
        let mut quads = layer.quad_allocator();
        self.lbterm_menu_rounded(&mut quads, grow(layout.card, stroke), radius + stroke, srgb(palette::RING_MIDDLE))?;
        self.lbterm_menu_rounded(&mut quads, layout.card, radius, srgb(card_rgb))?;
        if let Some(row) = focus.and_then(|pos| layout.rows.get(pos)) {
            let hover = srgb(palette::blend(card_rgb, palette::HOVER, palette::HOVER_ALPHA * 1.6));
            self.lbterm_menu_rounded(&mut quads, *row, 6.0 * scale, hover)?;
        }
        drop(quads);
        for computed in &words {
            self.render_element(computed, gl_state, None)?;
        }
        Ok(())
    }
}

/// LBTerm: the Conductor's menu, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// The menu while it's open: its items and ticks, the one picked by the pointer or the keys,
    /// and the one waiting for its answer
    pub(crate) fn lbterm_state_conductor_menu(&self) -> Option<lbterm_extras::state::Menu> {
        let open = self.lbterm_conductor.menu.as_ref()?;
        let (menu, items) = self.lbterm_conductor_menu_layout().ok()??;
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        Some(lbterm_extras::state::Menu {
            kind: "conductor".to_string(),
            tab: None,
            rect: Some(rect(&menu.card)),
            items: items
                .into_iter()
                .enumerate()
                .map(|(i, (label, checked))| lbterm_extras::state::MenuItem {
                    label,
                    checked,
                    rect: menu.rows.get(i).map(|r| rect(r)),
                })
                .collect(),
            focus: open.focus,
            renaming: None,
            waiting: open.pending.as_ref().map(|p| p.pos),
        })
    }

    /// What of the open menu is at `x`, `y`: `Some(Some(item))` on an item, `Some(None)` on the
    /// card elsewhere, `None` off it
    pub(crate) fn lbterm_conductor_menu_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        let (menu, _) = self.lbterm_conductor_menu_layout().ok()??;
        menu.card.contains(x, y).then(|| menu.hit(x, y))
    }

    /// An item waiting for its answer, and how long it waits at most
    pub(crate) fn lbterm_conductor_wait(&self, now: Instant) -> Option<lbterm_extras::state::Wait> {
        let pending = self.lbterm_conductor.menu.as_ref()?.pending.as_ref()?;
        let until = pending.since + conductor::PENDING_AT_MOST;
        Some(lbterm_extras::state::Wait {
            name: "conductor.answer".to_string(),
            left_ms: lbterm_extras::state::millis(until.saturating_duration_since(now)),
        })
    }

    /// Whether a waiting item's arc is turning this frame
    pub(crate) fn lbterm_conductor_spinning(&self) -> bool {
        self.lbterm_conductor.spinner.is_some()
    }
}
