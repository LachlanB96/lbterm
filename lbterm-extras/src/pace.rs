//! Frame pacing on the GUI thread: how a window's frames are spaced, and the counts that show the
//! message loop at work. The Windows side is window/src/os/windows/pace.rs and wm_paint.
//!
//! A window paints when Windows hands it WM_PAINT, then holds its next frame back so it paints at
//! most `max_fps` a second, and no faster than the screen it's on shows them, at the screen's
//! exact rate ([`period`], [`refresh_period`]): the
//! hold runs one period from when the paint started, or for the frame a hold was waited for,
//! from that hold's end ([`hold_until`]). Anything that asks for a frame during the hold
//! (`invalidate`, which is InvalidateRect) gets a WM_PAINT that's held: the window is validated,
//! so Windows doesn't send that WM_PAINT again, and the hold's end paints what was asked. The end
//! comes from a high-resolution waitable timer the message loop waits on beside its events,
//! within a fraction of a millisecond of when it's due.
//!
//! Until 2026-10-30 (upstream's wm_paint) a held WM_PAINT left the window unvalidated, so Windows
//! sent it again every time the queue was empty and the loop went round on it until the hold
//! ended: with a line printed every 50 ms, 335 to 342 WM_PAINTs a frame, and about two thirds of
//! the GUI thread's time spent outside its paints. And the hold was async-io's timer, which
//! Windows ends on its 15.6 ms tick in a process that doesn't raise its timer resolution (this
//! one doesn't): a 20 ms hold ended 7.3 to 7.6 ms late on average, so max_fps 50 painted 31 to 32
//! frames a second and max_fps 120 56 to 63, unevenly. The hold also ran from the paint's end, in
//! whole milliseconds, so a 5 ms paint at max_fps 60 made frames at least 21 ms apart.
//!
//! The counts are the process's since it started, every window together, and go to a test
//! window's state file as `pace` (lbterm/STATE.md). They cost a relaxed atomic add each.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How far apart a window's frames start, at least: one frame of `max_fps`, and never less than
/// one refresh of its screen, `refresh` when Windows says what that is ([`refresh_period`]). A
/// frame its screen can't show is work for nothing, and with Fifo presentation it also waits on
/// DWM for a refresh, on the GUI thread.
pub fn period(max_fps: u64, refresh: Option<Duration>) -> Duration {
    let by_fps = Duration::from_nanos(1_000_000_000 / max_fps.max(1));
    match refresh {
        Some(refresh) => by_fps.max(refresh),
        None => by_fps,
    }
}

/// One refresh of a screen whose rate is `numerator / denominator` Hz, as QueryDisplayConfig
/// gives a target's (159952/1000, 60000/1001), to the nanosecond below, so frames paced to it
/// never come slower than the screen shows them. None for a rate of 1 Hz or less, which is
/// Windows saying it doesn't know (0, or 1 for "the hardware's default"). EnumDisplaySettings'
/// dmDisplayFrequency, read until 2026-10-30, is a whole number Windows rounds: a 59.94 Hz mode
/// it calls 59 would have paced at 59 frames a second, one refresh shown twice about every second.
pub fn refresh_period(numerator: u32, denominator: u32) -> Option<Duration> {
    if denominator == 0 || numerator <= denominator {
        return None;
    }
    let nanos = 1_000_000_000u128 * denominator as u128 / numerator as u128;
    Some(Duration::from_nanos(nanos as u64))
}

/// When a window whose paint started at `started` may paint again: a period on. A paint that's
/// the frame asked for during the hold before it, whose end was `waited` for, counts from that
/// end, when it came within a period of it: the loop gets to it a fraction of a millisecond
/// late, and counting from the paint would put every frame after it later too, so frames would
/// come a little under max_fps (58.5 a second at 60). Any other paint counts from its start.
pub fn hold_until(started: Instant, waited: Option<Instant>, period: Duration) -> Instant {
    match waited {
        Some(end) if started >= end && started < end + period => end + period,
        _ => started + period,
    }
}

/// Whether a window whose hold runs until `until` is still holding at `now`.
pub fn holding(until: Option<Instant>, now: Instant) -> bool {
    matches!(until, Some(until) if now < until)
}

