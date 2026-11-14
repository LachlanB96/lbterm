//! LBTerm: the new-tab pad at the tab rail's foot, its picker and its + menu (lbterm/NEW-TAB.md,
//! landing 1). The logic (matching like @names, the order, the layouts, the keys, what's kept,
//! the Starting row's wait) is `lbterm_extras::newtab`; the drawing, the keys, the mouse and what
//! a pick fires come here. The pad's slot and its look at rest are the tab panel's
//! (render/tab_panel.rs, `Layout::foot`), as is the Starting row it draws at the end of the tabs.
//!
//! - The pad: a hairline pill with a + in it. Under the pointer, or while its picker or menu is
//!   open, the hairline and the + brighten: that look is drawn in [`PLUS_HOVER_Z`], a visual of its
//!   own with the compositor, faded on `button_hover` (`Compositor::prepare_new_tab`).
//! - A left click, or Ctrl+T (`LbtermNewTab 'Picker'`): the picker, beside the rail, its box level
//!   with the pad. Typing finds a project; Enter opens Claude there in front, Shift+Enter behind,
//!   Ctrl+Enter a shell. It fires `lbterm-project-action` with the project and what to do, and
//!   without a handler returning false opens the default program in its folder. It takes every
//!   key while it's open, and the panes dim as when the window loses the keyboard.
//! - A right click, as the button comes up: the + menu, upward from the pointer: the launch
//!   menu's shells here, Shell in a project… (the picker titled "Shell in…"), the tabs closed
//!   lately, New window.
//! - Every tab asked for in the window (a pick, a shell, Lua's `window:lbterm_expect_tab`) puts
//!   the Starting row at the end of the tabs until the first tab the window didn't have comes,
//!   which it dissolves into, or 20 s are up, when it folds and the strip says nothing opened.
//!   Several can be on their way at once (Shift+Enter keeps the picker open): they wait in order
//!   (`newtab::Starts`), the rail shows the oldest's row, and each tab that comes is the oldest's.

