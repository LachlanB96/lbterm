//! The compositor's tests: compositor.rs over the stand-in DirectComposition device
//! (compositor/recorder.rs), made through `Compositor::with_device` as `new` makes it over
//! DirectComposition's, each frame driven through `prepare` and committed as a paint does
//! (render/composited.rs), then the tree and the animations it built read back from the
//! recorder (lbterm/COMPOSITOR.md, "Checking it without a window"). The surfaces are real, on
//! WARP (`recorder::gpu()`), so no test holds the GPU or opens a window.
#![deny(non_fmt_panics)]

use super::super::recorder::{gpu, Call, Mark, Object, Recorder, Turn};
use super::*;
use lbterm_extras::state::Played;

/// The window the tests draw into: small, so WARP makes its surfaces quickly
const W: u32 = 800;
const H: u32 = 600;
const WINDOW: PixelRect = PixelRect { x: 0, y: 0, w: W, h: H };
/// Its two halves, as a split leaves them
const LEFT: PixelRect = PixelRect { x: 0, y: 0, w: W / 2, h: H };
const RIGHT: PixelRect = PixelRect { x: (W / 2) as i32, y: 0, w: W / 2, h: H };
const HALF: f64 = (W / 2) as f64;
/// The window's background, which the panes without the keyboard dim toward
const BACKDROP: [f64; 3] = [0.1, 0.1, 0.1];

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// The config's motion settings, its curves as they come, on or off
fn settings(on: bool) -> MotionSettings {
    let mut motion = ::config::lbterm::LbtermMotion::default();
    motion.enabled = if on { "on" } else { "off" }.to_string();
    MotionSettings::from_config(&motion)
}

/// What `drive` hands DWM for `curve` from `from` to `to`, starting still, with no delay
fn on(curve: &Curve, from: f64, to: f64, epsilon: f64) -> Segments {
    Motion::new(from, to, 0.0, curve.clone()).segments(STEP, epsilon)
}

/// The card templates for the window (render/card.rs draws them): a strip as wide as the window
/// and one as tall
fn cards() -> CardPlan {
    CardPlan {
        wide: PixelRect::new(0, 0, W, 32),
        tall: PixelRect::new(0, 0, 32, H),
        size: 32,
        corner: 16,
        line: 2,
        reach: 6.0,
    }
}

/// The window's surface configuration, which every surface the compositor makes takes its format
/// and present mode from (webgpu.rs makes it with the compositor on)
fn base() -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        format: wgpu::TextureFormat::Bgra8Unorm,
        width: W,
        height: H,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
    }
}

/// A pane's visuals: the container (its clip while it rides in, its dimming), the visual its
/// surface shows on (offset, opacity), the visual's clip (its size) and the container's clip
#[derive(Debug, Clone, Copy)]
struct Pane {
    container: Object,
    visual: Object,
    clip: Object,
    bounds: Object,
}

/// A compositor over the stand-in, and what it recorded
struct Screen {
    recorder: Recorder,
    compositor: Compositor,
    /// Where the log stood at the last frame's end, which `commit` reads on from
    checked: std::cell::Cell<Mark>,
    /// The test's turn with the WARP device, taken before the compositor makes a surface and let
    /// go after it and every surface are dropped: declared after `compositor`, so dropped after
    /// it (PITFALLS 303)
    gpu: Turn,
}

impl Screen {
    fn new() -> Self {
        let gpu = gpu();
        let recorder = Recorder::default();
        let compositor = Compositor::with_device(recorder.device(), std::ptr::null_mut(), true)
            .expect("the compositor over the stand-in device");
        Self {
            checked: std::cell::Cell::new(recorder.mark()),
            recorder,
            compositor,
            gpu,
        }
    }

    /// One frame of `panes`, `active` with the keyboard, as a paint makes it: `prepare`, then
    /// the commit. Nothing it changed is left uncommitted
    fn frame(&self, panes: &[(PaneId, PixelRect)], active: Option<PaneId>, motion: &MotionSettings, left: &dyn Fn(PaneId) -> bool) {
        self.paint(panes, active, motion, left);
        self.commit();
    }

