//! LBTerm: what the budget (lbterm_extras::budget, lbterm/PERF.md, "Warnings in the daily build")
//! is told of each window. What happens to it (input, its open tab's output, the mux, a reload)
//! keeps it busy; what its background tabs say is news, which keeps the process from being at
//! rest but leaves the window's frames judged; each frame is handed over with the sites that
//! asked for it, and, only when it would otherwise be at rest, what's in flight (DWM's
//! animations, the hearts, the state file's waits); what repaints at rest on purpose says so
//! with a tick as it arms its timer; and each timed action is begun where it starts and done at
//! the end of the first frame that shows it, into its histogram in `metrics`' `budget.actions`.
//! When the timing to the screen is on (lbterm_extras::present, off unless asked), the actions a
//! frame showed go on to present.rs.

use super::{MuxWindowId, TermWindow, TermWindowNotif};
use lbterm_extras::budget::{self, Action, Purpose, Settings};
use mux::pane::{Pane, PaneId};
use mux::tab::TabId;
use mux::{Mux, MuxNotification};
use std::sync::Arc;
use std::time::{Duration, Instant};
use window::WindowEvent;

/// How long after a reload starts a window's frame can still be what shows it
const RELOAD_AT_MOST: Duration = Duration::from_secs(10);

/// What says a timed action has been shown, checked as each frame ends
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shown {
    /// The next frame: the action was done before it, on the GUI thread
    Next,
    /// The next frame that draws the open tab's panes: not one that draws the tab before's in
    /// their place while a held pane's push hasn't come (frame_wait.rs)
    TabPanes,
    /// A frame with more tabs in the window than this
    Tabs(usize),
    /// A frame with more panes than this in tab `tab`
    Panes { tab: TabId, from: usize },
    /// A frame without tab `tab` in the window
    TabGone(TabId),
    /// A frame that drew pane `pane` past `seqno`: its output as the frame drew it (the
    /// state's `lbterm_state_drawn`), not as it is when the frame ends, which output that came
    /// while the frame was being drawn has moved on though the frame didn't draw it
    Output { pane: PaneId, seqno: usize },
    /// A frame with the InMail button's list open
    InboxList,
    /// A frame with the agenda's list open
    AgendaList,
    /// A frame with the Conductor's view open
    View,
}

/// A window's part in the budget
#[derive(Debug, Default)]
pub(crate) struct BudgetState {
    /// The actions begun and not yet shown, with what shows each
    pending: Vec<(u64, Shown)>,
    /// How many of the rail's rows count seconds, as its last paint left them, and the card of
    /// what a tab is running's: the clock's rate
    pub rail_ticking: usize,
    pub tasks_ticking: usize,
    /// When the card's next age changes: its wake is a clock tick only then
    pub tasks_aged: Option<Instant>,
    /// The last reload timed
    reload: u64,
    /// This frame's own paint
    paint: Option<u64>,
    /// Frames painted: the first few aren't a paint's to time, but start-up's
    frames: u64,
}

/// A window's first frames build its glyph atlas and the GPU's pipelines (the first took 178 ms
/// in a test window, 2026-10-30): start-up, timed as that, not as a paint
const FIRST_FRAMES: u64 = 3;

impl BudgetState {
    /// How often the clocks in view may repaint at rest: once a second for each row of the rail
    /// and each task on its card counting seconds, and once for the minutes and idle steps
    pub fn clock_rate(&self) -> f64 {
        (self.rail_ticking + self.tasks_ticking) as f64 + 1.0
    }
}

/// config's `lbterm_budget` as the budget takes it
fn settings(config: &config::ConfigHandle) -> Settings {
    let b = &config.lbterm_budget;
    let mut actions = [0.0; Action::COUNT];
    for (i, (name, ms)) in b.actions.by_name().iter().enumerate() {
        debug_assert_eq!(*name, Action::ALL[i].name());
        actions[i] = *ms;
    }
    Settings {
        enabled: b.enabled,
        quiet: Duration::from_millis(b.quiet_ms),
        rest_frames: b.rest_frames,
        rest_news_frames: b.rest_news_frames,
        rest_timers: b.rest_timers,
        rest_tasks: b.rest_tasks,
        rest_task_ms: b.rest_task_ms,
        actions,
    }
}

/// The budget's waker: a timer on the GUI thread that says what's due when it ends. Its task
/// and timer are this file's, which the budget leaves out of what runs at rest
fn wake_after(after: Duration) {
    promise::spawn::spawn_into_main_thread(async move {
        smol::Timer::after(after).await;
        budget::wake_up();
    })
    .detach();
}

/// The GUI starts (counters.rs): the budget's waker, on the GUI thread
pub fn start() {
    budget::set_waker(wake_after, file!());
}

