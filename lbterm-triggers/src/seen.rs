//! Which matches are new, so an effect plays and Lua hears about each occurrence once
//!
//! The pane is looked at every time it draws, and Claude Code redraws its lines many times a
//! second, so a match is new only when that rule hasn't matched that text at that place (row
//! and column) for a while. A place that stays on screen stays seen however long it's there;
//! one that's gone for `forget_after` (scrolled away, cleared) is new again when it comes back.

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

/// One pane's memory of what its rules have matched
#[derive(Debug)]
pub struct Seen {
    /// (row, rule, first column, text) to when it was last seen
    places: HashMap<(Row, usize, usize, String), Instant>,
    /// rule to when its effect last played
    played: HashMap<usize, Instant>,
    forget_after: Duration,
}

impl Default for Seen {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

impl Seen {
    pub fn new(forget_after: Duration) -> Self {
        Self {
            places: HashMap::new(),
            played: HashMap::new(),
            forget_after,
        }
    }

    /// What's new among the matches in `row`, looked at `now`
    pub fn look(&mut self, rules: &Rules, row: Row, hits: &[Hit], now: Instant) -> Vec<Fire> {
        let mut fires = vec![];
        for hit in hits {
            let key = (row, hit.rule, hit.cols.start, hit.text.clone());
            let new = match self.places.get(&key) {
                Some(&last) => now.saturating_duration_since(last) > self.forget_after,
                None => true,
            };
            self.places.insert(key, now);
            if !new {
                continue;
            }
            let rule = rules.get(hit.rule);
            let effect = match rule.effect {
                Some(effect) => {
                    let rested = match self.played.get(&hit.rule) {
                        Some(&last) => now.saturating_duration_since(last) >= rule.cooldown,
                        None => true,
                    };
                    if rested {
                        self.played.insert(hit.rule, now);
                        Some(effect)
                    } else {
                        None
                    }
                }
                None => None,
            };
            fires.push(Fire {
                rule: hit.rule,
                text: hit.text.clone(),
                effect,
            });
        }
        fires
    }

    /// Drop places not seen for `forget_after`, so the memory doesn't grow with the scrollback
    pub fn tidy(&mut self, now: Instant) {
        let forget_after = self.forget_after;
        self.places
            .retain(|_, last| now.saturating_duration_since(*last) <= forget_after);
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
        }
    }

    fn lb(col: usize) -> Hit {
        Hit {
            rule: 1,
            cols: col..col + 2,
            text: "LB".into(),
        }
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_new_match_fires_once_however_often_it_is_redrawn() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        let first = seen.look(&rules, 5, &[love(2)], t);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].effect, Some(Effect::Hearts));
        for i in 1..50 {
            assert!(seen.look(&rules, 5, &[love(2)], t + MS * (100 * i)).is_empty());
        }
    }

    #[test]
    fn the_effect_rests_for_its_cooldown_but_lua_still_hears() {
        let rules = eggs();
        let mut seen = Seen::default();
        let t = Instant::now();
        assert_eq!(seen.look(&rules, 5, &[love(2)], t)[0].effect, Some(Effect::Hearts));
        // another love elsewhere a second later: new, but the hearts are resting
        let second = seen.look(&rules, 9, &[love(0)], t + MS * 1000);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].effect, None);
        // after the 4 s cooldown they play again
        let third = seen.look(&rules, 12, &[love(0)], t + MS * 4500);
        assert_eq!(third[0].effect, Some(Effect::Hearts));
    }

    #[test]
    fn a_rule_without_an_effect_still_fires_for_lua() {
        let rules = eggs();
        let mut seen = Seen::default();
        let fires = seen.look(&rules, 0, &[lb(3)], Instant::now());
        assert_eq!(fires, vec![Fire { rule: 1, text: "LB".into(), effect: None }]);
    }

    #[test]
    fn a_place_gone_for_a_while_is_new_when_it_comes_back() {
        let rules = eggs();
        let mut seen = Seen::new(Duration::from_secs(10));
        let t = Instant::now();
        seen.look(&rules, 5, &[love(2)], t);
        // not drawn for 11 s (scrolled away), then back
        let back = seen.look(&rules, 5, &[love(2)], t + MS * 11_000);
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn tidy_forgets_places_not_seen_lately() {
        let rules = eggs();
        let mut seen = Seen::new(Duration::from_secs(10));
        let t = Instant::now();
        seen.look(&rules, 5, &[love(2)], t);
        seen.look(&rules, 6, &[lb(0)], t + MS * 9000);
        seen.tidy(t + MS * 12_000);
        assert_eq!(seen.len(), 1);
    }
}
