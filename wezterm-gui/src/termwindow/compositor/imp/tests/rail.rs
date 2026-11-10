//! The tab panel's rail over the stand-in device (lbterm/COMPOSITOR.md, "The compositor's
//! tests"): `prepare` given a `PanelPlan` as render/tab_panel.rs makes one, the rail closed, and
//! what it built read back. Each is a rule a perf script judged in a test window before
//! (.claude/docs/perf-scripts.md says which): the marker riding with the row that slides up into
//! the open tab's slot (tab-mouse.ps1's 3b), the rail holding still under a menu's card (Lachlan's
//! 7b, PITFALLS 210), the loops in DWM's tree only while they turn (dwm-loops.ps1), and the
//! Conductor's pad's spectrum coming as it acts and going as the hub lets go (conductor.ps1).

use super::*;
use crate::termwindow::compositor::recorder::Tree;
use crate::termwindow::compositor::{fade_out_lasts, RingFade, RingLight, RingPlan, RowMotion, RowPlan};

/// The rail's tabs, by their keys (render/tab_panel.rs gives each tab's id)
const A: usize = 10;
const B: usize = 11;
const C: usize = 12;

/// The rail's layout in the test's window, at 96 DPI, with `rows` tabs
fn layout(rows: usize) -> lbterm_panel::Layout {
    lbterm_panel::Layout {
        left: 0.0,
        body_top: 38.0,
        bottom: H as f32,
        right: W as f32,
        rail: 68.0,
        width: 290.0,
        scale: 1.0,
        rows,
        label: 0.0,
        label_line: 0.0,
        head: false,
        foot: false,
    }
}

/// An area of the layout in whole pixels, as render/tab_panel.rs's `pixels` rounds it
fn px(a: lbterm_panel::Area) -> PixelRect {
    PixelRect::new(a.x.round() as i32, a.y.round() as i32, a.w.round().max(0.0) as u32, a.h.round().max(0.0) as u32)
}

/// The closed rail's plan with `rows` (each a tab's key and its look), the open tab `open`, and
/// whether the loops turn, as render/tab_panel.rs makes it with motion on and no loops, rings,
/// progress, drag or wave
fn panel(rows: &[(usize, u64)], open: usize, motion: &MotionSettings, spin: bool) -> PanelPlan {
    let layout = layout(rows.len());
    let plans: Vec<RowPlan> = rows
        .iter()
        .enumerate()
        .map(|(i, &(key, look))| RowPlan {
            key,
            slot: px(layout.slot(i)),
            look,
            words_look: 0,
            hovered: false,
            was: None,
        })
        .collect();
    let marker = rows.iter().position(|&(key, _)| key == open).map(|i| (open, px(layout.slot(i)).y));
    PanelPlan {
        surface: px(layout.surface()),
        cap: px(layout.cap_surface()),
        seam: layout.seam(true).round() as i32,
        travel: layout.travel().round() as u32,
        panes_left: layout.panes_left().round() as i32,
        panes_right: layout.panes_right().round() as i32,
        open: false,
        edge: motion.panel_close.clone(),
        glow: motion.panel_glow.clone(),
        light: 0.0,
        hover: motion.hover.clone(),
        words: motion.exit.clone(),
        words_delay: 0.0,
        spot: px(layout.spot_surface()),
        spot_core: px(layout.spot_core_surface()),
        sweep: motion.panel_sweep.clone(),
        layout,
        spinners: vec![],
        spin,
        start_spinner: None,
        rows: plans,
        row_motion: RowMotion {
            slide: motion.layout.clone(),
            enter: motion.enter.clone(),
            exit: motion.exit.clone(),
            swap: motion.swap.clone(),
            hover: motion.hover.clone(),
            marker: motion.focus_move.clone(),
        },
        marker,
        progress: vec![],
        dragged: None,
        rings: vec![],
        ring_template: PixelRect::new(400, 400, 64, 64),
        light_template: PixelRect::new(480, 400, 16, 16),
        wave: None,
        cards: false,
    }
}

impl Screen {
    /// One frame of one pane with the rail as `plan` has it, then the commit
    fn rail(&self, plan: &PanelPlan, motion: &MotionSettings) {
        let gpu = &self.gpu;
        self.compositor
            .prepare(
                &gpu.instance,
                &gpu.device,
                &base(),
                WINDOW,
                &[(0, WINDOW)],
                motion,
                Some(plan),
                false,
                &cards(),
                Some(0),
                &|_: PaneId| false,
                BACKDROP,
            )
            .expect("prepare");
        self.commit();
    }

