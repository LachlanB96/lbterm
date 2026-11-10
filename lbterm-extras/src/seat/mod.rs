//! The Conductor's seat (Lachlan's ~\AI\Conductor, its SEAT.md): one long-lived process that holds
//! the Conductor's Claude and serves every interface on a named pipe. LBTerm draws the Conductor's
//! view itself (lbterm/TAB-PANEL.md, "The Conductor's view"), and this is the part of it tested
//! without a window:
//!
//! - [`protocol`]: the seat's lines (`seat.view`, `seat.items`, `seat.delta`, `seat.show`,
//!   `seat.reply`) read, LBTerm's commands written, and the conversation kept as the seat sends it.
//! - [`text`]: the conversation's words wrapped to a width, with the light markdown the
//!   Conductor's replies use (paragraphs, lists, bold, code, links).
//! - [`words`]: what the seat's words say, read for the view: a hub wake's events, a notice that
//!   a session needs him, an event's words without the session it names.
//! - [`input`]: a box being typed in: the caret, a selection, wrapping.
//! - [`layout`]: where everything in the view goes, as a list of rectangles and words for the
//!   GUI to draw and the places a click lands on.
//! - [`link`]: the connection to the seat, its lines both ways on one thread, every wait with an
//!   end, and the commands waiting for their replies.
//! - [`Views`]: the windows whose view is open, the only ones what the seat says repaints.
//!
//! The GUI side is wezterm-gui/src/seat_client.rs (the pipe) and
//! termwindow/render/conductor_view.rs (drawing it, the mouse and the keys).

pub mod input;
pub mod layout;
#[cfg(windows)]
pub mod link;
pub mod protocol;
pub mod text;
pub mod words;

/// The windows whose Conductor's view is open, by whatever names a window (the GUI's mux window
/// ids). Nothing but the view shows what the seat says (the pad's asks are its state file's), so a
/// line from the seat repaints these windows and no others, and with none open asks nothing of
/// the GUI at all: until 2026-10-30 every line repainted every window, view or none, a frame for
/// each new view (every session's status, the hub's guesses) and ten a second while the
/// Conductor wrote a reply.
#[derive(Debug, Default)]
pub struct Views<W> {
    open: Vec<W>,
}

impl<W: PartialEq + Copy> Views<W> {
    pub const fn new() -> Views<W> {
        Views { open: Vec::new() }
    }

    pub fn open(&mut self, window: W) {
        if !self.open.contains(&window) {
            self.open.push(window);
        }
    }

    pub fn close(&mut self, window: W) {
        self.open.retain(|w| *w != window);
    }

    /// Whether any window has it open: none, and the seat's lines repaint nothing
    pub fn any(&self) -> bool {
        !self.open.is_empty()
    }

    /// Of the windows still there (`alive`), those with the view open, to repaint for what the
    /// seat said; a window gone with it open is forgotten
    pub fn to_repaint(&mut self, alive: &[W]) -> Vec<W> {
        self.open.retain(|w| alive.contains(w));
        self.open.clone()
    }
}

#[cfg(test)]
mod test {
    use super::Views;

    #[test]
    fn what_the_seat_says_repaints_only_the_windows_with_the_view_open() {
        let mut views = Views::new();
        assert!(!views.any(), "none open: a line from the seat asks nothing of the GUI");
        assert_eq!(views.to_repaint(&[1, 2, 3]), Vec::<u32>::new());
        views.open(2);
        views.open(2);
        assert!(views.any());
        assert_eq!(views.to_repaint(&[1, 2, 3]), [2], "only the window showing it, once");
        views.open(3);
        views.close(2);
        assert_eq!(views.to_repaint(&[1, 2, 3]), [3]);
        // Its window went with the view open: forgotten, and nothing is asked again
        assert_eq!(views.to_repaint(&[1, 2]), Vec::<u32>::new());
        assert!(!views.any());
    }
}
