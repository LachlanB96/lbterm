//! LBTerm: a window's title updates, as many as what they show needs (lbterm_extras::titles has
//! why, and the parts tested without a window). perf/title-storm.ps1 holds it.
//!
//! - A pane's title, folder, unseen-output and progress alerts update the title of the window
//!   holding the pane only, and every one that comes before the update runs shares it. A
//!   background tab's title, folder, unseen output or user var updates nothing unless something
//!   reads every tab (lbterm_extras::titles::wants_update): the window's title is the open tab's.
//!   With an update-status or update-right-status handler, which can show other windows' tabs,
//!   every window's alerts update every window, as upstream's did.
//! - A pane's title repaints the window when its tab's rail row would read otherwise
//!   (render/tab_panel.rs, lbterm_panel_title_changed): nothing else paints for a background tab,
//!   whose output repaints nothing, and Claude Code's ◐ and ◑ change no row.
//! - A pane's title alert that would do none of that is dropped in the window's mux subscription,
//!   before any task (TitleWatch, lbterm_title_alert_dropped; lbterm_extras::titles::
//!   title_alert_goes): the subscription reads the handlers' answers, the taskbar's retry and the
//!   rail's rows as the last paint drew them from what the GUI thread leaves where any thread can
//!   read it. A working Claude Code's ◐ and ◑ in a tab out of view took the alert's hop and the
//!   window's event task to decide nothing, about 50 us of the GUI thread each (lbterm/FLOWS.md,
//!   rows t12 to t14).
//! - A taskbar button that couldn't be set (Explorer hadn't made it) is tried again on the next
//!   alert a second on, as every title alert's update did before (taskbar.rs, retry_due).
//! - The mux window's own title (on a mux domain, the server's, which follows whichever pane set
//!   a title last) updates nothing unless a Lua handler could read it: the window shows the open
//!   pane's title. Without one, the window's mux subscription drops it before its task and its
//!   hop to the window, about 20 us of the GUI thread each (lbterm_titles_read_update; with eight
//!   tabs setting their titles again, which turns the server's window title each time, a title
//!   went from 50 to 55 us to 24 to 35, the rest the server's message reaching the GUI thread).
//! - A Lua event with no handler for it isn't scheduled, and update-status isn't asked for with
//!   no handler for it or update-right-status: each was a task, a GuiWin and Lua userdata, five
//!   tasks an update.
//! - The taskbar's progress is worked out again only for what can change it.
//! - Every tab's and pane's information is made only for what reads it.

use crate::termwindow::{TermWindow, TermWindowNotif};
use config::ConfigHandle;
use lbterm_extras::titles::{self, Changed, Place, Readers};
use mux::pane::PaneId;
use mux::tab::TabId;
use mux::window::WindowId as MuxWindowId;
use mux::{Mux, MuxNotification};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wezterm_term::Alert;
use window::WindowOps;

/// The rail's rows as a paint drew them, by tab (render/tab_panel.rs): shared between the window
/// and its [`TitleWatch`], each paint's map made once and never changed after
pub(crate) type DrawnRows = Arc<HashMap<TabId, lbterm_panel::claude::Tab>>;

/// What a window's mux subscription reads to decide a pane's title alert before any task
/// ([`lbterm_title_alert_dropped`]), left by the GUI thread where the subscription's thread can
/// read it: the handlers' answers, asked at each config load (lbterm_titles_read_update), and the
/// rail's rows with the idle timer's length, left by each paint as it draws them, never after. The
/// taskbar's retry is the process's (taskbar.rs, retry_due). Until the config is first asked,
/// every title reaches the window
pub(crate) struct TitleWatch {
    /// A Lua handler a title update runs could read the mux window's own title: a status or
    /// format-window-title handler, or format-tab-title while the tab bar shows the tabs
    window_title_read: AtomicBool,
    /// An update-status or update-right-status handler (status_handlers)
    status_handlers: AtomicBool,
    /// A title update reads every tab: the tab bar shows the tabs' words, or a
    /// format-window-title handler (lbterm_extras::titles::needs_every_tab)
    every_tab: AtomicBool,
    /// The rows the rail last drew, and the seconds its idle timer fills over as it drew them
    drawn: Mutex<(DrawnRows, f64)>,
}

