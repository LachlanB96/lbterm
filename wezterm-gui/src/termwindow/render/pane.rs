use crate::quad::TripleLayerQuadAllocator;
use crate::selection::SelectionRange;
use crate::termwindow::box_model::*;
use crate::termwindow::render::line_key::{CursorRow, LineFrame};
use crate::termwindow::render::{
    LineQuadCacheValue, LineToEleShapeCacheKey, RenderScreenLineParams,
};
use crate::termwindow::{ScrollHit, UIItem, UIItemType};
use ::window::bitmaps::TextureRect;
use ::window::DeadKeyStatus;
use anyhow::Context;
use config::VisualBellTarget;
use mux::pane::{PaneId, WithPaneLines};
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::PositionedPane;
use std::time::Instant;
use wezterm_dynamic::Value;
use wezterm_term::color::{ColorAttribute, ColorPalette};
use wezterm_term::{Line, StableRowIndex};
use window::color::LinearRgba;

impl crate::TermWindow {
    fn paint_pane_box_model(&mut self, pos: &PositionedPane) -> anyhow::Result<()> {
        let computed = self.build_pane(pos)?;
        let mut ui_items = computed.ui_items();
        self.ui_items.append(&mut ui_items);
        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)
    }

    pub fn paint_pane(
        &mut self,
        pos: &PositionedPane,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        if self.config.use_box_model_render {
            return self.paint_pane_box_model(pos);
        }

        self.check_for_dirty_lines_and_invalidate_selection(&pos.pane);
        /*
        let zone = {
            let dims = pos.pane.get_dimensions();
            let position = self
                .get_viewport(pos.pane.pane_id())
                .unwrap_or(dims.physical_top);

            let zones = self.get_semantic_zones(&pos.pane);
            let idx = match zones.binary_search_by(|zone| zone.start_y.cmp(&position)) {
                Ok(idx) | Err(idx) => idx,
            };
            let idx = ((idx as isize) - 1).max(0) as usize;
            zones.get(idx).cloned()
        };
        */

        let global_cursor_fg = self.palette().cursor_fg;
        let global_cursor_bg = self.palette().cursor_bg;
        let config = self.config.clone();
        let palette = pos.pane.palette();

        let (padding_left, padding_top) = self.padding_left_top();

        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height()
                .context("tab_bar_pixel_height")?
        } else {
            0.
        };
        let (top_bar_height, _bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        let cursor = pos.pane.get_cursor_position();
        if pos.is_active {
            self.prev_cursor.update(&cursor);
        }

        let pane_id = pos.pane.pane_id();
        let current_viewport = self.get_viewport(pane_id);
        let dims = pos.pane.get_dimensions();
        // LBTerm: the pane as this frame draws it, for the state file and a key's echo
        self.lbterm_state_drawn(&pos.pane, &cursor, &dims);

        let gl_state = self.render_state.as_ref().unwrap();

        let cursor_border_color = palette.cursor_border.to_linear();
        let foreground = palette.foreground.to_linear();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();

        let window_is_transparent =
            !self.window_background.is_empty() || config.window_background_opacity != 1.0;

        let default_bg = palette
            .resolve_bg(ColorAttribute::Default)
            .to_linear()
            .mul_alpha(if window_is_transparent {
                0.
            } else {
                config.text_background_opacity
            });

        // LBTerm: the pane is a card with a gutter round it rather than a
        // background out to the splits' middles and the window's edges
        // (render/card.rs). With the compositor, half a gutter round the card
        // is the pane's visual
        let card = self.lbterm_card(pos).context("lbterm_card")?;
        let visual = self.lbterm_shape().visual(card);
        self.lbterm_spans.set_pane_rect(
            pane_id,
            lbterm_motion::PixelRect::snap(visual.x, visual.y, visual.w, visual.h),
        );
        if pos.is_active {
            self.lbterm_spans.set_active(pane_id);
        }
        // With the compositor a pane without the keyboard is its visual at
        // 85%, so it draws undimmed; and the outline, the active pane's
        // alone, is the one that glides between them
        let composited = self.lbterm_composited();
        let hsv = if pos.is_active || composited {
            None
        } else {
            Some(config.inactive_pane_hsb)
        };
        let outline = (pos.is_active && !composited).then(|| {
            let (r, g, b, _) = palette.background.to_srgb_u8();
            lbterm_panel::palette::card_outline(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
        });

        // Per-pane, palette-specified background
        let fill = self.window_background.is_empty().then(|| {
            palette
                .background
                .to_linear()
                .mul_alpha(config.window_background_opacity)
        });
        self.lbterm_paint_card(layers, card, fill, hsv, outline)
            .context("lbterm_paint_card")?;

        {
            // If the bell is ringing, we draw another background layer over the
            // top of this in the configured bell color
            if let Some(intensity) = self.get_intensity_if_bell_target_ringing(
                &pos.pane,
                &config,
                VisualBellTarget::BackgroundColor,
            ) {
                // target background color
                let LinearRgba(r, g, b, _) = config
                    .resolved_palette
                    .visual_bell
                    .as_deref()
                    .unwrap_or(&palette.foreground)
                    .to_linear();

                let background = if window_is_transparent {
                    // for transparent windows, we fade in the target color
                    // by adjusting its alpha
                    LinearRgba::with_components(r, g, b, intensity)
                } else {
                    // otherwise We'll interpolate between the background color
                    // and the target color
                    let (r1, g1, b1, a) = palette
                        .background
                        .to_linear()
                        .mul_alpha(config.window_background_opacity)
                        .tuple();
                    LinearRgba::with_components(
                        r1 + (r - r1) * intensity,
                        g1 + (g - g1) * intensity,
                        b1 + (b - b1) * intensity,
                        a,
                    )
                };
                log::trace!("bell color is {:?}", background);

                // LBTerm: the card, not the whole rectangle (render/card.rs)
                self.lbterm_paint_card(layers, card, Some(background), hsv, None)
                    .context("lbterm_paint_card")?;
            }
        }

        // TODO: we only have a single scrollbar in a single position.
        // We only update it for the active pane, but we should probably
        // do a per-pane scrollbar.  That will require more extensive
        // changes to ScrollHit, mouse positioning, PositionedPane
        // and tab size calculation.
        // LBTerm: none while a program has the whole screen, since the
        // viewport can't follow where it's scrolled (render/card.rs)
        if pos.is_active && self.show_scroll_bar && self.lbterm_scroll_bar_shows(&pos.pane) {
            // LBTerm: the track runs down the active pane's card, clear of
            // its rounded corners (render/card.rs)
            let (thumb_y_offset, track_height) =
                self.lbterm_scroll_track(pos).context("lbterm_scroll_track")?;

            let min_height = self.min_scroll_bar_height();

            let info = ScrollHit::thumb(
                &*pos.pane,
                current_viewport,
                track_height,
                min_height as usize,
            );
            let abs_thumb_top = thumb_y_offset + info.top;
            let thumb_size = info.height;
            let color = palette.scrollbar_thumb.to_linear();

            // Adjust the scrollbar thumb position
            let config = &self.config;
            let padding = self.effective_right_padding(&config) as f32;

            let thumb_x = self.dimensions.pixel_width - padding as usize - border.right.get();

            // Register the scroll bar location
            self.ui_items.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: thumb_y_offset,
                height: info.top,
                item_type: UIItemType::AboveScrollThumb,
            });
            self.ui_items.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: abs_thumb_top,
                height: thumb_size,
                item_type: UIItemType::ScrollThumb,
            });
            self.ui_items.push(UIItem {
                x: thumb_x,
                width: padding as usize,
                y: abs_thumb_top + thumb_size,
                height: self
                    .dimensions
                    .pixel_height
                    .saturating_sub(abs_thumb_top + thumb_size),
                item_type: UIItemType::BelowScrollThumb,
            });

            // LBTerm: the thumb sits in the window's padding, not the pane,
            // so it goes over the panes; it's drawn inside the card's
            // outline, rounded (render/card.rs)
            self.lbterm_spans
                .switch(lbterm_motion::Target::Over, layers.quad_counts());
            let (bar_x, bar_width) = self.lbterm_scroll_column(thumb_x as f32)?;
            self.lbterm_fill_rounded(
                layers,
                lbterm_panel::Area::new(bar_x, abs_thumb_top as f32, bar_width, thumb_size as f32),
                bar_width / 2.0,
                color,
                None,
            )
            .context("lbterm_fill_rounded")?;
            // LBTerm: a tick for each shell prompt, over the thumb (render/marks.rs)
            self.paint_lbterm_marks(
                layers,
                &pos.pane,
                bar_x,
                bar_width,
                thumb_y_offset,
                track_height,
            )
            .context("paint_lbterm_marks")?;
            self.lbterm_spans
                .switch(lbterm_motion::Target::Pane(pane_id), layers.quad_counts());
        }

        let (selrange, rectangular) = {
            let sel = self.selection(pos.pane.pane_id());
            (sel.range.clone(), sel.rectangular)
        };

        let start = Instant::now();
        let selection_fg = palette.selection_fg.to_linear();
        let selection_bg = palette.selection_bg.to_linear();
        let cursor_fg = palette.cursor_fg.to_linear();
        let cursor_bg = palette.cursor_bg.to_linear();
        let cursor_is_default_color =
            palette.cursor_fg == global_cursor_fg && palette.cursor_bg == global_cursor_bg;

        {
            let stable_range = match current_viewport {
                Some(top) => top..top + dims.viewport_rows as StableRowIndex,
                None => dims.physical_top..dims.physical_top + dims.viewport_rows as StableRowIndex,
            };

            pos.pane
                .apply_hyperlinks(stable_range.clone(), &self.config.hyperlink_rules);
            // LBTerm: the rows the pane draws, so the triggers know which of their lines have gone
            // (render/triggers.rs)
            self.lbterm_triggers_view(pane_id, stable_range.clone(), &palette);

            struct LineRender<'a, 'b> {
                term_window: &'a mut crate::TermWindow,
                selrange: Option<SelectionRange>,
                rectangular: bool,
                dims: RenderableDimensions,
                top_pixel_y: f32,
                left_pixel_x: f32,
                pos: &'a PositionedPane,
                pane_id: PaneId,
                cursor: &'a StableCursorPosition,
                palette: &'a ColorPalette,
                default_bg: LinearRgba,
                cursor_border_color: LinearRgba,
                selection_fg: LinearRgba,
                selection_bg: LinearRgba,
                cursor_fg: LinearRgba,
                cursor_bg: LinearRgba,
                foreground: LinearRgba,
                cursor_is_default_color: bool,
                white_space: TextureRect,
                filled_box: TextureRect,
                window_is_transparent: bool,
                layers: &'a mut TripleLayerQuadAllocator<'b>,
                error: Option<anyhow::Error>,
                // LBTerm: the cells linking to a picture row, drawn over after the lines
                // (render/thumbnails.rs)
                thumbnail_runs: Vec<lbterm_extras::thumbnails::Run>,
            }

            let left_pixel_x = padding_left
                + border.left.get() as f32
                + (pos.left as f32 * self.render_metrics.cell_size.width as f32);

            // LBTerm: a pane of the GUI's own has its rows copied out, its terminal held only for
            // that, and laid out after, so its parser goes on taking in output meanwhile
            // (render/pane_lines.rs)
            let mut copied = self.lbterm_copy_lines(&pos.pane, stable_range.clone());

            let mut render = LineRender {
                term_window: self,
                selrange,
                rectangular,
                dims,
                top_pixel_y,
                left_pixel_x,
                pos,
                pane_id,
                cursor: &cursor,
                palette: &palette,
                cursor_border_color,
                selection_fg,
                selection_bg,
                cursor_fg,
                default_bg,
                cursor_bg,
                foreground,
                cursor_is_default_color,
                white_space,
                filled_box,
                window_is_transparent,
                layers,
                error: None,
                thumbnail_runs: vec![],
            };

            impl<'a, 'b> LineRender<'a, 'b> {
                fn render_line(
                    &mut self,
                    stable_top: StableRowIndex,
                    line_idx: usize,
                    line: &&mut Line,
                ) -> anyhow::Result<()> {
                    let stable_row = stable_top + line_idx as StableRowIndex;
                    // LBTerm: what triggers colour is drawn from a recoloured copy, kept while the
                    // line is the same, and what's new is noted for its effect (render/triggers.rs)
                    let styled = self.term_window.lbterm_trigger_line(
                        self.pane_id,
                        stable_row,
                        line,
                        self.palette,
                    );
                    let line: &Line = styled.as_deref().unwrap_or(&**line);
                    let (shape_hash, by_state) = self.term_window.lbterm_shape_hash_for_line(line);
                    // LBTerm: noted before the line's cached quads can return early, and only for
                    // a line holding a link drawn by state, as a picture's are: its cells were
                    // looked through for one on every frame (render/thumbnails.rs)
                    if by_state {
                        crate::termwindow::render::thumbnails::lbterm_thumbnail_runs(
                            line_idx,
                            line,
                            &mut self.thumbnail_runs,
                        );
                    }

                    // LBTerm: the key is what the line shows, the cursor and the selected columns
                    // as they are on it included, and `row_y` its row, in pixels from the
                    // window's top: its quads are laid out at 0 and moved there as they're copied
                    // in, so a line that only moved keeps its quads (render/line_key.rs)
                    let (quad_key, row_y) = LineFrame {
                        pane_id: self.pane_id,
                        config_generation: self.term_window.config.generation(),
                        shape_generation: self.term_window.shape_generation,
                        quad_generation: self.term_window.quad_generation,
                        left_pixel_x: self.left_pixel_x,
                        top_pixel_y: self.top_pixel_y,
                        pos_top: self.pos.top,
                        cell_height: self.term_window.render_metrics.cell_size.height as f32,
                        cols: self.dims.cols,
                        reverse_video: self.dims.reverse_video,
                        cursor: self.cursor,
                        selection: self.selrange,
                        rectangular: self.rectangular,
                        is_active: self.pos.is_active,
                        composited: self.term_window.lbterm_composited(),
                        focused: self.term_window.focused.is_some(),
                    }
                    .line(line_idx, stable_row, shape_hash, || CursorRow {
                        dead_key_or_leader: self.term_window.dead_key_status != DeadKeyStatus::None
                            || self.term_window.leader_is_active(),
                        cursor_fg: self.cursor_fg,
                        cursor_bg: self.cursor_bg,
                        cursor_border_color: self.cursor_border_color,
                        cursor_is_default_color: self.cursor_is_default_color,
                        composing: match (self.pos.is_active, &self.term_window.dead_key_status) {
                            (true, DeadKeyStatus::Composing(composing)) => {
                                Some(composing.to_string())
                            }
                            _ => None,
                        },
                        password_input: if self.term_window.config.detect_password_input {
                            match self.pos.pane.get_metadata() {
                                Value::Object(obj) => {
                                    match obj.get(&Value::String("password_input".to_string())) {
                                        Some(Value::Bool(b)) => *b,
                                        _ => false,
                                    }
                                }
                                _ => false,
                            }
                        } else {
                            false
                        },
                    });
                    let selrange = quad_key.selection.clone();
                    let password_input = quad_key.password_input;

                    if let Some(cached_quad) =
                        self.term_window.line_quad_cache.borrow_mut().get(&quad_key)
                    {
                        let expired = cached_quad
                            .expires
                            .map(|i| Instant::now() >= i)
                            .unwrap_or(false);
                        // LBTerm: only when which of its own links is under the pointer changes,
                        // where any frame with the pointer on no link laid out every line holding
                        // one again (lbterm_extras::link_looks::hover_changed)
                        let hover_changed = cached_quad.invalidate_on_hover_change
                            && lbterm_extras::link_looks::hover_changed(
                                &cached_quad.lbterm_links,
                                cached_quad.current_highlight.as_ref(),
                                self.term_window.current_highlight.as_ref(),
                            );
                        if !expired && !hover_changed {
                            cached_quad
                                .layers
                                .lbterm_apply_moved(self.layers, row_y)
                                .context("cached_quad.layers.lbterm_apply_moved")?;
                            self.term_window.update_next_frame_time(cached_quad.expires);
                            return Ok(());
                        }
                    }

                    // LBTerm: a line laid out again, counted (lbterm_extras::work)
                    lbterm_extras::work::WORK.line();
                    // LBTerm: laid out where the window's lines are laid out, its room kept from
                    // line to line, then kept at its own size (quad.rs, `lbterm_exact`); taken out
                    // while it's drawn into, so a layout that fails leaves an empty one behind
                    let mut buf = self.term_window.lbterm_line_quads.take();
                    buf.lbterm_clear();
                    let next_due = self.term_window.has_animation.borrow_mut().take();

                    let shape_key = LineToEleShapeCacheKey {
                        shape_hash,
                        shape_generation: quad_key.shape_generation,
                        composing: if self.cursor.y == stable_row && self.pos.is_active {
                            if let DeadKeyStatus::Composing(composing) =
                                &self.term_window.dead_key_status
                            {
                                Some((self.cursor.x, composing.to_string()))
                            } else {
                                None
                            }
                        } else {
                            None
                        },
                    };

                    let render_result = self
                        .term_window
                        .render_screen_line(
                            RenderScreenLineParams {
                                // LBTerm: at row 0, the window's top; moved to `row_y` as
                                // it's copied in
                                top_pixel_y: 0.0,
                                left_pixel_x: self.left_pixel_x,
                                pixel_width: self.dims.cols as f32
                                    * self.term_window.render_metrics.cell_size.width as f32,
                                stable_line_idx: Some(stable_row),
                                line: &line,
                                selection: selrange.clone(),
                                cursor: &self.cursor,
                                palette: &self.palette,
                                dims: &self.dims,
                                config: &self.term_window.config,
                                cursor_border_color: self.cursor_border_color,
                                foreground: self.foreground,
                                is_active: self.pos.is_active,
                                pane: Some(&self.pos.pane),
                                selection_fg: self.selection_fg,
                                selection_bg: self.selection_bg,
                                cursor_fg: self.cursor_fg,
                                cursor_bg: self.cursor_bg,
                                cursor_is_default_color: self.cursor_is_default_color,
                                white_space: self.white_space,
                                filled_box: self.filled_box,
                                window_is_transparent: self.window_is_transparent,
                                default_bg: self.default_bg,
                                font: None,
                                style: None,
                                use_pixel_positioning: self
                                    .term_window
                                    .config
                                    .experimental_pixel_positioning,
                                render_metrics: self.term_window.render_metrics,
                                shape_key: Some(shape_key),
                                password_input,
                            },
                            &mut TripleLayerQuadAllocator::Heap(&mut buf),
                        )
                        .context("render_screen_line")?;

                    let expires = self.term_window.has_animation.borrow().as_ref().cloned();
                    self.term_window.update_next_frame_time(next_due);

                    buf.lbterm_apply_moved(self.layers, row_y)
                        .context("HeapQuadAllocator::lbterm_apply_moved")?;
                    let layers = buf.lbterm_exact();
                    self.term_window.lbterm_line_quads.replace(buf);

                    let quad_value = LineQuadCacheValue {
                        layers,
                        expires,
                        invalidate_on_hover_change: render_result.invalidate_on_hover_change,
                        current_highlight: if render_result.invalidate_on_hover_change {
                            self.term_window.current_highlight.clone()
                        } else {
                            None
                        },
                        lbterm_links: render_result.lbterm_links,
                    };

                    self.term_window
                        .line_quad_cache
                        .borrow_mut()
                        .put(quad_key, quad_value);

                    Ok(())
                }
            }

            impl<'a, 'b> WithPaneLines for LineRender<'a, 'b> {
                fn with_lines_mut(&mut self, stable_top: StableRowIndex, lines: &mut [&mut Line]) {
                    for (line_idx, line) in lines.iter().enumerate() {
                        if let Err(err) = self.render_line(stable_top, line_idx, line) {
                            self.error.replace(err);
                            return;
                        }
                    }
                }
            }

            match copied.as_mut() {
                Some(copied) => copied.lay_out(&mut render),
                None => pos.pane.with_lines_mut(stable_range.clone(), &mut render),
            }
            let error = render.error.take();
            let thumbnail_runs = std::mem::take(&mut render.thumbnail_runs);
            // LBTerm: the rows kept, so the next frame copies only those that change: a pass
            // that fails (a shape cache to clear, an atlas to grow) is done again at once
            if let Some(copied) = copied {
                self.lbterm_keep_lines(pane_id, copied);
            }
            if let Some(error) = error {
                return Err(error).context("error while calling with_lines_mut");
            }
            // LBTerm: the images the pane's lines link to, drawn into the rows they cover
            // (render/thumbnails.rs)
            self.lbterm_paint_thumbnails(layers, pos, &thumbnail_runs, top_pixel_y, left_pixel_x, hsv)
                .context("lbterm_paint_thumbnails")?;
        }

        /*
        if let Some(zone) = zone {
            // TODO: render a thingy to jump to prior prompt
        }
        */
        metrics::histogram!("paint_pane.lines").record(start.elapsed());
        log::trace!("lines elapsed {:?}", start.elapsed());

        Ok(())
    }

    pub fn build_pane(&mut self, pos: &PositionedPane) -> anyhow::Result<ComputedElement> {
        // First compute the bounds for the pane background

        let cell_width = self.render_metrics.cell_size.width as f32;
        let cell_height = self.render_metrics.cell_size.height as f32;
        let (padding_left, padding_top) = self.padding_left_top();
        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height()?
        } else {
            0.
        };
        let (top_bar_height, _bottom_bar_height) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar_height)
        } else {
            (tab_bar_height, 0.0)
        };

        let border = self.get_os_border();
        let top_pixel_y = top_bar_height + padding_top + border.top.get() as f32;

        // We want to fill out to the edges of the splits
        let (x, width_delta) = if pos.left == 0 {
            (
                0.,
                padding_left + border.left.get() as f32 + (cell_width / 2.0),
            )
        } else {
            (
                padding_left + border.left.get() as f32 - (cell_width / 2.0)
                    + (pos.left as f32 * cell_width),
                cell_width,
            )
        };

        let (y, height_delta) = if pos.top == 0 {
            (
                (top_pixel_y - padding_top),
                padding_top + (cell_height / 2.0),
            )
        } else {
            (
                top_pixel_y + (pos.top as f32 * cell_height) - (cell_height / 2.0),
                cell_height,
            )
        };

        let background_rect = euclid::rect(
            x,
            y,
            // Go all the way to the right edge if we're right-most
            if pos.left + pos.width >= self.terminal_size.cols as usize {
                self.dimensions.pixel_width as f32 - x
            } else {
                (pos.width as f32 * cell_width) + width_delta
            },
            // Go all the way to the bottom if we're bottom-most
            if pos.top + pos.height >= self.terminal_size.rows as usize {
                self.dimensions.pixel_height as f32 - y
            } else {
                (pos.height as f32 * cell_height) + height_delta as f32
            },
        );

        // Bounds for the terminal cells
        let content_rect = euclid::rect(
            padding_left + border.left.get() as f32 - (cell_width / 2.0)
                + (pos.left as f32 * cell_width),
            top_pixel_y + (pos.top as f32 * cell_height) - (cell_height / 2.0),
            pos.width as f32 * cell_width,
            pos.height as f32 * cell_height,
        );

        let palette = pos.pane.palette();

        // TODO: visual bell background layer
        // TODO: scrollbar

        Ok(ComputedElement {
            item_type: None,
            zindex: 0,
            bounds: background_rect,
            border: PixelDimension::default(),
            border_rect: background_rect,
            border_corners: None,
            colors: ElementColors {
                border: BorderColor::default(),
                bg: if self.window_background.is_empty() {
                    palette
                        .background
                        .to_linear()
                        .mul_alpha(self.config.window_background_opacity)
                        .into()
                } else {
                    InheritableColor::Inherited
                },
                text: InheritableColor::Inherited,
            },
            hover_colors: None,
            padding: background_rect,
            content_rect,
            baseline: 1.0,
            content: ComputedElementContent::Children(vec![]),
        })
    }
}
