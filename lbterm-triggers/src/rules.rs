//! Rules, and finding what they match in a line of cells

use crate::colour::Rgb;
use regex::{Regex, RegexSet};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::time::Duration;

/// How a rule colours what it matched
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Style {
    /// The text's own colour moved `amount` (0 to 1) of the way to `colour`
    Tint { colour: Rgb, amount: f32 },
    /// Red to violet across the match
    Rainbow,
}

/// What plays when a rule matches something new
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Hearts float up from the bottom-right corner of the pane
    Hearts,
}

impl Effect {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "hearts" => Some(Self::Hearts),
            _ => None,
        }
    }
}

/// A rule as the config gives it (`config.lbterm_triggers`)
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// What the rule is called in the Lua event
    pub name: String,
    /// A Rust regex (`(?i)` for any case, `\b` for a word boundary)
    pub regex: String,
    /// `#rrggbb` to tint towards
    pub tint: Option<String>,
    /// How far to tint, 0 to 1
    pub tint_amount: f32,
    pub rainbow: bool,
    /// `"hearts"`
    pub effect: Option<String>,
    /// The effect plays at most once in this long, per pane
    pub cooldown_ms: u64,
}

/// The rules LBTerm has when the config doesn't set `lbterm_triggers`: Lachlan's easter eggs.
/// "love" (a whole word, any case) goes a little pink and sends up hearts. "LB" in capitals at
/// the start of a word goes rainbow, so LBTerm's own name does and MLB doesn't.
pub fn easter_eggs() -> Vec<Spec> {
    vec![
        Spec {
            name: "love".to_string(),
            regex: r"(?i)\blove\b".to_string(),
            tint: Some("#ff7eb6".to_string()),
            tint_amount: 0.45,
            rainbow: false,
            effect: Some("hearts".to_string()),
            cooldown_ms: 4000,
        },
        Spec {
            name: "lb".to_string(),
            regex: r"\bLB".to_string(),
            tint: None,
            tint_amount: 0.0,
            rainbow: true,
            effect: None,
            cooldown_ms: 0,
        },
    ]
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub name: String,
    regex: Regex,
    pub style: Option<Style>,
    pub effect: Option<Effect>,
    pub cooldown: Duration,
}

/// One cell of a line: its column, its text and how many columns it takes (2 for wide
/// characters)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    pub col: usize,
    pub text: &'a str,
    pub width: usize,
}

/// A line's text as the rules read it, made once from its cells: their text run together, and
/// where each cell starts in it, its column and how many columns it takes. Built straight from a
/// terminal line's cells (one String for the line, where each cell's text was a String of its own)
#[derive(Debug, Clone, Default)]
pub struct LineText {
    text: String,
    /// (byte offset where the cell's text starts, its column, its width)
    cells: Vec<(usize, usize, usize)>,
}

impl LineText {
    pub fn with_capacity(cells: usize) -> Self {
        Self {
            text: String::with_capacity(cells),
            cells: Vec::with_capacity(cells),
        }
    }

    /// The next cell: its column, its text and how many columns it takes
    pub fn push(&mut self, col: usize, text: &str, width: usize) {
        self.cells.push((self.text.len(), col, width));
        self.text.push_str(text);
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// A hash of the text, so the same line seen on another row is known as the same
    /// ([`crate::Seen`]): what it says, without the blanks after it, which a terminal's line
    /// holds as many of as it was last cleared or written with (conpty's first screen and its
    /// redraws of the same rows end them differently)
    pub fn hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.text.trim_end().hash(&mut hasher);
        hasher.finish()
    }
}

/// Something a rule matched in a line
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hit {
    /// Which rule, by its place in the list
    pub rule: usize,
    /// The columns it covers
    pub cols: Range<usize>,
    pub text: String,
    /// A hash of the line's text up to the match's end: the same match in its row while what's
    /// before it is the same, as a reply streams on after it ([`crate::Seen`])
    pub prefix: u64,
}

#[derive(Debug, Clone)]
pub struct Rules {
    rules: Vec<Rule>,
    /// Every rule's regex together, to skip the lines nothing matches in one pass
    set: RegexSet,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            rules: vec![],
            set: RegexSet::empty(),
        }
    }
}

