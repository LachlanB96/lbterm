//! LBTerm: the projects card (lbterm/OBJECTIVES.md, "The projects card"; Lachlan's ask,
//! 2026-10-24). A pad on the window strip's second row, right of the context pill under the InMail
//! button (in the pill's place without it; Lachlan's 1 of 2026-11-08, when the context bar took the
//! objectives strip's room, whose top it was on until then), opens a card over the panes while the
//! pointer rests on it: every project in `lbterm_projects`, the busiest first, each with the Claude sessions open in
//! it in this window's tabs and every other (working, waiting on him, idle: the rail's colours),
//! their subagents and background tasks, and the InMail waiting for it (the InMail button's
//! folders, crate::inbox). On the row under the pointer, buttons fire the `lbterm-project-action`
//! Lua event with the project and what to do: start a session, one told to read its InMail, or
//! resume its last. Without a handler, New opens a tab in its folder; a handler that returns false
//! does it its own way (Terminal's lbterm.lua, through the Conductor and the Claude project).
//!
//! The pad is a pill of the strip's (render/strip.rs, `lbterm_prompt_row`), its four squares a
//! poly, built again only when it comes or goes or its card opens or closes
//! ([`TermWindow::lbterm_projects_refresh`]); where it is the strip's laid-out item says. The card
//! hangs under the strip from it and fades as the objectives' cards do, a third card in their
//! visual (`Target::Objectives(2)`), which the compositor keeps whether the objectives strip is on
//! or not. It needs neither `lbterm_objectives` nor its strip: its rest and grace are its own
//! (`lbterm_projects.open_delay_ms`, `close_delay_ms`). Where everything goes is
//! lbterm_extras::projects.

use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::scripting::guiwin::GuiWin;
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{BorderColor, BoxDimension, Element, ElementColors, ElementContent, SizedPoly, VerticalAlign};
use crate::termwindow::render::objectives::{over, CARD, ROW_HOVER_ALPHA, TEXT_ALPHA, NAME_ALPHA, TITLE_ALPHA, EVIDENCE_ALPHA};
use crate::termwindow::render::tab_panel::srgb;
use crate::termwindow::{TermWindowNotif, UIItemType};
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, RectF, WindowOps};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::{Dimension, DimensionContext, SrgbaTuple};
use lbterm_extras::projects::{self, Action, Card, Counts, Tone};
use lbterm_extras::strip::{self as strip_card, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Hover, RowState};
use mux::pane::CachePolicy;
use mux::Mux;
use mux_lua::MuxPane;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use wezterm_font::LoadedFont;
use window::color::LinearRgba;

/// Which of the objectives' cards this is (render/objectives.rs, `CARD`)
const WHICH: usize = 2;

/// The pad's four squares, each 3/8 of its side with a quarter between them: 3 px with 2 between
/// at 96 DPI, as they were on the objectives strip
macro_rules! square {
    ($x0:expr, $x1:expr, $y0:expr, $y1:expr) => {
        Poly {
            path: &[
                PolyCommand::MoveTo($x0, $y0),
                PolyCommand::LineTo($x1, $y0),
                PolyCommand::LineTo($x1, $y1),
                PolyCommand::LineTo($x0, $y1),
                PolyCommand::LineTo($x0, $y0),
            ],
            intensity: BlockAlpha::Full,
            style: PolyStyle::Fill,
        }
    };
}
const SQUARES: &[Poly] = &[
    square!(BlockCoord::Zero, BlockCoord::Frac(3, 8), BlockCoord::Zero, BlockCoord::Frac(3, 8)),
    square!(BlockCoord::Frac(5, 8), BlockCoord::One, BlockCoord::Zero, BlockCoord::Frac(3, 8)),
    square!(BlockCoord::Zero, BlockCoord::Frac(3, 8), BlockCoord::Frac(5, 8), BlockCoord::One),
    square!(BlockCoord::Frac(5, 8), BlockCoord::One, BlockCoord::Frac(5, 8), BlockCoord::One),
];

