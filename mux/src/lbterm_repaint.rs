//! LBTerm: the mux changes a window shows that upstream's GUI doesn't paint it for.
//!
//! A window paints when it's invalidated, and the GUI invalidates it on a visible pane's output,
//! on `WindowInvalidated` (a tab added, closed or switched to, a pane gone) and when the tab
//! bar's words change. The pane with the keyboard changing (`PaneFocused`) and a tab zoomed,
//! unzoomed or its splits moved (`TabResized`) do none of those, so they showed only once
//! something else painted the window: a program's output, the pointer, a blinking cursor
//! (Lachlan's doesn't blink). Upstream's `activate_pane_direction` sends `WindowInvalidated`
//! itself, but a mux server keeps that to itself and its client hears only `PaneFocused`. A tab
//! renamed (`TabTitleChanged`) paints only while the tab bar holds the tabs' words
//! (`show_tabs_in_tab_bar`); the rail and the strip name it either way. What the window does
//! itself (a key, a click) paints anyway; these come from outside it: `lbterm cli` (the
//! Conductor, AI Dashboard, Relay, terminal.ahk), Lua, and on a mux domain the server telling its
//! client about either. The GUI asks `repaints` of each notification it gets (wezterm-gui's
//! termwindow/mod.rs, `mux_pane_output_event_callback`).

use crate::pane::PaneId;
use crate::tab::TabId;
use crate::window::WindowId;
use crate::{Mux, MuxNotification};

/// Where a change is that the window holding it shows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    /// Which pane has the keyboard in this pane's tab
    Pane(PaneId),
    /// A tab's layout (zoom, splits) or name
    Tab(TabId),
}

/// What a notification changed that the GUI doesn't otherwise paint a window for. None for
/// those it already paints on (output, `WindowInvalidated`) and those that change nothing drawn.
pub fn changed(notification: &MuxNotification) -> Option<Changed> {
    match notification {
        MuxNotification::PaneFocused(pane_id) => Some(Changed::Pane(*pane_id)),
        MuxNotification::TabResized(tab_id) | MuxNotification::TabTitleChanged { tab_id, .. } => {
            Some(Changed::Tab(*tab_id))
        }
        _ => None,
    }
}

/// Whether mux window `window` shows the change: its pane or tab is in that window, as
/// `pane_window` and `tab_window` say (the mux's answers, in `repaints`)
pub fn shows(
    changed: Changed,
    window: WindowId,
    pane_window: impl FnOnce(PaneId) -> Option<WindowId>,
    tab_window: impl FnOnce(TabId) -> Option<WindowId>,
) -> bool {
    match changed {
        Changed::Pane(pane_id) => pane_window(pane_id) == Some(window),
        Changed::Tab(tab_id) => tab_window(tab_id) == Some(window),
    }
}

/// Whether the GUI window showing mux window `window` must paint for `notification`
pub fn repaints(notification: &MuxNotification, window: WindowId) -> bool {
    let Some(changed) = changed(notification) else {
        return false;
    };
    let Some(mux) = Mux::try_get() else {
        return false;
    };
    shows(
        changed,
        window,
        |pane_id| mux.resolve_pane_id(pane_id).map(|(_, window_id, _)| window_id),
        |tab_id| mux.window_containing_tab(tab_id),
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn focus_zoom_splits_and_names_are_changes_a_window_shows() {
        assert_eq!(
            changed(&MuxNotification::PaneFocused(3)),
            Some(Changed::Pane(3))
        );
        assert_eq!(
            changed(&MuxNotification::TabResized(2)),
            Some(Changed::Tab(2))
        );
        assert_eq!(
            changed(&MuxNotification::TabTitleChanged {
                tab_id: 4,
                title: "Repaint".to_string()
            }),
            Some(Changed::Tab(4))
        );
    }

    #[test]
    fn what_the_gui_already_paints_for_or_never_draws_isnt() {
        // Output and WindowInvalidated paint already (mux_pane_output_event, dispatch_notif)
        assert_eq!(changed(&MuxNotification::PaneOutput(1)), None);
        assert_eq!(changed(&MuxNotification::WindowInvalidated(0)), None);
        // The window's own title isn't drawn: the strip names the tab
        assert_eq!(
            changed(&MuxNotification::WindowTitleChanged {
                window_id: 0,
                title: "x".to_string()
            }),
            None
        );
        assert_eq!(changed(&MuxNotification::PaneAdded(5)), None);
        assert_eq!(changed(&MuxNotification::PaneRemoved(5)), None);
        assert_eq!(changed(&MuxNotification::Empty), None);
        assert_eq!(
            changed(&MuxNotification::SaveToDownloads {
                name: None,
                data: Arc::new(vec![])
            }),
            None
        );
    }

    #[test]
    fn a_window_paints_for_its_own_panes_and_tabs_only() {
        let in_window = |w: WindowId| move |_| Some(w);
        let nowhere = |_| None;
        // Pane 3 is in window 1
        assert!(shows(Changed::Pane(3), 1, in_window(1), nowhere));
        assert!(!shows(Changed::Pane(3), 0, in_window(1), nowhere));
        // Tab 2 is in window 0
        assert!(shows(Changed::Tab(2), 0, nowhere, in_window(0)));
        assert!(!shows(Changed::Tab(2), 1, nowhere, in_window(0)));
        // Gone from the mux (closed since): no window shows it
        assert!(!shows(Changed::Pane(3), 0, nowhere, nowhere));
        assert!(!shows(Changed::Tab(2), 0, nowhere, nowhere));
    }

    #[test]
    fn a_pane_and_a_tab_are_looked_up_as_what_they_are() {
        // The pane's id asks where panes are, the tab's where tabs are: ids of the two overlap
        assert!(shows(
            Changed::Pane(7),
            1,
            |p| (p == 7).then_some(1),
            |_| None
        ));
        assert!(shows(
            Changed::Tab(7),
            2,
            |_| None,
            |t| (t == 7).then_some(2)
        ));
    }
}
