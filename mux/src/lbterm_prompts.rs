//! LBTerm: which rows of a pane hold a shell's prompt (OSC 133 semantic zones of type Prompt),
//! for the scroll strip's marks (lbterm/SCROLL-STRIP.md, "A shell's marks"). The GUI reads its
//! own panes' rows with it, and the mux server answers a client's GetPromptRows with it, so a
//! mux client marks a pane's scrollback without ever having its lines.

use crate::pane::{Pane, WithPaneLines};
use std::ops::Range;
use wezterm_term::{Line, SemanticType, StableRowIndex};

/// Whether `line` holds a prompt: a Prompt zone, as `Line::semantic_zone_ranges` makes them
/// (trailing blanks left out). Most rows hold no prompt cell at all, which a look at their
/// attributes says without making their zones: a row in the scrollback is read off its runs of
/// attributes, not a cell at a time (Line::has_semantic_type; lbterm/PERF.md, findings 37 and 69)
pub fn is_prompt(line: &mut Line) -> bool {
    line.has_semantic_type(SemanticType::Prompt)
        && line
            .semantic_zone_ranges()
            .iter()
            .any(|z| z.semantic_type == SemanticType::Prompt)
}

/// The rows of `rows` that `pane` holds now and the prompts among them: `rows` cut to the pane's
/// scrollback and screen, and every row of that holding a prompt, in order
pub fn prompt_rows(pane: &dyn Pane, rows: Range<StableRowIndex>) -> (Range<StableRowIndex>, Vec<StableRowIndex>) {
    let dims = pane.get_dimensions();
    let end = dims.physical_top + dims.viewport_rows as StableRowIndex;
    let start = rows.start.max(dims.scrollback_top);
    let looked = start..rows.end.min(end).max(start);
    if looked.is_empty() {
        return (looked, vec![]);
    }
    let mut found = Found {
        looked: looked.clone(),
        rows: vec![],
    };
    pane.with_lines_mut(looked.clone(), &mut found);
    (looked, found.rows)
}

/// The prompt rows of the lines it's handed, those in `looked` only: a range scrolled out of the
/// scrollback since is handed over as the scrollback's first rows instead (Pane::get_lines)
struct Found {
    looked: Range<StableRowIndex>,
    rows: Vec<StableRowIndex>,
}

impl WithPaneLines for Found {
    fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]) {
        for (i, line) in lines.iter_mut().enumerate() {
            let row = first_row + i as StableRowIndex;
            if self.looked.contains(&row) && is_prompt(line) {
                self.rows.push(row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termwiz::surface::SEQ_ZERO;
    use wezterm_term::CellAttributes;

    fn line(parts: &[(&str, SemanticType)]) -> Line {
        let mut line = Line::with_width(0, SEQ_ZERO);
        for (text, kind) in parts {
            let mut attrs = CellAttributes::default();
            attrs.set_semantic_type(*kind);
            line.append_line(Line::from_text(text, &attrs, SEQ_ZERO, None), SEQ_ZERO);
        }
        line
    }

    #[test]
    fn a_prompt_row_is_one_with_a_prompt_zone() {
        assert!(is_prompt(&mut line(&[("PS C:\\work> ", SemanticType::Prompt), ("Get-Thing", SemanticType::Input)])));
        assert!(!is_prompt(&mut line(&[("  line 12 of the log", SemanticType::Output)])));
        assert!(!is_prompt(&mut line(&[])));
    }

    #[test]
    fn it_says_what_the_lines_zones_say() {
        // Blanks a shell cleared to the end of a row while its prompt was on are prompt cells to
        // Line::semantic_zone_ranges too (their attributes aren't a blank's)
        for parts in [
            vec![("done", SemanticType::Output), ("    ", SemanticType::Prompt)],
            vec![("> ", SemanticType::Prompt), ("x", SemanticType::Output)],
            vec![("x", SemanticType::Input), ("  ", SemanticType::Output)],
            vec![("   ", SemanticType::Output)],
        ] {
            let mut l = line(&parts);
            let zones = l.semantic_zone_ranges().iter().any(|z| z.semantic_type == SemanticType::Prompt);
            assert_eq!(is_prompt(&mut line(&parts)), zones, "{parts:?}");
        }
    }
}
