//! LBTerm: the questions bar (lbterm/QUESTIONS.md, "The questions bar"). While the right button
//! holds the question colours on a Claude pane, a bar the closed tab rail's size, the rail
//! mirrored (`config::lbterm::QUESTIONS_PANE_WIDTH`, Lachlan's ask of 2026-11-11 and his 3c of
//! 2026-11-14), lies down the window's right from under the tab bar to its foot, over the strips
//! the window keeps there and the panes' last few columns, listing that conversation's questions,
//! newest reply first, a hairline between replies: each a pad in its colour with its label, and
//! under it a tick once he's answered it or an open mark while he hasn't, with an arrow while its
//! words are off the screen. When they don't all fit, its last slot says how many more there are.
//!
//! The rows are lbterm_extras::questions::pane's (`rows`, the one function a harness session
//! changes): from the session's questions file, read off this thread with its summaries
//! (crate::summaries), and the turns read from its transcript on the conversations' reading
//! thread, which follows the session while a hold shows it (`Reader::Questions`). They're made
//! again only when either changes, and the card's quads and words are kept while they're the same
//! (render/kept_quads.rs, render/words.rs). Nothing here reads a file. Stage 4 of
//! feat/questions-pane drew a card as tall as its rows with a shadow; Lachlan, 2026-11-14: "it was
//! supposed to be a question bar, not some crappy tooltip pane".
//!
//! It's drawn in z-layers of its own, over the bands, the markers and the strips, and never moves
//! or resizes a pane: it lies over them. With the compositor those z-layers are a visual of its
//! own (`Target::QuestionsPane`), which DWM slides out from under the window's right edge on
//! `panel_open` as the hold starts and back under it on `panel_close` as the hold ends, from
//! wherever it is: a release mid-slide turns it round there, and a press while it goes brings it
//! out again from there (compositor.rs, `EdgeSlide`). The pane that's going is drawn as it was
//! until it's gone. Only its offset moves, so its words move flat with it, and the panes' text
//! under it never moves. Without the compositor, or with motion off, it comes and goes at once.

use crate::termwindow::render::kept_quads::KeptQuads;
use crate::termwindow::render::objectives::over;
use crate::termwindow::render::questions::srgb;
use crate::termwindow::render::words::{Word, Words};
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::WindowOps;
use config::DimensionContext;
use lbterm_extras::conversation::Chunks;
use lbterm_extras::questions::{self, pane::{self, Row}};
use lbterm_extras::state::{QuestionsPane, QuestionsPaneRow, Rect as StateRect};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{layout, palette};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// The bar's fill and ring, its hairlines, pads and open marks, then its words: in `over`, over
/// the bands and markers (23 to 25), the strips (38), a modal (100) and the context bar (101 to
/// 110, render/context_bar.rs, which had 101 first: PITFALLS 154)
const CARD_Z: i8 = 111;
const TEXT_Z: i8 = 112;

/// Whether a z-layer is the questions pane's: with the compositor, its visual's alone
pub fn owns_zindex(zindex: i8) -> bool {
    (CARD_Z..=TEXT_Z).contains(&zindex)
}
/// A pad's label, dark on its colour, as the markers' are
const LABEL_TEXT: u32 = 0x121212;
/// A pad's corners, as a tab's pad's on the rail (render/tab_panel.rs)
const PAD_RADIUS: f32 = 7.0;
/// The hairlines between replies, in the grey of the rule over the rail's new-tab pad
const RULE_ALPHA: f64 = 0.6;
/// White over the bar, as the eye sees it: the tick, the open mark's ring, "+3" and "none"
const TICK_ALPHA: f32 = 0.85;
const OPEN_ALPHA: f32 = 0.40;
const MORE_ALPHA: f32 = 0.50;
/// A question's arrow, up or down to where its words are, as the tick is
const ARROW_ALPHA: f32 = 0.85;

/// What the compositor is given of the bar each frame (compositor.rs, `prepare_questions_pane`):
/// its surface at its place at rest, what of the window it may show in (up to the window's right
/// edge), whether it's coming out (the hold goes on) or going back, how far it comes out (the
/// rail's strip), and, while the tab panel's rail is there to push off, the tab bar's bottom:
/// everything under it moves left as far as the bar comes out (Lachlan's 16 of 2026-11-14)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarVisual {
    pub bounds: PixelRect,
    pub clip: PixelRect,
    pub open: bool,
    pub travel: f64,
    pub push: Option<i32>,
}

