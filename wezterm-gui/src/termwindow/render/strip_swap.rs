//! LBTerm: the strip's words changing in place (lbterm/DESIGN.md, "The window strip"): a new
//! summary or its time, the last prompt, a notice coming or going, the zoomed badge. With the
//! compositor the strip as it was is drawn again over the new one, on the strip's own colour, in
//! a visual of its own that covers just the rows whose words changed, and DWM fades it out on
//! `swap`: the old words dissolve into the new ones. A new summary or prompt then brightens
//! once, a light over its row (render/lights.rs). A tab switch swaps the words at once, and
//! without the compositor, or with motion off, every change does.

use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{ComputedElement, ComputedElementContent};
use crate::termwindow::compositor::Light;
use crate::termwindow::UIItemType;
use ::window::RectF;
use lbterm_extras::strip::Content;
use lbterm_motion::PixelRect;
use std::time::Instant;

/// The strip's colour under the old words, and the old words: with the compositor, the swap's
/// visual's alone (`Target::Swap`)
const BG_Z: i8 = 36;
const TEXT_Z: i8 = 37;

/// Whether a z-layer is the swap's
pub fn owns_zindex(zindex: i8) -> bool {
    (BG_Z..=TEXT_Z).contains(&zindex)
}

#[derive(Default)]
pub struct SwapState {
    /// The strip as it was, drawn over the new one while it fades
    was: Option<ComputedElement>,
    /// Where: the rows whose words changed
    rect: Option<PixelRect>,
    /// Counts the swaps, so the compositor starts each fade once
    generation: u64,
    /// The rows to brighten once the new strip is built: the first's words, the prompt's
    flash_words: bool,
    flash_prompt: bool,
}

/// The boxes of `element`'s descendants (and itself) whose item `pick` takes
fn boxes(element: &ComputedElement, pick: &dyn Fn(&TabBarItem) -> bool, out: &mut Vec<RectF>) {
    if let Some(UIItemType::TabBar(item)) = &element.item_type {
        if pick(item) {
            out.push(element.bounds);
        }
    }
    if let ComputedElementContent::Children(kids) = &element.content {
        for kid in kids {
            boxes(kid, pick, out);
        }
    }
}

fn union(rects: &[RectF]) -> Option<RectF> {
    rects.iter().copied().reduce(|a, b| a.union(&b))
}

/// The strip's rows in a built tab bar: the first row's words up to the pill or the buttons,
/// and the second row's prompt to the same edge
fn rows(bar: &ComputedElement) -> (Option<RectF>, Option<RectF>) {
    let mut found = vec![];
    boxes(bar, &|i| matches!(i, TabBarItem::WindowButton(_) | TabBarItem::LbtermRestart), &mut found);
    let right = found
        .iter()
        .map(|r| r.min_x())
        .fold(bar.bounds.max_x(), f32::min);
    let to_right = |r: RectF| RectF::new(r.origin, euclid::size2((right - r.min_x()).max(0.), r.height()));
    let mut words = vec![];
    boxes(bar, &|i| matches!(i, TabBarItem::LbtermSummary), &mut words);
    let mut prompt = vec![];
    boxes(bar, &|i| matches!(i, TabBarItem::LbtermPrompt | TabBarItem::LbtermPromptList), &mut prompt);
    (union(&words).map(to_right), union(&prompt).map(to_right))
}

