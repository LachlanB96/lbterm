// Clippy hates the implement_vertex macro and won't let me scope
// this warning to its use
#![allow(clippy::unneeded_field_pattern)]

use crate::renderstate::BorrowedLayers;
use ::window::bitmaps::TextureRect;
use ::window::color::LinearRgba;
use config::HsbTransform;

/// Each cell is composed of two triangles built from 4 vertices.
/// The buffer is organized row by row.
pub const VERTICES_PER_CELL: usize = 4;
pub const V_TOP_LEFT: usize = 0;
pub const V_TOP_RIGHT: usize = 1;
pub const V_BOT_LEFT: usize = 2;
pub const V_BOT_RIGHT: usize = 3;

// LBTerm: what a quad is given, folded into one number as it's written, so a frame can tell
// which visuals' quads are what they were last frame without reading them back out of the GPU's
// upload memory, which is slow to read (render/composited.rs; lbterm_motion::content;
// lbterm/COMPOSITOR.md, "Only what changed is drawn"). Every write to a quad's vertices folds
// what it writes, so two quads given the same calls with the same values hash the same, and so
// draw the same.
use lbterm_motion::content::fold;

#[inline]
fn fold2(hash: &mut u64, a: f32, b: f32) {
    fold(hash, lbterm_motion::content::pair(a, b));
}

#[inline]
fn fold_rgba(hash: &mut u64, c: [f32; 4]) {
    fold2(hash, c[0], c[1]);
    fold2(hash, c[2], c[3]);
}

/// LBTerm: the hash of quads copied in whole (`QuadAllocator::extend_with`), from their bytes.
/// A `HeapQuadAllocator`'s quads (every pane's lines) come with theirs made already
/// (`HeapQuadAllocator::apply_to`), so this is for any other copy
pub(crate) fn fold_vertices(hash: &mut u64, vertices: &[Vertex]) {
    let words: &[u32] = bytemuck::cast_slice(vertices);
    let mut pairs = words.chunks_exact(2);
    for pair in &mut pairs {
        fold(hash, (pair[0] as u64) << 32 | pair[1] as u64);
    }
    for &last in pairs.remainder() {
        fold(hash, last as u64);
    }
}

/// a regular monochrome text glyph
const IS_GLYPH: f32 = 0.0;
/// a color emoji glyph
const IS_COLOR_EMOJI: f32 = 1.0;
/// a full color texture attached as the
/// background image of the window
const IS_BG_IMAGE: f32 = 2.0;
/// like 2.0, except that instead of an
/// image, we use the solid bg color
const IS_SOLID_COLOR: f32 = 3.0;
/// Grayscale poly quad for non-aa text render layers
const IS_GRAY_SCALE: f32 = 4.0;

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    // Physical position of the corner of the character cell
    pub position: [f32; 2],
    // glyph texture
    pub tex: [f32; 2],
    pub fg_color: [f32; 4],
    pub alt_color: [f32; 4],
    pub hsv: [f32; 3],
    pub has_color: f32,
    pub mix_value: f32,
}
::window::glium::implement_vertex!(
    Vertex, position, tex, fg_color, alt_color, hsv, has_color, mix_value
);

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x4,
    3 => Float32x4,
    4 => Float32x3,
    5 => Float32,
    6 => Float32,
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

pub trait QuadTrait {
    /// Assign the texture coordinates
    fn set_texture(&mut self, coords: TextureRect) {
        let x1 = coords.min_x();
        let x2 = coords.max_x();
        let y1 = coords.min_y();
        let y2 = coords.max_y();
        self.set_texture_discrete(x1, x2, y1, y2);
    }
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32);
    fn set_has_color_impl(&mut self, has_color: f32);

    /// Set the color glyph "flag"
    fn set_has_color(&mut self, has_color: bool) {
        self.set_has_color_impl(if has_color { IS_COLOR_EMOJI } else { IS_GLYPH });
    }

    /// Mark as a grayscale polyquad; color and alpha will be
    /// multipled with those in the texture
    fn set_grayscale(&mut self) {
        self.set_has_color_impl(IS_GRAY_SCALE);
    }

    /// Mark this quad as a background image.
    /// Mutually exclusive with set_has_color.
    fn set_is_background_image(&mut self) {
        self.set_has_color_impl(IS_BG_IMAGE);
    }

    fn set_is_background(&mut self) {
        self.set_has_color_impl(IS_SOLID_COLOR);
    }

    fn set_fg_color(&mut self, color: LinearRgba);

    /// Must be called after set_fg_color
    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32);

    fn set_hsv(&mut self, hsv: Option<HsbTransform>);
    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32);
}

