//! LBTerm: drawing a frame into the compositor's visuals
//! (lbterm/COMPOSITOR.md).
//!
//! The paint pass is WezTerm's own: it fills one set of vertex buffers with
//! every quad in the window. Here those buffers are drawn once per visual,
//! each time with only the quads the paint pass marked as that visual's,
//! through a projection that maps the visual's rectangle of the window onto
//! its swapchain. Anything outside the rectangle falls off the edge.
//!
//! Each visual drawn is one render pass, binding in turn every z-layer it
//! has quads in. Every visual is a swapchain, and D3D12 lets a command list,
//! and a submit, write to at most eight, so a frame records into one encoder
//! per eight visuals and submits each on its own; a frame that draws none
//! submits nothing.
//!
//! Only what changed is drawn: a visual whose swapchain already shows what
//! this frame would draw into it (`lbterm_content`, lbterm_motion::content)
//! isn't acquired, drawn or presented, and DWM goes on showing its last
//! frame. The InMail button's count changing draws the strip's visual and
//! the swap's, not the panes', the rail's and the cards' twenty more.

use crate::quad::VERTICES_PER_CELL;
use crate::termwindow::compositor::{Frame, Keyboard};
use crate::termwindow::render::{
    card, hover_fade, lights, menu_fade, new_tab, objectives, questions_pane, sides, strip, strip_swap, tab_panel,
    tasks_card,
};
use crate::termwindow::webgpu::ShaderUniform;
use anyhow::Context;
use lbterm_motion::content::{must_draw, pair, Key};
use lbterm_motion::{command_list_for, command_lists, PixelRect, Target};
use mux::pane::PaneId;
use std::cell::Cell;
use std::cmp::Ordering;
use std::ops::Range;
use window::WindowOps;

/// Two triangles per quad.
const INDICES_PER_QUAD: usize = 6;

/// LBTerm: when the soonest of what fades out and is put away once done is done: the panes of a
/// tab left or a closed pane, the strips' pictures with them, and the strip's words in flight
fn lbterm_fading_done(compositor: &crate::termwindow::compositor::Compositor) -> Option<std::time::Instant> {
    [compositor.fading_panes().1, compositor.fading_sides().1, compositor.fading_swaps().1]
        .iter()
        .flatten()
        .min()
        .copied()
}

