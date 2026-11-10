//! LBTerm: whether a client's connection to its mux server is up, and what a write to a pane
//! comes to.
//!
//! A window writes to a mux pane, rather than sending it a key, for a binding's `SendString`, a
//! voice draft, the scroll strip's keys, an IME's or the char selector's text, a key in
//! win32-input or kitty mode and Lua's `pane:send_text` (the pane's `writer()`, clientpane.rs's
//! `PaneWriter`). Since perf/mux-client it's sent from a task, behind the keys still waiting in
//! theirs (lbterm/PITFALLS.md, 137), and not waited for. One that fails is said where Lachlan
//! looks, the pane's scroll strip (Lachlan's 15b of 2026-11-07), not only in the log: the task
//! hears the server's answer ([`Failure::of`]) and tells the GUI (`mux::lbterm_writes`).
//!
//! While the connection is down every request waits for it, a write as a key does. A unix domain
//! whose server hangs up connects again a second apart, doubling to 10 s, eight times before it
//! gives up (client.rs, `after_disconnect`): about a minute in which its client thread reads no
//! request, so whatever is asked meanwhile waits in the client's channel, in its order, and goes
//! once a connection is made. Keys, pastes, mouse reports and writes all land in the same input
//! of the pane's program, so they share one fate (lbterm/PITFALLS.md, 178): a write that failed
//! at once while the keys round it waited would reach the program as a hole in what Lachlan
//! typed (Shift+Enter, a `SendString`, between letters that arrive after the reconnect: the
//! newline gone). So nothing fails for being made while the link is down. A write fails when the
//! server refuses it ([`Failure::Refused`]), when the connection ends before the server answers
//! ([`Failure::Lost`]: it may have arrived), or when the client gives up and it never went
//! ([`GaveUp`], [`Failure::NotSent`]: the domain detaches then and its tabs leave the window, so
//! that one is only logged). The window isn't held meanwhile: upstream's `PaneWriter` waited on
//! the GUI thread for the answer, and so for the whole reconnect.
//!
//! The scroll strip's seek is the one thing that asks first ([`Link::is_down`], through
//! `ClientPane::lbterm_link_down`): it steers by what each burst of keys brought, which nothing
//! brings while the link is down, so it isn't started then, and says why ([`Down`]), rather than
//! queue bursts that would scroll the conversation a minute later.
//!
//! The client thread marks the link down as its connection ends ([`Link::went_down`]) and up
//! once it has connected again ([`Link::came_up`]). Nothing polls or times out: dropping a
//! request's answer before it comes makes the client thread take itself for destroyed and detach
//! the domain (client.rs, the promise's `try_send`; lbterm/PITFALLS.md, 171).

use codec::{ErrorResponse, Pdu};
use std::sync::atomic::{AtomicBool, Ordering};

/// Why the scroll strip doesn't seek in a pane whose connection is down
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[error("the mux server isn't answering: the window is connecting to it again")]
pub struct Down;

/// A request that waited for a connection the client stopped trying to make: never sent
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[error("the window stopped trying to reach the mux server, so it wasn't sent")]
pub struct GaveUp;

/// Why a write to a pane didn't get there
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The client gave up reconnecting before it went: not sent
    NotSent,
    /// The server answered with an error (no such pane, its program's pipe gone)
    Refused(String),
    /// The connection ended before the server answered: it may or may not have arrived
    Lost(String),
}

impl Failure {
    /// What a write's answer says, `None` when it got there
    pub fn of(answer: anyhow::Result<Pdu>) -> Option<Failure> {
        match answer {
            Ok(Pdu::UnitResponse(_)) => None,
            Ok(Pdu::ErrorResponse(ErrorResponse { reason })) => Some(Failure::Refused(reason)),
            Ok(other) => Some(Failure::Refused(format!("it answered {}", other.pdu_name()))),
            Err(err) if err.downcast_ref::<GaveUp>().is_some() => Some(Failure::NotSent),
            Err(err) => Some(Failure::Lost(format!("{err:#}"))),
        }
    }

