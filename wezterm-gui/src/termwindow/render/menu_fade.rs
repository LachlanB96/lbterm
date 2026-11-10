//! LBTerm: every menu's fade and its highlight's glide, built once (lbterm/NEW-TAB.md, section 6;
//! lbterm/DESIGN.md, "Menus"). The tab menu, the prompt list, the Conductor's menu, and the
//! InMail list, the agenda, the context pill's list and the + menu (which share z-layers 77 to
//! 79, only one of them ever open) draw their light and card with [`TermWindow::lbterm_menu_card`] and their lit row with
//! [`TermWindow::lbterm_menu_light`]; the rules are `lbterm_motion::menu`'s and DWM plays them
//! (the compositor's `prepare_menus`).
//!
//! - Each menu is in a slot ([`Slot`]) whose card is a visual of its own over the tab panel: a
//!   group faded in on `pane_enter` as the menu opens and out on `pane_exit` as it closes, from
//!   wherever it is, holding the card (its three z-layers: glow, card, words) and over it a white
//!   surface seen through a rounded clip as its highlight. Over the words, so the lit row's words
//!   go 9.6% of the way to white with it: a few levels on the menus' white words. Words on a
//!   surface of their own under the highlight were tried first and changed every word's edges
//!   (lbterm/PITFALLS.md, 147).
//! - A paint starts with no menu drawn ([`TermWindow::lbterm_menu_fade_begin`], from
//!   render/paint.rs's `paint_pass`), and a menu drawn in it is open. A slot drawn in the paint
//!   before and not in this one is closing: its visual is left as its last frame drew it, fading
//!   out, and nothing is drawn, placed or moved in it again ([`TermWindow::lbterm_menu_visuals`],
//!   asked once a frame by render/composited.rs). The menu itself closed at once, so a card fading
//!   out takes no click or key. Nothing wakes for the fade's end: DWM ends it, and the slot is put
//!   away by the next paint for anything else.
//! - The highlight's row is handed over rather than drawn ([`TermWindow::lbterm_menu_light`]): its
//!   clip glides from row to row on `hover`, from wherever it is, and its light comes up and goes
//!   out on `hover`, at the share of white the drawn one is.
//! - Without the compositor the card and the highlight are drawn on the card, at once, as before.
//! - The light round every card is one falloff (`palette::menu_glow_alpha`), each pixel drawn
//!   once, as the panel's light is (lbterm/TAB-PANEL.md, "The light"): the four stacked rings it
//!   replaced banded at 2x.
//! - A key no row takes flares the card's edge once (`lbterm_motion::menu::bump`: up on
//!   `arrive_in`, down on `panel_glow`; Lachlan's 9a of 2026-11-08), the hairline going white and
//!   the light round the card white under its red ([`TermWindow::lbterm_menu_bump`]): the slot's
//!   count of such keys goes to the compositor with its card, whose lamp (`menu_lamp`) plays it on
//!   the card's own surfaces, put in the tree only while it plays. Without the compositor, or with
//!   motion off, there's none of it, as for the other brief lights (render/lights.rs). Over the +
//!   menu a key a row shows beside it as its shortcut picks that row instead (his 5a;
//!   [`TermWindow::lbterm_menus_binding`]).

use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::compositor::{MenuRing, MenuVisual, MENU_SLOTS};
use crate::termwindow::keyevent::Key;
use crate::termwindow::render::tab_panel::light_bands;
use crate::termwindow::TermWindow;
use ::window::WindowOps;
use lbterm_extras::menu;
use lbterm_extras::strip;
use lbterm_extras::tabs::Rect;
use lbterm_motion::{PixelRect, Target};
use lbterm_panel::{palette, Area};
use mux::pane::PaneId;
use std::time::Instant;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

