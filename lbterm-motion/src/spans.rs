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
    /// Over the tab panel and the menus: a tab link drag (z-layers 88 to 92). Only in a frame
    /// while something is drawn there, and for one cleared frame after. The menus were here
    /// until 2026-11-05; they're `MenuCard` and `MenuLight` now. The compositor's `top` visual
    /// holds this target's surface and the lights round the drag's matched blocks
    /// (`prepare_link_lamps`), which show a template of their own, not this target; the
    /// Conductor menu's turning arc (`MenuSpinner`) is in its menu's card, not `top`.
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
    /// One of the objectives panel's three cards over the panes on the right, the list of each
    /// tab's first objective (0, z-layers 39 to 41), one session's whole list (1, 42 to 44) and
    /// the projects card (2, 27 to 29): faded in and out by DWM (wezterm-gui's render/objectives.rs
    /// and render/projects.rs).
    Objectives(usize),
    /// The context bar's moving parts over the panes on the right (wezterm-gui's
    /// render/context_bar.rs): its pane (0, z-layers 101 to 103), its widened bar's drawer of labels
    /// (1, 105 to 107), each sliding out from under the bar's edge by DWM, and the scroll strip
    /// riding on their left edge (2, z-layer 104), glided by DWM.
    ContextBar(usize),
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
    /// The strips down the window's right (the scroll strip and the objectives strip) as the tab
    /// left showed them, over the next tab's while they fade out with its panes (z-layer 19;
    /// wezterm-gui's render/sides.rs). Only the frame a tab is left in draws one: each keeps that
    /// frame while it fades
    Sides,
    /// The card of what a tab's Claude session has running, beside its row on the open tab
    /// panel (z-layers 80 to 82), over the panel and under the tab menu: faded in and out by
    /// DWM (wezterm-gui's render/tasks_card.rs).
    TasksCard,
    /// The light running round a tab link's matched blocks while a tab is dragged onto the tab
    /// its wait matches (z-layer 92): a template never seen itself, which a visual per block over
    /// the drag's cards shows, carried round by DWM (wezterm-gui's render/link_drag.rs).
    LinkLamp,
    /// The light running to and fro along each suggested tab connection's line down the rail
    /// (z-layer 97): a template never seen itself, which a visual per line over the tab panel
    /// shows, carried along the line by DWM (wezterm-gui's render/connections.rs).
    ConnectionLamp,
    /// The easter egg's hearts (z-layer 20): a sheet of one heart in each of their pinks, never
    /// seen itself, which a visual per heart shows, each risen, swayed, grown and faded by DWM
    /// (wezterm-gui's render/triggers.rs).
    Hearts,
    /// The new-tab picker beside the rail (z-layers 93 to 95), over the panel and under its
    /// pads' glints: faded in and out by DWM (wezterm-gui's render/new_tab.rs).
    Picker,
    /// The new-tab pad's look under the pointer (z-layer 96), over the panel: faded in and out by
    /// DWM on `button_hover`, as the strip's hover slots are (wezterm-gui's render/new_tab.rs).
    PlusHover,
    /// One of the menus' cards (wezterm-gui's render/menu_fade.rs): its light, fill and words,
    /// the z-layers of the menus in slot `n` (0 the tab menu, 60 to 62; 1 the prompt list, 63 to
    /// 65; 2 the Conductor's, 66 to 68; 3 the InMail list, the agenda and the + menu, 77 to 79).
    /// Over the panel, faded in and out by DWM with the slot's highlight.
    MenuCard(usize),
    /// The slot's highlight, over its card: a white surface, never drawn into, seen through a
    /// rounded clip that glides between rows (`crate::menu::light`). No quads: cleared to white,
    /// and only in a frame when it's new or a new size.
    MenuLight(usize),
    /// The questions pane down the window's right while a right hold shows the question colours
    /// (z-layers 101 and 102), over the strips and the panes' last columns: slid out from under
    /// the window's right edge by DWM on `panel_open` as the hold starts and back on
    /// `panel_close` as it ends (wezterm-gui's render/questions_pane.rs).
    QuestionsPane,
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
    /// The white arc's template, shown on the Starting row's pad while a new tab is on its way
    StartSpinner,
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
    /// The wave of characters that runs ahead of the edge as it opens and on out over the panes:
    /// drawn once where its run ends and carried there a cell at a time
    Wave,
    /// The spray it throws where its crest breaks, drawn once where it's thrown
    Spray,
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

    /// The pixels `rects` cover between them, their edges rounded out so none is cut, inside a
    /// window `window` big; rectangles of no size take no room. A visual that holds what they
    /// drew (wezterm-gui's render/sides.rs)
    pub fn covering(rects: impl IntoIterator<Item = crate::Rect>, window: (u32, u32)) -> Option<PixelRect> {
        let mut it = rects.into_iter().filter(|r| r.w > 0.0 && r.h > 0.0);
        let first = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
        for r in it {
            x0 = x0.min(r.x);
            y0 = y0.min(r.y);
            x1 = x1.max(r.right());
            y1 = y1.max(r.bottom());
        }
        let x0 = x0.floor().max(0.0) as i32;
        let y0 = y0.floor().max(0.0) as i32;
        let x1 = (x1.ceil() as i32).min(window.0 as i32);
        let y1 = (y1.ceil() as i32).min(window.1 as i32);
        (x1 > x0 && y1 > y0).then(|| PixelRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
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

/// How many command lists a frame of `visuals` visuals needs: none for a
/// frame that draws none, which then submits nothing.
pub fn command_lists(visuals: usize) -> usize {
    visuals.div_ceil(SWAPCHAINS_PER_COMMAND_LIST)
}

/// What each of a frame's visuals draws, gathered z-layer by z-layer in the
/// order they're drawn, so that each visual is drawn in one render pass
/// (lbterm/COMPOSITOR.md, "What a frame costs"): its pass binds each layer
/// it has quads in, in turn, and draws them. `B` is a layer's buffers as the
/// draw binds them. Drawn layer by layer, a visual took a pass of its own for
/// every z-layer and sub-layer it had quads in, and one more to clear it
/// when it had none: each pass makes wgpu track and move its swapchain's
/// texture again and close a D3D12 command list of its own.
#[derive(Debug)]
pub struct Passes<B> {
    layers: Vec<B>,
    /// Each visual's draws: a layer, by its place in `layers`, and the runs
    /// of its quads
    draws: Vec<Vec<(usize, Vec<Range<usize>>)>>,
}

impl<B> Passes<B> {
    /// A frame drawing `visuals` visuals, nothing gathered yet
    pub fn new(visuals: usize) -> Self {
        Self {
            layers: vec![],
            draws: (0..visuals).map(|_| vec![]).collect(),
        }
    }

    /// The next layer, in the order they're drawn: `buffers`, and the runs
    /// of its quads each visual draws (by its place in the frame; one with
    /// none isn't listed). A layer no visual draws from isn't kept
    pub fn layer(&mut self, buffers: B, wanted: impl IntoIterator<Item = (usize, Vec<Range<usize>>)>) {
        let at = self.layers.len();
        let mut used = false;
        for (visual, ranges) in wanted {
            if !ranges.is_empty() {
                self.draws[visual].push((at, ranges));
                used = true;
            }
        }
        if used {
            self.layers.push(buffers);
        }
    }

    /// How many visuals the frame draws: one pass each
    pub fn visuals(&self) -> usize {
        self.draws.len()
    }

    /// What `visual`'s pass draws: each layer's buffers and the runs of its
    /// quads, in the order the layers were given
    pub fn of(&self, visual: usize) -> impl Iterator<Item = (&B, &[Range<usize>])> {
        self.draws[visual]
            .iter()
            .map(|(layer, ranges)| (&self.layers[*layer], ranges.as_slice()))
    }
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
        // A frame that draws nothing submits nothing
        assert_eq!(command_lists(0), 0);
    }

    #[test]
    fn each_visual_draws_every_layer_it_has_quads_in_in_one_pass_in_order() {
        // Three visuals: under (0), a pane (1) and over (2), drawn from three
        // layers, z-layer -1, 0 and 3, as the paint pass ordered them
        let mut passes = Passes::new(3);
        passes.layer("z-1", vec![(0, vec![0..4])]);
        passes.layer("z0", vec![(0, vec![0..2]), (1, vec![2..10, 12..15]), (2, vec![])]);
        passes.layer("z3", vec![(2, vec![0..7])]);
        // A layer no visual draws from isn't bound at all
        passes.layer("z9", vec![(1, vec![])]);
        assert_eq!(passes.visuals(), 3);
        let of = |v| passes.of(v).map(|(b, r)| (*b, r.to_vec())).collect::<Vec<_>>();
        assert_eq!(of(0), vec![("z-1", vec![0..4]), ("z0", vec![0..2])]);
        assert_eq!(of(1), vec![("z0", vec![2..10, 12..15])]);
        assert_eq!(of(2), vec![("z3", vec![0..7])]);
        assert_eq!(passes.layers, vec!["z-1", "z0", "z3"]);
        // A visual with nothing in any layer still gets its pass, which only
        // clears it
        let empty: Passes<&str> = Passes::new(1);
        assert_eq!(empty.of(0).count(), 0);
        assert_eq!(empty.visuals(), 1);
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

    #[test]
    fn a_visual_covers_what_was_drawn_rounded_out() {
        let r = crate::Rect::new;
        // The scroll strip's column and the objectives strip's beside it, a mark half a pixel in
        let drawn = [r(900.0, 40.0, 18.0, 600.0), r(918.0, 40.0, 18.0, 600.0), r(903.5, 100.25, 7.0, 7.0)];
        assert_eq!(PixelRect::covering(drawn, (1000, 700)), Some(PixelRect::new(900, 40, 36, 600)));
        // Fractions of a pixel round out, never in
        assert_eq!(PixelRect::covering([r(10.4, 20.6, 3.2, 3.2)], (1000, 700)), Some(PixelRect::new(10, 20, 4, 4)));
    }

    #[test]
    fn a_visual_covering_what_was_drawn_stays_in_the_window() {
        let r = crate::Rect::new;
        assert_eq!(PixelRect::covering([r(990.0, 650.0, 40.0, 100.0)], (1000, 700)), Some(PixelRect::new(990, 650, 10, 50)));
        assert_eq!(PixelRect::covering([r(-5.0, -5.0, 10.0, 10.0)], (1000, 700)), Some(PixelRect::new(0, 0, 5, 5)));
        assert_eq!(PixelRect::covering([r(1000.0, 10.0, 10.0, 10.0)], (1000, 700)), None);
        // Nothing of any size: no room taken
        assert_eq!(PixelRect::covering([r(5.0, 5.0, 0.0, 10.0), r(20.0, 20.0, 2.0, 2.0)], (1000, 700)), Some(PixelRect::new(20, 20, 2, 2)));
        assert_eq!(PixelRect::covering([], (1000, 700)), None);
    }
}