    /// A paint's `prepare` of `panes`, with no commit after it, for a frame that prepares more
    /// before its commit (the strips' pictures, `prepare_sides`)
    fn paint(&self, panes: &[(PaneId, PixelRect)], active: Option<PaneId>, motion: &MotionSettings, left: &dyn Fn(PaneId) -> bool) {
        let gpu = &self.gpu;
        self.compositor
            .prepare(
                &gpu.instance,
                &gpu.device,
                &base(),
                WINDOW,
                panes,
                motion,
                None,
                false,
                &cards(),
                active,
                left,
                BACKDROP,
            )
            .expect("prepare");
    }

    /// The end of a frame, as render/composited.rs ends one after presenting: the commit, which
    /// leaves nothing the frame changed uncommitted. Since the last frame's end nothing was added
    /// to an animation already handed to a property, which DWM wouldn't play (`late`, PITFALLS
    /// 294), and nothing was called that the stand-in doesn't record (`E_NOTIMPL`)
    fn commit(&self) {
        self.compositor.commit_if_changed().expect("commit");
        let pending = self.recorder.pending();
        assert!(pending.is_empty(), "the frame left changes uncommitted: {:?}", pending);
        let wrong: Vec<Call> = self
            .recorder
            .since(self.checked.get())
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    Call::Segment { late: true, .. }
                        | Call::End { late: true, .. }
                        | Call::Unrecorded { .. }
                )
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "the frame added to an animation already handed over, or called what the stand-in doesn't record: {:?}",
            wrong
        );
        self.checked.set(self.recorder.mark());
    }

    /// What the recorder knows one of the compositor's objects as
    fn object(&self, of: &impl Interface) -> Object {
        self.recorder.object(of).expect("an object of the stand-in device's")
    }

    /// The visuals of pane `id`, on screen or fading out
    fn pane(&self, id: PaneId) -> Pane {
        let layers = self.compositor.layers.borrow();
        let pane = layers
            .panes
            .iter()
            .map(|(i, p)| (*i, p))
            .chain(layers.exiting.iter().map(|(i, p, ..)| (*i, p)))
            .find(|(i, _)| *i == id)
            .map(|(_, p)| p)
            .expect("the pane's visuals");
        Pane {
            container: self.object(&pane.container),
            visual: self.object(&pane.layer.visual),
            clip: self.object(&pane.clip),
            bounds: self.object(&pane.bounds),
        }
    }

    /// The group of the active pane's outline, which its offset moves
    fn outline(&self) -> Object {
        let layers = self.compositor.layers.borrow();
        self.object(&layers.focus.as_ref().expect("made with the first frame").group)
    }

    /// The visual every pane's container is in
    fn panes(&self) -> Object {
        self.object(&self.compositor.panes)
    }

    /// What the state file says played under `name` of what plays from `since` on (the
    /// compositor's own bookkeeping, lbterm/STATE.md's `played`), or `None` when nothing plays
    /// under it. Asked at a time from before the frame, so nothing it started has ended yet
    fn played(&self, name: &str, since: Instant) -> Option<Vec<Played>> {
        self.compositor
            .animations(since)
            .into_iter()
            .find(|a| a.name == name)
            .map(|a| a.played)
    }
}

/// `Compositor::new`'s visuals, made in their order under the root and committed: the order is
/// held here (lbterm/COMPOSITOR.md, "How a frame reaches the screen", has the table)
#[test]
fn the_window_s_visuals_are_made_in_their_order() {
    let screen = Screen::new();
    let c = &screen.compositor;
    let tree = screen.recorder.tree();
    let root = tree.root().expect("the target's root");
    assert_eq!(root, screen.object(&c._root));
    let order = [
        ("cards", &c.cards),
        ("under", &c.under),
        ("panes", &c.panes),
        ("focus", &c.focus),
        ("over", &c.over),
        ("over_top", &c.over_top),
        ("sides", &c.sides),
        ("swap", &c.swap),
        ("lights", &c.lights),
        ("hover", &c.hover),
        ("strip_card", &c.strip_card),
        ("objectives", &c.objectives),
        ("readout", &c.readout),
        ("questions_pane", &c.questions_pane),
        ("panel", &c.panel),
        ("plus", &c.plus),
        ("picker", &c.picker),
        ("panel_lights", &c.panel_lights),
        ("tasks_card", &c.tasks_card),
        ("menus", &c.menus),
        ("menu_lights", &c.menu_lights),
        ("top", &c.top),
    ];
    let named = |o: Object| order.iter().find(|(_, v)| screen.object(*v) == o).map_or("?", |(n, _)| *n);
    let children: Vec<&str> = tree.children(root).into_iter().map(named).collect();
    let expected: Vec<&str> = order.iter().map(|(n, _)| *n).collect();
    assert_eq!(children, expected, "bottom to top\n{tree}");
    // The card templates are never seen, only their pieces on the panes: a clip with no size
    let clip = tree.link(screen.object(&c.cards), "Clip").expect("the templates' clip");
    assert_eq!(tree.kind(clip), "clip");
    assert!(tree.props(clip).is_empty(), "{}", tree);
    assert!(screen.recorder.pending().is_empty());
}

