//! LBTerm: the strip's last prompt and the list of them (lbterm/DESIGN.md, "The window strip");
//! the logic is lbterm_extras::prompts. With `lbterm_prompt_history` set, the strip's second row
//! (render/strip.rs) shows the active Claude pane's last prompt, claudehook.exe's `claude_prompt`
//! user var. A click on it jumps Claude Code's conversation to it. A click on the arrow before it
//! drops a list of every prompt the session sent, newest first, from Claude Code's
//! history.jsonl, read on a thread of its own; a click on one, or the arrow keys and Enter,
//! jumps to it.
//!
//! Claude Code's fullscreen renderer keeps the conversation to itself, so the jump asks it to
//! scroll, with the keys it knows: Ctrl+End, then PgUp until the prompt is on screen or in the
//! header Claude Code keeps at the top for the last prompt scrolled out of view, which it then
//! clicks (`prompts::Jump`). It runs between frames on the GUI thread's executor, a key and a
//! look at a time, and marks the row it lands on for a moment.
//!
//! The list is drawn in z-layers 63 to 65, beside the tab menu's in the compositor's `top`
//! visual, over the tab panel; the mark in z-layer 26, in `over`.

use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::{TermWindow, TermWindowNotif};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::DimensionContext;
use lbterm_extras::prompts::{self, Found, Jump, Row, Sent, Step};
use lbterm_extras::strip;
use lbterm_extras::tabs::Rect;
use lbterm_panel::palette;
use mux::pane::{Pane, PaneId};
use mux::tab::Tab;
use mux::Mux;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use wezterm_term::color::ColorAttribute;
use wezterm_term::{
    KeyCode, KeyModifiers, MouseButton, MouseEvent as TermMouseEvent,
    MouseEventKind as TermMouseKind, StableRowIndex,
};
use window::color::LinearRgba;

/// The list's light, card and words: in the tab menu's range, so the compositor's `top`
const GLOW_Z: i8 = 63;
const CARD_Z: i8 = 64;
const TEXT_Z: i8 = 65;
/// The mark on the row a jump lands on, in `over`
const MARK_Z: i8 = 26;

/// How long the mark and a note stay
const MARK_FOR: Duration = Duration::from_millis(1600);
const NOTE_FOR: Duration = Duration::from_secs(5);

/// How often a jump looks at the screen while it waits for Claude Code to draw, how long the
/// screen must hold still to count as drawn, and the longest it waits
const POLL: Duration = Duration::from_millis(25);
const QUIET: Duration = Duration::from_millis(80);
const MAX_WAIT: Duration = Duration::from_millis(1200);

/// The Claude pane the strip speaks for in a tab, and what it says.
pub struct Target {
    pub pane: Arc<dyn Pane>,
    pub sid: String,
    /// Its last prompt on one line, if it has sent one
    pub prompt: Option<String>,
}

/// The Claude pane the strip speaks for in `tab`: the active pane, else the first in the tab
/// that runs Claude Code (claudehook.exe's `claude_session` user var)
pub fn target(tab: &Tab) -> Option<Target> {
    let active = tab.get_active_pane();
    let others = tab.iter_panes_ignoring_zoom().into_iter().map(|pos| pos.pane);
    active.into_iter().chain(others).find_map(|pane| {
        let mut vars = pane.copy_user_vars();
        let sid = vars.remove("claude_session").filter(|sid| !sid.is_empty())?;
        let prompt = vars
            .remove("claude_prompt")
            .map(|p| prompts::one_line(&p))
            .filter(|p| !p.is_empty());
        Some(Target { pane, sid, prompt })
    })
}

#[derive(Default)]
pub struct PromptState {
    list: Option<OpenList>,
    /// A jump under way
    jumping: bool,
    mark: Option<Mark>,
    /// Why the last jump didn't get there, until when
    note: Option<(String, Instant)>,
}

/// The list while it's open
struct OpenList {
    /// The pane its prompts were sent in
    pane: PaneId,
    /// Oldest first; none while history.jsonl is being read
    prompts: Option<Vec<Sent>>,
    /// The prompt under the pointer, or where the arrow keys put it
    focus: Option<usize>,
    /// Rows scrolled down from the newest
    scroll: usize,
}

