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
const UNLIT: u32 = 0x24242C;

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

/// A click's ripple at its start, white over what it lights, fading to nothing
/// (lbterm/DESIGN.md, "Colour", `press`)
pub const PRESS: f64 = 0.35;
/// A copied or opened stretch of text at its brightest (lbterm/DESIGN.md, `copy`)
pub const COPY_FLASH: f64 = 0.22;

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

/// The pad's fill.
pub fn pad(state: RowState) -> u32 {
    match state {
        RowState::Working => GREEN,
        RowState::Waiting => 0xFF8A3D,
        RowState::Idle => 0x5B7CFF,
        RowState::Error => RED,
        RowState::NoClaude => UNLIT,
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
    }
}

/// The status words: the pad's colour lifted to read on the dark panel.
pub fn status(state: RowState) -> u32 {
    match state {
        RowState::Working => 0x7CF07C,
        RowState::Waiting => 0xFF9A62,
        RowState::Idle => 0x96A3FF,
        RowState::Error => 0xFF7070,
        RowState::NoClaude => 0x8A8A9E,
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

#[cfg(test)]
mod test {
    use super::*;

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
