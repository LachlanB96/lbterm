//! The menus' motion (lbterm/NEW-TAB.md, section 6; lbterm/DESIGN.md, "Menus"): the tab menu,
//! the + menu, the Conductor's, the InMail list and the prompt list, all by these rules, decided
//! here once and played by DWM (wezterm-gui's render/menu_fade.rs and the compositor's menus).
//!
//! - A card fades in on `pane_enter` as it opens and out on `pane_exit` as it closes, from
//!   wherever it is: one opened again while it still fades out comes back from as much of it as
//!   shows ([`fade`]). A card fading out is a picture of its last frame: it takes no input,
//!   since the menu it showed is closed.
//! - Its highlight is a light of its own over the card and its words: a rectangle whose edges
//!   glide from row to row on `hover`, from wherever they are and as fast as they're going, and
//!   whose light fades in and out on `hover` as a row is lit or none is ([`light`]). A highlight
//!   that comes where none showed starts on its row, rather than gliding in from a row long left,
//!   and a card that opens has its row lit at once, the card's own fade carrying it in.
//! - With motion off every step lands at its end in the next frame, and once a motion has played
//!   to its end nothing is left running.

use crate::{Curve, Motion};

/// A light's smallest step worth playing: half of one level of 255
pub const LIGHT_EPSILON: f64 = 0.5 / 255.0;

/// Where one number is now: its value, how fast it's going, and where it's headed
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Now {
    pub value: f64,
    pub velocity: f64,
    pub rest: f64,
}

impl Now {
    /// At rest at `value`
    pub fn at(value: f64) -> Self {
        Now { value, velocity: 0.0, rest: value }
    }
}

/// What one number does this frame
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Goes on as it was: at rest, or on its way to where it's still wanted
    Keep,
    /// To this at once
    Set(f64),
    /// Along this, from now
    Play(Motion),
}

impl Step {
    /// Where it ends up
    pub fn rest(&self, was: f64) -> f64 {
        match self {
            Step::Keep => was,
            Step::Set(v) => *v,
            Step::Play(m) => m.to,
        }
    }
}

/// The card's opacity this frame. `was_open`: which way it went last frame (open, or closing or
/// gone); `open`: which way it goes now. Opening plays `enter` from wherever it is to 1, closing
/// `exit` to 0, and a card going the way it went keeps on.
pub fn fade(now: Now, was_open: bool, open: bool, enabled: bool, enter: &Curve, exit: &Curve) -> Step {
    if open == was_open {
        return Step::Keep;
    }
    let to = if open { 1.0 } else { 0.0 };
    if !enabled {
        return Step::Set(to);
    }
    let curve = if open { enter } else { exit };
    // An opacity fade starts still: a card turned round mid-fade goes from where it is
    Step::Play(Motion::new(now.value, to, 0.0, curve.clone()))
}

/// The highlight as it is: its edges (left, top, right, bottom, in its card's visual) and its
/// light (0 for none, else the level it's lit to)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub edges: [Now; 4],
    pub lit: Now,
}

impl Light {
    /// Nothing lit, nowhere
    pub fn none() -> Self {
        Light { edges: [Now::at(0.0); 4], lit: Now::at(0.0) }
    }
}

/// What the highlight does this frame
#[derive(Debug, Clone, PartialEq)]
pub struct LightPlan {
    pub edges: [Step; 4],
    pub lit: Step,
}

