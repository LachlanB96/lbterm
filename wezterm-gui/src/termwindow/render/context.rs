//! LBTerm: the context pill (`config.lbterm_context`; lbterm/CONTEXT.md). Lachlan's ask,
//! 2026-11-07: "LBTerm should be able to show me what context files this agent has loaded
//! currently. There should also be a $ amount of how much this agents tab has cost. It should go
//! under the inmail button."
//!
//! A hairline pill on the strip's second row, centred on the tab rail's column under the InMail
//! button, saying what the open tab's Claude conversation has cost at list price: every run of it
//! and its subagents, as Terminal's statusline writes it to `<session>.usage.json` (read by
//! crate::summaries, off this thread). A click drops a list from it, in the InMail list's slot
//! (render/menu_fade.rs, `Slot::List`: its z-layers 77 to 79, its card fading in and out and its
//! light and hairline every menu's; only one of the lists in it is ever open), and while it's
//! open it has every key, a key it doesn't scroll with flaring its card's edge (`menu::bump`): a
//! heading with how many files are in the conversation's context, the tokens they take and their
//! share of its window (Lachlan's E5), the dollars again with what they count, and each file
//! Claude Code loaded since its last compaction, with how it came to be loaded and its size. A
//! CLAUDE.md pulled in from another project is in amber (Lachlan's E4). The files are read from
//! the transcript by crate::conversations, the scroll strip's reader, which follows the session
//! while the list is open (`Reader::Context`). Esc, a click elsewhere or the pill again closes it;
//! the wheel and the arrow keys scroll it.
//!
//! Nothing here reads a file: the paint and the list read memory.

use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::Element;
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, RectF, WindowOps};
use config::SrgbaTuple;
use lbterm_extras::context::{self, Context};
use lbterm_extras::conversation::Snapshot;
use lbterm_extras::prompts;
use lbterm_extras::summaries::Usage;
use lbterm_extras::tabs::Rect;
use super::menu_fade::Slot;
use lbterm_panel::palette;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wezterm_font::LoadedFont;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// The list's words: its slot's (render/menu_fade.rs, `Slot::List`), whose card
/// [`TermWindow::lbterm_menu_card`] draws
const TEXT_Z: i8 = Slot::List.text_z();

/// On the pill before the statusline has said what the conversation cost
pub const UNKNOWN: &str = "$–";
/// The list's line while the transcript hasn't been read yet
const READING: &str = "Reading the conversation…";

#[derive(Default)]
pub struct ContextState {
    /// What the pill was last built showing
    shown: Option<Shown>,
    list: Option<OpenList>,
    /// The list as last laid out, and what from: laid out again only when that changes
    laid: RefCell<Option<(ListAt, Rc<Laid>)>>,
    /// The list's words as its last frame laid them out (render/words.rs)
    words: RefCell<super::words::Words>,
}

impl ContextState {
    /// The list's words point into a glyph atlas that's gone (gpu_lost.rs)
    pub(crate) fn forget_words(&mut self) {
        self.words.get_mut().forget();
    }
}

/// What the pill shows: whose conversation, its words, and whether its list is open
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    sid: String,
    text: String,
    open: bool,
}

/// The list while it's open: how far down it's scrolled
struct OpenList {
    scroll: usize,
}

/// One line of the list: its first column (how a file came to be loaded), its words, and its
/// right end (a file's tokens), each with its colour
struct Line {
    first: String,
    words: String,
    right: String,
    colour: u32,
    /// A file pulled in from another project (Lachlan's E4): its words in amber
    elsewhere: bool,
}

/// The list laid out: where it goes and its rows, a column's width, and the lines, top first
struct Laid {
    list: prompts::List,
    cell_w: f64,
    lines: Vec<Line>,
}

/// What the list was laid out from: the context (shared by the reader's snapshots, so a new one
/// is a new Arc), what the conversation has cost, the scroll, the fonts and where it goes
struct ListAt {
    context: Option<Arc<Context>>,
    usage: Option<(u64, Option<u64>)>,
    scroll: usize,
    shapes: usize,
    geometry: [u64; 7],
}