/// The row a jump landed on, marked for a moment: a prompt jump's, a scrollbar mark's click,
/// a search's next match
struct Mark {
    pane: PaneId,
    row: MarkRow,
    until: Instant,
}

/// Where a marked row is: on the pane's screen as it was read, or in its scrollback, which a
/// scroll still to come moves onto the screen
#[derive(Clone, Copy)]
enum MarkRow {
    Screen(usize),
    Stable(wezterm_term::StableRowIndex),
}

/// How a jump ended
enum Outcome {
    /// On screen, at this row
    Landed(usize),
    /// Not there: the conversation's start or a compaction, or too far
    NotFound(Step),
    /// The pane went
    Gone,
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

/// The pane's screen as a jump reads it: each row's text, and whether the cell under the mark
/// it starts with is on Claude Code's message band (any background but the default)
fn read_rows(pane: &Arc<dyn Pane>) -> Vec<Row> {
    let dims = pane.get_dimensions();
    let top = dims.physical_top;
    let (_, lines) = pane.get_lines(top..top + dims.viewport_rows as StableRowIndex);
    lines
        .iter()
        .map(|line| {
            let first = line.visible_cells().find(|c| !c.str().trim().is_empty());
            let band = first.map_or(false, |c| {
                c.str().starts_with(prompts::MARK) && c.attrs().background() != ColorAttribute::Default
            });
            Row {
                text: line.as_str().into_owned(),
                band,
            }
        })
        .collect()
}

/// The screen once Claude Code has drawn what the last key asked for: nothing new from the
/// pane, and the same rows, for [`QUIET`]
async fn settled(pane: &Arc<dyn Pane>) -> Vec<Row> {
    let start = Instant::now();
    let mut seqno = pane.get_current_seqno();
    let mut rows = read_rows(pane);
    let mut still = Instant::now();
    loop {
        smol::Timer::after(POLL).await;
        let now_seqno = pane.get_current_seqno();
        let now_rows = read_rows(pane);
        if now_seqno != seqno || now_rows != rows {
            seqno = now_seqno;
            rows = now_rows;
            still = Instant::now();
        } else if still.elapsed() >= QUIET {
            return rows;
        }
        if start.elapsed() >= MAX_WAIT {
            return rows;
        }
    }
}

/// A click on Claude Code's header at `row`, which jumps it to the prompt the header shows
fn click(pane: &Arc<dyn Pane>, row: usize) {
    for kind in [TermMouseKind::Press, TermMouseKind::Release] {
        let _ = pane.mouse_event(TermMouseEvent {
            kind,
            x: 4,
            y: row as i64,
            x_pixel_offset: 0,
            y_pixel_offset: 0,
            button: MouseButton::Left,
            modifiers: KeyModifiers::NONE,
        });
    }
}

/// Scroll Claude Code's conversation in pane `pane_id` to `prompt` (as typed)
async fn jump(pane_id: PaneId, prompt: String) -> Outcome {
    let key = prompts::key(&prompt);
    let Some(pane) = Mux::get().get_pane(pane_id) else {
        return Outcome::Gone;
    };
    // Already on screen: nothing to scroll
    if let Some(Found::Row(row)) = prompts::find(&read_rows(&pane), &key) {
        return Outcome::Landed(row);
    }
    let mut look = Jump::new(&prompt);
    if pane.key_down(KeyCode::End, KeyModifiers::CTRL).is_err() {
        return Outcome::Gone;
    }
    loop {
        let rows = settled(&pane).await;
        match look.look(&rows) {
            Step::Found(row) => return Outcome::Landed(row),
            Step::Click(row) => click(&pane, row),
            Step::Up => {
                if pane.key_down(KeyCode::PageUp, KeyModifiers::NONE).is_err() {
                    return Outcome::Gone;
                }
            }
            step => return Outcome::NotFound(step),
        }
    }
}

impl TermWindow {
    /// The window's active tab, and where it is
    pub(crate) fn lbterm_active_tab(&self) -> Option<(usize, Arc<Tab>)> {
        let mux = Mux::get();
        let window = mux.get_window(self.mux_window_id)?;
        let idx = window.get_active_tab_idx();
        window.get_tab_at_idx(idx).cloned().map(|tab| (idx, tab))
    }

