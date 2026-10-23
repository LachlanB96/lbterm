//! One number moving along a curve, and the cubic segments that carry it to
//! DirectComposition.
//!
//! DComp has no springs. `IDCompositionAnimation` plays a list of cubics,
//! each `c0 + c1·τ + c2·τ² + c3·τ³` in seconds since its start, then holds
//! a final value. [`Motion::segments`] fits one cubic per step through the
//! exact position and velocity at both ends of the step (cubic Hermite), so
//! the curve on screen is the curve in the config, to well under a pixel.

use crate::curve::{Curve, Easing};

/// One number (a pixel offset, a clip edge, an opacity) going from `from`
/// to `to`, starting with velocity `v0` per second. Springs carry `v0`, so
/// a retarget mid-flight turns smoothly; timed curves ignore it, as CSS
/// transitions do.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub from: f64,
    pub to: f64,
    pub v0: f64,
    pub curve: Curve,
}

/// `c0 + c1·τ + c2·τ² + c3·τ³` with τ the seconds since `begin`: the
/// arguments of `IDCompositionAnimation::AddCubic`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cubic {
    pub begin: f64,
    pub coefficients: [f32; 4],
}

/// A motion as DComp plays it: the cubics in order, then `end_value` from
/// `end` seconds on (`IDCompositionAnimation::End`).
#[derive(Debug, Clone, PartialEq)]
pub struct Segments {
    pub cubics: Vec<Cubic>,
    pub end: f64,
    pub end_value: f32,
}

impl Motion {
    pub fn new(from: f64, to: f64, v0: f64, curve: Curve) -> Self {
        Self {
            from,
            to,
            v0,
            curve,
        }
    }

    /// Where it is `t` seconds after it started.
    pub fn value(&self, t: f64) -> f64 {
        match &self.curve {
            Curve::Instant => self.to,
            Curve::Spring(s) => self.to + s.state(self.from - self.to, self.v0, t.max(0.0)).0,
            Curve::Timed { duration, easing } => {
                self.from + (self.to - self.from) * easing.eval(t / duration)
            }
        }
    }

    /// How fast it's moving `t` seconds after it started, per second.
    pub fn velocity(&self, t: f64) -> f64 {
        match &self.curve {
            Curve::Instant => 0.0,
            Curve::Spring(s) => s.state(self.from - self.to, self.v0, t.max(0.0)).1,
            Curve::Timed { duration, easing } => {
                if t >= *duration {
                    0.0
                } else {
                    (self.to - self.from) * easing.slope(t / duration) / duration
                }
            }
        }
    }

