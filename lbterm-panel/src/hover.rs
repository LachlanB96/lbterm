//! When the panel opens and closes, and how its light answers the pointer
//! meanwhile.
//!
//! It opens once the pointer has rested on the rail for `open_delay`, so a
//! pointer passing over it on the way somewhere else doesn't fling it open,
//! and closes `close_delay` after the pointer leaves it, so overshooting
//! its edge by a few pixels doesn't snap it shut. The delays hold back the
//! edge, never the answer: the light goes to [`Light::Pending`] the moment
//! the pointer arrives or leaves, and back the moment it changes its mind
//! (lbterm/DESIGN.md, principle 1).
//!
//! Leaving while it's still opening turns it round at once, since it
//! wasn't in use yet; coming back while it's still closing opens it again
//! at once, since the pointer was just there. How long each takes on
//! screen comes from [`Hover::set_motion`]; without motion neither applies.
//!
//! The window strip's detail card and prompt tooltip wait the same way
//! (wezterm-gui's render/strip.rs).

use std::time::{Duration, Instant};

/// How lit the panel is. Its ring and glow follow this, whatever its edge
/// is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    /// Closed, with the pointer elsewhere
    Rest,
    /// Waiting on the pointer: resting on the closed rail, or gone from
    /// the open panel
    Pending,
    /// Open, with the pointer on it
    Full,
}

impl Light {
    /// How far the ring's light and glow are up, 0 to 1.
    pub fn level(self) -> f64 {
        match self {
            Light::Rest => 0.0,
            Light::Pending => crate::palette::PENDING,
            Light::Full => 1.0,
        }
    }
}

/// Where the panel is between shut and open: waiting out its rest before it opens, or its grace
/// before it closes, are phases of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Closed,
    /// The pointer is on the rail: it opens at [`Hover::deadline`] unless it leaves
    Opening,
    Open,
    /// The pointer left: it closes at [`Hover::deadline`] unless it comes back
    Closing,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Closed => "closed",
            Phase::Opening => "opening",
            Phase::Open => "open",
            Phase::Closing => "closing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Shut, or shutting on screen until `reopen` (a pointer back by then
    /// opens it again at once)
    Closed { reopen: Option<Instant> },
    /// The pointer is on the rail; opens at the instant unless it leaves
    Opening(Instant),
    /// Open, its edge still on its way out until `settled` (a pointer
    /// leaving before then shuts it at once)
    Open { settled: Instant },
    /// The pointer left; closes at the instant unless it comes back
    Closing { at: Instant, settled: Instant },
}

#[derive(Debug, Clone)]
pub struct Hover {
    state: State,
    open_delay: Duration,
    close_delay: Duration,
    /// How long opening and closing take on screen
    opening: Duration,
    closing: Duration,
}

impl Hover {
    pub fn new(open_delay: Duration, close_delay: Duration) -> Self {
        Self {
            state: State::Closed { reopen: None },
            open_delay,
            close_delay,
            opening: Duration::ZERO,
            closing: Duration::ZERO,
        }
    }

    pub fn set_delays(&mut self, open_delay: Duration, close_delay: Duration) {
        self.open_delay = open_delay;
        self.close_delay = close_delay;
    }

    /// How long opening and closing take on screen: zero with motion off.
    pub fn set_motion(&mut self, opening: Duration, closing: Duration) {
        self.opening = opening;
        self.closing = closing;
    }

    /// Whether the panel is open, or still open while it waits to close.
    pub fn is_open(&self) -> bool {
        matches!(self.state, State::Open { .. } | State::Closing { .. })
    }

    /// Whether it's still closing on screen at `now`, so a pointer coming
    /// back onto what's left of it opens it again at once: the open
    /// panel's area counts as inside until then.
    pub fn reopens(&self, now: Instant) -> bool {
        matches!(self.state, State::Closed { reopen: Some(until) } if now < until)
    }

    pub fn light(&self) -> Light {
        match self.state {
            State::Closed { .. } => Light::Rest,
            State::Opening(_) | State::Closing { .. } => Light::Pending,
            State::Open { .. } => Light::Full,
        }
    }

    /// What shows: whether it's open, and its light.
    fn shown(&self) -> (bool, Light) {
        (self.is_open(), self.light())
    }

