//! LBTerm: the InMail button (`config.lbterm_inbox`; lbterm/INBOX.md, lbterm/DESIGN.md, "The
//! window strip"), whose logic is lbterm_extras::inbox. In the strip's first row, centred over
//! the tab rail: a hairline pill round an envelope, with the count of messages waiting in the
//! config's folders beside it in the waiting orange. Its messages come from crate::inbox, read on
//! a thread of its own. A click drops a list of them, newest first, each with when it was sent,
//! who it's for, its subject and who sent it, in the tab menu's card and light; a click on one,
//! or the arrow keys and Enter, fires the `lbterm-inbox-open` Lua event with the message, and
//! unless a handler returns false, opens a new tab in the folder above the message's. Esc or a
//! click elsewhere closes it.
//!
//! The count changing in a window dissolves on `swap`, and more mail brightens the button once
//! (render/strip_swap.rs). The list is drawn in z-layers 77 to 79, the slot it shares with the
//! agenda and the + menu in the menus' visuals, fading in and out with its highlight gliding
//! between rows (render/menu_fade.rs, `Slot::List`).
//!
//! A link to one message in a pane (`lbterm://inmail/<project>?message=<file>`) is drawn in the
//! waiting orange while that message waits, and grey, struck through, once it's gone: its colour
//! is chosen where the line is shaped (render/screen_line.rs), and the lines are shaped again
//! when what waits changes.

use crate::termwindow::render::menu_fade::Slot;
use crate::scripting::guiwin::GuiWin;
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{
    BorderColor, BoxDimension, Corners, Element, ElementColors, ElementContent, LayoutContext,
    SizedPoly, VerticalAlign,
};
use crate::termwindow::{TermWindow, TermWindowNotif, UIItemType};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, RectF, WindowOps};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::{Dimension, DimensionContext, SrgbaTuple};
use lbterm_extras::inbox::{self, LinkState, Message};
use lbterm_extras::tabs::Rect;
use lbterm_extras::{projects, prompts, strip};
use lbterm_panel::palette;
use mux_lua::MuxPane;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use termwiz::hyperlink::Hyperlink;
use wezterm_font::LoadedFont;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// The list's card and words: its slot's (render/menu_fade.rs, `Slot::List`)
const CARD_Z: i8 = Slot::List.card_z();
const TEXT_Z: i8 = Slot::List.text_z();

/// What the list says with nothing waiting
const NONE_WAITING: &str = "No InMail waiting";

/// The columns a repo's mark takes before who the message is for: the mark, then a space
const REPO_MARK_COLS: usize = 2;

#[derive(Default)]
pub struct InboxState {
    /// What the button was last built showing: how many wait, and whether its list was open
    shown: Option<(usize, bool)>,
    list: Option<OpenList>,
    /// crate::inbox::changes() when the panes' lines were last shaped for links to messages
    links_drawn: usize,
    /// The list as last laid out, and what from: the paint, every mouse event while it's open
    /// and the arrow keys all ask for it, and it's laid out again only when something it's laid
    /// out from has changed (it was laid out twice a frame and once a mouse move, each time
    /// asking which project every message's folder is in)
    laid: RefCell<Option<(ListAt, Rc<Laid>)>>,
    /// The list's words as its last frame laid them out (render/words.rs)
    words: RefCell<super::words::Words>,
}

impl InboxState {
    /// The list's words point into a glyph atlas that's gone (gpu_lost.rs)
    pub(crate) fn forget_words(&mut self) {
        self.words.get_mut().forget();
    }
}

/// The list laid out: where it goes and its rows, a column's width, how wide the project's column
/// is, and each row shown's words by its message (oldest first): when it was sent, who it's for
/// and whether that's a repo, its subject, who sent it, and "taken 15:02" when a conversation has
/// taken it
struct Laid {
    list: prompts::List,
    cell_w: f64,
    project_w: f64,
    rows: HashMap<usize, (String, String, bool, String, String, String)>,
}

/// What the list was laid out from: the messages waiting (crate::inbox replaces its list when what
/// waits changes; held, so another can't be made where it was), the projects' folders, the day
/// (a stamp says a time today, a weekday this week, else a date), how far down it's scrolled, the
/// shape generation (the fonts) and where it goes, as f64 bits
struct ListAt {
    waiting: Arc<Vec<Message>>,
    folders: Option<Rc<super::projects::ProjectFolders>>,
    day: chrono::NaiveDate,
    scroll: usize,
    shapes: usize,
    geometry: [u64; 7],
}

