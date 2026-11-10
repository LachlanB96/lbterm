//! Tabs in flight over the stand-in device (lbterm/COMPOSITOR.md, "The compositor's tests";
//! `lbterm_motion::flight`): five tabs switched through faster than each fades, and back to the
//! oldest, as tab-switch.ps1's step 9 and side-strips.ps1's step 3b switch them in a window. What
//! each tab shows of itself is worked out from what DWM was handed, as DWM plays and composes it:
//! every animation from the Commit that handed it over (the recorder's `Tree::started`), the
//! panes' containers and the strips' pictures in their stacks, top to bottom, each showing its
//! opacity of what the ones over it leave. No tab in flight ever shows more of itself than it did
//! a moment before, the tab switched back to comes up from what it showed, and at a switch, and
//! between the strips and the panes, nothing moves further than the flight does in the spread of
//! the curves' lags: how long before its commit the compositor worked each out (`prepare`'s or
//! `prepare_sides`' own moment, PITFALLS 298 and 304), which DWM shows and this test measures and
//! allows, no more. The scripts judged this from captures until feat/dcomp-recorder; they keep
//! their captures for look and what only a window shows (the strip's words, the outlines, the
//! mux's switches).

use super::*;
use crate::termwindow::compositor::recorder::{Prop, Tree};
use std::collections::HashMap;

/// The five tabs, by their panes: tab one split in two with the keyboard in its right pane, the
/// others one pane each
const TABS: [&[PaneId]; 5] = [&[0, 1], &[2], &[3], &[4], &[5]];
/// Where the strips down the window's right are, which their picture is drawn over
const SIDES: PixelRect = PixelRect {
    x: (W - 40) as i32,
    y: 0,
    w: 40,
    h: H,
};
/// A point in each half: tab one's left pane is under the first and its right pane under the
/// second, every other tab's one pane under both
const POINTS: [(i32, i32); 2] = [(200, 300), (600, 300)];
/// How far a share may move against its way between two moments, or at a switch beyond what the
/// lags allow: DWM plays each opacity from samples within `OPACITY_EPSILON`, and a share is a
/// product of them
const SAMPLED: f64 = 0.005;
const SWITCH: f64 = 0.01;
/// How often a share is read between two frames
const EVERY: Duration = Duration::from_millis(2);

/// The config's motion on, slowed `by` (`lbterm_motion.slowdown`): `tab_switch` 1 s at 5, so
/// four tabs are in flight however long WARP takes to make each tab's surface
fn slowed(by: f64) -> MotionSettings {
    let mut motion = ::config::lbterm::LbtermMotion::default();
    motion.enabled = "on".to_string();
    motion.slowdown = by;
    MotionSettings::from_config(&motion)
}

fn tab_of(pane: PaneId) -> usize {
    TABS.iter().position(|t| t.contains(&pane)).expect("a pane of the test's")
}

fn rect_of(pane: PaneId) -> PixelRect {
    match pane {
        0 => LEFT,
        1 => RIGHT,
        _ => WINDOW,
    }
}

fn under(r: PixelRect, (x, y): (i32, i32)) -> bool {
    x >= r.x && x < r.x + r.w as i32 && y >= r.y && y < r.y + r.h as i32
}

/// One opacity in a stack: what it holds, the commit DWM started its animation at, and its lag,
/// how long before that commit the compositor worked the animation out (a pane's
/// `PaneLayer::alpha` start, `prepare`'s moment; a picture's `done` less its curve's length,
/// `prepare_sides`'). An animation shows its stretch of the flight `lag` late
struct Alpha {
    prop: Prop,
    started: Option<Instant>,
    lag: Option<Duration>,
}

impl Alpha {
    /// What it shows at `t` as DWM plays it: from the commit that handed it over
    fn at(&self, t: Instant) -> f64 {
        let alpha = match (&self.prop, self.started) {
            (Prop::Value(v), _) => *v as f64,
            (Prop::Animation(c), Some(s)) => c.value(t.saturating_duration_since(s).as_secs_f64()),
            (Prop::Animation(_), None) => panic!("an opacity animated and not committed"),
        };
        alpha.clamp(0.0, 1.0)
    }
}

/// What one frame left on the screen: its commit, the committed tree, the tab on screen, and
/// each opacity in the two stacks
struct Shot {
    /// The frame's commit, from which DWM plays what it handed over
    at: Instant,
    on: usize,
    tree: Tree,
    panes_visual: Object,
    sides_visual: Object,
    /// Each pane by its container: its id and its visual's opacity
    panes: HashMap<Object, (PaneId, Alpha)>,
    /// Each of the strips' pictures by its visual: its tab and its opacity
    sides: HashMap<Object, (usize, Alpha)>,
}

