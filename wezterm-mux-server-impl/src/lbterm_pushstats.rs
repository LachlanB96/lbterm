//! LBTerm: what a connection's pushes cost, step by step, for tracing the pane-output flow
//! (lbterm/FLOWS.md, "Pane output on a mux domain", rows 25 to 38).
//!
//! Each push `SessionHandler::lbterm_push` works out for a pane it owes is timed in the [`Step`]s
//! below, with the rows it carried, the rows it named changed without carrying and the bytes it
//! encoded, kept a pane at a time for its connection, with the connection's rounds and what each
//! round waited for after. Once a second, a server a test window started logs them, one line a
//! connection that pushed anything, target `lbterm::push`, each pane marked `shown` when it's in
//! its mux window's active tab. The harness gives every test window `LBTERM_STATE_FILE`, which the
//! server it starts inherits with the rest of its environment; Lachlan's own server, started from
//! his window, has none and logs nothing. A server that doesn't log still reads the thread's
//! timing slot at each step, a thread-local's look; one that logs reads the clock eight times a
//! push, against the 0.4 to 0.6 ms a flooding pane's push takes (FLOWS.md, row 35).
//!
//! Keys' and polls' pushes (`process_one`'s arms, row 39) aren't timed: only the pushes owed.

use crate::lbterm_flow::Wait;
use mux::pane::PaneId;
use mux::Mux;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::time::{Duration, Instant};

/// The steps a push is timed in, each ending where the next starts
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// From the push asked for to the pane read: the pane found, the alerts noted, and
    /// `compute_changes`' read of it (rows 26 to 28), which since the flood build is one read
    /// under one terminal lock, the changed rows with it (mux/src/lbterm_push_read.rs)
    Reads,
    /// What was read compared with what was sent (until the flood build, `get_changed_since`
    /// under a lock of its own, row 28)
    Changed,
    /// `lines_to_send`: the lines copied out of the terminal and compressed (row 30)
    Copy,
    /// `SerializedLines::from` and the answer built: the lines turned back into cells (rows 31
    /// and 32)
    Cells,
    /// The push's encode into the outbox, zstd included (row 35)
    Encode,
    /// Everything after it: the config's generation, the alerts' PDUs encoded (rows 33 and 34)
    Rest,
}

pub const STEPS: usize = 6;
const NAMES: [&str; STEPS] = ["reads", "changed", "copy", "cells", "encode", "rest"];

/// What one pane's pushes came to
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    /// Pushes worked out: `lbterm_push` for a pane this connection gets
    pub asked: u64,
    /// Of those, owes of alerts alone, sent without the pane's changes worked out
    /// (lbterm_flow::Owed)
    pub alone: u64,
    /// Of those, the ones that found a change and sent a GetPaneRenderChangesResponse
    pub pushes: u64,
    /// Lines the pushes carried (bonus_lines)
    pub rows: u64,
    /// Rows they named changed without carrying them (dirty_lines, mostly scrolled past the top)
    pub stale: u64,
    /// PDUs encoded, the pushes' own and their alerts' (NotifyAlert, SetProgramStatus, SetPalette)
    pub pdus: u64,
    /// The bytes those PDUs came to in the outbox, as sent
    pub bytes: u64,
    /// Nanoseconds in each [`Step`]
    pub ns: [u64; STEPS],
    /// Of the owes of alerts alone, the words that a pane's lines are held sent (PaneHeld,
    /// lbterm_flow::Owed::held)
    pub held: u64,
    /// Of those, the pushes of a held pane's title and folder alone (lbterm_flow::Owed::names)
    pub names: u64,
    /// The times a shown pane's changes, owed, waited for the client's word that a frame read
    /// its last push (codec's PanesDrawn; lbterm/MUX.md, "A shown pane's pace"), and the
    /// microseconds from that push to the word, summed: each a push no frame would have drawn
    pub waited: u64,
    pub waited_us: u64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.asked += other.asked;
        self.alone += other.alone;
        self.pushes += other.pushes;
        self.rows += other.rows;
        self.stale += other.stale;
        self.pdus += other.pdus;
        self.bytes += other.bytes;
        for (a, b) in self.ns.iter_mut().zip(other.ns.iter()) {
            *a += b;
        }
        self.held += other.held;
        self.names += other.names;
        self.waited += other.waited;
        self.waited_us += other.waited_us;
    }

    fn total_ns(&self) -> u64 {
        self.ns.iter().sum()
    }
}

struct Timing {
    last: Instant,
    counts: Counts,
}

thread_local! {
    /// The push being timed on this thread (the server's main thread), if any
    static TIMING: RefCell<Option<Timing>> = RefCell::new(None);
}

fn with_timing(f: impl FnOnce(&mut Timing)) {
    TIMING.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            f(t);
        }
    });
}

/// The push being timed ended `step` now: the time since the last step's end goes to it
pub fn mark(step: Step) {
    with_timing(|t| {
        let now = Instant::now();
        t.counts.ns[step as usize] += now.duration_since(t.last).as_nanos() as u64;
        t.last = now;
    });
}

