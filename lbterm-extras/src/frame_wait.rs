//! When a window's panes wait for the push of a pane its mux server holds (lbterm/MUX.md,
//! "Tabs out of view: output held at the server", "The switch's first frame"; Lachlan's 11a of
//! 2026-11-04, and his 1a and 2b of 2026-11-08).
//!
//! A server of this build holds a pane's lines while no window shows it, and tells the window so
//! (PaneHeld): the window's copy of it is as of that word, and the pane is held until a push past
//! it comes (wezterm-client's `ClientPane::lbterm_is_held`). A switch to its tab says it's shown
//! (SetShownPanes, from the client domain's mux subscriber, before the switch's first frame),
//! and the server pushes it first. So the panes of the first frames that would show a held pane
//! wait for that push, for at most the bound after the first of them tried to paint
//! (`SWITCH_WAIT` in config's lbterm.rs, his 1a), and then draw what they have: the push puts it
//! right when it comes. A frame of a view already drawn, or of one none of whose panes is held,
//! is drawn at once, so a tab with nothing held never waits, and only a view's first frames do.
//!
//! What a frame shows is its "view" (`K`): the window's tab and which pane is zoomed in it, so
//! a switch, an unzoom and a window's first frame are a new view, and a split or a pane closed
//! isn't (a new pane's first push goes whatever is shown, and nothing is held of it).
//!
//! Only the panes wait (his 2b): the window paints every frame as it would, the rail, the strip,
//! its cards and every fade with it, and a frame of the wait ([`Frame::Keep`]) draws in the panes'
//! place those of the view drawn last ([`FrameWait::drawn`]), as they were, so the new rail can
//! show over the old screen while the push comes and no motion pauses for it. No thread waits and
//! no paint is held back: the window paints again at the bound, or when invalidated before it, as
//! the push's PaneOutput does (wezterm-gui's termwindow/frame_wait.rs).

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// What to do with the frame about to be painted ([`FrameWait::frame`])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Draw the view, its panes too
    Draw,
    /// Draw the window, but in place of the view's panes those of the view drawn last
    /// ([`FrameWait::drawn`], none if none was), as they were, and paint again by `until` unless
    /// invalidated before. `first` is the wait's first frame, when the window arms that wake
    Keep { until: Instant, first: bool },
}

/// The waits for held panes' pushes, counted for the window's counters (`metrics`'
/// `frame_wait`, lbterm/CONTROL.md)
pub struct Waits {
    /// Views whose first frame waited
    waits: AtomicU64,
    /// Of those, ended by the push, and by the bound
    pushed: AtomicU64,
    bounded: AtomicU64,
    /// Frames drawn meanwhile with the view drawn before's panes in place of theirs
    /// ([`Frame::Keep`])
    held: AtomicU64,
    /// How long they waited, in all and the longest, in µs
    waited_us: AtomicU64,
    max_us: AtomicU64,
}

impl Waits {
    pub const fn new() -> Self {
        Self {
            waits: AtomicU64::new(0),
            pushed: AtomicU64::new(0),
            bounded: AtomicU64::new(0),
            held: AtomicU64::new(0),
            waited_us: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
        }
    }

    fn ended(&self, by: &AtomicU64, waited: Duration) {
        by.fetch_add(1, Ordering::Relaxed);
        let us = waited.as_micros().min(u64::MAX as u128) as u64;
        self.waited_us.fetch_add(us, Ordering::Relaxed);
        self.max_us.fetch_max(us, Ordering::Relaxed);
    }

    /// The counts as `metrics` reports them, zeroed after when `reset`
    pub fn report_json(&self, reset: bool) -> Value {
        let take = |a: &AtomicU64| {
            if reset {
                a.swap(0, Ordering::Relaxed)
            } else {
                a.load(Ordering::Relaxed)
            }
        };
        let ms = |us: u64| us as f64 / 1_000.0;
        json!({
            "waits": take(&self.waits),
            "pushed": take(&self.pushed),
            "bounded": take(&self.bounded),
            "held": take(&self.held),
            "waited_ms": ms(take(&self.waited_us)),
            "max_ms": ms(take(&self.max_us)),
        })
    }
}

/// The process's waits, every window's
pub static WAITS: Waits = Waits::new();

/// A window's view last drawn, and the wait under way for a new one's first frame
pub struct FrameWait<K> {
    drawn: Option<K>,
    /// The view waited for and when its first frame tried to paint
    waiting: Option<(K, Instant)>,
    counts: &'static Waits,
}

impl<K> Default for FrameWait<K> {
    fn default() -> Self {
        Self::counting(&WAITS)
    }
}

impl<K> FrameWait<K> {
    /// One that counts in `counts`
    pub fn counting(counts: &'static Waits) -> Self {
        Self {
            drawn: None,
            waiting: None,
            counts,
        }
    }

    /// The window drew something other than its panes (the Conductor's view over them): the next
    /// frame of any view is its first, and a wait then has no panes to keep
    pub fn forget(&mut self) {
        self.drawn = None;
        self.waiting = None;
    }

    /// The view whose panes were drawn last: during a wait ([`Frame::Keep`]) the one whose panes
    /// stand in for the new view's, as they were. None before any, or once forgotten
    pub fn drawn(&self) -> Option<&K> {
        self.drawn.as_ref()
    }
}

impl<K: PartialEq> FrameWait<K> {

