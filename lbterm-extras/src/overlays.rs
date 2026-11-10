//! What a window's frames read of a pane with an overlay over it, for the pace of a shown pane on
//! a mux domain (wezterm-gui's termwindow/panes_drawn.rs; lbterm/MUX.md, "A shown pane's pace").
//!
//! WezTerm has two kinds of overlay (lbterm/CODEMAP.md, "A frame"). A tab overlay
//! (`tab_state(tab).overlay`: the launcher, the tab navigator, the palette) replaces the whole
//! tab with its one pane, so the frames read none of the tab's panes. A pane overlay
//! (`pane_state(pane).overlay`) takes one pane's place in the paint: copy mode, search and quick
//! select report the id of the pane under them and read their lines from it, so the frames read
//! that pane through them; the close-pane confirmation is a pane of its own, with an id of its
//! own, and the frames read it instead of the pane. A server pacing a pane waits, with no bound,
//! for a word that a frame read its last push (PITFALLS 234), so which of these a pane is under
//! decides who says the word: the frame, or the window as the push comes.
//!
//! Pane ids are `mux::pane::PaneId`, a usize.

/// What a window's frames read of a pane that printed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shows {
    /// Its frames read it: a pane of the open tab, one under a pane overlay that reads its lines
    /// (copy mode, search, quick select), or the tab overlay's own pane. A frame says it
    Drawn,
    /// A pane of the open tab under a pane overlay of its own (the close-pane confirmation): the
    /// frames draw the overlay in its place and read nothing of it. Repainted, as upstream did,
    /// and said as the push comes
    Overlaid,
    /// A pane of the open tab under a tab overlay, which shows none of the tab. Said as the push
    /// comes
    Covered,
    /// Not the window's open tab's
    No,
}

/// What the frames of a window read of `pane`, which printed. `tab_overlay` is the open tab's
/// tab overlay's pane id, if one is open; `in_tab` asks whether the open tab holds `pane` (a walk
/// of its tree, so asked only when needed, as upstream's `is_pane_visible` does); `pane_overlay`
/// gives the id the pane overlay over `pane` reports, if there is one (a lookup, asked only for a
/// pane of the open tab with no tab overlay)
pub fn shows(
    pane: usize,
    tab_overlay: Option<usize>,
    in_tab: impl FnOnce() -> bool,
    pane_overlay: impl FnOnce() -> Option<usize>,
) -> Shows {
    if tab_overlay == Some(pane) {
        return Shows::Drawn;
    }
    if !in_tab() {
        return Shows::No;
    }
    if tab_overlay.is_some() {
        return Shows::Covered;
    }
    match pane_overlay() {
        Some(over) if !reads_under(pane, Some(over)) => Shows::Overlaid,
        _ => Shows::Drawn,
    }
}

/// Whether the pane overlay over `pane`, which reports `pane_overlay` as its id, reads its lines
/// from `pane`: copy mode, search and quick select report the pane under them's id
/// (wezterm-gui's overlay/copy.rs and quickselect.rs, `pane_id`, their `delegate`'s), and the
/// close-pane confirmation reports its own
pub fn reads_under(pane: usize, pane_overlay: Option<usize>) -> bool {
    pane_overlay == Some(pane)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_pane_with_no_overlay_is_drawn_when_the_tab_holds_it() {
        assert_eq!(shows(3, None, || true, || None), Shows::Drawn);
        assert_eq!(shows(3, None, || false, || None), Shows::No);
    }

    #[test]
    fn copy_mode_search_and_quick_select_read_the_pane_under_them() {
        // Each reports the id of the pane under it, so the frame reads that pane and says it
        assert_eq!(shows(3, None, || true, || Some(3)), Shows::Drawn);
        assert!(reads_under(3, Some(3)));
    }

    #[test]
    fn the_close_confirmation_is_a_pane_of_its_own_over_it() {
        assert_eq!(shows(3, None, || true, || Some(17)), Shows::Overlaid);
        assert!(!reads_under(3, Some(17)));
        assert!(!reads_under(3, None));
    }

    #[test]
    fn a_tab_overlay_covers_every_pane_of_the_tab_and_shows_its_own() {
        assert_eq!(shows(3, Some(9), || true, || None), Shows::Covered);
        assert_eq!(shows(9, Some(9), || false, || None), Shows::Drawn);
        assert_eq!(shows(4, Some(9), || false, || None), Shows::No);
    }

    #[test]
    fn a_pane_overlay_under_a_tab_overlay_is_still_covered() {
        // The tab overlay replaces the whole tab, copy mode under it included
        assert_eq!(shows(3, Some(9), || true, || Some(3)), Shows::Covered);
    }

    #[test]
    fn the_tree_and_the_pane_state_are_asked_only_when_needed() {
        let asked = std::cell::Cell::new(0);
        let tree = || {
            asked.set(asked.get() + 1);
            true
        };
        assert_eq!(shows(9, Some(9), tree, || panic!("no pane state")), Shows::Drawn);
        assert_eq!(asked.get(), 0);
        assert_eq!(shows(3, Some(9), || true, || panic!("no pane state")), Shows::Covered);
        assert_eq!(shows(3, None, || false, || panic!("no pane state")), Shows::No);
    }
}
