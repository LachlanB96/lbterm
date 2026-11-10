//! LBTerm: templates, made once for what they're made from (lbterm/COMPOSITOR.md, "What a frame
//! costs").
//!
//! With the compositor some z-layers hold quads that never show as they're drawn: the card
//! templates every pane's edges and the active outline are cut from (render/card.rs), the
//! Conductor's outlines and their light (render/tab_panel.rs, `tab_panel_rings`), and the panel's
//! glow, lit ring and running light, which it draws open and lit and DWM shows as much of as the
//! moment calls for. WezTerm's paint pass builds every quad of the window again every frame, and
//! these were built with it, quad by quad: about 1,600 for the tall card template alone in a
//! window 1,920 pixels tall, 900 for the Conductor's, 1,700 for the panel's glow and light, with a
//! sine and a cosine per corner and a colour turned to linear light per vertex, though nothing
//! they're made from had changed. Now each is made only when its key (the window's size, which
//! every quad is placed from the middle of, its DPI, where the white the quads are filled from is
//! in the glyph atlas, and what the template itself is made from) changes, and its layer keeps the
//! quads otherwise (`RenderLayer::lbterm_keep`, below, rather than in upstream's renderstate.rs,
//! which keeps only the fields). A kept layer's GPU copy isn't written again either: what it holds
//! is what it held.
//!
//! This is for a layer only its template writes into. Quads that share a layer with others and
//! seldom change are copied into it each frame instead (render/kept_quads.rs): the tab panel's
//! body, rows, marker, words and progress.

use crate::quad::TripleLayerQuadAllocator;
use crate::renderstate::{RenderContext, RenderLayer};
use lbterm_motion::content::{fold, fold_f32s, pair};

impl RenderLayer {
    /// Whether this layer's quads, which every paint pass starts by clearing, are this frame's
    /// again as they were made last from `key` (`lbterm_made`): then they're counted in once more,
    /// as they are, with their hashes, and nothing is written. Only with WebGpu, whose layer holds
    /// what was written into it from one frame to the next (termwindow/gpu_work.rs); Glium turns
    /// through three buffers. For a layer one owner alone writes into, whose quads are a template
    /// made from a few things
    pub fn lbterm_keep(&self, key: u64) -> bool {
        if !matches!(self.context, RenderContext::WebGpu(_)) {
            return false;
        }
        let Some((made, counts)) = self.lbterm_kept.get() else {
            return false;
        };
        if made != key {
            return false;
        }
        let vbs = self.vb.borrow();
        if vbs.iter().zip(&counts).any(|(vb, &n)| n > vb.capacity || *vb.next_quad.borrow() != 0) {
            return false;
        }
        for (vb, &n) in vbs.iter().zip(&counts) {
            *vb.next_quad.borrow_mut() = n;
        }
        true
    }

    /// This frame's quads in this layer were made from `key`, and are kept for a frame that asks
    /// for the same (`lbterm_keep`). Not when any went past what the layer holds: the pass is
    /// painted again once it holds more, and they're made again
    pub fn lbterm_made(&self, key: u64) {
        let vbs = self.vb.borrow();
        let counts = [0, 1, 2].map(|i| *vbs[i].next_quad.borrow());
        let fits = vbs.iter().zip(&counts).all(|(vb, &n)| n <= vb.capacity);
        self.lbterm_kept.set(fits.then_some((key, counts)));
    }
}

impl crate::TermWindow {
    /// What every template's key starts from, with `parts` (what the template itself is made
    /// from: `pair` for two numbers, a colour as it is) after: the window's size, as every quad
    /// is placed from its middle; its DPI; where in the atlas the white is that the quads are
    /// filled from, which a new atlas can move; and the shape and quad generations, which a
    /// config reload, a new font, a new atlas, a lost GPU and a resize move, as KeptQuads'
    /// key has them (render/kept_quads.rs): nothing a template is made from changes without
    /// one of these or its own parts changing
    pub(crate) fn lbterm_template_key(&self, parts: &[u64]) -> u64 {
        let gl_state = self.render_state.as_ref().unwrap();
        let white = gl_state.util_sprites.filled_box.texture_coords();
        let mut key = 0x6c62_7465_726d_0004;
        fold(&mut key, self.dimensions.pixel_width as u64);
        fold(&mut key, self.dimensions.pixel_height as u64);
        fold(&mut key, self.dimensions.dpi as u64);
        fold(&mut key, self.shape_generation as u64);
        fold(&mut key, self.quad_generation as u64);
        fold_f32s(&mut key, &[white.min_x(), white.min_y(), white.max_x(), white.max_y()]);
        fold(&mut key, parts.len() as u64);
        for &part in parts {
            fold(&mut key, part);
        }
        key
    }

    /// Z-layer `zindex`'s quads this frame: made by `make` only when `key` (`lbterm_template_key`)
    /// isn't what they were last made from, kept as they were otherwise. `zindex` must be this
    /// template's alone: nothing else may write into it. `name` is what the state file calls it
    /// when it's made (`visuals.built`, lbterm/STATE.md)
    pub(crate) fn lbterm_kept_layer(
        &self,
        zindex: i8,
        name: &'static str,
        key: u64,
        make: impl FnOnce(&mut TripleLayerQuadAllocator) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(zindex)?;
        if layer.lbterm_keep(key) {
            return Ok(());
        }
        {
            let mut quads = layer.quad_allocator();
            make(&mut quads)?;
        }
        layer.lbterm_made(key);
        self.lbterm_state_built(name);
        Ok(())
    }
}

/// An `Area`'s place and size, for a template's key
pub(crate) fn area(a: lbterm_panel::Area) -> [u64; 2] {
    [pair(a.x, a.y), pair(a.w, a.h)]
}

/// Everything the tab panel's layout is, for a template's key: where everything on the panel
/// goes is made from it
pub(crate) fn layout(l: &lbterm_panel::Layout) -> [u64; 5] {
    [
        pair(l.left, l.body_top),
        pair(l.bottom, l.right),
        pair(l.rail, l.width),
        pair(l.scale, l.label),
        (l.rows as u64) << 1 | l.head as u64,
    ]
}
