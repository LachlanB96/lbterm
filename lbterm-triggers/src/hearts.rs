//! The hearts effect: a handful of pink hearts float up from the bottom-right corner of the
//! pane, swaying, growing a little, and fade out. Where each heart is at a moment is worked
//! out here ([`Path`]); the GUI draws them. With the compositor DWM plays them: each heart's
//! place, size and opacity over its life are handed over once as the stretches DirectComposition
//! animates ([`Path::tracks`]), so a burst costs the GUI thread a frame or two, not one every
//! refresh for two seconds (lbterm/PERF.md, finding 64).

use crate::colour::Rgb;
use std::f32::consts::TAU;
use std::time::Duration;

/// How long a burst lasts, the last heart included
pub const DURATION: Duration = Duration::from_millis(2000);

/// The pinks the hearts come in
pub const PINKS: [Rgb; 3] = [Rgb(0xff, 0x5c, 0x8a), Rgb(0xff, 0x7e, 0xb6), Rgb(0xff, 0xa3, 0xc9)];

/// The biggest a heart grows, in pixels at 96 DPI: what a heart's picture is drawn at
pub const MAX_SIZE: f32 = 20.0 * 1.15;

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

/// One heart of a burst over its life: rising fast then slow from `start` seconds into the
/// burst for `life` seconds, swaying, growing a little, in fast and out over its last 45%
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Path {
    pub start: f32,
    pub life: f32,
    /// Where it sways about, left of the corner, and how far each way, how fast (radians a
    /// second) and from where in its swing
    base: f32,
    sway: f32,
    omega: f32,
    phase: f32,
    /// How far it rises over its life, from 10 px up
    rise: f32,
    /// Its size, before it grows (to 1.15 of this)
    size: f32,
    /// Which of [`PINKS`]
    pub colour: usize,
}

const COUNT: u32 = 7;
const LIFE: f32 = 1.1;
const STAGGER: f32 = 0.09;
const RISE: f32 = 150.0;
const FIRST_UP: f32 = 10.0;
/// In over the first 12% of its life, out over the last 45%
const IN: f32 = 0.12;
const OUT: f32 = 0.45;

/// The hearts of a burst. `seed` varies the burst so two in a row don't look the same.
pub fn paths(seed: u32) -> Vec<Path> {
    (0..COUNT)
        .map(|i| {
            let r = |k: u32| unit(seed.wrapping_mul(31).wrapping_add(i * 7 + k));
            let life = LIFE * (0.8 + 0.4 * r(2));
            Path {
                start: i as f32 * STAGGER + r(1) * 0.05,
                life,
                base: 18.0 + r(3) * 70.0,
                sway: 6.0 + 8.0 * r(6),
                omega: TAU * (0.8 + r(4)) / life,
                phase: r(5) * 6.0,
                rise: RISE * (0.75 + 0.5 * r(8)),
                size: 11.0 + 9.0 * r(7),
                colour: (i as usize + seed as usize) % PINKS.len(),
            }
        })
        .collect()
}

/// The hearts of a burst `age` after it started
pub fn frame(age: Duration, seed: u32) -> Vec<Heart> {
    let t = age.as_secs_f32();
    paths(seed).iter().filter_map(|p| p.at(t)).collect()
}

/// A stretch of a value as DirectComposition plays it: from `begin` seconds,
/// `c0 + c1 τ + c2 τ² + c3 τ³` with τ the seconds since `begin`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub begin: f64,
    pub coefficients: [f32; 4],
}

/// A value over a heart's life: its pieces in order, then `end_value` from `end` seconds on
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub pieces: Vec<Piece>,
    pub end: f64,
    pub end_value: f32,
}

impl Track {
    /// Its value `t` seconds into the burst
    pub fn at(&self, t: f64) -> f32 {
        if t >= self.end {
            return self.end_value;
        }
        let Some(piece) = self.pieces.iter().rev().find(|p| p.begin <= t).or(self.pieces.first()) else {
            return self.end_value;
        };
        let x = (t - piece.begin).max(0.0) as f32;
        let [c0, c1, c2, c3] = piece.coefficients;
        c0 + x * (c1 + x * (c2 + x * c3))
    }
}

/// A heart's tracks: its centre's x and y (pixels from the corner it rises from, at 96 DPI:
/// x leftward, y upward, as [`Heart`]), its size and its opacity, seconds into the burst
#[derive(Debug, Clone, PartialEq)]
pub struct Tracks {
    pub left: Track,
    pub up: Track,
    pub size: Track,
    pub alpha: Track,
}

/// How often the sway is sampled into cubics: under a hundredth of a pixel from the sine
const SWAY_STEP: f64 = 1.0 / 30.0;

