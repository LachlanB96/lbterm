//! The colours a style gives the cells it matched

/// An sRGB colour, 0-255 per channel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` or `rrggbb`
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Self(channel(0)?, channel(2)?, channel(4)?))
    }
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let c = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// `fg` moved `amount` (0 to 1) of the way to `towards`, mixed in linear light so a tint looks
/// as strong on a dim colour as on a bright one
pub fn tint(fg: Rgb, towards: Rgb, amount: f32) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| to_srgb(to_linear(a) + (to_linear(b) - to_linear(a)) * amount);
    Rgb(mix(fg.0, towards.0), mix(fg.1, towards.1), mix(fg.2, towards.2))
}

/// The rainbow at `t`, 0 at the start of a match to 1 at its end: red through orange, yellow,
/// green and blue to violet, bright enough to read on a dark background
pub fn rainbow(t: f32) -> Rgb {
    let hue = t.clamp(0.0, 1.0) * 280.0;
    hsl(hue, 0.95, 0.65)
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Rgb {
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = (hue % 360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = lightness - c / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(Rgb::parse("#ff8fb8"), Some(Rgb(0xff, 0x8f, 0xb8)));
        assert_eq!(Rgb::parse("102030"), Some(Rgb(0x10, 0x20, 0x30)));
        assert_eq!(Rgb::parse("#fff"), None);
        assert_eq!(Rgb::parse("#gg0000"), None);
    }

    #[test]
    fn tint_ends() {
        let grey = Rgb(200, 200, 200);
        let pink = Rgb(255, 128, 180);
        assert_eq!(tint(grey, pink, 0.0), grey);
        assert_eq!(tint(grey, pink, 1.0), pink);
    }

    #[test]
    fn a_light_tint_stays_close_to_the_text_colour() {
        let t = tint(Rgb(208, 208, 208), Rgb(255, 128, 180), 0.4);
        // pinker: red up, green down, and still a light colour
        assert!(t.0 > 208 && t.1 < 208 && t.1 > 160, "{t:?}");
    }

    #[test]
    fn the_rainbow_runs_red_to_violet() {
        let start = rainbow(0.0);
        let end = rainbow(1.0);
        assert!(start.0 > 240 && start.1 < 120 && start.2 < 120, "{start:?}");
        assert!(end.2 > 240 && end.0 > end.1, "{end:?}");
        let middle = rainbow(0.45);
        assert!(middle.1 > middle.0 && middle.1 > middle.2, "{middle:?}");
    }
}
