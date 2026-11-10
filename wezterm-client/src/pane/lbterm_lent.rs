//! LBTerm: a frame's rows lent where a mux client's pane holds them, not copied (wezterm-gui's
//! render/pane_lines.rs; lbterm/MUX.md, "A frame of a pane on the mux domain").
//!
//! Upstream drew a client pane's rows from copies: every frame read the rows in view through
//! get_lines (mux's impl_with_lines_via_get_lines), which copied each out of the cache. The key a
//! line's shapes are cached by (wezterm-gui's render/mod.rs, line_shape_hash) is kept on the line
//! it's worked out on, by the line's sequence number, so it went with the copy: every row of a
//! mux pane in view was hashed again on every frame (its cells walked twice, once for the hash and
//! once for a link drawn by state), where a pane of the window's own keeps the key on its
//! terminal's line and hashes only a line that changed. Now the rows are lent to the frame where
//! they're held, under the pane's lock, to a caller that touches nothing of the pane meanwhile
//! (render/pane_lines.rs's copy, which hashes each where it's held and copies only the rows that
//! changed since the frame before), so a line keeps its key until it's replaced. What reading a
//! row does to its entry (the fetch it starts, the state it leaves) is get_lines's, from the one
//! function both use, [`read_row`].
//!
//! And a line newer than the pane's sequence number (the server reads its pane's number, the rows
//! that changed and their lines one after another while the pane's parser goes on, so a line it
//! pushes can be newer than the push) was fetched again and marked stale, not as fetching, so the
//! line that came back was thrown away ("changed since fetch started") and fetched a second time
//! (the audit's client13). It's marked as fetching from then, as a stale line always was.

use super::{LineEntry, RenderableState};
use lru::LruCache;
use mux::pane::WithPaneLines;
use rangeset::RangeSet;
use std::ops::Range;
use std::time::Instant;
use termwiz::cell::CellAttributes;
use termwiz::color::AnsiColor;
use termwiz::surface::{SequenceNo, SEQ_ZERO};
use wezterm_term::{Line, StableRowIndex};

/// What reading a row (get_lines, or a frame lent its rows) does to the row's entry: the entry it
/// leaves, and whether the row is to be fetched. A line the client holds as the server last had
/// it stays; one newer than the pane (`seqno`) or stale is fetched, and kept as fetching from
/// `now`, so the line that comes back takes its place; a row not held is fetched; one on its way
/// stays on its way
pub(super) fn read_row(
    entry: Option<LineEntry>,
    seqno: SequenceNo,
    now: Instant,
) -> (LineEntry, bool) {
    match entry {
        Some(LineEntry::Line(line)) => {
            if line.changed_since(seqno) {
                (LineEntry::LineAndFetching(line, now), true)
            } else {
                (LineEntry::Line(line), false)
            }
        }
        Some(LineEntry::LineAndFetching(line, then)) => (LineEntry::LineAndFetching(line, then), false),
        Some(LineEntry::Fetching(then)) => (LineEntry::Fetching(then), false),
        Some(LineEntry::Stale(line)) => (LineEntry::LineAndFetching(line, now), true),
        None => (LineEntry::Fetching(now), true),
    }
}

/// The line an entry holds, if it holds one
fn held(entry: &mut LineEntry) -> Option<&mut Line> {
    match entry {
        LineEntry::Line(line) | LineEntry::LineAndFetching(line, _) | LineEntry::Stale(line) => {
            Some(line)
        }
        LineEntry::Fetching(_) => None,
    }
}