/// LBTerm: whether `n` is a mux domain pane's output out of `window`'s open tab, which the mux's
/// subscription then takes as news here, with no task and no hop to the window: all the window
/// did with one was take it as news ([`TermWindow::lbterm_budget_notif`]) and find it out of view
/// (`mux_pane_output_event`), two tasks on the GUI thread for every push of every busy tab out of
/// view. Another pane's output (one in the open tab, an overlay's, a local pane's) goes to the
/// window as before. The pane when it's taken here, for the scan of its screen (hidden_scan.rs)
pub(crate) fn lbterm_hidden_output(
    n: &MuxNotification,
    window: MuxWindowId,
) -> Option<Arc<dyn Pane>> {
    let MuxNotification::PaneOutput(pane_id) = n else {
        return None;
    };
    let mux = Mux::get();
    let pane = mux.get_pane(*pane_id)?;
    if pane
        .downcast_ref::<wezterm_client::pane::ClientPane>()
        .is_none()
    {
        return None;
    }
    let open = mux
        .get_active_tab_for_window(window)
        .map_or(true, |tab| tab.contains_pane(*pane_id));
    if open {
        return None;
    }
    // As lbterm_budget_notif takes another tab's output: not looked at while the window is busy
    // for more than half its quiet anyway
    let half = Duration::from_millis(config::configuration().lbterm_budget.quiet_ms / 2);
    if budget::busy_left(window, Instant::now()) <= half {
        budget::news(window, false);
    }
    Some(pane)
}

impl TermWindow {
    /// The config, as the window is made and on each reload; making or reloading is busy
    pub(crate) fn lbterm_budget_configure(&self) {
        budget::configure(settings(&self.config));
        budget::activity(self.mux_window_id);
    }

    /// Something happened to the window: busy for `quiet_ms`
    pub(crate) fn lbterm_budget_activity(&self) {
        budget::activity(self.mux_window_id);
    }

    /// A window event: all but a repaint, a notification (judged as what it says, below) and
    /// the window going are something happening to it
    pub(crate) fn lbterm_budget_event(&self, event: &WindowEvent) {
        match event {
            WindowEvent::NeedRepaint | WindowEvent::Notification(_) | WindowEvent::Destroyed => {}
            _ => self.lbterm_budget_activity(),
        }
    }

    /// A notification: what the mux says of this window's open tab, its tabs and itself is
    /// something happening to it; what a background tab says (its output, or an alert its rail
    /// row may show) is news, which keeps the process from being at rest for its own tasks but
    /// leaves the window's frames judged: a window with Claude sessions working behind its open
    /// tab is still seen painting at rest. The rest (a timer's repaint, the hub's word, a
    /// watcher's) isn't either. A pane's output comes often, so while the window is busy for more
    /// than half its quiet anyway, it isn't looked at
    pub(crate) fn lbterm_budget_notif(&self, notif: &TermWindowNotif) {
        let TermWindowNotif::MuxNotification(n) = notif else {
            return;
        };
        let (pane, alert) = match n {
            MuxNotification::PaneOutput(pane) => (Some(*pane), false),
            MuxNotification::Alert { pane_id: pane, .. } => (Some(*pane), true),
            _ => (None, false),
        };
        let Some(pane) = pane else {
            self.lbterm_budget_activity();
            return;
        };
        let half = Duration::from_millis(self.config.lbterm_budget.quiet_ms / 2);
        if budget::busy_left(self.mux_window_id, Instant::now()) > half {
            return;
        }
        // Under the Conductor's view no tab shows, the open one included: its output is news
        // like any other tab's (the view's own repaints are what's judged then)
        let mux = Mux::get();
        let open = !self.lbterm_view_is_open()
            && mux
                .get_active_tab_for_window(self.mux_window_id)
                .map_or(false, |tab| tab.contains_pane(pane));
        if open {
            self.lbterm_budget_activity();
        } else if alert {
            // Finding an alert's window walks every tab: an alert, not every line of output.
            // Another window's pane's alert repaints nothing here, but is news all the same
            let here = mux
                .resolve_pane_id(pane)
                .map_or(false, |(_, window, _)| window == self.mux_window_id);
            budget::news(self.mux_window_id, here);
        } else {
            budget::news(self.mux_window_id, false);
        }
    }

    /// The window has gone
    pub(crate) fn lbterm_budget_forget(&self) {
        budget::forget(self.mux_window_id);
        crate::present::forget(self.mux_window_id);
    }

    /// `purpose` repaints this window at `due`, as often as `per_second` a second
    pub(crate) fn lbterm_budget_tick(&self, purpose: Purpose, due: Instant, per_second: f64) {
        budget::tick(self.mux_window_id, purpose, due, per_second);
    }

