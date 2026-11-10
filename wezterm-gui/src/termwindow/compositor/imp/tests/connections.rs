//! A suggested tab connection's light over the stand-in device (lbterm/COMPOSITOR.md, "The
//! compositor's tests"; lbterm/LINKS.md, "Connections"): `prepare_connection_lamps` given a
//! `GlowPlan` as render/connections.rs makes one, and the light's visual read back. Its laps are
//! what DWM was handed, an animation of its height along the line that ends at the line's middle,
//! and between them it's still there by value, with nothing in the tree for DWM to compose
//! every refresh. tab-connections.ps1 judged these in captures until feat/dcomp-recorder.

use super::*;
use crate::termwindow::compositor::{GlowLine, GlowPlan};

/// The light's template, 16 pixels across and tall
const LIGHT: u32 = 16;
/// The pair's line, upright at x 30 from y 100 to 160 in window pixels
const X: f32 = 30.0;
const TOP: f32 = 100.0;
const BOTTOM: f32 = 160.0;
/// A lap's seconds: render/connections.rs gives the line's (lbterm_extras::links::glow_path, 1.8
/// s at the least), a test a short one so its three laps are over in a test's time
const LAP: f64 = 0.2;

/// The plan with the pair's line, the file saying `says` of it: where its light goes through a
/// lap as glow_path makes it, its visual's offsets from the line's middle going down, there and
/// back
fn plan(says: u64) -> GlowPlan {
    let (_, path) = lbterm_extras::links::glow_path(X, (TOP, BOTTOM), (LIGHT, LIGHT), 1.0);
    GlowPlan {
        template: PixelRect::new(0, 0, LIGHT, LIGHT),
        lines: vec![GlowLine {
            id: "c-pair".to_string(),
            says,
            lap: LAP,
            path,
        }],
    }
}

/// Where the light's visual is when the light is at the line's `y`: its template's middle on it
fn at(y: f32) -> f32 {
    y - LIGHT as f32 / 2.0
}
const MIDDLE: f32 = (TOP + BOTTOM) / 2.0;

impl Screen {
    /// A paint's `prepare_connection_lamps` with `plan`, its laps played when `play`
    /// (lbterm_extras::work::loops_play: motion on and the window seen), then the commit
    fn lamps(&self, plan: Option<GlowPlan>, play: bool) {
        let gpu = &self.gpu;
        self.compositor
            .prepare_connection_lamps(&gpu.instance, &gpu.device, &base(), plan, play)
            .expect("prepare_connection_lamps");
        self.commit();
    }

    /// The pair's light's visual, and when its laps end
    fn light(&self) -> Object {
        let lamps = self.compositor.connection_lamps.borrow();
        assert_eq!(lamps.lamps.len(), 1, "one light, the pair's");
        self.object(&lamps.lamps[0].visual)
    }

    fn running(&self) -> bool {
        self.compositor.loops().contains(&"connection.light")
    }
}

/// The light as the suggestion comes: three laps along the line from its middle, down to its
/// bottom and up to its top and back each lap, ending at the middle, carried by DWM (`loops`
/// says `connection.light`); once they're done the next paint sets it still at the middle by
/// value, the tree keeping no animation and `loops` no longer naming it; and the file saying a
/// new why of the pair runs them again (tab-connections.ps1's step 2: the light running along the
/// line, then still at its middle, "loops" with and without it)
#[test]
fn a_suggestion_s_light_runs_three_laps_and_rests_at_the_line_s_middle() {
    let screen = Screen::new();
    screen.lamps(Some(plan(1)), true);
    let light = screen.light();
    let tree = screen.recorder.tree();
    assert!(tree.shown(light), "{}", tree);
    assert_eq!(tree.parent(light), Some(screen.object(&screen.compositor.panel_lights)), "{tree}");
    assert_eq!(tree.value(light, "OffsetX"), Some(X - LIGHT as f32 / 2.0), "{tree}");
    let y = tree.curve(light, "OffsetY").expect("the light's laps").clone();
    let laps = lbterm_extras::glow::LAPS as f64;
    assert!(close(y.ends().unwrap(), laps * LAP), "{:?}, three laps of {LAP}", y.ends());
    assert_eq!(y.lap(), None, "its laps end, not a loop");
    for lap in 0..=lbterm_extras::glow::LAPS {
        let t = lap as f64 * LAP;
        assert!((y.value(t) - at(MIDDLE) as f64).abs() < 0.5, "at {t} s: {}", y.value(t));
    }
    assert!((y.value(LAP / 4.0) - at(BOTTOM) as f64).abs() < 0.5, "a quarter lap on: {}", y.value(LAP / 4.0));
    assert!((y.value(LAP * 0.75) - at(TOP) as f64).abs() < 0.5, "three quarters on: {}", y.value(LAP * 0.75));
    let (low, high) = (0..=300).map(|k| y.value(k as f64 * laps * LAP / 300.0)).fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
    assert!(low >= at(TOP) as f64 - 0.5 && high <= at(BOTTOM) as f64 + 0.5, "off the line: {} to {}", low, high);
    assert!(screen.running(), "{:?}", screen.compositor.loops());

    // Done: still at the middle by value, nothing for DWM to compose
    std::thread::sleep(Duration::from_secs_f64(laps * LAP) + Duration::from_millis(30));
    screen.lamps(Some(plan(1)), true);
    let tree = screen.recorder.tree();
    assert_eq!(screen.light(), light, "the same visual");
    assert_eq!(tree.value(light, "OffsetY"), Some(at(MIDDLE)), "{tree}");
    assert!(!screen.running(), "{:?}", screen.compositor.loops());

    // The file says a new why of the pair: its laps again
    screen.lamps(Some(plan(2)), true);
    let tree = screen.recorder.tree();
    let again = screen.light();
    let y = tree.curve(again, "OffsetY").expect("its laps again");
    assert!(close(y.ends().unwrap(), laps * LAP), "{:?}", y.ends());
    assert!(screen.running());

    // No suggestion: no light
    screen.lamps(None, true);
    assert!(!screen.recorder.tree().shown(again));
    assert!(!screen.running());
}

/// With motion off (or the window unseen) the light is still at the line's middle from the
/// start, by value, and nothing runs (tab-connections.ps1's step 4)
#[test]
fn with_motion_off_a_suggestion_s_light_is_still_at_the_line_s_middle() {
    let screen = Screen::new();
    screen.lamps(Some(plan(1)), false);
    let light = screen.light();
    let tree = screen.recorder.tree();
    assert!(tree.shown(light), "{}", tree);
    assert_eq!(tree.value(light, "OffsetY"), Some(at(MIDDLE)), "{tree}");
    assert!(!screen.running(), "{:?}", screen.compositor.loops());
}
