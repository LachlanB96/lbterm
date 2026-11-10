//! The menus' cards over the stand-in device (lbterm/COMPOSITOR.md, "The compositor's tests"):
//! `prepare_menus` and `prepare_lights` driven as a paint drives them for one slot's card, and
//! what they built read back. Each is a rule a perf script judged in a test window before
//! (.claude/docs/perf-scripts.md says which): the card's fade in and out under its slot's name,
//! the edge's flare a key no row takes plays (`MenuLamp`, menu-keys.ps1's step 16), a page turn's
//! swap (`menu_turn`) and a picked row's flash (`Light::Row`), which outlives the card's fade.

use super::*;
use lbterm_motion::menu;

/// The tab menu's slot (render/menu_fade.rs, `Slot::Tab`): its names are `menu.tab...`
const TAB: usize = 0;
/// Where the card sits in the window
const CARD: PixelRect = PixelRect { x: 100, y: 80, w: 240, h: 160 };
/// A row of it, where a pick flashes
const ROW: PixelRect = PixelRect { x: 108, y: 120, w: 224, h: 24 };

/// The card's hairline, its outer edge 8 pixels in from the card's surface and its fill a pixel
/// inside that (render/menu_fade.rs, `lbterm_menu_card`, gives them so)
fn ring() -> MenuRing {
    let inset = |by: f64| lbterm_motion::Rect {
        x: CARD.x as f64 + by,
        y: CARD.y as f64 + by,
        w: CARD.w as f64 - 2.0 * by,
        h: CARD.h as f64 - 2.0 * by,
    };
    MenuRing {
        outer: inset(8.0),
        outer_radius: 10.0,
        inner: inset(9.0),
        inner_radius: 9.0,
    }
}

/// The tab menu's card this frame: open or fading out, the keys that had nowhere to go and the
/// page turns so far, nothing lit
fn card(open: bool, bumps: u64, page: u64) -> MenuVisual {
    MenuVisual {
        rect: CARD,
        open,
        light: None,
        radius: 6.0,
        level: 0.0,
        ring: ring(),
        bumps,
        page,
    }
}

/// The slot's visuals as the recorder knows them
struct Card {
    /// The group whose opacity is the card's fade
    group: Object,
    /// The card's surface's visual (the front one, after a page turn) and its highlight's
    card: Object,
    light: Object,
}

impl Screen {
    /// One frame of the menus, the tab menu's card as `tab` and no other, then the commit
    fn menus(&self, tab: Option<MenuVisual>, motion: &MotionSettings) {
        let gpu = &self.gpu;
        self.compositor
            .prepare_menus(&gpu.instance, &gpu.device, &base(), [tab, None, None, None], motion)
            .expect("prepare_menus");
        self.commit();
    }

    /// One frame of the lights over the window (render/lights.rs), then the commit
    fn lights(&self, lights: Vec<Light>, motion: &MotionSettings) {
        let gpu = &self.gpu;
        self.compositor
            .prepare_lights(&gpu.instance, &gpu.device, &base(), WINDOW, lights, motion)
            .expect("prepare_lights");
        self.commit();
    }

    /// The tab menu's card's visuals, made by its first frame
    fn card(&self) -> Card {
        let cards = self.compositor.menu_cards.borrow();
        let card = &cards[TAB];
        Card {
            group: self.object(card.group.as_ref().expect("the card's group")),
            card: self.object(&card.card.as_ref().expect("the card's surface").visual),
            light: self.object(&card.light.as_ref().expect("the highlight's surface").visual),
        }
    }

    /// The card's other surface's visual, made by its first page turn
    fn ghost(&self) -> Option<Object> {
        let cards = self.compositor.menu_cards.borrow();
        cards[TAB].ghost.as_ref().map(|ghost| self.object(&ghost.visual))
    }

    /// The card's lamp's two visuals that go into its group for a flare: the hairline's group
    /// and the light round the card, made by the first key with nowhere to go
    fn lamp(&self) -> Option<(Object, Object)> {
        let cards = self.compositor.menu_cards.borrow();
        cards[TAB].lamp.as_ref().map(|lamp| (self.object(&lamp.opacity), self.object(&lamp.halo)))
    }

