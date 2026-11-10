//! LBTerm: what drawing a frame makes and writes on the GPU, and what it keeps from one frame to
//! the next (lbterm/COMPOSITOR.md, "What a frame costs").
//!
//! Upstream made everything a frame draws with afresh each frame: a vertex buffer for every
//! z-layer that held a quad, mapped as it was made (which wgpu backs with a second buffer it
//! copies the whole of across as it's unmapped), the glyph atlas's view and its two bind groups,
//! and a uniform buffer and its bind group for each pass. With the compositor that was every
//! visual's every frame, though most frames draw one or two of them. Now each z-layer keeps one
//! vertex buffer it writes into (`WebGpuVertexBuffer::upload`, here rather than in upstream's
//! renderstate.rs, which keeps only the fields and the calls), only when a visual drawn this frame
//! is drawn from it and what it holds isn't what the layer holds now; the atlas's bind groups are
//! kept while the atlas is the same; and each visual's uniforms are kept while its place, the
//! window's size and the text's colour transform are. What can be tested without a GPU (the
//! uniform's key, which quads are written, the counts) is lbterm_motion::content's.

use crate::quad::{Vertex, VERTICES_PER_CELL};
use crate::renderstate::{RenderContext, TripleVertexBuffer, WebGpuVertexBuffer};
use crate::termwindow::webgpu::{ShaderUniform, WebGpuState};
use lbterm_motion::content::{
    dirty_runs, fold, stale_quads, uniform_key, uniforms_at, uniforms_kept_since, UniformsAt, Work,
    WRITE_GAP, WRITE_RUNS,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use window::bitmaps::Texture2d;

/// A uniform not used for this many frames is let go
const UNIFORM_FRAMES: u64 = 600;

/// The most uniforms' bind groups a window keeps: config's `GPU_UNIFORMS_KEPT`, or in a test
/// window the number in `LBTERM_TEST_UNIFORMS_KEPT` (perf/gpu-memory.ps1 makes it small, to see
/// the oldest go and the window draw on). Read once
fn uniforms_kept() -> usize {
    static MOST: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MOST.get_or_init(|| match std::env::var("LBTERM_TEST_UNIFORMS_KEPT") {
        Ok(v) => v.parse().expect("LBTERM_TEST_UNIFORMS_KEPT is a number of bind groups"),
        Err(_) => config::lbterm::GPU_UNIFORMS_KEPT,
    })
}

/// Whether this window checks that every quad its uploads leave out holds what the GPU's copy
/// holds (LBTERM_CHECK_UPLOADS in a test window's environment, which perf/paint-cost.ps1 sets):
/// a shadow of each GPU copy kept and compared, which costs a copy of every write. Read once
pub(crate) fn checking() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LBTERM_CHECK_UPLOADS").is_some())
}

/// What a layer's buffer holds on this side, and what its GPU copy holds
pub struct Uploads {
    /// What the paint pass writes
    pub(crate) vertices: Vec<Vertex>,
    /// The key (`TripleVertexBuffer::lbterm_key`) of what the GPU copy holds: none until written
    holds: Option<u64>,
    /// Each quad's hash as the GPU copy holds it, as it was when it was written there
    held: Vec<u64>,
    /// What the GPU copy holds, while `checking`
    shadow: Option<Vec<Vertex>>,
}

impl Uploads {
    pub fn new(num_vertices: usize) -> Self {
        Self {
            vertices: vec![Vertex::default(); num_vertices],
            holds: None,
            held: vec![],
            shadow: checking().then(|| vec![Vertex::default(); num_vertices]),
        }
    }
}

