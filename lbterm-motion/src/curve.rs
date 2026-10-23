//! Curves, written the way the web writes them.
//!
//! - `spring(220ms, 0)`: Apple's and Motion's spring, a perceptual duration
//!   and a bounce. `spring(stiffness=800, damping=1)` is niri's and
//!   Material's form of the same thing (mass 1, damping as a ratio).
//! - `120ms cubic-bezier(0.23, 1, 0.32, 1)`, `100ms ease`, `150ms linear`,
//!   `200ms linear(0, 0.6 40%, 1)`: a CSS transition's duration and timing
//!   function, in either order.
//! - `none`: no motion, the end state at once.

use std::f64::consts::TAU;
use std::fmt;
use std::str::FromStr;

/// A damped spring with mass 1, settling at 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub stiffness: f64,
    /// 1 is critically damped (no overshoot), below 1 bounces.
    pub damping_ratio: f64,
}

impl Spring {
    /// Apple's form: `duration` in seconds is how long the motion feels,
    /// not how long it takes to settle; `bounce` 0 is none, up to 1.
    /// Negative bounce is overdamped, as in SwiftUI.
    pub fn new(duration: f64, bounce: f64) -> Self {
        let omega = TAU / duration;
        let damping_ratio = if bounce >= 0.0 {
            1.0 - bounce
        } else {
            1.0 / (1.0 + bounce)
        };
        Self {
            stiffness: omega * omega,
            damping_ratio,
        }
    }

    /// Natural frequency in radians per second.
    pub fn omega(&self) -> f64 {
        self.stiffness.sqrt()
    }

    /// The perceptual duration `new` was given.
    pub fn duration(&self) -> f64 {
        TAU / self.omega()
    }

    /// Displacement and velocity `t` seconds after starting at
    /// displacement `x0` with velocity `v0`.
    pub fn state(&self, x0: f64, v0: f64, t: f64) -> (f64, f64) {
        let w = self.omega();
        let z = self.damping_ratio;
        if (z - 1.0).abs() < 1e-9 {
            let e = (-w * t).exp();
            let b = v0 + w * x0;
            let x = e * (x0 + b * t);
            (x, e * b - w * x)
        } else if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * t).exp();
            let b = (v0 + z * w * x0) / wd;
            let (s, c) = (wd * t).sin_cos();
            let x = e * (x0 * c + b * s);
            let v = e * ((b * wd - z * w * x0) * c - (x0 * wd + z * w * b) * s);
            (x, v)
        } else {
            let r = (z * z - 1.0).sqrt();
            let r1 = -w * (z - r);
            let r2 = -w * (z + r);
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        }
    }

    /// Displacement and velocity folded into one distance from rest:
    /// sqrt(x² + (v/ω)²). It never rises, because damping only ever takes
    /// energy out, so the first time it drops under a threshold the spring
    /// has settled for good.
    pub fn amplitude(&self, x: f64, v: f64) -> f64 {
        (x * x + v * v / self.stiffness).sqrt()
    }
}

/// A CSS timing function: input progress 0..1 to output progress.
#[derive(Debug, Clone, PartialEq)]
pub enum Easing {
    Linear,
    CubicBezier(f64, f64, f64, f64),
    /// CSS `linear()`: (input, output) stops, inputs rising from 0 to 1.
    Stops(Vec<(f64, f64)>),
}

impl Easing {
    pub const EASE: Easing = Easing::CubicBezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Easing = Easing::CubicBezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Easing = Easing::CubicBezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Easing = Easing::CubicBezier(0.42, 0.0, 0.58, 1.0);

