//! Brief light over the panes that answers an action (lbterm/DESIGN.md, "Other moments"): a
//! click's ripple, a copied or opened stretch of text lighting up, and the readout of a pane's
//! size in cells while the window is resized or the font changes. This is where each goes; the
//! compositor plays them (wezterm-gui's render/lights.rs).

use crate::tabs::Rect;
use std::ops::Range;
use std::time::{Duration, Instant};

/// How far a ripple grows from where the click landed, and how big it starts, at 96 DPI
const PRESS_REACH: f64 = 40.0;
const PRESS_START: f64 = 4.0;
/// The corners of a lit stretch of text, at 96 DPI
const TEXT_RADIUS: f64 = 3.0;
/// How long the readout stays after the window's size last changed, and after the font's
const SIZE_FOR: Duration = Duration::from_millis(700);
const FONT_FOR: Duration = Duration::from_millis(1200);
/// The readout's card: in from its edges, at 96 DPI
const READOUT_PAD_X: f64 = 14.0;
const READOUT_PAD_Y: f64 = 8.0;

/// A ripple from `(x, y)`: how far it grows and how small it starts, in pixels.
pub fn press_reach(scale: f64) -> (f64, f64) {
    (PRESS_REACH * scale, PRESS_START * scale)
}

/// The corners of a lit stretch of text, in pixels.
pub fn text_radius(scale: f64) -> f64 {
    TEXT_RADIUS * scale
}

/// Where a pane's cells are in the window: its first cell's top-left, in pixels, and a cell's
/// size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cells {
    pub x: f64,
    pub y: f64,
    pub cell_w: f64,
    pub cell_h: f64,
    /// The pane's size in cells, to keep a stretch on it
    pub cols: usize,
    pub rows: usize,
}

/// The rectangles that light a stretch of text: one per row on screen of `spans` (each a row
/// down from the top of the pane's screen and the columns on it), rows off the screen left out.
/// Columns are cut to the pane, and pixels rounded out to whole ones.
pub fn text_rects(cells: Cells, spans: &[(isize, Range<usize>)]) -> Vec<Rect> {
    spans
        .iter()
        .filter(|(row, cols)| *row >= 0 && (*row as usize) < cells.rows && cols.start < cols.end)
        .filter_map(|(row, cols)| {
            let end = cols.end.min(cells.cols);
            if cols.start >= end {
                return None;
            }
            let x0 = (cells.x + cols.start as f64 * cells.cell_w).floor();
            let x1 = (cells.x + end as f64 * cells.cell_w).ceil();
            let y0 = (cells.y + *row as f64 * cells.cell_h).floor();
            let y1 = (cells.y + (*row + 1) as f64 * cells.cell_h).ceil();
            Some(Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        })
        .collect()
}

/// What the readout says: the pane's size in cells, and the font's against the config's when
/// it's the font that changed (`Some(1.1)` is 110%).
pub fn readout_text(cols: usize, rows: usize, font_scale: Option<f64>) -> String {
    let size = format!("{cols} × {rows}");
    match font_scale {
        Some(scale) => format!("{}% · {size}", (scale * 100.0).round() as i64),
        None => size,
    }
}

/// The readout's card, `text_w` by `text_h` of words, in the middle of `room`.
pub fn readout_card(room: Rect, text_w: f64, text_h: f64, scale: f64) -> Rect {
    let w = (text_w + 2.0 * READOUT_PAD_X * scale).ceil();
    let h = (text_h + 2.0 * READOUT_PAD_Y * scale).ceil();
    Rect {
        x: (room.x + (room.w - w) / 2.0).round(),
        y: (room.y + (room.h - h) / 2.0).round(),
        w,
        h,
    }
}

/// Where the readout's words start in its card.
pub fn readout_text_at(card: Rect, scale: f64) -> (f64, f64) {
    (card.x + READOUT_PAD_X * scale, card.y + READOUT_PAD_Y * scale)
}

/// When the readout shows: from the window's size or the font changing until a while after it
/// last did.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Readout {
    until: Option<Instant>,
    /// It was the font that changed last
    font: bool,
}

