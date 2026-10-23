//! Prompt marks on the scrollbar, as Windows Terminal draws them: a tick on the scrollbar's
//! track for every shell prompt in the pane's scrollback, where that prompt sits in the whole
//! buffer. Prompts are the rows a shell marks with OSC 133 (semantic zones of type Prompt).
//!
//! A pane's scrollback runs to tens of thousands of lines, so nothing here rescans it: the GUI
//! feeds [`PromptRows::set`] only the rows that changed since its last look, and the marks are
//! the first row of each run of prompt rows.

use std::collections::BTreeSet;

/// A row in a pane, counted from the start of its scrollback and stable as it scrolls
/// (wezterm's StableRowIndex)
pub type Row = isize;

/// The rows of one pane that hold prompt text
#[derive(Debug, Default, Clone)]
pub struct PromptRows {
    rows: BTreeSet<Row>,
}

impl PromptRows {
    /// Record what a changed row now holds
    pub fn set(&mut self, row: Row, is_prompt: bool) {
        if is_prompt {
            self.rows.insert(row);
        } else {
            self.rows.remove(&row);
        }
    }

    /// Forget rows that have scrolled out of the scrollback
    pub fn forget_before(&mut self, first_row: Row) {
        self.rows = self.rows.split_off(&first_row);
    }

    /// A mark for each prompt: the first row of each run of prompt rows, so a prompt that
    /// wraps onto a second line is one mark
    pub fn marks(&self) -> Vec<Row> {
        let mut marks = vec![];
        let mut prev: Option<Row> = None;
        for &row in &self.rows {
            if prev != Some(row - 1) {
                marks.push(row);
            }
            prev = Some(row);
        }
        marks
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Where the marks go on a track `track_height` pixels tall that stands for every row from
/// `first_row` for `total_rows` rows: one y (from the track's top) per mark, a mark's own
/// height apart at least, so a burst of short prompts doesn't draw one solid bar.
pub fn track_positions(
    marks: &[Row],
    first_row: Row,
    total_rows: usize,
    track_height: usize,
    mark_height: usize,
) -> Vec<usize> {
    if total_rows == 0 || track_height <= mark_height {
        return vec![];
    }
    let room = (track_height - mark_height) as f64;
    let mut ys: Vec<usize> = vec![];
    for &row in marks {
        let offset = row - first_row;
        if offset < 0 || offset as usize >= total_rows {
            continue;
        }
        let y = (offset as f64 / total_rows as f64 * room).round() as usize;
        match ys.last() {
            Some(&last) if y < last + mark_height => {}
            _ => ys.push(y),
        }
    }
    ys
}

/// The mark under a click on the track, if one is within `slop` pixels: the row to scroll to
pub fn mark_at(
    marks: &[Row],
    first_row: Row,
    total_rows: usize,
    track_height: usize,
    mark_height: usize,
    y: usize,
    slop: usize,
) -> Option<Row> {
    if total_rows == 0 || track_height <= mark_height {
        return None;
    }
    let room = (track_height - mark_height) as f64;
    marks
        .iter()
        .filter(|&&row| row >= first_row && ((row - first_row) as usize) < total_rows)
        .map(|&row| {
            let my = ((row - first_row) as f64 / total_rows as f64 * room).round() as usize;
            let centre = my + mark_height / 2;
            (row, centre.abs_diff(y))
        })
        .filter(|&(_, d)| d <= slop + mark_height / 2)
        .min_by_key(|&(_, d)| d)
        .map(|(row, _)| row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(list: &[(Row, bool)]) -> PromptRows {
        let mut p = PromptRows::default();
        for &(row, is_prompt) in list {
            p.set(row, is_prompt);
        }
        p
    }

    #[test]
    fn a_wrapped_prompt_is_one_mark() {
        let p = rows(&[(10, true), (11, true), (40, true)]);
        assert_eq!(p.marks(), vec![10, 40]);
    }

    #[test]
    fn a_row_that_stops_being_a_prompt_loses_its_mark() {
        let mut p = rows(&[(10, true), (40, true)]);
        p.set(40, false);
        assert_eq!(p.marks(), vec![10]);
    }

    #[test]
    fn rows_scrolled_out_of_the_scrollback_are_forgotten() {
        let mut p = rows(&[(10, true), (40, true), (90, true)]);
        p.forget_before(41);
        assert_eq!(p.marks(), vec![90]);
    }

    #[test]
    fn marks_sit_where_their_rows_are_in_the_whole_buffer() {
        // 1000 rows on a 102 px track with 2 px marks: 100 px of room
        assert_eq!(track_positions(&[0, 500, 999], 0, 1000, 102, 2), vec![0, 50, 100]);
        // counted from the first row still in the scrollback
        assert_eq!(track_positions(&[5500], 5000, 1000, 102, 2), vec![50]);
    }

    #[test]
    fn marks_closer_than_a_mark_are_drawn_once() {
        assert_eq!(track_positions(&[0, 1, 2, 500], 0, 1000, 102, 2), vec![0, 50]);
    }

    #[test]
    fn rows_outside_the_buffer_and_tiny_tracks_draw_nothing() {
        assert_eq!(track_positions(&[-1, 1000], 0, 1000, 102, 2), Vec::<usize>::new());
        assert_eq!(track_positions(&[5], 0, 1000, 2, 2), Vec::<usize>::new());
        assert_eq!(track_positions(&[5], 0, 0, 100, 2), Vec::<usize>::new());
    }

    #[test]
    fn a_click_near_a_mark_finds_its_row() {
        let marks = [0, 500, 999];
        assert_eq!(mark_at(&marks, 0, 1000, 102, 2, 52, 3), Some(500));
        assert_eq!(mark_at(&marks, 0, 1000, 102, 2, 49, 3), Some(500));
        assert_eq!(mark_at(&marks, 0, 1000, 102, 2, 30, 3), None);
    }
}
