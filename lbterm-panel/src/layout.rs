//! Where the panel's parts go, in window pixels.
//!
//! The panel is a card the colour of the panes, ringed with a grey hairline
//! at rest and a red gradient that glows while it's open, a gutter in from
//! the window's left edge and the tab bar (lbterm/DESIGN.md, "Shape").
//! Closed it's the rail, a column of status pads; open it's `width` wide,
//! and the panes move right to make room. The window gives the closed panel
//! a strip down its left (gutter, rail) as though it were border, and the
//! panes' cards sit a gutter past it, as they do from every edge.
//!
//! With the compositor nothing LBTerm draws moves when the panel opens: the
//! panel is drawn open, and DWM shows two slices of it. The body runs from
//! the left up to the `seam`, which moves with the edge; the cap, the ring's
//! two right-hand corners, rides on the edge itself. So everything in the
//! body that shows while closed sits left of the closed seam. The panes'
//! own cards are what shrink to make room ([`Layout::panes_left`]), and
//! each time it opens, a light runs once round the ring
//! ([`Layout::sweep_point`]).
//!
//! Sizes are in pixels at 96 DPI and scale with the window's DPI.

use crate::shape::Shape;

/// A rectangle in window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Bigger by `by` on every side.
    pub fn grow(&self, by: f32) -> Area {
        Area::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }
}

/// How far the glow reaches past the ring
const GLOW: f32 = 26.0;
/// From the ring's inside to the first row
const LIST_PAD: f32 = 8.0;
/// A row's height when there's room, and the least it shrinks to
const ROW_MAX: f32 = 46.0;
const ROW_MIN: f32 = 28.0;
/// Between one row's highlight and the next
const ROW_GAP: f32 = 2.0;
const PAD_W: f32 = 22.0;
const PAD_H: f32 = 20.0;
/// The idle timer's bars, stacked above the pad as on the APC and in
/// TabPanel.exe: as wide as the pad, this tall, this far apart
const BAR_H: f32 = 3.0;
const BAR_GAP: f32 = 2.0;
const BARS_TO_PAD: f32 = 2.0;
/// Half the width of a row's highlight on the closed rail, round its pad
const PILL_HALF: f32 = 15.0;
/// The highlight's corners, round a pad, and round a pad and its label
const PILL_RADIUS: f32 = 14.0;
const LABEL_PILL_RADIUS: f32 = 10.0;
/// Between a pad and its label, and the label in from the pill's sides
const LABEL_GAP: f32 = 3.0;
const LABEL_INSET: f32 = 2.0;
/// The turning arc's square, on a pad's top right corner
const SPINNER: f32 = 12.0;
/// The open tab's bar down the rail's left edge, beside its pill: this wide,
/// this far in from the ring, and this much shorter than the pill at each end
const EDGE_BAR_W: f32 = 3.0;
const EDGE_BAR_IN: f32 = 1.0;
const EDGE_BAR_END: f32 = 8.0;
/// From the pill to the words, and from the words to the open highlight's end
const TEXT_GAP: f32 = 8.0;
const TEXT_RIGHT: f32 = 10.0;
/// The soft edge either side of the ring's stroke
pub use crate::shape::FEATHER;
/// The light running round the ring: its halo, and its bright middle
const SPOT: f32 = 64.0;
const SPOT_CORE: f32 = 10.0;
/// Between the Conductor's slot and the first tab's, more than between two tabs: room for the
/// hairline rule that keeps them apart
const HEAD_GAP: f32 = 7.0;
/// The mark on the Conductor's pad for its remote control, on the pad's top right corner
const HEAD_MARK: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The window's left edge, inside any OS border
    pub left: f32,
    /// Under the tab bar
    pub body_top: f32,
    /// The window's bottom, inside any OS border and any tab bar there
    pub bottom: f32,
    /// The window's right edge, inside any OS border
    pub right: f32,
    /// The closed panel's width, ring included
    pub rail: f32,
    /// The open panel's width, ring included
    pub width: f32,
    /// DPI / 96
    pub scale: f32,
    /// One per tab
    pub rows: usize,
    /// The labels under the pads, their lines together; 0 without labels
    pub label: f32,
    /// A slot above the tabs' for the Conductor's pad (lbterm_extras::conductor)
    pub head: bool,
}

impl Layout {
    /// `v` pixels at 96 DPI, in this window's pixels.
    pub fn px(&self, v: f32) -> f32 {
        (v * self.scale).round().max(if v > 0.0 { 1.0 } else { 0.0 })
    }

    fn shape(&self) -> Shape {
        Shape::new(self.scale)
    }

    /// From the window's edges and the tab bar, and to the panes' cards:
    /// the gutter.
    pub fn gap(&self) -> f32 {
        self.shape().gutter()
    }

    /// The ring: a hairline.
    pub fn stroke(&self) -> f32 {
        self.shape().hairline()
    }

    pub fn radius(&self) -> f32 {
        self.shape().radius()
    }

    pub fn glow(&self) -> f32 {
        self.px(GLOW)
    }

