//! Which matches are new, so an effect plays and Lua hears about each occurrence once
//!
//! A pane's lines are matched when they change, not each time they're drawn (the GUI keeps what
//! each line matched until it does), and told here as they come onto the screen ([`Seen::came`])
//! and go from it ([`Seen::went`]); what's new is worked out once the frame has drawn them all
//! ([`Seen::settle`]). Each match is in two places: its line (that text, that rule, that column)
//! and its row (the text before it in that row, the rule, the column). A place counts how many of
//! the pane's lines on the screen have it, and a match is new when, over the frame, the count of
//! its line and the count of its row both went up. A line that moves to another row, as Claude
//! Code's fullscreen view scrolls, keeps its line's count (one row has it in place of another);
//! one in a row whose line goes on after it, as a reply streams into its row, keeps its row's;
//! and a line printed again while the first is still on the screen raises both, so `echo love`
//! twice plays twice. A place stays seen for as long as it's on the screen, however long that is
//! and however seldom the window paints, and for `forget_after` once it's gone (its line changed
//! or was cleared, or its row went out of view): one that comes back within that is the same, so
//! a screen drawn again from nothing doesn't play again, and one that comes back after is new.
//!
//! Until 2026-10-31 a place was seen when it had been looked at in the last `forget_after`, and
//! each of a pane's lines with a match was looked at on every frame: a match on the screen while
//! the window painted nothing for that long (as a window at rest doesn't) was new again at its
//! next frame, and one moving a row with its line was new at each row (lbterm/PERF.md, finding
//! 62).