impl PartialEq for ListAt {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.waiting, &o.waiting)
            && match (&self.folders, &o.folders) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && (self.day, self.scroll, self.shapes, self.geometry) == (o.day, o.scroll, o.shapes, o.geometry)
    }
}

/// The list while it's open
struct OpenList {
    /// The message under the pointer, or where the arrow keys put it, by its file: what waits
    /// can change while the list is open
    focus: Option<PathBuf>,
    /// How far down the newest-first list the first row shown is
    scroll: usize,
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Words in a strip pill: `rgb` at rest and `lit` under the pointer (sRGB 0..1)
pub(crate) fn pill_text(font: &Rc<LoadedFont>, text: String, rgb: [f32; 3], lit: [f32; 3]) -> Element {
    let linear = |rgb: [f32; 3]| SrgbaTuple(rgb[0], rgb[1], rgb[2], 1.).to_linear();
    Element::new(font, ElementContent::Text(text))
        .vertical_align(VerticalAlign::Middle)
        .colors(ElementColors {
            border: BorderColor::default(),
            bg: LinearRgba::TRANSPARENT.into(),
            text: linear(rgb).into(),
        })
        .hover_colors(Some(ElementColors {
            border: BorderColor::default(),
            bg: LinearRgba::TRANSPARENT.into(),
            text: linear(lit).into(),
        }))
}

/// The message as a Lua table, for `lbterm-inbox-open`
fn message_table<'lua>(lua: &'lua mlua::Lua, m: &Message) -> mlua::Result<mlua::Table<'lua>> {
    let table = lua.create_table()?;
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    table.set("file", path(&m.file))?;
    table.set("name", m.name.clone())?;
    table.set("folder", path(&m.folder))?;
    table.set("dir", path(&m.dir))?;
    table.set("project", m.project.clone())?;
    table.set("from", m.from.clone())?;
    table.set("subject", m.subject.clone())?;
    table.set("sent", m.sent.clone())?;
    // The conversation working on it, when one has taken it (inbox::claim): a handler can bring
    // that one forward rather than start a second reader (Lachlan's E2 of 2026-11-08)
    if let Some(taken) = &m.taken {
        table.set("taken", taken.session.clone())?;
    }
    Ok(table)
}

impl TermWindow {
    /// The button shows: the inbox in the config, and the strip holding only the window's buttons
    /// beside the tab panel (the retro tab bar doesn't know the panel)
    pub(crate) fn lbterm_inbox_on(&self) -> bool {
        self.config.lbterm_inbox.is_some()
            && self.config.lbterm_tab_panel.enabled
            && config::lbterm::hides_tabs(&self.config)
            && self.config.use_fancy_tab_bar
            && self.show_tab_bar
            && !self.config.tab_bar_at_bottom
    }

    /// Each paint, before anything's drawn: when what waits has changed since this window last
    /// shaped its lines, the lines holding a link to a message are shaped again, since such a
    /// link is drawn by whether that message still waits (`lbterm_inbox_link`); every other line
    /// keeps its shapes (lbterm_extras::link_looks). Nothing without the inbox in the config.
    pub(crate) fn lbterm_inbox_links_refresh(&mut self) {
        if self.config.lbterm_inbox.is_none() {
            return;
        }
        let changes = crate::inbox::changes();
        if changes != self.lbterm_inbox.links_drawn {
            self.lbterm_inbox.links_drawn = changes;
            self.lbterm_link_states += 1;
        }
    }