/// A split: the pane split narrows to its half on `pane_layout`, and the new pane rides in from
/// its old right edge on `pane_layout`, clipped to where it came out of, fading in on
/// `pane_enter` (lbterm_motion::plan's `Layout` and `Enter`); the outline glides onto the new
/// pane, which has the keyboard, on `focus_move`
#[test]
fn a_split_narrows_the_pane_split_and_rides_the_new_one_in() {
    let screen = Screen::new();
    let motion = settings(true);
    let none = |_: PaneId| false;
    screen.frame(&[(0, WINDOW)], Some(0), &motion, &none);
    let before = Instant::now();
    screen.frame(&[(0, LEFT), (1, RIGHT)], Some(1), &motion, &none);
    let tree = screen.recorder.tree();
    let (split, new) = (screen.pane(0), screen.pane(1));

    // The pane split stays where it was, its clip narrowing to its half
    assert_eq!(tree.value(split.visual, "OffsetX"), Some(0.0), "{tree}");
    assert_eq!(
        tree.curve(split.clip, "Right").and_then(|c| c.segments()),
        Some(on(&motion.layout, W as f64, HALF, PIXEL_EPSILON)),
        "{tree}"
    );
    assert_eq!(tree.value(split.clip, "Bottom"), Some(H as f32), "{tree}");

    // The new pane over it, clipped to the pane it came out of until it lands
    assert_eq!(tree.children(screen.panes()), vec![split.container, new.container], "{tree}");
    assert_eq!(tree.link(new.container, "Clip"), Some(new.bounds), "{tree}");
    for (edge, value) in [("Left", 0.0), ("Top", 0.0), ("Right", W as f32), ("Bottom", H as f32)] {
        assert_eq!(tree.value(new.bounds, edge), Some(value), "{edge}\n{tree}");
    }
    assert_eq!(
        tree.curve(new.visual, "OffsetX").and_then(|c| c.segments()),
        Some(on(&motion.layout, W as f64, HALF, PIXEL_EPSILON)),
        "{tree}"
    );
    assert_eq!(tree.value(new.visual, "OffsetY"), Some(0.0), "{tree}");
    let fade = tree.curve(new.visual, "Opacity").expect("the new pane fading in");
    assert_eq!(fade.segments(), Some(on(&motion.enter, 0.0, 1.0, OPACITY_EPSILON)), "{tree}");

    // The outline glides from the pane split to the new one
    assert_eq!(
        tree.curve(screen.outline(), "OffsetX").and_then(|c| c.segments()),
        Some(on(&motion.focus_move, 0.0, HALF, PIXEL_EPSILON)),
        "{tree}"
    );

    // The state file names them, and its numbers are those DWM was given
    assert_eq!(screen.played("pane.enter", before), Some(vec![fade.played().unwrap()]));
    for name in ["pane.layout", "outline.move"] {
        assert!(screen.played(name, before).is_some(), "{} isn't in flight", name);
    }
}

