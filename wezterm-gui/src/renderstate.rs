use super::glyphcache::GlyphCache;
use super::quad::*;
use super::utilsprites::{RenderMetrics, UtilSprites};
use crate::termwindow::webgpu::{adapter_info_to_gpu_info, WebGpuState, WebGpuTexture};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::bitmaps::Texture2d;
use ::window::glium::backend::Context as GliumContext;
use ::window::glium::buffer::{BufferMutSlice, Mapping};
use ::window::glium::{
    CapabilitiesSource, IndexBuffer as GliumIndexBuffer, VertexBuffer as GliumVertexBuffer,
};
use ::window::*;
use anyhow::Context;
use std::cell::{Ref, RefCell, RefMut};
use std::convert::TryInto;
use std::rc::Rc;
use wezterm_font::FontConfiguration;
use wgpu::util::DeviceExt;

const INDICES_PER_CELL: usize = 6;

#[derive(Clone)]
pub enum RenderContext {
    Glium(Rc<GliumContext>),
    WebGpu(Rc<WebGpuState>),
}

pub enum RenderFrame<'a> {
    Glium(&'a mut glium::Frame),
    WebGpu,
}

impl RenderContext {
    pub fn allocate_index_buffer(&self, indices: &[u32]) -> anyhow::Result<IndexBuffer> {
        match self {
            Self::Glium(context) => Ok(IndexBuffer::Glium(GliumIndexBuffer::new(
                context,
                glium::index::PrimitiveType::TrianglesList,
                indices,
            )?)),
            Self::WebGpu(state) => Ok(IndexBuffer::WebGpu(WebGpuIndexBuffer::new(indices, state))),
        }
    }

    pub fn allocate_vertex_buffer_initializer(&self, num_quads: usize) -> Vec<Vertex> {
        match self {
            Self::Glium(_) => {
                vec![Vertex::default(); num_quads * VERTICES_PER_CELL]
            }
            Self::WebGpu(_) => vec![],
        }
    }

    pub fn allocate_vertex_buffer(
        &self,
        num_quads: usize,
        initializer: &[Vertex],
    ) -> anyhow::Result<VertexBuffer> {
        match self {
            Self::Glium(context) => Ok(VertexBuffer::Glium(GliumVertexBuffer::dynamic(
                context,
                initializer,
            )?)),
            Self::WebGpu(state) => Ok(VertexBuffer::WebGpu(WebGpuVertexBuffer::new(
                num_quads * VERTICES_PER_CELL,
                state,
            ))),
        }
    }

    pub fn allocate_texture_atlas(&self, size: usize) -> anyhow::Result<Rc<dyn Texture2d>> {
        match self {
            Self::Glium(context) => {
                let caps = context.get_capabilities();
                // You'd hope that allocating a texture would automatically
                // include this check, but it doesn't, and instead, the texture
                // silently fails to bind when attempting to render into it later.
                // So! We check and raise here for ourselves!
                let max_texture_size: usize = caps
                    .max_texture_size
                    .try_into()
                    .context("represent Capabilities.max_texture_size as usize")?;
                if size > max_texture_size {
                    anyhow::bail!(
                        "Cannot use a texture of size {} as it is larger \
                         than the max {} supported by your GPU",
                        size,
                        caps.max_texture_size
                    );
                }
                use crate::glium::texture::SrgbTexture2d;
                let surface: Rc<dyn Texture2d> = Rc::new(SrgbTexture2d::empty_with_format(
                    context,
                    glium::texture::SrgbFormat::U8U8U8U8,
                    glium::texture::MipmapsOption::NoMipmap,
                    size as u32,
                    size as u32,
                )?);
                Ok(surface)
            }
            Self::WebGpu(state) => {
                let texture: Rc<dyn Texture2d> =
                    Rc::new(WebGpuTexture::new(size as u32, size as u32, state)?);
                Ok(texture)
            }
        }
    }

    pub fn renderer_info(&self) -> String {
        match self {
            Self::Glium(ctx) => format!(
                "OpenGL: {} {}",
                ctx.get_opengl_renderer_string(),
                ctx.get_opengl_version_string()
            ),
            Self::WebGpu(state) => {
                let info = adapter_info_to_gpu_info(state.adapter_info.clone());
                format!("WebGPU: {}", info.to_string())
            }
        }
    }
}