/// The highlight this frame. `want`: the row lit now (left, top, right, bottom), if any; `level`:
/// the light it's lit to; `opening`: the card turned toward open this frame (it opened, or came
/// back while it faded out), so its row is lit at once and the card's fade carries it in.
pub fn light(was: &Light, want: Option<[f64; 4]>, level: f64, opening: bool, enabled: bool, hover: &Curve) -> LightPlan {
    let keep_edges = || [Step::Keep, Step::Keep, Step::Keep, Step::Keep];
    let at_once = !enabled || opening;
    let Some(want) = want else {
        // Nothing lit: the light goes out where it is
        let lit = if was.lit.rest == 0.0 && was.lit.value <= LIGHT_EPSILON {
            Step::Keep
        } else if at_once {
            Step::Set(0.0)
        } else if was.lit.rest == 0.0 {
            Step::Keep
        } else {
            Step::Play(Motion::new(was.lit.value, 0.0, was.lit.velocity, hover.clone()))
        };
        return LightPlan { edges: keep_edges(), lit };
    };
    // Something of the light shows now: the edges glide from where they are; else the light comes
    // up on its row
    let shows = was.lit.value > LIGHT_EPSILON || was.lit.rest > 0.0;
    let edges: [Step; 4] = std::array::from_fn(|i| {
        let (now, to) = (was.edges[i], want[i]);
        if at_once || !shows {
            // There already, at rest: nothing to do; else there at once
            if now.rest == to && now.value == to {
                Step::Keep
            } else {
                Step::Set(to)
            }
        } else if now.rest == to {
            Step::Keep
        } else {
            Step::Play(Motion::new(now.value, to, now.velocity, hover.clone()))
        }
    });
    let lit = if at_once {
        if was.lit.rest == level && was.lit.value == level {
            Step::Keep
        } else {
            Step::Set(level)
        }
    } else if was.lit.rest == level {
        Step::Keep
    } else {
        Step::Play(Motion::new(was.lit.value, level, was.lit.velocity, hover.clone()))
    };
    LightPlan { edges, lit }
}

/// A key no row takes (any key an open menu holds, Lachlan's 7b): the card's edge flares once,
/// its hairline and the light round it going white (the compositor's `MenuLamp`), from wherever
/// the flare is up to `peak` on `rise` and back to nothing on `fall`, so a second key while it's
/// still lit lifts it again from there. `rise` is `arrive_in`, so it answers in the next frame;
/// `fall` is `panel_glow`, the light that lingers, so it's seen at real speed (his 9a of
/// 2026-11-08: the hairline alone, 35% of the way to white on `arrive_out`, was one pixel about a
/// fifth brighter for about 150 ms). With motion off there's nothing to play: the key changed
/// nothing, so the menu's end state is the one it shows.
pub fn bump(now: Now, enabled: bool, peak: f64, rise: &Curve, fall: &Curve) -> Option<crate::Chain> {
    enabled.then(|| crate::Chain::pulse(now.value, now.velocity, peak, 0.0, rise.clone(), fall.clone(), LIGHT_EPSILON))
}

