//! LBTerm: crash restore (lbterm_extras::restore). LBTerm keeps a note of its tabs in
//! `session-<window class>.json` in its data folder, rewritten a couple of seconds after they
//! change. Closing LBTerm normally marks the note clean. When LBTerm starts and finds a note
//! that isn't clean (a crash, the process killed, Windows restarting), a window asks which of
//! those tabs to open again once LBTerm's start-up is done (its mux domain attached). LBTerm
//! notes its own tabs from the start all the same, with the offered ones beside them as
//! `pending` until that's answered, so closing LBTerm without answering asks again next time,
//! and a crash before the answer loses neither (until 2026-10-30 it wrote nothing until the
//! answer, and a prompt left unanswered, or orphaned as on 2026-10-30 at 11:09, left a whole run
//! unnoted). A prompt that goes without an answer (its tab gone, its window moved to another
//! mux window, another overlay opened over it, the answer never getting to its window) is asked
//! again. It never goes over another overlay: it waits for that to close.

use crate::termwindow::{TermWindow, TermWindowNotif};
use lbterm_extras::restore::{
    self, Asking, Handed, NoTab, Outcome, Session, TabKind, TabNote, WindowNote,
};
use mux::pane::{CachePolicy, Pane, PaneId};
use mux::tab::{Tab, TabId};
use mux::termwiztermtab::TermWizTerminalPane;
use mux::window::WindowId;
use mux::{Mux, MuxNotification};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wezterm_client::domain::ClientDomain;
use wezterm_term::Alert;
use window::WindowOps;

/// A burst of changes is written once, this long after the first
const SAVE_AFTER: Duration = Duration::from_secs(2);
/// Panes found running in a mux server get their user vars (claude_session) a moment after
/// they're listed, so the offer waits this long after the attach when there are any
const USER_VARS_AFTER: Duration = Duration::from_secs(2);
/// PowerShell 7, by full path (the Store alias can't be started by path)
const PWSH: &str = r"C:\Program Files\PowerShell\7\pwsh.exe";

/// The prompt that's up: where, so it's taken down to be asked again if its tab goes
struct Prompt {
    window: ::window::Window,
    mux_window: WindowId,
    tab: TabId,
    overlay: PaneId,
}

#[derive(Default)]
struct State {
    /// The note found at start-up, until it's answered
    asking: Asking,
    /// The tabs offered and not answered yet, kept in our own note so they're offered again
    pending: Vec<TabNote>,
    prompt: Option<Prompt>,
    save_due: bool,
    pid: u32,
    started: u64,
    /// The last note written, so an unchanged one isn't written again
    last: Option<Session>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn path() -> PathBuf {
    let class: String = crate::termwindow::get_window_class()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    config::DATA_DIR.join(format!("session-{class}.json"))
}

/// When a process started (its creation FILETIME); `None` when it isn't running. An ended process
/// still opens by its pid, with its times, while anything holds a handle to it (a process
/// monitor, a test's harness), so it's running only while it has no exit code: taken for a
/// running LBTerm, a dead one's note was never offered, and the new LBTerm wrote over it
#[cfg(windows)]
fn start_time(pid: u32) -> Option<u64> {
    use windows58::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows58::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let mut code = 0u32;
        let times = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        let alive =
            GetExitCodeProcess(process, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(process);
        times.ok()?;
        if !alive {
            return None;
        }
        Some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }
}

#[cfg(not(windows))]
fn start_time(_: u32) -> Option<u64> {
    None
}

fn read(path: &PathBuf) -> Option<Session> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(s) => Some(s),
        Err(err) => {
            log::warn!("restore: {} isn't a session note: {err:#}", path.display());
            None
        }
    }
}

fn write(path: PathBuf, note: Session) {
    let Ok(text) = serde_json::to_string_pretty(&note) else {
        return;
    };
    // Written beside and renamed over, so a crash mid-write never leaves half a note
    let tmp = path.with_extension("json.tmp");
    let result = std::fs::create_dir_all(path.parent().unwrap_or(&path))
        .and_then(|()| std::fs::write(&tmp, text))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(err) = result {
        log::warn!("restore: writing {}: {err:#}", path.display());
    }
}