impl PartialEq for ListAt {
    fn eq(&self, o: &Self) -> bool {
        match (&self.context, &o.context) {
            (Some(a), Some(b)) if !Arc::ptr_eq(a, b) => return false,
            (Some(_), None) | (None, Some(_)) => return false,
            _ => {}
        }
        (self.usage, self.scroll, self.shapes, self.geometry) == (o.usage, o.scroll, o.shapes, o.geometry)
    }
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The list's lines, top first: the heading, what it cost, then each file
fn lines(context: Option<&Context>, usage: Option<Usage>, home: &str) -> Vec<Line> {
    let line = |words: String, colour: u32| Line {
        first: String::new(),
        words,
        right: String::new(),
        colour,
        elsewhere: false,
    };
    let mut out = vec![line(
        match context {
            Some(c) => context::heading(c, usage.and_then(|u| u.window)),
            None => READING.to_string(),
        },
        palette::TITLE,
    )];
    out.push(line(
        match usage {
            Some(u) => format!("{} at list price: every run of it and its subagents", lbterm_panel::text::dollars(u.usd)),
            None => "What it has cost isn't known yet".to_string(),
        },
        palette::PROJECT,
    ));
    for f in context.map(|c| c.files.as_slice()).unwrap_or_default() {
        out.push(Line {
            first: f.kind.word().to_string(),
            words: context::short_path(&f.path, home),
            right: format!("≈{}", context::count(f.tokens)),
            colour: palette::LABEL,
            elsewhere: f.elsewhere,
        });
    }
    out
}

impl TermWindow {
    /// The pill shows: asked for in the config, the strip holding only the window's buttons beside
    /// the tab panel with its second row, and not while the Conductor's view stands over the tabs
    pub(crate) fn lbterm_context_on(&self) -> bool {
        self.config.lbterm_context.is_some()
            && self.config.lbterm_tab_panel.enabled
            && config::lbterm::hides_tabs(&self.config)
            && self.config.use_fancy_tab_bar
            && self.show_tab_bar
            && !self.config.tab_bar_at_bottom
            && self.lbterm_prompt_row_on()
            && !self.lbterm_view_is_open()
    }

    /// Each paint, after the strip has found its tab's session (render/strip.rs): true when the
    /// strip must be built again, for other dollars on the pill or its list opening or closing.
    /// The list closes when the strip stands for another session, and the conversations' reader
    /// follows the list's session only while it's open.
    pub(crate) fn lbterm_context_refresh(&mut self) -> bool {
        let sid = self.lbterm_strip.session.clone().filter(|_| self.lbterm_context_on());
        if sid.as_deref() != self.lbterm_context.shown.as_ref().map(|s| s.sid.as_str()) {
            self.lbterm_context.list = None;
        }
        let shown = sid.map(|sid| {
            let usage = crate::summaries::get(&sid).and_then(|s| s.usage);
            Shown {
                text: usage.map_or_else(|| UNKNOWN.to_string(), |u| lbterm_panel::text::dollars(u.usd)),
                open: self.lbterm_context.list.is_some(),
                sid,
            }
        });
        crate::conversations::shows(
            self.mux_window_id,
            crate::conversations::Reader::Context,
            shown.as_ref().filter(|s| s.open).map(|s| s.sid.as_str()),
        );
        if shown == self.lbterm_context.shown {
            return false;
        }
        self.lbterm_context.shown = shown;
        true
    }

    /// Whether this window shows what one of `sids` has cost: the pill's session is one, or the
    /// open tab panel has a row for one (crate::summaries repaints only such windows when a
    /// session's cost alone changes)
    pub(crate) fn lbterm_shows_usage(&self, sids: &[String]) -> bool {
        if self.lbterm_context.shown.as_ref().map_or(false, |s| sids.contains(&s.sid)) {
            return true;
        }
        if !self.lbterm_panel.hover.is_open() {
            return false;
        }
        mux::Mux::try_get()
            .and_then(|mux| mux.get_window(self.mux_window_id).map(|w| w.iter_tabs().cloned().collect::<Vec<_>>()))
            .map_or(false, |tabs| {
                tabs.iter()
                    .any(|tab| super::tab_panel::tab_session(tab).map_or(false, |sid| sids.contains(&sid)))
            })
    }

