//! LBTerm: the strip's words changing in place (lbterm/DESIGN.md, "The window strip"): a new
//! summary or its time, the last prompt, a notice coming or going, the zoomed badge, the InMail
//! button's count. With the compositor the strip as it was is drawn again over the new one, on
//! the strip's own colour, in a visual of its own that holds the whole strip and shows, through a
//! clip, just the rows whose words changed (and the button), and DWM fades it out on `swap`: the
//! old words dissolve into the new ones. A
//! new summary or prompt then brightens once, a light over its row (render/lights.rs), and so
//! does the button when more mail comes. The words of a tab switched from or closed dissolve
//! into the next tab's the same way, in step with its panes (`tab_switch` and `swap` are the
//! same curve), and don't brighten for being shown again. Going back to the words still fading
//! out, the strip as it is now starts from as much as it showed, so a quick switch back and
//! forth never jumps. A swap that comes while the one before still fades, and doesn't go back to
//! it, leaves that one fading on over it with its last frame: the words in flight are a stack of
//! visuals by the panes' rules (compositor.rs, `SwapStack`; `lbterm_motion::flight`), so in a run
//! of tabs switched through each one's words keep its panes' share to the end of its fade, and a
//! tab switched back to from further back takes its words out of the stack without the rest
//! moving. Without the compositor, or with motion off, every change is at once.

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
    /// The whole strip, which the swap's swapchain holds (only `rect` shows): the same from one
    /// swap to the next, so its swapchain isn't made again for each (compositor.rs, `prepare_swap`)
    area: Option<PixelRect>,
    /// Counts the swaps, so the compositor starts each fade once
    generation: u64,
    /// The words `was` shows, and whether this swap goes back to the words the one before it
    /// was still fading out (over the same rows): then it starts from as much as they hid
    words: Option<Content>,
    back: bool,
    /// The rows to brighten once the new strip is built: the first's words, the prompt's
    flash_words: bool,
    flash_prompt: bool,
    /// And the buttons on the strip that brighten too: the InMail button when more mail came
    /// (render/inbox.rs), the agenda's when more is urgent or failing (render/agenda.rs)
    flash_buttons: Vec<TabBarItem>,
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