pub enum QuadImpl<'a> {
    Vert(Quad<'a>),
}

impl<'a> QuadTrait for QuadImpl<'a> {
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32) {
        match self {
            Self::Vert(q) => q.set_texture_discrete(x1, x2, y1, y2),
        }
    }

    fn set_has_color_impl(&mut self, has_color: f32) {
        match self {
            Self::Vert(q) => q.set_has_color_impl(has_color),
        }
    }

    fn set_fg_color(&mut self, color: LinearRgba) {
        match self {
            Self::Vert(q) => q.set_fg_color(color),
        }
    }

    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32) {
        match self {
            Self::Vert(q) => q.set_alt_color_and_mix_value(color, mix_value),
        }
    }

    fn set_hsv(&mut self, hsv: Option<HsbTransform>) {
        match self {
            Self::Vert(q) => q.set_hsv(hsv),
        }
    }

    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        match self {
            Self::Vert(q) => q.set_position(left, top, right, bottom),
        }
    }
}

impl<'a> QuadImpl<'a> {
    /// LBTerm: four corners anywhere (top-left, top-right, bottom-left,
    /// bottom-right, as the quad's two triangles take them), each with its
    /// own colour, blended across the quad. The tab panel's light is drawn
    /// with these (lbterm/TAB-PANEL.md).
    pub fn set_corners(&mut self, corners: [[f32; 2]; VERTICES_PER_CELL], colors: [LinearRgba; VERTICES_PER_CELL]) {
        match self {
            Self::Vert(q) => {
                fold(q.hash, 7);
                for (corner, color) in corners.iter().zip(&colors) {
                    fold2(q.hash, corner[0], corner[1]);
                    fold_rgba(q.hash, (*color).into());
                }
                for (i, v) in q.vert.iter_mut().enumerate() {
                    v.position = corners[i];
                    v.fg_color = colors[i].into();
                    v.alt_color = colors[i].into();
                    v.mix_value = 0.;
                }
            }
        }
    }

    /// LBTerm: the tint shades from `left` at the quad's left edge to `right` at its right,
    /// for a glyph a rainbow trigger colours (render/triggers.rs). After set_fg_color.
    pub fn set_fg_gradient(&mut self, left: LinearRgba, right: LinearRgba) {
        match self {
            Self::Vert(q) => {
                fold(q.hash, 8);
                fold_rgba(q.hash, left.into());
                fold_rgba(q.hash, right.into());
                for (i, v) in q.vert.iter_mut().enumerate() {
                    let c = if i == V_TOP_LEFT || i == V_BOT_LEFT { left } else { right };
                    v.fg_color = c.into();
                    v.alt_color = c.into();
                    v.mix_value = 0.;
                }
            }
        }
    }
}

/// A helper for updating the 4 vertices that compose a glyph cell
pub struct Quad<'a> {
    pub(crate) vert: &'a mut [Vertex],
    /// LBTerm: what it's been given, folded (`fold`)
    pub(crate) hash: &'a mut u64,
}

impl<'a> QuadTrait for Quad<'a> {
    fn set_texture_discrete(&mut self, x1: f32, x2: f32, y1: f32, y2: f32) {
        fold(self.hash, 1);
        fold2(self.hash, x1, x2);
        fold2(self.hash, y1, y2);
        self.vert[V_TOP_LEFT].tex = [x1, y1];
        self.vert[V_TOP_RIGHT].tex = [x2, y1];
        self.vert[V_BOT_LEFT].tex = [x1, y2];
        self.vert[V_BOT_RIGHT].tex = [x2, y2];
    }

    fn set_has_color_impl(&mut self, has_color: f32) {
        fold2(self.hash, 2.0, has_color);
        for v in self.vert.iter_mut() {
            v.has_color = has_color;
        }
    }