    /// The colour a link in a pane is drawn in, and whether it's struck through, when it's a link
    /// to one message (lbterm_extras::inbox::LINK; lbterm/INBOX.md, "Links to a message"): the
    /// waiting orange while the message waits in one of the folders, else grey and struck
    /// through, since it's been dealt with. Nothing for any other link, without the inbox in the
    /// config, before its folders are first looked in, or for a link to a project none of the
    /// folders is the InMail of (inbox::looks_in), whose message may well wait.
    pub(crate) fn lbterm_inbox_link(&self, link: &Hyperlink) -> Option<(LinearRgba, bool)> {
        let config = self.config.lbterm_inbox.as_ref()?;
        if !link.uri().starts_with(inbox::LINK) {
            return None;
        }
        let waiting = crate::inbox::looked()?;
        Some(match inbox::link_state(link.uri(), &waiting, &config.folders)? {
            LinkState::Waiting => (srgb(inbox::LINK_WAITING, 1.0), false),
            LinkState::Taken => (srgb(inbox::LINK_TAKEN, 1.0), false),
            LinkState::Read => (srgb(inbox::LINK_READ, 1.0), true),
        })
    }

    /// Each paint, before the strip is built: true when it must be built again, for a new count
    /// on the button or its list opening or closing. A new count in the same window dissolves,
    /// and more mail brightens the button once (render/strip_swap.rs).
    pub(crate) fn lbterm_inbox_refresh(&mut self) -> bool {
        let shown = self
            .lbterm_inbox_on()
            .then(|| (crate::inbox::waiting().len(), self.lbterm_inbox.list.is_some()));
        if shown == self.lbterm_inbox.shown {
            return false;
        }
        if let (Some((was, _)), Some((now, _))) = (self.lbterm_inbox.shown, shown) {
            if was != now {
                self.lbterm_strip_swap_button(TabBarItem::LbtermInbox, now > was);
            }
        }
        if shown.is_none() {
            self.lbterm_inbox.list = None;
        }
        self.lbterm_inbox.shown = shown;
        true
    }

    /// The button for the strip's first row, `first_row` the row's bounds, and how wide it is laid
    /// out alone: the envelope and the count in a hairline pill. The strip places it, centred over
    /// the tab rail (`lbterm_strip_buttons`). Nothing while it doesn't show.
    pub(crate) fn lbterm_inbox_button(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        first_row: RectF,
    ) -> anyhow::Result<Option<(Element, f32)>> {
        let (Some(inbox), Some((count, open))) = (self.config.lbterm_inbox.as_ref(), self.lbterm_inbox.shown) else {
            return Ok(None);
        };
        let waiting = count > 0;
        let colors = |lit: bool| inbox::button_colors([surface.0, surface.1, surface.2], waiting, lit);
        // Its list open, it stays lit as under the pointer
        let (rest, lit) = (colors(open), colors(true));
        let mut kids = vec![pill_text(font, inbox.glyph.clone(), rest.glyph, lit.glyph)];
        if waiting {
            kids.push(pill_text(font, format!(" {}", inbox::badge(count)), rest.count, lit.count));
        }
        self.lbterm_strip_pill(font, metrics, surface, first_row, TabBarItem::LbtermInbox, kids, (rest.outline, lit.outline), (rest.glyph, lit.glyph))
            .map(Some)
    }

