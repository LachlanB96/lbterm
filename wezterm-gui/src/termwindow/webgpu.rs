use crate::quad::Vertex;
use crate::termwindow::compositor::Compositor;
use config::{ConfigHandle, GpuInfo};
use std::cell::RefCell;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use window::bitmaps::Texture2d;
use window::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use window::{BitmapImage, Dimensions, Rect, Window};

#[repr(C)]
#[derive(Copy, Clone, Default, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderUniform {
    pub foreground_text_hsb: [f32; 3],
    pub milliseconds: u32,
    pub projection: [[f32; 4]; 4],
    // sampler2D atlas_nearest_sampler;
    // sampler2D atlas_linear_sampler;
}

pub struct WebGpuState {
    pub adapter_info: wgpu::AdapterInfo,
    pub downlevel_caps: wgpu::DownlevelCapabilities,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: Arc<wgpu::Queue>,
    pub config: RefCell<wgpu::SurfaceConfiguration>,
    pub dimensions: RefCell<Dimensions>,
    pub render_pipeline: wgpu::RenderPipeline,
    shader_uniform_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_nearest_sampler: wgpu::Sampler,
    pub texture_linear_sampler: wgpu::Sampler,
    pub handle: RawHandlePair,
    /// LBTerm: kept so the compositor can make a surface for each pane
    pub instance: wgpu::Instance,
    /// LBTerm: the window's visual tree when `lbterm_compositor` is on.
    /// Declared after `surface`, which holds a reference to its visual.
    pub compositor: Option<Compositor>,
    /// LBTerm: set when wgpu says the device has gone (gpu_lost.rs)
    pub lost: crate::termwindow::gpu_lost::Lost,
    /// LBTerm: what `surface`'s swapchain shows, with the compositor (the window background's
    /// visual, drawn only when that changes: render/composited.rs); nothing known once it's
    /// configured again
    pub lbterm_under_shows: std::rc::Rc<std::cell::Cell<Option<u64>>>,
    /// LBTerm: what a frame makes and writes on the GPU, and what's kept between frames
    /// (gpu_work.rs)
    pub lbterm_gpu: crate::termwindow::gpu_work::GpuWork,
}

#[derive(Clone, Copy)]
pub struct RawHandlePair {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

impl RawHandlePair {
    fn new(window: &Window) -> Self {
        Self {
            window: window.window_handle().expect("window handle").as_raw(),
            display: window.display_handle().expect("display handle").as_raw(),
        }
    }

    /// The window's HWND, for DirectComposition and the taskbar button; null off Windows.
    pub fn hwnd(&self) -> *mut std::ffi::c_void {
        match self.window {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => h.hwnd.get() as *mut std::ffi::c_void,
            _ => std::ptr::null_mut(),
        }
    }
}

impl HasWindowHandle for RawHandlePair {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        unsafe { Ok(WindowHandle::borrow_raw(self.window)) }
    }
}

impl HasDisplayHandle for RawHandlePair {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        unsafe { Ok(DisplayHandle::borrow_raw(self.display)) }
    }
}

pub struct WebGpuTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    queue: Arc<wgpu::Queue>,
}

impl std::ops::Deref for WebGpuTexture {
    type Target = wgpu::Texture;
    fn deref(&self) -> &Self::Target {
        &self.texture
    }
}

impl Texture2d for WebGpuTexture {
    fn write(&self, rect: Rect, im: &dyn BitmapImage) {
        let (im_width, im_height) = im.image_dimensions();

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: rect.min_x() as u32,
                    y: rect.min_y() as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            im.pixel_data_slice(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(im_width as u32 * 4),
                rows_per_image: Some(im_height as u32),
            },
            wgpu::Extent3d {
                width: im_width as u32,
                height: im_height as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    fn read(&self, _rect: Rect, _im: &mut dyn BitmapImage) {
        unimplemented!();
    }

    fn width(&self) -> usize {
        self.width as usize
    }

    fn height(&self) -> usize {
        self.height as usize
    }
}

impl WebGpuTexture {
    pub fn new(width: u32, height: u32, state: &WebGpuState) -> anyhow::Result<Self> {
        let limit = state.device.limits().max_texture_dimension_2d;

        if width > limit || height > limit {
            // Ideally, wgpu would have a fallible create_texture method,
            // but it doesn't: instead it will panic if the requested
            // dimension is too large.
            // So we check the limit ourselves here.
            // <https://github.com/wezterm/wezterm/issues/3713>
            anyhow::bail!(
                "texture dimensions {width}x{height} exceeed the \
                 max dimension {limit} supported by your GPU"
            );
        }

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let view_formats = if state
            .downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format, format.remove_srgb_suffix()]
        } else {
            vec![]
        };
        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("Texture Atlas"),
            view_formats: &view_formats,
        });
        Ok(Self {
            texture,
            width,
            height,
            queue: Arc::clone(&state.queue),
        })
    }
}

