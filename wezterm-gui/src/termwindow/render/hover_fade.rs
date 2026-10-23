//! LBTerm: the strip's hover fade (`lbterm_motion.button_hover`, lbterm/DESIGN.md "The window
//! strip"), whose logic is lbterm_extras::hover.
//!
//! With the compositor, an element that fades (the window buttons, the restart pill, the strip
//! buttons, the strip's words) is drawn in the strip with its resting colours only, its parts
//! too. Its look under the pointer is drawn again into one of two hover slots, z-layers that only their own visuals draw (compositor.rs,
//! `HoverLayers`), and DWM fades the slot's opacity on `button_hover`: in as the pointer
//! arrives, out as it leaves, from wherever it is. The slots sit just over the strip, so the
//! hover look covers the resting one as it fades in. An element made of parts (the words: the
//! title, " · " and the summary) puts on its slot only the parts with a look of their own under
//! the pointer, so a part that doesn't change isn't drawn twice. Without the compositor the strip swaps
//! colours at once, as WezTerm does.

use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::{ComputedElement, ComputedElementContent, ElementColors};
use crate::termwindow::{MouseCapture, UIItemType};
use lbterm_extras::hover::{Fade, HoverSlots, SLOTS};
use lbterm_motion::PixelRect;

/// The slots' z-layers, drawn only into their own visuals
pub const HOVER_Z: [i8; SLOTS] = [72, 73];

/// Whether a z-layer is a hover slot's: never drawn over the window
pub fn owns_zindex(zindex: i8) -> bool {
    HOVER_Z.contains(&zindex)
}

/// The slot a z-layer belongs to
pub fn slot_of(zindex: i8) -> Option<usize> {
    HOVER_Z.iter().position(|&z| z == zindex)
}

/// Whether an element's look under the pointer fades in and out, rather than switching: the
/// window buttons, the restart pill and the strip's words (the summary brightening the moment
/// the pointer reaches them, before the card waits out its rest: lbterm/DESIGN.md, principle 1).
/// Another strip element joins by being named here.
pub fn fades(item: &UIItemType) -> bool {
    matches!(
        item,
        UIItemType::TabBar(
            TabBarItem::WindowButton(_) | TabBarItem::LbtermRestart | TabBarItem::LbtermSummary
        )
    )
}

/// `element` as its slot draws it: in its look under the pointer on the slot's z-layer, with
/// only those of its parts that have a look of their own, in that look.
fn hover_look(element: &ComputedElement, hover: &ElementColors, zindex: i8) -> ComputedElement {
    let mut look = element.clone();
    look.zindex = zindex;
    look.colors = hover.clone();
    look.hover_colors = None;
    // Not a fading element itself any more, so it's drawn as it is
    look.item_type = None;
    if let ComputedElementContent::Children(kids) = &element.content {
        look.content =
            ComputedElementContent::Children(kids.iter().filter_map(|k| part_look(k, zindex)).collect());
    }
    look
}

/// A part on the slot when it has a look of its own, or parts that do
fn part_look(part: &ComputedElement, zindex: i8) -> Option<ComputedElement> {
    let look = hover_look(part, part.hover_colors.as_ref().unwrap_or(&part.colors), zindex);
    let has_parts = matches!(&look.content, ComputedElementContent::Children(kids) if !kids.is_empty());
    (part.hover_colors.is_some() || has_parts).then_some(look)
}

/// `element` with no look under the pointer anywhere in it, for the strip to draw at rest, or
/// `None` when none of its parts has one: a fading element's parts fade with it, on its slot.
pub fn at_rest(element: &ComputedElement) -> Option<ComputedElement> {
    fn has_part_hover(element: &ComputedElement) -> bool {
        match &element.content {
            ComputedElementContent::Children(kids) => kids
                .iter()
                .any(|k| k.hover_colors.is_some() || has_part_hover(k)),
            _ => false,
        }
    }
    fn strip(element: &mut ComputedElement) {
        element.hover_colors = None;
        if let ComputedElementContent::Children(kids) = &mut element.content {
            kids.iter_mut().for_each(strip);
        }
    }
    if !has_part_hover(element) {
        return None;
    }
    let mut rest = element.clone();
    strip(&mut rest);
    Some(rest)
}

