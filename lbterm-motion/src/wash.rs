//! The tab panel's wave as DirectComposition plays it (lbterm/TAB-PANEL.md, "The wave").
//!
//! Its front surges out just ahead of the panel's edge and runs on past where the edge stops,
//! across the panes, a character cell at a time ([`crate::path::ahead`]). Near the end of its run
//! the crest breaks: the water pulls back as it fades (`panel_wash`), and the spray it throws
//! lights up (`copy_in`) and scatters on as it fades (`panel_splash`). Every number here is one
//! DComp animation, timed from the moment the edge sets off; the edge's own motion is never
//! touched, so the panel opens exactly as fast with the wave as without it.

use crate::curve::Curve;
use crate::motion::{Chain, Cubic, Motion, Segments};
use crate::path;

/// DWM gets a cubic per this much of an animation, as the compositor's own motions do
const STEP: f64 = 1.0 / 120.0;
/// The crest breaks once its front is this many cells from the end of its run
const BREAK_CELLS: f64 = 2.0;
/// An opacity has settled this close to where it goes
const OPACITY: f64 = 0.002;

/// How the wave runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// How far ahead of the edge it sets off: this times the edge's speed times its curve's time
    /// constant
    pub lead: f64,
    /// How far past the edge's stop its front runs, in pixels
    pub beyond: f64,
    /// A character cell's width: everything moves a whole cell at a time
    pub cell: f64,
    /// How far the water pulls back as it fades, and the spray scatters on, in pixels
    pub recede: f64,
    pub drift: f64,
    /// The water pulling back and fading (`panel_wash`); the spray lighting up (`copy_in`) and
    /// scattering on as it fades (`panel_splash`)
    pub fade: Curve,
    pub rise: Curve,
    pub splash: Curve,
}

/// The wave's animations, from when the edge sets off.
#[derive(Debug, Clone, PartialEq)]
pub struct Wash {
    /// The water's offset from where it's drawn (its front at the end of its run), in whole cells
    pub wave_x: Segments,
    pub wave_opacity: Segments,
    /// The spray's offset from where it's drawn (from the end of the run on), and its opacity
    pub splash_x: Segments,
    pub splash_opacity: Segments,
    /// When the crest breaks
    pub breaks: f64,
    /// When the last of it has gone
    pub end: f64,
}

/// `segments` starting `delay` seconds in, holding `before` until then.
fn after(delay: f64, before: f64, segments: Segments) -> Segments {
    let mut cubics = vec![Cubic {
        begin: 0.0,
        coefficients: [before as f32, 0.0, 0.0, 0.0],
    }];
    cubics.extend(segments.cubics.into_iter().map(|c| Cubic {
        begin: c.begin + delay,
        coefficients: c.coefficients,
    }));
    Segments {
        cubics,
        end: segments.end + delay,
        end_value: segments.end_value,
    }
}

/// `px` to the nearest whole number of `cell`s.
fn cells(px: f64, cell: f64) -> f64 {
    (px / cell).round() * cell
}