impl Default for TitleWatch {
    fn default() -> Self {
        Self {
            window_title_read: AtomicBool::new(true),
            status_handlers: AtomicBool::new(true),
            every_tab: AtomicBool::new(true),
            drawn: Mutex::new((DrawnRows::default(), 0.0)),
        }
    }
}

impl TitleWatch {
    /// Whether a Lua handler could read the mux window's own title
    pub(crate) fn window_title_read(&self) -> bool {
        self.window_title_read.load(Ordering::Relaxed)
    }

    /// Whether an update-status or update-right-status handler runs with title updates
    pub(crate) fn status_handlers(&self) -> bool {
        self.status_handlers.load(Ordering::Relaxed)
    }

    /// Whether a title update reads every tab
    pub(crate) fn every_tab(&self) -> bool {
        self.every_tab.load(Ordering::Relaxed)
    }

    /// The handlers' and the taskbar's part in a title alert's decision for `window`
    fn readers(&self, window: MuxWindowId) -> Readers {
        Readers {
            retry_due: super::taskbar::retry_due(window),
            status_handlers: self.status_handlers(),
            every_tab: self.every_tab(),
        }
    }

    /// The rail drew `rows`, its idle timer filling over `idle_full` seconds (paint_tab_panel),
    /// or none (the rail off)
    pub(crate) fn drew(&self, rows: DrawnRows, idle_full: f64) {
        *self.drawn.lock().unwrap() = (rows, idle_full);
    }

    /// The rows the rail last drew, and its idle timer's length then
    fn drawn(&self) -> (DrawnRows, f64) {
        let drawn = self.drawn.lock().unwrap();
        (Arc::clone(&drawn.0), drawn.1)
    }
}

/// LBTerm: whether `n` is a pane's title alert (a title, icon title or tab title) that has nothing
/// to do in `window`, which the window's mux subscription then drops, with no task and no hop to
/// the window: all the window did with one was find nothing to do (lbterm_title_alert), two tasks
/// on the GUI thread for each ◐ and ◑ of every working Claude Code out of view. Decided as
/// lbterm_title_alert decides it (lbterm_extras::titles::title_alert_goes), from the mux, the
/// pane's titles now and `watch`. A folder, unseen output, progress or user var, and a title that
/// changes a rail row, the open tab's title, one a handler reads or one the taskbar's retry wants,
/// go to the window as before. Reads nothing that's the GUI thread's own, so it can run on any
/// thread a notification is sent from: the client thread asks it of every window before it hands
/// an alert over at all ([`TitleDecider`])
pub(crate) fn lbterm_title_alert_dropped(
    n: &MuxNotification,
    window: MuxWindowId,
    watch: &TitleWatch,
) -> bool {
    let dropped = drops(n, window, watch);
    if dropped {
        log::debug!(
            target: "lbterm::title",
            "title {}window {window}: nothing shown changed, dropped in the subscription",
            match n {
                MuxNotification::Alert { pane_id, .. } => format!("pane {pane_id} "),
                _ => String::new(),
            }
        );
    }
    dropped
}