pub fn adapter_info_to_gpu_info(info: wgpu::AdapterInfo) -> GpuInfo {
    GpuInfo {
        name: info.name,
        vendor: Some(info.vendor),
        device: Some(info.device),
        device_type: format!("{:?}", info.device_type),
        driver: if info.driver.is_empty() {
            None
        } else {
            Some(info.driver)
        },
        driver_info: if info.driver_info.is_empty() {
            None
        } else {
            Some(info.driver_info)
        },
        backend: format!("{:?}", info.backend),
    }
}

fn compute_compatibility_list(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    surface: &wgpu::Surface,
) -> Vec<String> {
    instance
        .enumerate_adapters(backends)
        .into_iter()
        .map(|a| {
            let info = adapter_info_to_gpu_info(a.get_info());
            let compatible = a.is_surface_supported(&surface);
            format!(
                "{}, compatible={}",
                info.to_string(),
                if compatible { "yes" } else { "NO" }
            )
        })
        .collect()
}

impl WebGpuState {
    pub async fn new(
        window: &Window,
        dimensions: Dimensions,
        config: &ConfigHandle,
    ) -> anyhow::Result<Self> {
        let handle = RawHandlePair::new(window);
        if config::lbterm::uses_compositor(config) {
            match Self::new_impl(handle, dimensions, config, true).await {
                Ok(state) => return Ok(state),
                // The window has no redirection bitmap, but a flip-model
                // swapchain on the HWND presents without one
                Err(err) => log::error!(
                    "lbterm_compositor: {err:#}; drawing to the window directly instead"
                ),
            }
        }
        Self::new_impl(handle, dimensions, config, false).await
    }