pub enum IndexBuffer {
    Glium(GliumIndexBuffer<u32>),
    WebGpu(WebGpuIndexBuffer),
}

impl IndexBuffer {
    pub fn glium(&self) -> &GliumIndexBuffer<u32> {
        match self {
            Self::Glium(g) => g,
            _ => unreachable!(),
        }
    }
    pub fn webgpu(&self) -> &WebGpuIndexBuffer {
        match self {
            Self::WebGpu(g) => g,
            _ => unreachable!(),
        }
    }
}

pub enum VertexBuffer {
    Glium(GliumVertexBuffer<Vertex>),
    WebGpu(WebGpuVertexBuffer),
}

impl VertexBuffer {
    pub fn glium(&self) -> &GliumVertexBuffer<Vertex> {
        match self {
            Self::Glium(g) => g,
            _ => unreachable!(),
        }
    }
    pub fn webgpu(&self) -> &WebGpuVertexBuffer {
        match self {
            Self::WebGpu(g) => g,
            _ => unreachable!(),
        }
    }
    pub fn webgpu_mut(&mut self) -> &mut WebGpuVertexBuffer {
        match self {
            Self::WebGpu(g) => g,
            _ => unreachable!(),
        }
    }
}

enum MappedVertexBuffer {
    Glium(GliumMappedVertexBuffer),
    WebGpu(WebGpuMappedVertexBuffer),
}

impl MappedVertexBuffer {
    fn slice_mut(&mut self, range: std::ops::Range<usize>) -> &mut [Vertex] {
        match self {
            Self::Glium(g) => &mut g.mapping[range],
            Self::WebGpu(g) => &mut g.mapping[range],
        }
    }
}

pub struct MappedQuads<'a> {
    mapping: MappedVertexBuffer,
    next: RefMut<'a, usize>,
    capacity: usize,
    /// LBTerm: each quad's hash (quad.rs, `fold`)
    hashes: RefMut<'a, Vec<u64>>,
}

/// LBTerm: the paint pass writes a WebGpu layer's quads here, on this side
/// (`WebGpuVertexBuffer::upload`)
pub struct WebGpuMappedVertexBuffer {
    mapping: &'static mut [Vertex],
    // Owner of what `mapping` points into, kept borrowed while it's written
    _owner: RefMut<'static, VertexBuffer>,
}

/// LBTerm: a layer's quads, written on this side by the paint pass, and the GPU's copy of them,
/// written only in part and only when it's drawn from (termwindow/gpu_work.rs, `upload`)
pub struct WebGpuVertexBuffer {
    pub(crate) buf: wgpu::Buffer,
    pub(crate) num_vertices: usize,
    /// LBTerm: what the paint pass writes, what `buf` holds and how it's written from this
    /// (termwindow/gpu_work.rs, `upload`)
    pub(crate) lbterm: crate::termwindow::gpu_work::Uploads,
    pub(crate) state: Rc<WebGpuState>,
}

impl std::ops::Deref for WebGpuVertexBuffer {
    type Target = wgpu::Buffer;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl WebGpuVertexBuffer {
    pub fn new(num_vertices: usize, state: &Rc<WebGpuState>) -> Self {
        state.lbterm_gpu.made(1);
        Self {
            buf: state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Vertex Buffer"),
                size: (num_vertices * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            num_vertices,
            lbterm: crate::termwindow::gpu_work::Uploads::new(num_vertices),
            state: Rc::clone(state),
        }
    }
}

pub struct WebGpuIndexBuffer {
    buf: wgpu::Buffer,
}

impl std::ops::Deref for WebGpuIndexBuffer {
    type Target = wgpu::Buffer;
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl WebGpuIndexBuffer {
    pub fn new(indices: &[u32], state: &WebGpuState) -> Self {
        state.lbterm_gpu.made(1);
        Self {
            buf: state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Index Buffer"),
                    usage: wgpu::BufferUsages::INDEX,
                    contents: bytemuck::cast_slice(indices),
                }),
        }
    }
}

/// This is a self-referential struct, but since those are not possible
/// to create safely in unstable rust, we transmute the lifetimes away
/// to static and store the owner (RefMut) and the derived Mapping object
/// in this struct
pub struct GliumMappedVertexBuffer {
    mapping: Mapping<'static, [Vertex]>,
    // Drop the owner after the mapping
    _owner: RefMut<'static, VertexBuffer>,
}

impl<'a> QuadAllocator for MappedQuads<'a> {
    fn allocate<'b>(&'b mut self) -> anyhow::Result<QuadImpl<'b>> {
        let idx = *self.next;
        *self.next += 1;
        let idx = if idx >= self.capacity {
            // We don't have enough quads, so we'll keep re-using
            // the first quad until we reach the end of the render
            // pass, at which point we'll detect this condition
            // and re-allocate the quads.
            0
        } else {
            idx
        };