    /// A pill on the strip's first row round `kids`, as `item`: a hairline with the restart pill's
    /// rounded ends (restart.rs), its outline and its words' colour at rest and under the pointer
    /// (sRGB 0..1), and how wide it is laid out alone
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lbterm_strip_pill(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        first_row: RectF,
        item: TabBarItem,
        kids: Vec<Element>,
        (outline, lit_outline): ([f32; 3], [f32; 3]),
        (glyph, lit_glyph): ([f32; 3], [f32; 3]),
    ) -> anyhow::Result<(Element, f32)> {
        use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
        // Arcs drawn as outlines, their insides the strip's
        macro_rules! arc {
            ($x:ident, $y:ident) => {
                &[Poly {
                    path: &[PolyCommand::Oval {
                        center: (BlockCoord::$x, BlockCoord::$y),
                        radiuses: (BlockCoord::One, BlockCoord::One),
                    }],
                    intensity: BlockAlpha::Full,
                    style: PolyStyle::Outline,
                }]
            };
        }
        const TOP_LEFT: &[Poly] = arc!(One, One);
        const TOP_RIGHT: &[Poly] = arc!(Zero, One);
        const BOTTOM_LEFT: &[Poly] = arc!(One, Zero);
        const BOTTOM_RIGHT: &[Poly] = arc!(Zero, Zero);
        let radius = Dimension::Pixels((metrics.cell_size.height as f32 * 0.6).round());
        let corner = |poly| SizedPoly {
            width: radius,
            height: radius,
            poly,
        };
        let linear = |rgb: [f32; 3]| SrgbaTuple(rgb[0], rgb[1], rgb[2], 1.).to_linear();
        let pill_colors = |outline: [f32; 3], glyph: [f32; 3]| ElementColors {
            border: BorderColor::new(linear(outline)),
            bg: linear([surface.0, surface.1, surface.2]).into(),
            text: linear(glyph).into(),
        };
        let pill = Element::new(font, ElementContent::Children(kids))
            .item_type(UIItemType::TabBar(item))
            .vertical_align(VerticalAlign::Middle)
            .zindex(1)
            .padding(BoxDimension {
                left: Dimension::Cells(0.6),
                right: Dimension::Cells(0.6),
                top: Dimension::Cells(0.1),
                bottom: Dimension::Cells(0.1),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(Corners {
                top_left: corner(TOP_LEFT),
                top_right: corner(TOP_RIGHT),
                bottom_left: corner(BOTTOM_LEFT),
                bottom_right: corner(BOTTOM_RIGHT),
            }))
            .colors(pill_colors(outline, glyph))
            .hover_colors(Some(pill_colors(lit_outline, lit_glyph)));

        // How wide it is, laid out alone
        let width = self
            .compute_element(
                &LayoutContext {
                    height: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_height as f32,
                        pixel_cell: metrics.cell_size.height as f32,
                    },
                    width: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: metrics.cell_size.width as f32,
                    },
                    bounds: first_row,
                    metrics,
                    gl_state: self.render_state.as_ref().unwrap(),
                    zindex: 10,
                },
                &pill,
            )?
            .bounds
            .width();
        Ok((pill, width))
    }

    /// The strip's buttons over the tab rail, for its first row (`first_row` its bounds, `indent`
    /// where its words start in it): the InMail button centred over the rail's pads and the
    /// agenda's after it (render/agenda.rs), or the agenda's in its place without it. How far in
    /// from the row's left they go, then the pills and the strip between them and the words,
    /// which drags the window as the rest of the strip does; they never go into the words: past
    /// them they push them on, a column after the last. Nothing while neither shows.
    pub(crate) fn lbterm_strip_buttons(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        first_row: RectF,
        indent: f32,
    ) -> anyhow::Result<Option<(f32, Vec<Element>)>> {
        let inbox = self.lbterm_inbox_button(font, metrics, surface, first_row)?;
        let agenda = self.lbterm_agenda_button(font, metrics, surface, first_row)?;
        let Some(first_w) = inbox.as_ref().or(agenda.as_ref()).map(|(_, w)| *w) else {
            return Ok(None);
        };
        let column = super::tab_panel::rail_column(&self.config, &self.dimensions, &self.render_metrics);
        let x = (column - first_w / 2.).round().max(0.);
        let spacer = |w: f32| {
            Element::new(font, ElementContent::Children(vec![]))
                .item_type(UIItemType::TabBar(TabBarItem::None))
                .min_width(Some(Dimension::Pixels(w)))
        };
        let between_pills = (metrics.cell_size.width as f32 * 0.5).round();
        let mut used = x;
        let mut out = vec![];
        let both = inbox.is_some() && agenda.is_some();
        if let Some((pill, w)) = inbox {
            out.push(pill);
            used += w;
        }
        if let Some((pill, w)) = agenda {
            if both {
                out.push(spacer(between_pills));
                used += between_pills;
            }
            out.push(pill);
            used += w;
        }
        out.push(spacer((indent - used).max(metrics.cell_size.width as f32)));
        Ok(Some((x, out)))
    }

    pub(crate) fn lbterm_inbox_list_is_open(&self) -> bool {
        self.lbterm_inbox.list.is_some()
    }

    fn lbterm_inbox_list_close(&mut self) {
        if self.lbterm_inbox.list.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// A click on the button: open the list, or close it
    pub(crate) fn lbterm_inbox_toggle(&mut self) {
        if self.lbterm_inbox.list.is_some() {
            self.lbterm_inbox_list_close();
            return;
        }
        if !self.lbterm_inbox_on() {
            return;
        }
        self.lbterm_inbox.list = Some(OpenList {
            focus: None,
            scroll: 0,
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Where the list goes: under the strip, its card hanging from the button's left, how wide a
    /// column of the frame's font is, and each row shown's words. Laid out again only when the
    /// messages, the projects, the day, the scroll, the fonts or where it goes have changed
    fn lbterm_inbox_list_layout(&self, waiting: &Arc<Vec<Message>>) -> anyhow::Result<Option<Rc<Laid>>> {
        let Some(list) = self.lbterm_inbox.list.as_ref() else {
            return Ok(None);
        };
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let scale = self.dimensions.dpi as f64 / 96.0;
        let cell_w = font.metrics().cell_width.get();
        let row_h = (metrics.cell_size.height as f64 * 1.6).round();
        let now = chrono::Local::now().naive_local();
        let border = self.get_os_border();
        let top = border.top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
        let window = (
            self.dimensions.pixel_width as f64,
            self.dimensions.pixel_height as f64,
        );
        // The card's left edge at the button's, which is centred over the rail: its text a
        // stamp's width further in. The window counts the rail's strip as border, so the
        // window's own edge is that much further out
        let column = super::tab_panel::rail_column(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let edge = border.left.get() as f64
            - super::tab_panel::rail_width(&self.config, &self.dimensions, &self.render_metrics) as f64;
        let left = edge.max(0.0) + (column - 2.0 * cell_w).max(0.0) + 12.0 * scale;
        let folders = self.config.lbterm_projects.as_ref().map(|settings| self.lbterm_project_folders(settings));
        let at = ListAt {
            waiting: Arc::clone(waiting),
            folders: folders.clone(),
            day: now.date(),
            scroll: list.scroll,
            shapes: self.shape_generation,
            geometry: [left, top, window.0, window.1, row_h, cell_w, scale].map(f64::to_bits),
        };
        if let Some((was, laid)) = self.lbterm_inbox.laid.borrow().as_ref() {
            if *was == at {
                return Ok(Some(Rc::clone(laid)));
            }
        }

        // When each was sent, in a column as wide as the widest stamp
        let rows: Vec<(String, String, String, String, String)> = waiting.iter().map(|m| inbox::row(m, now)).collect();
        let stamp_cols = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0).max(5);
        let stamp_w = (stamp_cols as f64 + 2.0) * cell_w;
        // Who each is for, in a column as wide as the longest (a repo's with its mark and a space
        // before it), then the subject and who sent it
        let repos = Self::lbterm_inbox_repos(folders.as_deref(), waiting);
        let project_cols = waiting
            .iter()
            .zip(&repos)
            .map(|(m, &repo)| m.project.chars().count() + if repo { REPO_MARK_COLS } else { 0 })
            .max()
            .unwrap_or(0)
            .min(24);
        let project_w = if project_cols > 0 {
            (project_cols as f64 + 2.0) * cell_w
        } else {
            0.0
        };
        let after = |words: &String| if words.is_empty() { 0 } else { words.chars().count() + 3 };
        let text_cols = rows
            .iter()
            .map(|(_, _, subject, from, taken)| subject.chars().count() + after(from) + after(taken))
            .max()
            .unwrap_or(NONE_WAITING.len())
            .clamp(24, 80);
        // 28 px round the columns: the stamps start 12 in from the card's edge (prompts::list's
        // inset), and the rows sit 6 in from each side with their words stopping 10 short of a
        // row's end (the paint below), and a column to spare, since words in a box only as wide as
        // their columns lose the last glyph to rounding. Until 2026-11-08 it was 24 and none, so
        // the widest row had a column less than it needed and never showed who sent it (with 28
        // and none, perf/inbox.ps1's taken.png read "from Tw")
        let width = 28.0 * scale + stamp_w + project_w + (text_cols as f64 + 1.0) * cell_w;
        let Some(list) = prompts::list(
            waiting.len().max(1),
            left,
            top,
            window,
            (row_h, stamp_w, width),
            list.scroll,
            scale,
        ) else {
            *self.lbterm_inbox.laid.borrow_mut() = None;
            return Ok(None);
        };
        let mut rows: Vec<Option<(String, String, String, String, String)>> = rows.into_iter().map(Some).collect();
        let shown = list
            .rows
            .iter()
            .filter_map(|(i, _)| {
                let (stamp, project, subject, from, taken) = rows.get_mut(*i)?.take()?;
                Some((*i, (stamp, project, repos.get(*i).copied().unwrap_or(false), subject, from, taken)))
            })
            .collect();
        let laid = Rc::new(Laid {
            list,
            cell_w,
            project_w,
            rows: shown,
        });
        *self.lbterm_inbox.laid.borrow_mut() = Some((at, Rc::clone(&laid)));
        Ok(Some(laid))
    }

    /// Which of `waiting` are for a repo: a message whose folder is in a project `lbterm_projects`
    /// lists under another (its `parent`), which the list marks with the projects card's blue repo
    /// mark (Lachlan's yes, 2026-10-28). None without `lbterm_projects`.
    fn lbterm_inbox_repos(folders: Option<&super::projects::ProjectFolders>, waiting: &[Message]) -> Vec<bool> {
        let Some(folders) = folders else {
            return vec![false; waiting.len()];
        };
        waiting
            .iter()
            .map(|m| folders.folders.in_repo(&folders.parents, &m.dir.to_string_lossy()))
            .collect()
    }

    /// Open the message at `index` (oldest first), closing the list
    fn lbterm_inbox_list_pick(&mut self, index: usize) {
        let waiting = crate::inbox::waiting();
        let Some(message) = waiting.get(index).cloned() else {
            return;
        };
        // Its row flashes white as the list goes (render/menu_fade.rs)
        if let Some(laid) = self.lbterm_inbox_list_layout(&waiting).ok().flatten() {
            if let Some((_, row)) = laid.list.rows.iter().find(|(i, _)| *i == index) {
                self.lbterm_menu_flash(Slot::List, *row);
            }
        }
        self.lbterm_inbox_list_close();
        self.lbterm_inbox_open(message);
    }

    /// Fire `lbterm-inbox-open` with `message`; unless a handler returns false, open a new tab in
    /// the folder above the message's, on the active pane's domain
    fn lbterm_inbox_open(&mut self, message: Message) {
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        log::info!("inbox: opening {}", message.file.display());
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        async fn open(
            lua: Option<Rc<mlua::Lua>>,
            window: GuiWin,
            pane: MuxPane,
            message: Message,
        ) -> anyhow::Result<()> {
            let default = match lua {
                Some(lua) => {
                    let table = message_table(&lua, &message)?;
                    let args = lua.pack_multi((window.clone(), pane, table))?;
                    config::lua::emit_event(&lua, ("lbterm-inbox-open".to_string(), args))
                        .await
                        .map_err(|e| {
                            log::error!("while processing lbterm-inbox-open event: {:#}", e);
                            e
                        })?
                }
                None => true,
            };
            if default {
                let spawn = SpawnCommand {
                    cwd: Some(message.dir.clone()),
                    domain: SpawnTabDomain::CurrentPaneDomain,
                    ..Default::default()
                };
                window
                    .window
                    .notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        tw.spawn_command(&spawn, crate::spawn::SpawnWhere::NewTab);
                    })));
            }
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            open(lua, window, pane, message)
        }))
        .detach();
    }

    /// While the list's open it takes every mouse event: true when it did
    pub(crate) fn lbterm_inbox_list_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        if self.lbterm_inbox.list.is_none() {
            return false;
        }
        let waiting = crate::inbox::waiting();
        let Ok(Some(laid)) = self.lbterm_inbox_list_layout(&waiting) else {
            self.lbterm_inbox_list_close();
            return false;
        };
        let layout = &laid.list;
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let hit = layout.hit(x, y).filter(|&i| i < waiting.len());
        match event.kind {
            WMEK::Move => {
                let focus = hit.map(|i| waiting[i].file.clone());
                if let Some(list) = self.lbterm_inbox.list.as_mut() {
                    if list.focus != focus && (focus.is_some() || layout.card.contains(x, y)) {
                        list.focus = focus;
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
                Some(index) => self.lbterm_inbox_list_pick(index),
                None if layout.card.contains(x, y) => {}
                // The button among them: a second click closes it
                None => self.lbterm_inbox_list_close(),
            },
            WMEK::Press(_) if !layout.card.contains(x, y) => self.lbterm_inbox_list_close(),
            WMEK::VertWheel(n) => {
                if let Some(list) = self.lbterm_inbox.list.as_mut() {
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

    /// While the list's open it takes every key: up and down the list, Enter to open, Esc to
    /// close. True when it did.
    pub(crate) fn lbterm_inbox_list_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        let Some(focus) = self.lbterm_inbox.list.as_ref().map(|l| l.focus.clone()) else {
            return false;
        };
        if *key == KeyCode::Escape {
            self.lbterm_inbox_list_close();
            return true;
        }
        let waiting = crate::inbox::waiting();
        let count = waiting.len();
        if count == 0 {
            return true;
        }
        let focus = focus.and_then(|file| waiting.iter().position(|m| m.file == file));
        // Enter picks the row lit; with none lit it lights the first, as Home does
        if *key == KeyCode::Enter && focus.is_some() {
            if let Some(index) = focus {
                self.lbterm_inbox_list_pick(index);
            }
            return true;
        }
        let shown = self
            .lbterm_inbox_list_layout(&waiting)
            .ok()
            .flatten()
            .map_or(1, |l| l.list.rows.len().max(1));
        // Positions down the list, newest first
        let at = focus.map(|i| count - 1 - i);
        let to = match key {
            KeyCode::UpArrow => Some(at.map_or(0, |p| p.saturating_sub(1))),
            KeyCode::DownArrow => Some(at.map_or(0, |p| (p + 1).min(count - 1))),
            KeyCode::PageUp => Some(at.map_or(0, |p| p.saturating_sub(shown))),
            KeyCode::PageDown => Some(at.map_or(0, |p| (p + shown).min(count - 1))),
            KeyCode::Home | KeyCode::Enter => Some(0),
            KeyCode::End => Some(count - 1),
            _ => {
                self.lbterm_menu_stray(Slot::List, key, mods);
                None
            }
        };
        if let (Some(to), Some(list)) = (to, self.lbterm_inbox.list.as_mut()) {
            list.focus = Some(waiting[count - 1 - to].file.clone());
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

    /// Draw the list, if it's open: the tab menu's card, a row a message
    pub(crate) fn paint_lbterm_inbox_list(&mut self) -> anyhow::Result<()> {
        if self.lbterm_inbox.list.is_none() {
            return Ok(());
        }
        let waiting = crate::inbox::waiting();
        let Some(laid) = self.lbterm_inbox_list_layout(&waiting)? else {
            return Ok(());
        };
        let (layout, cell_w, project_w) = (&laid.list, laid.cell_w, laid.project_w);
        // Scrolled past the end: held at the oldest
        if let Some(list) = self.lbterm_inbox.list.as_mut() {
            list.scroll = layout.scroll;
        }
        let focus = self
            .lbterm_inbox
            .list
            .as_ref()
            .and_then(|l| l.focus.as_ref())
            .and_then(|file| waiting.iter().position(|m| &m.file == file));
        let fill = self.lbterm_menu_fill();
        let card_rgb = fill.card;
        let scale = self.dimensions.dpi as f64 / 96.0;

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
        let mut lit = None;
        for (i, r) in &layout.rows {
            let y = r.y + (r.h - line_h) / 2.0;
            let stamp_x = layout.card.x + 12.0 * scale;
            let right = r.x + r.w - 10.0 * scale;
            let Some((stamp, project, repo, subject, from, taken)) = laid.rows.get(i).filter(|_| *i < waiting.len()) else {
                place(NONE_WAITING, stamp_x, y, right - stamp_x, palette::PROJECT);
                continue;
            };
            if focus == Some(*i) {
                lit = Some(*r);
            }
            place(stamp, stamp_x, y, layout.text_x - stamp_x, palette::PROJECT);
            let mut project_cols = ((project_w / cell_w) as usize).saturating_sub(2);
            let mut project_x = layout.text_x;
            // A repo's message: the projects card's blue repo mark before who it's for
            if *repo {
                let mark_w = REPO_MARK_COLS as f64 * cell_w;
                place(projects::REPO_MARK, project_x, y, mark_w, projects::REPO);
                project_x += mark_w;
                project_cols = project_cols.saturating_sub(REPO_MARK_COLS);
            }
            place(&strip::elide(project, project_cols), project_x, y, layout.text_x + project_w - project_x, palette::LABEL);
            // The subject first; then "taken 15:02" in the working green when a conversation has
            // taken it, which keeps its place by cutting the subject short; then who sent it, if
            // there's room. Each runs to the row's end: a box only as wide as its columns loses
            // its last glyph to rounding
            let subject_x = layout.text_x + project_w;
            // A hair over, so a row exactly as wide as the card was laid out for isn't a column
            // short by the float's rounding
            let room = ((right - subject_x) / cell_w + 1e-6).floor().max(0.0) as usize;
            let after = |words: &String| if words.is_empty() { 0 } else { words.chars().count() + 3 };
            let (from_cols, taken_cols) = (after(from), after(taken));
            let subject_cols = subject.chars().count().min(room.saturating_sub(taken_cols));
            let subject = strip::elide(subject, subject_cols);
            place(&subject, subject_x, y, right - subject_x, palette::TITLE);
            let mut next_x = subject_x + (subject.chars().count() as f64 + 3.0) * cell_w;
            if taken_cols > 0 {
                place(taken, next_x, y, right - next_x, inbox::LINK_TAKEN);
                next_x += taken_cols as f64 * cell_w;
            }
            if from_cols > 0 && subject.chars().count() + taken_cols + from_cols <= room {
                place(from, next_x, y, right - next_x, palette::PROJECT);
            }
        }

        // The light round it, its ring and card, and the row lit, gliding between rows: every
        // menu's (render/menu_fade.rs)
        self.lbterm_menu_card(Slot::List, layout.card, fill)?;
        let layer = gl_state.layer_for_zindex(CARD_Z)?;
        let mut quads = layer.quad_allocator();
        if let Some(r) = lit {
            self.lbterm_menu_light(Slot::List, &mut quads, r, card_rgb)?;
        }
        drop(quads);
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_inbox.words, &font, &metrics, room, TEXT_Z, words)
    }
}

/// LBTerm: the InMail button's list, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// How many messages the button says wait, while it shows
    pub(crate) fn lbterm_state_inbox(&self) -> Option<usize> {
        self.lbterm_inbox.shown.map(|(count, _)| count)
    }

    /// The list while it's open: the messages newest first, as it shows them ("project:
    /// subject"), or the one row saying none wait
    pub(crate) fn lbterm_state_inbox_list(&self) -> Option<lbterm_extras::state::Menu> {
        let list = self.lbterm_inbox.list.as_ref()?;
        let waiting = crate::inbox::waiting();
        let laid = self.lbterm_inbox_list_layout(&waiting).ok().flatten();
        let layout = laid.as_ref().map(|l| &l.list);
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        let rect_of = |index: usize| {
            layout
                .as_ref()
                .and_then(|l| l.rows.iter().find(|(i, _)| *i == index).map(|(_, r)| rect(r)))
        };
        // `checked` for a message a conversation has taken (lbterm/STATE.md)
        let item = |label: String, index: usize, checked: bool| lbterm_extras::state::MenuItem {
            label,
            checked,
            key: None,
            detail: None,
            rect: rect_of(index),
            inert: false,
        };
        let (items, focus) = if waiting.is_empty() {
            (vec![item(NONE_WAITING.to_string(), 0, false)], None)
        } else {
            let count = waiting.len();
            let focus = list
                .focus
                .as_ref()
                .and_then(|file| waiting.iter().position(|m| &m.file == file))
                .map(|i| count - 1 - i);
            (
                waiting
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, m)| item(format!("{}: {}", m.project, prompts::one_line(&m.subject)), i, m.taken.is_some()))
                    .collect(),
                focus,
            )
        };
        Some(lbterm_extras::state::Menu {
            kind: "inbox".to_string(),
            tab: None,
            rect: layout.as_ref().map(|l| rect(&l.card)),
            items,
            focus,
            renaming: None,
            waiting: None,
        })
    }

    /// What of the open list is at `x`, `y`, newest first: `Some(Some(item))` on a message,
    /// `Some(None)` on the card elsewhere, `None` off it
    pub(crate) fn lbterm_inbox_list_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        self.lbterm_inbox.list.as_ref()?;
        let waiting = crate::inbox::waiting();
        let laid = self.lbterm_inbox_list_layout(&waiting).ok()??;
        let layout = &laid.list;
        if !layout.card.contains(x, y) {
            return None;
        }
        let count = waiting.len();
        Some(layout.hit(x, y).filter(|&i| i < count).map(|i| count - 1 - i))
    }
}