    /// The window is about to paint a frame of `view` at `now`. `held` says whether a pane the
    /// frame would draw is held at its server, and is asked only for a view not drawn yet. Keep
    /// the panes drawn last in its panes' place while a held pane's push hasn't come, up to
    /// `bound` after its first try; else draw it, and the view is drawn
    pub fn frame(
        &mut self,
        view: K,
        now: Instant,
        bound: Duration,
        held: impl FnOnce() -> bool,
    ) -> Frame {
        if self.drawn.as_ref() == Some(&view) {
            self.waiting = None;
            return Frame::Draw;
        }
        let since = match self.waiting.take() {
            Some((waited, since)) if waited == view => Some(since),
            _ => None,
        };
        if held() {
            let first = since.is_none();
            let since = since.unwrap_or(now);
            let until = since + bound;
            if now < until {
                if first {
                    self.counts.waits.fetch_add(1, Ordering::Relaxed);
                }
                self.counts.held.fetch_add(1, Ordering::Relaxed);
                self.waiting = Some((view, since));
                return Frame::Keep { until, first };
            }
            if !first {
                self.counts.ended(&self.counts.bounded, now - since);
            }
        } else if let Some(since) = since {
            self.counts.ended(&self.counts.pushed, now - since);
        }
        self.drawn = Some(view);
        Frame::Draw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUND: Duration = Duration::from_millis(50);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn counts() -> &'static Waits {
        Box::leak(Box::new(Waits::new()))
    }

    #[test]
    fn a_new_view_with_nothing_held_is_drawn_at_once() {
        let t = Instant::now();
        let c = counts();
        let mut w = FrameWait::counting(c);
        assert_eq!(w.frame(1, t, BOUND, || false), Frame::Draw);
        assert_eq!(w.frame(2, t + ms(5), BOUND, || false), Frame::Draw);
        assert_eq!(c.report_json(false)["waits"], 0);
    }

    fn keep(until: Instant, first: bool) -> Frame {
        Frame::Keep { until, first }
    }

    #[test]
    fn a_switch_to_a_held_pane_keeps_the_panes_drawn_before_until_its_push_and_no_longer() {
        let t = Instant::now();
        let c = counts();
        let mut w = FrameWait::counting(c);
        w.frame(1, t, BOUND, || false);
        // The switch to tab 2, its pane held: the window draws, tab 1's panes in place of tab 2's,
        // to the bound after the first try, whose frame arms the wake
        assert_eq!(w.frame(2, t + ms(10), BOUND, || true), keep(t + ms(60), true));
        assert_eq!(w.drawn(), Some(&1));
        // The rail's motion asked for the next frame meanwhile: drawn, the panes still tab 1's,
        // the same bound, the wake armed already
        assert_eq!(w.frame(2, t + ms(18), BOUND, || true), keep(t + ms(60), false));
        assert_eq!(w.drawn(), Some(&1));
        // The push came: tab 2's panes drawn, and the view's later frames aren't asked about
        assert_eq!(w.frame(2, t + ms(24), BOUND, || false), Frame::Draw);
        assert_eq!(w.drawn(), Some(&2));
        assert_eq!(w.frame(2, t + ms(30), BOUND, || panic!("not asked")), Frame::Draw);
        let r = c.report_json(true);
        assert_eq!((r["waits"].as_u64(), r["pushed"].as_u64(), r["bounded"].as_u64()), (Some(1), Some(1), Some(0)));
        assert_eq!(r["held"], 2);
        assert_eq!(r["max_ms"], 14.0);
    }

    #[test]
    fn a_push_that_doesnt_come_keeps_the_panes_to_the_bound_then_they_draw_what_they_have() {
        let t = Instant::now();
        let c = counts();
        let mut w = FrameWait::counting(c);
        w.frame(6, t - ms(100), BOUND, || false);
        assert_eq!(w.frame(7, t, BOUND, || true), keep(t + BOUND, true));
        assert_eq!(w.frame(7, t + BOUND, BOUND, || true), Frame::Draw);
        assert_eq!(w.drawn(), Some(&7));
        // Drawn: it waits no more while it's shown, held or not
        assert_eq!(w.frame(7, t + ms(80), BOUND, || true), Frame::Draw);
        let r = c.report_json(false);
        assert_eq!((r["waits"].as_u64(), r["bounded"].as_u64()), (Some(1), Some(1)));
        assert_eq!(r["max_ms"], 50.0);
    }

    #[test]
    fn a_switch_away_during_a_wait_and_back_waits_afresh() {
        let t = Instant::now();
        let c = counts();
        let mut w = FrameWait::counting(c);
        w.frame(1, t, BOUND, || false);
        assert_eq!(w.frame(2, t + ms(1), BOUND, || true), keep(t + ms(51), true));
        // Back to tab 1 before tab 2 was drawn: tab 1's panes are the ones on screen
        assert_eq!(w.frame(1, t + ms(5), BOUND, || true), Frame::Draw);
        // And to tab 2 again: a first frame again, its own bound and its own wake
        assert_eq!(w.frame(2, t + ms(40), BOUND, || true), keep(t + ms(90), true));
        assert_eq!(c.report_json(false)["waits"], 2);
    }

    #[test]
    fn with_nothing_drawn_before_a_wait_has_no_panes_to_keep() {
        let t = Instant::now();
        let mut w = FrameWait::counting(counts());
        // The window's first frame
        assert_eq!(w.frame(3, t, BOUND, || true), keep(t + BOUND, true));
        assert_eq!(w.drawn(), None);
        // Drawn, then the Conductor's view over the panes, then gone again with 3 held
        assert_eq!(w.frame(3, t + ms(2), BOUND, || false), Frame::Draw);
        w.forget();
        assert_eq!(w.drawn(), None);
        assert_eq!(w.frame(3, t + ms(9), BOUND, || true), keep(t + ms(59), true));
        assert_eq!(w.drawn(), None);
    }
}
