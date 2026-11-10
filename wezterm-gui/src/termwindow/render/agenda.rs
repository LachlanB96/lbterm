//! LBTerm: the agenda's button (`config.lbterm_agenda`; lbterm/AGENDA.md), whose logic is
//! lbterm_extras::agenda. On the strip's first row after the InMail button (or in its place): a
//! hairline pill round a checklist, with the counts of what's urgent (the waiting orange) and
//! what's failing (red) beside it, the counts the agenda's file gives. The file is read on the
//! state files' thread when it changes (crate::state_files). A click drops a list from the
//! button, in the tab menu's card and light: what's urgent, what's failing, what's coming up and
//! what's yours to do, each under its heading, a group's rows folded into one line until it's
//! opened, and the agents' to-dos behind a line that shows them. A click on a row (or the arrow
//! keys and Enter) opens it: its detail and its buttons under it. A button fires the
//! `lbterm-agenda-press` Lua event with the row and what was pressed; one that asks first (its
//! `confirm`) says so on its first press and goes on the second. A row pressed shows dim until
//! the file says something new. Esc or a click elsewhere closes the list.
//!
//! The counts changing dissolve on `swap`, and more brightens the button once
//! (render/strip_swap.rs). The list is drawn in the InMail list's z-layers, 77 to 79, their slot
//! in the menus' visuals, fading in and out (render/menu_fade.rs, `Slot::List`): only one of them
//! is open at a time. Its lit row stays drawn on its card, under the opened row's buttons.

use crate::termwindow::render::menu_fade::Slot;
use crate::scripting::guiwin::GuiWin;
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{ComputedElement, ComputedElementContent, Element};
use crate::termwindow::{TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, RectF, WindowOps};
use config::SrgbaTuple;
use lbterm_extras::agenda::{self, Agenda, Line, Press, Row, RowKey, View};
use lbterm_extras::tabs::Rect;
use lbterm_extras::{prompts, strip};
use lbterm_panel::palette;
use mux_lua::MuxPane;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wezterm_font::LoadedFont;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// The list's card and words: the InMail list's, in their slot (render/menu_fade.rs,
/// `Slot::List`), 77 its glow's
const CARD_Z: i8 = Slot::List.card_z();
const TEXT_Z: i8 = Slot::List.text_z();

/// What the list says with nothing on it
const NOTHING: &str = "Nothing on the list";

/// How many columns the words after a row's when and project get
const TEXT_COLS: usize = 64;
/// The most columns when a row is due and the project it's about get
const WHEN_COLS: usize = 14;
const PROJECT_COLS: usize = 18;

#[derive(Default)]
pub struct AgendaState {
    /// What the button was last built showing: what's urgent, what's failing, and whether its
    /// list was open
    shown: Option<(usize, usize, bool)>,
    /// What's opened in the list, kept while it's closed: the agents' to-dos shown, the groups
    /// opened (an opened row closes with the list)
    view: View,
    list: Option<OpenList>,
    /// The list as last laid out, and what from: the paint, every mouse event while it's open
    /// and the arrow keys all ask for it, so it's laid out again only when something it's laid
    /// out from has changed (as the InMail list's)
    laid: RefCell<Option<(ListAt, Rc<Laid>)>>,
    /// The list's words as its last frame laid them out (render/words.rs)
    words: RefCell<super::words::Words>,
}

impl AgendaState {
    /// The list's words point into a glyph atlas that's gone (gpu_lost.rs)
    pub(crate) fn forget_words(&mut self) {
        self.words.get_mut().forget();
    }
}

/// What a line the pointer or the arrow keys are on is, kept by what it shows, since the file
/// can change while the list is open
#[derive(Debug, Clone, PartialEq)]
enum Focus {
    Group(String),
    Row(RowKey),
    Filter,
}

fn focus_of(agenda: &Agenda, line: &Line) -> Option<Focus> {
    match line {
        Line::Group { key, .. } => Some(Focus::Group(key.clone())),
        Line::Row { row, .. } => agenda.rows.get(*row).map(|r| Focus::Row(r.key())),
        Line::Filter { .. } => Some(Focus::Filter),
        _ => None,
    }
}

/// The list while it's open
struct OpenList {
    focus: Option<Focus>,
    /// The opened row's button under the pointer, by its place among them
    chip: Option<usize>,
    /// A button that asks first, pressed once: the row and the button's place
    armed: Option<(RowKey, usize)>,
    /// Rows pressed, shown dim until the file says something new, and what it said when they were
    pressed: Vec<RowKey>,
    pressed_in: Option<Arc<Agenda>>,
    /// How far down the first line shown is
    scroll: usize,
}

