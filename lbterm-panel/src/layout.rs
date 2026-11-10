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
/// The disc of a tab hidden from the Conductor's mark, on a pad's bottom right corner, and the
/// eye-off glyph on it (lbterm/NEW-TAB.md, section 8)
const HIDDEN_DISC: f32 = 12.0;
pub const HIDDEN_GLYPH: f32 = 10.0;
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
/// Tab links' lines down the rail's left edge (lbterm_extras::links): at most this many side by
/// side, between the open tab's bar and the pills, a link finding none free sharing the last
pub const LINK_LANES: usize = 3;
/// From the pills to the first lane, and between lanes
const LINK_LANE_GAP: f32 = 3.0;
const LINK_WIDTH: f32 = 2.0;
/// The mark at each end of a link, and how far left of the pad its middle is: clear of the orange
/// ring round a pad that asks something too (its first try at 5 ran into it)
const LINK_END: f32 = 6.0;
const LINK_END_IN: f32 = 7.0;
/// The mark on the Conductor's pad for its remote control, on the pad's top right corner
const HEAD_MARK: f32 = 10.0;
/// The badge on the Conductor's pad counting the tab connections suggested on its rail, on the
/// pad's top left corner, its count in the pads' numbers' font: this share of that font's line
/// tall (14 px for Lachlan's 9 pt at 96 DPI), its words this far in from each end, and this far
/// clear of the remote control's mark
const HEAD_BADGE_LINE: f32 = 0.875;
const HEAD_BADGE_INSET: f32 = 2.0;
const HEAD_BADGE_GAP: f32 = 1.0;
/// The new-tab pad at the rail's foot (lbterm/DESIGN.md, "The + pad"): a pill this big, the label
/// pill's corners, centred on the pads; the + in it, two strokes this long and this wide
const FOOT_W: f32 = 42.0;
const FOOT_H: f32 = 30.0;
const PLUS: f32 = 12.0;
const PLUS_STROKE: f32 = 1.5;

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
    /// One of a label's lines: rows give their labels' lines up one at a time, before any tab
    /// leaves the rail ([`Layout::label_lines`]). 0 keeps the label whole
    pub label_line: f32,
    /// A slot above the tabs' for the Conductor's pad (lbterm_extras::conductor)
    pub head: bool,
    /// The new-tab pad, fixed at the rail's foot (`lbterm_tab_panel.new_tab`)
    pub foot: bool,
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

    /// How many lines a label takes as the config asks: none without labels, one when its lines
    /// aren't known apart
    fn lines_wanted(&self) -> usize {
        if self.label <= 0.0 {
            0
        } else if self.label_line > 0.0 {
            (self.label / self.label_line).round().max(1.0) as usize
        } else {
            1
        }
    }

    /// A label of `lines` lines under a pad, the gap above it included; nothing without labels
    /// or lines
    fn label_block_of(&self, lines: usize) -> f32 {
        if self.label <= 0.0 || lines == 0 {
            return 0.0;
        }
        let h = if self.label_line > 0.0 { lines as f32 * self.label_line } else { self.label };
        self.px(LABEL_GAP) + h
    }

    /// How many lines each label shows (lbterm/DESIGN.md, "The + pad"): all of them while every
    /// row fits at its least with them, else one fewer at a time, down to none, before any tab
    /// leaves the rail. The open panel names every tab whatever this says. With `label_line` 0 the
    /// label is kept whole.
    pub fn label_lines(&self) -> usize {
        let want = self.lines_wanted();
        let slots = self.rows + self.head as usize;
        if slots == 0 || self.label_line <= 0.0 {
            return want;
        }
        let room = self.list_room();
        (0..=want)
            .rev()
            .find(|&n| slots as f32 * (self.px(ROW_MIN) + self.label_block_of(n)) <= room)
            .unwrap_or(0)
    }

    /// Under a pad, the label as many lines as it shows and the gap above it; nothing without
    /// labels.
    fn label_block(&self) -> f32 {
        self.label_block_of(self.label_lines())
    }

    /// The room down the rail for the Conductor's slot and the tabs' rows: inside the ring, less
    /// the gap under the Conductor's slot and the new-tab pad's at the foot
    fn list_room(&self) -> f32 {
        self.panel(false).h - 2.0 * (self.stroke() + self.px(LIST_PAD)) - self.head_gap() - self.foot_block()
    }

    /// Each row's height: as tall as they come when there's room, shrinking
    /// to fit more tabs. The Conductor's slot is a row too, and the new-tab pad's room at the
    /// foot is kept, so it never falls off.
    pub fn row_height(&self) -> f32 {
        let label = self.label_block();
        let (min, max) = (self.px(ROW_MIN) + label, self.px(ROW_MAX) + label);
        let slots = self.rows + self.head as usize;
        if slots == 0 {
            return max;
        }
        (self.list_room() / slots as f32).floor().clamp(min, max)
    }

    /// The new-tab pad's room at the foot: its pill and the gap with the rule above it
    fn foot_block(&self) -> f32 {
        if self.foot {
            self.px(FOOT_H) + self.px(HEAD_GAP)
        } else {
            0.0
        }
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

    /// Whether row `i` is on screen at all: above the new-tab pad's rule when it's there.
    pub fn row_visible(&self, i: usize) -> bool {
        let (y, h) = self.row_band(i);
        let end = if self.foot {
            self.foot_rule().y
        } else {
            self.panel(false).bottom() - self.stroke()
        };
        i < self.rows && y + h <= end
    }

    /// How many tabs the rail has no room for: past rows at their least with no label lines,
    /// the new-tab pad's rule says "+N" so no tab is cut without a word
    pub fn hidden(&self) -> usize {
        (0..self.rows).rev().take_while(|&i| !self.row_visible(i)).count()
    }

    /// The new-tab pad (lbterm/DESIGN.md, "The + pad"): a pill at the rail's foot, centred on the
    /// pads, its bottom a list's pad above the ring's inside. It never moves.
    pub fn foot_pill(&self) -> Area {
        let (w, h) = (self.px(FOOT_W), self.px(FOOT_H));
        let bottom = self.panel(false).bottom() - self.stroke() - self.px(LIST_PAD);
        Area::new(self.column() - (w / 2.0).round(), (bottom - h).round(), w, h)
    }

    /// The new-tab pad's corners: the label pill's
    pub fn foot_radius(&self) -> f32 {
        self.px(LABEL_PILL_RADIUS)
    }

    /// The + in the new-tab pad: a stroke across and one down, crossing at its middle
    pub fn plus(&self) -> [Area; 2] {
        let pill = self.foot_pill();
        let (len, w) = (self.px(PLUS), PLUS_STROKE * self.scale);
        let (cx, cy) = (pill.x + pill.w / 2.0, pill.y + pill.h / 2.0);
        [
            Area::new(cx - len / 2.0, cy - w / 2.0, len, w),
            Area::new(cx - w / 2.0, cy - len / 2.0, w, len),
        ]
    }

    /// The hairline over the new-tab pad, the Conductor's rule's grey: across the middle of the
    /// rail, 80% of the pad's width, halfway between the last row's pill and the pad's
    pub fn foot_rule(&self) -> Area {
        let pill = self.foot_pill();
        let above = pill.y - self.px(HEAD_GAP);
        let half = (pill.w / 2.0 * 0.8).round();
        let h = self.stroke();
        Area::new(self.column() - half, ((above + pill.y - h) / 2.0).round(), 2.0 * half, h)
    }

    /// The new-tab pad's row on the open panel, from its pill on, as a tab's
    pub fn foot_highlight(&self) -> Area {
        self.highlight_from(self.foot_pill())
    }

    /// Where the new-tab pad's words go on the open panel
    pub fn foot_text(&self) -> Area {
        self.text_beside(self.foot_pill())
    }

    /// Where a press on the new-tab pad lands: the pad's slot across the rail (the open panel's
    /// width while it's open) from the rule down. With the window maximized (`corner`: the
    /// window's last pixel row) it runs on to the window's left edge and its last row, so a flick
    /// into the screen's corner lands on it; otherwise it stops at the window's edge inside its
    /// border, where Windows' resize border begins.
    pub fn foot_zone(&self, open: bool, corner: Option<f32>) -> Area {
        let top = self.foot_rule().y;
        let right = self.panel(open).right() + self.gap();
        let (left, bottom) = match corner {
            Some(last) => (self.left.min(0.0), last.max(self.bottom)),
            None => (self.left, self.bottom),
        };
        Area::new(left, top, right - left, (bottom - top).max(0.0))
    }

    /// Whether a point is on the new-tab pad's zone
    pub fn foot_at(&self, x: f32, y: f32, open: bool, corner: Option<f32>) -> bool {
        self.foot && self.foot_zone(open, corner).contains(x, y)
    }

    /// The middle of the rail, where the pads line up: the window strip's InMail button is
    /// centred over it too.
    pub fn column(&self) -> f32 {
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

    /// The badge on the Conductor's pad counting the tab connections suggested on its rail
    /// (lbterm/LINKS.md, "Connections"; his E8), for words `w` wide in a font whose lines are
    /// `line` tall, the pads' numbers' (his "use a bigger one", 2026-11-01): a pill on the pad's
    /// top left corner, as the remote control's mark sits on its top right, a disc for one digit,
    /// growing to the right for more, its left end staying inside the pad's pill and its right
    /// short of the mark, its words nearer its ends where they'd reach it. Until 2026-11-02 it was
    /// 11 px across at 96 DPI with the count in the labels' 7 pt font, whose 1 came out half-toned
    pub fn head_badge(&self, w: f32, line: f32) -> Area {
        let pad = self.head_pad();
        let h = (line * HEAD_BADGE_LINE).round().max(1.0);
        let (cx, cy) = (pad.x + self.px(2.0), pad.y);
        let left = (cx - h / 2.0).max(self.head_pill().x + self.px(1.0)).round();
        let room = (self.head_mark().x - self.px(HEAD_BADGE_GAP) - left).floor();
        let width = (w + 2.0 * self.px(HEAD_BADGE_INSET)).round().min(room).max(h).max(w.ceil());
        Area::new(left, (cy - h / 2.0).round(), width, h)
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
        let block = self.label_block();
        if block <= 0.0 {
            return Area::new(pad.x, pad.bottom(), 0.0, 0.0);
        }
        let half = self.pill_half() - self.px(LABEL_INSET);
        Area::new(self.column() - half, pad.bottom() + self.px(LABEL_GAP), 2.0 * half, block - self.px(LABEL_GAP))
    }

    /// What covers row `i`'s label while the panel is open (Lachlan's 12a, lbterm/NEW-TAB.md,
    /// "Landing 4"), in the pill's colour on the open parts' surface: the label's room, a pixel
    /// wider either side for the glyphs' soft edges (still inside the pill: the room is
    /// `LABEL_INSET` in from it) and a pixel below. Its top is the room's, the gap under the pad
    /// left alone, where the hidden mark's disc ends. None while the rows show no label line.
    pub fn label_cover(&self, i: usize) -> Option<Area> {
        let room = self.label(i);
        if room.h <= 0.0 || room.w <= 0.0 {
            return None;
        }
        let edge = self.px(1.0);
        Some(Area::new(room.x - edge, room.y, room.w + 2.0 * edge, room.h + edge))
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

    /// Subagent `k` of `n`'s bar in row `i` (the hub's guesses): side by side, a gap between
    /// each, along the line over the program's bar when the row has one (`over`), else on its line.
    pub fn agent_bar(&self, i: usize, k: usize, n: usize, over: bool) -> Area {
        let main = self.progress_bar(i);
        let gap = self.px(4.0);
        let n = n.max(1) as f32;
        let w = ((main.w - gap * (n - 1.0)) / n).floor().max(0.0);
        let y = if over { main.y - main.h - self.px(2.0) } else { main.y };
        Area::new(main.x + k as f32 * (w + gap), y, w, main.h)
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

    /// The mark of a tab hidden from the Conductor: a disc on row `i`'s pad's bottom right
    /// corner, centred as the turning arc's square is on the top right (the top right is the
    /// arc's and the unread dot's, and the links' marks are left of the pad), for the eye-off
    /// glyph to sit in the middle of
    pub fn hidden_mark(&self, i: usize) -> Area {
        let pad = self.pad(i);
        let half = self.px(HIDDEN_DISC) / 2.0;
        let (cx, cy) = (pad.right() - self.px(3.0), pad.bottom() - self.px(3.0));
        Area::new(cx - half, cy - half, 2.0 * half, 2.0 * half)
    }

    /// The mark in the top right corner of row `i`'s pad for a result not
    /// yet seen.
    pub fn unread_dot(&self, i: usize) -> Area {
        let pad = self.pad(i);
        let d = self.px(6.0);
        Area::new(pad.right() - d - self.px(2.0), pad.y + self.px(2.0), d, d)
    }

    /// The middle of tab links' lane `lane` down the rail's left edge: the first a gap left of
    /// the pills, each next a gap further out (the last, beside the open tab's bar, is shared)
    pub fn link_lane(&self, lane: usize) -> f32 {
        let lanes = lane.min(LINK_LANES - 1) as f32 + 1.0;
        (self.column() - self.pill_half() - lanes * self.px(LINK_LANE_GAP)).round()
    }

    /// How wide a link's line is
    pub fn link_width(&self) -> f32 {
        self.px(LINK_WIDTH)
    }

    /// Where a link ends on row `i`: its mark's middle, left of the pad and level with its middle,
    /// and right of the first lane (without labels the pills are narrow and the lanes come close)
    pub fn link_end(&self, i: usize) -> (f32, f32) {
        let pad = self.pad(i);
        let x = (pad.x - self.px(LINK_END_IN)).max(self.link_lane(0) + self.px(LINK_LANE_GAP));
        (x, (pad.y + pad.h / 2.0).round())
    }

    /// How wide the mark at each end of a link is
    pub fn link_end_size(&self) -> f32 {
        self.px(LINK_END)
    }

    /// Whether a point is on the rail's link lanes, from the outer lane to the end marks: where
    /// a click on a suggested link's dashes lands
    pub fn on_link_lanes(&self, x: f32) -> bool {
        let half = self.link_width() + self.px(1.0);
        let end = self.link_end_size() / 2.0;
        x >= self.link_lane(LINK_LANES - 1) - half && x <= self.link_end(0).0 + end
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

    /// The room the edge crosses as the panel opens, over the panes it presses over: from the
    /// closed ring's right edge to the open one's, as tall as the ring. The wave washes over it
    /// just ahead of the edge (crate::wave).
    pub fn wave_band(&self) -> Area {
        let (closed, open) = (self.panel(false), self.panel(true));
        Area::new(closed.right(), open.y, open.right() - closed.right(), open.h)
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
            label_line: 0.0,
            head: false,
            foot: false,
        }
    }

    /// Lachlan's rail: two-line labels of a 12 px line, the Conductor's slot, the new-tab pad, the
    /// panel `h` tall inside its gutters, at `scale`
    fn his(rows: usize, h: f32, scale: f32) -> Layout {
        let gap = (4.0 * scale).round();
        Layout {
            left: 0.0,
            body_top: 38.0,
            bottom: 38.0 + 2.0 * gap + h,
            right: 1080.0,
            rail: (68.0 * scale).round(),
            width: (290.0 * scale).round(),
            scale,
            rows,
            label: 24.0 * scale,
            label_line: 12.0 * scale,
            head: true,
            foot: true,
        }
    }

    #[test]
    fn the_hidden_mark_sits_on_the_pads_bottom_right_clear_of_the_arc_and_the_links() {
        for scale in [1.0, 1.5, 2.0] {
            let l = his(6, 900.0, scale);
            for i in (0..6).filter(|&i| l.row_visible(i)) {
                let (pad, mark) = (l.pad(i), l.hidden_mark(i));
                // A 12 px disc, centred 3 px in from the pad's bottom right corner
                assert_eq!((mark.w, mark.h), (l.px(12.0), l.px(12.0)), "at {scale}");
                assert_eq!(mark.x + mark.w / 2.0, pad.right() - l.px(3.0));
                assert_eq!(mark.y + mark.h / 2.0, pad.bottom() - l.px(3.0));
                // The glyph fits inside it
                assert!(l.px(HIDDEN_GLYPH) < mark.w);
                // Below the arc's square and the unread dot, at the top right
                let (arc, dot) = (l.spinner(i), l.unread_dot(i));
                assert!(mark.y >= arc.bottom() - l.px(1.0) && mark.y >= dot.bottom(), "row {i} at {scale}: {mark:?} into {arc:?}");
                // Right of where the links' marks end, left of the pad
                let (end, _) = l.link_end(i);
                assert!(mark.x > end + l.link_end_size() / 2.0, "row {i} at {scale}");
                // Inside the row's slot, so it moves with the row
                let slot = l.slot(i);
                assert!(mark.y >= slot.y && mark.bottom() <= slot.bottom(), "row {i} at {scale}");
            }
        }
    }

    #[test]
    fn a_labels_cover_takes_its_room_inside_the_pill_and_clear_of_the_pad_and_its_marks() {
        for scale in [1.0, 1.5, 2.0] {
            // Two lines of label, then crowded down to one and to none
            for rows in [6, 30, 60] {
                let l = his(rows, 900.0, scale);
                for i in (0..rows).filter(|&i| l.row_visible(i)) {
                    let (room, pill, pad) = (l.label(i), l.pill(i), l.pad(i));
                    let Some(cover) = l.label_cover(i) else {
                        assert_eq!(l.label_lines(), 0, "row {i} of {rows} at {scale}: a label shows and nothing covers it");
                        continue;
                    };
                    // All the room the label is drawn in
                    assert!(cover.x <= room.x && cover.right() >= room.right(), "row {i} of {rows} at {scale}");
                    assert!(cover.y <= room.y && cover.bottom() >= room.bottom(), "row {i} of {rows} at {scale}");
                    // Inside the pill, so a lifted row's hover colour isn't cut square
                    assert!(cover.x >= pill.x && cover.right() <= pill.right(), "row {i} of {rows} at {scale}: {cover:?} past {pill:?}");
                    assert!(cover.bottom() <= pill.bottom(), "row {i} of {rows} at {scale}");
                    // Below the pad and the hidden mark's disc, which stay on the open panel
                    assert!(cover.y >= pad.bottom(), "row {i} of {rows} at {scale}");
                    assert!(cover.y >= l.hidden_mark(i).bottom() - 0.01, "row {i} of {rows} at {scale}");
                }
            }
        }
        // No labels: nothing to cover
        let l = layout();
        assert_eq!(l.label_cover(0), None);
    }

    #[test]
    fn the_new_tab_pad_sits_at_the_rails_foot_and_never_moves() {
        for scale in [1.0, 1.5] {
            for rows in [1, 8, 30] {
                let l = his(rows, 900.0, scale);
                let (pill, panel) = (l.foot_pill(), l.panel(false));
                assert_eq!((pill.w, pill.h), (l.px(42.0), l.px(30.0)), "at {scale}");
                assert_eq!(pill.bottom(), panel.bottom() - l.stroke() - l.px(8.0), "a list's pad above the ring's inside");
                assert!((pill.x + pill.w / 2.0 - l.column()).abs() <= 0.5, "centred on the pads");
                assert!(pill.right() < l.seam(false) && pill.x > panel.x + l.stroke(), "inside the closed rail, left of the seam");
                assert_eq!(l.foot_radius(), l.px(10.0), "the label pill's corners");
                // The rule over it: in the head rule's place, 80% of the pad
                let rule = l.foot_rule();
                assert!(rule.bottom() <= pill.y && rule.y >= pill.y - l.px(7.0), "{rule:?} over {pill:?}");
                assert_eq!(rule.w, (pill.w / 2.0 * 0.8).round() * 2.0);
                assert_eq!(rule.h, l.stroke());
                // Every visible row is above the rule
                for i in (0..rows).filter(|&i| l.row_visible(i)) {
                    assert!(l.pill(i).bottom() <= rule.y, "row {i} at {scale} runs into the rule");
                }
                // The + inside it: two strokes 12 px across, crossing at its middle
                let [across, down] = l.plus();
                assert_eq!((across.w, down.h), (l.px(12.0), l.px(12.0)));
                assert!(across.x > pill.x && across.right() < pill.right() && down.y > pill.y && down.bottom() < pill.bottom());
                // Open, its row runs beside it as a tab's does
                assert_eq!(l.foot_highlight().x, pill.x);
                assert!(l.foot_text().x > pill.right() && l.foot_text().right() < l.seam(true));
                // The same place however many tabs there are
                assert_eq!(pill, his(rows + 5, 900.0, scale).foot_pill());
            }
        }
    }

    #[test]
    fn the_row_height_keeps_room_for_the_conductor_and_the_new_tab_pad() {
        let mut l = his(12, 900.0, 1.0);
        let room = l.panel(false).h - 2.0 * (1.0 + 8.0) - 7.0 - (30.0 + 7.0);
        assert_eq!(l.row_height(), (room / 13.0).floor().clamp(28.0 + 3.0 + 24.0, 46.0 + 3.0 + 24.0));
        l.foot = false;
        let room = l.panel(false).h - 2.0 * (1.0 + 8.0) - 7.0;
        assert_eq!(l.row_height(), (room / 13.0).floor().clamp(55.0, 73.0), "without the pad, as before");
        l.head = false;
        let room = l.panel(false).h - 2.0 * (1.0 + 8.0);
        assert_eq!(l.row_height(), (room / 12.0).floor().clamp(55.0, 73.0));
    }

    #[test]
    fn labels_give_up_lines_before_any_tab_leaves_the_rail() {
        // 20 tabs in 900 px: no label lines, every tab drawn
        let l = his(20, 900.0, 1.0);
        assert_eq!(l.label_lines(), 0);
        assert_eq!(l.label(0).h, 0.0, "no label line shows");
        assert!((0..20).all(|i| l.row_visible(i)), "all 20 drawn");
        assert_eq!(l.hidden(), 0);
        // 16 in 978 px: one line each, every tab drawn; two would push the last off
        let l = his(16, 978.0, 1.0);
        assert_eq!(l.label_lines(), 1);
        assert_eq!(l.label(0).h, 12.0);
        assert!((0..16).all(|i| l.row_visible(i)), "all 16 drawn");
        let mut whole = l;
        whole.label_line = 0.0;
        assert!(!whole.row_visible(15), "kept whole, the last tab fell off");
        // Few tabs: both lines
        let l = his(8, 978.0, 1.0);
        assert_eq!(l.label_lines(), 2);
        assert_eq!(l.label(0).h, 24.0);
        // Without labels, none to give up
        let mut bare = his(8, 978.0, 1.0);
        bare.label = 0.0;
        assert_eq!(bare.label_lines(), 0);
    }

    #[test]
    fn plus_n_only_past_rows_at_their_least() {
        for rows in 1..=80 {
            let l = his(rows, 900.0, 1.0);
            if l.hidden() > 0 {
                assert_eq!(l.row_height(), 28.0, "{rows} tabs: a tab hidden before rows reached their least");
                assert_eq!(l.label_lines(), 0, "{rows} tabs: hidden while labels kept a line");
                assert_eq!(l.hidden(), (0..rows).filter(|&i| !l.row_visible(i)).count());
            }
        }
        assert_eq!(his(29, 900.0, 1.0).hidden(), 0);
        assert!(his(60, 900.0, 1.0).hidden() > 0, "60 tabs don't fit 900 px");
    }

    #[test]
    fn the_new_tab_pads_zone_runs_into_the_screens_corner_when_maximized() {
        let l = his(8, 900.0, 1.0);
        let pill = l.foot_pill();
        let mid = (pill.x + pill.w / 2.0, pill.y + pill.h / 2.0);
        for corner in [None, Some(l.bottom + 4.0)] {
            assert!(l.foot_at(mid.0, mid.1, false, corner), "the pill itself");
            assert!(l.foot_at(l.panel(false).right() + 2.0, mid.1, false, corner), "the gutter past the rail");
            assert!(!l.foot_at(l.panel(false).right() + 10.0, mid.1, false, corner), "not the panes");
            assert!(l.foot_at(l.panel(true).right() + 2.0, mid.1, true, corner), "the open panel's width");
            assert!(!l.foot_at(mid.0, l.pill(0).y + 2.0, false, corner), "not a tab's row");
            assert!(l.foot_at(0.0, mid.1, false, corner), "the window's left edge");
        }
        // Not maximized: it stops at the window's edge inside its border
        assert!(!l.foot_at(mid.0, l.bottom + 1.0, false, None));
        // Maximized: down to the window's last pixel row, the screen's corner
        let last = l.bottom + 3.0;
        assert!(l.foot_at(0.0, last, false, Some(l.bottom + 4.0)));
        assert!(l.foot_at(mid.0, last, false, Some(l.bottom + 4.0)));
        // No pad, no zone
        let mut none = l;
        none.foot = false;
        assert!(!none.foot_at(mid.0, mid.1, false, None));
    }

    #[test]
    fn the_wave_washes_over_the_room_the_edge_crosses() {
        let l = layout();
        let band = l.wave_band();
        assert_eq!(band.x, l.panel(false).right());
        assert_eq!(band.right(), l.panel(true).right());
        assert_eq!(band.w, l.travel());
        assert_eq!((band.y, band.h), (l.panel(true).y, l.panel(true).h));
        // Over the panes' room, not the closed rail's
        assert!(band.x >= l.strip());
    }

    #[test]
    fn link_lanes_run_between_the_open_tabs_bar_and_the_pills() {
        for label in [0.0, 20.0] {
            let mut l = layout();
            l.label = label;
            let pill = l.pill(1);
            let bar = l.edge_bar(1);
            let half = l.link_width() / 2.0;
            // The first two clear of both; the last, shared, may run beside the bar
            for lane in 0..LINK_LANES - 1 {
                let x = l.link_lane(lane);
                assert!(x + half <= pill.x && x - half >= bar.right(), "lane {lane} at {x} with labels {label}");
            }
            assert!(l.link_lane(LINK_LANES - 1) + half <= l.link_lane(LINK_LANES - 2) - half);
            assert_eq!(l.link_lane(LINK_LANES), l.link_lane(LINK_LANES - 1), "past the last lane it's shared");
            // Each end's mark sits left of the pad, level with its middle, and in its row's slot
            let (x, y) = l.link_end(1);
            let pad = l.pad(1);
            assert!(x + l.link_end_size() / 2.0 <= pad.x);
            assert!(x > l.link_lane(0));
            assert_eq!(y, (pad.y + pad.h / 2.0).round());
            let slot = l.slot(1);
            assert!(y > slot.y && y < slot.bottom());
            assert!(l.on_link_lanes(l.link_lane(2)) && l.on_link_lanes(x) && !l.on_link_lanes(pad.x + 2.0));
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
    fn subagents_bars_share_the_line_over_the_programs_bar() {
        let l = layout();
        let main = l.progress_bar(0);
        for n in 1..=4 {
            let bars: Vec<Area> = (0..n).map(|k| l.agent_bar(0, k, n, true)).collect();
            assert_eq!(bars[0].x, main.x);
            assert!(bars[n - 1].right() <= main.right(), "{n}: within the words' width");
            for pair in bars.windows(2) {
                assert!(pair[1].x >= pair[0].right() + l.px(4.0) - 0.01, "{n}: a gap between each");
            }
            for b in &bars {
                assert_eq!((b.y, b.h), (main.y - main.h - l.px(2.0), main.h), "the line over it");
                assert!(b.y > l.pill(0).y);
            }
        }
        // With no bar of the program's, on its line
        assert_eq!(l.agent_bar(0, 0, 2, false).y, main.y);
        assert_eq!(l.agent_bar(0, 0, 1, false).w, main.w.floor());
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
    fn the_conductors_count_of_suggested_connections_sits_on_its_pads_other_corner() {
        for (scale, label) in [(1.0, 24.0), (1.0, 0.0), (1.5, 24.0), (2.0, 24.0)] {
            let mut l = layout();
            l.scale = scale;
            l.label = label;
            l.head = true;
            let (pill, pad, mark) = (l.head_pill(), l.head_pad(), l.head_mark());
            // The pads' numbers' font: Lachlan's 9 pt, a digit 7 px wide in a line 16 tall at 96 DPI
            let (digit, line) = (7.0 * scale, 16.0 * scale);
            // One digit: a disc on the pad's top left corner, clear of the remote mark on its right,
            // and tall enough for the font's digits with room round them: 14 px at 96 DPI, where
            // the labels' 7 pt count had 11
            let one = l.head_badge(digit, line);
            assert_eq!(one.w, one.h, "a disc at {scale}");
            assert_eq!(one.h, (14.0 * scale).round(), "at {scale}");
            assert!(one.h >= line * 0.85 && one.w >= digit + 2.0 * l.px(HEAD_BADGE_INSET), "{one:?} at {scale}");
            assert!(one.x < pad.x && one.right() > pad.x && one.y < pad.y && one.bottom() > pad.y, "{one:?} on {pad:?}");
            assert!(one.right() <= mark.x - l.px(HEAD_BADGE_GAP), "clear of the remote mark: {one:?} {mark:?}");
            assert!(one.x > pill.x, "inside the pill: {one:?} {pill:?}");
            // Two digits: wider to the right, its left where it was, still short of the mark, with
            // a pixel at least either side of them
            let two = l.head_badge(2.0 * digit, line);
            assert!(two.w > one.w && two.x == one.x && two.y == one.y && two.h == one.h, "{two:?} {one:?}");
            assert!(two.right() <= mark.x - l.px(HEAD_BADGE_GAP), "12 clear of the remote mark at {scale}: {two:?} {mark:?}");
            assert!(two.w >= 2.0 * digit + 2.0, "12's room at {scale}: {two:?}");
            // Three never fit beside the mark: the badge takes them whole, over it
            let three = l.head_badge(3.0 * digit, line);
            assert!(three.w >= 3.0 * digit && three.x == one.x);
            // Whole pixels, so its edges are crisp
            for v in [one.x, one.y, one.w, one.h, two.x, two.y, two.w, two.h] {
                assert_eq!(v, v.round());
            }
        }
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
