//! LBTerm: brief light over the panes that answers an action (lbterm/DESIGN.md, "Other
//! moments"), which the compositor plays in DWM (compositor.rs, `Light`): a right-click's ripple
//! in a pane, where Terminal's wezterm.lua swallows the click so Claude Code doesn't paste; a
//! copied stretch of text lighting up (a drag-select let go, Ctrl+Shift+C, a quick-select
//! pick); a link lighting up as it's opened. And the readout of the active pane's size in
//! cells, in the middle of that pane while the frame is dragged or the font changes, there at
//! once and fading out on `pane_exit` once it's still. Where each goes is
//! lbterm_extras::lights'. Without the compositor, or with motion off, there's none of it.

use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::compositor::Light;
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::WindowOps;
use config::{DimensionContext, SrgbaTuple};
use lbterm_extras::lights::{self, Cells, Readout};
use lbterm_extras::strip;
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use mux::pane::{Pane, PaneId, WithPaneLines};
use mux::Mux;
use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::hyperlink::Hyperlink;
use termwiz::surface::Line;
use wezterm_term::StableRowIndex;
use window::color::LinearRgba;

/// The readout's shadow, card and words: with the compositor, its visual's alone
/// (`Target::Readout`)
const SHADOW_Z: i8 = 33;
const CARD_Z: i8 = 34;
const TEXT_Z: i8 = 35;

/// Whether a z-layer is the readout's
pub fn owns_zindex(zindex: i8) -> bool {
    (SHADOW_Z..=TEXT_Z).contains(&zindex)
}

/// The readout's words and hairline: white over the card, as the eye sees it (the strip
/// card's)
const TEXT_ALPHA: f32 = 0.85;
const RING_ALPHA: f32 = 0.14;
/// A window's first moments: its first sizes aren't a resize to show
const SETTLE: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct LightsState {
    /// Lights asked for since the last frame
    pending: RefCell<Vec<Light>>,
    readout: Readout,
    /// The readout as last drawn, which it shows while it fades: its card and words
    last: Option<(Rect, String)>,
    /// Where the readout's visual goes this frame, and whether it shows (or fades)
    visual: Option<(PixelRect, bool)>,
    /// When a repaint is already on its way
    wake: Option<Instant>,
}

fn over(surface: SrgbaTuple, alpha: f32) -> LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