    /// Output progress at input progress `t` (clamped to 0..1).
    pub fn eval(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::CubicBezier(x1, y1, x2, y2) => {
                let s = bezier_param(*x1, *x2, t);
                bezier(*y1, *y2, s)
            }
            Easing::Stops(stops) => {
                let (a, b) = stop_span(stops, t);
                if b.0 <= a.0 {
                    b.1
                } else {
                    a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0)
                }
            }
        }
    }

    /// d(output)/d(input) at `t`.
    pub fn slope(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => 1.0,
            Easing::CubicBezier(x1, y1, x2, y2) => {
                let s = bezier_param(*x1, *x2, t);
                let dx = bezier_slope(*x1, *x2, s);
                if dx.abs() > 1e-9 {
                    return bezier_slope(*y1, *y2, s) / dx;
                }
                // A control point on an end point (ease-out's (0, 0)) makes
                // both derivatives 0 there; the slope is the ratio of the
                // second derivatives instead
                let ddx = bezier_curvature(*x1, *x2, s);
                if ddx.abs() > 1e-9 {
                    bezier_curvature(*y1, *y2, s) / ddx
                } else {
                    0.0
                }
            }
            Easing::Stops(stops) => {
                let (a, b) = stop_span(stops, t);
                if b.0 <= a.0 {
                    0.0
                } else {
                    (b.1 - a.1) / (b.0 - a.0)
                }
            }
        }
    }
}

/// One coordinate of a CSS cubic bezier from (0,0) to (1,1) with control
/// points p1 and p2, at curve parameter `s`.
fn bezier(p1: f64, p2: f64, s: f64) -> f64 {
    let c = 3.0 * p1;
    let b = 3.0 * (p2 - p1) - c;
    let a = 1.0 - c - b;
    ((a * s + b) * s + c) * s
}

fn bezier_slope(p1: f64, p2: f64, s: f64) -> f64 {
    let c = 3.0 * p1;
    let b = 3.0 * (p2 - p1) - c;
    let a = 1.0 - c - b;
    (3.0 * a * s + 2.0 * b) * s + c
}

fn bezier_curvature(p1: f64, p2: f64, s: f64) -> f64 {
    let c = 3.0 * p1;
    let b = 3.0 * (p2 - p1) - c;
    let a = 1.0 - c - b;
    6.0 * a * s + 2.0 * b
}

/// The curve parameter where x reaches `t`: Newton's method, falling back
/// to bisection where the slope is flat. x is monotonic because CSS keeps
/// x1 and x2 within 0..1.
fn bezier_param(x1: f64, x2: f64, t: f64) -> f64 {
    let mut s = t;
    for _ in 0..8 {
        let err = bezier(x1, x2, s) - t;
        if err.abs() < 1e-9 {
            return s;
        }
        let d = bezier_slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s -= err / d;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    s = t;
    for _ in 0..60 {
        let x = bezier(x1, x2, s);
        if (x - t).abs() < 1e-9 {
            break;
        }
        if x < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    s
}

fn stop_span(stops: &[(f64, f64)], t: f64) -> ((f64, f64), (f64, f64)) {
    // The last stop at or before t, and the one after it
    let i = stops
        .iter()
        .rposition(|s| s.0 <= t)
        .unwrap_or(0)
        .min(stops.len() - 2);
    (stops[i], stops[i + 1])
}

/// How one number gets from where it is to where it's going.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// No motion: the end state at once.
    Instant,
    Spring(Spring),
    Timed { duration: f64, easing: Easing },
}

impl Curve {
    /// How long it takes as the eye sees it, in seconds: a timed curve's
    /// duration, a spring's perceptual one (not its settle time), nothing
    /// for `Instant`.
    pub fn duration(&self) -> f64 {
        match self {
            Curve::Instant => 0.0,
            Curve::Spring(s) => s.duration(),
            Curve::Timed { duration, .. } => *duration,
        }
    }

    /// The same curve `factor` times slower (`lbterm_motion.slowdown`).
    pub fn slowed(&self, factor: f64) -> Curve {
        match self {
            Curve::Instant => Curve::Instant,
            Curve::Spring(s) => Curve::Spring(Spring {
                stiffness: s.stiffness / (factor * factor),
                damping_ratio: s.damping_ratio,
            }),
            Curve::Timed { duration, easing } => Curve::Timed {
                duration: duration * factor,
                easing: easing.clone(),
            },
        }
    }
}

impl FromStr for Curve {
    type Err = String;

    fn from_str(text: &str) -> Result<Curve, String> {
        let text = text.trim().to_ascii_lowercase();
        if text == "none" {
            return Ok(Curve::Instant);
        }
        if let Some(args) = function_args(&text, "spring") {
            return parse_spring(&args).map(Curve::Spring);
        }

        let mut duration = None;
        let mut easing = None;
        for token in split_top_level(&text, |c| c.is_whitespace()) {
            if let Ok(d) = parse_duration(&token) {
                if duration.replace(d).is_some() {
                    return Err(format!("'{text}' has two durations"));
                }
            } else if easing.replace(parse_easing(&token)?).is_some() {
                return Err(format!("'{text}' has two timing functions"));
            }
        }
        let duration = duration.ok_or_else(|| {
            format!("'{text}' has no duration: write it like '120ms ease-out' or 'spring(220ms, 0)'")
        })?;
        if duration == 0.0 {
            return Ok(Curve::Instant);
        }
        Ok(Curve::Timed {
            duration,
            // CSS transitions default to ease
            easing: easing.unwrap_or(Easing::EASE),
        })
    }
}

/// The text inside `name(...)`, split at its top-level commas.
fn function_args(text: &str, name: &str) -> Option<Vec<String>> {
    let inner = text.strip_prefix(name)?.trim_start();
    let inner = inner.strip_prefix('(')?.strip_suffix(')')?;
    Some(
        split_top_level(inner, |c| c == ',')
            .into_iter()
            .map(|a| a.trim().to_string())
            .collect(),
    )
}

/// Split at `sep` characters that aren't inside parentheses, dropping
/// empty pieces.
fn split_top_level(text: &str, sep: impl Fn(char) -> bool) -> Vec<String> {
    let mut pieces = vec![];
    let mut depth = 0i32;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && sep(c) {
            if !current.trim().is_empty() {
                pieces.push(current.trim().to_string());
            }
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        pieces.push(current.trim().to_string());
    }
    pieces
}

fn parse_number(text: &str) -> Result<f64, String> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("'{text}' isn't a number"))
}