    /// Never narrower open than closed.
    fn open_width(&self) -> f32 {
        self.width.max(self.rail)
    }

    /// The strip the window keeps for the closed panel: gap, rail. The
    /// panes' cards sit a gutter past it.
    pub fn strip(&self) -> f32 {
        self.gap() + self.rail
    }

    /// Where the panes' visuals start while the panel is closed: half a
    /// gutter short of the first card. Opening presses everything from here
    /// on over by [`Layout::travel`], so the first card ends up a gutter
    /// past the open panel.
    pub fn panes_left(&self) -> f32 {
        self.left + self.strip() + self.gap() - self.shape().half_gutter()
    }

    /// Where the panes' visuals end: half a gutter past the last card, a
    /// gutter in from the window's right edge.
    pub fn panes_right(&self) -> f32 {
        self.right - self.gap() + self.shape().half_gutter()
    }

    /// How far the edge moves between closed and open.
    pub fn travel(&self) -> f32 {
        self.open_width() - self.rail
    }

    /// The panel's outside edge, ring included.
    pub fn panel(&self, open: bool) -> Area {
        let gap = self.gap();
        let y = self.body_top + gap;
        Area::new(
            self.left + gap,
            y,
            if open { self.open_width() } else { self.rail },
            (self.bottom - gap - y).max(0.0),
        )
    }

    /// Where the body meets the cap: the start of the ring's right-hand
    /// corners.
    pub fn seam(&self, open: bool) -> f32 {
        self.panel(open).right() - self.radius()
    }

    /// The surfaces of the body, its glow and the words: the open panel and
    /// its glow, from the window's left edge, under the tab bar.
    pub fn surface(&self) -> Area {
        let right = self.panel(true).right() + self.glow();
        Area::new(self.left, self.body_top, right - self.left, (self.bottom - self.body_top).max(0.0))
    }

    /// The surfaces of the cap and its glow, where they sit open.
    pub fn cap_surface(&self) -> Area {
        let surface = self.surface();
        let seam = self.seam(true);
        Area::new(seam, surface.y, surface.right() - seam, surface.h)
    }

    /// Whether a point is on the panel: the panel and the gutter past it,
    /// closed or open.
    pub fn inside(&self, x: f32, y: f32, open: bool) -> bool {
        let right = self.panel(open).right() + self.gap();
        x >= self.left && x < right && y >= self.body_top && y < self.bottom
    }

    /// Under a pad, the label and the gap above it; nothing without labels.
    fn label_block(&self) -> f32 {
        if self.label > 0.0 {
            self.px(LABEL_GAP) + self.label
        } else {
            0.0
        }
    }

    /// Each row's height: as tall as they come when there's room, shrinking
    /// to fit more tabs. The Conductor's slot is a row too.
    pub fn row_height(&self) -> f32 {
        let label = self.label_block();
        let (min, max) = (self.px(ROW_MIN) + label, self.px(ROW_MAX) + label);
        let slots = self.rows + self.head as usize;
        if slots == 0 {
            return max;
        }
        let room = self.panel(false).h - 2.0 * (self.stroke() + self.px(LIST_PAD)) - self.head_gap();
        (room / slots as f32).floor().clamp(min, max)
    }

    /// The extra room between the Conductor's slot and the tabs, without one none
    fn head_gap(&self) -> f32 {
        if self.head {
            self.px(HEAD_GAP)
        } else {
            0.0
        }
    }

    /// The top of the list: the Conductor's slot, or else the first tab's
    fn top_row(&self) -> f32 {
        self.panel(false).y + self.stroke() + self.px(LIST_PAD)
    }

    fn first_row(&self) -> f32 {
        let head = if self.head {
            self.row_height() + self.head_gap()
        } else {
            0.0
        };
        self.top_row() + head
    }

    fn row_band(&self, i: usize) -> (f32, f32) {
        let h = self.row_height();
        (self.first_row() + i as f32 * h, h - self.px(ROW_GAP))
    }

    fn head_band(&self) -> (f32, f32) {
        (self.top_row(), self.row_height() - self.px(ROW_GAP))
    }

    /// Row `i`'s slot: the whole of its height, gap included, across the open panel. Everything
    /// the row shows on the rail is inside it, so the row moves as one when tabs come, go or
    /// change places.
    pub fn slot(&self, i: usize) -> Area {
        let h = self.row_height();
        let panel = self.panel(true);
        // The gap between rows split between the two, so what's soft round a pill (its ring's
        // feather) is in its own row's slot and goes where the row goes
        let above = (self.px(ROW_GAP) / 2.0).floor();
        Area::new(panel.x, self.first_row() + i as f32 * h - above, panel.w, h)
    }

    /// The Conductor's slot, as a row's: the open tab's ring and bar glide to it while its tab
    /// is the open one
    pub fn head_slot(&self) -> Area {
        let panel = self.panel(true);
        let above = (self.px(ROW_GAP) / 2.0).floor();
        Area::new(panel.x, self.top_row() - above, panel.w, self.row_height())
    }

