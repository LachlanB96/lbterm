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
//! - Two layouts with no pane in common (a tab switch, a new window) don't
//!   animate at all.

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
    fn near(&self, other: &Rect, tolerance: f64) -> bool {
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
}

impl<Id: Copy> Change<Id> {
    pub fn id(&self) -> Id {
        match self {
            Change::Layout { id, .. }
            | Change::Enter { id, .. }
            | Change::FadeIn { id, .. }
            | Change::Exit { id, .. } => *id,
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
