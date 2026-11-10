//! LBTerm: which push of each pane a window shows its frames have read, said to the server
//! (codec's `PanesDrawn`), so a server that paces a shown pane sends its next push once a frame
//! has read the last: one push travels while a frame draws (lbterm/MUX.md, "A shown pane's pace";
//! lbterm/FLOWS.md, pane output, What could go 12).
//!
//! Each client pane keeps a [`Drawn`]: the seqno of the newest push of it applied and whether
//! it has been said since, two atomics the renderable sets as it applies a push
//! (`RenderableInner::apply_changes_to_surface`, on the GUI thread, and
//! `RenderableInner::lbterm_names_here`, on the client thread for a push of a tab out of view's
//! title and folder alone: lbterm_names.rs) and that are read without its lock, so two threads
//! set and take them: [`Drawn::applied`] stores the seqno before it raises the flag (Release) and
//! [`Drawn::take`] lowers the flag with a swap before it reads the seqno (Acquire), so a push
//! applied as a frame takes the word is said by that take or the next, never lost (lbterm/
//! PITFALLS.md, 318). Whoever knows a frame has read the pane, or that no frame will read what came, takes it
//! ([`Drawn::take`]): a window's paint for each pane it drew (wezterm-gui's
//! termwindow/panes_drawn.rs), the window's output notice for a pane it shows that no frame will
//! draw soon (the Conductor's view over it, an overlay, a window that can't be seen), and the
//! renderable for a push that changes nothing the pane shows, which asks for no frame. A frame
//! with nothing new costs an atomic load a pane, and nothing is allocated until there's a push to
//! say.
//!
//! Every push applied is said, not only those whose seqno moved: a server's round push of a title
//! alone has the seqno of the push before it, and a pane it leaves waiting needs the word again
//! (`Pending::pushed_changes`, which a word at that seqno or later ends).
//!
//! A notice goes only where the server knows it ([`Gate`]): a server that doesn't answers it with
//! ErrorResponse at serial 0, and the client thread ends the connection on an answer it holds no
//! promise for (lbterm/PITFALLS.md, 227). The client thread opens the gate when the server
//! answers a SetShownPanes on this connection with UnitResponse (7505 and 7507 landed together)
//! and closes it at each connection's start, so a notice queued on one connection and read on the
//! next, whose server may be another build, is dropped there ([`GateWatch`]).

use codec::Pdu;
use termwiz::surface::SequenceNo;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The newest push of a client pane applied, and whether it's been said read since
#[derive(Debug, Default)]
pub struct Drawn {
    seqno: AtomicUsize,
    unsaid: AtomicBool,
}

impl Drawn {
    /// A push of the pane, with the server's `seqno`, applied: the next frame that reads the
    /// pane says it
    pub fn applied(&self, seqno: SequenceNo) {
        self.seqno.store(seqno, Ordering::Relaxed);
        // Release: a take that sees the flag reads this seqno or a newer one
        self.unsaid.store(true, Ordering::Release);
    }

    /// Whether a push applied hasn't been said since
    pub fn unsaid(&self) -> bool {
        self.unsaid.load(Ordering::Relaxed)
    }

    /// A frame has read the pane, or no frame will read what came: the seqno of the newest push
    /// applied, once a push. A frame with nothing new loads one atomic
    pub fn take(&self) -> Option<SequenceNo> {
        // A frame with nothing new: one load, no write
        if !self.unsaid.load(Ordering::Relaxed) {
            return None;
        }
        // A swap, not a store: the client thread can raise the flag between the load and it
        // (lbterm_names.rs), and a store of false after that would lose that push's word
        if !self.unsaid.swap(false, Ordering::AcqRel) {
            return None;
        }
        Some(self.seqno.load(Ordering::Acquire))
    }
}

/// Whether this connection's server knows PanesDrawn: the client thread's, read by the GUI
/// thread so it builds no notice for a server that would end the connection on it
#[derive(Debug, Default)]
pub struct Gate(AtomicBool);

