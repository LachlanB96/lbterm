//! LBTerm: the links a mux client's pane found in its lines, kept by each line's text, so a line
//! that only moved isn't matched against the rules again (lbterm/MUX.md, "Links in a line that
//! moved").
//!
//! The server never scans a line for links: the client does, with the window's rules (WezTerm's
//! six and Lachlan's two, fancy_regex each, 35 to 80 µs a line), once a line it holds (renderable.rs,
//! scan_rows). But the server sends a row again whenever its line changes or moves, and a scroll
//! moves every row under it: Claude Code's fullscreen view moves its whole conversation up a line
//! as it grows, so every frame of it brought the client every row of the view as new lines, each
//! matched again though only the bottom one was new (perf/mux-client's busy-scroll budget: the
//! frame's pass 3.5 to 6 ms on the branch against 1.1 to 2.3 on main, which had matched them as
//! they came, in the push's task, at the same cost). A pane of the window's own keeps its lines
//! when they move, and their scan with them. Here a line's matches are kept by its text, for the
//! lines the pane has drawn in its last two frames' worth of rows (its rows now, and those it
//! showed before a scroll moved them), and a line whose text was matched is given those matches.
//! The links a match makes are the same whatever the line's other attributes, since the rules
//! see only its text, and the matches are put on its own cells as a scan would put them.

use lru::LruCache;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::Arc;
use termwiz::hyperlink::{Hyperlink, Rule, RuleMatch};
use wezterm_term::Line;

/// The rules a scan was made with, as they compare: each one's pattern, format and highlight
type RulesSeen = Vec<(String, String, usize)>;

fn seen(rules: &[Rule]) -> RulesSeen {
    rules
        .iter()
        .map(|rule| (rule.regex.as_str().to_string(), rule.format.clone(), rule.highlight))
        .collect()
}

fn same(seen: &RulesSeen, rules: &[Rule]) -> bool {
    seen.len() == rules.len()
        && seen.iter().zip(rules).all(|((pattern, format, highlight), rule)| {
            pattern == rule.regex.as_str() && *format == rule.format && *highlight == rule.highlight
        })
}

/// A line's matches: where each link is in its text, in bytes, and the link
type Matches = Vec<(Range<usize>, Arc<Hyperlink>)>;

pub(crate) struct KeptScans {
    rules: RulesSeen,
    scans: LruCache<String, Matches>,
    /// Lines matched against the rules, and lines given kept matches (for tests and the docs)
    pub matched: u64,
    pub kept: u64,
}

impl KeptScans {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            scans: LruCache::new(NonZeroUsize::MIN),
            matched: 0,
            kept: 0,
        }
    }

    /// `line` scanned for `rules`' links unless it has been, its matches those kept for its text
    /// when they're kept, matched now and kept otherwise. `rows` is how many rows the pane shows:
    /// the scans of two screens of them are kept, a frame's and the one before
    pub fn scan(&mut self, line: &mut Line, rules: &[Rule], rows: usize) {
        if !same(&self.rules, rules) {
            self.rules = seen(rules);
            self.scans.clear();
        }
        if let Some(cap) = NonZeroUsize::new(rows.saturating_mul(2)) {
            if cap != self.scans.cap() {
                self.scans.resize(cap);
            }
        }
        let scans = &mut self.scans;
        let matched = &mut self.matched;
        let kept = &mut self.kept;
        line.lbterm_scan_and_create_hyperlinks_with(&mut |text| {
            if let Some(found) = scans.get(text) {
                *kept += 1;
                return found
                    .iter()
                    .map(|(range, link)| RuleMatch {
                        range: range.clone(),
                        link: Arc::clone(link),
                    })
                    .collect();
            }
            *matched += 1;
            let found = Rule::match_hyperlinks(text, rules);
            scans.put(
                text.to_string(),
                found
                    .iter()
                    .map(|m| (m.range.clone(), Arc::clone(&m.link)))
                    .collect(),
            );
            found
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use termwiz::cell::CellAttributes;
    use termwiz::surface::SEQ_ZERO;

    fn rules() -> Vec<Rule> {
        vec![
            Rule::new(r"\b\w+://(?:[\w.-]+)\.[a-z]{2,15}\S*\b", "$0").unwrap(),
            Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
        ]
    }

    fn line(text: &str) -> Line {
        Line::from_text(text, &CellAttributes::default(), SEQ_ZERO, None)
    }

    /// Each cell's link, as the line holds it
    fn links(line: &Line) -> Vec<Option<String>> {
        line.visible_cells()
            .map(|cell| cell.attrs().hyperlink().map(|link| link.uri().to_string()))
            .collect()
    }

    #[test]
    fn a_line_whose_text_was_scanned_gets_the_same_links_without_matching_again() {
        let rules = rules();
        let mut kept = KeptScans::new();
        let text = "see https://example.com/a and me@example.com here";
        let mut first = line(text);
        kept.scan(&mut first, &rules, 10);
        assert_eq!(kept.matched, 1);

        // The same text arriving as another line, as a row the server sent again after a scroll
        let mut moved = line(text);
        kept.scan(&mut moved, &rules, 10);
        assert_eq!((kept.matched, kept.kept), (1, 1), "matched again");

        // And it holds exactly what a scan of its own would have given it
        let mut scanned = line(text);
        scanned.scan_and_create_hyperlinks(&rules);
        assert_eq!(links(&moved), links(&scanned));
        assert_eq!(links(&first), links(&scanned));
        assert!(moved.has_hyperlink());
    }

    #[test]
    fn a_scanned_line_is_left_alone_and_a_line_with_no_links_is_kept_too() {
        let rules = rules();
        let mut kept = KeptScans::new();
        let mut plain = line("nothing to link here");
        kept.scan(&mut plain, &rules, 10);
        kept.scan(&mut plain, &rules, 10);
        assert_eq!((kept.matched, kept.kept), (1, 0), "a scanned line is scanned once");
        let mut again = line("nothing to link here");
        kept.scan(&mut again, &rules, 10);
        assert_eq!((kept.matched, kept.kept), (1, 1));
        assert!(!again.has_hyperlink());
    }

    #[test]
    fn other_rules_match_again() {
        let mut kept = KeptScans::new();
        let text = "see https://example.com/a";
        kept.scan(&mut line(text), &rules(), 10);
        let fewer = vec![Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap()];
        let mut other = line(text);
        kept.scan(&mut other, &fewer, 10);
        assert_eq!(kept.matched, 2, "kept matches are only for the rules they were made with");
        assert!(!other.has_hyperlink());
    }

    #[test]
    fn two_screens_of_lines_are_kept() {
        let rules = rules();
        let mut kept = KeptScans::new();
        for i in 0..8 {
            kept.scan(&mut line(&format!("line {i}")), &rules, 4);
        }
        assert_eq!(kept.matched, 8);
        // The eight are kept, a screen of four rows twice
        for i in 0..8 {
            kept.scan(&mut line(&format!("line {i}")), &rules, 4);
        }
        assert_eq!((kept.matched, kept.kept), (8, 8));
        // A ninth puts the least recently used out
        kept.scan(&mut line("line 8"), &rules, 4);
        kept.scan(&mut line("line 0"), &rules, 4);
        assert_eq!(kept.matched, 10);
    }
}