/// A close: the pane closed goes under the one taking its room and fades out where it was on
/// `pane_exit`, the one left widening on `pane_layout` (lbterm_motion::plan's `Exit` and
/// `Layout`), the outline gliding onto it; once the fade is done the next frame takes the closed
/// pane out of the tree
#[test]
fn a_close_fades_the_pane_out_under_the_one_taking_its_room() {
    let screen = Screen::new();
    let motion = settings(true);
    let none = |_: PaneId| false;
    screen.frame(&[(0, LEFT), (1, RIGHT)], Some(1), &motion, &none);
    let before = Instant::now();
    screen.frame(&[(0, WINDOW)], Some(0), &motion, &none);
    let tree = screen.recorder.tree();
    let (kept, closed) = (screen.pane(0), screen.pane(1));

    assert_eq!(tree.children(screen.panes()), vec![closed.container, kept.container], "{tree}");
    assert_eq!(tree.value(closed.visual, "OffsetX"), Some(HALF as f32), "{tree}");
    let fade = tree.curve(closed.visual, "Opacity").expect("the closed pane fading out");
    assert_eq!(fade.segments(), Some(on(&motion.exit, 1.0, 0.0, OPACITY_EPSILON)), "{tree}");
    assert_eq!(
        tree.curve(kept.clip, "Right").and_then(|c| c.segments()),
        Some(on(&motion.layout, HALF, W as f64, PIXEL_EPSILON)),
        "{tree}"
    );
    assert_eq!(
        tree.curve(screen.outline(), "OffsetX").and_then(|c| c.segments()),
        Some(on(&motion.focus_move, HALF, 0.0, PIXEL_EPSILON)),
        "{tree}"
    );
    assert_eq!(screen.played("pane.exit", before), Some(vec![fade.played().unwrap()]));

    // Put away by the first frame after its fade is done
    let (fading, done) = screen.compositor.fading_panes();
    assert_eq!(fading, vec![1]);
    let done = done.expect("when the fade is done");
    std::thread::sleep(done.saturating_duration_since(Instant::now()) + Duration::from_millis(2));
    screen.frame(&[(0, WINDOW)], Some(0), &motion, &none);
    let tree = screen.recorder.tree();
    assert_eq!(tree.children(screen.panes()), vec![kept.container], "{tree}");
    assert!(!tree.shown(closed.visual), "{}", tree);
    assert!(screen.compositor.fading_panes().0.is_empty());
}

/// A tab switch: the panes of the tab left stay where they were, over the next tab's, which is
/// there whole at once, and fade out on `tab_switch` (lbterm_motion::dissolve's `Dissolve`, the
/// opacity from lbterm_motion::flight), each under the state file's `pane.dissolve`
#[test]
fn a_tab_switch_dissolves_the_tab_left_over_the_next() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.frame(&[(0, LEFT), (1, RIGHT)], Some(0), &motion, &|_: PaneId| false);
    let before = Instant::now();
    // The next tab's one pane; 0 and 1 are in the tab left
    screen.frame(&[(2, WINDOW)], Some(2), &motion, &|id: PaneId| id != 2);
    let tree = screen.recorder.tree();
    let (a, b, next) = (screen.pane(0), screen.pane(1), screen.pane(2));

    assert_eq!(
        tree.children(screen.panes()),
        vec![next.container, a.container, b.container],
        "the tab left over the next\n{tree}"
    );
    assert_eq!(tree.value(next.visual, "Opacity"), Some(1.0), "{tree}");
    assert_eq!(tree.value(b.visual, "OffsetX"), Some(HALF as f32), "{tree}");
    let ends = on(&motion.tab_switch, 1.0, 0.0, OPACITY_EPSILON).end;
    let mut played = vec![];
    for pane in [a, b] {
        let fade = tree.curve(pane.visual, "Opacity").expect("a pane of the tab left fading out");
        let numbers = fade.played().unwrap();
        assert!(close(numbers.from, 1.0) && numbers.to == 0.0, "{:?}\n{}", numbers, tree);
        assert!(close(fade.ends().unwrap(), ends), "ends at {:?}, tab_switch at {ends}", fade.ends());
        played.push(numbers);
    }
    assert_eq!(screen.played("pane.dissolve", before), Some(played));
    assert_eq!(screen.compositor.fading_panes().0, vec![0, 1]);
}

