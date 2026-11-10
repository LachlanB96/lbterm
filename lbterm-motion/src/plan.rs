//! Which panes move when a tab's layout changes, and from where.
//!
//! The planner only compares rectangles: each pane's rectangle before and
//! after (its visual's rectangle, so out to the window edge or the middle of
//! a split gap). It doesn't know about splits or zoom, which keeps it
//! testable and makes every layout change animate the same way:
//!
//! - A pane in both layouts moves and resizes to its new rectangle.
//! - A new pane that sits beside a pane whose old rectangle covered them
//!   both was split off that pane: it rides in from the edge of the old
//!   rectangle, on the same spring as the edge it's attached to, kept inside
//!   the old rectangle on the way so it never draws over a neighbour.
//! - Any other new pane fades in where it is.
//! - A pane that's gone fades out where it was.
//! - Two layouts with no pane in common (a tab switch, a new window) aren't
//!   `plan`'s. When the old tab's panes left the tab on screen (it's another
//!   tab now, or theirs closed), they stay over the next tab's where they were
//!   and fade out (`dissolve`), starting from what shows (`dissolve_from`), each tab by its
//!   share of the screen while others are in flight too (`flight`).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// Every edge within `tolerance` of the other's.
    pub(crate) fn near(&self, other: &Rect, tolerance: f64) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.right() - other.right()).abs() <= tolerance
            && (self.bottom() - other.bottom()).abs() <= tolerance
    }

    fn contains(&self, other: &Rect, tolerance: f64) -> bool {
        other.x >= self.x - tolerance
            && other.y >= self.y - tolerance
            && other.right() <= self.right() + tolerance
            && other.bottom() <= self.bottom() + tolerance
    }

    /// They share some of the same rows.
    fn overlaps_vertically(&self, other: &Rect, tolerance: f64) -> bool {
        self.y < other.bottom() - tolerance && other.y < self.bottom() - tolerance
    }

    /// They share some of the same columns.
    fn overlaps_horizontally(&self, other: &Rect, tolerance: f64) -> bool {
        self.x < other.right() - tolerance && other.x < self.right() - tolerance
    }
}

/// Rectangles are snapped to whole pixels before they get here; a pixel and
/// a half covers rounding on both sides of a shared edge.
const TOLERANCE: f64 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change<Id> {
    /// Moved, resized or both: position and clip go from `from` to `to` on
    /// the layout spring.
    Layout { id: Id, from: Rect, to: Rect },
    /// Split off another pane: rides in from `from`, just past the edge of
    /// that pane's old rectangle, to `to`, and fades in. It's clipped to
    /// `bounds` (the old rectangle) until it lands.
    Enter {
        id: Id,
        from: Rect,
        to: Rect,
        bounds: Rect,
    },
    /// New, with nothing it came out of: fades in where it is.
    FadeIn { id: Id, to: Rect },
    /// Gone: fades out where it was, under the panes growing over it.
    Exit { id: Id, from: Rect },
    /// Its tab was left (another tab's shown now, or it closed): stays where
    /// it was, over the next tab's panes, and fades out on `tab_switch`. The
    /// next tab is there at once, under it.
    Dissolve { id: Id, from: Rect },
}

impl<Id: Copy> Change<Id> {
    pub fn id(&self) -> Id {
        match self {
            Change::Layout { id, .. }
            | Change::Enter { id, .. }
            | Change::FadeIn { id, .. }
            | Change::Exit { id, .. }
            | Change::Dissolve { id, .. } => *id,
        }
    }
}

/// What moves when the panes on screen go from `old` to `new`, each a list
/// of (pane, rectangle). Panes that didn't move aren't listed.
pub fn plan<Id: Copy + PartialEq>(old: &[(Id, Rect)], new: &[(Id, Rect)]) -> Vec<Change<Id>> {
    let find = |list: &[(Id, Rect)], id: Id| list.iter().find(|(i, _)| *i == id).map(|(_, r)| *r);

    if !new.iter().any(|(id, _)| find(old, *id).is_some()) {
        return vec![];
    }

    let mut changes = vec![];
    for &(id, to) in new {
        match find(old, id) {
            Some(from) => {
                if !from.near(&to, TOLERANCE) {
                    changes.push(Change::Layout { id, from, to });
                }
            }
            None => changes.push(arrival(id, to, old, new)),
        }
    }
    for &(id, from) in old {
        if find(new, id).is_none() {
            changes.push(Change::Exit { id, from });
        }
    }
    changes
}