/// The rows as last made, and what they were made from
struct Made {
    session: String,
    /// The summaries' generation, and the conversation's revision once it's been read
    generation: u64,
    revision: Option<u64>,
    rows: Rc<Vec<Row>>,
    id: u64,
}

#[derive(Default)]
pub struct PaneState {
    made: Option<Made>,
    /// Rows made, so what's kept from them is known by it
    makes: u64,
    /// The card as this frame drew it, for the state file; none while it isn't shown
    pub(crate) drawn: Option<QuestionsPane>,
    quads: RefCell<KeptQuads>,
    words: RefCell<Words>,
    /// Its visual this frame, for the compositor (`BarVisual`); `None` while there's nothing to
    /// show
    pub(crate) visual: Option<BarVisual>,
    /// The session the hold last showed, drawn as it was while the pane goes back under the edge
    leaving: Option<String>,
    /// When a repaint is already on its way: the slide back done, so it's put away
    wake: Option<Instant>,
    /// Where the rows went down the bar (`pane::place`), kept while the same make of the rows
    /// goes in the same room at the same sizes: a hold's frames place nothing again
    placed: Option<(u64, [u64; 4], Rc<pane::Placed>)>,
    /// Where each question is against the screen, as the hold's last frame laid the colours out:
    /// each row's arrow (Lachlan's E15 of 2026-11-14), kept for the bar going back
    places: Vec<questions::Place>,
}