    fn set_fg_color(&mut self, color: LinearRgba) {
        fold(self.hash, 3);
        fold_rgba(self.hash, color.into());
        for v in self.vert.iter_mut() {
            v.fg_color = color.into();
        }
        self.set_alt_color_and_mix_value(color, 0.);
    }

    /// Must be called after set_fg_color
    fn set_alt_color_and_mix_value(&mut self, color: LinearRgba, mix_value: f32) {
        fold2(self.hash, 4.0, mix_value);
        fold_rgba(self.hash, color.into());
        for v in self.vert.iter_mut() {
            v.alt_color = color.into();
            v.mix_value = mix_value;
        }
    }

    fn set_hsv(&mut self, hsv: Option<HsbTransform>) {
        let (h, s, v) = hsv
            .map(|t| (t.hue, t.saturation, t.brightness))
            .unwrap_or((1., 1., 1.));
        fold2(self.hash, 5.0, h);
        fold2(self.hash, s, v);
        for vert in self.vert.iter_mut() {
            vert.hsv = [h, s, v];
        }
    }

    fn set_position(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        fold(self.hash, 6);
        fold2(self.hash, left, top);
        fold2(self.hash, right, bottom);
        self.vert[V_TOP_LEFT].position = [left, top];
        self.vert[V_TOP_RIGHT].position = [right, top];
        self.vert[V_BOT_LEFT].position = [left, bottom];
        self.vert[V_BOT_RIGHT].position = [right, bottom];
    }
}

pub trait QuadAllocator {
    fn allocate(&mut self) -> anyhow::Result<QuadImpl<'_>>;
    fn extend_with(&mut self, vertices: &[Vertex]);
}

pub trait TripleLayerQuadAllocatorTrait {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>>;
    fn extend_with(&mut self, layer_num: usize, vertices: &[Vertex]);
}

/// LBTerm: quads drawn into memory of their own rather than a layer's buffer, to be copied into
/// layers later, frame after frame: a pane's lines (the line quad cache, render/pane.rs), the
/// Conductor's view's shapes and what render/kept_quads.rs keeps. Each sub-layer's quads are the
/// vertices they're drawn from, four a quad, with each quad's hash, both written as a quad mapped
/// from a layer's buffer is (renderstate.rs, `MappedQuads::allocate`), so copying them in is one
/// copy of the vertices and one of the hashes. Upstream kept a `Box<BoxedQuad>` a quad (a heap
/// allocation a quad laid out, and its four vertices built again from it each time it was copied
/// in: about 1.3 ms of a frame of 92 rows, lbterm/FLOWS.md row 86), to keep large arrays of
/// vertices from reallocating; a line's are a few kilobytes, and a kept line is made the size it
/// holds (`lbterm_exact`)
#[derive(Default, Clone)]
pub struct HeapQuadAllocator {
    layers: [(Vec<Vertex>, Vec<u64>); 3],
}

impl std::fmt::Debug for HeapQuadAllocator {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("HeapQuadAllocator").finish()
    }
}

impl HeapQuadAllocator {
    /// The quads copied into `other` as they were drawn, with their hashes
    pub fn apply_to(&self, other: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        for (layer_num, (vertices, hashes)) in self.layers.iter().enumerate() {
            if !hashes.is_empty() {
                other.lbterm_extend_all_hashed(layer_num, vertices, hashes);
            }
        }
        metrics::histogram!("quad_buffer_apply").record(start.elapsed());
        Ok(())
    }