/// The pixels a box covers, its edges rounded out, inside the window
fn pixels(r: RectF, window: (u32, u32)) -> Option<PixelRect> {
    let x0 = r.min_x().floor().max(0.) as i32;
    let y0 = r.min_y().floor().max(0.) as i32;
    let x1 = (r.max_x().ceil() as i32).min(window.0 as i32);
    let y1 = (r.max_y().ceil() as i32).min(window.1 as i32);
    (x1 > x0 && y1 > y0).then(|| PixelRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// `element` drawn as it is, in the swap's z-layer: no item, so nothing of it fades on a hover
/// slot or takes the pointer
fn as_was(element: &ComputedElement) -> ComputedElement {
    let mut was = element.clone();
    was.zindex = TEXT_Z;
    was.item_type = None;
    was.hover_colors = None;
    if let ComputedElementContent::Children(kids) = &element.content {
        was.content = ComputedElementContent::Children(kids.iter().map(as_was).collect());
    }
    was
}

impl crate::TermWindow {
    /// Before the strip is built again for new words in the same tab: keep it as it was, over
    /// the rows whose words changed
    pub(crate) fn lbterm_strip_swap_begin(&mut self, old: &Content, new: &Content) {
        if !self.lbterm_composited() {
            return;
        }
        let (words, prompt) = lbterm_extras::strip::changed_rows(old, new);
        let Some(bar) = self.fancy_tab_bar.as_ref() else {
            return;
        };
        let (first, second) = rows(bar);
        let picked: Vec<RectF> = vec![words.then_some(first).flatten(), prompt.then_some(second).flatten()]
            .into_iter()
            .flatten()
            .collect();
        let window = (
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        let Some(rect) = union(&picked).and_then(|r| pixels(r, window)) else {
            return;
        };
        let swap = &mut self.lbterm_strip.swap;
        swap.was = Some(as_was(bar));
        swap.rect = Some(rect);
        swap.generation += 1;
        // A new summary or prompt brightens once; a time ticking over, a notice or a badge
        // only dissolves
        swap.flash_words |= old.summary != new.summary && new.summary.is_some();
        swap.flash_prompt |= old.prompt != new.prompt && new.prompt.is_some();
    }

    /// After the tab bar is painted: the strip as it was, while it fades over the new one, and
    /// the light on rows with new words
    pub(crate) fn paint_lbterm_strip_swap(&mut self) -> anyhow::Result<()> {
        let (flash_words, flash_prompt) = {
            let swap = &mut self.lbterm_strip.swap;
            (std::mem::take(&mut swap.flash_words), std::mem::take(&mut swap.flash_prompt))
        };
        if flash_words || flash_prompt {
            if let Some(bar) = self.fancy_tab_bar.as_ref() {
                let (first, second) = rows(bar);
                let window = (
                    self.dimensions.pixel_width as u32,
                    self.dimensions.pixel_height as u32,
                );
                let rects: Vec<PixelRect> = vec![flash_words.then_some(first).flatten(), flash_prompt.then_some(second).flatten()]
                    .into_iter()
                    .flatten()
                    .filter_map(|r| pixels(r, window))
                    .collect();
                if !rects.is_empty() {
                    let radius = lbterm_extras::lights::text_radius(self.dimensions.dpi as f64 / 96.0);
                    self.lbterm_light(Light::Text { rects, radius });
                }
            }
        }

        let fading = self
            .webgpu
            .as_ref()
            .and_then(|w| w.compositor.as_ref())
            .map_or(false, |c| c.swap_showing(self.lbterm_strip.swap.generation, Instant::now()));
        if !fading {
            self.lbterm_strip.swap.was = None;
            return Ok(());
        }
        let surface = self.palette().background.to_linear();
        let (Some(was), Some(rect)) = (self.lbterm_strip.swap.was.as_ref(), self.lbterm_strip.swap.rect) else {
            return Ok(());
        };
        let gl_state = self.render_state.as_ref().unwrap();
        {
            let layer = gl_state.layer_for_zindex(BG_Z)?;
            let mut quads = layer.quad_allocator();
            self.filled_rectangle(
                &mut quads,
                0,
                euclid::rect(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32),
                surface,
            )?;
        }
        self.render_element(was, gl_state, None)?;
        Ok(())
    }

    /// Where the swap's visual goes and which swap it shows, for the compositor; `None` while
    /// there's none to show
    pub(crate) fn lbterm_strip_swap_visual(&self) -> Option<(PixelRect, u64)> {
        let swap = &self.lbterm_strip.swap;
        swap.was.as_ref().and(swap.rect).map(|r| (r, swap.generation))
    }
}