impl crate::TermWindow {
    /// A brief light, played from the next frame
    pub(crate) fn lbterm_light(&self, light: Light) {
        if !self.lbterm_composited() {
            return;
        }
        self.lbterm_lights.pending.borrow_mut().push(light);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// This frame's lights, for the compositor
    pub(crate) fn lbterm_lights_take(&self) -> Vec<Light> {
        std::mem::take(&mut *self.lbterm_lights.pending.borrow_mut())
    }

    /// A click's ripple from `(x, y)` in the window
    pub(crate) fn lbterm_light_press(&self, x: f64, y: f64) {
        let (reach, start) = lights::press_reach(self.dimensions.dpi as f64 / 96.0);
        self.lbterm_light(Light::Press { x, y, reach, start });
    }

    /// Light up stretches of `pane_id`'s text: each a row of its scrollback and the columns
    /// on it. Rows off the screen are left out.
    pub(crate) fn lbterm_light_text(&self, pane_id: PaneId, spans: &[(StableRowIndex, Range<usize>)]) {
        if spans.is_empty() || !self.lbterm_composited() {
            return;
        }
        let Some((cells, top)) = self.lbterm_cells(pane_id) else {
            return;
        };
        let on_screen: Vec<(isize, Range<usize>)> = spans
            .iter()
            .map(|(row, cols)| ((*row - top) as isize, cols.clone()))
            .collect();
        let rects: Vec<PixelRect> = lights::text_rects(cells, &on_screen)
            .iter()
            .map(|r| PixelRect::new(r.x as i32, r.y as i32, r.w as u32, r.h as u32))
            .collect();
        if rects.is_empty() {
            return;
        }
        let radius = lights::text_radius(self.dimensions.dpi as f64 / 96.0);
        self.lbterm_light(Light::Text { rects, radius });
    }

    /// Light up `pane`'s selection as it's copied: a drag let go, Ctrl+Shift+C, a quick-select
    /// pick
    pub(crate) fn lbterm_light_selection(&self, pane: &Arc<dyn Pane>) {
        let (range, rectangular) = {
            let selection = self.selection(pane.pane_id());
            match selection.range {
                Some(range) => (range.normalize(), selection.rectangular),
                None => return,
            }
        };
        let spans: Vec<(StableRowIndex, Range<usize>)> = range
            .rows()
            .map(|row| (row, range.cols_for_row(row, rectangular)))
            .collect();
        self.lbterm_light_text(pane.pane_id(), &spans);
    }

    /// Light up `link` where it shows in `pane`, as it's opened
    pub(crate) fn lbterm_light_link(&self, pane: &Arc<dyn Pane>, link: &Arc<Hyperlink>) {
        struct FindLink<'a> {
            link: &'a Arc<Hyperlink>,
            spans: Vec<(StableRowIndex, Range<usize>)>,
        }
        impl WithPaneLines for FindLink<'_> {
            fn with_lines_mut(&mut self, stable_top: StableRowIndex, lines: &mut [&mut Line]) {
                for (i, line) in lines.iter().enumerate() {
                    let row = stable_top + i as StableRowIndex;
                    let mut run: Option<Range<usize>> = None;
                    for cell in line.visible_cells() {
                        let on = cell
                            .attrs()
                            .hyperlink()
                            .map_or(false, |l| Arc::ptr_eq(l, self.link));
                        let (start, end) = (cell.cell_index(), cell.cell_index() + cell.width());
                        match (&mut run, on) {
                            (Some(r), true) if r.end == start => r.end = end,
                            (_, true) => {
                                if let Some(r) = run.take() {
                                    self.spans.push((row, r));
                                }
                                run = Some(start..end);
                            }
                            (_, false) => {
                                if let Some(r) = run.take() {
                                    self.spans.push((row, r));
                                }
                            }
                        }
                    }
                    if let Some(r) = run {
                        self.spans.push((row, r));
                    }
                }
            }
        }
        let dims = pane.get_dimensions();
        let top = self.get_viewport(pane.pane_id()).unwrap_or(dims.physical_top);
        let mut find = FindLink {
            link,
            spans: vec![],
        };
        pane.with_lines_mut(top..top + dims.viewport_rows as StableRowIndex, &mut find);
        self.lbterm_light_text(pane.pane_id(), &find.spans);
    }

    /// Where `pane_id`'s cells are in the window, and the scrollback row at the top of its
    /// screen
    fn lbterm_cells(&self, pane_id: PaneId) -> Option<(Cells, StableRowIndex)> {
        let mux = Mux::get();
        let tab = mux.get_active_tab_for_window(self.mux_window_id)?;
        let pos = tab
            .iter_panes()
            .into_iter()
            .find(|p| p.pane.pane_id() == pane_id)?;
        let dims = pos.pane.get_dimensions();
        let top = self.get_viewport(pane_id).unwrap_or(dims.physical_top);
        let (padding_left, padding_top) = self.padding_left_top();
        let border = self.get_os_border();
        let top_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height().ok()?
        } else {
            0.
        };
        let cell_w = self.render_metrics.cell_size.width as f64;
        let cell_h = self.render_metrics.cell_size.height as f64;
        Some((
            Cells {
                x: border.left.get() as f64 + padding_left as f64 + pos.left as f64 * cell_w,
                y: top_bar as f64
                    + padding_top as f64
                    + border.top.get() as f64
                    + pos.top as f64 * cell_h,
                cell_w,
                cell_h,
                cols: pos.width,
                rows: pos.height,
            },
            top,
        ))
    }

    /// The window's size changed: the readout shows the active pane's size in cells, unless
    /// the window has only just opened
    pub(crate) fn lbterm_readout_resized(&mut self) {
        let now = Instant::now();
        if !self.lbterm_composited() || now.saturating_duration_since(self.created) < SETTLE {
            return;
        }
        self.lbterm_lights.readout.resized(now);
        self.lbterm_readout_wake(self.lbterm_lights.readout.until());
    }

    /// The font got bigger, smaller or back: the readout says so, with the pane's new size
    pub(crate) fn lbterm_readout_font(&mut self) {
        if !self.lbterm_composited() {
            return;
        }
        self.lbterm_lights.readout.font(Instant::now());
        self.lbterm_readout_wake(self.lbterm_lights.readout.until());
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Where the readout's visual goes this frame and whether it shows (true) or fades, for
    /// the compositor; `None` while there's nothing to show
    pub(crate) fn lbterm_readout_visual(&self) -> Option<(PixelRect, bool)> {
        self.lbterm_lights.visual
    }

    /// The readout: laid out afresh while it shows, and drawn as it was while it fades out
    pub(crate) fn paint_lbterm_readout(&mut self) -> anyhow::Result<()> {
        self.lbterm_lights.visual = None;
        if !self.lbterm_composited() {
            return Ok(());
        }
        let now = Instant::now();
        let showing = self.lbterm_lights.readout.showing(now);
        let (fading, settles) = self
            .webgpu
            .as_ref()
            .and_then(|w| w.compositor.as_ref())
            .map_or((false, None), |c| c.readout_fading(now));
        let card = if showing {
            self.lbterm_readout_lay_out()?
        } else if fading {
            self.lbterm_lights.last.clone()
        } else {
            None
        };
        let Some((rect, text)) = card else {
            return Ok(());
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let window = (
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        self.lbterm_lights.visual = strip::visual_bounds(&rect, scale, window)
            .map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), showing));
        let surface = self.palette().background;
        self.lbterm_readout_draw(rect, &text, surface)?;
        self.lbterm_lights.last = Some((rect, text));
        if !showing {
            // Drawn until it has faded, then not
            self.lbterm_readout_wake(settles);
        }
        Ok(())
    }

    /// The readout for the active pane now, in the middle of it
    fn lbterm_readout_lay_out(&self) -> anyhow::Result<Option<(Rect, String)>> {
        let Some(pane) = Mux::get()
            .get_active_tab_for_window(self.mux_window_id)
            .and_then(|tab| tab.get_active_pane())
        else {
            return Ok(None);
        };
        let dims = pane.get_dimensions();
        let font_scale = self
            .lbterm_lights
            .readout
            .says_font()
            .then(|| self.fonts.get_font_scale());
        let text = lights::readout_text(dims.cols, dims.viewport_rows, font_scale);
        let scale = self.dimensions.dpi as f64 / 96.0;
        // The frame's font is proportional: what the words take, laid out
        let measured = self.lbterm_readout_words(
            &text,
            0.,
            0.,
            self.dimensions.pixel_width as f64,
            LinearRgba::TRANSPARENT,
        )?;
        let text_w = measured.content_rect.width() as f64;
        let text_h = measured.content_rect.height() as f64;
        // Over the pane it measures; the window's middle if that can't be found
        let room = match self.lbterm_cells(pane.pane_id()) {
            Some((cells, _)) => Rect {
                x: cells.x,
                y: cells.y,
                w: cells.cell_w * cells.cols as f64,
                h: cells.cell_h * cells.rows as f64,
            },
            None => Rect {
                x: 0.,
                y: 0.,
                w: self.dimensions.pixel_width as f64,
                h: self.dimensions.pixel_height as f64,
            },
        };
        Ok(Some((lights::readout_card(room, text_w, text_h, scale), text)))
    }

    /// The readout's words in the frame's font, laid out at `(x, y)` in `w`
    fn lbterm_readout_words(
        &self,
        text: &str,
        x: f64,
        y: f64,
        w: f64,
        colour: LinearRgba,
    ) -> anyhow::Result<crate::termwindow::box_model::ComputedElement> {
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let gl_state = self.render_state.as_ref().unwrap();
        let element = Element::new(&font, ElementContent::Text(text.to_string())).colors(ElementColors {
            border: BorderColor::default(),
            bg: LinearRgba::TRANSPARENT.into(),
            text: colour.into(),
        });
        self.compute_element(
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
                bounds: euclid::rect(x as f32, y as f32, w as f32, metrics.cell_size.height as f32),
                metrics: &metrics,
                gl_state,
                zindex: TEXT_Z,
            },
            &element,
        )
    }

    /// Draw the readout: the strip card's shadow, hairline and fill, and its words
    fn lbterm_readout_draw(&self, card: Rect, text: &str, surface: SrgbaTuple) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        let grow = |r: Rect, by: f64, down: f64| Rect {
            x: r.x - by,
            y: r.y - by + down,
            w: r.w + 2.0 * by,
            h: r.h + 2.0 * by,
        };
        {
            let layer = gl_state.layer_for_zindex(SHADOW_Z)?;
            let mut quads = layer.quad_allocator();
            let (reach, down) = strip::shadow_reach(scale);
            for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                let shadow = LinearRgba::with_components(0., 0., 0., *alpha);
                self.lbterm_menu_rounded(&mut quads, grow(card, by, down), radius + by, shadow)?;
            }
        }
        {
            let layer = gl_state.layer_for_zindex(CARD_Z)?;
            let mut quads = layer.quad_allocator();
            self.lbterm_menu_rounded(&mut quads, grow(card, stroke, 0.), radius + stroke, over(surface, RING_ALPHA))?;
            self.lbterm_menu_rounded(&mut quads, card, radius, surface.to_linear())?;
        }
        let (x, y) = lights::readout_text_at(card, scale);
        let computed = self.lbterm_readout_words(text, x, y, card.x + card.w - x, over(surface, TEXT_ALPHA))?;
        self.render_element(&computed, gl_state, None)?;
        Ok(())
    }

    /// Repaint at `at`: when the readout starts to fade, and when it has
    fn lbterm_readout_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        // One already due sooner repaints, and the paint asks again
        if matches!(self.lbterm_lights.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_lights.wake = Some(at);
        promise::spawn::spawn(async move {
            smol::Timer::at(at).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_lights.wake == Some(at) {
                    tw.lbterm_lights.wake = None;
                }
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }
}

/// LBTerm: the readout, for the state file (lbterm/STATE.md)
impl crate::TermWindow {
    /// What the readout says while it shows or fades this frame
    pub(crate) fn lbterm_state_readout(&self) -> Option<String> {
        self.lbterm_lights.visual.as_ref()?;
        self.lbterm_lights.last.as_ref().map(|(_, text)| text.clone())
    }

    /// How long the readout stays before it fades, while it shows
    pub(crate) fn lbterm_readout_wait(&self, now: Instant) -> Option<lbterm_extras::state::Wait> {
        let until = self.lbterm_lights.readout.until().filter(|u| *u > now)?;
        Some(lbterm_extras::state::Wait {
            name: "readout".to_string(),
            left_ms: lbterm_extras::state::millis(until.saturating_duration_since(now)),
        })
    }
}