/// A waitable timer's due time for `until`, from `now`: relative, in 100 ns units, so negative,
/// and at least one unit, so one already due fires straight away.
pub fn due_time(now: Instant, until: Instant) -> i64 {
    let units = until.saturating_duration_since(now).as_nanos() / 100;
    -(units.clamp(1, i64::MAX as u128) as i64)
}

/// The holds a frame was asked for during, by window (`K`), each with when it ends: what the
/// message loop's timer wakes it for. A window has one at a time.
#[derive(Debug)]
pub struct Wakes<K> {
    wakes: Vec<(K, Instant)>,
    /// What the timer was last set for
    armed: Option<Instant>,
}

/// What the timer is to do before the loop waits ([`Wakes::arm`])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// No wake: the loop waits on its events alone
    Idle,
    /// Set it for this
    Set(Instant),
    /// It's set for the soonest already
    Armed,
}

impl<K: Copy + PartialEq> Default for Wakes<K> {
    fn default() -> Self {
        Self {
            wakes: vec![],
            armed: None,
        }
    }
}

impl<K: Copy + PartialEq> Wakes<K> {
    /// `key`'s hold, which ends at `until`, had a frame asked for during it
    pub fn wake(&mut self, key: K, until: Instant) {
        if let Some(wake) = self.wakes.iter_mut().find(|(k, _)| *k == key) {
            wake.1 = until;
        } else {
            self.wakes.push((key, until));
        }
    }

    /// The wakes due at `now`, taken off the list; nothing allocated when none is. A timer set for
    /// `now` or before has had its time: it has fired, or is about to, and the wait that sees it
    /// takes its signal (it resets itself), so it's no longer taken to be set. A wake put back
    /// that's already due, as [`Wakes::wake`] after this, sets it again, to fire at once, where it
    /// would have been taken as set and the loop would have waited on a timer that never fires.
    pub fn take_due(&mut self, now: Instant) -> Vec<(K, Instant)> {
        if matches!(self.armed, Some(at) if at <= now) {
            self.armed = None;
        }
        if !self.wakes.iter().any(|(_, until)| *until <= now) {
            return vec![];
        }
        let mut due = vec![];
        self.wakes.retain(|wake| {
            if wake.1 <= now {
                due.push(*wake);
                false
            } else {
                true
            }
        });
        due
    }

    /// What the timer is to do: set for the soonest wake, when it isn't set for that already,
    /// which it's then taken to be
    pub fn arm(&mut self) -> Arm {
        let Some(soonest) = self.wakes.iter().map(|(_, until)| *until).min() else {
            return Arm::Idle;
        };
        if self.armed == Some(soonest) {
            return Arm::Armed;
        }
        self.armed = Some(soonest);
        Arm::Set(soonest)
    }

    /// Setting the timer [`Wakes::arm`] asked for failed: it isn't set, so the next arm asks
    /// again
    pub fn disarm(&mut self) {
        self.armed = None;
    }
}

/// A hold that ends this much after it was due ended late: a frame came later than max_fps
/// allowed. Windows' timer tick is 15.6 ms, so a hold that ends on it is late most of the time.
pub const LATE_AFTER: Duration = Duration::from_millis(2);

/// What the state file's `pace` holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Turns of the message loop: each runs the queued tasks, then handles one window message or
    /// waits for one
    pub turns: u64,
    /// WM_PAINTs that painted a frame
    pub paints: u64,
    /// WM_PAINTs that came while the window held its next frame back
    pub held: u64,
    /// The GUI thread's cycles inside those paints (QueryThreadCycleTime): what painting cost,
    /// apart from the waits on DWM inside it, which aren't the thread's
    pub paint_cycles: u64,
    /// Holds that ended with a frame asked for during them
    pub holds: u64,
    /// How much later than due those ended, in all, in microseconds
    pub late_us: u64,
    /// Those that ended more than [`LATE_AFTER`] late
    pub late: u64,
}

/// The counts, as atomics: one for the process, [`PACE`].
#[derive(Debug, Default)]
pub struct Pace {
    turns: AtomicU64,
    paints: AtomicU64,
    held: AtomicU64,
    paint_cycles: AtomicU64,
    holds: AtomicU64,
    late_us: AtomicU64,
    late: AtomicU64,
}