    /// Upstream's next frame (render/paint.rs) at `due`, on purpose: at the rate it really comes.
    /// An easing animates at `animation_fps`, but a blink (the cursor's, text's, rapid text's)
    /// with `animation_fps` 1 turns on and off at its own rate, 1.25 a second at
    /// `cursor_blink_rate` 800, so the tick says the rate its wait gives, never under
    /// `animation_fps` and never over the fastest of those rates
    pub(crate) fn lbterm_budget_animation(&self, due: Instant) {
        let fps = (self.config.animation_fps as f64).max(1.0);
        let per = |ms: u64| if ms > 0 { 1000.0 / ms as f64 } else { 0.0 };
        let fastest = fps
            .max(per(self.config.cursor_blink_rate))
            .max(per(self.config.text_blink_rate))
            .max(per(self.config.text_blink_rate_rapid));
        let rate = budget::animation_rate(due.saturating_duration_since(Instant::now()), fps, fastest);
        budget::tick(self.mux_window_id, Purpose::Animation, due, rate);
    }

    /// `action` starts now, from what started what's being done (the last key or click, else
    /// the message or task running), and `shown` will say when a frame shows it
    pub(crate) fn lbterm_budget_begin(&self, action: Action, shown: Shown) {
        let start = lbterm_extras::slow::cause(Instant::now());
        self.lbterm_budget_begin_at(action, start, shown);
    }

    pub(crate) fn lbterm_budget_begin_at(&self, action: Action, start: Instant, shown: Shown) {
        if let Some(ticket) = budget::begin(self.mux_window_id, action, start) {
            self.lbterm_budget.borrow_mut().pending.push((ticket, shown));
        }
    }

    /// A key (or a paste) went to `pane`: timed to the frame after its echo, one at a time
    pub(crate) fn lbterm_budget_key(&self, pane: &Arc<dyn Pane>) {
        let id = pane.pane_id();
        let waiting = self
            .lbterm_budget
            .borrow()
            .pending
            .iter()
            .any(|(t, s)| matches!(s, Shown::Output { pane, .. } if *pane == id) && budget::waiting(*t));
        if waiting {
            // Taken all the same: what comes next isn't this key's
            lbterm_extras::slow::cause(Instant::now());
        } else {
            let seqno = pane.get_current_seqno();
            self.lbterm_budget_begin(Action::Key, Shown::Output { pane: id, seqno });
        }
    }

    /// A tab switch, when it switches: timed to the frame with the tab's panes
    pub(crate) fn lbterm_budget_tab(&self, switches: bool) {
        if switches {
            self.lbterm_budget_begin(Action::Tab, Shown::TabPanes);
        }
    }

    /// A click on the InMail button: when it opens the list, timed to the frame with it
    pub(crate) fn lbterm_budget_inbox(&self) {
        if !self.lbterm_inbox_list_is_open() {
            self.lbterm_budget_begin(Action::Inbox, Shown::InboxList);
        }
    }

    /// A click on the agenda's button: when it opens the list, timed to the frame with it
    pub(crate) fn lbterm_budget_agenda(&self) {
        if !self.lbterm_agenda_list_is_open() {
            self.lbterm_budget_begin(Action::Agenda, Shown::AgendaList);
        }
    }

    /// The new-tab picker opened (render/new_tab.rs): timed to the next frame, which draws it
    pub(crate) fn lbterm_budget_picker(&self) {
        self.lbterm_budget_begin(Action::Picker, Shown::Next);
    }

    /// A pick in the picker asked for a tab: timed to the next frame, which draws its Starting row
    pub(crate) fn lbterm_budget_pick(&self) {
        self.lbterm_budget_begin(Action::Pick, Shown::Next);
    }

    /// A tab is about to be opened: timed to the frame with it
    pub(crate) fn lbterm_budget_new_tab(&self) {
        let tabs = Mux::get().get_window(self.mux_window_id).map_or(0, |w| w.count_tabs());
        self.lbterm_budget_begin(Action::NewTab, Shown::Tabs(tabs));
    }

    /// The active tab is about to be split: timed to the frame with the new pane
    pub(crate) fn lbterm_budget_split(&self) {
        if let Some(tab) = Mux::get().get_active_tab_for_window(self.mux_window_id) {
            let from = tab.count_panes().unwrap_or(0);
            self.lbterm_budget_begin(Action::Split, Shown::Panes { tab: tab.tab_id(), from });
        }
    }

    /// A config reload has been applied: timed from when it started reading the file, once
    pub(crate) fn lbterm_budget_reload(&self) {
        let Some((n, started)) = config::lbterm::reload_started() else {
            return;
        };
        let mut state = self.lbterm_budget.borrow_mut();
        if n == state.reload || started.elapsed() > RELOAD_AT_MOST {
            return;
        }
        state.reload = n;
        drop(state);
        self.lbterm_budget_begin_at(Action::Reload, started, Shown::Next);
    }

