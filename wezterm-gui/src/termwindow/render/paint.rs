use crate::termwindow::{RenderFrame, TermWindowNotif};
use ::window::bitmaps::atlas::OutOfTextureSpace;
use ::window::WindowOps;
use anyhow::Context;
use lbterm_motion::Target;
use smol::Timer;
use std::time::{Duration, Instant};
use wezterm_font::ClearShapeCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowImage {
    Yes,
    Scale(usize),
    No,
}

impl crate::TermWindow {
    pub fn paint_impl(&mut self, frame: &mut RenderFrame) {
        self.num_frames += 1;
        // LBTerm: the frame counted for its window, and credited to the sites that asked for it
        // since its last (lbterm_extras::counters), which the budget judges (termwindow/budget.rs)
        self.lbterm_budget_frame_start();
        // If nothing on screen needs animating, then we can avoid
        // invalidating as frequently
        *self.has_animation.borrow_mut() = None;
        // LBTerm: this frame shows a composition's end (termwindow/echo.rs)
        self.lbterm_composition_ended = false;
        // LBTerm: whose panes this frame draws: the open tab's, or while a held pane's push
        // hasn't come the tab drawn last's (termwindow/frame_wait.rs)
        self.lbterm_frame_panes();
        // Start with the assumption that we should allow images to render
        self.allow_images = AllowImage::Yes;

        let start = Instant::now();
        // LBTerm: the process's first paint, as a mark of its start (lbterm_extras::start)
        static PAINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !PAINTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            lbterm_extras::start::mark("first_paint");
        }

        {
            let diff = start.duration_since(self.last_fps_check_time);
            if diff > Duration::from_secs(1) {
                let seconds = diff.as_secs_f32();
                self.fps = self.num_frames as f32 / seconds;
                self.num_frames = 0;
                self.last_fps_check_time = start;
            }
        }

        // LBTerm: links to InMail messages are shaped again when what waits changes (render/inbox.rs)
        self.lbterm_inbox_links_refresh();
        // LBTerm: an image's caption is shaped again when its file goes or comes back, and what the
        // panes drew of their thumbnails is gathered afresh (render/thumbnails.rs)
        self.lbterm_thumbnails_refresh();