        let hash = &mut self.hashes[idx];
        *hash = 0;
        let idx = idx * VERTICES_PER_CELL;
        let mut quad = Quad {
            vert: self.mapping.slice_mut(idx..idx + VERTICES_PER_CELL),
            hash,
        };
        // LBTerm: from nothing, as a buffer made for the frame was, so what a quad isn't given
        // is the same every frame, not what the last quad here had (its hash folds only what
        // it's given: quad.rs). A layer's buffer is written into frame after frame since
        // WebGpuVertexBuffer::upload
        quad.vert.fill(Vertex::default());

        quad.set_has_color(false);

        Ok(QuadImpl::Vert(quad))
    }

    fn extend_with(&mut self, vertices: &[Vertex]) {
        let idx = *self.next;
        let len = vertices.len();

        // idx and next are number of quads, so divide by number of vertices
        *self.next += len / VERTICES_PER_CELL;
        // Only copy in if there is enough room.
        // We'll detect the out of space condition at the end of
        // the render pass.
        let first = idx;
        let idx = idx * VERTICES_PER_CELL;
        let capacity = self.capacity * VERTICES_PER_CELL;
        if idx + len <= capacity {
            self.mapping
                .slice_mut(idx..idx + len)
                .copy_from_slice(vertices);
            // LBTerm: each quad's hash, from its vertices (quad.rs, `fold_vertices`)
            for (quad, verts) in vertices.chunks_exact(VERTICES_PER_CELL).enumerate() {
                let hash = &mut self.hashes[first + quad];
                *hash = 0;
                crate::quad::fold_vertices(hash, verts);
            }
        }
    }
}

impl<'a> MappedQuads<'a> {
    /// LBTerm: `extend_with` for quads whose hashes are made already, `vertices` four to each of
    /// `hashes`, so their vertices aren't folded again every frame: one copy of them all
    /// (quad.rs, `HeapQuadAllocator::apply_to`; render/kept_quads.rs)
    pub fn lbterm_extend_all_hashed(&mut self, vertices: &[Vertex], hashes: &[u64]) {
        let idx = *self.next;
        let n = hashes.len().min(vertices.len() / VERTICES_PER_CELL);
        *self.next += n;
        // As extend_with: what doesn't fit isn't copied in, and the pass is painted again with more
        let fit = n.min(self.capacity.saturating_sub(idx));
        if fit > 0 {
            let at = idx * VERTICES_PER_CELL;
            self.mapping
                .slice_mut(at..at + fit * VERTICES_PER_CELL)
                .copy_from_slice(&vertices[..fit * VERTICES_PER_CELL]);
            self.hashes[idx..idx + fit].copy_from_slice(&hashes[..fit]);
        }
    }

    /// LBTerm: `lbterm_extend_all_hashed` for a pane's line, laid out at row 0 and kept so (the
    /// line quad cache, render/pane.rs): copied in `dy` pixels lower, at its row, each quad's
    /// hash moved with it (`lbterm_motion::content::place`)
    pub fn lbterm_extend_all_moved(&mut self, vertices: &[Vertex], hashes: &[u64], dy: f32) {
        let idx = *self.next;
        let n = hashes.len().min(vertices.len() / VERTICES_PER_CELL);
        *self.next += n;
        // As extend_with: what doesn't fit isn't copied in, and the pass is painted again with more
        let fit = n.min(self.capacity.saturating_sub(idx));
        if fit > 0 {
            let at = idx * VERTICES_PER_CELL;
            lbterm_motion::content::place(
                self.mapping.slice_mut(at..at + fit * VERTICES_PER_CELL),
                &mut self.hashes[idx..idx + fit],
                &vertices[..fit * VERTICES_PER_CELL],
                &hashes[..fit],
                VERTICES_PER_CELL,
                dy,
                crate::quad::move_down,
            );
        }
    }
}

