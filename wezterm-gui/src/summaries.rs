//! LBTerm: each Claude Code session's summary and label, from the folder
//! `lbterm_summaries` names, read on a thread of their own
//! (lbterm-extras/src/summaries.rs) and shared by every window: the tab
//! rail's labels and the window strip's summary.
//!
//! A pane's session is its `claude_session` user var. [`get`] answers from
//! memory; a session it hasn't seen is read off this thread, and every window
//! repaints when it arrives or any summary asked for changes.

use crate::termwindow::TermWindowNotif;
use lbterm_extras::summaries::{Summaries, Summary};
use std::sync::{Arc, Mutex};
use window::WindowOps;

/// The store for the folder last configured
static STORE: Mutex<Option<(String, Arc<Summaries>)>> = Mutex::new(None);

/// The store for the configured folder, started the first time it's asked for
/// and again if the folder changes. `None` with `lbterm_summaries` unset.
pub fn store() -> Option<Arc<Summaries>> {
    let config = config::configuration();
    let dir = config.lbterm_summaries.as_deref().filter(|d| !d.is_empty())?;
    let mut store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((folder, summaries)) = store.as_ref() {
        if folder == dir {
            return Some(Arc::clone(summaries));
        }
    }
    let summaries = Arc::new(Summaries::start(dir.into(), || {
        promise::spawn::spawn_into_main_thread(async move { repaint() }).detach();
    }));
    *store = Some((dir.to_string(), Arc::clone(&summaries)));
    Some(summaries)
}

/// What's known of session `sid`, if summaries are on.
pub fn get(sid: &str) -> Option<Summary> {
    store()?.get(sid)
}

/// Every window draws again, with what's new
fn repaint() {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    for win in fe.gui_windows() {
        win.window.notify(TermWindowNotif::Apply(Box::new(|tw| {
            if let Some(window) = tw.window.as_ref() {
                window.invalidate();
            }
        })));
    }
}
