//! LBTerm: each Claude Code session's summary and label, from the folder
//! `lbterm_summaries` names, read on a thread of their own
//! (lbterm-extras/src/summaries.rs) and shared by every window: the tab
//! rail's labels and the window strip's summary.
//!
//! When a session was last prompted (`<sid>.prompt.json`) shows only on the open
//! tab panel's rows, beside its cost, so a change to it, alone or with the cost,
//! comes as a cost does (`Changed::Usage`) and repaints only the windows showing
//! that session's dollars, below (lbterm/TAB-PANEL.md, "What a row says").
//!
//! A pane's session is its `claude_session` user var. [`get`] answers from
//! memory; a session it hasn't seen is read off this thread, and every window
//! repaints when it arrives or any summary asked for changes, except for a
//! change to what a session has cost alone (`<sid>.usage.json`, which Terminal's
//! statusline rewrites as often as a cent moves): only a window showing that
//! session's dollars repaints, on the context pill or the open tab panel
//! (render/context.rs, `lbterm_shows_usage`). A write of `<sid>.cost.json`, the
//! Claude project's split, which nothing here reads, wakes no window: it goes to
//! crate::cost on the summaries thread, which asks for the split only for a cost
//! pane open on that session.

use crate::termwindow::TermWindowNotif;
use lbterm_extras::summaries::{Changed, Summaries, Summary};
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
    let summaries = Arc::new(Summaries::start(dir.into(), |changed| match changed {
        // On this thread: nothing wakes while no cost pane is open on one of them
        Changed::Split(sids) => crate::cost::split_written(&sids),
        changed => {
            promise::spawn::spawn_into_main_thread(async move { repaint_for(changed) }).detach();
        }
    }));
    *store = Some((dir.to_string(), Arc::clone(&summaries)));
    Some(summaries)
}

/// What's known of session `sid`, if summaries are on: shared, nothing of it copied (the rail
/// asks for every tab's on every frame).
pub fn get(sid: &str) -> Option<Arc<Summary>> {
    store()?.get(sid)
}

/// How many times session `sid`'s `<sid>.cost.json` has been heard written (0 with summaries off,
/// or before any): the cost pane asks for the split again only when this has moved
/// (lbterm_extras::summaries::Summaries::split_written)
pub fn split_written(sid: &str) -> u64 {
    store().map_or(0, |s| s.split_written(sid))
}

/// The windows that show what changed draw again: every one for anything a window may show, and
/// for what sessions have cost alone, those showing one of them
fn repaint_for(changed: Changed) {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    let sessions = match changed {
        Changed::Any => None,
        Changed::Usage(sids) => Some(Arc::new(sids)),
        // Never sent here: store() gives it to crate::cost
        Changed::Split(_) => return,
    };
    for win in fe.gui_windows() {
        let sessions = sessions.clone();
        win.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            if sessions.as_deref().map_or(true, |sids| tw.lbterm_shows_usage(sids)) {
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            }
        })));
    }
}