pub struct ProjectsState {
    hover: Hover,
    /// The pad's pill on the strip, as the strip last laid it out: what the pointer rests on to
    /// open the card
    zone: Option<Rect>,
    /// What the strip was last built with: no pad (None), or the pad, lit for an open card or not.
    /// Another, and the strip is built again (`lbterm_projects_refresh`)
    shown: Option<bool>,
    /// When a repaint is already on its way
    wake: Option<Instant>,
    card: Option<Rc<Card>>,
    last_card: Option<Rc<Card>>,
    /// The card as last laid out, and what from: the same again is the same card
    laid: Option<(CardAt, Option<Rc<Card>>)>,
    /// The projects' folders as last made from the config (`lbterm_project_folders`), and each
    /// one's mail as last counted, with the list of messages it was counted from
    folders: RefCell<Option<Rc<ProjectFolders>>>,
    mail: RefCell<Option<(Rc<ProjectFolders>, Arc<Vec<lbterm_extras::inbox::Message>>, Vec<usize>)>>,
    /// Each project's counts as last drawn, by its index in `lbterm_projects.projects`
    counts: Vec<Counts>,
    /// The card's row under the pointer, and the button under it
    row: Option<usize>,
    chip: Option<Action>,
    visual: Option<(PixelRect, bool)>,
    pressed: bool,
    /// The pointer's on the pad: its arrival and leaving repaint, for the pad's look under the
    /// pointer and the state file's waits, since this takes the pad's events before the strip's
    /// own items would repaint for them (lbterm/PITFALLS.md, 215)
    on_pad: bool,
}

impl Default for ProjectsState {
    fn default() -> Self {
        Self {
            hover: Hover::new(Duration::from_millis(300), Duration::from_millis(200)),
            zone: None,
            shown: None,
            wake: None,
            card: None,
            last_card: None,
            laid: None,
            folders: RefCell::new(None),
            mail: RefCell::new(None),
            counts: vec![],
            row: None,
            chip: None,
            visual: None,
            pressed: false,
            on_pad: false,
        }
    }
}

impl ProjectsState {
    /// A wait is up
    pub(crate) fn tick(&mut self, now: Instant) {
        self.hover.tick(now);
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.hover.deadline()
    }

    /// Put away: the pad isn't on the strip
    pub(crate) fn reset(&mut self) {
        self.hover.close();
        (self.zone, self.card, self.row, self.chip, self.visual) = (None, None, None, None, None);
        self.on_pad = false;
    }

    pub(crate) fn visual(&self) -> Option<(PixelRect, bool)> {
        self.visual
    }
}

/// The projects as the config lists them, with each one's folder as folders are compared and the
/// project each is a repo of: made once for a list (`lbterm_project_folders`)
pub(crate) struct ProjectFolders {
    pub(crate) projects: Vec<config::lbterm::LbtermProject>,
    pub(crate) folders: projects::Folders,
    pub(crate) parents: Vec<Option<usize>>,
}

/// What the card was laid out from: the projects, their counts, and where it goes (its right
/// edge, its top under the strip and its floor, its width, a column and a line, the scale, as f64
/// bits)
#[derive(PartialEq)]
struct CardAt {
    folders: Rc<ProjectFolders>,
    counts: Vec<Counts>,
    geometry: [u64; 7],
}

impl PartialEq for ProjectFolders {
    /// The same list made once: by its address
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// The pad's squares in `rgb`, on nothing of their own: the pill's surface shows round them
fn squares_colors(rgb: [f32; 3]) -> ElementColors {
    ElementColors {
        border: BorderColor::default(),
        bg: LinearRgba::TRANSPARENT.into(),
        text: SrgbaTuple(rgb[0], rgb[1], rgb[2], 1.).to_linear().into(),
    }
}

/// A bit's colour: the rail's status words', InMail's waiting orange, subagents dimmer, a repo's
/// mark its own blue
fn tone_colour(tone: Tone, surface: SrgbaTuple) -> window::color::LinearRgba {
    match tone {
        Tone::Working => srgb(palette::status(RowState::Working)),
        Tone::Waiting | Tone::Mail => srgb(palette::status(RowState::Waiting)),
        Tone::Idle => srgb(palette::status(RowState::Idle)),
        Tone::Parts => over(surface, EVIDENCE_ALPHA),
        Tone::Repo => srgb(projects::REPO),
    }
}

impl crate::TermWindow {
    /// Whether the card is open, counting every window's tabs as it paints: a program's report in
    /// another window's tab can change it (lbterm_program_status)
    pub(crate) fn lbterm_projects_open(&self) -> bool {
        let state = &self.lbterm_projects;
        state.card.is_some() || state.hover.is_open()
    }

