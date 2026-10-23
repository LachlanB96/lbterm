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

    #[test]
    fn no_motion_puts_it_straight_at_the_end() {
        let [x, y] = along(&Curve::Instant, 1.0 / 120.0, |p| (10.0 * p, 0.0));
        assert!(x.cubics.is_empty() && y.cubics.is_empty());
        assert_eq!(x.end_value, 10.0);
    }
}
