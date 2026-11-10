//! LBTerm: where a screen's lines can have changed since a sequence number, so that finding the
//! ones that did reads those and not every line of the scrollback.
//!
//! A mux server works out what to send a client from the lines changed since the sequence number
//! it last sent (`Screen::get_changed_stable_rows`, from `GetPaneRenderChangesResponse`'s
//! `compute_changes`), asked from the top of the scrollback down, on every push of the pane's
//! output, every key sent to it and every poll, with the pane's terminal locked. With 50,000
//! lines of scrollback (Lachlan's `scrollback_lines`) that read every line's sequence number to
//! find the one or two that changed.
//!
//! But a line only changes while it's in the viewport, and the viewport's top only moves down,
//! unless the screen is resized or something reaches past the viewport (the alternate screen's
//! switch dirtying the top of the scrollback, a palette change dirtying every line, a kitty image
//! taken out of the scrollback, a line handed out by its physical index). So each time the
//! terminal's sequence number moves on, the screen notes where its viewport's top is
//! ([`ChangeBounds::advanced`]): every line changed from then on is at or below that row. A
//! change above the viewport lowers every note to its row ([`ChangeBounds::touched`]), and a
//! resize forgets them ([`ChangeBounds::forget`]). The lines changed since `seqno` are then the
//! lines changed at `seqno + 1` or later, all at or below the note made when the sequence number
//! became `seqno + 1` or before ([`ChangeBounds::lowest_since`]); with no such note (it's older
//! than the oldest kept), every line is read, as upstream did. The rows found are the same as
//! reading every line finds (`term/src/test/lbterm_changes.rs` holds them to it).

use crate::StableRowIndex;
use std::collections::VecDeque;
use wezterm_surface::SequenceNo;

/// How many notes a screen keeps: one each time the viewport's top has moved since the last, so
/// these cover the last 256 times the screen scrolled. A client asking about older changes than
/// that (one that hasn't been sent the pane for a while) has every line read.
pub const KEPT: usize = 256;

/// Lower bounds on the rows a screen's lines can have changed in since recent sequence numbers
#[derive(Debug, Clone, Default)]
pub struct ChangeBounds {
    /// `(seqno, row)`, oldest first: every line changed at `seqno` or later is at stable row
    /// `row` or below it
    notes: VecDeque<(SequenceNo, StableRowIndex)>,
}

impl ChangeBounds {
    /// The terminal's sequence number has moved on to `seqno` with the viewport's top at stable
    /// row `top`: nothing has been changed at `seqno` yet
    pub fn advanced(&mut self, seqno: SequenceNo, top: StableRowIndex) {
        if let Some(&(_, row)) = self.notes.back() {
            // The last note already says it
            if row == top {
                return;
            }
        }
        if self.notes.len() == KEPT {
            self.notes.pop_front();
        }
        self.notes.push_back((seqno, top));
    }

    /// A line above the viewport, at stable row `row`, may have changed now: every note made
    /// before now says changes since reach up to it
    pub fn touched(&mut self, row: StableRowIndex) {
        for note in self.notes.iter_mut() {
            note.1 = note.1.min(row);
        }
    }

    /// The lines have moved (a resize): nothing noted holds any more
    pub fn forget(&mut self) {
        self.notes.clear();
    }

    /// The highest stable row every line changed since `seqno` (at `seqno + 1` or later) is at or
    /// below, or None when nothing kept says (every line has to be read)
    pub fn lowest_since(&self, seqno: SequenceNo) -> Option<StableRowIndex> {
        let after = seqno.saturating_add(1);
        // The latest note made at or before `after`: its row is the highest such bound, since
        // the notes' rows rise with them
        let idx = self.notes.partition_point(|&(s, _)| s <= after);
        if idx == 0 {
            None
        } else {
            Some(self.notes[idx - 1].1)
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.notes.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn nothing_noted_reads_everything() {
        let b = ChangeBounds::default();
        assert_eq!(b.lowest_since(0), None);
        assert_eq!(b.lowest_since(100), None);
    }

    #[test]
    fn a_change_since_a_seqno_is_at_or_below_the_top_when_it_moved_on() {
        let mut b = ChangeBounds::default();
        b.advanced(5, 100);
        b.advanced(6, 101);
        b.advanced(9, 140);
        // Changed at 5 or later: from 100
        assert_eq!(b.lowest_since(4), Some(100));
        // At 6 or later
        assert_eq!(b.lowest_since(5), Some(101));
        assert_eq!(b.lowest_since(6), Some(101));
        assert_eq!(b.lowest_since(7), Some(101));
        // At 9 or later
        assert_eq!(b.lowest_since(8), Some(140));
        assert_eq!(b.lowest_since(1000), Some(140));
        // Older than the oldest note
        assert_eq!(b.lowest_since(3), None);
    }

    #[test]
    fn the_same_top_again_notes_nothing() {
        let mut b = ChangeBounds::default();
        b.advanced(5, 100);
        b.advanced(6, 100);
        b.advanced(7, 100);
        assert_eq!(b.len(), 1);
        assert_eq!(b.lowest_since(6), Some(100));
    }

    #[test]
    fn a_change_above_the_viewport_lowers_every_note() {
        let mut b = ChangeBounds::default();
        b.advanced(5, 100);
        b.advanced(6, 110);
        b.touched(20);
        assert_eq!(b.lowest_since(4), Some(20));
        assert_eq!(b.lowest_since(5), Some(20));
        // A later note isn't lowered: nothing changed at its seqno yet
        b.advanced(7, 110);
        assert_eq!(b.lowest_since(6), Some(110));
        assert_eq!(b.lowest_since(5), Some(20));
    }

    #[test]
    fn forgotten_reads_everything_until_the_next_note() {
        let mut b = ChangeBounds::default();
        b.advanced(5, 100);
        b.forget();
        assert_eq!(b.lowest_since(10), None);
        b.advanced(11, 30);
        assert_eq!(b.lowest_since(10), Some(30));
        assert_eq!(b.lowest_since(9), None);
    }

    #[test]
    fn only_the_latest_notes_are_kept() {
        let mut b = ChangeBounds::default();
        for i in 0..(KEPT + 10) {
            b.advanced(i as SequenceNo + 1, i as StableRowIndex);
        }
        assert_eq!(b.len(), KEPT);
        assert_eq!(b.lowest_since(5), None);
        assert_eq!(
            b.lowest_since(KEPT as SequenceNo + 8),
            Some(KEPT as StableRowIndex + 8)
        );
    }
}
