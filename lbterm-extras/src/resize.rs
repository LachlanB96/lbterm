//! Which of a window's tabs take a size as the window is resized (lbterm/MUX.md, "A window's
//! resize"). Upstream gave every tab every size the window passed through, and a tab's size
//! reaches each of its panes' terminals, which fit their whole scrollback to it: on a mux domain
//! the server's main thread, one pane after another, about 70 ms a pane with 50,000 lines of
//! scrollback, so a drag of the frame through 40 sizes with 12 such tabs held every tab's echo
//! for 35 s (perf/mux-resize.ps1, 2026-11-05); in a window's own tabs the GUI thread.
//!
//! While the frame is being dragged (Windows' sizing loop, WM_ENTERSIZEMOVE to WM_EXITSIZEMOVE)
//! only the tab in view takes each size, as it's the one seen redrawn; the others take the size
//! the drag ends at, once, when it ends. A size that comes outside a drag (maximized, restored,
//! snapped, a font size, a move to another screen) goes to every tab at once, as upstream's did.
//! The window's code (wezterm-gui's termwindow/resize.rs) says each size it applies
//! ([`Behind::sizing`]), asks [`Behind::takes`] for each tab and says when it's done
//! ([`Behind::applied`]); the drag's end, which brings no new size, is [`Behind::ended`].

/// Whether tabs are behind the window's size, left there by a drag of its frame
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Behind {
    /// the size being applied is a drag's
    live: bool,
    /// the last size the window was given was a drag's, and the drag isn't over: a size the
    /// window's code finds for itself meanwhile (the paint's check of the window's size, which
    /// can see the drag's next size before the window is told of it) is the drag's too
    dragging: bool,
    /// a tab was left at an older size by a drag's size
    behind: bool,
}

impl Behind {
    /// A size is being applied: whether it's a drag's (`live`), until the next
    pub fn sizing(&mut self, live: bool) {
        self.live = live;
        self.dragging = live;
    }

    /// Whether a drag of the frame is under way: the last size given was a drag's, and none
    /// since said it was over
    pub fn dragging(&self) -> bool {
        self.dragging
    }

    /// Whether a tab, in view (`shown`) or not, takes the size being applied now. One that
    /// doesn't is behind until [`Behind::ended`]
    pub fn takes(&mut self, shown: bool) -> bool {
        if self.live && !shown {
            self.behind = true;
            return false;
        }
        true
    }

    /// The size being applied has gone to the tabs [`Behind::takes`] said: unless it was a
    /// drag's, that's every tab, so none is behind
    pub fn applied(&mut self) {
        if !self.live {
            self.behind = false;
        }
        self.live = false;
    }

    /// The drag is over with no new size (the window's code is given its last size again):
    /// whether tabs were left behind, which then take the window's size; they're no longer behind
    pub fn ended(&mut self) -> bool {
        self.live = false;
        self.dragging = false;
        std::mem::take(&mut self.behind)
    }

    /// Whether a drag left tabs behind
    pub fn is_behind(&self) -> bool {
        self.behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_a_drag_every_tab_takes_the_size() {
        let mut b = Behind::default();
        b.sizing(false);
        assert!(b.takes(true));
        assert!(b.takes(false));
        b.applied();
        assert!(!b.is_behind());
        assert!(!b.ended());
    }

    #[test]
    fn a_drag_gives_the_tab_in_view_each_size_and_the_rest_the_last() {
        let mut b = Behind::default();
        for _ in 0..3 {
            b.sizing(true);
            assert!(b.takes(true), "the tab in view takes each size of the drag");
            assert!(!b.takes(false), "a tab out of view waits for the drag's end");
            assert!(!b.takes(false));
            b.applied();
        }
        assert!(b.is_behind());
        assert!(b.ended(), "the drag's end says tabs were left behind");
        assert!(!b.is_behind());
        assert!(!b.ended(), "and only once");
    }

    #[test]
    fn a_size_outside_a_drag_after_one_brings_every_tab_along() {
        let mut b = Behind::default();
        b.sizing(true);
        assert!(!b.takes(false));
        b.applied();
        // Maximized straight after the drag, before its end was seen: every tab takes that size,
        // so none is behind any more
        b.sizing(false);
        assert!(b.takes(false));
        b.applied();
        assert!(!b.is_behind());
        assert!(!b.ended());
    }

    #[test]
    fn a_size_applied_outside_sizing_isnt_a_drags() {
        // A font size or a new screen applies the window's size without a resize event: whatever
        // the last drag's size left, that isn't a drag's
        let mut b = Behind::default();
        b.sizing(true);
        b.applied();
        assert!(b.takes(false));
    }

    #[test]
    fn a_drag_is_under_way_until_a_size_that_isnt_a_drags() {
        let mut b = Behind::default();
        assert!(!b.dragging());
        b.sizing(true);
        assert!(!b.takes(false));
        b.applied();
        assert!(b.dragging(), "between a drag's sizes, the drag goes on");
        b.sizing(true);
        b.applied();
        assert!(b.dragging());
        assert!(b.ended());
        assert!(!b.dragging(), "its end ends it");
        b.sizing(true);
        b.applied();
        b.sizing(false);
        assert!(!b.dragging(), "and so does a size outside a drag");
    }

    #[test]
    fn a_drag_with_only_the_tab_in_view_leaves_nothing_behind() {
        let mut b = Behind::default();
        b.sizing(true);
        assert!(b.takes(true));
        b.applied();
        assert!(!b.ended());
    }
}
