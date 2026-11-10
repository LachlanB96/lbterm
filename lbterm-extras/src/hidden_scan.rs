//! When a mux pane in a tab out of view has gone quiet, so its screen is scanned for links before
//! it's shown (lbterm/MUX.md, "Tabs out of view").
//!
//! A mux domain's window scans a line for its implicit hyperlinks as it's drawn, not as a push
//! brings it (wezterm-client's `scan_rows`), so a busy tab out of view costs the GUI thread
//! nothing for lines it never shows. But a tab whose screen changed out of view then had its
//! whole screen scanned in the first frame that shows it: with Lachlan's rules about 80 µs a line
//! of text, 3 ms for a screen of 36, which the switch to it waited on. Most switches are to a tab
//! that has stopped (a Claude Code session done, waiting on him), so a pane out of view that's had
//! no output for its quiet span ([`QuietPanes::new`]) has the rows it would show scanned then, on the GUI thread, once: a tab
//! still working is never scanned out of view, and a switch to it scans what it shows as before.
//!
//! [`QuietPanes`] keeps when each such pane is due and says when to look again; one timer a
//! window, armed by output only when none is (so a push costs a lock and a map insert), and armed
//! again for the next pane due when it fires. At rest nothing is armed.
//!
//! A server of perf/hidden-output's build holds a pane's lines while no window shows it and
//! pushes them once it has printed nothing for `config::lbterm::HELD_QUIET`, which the window
//! gives [`QuietPanes::new`] as its quiet span, so the two are one number (lbterm/MUX.md,
//! "Tabs out of view: output held at the server"): the push that ends a hold comes of a pane quiet
//! already. So a push that ended one (`ClientPane::lbterm_holds_ended` moved) is due at once
//! ([`Scan::Soon`]: one task for every such push until it runs, [`QuietPanes::due_now`]), and only
//! other pushes with rows wait the quiet span on the timer: a pane another window shows, which no
//! server holds, and every pane of a server of an older build, which pushes as it prints.
//!
//! Output here is a push that brought the pane new rows, a line whose text it didn't hold or a row
//! named and not carried: the client pane counts those (`ClientPane::lbterm_rows_changed`), and a
//! push with the count where it was (its title, its folder or its cursor alone, which carries the
//! cursor's row again as it was, since the server carries that row in every push) leaves its
//! screen as scanned as it was, so it's noted not at all and puts back no pane's quiet span. A
//! working Claude Code session out of view sets its title every 960 ms, and each title armed the
//! timer, its task, its wake and the hop to the window for a scan of that one line again: about
//! 0.1 ms of the GUI thread a title (lbterm/PITFALLS.md, 176; lbterm/MUX.md, "What a title
//! costs").
//!
//! Quiet is measured from when the GUI thread applied the pane's last such push (the window's
//! subscriber notes it as the push's PaneOutput comes), not from when its program printed. So a
//! hold of the GUI thread (a stall, a long task) makes every flooding pane out of view look quiet,
//! and they're all scanned as it resumes: after a 1.7 s stall of a test's whole job, 111 runs of
//! the scan's Apply task, the rest about 0.45 ms each (the review of perf/mux-client,
//! 2026-11-08; lbterm/FLOWS.md's pane-output flow, row 60). Cheap and harmless: the scan finds
//! what it would have found, and the pane is noted again at its next push.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What the window does for a pane's output out of view ([`QuietPanes::output`])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scan {
    /// Nothing more: no new rows, or a look is coming already
    Nothing,
    /// Arm the timer for then: none is armed
    Arm(Instant),
    /// Scan the panes due now, on the GUI thread's next turn ([`QuietPanes::due_now`]): the push
    /// ended the server's hold of the pane, which comes once it has been quiet there (no task of
    /// that kind is queued yet)
    Soon,
}

/// The panes out of view that have printed since their screen was last scanned, by pane id
#[derive(Debug)]
pub struct QuietPanes {
    /// How long a pane out of view has to have printed nothing before its screen is scanned:
    /// the window's is the span a mux server holds a pane's lines before it pushes them
    /// (`config::lbterm::HELD_QUIET`), since a push that ended a hold is taken as one quiet that
    /// long ([`Scan::Soon`])
    quiet: Duration,
    /// When each is due: the quiet span after the last push that brought it rows, or at that push when
    /// it ended a hold
    due: HashMap<usize, Instant>,
    /// Each pane's count of pushes that brought it rows, and of holds a push ended, when it was
    /// last noted: a push with the same rows brought none. Kept for every pane noted out of view
    /// while the window lasts, 24 bytes each, never more than the panes it ever had
    rows: HashMap<usize, (u64, u64)>,
    armed: bool,
    /// A task to scan the panes due at once is queued
    soon: bool,
}