/// How a new pane arrives: from the pane it was split off, if there is one.
fn arrival<Id: Copy + PartialEq>(
    id: Id,
    to: Rect,
    old: &[(Id, Rect)],
    new: &[(Id, Rect)],
) -> Change<Id> {
    for &(source, source_now) in new {
        let Some(source_was) = old.iter().find(|(i, _)| *i == source).map(|(_, r)| *r) else {
            continue;
        };
        // The source's old rectangle held both of them
        if !source_was.contains(&to, TOLERANCE) || !source_was.contains(&source_now, TOLERANCE) {
            continue;
        }
        let from = if (to.x - source_now.right()).abs() <= TOLERANCE
            && to.overlaps_vertically(&source_now, TOLERANCE)
        {
            // To its right: in from the old right edge
            Rect {
                x: source_was.right(),
                ..to
            }
        } else if (to.right() - source_now.x).abs() <= TOLERANCE
            && to.overlaps_vertically(&source_now, TOLERANCE)
        {
            Rect {
                x: source_was.x - to.w,
                ..to
            }
        } else if (to.y - source_now.bottom()).abs() <= TOLERANCE
            && to.overlaps_horizontally(&source_now, TOLERANCE)
        {
            Rect {
                y: source_was.bottom(),
                ..to
            }
        } else if (to.bottom() - source_now.y).abs() <= TOLERANCE
            && to.overlaps_horizontally(&source_now, TOLERANCE)
        {
            Rect {
                y: source_was.y - to.h,
                ..to
            }
        } else {
            continue;
        };
        return Change::Enter {
            id,
            from,
            to,
            bounds: source_was,
        };
    }
    Change::FadeIn { id, to }
}

/// What the panes of the tab that was on screen do as another tab's take it:
/// a tab switch, or a tab that closed. When `old` and `new` have no pane in
/// common and every pane in `old` has `left` the tab on screen (it's in
/// another tab, or closed), each stays over the new ones where it was and
/// fades out. Nothing for a layout with a pane in both (that's `plan`'s), an
/// overlay over a tab's panes (they're still in the tab on screen), or
/// nothing on screen after. `left` is asked only once the rest holds.
pub fn dissolve<Id: Copy + PartialEq>(
    old: &[(Id, Rect)],
    new: &[(Id, Rect)],
    left: impl Fn(Id) -> bool,
) -> Vec<Change<Id>> {
    let shared = new.iter().any(|(id, _)| old.iter().any(|(o, _)| o == id));
    if old.is_empty() || new.is_empty() || shared || !old.iter().all(|&(id, _)| left(id)) {
        return vec![];
    }
    old.iter().map(|&(id, from)| Change::Dissolve { id, from }).collect()
}

/// Whether the panes on screen gave way to others: some on screen before and after, and none in
/// both. What `dissolve` asks first, so where it's false no pane dissolves
pub fn gave_way<Id: PartialEq>(was: &[Id], now: &[Id]) -> bool {
    !was.is_empty() && !now.is_empty() && !now.iter().any(|id| was.contains(id))
}

/// How visible a pane that's dissolving starts its fade: at `alpha` now
/// (1 unless it was still fading in), times the share of the screen the tab
/// coming in didn't have. That tab may be one still dissolving itself,
/// switched back to at `back` (0 for a tab that wasn't): it comes back whole
/// under the one it was fading over, which then shows as much as it did, so
/// the switch starts from what's on screen and nothing jumps. A tab coming
/// back from under another still dissolving over both keeps its share too.
pub fn dissolve_from(alpha: f64, back: f64) -> f64 {
    (alpha * (1.0 - back.clamp(0.0, 1.0))).clamp(0.0, 1.0)
}

/// How the active pane's outline gets to the pane with the keyboard
/// (lbterm/DESIGN.md, "Panes"). One outline for the window: it glides from
/// the pane that had the keyboard to the one that has it now, and otherwise
/// follows its pane wherever that pane's own motion takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus<Id> {
    /// No pane on screen has the keyboard: hidden
    Hide,
    /// Straight there: the first pane, or a tab switch (no pane in common)
    Place(Id),
    /// The keyboard moved between panes on screen: a glide on `focus_move`
    Glide(Id),
    /// The same pane as before: wherever its own motion takes it
    Follow(Id),
}

