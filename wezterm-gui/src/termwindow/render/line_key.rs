//! LBTerm: a pane's line's key in the line quad cache, and the pixel row it's drawn at
//! (render/pane.rs's render_line). Since perf/line-quads-by-content the key is what the line
//! shows, not where it is: a line is laid out at row 0 and moved to its row as its quads are
//! copied into the layer (quad.rs, `HeapQuadAllocator::lbterm_apply_moved`). So the row goes into
//! the key only through what's drawn on it there, the cursor and the selected columns, each as it
//! is on that row; the row itself comes out beside the key, never in it (lbterm/FLOWS.md rows 85
//! to 87 and the list under segment 4's table; lbterm/PITFALLS.md, 128).

use crate::selection::SelectionRange;
use crate::termwindow::render::{CursorProperties, LineQuadCacheKey};
use mux::pane::PaneId;
use mux::renderable::StableCursorPosition;
use ordered_float::NotNan;
use wezterm_term::StableRowIndex;
use window::color::LinearRgba;

/// What a pane's lines share in one frame, for each line's key and row
pub struct LineFrame<'a> {
    pub pane_id: PaneId,
    pub config_generation: usize,
    pub shape_generation: usize,
    /// Moved on every resize (resize.rs), so the window's size is in the key through it
    pub quad_generation: usize,
    /// The pane's left edge, in pixels from the window's
    pub left_pixel_x: f32,
    /// The window's pixel row the tab's rows start at, the pane's first row in the tab, and a
    /// row's height: where a line is drawn, never in its key
    pub top_pixel_y: f32,
    pub pos_top: usize,
    pub cell_height: f32,
    pub cols: usize,
    pub reverse_video: bool,
    pub cursor: &'a StableCursorPosition,
    pub selection: Option<SelectionRange>,
    pub rectangular: bool,
    /// Whether the pane has the keyboard, the window is drawn by the compositor and the window
    /// has the focus (`lbterm_extras::work::line_keyboard`)
    pub is_active: bool,
    pub composited: bool,
    pub focused: bool,
}

/// What only the cursor's row is drawn with, besides where the cursor is on it: asked for only
/// on that row (the password input is read from the pane's metadata)
pub struct CursorRow {
    pub dead_key_or_leader: bool,
    pub cursor_is_default_color: bool,
    pub cursor_fg: LinearRgba,
    pub cursor_bg: LinearRgba,
    pub cursor_border_color: LinearRgba,
    pub composing: Option<String>,
    pub password_input: bool,
}

impl LineFrame<'_> {
    /// The key of the line `line_idx` rows into the pane's view, at stable row `stable_row` and
    /// showing `shape_hash`, and the window's pixel row it's drawn at. Two rows showing the same
    /// line with neither the cursor nor a selection on them give the same key; the cursor's row
    /// and a selected row give their own, which follow the cursor and the selection to whatever
    /// row they're on
    pub fn line(
        &self,
        line_idx: usize,
        stable_row: StableRowIndex,
        shape_hash: [u8; 16],
        cursor_row: impl FnOnce() -> CursorRow,
    ) -> (LineQuadCacheKey, f32) {
        let selection = self
            .selection
            .map_or(0..0, |sel| sel.cols_for_row(stable_row, self.rectangular));
        // Constrain to the pane width!
        let selection = selection.start..selection.end.min(self.cols);

        let (cursor, composing, password_input) = if self.cursor.y == stable_row {
            let row = cursor_row();
            (
                Some(CursorProperties {
                    // The row left out: the cursor is on this line, wherever the line is
                    position: StableCursorPosition {
                        y: 0,
                        ..*self.cursor
                    },
                    dead_key_or_leader: row.dead_key_or_leader,
                    cursor_fg: row.cursor_fg,
                    cursor_bg: row.cursor_bg,
                    cursor_border_color: row.cursor_border_color,
                    cursor_is_default_color: row.cursor_is_default_color,
                }),
                row.composing,
                row.password_input,
            )
        } else {
            (None, None, false)
        };

        // Only the cursor's line goes by the keyboard, with the compositor, so a move of it
        // between panes or windows makes only the cursors' lines again
        let (pane_is_active, lbterm_focused) = lbterm_extras::work::line_keyboard(
            self.is_active,
            cursor.is_some(),
            self.composited,
            self.focused,
        );

        let row_y = self.top_pixel_y + (line_idx + self.pos_top) as f32 * self.cell_height;
        let key = LineQuadCacheKey {
            pane_id: self.pane_id,
            password_input,
            pane_is_active,
            lbterm_focused,
            config_generation: self.config_generation,
            shape_generation: self.shape_generation,
            quad_generation: self.quad_generation,
            composing,
            selection,
            cursor,
            shape_hash,
            left_pixel_x: NotNan::new(self.left_pixel_x).unwrap(),
            reverse_video: self.reverse_video,
        };
        (key, row_y)
    }
}