impl QuietPanes {
    /// No pane noted yet, each to be scanned once it has printed nothing for `quiet`
    pub fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            due: HashMap::new(),
            rows: HashMap::new(),
            armed: false,
            soon: false,
        }
    }

    /// `pane`, out of view, was pushed at `now`, with `rows` its count of pushes that brought it
    /// rows (`ClientPane::lbterm_rows_changed`) and `ended` its count of the server's holds a
    /// push ended (`ClientPane::lbterm_holds_ended`). A push with the rows it had when last
    /// noted brought none: nothing, and the pane, if it's waiting, waits as it was. One that
    /// ended a hold is the server's push of a pane quiet for `HELD_QUIET` (or one a poll or a
    /// resize asked): due at once. Any other waits the quiet span from now
    pub fn output(&mut self, pane: usize, rows: u64, ended: u64, now: Instant) -> Scan {
        let was = self.rows.insert(pane, (rows, ended));
        if was.map(|(rows, _)| rows) == Some(rows) {
            return Scan::Nothing;
        }
        // A pane first noted counts every hold ended before, in view too: taken as this push's,
        // which at worst scans at once what would have been scanned a quiet span on
        if was.map_or(ended > 0, |(_, before)| before != ended) {
            self.due.insert(pane, now);
            if self.soon {
                return Scan::Nothing;
            }
            self.soon = true;
            return Scan::Soon;
        }
        self.due.insert(pane, now + self.quiet);
        if self.armed {
            Scan::Nothing
        } else {
            self.armed = true;
            Scan::Arm(now + self.quiet)
        }
    }

    /// The timer fired at `now`: the panes due, taken out, and when to fire next for those still
    /// printing (the soonest of them to be due), or `None` with none left, when it's no longer
    /// armed
    pub fn due(&mut self, now: Instant) -> (Vec<usize>, Option<Instant>) {
        let due = self.take_due(now);
        let next = self.due.values().copied().min();
        self.armed = next.is_some();
        (due, next)
    }

    /// The task [`Scan::Soon`] asked for runs at `now`: the panes due, taken out. The timer is
    /// left as it is: every pane still waiting waits on it
    pub fn due_now(&mut self, now: Instant) -> Vec<usize> {
        self.soon = false;
        self.take_due(now)
    }

    fn take_due(&mut self, now: Instant) -> Vec<usize> {
        let mut due = vec![];
        self.due.retain(|pane, at| {
            if *at <= now {
                due.push(*pane);
                false
            } else {
                true
            }
        });
        due.sort_unstable();
        due
    }

    /// Whether the timer is armed
    pub fn armed(&self) -> bool {
        self.armed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's quiet span, `config::lbterm::HELD_QUIET`
    const QUIET: Duration = Duration::from_millis(250);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_first_output_arms_the_timer_and_the_rest_dont() {
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        assert_eq!(q.output(1, 1, 0, t), Scan::Arm(t + QUIET));
        assert_eq!(q.output(1, 2, 0, t + ms(10)), Scan::Nothing);
        assert_eq!(q.output(2, 3, 0, t + ms(20)), Scan::Nothing);
        assert!(q.armed());
    }

    #[test]
    fn a_pane_still_printing_waits_and_the_timer_is_armed_for_it() {
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        q.output(1, 4, 0, t);
        q.output(1, 5, 0, t + ms(200));
        // Fired at 250: pane 1 printed 50 ms ago, so it's due at 450
        assert_eq!(q.due(t + QUIET), (vec![], Some(t + ms(200) + QUIET)));
        assert!(q.armed());
        assert_eq!(q.due(t + ms(450)), (vec![1], None));
        assert!(!q.armed());
    }

    #[test]
    fn each_pane_is_due_as_it_goes_quiet_and_a_busy_one_goes_on_waiting() {
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        q.output(3, 6, 0, t);
        q.output(1, 7, 0, t + ms(10));
        q.output(2, 8, 0, t + ms(100));
        // A pane that never stops is never due: one look at each of its quiet spans' end
        let mut fired = 0;
        let mut at = t + QUIET;
        let mut got = vec![];
        for i in 1..=20u64 {
            q.output(2, 100 + i, 0, t + ms(100 + 100 * i));
            if at <= t + ms(100 + 100 * i) {
                let (due, next) = q.due(at);
                got.extend(due);
                fired += 1;
                match next {
                    Some(n) => at = n,
                    None => break,
                }
            }
        }
        // Each when its own quiet span ended, pane 3's first
        assert_eq!(got, vec![3, 1]);
        assert!(fired <= 21, "{fired} wakes");
        assert!(q.armed(), "still waiting on the busy pane");
    }

    #[test]
    fn once_every_pane_is_due_nothing_is_armed_till_the_next_output() {
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        q.output(1, 10, 0, t);
        assert_eq!(q.due(t + QUIET), (vec![1], None));
        assert!(!q.armed());
        assert_eq!(q.due(t + QUIET * 2), (vec![], None), "a pane is scanned once a quiet span");
        assert_eq!(q.output(1, 11, 0, t + ms(300)), Scan::Arm(t + ms(300) + QUIET));
    }

    #[test]
    fn a_push_that_brought_no_rows_arms_nothing_once_its_screen_was_scanned() {
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        q.output(1, 4, 0, t);
        assert_eq!(q.due(t + QUIET), (vec![1], None));
        // A title every 960 ms, as a working Claude Code session out of view sets one: its rows
        // as they were, so no timer, no task and no scan
        for i in 1..=5u64 {
            assert_eq!(q.output(1, 4, 0, t + ms(960 * i)), Scan::Nothing, "title {i}");
            assert!(!q.armed());
        }
        assert_eq!(q.due(t + ms(6000)), (vec![], None));
        // Its next line is news again
        assert_eq!(q.output(1, 5, 0, t + ms(6000)), Scan::Arm(t + ms(6000) + QUIET));
    }

    #[test]
    fn a_push_that_brought_no_rows_doesnt_put_back_a_waiting_panes_quiet_span() {
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        q.output(1, 1, 0, t);
        // Its title 200 ms on: it's still due at 250 from its last line, not at 450
        assert_eq!(q.output(1, 1, 0, t + ms(200)), Scan::Nothing);
        assert_eq!(q.due(t + QUIET), (vec![1], None));
    }

    #[test]
    fn a_pane_first_seen_by_its_title_is_scanned_once() {
        // A pane whose rows came while it was in view, or before the window noted it, has a count
        // the book hasn't seen: its first push out of view is news, whatever it brought
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        assert_eq!(q.output(7, 12, 0, t), Scan::Arm(t + QUIET));
        assert_eq!(q.due(t + QUIET), (vec![7], None));
        assert_eq!(q.output(7, 12, 0, t + ms(960)), Scan::Nothing);
    }

    #[test]
    fn the_push_that_ends_a_hold_is_scanned_at_once_and_one_task_takes_them_all() {
        // A server of this build holds a pane's lines while it's out of view and pushes them once
        // it has been quiet for HELD_QUIET: that push ended its hold, so it's quiet already
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        // Each pane seen first by a title out of view, before any hold of it ended
        q.output(1, 3, 0, t);
        q.output(2, 9, 0, t);
        assert_eq!(q.due(t + QUIET), (vec![1, 2], None));
        // Their quiet pushes: their rows and their holds ended both moved
        assert_eq!(q.output(1, 4, 1, t + ms(800)), Scan::Soon);
        assert_eq!(q.output(2, 10, 1, t + ms(801)), Scan::Nothing, "the task queued takes it too");
        assert!(!q.armed(), "no timer for them");
        assert_eq!(q.due_now(t + ms(802)), vec![1, 2]);
        // The next such push queues the next task
        assert_eq!(q.output(1, 5, 2, t + ms(2000)), Scan::Soon);
    }

    #[test]
    fn a_push_that_ended_no_hold_waits_its_quiet_span_beside_one_that_did() {
        // A pane another window shows is pushed as it prints, never held: it waits QUIET, as on a
        // server of a build before the holds; the scan at once leaves its timer as it was
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        assert_eq!(q.output(4, 1, 0, t), Scan::Arm(t + QUIET));
        assert_eq!(q.output(5, 1, 0, t), Scan::Nothing);
        assert_eq!(q.output(5, 2, 1, t + ms(10)), Scan::Soon);
        assert_eq!(q.due_now(t + ms(11)), vec![5], "pane 4 still waits for its quiet");
        assert!(q.armed());
        assert_eq!(q.due(t + QUIET), (vec![4], None));
    }

    #[test]
    fn a_pane_first_noted_by_the_push_that_ended_its_hold_is_scanned_at_once() {
        // Its hold began and ended out of view with nothing between pushed: its first output out
        // of view is the push that ends it
        let t = Instant::now();
        let mut q = QuietPanes::new(QUIET);
        assert_eq!(q.output(6, 2, 1, t), Scan::Soon);
        assert_eq!(q.due_now(t), vec![6]);
        // Its next line, no hold ended: a quiet span
        assert_eq!(q.output(6, 3, 1, t + ms(5)), Scan::Arm(t + ms(5) + QUIET));
    }
}
