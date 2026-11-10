//! The light that runs along the tab panel's rail from a tab's pad to the Conductor's as Lachlan
//! hides the tab from the Conductor or shows it to it again (lbterm/NEW-TAB.md, section 8; his 6a
//! of 2026-11-01: a request going from the tab to the Conductor, on a 600 ms token of its own,
//! `send`, since a 120 ms wave couldn't be seen and he kept a 0.8 s one).
//!
//! It's the pad's glint sent on: a piece of the lights' white swatch the size of the glint, up to
//! the press's white over the tab's pad on `copy_in` as it sets off, carried up the rail's column
//! on `send` to the Conductor's pad, held at that white all the way, and out over the Conductor's
//! pad on `copy_out` once it's there. `send` is the panel sweep's curve, an even pace from the
//! start (slope 1, so no ease-in) slowing to a soft stop: a light that runs, rather than one that
//! jumps most of the way and then creeps, as an ease-out would make it. The Conductor's word comes
//! back later as the tab's own pad's glint (render/conductor_hide.rs), so a hide reads as a request
//! leaving the tab and an answer coming back to it.
//!
//! One visual, its two offsets and its opacity given to DWM in one commit (wezterm-gui's
//! compositor.rs, `Light::Send`), so nothing runs on the GUI thread while it plays. Each pick sends
//! a light of its own: a second request is a second light, never a retarget of the first. With
//! motion off there's none: what a hide changes is the row's look, there in the next frame.

use crate::curve::Curve;
use crate::menu::LIGHT_EPSILON;
use crate::motion::{Cubic, Motion, Segments};

/// The light's way and its light: its top left corner from over the tab's pad to over the
/// Conductor's (`x`, `y`, on `send`), and its opacity, up on `copy_in`, held while it runs and out
/// on `copy_out` once it's there
#[derive(Debug, Clone, PartialEq)]
pub struct Send {
    pub x: Motion,
    pub y: Motion,
    pub opacity: Segments,
}

impl Send {
    /// When it has run and gone out, in seconds from its start: when its visual can be put away
    pub fn end(&self) -> f64 {
        self.opacity.end
    }

    /// When it reaches the Conductor's pad, within `epsilon` pixels, in seconds from its start
    pub fn arrives(&self, epsilon: f64) -> f64 {
        self.x.end(epsilon).max(self.y.end(epsilon))
    }
}