impl Shot {
    fn take(screen: &Screen, on: usize) -> Self {
        let tree = screen.recorder.tree();
        let at = screen
            .recorder
            .calls()
            .iter()
            .rev()
            .find_map(|c| match c {
                Call::Commit { at } => Some(*at),
                _ => None,
            })
            .expect("the frame's commit");
        let mut panes = HashMap::new();
        {
            let layers = screen.compositor.layers.borrow();
            let all = layers
                .panes
                .iter()
                .map(|(i, p)| (*i, p))
                .chain(layers.exiting.iter().map(|(i, p, ..)| (*i, p)));
            for (id, pane) in all {
                let container = screen.object(&pane.container);
                assert_eq!(tree.prop(container, "Opacity"), None, "pane {id}'s container has an opacity\n{tree}");
                let visual = screen.object(&pane.opacity);
                let prop = tree.prop(visual, "Opacity").cloned().unwrap_or(Prop::Value(1.0));
                let started = tree.started(visual, "Opacity");
                let lag = match (prop.curve(), started, pane.alpha.as_ref()) {
                    (Some(_), Some(s), Some(a)) => Some(s.saturating_duration_since(a.start)),
                    _ => None,
                };
                panes.insert(container, (id, Alpha { prop, started, lag }));
            }
        }
        let mut sides = HashMap::new();
        for p in &screen.compositor.sides_layers.borrow().fading {
            let visual = screen.object(&p.layer.visual);
            let prop = tree.prop(visual, "Opacity").cloned().expect("a picture's opacity");
            let started = tree.started(visual, "Opacity");
            // Played from the flight's clock to its end (`prepare_sides`): worked out its length
            // before it's done
            let lag = prop.curve().map(|c| {
                let worked = p.done - Duration::from_secs_f64(c.ends().expect("a picture's fade ends"));
                started.expect("a picture's fade committed").saturating_duration_since(worked)
            });
            sides.insert(visual, (tab_of(p.panes[0]), Alpha { prop, started, lag }));
        }
        Shot {
            at,
            on,
            panes_visual: screen.panes(),
            sides_visual: screen.object(&screen.compositor.sides),
            tree,
            panes,
            sides,
        }
    }

    /// The lags of every animated opacity on screen after this frame
    fn lags(&self) -> Vec<Duration> {
        let panes = self.panes.values().filter_map(|(_, a)| a.lag);
        let sides = self.sides.values().filter_map(|(_, a)| a.lag);
        panes.chain(sides).collect()
    }

    /// Each tab's share of the screen at `point` at `t`, as DWM composes the panes' stack
    fn panes_share(&self, point: (i32, i32), t: Instant) -> [f64; 5] {
        let mut share = [0.0; 5];
        let mut left = 1.0;
        for c in self.tree.children(self.panes_visual).iter().rev() {
            let (id, alpha) = self
                .panes
                .get(c)
                .unwrap_or_else(|| panic!("{c:?} in the panes' visual is no pane's\n{}", self.tree));
            if !under(rect_of(*id), point) {
                continue;
            }
            let alpha = alpha.at(t);
            share[tab_of(*id)] += alpha * left;
            left *= 1.0 - alpha;
        }
        share
    }

    /// Each tab's share of the strips at `t`: the pictures' stack over the strips the tab on
    /// screen draws
    fn sides_share(&self, t: Instant) -> [f64; 5] {
        let mut share = [0.0; 5];
        let mut left = 1.0;
        for c in self.tree.children(self.sides_visual).iter().rev() {
            let (tab, alpha) = self
                .sides
                .get(c)
                .unwrap_or_else(|| panic!("{c:?} in the strips' visual is no picture's\n{}", self.tree));
            let alpha = alpha.at(t);
            share[*tab] += alpha * left;
            left *= 1.0 - alpha;
        }
        share[self.on] += left;
        share
    }

    /// How far each tab's share of the panes at `point` moves within `by` of `t`, either way:
    /// what a lag of `by` more or less shows of the flight there
    fn moves(&self, point: (i32, i32), t: Instant, by: Duration) -> [f64; 5] {
        let now = self.panes_share(point, t);
        let later = self.panes_share(point, t + by);
        let sooner = self.panes_share(point, t.checked_sub(by).unwrap_or(t));
        let mut most = [0.0; 5];
        for tab in 0..5 {
            most[tab] = (later[tab] - now[tab]).abs().max((sooner[tab] - now[tab]).abs());
        }
        most
    }
}