use crate::rules::{Effect, Hit, Rules};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A row in a pane, stable as it scrolls (wezterm's StableRowIndex)
pub type Row = isize;

/// Something to do about a new match
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fire {
    pub rule: usize,
    pub text: String,
    /// The rule's effect, unless it played within its cooldown
    pub effect: Option<Effect>,
}

/// Where a match is: in a row after some text (the hash of the line up to its end,
/// `Hit::prefix`), or in a line of some text (its hash, `LineText::hash`). Either says what it
/// matched too, so a place is this, the rule and the column: nothing a key copies
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Where {
    Row(Row, u64),
    Line(u64),
}

type Key = (Where, usize, usize);

/// A place: how many of the pane's lines on the screen have it, and when each of the others
/// that had it went (newest last), kept for `forget_after`
#[derive(Debug, Default)]
struct Place {
    shown: u32,
    gone: Vec<Instant>,
}

/// A match that came this frame, and its two places
#[derive(Debug)]
struct Came {
    line: Key,
    row: Key,
    rule: usize,
    text: String,
}

/// One pane's memory of what its rules have matched
#[derive(Debug)]
pub struct Seen {
    places: HashMap<Key, Place>,
    /// This frame's: how each place's count moved, and the matches that came, in order
    moved: HashMap<Key, i32>,
    came: Vec<Came>,
    /// rule to when its effect last played
    played: HashMap<usize, Instant>,
    forget_after: Duration,
}

impl Default for Seen {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

fn line_key(line: u64, hit: &Hit) -> Key {
    (Where::Line(line), hit.rule, hit.cols.start)
}

fn row_key(row: Row, hit: &Hit) -> Key {
    (Where::Row(row, hit.prefix), hit.rule, hit.cols.start)
}

impl Seen {
    pub fn new(forget_after: Duration) -> Self {
        Self {
            places: HashMap::new(),
            moved: HashMap::new(),
            came: vec![],
            played: HashMap::new(),
            forget_after,
        }
    }

    /// A line's matches as it comes onto the screen with them (drawn the first time, or
    /// changed): `row` its row, `line` its text's hash. They stay on the screen until
    /// [`Self::went`] says the line has gone; which are new is [`Self::settle`]'s, at the frame's
    /// end
    pub fn came(&mut self, row: Row, line: u64, hits: &[Hit]) {
        for hit in hits {
            let (line, row) = (line_key(line, hit), row_key(row, hit));
            *self.moved.entry(line).or_default() += 1;
            *self.moved.entry(row).or_default() += 1;
            self.came.push(Came {
                line,
                row,
                rule: hit.rule,
                text: hit.text.clone(),
            });
        }
    }

    /// A line's matches gone from the screen: the line changed or was cleared, or its row went
    /// out of view. The same row, line and hits as it [`Self::came`] with
    pub fn went(&mut self, row: Row, line: u64, hits: &[Hit]) {
        for hit in hits {
            *self.moved.entry(line_key(line, hit)).or_default() -= 1;
            *self.moved.entry(row_key(row, hit)).or_default() -= 1;
        }
    }

    /// Whether anything came or went since the last [`Self::settle`]
    pub fn unsettled(&self) -> bool {
        !self.moved.is_empty()
    }

    /// The frame's end, at `now`: each place's count moved by what came and went, and what's
    /// new fires, in the order it came. A place whose count went up takes back as many of the
    /// lines that had it and went within `forget_after` as it can, and what's left over is new;
    /// a match is new when both its places have something new left, and takes one of each
    pub fn settle(&mut self, rules: &Rules, now: Instant) -> Vec<Fire> {
        let forget_after = self.forget_after;
        let mut new: HashMap<Key, u32> = HashMap::new();
        for (key, by) in self.moved.drain() {
            if by == 0 {
                continue;
            }
            let place = self.places.entry(key).or_default();
            place.gone.retain(|at| now.saturating_duration_since(*at) <= forget_after);
            if by > 0 {
                let by = by as u32;
                let back = by.min(place.gone.len() as u32);
                place.gone.truncate(place.gone.len() - back as usize);
                place.shown += by;
                if by > back {
                    new.insert(key, by - back);
                }
            } else {
                let by = by.unsigned_abs().min(place.shown);
                place.shown -= by;
                place.gone.extend(std::iter::repeat(now).take(by as usize));
            }
        }
        let mut fires = vec![];
        for came in std::mem::take(&mut self.came) {
            let left = |key: &Key| new.get(key).copied().unwrap_or(0);
            if left(&came.line) == 0 || left(&came.row) == 0 {
                continue;
            }
            for key in [came.line, came.row] {
                if let Some(n) = new.get_mut(&key) {
                    *n -= 1;
                }
            }
            let rule = rules.get(came.rule);
            let effect = match rule.effect {
                Some(effect) => {
                    let rested = match self.played.get(&came.rule) {
                        Some(&last) => now.saturating_duration_since(last) >= rule.cooldown,
                        None => true,
                    };
                    if rested {
                        self.played.insert(came.rule, now);
                        Some(effect)
                    } else {
                        None
                    }
                }
                None => None,
            };
            fires.push(Fire {
                rule: came.rule,
                text: came.text,
                effect,
            });
        }
        fires
    }

    /// Drop places gone for `forget_after`, so the memory doesn't grow with the scrollback
    pub fn tidy(&mut self, now: Instant) {
        let forget_after = self.forget_after;
        self.places.retain(|_, place| {
            place.gone.retain(|at| now.saturating_duration_since(*at) <= forget_after);
            place.shown > 0 || !place.gone.is_empty()
        });
    }

    pub fn len(&self) -> usize {
        self.places.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{easter_eggs, Rules};

    fn eggs() -> Rules {
        Rules::compile(&easter_eggs()).0
    }

    fn love(col: usize) -> Hit {
        Hit {
            rule: 0,
            cols: col..col + 4,
            text: "love".into(),
            prefix: col as u64,
        }
    }

    fn lb(col: usize) -> Hit {
        Hit {
            rule: 1,
            cols: col..col + 2,
            text: "LB".into(),
            prefix: 100 + col as u64,
        }
    }

    const MS: Duration = Duration::from_millis(1);
    const HOUR: Duration = Duration::from_secs(3600);

    /// One frame: what `draw` comes and goes with, settled at `now`
    fn frame(seen: &mut Seen, rules: &Rules, now: Instant, draw: impl FnOnce(&mut Seen)) -> Vec<Fire> {
        draw(seen);
        seen.settle(rules, now)
    }

    #[test]
    fn a_new_match_fires_once_however_long_it_stays() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        let first = frame(&mut seen, &rules, t, |s| s.came(5, 1, &[love(2)]));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].effect, Some(Effect::Hearts));
        // An hour on the screen with nothing painted, then its line drawn again the same: its
        // place never went
        let again = frame(&mut seen, &rules, t + HOUR, |s| {
            s.went(5, 1, &[love(2)]);
            s.came(5, 1, &[love(2)]);
        });
        assert!(again.is_empty());
        // And a frame with nothing to settle settles nothing
        assert!(!seen.unsettled());
        assert!(seen.settle(&rules, t + HOUR).is_empty());
    }

    #[test]
    fn the_same_line_printed_again_while_the_first_shows_is_new() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        // `echo love`, then `echo love` again: rows 10 and 12, the same text
        assert_eq!(frame(&mut seen, &rules, t, |s| s.came(10, 7, &[love(0)])).len(), 1);
        let second = frame(&mut seen, &rules, t + MS * 5000, |s| s.came(12, 7, &[love(0)]));
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].effect, Some(Effect::Hearts));
        // Within the cooldown Lua still hears of a third, though the hearts rest
        let third = frame(&mut seen, &rules, t + MS * 6000, |s| s.came(14, 7, &[love(0)]));
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].effect, None);
        // Two printed in one frame are two
        let both = frame(&mut seen, &rules, t + MS * 7000, |s| {
            s.came(16, 7, &[love(0)]);
            s.came(17, 7, &[love(0)]);
        });
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn the_effect_rests_for_its_cooldown_but_lua_still_hears() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        let first = frame(&mut seen, &rules, t, |s| s.came(5, 1, &[love(2)]));
        assert_eq!(first[0].effect, Some(Effect::Hearts));
        // another love elsewhere a second later: new, but the hearts are resting
        let second = frame(&mut seen, &rules, t + MS * 1000, |s| s.came(9, 2, &[love(0)]));
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].effect, None);
        // after the 4 s cooldown they play again
        let third = frame(&mut seen, &rules, t + MS * 4500, |s| s.came(12, 3, &[love(0)]));
        assert_eq!(third[0].effect, Some(Effect::Hearts));
    }

    #[test]
    fn a_rule_without_an_effect_still_fires_for_lua() {
        let rules = eggs();
        let mut seen = Seen::default();
        let fires = frame(&mut seen, &rules, Instant::now(), |s| s.came(0, 1, &[lb(3)]));
        assert_eq!(fires, vec![Fire { rule: 1, text: "LB".into(), effect: None }]);
    }

    #[test]
    fn a_place_gone_for_a_while_is_new_when_it_comes_back() {
        let rules = eggs();
        let mut seen = Seen::new(Duration::from_secs(10));
        let t = Instant::now();
        frame(&mut seen, &rules, t, |s| s.came(5, 1, &[love(2)]));
        // scrolled away at 1 s; back at 3 s is the same, back at 15 s new
        frame(&mut seen, &rules, t + MS * 1000, |s| s.went(5, 1, &[love(2)]));
        assert!(frame(&mut seen, &rules, t + MS * 3000, |s| s.came(5, 1, &[love(2)])).is_empty());
        frame(&mut seen, &rules, t + MS * 4000, |s| s.went(5, 1, &[love(2)]));
        assert_eq!(frame(&mut seen, &rules, t + MS * 15_000, |s| s.came(5, 1, &[love(2)])).len(), 1);
    }

    #[test]
    fn a_screen_cleared_and_drawn_again_is_the_same() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        frame(&mut seen, &rules, t, |s| {
            s.came(3, 1, &[love(0)]);
            s.came(4, 1, &[love(0)]);
        });
        // A full-screen program clears, a frame shows nothing, then it draws the same again
        frame(&mut seen, &rules, t + MS * 100, |s| {
            s.went(3, 1, &[love(0)]);
            s.went(4, 1, &[love(0)]);
        });
        let back = frame(&mut seen, &rules, t + MS * 150, |s| {
            s.came(3, 1, &[love(0)]);
            s.came(4, 1, &[love(0)]);
        });
        assert!(back.is_empty());
    }

    #[test]
    fn a_line_moving_rows_with_its_text_is_the_same_line() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        frame(&mut seen, &rules, t, |s| s.came(20, 7, &[lb(4)]));
        // A minute later the view scrolls a row: row 19 gets the line while row 20 still has it,
        // then row 20 gets the next, a line of other text, which is new
        let later = t + Duration::from_secs(60);
        let scrolled = frame(&mut seen, &rules, later, |s| {
            s.came(19, 7, &[lb(4)]);
            s.went(20, 7, &[lb(4)]);
            s.came(20, 8, &[lb(6)]);
        });
        assert_eq!(scrolled.len(), 1);
        // And again, a row that gives its line up first
        let again = frame(&mut seen, &rules, later + MS * 50, |s| {
            s.went(19, 7, &[lb(4)]);
            s.came(18, 7, &[lb(4)]);
            s.went(20, 8, &[lb(6)]);
            s.came(19, 8, &[lb(6)]);
        });
        assert!(again.is_empty());
    }

    #[test]
    fn a_line_changing_in_its_row_keeps_what_it_matched() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        // "I love" streams on into "I love it" in the same row, in one frame or over two
        assert_eq!(frame(&mut seen, &rules, t, |s| s.came(3, 1, &[love(2)])).len(), 1);
        let on = frame(&mut seen, &rules, t + MS * 40, |s| {
            s.went(3, 1, &[love(2)]);
            s.came(3, 2, &[love(2)]);
        });
        assert!(on.is_empty());
        frame(&mut seen, &rules, t + MS * 80, |s| s.went(3, 2, &[love(2)]));
        assert!(frame(&mut seen, &rules, t + MS * 120, |s| s.came(3, 3, &[love(2)])).is_empty());
    }

    #[test]
    fn another_line_with_a_match_where_one_was_is_new() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        assert_eq!(frame(&mut seen, &rules, t, |s| s.came(19, 1, &[lb(4)])).len(), 1);
        // The next line scrolled into the bottom row has LB at the same column, after other text
        let other = Hit { prefix: 7, ..lb(4) };
        let next = frame(&mut seen, &rules, t + MS * 50, |s| {
            s.went(19, 1, &[lb(4)]);
            s.came(19, 2, &[other]);
        });
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn a_line_on_two_rows_is_two_and_one_back_soon_is_the_same() {
        let rules = eggs();
        let mut seen = Seen::new(Duration::from_secs(10));
        let t = Instant::now();
        let both = frame(&mut seen, &rules, t, |s| {
            s.came(1, 9, &[lb(0)]);
            s.came(2, 9, &[lb(0)]);
        });
        assert_eq!(both.len(), 2);
        frame(&mut seen, &rules, t, |s| s.went(1, 9, &[lb(0)]));
        // Its row back two seconds later is the same; another row with it a minute later is new
        assert!(frame(&mut seen, &rules, t + MS * 2000, |s| s.came(1, 9, &[lb(0)])).is_empty());
        frame(&mut seen, &rules, t + MS * 3000, |s| s.went(1, 9, &[lb(0)]));
        let later = frame(&mut seen, &rules, t + Duration::from_secs(60), |s| s.came(3, 9, &[lb(0)]));
        assert_eq!(later.len(), 1);
    }

    #[test]
    fn tidy_forgets_places_gone_a_while_and_keeps_the_shown() {
        let rules = eggs();
        let mut seen = Seen::new(Duration::from_secs(10));
        let t = Instant::now();
        frame(&mut seen, &rules, t, |s| {
            s.came(5, 1, &[love(2)]);
            s.came(6, 2, &[lb(0)]);
        });
        frame(&mut seen, &rules, t + MS * 1000, |s| s.went(5, 1, &[love(2)]));
        seen.tidy(t + HOUR);
        // The LB still shown, its row and its line
        assert_eq!(seen.len(), 2);
    }
}
