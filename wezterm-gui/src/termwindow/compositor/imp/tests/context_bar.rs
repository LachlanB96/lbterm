//! The context bar's slides over the stand-in device (lbterm/COMPOSITOR.md, "The compositor's
//! tests"; lbterm/CONTEXT.md, "At rest and widened"): `prepare_context_bar` given the widened
//! bar's, the thin numbers' pane's and the riding strip's places as render/context_bar.rs makes
//! them, and where DWM has each edge at every 5 ms of what it built. Lachlan's E9 of 2026-11-10:
//! the thin numbers' pane comes out from under the widened bar's moving left edge in the bar's own
//! slide, one motion settled when the bar is, the strip riding the thin pane's left edge with it;
//! going back from out keeps round 3's one commit and his 3a (the thin pane in first, the strip
//! gliding home under the widened bar, then the bar narrowing).

use super::*;
use crate::termwindow::compositor::recorder::Tree;
use crate::termwindow::render::context_bar::{Slide, Visuals};

/// The bar at rest: its left edge, its right edge, and the hairline its rings are drawn with
const BAR_X: i32 = 700;
const BAR_RIGHT: i32 = 714;
const STROKE: i32 = 1;
/// The widened bar's card, the closed rail's width (68 px at 96 DPI) ending at the bar's right
const WIDE: PixelRect = PixelRect { x: 646, y: 40, w: 68, h: 400 };
/// The thin numbers' pane's card, as wide as the widened bar, its right ring on the bar's left
/// ring, and its visual's bounds, its shadow's reach (11 px, 3 more down) round it, as
/// `strip::visual_bounds` gives them
const THIN: PixelRect = PixelRect { x: 579, y: 40, w: 68, h: 400 };
const THIN_BOUNDS: PixelRect = PixelRect { x: 568, y: 29, w: 90, h: 425 };
const REACH: f64 = 11.0;
/// The scroll strip drawn where it rides the thin pane's left edge, and its own place
const STRIP: PixelRect = PixelRect { x: 573, y: 40, w: 6, h: 400 };
const STRIP_HOME: i32 = 694;

/// The widened bar's slide as render/context_bar.rs makes it: its clip ending inside the bar's
/// right ring, its content coming out from where the bar is (`home`), its slide back waiting for
/// the thin pane's
fn bar(open: bool) -> Slide {
    Slide {
        bounds: WIDE,
        clip: PixelRect::new(WIDE.x, WIDE.y, (BAR_RIGHT - STROKE - WIDE.x) as u32, WIDE.h),
        open,
        home: Some((BAR_X - WIDE.x) as f64),
        after_thin: true,
    }
}

/// The thin numbers' pane's: its clip ending at its card's right edge, its content, shadow and
/// all, in past it
fn thin(open: bool) -> Slide {
    let clip = PixelRect { w: (THIN.x + THIN.w as i32 - THIN_BOUNDS.x) as u32, ..THIN_BOUNDS };
    Slide { bounds: THIN_BOUNDS, clip, open, home: None, after_thin: false }
}

fn both(open: bool) -> Visuals {
    Visuals { pane: None, bar: Some(bar(open)), thin: Some(thin(open)) }
}

/// The strip riding the thin pane (out), or drawn at its own place on its way home (back)
fn ride(out: bool) -> Option<(PixelRect, PixelRect, i32)> {
    let at = if out { STRIP } else { PixelRect { x: STRIP_HOME, ..STRIP } };
    Some((at, WINDOW, STRIP_HOME))
}

/// The visuals whose offsets and clip make the edges
#[derive(Debug, Clone, Copy)]
struct Parts {
    /// The widened bar's slider
    bar: Object,
    /// The visual the thin pane's clip is in, which rides the bar's slide
    follow: Object,
    /// The thin pane's slider inside it, and its clip
    thin: Object,
    clip: Object,
    /// The riding strip's slider, and its surface's right edge in the window, where it's placed
    ride: Object,
    strip: i32,
}

impl Screen {
    /// One frame of the context bar's slides, then the commit
    fn context_bar(&self, visuals: Visuals, ride: Option<(PixelRect, PixelRect, i32)>, motion: &MotionSettings) {
        let gpu = &self.gpu;
        self.compositor
            .prepare_context_bar(&gpu.instance, &gpu.device, &base(), visuals, ride, motion)
            .expect("prepare_context_bar");
        self.commit();
    }

    fn parts(&self) -> Parts {
        let layers = self.compositor.context_layers.borrow();
        Parts {
            bar: self.object(layers.drawer.slider.as_ref().expect("the bar's slider")),
            follow: self.object(layers.thin.follow.as_ref().expect("the thin pane's visual riding the bar's slide")),
            thin: self.object(layers.thin.slider.as_ref().expect("the thin pane's slider")),
            clip: self.object(layers.thin.clip.as_ref().expect("the thin pane's clip")),
            ride: self.object(layers.ride.slider.as_ref().expect("the strip's slider")),
            strip: layers.ride.layer.as_ref().map(|l| l.rect.x + l.rect.w as i32).expect("the strip's surface"),
        }
    }
}