    /// Why the last jump didn't get there, while that's still worth saying
    pub(crate) fn lbterm_prompt_note(&self) -> Option<String> {
        let (note, until) = self.lbterm_prompts.note.as_ref()?;
        (Instant::now() < *until).then(|| note.clone())
    }

    /// Mark row `row` of `pane`'s scrollback for a moment, as a prompt jump marks where it
    /// lands: a scrollbar mark's click, a search's next match
    pub(crate) fn lbterm_mark_landed(&mut self, pane: PaneId, row: wezterm_term::StableRowIndex) {
        let until = Instant::now() + MARK_FOR;
        self.lbterm_prompts.mark = Some(Mark {
            pane,
            row: MarkRow::Stable(row),
            until,
        });
        self.lbterm_strip_wake(Some(until));
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// When the note goes, or the mark: a repaint then
    pub(crate) fn lbterm_prompt_note_due(&self) -> Option<Instant> {
        let now = Instant::now();
        let note = self.lbterm_prompts.note.as_ref().map(|(_, until)| *until);
        let mark = self.lbterm_prompts.mark.as_ref().map(|m| m.until);
        [note, mark].iter().flatten().copied().filter(|t| *t > now).min()
    }

    /// The prompts session `sid` sent, read from history.jsonl on a thread of its own, then
    /// handed to `then` on this one
    fn lbterm_prompts_load(
        &self,
        sid: String,
        then: impl FnOnce(&mut TermWindow, Vec<Sent>) + Send + Sync + 'static,
    ) {
        let Some(path) = self.config.lbterm_prompt_history.clone() else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("lbterm-prompts".into())
            .spawn(move || {
                let prompts = std::fs::read(&path)
                    .map(|bytes| prompts::sent(&String::from_utf8_lossy(&bytes), &sid))
                    .unwrap_or_else(|err| {
                        log::warn!("prompts: reading {path}: {err:#}");
                        vec![]
                    });
                window.notify(TermWindowNotif::Apply(Box::new(move |tw| then(tw, prompts))));
            });
        if let Err(err) = spawned {
            log::warn!("prompts: no thread to read the history on: {err:#}");
        }
    }

    /// A click on the strip's last prompt: jump to it. Its text as typed comes from the history,
    /// since the user var has it on one line and Claude Code shows its first line first.
    pub(crate) fn lbterm_prompt_jump_last(&mut self) {
        let Some((_, tab)) = self.lbterm_active_tab() else {
            return;
        };
        let Some(Target {
            pane,
            sid,
            prompt: Some(shown),
        }) = target(&tab)
        else {
            return;
        };
        let pane = pane.pane_id();
        self.lbterm_prompts_load(sid, move |tw, prompts| {
            let start: String = shown.chars().take(prompts::KEY_CHARS).collect();
            let text = prompts
                .iter()
                .rev()
                .find(|s| prompts::one_line(&s.text).starts_with(&start))
                .map_or(shown, |s| s.text.clone());
            tw.lbterm_prompt_jump(pane, text);
        });
    }

    /// Jump Claude Code's conversation in `pane` to `prompt`, unless a jump is under way
    pub(crate) fn lbterm_prompt_jump(&mut self, pane: PaneId, prompt: String) {
        if self.lbterm_prompts.jumping {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_prompts.jumping = true;
        self.lbterm_prompts.mark = None;
        self.lbterm_prompts.note = None;
        promise::spawn::spawn(async move {
            let outcome = jump(pane, prompt).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                tw.lbterm_prompt_jumped(pane, outcome)
            })));
        })
        .detach();
    }

    fn lbterm_prompt_jumped(&mut self, pane: PaneId, outcome: Outcome) {
        self.lbterm_prompts.jumping = false;
        let now = Instant::now();
        match outcome {
            Outcome::Landed(row) => {
                self.lbterm_prompts.mark = Some(Mark {
                    pane,
                    row: MarkRow::Screen(row),
                    until: now + MARK_FOR,
                });
            }
            Outcome::Gone => {}
            Outcome::NotFound(step) => {
                let why = if step == Step::Top {
                    "Claude Code doesn't scroll back that far: that message is before the last compaction"
                } else {
                    "Couldn't find that message in the conversation"
                };
                self.lbterm_prompts.note = Some((why.to_string(), now + NOTE_FOR));
            }
        }
        let due = self.lbterm_prompt_note_due();
        self.lbterm_strip_wake(due);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub(crate) fn lbterm_prompt_list_is_open(&self) -> bool {
        self.lbterm_prompts.list.is_some()
    }

    fn lbterm_prompt_list_close(&mut self) {
        if self.lbterm_prompts.list.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// A click on the arrow before the strip's prompt: open the list of the session's prompts,
    /// or close it
    pub(crate) fn lbterm_prompt_list_toggle(&mut self) {
        if self.lbterm_prompt_list_is_open() {
            self.lbterm_prompt_list_close();
            return;
        }
        let Some((_, tab)) = self.lbterm_active_tab() else {
            return;
        };
        let Some(t) = target(&tab) else {
            return;
        };
        let pane = t.pane.pane_id();
        self.lbterm_prompts.list = Some(OpenList {
            pane,
            prompts: None,
            focus: None,
            scroll: 0,
        });
        self.lbterm_prompts_load(t.sid, move |tw, prompts| {
            if let Some(list) = tw.lbterm_prompts.list.as_mut().filter(|l| l.pane == pane) {
                list.prompts = Some(prompts);
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            }
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Where the list goes: under the strip, its text where the strip's words are
    fn lbterm_prompt_list_layout(&self) -> anyhow::Result<Option<(prompts::List, f64)>> {
        let Some(list) = self.lbterm_prompts.list.as_ref() else {
            return Ok(None);
        };
        // While it's read, or with none, one row says so
        let count = list.prompts.as_ref().map_or(1, |p| p.len().max(1));
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let scale = self.dimensions.dpi as f64 / 96.0;
        let cell_w = font.metrics().cell_width.get();
        let row_h = (metrics.cell_size.height as f64 * 1.6).round();
        let stamp_w = 11.0 * cell_w;
        let border = self.get_os_border();
        let top = border.top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
        let window = (
            self.dimensions.pixel_width as f64,
            self.dimensions.pixel_height as f64,
        );
        Ok(prompts::list(
            count,
            self.lbterm_strip_text_x() as f64,
            top,
            window,
            (row_h, stamp_w, stamp_w + 96.0 * cell_w),
            list.scroll,
            scale,
        )
        .map(|l| (l, cell_w)))
    }

    /// Jump to the list's prompt `index`, closing it
    fn lbterm_prompt_list_pick(&mut self, index: usize) {
        let Some(list) = self.lbterm_prompts.list.take() else {
            return;
        };
        let text = list
            .prompts
            .as_ref()
            .and_then(|p| p.get(index))
            .map(|s| s.text.clone());
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        if let Some(text) = text {
            self.lbterm_prompt_jump(list.pane, text);
        }
    }

    /// While the list's open it takes every mouse event: true when it did
    pub(crate) fn lbterm_prompt_list_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if self.lbterm_prompts.list.is_none() {
            return false;
        }
        let Ok(Some((layout, _))) = self.lbterm_prompt_list_layout() else {
            self.lbterm_prompt_list_close();
            return false;
        };
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let loaded = self
            .lbterm_prompts
            .list
            .as_ref()
            .map_or(false, |l| l.prompts.as_ref().map_or(false, |p| !p.is_empty()));
        let hit = layout.hit(x, y).filter(|_| loaded);
        match event.kind {
            WMEK::Move => {
                if let Some(list) = self.lbterm_prompts.list.as_mut() {
                    if list.focus != hit && (hit.is_some() || layout.card.contains(x, y)) {
                        list.focus = hit;
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
                Some(index) => self.lbterm_prompt_list_pick(index),
                None if layout.card.contains(x, y) => {}
                None => self.lbterm_prompt_list_close(),
            },
            WMEK::Press(_) if !layout.card.contains(x, y) => self.lbterm_prompt_list_close(),
            WMEK::VertWheel(n) => {
                if let Some(list) = self.lbterm_prompts.list.as_mut() {
                    list.scroll = if n > 0 {
                        list.scroll.saturating_sub(3)
                    } else {
                        list.scroll + 3
                    };
                    context.invalidate();
                }
            }
            _ => {}
        }
        true
    }

    /// While the list's open it takes every key: up and down the list, Enter to jump, Esc to
    /// close. True when it did.
    pub(crate) fn lbterm_prompt_list_key(&mut self, key: &KeyCode, _mods: KeyModifiers) -> bool {
        let Some(list) = self.lbterm_prompts.list.as_ref() else {
            return false;
        };
        let count = list.prompts.as_ref().map_or(0, |p| p.len());
        let focus = list.focus;
        if *key == KeyCode::Escape {
            self.lbterm_prompt_list_close();
            return true;
        }
        if count == 0 {
            return true;
        }
        if *key == KeyCode::Enter {
            if let Some(index) = focus {
                self.lbterm_prompt_list_pick(index);
            }
            return true;
        }
        let shown = self
            .lbterm_prompt_list_layout()
            .ok()
            .flatten()
            .map_or(1, |(l, _)| l.rows.len().max(1));
        // Positions down the list, newest first
        let at = focus.map(|i| count - 1 - i);
        let to = match key {
            KeyCode::UpArrow => Some(at.map_or(0, |p| p.saturating_sub(1))),
            KeyCode::DownArrow => Some(at.map_or(0, |p| (p + 1).min(count - 1))),
            KeyCode::PageUp => Some(at.map_or(0, |p| p.saturating_sub(shown))),
            KeyCode::PageDown => Some(at.map_or(0, |p| (p + shown).min(count - 1))),
            KeyCode::Home => Some(0),
            KeyCode::End => Some(count - 1),
            _ => None,
        };
        if let (Some(to), Some(list)) = (to, self.lbterm_prompts.list.as_mut()) {
            list.focus = Some(count - 1 - to);
            // Keep it on screen
            if to < list.scroll {
                list.scroll = to;
            } else if to >= list.scroll + shown {
                list.scroll = to + 1 - shown;
            }
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        true
    }

    /// Draw the list, if it's open: the tab menu's card, a row a prompt
    pub(crate) fn paint_lbterm_prompt_list(&mut self) -> anyhow::Result<()> {
        let Some(list) = self.lbterm_prompts.list.as_ref() else {
            return Ok(());
        };
        if Mux::get().get_pane(list.pane).is_none() {
            self.lbterm_prompts.list = None;
            return Ok(());
        }
        let Some((layout, cell_w)) = self.lbterm_prompt_list_layout()? else {
            return Ok(());
        };
        let card_rgb = {
            let (r, g, b, _) = self.palette().background.to_srgb_u8();
            palette::blend(((r as u32) << 16) | ((g as u32) << 8) | b as u32, palette::CHIP, 0.55)
        };
        let Some(list) = self.lbterm_prompts.list.as_ref() else {
            return Ok(());
        };
        // Scrolled past the end: held at the oldest
        let scroll = layout.scroll;
        let focus = list.focus;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let now = chrono::Local::now().naive_local();
        let stamp = |at_ms: i64| {
            let at = SystemTime::UNIX_EPOCH + Duration::from_millis(at_ms.max(0) as u64);
            prompts::stamp(chrono::DateTime::<chrono::Local>::from(at).naive_local(), now)
        };
        // What each row says: its stamp and its prompt, or why there are none
        let rows: Vec<(Rect, Option<String>, String, bool)> = match &list.prompts {
            None => layout
                .rows
                .iter()
                .map(|(_, r)| (*r, None, "Reading your prompts\u{2026}".to_string(), false))
                .collect(),
            Some(p) if p.is_empty() => layout
                .rows
                .iter()
                .map(|(_, r)| (*r, None, "No prompts sent in this session yet".to_string(), false))
                .collect(),
            Some(p) => layout
                .rows
                .iter()
                .filter_map(|(i, r)| {
                    let sent = p.get(*i)?;
                    let room = ((r.x + r.w - 10.0 * scale - layout.text_x) / cell_w).floor();
                    let text = strip::elide(&prompts::one_line(&sent.text), room.max(0.0) as usize);
                    Some((*r, Some(stamp(sent.at_ms)), text, focus == Some(*i)))
                })
                .collect(),
        };
        if let Some(list) = self.lbterm_prompts.list.as_mut() {
            list.scroll = scroll;
        }

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_h = metrics.cell_size.height as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        let mut words = vec![];
        for (r, stamp, text, _) in &rows {
            let y = r.y + (r.h - line_h) / 2.0;
            let mut place = |text: &str, x: f64, w: f64, colour: u32| -> anyhow::Result<()> {
                let element = Element::new(&font, ElementContent::Text(text.to_string())).colors(ElementColors {
                    border: BorderColor::default(),
                    bg: LinearRgba::TRANSPARENT.into(),
                    text: srgb(colour, 1.0).into(),
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
            match stamp {
                Some(stamp) => {
                    let stamp_x = layout.card.x + 12.0 * scale;
                    place(stamp, stamp_x, layout.text_x - stamp_x, palette::PROJECT)?;
                    place(text, layout.text_x, r.x + r.w - layout.text_x, palette::TITLE)?;
                }
                None => {
                    let x = layout.card.x + 12.0 * scale;
                    place(text, x, r.x + r.w - x, palette::PROJECT)?;
                }
            }
        }

        // The light round it, fading out, as the tab menu's
        {
            let layer = gl_state.layer_for_zindex(GLOW_Z)?;
            let mut quads = layer.quad_allocator();
            for (i, alpha) in [0.16, 0.09, 0.05, 0.025].iter().enumerate() {
                let by = (2.0 + 3.0 * i as f64) * scale;
                self.lbterm_menu_rounded(&mut quads, grow(layout.card, by), radius + by, srgb(palette::GLOW, *alpha))?;
            }
        }
        let layer = gl_state.layer_for_zindex(CARD_Z)?;
        let mut quads = layer.quad_allocator();
        self.lbterm_menu_rounded(&mut quads, grow(layout.card, stroke), radius + stroke, srgb(palette::RING_MIDDLE, 1.0))?;
        self.lbterm_menu_rounded(&mut quads, layout.card, radius, srgb(card_rgb, 1.0))?;
        let hover = srgb(palette::blend(card_rgb, palette::HOVER, palette::HOVER_ALPHA * 1.6), 1.0);
        for (r, _, _, lit) in &rows {
            if *lit {
                self.lbterm_menu_rounded(&mut quads, *r, 6.0 * scale, hover)?;
            }
        }
        drop(quads);
        for computed in &words {
            self.render_element(computed, gl_state, None)?;
        }
        Ok(())
    }

    /// Mark the row a jump landed on, for a moment: the panel's light along it
    pub(crate) fn paint_lbterm_prompt_mark(&mut self) -> anyhow::Result<()> {
        let Some(mark) = self.lbterm_prompts.mark.as_ref() else {
            return Ok(());
        };
        if Instant::now() >= mark.until {
            self.lbterm_prompts.mark = None;
            return Ok(());
        }
        let (pane, row) = (mark.pane, mark.row);
        // The tab's own panes, so a pane under a search overlay is still found
        let Some(tab) = Mux::get().get_active_tab_for_window(self.mux_window_id) else {
            return Ok(());
        };
        let panes = tab.iter_panes();
        let Some(pos) = panes.iter().find(|p| p.pane.pane_id() == pane) else {
            return Ok(());
        };
        let row = match row {
            MarkRow::Screen(row) => row,
            MarkRow::Stable(stable) => {
                let dims = pos.pane.get_dimensions();
                let top = self.get_viewport(pane).unwrap_or(dims.physical_top);
                if stable < top || (stable - top) as usize >= pos.height {
                    return Ok(());
                }
                (stable - top) as usize
            }
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
        let x = border.left.get() as f64 + padding_left as f64 + pos.left as f64 * cell_w;
        let y = top_bar as f64
            + padding_top as f64
            + border.top.get() as f64
            + (pos.top + row) as f64 * cell_h;
        let band = Rect {
            x: x - 4.0 * scale,
            y,
            w: pos.width as f64 * cell_w + 8.0 * scale,
            h: cell_h,
        };
        let bar = Rect {
            x: band.x,
            y,
            w: (3.0 * scale).max(2.0),
            h: cell_h,
        };
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(MARK_Z)?;
        let mut quads = layer.quad_allocator();
        self.lbterm_menu_rounded(&mut quads, band, 4.0 * scale, srgb(palette::RING_MIDDLE, 0.16))?;
        self.lbterm_menu_rounded(&mut quads, bar, bar.w / 2.0, srgb(palette::RING_MIDDLE, 0.85))?;
        Ok(())
    }
}

/// LBTerm: the list of prompts and the landed mark, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// The list while it's open: the session's prompts newest first, as it shows them, or the
    /// one row saying why there are none
    pub(crate) fn lbterm_state_prompt_list(&self) -> Option<lbterm_extras::state::Menu> {
        let list = self.lbterm_prompts.list.as_ref()?;
        let layout = self.lbterm_prompt_list_layout().ok().flatten().map(|(layout, _)| layout);
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        // Where prompt `index` (oldest first) shows, if it's scrolled into view
        let rect_of = |index: usize| {
            layout
                .as_ref()
                .and_then(|l| l.rows.iter().find(|(i, _)| *i == index).map(|(_, r)| rect(r)))
        };
        let item = |label: &str, index: usize| lbterm_extras::state::MenuItem {
            label: label.to_string(),
            checked: false,
            rect: rect_of(index),
        };
        let (items, focus) = match &list.prompts {
            None => (vec![item("Reading your prompts\u{2026}", 0)], None),
            Some(p) if p.is_empty() => (vec![item("No prompts sent in this session yet", 0)], None),
            Some(p) => (
                p.iter()
                    .enumerate()
                    .rev()
                    .map(|(i, sent)| item(&prompts::one_line(&sent.text), i))
                    .collect(),
                list.focus.filter(|&i| i < p.len()).map(|i| p.len() - 1 - i),
            ),
        };
        Some(lbterm_extras::state::Menu {
            kind: "prompts".to_string(),
            tab: None,
            rect: layout.as_ref().map(|l| rect(&l.card)),
            items,
            focus,
            renaming: None,
            waiting: None,
        })
    }

    /// What of the open list is at `x`, `y`, newest first: `Some(Some(item))` on a prompt,
    /// `Some(None)` on the card elsewhere, `None` off it
    pub(crate) fn lbterm_prompt_list_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        let (layout, _) = self.lbterm_prompt_list_layout().ok()??;
        if !layout.card.contains(x, y) {
            return None;
        }
        let count = self
            .lbterm_prompts
            .list
            .as_ref()
            .and_then(|l| l.prompts.as_ref())
            .map_or(0, |p| p.len());
        Some(layout.hit(x, y).filter(|&i| i < count).map(|i| count - 1 - i))
    }

    /// The row a jump, a scrollbar mark or a search landed on, while it's marked
    pub(crate) fn lbterm_state_landed(&self, now: Instant) -> Option<lbterm_extras::state::Landed> {
        let mark = self.lbterm_prompts.mark.as_ref().filter(|m| m.until > now)?;
        let row = match mark.row {
            MarkRow::Screen(row) => Some(row),
            MarkRow::Stable(stable) => {
                let pane = Mux::get().get_pane(mark.pane)?;
                let dims = pane.get_dimensions();
                let top = self.get_viewport(mark.pane).unwrap_or(dims.physical_top);
                (stable >= top && ((stable - top) as usize) < dims.viewport_rows).then(|| (stable - top) as usize)
            }
        };
        Some(lbterm_extras::state::Landed {
            pane: mark.pane as u64,
            row,
            left_ms: lbterm_extras::state::millis(mark.until.saturating_duration_since(now)),
        })
    }

    /// Whether a jump back to a prompt is under way: Claude Code's view is being scrolled to it
    pub(crate) fn lbterm_prompt_jumping(&self) -> bool {
        self.lbterm_prompts.jumping
    }

    /// The prompt row's note's time, while one shows
    pub(crate) fn lbterm_prompt_note_wait(&self, now: Instant) -> Option<lbterm_extras::state::Wait> {
        let (_, until) = self.lbterm_prompts.note.as_ref().filter(|(_, until)| *until > now)?;
        Some(lbterm_extras::state::Wait {
            name: "strip.note".to_string(),
            left_ms: lbterm_extras::state::millis(until.saturating_duration_since(now)),
        })
    }
}
