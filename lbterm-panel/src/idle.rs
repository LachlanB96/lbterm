//! The three-bar idle timer under each pad, as APCommand's Terminal mode
//! and TabPanel.exe (`IdleTimer` in its Rows.cs) draw it.

/// One bar of the timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bar {
    Off,
    Green,
    Yellow,
    Red,
}

impl Bar {
    /// Its colour's name, as test windows' state files say it
    pub fn name(self) -> &'static str {
        match self {
            Bar::Off => "off",
            Bar::Green => "green",
            Bar::Yellow => "yellow",
            Bar::Red => "red",
        }
    }
}

/// Timer steps lit for an idle age: 1 the moment Claude stops, 9 (all red)
/// at `full` seconds.
pub fn level(age: f64, full: f64) -> u8 {
    let step = (age.max(0.0) / (full / 8.0)) as u64;
    (1 + step).min(9) as u8
}

/// Bar 0-2 (bottom first) at a level 1-9: green fills the three bars,
/// yellow replaces them bar by bar from the bottom, then red.
pub fn bar(level: u8, bar: usize) -> Bar {
    let level = level.clamp(1, 9) as usize;
    let (stage, fill) = ((level - 1) / 3, (level - 1) % 3);
    let colour = |stage: usize| match stage {
        0 => Bar::Green,
        1 => Bar::Yellow,
        _ => Bar::Red,
    };
    if bar <= fill {
        colour(stage)
    } else if stage > 0 {
        colour(stage - 1)
    } else {
        Bar::Off
    }
}

/// All three bars at `level`, bottom first; all off at level 0 (working,
/// or no Claude).
pub fn bars(level: u8) -> [Bar; 3] {
    if level == 0 {
        [Bar::Off; 3]
    } else {
        [0, 1, 2].map(|b| bar(level, b))
    }
}

/// The bars to draw, bottom first, each with its place (0 the bottom): only those that are lit
/// (Lachlan's 12a of 2026-11-01, lbterm/NEW-TAB.md, "Landing 4"). An unlit bar is drawn as
/// nothing, so a pad at work or without Claude has none over it, where each had three dim
/// stripes saying nothing. The bars light from the bottom, so what's drawn is a stack.
pub fn lit(bars: [Bar; 3]) -> impl Iterator<Item = (usize, Bar)> {
    bars.into_iter().enumerate().filter(|&(_, bar)| bar != Bar::Off)
}

/// How many seconds after `age` the timer next changes step, for waking
/// the panel; `None` once it's full.
pub fn next_step_in(age: f64, full: f64) -> Option<f64> {
    if level(age, full) >= 9 {
        return None;
    }
    let step = full / 8.0;
    let next = ((age.max(0.0) / step).floor() + 1.0) * step;
    Some(next - age)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn level_climbs_one_step_per_eighth() {
        let full = 3600.0;
        assert_eq!(level(0.0, full), 1);
        assert_eq!(level(449.0, full), 1);
        assert_eq!(level(450.0, full), 2);
        assert_eq!(level(3599.0, full), 8);
        assert_eq!(level(3600.0, full), 9);
        assert_eq!(level(99_999.0, full), 9);
        assert_eq!(level(-5.0, full), 1, "a clock skew isn't a negative age");
    }

    #[test]
    fn bars_go_green_then_yellow_then_red_from_the_bottom() {
        use Bar::*;
        assert_eq!(bars(1), [Green, Off, Off]);
        assert_eq!(bars(3), [Green, Green, Green]);
        assert_eq!(bars(4), [Yellow, Green, Green]);
        assert_eq!(bars(6), [Yellow, Yellow, Yellow]);
        assert_eq!(bars(7), [Red, Yellow, Yellow]);
        assert_eq!(bars(9), [Red, Red, Red]);
        assert_eq!(bars(0), [Off, Off, Off]);
    }

    #[test]
    fn only_lit_bars_are_drawn_none_at_rest() {
        use Bar::*;
        let drawn = |level: u8| lit(bars(level)).collect::<Vec<_>>();
        // At work, or no Claude: nothing over the pad
        assert_eq!(drawn(0), vec![]);
        // The moment Claude stops, the bottom bar alone; then up the stack
        assert_eq!(drawn(1), vec![(0, Green)]);
        assert_eq!(drawn(2), vec![(0, Green), (1, Green)]);
        assert_eq!(drawn(3), vec![(0, Green), (1, Green), (2, Green)]);
        // From level 3 every bar is lit, whatever its colour
        for level in 3..=9 {
            assert_eq!(drawn(level).len(), 3, "at level {level}");
            assert_eq!(drawn(level), bars(level).into_iter().enumerate().collect::<Vec<_>>());
        }
        assert_eq!(drawn(7), vec![(0, Red), (1, Yellow), (2, Yellow)]);
        // Each in its own place, never moved down to fill a gap
        assert_eq!(lit([Off, Green, Off]).collect::<Vec<_>>(), vec![(1, Green)]);
    }

    #[test]
    fn the_next_step_is_when_the_level_changes() {
        let full = 800.0;
        assert_eq!(next_step_in(0.0, full), Some(100.0));
        assert_eq!(next_step_in(250.0, full), Some(50.0));
        assert_eq!(level(250.0 + 50.0, full), level(250.0, full) + 1);
        assert_eq!(next_step_in(800.0, full), None);
    }
}
