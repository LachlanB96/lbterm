//! LBTerm: a mux window's title as its mux server last gave it, kept where any thread can read
//! it until it's set on the window.
//!
//! A GUI on a mux domain hears its server's window title change with every title of every pane,
//! since the server's window takes the title of whichever pane set one last: Claude Code's ◐ and
//! ◑ in eight working tabs are eight a second. Nothing in the GUI reads that title as it changes
//! unless a Lua handler could (format-window-title, format-tab-title, the status handlers) or a
//! connection to the GUI's own socket listens, so the client thread keeps it here and hands the
//! GUI thread nothing for it (wezterm-client's lbterm_titles.rs). It's set on the window by the
//! next take the GUI thread runs for anything else ([`take`]), and whatever reads a window's title
//! reads it through here first ([`title`]): Lua's `MuxWindow:get_title`, the `window_title` of the
//! tab information format-tab-title and format-window-title are given, the GUI socket's
//! ListPanes, and the client domain's check of a title before it sends one back. So nothing reads
//! an older title than the server's (lbterm/FLOWS.md, "A title out of view", What could go 13,
//! part 3).
//!
//! Only the newest title of each window is kept, so it holds a title a window at most.

use crate::window::{Window, WindowId};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

static KEPT: Mutex<Option<HashMap<WindowId, String>>> = Mutex::new(None);
/// Whether anything is kept: a reader or a take with nothing kept costs one load
static ANY: AtomicBool = AtomicBool::new(false);

/// The server's newest `title` for local window `window`, not yet set on it
pub fn keep(window: WindowId, title: &str) {
    let mut kept = KEPT.lock();
    kept.get_or_insert_with(HashMap::new)
        .insert(window, title.to_string());
    ANY.store(true, Ordering::Release);
}

/// What a reader of `window`'s title reads: the title kept for it, else its own
pub fn title(window: &Window) -> String {
    kept_or(window.window_id(), window.get_title())
}

/// The title kept for `window`, else `own`
fn kept_or(window: WindowId, own: &str) -> String {
    if ANY.load(Ordering::Acquire) {
        if let Some(title) = KEPT.lock().as_ref().and_then(|kept| kept.get(&window)) {
            return title.clone();
        }
    }
    own.to_string()
}

/// Every title kept, taken to be set on its window (on the GUI thread, whose subscribers a
/// window's title notifies)
pub fn take() -> Vec<(WindowId, String)> {
    if !ANY.load(Ordering::Acquire) {
        return vec![];
    }
    let mut kept = KEPT.lock();
    ANY.store(false, Ordering::Release);
    kept.take()
        .map(|kept| kept.into_iter().collect())
        .unwrap_or_default()
}

/// `title` was set on `window`: forgotten here if it's still the one kept, so a newer one kept
/// while it was on its way stays
pub fn set(window: WindowId, title: &str) {
    if !ANY.load(Ordering::Acquire) {
        return;
    }
    let mut kept = KEPT.lock();
    if let Some(map) = kept.as_mut() {
        if map.get(&window).map_or(false, |kept| kept == title) {
            map.remove(&window);
        }
        if map.is_empty() {
            ANY.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // KEPT is the process's, so one test holds every rule in turn
    #[test]
    fn a_kept_title_is_read_until_it_is_set_and_a_newer_one_outlives_an_older_set() {
        // A window id no Window has (Window::new needs a Mux)
        let id: WindowId = usize::MAX - 7;
        let title = || kept_or(id, "own");
        assert_eq!(title(), "own", "nothing kept: the window's own");
        keep(id, "◐ one");
        keep(id, "◑ one");
        assert_eq!(title(), "◑ one", "the newest kept");
        set(id, "◐ one");
        assert_eq!(title(), "◑ one", "an older title set leaves the newer kept");
        set(id, "◑ one");
        assert_eq!(title(), "own", "the kept title set is forgotten");
        assert!(take().is_empty());
        keep(id, "◐ two");
        assert_eq!(take(), vec![(id, "◐ two".to_string())]);
        assert!(take().is_empty(), "taken once");
        assert_eq!(title(), "own");
    }
}