/// lbterm_title_alert_dropped's decision, with no line said
fn drops(n: &MuxNotification, window: MuxWindowId, watch: &TitleWatch) -> bool {
    let MuxNotification::Alert {
        pane_id,
        alert: Alert::WindowTitleChanged(_) | Alert::IconTitleChanged(_) | Alert::TabTitleChanged(_),
    } = n
    else {
        return false;
    };
    let (tab, open) = {
        let mux = Mux::get();
        // A window the mux doesn't have (a workspace switch under way): to the window, whose
        // callback skips it as before
        let Some(mux_window) = mux.get_window(window) else {
            return false;
        };
        let tab = mux_window
            .iter_tabs()
            .find(|tab| tab.contains_pane(*pane_id))
            .cloned();
        let open = tab.as_ref().map_or(false, |tab| {
            mux_window
                .get_active_tab()
                .map_or(false, |active| active.tab_id() == tab.tab_id())
        });
        (tab, open)
    };
    let place = match (&tab, open) {
        (None, _) => Place::Elsewhere,
        (Some(_), true) => Place::OpenTab,
        (Some(_), false) => Place::Background,
    };
    let goes = titles::title_alert_goes(watch.readers(window), place, || {
        let Some(tab) = tab.as_ref() else {
            return false;
        };
        let (rows, idle_full) = watch.drawn();
        rows.get(&tab.tab_id()).map_or(false, |drawn| {
            super::render::tab_panel::titles_move_row(tab, drawn, idle_full)
        })
    });
    !goes
}

/// LBTerm: a window the client thread's title decider asks ([`TitleDecider`]): its mux window,
/// its TitleWatch and its subscription's dead flag, the three its mux subscription captures
struct Watched {
    mux_window_id: Arc<Mutex<MuxWindowId>>,
    watch: Arc<TitleWatch>,
    dead: Arc<AtomicBool>,
}

static WATCHED: Mutex<Vec<Watched>> = Mutex::new(Vec::new());

/// LBTerm: the window whose mux subscription captures these, asked by the client thread's title
/// decider until its subscription is dead; the first installs the decider (subscribe_to_pane_updates)
pub(crate) fn lbterm_watch_window(
    mux_window_id: &Arc<Mutex<MuxWindowId>>,
    watch: &Arc<TitleWatch>,
    dead: &Arc<AtomicBool>,
) {
    let mut watched = WATCHED.lock().unwrap();
    watched.retain(|w| !w.dead.load(Ordering::Relaxed));
    watched.push(Watched {
        mux_window_id: Arc::clone(mux_window_id),
        watch: Arc::clone(watch),
        dead: Arc::clone(dead),
    });
    wezterm_client::lbterm_titles::set_decider(Box::new(TitleDecider));
}

/// LBTerm: what the client thread asks of a title before it hands it to the GUI thread
/// (wezterm-client's lbterm_titles.rs; lbterm/FLOWS.md, "A title out of view", What could go 13):
/// a title alert is dropped there when every window's subscription would drop it and no
/// connection to the GUI's own socket is open to hear it, and a mux window's title is kept there
/// while nothing reads one as it changes. Each window's part is read from its TitleWatch and the
/// mux, as its subscription reads them; WATCHED is held while they're asked, a lock nothing on
/// the GUI thread takes while it holds any other (lbterm/PITFALLS.md, 321)
struct TitleDecider;

impl wezterm_client::lbterm_titles::Decider for TitleDecider {
    fn alert_dropped(&self, n: &MuxNotification) -> bool {
        if wezterm_mux_server_impl::dispatch::lbterm_connections() > 0 {
            return false;
        }
        let watched = WATCHED.lock().unwrap();
        let mut asked = false;
        for w in watched.iter().filter(|w| !w.dead.load(Ordering::Relaxed)) {
            let window = *w.mux_window_id.lock().unwrap();
            if !drops(n, window, &w.watch) {
                return false;
            }
            asked = true;
        }
        asked
    }

    fn window_titles_read(&self) -> bool {
        if wezterm_mux_server_impl::dispatch::lbterm_connections() > 0 {
            return true;
        }
        let watched = WATCHED.lock().unwrap();
        let mut live = watched.iter().filter(|w| !w.dead.load(Ordering::Relaxed));
        let mut any = false;
        for w in &mut live {
            if w.watch.window_title_read() {
                return true;
            }
            any = true;
        }
        // No window yet: as before
        !any
    }
}

