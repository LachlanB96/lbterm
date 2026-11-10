//! LBTerm: the question colours (lbterm/QUESTIONS.md); the logic is lbterm_extras::questions.
//! While the right button's held on a Claude pane (Terminal's lbterm.lua binds its press and
//! release there to `LbtermQuestions`), each question Claude's last replies asked, as Terminal's
//! question worker found them, gets a colour, and so does every passage of the reply it's
//! about; a question with a part scrolled off the screen gets a marker at that edge. They go
//! when the button's let go, wherever that is, and when the window loses the keyboard. The wheel
//! still scrolls Claude Code's conversation meanwhile, and the colours follow the screen. While a
//! voice draft is open over a pane they show there with no button held, and the next question it
//! hasn't answered is ringed in red (lbterm/QUESTIONS.md, "While a voice draft is open").
//!
//! The pane's screen is read again only when the pane has changed, and laid out again only when
//! its conversation (what's above Claude Code's input box) or the session's questions change; the
//! bands', markers' and ticks' quads and words are kept while they're the same (render/words.rs,
//! render/kept_quads.rs). Each frame a voice draft showed read its pane's whole screen again, and
//! laid it out again for any output in the pane, its status line's turning included (PERF.md,
//! finding 63). Bands are drawn over the text in z-layer 23 and the markers in 24 and 25, all in
//! the compositor's `over` visual, beside the prompt mark.