impl WebGpuVertexBuffer {
    /// The GPU's copy of the first `vertices` written by the paint pass, each quad's hash in
    /// `hashes`: nothing written when `key` (what they all are: `TripleVertexBuffer::lbterm_key`)
    /// is what it holds already, else only the quads whose hashes differ from what it holds there
    /// (lbterm_motion::content::dirty_runs). Ordered on the queue, so frames already submitted
    /// draw what they were given
    pub fn upload(&mut self, vertices: usize, key: u64, hashes: &[u64]) -> &wgpu::Buffer {
        let quads = (vertices.min(self.num_vertices) / VERTICES_PER_CELL).min(hashes.len());
        let up = &mut self.lbterm;
        if quads == 0 {
            return &self.buf;
        }
        if let Some(shadow) = up.shadow.as_ref() {
            // Every quad left out of the writes below, its hash the same as what's held, must hold
            // what the GPU's copy does
            let held: &[u64] = if up.holds == Some(key) { &hashes[..quads] } else { &up.held };
            let (now, was): (&[u8], &[u8]) = (bytemuck::cast_slice(&up.vertices), bytemuck::cast_slice(shadow));
            let per_quad = VERTICES_PER_CELL * std::mem::size_of::<Vertex>();
            let stale = stale_quads(&hashes[..quads], held, now, was, per_quad, quads);
            self.state.lbterm_gpu.stale(stale);
        }
        if up.holds != Some(key) {
            for run in dirty_runs(&hashes[..quads], &up.held, WRITE_GAP, WRITE_RUNS) {
                let range = run.start * VERTICES_PER_CELL..run.end * VERTICES_PER_CELL;
                let bytes: &[u8] = bytemuck::cast_slice(&up.vertices[range.clone()]);
                let at = (range.start * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress;
                self.state.queue.write_buffer(&self.buf, at, bytes);
                self.state.lbterm_gpu.uploaded(bytes.len());
                if let Some(shadow) = up.shadow.as_mut() {
                    shadow[range.clone()].copy_from_slice(&up.vertices[range]);
                }
            }
            up.held.clear();
            up.held.extend_from_slice(&hashes[..quads]);
            up.holds = Some(key);
        }
        &self.buf
    }
}

impl TripleVertexBuffer {
    /// What this frame's quads are, as one number: how many, and each one's hash, folded as it was
    /// written (quad.rs). A GPU copy written from quads with the same key holds the same vertices
    /// (`WebGpuVertexBuffer::upload`)
    pub fn lbterm_key(&self) -> u64 {
        let quads = *self.next_quad.borrow();
        let hashes = self.hashes.borrow();
        let mut key = 0x6c62_7465_726d_0003;
        fold(&mut key, quads as u64);
        for &hash in hashes.iter().take(quads) {
            fold(&mut key, hash);
        }
        key
    }
}

impl crate::renderstate::RenderLayer {
    /// How many quads sub-layer `idx` is made again to hold when it needs `need` (a multiple of
    /// 128, as upstream rounds it): with WebGpu at least half as many again as it held
    /// (lbterm_motion::content::grow), since only what a frame uses is written to the GPU
    /// (`WebGpuVertexBuffer::upload`), where upstream's whole buffer was copied every frame; so a
    /// pane filling up, a line at a time, makes a layer again a few times rather than every 128
    /// quads, each time with the frame's pass painted twice and its buffers made again
    pub fn lbterm_grow_to(&self, idx: usize, need: usize) -> usize {
        match self.context {
            RenderContext::WebGpu(_) => lbterm_motion::content::grow(self.vb.borrow()[idx].capacity, need),
            RenderContext::Glium(_) => need,
        }
    }
}

impl RenderContext {
    /// How many vertex buffers a layer turns through. Glium's three, as upstream's; one with
    /// WebGpu, which writes its own into the GPU's copy only when that's out of date
    /// (`WebGpuVertexBuffer::upload`), where upstream made a new one every frame and turned
    /// through three besides
    pub(crate) fn vertex_buffers_per_layer(&self) -> usize {
        match self {
            Self::Glium(_) => 3,
            Self::WebGpu(_) => 1,
        }
    }
}

/// The glyph atlas's bind groups, kept with the atlas they were made for: holding its `Rc` keeps
/// it from being freed and another made at the same address
struct Atlas {
    texture: Rc<dyn Texture2d>,
    linear: Rc<wgpu::BindGroup>,
    nearest: Rc<wgpu::BindGroup>,
}

/// The parts of a uniform the shader reads, bit for bit (lbterm_motion::content::uniform_key)
type UniformKey = [u32; 19];

/// A uniform's bind group kept: the frame it was last drawn in, and the window's size and colour
/// transform it was last drawn under (lbterm_motion::content::uniforms_at)
struct Kept {
    group: Rc<wgpu::BindGroup>,
    used: u64,
    at: Option<[u32; 5]>,
}

#[derive(Default)]
pub struct GpuWork {
    work: Cell<Work>,
    atlas: RefCell<Option<Atlas>>,
    uniforms: RefCell<HashMap<UniformKey, Kept>>,
    /// The sizes and colour transforms whose uniforms are kept: now and the one before
    at: Cell<UniformsAt>,
    frame: Cell<u64>,
}

impl GpuWork {
    fn add(&self, f: impl FnOnce(&mut Work)) {
        let mut w = self.work.get();
        f(&mut w);
        self.work.set(w);
    }

    /// `n` buffers or bind groups made
    pub fn made(&self, n: usize) {
        self.add(|w| w.made(n));
    }

    /// A vertex buffer written, `bytes` of it
    pub fn uploaded(&self, bytes: usize) {
        self.add(|w| w.uploaded(bytes));
    }

    /// `n` quads left out of a write whose vertices weren't what the GPU's copy holds (`checking`)
    pub fn stale(&self, n: usize) {
        self.add(|w| w.stale += n);
    }

