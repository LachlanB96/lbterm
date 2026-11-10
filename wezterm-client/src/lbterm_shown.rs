//! LBTerm: what a mux domain's windows show, said to its server (codec's `SetShownPanes`), so a
//! server of this build holds the lines of every pane they don't show until it's shown, quiet or
//! asked for (lbterm/MUX.md, "Tabs out of view: output held at the server").
//!
//! The set is every pane of this domain in the active tab of each window of the GUI's active
//! workspace (a zoomed tab's zoomed pane alone), as the server numbers them, worked out on the GUI
//! thread by the task the domain's mux subscriber queues on the notifications that change what a
//! window shows (domain.rs, `lbterm_mark_shown`), and sent only when it differs from the last one
//! sent on this connection. Each connection starts at "not said" on the server, so a connect or a
//! reconnect says it again whatever was said on the last ([`Shown::connected`]).
//!
//! A server that doesn't know the PDU (an older build) answers ErrorResponse naming it
//! (`codec::lacks_shown_panes`): nothing more is said on that connection, and it pushes every pane
//! as it always did. Any other failure (the connection lost) forgets what was said, so the next
//! change, or the next connection, says it whole. The rpc's answer is always waited for: a
//! dropped receiver would detach the domain (lbterm/PITFALLS.md, 171).

use codec::{SetShownPanes, ShownPane};
use mux::pane::PaneId;
use std::sync::Mutex;

/// What the server on this connection makes of SetShownPanes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Server {
    /// Not asked yet on this connection
    Unknown,
    /// It answered one: it holds what isn't shown
    Knows,
    /// An older build: it pushes every pane, and is told nothing more
    Lacks,
}

#[derive(Debug)]
struct State {
    server: Server,
    /// The set last sent on this connection, sorted; None before the first, or after a send
    /// failed for some reason other than the server not knowing the PDU
    said: Option<Vec<PaneId>>,
    /// A GUI-thread task to say the set is queued and hasn't run
    queued: bool,
    /// Which connection this is, counted from the first: an answer to a request sent on an
    /// earlier one changes nothing here
    connection: u64,
}

/// What a domain's client has said its windows show, kept in its `ClientInner`
#[derive(Debug)]
pub struct Shown(Mutex<State>);

impl Default for Shown {
    fn default() -> Self {
        Self(Mutex::new(State {
            server: Server::Unknown,
            said: None,
            queued: false,
            connection: 0,
        }))
    }
}

/// A set to send: the PDU, and the connection it's for
#[derive(Debug, PartialEq)]
pub struct Say {
    pub pdu: SetShownPanes,
    pub connection: u64,
}

impl Shown {
    /// Something that may change what a window shows happened: whether to queue the task that
    /// says it ([`Shown::to_say`]). Once until it runs, so a burst of notifications (a resync's
    /// tabs, a window's resize) is one task
    pub fn mark(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        if state.queued || state.server == Server::Lacks {
            return false;
        }
        state.queued = true;
        true
    }

    /// The task runs, the windows showing `panes` (the server's ids, any order): what to send,
    /// when it's news to the server
    pub fn to_say(&self, mut panes: Vec<PaneId>) -> Option<Say> {
        panes.sort_unstable();
        panes.dedup();
        let mut state = self.0.lock().unwrap();
        state.queued = false;
        if state.server == Server::Lacks || state.said.as_ref() == Some(&panes) {
            return None;
        }
        let pdu = SetShownPanes {
            panes: panes
                .iter()
                .map(|&pane_id| ShownPane {
                    pane_id,
                    drawn: None,
                })
                .collect(),
        };
        state.said = Some(panes);
        Some(Say {
            pdu,
            connection: state.connection,
        })
    }

