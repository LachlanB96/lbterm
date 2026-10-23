//! The shape every LBTerm surface shares (lbterm/DESIGN.md, "Shape"):
//! hairline outlines, 12 px corners and the 4 px gutter, and where a pane's
//! card goes in the room its cells leave it.
//!
//! Every pane is a card: the surface colour, rounded corners, a hairline
//! round it. Half a gutter round the card is the pane's visual, so two
//! panes' visuals meet in the middle of the gap between them and the cards
//! sit a whole gutter apart. On a side against the window's edge, the tab
//! bar or the tab panel, the card sits a whole gutter in from it.
//!
//! With the compositor, a card's outline isn't drawn into the pane: it's
//! cut from one small card, the [`Template`], in nine pieces (four
//! corners, four edges stretched to length), which DWM moves with the
//! pane's visual. So the card stays whole and rounded at every size the
//! visual passes through, as the tab panel presses the panes over.
//!
//! Sizes are in pixels at 96 DPI and scale with the window's DPI.

use crate::layout::Area;

/// Every outline: panes, the panel's ring, menus. Never under a pixel
pub const HAIRLINE: f32 = 1.0;
/// The soft edge either side of an outline
pub const FEATHER: f32 = 1.0;
/// Panes, the panel and menus. Pills and pads inside keep their own
pub const RADIUS: f32 = 12.0;
/// Between panes, and between the panes, the panel and the window's edges
pub const GUTTER: f32 = 4.0;
/// Between the text's last column and the scrollbar's thumb
pub const SCROLL_GAP: f32 = 2.0;
/// The straight stretch in the middle of the template's sides: wide enough
/// that the pixel an edge is stretched from has its own colour either side
const TEMPLATE_STRAIGHT: f32 = 5.0;

/// The shape's sizes in one window's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    /// DPI / 96
    pub scale: f32,
}

/// One side of a pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    /// Against the edge of the panes' room (the window's edge, the tab bar
    /// or the tab panel's strip), at this x or y
    Outer(f32),
    /// Across a split from another pane, the middle of the gap at this x
    /// or y
    Inner(f32),
}

/// Which way an edge faces.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Facing {
    /// Left or top: the card starts past it
    Start,
    /// Right or bottom: the card ends before it
    End,
}

impl Shape {
    pub fn new(scale: f32) -> Self {
        Self { scale }
    }

    /// `v` pixels at 96 DPI, in this window's pixels: whole, and never
    /// under one when there's any.
    pub fn px(&self, v: f32) -> f32 {
        (v * self.scale).round().max(if v > 0.0 { 1.0 } else { 0.0 })
    }

    pub fn hairline(&self) -> f32 {
        self.px(HAIRLINE)
    }

    pub fn feather(&self) -> f32 {
        self.px(FEATHER)
    }

    pub fn radius(&self) -> f32 {
        self.px(RADIUS)
    }

    /// Each card's share of the gutter between two: whole, so the gutter
    /// is even and the middle of it lands on a pixel boundary.
    pub fn half_gutter(&self) -> f32 {
        self.px(GUTTER / 2.0)
    }

    pub fn gutter(&self) -> f32 {
        2.0 * self.half_gutter()
    }

    /// Where a split's gap divides the two panes' visuals: its middle, on
    /// the pixel boundary the GPU's fill rule gives it (a pixel whose centre
    /// is exactly on an edge belongs to the quad right of it or below), so
    /// both panes agree.
    pub fn split_at(middle: f32) -> f32 {
        (middle - 0.5).ceil()
    }

    fn edge(&self, side: Side, facing: Facing) -> f32 {
        let sign = match facing {
            Facing::Start => 1.0,
            Facing::End => -1.0,
        };
        match side {
            Side::Outer(at) => at.round() + sign * self.gutter(),
            Side::Inner(middle) => Self::split_at(middle) + sign * self.half_gutter(),
        }
    }

    /// A pane's card, from its four sides.
    pub fn card(&self, left: Side, top: Side, right: Side, bottom: Side) -> Area {
        let (x0, y0) = (self.edge(left, Facing::Start), self.edge(top, Facing::Start));
        let (x1, y1) = (self.edge(right, Facing::End), self.edge(bottom, Facing::End));
        Area::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    /// How much further in than its padding says the text sits on the
    /// right while the scrollbar is shown: the scrollbar lies inside the
    /// card, so the card's gutter, its outline and two pixels clear of it
    /// come on top of the room the padding gives the thumb.
    pub fn scroll_inset(&self) -> f32 {
        self.gutter() + self.hairline() + self.feather() + self.px(2.0)
    }

    /// Between the text and the scrollbar's thumb, on the text's side of
    /// it: the padding grows by this, and the thumb starts this far into
    /// it, so the thumb keeps the padding's width and never touches the
    /// last column (Lachlan, 2026-10-21).
    pub fn scroll_gap(&self) -> f32 {
        self.px(SCROLL_GAP)
    }

    /// The pane's visual round its card: half a gutter all round.
    pub fn visual(&self, card: Area) -> Area {
        card.grow(self.half_gutter())
    }

    /// The card the outlines are cut from.
    pub fn template(&self) -> Template {
        let band = self.half_gutter();
        let corner = band + self.radius() + self.feather();
        let straight = self.px(TEMPLATE_STRAIGHT);
        Template {
            size: 2.0 * corner + straight,
            band,
            corner,
            sample: corner + (straight / 2.0).floor(),
        }
    }
}

/// A small card with half a gutter round it, its outline and what's
/// outside it drawn, its inside clear: the piece every pane's outline is
/// cut from. A corner piece is `corner` square; an edge piece is the
/// one-pixel row or column at `sample` across the band and the side,
/// stretched to the length the edge needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Template {
    /// Its side, half gutter included
    pub size: f32,
    /// The half gutter round the card
    pub band: f32,
    /// A corner piece's side: the band, the whole curve and its soft edge
    pub corner: f32,
    /// The row and column the edges are stretched from, in the straight
    /// middle of each side
    pub sample: f32,
}