    /// The names in flight from `since` on that start with `prefix`
    fn names(&self, since: Instant, prefix: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .compositor
            .animations(since)
            .into_iter()
            .map(|a| a.name)
            .filter(|n| n.starts_with(prefix))
            .collect();
        names.sort();
        names
    }

    /// How long the state file says `name` has left at `since`, the longest of it
    fn left_ms(&self, since: Instant, name: &str) -> Option<u64> {
        self.compositor
            .animations(since)
            .into_iter()
            .filter(|a| a.name == name)
            .map(|a| a.left_ms)
            .max()
    }
}

/// The open tab closed (Close tab, Ctrl+W, a middle click): the row below slides up into its
/// slot and opens, and the marker rides up with it on the rows' `slide` from where that row
/// shows, so it ends as the row's slide ends, where a glide on `focus_move` would end 80 ms or more
/// after (Lachlan's 3b; lbterm_motion::rail::marker's `Slide`; FLOWS.md, "The tab menu's facts",
/// step 10). tab-mouse.ps1 judged the 5 ms in a window until feat/dcomp-recorder
#[test]
fn the_marker_rides_with_the_row_sliding_into_the_open_tab_s_slot() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.rail(&panel(&[(A, 1), (B, 2)], A, &motion, true), &motion);
    let before = Instant::now();
    screen.rail(&panel(&[(B, 2)], B, &motion, true), &motion);

    let names = screen.names(before, "rail.");
    for name in ["rail.marker", "rail.row.exit", "rail.row.slide"] {
        assert!(names.iter().any(|n| n == name), "no {} in {:?}", name, names);
    }
    let marker = screen.left_ms(before, "rail.marker").unwrap();
    let slide = screen.left_ms(before, "rail.row.slide").unwrap();
    assert!(
        marker <= slide + 5,
        "the marker has {} ms to go and the slowest slide {}: it glided rather than riding with its row", marker, slide
    );
    // Its visual is what moves (drawn at the slot, offset from the rail's top by how far its row
    // is from there): up from where the row showed, a slot lower, to none
    let tree = screen.recorder.tree();
    let layers = screen.compositor.layers.borrow();
    let rail = layers.panel.as_ref().expect("the rail");
    let visual = screen.object(&rail.marker.visual);
    let ride = tree.curve(visual, "OffsetY").expect("the marker moving");
    let rest = px(layout(1).surface()).y as f64;
    assert!(ride.value(0.0) > rest && close(ride.value(ride.ends().unwrap()), rest), "{}", tree);
}

/// The rail holds still under a menu's card, open or fading out (`PanelPlan::cards`; Lachlan's 7b
/// of 2026-11-08, lbterm_motion::rail::Hold): a tab closed and another opened as the card fades
/// out, a row's look changing and the marker moving to another tab play nothing under it, none of
/// the rows' and the marker's names (of the harness's `Get-RailUnderCard`'s Held, those the
/// compositor drives); the first frame after the card has gone plays it all from what the tree
/// still shows. The progress fills the compositor holds too aren't driven here (the plan has no
/// progress), and the panel's edge, light, words and sweep and a pad's glint are held before the
/// plan reaches the compositor (render/tab_panel.rs's `Hover::hold`, render/lights.rs), so the
/// windows' `New-UnderCardWatch` judges those
#[test]
fn the_rail_holds_still_under_a_menu_s_card_and_plays_it_after() {
    let screen = Screen::new();
    let motion = settings(true);
    let before_any = panel(&[(A, 1), (B, 2)], A, &motion, true);
    screen.rail(&before_any, &motion);
    // The card comes over the window
    let mut held = before_any.clone();
    held.cards = true;
    screen.rail(&held, &motion);

    // As it fades out: A closed, B's look changed and open, C new
    let mark = screen.recorder.mark();
    let before = Instant::now();
    let mut changed = panel(&[(B, 3), (C, 4)], B, &motion, true);
    changed.cards = true;
    screen.rail(&changed, &motion);
    let handed = screen.recorder.animated(mark);
    assert!(handed.is_empty(), "the rail moved under the card: {handed:?}\n{}", screen.recorder.tree());
    assert_eq!(screen.names(before, "rail."), Vec::<String>::new());

    // The card gone: all of it plays
    let after = Instant::now();
    changed.cards = false;
    screen.rail(&changed, &motion);
    let names = screen.names(after, "rail.");
    for name in ["rail.marker", "rail.row.exit", "rail.row.slide"] {
        assert!(names.iter().any(|n| n == name), "no {} once the card has gone: {:?}", name, names);
    }
}