/// At start-up, before any window: read what the last LBTerm left, and start listening for
/// changes to the tabs
pub fn startup() {
    let pid = std::process::id();
    let started = start_time(pid).unwrap_or(0);
    let previous = read(&path());
    let offer = previous.filter(|p| {
        restore::offer(Some(p), |old_pid, old_started| {
            old_pid != pid && old_started != 0 && start_time(old_pid) == Some(old_started)
        })
    });
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pid = pid;
        s.started = started;
        s.pending = offer.as_ref().map(restore::to_offer).unwrap_or_default();
        s.asking = Asking::new(offer);
    });

    Mux::get().subscribe(|n| {
        let tabs_changed = match &n {
            MuxNotification::WindowCreated(_)
            | MuxNotification::WindowRemoved(_)
            | MuxNotification::TabAddedToWindow { .. }
            | MuxNotification::PaneRemoved(_)
            | MuxNotification::TabTitleChanged { .. }
            // A park is noted, so a tab reopened from the note comes back parked
            | MuxNotification::LbtermTabParked { .. } => true,
            MuxNotification::Alert {
                alert: Alert::SetUserVar { name, .. },
                ..
            } => name == "claude_session",
            MuxNotification::Alert {
                alert: Alert::CurrentWorkingDirectoryChanged,
                ..
            } => true,
            _ => false,
        };
        // A tab came or went: the prompt's tab may have gone, or a note waiting for a tab to ask
        // over may have one
        let moved = matches!(
            n,
            MuxNotification::WindowRemoved(_)
                | MuxNotification::TabAddedToWindow { .. }
                | MuxNotification::PaneRemoved(_)
        );
        if tabs_changed || moved {
            promise::spawn::spawn_into_main_thread(async move {
                if tabs_changed {
                    changed();
                }
                if moved {
                    tabs_moved();
                }
            })
            .detach();
        }
        true
    });
}

/// LBTerm's start-up is done (main.rs, async_run_terminal_gui): the domain has attached, so the
/// windows have their final tabs. Until now a mux domain's window held only its connection
/// status, a tab of its own that goes when the domain attaches, and on 2026-10-30 the prompt went
/// up over it and went with it. The note is offered from now on, after USER_VARS_AFTER when the
/// GUI found panes running in a mux server
pub fn start_up_done() {
    if STATE.with(|s| s.borrow().asking.answered()) {
        return;
    }
    if !found_running() {
        offer_from_now();
        return;
    }
    promise::spawn::spawn(async {
        // LBTerm: counted (lbterm_extras::counters)
        lbterm_extras::counters::timer(smol::Timer::after(USER_VARS_AFTER)).await;
        offer_from_now();
    })
    .detach();
}

fn offer_from_now() {
    STATE.with(|s| s.borrow_mut().asking.start());
    offer_soon();
}

/// Offer the note in the first window with a tab of its own (restore::window_to_ask), when it's
/// waiting to be (restore::Asking::waiting): not a window of passing tabs alone, a configuration
/// error's, which may sort first. A window that's still being made offers it itself once it's
/// there (termwindow/mod.rs)
fn offer_soon() {
    if !STATE.with(|s| s.borrow().asking.waiting()) {
        return;
    }
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    let windows = fe.gui_windows();
    let kinds: Vec<Vec<TabKind>> = windows.iter().map(|w| tab_kinds(w.mux_window_id)).collect();
    if let Some(i) = restore::window_to_ask(&kinds) {
        windows[i].window.notify(TermWindowNotif::Apply(Box::new(|tw| {
            tw.lbterm_offer_restore()
        })));
    }
}

/// A tab as the mux has it: one of the window's own, or one of the GUI's passing ones. Whether
/// something is over it is the GUI window's to say (lbterm_prompt_tab)
fn kind_of(tab: &Arc<Tab>) -> TabKind {
    match tab.get_active_pane() {
        Some(pane) if !is_passing(&pane) => TabKind::Free,
        _ => TabKind::Passing,
    }
}

/// A mux window's tabs, in order, as the mux has them (kind_of)
fn tab_kinds(mux_window: WindowId) -> Vec<TabKind> {
    let mux = Mux::get();
    let Some(window) = mux.get_window(mux_window) else {
        return vec![];
    };
    window.iter_tabs().map(kind_of).collect()
}

/// A tab or a pane came or went. A prompt whose tab has left its window is taken down (its
/// thread then gives the note back, to be asked again); a note waiting for a tab to ask over is
/// offered, as when another overlay that kept it waiting closes (its pane leaves the mux)
fn tabs_moved() {
    let gone = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let left = s.prompt.as_ref().map_or(false, |p| {
            Mux::get().window_containing_tab(p.tab) != Some(p.mux_window)
        });
        if left {
            s.prompt.take()
        } else {
            None
        }
    });
    if let Some(p) = gone {
        log::info!("restore: the prompt's tab {} left its window: asking again", p.tab);
        TermWindow::schedule_cancel_overlay(p.window, p.tab, Some(p.overlay));
    }
    offer_soon();
}