impl crate::TermWindow {
    pub fn call_draw_composited(&mut self) -> anyhow::Result<()> {
        // LBTerm: the bursts of hearts risen since the last frame, handed to DWM once each, and
        // where the panes they rise over are now (render/triggers.rs)
        let hearts = self.lbterm_hearts_plan();
        // LBTerm: what the panes without the keyboard dim toward, the window background as
        // `under` stores it (compositor.rs, `PaneLayer::dim_to`)
        let backdrop = {
            let (r, g, b, _) = self.palette().background.to_srgb_u8();
            [r, g, b].map(|c| c as f64 / 255.0)
        };
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
        // LBTerm: whether anything is drawn in a tab link drag's z-layers this frame
        // (render/link_drag.rs), which `top` shows. The menus have visuals of their own
        // (render/menu_fade.rs)
        let top = render_state.layers.borrow().iter().any(|layer| {
            crate::termwindow::render::link_drag::owns_zindex(layer.zindex())
                && (0..3).any(|i| layer.vb.borrow()[i].vertex_index_count().0 > 0)
        });
        let started = std::time::Instant::now();
        let motion = self.lbterm_motion();
        // LBTerm: every pane dimmed while the new-tab picker has the keys (render/new_tab.rs), as
        // the window losing the keyboard dims them: before `prepare`, which dims them
        compositor.dim_all(self.lbterm_new_tab_has_keys());
        // LBTerm: a pane gone from the screen that isn't in the tab shown now left with its tab,
        // switched from or closed, which fades into the next (only asked when no pane is on
        // screen in both frames). An overlay's pane is in no tab, so one closing fades as well;
        // the panes under one that opens are still in the tab shown, and don't. Under the
        // Conductor's view nothing of the panes showed, so they go at once, as before any fade
        let shown = mux::Mux::get().get_active_tab_for_window(self.mux_window_id);
        let covered = self.lbterm_view_covers_panes();
        let left = |pane| !covered && shown.as_ref().map_or(true, |tab| !tab.contains_pane(pane));
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
            &left,
            backdrop,
        )?;
        // LBTerm: the strips down the right as the tab left showed them, fading with its panes
        // (render/sides.rs): after `prepare`, which says what the panes did
        compositor.prepare_sides(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_sides_picture(),
            &motion,
        )?;
        // LBTerm: the active outline's light: out without the keyboard, a pulse as it arrives
        let keyboard = Keyboard {
            // ...out too while the new-tab picker has the keys (render/new_tab.rs)
            has: (self.focused.is_some() || self.lbterm_keyboard_assumed) && !self.lbterm_new_tab_has_keys(),
            arrivals: self
                .lbterm_arrival
                .frame(self.lbterm_spans.active(), std::time::Instant::now()),
            // ...and its colour while modifiers are held (termwindow/held.rs)
            held: self.lbterm_held_colour(),
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
        // LBTerm: the objectives panel's cards, fading as they open and close (render/objectives.rs)
        compositor.prepare_objectives(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_objectives_visuals(),
            &motion,
        )?;
        // LBTerm: the context bar widening, the pane of its thin sections' numbers and its pane
        // sliding out from under it, and the scroll strip riding on their edge
        // (render/context_bar.rs)
        let (visuals, ride) = self.lbterm_context_bar_visuals();
        compositor.prepare_context_bar(&webgpu.instance, &webgpu.device, &webgpu.config.borrow(), visuals, ride, &motion)?;
        // LBTerm: the card of what a tab is running, beside its row (render/tasks_card.rs)
        compositor.prepare_tasks_card(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_tasks_card_visual(),
            &motion,
        )?;
        // LBTerm: the questions pane down the window's right while a right hold shows the
        // question colours, sliding out from under the window's edge (render/questions_pane.rs)
        compositor.prepare_questions_pane(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_questions_pane_visual(),
            &motion,
        )?;
        // LBTerm: the new-tab pad's look under the pointer, faded on `button_hover`, and its
        // picker beside the rail, fading as it opens and closes (render/new_tab.rs)
        compositor.prepare_new_tab(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_plus_visual(),
            self.lbterm_picker_visual(),
            &motion,
        )?;
        // LBTerm: every menu's card, fading as it opens and closes, its highlight gliding between
        // rows (render/menu_fade.rs)
        compositor.prepare_menus(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_menu_visuals(),
            &motion,
        )?;
        // LBTerm: the turning arc on a Conductor menu item that waits (render/conductor.rs), still
        // while the window can't be seen, as the rail's loops are
        compositor.prepare_menu_spinner(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_conductor_spinner(),
            lbterm_extras::work::loops_play(motion.enabled, self.lbterm_unseen()),
        )?;
        // LBTerm: the lights round a tab link drag's matched blocks (render/link_drag.rs)
        compositor.prepare_link_lamps(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_link_lamps(),
            &motion,
        )?;
        // LBTerm: the lights running along the rail's suggested tab connections
        // (render/connections.rs), still while the window can't be seen, as the rail's loops are
        compositor.prepare_connection_lamps(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            self.lbterm_connection_glow(),
            lbterm_extras::work::loops_play(motion.enabled, self.lbterm_unseen()),
        )?;
        // LBTerm: the easter egg's bursts of hearts, played by DWM (render/triggers.rs)
        compositor.prepare_hearts(
            &webgpu.instance,
            &webgpu.device,
            &webgpu.config.borrow(),
            hearts,
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
        // LBTerm: the panes of a tab left, or a closed pane, fading out, the strips' pictures
        // with them, and the strip's words in flight: once each is done its visual leaves the tree,
        // though nothing may paint by then
        let fading_done = lbterm_fading_done(compositor);
        // LBTerm: a menu's card fading out from this frame, over a tab panel held still under it:
        // the panel's wake at its end (render/tab_panel.rs, `Hover::hold`)
        let menus_faded = compositor.menus_fading_until(std::time::Instant::now());
        let prepared = started.elapsed();

        // Only the visuals whose swapchains don't show what this frame would draw into them
        // already: every other one is left as it is
        let atlas = {
            let tex = render_state.glyph_cache.borrow().atlas.texture();
            std::rc::Rc::as_ptr(&tex) as *const u8 as usize as u64
        };
        let every = FrameDump::due() || draw_all();
        let asked = Cell::new(0usize);
        let content = |target: Target<PaneId>, rect: PixelRect| -> Option<u64> {
            asked.set(asked.get() + 1);
            (!every).then(|| self.lbterm_content(target, rect, atlas))
        };
        let mut frames = vec![];
        let under = content(Target::Under, window);
        if must_draw(webgpu.lbterm_under_shows.get(), under) {
            frames.push(Frame {
                target: Target::Under,
                rect: window,
                texture: webgpu.surface.get_current_texture()?,
                content: under,
                shows: webgpu.lbterm_under_shows.clone(),
                chain: crate::present::chain(&webgpu.surface),
            });
        }
        frames.extend(compositor.acquire(&content)?);
        frames.extend(compositor.acquire_sides(&content)?);
        frames.extend(compositor.acquire_hover(&content)?);
        frames.extend(compositor.acquire_strip_card(&content)?);
        frames.extend(compositor.acquire_objectives(&content)?);
        frames.extend(compositor.acquire_context_bar(&content)?);
        frames.extend(compositor.acquire_tasks_card(&content)?);
        frames.extend(compositor.acquire_questions_pane(&content)?);
        frames.extend(compositor.acquire_new_tab(&content)?);
        frames.extend(compositor.acquire_menus(&content)?);
        frames.extend(compositor.acquire_menu_spinner(&content)?);
        frames.extend(compositor.acquire_link_lamps(&content)?);
        frames.extend(compositor.acquire_connection_lamps(&content)?);
        frames.extend(compositor.acquire_hearts(&content)?);
        frames.extend(compositor.acquire_lights(&content)?);
        frames.extend(compositor.acquire_readout(&content)?);
        frames.extend(compositor.acquire_swap(&content)?);
        self.lbterm_state_visuals(frames.iter().map(|f| f.target), asked.get().saturating_sub(frames.len()));
        let acquired = started.elapsed() - prepared;
        // Making or resizing swapchains blocks the GUI thread: worth seeing
        if prepared + acquired > std::time::Duration::from_millis(8) {
            log::debug!("compositor: prepare {prepared:?}, acquire {acquired:?}");
        }

        // LBTerm: the atlas's bind groups, kept while it's the same atlas (gpu_work.rs)
        let (linear, nearest) = webgpu
            .lbterm_gpu
            .atlas(webgpu, &render_state.glyph_cache.borrow().atlas.texture());

        let hsb = self.config.foreground_text_hsb;
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
        // LBTerm: each visual's, kept while its place, the window's size and the text's colour
        // transform are the same, and those of the size before this one in case it goes back
        // (gpu_work.rs); the shader reads no clock
        let hsb = [hsb.hue, hsb.saturation, hsb.brightness];
        webgpu.lbterm_gpu.drawing_at(window.w, window.h, hsb);
        let uniforms: Vec<std::rc::Rc<wgpu::BindGroup>> = frames
            .iter()
            .map(|f| {
                let r = f.rect;
                webgpu.lbterm_gpu.uniform(webgpu, ShaderUniform {
                    foreground_text_hsb: hsb,
                    milliseconds: 0,
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

        // LBTerm: one render pass for each visual drawn (lbterm_motion::Passes). What each draws is
        // gathered z-layer by z-layer, in the order they're drawn, each layer's GPU copy written
        // as it's reached; then each visual's pass binds the layers it has quads in, in turn.
        // Drawn layer by layer as upstream draws, a visual took a pass of its own for every
        // z-layer and sub-layer it had quads in, and one more to clear it when it had none, and
        // each pass had wgpu track and move its swapchain's texture again and close a D3D12
        // command list of its own
        let recording = std::time::Instant::now();
        let mut passes = lbterm_motion::Passes::new(frames.len());
        for layer in render_state.layers.borrow().iter() {
            let zindex = layer.zindex();
            for idx in 0..3 {
                let vb = &layer.vb.borrow()[idx];
                let (vertex_count, _) = vb.vertex_index_count();
                if vertex_count > 0 {
                    let quads = vertex_count / VERTICES_PER_CELL;
                    // LBTerm: the visuals drawn this frame that are drawn from it. With none, its
                    // GPU copy isn't needed, and with some only the quads it doesn't hold already
                    // are written to it (termwindow/gpu_work.rs, WebGpuVertexBuffer::upload): upstream
                    // made and filled a new buffer for every layer with a quad
                    let wanted: Vec<(usize, Vec<Range<usize>>)> = frames
                        .iter()
                        .enumerate()
                        .map(|(i, frame)| (i, self.lbterm_ranges(zindex, idx, frame.target, quads)))
                        .filter(|(_, ranges)| !ranges.is_empty())
                        .collect();
                    if !wanted.is_empty() {
                        let key = vb.lbterm_key();
                        let mut vertices = vb.current_vb_mut();
                        let vertex_buffer = vertices
                            .webgpu_mut()
                            .upload(vertex_count, key, &vb.hashes.borrow())
                            .clone();
                        let indices: &wgpu::Buffer = vb.indices.webgpu();
                        passes.layer((vertex_buffer, indices.clone()), wanted);
                    }
                }
                vb.next_index();
            }
        }
        let gathered = recording.elapsed();

        // One command list for each eight visuals, none for a frame that draws none
        let mut encoders: Vec<wgpu::CommandEncoder> = (0..command_lists(frames.len()))
            .map(|_| {
                webgpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("LBTerm composited encoder"),
                    })
            })
            .collect();
        for (i, frame) in frames.iter().enumerate() {
            // Cleared first: to nothing, and the lights' swatch, which nothing is drawn into, to
            // white (render/lights.rs)
            let clear = if lbterm_cleared_white(frame.target) {
                wgpu::Color::WHITE
            } else {
                wgpu::Color::TRANSPARENT
            };
            let mut pass = begin_pass(&mut encoders[command_list_for(i)], &views[i], clear);
            let mut draws = passes.of(i).peekable();
            if draws.peek().is_some() {
                pass.set_pipeline(&webgpu.render_pipeline);
                pass.set_bind_group(0, &*uniforms[i], &[]);
                pass.set_bind_group(1, &*linear, &[]);
                pass.set_bind_group(2, &*nearest, &[]);
            }
            for ((vertices, indices), ranges) in draws {
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                for r in ranges {
                    pass.draw_indexed(
                        (r.start * INDICES_PER_QUAD) as u32..(r.end * INDICES_PER_QUAD) as u32,
                        0,
                        0..1,
                    );
                }
            }
        }

        let dumps = FrameDump::start(&webgpu.device, &mut encoders, &frames);
        let recorded = recording.elapsed();

        let submits = encoders.len();
        for encoder in encoders {
            webgpu.queue.submit(std::iter::once(encoder.finish()));
        }
        webgpu.lbterm_gpu.recorded(
            passes.visuals(),
            submits,
            [gathered, recorded - gathered, recording.elapsed() - recorded],
        );
        if let Some(dumps) = dumps {
            dumps.save(&webgpu.device);
        }
        // The slow-frame log times presenting apart from drawing: a present can wait on DWM, a
        // refresh at a time (lbterm_extras::slow)
        let _slow =
            lbterm_extras::slow::paint(self.mux_window_id, lbterm_extras::slow::Part::Present);
        // LBTerm: each presented swapchain, while the timing to the screen is on (present.rs);
        // off, none is handed over and nothing is allocated
        let mut chains = Vec::new();
        for mut frame in frames {
            let chain = frame.chain.take().map(|c| (frame.target, c));
            frame.present();
            chains.extend(chain);
        }
        let committed = compositor.commit_if_changed();
        // LBTerm: what this frame made and wrote on the GPU, for the state file
        let work = webgpu.lbterm_gpu.take();
        let bind_groups = webgpu.lbterm_gpu.kept();
        crate::present::presented(self.mux_window_id, chains);
        self.lbterm_state_work(work, bind_groups);
        self.lbterm_sides_presented();
        self.lbterm_view_presented();
        self.lbterm_fading_wake(fading_done);
        self.tab_panel_hold_wake(menus_faded);
        committed
    }

    /// LBTerm: just after `at`, when the soonest pane fading out is done, put it away: its
    /// visual out of the tree and its surface back in the pool, with a commit and no frame,
    /// which would draw and present every visual for nothing on screen changing (about 20 ms
    /// of the GUI thread a frame with the tab panel on, 2026-10-29). Then again for the next
    pub(crate) fn lbterm_fading_wake(&mut self, at: Option<std::time::Instant>) {
        let Some(at) = at.map(|at| at + std::time::Duration::from_millis(2)) else {
            return;
        };
        // One already due sooner puts that away, and asks again
        if matches!(self.lbterm_fading_wake, Some(due) if due > std::time::Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_fading_wake = Some(at);
        promise::spawn::spawn(async move {
            smol::Timer::at(at).await;
            window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_fading_wake == Some(at) {
                    tw.lbterm_fading_wake = None;
                }
                let Some(webgpu) = tw.webgpu.clone() else {
                    return;
                };
                let Some(compositor) = webgpu.compositor.as_ref() else {
                    return;
                };
                match compositor.put_away_faded() {
                    // A change no frame has committed: the next frame puts them away with it
                    Ok(true) => {
                        if let Some(window) = tw.window.as_ref() {
                            window.invalidate();
                        }
                        return;
                    }
                    Ok(false) => {}
                    Err(err) => log::error!("putting away the panes done fading: {err:#}"),
                }
                tw.lbterm_fading_wake(lbterm_fading_done(compositor));
            })));
        })
        .detach();
    }