/// A title update's pass, timed for perf/title-storm.ps1 while WEZTERM_LOG has lbterm::title at
/// debug: its line says how long it took when it ends
pub(super) struct Pass(Option<(std::time::Instant, mux::window::WindowId)>);

impl Pass {
    pub(super) fn start(window: mux::window::WindowId) -> Pass {
        Pass(
            log::log_enabled!(target: "lbterm::title", log::Level::Debug)
                .then(|| (std::time::Instant::now(), window)),
        )
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        if let Some((started, window)) = self.0 {
            log::debug!(target: "lbterm::title", "update_title window {window}: {} us", started.elapsed().as_micros());
        }
    }
}

/// Whether the config has a handler for Lua event `name` (wezterm.on)
pub(super) fn has_handler(name: &str) -> bool {
    matches!(
        config::run_immediate_with_lua_config(|lua| Ok(lua.map_or(false, |lua| {
            config::lua::has_event_handler(&lua, name)
        }))),
        Ok(true)
    )
}

/// Whether update-status or update-right-status has a handler
pub(super) fn status_handlers() -> bool {
    has_handler("update-status") || has_handler("update-right-status")
}

/// Whether the tab bar shows the tabs' words (the tab panel hides them: lbterm/TAB-PANEL.md)
pub(super) fn tab_bar_shows_tabs(config: &ConfigHandle) -> bool {
    config.show_tabs_in_tab_bar && !config::lbterm::hides_tabs(config)
}

/// What the tab bar was last made of, while it doesn't show the tabs: TabBarState::new reads
/// nothing else, so the same again makes the same bar (its colours cloned, its new-tab button,
/// statuses and window buttons each parsed as escapes, every title update)
pub(super) struct TabBarKey {
    /// The config it was made with, held so its address can't be another's
    config: ConfigHandle,
    width: usize,
    mouse_x: Option<usize>,
    left: String,
    right: String,
}

impl TermWindow {
    /// Whether the tab bar has to be made again: it shows the tabs' words, or what it's made of
    /// changed since it was last made (TabBarKey)
    pub(super) fn lbterm_tab_bar_changed(&mut self, width: usize, mouse_x: Option<usize>) -> bool {
        if tab_bar_shows_tabs(&self.config) {
            self.lbterm_tab_bar_key = None;
            return true;
        }
        let same = self.lbterm_tab_bar_key.as_ref().map_or(false, |key| {
            std::ptr::eq::<config::Config>(&*key.config, &*self.config)
                && key.width == width
                && key.mouse_x == mouse_x
                && key.left == self.left_status
                && key.right == self.right_status
        });
        if !same {
            self.lbterm_tab_bar_key = Some(TabBarKey {
                config: self.config.clone(),
                width,
                mouse_x,
                left: self.left_status.clone(),
                right: self.right_status.clone(),
            });
        }
        !same
    }

    /// Whether one of this window's tabs holds `pane_id`
    pub(super) fn lbterm_holds_pane(&self, pane_id: PaneId) -> bool {
        Mux::get()
            .get_window(self.mux_window_id)
            .map_or(false, |window| window.iter_tabs().any(|tab| tab.contains_pane(pane_id)))
    }