    /// The server's answer to the set sent on `connection`: `Err` with the error as `{:#}` shows
    /// it
    pub fn answered(&self, connection: u64, answer: Result<(), String>) {
        let mut state = self.0.lock().unwrap();
        if connection != state.connection {
            return;
        }
        match answer {
            Ok(()) => state.server = Server::Knows,
            Err(error) if codec::lacks_shown_panes(&error) => {
                log::info!(
                    target: "lbterm::shown",
                    "the mux server doesn't know SetShownPanes (an older build): it pushes every pane"
                );
                state.server = Server::Lacks;
            }
            Err(error) => {
                log::debug!(target: "lbterm::shown", "saying what the windows show: {error}");
                state.said = None;
            }
        }
    }

    /// A new connection to the server (a reconnect): nothing is said on it yet, and its server
    /// may be another build
    pub fn connected(&self) {
        let mut state = self.0.lock().unwrap();
        state.connection += 1;
        state.server = Server::Unknown;
        state.said = None;
    }

    /// Whether this connection's server has said it doesn't know SetShownPanes
    pub fn lacks(&self) -> bool {
        self.0.lock().unwrap().server == Server::Lacks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(say: &Option<Say>) -> Option<Vec<PaneId>> {
        say.as_ref()
            .map(|s| s.pdu.panes.iter().map(|p| p.pane_id).collect())
    }

    #[test]
    fn a_burst_of_notifications_queues_one_task() {
        let shown = Shown::default();
        assert!(shown.mark());
        assert!(!shown.mark());
        assert!(!shown.mark());
        shown.to_say(vec![1]);
        assert!(shown.mark(), "the task ran: the next change queues another");
    }

    #[test]
    fn the_set_goes_only_when_it_changed_whole_and_sorted() {
        let shown = Shown::default();
        shown.mark();
        let first = shown.to_say(vec![5, 2, 5]);
        assert_eq!(ids(&first), Some(vec![2, 5]));
        assert!(
            first.unwrap().pdu.panes.iter().all(|p| p.drawn.is_none()),
            "nothing says what a frame drew yet (What could go 12)"
        );
        shown.answered(0, Ok(()));
        shown.mark();
        assert_eq!(shown.to_say(vec![2, 5]), None, "the same set again tells it nothing");
        shown.mark();
        assert_eq!(ids(&shown.to_say(vec![2, 7])), Some(vec![2, 7]));
        shown.mark();
        assert_eq!(ids(&shown.to_say(vec![])), Some(vec![]), "a window showing none of its panes");
    }

    #[test]
    fn an_older_server_is_told_nothing_more() {
        let shown = Shown::default();
        shown.mark();
        let say = shown.to_say(vec![1]).unwrap();
        shown.answered(
            say.connection,
            Err(r#"unexpected response Ok(ErrorResponse(ErrorResponse { reason: "Error: Invalid { ident: 7505 }" }))"#.into()),
        );
        assert!(shown.lacks());
        assert!(!shown.mark(), "no task for a server that doesn't know it");
        assert_eq!(shown.to_say(vec![2]), None);
    }

    #[test]
    fn a_failed_send_forgets_what_was_said() {
        let shown = Shown::default();
        shown.mark();
        let say = shown.to_say(vec![1, 2]).unwrap();
        shown.answered(say.connection, Err("BrokenPromise: the client went away".into()));
        assert!(!shown.lacks());
        shown.mark();
        assert_eq!(ids(&shown.to_say(vec![1, 2])), Some(vec![1, 2]), "said again, the same set");
    }

    #[test]
    fn a_new_connection_is_told_again_and_its_server_asked_afresh() {
        let shown = Shown::default();
        shown.mark();
        let old = shown.to_say(vec![1]).unwrap();
        shown.answered(
            old.connection,
            Err("Invalid { ident: 7505 }".into()),
        );
        assert!(shown.lacks());
        // The server went and another, of this build, answers the reconnect
        shown.connected();
        assert!(!shown.lacks());
        assert!(shown.mark());
        let new = shown.to_say(vec![1]).unwrap();
        assert_ne!(new.connection, old.connection);
        // An answer to what was sent on the last connection changes nothing on this one
        shown.answered(old.connection, Err("BrokenPromise".into()));
        shown.answered(new.connection, Ok(()));
        shown.mark();
        assert_eq!(shown.to_say(vec![1]), None);
    }
}
