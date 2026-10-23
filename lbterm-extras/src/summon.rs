//! The summon key (`lbterm_summon`): one key that brings LBTerm forward from any other app,
//! held with Windows' RegisterHotKey only while LBTerm hasn't got the keyboard, so inside
//! LBTerm the key reaches the terminal as usual. Lachlan's is PgUp (PgDn opens the Relay box).
//!
//! Here: reading the key and modifiers the config names into what RegisterHotKey takes.

/// RegisterHotKey's modifier bits
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
/// Holding the key down doesn't repeat the hotkey
pub const MOD_NOREPEAT: u32 = 0x4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: u32,
    pub vk: u32,
}

/// The virtual-key code for a key named as wezterm names keys (`PageUp`, `F12`, `Insert`,
/// `a`, `5`), in any case
pub fn virtual_key(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    let vk = match lower.as_str() {
        "pageup" | "pgup" | "prior" => 0x21,
        "pagedown" | "pgdn" | "next" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "insert" => 0x2d,
        "delete" => 0x2e,
        "pause" => 0x13,
        "scrolllock" => 0x91,
        "printscreen" => 0x2c,
        "escape" => 0x1b,
        "space" => 0x20,
        "tab" => 0x09,
        "applications" | "menu" => 0x5d,
        _ => {
            if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
                if (1..=24).contains(&n) {
                    return Some(0x70 + n - 1);
                }
                return None;
            }
            let mut chars = lower.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_lowercase() => c.to_ascii_uppercase() as u32,
                (Some(c), None) if c.is_ascii_digit() => c as u32,
                _ => return None,
            }
        }
    };
    Some(vk)
}

/// The modifiers in wezterm's form: `CTRL|SHIFT`, `ALT`, `SUPER`, or empty / `NONE`
pub fn modifiers(text: &str) -> Option<u32> {
    let mut mods = 0;
    for part in text.split('|').map(|p| p.trim().to_ascii_uppercase()) {
        mods |= match part.as_str() {
            "" | "NONE" => 0,
            "CTRL" | "CONTROL" => MOD_CONTROL,
            "SHIFT" => MOD_SHIFT,
            "ALT" | "OPT" | "META" => MOD_ALT,
            "SUPER" | "WIN" | "CMD" => MOD_WIN,
            _ => return None,
        };
    }
    Some(mods)
}

pub fn parse(key: &str, mods: &str) -> Result<Hotkey, String> {
    let vk = virtual_key(key).ok_or_else(|| format!("lbterm_summon: no key called {key:?}"))?;
    let mods =
        modifiers(mods).ok_or_else(|| format!("lbterm_summon: modifiers {mods:?} aren't CTRL, SHIFT, ALT or SUPER"))?;
    Ok(Hotkey {
        mods: mods | MOD_NOREPEAT,
        vk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_up_alone() {
        assert_eq!(parse("PageUp", ""), Ok(Hotkey { mods: MOD_NOREPEAT, vk: 0x21 }));
        assert_eq!(parse("pageup", "NONE"), parse("PageUp", ""));
    }

    #[test]
    fn other_keys() {
        assert_eq!(virtual_key("F12"), Some(0x7b));
        assert_eq!(virtual_key("f1"), Some(0x70));
        assert_eq!(virtual_key("a"), Some(0x41));
        assert_eq!(virtual_key("7"), Some(0x37));
        assert_eq!(virtual_key("PageDown"), Some(0x22));
        assert_eq!(virtual_key("F25"), None);
        assert_eq!(virtual_key("Hyper"), None);
    }

    #[test]
    fn modifiers_combine() {
        assert_eq!(modifiers("CTRL|SHIFT"), Some(MOD_CONTROL | MOD_SHIFT));
        assert_eq!(modifiers("SUPER"), Some(MOD_WIN));
        assert_eq!(modifiers("ctrl | alt"), Some(MOD_CONTROL | MOD_ALT));
        assert_eq!(modifiers("LEADER"), None);
        assert!(parse("PageUp", "LEADER").is_err());
        assert!(parse("Nope", "").is_err());
    }
}