/// A window takes the note found at start-up, to offer it, once it's time (restore::Asking)
fn take_offer() -> Option<Session> {
    STATE.with(|s| s.borrow_mut().asking.take())
}

/// The note goes back to wait for the next chance: its prompt went without an answer, or there
/// was no tab to ask over
fn give_back(session: Session) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.asking.give_back(session);
        s.prompt = None;
    });
}

/// The prompt's input ended without an answer (its overlay was taken away), or the answer never
/// got to its window: the note stands, and is asked again in the first window with a tab of its
/// own, on its mux window as it is now
fn ask_again(session: Session, why: &str) {
    log::info!("restore: {why}: asking again");
    give_back(session);
    offer_soon();
}

/// The offer was answered (or there was nothing left in it to offer): the note stops carrying it
pub fn answered() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.asking.answer();
        s.pending.clear();
        s.prompt = None;
    });
    changed();
}

/// What's left to offer once the tabs still open in a mux server are left out: the note carries
/// those alone
fn still_pending(tabs: &[TabNote]) {
    STATE.with(|s| s.borrow_mut().pending = tabs.to_vec());
    changed();
}

/// Something about the tabs changed: write the note soon
pub fn changed() {
    let due = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.save_due {
            return false;
        }
        s.save_due = true;
        true
    });
    if due {
        promise::spawn::spawn(async {
            lbterm_extras::counters::timer(smol::Timer::after(SAVE_AFTER)).await;
            STATE.with(|s| s.borrow_mut().save_due = false);
            save(false, false);
        })
        .detach();
    }
}

/// LBTerm is closing normally: mark the note clean, so nothing is offered next time. Unless
/// panes outlive it in a mux server: then they're still running, and if the server dies before
/// the next GUI (a restart), the note should still offer them. Tabs still pending are offered
/// next time either way.
pub fn clean_exit() {
    save(server_panes().is_empty(), true);
}

/// Panes in a mux server (a client domain's). They outlive a GUI, so after a GUI crash they're
/// still running and the next GUI reattaches to them. Not the GUI's own passing panes in no
/// domain of a server (a domain's connection status, an overlay), which went with the last GUI.
fn server_panes() -> Vec<Arc<dyn Pane>> {
    let mux = Mux::get();
    mux.iter_panes()
        .into_iter()
        .filter(|p| {
            mux.get_domain(p.domain_id())
                .map_or(false, |d| d.downcast_ref::<ClientDomain>().is_some())
        })
        .collect()
}

/// One of the GUI's own passing panes, made in a tab of its own for the moment (a mux domain's
/// connection status while it attaches), not a tab to note or ask over
fn is_passing(pane: &Arc<dyn Pane>) -> bool {
    pane.downcast_ref::<TermWizTerminalPane>().is_some()
}

/// Whether this GUI found panes already running in a mux server: one it didn't start itself,
/// so they may be noted tabs that outlived the last GUI
fn found_running() -> bool {
    !wezterm_client::client::lbterm_started_server() && !server_panes().is_empty()
}

/// The live panes that may stand for noted tabs (restore::survivors): a server's the GUI
/// attached to, or of one it started itself, those with a Claude session
fn live_panes() -> Vec<restore::LivePane> {
    let panes = server_panes()
        .iter()
        .map(|p| restore::LivePane {
            cwd: p
                .get_current_working_dir(CachePolicy::AllowStale)
                .and_then(|url| url.to_file_path().ok())
                .map(|p| p.display().to_string()),
            claude_session: p
                .copy_user_vars()
                .get("claude_session")
                .filter(|id| restore::is_session_id(id))
                .cloned(),
            // survivors() says whether it outlived the last GUI
            survived: false,
        })
        .collect();
    restore::survivors(wezterm_client::client::lbterm_started_server(), panes)
}

/// Write the note if it changed; `now` on this thread (the process is about to end), else on
/// a thread of its own
fn save(clean: bool, now: bool) {
    let note = note(clean);
    let unchanged = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.last.as_ref() == Some(&note) {
            return true;
        }
        s.last = Some(note.clone());
        false
    });
    if unchanged {
        return;
    }
    if now {
        write(path(), note);
    } else {
        let path = path();
        std::thread::spawn(move || write(path, note));
    }
}

