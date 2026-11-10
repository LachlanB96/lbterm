//! The open tab's marker on the rail (its ring and bar, lbterm/TAB-PANEL.md; lbterm/DESIGN.md,
//! "The tab panel"): where it starts this frame, off the open row's slot, and on which curve, decided
//! here once and played by DWM (wezterm-gui's compositor, `PanelLayers::sync_marker`).
//!
//! - Another tab opening glides the marker to its row on `focus_move`, from wherever it shows and
//!   as fast as it's going.
//! - The same tab's row moving (rows above it closed, or it was dragged and let go) takes the marker
//!   with it on the rows' `pane_layout`, from where it shows.
//! - The open tab closing (Ctrl+W, a middle click, Close tab or Close finished tabs) while the row
//!   that opens in its place slides up into it: the marker rides up with that row, from where the
//!   row shows and at its speed, on its spring, so the ring stays on the open row the whole way
//!   (Lachlan's 3b of 2026-11-07). Until then it waited in the slot while the row slid up under it,
//!   which is what main did for Ctrl+W. When the open row doesn't move (the last row closed, so the
//!   one above opens), it glides up to it on `focus_move` as before.
//! - With motion off, or with no open row before or now, it's on its row in the next frame.
//!
//! And the rail's rows and pads under a menu's card ([`Hold`], Lachlan's 7b of 2026-11-08): they
//! hold still while one shows, its fade out included, and play what came meanwhile once it has
//! gone, as one change from what the last frame before the card showed.

/// The rail's rows and pads under a menu's card (Lachlan's 7b of 2026-11-08: nothing of the rail
/// plays under one): the rows' exits, slides and dissolves (a status word's among them), the
/// marker's moves, the pads' arcs and the Conductor's outlines. While it holds, the compositor
/// applies none of the plan's rows, arcs or outlines and draws none of their surfaces, so the
/// tree keeps what the last frame before the card showed, the surfaces their last frames; the
/// first frame after it goes takes the plan whole, as the frame after a change would have, so a
/// tab closed under the card fades and the rows below slide up from where they were then, the
/// marker riding up with them (`marker`), and with motion off all of it lands there at once.
/// The edge, its light and the wave hold by the panel's hover (lbterm_panel's `Hover::hold`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hold {
    held: bool,
    /// Whether the pads' arcs and the outlines were set turning when the plan was last applied
    /// (`spin`: motion on and the window seen): a change of it is never held, so loops stop for a
    /// window that can't be seen even under a card
    spin: Option<bool>,
}

/// What the rail does this frame, as [`Hold::step`] says
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Under {
    /// Apply the plan and draw what it shows, as any frame does
    Apply,
    /// Hold still: apply nothing and draw nothing of the rows, arcs and outlines
    Held,
    /// The card has gone: apply the plan whole, everything that changed under it at once
    Released,
}

impl Hold {
    /// This frame's: `cards`, a menu's card shows over the window (the plan's `cards`); `placed`,
    /// the panel is placed afresh (made, shown again or the window resized by its frame), when
    /// everything goes straight where it is with nothing to hold; `rows`, the tree has rows to
    /// hold; `spin`, the plan's.
    pub fn step(&mut self, cards: bool, placed: bool, rows: bool, spin: bool) -> Under {
        let hold = cards && !placed && rows && self.spin == Some(spin);
        let was = std::mem::replace(&mut self.held, hold);
        if hold {
            return Under::Held;
        }
        self.spin = Some(spin);
        if was {
            Under::Released
        } else {
            Under::Apply
        }
    }

    /// Whether it holds now: nothing of the rows, arcs or outlines is drawn
    pub fn held(&self) -> bool {
        self.held
    }
}

/// Which of the rows' curves a marker's move is played on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerCurve {
    /// `focus_move`: another tab opening
    Glide,
    /// The rows' `pane_layout`: moving with its row
    Slide,
}

/// What the marker does this frame
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MarkerMove {
    /// On its row's slot at once, with nothing running
    Place,
    /// From `from` pixels off its slot, moving at `velocity`, to the slot on `curve`. A move of no
    /// distance at rest plays nothing
    Move { from: f64, velocity: f64, curve: MarkerCurve },
}

/// The open row, as the marker knows it: its row's key and the top of its slot
pub type Open = (usize, i32);