/// `rows` of `lines` read as get_lines reads them ([`read_row`]) and handed to `with` in one call:
/// each line held lent where it is, taken out of its entry for the call and put back after it, and
/// a blank `cols` wide for a row not held, as get_lines gives. `overlay` is a row `with` gets a
/// copy of instead, drawn on by the function (get_lines' lag indicator), the line held left as it
/// is. Returns the rows to fetch.
///
/// `with` runs while the caller holds the pane, so it must not reach the pane (wezterm-gui's
/// render/pane_lines.rs's copy reaches nothing but the window's own caches).
pub(super) fn lend_rows(
    lines: &mut LruCache<StableRowIndex, LineEntry>,
    rows: Range<StableRowIndex>,
    seqno: SequenceNo,
    cols: usize,
    now: Instant,
    overlay: Option<(StableRowIndex, &dyn Fn(&mut Line))>,
    with: &mut dyn WithPaneLines,
) -> RangeSet<StableRowIndex> {
    let mut to_fetch = RangeSet::new();
    let drawn_over = |row: StableRowIndex| matches!(overlay, Some((at, _)) if at == row);
    // Each row's line for the call, and whether it was taken out of its entry
    let mut lent: Vec<(Line, bool)> = Vec::with_capacity(rows.end.saturating_sub(rows.start) as usize);
    for row in rows.clone() {
        // As get_lines: the row read (and the most recently used), its line taken out if it holds
        // one, a placeholder of no cells in its place for the call
        let (line, taken) = match lines.get_mut(&row) {
            Some(entry) => {
                let (next, fetch) = read_row(Some(std::mem::replace(entry, LineEntry::Fetching(now))), seqno, now);
                *entry = next;
                if fetch {
                    to_fetch.add(row);
                }
                match held(entry) {
                    Some(line) if drawn_over(row) => {
                        let copy = line.clone();
                        copy.clear_appdata();
                        (copy, false)
                    }
                    Some(line) => (std::mem::replace(line, Line::new(SEQ_ZERO)), true),
                    None => (Line::with_width(cols, SEQ_ZERO), false),
                }
            }
            None => {
                let (next, _) = read_row(None, seqno, now);
                lines.put(row, next);
                to_fetch.add(row);
                (Line::with_width(cols, SEQ_ZERO), false)
            }
        };
        // The overlay drawn on the copy or the blank, as get_lines draws it on whatever it gives
        let mut line = line;
        if let Some((_, draw)) = overlay.filter(|_| drawn_over(row)) {
            draw(&mut line);
        }
        lent.push((line, taken));
    }

    {
        let mut refs: Vec<&mut Line> = lent.iter_mut().map(|(line, _)| line).collect();
        with.with_lines_mut(rows.start, &mut refs);
    }

    // Each taken line back where it came from: nothing else could reach the cache meanwhile. A
    // row the cache let go of while the others were read (a cache smaller than the rows read) has
    // its line dropped, to be fetched when it's read again, as get_lines' would have been
    for (row, (line, taken)) in rows.zip(lent) {
        if taken {
            if let Some(slot) = lines.peek_mut(&row).and_then(held) {
                *slot = line;
            }
        }
    }
    to_fetch
}