/// The note as things stand: one row per tab, from the pane running Claude Code if one is,
/// else the active pane
fn note(clean: bool) -> Session {
    let (pid, started, pending) = STATE.with(|s| {
        let s = s.borrow();
        (s.pid, s.started, s.pending.clone())
    });
    let mux = Mux::get();
    let mut windows = vec![];
    for window_id in mux.iter_windows() {
        let Some(window) = mux.get_window(window_id) else {
            continue;
        };
        let mut tabs = vec![];
        for tab in window.iter_tabs() {
            let panes = tab.iter_panes_ignoring_zoom();
            let claude = panes.iter().find_map(|p| {
                p.pane
                    .copy_user_vars()
                    .get("claude_session")
                    .filter(|id| restore::is_session_id(id))
                    .map(|id| (p.pane.clone(), id.clone()))
            });
            let (pane, claude_session) = match claude {
                Some((pane, id)) => (Some(pane), Some(id)),
                None => (tab.get_active_pane(), None),
            };
            let Some(pane) = pane else {
                continue;
            };
            // Not one of the GUI's own passing tabs: a mux domain's connection status sits in
            // the window as a tab while the domain attaches, and a note written then (2 s after
            // it came) listed it, "wezterm: Connecting..." with no folder, as a tab to open again
            if is_passing(&pane) {
                continue;
            }
            let cwd = pane
                .get_current_working_dir(CachePolicy::AllowStale)
                .and_then(|url| url.to_file_path().ok())
                .map(|p| p.display().to_string());
            let title = match tab.get_title() {
                t if !t.is_empty() => t,
                _ => pane.get_title(),
            };
            // Claude Code's spinner changes the title many times a second: keep the words
            let title = title
                .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '(' && c != '[')
                .trim()
                .to_string();
            tabs.push(TabNote {
                title,
                cwd,
                claude_session,
                parked: tab.lbterm_parked(),
            });
        }
        if !tabs.is_empty() {
            windows.push(WindowNote { tabs });
        }
    }
    Session {
        pid,
        started,
        clean,
        windows,
        pending,
    }
}

impl TermWindow {
    /// Ask in this window about the note found at start-up, when it's time: once LBTerm's
    /// start-up is done, and not while a prompt is up or once it's answered (restore::Asking).
    /// Every window calls it as it opens; start_up_done, a prompt gone without an answer and a
    /// tab or pane coming or going call it in the first window with a tab of its own
    /// (offer_soon).
    pub(crate) fn lbterm_offer_restore(&mut self) {
        let Some(session) = take_offer() else {
            return;
        };
        self.lbterm_offer_now(session);
    }