/// How far apart the lags of the shots' animated opacities are: the most the stretches of the
/// flight they show can be apart in time, as DWM plays them
fn spread(shots: &[&Shot]) -> Duration {
    let lags: Vec<Duration> = shots.iter().flat_map(|s| s.lags()).collect();
    match (lags.iter().min(), lags.iter().max()) {
        (Some(least), Some(most)) => *most - *least,
        _ => Duration::ZERO,
    }
}

impl Screen {
    /// A frame of tab `tab` as a paint makes it with the strips beside it: `prepare`, the strips'
    /// picture over where they are (`prepare_sides`, after it, as render/composited.rs calls it),
    /// then the commit
    fn tab(&self, tab: usize, motion: &MotionSettings) {
        let panes: Vec<(PaneId, PixelRect)> = TABS[tab].iter().map(|&p| (p, rect_of(p))).collect();
        let active = TABS[tab].last().copied();
        self.paint(&panes, active, motion, &|id: PaneId| !TABS[tab].contains(&id));
        let gpu = &self.gpu;
        self.compositor
            .prepare_sides(&gpu.instance, &gpu.device, &base(), Some(SIDES), motion)
            .expect("prepare_sides");
        self.commit();
    }
}

fn shares(shares: &[f64; 5]) -> String {
    shares.iter().map(|s| format!("{s:.3}")).collect::<Vec<_>>().join(" ")
}