impl Readout {
    /// The window's size changed (its frame dragged, maximised, snapped). The font's change
    /// resizes the panes too, so its readout keeps saying so.
    pub fn resized(&mut self, now: Instant) {
        let font_showing = self.font && self.showing(now);
        let until = now + if font_showing { FONT_FOR } else { SIZE_FOR };
        self.until = Some(self.until.map_or(until, |u| u.max(until)));
        self.font = font_showing;
    }

    /// The font got bigger, smaller or back to the config's.
    pub fn font(&mut self, now: Instant) {
        self.until = Some(now + FONT_FOR);
        self.font = true;
    }

    pub fn showing(&self, now: Instant) -> bool {
        self.until.map_or(false, |u| now < u)
    }

    /// Whether it says the font's size too.
    pub fn says_font(&self) -> bool {
        self.font
    }

    /// When it next stops showing, for a repaint then.
    pub fn until(&self) -> Option<Instant> {
        self.until
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn cells() -> Cells {
        Cells {
            x: 76.0,
            y: 40.5,
            cell_w: 9.0,
            cell_h: 19.0,
            cols: 100,
            rows: 30,
        }
    }

    #[test]
    fn a_stretch_of_text_lights_its_cells_row_by_row() {
        let rects = text_rects(cells(), &[(2, 10..30), (3, 0..5)]);
        assert_eq!(
            rects,
            vec![
                Rect { x: 166.0, y: 78.0, w: 180.0, h: 20.0 },
                Rect { x: 76.0, y: 97.0, w: 45.0, h: 20.0 },
            ]
        );
    }

    #[test]
    fn rows_off_the_screen_and_columns_past_the_pane_are_left_out() {
        let rects = text_rects(cells(), &[(-1, 0..10), (30, 0..10), (5, 95..140), (6, 120..130), (7, 4..4)]);
        assert_eq!(rects, vec![Rect { x: 931.0, y: 135.0, w: 45.0, h: 20.0 }]);
    }

    #[test]
    fn the_readout_says_the_size_and_the_font_when_it_changed() {
        assert_eq!(readout_text(120, 32, None), "120 × 32");
        assert_eq!(readout_text(109, 29, Some(1.1)), "110% · 109 × 29");
        assert_eq!(readout_text(132, 35, Some(1.0 / 1.1)), "91% · 132 × 35");
    }

    #[test]
    fn the_readout_sits_in_the_middle_of_the_room() {
        let room = Rect { x: 72.0, y: 40.0, w: 1000.0, h: 600.0 };
        let card = readout_card(room, 72.0, 15.0, 1.0);
        assert_eq!(card, Rect { x: 522.0, y: 325.0, w: 100.0, h: 31.0 });
        assert_eq!(readout_text_at(card, 1.0), (536.0, 333.0));
        // At 150%, padded half as much again
        let card = readout_card(room, 108.0, 22.0, 1.5);
        assert_eq!((card.w, card.h), (150.0, 46.0));
    }

    #[test]
    fn the_readout_stays_a_while_after_the_last_change() {
        let now = Instant::now();
        let mut r = Readout::default();
        assert!(!r.showing(now));
        r.resized(now);
        assert!(r.showing(now + Duration::from_millis(600)));
        // Dragging on keeps it up
        r.resized(now + Duration::from_millis(500));
        assert!(r.showing(now + Duration::from_millis(1100)));
        assert!(!r.showing(now + Duration::from_millis(1300)));
        assert!(!r.says_font());
    }

    #[test]
    fn a_font_change_says_the_font_while_its_resize_follows() {
        let now = Instant::now();
        let mut r = Readout::default();
        r.font(now);
        // The window's panes get their new size a moment later
        r.resized(now + Duration::from_millis(20));
        assert!(r.says_font());
        assert!(r.showing(now + Duration::from_millis(1100)));
        // A drag long after is the size alone
        r.resized(now + Duration::from_millis(5000));
        assert!(!r.says_font());
        assert_eq!(r.until(), Some(now + Duration::from_millis(5700)));
    }

    #[test]
    fn a_ripple_grows_from_small_to_its_reach() {
        assert_eq!(press_reach(1.0), (40.0, 4.0));
        assert_eq!(press_reach(1.5), (60.0, 6.0));
        assert_eq!(text_radius(2.0), 6.0);
    }
}