pub struct TripleVertexBuffer {
    pub index: RefCell<usize>,
    /// LBTerm: three with Glium, one with WebGpu (`RenderContext::vertex_buffers_per_layer`)
    pub bufs: RefCell<Vec<VertexBuffer>>,
    pub indices: IndexBuffer,
    pub capacity: usize,
    pub next_quad: RefCell<usize>,
    /// LBTerm: what each quad allocated this frame was given, folded into one number as it was
    /// written (quad.rs, `fold`), so the compositor can tell which visuals' quads are what they
    /// were last frame (render/composited.rs)
    pub hashes: RefCell<Vec<u64>>,
}

/// A trait to avoid broadly-scoped transmutes; we only want to
/// transmute to extend a lifetime to static, and not to change
/// the underlying type.
/// These ExtendStatic trait impls constrain the transmutes in that way,
/// so that the type checker can still catch issues.
unsafe trait ExtendStatic {
    type T;
    unsafe fn extend_lifetime(self) -> Self::T;
}

unsafe impl<'a, T: 'static> ExtendStatic for Ref<'a, T> {
    type T = Ref<'static, T>;
    unsafe fn extend_lifetime(self) -> Self::T {
        std::mem::transmute(self)
    }
}

unsafe impl<'a, T: 'static> ExtendStatic for RefMut<'a, T> {
    type T = RefMut<'static, T>;
    unsafe fn extend_lifetime(self) -> Self::T {
        std::mem::transmute(self)
    }
}

unsafe impl<'a> ExtendStatic for wgpu::BufferSlice<'a> {
    type T = wgpu::BufferSlice<'static>;
    unsafe fn extend_lifetime(self) -> Self::T {
        std::mem::transmute(self)
    }
}

unsafe impl<'a> ExtendStatic for MappedQuads<'a> {
    type T = MappedQuads<'static>;
    unsafe fn extend_lifetime(self) -> Self::T {
        std::mem::transmute(self)
    }
}

unsafe impl<'a, T: ?Sized + ::window::glium::buffer::Content + 'static> ExtendStatic
    for BufferMutSlice<'a, T>
{
    type T = BufferMutSlice<'static, T>;
    unsafe fn extend_lifetime(self) -> Self::T {
        std::mem::transmute(self)
    }
}

impl TripleVertexBuffer {
    pub fn clear_quad_allocation(&self) {
        *self.next_quad.borrow_mut() = 0;
    }

    pub fn need_more_quads(&self) -> Option<usize> {
        let next = *self.next_quad.borrow();
        if next > self.capacity {
            Some(next)
        } else {
            None
        }
    }

    pub fn vertex_index_count(&self) -> (usize, usize) {
        let num_quads = *self.next_quad.borrow();
        (num_quads * VERTICES_PER_CELL, num_quads * INDICES_PER_CELL)
    }

    pub fn map(&self) -> MappedQuads<'_> {
        let mut bufs = self.current_vb_mut();

        // To map the vertex buffer, we need to hold a mutable reference to
        // the buffer and hold the mapping object alive for the duration
        // of the access.  Rust doesn't allow us to create a struct that
        // holds both of those things, because one references the other
        // and it doesn't permit self-referential structs.
        // We use the very blunt instrument "transmute" to force Rust to
        // treat the lifetimes of both of these things as static, which
        // we can then store in the same struct.
        // This is "safe" because we carry them around together and ensure
        // that the owner is dropped after the derived data.
        let mapping = match &mut *bufs {
            VertexBuffer::Glium(vb) => {
                let buf_slice = unsafe {
                    vb.slice_mut(..)
                        .expect("to map vertex buffer")
                        .extend_lifetime()
                };
                let mapping = buf_slice.map();

                MappedVertexBuffer::Glium(GliumMappedVertexBuffer {
                    _owner: bufs,
                    mapping,
                })
            }
            // LBTerm: the vertices on this side, which the owner keeps borrowed while they're
            // written; the GPU's copy is written from them when it's drawn from
            VertexBuffer::WebGpu(vb) => {
                let vertices = &mut vb.lbterm.vertices;
                let mapping = unsafe { std::slice::from_raw_parts_mut(vertices.as_mut_ptr(), vertices.len()) };
                MappedVertexBuffer::WebGpu(WebGpuMappedVertexBuffer {
                    mapping,
                    _owner: bufs,
                })
            }
        };