/// The key render_line makes, with the row it's drawn at. Run by `lbterm.ps1 test -Package
/// wezterm-gui`, which the check doesn't; lbterm-motion's content.rs holds the moved hashes in the
/// check, and perf/line-quads.ps1 the whole of it in a window
#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::SelectionCoordinate;
    use termwiz::surface::{CursorShape, CursorVisibility};

    const ROW: f32 = 19.0;

    fn cursor_at(x: usize, y: StableRowIndex) -> StableCursorPosition {
        StableCursorPosition {
            x,
            y,
            shape: CursorShape::Default,
            visibility: CursorVisibility::Visible,
        }
    }

    fn selected(x: (usize, usize), rows: (StableRowIndex, StableRowIndex)) -> Option<SelectionRange> {
        Some(SelectionRange {
            start: SelectionCoordinate::x_y(x.0, rows.0),
            end: SelectionCoordinate::x_y(x.1, rows.1),
        })
    }

    /// A frame of a pane whose view starts at stable row 100, drawn 40 px down the window
    fn frame<'a>(cursor: &'a StableCursorPosition, selection: Option<SelectionRange>) -> LineFrame<'a> {
        LineFrame {
            pane_id: 7,
            config_generation: 1,
            shape_generation: 2,
            quad_generation: 3,
            left_pixel_x: 12.0,
            top_pixel_y: 40.0,
            pos_top: 0,
            cell_height: ROW,
            cols: 80,
            reverse_video: false,
            cursor,
            selection,
            rectangular: false,
            is_active: true,
            composited: true,
            focused: true,
        }
    }

    fn looks() -> CursorRow {
        let c = LinearRgba::with_components(0.5, 0.5, 0.5, 1.0);
        CursorRow {
            dead_key_or_leader: false,
            cursor_is_default_color: true,
            cursor_fg: c,
            cursor_bg: c,
            cursor_border_color: c,
            composing: None,
            password_input: false,
        }
    }

    /// The line `line_idx` rows into the view (stable row 100 + it), showing `shape`
    fn at(f: &LineFrame, line_idx: usize, shape: u8) -> (LineQuadCacheKey, f32) {
        f.line(line_idx, 100 + line_idx as StableRowIndex, [shape; 16], looks)
    }

    #[test]
    fn the_same_line_at_two_rows_has_one_key_and_two_rows() {
        let cursor = cursor_at(4, 150);
        let f = frame(&cursor, None);
        let (twelve, y12) = at(&f, 12, 0xa);
        let (eleven, y11) = at(&f, 11, 0xa);
        assert_eq!(twelve, eleven, "a line moved a row keeps its key");
        assert_eq!(y12, 40.0 + 12.0 * ROW);
        assert_eq!(y12 - y11, ROW, "and is drawn a row higher");
        assert_ne!(at(&f, 11, 0xb).0, eleven, "another line in its place misses");
        // The pane moved down the tab (a split above it): the same key, drawn lower
        let lower = LineFrame { pos_top: 5, ..frame(&cursor, None) };
        let (moved, y) = at(&lower, 12, 0xa);
        assert_eq!(moved, twelve);
        assert_eq!(y, y12 + 5.0 * ROW);
    }

    #[test]
    fn the_cursors_row_has_its_own_key_which_follows_the_cursor() {
        let cursor = cursor_at(4, 112);
        let f = frame(&cursor, None);
        let (on, _) = at(&f, 12, 0xa);
        let (off, _) = at(&f, 13, 0xa);
        assert_ne!(on, off, "the cursor's row misses the same line without it");
        assert_eq!(on.cursor.as_ref().map(|c| c.position.y), Some(0), "the cursor's row left out");
        // The line and its cursor moved up a row together (a program scrolled): the same key
        let up = cursor_at(4, 111);
        assert_eq!(at(&frame(&up, None), 11, 0xa).0, on);
        // The cursor at another column on the same line: another key
        let right = cursor_at(5, 112);
        assert_ne!(at(&frame(&right, None), 12, 0xa).0, on);
    }

    #[test]
    fn only_the_cursors_row_asks_what_it_draws_with() {
        let cursor = cursor_at(0, 112);
        let f = frame(&cursor, None);
        let asked = std::cell::Cell::new(0);
        for line_idx in 0..30 {
            f.line(line_idx, 100 + line_idx as StableRowIndex, [1; 16], || {
                asked.set(asked.get() + 1);
                looks()
            });
        }
        assert_eq!(asked.get(), 1);
        let (key, _) = f.line(12, 112, [1; 16], || CursorRow {
            composing: Some("é".to_string()),
            password_input: true,
            ..looks()
        });
        assert_eq!(key.composing.as_deref(), Some("é"));
        assert!(key.password_input);
        assert_ne!(key, at(&f, 12, 1).0, "the composing text and the password input are in it");
    }

    #[test]
    fn a_selected_row_has_its_own_key_which_follows_the_selection() {
        let cursor = cursor_at(0, 150);
        let f = frame(&cursor, selected((2, 9), (112, 112)));
        let (sel, _) = at(&f, 12, 0xa);
        let (plain, _) = at(&f, 13, 0xa);
        assert_eq!(sel.selection, 2..10);
        assert_ne!(sel, plain, "a selected row misses the same line unselected");
        // The selection and the line moved up a row together: the same key
        let up = frame(&cursor, selected((2, 9), (111, 111)));
        assert_eq!(at(&up, 11, 0xa).0, sel);
        // Clipped to the pane
        let wide = frame(&cursor, selected((70, 200), (112, 112)));
        assert_eq!(at(&wide, 12, 0xa).0.selection, 70..80);
    }
}
