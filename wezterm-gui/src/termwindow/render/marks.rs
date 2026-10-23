//! LBTerm: prompt marks on the scrollbar (lbterm_extras::marks). A tick on the track for each
//! shell prompt (OSC 133) in the active pane's scrollback; a click on one scrolls to it.

use crate::termwindow::TermWindow;
use lbterm_extras::marks::{self, PromptRows};
use mux::pane::{Pane, PaneId, WithPaneLines};
use std::collections::HashMap;
use std::sync::Arc;
use wezterm_term::{Line, SemanticType, StableRowIndex};
use window::color::LinearRgba;

/// A mark's colour: the coral of the tab panel's light
const MARK: (u8, u8, u8) = (0xff, 0x8a, 0x80);
/// How far a click may miss a mark and still land on it, in pixels at 96 DPI
const SLOP: f32 = 3.0;

#[derive(Default)]
struct PaneMarks {
    rows: PromptRows,
    /// The pane's sequence number when its rows were last looked at
    seqno: usize,
}

/// Where the marks were drawn, for clicks on them
struct Drawn {
    pane: PaneId,
    track_top: usize,
    track_height: usize,
    mark_height: usize,
    marks: Vec<StableRowIndex>,
    first_row: StableRowIndex,
    total_rows: usize,
}

#[derive(Default)]
pub struct MarksState {
    panes: HashMap<PaneId, PaneMarks>,
    drawn: Option<Drawn>,
}

struct LookForPrompts<'a>(&'a mut PromptRows);

impl WithPaneLines for LookForPrompts<'_> {
    fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]) {
        for (i, line) in lines.iter_mut().enumerate() {
            let prompt = line
                .semantic_zone_ranges()
                .iter()
                .any(|z| z.semantic_type == SemanticType::Prompt);
            self.0.set(first_row + i as StableRowIndex, prompt);
        }
    }
}

impl TermWindow {
    /// The pane's prompt marks. Only the rows that changed since the last look are read, so
    /// a long scrollback costs once, the first time.
    fn lbterm_prompt_marks(&mut self, pane: &Arc<dyn Pane>) -> Vec<StableRowIndex> {
        let dims = pane.get_dimensions();
        let first = dims.scrollback_top;
        let end = dims.physical_top + dims.viewport_rows as StableRowIndex;
        let entry = self.lbterm_marks.panes.entry(pane.pane_id()).or_default();
        let current = pane.get_current_seqno();
        if current != entry.seqno {
            let changed = pane.get_changed_since(first..end, entry.seqno);
            for range in changed.iter() {
                pane.with_lines_mut(range.clone(), &mut LookForPrompts(&mut entry.rows));
            }
            entry.rows.forget_before(first);
            entry.seqno = current;
        }
        entry.rows.marks()
    }

    /// Draw the active pane's marks on the scrollbar's track: `track_top` and `track_height`
    /// are the track the thumb moves in, `x` and `width` the scrollbar's column
    pub(crate) fn paint_lbterm_marks(
        &mut self,
        layers: &mut crate::quad::TripleLayerQuadAllocator,
        pane: &Arc<dyn Pane>,
        x: f32,
        width: f32,
        track_top: usize,
        track_height: usize,
    ) -> anyhow::Result<()> {
        let rows = self.lbterm_prompt_marks(pane);
        // Forget panes that have closed
        if self.lbterm_marks.panes.len() > 1 {
            let mux = mux::Mux::get();
            self.lbterm_marks
                .panes
                .retain(|id, _| mux.get_pane(*id).is_some());
        }
        let dims = pane.get_dimensions();
        let scale = self.dimensions.dpi as f32 / 96.0;
        let mark_height = (2.0 * scale).round().max(1.0) as usize;
        let ys = marks::track_positions(
            &rows,
            dims.scrollback_top,
            dims.scrollback_rows,
            track_height,
            mark_height,
        );
        let inset = (width * 0.2).round();
        let colour = LinearRgba::with_srgba(MARK.0, MARK.1, MARK.2, 0xe6);
        for y in ys {
            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    x + inset,
                    (track_top + y) as f32,
                    (width - 2.0 * inset).max(1.0),
                    mark_height as f32,
                ),
                colour,
            )?;
        }
        self.lbterm_marks.drawn = Some(Drawn {
            pane: pane.pane_id(),
            track_top,
            track_height,
            mark_height,
            marks: rows,
            first_row: dims.scrollback_top,
            total_rows: dims.scrollback_rows,
        });
        Ok(())
    }

    /// A click on the scrollbar's track: on a mark, scroll so its prompt is at the top and
    /// return true; anywhere else, false (and the click pages as usual)
    pub(crate) fn lbterm_mark_click(&mut self, pane: &Arc<dyn Pane>, y: isize) -> bool {
        let Some(drawn) = &self.lbterm_marks.drawn else {
            return false;
        };
        if drawn.pane != pane.pane_id() || y < drawn.track_top as isize {
            return false;
        }
        let scale = self.dimensions.dpi as f32 / 96.0;
        let Some(row) = marks::mark_at(
            &drawn.marks,
            drawn.first_row,
            drawn.total_rows,
            drawn.track_height,
            drawn.mark_height,
            y as usize - drawn.track_top,
            (SLOP * scale).round() as usize,
        ) else {
            return false;
        };
        let dims = pane.get_dimensions();
        self.set_viewport(pane.pane_id(), Some(row), dims);
        // LBTerm: the prompt it scrolled to is marked for a moment, as a prompt jump's is
        self.lbterm_mark_landed(pane.pane_id(), row);
        true
    }
}

/// LBTerm: the scrollbar's marks, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// How many prompt marks the scrollbar showed for `pane` when it last drew them
    pub(crate) fn lbterm_state_marks(&self, pane: PaneId) -> usize {
        self.lbterm_marks
            .drawn
            .as_ref()
            .filter(|d| d.pane == pane)
            .map_or(0, |d| d.marks.len())
    }
}
