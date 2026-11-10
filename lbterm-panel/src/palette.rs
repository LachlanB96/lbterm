//! The panel's colours, as 0xRRGGBB sRGB, from the "edge light" design
//! Lachlan picked (2026-10-20): APCommand's states (green working, orange
//! blocked, blue your turn, dim without Claude) in Android's softer shades,
//! on a card the colour of the panes, ringed in grey at rest and in a red
//! gradient that glows while it's open.

use crate::claude::RowState;
use crate::idle::Bar;

/// The ring, top to bottom
pub const RING_TOP: u32 = 0xFF3B30;
pub const RING_MIDDLE: u32 = 0xFF8A5C;
pub const RING_BOTTOM: u32 = 0xFF2D6F;
/// The light the ring casts
pub const GLOW: u32 = 0xFF3B30;
/// How far the ring's light and glow come up while the panel waits on the
/// pointer (`Light::Pending`): resting on the closed rail, or gone from the
/// open panel. Open it's 1, at rest 0. The answer the pointer gets at once,
/// before the edge moves (lbterm/DESIGN.md, principle 1)
pub const PENDING: f64 = 0.35;
/// Laid over the panel for the active tab's highlight, and at this strength.
/// At full strength it's the ring round the open tab's highlight and the bar
/// beside it down the rail's edge (Lachlan's pick, 2026-10-20)
pub const ACTIVE: u32 = 0xFF5449;
pub const ACTIVE_ALPHA: f64 = 0.16;
/// ...and for the row under the pointer
pub const HOVER: u32 = 0xFFFFFF;
pub const HOVER_ALPHA: f64 = 0.06;
pub const TITLE: u32 = 0xECECF4;
pub const PROJECT: u32 = 0xB4B4CC;
/// A label under a pad, but the open tab's, which is the title's colour
pub const LABEL: u32 = 0xA8A8BC;
/// Behind the project's name
pub const CHIP: u32 = 0x262634;
/// A tab hidden from the Conductor (lbterm/NEW-TAB.md, section 8): the eye-off mark on its pad's
/// bottom-right corner, white at 60% over the disc it sits on, and on the open panel the
/// "Hidden" chip's words, white at 45% over the project chip's fill. Never a status colour: the
/// pad keeps its own, orange when it waits on him
pub const HIDDEN_MARK: f64 = 0.60;
pub const HIDDEN_WORD: f64 = 0.45;
/// The eye-off mark's colour on a disc of `under`
pub fn hidden_mark(under: u32) -> u32 {
    blend(under, 0xFFFFFF, HIDDEN_MARK)
}
/// The "Hidden" chip's words, on [`CHIP`]
pub fn hidden_word() -> u32 {
    blend(CHIP, 0xFFFFFF, HIDDEN_WORD)
}
/// A parked tab's row on the rail (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing 4"): its pad, bars,
/// rings, number, label and marks drawn this share of the way from what's under them (the row's
/// pill), blended and drawn opaque (PITFALLS 248), as if behind a veil of the pill's colour. The
/// open tab's ring and bar beside its pill aren't the row's, and stay
pub const PARKED: f64 = 0.40;
/// `colour` on a parked row whose pill is `under`
pub fn parked(under: u32, colour: u32) -> u32 {
    blend(under, colour, PARKED)
}
/// The "Parked" chip's words on the open panel, as the "Hidden" chip's, on [`CHIP`]
pub fn parked_word() -> u32 {
    blend(CHIP, 0xFFFFFF, HIDDEN_WORD)
}
/// The mark on a pad whose result hasn't been seen
pub const UNREAD: u32 = 0xFFFFFF;
/// Under a progress bar's fill
pub const TRACK: u32 = 0x2A2A36;
/// The ring at rest: a quiet grey, so only the open panel's ring is lit
pub const OUTLINE: u32 = 0x5E5E6E;
/// A pane's card outline at rest: white at this share of the way from the
/// card, as the eye sees it (lbterm/DESIGN.md, "Colour")
pub const OUTLINE_REST: f64 = 0.06;
/// The light running round the ring: its halo, warmer and brighter than the
/// glow at the ring, and its middle, nearly white
pub const SPOT: u32 = 0xFF6B4F;
pub const SPOT_PEAK: f64 = 0.55;
pub const SPOT_CORE: u32 = 0xFFE4DA;
pub const SPOT_CORE_PEAK: f64 = 0.9;

