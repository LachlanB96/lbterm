//! What becomes of the active pane's outline as its tab is left (lbterm/COMPOSITOR.md, "One
//! outline, on the pane with the keyboard").
//!
//! The outline is drawn round a pane's visual rectangle in pieces of the card template: a
//! corner in each `corner`-square box and a straight run along each side between them. At a tab
//! switch the next tab's outline is there at once, at full, over the panes, so the old tab's,
//! fading out with its panes, would draw twice whatever the two have in common: the same side's
//! line along the same run (a split pane's top beside the whole tab's), the same corner, and the
//! last few pixels of a corner's curve, which lie on its side's line where the curve runs into
//! it. Drawn twice the line is brighter for as long as it fades (at rest white 6% goes to nearly
//! 12% in the first frame), so the old outline keeps only what the new one doesn't draw
//! ([`Outline::without`]): its runs stop where the new one's curves reach its line, and a corner
//! whose curve runs into a line the new one draws straight leaves out the strip of it on that
//! line's rows ([`Parts`]). Two tabs with one pane each have the same outline, so nothing of the
//! old one is left to fade.
//!
//! A tab switched back to while its outline still fades turns it round from where it is: the
//! window's outline comes up from nothing as its panes come back (on `tab_switch`, or with other
//! tabs still in flight as those go, wezterm-gui's `switch_outline`), and everything that lies on
//! its lines, of the outline that was on screen (`Switch::under`) and of every outline still
//! fading, its own tab's pieces among them (`Switch::split`), keeps what it shows and goes as the
//! window's comes up. On every line the two add up to what showed and rise together to the
//! window's alone, so every pixel goes on from what it was, however many turns came before
//! ([`switch`]). What lies off its lines goes on as it was: the outline that was on screen fades
//! with its tab, and the others as they faded.
//!
//! Until 2026-10-31 the window's outline came back at the share its own tab's fading piece
//! showed, and that share came off what else lay on its lines. A second turn round within the
//! fade (back and forth twice by the side buttons or clicks on the rail's pads) took it off twice
//! where both outlines draw: the top line a split pane shares with a whole tab's lost 36 to 44%
//! of its light at the click and crept back over 300 ms (the review of 2026-10-31). Taking the
//! share off once there still went wrong when the outline on screen showed less than the share
//! (a second click soon after the first), or when two outlines still fading lay on one line.

use crate::Rect;

/// Two lines closer than this are the same line: rectangles are whole pixels at rest.
const SAME: f64 = 0.5;

/// A span along a side shorter than this is nothing.
const SHORT: f64 = 0.5;

/// A part showing this share or less goes at once: the line is lit about 7 levels over the card
/// at rest, so it's under a twentieth of a level, and nothing is kept in the tree for it.
const FAINT: f64 = 1.0 / 255.0;

/// The card template's sizes the outline's pieces are cut to (wezterm-gui's `CardPlan`, from
/// lbterm-panel's `Template`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Card {
    /// A corner piece's side
    pub corner: f64,
    /// How far back from a corner piece's inner end, along either of its sides, its curve lies
    /// on that side's line: to where it covers half the middle of the line's rows
    pub reach: f64,
}

/// The four corners, in the order [`Outline::corners`] keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    /// Where the corner of `rect` is
    fn at(self, rect: &Rect) -> (f64, f64) {
        match self {
            Corner::TopLeft => (rect.x, rect.y),
            Corner::TopRight => (rect.right(), rect.y),
            Corner::BottomLeft => (rect.x, rect.bottom()),
            Corner::BottomRight => (rect.right(), rect.bottom()),
        }
    }

    /// The side across the top or the bottom it turns into
    pub fn horizontal(self) -> Side {
        match self {
            Corner::TopLeft | Corner::TopRight => Side::Top,
            Corner::BottomLeft | Corner::BottomRight => Side::Bottom,
        }
    }

    /// The side down the left or the right it turns into
    pub fn vertical(self) -> Side {
        match self {
            Corner::TopLeft | Corner::BottomLeft => Side::Left,
            Corner::TopRight | Corner::BottomRight => Side::Right,
        }
    }

    /// Where along `side`'s line (one of its two) the corner's curve lies on it: the last
    /// `reach` of its box before the straight run
    fn tangent(self, rect: &Rect, card: &Card, side: Side) -> (f64, f64) {
        let (start, end, first) = match side {
            Side::Top | Side::Bottom => (rect.x, rect.right(), matches!(self, Corner::TopLeft | Corner::BottomLeft)),
            Side::Left | Side::Right => (rect.y, rect.bottom(), matches!(self, Corner::TopLeft | Corner::TopRight)),
        };
        if first {
            (start + card.corner - card.reach, start + card.corner)
        } else {
            (end - card.corner, end - card.corner + card.reach)
        }
    }
}

/// What's drawn of a corner piece: its curve, and the strips of the piece on the rows of its two
/// sides' lines, where the curve runs into the straight line (the top's or the bottom's, the
/// left's or the right's). The three make up the piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parts {
    pub curve: bool,
    /// The strip on the top's or the bottom's line
    pub horizontal: bool,
    /// The strip on the left's or the right's line
    pub vertical: bool,
}