use crate::scripting::guiwin::GuiWin;
use crate::termwindow::box_model::{Element, ElementContent};
use crate::termwindow::{TermWindow, TermWindowNotif};
use crate::termwindow::render::menu_fade::{self, MenuFill, Slot};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, Modifiers, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps, WindowState};
use config::keyassignment::{ClipboardPasteSource, KeyAssignment, SpawnCommand, SpawnTabDomain};
use config::lbterm::LbtermNewTab;
use config::TermConfig;
use lbterm_extras::newtab::{self, Chip, KeyWords, Listed, Mode, Pick, PickerDid, PickerKey, PickerLayout, PickerState, PlusItem};
use lbterm_extras::projects::{self, Counts};
use lbterm_extras::strip::{self, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::{PixelRect, Target};
use lbterm_panel::{palette, Area, Layout};
use mux::pane::{CachePolicy, PaneId};
use mux::tab::TabId;
use mux::Mux;
use mux_lua::MuxPane;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;
use wezterm_font::LoadedFont;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// The picker beside the rail: its glow, its card and its words, in the compositor's `picker`
/// visual, faded in and out by DWM (`Compositor::prepare_new_tab`)
pub const PICKER_GLOW_Z: i8 = 93;
pub const PICKER_CARD_Z: i8 = 94;
pub const PICKER_TEXT_Z: i8 = 95;
/// The + pad's look under the pointer, in the compositor's `plus` visual, faded on `button_hover`
pub const PLUS_HOVER_Z: i8 = 96;
/// The + menu's card and words: the InMail list's and the agenda's z-layers, their slot in the
/// menus' visuals (render/menu_fade.rs, `Slot::List`), 77 its glow's. Only one of the three is
/// ever open: each takes every click and key while it is, and closes on one elsewhere.
const PLUS_CARD_Z: i8 = Slot::List.card_z();
const PLUS_TEXT_Z: i8 = Slot::List.text_z();

/// The Starting row's key among the rail's rows for the compositor: no tab has it
pub const STARTING_KEY: TabId = TabId::MAX - 1;

/// The visual that draws z-layer `zindex`, when it's one of the new-tab pad's
pub fn target_of(zindex: i8) -> Option<Target<PaneId>> {
    match zindex {
        PICKER_GLOW_Z | PICKER_CARD_Z | PICKER_TEXT_Z => Some(Target::Picker),
        PLUS_HOVER_Z => Some(Target::PlusHover),
        _ => None,
    }
}

/// White over the card, as the eye sees it (lbterm/NEW-TAB.md, "The build spec")
const NAME_ALPHA: f64 = 0.92;
const QUIET_ALPHA: f64 = 0.60;
const DIM_ALPHA: f64 = 0.35;
const MESSAGE_ALPHA: f64 = 0.60;
const BOX_FILL: f64 = 0.06;
const BOX_LINE: f64 = 0.12;
const BOX_LINE_KEYS: f64 = 0.24;
const CARET_ALPHA: f64 = 0.90;
const KEY_CHIP_ALPHA: f64 = 0.07;
const CHIP_ALPHA: f64 = 0.12;
const CHIP_LIT_ALPHA: f64 = 0.22;
/// The pad: its hairline and its + at rest, and lit
pub(crate) const PAD_LINE: f64 = 0.14;
pub(crate) const PAD_PLUS: f64 = 0.45;
const PAD_LINE_LIT: f64 = 0.50;
const PAD_PLUS_LIT: f64 = 1.0;
/// The open panel's row for the pad: "New session", and its key
pub(crate) const FOOT_WORDS: &str = "New session";
pub(crate) const FOOT_WORDS_ALPHA: f64 = 0.85;
pub(crate) const FOOT_KEY_ALPHA: f64 = 0.35;
/// "+N" over the pad, for tabs the rail has no room for
pub(crate) const HIDDEN_ALPHA: f64 = 0.50;
/// The Starting row's pad, and its words on the open panel
pub(crate) const STARTING_PAD: u32 = 0x24242C;
pub(crate) const STARTING_ALPHA: f64 = 0.50;

/// What the picker lists from: the projects' names, the order with nothing typed, and each
/// project's sessions and mail
struct Base {
    names: Vec<String>,
    /// Each project's own folder, as the config spells it: what a pick hands Lua as `project_dir`
    dirs: Vec<String>,
    /// Each project's other names, taken whole (the root's AI and ~)
    aliases: Vec<Vec<String>>,
    entries: Vec<newtab::Entry>,
    counts: Vec<Counts>,
}

impl Base {
    fn names(&self) -> Vec<&str> {
        self.names.iter().map(String::as_str).collect()
    }

    fn listed(&self, query: &str) -> Listed {
        newtab::listed(query, &self.names(), &self.aliases, &self.entries)
    }

    fn mail(&self, project: usize) -> usize {
        self.counts.get(project).map_or(0, |c| c.mail)
    }
}

struct OpenPicker {
    state: PickerState,
    /// The last frame painted as it opened, and the first that drew it (the state file's)
    asked: u64,
    shown: Option<u64>,
    /// The window's open tab as it opened: it closes when that changes
    tab: Option<TabId>,
    /// The chip on the highlighted row under the pointer
    chip: Option<Chip>,
}

/// The picker as last laid out: where everything is, what it lists, and the words it draws
struct Drawn {
    layout: PickerLayout,
    listed: Listed,
    highlight: Option<usize>,
    chips: Vec<(Chip, Rect)>,
    chip: Option<Chip>,
    /// Each row shown's key chip, by its rect
    key_chips: Vec<Rect>,
    /// The box's hairline: with the keyboard brighter
    keys: bool,
    caret: Option<Rect>,
    words: Vec<(String, Rect, u32, f64)>,
}

struct OpenPlus {
    at: (f64, f64),
    focus: Option<PlusItem>,
    rows: Vec<Option<PlusItem>>,
    labels: Vec<String>,
    keys: Vec<Option<char>>,
}

#[derive(Default)]
pub struct NewTabState {
    picker: Option<OpenPicker>,
    base: Option<Rc<Base>>,
    /// The picker as last laid out: what the mouse and the keys go by, and what a closing one
    /// fades out showing
    drawn: Option<Rc<Drawn>>,
    picker_visual: Option<(PixelRect, bool)>,
    plus: Option<OpenPlus>,
    /// The pointer is on the pad's zone, and the button pressed on it
    pad_hovered: bool,
    /// Lit at the last paint, and when it stopped being: its look fades out on `button_hover`
    /// from then, and its visual is put away once that's done, so at rest nothing of it is drawn
    pad_lit: bool,
    unlit_at: Option<Instant>,
    pad_pressed: Option<MousePress>,
    pad_visual: Option<(PixelRect, bool)>,
    /// The tabs on their way, oldest first: the rail shows the oldest's Starting row
    pub(crate) starts: newtab::Starts,
    /// The tabs asked for lately, by frames, oldest first and kept after they're done (the state
    /// file's `starts`, `start` the last), and the next start's id
    records: Vec<lbterm_extras::state::StartSaid>,
    next_start: u64,
    /// The keys bound to the picker, a shell here and Reopen, as the pad's row and the + menu say
    /// them (`lbterm_new_tab_keys`, as the key table is made again)
    pub(crate) keys: KeyWords,
    /// Tabs closed lately, newest first, to reopen (lbterm/NEW-TAB.md, "Reopen")
    pub(crate) closed: Vec<newtab::Closed>,
    /// When a repaint is already on its way, for a fade's end
    wake: Option<Instant>,
    words: RefCell<super::words::Words>,
    plus_words: RefCell<super::words::Words>,
    /// Words' widths in the frame's font, by their text, the font and the shape generation
    widths: RefCell<std::collections::HashMap<(String, usize, usize), f64>>,
}

impl NewTabState {
    /// Its words point into a glyph atlas that's gone (gpu_lost.rs)
    pub(crate) fn forget_words(&mut self) {
        self.words.get_mut().forget();
        self.plus_words.get_mut().forget();
        self.widths.get_mut().clear();
    }
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// White at `alpha` over `under`, opaque
pub(crate) fn white_over(under: u32, alpha: f64) -> u32 {
    palette::blend(under, 0xFFFFFF, alpha)
}

fn rect_f(r: Rect) -> window::RectF {
    euclid::rect(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect { x: r.x - by, y: r.y - by, w: r.w + 2.0 * by, h: r.h + 2.0 * by }
}

fn area(r: Rect) -> Area {
    Area::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
}

fn pixels(a: Area) -> PixelRect {
    let (x0, y0) = (a.x.floor(), a.y.floor());
    let (x1, y1) = ((a.x + a.w).ceil(), (a.y + a.h).ceil());
    PixelRect::new(x0 as i32, y0 as i32, (x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32)
}

fn state_rect(r: &Rect) -> lbterm_extras::state::Rect {
    lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h)
}

fn group_name(g: newtab::Group) -> &'static str {
    match g {
        newtab::Group::Here => "here",
        newtab::Group::Home => "home",
        newtab::Group::Recent => "recent",
        newtab::Group::All => "all",
    }
}

/// A project's sessions and mail at a row's right, as the projects card shows them: its sessions'
/// dots (working, waiting, idle), "+N" for their subagents and tasks, "N mail"
fn bits(c: &Counts) -> Vec<(String, u32, f64)> {
    let mut out = vec![];
    for (n, rgb) in [
        (c.working, palette::status(lbterm_panel::RowState::Working)),
        (c.waiting, palette::status(lbterm_panel::RowState::Waiting)),
        (c.idle, palette::status(lbterm_panel::RowState::Idle)),
    ] {
        if n > 0 {
            out.push((format!("\u{25cf}{n}"), rgb, 1.0));
        }
    }
    if c.parts > 0 {
        out.push((format!("+{}", c.parts), 0xFFFFFF, DIM_ALPHA));
    }
    if c.mail > 0 {
        out.push((format!("{} mail", c.mail), palette::status(lbterm_panel::RowState::Waiting), 1.0));
    }
    out
}

/// What the Starting row and the strip say for a pick: the row's label is the project's name
fn opening_words(pick: Pick, name: &str) -> String {
    match pick {
        Pick::New { .. } => format!("Opening Claude in {name}"),
        Pick::Shell => format!("Opening a shell in {name}"),
        Pick::Resume => format!("Resuming {name}"),
        Pick::Inmail => format!("Opening {name}'s InMail"),
    }
}

/// The shell a spawn of the default program opens, by name: its program's file name
fn shell_name(config: &config::ConfigHandle) -> String {
    config
        .default_prog
        .as_ref()
        .and_then(|args| args.first())
        .and_then(|prog| std::path::Path::new(prog).file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Shell".to_string())
}

/// A launch menu entry's label: its own, else its command line
fn launch_label(spawn: &SpawnCommand) -> String {
    spawn
        .label
        .clone()
        .or_else(|| spawn.args.as_ref().map(|a| a.join(" ")))
        .unwrap_or_else(|| "Shell".to_string())
}

/// The records of the tabs asked for that the state file keeps
const START_RECORDS: usize = 8;

/// Now, in Unix milliseconds, for a start's record
fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// What a pick fires `lbterm-project-action` with
struct Fired {
    name: String,
    /// Where to open
    dir: String,
    /// The project's own folder, as the config spells it
    project_dir: String,
    pick: Pick,
    prompt: Option<String>,
    source: &'static str,
    counts: Counts,
    /// Its Starting row's start, to fold when nothing can open for it
    start: Option<u64>,
    /// The pane the event names: the tab menu's tab's active pane; none for the picker, whose
    /// event names the window's active pane
    pane: Option<PaneId>,
}

/// Tab `tab`'s folder, as its Claude session's statusline says it (cwd.txt), else as its active
/// pane last said it (OSC 7, else its youngest console process's), as a drive's path: Here's for
/// the open tab, the tab menu's New session's for its own
pub(crate) fn tab_folder(tab: &mux::tab::Tab) -> Option<String> {
    let said = super::tab_panel::panel_tab(tab, 0, true).cwd;
    let said = if said.is_empty() {
        tab.get_active_pane()?.get_current_working_dir(CachePolicy::AllowStale)?.to_string()
    } else {
        said
    };
    let folder = newtab::folder_of(&said);
    (!folder.is_empty()).then_some(folder)
}

impl TermWindow {
    /// The pad shows: the tab panel's on, and its `new_tab`
    pub(crate) fn lbterm_new_tab_on(&self) -> bool {
        self.config.lbterm_tab_panel.enabled && self.config.lbterm_tab_panel.new_tab
    }

    /// The picker or the + menu is open: it takes every key and click
    pub(crate) fn lbterm_new_tab_is_open(&self) -> bool {
        self.lbterm_new_tab.picker.is_some() || self.lbterm_new_tab.plus.is_some()
    }

    /// Whether the + menu is open
    pub(crate) fn lbterm_plus_is_open(&self) -> bool {
        self.lbterm_new_tab.plus.is_some()
    }

    /// The picker has the keys: the panes dim and the active outline goes out
    pub(crate) fn lbterm_new_tab_has_keys(&self) -> bool {
        self.lbterm_new_tab.picker.is_some()
    }

    /// The config was loaded: where the picker keeps its recent picks
    pub(crate) fn lbterm_new_tab_configure(&self) {
        let file = self.config.lbterm_projects.as_ref().and_then(|p| p.recent_file.as_ref());
        newtab::keep_in(file.map(std::path::Path::new));
    }

    /// The key table was made: the keys `LbtermNewTab`'s three are bound to, named as the pad's row
    /// and the + menu say them, none where nothing is (Lachlan's lbterm.lua binds them only in a
    /// build with the action, and another config may bind others). The first of each in the
    /// default table, fewest modifiers first, as the menus upstream pick one.
    pub(crate) fn lbterm_new_tab_keys(&mut self) {
        let key = |what: LbtermNewTab| -> Option<String> {
            let mut found = self
                .input_map
                .locate_app_wide_key_assignment(&config::keyassignment::KeyAssignment::LbtermNewTab(what));
            found.sort_by_key(|(key, mods)| (mods.bits().count_ones(), format!("{key:?}")));
            let (key, mods) = found.into_iter().next()?;
            Some(key_name(&key, mods))
        };
        let keys = KeyWords {
            picker: key(LbtermNewTab::Picker),
            shell: key(LbtermNewTab::Shell),
            reopen: key(LbtermNewTab::Reopen),
        };
        self.lbterm_new_tab.keys = keys;
    }

}

/// A key from the key table as the pad and the + menu name it. The table keeps
/// `{key='t', mods='CTRL|SHIFT'}` as Ctrl with 'T' (`Config::key_bindings` through
/// wezterm-input-types' `normalize_shift`), so a capital letter is Shift held, and `ui_key` makes
/// every letter a capital: named from the modifiers alone it says Ctrl+T (lbterm/PITFALLS.md, 122)
fn key_name(key: &::window::KeyCode, mods: Modifiers) -> String {
    let shift = mods.contains(Modifiers::SHIFT) || matches!(key, ::window::KeyCode::Char(c) if c.is_ascii_uppercase());
    let name = crate::inputmap::ui_key(key, ::window::UIKeyCapRendering::WindowsSymbols);
    newtab::key_words(
        mods.contains(Modifiers::CTRL),
        mods.contains(Modifiers::ALT),
        shift,
        mods.contains(Modifiers::SUPER),
        &name,
    )
}

impl TermWindow {
    /// Close the picker and the + menu
    pub(crate) fn lbterm_new_tab_close(&mut self) {
        let was = self.lbterm_new_tab.picker.take().is_some() | self.lbterm_new_tab.plus.take().is_some();
        if was {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// `LbtermNewTab`: the picker (Ctrl+T), a shell in the open pane's folder (Ctrl+Alt+T), or the
    /// tab closed last (Ctrl+Shift+T)
    pub(crate) fn lbterm_new_tab_action(&mut self, what: LbtermNewTab) {
        match what {
            LbtermNewTab::Picker => self.lbterm_new_tab_open_picker(Mode::Claude),
            LbtermNewTab::Shell => self.lbterm_new_tab_shell_here(),
            LbtermNewTab::Reopen => self.lbterm_new_tab_reopen(0),
        }
    }

    /// The open pane's folder, as its session's statusline says it (cwd.txt), else as the pane
    /// last said: Here's, and where a shell here opens
    fn lbterm_here(&self) -> Option<String> {
        let tab = Mux::get().get_active_tab_for_window(self.mux_window_id)?;
        tab_folder(&tab)
    }

    /// The project `dir` is in, as the picker names it: its place in the config's list, its name,
    /// and its own folder as the config spells it. None without projects in the config, or for a
    /// folder in none. The closes kept for Reopen and the tab menu's New session read it
    /// (projects::Folders, kept by the config's list: render/projects.rs)
    pub(crate) fn lbterm_project_of(&self, dir: &str) -> Option<(usize, String, String)> {
        let settings = self.config.lbterm_projects.as_ref().filter(|p| !p.projects.is_empty())?;
        let folders = self.lbterm_project_folders(settings);
        let i = folders.folders.of(dir)?;
        let project = folders.projects.get(i)?;
        Some((i, project.name.clone(), project.dir.clone()))
    }

    /// The tab menu's New session in <Project> (render/tab_menu.rs): Claude in `dir`, the menu's
    /// tab's folder, in front, fired as the picker fires a pick (`lbterm-project-action`, `source`
    /// "tab menu", `pane` the tab's active pane, the event's `dir` that folder and `project_dir`
    /// the project's own), its Starting row up from the next frame as the menu fades. It's a
    /// recent pick in the picker, as a pick in the picker is (Lachlan's 8b of 2026-11-07)
    pub(crate) fn lbterm_new_tab_session_in(&mut self, dir: String, pane: Option<PaneId>) {
        let Some((i, name, project_dir)) = self.lbterm_project_of(&dir) else {
            return;
        };
        let counts = match self.config.lbterm_projects.as_ref() {
            Some(settings) => {
                let folders = self.lbterm_project_folders(settings);
                self.lbterm_projects_counts(&folders).get(i).copied().unwrap_or_default()
            }
            None => Counts::default(),
        };
        let pick = Pick::New { behind: false };
        // A recent pick, as one in the picker is (Lachlan's 8b of 2026-11-07): first in memory at
        // once, its file written on a thread of its own (newtab::pick)
        newtab::pick(&name);
        let start = self.lbterm_new_tab_expect(name.clone(), opening_words(pick, &name), false);
        self.lbterm_new_tab_fire(Fired {
            name,
            dir,
            project_dir,
            pick,
            prompt: None,
            source: "tab menu",
            counts,
            start,
            pane,
        });
    }

    /// Lachlan is closing `tab` himself (the closes that write close marks, termwindow/last_tab.rs):
    /// it's kept, first of the ten, for Reopen and Ctrl+Shift+T. Its row's label, every pane's Claude
    /// session (the active pane's first), its folder as its session's statusline or else its pane
    /// last said it, and the project that's in, the picker's name for it. Nothing's read from a
    /// file: what the rail already keeps.
    pub(crate) fn lbterm_new_tab_closing(&mut self, tab: &Arc<mux::tab::Tab>) {
        let active = tab.get_active_pane();
        let active_id = active.as_ref().map(|p| p.pane_id());
        let mut sessions: Vec<(bool, String)> = tab
            .iter_panes_ignoring_zoom()
            .into_iter()
            .filter_map(|p| {
                let sid = p.pane.copy_user_vars().get("claude_session").cloned()?;
                Some((Some(p.pane.pane_id()) == active_id, sid))
            })
            .collect();
        // The active pane's first, the rest in their order
        sessions.sort_by_key(|(is_active, _)| !*is_active);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let panel = super::tab_panel::panel_tab(tab, 0, false);
        let label = lbterm_panel::row(&panel, now, 3600.0).0.label;
        let pane_said = active
            .and_then(|p| p.get_current_working_dir(CachePolicy::AllowStale))
            .map(|u| u.to_string())
            .unwrap_or_default();
        let mut closed = newtab::closed(sessions.into_iter().map(|(_, s)| s).collect(), &label, &panel.cwd, &pane_said, String::new(), now);
        closed.project = self.lbterm_project_of(&closed.dir).map(|(_, name, _)| name).unwrap_or_default();
        log::info!("new tab: closed {:?} in {:?} kept to reopen", closed.label, closed.dir);
        newtab::keep_closed(&mut self.lbterm_new_tab.closed, closed);
    }

    fn lbterm_picker_base(&self) -> Option<Base> {
        let settings = self.config.lbterm_projects.as_ref().filter(|p| !p.projects.is_empty())?;
        let folders = self.lbterm_project_folders(settings);
        let names: Vec<String> = folders.projects.iter().map(|p| p.name.clone()).collect();
        let dirs: Vec<String> = folders.projects.iter().map(|p| p.dir.clone()).collect();
        let aliases: Vec<Vec<String>> = folders.projects.iter().map(|p| p.aliases.clone()).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let here = self.lbterm_here();
        let entries = newtab::order(&refs, &dirs, &folders.parents, here.as_deref(), &newtab::recent());
        let counts = self.lbterm_projects_counts(&folders);
        Some(Base { names, dirs, aliases, entries, counts })
    }

    /// Open the picker in `mode`: Claude's (the pad, Ctrl+T) or the shell's (the + menu's Shell in
    /// a project…). Without projects in the config there's nothing to pick from, and the pad opens
    /// the default program in the open pane's folder, as upstream's + did.
    pub(crate) fn lbterm_new_tab_open_picker(&mut self, mode: Mode) {
        if !self.lbterm_new_tab_on() {
            // Ctrl+T bound to it in a config with the pad off: say so rather than nothing
            log::info!("new tab: the picker is off");
            self.lbterm_strip_notice(newtab::PICKER_OFF);
            return;
        }
        let Some(base) = self.lbterm_picker_base() else {
            self.lbterm_new_tab_shell_here();
            return;
        };
        self.lbterm_new_tab.plus = None;
        // Any other menu gives way
        self.lbterm_close_tab_menu();
        // The panel holds where it is: closed stays closed (no opening after a rest on the pad),
        // open stays open (lbterm/NEW-TAB.md, "Left click")
        let now = Instant::now();
        if self.lbterm_panel.hover.is_open() {
            self.lbterm_panel.hover.open_now(now);
        } else {
            self.lbterm_panel.hover.close();
        }
        let tab = Mux::get().get_active_tab_for_window(self.mux_window_id).map(|t| t.tab_id());
        self.lbterm_new_tab.base = Some(Rc::new(base));
        let asked = self.lbterm_state_frames();
        self.lbterm_new_tab.picker = Some(OpenPicker { state: PickerState::new(mode), asked, shown: None, tab, chip: None });
        // Timed to the frame that shows it (termwindow/budget.rs)
        self.lbterm_budget_picker();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// The Starting row for a shell, the default program, by its name (the tab menu's Duplicate
    /// tab, render/tab_menu.rs)
    pub(crate) fn lbterm_new_tab_expect_shell(&mut self) {
        let name = shell_name(&self.config);
        self.lbterm_new_tab_expect(name.clone(), format!("Opening {name}"), false);
    }

    /// A shell (the default program) in the open pane's folder, in front: Ctrl+Alt+T, and the
    /// pad without projects to pick from
    fn lbterm_new_tab_shell_here(&mut self) {
        let dir = self.lbterm_here();
        self.lbterm_new_tab_expect_shell();
        self.spawn_command(
            &SpawnCommand {
                cwd: dir.map(Into::into),
                domain: SpawnTabDomain::CurrentPaneDomain,
                ..Default::default()
            },
            crate::spawn::SpawnWhere::NewTab,
        );
    }

    /// Reopen the tab closed `index`th last: fire `lbterm-reopen-tab` with it, and without a
    /// handler returning false open the default program in its folder. It leaves the list.
    fn lbterm_new_tab_reopen(&mut self, index: usize) {
        if index >= self.lbterm_new_tab.closed.len() {
            // Said where he looks, not only in the log: Ctrl+Shift+T did nothing he could see
            log::info!("new tab: nothing closed to reopen");
            self.lbterm_strip_notice(newtab::NOTHING_CLOSED);
            return;
        }
        let closed = self.lbterm_new_tab.closed.remove(index);
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        let label = if closed.label.trim().is_empty() { "tab".to_string() } else { closed.label.clone() };
        self.lbterm_new_tab_expect(label.clone(), format!("Reopening {label}"), false);
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        async fn fire(lua: Option<Rc<mlua::Lua>>, window: GuiWin, pane: MuxPane, closed: newtab::Closed) -> anyhow::Result<()> {
            let default = match lua {
                Some(lua) => {
                    let table = lua.create_table()?;
                    if let Some(sid) = closed.sessions.first() {
                        table.set("session", sid.clone())?;
                    }
                    table.set("sessions", lua.create_sequence_from(closed.sessions.clone())?)?;
                    table.set("dir", closed.dir.clone())?;
                    table.set("label", closed.label.clone())?;
                    table.set("project", closed.project.clone())?;
                    let args = lua.pack_multi((window.clone(), pane, table))?;
                    config::lua::emit_event(&lua, ("lbterm-reopen-tab".to_string(), args))
                        .await
                        .map_err(|e| {
                            log::error!("while processing lbterm-reopen-tab event: {:#}", e);
                            e
                        })?
                }
                None => true,
            };
            if default {
                // Where it was, or where the default program opens when nothing said
                let dir = Some(closed.dir.clone()).filter(|d| !d.is_empty());
                window.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.lbterm_new_tab_spawn(dir, false);
                })));
            }
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| fire(lua, window, pane, closed))).detach();
    }

    /// The default program in `dir`, in a new tab of this window on the open pane's domain: in
    /// front, or behind, the window staying on its tab
    pub(crate) fn lbterm_new_tab_spawn(&mut self, dir: Option<String>, behind: bool) {
        if !behind {
            self.spawn_command(
                &SpawnCommand {
                    cwd: dir.map(Into::into),
                    domain: SpawnTabDomain::CurrentPaneDomain,
                    ..Default::default()
                },
                crate::spawn::SpawnWhere::NewTab,
            );
            return;
        }
        let size = self.terminal_size;
        let window_id = self.mux_window_id;
        let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
        promise::spawn::spawn(async move {
            let mux = Mux::get();
            let current = mux
                .get_active_tab_for_window(window_id)
                .and_then(|t| t.get_active_pane())
                .map(|p| p.pane_id());
            let command = dir.as_ref().map(|dir| {
                let mut builder = portable_pty::CommandBuilder::new_default_prog();
                builder.cwd(dir);
                builder
            });
            let workspace = mux.active_workspace();
            match mux
                .spawn_tab_or_window_activating(
                    Some(window_id),
                    SpawnTabDomain::CurrentPaneDomain,
                    command,
                    dir,
                    size,
                    current,
                    workspace,
                    None,
                    false,
                )
                .await
            {
                Ok((_, pane, _)) => pane.set_config(term_config),
                Err(err) => log::error!("new tab: the tab behind didn't open: {err:#}"),
            }
        })
        .detach();
    }

    /// Lua's `window:lbterm_expect_tab(text)`: a tab is on its way (the strip says so past a
    /// quarter second, render/strip.rs) and the Starting row shows at the end of the tabs, unless
    /// one already does (LBTerm's own, for the pick whose handler says this). `nil` takes it back:
    /// the row folds and the strip says nothing opened.
    pub(crate) fn lbterm_expect_tab(&mut self, text: Option<String>) {
        match text {
            Some(text) => {
                if self.lbterm_new_tab.starts.is_empty() && self.config.lbterm_tab_panel.enabled {
                    let label = newtab::starting_label(&text);
                    self.lbterm_new_tab_ask(label, false);
                }
                self.lbterm_strip_expect(Some(text));
            }
            None => {
                self.lbterm_strip_expect(None);
                // The newest: the handler of the pick just made takes back its own
                if let Some(gone) = self.lbterm_new_tab.starts.take_back() {
                    self.lbterm_new_tab_ended(gone.id, "folded", None, 1);
                    self.lbterm_strip_notice(newtab::nothing_opened(&gone.label));
                }
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A tab is asked for in this window by LBTerm itself: a Starting row, labelled `label`, and
    /// the strip's `text` while it comes. Its id, for taking it back, while the tab panel's on.
    pub(crate) fn lbterm_new_tab_expect(&mut self, label: String, text: String, behind: bool) -> Option<u64> {
        // The row shows, and its tab is looked for, with the tab panel (render/tab_panel.rs)
        let id = self.config.lbterm_tab_panel.enabled.then(|| self.lbterm_new_tab_ask(label, behind));
        self.lbterm_strip_expect(Some(text));
        // Timed to the frame that shows the row (termwindow/budget.rs)
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        id
    }

    /// A start, queued behind any still on their way, with its record: the window's tabs now are
    /// the ones it won't take for its own
    fn lbterm_new_tab_ask(&mut self, label: String, behind: bool) -> u64 {
        self.lbterm_new_tab.next_start += 1;
        let id = self.lbterm_new_tab.next_start;
        self.lbterm_new_tab_asked(id, &label, behind);
        let tabs = self.lbterm_window_tab_ids();
        self.lbterm_new_tab.starts.ask(newtab::Starting { id, label, since: Instant::now(), tabs, behind });
        id
    }

    /// The start `id` opened nothing it could have (a pick no handler acted on, whose default
    /// opens nothing): its row folds now rather than at 20 s, and the strip says so
    pub(crate) fn lbterm_new_tab_fold(&mut self, id: u64) {
        if let Some(gone) = self.lbterm_new_tab.starts.take(id) {
            self.lbterm_strip_expect(None);
            self.lbterm_new_tab_ended(gone.id, "folded", None, 1);
            self.lbterm_strip_notice(newtab::nothing_opened(&gone.label));
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// Each paint of the tab panel, before it's laid out: whether the Starting row's tab has come
    /// (the first the window didn't have), which it gives the tab whose row grows out of it; past its
    /// time it folds and the strip says nothing opened. When to look again, while it waits.
    pub(crate) fn lbterm_new_tab_starting(&mut self) -> (Option<TabId>, Option<Instant>) {
        if self.lbterm_new_tab.starts.is_empty() {
            return (None, None);
        }
        // The window's tabs, asked only while a row waits: nothing more a frame otherwise
        let tabs = self.lbterm_window_tab_ids();
        let showing = self.lbterm_new_tab.starts.showing().map(|s| s.id);
        let (came, due) = self.lbterm_new_tab.starts.look(Instant::now(), &tabs);
        // The tab whose row grows out of the Starting row: the one the row on the rail was for
        let mut grows = None;
        for c in came {
            match c {
                newtab::Came::Arrived { start, tab } => {
                    self.lbterm_new_tab_ended(start.id, "arrived", Some(tab), 1);
                    // Behind, the pad glints as it comes: the window stays where it was
                    if start.behind && !self.lbterm_panel.glints.contains(&tab) {
                        self.lbterm_panel.glints.push(tab);
                    }
                    if Some(start.id) == showing {
                        grows = Some(tab);
                    }
                }
                newtab::Came::Expired(gone) => {
                    self.lbterm_new_tab_ended(gone.id, "folded", None, 1);
                    log::info!("new tab: nothing opened for {} in {:?}", gone.label, newtab::STARTING_FOR);
                    self.lbterm_strip_notice(newtab::nothing_opened(&gone.label));
                }
            }
        }
        (grows, due)
    }

    /// How many tabs the rail has no room for, "+N" on the pad's rule: the layout's rows past its
    /// room, less the Starting row's slot when it's among them (it's the last), which is no tab
    pub(crate) fn lbterm_hidden_tabs(&self, layout: &Layout) -> usize {
        let hidden = layout.hidden();
        if hidden > 0 && self.lbterm_starting_label().is_some() {
            hidden - 1
        } else {
            hidden
        }
    }

    /// The Starting row's label, while one shows: the oldest start still on its way
    pub(crate) fn lbterm_starting_label(&self) -> Option<&str> {
        self.lbterm_new_tab.starts.showing().map(|s| s.label.as_str())
    }

    /// A tab was asked for: its record, from the last frame painted, the eight newest kept
    fn lbterm_new_tab_asked(&mut self, id: u64, label: &str, behind: bool) {
        let records = &mut self.lbterm_new_tab.records;
        if records.len() >= START_RECORDS {
            records.remove(0);
        }
        let asked = self.lbterm_state_frames();
        self.lbterm_new_tab.records.push(lbterm_extras::state::StartSaid {
            id,
            label: label.to_string(),
            asked,
            asked_ms: unix_ms(),
            ended_ms: None,
            shown: None,
            ended: None,
            ended_frame: None,
            tab: None,
            behind,
        });
    }

    /// The paint under way drew the Starting row (render/tab_panel.rs): the oldest waiting's
    pub(crate) fn lbterm_new_tab_row_drawn(&mut self) {
        let next = self.lbterm_state_frames() + 1;
        let Some(id) = self.lbterm_new_tab.starts.showing().map(|s| s.id) else {
            return;
        };
        if let Some(start) = self.lbterm_new_tab.records.iter_mut().find(|s| s.id == id && s.ended.is_none()) {
            start.shown.get_or_insert(next);
        }
    }

    /// The start `id` ended `how`, shown by the frame `ahead` of the last painted (the paint under
    /// way's, or the next for one decided between paints)
    fn lbterm_new_tab_ended(&mut self, id: u64, how: &str, tab: Option<TabId>, ahead: u64) {
        let frame = self.lbterm_state_frames() + ahead;
        if let Some(start) = self.lbterm_new_tab.records.iter_mut().find(|s| s.id == id && s.ended.is_none()) {
            start.ended = Some(how.to_string());
            start.ended_frame = Some(frame);
            start.ended_ms = Some(unix_ms());
            start.tab = tab.map(|t| t as u64);
        }
    }

    /// The window lost the keyboard: the picker and the menu close
    pub(crate) fn lbterm_new_tab_focus_lost(&mut self) {
        self.lbterm_new_tab_close();
    }

    // ------------------------------------------------------------------------------ the pad

    /// The pointer on the tab panel, from render/tab_panel.rs: `on` its pad's zone. True when the
    /// look changed.
    pub(crate) fn lbterm_new_tab_pad_pointer(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.lbterm_new_tab.pad_hovered, on) != on
    }

    /// A press on the pad's zone at `x`, `y`, from render/tab_panel.rs: the ripple clipped to the
    /// pill, and a left button opens the picker at once; the right one waits for its release to
    /// open the + menu
    pub(crate) fn lbterm_new_tab_pad_press(&mut self, press: MousePress, x: f64, y: f64, layout: &Layout) {
        let pill = layout.foot_pill();
        let (reach, start) = lbterm_extras::lights::press_reach(self.dimensions.dpi as f64 / 96.0);
        self.lbterm_light(crate::termwindow::compositor::Light::Press {
            x,
            y,
            reach,
            start,
            within: Some((pixels(pill), layout.foot_radius() as f64)),
        });
        self.lbterm_new_tab.pad_pressed = Some(press);
        if press == MousePress::Left {
            // Timed to the frame that shows it (termwindow/budget.rs)
            self.lbterm_new_tab_open_picker(Mode::Claude);
        }
    }

    /// A button let go, from render/tab_panel.rs: true when it was the pad's. The right one opens
    /// the + menu at the pointer as it comes up.
    pub(crate) fn lbterm_new_tab_pad_release(&mut self, press: MousePress, x: f64, y: f64) -> bool {
        if self.lbterm_new_tab.pad_pressed != Some(press) {
            return false;
        }
        self.lbterm_new_tab.pad_pressed = None;
        if press == MousePress::Right {
            self.lbterm_new_tab_open_plus((x, y));
        }
        true
    }

    /// The pad's look under the pointer this frame, for the compositor: where, and whether lit
    pub(crate) fn lbterm_plus_visual(&self) -> Option<(PixelRect, bool)> {
        self.lbterm_new_tab.pad_visual
    }

    /// The picker's visual this frame: where, and whether it's opening or closing
    pub(crate) fn lbterm_picker_visual(&self) -> Option<(PixelRect, bool)> {
        self.lbterm_new_tab.picker_visual
    }

    // ------------------------------------------------------------------------------ the + menu

    fn lbterm_new_tab_open_plus(&mut self, at: (f64, f64)) {
        if !self.lbterm_new_tab_on() {
            return;
        }
        self.lbterm_new_tab.picker = None;
        self.lbterm_close_tab_menu();
        let labels: Vec<String> = self.config.launch_menu.iter().map(launch_label).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let keys = newtab::launch_keys(&refs);
        let rows = newtab::plus_rows(labels.len(), self.lbterm_new_tab.closed.len());
        self.lbterm_new_tab.plus = Some(OpenPlus { at, focus: None, rows, labels, keys });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn lbterm_plus_layout(&self, open: &OpenPlus) -> newtab::PlusLayout {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let size = (self.dimensions.pixel_width as f64, self.dimensions.pixel_height as f64);
        newtab::plus_layout(open.at, size, scale, &open.rows)
    }

    /// Do what a row of the + menu says. The row flashes white as it goes, as the picker's do
    /// (render/menu_fade.rs, `lbterm_menu_flash`), but not "Nothing closed", which picks nothing
    fn lbterm_plus_pick(&mut self, item: PlusItem) {
        if let Some(open) = self.lbterm_new_tab.plus.as_ref().filter(|_| item != PlusItem::NothingClosed) {
            if let Some((_, row)) = self.lbterm_plus_layout(open).rows.iter().find(|(i, _)| *i == Some(item)) {
                self.lbterm_menu_flash(Slot::List, *row);
            }
        }
        let Some(open) = self.lbterm_new_tab.plus.take() else {
            return;
        };
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        match item {
            PlusItem::Launch(i) => {
                let Some(entry) = self.config.launch_menu.get(i).cloned() else {
                    return;
                };
                let label = open.labels.get(i).cloned().unwrap_or_else(|| launch_label(&entry));
                self.lbterm_new_tab_expect(label.clone(), format!("Opening {label}"), false);
                let here = self.lbterm_here();
                self.spawn_command(
                    &SpawnCommand {
                        cwd: here.map(Into::into).or(entry.cwd.clone()),
                        domain: SpawnTabDomain::CurrentPaneDomain,
                        ..entry
                    },
                    crate::spawn::SpawnWhere::NewTab,
                );
            }
            PlusItem::ShellIn => self.lbterm_new_tab_open_picker(Mode::Shell),
            PlusItem::Reopen(i) => self.lbterm_new_tab_reopen(i),
            PlusItem::NothingClosed => {}
            PlusItem::NewWindow => self.spawn_command(
                &SpawnCommand {
                    domain: SpawnTabDomain::CurrentPaneDomain,
                    ..Default::default()
                },
                crate::spawn::SpawnWhere::NewWindow,
            ),
        }
    }

    // ------------------------------------------------------------------------------ picks

    /// Do `pick` in row `row`'s project of the picker's list
    fn lbterm_picker_pick(&mut self, row: usize, pick: Pick) {
        let (Some(base), Some(open)) = (self.lbterm_new_tab.base.clone(), self.lbterm_new_tab.picker.as_ref()) else {
            return;
        };
        let listed = base.listed(open.state.query());
        let Some(item) = listed.rows.get(row) else {
            return;
        };
        let Some(name) = base.names.get(item.project).cloned() else {
            return;
        };
        let dir = item.dir.clone();
        let project_dir = base.dirs.get(item.project).cloned().unwrap_or_default();
        let prompt = match pick {
            Pick::New { .. } if open.state.highlight == row => open.state.message(&listed, &base.names()),
            _ => None,
        };
        let counts = base.counts.get(item.project).copied().unwrap_or_default();
        // The picked row flashes white as the card goes (copy_in, copy_out)
        if let Some(drawn) = self.lbterm_new_tab.drawn.as_ref() {
            if let Some((_, r)) = drawn.layout.rows.iter().find(|(place, _)| *place == row) {
                let radius = 6.0 * self.dimensions.dpi as f64 / 96.0;
                self.lbterm_light(crate::termwindow::compositor::Light::Pad { rect: pixels(area(*r)), radius });
            }
        }
        newtab::pick(&name);
        let behind = matches!(pick, Pick::New { behind: true });
        if behind {
            // The window stays on its tab, the picker open with its box cleared
            if let Some(open) = self.lbterm_new_tab.picker.as_mut() {
                open.state.started_behind(&name);
            }
        } else {
            self.lbterm_new_tab.picker = None;
        }
        // Timed to the frame with its Starting row (termwindow/budget.rs)
        self.lbterm_budget_pick();
        let start = self.lbterm_new_tab_expect(name.clone(), opening_words(pick, &name), behind);
        self.lbterm_new_tab_fire(Fired { name, dir, project_dir, pick, prompt, source: "picker", counts, start, pane: None });
    }

    /// Fire `lbterm-project-action (window, pane, p)`, `p = {name, dir, project_dir, action,
    /// prompt, behind, source, mail, sessions}`: `dir` where to open (Here's folder for its row and
    /// for its own project typed, else the project's), `project_dir` the project's own folder as
    /// the config spells it, so a handler finds the project whatever folder it opens in (lbterm/
    /// PITFALLS.md, 115). Unless a handler returns false, new and shell open the default program
    /// in `dir`, in front unless `behind`; resume and inmail have no default, so their Starting row
    /// folds at once and the strip says nothing opened, rather than waiting its 20 s.
    fn lbterm_new_tab_fire(&mut self, f: Fired) {
        let pane = match f.pane {
            Some(id) => Mux::get().get_pane(id),
            None => self.get_active_pane_or_overlay(),
        };
        let Some(pane) = pane else {
            return;
        };
        log::info!("new tab: {} for {} from the {}", f.pick.action(), f.name, f.source);
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        async fn fire(lua: Option<Rc<mlua::Lua>>, window: GuiWin, pane: MuxPane, f: Fired) -> anyhow::Result<()> {
            let behind = matches!(f.pick, Pick::New { behind: true });
            let default = match lua {
                Some(lua) => {
                    let table = lua.create_table()?;
                    table.set("name", f.name.clone())?;
                    table.set("dir", f.dir.clone())?;
                    table.set("project_dir", f.project_dir.clone())?;
                    table.set("action", f.pick.action())?;
                    if let Some(prompt) = f.prompt.clone() {
                        table.set("prompt", prompt)?;
                    }
                    table.set("behind", behind)?;
                    table.set("source", f.source)?;
                    table.set("mail", f.counts.mail)?;
                    table.set("sessions", f.counts.sessions())?;
                    let args = lua.pack_multi((window.clone(), pane, table))?;
                    config::lua::emit_event(&lua, ("lbterm-project-action".to_string(), args))
                        .await
                        .map_err(|e| {
                            log::error!("while processing lbterm-project-action event: {:#}", e);
                            e
                        })?
                }
                None => true,
            };
            if !default {
                return Ok(());
            }
            let (dir, start) = (f.dir, f.start);
            if matches!(f.pick, Pick::New { .. } | Pick::Shell) {
                window.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.lbterm_new_tab_spawn(Some(dir), behind);
                })));
            } else if let Some(start) = start {
                // Nothing handled it, and there's nothing of LBTerm's own to open for it
                log::info!("new tab: no handler opened {} for {}", f.pick.action(), f.name);
                window.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.lbterm_new_tab_fold(start);
                })));
            }
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| fire(lua, window, pane, f))).detach();
    }

    // ------------------------------------------------------------------------------ the mouse

    /// While the picker or the + menu is open it takes every mouse event: true when it did
    pub(crate) fn lbterm_new_tab_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        // The release of the press on the pad that opened it is the pad's
        if let WMEK::Release(press) = &event.kind {
            if self.lbterm_new_tab_is_open() && self.lbterm_new_tab.pad_pressed == Some(*press) {
                self.lbterm_new_tab.pad_pressed = None;
                self.lbterm_panel.pressed = false;
                return true;
            }
        }
        if self.lbterm_new_tab.plus.is_some() {
            return self.lbterm_plus_mouse(event, context);
        }
        if self.lbterm_new_tab.picker.is_none() {
            return false;
        }
        let Some(drawn) = self.lbterm_new_tab.drawn.clone() else {
            // Not drawn yet: the press that opened it may still be let go
            return !matches!(event.kind, WMEK::Press(_));
        };
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let on_card = drawn.layout.card.contains(x, y);
        let row = drawn.layout.hit(x, y).filter(|&r| r < drawn.listed.rows.len());
        let chip = drawn.chips.iter().find(|(_, r)| r.contains(x, y)).map(|(c, _)| *c);
        match event.kind {
            WMEK::Move => {
                let mut changed = false;
                if let Some(open) = self.lbterm_new_tab.picker.as_mut() {
                    if let Some(row) = row.filter(|&r| r != open.state.highlight) {
                        open.state.highlight = row;
                        changed = true;
                    }
                    if open.chip != chip {
                        open.chip = chip;
                        changed = true;
                    }
                }
                if changed {
                    context.lbterm_invalidate();
                }
                context.set_cursor(Some(if row.is_some() { CursorIcon::Pointer } else { CursorIcon::Default }));
            }
            WMEK::Press(MousePress::Left) if row.is_some() => {
                let row = row.expect("matched");
                let mode = self.lbterm_new_tab.picker.as_ref().map_or(Mode::Claude, |o| o.state.mode);
                let pick = match chip {
                    Some(chip) => chip.pick(),
                    None => newtab::enter(
                        mode,
                        event.modifiers.contains(Modifiers::SHIFT),
                        event.modifiers.contains(Modifiers::CTRL),
                    ),
                };
                if let Some(open) = self.lbterm_new_tab.picker.as_mut() {
                    open.state.highlight = row;
                }
                self.lbterm_picker_pick(row, pick);
            }
            WMEK::Press(_) if on_card => {}
            // A click away closes it, the pad's own among them
            WMEK::Press(_) => self.lbterm_new_tab_close(),
            WMEK::VertWheel(n) => {
                if let Some(open) = self.lbterm_new_tab.picker.as_mut() {
                    // Up the card, away from the box, for a wheel turned away
                    let rows = drawn.listed.rows.len();
                    let shown = drawn.layout.shown();
                    let most = rows.saturating_sub(shown);
                    open.state.scroll = if n > 0 { (open.state.scroll + 3).min(most) } else { open.state.scroll.saturating_sub(3) };
                    context.lbterm_invalidate();
                }
            }
            _ => {}
        }
        true
    }

    fn lbterm_plus_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(open) = self.lbterm_new_tab.plus.as_ref() else {
            return false;
        };
        let layout = self.lbterm_plus_layout(open);
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let hit = layout.hit(x, y);
        match event.kind {
            WMEK::Move => {
                if hit != open.focus {
                    if let Some(open) = self.lbterm_new_tab.plus.as_mut() {
                        open.focus = hit;
                    }
                    context.lbterm_invalidate();
                }
                context.set_cursor(Some(if hit.is_some() { CursorIcon::Pointer } else { CursorIcon::Default }));
            }
            // The release of the right button that opened it lands where it opened
            WMEK::Release(_) => {}
            WMEK::Press(MousePress::Left) => match hit {
                Some(item) => self.lbterm_plus_pick(item),
                None if layout.card.contains(x, y) => {}
                None => self.lbterm_new_tab_close(),
            },
            WMEK::Press(_) if !layout.card.contains(x, y) => self.lbterm_new_tab_close(),
            _ => {}
        }
        true
    }

    // ------------------------------------------------------------------------------ the keys

    /// While the picker or the + menu is open it takes every key: true when it did
    pub(crate) fn lbterm_new_tab_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        if self.lbterm_new_tab.plus.is_some() {
            self.lbterm_plus_key(key, mods);
            return true;
        }
        if self.lbterm_new_tab.picker.is_none() {
            return false;
        }
        let ctrl = mods.contains(KeyModifiers::CTRL);
        let shift = mods.contains(KeyModifiers::SHIFT);
        let alt = mods.contains(KeyModifiers::ALT);
        let as_key = match key {
            KeyCode::Escape => Some(PickerKey::Escape),
            KeyCode::Enter => Some(PickerKey::Enter { shift, ctrl }),
            KeyCode::UpArrow => Some(PickerKey::Up),
            KeyCode::DownArrow => Some(PickerKey::Down),
            KeyCode::PageUp => Some(PickerKey::PageUp),
            KeyCode::PageDown => Some(PickerKey::PageDown),
            KeyCode::Tab => Some(PickerKey::Tab),
            KeyCode::Char(c) if alt && c.eq_ignore_ascii_case(&'r') => Some(PickerKey::Resume),
            KeyCode::Char(c) if alt && c.eq_ignore_ascii_case(&'m') => Some(PickerKey::Inmail),
            KeyCode::Char(c) if !ctrl && !alt && c.is_ascii_digit() => Some(PickerKey::Digit(*c)),
            _ => None,
        };
        let did = match (as_key, self.lbterm_new_tab.base.clone()) {
            (Some(k), Some(base)) => {
                let shown = self.lbterm_new_tab.drawn.as_ref().map_or(1, |d| d.layout.shown().max(1));
                let open = self.lbterm_new_tab.picker.as_mut().expect("open");
                let listed = base.listed(open.state.query());
                open.state.key(k, &listed, &base.names(), |p| base.mail(p), shown)
            }
            (Some(PickerKey::Escape), None) => PickerDid::Close,
            _ => PickerDid::Nothing,
        };
        match did {
            PickerDid::Close => {
                self.lbterm_new_tab_close();
                return true;
            }
            PickerDid::Pick { row, pick } => {
                self.lbterm_picker_pick(row, pick);
                return true;
            }
            PickerDid::NoMatch => {
                self.lbterm_picker_light();
                return true;
            }
            PickerDid::Changed => {
                self.lbterm_new_tab_changed();
                return true;
            }
            PickerDid::Nothing => {}
        }
        // Typing, and editing the box
        let open = self.lbterm_new_tab.picker.as_mut().expect("open");
        let input = &mut open.state.input;
        let before = input.text().to_string();
        // A key the picker has no use for: neither one of its keys above nor one that edits the
        // box (a key the config binds, since his 7b every key while it's open)
        let mut unused = false;
        match key {
            KeyCode::Char(c) if ctrl && !alt => match c.to_ascii_lowercase() {
                'a' => input.select_all(),
                'u' => input.set(""),
                _ => unused = true,
            },
            KeyCode::Char(c) if !alt && !c.is_control() => {
                let mut buf = [0u8; 4];
                input.insert(c.encode_utf8(&mut buf));
            }
            KeyCode::Backspace if ctrl => input.backspace_word(),
            KeyCode::Backspace => input.backspace(),
            KeyCode::Delete => input.delete(),
            KeyCode::LeftArrow if ctrl => input.word_left(shift),
            KeyCode::RightArrow if ctrl => input.word_right(shift),
            KeyCode::LeftArrow => input.left(shift),
            KeyCode::RightArrow => input.right(shift),
            KeyCode::Home => {
                let at = 0;
                input.place(at, shift);
            }
            KeyCode::End => {
                let at = input.text().len();
                input.place(at, shift);
            }
            _ => unused = true,
        }
        if input.text() != before {
            open.state.edited();
        }
        // ...answered by the light round the box, as Enter with nothing named is (his 3a of
        // 2026-11-08: a key no row takes flares a menu's card, and the picker has no lamp), but
        // never a modifier on its own, which every menu lets be
        if unused && menu_fade::menu_key(key, mods) != lbterm_extras::menu::Key::Modifier {
            self.lbterm_picker_light();
        }
        self.lbterm_new_tab_changed();
        true
    }

    /// A light round the picker's box once (`Light::Pad`), its answer to a key it has no use for:
    /// Enter with nothing named, and since his 3a of 2026-11-08 a key that neither moves, picks
    /// nor edits (a menu's card flares for one, and the picker isn't one of the compositor's menu
    /// slots, so it has no lamp to flare: render/menu_fade.rs)
    fn lbterm_picker_light(&self) {
        if let Some(drawn) = self.lbterm_new_tab.drawn.as_ref() {
            let radius = 6.0 * self.dimensions.dpi as f64 / 96.0;
            let rect = pixels(area(drawn.layout.input));
            self.lbterm_light(crate::termwindow::compositor::Light::Pad { rect, radius });
        }
    }

    /// A key in the + menu: its rows' letters act here and nowhere else, and never reach the pane
    fn lbterm_plus_key(&mut self, key: &KeyCode, mods: KeyModifiers) {
        use lbterm_extras::menu::Key;
        let Some(open) = self.lbterm_new_tab.plus.as_mut() else {
            return;
        };
        let pick = match menu_fade::menu_key(key, mods) {
            Key::Escape => {
                self.lbterm_new_tab_close();
                return;
            }
            Key::Up => {
                open.focus = newtab::plus_step(&open.rows, open.focus, -1);
                None
            }
            Key::Down => {
                open.focus = newtab::plus_step(&open.rows, open.focus, 1);
                None
            }
            // The row lit, or with none lit the first lights, as every menu does
            // (lbterm/NEW-TAB.md, section 6)
            Key::Enter => match open.focus {
                Some(item) => Some(item),
                None => {
                    open.focus = newtab::plus_step(&open.rows, None, 1);
                    None
                }
            },
            Key::Char(c) => match newtab::plus_by_key(&open.rows, &open.keys, c) {
                Some(item) => Some(item),
                None => {
                    self.lbterm_menu_bump(Slot::List);
                    return;
                }
            },
            Key::Modifier => return,
            // Nowhere to go: the card's hairline brightens once
            _ => {
                self.lbterm_menu_bump(Slot::List);
                return;
            }
        };
        match pick {
            Some(item) => self.lbterm_plus_pick(item),
            None => self.lbterm_new_tab_changed(),
        }
    }

    /// Composed words (an IME's, a dead key's) into the picker's box while it's open
    pub(crate) fn lbterm_new_tab_text(&mut self, words: &str) -> bool {
        // The + menu has no box: a word there is a key none of its rows takes
        if self.lbterm_new_tab.plus.is_some() {
            self.lbterm_menu_bump(Slot::List);
            return true;
        }
        let Some(open) = self.lbterm_new_tab.picker.as_mut() else {
            return false;
        };
        open.state.input.insert(words);
        open.state.edited();
        self.lbterm_new_tab_changed();
        true
    }

    /// A key down the key table binds, while the picker or its + menu has the keys (and no menu
    /// before them in `lbterm_menus_key`'s order is open, render/menu_fade.rs
    /// `lbterm_menus_binding`), that acts there: over the + menu a key a row shows beside it as
    /// its shortcut picks that row, with its flash, as its letter would (Lachlan's 5a of
    /// 2026-11-08); over the picker Ctrl+V, bound to `PasteFrom`, pastes the clipboard's first
    /// line into its box as typed words (his 11a; Ctrl+V alone, his 4b, `newtab::pastes_into_picker`:
    /// Ctrl+Shift+V and Shift+Insert, bound to a paste too, go on as keys no row takes). True when it acted; any other
    /// bound key goes on to the menu as a key no row takes (his 7b). `key` and `mods` are as the
    /// key table was asked, which keeps each key `normalize_shift`ed, so named here they're the
    /// words the + menu draws (`key_name`, the same as `lbterm_new_tab_keys` names them by)
    pub(crate) fn lbterm_new_tab_binding(&mut self, action: &KeyAssignment, key: &::window::KeyCode, mods: Modifiers) -> bool {
        if let Some(open) = self.lbterm_new_tab.plus.as_ref() {
            let hinted = match action {
                KeyAssignment::LbtermNewTab(LbtermNewTab::Shell) => newtab::Hinted::Shell,
                KeyAssignment::LbtermNewTab(LbtermNewTab::Reopen) => newtab::Hinted::Reopen,
                _ => return false,
            };
            let (code, mods) = key.normalize_shift(mods.remove_positional_mods());
            let words = key_name(&code, mods);
            let Some(item) = newtab::plus_by_shortcut(&open.rows, &self.lbterm_new_tab.keys, hinted, &words) else {
                return false;
            };
            log::info!("+ menu: {words}, its shortcut, picks {item:?}");
            self.lbterm_plus_pick(item);
            return true;
        }
        if self.lbterm_new_tab.picker.is_some() {
            if let KeyAssignment::PasteFrom(source) = action {
                // Ctrl+V alone (his 4b of 2026-11-08); another paste key is a key no row takes
                let (code, mods) = key.normalize_shift(mods.remove_positional_mods());
                if newtab::pastes_into_picker(&key_name(&code, mods)) {
                    self.lbterm_new_tab_paste(*source);
                    return true;
                }
            }
        }
        false
    }

    /// A paste into the picker's box (his 11a): the clipboard's text, asked of Windows off the GUI
    /// thread as a pane's paste is (termwindow/clipboard.rs), put in when it comes if the picker is
    /// still open. With the state file on (a test window) it's what the window last copied, the
    /// state file's `copied`, and never the clipboard, which is Lachlan's (PITFALLS 151)
    fn lbterm_new_tab_paste(&mut self, source: ClipboardPasteSource) {
        if crate::state_probe::on() {
            let copied = self.lbterm_menu.copied.borrow().clone();
            log::info!("picker: a paste from the state file's copied ({})", if copied.is_some() { "something copied" } else { "nothing copied" });
            if let Some(text) = copied {
                self.lbterm_new_tab_pasted(&text);
            }
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let future = window.get_clipboard(match source {
            ClipboardPasteSource::Clipboard => ::window::Clipboard::Clipboard,
            ClipboardPasteSource::PrimarySelection => ::window::Clipboard::PrimarySelection,
        });
        promise::spawn::spawn(async move {
            match future.await {
                Ok(text) => window.notify(TermWindowNotif::Apply(Box::new(move |myself| myself.lbterm_new_tab_pasted(&text)))),
                Err(err) => log::warn!("picker: the clipboard couldn't be read for a paste: {err:#}"),
            }
        })
        .detach();
    }

    /// The pasted text into the picker's box, as typed words, so the search runs on it; nothing if
    /// the picker closed before it came
    fn lbterm_new_tab_pasted(&mut self, clip: &str) {
        let Some(open) = self.lbterm_new_tab.picker.as_mut() else {
            return;
        };
        let line = newtab::pasted(clip);
        if line.is_empty() {
            return;
        }
        open.state.input.insert(line);
        open.state.edited();
        self.lbterm_new_tab_changed();
    }

    fn lbterm_new_tab_changed(&self) {
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    // ------------------------------------------------------------------------------ the paint

    /// Each paint, after the tab panel: the pad's look under the pointer, the picker (open, or
    /// with the compositor fading out as it was), and the + menu
    pub(crate) fn paint_lbterm_new_tab(&mut self) -> anyhow::Result<()> {
        self.lbterm_new_tab.pad_visual = None;
        self.lbterm_new_tab.picker_visual = None;
        let layout = self.lbterm_panel.layout.filter(|l| l.foot);
        let Some(layout) = layout else {
            self.lbterm_new_tab.picker = None;
            self.lbterm_new_tab.plus = None;
            self.lbterm_new_tab.pad_hovered = false;
            return Ok(());
        };
        // The picker closes when the open tab changes under it, or a menu opens: asked only while
        // it or the + menu is open, nothing at rest
        if self.lbterm_new_tab_is_open() {
            let tab = Mux::get().get_active_tab_for_window(self.mux_window_id).map(|t| t.tab_id());
            let menu = self.lbterm_menu_is_open()
                || self.lbterm_prompt_list_is_open()
                || self.lbterm_conductor_menu_is_open()
                || self.lbterm_inbox_list_is_open()
                || self.lbterm_agenda_list_is_open();
            if self.lbterm_new_tab.picker.as_ref().map_or(false, |o| o.tab != tab || menu) {
                self.lbterm_new_tab.picker = None;
            }
            if menu {
                self.lbterm_new_tab.plus = None;
            }
        }
        self.lbterm_new_tab_paint_pad(&layout)?;
        self.lbterm_new_tab_paint_picker(&layout)?;
        self.lbterm_new_tab_paint_plus()?;
        Ok(())
    }

    /// The pad lit: its hairline and its + brighter, while it's lit and, with the compositor,
    /// while it fades out after: a template kept in its layer, made again only when the pad moves
    /// or its colours change (render/kept.rs), its visual faded in and out on `button_hover`
    fn lbterm_new_tab_paint_pad(&mut self, layout: &Layout) -> anyhow::Result<()> {
        let lit = self.lbterm_new_tab.pad_hovered || self.lbterm_new_tab_is_open();
        let composited = self.lbterm_composited();
        let pill = layout.foot_pill();
        // Fading out from when it went unlit, for as long as `button_hover` takes on screen; past
        // that its visual is put away and nothing of it is drawn or looked at a frame
        let now = Instant::now();
        if lit {
            self.lbterm_new_tab.unlit_at = None;
        } else if self.lbterm_new_tab.pad_lit {
            self.lbterm_new_tab.unlit_at = Some(now);
        }
        self.lbterm_new_tab.pad_lit = lit;
        let motion = self.lbterm_motion();
        let fade = if motion.enabled && composited { std::time::Duration::from_secs_f64(motion.button_hover.duration()) } else { std::time::Duration::ZERO };
        let fading = !lit && self.lbterm_new_tab.unlit_at.map_or(false, |at| now < at + fade);
        if !(lit || fading) {
            return Ok(());
        }
        if composited {
            self.lbterm_new_tab.pad_visual = Some((pixels(pill.grow(layout.px(2.0))), lit));
        }
        let card = {
            let (r, g, b, _) = self.palette().background.to_srgb_u8();
            ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        };
        let draw = |quads: &mut crate::quad::TripleLayerQuadAllocator| -> anyhow::Result<()> {
            let line = white_over(card, PAD_LINE_LIT);
            self.lbterm_stroke(
                quads,
                pill,
                layout.foot_radius(),
                layout.stroke(),
                layout.px(lbterm_panel::layout::FEATHER),
                |_| line,
            )?;
            let plus = srgb(white_over(card, PAD_PLUS_LIT), 1.0);
            for stroke in layout.plus() {
                self.filled_rectangle(quads, 0, euclid::rect(stroke.x, stroke.y, stroke.w, stroke.h), plus)?;
            }
            Ok(())
        };
        if composited {
            let [x, y, w, h, r, s] = [pill.x, pill.y, pill.w, pill.h, layout.foot_radius(), layout.stroke()].map(|v| v.to_bits() as u64);
            let key = self.lbterm_template_key(&[x, y, w, h, r, s, card as u64]);
            return self.lbterm_kept_layer(PLUS_HOVER_Z, "plus-hover", key, draw);
        }
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(PLUS_HOVER_Z)?;
        let mut quads = layer.quad_allocator();
        draw(&mut quads)
    }

    fn lbterm_new_tab_paint_picker(&mut self, layout: &Layout) -> anyhow::Result<()> {
        let now = Instant::now();
        let composited = self.lbterm_composited();
        let open = self.lbterm_new_tab.picker.is_some();
        if open {
            if let Some(base) = self.lbterm_picker_base() {
                self.lbterm_new_tab.base = Some(Rc::new(base));
            }
        }
        let (fading, settles) = match self.webgpu.as_ref().and_then(|w| w.compositor.as_ref()) {
            Some(c) if composited => c.picker_showing(now),
            _ => (false, None),
        };
        let drawn = match strip::card_draw(open, fading) {
            CardDraw::Fresh => self.lbterm_picker_lay_out(layout)?.map(Rc::new),
            CardDraw::Last => self.lbterm_new_tab.drawn.clone(),
            CardDraw::None => None,
        };
        let Some(drawn) = drawn else {
            if !open {
                self.lbterm_new_tab.drawn = None;
            }
            return Ok(());
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        self.lbterm_new_tab.picker_visual = strip::visual_bounds(&drawn.layout.card, scale, window)
            .map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open));
        let fill = self.lbterm_menu_fill();
        self.lbterm_picker_draw(&drawn, fill)?;
        let next = self.lbterm_state_frames() + 1;
        if let Some(open) = self.lbterm_new_tab.picker.as_mut() {
            open.shown.get_or_insert(next);
        }
        self.lbterm_new_tab.drawn = Some(drawn);
        if !open && fading {
            self.lbterm_new_tab_wake(settles);
        }
        Ok(())
    }

    /// Repaint at `at`, when a fade is done
    fn lbterm_new_tab_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        if matches!(self.lbterm_new_tab.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_new_tab.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_new_tab.wake != Some(at) {
                    return;
                }
                tw.lbterm_new_tab.wake = None;
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// The picker laid out afresh: where it goes, what it lists for the box, and its words
    fn lbterm_picker_lay_out(&mut self, layout: &Layout) -> anyhow::Result<Option<Drawn>> {
        let Some(base) = self.lbterm_new_tab.base.clone() else {
            return Ok(None);
        };
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = font.metrics().cell_width.get();
        let line = metrics.cell_size.height as f64;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let gutter = self.lbterm_shape().gutter() as f64;
        let keys = self.focused.is_some() || self.lbterm_keyboard_assumed;
        let panel_open = self.lbterm_panel.hover.is_open();
        let window_h = self.dimensions.pixel_height as f64;
        let Some(open) = self.lbterm_new_tab.picker.as_mut() else {
            return Ok(None);
        };
        let names = base.names();
        let listed = base.listed(open.state.query());
        let message = open.state.message(&listed, &names);
        let left = layout.panel(panel_open).right() as f64 + gutter;
        let bottom = layout.panel(false).bottom() as f64;
        // Laid out once for how many rows fit, then again with the highlight kept in sight
        let lay = |scroll: usize| {
            newtab::picker_layout(
                left,
                bottom,
                window_h,
                scale,
                line,
                listed.rows.len(),
                &listed.breaks,
                scroll,
                message.is_some(),
            )
        };
        let first = lay(open.state.scroll);
        open.state.hold(listed.rows.len(), first.shown());
        let picker = lay(open.state.scroll);
        open.state.scroll = picker.scroll;
        let highlight = (!listed.rows.is_empty()).then_some(open.state.highlight);
        let state = open.state.clone();
        let chip_under = open.chip;

        let mut words: Vec<(String, Rect, u32, f64)> = vec![];
        let word = |words: &mut Vec<(String, Rect, u32, f64)>, text: String, x: f64, y: f64, w: f64, rgb: u32, alpha: f64| {
            if !text.is_empty() && w > 0.0 {
                words.push((text, Rect { x, y, w, h: line }, rgb, alpha));
            }
        };
        // The frame's font needn't be monospaced (the system's title font, which the frame takes
        // where window_frame.font is unset, isn't): what goes beside or after words is placed by
        // their widths as laid out, not by columns
        let width = |text: &str| self.lbterm_new_tab_width(&font, &metrics, text);
        let advance = |text: &str| self.lbterm_new_tab_advance(&font, &metrics, text);
        let fit = |text: &str, room: f64| self.lbterm_new_tab_fit(&font, &metrics, text, room);
        let mut key_chips = vec![];
        let mut chips = vec![];
        let empty = state.input.is_empty();
        for (place, r) in &picker.rows {
            let y = (r.y + (r.h - line) / 2.0).round();
            let right = r.x + r.w - 6.0 * scale;
            let Some(item) = listed.rows.get(*place) else {
                if let Some(none) = &listed.none {
                    let x = picker.name_x(r);
                    word(&mut words, fit(none, right - x), x, y, right - x + cell, 0xFFFFFF, QUIET_ALPHA);
                }
                continue;
            };
            if empty {
                if let Some(chip) = picker.key_chip(r, *place) {
                    let digit = format!("{}", place + 1);
                    let dx = chip.x + ((chip.w - width(&digit)) / 2.0).max(0.0);
                    word(&mut words, digit, dx, y, chip.w + cell, 0xFFFFFF, DIM_ALPHA);
                    key_chips.push(chip);
                }
            }
            let counts = base.counts.get(item.project).copied().unwrap_or_default();
            // The right side: the session dots and mail, or on the highlighted row its chips
            let mut name_right = right;
            if highlight == Some(*place) {
                let on = newtab::chips(r, counts.mail, cell, scale);
                if let Some((_, first)) = on.first() {
                    name_right = first.x - 6.0 * scale;
                }
                for (chip, cr) in &on {
                    let label = chip.label(counts.mail);
                    let lx = cr.x + ((cr.w - width(&label)) / 2.0).max(0.0);
                    word(&mut words, label, lx, y, cr.w + cell, 0xFFFFFF, NAME_ALPHA);
                }
                chips = on;
            } else {
                let mut x = right;
                for (text, rgb, alpha) in bits(&counts).into_iter().rev() {
                    let w = width(&text);
                    x -= w;
                    word(&mut words, text, x, y, w + cell, rgb, alpha);
                    x -= 0.5 * cell;
                }
                name_right = x - 0.5 * cell;
            }
            // The name, a repo's under its project past the blue repo mark, the letters the box's
            // word matched at white 100%
            let mut x = picker.name_x(r);
            if item.indent {
                x += width("\u{2003}");
            }
            if item.repo {
                let mark = projects::REPO_MARK.to_string();
                let w = advance(&format!("{mark} "));
                word(&mut words, mark, x, y, w + cell, projects::REPO, 1.0);
                x += w;
            }
            let name = names.get(item.project).copied().unwrap_or("");
            let shown = fit(name, name_right - x);
            let alpha = if counts.quiet() { QUIET_ALPHA } else { NAME_ALPHA };
            let marks: &[usize] = item.found.as_ref().map(|f| f.marks.as_slice()).unwrap_or(&[]);
            let chars: Vec<char> = shown.chars().collect();
            let mut k = 0;
            while k < chars.len() {
                let lit = marks.contains(&k);
                let start = k;
                while k < chars.len() && marks.contains(&k) == lit {
                    k += 1;
                }
                let run: String = chars[start..k].iter().collect();
                let before: String = chars[..start].iter().collect();
                let rx = x + advance(&before);
                word(&mut words, run, rx, y, name_right - rx + cell, 0xFFFFFF, if lit { 1.0 } else { alpha });
            }
            // Here's path past its project's folder, dim
            if let Some(sub) = &item.sub {
                let sx = x + advance(&format!("{shown} "));
                if name_right - sx > 3.0 * cell {
                    word(&mut words, fit(sub, name_right - sx), sx, y, name_right - sx + cell, 0xFFFFFF, DIM_ALPHA);
                }
            }
        }
        // The box: what's typed, else the placeholder; the caret after the words, a long line
        // showing as much as fits before the caret
        let input = picker.input;
        let pad = 10.0 * scale;
        let iy = (input.y + (input.h - line) / 2.0).round();
        let room = (input.w - 2.0 * pad).max(1.0);
        let text = state.query();
        let chars: Vec<char> = text.chars().collect();
        let caret_at = text[..state.input.caret()].chars().count();
        // Galloped out from the caret both ways, so a pasted long line measures and shapes what
        // the box holds wherever the caret is in it, the word cut where it passes the box
        // (`newtab::box_span`, PITFALLS 192 and 209)
        let (skip, end) =
            newtab::box_span(caret_at, chars.len(), |from, to| advance(&chars[from..to].iter().collect::<String>()) <= room);
        let shown: String = chars[skip..end].iter().collect();
        if text.is_empty() {
            word(&mut words, newtab::placeholder(state.mode).to_string(), input.x + pad, iy, room, 0xFFFFFF, DIM_ALPHA);
        } else {
            word(&mut words, shown, input.x + pad, iy, room, 0xFFFFFF, 1.0);
        }
        let caret = Rect {
            x: (input.x + pad + advance(&chars[skip..caret_at].iter().collect::<String>())).round(),
            y: iy,
            w: (2.0 * scale).round().max(1.0),
            h: line,
        };
        if let (Some(m), Some(text)) = (picker.message, message.as_ref()) {
            let line_text = fit(&format!("First message: {text}"), m.w - 4.0 * scale);
            word(&mut words, line_text, m.x + 4.0 * scale, m.y + (m.h - line) / 2.0, m.w, 0xFFFFFF, MESSAGE_ALPHA);
        }
        let f = picker.footer;
        let footer = state.started.clone().unwrap_or_else(|| newtab::footer(state.mode).to_string());
        word(&mut words, fit(&footer, f.w - 4.0 * scale), f.x + 4.0 * scale, f.y + (f.h - line) / 2.0, f.w, 0xFFFFFF, DIM_ALPHA);
        Ok(Some(Drawn {
            layout: picker,
            listed,
            highlight,
            chip: chip_under.filter(|c| chips.iter().any(|(k, _)| k == c)),
            chips,
            key_chips,
            keys,
            caret: Some(caret),
            words,
        }))
    }

    /// How wide `text` lays out in `font`, measured once a shape generation and kept: the frame's
    /// font needn't be monospaced
    pub(crate) fn lbterm_new_tab_width(&self, font: &Rc<LoadedFont>, metrics: &RenderMetrics, text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        let key = (text.to_string(), Rc::as_ptr(font) as *const () as usize, self.shape_generation);
        if let Some(w) = self.lbterm_new_tab.widths.borrow().get(&key) {
            return *w;
        }
        let gl_state = self.render_state.as_ref().unwrap();
        let cell = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        let bounds = euclid::rect(0.0, 0.0, self.dimensions.pixel_width as f32, metrics.cell_size.height as f32);
        let element = Element::new(font, ElementContent::Text(text.to_string()));
        let w = self
            .compute_element(&self.lbterm_words_context(gl_state, metrics, cell, bounds, PICKER_TEXT_Z), &element)
            .map_or(0.0, |c| c.content_rect.width() as f64);
        let mut widths = self.lbterm_new_tab.widths.borrow_mut();
        // Kept while the picker's words come and go; a long session's typing lets them go
        if widths.len() > 4096 {
            widths.clear();
        }
        widths.insert(key, w);
        w
    }

    /// How far `text` reaches, its spaces counted: where what follows it starts
    fn lbterm_new_tab_advance(&self, font: &Rc<LoadedFont>, metrics: &RenderMetrics, text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        (self.lbterm_new_tab_width(font, metrics, &format!("{text}|")) - self.lbterm_new_tab_width(font, metrics, "|")).max(0.0)
    }

    /// `text` as much as fits in `room`, an ellipsis where it was cut
    pub(crate) fn lbterm_new_tab_fit(&self, font: &Rc<LoadedFont>, metrics: &RenderMetrics, text: &str, room: f64) -> String {
        if room <= 0.0 {
            return String::new();
        }
        if self.lbterm_new_tab_width(font, metrics, text) <= room {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        for n in (0..chars.len()).rev() {
            let cut: String = chars[..n].iter().collect();
            let cut = format!("{}\u{2026}", cut.trim_end());
            if self.lbterm_new_tab_width(font, metrics, &cut) <= room {
                return cut;
            }
        }
        String::new()
    }

    fn lbterm_picker_draw(&self, drawn: &Drawn, fill: MenuFill) -> anyhow::Result<()> {
        let card_rgb = fill.card;
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let over = |alpha: f64| srgb(white_over(card_rgb, alpha), 1.0);
        let gl_state = self.render_state.as_ref().unwrap();
        let layout = &drawn.layout;
        // The light round it, as the menus' (render/menu_fade.rs): made only when its card or the
        // background changes, kept in its layer (PICKER_GLOW_Z, the picker's alone) otherwise
        self.lbterm_kept_layer(PICKER_GLOW_Z, "picker-glow", self.lbterm_menu_glow_key(layout.card, fill.surface), |quads| {
            self.lbterm_menu_glow(quads, layout.card, fill.surface)
        })?;
        let layer = gl_state.layer_for_zindex(PICKER_CARD_Z)?;
        let mut quads = layer.quad_allocator();
        self.lbterm_menu_rounded(&mut quads, grow(layout.card, stroke), radius + stroke, srgb(palette::RING_MIDDLE, 1.0))?;
        self.lbterm_menu_rounded(&mut quads, layout.card, radius, srgb(card_rgb, 1.0))?;
        // The highlighted row
        if let Some(h) = drawn.highlight {
            if let Some((_, r)) = layout.rows.iter().find(|(place, _)| *place == h) {
                let hover = srgb(palette::blend(card_rgb, palette::HOVER, palette::HOVER_ALPHA * 1.6), 1.0);
                self.lbterm_menu_rounded(&mut quads, *r, 6.0 * scale, hover)?;
            }
        }
        for rule in &layout.rules {
            self.filled_rectangle(&mut quads, 0, rect_f(*rule), srgb(palette::OUTLINE, 0.6))?;
        }
        for chip in &drawn.key_chips {
            self.lbterm_menu_rounded(&mut quads, *chip, 4.0 * scale, over(KEY_CHIP_ALPHA))?;
        }
        for (chip, r) in &drawn.chips {
            let alpha = if drawn.chip == Some(*chip) { CHIP_LIT_ALPHA } else { CHIP_ALPHA };
            self.lbterm_menu_rounded(&mut quads, *r, (r.h / 2.0).floor(), over(alpha))?;
        }
        // The box: a hairline, brighter while it has the keyboard, round its fill
        let input = layout.input;
        let line = if drawn.keys { BOX_LINE_KEYS } else { BOX_LINE };
        self.lbterm_menu_rounded(&mut quads, grow(input, stroke), 6.0 * scale + stroke, over(line))?;
        self.lbterm_menu_rounded(&mut quads, input, 6.0 * scale, over(BOX_FILL))?;
        if let Some(caret) = drawn.caret {
            self.filled_rectangle(&mut quads, 0, rect_f(caret), over(CARET_ALPHA))?;
        }
        drop(quads);
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        let words = drawn
            .words
            .iter()
            .map(|(text, r, rgb, alpha)| (text.clone(), rect_f(*r), srgb(palette::blend(card_rgb, *rgb, *alpha), 1.0)));
        self.lbterm_draw_words(&self.lbterm_new_tab.words, &font, &metrics, room, PICKER_TEXT_Z, words)
    }

    /// The + menu, while it's open: the tab menu's card, a row an item, its key hints right
    fn lbterm_new_tab_paint_plus(&mut self) -> anyhow::Result<()> {
        if self.lbterm_new_tab.plus.is_none() {
            return Ok(());
        }
        let fill = self.lbterm_menu_fill();
        let card_rgb = fill.card;
        let Some(open) = self.lbterm_new_tab.plus.as_ref() else {
            return Ok(());
        };
        let layout = self.lbterm_plus_layout(open);
        let scale = layout.scale;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line = metrics.cell_size.height as f64;
        let cell = font.metrics().cell_width.get();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let colour = |alpha: f64| srgb(palette::blend(card_rgb, 0xFFFFFF, alpha), 1.0);
        let mut words: Vec<super::words::Word> = vec![];
        let mut place = |text: String, x: f64, y: f64, w: f64, c: LinearRgba| {
            if !text.is_empty() && w > 0.0 {
                words.push((text, euclid::rect(x as f32, y as f32, w as f32, line as f32), c));
            }
        };
        let width = |text: &str| self.lbterm_new_tab_width(&font, &metrics, text);
        let fit = |text: &str, room: f64| self.lbterm_new_tab_fit(&font, &metrics, text, room);
        for (item, r) in &layout.rows {
            let Some(item) = item else { continue };
            let x = r.x + 10.0 * scale;
            let right = r.x + r.w - 10.0 * scale;
            let y = (r.y + (r.h - line) / 2.0).round();
            // Its key and its shortcut, right-aligned, dim
            let key = newtab::plus_key(*item, &open.keys);
            let hint = match (newtab::plus_hint(*item, &self.lbterm_new_tab.keys), key) {
                (Some(h), Some(k)) => format!("{h}  {k}"),
                (Some(h), None) => h.to_string(),
                (None, Some(k)) => k.to_string(),
                (None, None) => String::new(),
            };
            let hint_w = width(&hint);
            if !hint.is_empty() {
                place(hint, right - hint_w, y, hint_w + cell, colour(DIM_ALPHA));
            }
            let room = right - x - hint_w - cell;
            match item {
                PlusItem::Launch(i) => {
                    let label = format!("{} here", open.labels.get(*i).map(String::as_str).unwrap_or("Shell"));
                    place(fit(&label, room), x, y, room + cell, srgb(palette::TITLE, 1.0));
                }
                PlusItem::ShellIn => place(fit("Shell in a project\u{2026}", room), x, y, room + cell, srgb(palette::TITLE, 1.0)),
                PlusItem::NothingClosed => place(fit("Nothing closed yet", room), x, y, room + cell, colour(DIM_ALPHA)),
                PlusItem::NewWindow => place(fit("New window", room), x, y, room + cell, srgb(palette::TITLE, 1.0)),
                PlusItem::Reopen(i) => {
                    if let Some(c) = self.lbterm_new_tab.closed.get(*i) {
                        let (first, second) = newtab::reopen_words(c, now);
                        let top = (r.y + (r.h - 2.0 * line) / 2.0).round();
                        place(fit(&first, room), x, top, room + cell, colour(0.85));
                        place(fit(&second, room), x, top + line, room + cell, colour(0.50));
                    }
                }
            }
        }
        let gl_state = self.render_state.as_ref().unwrap();
        // The light round it, its ring and card, and the row lit, gliding between rows: every
        // menu's (render/menu_fade.rs)
        self.lbterm_menu_card(Slot::List, layout.card, fill)?;
        let layer = gl_state.layer_for_zindex(PLUS_CARD_Z)?;
        let mut quads = layer.quad_allocator();
        for (item, r) in &layout.rows {
            match item {
                None => {
                    let rule = Rect { x: r.x + 8.0 * scale, y: r.y + r.h / 2.0, w: r.w - 16.0 * scale, h: scale.max(1.0) };
                    self.filled_rectangle(&mut quads, 0, rect_f(rule), srgb(palette::OUTLINE, 0.6))?;
                }
                Some(item) if Some(*item) == open.focus && *item != PlusItem::NothingClosed => {
                    self.lbterm_menu_light(Slot::List, &mut quads, *r, card_rgb)?;
                }
                Some(_) => {}
            }
        }
        drop(quads);
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_new_tab.plus_words, &font, &metrics, room, PLUS_TEXT_Z, words)
    }

    // ------------------------------------------------------------------------------ the state file

    /// The pad and its picker, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_state_new_tab(&self) -> Option<lbterm_extras::state::NewTab> {
        use lbterm_extras::state as S;
        let layout = self.lbterm_panel.layout.filter(|l| l.foot)?;
        let corner = self.lbterm_new_tab_corner();
        let open = self.lbterm_panel.hover.is_open();
        let pill = layout.foot_pill();
        let zone = layout.foot_zone(open, corner);
        let picker = match (self.lbterm_new_tab.picker.as_ref(), self.lbterm_new_tab.drawn.as_ref(), self.lbterm_new_tab.base.as_ref()) {
            (Some(p), Some(drawn), Some(base)) => {
                let names = base.names();
                let listed = &drawn.listed;
                Some(S::Picker {
                    mode: match p.state.mode {
                        Mode::Claude => "claude",
                        Mode::Shell => "shell",
                    }
                    .to_string(),
                    rect: state_rect(&drawn.layout.card),
                    input: state_rect(&drawn.layout.input),
                    query: p.state.query().to_string(),
                    rows: listed
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(i, r)| S::PickerRow {
                            name: names.get(r.project).copied().unwrap_or("").to_string(),
                            dir: r.dir.clone(),
                            group: r.group.map(|g| group_name(g).to_string()),
                            repo: r.repo,
                            marks: r.found.as_ref().map(|f| f.marks.clone()).unwrap_or_default(),
                            rect: drawn.layout.rows.iter().find(|(place, _)| *place == i).map(|(_, r)| state_rect(r)),
                        })
                        .collect(),
                    highlight: drawn.highlight,
                    message: p.state.message(listed, &names),
                    none: listed.none.clone(),
                    started: p.state.started.clone(),
                    footer: newtab::footer(p.state.mode).to_string(),
                    asked: p.asked,
                    shown: p.shown,
                })
            }
            _ => None,
        };
        Some(S::NewTab {
            rect: S::Rect::round(pill.x as f64, pill.y as f64, pill.w as f64, pill.h as f64),
            zone: S::Rect::round(zone.x as f64, zone.y as f64, zone.w as f64, zone.h as f64),
            hover: self.lbterm_new_tab.pad_hovered,
            open: self.lbterm_new_tab_is_open(),
            hidden: self.lbterm_hidden_tabs(&layout),
            picker,
            start: self.lbterm_new_tab.records.last().cloned(),
            starts: self.lbterm_new_tab.records.clone(),
            keys: S::NewTabKeys {
                picker: self.lbterm_new_tab.keys.picker.clone(),
                shell: self.lbterm_new_tab.keys.shell.clone(),
                reopen: self.lbterm_new_tab.keys.reopen.clone(),
            },
            closed: self
                .lbterm_new_tab
                .closed
                .iter()
                .map(|c| S::ClosedSaid {
                    label: c.label.clone(),
                    dir: c.dir.clone(),
                    project: c.project.clone(),
                    sessions: c.sessions.clone(),
                    at: c.at,
                })
                .collect(),
        })
    }

    /// The + menu while it's open, for the state file's `menu`
    pub(crate) fn lbterm_state_plus_menu(&self) -> Option<lbterm_extras::state::Menu> {
        let open = self.lbterm_new_tab.plus.as_ref()?;
        let layout = self.lbterm_plus_layout(open);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let items: Vec<(PlusItem, Rect)> = layout.rows.iter().filter_map(|(item, r)| item.map(|i| (i, *r))).collect();
        let label = |item: PlusItem| match item {
            PlusItem::Launch(i) => format!("{} here", open.labels.get(i).map(String::as_str).unwrap_or("Shell")),
            PlusItem::ShellIn => "Shell in a project\u{2026}".to_string(),
            PlusItem::NothingClosed => "Nothing closed yet".to_string(),
            PlusItem::NewWindow => "New window".to_string(),
            PlusItem::Reopen(i) => self
                .lbterm_new_tab
                .closed
                .get(i)
                .map(|c| newtab::reopen_words(c, now).0)
                .unwrap_or_default(),
        };
        Some(lbterm_extras::state::Menu {
            kind: "plus".to_string(),
            tab: None,
            rect: Some(state_rect(&layout.card)),
            items: items
                .iter()
                .map(|(item, r)| lbterm_extras::state::MenuItem {
                    label: label(*item),
                    checked: false,
                    key: newtab::plus_key(*item, &open.keys).map(String::from),
                    detail: None,
                    rect: Some(state_rect(r)),
                    inert: false,
                })
                .collect(),
            focus: open.focus.and_then(|f| items.iter().position(|(i, _)| *i == f)),
            renaming: None,
            waiting: None,
        })
    }

    /// With the window maximized, the pad's zone runs on into the screen's corner: the window's
    /// last pixel row
    pub(crate) fn lbterm_new_tab_corner(&self) -> Option<f32> {
        self.window_state
            .intersects(WindowState::MAXIMIZED | WindowState::FULL_SCREEN)
            .then(|| self.dimensions.pixel_height as f32)
    }

    /// What of the picker or the + menu is under `x`, `y`, for the state file's pointer
    pub(crate) fn lbterm_new_tab_under(&self, x: f64, y: f64) -> Option<(&'static str, Option<usize>)> {
        if let Some(open) = self.lbterm_new_tab.plus.as_ref() {
            let layout = self.lbterm_plus_layout(open);
            if layout.card.contains(x, y) {
                let items: Vec<&Rect> = layout.rows.iter().filter(|(i, _)| i.is_some()).map(|(_, r)| r).collect();
                return Some(("menu", items.iter().position(|r| r.contains(x, y))));
            }
        }
        if self.lbterm_new_tab.picker.is_some() {
            if let Some(drawn) = self.lbterm_new_tab.drawn.as_ref().filter(|d| d.layout.card.contains(x, y)) {
                return Some(("picker", drawn.layout.hit(x, y)));
            }
        }
        let layout = self.lbterm_panel.layout.filter(|l| l.foot)?;
        layout
            .foot_at(x as f32, y as f32, self.lbterm_panel.hover.is_open(), self.lbterm_new_tab_corner())
            .then_some(("rail.new_tab", None))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_bound_key_is_named_with_the_shift_its_capital_carries() {
        use ::window::KeyCode;
        // {key='t', mods='CTRL|SHIFT'} as the key table keeps it (normalize_shift)
        assert_eq!(key_name(&KeyCode::Char('T'), Modifiers::CTRL), "Ctrl+Shift+T");
        let (_, kept) = KeyCode::Char('t').normalize_shift(Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(key_name(&KeyCode::Char('T'), kept), "Ctrl+Shift+T");
        assert_eq!(key_name(&KeyCode::Char('t'), Modifiers::CTRL), "Ctrl+T");
        assert_eq!(key_name(&KeyCode::Char('t'), Modifiers::CTRL | Modifiers::ALT), "Ctrl+Alt+T");
        assert_eq!(key_name(&KeyCode::Function(7), Modifiers::NONE), "F7");
        // Shift kept as a modifier (a key normalize_shift leaves alone) is said once
        assert_eq!(key_name(&KeyCode::Char('T'), Modifiers::CTRL | Modifiers::SHIFT), "Ctrl+Shift+T");
    }

    /// A key pressed over the + menu is named as `lbterm_new_tab_binding` names it, from the form
    /// the key table was asked by, `normalize_shift`ed with the positional modifiers gone: the
    /// very words the menu draws for the binding, however Shift and the sides come with it
    /// (Lachlan's 5a of 2026-11-08)
    #[test]
    fn a_key_pressed_is_named_as_the_plus_menu_names_its_binding() {
        use ::window::KeyCode;
        // His {key='t', mods='CTRL|SHIFT'}, as the key table keeps it, named as the hint is
        let table = KeyCode::Char('t').normalize_shift(Modifiers::CTRL | Modifiers::SHIFT);
        let hint = key_name(&table.0, table.1);
        assert_eq!(hint, "Ctrl+Shift+T");
        let left = Modifiers::LEFT_CTRL | Modifiers::LEFT_SHIFT;
        for (key, mods) in [
            (KeyCode::Char('t'), Modifiers::CTRL | Modifiers::SHIFT),
            (KeyCode::Char('T'), Modifiers::CTRL | Modifiers::SHIFT),
            (KeyCode::Char('T'), Modifiers::CTRL | Modifiers::SHIFT | left),
        ] {
            let (code, mods) = key.normalize_shift(mods.remove_positional_mods());
            assert_eq!((code.clone(), mods), table, "{key:?}");
            assert_eq!(key_name(&code, mods), hint);
        }
        // Ctrl+Alt+T, the shell's: named apart from Reopen's
        let (code, mods) = KeyCode::Char('t').normalize_shift((Modifiers::CTRL | Modifiers::ALT | Modifiers::LEFT_ALT).remove_positional_mods());
        assert_eq!(key_name(&code, mods), "Ctrl+Alt+T");
        assert_ne!(key_name(&code, mods), hint);
    }

    #[test]
    fn the_new_tab_pads_z_layers_are_its_visuals_alone() {
        let ours: Vec<i8> = (i8::MIN..=i8::MAX).filter(|&z| target_of(z).is_some()).collect();
        assert_eq!(ours, vec![93, 94, 95, 96]);
        for z in ours {
            let others = [
                crate::termwindow::render::tab_panel::owns_zindex(z),
                crate::termwindow::render::menu_fade::target_of(z).is_some(),
                crate::termwindow::render::link_drag::owns_zindex(z),
                crate::termwindow::render::conductor_view::owns_zindex(z),
                crate::termwindow::render::tasks_card::owns_zindex(z),
                crate::termwindow::render::questions_pane::owns_zindex(z),
                crate::termwindow::render::strip::owns_zindex(z),
                crate::termwindow::render::strip_swap::owns_zindex(z),
                crate::termwindow::render::sides::owns_zindex(z),
                crate::termwindow::render::lights::owns_zindex(z),
                crate::termwindow::render::hover_fade::owns_zindex(z),
                crate::termwindow::render::card::part_of_zindex(z).is_some(),
                crate::termwindow::render::objectives::card_of(z).is_some(),
                z == crate::termwindow::render::link_drag::LAMP_Z,
                z == crate::termwindow::render::connections::LAMP_Z,
                z == crate::termwindow::render::triggers::HEARTS_Z,
                z == crate::termwindow::render::conductor::MENU_SPIN_Z,
            ];
            assert!(!others.contains(&true), "z-layer {} is another feature's too", z);
        }
    }

    #[test]
    fn the_plus_menu_draws_in_the_menus_visual() {
        // The InMail list's and the agenda's z-layers, their slot's card and words, which the
        // compositor fades with the other menus: only one of the three is open at a time
        use crate::termwindow::render::menu_fade::{self, Slot};
        for z in [Slot::List.glow_z(), PLUS_CARD_Z, PLUS_TEXT_Z] {
            assert!(menu_fade::target_of(z).is_some(), "z-layer {} isn't the menus'", z);
            assert!(target_of(z).is_none());
        }
    }

    #[test]
    fn a_picks_words_name_what_opens() {
        assert_eq!(opening_words(Pick::New { behind: false }, "Warden"), "Opening Claude in Warden");
        assert_eq!(newtab::starting_label(&opening_words(Pick::New { behind: true }, "Warden")), "Warden");
        assert_eq!(opening_words(Pick::Shell, "Warden"), "Opening a shell in Warden");
        assert_eq!(opening_words(Pick::Inmail, "Warden"), "Opening Warden's InMail");
        assert_ne!(STARTING_KEY, crate::termwindow::render::conductor_view::VIEW_KEY);
        let c = Counts { mail: 2, working: 1, waiting: 0, idle: 3, parts: 4 };
        let said: Vec<String> = bits(&c).into_iter().map(|(t, _, _)| t).collect();
        assert_eq!(said, vec!["\u{25cf}1", "\u{25cf}3", "+4", "2 mail"]);
    }
}