const GREEN: u32 = 0x3DDC84;
const YELLOW: u32 = 0xF2E86D;
const RED: u32 = 0xFF5449;
/// Something that failed, said outside the pads: the head of the scroll strip's note that a
/// write to the pane didn't get there. The status red (lbterm/DESIGN.md, "Colour": status
/// colours mean status, and red is failed)
pub const FAILED: u32 = RED;
const UNLIT: u32 = 0x24242C;
/// A tab waiting on another tab (Lachlan's 4a, 2026-10-29: "it should go purple"): the tab
/// menu's purple swatch (#B083F0) a shade deeper, so the white number on it reads
const LINKED: u32 = 0x9F6EEB;

/// A working reply gone quiet or stuck (Lachlan's 2a, 2026-11-02): amber, the ring round the green
/// pad at 3 minutes and the pad itself at 7, yellower than the waiting orange
pub const AMBER: u32 = 0xFFBF00;

/// The ring round a quiet reply's green pad
pub fn quiet_ring() -> u32 {
    AMBER
}

/// A tab link's line down the rail's edge, as it stands (lbterm_extras::links::Look)
pub const LINK_WAITING: u32 = LINKED;
/// What it waits on finished: green until it has carried on
pub const LINK_DONE: u32 = GREEN;
pub const LINK_ERROR: u32 = RED;
/// A suggested link is drawn dashed, this much of the purple over what's under it
pub const LINK_SUGGESTED_ALPHA: f64 = 0.55;
/// Two or more tabs connected, so their sessions talk (lbterm/LINKS.md, "Connections"): cyan,
/// Lachlan's colour for connections (2026-11-01), between the done green (hue 147°) and the extras
/// blue (228°) at 189°, as bright as the green so it reads on the rail beside the purple links. A
/// suggested connection's line is this much of it over what's under it, as a suggested link's is
pub const CONNECTED: u32 = 0x33CCE8;
/// The light that runs along a suggested connection's line: the cyan lifted towards white, so it
/// reads as light
pub const CONNECTION_LIGHT: u32 = 0xD4F8FF;
/// The count on the Conductor's pad of the connections suggested on its rail (his E8), on a badge
/// of [`CONNECTED`]: the cyan's own dark, as the working pad's number is the green's
pub const CONNECTED_INK: u32 = 0x031C22;

/// The ring round a linked pad that asks you something too: the waiting pad's orange, whole
/// (Lachlan's 4a, 2026-10-29: "if it also has question, it gets the orange outline")
pub fn asks_ring() -> u32 {
    pad(RowState::Waiting)
}