impl Pace {
    pub const fn new() -> Self {
        Self {
            turns: AtomicU64::new(0),
            paints: AtomicU64::new(0),
            held: AtomicU64::new(0),
            paint_cycles: AtomicU64::new(0),
            holds: AtomicU64::new(0),
            late_us: AtomicU64::new(0),
            late: AtomicU64::new(0),
        }
    }

    pub fn turn(&self) {
        self.turns.fetch_add(1, Ordering::Relaxed);
    }

    /// A WM_PAINT painted, taking `cycles` of the thread's
    pub fn painted(&self, cycles: u64) {
        self.paints.fetch_add(1, Ordering::Relaxed);
        self.paint_cycles.fetch_add(cycles, Ordering::Relaxed);
    }

    /// A WM_PAINT came during a hold
    pub fn held(&self) {
        self.held.fetch_add(1, Ordering::Relaxed);
    }

    /// A hold with a frame asked for ended `late` after it was due
    pub fn hold_ended(&self, late: Duration) {
        self.holds.fetch_add(1, Ordering::Relaxed);
        self.late_us.fetch_add(
            late.as_micros().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        if late > LATE_AFTER {
            self.late.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn counts(&self) -> Counts {
        Counts {
            turns: self.turns.load(Ordering::Relaxed),
            paints: self.paints.load(Ordering::Relaxed),
            held: self.held.load(Ordering::Relaxed),
            paint_cycles: self.paint_cycles.load(Ordering::Relaxed),
            holds: self.holds.load(Ordering::Relaxed),
            late_us: self.late_us.load(Ordering::Relaxed),
            late: self.late.load(Ordering::Relaxed),
        }
    }
}

/// The process's counts.
pub static PACE: Pace = Pace::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_is_one_frame_of_max_fps_to_the_nanosecond() {
        assert_eq!(period(120, None), Duration::from_nanos(8_333_333));
        assert_eq!(period(60, None), Duration::from_nanos(16_666_666));
        assert_eq!(period(50, None), Duration::from_millis(20));
    }

    #[test]
    fn a_period_is_never_shorter_than_a_refresh_of_the_screen() {
        // max_fps 120 on a 60 Hz screen: the screen's
        assert_eq!(
            period(120, refresh_period(60, 1)),
            Duration::from_nanos(16_666_666)
        );
        // On a 144 Hz screen max_fps 120 is the slower
        assert_eq!(
            period(120, refresh_period(144, 1)),
            Duration::from_nanos(8_333_333)
        );
        assert_eq!(period(50, refresh_period(60, 1)), Duration::from_millis(20));
        // Lachlan's main screen, 159.952 Hz: max_fps 120 is the slower
        assert_eq!(
            period(120, refresh_period(159_952, 1000)),
            Duration::from_nanos(8_333_333)
        );
        // No rate known: max_fps alone
        assert_eq!(period(120, None), Duration::from_nanos(8_333_333));
    }

    #[test]
    fn a_refresh_is_the_screen_s_exact_rate_to_the_nanosecond_below() {
        assert_eq!(refresh_period(60, 1), Some(Duration::from_nanos(16_666_666)));
        // 59.94 Hz, which EnumDisplaySettings calls 59: paced at 59 a second, a refresh would
        // be shown twice about once a second
        assert_eq!(
            refresh_period(60_000, 1001),
            Some(Duration::from_nanos(16_683_333))
        );
        assert_eq!(
            refresh_period(59_940, 1000),
            Some(Duration::from_nanos(16_683_350))
        );
        assert!(refresh_period(60_000, 1001).unwrap() < Duration::from_nanos(1_000_000_000 / 59));
        // 159.952 Hz, which it calls 160: a hair longer than 1/160 s
        assert_eq!(
            refresh_period(159_952, 1000),
            Some(Duration::from_nanos(6_251_875))
        );
        // Windows not knowing: 0/0, 0, or 1 Hz ("the hardware's default")
        assert_eq!(refresh_period(0, 0), None);
        assert_eq!(refresh_period(60, 0), None);
        assert_eq!(refresh_period(0, 1), None);
        assert_eq!(refresh_period(1, 1), None);
        assert_eq!(refresh_period(1000, 1000), None);
        // The largest a rational can hold doesn't overflow
        assert_eq!(refresh_period(u32::MAX, u32::MAX - 1), Some(Duration::from_nanos(999_999_999)));
    }

    #[test]
    fn max_fps_0_is_one_a_second_not_a_division_by_zero() {
        assert_eq!(period(0, None), Duration::from_secs(1));
    }

    #[test]
    fn a_hold_runs_one_period_from_the_paint_starting() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        assert_eq!(hold_until(t, None, period(50, refresh_period(60, 1))), t + ms(20));
        // Any paint but the one waited for: from its start, wherever the last hold ended
        assert_eq!(hold_until(t + ms(25), None, ms(20)), t + ms(45));
    }

    #[test]
    fn the_frame_a_hold_was_waited_for_counts_from_the_hold_s_end() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        let us = Duration::from_micros;
        // Painted 0.3 ms after its hold ended at t: the next is a period from t
        assert_eq!(hold_until(t + us(300), Some(t), ms(20)), t + ms(20));
        // Started 19 ms late, the loop busy with something else: still from the hold's end, so
        // the next paints as soon as this one is done, catching up one frame and never more
        assert_eq!(hold_until(t + ms(19), Some(t), ms(20)), t + ms(20));
        // A period or more after it: from the paint
        assert_eq!(hold_until(t + ms(20), Some(t), ms(20)), t + ms(40));
        assert_eq!(hold_until(t + ms(50), Some(t), ms(20)), t + ms(70));
        // Before it (it can't, but the clock is the clock): from the paint
        assert_eq!(hold_until(t, Some(t + ms(1)), ms(20)), t + ms(20));
    }

