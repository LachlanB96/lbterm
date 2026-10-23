//! LBTerm: drawing a frame into the compositor's visuals
//! (lbterm/COMPOSITOR.md).
//!
//! The paint pass is WezTerm's own: it fills one set of vertex buffers with
//! every quad in the window. Here those buffers are drawn once per visual,
//! each time with only the quads the paint pass marked as that visual's,
//! through a projection that maps the visual's rectangle of the window onto
//! its swapchain. Anything outside the rectangle falls off the edge.
//!
//! Every visual is a swapchain, and D3D12 lets a command list, and a
//! submit, write to at most eight, so a frame records into one encoder per
//! eight visuals and submits each on its own.

use crate::quad::VERTICES_PER_CELL;
use crate::termwindow::compositor::{Frame, Keyboard, MotionSettings};
use crate::termwindow::render::{card, hover_fade, lights, strip, strip_swap, tab_panel};
use crate::termwindow::webgpu::{ShaderUniform, WebGpuTexture};
use anyhow::Context;
use lbterm_motion::{command_list_for, command_lists, PixelRect, Target};
use std::cmp::Ordering;

/// Two triangles per quad.
const INDICES_PER_QUAD: usize = 6;

impl crate::TermWindow {
    pub fn call_draw_composited(&mut self) -> anyhow::Result<()> {
        let render_state = self.render_state.as_ref().unwrap();

        // The last span runs to wherever z-layer 0 ended, modals included
        let counts = {
            let layer = render_state.layer_for_zindex(0)?;
            let vbs = layer.vb.borrow();
            [0, 1, 2].map(|i| vbs[i].vertex_index_count().0 / VERTICES_PER_CELL)
        };
        self.lbterm_spans.finish(counts);

        let webgpu = self.webgpu.as_ref().unwrap();
        let compositor = webgpu.compositor.as_ref().unwrap();
        let window = PixelRect::new(
            0,
            0,
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        // LBTerm: whether anything is drawn in the tab menu's z-layers this frame
        let top = render_state.layers.borrow().iter().any(|layer| {
            crate::termwindow::render::tab_menu::owns_zindex(layer.zindex())
                && (0..3).any(|i| layer.vb.borrow()[i].vertex_index_count().0 > 0)
        });
        let started = std::time::Instant::now();
        let motion = MotionSettings::from_config(&self.config.lbterm_motion);
        compositor.prepare(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            window,
            self.lbterm_spans.panes(),
            &motion,
            self.lbterm_panel.plan.as_ref(),
            top,
            self.lbterm_cards
                .as_ref()
                .context("the card templates weren't drawn this frame")?,
            self.lbterm_spans.active(),
        )?;
        // LBTerm: the active outline's light: out without the keyboard, a pulse as it arrives
        let keyboard = Keyboard {
            has: self.focused.is_some() || self.lbterm_keyboard_assumed,
            arrivals: self
                .lbterm_arrival
                .frame(self.lbterm_spans.active(), std::time::Instant::now()),
        };
        compositor.prepare_keyboard(keyboard, &motion)?;
        // LBTerm: the strip's hover slots, and the fades this paint decided (render/hover_fade.rs)
        {
            let mut hover = self.lbterm_hover.borrow_mut();
            let fades = std::mem::take(&mut hover.pending);
            compositor.prepare_hover(
                &webgpu.instance,
                &webgpu.device,
                &webgpu.config.borrow(),
                hover.strip.take(),
                fades,
                &motion,
            )?;
        }
        // LBTerm: the strip's detail card, fading as it opens and closes (render/strip.rs)
        compositor.prepare_strip_card(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_strip_card_visual(),
            &motion,
        )?;
        // LBTerm: the turning arc on a Conductor menu item that waits (render/conductor.rs)
        compositor.prepare_menu_spinner(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_conductor_spinner(),
            &motion,
        )?;
        // LBTerm: the lights asked for since the last frame, and the readout (render/lights.rs)
        compositor.prepare_lights(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            window,
            self.lbterm_lights_take(),
            &motion,
        )?;
        compositor.prepare_readout(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_readout_visual(),
            &motion,
        )?;
        // LBTerm: the strip as it was, fading over its new words (render/strip_swap.rs)
        compositor.prepare_swap(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_strip_swap_visual(),
            &motion,
        )?;
        let prepared = started.elapsed();

        let mut frames = vec![Frame {
            target: Target::Under,
            rect: window,
            texture: webgpu.surface.get_current_texture()?,
        }];
        frames.extend(compositor.acquire()?);
        frames.extend(compositor.acquire_hover()?);
        frames.extend(compositor.acquire_strip_card()?);
        frames.extend(compositor.acquire_menu_spinner()?);
        frames.extend(compositor.acquire_lights()?);
        frames.extend(compositor.acquire_readout()?);
        frames.extend(compositor.acquire_swap()?);
        let acquired = started.elapsed() - prepared;
        // Making or resizing swapchains blocks the GUI thread: worth seeing
        if prepared + acquired > std::time::Duration::from_millis(8) {
            log::debug!("compositor: prepare {prepared:?}, acquire {acquired:?}");
        }

        let tex = render_state.glyph_cache.borrow().atlas.texture();
        let tex = tex.downcast_ref::<WebGpuTexture>().unwrap();
        let texture_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_bind_group = |sampler: &wgpu::Sampler| {
            webgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &webgpu.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
                label: Some("atlas bind group"),
            })
        };
        let linear = atlas_bind_group(&webgpu.texture_linear_sampler);
        let nearest = atlas_bind_group(&webgpu.texture_nearest_sampler);

