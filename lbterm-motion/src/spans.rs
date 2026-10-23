//! Which visual each quad of a frame is drawn into.
//!
//! WezTerm's paint pass writes every quad of the window into one set of
//! vertex buffers. With the compositor, the same buffers are drawn once per
//! visual, each time with only that visual's quads. The paint pass marks
//! where each run of quads starts ([`QuadSpans::switch`]); the draw asks
//! for one visual's runs ([`QuadSpans::ranges`]).

use std::ops::Range;

/// A visual in the window's tree, bottom to top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<Id> {
    /// The window background, under everything. Each pane draws it first
    /// too, so a pane's visual is opaque to its edges even where its own
    /// background stops half a pixel short of the snapped rectangle.
    Under,
    /// One pane's background, text and cursor.
    Pane(Id),
    /// The split lines. Each pane draws them too, keeping what falls inside
    /// its own rectangle, so a line moves with the pane beside it.
    Splits,
    /// Over the panes: tab bar, borders, scrollbar, modals.
    Over,
    /// A part of the tab panel, over everything. Its quads have z-layers of
    /// their own rather than spans in z-layer 0.
    Panel(PanelPart),
    /// Over the tab panel too: the tab menu (z-layers 60-69). Only in a
    /// frame while something is drawn there, and for one cleared frame after.
    Top,
    /// A card template the panes' edges and the active outline are cut
    /// from (z-layers 70, 71, 74 and 75; the strip's hover slots have 72 and
    /// 73): never seen itself, only in pieces.
    Card(CardPart),
    /// One of the strip's two hover slots (z-layers 72 and 73), just over
    /// `over`: a window button's or the restart pill's look under the
    /// pointer, faded in and out by DWM (`lbterm_extras::hover`).
    Hover(usize),
    /// The window strip's detail card (z-layers 30 to 32), over the hover slots and under the
    /// tab panel: faded in and out by DWM (wezterm-gui's render/strip.rs).
    StripCard,
    /// The turning arc on a Conductor menu item that waits for its answer (z-layer 76): a
    /// template never seen itself, which a visual over the menu shows, turned by DWM
    /// (wezterm-gui's render/conductor.rs).
    MenuSpinner,
    /// The white swatch every brief light shows, clipped to its shape: a click's ripple, a
    /// copied or opened stretch of text (wezterm-gui's render/lights.rs). No quads: it's
    /// cleared to white, and only in a frame when it's new or a new size.
    Swatch,
    /// The readout of the window's size in cells and the font's, over the panes while the
    /// window is resized or the font changes (z-layers 33 to 35; render/lights.rs)
    Readout,
    /// The window strip as it was, over its new words while it fades out (z-layers 36 and 37;
    /// render/strip_swap.rs)
    Swap,
}

/// The template cards, each drawn as wide as the window (its corners, top
/// and bottom edges) and as tall (its left and right edges). A pane shows
/// nine pieces of the mask, so it stays rounded at every size DWM shows it
/// at; the active pane's outline is nine pieces of the line, gliding
/// between panes as the keyboard moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPart {
    /// What's outside a card: the window's background, clear inside
    MaskWide,
    MaskTall,
    /// The outline alone
    LineWide,
    LineTall,
}

/// The tab panel's visuals (lbterm/TAB-PANEL.md). The panel is drawn open;
/// DWM shows the body and its glow up to a seam that moves with the edge,
/// and the cap (the ring's right-hand corners) and its glow riding on the
/// edge. The light that runs round the ring is drawn where it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPart {
    Glow,
    /// The card, the grey ring round it, the pads
    Body,
    /// The ring lit, over the grey one while the panel is open
    Lit,
    /// Titles and the open highlight, which fade in after the edge
    Words,
    CapGlow,
    Cap,
    CapLit,
    /// The light's halo, and its bright middle
    Spot,
    SpotCore,
    /// The turning arc's template, shown on each pad waiting on tasks
    Spinner,
    /// The Conductor's outlines' templates, shown round its pad and the tabs it's on, and its
    /// light's, carried round them
    RingSpectrum,
    RingIris,
    RingLight,
    /// The tabs' rows on the rail (pads, pills, labels): drawn into one of two surfaces by
    /// turns, never seen themselves, shown a row at a time so DWM can slide, bring in, fade out
    /// and dissolve each
    Rows,
    /// The rows' parts on the open panel (the long highlight, the words, the chips, the
    /// progress track), drawn and shown the same way, each row's under the words' fade
    RowWords,
    /// The open tab's ring and bar, which glide to the tab that opens
    Marker,
    /// Each row's progress fill drawn whole, never seen itself: the open panel shows as
    /// much of each as the program says, and DWM glides it when that changes
    Progress,
}

