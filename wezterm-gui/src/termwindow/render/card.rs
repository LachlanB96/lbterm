//! LBTerm: the shape every surface shares (lbterm/DESIGN.md, "Shape").
//!
//! Every pane is a card: the surface colour, 12 px corners, a hairline
//! round it and a 4 px gutter between it and its neighbours, the tab panel
//! and the window's edges. Where a card goes is `lbterm_panel::Shape`'s;
//! this draws it, and the outlines the tab panel's ring shares.
//!
//! Only the pane with the keyboard has an outline; the others have none and
//! their text sits at 85% (DESIGN.md, "Panes"). Without the compositor the
//! active pane draws its own outline and the others are dimmed by upstream's
//! `inactive_pane_hsb`. With it, every pane's edges and the active outline
//! are cut from template cards drawn into visuals of their own (z-layers 70,
//! 71, 74 and 75), each as wide as the window and as tall: the mask (what's outside
//! a card) and the line (the outline alone). Each pane shows nine pieces of
//! the mask riding on its visual's edges, so a card stays whole and rounded
//! at every size DWM shows it at, as the tab panel presses the panes over;
//! one outline of nine pieces of the line glides to the pane that takes the
//! keyboard, and a pane without it is its visual at 85% (`compositor.rs`,
//! `Edges`, `FocusOutline`).

use crate::quad::{QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::compositor::CardPlan;
use config::HsbTransform;
use lbterm_motion::{CardPart, PixelRect};
use lbterm_panel::{palette, Area, Shape, Side};
use mux::pane::Pane;
use mux::tab::PositionedPane;
use std::sync::Arc;
use window::color::LinearRgba;

/// The card templates' z-layers, each drawn only into its template's
/// visual, never over the window
const CARD_ZINDEX: [(i8, CardPart); 4] = [
    (70, CardPart::MaskWide),
    (71, CardPart::MaskTall),
    (74, CardPart::LineWide),
    (75, CardPart::LineTall),
];

/// The card template a z-layer belongs to, if it's one of theirs.
pub fn part_of_zindex(zindex: i8) -> Option<CardPart> {
    CARD_ZINDEX.iter().find(|(z, _)| *z == zindex).map(|(_, part)| *part)
}

/// Segments round a quarter of a corner
const ARC: usize = 16;

fn srgb(rgb: u32) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, 255)
}