    /// Whether the pad shows: projects to list, and the strip's second row over the tab panel's
    /// rail, where the context pill is (render/context.rs, `lbterm_context_on`, less the pill's own
    /// key and the Conductor's view). Not `lbterm_objectives`: the pad left its strip on 2026-11-08
    pub(crate) fn lbterm_projects_on(&self) -> bool {
        self.config.lbterm_projects.as_ref().map_or(false, |p| !p.projects.is_empty())
            && self.config.lbterm_tab_panel.enabled
            && config::lbterm::hides_tabs(&self.config)
            && self.config.use_fancy_tab_bar
            && self.show_tab_bar
            && !self.config.tab_bar_at_bottom
            && self.lbterm_prompt_row_on()
    }

    /// Each paint, as the strip is refreshed (render/strip.rs): true when the strip must be built
    /// again, for the pad coming or going or its card opening or closing, which lights it
    pub(crate) fn lbterm_projects_refresh(&mut self) -> bool {
        let shown = self.lbterm_projects_on().then(|| self.lbterm_projects.hover.is_open());
        if shown == self.lbterm_projects.shown {
            return false;
        }
        self.lbterm_projects.shown = shown;
        true
    }

    /// The pad for the strip's second row (`row` its bounds), and how wide it is laid out alone:
    /// four small squares in a hairline pill, the context pill's kind, sitting back at rest, lit
    /// under the pointer and while its card is open. Nothing while it doesn't show. The strip
    /// places it after the context pill, or in the pill's place (render/strip.rs, `lbterm_prompt_row`)
    pub(crate) fn lbterm_projects_pad(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        row: RectF,
    ) -> anyhow::Result<Option<(Element, f32)>> {
        let Some(open) = self.lbterm_projects.shown else {
            return Ok(None);
        };
        let over = |alpha: f32| [surface.0, surface.1, surface.2].map(|c| c + (1. - c) * alpha);
        let rgb = |c: u32| {
            let (r, g, b) = palette::bytes(c);
            [r as f32 / 255., g as f32 / 255., b as f32 / 255.]
        };
        // The squares in the grey the objectives strip drew them in, its title's white lit; the
        // pill's hairline as the context pill's
        let lit = (over(0.50), rgb(palette::TITLE));
        let rest = if open { lit } else { (over(0.14), rgb(palette::LABEL)) };
        let scale = self.dimensions.dpi as f32 / 96.0;
        let side = (8.0 * scale).round().max(6.0);
        // As tall as a line of the pill's words, so it's the context pill's height
        let room = (metrics.cell_size.height as f32 - side).max(0.0);
        let top = (room / 2.0).floor();
        // And wide enough for the pill's round ends (`lbterm_strip_pill`: a radius of 0.6 of a
        // line, padding of 0.6 of a column each side, a pixel's hairline) with a straight run
        // between them: narrower, the ends' arcs crossed and the ring read as a broken circle
        let radius = (metrics.cell_size.height as f32 * 0.6).round();
        let pill = side + 2.0 * 0.6 * metrics.cell_size.width as f32 + 2.0;
        let wider = (2.0 * radius + (4.0 * scale).round() - pill).max(0.0).ceil();
        let side_pad = (wider / 2.0).floor();
        let squares = Element::new(
            font,
            ElementContent::Poly {
                line_width: 1,
                poly: SizedPoly {
                    poly: SQUARES,
                    width: Dimension::Pixels(side),
                    height: Dimension::Pixels(side),
                },
            },
        )
        .vertical_align(VerticalAlign::Middle)
        // Its own look under the pointer, as `inbox::pill_text` gives the pill's words: the hover
        // slot draws only the parts that have one (render/hover_fade.rs, `part_look`), so without
        // it the lit pill's surface covered the squares while it faded in (lbterm/PITFALLS.md, 216)
        .colors(squares_colors(rest.1))
        .hover_colors(Some(squares_colors(lit.1)))
        .padding(BoxDimension {
            left: Dimension::Pixels(side_pad),
            right: Dimension::Pixels(wider - side_pad),
            top: Dimension::Pixels(top),
            bottom: Dimension::Pixels(room - top),
        });
        self.lbterm_strip_pill(font, metrics, surface, row, TabBarItem::LbtermProjects, vec![squares], (rest.0, lit.0), (rest.1, lit.1))
            .map(Some)
    }

