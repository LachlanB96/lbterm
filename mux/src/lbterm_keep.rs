//! LBTerm: a window's last tab, when its program ends by itself (`exit`, or `lbterm cli
//! kill-pane` in a mux server), can be kept until the frontend has a new tab in its place, so the
//! window stays: the GUI's rule while the Conductor's pad is on its rail (wezterm-gui's
//! termwindow/last_tab.rs, lbterm/TAB-MOUSE.md, "The last tab"). Upstream removes a dead tab, then
//! its emptied window, in `Mux::prune_dead_windows`; with a keeper set, a dead tab that would leave
//! its window with nothing alive is handed to it once instead, and stays until the keeper removes
//! it. A mux server sets no keeper, so it prunes as upstream does.

use crate::tab::TabId;
use crate::window::WindowId;
use std::sync::Mutex;

/// Asked once for each dead tab that is its window's last: true when it takes the tab on, to
/// remove it itself once a new tab is in its place
pub type Keeper = Box<dyn Fn(WindowId, TabId) -> bool + Send + Sync>;

static KEEPER: Mutex<Option<Keeper>> = Mutex::new(None);

/// The tabs a keeper has taken on: left alone by the pruning until they're gone
static KEPT: Mutex<Vec<TabId>> = Mutex::new(Vec::new());

pub fn set_keeper(keeper: Keeper) {
    *KEEPER.lock().unwrap() = Some(keeper);
}

/// Whether a dead tab is its window's last: none of the window's other tabs is alive.
/// `others` is whether each of the other tabs is alive.
pub fn is_last(others: impl IntoIterator<Item = bool>) -> bool {
    !others.into_iter().any(|alive| alive)
}

/// Whether the pruning leaves dead tab `tab`, its window's last, to the keeper: one it took on
/// before, or one it takes on now. `live` is every tab the mux still has, so tabs gone since are
/// forgotten.
pub(crate) fn keeps(window: WindowId, tab: TabId, live: &[TabId]) -> bool {
    let mut kept = KEPT.lock().unwrap();
    kept.retain(|id| live.contains(id));
    if kept.contains(&tab) {
        return true;
    }
    let taken = match KEEPER.lock().unwrap().as_ref() {
        Some(keeper) => keeper(window, tab),
        None => false,
    };
    if taken {
        log::info!("last tab {tab} of window {window} ended: kept until a new tab takes its place");
        kept.push(tab);
    }
    taken
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_dead_tab_is_last_when_nothing_else_in_its_window_lives() {
        assert!(is_last([]));
        assert!(is_last([false, false]));
        assert!(!is_last([false, true]));
        assert!(!is_last([true]));
    }
}