/// The panes' room: inside the window's border (the tab panel's strip
/// included) and under the tab bar, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl crate::TermWindow {
    pub(crate) fn lbterm_shape(&self) -> Shape {
        Shape::new(self.dimensions.dpi as f32 / 96.0)
    }

    /// Where the panes' room is. The top is under the tab bar as it's
    /// drawn: the fancy tab bar can draw a little taller than it says it
    /// is, and the tab panel sits under the same line.
    pub(crate) fn lbterm_room(&self) -> anyhow::Result<Room> {
        let border = self.get_os_border();
        let tab_bar = if self.show_tab_bar {
            self.tab_bar_pixel_height()?
        } else {
            0.0
        };
        let (above, below) = if self.config.tab_bar_at_bottom {
            (0.0, tab_bar)
        } else {
            (tab_bar, 0.0)
        };
        let drawn = match (&self.fancy_tab_bar, self.config.tab_bar_at_bottom) {
            (Some(bar), false) if self.show_tab_bar => bar.bounds.max_y(),
            _ => 0.0,
        };
        Ok(Room {
            left: border.left.get() as f32,
            top: (border.top.get() as f32 + above).max(drawn),
            right: self.dimensions.pixel_width as f32 - border.right.get() as f32,
            bottom: self.dimensions.pixel_height as f32 - border.bottom.get() as f32 - below,
        })
    }

    /// A pane's card: a gutter in from the room on its outer sides, half a
    /// gutter short of the middle of the split's gap on its inner ones.
    pub(crate) fn lbterm_card(&self, pos: &PositionedPane) -> anyhow::Result<Area> {
        let room = self.lbterm_room()?;
        let (padding_left, padding_top) = self.padding_left_top();
        let (cell_w, cell_h) = (
            self.render_metrics.cell_size.width as f32,
            self.render_metrics.cell_size.height as f32,
        );
        let border = self.get_os_border();
        let tab_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height()?
        } else {
            0.0
        };
        // Where the pane's cells start, as the text is drawn
        let x0 = border.left.get() as f32 + padding_left;
        let y0 = border.top.get() as f32 + tab_bar + padding_top;
        let column = |c: usize| x0 + c as f32 * cell_w;
        let row = |r: usize| y0 + r as f32 * cell_h;
        let left = if pos.left == 0 {
            Side::Outer(room.left)
        } else {
            Side::Inner(column(pos.left) - cell_w / 2.0)
        };
        let top = if pos.top == 0 {
            Side::Outer(room.top)
        } else {
            Side::Inner(row(pos.top) - cell_h / 2.0)
        };
        let right = if pos.left + pos.width >= self.terminal_size.cols as usize {
            Side::Outer(room.right)
        } else {
            Side::Inner(column(pos.left + pos.width) + cell_w / 2.0)
        };
        let bottom = if pos.top + pos.height >= self.terminal_size.rows as usize {
            Side::Outer(room.bottom)
        } else {
            Side::Inner(row(pos.top + pos.height) + cell_h / 2.0)
        };
        Ok(self.lbterm_shape().card(left, top, right, bottom))
    }

    /// Where the scrollbar's track runs, its top and height: down the
    /// active pane's card (`pos`), clear of its rounded corners. The
    /// scrollbar is still the window's, at its right edge, following the
    /// active pane, so beside stacked panes it stays beside the one it's for.
    pub(crate) fn lbterm_scroll_track(&self, pos: &PositionedPane) -> anyhow::Result<(usize, usize)> {
        let card = self.lbterm_card(pos)?;
        let r = self.lbterm_shape().radius();
        let top = (card.y + r).max(0.0);
        Ok((top as usize, (card.h - 2.0 * r).max(0.0) as usize))
    }

    /// Whether the scrollbar shows for the active pane: not while its
    /// program has the whole screen (the alternate screen or mouse reporting
    /// on, which a mux pane's server sends with its changes), unless
    /// `lbterm_hide_scroll_bar_in_apps` is off. Its room stays, so the pane
    /// isn't resized each time a program starts or ends.
    pub(crate) fn lbterm_scroll_bar_shows(&self, pane: &Arc<dyn Pane>) -> bool {
        let screen = lbterm_extras::scrollbar::PaneScreen {
            alt_screen: pane.is_alt_screen_active(),
            mouse_grabbed: pane.is_mouse_grabbed(),
        };
        lbterm_extras::scrollbar::shows(
            self.show_scroll_bar,
            self.config.lbterm_hide_scroll_bar_in_apps,
            screen,
        )
    }

    /// Where the scrollbar's thumb is drawn across: from two pixels past
    /// where WezTerm puts it, `from` (the start of the window's right
    /// padding, which `effective_right_padding` widened for the card and the
    /// gap), so it stays clear of the text, to two pixels clear of the
    /// outline of the cards at the window's right.
    pub(crate) fn lbterm_scroll_column(&self, from: f32) -> anyhow::Result<(f32, f32)> {
        let shape = self.lbterm_shape();
        let right = self.lbterm_room()?.right - shape.scroll_inset();
        let from = from + shape.scroll_gap();
        Ok((from, (right - from).max(1.0)))
    }

    /// Whether this frame goes through the compositor's visuals.
    pub(crate) fn lbterm_composited(&self) -> bool {
        self.webgpu
            .as_ref()
            .map_or(false, |w| w.compositor.is_some())
    }

    /// A pane's card filled with `fill` (none over a window background
    /// image), `hsv` applied as to the rest of the pane, and its outline
    /// when the pane draws its own.
    pub(crate) fn lbterm_paint_card(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        card: Area,
        fill: Option<LinearRgba>,
        hsv: Option<HsbTransform>,
        outline: Option<u32>,
    ) -> anyhow::Result<()> {
        let shape = self.lbterm_shape();
        if let Some(fill) = fill {
            self.lbterm_fill_rounded(quads, card, shape.radius(), fill, hsv)?;
        }
        if let Some(outline) = outline {
            self.lbterm_stroke(
                quads,
                card,
                shape.radius(),
                shape.hairline(),
                shape.feather(),
                |_| outline,
            )?;
        }
        Ok(())
    }

    /// The two template cards the panes' outlines are cut from, for the
    /// compositor: each the window's background outside the card, the
    /// outline at its brightest on it, and clear inside. `None` without the
    /// compositor, which has panes draw their own.
    pub(crate) fn paint_lbterm_card_templates(&mut self) -> anyhow::Result<Option<CardPlan>> {
        if !self.lbterm_composited() {
            return Ok(None);
        }
        let shape = self.lbterm_shape();
        let template = shape.template();
        let (w, h) = (
            self.dimensions.pixel_width as f32,
            self.dimensions.pixel_height as f32,
        );
        let size = template.size;
        let wide = Area::new(0.0, 0.0, w.max(size), size);
        let tall = Area::new(0.0, 0.0, size, h.max(size));
        let (r, g, b, _) = self.palette().background.to_srgb_u8();
        let background = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
        // At its brightest: the active outline's group shows it at the rest opacity, and
        // brighter as the keyboard arrives (compositor.rs, `FocusOutline`)
        let outline = palette::card_outline_lit(background);
        let gl_state = self.render_state.as_ref().unwrap();
        for (zindex, part) in CARD_ZINDEX {
            let bounds = match part {
                CardPart::MaskWide | CardPart::LineWide => wide,
                CardPart::MaskTall | CardPart::LineTall => tall,
            };
            let layer = gl_state.layer_for_zindex(zindex)?;
            let mut quads = layer.quad_allocator();
            let card = bounds.grow(-template.band);
            match part {
                // Clear inside, so the pane shows through; the line alone
                // glides over panes and gutters without covering either
                CardPart::MaskWide | CardPart::MaskTall => {
                    self.lbterm_outside(&mut quads, card, shape.radius(), bounds, srgb(background))?
                }
                CardPart::LineWide | CardPart::LineTall => self.lbterm_stroke(
                    &mut quads,
                    card,
                    shape.radius(),
                    shape.hairline(),
                    shape.feather(),
                    |_| outline,
                )?,
            }
        }
        let pixels = |a: Area| PixelRect::new(a.x as i32, a.y as i32, a.w as u32, a.h as u32);
        Ok(Some(CardPlan {
            wide: pixels(wide),
            tall: pixels(tall),
            size: size as u32,
            corner: template.corner as u32,
        }))
    }

    /// One quad with its own corners, in window pixels, and a colour at
    /// each.
    fn lbterm_quad(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        corners: [(f32, f32); 4],
        colours: [LinearRgba; 4],
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<()> {
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), colours[0])?;
        q.set_corners(corners.map(|(x, y)| [x - half_w, y - half_h]), colours);
        q.set_hsv(hsv);
        Ok(())
    }

    /// The four corners of a rounded rectangle: centre, and the angle its
    /// quarter starts at (y running down, so each turns clockwise on
    /// screen).
    fn lbterm_corners(area: Area, r: f32) -> [(f32, f32, f32); 4] {
        use std::f32::consts::{FRAC_PI_2, PI};
        let (left, top, right, bottom) = (area.x, area.y, area.right(), area.bottom());
        [
            (left + r, top + r, PI),
            (right - r, top + r, 1.5 * PI),
            (right - r, bottom - r, 0.0),
            (left + r, bottom - r, FRAC_PI_2),
        ]
    }

    /// `area` filled, its corners rounded to `radius`: straight runs as
    /// rectangles and each corner a fan, with the same segments as
    /// [`Self::lbterm_stroke`], so an outline drawn over it meets it
    /// exactly.
    pub(crate) fn lbterm_fill_rounded(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        area: Area,
        radius: f32,
        colour: LinearRgba,
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<()> {
        use std::f32::consts::FRAC_PI_2;
        if area.w <= 0.0 || area.h <= 0.0 {
            return Ok(());
        }
        let r = radius.min(area.w / 2.0).min(area.h / 2.0).max(0.0);
        let rect = |quads: &mut TripleLayerQuadAllocator, x: f32, y: f32, w: f32, h: f32| -> anyhow::Result<()> {
            if w > 0.0 && h > 0.0 {
                self.filled_rectangle(quads, 0, euclid::rect(x, y, w, h), colour)?
                    .set_hsv(hsv);
            }
            Ok(())
        };
        rect(quads, area.x + r, area.y, area.w - 2.0 * r, area.h)?;
        rect(quads, area.x, area.y + r, r, area.h - 2.0 * r)?;
        rect(quads, area.right() - r, area.y + r, r, area.h - 2.0 * r)?;
        if r <= 0.0 {
            return Ok(());
        }
        for (cx, cy, from) in Self::lbterm_corners(area, r) {
            let at = |angle: f32| (cx + r * angle.cos(), cy + r * angle.sin());
            for i in 0..ARC {
                let a0 = from + FRAC_PI_2 * i as f32 / ARC as f32;
                let a1 = from + FRAC_PI_2 * (i + 1) as f32 / ARC as f32;
                self.lbterm_quad(quads, [(cx, cy), (cx, cy), at(a0), at(a1)], [colour; 4], hsv)?;
            }
        }
        Ok(())
    }

    /// Everything in `bounds` outside `card`'s rounded rectangle, in
    /// `colour`: the bands round its sides and, in each corner, what's
    /// outside the curve, as fans meeting the curve where
    /// [`Self::lbterm_stroke`]'s segments do.
    fn lbterm_outside(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        card: Area,
        radius: f32,
        bounds: Area,
        colour: LinearRgba,
    ) -> anyhow::Result<()> {
        use std::f32::consts::FRAC_PI_2;
        let rect = |quads: &mut TripleLayerQuadAllocator, x: f32, y: f32, w: f32, h: f32| -> anyhow::Result<()> {
            if w > 0.0 && h > 0.0 {
                self.filled_rectangle(quads, 0, euclid::rect(x, y, w, h), colour)?;
            }
            Ok(())
        };
        rect(quads, bounds.x, bounds.y, bounds.w, card.y - bounds.y)?;
        rect(quads, bounds.x, card.bottom(), bounds.w, bounds.bottom() - card.bottom())?;
        rect(quads, bounds.x, card.y, card.x - bounds.x, card.h)?;
        rect(quads, card.right(), card.y, bounds.right() - card.right(), card.h)?;
        let r = radius.min(card.w / 2.0).min(card.h / 2.0);
        if r <= 0.0 {
            return Ok(());
        }
        for (cx, cy, from) in Self::lbterm_corners(card, r) {
            // Out along each angle to the corner's square
            let edge = |angle: f32| {
                let (dx, dy) = (angle.cos(), angle.sin());
                let t = (r / dx.abs().max(1e-6)).min(r / dy.abs().max(1e-6));
                (cx + t * dx, cy + t * dy)
            };
            let at = |angle: f32| (cx + r * angle.cos(), cy + r * angle.sin());
            for i in 0..ARC {
                let a0 = from + FRAC_PI_2 * i as f32 / ARC as f32;
                let a1 = from + FRAC_PI_2 * (i + 1) as f32 / ARC as f32;
                self.lbterm_quad(quads, [at(a0), at(a1), edge(a0), edge(a1)], [colour; 4], None)?;
            }
        }
        Ok(())
    }

    /// An outline: `outline`'s rounded rectangle (corner radius `radius`)
    /// stroked `stroke` wide inward from its edge, soft `feather` pixels
    /// either side so its curves aren't stepped, its colour at each point
    /// `colour(y)`. Every pixel is drawn once: bands along the sides (in
    /// short lengths where the colour changes down them) and fans round the
    /// corners. The panes' cards and the tab panel's ring, grey at rest and
    /// lit red, coral and pink top to bottom.
    pub(crate) fn lbterm_stroke(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        outline: Area,
        radius: f32,
        stroke: f32,
        feather: f32,
        colour: impl Fn(f32) -> u32,
    ) -> anyhow::Result<()> {
        use std::f32::consts::FRAC_PI_2;
        let r = radius.min(outline.w / 2.0).min(outline.h / 2.0);
        if r < stroke + feather || outline.w <= 2.0 * r || outline.h <= 2.0 * r {
            return Ok(());
        }
        let at = |(_, y): (f32, f32), alpha: f32| {
            let (red, green, blue) = palette::bytes(colour(y));
            LinearRgba::with_srgba(red, green, blue, (alpha * 255.0).round() as u8)
        };
        let mut quad = |corners: [(f32, f32); 4], alphas: [f32; 4]| -> anyhow::Result<()> {
            let colours = [0, 1, 2, 3].map(|i| at(corners[i], alphas[i]));
            self.lbterm_quad(quads, corners, colours, None)
        };
        // Out from the edge (in when negative), opaque in the middle
        let bands = [
            (-(stroke + feather), 0.0, -stroke, 1.0),
            (-stroke, 1.0, 0.0, 1.0),
            (0.0, 1.0, feather, 0.0),
        ];
        let (left, top, right, bottom) = (outline.x, outline.y, outline.right(), outline.bottom());
        let step = (8.0 * self.dimensions.dpi as f32 / 96.0).round().max(1.0);
        for &(near, inner, far, outer) in &bands {
            quad(
                [(left + r, top - far), (right - r, top - far), (left + r, top - near), (right - r, top - near)],
                [outer, outer, inner, inner],
            )?;
            quad(
                [(left + r, bottom + near), (right - r, bottom + near), (left + r, bottom + far), (right - r, bottom + far)],
                [inner, inner, outer, outer],
            )?;
            let mut y = top + r;
            while y < bottom - r {
                let y1 = (y + step).min(bottom - r);
                quad(
                    [(left - far, y), (left - near, y), (left - far, y1), (left - near, y1)],
                    [outer, inner, outer, inner],
                )?;
                quad(
                    [(right + near, y), (right + far, y), (right + near, y1), (right + far, y1)],
                    [inner, outer, inner, outer],
                )?;
                y = y1;
            }
            for (cx, cy, from) in Self::lbterm_corners(outline, r) {
                let point = |radius: f32, angle: f32| (cx + radius * angle.cos(), cy + radius * angle.sin());
                for i in 0..ARC {
                    let a0 = from + FRAC_PI_2 * i as f32 / ARC as f32;
                    let a1 = from + FRAC_PI_2 * (i + 1) as f32 / ARC as f32;
                    quad(
                        [point(r + near, a0), point(r + near, a1), point(r + far, a0), point(r + far, a1)],
                        [inner, inner, outer, outer],
                    )?;
                }
            }
        }
        Ok(())
    }
}