/// Where the outline goes when the pane with the keyboard was `was` among
/// the panes `old` and is `now` among `new`.
pub fn focus<Id: Copy + PartialEq>(was: Option<Id>, now: Option<Id>, old: &[Id], new: &[Id]) -> Focus<Id> {
    let Some(now) = now else {
        return Focus::Hide;
    };
    match was {
        Some(was) if was == now => Focus::Follow(now),
        // From a pane that was on screen, to one in a layout that shares a pane with it
        Some(was) if old.contains(&was) && new.iter().any(|id| old.contains(id)) => Focus::Glide(now),
        _ => Focus::Place(now),
    }
}

/// How visible a pane is: all of it with the keyboard, `inactive` without.
pub fn dim_level(active: bool, inactive: f64) -> f64 {
    if active {
        1.0
    } else {
        inactive
    }
}

/// How a pane at `level` is dimmed, channel by channel, in the stored values DWM composes in:
/// `scale` times the pane's own colour plus `offset`, which is `level` of it over the window's
/// `backdrop` (each 0 to 1), its alpha left as it is. At rest that's what the pane at `level`'s
/// opacity over the window background shows, but it stays opaque: dimmed by its opacity, a pane
/// without the keyboard let `1 - level` of whatever was under it through, and once its tab was
/// left that was the next tab's, there at once (the review of 2026-10-30: 15% of the new tab
/// through the panes without the keyboard in the first frame of a switch, none through the one
/// with it). Each channel's scale and offset, in order red, green, blue.
pub fn dim_tint(level: f64, backdrop: [f64; 3]) -> [(f64, f64); 3] {
    let level = level.clamp(0.0, 1.0);
    backdrop.map(|b| (level, (1.0 - level) * b.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod test {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(x, y, w, h)
    }

    const FULL: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1000.0,
        h: 600.0,
    };
    const LEFT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 500.0,
        h: 600.0,
    };
    const RIGHT: Rect = Rect {
        x: 500.0,
        y: 0.0,
        w: 500.0,
        h: 600.0,
    };

    #[test]
    fn nothing_moved_nothing_planned() {
        assert_eq!(plan(&[(1, LEFT), (2, RIGHT)], &[(2, RIGHT), (1, LEFT)]), vec![]);
        // Sub-pixel differences are rounding, not motion
        assert_eq!(plan(&[(1, FULL)], &[(1, r(0.4, 0.0, 999.8, 600.6))]), vec![]);
    }

    #[test]
    fn a_tab_switch_does_not_animate() {
        assert_eq!(plan(&[(1, LEFT), (2, RIGHT)], &[(3, FULL)]), vec![]);
        assert_eq!(plan::<u32>(&[], &[(3, FULL)]), vec![]);
    }

    #[test]
    fn a_tab_left_dissolves_into_the_next() {
        // Switched from the tab with panes 1 and 2 to tab 3's, or that tab closed: both stay
        // where they were, over tab 3's pane, fading out
        assert_eq!(
            dissolve(&[(1, LEFT), (2, RIGHT)], &[(3, FULL)], |_| true),
            vec![Change::Dissolve { id: 1, from: LEFT }, Change::Dissolve { id: 2, from: RIGHT }]
        );
        assert_eq!(dissolve(&[(1, FULL)], &[(3, FULL)], |_| true), vec![Change::Dissolve { id: 1, from: FULL }]);
        // Every layout change is still `plan`'s: it found nothing to move here
        assert_eq!(plan(&[(1, LEFT), (2, RIGHT)], &[(3, FULL)]), vec![]);
    }

    #[test]
    fn only_a_tab_left_dissolves() {
        // An overlay over the tab's pane (the launcher, confirm close): the pane is still in
        // the tab on screen, and the overlay comes at once
        assert_eq!(dissolve(&[(1, FULL)], &[(9, FULL)], |id| id != 1), vec![]);
        // One of them is still in the tab on screen, out of sight
        assert_eq!(dissolve(&[(1, LEFT), (2, RIGHT)], &[(3, FULL)], |id| id == 1), vec![]);
        // A pane in both: a layout change, `plan`'s
        assert_eq!(dissolve(&[(1, LEFT), (2, RIGHT)], &[(1, FULL)], |_| true), vec![]);
        // The last tab: nothing under it to fade into, as the window closes
        assert_eq!(dissolve(&[(1, FULL)], &[], |_| true), vec![]);
        // The first frame
        assert_eq!(dissolve::<u32>(&[], &[(3, FULL)], |_| true), vec![]);
    }

    #[test]
    fn whether_a_pane_left_is_asked_last() {
        // Finding out asks the mux, so a frame with a pane in common never asks
        let asked = std::cell::Cell::new(0);
        let left = |_| {
            asked.set(asked.get() + 1);
            true
        };
        dissolve(&[(1, LEFT), (2, RIGHT)], &[(1, LEFT), (2, RIGHT)], left);
        dissolve(&[(1, FULL)], &[], left);
        assert_eq!(asked.get(), 0);
    }

    /// How much of the screen each tab shows, for visuals stacked bottom to top as (tab,
    /// opacity), each tab's panes covering the screen: a layer shows its opacity of what's
    /// left by the layers over it
    fn shares(stack: &[(usize, f64)]) -> [f64; 4] {
        let mut shares = [0.0; 4];
        let mut left = 1.0;
        for &(tab, alpha) in stack.iter().rev() {
            shares[tab] += left * alpha;
            left *= 1.0 - alpha;
        }
        shares
    }

    fn same(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    #[test]
    fn a_first_switch_starts_from_the_old_tab_whole() {
        // Tab 0 on screen, then tab 1's under it as it starts to go
        let before = shares(&[(0, 1.0)]);
        let after = shares(&[(1, 1.0), (0, dissolve_from(1.0, 0.0))]);
        assert!(same(before, after), "{before:?} {after:?}");
        // A pane still fading in when its tab is left goes from where it got to
        assert!((dissolve_from(0.4, 0.0) - 0.4).abs() < 1e-12);
    }

    #[test]
    fn a_switch_back_starts_from_what_shows() {
        // 0 to 1, then back to 0 with 0 still 30% in: 0 comes back whole under 1, which starts
        // its own fade at what it showed, so the screen is as it was and nothing jumps
        for a in [0.05, 0.3, 0.5, 0.9] {
            let before = shares(&[(1, 1.0), (0, a)]);
            let after = shares(&[(0, 1.0), (1, dissolve_from(1.0, a))]);
            assert!(same(before, after), "at {a}: {before:?} {after:?}");
        }
    }

    #[test]
    fn a_switch_back_under_an_older_fade_keeps_it_over_both() {
        // 0 to 1 to 2 (0 still going over 1, which goes over 2), then back to 1: 1 comes back
        // whole under 2, 2 starts from what it showed, and 0 stays over them all where it is
        let (a, b) = (0.25, 0.6);
        let before = shares(&[(2, 1.0), (1, b), (0, a)]);
        let after = shares(&[(1, 1.0), (2, dissolve_from(1.0, b)), (0, a)]);
        assert!(same(before, after), "{before:?} {after:?}");
        // The older fade on top keeps its share all along
        assert!((after[0] - a).abs() < 1e-9);
    }

    #[test]
    fn only_panes_that_gave_way_can_dissolve() {
        // A tab switch or a closed tab: none of the panes on screen were before
        assert!(gave_way(&[1, 2], &[3]));
        // A split, a closed pane, a zoom: one in both
        assert!(!gave_way(&[1], &[1, 2]));
        assert!(!gave_way(&[1, 2], &[2]));
        // The first frame, or the last tab closing
        assert!(!gave_way::<u32>(&[], &[3]));
        assert!(!gave_way(&[1], &[]));
        // Wherever it's false nothing dissolves, whatever left the tab
        let cases: [(&[(u32, Rect)], &[(u32, Rect)]); 5] = [
            (&[(1, LEFT), (2, RIGHT)], &[(3, FULL)]),
            (&[(1, FULL)], &[(1, LEFT), (2, RIGHT)]),
            (&[(1, LEFT), (2, RIGHT)], &[(2, FULL)]),
            (&[], &[(3, FULL)]),
            (&[(1, FULL)], &[]),
        ];
        for (old, new) in cases {
            let ids = |p: &[(u32, Rect)]| p.iter().map(|(id, _)| *id).collect::<Vec<_>>();
            if !gave_way(&ids(old), &ids(new)) {
                assert_eq!(dissolve(old, new, |_| true), vec![], "{old:?} to {new:?}");
            }
        }
    }

    #[test]
    fn a_tab_seen_again_after_its_fade_starts_whole() {
        assert_eq!(dissolve_from(1.0, 0.0), 1.0);
        // Nothing left of what it was fading over: the tab leaving shows nothing to fade
        assert_eq!(dissolve_from(1.0, 1.0), 0.0);
        // Out of range is clamped
        assert_eq!(dissolve_from(1.0, 1.5), 0.0);
        assert_eq!(dissolve_from(1.0, -0.5), 1.0);
    }

    #[test]
    fn a_split_slides_the_new_pane_in_from_the_edge() {
        assert_eq!(
            plan(&[(1, FULL)], &[(1, LEFT), (2, RIGHT)]),
            vec![
                Change::Layout {
                    id: 1,
                    from: FULL,
                    to: LEFT
                },
                Change::Enter {
                    id: 2,
                    from: r(1000.0, 0.0, 500.0, 600.0),
                    to: RIGHT,
                    bounds: FULL
                },
            ]
        );
    }

    #[test]
    fn the_new_pane_and_the_shrinking_edge_travel_together() {
        // Pane 1's right edge goes 1000 -> 500 and pane 2's left edge goes
        // 1000 -> 500: same start, same end, same spring, so no gap opens
        let changes = plan(&[(1, FULL)], &[(1, LEFT), (2, RIGHT)]);
        let Change::Layout { from, to, .. } = changes[0] else {
            panic!()
        };
        let Change::Enter {
            from: enter_from,
            to: enter_to,
            ..
        } = changes[1]
        else {
            panic!()
        };
        assert_eq!(from.right(), enter_from.x);
        assert_eq!(to.right(), enter_to.x);
    }

    #[test]
    fn a_split_below_rides_up_inside_the_old_pane() {
        // A | S, then S splits into S over E: E comes up from S's old
        // bottom edge and is kept inside S's old rectangle, clear of A
        let s_after = r(500.0, 0.0, 500.0, 590.0);
        let e = r(500.0, 590.0, 500.0, 10.0);
        let changes = plan(&[(1, LEFT), (2, RIGHT)], &[(1, LEFT), (2, s_after), (3, e)]);
        assert_eq!(
            changes,
            vec![
                Change::Layout {
                    id: 2,
                    from: RIGHT,
                    to: s_after
                },
                Change::Enter {
                    id: 3,
                    from: r(500.0, 600.0, 500.0, 10.0),
                    to: e,
                    bounds: RIGHT
                },
            ]
        );
    }

    #[test]
    fn splits_to_the_left_and_above_come_from_those_edges() {
        let changes = plan(&[(1, FULL)], &[(2, LEFT), (1, RIGHT)]);
        assert!(changes.contains(&Change::Enter {
            id: 2,
            from: r(-500.0, 0.0, 500.0, 600.0),
            to: LEFT,
            bounds: FULL
        }));
        let top = r(0.0, 0.0, 1000.0, 200.0);
        let rest = r(0.0, 200.0, 1000.0, 400.0);
        let changes = plan(&[(1, FULL)], &[(2, top), (1, rest)]);
        assert!(changes.contains(&Change::Enter {
            id: 2,
            from: r(0.0, -200.0, 1000.0, 200.0),
            to: top,
            bounds: FULL
        }));
    }

    #[test]
    fn closing_a_pane_grows_its_neighbour_over_it() {
        assert_eq!(
            plan(&[(1, LEFT), (2, RIGHT)], &[(1, FULL)]),
            vec![
                Change::Layout {
                    id: 1,
                    from: LEFT,
                    to: FULL
                },
                Change::Exit { id: 2, from: RIGHT },
            ]
        );
    }

    #[test]
    fn swapping_moves_both() {
        assert_eq!(
            plan(&[(1, LEFT), (2, RIGHT)], &[(1, RIGHT), (2, LEFT)]),
            vec![
                Change::Layout {
                    id: 1,
                    from: LEFT,
                    to: RIGHT
                },
                Change::Layout {
                    id: 2,
                    from: RIGHT,
                    to: LEFT
                },
            ]
        );
    }

    #[test]
    fn unzooming_brings_the_others_back_from_the_edge() {
        // Zoomed, pane 1 had the whole tab; pane 2 comes back from the side
        let changes = plan(&[(1, FULL)], &[(1, LEFT), (2, RIGHT)]);
        assert!(matches!(changes[1], Change::Enter { id: 2, .. }));
    }

    #[test]
    fn the_outline_glides_between_panes_and_follows_its_own() {
        // Focus moved from pane 1 to pane 2, both on screen
        assert_eq!(focus(Some(1), Some(2), &[1, 2], &[1, 2]), Focus::Glide(2));
        // A split: the new pane takes the keyboard, out of the old one
        assert_eq!(focus(Some(1), Some(2), &[1], &[1, 2]), Focus::Glide(2));
        // A close: the keyboard goes to the pane that's left
        assert_eq!(focus(Some(2), Some(1), &[1, 2], &[1]), Focus::Glide(1));
        // The same pane: it follows the pane
        assert_eq!(focus(Some(1), Some(1), &[1, 2], &[1]), Focus::Follow(1));
    }

    #[test]
    fn a_tab_switch_or_the_first_frame_places_it() {
        // No pane in common: another tab
        assert_eq!(focus(Some(1), Some(7), &[1, 2], &[7, 8]), Focus::Place(7));
        assert_eq!(focus(None, Some(1), &[], &[1]), Focus::Place(1));
        // The pane it was on wasn't on screen
        assert_eq!(focus(Some(9), Some(1), &[1, 2], &[1, 2]), Focus::Place(1));
        assert_eq!(focus::<u32>(Some(1), None, &[1], &[]), Focus::Hide);
    }

    #[test]
    fn panes_without_the_keyboard_dim() {
        assert_eq!(dim_level(true, 0.85), 1.0);
        assert_eq!(dim_level(false, 0.85), 0.85);
    }

    /// A pane's stored colour channel `c` dimmed by `dim_tint` at `level` over `backdrop`
    fn tinted(c: f64, level: f64, backdrop: f64) -> f64 {
        let [(scale, offset), ..] = dim_tint(level, [backdrop; 3]);
        scale * c + offset
    }

    #[test]
    fn a_dimmed_pane_looks_as_it_did_at_its_opacity_over_the_window() {
        // White text, the card, the window background, a grey: each as the pane's opacity over
        // the window background showed it (lbterm/COMPOSITOR.md: white to 220 at 85% over #121212)
        let back = 18.0 / 255.0;
        for c in [1.0, 18.0 / 255.0, 0.0, 0.5] {
            let over = 0.85 * c + 0.15 * back;
            assert!((tinted(c, 0.85, back) - over).abs() < 1e-12, "{c}");
        }
        assert!((tinted(1.0, 0.85, back) * 255.0 - 219.45).abs() < 1e-9);
        // With the keyboard, and out of range, nothing changes or it's held to 0..1
        assert_eq!(dim_tint(1.0, [0.2, 0.4, 0.6]), [(1.0, 0.0); 3]);
        assert_eq!(dim_tint(1.4, [0.2; 3]), [(1.0, 0.0); 3]);
        assert_eq!(dim_tint(0.85, [0.5, 0.0, 1.0]).map(|(s, _)| s), [0.85; 3]);
        let [(_, r), (_, g), (_, b)] = dim_tint(0.85, [0.5, 0.0, 1.0]);
        assert!((r - 0.075).abs() < 1e-12 && g == 0.0 && (b - 0.15).abs() < 1e-12);
    }

    #[test]
    fn panes_with_and_without_the_keyboard_dissolve_alike() {
        // A tab left with two panes, one with the keyboard and one without, fading at `alpha`
        // over the next tab's: how much of the next tab shows through each, where the old pane
        // shows `p` and the new `q`. Dimmed by opacity the one without let 15% through at once;
        // dimmed by its tint it's opaque, so both show the same share of the next tab
        let back = 18.0 / 255.0;
        let (p, q) = (0.9, 0.4);
        for alpha in [1.0, 0.7, 0.3, 0.05] {
            // What each old pane is, alone, at rest: the reference the share is read against
            let rest_on = p;
            let rest_off = tinted(p, 0.85, back);
            let on = alpha * p + (1.0 - alpha) * q;
            let off = alpha * tinted(p, 0.85, back) + (1.0 - alpha) * q;
            let share_on = (on - rest_on) / (q - rest_on);
            let share_off = (off - rest_off) / (q - rest_off);
            assert!((share_on - (1.0 - alpha)).abs() < 1e-12);
            assert!((share_off - share_on).abs() < 1e-12, "at {alpha}: {share_on} and {share_off}");
            // By opacity the share through the one without wasn't the same
            let by_opacity = 0.85 * alpha * p + (1.0 - 0.85 * alpha) * q;
            let share_then = (by_opacity - rest_off) / (q - rest_off);
            assert!((share_then - share_on).abs() > 0.05 * alpha, "at {alpha}: {share_then} against {share_on}");
        }
    }

    #[test]
    fn a_pane_with_nowhere_to_come_from_fades_in() {
        // Pane 2 closed and pane 3 opened where it was in the same frame
        assert_eq!(
            plan(&[(1, LEFT), (2, RIGHT)], &[(1, LEFT), (3, RIGHT)]),
            vec![
                Change::FadeIn { id: 3, to: RIGHT },
                Change::Exit { id: 2, from: RIGHT },
            ]
        );
    }
}