    /// Offer the noted tabs (restore::to_offer: those still pending from an earlier offer, and
    /// the note's own if it didn't end cleanly) that aren't still open in a mux server
    fn lbterm_offer_now(&mut self, session: Session) {
        let offered = restore::to_offer(&session);
        let noted: Vec<&TabNote> = offered.iter().collect();
        let live = live_panes();
        let tabs: Vec<TabNote> = restore::still_closed(&noted, &live)
            .into_iter()
            .map(|i| noted[i].clone())
            .collect();
        log::info!(
            "restore: {} noted tabs, {} panes found running ({} with a Claude session, {} with a \
             folder): {} still closed",
            noted.len(),
            live.len(),
            live.iter().filter(|p| p.claude_session.is_some()).count(),
            live.iter().filter(|p| p.cwd.is_some()).count(),
            tabs.len()
        );
        log::debug!("restore: noted {noted:?}, found running {live:?}");
        if tabs.is_empty() {
            log::info!("restore: every noted tab is still open: nothing to ask");
            answered();
            return;
        }
        still_pending(&tabs);
        let Some(window) = self.window.clone() else {
            give_back(session);
            return;
        };
        // Asked again when a tab or a pane comes or goes (tabs_moved): another overlay's pane
        // leaves the mux as it closes
        let tab = match self.lbterm_prompt_tab() {
            Ok(tab) => tab,
            Err(NoTab::Covered) => {
                log::info!(
                    "restore: the open tab has something else over it: asking once it's gone"
                );
                give_back(session);
                return;
            }
            Err(NoTab::NoneFree) => {
                log::info!("restore: no tab of the window's own to ask over yet");
                give_back(session);
                return;
            }
        };
        let count = tabs.len();
        let tab_id = tab.tab_id();
        let answer_to = window.clone();
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |_, mut term| {
            match crate::overlay::restore::ask(&mut term, &tabs) {
                Outcome::Answered(chosen) => {
                    let picked: Vec<TabNote> =
                        chosen.into_iter().filter_map(|i| tabs.get(i).cloned()).collect();
                    log::info!("restore: answered, opening {} of {}", picked.len(), tabs.len());
                    // A window that has gone by the time the answer gets there drops it unrun:
                    // then the note is handed back, to be asked again (restore::Handed)
                    let handed = Handed::new(session, |session| {
                        promise::spawn::spawn_into_main_thread(async move {
                            ask_again(session, "the answer never got to its window")
                        })
                        .detach();
                    });
                    answer_to.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        handed.claim();
                        tw.lbterm_restore_tabs(picked);
                        answered();
                    })));
                }
                // Not an answer: the note stands, and is asked again (start_overlay takes down
                // what's left of this overlay, and only this one)
                Outcome::Cancelled => {
                    promise::spawn::spawn_into_main_thread(async move {
                        ask_again(session, "the prompt went without an answer")
                    })
                    .detach();
                }
            }
            Ok(())
        });
        let overlay_id = overlay.pane_id();
        self.assign_overlay(tab_id, overlay);
        STATE.with(|s| {
            s.borrow_mut().prompt = Some(Prompt {
                window,
                mux_window: self.mux_window_id,
                tab: tab_id,
                overlay: overlay_id,
            })
        });
        log::info!(
            "restore: asking about {count} tab{} over tab {tab_id} of window {}",
            if count == 1 { "" } else { "s" },
            self.mux_window_id
        );
        promise::spawn::spawn(future).detach();
    }

    /// The tab the prompt goes over (restore::prompt_tab), opened: the open tab when it's one of
    /// the window's own with nothing over it, else the first such. Not a mux domain's connection
    /// status, a tab of the GUI's own (TermWizTerminalPane) that goes when the domain has
    /// attached, and never over another overlay (the launcher, a close confirmation), which the
    /// prompt's would take down: nothing while the open tab has one
    fn lbterm_prompt_tab(&mut self) -> Result<Arc<Tab>, NoTab> {
        let mux = Mux::get();
        let (tabs, active) = {
            let window = mux.get_window(self.mux_window_id).ok_or(NoTab::NoneFree)?;
            let tabs: Vec<Arc<Tab>> = window.iter_tabs().cloned().collect();
            (tabs, window.get_active_tab_idx())
        };
        let kinds: Vec<TabKind> = tabs
            .iter()
            .map(|t| match kind_of(t) {
                TabKind::Free if self.tab_state(t.tab_id()).overlay.is_some() => TabKind::Covered,
                kind => kind,
            })
            .collect();
        let idx = restore::prompt_tab(&kinds, active)?;
        if idx != active {
            if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
                if idx < window.count_tabs() {
                    window.set_active_tab_idx_without_saving(idx);
                }
            }
        }
        tabs.get(idx).cloned().ok_or(NoTab::NoneFree)
    }

    /// Open the picked tabs, each in its folder, Claude Code ones resuming their session, and one
    /// that was parked parked again once it's open (Lachlan's 8a; lbterm/NEW-TAB.md, "Landing 4")
    fn lbterm_restore_tabs(&mut self, tabs: Vec<TabNote>) {
        for tab in tabs {
            // LBTERM_RESTORE_CLAUDE stands something else in for claude, for tests
            // (lbterm\perf\restore.ps1), so a test never starts a real Claude Code
            let claude = std::env::var("LBTERM_RESTORE_CLAUDE").unwrap_or_else(|_| "claude".into());
            let args = restore::command(&tab, PWSH, &claude);
            let cwd = tab.cwd.clone().filter(|c| std::path::Path::new(c).is_dir());
            let spawn = config::keyassignment::SpawnCommand {
                args,
                cwd: cwd.map(PathBuf::from),
                ..Default::default()
            };
            if !tab.parked {
                self.spawn_command(&spawn, crate::spawn::SpawnWhere::NewTab);
                continue;
            }
            // The spawn hands back its tab, parked as the note says: at once for the GUI's own,
            // asked of the server for a mux domain's (park.rs), where a server that can't park
            // leaves it unparked, logged
            self.lbterm_spawn_tab_then(spawn, |tab| {
                if crate::park::can_park(&tab) {
                    crate::park::set_parked(&tab, true);
                } else {
                    log::info!("restore: tab {}'s server can't park tabs: reopened unparked", tab.tab_id());
                }
            });
        }
    }
}
