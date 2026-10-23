//! The tab menu's layout and what's under the pointer or the keyboard (the GUI draws it in the
//! tab panel's style: lbterm/TAB-MOUSE.md). A card of rows, one of which is a row of colour
//! swatches: Rename tab, Colour, Duplicate tab, Move to new window, then Close other tabs and
//! Close tab under a rule.

use crate::tabs::{Rect, COLOURS};

/// What a row (or a swatch) does
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Rename,
    /// An index into COLOURS, or None for no colour
    Colour(Option<usize>),
    Duplicate,
    NewWindow,
    CloseOthers,
    Close,
}

/// The rows top to bottom; `None` is the rule between them
pub const ROWS: [Option<Item>; 7] = [
    Some(Item::Rename),
    Some(Item::Colour(None)),
    Some(Item::Duplicate),
    Some(Item::NewWindow),
    None,
    Some(Item::CloseOthers),
    Some(Item::Close),
];

pub fn label(item: Item) -> &'static str {
    match item {
        Item::Rename => "Rename tab",
        Item::Colour(_) => "Colour",
        Item::Duplicate => "Duplicate tab",
        Item::NewWindow => "Move to new window",
        Item::CloseOthers => "Close other tabs",
        Item::Close => "Close tab",
    }
}

/// Sizes at 96 DPI
const WIDTH: f64 = 260.0;
const ROW: f64 = 30.0;
const RULE: f64 = 9.0;
const PAD: f64 = 6.0;
const SWATCH: f64 = 14.0;
const SWATCH_GAP: f64 = 6.0;
/// How far from the window's edges the card keeps
const MARGIN: f64 = 6.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub card: Rect,
    /// Each row's rectangle, with what it is (`None` for the rule)
    pub rows: Vec<(Option<Item>, Rect)>,
    /// The swatches on the Colour row: None first, then each colour
    pub swatches: Vec<(Option<usize>, Rect)>,
    /// Where the Colour row's label ends and its swatches begin
    pub label_width: f64,
    pub scale: f64,
}

/// The menu opened at `at` (the pointer, in the window) in a window `size` big: down and to the
/// right of the pointer when it fits, flipped up or left when it doesn't
pub fn layout(at: (f64, f64), size: (f64, f64), scale: f64) -> Layout {
    let width = WIDTH * scale;
    let height = PAD * 2.0 * scale
        + ROWS
            .iter()
            .map(|r| if r.is_some() { ROW } else { RULE })
            .sum::<f64>()
            * scale;
    let margin = MARGIN * scale;
    let mut x = at.0;
    let mut y = at.1;
    if x + width + margin > size.0 {
        x = (at.0 - width).max(margin);
    }
    if y + height + margin > size.1 {
        y = (at.1 - height).max(margin);
    }
    let card = Rect {
        x,
        y,
        w: width,
        h: height,
    };
    let mut rows = vec![];
    let mut top = y + PAD * scale;
    for row in ROWS {
        let h = if row.is_some() { ROW } else { RULE } * scale;
        rows.push((
            row,
            Rect {
                x: x + PAD * scale,
                y: top,
                w: width - 2.0 * PAD * scale,
                h,
            },
        ));
        top += h;
    }
    let colour_row = rows
        .iter()
        .find(|(r, _)| matches!(r, Some(Item::Colour(_))))
        .map(|(_, rect)| *rect)
        .expect("a Colour row");
    let count = COLOURS.len() + 1;
    let swatches_width = count as f64 * SWATCH * scale + (count - 1) as f64 * SWATCH_GAP * scale;
    let first = colour_row.x + colour_row.w - PAD * scale - swatches_width;
    let swatches = (0..count)
        .map(|i| {
            let colour = if i == 0 { None } else { Some(i - 1) };
            (
                colour,
                Rect {
                    x: first + i as f64 * (SWATCH + SWATCH_GAP) * scale,
                    y: colour_row.y + (colour_row.h - SWATCH * scale) / 2.0,
                    w: SWATCH * scale,
                    h: SWATCH * scale,
                },
            )
        })
        .collect();
    Layout {
        card,
        rows,
        swatches,
        label_width: first - colour_row.x,
        scale,
    }
}

