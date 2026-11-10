//! LBTerm: a pane of the GUI's own has the rows a frame shows copied out of its terminal, with the
//! terminal held only for the copy, and laid out after it's let go (lbterm/PERF.md, finding 37).
//!
//! Upstream laid them out inside the pane's with_lines_mut, which holds the pane's terminal for
//! all of it: every line that changed shaped and turned into quads, about 3.8 ms a frame for a
//! pane of 30 rows printing at full speed. The pane's parser takes the same lock to put each batch
//! of output into the terminal, so it waited on every frame, and the more frames, the slower
//! output went: since main's perf/frame-loop painted about twice the frames, 7 to 12% slower.
//! A mux domain's pane (ClientPane), every pane of Lachlan's, is copied the same way since
//! perf/mux-client, for another reason: upstream laid it out from copies it made of every row in
//! view on every frame (get_lines), so the key its shapes are cached by, which a line keeps on
//! itself (render/mod.rs, line_shape_hash), went with the copy, and every row of every mux pane in
//! view was hashed again on every frame, its cells walked twice. Its client lends the rows where
//! it holds them (wezterm-client's pane/lbterm_lent.rs), so the key stays on its line, and only
//! the rows that changed are copied. The pane is held while they're lent, so nothing done with
//! them here reaches it. An overlay (copy mode, quick select) draws its own over its pane's, so it
//! keeps upstream's way.
//!
//! A row the last frame copied that's the same as the terminal's now (Line's equality: its
//! sequence number, its bits, its cells) is that copy again, so only the rows that changed are
//! copied: a row of 24-bit colours keeps each cell's attributes on the heap, and copying a screen
//! of 160 by 60 of them anew took 0.9 ms a frame where comparing it takes a fraction of that.
//!
//! A pane's copy is kept until the pane goes (its `PaneRemoved`, which every window hears), so a
//! long-lived window keeps one screen of rows for each pane of its own it has painted that's still
//! open, and a tab gone back to copies only the rows that changed while it was away. Until
//! 2026-11-01 copies were let go only as a new pane was first copied, so closing tabs without
//! opening any kept theirs for good (the review of perf/key-latency).
//!
//! With `lbterm::lines` at debug (WEZTERM_LOG) each copy says how many rows it took, how many it
//! copied anew, how long it held the terminal and how long it waited for it first, and a copy
//! dropped is said (perf/output-hold.ps1 reads them).

use crate::termwindow::TermWindow;
use mux::localpane::LocalPane;
use mux::pane::{Pane, PaneId, WithPaneLines};
use wezterm_client::pane::ClientPane;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wezterm_term::{Line, StableRowIndex};

/// A pane's rows taken out of its terminal for a frame, the first of them `first`
#[derive(Default)]
pub(crate) struct CopiedLines {
    first: StableRowIndex,
    lines: Vec<Line>,
}

impl CopiedLines {
    /// Lays them out with `with`, as the pane's with_lines_mut would have with its terminal held
    pub(crate) fn lay_out(&mut self, with: &mut dyn WithPaneLines) {
        let mut refs: Vec<&mut Line> = self.lines.iter_mut().collect();
        with.with_lines_mut(self.first, &mut refs);
    }

    /// The copy of `row` this holds, taken out, when it's the same as `line`
    fn take_same(&mut self, row: StableRowIndex, line: &Line) -> Option<Line> {
        let i = usize::try_from(row - self.first).ok()?;
        match self.lines.get_mut(i) {
            Some(kept) if *kept == *line => Some(std::mem::replace(kept, Line::new(0))),
            _ => None,
        }
    }
}

/// The rows each pane's last frame copied (lbterm_keep_lines puts them back)
#[derive(Default)]
pub(crate) struct LineCopies {
    panes: HashMap<PaneId, CopiedLines>,
}

/// The copy, made inside the pane's with_lines_mut
struct Copy<'a> {
    term_window: &'a mut TermWindow,
    last: CopiedLines,
    copied: CopiedLines,
    /// How many rows were copied anew, not taken from the last frame's
    fresh: usize,
    /// When the terminal was taken and for how long, when lbterm::lines says so
    held: Option<(Instant, Duration)>,
    timed: bool,
}

impl WithPaneLines for Copy<'_> {
    fn with_lines_mut(&mut self, first: StableRowIndex, lines: &mut [&mut Line]) {
        let took = self.timed.then(Instant::now);
        self.copied.first = first;
        self.copied.lines.reserve(lines.len());
        for (i, line) in lines.iter().enumerate() {
            // The key its shapes are cached by is worked out on the terminal's own line, which
            // keeps it (render/mod.rs, line_shape_hash), so a line that hasn't changed isn't
            // hashed again next frame; the copy carries it
            self.term_window.line_shape_hash(line);
            let row = first + i as StableRowIndex;
            let copy = match self.last.take_same(row, line) {
                Some(kept) => kept,
                None => {
                    self.fresh += 1;
                    (**line).clone()
                }
            };
            self.copied.lines.push(copy);
        }
        self.held = took.map(|took| (took, took.elapsed()));
    }
}

impl TermWindow {
    /// `pane`'s rows in `range`, copied with its terminal held only for the copy, when it's a pane
    /// of the GUI's own, or lent by its client where they're held, when it's a mux domain's; None
    /// for any other (an overlay), laid out inside its with_lines_mut as upstream did
    pub(crate) fn lbterm_copy_lines(
        &mut self,
        pane: &Arc<dyn Pane>,
        range: Range<StableRowIndex>,
    ) -> Option<CopiedLines> {
        let client = pane.downcast_ref::<ClientPane>();
        if client.is_none() {
            pane.downcast_ref::<LocalPane>()?;
        }
        let pane_id = pane.pane_id();
        let last = self
            .lbterm_line_copies
            .panes
            .remove(&pane_id)
            .unwrap_or_default();
        let timed = log::log_enabled!(target: "lbterm::lines", log::Level::Debug);
        let asked = timed.then(Instant::now);
        let mut copy = Copy {
            term_window: self,
            last,
            copied: CopiedLines {
                first: range.start,
                lines: vec![],
            },
            fresh: 0,
            held: None,
            timed,
        };
        match client {
            // The copy reaches nothing of the pane, which its client holds for the call
            Some(client) => client.lbterm_with_lines_lent(range, &mut copy),
            None => pane.with_lines_mut(range, &mut copy),
        }
        if let (Some(asked), Some((took, held))) = (asked, copy.held) {
            log::debug!(
                target: "lbterm::lines",
                "pane {}: {} rows copied, {} anew, its terminal held {:.3} ms, waited for {:.3} ms",
                pane_id,
                copy.copied.lines.len(),
                copy.fresh,
                held.as_secs_f64() * 1000.0,
                took.saturating_duration_since(asked).as_secs_f64() * 1000.0
            );
        }
        Some(copy.copied)
    }

    /// The rows a frame laid out, kept for the next frame's copy of `pane`
    pub(crate) fn lbterm_keep_lines(&mut self, pane: PaneId, copied: CopiedLines) {
        self.lbterm_line_copies.panes.insert(pane, copied);
    }

    /// `pane` has gone (MuxNotification::PaneRemoved): its copy, if this window kept one, goes too
    pub(crate) fn lbterm_lines_gone(&mut self, pane: PaneId) {
        if self.lbterm_line_copies.panes.remove(&pane).is_some() {
            log::debug!(target: "lbterm::lines", "pane {pane}: its copy dropped, the pane gone");
        }
    }
}
