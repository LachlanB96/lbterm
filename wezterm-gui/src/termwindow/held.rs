//! LBTerm: the modifiers held (Lachlan, 2026-10-25: "when we hold a modifier, the active pane's
//! outline should go a different colour for each modifier, so we know what's going on", after
//! Ctrl+T sometimes typed a "t" into a Claude session). While Ctrl, Shift or Alt is held the
//! active outline goes to its full light in their colour (`lbterm_panel::palette::held`: Ctrl
//! blue, Shift green, Alt red, mixed as light), on `arrive_in`, and back on `arrive_out` once the
//! last is let go (compositor.rs, `FocusOutline`). While a tab is right-dragged on the rail, Shift
//! isn't shown: there it says the drag connects two tabs, and the drag turns cyan for it
//! (lbterm/LINKS.md, "Connections"; his 2a, 2026-11-01).
//!
//! What's held is what LBTerm itself would give the next key: the modifiers on each key message
//! it takes (Windows' key state as the message left the queue, `GetKeyboardState`), and on each
//! pointer message (`MK_CONTROL`, `MK_SHIFT`, and Alt's key state). So a Ctrl that Windows never
//! reported as down shows no blue, and the "t" after it is no surprise. Without the keyboard
//! nothing is held: a key let go in another window would otherwise leave it lit. The Win key
//! isn't shown: Windows keeps its combinations for itself.

use crate::termwindow::TermWindow;
use ::window::{Modifiers, WindowOps};

/// Ctrl, Shift and Alt, held or not
pub(crate) type Held = [bool; 3];

pub(crate) fn held(mods: Modifiers) -> Held {
    [
        mods.intersects(Modifiers::CTRL | Modifiers::LEFT_CTRL | Modifiers::RIGHT_CTRL),
        mods.intersects(Modifiers::SHIFT | Modifiers::LEFT_SHIFT | Modifiers::RIGHT_SHIFT),
        mods.intersects(Modifiers::ALT | Modifiers::LEFT_ALT | Modifiers::RIGHT_ALT),
    ]
}

impl TermWindow {
    /// What a key or pointer message says is held; a change repaints the outline
    pub(crate) fn lbterm_note_held(&mut self, mods: Modifiers) {
        // A tab's right-drag on the rail with Shift held connects tabs, so it follows Shift as it
        // comes and goes, the pointer still or not (tab_links.rs)
        self.lbterm_link_drag_held(mods);
        let keyboard = self.focused.is_some() || self.lbterm_keyboard_assumed;
        let now = if keyboard { held(mods) } else { [false; 3] };
        if now != self.lbterm_held {
            log::debug!("held: {:?} (ctrl, shift, alt)", now);
            self.lbterm_held = now;
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// What's held that the active outline shows: Shift not while a tab is right-dragged on the
    /// rail, where it says the drag connects tabs and the drag is cyan for it (Lachlan's 2a,
    /// 2026-11-01; lbterm_extras::links::outline_held). The drag's start and end each paint a
    /// frame, which takes the outline to white and back (tab_links.rs)
    fn lbterm_outline_held(&self) -> Held {
        lbterm_extras::links::outline_held(self.lbterm_held, self.lbterm_link_dragging().is_some())
    }

    /// The colour the active outline takes for what's held, if anything is
    pub(crate) fn lbterm_held_colour(&self) -> Option<u32> {
        let [ctrl, shift, alt] = self.lbterm_outline_held();
        lbterm_panel::palette::held(ctrl, shift, alt)
    }

    /// What's held, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_state_held(&self) -> Vec<String> {
        names(self.lbterm_held)
    }

    /// What of it the active outline shows, for the state file
    pub(crate) fn lbterm_state_tint(&self) -> Vec<String> {
        names(self.lbterm_outline_held())
    }
}

/// `ctrl`, `shift` and `alt`, those of them that are on, in that order
fn names(held: Held) -> Vec<String> {
    ["ctrl", "shift", "alt"]
        .iter()
        .zip(held)
        .filter(|(_, on)| *on)
        .map(|(name, _)| name.to_string())
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn either_side_counts() {
        assert_eq!(held(Modifiers::NONE), [false; 3]);
        assert_eq!(held(Modifiers::CTRL | Modifiers::LEFT_CTRL), [true, false, false]);
        assert_eq!(held(Modifiers::RIGHT_SHIFT), [false, true, false]);
        assert_eq!(held(Modifiers::ALT | Modifiers::SHIFT), [false, true, true]);
        // The Win key isn't shown
        assert_eq!(held(Modifiers::SUPER), [false; 3]);
    }
}
