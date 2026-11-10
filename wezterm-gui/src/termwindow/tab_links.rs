//! LBTerm: tab links on the rail (lbterm/LINKS.md). A right-button drag of a tab's row onto
//! another's makes the first wait on the second (Lachlan's 1a, 2026-10-29): the press is only a
//! drag once it moves (`tabs::dragging`), and let go where it started it opens the tab menu, as a
//! right click did. Each thing done to a link (the drop, the menu's rows, a suggested line's
//! click) fires the `lbterm-tab-link` Lua event, `(window, pane, link)`, `link` being `{action,
//! waiter, on, objective}`: `toggle` (the drop: made, or the one there taken away), `cancel`,
//! `again`, `release` (what it waits for marked complete by hand), `confirm` (a suggested link made)
//! or `dismiss`, sessions by their ids. The config's
//! handler changes the links file (Terminal's `terminal links`), and the rail draws what the file
//! says; until it says it, for up to [`PENDING`], the rail draws the change already, so the drop
//! answers at once. Resting the pointer on a line opens a card saying who waits on whom and for
//! what (Lachlan's E3), without opening the panel.
//!
//! The tab connections (lbterm/LINKS.md, "Connections") fire the same event with tables of their
//! own (`lbterm_extras::links::Act`): a left click on a suggested connection's line `{action =
//! 'accept', connection}`; the drag let go with Shift held `{action = 'connect', waiter, on}`, which
//! connects the two or takes the dragged one out where they're connected already, Shift read live
//! so the drag looks cyan while it's held and is a wait link again once it's let go; and the tab
//! menu's rows `leave` `{connection, session}`, `accept` and `decline`. The rail draws each before
//! the file says it, as for links (`lbterm_extras::links::ConnectionChange`), and resting on a
//! connection's line opens its card.

