//! LBTerm: what a window says it shows, for the state file (lbterm/STATE.md). The end of each
//! paint gathers it here and hands it to crate::state_probe, which writes every window's to the
//! file off this thread. Nothing here runs without `LBTERM_STATE_FILE`.
//!
//! Most of LBTerm's motion runs in DWM (compositor.rs), and the GUI paints nothing while it
//! plays. So the compositor keeps what it started, by name, and a window with something in
//! flight asks for one frame more when the last of it ends: that frame's snapshot says
//! `animating: false`, which is how a test knows things have settled.

use crate::state_probe;
use crate::tabbar::TabBarItem;
use crate::termwindow::render::tab_panel;
use crate::termwindow::{TermWindow, TermWindowNotif, UIItemType};
use lbterm_extras::state::{self, Animation, Wait};
use mux::pane::{Pane, PaneId};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::Mux;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::surface::SequenceNo;
use wezterm_term::Progress;
use window::{IntegratedTitleButton, WindowOps};

/// The frame asked for once DWM's animations end comes this long after, so they have; and after
/// the compositor's own wake 2 ms past a fade's end (render/composited.rs, `lbterm_fading_wake`),
/// which puts what faded away with a commit and no frame: were this frame first, its `prepare`
/// would put them away, and no test could tell whether the wake did, which Lachlan's windows,
/// with no state probe painting, rely on (the review of 2026-10-30)
const PAST_THE_END: Duration = Duration::from_millis(20);

/// A window's part in the state file.
#[derive(Default)]
pub struct StateProbe {
    /// Frames this window has painted
    frames: u64,
    /// When a repaint is already on its way for the end of what DWM plays
    wake: Option<Instant>,
    /// The visuals the last frame drew and kept (render/composited.rs)
    visuals: std::cell::RefCell<Option<state::Visuals>>,
    /// The templates this frame made again (render/kept.rs)
    built: std::cell::RefCell<Vec<&'static str>>,
    /// Each pane as the frame being painted drew it (`lbterm_state_drawn`)
    drawn: HashMap<PaneId, Drawn>,
}

/// A pane as a frame drew it: its cursor, and its output so far (its seqno) as it was drawn
#[derive(Clone, Copy)]
struct Drawn {
    cursor: state::Cell,
    seqno: SequenceNo,
}