    /// LBTerm: a pane's line, laid out at row 0, copied into `other` `dy` pixels lower, at its
    /// row, each quad's hash moved with it (`lbterm_motion::content::moved`), so the compositor
    /// sees the line moved (render/pane.rs)
    pub fn lbterm_apply_moved(&self, other: &mut TripleLayerQuadAllocator, dy: f32) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        for (layer_num, (vertices, hashes)) in self.layers.iter().enumerate() {
            if !hashes.is_empty() {
                other.lbterm_extend_all_moved(layer_num, vertices, hashes, dy);
            }
        }
        metrics::histogram!("quad_buffer_apply").record(start.elapsed());
        Ok(())
    }

    /// LBTerm: each sub-layer's quads as the vertices they're drawn from and their hashes, for
    /// quads copied in whole frame after frame (render/kept_quads.rs,
    /// `TripleLayerQuadAllocator::lbterm_extend_all_hashed`)
    pub fn lbterm_into_vertices(self) -> [(Vec<Vertex>, Vec<u64>); 3] {
        self.layers
    }

    /// LBTerm: what it holds, in allocations just its size, and itself emptied with its own kept:
    /// a pane's lines are laid out one after another in one allocator (render/pane.rs), and each
    /// is kept in the line quad cache at the size it is, where a vector grown as a line was laid
    /// out would hold up to half again unused
    pub fn lbterm_exact(&mut self) -> Self {
        let kept = self.clone();
        self.lbterm_clear();
        kept
    }

    /// LBTerm: emptied, its allocations kept for what's drawn into it next
    pub fn lbterm_clear(&mut self) {
        for (vertices, hashes) in &mut self.layers {
            vertices.clear();
            hashes.clear();
        }
    }

    /// LBTerm: the quads it holds, all sub-layers
    pub fn lbterm_quads(&self) -> usize {
        self.layers.iter().map(|(_, hashes)| hashes.len()).sum()
    }

    /// LBTerm: quads `vertices` drawn somewhere else with their `hashes` added, `dy` pixels lower
    /// (`lbterm_motion::content::place`)
    fn lbterm_extend_moved(&mut self, layer_num: usize, vertices: &[Vertex], hashes: &[u64], dy: f32) {
        let (to, to_hashes) = &mut self.layers[layer_num];
        let quads = hashes.len().min(vertices.len() / VERTICES_PER_CELL);
        let (at, hashed) = (to.len(), to_hashes.len());
        to.resize(at + quads * VERTICES_PER_CELL, Vertex::default());
        to_hashes.resize(hashed + quads, 0);
        lbterm_motion::content::place(
            &mut to[at..],
            &mut to_hashes[hashed..],
            vertices,
            hashes,
            VERTICES_PER_CELL,
            dy,
            move_down,
        );
    }
}

/// LBTerm: a vertex moved `dy` pixels down (`lbterm_motion::content::place`)
#[inline]
pub(crate) fn move_down(v: &mut Vertex, dy: f32) {
    v.position[1] += dy;
}

impl TripleLayerQuadAllocatorTrait for HeapQuadAllocator {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        let (vertices, hashes) = &mut self.layers[layer_num];
        let at = vertices.len();
        vertices.resize(at + VERTICES_PER_CELL, Vertex::default());
        hashes.push(0);
        let mut quad = Quad {
            vert: &mut vertices[at..],
            hash: hashes.last_mut().expect("just pushed"),
        };
        // As a quad mapped from a layer's buffer starts (renderstate.rs, `MappedQuads::allocate`),
        // so the same calls give the same vertices and the same hash in either
        quad.set_has_color(false);
        Ok(QuadImpl::Vert(quad))
    }

    fn extend_with(&mut self, layer_num: usize, vertices: &[Vertex]) {
        if vertices.is_empty() {
            return;
        }
        assert_eq!(vertices.len() % VERTICES_PER_CELL, 0);
        let (to, hashes) = &mut self.layers[layer_num];
        to.extend_from_slice(vertices);
        // Each quad's hash from its vertices, as MappedQuads::extend_with makes it
        for verts in vertices.chunks_exact(VERTICES_PER_CELL) {
            let mut hash = 0;
            fold_vertices(&mut hash, verts);
            hashes.push(hash);
        }
    }
}

pub enum TripleLayerQuadAllocator<'a> {
    Gpu(BorrowedLayers),
    Heap(&'a mut HeapQuadAllocator),
}

impl<'a> TripleLayerQuadAllocatorTrait for TripleLayerQuadAllocator<'a> {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        match self {
            Self::Gpu(b) => b.allocate(layer_num),
            Self::Heap(h) => h.allocate(layer_num),
        }
    }

    fn extend_with(&mut self, layer_num: usize, vertices: &[Vertex]) {
        match self {
            Self::Gpu(b) => b.extend_with(layer_num, vertices),
            Self::Heap(h) => h.extend_with(layer_num, vertices),
        }
    }
}