/// A rectangle of the window in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl PixelRect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    /// Each edge to a pixel boundary the way the GPU fills quads: a pixel
    /// belongs to the quad its centre falls in, and a centre exactly on an
    /// edge belongs to the quad on its right or below (the top-left rule).
    /// Two rectangles that meet still meet, text stays 1:1 with the screen,
    /// and each pane's visual holds exactly the pixels its quads fill.
    pub fn snap(x: f32, y: f32, w: f32, h: f32) -> Self {
        let edge = |e: f32| (e - 0.5).ceil() as i32;
        let (x0, y0) = (edge(x), edge(y));
        let (x1, y1) = (edge(x + w), edge(y + h));
        Self {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0) as u32,
            h: (y1 - y0).max(0) as u32,
        }
    }

    pub fn to_rect(&self) -> crate::Rect {
        crate::Rect::new(self.x as f64, self.y as f64, self.w as f64, self.h as f64)
    }

    /// Where this pane shows while the tab panel is open: everything from
    /// `left` to `right` (the panes' visuals, from the first card's to the
    /// last's) pressed into the room left once the panes move `travel` to
    /// the right. Edges left of `left` just move, and edges past `right`
    /// stay. Only the visual changes: its content keeps its size, anchored
    /// at its top-left, so the right of each pane is cut off, never
    /// squashed; its card's outline rides on the cut.
    pub fn squeeze(&self, left: i32, travel: i32, right: i32) -> PixelRect {
        let room = (right - left) as f64;
        if room <= 0.0 || travel <= 0 {
            return *self;
        }
        let k = (room - travel as f64).max(0.0) / room;
        let at = |x: i32| -> i32 {
            if x <= left {
                x + travel
            } else if x >= right {
                x
            } else {
                (left as f64 + travel as f64 + (x - left) as f64 * k).round() as i32
            }
        };
        let (x0, x1) = (at(self.x), at(self.x + self.w as i32));
        PixelRect::new(x0, self.y, (x1 - x0).max(0) as u32, self.h)
    }
}

/// D3D12 lets one command list, and one ExecuteCommandLists call, write to
/// at most this many swapchains (COMMAND_LIST_TOO_MANY_SWAPCHAIN_REFERENCES,
/// COMMAND_QUEUE_TOO_MANY_SWAPCHAIN_REFERENCES), and every visual is one.
/// Past it a list fails to close, and wgpu 25 hangs handling that; a call
/// past it removes the device.
pub const SWAPCHAINS_PER_COMMAND_LIST: usize = 8;

/// Which of a frame's command lists draws its `visual`th visual, so that
/// none writes to more than [`SWAPCHAINS_PER_COMMAND_LIST`] swapchains.
pub fn command_list_for(visual: usize) -> usize {
    visual / SWAPCHAINS_PER_COMMAND_LIST
}

/// How many command lists a frame of `visuals` visuals needs.
pub fn command_lists(visuals: usize) -> usize {
    visuals.div_ceil(SWAPCHAINS_PER_COMMAND_LIST).max(1)
}

/// Runs of quads in z-layer 0, per sub-layer, each belonging to one target,
/// plus the rectangle of every pane painted.
#[derive(Debug, Default)]
pub struct QuadSpans<Id> {
    open: Option<(Target<Id>, [usize; 3])>,
    spans: Vec<(Target<Id>, [Range<usize>; 3])>,
    panes: Vec<(Id, PixelRect)>,
    active: Option<Id>,
}

impl<Id: Copy + PartialEq> QuadSpans<Id> {
    /// A new paint pass: forget the last one, and count from `counts` (the
    /// quads already in each sub-layer) as the window background.
    pub fn reset(&mut self, counts: [usize; 3]) {
        self.spans.clear();
        self.panes.clear();
        self.active = None;
        self.open = Some((Target::Under, counts));
    }

    /// From `counts` on, quads belong to `target`.
    pub fn switch(&mut self, target: Target<Id>, counts: [usize; 3]) {
        self.close(counts);
        self.open = Some((target, counts));
    }