impl RenderableState {
    /// `rows` lent to `with` where this client holds them, read as get_lines reads them (the same
    /// fetches started, each entry left as get_lines leaves it), a blank for a row not held:
    /// what `with` does to a line stays with it, the key its shapes are cached by included.
    /// `with` must not reach this pane: it's held for the call
    pub fn lbterm_with_lines_lent(
        &self,
        rows: Range<StableRowIndex>,
        with: &mut dyn WithPaneLines,
    ) {
        let mut inner = self.inner.borrow_mut();
        let now = Instant::now();
        let seqno = inner.seqno;
        let cols = inner.dimensions.cols;
        // get_lines' lag indicator, drawn on a copy of the top row's line as get_lines draws it
        let lag = (inner.client.overlay_lag_indicator && inner.is_tardy()).then(|| {
            let status = format!(
                "wezterm: {:.0?}⏳since last response",
                inner.last_recv_time.elapsed()
            );
            let col = cols.saturating_sub(wezterm_term::unicode_column_width(&status, None));
            (inner.dimensions.physical_top, col, status)
        });
        let draw = |line: &mut Line| {
            if let Some((_, col, status)) = &lag {
                let mut attr = CellAttributes::default();
                attr.set_foreground(AnsiColor::White);
                attr.set_background(AnsiColor::Blue);
                line.overlay_text_with_attribute(*col, status, attr, SEQ_ZERO);
            }
        };
        let overlay = lag
            .as_ref()
            .map(|(row, _, _)| (*row, &draw as &dyn Fn(&mut Line)));
        let to_fetch = lend_rows(&mut inner.lines, rows, seqno, cols, now, overlay, with);
        inner.schedule_fetch_lines(to_fetch, now);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    fn line(text: &str, seqno: SequenceNo) -> Line {
        Line::from_text(text, &CellAttributes::default(), seqno, None)
    }

    fn text(line: &Line) -> String {
        line.as_str().trim_end().to_string()
    }

    fn state(entry: &LineEntry) -> (&'static str, Option<String>) {
        match entry {
            LineEntry::Line(l) => ("Line", Some(text(l))),
            LineEntry::LineAndFetching(l, _) => ("LineAndFetching", Some(text(l))),
            LineEntry::Stale(l) => ("Stale", Some(text(l))),
            LineEntry::Fetching(_) => ("Fetching", None),
        }
    }

    /// A line `with` was handed, and what it did: its text, and a mark left on it
    struct Seen {
        first: StableRowIndex,
        texts: Vec<String>,
        mark: Arc<u32>,
    }

    impl WithPaneLines for Seen {
        fn with_lines_mut(&mut self, first: StableRowIndex, lines: &mut [&mut Line]) {
            self.first = first;
            for line in lines.iter_mut() {
                self.texts.push(text(line));
                // What the renderer leaves on a line: the key its shapes are cached by
                line.set_appdata(Arc::clone(&self.mark));
            }
        }
    }

    fn seen() -> Seen {
        Seen {
            first: -1,
            texts: vec![],
            mark: Arc::new(7),
        }
    }

    fn marked(entry: &LineEntry) -> bool {
        match entry {
            LineEntry::Line(l) | LineEntry::LineAndFetching(l, _) | LineEntry::Stale(l) => {
                l.get_appdata().is_some()
            }
            LineEntry::Fetching(_) => false,
        }
    }

    #[test]
    fn reading_a_row_fetches_what_isnt_held_as_the_server_had_it() {
        let now = Instant::now();
        let then = now - std::time::Duration::from_millis(5);
        let seqno = 10;

        let (e, fetch) = read_row(Some(LineEntry::Line(line("as it is", 9))), seqno, now);
        assert_eq!((state(&e), fetch), (("Line", Some("as it is".into())), false));

        // A line newer than the pane is fetched, and marked as fetching from now, so the line
        // that comes back is taken (put_line takes a fetch only over an entry fetching from the
        // instant it began); upstream left it stale and threw that line away
        let (e, fetch) = read_row(Some(LineEntry::Line(line("newer", 11))), seqno, now);
        assert!(fetch);
        assert!(matches!(&e, LineEntry::LineAndFetching(l, t) if text(l) == "newer" && *t == now));

        let (e, fetch) = read_row(Some(LineEntry::Stale(line("old", 3))), seqno, now);
        assert!(fetch);
        assert!(matches!(&e, LineEntry::LineAndFetching(l, t) if text(l) == "old" && *t == now));

        let (e, fetch) = read_row(Some(LineEntry::LineAndFetching(line("coming", 3), then)), seqno, now);
        assert!(!fetch, "a fetch is on its way");
        assert!(matches!(&e, LineEntry::LineAndFetching(_, t) if *t == then));

        let (e, fetch) = read_row(Some(LineEntry::Fetching(then)), seqno, now);
        assert!(!fetch);
        assert!(matches!(e, LineEntry::Fetching(t) if t == then));

        let (e, fetch) = read_row(None, seqno, now);
        assert!(fetch);
        assert!(matches!(e, LineEntry::Fetching(t) if t == now));
    }

    #[test]
    fn a_frame_is_lent_the_lines_held_where_they_are() {
        let mut lines = LruCache::new(NonZeroUsize::new(16).unwrap());
        let now = Instant::now();
        let then = now - std::time::Duration::from_millis(5);
        lines.put(1, LineEntry::Line(line("one", 5)));
        lines.put(2, LineEntry::Stale(line("two", 5)));
        lines.put(3, LineEntry::LineAndFetching(line("three", 5), then));
        lines.put(4, LineEntry::Fetching(then));
        lines.put(5, LineEntry::Line(line("five, newer", 12)));

        let mut with = seen();
        let fetch = lend_rows(&mut lines, 1..7, 10, 20, now, None, &mut with);
        assert_eq!(with.first, 1);
        // Blanks for the rows not held, as get_lines gives
        assert_eq!(with.texts, vec!["one", "two", "three", "", "five, newer", ""]);

        // What the frame left on a held line stays with it: the line was lent, not copied
        for row in [1, 2, 3, 5] {
            assert!(marked(lines.peek(&row).unwrap()), "row {} kept the frame's mark", row);
        }
        // Each entry as get_lines leaves it, with the same rows fetched
        assert_eq!(state(lines.peek(&1).unwrap()), ("Line", Some("one".into())));
        assert_eq!(state(lines.peek(&2).unwrap()), ("LineAndFetching", Some("two".into())));
        assert!(matches!(lines.peek(&3), Some(LineEntry::LineAndFetching(_, t)) if *t == then));
        assert!(matches!(lines.peek(&4), Some(LineEntry::Fetching(t)) if *t == then));
        assert_eq!(state(lines.peek(&5).unwrap()), ("LineAndFetching", Some("five, newer".into())));
        assert!(matches!(lines.peek(&6), Some(LineEntry::Fetching(t)) if *t == now));
        let fetched: Vec<StableRowIndex> = fetch.iter().flat_map(|r| r.clone()).collect();
        assert_eq!(fetched, vec![2, 5, 6]);

        // Lent again, a line keeps what it was given and nothing is fetched for it
        let mut again = seen();
        let fetch = lend_rows(&mut lines, 1..2, 10, 20, now, None, &mut again);
        assert!(fetch.is_empty());
        assert_eq!(again.texts, vec!["one"]);
    }

    #[test]
    fn the_rows_read_become_the_most_recently_used() {
        let mut lines = LruCache::new(NonZeroUsize::new(3).unwrap());
        let now = Instant::now();
        lines.put(1, LineEntry::Line(line("a", 1)));
        lines.put(2, LineEntry::Line(line("b", 1)));
        lines.put(3, LineEntry::Line(line("c", 1)));
        lend_rows(&mut lines, 1..2, 5, 10, now, None, &mut seen());
        lines.put(4, LineEntry::Line(line("d", 1)));
        assert!(lines.peek(&2).is_none(), "2 was the least recently used once 1 was read");
        assert_eq!(state(lines.peek(&1).unwrap()), ("Line", Some("a".into())));
    }

    #[test]
    fn a_row_drawn_over_is_lent_a_copy_and_the_line_held_is_left() {
        let mut lines = LruCache::new(NonZeroUsize::new(8).unwrap());
        let now = Instant::now();
        lines.put(1, LineEntry::Line(line("top row", 5)));
        lines.put(2, LineEntry::Line(line("next", 5)));
        let draw = |line: &mut Line| line.overlay_text_with_attribute(10, "late", CellAttributes::default(), SEQ_ZERO);
        let mut with = seen();
        lend_rows(&mut lines, 1..3, 10, 20, now, Some((1, &draw as &dyn Fn(&mut Line))), &mut with);
        assert_eq!(with.texts, vec!["top row   late", "next"]);
        assert_eq!(state(lines.peek(&1).unwrap()), ("Line", Some("top row".into())));
        assert!(!marked(lines.peek(&1).unwrap()), "the copy drawn on took the mark, not the line held");
        assert!(marked(lines.peek(&2).unwrap()));

        // A row not held there gets its blank drawn on, as get_lines draws on whatever it gives
        let mut blank = seen();
        lend_rows(&mut lines, 7..8, 10, 20, now, Some((7, &draw as &dyn Fn(&mut Line))), &mut blank);
        assert_eq!(blank.texts, vec!["          late"]);
    }

    #[test]
    fn a_cache_smaller_than_the_rows_read_loses_a_line_not_a_placeholder() {
        // Two rows held, three read: the third's entry pushes out the least recently used, which
        // is neither of the two just read; with one held and two read into a cache of one, the
        // row read first goes, and its line with it
        let mut lines = LruCache::new(NonZeroUsize::new(1).unwrap());
        let now = Instant::now();
        lines.put(1, LineEntry::Line(line("one", 5)));
        let mut with = seen();
        lend_rows(&mut lines, 1..3, 10, 20, now, None, &mut with);
        assert_eq!(with.texts, vec!["one", ""]);
        assert!(lines.peek(&1).is_none());
        assert!(matches!(lines.peek(&2), Some(LineEntry::Fetching(_))));
    }
}