/// Which visual a menu is drawn in: each has z-layers of its own, and a menu that opens closes
/// any other in its slot
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// The tab menu (render/tab_menu.rs)
    Tab = 0,
    /// The list of prompts (render/prompt.rs)
    Prompt = 1,
    /// The Conductor's menu (render/conductor.rs)
    Conductor = 2,
    /// The InMail button's list (render/inbox.rs), the agenda (render/agenda.rs), the context
    /// pill's list (render/context.rs) and the + menu (render/new_tab.rs)
    List = 3,
}

/// Each slot's z-layers: its glow, its card, its words
const ZS: [(i8, i8, i8); MENU_SLOTS] = [(60, 61, 62), (63, 64, 65), (66, 67, 68), (77, 78, 79)];

impl Slot {
    pub const fn glow_z(self) -> i8 {
        ZS[self as usize].0
    }

    pub const fn card_z(self) -> i8 {
        ZS[self as usize].1
    }

    pub const fn text_z(self) -> i8 {
        ZS[self as usize].2
    }
}

/// The visual that draws z-layer `zindex`, when it's one of the menus': its slot's card
pub fn target_of(zindex: i8) -> Option<Target<PaneId>> {
    ZS.iter()
        .position(|&(glow, card, text)| zindex == glow || zindex == card || zindex == text)
        .map(Target::MenuCard)
}

/// The lit row: white at this share over the card, as the row drawn on it has always been
pub const LIGHT_LEVEL: f64 = palette::HOVER_ALPHA * 1.6;
/// Its corners, at 96 DPI
const LIGHT_RADIUS: f64 = 6.0;
/// The light round a card: its colour blends between this many knots out from the card, and each
/// corner is a fan of this many quads (the panel's light's, render/tab_panel.rs)
const GLOW_KNOTS: usize = 10;
const GLOW_ARC: usize = 12;

#[derive(Debug, Default, Clone, Copy)]
struct SlotPaint {
    /// This paint: the card's visual and its hairline, and the row it lights
    drawn: Option<(PixelRect, MenuRing)>,
    light: Option<Rect>,
    /// Where the compositor last had it, open or fading out
    shown: Option<(PixelRect, MenuRing)>,
    /// Keys that had nowhere to go in this slot's menus, ever: each one more brightens the
    /// hairline once
    bumps: u64,
    /// The times this slot's menu turned its page (the tab menu's Wait on… and back): each one
    /// more dissolves the card's rows into the new page's
    pages: u64,
}

/// What a menu's card is drawn over and in: the window's background, and the menus' fill over it
/// (#262634 at 55%, `palette::menu_fill`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuFill {
    pub surface: u32,
    pub card: u32,
}

/// A key as the menus take it ([`menu::Key`]): a character with no Ctrl, Alt or Windows key, or
/// one of the keys they move or pick with; the rest go nowhere, except a modifier on its own
pub fn menu_key(key: &KeyCode, mods: KeyModifiers) -> menu::Key {
    let plain = !mods.intersects(KeyModifiers::CTRL | KeyModifiers::ALT | KeyModifiers::SUPER);
    match key {
        k if k.is_modifier() => menu::Key::Modifier,
        KeyCode::Char(c) if plain && !c.is_control() => menu::Key::Char(*c),
        KeyCode::Enter => menu::Key::Enter,
        KeyCode::Escape => menu::Key::Escape,
        KeyCode::UpArrow => menu::Key::Up,
        KeyCode::DownArrow => menu::Key::Down,
        KeyCode::LeftArrow => menu::Key::Left,
        KeyCode::RightArrow => menu::Key::Right,
        KeyCode::Delete => menu::Key::Delete,
        _ => menu::Key::Other,
    }
}

/// What each slot's menu drew this paint
#[derive(Debug, Default)]
pub struct MenuFades {
    slots: [SlotPaint; MENU_SLOTS],
}

impl MenuFades {
    /// A new paint: nothing drawn yet
    fn begin(&mut self) {
        for slot in self.slots.iter_mut() {
            slot.drawn = None;
            slot.light = None;
        }
    }

