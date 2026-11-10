//! LBTerm: the quads of something LBTerm draws that seldom changes, made once and copied into its
//! z-layer each frame while what they're made from stays the same (lbterm/PERF.md, "What the
//! cards cost"). The tab panel made its ring, its light, its lit gradient and the running light's
//! discs again on every frame, a few thousand quads with a sine and a cosine each, though they
//! change only as the window is resized or the panel's colours are: about 1.4 ms of a 3.7 ms
//! paint pass with fifteen tabs (2026-10-31), whatever the frame was for. Kept as the vertices
//! they're drawn from, with each quad's hash made already, they're copied into the layer's
//! vertex buffer in one go (renderstate.rs, `lbterm_extend_all_hashed`), and the compositor's
//! look at what changed doesn't hash them again either. Words drawn this way go into one
//! allocator for all of them (render/words.rs), where render_element makes one for each, three
//! mappings of the layer's vertex buffers a word.

use crate::quad::{HeapQuadAllocator, TripleLayerQuadAllocator, Vertex};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// One part's quads as the vertices each sub-layer draws them from with their hashes, and a
/// hash of what they were made from
#[derive(Default)]
pub(crate) struct KeptQuads {
    made: Option<u64>,
    layers: [(Vec<Vertex>, Vec<u64>); 3],
}

impl KeptQuads {
    /// Made again at the next frame whatever it's made from: the glyph atlas the corners' sprites
    /// are in has gone (gpu_lost.rs)
    pub(crate) fn forget(&mut self) {
        self.made = None;
        self.layers = Default::default();
    }
}

/// Several parts' quads kept, one a z-layer: a set of words some of which go in one layer and
/// some in another (the panel's numbers and labels on the rail, its words on the open panel)
#[derive(Default)]
pub(crate) struct KeptLayers {
    layers: Vec<(i8, KeptQuads)>,
}

impl KeptLayers {
    pub(crate) fn forget(&mut self) {
        self.layers.clear();
    }

    /// The part for z-layer `zindex`
    pub(crate) fn layer(&mut self, zindex: i8) -> &mut KeptQuads {
        let at = match self.layers.iter().position(|(z, _)| *z == zindex) {
            Some(at) => at,
            None => {
                self.layers.push((zindex, KeptQuads::default()));
                self.layers.len() - 1
            }
        };
        &mut self.layers[at].1
    }
}

impl crate::TermWindow {
    /// A part's quads into z-layer `zindex`: `make`'s, made into `kept` when `from` (what they're
    /// made from besides the window's size and its shape and quad generations) isn't what they
    /// were last made from, else copied from it as they were.
    pub(crate) fn lbterm_kept_quads(
        &self,
        kept: &RefCell<KeptQuads>,
        zindex: i8,
        from: impl Hash,
        make: impl FnOnce(&mut TripleLayerQuadAllocator) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.lbterm_kept_quads_in(&mut kept.borrow_mut(), zindex, from, make)
    }

    /// [`Self::lbterm_kept_quads`], `kept` borrowed already
    pub(crate) fn lbterm_kept_quads_in(
        &self,
        kept: &mut KeptQuads,
        zindex: i8,
        from: impl Hash,
        make: impl FnOnce(&mut TripleLayerQuadAllocator) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut hasher = DefaultHasher::new();
        (
            self.shape_generation,
            self.quad_generation,
            self.dimensions.pixel_width,
            self.dimensions.pixel_height,
            zindex,
        )
            .hash(&mut hasher);
        from.hash(&mut hasher);
        let at = hasher.finish();
        if kept.made != Some(at) {
            kept.forget();
            let mut heap = HeapQuadAllocator::default();
            make(&mut TripleLayerQuadAllocator::Heap(&mut heap))?;
            kept.layers = heap.lbterm_into_vertices();
            kept.made = Some(at);
            lbterm_extras::kept::QUADS.add(1, 0);
        } else {
            lbterm_extras::kept::QUADS.add(0, 1);
        }
        if kept.layers.iter().all(|(_, hashes)| hashes.is_empty()) {
            return Ok(());
        }
        let layer = self.render_state.as_ref().unwrap().layer_for_zindex(zindex)?;
        let mut quads = layer.quad_allocator();
        for (sub, (vertices, hashes)) in kept.layers.iter().enumerate() {
            if !hashes.is_empty() {
                quads.lbterm_extend_all_hashed(sub, vertices, hashes);
            }
        }
        Ok(())
    }
}

/// An area's place and size as bits, to hash
pub(crate) fn bits(a: lbterm_panel::Area) -> [u32; 4] {
    [a.x, a.y, a.w, a.h].map(f32::to_bits)
}

/// A layout's every field as bits, to hash: what the panel's parts are placed from. The label's
/// line and the new-tab pad's foot slot move every row (Layout::label_lines, Layout::foot), so
/// they're in it too
pub(crate) fn layout_bits(l: &lbterm_panel::Layout) -> ([u32; 9], usize, bool, bool) {
    (
        [l.left, l.body_top, l.bottom, l.right, l.rail, l.width, l.scale, l.label, l.label_line].map(f32::to_bits),
        l.rows,
        l.head,
        l.foot,
    )
}