/// The keyboard moving to another pane: the outline glides onto it on `focus_move`, and nothing
/// of the panes themselves moves (lbterm_motion::focus's `Glide`)
#[test]
fn the_outline_glides_to_the_pane_that_takes_the_keyboard() {
    let screen = Screen::new();
    let motion = settings(true);
    let none = |_: PaneId| false;
    screen.frame(&[(0, LEFT), (1, RIGHT)], Some(0), &motion, &none);
    assert_eq!(screen.recorder.tree().value(screen.outline(), "OffsetX"), Some(0.0));
    let mark = screen.recorder.mark();
    let before = Instant::now();
    screen.frame(&[(0, LEFT), (1, RIGHT)], Some(1), &motion, &none);
    let tree = screen.recorder.tree();

    let outline = screen.outline();
    assert_eq!(
        tree.curve(outline, "OffsetX").and_then(|c| c.segments()),
        Some(on(&motion.focus_move, 0.0, HALF, PIXEL_EPSILON)),
        "{tree}"
    );
    assert_eq!(tree.value(outline, "OffsetY"), Some(0.0), "{tree}");
    let (a, b) = (screen.pane(0), screen.pane(1));
    let still = [a.container, a.visual, a.clip, b.container, b.visual, b.clip];
    for handed in screen.recorder.animated(mark) {
        assert!(!still.contains(&Object::Ours(handed.id)), "a pane moved: {:?}\n{}", handed, tree);
    }
    assert!(screen.played("outline.move", before).is_some());
    assert!(screen.played("pane.layout", before).is_none());
}

/// Motion off: a split is there at once, nothing handed to DWM to play and nothing in flight
#[test]
fn with_motion_off_a_split_is_there_at_once() {
    let screen = Screen::new();
    let motion = settings(false);
    let none = |_: PaneId| false;
    screen.frame(&[(0, WINDOW)], Some(0), &motion, &none);
    let mark = screen.recorder.mark();
    let before = Instant::now();
    screen.frame(&[(0, LEFT), (1, RIGHT)], Some(1), &motion, &none);
    let tree = screen.recorder.tree();

    let handed = screen.recorder.animated(mark);
    assert!(handed.is_empty(), "animated with motion off: {:?}", handed);
    assert!(screen.compositor.animations(before).is_empty());
    let (split, new) = (screen.pane(0), screen.pane(1));
    assert_eq!(tree.value(split.clip, "Right"), Some(HALF as f32), "{tree}");
    assert_eq!(tree.value(new.visual, "OffsetX"), Some(HALF as f32), "{tree}");
    assert_eq!(tree.value(new.visual, "Opacity"), Some(1.0), "{tree}");
    assert_eq!(tree.link(new.container, "Clip"), None, "{tree}");
    assert_eq!(tree.children(screen.panes()), vec![split.container, new.container], "{tree}");
    assert_eq!(tree.value(screen.outline(), "OffsetX"), Some(HALF as f32), "{tree}");
}

/// The menus' cards, their flare, page turn and picked row's flash
mod menus;
/// The tab panel's rail: its rows and marker, what it holds under a menu's card, its loops and
/// the Conductor's pad's spectrum
mod rail;
/// Tabs in flight: switched through faster than they fade, and back to the oldest, the panes and
/// the strips' pictures
mod flight;
/// The light running its laps along a suggested tab connection's line
mod connections;