    /// When it's done, in seconds. A timed curve ends on time. A spring
    /// ends when it's within `epsilon` of the target and moving too slowly
    /// to leave it again (its amplitude, which never rises).
    pub fn end(&self, epsilon: f64) -> f64 {
        match &self.curve {
            Curve::Instant => 0.0,
            Curve::Timed { duration, .. } => {
                if self.from == self.to {
                    0.0
                } else {
                    *duration
                }
            }
            Curve::Spring(s) => {
                let x0 = self.from - self.to;
                let settled = |t: f64| {
                    let (x, v) = s.state(x0, self.v0, t);
                    s.amplitude(x, v) < epsilon
                };
                if settled(0.0) {
                    return 0.0;
                }
                // Double until settled, then bisect to a millisecond.
                // Ten seconds is the cap for any spring LBTerm allows.
                let mut hi = 0.05;
                while !settled(hi) && hi < 10.0 {
                    hi *= 2.0;
                }
                let mut lo = 0.0;
                while hi - lo > 0.001 {
                    let mid = (lo + hi) / 2.0;
                    if settled(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                hi
            }
        }
    }

    /// The cubics DComp plays for this motion, one per `step` seconds (the
    /// last one shorter), each through the exact position and velocity at
    /// its ends. The final cubic lands on `to` at rest, so the hand-off to
    /// `end_value` doesn't jump.
    pub fn segments(&self, step: f64, epsilon: f64) -> Segments {
        let end = self.end(epsilon);
        let mut cubics = vec![];
        if end > 0.0 {
            match &self.curve {
                Curve::Timed {
                    duration,
                    easing: Easing::Stops(stops),
                } => {
                    // linear() is straight lines already: one per stop span
                    for pair in stops.windows(2) {
                        let (a, b) = (pair[0], pair[1]);
                        if b.0 <= a.0 {
                            continue;
                        }
                        let span = self.to - self.from;
                        let c0 = self.from + span * a.1;
                        let c1 = span * (b.1 - a.1) / ((b.0 - a.0) * duration);
                        cubics.push(Cubic {
                            begin: a.0 * duration,
                            coefficients: [c0 as f32, c1 as f32, 0.0, 0.0],
                        });
                    }
                }
                _ => {
                    let mut t0 = 0.0;
                    while t0 < end - 1e-9 {
                        let t1 = (t0 + step).min(end);
                        self.fit(t0, t1, end, epsilon / 4.0, 0, &mut cubics);
                        t0 = t1;
                    }
                }
            }
        }
        Segments {
            cubics,
            end,
            end_value: self.to as f32,
        }
    }

    /// One cubic over [t0, t1], halved until it's within `tolerance` of
    /// the motion (at most four times: steep curves like a fast ease-out
    /// need it early on, springs rarely do).
    fn fit(&self, t0: f64, t1: f64, end: f64, tolerance: f64, depth: u32, out: &mut Vec<Cubic>) {
        let last = t1 >= end - 1e-9;
        let (p1, m1) = if last {
            (self.to, 0.0)
        } else {
            (self.value(t1), self.velocity(t1))
        };
        let cubic = hermite(t0, self.value(t0), self.velocity(t0), t1, p1, m1);
        // The last cubic bends onto the target, which the motion only
        // reaches within epsilon, so it isn't held to the tolerance
        let fits = depth >= 4
            || last
            || [0.25, 0.5, 0.75].iter().all(|f| {
                let t = t0 + (t1 - t0) * f;
                (cubic.value(t) - self.value(t)).abs() <= tolerance
            });
        if fits {
            out.push(cubic);
        } else {
            let mid = (t0 + t1) / 2.0;
            self.fit(t0, mid, end, tolerance, depth + 1, out);
            self.fit(mid, t1, end, tolerance, depth + 1, out);
        }
    }
}

impl Cubic {
    /// Its value at time `t` (seconds since the motion started), with the
    /// coefficients rounded to f32 as DComp holds them.
    pub fn value(&self, t: f64) -> f64 {
        let tau = (t - self.begin).max(0.0);
        let [c0, c1, c2, c3] = self.coefficients.map(|c| c as f64);
        c0 + tau * (c1 + tau * (c2 + tau * c3))
    }
}

/// The cubic through (t0, p0) with slope m0 and (t1, p1) with slope m1.
fn hermite(t0: f64, p0: f64, m0: f64, t1: f64, p1: f64, m1: f64) -> Cubic {
    let h = t1 - t0;
    let d = (p1 - p0) / h;
    let c2 = (3.0 * d - 2.0 * m0 - m1) / h;
    let c3 = (m0 + m1 - 2.0 * d) / (h * h);
    Cubic {
        begin: t0,
        coefficients: [p0 as f32, m0 as f32, c2 as f32, c3 as f32],
    }
}

impl Segments {
    /// What DComp shows `t` seconds in: for tests, plots and retargeting.
    pub fn value(&self, t: f64) -> f64 {
        if t >= self.end || self.cubics.is_empty() {
            return self.end_value as f64;
        }
        let i = self
            .cubics
            .iter()
            .rposition(|c| c.begin <= t)
            .unwrap_or(0);
        self.cubics[i].value(t)
    }
}

/// Motions played one after another on one number, each starting where the
/// last one ended, as one DComp animation: a pulse rising to a peak and
/// settling back (the active outline when the keyboard arrives).
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    parts: Vec<Motion>,
    /// When each part starts, and when the last one ends, in seconds
    starts: Vec<f64>,
    end: f64,
}

impl Chain {
    /// `parts` in order, each ending within `epsilon` of its target before
    /// the next starts.
    pub fn new(parts: Vec<Motion>, epsilon: f64) -> Self {
        let mut starts = Vec::with_capacity(parts.len());
        let mut at = 0.0;
        for part in &parts {
            starts.push(at);
            at += part.end(epsilon);
        }
        Self {
            parts,
            starts,
            end: at,
        }
    }

    /// From `from` (moving at `v0`) up to `peak` on `rise`, then to `rest`
    /// on `fall`.
    pub fn pulse(from: f64, v0: f64, peak: f64, rest: f64, rise: Curve, fall: Curve, epsilon: f64) -> Self {
        Self::new(
            vec![
                Motion::new(from, peak, v0, rise),
                Motion::new(peak, rest, 0.0, fall),
            ],
            epsilon,
        )
    }

    /// When the last part is done, in seconds from the start.
    pub fn end(&self) -> f64 {
        self.end
    }

    /// Where it ends up.
    pub fn to(&self) -> f64 {
        self.parts.last().map_or(0.0, |p| p.to)
    }

