//! The mic mark (lbterm/CONTROL.md, "The mic mark"). Lachlan's yes (2026-10-25): while Earshot
//! listens for LBTerm, a mic on the window strip says so, left of its buttons: dim while it
//! listens, lit while it hears him speak, struck through while it's paused. Earshot tells LBTerm
//! its state over the control pipe (`mic`) as it changes, and again every so often while it
//! listens; each state holds for its `ttl` and then goes, so a listener that stopped without
//! saying so doesn't leave a mic behind.

use std::time::{Duration, Instant};

/// How long a state holds unless it's said again, and the longest it can be asked to
pub const TTL: Duration = Duration::from_secs(30);
pub const TTL_MAX: Duration = Duration::from_secs(600);
/// The longest `heard` shown on the strip, in characters
pub const MAX_HEARD: usize = 200;

/// The mic's glyphs, in the Symbols Nerd Font every LBTerm falls back to: a mic, and one struck
/// through
pub const GLYPH: char = '\u{f130}';
pub const GLYPH_MUTED: char = '\u{f131}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicState {
    /// Listening for him, nothing heard yet
    Listening,
    /// Hearing him speak now
    Hearing,
    /// Paused: on a phone call, in Q&A, or muted
    Muted,
}

impl MicState {
    /// Its name on the pipe: `off` is no state at all
    pub fn parse(name: &str) -> Result<Option<Self>, String> {
        match name {
            "off" => Ok(None),
            "listening" => Ok(Some(Self::Listening)),
            "hearing" => Ok(Some(Self::Hearing)),
            "muted" => Ok(Some(Self::Muted)),
            other => Err(format!("the mic is off, listening, hearing or muted, not {other:?}")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Listening => "listening",
            Self::Hearing => "hearing",
            Self::Muted => "muted",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Self::Muted => GLYPH_MUTED,
            _ => GLYPH,
        }
    }
}

/// A `ttl` in seconds as asked: none is [`TTL`], and it's held to [`TTL_MAX`]
pub fn ttl(seconds: Option<f64>) -> Duration {
    match seconds {
        Some(s) if s.is_finite() && s > 0.0 => Duration::from_secs_f64(s).min(TTL_MAX),
        _ => TTL,
    }
}

/// What the mark shows, and until when
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mic {
    state: Option<(MicState, Instant)>,
}

impl Mic {
    /// Off
    pub const fn new() -> Self {
        Self { state: None }
    }

    /// `state` from `now`, holding for `ttl`; None turns it off
    pub fn set(&mut self, state: Option<MicState>, ttl: Duration, now: Instant) {
        self.state = state.map(|s| (s, now + ttl));
    }

    /// What it shows at `now`: nothing once its time's run out
    pub fn shown(&self, now: Instant) -> Option<MicState> {
        self.state.filter(|(_, until)| now < *until).map(|(s, _)| s)
    }

    /// When what it shows runs out, for a repaint then
    pub fn until(&self) -> Option<Instant> {
        self.state.map(|(_, until)| until)
    }
}

/// What Earshot heard, for the strip: one line, cut to [`MAX_HEARD`] characters
pub fn heard(text: &str) -> Option<String> {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= MAX_HEARD {
        return Some(line);
    }
    let mut cut: String = line.chars().take(MAX_HEARD - 1).collect();
    cut.push('\u{2026}');
    Some(cut)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn its_states_are_read_by_name() {
        assert_eq!(MicState::parse("listening"), Ok(Some(MicState::Listening)));
        assert_eq!(MicState::parse("hearing"), Ok(Some(MicState::Hearing)));
        assert_eq!(MicState::parse("muted"), Ok(Some(MicState::Muted)));
        assert_eq!(MicState::parse("off"), Ok(None));
        assert!(MicState::parse("loud").is_err());
        for s in [MicState::Listening, MicState::Hearing, MicState::Muted] {
            assert_eq!(MicState::parse(s.name()), Ok(Some(s)));
        }
        assert_eq!(MicState::Muted.glyph(), GLYPH_MUTED);
        assert_eq!(MicState::Hearing.glyph(), GLYPH);
    }

    #[test]
    fn a_state_holds_for_its_ttl_and_then_goes() {
        let now = Instant::now();
        let mut mic = Mic::default();
        assert_eq!(mic.shown(now), None);
        mic.set(Some(MicState::Listening), ttl(Some(10.0)), now);
        assert_eq!(mic.shown(now + Duration::from_secs(9)), Some(MicState::Listening));
        assert_eq!(mic.shown(now + Duration::from_secs(10)), None, "a listener that stopped leaves no mic");
        assert_eq!(mic.until(), Some(now + Duration::from_secs(10)));
        // Said again, it holds again
        mic.set(Some(MicState::Hearing), ttl(None), now + Duration::from_secs(9));
        assert_eq!(mic.shown(now + Duration::from_secs(20)), Some(MicState::Hearing));
        mic.set(None, TTL, now);
        assert_eq!((mic.shown(now), mic.until()), (None, None));
    }

    #[test]
    fn ttls_are_held_in_bounds() {
        assert_eq!(ttl(None), TTL);
        assert_eq!(ttl(Some(0.0)), TTL);
        assert_eq!(ttl(Some(-3.0)), TTL);
        assert_eq!(ttl(Some(f64::NAN)), TTL);
        assert_eq!(ttl(Some(2.5)), Duration::from_millis(2500));
        assert_eq!(ttl(Some(1e9)), TTL_MAX);
    }

    #[test]
    fn what_was_heard_is_one_line_cut_short() {
        assert_eq!(heard("  typed 1a\n2. yes "), Some("typed 1a 2. yes".to_string()));
        assert_eq!(heard(" \n "), None);
        let long = heard(&"word ".repeat(100)).unwrap();
        assert_eq!(long.chars().count(), MAX_HEARD);
        assert!(long.ends_with('\u{2026}'));
    }
}
