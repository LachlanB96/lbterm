//! LBTerm: a read of a mux client pane that must see it as its program has it, when its server
//! holds its output (Lachlan's 6b of 2026-11-08; lbterm/MUX.md, "What reads a held pane's lines").
//!
//! A server of perf/hidden-output's build holds a pane's lines while no window shows it, and tells
//! the window so (PaneHeld): the window's copy is the pane as last pushed until the push that ends
//! the hold. A reader that must be current (Lua's `pane:get_lines_as_text()` and the other reads,
//! `cli get-text` through the window's own socket, a voice draft sent to a pane out of view)
//! asks the server for the pane's push first: GetPaneRenderChanges, as the poll does, which the
//! server answers with the push and then its LivenessResponse, in the connection's one order, so
//! when the answer comes the push is in the connection's backlog already. The reader applies the
//! pane's pushes from there itself ([`crate::client::Client::lbterm_apply_handed`]), which ends
//! the hold, and reads. A reader in a task awaits the answer ([`Ask::wait`]); Lua's methods,
//! synchronous on the GUI thread, wait for it there (`ClientPane::lbterm_current_now`).
//!
//! The wait is bounded by config's `HELD_READ_WAIT` from the ask, and kept to one bound however
//! many reads there are: while the connection is down nothing is asked (no answer could come
//! before the reconnect, PITFALLS 172), and an ask that runs out its bound makes the connection
//! late ([`Reads`]) until its answer comes, while no read asks.

use crate::lbterm_backlog::Backlog;
use codec::Pdu;
use mux::pane::PaneId;
use smol::channel::Receiver;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a connection's reads share with its client thread: the backlog the thread hands the GUI
/// thread its PDUs through, a new one each connection, and whether an ask has run out its bound
/// with no answer yet
#[derive(Default)]
pub struct Reads {
    backlog: Mutex<Option<Arc<Backlog>>>,
    late: AtomicBool,
}

impl Reads {
    /// The client thread is on a new connection, handing over through `backlog`: no ask on the
    /// last can be answered now, so none is late
    pub fn connected(&self, backlog: &Arc<Backlog>) {
        *self.backlog.lock().unwrap() = Some(Arc::clone(backlog));
        self.late.store(false, Ordering::Release);
    }

    /// The connection's backlog, while it has one
    pub fn backlog(&self) -> Option<Arc<Backlog>> {
        self.backlog.lock().unwrap().clone()
    }

    /// The answer to a read's ask came (the client thread, [`crate::lbterm_backlog::Input::answered`])
    pub fn answered(&self) {
        self.late.store(false, Ordering::Release);
    }

    /// A read's ask ran out its bound unanswered
    pub fn ran_out(&self) {
        self.late.store(true, Ordering::Release);
    }

    /// Whether an ask has run out its bound and its answer hasn't come
    pub fn is_late(&self) -> bool {
        self.late.load(Ordering::Acquire)
    }
}

/// Why a read reads what the window has without asking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The pane isn't held: what the window has is the pane's
    Current,
    /// The connection is down: nothing could answer before the reconnect
    Down,
    /// An ask on this connection ran out its bound and hasn't been answered
    Late,
}

/// What a read does first
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Read what the window has
    Read(Why),
    /// Ask the server for the pane's push, and wait for it up to the bound
    Ask,
}

/// What a read of a pane does first: `held` whether its server holds it (PaneHeld), `down` whether
/// the connection is down, `late` whether an ask has run out its bound unanswered
pub fn plan(held: bool, down: bool, late: bool) -> Plan {
    if !held {
        Plan::Read(Why::Current)
    } else if down {
        Plan::Read(Why::Down)
    } else if late {
        Plan::Read(Why::Late)
    } else {
        Plan::Ask
    }
}

/// How an ask ended
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The push came and is applied: the hold is over
    Brought,
    /// The answer came and the pane is still held: its push wasn't applied with it (one with
    /// images is applied once they're fetched, in a task of its own)
    StillHeld,
    /// The server answered with an error, or the connection ended first
    Failed,
    /// The bound came first: the connection is late until the answer comes
    RanOut,
}

impl Ended {
    pub fn says(self) -> &'static str {
        match self {
            Ended::Brought => "its push brought",
            Ended::StillHeld => "answered, still held",
            Ended::Failed => "the ask failed",
            Ended::RanOut => "no answer within the bound",
        }
    }
}