    /// The paint pass is over and each sub-layer ended at `counts`.
    pub fn finish(&mut self, counts: [usize; 3]) {
        self.close(counts);
    }

    fn close(&mut self, counts: [usize; 3]) {
        if let Some((target, start)) = self.open.take() {
            let ranges = [0, 1, 2].map(|i| start[i]..counts[i].max(start[i]));
            if ranges.iter().any(|r| !r.is_empty()) {
                self.spans.push((target, ranges));
            }
        }
    }

    pub fn set_pane_rect(&mut self, pane: Id, rect: PixelRect) {
        match self.panes.iter_mut().find(|(id, _)| *id == pane) {
            Some(entry) => entry.1 = rect,
            None => self.panes.push((pane, rect)),
        }
    }

    /// Every pane painted, in paint order, with its rectangle.
    pub fn panes(&self) -> &[(Id, PixelRect)] {
        &self.panes
    }

    /// The pane painted as the one with the keyboard.
    pub fn set_active(&mut self, pane: Id) {
        self.active = Some(pane);
    }

    pub fn active(&self) -> Option<Id> {
        self.active
    }

    /// The quads of sub-layer `sub` that `target` draws, in paint order,
    /// joined where they touch and cut off at `limit` (the quads the
    /// buffer really holds).
    pub fn ranges(&self, sub: usize, target: Target<Id>, limit: usize) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = vec![];
        for (span, ranges) in &self.spans {
            let wanted = *span == target
                || (matches!(span, Target::Under | Target::Splits)
                    && matches!(target, Target::Pane(_)));
            if !wanted {
                continue;
            }
            let r = ranges[sub].start.min(limit)..ranges[sub].end.min(limit);
            if r.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => out.push(r),
            }
        }
        out
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn no_command_list_writes_to_more_than_eight_swapchains() {
        // The window, over, the panel's five and two panes: one more than a
        // single list may write to, as the first split with the panel on was
        for visuals in 1..40 {
            let lists = command_lists(visuals);
            let mut per = vec![0; lists];
            for v in 0..visuals {
                per[command_list_for(v)] += 1;
            }
            assert!(per.iter().all(|&n| (1..=8).contains(&n)), "{visuals}: {per:?}");
        }
        assert_eq!((command_lists(8), command_lists(9)), (1, 2));
        assert_eq!(command_lists(0), 1);
    }

    /// A paint pass like WezTerm's: background, two panes (the active one
    /// with a scrollbar thumb in sub-layer 2), splits, tab bar.
    fn frame() -> QuadSpans<u32> {
        let mut s = QuadSpans::default();
        s.reset([0, 0, 0]);
        // window background: 1 quad
        s.switch(Target::Pane(1), [1, 0, 0]);
        s.set_pane_rect(1, PixelRect::new(0, 30, 500, 570));
        // pane 1: 10 backgrounds, 40 glyphs, 1 cursor
        s.switch(Target::Pane(2), [11, 40, 1]);
        s.set_pane_rect(2, PixelRect::new(500, 30, 500, 570));
        // pane 2: background, then its scrollbar thumb, then the rest
        s.switch(Target::Over, [12, 40, 1]);
        s.switch(Target::Pane(2), [12, 40, 2]);
        s.switch(Target::Splits, [20, 70, 3]);
        // one split line
        s.switch(Target::Over, [20, 70, 4]);
        // tab bar: 3 quads, 12 glyphs
        s.finish([23, 82, 4]);
        s
    }

    #[test]
    fn each_target_gets_only_its_quads() {
        let s = frame();
        assert_eq!(s.ranges(0, Target::Under, 1000), vec![0..1]);
        assert_eq!(s.ranges(1, Target::Pane(1), 1000), vec![0..40]);
        // Its cursor, and the split line every pane draws
        assert_eq!(s.ranges(2, Target::Pane(1), 1000), vec![0..1, 3..4]);
        // The thumb went to Over, not to the pane
        assert_eq!(s.ranges(2, Target::Over, 1000), vec![1..2]);
        assert_eq!(s.ranges(0, Target::Over, 1000), vec![20..23]);
    }

    #[test]
    fn panes_draw_the_split_lines_too() {
        let s = frame();
        // Pane 2's own runs and the split line, joined because they touch
        assert_eq!(s.ranges(2, Target::Pane(2), 1000), vec![2..4]);
        assert_eq!(s.ranges(2, Target::Pane(1), 1000), vec![0..1, 3..4]);
        assert!(s.ranges(2, Target::Over, 1000).iter().all(|r| !r.contains(&3)));
    }

    #[test]
    fn panes_draw_the_window_background_first() {
        let s = frame();
        // Background quad 0, then pane 2's two runs, joined
        assert_eq!(s.ranges(0, Target::Pane(2), 1000), vec![0..1, 11..20]);
        assert_eq!(s.ranges(0, Target::Pane(1), 1000), vec![0..11]);
    }

    #[test]
    fn every_quad_lands_in_exactly_one_target_besides_splits() {
        let s = frame();
        for sub in 0..3 {
            let mut seen = vec![];
            for t in [Target::Under, Target::Pane(1), Target::Pane(2), Target::Over] {
                for r in s.ranges(sub, t, 1000) {
                    seen.extend(r);
                }
            }
            seen.sort();
            seen.dedup();
            let splits: Vec<usize> = s.ranges(sub, Target::Splits, 1000).into_iter().flatten().collect();
            let total = [23, 82, 4][sub];
            // Every quad is somewhere; split quads are shared by the panes
            assert_eq!(seen.len(), total, "sub-layer {sub}, splits {splits:?}");
        }
    }

    #[test]
    fn ranges_stop_at_what_the_buffer_holds() {
        assert_eq!(frame().ranges(1, Target::Pane(1), 25), vec![0..25]);
        assert_eq!(frame().ranges(1, Target::Over, 25), vec![]);
    }

    #[test]
    fn a_new_pass_starts_clean() {
        let mut s = frame();
        s.set_active(2);
        assert_eq!(s.active(), Some(2));
        s.reset([0, 0, 0]);
        s.finish([2, 0, 0]);
        assert_eq!(s.ranges(0, Target::Under, 1000), vec![0..2]);
        assert!(s.panes().is_empty());
        assert_eq!(s.active(), None, "the keyboard's pane is this pass's to say");
    }

    #[test]
    fn snapped_neighbours_still_meet() {
        // Half-cell edges land on .5: both sides round the same way
        let left = PixelRect::snap(0.0, 30.0, 503.5, 570.0);
        let right = PixelRect::snap(503.5, 30.0, 496.5, 570.0);
        assert_eq!(left.x + left.w as i32, right.x);
        assert_eq!(right.x + right.w as i32, 1000);
    }

    #[test]
    fn the_open_panel_presses_the_panes_into_the_room_left() {
        // The panes' visuals run from 74 to 998; the panel opens 222 px wider
        let (left, travel, right) = (74, 222, 998);
        let first = PixelRect::new(74, 38, 429, 700).squeeze(left, travel, right);
        let second = PixelRect::new(503, 38, 495, 700).squeeze(left, travel, right);
        assert_eq!(first.x, 74 + 222, "the first moves over by the travel");
        assert_eq!(first.x + first.w as i32, second.x, "neighbours still meet");
        assert_eq!(second.x + second.w as i32, 998, "the last still ends where it did");
        assert!(second.w < 495 && first.w < 429);
        assert_eq!((second.y, second.h), (38, 700));
        // Past either end: moved over, or left where it is
        assert_eq!(PixelRect::new(0, 0, 10, 10).squeeze(left, travel, right).x, 222);
        assert_eq!(PixelRect::new(998, 0, 2, 10).squeeze(left, travel, right), PixelRect::new(998, 0, 2, 10));
        // Nothing to do when nothing moves
        assert_eq!(first.squeeze(left, 0, right), first);
    }

    #[test]
    fn snapping_follows_the_gpu_fill_rule() {
        // Pixel 503's centre is 503.5: on the edge, so it's the right-hand
        // pane's, as the rasteriser gives it to the quad starting there
        assert_eq!(PixelRect::snap(503.5, 0.0, 10.0, 10.0).x, 503);
        assert_eq!(PixelRect::snap(503.4, 0.0, 10.0, 10.0).x, 503);
        assert_eq!(PixelRect::snap(503.6, 0.0, 10.0, 10.0).x, 504);
        // Whole pixels stay put
        assert_eq!(PixelRect::snap(28.0, 30.0, 500.0, 600.0), PixelRect::new(28, 30, 500, 600));
    }
}