fn grow(r: Rect, by: f64, down: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by + down,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

impl crate::TermWindow {
    /// Session `sid`'s rows, and which make of them it is: made again when its questions file or
    /// its conversation has changed. `None` until its summaries have been read (asked for now)
    fn lbterm_questions_pane_rows(&mut self, sid: &str) -> Option<(Rc<Vec<Row>>, u64)> {
        let store = crate::summaries::store()?;
        let generation = store.generation();
        let snapshot = crate::conversations::get(sid);
        let revision = snapshot.as_ref().map(|s| s.revision);
        let state = &mut self.lbterm_questions.pane;
        if let Some(made) = state
            .made
            .as_ref()
            .filter(|m| m.session == sid && m.generation == generation && m.revision == revision)
        {
            return Some((Rc::clone(&made.rows), made.id));
        }
        let summary = store.get(sid)?;
        // Until the conversation's been read every question shows open, and its first read
        // repaints the window (crate::conversations)
        let none = Chunks::default();
        let turns = snapshot.as_ref().map_or(&none, |s| &s.turns);
        let rows = Rc::new(summary.questions.as_ref().map_or_else(Vec::new, |map| pane::rows(map, turns)));
        state.makes += 1;
        state.made = Some(Made {
            session: sid.to_string(),
            generation,
            revision,
            rows: Rc::clone(&rows),
            id: state.makes,
        });
        Some((rows, state.makes))
    }

    /// The questions pane going back under the window's edge once the hold that showed it has
    /// ended: drawn as it was, for the session it showed, while the compositor says its slide
    /// back still shows something, and a repaint asked for when that's done, which puts it away.
    /// Nothing once it's gone, without the compositor or with motion off (gone at once)
    pub(crate) fn paint_lbterm_questions_pane_leaving(&mut self) -> anyhow::Result<()> {
        let Some(sid) = self.lbterm_questions.pane.leaving.clone() else {
            return Ok(());
        };
        let (showing, settles) = if self.lbterm_composited() && self.lbterm_motion().enabled {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or((false, None), |c| c.questions_pane_showing(Instant::now()))
        } else {
            (false, None)
        };
        if !showing {
            self.lbterm_questions.pane.leaving = None;
            return Ok(());
        }
        self.paint_lbterm_questions_pane(Some(&sid), false, None)?;
        self.lbterm_questions_pane_wake(settles);
        Ok(())
    }

    /// Repaint at `at`, when the slide back is done
    fn lbterm_questions_pane_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        // One already due as soon repaints, and the paint asks again
        if matches!(self.lbterm_questions.pane.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_questions.pane.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_questions.pane.wake != Some(at) {
                    return;
                }
                tw.lbterm_questions.pane.wake = None;
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// Where the questions pane's visual goes this frame, what it may show in and whether it's
    /// coming out, for the compositor; `None` while there's nothing to show
    pub(crate) fn lbterm_questions_pane_visual(&self) -> Option<BarVisual> {
        self.lbterm_questions.pane.visual
    }

    /// Draw the questions bar for session `sid`, coming out (`open`, the hold goes on) or going
    /// back, and say where its left edge is, so the ticks stay clear of it; nothing without a
    /// session or before its summaries are read. `places` says where each question is against
    /// the screen, for its arrow; `None` keeps the last (the bar going back draws as it was)
    pub(crate) fn paint_lbterm_questions_pane(
        &mut self,
        sid: Option<&str>,
        open: bool,
        places: Option<&[questions::Place]>,
    ) -> anyhow::Result<Option<f64>> {
        let Some(sid) = sid else {
            return Ok(None);
        };
        let Some((rows, rows_id)) = self.lbterm_questions_pane_rows(sid) else {
            return Ok(None);
        };
        if let Some(places) = places {
            if self.lbterm_questions.pane.places[..] != *places {
                self.lbterm_questions.pane.places = places.to_vec();
            }
        }
        // Each row's arrow: up or down while its question's words are off the screen that way
        let arrows: Vec<Option<bool>> = {
            let places = &self.lbterm_questions.pane.places;
            rows.iter()
                .map(|row| {
                    places
                        .iter()
                        .find(|p| p.reply == row.reply && p.colour == row.colour)
                        .and_then(|p| p.at.arrow())
                })
                .collect()
        };
        let surface = self.palette().background;
        let scale = self.dimensions.dpi as f64 / 96.0;
        // The rail's own sizes at 96 DPI in this window's pixels, as its Layout::px has them
        let px = |v: f32| (v as f64 * scale).round().max(1.0);
        let shape = self.lbterm_shape();
        let (gutter, stroke, radius) = (shape.gutter() as f64, shape.hairline() as f64, shape.radius() as f64);
        let room = self.lbterm_room()?;
        // The window's right edge inside any border of its frame's own: past the scroll strip
        // and the objectives strip the window keeps down its right, as the objectives strip
        // finds it (render/objectives.rs)
        let edge = (room.right as f64
            + super::scroll_strip::strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64
            + super::objectives::strip_width(&self.config, &self.dimensions, &self.render_metrics) as f64)
            .round();
        // The rail's strip on the left (render/tab_panel.rs, `rail_width`: a gutter and the rail).
        // The bar is the rail's size, and comes out as far as the rail is from the left edge, so
        // what it pushes left (Lachlan's 16 of 2026-11-14) takes the rail off the screen and the
        // conversation comes to rest a gutter in, as the bar is. With no tab panel there's no
        // rail to push: the token's width, lying over the right as it did
        let rail_strip = super::tab_panel::rail_width(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let (width, strip) = if rail_strip > 0.0 {
            let strip = rail_strip.round();
            (strip - gutter.round(), strip)
        } else {
            let width = config::lbterm::QUESTIONS_PANE_WIDTH
                .evaluate_as_pixels(DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_width as f32,
                    pixel_cell: self.render_metrics.cell_size.width as f32,
                })
                .round() as f64;
            (width, width + gutter.round())
        };

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let text_w = font.metrics().cell_width.get();
        let text_h = metrics.cell_size.height as f64;

        // The bar: the closed tab rail mirrored down the window's right (Lachlan's ask of
        // 2026-11-11, "a pane the size of the non-expanded tab bar pane", and his words of
        // 2026-11-14 on the card first built here, "it was supposed to be a question bar"): the
        // rail's width, from a gutter under the tab bar to a gutter above the window's foot, its
        // fill and its grey ring, and no shadow: a part of the layout, not a card over it
        let top = room.top as f64 + gutter;
        let bar = Rect {
            x: edge - strip,
            y: top,
            w: width,
            h: (room.bottom as f64 - gutter - top).max(0.0),
        };
        // Its list, as the rail's: in from the ring, a slot a question, each a pad and a line
        // under it, every slot as tall as the rail's rows while they fit and shrinking together,
        // to the rail's least, to fit more (pane::place)
        let inset = stroke + px(layout::LIST_PAD);
        let list_top = bar.y + inset;
        let list = (bar.h - 2.0 * inset).max(0.0);
        let line_block = px(layout::LABEL_GAP) + text_h;
        let (row_min, row_max) = (px(layout::ROW_MIN) + line_block, px(layout::ROW_MAX) + line_block);
        let rule_h = px(layout::HEAD_GAP);
        let sizes = [list, row_min, row_max, rule_h].map(f64::to_bits);
        let kept = match &self.lbterm_questions.pane.placed {
            Some((id, kept, placed)) if *id == rows_id && *kept == sizes => Some(Rc::clone(placed)),
            _ => None,
        };
        let placed = match kept {
            Some(placed) => placed,
            None => {
                let placed = Rc::new(pane::place(&rows, list, row_min, row_max, rule_h));
                self.lbterm_questions.pane.placed = Some((rows_id, sizes, Rc::clone(&placed)));
                placed
            }
        };
        let column = (bar.x + width / 2.0).round();
        let (pad_w, pad_h) = (px(layout::PAD_W), px(layout::PAD_H));
        let pad_most = (width - 2.0 * inset).max(pad_w);
        let mark = px(9.0);

        // A row's pad, as a tab's on the rail: centred on the column, with the line under it,
        // the two centred in the slot together; wide enough for its label ("Q10", "E15")
        let pad_of = |row: &Row, slot_top: f64| -> Rect {
            let label_w = row.label.chars().count() as f64 * text_w;
            let w = (label_w + px(8.0)).max(pad_w).min(pad_most).round();
            let slot_h = placed.row_h - px(layout::ROW_GAP);
            Rect {
                x: column - (w / 2.0).round(),
                y: (list_top + slot_top + (slot_h - pad_h - line_block) / 2.0).round(),
                w,
                h: pad_h,
            }
        };
        // The middle of the line under a pad
        let line_middle = |pad: &Rect| pad.y + pad.h + px(layout::LABEL_GAP) + text_h / 2.0;
        // Where on that line the answered mark goes: the column, or left of it with an arrow
        // beside it on its right (Lachlan's E15 of 2026-11-14)
        let mark_x = |i: usize| if arrows[i].is_some() { column - 0.75 * text_w } else { column };
        let arrow_x = column + 0.75 * text_w;

        // Its shapes, bottom to top: the fill and the grey ring as the rail's (render/tab_panel.rs),
        // the hairlines between replies in the grey of the rule over the rail's new-tab pad, then
        // each row's pad and open mark. Made only when they aren't kept: a hold's frames with the
        // same rows in the same place make none
        let arrows_key = arrows.iter().fold(0u64, |key, a| key.rotate_left(2) ^ a.map_or(0, |up| if up { 1 } else { 2 }));
        let from = (
            (rows_id, arrows_key),
            [bar.x, bar.y, bar.w, bar.h, placed.row_h, scale, text_w, list].map(f64::to_bits),
            [surface.0, surface.1, surface.2].map(f32::to_bits),
        );
        self.lbterm_kept_quads(&self.lbterm_questions.pane.quads, CARD_Z, from, |quads| {
            self.lbterm_menu_rounded(quads, grow(bar, -stroke, 0.0), radius - stroke, surface.to_linear())?;
            self.lbterm_stroke(
                quads,
                lbterm_panel::Area::new(bar.x as f32, bar.y as f32, bar.w as f32, bar.h as f32),
                radius as f32,
                stroke as f32,
                px(layout::FEATHER) as f32,
                |_| palette::OUTLINE,
            )?;
            let half = ((width / 2.0 - inset) * 0.8).round();
            for rule in &placed.rules {
                let line = Rect {
                    x: column - half,
                    y: (list_top + rule - stroke / 2.0).round(),
                    w: 2.0 * half,
                    h: stroke,
                };
                self.lbterm_menu_rounded(quads, line, 0.0, srgb(palette::OUTLINE, RULE_ALPHA))?;
            }
            for (i, slot_top) in &placed.rows {
                let row = &rows[*i];
                let pad = pad_of(row, *slot_top);
                // An answered one's at half its colour over the bar, so the open ones stand out
                // (Lachlan's 16a of 2026-11-14, `Row::fill`)
                self.lbterm_menu_rounded(quads, pad, px(PAD_RADIUS), srgb(questions::colour(row.colour), row.fill()))?;
                if !row.answered() {
                    // An open ring: a disc, and the bar's fill inside it
                    let middle = line_middle(&pad);
                    let disc = Rect {
                        x: mark_x(*i) - mark / 2.0,
                        y: middle - mark / 2.0,
                        w: mark,
                        h: mark,
                    };
                    self.lbterm_menu_rounded(quads, disc, mark / 2.0, over(surface, OPEN_ALPHA))?;
                    let hole = grow(disc, -(1.5 * scale).max(1.0), 0.0);
                    self.lbterm_menu_rounded(quads, hole, hole.w / 2.0, surface.to_linear())?;
                }
            }
            Ok(())
        })?;

        // Its words: each label dark on its pad, a tick under it once answered, "+N" and "none"
        let mut words: Vec<Word> = vec![];
        let centred_at = |text: &str, x: f64, middle: f64, alpha: f32| -> Word {
            let w = text.chars().count() as f64 * text_w;
            let bounds = euclid::rect(
                (x - w / 2.0) as f32,
                (middle - text_h / 2.0) as f32,
                w as f32 + 1.0,
                text_h as f32,
            );
            (text.to_string(), bounds, over(surface, alpha))
        };
        for (i, slot_top) in &placed.rows {
            let row = &rows[*i];
            let pad = pad_of(row, *slot_top);
            let w = row.label.chars().count() as f64 * text_w;
            words.push((
                row.label.clone(),
                euclid::rect(
                    (pad.x + (pad.w - w) / 2.0) as f32,
                    (pad.y + (pad.h - text_h) / 2.0) as f32,
                    w as f32 + 1.0,
                    text_h as f32,
                ),
                srgb(LABEL_TEXT, 1.0),
            ));
            if row.answered() {
                words.push(centred_at("\u{2713}", mark_x(*i), line_middle(&pad), TICK_ALPHA));
            }
            if let Some(up) = arrows[*i] {
                let arrow = if up { "\u{2191}" } else { "\u{2193}" };
                words.push(centred_at(arrow, arrow_x, line_middle(&pad), ARROW_ALPHA));
            }
        }
        let more_middle = list_top + placed.more_at + (placed.row_h - px(layout::ROW_GAP)) / 2.0;
        if placed.more > 0 {
            words.push(centred_at(&format!("+{}", placed.more), column, more_middle, MORE_ALPHA));
        }
        if rows.is_empty() {
            words.push(centred_at("none", column, more_middle, MORE_ALPHA));
        }
        let cell = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_questions.pane.words, &font, &metrics, cell, TEXT_Z, words)?;

        // Its visual: the bar at rest and a pixel round it for its ring's soft edge, seen up to
        // the window's right edge, which it comes out from under as far as the rail's strip;
        // pushing what's under the tab bar left as far while there's a rail to push off
        let window = (self.dimensions.pixel_width as i32, self.dimensions.pixel_height as i32);
        let x = (bar.x.floor() as i32 - 1).max(0);
        let y = (bar.y.floor() as i32 - 1).max(0);
        let right = ((bar.x + bar.w).ceil() as i32 + 1).min(window.0);
        let bottom = ((bar.y + bar.h).ceil() as i32 + 1).min(window.1);
        let pushing = rail_strip > 0.0;
        self.lbterm_questions.pane.visual = (right > x && bottom > y).then(|| {
            let clip_right = (edge as i32).clamp(x, window.0);
            BarVisual {
                bounds: PixelRect::new(x, y, (right - x) as u32, (bottom - y) as u32),
                clip: PixelRect::new(x, y, (clip_right - x) as u32, (bottom - y) as u32),
                open,
                travel: strip,
                push: pushing.then(|| room.top.round() as i32),
            }
        });
        if self.lbterm_questions.pane.leaving.as_deref() != Some(sid) {
            self.lbterm_questions.pane.leaving = Some(sid.to_string());
        }

        // What the state file says shows is the bar a hold shows; the one going back is
        // `questions.pane` among its animations
        if open && crate::state_probe::on() {
            self.lbterm_questions.pane.drawn = Some(QuestionsPane {
                rect: StateRect::round(bar.x, bar.y, bar.w, bar.h),
                rows: placed
                    .rows
                    .iter()
                    .map(|(i, _)| {
                        let row = &rows[*i];
                        QuestionsPaneRow {
                            label: row.label.clone(),
                            colour: row.colour,
                            answered: row.answered(),
                            age: row.age,
                            arrow: arrows[*i].map(|up| if up { "up" } else { "down" }),
                        }
                    })
                    .collect(),
                more: placed.more,
            });
        }
        // Pushing, the conversation moves out of the bar's way, so the ticks keep to the panes'
        // right edge; lying over it, they stay left of the bar
        Ok(Some(if pushing { edge } else { bar.x }))
    }

    /// The questions pane as this frame drew it, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_state_questions_pane(&self) -> Option<QuestionsPane> {
        self.lbterm_questions.pane.drawn.clone()
    }
}
