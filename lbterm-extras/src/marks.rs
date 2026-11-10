//! Prompt marks on the scrollbar, as Windows Terminal draws them: a tick on the scrollbar's
//! track for every shell prompt in the pane's scrollback, where that prompt sits in the whole
//! buffer. Prompts are the rows a shell marks with OSC 133 (semantic zones of type Prompt).
//!
//! A pane's scrollback runs to tens of thousands of lines, so nothing here rescans it: the GUI
//! feeds [`PromptRows::set`] only the rows that changed since its last look ([`plan`] says
//! which), and the marks are the first row of each run of prompt rows.

use std::collections::BTreeSet;
use std::ops::Range;

/// A row in a pane, counted from the start of its scrollback and stable as it scrolls
/// (wezterm's StableRowIndex)
pub type Row = isize;

/// The rows of one pane that hold prompt text
#[derive(Debug, Default, Clone)]
pub struct PromptRows {
    rows: BTreeSet<Row>,
    /// Counts the changes, so what's drawn from the marks is made again only after one
    version: u64,
}

impl PromptRows {
    /// Record what a changed row now holds
    pub fn set(&mut self, row: Row, is_prompt: bool) {
        let changed = if is_prompt {
            self.rows.insert(row)
        } else {
            self.rows.remove(&row)
        };
        if changed {
            self.version += 1;
        }
    }

    /// Record what every row of `looked` holds: `prompts` (rows in it, in any order) are prompts,
    /// the rest aren't. Whether that changed anything
    pub fn set_range(&mut self, looked: Range<Row>, prompts: &[Row]) -> bool {
        if looked.is_empty() {
            return false;
        }
        let mut now: BTreeSet<Row> = prompts.iter().copied().filter(|r| looked.contains(r)).collect();
        let had: Vec<Row> = self.rows.range(looked.clone()).copied().collect();
        if had.len() == now.len() && had.iter().all(|r| now.contains(r)) {
            return false;
        }
        for r in had {
            self.rows.remove(&r);
        }
        self.rows.append(&mut now);
        self.version += 1;
        true
    }

    /// Forget rows that have scrolled out of the scrollback
    pub fn forget_before(&mut self, first_row: Row) {
        if self.rows.first().map_or(false, |&r| r < first_row) {
            self.rows = self.rows.split_off(&first_row);
            self.version += 1;
        }
    }

    /// Forget every row
    pub fn clear(&mut self) {
        if !self.rows.is_empty() {
            self.rows.clear();
            self.version += 1;
        }
    }

    /// Changes with every change to the rows
    pub fn version(&self) -> u64 {
        self.version
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

/// What a pane's marks were last read at: its sequence number (none before the first look), its
/// width, and the first row of its screen. The rows above that had gone into the scrollback,
/// where nothing writes them again, and were read as they went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seen {
    pub seqno: Option<usize>,
    pub cols: usize,
    pub top: Row,
}

/// A pane as it is now
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dims {
    /// The scrollback's first row
    pub first: Row,
    /// The screen's first row
    pub top: Row,
    /// The row after the screen's last
    pub end: Row,
    pub cols: usize,
    /// Its sequence number, which moves with every change to its rows
    pub seqno: usize,
}

/// What a look at a pane has to read
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Nothing was written since the last look
    Nothing,
    /// Every row: the first look, or a new width (the scrollback rewrapped and every row was
    /// numbered again)
    Everything { history: Range<Row>, screen: Range<Row> },
    /// The rows that went into the scrollback since the last look (`history`: on the screen then,
    /// so they may have changed before they went) and the screen's rows (`screen`), each read
    /// only if it changed after sequence number `since`
    Since { history: Range<Row>, screen: Range<Row>, since: usize },
}

/// What a look at a pane `now` reads, after the look that `seen` says: never more than the rows
/// that were written since, so a long scrollback costs once, when it's first looked at
pub fn plan(seen: &Seen, now: &Dims) -> Plan {
    let top = now.top.max(now.first);
    let screen = top..now.end.max(top);
    match seen.seqno {
        None => Plan::Everything { history: now.first..top, screen },
        Some(_) if seen.cols != now.cols => Plan::Everything { history: now.first..top, screen },
        Some(seqno) if seqno == now.seqno => Plan::Nothing,
        Some(since) => {
            // A screen made taller takes rows back from the scrollback: none went into it
            let from = seen.top.clamp(now.first, top);
            Plan::Since { history: from..top, screen, since }
        }
    }
}