/// The light from `from` (the tab's pad's glint's top left corner, in pixels) to `to` (the
/// Conductor's), at `peak` (the press's white) while it runs, its opacity fitted every `step`
/// seconds; `None` with motion off. Every pad on the rail is the same size (lbterm_panel's
/// `Layout::pad_in`), so only the corner moves. With `send` set to `none` it's on the Conductor's
/// pad at once and glints there
pub fn send(
    enabled: bool,
    from: (f64, f64),
    to: (f64, f64),
    peak: f64,
    travel: &Curve,
    copy_in: &Curve,
    copy_out: &Curve,
    step: f64,
    pixel_epsilon: f64,
) -> Option<Send> {
    if !enabled {
        return None;
    }
    let x = Motion::new(from.0, to.0, 0.0, travel.clone());
    let y = Motion::new(from.1, to.1, 0.0, travel.clone());
    let arrives = x.end(pixel_epsilon).max(y.end(pixel_epsilon));
    let rise = Motion::new(0.0, peak, 0.0, copy_in.clone()).segments(step, LIGHT_EPSILON);
    // Out once it's there, never before it has come all the way up
    let out_at = rise.end.max(arrives);
    let fall = Motion::new(peak, 0.0, 0.0, copy_out.clone()).segments(step, LIGHT_EPSILON);
    let mut cubics = rise.cubics;
    if out_at > rise.end {
        // Held at its peak while it runs
        cubics.push(Cubic {
            begin: rise.end,
            coefficients: [peak as f32, 0.0, 0.0, 0.0],
        });
    }
    cubics.extend(fall.cubics.iter().map(|c| Cubic {
        begin: c.begin + out_at,
        coefficients: c.coefficients,
    }));
    Some(Send {
        x,
        y,
        opacity: Segments {
            cubics,
            end: out_at + fall.end,
            end_value: 0.0,
        },
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Easing;

    const STEP: f64 = 1.0 / 60.0;
    const PIXEL: f64 = 0.5;

    fn travel() -> Curve {
        Curve::Timed { duration: 0.6, easing: Easing::CubicBezier(0.33, 0.33, 0.4, 1.0) }
    }
    fn copy_in() -> Curve {
        Curve::Timed { duration: 0.18, easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0) }
    }
    fn copy_out() -> Curve {
        Curve::Timed { duration: 0.26, easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0) }
    }
    fn lit(from: (f64, f64), to: (f64, f64)) -> Send {
        send(true, from, to, 0.35, &travel(), &copy_in(), &copy_out(), STEP, PIXEL).expect("a light")
    }

    #[test]
    fn it_runs_from_the_tabs_pad_to_the_conductors_on_send() {
        // Row 4's pad up to the Conductor's, in the rail's column: x stays, y climbs
        let s = lit((14.0, 262.0), (14.0, 48.0));
        assert_eq!((s.x.from, s.x.to), (14.0, 14.0));
        assert_eq!((s.y.from, s.y.to), (262.0, 48.0));
        assert_eq!(s.y.curve, travel());
        // There at send's end, never past it, and only ever on its way up
        assert!((s.arrives(PIXEL) - 0.6).abs() < 1e-6, "arrives at {}", s.arrives(PIXEL));
        let mut was = s.y.value(0.0);
        assert_eq!(was, 262.0);
        for i in 1..=60 {
            let at = s.y.value(i as f64 * 0.01);
            assert!(at <= was + 1e-9 && at >= 48.0 - 1e-9, "{at} after {was} at {} s", i as f64 * 0.01);
            was = at;
        }
        assert!((s.y.value(0.6) - 48.0).abs() < 1e-6);
    }

    #[test]
    fn it_runs_rather_than_jumping_and_creeping() {
        // The sweep's curve: an even pace from the start and a soft stop, so at half its time it
        // has come more than half the way and well short of all of it (an ease-out like copy_in's
        // would be nine tenths there)
        let s = lit((14.0, 262.0), (14.0, 48.0));
        let share = |t: f64| (262.0 - s.y.value(t)) / 214.0;
        assert!(share(0.3) > 0.55 && share(0.3) < 0.85, "at half its time: {}", share(0.3));
        // It sets off at once: a tenth of the way within the first 60 ms
        assert!(share(0.06) > 0.08, "after 60 ms: {}", share(0.06));
    }

    #[test]
    fn it_lights_as_it_sets_off_holds_while_it_runs_and_goes_out_on_the_conductors_pad() {
        let s = lit((14.0, 262.0), (14.0, 48.0));
        let o = |t: f64| s.opacity.value(t) as f64;
        assert!(o(0.0).abs() < 1e-6, "from nothing: {}", o(0.0));
        // Up to the press's white by copy_in's end, and there all the way up
        for t in [0.18, 0.3, 0.45, 0.59] {
            assert!((o(t) - 0.35).abs() < 0.01, "{} at {t} s", o(t));
        }
        // Out on copy_out once it's there: lit at first, most of it gone halfway, none at the end
        assert!(o(0.6) > 0.34, "just there: {}", o(0.6));
        assert!(o(0.62) < o(0.6), "going out: {}", o(0.62));
        assert!(o(0.73) < 0.1, "halfway out: {}", o(0.73));
        assert!((s.end() - 0.86).abs() < 1e-6, "ends at {}", s.end());
        assert_eq!(s.opacity.end_value, 0.0);
        assert_eq!(o(0.9), 0.0);
        // Never above its white
        for i in 0..=90 {
            assert!(o(i as f64 * 0.01) <= 0.35 + 1e-3);
        }
    }

    #[test]
    fn a_short_way_still_lights_before_it_goes_out() {
        // The first row, just under the Conductor's: the same time to run, so the same light
        let s = lit((14.0, 96.0), (14.0, 48.0));
        assert!((s.arrives(PIXEL) - 0.6).abs() < 1e-6);
        assert!((s.end() - 0.86).abs() < 1e-6);
        // Instant travel: on the Conductor's pad at once, out once it's lit
        let s = send(true, (14.0, 96.0), (14.0, 48.0), 0.35, &Curve::Instant, &copy_in(), &copy_out(), STEP, PIXEL).unwrap();
        assert_eq!(s.y.value(0.0), 48.0);
        assert!((s.end() - 0.44).abs() < 1e-6, "lit and out: {}", s.end());
        assert!((s.opacity.value(0.18) as f64 - 0.35).abs() < 0.01);
    }

    #[test]
    fn slowed_it_keeps_its_shape() {
        let slow = |c: Curve| c.slowed(5.0);
        let s = send(true, (14.0, 262.0), (14.0, 48.0), 0.35, &slow(travel()), &slow(copy_in()), &slow(copy_out()), STEP, PIXEL).unwrap();
        assert!((s.arrives(PIXEL) - 3.0).abs() < 1e-6);
        assert!((s.end() - 4.3).abs() < 1e-6);
        let fast = lit((14.0, 262.0), (14.0, 48.0));
        assert!((s.y.value(1.5) - fast.y.value(0.3)).abs() < 1e-6);
    }

    #[test]
    fn with_motion_off_theres_none() {
        assert!(send(false, (14.0, 262.0), (14.0, 48.0), 0.35, &travel(), &copy_in(), &copy_out(), STEP, PIXEL).is_none());
    }
}