    /// A frame's drawing: `passes` render passes in `submits` submits, and how long it took to
    /// gather what each visual draws (writing the layers' GPU copies), to record the passes, and to
    /// finish and submit them
    pub fn recorded(&self, passes: usize, submits: usize, times: [std::time::Duration; 3]) {
        self.add(|w| w.recorded(passes, submits, times));
    }

    /// What was made and written since the last time this was asked, and a frame begun: what the
    /// last frame did
    pub fn take(&self) -> Work {
        self.frame.set(self.frame.get() + 1);
        let frame = self.frame.get();
        let mut uniforms = self.uniforms.borrow_mut();
        uniforms.retain(|_, k| frame.saturating_sub(k.used) < UNIFORM_FRAMES);
        // No more than config's GPU_UNIFORMS_KEPT, a buffer of 64 KB of the GPU's memory each:
        // past it, the half drawn longest ago go
        if let Some(since) = uniforms_kept_since(uniforms.values().map(|k| k.used), uniforms_kept()) {
            uniforms.retain(|_, k| k.used >= since);
        }
        self.work.take()
    }

    /// The window about to be drawn at `width` by `height` pixels with the text's colour transform
    /// `hsb`: the uniforms of a size it was at before the one before go, since their projections
    /// are made from the window's size and none of them can be drawn with till it's back there.
    /// Each was a buffer of 64 KB of the GPU's memory, and kept by age a window resized held one
    /// for each place at each size (lbterm_motion::content::UniformsAt)
    pub fn drawing_at(&self, width: u32, height: u32, hsb: [f32; 3]) {
        let mut at = self.at.get();
        let gone = at.drawn_at(uniforms_at(width, height, hsb));
        self.at.set(at);
        if let Some(gone) = gone {
            self.uniforms.borrow_mut().retain(|_, k| k.at != Some(gone));
        }
    }

    /// The bind groups this window keeps for its frames: the atlas's two and each kept uniform's
    /// (the state file's `visuals.bind_groups`). With D3D12 each takes a view in the device's view
    /// heap, the atlas's two each
    pub fn kept(&self) -> usize {
        self.uniforms.borrow().len() + self.atlas.borrow().as_ref().map_or(0, |_| 2)
    }

    /// The atlas `texture`'s bind groups, its linear sampler's and its nearest's: made again only
    /// for another atlas
    pub fn atlas(&self, webgpu: &WebGpuState, texture: &Rc<dyn Texture2d>) -> (Rc<wgpu::BindGroup>, Rc<wgpu::BindGroup>) {
        let mut atlas = self.atlas.borrow_mut();
        if let Some(a) = atlas.as_ref().filter(|a| Rc::ptr_eq(&a.texture, texture)) {
            return (Rc::clone(&a.linear), Rc::clone(&a.nearest));
        }
        let tex = texture
            .downcast_ref::<crate::termwindow::webgpu::WebGpuTexture>()
            .expect("the WebGpu front end's atlas is a WebGpuTexture");
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let group = |sampler: &wgpu::Sampler| {
            Rc::new(webgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &webgpu.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
                label: Some("atlas bind group"),
            }))
        };
        let made = Atlas {
            texture: Rc::clone(texture),
            linear: group(&webgpu.texture_linear_sampler),
            nearest: group(&webgpu.texture_nearest_sampler),
        };
        self.made(3);
        let out = (Rc::clone(&made.linear), Rc::clone(&made.nearest));
        *atlas = Some(made);
        out
    }

    /// A bind group holding `uniform`: the one made for the same projection and colour transform
    /// before, while it's been used in the last `UNIFORM_FRAMES` frames and the window's size
    /// hasn't moved on twice since (`drawing_at`, which the frame calls first)
    pub fn uniform(&self, webgpu: &WebGpuState, uniform: ShaderUniform) -> Rc<wgpu::BindGroup> {
        let key = uniform_key(uniform.foreground_text_hsb, &uniform.projection);
        let frame = self.frame.get();
        let at = self.at.get().now();
        let mut uniforms = self.uniforms.borrow_mut();
        if let Some(kept) = uniforms.get_mut(&key) {
            // A projection can come out the same at another size (a visual a pixel over in a
            // window a pixel wider): it's this size's now
            kept.used = frame;
            kept.at = at;
            return Rc::clone(&kept.group);
        }
        let group = Rc::new(webgpu.create_uniform(ShaderUniform {
            milliseconds: 0,
            ..uniform
        }));
        self.made(2);
        uniforms.insert(
            key,
            Kept {
                group: Rc::clone(&group),
                used: frame,
                at,
            },
        );
        group
    }
}
