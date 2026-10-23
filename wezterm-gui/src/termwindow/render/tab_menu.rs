//! LBTerm: the tab menu (lbterm/TAB-MOUSE.md). A right-click on a tab, in the tab panel or the
//! tab bar, opens a card in the panel's style at the pointer: dark, ringed in the panel's red
//! light, with the panel's font. Rename tab (typed in place), Colour (a row of swatches), Duplicate
//! tab, Move to new window, Close other tabs, Close tab. The mouse or the keyboard (arrows, Enter,
//! Esc) picks; a click outside or Esc closes it.
//!
//! It's drawn in z-layers 60-69, which the compositor gives a visual of its own over the panel
//! (`top`), and takes every mouse event and key while it's open.

use crate::quad::{QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::box_model::{BorderColor, Element, ElementColors, ElementContent, LayoutContext};
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::DimensionContext;
use lbterm_extras::menu::{self, Item, Layout};
use lbterm_extras::tabs::{Rect, COLOURS};
use lbterm_panel::palette;
use mux::pane::CachePolicy;
use mux::tab::TabId;
use mux::Mux;
use wezterm_term::{KeyCode, KeyModifiers};
use window::color::LinearRgba;

const GLOW_Z: i8 = 60;
const CARD_Z: i8 = 61;
const TEXT_Z: i8 = 62;

/// Whether a z-layer is the menu's (the compositor's `top` visual)
pub fn owns_zindex(zindex: i8) -> bool {
    (60..70).contains(&zindex)
}

/// The menu while it's open
pub struct Open {
    tab: TabId,
    /// Where it was opened, in the window
    at: (f64, f64),
    /// Under the pointer, or where the arrow keys put it
    focus: Option<Item>,
    /// The new name being typed, after Rename tab
    renaming: Option<menu::Name>,
}

#[derive(Default)]
pub struct TabMenuState {
    open: Option<Open>,
}

fn srgb(rgb: u32, alpha: f64) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn rect_f(r: Rect) -> window::RectF {
    euclid::rect(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
}

fn grow(r: Rect, by: f64) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

impl TermWindow {
    fn lbterm_menu_scale(&self) -> f64 {
        self.dimensions.dpi as f64 / 96.0
    }

    fn lbterm_menu_layout(&self, at: (f64, f64)) -> Layout {
        menu::layout(
            at,
            (
                self.dimensions.pixel_width as f64,
                self.dimensions.pixel_height as f64,
            ),
            self.lbterm_menu_scale(),
        )
    }

    pub(crate) fn lbterm_menu_is_open(&self) -> bool {
        self.lbterm_menu.open.is_some()
    }

    /// The open tab menu's card, for the tab panel behind it (render/tab_panel.rs)
    pub(crate) fn lbterm_tab_menu_card(&self) -> Option<Rect> {
        let open = self.lbterm_menu.open.as_ref()?;
        Some(self.lbterm_menu_layout(open.at).card)
    }

    /// Open the menu for `tab` at a point in the window
    pub(crate) fn lbterm_open_tab_menu(&mut self, tab: TabId, at: (f64, f64)) {
        self.lbterm_menu.open = Some(Open {
            tab,
            at,
            focus: None,
            renaming: None,
        });
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub(crate) fn lbterm_close_tab_menu(&mut self) {
        if self.lbterm_menu.open.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// While the menu's open it takes every mouse event: true when it did
    pub(crate) fn lbterm_tab_menu_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(open) = self.lbterm_menu.open.as_ref() else {
            return false;
        };
        let layout = self.lbterm_menu_layout(open.at);
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let hit = layout.hit(x, y);
        match event.kind {
            WMEK::Move => {
                let focus = hit.or(if open.renaming.is_some() { Some(Item::Rename) } else { None });
                if focus != open.focus {
                    if let Some(open) = self.lbterm_menu.open.as_mut() {
                        open.focus = focus;
                    }
                    context.invalidate();
                }
                context.set_cursor(Some(if hit.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                }));
            }
            WMEK::Press(MousePress::Left) => match hit {
                Some(item) => self.lbterm_menu_pick(item),
                None if layout.card.contains(x, y) => {}
                None => self.lbterm_close_tab_menu(),
            },
            WMEK::Press(_) => {
                if !layout.card.contains(x, y) {
                    self.lbterm_close_tab_menu();
                }
            }
            _ => {}
        }
        true
    }

    /// While the menu's open it takes every key: true when it did
    pub(crate) fn lbterm_tab_menu_key(&mut self, key: &KeyCode, mods: KeyModifiers) -> bool {
        let Some(open) = self.lbterm_menu.open.as_mut() else {
            return false;
        };
        if let Some(name) = open.renaming.as_mut() {
            match key {
                KeyCode::Escape => open.renaming = None,
                KeyCode::Enter => {
                    // Untouched, it stays as it was (a pane's title isn't pinned as the tab's);
                    // empty gives the tab back the title its program sets
                    let changed = !name.selected;
                    let name = name.text.trim().to_string();
                    let tab = open.tab;
                    self.lbterm_close_tab_menu();
                    if let Some(tab) = Mux::get().get_tab(tab).filter(|_| changed) {
                        tab.set_title(&name);
                        // LBTerm: its pad glints with the new name (render/tab_panel.rs)
                        self.lbterm_panel_glint_tab(tab.tab_id());
                    }
                    self.update_title();
                }
                KeyCode::Backspace | KeyCode::Delete => name.backspace(),
                KeyCode::LeftArrow | KeyCode::RightArrow | KeyCode::Home | KeyCode::End => name.deselect(),
                KeyCode::Char(c) if !mods.contains(KeyModifiers::CTRL) && !c.is_control() => {
                    name.type_char(*c);
                }
                _ => {}
            }
        } else {
            let focus = open.focus;
            match key {
                KeyCode::Escape => {
                    self.lbterm_close_tab_menu();
                    return true;
                }
                KeyCode::UpArrow => open.focus = Some(focus.map_or(Item::Close, |f| menu::step(f, -1, 0))),
                KeyCode::DownArrow => open.focus = Some(focus.map_or(Item::Rename, |f| menu::step(f, 1, 0))),
                KeyCode::LeftArrow => open.focus = focus.map(|f| menu::step(f, 0, -1)),
                KeyCode::RightArrow => open.focus = focus.map(|f| menu::step(f, 0, 1)),
                KeyCode::Enter => {
                    if let Some(item) = focus {
                        self.lbterm_menu_pick(item);
                    }
                    return true;
                }
                _ => {}
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    /// Do what the item says, for the menu's tab
    fn lbterm_menu_pick(&mut self, item: Item) {
        let Some(open) = self.lbterm_menu.open.as_mut() else {
            return;
        };
        let tab_id = open.tab;
        let mux = Mux::get();
        let Some(tab) = mux.get_tab(tab_id) else {
            self.lbterm_close_tab_menu();
            return;
        };
        match item {
            Item::Rename => {
                // The name it shows: its own, or else its pane's
                let mut shown = tab.get_title();
                if shown.is_empty() {
                    if let Some(pane) = tab.get_active_pane() {
                        shown = pane.get_title();
                    }
                }
                open.renaming = Some(menu::Name::new(&shown));
                open.focus = Some(Item::Rename);
                if let Some(window) = self.window.as_ref() {
                    window.invalidate();
                }
                return;
            }
            Item::Colour(colour) => {
                crate::tabcolors::tidy();
                crate::tabcolors::set(tab_id, colour);
            }
            Item::Duplicate => {
                // A new tab in the same folder and domain, beside it
                if let Some(pane) = tab.get_active_pane() {
                    let cwd = pane
                        .get_current_working_dir(CachePolicy::AllowStale)
                        .and_then(|url| url.to_file_path().ok());
                    self.spawn_command(
                        &SpawnCommand {
                            cwd,
                            domain: SpawnTabDomain::DomainId(pane.domain_id()),
                            ..Default::default()
                        },
                        crate::spawn::SpawnWhere::NewTab,
                    );
                }
            }
            Item::NewWindow => self.lbterm_tear_off(tab_id, None),
            Item::CloseOthers => self.lbterm_close_other_tabs(tab_id),
            Item::Close => {
                let idx = mux
                    .get_window(self.mux_window_id)
                    .and_then(|w| w.get_tab_idx_for_id(tab_id));
                if let Some(idx) = idx {
                    // Any "really close?" shows in the tab itself, so it comes forward first
                    if let Err(err) = self.activate_tab(idx as isize) {
                        log::error!("tab menu: {err:#}");
                    }
                    self.close_specific_tab(idx, true);
                }
            }
        }
        self.lbterm_close_tab_menu();
    }

    /// Close every tab in this window but `keep`, asking once if any of them is running
    /// something that would be lost
    fn lbterm_close_other_tabs(&mut self, keep: TabId) {
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            return;
        };
        // The Conductor's tab is its pad's, not one of the rail's: it stays
        let conductor = self.lbterm_panel.conductor_tab;
        let others: Vec<_> = window
            .iter_tabs()
            .filter(|t| t.tab_id() != keep && Some(t.tab_id()) != conductor)
            .cloned()
            .collect();
        drop(window);
        if others.is_empty() {
            return;
        }
        let busy = others
            .iter()
            .filter(|t| !t.can_close_without_prompting(mux::pane::CloseReason::Tab))
            .count();
        let ids: Vec<TabId> = others.iter().map(|t| t.tab_id()).collect();
        if busy == 0 {
            for id in ids {
                mux.remove_tab(id);
            }
            return;
        }
        let Some(keep_tab) = mux.get_tab(keep) else {
            return;
        };
        // The question shows in the tab that stays, so it comes forward first
        let keep_idx = mux
            .get_window(self.mux_window_id)
            .and_then(|w| w.get_tab_idx_for_id(keep));
        if let Some(idx) = keep_idx {
            if let Err(err) = self.activate_tab(idx as isize) {
                log::error!("tab menu: {err:#}");
            }
        }
        // A mux server's panes can't say what they're running, so they always ask
        let known = others.iter().all(|t| {
            t.iter_panes_ignoring_zoom().iter().all(|p| {
                mux.get_domain(p.pane.domain_id())
                    .map_or(true, |d| d.domain_name() == "local")
            })
        });
        let question = menu::close_others_question(ids.len(), busy, known);
        let window = self.window.clone().unwrap();
        let (overlay, future) = crate::overlay::start_overlay(self, &keep_tab, move |tab_id, mut term| {
            if crate::overlay::confirm::run_confirmation(&question, &mut term)? {
                promise::spawn::spawn_into_main_thread(async move {
                    let mux = Mux::get();
                    for id in ids {
                        mux.remove_tab(id);
                    }
                })
                .detach();
            }
            TermWindow::schedule_cancel_overlay(window, tab_id, None);
            Ok(())
        });
        self.assign_overlay(keep, overlay);
        promise::spawn::spawn(future).detach();
    }

    /// A rounded rectangle in one colour, in `quads`' layer 0
    pub(crate) fn lbterm_menu_rounded(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Rect,
        radius: f64,
        colour: LinearRgba,
    ) -> anyhow::Result<()> {
        if area.w <= 0.0 || area.h <= 0.0 {
            return Ok(());
        }
        let r = radius.min(area.w / 2.0).min(area.h / 2.0).floor() as f32;
        let (x, y, w, h) = (area.x as f32, area.y as f32, area.w as f32, area.h as f32);
        if r < 1.0 {
            self.filled_rectangle(quads, 0, euclid::rect(x, y, w, h), colour)?;
            return Ok(());
        }
        let size = euclid::size2(r, r);
        for (poly, cx, cy) in [
            (TOP_LEFT_ROUNDED_CORNER, x, y),
            (TOP_RIGHT_ROUNDED_CORNER, x + w - r, y),
            (BOTTOM_LEFT_ROUNDED_CORNER, x, y + h - r),
            (BOTTOM_RIGHT_ROUNDED_CORNER, x + w - r, y + h - r),
        ] {
            self.poly_quad(quads, 0, euclid::point2(cx, cy), poly, 1, size, colour)?
                .set_grayscale();
        }
        self.filled_rectangle(quads, 0, euclid::rect(x + r, y, w - 2.0 * r, h), colour)?;
        self.filled_rectangle(quads, 0, euclid::rect(x, y + r, r, h - 2.0 * r), colour)?;
        self.filled_rectangle(quads, 0, euclid::rect(x + w - r, y + r, r, h - 2.0 * r), colour)?;
        Ok(())
    }

    /// Draw the menu, if it's open
    pub(crate) fn paint_lbterm_tab_menu(&mut self) -> anyhow::Result<()> {
        let Some(open) = self.lbterm_menu.open.as_ref() else {
            return Ok(());
        };
        let tab_id = open.tab;
        if Mux::get().get_tab(tab_id).is_none() {
            self.lbterm_menu.open = None;
            return Ok(());
        }
        let focus = open.focus;
        let renaming = open.renaming.clone();
        let layout = self.lbterm_menu_layout(open.at);
        let scale = layout.scale;
        let card_rgb = {
            let (r, g, b, _) = self.palette().background.to_srgb_u8();
            palette::blend(((r as u32) << 16) | ((g as u32) << 8) | b as u32, palette::CHIP, 0.55)
        };
        // LBTerm's shape: menus take the panes' corners and hairline (render/card.rs)
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let gl_state = self.render_state.as_ref().unwrap();

        // The words, laid out first so a selected name's width is known for its highlight
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_height = metrics.cell_size.height as f64;
        let mut words = vec![];
        let mut selection = None;
        for (row, r) in &layout.rows {
            let Some(item) = row else { continue };
            let text_rect = Rect {
                x: r.x + 10.0 * scale,
                y: r.y + (r.h - line_height) / 2.0,
                w: r.w - 20.0 * scale,
                h: line_height,
            };
            let (text, bounds, colour) = match (item, &renaming) {
                // The name as it's typed: all of it selected at first, else with a caret
                (Item::Rename, Some(name)) if name.selected => (name.text.clone(), text_rect, palette::TITLE),
                (Item::Rename, Some(name)) => (format!("{}\u{258f}", name.text), text_rect, palette::TITLE),
                (Item::Colour(_), _) => (
                    menu::label(*item).to_string(),
                    Rect { w: layout.label_width - 10.0 * scale, ..text_rect },
                    palette::PROJECT,
                ),
                _ => (menu::label(*item).to_string(), text_rect, palette::TITLE),
            };
            let element = Element::new(&font, ElementContent::Text(text)).colors(ElementColors {
                border: BorderColor::default(),
                bg: LinearRgba::TRANSPARENT.into(),
                text: srgb(colour, 1.0).into(),
            });
            let computed = self.compute_element(
                &LayoutContext {
                    width: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_width as f32,
                        pixel_cell: metrics.cell_size.width as f32,
                    },
                    height: DimensionContext {
                        dpi: self.dimensions.dpi as f32,
                        pixel_max: self.dimensions.pixel_height as f32,
                        pixel_cell: metrics.cell_size.height as f32,
                    },
                    bounds: rect_f(bounds),
                    metrics: &metrics,
                    gl_state,
                    zindex: TEXT_Z,
                },
                &element,
            )?;
            if matches!((item, &renaming), (Item::Rename, Some(name)) if name.selected) {
                let width = (computed.content_rect.width() as f64).min(text_rect.w);
                selection = Some(Rect {
                    x: text_rect.x - 3.0 * scale,
                    y: r.y + 5.0 * scale,
                    w: width + 6.0 * scale,
                    h: r.h - 10.0 * scale,
                });
            }
            words.push(computed);
        }

        // The light round it, fading out: a few rings, faint to fainter
        {
            let layer = gl_state.layer_for_zindex(GLOW_Z)?;
            let mut quads = layer.quad_allocator();
            for (i, alpha) in [0.16, 0.09, 0.05, 0.025].iter().enumerate() {
                let by = (2.0 + 3.0 * i as f64) * scale;
                self.lbterm_menu_rounded(&mut quads, grow(layout.card, by), radius + by, srgb(palette::GLOW, *alpha))?;
            }
        }

        let layer = gl_state.layer_for_zindex(CARD_Z)?;
        let mut quads = layer.quad_allocator();
        // The ring: the panel's red, coral at the middle; then the card inside it
        let stroke = shape.hairline() as f64;
        self.lbterm_menu_rounded(&mut quads, grow(layout.card, stroke), radius + stroke, srgb(palette::RING_MIDDLE, 1.0))?;
        self.lbterm_menu_rounded(&mut quads, layout.card, radius, srgb(card_rgb, 1.0))?;

        // The row under the pointer or the keys
        let hover = srgb(palette::blend(card_rgb, palette::HOVER, palette::HOVER_ALPHA * 1.6), 1.0);
        for (row, r) in &layout.rows {
            match row {
                None => {
                    // The rule
                    let line = Rect {
                        x: r.x + 8.0 * scale,
                        y: r.y + r.h / 2.0,
                        w: r.w - 16.0 * scale,
                        h: scale.max(1.0),
                    };
                    self.filled_rectangle(&mut quads, 0, rect_f(line), srgb(palette::OUTLINE, 0.6))?;
                }
                Some(item) => {
                    let lit = match (item, focus) {
                        (Item::Colour(_), _) => false,
                        (a, Some(b)) => std::mem::discriminant(a) == std::mem::discriminant(&b),
                        _ => false,
                    };
                    if lit || (renaming.is_some() && *item == Item::Rename) {
                        self.lbterm_menu_rounded(&mut quads, *r, 6.0 * scale, hover)?;
                    }
                }
            }
        }
        // A selected name, in the panel's red
        if let Some(s) = selection {
            self.lbterm_menu_rounded(&mut quads, s, 4.0 * scale, srgb(palette::RING_MIDDLE, 0.45))?;
        }

        // The swatches: "no colour" as a hollow ring, then each colour; the tab's own ringed
        let current = crate::tabcolors::get(tab_id);
        for (colour, r) in &layout.swatches {
            let focused = focus == Some(Item::Colour(*colour));
            let chosen = current == *colour;
            if focused || chosen {
                let ring = if chosen { palette::TITLE } else { palette::PROJECT };
                self.lbterm_menu_rounded(&mut quads, grow(*r, 2.5 * scale), r.w, srgb(ring, 0.9))?;
                self.lbterm_menu_rounded(&mut quads, grow(*r, 1.0 * scale), r.w, srgb(card_rgb, 1.0))?;
            }
            match colour {
                Some(i) => {
                    let (cr, cg, cb) = COLOURS[*i].1;
                    self.lbterm_menu_rounded(&mut quads, *r, r.w / 2.0, LinearRgba::with_srgba(cr, cg, cb, 255))?;
                }
                None => {
                    self.lbterm_menu_rounded(&mut quads, *r, r.w / 2.0, srgb(palette::OUTLINE, 1.0))?;
                    self.lbterm_menu_rounded(&mut quads, grow(*r, -1.5 * scale), r.w / 2.0, srgb(card_rgb, 1.0))?;
                }
            }
        }
        drop(quads);

        for computed in &words {
            self.render_element(computed, gl_state, None)?;
        }
        Ok(())
    }
}

/// LBTerm: the tab menu, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// The menu while it's open: its items, the one picked by the pointer or the keys, and the
    /// name being typed
    pub(crate) fn lbterm_state_tab_menu(&self) -> Option<lbterm_extras::state::Menu> {
        let open = self.lbterm_menu.open.as_ref()?;
        let layout = self.lbterm_menu_layout(open.at);
        let rect = |r: &Rect| lbterm_extras::state::Rect::round(r.x, r.y, r.w, r.h);
        let rects: Vec<&Rect> = layout.rows.iter().filter(|(item, _)| item.is_some()).map(|(_, r)| r).collect();
        let items: Vec<Item> = menu::ROWS.iter().flatten().copied().collect();
        let focus = open.focus.and_then(|f| {
            items.iter().position(|item| match (item, f) {
                (Item::Colour(_), Item::Colour(_)) => true,
                (a, b) => *a == b,
            })
        });
        Some(lbterm_extras::state::Menu {
            kind: "tab".to_string(),
            tab: Some(open.tab as u64),
            rect: Some(rect(&layout.card)),
            items: items
                .iter()
                .enumerate()
                .map(|(i, &item)| lbterm_extras::state::MenuItem {
                    label: menu::label(item).to_string(),
                    checked: false,
                    rect: rects.get(i).map(|r| rect(r)),
                })
                .collect(),
            focus,
            renaming: open.renaming.as_ref().map(|name| name.text.clone()),
            waiting: None,
        })
    }

    /// What of the open menu is at `x`, `y`: `Some(Some(item))` on an item, `Some(None)` on the
    /// card elsewhere, `None` off it
    pub(crate) fn lbterm_tab_menu_under(&self, x: f64, y: f64) -> Option<Option<usize>> {
        let open = self.lbterm_menu.open.as_ref()?;
        let layout = self.lbterm_menu_layout(open.at);
        if !layout.card.contains(x, y) {
            return None;
        }
        Some(
            layout
                .rows
                .iter()
                .filter(|(item, _)| item.is_some())
                .position(|(_, r)| r.contains(x, y)),
        )
    }
}