/// `over` laid on `base` at `alpha`, as one solid colour: highlights that
/// meet or overlap then never show a seam.
pub fn blend(base: u32, over: u32, alpha: f64) -> u32 {
    let mix = |shift: u32| {
        let (b, o) = (((base >> shift) & 0xFF) as f64, ((over >> shift) & 0xFF) as f64);
        ((b + (o - b) * alpha).round() as u32).min(255)
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/// How much of a pane shows without the keyboard: its text at 85%
/// (lbterm/DESIGN.md, "Colour")
pub const INACTIVE: f64 = 0.85;

/// A pane's card outline at rest, on a card of colour `card`.
pub fn card_outline(card: u32) -> u32 {
    blend(card, 0xFFFFFF, OUTLINE_REST)
}

/// The active outline at its brightest, as the keyboard arrives from outside the window: white
/// at this share of the way from the card, the press token's 35% (lbterm/DESIGN.md, "Colour")
pub const OUTLINE_LIT: f64 = 0.35;

/// The active outline's template with the compositor, drawn at its brightest: DWM shows it at
/// [`outline_rest_opacity`] the rest of the time.
pub fn card_outline_lit(card: u32) -> u32 {
    blend(card, 0xFFFFFF, OUTLINE_LIT)
}

/// The active outline while modifiers are held (Lachlan, 2026-10-25: a colour for each, so a held
/// key that didn't register shows; lbterm/DESIGN.md, "Colour"): Ctrl blue, Shift green, Alt red,
/// mixed as light when more than one is held, so Ctrl+Shift is cyan, Ctrl+Alt magenta,
/// Shift+Alt yellow and all three white. Shown at the outline's full light.
pub const HELD_CTRL: u32 = 0x3D7BFF;
pub const HELD_SHIFT: u32 = 0x2EE06A;
pub const HELD_ALT: u32 = 0xFF4040;

/// The colour of the held modifiers, added as light (each channel summed, at most full); `None`
/// with none held
pub fn held(ctrl: bool, shift: bool, alt: bool) -> Option<u32> {
    let lit = [(ctrl, HELD_CTRL), (shift, HELD_SHIFT), (alt, HELD_ALT)];
    if !lit.iter().any(|(on, _)| *on) {
        return None;
    }
    let add = |shift: u32| {
        lit.iter()
            .filter(|(on, _)| *on)
            .map(|(_, c)| (c >> shift) & 0xFF)
            .sum::<u32>()
            .min(255)
    };
    Some((add(16) << 16) | (add(8) << 8) | add(0))
}

/// A click's ripple at its start, white over what it lights, fading to nothing
/// (lbterm/DESIGN.md, "Colour", `press`)
pub const PRESS: f64 = 0.35;
/// A copied or opened stretch of text at its brightest (lbterm/DESIGN.md, `copy`)
pub const COPY_FLASH: f64 = 0.22;

/// A key no row of an open menu takes (lbterm/DESIGN.md, "Menus"; Lachlan's 9a of 2026-11-08):
/// the card's edge flares, its hairline going all the way to white at the flare's peak...
pub const MENU_BUMP_HAIRLINE: f64 = 1.0;
/// ...and the light round the card going white under its own red, this strong at the card's
/// edge (the `light` token's most, white 50%), falling away as the card's light does out to its
/// reach ([`menu_bump_light_scale`])
pub const MENU_BUMP_LIGHT: f64 = 0.5;

/// What the card's light's alpha is multiplied by to make the flare's white light round it, at
/// the flare's peak: [`MENU_BUMP_LIGHT`] where the card's light is [`MENU_GLOW_EDGE`], at the
/// card's edge, and the same share of it further out, so the white falls away with the red. The
/// compositor's colour matrix over the card's own surface (`MenuLamp`'s halo), whose opacity
/// plays the flare from 0 to [`MENU_BUMP_HAIRLINE`]
pub fn menu_bump_light_scale() -> f64 {
    MENU_BUMP_LIGHT / (MENU_GLOW_EDGE * MENU_BUMP_HAIRLINE)
}

/// The opacity that shows [`card_outline_lit`] as [`card_outline`]. DWM blends a visual in
/// its stored values, as [`blend`] mixes, so a share of the lit line is that share of the way
/// from the card.
pub fn outline_rest_opacity() -> f64 {
    OUTLINE_REST / OUTLINE_LIT
}

/// The ring's colour `t` of the way down it, 0 at the top.
pub fn ring(t: f64) -> u32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        blend(RING_TOP, RING_MIDDLE, t * 2.0)
    } else {
        blend(RING_MIDDLE, RING_BOTTOM, (t - 0.5) * 2.0)
    }
}

/// How far the ring round a finished pad that only offers extras goes from
/// what's under the pad towards the waiting pad's orange: blue, with a subtle
/// orange outline (Lachlan's ask, 2026-10-23)
pub const EXTRAS_RING: f64 = 0.6;

/// That ring's colour, over `under`.
pub fn extras_ring(under: u32) -> u32 {
    blend(under, pad(RowState::Waiting), EXTRAS_RING)
}

/// The pad's fill.
pub fn pad(state: RowState) -> u32 {
    match state {
        RowState::Working => GREEN,
        RowState::Waiting => 0xFF8A3D,
        RowState::Idle => 0x5B7CFF,
        RowState::Error => RED,
        RowState::NoClaude => UNLIT,
        RowState::Linked => LINKED,
        RowState::Stuck => AMBER,
    }
}

