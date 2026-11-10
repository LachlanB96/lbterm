//! LBTerm: how often a client pane asks its server for the pane's changes (upstream's poll).
//!
//! Every frame that paints a pane asks it what changed (`get_changed_since`), and a client pane
//! then asks the server too (GetPaneRenderChanges) once its poll interval has passed since the
//! last time. The interval starts at 20 ms and doubles each poll up to 30 s, and upstream set it
//! back to 20 ms on every push from the server and every key, paste, resize or mouse event sent:
//! so a pane in view that was printing, or being typed into, was polled every 20 to 40 ms, about
//! once a push. The server answered each with a push of its own, worked out on its main thread
//! outside the pushes it owes (`maybe_push_pane_changes`), and a LivenessResponse, though the
//! pushes had brought every change already. On a unix domain the server pushes every change of
//! every pane's output to a window, and says when a pane has gone (PaneRemoved), so a poll there
//! is the backoff's check that the pane lives: a push, a key, a paste or the mouse doesn't bring
//! it back. Over TLS and ssh, where a link can stall without a word, every push and input still
//! does, as upstream's did.
//!
//! A resize or a zoom brought it back on any domain until perf/mux-client's close-pane stage: the
//! server rewraps the pane with no output, so no push said so, and until the next output the
//! window had neither the pane's new size nor the sequence number its lines have now (a line
//! newer than the pane's number is fetched again every frame until it has one: a quiet pane split
//! or zoomed fetched a line and painted 60 times a second, perf/cli-repaint.ps1 -Mux, 2026-10-31).
//! But a poll brought back goes out with the next frame only once 20 ms have passed since the
//! last, and only for a pane a frame draws: the last size of a drag, or a tab out of view, could
//! go without it. Since then the resize itself asks for the push once the server has answered it
//! (renderable.rs's `ask_changes`, from clientpane.rs's resize), which a window can take the
//! server's word from for the rows the rewrap changed.

use std::time::{Duration, Instant};

/// The first interval, after a push or input over TLS or ssh
pub const BASE: Duration = Duration::from_millis(20);
/// The longest the interval grows to
pub const MAX: Duration = Duration::from_secs(30);

pub struct PollPace {
    /// Whether the connection is a unix domain's, which pushes every change
    local: bool,
    interval: Duration,
    last: Instant,
}

impl PollPace {
    pub fn new(local: bool, now: Instant) -> Self {
        Self {
            local,
            interval: BASE,
            last: now,
        }
    }

    /// The interval now
    #[cfg(test)]
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// How long input may go unanswered before the pane is tardy (`is_tardy`: the lag
    /// indicator, and Lua's `pane:get_metadata().is_tardy`). Upstream waited out the poll
    /// interval or 3 s, whichever was longer, and since input set the interval back to 20 ms,
    /// that was 3 s after a key. On a unix domain input leaves the interval where the backoff has
    /// it, up to 30 s, so it's 3 s there: what upstream's came to. Over TLS and ssh, upstream's
    pub fn tardy_after(&self) -> Duration {
        let floor = Duration::from_secs(3);
        if self.local {
            floor
        } else {
            self.interval.max(floor)
        }
    }

    /// The server pushed the pane's changes: over TLS or ssh the next poll comes soon again; on
    /// a unix domain a push says nothing a poll would
    pub fn pushed(&mut self) {
        if !self.local {
            self.interval = BASE;
        }
    }

    /// Input went to the pane (a key, a paste, a resize, the mouse): as `pushed`, its echo comes
    /// as a push on a unix domain
    pub fn sent(&mut self) {
        if !self.local {
            self.interval = BASE;
        }
    }

    /// Whether to poll at `now`: when the interval has passed since the last, which then doubles
    pub fn due(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.last) < self.interval {
            return false;
        }
        self.interval = (self.interval + self.interval).min(MAX);
        self.last = now;
        true
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// How many polls a pane painted every `frame` and pushed every `push` gets over `span`
    fn polls(local: bool, frame: Duration, push: Duration, span: Duration) -> usize {
        let start = Instant::now();
        let mut pace = PollPace::new(local, start);
        let mut polls = 0;
        let mut t = Duration::ZERO;
        let mut next_push = push;
        while t < span {
            t += frame;
            while next_push <= t {
                pace.pushed();
                next_push += push;
            }
            if pace.due(start + t) {
                polls += 1;
            }
        }
        polls
    }

    #[test]
    fn a_unix_domain_pane_printing_is_polled_on_the_backoff_alone() {
        // A frame and a push every 16 ms for 10 s: 20, 40, 80 ... 5,120 ms after the last, about
        // a dozen polls in all, not one a frame
        let n = polls(true, Duration::from_millis(16), Duration::from_millis(16), Duration::from_secs(10));
        assert!(n <= 10, "{} polls", n);
        // and none past the cap once it's reached
        let n = polls(true, Duration::from_millis(16), Duration::from_millis(16), Duration::from_secs(120));
        assert!(n <= 14, "{} polls in two minutes", n);
    }

    #[test]
    fn over_tls_a_push_brings_the_poll_back_as_upstream_did() {
        let n = polls(false, Duration::from_millis(16), Duration::from_millis(16), Duration::from_secs(10));
        assert!(n > 200, "{} polls: about one each 20 to 32 ms", n);
    }

    #[test]
    fn input_brings_it_back_over_tls_only() {
        let start = Instant::now();
        for local in [true, false] {
            let mut pace = PollPace::new(local, start);
            let mut t = start;
            for _ in 0..12 {
                t += pace.interval();
                assert!(pace.due(t));
            }
            assert_eq!(pace.interval(), MAX);
            pace.sent();
            assert_eq!(pace.interval(), if local { MAX } else { BASE });
        }
    }

    #[test]
    fn unanswered_input_is_tardy_after_3_s_as_upstream() {
        let start = Instant::now();
        for local in [true, false] {
            let mut pace = PollPace::new(local, start);
            let mut t = start;
            for _ in 0..12 {
                t += pace.interval();
                assert!(pace.due(t));
            }
            assert_eq!(pace.interval(), MAX);
            // A key after a long quiet: upstream set the interval back to 20 ms, so 3 s
            pace.sent();
            assert_eq!(pace.tardy_after(), Duration::from_secs(3), "local {}", local);
        }
        // Over TLS a poll interval past 3 s is waited out, as upstream did
        let mut pace = PollPace::new(false, start);
        let mut t = start;
        for _ in 0..12 {
            t += pace.interval();
            assert!(pace.due(t));
        }
        assert_eq!(pace.tardy_after(), MAX);
    }

    #[test]
    fn not_before_the_interval() {
        let start = Instant::now();
        let mut pace = PollPace::new(true, start);
        assert!(!pace.due(start + Duration::from_millis(19)));
        assert!(pace.due(start + Duration::from_millis(20)));
        assert_eq!(pace.interval(), Duration::from_millis(40));
        assert!(!pace.due(start + Duration::from_millis(59)));
        assert!(pace.due(start + Duration::from_millis(60)));
    }
}