    /// Each paint, after the tab bar (whose laid-out pad it hangs the card from) and the tab panel:
    /// the card while it's open or fading. Nothing while the pad isn't on the strip
    pub(crate) fn paint_lbterm_projects(&mut self) -> anyhow::Result<()> {
        // Borrowed from the config, which is shared: nothing of it is copied for a paint
        let config = self.config.clone();
        let pad = self
            .ui_items
            .iter()
            .find(|i| i.item_type == UIItemType::TabBar(TabBarItem::LbtermProjects))
            .map(|i| Rect { x: i.x as f64, y: i.y as f64, w: i.width as f64, h: i.height as f64 });
        let (Some(settings), Some(pad), Some(layout), true) = (
            config.lbterm_projects.as_ref().filter(|p| !p.projects.is_empty()),
            pad,
            self.lbterm_panel.layout,
            self.lbterm_projects.shown.is_some(),
        ) else {
            self.lbterm_projects.reset();
            return Ok(());
        };
        let motion = self.lbterm_motion();
        let composited = self.lbterm_composited();
        let closing = if composited && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        let state = &mut self.lbterm_projects;
        state.hover.set_delays(Duration::from_millis(settings.open_delay_ms), Duration::from_millis(settings.close_delay_ms));
        // Coming back while the card fades out opens it again at once (lbterm_panel::hover)
        state.hover.set_motion(Duration::ZERO, closing);
        state.zone = Some(pad);
        let open = state.hover.is_open();
        let scale = self.dimensions.dpi as f64 / 96.0;
        let surface = self.palette().background;
        // Whether its visual still shows something as it fades, and when the fades are done
        let (fading, settles) = if composited {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or((false, None), |c| {
                    let (showing, settles) = c.objectives_showing(Instant::now());
                    (showing[WHICH], settles)
                })
        } else {
            (false, None)
        };

        let card = match strip_card::card_draw(open, fading) {
            CardDraw::Fresh => {
                let folders = self.lbterm_project_folders(settings);
                let counts = self.lbterm_projects_counts(&folders);
                let (_, _, cell) = self.lbterm_objectives_font()?;
                let room = self.lbterm_room()?;
                let gutter = self.lbterm_shape().gutter() as f64;
                // Down from the pad under the strip, as the InMail list hangs from its button, its
                // left edge at the pad's but never over the closed rail, which is over it
                // (lbterm/PITFALLS.md, 218)
                let top = self.get_os_border().top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
                let floor = layout.bottom as f64 - layout.gap() as f64;
                let left = pad.x.max(room.left as f64);
                let width = settings
                    .card_width
                    .evaluate_as_pixels(DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: self.render_metrics.cell_size.width as f32,
                    }) as f64;
                let width = width.min(room.right as f64 - gutter - left).max(0.0).round();
                let right = left + width;
                // Laid out again only when what it shows or where it goes has changed: the same
                // counts in the same place are the same card (it was laid out twice a frame)
                let at = CardAt {
                    folders: Rc::clone(&folders),
                    counts,
                    geometry: [right, top, floor, width, cell.0, cell.1, scale].map(f64::to_bits),
                };
                let state = &mut self.lbterm_projects;
                if let Some((_, card)) = state.laid.as_ref().filter(|(was, _)| *was == at) {
                    card.clone()
                } else {
                    let names: Vec<&str> = folders.projects.iter().map(|p| p.name.as_str()).collect();
                    let card = projects::card(&names, &at.counts, &folders.parents, right, (top, floor), width, cell, scale).map(Rc::new);
                    state.counts = at.counts.clone();
                    state.laid = Some((at, card.clone()));
                    card
                }
            }
            CardDraw::Last => self.lbterm_projects.last_card.clone(),
            CardDraw::None => None,
        };
        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        let state = &mut self.lbterm_projects;
        state.visual = card.as_ref().and_then(|c| {
            strip_card::visual_bounds(&c.rect, scale, window).map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open))
        });
        state.card = if open { card.clone() } else { None };
        if !open {
            (state.row, state.chip) = (None, None);
        }
        if let Some(card) = card {
            self.lbterm_projects_draw_card(&card, surface, scale)?;
            self.lbterm_projects.last_card = Some(card);
        }
        // A card closing is drawn until it has faded, then not; and the hover's wait
        self.lbterm_projects_wake(settles);
        let due = self.lbterm_projects.hover.deadline();
        self.lbterm_projects_wake(due);
        Ok(())
    }

    /// Repaint at `at`: when the hover's wait is up, or the card has faded
    pub(crate) fn lbterm_projects_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        if matches!(self.lbterm_projects.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_projects.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_projects.wake == Some(at) {
                    tw.lbterm_projects.wake = None;
                }
                tw.lbterm_projects.tick(Instant::now());
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// The projects' folders and parents as the config lists them (lbterm_extras::projects::Folders),
    /// made again only when it lists others: the projects card and the InMail list ask which
    /// project a folder is in for every message and tab, every frame they're open
    pub(crate) fn lbterm_project_folders(&self, settings: &config::lbterm::LbtermProjects) -> Rc<ProjectFolders> {
        let mut kept = self.lbterm_projects.folders.borrow_mut();
        if let Some(folders) = kept.as_ref().filter(|f| f.projects == settings.projects) {
            return Rc::clone(folders);
        }
        let names: Vec<&str> = settings.projects.iter().map(|p| p.name.as_str()).collect();
        let named: Vec<Option<&str>> = settings.projects.iter().map(|p| p.parent.as_deref()).collect();
        let dirs: Vec<&str> = settings.projects.iter().map(|p| p.dir.as_str()).collect();
        let folders = Rc::new(ProjectFolders {
            projects: settings.projects.clone(),
            folders: projects::Folders::new(&dirs),
            parents: projects::parents(&names, &named),
        });
        *kept = Some(Rc::clone(&folders));
        folders
    }

    /// Each project's InMail waiting, from the messages waiting now: counted again only when
    /// they or the projects are others (crate::inbox replaces its list when what waits changes)
    fn lbterm_projects_mail(&self, folders: &Rc<ProjectFolders>) -> Vec<usize> {
        let waiting = crate::inbox::waiting();
        let mut kept = self.lbterm_projects.mail.borrow_mut();
        if let Some((f, w, mail)) = kept.as_ref() {
            if Rc::ptr_eq(f, folders) && Arc::ptr_eq(w, &waiting) {
                return mail.clone();
            }
        }
        let mut mail = vec![0; folders.folders.len()];
        for m in waiting.iter() {
            if let Some(i) = folders.folders.of(&m.dir.to_string_lossy()) {
                mail[i] += 1;
            }
        }
        // The list itself is kept with them, so another can't be made where it was
        *kept = Some((Rc::clone(folders), waiting, mail.clone()));
        mail
    }

    /// Every project's counts: its InMail, and the Claude sessions in every window's tabs, each in
    /// the deepest project folder above the folder it's in now
    pub(crate) fn lbterm_projects_counts(&self, folders: &Rc<ProjectFolders>) -> Vec<Counts> {
        let mail = self.lbterm_projects_mail(folders);
        let mut counts: Vec<Counts> = mail.iter().map(|&mail| Counts { mail, ..Default::default() }).collect();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let idle_full = self.config.lbterm_tab_panel.idle_full_minutes * 60.0;
        let mux = Mux::get();
        for window_id in mux.iter_windows() {
            let Some(window) = mux.get_window(window_id) else {
                continue;
            };
            for (idx, tab) in window.iter_tabs().enumerate() {
                let t = super::tab_panel::panel_tab(tab, idx, false);
                let (row, _) = lbterm_panel::row(&t, now, idle_full);
                // A tab holding a session is one, whatever its light says: a finished one's is
                // stopped, and only Claude Code's ✳ in its title tells the rail it's still there
                if row.state == RowState::NoClaude && super::objectives::session_of(tab).is_none() {
                    continue;
                }
                // Where its session is now, as its statusline says; else its pane's folder
                let folder = if !t.cwd.is_empty() {
                    t.cwd.clone()
                } else {
                    tab.get_active_pane()
                        .and_then(|p| p.get_current_working_dir(CachePolicy::AllowStale))
                        .map(|u| u.to_string())
                        .unwrap_or_default()
                };
                let Some(i) = folders.folders.of(&folder) else {
                    continue;
                };
                match row.state {
                    // A stuck reply is still a turn under way
                    RowState::Working | RowState::Stuck => counts[i].working += 1,
                    RowState::Waiting => counts[i].waiting += 1,
                    _ => counts[i].idle += 1,
                }
                counts[i].parts += row.agents + row.waiting;
            }
        }
        counts
    }

    fn lbterm_projects_draw_card(&self, card: &Card, surface: SrgbaTuple, scale: f64) -> anyhow::Result<()> {
        let shape = self.lbterm_shape();
        let (radius, stroke) = (shape.radius() as f64, shape.hairline() as f64);
        self.lbterm_objectives_card(WHICH, card.rect, surface, radius, stroke, scale)?;
        let state = &self.lbterm_projects;
        {
            let gl_state = self.render_state.as_ref().unwrap();
            let layer = gl_state.layer_for_zindex(CARD[WHICH][1])?;
            let mut quads = layer.quad_allocator();
            if let Some(row) = state.row.and_then(|i| card.rows.get(i)) {
                let inset = 4.0 * scale;
                let band = Rect { x: row.rect.x + inset, y: row.rect.y + 1.0, w: row.rect.w - 2.0 * inset, h: row.rect.h - 2.0 };
                self.lbterm_menu_rounded(&mut quads, band, radius / 2.0, over(surface, ROW_HOVER_ALPHA))?;
                for chip in &row.chips {
                    let alpha = if state.chip == Some(chip.action) { 0.22 } else { 0.12 };
                    self.lbterm_menu_rounded(&mut quads, chip.rect, (chip.rect.h / 2.0).floor(), over(surface, alpha))?;
                }
            }
        }
        let right = card.rect.x + card.rect.w;
        let mut words = vec![(card.title.0, card.title.1, right - card.title.0, projects::TITLE.to_string(), over(surface, TITLE_ALPHA))];
        for bit in &card.legend {
            words.push((bit.x, card.title.1, right - bit.x, bit.text.clone(), tone_colour(bit.tone, surface)));
        }
        for (i, row) in card.rows.iter().enumerate() {
            if let Some(mark) = &row.repo {
                words.push((mark.x, row.text_y, row.name_x - mark.x, mark.text.clone(), tone_colour(mark.tone, surface)));
            }
            let name_w = row.bits.first().map_or(right, |b| b.x) - row.name_x;
            words.push((row.name_x, row.text_y, name_w, row.name.clone(), over(surface, if row.quiet { NAME_ALPHA } else { TEXT_ALPHA })));
            for bit in &row.bits {
                words.push((bit.x, row.text_y, right - bit.x, bit.text.clone(), tone_colour(bit.tone, surface)));
            }
            if state.row == Some(i) {
                for chip in &row.chips {
                    words.push((chip.text_x, row.text_y, chip.rect.w, chip.action.label().to_string(), over(surface, TITLE_ALPHA)));
                }
            }
        }
        if let Some((x, y, text)) = &card.more {
            words.push((*x, *y, right - *x, text.clone(), over(surface, NAME_ALPHA)));
        }
        self.lbterm_objectives_words(WHICH, &words)
    }

    /// The pointer moved, pressed or released: the card opens once it has rested on the pad and
    /// stays while it's on the pad or the card; a press on a button does what it says. True when
    /// it took the event: nothing under the pad or the card gets it, the strip's own handling of
    /// its items included (mouseevent.rs, before the objectives strip and the tab panel).
    pub(crate) fn lbterm_projects_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        // A drag that began elsewhere carries on over it without opening it
        let busy = self.current_mouse_capture.is_some() || self.window_drag_position.is_some() || self.dragging.is_some();
        let state = &mut self.lbterm_projects;
        let Some(zone) = state.zone else {
            return false;
        };
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut state.pressed) {
                return true;
            }
        }
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let state = &mut self.lbterm_projects;
        let on_pad = zone.contains(x, y);
        let on_card = state.card.as_ref().map_or(false, |c| c.rect.contains(x, y));
        let inside = !busy && (on_pad || on_card);
        let now = Instant::now();
        let mut changed = state.hover.pointer(inside, now);
        // A rest on the pad changes nothing the card shows until it opens, but the pad lights at
        // once and the wait is under way: repaint as the pointer comes and goes
        if on_pad != state.on_pad {
            state.on_pad = on_pad;
            changed = true;
        }
        let row = if on_card {
            state.card.as_ref().and_then(|c| c.rows.iter().position(|r| r.rect.contains(x, y)))
        } else {
            None
        };
        let chip = row
            .and_then(|i| state.card.as_ref()?.rows.get(i))
            .and_then(|r| r.chips.iter().find(|c| c.rect.contains(x, y)))
            .map(|c| c.action);
        if (row, chip) != (state.row, state.chip) {
            (state.row, state.chip) = (row, chip);
            changed = true;
        }
        if changed {
            context.lbterm_invalidate();
        }
        let due = state.hover.deadline();
        self.lbterm_projects_wake(due);
        if !inside {
            return false;
        }
        if let WMEK::Press(press) = &event.kind {
            self.lbterm_projects.pressed = true;
            // A press on the pad opens the card at once, without waiting out the rest that opens
            // it on its own (Lachlan's E5, 2026-10-29); the pointer closes it as usual
            if *press == MousePress::Left && on_pad && !on_card {
                if self.lbterm_projects.hover.open_now(now) {
                    context.lbterm_invalidate();
                }
                let due = self.lbterm_projects.hover.deadline();
                self.lbterm_projects_wake(due);
            }
            if let (MousePress::Left, Some(i), Some(action)) = (press, row, chip) {
                let project = self.lbterm_projects.card.as_ref().and_then(|c| c.rows.get(i)).map(|r| r.project);
                if let Some(project) = project {
                    self.lbterm_projects_fire(project, action);
                    // Done with: it closes, and the tab it starts comes up in front
                    self.lbterm_projects.hover.close();
                    context.lbterm_invalidate();
                }
            }
        }
        if matches!(event.kind, WMEK::Move) {
            // The pad is pressed to open the card at once, a button to do what it says
            context.set_cursor(Some(if chip.is_some() || (on_pad && !on_card) { CursorIcon::Pointer } else { CursorIcon::Default }));
        }
        true
    }

    /// The pointer left the window
    pub(crate) fn lbterm_projects_mouse_leave(&mut self) {
        let state = &mut self.lbterm_projects;
        let changed = state.hover.left_window(Instant::now()) | std::mem::take(&mut state.on_pad);
        (state.row, state.chip) = (None, None);
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        let due = self.lbterm_projects.deadline();
        self.lbterm_projects_wake(due);
    }

    /// Fire `lbterm-project-action` for project `project` (its index in `lbterm_projects`): unless a
    /// handler returns false, New opens a new tab in its folder, on the active pane's domain, in front
    fn lbterm_projects_fire(&mut self, project: usize, action: Action) {
        let Some(p) = self.config.lbterm_projects.as_ref().and_then(|c| c.projects.get(project)).cloned() else {
            return;
        };
        let counts = self.lbterm_projects.counts.get(project).copied().unwrap_or_default();
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        log::info!("projects: {} for {}", action.name(), p.name);
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        async fn fire(
            lua: Option<std::rc::Rc<mlua::Lua>>,
            window: GuiWin,
            pane: MuxPane,
            p: config::lbterm::LbtermProject,
            action: Action,
            counts: Counts,
        ) -> anyhow::Result<()> {
            let default = match lua {
                Some(lua) => {
                    let table = lua.create_table()?;
                    table.set("name", p.name.clone())?;
                    table.set("dir", p.dir.clone())?;
                    // The project's own folder, as the picker's events say it (render/new_tab.rs)
                    table.set("project_dir", p.dir.clone())?;
                    table.set("action", action.name())?;
                    table.set("mail", counts.mail)?;
                    table.set("sessions", counts.sessions())?;
                    // Where it came from, as the new-tab picker says "picker" (lbterm/NEW-TAB.md)
                    table.set("source", "card")?;
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
            if default && action == Action::New {
                let spawn = SpawnCommand {
                    cwd: Some(p.dir.clone().into()),
                    domain: SpawnTabDomain::CurrentPaneDomain,
                    ..Default::default()
                };
                window.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.spawn_command(&spawn, crate::spawn::SpawnWhere::NewTab);
                })));
            }
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            fire(lua, window, pane, p, action, counts)
        }))
        .detach();
    }

    /// What the card shows, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_state_projects(&self) -> Option<lbterm_extras::state::ProjectsCard> {
        use lbterm_extras::state as S;
        let at = |r: &Rect| S::Rect::round(r.x, r.y, r.w, r.h);
        let state = &self.lbterm_projects;
        let zone = state.zone?;
        let names = self.config.lbterm_projects.as_ref().map(|c| c.projects.clone()).unwrap_or_default();
        Some(S::ProjectsCard {
            pad: at(&zone),
            open: state.card.is_some(),
            card: state.card.as_ref().map(|c| at(&c.rect)),
            rows: state
                .card
                .as_ref()
                .map(|c| {
                    c.rows
                        .iter()
                        .map(|r| {
                            let n = state.counts.get(r.project).copied().unwrap_or_default();
                            S::ProjectRow {
                                name: names.get(r.project).map(|p| p.name.clone()).unwrap_or_default(),
                                parent: r.repo.as_ref().and_then(|_| names.get(r.project)?.parent.clone()),
                                mail: n.mail,
                                working: n.working,
                                waiting: n.waiting,
                                idle: n.idle,
                                parts: n.parts,
                                rect: at(&r.rect),
                                chips: r.chips.iter().map(|c| S::ProjectChip { action: c.action.name().to_string(), rect: at(&c.rect) }).collect(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            more: state.card.as_ref().and_then(|c| c.more.as_ref().map(|m| m.2.clone())),
            hovered: state.row,
        })
    }

    /// What of the card or pad is at `x`, `y`, for the state file's pointer
    pub(crate) fn lbterm_projects_under(&self, x: f64, y: f64) -> Option<(&'static str, Option<usize>)> {
        let state = &self.lbterm_projects;
        if let Some(card) = state.card.as_ref().filter(|c| c.rect.contains(x, y)) {
            return Some(("projects.card", card.rows.iter().position(|r| r.rect.contains(x, y))));
        }
        state.zone?.contains(x, y).then_some(("projects.pad", None))
    }

    /// The card's waits under way at `now`
    pub(crate) fn lbterm_projects_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let hover = &self.lbterm_projects.hover;
        let name = match hover.phase() {
            lbterm_panel::Phase::Opening => "projects.open",
            lbterm_panel::Phase::Closing => "projects.close",
            _ => return vec![],
        };
        hover
            .deadline()
            .map(|due| lbterm_extras::state::Wait {
                name: name.to_string(),
                left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
            })
            .into_iter()
            .collect()
    }
}