impl Parts {
    pub const ALL: Parts = Parts { curve: true, horizontal: true, vertical: true };
    pub const NONE: Parts = Parts { curve: false, horizontal: false, vertical: false };

    pub fn any(&self) -> bool {
        self.curve || self.horizontal || self.vertical
    }

    /// How many of the three
    pub fn count(&self) -> usize {
        [self.curve, self.horizontal, self.vertical].iter().filter(|p| **p).count()
    }
}

/// The four sides, in the order [`Outline::runs`] keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

    /// The line the side lies on: a y for the top and bottom, an x for the left and right
    fn line(self, rect: &Rect) -> f64 {
        match self {
            Side::Top => rect.y,
            Side::Bottom => rect.bottom(),
            Side::Left => rect.x,
            Side::Right => rect.right(),
        }
    }

    /// The straight run between the side's corners, along it: x for the top and bottom, y for
    /// the left and right
    fn run(self, rect: &Rect, corner: f64) -> (f64, f64) {
        match self {
            Side::Top | Side::Bottom => (rect.x + corner, rect.right() - corner),
            Side::Left | Side::Right => (rect.y + corner, rect.bottom() - corner),
        }
    }
}

/// What of an outline round `rect` is drawn: the spans of each side's straight run, along the
/// side in window coordinates, and which parts of each corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub rect: Rect,
    pub card: Card,
    /// Top, bottom, left, right: each a list of spans
    pub runs: [Vec<(f64, f64)>; 4],
    /// Top-left, top-right, bottom-left, bottom-right
    pub corners: [Parts; 4],
}

impl Outline {
    /// All of the outline round `rect`
    pub fn whole(rect: Rect, card: Card) -> Self {
        let runs = Side::ALL.map(|side| {
            let (a, b) = side.run(&rect, card.corner);
            if b - a >= SHORT {
                vec![(a, b)]
            } else {
                vec![]
            }
        });
        Self { rect, card, runs, corners: [Parts::ALL; 4] }
    }

    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.is_empty()) && !self.corners.iter().any(Parts::any)
    }

    /// Whether the whole outline round `other` draws `side`'s line here
    fn shares_line(&self, side: Side, other: &Rect) -> bool {
        (side.line(&self.rect) - side.line(other)).abs() <= SAME
    }

    fn shares_corner(&self, corner: Corner, other: &Rect) -> bool {
        let ((x, y), (ox, oy)) = (corner.at(&self.rect), corner.at(other));
        (x - ox).abs() <= SAME && (y - oy).abs() <= SAME
    }

    /// Where along `side`'s line the whole outline round `other` lights it, when it's this one's
    /// line too: its straight run, and at either end as far back as its corners' curves lie on it
    fn lit(&self, side: Side, other: &Rect) -> Option<(f64, f64)> {
        self.shares_line(side, other).then(|| {
            let (a, b) = side.run(other, self.card.corner);
            (a - self.card.reach, b + self.card.reach)
        })
    }

    /// Whether the whole outline round `other` draws `side`'s line straight all along where
    /// `corner`'s curve lies on it, so the corner's strip there is drawn by `other` too
    fn covers(&self, corner: Corner, side: Side, other: &Rect) -> bool {
        self.shares_line(side, other) && {
            let (a, b) = side.run(other, self.card.corner);
            let (s, e) = corner.tangent(&self.rect, &self.card, side);
            a - SAME <= s && e <= b + SAME
        }
    }

    /// What's left of it where the whole outline round `other` doesn't draw: its runs on the
    /// same lines as `other`'s less where `other` lights them, and its corners where `other` has
    /// none less the strips of them on lines `other` draws straight there.
    pub fn without(&self, other: &Rect) -> Self {
        let mut out = self.clone();
        for (i, side) in Side::ALL.into_iter().enumerate() {
            if let Some(cut) = self.lit(side, other) {
                out.runs[i] = self.runs[i].iter().flat_map(|&span| minus(span, cut)).collect();
            }
        }
        for (i, corner) in Corner::ALL.into_iter().enumerate() {
            let p = self.corners[i];
            out.corners[i] = if self.shares_corner(corner, other) {
                Parts::NONE
            } else {
                Parts {
                    curve: p.curve,
                    horizontal: p.horizontal && !self.covers(corner, corner.horizontal(), other),
                    vertical: p.vertical && !self.covers(corner, corner.vertical(), other),
                }
            };
        }
        out
    }

    /// Only what the whole outline round `other` draws too: what [`Outline::without`] leaves out.
    pub fn within(&self, other: &Rect) -> Self {
        let mut out = self.clone();
        for (i, side) in Side::ALL.into_iter().enumerate() {
            out.runs[i] = match self.lit(side, other) {
                Some(keep) => self.runs[i].iter().filter_map(|&span| both(span, keep)).collect(),
                None => vec![],
            };
        }
        for (i, corner) in Corner::ALL.into_iter().enumerate() {
            let p = self.corners[i];
            out.corners[i] = if self.shares_corner(corner, other) {
                p
            } else {
                Parts {
                    curve: false,
                    horizontal: p.horizontal && self.covers(corner, corner.horizontal(), other),
                    vertical: p.vertical && self.covers(corner, corner.vertical(), other),
                }
            };
        }
        out
    }

    /// How many pieces it takes to draw: a run each, and each corner's parts
    pub fn pieces(&self) -> usize {
        self.runs.iter().map(Vec::len).sum::<usize>() + self.corners.iter().map(Parts::count).sum::<usize>()
    }
}

