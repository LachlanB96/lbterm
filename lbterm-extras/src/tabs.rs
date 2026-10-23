//! Tabs with the mouse: drag one to move it along the bar, drag it out of the window to give
//! it a window of its own (or drop it on another LBTerm window's tabs), and the colours the
//! right-click menu offers.

/// The colours a tab can be given, by name: muted enough to sit beside the status pills
pub const COLOURS: [(&str, (u8, u8, u8)); 8] = [
    ("Red", (0xe5, 0x53, 0x4b)),
    ("Orange", (0xe0, 0x82, 0x3d)),
    ("Yellow", (0xc6, 0x90, 0x26)),
    ("Green", (0x57, 0xab, 0x5a)),
    ("Teal", (0x39, 0xa5, 0x9c)),
    ("Blue", (0x53, 0x9b, 0xf5)),
    ("Purple", (0xb0, 0x83, 0xf0)),
    ("Pink", (0xe2, 0x75, 0xad)),
];

/// How far the pointer travels with the button down, in pixels at 96 DPI, before a press on a
/// tab becomes a drag (so a click that wobbles is still a click)
pub const DRAG_START: f64 = 6.0;

/// How far outside the window the pointer must be, in pixels at 96 DPI, for letting go to tear
/// the tab off rather than leave it where it is
pub const TEAR_OFF: f64 = 24.0;

/// A rectangle on the screen, in pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// How far outside the rectangle a point is (0 inside)
    pub fn distance(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(x - (self.x + self.w)).max(0.0);
        let dy = (self.y - y).max(y - (self.y + self.h)).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

/// Whether the pointer has moved far enough from the press to make it a drag
pub fn dragging(from: (f64, f64), to: (f64, f64), scale: f64) -> bool {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    (dx * dx + dy * dy).sqrt() >= DRAG_START * scale
}

/// The slot a dragged tab moves to, from the pointer's x and each tab's span along the bar
/// (left edge and width, in tab order): the tab under the pointer, or the first or last when
/// the pointer is past either end
pub fn slot(spans: &[(f64, f64)], x: f64) -> Option<usize> {
    if spans.is_empty() {
        return None;
    }
    for (i, &(left, width)) in spans.iter().enumerate() {
        if x < left + width {
            return Some(i);
        }
    }
    Some(spans.len() - 1)
}

/// Where a tab dragged out of its window goes when it's let go
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drop {
    /// Still on or near its window: stays where the drag left it
    Stay,
    /// Into another LBTerm window, the one under the pointer
    OtherWindow(usize),
    /// A window of its own, its top-left at the pointer
    NewWindow { x: f64, y: f64 },
}

/// Where a tab goes when let go at `pointer`: `own` is its window's rectangle; `others` the
/// rectangles of LBTerm's other windows, front to back, each with an id
pub fn drop_at(own: Rect, others: &[(usize, Rect)], pointer: (f64, f64), scale: f64) -> Drop {
    let (x, y) = pointer;
    if own.distance(x, y) < TEAR_OFF * scale {
        return Drop::Stay;
    }
    if let Some((id, _)) = others.iter().find(|(_, r)| r.contains(x, y)) {
        return Drop::OtherWindow(*id);
    }
    Drop::NewWindow { x, y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wobble_is_still_a_click() {
        assert!(!dragging((100.0, 10.0), (103.0, 12.0), 1.0));
        assert!(dragging((100.0, 10.0), (107.0, 10.0), 1.0));
        // twice the pixels at 192 DPI
        assert!(!dragging((100.0, 10.0), (107.0, 10.0), 2.0));
    }

    #[test]
    fn the_slot_under_the_pointer() {
        let spans = [(0.0, 100.0), (100.0, 120.0), (220.0, 80.0)];
        assert_eq!(slot(&spans, 50.0), Some(0));
        assert_eq!(slot(&spans, 150.0), Some(1));
        assert_eq!(slot(&spans, 299.0), Some(2));
        // past either end
        assert_eq!(slot(&spans, -40.0), Some(0));
        assert_eq!(slot(&spans, 900.0), Some(2));
        assert_eq!(slot(&[], 10.0), None);
    }

    #[test]
    fn letting_go_near_the_window_keeps_the_tab() {
        let own = Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 };
        assert_eq!(drop_at(own, &[], (400.0, 300.0), 1.0), Drop::Stay);
        assert_eq!(drop_at(own, &[], (810.0, 300.0), 1.0), Drop::Stay);
    }

    #[test]
    fn far_off_it_gets_a_window_or_joins_one() {
        let own = Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 };
        let other = Rect { x: 1000.0, y: 0.0, w: 800.0, h: 600.0 };
        assert_eq!(drop_at(own, &[(7, other)], (1200.0, 100.0), 1.0), Drop::OtherWindow(7));
        assert_eq!(
            drop_at(own, &[(7, other)], (900.0, 700.0), 1.0),
            Drop::NewWindow { x: 900.0, y: 700.0 }
        );
    }

    #[test]
    fn eight_named_colours() {
        assert_eq!(COLOURS.len(), 8);
        assert!(COLOURS.iter().all(|(name, _)| !name.is_empty()));
    }
}