    /// Whether row `i` is on screen at all.
    pub fn row_visible(&self, i: usize) -> bool {
        let (y, h) = self.row_band(i);
        let panel = self.panel(false);
        i < self.rows && y + h <= panel.bottom() - self.stroke()
    }

    /// The middle of the rail, where the pads line up.
    fn column(&self) -> f32 {
        (self.panel(false).x + self.rail / 2.0).round()
    }

    /// Half a pill's width: round the pad, or with labels as wide as the
    /// closed rail lets it be, centred on the pads and left of the seam
    /// (what's right of it is the cap's, the ring's right-hand corners), clear
    /// of the open tab's bar.
    fn pill_half(&self) -> f32 {
        let half = self.px(PILL_HALF);
        if self.label <= 0.0 {
            return half;
        }
        let right = self.seam(false) - self.px(1.0) - self.column();
        let left = self.column() - self.edge_bar_x() - self.px(EDGE_BAR_W) - self.px(1.0);
        right.min(left).floor().max(half)
    }

    /// The pills' corners, and the open highlight's.
    pub fn pill_radius(&self) -> f32 {
        self.px(if self.label > 0.0 { LABEL_PILL_RADIUS } else { PILL_RADIUS })
    }

    /// Row `i`'s highlight on the closed rail: a pill round its pad, and its
    /// label with labels on.
    pub fn pill(&self, i: usize) -> Area {
        self.pill_in(self.row_band(i))
    }

    fn pill_in(&self, (y, h): (f32, f32)) -> Area {
        let half = self.pill_half();
        Area::new(self.column() - half, y, 2.0 * half, h)
    }

    /// The Conductor's pill, at the top of the rail: a tab's shape
    pub fn head_pill(&self) -> Area {
        self.pill_in(self.head_band())
    }

    /// The Conductor's pad, holding its glyph, with its label under it; no timer
    pub fn head_pad(&self) -> Area {
        self.pad_in(self.head_pill(), false)
    }

    pub fn head_label(&self) -> Area {
        self.label_under(self.head_pad())
    }

    /// The mark for the Conductor's remote control: a square centred on its pad's top right
    /// corner, as the turning arc sits on a tab's
    pub fn head_mark(&self) -> Area {
        let pad = self.head_pad();
        let half = self.px(HEAD_MARK) / 2.0;
        let (cx, cy) = (pad.right() - self.px(2.0), pad.y + self.px(2.0));
        Area::new(cx - half, cy - half, 2.0 * half, 2.0 * half)
    }

    /// The hairline between the Conductor's slot and the tabs: across the middle of the rail,
    /// halfway between their pills
    pub fn head_rule(&self) -> Area {
        let (above, below) = (self.head_pill().bottom(), self.first_row());
        let half = (self.pill_half() * 0.8).round();
        let h = self.stroke();
        Area::new(self.column() - half, ((above + below - h) / 2.0).round(), 2.0 * half, h)
    }

    /// The Conductor's row on the open panel, from its pill on, as a tab's
    pub fn head_highlight(&self) -> Area {
        self.highlight_from(self.head_pill())
    }

    /// Where the Conductor's words go on the open panel
    pub fn head_text(&self) -> Area {
        self.text_beside(self.head_pill())
    }

    /// Whether a point is on the Conductor's slot, closed or open
    pub fn head_at(&self, x: f32, y: f32, open: bool) -> bool {
        let (top, h) = self.head_band();
        self.head && self.inside(x, y, open) && y >= top && y < top + h
    }

    fn edge_bar_x(&self) -> f32 {
        self.panel(false).x + self.stroke() + self.px(EDGE_BAR_IN)
    }

    /// The open tab's bar down the rail's left edge, beside row `i`'s pill.
    pub fn edge_bar(&self, i: usize) -> Area {
        self.edge_bar_beside(self.pill(i))
    }

    /// The open tab's bar beside the Conductor's pill, while its tab is the open one
    pub fn head_edge_bar(&self) -> Area {
        self.edge_bar_beside(self.head_pill())
    }

    fn edge_bar_beside(&self, pill: Area) -> Area {
        let end = self.px(EDGE_BAR_END);
        Area::new(self.edge_bar_x(), pill.y + end, self.px(EDGE_BAR_W), (pill.h - 2.0 * end).max(0.0))
    }

    /// Where row `i`'s label goes, under its pad: as wide as its pill, less
    /// an inset, `label` tall. Empty without labels.
    pub fn label(&self, i: usize) -> Area {
        self.label_under(self.pad(i))
    }

    fn label_under(&self, pad: Area) -> Area {
        if self.label <= 0.0 {
            return Area::new(pad.x, pad.bottom(), 0.0, 0.0);
        }
        let half = self.pill_half() - self.px(LABEL_INSET);
        Area::new(self.column() - half, pad.bottom() + self.px(LABEL_GAP), 2.0 * half, self.label)
    }

    /// Row `i`'s highlight on the open panel, from the pill on, ending short
    /// of the cap.
    pub fn highlight(&self, i: usize) -> Area {
        self.highlight_from(self.pill(i))
    }