/// The questions pane comes out from under the window's right edge on `panel_open` as a right
/// hold starts: its slider inside a clip that ends at the edge, from the clip's right edge to
/// nothing. Let go mid-slide, it turns round where it is and goes back under the edge on
/// `panel_close`, showing until that's done; put away, nothing of it shows; and with motion off
/// it's out at once (render/questions_pane.rs, lbterm/QUESTIONS.md, "The questions pane")
#[test]
fn the_questions_pane_slides_out_from_under_the_window_s_edge_and_turns_round_where_it_is() {
    let screen = Screen::new();
    let motion = settings(true);
    let none = |_: PaneId| false;
    screen.frame(&[(0, WINDOW)], Some(0), &motion, &none);
    // The card and its shadow, a gutter in from the edge, and what of the window it may show in:
    // up to the edge
    let bounds = PixelRect::new(W as i32 - 92, 30, 90, 200);
    let clip = PixelRect::new(W as i32 - 92, 30, 92, 200);
    let out = 92.0;
    let bar = |open| crate::termwindow::render::questions_pane::BarVisual { bounds, clip, open, travel: out, push: None };
    let prepare = |want: Option<crate::termwindow::render::questions_pane::BarVisual>, motion: &MotionSettings| {
        let gpu = &screen.gpu;
        screen
            .compositor
            .prepare_questions_pane(&gpu.instance, &gpu.device, &base(), want, motion)
            .expect("prepare_questions_pane");
        screen.commit();
    };

    let before = Instant::now();
    prepare(Some(bar(true)), &motion);
    let (container, slider, edges) = {
        let s = screen.compositor.questions_pane_slide.borrow();
        (
            screen.object(s.container.as_ref().expect("its container")),
            screen.object(s.slider.as_ref().expect("its slider")),
            screen.object(s.clip.as_ref().expect("its clip")),
        )
    };
    let tree = screen.recorder.tree();
    assert_eq!(tree.children(screen.object(&screen.compositor.questions_pane)), vec![container], "{tree}");
    assert_eq!(tree.link(container, "Clip"), Some(edges), "{tree}");
    for (edge, value) in [("Left", W as f32 - 92.0), ("Top", 30.0), ("Right", W as f32), ("Bottom", 230.0)] {
        assert_eq!(tree.value(edges, edge), Some(value), "{edge}\n{tree}");
    }
    assert_eq!(
        tree.curve(slider, "OffsetX").and_then(|c| c.segments()),
        Some(on(&motion.panel_open, out, 0.0, PIXEL_EPSILON)),
        "{tree}"
    );
    assert!(screen.played("questions.pane", before).is_some(), "questions.pane isn't in flight");

    // Let go part way out: back from where it is, with its speed, not from either end
    std::thread::sleep(std::time::Duration::from_millis(30));
    prepare(Some(bar(false)), &motion);
    let tree = screen.recorder.tree();
    let back = tree.curve(slider, "OffsetX").and_then(|c| c.segments()).expect("the slide back");
    let from = back.cubics.first().expect("a curve").coefficients[0] as f64;
    assert!(from > 0.5 && from < out - 0.5, "turned round from {}, not where it was\n{}", from, tree);
    assert_eq!(back.end_value, out as f32, "{tree}");
    let (showing, settles) = screen.compositor.questions_pane_showing(Instant::now());
    assert!(showing && settles.is_some(), "it shows until it's back under the edge");

    // Put away: nothing of it shows
    prepare(None, &motion);
    let tree = screen.recorder.tree();
    assert_eq!(tree.value(edges, "Right"), Some(0.0), "{tree}");
    assert_eq!(screen.compositor.questions_pane_showing(Instant::now()), (false, None));

    // Motion off: out at once
    prepare(Some(bar(true)), &settings(false));
    let tree = screen.recorder.tree();
    assert_eq!(tree.value(slider, "OffsetX"), Some(0.0), "{tree}");
    // With no push nothing else moves
    assert_eq!(tree.value(screen.object(&screen.compositor.panes), "OffsetX"), None, "{tree}");
}