/// `220ms` or `0.22s`, in seconds.
fn parse_duration(text: &str) -> Result<f64, String> {
    let seconds = if let Some(ms) = text.strip_suffix("ms") {
        parse_number(ms)? / 1000.0
    } else if let Some(s) = text.strip_suffix('s') {
        parse_number(s)?
    } else {
        return Err(format!("'{text}' isn't a duration like 220ms or 0.22s"));
    };
    if seconds < 0.0 {
        return Err(format!("'{text}' is negative"));
    }
    Ok(seconds)
}

fn parse_spring(args: &[String]) -> Result<Spring, String> {
    if args.iter().any(|a| a.contains('=')) {
        let mut stiffness = None;
        let mut damping = None;
        for arg in args {
            let (key, value) = arg
                .split_once('=')
                .ok_or_else(|| format!("spring(): '{arg}' should be key=value"))?;
            let value = parse_number(value)?;
            match key.trim() {
                "stiffness" => stiffness = Some(value),
                "damping" => damping = Some(value),
                other => return Err(format!("spring(): no such setting '{other}'")),
            }
        }
        let stiffness =
            stiffness.ok_or_else(|| "spring(): stiffness is missing".to_string())?;
        let damping_ratio = damping.unwrap_or(1.0);
        if stiffness <= 0.0 || damping_ratio <= 0.0 {
            return Err("spring(): stiffness and damping must be above 0".to_string());
        }
        return Ok(Spring {
            stiffness,
            damping_ratio,
        });
    }

    let (duration, bounce) = match args {
        [d] => (parse_duration(d)?, 0.0),
        [d, b] => (parse_duration(d)?, parse_number(b)?),
        _ => return Err("spring() takes a duration and a bounce: spring(220ms, 0)".to_string()),
    };
    if duration <= 0.0 {
        return Err("spring(): the duration must be above 0".to_string());
    }
    if !(-0.9..=0.9).contains(&bounce) {
        return Err(format!("spring(): bounce {bounce} is outside -0.9 to 0.9"));
    }
    Ok(Spring::new(duration, bounce))
}