/// Where DWM has the edges at `t`: the widened bar's left edge, the thin pane's clip's right edge
/// and its card's left edge, and the strip's right edge, in the window
#[derive(Debug, Clone, Copy)]
struct Edges {
    bar: f64,
    follow: f64,
    clip: f64,
    thin: f64,
    strip: f64,
}

fn edges(tree: &Tree, p: &Parts, t: Instant) -> Edges {
    let shows = |of: Object| tree.shows(of, "OffsetX", t).unwrap_or(0.0);
    let follow = shows(p.follow);
    Edges {
        bar: WIDE.x as f64 + shows(p.bar),
        follow,
        clip: follow + tree.value(p.clip, "Right").expect("the thin pane's clip's right edge") as f64,
        thin: THIN.x as f64 + follow + shows(p.thin),
        strip: p.strip as f64 + shows(p.ride),
    }
}

/// The edges every 5 ms from `from` for `ms`
fn walk(tree: &Tree, p: &Parts, from: Instant, ms: u64) -> Vec<(u64, Edges)> {
    (0..=ms / 5).map(|i| (i * 5, edges(tree, p, from + Duration::from_millis(i * 5)))).collect()
}

/// Coming out: the bar's slide, the thin pane's inside the visual that rides it, and the strip's
/// glide all start in the one commit, wait for nothing, and settle together; the thin pane's clip
/// stays on the bar's moving left ring the whole way (the visual it's in plays the bar's own
/// animation), so the thin pane comes out from under that edge, never seen past it, and the strip
/// rides its left edge: ahead of it by what the thin pane's shadow's reach adds to its slide (its
/// 121 px against the card's 133, so 12 px at the start and none at rest), never over it by
/// half a pixel
#[test]
fn the_thin_pane_comes_out_from_under_the_bar_s_moving_edge_in_its_slide() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.context_bar(both(true), ride(true), &motion);
    let tree = screen.recorder.tree();
    let p = screen.parts();

    let start = tree.started(p.bar, "OffsetX").expect("the bar's slide");
    for (what, of) in [("the visual riding the bar", p.follow), ("the thin pane", p.thin), ("the strip", p.ride)] {
        assert_eq!(tree.started(of, "OffsetX"), Some(start), "{what} starts in the bar's commit\n{tree}");
    }
    let bar_curve = tree.curve(p.bar, "OffsetX").expect("the bar's slide").clone();
    assert_eq!(tree.curve(p.follow, "OffsetX"), Some(&bar_curve), "the thin pane's clip rides the bar's own animation\n{tree}");
    assert_eq!(bar_curve.segments(), Some(on(&motion.panel_open, (BAR_X - WIDE.x) as f64, 0.0, PIXEL_EPSILON)), "{tree}");
    let thin_curve = tree.curve(p.thin, "OffsetX").expect("the thin pane's slide");
    assert_eq!(thin_curve.segments(), Some(on(&motion.panel_open, THIN.w as f64 + REACH, 0.0, PIXEL_EPSILON)), "its own slide inside, from rest at once\n{tree}");
    let ends = [bar_curve.ends(), thin_curve.ends(), tree.curve(p.ride, "OffsetX").and_then(|c| c.ends())].map(|e| e.expect("a slide's end"));
    assert!(ends.iter().all(|e| (e - ends[0]).abs() < 0.05), "{}", format!("one motion, settled when the bar is: ends at {ends:?} s"));

    let mut seen = -REACH;
    for (ms, e) in walk(&tree, &p, start, 600) {
        assert!((e.follow - (e.bar - WIDE.x as f64)).abs() < 1e-6, "{}", format!("at {ms} ms the thin pane's clip is off the bar's edge: {e:?}"));
        assert!((e.clip - (e.bar + STROKE as f64)).abs() < 1e-3, "{}", format!("at {ms} ms the clip ends on the bar's left ring: {e:?}"));
        let shown = e.clip - e.thin;
        assert!((-REACH - 1e-3..=THIN.w as f64 + 1e-3).contains(&shown), "{}", format!("at {ms} ms the thin pane shows {shown} px: {e:?}"));
        assert!(shown >= seen - 1e-3, "{}", format!("at {ms} ms the thin pane went back in, {seen} px to {shown}: {e:?}"));
        seen = shown;
        // The springs' tails end apart by their epsilons, so the strip may be over the card by
        // less than half a pixel there
        assert!(e.strip <= e.thin + 0.5 && e.strip >= e.thin - REACH - STROKE as f64 - 1e-3, "{}", format!("at {ms} ms the strip is off the thin pane's left edge: {e:?}"));
        if ms == 0 {
            assert!((shown + REACH).abs() < 1e-3, "{}", format!("nothing of the thin pane, its shadow included, shows as the bar starts: {e:?}"));
            assert!((e.bar - BAR_X as f64).abs() < 1e-3, "{}", format!("the widened bar starts on the bar's left edge: {e:?}"));
        }
    }
    let end = edges(&tree, &p, start + Duration::from_secs_f64(ends.iter().cloned().fold(0.0, f64::max)));
    assert_eq!((end.bar, end.clip - end.thin, end.strip), (WIDE.x as f64, THIN.w as f64, THIN.x as f64), "out: {end:?}");
}