    /// The pointer is over the panel (`inside`) or not at `now`. Inside
    /// means over the rail while closed, and anywhere on the open panel
    /// once open. True if what it shows changed: it opened or closed, or
    /// its light moved.
    pub fn pointer(&mut self, inside: bool, now: Instant) -> bool {
        let before = self.shown();
        self.state = match (self.state, inside) {
            (State::Closed { reopen: Some(until) }, true) if now < until => State::Open {
                settled: now + self.opening,
            },
            (State::Closed { .. }, true) => State::Opening(now + self.open_delay),
            (State::Opening(_), false) => State::Closed { reopen: None },
            (State::Open { settled }, false) if now < settled => State::Closed {
                reopen: Some(now + self.closing),
            },
            (State::Open { settled }, false) => State::Closing {
                at: now + self.close_delay,
                settled,
            },
            (State::Closing { settled, .. }, true) => State::Open { settled },
            (state, _) => state,
        };
        self.tick(now);
        self.shown() != before
    }

    /// The pointer left the window: close as if it had left the panel.
    pub fn left_window(&mut self, now: Instant) -> bool {
        self.pointer(false, now)
    }

    /// Apply whatever was due by `now`. True if the panel opened or closed.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.state {
            State::Opening(at) if at <= now => {
                self.state = State::Open {
                    settled: now + self.opening,
                };
                true
            }
            State::Closing { at, .. } if at <= now => {
                self.state = State::Closed {
                    reopen: Some(now + self.closing),
                };
                true
            }
            _ => false,
        }
    }

    /// Where it is between shut and open, for LBTerm's state file (lbterm/STATE.md).
    pub fn phase(&self) -> Phase {
        match self.state {
            State::Closed { .. } => Phase::Closed,
            State::Opening(_) => Phase::Opening,
            State::Open { .. } => Phase::Open,
            State::Closing { .. } => Phase::Closing,
        }
    }

    /// When [`Hover::tick`] has something to do next.
    pub fn deadline(&self) -> Option<Instant> {
        match self.state {
            State::Opening(at) | State::Closing { at, .. } => Some(at),
            State::Closed { .. } | State::Open { .. } => None,
        }
    }

    /// Shut at once, as when the panel is turned off or a click dismisses
    /// it: a pointer coming back waits to open it as usual.
    pub fn close(&mut self) -> bool {
        let was_open = self.is_open();
        self.state = State::Closed { reopen: None };
        was_open
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The panel's defaults, without motion
    fn hover() -> (Hover, Instant) {
        (Hover::new(ms(150), ms(300)), Instant::now())
    }

    /// With the panel's motion: 220 ms out, 150 ms back
    fn moving() -> (Hover, Instant) {
        let (mut h, t) = hover();
        h.set_motion(ms(220), ms(150));
        (h, t)
    }

    #[test]
    fn the_phase_names_the_waits() {
        let (mut h, t) = moving();
        assert_eq!(h.phase(), Phase::Closed);
        h.pointer(true, t);
        assert_eq!(h.phase(), Phase::Opening);
        h.tick(t + ms(150));
        assert_eq!(h.phase(), Phase::Open);
        h.pointer(false, t + ms(1000));
        assert_eq!(h.phase(), Phase::Closing);
        h.tick(t + ms(1300));
        assert_eq!(h.phase(), Phase::Closed);
        assert_eq!(Phase::Opening.name(), "opening");
    }

    #[test]
    fn opens_after_resting_on_the_rail() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        assert_eq!(h.deadline(), Some(t + ms(150)));
        assert!(!h.tick(t + ms(149)));
        assert!(!h.is_open());
        assert!(h.tick(t + ms(150)));
        assert!(h.is_open());
        assert_eq!(h.deadline(), None);
    }

    #[test]
    fn the_light_answers_the_moment_the_pointer_arrives() {
        let (mut h, t) = hover();
        assert_eq!(h.light(), Light::Rest);
        assert!(h.pointer(true, t), "arriving changes what shows");
        assert_eq!(h.light(), Light::Pending);
        assert!(!h.is_open(), "the edge still waits");
        h.tick(t + ms(150));
        assert_eq!(h.light(), Light::Full);
    }

    #[test]
    fn passing_over_the_rail_does_not_open_it() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        assert!(h.pointer(false, t + ms(60)), "the light goes back");
        assert_eq!(h.light(), Light::Rest);
        assert!(!h.tick(t + ms(500)));
        assert!(!h.is_open());
        assert_eq!(h.deadline(), None);
    }

    #[test]
    fn moving_about_on_the_rail_does_not_restart_the_wait() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        assert!(!h.pointer(true, t + ms(100)), "nothing new to show");
        assert!(h.pointer(true, t + ms(150)), "opened on the first move after the delay");
    }

    #[test]
    fn closes_after_the_pointer_leaves() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.pointer(false, t + ms(1000));
        assert!(h.is_open(), "still open while it waits");
        assert!(h.tick(t + ms(1300)));
        assert!(!h.is_open());
        assert_eq!(h.light(), Light::Rest);
    }

    #[test]
    fn the_light_answers_the_moment_the_pointer_leaves() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        assert!(h.pointer(false, t + ms(1000)), "leaving changes what shows");
        assert_eq!(h.light(), Light::Pending);
        assert!(h.is_open());
    }

    #[test]
    fn coming_back_in_time_keeps_it_open_and_relights_it() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.pointer(false, t + ms(1000));
        assert!(h.pointer(true, t + ms(1200)), "coming back changes what shows");
        assert_eq!(h.light(), Light::Full);
        assert!(!h.tick(t + ms(2000)));
        assert!(h.is_open());
    }

    #[test]
    fn leaving_while_it_opens_turns_it_round_at_once() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        assert!(h.pointer(false, t + ms(250)), "100 ms into its 220");
        assert!(!h.is_open());
        assert_eq!(h.light(), Light::Rest);
        assert_eq!(h.deadline(), None);
    }

    #[test]
    fn once_settled_open_leaving_waits_out_the_grace() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.pointer(false, t + ms(370));
        assert!(h.is_open(), "its 220 ms were up");
        assert_eq!(h.deadline(), Some(t + ms(670)));
    }

    #[test]
    fn coming_back_while_it_closes_opens_it_at_once() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.pointer(false, t + ms(1000));
        assert!(h.tick(t + ms(1300)), "closing");
        assert!(h.pointer(true, t + ms(1400)), "100 ms into its 150");
        assert!(h.is_open(), "no wait on the rail");
        assert_eq!(h.light(), Light::Full);
    }

    #[test]
    fn it_reopens_only_while_it_closes_on_screen() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        assert!(!h.reopens(t + ms(200)), "open");
        h.pointer(false, t + ms(1000));
        h.tick(t + ms(1300));
        assert!(h.reopens(t + ms(1449)));
        assert!(!h.reopens(t + ms(1450)));
        h.close();
        assert!(!h.reopens(t + ms(1301)), "dismissed");
    }

    #[test]
    fn coming_back_once_it_has_closed_waits_again() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.pointer(false, t + ms(1000));
        h.tick(t + ms(1300));
        h.pointer(true, t + ms(1450));
        assert!(!h.is_open());
        assert_eq!(h.light(), Light::Pending);
        assert_eq!(h.deadline(), Some(t + ms(1600)));
    }

    #[test]
    fn turned_round_it_can_be_turned_back() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.pointer(false, t + ms(250));
        assert!(h.pointer(true, t + ms(300)), "back while it shuts");
        assert!(h.is_open());
        assert!(h.pointer(false, t + ms(350)), "and out while it opens");
        assert!(!h.is_open());
    }

    #[test]
    fn a_dismissed_panel_waits_to_open_again() {
        let (mut h, t) = moving();
        h.pointer(true, t);
        h.tick(t + ms(150));
        assert!(h.close());
        h.pointer(true, t + ms(200));
        assert!(!h.is_open());
        assert_eq!(h.light(), Light::Pending);
    }

    #[test]
    fn no_delays_open_and_close_at_once() {
        let mut h = Hover::new(Duration::ZERO, Duration::ZERO);
        let t = Instant::now();
        assert!(h.pointer(true, t));
        assert!(h.is_open());
        assert!(h.pointer(false, t));
        assert!(!h.is_open());
    }

    #[test]
    fn leaving_the_window_closes_it_too() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.left_window(t + ms(200));
        assert!(h.tick(t + ms(500)));
        assert!(!h.is_open());
        assert!(!h.close());
    }
}