use crate::scripting::guiwin::GuiWin;
use crate::termwindow::render::tab_panel::tab_session;
use crate::termwindow::TermWindow;
use ::window::{CursorIcon, Modifiers, MouseButtons, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use lbterm_extras::links::{self, Act, ConnectionChange, Link, LinkState, Links};
use lbterm_extras::tabs;
use mux::tab::TabId;
use mux::Mux;
use lbterm_panel::Hover;
use mux_lua::MuxPane;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a change asked of the links file shows before the file says it
const PENDING: Duration = Duration::from_secs(5);
/// The pointer rests on a line this long before its card opens, and the card waits this long after
/// it leaves: the tasks card's (render/tasks_card.rs)
const CARD_REST: Duration = Duration::from_millis(300);
const CARD_GRACE: Duration = Duration::from_millis(200);

/// A row the tab menu adds for one of a tab's links or connections
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuLink {
    pub(crate) label: String,
    pub(crate) act: MenuAct,
}

/// What a menu row for a link or a connection fires
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MenuAct {
    Link { action: &'static str, waiter: String, on: Option<String> },
    Connection(Act),
}

/// A tab being right-dragged onto another, to wait on it, or with Shift held to connect them
pub(crate) struct LinkDrag {
    pub(crate) tab: TabId,
    /// Its Claude session: without one a drop makes nothing
    pub(crate) session: Option<String>,
    /// Where the press was, on the screen, and in the window (the tab menu opens there when the
    /// press doesn't move)
    from: (f64, f64),
    press: (f64, f64),
    pub(crate) started: bool,
    /// Where the pointer is in the window
    pub(crate) at: (f64, f64),
    /// Shift is held, as the last key or pointer message said it: let go now, it connects the two
    /// tabs rather than making a link, and it looks cyan
    pub(crate) shift: bool,
}

/// A line on the rail: a tab link's, by its place in the rail's lines, or a tab connection's, by
/// its place in the rail's joins
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RailLine {
    Link(usize),
    Join(usize),
}

#[derive(Debug, Clone, PartialEq)]
enum Change {
    Add { what: String, objective: Option<u64> },
    Remove,
    Dismiss,
    /// What it waits for marked complete: the line goes green at once
    Release,
    /// Something done to a connection (`waiter` and `on` empty)
    Connection(ConnectionChange),
}

/// A change asked of the links file that it doesn't say yet
struct Pending {
    waiter: String,
    on: String,
    change: Change,
    asked: Instant,
}

pub(crate) struct TabLinksState {
    pub(crate) drag: Option<LinkDrag>,
    pending: Vec<Pending>,
    /// What the last paint showed of the drag (render/link_drag.rs)
    pub(crate) shown: crate::termwindow::render::link_drag::Shown,
    /// The line the pointer is on, where it came onto it, and when its card opens and closes
    /// (render/link_drag.rs)
    pub(crate) line: Option<(RailLine, f64)>,
    pub(crate) line_hover: Hover,
}

impl Default for TabLinksState {
    fn default() -> Self {
        Self {
            drag: None,
            pending: vec![],
            shown: Default::default(),
            line: None,
            line_hover: Hover::new(CARD_REST, CARD_GRACE),
        }
    }
}

impl TermWindow {
    /// A right press on the rail's row for `tab`: a drag once it moves, the tab menu if it's let go
    /// where it started
    pub(crate) fn lbterm_link_pressed(&mut self, tab: TabId, event: &MouseEvent) {
        let session = Mux::get().get_tab(tab).and_then(|t| tab_session(&t));
        self.lbterm_links.drag = Some(LinkDrag {
            tab,
            session,
            from: (event.screen_coords.x as f64, event.screen_coords.y as f64),
            press: (event.coords.x as f64, event.coords.y as f64),
            started: false,
            at: (event.coords.x as f64, event.coords.y as f64),
            shift: super::held::held(event.modifiers)[1],
        });
    }

    /// The modifiers a key or pointer message came with (held.rs): while a tab is dragged, Shift
    /// held makes it a connection's drag and let go a link's again, and the drag shows which at
    /// once, without waiting for the pointer to move
    pub(crate) fn lbterm_link_drag_held(&mut self, mods: Modifiers) {
        let shift = super::held::held(mods)[1];
        let Some(drag) = self.lbterm_links.drag.as_mut().filter(|d| d.shift != shift) else {
            return;
        };
        drag.shift = shift;
        if drag.started {
            log::debug!("tab links: the drag {}", if shift { "connects" } else { "links" });
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// The moves and the release of a right-button drag of a tab: true when it took the event.
    /// While it drags, the tab panel still sees the pointer, so the rail opens under it as ever.
    pub(crate) fn lbterm_link_drag_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(drag) = self.lbterm_links.drag.as_mut() else {
            return false;
        };
        let scale = self.dimensions.dpi as f64 / 96.0;
        match event.kind {
            WMEK::Move => {
                if !event.mouse_buttons.contains(MouseButtons::RIGHT) {
                    self.lbterm_links.drag = None;
                    context.lbterm_invalidate();
                    return false;
                }
                let screen = (event.screen_coords.x as f64, event.screen_coords.y as f64);
                if !drag.started {
                    if !tabs::dragging(drag.from, screen, scale) {
                        return false;
                    }
                    drag.started = true;
                    log::debug!("tab links: dragging tab {}", drag.tab);
                }
                drag.at = (event.coords.x as f64, event.coords.y as f64);
                self.tab_panel_mouse(event, context);
                context.set_cursor(Some(CursorIcon::Grabbing));
                context.lbterm_invalidate();
                true
            }
            WMEK::Release(MousePress::Right) => {
                // Where it's let go: the row under it, asked while the drag's still there
                if let Some(drag) = self.lbterm_links.drag.as_mut() {
                    drag.at = (event.coords.x as f64, event.coords.y as f64);
                }
                let target = self.lbterm_link_target();
                let drag = self.lbterm_links.drag.take().unwrap();
                // The panel saw the press: its release is this
                self.lbterm_panel.pressed = false;
                if !drag.started {
                    self.lbterm_open_tab_menu(drag.tab, drag.press);
                } else if let (true, Some(waiter), Some((_, on))) = (drag.shift, drag.session.clone(), target.clone()) {
                    // With Shift held as it's let go, the two are connected, or the dragged one
                    // taken out of the connection they share
                    self.lbterm_connection_fire(Act::Connect { waiter, on });
                } else if let (Some(waiter), Some((_, on))) = (drag.session.clone(), target) {
                    self.lbterm_link_wait_on(waiter, on);
                }
                context.set_cursor(Some(CursorIcon::Default));
                context.lbterm_invalidate();
                true
            }
            // Any other button while it drags puts the tab down where it was
            WMEK::Press(_) if drag.started => {
                self.lbterm_links.drag = None;
                context.lbterm_invalidate();
                true
            }
            _ => false,
        }
    }

    /// The tab being dragged onto another, while it is
    pub(crate) fn lbterm_link_dragging(&self) -> Option<&LinkDrag> {
        self.lbterm_links.drag.as_ref().filter(|d| d.started)
    }

    /// The rail's row under a drag that its tab would wait on, and that row's session: a tab with
    /// a Claude session that isn't the dragged one
    pub(crate) fn lbterm_link_target(&self) -> Option<(usize, String)> {
        let drag = self.lbterm_link_dragging()?;
        drag.session.as_ref()?;
        let layout = self.lbterm_panel.layout.as_ref()?;
        let (x, y) = (drag.at.0 as f32, drag.at.1 as f32);
        let row = layout.row_at(x, y, self.lbterm_panel.hover.is_open())?;
        let session = self.lbterm_panel.links.sessions.get(row)?.clone()?;
        (Some(&session) != drag.session.as_ref()).then_some((row, session))
    }

    /// The rows the tab menu adds for `tab`'s links: Stop waiting on each tab it waits on, Wait
    /// on it again where the link went wrong, and a suggested link to make or dismiss; then its
    /// connections': Leave each one made, and make or decline each one suggested
    pub(crate) fn lbterm_link_menu(&self, tab: TabId) -> Vec<MenuLink> {
        if self.config.lbterm_links.is_none() {
            return vec![];
        }
        let Some(waiter) = Mux::get().get_tab(tab).and_then(|t| tab_session(&t)) else {
            return vec![];
        };
        let rail = &self.lbterm_panel.links;
        let name = |s: &str| {
            rail.labels
                .get(s)
                .cloned()
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| "the other tab".to_string())
        };
        let mut rows = vec![];
        for l in rail.said.of(&waiter) {
            let row = |label: String, action| MenuLink {
                label,
                act: MenuAct::Link { action, waiter: waiter.clone(), on: Some(l.on.clone()) },
            };
            rows.push(row(format!("Stop waiting on {}", name(&l.on)), "cancel"));
            // What it waits for marked complete by hand, in Lachlan's words (his E2): the waiting
            // session is told now
            if matches!(l.state, LinkState::Waiting | LinkState::Stuck | LinkState::Failed) {
                let task = if l.what.is_empty() { "Its task".to_string() } else { l.what.clone() };
                rows.push(row(format!("{task} is now complete by {}", name(&l.on)), "release"));
            }
            if matches!(l.state, LinkState::Stuck | LinkState::Failed) {
                rows.push(row(format!("Wait on {} again", name(&l.on)), "again"));
            }
        }
        if let Some(on) = rail.said.suggestion(&waiter).and_then(|w| w.on.clone()) {
            let link = |action, on| MenuAct::Link { action, waiter: waiter.clone(), on };
            rows.push(MenuLink { label: format!("Wait on {}, as suggested", name(&on)), act: link("confirm", Some(on)) });
            rows.push(MenuLink { label: "Dismiss the suggested link".into(), act: link("dismiss", None) });
        }
        // The connections it's in, under its links (lbterm/LINKS.md, "Connections")
        rows.extend(
            links::connection_rows(&rail.said, &waiter, name)
                .into_iter()
                .map(|(label, act)| MenuLink { label, act: MenuAct::Connection(act) }),
        );
        rows
    }

    /// Fire what a menu row for a link or a connection does
    pub(crate) fn lbterm_link_menu_fire(&mut self, act: MenuAct) {
        match act {
            MenuAct::Link { action, waiter, on } => self.lbterm_link_fire(action, waiter, on, None),
            MenuAct::Connection(act) => self.lbterm_connection_fire(act),
        }
    }

    /// Fire `lbterm-tab-link` for an act on a tab connection, and show the change on the rail
    /// until the links file says it
    pub(crate) fn lbterm_connection_fire(&mut self, act: Act) {
        log::info!("tab connections: {} {:?}", act.action(), act.fields());
        let change = ConnectionChange::of(&act, &self.lbterm_panel.links.said);
        self.lbterm_links.pending.retain(|p| match &p.change {
            Change::Connection(was) => !change.replaces(was),
            _ => true,
        });
        self.lbterm_links.pending.push(Pending {
            waiter: String::new(),
            on: String::new(),
            change: Change::Connection(change),
            asked: Instant::now(),
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        let mut fields: Vec<(&'static str, String)> = vec![("action", act.action().to_string())];
        fields.extend(act.fields());
        self.lbterm_tab_link_event(fields, None);
    }

    /// Make session `waiter` wait on session `on`, or stop it waiting there if it does: a right
    /// drag let go on `on`'s row, or `on` picked on the tab menu's Wait on… page (render/
    /// tab_menu.rs). `lbterm-tab-link {action='toggle', waiter, on}`, with the objective its wait
    /// names when that wait is on `on` (his E6)
    pub(crate) fn lbterm_link_wait_on(&mut self, waiter: String, on: String) {
        let objective = self
            .lbterm_panel
            .links
            .said
            .wait(&waiter)
            .filter(|w| w.on.as_deref() == Some(on.as_str()))
            .and_then(|w| w.objective);
        self.lbterm_link_fire("toggle", waiter, Some(on), objective);
    }

    /// Fire `lbterm-tab-link` for `action` on the link from `waiter` to `on`, and show the change
    /// on the rail until the links file says it
    pub(crate) fn lbterm_link_fire(&mut self, action: &'static str, waiter: String, on: Option<String>, objective: Option<u64>) {
        log::info!("tab links: {action} {waiter} -> {on:?}");
        let said = self.lbterm_panel.links.said.clone();
        let change = match (action, on.as_deref()) {
            ("toggle", Some(o)) if said.between(&waiter, o).is_some() => Some(Change::Remove),
            ("toggle", Some(_)) | ("confirm", Some(_)) => Some(Change::Add {
                what: said.wait(&waiter).map(|w| w.what.clone()).unwrap_or_default(),
                objective,
            }),
            ("cancel", Some(_)) => Some(Change::Remove),
            ("release", Some(_)) => Some(Change::Release),
            ("dismiss", _) => Some(Change::Dismiss),
            _ => None,
        };
        if let Some(change) = change {
            self.lbterm_links.pending.retain(|p| p.waiter != waiter || on.as_deref().map_or(false, |o| p.on != o));
            self.lbterm_links.pending.push(Pending {
                waiter: waiter.clone(),
                on: on.clone().unwrap_or_default(),
                change,
                asked: Instant::now(),
            });
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        let mut fields: Vec<(&'static str, String)> = vec![("action", action.to_string()), ("waiter", waiter)];
        if let Some(on) = on {
            fields.push(("on", on));
        }
        self.lbterm_tab_link_event(fields, objective);
    }

    /// The `lbterm-tab-link` event, `(window, pane, link)`, `link` a table of `fields` (strings)
    /// and `objective` (a number) when there's one
    fn lbterm_tab_link_event(&mut self, fields: Vec<(&'static str, String)>, objective: Option<u64>) {
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        let window = GuiWin::new(self);
        let pane = MuxPane(pane.pane_id());

        async fn fire(
            lua: Option<std::rc::Rc<mlua::Lua>>,
            window: GuiWin,
            pane: MuxPane,
            fields: Vec<(&'static str, String)>,
            objective: Option<u64>,
        ) -> anyhow::Result<()> {
            let Some(lua) = lua else {
                return Ok(());
            };
            let table = lua.create_table()?;
            for (key, value) in fields {
                table.set(key, value)?;
            }
            if let Some(objective) = objective {
                table.set("objective", objective)?;
            }
            let args = lua.pack_multi((window, pane, table))?;
            config::lua::emit_event(&lua, ("lbterm-tab-link".to_string(), args))
                .await
                .map_err(|e| {
                    log::error!("while processing lbterm-tab-link event: {:#}", e);
                    e
                })?;
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            fire(lua, window, pane, fields, objective)
        }))
        .detach();
    }

    /// The links file's word with the changes asked of it laid over, those it says already or
    /// that have waited [`PENDING`] dropped; and when the next of those still showing runs out
    pub(crate) fn lbterm_links_pending(&mut self, said: Arc<Links>) -> (Arc<Links>, Option<Instant>) {
        let now = Instant::now();
        let pending = &mut self.lbterm_links.pending;
        pending.retain(|p| {
            let done = match &p.change {
                Change::Add { .. } => said.between(&p.waiter, &p.on).is_some(),
                Change::Remove => said.between(&p.waiter, &p.on).is_none(),
                Change::Dismiss => said.suggestion(&p.waiter).is_none(),
                Change::Release => said
                    .between(&p.waiter, &p.on)
                    .map_or(true, |l| matches!(l.state, LinkState::Sending | LinkState::Sent)),
                Change::Connection(c) => c.said(&said),
            };
            !done && now.duration_since(p.asked) < PENDING
        });
        if pending.is_empty() {
            return (said, None);
        }
        let mut shown: Links = (*said).clone();
        for p in pending.iter() {
            match &p.change {
                Change::Add { what, objective } => shown.links.push(Link {
                    waiter: p.waiter.clone(),
                    on: p.on.clone(),
                    what: what.clone(),
                    objective: *objective,
                    state: LinkState::Waiting,
                    note: String::new(),
                }),
                Change::Remove => shown.links.retain(|l| !(l.waiter == p.waiter && l.on == p.on)),
                Change::Dismiss => {
                    for w in shown.waits.iter_mut().filter(|w| w.session == p.waiter) {
                        w.dismissed = true;
                    }
                }
                Change::Release => {
                    for l in shown.links.iter_mut().filter(|l| l.waiter == p.waiter && l.on == p.on) {
                        l.state = LinkState::Sending;
                    }
                }
                Change::Connection(c) => c.apply(&mut shown),
            }
        }
        let next = pending.iter().map(|p| p.asked + PENDING).min();
        (Arc::new(shown), next)
    }

    /// The rail's line under the pointer at `x`, `y`: on its lane between its ends, or across from
    /// the lane to an end's mark, a couple of pixels either side counting (`links::on_line`). The
    /// links' lines are taken as ever, the first the pointer is on; a connection's line, whose ends
    /// are its members' rows, is taken over that only when it's nearer the pointer, the nearest of
    /// them otherwise: the lanes are closer together than the slack either side of each (3 px at
    /// 96 DPI), and a connection's lane passes rows where the next lane's line has a mark
    pub(crate) fn lbterm_link_line_at(&self, x: f32, y: f32) -> Option<RailLine> {
        let layout = self.lbterm_panel.layout?;
        if self.config.lbterm_links.is_none() || !layout.on_link_lanes(x) {
            return None;
        }
        let slack = layout.link_width() / 2.0 + layout.px(2.0);
        let mark = layout.link_end_size() / 2.0;
        let distance = |lane: usize, rows: &[usize]| {
            let ends: Vec<(f32, f32)> = rows.iter().map(|&r| layout.link_end(r)).collect();
            links::line_distance((x, y), layout.link_lane(lane), &ends, slack, mark, layout.px(1.0))
        };
        let rail = &self.lbterm_panel.links;
        let link = rail.lines.iter().enumerate().find_map(|(k, l)| {
            (layout.row_visible(l.waiter) && layout.row_visible(l.on))
                .then(|| distance(l.lane, &[l.waiter, l.on]))
                .flatten()
                .map(|d| (k, d))
        });
        let join = rail
            .joins
            .iter()
            .enumerate()
            .filter(|(_, j)| j.rows.iter().all(|&r| layout.row_visible(r)))
            .filter_map(|(k, j)| distance(j.lane, &j.rows).map(|d| (k, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match (link, join) {
            (Some((_, near)), Some((k, d))) if d < near => Some(RailLine::Join(k)),
            (Some((k, _)), _) => Some(RailLine::Link(k)),
            (None, join) => join.map(|(k, _)| RailLine::Join(k)),
        }
    }

    /// Whether a line on the rail is a suggestion a click makes: a suggested link's, or a
    /// suggested connection's
    pub(crate) fn lbterm_link_line_suggested(&self, line: RailLine) -> bool {
        let rail = &self.lbterm_panel.links;
        match line {
            RailLine::Link(k) => rail.lines.get(k).map_or(false, |l| l.look == links::Look::Suggested),
            RailLine::Join(k) => rail.joins.get(k).map_or(false, |j| j.look == links::Look::ConnectionSuggested),
        }
    }

    /// The pointer on line `line` (or none) at `y`: its card opens once it has rested there and
    /// goes once it has left (Lachlan's E3), level with where the pointer came onto the line, so
    /// it stays put while the pointer runs along it. Whether what shows changed, and when the card
    /// next opens or closes.
    pub(crate) fn lbterm_link_line_pointer(&mut self, line: Option<RailLine>, y: f32, now: Instant) -> (bool, Option<Instant>) {
        let state = &mut self.lbterm_links;
        let before = (state.line_hover.is_open(), state.line.map(|(k, _)| k));
        // Onto another line while a card shows: that one's, at once
        if let Some(k) = line.filter(|&k| state.line.map(|(was, _)| was) != Some(k)) {
            state.line = Some((k, y as f64));
        }
        state.line_hover.pointer(line.is_some(), now);
        if line.is_none() && !state.line_hover.is_open() {
            state.line = None;
        }
        let after = (state.line_hover.is_open(), state.line.map(|(k, _)| k));
        (before != after, state.line_hover.deadline())
    }

    /// The pointer left the window: a line's card goes at once
    pub(crate) fn lbterm_link_line_leave(&mut self) -> bool {
        self.lbterm_links.line = None;
        self.lbterm_links.line_hover.close()
    }

    /// The line whose card shows, if one does
    pub(crate) fn lbterm_link_line_card(&mut self, now: Instant) -> Option<(RailLine, f64)> {
        let hover = &mut self.lbterm_links.line_hover;
        hover.tick(now);
        if !hover.is_open() {
            // Closed after its grace, rather than still resting before it opens
            if hover.deadline().is_none() {
                self.lbterm_links.line = None;
            }
            return None;
        }
        let rail = &self.lbterm_panel.links;
        self.lbterm_links.line.filter(|(line, _)| match line {
            RailLine::Link(k) => *k < rail.lines.len(),
            RailLine::Join(k) => *k < rail.joins.len(),
        })
    }

    /// A left press on line `line`: a suggested one is made (Lachlan's E7: one click confirms it),
    /// a suggested connection accepted. True when it was one.
    pub(crate) fn lbterm_link_line_click(&mut self, line: RailLine) -> bool {
        let line = match line {
            RailLine::Link(k) => k,
            RailLine::Join(k) => {
                let rail = &self.lbterm_panel.links;
                let Some(c) = rail
                    .joins
                    .get(k)
                    .filter(|j| j.look == links::Look::ConnectionSuggested)
                    .and_then(|j| rail.said.connections.get(j.connection))
                else {
                    return false;
                };
                let connection = c.id.clone();
                self.lbterm_connection_fire(Act::Accept { connection });
                return true;
            }
        };
        let rail = &self.lbterm_panel.links;
        let Some(line) = rail.lines.get(line).filter(|l| l.look == links::Look::Suggested) else {
            return false;
        };
        let (Some(Some(waiter)), Some(Some(on))) = (rail.sessions.get(line.waiter), rail.sessions.get(line.on)) else {
            return false;
        };
        let (waiter, on) = (waiter.clone(), on.clone());
        let objective = rail.said.wait(&waiter).and_then(|w| w.objective);
        self.lbterm_link_fire("confirm", waiter, Some(on), objective);
        true
    }
}
