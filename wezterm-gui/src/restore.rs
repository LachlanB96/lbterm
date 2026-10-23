//! LBTerm: crash restore (lbterm_extras::restore). LBTerm keeps a note of its tabs in
//! `session-<window class>.json` in its data folder, rewritten a couple of seconds after they
//! change. Closing LBTerm normally marks the note clean. When LBTerm starts and finds a note
//! that isn't clean (a crash, the process killed, Windows restarting), its first window asks
//! which of those tabs to open again, before LBTerm writes a note of its own: until that's
//! answered, the old note stays, so closing LBTerm without answering asks again next time.

use crate::termwindow::TermWindow;
use lbterm_extras::restore::{self, Session, TabNote, WindowNote};
use mux::pane::CachePolicy;
use mux::{Mux, MuxNotification};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;
use wezterm_term::Alert;
use window::WindowOps;

/// A burst of changes is written once, this long after the first
const SAVE_AFTER: Duration = Duration::from_secs(2);
/// PowerShell 7, by full path (the Store alias can't be started by path)
const PWSH: &str = r"C:\Program Files\PowerShell\7\pwsh.exe";

#[derive(Default)]
struct State {
    /// Our own note is written only once the last one has been offered and answered
    writing: bool,
    save_due: bool,
    pid: u32,
    started: u64,
    /// The note found at start-up, until the first window offers it
    offer: Option<Session>,
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

/// When a process started (its creation FILETIME); `None` when it isn't running
#[cfg(windows)]
fn start_time(pid: u32) -> Option<u64> {
    use windows58::Win32::Foundation::{CloseHandle, FILETIME};
    use windows58::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let ok = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        ok.ok()?;
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
    let writing = offer.is_none();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pid = pid;
        s.started = started;
        s.writing = writing;
        s.offer = offer;
    });

    Mux::get().subscribe(|n| {
        let tabs_changed = match &n {
            MuxNotification::WindowCreated(_)
            | MuxNotification::WindowRemoved(_)
            | MuxNotification::TabAddedToWindow { .. }
            | MuxNotification::PaneRemoved(_)
            | MuxNotification::TabTitleChanged { .. } => true,
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
        if tabs_changed {
            promise::spawn::spawn_into_main_thread(async { changed() }).detach();
        }
        true
    });
}

/// The first window takes the note found at start-up, to offer it
pub fn take_offer() -> Option<Session> {
    STATE.with(|s| s.borrow_mut().offer.take())
}

/// The offer was answered (or there was none): from now on LBTerm keeps its own note
pub fn answered() {
    STATE.with(|s| s.borrow_mut().writing = true);
    changed();
}

/// Something about the tabs changed: write the note soon
pub fn changed() {
    let due = STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.writing || s.save_due {
            return false;
        }
        s.save_due = true;
        true
    });
    if due {
        promise::spawn::spawn(async {
            smol::Timer::after(SAVE_AFTER).await;
            STATE.with(|s| s.borrow_mut().save_due = false);
            save(false, false);
        })
        .detach();
    }
}

/// LBTerm is closing normally: mark the note clean, so nothing is offered next time. Unless
/// panes outlive it in a mux server: then they're still running, and if the server dies before
/// the next GUI (a restart), the note should still offer them.
pub fn clean_exit() {
    if STATE.with(|s| s.borrow().writing) {
        save(surviving_panes().is_empty(), true);
    }
}

/// Panes in a domain other than the local one. They outlive a GUI (a mux server's), so after
/// a GUI crash they're still running and the next GUI reattaches to them.
fn surviving_panes() -> Vec<std::sync::Arc<dyn mux::pane::Pane>> {
    let mux = Mux::get();
    mux.iter_panes()
        .into_iter()
        .filter(|p| {
            mux.get_domain(p.domain_id())
                .map_or(false, |d| d.domain_name() != "local")
        })
        .collect()
}

fn live_panes() -> Vec<restore::LivePane> {
    surviving_panes()
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
        })
        .collect()
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
    let (pid, started) = STATE.with(|s| {
        let s = s.borrow();
        (s.pid, s.started)
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
    }
}

impl TermWindow {
    /// Ask in this window about the note found at start-up, if there was one
    pub(crate) fn lbterm_offer_restore(&mut self) {
        let Some(session) = take_offer() else {
            return;
        };
        if surviving_panes().is_empty() {
            self.lbterm_offer_now(session);
            return;
        }
        // Panes reattached from a mux server: their user vars (claude_session) arrive a moment
        // after they're listed, so look a little later
        let Some(window) = self.window.clone() else {
            answered();
            return;
        };
        promise::spawn::spawn(async move {
            smol::Timer::after(Duration::from_secs(2)).await;
            window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(move |tw| {
                tw.lbterm_offer_now(session)
            })));
        })
        .detach();
    }

    /// Offer the noted tabs that aren't still open in a mux server
    fn lbterm_offer_now(&mut self, session: Session) {
        let noted: Vec<&TabNote> = session.tabs().collect();
        let tabs: Vec<TabNote> = restore::still_closed(&noted, &live_panes())
            .into_iter()
            .map(|i| noted[i].clone())
            .collect();
        if tabs.is_empty() {
            answered();
            return;
        }
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            answered();
            return;
        };
        let window = self.window.clone().unwrap();
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |tab_id, mut term| {
            let chosen = crate::overlay::restore::ask(&mut term, &tabs).unwrap_or_default();
            let picked: Vec<TabNote> = chosen.into_iter().filter_map(|i| tabs.get(i).cloned()).collect();
            window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(move |tw| {
                tw.lbterm_restore_tabs(picked);
                answered();
            })));
            TermWindow::schedule_cancel_overlay(window, tab_id, None);
            Ok(())
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    /// Open the picked tabs, each in its folder, Claude Code ones resuming their session
    fn lbterm_restore_tabs(&mut self, tabs: Vec<TabNote>) {
        for tab in tabs {
            // LBTERM_RESTORE_CLAUDE stands something else in for claude, for tests
            // (lbterm\perf\restore.ps1), so a test never starts a real Claude Code
            let claude = std::env::var("LBTERM_RESTORE_CLAUDE").unwrap_or_else(|_| "claude".into());
            let args = restore::command(&tab, PWSH, &claude);
            let cwd = tab.cwd.clone().filter(|c| std::path::Path::new(c).is_dir());
            self.spawn_command(
                &config::keyassignment::SpawnCommand {
                    args,
                    cwd: cwd.map(PathBuf::from),
                    ..Default::default()
                },
                crate::spawn::SpawnWhere::NewTab,
            );
        }
    }
}