        MappedQuads {
            mapping,
            next: self.next_quad.borrow_mut(),
            capacity: self.capacity,
            hashes: self.hashes.borrow_mut(),
        }
    }

    pub fn current_vb_mut(&self) -> RefMut<'static, VertexBuffer> {
        let index = *self.index.borrow();
        let bufs = self.bufs.borrow_mut();
        unsafe { RefMut::map(bufs, |bufs| &mut bufs[index]).extend_lifetime() }
    }

    pub fn next_index(&self) {
        let mut index = self.index.borrow_mut();
        *index += 1;
        // LBTerm: WebGpu keeps one (RenderContext::vertex_buffers_per_layer)
        if *index >= self.bufs.borrow().len() {
            *index = 0;
        }
    }
}

pub struct RenderLayer {
    pub vb: RefCell<[TripleVertexBuffer; 3]>,
    pub(crate) context: RenderContext,
    zindex: i8,
    /// LBTerm: what its owner last made its quads from, and how many each of its three held then:
    /// kept while nothing writes over them (render/kept.rs, `lbterm_keep`)
    pub(crate) lbterm_kept: std::cell::Cell<Option<(u64, [usize; 3])>>,
}

impl RenderLayer {
    pub fn new(context: &RenderContext, num_quads: usize, zindex: i8) -> anyhow::Result<Self> {
        let vb = [
            Self::compute_vertices(context, 32)?,
            Self::compute_vertices(context, num_quads)?,
            Self::compute_vertices(context, 32)?,
        ];

        Ok(Self {
            context: context.clone(),
            vb: RefCell::new(vb),
            zindex,
            lbterm_kept: Default::default(),
        })
    }

    pub fn clear_quad_allocation(&self) {
        for vb in self.vb.borrow().iter() {
            vb.clear_quad_allocation();
        }
    }

    /// LBTerm: the compositor draws layers below 0 under the panes and
    /// layers above 0 over them.
    pub fn zindex(&self) -> i8 {
        self.zindex
    }

    pub fn quad_allocator(&self) -> TripleLayerQuadAllocator<'_> {
        // We're creating a self-referential struct here to manage the lifetimes
        // of these related items.  The transmutes are safe because we're only
        // transmuting the lifetimes (not the types), and we're keeping hold
        // of the owner in the returned struct.
        unsafe {
            let vbs = self.vb.borrow().extend_lifetime();
            let layer0 = vbs[0].map().extend_lifetime();
            let layer1 = vbs[1].map().extend_lifetime();
            let layer2 = vbs[2].map().extend_lifetime();
            TripleLayerQuadAllocator::Gpu(BorrowedLayers {
                layers: [layer0, layer1, layer2],
                _owner: vbs,
            })
        }
    }

    pub fn need_more_quads(&self, vb_idx: usize) -> Option<usize> {
        self.vb.borrow()[vb_idx].need_more_quads()
    }

    pub fn reallocate_quads(&self, idx: usize, num_quads: usize) -> anyhow::Result<()> {
        let vb = Self::compute_vertices(&self.context, num_quads)?;
        self.vb.borrow_mut()[idx] = vb;
        // LBTerm: what it kept went with its buffer
        self.lbterm_kept.set(None);
        Ok(())
    }

    /// Compute a vertex buffer to hold the quads that comprise the visible
    /// portion of the screen.   We recreate this when the screen is resized.
    /// The idea is that we want to minimize any heavy lifting and computation
    /// and instead just poke some attributes into the offset that corresponds
    /// to a changed cell when we need to repaint the screen, and then just
    /// let the GPU figure out the rest.
    fn compute_vertices(
        context: &RenderContext,
        num_quads: usize,
    ) -> anyhow::Result<TripleVertexBuffer> {
        let verts = context.allocate_vertex_buffer_initializer(num_quads);
        log::trace!(
            "compute_vertices num_quads={}, allocated {} bytes",
            num_quads,
            verts.len() * std::mem::size_of::<Vertex>()
        );
        let mut indices = vec![];
        indices.reserve(num_quads * INDICES_PER_CELL);

        for q in 0..num_quads {
            let idx = (q * VERTICES_PER_CELL) as u32;

            // Emit two triangles to form the glyph quad
            indices.push(idx + V_TOP_LEFT as u32);
            indices.push(idx + V_TOP_RIGHT as u32);
            indices.push(idx + V_BOT_LEFT as u32);

            indices.push(idx + V_TOP_RIGHT as u32);
            indices.push(idx + V_BOT_LEFT as u32);
            indices.push(idx + V_BOT_RIGHT as u32);
        }

        let buffer = TripleVertexBuffer {
            index: RefCell::new(0),
            bufs: RefCell::new(
                (0..context.vertex_buffers_per_layer())
                    .map(|_| context.allocate_vertex_buffer(num_quads, &verts))
                    .collect::<anyhow::Result<_>>()?,
            ),
            capacity: num_quads,
            indices: context.allocate_index_buffer(&indices)?,
            next_quad: RefCell::new(0),
            hashes: RefCell::new(vec![0; num_quads.max(1)]),
        };

        Ok(buffer)
    }
}