    /// Each slot's visual this frame: open where a menu drew it; else, while `showing` says it
    /// still shows something, fading out where it last was; else none
    fn visuals(&mut self, showing: impl Fn(usize) -> bool, radius: f64) -> [Option<MenuVisual>; MENU_SLOTS] {
        std::array::from_fn(|i| {
            let slot = &mut self.slots[i];
            let light = slot.light.map(|r| lbterm_motion::Rect::new(r.x, r.y, r.w, r.h));
            let (bumps, page) = (slot.bumps, slot.pages);
            match (slot.drawn, slot.shown) {
                (Some((rect, ring)), _) => {
                    slot.shown = Some((rect, ring));
                    Some(MenuVisual { rect, open: true, light, radius, level: LIGHT_LEVEL, ring, bumps, page })
                }
                (None, Some((rect, ring))) if showing(i) => {
                    Some(MenuVisual { rect, open: false, light: None, radius, level: LIGHT_LEVEL, ring, bumps, page })
                }
                (None, _) => {
                    slot.shown = None;
                    None
                }
            }
        })
    }
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect { x: r.x - by, y: r.y - by, w: r.w + 2.0 * by, h: r.h + 2.0 * by }
}

fn motion_rect(r: Rect) -> lbterm_motion::Rect {
    lbterm_motion::Rect::new(r.x, r.y, r.w, r.h)
}

impl TermWindow {
    /// A paint starts: no menu drawn yet (render/paint.rs)
    pub(crate) fn lbterm_menu_fade_begin(&self) {
        self.lbterm_menu.fades.borrow_mut().begin();
    }

    /// The colours every menu's card is drawn in: the window's background under it, and the
    /// menus' fill over that
    pub(crate) fn lbterm_menu_fill(&mut self) -> MenuFill {
        let (r, g, b, _) = self.palette().background.to_srgb_u8();
        let surface = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        MenuFill { surface, card: palette::menu_fill(surface) }
    }