impl Layout {
    /// What's under the pointer: a swatch on the Colour row, else the row
    pub fn hit(&self, x: f64, y: f64) -> Option<Item> {
        if !self.card.contains(x, y) {
            return None;
        }
        // A little slack round each swatch: they're small
        let slack = SWATCH_GAP / 2.0 * self.scale;
        for (colour, r) in &self.swatches {
            let grown = Rect {
                x: r.x - slack,
                y: r.y - slack,
                w: r.w + 2.0 * slack,
                h: r.h + 2.0 * slack,
            };
            if grown.contains(x, y) {
                return Some(Item::Colour(*colour));
            }
        }
        self.rows
            .iter()
            .find(|(row, r)| row.is_some() && r.contains(x, y))
            .and_then(|(row, _)| match row {
                // The row itself, off the swatches, does nothing
                Some(Item::Colour(_)) => None,
                other => *other,
            })
    }
}

/// The item the keyboard is on, moved by `rows` (up negative, down positive) and, on the
/// Colour row, `across` along the swatches. Skips the rule, wraps top to bottom.
pub fn step(from: Item, rows: i32, across: i32) -> Item {
    let items: Vec<Item> = ROWS.iter().flatten().copied().collect();
    let here = items
        .iter()
        .position(|i| std::mem::discriminant(i) == std::mem::discriminant(&from))
        .unwrap_or(0);
    if rows == 0 {
        if let Item::Colour(c) = from {
            let n = COLOURS.len() as i32 + 1;
            let at = c.map_or(0, |i| i as i32 + 1);
            let next = (at + across).rem_euclid(n);
            return Item::Colour(if next == 0 { None } else { Some(next as usize - 1) });
        }
        return from;
    }
    let len = items.len() as i32;
    let next = (here as i32 + rows).rem_euclid(len) as usize;
    items[next]
}

/// What Close other tabs asks before closing `count` tabs, `busy` of which are running
/// something. `known` is false when some are in a mux server, whose panes can't say.
pub fn close_others_question(count: usize, busy: usize, known: bool) -> String {
    if count == 1 {
        return if known {
            "Close the other tab? It's running something.".to_string()
        } else {
            "Close the other tab?".to_string()
        };
    }
    if !known {
        format!("Close the {count} other tabs?")
    } else if busy == count {
        format!("Close the {count} other tabs? All of them are running something.")
    } else {
        let verb = if busy == 1 { "is" } else { "are" };
        format!("Close the {count} other tabs? {busy} of them {verb} running something.")
    }
}

/// A tab's new name as it's typed. It starts as the old name, all of it selected as in
/// Windows' rename boxes: typing replaces it, Backspace clears it, and an arrow, Home or End
/// keeps it to edit from its end.
#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    pub text: String,
    pub selected: bool,
}