/// The tab number on the pad.
pub fn number(state: RowState) -> u32 {
    match state {
        RowState::Working => 0x0B2414,
        RowState::Waiting => 0x2A1200,
        RowState::Idle => 0xFFFFFF,
        RowState::Error => 0xFFFFFF,
        RowState::NoClaude => 0x8A8A9E,
        RowState::Linked => 0xFFFFFF,
        RowState::Stuck => 0x2A1E00,
    }
}

/// A guessed bar's fill (the Conductor's estimates): the status colour half sunk into the
/// track, so a guess reads apart from progress a program reports.
pub fn estimate(state: RowState) -> u32 {
    blend(TRACK, status(state), 0.55)
}

/// The status words: the pad's colour lifted to read on the dark panel.
pub fn status(state: RowState) -> u32 {
    match state {
        RowState::Working => 0x7CF07C,
        RowState::Waiting => 0xFF9A62,
        RowState::Idle => 0x96A3FF,
        RowState::Error => 0xFF7070,
        RowState::NoClaude => 0x8A8A9E,
        RowState::Linked => 0xC6A6FF,
        RowState::Stuck => 0xFFCC40,
    }
}

pub fn bar(bar: Bar) -> u32 {
    match bar {
        Bar::Green => GREEN,
        Bar::Yellow => YELLOW,
        Bar::Red => RED,
        Bar::Off => UNLIT,
    }
}

/// The age, in the colour of the timer's stage.
pub fn age(level: u8) -> u32 {
    match level.saturating_sub(1) / 3 {
        0 => GREEN,
        1 => YELLOW,
        _ => RED,
    }
}

/// The Conductor's mode under its pad, in the idle timer's colours: read only green, accept most
/// yellow, accept all red (lbterm/DESIGN.md, picked 2026-10-21)
pub const MODE_READ: u32 = GREEN;
pub const MODE_MOST: u32 = YELLOW;
pub const MODE_ALL: u32 = RED;
/// The Conductor's pad, its glyph while it's off and while it's on, and its words while it's off
pub const CONDUCTOR_PAD: u32 = UNLIT;
pub const CONDUCTOR_OFF: u32 = 0x8A8A9E;
pub const CONDUCTOR_ON: u32 = TITLE;

/// The Conductor's outline round its pad while it may act on its own: every hue, strong. `hue` is
/// 0 to 1 round the outline; DWM turns the hues, so the colours run round it.
pub fn spectrum(hue: f64) -> u32 {
    hsv(hue, 0.85, 1.0)
}

/// The outline round a tab's pad while the Conductor is on that tab: every hue, pale, as on
/// foil (lbterm/DESIGN.md, picked 2026-10-21)
pub fn iridescent(hue: f64) -> u32 {
    hsv(hue, 0.30, 1.0)
}