    /// The pill for the strip's second row, `row` the strip's bounds, and how wide it is laid out
    /// alone: what the conversation has cost in a hairline pill, which the strip places centred on
    /// the rail's column (render/strip.rs, `lbterm_prompt_row`). Nothing while it doesn't show.
    pub(crate) fn lbterm_context_pill(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        row: RectF,
    ) -> anyhow::Result<Option<(Element, f32)>> {
        let Some(shown) = self.lbterm_context.shown.as_ref() else {
            return Ok(None);
        };
        let over = |alpha: f32| [surface.0, surface.1, surface.2].map(|c| c + (1. - c) * alpha);
        // It sits back as the InMail button does with nothing waiting, its words a little
        // brighter, since they're always something to read; lit under the pointer or open
        let (rest, lit) = if shown.open {
            ((over(0.50), over(1.0)), (over(0.50), over(1.0)))
        } else {
            ((over(0.14), over(0.70)), (over(0.50), over(1.0)))
        };
        let kids = vec![super::inbox::pill_text(font, shown.text.clone(), rest.1, lit.1)];
        self.lbterm_strip_pill(font, metrics, surface, row, TabBarItem::LbtermContext, kids, (rest.0, lit.0), (rest.1, lit.1))
            .map(Some)
    }

    pub(crate) fn lbterm_context_list_is_open(&self) -> bool {
        self.lbterm_context.list.is_some()
    }