pub struct BorrowedLayers {
    pub layers: [MappedQuads<'static>; 3],

    // layers references _owner, so it must be dropped after layers.
    _owner: Ref<'static, [TripleVertexBuffer; 3]>,
}

impl TripleLayerQuadAllocatorTrait for BorrowedLayers {
    fn allocate(&mut self, layer_num: usize) -> anyhow::Result<QuadImpl<'_>> {
        self.layers[layer_num].allocate()
    }

    fn extend_with(&mut self, layer_num: usize, vertices: &[Vertex]) {
        self.layers[layer_num].extend_with(vertices)
    }
}

impl BorrowedLayers {
    /// LBTerm: see `TripleLayerQuadAllocator::quad_counts`.
    pub fn quad_counts(&self) -> [usize; 3] {
        [0, 1, 2].map(|i| *self.layers[i].next)
    }
}

pub struct RenderState {
    pub context: RenderContext,
    pub glyph_cache: RefCell<GlyphCache>,
    pub util_sprites: UtilSprites,
    pub glyph_prog: Option<glium::Program>,
    pub layers: RefCell<Vec<Rc<RenderLayer>>>,
    /// LBTerm: frames painted since the glyph atlas was made (render/paint.rs)
    pub lbterm_atlas_frames: usize,
}

impl RenderState {
    pub fn new(
        context: RenderContext,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        mut atlas_size: usize,
    ) -> anyhow::Result<Self> {
        loop {
            let glyph_cache = RefCell::new(GlyphCache::new_gl(&context, fonts, atlas_size)?);
            let result = UtilSprites::new(&mut *glyph_cache.borrow_mut(), metrics);
            match result {
                Ok(util_sprites) => {
                    let glyph_prog = match &context {
                        RenderContext::Glium(context) => {
                            Some(Self::compile_prog(&context, Self::glyph_shader)?)
                        }
                        RenderContext::WebGpu(_) => None,
                    };

                    let main_layer = Rc::new(RenderLayer::new(&context, 1024, 0)?);

                    return Ok(Self {
                        context,
                        glyph_cache,
                        util_sprites,
                        glyph_prog,
                        layers: RefCell::new(vec![main_layer]),
                        lbterm_atlas_frames: 0,
                    });
                }
                Err(OutOfTextureSpace {
                    size: Some(size), ..
                }) => {
                    atlas_size = size;
                }
                Err(OutOfTextureSpace { size: None, .. }) => {
                    anyhow::bail!("requested texture size is impossible!?")
                }
            };
        }
    }

    pub fn layer_for_zindex(&self, zindex: i8) -> anyhow::Result<Rc<RenderLayer>> {
        if let Some(layer) = self
            .layers
            .borrow()
            .iter()
            .find(|l| l.zindex == zindex)
            .map(Rc::clone)
        {
            return Ok(layer);
        }

        let layer = Rc::new(RenderLayer::new(&self.context, 128, zindex)?);
        let mut layers = self.layers.borrow_mut();
        layers.push(Rc::clone(&layer));

        // Keep the layers sorted by zindex so that they are rendered in
        // the correct order when the layers array is iterated.
        layers.sort_by(|a, b| a.zindex.cmp(&b.zindex));

        Ok(layer)
    }

