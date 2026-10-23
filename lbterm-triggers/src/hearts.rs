//! The hearts effect: a handful of pink hearts float up from the bottom-right corner of the
//! pane, swaying, growing a little, and fade out. Where each heart is at a moment is worked
//! out here; the GUI draws them.

use crate::colour::Rgb;
use std::time::Duration;

/// How long a burst lasts, the last heart included
pub const DURATION: Duration = Duration::from_millis(2000);

/// The pinks the hearts come in
const PINKS: [Rgb; 3] = [Rgb(0xff, 0x5c, 0x8a), Rgb(0xff, 0x7e, 0xb6), Rgb(0xff, 0xa3, 0xc9)];

/// One heart in one frame, relative to the corner it rises from, in pixels at 96 DPI
/// (the GUI scales them): x to the left of the corner, y up from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heart {
    pub left: f32,
    pub up: f32,
    /// Width and height
    pub size: f32,
    pub alpha: f32,
    pub colour: Rgb,
}

/// The hearts of a burst `age` after it started. `seed` varies the burst so two in a row
/// don't look the same.
pub fn frame(age: Duration, seed: u32) -> Vec<Heart> {
    const COUNT: u32 = 7;
    const LIFE: f32 = 1.1;
    const STAGGER: f32 = 0.09;
    const RISE: f32 = 150.0;
    let t = age.as_secs_f32();
    let mut hearts = vec![];
    for i in 0..COUNT {
        let r = |k: u32| unit(seed.wrapping_mul(31).wrapping_add(i * 7 + k));
        let start = i as f32 * STAGGER + r(1) * 0.05;
        let life = LIFE * (0.8 + 0.4 * r(2));
        let p = (t - start) / life;
        if !(0.0..1.0).contains(&p) {
            continue;
        }
        // Rise fast, then slow, like something light let go of
        let eased = 1.0 - (1.0 - p) * (1.0 - p);
        let base = 18.0 + r(3) * 70.0;
        let sway = (p * std::f32::consts::TAU * (0.8 + r(4)) + r(5) * 6.0).sin() * (6.0 + 8.0 * r(6));
        let size = (11.0 + 9.0 * r(7)) * (0.7 + 0.45 * eased);
        // In quickly, out over the last 45%
        let alpha = (p / 0.12).min(1.0) * ((1.0 - p) / 0.45).min(1.0);
        hearts.push(Heart {
            left: base + sway,
            up: 10.0 + eased * RISE * (0.75 + 0.5 * r(8)),
            size,
            alpha: alpha.clamp(0.0, 1.0),
            colour: PINKS[(i as usize + seed as usize) % PINKS.len()],
        });
    }
    hearts
}

/// A number in 0..1 from an integer, the same every time (a hash, not a random source)
fn unit(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x % 10_000) as f32 / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn hearts_appear_rise_and_are_all_gone_by_the_end() {
        assert!(!frame(MS * 50, 1).is_empty());
        let early = frame(MS * 200, 1);
        let later = frame(MS * 700, 1);
        let top = |hs: &[Heart]| hs.iter().map(|h| h.up).fold(0.0, f32::max);
        assert!(top(&later) > top(&early) + 40.0);
        assert!(frame(DURATION, 1).is_empty());
        assert!(frame(DURATION + MS * 500, 1).is_empty());
    }

    #[test]
    fn they_fade_out() {
        let near_end = frame(MS * 1450, 3);
        assert!(near_end.iter().all(|h| h.alpha < 0.8), "{near_end:?}");
    }

    #[test]
    fn they_stay_by_the_corner_they_rise_from() {
        for ms in (0..2000).step_by(50) {
            for h in frame(MS * ms, 7) {
                assert!(h.left > 0.0 && h.left < 110.0, "{h:?}");
                assert!(h.up >= 10.0 && h.up < 260.0, "{h:?}");
                assert!(h.size > 5.0 && h.size < 25.0, "{h:?}");
                assert!((0.0..=1.0).contains(&h.alpha));
            }
        }
    }

    #[test]
    fn the_seed_changes_the_burst() {
        assert_ne!(frame(MS * 400, 1), frame(MS * 400, 2));
        assert_eq!(frame(MS * 400, 1), frame(MS * 400, 1));
    }
}
