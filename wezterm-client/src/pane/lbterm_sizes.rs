//! LBTerm: sizes a client pane is given that came from its server, which aren't sent back to it
//! (lbterm/MUX.md, "A window's resize").
//!
//! A resync (`ClientDomain::resync`, on every TabResized and TabAddedToWindow the server sends)
//! lists the server's panes and syncs each local tab to the server's tree at the server's sizes
//! (`Tab::sync_with_pane_tree`), which resizes every pane, and a client pane sent every size it
//! was given to the server as a Resize, though those had come from the server. During a drag of
//! the window's frame the server's sizes are older than the window's: the window had sent newer
//! ones, which the server hadn't applied yet when it listed its panes. So each resync sent the
//! older sizes back, the server fitted every scrollback to them again, said TabResized again and
//! the window resynced again: 100 Resize more than the drag's 480 on 2026-11-05, and the tabs
//! could end the drag at a size the window wasn't (perf/mux-resize.ps1; PITFALLS 141).
//!
//! Now a pane given a size inside [`from_server`] takes it without sending it. A listing older
//! than what the window asked for is one asked before the server had the window's Resize, so it
//! comes while that Resize is unanswered ([`Unanswered`]): then the resync puts the window's own
//! size back on the tab, without sending it either, as the server will get there by itself and
//! say so (TabResized, then a listing at that size). With nothing unanswered, the server's size
//! is the tab's, as upstream took it. Putting the window's size back whenever the two differed,
//! and sending it, as this branch first did, sent 21 Resize more in a drag, and two windows
//! showing one tab at different sizes would each have sent their own back at every TabResized,
//! for ever (the review of perf/mux-client).
//!
//! A server of this build lists panes only once none is being resized
//! (wezterm-mux-server-impl's lbterm_resize.rs, `Resizes::after_all`); an older one lists them as
//! it comes to the request, behind the Resizes before it. Either way a listing asked after a
//! Resize has its size.

use mux::tab::Tab;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use wezterm_term::TerminalSize;

thread_local! {
    static FROM_SERVER: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with every size a client pane is given in it taken as the server's own
pub fn from_server<R>(f: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            FROM_SERVER.with(|c| c.set(self.0));
        }
    }
    let _reset = Reset(FROM_SERVER.with(|c| c.replace(true)));
    f()
}

/// Whether a size given now came from the server, so isn't sent to it
pub fn is_from_server() -> bool {
    FROM_SERVER.with(|c| c.get())
}

/// The Resize requests a client pane has sent that the server hasn't answered yet
#[derive(Clone, Debug, Default)]
pub struct Unanswered(Arc<AtomicUsize>);

/// One Resize sent: unanswered until this is dropped, when its answer comes (or its task goes)
pub struct Sent(Arc<AtomicUsize>);

impl Drop for Sent {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Sent {
    /// Its answer came: whether it was the pane's last Resize unanswered, so no later one will
    /// ask for the push that says what the rewrap changed (clientpane.rs's `resize`: a drag
    /// whose answers come behind its next sizes asks once, at its end, rather than at every
    /// size, FLOWS.md's resize flow, What could go 9). Asked on the GUI thread, which sends
    /// every Resize, so none is sent between the answer and the count
    pub fn answered(self) -> bool {
        let count = Arc::clone(&self.0);
        drop(self);
        count.load(Ordering::SeqCst) == 0
    }
}

impl Unanswered {
    /// A Resize is being sent: keep what this gives until its answer comes
    pub fn sent(&self) -> Sent {
        self.0.fetch_add(1, Ordering::SeqCst);
        Sent(Arc::clone(&self.0))
    }

    /// Whether any Resize sent hasn't been answered
    pub fn any(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }
}

/// Whether a pane of `tab` has a Resize the server hasn't answered: a listing of the server's
/// meanwhile can be older than the size the window gave the tab
pub fn tab_unanswered(tab: &Tab) -> bool {
    tab.iter_panes_ignoring_zoom().iter().any(|p| {
        p.pane
            .downcast_ref::<super::ClientPane>()
            .map(|pane| pane.lbterm_resize_unanswered())
            .unwrap_or(false)
    })
}

/// What a resync does with a tab the GUI had at `local`, which the server listed at `server`,
/// a pane of it with a Resize out or not (`unanswered`): whether it puts `local` back (without
/// sending it), or takes the server's
pub fn keeps_its_own(local: &TerminalSize, server: &TerminalSize, unanswered: bool) -> bool {
    unanswered && local != server
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_inside_from_server() {
        assert!(!is_from_server());
        from_server(|| {
            assert!(is_from_server());
            from_server(|| assert!(is_from_server()));
            assert!(is_from_server(), "a nested one leaves the outer one as it was");
        });
        assert!(!is_from_server());
    }

    #[test]
    fn put_back_when_it_panics() {
        let r = std::panic::catch_unwind(|| from_server(|| panic!("in a sync")));
        assert!(r.is_err());
        assert!(!is_from_server());
    }

    #[test]
    fn a_resize_is_unanswered_until_its_answer_comes() {
        let out = Unanswered::default();
        assert!(!out.any());
        let first = out.sent();
        let second = out.clone().sent();
        assert!(out.any(), "two sent");
        drop(first);
        assert!(out.any(), "one still out");
        drop(second);
        assert!(!out.any(), "both answered");
    }

    #[test]
    fn only_the_last_resize_answered_says_so() {
        let out = Unanswered::default();
        let first = out.sent();
        let second = out.sent();
        assert!(!first.answered(), "the second is still out: it asks when answered");
        assert!(second.answered(), "the last answered asks");
        assert!(!out.any());
        // Answered out of their order, the last answered still asks
        let first = out.sent();
        let second = out.sent();
        assert!(!second.answered());
        assert!(first.answered());
    }

    #[test]
    fn the_windows_size_is_kept_only_while_its_resize_is_out() {
        let size = |cols| TerminalSize {
            rows: 50,
            cols,
            pixel_width: cols * 9,
            pixel_height: 1000,
            dpi: 96,
        };
        assert!(
            keeps_its_own(&size(120), &size(119), true),
            "a listing older than the Resize out: the window's size stays"
        );
        assert!(
            !keeps_its_own(&size(120), &size(100), false),
            "nothing out: the server's size, another window's perhaps, is the tab's"
        );
        assert!(!keeps_its_own(&size(120), &size(120), true), "the same: nothing to do");
    }
}