impl Rules {
    /// The rules that compile, and an error for each that doesn't
    pub fn compile(specs: &[Spec]) -> (Self, Vec<String>) {
        let mut rules = vec![];
        let mut errors = vec![];
        for spec in specs {
            let regex = match Regex::new(&spec.regex) {
                Ok(r) => r,
                Err(err) => {
                    errors.push(format!("lbterm_triggers {:?}: {err}", spec.name));
                    continue;
                }
            };
            let style = if spec.rainbow {
                Some(Style::Rainbow)
            } else if let Some(text) = &spec.tint {
                match Rgb::parse(text) {
                    Some(colour) => Some(Style::Tint {
                        colour,
                        amount: spec.tint_amount.clamp(0.0, 1.0),
                    }),
                    None => {
                        errors.push(format!(
                            "lbterm_triggers {:?}: tint {text:?} isn't #rrggbb",
                            spec.name
                        ));
                        None
                    }
                }
            } else {
                None
            };
            let effect = match &spec.effect {
                Some(name) => match Effect::parse(name) {
                    Some(e) => Some(e),
                    None => {
                        errors.push(format!(
                            "lbterm_triggers {:?}: no effect called {name:?}",
                            spec.name
                        ));
                        None
                    }
                },
                None => None,
            };
            rules.push(Rule {
                name: spec.name.clone(),
                regex,
                style,
                effect,
                cooldown: Duration::from_millis(spec.cooldown_ms),
            });
        }
        let set = RegexSet::new(rules.iter().map(|r| r.regex.as_str()))
            .expect("each regex compiled on its own");
        (Self { rules, set }, errors)
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn get(&self, rule: usize) -> &Rule {
        &self.rules[rule]
    }

    /// Whether any rule colours text, so the renderer can skip looking otherwise
    pub fn styles_anything(&self) -> bool {
        self.rules.iter().any(|r| r.style.is_some())
    }

    /// Everything the rules match in a line, in the order of the rules and then of the line
    pub fn find(&self, cells: &[Cell]) -> Vec<Hit> {
        let mut line = LineText::with_capacity(cells.len());
        for cell in cells {
            line.push(cell.col, cell.text, cell.width);
        }
        self.find_in(&line)
    }

    /// The same, of a line's text made once ([`LineText`])
    pub fn find_in(&self, line: &LineText) -> Vec<Hit> {
        if self.rules.is_empty() || line.cells.is_empty() {
            return vec![];
        }
        let text = &line.text;
        let matching = self.set.matches(text);
        if !matching.matched_any() {
            return vec![];
        }
        let cell_at = |byte: usize| match line.cells.binary_search_by_key(&byte, |c| c.0) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let mut hits = vec![];
        for rule in matching.iter() {
            for m in self.rules[rule].regex.find_iter(text) {
                if m.start() == m.end() {
                    continue;
                }
                let (_, first, _) = line.cells[cell_at(m.start())];
                let (_, last, width) = line.cells[cell_at(m.end() - 1)];
                let mut hasher = DefaultHasher::new();
                text[..m.end()].hash(&mut hasher);
                hits.push(Hit {
                    rule,
                    cols: first..last + width.max(1),
                    text: m.as_str().to_string(),
                    prefix: hasher.finish(),
                });
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(text: &str) -> Vec<Cell<'_>> {
        text.char_indices()
            .enumerate()
            .map(|(col, (i, c))| Cell {
                col,
                text: &text[i..i + c.len_utf8()],
                width: 1,
            })
            .collect()
    }

    fn eggs() -> Rules {
        let (rules, errors) = Rules::compile(&easter_eggs());
        assert!(errors.is_empty(), "{errors:?}");
        rules
    }

    fn found(rules: &Rules, text: &str) -> Vec<(String, Range<usize>)> {
        rules
            .find(&cells(text))
            .into_iter()
            .map(|h| (rules.get(h.rule).name.clone(), h.cols))
            .collect()
    }

    #[test]
    fn love_as_a_word_in_any_case() {
        let r = eggs();
        assert_eq!(found(&r, "I love it"), vec![("love".to_string(), 2..6)]);
        assert_eq!(found(&r, "LOVE, Love."), vec![
            ("love".to_string(), 0..4),
            ("love".to_string(), 6..10)
        ]);
        assert!(found(&r, "glove lovely beloved").is_empty());
    }

    #[test]
    fn lb_in_capitals_at_the_start_of_a_word() {
        let r = eggs();
        assert_eq!(found(&r, r"cd C:\Code\LBTerm"), vec![("lb".to_string(), 11..13)]);
        assert_eq!(found(&r, "LB"), vec![("lb".to_string(), 0..2)]);
        assert!(found(&r, "MLB lb Lb LachlanB").is_empty());
    }

    #[test]
    fn the_easter_eggs_style_and_play() {
        let r = eggs();
        assert!(matches!(r.get(0).style, Some(Style::Tint { amount, .. }) if amount > 0.0 && amount < 1.0));
        assert_eq!(r.get(0).effect, Some(Effect::Hearts));
        assert_eq!(r.get(1).style, Some(Style::Rainbow));
        assert_eq!(r.get(1).effect, None);
    }

    #[test]
    fn wide_characters_count_their_columns() {
        let r = eggs();
        // 漢 takes columns 0-1, the space 2, love 3-6
        let line = vec![
            Cell { col: 0, text: "漢", width: 2 },
            Cell { col: 2, text: " ", width: 1 },
            Cell { col: 3, text: "l", width: 1 },
            Cell { col: 4, text: "o", width: 1 },
            Cell { col: 5, text: "v", width: 1 },
            Cell { col: 6, text: "e", width: 1 },
        ];
        let hits = r.find(&line);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].cols, 3..7);
        assert_eq!(hits[0].text, "love");
    }

    #[test]
    fn bad_rules_are_reported_and_left_out() {
        let mut specs = easter_eggs();
        specs.push(Spec {
            name: "broken".into(),
            regex: "(".into(),
            tint: None,
            tint_amount: 0.0,
            rainbow: false,
            effect: None,
            cooldown_ms: 0,
        });
        specs.push(Spec {
            name: "odd".into(),
            regex: "x".into(),
            tint: Some("pink".into()),
            tint_amount: 0.5,
            rainbow: false,
            effect: Some("fireworks".into()),
            cooldown_ms: 0,
        });
        let (rules, errors) = Rules::compile(&specs);
        assert_eq!(errors.len(), 3, "{errors:?}");
        // the regex that compiled is kept, without the style and effect it got wrong
        assert_eq!(found(&rules, "x"), vec![("odd".to_string(), 0..1)]);
        assert_eq!(rules.get(2).style, None);
    }

    #[test]
    fn a_line_made_once_finds_what_its_cells_do() {
        let r = eggs();
        let text = "I love LBTerm";
        let mut line = LineText::default();
        for c in cells(text) {
            line.push(c.col, c.text, c.width);
        }
        assert_eq!(line.text(), text);
        assert_eq!(r.find_in(&line), r.find(&cells(text)));
        // The same text, the same hash; other text, another
        let mut again = LineText::default();
        for c in cells(text) {
            again.push(c.col, c.text, c.width);
        }
        assert_eq!(line.hash(), again.hash());
        let mut other = LineText::default();
        other.push(0, "x", 1);
        assert_ne!(line.hash(), other.hash());
        // The blanks after the text are no part of it
        let mut padded = again.clone();
        padded.push(13, " ", 1);
        padded.push(14, " ", 1);
        assert_eq!(padded.hash(), line.hash());
        // A wide cell's columns, from where it starts in the text
        let mut wide = LineText::default();
        wide.push(0, "漢", 2);
        wide.push(2, " ", 1);
        for (i, ch) in "love".chars().enumerate() {
            wide.push(3 + i, &ch.to_string(), 1);
        }
        assert_eq!(r.find_in(&wide)[0].cols, 3..7);
    }

    #[test]
    fn no_rules_match_nothing() {
        let (rules, _) = Rules::compile(&[]);
        assert!(rules.find(&cells("love LB")).is_empty());
        assert!(!rules.styles_anything());
    }
}