/// The newest `most` rows of a set of rows (`ranges`, in order, apart): the next chunk of a
/// scrollback to read, newest first, so the marks nearest where the view usually is come first
pub fn newest<'a>(ranges: impl Iterator<Item = &'a Range<Row>>, most: Row) -> Option<Range<Row>> {
    let last = ranges.filter(|r| !r.is_empty()).last()?;
    Some(last.start.max(last.end - most.max(1))..last.end)
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

/// What a [`Plan::Since`] look at the GUI's own pane reads at once, and what it leaves to be read
/// a chunk of `chunk` rows at a time between frames, newest first, as a first look's scrollback
/// is: the rows that went into the scrollback since the last look, when more than a chunk of them
/// went (a tab printing a long log in the background, looked at again), so no look holds a
/// frame for a long run of output, where it read them all in the frame that showed the tab
pub fn split_since(history: Range<Row>, screen: Range<Row>, chunk: Row) -> (Range<Row>, Option<Range<Row>>) {
    if history.end - history.start > chunk {
        (screen, Some(history))
    } else {
        (history.start..screen.end, None)
    }
}

/// GetPromptRows' PDU number, as codec numbers it (its round-trip test holds it there)
pub const GET_PROMPT_ROWS_PDU: u64 = 7503;

/// Whether an error asking a mux server which rows hold a prompt is a server of a build from
/// before GetPromptRows, which answers that it doesn't know the PDU, naming it: that one never
/// will, so the GUI marks only what it has drawn until the server restarts. Any other error (a
/// pane gone as it's respawned, a connection lost and made again) may not happen next time
pub fn server_lacks_prompt_rows(error: &str) -> bool {
    error.contains(&format!("Invalid {{ ident: {GET_PROMPT_ROWS_PDU} }}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_run_of_output_since_the_last_look_goes_in_chunks() {
        // A line or two since the last look: read with the screen's rows at once
        assert_eq!(split_since(100..102, 102..140, 5_000), (100..140, None));
        // Exactly a chunk: at once
        assert_eq!(split_since(0..5_000, 5_000..5_040, 5_000), (0..5_040, None));
        // 50,000 rows printed in the background: the screen now, the rows to the chunks
        assert_eq!(split_since(0..50_000, 50_000..50_040, 5_000), (50_000..50_040, Some(0..50_000)));
    }

    #[test]
    fn only_a_server_that_doesnt_know_the_pdu_is_taken_for_an_old_one() {
        // What the GUI was told by a server of a build from before GetPromptRows (2026-10-31)
        let old = r#"unexpected response Ok(ErrorResponse(ErrorResponse { reason: "Error: invalid PDU Invalid { ident: 7503 }" }))"#;
        assert!(server_lacks_prompt_rows(old));
        // A pane gone, a lost connection, another PDU unknown: asked again
        assert!(!server_lacks_prompt_rows(r#"unexpected response Ok(ErrorResponse(ErrorResponse { reason: "Error: no such pane 7" }))"#));
        assert!(!server_lacks_prompt_rows("BrokenPromise: the client went away"));
        assert!(!server_lacks_prompt_rows("Error: invalid PDU Invalid { ident: 75030 }"));
    }

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
    fn a_range_read_whole_replaces_what_was_known_of_it() {
        let mut p = rows(&[(10, true), (40, true), (90, true)]);
        let v = p.version();
        // 30..60 now holds prompts at 35 and 50 (and 70 is outside it)
        assert!(p.set_range(30..60, &[50, 35, 70]));
        assert_eq!(p.marks(), vec![10, 35, 50, 90]);
        assert!(p.version() > v);
        // The same again changes nothing
        let v = p.version();
        assert!(!p.set_range(30..60, &[35, 50]));
        assert_eq!(p.version(), v);
        assert!(!p.set_range(60..60, &[]));
    }

    #[test]
    fn the_version_moves_only_with_a_change() {
        let mut p = PromptRows::default();
        let v = p.version();
        p.set(5, false);
        p.forget_before(0);
        p.clear();
        assert_eq!(p.version(), v);
        p.set(5, true);
        assert_ne!(p.version(), v);
        let v = p.version();
        p.set(5, true);
        assert_eq!(p.version(), v);
        p.forget_before(6);
        assert_ne!(p.version(), v);
        assert!(p.is_empty());
    }

    fn dims(first: Row, top: Row, rows: Row, cols: usize, seqno: usize) -> Dims {
        Dims { first, top, end: top + rows, cols, seqno }
    }

    #[test]
    fn the_first_look_reads_everything() {
        assert_eq!(
            plan(&Seen::default(), &dims(0, 50_000, 30, 110, 7)),
            Plan::Everything { history: 0..50_000, screen: 50_000..50_030 }
        );
    }

    #[test]
    fn nothing_written_reads_nothing() {
        let seen = Seen { seqno: Some(7), cols: 110, top: 50_000 };
        assert_eq!(plan(&seen, &dims(0, 50_000, 30, 110, 7)), Plan::Nothing);
    }

    #[test]
    fn output_reads_what_went_into_the_scrollback_and_the_screen() {
        // Three lines printed, the scrollback full: its first row went
        let seen = Seen { seqno: Some(7), cols: 110, top: 50_000 };
        assert_eq!(
            plan(&seen, &dims(3, 50_003, 30, 110, 9)),
            Plan::Since { history: 50_000..50_003, screen: 50_003..50_033, since: 7 }
        );
        // A line rewritten in place: only the screen
        assert_eq!(
            plan(&seen, &dims(0, 50_000, 30, 110, 8)),
            Plan::Since { history: 50_000..50_000, screen: 50_000..50_030, since: 7 }
        );
    }

    #[test]
    fn a_taller_screen_puts_nothing_into_the_scrollback() {
        let seen = Seen { seqno: Some(7), cols: 110, top: 50_000 };
        assert_eq!(
            plan(&seen, &dims(0, 49_990, 40, 110, 8)),
            Plan::Since { history: 49_990..49_990, screen: 49_990..50_030, since: 7 }
        );
    }

    #[test]
    fn a_new_width_reads_everything_again() {
        let seen = Seen { seqno: Some(7), cols: 110, top: 50_000 };
        assert_eq!(
            plan(&seen, &dims(0, 61_000, 30, 90, 8)),
            Plan::Everything { history: 0..61_000, screen: 61_000..61_030 }
        );
    }

    #[test]
    fn a_cleared_scrollback_reads_from_its_new_start() {
        // cls: the scrollback emptied, its first row the screen's
        let seen = Seen { seqno: Some(7), cols: 110, top: 50_000 };
        assert_eq!(
            plan(&seen, &dims(50_000, 50_000, 30, 110, 8)),
            Plan::Since { history: 50_000..50_000, screen: 50_000..50_030, since: 7 }
        );
    }

    #[test]
    fn a_scrollback_is_read_newest_first_a_chunk_at_a_time() {
        let mut rows = vec![0..25_000, 30_000..30_010];
        let mut chunks = vec![];
        while let Some(chunk) = newest(rows.iter(), 10_000) {
            // Taken out, as the GUI's RangeSet does
            let last = rows.pop().unwrap();
            if last.start < chunk.start {
                rows.push(last.start..chunk.start);
            }
            chunks.push(chunk);
        }
        assert_eq!(chunks, vec![30_000..30_010, 15_000..25_000, 5_000..15_000, 0..5_000]);
        assert_eq!(newest([5..5].iter(), 10), None);
    }

    #[test]
    fn a_click_near_a_mark_finds_its_row() {
        let marks = [0, 500, 999];
        assert_eq!(mark_at(&marks, 0, 1000, 102, 2, 52, 3), Some(500));
        assert_eq!(mark_at(&marks, 0, 1000, 102, 2, 49, 3), Some(500));
        assert_eq!(mark_at(&marks, 0, 1000, 102, 2, 30, 3), None);
    }
}