/// A row picked in any menu flashes white, as the picker's rows do: from nothing up to `peak` on
/// `copy_in` and back to nothing on `copy_out`, a light of its own over the card, so the card
/// fading out on `pane_exit` as the pick goes doesn't take the flash with it (the card is gone in
/// 120 ms, before `copy_in`'s 180 ms has lit it). With motion off there's nothing to play: the
/// pick's end state is in the next frame, and nothing runs after.
pub fn flash(enabled: bool, peak: f64, copy_in: &Curve, copy_out: &Curve) -> Option<crate::Chain> {
    enabled.then(|| crate::Chain::pulse(0.0, 0.0, peak, 0.0, copy_in.clone(), copy_out.clone(), LIGHT_EPSILON))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Easing, Spring};

    fn enter() -> Curve {
        Curve::Spring(Spring::new(0.16, 0.0))
    }
    fn exit() -> Curve {
        Curve::Timed { duration: 0.12, easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0) }
    }
    fn hover() -> Curve {
        Curve::Spring(Spring::new(0.25, 0.0))
    }
    const LEVEL: f64 = 0.096;

    fn played(step: &Step) -> &Motion {
        match step {
            Step::Play(m) => m,
            other => panic!("expected a motion, got {other:?}"),
        }
    }

    /// The light as `plan` leaves it, `t` seconds on
    fn after(was: &Light, plan: &LightPlan, t: f64) -> Light {
        let one = |now: Now, step: &Step| match step {
            Step::Keep => now,
            Step::Set(v) => Now::at(*v),
            Step::Play(m) => Now { value: m.value(t), velocity: m.velocity(t), rest: m.to },
        };
        Light {
            edges: std::array::from_fn(|i| one(was.edges[i], &plan.edges[i])),
            lit: one(was.lit, &plan.lit),
        }
    }

    #[test]
    fn a_card_fades_in_on_pane_enter_as_it_opens_and_out_on_pane_exit_as_it_closes() {
        let step = fade(Now::at(0.0), false, true, true, &enter(), &exit());
        let m = played(&step);
        assert_eq!((m.from, m.to), (0.0, 1.0));
        assert_eq!(m.curve, enter());
        let step = fade(Now::at(1.0), true, false, true, &enter(), &exit());
        let m = played(&step);
        assert_eq!((m.from, m.to), (1.0, 0.0));
        assert_eq!(m.curve, exit());
        // Going the way it went: nothing new
        assert_eq!(fade(Now::at(1.0), true, true, true, &enter(), &exit()), Step::Keep);
        assert_eq!(fade(Now { value: 0.4, velocity: -3.0, rest: 0.0 }, false, false, true, &enter(), &exit()), Step::Keep);
    }

    #[test]
    fn a_card_opened_again_while_it_fades_out_comes_back_from_where_it_is() {
        // Half way out on pane_exit
        let out = Motion::new(1.0, 0.0, 0.0, exit());
        let t = 0.03;
        let now = Now { value: out.value(t), velocity: out.velocity(t), rest: 0.0 };
        assert!(now.value > 0.2 && now.value < 0.9, "{}", now.value);
        let step = fade(now, false, true, true, &enter(), &exit());
        let back = played(&step);
        assert_eq!(back.from, now.value, "no jump: it starts where it is");
        assert!((back.value(0.0) - now.value).abs() < 1e-9);
        assert_eq!(back.to, 1.0);
        // And closed again while it still comes in
        let now = Now { value: back.value(0.02), velocity: back.velocity(0.02), rest: 1.0 };
        let step = fade(now, true, false, true, &enter(), &exit());
        assert_eq!(played(&step).from, now.value);
    }

    #[test]
    fn with_motion_off_a_card_is_open_or_gone_in_the_next_frame() {
        assert_eq!(fade(Now::at(0.0), false, true, false, &enter(), &exit()), Step::Set(1.0));
        assert_eq!(fade(Now { value: 0.6, velocity: 0.0, rest: 1.0 }, true, false, false, &enter(), &exit()), Step::Set(0.0));
    }

    #[test]
    fn a_fade_ends_on_its_token_and_nothing_runs_after() {
        for (open, curve) in [(true, enter()), (false, exit())] {
            let from = if open { 0.0 } else { 1.0 };
            let step = fade(Now::at(from), !open, open, true, &enter(), &exit());
            let m = played(&step);
            let end = m.end(LIGHT_EPSILON);
            // A spring settles a little after its perceptual time; a timed curve on it
            assert!(end <= curve.duration() * 2.5 && end > 0.0, "{open}: ends at {end}");
            assert!((m.value(end) - m.to).abs() <= LIGHT_EPSILON, "{open}: {} at its end", m.value(end));
            // Once there, the card goes the way it went: no step, so nothing is played again
            assert_eq!(fade(Now::at(m.to), open, open, true, &enter(), &exit()), Step::Keep);
        }
    }

    #[test]
    fn an_open_cards_first_row_is_lit_at_once_and_its_fade_carries_it() {
        let row = [10.0, 40.0, 210.0, 68.0];
        let plan = light(&Light::none(), Some(row), LEVEL, true, true, &hover());
        assert_eq!(plan.edges, [Step::Set(10.0), Step::Set(40.0), Step::Set(210.0), Step::Set(68.0)]);
        assert_eq!(plan.lit, Step::Set(LEVEL));
        // A card opening with nothing lit lights nothing
        let plan = light(&Light::none(), None, LEVEL, true, true, &hover());
        assert_eq!(plan.lit, Step::Keep);
    }

    #[test]
    fn a_highlight_where_none_showed_starts_on_its_row_and_its_light_comes_up_on_hover() {
        // The card is open; the pointer reaches a row, the last lit long ago further up
        let was = Light { edges: [Now::at(10.0), Now::at(8.0), Now::at(210.0), Now::at(36.0)], lit: Now::at(0.0) };
        let row = [10.0, 72.0, 210.0, 100.0];
        let plan = light(&was, Some(row), LEVEL, false, true, &hover());
        assert_eq!(plan.edges, [Step::Keep, Step::Set(72.0), Step::Keep, Step::Set(100.0)], "no glide from a row long left");
        let m = played(&plan.lit);
        assert_eq!((m.from, m.to, m.curve.clone()), (0.0, LEVEL, hover()));
    }

    #[test]
    fn the_highlight_glides_between_rows_on_hover_from_wherever_it_is() {
        let row1 = [10.0, 8.0, 210.0, 36.0];
        let row2 = [10.0, 36.0, 210.0, 64.0];
        let row3 = [10.0, 64.0, 210.0, 92.0];
        let lit = Light { edges: std::array::from_fn(|i| Now::at(row1[i])), lit: Now::at(LEVEL) };
        let plan = light(&lit, Some(row2), LEVEL, false, true, &hover());
        // The sides stay; top and bottom glide, the light stays as it is
        assert_eq!(plan.edges[0], Step::Keep);
        assert_eq!(plan.edges[2], Step::Keep);
        assert_eq!(plan.lit, Step::Keep);
        for i in [1, 3] {
            let m = played(&plan.edges[i]);
            assert_eq!((m.from, m.to, m.curve.clone()), (row1[i], row2[i], hover()));
        }
        // A key moves it on mid-glide: it turns from where it is, at the speed it has
        let mid = after(&lit, &plan, 0.06);
        assert!(mid.edges[1].value > row1[1] && mid.edges[1].value < row2[1], "{:?}", mid.edges[1]);
        assert!(mid.edges[1].velocity > 0.0);
        let plan = light(&mid, Some(row3), LEVEL, false, true, &hover());
        let m = played(&plan.edges[1]);
        assert_eq!(m.from, mid.edges[1].value);
        assert_eq!(m.v0, mid.edges[1].velocity, "its speed carried, never restarted");
        assert_eq!(m.to, row3[1]);
        // It lands on the row exactly, within the spring's settle, and goes no further
        let end = m.end(0.5);
        assert!(end < 0.6, "settles in {end}");
        assert!((m.value(end) - row3[1]).abs() <= 0.5);
        let mut top = m.value(0.0);
        for k in 1..=60 {
            let v = m.value(end * k as f64 / 60.0);
            assert!(v <= row3[1] + 0.5, "no overshoot past its row: {v}");
            assert!(v >= top - 1e-9, "a critically damped glide never turns back");
            top = v;
        }
    }

    #[test]
    fn leaving_the_rows_puts_the_light_out_where_it_is_and_coming_back_glides_from_there() {
        let row1 = [10.0, 8.0, 210.0, 36.0];
        let lit = Light { edges: std::array::from_fn(|i| Now::at(row1[i])), lit: Now::at(LEVEL) };
        let plan = light(&lit, None, LEVEL, false, true, &hover());
        assert_eq!(plan.edges, [Step::Keep, Step::Keep, Step::Keep, Step::Keep]);
        let m = played(&plan.lit);
        assert_eq!((m.from, m.to), (LEVEL, 0.0));
        // Back on another row while some light still shows: it glides there as it comes up
        let fading = after(&lit, &plan, 0.05);
        assert!(fading.lit.value > LIGHT_EPSILON);
        let row2 = [10.0, 36.0, 210.0, 64.0];
        let plan = light(&fading, Some(row2), LEVEL, false, true, &hover());
        assert_eq!(played(&plan.edges[1]).from, row1[1]);
        let m = played(&plan.lit);
        assert_eq!((m.from, m.v0, m.to), (fading.lit.value, fading.lit.velocity, LEVEL));
        // Once out, it stays out: nothing more is played
        let out = Light { lit: Now::at(0.0), ..lit };
        assert_eq!(light(&out, None, LEVEL, false, true, &hover()).lit, Step::Keep);
    }

    #[test]
    fn with_motion_off_the_highlight_is_on_its_row_in_the_next_frame() {
        let row1 = [10.0, 8.0, 210.0, 36.0];
        let row2 = [10.0, 36.0, 210.0, 64.0];
        let lit = Light { edges: std::array::from_fn(|i| Now::at(row1[i])), lit: Now::at(LEVEL) };
        let plan = light(&lit, Some(row2), LEVEL, false, false, &hover());
        assert_eq!(plan.edges, [Step::Keep, Step::Set(36.0), Step::Keep, Step::Set(64.0)]);
        assert_eq!(plan.lit, Step::Keep);
        assert_eq!(light(&lit, None, LEVEL, false, false, &hover()).lit, Step::Set(0.0));
        // Mid-glide when motion went off: every edge lands where it's going
        let gliding = Light { edges: [Now::at(10.0), Now { value: 20.0, velocity: 300.0, rest: 36.0 }, Now::at(210.0), Now { value: 48.0, velocity: 300.0, rest: 64.0 }], lit: Now::at(LEVEL) };
        let plan = light(&gliding, Some(row2), LEVEL, false, false, &hover());
        assert_eq!(plan.edges, [Step::Keep, Step::Set(36.0), Step::Keep, Step::Set(64.0)]);
    }

    #[test]
    fn a_key_no_row_takes_flares_the_cards_edge_once_from_where_it_is() {
        // The defaults: arrive_in to rise, panel_glow to fall
        let arrive_in = Curve::Timed { duration: 0.08, easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0) };
        let panel_glow = Curve::Spring(Spring::new(0.4, 0.0));
        // Out: up to the peak, then back to nothing, and nothing left running
        let chain = bump(Now::at(0.0), true, 1.0, &arrive_in, &panel_glow).expect("a flare");
        assert_eq!(chain.value(0.0), 0.0);
        let peak = (0..=100).map(|i| chain.value(chain.end() * i as f64 / 100.0)).fold(0.0, f64::max);
        assert!((peak - 1.0).abs() < 0.01, "it reaches the peak: {peak}");
        assert_eq!(chain.to(), 0.0);
        assert!(chain.value(chain.end()).abs() <= LIGHT_EPSILON);
        // It answers at once: most of the way up within half of arrive_in
        assert!(chain.value(0.04) > 0.8, "up within 40 ms: {}", chain.value(0.04));
        // And lingers on panel_glow's spring, which leaves its peak slowly: about half of it 100
        // ms after the peak, and still a sixth of it 200 ms after, where arrive_out's 220 ms
        // spring, the hairline's fall until 2026-11-08, has 2% of it left by then
        assert!(chain.value(0.08 + 0.1) > 0.45, "about half of it 100 ms on: {}", chain.value(0.18));
        assert!(chain.value(0.08 + 0.2) > 0.15, "a sixth of it 200 ms on: {}", chain.value(0.28));
        assert!(chain.value(0.08 + 0.4) < 0.02, "all but gone 400 ms on: {}", chain.value(0.48));
        // A second key while it's lit lifts it again from there, not from nothing
        let lit = Now { value: 0.6, velocity: -2.0, rest: 0.0 };
        let again = bump(lit, true, 1.0, &arrive_in, &panel_glow).expect("a flare");
        assert_eq!(again.value(0.0), 0.6);
        // Motion off: nothing to play
        assert!(bump(Now::at(0.0), false, 1.0, &arrive_in, &panel_glow).is_none());
    }

    #[test]
    fn a_picked_row_flashes_up_on_copy_in_and_out_on_copy_out_and_nothing_runs_after() {
        let copy_in = Curve::Timed { duration: 0.18, easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0) };
        let copy_out = Curve::Timed { duration: 0.26, easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0) };
        let chain = flash(true, 0.35, &copy_in, &copy_out).expect("a flash");
        // From nothing, up to the peak at copy_in's end, then down to nothing on copy_out
        assert_eq!(chain.value(0.0), 0.0);
        assert!((chain.value(0.18) - 0.35).abs() < 1e-9, "at its peak once copy_in ends: {}", chain.value(0.18));
        assert!(chain.value(0.09) > 0.3, "copy_in's ease lights it most of the way in its first half");
        assert!(chain.value(0.18 + 0.13) < 0.35 && chain.value(0.18 + 0.13) > 0.0);
        assert_eq!(chain.to(), 0.0);
        // It ends where its two tokens do, at nothing, so the compositor puts it away then
        assert!((chain.end() - 0.44).abs() < 0.01, "ends with copy_out: {}", chain.end());
        assert!(chain.value(chain.end()).abs() <= LIGHT_EPSILON);
        // The card fading out on pane_exit (120 ms) would be gone before the flash lit: it's a
        // light of its own, not the card's
        let card = Motion::new(1.0, 0.0, 0.0, exit());
        assert!(card.end(LIGHT_EPSILON) < 0.18);
        // Motion off: nothing to play, the pick's end state in the next frame
        assert!(flash(false, 0.35, &copy_in, &copy_out).is_none());
    }
}