use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::render::draft::draft_for;
use crate::termwindow::render::kept_quads::KeptQuads;
use crate::termwindow::render::words::{Word, Words};
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use ::window::{MouseButtons as WMB, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::lbterm::LbtermQuestions;
use std::cell::RefCell;
use lbterm_extras::questions::{self, Asked, Glyph, Layout, Next, Screen};
use lbterm_extras::state::{NextQuestion, Rect as StateRect};
use lbterm_extras::tabs::Rect;
use lbterm_panel::{palette, Area};
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
/// The ring round the next question a voice draft hasn't answered: red, which none of the
/// questions' colours is, so it's the one thing on the screen saying "this one next"
const NEXT_RING: u32 = 0xFF3B30;

/// The colours while they're shown, the last layout, and the screens it and the drafts read
#[derive(Default)]
pub struct QuestionsState {
    shown: Option<Shown>,
    cache: Option<Laid>,
    /// Layouts made, so what's kept from one is known by it
    layouts: u64,
    /// The screens of the panes whose drafts or colours show, as last read (render/draft.rs too)
    screens: Vec<KeptScreen>,
    /// Screens read, so what's made from one is known by it
    reads: u64,
    /// The bands' quads, the markers' and ticks' chips and their words, kept while the same
    bands: RefCell<KeptQuads>,
    chips: RefCell<KeptQuads>,
    pub(crate) words: RefCell<Words>,
}

/// The last layout: what it was made from, the screen it was made of (by its read and by its
/// conversation), and what it made; none without a Claude session's questions
struct Laid {
    key: Key,
    read: u64,
    conversation: u64,
    id: u64,
    layout: Option<Rc<Layout>>,
}

/// A pane's screen as last read, kept while the pane shows the same
struct KeptScreen {
    pane: PaneId,
    top: StableRowIndex,
    rows: usize,
    cols: usize,
    seqno: SequenceNo,
    read: u64,
    screen: Rc<Screen>,
    /// Every row was there to read as the pane has it ([`rows_held`]): a read that met a mux
    /// pane's row still on its way is read again, since the row's arrival moves no seqno
    whole: bool,
}

struct Shown {
    pane: PaneId,
    /// The button that showed them, which holds them
    held: Option<MousePress>,
}

/// What a layout was made from, besides its screen
#[derive(PartialEq)]
struct Key {
    pane: PaneId,
    session: String,
    top: StableRowIndex,
    rows: usize,
    cols: usize,
    generation: u64,
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The ring round the next question, for the state file
fn said_next(label: &str, off: bool, ring: Area) -> NextQuestion {
    NextQuestion {
        label: label.to_string(),
        off,
        rect: StateRect::round(ring.x as f64, ring.y as f64, ring.w as f64, ring.h as f64),
    }
}

fn button(press: MousePress) -> WMB {
    match press {
        MousePress::Left => WMB::LEFT,
        MousePress::Right => WMB::RIGHT,
        MousePress::Middle => WMB::MIDDLE,
        MousePress::Back => WMB::X1,
        MousePress::Forward => WMB::X2,
    }
}

/// The screen `rows` from `top` of `pane` show, as lay_out reads it (and a voice draft's checks,
/// termwindow/control.rs)
/// Whether `pane` has every line of `rows` to read as it is: a mux pane's client may not hold
/// one yet, or hold it stale, and get_lines gives a blank or an old line for it while it fetches
/// it, which moves no sequence number when it comes (wezterm-client's `lbterm_holds`). Always
/// for the GUI's own panes
pub(crate) fn rows_held(pane: &Arc<dyn Pane>, rows: std::ops::Range<StableRowIndex>) -> bool {
    match pane.downcast_ref::<wezterm_client::pane::ClientPane>() {
        Some(client) => client.renderable.lock().lbterm_holds(rows),
        None => true,
    }
}

pub(crate) fn read_screen(pane: &Arc<dyn Pane>, top: StableRowIndex, rows: usize) -> Screen {
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

    /// The screen `pane` shows now, from the top of its view, as [`read_screen`] reads it: read
    /// again only when the pane has changed since it was last read (its seqno), or that read met
    /// a row the pane didn't have to give yet ([`rows_held`]), and which read it is, so what's
    /// made from it can be kept while it's the same
    pub(crate) fn lbterm_screen(&mut self, pane: &Arc<dyn Pane>) -> (Rc<Screen>, u64) {
        let dims = pane.get_dimensions();
        let top = self.get_viewport(pane.pane_id()).unwrap_or(dims.physical_top);
        let (id, rows, cols) = (pane.pane_id(), dims.viewport_rows, dims.cols);
        let seqno = pane.get_current_seqno();
        let state = &mut self.lbterm_questions;
        if let Some(kept) = state.screens.iter().find(|k| {
            k.whole && k.pane == id && k.top == top && k.rows == rows && k.cols == cols && k.seqno == seqno
        }) {
            return (Rc::clone(&kept.screen), kept.read);
        }
        let screen = Rc::new(read_screen(pane, top, rows));
        let whole = rows_held(pane, top..top + rows as StableRowIndex);
        let state = &mut self.lbterm_questions;
        state.reads += 1;
        state.screens.retain(|k| k.pane != id);
        // A pane or two have a draft or the colours at a time; a few kept is plenty
        if state.screens.len() >= 4 {
            state.screens.remove(0);
        }
        state.screens.push(KeptScreen {
            pane: id,
            top,
            rows,
            cols,
            seqno,
            read: state.reads,
            screen: Rc::clone(&screen),
            whole,
        });
        (screen, state.reads)
    }

    /// What the colours show on `pane` now, and which layout it is: laid out again when its
    /// conversation (what's above Claude Code's input box) or its session's questions change.
    /// None without a Claude session or questions.
    fn lbterm_questions_layout(&mut self, pane: &Arc<dyn Pane>) -> Option<(Rc<Layout>, u64)> {
        let sid = pane
            .copy_user_vars()
            .remove("claude_session")
            .filter(|sid| !sid.is_empty())?;
        let store = crate::summaries::store()?;
        let dims = pane.get_dimensions();
        let key = Key {
            pane: pane.pane_id(),
            session: sid,
            top: self.get_viewport(pane.pane_id()).unwrap_or(dims.physical_top),
            rows: dims.viewport_rows,
            cols: dims.cols,
            generation: store.generation(),
        };
        let (screen, read) = self.lbterm_screen(pane);
        if let Some(laid) = self.lbterm_questions.cache.as_mut().filter(|l| l.key == key) {
            if laid.read != read {
                // Read again: the same layout while the conversation's the same
                let conversation = questions::conversation_hash(&screen);
                if conversation != laid.conversation {
                    return self.lbterm_questions_lay_out(key, &screen, read, Some(conversation));
                }
                laid.read = read;
            }
            return laid.layout.clone().map(|l| (l, laid.id));
        }
        self.lbterm_questions_lay_out(key, &screen, read, None)
    }

    fn lbterm_questions_lay_out(
        &mut self,
        key: Key,
        screen: &Screen,
        read: u64,
        conversation: Option<u64>,
    ) -> Option<(Rc<Layout>, u64)> {
        let conversation = conversation.unwrap_or_else(|| questions::conversation_hash(screen));
        let map = crate::summaries::store().and_then(|store| store.get(&key.session)?.questions.clone());
        let layout = map.map(|map| Rc::new(questions::lay_out(&map, screen)));
        let state = &mut self.lbterm_questions;
        state.layouts += 1;
        let id = state.layouts;
        state.cache = Some(Laid {
            key,
            read,
            conversation,
            id,
            layout: layout.clone(),
        });
        layout.map(|l| (l, id))
    }

    /// Draw the colours, while they're shown: a band over each question and passage with a bar
    /// down its left, a bar for each question a passage goes with, and the markers at the
    /// conversation's top and bottom edges, right-aligned. Under a voice draft, a tick on each
    /// question it answers and a red ring round the next it doesn't, or round its marker when
    /// it's off the screen
    pub(crate) fn paint_lbterm_questions(&mut self) -> anyhow::Result<()> {
        self.lbterm_drafts.ticked.clear();
        self.lbterm_drafts.next = None;
        // Nothing shown and no draft anywhere, as nearly always: no pane is looked at
        if self.lbterm_questions.shown.is_none() && !crate::drafts::any() {
            return Ok(());
        }
        // Over the panes drawn: the open tab's, or the ones kept in their place while a held
        // pane's push hasn't come (termwindow/frame_wait.rs)
        let panes = self.lbterm_panes_to_paint();
        // Shown by the held button; else, while a voice draft is open over a pane on screen,
        // there (the active pane's first), each question the draft answers ticked
        // (render/draft.rs)
        let pos = match self.lbterm_questions.shown.as_ref().map(|s| s.pane) {
            Some(pane_id) => panes.iter().find(|p| p.pane.pane_id() == pane_id),
            None => panes
                .iter()
                .filter(|p| draft_for(&p.pane).is_some())
                .max_by_key(|p| p.is_active),
        };
        let Some(pos) = pos else {
            return Ok(());
        };
        let draft = draft_for(&pos.pane);
        let answered = draft.as_ref().map(|d| d.answered()).unwrap_or_default();
        let Some((layout, layout_id)) = self.lbterm_questions_layout(&pos.pane) else {
            return Ok(());
        };
        if layout.is_empty() {
            return Ok(());
        }
        let ticks: Vec<&Asked> = layout
            .asked
            .iter()
            .filter(|a| a.newest && a.number.map_or(false, |n| answered.contains(&n)))
            .collect();
        self.lbterm_drafts.ticked = ticks.iter().filter_map(|a| a.number).collect();
        // A voice draft's next question to answer, ringed in red. Not a picker's: its questions
        // are the picker's, not the reply's
        let next = match &draft {
            Some(d) if !d.is_picker() => layout.next(&answered, self.config.lbterm_next_question_extras),
            _ => None,
        };

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

        // The next question on the screen: a ring just outside its band and its options'
        let next_ring = match &next {
            Some(Next::On(asked)) => {
                let x0 = (left + asked.cols.start as f64 * cell_w - pad).max(left - pad);
                let x1 = (left + asked.cols.end as f64 * cell_w + pad).min(right + pad);
                let out = scale;
                let ring = Area::new(
                    (x0 - out) as f32,
                    (row_y(asked.rows.start) + scale - out) as f32,
                    (x1 - x0 + 2.0 * out) as f32,
                    (asked.rows.len() as f64 * cell_h - 2.0 * scale + 2.0 * out) as f32,
                );
                self.lbterm_drafts.next = Some(said_next(&layout.newest[asked.colour].label, false, ring));
                Some((ring, (4.0 * scale + out) as f32))
            }
            _ => None,
        };
        // Kept while the same layout is drawn in the same place
        let bands_from = (
            layout_id,
            [left, right, row_y(0), cell_w, cell_h, scale].map(f64::to_bits),
            next_ring.map(|(ring, radius)| (crate::termwindow::render::kept_quads::bits(ring), radius.to_bits())),
        );
        self.lbterm_kept_quads(&self.lbterm_questions.bands, BAND_Z, bands_from, |quads| {
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
                self.lbterm_menu_rounded(quads, area, 4.0 * scale, srgb(questions::colour(first), alpha))?;
                for (i, colour) in band.colours.iter().enumerate() {
                    let bar = Rect {
                        x: area.x + i as f64 * (bar_w + scale),
                        y: area.y,
                        w: bar_w,
                        h: area.h,
                    };
                    self.lbterm_menu_rounded(quads, bar, bar_w / 2.0, srgb(questions::colour(*colour), BAR_ALPHA))?;
                }
            }
            if let Some((ring, radius)) = next_ring {
                self.lbterm_next_ring(quads, ring, radius, scale as f32)?;
            }
            Ok(())
        })?;

        // The markers, with one of its own for the next question when none of it is on the screen
        let mut markers = layout.markers.clone();
        let next_label = match &next {
            Some(Next::Off(marker)) => marker.label.clone(),
            _ => String::new(),
        };
        let ringed = match next {
            Some(Next::Off(marker)) => Some(match markers.iter().position(|m| *m == marker) {
                Some(i) => i,
                None => {
                    markers.push(marker);
                    markers.len() - 1
                }
            }),
            _ => None,
        };
        if markers.is_empty() && ticks.is_empty() {
            return Ok(());
        }
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let text_w = font.metrics().cell_width.get();
        let text_h = metrics.cell_size.height as f64;
        let chip_h = text_h + 2.0 * scale;
        let gap = 6.0 * scale;
        let mut chips = vec![];
        let mut ringed_chip = None;
        for up in [true, false] {
            // At the edge of the conversation, clear of Claude Code's input box
            let y = if up {
                row_y(layout.area.start) + 2.0 * scale
            } else {
                row_y(layout.area.end.max(1)) - chip_h - 2.0 * scale
            };
            // Right-aligned, in the questions' order from the left
            let edge: Vec<(String, f64, u32, bool)> = markers
                .iter()
                .enumerate()
                .filter(|(_, m)| m.up == up)
                .map(|(i, m)| {
                    let label = format!("{} {}", m.label, if up { '\u{2191}' } else { '\u{2193}' });
                    let w = label.chars().count() as f64 * text_w + 2.0 * pad;
                    (label, w, questions::colour(m.colour), ringed == Some(i))
                })
                .collect();
            let total: f64 = edge.iter().map(|(_, w, _, _)| w + gap).sum();
            let mut x = right - total;
            for (label, w, colour, ring) in edge {
                let chip = Rect { x, y, w, h: chip_h };
                if ring {
                    ringed_chip = Some(chip);
                }
                chips.push((chip, label, colour));
                x += w + gap;
            }
        }
        // A voice draft's answers: a tick at the right of each question's first row
        for tick in &ticks {
            let label = format!(
                "\u{2713} {}",
                tick.number.map_or(String::new(), |n| format!("Q{n}"))
            );
            let w = label.chars().count() as f64 * text_w + 2.0 * pad;
            let y = row_y(tick.row) + (cell_h - chip_h) / 2.0;
            chips.push((Rect { x: right - w - gap, y, w, h: chip_h }, label, questions::colour(tick.colour)));
        }
        // The next question off the screen: a ring round its marker
        let chip_ring = ringed_chip.map(|chip| {
            let out = 2.0 * scale;
            let ring = Area::new(
                (chip.x - out) as f32,
                (chip.y - out) as f32,
                (chip.w + 2.0 * out) as f32,
                (chip.h + 2.0 * out) as f32,
            );
            self.lbterm_drafts.next = Some(said_next(&next_label, true, ring));
            ring
        });
        let chips_from = (
            chips
                .iter()
                .map(|(chip, _, colour)| ([chip.x, chip.y, chip.w, chip.h].map(f64::to_bits), *colour))
                .collect::<Vec<_>>(),
            chip_ring.map(crate::termwindow::render::kept_quads::bits),
            scale.to_bits(),
        );
        self.lbterm_kept_quads(&self.lbterm_questions.chips, CHIP_Z, chips_from, |quads| {
            for (chip, _, colour) in &chips {
                self.lbterm_menu_rounded(quads, *chip, chip.h / 2.0, srgb(*colour, 0.95))?;
            }
            if let Some(ring) = chip_ring {
                // Just under half its height: a radius of half leaves no straight side to stroke
                self.lbterm_next_ring(quads, ring, ring.h / 2.0 - 1.0, scale as f32)?;
            }
            Ok(())
        })?;
        let words: Vec<Word> = chips
            .iter()
            .map(|(chip, label, _)| {
                let bounds = euclid::rect(
                    (chip.x + pad) as f32,
                    (chip.y + (chip.h - text_h) / 2.0) as f32,
                    (chip.w - pad) as f32,
                    text_h as f32,
                );
                (label.clone(), bounds, srgb(MARKER_TEXT, 1.0))
            })
            .collect();
        let cell = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_questions.words, &font, &metrics, cell, TEXT_Z, words)
    }

    /// The red ring round the next question to answer, two pixels wide at 96 dpi
    fn lbterm_next_ring(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        ring: Area,
        radius: f32,
        scale: f32,
    ) -> anyhow::Result<()> {
        let stroke = (2.0 * scale).round().max(2.0);
        let feather = scale.round().max(1.0);
        self.lbterm_stroke(quads, ring, radius, stroke, feather, |_| NEXT_RING)
    }
}

/// LBTerm: the question colours, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// The pane whose question colours show
    pub(crate) fn lbterm_state_questions(&self) -> Option<u64> {
        self.lbterm_questions.shown.as_ref().map(|s| s.pane as u64)
    }
}