fn parse_easing(text: &str) -> Result<Easing, String> {
    match text {
        "linear" => return Ok(Easing::Linear),
        "ease" => return Ok(Easing::EASE),
        "ease-in" => return Ok(Easing::EASE_IN),
        "ease-out" => return Ok(Easing::EASE_OUT),
        "ease-in-out" => return Ok(Easing::EASE_IN_OUT),
        _ => {}
    }
    if let Some(args) = function_args(text, "cubic-bezier") {
        let n = args
            .iter()
            .map(|a| parse_number(a))
            .collect::<Result<Vec<_>, _>>()?;
        if n.len() != 4 {
            return Err(format!("'{text}' needs four numbers"));
        }
        if !(0.0..=1.0).contains(&n[0]) || !(0.0..=1.0).contains(&n[2]) {
            return Err(format!("'{text}': the x values must be between 0 and 1"));
        }
        return Ok(Easing::CubicBezier(n[0], n[1], n[2], n[3]));
    }
    if let Some(args) = function_args(text, "linear") {
        return parse_linear_stops(&args).map(Easing::Stops);
    }
    Err(format!(
        "'{text}' isn't a duration or a timing function (linear, ease, ease-in, ease-out, \
         ease-in-out, cubic-bezier(), linear())"
    ))
}

/// CSS `linear()` stops: `output [input%] [input%]`, with missing inputs
/// spread evenly between their neighbours, the first at 0% and the last at
/// 100%, and each input held at least as high as the one before it.
fn parse_linear_stops(args: &[String]) -> Result<Vec<(f64, f64)>, String> {
    let mut points: Vec<(Option<f64>, f64)> = vec![];
    for arg in args {
        let parts: Vec<&str> = arg.split_whitespace().collect();
        let output = parse_number(parts.first().ok_or("linear(): empty stop")?)?;
        let percent = |p: &str| -> Result<f64, String> {
            let n = p
                .strip_suffix('%')
                .ok_or_else(|| format!("linear(): '{p}' should be a percentage"))?;
            Ok(parse_number(n)? / 100.0)
        };
        match parts.len() {
            1 => points.push((None, output)),
            2 => points.push((Some(percent(parts[1])?), output)),
            3 => {
                points.push((Some(percent(parts[1])?), output));
                points.push((Some(percent(parts[2])?), output));
            }
            _ => return Err(format!("linear(): '{arg}' has too many parts")),
        }
    }
    if points.len() < 2 {
        return Err("linear() needs at least two stops".to_string());
    }
    let last = points.len() - 1;
    if points[0].0.is_none() {
        points[0].0 = Some(0.0);
    }
    if points[last].0.is_none() {
        points[last].0 = Some(1.0);
    }
    let mut highest = f64::MIN;
    for p in points.iter_mut() {
        if let Some(input) = p.0.as_mut() {
            *input = input.max(highest);
            highest = *input;
        }
    }
    let mut i = 0;
    while i < points.len() {
        if points[i].0.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let end = (i..points.len()).find(|&j| points[j].0.is_some()).unwrap();
        let (a, b) = (points[start].0.unwrap(), points[end].0.unwrap());
        for j in i..end {
            points[j].0 = Some(a + (b - a) * (j - start) as f64 / (end - start) as f64);
        }
        i = end;
    }
    Ok(points.into_iter().map(|(i, o)| (i.unwrap(), o)).collect())
}