/// `span` less `cut`: none, one or two spans
fn minus((a, b): (f64, f64), (c, d): (f64, f64)) -> Vec<(f64, f64)> {
    if d <= a || c >= b {
        return vec![(a, b)];
    }
    [(a, c.min(b)), (d.max(a), b)].into_iter().filter(|(a, b)| b - a >= SHORT).collect()
}

/// Where `span` and `keep` overlap
fn both((a, b): (f64, f64), (c, d): (f64, f64)) -> Option<(f64, f64)> {
    let (a, b) = (a.max(c), b.min(d));
    (b - a >= SHORT).then_some((a, b))
}

/// One outline still fading out with its tab, as [`switch`] sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Fading {
    pub outline: Outline,
    /// How much of it shows now
    pub alpha: f64,
    /// Its tab's panes came back on screen this frame: the tab switched back to
    pub back: bool,
}

/// What the outlines do as the tab on screen gives way to another whose panes dissolve over it.
#[derive(Debug, Clone, PartialEq)]
pub struct Switch {
    /// The window's outline, on the next tab's pane with the keyboard: there at once at full
    /// (false), or, for a tab switched back to while its outline fades, coming up from nothing as
    /// its panes come back (true), with all that lies on its lines going as it comes (`under`,
    /// `split`)
    pub rises: bool,
    /// What of the outline that was on screen the window's doesn't draw, each part with the share
    /// it showed: it fades out with its tab, on `tab_switch`
    pub leave: Vec<(Outline, f64)>,
    /// When it rises: what of the outline that was on screen lies on the window's lines, with the
    /// share it showed, going as the window's comes up, so the two add up to what showed and rise
    /// together, never lit twice
    pub under: Vec<(Outline, f64)>,
    /// What's left of each outline already fading, in the order given: what lies on the window's
    /// lines is gone from it (at once, or into `split` when the window's rises)
    pub keep: Vec<Outline>,
    /// When it rises: what of each outline already fading lies on the window's lines, its own
    /// tab's pieces included, split off it as (which, part, share it showed), going as the
    /// window's comes up, as `under` does
    pub split: Vec<(usize, Outline, f64)>,
}

/// The outlines at a tab switch whose panes dissolve. `was` is the window's outline as it shows
/// now (where it is, and its share: 1 at rest, less while it rises), `None` while hidden; `to`
/// is where it goes, the next tab's pane with the keyboard; `fading` is each outline already
/// fading out, oldest first; `card` the template's sizes.
///
/// A tab switched back to while its outline fades (an outline `back`, at `to`) turns it round:
/// the window's outline comes up from nothing as its panes come back, and every part that lies on
/// its lines, of the outline that was on screen and of each outline fading (its own tab's too),
/// goes on from what it showed as the window's comes up, so every line goes on from what it was,
/// however many turns came before and whatever each part showed. Otherwise the window's outline
/// is there at once at full, and what lay on its lines goes at once. Either way, what of the
/// outline that was on screen lies off the window's lines fades with its tab from what showed of
/// it, and what of each outline fading lies off them fades on as it was.
pub fn switch(was: Option<(Rect, f64)>, to: Rect, fading: &[Fading], card: Card) -> Switch {
    let rises = fading
        .iter()
        .any(|f| f.back && !f.outline.is_empty() && f.outline.rect.near(&to, SAME));
    let mut leave = vec![];
    let mut under = vec![];
    if let Some((rect, shown)) = was {
        let whole = Outline::whole(rect, card);
        leave.push((whole.without(&to), shown));
        if rises {
            under.push((whole.within(&to), shown));
        }
    }
    let keep = fading.iter().map(|f| f.outline.without(&to)).collect();
    let mut split: Vec<(usize, Outline, f64)> = if rises {
        fading
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.outline.within(&to), f.alpha.clamp(0.0, 1.0)))
            .collect()
    } else {
        vec![]
    };
    leave.retain(|(o, from)| !o.is_empty() && *from > FAINT);
    under.retain(|(o, from)| !o.is_empty() && *from > FAINT);
    split.retain(|(_, o, from)| !o.is_empty() && *from > FAINT);
    Switch { rises, leave, under, keep, split }
}

#[cfg(test)]
mod test {
    use super::*;

