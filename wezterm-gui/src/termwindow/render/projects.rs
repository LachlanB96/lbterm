//! LBTerm: the projects card (lbterm/OBJECTIVES.md, "The projects card"; Lachlan's ask,
//! 2026-10-24). A pad at the top of the objectives strip, level with the Conductor's on the tab
//! rail (at the strip's foot without one), opens a card over the panes while the pointer rests on
//! it: every project in `lbterm_projects`, the busiest first, each with the Claude sessions open in
//! it in this window's tabs and every other (working, waiting on him, idle: the rail's colours),
//! their subagents and background tasks, and the InMail waiting for it (the InMail button's
//! folders, crate::inbox). On the row under the pointer, buttons fire the `lbterm-project-action`
//! Lua event with the project and what to do: start a session, one told to read its InMail, or
//! resume its last. Without a handler, New opens a tab in its folder; a handler that returns false
//! does it its own way (Terminal's lbterm.lua, through the Conductor and the Claude project).
//!
//! The pad is drawn with the strip, in `over`, as shapes for render/sides.rs; the card fades as the objectives' cards do, a third
//! card in their visual (`Target::Objectives(2)`). Where everything goes is lbterm_extras::projects.

use crate::scripting::guiwin::GuiWin;
use crate::termwindow::render::objectives::{over, CARD, ROW_HOVER_ALPHA, TEXT_ALPHA, NAME_ALPHA, TITLE_ALPHA, EVIDENCE_ALPHA};
use crate::termwindow::render::sides::Shape;
use crate::termwindow::render::tab_panel::srgb;
use crate::termwindow::TermWindowNotif;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::{DimensionContext, SrgbaTuple};
use lbterm_extras::projects::{self, Action, Card, Counts, Tone};
use lbterm_extras::strip::{self as strip_card, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Hover, Layout, RowState};
use mux::pane::CachePolicy;
use mux::Mux;
use mux_lua::MuxPane;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Which of the objectives' cards this is (render/objectives.rs, `CARD`)
const WHICH: usize = 2;

pub struct ProjectsState {
    hover: Hover,
    /// Where the pad is, the strip's width across: what the pointer rests on to open the card
    zone: Option<Rect>,
    /// The pad's middle
    pad: Option<(f64, f64)>,
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
}