/// A window's hover slots, and what its last paint decided for the compositor to start.
#[derive(Debug, Default)]
pub struct HoverFade {
    pub slots: HoverSlots<UIItemType>,
    /// What each slot's opacity does next, waiting for the compositor
    pub pending: [Option<Fade>; SLOTS],
    /// Where the strip was drawn, which the slots' visuals cover
    pub strip: Option<PixelRect>,
}

/// The pixels a box covers, its fractional edges rounded out, inside the window
fn pixels(bounds: ::window::RectF, window: (u32, u32)) -> Option<PixelRect> {
    let x0 = bounds.min_x().floor().max(0.0) as i32;
    let y0 = bounds.min_y().floor().max(0.0) as i32;
    let x1 = (bounds.max_x().ceil() as i32).min(window.0 as i32);
    let y1 = (bounds.max_y().ceil() as i32).min(window.1 as i32);
    (x1 > x0 && y1 > y0).then(|| PixelRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

impl crate::TermWindow {
    /// Before the strip is drawn: which fading element the pointer is over, and so which slots
    /// fade which way. The compositor starts the fades once the frame is drawn.
    pub fn lbterm_hover_frame(&self, strip: &ComputedElement) {
        if !self.lbterm_composited() {
            return;
        }
        let key = self.lbterm_hovered(strip);
        let mut hover = self.lbterm_hover.borrow_mut();
        let fades = hover.slots.hover(key.as_ref());
        for (pending, fade) in hover.pending.iter_mut().zip(fades) {
            if fade.is_some() {
                *pending = fade;
            }
        }
        hover.strip = pixels(
            strip.bounds,
            (
                self.dimensions.pixel_width as u32,
                self.dimensions.pixel_height as u32,
            ),
        );
    }

    /// The fading element under the pointer, as `render_element` decides hover: the pointer
    /// inside its box, and nothing but the UI holding the mouse
    fn lbterm_hovered(&self, element: &ComputedElement) -> Option<UIItemType> {
        if !matches!(self.current_mouse_capture, None | Some(MouseCapture::UI)) {
            return None;
        }
        let event = self.current_mouse_event.as_ref()?;
        find(element, event.coords.x as f32, event.coords.y as f32)
    }

    /// Whether `element`'s hover goes on a slot: it fades, and the compositor draws this window
    pub fn lbterm_hover_fades(&self, element: &ComputedElement) -> bool {
        element.hover_colors.is_some()
            && element.item_type.as_ref().map_or(false, fades)
            && self.lbterm_composited()
    }

    /// `element`'s look under the pointer, on every slot that shows it
    pub fn lbterm_render_hover_slots(
        &self,
        element: &ComputedElement,
        gl_state: &crate::renderstate::RenderState,
        inherited_colors: Option<&ElementColors>,
    ) -> anyhow::Result<()> {
        let (item, hover) = match (&element.item_type, &element.hover_colors) {
            (Some(item), Some(hover)) => (item, hover),
            _ => return Ok(()),
        };
        let slots: Vec<usize> = self.lbterm_hover.borrow().slots.slots_for(item).collect();
        for slot in slots {
            let look = hover_look(element, hover, HOVER_Z[slot]);
            self.render_element(&look, gl_state, inherited_colors)?;
        }
        Ok(())
    }
}

fn find(element: &ComputedElement, x: f32, y: f32) -> Option<UIItemType> {
    if let (Some(item), Some(_)) = (&element.item_type, &element.hover_colors) {
        let b = element.bounds;
        if fades(item) && x >= b.min_x() && x <= b.max_x() && y >= b.min_y() && y <= b.max_y() {
            return Some(item.clone());
        }
    }
    match &element.content {
        ComputedElementContent::Children(kids) => kids.iter().find_map(|kid| find(kid, x, y)),
        _ => None,
    }
}
