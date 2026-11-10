//! LBTerm: what the scroll strip's prompt marks need of a mux client's pane (wezterm-gui's
//! render/marks.rs; lbterm/SCROLL-STRIP.md, "A shell's marks"): the lines it already holds as the
//! server has them, looked at without fetching any, and the server asked which rows of the rest
//! hold a prompt, so no line of a scrollback is ever fetched to mark it.

use super::{LineEntry, RenderableState};
use codec::{GetPromptRows, GetPromptRowsResponse};
use mux::pane::WithPaneLines;
use rangeset::RangeSet;
use std::future::Future;
use std::ops::Range;
use std::sync::Arc;
use wezterm_term::StableRowIndex;

impl RenderableState {
    /// Hands `with` each line of `rows` that this client holds as the server last had it, without
    /// fetching any or moving any in the cache, and returns the rows it doesn't hold so: never
    /// fetched, gone from the cache, stale, or on their way. Those get_lines would fetch
    pub fn lbterm_with_held_lines(
        &self,
        rows: Range<StableRowIndex>,
        with: &mut dyn WithPaneLines,
    ) -> RangeSet<StableRowIndex> {
        let mut inner = self.inner.borrow_mut();
        let seqno = inner.seqno;
        let mut missing = RangeSet::new();
        for row in rows {
            match inner.lines.peek_mut(&row) {
                // get_lines' own test of a line it needn't fetch again
                Some(LineEntry::Line(line)) if !line.changed_since(seqno) => {
                    with.with_lines_mut(row, &mut [line]);
                }
                _ => missing.add(row),
            }
        }
        missing
    }

    /// Whether this client holds every line of `rows` as the server last had it, so a read of
    /// them (get_lines) had them all and can be kept until the pane's sequence number moves: a
    /// fetch that completes changes a line without moving it, so a read that met a line on its
    /// way, or stale, or not held at all (get_lines gives a blank or an old line for those) is
    /// read again (wezterm-gui's render/questions.rs, `lbterm_screen`, and render/prompt.rs)
    pub fn lbterm_holds(&self, rows: Range<StableRowIndex>) -> bool {
        let inner = self.inner.borrow();
        let seqno = inner.seqno;
        rows.into_iter().all(|row| {
            matches!(inner.lines.peek(&row), Some(LineEntry::Line(line)) if !line.changed_since(seqno))
        })
    }

    /// Asks the server which of `rows` hold a prompt (GetPromptRows): the rows, not their lines
    pub fn lbterm_prompt_rows(
        &self,
        rows: Range<StableRowIndex>,
    ) -> impl Future<Output = anyhow::Result<GetPromptRowsResponse>> + 'static {
        let inner = self.inner.borrow();
        let client = Arc::clone(&inner.client);
        let pane_id = inner.remote_pane_id;
        async move { client.client.get_prompt_rows(GetPromptRows { pane_id, rows }).await }
    }
}
