//! LBTerm: the tab menu (lbterm/TAB-MOUSE.md). A right-click on a tab, in the tab panel or the
//! tab bar, opens a card in the panel's style at the pointer: dark, ringed in the panel's red
//! light, with the panel's font. Its rows are lbterm_extras::menu's, from what's known of the tab
//! (`menu::Facts`, gathered as it opens): Rename tab (typed in place, from the label under the
//! pad), Colour (a row of swatches), Copy link (a Claude tab's), New session in <Project> (when
//! its folder is in one), Shell in its folder, Hide from Conductor or Show to Conductor (a Claude
//! tab's, while the Conductor's file has `hidden`: render/conductor_hide.rs), Wait on… (a page of
//! the other Claude tabs, each with its pad's colour, to make it wait on one), the tab's links,
//! Move to new window, Close finished tabs (with a second line naming them), Close other tabs,
//! Close tab. The mouse or the keyboard (a row's letter, a digit, arrows, Enter, Esc) picks; a
//! click outside or Esc closes it.
//!
//! It's drawn in z-layers 60 to 62, which the compositor gives a visual of its own over the panel
//! that fades in and out, its highlight gliding between rows (render/menu_fade.rs, `Slot::Tab`),
//! and takes every mouse event and key while it's open.

use crate::quad::{QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::render::menu_fade::{self, Slot};
use crate::termwindow::render::new_tab::white_over;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use lbterm_extras::conductor::Offer;
use lbterm_extras::menu::{self, Did, Item, Layout, Page};
use lbterm_extras::tabs::{Rect, COLOURS};
use lbterm_panel::palette;
use mux::pane::CachePolicy;
use mux::tab::TabId;
use mux::Mux;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// Its slot's card and words (render/menu_fade.rs): 60 is its glow's
const CARD_Z: i8 = Slot::Tab.card_z();
const TEXT_Z: i8 = Slot::Tab.text_z();
/// A row's key at its right: white at 35%, as the + menu's (lbterm/NEW-TAB.md, section 3).
/// Every dim word here is white blended into the card's colour and drawn opaque
/// (`new_tab::white_over`): the glyph shader takes a glyph's alpha from its coverage alone and
/// drops its colour's, so white at 35% given as a colour's alpha drew full white (PITFALLS 248;
/// the keys and Close finished tabs' second line did until 2026-11-09)
const KEY_ALPHA: f64 = 0.35;
/// Close finished tabs' second line: white at 50% (lbterm/NEW-TAB.md, section 7, row 10)
const SECOND_ALPHA: f64 = 0.5;
/// A greyed row's words (`menu::inert`: Hide from Conductor while a pick for the tab waits,
/// Lachlan's 11b of 2026-11-09): white at 50%, as the + menu's Starting row says what's pending
const INERT_ALPHA: f64 = super::new_tab::STARTING_ALPHA;
/// The corners of Wait on…'s pads, at 96 DPI: the rail's pads' look, smaller
const DOT_RADIUS: f64 = 3.0;

/// The menu while it's open
pub struct Open {
    tab: TabId,
    /// Where it was opened, in the window
    at: (f64, f64),
    /// Under the pointer, or where the arrow keys put it
    focus: Option<Item>,
    /// The new name being typed, after Rename tab
    renaming: Option<menu::Name>,
    /// The rows for the tab's links and connections, as it opened (tab_links.rs)
    links: Vec<crate::termwindow::tab_links::MenuLink>,
    /// What decides its rows, as it opened
    facts: menu::Facts,
    /// The tab's Claude session as it opened (render/tab_panel.rs, `tab_session`): Copy link's id
    session: Option<String>,
    /// The tab's folder as it opened (render/new_tab.rs, `tab_folder`), where New session in
    /// <Project> opens Claude
    dir: Option<String>,
    /// The menu, or Wait on…'s page
    page: Page,
    /// Its widest row as the title font lays it out, and a line's height, measured as it opened
    /// or turned its page (lbterm_menu_text)
    text: menu::Text,
    /// Close finished tabs' second line, the tabs it closes by label cut to the card's width as
    /// the title font lays it out, cut once as it opened (lbterm_menu_second)
    second: String,
}

#[derive(Default)]
pub struct TabMenuState {
    open: Option<Open>,
    /// The menu's words as its last frame laid them out (render/words.rs)
    pub(crate) words: std::cell::RefCell<super::words::Words>,
    /// What every menu drew this paint, for its fade and its highlight's glide
    /// (render/menu_fade.rs): kept here, with the tab menu, for all of them
    pub(crate) fades: std::cell::RefCell<super::menu_fade::MenuFades>,
    /// What the window last copied while the state file is on (Copy link, a selection, CopyTo,
    /// the Conductor's view): every copy goes there instead (`copied`, lbterm/STATE.md), since a
    /// test window shares Lachlan's clipboard (lbterm/PITFALLS.md, 151). A cell, as
    /// `copy_to_clipboard` has only `&self`
    pub(crate) copied: std::cell::RefCell<Option<String>>,
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn rect_f(r: Rect) -> window::RectF {
    euclid::rect(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

impl TermWindow {
    fn lbterm_menu_scale(&self) -> f64 {
        self.dimensions.dpi as f64 / 96.0
    }

    fn lbterm_menu_layout(&self, open: &Open) -> Layout {
        menu::layout(
            open.at,
            (
                self.dimensions.pixel_width as f64,
                self.dimensions.pixel_height as f64,
            ),
            self.lbterm_menu_scale(),
            &open.facts,
            open.page,
            open.text,
        )
    }

    pub(crate) fn lbterm_menu_is_open(&self) -> bool {
        self.lbterm_menu.open.is_some()
    }

    /// The open tab menu's card, for the tab panel behind it (render/tab_panel.rs)
    pub(crate) fn lbterm_tab_menu_card(&self) -> Option<Rect> {
        let open = self.lbterm_menu.open.as_ref()?;
        Some(self.lbterm_menu_layout(open).card)
    }

    /// Open the menu for `tab` at a point in the window
    pub(crate) fn lbterm_open_tab_menu(&mut self, tab: TabId, at: (f64, f64)) {
        let links = self.lbterm_link_menu(tab);
        // What decides the rows, read once as it opens and never on a pointer move: its Claude
        // session (Copy link), the project its folder is in, by the picker's name for it (New
        // session in <Project>), whether tab links are on, and the rail's tabs as its last paint
        // drew them (Wait on…'s other Claude tabs, the tabs Close finished tabs would close:
        // lbterm/TAB-MOUSE.md, "The tab menu's model"), and whether the Conductor's file lets it
        // be hidden from the Conductor or shown to it again (render/conductor_hide.rs)
        // And whether it's parked, where it can be: not the Conductor's tab, which its pad
        // stands for, nor one whose server can't park (park.rs; Lachlan's 8a)
        let conductor_tab = self.lbterm_panel.conductor_tab;
        let (session, dir, conductor, park) = match Mux::get().get_tab(tab) {
            Some(t) => (
                super::tab_panel::tab_session(&t),
                super::new_tab::tab_folder(&t),
                self.lbterm_hide_offer(&t),
                (Some(tab) != conductor_tab && crate::park::can_park(&t)).then(|| t.lbterm_parked()),
            ),
            None => (None, None, None, None),
        };
        let project = dir.as_deref().and_then(|d| self.lbterm_project_of(d)).map(|(_, name, _)| name);
        let rail = self.tab_panel_menu_rail();
        let facts = menu::Facts {
            conductor,
            park,
            ..menu::Facts::from_rail(
                tab,
                session.is_some(),
                project,
                self.config.lbterm_links.is_some(),
                links.iter().map(|l| l.label.clone()).collect(),
                &rail,
            )
        };
        // A link's row can name a task: the menu widens to fit its widest row
        let text = self.lbterm_menu_text(&facts, Page::Main);
        let second = self.lbterm_menu_second(&facts, at, text);
        self.lbterm_menu.open = Some(Open {
            tab,
            at,
            focus: None,
            renaming: None,
            links,
            facts,
            session,
            dir,
            page: Page::Main,
            text,
            second,
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// The menu's words as the title font lays them out: its widest row on `page`, measured once
    /// as it opens or turns the page (never characters times `cell_width`, PITFALLS 76), and a
    /// line's height
    fn lbterm_menu_text(&self, facts: &menu::Facts, page: Page) -> menu::Text {
        let Ok(font) = self.fonts.title_font() else {
            return menu::Text::default();
        };
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        menu::Text {
            widest: menu::widest(facts, page, self.lbterm_menu_scale(), |words| {
                self.lbterm_new_tab_width(&font, &metrics, words)
            }),
            line: metrics.cell_size.height as f64,
        }
    }

    /// Close finished tabs' second line, cut to `Layout::second`'s width by what the title font
    /// lays out (`menu::finished_line`, a few measures by halving, each kept by its words): once
    /// as the menu opens, never on a paint or a pointer move. Empty when the row isn't there
    fn lbterm_menu_second(&self, facts: &menu::Facts, at: (f64, f64), text: menu::Text) -> String {
        let size = (self.dimensions.pixel_width as f64, self.dimensions.pixel_height as f64);
        let layout = menu::layout(at, size, self.lbterm_menu_scale(), facts, Page::Main, text);
        let (Some(room), Ok(font)) = (layout.second, self.fonts.title_font()) else {
            return String::new();
        };
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        menu::finished_line(facts, room.w, |words| self.lbterm_new_tab_width(&font, &metrics, words))
    }

    pub(crate) fn lbterm_close_tab_menu(&mut self) {
        if self.lbterm_menu.open.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// While the menu's open it takes every mouse event: true when it did
    pub(crate) fn lbterm_tab_menu_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(open) = self.lbterm_menu.open.as_ref() else {
            return false;
        };
        let layout = self.lbterm_menu_layout(open);
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let hit = layout.hit(x, y);
        match event.kind {
            WMEK::Move => {
                let focus = hit.or(if open.renaming.is_some() { Some(Item::Rename) } else { None });
                if focus != open.focus {
                    if let Some(open) = self.lbterm_menu.open.as_mut() {
                        open.focus = focus;
                    }
                    context.lbterm_invalidate();
                }
                context.set_cursor(Some(if hit.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                }));
            }
            WMEK::Press(MousePress::Left) => match hit {
                Some(item) => {
                    self.lbterm_menu_flash_item(item);
                    self.lbterm_menu_pick(item);
                }
                None if layout.card.contains(x, y) => {}
                None => self.lbterm_close_tab_menu(),
            },
            WMEK::Press(_) => {
                if !layout.card.contains(x, y) {
                    self.lbterm_close_tab_menu();
                }
            }
            _ => {}
        }
        true
    }

    /// While the menu's open it takes every key: true when it did
    pub(crate) fn lbterm_tab_menu_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        let Some(open) = self.lbterm_menu.open.as_mut() else {
            return false;
        };
        if let Some(name) = open.renaming.as_mut() {
            match key {
                KeyCode::Escape => open.renaming = None,
                KeyCode::Enter => {
                    // Untouched, it stays as it was (a pane's title isn't pinned as the tab's);
                    // empty gives the tab back the title its program sets
                    let changed = !name.selected;
                    let name = name.text.trim().to_string();
                    let tab = open.tab;
                    self.lbterm_close_tab_menu();
                    if let Some(tab) = Mux::get().get_tab(tab).filter(|_| changed) {
                        tab.set_title(&name);
                        // LBTerm: its pad glints with the new name (render/tab_panel.rs)
                        self.lbterm_panel_glint_tab(tab.tab_id());
                    }
                    self.update_title();
                }
                KeyCode::Backspace | KeyCode::Delete => name.backspace(),
                KeyCode::LeftArrow | KeyCode::RightArrow | KeyCode::Home | KeyCode::End => name.deselect(),
                KeyCode::Char(c) if !mods.contains(KeyModifiers::CTRL) && !c.is_control() => {
                    name.type_char(*c);
                }
                _ => {}
            }
        } else {
            // Its letters and digits act here and nowhere else, and never reach the pane
            // (menu::answer: lbterm/NEW-TAB.md, sections 6 and 7)
            let all = menu::items(&open.facts, open.page);
            match menu::answer(menu_fade::menu_key(key, mods), open.page, &all, open.focus) {
                Did::Nothing => return true,
                Did::Focus(focus) => open.focus = focus,
                Did::Pick(item) => {
                    self.lbterm_menu_flash_item(item);
                    self.lbterm_menu_pick(item);
                    return true;
                }
                Did::Back => {
                    self.lbterm_menu_pick(Item::Back);
                    return true;
                }
                Did::Close => {
                    self.lbterm_close_tab_menu();
                    return true;
                }
                Did::Nowhere => {
                    self.lbterm_menu_bump(Slot::Tab);
                    return true;
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    /// Composed words (an IME's, a dead key's) while the tab menu is open: into the name while one
    /// is typed, else a key no row takes, its hairline brightening once. False when it isn't open
    pub(crate) fn lbterm_tab_menu_text(&mut self, words: &str) -> bool {
        let Some(open) = self.lbterm_menu.open.as_mut() else {
            return false;
        };
        let Some(name) = open.renaming.as_mut() else {
            self.lbterm_menu_bump(Slot::Tab);
            return true;
        };
        for c in words.chars().filter(|c| !c.is_control()) {
            name.type_char(c);
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    /// A row picked by a click, Enter, its letter or digit, or Delete: it flashes white as the pick
    /// goes (render/menu_fade.rs, `lbterm_menu_flash`), a swatch its own square. Not ‹ Back by Esc
    /// or Left, which picks no row
    fn lbterm_menu_flash_item(&self, item: Item) {
        let Some(open) = self.lbterm_menu.open.as_ref() else {
            return;
        };
        if let Some(row) = self.lbterm_menu_layout(open).flash_box(item) {
            self.lbterm_menu_flash(Slot::Tab, row);
        }
    }

    /// Do what the item says, for the menu's tab
    fn lbterm_menu_pick(&mut self, item: Item) {
        let Some(tab_id) = self.lbterm_menu.open.as_ref().map(|open| open.tab) else {
            return;
        };
        // The tab's row on the rail, for a row's "tab N" when it has no title at all: the rail
        // numbers its rows without the Conductor's tab (panel_tab's `idx`), so the window's own
        // index would name a title-less tab after the Conductor's one higher than its row does
        let idx = self.tab_panel_row_of(tab_id).unwrap_or(0);
        let Some(open) = self.lbterm_menu.open.as_mut() else {
            return;
        };
        let mux = Mux::get();
        let Some(tab) = mux.get_tab(tab_id) else {
            self.lbterm_close_tab_menu();
            return;
        };
        match item {
            Item::Rename => {
                // The name it shows under its pad: the name it was given, else its session's
                // label, else its title as its row reads it (lbterm/NEW-TAB.md, section 7, row 1)
                let shown = super::tab_panel::tab_label(&tab, idx);
                open.renaming = Some(menu::Name::new(&shown));
                open.focus = Some(Item::Rename);
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
                return;
            }
            Item::Colour(colour) => {
                crate::tabcolors::tidy();
                crate::tabcolors::set(tab_id, colour);
            }
            Item::Shell => {
                // A new tab in the same folder and domain, beside it, the Starting row at the end
                // of the tabs until it comes (render/new_tab.rs)
                if let Some(pane) = tab.get_active_pane() {
                    self.lbterm_new_tab_expect_shell();
                    let cwd = pane
                        .get_current_working_dir(CachePolicy::AllowStale)
                        .and_then(|url| url.to_file_path().ok());
                    self.spawn_command(
                        &SpawnCommand {
                            cwd,
                            domain: SpawnTabDomain::DomainId(pane.domain_id()),
                            ..Default::default()
                        },
                        crate::spawn::SpawnWhere::NewTab,
                    );
                }
            }
            Item::NewWindow => self.lbterm_tear_off(tab_id, None),
            Item::CloseOthers => self.lbterm_close_other_tabs(tab_id),
            // Picked from the menu it closes without asking, whatever runs in it: picking it is
            // the answer (Lachlan, 2026-10-25). The last tab gives way to a new one while the
            // Conductor's pad stays (termwindow/last_tab.rs). Its Claude sessions are marked closed
            // by hand first
            Item::Close => {
                self.lbterm_mark_tab(tab_id);
                self.lbterm_remove_tab(tab_id)
            }
            // One of the tab's links or connections (tab_links.rs)
            Item::Link(i) => {
                if let Some(l) = open.links.get(i).cloned() {
                    self.lbterm_link_menu_fire(l.act);
                }
            }
            // Wait on…'s page, and back: the card stays open, its rows dissolving into the other
            // page's on `swap` (render/menu_fade.rs, `lbterm_menu_turn`)
            Item::WaitOn | Item::Back => {
                let page = if item == Item::WaitOn { Page::WaitOn } else { Page::Main };
                let facts = open.facts.clone();
                let text = self.lbterm_menu_text(&facts, page);
                if let Some(open) = self.lbterm_menu.open.as_mut() {
                    open.page = page;
                    open.focus = None;
                    open.text = text;
                }
                self.lbterm_menu_turn(Slot::Tab);
                return;
            }
            // Another Claude tab on Wait on…'s page: the menu's tab waits on it, or stops waiting
            // there if it did, as a right drag let go on its row does (tab_links.rs)
            Item::On(i) => {
                let waiter = open.session.clone();
                let on = open
                    .facts
                    .others
                    .get(i)
                    .and_then(|o| mux.get_tab(o.tab))
                    .and_then(|t| super::tab_panel::tab_session(&t));
                if let (Some(waiter), Some(on)) = (waiter, on) {
                    self.lbterm_link_wait_on(waiter, on);
                }
            }
            // Every finished tab it listed that's finished still, at once and without asking, each
            // kept to reopen with its close marks written (render/tab_menu.rs, lbterm_close_finished)
            Item::CloseFinished => {
                let listed = open.facts.finished.clone();
                self.lbterm_close_tab_menu();
                self.lbterm_close_finished(&listed);
                return;
            }
            // `[<label>](lbterm://session/<id>) in <Project>` on the clipboard, the label as the
            // row shows it now, and the pad glints (copy_in, copy_out)
            Item::CopyLink => {
                if let Some(sid) = open.session.clone() {
                    let label = super::tab_panel::tab_label(&tab, idx);
                    let text = menu::copy_link(&label, &sid, open.facts.project.as_deref());
                    self.copy_to_clipboard(
                        config::keyassignment::ClipboardCopyDestination::Clipboard,
                        text,
                    );
                    self.lbterm_panel_glint_tab(tab_id);
                }
            }
            // Hide from Conductor or Show to Conductor: the menu fades as it goes (pane_exit), the
            // row takes the look asked for from the next paint (`swap`), and the config is asked
            // to tell the Conductor (render/conductor_hide.rs)
            // Greyed while a pick for the tab waits (Offer::Waiting): nothing reaches it then, the
            // keys and the pointer passing over it (`menu::inert`), so only Hide and Show pick
            // Park tab or Unpark tab: the menu fades as it goes (pane_exit), and the row dims or
            // brightens on `swap` once the flag changes, at once on the GUI's own tab, at the
            // server's word on a mux domain's (park.rs)
            Item::Park => {
                let parked = open.facts.park.unwrap_or(false);
                self.lbterm_close_tab_menu();
                crate::park::set_parked(&tab, !parked);
                return;
            }
            Item::Hide => match { open.facts.conductor } {
                Some(offer @ (Offer::Hide | Offer::Show)) => {
                    let label = super::tab_panel::tab_label(&tab, idx);
                    self.lbterm_close_tab_menu();
                    self.lbterm_conductor_hide(&tab, label, offer == Offer::Hide);
                    return;
                }
                Some(Offer::Waiting { .. }) => return,
                None => {}
            },
            // Claude in the tab's folder, in front: the menu fades as it goes (pane_exit), then
            // it's a pick, its Starting row up from the next frame (render/new_tab.rs)
            Item::NewSession => {
                if let Some(dir) = open.dir.clone() {
                    let pane = tab.get_active_pane().map(|p| p.pane_id());
                    self.lbterm_close_tab_menu();
                    self.lbterm_new_tab_session_in(dir, pane);
                    return;
                }
            }
        }
        self.lbterm_close_tab_menu();
    }

    /// The open menu's Hide from Conductor row asked again when a pick for its tab `tab` has
    /// begun or had the Conductor's word (render/conductor_hide.rs): the greyed "Hiding from
    /// Conductor…" comes alive in its place as Show to Conductor once the Conductor says it, or
    /// as it was once 5 s have gone (Lachlan's 11b of 2026-11-09). Only the row's words and
    /// whether it picks change: a row the menu didn't open with isn't added and one it did isn't
    /// taken away, so the card never jumps under the pointer
    pub(crate) fn lbterm_menu_hide_again(&mut self, tab: TabId) {
        let Some(open) = self.lbterm_menu.open.as_ref().filter(|open| open.tab == tab) else {
            return;
        };
        let Some(t) = Mux::get().get_tab(tab) else {
            return;
        };
        let offer = self.lbterm_hide_offer(&t);
        if offer.is_none() || open.facts.conductor.is_none() || offer == open.facts.conductor {
            return;
        }
        let page = open.page;
        let at = open.at;
        let facts = menu::Facts {
            conductor: offer,
            ..open.facts.clone()
        };
        // Its words measured again, as on opening (a greyed row's are a little longer)
        let text = (page == Page::Main).then(|| self.lbterm_menu_text(&facts, page));
        let second = text.map(|text| self.lbterm_menu_second(&facts, at, text));
        log::info!("tab menu: tab {tab}'s Conductor row is now {offer:?}");
        if let Some(open) = self.lbterm_menu.open.as_mut() {
            open.facts.conductor = offer;
            if let (Some(text), Some(second)) = (text, second) {
                open.text = text;
                open.second = second;
            }
            if open.focus.is_some_and(|f| menu::inert(f, &open.facts)) {
                open.focus = None;
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A copy the window makes while the state file is on goes to the state file (`copied`,
    /// lbterm/STATE.md) and never to the clipboard: a test window shares Lachlan's, and on Windows
    /// every kind of clipboard is that one, PrimarySelection too (lbterm/PITFALLS.md, 151). Called
    /// first by `copy_to_clipboard` (termwindow/clipboard.rs), so every copy a test makes, Copy
    /// link, a selection's let-go, CopyTo and the Conductor's view, is kept from his clipboard
    /// whatever the test's config binds. True when it took the copy
    pub(crate) fn lbterm_copy_to_state(&self, text: &str) -> bool {
        if !crate::state_probe::on() {
            return false;
        }
        log::info!("copied {} characters to the state file", text.chars().count());
        *self.lbterm_menu.copied.borrow_mut() = Some(text.to_string());
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    /// Close finished tabs (lbterm/NEW-TAB.md, section 7, row 10): every tab the row listed as the
    /// menu opened that still qualifies on the rail now (`menu::to_close`), at once, without
    /// asking, in the rail's order. Each is marked closed by hand and kept to reopen
    /// (`lbterm_mark_tab`), then removed (`lbterm_remove_tab`, which gives the last tab's place to
    /// a new one while the Conductor's pad stays). All go in this one turn of the GUI, so the
    /// rail's next paint has every row gone at once: each fades where it was on `pane_exit` while
    /// the rows below slide up on one spring (`pane_layout`), and when the open tab was one, the
    /// mux makes another active, its panes dissolving in (`tab_switch`) and the marker gliding to
    /// its row (`focus_move`): the rail's own motion for a tab closed (lbterm/TAB-PANEL.md)
    fn lbterm_close_finished(&mut self, listed: &[menu::Named]) {
        let rail = self.tab_panel_menu_rail();
        let close = menu::to_close(listed, &rail);
        log::info!(
            "tab menu: Close finished tabs closes {} of the {} it listed",
            close.len(),
            listed.len()
        );
        for tab in close {
            self.lbterm_mark_tab(tab);
            self.lbterm_remove_tab(tab);
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Close every tab in this window but `keep`, at once, whatever runs in them: picking it from
    /// the menu is the answer, as for Close tab (Lachlan, 2026-10-25)
    fn lbterm_close_other_tabs(&mut self, keep: TabId) {
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            return;
        };
        // The Conductor's tab is its pad's, not one of the rail's: it stays
        let conductor = self.lbterm_panel.conductor_tab;
        let others: Vec<TabId> = window
            .iter_tabs()
            .map(|t| t.tab_id())
            .filter(|&id| id != keep && Some(id) != conductor)
            .collect();
        drop(window);
        for id in others {
            // Each one's Claude sessions marked closed by hand (termwindow/last_tab.rs)
            self.lbterm_mark_tab(id);
            mux.remove_tab(id);
        }
    }

    /// A rounded rectangle in one colour, in `quads`' layer 0
    pub(crate) fn lbterm_menu_rounded(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Rect,
        radius: f64,
        colour: LinearRgba,
    ) -> anyhow::Result<()> {
        if area.w <= 0.0 || area.h <= 0.0 {
            return Ok(());
        }
        let r = radius.min(area.w / 2.0).min(area.h / 2.0).floor() as f32;
        let (x, y, w, h) = (area.x as f32, area.y as f32, area.w as f32, area.h as f32);
        if r < 1.0 {
            self.filled_rectangle(quads, 0, euclid::rect(x, y, w, h), colour)?;
            return Ok(());
        }
        let size = euclid::size2(r, r);
        for (poly, cx, cy) in [
            (TOP_LEFT_ROUNDED_CORNER, x, y),
            (TOP_RIGHT_ROUNDED_CORNER, x + w - r, y),
            (BOTTOM_LEFT_ROUNDED_CORNER, x, y + h - r),
            (BOTTOM_RIGHT_ROUNDED_CORNER, x + w - r, y + h - r),
        ] {
            self.poly_quad(quads, 0, euclid::point2(cx, cy), poly, 1, size, colour)?
                .set_grayscale();
        }
        self.filled_rectangle(quads, 0, euclid::rect(x + r, y, w - 2.0 * r, h), colour)?;
        self.filled_rectangle(quads, 0, euclid::rect(x, y + r, r, h - 2.0 * r), colour)?;
        self.filled_rectangle(quads, 0, euclid::rect(x + w - r, y + r, r, h - 2.0 * r), colour)?;
        Ok(())
    }

    /// Draw the menu, if it's open
    pub(crate) fn paint_lbterm_tab_menu(&mut self) -> anyhow::Result<()> {
        let Some(open) = self.lbterm_menu.open.as_ref() else {
            return Ok(());
        };
        let tab_id = open.tab;
        if Mux::get().get_tab(tab_id).is_none() {
            self.lbterm_menu.open = None;
            return Ok(());
        }
        let focus = open.focus;
        let renaming = open.renaming.clone();
        let layout = self.lbterm_menu_layout(open);
        let links = open.links.clone();
        let facts = open.facts.clone();
        let second = open.second.clone();
        let scale = layout.scale;
        let fill = self.lbterm_menu_fill();
        let card_rgb = fill.card;
        let gl_state = self.render_state.as_ref().unwrap();

        // The words, laid out first so a selected name's width is known for its highlight
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        // Each laid out only when the menu's last frame didn't have it (render/words.rs)
        let mut kept = self.lbterm_menu.words.borrow_mut();
        kept.at(self.lbterm_words_at());
        let mut words = vec![];
        let mut selection = None;
        for (row, r) in &layout.rows {
            let Some(item) = row else { continue };
            // Its words, leaving the key's room at the right (the Colour row's, the swatches')
            let text_rect = layout.words_box(*item, *r);
            let (text, bounds, colour) = match (item, &renaming) {
                // The name as it's typed: all of it selected at first, else with a caret
                (Item::Rename, Some(name)) if name.selected => (name.text.clone(), text_rect, palette::TITLE),
                (Item::Rename, Some(name)) => (format!("{}\u{258f}", name.text), text_rect, palette::TITLE),
                (Item::Colour(_), _) => (menu::label(*item, &facts), text_rect, palette::PROJECT),
                (Item::Link(i), _) => (
                    links.get(*i).map(|l| l.label.clone()).unwrap_or_default(),
                    text_rect,
                    palette::TITLE,
                ),
                _ => (menu::label(*item, &facts), text_rect, palette::TITLE),
            };
            // A greyed row says what it waits for, dim (opaque, over the card: PITFALLS 248), and
            // draws no key
            let colour = if menu::inert(*item, &facts) { white_over(card_rgb, INERT_ALPHA) } else { colour };
            let (key, computed) = self.lbterm_take_word(&mut kept, &font, &metrics, room, TEXT_Z, (text, rect_f(bounds), srgb(colour, 1.0)))?;
            if matches!((item, &renaming), (Item::Rename, Some(name)) if name.selected) {
                let width = (computed.content_rect.width() as f64).min(text_rect.w);
                selection = Some(Rect {
                    x: text_rect.x - 3.0 * scale,
                    y: r.y + 5.0 * scale,
                    w: width + 6.0 * scale,
                    h: r.h - 10.0 * scale,
                });
            }
            words.push((key, computed));
            // Its key, dim at its right: not while the name it would start is being typed
            let typing = renaming.is_some() && *item == Item::Rename;
            if let Some(letter) = menu::shown_key(*item, &facts).filter(|_| !typing) {
                let letter = letter.to_string();
                let kb = layout.key_box(*r);
                let width = self.lbterm_new_tab_width(&font, &metrics, &letter);
                let bounds = Rect { x: kb.x + kb.w - width, w: width + room.0 as f64, ..kb };
                words.push(self.lbterm_take_word(&mut kept, &font, &metrics, room, TEXT_Z, (letter, rect_f(bounds), srgb(white_over(card_rgb, KEY_ALPHA), 1.0)))?);
            }
            // Close finished tabs' second line: the tabs it closes by label, white at 50%
            if let (Item::CloseFinished, Some(line)) = (item, layout.second) {
                if !second.is_empty() {
                    words.push(self.lbterm_take_word(&mut kept, &font, &metrics, room, TEXT_Z, (second.clone(), rect_f(line), srgb(white_over(card_rgb, SECOND_ALPHA), 1.0)))?);
                }
            }
        }

        // The light round it, the ring in the panel's red and the card: every menu's, faded in
        // and out with its highlight and words (render/menu_fade.rs)
        self.lbterm_menu_card(Slot::Tab, layout.card, fill)?;
        let layer = gl_state.layer_for_zindex(CARD_Z)?;
        let mut quads = layer.quad_allocator();

        // The row under the pointer or the keys: the menus' highlight, gliding between rows. The
        // name being typed keeps its row lit on the card, under its selection
        let hover = srgb(palette::blend(card_rgb, palette::HOVER, menu_fade::LIGHT_LEVEL), 1.0);
        // The Colour row lit (Lachlan's 12 of 2026-11-08: the highlight covers it too when the
        // keys or the pointer land on a swatch, as on every other row), so without the compositor
        // the swatches' rings are cut out of the lit row's colour rather than the card's
        let mut swatch_under = card_rgb;
        for (row, r) in &layout.rows {
            match row {
                None => {
                    // The rule
                    let line = Rect {
                        x: r.x + 8.0 * scale,
                        y: r.y + r.h / 2.0,
                        w: r.w - 16.0 * scale,
                        h: scale.max(1.0),
                    };
                    self.filled_rectangle(&mut quads, 0, rect_f(line), srgb(palette::OUTLINE, 0.6))?;
                }
                Some(item) => {
                    // Every swatch is on the Colour row (`same_row`), so a swatch with the keys or
                    // under the pointer lights the whole row; until 2026-11-08 the row never lit,
                    // the swatch's ring alone saying where the keys were, and the light went out
                    let lit = focus.is_some_and(|b| menu::same_row(*item, b));
                    if renaming.is_some() && *item == Item::Rename {
                        self.lbterm_menu_rounded(&mut quads, *r, 6.0 * scale, hover)?;
                    } else if lit {
                        self.lbterm_menu_light(Slot::Tab, &mut quads, *r, card_rgb)?;
                        if matches!(item, Item::Colour(_)) && !self.lbterm_composited() {
                            swatch_under = palette::blend(card_rgb, palette::HOVER, menu_fade::LIGHT_LEVEL);
                        }
                    }
                    // Wait on…'s tabs: each one's pad, a small rounded square in the colour the
                    // rail draws it now, before its label
                    if let (Item::On(i), Some(dot)) = (item, layout.dot_box(*item, *r)) {
                        if let Some(other) = facts.others.get(*i) {
                            self.lbterm_menu_rounded(&mut quads, dot, DOT_RADIUS * scale, srgb(other.colour, 1.0))?;
                        }
                    }
                }
            }
        }
        // A selected name, in the panel's red
        if let Some(s) = selection {
            self.lbterm_menu_rounded(&mut quads, s, 4.0 * scale, srgb(palette::RING_MIDDLE, 0.45))?;
        }

        // The swatches: "no colour" as a hollow ring, then each colour; the tab's own ringed
        let current = crate::tabcolors::get(tab_id);
        for (colour, r) in &layout.swatches {
            let focused = focus == Some(Item::Colour(*colour));
            let chosen = current == *colour;
            if focused || chosen {
                let ring = if chosen { palette::TITLE } else { palette::PROJECT };
                self.lbterm_menu_rounded(&mut quads, grow(*r, 2.5 * scale), r.w, srgb(ring, 0.9))?;
                self.lbterm_menu_rounded(&mut quads, grow(*r, 1.0 * scale), r.w, srgb(swatch_under, 1.0))?;
            }
            match colour {
                Some(i) => {
                    let (cr, cg, cb) = COLOURS[*i].1;
                    self.lbterm_menu_rounded(&mut quads, *r, r.w / 2.0, LinearRgba::with_srgba(cr, cg, cb, 255))?;
                }
                None => {
                    self.lbterm_menu_rounded(&mut quads, *r, r.w / 2.0, srgb(palette::OUTLINE, 1.0))?;
                    self.lbterm_menu_rounded(&mut quads, grow(*r, -1.5 * scale), r.w / 2.0, srgb(swatch_under, 1.0))?;
                }
            }
        }
        drop(quads);

        self.lbterm_draw_taken(&mut kept, TEXT_Z, &words)?;
        kept.keep(words);
        Ok(())
    }
}

/// LBTerm: the tab menu, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// The menu while it's open: its items, the one picked by the pointer or the keys, and the
    /// name being typed
    pub(crate) fn lbterm_state_tab_menu(&self) -> Option<lbterm_extras::state::Menu> {
        let open = self.lbterm_menu.open.as_ref()?;
        let layout = self.lbterm_menu_layout(open);
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        let rects: Vec<&Rect> = layout.rows.iter().filter(|(item, _)| item.is_some()).map(|(_, r)| r).collect();
        // Every row the card shows, a greyed one (`inert`) included, so each lines up with its rect
        let items = menu::shown(&open.facts, open.page);
        let focus = open.focus.and_then(|f| items.iter().position(|item| menu::same_row(*item, f)));
        Some(lbterm_extras::state::Menu {
            kind: "tab".to_string(),
            tab: Some(open.tab as u64),
            rect: Some(rect(&layout.card)),
            items: items
                .iter()
                .enumerate()
                .map(|(i, &item)| lbterm_extras::state::MenuItem {
                    label: match item {
                        Item::Link(i) => open.links.get(i).map(|l| l.label.clone()).unwrap_or_default(),
                        _ => menu::label(item, &open.facts),
                    },
                    checked: false,
                    key: menu::shown_key(item, &open.facts).map(String::from),
                    detail: (item == Item::CloseFinished && !open.second.is_empty()).then(|| open.second.clone()),
                    rect: rects.get(i).map(|r| rect(r)),
                    inert: menu::inert(item, &open.facts),
                })
                .collect(),
            focus,
            renaming: open.renaming.as_ref().map(|name| name.text.clone()),
            waiting: None,
        })
    }

    /// What of the open menu is at `x`, `y`: `Some(Some(item))` on an item, `Some(None)` on the
    /// card elsewhere, `None` off it
    pub(crate) fn lbterm_tab_menu_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        let open = self.lbterm_menu.open.as_ref()?;
        let layout = self.lbterm_menu_layout(open);
        if !layout.card.contains(x, y) {
            return None;
        }
        Some(
            layout
                .rows
                .iter()
                .filter(|(item, _)| item.is_some())
                .position(|(_, r)| r.contains(x, y)),
        )
    }
}