/// `new`, and `old` too when a swap still shows: what changed in the same frame (the words and
/// the InMail button's count) dissolves together. The strip as it is now is what's kept, so over
/// an older swap's rows it changes nothing that isn't already there.
fn cover(old: Option<PixelRect>, new: PixelRect) -> PixelRect {
    let Some(old) = old else {
        return new;
    };
    let (x0, y0) = (old.x.min(new.x), old.y.min(new.y));
    let x1 = (old.x + old.w as i32).max(new.x + new.w as i32);
    let y1 = (old.y + old.h as i32).max(new.y + new.h as i32);
    PixelRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// The strip, `bar`'s box, and `rows` with it (they're inside it), inside the window: what the
/// swap's swapchain holds
fn strip_area(bar: &ComputedElement, rows: PixelRect, window: (u32, u32)) -> PixelRect {
    cover(pixels(bar.bounds, window), rows)
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
    /// Before the strip is built again for new words in the same tab, or another tab's
    /// (`tab_changed`, a switch or a closed tab): keep it as it was, over the rows whose words
    /// changed
    pub(crate) fn lbterm_strip_swap_begin(&mut self, old: &Content, new: &Content, tab_changed: bool) {
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
        let was = as_was(bar);
        let area = strip_area(bar, rect, window);
        let swap = &mut self.lbterm_strip.swap;
        let fading = swap.rect.filter(|_| swap.was.is_some());
        swap.back = fading.is_some_and(|f| cover(Some(f), rect) == f) && swap.words.as_ref() == Some(new);
        swap.rect = Some(cover(fading, rect));
        swap.area = Some(area);
        swap.was = Some(was);
        swap.words = Some(old.clone());
        swap.generation += 1;
        log::debug!(
            "strip: swap {} over {:?}{}{}: {:?} to {:?}",
            swap.generation,
            swap.rect,
            if tab_changed { ", another tab's" } else { "" },
            if swap.back { ", back to what it showed" } else { "" },
            old.title,
            new.title
        );
        // A new summary or prompt brightens once; a time ticking over, a notice or a badge
        // only dissolves, and so do another tab's words, which aren't new
        if !tab_changed {
            swap.flash_words |= old.summary != new.summary && new.summary.is_some();
            swap.flash_prompt |= old.prompt != new.prompt && new.prompt.is_some();
        }
    }

    /// Before the strip is built again for a new count on one of its buttons, `item` (the InMail
    /// button, render/inbox.rs; the agenda's, render/agenda.rs): keep it as it was over the
    /// button, on top of any other change this frame, and brighten the button once it's built if
    /// there's `more`
    pub(crate) fn lbterm_strip_swap_button(&mut self, item: TabBarItem, more: bool) {
        if !self.lbterm_composited() {
            return;
        }
        let Some(bar) = self.fancy_tab_bar.as_ref() else {
            return;
        };
        let mut found = vec![];
        boxes(bar, &|i| *i == item, &mut found);
        let window = (
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        let Some(button) = union(&found).and_then(|r| pixels(r, window)) else {
            return;
        };
        let was = as_was(bar);
        let area = strip_area(bar, button, window);
        let swap = &mut self.lbterm_strip.swap;
        swap.rect = Some(cover(swap.rect.filter(|_| swap.was.is_some()), button));
        swap.area = Some(area);
        swap.was = Some(was);
        swap.words = None;
        swap.back = false;
        swap.generation += 1;
        if more && !swap.flash_buttons.contains(&item) {
            swap.flash_buttons.push(item);
        }
    }

    /// After the tab bar is painted: the strip as it was, while it fades over the new one, and
    /// the light on rows with new words
    pub(crate) fn paint_lbterm_strip_swap(&mut self) -> anyhow::Result<()> {
        let (flash_words, flash_prompt, flash_buttons) = {
            let swap = &mut self.lbterm_strip.swap;
            (
                std::mem::take(&mut swap.flash_words),
                std::mem::take(&mut swap.flash_prompt),
                std::mem::take(&mut swap.flash_buttons),
            )
        };
        if flash_words || flash_prompt || !flash_buttons.is_empty() {
            if let Some(bar) = self.fancy_tab_bar.as_ref() {
                let (first, second) = rows(bar);
                let mut buttons = vec![];
                if !flash_buttons.is_empty() {
                    boxes(bar, &|i| flash_buttons.contains(i), &mut buttons);
                }
                let window = (
                    self.dimensions.pixel_width as u32,
                    self.dimensions.pixel_height as u32,
                );
                let rects: Vec<PixelRect> = vec![flash_words.then_some(first).flatten(), flash_prompt.then_some(second).flatten()]
                    .into_iter()
                    .flatten()
                    .chain(buttons)
                    .filter_map(|r| pixels(r, window))
                    .collect();
                if !rects.is_empty() {
                    let radius = lbterm_extras::lights::text_radius(self.dimensions.dpi as f64 / 96.0);
                    self.lbterm_light_this_frame(Light::Text { rects, radius });
                }
            }
        }

        let generation = self.lbterm_strip.swap.generation;
        let compositor = self.webgpu.as_ref().and_then(|w| w.compositor.as_ref());
        let fading = compositor.map_or(false, |c| c.swap_showing(generation, Instant::now()));
        if !fading {
            if self.lbterm_strip.swap.was.take().is_some() {
                log::debug!("strip: swap {generation} faded");
            }
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

    /// Where the swap's visual goes (the strip), the rows of it that show, which swap it shows
    /// and whether it goes back to what the one before showed, for the compositor; `None`
    /// while there's none to show
    pub(crate) fn lbterm_strip_swap_visual(&self) -> Option<(PixelRect, PixelRect, u64, bool)> {
        let swap = &self.lbterm_strip.swap;
        let rect = swap.was.as_ref().and(swap.rect)?;
        Some((cover(swap.area, rect), rect, swap.generation, swap.back))
    }
}