impl fmt::Display for Easing {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Easing::Linear => write!(f, "linear"),
            Easing::CubicBezier(a, b, c, d) => write!(f, "cubic-bezier({a}, {b}, {c}, {d})"),
            Easing::Stops(stops) => {
                write!(f, "linear(")?;
                for (i, (input, output)) in stops.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{output} {}%", input * 100.0)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for Curve {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Curve::Instant => write!(f, "none"),
            Curve::Spring(s) => write!(
                f,
                "spring(stiffness={}, damping={})",
                s.stiffness, s.damping_ratio
            ),
            Curve::Timed { duration, easing } => write!(f, "{}ms {easing}", duration * 1000.0),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn spring_from_duration_and_bounce() {
        // niri's movement spring is stiffness 800, damping 1: about 220 ms
        let s = Spring::new(0.22, 0.0);
        assert!(close(s.stiffness, 815.6, 0.1), "{s:?}");
        assert_eq!(s.damping_ratio, 1.0);
        assert!(close(s.duration(), 0.22, 1e-12));
        assert!(close(Spring::new(0.24, 0.1).damping_ratio, 0.9, 1e-12));
        assert!(Spring::new(0.24, -0.5).damping_ratio > 1.0);
    }

    #[test]
    fn spring_state_starts_where_it_was_told() {
        for s in [
            Spring::new(0.22, 0.0),
            Spring::new(0.3, 0.3),
            Spring::new(0.3, -0.5),
        ] {
            let (x, v) = s.state(-500.0, 1200.0, 0.0);
            assert!(close(x, -500.0, 1e-9) && close(v, 1200.0, 1e-9), "{s:?}");
        }
    }

    #[test]
    fn spring_velocity_is_the_derivative_of_its_position() {
        for s in [
            Spring::new(0.22, 0.0),
            Spring::new(0.3, 0.3),
            Spring::new(0.3, -0.5),
        ] {
            for i in 1..60 {
                let t = i as f64 * 0.01;
                let h = 1e-6;
                let numeric = (s.state(-500.0, 300.0, t + h).0 - s.state(-500.0, 300.0, t - h).0)
                    / (2.0 * h);
                assert!(close(s.state(-500.0, 300.0, t).1, numeric, 1e-3), "{s:?} t={t}");
            }
        }
    }

    #[test]
    fn critically_damped_never_overshoots_and_bounce_does() {
        let crit = Spring::new(0.22, 0.0);
        let bouncy = Spring::new(0.22, 0.3);
        let mut overshot = false;
        for i in 0..2000 {
            let t = i as f64 * 0.001;
            assert!(crit.state(-1000.0, 0.0, t).0 <= 0.0, "t={t}");
            overshot |= bouncy.state(-1000.0, 0.0, t).0 > 0.0;
        }
        assert!(overshot);
    }

    #[test]
    fn css_keywords_and_known_values() {
        assert_eq!(Easing::Linear.eval(0.3), 0.3);
        for e in [Easing::EASE, Easing::EASE_OUT, Easing::EASE_IN_OUT] {
            assert!(close(e.eval(0.0), 0.0, 1e-9) && close(e.eval(1.0), 1.0, 1e-9));
        }
        // Browsers' value for ease at 50%
        assert!(close(Easing::EASE.eval(0.5), 0.8024, 1e-3), "{}", Easing::EASE.eval(0.5));
        // ease-out starts fast and ends slow; its start is 0/0, so check
        // the limit: 3 / (3 × 0.58)
        assert!(close(Easing::EASE_OUT.slope(0.0), 3.0 / 1.74, 1e-6));
        assert!(close(Easing::EASE_OUT.slope(1e-4), 3.0 / 1.74, 1e-2));
        assert!(Easing::EASE_OUT.slope(0.0) > Easing::EASE_OUT.slope(0.9));
    }

    #[test]
    fn bezier_slope_matches_its_values() {
        let e = Easing::CubicBezier(0.23, 1.0, 0.32, 1.0);
        for i in 1..20 {
            let t = i as f64 / 20.0;
            let numeric = (e.eval(t + 1e-6) - e.eval(t - 1e-6)) / 2e-6;
            assert!(close(e.slope(t), numeric, 1e-3), "t={t}");
        }
    }

    #[test]
    fn css_linear_function() {
        let e: Curve = "100ms linear(0, 0.25 75%, 1)".parse().unwrap();
        let Curve::Timed { easing, .. } = e else {
            panic!()
        };
        assert_eq!(easing, Easing::Stops(vec![(0.0, 0.0), (0.75, 0.25), (1.0, 1.0)]));
        assert!(close(easing.eval(0.75), 0.25, 1e-12));
        assert!(close(easing.eval(0.875), 0.625, 1e-12));

        // Missing inputs spread evenly; a two-percentage stop holds
        let e = parse_linear_stops(&["0".into(), "0.5".into(), "0.5 50% 70%".into(), "1".into()])
            .unwrap();
        assert_eq!(e, vec![(0.0, 0.0), (0.25, 0.5), (0.5, 0.5), (0.7, 0.5), (1.0, 1.0)]);
    }

    #[test]
    fn parses_the_forms_the_config_uses() {
        assert_eq!(
            "spring(220ms, 0)".parse::<Curve>(),
            Ok(Curve::Spring(Spring::new(0.22, 0.0)))
        );
        assert_eq!(
            "spring(0.24s, 0.1)".parse::<Curve>(),
            Ok(Curve::Spring(Spring::new(0.24, 0.1)))
        );
        assert_eq!(
            "spring(stiffness=800, damping=1.0)".parse::<Curve>(),
            Ok(Curve::Spring(Spring {
                stiffness: 800.0,
                damping_ratio: 1.0
            }))
        );
        let ease_out = Curve::Timed {
            duration: 0.12,
            easing: Easing::CubicBezier(0.23, 1.0, 0.32, 1.0),
        };
        assert_eq!("120ms cubic-bezier(0.23, 1, 0.32, 1)".parse(), Ok(ease_out.clone()));
        assert_eq!(" Cubic-Bezier(0.23,1,0.32,1)   0.12s ".parse(), Ok(ease_out));
        assert_eq!(
            "100ms".parse(),
            Ok(Curve::Timed {
                duration: 0.1,
                easing: Easing::EASE
            })
        );
        assert_eq!("none".parse(), Ok(Curve::Instant));
        assert_eq!("0ms linear".parse(), Ok(Curve::Instant));
    }

    #[test]
    fn rejects_what_it_cannot_play() {
        for bad in [
            "spring()",
            "spring(220ms, 1)",
            "spring(0ms, 0)",
            "spring(stiffness=0)",
            "spring(mass=2, stiffness=800)",
            "120 ease",
            "ease",
            "120ms 200ms",
            "120ms ease ease-out",
            "120ms cubic-bezier(2, 0, 0, 1)",
            "120ms cubic-bezier(0, 0, 1)",
            "120ms wobble",
            "-5ms linear",
        ] {
            assert!(bad.parse::<Curve>().is_err(), "{bad} parsed");
        }
    }

    #[test]
    fn display_round_trips() {
        for text in [
            "spring(220ms, 0.1)",
            "120ms cubic-bezier(0.23, 1, 0.32, 1)",
            "200ms linear(0, 0.6 40%, 1)",
            "none",
        ] {
            let curve: Curve = text.parse().unwrap();
            assert_eq!(curve.to_string().parse::<Curve>(), Ok(curve), "{text}");
        }
    }

    #[test]
    fn slowdown_scales_time() {
        let s = Spring::new(0.22, 0.0);
        let Curve::Spring(slow) = Curve::Spring(s).slowed(5.0) else {
            panic!()
        };
        assert!(close(slow.duration(), 1.1, 1e-12));
        // Same shape, five times the time
        assert!(close(slow.state(-100.0, 0.0, 0.5).0, s.state(-100.0, 0.0, 0.1).0, 1e-9));
    }

    #[test]
    fn duration_is_what_the_eye_sees() {
        let spring: Curve = "spring(220ms, 0)".parse().unwrap();
        assert!(close(spring.duration(), 0.22, 1e-12));
        assert!(close(spring.slowed(5.0).duration(), 1.1, 1e-12));
        let timed: Curve = "150ms cubic-bezier(0.23, 1, 0.32, 1)".parse().unwrap();
        assert!(close(timed.duration(), 0.15, 1e-12));
        assert_eq!(Curve::Instant.duration(), 0.0);
    }
}