    #[test]
    fn holding_is_until_the_hold_s_end() {
        let t = Instant::now();
        assert!(holding(
            Some(t + Duration::from_millis(20)),
            t + Duration::from_millis(19)
        ));
        assert!(!holding(
            Some(t + Duration::from_millis(20)),
            t + Duration::from_millis(20)
        ));
        assert!(!holding(None, t));
    }

    #[test]
    fn a_due_time_is_relative_in_100_ns_units_and_never_zero() {
        let t = Instant::now();
        assert_eq!(due_time(t, t + Duration::from_millis(8)), -80_000);
        assert_eq!(due_time(t, t + Duration::from_nanos(8_333_333)), -83_333);
        // Due already, or in under 100 ns: 100 ns from now, relative like the rest
        assert_eq!(due_time(t + Duration::from_millis(1), t), -1);
        assert_eq!(due_time(t, t + Duration::from_nanos(50)), -1);
    }

    #[test]
    fn a_window_has_one_wake_its_latest() {
        let t = Instant::now();
        let mut wakes = Wakes::default();
        wakes.wake(1, t + Duration::from_millis(5));
        wakes.wake(1, t + Duration::from_millis(9));
        assert!(wakes.take_due(t + Duration::from_millis(5)).is_empty());
        assert_eq!(
            wakes.take_due(t + Duration::from_millis(9)),
            [(1, t + Duration::from_millis(9))]
        );
        assert!(wakes.take_due(t + Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn only_the_due_wakes_are_taken() {
        let t = Instant::now();
        let mut wakes = Wakes::default();
        wakes.wake(1, t + Duration::from_millis(8));
        wakes.wake(2, t + Duration::from_millis(3));
        wakes.wake(3, t + Duration::from_millis(20));
        let due = wakes.take_due(t + Duration::from_millis(8));
        assert_eq!(
            due,
            [
                (1, t + Duration::from_millis(8)),
                (2, t + Duration::from_millis(3))
            ]
        );
        assert_eq!(
            wakes.take_due(t + Duration::from_millis(30)),
            [(3, t + Duration::from_millis(20))]
        );
    }

    #[test]
    fn the_timer_is_set_once_for_the_soonest_wake() {
        let t = Instant::now();
        let mut wakes = Wakes::default();
        assert_eq!(wakes.arm(), Arm::Idle);
        wakes.wake(1, t + Duration::from_millis(8));
        assert_eq!(wakes.arm(), Arm::Set(t + Duration::from_millis(8)));
        // Waiting again before it's due: set already
        assert_eq!(wakes.arm(), Arm::Armed);
        // Another window's, later: the soonest is still the first
        wakes.wake(2, t + Duration::from_millis(12));
        assert_eq!(wakes.arm(), Arm::Armed);
        // A sooner one sets it again
        wakes.wake(3, t + Duration::from_millis(2));
        assert_eq!(wakes.arm(), Arm::Set(t + Duration::from_millis(2)));
        wakes.take_due(t + Duration::from_millis(2));
        assert_eq!(wakes.arm(), Arm::Set(t + Duration::from_millis(8)));
        wakes.take_due(t + Duration::from_millis(20));
        assert_eq!(wakes.arm(), Arm::Idle);
    }

    #[test]
    fn a_wake_put_back_after_its_timer_fired_sets_the_timer_again() {
        // end_paint_holds puts a wake back when its window is busy: its timer fired, and the
        // wait that saw it took the signal, so taking it as set would wait on nothing
        let t = Instant::now();
        let due = t + Duration::from_millis(8);
        let mut wakes = Wakes::default();
        wakes.wake(1, due);
        assert_eq!(wakes.arm(), Arm::Set(due));
        assert_eq!(wakes.take_due(due + Duration::from_micros(300)), [(1, due)]);
        wakes.wake(1, due);
        assert_eq!(wakes.arm(), Arm::Set(due));
    }

    #[test]
    fn a_timer_whose_time_has_come_isnt_taken_as_set_though_nothing_was_due() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        let mut wakes = Wakes::default();
        wakes.wake(1, t + ms(8));
        assert_eq!(wakes.arm(), Arm::Set(t + ms(8)));
        // The loop woke for a message before it: still set
        assert!(wakes.take_due(t + ms(5)).is_empty());
        assert_eq!(wakes.arm(), Arm::Armed);
        // The window's hold moved later before the loop waited again, and the timer fired for
        // nothing: a turn after it finds nothing due
        wakes.wake(1, t + ms(20));
        assert!(wakes.take_due(t + ms(9)).is_empty());
        // Another window's wake at the time the timer had, already due: set again, at once
        wakes.wake(2, t + ms(8));
        assert_eq!(wakes.arm(), Arm::Set(t + ms(8)));
    }

    #[test]
    fn a_timer_that_couldnt_be_set_is_asked_for_again() {
        let t = Instant::now();
        let mut wakes = Wakes::default();
        wakes.wake(1, t + Duration::from_millis(8));
        assert_eq!(wakes.arm(), Arm::Set(t + Duration::from_millis(8)));
        wakes.disarm();
        assert_eq!(wakes.arm(), Arm::Set(t + Duration::from_millis(8)));
        assert_eq!(wakes.arm(), Arm::Armed);
    }

    #[test]
    fn counts_start_at_nothing() {
        assert_eq!(Pace::new().counts(), Counts::default());
    }

    #[test]
    fn each_count_adds_up() {
        let pace = Pace::new();
        pace.turn();
        pace.turn();
        pace.painted(1_000);
        pace.painted(500);
        pace.held();
        let c = pace.counts();
        assert_eq!(
            (c.turns, c.paints, c.paint_cycles, c.held),
            (2, 2, 1_500, 1)
        );
    }

    #[test]
    fn a_hold_more_than_two_ms_past_due_is_late() {
        let pace = Pace::new();
        pace.hold_ended(Duration::from_micros(300));
        pace.hold_ended(LATE_AFTER);
        pace.hold_ended(Duration::from_micros(2_001));
        pace.hold_ended(Duration::from_millis(15));
        let c = pace.counts();
        assert_eq!(c.holds, 4);
        assert_eq!(c.late, 2);
        assert_eq!(c.late_us, 300 + 2_000 + 2_001 + 15_000);
    }

    #[test]
    fn the_counts_serialise_as_the_state_file_names_them() {
        let json = serde_json::to_value(Counts::default()).unwrap();
        let mut names: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "held",
                "holds",
                "late",
                "late_us",
                "paint_cycles",
                "paints",
                "turns"
            ]
        );
    }
}
