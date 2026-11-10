//! LBTerm: which of a window's mouse events a mux client's pane sends its server (lbterm/MUX.md,
//! "The pointer over a pane").
//!
//! The window hands a pane every move of the pointer over it, and upstream's client sent each to
//! the server (SendMouseEvent), one at a time, the next as the last was answered: a few hundred a
//! second while the pointer crossed a pane on its way to the rail or the strip, each a task on the
//! server's only thread and an answer back. The terminal there reports a move to its program only
//! when the program asked for the mouse (term's mouse_move: button-event or any-event tracking),
//! so for a pane whose program hasn't (a shell at its prompt, which is most of Lachlan's that
//! aren't Claude Code's fullscreen view) every move was answered and dropped. Those aren't sent.
//! A move with nothing asked for changes nothing else there: the buttons it knows are held come
//! from presses and releases, which still go, and the last move it reported is kept only when it
//! reports one. Moves over a pane whose program has the mouse still all go: which of them it
//! reports (a cell changed, or a pixel in SGR-pixels mode, 1016) turns on what the program asked
//! for, which the server doesn't send.

use wezterm_term::{MouseEvent, MouseEventKind};

/// Whether `event` goes to the server, the pane's program having the mouse or not (`grabbed`:
/// the server's word in the pane's last push)
pub(crate) fn sends(event: &MouseEvent, grabbed: bool) -> bool {
    grabbed || event.kind != MouseEventKind::Move
}

#[cfg(test)]
mod test {
    use super::*;
    use wezterm_term::{KeyModifiers, MouseButton};

    fn event(kind: MouseEventKind, button: MouseButton) -> MouseEvent {
        MouseEvent {
            kind,
            x: 3,
            y: 2,
            x_pixel_offset: 4,
            y_pixel_offset: 5,
            button,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn a_move_goes_only_to_a_program_that_has_the_mouse() {
        let moved = event(MouseEventKind::Move, MouseButton::None);
        assert!(!sends(&moved, false), "a shell's pane drops every move");
        assert!(sends(&moved, true));
        // A drag over a pane that hasn't the mouse is the window's selection, never the program's
        let dragged = event(MouseEventKind::Move, MouseButton::Left);
        assert!(!sends(&dragged, false));
        assert!(sends(&dragged, true));
    }

    #[test]
    fn presses_releases_and_the_wheel_always_go() {
        // The server keeps which buttons are held from these whether or not the program has the
        // mouse, so a move after a press is reported as a drag once it does
        for (kind, button) in [
            (MouseEventKind::Press, MouseButton::Left),
            (MouseEventKind::Release, MouseButton::Left),
            (MouseEventKind::Press, MouseButton::WheelUp(1)),
            (MouseEventKind::Press, MouseButton::Right),
        ] {
            assert!(sends(&event(kind, button), false), "{:?} {:?}", kind, button);
            assert!(sends(&event(kind, button), true), "{:?} {:?}", kind, button);
        }
    }
}