impl Default for ProjectsState {
    fn default() -> Self {
        Self {
            hover: Hover::new(Duration::from_millis(300), Duration::from_millis(200)),
            zone: None,
            pad: None,
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

    /// Put away: the panel is off
    pub(crate) fn reset(&mut self) {
        self.hover.close();
        (self.zone, self.pad, self.card, self.row, self.chip, self.visual) = (None, None, None, None, None, None);
    }

    pub(crate) fn visual(&self) -> Option<(PixelRect, bool)> {
        self.visual
    }

    /// Where the pad's zone is, for the objectives' strip to leave alone
    pub(crate) fn zone(&self) -> Option<Rect> {
        self.zone
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
/// edge, the room between ceiling and floor, the pad's zone, its width, a column and a line, the
/// scale, as f64 bits)
#[derive(PartialEq)]
struct CardAt {
    folders: Rc<ProjectFolders>,
    counts: Vec<Counts>,
    geometry: [u64; 9],
}

impl PartialEq for ProjectFolders {
    /// The same list made once: by its address
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
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
        let state = &self.lbterm_objectives.projects;
        state.card.is_some() || state.hover.is_open()
    }

    /// Each paint, from the objectives strip's (render/objectives.rs): the pad, and the card while
    /// it's open or fading (`fading`: its visual still shows something)
    pub(crate) fn paint_lbterm_projects(
        &mut self,
        layout: &Layout,
        strip: Rect,
        surface: SrgbaTuple,
        fading: bool,
        scale: f64,
    ) -> anyhow::Result<()> {
        // Borrowed from the config, which is shared: nothing of it is copied for a paint
        let config = self.config.clone();
        let Some(settings) = config.lbterm_projects.as_ref().filter(|p| !p.projects.is_empty()) else {
            self.lbterm_objectives.projects.reset();
            return Ok(());
        };
        let (open_delay_ms, close_delay_ms) = config
            .lbterm_objectives
            .as_ref()
            .map(|o| (o.open_delay_ms, o.close_delay_ms))
            .unwrap_or_else(|| {
                let o = config::lbterm::LbtermObjectives::default();
                (o.open_delay_ms, o.close_delay_ms)
            });
        let motion = self.lbterm_motion();
        let closing = if self.lbterm_composited() && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        let state = &mut self.lbterm_objectives.projects;
        state.hover.set_delays(Duration::from_millis(open_delay_ms), Duration::from_millis(close_delay_ms));
        state.hover.set_motion(Duration::ZERO, closing);

        // Level with the Conductor's pad on the rail, else at the strip's foot
        let (middle, zone_y, zone_h) = if layout.head {
            let pad = layout.head_pad();
            let pill = layout.head_pill();
            ((pad.y + pad.h / 2.0) as f64, pill.y as f64, pill.h as f64)
        } else {
            let h = layout.row_height() as f64;
            let bottom = strip.y + strip.h;
            (bottom - h / 2.0, bottom - h, h)
        };
        let middle = middle.round();
        state.zone = Some(Rect { x: strip.x, y: zone_y, w: strip.w, h: zone_h });
        state.pad = Some((strip.x + strip.w / 2.0, middle));
        let open = state.hover.is_open();
        let lit = open || state.hover.light() != lbterm_panel::Light::Rest;
        let pad = self.lbterm_projects_draw_pad((strip.x + strip.w / 2.0, middle), lit, scale);
        self.lbterm_sides_draw(pad);

        let card = match strip_card::card_draw(open, fading) {
            CardDraw::Fresh => {
                let folders = self.lbterm_project_folders(settings);
                let counts = self.lbterm_projects_counts(&folders);
                let (_, _, cell) = self.lbterm_objectives_font()?;
                let room = self.lbterm_room()?;
                let gutter = self.lbterm_shape().gutter() as f64;
                let right = room.right as f64 - gutter;
                let width = settings
                    .card_width
                    .evaluate_as_pixels(DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: self.render_metrics.cell_size.width as f32,
                    }) as f64;
                let width = width.min(right - room.left as f64 - gutter).max(0.0).round();
                // Measured with all the room there is, then put by the pad: down from it at the
                // strip's top, up from it at the strip's foot
                let (ceiling, floor) = (layout.body_top as f64 + layout.gap() as f64, layout.bottom as f64 - layout.gap() as f64);
                // Laid out again only when what it shows or where it goes has changed: the same
                // counts in the same place are the same card (it was laid out twice a frame)
                let at = CardAt {
                    folders: Rc::clone(&folders),
                    counts,
                    geometry: [right, ceiling, floor, zone_y, zone_h, width, cell.0, cell.1, scale].map(f64::to_bits),
                };
                let state = &mut self.lbterm_objectives.projects;
                if let Some((_, card)) = state.laid.as_ref().filter(|(was, _)| *was == at) {
                    card.clone()
                } else {
                    let names: Vec<&str> = folders.projects.iter().map(|p| p.name.as_str()).collect();
                    let parents = &folders.parents;
                    let height = projects::card(&names, &at.counts, parents, right, (ceiling, floor), width, cell, scale).map_or(0.0, |c| c.rect.h);
                    let top = projects::place((zone_y, zone_y + zone_h), height, (ceiling, floor));
                    let card = projects::card(&names, &at.counts, parents, right, (top, floor), width, cell, scale).map(Rc::new);
                    state.counts = at.counts.clone();
                    state.laid = Some((at, card.clone()));
                    card
                }
            }
            CardDraw::Last => self.lbterm_objectives.projects.last_card.clone(),
            CardDraw::None => None,
        };
        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        let state = &mut self.lbterm_objectives.projects;
        state.visual = card.as_ref().and_then(|c| {
            strip_card::visual_bounds(&c.rect, scale, window).map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open))
        });
        state.card = if open { card.clone() } else { None };
        if !open {
            (state.row, state.chip) = (None, None);
        }
        if let Some(card) = card {
            self.lbterm_projects_draw_card(&card, surface, scale)?;
            self.lbterm_objectives.projects.last_card = Some(card);
        }
        Ok(())
    }