    pub async fn new_impl(
        handle: RawHandlePair,
        dimensions: Dimensions,
        config: &ConfigHandle,
        use_compositor: bool,
    ) -> anyhow::Result<Self> {
        // LBTerm: how long each step of making a window's GPU state takes, said on lbterm::start
        // at debug (lbterm/PERF.md, "What font setup costs": a window's start)
        let mut steps = lbterm_extras::start::Steps::new("webgpu");
        let compositor = if use_compositor {
            Some(Compositor::new(handle.hwnd())?)
        } else {
            None
        };
        steps.step("compositor");
        // A DComp visual takes a DXGI swapchain, so only DX12 will do
        let backends = super::gpu_warm::backends_for(config, compositor.is_some());
        // LBTerm: the instance and the adapter are the process's, made once and begun as the GUI
        // starts (gpu_warm.rs); this waits for them if they're still being made
        let gpu = super::gpu_warm::shared(backends, config)?;
        let instance = gpu.instance.clone();
        steps.step("instance");
        let surface = match &compositor {
            Some(compositor) => compositor.create_under_surface(&instance)?,
            None => unsafe {
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&handle)?)?
            },
        };
        steps.step("surface");

        let adapter = match &gpu.adapter {
            Some(adapter) if adapter.is_surface_supported(&surface) => adapter.clone(),
            Some(adapter) => {
                let adapters = compute_compatibility_list(&instance, backends, &surface);
                anyhow::bail!(
                    "{} can't draw to this window. Available:\n{}",
                    adapter_info_to_gpu_info(adapter.get_info()).to_string(),
                    adapters.join("\n")
                );
            }
            None => {
                let adapters = compute_compatibility_list(&instance, backends, &surface);
                anyhow::bail!(
                    "no compatible adapter found. Available:\n{}",
                    adapters.join("\n")
                );
            }
        };
        steps.step("adapter");
        let adapter_info = adapter.get_info();
        log::trace!("Using adapter: {adapter_info:?}");
        let caps = surface.get_capabilities(&adapter);
        log::trace!("caps: {caps:?}");
        let downlevel_caps = adapter.get_downlevel_capabilities();
        log::trace!("downlevel_caps: {downlevel_caps:?}");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                required_limits: wgpu::Limits {
                    // LBTerm: D3D12's view heap the size of what a window binds, with room, where
                    // the default's 1,000,000 views were 30.5 MB of the GPU's own memory a window
                    // (config/src/lbterm.rs, GPU_VIEW_DESCRIPTORS; lbterm/PERF.md, finding 96)
                    max_non_sampler_bindings: config::lbterm::GPU_VIEW_DESCRIPTORS,
                    ..if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::downlevel_defaults()
                    }
                    .using_resolution(adapter.limits())
                },
                label: None,
                // LBTerm: D3D12's allocator's blocks smaller than Performance's 256 MB, which a
                // window committed whole for the few MB a terminal draws with
                // (config/src/lbterm.rs, GPU_MEMORY_BLOCK; lbterm/PERF.md, finding 95). Only
                // D3D12's, the compositor's backend, where it was measured: Vulkan's allocator
                // takes the hint its own way (gpu_alloc's free-list chunks, and its buddy
                // allocator's first chunk 8 times Performance's), which nothing measured
                memory_hints: if adapter_info.backend == wgpu::Backend::Dx12 {
                    wgpu::MemoryHints::Manual {
                        suballocated_device_memory_block_size: config::lbterm::GPU_MEMORY_BLOCK
                            ..config::lbterm::GPU_MEMORY_BLOCK,
                    }
                } else {
                    wgpu::MemoryHints::default()
                },
                trace: wgpu::Trace::Off,
            })
            .await?;
        steps.step("device");

        // LBTerm: a lost device or an error nobody captured is noted, not a panic (gpu_lost.rs)
        let lost = crate::termwindow::gpu_lost::Lost::default();
        crate::termwindow::gpu_lost::watch(&device, &lost);

        let queue = Arc::new(queue);

        // Explicitly request an SRGB format, if available
        let pref_format_srgb = caps.formats[0].add_srgb_suffix();
        let format = if caps.formats.contains(&pref_format_srgb) {
            pref_format_srgb
        } else {
            caps.formats[0]
        };

        // Need to check that this is supported, as trying to set
        // view_formats without it will cause surface.configure
        // to panic
        // <https://github.com/wezterm/wezterm/issues/3565>
        let view_formats = if downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            vec![format.add_srgb_suffix(), format.remove_srgb_suffix()]
        } else {
            vec![]
        };

        let config = wgpu::SurfaceConfiguration {
            // LBTerm: copyable with the compositor, for LBTERM_DUMP_FRAMES
            usage: if compositor.is_some() {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            },
            format,
            width: dimensions.pixel_width as u32,
            height: dimensions.pixel_height as u32,
            present_mode: wgpu::PresentMode::Fifo,
            // Composition swapchains take premultiplied alpha or none; DXGI
            // refuses straight alpha. Rendering onto a cleared, transparent
            // target leaves premultiplied pixels behind.
            alpha_mode: if compositor.is_some() {
                if config.window_background_opacity < 1.0 {
                    wgpu::CompositeAlphaMode::PreMultiplied
                } else {
                    wgpu::CompositeAlphaMode::Opaque
                }
            } else if caps
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
            {
                wgpu::CompositeAlphaMode::PostMultiplied
            } else if caps
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
            {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                wgpu::CompositeAlphaMode::Auto
            },
            view_formats,
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        steps.step("configure");
        if let Some(compositor) = &compositor {
            // configure made the swapchain the visual's content
            compositor.commit()?;
            log::info!(
                "lbterm_compositor: drawing through DirectComposition on {} ({:?})",
                adapter_info.name,
                adapter_info.backend
            );
        }

        let shader =device.create_shader_module(wgpu::include_wgsl!("../shader.wgsl"));
        steps.step("shader");

        let shader_uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("ShaderUniform bind group layout"),
            });

        let texture_nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture_linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture bind group layout"),
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[
                    &shader_uniform_bind_group_layout,
                    &texture_bind_group_layout,
                    &texture_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),

            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });
        steps.step("pipeline");
        steps.done();

        Ok(Self {
            adapter_info,
            downlevel_caps,
            surface,
            device,
            queue,
            config: RefCell::new(config),
            dimensions: RefCell::new(dimensions),
            render_pipeline,
            handle,
            shader_uniform_bind_group_layout,
            texture_bind_group_layout,
            texture_nearest_sampler,
            texture_linear_sampler,
            instance,
            compositor,
            lost,
            lbterm_under_shows: Default::default(),
            lbterm_gpu: Default::default(),
        })
    }

    pub fn create_uniform(&self, uniform: ShaderUniform) -> wgpu::BindGroup {
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ShaderUniform Buffer"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.shader_uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
            label: Some("ShaderUniform Bind Group"),
        })
    }

    #[allow(unused_mut)]
    /// LBTerm: the window's size now, as Windows has it, which the surface is made at
    pub fn client_size(&self) -> Option<(usize, usize)> {
        match self.handle.window {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => {
                let mut rect = unsafe { std::mem::zeroed() };
                unsafe { winapi::um::winuser::GetClientRect(h.hwnd.get() as _, &mut rect) };
                Some(((rect.right - rect.left) as usize, (rect.bottom - rect.top) as usize))
            }
            _ => None,
        }
    }

    pub fn resize(&self, mut dims: Dimensions) {
        // During a live resize on Windows, the Dimensions that we're processing may be
        // lagging behind the true client size. We have to take the very latest value
        // from the window or else the underlying driver will raise an error about
        // the mismatch, so we need to sneakily read through the handle
        match self.handle.window {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => {
                let mut rect = unsafe { std::mem::zeroed() };
                unsafe { winapi::um::winuser::GetClientRect(h.hwnd.get() as _, &mut rect) };
                dims.pixel_width = (rect.right - rect.left) as usize;
                dims.pixel_height = (rect.bottom - rect.top) as usize;
            }
            _ => {}
        }

        if dims == *self.dimensions.borrow() {
            return;
        }
        *self.dimensions.borrow_mut() = dims;
        let mut config = self.config.borrow_mut();
        config.width = dims.pixel_width as u32;
        config.height = dims.pixel_height as u32;
        if config.width > 0 && config.height > 0 {
            // Avoid reconfiguring with a 0 sized surface, as webgpu will
            // panic in that case
            // <https://github.com/wezterm/wezterm/issues/2881>
            self.surface.configure(&self.device, &config);
            self.lbterm_under_shows.set(None);
        }
    }
}