impl Gate {
    pub fn is_open(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn set(&self, open: bool) {
        self.0.store(open, Ordering::Relaxed);
    }
}

/// What the client thread keeps of a connection for its [`Gate`]: the serial of the last
/// SetShownPanes it sent, whose answer opens it
#[derive(Debug)]
pub(crate) struct GateWatch<'a> {
    gate: &'a Gate,
    asked: Option<u64>,
}

impl<'a> GateWatch<'a> {
    /// A new connection: nothing known of its server, so the gate is shut until it answers
    pub(crate) fn connected(gate: &'a Gate) -> Self {
        gate.set(false);
        Self { gate, asked: None }
    }

    /// The client thread sent `pdu` at `serial`
    pub(crate) fn sent(&mut self, serial: u64, pdu: &Pdu) {
        if matches!(pdu, Pdu::SetShownPanes(_)) {
            self.asked = Some(serial);
        }
    }

    /// The server answered `serial` with `pdu`: a UnitResponse to the SetShownPanes opens the
    /// gate for the rest of the connection; any other answer leaves it as it was
    pub(crate) fn answered(&mut self, serial: u64, pdu: &Pdu) {
        if self.asked == Some(serial) {
            self.asked = None;
            if matches!(pdu, Pdu::UnitResponse(_)) {
                if !self.gate.is_open() {
                    log::debug!(
                        target: "lbterm::drawn",
                        "the mux server knows SetShownPanes: frames say what they read (PanesDrawn)"
                    );
                }
                self.gate.set(true);
            }
        }
    }

    /// Whether a notice goes on this connection: read by the client thread as it takes one, so
    /// one queued before a reconnect isn't sent to a server that hasn't answered yet
    pub(crate) fn open(&self) -> bool {
        self.gate.is_open()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::{ErrorResponse, SetShownPanes, UnitResponse};

    #[test]
    fn a_push_is_said_once_with_the_newest_seqno() {
        let drawn = Drawn::default();
        assert_eq!(drawn.take(), None, "nothing applied: nothing to say");
        drawn.applied(4);
        drawn.applied(7);
        assert!(drawn.unsaid());
        assert_eq!(drawn.take(), Some(7), "two pushes before a frame: the newest");
        assert_eq!(drawn.take(), None, "said: a frame with nothing new says nothing");
        assert!(!drawn.unsaid());
    }

    #[test]
    fn a_push_at_the_same_seqno_is_said_again() {
        // A round push of a title alone keeps the seqno of the push before it, and the server
        // leaves the pane waiting for a word at that seqno (Pending::pushed_changes)
        let drawn = Drawn::default();
        drawn.applied(9);
        assert_eq!(drawn.take(), Some(9));
        drawn.applied(9);
        assert_eq!(drawn.take(), Some(9));
    }

    fn shown() -> Pdu {
        Pdu::SetShownPanes(SetShownPanes { panes: vec![] })
    }

    #[test]
    fn the_gate_opens_at_the_answer_to_set_shown_panes_and_shuts_at_a_new_connection() {
        let gate = Gate::default();
        let mut watch = GateWatch::connected(&gate);
        assert!(!watch.open(), "a new connection: shut");
        watch.sent(3, &shown());
        watch.answered(2, &Pdu::UnitResponse(UnitResponse {}));
        assert!(!watch.open(), "another request's answer opens nothing");
        watch.answered(3, &Pdu::UnitResponse(UnitResponse {}));
        assert!(watch.open() && gate.is_open(), "the server knows SetShownPanes, so PanesDrawn");
        drop(watch);
        let watch = GateWatch::connected(&gate);
        assert!(!watch.open() && !gate.is_open(), "a reconnect: its server may be another build");
    }

    #[test]
    fn an_older_servers_error_keeps_the_gate_shut() {
        let gate = Gate::default();
        let mut watch = GateWatch::connected(&gate);
        watch.sent(1, &shown());
        watch.answered(
            1,
            &Pdu::ErrorResponse(ErrorResponse {
                reason: "Invalid".to_string(),
            }),
        );
        assert!(!watch.open(), "an older server answers ErrorResponse: told nothing more");
    }
}