impl Path {
    /// How far through its life it is at `t` seconds into the burst, while it's in the air
    fn progress(&self, t: f32) -> Option<f32> {
        let p = (t - self.start) / self.life;
        (0.0..1.0).contains(&p).then_some(p)
    }

    /// The heart `t` seconds into the burst, while it's in the air
    pub fn at(&self, t: f32) -> Option<Heart> {
        let p = self.progress(t)?;
        // Rise fast, then slow, like something light let go of
        let eased = 1.0 - (1.0 - p) * (1.0 - p);
        let alpha = (p / IN).min(1.0) * ((1.0 - p) / OUT).min(1.0);
        Some(Heart {
            left: self.base + (p * self.life * self.omega + self.phase).sin() * self.sway,
            up: FIRST_UP + eased * self.rise,
            size: self.size * (0.7 + 0.45 * eased),
            alpha: alpha.clamp(0.0, 1.0),
            colour: PINKS[self.colour],
        })
    }

    /// When it's gone, in seconds into the burst
    pub fn end(&self) -> f64 {
        (self.start + self.life) as f64
    }

    /// What DWM plays: each value from the burst's start, held at where it starts until its
    /// heart does (unseen, its opacity 0), then exactly as [`Self::at`] has it but for the
    /// sway, a sine sampled into cubics
    pub fn tracks(&self) -> Tracks {
        let (start, life, end) = (self.start as f64, self.life as f64, self.end());
        // eased = 2τ/L - τ²/L², so the rise and the growth are each one quadratic from the start
        let quadratic = |from: f32, by: f32| Track {
            pieces: vec![
                Piece { begin: 0.0, coefficients: [from, 0.0, 0.0, 0.0] },
                Piece {
                    begin: start,
                    coefficients: [from, 2.0 * by / self.life, -by / (self.life * self.life), 0.0],
                },
            ],
            end,
            end_value: from + by,
        };
        let up = quadratic(FIRST_UP, self.rise);
        let size = quadratic(0.7 * self.size, 0.45 * self.size);
        // In, held, out: straight lines
        let (rise_at, fall_at) = (start + IN as f64 * life, start + (1.0 - OUT) as f64 * life);
        let alpha = Track {
            pieces: vec![
                Piece { begin: 0.0, coefficients: [0.0; 4] },
                Piece { begin: start, coefficients: [0.0, 1.0 / (IN * self.life), 0.0, 0.0] },
                Piece { begin: rise_at, coefficients: [1.0, 0.0, 0.0, 0.0] },
                Piece { begin: fall_at, coefficients: [1.0, -1.0 / (OUT * self.life), 0.0, 0.0] },
            ],
            end,
            end_value: 0.0,
        };
        // The sway: a cubic between samples, each matching the sine and its slope at both ends
        let x = |tau: f64| self.base as f64 + (self.omega as f64 * tau + self.phase as f64).sin() * self.sway as f64;
        let dx = |tau: f64| self.omega as f64 * (self.omega as f64 * tau + self.phase as f64).cos() * self.sway as f64;
        let steps = (life / SWAY_STEP).ceil().max(1.0) as usize;
        let h = life / steps as f64;
        let mut pieces = vec![Piece { begin: 0.0, coefficients: [x(0.0) as f32, 0.0, 0.0, 0.0] }];
        for k in 0..steps {
            let (a, b) = (k as f64 * h, (k + 1) as f64 * h);
            let (y0, y1, m0, m1) = (x(a), x(b), dx(a), dx(b));
            let c2 = (3.0 * (y1 - y0) / h - 2.0 * m0 - m1) / h;
            let c3 = (2.0 * (y0 - y1) / h + m0 + m1) / (h * h);
            pieces.push(Piece {
                begin: start + a,
                coefficients: [y0 as f32, m0 as f32, c2 as f32, c3 as f32],
            });
        }
        let left = Track { pieces, end, end_value: x(life) as f32 };
        Tracks { left, up, size, alpha }
    }
}

/// Where a burst's hearts rise from, in the window's pixels: their pane's bottom right corner
/// and its top, which they're cut off at
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub corner: (f32, f32),
    pub top: f32,
}