    /// The visual the menus' cards are in, and the one their picked rows flash in, over them
    fn menus_visual(&self) -> Object {
        self.object(&self.compositor.menus)
    }
    fn menu_lights(&self) -> Object {
        self.object(&self.compositor.menu_lights)
    }
}

/// Sleeps `seconds` and a little more, so whatever started before now and lasts that long has
/// ended. From now, not from a time before the frame: the frame that starts it makes surfaces
/// on WARP first, which can take longer than the animation itself
fn sleep_past(seconds: f64) {
    std::thread::sleep(Duration::from_secs_f64(seconds) + Duration::from_millis(5));
}

/// A menu's card: its group made in the menus' visual with the card and its highlight in it, the
/// group fading in from nothing on `pane_enter` as it opens and out on `pane_exit` as it closes,
/// each under the slot's name (`menu.tab`, the state file's), and put away unseen once the slot
/// has no card (render/menu_fade.rs gives none once its fade is done)
#[test]
fn a_menu_s_card_fades_in_and_out_under_its_slot_s_name() {
    let screen = Screen::new();
    let motion = settings(true);
    let before = Instant::now();
    screen.menus(Some(card(true, 0, 0)), &motion);
    let tree = screen.recorder.tree();
    let c = screen.card();

    assert_eq!(tree.children(screen.menus_visual()), vec![c.group], "{tree}");
    assert_eq!(tree.children(c.group), vec![c.card, c.light], "the card under its highlight\n{tree}");
    let fade_in = tree.curve(c.group, "Opacity").expect("the card fading in").clone();
    assert_eq!(fade_in.segments(), Some(on(&motion.enter, 0.0, 1.0, OPACITY_EPSILON)), "{tree}");
    assert_eq!(screen.played("menu.tab", before), Some(vec![fade_in.played().unwrap()]));
    // Nothing lit: the highlight unseen
    assert_eq!(tree.value(c.light, "Opacity"), Some(0.0), "{tree}");

    // Open again, nothing changed: nothing more is handed to DWM
    sleep_past(fade_in.ends().unwrap());
    let mark = screen.recorder.mark();
    screen.menus(Some(card(true, 0, 0)), &motion);
    assert!(screen.recorder.animated(mark).is_empty(), "an open card at rest played something");

    // Closing: from all of it to nothing on pane_exit
    let closing = Instant::now();
    screen.menus(Some(card(false, 0, 0)), &motion);
    let tree = screen.recorder.tree();
    let fade_out = tree.curve(c.group, "Opacity").expect("the card fading out").clone();
    assert_eq!(fade_out.segments(), Some(on(&motion.exit, 1.0, 0.0, OPACITY_EPSILON)), "{tree}");
    assert_eq!(screen.played("menu.tab", closing), Some(vec![fade_out.played().unwrap()]));

    // Gone: unseen, its group kept for the slot's next menu
    sleep_past(fade_out.ends().unwrap());
    screen.menus(None, &motion);
    let tree = screen.recorder.tree();
    assert_eq!(tree.value(c.group, "Opacity"), Some(0.0), "{tree}");
    assert!(tree.shown(c.group), "{}", tree);
    assert!(!screen.compositor.menu_cards.borrow()[TAB].wanted);
}

/// With motion off a card is there at once and gone at once: nothing handed to DWM
#[test]
fn with_motion_off_a_menu_s_card_is_there_at_once() {
    let screen = Screen::new();
    let motion = settings(false);
    let mark = screen.recorder.mark();
    let before = Instant::now();
    screen.menus(Some(card(true, 0, 0)), &motion);
    let c = screen.card();
    assert_eq!(screen.recorder.tree().value(c.group, "Opacity"), Some(1.0));
    screen.menus(Some(card(false, 1, 0)), &motion);
    assert_eq!(screen.recorder.tree().value(c.group, "Opacity"), Some(0.0));
    assert!(screen.recorder.animated(mark).is_empty(), "animated with motion off");
    assert!(screen.compositor.animations(before).is_empty());
    assert!(screen.lamp().is_none(), "a key with nowhere to go made a lamp with motion off");
}