    /// The corner boxes, and how far back from a box's end its curve lies on the line
    const C: f64 = 14.0;
    const R: f64 = 6.0;
    const CARD: Card = Card { corner: C, reach: R };

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(x, y, w, h)
    }

    /// A window's room, and its halves side by side
    const FULL: Rect = Rect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };
    const LEFT: Rect = Rect { x: 0.0, y: 0.0, w: 500.0, h: 600.0 };
    const RIGHT: Rect = Rect { x: 500.0, y: 0.0, w: 500.0, h: 600.0 };

    /// A corner's curve without its strip on the top's or the bottom's line
    const NOT_ACROSS: Parts = Parts { curve: true, horizontal: false, vertical: true };

    #[test]
    fn the_same_outline_leaves_nothing_to_fade() {
        // Two tabs of one pane each: the next tab's outline is the same line, there at once
        assert!(Outline::whole(FULL, CARD).without(&FULL).is_empty());
        let s = switch(Some((FULL, 1.0)), FULL, &[], CARD);
        assert_eq!(s, Switch { rises: false, leave: vec![], under: vec![], keep: vec![], split: vec![] });
    }

    #[test]
    fn a_split_pane_keeps_only_its_own_side() {
        // From the right half to the whole room: the top, right and bottom are the whole room's
        // too, and the right corners; the left side at the split and its corners go alone, but
        // not the ends of their curves on the top and bottom lines, which the whole room's
        // outline draws straight there
        let left = Outline::whole(RIGHT, CARD).without(&FULL);
        assert_eq!(left.runs, [vec![], vec![], vec![(C, 600.0 - C)], vec![]]);
        assert_eq!(left.corners, [NOT_ACROSS, Parts::NONE, NOT_ACROSS, Parts::NONE]);
        assert_eq!(left.pieces(), 5);
        // The other way: the whole room's left half of the top and bottom, stopping where the
        // right half's corners curve onto them, its left side and corners
        let left = Outline::whole(FULL, CARD).without(&RIGHT);
        assert_eq!(left.runs, [vec![(C, 500.0 + C - R)], vec![(C, 500.0 + C - R)], vec![(C, 600.0 - C)], vec![]]);
        assert_eq!(left.corners, [Parts::ALL, Parts::NONE, Parts::ALL, Parts::NONE]);
    }

    #[test]
    fn a_run_cut_in_the_middle_leaves_both_ends() {
        // A tab in three columns, the middle one with the keyboard, after one of the whole room
        let middle = r(300.0, 0.0, 400.0, 600.0);
        let left = Outline::whole(FULL, CARD).without(&middle);
        assert_eq!(left.runs[0], vec![(C, 300.0 + C - R), (700.0 - C + R, 1000.0 - C)]);
        assert_eq!(left.corners, [Parts::ALL; 4]);
        // Lines that only meet aren't shared: a pane below the other's bottom edge
        let below = r(0.0, 600.0, 1000.0, 300.0);
        assert_eq!(Outline::whole(FULL, CARD).without(&below), Outline::whole(FULL, CARD));
    }

    #[test]
    fn within_is_what_without_leaves_out() {
        for (a, b) in [(RIGHT, FULL), (FULL, RIGHT), (LEFT, RIGHT), (FULL, r(300.0, 0.0, 400.0, 300.0)), (FULL, FULL)] {
            let whole = Outline::whole(a, CARD);
            let (out, inn) = (whole.without(&b), whole.within(&b));
            for i in 0..4 {
                let length = |spans: &[(f64, f64)]| spans.iter().map(|(a, b)| b - a).sum::<f64>();
                assert!((length(&out.runs[i]) + length(&inn.runs[i]) - length(&whole.runs[i])).abs() < 1e-9, "{a:?} {b:?} side {i}");
                let (o, n) = (out.corners[i], inn.corners[i]);
                assert!(o.curve != n.curve && o.horizontal != n.horizontal && o.vertical != n.vertical, "{a:?} {b:?} corner {i}: {o:?} {n:?}");
            }
        }
    }

    /// Points along every line either outline draws: each run's middle and ends, a little in,
    /// where each corner's curve lies on the line, and each corner's curve, as (side or corner,
    /// line or corner's place)
    fn points(rects: &[Rect]) -> Vec<(usize, f64, f64)> {
        let mut out = vec![];
        for rect in rects {
            for (i, side) in Side::ALL.into_iter().enumerate() {
                let (a, b) = side.run(rect, C);
                for t in [a + 1.0, (a + b) / 2.0, b - 1.0, a - R / 2.0, b + R / 2.0] {
                    out.push((i, side.line(rect), t));
                }
            }
            for (i, corner) in Corner::ALL.into_iter().enumerate() {
                let (x, y) = corner.at(rect);
                out.push((4 + i, x, y));
            }
        }
        out
    }

    /// How much line shows at `p` from outlines each at a share, as the light adds up (the line
    /// is faint: 6% at rest, so drawn twice it nearly doubles)
    fn light(p: (usize, f64, f64), layers: &[(&Outline, f64)]) -> f64 {
        let (kind, a, b) = p;
        layers
            .iter()
            .map(|(o, share)| {
                let on = if kind < 4 {
                    let side = Side::ALL[kind];
                    let strip = |corner: Corner, parts: Parts| {
                        let drawn = if corner.horizontal() == side {
                            parts.horizontal
                        } else {
                            corner.vertical() == side && parts.vertical
                        };
                        let (s, e) = corner.tangent(&o.rect, &o.card, side);
                        drawn && b >= s && b <= e
                    };
                    (side.line(&o.rect) - a).abs() <= SAME
                        && (o.runs[kind].iter().any(|&(s, e)| b >= s && b <= e)
                            || Corner::ALL.into_iter().zip(o.corners).any(|(corner, parts)| strip(corner, parts)))
                } else {
                    let (x, y) = Corner::ALL[kind - 4].at(&o.rect);
                    o.corners[kind - 4].curve && (x - a).abs() <= SAME && (y - b).abs() <= SAME
                };
                if on {
                    *share
                } else {
                    0.0
                }
            })
            .sum()
    }

    #[test]
    fn a_switch_never_draws_a_line_twice() {
        // From each layout to each other one, the window's outline at full and the old one's
        // leftover at full as it starts to fade: no point is lit more than once, where a corner's
        // curve meets a line included, the new outline all lit, and the old one's lit where it was
        let layouts = [FULL, LEFT, RIGHT, r(0.0, 0.0, 1000.0, 300.0), r(300.0, 0.0, 400.0, 600.0)];
        for from in layouts {
            for to in layouts {
                let s = switch(Some((from, 1.0)), to, &[], CARD);
                assert!(!s.rises);
                let new = Outline::whole(to, CARD);
                let mut layers: Vec<(&Outline, f64)> = s.leave.iter().map(|(o, a)| (o, *a)).collect();
                layers.push((&new, 1.0));
                for p in points(&[from, to]) {
                    let lit = light(p, &layers);
                    assert!(lit <= 1.0 + 1e-9, "{from:?} to {to:?} at {p:?}: {lit}");
                    let old = Outline::whole(from, CARD);
                    let was = light(p, &[(&old, 1.0)]);
                    assert!(lit >= was - 1e-9, "{from:?} to {to:?} at {p:?}: {was} went to {lit}");
                }
            }
        }
    }

    #[test]
    fn a_corner_meeting_a_line_drawn_straight_is_lit_once() {
        // The right half's top-left corner curves onto the top line, which the whole room's
        // outline draws straight: from the right half to the whole room the corner's end there is
        // the whole room's, and the other way the whole room's top stops where the corner's curve
        // reaches it. Kept, either was lit twice there for as long as it faded
        let on_top = (0, 0.0, 500.0 + C - R / 2.0);
        for (from, to) in [(RIGHT, FULL), (FULL, RIGHT)] {
            let s = switch(Some((from, 1.0)), to, &[], CARD);
            let new = Outline::whole(to, CARD);
            let mut layers: Vec<(&Outline, f64)> = s.leave.iter().map(|(o, a)| (o, *a)).collect();
            assert_eq!(light(on_top, &layers), 0.0, "{from:?} to {to:?}: the old outline still draws there");
            layers.push((&new, 1.0));
            assert_eq!(light(on_top, &layers), 1.0, "{from:?} to {to:?}");
        }
    }

    #[test]
    fn a_switch_back_goes_on_from_what_shows() {
        // From the right half (one) to the whole room (two), then back to one with its outline
        // `g` faded: every point of either outline shows what it showed, then the window's
        // outline rises to full from nothing and the rest goes as it comes, so it ends as one at
        // rest. Where only one's pieces showed, they and the window's add up to `g` rising, as
        // the window's rising from `g` did until 2026-10-31
        for g in [0.1, 0.4, 0.8] {
            let first = switch(Some((RIGHT, 1.0)), FULL, &[], CARD);
            assert_eq!(first.leave.len(), 1);
            let one = first.leave[0].0.clone();
            let two = Outline::whole(FULL, CARD);
            let before: Vec<(&Outline, f64)> = vec![(&one, g), (&two, 1.0)];
            let back = switch(Some((FULL, 1.0)), RIGHT, &[Fading { outline: one.clone(), alpha: g, back: true }], CARD);
            assert!(back.rises);
            assert!(back.keep[0].is_empty(), "one's fading outline lies on the window's lines");
            assert_eq!(back.split, vec![(0, one.clone(), g)]);
            let window = Outline::whole(RIGHT, CARD);
            for e in [0.0, 0.3, 0.7, 1.0] {
                // `e` through tab_switch: the window's outline up from nothing, what went to the
                // tab left going on its own and what lies on the window's lines as it comes
                let mut layers: Vec<(&Outline, f64)> = vec![(&window, e)];
                layers.extend(back.leave.iter().chain(&back.under).map(|(o, from)| (o, from * (1.0 - e))));
                layers.extend(back.split.iter().map(|(_, o, from)| (o, from * (1.0 - e))));
                for p in points(&[RIGHT, FULL]) {
                    let now = light(p, &layers);
                    if e == 0.0 {
                        let was = light(p, &before);
                        assert!((now - was).abs() < 1e-9, "at {g}, {p:?}: {was} turned to {now}");
                    }
                    // Never more than once, and the window's own line never less than it was
                    assert!(now <= 1.0 + 1e-9, "at {g}, {e}, {p:?}: {now}");
                    if light(p, &[(&window, 1.0)]) > 0.0 {
                        assert!(now >= g - 1e-9, "at {g}, {e}, {p:?}: {now}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_switch_back_past_a_newer_fade_goes_on_from_what_shows() {
        // One (the right half) to two (the whole room) to three (the left half), then straight
        // back to one while one's outline is at `a` and two's at `b` (one left first, so further
        // through: `a` under `b`). Two's leftover runs along one's top and right side: it goes on
        // from what it showed as one's comes up there, so nothing jumps and no line is lit twice.
        // Until 2026-10-31 it was kept whole first (those lines went from `b` to `a + b`), then
        // went from `b - a` as one's rose from `a`
        let one = switch(Some((RIGHT, 1.0)), FULL, &[], CARD).leave[0].0.clone();
        let s2 = switch(Some((FULL, 1.0)), LEFT, &[Fading { outline: one.clone(), alpha: 0.6, back: false }], CARD);
        let (one, two) = (s2.keep[0].clone(), s2.leave[0].0.clone());
        assert!(!one.is_empty() && !two.within(&RIGHT).is_empty(), "the case has an overlap to test");
        let three = Outline::whole(LEFT, CARD);
        for (a, b) in [(0.2, 0.5), (0.3, 0.7), (0.45, 0.9)] {
            let fading = [Fading { outline: one.clone(), alpha: a, back: true }, Fading { outline: two.clone(), alpha: b, back: false }];
            let back = switch(Some((LEFT, 1.0)), RIGHT, &fading, CARD);
            assert!(back.rises);
            assert!(back.keep[0].is_empty(), "one's fading outline lies on the window's lines");
            assert_eq!(back.keep[1], two.without(&RIGHT));
            assert_eq!(back.split, vec![(0, one.clone(), a), (1, two.within(&RIGHT), b)]);
            let before: Vec<(&Outline, f64)> = vec![(&one, a), (&two, b), (&three, 1.0)];
            let window = Outline::whole(RIGHT, CARD);
            for e in [0.0, 0.3, 0.7, 1.0] {
                // Each fade on from its share, the window's outline up from nothing, alike on the
                // curve (`e` through it)
                let mut layers: Vec<(&Outline, f64)> = vec![(&window, e), (&back.keep[1], b * (1.0 - e))];
                layers.extend(back.leave.iter().chain(&back.under).map(|(o, from)| (o, from * (1.0 - e))));
                layers.extend(back.split.iter().map(|(_, o, from)| (o, from * (1.0 - e))));
                for p in points(&[RIGHT, FULL, LEFT]) {
                    let now = light(p, &layers);
                    if e == 0.0 {
                        let was = light(p, &before);
                        assert!((now - was).abs() < 1e-9, "at {a}, {b}, {p:?}: {was} turned to {now}");
                    }
                    assert!(now <= 1.0 + 1e-9, "at {a}, {b}, {e}, {p:?}: {now}");
                }
            }
        }
    }

    #[test]
    fn what_goes_as_the_window_comes_up_adds_up_with_it_on_any_rise() {
        // Back to one (the right half), with the window's outline rising on a curve of its own,
        // as when the tabs still in flight go on their own (lbterm_motion::flight): where it
        // draws, it and what goes as it comes up (`under`, `split`) add up to what showed at the
        // start and rise together to the window's alone, never past it, whatever the curve; what
        // leaves on its tab's own curve lies where the window's doesn't draw. From the whole room
        // straight back (its top, right side and bottom on one's lines go as one comes, `under`,
        // and one's own pieces, `split`), and past a newer fade (two's leftover on one's top and
        // right side too)
        let one = switch(Some((RIGHT, 1.0)), FULL, &[], CARD).leave[0].0.clone();
        let a = 0.3;
        let straight = switch(Some((FULL, 1.0)), RIGHT, &[Fading { outline: one.clone(), alpha: a, back: true }], CARD);
        assert!(!straight.under.is_empty(), "the whole room's outline lies on one's lines");
        let s2 = switch(Some((FULL, 1.0)), LEFT, &[Fading { outline: one.clone(), alpha: 0.6, back: false }], CARD);
        let (one2, two) = (s2.keep[0].clone(), s2.leave[0].0.clone());
        let b = 0.7;
        let past = switch(
            Some((LEFT, 1.0)),
            RIGHT,
            &[Fading { outline: one2.clone(), alpha: a, back: true }, Fading { outline: two.clone(), alpha: b, back: false }],
            CARD,
        );
        assert!(!past.split.is_empty(), "two's leftover lies on one's lines");
        let window = Outline::whole(RIGHT, CARD);
        for (back, kept) in [(&straight, None), (&past, Some((&past.keep[1], b)))] {
            for p in points(&[RIGHT, FULL, LEFT]) {
                if light(p, &[(&window, 1.0)]) == 0.0 {
                    continue;
                }
                let own: Vec<(&Outline, f64)> = back.leave.iter().map(|(o, f)| (o, *f)).collect();
                assert_eq!(light(p, &own), 0.0, "{p:?}: a part on its own curve where the window's draws");
                let mut last = None;
                for r in [0.0, 0.1, 0.25, 0.6, 0.9, 1.0] {
                    // `r`: how far up the window's outline has come, on whatever curve
                    let mut layers: Vec<(&Outline, f64)> = vec![(&window, r)];
                    layers.extend(kept);
                    layers.extend(back.under.iter().map(|(o, f)| (o, f * (1.0 - r))));
                    layers.extend(back.split.iter().map(|(_, o, f)| (o, f * (1.0 - r))));
                    let lit = light(p, &layers);
                    assert!(lit <= 1.0 + 1e-9, "{p:?} at {r}: {lit}");
                    if let Some(was) = last {
                        assert!(lit >= was - 1e-9, "{p:?} at {r}: fell from {was} to {lit}");
                    }
                    last = Some(lit);
                }
            }
        }
    }

    #[test]
    fn outlines_still_fading_lose_what_the_next_one_draws() {
        // One (the right half) to two (the whole room) to three (the right half again): one's
        // left side, still fading, is three's left side now, there at once at full
        let one = switch(Some((RIGHT, 1.0)), FULL, &[], CARD).leave[0].0.clone();
        let s = switch(Some((FULL, 1.0)), RIGHT, &[Fading { outline: one.clone(), alpha: 0.5, back: false }], CARD);
        assert!(!s.rises);
        assert!(s.keep[0].is_empty());
        assert!(s.under.is_empty() && s.split.is_empty());
        // Two's left half fades with it, from all of it
        assert_eq!(s.leave.len(), 1);
        assert_eq!(s.leave[0].1, 1.0);
        assert_eq!(s.leave[0].0, Outline::whole(FULL, CARD).without(&RIGHT));
    }

    #[test]
    fn only_the_tab_come_back_at_the_same_pane_turns_round() {
        let one = switch(Some((RIGHT, 1.0)), FULL, &[], CARD).leave[0].0.clone();
        // Its tab back but the keyboard in another of its panes now: one's outline fades on,
        // and the window's is there at once
        let s = switch(Some((FULL, 1.0)), LEFT, &[Fading { outline: one.clone(), alpha: 0.5, back: true }], CARD);
        assert!(!s.rises);
        assert_eq!(s.keep[0], one.without(&LEFT));
        // Hidden before (no pane had the keyboard): nothing leaves
        let s = switch(None, FULL, &[], CARD);
        assert!(s.leave.is_empty());
    }

    #[test]
    fn a_rising_outline_left_goes_from_what_showed() {
        // The window's outline 0.6 of the way back up when the tab is left again
        let s = switch(Some((RIGHT, 0.6)), FULL, &[], CARD);
        assert_eq!(s.leave.len(), 1);
        assert_eq!(s.leave[0].1, 0.6);
    }

    /// How a part of an outline goes on, in a model of the curves wezterm-gui plays: on its own
    /// from `from` at `start` (a tab left, `tab_switch`), or from `from` as the window's outline,
    /// rising from `start`, comes up (on the curve the tabs still in flight leave it, which isn't
    /// `tab_switch`: here a different one)
    #[derive(Debug, Clone, Copy)]
    enum Goes {
        Own { from: f64, start: f64 },
        With { from: f64, start: f64 },
    }

    /// The fade, a second long, and the window's rise, quicker and on another curve
    fn fade(t: f64) -> f64 {
        (1.0 - t).clamp(0.0, 1.0)
    }
    fn rise(t: f64) -> f64 {
        let x = (t / 0.7).clamp(0.0, 1.0);
        1.0 - (1.0 - x) * (1.0 - x)
    }

    impl Goes {
        fn at(&self, t: f64) -> f64 {
            match *self {
                Goes::Own { from, start } => from * fade(t - start),
                Goes::With { from, start } => from * (1.0 - rise(t - start)),
            }
        }
    }

    /// A window's outlines through a run of switches, as wezterm-gui's `switch_outline` keeps
    /// them: each tab's pane with the keyboard, the tab on screen, when the window's outline
    /// started rising (while it does), and every part still fading with how it goes and its tab
    struct Scene {
        tabs: Vec<Rect>,
        on: usize,
        rising: Option<f64>,
        parts: Vec<(Outline, Goes, usize)>,
    }

    impl Scene {
        fn new(tabs: Vec<Rect>) -> Self {
            Self { tabs, on: 0, rising: None, parts: vec![] }
        }

        fn window(&self, t: f64) -> f64 {
            self.rising.map_or(1.0, |start| rise(t - start))
        }

        /// What shows at `p` at `t`
        fn light_at(&self, p: (usize, f64, f64), t: f64) -> f64 {
            let window = Outline::whole(self.tabs[self.on], CARD);
            let mut layers: Vec<(&Outline, f64)> = vec![(&window, self.window(t))];
            layers.extend(self.parts.iter().map(|(o, goes, _)| (o, goes.at(t))));
            light(p, &layers)
        }

        /// The tab `to` on screen at `t`, its panes back from the flight if any of its outline
        /// still fades there (the compositor's `back`, by its panes)
        fn switch(&mut self, to: usize, t: f64) -> Switch {
            let fading: Vec<Fading> = self
                .parts
                .iter()
                .map(|(o, goes, tab)| Fading { outline: o.clone(), alpha: goes.at(t), back: *tab == to })
                .collect();
            let s = switch(Some((self.tabs[self.on], self.window(t))), self.tabs[to], &fading, CARD);
            let old = std::mem::take(&mut self.parts);
            for ((_, goes, tab), keep) in old.iter().zip(&s.keep) {
                if !keep.is_empty() {
                    self.parts.push((keep.clone(), *goes, *tab));
                }
            }
            for (i, part, from) in &s.split {
                self.parts.push((part.clone(), Goes::With { from: *from, start: t }, old[*i].2));
            }
            for (part, from) in &s.under {
                self.parts.push((part.clone(), Goes::With { from: *from, start: t }, self.on));
            }
            for (part, from) in &s.leave {
                self.parts.push((part.clone(), Goes::Own { from: *from, start: t }, self.on));
            }
            self.on = to;
            self.rising = s.rises.then_some(t);
            s
        }
    }

    /// Plays `run` (each switch: the tab and when) and checks every line: at a switch that turns
    /// round, each point shows what it showed (but for what went for being too faint), and at one
    /// that doesn't, the new outline is all there and nothing else changes; between switches no
    /// point is lit more than once; at the end only the last tab's outline is lit, all of it.
    /// What turned round, in order
    fn play(tabs: &[Rect], run: &[(usize, f64)]) -> Vec<bool> {
        let mut scene = Scene::new(tabs.to_vec());
        let at = points(tabs);
        let mut turned = vec![];
        let mut last = 0.0;
        for &(to, t) in run {
            // Never more than once on the way
            let mut k = last;
            while k < t {
                for &p in &at {
                    let lit = scene.light_at(p, k);
                    assert!(lit <= 1.0 + 1e-9, "{run:?} at {k:.3}, {p:?}: lit {lit}");
                }
                k += 0.01;
            }
            let before: Vec<f64> = at.iter().map(|&p| scene.light_at(p, t)).collect();
            let s = scene.switch(to, t);
            turned.push(s.rises);
            let window = Outline::whole(tabs[to], CARD);
            for (&p, was) in at.iter().zip(before) {
                let now = scene.light_at(p, t);
                if s.rises || light(p, &[(&window, 1.0)]) == 0.0 {
                    assert!((now - was).abs() < 0.02, "{run:?}, switching to {to} at {t}, {p:?}: {was} turned to {now}");
                } else {
                    assert!((now - 1.0).abs() < 1e-9, "{run:?}, switching to {to} at {t}, {p:?}: the new outline at {now}");
                }
            }
            last = t;
        }
        let end = last + 2.0;
        let window = Outline::whole(tabs[scene.on], CARD);
        for &p in &at {
            let want = light(p, &[(&window, 1.0)]);
            let lit = scene.light_at(p, end);
            assert!((lit - want).abs() < 1e-9, "{run:?} at the end, {p:?}: {lit} for {want}");
        }
        turned
    }

    #[test]
    fn a_second_turn_round_goes_on_from_what_shows() {
        // Tab one split with the keyboard in its right pane, tab two whole: one to two, back to
        // one mid-fade, and to two again while one's outline still rises, as the review of
        // 2026-10-31 did it with the back button and two clicks on the rail's pads. Until then the
        // top line both outlines draw lost what two's piece on it showed (36 to 44% of it, read in
        // a test window) at the second click. Each line goes on from what it showed: with the
        // second click late in one's rise, early in it (the outline on screen showing less than
        // two's piece on its lines), and at once; and a third turn
        let tabs = [RIGHT, FULL];
        for (first, second) in [(0.15, 0.15), (0.15, 0.03), (0.6, 0.02), (0.3, 0.0001), (0.05, 0.4)] {
            let run = [(1, 0.0), (0, first), (1, first + second), (0, first + second + 0.1)];
            assert_eq!(play(&tabs, &run), vec![false, true, true, true], "{run:?}");
        }
        // At the second click the top both draw shows all of one line, as before it
        let mut scene = Scene::new(tabs.to_vec());
        scene.switch(1, 0.0);
        scene.switch(0, 0.15);
        let top = (0, 0.0, 750.0);
        let t = 0.3;
        let was = scene.light_at(top, t);
        assert!((was - 1.0).abs() < 1e-9, "{was}");
        scene.switch(1, t);
        assert!((scene.light_at(top, t) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn every_line_goes_on_from_what_it_was_through_any_run_of_switches() {
        // Runs of switches between two to four tabs, each with its pane with the keyboard in one
        // of five layouts (tabs may share one), at gaps from 2 ms to most of a fade, often back
        // to the tab before, as side buttons and clicks on the rail's pads go
        let layouts = [FULL, LEFT, RIGHT, r(0.0, 0.0, 1000.0, 300.0), r(300.0, 0.0, 400.0, 600.0)];
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut turns = 0;
        for _ in 0..300 {
            let count = 2 + next(3) as usize;
            let tabs: Vec<Rect> = (0..count).map(|_| layouts[next(5) as usize]).collect();
            let mut run = vec![];
            let (mut on, mut before, mut t) = (0, 0, 0.0);
            for _ in 0..(3 + next(6)) {
                let to = if next(2) == 0 && before != on {
                    before
                } else {
                    let mut to = next(count as u64) as usize;
                    while to == on {
                        to = next(count as u64) as usize;
                    }
                    to
                };
                run.push((to, t));
                before = on;
                on = to;
                t += [0.002, 0.02, 0.05, 0.1, 0.15, 0.3, 0.6][next(7) as usize];
            }
            turns += play(&tabs, &run).iter().filter(|r| **r).count();
        }
        assert!(turns > 100, "only {turns} turns round to test");
    }
}