        // LBTerm: the slow-frame log times building the frame and drawing it apart, by window,
        // and with the compositor presenting too (render/composited.rs; lbterm_extras::slow)
        let slow_pass =
            lbterm_extras::slow::paint(self.mux_window_id, lbterm_extras::slow::Part::Pass);
        'pass: for pass in 0.. {
            match self.paint_pass() {
                Ok(_) => match self.render_state.as_mut().unwrap().allocated_more_quads() {
                    Ok(allocated) => {
                        if !allocated {
                            break 'pass;
                        }
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                    }
                    Err(err) => {
                        log::error!("{:#}", err);
                        break 'pass;
                    }
                },
                Err(err) => {
                    if let Some(&OutOfTextureSpace {
                        size: Some(size),
                        current_size,
                    }) = err.root_cause().downcast_ref::<OutOfTextureSpace>()
                    {
                        // LBTerm: an atlas that hasn't yet seen a frame through holds nothing but
                        // what this frame draws, so clearing it frees nothing: it grows at once.
                        // Upstream cleared it first whatever, so a window's first frame (its atlas
                        // 128 px a side) and the first after a zoom drew everything again for
                        // nothing (lbterm/PERF.md, "What font setup costs")
                        let fresh = self
                            .render_state
                            .as_ref()
                            .map_or(false, |rs| rs.lbterm_atlas_frames == 0);
                        let result = if pass == 0 && !fresh {
                            // Let's try clearing out the atlas and trying again
                            // self.clear_texture_atlas()
                            log::trace!("recreate_texture_atlas");
                            crate::counters::fonts::ATLAS_CLEARED.one();
                            self.recreate_texture_atlas(Some(current_size))
                        } else {
                            log::trace!("grow texture atlas to {}", size);
                            crate::counters::fonts::ATLAS_GROWN.one();
                            self.recreate_texture_atlas(Some(size))
                        };
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();

                        if let Err(err) = result {
                            self.allow_images = match self.allow_images {
                                AllowImage::Yes => AllowImage::Scale(2),
                                AllowImage::Scale(2) => AllowImage::Scale(4),
                                AllowImage::Scale(4) => AllowImage::Scale(8),
                                AllowImage::Scale(8) => AllowImage::No,
                                AllowImage::No | _ => {
                                    log::error!(
                                        "Failed to {} texture: {}",
                                        if pass == 0 { "clear" } else { "resize" },
                                        err
                                    );
                                    break 'pass;
                                }
                            };

                            log::info!(
                                "Not enough texture space ({:#}); \
                                     will retry render with {:?}",
                                err,
                                self.allow_images,
                            );
                        }
                    } else if err.root_cause().downcast_ref::<ClearShapeCache>().is_some() {
                        self.invalidate_fancy_tab_bar();
                        self.invalidate_modal();
                        crate::counters::fonts::RESHAPES.one();
                        self.shape_generation += 1;
                        self.shape_cache.borrow_mut().clear();
                        self.line_to_ele_shape_cache.borrow_mut().clear();
                    } else {
                        log::error!("paint_pass failed: {:#}", err);
                        break 'pass;
                    }
                }
            }
        }
        drop(slow_pass);
        // LBTerm: the atlas has seen a frame through, so what's in it may be glyphs no longer drawn
        if let Some(rs) = self.render_state.as_mut() {
            rs.lbterm_atlas_frames += 1;
        }
        log::debug!("paint_impl before call_draw elapsed={:?}", start.elapsed());

        let slow_draw =
            lbterm_extras::slow::paint(self.mux_window_id, lbterm_extras::slow::Part::Draw);
        self.call_draw(frame).ok();
        drop(slow_draw);
        self.last_frame_duration = start.elapsed();
        // LBTerm: and the end of the process's first, drawn
        static DRAWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !DRAWN.swap(true, std::sync::atomic::Ordering::Relaxed) {
            lbterm_extras::start::mark("first_drawn");
        }
        log::debug!(
            "paint_impl elapsed={:?}, fps={}",
            self.last_frame_duration,
            self.fps
        );
        metrics::histogram!("gui.paint.impl").record(self.last_frame_duration);
        metrics::histogram!("gui.paint.impl.rate").record(1.);

        // If self.has_animation is some, then the last render detected
        // image attachments with multiple frames, so we also need to
        // invalidate the viewport when the next frame is due
        if self.focused.is_some() {
            if let Some(next_due) = *self.has_animation.borrow() {
                let prior = self.scheduled_animation.borrow_mut().take();
                match prior {
                    Some(prior) if prior <= next_due => {
                        // Already due before that time
                    }
                    _ => {
                        self.scheduled_animation.borrow_mut().replace(next_due);
                        // LBTerm: on purpose at rest, at the rate it comes (termwindow/budget.rs)
                        self.lbterm_budget_animation(next_due);
                        let window = self.window.clone().take().unwrap();
                        promise::spawn::spawn(async move {
                            lbterm_extras::counters::timer(Timer::at(next_due)).await;
                            let win = window.clone();
                            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                                tw.scheduled_animation.borrow_mut().take();
                                if tw.tab_bar.next_progress_frame_due().is_some() {
                                    tw.update_title_post_status();
                                }
                                win.invalidate();
                            })));
                        })
                        .detach();
                    }
                }
            }
        }
        // LBTerm: what this frame shows, to a test window's state file (termwindow/state.rs), and
        // the budget's actions it shows (termwindow/budget.rs)
        self.lbterm_state_frame();
        self.lbterm_budget_frame_end();
        // LBTerm: the pushes its frame read, to their servers (termwindow/panes_drawn.rs)
        self.lbterm_say_read();
    }

    pub fn paint_modal(&mut self) -> anyhow::Result<()> {
        if let Some(modal) = self.get_modal() {
            for computed in modal.computed_element(self)?.iter() {
                let mut ui_items = computed.ui_items();

                let gl_state = self.render_state.as_ref().unwrap();
                self.render_element(&computed, gl_state, None)?;

                self.ui_items.append(&mut ui_items);
            }
        }

        Ok(())
    }

    pub fn paint_pass(&mut self) -> anyhow::Result<()> {
        {
            let gl_state = self.render_state.as_ref().unwrap();
            for layer in gl_state.layers.borrow().iter() {
                layer.clear_quad_allocation();
            }
        }

        // Clear out UI item positions; we'll rebuild these as we render
        self.ui_items.clear();

        // LBTerm: the strip's words follow the active tab and its summary (render/strip.rs)
        self.lbterm_strip_refresh();

        // LBTerm: the fancy tab bar is laid out before the panes, whose cards
        // sit a gutter under it as it's drawn (render/card.rs); built when
        // it's painted, a pane painted first would see last frame's or none
        if self.show_tab_bar && self.config.use_fancy_tab_bar && self.fancy_tab_bar.is_none() {
            let palette = self.palette().clone();
            let tab_bar = self.build_fancy_tab_bar(&palette)?;
            self.fancy_tab_bar.replace(tab_bar);
        }

        // LBTerm: the open tab's panes, or the ones kept in their place (termwindow/frame_wait.rs)
        let panes = self.lbterm_panes_to_paint();
        let focused = self.focused.is_some();
        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;

        let start = Instant::now();
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state
            .layer_for_zindex(0)
            .context("layer_for_zindex(0)")?;
        let mut layers = layer.quad_allocator();
        log::trace!("quad map elapsed {:?}", start.elapsed());
        metrics::histogram!("quad.map").record(start.elapsed());
        // LBTerm: mark which visual each quad goes to (lbterm/COMPOSITOR.md),
        // starting with the window background
        self.lbterm_spans.reset(layers.quad_counts());

        let mut paint_terminal_background = false;

        // Render the full window background
        match (self.window_background.is_empty(), self.allow_images) {
            (false, AllowImage::Yes | AllowImage::Scale(_)) => {
                let bg_color = self.palette().background.to_linear();

                let top = panes
                    .iter()
                    .find(|p| p.is_active)
                    .map(|p| match self.get_viewport(p.pane.pane_id()) {
                        Some(top) => top,
                        None => p.pane.get_dimensions().physical_top,
                    })
                    .unwrap_or(0);

                let loaded_any = self
                    .render_backgrounds(bg_color, top)
                    .context("render_backgrounds")?;

                if !loaded_any {
                    // Either there was a problem loading the background(s)
                    // or they haven't finished loading yet.
                    // Use the regular terminal background until that changes.
                    paint_terminal_background = true;
                }
            }
            _ if window_is_transparent => {
                // Avoid doubling up the background color: the panes
                // will render out through the padding so there
                // should be no gaps that need filling in
            }
            _ => {
                paint_terminal_background = true;
            }
        }

        if paint_terminal_background {
            // Regular window background color
            let background = if panes.len() == 1 {
                // If we're the only pane, use the pane's palette
                // to draw the padding background
                panes[0].pane.palette().background
            } else {
                self.palette().background
            }
            .to_linear()
            .mul_alpha(self.config.window_background_opacity);

            self.filled_rectangle(
                &mut layers,
                0,
                euclid::rect(
                    0.,
                    0.,
                    self.dimensions.pixel_width as f32,
                    self.dimensions.pixel_height as f32,
                ),
                background,
            )
            .context("filled_rectangle for window background")?;
        }

        // LBTerm: panes kept in the open tab's place while a held pane's push hasn't come are
        // only drawn: the keyboard, the IME's caret and the server's focus stay with the open
        // tab's pane, which `activate_tab` told already (termwindow/frame_wait.rs)
        let kept = self.lbterm_panes_kept();
        for pos in panes {
            if pos.is_active && !kept {
                // LBTerm: the Conductor's view over the panes hands the IME its own caret, and
                // only when it moves (render/conductor_view.rs)
                if !self.lbterm_view_is_open() {
                    self.update_text_cursor(&pos);
                }
                if focused {
                    pos.pane.advise_focus();
                    mux::Mux::get().record_focus_for_current_identity(pos.pane.pane_id());
                }
            }
            self.lbterm_spans
                .switch(Target::Pane(pos.pane.pane_id()), layers.quad_counts());
            // LBTerm: the push of it this frame reads, said at the paint's end (panes_drawn.rs);
            // noted first, so a pass that fails here leaves no push unsaid
            self.lbterm_frame_read(&pos.pane);
            self.paint_pane(&pos, &mut layers).context("paint_pane")?;
        }
        // LBTerm: what the panes' lines showed that's new plays its effect (render/triggers.rs)
        self.lbterm_triggers_fire();

        self.lbterm_spans
            .switch(Target::Splits, layers.quad_counts());
        // LBTerm: those of the panes drawn (termwindow/frame_wait.rs)
        if let Some(pane) = self.lbterm_pane_to_paint() {
            let splits = self.lbterm_splits_to_paint();
            for split in &splits {
                self.paint_split(&mut layers, split, &pane)
                    .context("paint_split")?;
            }
        }

        self.lbterm_spans.switch(Target::Over, layers.quad_counts());
        if self.show_tab_bar {
            self.paint_tab_bar(&mut layers).context("paint_tab_bar")?;
        }

        self.paint_window_borders(&mut layers)
            .context("paint_window_borders")?;
        drop(layers);
        // LBTerm: what the panes' outlines are cut from, with the compositor (render/card.rs)
        self.lbterm_cards = self
            .paint_lbterm_card_templates()
            .context("paint_lbterm_card_templates")?;
        // LBTerm: in z-layers of their own, over all but modals
        self.paint_lbterm_hearts().context("paint_lbterm_hearts")?;
        self.paint_lbterm_strip_card()
            .context("paint_lbterm_strip_card")?;
        self.paint_lbterm_readout().context("paint_lbterm_readout")?;
        self.paint_lbterm_strip_swap()
            .context("paint_lbterm_strip_swap")?;
        self.paint_lbterm_prompt_mark()
            .context("paint_lbterm_prompt_mark")?;
        self.paint_lbterm_questions()
            .context("paint_lbterm_questions")?;
        // LBTerm: a voice draft's card over its pane (render/draft.rs)
        self.paint_lbterm_drafts().context("paint_lbterm_drafts")?;
        // LBTerm: the Conductor's view over the panes, under the panel (render/conductor_view.rs)
        self.paint_lbterm_conductor_view()
            .context("paint_lbterm_conductor_view")?;
        self.paint_tab_panel().context("paint_tab_panel")?;
        // LBTerm: after the panel, whose rows its marks line up with (render/objectives.rs)
        self.lbterm_sides_begin();
        self.paint_lbterm_objectives()
            .context("paint_lbterm_objectives")?;
        // LBTerm: or the context bar in its room, which wins over it (render/context_bar.rs)
        self.paint_lbterm_context_bar()
            .context("paint_lbterm_context_bar")?;
        // LBTerm: the projects card, hanging from its pad on the strip's second row, which the tab
        // bar has laid out by now (render/projects.rs)
        self.paint_lbterm_projects()
            .context("paint_lbterm_projects")?;
        // LBTerm: the cost pane under the strip, which the context pill's press opens
        // (render/cost.rs)
        self.paint_lbterm_cost().context("paint_lbterm_cost")?;
        // LBTerm: the scroll strip left of it, and its card (render/scroll_strip.rs)
        self.paint_lbterm_scroll_strip()
            .context("paint_lbterm_scroll_strip")?;
        // LBTerm: what the two strips drew, and as a tab is left how they were (render/sides.rs)
        self.paint_lbterm_sides().context("paint_lbterm_sides")?;
        // LBTerm: what a tab is running, beside its row on the open panel (render/tasks_card.rs)
        self.paint_lbterm_tasks_card()
            .context("paint_lbterm_tasks_card")?;
        // LBTerm: the new-tab pad's look under the pointer, its picker and its + menu
        // (render/new_tab.rs), after the tab panel, whose layout places them
        // LBTerm: no menu drawn yet in this paint: those drawn below are open, and any drawn in
        // the last and not in this one fades out (render/menu_fade.rs)
        self.lbterm_menu_fade_begin();
        self.paint_lbterm_new_tab().context("paint_lbterm_new_tab")?;
        self.paint_modal().context("paint_modal")?;
        // LBTerm: over the panel too (render/tab_menu.rs)
        self.paint_lbterm_tab_menu().context("paint_lbterm_tab_menu")?;
        // LBTerm: a tab being right-dragged onto another, and its cards (render/link_drag.rs)
        self.paint_lbterm_link_drag().context("paint_lbterm_link_drag")?;
        // LBTerm: the list of prompts, beside it (render/prompt.rs)
        self.paint_lbterm_prompt_list()
            .context("paint_lbterm_prompt_list")?;
        // LBTerm: and the Conductor's menu (render/conductor.rs)
        self.paint_lbterm_conductor_menu()
            .context("paint_lbterm_conductor_menu")?;
        // LBTerm: and the InMail button's list (render/inbox.rs)
        self.paint_lbterm_inbox_list()
            .context("paint_lbterm_inbox_list")?;
        self.paint_lbterm_agenda_list()
            .context("paint_lbterm_agenda_list")?;

        Ok(())
    }
}