/// Going back from out (Lachlan's 3a of 2026-11-10, round 3's one commit): the thin pane goes in
/// under the widened bar's still edge, the strip gliding home with it and under the widened bar
/// for the last of the way, and the bar's narrowing, started in the same commit, holds until the
/// thin pane is in; the thin pane's clip goes with the bar as it narrows
#[test]
fn going_back_from_out_the_thin_pane_and_the_strip_go_in_before_the_bar_narrows() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.context_bar(both(true), ride(true), &motion);
    let out = screen.recorder.tree().curve(screen.parts().thin, "OffsetX").and_then(|c| c.ends()).expect("the slide out's end");
    std::thread::sleep(Duration::from_secs_f64(out) + Duration::from_millis(30));
    screen.context_bar(both(false), ride(false), &motion);
    let tree = screen.recorder.tree();
    let p = screen.parts();

    let start = tree.started(p.thin, "OffsetX").expect("the thin pane's slide back");
    assert_eq!(tree.started(p.bar, "OffsetX"), Some(start), "one commit for both\n{tree}");
    assert_eq!(tree.curve(p.follow, "OffsetX"), tree.curve(p.bar, "OffsetX"), "{tree}");
    let thin_in = tree.curve(p.thin, "OffsetX").and_then(|c| c.ends()).expect("the thin pane's slide back's end");
    let walked = walk(&tree, &p, start, 400);
    let mut under = false;
    for &(ms, e) in &walked {
        assert!((e.clip - (e.bar + STROKE as f64)).abs() < 1e-3, "{}", format!("at {ms} ms the clip ends on the bar's left ring: {e:?}"));
        let shown = e.clip - e.thin;
        assert!((-REACH - 1e-3..=THIN.w as f64 + 1e-3).contains(&shown), "{}", format!("at {ms} ms the thin pane shows {shown} px: {e:?}"));
        if (ms as f64) < thin_in * 1000.0 {
            assert!((e.bar - WIDE.x as f64).abs() < 1e-3, "{}", format!("at {ms} ms the bar narrowed before the thin pane was in: {e:?}"));
        }
        under |= e.strip > e.bar + 1.0 && e.bar < (WIDE.x + 1) as f64;
    }
    assert!(under, "{}", format!("the strip never went under the widened bar on its way home: {walked:?}"));
    let last = walked.last().unwrap().1;
    assert_eq!((last.bar, last.strip), (BAR_X as f64, (STRIP_HOME + STRIP.w as i32) as f64), "in: {last:?}");
}

/// Turned round while it's still coming out, the bar goes back at once with the thin pane, from
/// where it is: held for the thin pane's 150 ms there, it would stop dead mid-slide
#[test]
fn turned_round_coming_out_the_bar_goes_back_at_once() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.context_bar(both(true), ride(true), &motion);
    std::thread::sleep(Duration::from_millis(60));
    screen.context_bar(both(false), ride(false), &motion);
    let tree = screen.recorder.tree();
    let p = screen.parts();

    let start = tree.started(p.bar, "OffsetX").expect("the bar's slide back");
    assert_eq!(tree.started(p.thin, "OffsetX"), Some(start), "{tree}");
    assert_eq!(tree.curve(p.follow, "OffsetX"), tree.curve(p.bar, "OffsetX"), "{tree}");
    let at = |ms: u64| edges(&tree, &p, start + Duration::from_millis(ms));
    assert!(at(20).bar > at(0).bar + 1.0, "the bar held where it was rather than going back: {:?} then {:?}", at(0), at(20));
    for (ms, e) in walk(&tree, &p, start, 300) {
        let shown = e.clip - e.thin;
        assert!((-REACH - 1e-3..=THIN.w as f64 + 1e-3).contains(&shown), "{}", format!("at {ms} ms the thin pane shows {shown} px: {e:?}"));
    }
    assert_eq!(at(300).bar, BAR_X as f64, "{:?}", at(300));
}

/// With motion off the thin pane's clip is put where the bar is at once, with it
#[test]
fn with_motion_off_the_thin_pane_s_clip_is_where_the_bar_is() {
    let screen = Screen::new();
    let motion = settings(false);
    screen.context_bar(both(true), ride(true), &motion);
    let tree = screen.recorder.tree();
    let p = screen.parts();
    assert_eq!((tree.value(p.bar, "OffsetX"), tree.value(p.follow, "OffsetX")), (Some(0.0), Some(0.0)), "{tree}");
    screen.context_bar(both(false), None, &motion);
    let tree = screen.recorder.tree();
    let home = (BAR_X - WIDE.x) as f32;
    assert_eq!((tree.value(p.bar, "OffsetX"), tree.value(p.follow, "OffsetX")), (Some(home), Some(home)), "{tree}");
}