    /// The projects' folders and parents as the config lists them (lbterm_extras::projects::Folders),
    /// made again only when it lists others: the projects card and the InMail list ask which
    /// project a folder is in for every message and tab, every frame they're open
    pub(crate) fn lbterm_project_folders(&self, settings: &config::lbterm::LbtermProjects) -> Rc<ProjectFolders> {
        let mut kept = self.lbterm_objectives.projects.folders.borrow_mut();
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
        let mut kept = self.lbterm_objectives.projects.mail.borrow_mut();
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

    /// The pad: four small squares, grey, lit while the pointer's on it or the card is open, as
    /// shapes for the strip in `over` (render/sides.rs)
    fn lbterm_projects_draw_pad(&self, (cx, cy): (f64, f64), lit: bool, scale: f64) -> Vec<Shape> {
        let side = (3.0 * scale).round().max(2.0);
        let gap = (2.0 * scale).round().max(1.0);
        let whole = 2.0 * side + gap;
        let (x0, y0) = ((cx - whole / 2.0).round(), (cy - whole / 2.0).round());
        let colour = srgb(if lit { palette::TITLE } else { palette::LABEL });
        [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
            .map(|(i, j)| Shape::new(Rect { x: x0 + i * (side + gap), y: y0 + j * (side + gap), w: side, h: side }, 1.0, colour))
            .to_vec()
    }

    fn lbterm_projects_draw_card(&self, card: &Card, surface: SrgbaTuple, scale: f64) -> anyhow::Result<()> {
        let shape = self.lbterm_shape();
        let (radius, stroke) = (shape.radius() as f64, shape.hairline() as f64);
        self.lbterm_objectives_card(WHICH, card.rect, surface, radius, stroke, scale)?;
        let state = &self.lbterm_objectives.projects;
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
    /// it took the event. `busy`: a drag that began elsewhere, which carries on over it.
    pub(crate) fn lbterm_projects_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps, busy: bool) -> bool {
        let state = &mut self.lbterm_objectives.projects;
        let Some(zone) = state.zone else {
            return false;
        };
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut state.pressed) {
                return true;
            }
        }
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let gutter = self.lbterm_shape().gutter() as f64;
        let state = &mut self.lbterm_objectives.projects;
        let on_pad = x >= zone.x - gutter && y >= zone.y && y < zone.y + zone.h;
        let on_card = state.card.as_ref().map_or(false, |c| c.rect.contains(x, y));
        let inside = !busy && (on_pad || on_card);
        let now = Instant::now();
        let mut changed = state.hover.pointer(inside, now);
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
        self.lbterm_objectives_wake(due);
        if !inside {
            return false;
        }
        if let WMEK::Press(press) = &event.kind {
            self.lbterm_objectives.projects.pressed = true;
            // A press on the pad opens the card at once, without waiting out the rest that opens
            // it on its own (Lachlan's E5, 2026-10-29); the pointer closes it as usual
            if *press == MousePress::Left && on_pad && !on_card {
                if self.lbterm_objectives.projects.hover.open_now(now) {
                    context.lbterm_invalidate();
                }
                let due = self.lbterm_objectives.projects.hover.deadline();
                self.lbterm_objectives_wake(due);
            }
            if let (MousePress::Left, Some(i), Some(action)) = (press, row, chip) {
                let project = self.lbterm_objectives.projects.card.as_ref().and_then(|c| c.rows.get(i)).map(|r| r.project);
                if let Some(project) = project {
                    self.lbterm_projects_fire(project, action);
                    // Done with: it closes, and the tab it starts comes up in front
                    self.lbterm_objectives.projects.hover.close();
                    context.lbterm_invalidate();
                }
            }
        }
        if matches!(event.kind, WMEK::Move) {
            context.set_cursor(Some(if chip.is_some() { CursorIcon::Pointer } else { CursorIcon::Default }));
        }
        true
    }

    /// The pointer left the window
    pub(crate) fn lbterm_projects_mouse_leave(&mut self) {
        let state = &mut self.lbterm_objectives.projects;
        let changed = state.hover.left_window(Instant::now());
        (state.row, state.chip) = (None, None);
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        let due = self.lbterm_objectives.projects.deadline();
        self.lbterm_objectives_wake(due);
    }

    /// Fire `lbterm-project-action` for project `project` (its index in `lbterm_projects`): unless a
    /// handler returns false, New opens a new tab in its folder, on the active pane's domain, in front
    fn lbterm_projects_fire(&mut self, project: usize, action: Action) {
        let Some(p) = self.config.lbterm_projects.as_ref().and_then(|c| c.projects.get(project)).cloned() else {
            return;
        };
        let counts = self.lbterm_objectives.projects.counts.get(project).copied().unwrap_or_default();
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
        let state = &self.lbterm_objectives.projects;
        let zone = state.zone?;
        let names = self.config.lbterm_projects.as_ref().map(|c| c.projects.clone()).unwrap_or_default();
        Some(S::ProjectsCard {
            pad: at(&zone),
            open: state.card.is_some(),
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
        let state = &self.lbterm_objectives.projects;
        if let Some(card) = state.card.as_ref().filter(|c| c.rect.contains(x, y)) {
            return Some(("projects.card", card.rows.iter().position(|r| r.rect.contains(x, y))));
        }
        let zone = state.zone?;
        let gutter = self.lbterm_shape().gutter() as f64;
        (x >= zone.x - gutter && y >= zone.y && y < zone.y + zone.h).then_some(("projects.pad", None))
    }

    /// The card's waits under way at `now`
    pub(crate) fn lbterm_projects_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let hover = &self.lbterm_objectives.projects.hover;
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
