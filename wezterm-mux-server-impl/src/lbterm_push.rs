//! LBTerm: what a push of a pane's changes copies.
//!
//! `PerPane::compute_changes` sends a client the lines that changed in the viewport and the
//! cursor's line, on every push of a pane's output, every key and every poll. Upstream copied
//! every line of the viewport (`Pane::get_lines`, a clone of each) to keep the changed ones, then
//! took the terminal's lock again to copy the cursor's line once more, so a cursor line that had
//! changed went twice. [`lines_to_send`] copies only the lines that go, the cursor's once, under
//! one lock.

use mux::pane::{Pane, WithPaneLines};
use rangeset::RangeSet;
use std::ops::Range;
use std::sync::Arc;
use wezterm_term::{Line, StableRowIndex};

/// The lines a push sends besides the dirty rows it names: each dirty line in `viewport` and the
/// cursor's line, copied and compressed, in row order, each once. The rows sent are taken out of
/// `dirty` (the client gets them whole, so they aren't fetched).
pub fn lines_to_send(
    pane: &Arc<dyn Pane>,
    viewport: Range<StableRowIndex>,
    dirty: &mut RangeSet<StableRowIndex>,
    cursor_row: StableRowIndex,
) -> Vec<(StableRowIndex, Line)> {
    let mut take = TakeLines {
        dirty,
        cursor_row,
        lines: vec![],
        cursor_sent: false,
    };
    pane.with_lines_mut(viewport, &mut take);
    let cursor_sent = take.cursor_sent;
    let mut lines = take.lines;
    if !cursor_sent {
        // The cursor outside the viewport: as upstream fetched it
        let (row, mut got) = pane.get_lines(cursor_row..cursor_row + 1);
        if !got.is_empty() {
            let mut line = got.remove(0);
            line.compress_for_scrollback();
            lines.push((row, line));
        }
    }
    lines
}

/// Takes the lines a push sends out of the lines a pane hands it
pub struct TakeLines<'a> {
    pub dirty: &'a mut RangeSet<StableRowIndex>,
    pub cursor_row: StableRowIndex,
    pub lines: Vec<(StableRowIndex, Line)>,
    pub cursor_sent: bool,
}

impl WithPaneLines for TakeLines<'_> {
    fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]) {
        for (idx, line) in lines.iter().enumerate() {
            let row = first_row + idx as StableRowIndex;
            let is_dirty = self.dirty.contains(row);
            if !is_dirty && row != self.cursor_row {
                continue;
            }
            if is_dirty {
                self.dirty.remove(row);
            }
            if row == self.cursor_row {
                self.cursor_sent = true;
            }
            let mut line = (**line).clone();
            line.compress_for_scrollback();
            self.lines.push((row, line));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn line(text: &str) -> Line {
        Line::from_text(text, &Default::default(), 0, None)
    }

    /// What a push takes from `lines` (rows `first` on) with `dirty` changed and the cursor on
    /// row `cursor`: the rows and their text, the dirty rows left, whether the cursor's went
    fn take(
        first: StableRowIndex,
        lines: &mut [Line],
        dirty: &[StableRowIndex],
        cursor: StableRowIndex,
    ) -> (Vec<(StableRowIndex, String)>, RangeSet<StableRowIndex>, bool) {
        let mut set = RangeSet::new();
        for &r in dirty {
            set.add(r);
        }
        let mut refs: Vec<&mut Line> = lines.iter_mut().collect();
        let mut t = TakeLines {
            dirty: &mut set,
            cursor_row: cursor,
            lines: vec![],
            cursor_sent: false,
        };
        t.with_lines_mut(first, &mut refs);
        let sent = t.cursor_sent;
        let got = t
            .lines
            .iter()
            .map(|(r, l)| (*r, l.as_str().trim_end().to_string()))
            .collect();
        (got, set, sent)
    }

    #[test]
    fn only_the_dirty_lines_and_the_cursors_are_copied_each_once() {
        let mut lines: Vec<Line> = (0..40).map(|i| line(&format!("row {i}"))).collect();
        // Rows 100..140, rows 105 and 106 changed and one above the viewport, the cursor on 106
        let (got, left, sent) = take(100, &mut lines, &[50, 105, 106], 106);
        assert_eq!(got, vec![(105, "row 5".to_string()), (106, "row 6".to_string())]);
        assert!(sent);
        // The one above the viewport is still named as dirty, for the client to make stale
        assert!(left.contains(50));
        assert!(!left.contains(105) && !left.contains(106));
    }

    #[test]
    fn the_cursors_line_goes_when_nothing_changed() {
        let mut lines: Vec<Line> = (0..10).map(|i| line(&format!("row {i}"))).collect();
        let (got, _, sent) = take(0, &mut lines, &[], 3);
        assert_eq!(got, vec![(3, "row 3".to_string())]);
        assert!(sent);
    }

    #[test]
    fn a_cursor_outside_what_was_handed_out_is_left_to_fetch() {
        let mut lines: Vec<Line> = (0..10).map(|i| line(&format!("row {i}"))).collect();
        let (got, _, sent) = take(0, &mut lines, &[2], 20);
        assert_eq!(got, vec![(2, "row 2".to_string())]);
        assert!(!sent);
    }
}