/// Everything in the tree DWM plays for ever: each property, of a visual or what it links (its
/// clip, effect or transform), handed an animation that repeats
fn loops_in(tree: &Tree) -> Vec<String> {
    let mut found = vec![];
    for (_, visual) in tree.walk() {
        let linked = ["Clip", "Effect", "Transform"].iter().filter_map(|l| tree.link(visual, *l));
        for object in std::iter::once(visual).chain(linked) {
            for (name, prop) in tree.props(object) {
                if prop.curve().is_some_and(|c| c.lap().is_some()) {
                    found.push(format!("{} {object:?} {name}", tree.kind(object)));
                }
            }
        }
    }
    found
}

/// The loops (lbterm/COMPOSITOR.md, "Loops"): a waiting pad's arc turning and the Conductor's
/// outline's hues turning with its light going round, in DWM's tree only while the plan's `spin`
/// says they turn (lbterm_extras::work::loops_play: motion on and the window seen). Held still
/// (minimized), none is left in the tree for DWM to compose every refresh, the arc and the outline
/// and its light still shown where they were; then turning again. dwm-loops.ps1 judges the window
/// minimized and restored, the paints it makes meanwhile and DWM turning them
#[test]
fn the_loops_are_in_the_tree_only_while_they_turn() {
    let screen = Screen::new();
    let motion = settings(true);
    let plan = |spin: bool| {
        let mut plan = panel(&[(A, 1), (B, 2)], A, &motion, spin);
        plan.spinners = vec![px(plan.layout.spinner(0))];
        let pad = px(plan.layout.slot(1));
        plan.rings = vec![RingPlan {
            rect: PixelRect::new(pad.x, pad.y, 64, 64),
            style: RingStyle::Spectrum,
            light: Some(RingLight {
                lap: 2.0,
                opacity: 0.8,
                offsets: vec![(0.0, 0.0), (48.0, 0.0), (48.0, 48.0), (0.0, 48.0)],
            }),
            fade: None,
        }];
        plan
    };
    let all = vec!["rail.spinner", "conductor.outline", "conductor.light"];

    screen.rail(&plan(true), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(screen.compositor.loops(), all);
    let turning = loops_in(&tree);
    assert_eq!(turning.len(), 4, "the arc's angle, the hues' and the light's x and y: {turning:?}\n{tree}");
    let shown = |tree: &Tree| {
        let layers = screen.compositor.layers.borrow();
        let rail = layers.panel.as_ref().expect("the rail");
        let spinner = screen.object(&rail.spinners[0].visual);
        let ring = screen.object(&rail.rings[0].visual);
        let light = screen.object(rail.rings[0].light.as_ref().expect("the outline's light"));
        assert!(tree.shown(spinner) && tree.shown(ring) && tree.shown(light), "{}", tree);
        (spinner, ring, light)
    };
    shown(&tree);

    // Held still: none of them in the tree, all still shown
    screen.rail(&plan(false), &motion);
    let tree = screen.recorder.tree();
    assert!(screen.compositor.loops().is_empty(), "{:?}", screen.compositor.loops());
    assert_eq!(loops_in(&tree), Vec::<String>::new(), "{tree}");
    let (spinner, ring, light) = shown(&tree);
    let rotate = tree.link(spinner, "Transform").expect("the arc's rotation");
    assert_eq!(tree.value(rotate, "Angle"), Some(0.0), "{tree}");
    assert_eq!(tree.link(ring, "Effect"), None, "a hue rotation held still\n{tree}");
    assert_eq!((tree.value(light, "OffsetX"), tree.value(light, "OffsetY")), (Some(0.0), Some(0.0)), "{tree}");

    // Turning again
    screen.rail(&plan(true), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(screen.compositor.loops(), all);
    assert_eq!(loops_in(&tree).len(), 4, "{tree}");
    shown(&tree);
}

/// The Conductor's own pad acting (Lachlan's 7a; render/tab_panel.rs gives its ring, every hue,
/// with a `fade`): at rest no ring and nothing turning; as it acts the spectrum fades in from
/// nothing on its curve under `conductor.outline.fade`, its hues turning once round every
/// `HUE_PERIOD` for as long as it shows (`loops` says `conductor.outline`); as the hub lets go it
/// fades out on the same curve from where it is, the same visual; and once the plan no longer has
/// it, it's out of the tree with nothing turning. In read only it goes at once, handing DWM
/// nothing (conductor.ps1's steps 2 and 3: no `conductor.outline` among the loops at rest or let
/// go, and among them while it acts; the fades' names stay judged there, where they're what says
/// the hub's push reached the compositor)
#[test]
fn the_conductor_s_spectrum_turns_only_while_it_shows() {
    use crate::termwindow::render::conductor::HUE_PERIOD;
    let screen = Screen::new();
    let motion = settings(true);
    let plan = |fade: Option<RingFade>| {
        let mut plan = panel(&[(A, 1), (B, 2)], A, &motion, true);
        let pad = px(plan.layout.slot(1));
        plan.rings = fade
            .map(|fade| RingPlan {
                rect: PixelRect::new(pad.x, pad.y, 64, 64),
                style: RingStyle::Spectrum,
                light: None,
                fade: Some(fade),
            })
            .into_iter()
            .collect();
        plan
    };
    let lit = |lit: bool, curve: &Curve| Some(RingFade { lit, curve: curve.clone() });
    let ring = || {
        let layers = screen.compositor.layers.borrow();
        let rail = layers.panel.as_ref().expect("the rail");
        assert_eq!(rail.rings.len(), 1, "the pad's ring");
        screen.object(&rail.rings[0].visual)
    };
    let turning = || screen.compositor.loops().contains(&"conductor.outline");

    // At rest
    screen.rail(&plan(None), &motion);
    assert!(!turning(), "{:?}", screen.compositor.loops());
    assert_eq!(loops_in(&screen.recorder.tree()), Vec::<String>::new());

    // Acting: the spectrum fading in, its hues turning
    let before = Instant::now();
    screen.rail(&plan(lit(true, &motion.panel_glow)), &motion);
    let tree = screen.recorder.tree();
    let spectrum = ring();
    assert!(tree.shown(spectrum), "{}", tree);
    let fade = tree.curve(spectrum, "Opacity").expect("the spectrum fading in");
    assert_eq!(fade.segments(), Some(on(&motion.panel_glow, 0.0, 1.0, OPACITY_EPSILON)), "{tree}");
    assert_eq!(screen.played("conductor.outline.fade", before), Some(vec![fade.played().unwrap()]));
    let hues = tree.link(spectrum, "Effect").expect("the hue rotation");
    assert_eq!(tree.kind(hues), "hue rotation", "{tree}");
    let angle = tree.curve(hues, "Angle").expect("the hues turning");
    assert_eq!(angle.lap(), Some(HUE_PERIOD), "{tree}");
    for (t, want) in [(0.0, 0.0), (HUE_PERIOD / 2.0, 180.0), (HUE_PERIOD * 1.25, 90.0)] {
        assert!((angle.value(t) - want).abs() < 0.01, "at {t} s the hues at {} degrees", angle.value(t));
    }
    assert!(turning(), "{:?}", screen.compositor.loops());

    // Let go, once it has come in whole: fading out from all of it, the same visual turning on
    std::thread::sleep(fade_out_lasts(&motion.panel_glow) + Duration::from_millis(20));
    let before = Instant::now();
    screen.rail(&plan(lit(false, &motion.panel_glow)), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(ring(), spectrum, "the same ring, not made again");
    let fade = tree.curve(spectrum, "Opacity").expect("the spectrum fading out");
    assert_eq!(fade.segments(), Some(on(&motion.panel_glow, 1.0, 0.0, OPACITY_EPSILON)), "{tree}");
    assert_eq!(screen.played("conductor.outline.fade", before), Some(vec![fade.played().unwrap()]));

    // Gone from the plan: out of the tree, nothing turning
    std::thread::sleep(fade_out_lasts(&motion.panel_glow) + Duration::from_millis(20));
    screen.rail(&plan(None), &motion);
    let tree = screen.recorder.tree();
    assert!(!tree.shown(spectrum), "{}", tree);
    assert!(!turning(), "{:?}", screen.compositor.loops());
    assert_eq!(loops_in(&tree), Vec::<String>::new(), "{tree}");

    // Read only while it acts: lit, then gone at once with nothing handed to DWM
    screen.rail(&plan(lit(true, &motion.panel_glow)), &motion);
    std::thread::sleep(fade_out_lasts(&motion.panel_glow) + Duration::from_millis(20));
    let mark = screen.recorder.mark();
    let before = Instant::now();
    screen.rail(&plan(lit(false, &Curve::Instant)), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(tree.value(ring(), "Opacity"), Some(0.0), "{tree}");
    assert_eq!(screen.recorder.animated(mark).iter().filter(|h| h.property == "Opacity").count(), 0);
    assert_eq!(screen.played("conductor.outline.fade", before), None);
}