    /// Returns true if any of the layers needed more quads to be allocated,
    /// and if we successfully allocated them.
    /// Returns false if the quads were sufficient.
    /// Returns Err if we needed to allocate but failed.
    pub fn allocated_more_quads(&mut self) -> anyhow::Result<bool> {
        let mut allocated = false;

        for layer in self.layers.borrow().iter() {
            for vb_idx in 0..3 {
                if let Some(need_quads) = layer.need_more_quads(vb_idx) {
                    // Round up to next multiple of 128 that is >=
                    // the number of needed quads for this frame
                    let num_quads = (need_quads + 127) & !127;
                    let num_quads = layer.lbterm_grow_to(vb_idx, num_quads);
                    layer.reallocate_quads(vb_idx, num_quads).with_context(|| {
                        format!(
                            "Failed to allocate {} quads (needed {})",
                            num_quads, need_quads,
                        )
                    })?;
                    log::trace!("Allocated {} quads (needed {})", num_quads, need_quads);
                    allocated = true;
                }
            }
        }

        Ok(allocated)
    }

    fn compile_prog(
        context: &Rc<GliumContext>,
        fragment_shader: fn(&str) -> (String, String),
    ) -> anyhow::Result<glium::Program> {
        let mut errors = vec![];

        let caps = context.get_capabilities();
        log::trace!("Compiling shader. context.capabilities.srgb={}", caps.srgb);

        for version in &["330 core", "330", "320 es", "300 es"] {
            let (vertex_shader, fragment_shader) = fragment_shader(version);
            let source = glium::program::ProgramCreationInput::SourceCode {
                vertex_shader: &vertex_shader,
                fragment_shader: &fragment_shader,
                outputs_srgb: true,
                tessellation_control_shader: None,
                tessellation_evaluation_shader: None,
                transform_feedback_varyings: None,
                uses_point_size: false,
                geometry_shader: None,
            };
            match glium::Program::new(context, source) {
                Ok(prog) => {
                    return Ok(prog);
                }
                Err(err) => errors.push(format!("shader version: {}: {:#}", version, err)),
            };
        }

        anyhow::bail!("Failed to compile shaders: {}", errors.join("\n"))
    }

    fn glyph_shader(version: &str) -> (String, String) {
        (
            format!(
                "#version {}\n{}",
                version,
                include_str!("glyph-vertex.glsl")
            ),
            format!("#version {}\n{}", version, include_str!("glyph-frag.glsl")),
        )
    }

    pub fn config_changed(&mut self) {
        self.glyph_cache.borrow_mut().config_changed();
    }

    pub fn recreate_texture_atlas(
        &mut self,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        size: Option<usize>,
    ) -> anyhow::Result<()> {
        // We make a a couple of passes at resizing; if the user has selected a large
        // font size (or a large scaling factor) then the `size==None` case will not
        // be able to fit the initial utility glyphs and apply_scale_change won't
        // be able to deal with that error situation.  Rather than make every
        // caller know how to deal with OutOfTextureSpace we try to absorb
        // and accomodate that here.
        let mut size = size;
        let mut attempt = 10;
        loop {
            match self.recreate_texture_atlas_impl(fonts, metrics, size) {
                Ok(_) => return Ok(()),
                Err(err) => {
                    attempt -= 1;
                    if attempt == 0 {
                        return Err(err);
                    }

                    if let Some(&OutOfTextureSpace {
                        size: Some(needed_size),
                        ..
                    }) = err.downcast_ref::<OutOfTextureSpace>()
                    {
                        size.replace(needed_size);
                        continue;
                    }

                    return Err(err);
                }
            }
        }
    }

    fn recreate_texture_atlas_impl(
        &mut self,
        fonts: &Rc<FontConfiguration>,
        metrics: &RenderMetrics,
        size: Option<usize>,
    ) -> anyhow::Result<()> {
        let size = size.unwrap_or_else(|| self.glyph_cache.borrow().atlas.size());
        let mut new_glyph_cache = GlyphCache::new_gl(&self.context, fonts, size)?;
        self.util_sprites = UtilSprites::new(&mut new_glyph_cache, metrics)?;

        let mut glyph_cache = self.glyph_cache.borrow_mut();

        // Steal the decoded image cache; without this, any animating gifs
        // would reset back to frame 0 each time we filled the texture
        std::mem::swap(
            &mut glyph_cache.image_cache,
            &mut new_glyph_cache.image_cache,
        );

        *glyph_cache = new_glyph_cache;
        self.lbterm_atlas_frames = 0;
        Ok(())
    }
}