/// A button under the opened row: where it is, what it does, its place among the row's and the
/// words on it
struct Chip {
    rect: Rect,
    press: Press,
    index: usize,
    label: String,
    armed: bool,
}

/// The list laid out: its card and rows (a row's index into `prompts::List`, which counts from
/// the bottom, is `lines.len() - 1 - k`), its lines, a column's width, how wide the when and the
/// project's columns are, and the opened row's buttons while they're shown
struct Laid {
    list: prompts::List,
    lines: Vec<Line>,
    cell_w: f64,
    when_w: f64,
    project_w: f64,
    chips: Vec<Chip>,
}

impl Laid {
    /// The line at `x`, `y`, by its index
    fn line_at(&self, x: f64, y: f64) -> Option<usize> {
        self.list.hit(x, y).map(|k| self.lines.len() - 1 - k)
    }

    /// Each line shown, top to bottom, by its index, and where it is
    fn shown(&self) -> impl Iterator<Item = (usize, &Rect)> + '_ {
        let n = self.lines.len();
        self.list.rows.iter().map(move |(k, r)| (n - 1 - k, r))
    }
}

/// What the list was laid out from: the agenda (crate::state_files replaces it when what the file
/// says changes; held, so another can't be made where it was), what's opened, the button armed,
/// the scroll, the shape generation (the fonts) and where it goes, as f64 bits
struct ListAt {
    agenda: Arc<Agenda>,
    view: View,
    armed: Option<(RowKey, usize)>,
    scroll: usize,
    shapes: usize,
    geometry: [u64; 7],
}