    /// Whether it may have reached the pane anyway, which the strip's head says (Lachlan's 4a of
    /// 2026-11-08): only one the connection lost before the server answered
    pub fn reached(&self) -> mux::lbterm_writes::Reached {
        match self {
            Failure::Lost(_) => mux::lbterm_writes::Reached::Maybe,
            Failure::NotSent | Failure::Refused(_) => mux::lbterm_writes::Reached::No,
        }
    }
}

/// What the scroll strip says: the cause, for Lachlan, not the log's detail
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Failure::NotSent => write!(f, "{}", GaveUp),
            Failure::Refused(reason) => write!(f, "the mux server refused it: {reason}"),
            // The head says it may not have arrived ("May not have arrived"): the text says why
            Failure::Lost(_) => write!(f, "the connection to the mux server ended before it answered"),
        }
    }
}

/// Whether a client's connection is up, shared by the client and its thread
#[derive(Debug, Default)]
pub struct Link {
    down: AtomicBool,
}

impl Link {
    pub fn is_down(&self) -> bool {
        self.down.load(Ordering::Relaxed)
    }

    /// The connection has ended: requests wait in the client's channel for the next
    pub fn went_down(&self) {
        self.down.store(true, Ordering::Relaxed);
    }

    /// Connected again: what waited goes first, in its order
    pub fn came_up(&self) {
        self.down.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use codec::{UnitResponse, WriteToPane};

    fn write(data: &str) -> Pdu {
        Pdu::WriteToPane(WriteToPane {
            pane_id: 1,
            data: data.as_bytes().to_vec(),
        })
    }

    #[test]
    fn the_link_says_whether_its_connection_is_down() {
        let link = Link::default();
        assert!(!link.is_down());
        link.went_down();
        assert!(link.is_down());
        link.came_up();
        assert!(!link.is_down());
    }

    #[test]
    fn what_an_answer_says() {
        assert_eq!(Failure::of(Ok(Pdu::UnitResponse(UnitResponse {}))), None);
        assert_eq!(
            Failure::of(Ok(Pdu::ErrorResponse(ErrorResponse {
                reason: "no such pane 7".into()
            }))),
            Some(Failure::Refused("no such pane 7".into()))
        );
        assert_eq!(Failure::of(Err(anyhow::Error::new(GaveUp))), Some(Failure::NotSent));
        // GaveUp as the cause of a context's error, as send_pdu's are
        let wrapped: anyhow::Result<Pdu> = Err(anyhow::Error::new(GaveUp).context("send_pdu recv"));
        assert_eq!(Failure::of(wrapped), Some(Failure::NotSent));
        match Failure::of(Err(anyhow::anyhow!("Client was destroyed"))) {
            Some(Failure::Lost(why)) => assert!(why.contains("destroyed"), "{}", why),
            other => panic!("{:?}", other),
        }
        match Failure::of(Ok(write("x"))) {
            Some(Failure::Refused(why)) => assert!(why.contains("WriteToPane"), "{}", why),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn what_the_strip_says() {
        assert!(Down.to_string().contains("connecting to it again"));
        assert!(Failure::NotSent.to_string().contains("wasn't sent"));
        assert_eq!(Failure::Refused("no such pane 7".into()).to_string(), "the mux server refused it: no such pane 7");
        // The log has the detail; the strip says what it means
        assert!(!Failure::Lost("send_pdu recv: closed".into()).to_string().contains("send_pdu"));
    }

    #[test]
    fn only_a_lost_write_may_have_arrived() {
        use mux::lbterm_writes::Reached;
        assert_eq!(Failure::Lost("closed".into()).reached(), Reached::Maybe);
        assert_eq!(Failure::NotSent.reached(), Reached::No);
        assert_eq!(Failure::Refused("no such pane 7".into()).reached(), Reached::No);
    }
}