    fn lbterm_context_list_close(&mut self) {
        if self.lbterm_context.list.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// A click on the pill: open the list, or close it
    pub(crate) fn lbterm_context_toggle(&mut self) {
        if self.lbterm_context.list.is_some() {
            self.lbterm_context_list_close();
            return;
        }
        if self.lbterm_context.shown.is_none() {
            return;
        }
        self.lbterm_context.list = Some(OpenList { scroll: 0 });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// The pill's session's conversation as read so far, and what it has cost
    fn lbterm_context_now(&self) -> Option<(Option<Arc<Snapshot>>, Option<Usage>)> {
        let sid = &self.lbterm_context.shown.as_ref()?.sid;
        let snapshot = crate::conversations::get(sid);
        let usage = crate::summaries::get(sid).and_then(|s| s.usage);
        Some((snapshot, usage))
    }

    /// Where the list goes: under the strip, its card hanging from the pill's left as the InMail
    /// list's does from its button, a column's width, and its lines. Laid out again only when the
    /// context, the cost, the scroll, the fonts or where it goes have changed
    fn lbterm_context_list_layout(&self) -> anyhow::Result<Option<Rc<Laid>>> {
        let Some(list) = self.lbterm_context.list.as_ref() else {
            return Ok(None);
        };
        let Some((snapshot, usage)) = self.lbterm_context_now() else {
            return Ok(None);
        };
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let scale = self.dimensions.dpi as f64 / 96.0;
        let cell_w = font.metrics().cell_width.get();
        let row_h = (metrics.cell_size.height as f64 * 1.6).round();
        let border = self.get_os_border();
        let top = border.top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
        let window = (self.dimensions.pixel_width as f64, self.dimensions.pixel_height as f64);
        let column = super::tab_panel::rail_column(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let edge = border.left.get() as f64
            - super::tab_panel::rail_width(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let left = edge.max(0.0) + (column - 2.0 * cell_w).max(0.0) + 12.0 * scale;
        let context = snapshot.as_ref().map(|s| Arc::clone(&s.context));
        let at = ListAt {
            context: context.clone(),
            usage: usage.map(|u| (u.usd.to_bits(), u.window)),
            scroll: list.scroll,
            shapes: self.shape_generation,
            geometry: [left, top, window.0, window.1, row_h, cell_w, scale].map(f64::to_bits),
        };
        if let Some((was, laid)) = self.lbterm_context.laid.borrow().as_ref() {
            if *was == at {
                return Ok(Some(Rc::clone(laid)));
            }
        }

        let home = config::HOME_DIR.to_string_lossy();
        let lines = lines(context.as_deref(), usage, &home);
        // How each file came to be loaded, in a column as wide as the widest word; its path,
        // then its tokens at the end
        let first_cols = lines.iter().map(|l| l.first.chars().count()).max().unwrap_or(0);
        let first_w = (first_cols as f64 + 2.0) * cell_w;
        let text_cols = lines
            .iter()
            .map(|l| {
                if l.first.is_empty() {
                    l.words.chars().count().saturating_sub(first_cols + 2)
                } else {
                    l.words.chars().count() + l.right.chars().count() + 3
                }
            })
            .max()
            .unwrap_or(0)
            .clamp(40, 96);
        let width = 24.0 * scale + first_w + text_cols as f64 * cell_w;
        // prompts::list puts its last index at the top: line 0 is the heading
        let count = lines.len();
        let Some(list) = prompts::list(count, left, top, window, (row_h, first_w, width), list.scroll, scale) else {
            *self.lbterm_context.laid.borrow_mut() = None;
            return Ok(None);
        };
        let laid = Rc::new(Laid { list, cell_w, lines });
        *self.lbterm_context.laid.borrow_mut() = Some((at, Rc::clone(&laid)));
        Ok(Some(laid))
    }

    /// While the list's open it takes every mouse event: true when it did. Its rows are to read,
    /// not to press: a click on the card does nothing, elsewhere closes it
    pub(crate) fn lbterm_context_list_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if self.lbterm_context.list.is_none() {
            return false;
        }
        let Ok(Some(laid)) = self.lbterm_context_list_layout() else {
            self.lbterm_context_list_close();
            return false;
        };
        let card = laid.list.card;
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        match event.kind {
            WMEK::Move => context.set_cursor(Some(CursorIcon::Default)),
            // The pill among them: a second click closes it
            WMEK::Press(_) if !card.contains(x, y) => self.lbterm_context_list_close(),
            WMEK::VertWheel(n) => {
                if let Some(list) = self.lbterm_context.list.as_mut() {
                    list.scroll = if n > 0 { list.scroll.saturating_sub(3) } else { list.scroll + 3 };
                    context.invalidate();
                }
            }
            _ => {}
        }
        true
    }

    /// While the list's open it takes every key: Esc closes it, the arrows, Page Up and Down,
    /// Home and End scroll it, and any other flares its card's edge (`menu::bump`; its rows are to read, so
    /// Enter is one of those). True when it did.
    pub(crate) fn lbterm_context_list_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        if self.lbterm_context.list.is_none() {
            return false;
        }
        if *key == KeyCode::Escape {
            self.lbterm_context_list_close();
            return true;
        }
        let (count, shown) = self
            .lbterm_context_list_layout()
            .ok()
            .flatten()
            .map_or((1, 1), |l| (l.lines.len(), l.list.rows.len().max(1)));
        let most = count.saturating_sub(shown);
        if let Some(list) = self.lbterm_context.list.as_mut() {
            let to = match key {
                KeyCode::UpArrow => list.scroll.saturating_sub(1),
                KeyCode::DownArrow => (list.scroll + 1).min(most),
                KeyCode::PageUp => list.scroll.saturating_sub(shown),
                KeyCode::PageDown => (list.scroll + shown).min(most),
                KeyCode::Home => 0,
                KeyCode::End => most,
                _ => {
                    self.lbterm_menu_stray(Slot::List, key, mods);
                    return true;
                }
            };
            if to != list.scroll {
                list.scroll = to;
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
            }
        }
        true
    }

    /// Draw the list, if it's open: the tab menu's card, a line a row
    pub(crate) fn paint_lbterm_context_list(&mut self) -> anyhow::Result<()> {
        if self.lbterm_context.list.is_none() {
            return Ok(());
        }
        let Some(laid) = self.lbterm_context_list_layout()? else {
            return Ok(());
        };
        let (layout, cell_w) = (&laid.list, laid.cell_w);
        // Scrolled past the end: held at the last
        if let Some(list) = self.lbterm_context.list.as_mut() {
            list.scroll = layout.scroll;
        }
        let fill = self.lbterm_menu_fill();
        let scale = self.dimensions.dpi as f64 / 96.0;

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_h = metrics.cell_size.height as f64;
        let mut words: Vec<super::words::Word> = vec![];
        let mut place = |text: &str, x: f64, y: f64, w: f64, colour: u32| {
            if !text.is_empty() && w > 0.0 {
                words.push((text.to_string(), euclid::rect(x as f32, y as f32, w as f32, line_h as f32), srgb(colour, 1.0)));
            }
        };
        let count = laid.lines.len();
        for (i, r) in &layout.rows {
            let Some(line) = laid.lines.get(count - 1 - i) else {
                continue;
            };
            let y = r.y + (r.h - line_h) / 2.0;
            let first_x = layout.card.x + 12.0 * scale;
            let right = r.x + r.w - 10.0 * scale;
            let cols = |from: f64, to: f64| ((to - from) / cell_w).floor().max(0.0) as usize;
            if line.first.is_empty() {
                // The heading and the cost run the card's width
                place(&lbterm_extras::strip::elide(&line.words, cols(first_x, right)), first_x, y, right - first_x, line.colour);
                continue;
            }
            let (first, path) = if line.elsewhere { (palette::AMBER, palette::AMBER) } else { (palette::PROJECT, line.colour) };
            place(&line.first, first_x, y, layout.text_x - first_x, first);
            let right_w = line.right.chars().count() as f64 * cell_w;
            place(&line.right, right - right_w, y, right_w + cell_w, palette::PROJECT);
            let path_end = right - right_w - cell_w;
            place(&context::fit_path(&line.words, cols(layout.text_x, path_end)), layout.text_x, y, path_end - layout.text_x, path);
        }

        // The light round it, its ring and card, fading in and out: every menu's
        // (render/menu_fade.rs). No row is lit: its rows are to read
        self.lbterm_menu_card(Slot::List, layout.card, fill)?;
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_context.words, &font, &metrics, room, TEXT_Z, words)
    }
}

/// LBTerm: the context pill and its list, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// What the pill says, while it shows
    pub(crate) fn lbterm_state_context(&self) -> Option<String> {
        self.lbterm_context.shown.as_ref().map(|s| s.text.clone())
    }

