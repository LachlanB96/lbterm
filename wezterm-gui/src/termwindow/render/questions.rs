//! LBTerm: the question colours (lbterm/QUESTIONS.md); the logic is lbterm_extras::questions.
//! While the right button's held on a Claude pane (Terminal's lbterm.lua binds its press and
//! release there to `LbtermQuestions`), each question Claude's last replies asked, as Terminal's
//! question worker found them, gets a colour, and so does every passage of the reply it's
//! about; a question with a part scrolled off the screen gets a marker at that edge. They go
//! when the button's let go, wherever that is, and when the window loses the keyboard. The wheel
//! still scrolls Claude Code's conversation meanwhile, and the colours follow the screen.
//!
//! The pane's screen is laid out again only when it or the session's questions change. Bands
//! are drawn over the text in z-layer 23 and the markers in 24 and 25, all in the compositor's
//! `over` visual, beside the prompt mark.

use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use ::window::{MouseButtons as WMB, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::lbterm::LbtermQuestions;
use config::DimensionContext;
use lbterm_extras::questions::{self, Glyph, Layout, Screen};
use lbterm_extras::tabs::Rect;
use lbterm_panel::palette;
use mux::pane::{Pane, PaneId};
use std::rc::Rc;
use std::sync::Arc;
use termwiz::surface::SequenceNo;
use wezterm_term::StableRowIndex;
use window::color::LinearRgba;

/// The bands, and the markers' chips and words, in `over`
const BAND_Z: i8 = 23;
const CHIP_Z: i8 = 24;
const TEXT_Z: i8 = 25;
/// How much of its question's colour fills a passage's band, a question's, and the bar down
/// its left
const PASSAGE_ALPHA: f64 = 0.20;
const QUESTION_ALPHA: f64 = 0.30;
const BAR_ALPHA: f64 = 0.9;
/// The markers' words, dark on the colour
const MARKER_TEXT: u32 = 0x121212;

/// The colours while they're shown, and the last layout
#[derive(Default)]
pub struct QuestionsState {
    shown: Option<Shown>,
    cache: Option<(Key, Rc<Layout>)>,
}

struct Shown {
    pane: PaneId,
    /// The button that showed them, which holds them
    held: Option<MousePress>,
}

/// What a layout was made from
#[derive(PartialEq)]
struct Key {
    pane: PaneId,
    seqno: SequenceNo,
    top: StableRowIndex,
    rows: usize,
    cols: usize,
    generation: u64,
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn button(press: MousePress) -> WMB {
    match press {
        MousePress::Left => WMB::LEFT,
        MousePress::Right => WMB::RIGHT,
        MousePress::Middle => WMB::MIDDLE,
    }
}

/// The screen `rows` from `top` of `pane` show, as lay_out reads it
fn read_screen(pane: &Arc<dyn Pane>, top: StableRowIndex, rows: usize) -> Screen {
    let (_, lines) = pane.get_lines(top..top + rows as StableRowIndex);
    Screen {
        rows: lines
            .iter()
            .map(|line| {
                line.visible_cells()
                    .map(|c| Glyph {
                        col: c.cell_index(),
                        ch: c.str().chars().next().unwrap_or(' '),
                        width: c.width(),
                    })
                    .collect()
            })
            .collect(),
    }
}

impl TermWindow {
    /// `LbtermQuestions`: show the colours on `pane`, or hide them. A press that shows them
    /// holds them until it's let go.
    pub(crate) fn lbterm_questions(&mut self, pane: &Arc<dyn Pane>, mode: LbtermQuestions) {
        let show = match mode {
            LbtermQuestions::Show => true,
            LbtermQuestions::Hide => false,
            LbtermQuestions::Toggle => self.lbterm_questions.shown.is_none(),
        };
        if !show {
            self.lbterm_questions_hide();
            return;
        }
        let held = match self.current_mouse_event.as_ref().map(|e| &e.kind) {
            Some(WMEK::Press(press)) if mode == LbtermQuestions::Show => Some(*press),
            _ => None,
        };
        self.lbterm_questions.shown = Some(Shown {
            pane: pane.pane_id(),
            held,
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub(crate) fn lbterm_questions_hide(&mut self) {
        self.lbterm_questions.cache = None;
        if self.lbterm_questions.shown.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// Every mouse event, before anything takes it: the colours go once the button holding them
    /// is let go, or is no longer down, which is how one let go outside the window shows
    pub(crate) fn lbterm_questions_mouse(&mut self, event: &MouseEvent) {
        let Some(held) = self.lbterm_questions.shown.as_ref().and_then(|s| s.held) else {
            return;
        };
        let let_go = matches!(event.kind, WMEK::Release(press) if press == held);
        if let_go || !event.mouse_buttons.contains(button(held)) {
            self.lbterm_questions_hide();
        }
    }

    /// What the colours show on `pane` now: laid out again when its screen or its session's
    /// questions change. None without a Claude session or questions.
    fn lbterm_questions_layout(&mut self, pane: &Arc<dyn Pane>) -> Option<Rc<Layout>> {
        let sid = pane
            .copy_user_vars()
            .remove("claude_session")
            .filter(|sid| !sid.is_empty())?;
        let store = crate::summaries::store()?;
        let map = store.get(&sid)?.questions?;
        let dims = pane.get_dimensions();
        let top = self.get_viewport(pane.pane_id()).unwrap_or(dims.physical_top);
        let key = Key {
            pane: pane.pane_id(),
            seqno: pane.get_current_seqno(),
            top,
            rows: dims.viewport_rows,
            cols: dims.cols,
            generation: store.generation(),
        };
        if let Some((k, layout)) = &self.lbterm_questions.cache {
            if *k == key {
                return Some(Rc::clone(layout));
            }
        }
        let screen = read_screen(pane, top, dims.viewport_rows);
        let layout = Rc::new(questions::lay_out(&map, &screen));
        self.lbterm_questions.cache = Some((key, Rc::clone(&layout)));
        Some(layout)
    }

    /// Draw the colours, while they're shown: a band over each question and passage with a bar
    /// down its left, a bar for each question a passage goes with, and the markers at the
    /// conversation's top and bottom edges, right-aligned
    pub(crate) fn paint_lbterm_questions(&mut self) -> anyhow::Result<()> {
        let Some(pane_id) = self.lbterm_questions.shown.as_ref().map(|s| s.pane) else {
            return Ok(());
        };
        let panes = self.get_panes_to_render();
        let Some(pos) = panes.iter().find(|p| p.pane.pane_id() == pane_id) else {
            return Ok(());
        };
        let Some(layout) = self.lbterm_questions_layout(&pos.pane) else {
            return Ok(());
        };
        if layout.is_empty() {
            return Ok(());
        }

        let (padding_left, padding_top) = self.padding_left_top();
        let border = self.get_os_border();
        let top_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let cell_w = self.render_metrics.cell_size.width as f64;
        let cell_h = self.render_metrics.cell_size.height as f64;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let left = border.left.get() as f64 + padding_left as f64 + pos.left as f64 * cell_w;
        let right = left + pos.width as f64 * cell_w;
        let row_y = |row: usize| {
            top_bar as f64 + padding_top as f64 + border.top.get() as f64 + (pos.top + row) as f64 * cell_h
        };
        let pad = 4.0 * scale;
        let bar_w = (3.0 * scale).max(2.0);

        let gl_state = self.render_state.as_ref().unwrap();
        {
            let layer = gl_state.layer_for_zindex(BAND_Z)?;
            let mut quads = layer.quad_allocator();
            for band in &layout.bands {
                let Some(&first) = band.colours.first() else {
                    continue;
                };
                let x0 = (left + band.cols.start as f64 * cell_w - pad).max(left - pad);
                let x1 = (left + band.cols.end as f64 * cell_w + pad).min(right + pad);
                // A pixel in at the top and bottom, so bands on rows next to each other stay apart
                let area = Rect {
                    x: x0,
                    y: row_y(band.rows.start) + scale,
                    w: x1 - x0,
                    h: band.rows.len() as f64 * cell_h - 2.0 * scale,
                };
                let alpha = if band.question { QUESTION_ALPHA } else { PASSAGE_ALPHA };
                self.lbterm_menu_rounded(&mut quads, area, 4.0 * scale, srgb(questions::colour(first), alpha))?;
                for (i, colour) in band.colours.iter().enumerate() {
                    let bar = Rect {
                        x: area.x + i as f64 * (bar_w + scale),
                        y: area.y,
                        w: bar_w,
                        h: area.h,
                    };
                    self.lbterm_menu_rounded(&mut quads, bar, bar_w / 2.0, srgb(questions::colour(*colour), BAR_ALPHA))?;
                }
            }
        }

        if layout.markers.is_empty() {
            return Ok(());
        }
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let text_w = font.metrics().cell_width.get();
        let text_h = metrics.cell_size.height as f64;
        let chip_h = text_h + 2.0 * scale;
        let gap = 6.0 * scale;
        let mut chips = vec![];
        for up in [true, false] {
            // At the edge of the conversation, clear of Claude Code's input box
            let y = if up {
                row_y(layout.area.start) + 2.0 * scale
            } else {
                row_y(layout.area.end.max(1)) - chip_h - 2.0 * scale
            };
            // Right-aligned, in the questions' order from the left
            let edge: Vec<(String, f64, u32)> = layout
                .markers
                .iter()
                .filter(|m| m.up == up)
                .map(|m| {
                    let label = format!("{} {}", m.label, if up { '\u{2191}' } else { '\u{2193}' });
                    let w = label.chars().count() as f64 * text_w + 2.0 * pad;
                    (label, w, questions::colour(m.colour))
                })
                .collect();
            let total: f64 = edge.iter().map(|(_, w, _)| w + gap).sum();
            let mut x = right - total;
            for (label, w, colour) in edge {
                chips.push((Rect { x, y, w, h: chip_h }, label, colour));
                x += w + gap;
            }
        }
        let mut words = vec![];
        for (chip, label, _) in &chips {
            let element = Element::new(&font, ElementContent::Text(label.clone())).colors(ElementColors {
                border: BorderColor::default(),
                bg: LinearRgba::TRANSPARENT.into(),
                text: srgb(MARKER_TEXT, 1.0).into(),
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
                    bounds: euclid::rect(
                        (chip.x + pad) as f32,
                        (chip.y + (chip.h - text_h) / 2.0) as f32,
                        (chip.w - pad) as f32,
                        text_h as f32,
                    ),
                    metrics: &metrics,
                    gl_state,
                    zindex: TEXT_Z,
                },
                &element,
            )?);
        }
        {
            let layer = gl_state.layer_for_zindex(CHIP_Z)?;
            let mut quads = layer.quad_allocator();
            for (chip, _, colour) in &chips {
                self.lbterm_menu_rounded(&mut quads, *chip, chip.h / 2.0, srgb(*colour, 0.95))?;
            }
        }
        for computed in &words {
            self.render_element(computed, gl_state, None)?;
        }
        Ok(())
    }
}

/// LBTerm: the question colours, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// The pane whose question colours show
    pub(crate) fn lbterm_state_questions(&self) -> Option<u64> {
        self.lbterm_questions.shown.as_ref().map(|s| s.pane as u64)
    }
}
