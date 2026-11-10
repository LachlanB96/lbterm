//! Motion that isn't one number on one curve: a point moving along a path
//! (the light running round the tab panel's ring), and a value through
//! keyframes (that light fading in and out). DComp animates one number at
//! a time, so each becomes straight cubics between samples.

use crate::curve::Curve;
use crate::motion::{Cubic, Motion, Segments};

/// Straight lines through `(seconds, value)` keyframes, in time order, then
/// the last value from the last keyframe on.
pub fn linear(keyframes: &[(f64, f64)]) -> Segments {
    let cubics = keyframes
        .windows(2)
        .filter(|pair| pair[1].0 > pair[0].0)
        .map(|pair| {
            let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
            Cubic {
                begin: t0,
                coefficients: [v0 as f32, ((v1 - v0) / (t1 - t0)) as f32, 0.0, 0.0],
            }
        })
        .collect();
    let (end, end_value) = keyframes.last().copied().unwrap_or((0.0, 0.0));
    Segments {
        cubics,
        end,
        end_value: end_value as f32,
    }
}

/// The x and y animations that carry a point along `point(p)` while `p`
/// goes from 0 to 1 on `curve`, sampled every `step` seconds: offsets from
/// where it starts, so they can move a visual drawn there.
pub fn along(curve: &Curve, step: f64, point: impl Fn(f64) -> (f64, f64)) -> [Segments; 2] {
    let progress = Motion::new(0.0, 1.0, 0.0, curve.clone());
    let end = progress.end(1.0e-4);
    let (x0, y0) = point(0.0);
    let mut times: Vec<f64> = (0..)
        .map(|i| i as f64 * step)
        .take_while(|t| *t < end)
        .collect();
    times.push(end);
    let sample = |t: f64| {
        let p = if t >= end { 1.0 } else { progress.value(t) };
        let (x, y) = point(p);
        (x - x0, y - y0)
    };
    let points: Vec<(f64, (f64, f64))> = times.iter().map(|&t| (t, sample(t))).collect();
    let xs: Vec<(f64, f64)> = points.iter().map(|&(t, (x, _))| (t, x)).collect();
    let ys: Vec<(f64, f64)> = points.iter().map(|&(t, (_, y))| (t, y)).collect();
    [linear(&xs), linear(&ys)]
}

/// Steps fall between samples this close together: under a frame at any refresh rate
const SAMPLE: f64 = 1.0 / 480.0;

/// `value(t)` from 0 to `end`, sampled under a frame apart, as segments that hold each value until
/// it changes, then `end_value` from `end` on: for something that moves in whole steps.
pub fn held(value: impl Fn(f64) -> f64, end: f64, end_value: f64) -> Segments {
    let mut cubics: Vec<Cubic> = vec![];
    let mut last = f64::NAN;
    let mut t = 0.0;
    while t < end {
        let v = value(t);
        if v != last {
            cubics.push(Cubic {
                begin: t,
                coefficients: [v as f32, 0.0, 0.0, 0.0],
            });
            last = v;
        }
        t += SAMPLE;
    }
    // Already where it ends: nothing to play
    if cubics.len() == 1 && last == end_value {
        cubics.clear();
    }
    Segments {
        cubics,
        end: if end > 0.0 { end } else { 0.0 },
        end_value: end_value as f32,
    }
}