    /// The part playing `t` seconds in, and how far into it.
    fn part_at(&self, t: f64) -> Option<(&Motion, f64)> {
        let i = self.starts.iter().rposition(|s| *s <= t)?;
        Some((&self.parts[i], t - self.starts[i]))
    }

    /// Where it is `t` seconds in.
    pub fn value(&self, t: f64) -> f64 {
        if t >= self.end {
            return self.to();
        }
        match self.part_at(t) {
            Some((part, local)) => part.value(local),
            None => self.parts.first().map_or(0.0, |p| p.from),
        }
    }

    /// How fast it's moving `t` seconds in, per second.
    pub fn velocity(&self, t: f64) -> f64 {
        if t >= self.end {
            return 0.0;
        }
        match self.part_at(t) {
            Some((part, local)) => part.velocity(local),
            None => 0.0,
        }
    }

    /// Every part's cubics, each moved to when its part starts: one
    /// animation for DComp.
    pub fn segments(&self, step: f64, epsilon: f64) -> Segments {
        let mut cubics = vec![];
        for (part, start) in self.parts.iter().zip(&self.starts) {
            for cubic in part.segments(step, epsilon).cubics {
                cubics.push(Cubic {
                    begin: cubic.begin + start,
                    coefficients: cubic.coefficients,
                });
            }
        }
        Segments {
            cubics,
            end: self.end,
            end_value: self.to() as f32,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::curve::Spring;

    fn pulse() -> Chain {
        Chain::pulse(
            0.0,
            0.0,
            1.0,
            0.2,
            "80ms cubic-bezier(0.23, 1, 0.32, 1)".parse().unwrap(),
            "spring(220ms, 0)".parse().unwrap(),
            0.002,
        )
    }

    #[test]
    fn a_pulse_rises_to_its_peak_then_settles() {
        let p = pulse();
        assert_eq!(p.value(0.0), 0.0);
        assert!((p.value(0.08) - 1.0).abs() < 1e-9, "{}", p.value(0.08));
        assert!(p.value(0.04) > 0.5 && p.value(0.04) < 1.0);
        assert!(p.value(0.2) < 0.5 && p.value(0.2) > 0.2);
        assert_eq!(p.value(p.end()), 0.2);
        assert_eq!(p.to(), 0.2);
        // Rise and fall: well under half a second in all
        assert!(p.end() > 0.3 && p.end() < 0.5, "{}", p.end());
        // Never past the peak or under the rest
        assert!((0..=500).all(|i| {
            let v = p.value(i as f64 / 1000.0);
            (-1e-9..=1.0 + 1e-9).contains(&v)
        }));
    }

    #[test]
    fn a_pulse_is_one_animation_dcomp_plays_to_the_curve() {
        let p = pulse();
        let s = p.segments(1.0 / 120.0, 0.002);
        assert!(s.cubics.windows(2).all(|w| w[0].begin < w[1].begin));
        assert_eq!(s.end, p.end());
        assert_eq!(s.end_value, 0.2);
        let worst = (0..=500)
            .map(|i| {
                let t = i as f64 / 1000.0;
                (s.value(t) - p.value(t)).abs()
            })
            .fold(0.0, f64::max);
        // Opacity: well under one level of 255
        assert!(worst < 0.004, "{worst}");
    }

    #[test]
    fn a_pulse_from_partway_keeps_its_speed() {
        let first = pulse();
        let t = 0.15;
        let again = Chain::pulse(
            first.value(t),
            first.velocity(t),
            1.0,
            0.2,
            "80ms cubic-bezier(0.23, 1, 0.32, 1)".parse().unwrap(),
            "spring(220ms, 0)".parse().unwrap(),
            0.002,
        );
        assert_eq!(again.value(0.0), first.value(t));
        assert!((again.value(0.08) - 1.0).abs() < 1e-9);
    }

    /// Every spring and curve in the skill's token table.
    fn tokens() -> Vec<Curve> {
        [
            "spring(220ms, 0)",
            "spring(160ms, 0)",
            "spring(240ms, 0.1)",
            "120ms cubic-bezier(0.23, 1, 0.32, 1)",
            "100ms ease",
        ]
        .iter()
        .map(|t| t.parse().unwrap())
        .collect()
    }

    /// The biggest gap between the motion and what DComp would play,
    /// sampled every 0.1 ms.
    fn worst_error(m: &Motion, s: &Segments) -> f64 {
        (0..=((s.end + 0.05) * 10_000.0) as usize)
            .map(|i| {
                let t = i as f64 / 10_000.0;
                let want = if t >= s.end { m.to } else { m.value(t) };
                (s.value(t) - want).abs()
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn cubics_follow_the_curve_to_under_half_a_pixel() {
        // Across a whole 4K-wide window and back, at 120 cubics a second
        for curve in tokens() {
            for (from, to, v0) in [(0.0, 3840.0, 0.0), (3840.0, 0.0, 0.0), (500.0, 900.0, -4000.0)]
            {
                let m = Motion::new(from, to, v0, curve.clone());
                let s = m.segments(1.0 / 120.0, 0.1);
                let err = worst_error(&m, &s);
                // Settling snaps the last 0.1 px; the fit adds hundredths
                assert!(err < 0.5, "{curve} {from}->{to} v0={v0}: off by {err} px");
            }
        }
    }

    #[test]
    fn segments_start_where_it_is_and_end_on_the_target() {
        for curve in tokens() {
            let m = Motion::new(120.0, 860.0, 0.0, curve.clone());
            let s = m.segments(1.0 / 120.0, 0.1);
            assert_eq!(s.cubics[0].begin, 0.0);
            assert!((s.value(0.0) - 120.0).abs() < 1e-3, "{curve}");
            assert_eq!(s.end_value, 860.0);
            assert!((s.value(s.end - 1e-9) - 860.0).abs() < 0.11, "{curve}");
            // Cubics in time order, all before the end
            assert!(s.cubics.windows(2).all(|w| w[0].begin < w[1].begin));
            assert!(s.cubics.last().unwrap().begin < s.end);
        }
    }

    #[test]
    fn the_layout_spring_settles_in_well_under_a_second() {
        let m = Motion::new(0.0, 1920.0, 0.0, "spring(220ms, 0)".parse().unwrap());
        let end = m.end(0.1);
        assert!(end > 0.3 && end < 0.8, "{end}");
        // About 4 ms of DComp frames per cubic would be wasteful; 1/120 s
        // keeps a long move under a hundred
        assert!(m.segments(1.0 / 120.0, 0.1).cubics.len() < 100);
    }

    #[test]
    fn a_retarget_keeps_its_speed() {
        // Halfway through a move, the target changes: the new motion starts
        // where and as fast as the old one was going
        let curve: Curve = "spring(220ms, 0)".parse().unwrap();
        let first = Motion::new(0.0, 1000.0, 0.0, curve.clone());
        let t = 0.08;
        let second = Motion::new(first.value(t), 200.0, first.velocity(t), curve);
        assert_eq!(second.value(0.0), first.value(t));
        assert!((second.velocity(0.0) - first.velocity(t)).abs() < 1e-9);
        // It carries on the old way for a moment before turning back, and
        // still settles
        assert!(second.value(0.01) > second.value(0.0));
        assert!(second.end(0.1) < 1.5);
    }

    #[test]
    fn nothing_to_do_means_no_cubics() {
        let m = Motion::new(5.0, 5.0, 0.0, "spring(220ms, 0)".parse().unwrap());
        assert_eq!(m.end(0.1), 0.0);
        assert!(m.segments(1.0 / 120.0, 0.1).cubics.is_empty());
        let m = Motion::new(0.0, 5.0, 0.0, Curve::Instant);
        let s = m.segments(1.0 / 120.0, 0.1);
        assert!(s.cubics.is_empty());
        assert_eq!(s.value(0.0), 5.0);
    }

    #[test]
    fn linear_stops_become_straight_segments() {
        let m = Motion::new(0.0, 100.0, 0.0, "200ms linear(0, 0.25 75%, 1)".parse().unwrap());
        let s = m.segments(1.0 / 120.0, 0.1);
        assert_eq!(s.cubics.len(), 2);
        assert!((s.value(0.15) - 25.0).abs() < 1e-4);
        assert!((s.value(0.175) - 62.5).abs() < 1e-4);
        assert_eq!(s.value(0.2), 100.0);
    }

    #[test]
    fn opacity_fades_end_on_time() {
        let m = Motion::new(0.0, 1.0, 0.0, "spring(160ms, 0)".parse().unwrap());
        let end = m.end(0.002);
        assert!(end > 0.15 && end < 0.6, "{end}");
        let s = m.segments(1.0 / 120.0, 0.002);
        assert!((0..=600).all(|i| {
            let v = s.value(i as f64 / 1000.0);
            (-1e-3..=1.0 + 1e-3).contains(&v)
        }));
    }

    #[test]
    fn spring_value_matches_spring_state() {
        let s = Spring::new(0.22, 0.0);
        let m = Motion::new(100.0, 300.0, 0.0, Curve::Spring(s));
        assert_eq!(m.value(0.1), 300.0 + s.state(-200.0, 0.0, 0.1).0);
    }
}