impl<'a> TripleLayerQuadAllocator<'a> {
    /// LBTerm: quads copied in whole, all at once, their hashes made already
    /// (`HeapQuadAllocator::apply_to`, render/kept_quads.rs); into another heap allocator, which
    /// makes its own from their vertices
    pub fn lbterm_extend_all_hashed(&mut self, layer_num: usize, vertices: &[Vertex], hashes: &[u64]) {
        match self {
            Self::Gpu(b) => b.layers[layer_num].lbterm_extend_all_hashed(vertices, hashes),
            Self::Heap(h) => h.extend_with(layer_num, vertices),
        }
    }

    /// LBTerm: quads laid out at row 0 copied in all at once `dy` pixels lower, their hashes
    /// moved with them (`HeapQuadAllocator::lbterm_apply_moved`: a pane's line)
    pub fn lbterm_extend_all_moved(&mut self, layer_num: usize, vertices: &[Vertex], hashes: &[u64], dy: f32) {
        match self {
            Self::Gpu(b) => b.layers[layer_num].lbterm_extend_all_moved(vertices, hashes, dy),
            Self::Heap(h) => h.lbterm_extend_moved(layer_num, vertices, hashes, dy),
        }
    }

    /// LBTerm: the quads allocated so far in each sub-layer, so the paint
    /// pass can mark which visual they belong to (lbterm/COMPOSITOR.md).
    pub fn quad_counts(&self) -> [usize; 3] {
        match self {
            Self::Gpu(b) => b.quad_counts(),
            Self::Heap(h) => h.layers.each_ref().map(|(_, hashes)| hashes.len()),
        }
    }
}

/// LBTerm: a quad's hash covers every byte of its vertices. The GPU's copy of a layer is written
/// only with the quads whose hashes changed (termwindow/gpu_work.rs, `WebGpuVertexBuffer::upload`),
/// so a setter that wrote a field without folding it in would leave the old vertices on screen for
/// good (lbterm/PITFALLS.md, "Caches and memory"). Run by `lbterm.ps1 test -Package wezterm-gui`,
/// which the check doesn't; perf/paint-cost.ps1 holds the same end to end (LBTERM_CHECK_UPLOADS)
#[cfg(test)]
mod lbterm_hash_tests {
    use super::*;

    type Set = fn(&mut QuadImpl, f32);

    fn c(x: f32) -> LinearRgba {
        LinearRgba::with_components(x, 0.25, 0.5, 0.75)
    }