    /// A pane's title, icon or tab title, folder, unseen output or progress changed
    pub(super) fn lbterm_title_alert(&mut self, pane_id: PaneId, alert: &Alert) {
        let alert = match alert {
            Alert::CurrentWorkingDirectoryChanged => titles::Alert::Folder,
            Alert::OutputSinceFocusLost => titles::Alert::UnseenOutput,
            Alert::Progress(_) => titles::Alert::Progress,
            _ => titles::Alert::Title,
        };
        // A taskbar button Explorer hadn't made when this window's progress was last set: any
        // alert tries it again, as each one's update did before (taskbar.rs)
        if super::taskbar::retry_due(self.mux_window_id) {
            self.lbterm_update_title_soon(Changed::PROGRESS);
        }
        if !self.lbterm_holds_pane(pane_id) {
            // Another window's pane changes nothing this window shows, but an update-status or
            // update-right-status handler can show other windows' tabs (their lights, counts):
            // upstream ran them in every window for every pane's alert, and with one they still
            // run, every alert before the update sharing it
            if self.lbterm_title_watch.status_handlers() {
                self.lbterm_update_title_soon(Changed::TITLE);
            }
            return;
        }
        // Its tab's rail row, which nothing else repaints for a background tab
        // (render/tab_panel.rs)
        if alert == titles::Alert::Title {
            self.lbterm_panel_title_changed(pane_id);
        }
        self.lbterm_pane_changed(pane_id, alert);
    }

    /// An alert of this window's pane `pane_id`: an update, unless nothing it shows can change
    pub(super) fn lbterm_pane_changed(&mut self, pane_id: PaneId, alert: titles::Alert) {
        let in_open_tab = Mux::get()
            .get_active_tab_for_window(self.mux_window_id)
            .map_or(false, |tab| tab.contains_pane(pane_id));
        // The handlers' answers as the last config load asked them (lbterm_titles_read_update),
        // not three Lua lookups an alert
        let watch = &self.lbterm_title_watch;
        if titles::wants_update(alert, in_open_tab, watch.every_tab(), watch.status_handlers()) {
            self.lbterm_update_title_soon(alert.changed());
        }
    }

    /// The mux window's own title changed
    pub(super) fn lbterm_window_title_changed(&mut self) {
        if self.lbterm_title_watch.window_title_read() {
            self.lbterm_update_title_soon(Changed::TITLE);
        }
    }

    /// What the window's handlers read, for its title alerts and for its mux subscription
    /// (TitleWatch), which drops the mux window's title changes before they reach the window when
    /// none can read them (subscribe_to_pane_updates) and a pane's title alert that changes
    /// nothing here (lbterm_title_alert_dropped). Asked as the window starts and at each config
    /// load or override, when wezterm.on registers the handlers: three to five Lua lookups a
    /// load, where lbterm_pane_changed made three every alert
    pub(super) fn lbterm_titles_read_update(&self) {
        let watch = &self.lbterm_title_watch;
        let status = status_handlers();
        let format_window_title = has_handler("format-window-title");
        let shows_tabs = tab_bar_shows_tabs(&self.config);
        watch.status_handlers.store(status, Ordering::Relaxed);
        watch.every_tab.store(
            titles::needs_every_tab(shows_tabs, format_window_title),
            Ordering::Relaxed,
        );
        // lua_reads_titles, without asking its handlers again
        let reads = status
            || format_window_title
            || (shows_tabs && has_handler("format-tab-title"));
        watch.window_title_read.store(reads, Ordering::Relaxed);
    }

    /// update_title, once for everything that asks before it runs, doing what they changed
    pub(super) fn lbterm_update_title_soon(&mut self, changed: Changed) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if self.lbterm_titles.ask(changed) {
            window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                if let Some(changed) = tw.lbterm_titles.take() {
                    tw.schedule_status_update();
                    tw.lbterm_update_title_impl(changed.progress);
                }
            })));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use wezterm_client::lbterm_titles::Decider;

    // No window has subscribed in the test binary: the client thread decides nothing, and every
    // title goes to the GUI thread as before
    #[test]
    fn with_no_window_the_decider_drops_no_alert_and_keeps_no_window_title() {
        let alert = MuxNotification::Alert {
            pane_id: 1,
            alert: Alert::WindowTitleChanged("◑ work".into()),
        };
        assert!(!TitleDecider.alert_dropped(&alert));
        assert!(TitleDecider.window_titles_read());
    }
}