/// A burst handed to DWM where its pane was as it rose (`rose`), with the pane at `now`: how
/// far its hearts move to rise from where the pane is now, and where in their own pixels (as
/// they rose) the pane's top is to cut them off at; nothing while the pane isn't shown, when
/// they're hidden, as the GUI's own drawing leaves out a burst over a pane it doesn't draw
pub fn follow(rose: Place, now: Option<Place>) -> Option<(f32, f32, f32)> {
    let now = now?;
    let dx = now.corner.0 - rose.corner.0;
    let dy = now.corner.1 - rose.corner.1;
    Some((dx, dy, now.top - dy))
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
        // Every heart's own end is within the burst's
        for seed in 0..50 {
            assert!(paths(seed).iter().all(|p| p.end() <= DURATION.as_secs_f64()));
        }
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
                assert!(h.size > 5.0 && h.size <= MAX_SIZE, "{h:?}");
                assert!((0.0..=1.0).contains(&h.alpha));
            }
        }
    }

    #[test]
    fn a_burst_follows_its_pane_and_hides_while_it_isnt_shown() {
        let rose = Place { corner: (700.0, 400.0), top: 30.0 };
        // Where it rose: nothing to move, cut at the top
        assert_eq!(follow(rose, Some(rose)), Some((0.0, 0.0, 30.0)));
        // The pane moved right 100 and made shorter by 50 at the top (a split, the rail): the
        // corner moves by (100, 0), and the cut, in the hearts' own pixels, lands on the new top
        let now = Place { corner: (800.0, 400.0), top: 80.0 };
        let (dx, dy, top) = follow(rose, Some(now)).unwrap();
        assert_eq!((dx, dy), (100.0, 0.0));
        assert_eq!(top + dy, now.top);
        // Zoomed to the whole window: the corner goes down and right, the top up
        let zoomed = Place { corner: (1200.0, 800.0), top: 10.0 };
        let (dx, dy, top) = follow(rose, Some(zoomed)).unwrap();
        assert_eq!((rose.corner.0 + dx, rose.corner.1 + dy), zoomed.corner);
        assert_eq!(top + dy, zoomed.top);
        // A tab switched away from: hidden
        assert_eq!(follow(rose, None), None);
    }

    #[test]
    fn the_seed_changes_the_burst() {
        assert_ne!(frame(MS * 400, 1), frame(MS * 400, 2));
        assert_eq!(frame(MS * 400, 1), frame(MS * 400, 1));
    }

    /// The burst as it was drawn frame by frame until 2026-10-31, worked out in one go
    fn frame_before(age: Duration, seed: u32) -> Vec<Heart> {
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
            let eased = 1.0 - (1.0 - p) * (1.0 - p);
            let base = 18.0 + r(3) * 70.0;
            let sway = (p * TAU * (0.8 + r(4)) + r(5) * 6.0).sin() * (6.0 + 8.0 * r(6));
            let size = (11.0 + 9.0 * r(7)) * (0.7 + 0.45 * eased);
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

    #[test]
    fn the_paths_draw_the_burst_as_it_always_was() {
        for seed in [1, 2, 7, 40, 977] {
            for ms in (0..2100).step_by(10) {
                let (now, before) = (frame(MS * ms, seed), frame_before(MS * ms, seed));
                assert_eq!(now.len(), before.len(), "seed {seed} at {ms} ms");
                for (a, b) in now.iter().zip(&before) {
                    assert!((a.left - b.left).abs() < 0.01 && (a.up - b.up).abs() < 0.01, "{a:?} {b:?}");
                    assert!((a.size - b.size).abs() < 0.01 && (a.alpha - b.alpha).abs() < 0.001, "{a:?} {b:?}");
                    assert_eq!(a.colour, b.colour);
                }
            }
        }
    }

    #[test]
    fn what_dwm_plays_is_the_heart_within_a_tenth_of_a_pixel() {
        for seed in [1, 2, 7, 40, 977] {
            for path in paths(seed) {
                let tracks = path.tracks();
                for ms in 0..2100 {
                    let t = ms as f64 / 1000.0;
                    match path.at(t as f32) {
                        Some(h) => {
                            assert!((tracks.left.at(t) - h.left).abs() < 0.1, "left at {ms} ms: {} {}", tracks.left.at(t), h.left);
                            assert!((tracks.up.at(t) - h.up).abs() < 0.1, "up at {ms} ms");
                            assert!((tracks.size.at(t) - h.size).abs() < 0.05, "size at {ms} ms");
                            assert!((tracks.alpha.at(t) - h.alpha).abs() < 0.005, "alpha at {ms} ms: {} {}", tracks.alpha.at(t), h.alpha);
                        }
                        // Before it rises and after it's gone it can't be seen
                        None => assert!(tracks.alpha.at(t).abs() < 0.005, "alpha at {ms} ms, out of the air"),
                    }
                }
                // Each track's pieces in order, starting at the burst's start, ending with it
                for track in [&tracks.left, &tracks.up, &tracks.size, &tracks.alpha] {
                    assert_eq!(track.pieces[0].begin, 0.0);
                    assert!(track.pieces.windows(2).all(|w| w[0].begin < w[1].begin));
                    assert_eq!(track.end, path.end());
                }
            }
        }
    }
}
