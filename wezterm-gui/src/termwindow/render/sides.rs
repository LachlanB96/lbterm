//! LBTerm: the strips down the window's right, the scroll strip and the objectives strip with its
//! projects pad (render/scroll_strip.rs, render/objectives.rs, render/projects.rs), fading into
//! the next tab's with the panes (lbterm/DESIGN.md, "Other moments").
//!
//! They're drawn in `over` and never move. Each frame what they draw is kept as a picture: the
//! shapes, in the order drawn. When the panes on screen all give way to others (a tab switched
//! from or closed, an overlay closing) and dissolve over them, the picture the frame before showed
//! is drawn once more, into a visual of its own just over `over`, which DWM fades out with the
//! panes' tab in flight (`lbterm_motion::flight`), at the opacity that shows its share: the old
//! tab's thumb and marks fade out over the new tab's, in step with its panes. The strips' colour
//! is in the picture, so it's opaque and the fade is a true cross-fade, the new strips there at
//! once under it. A tab switched back to takes its picture away as its panes come back whole,
//! pictures still fading stack oldest on top, as the panes do, and each whose tab's opacity is
//! played again to keep its share (a tab further back switched back to) plays it again too. Only
//! that one frame draws a picture: its visual keeps it while it fades. The strips' cards fade on
//! their own (`pane_enter`, `pane_exit`), over it. Without the compositor, or with motion off, the
//! strips change at once.

use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::render::objectives::STRIP_Z;
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use mux::pane::PaneId;
use window::color::LinearRgba;

/// The strips as they were, drawn for their visual alone (`Target::Sides`)
const PICTURE_Z: i8 = 19;

/// Whether a z-layer is the strips' picture's
pub fn owns_zindex(zindex: i8) -> bool {
    zindex == PICTURE_Z
}

/// One shape the strips draw: a rectangle of one colour, its corners rounded to `radius`
/// (`lbterm_menu_rounded`'s, square under a pixel)
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Shape {
    pub rect: Rect,
    pub radius: f64,
    pub colour: LinearRgba,
}

impl Shape {
    pub(crate) fn new(rect: Rect, radius: f64, colour: LinearRgba) -> Self {
        Self { rect, radius, colour }
    }
}

/// The three lists of shapes swap places as frames go by, and the panes' ids are kept in lists
/// cleared and filled again, so nothing is allocated again on every frame (typing and output at
/// full speed included) once they've grown to the strips' size
#[derive(Default)]
pub struct SidesState {
    /// What the strips have drawn so far this frame, in order
    drawing: Vec<Shape>,
    /// This frame's picture, once they all have
    frame: Vec<Shape>,
    /// The picture the last frame presented, the panes it showed, and the window's size then
    shown: Vec<Shape>,
    shown_panes: Vec<PaneId>,
    shown_window: (usize, usize),
    /// This frame's panes, while they're compared with the last frame's
    panes: Vec<PaneId>,
    /// Where the picture drawn again this frame is, for the compositor
    picture: Option<PixelRect>,
    /// Whether this frame's paint pass got as far as the strips: one that failed before them
    /// is still drawn and presented, and `frame` then holds an older picture
    painted: bool,
}

impl crate::TermWindow {
    /// Before the strips draw: none of this frame's shapes yet
    pub(crate) fn lbterm_sides_begin(&mut self) {
        self.lbterm_sides.drawing.clear();
    }

    /// Shapes a strip draws this frame, over the ones drawn before them
    pub(crate) fn lbterm_sides_draw(&mut self, shapes: Vec<Shape>) {
        self.lbterm_sides.drawing.extend(shapes);
    }

    /// `shapes` into `quads`' layer 0, in order
    pub(crate) fn lbterm_shapes_into(&self, quads: &mut TripleLayerQuadAllocator, shapes: &[Shape]) -> anyhow::Result<()> {
        for s in shapes {
            self.lbterm_menu_rounded(quads, s.rect, s.radius, s.colour)?;
        }
        Ok(())
    }

    pub(crate) fn lbterm_shapes(&self, zindex: i8, shapes: &[Shape]) -> anyhow::Result<()> {
        if shapes.is_empty() {
            return Ok(());
        }
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(zindex)?;
        let mut quads = layer.quad_allocator();
        self.lbterm_shapes_into(&mut quads, shapes)
    }

    /// After the strips have drawn: what they drew, in `over`; and when every pane on screen gave
    /// way to another this frame, the picture the last frame showed again, for its visual, should
    /// it look different
    pub(crate) fn paint_lbterm_sides(&mut self) -> anyhow::Result<()> {
        // This frame's shapes become the frame's picture; what was there is cleared at the next
        // `lbterm_sides_begin`
        {
            let state = &mut self.lbterm_sides;
            std::mem::swap(&mut state.drawing, &mut state.frame);
            state.panes.clear();
            state.panes.extend(self.lbterm_spans.panes().iter().map(|(id, _)| *id));
            state.painted = true;
        }
        self.lbterm_shapes(STRIP_Z, &self.lbterm_sides.frame)?;
        let window = (self.dimensions.pixel_width, self.dimensions.pixel_height);
        let state = &self.lbterm_sides;
        let picture = if state.shown_window == window
            && lbterm_motion::gave_way(&state.shown_panes, &state.panes)
            && state.shown != state.frame
            && self.lbterm_composited()
            && self.lbterm_motion().enabled
        {
            let rects = state.shown.iter().map(|s| lbterm_motion::Rect::new(s.rect.x, s.rect.y, s.rect.w, s.rect.h));
            PixelRect::covering(rects, (window.0 as u32, window.1 as u32))
        } else {
            None
        };
        if picture.is_some() {
            log::debug!("sides: the strips as they were over {picture:?}, {} shapes, the panes gave way", state.shown.len());
            self.lbterm_shapes(PICTURE_Z, &self.lbterm_sides.shown)?;
        }
        self.lbterm_sides.picture = picture;
        Ok(())
    }

    /// Where this frame's picture of the strips as they were is, for the compositor, which fades
    /// it if the panes dissolved; `None` without one
    pub(crate) fn lbterm_sides_picture(&self) -> Option<PixelRect> {
        self.lbterm_sides.picture
    }

    /// The frame is presented: its picture of the strips, and its panes, are what show now. The
    /// picture before goes to `frame`, overwritten by the next paint. After a paint pass that
    /// failed before the strips drew, what shows of them isn't known, so there's no picture to
    /// fade from; the panes are the ones presented, whichever
    pub(crate) fn lbterm_sides_presented(&mut self) {
        let window = (self.dimensions.pixel_width, self.dimensions.pixel_height);
        let spans = &self.lbterm_spans;
        let state = &mut self.lbterm_sides;
        if std::mem::take(&mut state.painted) {
            std::mem::swap(&mut state.shown, &mut state.frame);
        } else {
            state.shown.clear();
        }
        state.shown_panes.clear();
        state.shown_panes.extend(spans.panes().iter().map(|(id, _)| *id));
        state.shown_window = window;
        state.picture = None;
    }
}
