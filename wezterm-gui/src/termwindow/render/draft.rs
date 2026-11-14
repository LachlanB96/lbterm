//! LBTerm: the voice drafts' cards (lbterm/CONTROL.md, "Drafts"; the drafts are crate::drafts',
//! the logic lbterm_extras::drafts). Over each pane with a draft, just above Claude Code's input
//! box (or what has its place: a permission prompt, a menu), a card shows the draft's lines as
//! they'll be typed, each answer with a bar in its question's colour, under a line saying what to
//! say to it. While a draft's open the pane's question colours show too, and the questions it
//! answers are ticked (render/questions.rs).
//!
//! Drawn in z-layers of its own in the compositor's `over` visual, over the panes' text. The card
//! keeps its newest lines when the pane hasn't room for them all, and isn't drawn in a pane too
//! short for two. Its lines are made again only when the draft or the questions change, where to
//! put it only when the pane's screen does (it's read again only then: render/questions.rs), and
//! its shapes and words are kept while they're the same (render/kept_quads.rs, render/words.rs):
//! each frame a draft showed read its pane's whole screen, looked its questions' colours up and
//! laid its words out again (lbterm/PERF.md, finding 63).

use crate::termwindow::render::kept_quads::KeptQuads;
use crate::termwindow::render::tab_panel::server_pane_id;
use crate::termwindow::render::words::{Word, Words};
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use config::SrgbaTuple;
use lbterm_extras::drafts::{self, Draft};
use lbterm_extras::questions;
use lbterm_extras::strip;
use lbterm_extras::tabs::Rect;
use lbterm_panel::palette;
use std::cell::RefCell;
use std::rc::Rc;
use window::color::LinearRgba;

/// The card's shadow, the card and its bars, its words: in `over`
const SHADOW_Z: i8 = 85;
const CARD_Z: i8 = 86;
const TEXT_Z: i8 = 87;
/// White over the card, as the eye sees it: the strip card's way
const HEAD_ALPHA: f32 = 0.50;
const TEXT_ALPHA: f32 = 0.92;
const RING_ALPHA: f32 = 0.14;
/// An answer's bar when its question has no colour (the question worker hasn't found it)
const BAR_PLAIN: u32 = 0x8A8AA0;
const BAR_ALPHA: f64 = 0.9;
/// What the card says over the draft
const HEAD_TEXT: &str = "Voice draft \u{b7} say \u{201c}send\u{201d}, \u{201c}scratch that\u{201d} or \u{201c}edit\u{201d}";

/// A card as last drawn, for the state file
#[derive(Debug, Clone)]
pub struct Drawn {
    /// The pane, by the mux server's id
    pub pane: u64,
    pub rect: Rect,
    pub text: String,
}

#[derive(Default)]
pub struct DraftsState {
    pub drawn: Vec<Drawn>,
    /// The questions ticked on the pane whose colours show, by number (render/questions.rs)
    pub ticked: Vec<u32>,
    /// The ring round the next question to answer there (render/questions.rs)
    pub next: Option<lbterm_extras::state::NextQuestion>,
    /// What each card showing was laid out from
    kept: Vec<KeptCard>,
    /// The cards' shadows, the cards and their bars, and their words, kept while the same
    shadows: RefCell<KeptQuads>,
    cards: RefCell<KeptQuads>,
    pub(crate) words: RefCell<Words>,
}

/// A card's line: its text, its bar's colour, and how white its words are over the card
type Row = (String, Option<u32>, f32);

/// What a draft's card was last laid out from: the draft and the questions' generation its lines
/// were made from, and the read of its pane's screen the box was found in
struct KeptCard {
    draft: Draft,
    generation: Option<u64>,
    rows: Rc<Vec<Row>>,
    read: u64,
    anchor: usize,
}

/// A card's lines: the head, and the draft's lines with their questions' colours
fn card_rows(draft: &Draft) -> Vec<Row> {
    let session = draft.session.as_deref();
    let mut rows: Vec<Row> = vec![(HEAD_TEXT.to_string(), None, HEAD_ALPHA)];
    for line in draft.lines() {
        let bar = line
            .question
            .map(|n| question_colour(session, n).unwrap_or(BAR_PLAIN));
        rows.push((line.text, bar, TEXT_ALPHA));
    }
    rows
}