/// What `item` is called in the state file, and the window button it is, if one
fn item_name(item: &UIItemType) -> (&'static str, Option<String>) {
    match item {
        UIItemType::TabBar(TabBarItem::WindowButton(button)) => (
            "strip.button",
            Some(
                match button {
                    IntegratedTitleButton::Hide => "minimize",
                    IntegratedTitleButton::Maximize => "maximize",
                    IntegratedTitleButton::Close => "close",
                }
                .to_string(),
            ),
        ),
        UIItemType::TabBar(TabBarItem::LbtermRestart) => ("strip.pill", Some("restart".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermSummary) => ("strip.words", Some("words".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermPrompt) => ("strip.prompt", Some("prompt".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermPromptList) => ("strip.arrow", Some("arrow".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermInbox) => ("strip.inbox", Some("inbox".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermAgenda) => ("strip.agenda", Some("agenda".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermContext) => ("strip.context", Some("context".to_string())),
        UIItemType::TabBar(TabBarItem::LbtermProjects) => ("strip.projects", Some("projects".to_string())),
        UIItemType::TabBar(TabBarItem::Tab { .. }) => ("tab", None),
        UIItemType::TabBar(_) => ("strip", None),
        UIItemType::CloseTab(_) => ("tab.close", None),
        UIItemType::AboveScrollThumb | UIItemType::ScrollThumb | UIItemType::BelowScrollThumb => {
            ("scrollbar", None)
        }
        UIItemType::Split(_) => ("split", None),
    }
}

fn progress(progress: Progress) -> Option<String> {
    match progress {
        Progress::None => None,
        Progress::Percentage(n) => Some(format!("{n}%")),
        Progress::Error(n) => Some(format!("error {n}%")),
        Progress::Indeterminate => Some("indeterminate".to_string()),
        Progress::Paused => Some("paused".to_string()),
    }
}

impl TermWindow {
    /// The window exists: it's in the file before its first frame
    pub(crate) fn lbterm_state_created(&mut self) {
        if state_probe::on() {
            // The file's own wake after DWM's motion ends (lbterm_state_wake) is the measuring's,
            // a test window's alone: not counted at rest by the budget
            lbterm_extras::budget::own(file!());
            let window = self.lbterm_state_window(Instant::now());
            state_probe::publish(window);
        }
    }

    /// A frame was painted: what it shows goes to the file, and when DWM has something in
    /// flight, the frame its end needs is asked for
    pub(crate) fn lbterm_state_frame(&mut self) {
        if !state_probe::on() {
            return;
        }
        self.lbterm_state.frames += 1;
        state_probe::painted();
        let now = Instant::now();
        let window = self.lbterm_state_window(now);
        let compositor = self.webgpu.as_ref().and_then(|w| w.compositor.as_ref());
        let settles = compositor.and_then(|c| c.settles());
        // And when a suggested connection's light ends its laps, so a frame says its loop is
        // gone (lbterm_extras::glow): whichever comes first, the frame then asking for the other
        let laps = compositor.and_then(|c| c.loops_until());
        let due = settles.into_iter().chain(laps).filter(|at| *at > now).min();
        self.lbterm_state_wake(due);
        state_probe::publish(window);
    }

    /// The frames this window has painted, for what a frame is judged by (render/new_tab.rs)
    pub(crate) fn lbterm_state_frames(&self) -> u64 {
        self.lbterm_state.frames
    }

    /// A frame begins: what the last one drew of each pane is the last one's
    pub(crate) fn lbterm_state_frame_start(&mut self) {
        self.lbterm_state.drawn.clear();
        self.lbterm_state.built.borrow_mut().clear();
    }

    /// This frame made the template `name` again (render/kept.rs)
    pub(crate) fn lbterm_state_built(&self, name: &'static str) {
        if state_probe::on() {
            self.lbterm_state.built.borrow_mut().push(name);
        }
    }

    /// `pane` as this frame draws it (render/pane.rs, with the cursor it draws): the state
    /// file's `drawn_cursor`, and what a key's echo is judged by (budget.rs), so a frame that
    /// began before a pane's output and ended after it isn't taken for one that shows it.
    /// Kept only while something reads it: the state file, or a key waiting on its echo
    pub(crate) fn lbterm_state_drawn(
        &mut self,
        pane: &Arc<dyn Pane>,
        cursor: &StableCursorPosition,
        dims: &RenderableDimensions,
    ) {
        if !state_probe::on() && !self.lbterm_budget_waits_on_output() {
            return;
        }
        let drawn = Drawn {
            cursor: state::Cell {
                x: cursor.x,
                y: (cursor.y - dims.physical_top) as i64,
            },
            seqno: pane.get_current_seqno(),
        };
        self.lbterm_state.drawn.insert(pane.pane_id(), drawn);
    }

    /// The output `pane` had when this frame drew it, if this frame drew it
    pub(crate) fn lbterm_state_drawn_seqno(&self, pane: PaneId) -> Option<SequenceNo> {
        self.lbterm_state.drawn.get(&pane).map(|d| d.seqno)
    }

    /// The window has gone
    pub(crate) fn lbterm_state_closed(&mut self) {
        if state_probe::on() {
            state_probe::forget(self.mux_window_id as u64);
        }
    }

    /// Paint again just after `at`, when what DWM plays has ended
    fn lbterm_state_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at.map(|at| at + PAST_THE_END) else {
            return;
        };
        // One already due sooner repaints, and the paint asks again
        if matches!(self.lbterm_state.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_state.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_state.wake == Some(at) {
                    tw.lbterm_state.wake = None;
                }
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    pub(crate) fn lbterm_state_hwnd(&self) -> u64 {
        if let Some(webgpu) = self.webgpu.as_ref() {
            return webgpu.handle.hwnd() as u64;
        }
        // Not window_handle(): it borrows the window's state, which the OpenGL front end's
        // paint already holds, and panicked every test window that didn't ask for WebGpu
        #[cfg(windows)]
        if let Some(window) = self.window.as_ref() {
            return window.lbterm_hwnd() as u64;
        }
        0
    }

    /// The waits under way at `now`, soonest to end first: the state file's `waits`, and what
    /// keeps a window from being at rest for the budget (termwindow/budget.rs)
    pub(crate) fn lbterm_waits(&self, now: Instant) -> Vec<Wait> {
        let mut waits: Vec<Wait> = vec![];
        waits.extend(self.tab_panel_waits(now));
        waits.extend(self.lbterm_strip_waits(now));
        waits.extend(self.lbterm_objectives_waits(now));
        waits.extend(self.lbterm_projects_waits(now));
        waits.extend(self.lbterm_cost_waits(now));
        waits.extend(self.lbterm_scroll_strip_waits(now));
        waits.extend(self.lbterm_context_bar_waits(now));
        waits.extend(self.lbterm_tasks_waits(now));
        waits.extend(self.lbterm_prompt_note_wait(now));
        waits.extend(self.lbterm_readout_wait(now));
        waits.extend(self.lbterm_conductor_wait(now));
        waits.extend(self.lbterm_view_wait(now));
        // A prompt jump scrolls Claude Code's view until it lands; when isn't known ahead
        if self.lbterm_prompt_jumping() {
            waits.push(Wait {
                name: "strip.jump".to_string(),
                left_ms: 0,
            });
        }
        if let Some(landed) = self.lbterm_state_landed(now) {
            waits.push(Wait {
                name: "landed".to_string(),
                left_ms: landed.left_ms,
            });
        }
        waits.sort_by(|a, b| a.left_ms.cmp(&b.left_ms).then_with(|| a.name.cmp(&b.name)));
        waits
    }

    /// Everything the file says about this window at `now`
    fn lbterm_state_window(&self, now: Instant) -> state::Window {
        let compositor = self.webgpu.as_ref().and_then(|w| w.compositor.as_ref());
        let focused = self.focused.is_some();

        // What's moving: DWM's (the hearts too, with the compositor), the hearts the GUI draws
        // frame by frame without it, and upstream's frames (a blinking cursor, an animated
        // image), which it only asks for while focused
        let mut animations = compositor.map(|c| c.animations(now)).unwrap_or_default();
        let (hearts, flying) = self.lbterm_state_hearts(now);
        if let Some((from, to)) = flying {
            let whole = to.saturating_duration_since(from).as_secs_f64();
            animations.push(Animation {
                name: "hearts".to_string(),
                progress: (now.saturating_duration_since(from).as_secs_f64() / whole.max(1e-9)).clamp(0.0, 1.0),
                left_ms: state::millis(to.saturating_duration_since(now)),
                played: vec![],
            });
        }
        if focused {
            if let Some(next) = *self.has_animation.borrow() {
                animations.push(Animation {
                    name: "wezterm".to_string(),
                    progress: 0.0,
                    left_ms: state::millis(next.saturating_duration_since(now)),
                    played: vec![],
                });
            }
        }
        animations.sort_by(|a, b| a.left_ms.cmp(&b.left_ms).then_with(|| a.name.cmp(&b.name)));

        let waits = self.lbterm_waits(now);
        let landed = self.lbterm_state_landed(now);

        // What DWM was given to play for ever, from the compositor's own tree: the plan's `spin`
        // would pass a check while the code applying it was broken (lbterm/PITFALLS.md, "Measuring")
        let loops: Vec<String> = compositor
            .map(|c| c.loops().into_iter().map(str::to_string).collect())
            .unwrap_or_default();

        // The tabs, the window's lock let go before the panes are asked anything
        let (tabs, active_tab) = match Mux::get().get_window(self.mux_window_id) {
            Some(window) => (
                window.iter_tabs().cloned().collect::<Vec<_>>(),
                window.get_active_tab_idx(),
            ),
            None => (vec![], 0),
        };
        let said_tabs: Vec<state::Tab> = tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| state::Tab {
                index,
                id: tab.tab_id() as u64,
                title: tab.get_title(),
                active: index == active_tab,
                panes: tab
                    .iter_panes_ignoring_zoom()
                    .iter()
                    .map(|p| p.pane.pane_id() as u64)
                    .collect(),
                zoomed: tab.get_zoomed_pane().is_some(),
                colour: crate::tabcolors::rgb(tab.tab_id())
                    .map(|(r, g, b)| state::hex(u32::from_be_bytes([0, r, g, b]))),
            })
            .collect();

        let active = self.get_active_pane_or_overlay();
        // The panes this frame drew: the open tab's, or the ones kept in their place while a held
        // pane's push hasn't come (frame_wait.rs)
        let panes: Vec<state::Pane> = self
            .lbterm_panes_to_paint()
            .iter()
            .map(|pos| {
                let pane = &pos.pane;
                let id = pane.pane_id();
                let rect = self
                    .lbterm_card(pos)
                    .map(|a| state::Rect::round(a.x as f64, a.y as f64, a.w as f64, a.h as f64))
                    .unwrap_or_default();
                let cells = self
                    .lbterm_pane_cells(pos)
                    .map(|a| state::Rect::round(a.x as f64, a.y as f64, a.w as f64, a.h as f64))
                    .unwrap_or_default();
                let dims = pane.get_dimensions();
                let cursor = pane.get_cursor_position();
                let server = tab_panel::server_pane_id(pane);
                state::Pane {
                    id: id as u64,
                    server_id: (server != id as u64).then_some(server),
                    rect,
                    cells,
                    cols: pos.width,
                    rows: pos.height,
                    active: pos.is_active,
                    zoomed: pos.is_zoomed,
                    title: pane.get_title(),
                    alt_screen: pane.is_alt_screen_active(),
                    mouse_reporting: pane.is_mouse_grabbed(),
                    cursor: state::Cell {
                        x: cursor.x,
                        y: (cursor.y - dims.physical_top) as i64,
                    },
                    drawn_cursor: self.lbterm_state.drawn.get(&id).map(|d| d.cursor),
                    scroll_top: self.get_viewport(id).map(|top| top as i64),
                    session: pane.copy_user_vars().get("claude_session").cloned(),
                    progress: progress(pane.get_progress()),
                    weights: self.lbterm_state_weights(id),
                }
            })
            .collect();

        let shown = active.as_ref().map_or(false, |p| self.lbterm_scroll_bar_shows(p));
        let scrollbar = state::Scrollbar {
            enabled: self.show_scroll_bar,
            shown,
            marks: match active.as_ref() {
                Some(p) if shown => self.lbterm_state_marks(p.pane_id()),
                _ => 0,
            },
        };

        // An overlay or modal over the panes: the active pane isn't the tab's own
        let overlay = if self.get_modal().is_some() {
            Some("modal".to_string())
        } else {
            let own = tabs.get(active_tab).and_then(|t| t.get_active_pane()).map(|p| p.pane_id());
            match active.as_ref() {
                Some(p) if Some(p.pane_id()) != own => Some(p.get_title()),
                _ => None,
            }
        };

        let lit: Vec<String> = self
            .lbterm_hover
            .borrow()
            .slots
            .lit()
            .filter_map(|item| item_name(item).1)
            .collect();
        let items: Vec<state::Item> = self
            .ui_items
            .iter()
            .filter_map(|item| match item_name(&item.item_type) {
                (over, Some(name)) if over.starts_with("strip.") => Some(state::Item {
                    name,
                    rect: state::Rect {
                        x: item.x as i32,
                        y: item.y as i32,
                        w: item.width as u32,
                        h: item.height as u32,
                    },
                }),
                _ => None,
            })
            .collect();

        let menu = self
            .lbterm_state_tab_menu()
            .or_else(|| self.lbterm_state_prompt_list())
            .or_else(|| self.lbterm_state_conductor_menu())
            .or_else(|| self.lbterm_state_inbox_list())
            .or_else(|| self.lbterm_state_agenda_list())
            .or_else(|| self.lbterm_state_plus_menu());

        let animating = !animations.is_empty();
        state::Window {
            id: self.mux_window_id as u64,
            hwnd: self.lbterm_state_hwnd(),
            frame: self.lbterm_state.frames,
            focused,
            keyboard: focused || self.lbterm_keyboard_assumed,
            held: self.lbterm_state_held(),
            tint: self.lbterm_state_tint(),
            animating,
            animations,
            settled: !animating && waits.is_empty(),
            waits,
            loops,
            size: state::Size {
                width: self.dimensions.pixel_width as u32,
                height: self.dimensions.pixel_height as u32,
                dpi: self.dimensions.dpi as u32,
            },
            pointer: self
                .current_mouse_event
                .as_ref()
                .map(|e| self.lbterm_state_pointer(e.coords.x, e.coords.y)),
            active_tab: (!tabs.is_empty()).then_some(active_tab),
            active_pane: active.as_ref().map(|p| p.pane_id() as u64),
            tabs: said_tabs,
            panes_tab: self.lbterm_painted_tab_id().map(|id| id as u64),
            panes,
            fading_panes: compositor
                .map(|c| c.fading_panes().0.into_iter().map(|id| id as u64).collect())
                .unwrap_or_default(),
            fading_sides: compositor.map_or(0, |c| c.fading_sides().0),
            fading_swaps: compositor.map_or(0, |c| c.fading_swaps().0),
            spare_panes: compositor.map_or(0, |c| c.spare_panes()),
            scrollbar,
            rail: self.lbterm_panel.said.clone(),
            strip: self.lbterm_state_strip(lit, items),
            objectives: self.lbterm_state_objectives(),
            scroll_strip: self.lbterm_state_scroll_strip(),
            context_bar: self.lbterm_state_context_bar(),
            projects: self.lbterm_state_projects(),
            cost: self.lbterm_state_cost(),
            menu,
            overlay,
            readout: self.lbterm_state_readout(),
            landed,
            questions: self.lbterm_state_questions(),
            questions_pane: self.lbterm_state_questions_pane(),
            thumbnails: self.lbterm_state_thumbnails(),
            hearts,
            conductor_view: self.lbterm_state_view(),
            tasks: self.lbterm_tasks_said(),
            drafts: self.lbterm_state_drafts(),
            ticked: self.lbterm_drafts.ticked.clone(),
            next: self.lbterm_drafts.next.clone(),
            new_tab: self.lbterm_state_new_tab(),
            copied: self.lbterm_menu.copied.borrow().clone(),
            visuals: self.lbterm_state.visuals.borrow().clone(),
        }
    }

    /// This frame drew the visuals `drawn` names and kept `kept` as they were
    /// (render/composited.rs)
    pub(crate) fn lbterm_state_visuals(
        &self,
        drawn: impl ExactSizeIterator<Item = lbterm_motion::Target<mux::pane::PaneId>>,
        kept: usize,
    ) {
        if state_probe::on() {
            *self.lbterm_state.visuals.borrow_mut() = Some(state::Visuals {
                drawn: drawn.len(),
                kept,
                which: drawn.map(|target| format!("{target:?}")).collect(),
                ..Default::default()
            });
        }
    }

    /// What drawing this frame made and wrote on the GPU (termwindow/gpu_work.rs), the passes it
    /// recorded and the submits it made, and the templates it made again (render/kept.rs), beside
    /// the visuals it drew, and the bind groups the window keeps after it
    pub(crate) fn lbterm_state_work(&self, work: lbterm_motion::content::Work, bind_groups: usize) {
        if state_probe::on() {
            if let Some(v) = self.lbterm_state.visuals.borrow_mut().as_mut() {
                v.made = work.made;
                v.bind_groups = bind_groups;
                v.uploads = work.uploads;
                v.upload_bytes = work.upload_bytes;
                v.built = self.lbterm_state.built.borrow().iter().map(|s| s.to_string()).collect();
                v.passes = work.passes;
                v.submits = work.submits;
                v.upload_us = work.upload_us;
                v.record_us = work.record_us;
                v.submit_us = work.submit_us;
                v.stale = crate::termwindow::gpu_work::checking().then_some(work.stale);
            }
        }
    }

    /// The pointer at `x`, `y` in the window, and what's under it: a menu over everything, then
    /// the tab panel, the strip's card, the strip and the scrollbar, then the panes
    fn lbterm_state_pointer(&self, x: isize, y: isize) -> state::Pointer {
        let mut pointer = state::Pointer {
            x: x as i32,
            y: y as i32,
            over: "window".to_string(),
            row: None,
            item: None,
            pane: None,
            button: None,
        };
        let (fx, fy) = (x as f64, y as f64);
        for under in [
            self.lbterm_tab_menu_under(fx, fy),
            self.lbterm_prompt_list_under(fx, fy),
            self.lbterm_conductor_menu_under(fx, fy),
            self.lbterm_inbox_list_under(fx, fy),
            self.lbterm_agenda_list_under(fx, fy),
        ] {
            if let Some(item) = under {
                pointer.over = if item.is_some() { "menu.item" } else { "menu" }.to_string();
                pointer.item = item;
                return pointer;
            }
        }
        // The new-tab picker and its + menu, over everything but the menus, and the pad's zone
        // (render/new_tab.rs): "menu" or "menu.item" for the + menu, "picker" with its row,
        // "rail.new_tab"
        if let Some((over, item)) = self.lbterm_new_tab_under(fx, fy) {
            match over {
                "menu" => {
                    pointer.over = if item.is_some() { "menu.item" } else { "menu" }.to_string();
                    pointer.item = item;
                }
                _ => {
                    pointer.over = over.to_string();
                    pointer.row = item;
                }
            }
            return pointer;
        }
        if self.lbterm_tasks_card_at(fx, fy) {
            pointer.over = "rail.tasks".to_string();
            return pointer;
        }
        if let Some((over, row)) = self.tab_panel_under(x as f32, y as f32) {
            pointer.over = over.to_string();
            pointer.row = row;
            return pointer;
        }
        // The Conductor's view, over the panes while it's open
        if let Some((over, button)) = self.lbterm_view_under(fx, fy) {
            pointer.over = over.to_string();
            pointer.button = button;
            return pointer;
        }
        if self.lbterm_strip_card_at(fx, fy) {
            pointer.over = "strip.card".to_string();
            return pointer;
        }
        // The cost pane under the strip (render/cost.rs)
        if let Some(over) = self.lbterm_cost_under(fx, fy) {
            pointer.over = over.to_string();
            return pointer;
        }
        // The projects pad on the strip's second row and its card (render/projects.rs)
        if let Some((over, row)) = self.lbterm_projects_under(fx, fy) {
            pointer.over = over.to_string();
            pointer.row = row;
            return pointer;
        }
        if let Some((over, row)) = self.lbterm_objectives_under(fx, fy) {
            pointer.over = over.to_string();
            pointer.row = row;
            return pointer;
        }
        if let Some(over) = self.lbterm_context_bar_under(fx, fy) {
            pointer.over = over.to_string();
            return pointer;
        }
        if let Some(over) = self.lbterm_scroll_strip_under(fx, fy) {
            pointer.over = over.to_string();
            return pointer;
        }
        if let Some(item) = self.ui_items.iter().rev().find(|item| item.hit_test(x, y)) {
            let (over, button) = item_name(&item.item_type);
            pointer.over = over.to_string();
            pointer.button = button;
            if let UIItemType::TabBar(TabBarItem::Tab { tab_idx, .. }) | UIItemType::CloseTab(tab_idx) =
                &item.item_type
            {
                pointer.item = Some(*tab_idx);
            }
            return pointer;
        }
        let on = |r: &lbterm_motion::PixelRect| {
            x as i32 >= r.x && (x as i32) < r.x + r.w as i32 && y as i32 >= r.y && (y as i32) < r.y + r.h as i32
        };
        if let Some((id, _)) = self.lbterm_spans.panes().iter().find(|(_, r)| on(r)) {
            pointer.over = "pane".to_string();
            pointer.pane = Some(*id as u64);
        }
        pointer
    }
}