    /// Every way a quad's vertices are written, each given one number that changes what it writes
    fn setters() -> Vec<(&'static str, Set)> {
        vec![
            ("set_texture_discrete", |q, x| q.set_texture_discrete(x, 0.5, 0.25, 0.75)),
            ("set_texture_discrete y", |q, x| q.set_texture_discrete(0.1, 0.5, 0.25, x)),
            ("set_has_color_impl", |q, x| q.set_has_color_impl(x)),
            ("set_fg_color", |q, x| q.set_fg_color(LinearRgba::with_components(0.1, x, 0.3, 1.0))),
            ("set_alt_color_and_mix_value", |q, x| {
                q.set_alt_color_and_mix_value(LinearRgba::with_components(0.1, 0.2, x, 1.0), 0.5)
            }),
            ("set_alt_color_and_mix_value mix", |q, x| {
                q.set_alt_color_and_mix_value(LinearRgba::with_components(0.1, 0.2, 0.3, 1.0), x)
            }),
            ("set_hsv", |q, x| {
                q.set_hsv(Some(HsbTransform {
                    hue: 1.0,
                    saturation: x,
                    brightness: 1.0,
                }))
            }),
            ("set_position", |q, x| q.set_position(x, 2.0, 30.0, 40.0)),
            ("set_position bottom", |q, x| q.set_position(1.0, 2.0, 30.0, x)),
            ("set_corners", |q, x| {
                q.set_corners([[0.0, 0.0], [x, 0.0], [0.0, 1.0], [1.0, 1.0]], [c(0.1), c(0.2), c(0.3), c(0.4)])
            }),
            ("set_corners colour", |q, x| {
                q.set_corners([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]], [c(0.1), c(0.2), c(x), c(0.4)])
            }),
            ("set_fg_gradient", |q, x| q.set_fg_gradient(c(0.1), c(x))),
        ]
    }

    /// The vertices and hash a mapped quad has after `set` with `x`, on a quad written with the
    /// same few calls before it, as a paint writes one, from nothing as it's allocated
    fn mapped(set: Set, x: f32) -> ([Vertex; VERTICES_PER_CELL], u64) {
        let mut vert: [Vertex; VERTICES_PER_CELL] = Default::default();
        let mut hash = 0x6c62_7465_726d_0001;
        let mut quad = QuadImpl::Vert(Quad {
            vert: &mut vert,
            hash: &mut hash,
        });
        quad.set_position(1.0, 2.0, 30.0, 40.0);
        quad.set_fg_color(LinearRgba::with_components(0.9, 0.8, 0.7, 1.0));
        set(&mut quad, x);
        (vert, hash)
    }

    /// The same written into a heap allocator, as a pane's line is laid out for the line quad cache
    fn heap(set: Set, x: f32) -> ([Vertex; VERTICES_PER_CELL], u64) {
        let mut h = HeapQuadAllocator::default();
        {
            let mut quad = h.allocate(1).unwrap();
            quad.set_position(1.0, 2.0, 30.0, 40.0);
            quad.set_fg_color(LinearRgba::with_components(0.9, 0.8, 0.7, 1.0));
            set(&mut quad, x);
        }
        let [_, (vertices, hashes), _] = h.lbterm_into_vertices();
        // Copied into an array without `try_into`, which this crate's edition (2018) has no
        // prelude import of (lbterm/PITFALLS.md, 130)
        let mut v = [Vertex::default(); VERTICES_PER_CELL];
        v.copy_from_slice(&vertices);
        (v, hashes[0])
    }

    fn bytes(v: &[Vertex]) -> &[u8] {
        bytemuck::cast_slice(v)
    }

    #[test]
    fn every_write_to_a_quad_changes_its_hash() {
        for (name, set) in setters() {
            let makers: [fn(Set, f32) -> ([Vertex; VERTICES_PER_CELL], u64); 2] = [mapped, heap];
            for make in makers {
                for (a, b) in [(0.25f32, 0.5f32), (0.0, -0.0), (1.0, f32::from_bits(1.0f32.to_bits() + 1))] {
                    let (va, ha) = make(set, a);
                    let (vb, hb) = make(set, b);
                    if bytes(&va) != bytes(&vb) {
                        assert_ne!(ha, hb, "{name} with {a} and {b}: other vertices, the same hash");
                    }
                }
                // The same calls with the same values: the same hash, so nothing is written again
                let (va, ha) = make(set, 0.25);
                let (vb, hb) = make(set, 0.25);
                assert_eq!(bytes(&va), bytes(&vb), "{name}");
                assert_eq!(ha, hb, "{name}: the same calls, another hash");
            }
            // Each changes what a mapped quad's vertices are for at least one pair, or the test
            // above judged nothing for it
            assert_ne!(bytes(&mapped(set, 0.25).0), bytes(&mapped(set, 0.5).0), "{name} wrote nothing");
        }
    }

    #[test]
    fn a_quad_drawn_into_a_heap_is_the_quad_a_layer_would_have_been_given() {
        for (name, set) in setters() {
            // A quad as MappedQuads::allocate starts one: from nothing, its hash 0, no colour flag
            let mut vert: [Vertex; VERTICES_PER_CELL] = Default::default();
            let mut hash = 0;
            {
                let mut quad = QuadImpl::Vert(Quad {
                    vert: &mut vert,
                    hash: &mut hash,
                });
                quad.set_has_color(false);
                quad.set_position(1.0, 2.0, 30.0, 40.0);
                quad.set_fg_color(LinearRgba::with_components(0.9, 0.8, 0.7, 1.0));
                set(&mut quad, 0.25);
            }
            let (v, h) = heap(set, 0.25);
            assert_eq!(bytes(&v), bytes(&vert), "{name}: other vertices");
            assert_eq!(h, hash, "{name}: another hash");
        }
    }

    /// A line of two quads laid out at row 0 in a heap, the way render/pane.rs lays one out
    fn line() -> HeapQuadAllocator {
        let mut h = HeapQuadAllocator::default();
        for (layer, x) in [(0, 0.0f32), (1, 9.0)] {
            let mut quad = h.allocate(layer).unwrap();
            quad.set_position(-960.0 + x, -540.0, -951.0 + x, -520.0);
            quad.set_fg_color(LinearRgba::with_components(0.9, 0.8, 0.7, 1.0));
            quad.set_texture_discrete(0.1, 0.2, 0.3, 0.4);
        }
        h
    }

    fn moved(line: &HeapQuadAllocator, dy: f32) -> [(Vec<Vertex>, Vec<u64>); 3] {
        let mut into = HeapQuadAllocator::default();
        line.lbterm_apply_moved(&mut TripleLayerQuadAllocator::Heap(&mut into), dy).unwrap();
        into.lbterm_into_vertices()
    }

    #[test]
    fn a_line_copied_to_its_row_is_moved_down_by_it_and_its_hashes_with_it() {
        let line = line();
        let laid = line.clone().lbterm_into_vertices();
        let at = moved(&line, 190.0);
        for layer in 0..3 {
            let (vertices, hashes) = &at[layer];
            assert_eq!(vertices.len(), laid[layer].0.len());
            for (to, was) in vertices.iter().zip(&laid[layer].0) {
                assert_eq!(to.position, [was.position[0], was.position[1] + 190.0]);
                let (mut to, mut was) = (*to, *was);
                to.position = [0.0; 2];
                was.position = [0.0; 2];
                assert_eq!(bytes(&[to]), bytes(&[was]), "nothing but the place changes");
            }
            for (h, k) in hashes.iter().zip(&laid[layer].1) {
                assert_eq!(*h, lbterm_motion::content::moved(*k, 190.0));
            }
        }
        // At the same row again: the same vertices and hashes, nothing to write to the GPU
        let again = moved(&line, 190.0);
        for layer in 0..3 {
            assert_eq!(bytes(&again[layer].0), bytes(&at[layer].0));
            assert_eq!(again[layer].1, at[layer].1);
        }
        // A row up: every hash another, so the pane is drawn again
        let up = moved(&line, 170.0);
        for layer in 0..3 {
            for (a, b) in up[layer].1.iter().zip(&at[layer].1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn a_kept_line_is_the_size_it_holds_and_the_layout_keeps_its_room() {
        let mut scratch = HeapQuadAllocator::default();
        for _ in 0..37 {
            scratch.allocate(1).unwrap().set_position(0.0, 0.0, 1.0, 1.0);
        }
        let room = scratch.layers[1].0.capacity();
        let kept = scratch.lbterm_exact();
        assert_eq!(kept.lbterm_quads(), 37);
        assert_eq!(kept.layers[1].0.capacity(), 37 * VERTICES_PER_CELL);
        assert_eq!(kept.layers[1].1.capacity(), 37);
        assert_eq!(scratch.lbterm_quads(), 0, "emptied for the next line");
        assert_eq!(scratch.layers[1].0.capacity(), room, "with its room kept");
    }

    #[test]
    fn quads_copied_in_whole_hash_their_bytes() {
        let (v, _) = mapped(setters()[0].1, 0.25);
        let mut one = 0;
        fold_vertices(&mut one, &v);
        for i in 0..std::mem::size_of_val(&v) / 4 {
            let mut other = v;
            let words: &mut [u32] = bytemuck::cast_slice_mut(&mut other);
            words[i] ^= 1;
            let mut two = 0;
            fold_vertices(&mut two, &other);
            assert_ne!(one, two, "word {i} of the quad's vertices");
        }
        // Into a heap they're hashed the same way
        let mut h = HeapQuadAllocator::default();
        h.extend_with(2, &v);
        assert_eq!(h.layers[2].1, vec![one]);
    }
}

#[cfg(test)]
#[test]
fn size() {
    // LBTerm: what a quad is kept as in the line quad cache and copied into a layer as, besides its
    // 8-byte hash (upstream kept a boxed quad of 84 bytes, 104 with LBTerm's corners and gradient)
    assert_eq!(std::mem::size_of::<Vertex>() * VERTICES_PER_CELL, 272);
}