/// How an ask ended: `answer` None when the bound came first, else whether the server answered
/// it; `held` whether the pane is held once its pushes handed over are applied
pub fn ended(answer: Option<bool>, held: bool) -> Ended {
    match answer {
        None => Ended::RanOut,
        Some(false) => Ended::Failed,
        Some(true) if held => Ended::StillHeld,
        Some(true) => Ended::Brought,
    }
}

/// A held pane's push asked of its server, for a read to wait on
pub struct Ask {
    pub(crate) client: crate::client::Client,
    pub(crate) remote_pane_id: PaneId,
    pub(crate) local_pane_id: PaneId,
    /// The pane's hold (`RenderableInner::lbterm_held`): nonzero while held
    pub(crate) held: Arc<AtomicU64>,
    pub(crate) answer: Receiver<anyhow::Result<Pdu>>,
    pub(crate) asked: Instant,
    pub(crate) bound: Duration,
}

impl Ask {
    /// Waits for the answer, up to the bound from the ask, then applies the pane's pushes the
    /// client thread has handed over: the hold ends with its push. Logged at info, once a read
    /// that asked (perf/held-reads.ps1 reads it)
    pub async fn wait(self) {
        let deadline = self.asked + self.bound;
        let answer = smol::future::or(
            // An ErrorResponse is an answer that failed: the pane gone, say
            async {
                Some(matches!(
                    self.answer.recv().await,
                    Ok(Ok(ref pdu)) if !matches!(pdu, Pdu::ErrorResponse(_))
                ))
            },
            async {
                async_io::Timer::at(deadline).await;
                None
            },
        )
        .await;
        if answer.is_some() {
            self.client.lbterm_apply_handed(self.remote_pane_id);
        }
        let how = ended(answer, self.held.load(Ordering::Acquire) != 0);
        if how == Ended::RanOut {
            self.client.lbterm_reads().ran_out();
            // The answer's receiver is kept until the answer comes: the client thread ends the
            // connection when an answer finds nobody to take it (client.rs's ClientWasDestroyed,
            // PITFALLS 224), and the client thread clears the late it says meanwhile
            let answer = self.answer;
            promise::spawn::spawn(async move {
                let _ = answer.recv().await;
            })
            .detach();
        }
        log::info!(
            target: "lbterm::shown",
            "a read of held pane {} (the server's {}): {} in {:.2} ms",
            self.local_pane_id,
            self.remote_pane_id,
            how.says(),
            self.asked.elapsed().as_secs_f64() * 1000.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_held_pane_asks_and_only_while_the_connection_can_answer() {
        assert_eq!(plan(false, false, false), Plan::Read(Why::Current));
        // Not held: nothing to ask whatever the connection is doing
        assert_eq!(plan(false, true, true), Plan::Read(Why::Current));
        assert_eq!(plan(true, false, false), Plan::Ask);
        assert_eq!(plan(true, true, false), Plan::Read(Why::Down));
        assert_eq!(plan(true, false, true), Plan::Read(Why::Late));
        // Down comes first: a late connection that's gone down is down
        assert_eq!(plan(true, true, true), Plan::Read(Why::Down));
    }

    #[test]
    fn an_ask_ends_by_its_answer_or_its_bound() {
        assert_eq!(ended(Some(true), false), Ended::Brought);
        assert_eq!(ended(Some(true), true), Ended::StillHeld);
        assert_eq!(ended(Some(false), true), Ended::Failed);
        assert_eq!(ended(None, true), Ended::RanOut);
        assert_eq!(ended(None, false), Ended::RanOut);
    }

    #[test]
    fn late_from_a_bound_run_out_until_an_answer_or_a_new_connection() {
        let reads = Reads::default();
        assert!(!reads.is_late());
        assert!(reads.backlog().is_none());
        reads.ran_out();
        assert!(reads.is_late());
        reads.answered();
        assert!(!reads.is_late());
        reads.ran_out();
        let backlog = Backlog::new();
        reads.connected(&backlog);
        assert!(!reads.is_late(), "a new connection answers nothing of the last");
        assert!(Arc::ptr_eq(&reads.backlog().unwrap(), &backlog));
    }

    /// The bound: what Lua's read holds the GUI thread for at most is the switch's
    #[test]
    fn the_bound_is_the_switchs() {
        assert_eq!(config::lbterm::HELD_READ_WAIT, config::lbterm::SWITCH_WAIT);
    }
}