/// Something running just ahead of `motion` on its way to `motion.to`, and on `beyond` past it,
/// moving in whole steps of `size` (the tab panel's wave, a character cell at a time): its
/// distance from where it ends, as segments that hold each step until the next and land on 0.
///
/// It leads by `lead` times the motion's speed times the curve's time constant (its duration over
/// 2π, a spring's 1/ω), plus `beyond` times how far along the motion is, never past where it ends
/// and never behind the motion. So it sets off at once, ahead, and runs on out past the motion as
/// the motion slows. For a critically damped spring from rest, `lead` 1 and nothing beyond,
/// that's `1 − e^(−ωt)` of the way: an ease-out that starts at full speed and never overshoots,
/// furthest ahead a time constant in, by the travel over e. Each step is taken late rather than
/// early, so at the start it's still level with the motion or behind it.
pub fn ahead(motion: &Motion, lead: f64, beyond: f64, size: f64, epsilon: f64) -> Segments {
    let rest = Segments {
        cubics: vec![],
        end: 0.0,
        end_value: 0.0,
    };
    let span = motion.to - motion.from;
    let end = motion.end(epsilon);
    if span.abs() < 1e-9 || size <= 0.0 || end <= 0.0 {
        return rest;
    }
    let toward = span.signum();
    let reach = motion.to + toward * beyond.max(0.0);
    let k = lead.max(0.0) * motion.curve.duration() / std::f64::consts::TAU;
    let offset = |t: f64| {
        let x = motion.value(t);
        let along = ((x - motion.from) / span).clamp(0.0, 1.0);
        // Between the motion and where it ends, whichever side each is on
        let (near, far) = if toward > 0.0 { (x, reach) } else { (reach, x) };
        let front = (x + k * motion.velocity(t) + toward * beyond.max(0.0) * along).clamp(near, far);
        let left = ((reach - front) * toward).max(0.0);
        -toward * size * (left / size - 1e-9).ceil().max(0.0)
    };
    // It's there once its last step is taken, or when the motion ends
    let mut done = end;
    let mut t = 0.0;
    while t < end {
        if offset(t) == 0.0 {
            done = t;
            break;
        }
        t += SAMPLE;
    }
    if done <= 0.0 {
        return rest;
    }
    held(offset, done, 0.0)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn keyframes_join_with_straight_lines_and_hold_the_last() {
        let s = linear(&[(0.0, 0.0), (0.1, 1.0), (0.5, 1.0), (0.6, 0.0)]);
        assert_eq!(s.cubics.len(), 3);
        assert!((s.value(0.05) - 0.5).abs() < 1e-6);
        assert!((s.value(0.3) - 1.0).abs() < 1e-6);
        assert!((s.value(0.55) - 0.5).abs() < 1e-5);
        assert_eq!((s.end, s.end_value), (0.6, 0.0));
        assert_eq!(s.value(2.0), 0.0);
        assert!(s.cubics.iter().all(|c| c.coefficients[2] == 0.0 && c.coefficients[3] == 0.0));
    }

    #[test]
    fn a_point_moves_along_its_path_on_the_curve() {
        let curve: Curve = "1s linear".parse().unwrap();
        let [x, y] = along(&curve, 1.0 / 120.0, |p| (100.0 + 300.0 * p, 50.0 + 20.0 * p));
        // Offsets from the start, so a visual drawn there moves with it
        assert!(x.value(0.0).abs() < 1e-6 && y.value(0.0).abs() < 1e-6);
        assert!((x.value(0.5) - 150.0).abs() < 0.01);
        assert!((y.value(0.25) - 5.0).abs() < 0.01);
        assert_eq!((x.end, x.end_value, y.end_value), (1.0, 300.0, 20.0));
        assert_eq!(x.cubics.len(), 120);
    }

    #[test]
    fn the_point_follows_the_curves_timing() {
        // Eased out: well past halfway by half time
        let curve: Curve = "1s cubic-bezier(0.22, 0.61, 0.36, 1)".parse().unwrap();
        let [x, _] = along(&curve, 1.0 / 120.0, |p| (100.0 * p, 0.0));
        assert!(x.value(0.5) > 70.0, "{}", x.value(0.5));
        let mut last = 0.0;
        for i in 0..=120 {
            let v = x.value(i as f64 / 120.0);
            assert!(v >= last - 1e-4, "it never runs back");
            last = v;
        }
    }

    /// The tab panel's edge: 222 px on `panel_open`
    fn edge() -> Motion {
        Motion::new(0.0, 222.0, 0.0, "spring(220ms, 0)".parse().unwrap())
    }

    #[test]
    fn ahead_runs_in_whole_steps_and_lands_on_the_target() {
        let s = ahead(&edge(), 1.0, 0.0, 9.0, 0.1);
        assert!(!s.cubics.is_empty());
        let mut last = f64::NEG_INFINITY;
        for c in &s.cubics {
            let v = c.coefficients[0] as f64;
            assert_eq!(c.coefficients[1..], [0.0, 0.0, 0.0], "held, not moving");
            assert_eq!(v % 9.0, 0.0, "a whole cell: {v}");
            assert!(v <= 0.0 && v > last, "only ever towards the target: {v} after {last}");
            last = v;
        }
        assert_eq!(s.end_value, 0.0);
        assert!(s.end <= edge().end(0.1));
    }

    #[test]
    fn ahead_leads_the_motion_from_the_first_frame_and_never_passes_the_target() {
        let m = edge();
        let s = ahead(&m, 1.0, 0.0, 9.0, 0.1);
        // Level with the edge or behind it at the start, so nothing shows before it moves
        assert!(222.0 + s.value(0.0) <= m.value(0.0));
        // A frame in at 144 Hz it's cells ahead
        let t = 1.0 / 144.0;
        assert!(222.0 + s.value(t) - m.value(t) > 20.0, "{} vs {}", 222.0 + s.value(t), m.value(t));
        for i in 0..=300 {
            let t = i as f64 / 1000.0;
            let front = 222.0 + s.value(t);
            assert!(front <= 222.0 + 1e-6);
            // Never a whole cell behind the edge while it moves
            assert!(front > m.value(t) - 9.0, "at {t}: {front} vs {}", m.value(t));
        }
        // Furthest ahead a time constant in: the travel over e, less up to a cell and a sample
        // for the steps
        let tc = 0.22 / std::f64::consts::TAU;
        let gap = 222.0 + s.value(tc) - m.value(tc);
        assert!((gap - 222.0 / std::f64::consts::E).abs() < 14.0, "{gap}");
    }

    #[test]
    fn ahead_leads_by_less_for_a_smaller_lead_and_not_at_all_for_none() {
        let m = edge();
        let tc = 0.22 / std::f64::consts::TAU;
        let half = ahead(&m, 0.5, 0.0, 9.0, 0.1);
        let gap = 222.0 + half.value(tc) - m.value(tc);
        assert!((gap - 0.5 * 222.0 / std::f64::consts::E).abs() < 14.0, "{gap}");
        let none = ahead(&m, 0.0, 0.0, 9.0, 0.1);
        for i in 0..=300 {
            let t = i as f64 / 1000.0;
            assert!(222.0 + none.value(t) <= m.value(t) + 1e-6);
        }
    }

    #[test]
    fn ahead_of_a_motion_turned_round_waits_for_it() {
        // Opening again while it closes: moving away from the target at first, so the front
        // stays with the motion until it turns
        let m = Motion::new(100.0, 222.0, -1500.0, "spring(220ms, 0)".parse().unwrap());
        let s = ahead(&m, 1.0, 0.0, 9.0, 0.1);
        assert!(222.0 + s.value(0.0) <= m.value(0.0));
        for i in 0..=300 {
            let t = i as f64 / 1000.0;
            let front = 222.0 + s.value(t);
            assert!(front <= 222.0 + 1e-6 && front > m.value(t) - 9.0, "at {t}");
        }
    }

    #[test]
    fn nothing_runs_ahead_of_no_motion() {
        let still = Motion::new(0.0, 222.0, 0.0, Curve::Instant);
        let s = ahead(&still, 1.0, 0.0, 9.0, 0.1);
        assert!(s.cubics.is_empty());
        assert_eq!(s.end_value, 0.0);
        let there = Motion::new(222.0, 222.0, 0.0, "spring(220ms, 0)".parse().unwrap());
        assert!(ahead(&there, 1.0, 0.0, 9.0, 0.1).cubics.is_empty());
    }

    #[test]
    fn ahead_runs_on_past_the_motion_by_beyond() {
        let m = edge();
        // Out 222 px past where the edge stops: its offsets are from there
        let s = ahead(&m, 0.8, 222.0, 9.0, 0.1);
        assert!(444.0 + s.value(0.0) <= m.value(0.0), "level with the edge or behind it at first");
        let mut furthest_ahead: f64 = 0.0;
        for i in 0..=400 {
            let t = i as f64 / 1000.0;
            let front = 444.0 + s.value(t);
            assert!(front <= 444.0 + 1e-6);
            assert!(front > m.value(t) - 9.0, "at {t}: {front} vs {}", m.value(t));
            furthest_ahead = furthest_ahead.max(front - m.value(t));
        }
        // Once the edge has stopped the front is out on the pane, a cell or less short of the end
        assert!(444.0 + s.value(s.end - 1e-6) >= 444.0 - 9.0);
        assert!(furthest_ahead > 200.0, "{furthest_ahead}");
    }

    #[test]
    fn held_keeps_each_value_until_it_changes() {
        let s = held(|t| if t < 0.1 { -18.0 } else if t < 0.2 { -9.0 } else { 0.0 }, 0.3, 0.0);
        assert_eq!(s.cubics.len(), 3);
        assert_eq!(s.value(0.05), -18.0);
        assert_eq!(s.value(0.15), -9.0);
        assert_eq!((s.end, s.end_value), (0.3, 0.0));
        assert!(held(|_| 4.0, 0.3, 4.0).cubics.is_empty(), "already there");
    }

    #[test]
    fn no_motion_puts_it_straight_at_the_end() {
        let [x, y] = along(&Curve::Instant, 1.0 / 120.0, |p| (10.0 * p, 0.0));
        assert!(x.cubics.is_empty() && y.cubics.is_empty());
        assert_eq!(x.end_value, 10.0);
    }
}