    fn highlight_from(&self, pill: Area) -> Area {
        let right = self.seam(true) - self.px(2.0);
        Area::new(pill.x, pill.y, (right - pill.x).max(pill.w), pill.h)
    }

    /// The height of the bars above a pad, gap included.
    fn bars_height(&self) -> f32 {
        3.0 * self.px(BAR_H) + 2.0 * self.px(BAR_GAP) + self.px(BARS_TO_PAD)
    }

    /// Whether row `i` is tall enough for the timer above its pad.
    pub fn has_timer(&self, i: usize) -> bool {
        self.pill(i).h >= self.bars_height() + self.px(PAD_H) + self.px(4.0) + self.label_block()
    }

    /// The state pad, holding the tab's number: with the timer above it and
    /// the label below, the three centred in the row together.
    pub fn pad(&self, i: usize) -> Area {
        self.pad_in(self.pill(i), self.has_timer(i))
    }

    fn pad_in(&self, row: Area, timer: bool) -> Area {
        let h = self.px(PAD_H).min(row.h);
        let w = self.px(PAD_W);
        let above = if timer { self.bars_height() } else { 0.0 };
        let below = self.label_block();
        Area::new(
            self.column() - (w / 2.0).round(),
            (row.y + (row.h - h - above - below) / 2.0 + above).round(),
            w,
            h,
        )
    }

    /// Bar `bar` of the idle timer above the pad, 0 nearest it. Empty when
    /// the row is too short for the timer.
    pub fn bar(&self, i: usize, bar: usize) -> Area {
        let pad = self.pad(i);
        if !self.has_timer(i) {
            return Area::new(pad.x, pad.y, 0.0, 0.0);
        }
        let (h, gap) = (self.px(BAR_H), self.px(BAR_GAP));
        let bottom = pad.y - self.px(BARS_TO_PAD);
        Area::new(pad.x, bottom - h - bar as f32 * (h + gap), pad.w, h)
    }

    /// Where the words go in row `i`: past the pill, inside the open
    /// highlight.
    pub fn text(&self, i: usize) -> Area {
        self.text_beside(self.pill(i))
    }

    fn text_beside(&self, pill: Area) -> Area {
        let highlight = self.highlight_from(pill);
        let x = pill.right() + self.px(TEXT_GAP);
        let right = highlight.right() - self.px(TEXT_RIGHT);
        Area::new(x, pill.y, (right - x).max(0.0), pill.h)
    }

    /// A thin bar along the foot of row `i`'s words, for a program's
    /// progress.
    pub fn progress_bar(&self, i: usize) -> Area {
        let text = self.text(i);
        let h = self.px(2.0);
        Area::new(text.x, self.pill(i).bottom() - h - self.px(3.0), text.w, h)
    }

    /// The turning arc on row `i`'s pad while its program waits on tasks of
    /// its own: a square centred on the pad's top right corner, the arc and
    /// the disc of the panel's colour behind it inside, so it reads apart
    /// from the pad and turns about its own middle.
    pub fn spinner(&self, i: usize) -> Area {
        let pad = self.pad(i);
        let half = self.px(SPINNER) / 2.0;
        let (cx, cy) = (pad.right() - self.px(3.0), pad.y + self.px(3.0));
        Area::new(cx - half, cy - half, 2.0 * half, 2.0 * half)
    }

    /// The mark in the top right corner of row `i`'s pad for a result not
    /// yet seen.
    pub fn unread_dot(&self, i: usize) -> Area {
        let pad = self.pad(i);
        let d = self.px(6.0);
        Area::new(pad.right() - d - self.px(2.0), pad.y + self.px(2.0), d, d)
    }

    /// The row under a point, if any.
    pub fn row_at(&self, x: f32, y: f32, open: bool) -> Option<usize> {
        if !self.inside(x, y, open) || y < self.first_row() {
            return None;
        }
        let i = ((y - self.first_row()) / self.row_height()) as usize;
        self.row_visible(i).then_some(i)
    }

    /// Where the running light is, `progress` of the way round the open
    /// ring's middle line, 0 to 1: from the top of its top-left corner, down
    /// the left side first (which never moves), along the bottom, up the
    /// right side and back along the top. By the time it reaches the right
    /// side, the panel's edge has stopped there.
    pub fn sweep_point(&self, progress: f64) -> (f32, f32) {
        let ring = self.panel(true).grow(-self.stroke() / 2.0);
        outline_point(ring, self.radius() - self.stroke() / 2.0, progress)
    }

    /// The light's halo and its bright middle: surfaces centred where it
    /// starts, which DWM then carries round.
    pub fn spot_surface(&self) -> Area {
        self.around_start(self.px(SPOT))
    }

    pub fn spot_core_surface(&self) -> Area {
        self.around_start(self.px(SPOT_CORE))
    }

    fn around_start(&self, radius: f32) -> Area {
        let (x, y) = self.sweep_point(0.0);
        Area::new((x - radius).floor(), (y - radius).floor(), 2.0 * radius + 1.0, 2.0 * radius + 1.0)
    }
}