    /// The quads of z-layer `zindex`'s vertex buffer `sub`, `quads` of them this frame, that
    /// `target`'s visual is drawn from, in the order it draws them
    fn lbterm_ranges(&self, zindex: i8, sub: usize, target: Target<PaneId>, quads: usize) -> Vec<Range<usize>> {
        match zindex.cmp(&0) {
            // LBTerm: the Conductor's view's z-layers are `over`'s, before any
            // other owner, so a visual claiming them can't hide its words
            // (render/conductor_view.rs)
            _ if crate::termwindow::render::conductor_view::owns_zindex(zindex) => {
                if target == Target::Over {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: each menu's z-layers are its slot's card's alone (render/menu_fade.rs)
            _ if menu_fade::target_of(zindex).is_some() => {
                if Some(target) == menu_fade::target_of(zindex) {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: a tab link drag's z-layers are `top`'s alone (render/link_drag.rs)
            _ if crate::termwindow::render::link_drag::owns_zindex(zindex) => {
                if target == Target::Top {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: a card template's z-layer is its visual's alone (render/card.rs)
            _ if card::part_of_zindex(zindex).is_some() => {
                if Some(target) == card::part_of_zindex(zindex).map(Target::Card) {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the strip's card's z-layers are its visual's alone (render/strip.rs)
            _ if strip::owns_zindex(zindex) => {
                if target == Target::StripCard {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: each objectives card's z-layers are its visual's alone (render/objectives.rs)
            _ if objectives::card_of(zindex).is_some() => {
                if Some(target) == objectives::card_of(zindex).map(Target::Objectives) {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the context bar's pane's, drawer's and riding scroll strip's z-layers are
            // their visuals' alone (render/context_bar.rs)
            _ if crate::termwindow::render::context_bar::target_of(zindex).is_some() => {
                if Some(target) == crate::termwindow::render::context_bar::target_of(zindex) {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the tasks card's z-layers are its visual's alone (render/tasks_card.rs)
            _ if tasks_card::owns_zindex(zindex) => {
                if target == Target::TasksCard {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the questions pane's z-layers are its visual's alone (render/questions_pane.rs)
            _ if questions_pane::owns_zindex(zindex) => {
                if target == Target::QuestionsPane {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the strips' picture as a tab left showed them is its visual's alone
            // (render/sides.rs)
            _ if sides::owns_zindex(zindex) => {
                if target == Target::Sides {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the strip's swap's z-layers are its visual's alone (render/strip_swap.rs)
            _ if strip_swap::owns_zindex(zindex) => {
                if target == Target::Swap {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the readout's z-layers are its visual's alone (render/lights.rs)
            _ if lights::owns_zindex(zindex) => {
                if target == Target::Readout {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the new-tab picker's and its pad's look under the pointer are their visuals'
            // alone (render/new_tab.rs)
            _ if new_tab::target_of(zindex).is_some() => {
                if Some(target) == new_tab::target_of(zindex) {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: a hover slot's z-layer is its visual's alone (render/hover_fade.rs)
            _ if hover_fade::owns_zindex(zindex) => {
                if Some(target) == hover_fade::slot_of(zindex).map(Target::Hover) {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: a tab link drag's light is its template's alone (render/link_drag.rs)
            _ if zindex == crate::termwindow::render::link_drag::LAMP_Z => {
                if target == Target::LinkLamp {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: a suggested tab connection's light is its template's alone
            // (render/connections.rs)
            _ if zindex == crate::termwindow::render::connections::LAMP_Z => {
                if target == Target::ConnectionLamp {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the hearts' sheet is its template's alone while DWM plays them; drawn frame
            // by frame (motion off), they're `over`'s (render/triggers.rs)
            _ if zindex == crate::termwindow::render::triggers::HEARTS_Z && self.lbterm_hearts_by_dwm() => {
                if target == Target::Hearts {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // LBTerm: the menu's turning arc is its visual's alone (render/conductor.rs)
            _ if zindex == crate::termwindow::render::conductor::MENU_SPIN_Z => {
                if target == Target::MenuSpinner {
                    vec![0..quads]
                } else {
                    vec![]
                }
            }
            // The tab panel's z-layers are its visuals' alone
            _ if tab_panel::owns_zindex(zindex) => match target {
                Target::Panel(part) if tab_panel::draws(zindex, part) => {
                    vec![0..quads]
                }
                _ => vec![],
            },
            Ordering::Equal => self.lbterm_spans.ranges(sub, target, quads),
            Ordering::Less if target == Target::Under => vec![0..quads],
            Ordering::Greater if target == Target::Over => vec![0..quads],
            _ => vec![],
        }
    }

    /// What this frame would draw into `target`'s visual at `rect`, as lbterm_motion::content's
    /// key: every quad it's given, in the order it's drawn (each quad's hash, folded as it was
    /// written: quad.rs), the glyph atlas they're cut from (`atlas`, and the shape generation
    /// a new atlas moves on), the text's colour transform, and the colour it's cleared to. Two
    /// frames that agree on it draw the same pixels into it.
    fn lbterm_content(&self, target: Target<PaneId>, rect: PixelRect, atlas: u64) -> u64 {
        let render_state = self.render_state.as_ref().unwrap();
        let hsb = self.config.foreground_text_hsb;
        let cleared_white = lbterm_cleared_white(target);
        let mut key = Key::new(
            rect,
            &[
                atlas,
                self.shape_generation as u64,
                pair(hsb.hue, hsb.saturation),
                hsb.brightness.to_bits() as u64,
                cleared_white as u64,
            ],
        );
        for layer in render_state.layers.borrow().iter() {
            let zindex = layer.zindex();
            let vbs = layer.vb.borrow();
            for (sub, vb) in vbs.iter().enumerate() {
                let quads = vb.vertex_index_count().0 / VERTICES_PER_CELL;
                if quads == 0 {
                    continue;
                }
                let ranges = self.lbterm_ranges(zindex, sub, target, quads);
                if !ranges.is_empty() {
                    key.quads(zindex, sub, &ranges, &vb.hashes.borrow());
                }
            }
        }
        key.get()
    }
}

/// `LBTERM_DRAW_ALL` in a test window's environment draws every visual in every frame, kept or
/// not, as every frame did before only what changed was drawn: what a frame that changes them all
/// costs (a resize, a new palette, the first), measured frame after frame (`perf\paint-cost.ps1
/// -DrawAll`). Nothing sets it for Lachlan's LBTerm
fn draw_all() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| std::env::var_os("LBTERM_DRAW_ALL").is_some())
}

/// `LBTERM_DUMP_FRAMES=<dir>` saves every visual's texture of the first
/// [`FrameDump::FRAMES`] frames as PNGs, so what LBTerm drew can be checked
/// apart from what DWM shows. Each file is `frame<n>-<visual>.png`.
struct FrameDump {
    dir: std::path::PathBuf,
    number: usize,
    copies: Vec<(String, wgpu::Buffer, u32, u32, u32)>,
}

/// How many frames have been dumped
static DUMPED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl FrameDump {
    const FRAMES: usize = 6;

    /// Whether this frame is dumped: then every visual is drawn, kept or not, so each frame's
    /// files are every visual's
    fn due() -> bool {
        static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ASKED.get_or_init(|| std::env::var_os("LBTERM_DUMP_FRAMES").is_some())
            && DUMPED.load(std::sync::atomic::Ordering::Relaxed) < Self::FRAMES
    }

    fn start(
        device: &wgpu::Device,
        encoders: &mut [wgpu::CommandEncoder],
        frames: &[Frame],
    ) -> Option<Self> {
        use std::sync::atomic::Ordering;

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
                    Target::Objectives(card) => format!("objectives{card}"),
                    Target::ContextBar(part) => format!("context{part}"),
                    Target::MenuSpinner => "menu-spinner".to_string(),
                    Target::Swatch => "swatch".to_string(),
                    Target::Readout => "readout".to_string(),
                    Target::Swap => "swap".to_string(),
                    Target::Sides => "sides".to_string(),
                    Target::TasksCard => "tasks-card".to_string(),
                    Target::QuestionsPane => "questions-pane".to_string(),
                    Target::LinkLamp => "link-lamp".to_string(),
                    Target::ConnectionLamp => "connection-lamp".to_string(),
                    Target::Hearts => "hearts".to_string(),
                    Target::Picker => "picker".to_string(),
                    Target::PlusHover => "plus-hover".to_string(),
                    Target::MenuCard(slot) => format!("menu{slot}"),
                    Target::MenuLight(slot) => format!("menu{slot}-light"),
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

/// A visual's one pass in a frame: its texture cleared to `clear`, and everything it draws drawn
/// over that.
fn begin_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    clear: wgpu::Color,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("LBTerm visual pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        occlusion_query_set: None,
        timestamp_writes: None,
    })
}

/// LBTerm: every owner `lbterm_ranges` asks before the plain under, spans and over, in its order,
/// each with whether it claims `zindex`. `lbterm_ranges` gives a z-layer to the first arm that
/// claims it, so a second owner of a number has its quads sent into the first one's visual, where
/// they show only while that visual does (lbterm/PITFALLS.md 207, 220 and 300). The tests below
/// hold every number to one owner at most, and this list to `lbterm_ranges`'s arms
#[cfg(test)]
pub(crate) fn owners_of(zindex: i8) -> Vec<&'static str> {
    use crate::termwindow::render::{conductor, conductor_view, connections, context_bar, link_drag, triggers};
    let owners: [(&str, bool); 19] = [
        ("the Conductor's view", conductor_view::owns_zindex(zindex)),
        ("the menus", menu_fade::target_of(zindex).is_some()),
        ("a link's drag", link_drag::owns_zindex(zindex)),
        ("the card templates", card::part_of_zindex(zindex).is_some()),
        ("the strip's card", strip::owns_zindex(zindex)),
        ("the objectives' cards", objectives::card_of(zindex).is_some()),
        ("the context bar", context_bar::target_of(zindex).is_some()),
        ("the questions bar", questions_pane::owns_zindex(zindex)),
        ("the tasks card", tasks_card::owns_zindex(zindex)),
        ("the sides", sides::owns_zindex(zindex)),
        ("the strip's swap", strip_swap::owns_zindex(zindex)),
        ("the lights' readout", lights::owns_zindex(zindex)),
        ("the new-tab picker", new_tab::target_of(zindex).is_some()),
        ("the hover slots", hover_fade::owns_zindex(zindex)),
        ("a link's lamp", zindex == link_drag::LAMP_Z),
        ("a connection's lamp", zindex == connections::LAMP_Z),
        ("the hearts", zindex == triggers::HEARTS_Z),
        ("the menu's spinner", zindex == conductor::MENU_SPIN_Z),
        ("the tab panel", tab_panel::owns_zindex(zindex)),
    ];
    owners.iter().filter(|(_, owns)| *owns).map(|(who, _)| *who).collect()
}

#[cfg(test)]
mod lbterm_z_tests {
    /// One owner a z-layer at most, every number walked: a value added to an existing table (a
    /// fifth row of the objectives' CARD) isn't a new `_Z` constant a grep would find, and a test
    /// per feature checks only that feature's own numbers (lbterm/PITFALLS.md 300)
    #[test]
    fn every_z_layer_has_one_owner_at_most() {
        for z in i8::MIN..=i8::MAX {
            let owners = super::owners_of(z);
            assert!(owners.len() <= 1, "z-layer {} is claimed by {:?}", z, owners);
        }
    }

    /// `owners_of` names every arm of `lbterm_ranges` that gives z-layers to one visual, so an
    /// owner added there and not here fails
    #[test]
    fn owners_of_lists_every_arm_of_lbterm_ranges() {
        let source = include_str!("composited.rs");
        let start = source.find("fn lbterm_ranges(").expect("lbterm_ranges");
        let end = start + source[start..].find("Ordering::Equal =>").expect("its plain arms");
        let arms = source[start..end].matches("_ if ").count();
        assert_eq!(arms, 19, "lbterm_ranges has {} owner arms, owners_of names 19", arms);
    }
}

/// LBTerm: the visuals cleared to white and never drawn into, which only show through a clip:
/// the lights' swatch (render/lights.rs) and each menu's highlight (render/menu_fade.rs)
fn lbterm_cleared_white(target: Target<PaneId>) -> bool {
    matches!(target, Target::Swatch | Target::MenuLight(_))
}