    /// A menu's light round it and its card: the light in its slot's glow z-layer, one falloff
    /// drawn a pixel once; the ring (the panel's red) and the card's fill in its card z-layer; with
    /// the compositor its visual is handed over for this frame, open, with its hairline
    pub(crate) fn lbterm_menu_card(&self, slot: Slot, card: Rect, fill: MenuFill) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        // The light is made only when the card's place or size, the background under it or the
        // window's own key change, and kept in its z-layer otherwise (render/kept.rs): it was
        // made again every paint the menu was in, about 520 quads and the bands' colours, under
        // full-speed output in the pane below too. The glow layer is the slot's alone
        let name = match slot {
            Slot::Tab => "menu-glow-tab",
            Slot::Prompt => "menu-glow-prompt",
            Slot::Conductor => "menu-glow-conductor",
            Slot::List => "menu-glow-list",
        };
        self.lbterm_kept_layer(slot.glow_z(), name, self.lbterm_menu_glow_key(card, fill.surface), |quads| {
            self.lbterm_menu_glow(quads, card, fill.surface)
        })?;
        {
            let layer = gl_state.layer_for_zindex(slot.card_z())?;
            let mut quads = layer.quad_allocator();
            self.lbterm_menu_rounded(&mut quads, grow(card, stroke), radius + stroke, srgb(palette::RING_MIDDLE, 1.0))?;
            self.lbterm_menu_rounded(&mut quads, card, radius, srgb(fill.card, 1.0))?;
        }
        if self.lbterm_composited() {
            let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
            let ring = MenuRing {
                outer: motion_rect(grow(card, stroke)),
                outer_radius: radius + stroke,
                inner: motion_rect(card),
                inner_radius: radius,
            };
            self.lbterm_menu.fades.borrow_mut().slots[slot as usize].drawn = strip::visual_bounds(&card, scale, window)
                .map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), ring));
        }
        Ok(())
    }

    /// What a menu's light is made from, for its kept layer (render/kept.rs): the card's place and
    /// size, the background under it, and the window's own key (its size, DPI, shape and where the
    /// white is in the atlas), which carries the corners' radius
    pub(crate) fn lbterm_menu_glow_key(&self, card: Rect, surface: u32) -> u64 {
        self.lbterm_template_key(&[card.x.to_bits(), card.y.to_bits(), card.w.to_bits(), card.h.to_bits(), surface as u64])
    }

    /// The light round a menu's card (and the picker's, render/new_tab.rs), in `quads`: one
    /// falloff out to `palette::MENU_GLOW_REACH`, as strong at the card's edge as the four rings
    /// it replaced, each pixel drawn once in bands whose colour carries the falloff
    /// (`light_bands`, lbterm/TAB-PANEL.md, "The light"): stacked rings banded at 2x. About 520
    /// quads (10 bands, each four sides and four fans of 12), drawn whenever the card is
    pub(crate) fn lbterm_menu_glow(&self, quads: &mut TripleLayerQuadAllocator, card: Rect, surface: u32) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let radius = self.lbterm_shape().radius() as f64;
        let reach = (palette::MENU_GLOW_REACH * scale) as f32;
        let bands = light_bands(palette::GLOW, reach, GLOW_KNOTS, |d| palette::menu_glow_alpha(d, reach as f64, surface));
        let outline = Area::new(card.x as f32, card.y as f32, card.w as f32, card.h as f32);
        // The card's corners as lbterm_menu_rounded draws them
        let r = radius.min(card.w / 2.0).min(card.h / 2.0).floor() as f32;
        self.tab_panel_bands(quads, outline, r, &bands, GLOW_ARC)
    }

    /// A key no row of the menu in `slot` takes: its card's edge flares once (`menu::bump`:
    /// `arrive_in` up, `panel_glow` down), from the next frame (the compositor's `menu_lamp`). Without the compositor there's no light, as for every brief
    /// light (render/lights.rs)
    pub(crate) fn lbterm_menu_bump(&self, slot: Slot) {
        if !self.lbterm_composited() {
            return;
        }
        self.lbterm_menu.fades.borrow_mut().slots[slot as usize].bumps += 1;
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A row of the menu in `slot` picked, at `row` in the window (Lachlan's 9b of 2026-11-07): it
    /// flashes white as the picker's rows do, up to the press's white on `copy_in` and back on
    /// `copy_out`, from the next frame, then the pick goes as it would. Called before the pick
    /// closes the menu or turns its page, while its layout still has the row. The flash is a light
    /// of its own over every menu's card (the compositor's `Light::Row`, in `menu_lights`), not in
    /// the card's group: the card fades out on `pane_exit` in 120 ms, gone before `copy_in`'s 180
    /// ms would have lit it. Without the compositor, or with motion off, there's none, as for
    /// every brief light (render/lights.rs)
    pub(crate) fn lbterm_menu_flash(&self, slot: Slot, row: Rect) {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let (x0, y0) = (row.x.floor(), row.y.floor());
        let (x1, y1) = ((row.x + row.w).ceil(), (row.y + row.h).ceil());
        let rect = PixelRect::new(x0 as i32, y0 as i32, (x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32);
        self.lbterm_light(crate::termwindow::compositor::Light::Row { rect, radius: LIGHT_RADIUS * scale, slot: slot as usize });
    }

    /// The menu in `slot` turned its page (the tab menu's Wait on… and ‹ Back): from the next
    /// frame its card's rows dissolve into the new page's on `swap` (the compositor's
    /// `prepare_menu`). Without the compositor, or with motion off, the new page is just there
    pub(crate) fn lbterm_menu_turn(&self, slot: Slot) {
        self.lbterm_menu.fades.borrow_mut().slots[slot as usize].pages += 1;
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// A key a list menu (the prompt list, the Conductor's, the InMail list, the agenda, the
    /// context list) has no
    /// use for: its card's edge flares once (`menu::bump`), unless it's a modifier on its own. Never the pane's
    pub(crate) fn lbterm_menu_stray(&self, slot: Slot, key: &KeyCode, mods: KeyModifiers) {
        if menu_key(key, mods) != menu::Key::Modifier {
            self.lbterm_menu_bump(slot);
        }
    }

    /// Whether a menu is open that takes every key (Lachlan's 7b of 2026-11-07): the tab menu, the
    /// prompt list, the Conductor's menu, the InMail list, the agenda, the context pill's list, the
    /// new-tab picker or its + menu. While one is, keyevent.rs acts on no binding (its raw passes
    /// and key_event_impl's ask this before the leader and the bindings) and gives each key to the
    /// menu ([`TermWindow::lbterm_menus_key`]): a bound key it has no use for flares the card's
    /// edge once (`menu::bump`), as any key no row takes does, and does nothing else. Two
    /// exceptions are asked first, by [`TermWindow::lbterm_menus_binding`], only while the picker
    /// or the + menu has the keys: a key the + menu shows beside a row as that row's shortcut picks
    /// it (Lachlan's 5a of 2026-11-08), and a paste key over the picker pastes into its box (his
    /// 11a; lbterm/PITFALLS.md, 150 and 185). A menu
    /// fading out is closed already (its state went at once), so the bindings act again from the
    /// moment it closes
    pub(crate) fn lbterm_menu_takes_keys(&self) -> bool {
        self.lbterm_menu_is_open()
            || self.lbterm_prompt_list_is_open()
            || self.lbterm_conductor_menu_is_open()
            || self.lbterm_inbox_list_is_open()
            || self.lbterm_agenda_list_is_open()
            || self.lbterm_context_list_is_open()
            || self.lbterm_new_tab_is_open()
    }

    /// Whether a menu's card shows over the window: a menu open (the picker beside the rail
    /// isn't one: it holds the rail itself as it opens, render/new_tab.rs), or a card the
    /// compositor is still fading out; and when the last of those fading out has gone. The tab
    /// panel holds still while one shows, so nothing of the rail moves through or over a card
    /// (render/tab_panel.rs, `Hover::hold`; lbterm/FLOWS.md, "A menu, from what opens it to the
    /// end of its fade", step 10)
    pub(crate) fn lbterm_menu_cards_show(&self, now: Instant) -> (bool, Option<Instant>) {
        let open = self.lbterm_menu_is_open()
            || self.lbterm_prompt_list_is_open()
            || self.lbterm_conductor_menu_is_open()
            || self.lbterm_inbox_list_is_open()
            || self.lbterm_agenda_list_is_open()
            || self.lbterm_context_list_is_open()
            || self.lbterm_plus_is_open();
        let compositor = self.webgpu.as_ref().and_then(|w| w.compositor.as_ref());
        // A card the compositor still has open shows too: in the paint where its menu closes, its
        // fade out starts only in the frame after it (render/composited.rs asks for its end then)
        let showing = compositor.map_or(false, |c| (0..MENU_SLOTS).any(|slot| c.menu_showing(slot, now)));
        let fading = compositor.and_then(|c| c.menus_fading_until(now));
        (open || showing, fading)
    }

    /// A key down while a menu takes every key, looked up in the default key table as the
    /// bindings would be, for the bindings an open menu acts on rather than only brightening its
    /// hairline: a key the + menu shows beside a row as its shortcut picks that row (Lachlan's 5a
    /// of 2026-11-08), and a paste key over the picker pastes into its box (his 11a), both
    /// render/new_tab.rs's `lbterm_new_tab_binding`. Only those two take any binding, so the menus
    /// before them in [`TermWindow::lbterm_menus_key`]'s order, which get the key first, act on
    /// none: with one of them open the key is a key no row takes. keyevent.rs asks this in
    /// raw_key_event_impl for each form the binding passes look keys up by (the physical code, the
    /// raw code, the key) and in key_event_impl's pass for the cooked key; true when it acted,
    /// and the key goes no further (the raw event marked handled, so no cooked one follows). Any
    /// other key goes on to [`TermWindow::lbterm_menus_key`] as before (his 7b of 2026-11-07)
    pub(crate) fn lbterm_menus_binding(&mut self, key: &::window::KeyCode, mods: ::window::Modifiers) -> bool {
        if !self.lbterm_new_tab_is_open()
            || self.lbterm_menu_is_open()
            || self.lbterm_prompt_list_is_open()
            || self.lbterm_conductor_menu_is_open()
            || self.lbterm_inbox_list_is_open()
            || self.lbterm_agenda_list_is_open()
            || self.lbterm_context_list_is_open()
        {
            return false;
        }
        let Some(entry) = self.input_map.lookup_key(key, mods, None) else {
            return false;
        };
        self.lbterm_new_tab_binding(&entry.action, key, mods)
    }

    /// A key pressed while a menu takes every key ([`TermWindow::lbterm_menu_takes_keys`]), for
    /// the first one open in this order: the tab menu, the prompt list, the Conductor's, the InMail
    /// list, the agenda, the context list, the picker and its + menu (each answers false only when it isn't open).
    /// Composed words (an IME's, a dead key's) go into the name the tab menu is renaming to or the
    /// picker's box; anywhere else they're a key no row takes. Nothing reaches the pane
    pub(crate) fn lbterm_menus_key(&mut self, key: Key, mods: KeyModifiers) {
        match key {
            Key::Code(code) => {
                let _ = self.lbterm_tab_menu_key(&code, mods)
                    || self.lbterm_prompt_list_key(&code, mods)
                    || self.lbterm_conductor_menu_key(&code, mods)
                    || self.lbterm_inbox_list_key(&code, mods)
                    || self.lbterm_agenda_list_key(&code, mods)
                    || self.lbterm_context_list_key(&code, mods)
                    || self.lbterm_new_tab_key(&code, mods);
            }
            Key::Composed(words) => {
                if self.lbterm_tab_menu_text(&words) {
                    return;
                }
                if self.lbterm_prompt_list_is_open() {
                    self.lbterm_menu_bump(Slot::Prompt);
                } else if self.lbterm_conductor_menu_is_open() {
                    self.lbterm_menu_bump(Slot::Conductor);
                } else if self.lbterm_inbox_list_is_open()
                    || self.lbterm_agenda_list_is_open()
                    || self.lbterm_context_list_is_open()
                {
                    self.lbterm_menu_bump(Slot::List);
                } else {
                    self.lbterm_new_tab_text(&words);
                }
            }
            Key::None => {}
        }
    }

    /// The row lit under the pointer or the keys: with the compositor, its slot's highlight, which
    /// glides there; without, drawn on the card in `quads` (its card z-layer's)
    pub(crate) fn lbterm_menu_light(
        &self,
        slot: Slot,
        quads: &mut TripleLayerQuadAllocator,
        row: Rect,
        card_rgb: u32,
    ) -> anyhow::Result<()> {
        if self.lbterm_composited() {
            self.lbterm_menu.fades.borrow_mut().slots[slot as usize].light = Some(row);
            return Ok(());
        }
        let scale = self.dimensions.dpi as f64 / 96.0;
        let lit = srgb(palette::blend(card_rgb, palette::HOVER, LIGHT_LEVEL), 1.0);
        self.lbterm_menu_rounded(quads, row, LIGHT_RADIUS * scale, lit)
    }

    /// Each slot's card for the compositor this frame (render/composited.rs): open where a menu
    /// drew it this paint, fading out where it last was while its fade lasts, else put away
    pub(crate) fn lbterm_menu_visuals(&self) -> [Option<MenuVisual>; MENU_SLOTS] {
        let now = Instant::now();
        let scale = self.dimensions.dpi as f64 / 96.0;
        let compositor = self.webgpu.as_ref().and_then(|w| w.compositor.as_ref());
        let showing = |slot| compositor.map_or(false, |c| c.menu_showing(slot, now));
        self.lbterm_menu.fades.borrow_mut().visuals(showing, LIGHT_RADIUS * scale)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn each_menus_layers_go_to_its_slots_card() {
        assert_eq!(target_of(60), Some(Target::MenuCard(0)));
        assert_eq!(target_of(61), Some(Target::MenuCard(0)));
        assert_eq!(target_of(62), Some(Target::MenuCard(0)));
        assert_eq!(target_of(65), Some(Target::MenuCard(1)));
        assert_eq!(target_of(67), Some(Target::MenuCard(2)));
        assert_eq!(target_of(77), Some(Target::MenuCard(3)));
        assert_eq!(target_of(79), Some(Target::MenuCard(3)));
        // The Conductor's spinner, a tab link drag's, the tasks card's: not the menus'
        for z in [69, 76, 80, 88, 92] {
            assert_eq!(target_of(z), None, "{z}");
        }
        for slot in [Slot::Tab, Slot::Prompt, Slot::Conductor, Slot::List] {
            assert_eq!(target_of(slot.glow_z()), Some(Target::MenuCard(slot as usize)));
            assert_eq!(target_of(slot.text_z()), Some(Target::MenuCard(slot as usize)));
        }
    }

    fn rect(y: i32) -> PixelRect {
        PixelRect::new(10, y, 200, 120)
    }

    fn ring() -> MenuRing {
        let card = Rect { x: 20.0, y: 50.0, w: 180.0, h: 100.0 };
        MenuRing { outer: motion_rect(grow(card, 1.0)), outer_radius: 13.0, inner: motion_rect(card), inner_radius: 12.0 }
    }

    #[test]
    fn keys_nowhere_go_to_the_compositor_with_the_card_and_are_kept_across_paints() {
        let mut fades = MenuFades::default();
        fades.begin();
        fades.slots[0].drawn = Some((rect(40), ring()));
        assert_eq!(fades.visuals(|_| true, 6.0)[0].unwrap().bumps, 0);
        // Two keys with nowhere to go, then a paint: the count, and the edge to flare
        fades.slots[0].bumps += 2;
        fades.begin();
        fades.slots[0].drawn = Some((rect(40), ring()));
        let v = fades.visuals(|_| true, 6.0)[0].unwrap();
        assert_eq!((v.bumps, v.ring), (2, ring()));
        // A paint starting doesn't forget them: the compositor answers only a count that grew
        fades.begin();
        assert_eq!(fades.slots[0].bumps, 2);
    }

    #[test]
    fn a_bound_key_over_the_tab_menu_is_a_key_no_row_takes() {
        // Lachlan's 7b of 2026-11-07: an open menu takes every key, his bound ones too, and one
        // it has no use for flares the card's edge (Did::Nowhere), never a row's pick: Ctrl+Shift+R
        // isn't Rename's R, nor Ctrl+Shift+X Close tab's X
        let items = menu::items(&menu::Facts::default(), menu::Page::Main);
        let ctrl_shift = KeyModifiers::CTRL | KeyModifiers::SHIFT;
        for (key, mods) in [
            // Ctrl+Shift+T, Reopen: as a letter either way round, and as the control character
            (KeyCode::Char('T'), ctrl_shift),
            (KeyCode::Char('t'), ctrl_shift),
            (KeyCode::Char('\u{14}'), ctrl_shift),
            (KeyCode::Char('t'), KeyModifiers::CTRL),
            (KeyCode::Char('t'), KeyModifiers::CTRL | KeyModifiers::ALT),
            (KeyCode::Char('w'), KeyModifiers::CTRL),
            (KeyCode::Char('s'), KeyModifiers::ALT),
            (KeyCode::Char('r'), ctrl_shift),
            (KeyCode::Char('x'), ctrl_shift),
            (KeyCode::Tab, KeyModifiers::CTRL),
            (KeyCode::Function(9), KeyModifiers::NONE),
        ] {
            let did = menu::answer(menu_key(&key, mods), menu::Page::Main, &items, None);
            assert_eq!(did, menu::Did::Nowhere, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn the_menus_keys_are_plain_characters_and_their_moves_and_a_modifier_is_nothing() {
        use menu::Key;
        let none = KeyModifiers::NONE;
        assert_eq!(menu_key(&KeyCode::Char('r'), none), Key::Char('r'));
        assert_eq!(menu_key(&KeyCode::Char('R'), KeyModifiers::SHIFT), Key::Char('R'));
        assert_eq!(menu_key(&KeyCode::Char('3'), none), Key::Char('3'));
        // With Ctrl or Alt a letter is no row's: nowhere
        assert_eq!(menu_key(&KeyCode::Char('r'), KeyModifiers::CTRL), Key::Other);
        assert_eq!(menu_key(&KeyCode::Char('r'), KeyModifiers::ALT), Key::Other);
        assert_eq!(menu_key(&KeyCode::Enter, none), Key::Enter);
        assert_eq!(menu_key(&KeyCode::Delete, none), Key::Delete);
        assert_eq!(menu_key(&KeyCode::Tab, none), Key::Other);
        assert_eq!(menu_key(&KeyCode::Function(5), none), Key::Other);
        for m in [KeyCode::Shift, KeyCode::Control, KeyCode::Alt, KeyCode::LeftShift, KeyCode::RightControl] {
            assert_eq!(menu_key(&m, none), Key::Modifier, "{m:?}");
        }
    }

    #[test]
    fn a_menu_drawn_is_open_one_not_drawn_fades_out_where_it_was_then_goes() {
        let mut fades = MenuFades::default();
        fades.begin();
        fades.slots[0].drawn = Some((rect(40), ring()));
        fades.slots[0].light = Some(Rect { x: 12.0, y: 50.0, w: 196.0, h: 28.0 });
        let v = fades.visuals(|_| true, 6.0);
        let open = v[0].expect("the tab menu");
        assert!(open.open);
        assert_eq!(open.rect, rect(40));
        assert_eq!(open.light.map(|r| (r.y, r.h)), Some((50.0, 28.0)));
        assert!(v[1..].iter().all(Option::is_none));
        // Closed: not drawn in the next paint, its picture fades where it was, its row as it was
        fades.begin();
        let v = fades.visuals(|_| true, 6.0);
        let closing = v[0].expect("fading out");
        assert!(!closing.open);
        assert_eq!(closing.rect, rect(40));
        assert_eq!(closing.light, None);
        // Its fade done: put away, and it stays away
        fades.begin();
        assert!(fades.visuals(|_| false, 6.0)[0].is_none());
        fades.begin();
        assert!(fades.visuals(|_| true, 6.0)[0].is_none());
    }

    #[test]
    fn a_menu_opened_again_mid_fade_is_open_where_it_is_drawn_now() {
        let mut fades = MenuFades::default();
        fades.begin();
        fades.slots[3].drawn = Some((rect(40), ring()));
        fades.visuals(|_| true, 6.0);
        fades.begin();
        assert!(!fades.visuals(|_| true, 6.0)[3].unwrap().open);
        fades.begin();
        fades.slots[3].drawn = Some((rect(300), ring()));
        let back = fades.visuals(|_| true, 6.0)[3].unwrap();
        assert!(back.open);
        assert_eq!(back.rect, rect(300));
    }
}
