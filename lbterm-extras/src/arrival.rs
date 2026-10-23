//! When the keyboard arrives in a window from outside it, so the active pane's outline can
//! answer with a pulse (lbterm/DESIGN.md, `arrive_in` and `arrive_out`): the window takes the
//! keyboard back (the summon key, a toast's click, Alt+Tab, a click into it), or, while it has
//! the keyboard, the pane with it changes with no key or click in the window to do it (AI
//! Dashboard or Conductor activating a pane through the mux, a pane that closed by itself).
//!
//! Each arrival counts one up; the compositor pulses when the count changes.

use std::time::{Duration, Instant};

/// A window's first moments: its first focus isn't an arrival, since nothing animates as
/// LBTerm starts
const SETTLE: Duration = Duration::from_millis(1500);
/// A change of pane this soon after a key or a click in the window is that key's or click's
const QUIET: Duration = Duration::from_millis(1000);

#[derive(Debug)]
pub struct Arrival<Id> {
    created: Instant,
    focused: bool,
    active: Option<Id>,
    input: Option<Instant>,
    count: u64,
}

impl<Id: Copy + PartialEq> Arrival<Id> {
    pub fn new(now: Instant) -> Self {
        Self {
            created: now,
            focused: false,
            active: None,
            input: None,
            count: 0,
        }
    }

    /// The window took the keyboard, or lost it.
    pub fn focus(&mut self, focused: bool, now: Instant) {
        if focused && !self.focused && now.saturating_duration_since(self.created) >= SETTLE {
            self.count += 1;
        }
        self.focused = focused;
    }

    /// A key, or a click, in the window.
    pub fn input(&mut self, now: Instant) {
        self.input = Some(now);
    }

    /// The pane with the keyboard this frame; the arrivals so far.
    pub fn frame(&mut self, active: Option<Id>, now: Instant) -> u64 {
        let moved = matches!((self.active, active), (Some(was), Some(is)) if was != is);
        let quiet = self
            .input
            .map_or(true, |at| now.saturating_duration_since(at) >= QUIET);
        if moved
            && self.focused
            && quiet
            && now.saturating_duration_since(self.created) >= SETTLE
        {
            self.count += 1;
        }
        if active.is_some() {
            self.active = active;
        }
        self.count
    }

    /// Whether the window has the keyboard.
    pub fn focused(&self) -> bool {
        self.focused
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn after(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// A window up for a while, with the keyboard, on pane 1
    fn settled() -> (Arrival<u32>, Instant) {
        let start = Instant::now();
        let mut a = Arrival::new(start);
        a.focus(true, start);
        a.frame(Some(1), after(start, 16));
        (a, after(start, 5000))
    }

    #[test]
    fn starting_up_is_not_an_arrival() {
        let start = Instant::now();
        let mut a = Arrival::new(start);
        a.focus(true, after(start, 100));
        assert_eq!(a.frame(Some(1), after(start, 116)), 0);
        // A tab the start-up opens isn't one either
        assert_eq!(a.frame(Some(2), after(start, 400)), 0);
        assert!(a.focused());
    }

    #[test]
    fn taking_the_keyboard_back_is_an_arrival() {
        let (mut a, now) = settled();
        a.focus(false, now);
        assert!(!a.focused());
        assert_eq!(a.frame(Some(1), after(now, 16)), 0);
        a.focus(true, after(now, 2000));
        assert_eq!(a.frame(Some(1), after(now, 2016)), 1);
        // Focus said twice is still one arrival
        a.focus(true, after(now, 2020));
        assert_eq!(a.frame(Some(1), after(now, 2036)), 1);
    }

    #[test]
    fn a_pane_changing_with_no_key_or_click_is_an_arrival() {
        let (mut a, now) = settled();
        assert_eq!(a.frame(Some(2), now), 1);
        // The same pane again is nothing new
        assert_eq!(a.frame(Some(2), after(now, 16)), 1);
    }

    #[test]
    fn a_pane_changing_after_a_key_is_that_keys() {
        let (mut a, now) = settled();
        a.input(now);
        assert_eq!(a.frame(Some(2), after(now, 40)), 0);
        // A spawn the server took a while over is still the key's
        assert_eq!(a.frame(Some(3), after(now, 900)), 0);
        // Long after it, the keyboard moved by itself
        assert_eq!(a.frame(Some(4), after(now, 2500)), 1);
    }

    #[test]
    fn a_pane_changing_in_a_window_without_the_keyboard_waits_for_it() {
        let (mut a, now) = settled();
        a.focus(false, now);
        assert_eq!(a.frame(Some(2), after(now, 100)), 0);
        // A toast's click brings the window forward on a new pane: the count moves on in the
        // one frame, which is one pulse
        a.focus(true, after(now, 3000));
        assert!(a.frame(Some(3), after(now, 3016)) > 0);
    }

    #[test]
    fn frames_without_an_active_pane_keep_the_last() {
        let (mut a, now) = settled();
        assert_eq!(a.frame(None, now), 0);
        assert_eq!(a.frame(Some(1), after(now, 16)), 0);
    }
}