    /// The list while it's open: its lines top first, a file's as "<kind> · <path> · ≈<tokens>",
    /// `checked` for one pulled in from another project (drawn in amber)
    pub(crate) fn lbterm_state_context_list(&self) -> Option<lbterm_extras::state::Menu> {
        self.lbterm_context.list.as_ref()?;
        let laid = self.lbterm_context_list_layout().ok().flatten()?;
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        let count = laid.lines.len();
        let items = laid
            .lines
            .iter()
            .enumerate()
            .map(|(d, line)| lbterm_extras::state::MenuItem {
                label: if line.first.is_empty() {
                    line.words.clone()
                } else {
                    format!("{} · {} · {}", line.first, line.words, line.right)
                },
                checked: line.elsewhere,
                key: None,
                detail: None,
                rect: laid.list.rows.iter().find(|(i, _)| count - 1 - *i == d).map(|(_, r)| rect(r)),
                inert: false,
            })
            .collect();
        Some(lbterm_extras::state::Menu {
            kind: "context".to_string(),
            tab: None,
            rect: Some(rect(&laid.list.card)),
            items,
            focus: None,
            renaming: None,
            waiting: None,
        })
    }

    /// What of the open list is at `x`, `y`, top first: `Some(Some(line))` on a line, `Some(None)`
    /// on the card elsewhere, `None` off it
    pub(crate) fn lbterm_context_list_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        self.lbterm_context.list.as_ref()?;
        let laid = self.lbterm_context_list_layout().ok()??;
        if !laid.list.card.contains(x, y) {
            return None;
        }
        let count = laid.lines.len();
        Some(laid.list.hit(x, y).filter(|&i| i < count).map(|i| count - 1 - i))
    }
}