impl Name {
    pub fn new(old: &str) -> Self {
        Self {
            text: old.to_string(),
            selected: !old.is_empty(),
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.selected {
            self.text.clear();
            self.selected = false;
        }
        self.text.push(c);
    }

    pub fn backspace(&mut self) {
        if self.selected {
            self.text.clear();
            self.selected = false;
        } else {
            self.text.pop();
        }
    }

    pub fn deselect(&mut self) {
        self.selected = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_replaces_the_old_name() {
        let mut n = Name::new("bravo");
        assert!(n.selected);
        for c in "moved".chars() {
            n.type_char(c);
        }
        assert_eq!(n.text, "moved");
        assert!(!n.selected);
    }

    #[test]
    fn backspace_clears_the_selection_then_deletes() {
        let mut n = Name::new("bravo");
        n.backspace();
        assert_eq!(n.text, "");
        n.type_char('a');
        n.type_char('b');
        n.backspace();
        assert_eq!(n.text, "a");
    }

    #[test]
    fn an_arrow_keeps_the_old_name_to_edit() {
        let mut n = Name::new("bravo");
        n.deselect();
        n.type_char('2');
        assert_eq!(n.text, "bravo2");
        n.backspace();
        n.backspace();
        assert_eq!(n.text, "brav");
    }

    #[test]
    fn close_others_says_what_it_knows() {
        assert_eq!(
            close_others_question(4, 1, true),
            "Close the 4 other tabs? 1 of them is running something."
        );
        assert_eq!(
            close_others_question(3, 3, true),
            "Close the 3 other tabs? All of them are running something."
        );
        assert_eq!(close_others_question(1, 1, true), "Close the other tab? It's running something.");
        // In a mux server nobody knows
        assert_eq!(close_others_question(4, 4, false), "Close the 4 other tabs?");
        assert_eq!(close_others_question(1, 1, false), "Close the other tab?");
    }

    #[test]
    fn nothing_to_select_in_an_empty_name() {
        assert!(!Name::new("").selected);
    }

    #[test]
    fn opens_down_and_right_of_the_pointer() {
        let l = layout((100.0, 50.0), (1200.0, 900.0), 1.0);
        assert_eq!((l.card.x, l.card.y), (100.0, 50.0));
        assert_eq!(l.card.w, WIDTH);
        // seven rows: six items and the rule
        assert_eq!(l.rows.len(), 7);
        assert_eq!(l.card.h, 2.0 * PAD + 6.0 * ROW + RULE);
    }

    #[test]
    fn flips_to_stay_in_the_window() {
        let l = layout((1150.0, 880.0), (1200.0, 900.0), 1.0);
        assert!(l.card.x + l.card.w <= 1200.0);
        assert!(l.card.y + l.card.h <= 900.0);
        assert_eq!(l.card.x, 1150.0 - WIDTH);
    }

    #[test]
    fn rows_and_swatches_are_hit() {
        let l = layout((0.0, 0.0), (1200.0, 900.0), 1.0);
        let middle = |r: &Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = middle(&l.rows[0].1);
        assert_eq!(l.hit(x, y), Some(Item::Rename));
        let (x, y) = middle(&l.rows[6].1);
        assert_eq!(l.hit(x, y), Some(Item::Close));
        // the rule does nothing
        let (x, y) = middle(&l.rows[4].1);
        assert_eq!(l.hit(x, y), None);
        // the first swatch is "no colour", the next the first colour
        let (x, y) = middle(&l.swatches[0].1);
        assert_eq!(l.hit(x, y), Some(Item::Colour(None)));
        let (x, y) = middle(&l.swatches[1].1);
        assert_eq!(l.hit(x, y), Some(Item::Colour(Some(0))));
        // outside the card
        assert_eq!(l.hit(900.0, 800.0), None);
    }

    #[test]
    fn swatches_fit_beside_the_label() {
        let l = layout((0.0, 0.0), (1200.0, 900.0), 1.0);
        assert_eq!(l.swatches.len(), COLOURS.len() + 1);
        assert!(l.label_width > 50.0, "{}", l.label_width);
        let last = l.swatches.last().unwrap().1;
        assert!(last.x + last.w <= l.card.x + l.card.w);
    }

    #[test]
    fn the_keyboard_skips_the_rule_and_wraps() {
        assert_eq!(step(Item::Rename, 1, 0), Item::Colour(None));
        assert_eq!(step(Item::NewWindow, 1, 0), Item::CloseOthers);
        assert_eq!(step(Item::Close, 1, 0), Item::Rename);
        assert_eq!(step(Item::Rename, -1, 0), Item::Close);
        // along the swatches, wrapping
        assert_eq!(step(Item::Colour(None), 0, 1), Item::Colour(Some(0)));
        assert_eq!(step(Item::Colour(None), 0, -1), Item::Colour(Some(COLOURS.len() - 1)));
        assert_eq!(step(Item::Colour(Some(2)), 1, 0), Item::Duplicate);
    }
}