/// Where the marker goes this frame. `was`: the open row last frame; `now`: this frame's; `shows`:
/// where the marker shows off `was`'s slot now and how fast it's going; `was_gone`: `was`'s row has
/// left the rail this frame; `row_shows`: where `now`'s row shows off its slot now and how fast it
/// goes, as the rows' motion has it this frame (zero for a row at rest in its slot).
pub fn marker(was: Option<Open>, now: Option<Open>, animate: bool, shows: (f64, f64), was_gone: bool, row_shows: (f64, f64)) -> MarkerMove {
    let (Some((was_key, was_y)), Some((key, y)), true) = (was, now, animate) else {
        return MarkerMove::Place;
    };
    let (dy, v) = shows;
    let off = dy + (was_y - y) as f64;
    if key == was_key {
        // The same tab: with its row, from where it shows
        return MarkerMove::Move { from: off, velocity: v, curve: MarkerCurve::Slide };
    }
    let (row_dy, row_v) = row_shows;
    if was_gone && (row_dy != 0.0 || row_v != 0.0) {
        // The open tab closed and the row opening in its place is on its way up into it: on that
        // row from where it shows, moving as it moves
        return MarkerMove::Move { from: row_dy, velocity: row_v, curve: MarkerCurve::Slide };
    }
    MarkerMove::Move { from: off, velocity: v, curve: MarkerCurve::Glide }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Curve, Motion, Spring};

    const ROW: i32 = 46;

    #[test]
    fn another_tab_opening_glides_the_marker_there_from_where_it_shows() {
        // Tab 1 open at the second slot, tab 3 opens at the fourth: from two rows up, on focus_move
        let m = marker(Some((1, ROW)), Some((3, 3 * ROW)), true, (0.0, 0.0), false, (0.0, 0.0));
        assert_eq!(m, MarkerMove::Move { from: -2.0 * ROW as f64, velocity: 0.0, curve: MarkerCurve::Glide });
        // Still on its way to tab 1, it keeps its place and speed
        let m = marker(Some((1, ROW)), Some((3, 3 * ROW)), true, (10.0, -40.0), false, (0.0, 0.0));
        assert_eq!(m, MarkerMove::Move { from: 10.0 - 2.0 * ROW as f64, velocity: -40.0, curve: MarkerCurve::Glide });
    }

    #[test]
    fn the_open_tabs_row_moving_takes_the_marker_with_it_on_the_rows_curve() {
        // A row above closed: tab 3 slides up a slot, and the marker with it
        let m = marker(Some((3, 3 * ROW)), Some((3, 2 * ROW)), true, (0.0, 0.0), false, (ROW as f64, 0.0));
        assert_eq!(m, MarkerMove::Move { from: ROW as f64, velocity: 0.0, curve: MarkerCurve::Slide });
    }

    #[test]
    fn the_open_tab_closing_while_the_next_slides_into_its_slot_rides_the_marker_up_with_it() {
        // Tab 2 open at the second slot closes; tab 3, a slot below, opens and slides up into it.
        // The marker's own glide would be from 0 to 0 (it waited in the slot while the row slid up
        // under it): it's on tab 3's row instead, a slot down, moving as the row moves, on its curve
        let m = marker(Some((2, ROW)), Some((3, ROW)), true, (0.0, 0.0), true, (ROW as f64, 0.0));
        assert_eq!(m, MarkerMove::Move { from: ROW as f64, velocity: 0.0, curve: MarkerCurve::Slide });
        // Played on the row's spring from the row's place and speed, it's where the row is at
        // every moment, so the ring stays on it the whole way
        let spring = Curve::Spring(Spring::new(0.22, 0.0));
        let row = Motion::new(ROW as f64, 0.0, 0.0, spring.clone());
        let MarkerMove::Move { from, velocity, .. } = m else { unreachable!() };
        let ring = Motion::new(from, 0.0, velocity, spring);
        for i in 0..=20 {
            let t = 0.3 * i as f64 / 20.0;
            assert_eq!(ring.value(t), row.value(t), "together at {t} s");
        }
        // Mid-slide (more tabs closing), it takes the row's place and speed then
        let m = marker(Some((2, ROW)), Some((3, ROW)), true, (0.0, 0.0), true, (20.0, -300.0));
        assert_eq!(m, MarkerMove::Move { from: 20.0, velocity: -300.0, curve: MarkerCurve::Slide });
    }

    #[test]
    fn the_last_row_closing_glides_the_marker_up_to_the_row_above() {
        // Tab 4 open at the last slot closes; tab 3 above opens, its row where it was: a glide up
        let m = marker(Some((4, 3 * ROW)), Some((3, 2 * ROW)), true, (0.0, 0.0), true, (0.0, 0.0));
        assert_eq!(m, MarkerMove::Move { from: ROW as f64, velocity: 0.0, curve: MarkerCurve::Glide });
    }

    #[test]
    fn a_tab_opened_by_hand_while_its_row_slides_still_glides_from_the_last() {
        // The last open tab is still there: a switch, not a close, so the ring glides from it
        let m = marker(Some((1, ROW)), Some((3, 2 * ROW)), true, (0.0, 0.0), false, (ROW as f64, 0.0));
        assert_eq!(m, MarkerMove::Move { from: -(ROW as f64), velocity: 0.0, curve: MarkerCurve::Glide });
    }

    #[test]
    fn the_rail_holds_under_a_menus_card_and_takes_what_came_meanwhile_once_it_has_gone() {
        let mut hold = Hold::default();
        // No card: every frame applies the plan
        assert_eq!(hold.step(false, false, true, true), Under::Apply);
        assert!(!hold.held());
        // A card comes and stays, open then fading out: held every frame of it
        for _ in 0..3 {
            assert_eq!(hold.step(true, false, true, true), Under::Held);
            assert!(hold.held());
        }
        // It has gone: the plan whole in that frame, then frames as usual
        assert_eq!(hold.step(false, false, true, true), Under::Released);
        assert!(!hold.held());
        assert_eq!(hold.step(false, false, true, true), Under::Apply);
    }

    #[test]
    fn a_close_held_under_the_card_rides_the_marker_up_once_it_has_gone_as_without_a_card() {
        // Tab 2 open at the second slot closes as Close tab's card fades out. Held, the tree keeps
        // tab 2's row and the marker on it; once the card has gone the rows' frame starts tab 3's
        // slide up from its old slot, and the marker is given what it would have been given in the
        // frame of the close: the same ride
        let mut hold = Hold::default();
        assert_eq!(hold.step(false, false, true, true), Under::Apply);
        assert_eq!(hold.step(true, false, true, true), Under::Held);
        assert_eq!(hold.step(true, false, true, true), Under::Held);
        assert_eq!(hold.step(false, false, true, true), Under::Released);
        let at_once = marker(Some((2, ROW)), Some((3, ROW)), true, (0.0, 0.0), true, (ROW as f64, 0.0));
        assert_eq!(at_once, MarkerMove::Move { from: ROW as f64, velocity: 0.0, curve: MarkerCurve::Slide });
        // With motion off it's on its row in the release's frame, nothing playing after
        assert_eq!(marker(Some((2, ROW)), Some((3, ROW)), false, (0.0, 0.0), true, (ROW as f64, 0.0)), MarkerMove::Place);
    }

    #[test]
    fn the_rail_isnt_held_when_placed_afresh_with_no_rows_or_as_its_loops_stop_or_start() {
        let mut hold = Hold::default();
        // The first frame has nothing applied yet, and a panel placed afresh (made, shown again,
        // the window resized by its frame) goes straight where it is: nothing to hold
        assert_eq!(hold.step(true, false, true, true), Under::Apply);
        assert_eq!(hold.step(true, true, true, true), Under::Apply);
        assert_eq!(hold.step(true, false, false, true), Under::Apply);
        assert_eq!(hold.step(true, false, true, true), Under::Held);
        // The window can't be seen any more (or motion went off): the arcs and outlines stop, so
        // the plan is applied under the card, and it holds again from there
        assert_eq!(hold.step(true, false, true, false), Under::Released);
        assert_eq!(hold.step(true, false, true, false), Under::Held);
        // A resize under the card puts it all straight there
        assert_eq!(hold.step(true, true, true, false), Under::Released);
        assert_eq!(hold.step(true, false, true, false), Under::Held);
    }

    #[test]
    fn with_motion_off_or_no_open_row_the_marker_is_on_its_row_in_the_next_frame() {
        assert_eq!(marker(Some((2, ROW)), Some((3, ROW)), false, (0.0, 0.0), true, (ROW as f64, 0.0)), MarkerMove::Place);
        assert_eq!(marker(None, Some((3, ROW)), true, (0.0, 0.0), false, (0.0, 0.0)), MarkerMove::Place);
        assert_eq!(marker(Some((3, ROW)), None, true, (0.0, 0.0), true, (0.0, 0.0)), MarkerMove::Place);
    }
}