fn over(surface: SrgbaTuple, alpha: f32) -> LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `text` cut to `chars`, an ellipsis showing where
fn fit(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(chars.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// The colour of question `number` in the session's newest reply that asked something
pub(crate) fn question_colour(session: Option<&str>, number: u32) -> Option<u32> {
    let map = crate::summaries::get(session?)?.questions.clone()?;
    let message = map.messages.last()?;
    let index = message.questions.iter().position(|q| q.number() == Some(number))?;
    Some(questions::colour(index))
}

/// The draft over `pane`, if it's still for the session in it
pub(crate) fn draft_for(pane: &std::sync::Arc<dyn mux::pane::Pane>) -> Option<Draft> {
    let draft = crate::drafts::get(server_pane_id(pane))?;
    let session = pane
        .copy_user_vars()
        .remove("claude_session")
        .filter(|s| !s.is_empty());
    (draft.session == session).then_some(draft)
}

impl TermWindow {
    /// The drafts' cards as last drawn, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_state_drafts(&self) -> Vec<lbterm_extras::state::DraftCard> {
        self.lbterm_drafts
            .drawn
            .iter()
            .map(|d| lbterm_extras::state::DraftCard {
                pane: d.pane,
                text: d.text.clone(),
                rect: lbterm_extras::state::Rect::round(d.rect.x, d.rect.y, d.rect.w, d.rect.h),
            })
            .collect()
    }

    pub(crate) fn paint_lbterm_drafts(&mut self) -> anyhow::Result<()> {
        self.lbterm_drafts.drawn.clear();
        if !crate::drafts::any() {
            self.lbterm_drafts.kept.clear();
            return Ok(());
        }
        // Over the panes drawn (termwindow/frame_wait.rs)
        let panes = self.lbterm_panes_to_paint();
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_h = metrics.cell_size.height as f64;
        let char_w = font.metrics().cell_width.get();
        let scale = self.dimensions.dpi as f64 / 96.0;
        let (padding_left, padding_top) = self.padding_left_top();
        let border = self.get_os_border();
        let top_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let cell_w = self.render_metrics.cell_size.width as f64;
        let cell_h = self.render_metrics.cell_size.height as f64;
        let surface = self.palette().background;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let pad = 8.0 * scale;
        let bar_w = (3.0 * scale).max(2.0);
        let bar_gap = 8.0 * scale;
        let generation = crate::summaries::store().map(|s| s.generation());

        // Each card's place and its bars, and every card's words, for drawing all at once
        let mut cards: Vec<(Rect, Vec<(Rect, u32)>)> = vec![];
        let mut words: Vec<Word> = vec![];
        let mut drafted = vec![];
        for pos in &panes {
            let Some(draft) = draft_for(&pos.pane) else {
                continue;
            };
            drafted.push(draft.pane);
            // Where the box is, found again only when the pane's screen is read again (it's read
            // only when the pane has changed), and the card's lines only when the draft or the
            // questions do
            let (screen, read) = self.lbterm_screen(&pos.pane);
            let kept = &mut self.lbterm_drafts.kept;
            let at = match kept.iter().position(|k| k.draft.pane == draft.pane) {
                Some(at) => at,
                None => {
                    kept.push(KeptCard {
                        rows: Rc::new(card_rows(&draft)),
                        draft: draft.clone(),
                        generation,
                        read,
                        anchor: drafts::anchor_row(&screen),
                    });
                    kept.len() - 1
                }
            };
            let card_kept = &mut kept[at];
            if card_kept.read != read {
                card_kept.anchor = drafts::anchor_row(&screen);
                card_kept.read = read;
            }
            if card_kept.draft != draft || card_kept.generation != generation {
                card_kept.rows = Rc::new(card_rows(&draft));
                card_kept.draft = draft.clone();
                card_kept.generation = generation;
            }
            let anchor = card_kept.anchor.min(pos.height);
            let all_rows = Rc::clone(&card_kept.rows);

            let row_y = |row: usize| {
                top_bar as f64 + padding_top as f64 + border.top.get() as f64 + (pos.top + row) as f64 * cell_h
            };
            let left = border.left.get() as f64 + padding_left as f64 + pos.left as f64 * cell_w;
            let pane_w = pos.width as f64 * cell_w;

            // As many as fit between the pane's top and the box, the newest kept
            let bottom = row_y(anchor) - 4.0 * scale;
            let room = ((bottom - row_y(0) - 2.0 * pad) / line_h).floor().max(0.0) as usize;
            if room < 2 {
                continue;
            }
            let rows: Vec<&Row> = if all_rows.len() > room {
                let cut = all_rows.len() - (room - 1);
                std::iter::once(&all_rows[0]).chain(all_rows[cut..].iter()).collect()
            } else {
                all_rows.iter().collect()
            };
            let text_x = pad + bar_w + bar_gap;
            let max_w = (pane_w - 2.0 * cell_w).max(0.0);
            let max_chars = (((max_w - text_x - pad) / char_w).floor().max(1.0)) as usize;
            let longest = rows.iter().map(|(t, _, _)| t.chars().count()).max().unwrap_or(0).min(max_chars);
            let w = (text_x + longest as f64 * char_w + pad).min(max_w);
            let h = rows.len() as f64 * line_h + 2.0 * pad;
            let card = Rect {
                x: left + cell_w,
                y: bottom - h,
                w,
                h,
            };
            let mut bars = vec![];
            for (i, (text, bar, alpha)) in rows.iter().enumerate() {
                if let Some(colour) = bar {
                    let bar_rect = Rect {
                        x: card.x + pad,
                        y: card.y + pad + i as f64 * line_h + 2.0 * scale,
                        w: bar_w,
                        h: line_h - 4.0 * scale,
                    };
                    bars.push((bar_rect, *colour));
                }
                if text.is_empty() {
                    continue;
                }
                let x = if i == 0 { card.x + pad } else { card.x + text_x };
                let bounds = euclid::rect(
                    x as f32,
                    (card.y + pad + i as f64 * line_h) as f32,
                    (card.x + card.w - x - pad).max(0.0) as f32,
                    line_h as f32,
                );
                words.push((fit(text, max_chars), bounds, over(surface, *alpha)));
            }
            cards.push((card, bars));
            self.lbterm_drafts.drawn.push(Drawn {
                pane: draft.pane,
                rect: card,
                text: draft.text(),
            });
        }
        self.lbterm_drafts.kept.retain(|k| drafted.contains(&k.draft.pane));

        // The cards' shadows and the cards with their bars, kept while they're the same cards in
        // the same places
        let grow = |r: Rect, by: f64, down: f64| Rect {
            x: r.x - by,
            y: r.y - by + down,
            w: r.w + 2.0 * by,
            h: r.h + 2.0 * by,
        };
        let rect_bits = |r: &Rect| [r.x, r.y, r.w, r.h].map(f64::to_bits);
        let places: Vec<[u64; 4]> = cards.iter().map(|(card, _)| rect_bits(card)).collect();
        let (reach, down) = strip::shadow_reach(scale);
        self.lbterm_kept_quads(
            &self.lbterm_drafts.shadows,
            SHADOW_Z,
            (&places, [radius, reach, down].map(f64::to_bits)),
            |quads| {
                for (card, _) in &cards {
                    for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                        let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                        let shadow = LinearRgba::with_components(0., 0., 0., *alpha);
                        self.lbterm_menu_rounded(quads, grow(*card, by, down), radius + by, shadow)?;
                    }
                }
                Ok(())
            },
        )?;
        let looks: Vec<([u64; 4], Vec<([u64; 4], u32)>)> = cards
            .iter()
            .map(|(card, bars)| (rect_bits(card), bars.iter().map(|(r, c)| (rect_bits(r), *c)).collect()))
            .collect();
        let surface_bits = [surface.0, surface.1, surface.2, surface.3].map(f32::to_bits);
        self.lbterm_kept_quads(
            &self.lbterm_drafts.cards,
            CARD_Z,
            (&looks, surface_bits, [radius, stroke, bar_w].map(f64::to_bits)),
            |quads| {
                for (card, bars) in &cards {
                    self.lbterm_menu_rounded(quads, grow(*card, stroke, 0.), radius + stroke, over(surface, RING_ALPHA))?;
                    self.lbterm_menu_rounded(quads, *card, radius, surface.to_linear())?;
                    for (bar, colour) in bars {
                        self.lbterm_menu_rounded(quads, *bar, bar_w / 2.0, srgb(*colour, BAR_ALPHA))?;
                    }
                }
                Ok(())
            },
        )?;
        let cell = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_drafts.words, &font, &metrics, cell, TEXT_Z, words)
    }
}
