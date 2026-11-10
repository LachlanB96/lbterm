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
    /// Held as it showed while a menu's card is over it ([`Hover::hold`]): open or not, and its
    /// light
    held: Option<(bool, Light)>,
    /// What the last paint showed, open or not and its light: each [`Hover::hold`] notes it, one
    /// call at the start of every paint of the panel. A hold takes this, not the state as it is
    /// when the card first paints, which the pointer's rest ending in between may have opened
    /// (PITFALLS 186). `None` after [`Hover::close`] or [`Hover::open_now`], which act at once
    drawn: Option<(bool, Light)>,
}

impl Hover {
    pub fn new(open_delay: Duration, close_delay: Duration) -> Self {
        Self {
            state: State::Closed { reopen: None },
            open_delay,
            close_delay,
            opening: Duration::ZERO,
            closing: Duration::ZERO,
            held: None,
            drawn: None,
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

    /// Whether the panel is open, or still open while it waits to close; while held, whether it
    /// was as the hold began.
    pub fn is_open(&self) -> bool {
        self.held.map_or_else(|| self.live_open(), |(open, _)| open)
    }

    fn live_open(&self) -> bool {
        matches!(self.state, State::Open { .. } | State::Closing { .. })
    }

    /// Hold the panel as it shows now, or let it go (lbterm/FLOWS.md, "A menu, from what opens it
    /// to the end of its fade", step 10): held while a menu's card shows over it, open or fading
    /// out, so nothing of the rail moves through or over the card. While held it neither opens nor
    /// closes and its light stays as it was: the pointer and the delays go on underneath, and let
    /// go it shows where they've got to, opening or closing then. [`Hover::close`] and
    /// [`Hover::open_now`] still act at once, and the hold takes the panel as they leave it. True
    /// if what it shows changed, which only letting go can do.
    ///
    /// It's called once at the start of every paint of the panel, so it also notes what that
    /// paint shows, and a hold starts from what the last paint showed: a right click that opens
    /// a menu just as the pointer's rest ends has the rest's tick (the panel's own wake) run
    /// before the menu's first paint, and the state underneath is open by then though no frame
    /// drew it open; held from there, the rail would open under the new card (PITFALLS 186).
    pub fn hold(&mut self, hold: bool) -> bool {
        let changed = match (hold, self.held) {
            (true, None) => {
                self.held = Some(self.drawn.unwrap_or((self.live_open(), self.live_light())));
                false
            }
            (false, Some(was)) => {
                self.held = None;
                (self.live_open(), self.live_light()) != was
            }
            _ => false,
        };
        self.drawn = Some(self.shown());
        changed
    }

    /// Whether it's held ([`Hover::hold`])
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// Whether it's still closing on screen at `now`, so a pointer coming
    /// back onto what's left of it opens it again at once: the open
    /// panel's area counts as inside until then.
    pub fn reopens(&self, now: Instant) -> bool {
        matches!(self.state, State::Closed { reopen: Some(until) } if now < until)
    }

    pub fn light(&self) -> Light {
        self.held.map_or_else(|| self.live_light(), |(_, light)| light)
    }

    fn live_light(&self) -> Light {
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

    /// Where it is between shut and open as it shows: while held, the held state's (open or
    /// not, and its light: closed with its light pending is waiting out its rest), not the one
    /// underneath, which [`Hover::phase`] says. LBTerm's state file says this one, so a script
    /// waiting on `rail.phase` waits on what the screen shows (lbterm/STATE.md)
    pub fn shown_phase(&self) -> Phase {
        match self.held {
            None => self.phase(),
            Some((false, Light::Pending)) => Phase::Opening,
            Some((false, _)) => Phase::Closed,
            Some((true, Light::Pending)) => Phase::Closing,
            Some((true, _)) => Phase::Open,
        }
    }

    /// Where it is between shut and open underneath, held or not: what its waits are counting
    /// to (lbterm/STATE.md's `waits`).
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

    /// Open at once, as for a press on what opens it, without waiting out
    /// the open delay; from there the pointer closes it as usual. True if
    /// what it shows changed.
    pub fn open_now(&mut self, now: Instant) -> bool {
        let before = self.shown();
        self.state = match self.state {
            State::Closed { .. } | State::Opening(_) => State::Open {
                settled: now + self.opening,
            },
            State::Closing { settled, .. } => State::Open { settled },
            open @ State::Open { .. } => open,
        };
        self.held = None;
        self.drawn = None;
        self.shown() != before
    }

    /// Shut at once, as when the panel is turned off or a click dismisses
    /// it: a pointer coming back waits to open it as usual.
    pub fn close(&mut self) -> bool {
        let was_open = self.is_open();
        self.state = State::Closed { reopen: None };
        self.held = None;
        self.drawn = None;
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

    #[test]
    fn held_under_a_menus_card_the_closed_panel_waits_out_its_rest_and_opens_once_let_go() {
        // A right click on a pad of the closed rail before its rest is up: the menu opens, and
        // the rail waits under its card instead of opening through it
        let (mut h, t) = hover();
        h.pointer(true, t);
        assert_eq!((h.is_open(), h.light()), (false, Light::Pending));
        assert!(!h.hold(true));
        assert!(h.held());
        assert!(h.tick(t + ms(200)), "underneath, the rest is up");
        assert_eq!((h.is_open(), h.light()), (false, Light::Pending), "held as it showed");
        // The card gone, the pointer still on the rail: it opens then
        assert!(h.hold(false));
        assert_eq!((h.is_open(), h.light()), (true, Light::Full));
        assert!(!h.hold(false), "let go once");
    }

    #[test]
    fn held_under_a_menus_card_the_open_panel_stays_until_let_go_and_closes_then() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        assert!(h.is_open());
        h.hold(true);
        // The pointer off the panel and the card: past the close delay it's closed underneath,
        // and still drawn open, at its full light
        h.pointer(false, t + ms(400));
        h.tick(t + ms(800));
        assert_eq!((h.is_open(), h.light()), (true, Light::Full));
        assert!(h.hold(false), "closing once the card has gone");
        assert!(!h.is_open());
    }

    #[test]
    fn a_hold_starts_from_what_the_last_paint_showed_not_from_a_rest_ended_since() {
        // A right click on the closed rail's pad just as its rest is up: the rest's tick runs
        // before the menu's first paint, and the hold keeps the rail as the last frame drew it
        let (mut h, t) = hover();
        h.pointer(true, t);
        assert!(!h.hold(false), "a paint with no card");
        assert!(h.tick(t + ms(150)), "the rest up underneath");
        assert!(h.is_open());
        h.hold(true);
        assert_eq!((h.is_open(), h.light()), (false, Light::Pending), "as the last paint drew it");
        assert_eq!(h.shown_phase(), Phase::Opening);
        assert_eq!(h.phase(), Phase::Open, "underneath");
        assert!(h.hold(false), "it opens once the card has gone");
        assert_eq!(h.shown_phase(), Phase::Open);
    }

    #[test]
    fn a_hold_after_a_close_on_purpose_takes_it_closed() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.hold(false);
        assert!(h.close());
        h.hold(true);
        assert_eq!((h.is_open(), h.shown_phase()), (false, Phase::Closed));
    }

    #[test]
    fn a_pointer_that_comes_back_while_held_changes_nothing_when_let_go() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.hold(true);
        h.pointer(false, t + ms(400));
        h.pointer(true, t + ms(500));
        h.tick(t + ms(900));
        assert!(!h.hold(false));
        assert!(h.is_open());
    }

    #[test]
    fn closing_or_opening_it_on_purpose_acts_at_once_while_held() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        h.tick(t + ms(150));
        h.hold(true);
        assert!(h.close());
        assert!(!h.is_open() && !h.held());
        h.hold(true);
        assert!(h.open_now(t + ms(200)));
        assert!(h.is_open() && !h.held());
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
    fn a_press_opens_it_at_once_and_the_pointer_closes_it_as_usual() {
        let (mut h, t) = hover();
        h.pointer(true, t);
        assert!(h.open_now(t + ms(20)), "pressed before the open delay was out");
        assert!(h.is_open());
        assert_eq!(h.deadline(), None, "no open left pending");
        h.pointer(false, t + ms(500));
        assert!(h.tick(t + ms(800)), "leaving closes it after the close delay");
        assert!(!h.is_open());
        // From shut, and while it waits to close
        let (mut h, t) = hover();
        assert!(h.open_now(t));
        h.pointer(false, t + ms(100));
        assert!(h.open_now(t + ms(150)), "waiting to close, it lights fully again");
        assert!(!h.tick(t + ms(1000)), "the press kept it open");
        assert!(h.is_open());
        assert!(!h.open_now(t + ms(1100)), "already open");
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