/// The questions bar's push (Lachlan's 16 of 2026-11-14): as the bar comes out, everything under
/// the tab bar that isn't over the window (the panes, their outline, `over`, the strips' pictures,
/// the objectives' and the context bar's visuals, the tab panel) moves left as far, on one
/// animation of the bar's curve from the bar's commit, so they stay `travel` apart all the way;
/// `over` is cut at the tab bar's bottom and the band above shows through `over_top`, which
/// doesn't move. Let go mid-slide, all of it comes back from where it is; put away, it's all
/// where it was, still, and `over` is whole again; motion off, pushed at once
#[test]
fn the_questions_bar_pushes_the_rail_and_the_conversation_left_as_far_as_it_comes_out() {
    let screen = Screen::new();
    let motion = settings(true);
    let none = |_: PaneId| false;
    screen.frame(&[(0, WINDOW)], Some(0), &motion, &none);
    // The bar a gutter in from the edge, the rail's strip (a gutter and the rail) its travel
    let travel = 72.0;
    let bounds = PixelRect::new(W as i32 - 73, 30, 70, 560);
    let clip = PixelRect::new(W as i32 - 73, 30, 73, 560);
    let band = 28;
    let bar = |open| crate::termwindow::render::questions_pane::BarVisual { bounds, clip, open, travel, push: Some(band) };
    let prepare = |want: Option<crate::termwindow::render::questions_pane::BarVisual>, motion: &MotionSettings| {
        let gpu = &screen.gpu;
        screen
            .compositor
            .prepare_questions_pane(&gpu.instance, &gpu.device, &base(), want, motion)
            .expect("prepare_questions_pane");
        screen.commit();
    };
    let c = &screen.compositor;
    let pushed = [
        ("panes", &c.panes),
        ("focus", &c.focus),
        ("over", &c.over),
        ("sides", &c.sides),
        ("objectives", &c.objectives),
        ("panel", &c.panel),
    ];

    let before = Instant::now();
    prepare(Some(bar(true)), &motion);
    let slider = screen.object(c.questions_pane_slide.borrow().slider.as_ref().expect("its slider"));
    let tree = screen.recorder.tree();
    for (name, visual) in pushed {
        assert_eq!(tree.curve(screen.object(visual), "OffsetX").and_then(|c| c.segments()), Some(on(&motion.panel_open, 0.0, -travel, PIXEL_EPSILON)), "{name}\n{tree}");
    }
    assert_eq!(
        tree.curve(slider, "OffsetX").and_then(|c| c.segments()),
        Some(on(&motion.panel_open, travel, 0.0, PIXEL_EPSILON)),
        "the bar's own slide\n{tree}"
    );
    assert!(screen.played("questions.push", before).is_some(), "questions.push isn't in flight");
    // `over` below the tab bar moves; the band above it shows through `over_top`, which stays
    let over_clip = tree.link(screen.object(&c.over), "Clip").expect("over's clip");
    for (edge, value) in [("Left", 0.0), ("Top", band as f32), ("Right", W as f32), ("Bottom", H as f32)] {
        assert_eq!(tree.value(over_clip, edge), Some(value), "over's {edge}\n{tree}");
    }
    let top = screen.object(&c.over_top);
    let top_clip = tree.link(top, "Clip").expect("over_top's clip");
    for (edge, value) in [("Left", 0.0), ("Top", 0.0), ("Right", W as f32), ("Bottom", band as f32)] {
        assert_eq!(tree.value(top_clip, edge), Some(value), "over_top's {edge}\n{tree}");
    }
    assert!(tree.link(top, "Content").is_some(), "over_top shows over's swapchain\n{}", tree);
    assert_eq!(tree.curve(top, "OffsetX"), None, "the tab bar stays\n{tree}");
    for (name, visual) in [("swap", &c.swap), ("hover", &c.hover), ("menus", &c.menus), ("lights", &c.lights)] {
        assert_eq!(tree.curve(screen.object(visual), "OffsetX"), None, "{name} isn't pushed\n{tree}");
    }

    // Let go part way: all of it back from where it is, still `travel` from the bar
    std::thread::sleep(std::time::Duration::from_millis(30));
    prepare(Some(bar(false)), &motion);
    let tree = screen.recorder.tree();
    let back = tree.curve(screen.object(&c.panes), "OffsetX").and_then(|c| c.segments()).expect("the push back");
    let slide = tree.curve(slider, "OffsetX").and_then(|c| c.segments()).expect("the bar's slide back");
    let from = back.cubics.first().expect("a curve").coefficients[0] as f64;
    assert!(from < -0.5 && from > -travel + 0.5, "turned round from {}, not where it was\n{}", from, tree);
    assert_eq!(back.end_value, 0.0, "{tree}");
    let bar_from = slide.cubics.first().expect("a curve").coefficients[0] as f64;
    assert!((bar_from - from - travel).abs() < 1e-3, "the bar at {}, the push at {}: not {} apart", bar_from, from, travel);
    for (name, visual) in pushed {
        assert_eq!(tree.curve(screen.object(visual), "OffsetX").and_then(|c| c.segments()), tree.curve(screen.object(&c.panes), "OffsetX").and_then(|c| c.segments()), "{name}\n{tree}");
    }

    // Put away: everything where it was, still, `over` whole and `over_top` showing nothing
    prepare(None, &motion);
    let tree = screen.recorder.tree();
    for (name, visual) in pushed {
        assert_eq!(tree.value(screen.object(visual), "OffsetX"), Some(0.0), "{name}\n{tree}");
    }
    assert_eq!(tree.link(screen.object(&c.over), "Clip"), None, "{tree}");
    assert_eq!(tree.link(top, "Clip"), None, "{tree}");
    assert_eq!(tree.link(top, "Content"), None, "{tree}");

    // Motion off: pushed at once
    prepare(Some(bar(true)), &settings(false));
    let tree = screen.recorder.tree();
    for (name, visual) in pushed {
        assert_eq!(tree.value(screen.object(visual), "OffsetX"), Some(-travel as f32), "{name}\n{tree}");
    }
}

/// The context bar's slides: the thin numbers' pane coming out from under the widened bar's
/// moving edge in its slide, and going back
mod context_bar;