/// Lachlan's 3a: from tab one to five by the side buttons 40 ms apart, and back to one 40 ms
/// after, so four tabs are in flight and the one switched back to is the oldest, on top of them
/// all. Read every 2 ms from each frame's commit to the next and on until the last is done, as
/// DWM plays each animation from its commit: each tab in flight shows no more of itself than it
/// did 2 ms before, at either half of the window and on the strips (tab-switch.ps1's "never
/// rises while in flight", side-strips.ps1's "two's never rises again"); at each switch nothing
/// moves further than the flight does in the spread of the lags then (tab-switch.ps1's "never
/// steps further than the curve", side-strips.ps1's "one's never jumps"); tab one comes back from
/// what its fade had got to and only rises, to all of the screen; tab one's two panes show the
/// same share; and the strips' pictures each tab's share as its panes do, apart by no more than
/// the flight moves in the spread of the lags (side-strips.ps1's "keeps its panes' to 0.12";
/// PITFALLS 298)
#[test]
fn tabs_in_flight_never_show_more_of_themselves_than_a_moment_before() {
    let screen = Screen::new();
    let motion = slowed(5.0);
    let between = Duration::from_millis(40);
    // The first frame is a resize (PITFALLS 295)
    screen.tab(0, &motion);
    let mut shots = vec![Shot::take(&screen, 0)];
    for tab in [1, 2, 3, 4, 0] {
        std::thread::sleep(between);
        if tab == 0 {
            assert_eq!(screen.compositor.layers.borrow().flight.len(), 4, "four tabs in flight at the click");
        }
        screen.tab(tab, &motion);
        shots.push(Shot::take(&screen, tab));
    }
    {
        let layers = screen.compositor.layers.borrow();
        let left: Vec<_> = layers.flight.leaving().iter().map(|l| l.key.clone()).collect();
        assert_eq!(left, vec![vec![2], vec![3], vec![4], vec![5]], "tab one out of the stack, five in at its bottom");
    }
    let back = shots.len() - 1;
    // When the last of them is done on the compositor's clock (`fading_panes` gives the soonest),
    // and on the screen, its lag after
    let done = {
        let layers = screen.compositor.layers.borrow();
        layers.exiting.iter().map(|(.., done, _)| *done).max().expect("the tabs still in flight")
    };
    let shown = done + shots[back].lags().into_iter().max().unwrap_or_default();

    // At a switch: what the frame before left, at this one's commit, is what this one shows, but
    // for what the flight moves in the spread of the two frames' lags. The two frames' flights part
    // at the switch (the tab switched to starts rising where it was falling), so over a spread
    // they can part by what each moves in it, the two together
    let mut switches = vec![];
    for k in 1..shots.len() {
        let (was, is) = (&shots[k - 1], &shots[k]);
        let t = is.at;
        let lags = spread(&[was, is]);
        switches.push(lags.as_millis());
        let allowed = |point| {
            let (a, b) = (was.moves(point, t, lags), is.moves(point, t, lags));
            [0, 1, 2, 3, 4].map(|tab| a[tab] + b[tab])
        };
        for point in POINTS {
            let (before, after) = (was.panes_share(point, t), is.panes_share(point, t));
            let allow = allowed(point);
            for tab in 0..5 {
                assert!(
                    (before[tab] - after[tab]).abs() < allow[tab] + SWITCH,
                    "switch {k} to tab {}: at {point:?} the tabs showed [{}] and the switch [{}], {:.3} allowed for lags {} ms apart\n{}",
                    is.on + 1,
                    shares(&before),
                    shares(&after),
                    allow[tab] + SWITCH,
                    lags.as_millis(),
                    is.tree
                );
            }
        }
        let (before, after) = (was.sides_share(t), is.sides_share(t));
        let allow = allowed(POINTS[1]);
        for tab in 0..5 {
            assert!(
                (before[tab] - after[tab]).abs() < allow[tab] + SWITCH,
                "switch {k} to tab {}: the strips showed [{}] and the switch [{}], {:.3} allowed for lags {} ms apart",
                is.on + 1,
                shares(&before),
                shares(&after),
                allow[tab] + SWITCH,
                lags.as_millis()
            );
        }
    }

    // From each commit to the next, and after the last until the tabs in flight are done
    let mut read = 0;
    let mut widest = Duration::ZERO;
    for k in 1..shots.len() {
        let shot = &shots[k];
        let until = shots.get(k + 1).map_or(shown, |s| s.at);
        let lags = spread(&[shot]);
        widest = widest.max(lags);
        let mut last: Option<([f64; 5], [f64; 5], [f64; 5])> = None;
        let mut t = shot.at;
        while t <= until {
            let now = (
                shot.panes_share(POINTS[0], t),
                shot.panes_share(POINTS[1], t),
                shot.sides_share(t),
            );
            let allow = shot.moves(POINTS[1], t, lags);
            let ms = t.saturating_duration_since(shots[1].at).as_millis();
            for tab in 0..5 {
                assert!(
                    (now.0[tab] - now.1[tab]).abs() < SWITCH,
                    "{ms} ms: tab {}'s halves apart, [{}] and [{}]",
                    tab + 1,
                    shares(&now.0),
                    shares(&now.1)
                );
                assert!(
                    (now.2[tab] - now.1[tab]).abs() < allow[tab] + SWITCH,
                    "{ms} ms: tab {}'s strips and panes apart, [{}] and [{}], {:.3} allowed for lags {} ms apart",
                    tab + 1,
                    shares(&now.2),
                    shares(&now.1),
                    allow[tab] + SWITCH,
                    lags.as_millis()
                );
            }
            if let Some(was) = last {
                for (what, was, now) in [("panes", was.1, now.1), ("strips", was.2, now.2)] {
                    for tab in 0..5 {
                        if tab != shot.on {
                            assert!(
                                now[tab] <= was[tab] + SAMPLED,
                                "{ms} ms, on tab {}: tab {} in flight rose in its {what} from {:.3} to {:.3}",
                                shot.on + 1,
                                tab + 1,
                                was[tab],
                                now[tab]
                            );
                        } else if k == back {
                            assert!(
                                now[tab] >= was[tab] - SAMPLED,
                                "{ms} ms: tab one, switched back to, fell in its {what} from {:.3} to {:.3}",
                                was[tab],
                                now[tab]
                            );
                        }
                    }
                }
            }
            last = Some(now);
            read += 1;
            t += EVERY;
        }
    }
    assert!(read > 400, "only {} moments read", read);
    // The lags allowed for are a frame's own work before its commit, on WARP: well short of the
    // flight, or the allowances above would let anything through
    assert!(widest < Duration::from_millis(100), "lags {} ms apart in one frame", widest.as_millis());
    // Said for `--nocapture`: what the lags were on this run (COMPOSITOR.md, "What they hold")
    println!(
        "flight: lags at most {} ms apart in a frame, {:?} ms at the switches",
        widest.as_millis(),
        switches
    );
    let end = shots[back].panes_share(POINTS[1], shown);
    assert!(end[0] > 0.99, "tab one at the end: [{}]", shares(&end));
    let at_click = shots[back - 1].panes_share(POINTS[1], shots[back].at)[0];
    assert!(
        at_click > SWITCH && at_click < 1.0 - SWITCH,
        "tab one, switched back to, had shown {:.3}: in flight and still seen", at_click
    );

    // Done: the next frame puts the tabs left away, the panes' and the strips' pictures
    std::thread::sleep(shown.saturating_duration_since(Instant::now()) + Duration::from_millis(20));
    screen.tab(0, &motion);
    let last = Shot::take(&screen, 0);
    assert_eq!(screen.compositor.fading_panes().0, Vec::<PaneId>::new());
    assert!(screen.compositor.layers.borrow().flight.is_empty());
    assert!(last.sides.is_empty(), "{}", last.tree);
    assert_eq!(last.panes.len(), 2, "{}", last.tree);
}
