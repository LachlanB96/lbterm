//! LBTerm: voice drafts (lbterm/CONTROL.md, "Drafts"; the logic is lbterm_extras::drafts), one a
//! pane, by the mux server's pane id, so every window showing the pane draws it
//! (termwindow/render/draft.rs). The control pipe changes them (termwindow/control.rs), on the
//! GUI thread; every change repaints the windows.

use crate::state_files::repaint_windows;
use lbterm_extras::drafts::Draft;
use std::collections::BTreeMap;
use std::sync::Mutex;

static DRAFTS: Mutex<BTreeMap<u64, Draft>> = Mutex::new(BTreeMap::new());

/// The draft over `pane`
pub fn get(pane: u64) -> Option<Draft> {
    DRAFTS.lock().unwrap().get(&pane).cloned()
}

/// Whether there's a draft over any pane: every paint asks, and nearly always there's none
pub fn any() -> bool {
    !DRAFTS.lock().unwrap().is_empty()
}

/// Keeps `draft` over its pane, or lets it go once it's empty
pub fn put(draft: Draft) {
    {
        let mut drafts = DRAFTS.lock().unwrap();
        if draft.is_empty() {
            drafts.remove(&draft.pane);
        } else {
            drafts.insert(draft.pane, draft);
        }
    }
    changed();
}

/// Lets the draft over `pane` go: what it was
pub fn take(pane: u64) -> Option<Draft> {
    let went = DRAFTS.lock().unwrap().remove(&pane);
    if went.is_some() {
        changed();
    }
    went
}

fn changed() {
    promise::spawn::spawn_into_main_thread(async { repaint_windows() }).detach();
}