/// The push being timed found a change: it carries `rows` lines and names the rows `stale` says
/// changed without carrying them (worked out only while a push is timed)
pub fn carried(rows: usize, stale: impl FnOnce() -> u64) {
    with_timing(|t| {
        t.counts.pushes += 1;
        t.counts.rows += rows as u64;
        t.counts.stale += stale();
    });
}

/// The push being timed is of alerts alone, the pane's changes not worked out
pub fn alerts_alone() {
    with_timing(|t| t.counts.alone += 1);
}

/// The push being timed, of alerts alone, told the client the pane's lines are held (PaneHeld)
pub fn held() {
    with_timing(|t| t.counts.held += 1);
}

/// The push being timed, of alerts alone, sent the pane's title and folder alone
pub fn names() {
    with_timing(|t| t.counts.names += 1);
}

/// The push being timed encoded a PDU of `bytes` bytes: the push's own ends [`Step::Encode`]
pub fn encoded(bytes: usize, the_push: bool) {
    with_timing(|t| {
        t.counts.pdus += 1;
        t.counts.bytes += bytes as u64;
    });
    if the_push {
        mark(Step::Encode);
    }
}

/// A connection's pushes, kept until it next logs them
pub struct PushStats {
    logs: bool,
    since: Instant,
    rounds: u64,
    /// Rounds that then waited for: nothing (Now), the socket (Writable), news (News)
    waits: [u64; 3],
    panes: BTreeMap<PaneId, Counts>,
}

impl PushStats {
    pub fn new() -> Self {
        Self::logging(std::env::var_os("LBTERM_STATE_FILE").is_some())
    }

    fn logging(logs: bool) -> Self {
        Self {
            logs,
            since: Instant::now(),
            rounds: 0,
            waits: [0; 3],
            panes: BTreeMap::new(),
        }
    }

    /// A push for a pane is being worked out on this thread: time it, when this server logs
    pub fn begin(&self) {
        if self.logs {
            TIMING.with(|t| {
                *t.borrow_mut() = Some(Timing {
                    last: Instant::now(),
                    counts: Counts {
                        asked: 1,
                        ..Counts::default()
                    },
                })
            });
        }
    }

    /// The push for `pane_id` begun last is done
    pub fn end(&mut self, pane_id: PaneId) {
        if !self.logs {
            return;
        }
        mark(Step::Rest);
        if let Some(t) = TIMING.with(|t| t.borrow_mut().take()) {
            self.panes.entry(pane_id).or_default().add(&t.counts);
        }
    }

    /// Shown pane `pane_id`'s changes waited `waited` for the client's word that a frame read
    /// its last push (lbterm_flow::Pending::drawn)
    pub fn waited(&mut self, pane_id: PaneId, waited: Duration) {
        if self.logs {
            let counts = self.panes.entry(pane_id).or_default();
            counts.waited += 1;
            counts.waited_us += waited.as_micros() as u64;
        }
    }

    /// The connection ended a round and waits for `wait`; once a second since the last, the
    /// line to log, if it pushed anything meanwhile
    pub fn round(&mut self, wait: Wait) -> Option<String> {
        if !self.logs {
            return None;
        }
        self.rounds += 1;
        self.waits[match wait {
            Wait::Now => 0,
            Wait::Writable => 1,
            Wait::News => 2,
        }] += 1;
        let span = self.since.elapsed();
        if span < Duration::from_secs(1) {
            return None;
        }
        let line = if self.panes.values().any(|c| c.asked > 0) {
            Some(self.line(span, &shown_panes()))
        } else {
            None
        };
        *self = Self::logging(true);
        line
    }

    fn line(&self, span: Duration, shown: &HashSet<PaneId>) -> String {
        let mut all = Counts::default();
        for c in self.panes.values() {
            all.add(c);
        }
        let mut s = format!(
            "{:.3} s: {} rounds (then now {}, writable {}, news {}); pushes {}",
            span.as_secs_f64(),
            self.rounds,
            self.waits[0],
            self.waits[1],
            self.waits[2],
            counts_text(&all),
        );
        for (pane_id, c) in &self.panes {
            let _ = write!(
                s,
                "; pane {}{} {}",
                pane_id,
                if shown.contains(pane_id) { " shown" } else { "" },
                counts_text(c)
            );
        }
        s
    }
}

fn counts_text(c: &Counts) -> String {
    let mut s = format!(
        "asked {} alone {} pushed {} rows {} stale {} pdus {} bytes {} us {} held {} names {}",
        c.asked,
        c.alone,
        c.pushes,
        c.rows,
        c.stale,
        c.pdus,
        c.bytes,
        c.total_ns() / 1000,
        c.held,
        c.names
    );
    for (name, ns) in NAMES.iter().zip(c.ns.iter()) {
        let _ = write!(s, " {} {}", name, ns / 1000);
    }
    let _ = write!(s, " waited {} waited_us {}", c.waited, c.waited_us);
    s
}