/// A point `progress` of the way round a rounded rectangle's outline `ring` (corners of radius
/// `radius`), 0 to 1, at an even speed: from the top of its top-left corner, down the left side
/// first, along the bottom, up the right side and back along the top. The panel's running light
/// goes round the ring this way, and the Conductor's light round a pill.
pub fn outline_point(ring: Area, radius: f32, progress: f64) -> (f32, f32) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let r = (radius as f64).min(ring.w as f64 / 2.0).min(ring.h as f64 / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (ring.x as f64, ring.y as f64, ring.right() as f64, ring.bottom() as f64);
    let (straight_w, straight_h) = ((x1 - x0 - 2.0 * r).max(0.0), (y1 - y0 - 2.0 * r).max(0.0));
    let arc = FRAC_PI_2 * r;
    // Corner arc: centre, angle it starts at; each turns a quarter
    // anticlockwise on screen (the angle falls, y running down)
    enum Leg {
        Arc(f64, f64, f64),
        Line(f64, f64, f64, f64),
    }
    let legs = [
        (arc, Leg::Arc(x0 + r, y0 + r, 1.5 * PI)),
        (straight_h, Leg::Line(x0, y0 + r, x0, y1 - r)),
        (arc, Leg::Arc(x0 + r, y1 - r, PI)),
        (straight_w, Leg::Line(x0 + r, y1, x1 - r, y1)),
        (arc, Leg::Arc(x1 - r, y1 - r, FRAC_PI_2)),
        (straight_h, Leg::Line(x1, y1 - r, x1, y0 + r)),
        (arc, Leg::Arc(x1 - r, y0 + r, 0.0)),
        (straight_w, Leg::Line(x1 - r, y0, x0 + r, y0)),
    ];
    let total: f64 = legs.iter().map(|(len, _)| len).sum();
    let mut along = progress.rem_euclid(1.0) * total;
    for (len, leg) in &legs {
        if along <= *len || std::ptr::eq(leg, &legs[legs.len() - 1].1) {
            let t = if *len > 0.0 { (along / len).min(1.0) } else { 0.0 };
            let (x, y) = match leg {
                Leg::Arc(cx, cy, from) => {
                    let angle = from - FRAC_PI_2 * t;
                    (cx + r * angle.cos(), cy + r * angle.sin())
                }
                Leg::Line(ax, ay, bx, by) => (ax + (bx - ax) * t, ay + (by - ay) * t),
            };
            return (x as f32, y as f32);
        }
        along -= len;
    }
    (x0 as f32, y0 as f32)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A window at 96 DPI with a 38 px tab bar and four tabs.
    fn layout() -> Layout {
        Layout {
            left: 0.0,
            body_top: 38.0,
            bottom: 800.0,
            right: 1000.0,
            rail: 68.0,
            width: 290.0,
            scale: 1.0,
            rows: 4,
            label: 0.0,
            head: false,
        }
    }

    #[test]
    fn a_rows_slot_holds_all_it_shows_and_slots_meet() {
        for head in [false, true] {
            let mut l = layout();
            l.head = head;
            l.label = 20.0;
            for i in 0..l.rows {
                let slot = l.slot(i);
                for part in [l.pill(i), l.pad(i), l.label(i), l.bar(i, 2), l.edge_bar(i), l.highlight(i)] {
                    assert!(
                        part.y >= slot.y && part.bottom() <= slot.bottom() && part.x >= slot.x && part.right() <= slot.right(),
                        "row {i}: {part:?} outside {slot:?}"
                    );
                }
                if i > 0 {
                    assert_eq!(l.slot(i - 1).bottom(), slot.y, "no gap or overlap between slots");
                }
            }
        }
        // A pill's ring is soft a pixel past it: still in its slot, at 96 DPI and at 192
        for scale in [1.0, 2.0] {
            let mut l = layout();
            l.scale = scale;
            l.label = 20.0;
            for i in 0..l.rows {
                let (soft, slot) = (l.pill(i).grow(l.px(FEATHER)), l.slot(i));
                assert!(soft.y >= slot.y && soft.bottom() <= slot.bottom(), "row {i} at {scale}: {soft:?} outside {slot:?}");
            }
        }
    }

    #[test]
    fn the_panel_sits_a_gutter_in_and_the_strip_holds_it() {
        let l = layout();
        let closed = l.panel(false);
        assert_eq!((closed.x, closed.y, closed.w), (4.0, 42.0, 68.0));
        assert_eq!(closed.bottom(), 796.0);
        assert_eq!(l.strip(), 72.0, "the gutter and the rail");
        assert_eq!(l.panel(true).w, 290.0);
        assert_eq!(l.travel(), 222.0);
        // A hairline ring with the design system's corners
        assert_eq!((l.stroke(), l.radius()), (1.0, 12.0));
    }

    #[test]
    fn the_panes_cards_start_a_gutter_past_the_panel_open_or_closed() {
        let l = layout();
        let half = 2.0;
        // The first card a gutter past the rail, its visual half a gutter short of it
        assert_eq!(l.panes_left() + half, l.panel(false).right() + l.gap());
        // Pressed over by the travel, it's a gutter past the open panel
        assert_eq!(l.panes_left() + half + l.travel(), l.panel(true).right() + l.gap());
        // The last card a gutter in from the window's right edge
        assert_eq!(l.panes_right() - half, 1000.0 - l.gap());
    }

    #[test]
    fn what_shows_while_closed_is_left_of_the_closed_seam() {
        let l = layout();
        let seam = l.seam(false);
        for i in 0..l.rows {
            for part in [l.pill(i), l.pad(i), l.bar(i, 0), l.bar(i, 1), l.bar(i, 2)] {
                assert!(part.right() <= seam, "{part:?} crosses the seam at {seam}");
                assert!(part.x >= l.panel(false).x + l.stroke(), "{part:?}");
                assert!(part.w > 0.0 && part.h > 0.0, "{part:?}");
            }
        }
    }

    #[test]
    fn the_open_highlight_and_words_stop_short_of_the_cap() {
        let l = layout();
        for i in 0..l.rows {
            assert!(l.highlight(i).right() < l.seam(true));
            assert!(l.text(i).right() < l.highlight(i).right());
            assert!(l.text(i).x > l.pill(i).right());
            assert_eq!(l.highlight(i).x, l.pill(i).x, "the pill grows into it");
        }
    }

    #[test]
    fn the_surfaces_hold_the_open_panel_and_its_glow() {
        let l = layout();
        let (surface, cap) = (l.surface(), l.cap_surface());
        assert_eq!(surface.x, 0.0);
        assert_eq!(surface.y, l.body_top, "nothing over the tab bar");
        assert_eq!(surface.right(), l.panel(true).right() + 26.0);
        assert_eq!(cap.x, l.seam(true));
        assert_eq!(cap.right(), surface.right());
        assert_eq!(cap.w, 12.0 + 26.0);
    }

    #[test]
    fn rows_stack_down_inside_the_ring() {
        let l = layout();
        assert_eq!(l.row_height(), 46.0);
        assert_eq!(l.pill(0).y, 42.0 + 1.0 + 8.0);
        assert_eq!(l.pill(1).y, l.pill(0).y + 46.0);
        assert!(l.pill(0).bottom() < l.pill(1).y, "a gap between highlights");
    }

    #[test]
    fn rows_shrink_to_fit_then_run_off_the_bottom() {
        let mut l = layout();
        l.rows = 40;
        assert_eq!(l.row_height(), 28.0, "no smaller than the least");
        assert!(l.row_visible(24));
        assert!(!l.row_visible(30));
        assert!(!l.has_timer(0), "too short for the timer");
        assert_eq!(l.bar(0, 1).w, 0.0);
    }

    #[test]
    fn the_timer_stacks_up_from_the_pad() {
        let l = layout();
        let (row, pad, b0, b1, b2) = (l.pill(0), l.pad(0), l.bar(0, 0), l.bar(0, 1), l.bar(0, 2));
        assert!(b2.y < b1.y && b1.y < b0.y && b0.bottom() < pad.y);
        assert_eq!((b0.x, b0.w), (pad.x, pad.w));
        let (top, bottom) = (b2.y - row.y, row.bottom() - pad.bottom());
        assert!((top - bottom).abs() <= 1.0, "{top} above, {bottom} below");
    }

    #[test]
    fn labels_sit_under_the_pads_left_of_the_seam() {
        let mut l = layout();
        // Two lines of a 12 px label font
        l.label = 24.0;
        assert_eq!(l.row_height(), 46.0 + 3.0 + 24.0, "taller by the label and its gap");
        let (pill, pad, label, bar) = (l.pill(1), l.pad(1), l.label(1), l.edge_bar(1));
        // The pill holds the timer, the pad and the label, centred on the pad
        assert!(l.has_timer(1));
        assert_eq!(pill.x + pill.w / 2.0, pad.x + pad.w / 2.0);
        assert_eq!(label.y, pad.bottom() + 3.0);
        assert!(label.bottom() <= pill.bottom() - 2.0);
        assert!(label.x > pill.x && label.right() < pill.right());
        let (top, bottom) = (l.bar(1, 2).y - pill.y, pill.bottom() - label.bottom());
        assert!((top - bottom).abs() <= 1.0, "{top} above, {bottom} below");
        // Everything that shows closed is left of the seam, where the cap begins
        assert!(pill.right() < l.seam(false), "{} against {}", pill.right(), l.seam(false));
        // And as wide as that lets it be: 42 px on the 68 px rail
        assert_eq!(pill.w, 42.0);
        assert_eq!(label.w, 38.0);
        // The open tab's bar sits down the rail's left edge, clear of the ring and the pill
        assert!(bar.x > l.panel(false).x + l.stroke() && bar.right() < pill.x);
        assert!(bar.y > pill.y && bar.bottom() < pill.bottom());
        assert_eq!(l.pill_radius(), 10.0);
        // Without labels, the pill hugs the pad as before
        l.label = 0.0;
        assert_eq!((l.pill(1).w, l.label(1).w, l.pill_radius()), (30.0, 0.0, 14.0));
    }

    #[test]
    fn the_spinner_sits_on_the_pads_corner() {
        let l = layout();
        let (pad, spin) = (l.pad(1), l.spinner(1));
        assert_eq!((spin.w, spin.h), (12.0, 12.0));
        // Centred 3 px in from the pad's top right corner, so it overhangs it
        assert_eq!((spin.x + 6.0, spin.y + 6.0), (pad.right() - 3.0, pad.y + 3.0));
        assert!(spin.right() > pad.right() && spin.y < pad.y);
        // On the closed rail, clear of the seam
        assert!(spin.right() < l.seam(false));
    }

    #[test]
    fn many_labelled_tabs_still_shrink_to_fit() {
        let mut l = layout();
        l.label = 24.0;
        l.rows = 40;
        assert_eq!(l.row_height(), 28.0 + 3.0 + 24.0);
        assert!(!l.has_timer(0));
        assert!(l.label(0).bottom() <= l.pill(0).bottom());
        assert!(l.pad(0).y >= l.pill(0).y);
    }

    #[test]
    fn hits_follow_the_open_state() {
        let l = layout();
        let y = l.pill(2).y + 5.0;
        assert_eq!(l.row_at(20.0, y, false), Some(2));
        assert_eq!(l.row_at(74.0, y, false), Some(2), "the gutter past it counts");
        assert_eq!(l.row_at(200.0, y, false), None, "past the rail while closed");
        assert_eq!(l.row_at(200.0, y, true), Some(2));
        assert_eq!(l.row_at(20.0, 20.0, true), None, "the tab bar isn't the panel");
        assert!(l.inside(297.0, y, true) && !l.inside(298.0, y, true));
    }

    #[test]
    fn everything_scales_with_dpi() {
        let mut l = layout();
        l.scale = 1.5;
        l.rail = 102.0;
        l.width = 435.0;
        assert_eq!(l.row_height(), 69.0);
        assert_eq!(l.gap(), 6.0);
        assert_eq!(l.radius(), 18.0);
        assert_eq!(l.stroke(), 2.0);
        for i in 0..l.rows {
            assert!(l.pill(i).right() <= l.seam(false));
            assert!(l.has_timer(i));
        }
    }

    #[test]
    fn the_light_runs_round_the_ring_down_the_left_first() {
        let l = layout();
        let ring = l.panel(true);
        let middle = |p| l.sweep_point(p);
        let (sx, sy) = middle(0.0);
        assert_eq!(sy, ring.y + 0.5, "starts on the top stroke's middle line");
        assert_eq!(sx, ring.x + 12.0);
        // A little way in it's on its way down the left side
        let (x, y) = middle(0.2);
        assert_eq!(x, ring.x + 0.5);
        assert!(y > sy);
        // Halfway, on the bottom or the right; it comes back to the start
        let (_, y) = middle(0.5);
        assert!(y > ring.y + ring.h / 2.0);
        assert_eq!(middle(1.0), middle(0.0));
        // Every point is on the middle line: within the ring, never inside the card
        for i in 0..200 {
            let (x, y) = middle(i as f64 / 200.0);
            assert!(x >= ring.x && x <= ring.right() && y >= ring.y && y <= ring.bottom());
            let inside = x > ring.x + 1.0 && x < ring.right() - 1.0 && y > ring.y + 1.0 && y < ring.bottom() - 1.0;
            let near_corner = (x < ring.x + 12.0 || x > ring.right() - 12.0) && (y < ring.y + 12.0 || y > ring.bottom() - 12.0);
            assert!(!inside || near_corner, "({x}, {y}) is inside the card");
        }
        // Steps along it are even: no jumps between legs
        let mut last = middle(0.0);
        for i in 1..=400 {
            let p = middle(i as f64 / 400.0);
            let step = ((p.0 - last.0).powi(2) + (p.1 - last.1).powi(2)).sqrt();
            assert!(step < 6.0, "a jump of {step} at {i}");
            last = p;
        }
    }

    #[test]
    fn the_progress_bar_and_unread_mark_sit_inside_their_row() {
        let l = layout();
        for i in 0..l.rows {
            let (bar, row) = (l.progress_bar(i), l.pill(i));
            assert!(bar.y > l.text(i).y && bar.bottom() < row.bottom());
            assert_eq!((bar.x, bar.w), (l.text(i).x, l.text(i).w));
            let (dot, pad) = (l.unread_dot(i), l.pad(i));
            assert!(dot.x >= pad.x && dot.right() <= pad.right() && dot.y >= pad.y && dot.bottom() <= pad.bottom());
        }
    }

    #[test]
    fn the_lights_surfaces_centre_on_its_start() {
        let l = layout();
        let (x, y) = l.sweep_point(0.0);
        for s in [l.spot_surface(), l.spot_core_surface()] {
            assert!((s.x + s.w / 2.0 - x).abs() <= 1.0 && (s.y + s.h / 2.0 - y).abs() <= 1.0, "{s:?}");
        }
        assert!(l.spot_surface().w > l.spot_core_surface().w);
    }

    #[test]
    fn the_conductors_slot_tops_the_rail_and_the_tabs_move_down_a_slot() {
        let mut l = layout();
        l.label = 24.0;
        let first = l.pill(0);
        l.head = true;
        let (head, pad, label, rule) = (l.head_pill(), l.head_pad(), l.head_label(), l.head_rule());
        // Where the first tab was, a tab's shape
        assert_eq!((head.x, head.y, head.w, head.h), (first.x, first.y, first.w, first.h));
        // The tabs a slot lower, and a little more for the rule
        assert_eq!(l.pill(0).y, head.y + l.row_height() + 7.0);
        assert!(rule.y > head.bottom() && rule.bottom() < l.pill(0).y, "{rule:?}");
        assert_eq!(rule.h, l.stroke());
        assert!(rule.x > head.x && rule.right() < head.right());
        // No timer: the pad and its label centred in the pill
        assert_eq!(label.y, pad.bottom() + 3.0);
        let (top, bottom) = (pad.y - head.y, head.bottom() - label.bottom());
        assert!((top - bottom).abs() <= 1.0, "{top} above, {bottom} below");
        // The remote mark on the pad's corner, and nothing past the seam
        let mark = l.head_mark();
        assert!(mark.right() > pad.right() && mark.y < pad.y && mark.right() < l.seam(false));
        // The slot is no tab's
        let y = head.y + 5.0;
        assert!(l.head_at(20.0, y, false) && l.head_at(200.0, y, true));
        assert_eq!(l.row_at(20.0, y, false), None);
        assert_eq!(l.row_at(20.0, l.pill(0).y + 5.0, false), Some(0));
        assert!(!l.head_at(20.0, l.pill(0).y + 5.0, false));
        // Open, its row runs as a tab's does
        assert_eq!(l.head_highlight().x, head.x);
        assert!(l.head_text().x > head.right() && l.head_text().right() < l.head_highlight().right());
        l.head = false;
        assert!(!l.head_at(20.0, y, false), "no slot without the Conductor");
    }

    #[test]
    fn the_conductors_slot_holds_the_open_tabs_ring_and_bar_as_a_rows_does() {
        for scale in [1.0, 2.0] {
            let mut l = layout();
            l.head = true;
            l.label = 20.0;
            l.scale = scale;
            let slot = l.head_slot();
            // The same height as a row's, ending just short of the rows': the rule's gap between
            assert_eq!(slot.h, l.slot(0).h);
            assert!(slot.bottom() <= l.slot(0).y, "{slot:?} runs into row 0's {:?}", l.slot(0));
            let soft = l.head_pill().grow(l.px(FEATHER));
            for part in [soft, l.head_pad(), l.head_label(), l.head_edge_bar()] {
                assert!(
                    part.y >= slot.y && part.bottom() <= slot.bottom() && part.x >= slot.x && part.right() <= slot.right(),
                    "at {scale}: {part:?} outside {slot:?}"
                );
            }
            // The bar beside the pill as a row's is beside its own
            let (bar, row_bar) = (l.head_edge_bar(), l.edge_bar(0));
            assert_eq!((bar.x, bar.w, bar.h), (row_bar.x, row_bar.w, row_bar.h));
            assert_eq!(bar.y - l.head_pill().y, row_bar.y - l.pill(0).y);
            // As a row's slot sits round its pill, so the marker lands the same way on either
            assert_eq!(l.head_pill().y - slot.y, l.pill(0).y - l.slot(0).y);
        }
    }

    #[test]
    fn with_the_conductor_many_tabs_still_shrink_to_fit() {
        let mut l = layout();
        l.head = true;
        l.rows = 15;
        let h = l.row_height();
        assert!(h < 46.0);
        // The slot, the gap and the rows fill the rail
        let room = l.panel(false).h - 2.0 * (1.0 + 8.0) - 7.0;
        assert_eq!(h, (room / 16.0).floor());
        assert!(l.row_visible(14));
    }

    #[test]
    fn a_light_goes_round_a_pill_evenly() {
        let pill = Area::new(16.0, 66.0, 42.0, 72.0);
        let ring = pill.grow(-0.5);
        let at = |p| outline_point(ring, 9.5, p);
        assert_eq!(at(0.0), (ring.x + 9.5, ring.y), "from the top of its top-left corner");
        assert_eq!(at(1.0), at(0.0));
        let mut last = at(0.0);
        for i in 1..=200 {
            let p = at(i as f64 / 200.0);
            let step = ((p.0 - last.0).powi(2) + (p.1 - last.1).powi(2)).sqrt();
            assert!(step < 2.0, "a jump of {step} at {i}");
            assert!(p.0 >= ring.x - 0.01 && p.0 <= ring.right() + 0.01 && p.1 >= ring.y - 0.01 && p.1 <= ring.bottom() + 0.01);
            last = p;
        }
    }

    #[test]
    fn the_panel_never_opens_narrower_than_the_rail() {
        let mut l = layout();
        l.width = 10.0;
        assert_eq!(l.panel(true).w, 68.0);
        assert_eq!(l.travel(), 0.0);
    }
}