/// `hue` (0 to 1, wrapping), saturation and value as sRGB
fn hsv(hue: f64, s: f64, v: f64) -> u32 {
    let h = hue.rem_euclid(1.0) * 6.0;
    let (sector, f) = (h.floor() as u32 % 6, h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match sector {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

/// The three bytes of `rgb`.
pub fn bytes(rgb: u32) -> (u8, u8, u8) {
    ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// sRGB's transfer function, from linear light to the 0-1 a pixel stores...
pub fn encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// ...and back.
pub fn decode(stored: f64) -> f64 {
    if stored <= 0.040_45 {
        stored / 12.92
    } else {
        ((stored + 0.055) / 1.055).powf(2.4)
    }
}

/// How bright the light is at the ring: this share of the way from the card
/// to the light's colour, as the eye sees it.
pub const GLOW_PEAK: f64 = 0.30;

/// The light's alpha `distance` pixels out from the ring, for a light
/// reaching `reach` pixels over `base`: its knots, which the panel blends
/// between.
///
/// The light is drawn into a surface with premultiplied alpha, each pixel
/// once, in linear light; DWM then lays that surface over the panes with its
/// stored, gamma-encoded values. Over a dark pane that makes a faint light
/// look far stronger than its alpha, so even falloff in alpha reads as a
/// flat band with a hard edge. The alpha is worked back from what should be
/// seen instead: a Gaussian falloff from [`GLOW_PEAK`] at the ring to
/// nothing at `reach`.
pub fn glow_alpha(distance: f64, reach: f64, base: u32) -> f64 {
    light_alpha(distance, reach, base, GLOW, GLOW_PEAK)
}

/// The same for any `light` rising to `peak` of the way from `base` to it,
/// as the eye sees it: the running light's halo and middle.
pub fn light_alpha(distance: f64, reach: f64, base: u32, light: u32, peak: f64) -> f64 {
    if reach <= 0.0 || distance >= reach {
        return 0.0;
    }
    let light = bytes(light).0 as f64 / 255.0;
    let card = bytes(base).0 as f64 / 255.0;
    let linear = decode(light);
    let seen = |alpha: f64| encode(linear * alpha) + card * (1.0 - alpha);
    let sigma = reach * 0.4;
    let fall = |d: f64| (-d * d / (2.0 * sigma * sigma)).exp();
    let floor = fall(reach);
    let share = ((fall(distance.max(0.0)) - floor) / (1.0 - floor)).max(0.0);
    let target = card + (light - card) * peak * share;
    // seen() rises with alpha: halve the interval
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        if seen(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Every menu's fill: [`CHIP`] at this share over the surface under it (lbterm/DESIGN.md,
/// "Menus": #262634 at 55%)
pub const MENU_FILL: f64 = 0.55;

/// Every menu's card (the tab menu, the + menu and picker, the Conductor's, the InMail list, the
/// agenda, the prompt list), over `surface`, the window's background
pub fn menu_fill(surface: u32) -> u32 {
    blend(surface, CHIP, MENU_FILL)
}

/// How far a menu's light reaches past its card, at 96 DPI: as far as the four rings it replaced
/// (2, 5, 8 and 11 px out)
pub const MENU_GLOW_REACH: f64 = 11.0;

/// A menu's light at its card's edge: the four rings it replaced (16, 9, 5 and 2.5%) stacked
/// there, 1 - 0.84 x 0.91 x 0.95 x 0.975. Its strength until 2026-11-05, kept
pub const MENU_GLOW_EDGE: f64 = 0.292;

/// A menu's light `distance` pixels out from its card, reaching `reach`, over `base`: the panel's
/// falloff ([`glow_alpha`], worked back from what's seen), scaled so it's as strong at the card's
/// edge as the four stacked rings were ([`MENU_GLOW_EDGE`]). Drawn once a pixel, as the panel's
/// light is, the rings' bands are gone (lbterm/TAB-PANEL.md, "The light")
pub fn menu_glow_alpha(distance: f64, reach: f64, base: u32) -> f64 {
    let edge = glow_alpha(0.0, reach, base);
    if edge <= 0.0 {
        return 0.0;
    }
    glow_alpha(distance, reach, base) * MENU_GLOW_EDGE / edge
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_menus_fill_is_the_chip_colour_at_55_percent_over_the_surface() {
        assert_eq!(menu_fill(0x000000), blend(0x000000, 0x262634, 0.55));
        assert_eq!(menu_fill(0x121212), 0x1D1D25);
    }

    #[test]
    fn a_menus_light_is_the_four_rings_strength_falling_away_without_steps() {
        let reach = MENU_GLOW_REACH;
        // The four rings' alpha at each pixel out, as they were stacked
        let rings = |d: f64| -> f64 {
            1.0 - [0.16, 0.09, 0.05, 0.025]
                .iter()
                .enumerate()
                .filter(|(i, _)| d < 2.0 + 3.0 * *i as f64)
                .map(|(_, a)| 1.0 - a)
                .product::<f64>()
        };
        for base in [0x0C0C0C, 0x121212, 0x1E1E1E] {
            let light: Vec<f64> = (0..=reach as usize).map(|d| menu_glow_alpha(d as f64, reach, base)).collect();
            // As strong as the rings at the card's edge...
            assert!((light[0] - MENU_GLOW_EDGE).abs() < 1e-9, "{light:?}");
            assert!((rings(0.0) - MENU_GLOW_EDGE).abs() < 0.001);
            // ...falling every pixel out, never by a band's step at once (the rings fell by 0.135
            // from one pixel to the next)...
            for pair in light.windows(2) {
                assert!(pair[1] <= pair[0], "{light:?}");
                assert!(pair[0] - pair[1] < 0.07, "a step: {light:?}");
            }
            // ...gone at the reach...
            assert_eq!(menu_glow_alpha(reach, reach, base), 0.0);
            // ...and about as much light in all as the rings gave
            let total: f64 = light.iter().sum();
            let was: f64 = (0..=reach as usize).map(|d| rings(d as f64 + 0.5)).sum();
            assert!((total / was - 1.0).abs() < 0.15, "{total} against the rings' {was}");
        }
    }

    #[test]
    fn a_menus_flare_is_its_light_gone_white_at_half_strength_at_its_edge_falling_away_with_it() {
        let k = menu_bump_light_scale();
        let base = 0x121212;
        let reach = MENU_GLOW_REACH;
        // White 50% at the card's edge, as the `light` token's most
        assert!((menu_glow_alpha(0.0, reach, base) * k * MENU_BUMP_HAIRLINE - MENU_BUMP_LIGHT).abs() < 1e-9);
        // Out from there, the same share of the card's light, so it never goes past a whole
        // white and is gone at the reach
        for d in 0..=reach as usize {
            let white = (menu_glow_alpha(d as f64, reach, base) * k).min(1.0);
            assert!(white <= MENU_BUMP_LIGHT + 1e-9, "{d} px out: {white}");
        }
        assert_eq!(menu_glow_alpha(reach, reach, base) * k, 0.0);
        // The hairline goes all the way to white
        assert_eq!(MENU_BUMP_HAIRLINE, 1.0);
    }

    #[test]
    fn each_held_modifier_has_its_colour_and_they_mix_as_light() {
        assert_eq!(held(false, false, false), None);
        assert_eq!(held(true, false, false), Some(HELD_CTRL));
        assert_eq!(held(false, true, false), Some(HELD_SHIFT));
        assert_eq!(held(false, false, true), Some(HELD_ALT));
        // Blue and green make cyan, blue and red magenta, green and red yellow, all three white
        let (r, g, b) = bytes(held(true, true, false).unwrap());
        assert!(g == 255 && b == 255 && r < 128, "Ctrl+Shift reads as cyan: {r} {g} {b}");
        let (r, g, b) = bytes(held(true, false, true).unwrap());
        assert!(r == 255 && b == 255 && g < 200, "Ctrl+Alt reads as magenta: {r} {g} {b}");
        let (r, g, b) = bytes(held(false, true, true).unwrap());
        assert!(r == 255 && g == 255 && b < 200, "Shift+Alt reads as yellow: {r} {g} {b}");
        assert_eq!(held(true, true, true), Some(0xFFFFFF));
        // Every one tells apart from the others
        let all: Vec<u32> = (1..8u8)
            .map(|m| held(m & 1 != 0, m & 2 != 0, m & 4 != 0).unwrap())
            .collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "{a:06x} twice");
        }
    }

    /// A colour's hue in degrees
    fn hue(rgb: u32) -> f64 {
        let (r, g, b) = bytes(rgb);
        let (r, g, b) = (r as f64, g as f64, b as f64);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = (max - min).max(1.0);
        let h = if max == r {
            60.0 * ((g - b) / d)
        } else if max == g {
            60.0 * ((b - r) / d) + 120.0
        } else {
            60.0 * ((r - g) / d) + 240.0
        };
        (h + 360.0) % 360.0
    }

    #[test]
    fn connections_are_a_cyan_apart_from_every_other_colour_on_the_rail() {
        let cyan = hue(CONNECTED);
        // Between the done green and the extras blue, well clear of both and of the link purple
        assert!(cyan > hue(LINK_DONE) + 25.0 && cyan < hue(pad(RowState::Idle)) - 25.0, "hue {cyan:.0}");
        for other in [LINK_WAITING, LINK_DONE, LINK_ERROR, pad(RowState::Waiting), pad(RowState::Idle), pad(RowState::Working)] {
            let apart = (hue(other) - cyan).abs().min(360.0 - (hue(other) - cyan).abs());
            assert!(apart >= 25.0, "{other:06x} is {apart:.0}° from the cyan");
        }
        // The light along a suggested one is the cyan lifted towards white
        let (r, g, b) = bytes(CONNECTION_LIGHT);
        let (cr, cg, cb) = bytes(CONNECTED);
        assert!(r > cr && g > cg && b >= cb && (hue(CONNECTION_LIGHT) - cyan).abs() < 12.0);
        // The Conductor's count reads on its cyan badge: the cyan's own dark, at about the contrast
        // the working pad's number has on its green (9.2)
        let luminance = |rgb: u32| {
            let (r, g, b) = bytes(rgb);
            let lin = |c: u8| decode(c as f64 / 255.0);
            0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        };
        let contrast = |a: u32, b: u32| {
            let (x, y) = (luminance(a), luminance(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        let ink = contrast(CONNECTED_INK, CONNECTED);
        let working = contrast(number(RowState::Working), pad(RowState::Working));
        assert!(ink >= 7.0 && (ink - working).abs() < 1.0, "{ink:.1} against {working:.1}");
        assert!((hue(CONNECTED_INK) - cyan).abs() < 12.0, "a cyan's dark");
    }

    #[test]
    fn a_stuck_pad_is_an_amber_apart_from_the_waiting_orange_and_the_failed_red() {
        let amber = hue(pad(RowState::Stuck));
        for other in [pad(RowState::Waiting), pad(RowState::Error), pad(RowState::Working)] {
            let apart = (hue(other) - amber).abs().min(360.0 - (hue(other) - amber).abs());
            assert!(apart >= 20.0, "{other:06x} is {apart:.0}° from the amber");
        }
        assert_eq!(quiet_ring(), pad(RowState::Stuck), "the quiet ring is the stuck pad's amber");
        // Its number reads on it as the working pad's does on green
        let luminance = |rgb: u32| {
            let (r, g, b) = bytes(rgb);
            let lin = |c: u8| decode(c as f64 / 255.0);
            0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        };
        let contrast = |a: u32, b: u32| {
            let (x, y) = (luminance(a), luminance(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        assert!(contrast(number(RowState::Stuck), pad(RowState::Stuck)) >= 7.0);
    }

    #[test]
    fn the_extras_ring_is_a_quiet_orange_on_what_is_under_it() {
        let under = 0x1A1A22;
        let ring = extras_ring(under);
        let orange = pad(RowState::Waiting);
        assert_eq!(ring, blend(under, orange, EXTRAS_RING));
        assert!(ring != orange && ring != under, "between the two");
        let (r, g, b) = bytes(ring);
        assert!(r > g && g > b, "still reads as orange: {ring:06x}");
    }

    #[test]
    fn the_running_light_is_brighter_than_the_glow_and_fades_out() {
        let card = 0x121212;
        let at = |d| light_alpha(d, 64.0, card, SPOT, SPOT_PEAK);
        assert!(at(0.0) > glow_alpha(0.0, 64.0, card), "brighter than the ring's light");
        assert!(at(0.0) > at(20.0) && at(20.0) > at(50.0));
        assert_eq!(at(64.0), 0.0);
        assert_eq!(glow_alpha(10.0, 26.0, card), light_alpha(10.0, 26.0, card, GLOW, GLOW_PEAK));
    }

    #[test]
    fn blending_lays_one_colour_over_another() {
        assert_eq!(blend(0x121212, 0xFF5449, 0.0), 0x121212);
        assert_eq!(blend(0x121212, 0xFF5449, 1.0), 0xFF5449);
        // The active highlight on the panes' #121212, as the design draws it
        assert_eq!(blend(0x121212, ACTIVE, ACTIVE_ALPHA), 0x381D1B);
    }

    #[test]
    fn a_cards_outline_is_a_faint_white_over_it() {
        // 6% of the way from #121212 to white: 18 + 237 * 0.06
        assert_eq!(card_outline(0x121212), 0x202020);
        assert_eq!(card_outline(0x000000), 0x0F0F0F);
    }

    #[test]
    fn the_lit_outline_at_rest_opacity_is_the_outline_at_rest() {
        // DWM lays the lit template over the card at the rest opacity: within a level of the
        // outline drawn at rest, for every grey card
        for card in [0x000000, 0x121212, 0x202020, 0x303030] {
            let (lit, rest) = (card_outline_lit(card) & 0xFF, card_outline(card) & 0xFF);
            let shown = (card & 0xFF) as f64 + (lit as f64 - (card & 0xFF) as f64) * outline_rest_opacity();
            assert!((shown - rest as f64).abs() <= 1.0, "{card:06X}: {shown} against {rest}");
        }
        assert_eq!(card_outline_lit(0x121212), 0x656565);
    }

    #[test]
    fn the_ring_runs_red_to_coral_to_pink() {
        assert_eq!(ring(0.0), RING_TOP);
        assert_eq!(ring(0.5), RING_MIDDLE);
        assert_eq!(ring(1.0), RING_BOTTOM);
        assert_eq!(ring(-1.0), RING_TOP);
    }

    #[test]
    fn srgb_encodes_and_decodes() {
        for v in [0.0, 0.002, 0.04, 0.2, 0.5, 1.0] {
            assert!((decode(encode(v)) - v).abs() < 1e-9, "{v}");
        }
        assert!((encode(0.214) - 0.5).abs() < 0.001, "linear 0.214 is about half grey");
    }

    /// What the light shows `d` pixels out, over `base`, as DWM lays the
    /// surface over the pane, in 0-255.
    fn seen(d: f64, base: u32) -> f64 {
        let alpha = glow_alpha(d, 26.0, base);
        let light = decode(bytes(GLOW).0 as f64 / 255.0);
        let card = bytes(base).0 as f64 / 255.0;
        255.0 * (encode(light * alpha) + card * (1.0 - alpha))
    }

    #[test]
    fn the_light_falls_away_smoothly_to_nothing() {
        let red: Vec<f64> = (0..=26).map(|d| seen(d as f64, 0x121212)).collect();
        // Bright at the ring, a third of the way to the light's colour...
        assert!((red[0] - (18.0 + 237.0 * GLOW_PEAK)).abs() < 1.0, "{red:?}");
        // ...falling every pixel out, never by much at once...
        for pair in red.windows(2) {
            assert!(pair[1] <= pair[0] + 1e-9, "{red:?}");
            assert!(pair[0] - pair[1] < 6.0, "a step: {red:?}");
        }
        // ...and gone at the edge, so there's no line where it stops
        assert!(red[25] - 18.0 < 1.0, "{red:?}");
        assert_eq!(glow_alpha(26.0, 26.0, 0x121212), 0.0);
        // Its alpha stays faint: what the eye sees is the light added
        assert!(glow_alpha(0.0, 26.0, 0x121212) < 0.1);
    }

    #[test]
    fn age_follows_the_timer_stage() {
        assert_eq!(age(1), age(3));
        assert_ne!(age(3), age(4));
        assert_eq!(age(9), RED);
        assert_eq!(age(0), GREEN);
    }

    #[test]
    fn the_conductors_outlines_run_through_every_hue() {
        // Spectrum: strong; red at 0, round to red again
        assert_eq!(bytes(spectrum(0.0)), (255, 38, 38));
        assert_eq!(spectrum(1.0), spectrum(0.0));
        let (r, g, b) = bytes(spectrum(1.0 / 3.0));
        assert!(g == 255 && r < 60 && b < 60, "green a third of the way round: {r} {g} {b}");
        // Iridescent: the same hues, pale, every channel high
        for i in 0..12 {
            let (r, g, b) = bytes(iridescent(i as f64 / 12.0));
            assert!(r >= 178 && g >= 178 && b >= 178, "{i}: {r} {g} {b}");
        }
        assert_ne!(iridescent(0.0), iridescent(0.5));
        // The modes in the timer's colours
        assert_eq!((MODE_READ, MODE_MOST, MODE_ALL), (GREEN, YELLOW, RED));
    }
}
