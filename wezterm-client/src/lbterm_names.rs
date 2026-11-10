//! LBTerm: a push of a pane's title and folder alone, applied on the connection's client thread
//! as it's read, with nothing handed to the GUI thread (Lachlan's 1b of 2026-11-09; lbterm/FLOWS.md,
//! "A title out of view", rows t1 to t7 and What could go 13, part 1).
//!
//! A working Claude Code session retitles its tab every 960 ms (◐ and ◑ before its words). For a
//! pane its window doesn't show, the server sends the title as a names push: a
//! GetPaneRenderChangesResponse with no lines, no rows named and no key's serial, the cursor, size
//! and flags as its last push had them, and the seqno of its last push or, when the pane printed
//! nothing a push compares since, of the server's read of it (wezterm-mux-server-impl's
//! sessionhandler.rs, `lbterm_out_of_view`), then the alert. Handed over like any push, it cost
//! the GUI thread a take, a wake and its apply (run 16: the take's task 37 to 40 µs, two wakes),
//! and its apply's PaneOutput, which for a pane out of view only tells the window's subscription
//! news it takes nothing from (hidden_scan finds no rows moved; the budget's news excuses work
//! that doesn't run).
//!
//! Now [`applied_here`] applies it where it's decoded, when:
//!
//! - it's a names push ([`alone`]);
//! - nothing of its pane is handed over and not yet applied ([`Backlog::waits`]: taken or not, or
//!   applied by a task that went on, an image's or a resync's), so nothing older of the pane can be
//!   applied after it and put an older title back. The client thread alone hands anything over,
//!   so a no stays a no while it applies this;
//! - its pane is a ClientPane of this domain that the window knows already (a pane it doesn't is
//!   the GUI thread's to resync);
//! - it changes nothing else the pane holds ([`current`]): the cursor and the size as last applied
//!   (a local resize leaves the size otherwise until the server's push), the mouse and screen
//!   flags, a seqno not behind the last applied, no hold it would end (a hold's end is counted for
//!   hidden_scan, which takes it in the PaneOutput this doesn't send), and no local echo guessing
//!   (whose pushes notify the window whatever they carry).
//!
//! Then the pane's title, folder and seqno are set under its renderable's lock, where
//! `get_title` and `get_current_working_dir` read them (PITFALLS 163), `Drawn::applied` notes it
//! for the next frame that reads the pane to say (lbterm_drawn.rs; the server waits on no word for
//! a names push), and nothing is handed: no take, no wake, no PaneOutput. Anything else goes as
//! every push does. The alert the server sends after it is handed over as before, so a window
//! acts on it with the title in place, and a connection to the GUI's own socket (a cli) hears of
//! it there: its subscription owes the pane's changes for a title alert as it would for the
//! PaneOutput, and reads the title stored here.
//!
//! What it costs the client thread: the lookups (the mux's and the domain's maps, each a lock),
//! the backlog's mutex, and the pane's renderable lock, which the GUI thread may hold while it
//! reads the pane (a Lua read, hidden_scan, the rail's `get_title`): the client thread waits that
//! long before it reads on, a key's echo with it.

use crate::domain::ClientDomain;
use crate::lbterm_backlog::Backlog;
use crate::pane::ClientPane;
use codec::{GetPaneRenderChangesResponse, Pdu};
use mux::domain::DomainId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::Mux;
use termwiz::surface::SequenceNo;

/// Whether `push` carries the pane's title and folder alone: no line, no row named, and no key's
/// serial (a key's answer, which the window times its round trip and places its cursor by)
pub fn alone(push: &GetPaneRenderChangesResponse) -> bool {
    push.bonus_lines.lbterm_is_empty() && push.dirty_lines.is_empty() && push.input_serial.is_none()
}

/// Whether `push` changes nothing a pane holds but its title, folder and seqno: the cursor and
/// size as last applied, a seqno not behind the last applied (`seqno`), and no hold of the
/// pane's ended by it (`held`, the renderable's `lbterm_held`: the seqno the hold was said from
/// plus one, or 0)
pub fn current(
    push: &GetPaneRenderChangesResponse,
    seqno: SequenceNo,
    cursor: &StableCursorPosition,
    dimensions: &RenderableDimensions,
    held: u64,
) -> bool {
    push.seqno >= seqno
        && push.cursor_position == *cursor
        && push.dimensions == *dimensions
        && !(held != 0 && push.seqno as u64 >= held)
}