impl Template {
    /// The card inside it, in the template's pixels.
    pub fn card(&self) -> Area {
        Area::new(self.band, self.band, self.size - 2.0 * self.band, self.size - 2.0 * self.band)
    }

    /// How long an edge piece is stretched to on a visual `length` across:
    /// what's left between its two corners.
    pub fn stretch(&self, length: f64) -> f64 {
        (length - 2.0 * self.corner as f64).max(0.0)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_tokens_are_the_design_systems() {
        let s = Shape::new(1.0);
        assert_eq!((s.hairline(), s.radius(), s.gutter(), s.half_gutter()), (1.0, 12.0, 4.0, 2.0));
        // Scaled, whole, and the gutter always even
        let s = Shape::new(1.5);
        assert_eq!((s.hairline(), s.radius(), s.gutter()), (2.0, 18.0, 6.0));
        let s = Shape::new(1.25);
        assert_eq!(s.gutter() % 2.0, 0.0);
        assert_eq!(Shape::new(0.5).hairline(), 1.0, "never under a pixel");
    }

    #[test]
    fn a_lone_pane_sits_a_gutter_in_from_its_room() {
        let s = Shape::new(1.0);
        let card = s.card(Side::Outer(72.0), Side::Outer(30.0), Side::Outer(1000.0), Side::Outer(700.0));
        assert_eq!(card, Area::new(76.0, 34.0, 920.0, 662.0));
        assert_eq!(s.visual(card), Area::new(74.0, 32.0, 924.0, 666.0));
    }

    #[test]
    fn panes_across_a_split_are_a_gutter_apart_and_their_visuals_meet() {
        let s = Shape::new(1.0);
        // A 9 px cell: the gap's middle lands on a half pixel
        let middle = 503.5;
        let left = s.card(Side::Outer(72.0), Side::Outer(30.0), Side::Inner(middle), Side::Outer(700.0));
        let right = s.card(Side::Inner(middle), Side::Outer(30.0), Side::Outer(1000.0), Side::Outer(700.0));
        assert_eq!(right.x - left.right(), 4.0, "a gutter apart");
        assert_eq!(s.visual(left).right(), s.visual(right).x, "the visuals meet");
        assert_eq!(s.visual(left).right(), 503.0, "on the fill rule's side of the half pixel");
        for edge in [left.x, left.right(), right.x, right.right()] {
            assert_eq!(edge, edge.round(), "whole pixels");
        }
        // Stacked: the same between top and bottom
        let top = s.card(Side::Outer(72.0), Side::Outer(30.0), Side::Outer(1000.0), Side::Inner(361.0));
        let bottom = s.card(Side::Outer(72.0), Side::Inner(361.0), Side::Outer(1000.0), Side::Outer(700.0));
        assert_eq!(bottom.y - top.bottom(), 4.0);
        assert_eq!(s.visual(top).bottom(), s.visual(bottom).y);
    }

    #[test]
    fn the_scrollbar_clears_the_cards_outline() {
        let s = Shape::new(1.0);
        // From the window's edge: the gutter, the outline and its soft inner
        // pixel, then two clear
        assert_eq!(s.scroll_inset(), 4.0 + 1.0 + 1.0 + 2.0);
        assert!(s.scroll_inset() > s.gutter() + s.hairline() + s.feather());
        // And two pixels clear of the text, scaled
        assert_eq!(s.scroll_gap(), 2.0);
        assert_eq!(Shape::new(1.5).scroll_gap(), 3.0);
    }

    #[test]
    fn the_template_holds_whole_corners_and_a_straight_middle() {
        let s = Shape::new(1.0);
        let t = s.template();
        // The band, the 12 px curve, its soft edge: then the straight run
        assert_eq!(t.corner, 2.0 + 12.0 + 1.0);
        assert_eq!(t.size, 2.0 * t.corner + 5.0);
        assert_eq!(t.card(), Area::new(2.0, 2.0, t.size - 4.0, t.size - 4.0));
        // The sampled column has straight edge either side of it
        assert!(t.sample - 1.0 >= t.corner && t.sample + 2.0 <= t.size - t.corner);
        // Stretched edges fill the gap between the corners, never less than nothing
        assert_eq!(t.stretch(100.0), 70.0);
        assert_eq!(t.stretch(10.0), 0.0);
    }
}