    /// Until when what's on screen moves or waits by itself: DWM's animations, the hearts, and
    /// the waits the state file lists. Asked only of a frame that would otherwise be at rest
    fn lbterm_in_flight(&self, now: Instant) -> Option<Instant> {
        let mut until: Option<Instant> = None;
        let mut upto = |t: Instant| until = Some(until.map_or(t, |u| u.max(t)));
        if let Some(settles) = self
            .webgpu
            .as_ref()
            .and_then(|w| w.compositor.as_ref())
            .and_then(|c| c.settles())
        {
            upto(settles);
        }
        if let (_, Some((_, end))) = self.lbterm_state_hearts(now) {
            upto(end);
        }
        for wait in self.lbterm_waits(now) {
            upto(now + Duration::from_millis(wait.left_ms));
        }
        until
    }

    /// A frame starts (paint_impl): it's handed to the budget with who asked for it, and its
    /// own paint is timed
    pub(crate) fn lbterm_budget_frame_start(&mut self) {
        self.lbterm_state_frame_start();
        let now = Instant::now();
        if lbterm_extras::present::on() {
            crate::present::frame_start(self.mux_window_id, now);
        } else {
            crate::present::off();
        }
        let reasons = lbterm_extras::counters::frame(self.lbterm_state_hwnd() as usize, self.mux_window_id);
        let window = self.mux_window_id;
        if !budget::busy(window, now) {
            budget::frame(window, now, &reasons, || self.lbterm_in_flight(now));
        }
        let state = self.lbterm_budget.get_mut();
        state.frames += 1;
        state.paint = if state.frames > FIRST_FRAMES {
            budget::begin(window, Action::Paint, now)
        } else {
            None
        };
    }

    /// The frame has ended: its paint is done, and what it shows of what was waiting. With the
    /// timing to the screen on, each is then watched to the screen (present.rs) with the motion
    /// the frame plays; off, nothing more is done or kept
    pub(crate) fn lbterm_budget_frame_end(&mut self) {
        let now = Instant::now();
        let present = lbterm_extras::present::on();
        let mut shown = vec![];
        if let Some(ticket) = self.lbterm_budget.get_mut().paint.take() {
            if let Some(done) = budget::done(ticket, now) {
                if present {
                    shown.push((done, None));
                }
            }
        }
        if !self.lbterm_budget.get_mut().pending.is_empty() {
            let pending = std::mem::take(&mut self.lbterm_budget.get_mut().pending);
            let mut left = vec![];
            for (ticket, waits) in pending {
                if !budget::waiting(ticket) {
                    continue;
                }
                if self.lbterm_budget_shows(waits) {
                    let pane = match waits {
                        Shown::Output { pane, .. } => Some(pane),
                        _ => None,
                    };
                    if let Some(done) = budget::done(ticket, now) {
                        if present {
                            shown.push((done, pane));
                        }
                    }
                } else {
                    left.push((ticket, waits));
                }
            }
            let state = self.lbterm_budget.get_mut();
            left.append(&mut state.pending);
            state.pending = left;
        }
        if present {
            let playing = self
                .webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or(vec![], |c| c.playing(now));
            crate::present::frame_end(self.mux_window_id, self.lbterm_state_hwnd(), &shown, playing, now);
        }
    }

    /// Whether a key waits on its echo: then each frame notes what it drew of each pane
    /// (`lbterm_state_drawn`)
    pub(crate) fn lbterm_budget_waits_on_output(&self) -> bool {
        self.lbterm_budget
            .borrow()
            .pending
            .iter()
            .any(|(_, s)| matches!(s, Shown::Output { .. }))
    }

    /// Whether this frame shows what `shown` waits for
    fn lbterm_budget_shows(&self, shown: Shown) -> bool {
        let mux = Mux::get();
        match shown {
            Shown::Next => true,
            Shown::TabPanes => !self.lbterm_panes_kept(),
            Shown::Tabs(from) => mux.get_window(self.mux_window_id).map_or(false, |w| w.count_tabs() > from),
            Shown::Panes { tab, from } => mux
                .get_tab(tab)
                .map_or(false, |t| t.count_panes().unwrap_or(0) > from),
            Shown::TabGone(tab) => mux
                .get_window(self.mux_window_id)
                .map_or(true, |w| w.get_tab_idx_for_id(tab).is_none()),
            Shown::Output { pane, seqno } => match self.lbterm_state_drawn_seqno(pane) {
                Some(drawn) => drawn > seqno,
                // A pane this frame didn't draw (out of sight): as it is now
                None => mux
                    .get_pane(pane)
                    .map_or(false, |p| p.get_current_seqno() > seqno),
            },
            Shown::InboxList => self.lbterm_inbox_list_is_open(),
            Shown::AgendaList => self.lbterm_agenda_list_is_open(),
            Shown::View => self.lbterm_view_is_open(),
        }
    }
}