/// `pdu` applied here on the client thread when it's a push of a pane's title and folder alone
/// that nothing older of the pane waits ahead of and that changes nothing else (the module's
/// doc). Whether it was; when not, the caller hands it over as any other
pub fn applied_here(local_domain_id: DomainId, pdu: &Pdu, backlog: &Backlog) -> bool {
    let push = match pdu {
        Pdu::GetPaneRenderChangesResponse(push) if alone(push) => push,
        _ => return false,
    };
    if backlog.waits(push.pane_id) {
        return false;
    }
    // What it costs the client thread, said on its line while perf/title-storm.ps1 counts
    let started = log::log_enabled!(target: "lbterm::title", log::Level::Debug)
        .then(std::time::Instant::now);
    let Some(mux) = Mux::try_get() else {
        return false;
    };
    let Some(domain) = mux.get_domain(local_domain_id) else {
        return false;
    };
    let Some(domain) = domain.downcast_ref::<ClientDomain>() else {
        return false;
    };
    let Some(local_pane_id) = domain.remote_to_local_pane_id(push.pane_id) else {
        return false;
    };
    let Some(pane) = mux.get_pane(local_pane_id) else {
        return false;
    };
    let Some(pane) = pane.downcast_ref::<ClientPane>() else {
        return false;
    };
    let applied = pane.lbterm_names_here(push);
    log::debug!(
        target: "lbterm::title",
        "names push pane {local_pane_id} (server's {}) seqno {} in {} us: {}",
        push.pane_id,
        push.seqno,
        started.map_or(0, |started| started.elapsed().as_micros()),
        if applied {
            "applied on the client thread"
        } else {
            "changes more than its names: to the GUI thread"
        }
    );
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::{DecodedPdu, InputSerial, NotifyAlert, PaneHeld};
    use termwiz::cell::CellAttributes;
    use wezterm_term::Line;

    fn names(pane_id: mux::pane::PaneId, seqno: SequenceNo, title: &str) -> GetPaneRenderChangesResponse {
        GetPaneRenderChangesResponse {
            pane_id,
            mouse_grabbed: false,
            alt_screen_active: false,
            cursor_position: StableCursorPosition::default(),
            dimensions: RenderableDimensions::default(),
            dirty_lines: vec![],
            title: title.to_string(),
            working_dir: None,
            bonus_lines: vec![].into(),
            input_serial: None,
            seqno,
        }
    }

    fn decoded(pdu: Pdu) -> DecodedPdu {
        DecodedPdu { serial: 0, pdu }
    }

    #[test]
    fn a_names_push_has_no_line_no_row_and_no_keys_serial() {
        assert!(alone(&names(1, 5, "◐ claude")));
        let mut rows = names(1, 5, "◐ claude");
        rows.dirty_lines = vec![3..4];
        assert!(!alone(&rows), "a row named");
        let mut lines = names(1, 5, "◐ claude");
        lines.bonus_lines = vec![(3, Line::from_text("x", &CellAttributes::default(), 1, None))].into();
        assert!(!alone(&lines), "a line carried");
        let mut key = names(1, 5, "◐ claude");
        key.input_serial = Some(InputSerial::empty());
        assert!(!alone(&key), "a key's answer");
    }

    #[test]
    fn it_changes_nothing_else_the_pane_holds() {
        let cursor = StableCursorPosition::default();
        let dims = RenderableDimensions::default();
        assert!(current(&names(1, 5, "t"), 5, &cursor, &dims, 0), "its last push's seqno");
        assert!(
            current(&names(1, 9, "t"), 5, &cursor, &dims, 0),
            "the server's read of a pane that printed nothing a push compares"
        );
        assert!(!current(&names(1, 4, "t"), 5, &cursor, &dims, 0), "behind the last applied");

        let mut moved = names(1, 5, "t");
        moved.cursor_position.y = 7;
        assert!(!current(&moved, 5, &cursor, &dims, 0), "the cursor elsewhere");
        let mut resized = names(1, 5, "t");
        resized.dimensions.cols = 120;
        assert!(!current(&resized, 5, &cursor, &dims, 0), "a size the window doesn't have");

        // Held from seqno 5 (said as 6): a push at 5 leaves it, one past it ends it, which only
        // the GUI thread's apply tells hidden_scan of
        assert!(current(&names(1, 5, "t"), 5, &cursor, &dims, 6), "the hold's own seqno");
        assert!(!current(&names(1, 6, "t"), 5, &cursor, &dims, 6), "a push that ends the hold");
    }

    #[test]
    fn nothing_of_the_pane_waits_ahead_of_it() {
        let backlog = Backlog::new();
        assert!(!backlog.waits(1));

        // A push of pane 1 handed over: it waits until the GUI thread has applied it, taken or not
        backlog.hand(decoded(Pdu::GetPaneRenderChangesResponse(names(1, 4, "◐ a"))), None);
        assert!(backlog.waits(1));
        assert!(!backlog.waits(2), "another pane's names push needn't wait for it");
        let taken = backlog.take();
        assert!(backlog.waits(1), "taken, not yet applied: its older title would come after");
        drop(taken);
        assert!(!backlog.waits(1), "applied");

        // So does its hold's word, and an alert
        backlog.hand(decoded(Pdu::PaneHeld(PaneHeld { pane_id: 1, seqno: 4 })), None);
        backlog.hand(
            decoded(Pdu::NotifyAlert(NotifyAlert {
                pane_id: 1,
                alert: wezterm_term::Alert::WindowTitleChanged("◐ a".into()),
            })),
            None,
        );
        let mut taken = backlog.take();
        let alert = taken.pop().unwrap();
        drop(taken);
        assert!(backlog.waits(1), "the alert's still to be applied");
        drop(alert);
        assert!(!backlog.waits(1));
        assert_eq!(backlog.unapplied(), 0);
    }

    #[test]
    fn a_push_applied_by_a_task_that_goes_on_keeps_its_pane_waiting_until_it_ends() {
        // A push with an image's data to fetch, or of a pane to resync, counts as applied so the
        // client thread reads on for its answer; its pane waits until its task has applied it
        let backlog = Backlog::new();
        backlog.hand(decoded(Pdu::GetPaneRenderChangesResponse(names(1, 4, "◐ a"))), None);
        let (_, mut applied) = backlog.take().pop().unwrap();
        let waits = applied.goes_on();
        drop(applied);
        assert_eq!(backlog.unapplied(), 0, "the client thread reads on");
        assert!(backlog.waits(1), "the task hasn't applied it yet");
        drop(waits);
        assert!(!backlog.waits(1));
    }

    #[test]
    fn merged_pushes_of_a_pane_wait_as_many() {
        let backlog = Backlog::new();
        for seqno in 1..=3 {
            backlog.hand(decoded(Pdu::GetPaneRenderChangesResponse(names(1, seqno, "t"))), None);
        }
        let taken = backlog.take();
        assert_eq!(taken.len(), 1, "merged into one");
        assert_eq!(taken[0].1.count(), 3);
        assert!(backlog.waits(1));
        drop(taken);
        assert!(!backlog.waits(1));
    }
}
