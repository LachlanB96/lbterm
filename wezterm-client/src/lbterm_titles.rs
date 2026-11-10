//! LBTerm: a tab out of view's title alert, and its mux window's title, decided on the client
//! thread as they're read, so a title that changes nothing on the screen hands the GUI thread
//! nothing: no take, no wake, no task (lbterm/FLOWS.md, "A title out of view", What could go 13,
//! parts 2 and 3; the names push before them is lbterm_names.rs, part 1).
//!
//! The server sends three PDUs for each title of a pane out of view: its names push, a NotifyAlert
//! of the title, and the title of the server's mux window, which takes the title of whichever pane
//! set one last. With the names push applied here, this decides the other two:
//!
//! - A title alert (a title, an icon title or a tab title) of a pane with nothing of it waiting in
//!   the backlog (so the names push before it, and anything older, has been applied) is dropped
//!   when the GUI's [`Decider`] says no window would do anything for it: every window's mux
//!   subscription would drop it (wezterm-gui's termwindow/titles.rs, `lbterm_title_alert_dropped`,
//!   which decides as the window's `lbterm_title_alert` does), and no connection to the GUI's own
//!   socket is open to hear it. The GUI's other subscribers of an alert do nothing for a title
//!   (frontend's toasts, crash restore's folders and `claude_session`, the client domain's,
//!   `lbterm_parked`'s), so nothing notifies the mux and nothing is handed. Anything else of a
//!   title alert goes to the backlog, whose take notifies it on the GUI thread as before.
//! - The mux window's title is kept where any thread reads it (mux's `lbterm_window_titles`),
//!   noted as the server's so it isn't sent back, and handed only while the Decider says something
//!   reads a mux window's title as it changes (a format-window-title, format-tab-title or status
//!   handler, or a connection to the GUI's socket); otherwise the next take of anything sets it
//!   on the window ([`set_kept`]), and every reader reads the kept one first.
//!
//! Without a Decider (a process with no GUI) everything goes to the GUI thread as before. The
//! decision's cost is the client thread's, which also reads key echoes: the lookups, the window's
//! tabs walked, each pane of the title's tab asked its title (its renderable's lock), and the
//! rail's row compared, which for Claude Code's ◐ against ◑ makes no row (lbterm_panel's
//! `titles_read`). Nothing it locks is held by the GUI thread while it waits on this thread
//! (lbterm/PITFALLS.md, 313 and 321).

use crate::domain::ClientDomain;
use crate::lbterm_backlog::Backlog;
use codec::{NotifyAlert, Pdu, WindowTitleChanged};
use mux::domain::DomainId;
use mux::{Mux, MuxNotification};
use std::sync::OnceLock;
use wezterm_term::Alert;

/// What only the GUI knows of a title: installed once by wezterm-gui at its first window
pub trait Decider: Send + Sync {
    /// Whether `notification`, a pane's title alert, would change nothing anywhere: every window
    /// drops it and nothing else listens for it
    fn alert_dropped(&self, notification: &MuxNotification) -> bool;
    /// Whether anything reads a mux window's title as it changes
    fn window_titles_read(&self) -> bool;
}

static DECIDER: OnceLock<Box<dyn Decider>> = OnceLock::new();

/// The GUI's Decider, installed once
pub fn set_decider(decider: Box<dyn Decider>) {
    let _ = DECIDER.set(decider);
}

/// Whether `alert` is a title alert this decides
pub fn is_title(alert: &Alert) -> bool {
    matches!(
        alert,
        Alert::WindowTitleChanged(_) | Alert::IconTitleChanged(_) | Alert::TabTitleChanged(_)
    )
}

/// Whether `pdu` was decided here and goes no further: a title alert dropped, or a mux window's
/// title kept with nothing reading it
pub fn decided_here(local_domain_id: DomainId, pdu: &Pdu, backlog: &Backlog) -> bool {
    let Some(decider) = DECIDER.get() else {
        return false;
    };
    // What it costs the client thread, said on its line while perf/title-storm.ps1 counts
    let started = log::log_enabled!(target: "lbterm::title", log::Level::Debug)
        .then(std::time::Instant::now);
    let us = || started.map_or(0, |started| started.elapsed().as_micros());
    match pdu {
        Pdu::NotifyAlert(NotifyAlert { pane_id, alert }) if is_title(alert) => {
            // The names push before it, and anything older of the pane, applied first, so the
            // decision reads the title it brought (lbterm_names.rs: Backlog::waits)
            if backlog.waits(*pane_id) {
                return false;
            }
            let Some(local_pane_id) =
                domain(local_domain_id, |domain| domain.remote_to_local_pane_id(*pane_id))
            else {
                return false;
            };
            let notification = MuxNotification::Alert {
                pane_id: local_pane_id,
                alert: alert.clone(),
            };
            let dropped = decider.alert_dropped(&notification);
            log::debug!(
                target: "lbterm::title",
                "title pane {local_pane_id} decided in {} us: {}",
                us(),
                if dropped {
                    "nothing shown changed, dropped on the client thread"
                } else {
                    "to the GUI thread"
                }
            );
            dropped
        }
        Pdu::WindowTitleChanged(WindowTitleChanged { window_id, title }) => {
            let kept = domain(local_domain_id, |domain| {
                Some(domain.lbterm_window_title_kept(*window_id, title))
            })
            .unwrap_or(false);
            // Kept whether or not it's handed, so a reader never reads an older one; handed while
            // something reads it as it changes, whose take sets it as before
            let here = kept && !decider.window_titles_read();
            log::debug!(
                target: "lbterm::title",
                "window title (server's {window_id}) in {} us: {}",
                us(),
                if here {
                    "kept on the client thread"
                } else {
                    "to the GUI thread"
                }
            );
            here
        }
        _ => false,
    }
}

/// `with` the ClientDomain `local_domain_id`, if the mux has it
fn domain<T>(local_domain_id: DomainId, with: impl FnOnce(&ClientDomain) -> Option<T>) -> Option<T> {
    let mux = Mux::try_get()?;
    let domain = mux.get_domain(local_domain_id)?;
    let domain = domain.downcast_ref::<ClientDomain>()?;
    with(domain)
}

/// The mux window titles kept, set on their windows: by each take, on the GUI thread, which every
/// subscriber of a window's title is written for
pub fn set_kept() {
    let kept = mux::lbterm_window_titles::take();
    if kept.is_empty() {
        return;
    }
    let Some(mux) = Mux::try_get() else {
        return;
    };
    for (window, title) in kept {
        if let Some(mut window) = mux.get_window_mut(window) {
            window.set_title(&title);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_title_icon_title_or_tab_title_is_decided() {
        assert!(is_title(&Alert::WindowTitleChanged("◐ a".into())));
        assert!(is_title(&Alert::IconTitleChanged(Some("◑ a".into()))));
        assert!(is_title(&Alert::TabTitleChanged(Some("a".into()))));
        assert!(!is_title(&Alert::CurrentWorkingDirectoryChanged));
        assert!(!is_title(&Alert::Bell));
        assert!(!is_title(&Alert::OutputSinceFocusLost));
    }
}