        let hsb = self.config.foreground_text_hsb;
        let milliseconds = self.created.elapsed().as_millis() as u32;
        // WezTerm's vertices are in window pixels, with 0,0 in the middle
        let (half_w, half_h) = (window.w as f32 / 2.0, window.h as f32 / 2.0);
        let views: Vec<wgpu::TextureView> = frames
            .iter()
            .map(|f| {
                f.texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default())
            })
            .collect();
        let uniforms: Vec<wgpu::BindGroup> = frames
            .iter()
            .map(|f| {
                let r = f.rect;
                webgpu.create_uniform(ShaderUniform {
                    foreground_text_hsb: [hsb.hue, hsb.saturation, hsb.brightness],
                    milliseconds,
                    // Not to_arrays_transposed() as upstream has it: WGSL's
                    // column-major mat4x4 wants euclid's rows as they are.
                    // Upstream's window-centred projection has no
                    // translation, so the transpose never showed; a pane's
                    // has one, and transposed it lands in w and warps rows.
                    projection: euclid::Transform3D::<f32, f32, f32>::ortho(
                        r.x as f32 - half_w,
                        (r.x + r.w as i32) as f32 - half_w,
                        (r.y + r.h as i32) as f32 - half_h,
                        r.y as f32 - half_h,
                        -1.0,
                        1.0,
                    )
                    .to_arrays(),
                })
            })
            .collect();
        let mut drawn = vec![false; frames.len()];

        if log::log_enabled!(log::Level::Debug) {
            for f in &frames {
                let tex = &f.texture.texture;
                log::debug!(
                    "visual {:?} at {:?}, texture {}x{} {:?}, quads {:?}",
                    f.target,
                    f.rect,
                    tex.width(),
                    tex.height(),
                    tex.format(),
                    [0, 1, 2].map(|sub| self.lbterm_spans.ranges(sub, f.target, usize::MAX))
                );
            }
        }

        let mut encoders: Vec<wgpu::CommandEncoder> = (0..command_lists(frames.len()))
            .map(|_| {
                webgpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("LBTerm composited encoder"),
                    })
            })
            .collect();

        for layer in render_state.layers.borrow().iter() {
            let zindex = layer.zindex();
            for idx in 0..3 {
                let vb = &layer.vb.borrow()[idx];
                let (vertex_count, _) = vb.vertex_index_count();
                if vertex_count > 0 {
                    let quads = vertex_count / VERTICES_PER_CELL;
                    let mut vertices = vb.current_vb_mut();
                    let vertex_buffer = vertices.webgpu_mut().recreate();
                    vertex_buffer.unmap();

                    for (i, frame) in frames.iter().enumerate() {
                        let ranges = match zindex.cmp(&0) {
                            // LBTerm: the tab menu's z-layers are `top`'s alone
                            _ if crate::termwindow::render::tab_menu::owns_zindex(zindex) => {
                                if frame.target == Target::Top {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // LBTerm: a card template's z-layer is its visual's alone (render/card.rs)
                            _ if card::part_of_zindex(zindex).is_some() => {
                                if Some(frame.target) == card::part_of_zindex(zindex).map(Target::Card) {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // LBTerm: the strip's card's z-layers are its visual's alone (render/strip.rs)
                            _ if strip::owns_zindex(zindex) => {
                                if frame.target == Target::StripCard {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // LBTerm: the strip's swap's z-layers are its visual's alone (render/strip_swap.rs)
                            _ if strip_swap::owns_zindex(zindex) => {
                                if frame.target == Target::Swap {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // LBTerm: the readout's z-layers are its visual's alone (render/lights.rs)
                            _ if lights::owns_zindex(zindex) => {
                                if frame.target == Target::Readout {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // LBTerm: a hover slot's z-layer is its visual's alone (render/hover_fade.rs)
                            _ if hover_fade::owns_zindex(zindex) => {
                                if Some(frame.target) == hover_fade::slot_of(zindex).map(Target::Hover) {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // LBTerm: the menu's turning arc is its visual's alone (render/conductor.rs)
                            _ if zindex == crate::termwindow::render::conductor::MENU_SPIN_Z => {
                                if frame.target == Target::MenuSpinner {
                                    vec![0..quads]
                                } else {
                                    vec![]
                                }
                            }
                            // The tab panel's z-layers are its visuals' alone
                            _ if tab_panel::owns_zindex(zindex) => match frame.target {
                                Target::Panel(part) if tab_panel::draws(zindex, part) => {
                                    vec![0..quads]
                                }
                                _ => vec![],
                            },
                            Ordering::Equal => self.lbterm_spans.ranges(idx, frame.target, quads),
                            Ordering::Less if frame.target == Target::Under => vec![0..quads],
                            Ordering::Greater if frame.target == Target::Over => vec![0..quads],
                            _ => vec![],
                        };
                        if ranges.is_empty() {
                            continue;
                        }
                        let encoder = &mut encoders[command_list_for(i)];
                        let mut pass = begin_pass(encoder, &views[i], drawn[i]);
                        drawn[i] = true;
                        pass.set_pipeline(&webgpu.render_pipeline);
                        pass.set_bind_group(0, &uniforms[i], &[]);
                        pass.set_bind_group(1, &linear, &[]);
                        pass.set_bind_group(2, &nearest, &[]);
                        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            vb.indices.webgpu().slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        for r in ranges {
                            pass.draw_indexed(
                                (r.start * INDICES_PER_QUAD) as u32
                                    ..(r.end * INDICES_PER_QUAD) as u32,
                                0,
                                0..1,
                            );
                        }
                    }
                }
                vb.next_index();
            }
        }

        // A visual nothing was drawn into still needs a cleared frame; the lights' swatch is
        // all white (render/lights.rs)
        for (i, drawn) in drawn.iter().enumerate() {
            if !drawn {
                let clear = if frames[i].target == Target::Swatch {
                    wgpu::Color::WHITE
                } else {
                    wgpu::Color::TRANSPARENT
                };
                clear_pass(&mut encoders[command_list_for(i)], &views[i], clear);
            }
        }

        let dumps = FrameDump::start(&webgpu.device, &mut encoders, &frames);

        for encoder in encoders {
            webgpu.queue.submit(std::iter::once(encoder.finish()));
        }
        if let Some(dumps) = dumps {
            dumps.save(&webgpu.device);
        }
        // The slow-frame log times presenting apart from drawing: a present can wait on DWM, a
        // refresh at a time (lbterm_extras::slow)
        let _slow =
            lbterm_extras::slow::paint(self.mux_window_id, lbterm_extras::slow::Part::Present);
        for frame in frames {
            frame.texture.present();
        }
        compositor.commit_if_changed()
    }
}

/// `LBTERM_DUMP_FRAMES=<dir>` saves every visual's texture of the first
/// [`FrameDump::FRAMES`] frames as PNGs, so what LBTerm drew can be checked
/// apart from what DWM shows. Each file is `frame<n>-<visual>.png`.
struct FrameDump {
    dir: std::path::PathBuf,
    number: usize,
    copies: Vec<(String, wgpu::Buffer, u32, u32, u32)>,
}

impl FrameDump {
    const FRAMES: usize = 6;

    fn start(
        device: &wgpu::Device,
        encoders: &mut [wgpu::CommandEncoder],
        frames: &[Frame],
    ) -> Option<Self> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DUMPED: AtomicUsize = AtomicUsize::new(0);

        let dir = std::path::PathBuf::from(std::env::var_os("LBTERM_DUMP_FRAMES")?);
        let number = DUMPED.fetch_add(1, Ordering::Relaxed);
        if number >= Self::FRAMES {
            return None;
        }
        let copies = frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let texture = &f.texture.texture;
                let (w, h) = (texture.width(), texture.height());
                // Rows of a texture copy are 256-byte aligned
                let row = (w * 4 + 255) / 256 * 256;
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("LBTerm frame dump"),
                    size: (row * h) as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoders[command_list_for(i)].copy_texture_to_buffer(
                    texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(row),
                            rows_per_image: Some(h),
                        },
                    },
                    texture.size(),
                );
                let name = match f.target {
                    Target::Under => "under".to_string(),
                    Target::Over => "over".to_string(),
                    Target::Pane(id) => format!("pane{id}"),
                    Target::Splits => "splits".to_string(),
                    Target::Panel(part) => format!("panel-{part:?}").to_lowercase(),
                    Target::Top => "top".to_string(),
                    Target::Card(part) => format!("card-{part:?}").to_lowercase(),
                    Target::Hover(slot) => format!("hover{slot}"),
                    Target::StripCard => "strip-card".to_string(),
                    Target::MenuSpinner => "menu-spinner".to_string(),
                    Target::Swatch => "swatch".to_string(),
                    Target::Readout => "readout".to_string(),
                    Target::Swap => "swap".to_string(),
                };
                (name, buffer, w, h, row)
            })
            .collect();
        Some(Self {
            dir,
            number,
            copies,
        })
    }

    fn save(self, device: &wgpu::Device) {
        for (_, buffer, ..) in &self.copies {
            buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        if let Err(err) = device.poll(wgpu::PollType::Wait) {
            log::error!("frame dump: {err}");
            return;
        }
        for (name, buffer, w, h, row) in &self.copies {
            let data = buffer.slice(..).get_mapped_range();
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..*h as usize {
                let line = &data[y * *row as usize..][..*w as usize * 4];
                // The swapchains are BGRA
                for px in line.chunks_exact(4) {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            }
            let path = self.dir.join(format!("frame{}-{name}.png", self.number));
            match image::RgbaImage::from_raw(*w, *h, rgba) {
                Some(img) => {
                    if let Err(err) = img.save(&path) {
                        log::error!("frame dump {}: {err}", path.display());
                    }
                }
                None => log::error!("frame dump {}: wrong size", path.display()),
            }
        }
    }
}

/// A pass that only clears one visual's texture, to `colour`.
fn clear_pass(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, colour: wgpu::Color) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("LBTerm clear pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(colour),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
    });
}

/// A pass over one visual's texture: cleared to transparent the first time
/// in a frame, drawn over after that.
fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    keep: bool,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("LBTerm visual pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if keep {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
    })
}