impl PartialEq for ListAt {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.agenda, &o.agenda)
            && self.view == o.view
            && self.armed == o.armed
            && (self.scroll, self.shapes, self.geometry) == (o.scroll, o.shapes, o.geometry)
    }
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn rgb01(rgb: u32) -> [f32; 3] {
    let (r, g, b) = palette::bytes(rgb);
    [r as f32 / 255., g as f32 / 255., b as f32 / 255.]
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

/// Where the strip's element for `item` is, in the window
fn bounds_of(element: &ComputedElement, item: TabBarItem) -> Option<RectF> {
    if element.item_type == Some(UIItemType::TabBar(item)) {
        return Some(element.bounds);
    }
    match &element.content {
        ComputedElementContent::Children(kids) => kids.iter().find_map(|k| bounds_of(k, item)),
        _ => None,
    }
}

/// What a line says in the state file
fn label(agenda: &Agenda, line: &Line) -> String {
    match line {
        Line::Heading { section, count } => format!("{} ({count})", section.title()),
        Line::Group { title, count, .. } => format!("{title} ({count})"),
        Line::Row { row, .. } => agenda.rows.get(*row).map_or(String::new(), |r| format!("{}: {}", r.project, r.title)),
        Line::Detail { row } => agenda.rows.get(*row).map_or(String::new(), |r| format!("detail: {}", r.detail)),
        Line::Buttons { .. } => String::new(),
        Line::Filter { count, .. } => format!("{} ({count})", agenda::Section::Agents.title()),
        Line::Empty => NOTHING.to_string(),
    }
}

impl TermWindow {
    /// The button shows: the agenda in the config, and the strip holding only the window's
    /// buttons beside the tab panel, as the InMail button's
    pub(crate) fn lbterm_agenda_on(&self) -> bool {
        self.config.lbterm_agenda.is_some()
            && self.config.lbterm_tab_panel.enabled
            && config::lbterm::hides_tabs(&self.config)
            && self.config.use_fancy_tab_bar
            && self.show_tab_bar
            && !self.config.tab_bar_at_bottom
    }

    /// What the agenda's file says now, as last read; nothing without the file
    fn lbterm_agenda_now(&self) -> Option<Arc<Agenda>> {
        let file = &self.config.lbterm_agenda.as_ref()?.file;
        crate::state_files::agenda(file)
    }

    /// Each paint, before the strip is built: true when it must be built again, for new counts on
    /// the button or its list opening or closing. New counts in the same window dissolve, and
    /// more brightens the button once (render/strip_swap.rs). Rows pressed stop showing dim once
    /// the file says something new.
    pub(crate) fn lbterm_agenda_refresh(&mut self) -> bool {
        let agenda = if self.lbterm_agenda_on() { self.lbterm_agenda_now() } else { None };
        if let (Some(list), Some(now)) = (self.lbterm_agenda.list.as_mut(), agenda.as_ref()) {
            if list.pressed_in.as_ref().map_or(false, |was| !Arc::ptr_eq(was, now)) {
                list.pressed.clear();
                list.pressed_in = None;
            }
        }
        let shown = agenda
            .as_ref()
            .map(|a| (a.urgent, a.failing, self.lbterm_agenda.list.is_some()));
        if shown == self.lbterm_agenda.shown {
            return false;
        }
        if let (Some((u0, f0, _)), Some((u1, f1, _))) = (self.lbterm_agenda.shown, shown) {
            if (u0, f0) != (u1, f1) {
                self.lbterm_strip_swap_button(TabBarItem::LbtermAgenda, u1 > u0 || f1 > f0);
            }
        }
        if shown.is_none() {
            self.lbterm_agenda.list = None;
        }
        self.lbterm_agenda.shown = shown;
        true
    }

    /// The button for the strip's first row, `first_row` the row's bounds, and how wide it is laid
    /// out alone: the checklist and the counts in a hairline pill, which the strip places after
    /// the InMail button (`lbterm_strip_buttons`). Nothing while it doesn't show.
    pub(crate) fn lbterm_agenda_button(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        first_row: RectF,
    ) -> anyhow::Result<Option<(Element, f32)>> {
        let (Some(settings), Some((urgent, failing, open))) = (self.config.lbterm_agenda.as_ref(), self.lbterm_agenda.shown) else {
            return Ok(None);
        };
        let badges = agenda::badges(urgent, failing);
        // As the InMail button's: sat back with nothing to count, lit under the pointer or open
        let colors = |lit: bool| lbterm_extras::inbox::button_colors([surface.0, surface.1, surface.2], !badges.is_empty(), lit);
        let (rest, lit) = (colors(open), colors(true));
        let mut kids = vec![super::inbox::pill_text(font, settings.glyph.clone(), rest.glyph, lit.glyph)];
        for (count, colour) in badges {
            kids.push(super::inbox::pill_text(font, format!(" {count}"), rgb01(colour), rgb01(colour)));
        }
        self.lbterm_strip_pill(
            font,
            metrics,
            surface,
            first_row,
            TabBarItem::LbtermAgenda,
            kids,
            (rest.outline, lit.outline),
            (rest.glyph, lit.glyph),
        )
        .map(Some)
    }

    pub(crate) fn lbterm_agenda_list_is_open(&self) -> bool {
        self.lbterm_agenda.list.is_some()
    }

    fn lbterm_agenda_list_close(&mut self) {
        if self.lbterm_agenda.list.take().is_some() {
            self.lbterm_agenda.view.row = None;
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// A click on the button: open the list, or close it
    pub(crate) fn lbterm_agenda_toggle(&mut self) {
        if self.lbterm_agenda.list.is_some() {
            self.lbterm_agenda_list_close();
            return;
        }
        if !self.lbterm_agenda_on() || self.lbterm_agenda_now().is_none() {
            return;
        }
        self.lbterm_agenda.list = Some(OpenList {
            focus: None,
            chip: None,
            armed: None,
            pressed: vec![],
            pressed_in: None,
            scroll: 0,
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Where the list goes: under the strip, its card hanging from the button's left, its lines
    /// and the opened row's buttons. Laid out again only when the agenda, what's opened, the
    /// button armed, the scroll, the fonts or where it goes have changed. Nothing while the button
    /// isn't on the strip.
    fn lbterm_agenda_list_layout(&self, agenda: &Arc<Agenda>) -> anyhow::Result<Option<Rc<Laid>>> {
        let Some(list) = self.lbterm_agenda.list.as_ref() else {
            return Ok(None);
        };
        let Some(button) = self
            .fancy_tab_bar
            .as_ref()
            .and_then(|bar| bounds_of(bar, TabBarItem::LbtermAgenda))
        else {
            return Ok(None);
        };
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let scale = self.dimensions.dpi as f64 / 96.0;
        let cell_w = font.metrics().cell_width.get();
        let row_h = (metrics.cell_size.height as f64 * 1.6).round();
        let border = self.get_os_border();
        let top = border.top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
        let window = (
            self.dimensions.pixel_width as f64,
            self.dimensions.pixel_height as f64,
        );
        // The card's left edge at the button's: prompts::list puts it its inset before `left`
        let left = button.min_x() as f64 + 12.0 * scale;
        let view = &self.lbterm_agenda.view;
        let at = ListAt {
            agenda: Arc::clone(agenda),
            view: view.clone(),
            armed: list.armed.clone(),
            scroll: list.scroll,
            shapes: self.shape_generation,
            geometry: [left, top, window.0, window.1, row_h, cell_w, scale].map(f64::to_bits),
        };
        if let Some((was, laid)) = self.lbterm_agenda.laid.borrow().as_ref() {
            if *was == at {
                return Ok(Some(Rc::clone(laid)));
            }
        }

        let lines = agenda::lines(agenda, view);
        // The mark (or a group's chevron) first, then when, the project and the words
        let mark_w = 2.0 * cell_w;
        let cols = |n: usize| if n > 0 { (n as f64 + 2.0) * cell_w } else { 0.0 };
        let when_w = cols(agenda.rows.iter().map(|r| r.when.chars().count()).max().unwrap_or(0).min(WHEN_COLS));
        let project_w = cols(agenda.rows.iter().map(|r| r.project.chars().count()).max().unwrap_or(0).min(PROJECT_COLS));
        let width = 24.0 * scale + mark_w + when_w + project_w + TEXT_COLS as f64 * cell_w;
        let Some(plist) = prompts::list(lines.len(), left, top, window, (row_h, mark_w, width), list.scroll, scale) else {
            *self.lbterm_agenda.laid.borrow_mut() = None;
            return Ok(None);
        };
        // The opened row's buttons, while its line shows: its own, Claude, snoozes, its page
        let mut chips = vec![];
        for (k, r) in &plist.rows {
            let Line::Buttons { row } = &lines[lines.len() - 1 - k] else {
                continue;
            };
            let Some(row) = agenda.rows.get(*row) else {
                continue;
            };
            let mut x = plist.text_x;
            for (index, (press, word)) in agenda::presses(row).into_iter().enumerate() {
                let armed = list.armed.as_ref() == Some(&(row.key(), index));
                let label = match agenda::confirm(row, &press).filter(|_| armed) {
                    Some(ask) => ask.to_string(),
                    None => word,
                };
                let w = (label.chars().count() as f64 + 2.0) * cell_w;
                chips.push(Chip {
                    rect: Rect {
                        x,
                        y: r.y + 4.0 * scale,
                        w,
                        h: r.h - 8.0 * scale,
                    },
                    press,
                    index,
                    label,
                    armed,
                });
                x += w + 6.0 * scale;
            }
        }
        let laid = Rc::new(Laid {
            list: plist,
            lines,
            cell_w,
            when_w,
            project_w,
            chips,
        });
        *self.lbterm_agenda.laid.borrow_mut() = Some((at, Rc::clone(&laid)));
        Ok(Some(laid))
    }

    /// A click on a line, or Enter on it: a group or the agents' to-dos opened or folded again, a
    /// row opened (its detail and buttons) or closed, the line flashing white as it is (Lachlan's
    /// 6 of 2026-11-08: as every other menu's pick does, 9b's flash)
    fn lbterm_agenda_activate(&mut self, agenda: &Arc<Agenda>, line: &Line) {
        let pressed = focus_of(agenda, line);
        self.lbterm_agenda_open_or_fold(agenda, line);
        if let Some(focus) = pressed {
            self.lbterm_agenda_flash_line(agenda, &focus);
        }
    }

    /// A line pressed flashes where it shows now that the press has opened or folded it (a row's
    /// detail and buttons come in under it, and a group's rows under its heading), in the list's
    /// slot (render/menu_fade.rs, `lbterm_menu_flash`, over the card, which stays open). Laid out
    /// here for the line's place, the layout the next paint then reuses
    fn lbterm_agenda_flash_line(&self, agenda: &Arc<Agenda>, focus: &Focus) {
        let Ok(Some(laid)) = self.lbterm_agenda_list_layout(agenda) else {
            return;
        };
        let at = laid
            .shown()
            .find(|(i, _)| focus_of(agenda, &laid.lines[*i]).as_ref() == Some(focus))
            .map(|(_, r)| *r);
        if let Some(row) = at {
            self.lbterm_menu_flash(Slot::List, row);
        }
    }

    fn lbterm_agenda_open_or_fold(&mut self, agenda: &Agenda, line: &Line) {
        let view = &mut self.lbterm_agenda.view;
        match line {
            Line::Group { key, .. } => {
                if !view.groups.remove(key) {
                    view.groups.insert(key.clone());
                }
            }
            Line::Row { row, .. } => {
                let Some(row) = agenda.rows.get(*row) else {
                    return;
                };
                let key = row.key();
                view.row = if view.row.as_ref() == Some(&key) { None } else { Some(key) };
                if let Some(list) = self.lbterm_agenda.list.as_mut() {
                    list.armed = None;
                }
            }
            Line::Filter { .. } => view.agents = !view.agents,
            _ => return,
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A press on the opened row's button `chip`: one that asks first is armed by the first press
    /// and goes on the second; then `lbterm-agenda-press` fires with the row and what it does.
    /// Claude and the row's page close the list, as they take him elsewhere; the rest leave it
    /// open, the row dim until the file says something new. Each press flashes the button white
    /// (Lachlan's 6 of 2026-11-08): the one that asks first where it shows asking, the rest where
    /// they were pressed, as the row closes or the list goes
    fn lbterm_agenda_press(&mut self, agenda: &Arc<Agenda>, chip: &Chip) {
        let Some(key) = self.lbterm_agenda.view.row.clone() else {
            return;
        };
        let Some(row) = agenda.rows.iter().find(|r| r.key() == key).cloned() else {
            return;
        };
        let Some(list) = self.lbterm_agenda.list.as_mut() else {
            return;
        };
        if agenda::confirm(&row, &chip.press).is_some() && !chip.armed {
            list.armed = Some((key, chip.index));
            // Its words are the ask now, so it's wider: flashed as it shows them
            let asking = self
                .lbterm_agenda_list_layout(agenda)
                .ok()
                .flatten()
                .and_then(|laid| laid.chips.iter().find(|c| c.index == chip.index).map(|c| c.rect));
            self.lbterm_menu_flash(Slot::List, asking.unwrap_or(chip.rect));
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
            return;
        }
        self.lbterm_menu_flash(Slot::List, chip.rect);
        let Some(list) = self.lbterm_agenda.list.as_mut() else {
            return;
        };
        list.armed = None;
        let press = chip.press.clone();
        match press {
            Press::Claude | Press::Open => self.lbterm_agenda_list_close(),
            Press::Action(_) | Press::Snooze(_) => {
                list.pressed.push(key);
                list.pressed_in = Some(Arc::clone(agenda));
                self.lbterm_agenda.view.row = None;
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
            }
        }
        self.lbterm_agenda_fire(row, press);
    }

    /// Fire `lbterm-agenda-press` with the window, the active pane and a table of what was
    /// pressed: `what` (`do`, `claude`, `snooze` or `open`), the row's `source`, `id`, `project`
    /// and `title`, and `button` (the row's own button's word), `days` (a snooze's) or `open` (its
    /// page), and `file`, the agenda's file as the config names it, so a handler leaves alone a
    /// press from an agenda that isn't its own (a test window running Lachlan's config with a file
    /// of its own). The handler runs it; LBTerm does nothing more.
    fn lbterm_agenda_fire(&mut self, row: Row, press: Press) {
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        log::info!("agenda: {} on {} {}", press.what(), row.source, row.id);
        let file = self.config.lbterm_agenda.as_ref().map(|a| a.file.clone());
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        async fn fire(lua: Option<Rc<mlua::Lua>>, window: GuiWin, pane: MuxPane, row: Row, press: Press, file: Option<String>) -> anyhow::Result<()> {
            let Some(lua) = lua else {
                return Ok(());
            };
            let table = lua.create_table()?;
            table.set("what", press.what())?;
            table.set("source", row.source.clone())?;
            table.set("id", row.id.clone())?;
            table.set("project", row.project.clone())?;
            table.set("title", row.title.clone())?;
            table.set("file", file)?;
            match press {
                Press::Action(i) => table.set("button", row.buttons.get(i).map(|b| b.label.clone()))?,
                Press::Snooze(days) => table.set("days", days)?,
                Press::Open => table.set("open", row.open.clone())?,
                Press::Claude => {}
            }
            let args = lua.pack_multi((window, pane, table))?;
            config::lua::emit_event(&lua, ("lbterm-agenda-press".to_string(), args))
                .await
                .map_err(|e| {
                    log::error!("while processing lbterm-agenda-press event: {:#}", e);
                    e
                })?;
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| fire(lua, window, pane, row, press, file))).detach();
    }

    /// While the list's open it takes every mouse event: true when it did
    pub(crate) fn lbterm_agenda_list_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if self.lbterm_agenda.list.is_none() {
            return false;
        }
        let Some(agenda) = self.lbterm_agenda_now() else {
            self.lbterm_agenda_list_close();
            return false;
        };
        let Ok(Some(laid)) = self.lbterm_agenda_list_layout(&agenda) else {
            self.lbterm_agenda_list_close();
            return false;
        };
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let on_card = laid.list.card.contains(x, y);
        let chip = laid.chips.iter().position(|c| c.rect.contains(x, y));
        let line = laid.line_at(x, y);
        match event.kind {
            WMEK::Move => {
                let focus = line.and_then(|l| focus_of(&agenda, &laid.lines[l]));
                let takes = focus.is_some() || chip.is_some();
                if let Some(list) = self.lbterm_agenda.list.as_mut() {
                    let mut changed = false;
                    if list.focus != focus && (focus.is_some() || on_card) {
                        list.focus = focus;
                        changed = true;
                    }
                    if list.chip != chip {
                        list.chip = chip;
                        changed = true;
                    }
                    if changed {
                        context.invalidate();
                    }
                }
                context.set_cursor(Some(if takes { CursorIcon::Pointer } else { CursorIcon::Default }));
            }
            WMEK::Press(MousePress::Left) => {
                if let Some(c) = chip {
                    self.lbterm_agenda_press(&agenda, &laid.chips[c]);
                } else if let Some(l) = line {
                    self.lbterm_agenda_activate(&agenda, &laid.lines[l]);
                } else if !on_card {
                    // The button among them: a second click closes it
                    self.lbterm_agenda_list_close();
                }
            }
            WMEK::Press(_) if !on_card => self.lbterm_agenda_list_close(),
            WMEK::VertWheel(n) => {
                if let Some(list) = self.lbterm_agenda.list.as_mut() {
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

    /// While the list's open it takes every key: up and down its rows, groups and the agents'
    /// line, Enter to open or fold one, Esc to close. True when it did.
    pub(crate) fn lbterm_agenda_list_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        let Some(focus) = self.lbterm_agenda.list.as_ref().map(|l| l.focus.clone()) else {
            return false;
        };
        if *key == KeyCode::Escape {
            self.lbterm_agenda_list_close();
            return true;
        }
        let Some(agenda) = self.lbterm_agenda_now() else {
            return true;
        };
        let Ok(Some(laid)) = self.lbterm_agenda_list_layout(&agenda) else {
            return true;
        };
        let takes: Vec<usize> = laid
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.takes_focus())
            .map(|(i, _)| i)
            .collect();
        if takes.is_empty() {
            return true;
        }
        let at = focus.and_then(|f| takes.iter().position(|&i| focus_of(&agenda, &laid.lines[i]).as_ref() == Some(&f)));
        // Enter acts on the row lit; with none lit it lights the first, as Home does
        if *key == KeyCode::Enter && at.is_some() {
            if let Some(p) = at {
                let line = laid.lines[takes[p]].clone();
                self.lbterm_agenda_activate(&agenda, &line);
            }
            return true;
        }
        let shown = laid.list.rows.len().max(1);
        let last = takes.len() - 1;
        let to = match key {
            KeyCode::UpArrow => at.map_or(0, |p| p.saturating_sub(1)),
            KeyCode::DownArrow => at.map_or(0, |p| (p + 1).min(last)),
            KeyCode::PageUp => at.map_or(0, |p| p.saturating_sub(shown)),
            KeyCode::PageDown => at.map_or(0, |p| (p + shown).min(last)),
            KeyCode::Home | KeyCode::Enter => 0,
            KeyCode::End => last,
            _ => {
                self.lbterm_menu_stray(Slot::List, key, mods);
                return true;
            }
        };
        let line = takes[to];
        if let Some(list) = self.lbterm_agenda.list.as_mut() {
            list.focus = focus_of(&agenda, &laid.lines[line]);
            // Keep it on screen
            if line < list.scroll {
                list.scroll = line;
            } else if line >= list.scroll + shown {
                list.scroll = line + 1 - shown;
            }
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        true
    }

    /// Draw the list, if it's open: the tab menu's card, a row a line, the opened row's buttons
    pub(crate) fn paint_lbterm_agenda_list(&mut self) -> anyhow::Result<()> {
        if self.lbterm_agenda.list.is_none() {
            return Ok(());
        }
        let Some(agenda) = self.lbterm_agenda_now() else {
            return Ok(());
        };
        let Some(laid) = self.lbterm_agenda_list_layout(&agenda)? else {
            return Ok(());
        };
        let layout = &laid.list;
        // Scrolled past the end: held at the last
        if let Some(list) = self.lbterm_agenda.list.as_mut() {
            list.scroll = layout.scroll;
        }
        let (focus, chip, pressed) = match self.lbterm_agenda.list.as_ref() {
            Some(l) => (l.focus.clone(), l.chip, l.pressed.clone()),
            None => (None, None, vec![]),
        };
        let fill = self.lbterm_menu_fill();
        let card_rgb = fill.card;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let stroke = shape.hairline() as f64;
        let cell_w = laid.cell_w;

        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_h = metrics.cell_size.height as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        let mut words: Vec<super::words::Word> = vec![];
        let mut place = |text: &str, x: f64, y: f64, w: f64, colour: u32| {
            if !text.is_empty() && w > 0.0 {
                words.push((text.to_string(), euclid::rect(x as f32, y as f32, w as f32, line_h as f32), srgb(colour, 1.0)));
            }
        };
        let cols_in = |w: f64| (w / cell_w).floor().max(0.0) as usize;
        let mut lit = None;
        let opened = self.lbterm_agenda.view.row.clone();
        let mut band: Option<Rect> = None;
        for (li, r) in laid.shown() {
            let line = &laid.lines[li];
            let of_opened = match line {
                Line::Row { row, .. } | Line::Detail { row } | Line::Buttons { row } => {
                    agenda.rows.get(*row).map(|r| r.key()) == opened && opened.is_some()
                }
                _ => false,
            };
            if of_opened {
                band = Some(match band {
                    Some(b) => Rect { x: b.x, y: b.y, w: b.w, h: r.y + r.h - b.y },
                    None => *r,
                });
            }
            let y = r.y + (r.h - line_h) / 2.0;
            let mark_x = layout.card.x + 12.0 * scale;
            let text_x = layout.text_x;
            let project_x = text_x + laid.when_w;
            let title_x = project_x + laid.project_w;
            let right = r.x + r.w - 10.0 * scale;
            if focus.is_some() && focus_of(&agenda, line) == focus {
                lit = Some(*r);
            }
            match line {
                Line::Heading { section, count } => {
                    let title = section.title();
                    place(title, mark_x, y, right - mark_x, palette::LABEL);
                    let count_x = mark_x + (title.chars().count() as f64 + 2.0) * cell_w;
                    place(&count.to_string(), count_x, y, right - count_x, palette::PROJECT);
                }
                Line::Group { title, count, open, level, .. } => {
                    let chevron = if *open { "\u{25be}" } else { "\u{25b8}" };
                    let colour = if *level == agenda::Level::Urgent { agenda::URGENT } else { palette::LABEL };
                    place(chevron, mark_x, y, text_x - mark_x, colour);
                    let count = count.to_string();
                    let count_x = right - count.chars().count() as f64 * cell_w;
                    place(&strip::elide(title, cols_in(count_x - text_x - cell_w)), text_x, y, count_x - text_x, palette::TITLE);
                    // A box only as wide as its columns loses its last glyph to rounding
                    place(&count, count_x, y, right - count_x + cell_w, palette::PROJECT);
                }
                Line::Row { row, inner } => {
                    let Some(row) = agenda.rows.get(*row) else {
                        continue;
                    };
                    let dim = pressed.contains(&row.key());
                    let mark_at = if *inner { mark_x + cell_w * 0.5 } else { mark_x };
                    match agenda::mark(row) {
                        Some(colour) => place("\u{25cf}", mark_at, y, text_x - mark_at, colour),
                        None if *inner => place("\u{b7}", mark_at, y, text_x - mark_at, palette::PROJECT),
                        None => {}
                    }
                    place(&strip::elide(&row.when, cols_in(laid.when_w).saturating_sub(1)), text_x, y, laid.when_w, palette::PROJECT);
                    place(&strip::elide(&row.project, cols_in(laid.project_w).saturating_sub(1)), project_x, y, laid.project_w, palette::LABEL);
                    let colour = if dim { palette::PROJECT } else { palette::TITLE };
                    place(&strip::elide(&row.title, cols_in(right - title_x)), title_x, y, right - title_x, colour);
                }
                Line::Detail { row } => {
                    if let Some(row) = agenda.rows.get(*row) {
                        place(&strip::elide(&row.detail, cols_in(right - text_x)), text_x, y, right - text_x, palette::PROJECT);
                    }
                }
                Line::Buttons { .. } => {}
                Line::Filter { on, count } => {
                    let chevron = if *on { "\u{25be}" } else { "\u{25b8}" };
                    place(chevron, mark_x, y, text_x - mark_x, palette::LABEL);
                    let title = agenda::Section::Agents.title();
                    place(title, text_x, y, right - text_x, palette::LABEL);
                    let count_x = text_x + (title.chars().count() as f64 + 2.0) * cell_w;
                    place(&count.to_string(), count_x, y, right - count_x, palette::PROJECT);
                }
                Line::Empty => place(NOTHING, mark_x, y, right - mark_x, palette::PROJECT),
            }
        }
        for c in &laid.chips {
            let y = c.rect.y + (c.rect.h - line_h) / 2.0;
            let colour = if c.armed { agenda::URGENT } else { palette::TITLE };
            place(&c.label, c.rect.x + cell_w, y, c.rect.w - cell_w, colour);
        }

        // Its card, faded in and out with the menus' (render/menu_fade.rs): its lit row stays on
        // the card, under the opened row's buttons
        self.lbterm_menu_card(Slot::List, layout.card, fill)?;
        let layer = gl_state.layer_for_zindex(CARD_Z)?;
        let mut quads = layer.quad_allocator();
        let hover = palette::blend(card_rgb, palette::HOVER, palette::HOVER_ALPHA * 1.6);
        // The opened row with its detail and buttons, a shade lighter than the card, so they read
        // as one
        if let Some(b) = band {
            let shade = palette::blend(card_rgb, palette::HOVER, palette::HOVER_ALPHA * 0.8);
            self.lbterm_menu_rounded(&mut quads, b, 6.0 * scale, srgb(shade, 1.0))?;
        }
        if let Some(r) = lit {
            self.lbterm_menu_rounded(&mut quads, r, 6.0 * scale, srgb(hover, 1.0))?;
        }
        // The opened row's buttons: a hairline round each, filled lighter under the pointer, in
        // the waiting orange while one asks first
        let chip_radius = 6.0 * scale;
        for (i, c) in laid.chips.iter().enumerate() {
            let ring = if c.armed { agenda::URGENT } else { palette::OUTLINE };
            let fill = if chip == Some(i) { hover } else { card_rgb };
            self.lbterm_menu_rounded(&mut quads, grow(c.rect, stroke), chip_radius + stroke, srgb(ring, 1.0))?;
            self.lbterm_menu_rounded(&mut quads, c.rect, chip_radius, srgb(fill, 1.0))?;
        }
        drop(quads);
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_agenda.words, &font, &metrics, room, TEXT_Z, words)
    }
}

/// LBTerm: the agenda's button and list, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// What the button counts, while it shows
    pub(crate) fn lbterm_state_agenda(&self) -> Option<lbterm_extras::state::AgendaCounts> {
        self.lbterm_agenda
            .shown
            .map(|(urgent, failing, _)| lbterm_extras::state::AgendaCounts { urgent, failing })
    }

    /// The list while it's open: its lines top to bottom as it shows them ("Urgent (2)",
    /// "project: title", a folded group "title (12)", "detail: …", "Agents' to-dos (145)"), an
    /// opened row's buttons one item each ("button: Tick"), `checked` for what's opened
    pub(crate) fn lbterm_state_agenda_list(&self) -> Option<lbterm_extras::state::Menu> {
        let list = self.lbterm_agenda.list.as_ref()?;
        let agenda = self.lbterm_agenda_now()?;
        let laid = self.lbterm_agenda_list_layout(&agenda).ok().flatten();
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        let rect_of = |index: usize| {
            laid.as_ref()
                .and_then(|l| l.shown().find(|(i, _)| *i == index).map(|(_, r)| rect(r)))
        };
        let lines = match laid.as_ref() {
            Some(l) => l.lines.clone(),
            None => agenda::lines(&agenda, &self.lbterm_agenda.view),
        };
        let view = &self.lbterm_agenda.view;
        let mut items = vec![];
        let mut focus = None;
        for (i, line) in lines.iter().enumerate() {
            if let Line::Buttons { row } = line {
                let Some(row) = agenda.rows.get(*row) else {
                    continue;
                };
                for (index, (_, word)) in agenda::presses(row).into_iter().enumerate() {
                    let chip = laid.as_ref().and_then(|l| l.chips.iter().find(|c| c.index == index));
                    items.push(lbterm_extras::state::MenuItem {
                        label: format!("button: {}", chip.map_or(word, |c| c.label.clone())),
                        checked: chip.map_or(false, |c| c.armed),
                        key: None,
                        detail: None,
                        rect: chip.map(|c| rect(&c.rect)),
                        inert: false,
                    });
                }
                continue;
            }
            if list.focus.is_some() && focus_of(&agenda, line) == list.focus {
                focus = Some(items.len());
            }
            let checked = match line {
                Line::Group { open, .. } => *open,
                Line::Row { row, .. } => agenda.rows.get(*row).map_or(false, |r| view.row.as_ref() == Some(&r.key())),
                Line::Filter { on, .. } => *on,
                _ => false,
            };
            items.push(lbterm_extras::state::MenuItem {
                label: label(&agenda, line),
                checked,
                key: None,
                detail: None,
                rect: rect_of(i),
                inert: false,
            });
        }
        Some(lbterm_extras::state::Menu {
            kind: "agenda".to_string(),
            tab: None,
            rect: laid.as_ref().map(|l| rect(&l.list.card)),
            items,
            focus,
            renaming: None,
            waiting: None,
        })
    }

    /// What of the open list is at `x`, `y`, by its item in the state file's menu: `Some(Some(item))`
    /// on a line or a button, `Some(None)` on the card elsewhere, `None` off it
    pub(crate) fn lbterm_agenda_list_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        self.lbterm_agenda.list.as_ref()?;
        let menu = self.lbterm_state_agenda_list()?;
        let card = menu.rect.as_ref()?;
        let inside = |r: &lbterm_extras::state::Rect| {
            x >= r.x as f64 && x < (r.x + r.w as i32) as f64 && y >= r.y as f64 && y < (r.y + r.h as i32) as f64
        };
        if !inside(card) {
            return None;
        }
        Some(menu.items.iter().position(|item| item.rect.as_ref().map_or(false, inside)))
    }
}