/// The wave for an edge moving as `edge` does, or nothing when the edge doesn't move.
pub fn wash(edge: &Motion, s: &Settings, epsilon: f64) -> Option<Wash> {
    let run = path::ahead(edge, s.lead, s.beyond, s.cell, epsilon);
    if run.cubics.is_empty() || s.cell <= 0.0 {
        return None;
    }
    let toward = (edge.to - edge.from).signum();
    // It breaks when its front gets within a couple of cells of the end of its run
    let breaks = run
        .cubics
        .iter()
        .find(|c| (c.coefficients[0] as f64).abs() <= BREAK_CELLS * s.cell + 1e-6)
        .map_or(run.end, |c| c.begin);
    let at_break = run.value(breaks);

    // The water: its run, then pulling back as it fades
    let fade = Motion::new(0.0, 1.0, 0.0, s.fade.clone());
    let fade_end = fade.end(1e-4);
    let back = |p: f64| at_break - toward * cells(s.recede * p, s.cell);
    let wave_x = path::held(
        |t| if t < breaks { run.value(t) } else { back(fade.value(t - breaks)) },
        breaks + fade_end,
        back(1.0),
    );
    let wave_opacity = after(breaks, 1.0, Motion::new(1.0, 0.0, 0.0, s.fade.clone()).segments(STEP, OPACITY));

    // The spray: hidden until the crest breaks, then up and scattering on as it fades
    let pulse = Chain::pulse(0.0, 0.0, 1.0, 0.0, s.rise.clone(), s.splash.clone(), OPACITY);
    let splash_opacity = after(breaks, 0.0, pulse.segments(STEP, OPACITY));
    let scatter = Motion::new(0.0, 1.0, 0.0, s.splash.clone());
    let scatter_end = pulse.end();
    let on = |p: f64| toward * cells(s.drift * p, s.cell);
    let splash_x = path::held(
        |t| if t < breaks { 0.0 } else { on(scatter.value(t - breaks)) },
        breaks + scatter_end,
        on(1.0),
    );

    let end = [wave_x.end, wave_opacity.end, splash_x.end, splash_opacity.end]
        .into_iter()
        .fold(0.0, f64::max);
    Some(Wash {
        wave_x,
        wave_opacity,
        splash_x,
        splash_opacity,
        breaks,
        end,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    /// The tab panel's edge, 222 px on `panel_open`, and a wave running 222 px past it in 9 px
    /// cells
    fn edge() -> Motion {
        Motion::new(0.0, 222.0, 0.0, "spring(220ms, 0)".parse().unwrap())
    }

    fn settings() -> Settings {
        Settings {
            lead: 0.8,
            beyond: 222.0,
            cell: 9.0,
            recede: 27.0,
            drift: 36.0,
            fade: "450ms cubic-bezier(0.23, 1, 0.32, 1)".parse().unwrap(),
            rise: "180ms cubic-bezier(0.23, 1, 0.32, 1)".parse().unwrap(),
            splash: "600ms cubic-bezier(0.23, 1, 0.32, 1)".parse().unwrap(),
        }
    }

    fn whole_cells(s: &Segments) -> bool {
        s.cubics.iter().all(|c| c.coefficients[0] % 9.0 == 0.0 && c.coefficients[1..] == [0.0, 0.0, 0.0])
    }

    #[test]
    fn the_wave_runs_out_past_the_edge_then_breaks() {
        let m = edge();
        let w = wash(&m, &settings(), 0.1).unwrap();
        let front = |t: f64| 444.0 + w.wave_x.value(t);
        // Ahead of the edge until it breaks
        for i in 0..=1000 {
            let t = i as f64 / 1000.0 * w.breaks;
            assert!(front(t) > m.value(t) - 9.0, "at {t}");
        }
        // Out on the panes when it breaks, within a couple of cells of the end of its run
        assert!(front(w.breaks) >= 444.0 - 18.0 && front(w.breaks) > 222.0 + 180.0);
        // While the edge is still settling: it breaks in under a quarter of a second
        assert!(w.breaks > 0.05 && w.breaks < 0.25, "{}", w.breaks);
        // ...then the water pulls back three cells
        assert_eq!(w.wave_x.end_value as f64, w.wave_x.value(w.breaks) - 27.0);
        assert!(whole_cells(&w.wave_x));
        // Gone within a second
        assert!(w.end < 1.0, "{}", w.end);
    }

    #[test]
    fn the_water_shows_until_it_breaks_and_fades_after() {
        let w = wash(&edge(), &settings(), 0.1).unwrap();
        assert_eq!(w.wave_opacity.value(0.0), 1.0);
        assert_eq!(w.wave_opacity.value(w.breaks - 0.001), 1.0);
        let mid = w.wave_opacity.value(w.breaks + 0.1);
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
        assert_eq!(w.wave_opacity.value(w.end), 0.0);
        for i in 0..=100 {
            let o = w.wave_opacity.value(i as f64 / 100.0);
            assert!((0.0..=1.0 + 1e-6).contains(&o));
        }
    }

    #[test]
    fn the_spray_comes_up_as_it_breaks_and_scatters_on() {
        let w = wash(&edge(), &settings(), 0.1).unwrap();
        assert_eq!(w.splash_opacity.value(0.0), 0.0);
        assert_eq!(w.splash_opacity.value(w.breaks - 0.001), 0.0);
        let peak = (0..=200).map(|i| w.splash_opacity.value(w.breaks + i as f64 / 400.0)).fold(0.0, f64::max);
        assert!(peak > 0.95, "{peak}");
        assert_eq!(w.splash_opacity.value(w.end), 0.0);
        // On in the edge's direction, a cell at a time, four cells by the end
        assert_eq!(w.splash_x.value(w.breaks - 0.001), 0.0);
        assert_eq!(w.splash_x.end_value, 36.0);
        assert!(whole_cells(&w.splash_x));
        let mut last = 0.0;
        for i in 0..=100 {
            let x = w.splash_x.value(i as f64 / 100.0);
            assert!(x >= last);
            last = x;
        }
    }

    #[test]
    fn no_wave_without_motion() {
        let still = Motion::new(0.0, 222.0, 0.0, Curve::Instant);
        assert!(wash(&still, &settings(), 0.1).is_none());
    }
}