/// A key no row takes flares the card's edge (Lachlan's 9a of 2026-11-08; menu-keys.ps1's step
/// 16 judged it in a window until feat/dcomp-recorder): the lamp's two visuals, the hairline's
/// group just over the card and the light just under it, go into the card's group as the flare
/// starts, both on one chain up to `MENU_BUMP_HAIRLINE` on `arrive_in` and back to nothing on
/// `panel_glow` (lbterm_motion::menu::bump), under `menu.tab.hairline`; and they're out of the
/// tree again by the first frame after its end, so a menu at rest has nothing of the lamp for DWM
/// to compose. A card just opened has no key to answer
#[test]
fn a_key_with_nowhere_to_go_flares_the_card_s_edge_and_leaves_nothing_after() {
    let screen = Screen::new();
    let motion = settings(true);
    // A count of keys already up as the card opens: nothing to answer
    screen.menus(Some(card(true, 3, 0)), &motion);
    let c = screen.card();
    assert!(screen.lamp().is_none(), "a card just opened flared");
    screen.menus(Some(card(true, 3, 0)), &motion);
    assert!(screen.lamp().is_none());
    assert_eq!(screen.recorder.tree().children(c.group), vec![c.card, c.light]);

    let before = Instant::now();
    screen.menus(Some(card(true, 4, 0)), &motion);
    let tree = screen.recorder.tree();
    let (hairline, halo) = screen.lamp().expect("the lamp, made by the key");
    assert_eq!(
        tree.children(c.group),
        vec![halo, c.card, hairline, c.light],
        "the light under the card, the hairline over it and under the highlight\n{tree}"
    );
    assert_eq!(tree.children(hairline).len(), 2, "the white and the card's fill over it\n{tree}");
    let peak = lbterm_panel::palette::MENU_BUMP_HAIRLINE;
    let chain = menu::bump(menu::Now::at(0.0), true, peak, &motion.arrive_in, &motion.panel_glow).expect("a flare");
    let flare = tree.curve(hairline, "Opacity").expect("the hairline flaring").clone();
    assert_eq!(flare.segments(), Some(chain.segments(STEP, OPACITY_EPSILON)), "{tree}");
    assert_eq!(tree.curve(halo, "Opacity"), Some(&flare), "one chain on both\n{tree}");
    // Up to the hairline's full light and back to none: what menu-keys.ps1 read from the state
    // file's played numbers
    let played = flare.played().unwrap();
    assert!(played.from == 0.0 && played.to == 0.0, "{:?}", played);
    assert!((played.high - peak).abs() < 1e-3, "{:?}", played);
    assert_eq!(screen.played("menu.tab.hairline", before), Some(vec![played]));

    // While it plays a frame leaves it; the first after its end takes both out
    screen.menus(Some(card(true, 4, 0)), &motion);
    assert!(screen.recorder.tree().shown(hairline));
    sleep_past(flare.ends().unwrap());
    screen.menus(Some(card(true, 4, 0)), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(tree.children(c.group), vec![c.card, c.light], "{tree}");
    assert!(!tree.shown(hairline) && !tree.shown(halo), "{}", tree);

    // Another key puts them back for its flare, the same visuals
    screen.menus(Some(card(true, 5, 0)), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(screen.lamp(), Some((hairline, halo)));
    assert_eq!(tree.children(c.group), vec![halo, c.card, hairline, c.light], "{tree}");
}

/// A page turn (the tab menu's Wait on… and back, lbterm/NEW-TAB.md, section 10): the card's two
/// surfaces swap round, the one holding the old page going over the new one, just above it and
/// under the highlight, and fading out on `swap` under `menu.tab.swap`, the new page whole at once
/// under it; the next turn swaps them back with no surface made
#[test]
fn a_page_turn_dissolves_the_old_page_over_the_new() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.menus(Some(card(true, 0, 0)), &motion);
    screen.menus(Some(card(true, 0, 0)), &motion);
    let first = screen.card();
    assert_eq!(screen.ghost(), None, "a surface for a page before any turn");

    let before = Instant::now();
    screen.menus(Some(card(true, 0, 1)), &motion);
    let tree = screen.recorder.tree();
    let turned = screen.card();
    let ghost = screen.ghost().expect("the page before");
    assert_eq!(ghost, first.card, "the old page is the surface it was drawn in");
    assert_ne!(turned.card, first.card);
    assert_eq!(tree.children(first.group), vec![turned.card, ghost, first.light], "{tree}");
    assert_eq!(tree.value(turned.card, "Opacity"), Some(1.0), "{tree}");
    let swap = tree.curve(ghost, "Opacity").expect("the old page fading").clone();
    assert_eq!(swap.segments(), Some(on(&motion.swap, 1.0, 0.0, OPACITY_EPSILON)), "{tree}");
    assert_eq!(screen.played("menu.tab.swap", before), Some(vec![swap.played().unwrap()]));

    // And back: the two swap round again
    sleep_past(swap.ends().unwrap());
    screen.menus(Some(card(true, 0, 2)), &motion);
    let tree = screen.recorder.tree();
    assert_eq!(screen.card().card, first.card);
    assert_eq!(screen.ghost(), Some(turned.card));
    assert_eq!(tree.children(first.group), vec![first.card, turned.card, first.light], "{tree}");
    assert_eq!(tree.value(first.card, "Opacity"), Some(1.0), "{tree}");
    assert!(tree.curve(turned.card, "Opacity").is_some(), "{}", tree);

    // With motion off the new page is just there. The first screen goes first: a screen holds
    // the test's turn with the WARP device until it's dropped (PITFALLS 303)
    drop(screen);
    let screen = Screen::new();
    let off = settings(false);
    screen.menus(Some(card(true, 0, 0)), &off);
    let mark = screen.recorder.mark();
    screen.menus(Some(card(true, 0, 1)), &off);
    assert_eq!(screen.ghost(), None);
    assert!(screen.recorder.animated(mark).is_empty());
}

/// A row picked flashes white as the card fades out (Lachlan's 9b; `Light::Row`): its light is in
/// `menu_lights`, over every card and not in the card's group, so the card's fade doesn't take
/// it; up to the press's white on `copy_in` and back on `copy_out` (lbterm_motion::menu::flash),
/// under the slot's `menu.tab.flash`; still lit once the card has gone, and out of the tree by the
/// first frame of lights after its end
#[test]
fn a_picked_row_flashes_over_the_card_and_outlives_its_fade() {
    let screen = Screen::new();
    let motion = settings(true);
    screen.menus(Some(card(true, 0, 0)), &motion);
    let c = screen.card();
    sleep_past(on(&motion.enter, 0.0, 1.0, OPACITY_EPSILON).end);

    // The pick: the card fades out, its row flashes, in one paint
    let before = Instant::now();
    screen.menus(Some(card(false, 0, 0)), &motion);
    screen.lights(vec![Light::Row { rect: ROW, radius: 4.0, slot: TAB }], &motion);
    let tree = screen.recorder.tree();
    let lit = tree.children(screen.menu_lights());
    assert_eq!(lit.len(), 1, "one flash\n{tree}");
    let flash = lit[0];
    assert_eq!(tree.parent(flash), Some(screen.menu_lights()));
    assert!(!tree.children(c.group).contains(&flash), "{}", tree);
    assert_eq!((tree.value(flash, "OffsetX"), tree.value(flash, "OffsetY")), (Some(ROW.x as f32), Some(ROW.y as f32)), "{tree}");
    let chain = menu::flash(true, lbterm_panel::palette::PRESS, &motion.copy_in, &motion.copy_out).expect("a flash");
    let curve = tree.curve(flash, "Opacity").expect("the flash").clone();
    assert_eq!(curve.segments(), Some(chain.segments(STEP, OPACITY_EPSILON)), "{tree}");
    assert_eq!(screen.played("menu.tab.flash", before), Some(vec![curve.played().unwrap()]));
    let fade = tree.curve(c.group, "Opacity").expect("the card fading out").clone();
    assert!(
        curve.ends().unwrap() > fade.ends().unwrap(),
        "the flash ({:?} s) ends before the card's fade ({:?} s)",
        curve.ends(),
        fade.ends()
    );

    // The card gone, the flash still lit over where it was
    sleep_past(fade.ends().unwrap());
    screen.menus(None, &motion);
    screen.lights(vec![], &motion);
    let tree = screen.recorder.tree();
    assert_eq!(tree.value(c.group, "Opacity"), Some(0.0), "{tree}");
    assert!(tree.shown(flash), "the flash went with the card\n{}", tree);

    sleep_past(curve.ends().unwrap());
    screen.lights(vec![], &motion);
    let tree = screen.recorder.tree();
    assert!(tree.children(screen.menu_lights()).is_empty(), "{}", tree);
}