/// The panes in each mux window's active tab
fn shown_panes() -> HashSet<PaneId> {
    let mut shown = HashSet::new();
    let mux = match Mux::try_get() {
        Some(mux) => mux,
        None => return shown,
    };
    for window_id in mux.iter_windows() {
        let tab = mux
            .get_window(window_id)
            .and_then(|window| window.get_active_tab().map(std::sync::Arc::clone));
        if let Some(tab) = tab {
            for pos in tab.iter_panes() {
                shown.insert(pos.pane.pane_id());
            }
        }
    }
    shown
}

#[cfg(test)]
mod test {
    use super::*;

    fn pushed(stats: &mut PushStats, pane_id: PaneId, rows: usize, bytes: usize) {
        stats.begin();
        mark(Step::Reads);
        mark(Step::Changed);
        mark(Step::Copy);
        carried(rows, || 7);
        mark(Step::Cells);
        encoded(bytes, true);
        encoded(5, false);
        stats.end(pane_id);
    }

    #[test]
    fn a_logging_connection_keeps_each_panes_pushes() {
        let mut stats = PushStats::logging(true);
        pushed(&mut stats, 3, 40, 300);
        pushed(&mut stats, 3, 2, 100);
        pushed(&mut stats, 5, 1, 50);
        // An owe of alerts alone: no read, its alert's PDU
        stats.begin();
        alerts_alone();
        encoded(5, false);
        stats.end(5);
        // Two of a held pane: the word of its hold, then its title alone
        stats.begin();
        alerts_alone();
        held();
        stats.end(6);
        stats.begin();
        alerts_alone();
        names();
        stats.end(6);
        // A shown pane's changes that waited twice for a frame's word
        stats.waited(5, Duration::from_micros(4_000));
        stats.waited(5, Duration::from_micros(9_500));
        let three = stats.panes[&3];
        assert_eq!(
            (three.asked, three.pushes, three.rows, three.stale, three.pdus, three.bytes),
            (2, 2, 42, 14, 4, 410)
        );
        assert_eq!(stats.panes[&5].rows, 1);
        assert_eq!((stats.panes[&5].asked, stats.panes[&5].alone), (2, 1));
        assert!(TIMING.with(|t| t.borrow().is_none()), "the timing slot is let go");

        let shown: HashSet<PaneId> = std::iter::once(5).collect();
        let line = stats.line(Duration::from_secs(1), &shown);
        assert!(line.contains("pushes asked 6 alone 3 pushed 3 rows 43"), "{}", line);
        assert!(line.contains("; pane 6 asked 2 alone 2 pushed 0"), "{}", line);
        assert!(line.contains(" held 1 names 1 reads "), "{}", line);
        assert!(line.contains("; pane 3 asked 2 alone 0 pushed 2 rows 42 stale 14"), "{}", line);
        assert!(line.contains("; pane 5 shown asked 2 alone 1"), "{}", line);
        assert!(line.contains(" reads ") && line.contains(" rest "), "{}", line);
        let five = line.split("; ").find(|part| part.starts_with("pane 5 ")).unwrap();
        assert!(five.ends_with(" waited 2 waited_us 13500"), "{}", line);
        assert!(line.contains(" waited 2 waited_us 13500; pane 3 "), "the connection's: {}", line);
    }

    #[test]
    fn a_connection_that_doesnt_log_times_nothing() {
        let mut stats = PushStats::logging(false);
        pushed(&mut stats, 3, 40, 300);
        stats.waited(3, Duration::from_millis(5));
        assert!(stats.panes.is_empty());
        assert_eq!(stats.round(Wait::Now), None);
        assert_eq!(stats.rounds, 0);
    }

    #[test]
    fn a_push_that_finds_no_change_is_asked_and_not_pushed() {
        let mut stats = PushStats::logging(true);
        stats.begin();
        mark(Step::Reads);
        mark(Step::Changed);
        stats.end(9);
        let nine = stats.panes[&9];
        assert_eq!((nine.asked, nine.pushes, nine.pdus), (1, 0, 0));
    }

    #[test]
    fn rounds_are_counted_by_what_they_wait_for_and_logged_after_a_second() {
        let mut stats = PushStats::logging(true);
        assert_eq!(stats.round(Wait::Now), None);
        assert_eq!(stats.round(Wait::Writable), None);
        assert_eq!(stats.waits, [1, 1, 0]);
        pushed(&mut stats, 1, 3, 30);
        stats.since -= Duration::from_secs(2);
        let line = stats.round(Wait::News).expect("a line after a second");
        assert!(line.contains("3 rounds (then now 1, writable 1, news 1)"), "{}", line);
        assert_eq!(stats.rounds, 0, "counted again from the line");
        stats.since -= Duration::from_secs(2);
        assert_eq!(stats.round(Wait::News), None, "nothing pushed, nothing logged");
    }
}
