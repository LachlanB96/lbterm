//! LBTerm: what a mux server's push of a pane reads of it, in one go.
//!
//! `PerPane::compute_changes` (wezterm-mux-server-impl's sessionhandler.rs) compares a pane's
//! state with what its client was last sent, at every push it owes, every key's answer and every
//! poll. It asked a getter at a time, each taking the pane's terminal lock (seven of them, and
//! the rows changed an eighth), while the pane's parser takes the same lock for each batch it
//! applies: under a flood each could wait out a whole batch, and under mux-flood's alerts flood
//! the waits were 85% of a push (lbterm/FLOWS.md, the pane-output flow, rows 27 and 28; perf/
//! mux-client's flood trace, run 6), and what they read together wasn't one state (PITFALLS 86).
//! A local pane now reads all of it under one lock ([`from_terminal`]), the seqno and the rows
//! changed since the last push with it. What isn't the terminal's is asked after the lock is let
//! go: the foreground process's name, for a pane whose title is still the default, and the
//! foreground's cwd, for one whose shell says none (OSC 7), which a push takes as last listed and
//! never reads there and then (localpane.rs's `lbterm_read_for_push`, lbterm_proclist.rs's
//! `Ask::Listed`). Any other pane answers getter by getter, as before ([`by_getters`]).

use crate::pane::{CachePolicy, Pane};
use crate::renderable::{
    terminal_get_cursor_position, terminal_get_dimensions, terminal_get_dirty_lines,
    RenderableDimensions, StableCursorPosition,
};
use rangeset::RangeSet;
use termwiz::surface::SequenceNo;
use url::Url;
use wezterm_term::{StableRowIndex, Terminal};

/// A pane as a push reads it: what it compares with what its client was last sent
#[derive(Debug, Clone, PartialEq)]
pub struct PushRead {
    pub mouse_grabbed: bool,
    pub alt_screen_active: bool,
    pub dimensions: RenderableDimensions,
    pub cursor_position: StableCursorPosition,
    pub title: String,
    pub working_dir: Option<Url>,
    /// The terminal's seqno as the rest was read
    pub seqno: SequenceNo,
    /// The rows in the scrollback and the viewport changed since the seqno the push asked from
    pub changed: RangeSet<StableRowIndex>,
}

/// What a push of a pane out of its client's view reads of it when the server holds its lines
/// (`Pane::lbterm_read_names`; wezterm-mux-server-impl's `PerPane::lbterm_out_of_view`): its
/// title and cwd, read as [`PushRead`] reads them, and its size with them, under the same lock,
/// since the push of them alone carries the size its client was last sent and so mustn't go once
/// that's no longer the pane's. No rows: after a long hold they would walk every line's seqno
#[derive(Debug, Clone, PartialEq)]
pub struct Names {
    pub title: String,
    pub working_dir: Option<Url>,
    pub dimensions: RenderableDimensions,
}

/// The rows a push looks at for changes: the scrollback and the viewport, as upstream's
/// compute_changes asked
fn rows(dims: &RenderableDimensions) -> std::ops::Range<StableRowIndex> {
    0..dims.physical_top + dims.viewport_rows as StableRowIndex
}

/// A terminal as a push reads it, under the one lock its caller holds: its own title and its own
/// cwd (OSC 7), which the pane makes its processes' after the lock (localpane.rs). `tmux`: the
/// pane runs tmux control mode, whose own pane shows no mouse grab, no alternate screen and no
/// cursor, as its getters say
pub fn from_terminal(terminal: &mut Terminal, tmux: bool, since: SequenceNo) -> PushRead {
    let dimensions = terminal_get_dimensions(terminal);
    let mut cursor_position = terminal_get_cursor_position(terminal);
    if tmux {
        cursor_position.visibility = termwiz::surface::CursorVisibility::Hidden;
    }
    let changed = terminal_get_dirty_lines(terminal, rows(&dimensions), since);
    PushRead {
        mouse_grabbed: !tmux && terminal.is_mouse_grabbed(),
        alt_screen_active: !tmux && terminal.is_alt_screen_active(),
        dimensions,
        cursor_position,
        title: terminal.get_title().to_string(),
        working_dir: terminal.get_current_dir().cloned(),
        seqno: terminal.current_seqno(),
        changed,
    }
}

/// A pane as a push reads it, a getter at a time, in the order upstream's compute_changes asked:
/// any pane without a terminal of its own to read under one lock
pub fn by_getters<P: Pane + ?Sized>(pane: &P, since: SequenceNo) -> PushRead {
    let mouse_grabbed = pane.is_mouse_grabbed();
    let alt_screen_active = pane.is_alt_screen_active();
    let dimensions = pane.get_dimensions();
    let cursor_position = pane.get_cursor_position();
    let title = pane.get_title();
    let working_dir = pane.get_current_working_dir(CachePolicy::AllowStale);
    let seqno = pane.get_current_seqno();
    let changed = pane.get_changed_since(rows(&dimensions), since);
    PushRead {
        mouse_grabbed,
        alt_screen_active,
        dimensions,
        cursor_position,
        title,
        working_dir,
        seqno,
        changed,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;
    use wezterm_term::TerminalSize;

    fn terminal() -> Terminal {
        config::use_test_configuration();
        Terminal::new(
            TerminalSize {
                rows: 5,
                cols: 20,
                ..Default::default()
            },
            Arc::new(config::TermConfig::new()),
            "LBTerm",
            "test",
            Box::new(Vec::new()),
        )
    }

    /// The getters a push asked before, each with a lock of its own, on the same terminal
    fn by_each(terminal: &mut Terminal, since: SequenceNo) -> PushRead {
        let dimensions = terminal_get_dimensions(terminal);
        PushRead {
            mouse_grabbed: terminal.is_mouse_grabbed(),
            alt_screen_active: terminal.is_alt_screen_active(),
            cursor_position: terminal_get_cursor_position(terminal),
            title: terminal.get_title().to_string(),
            working_dir: terminal.get_current_dir().cloned(),
            seqno: terminal.current_seqno(),
            changed: terminal_get_dirty_lines(terminal, rows(&dimensions), since),
            dimensions,
        }
    }

    #[test]
    fn one_read_is_what_the_getters_say() {
        let mut t = terminal();
        let first = from_terminal(&mut t, false, 0);
        assert_eq!(first, by_each(&mut t, 0), "a new terminal");

        t.advance_bytes(
            b"\x1b]2;a title\x1b\\\x1b]7;file://host/C:/work\x1b\\one\r\ntwo\r\nthree\x1b[?1000h",
        );
        let read = from_terminal(&mut t, false, first.seqno);
        assert_eq!(read, by_each(&mut t, first.seqno), "after output");
        assert_eq!(read.title, "a title");
        assert!(read.working_dir.is_some(), "its OSC 7 cwd");
        assert!(read.mouse_grabbed, "the program has the mouse");
        assert!(!read.changed.is_empty(), "the rows it printed changed");
        assert!(read.seqno > first.seqno);

        let again = from_terminal(&mut t, false, read.seqno);
        assert!(again.changed.is_empty(), "nothing changed since the last read");
        assert_eq!(again.seqno, read.seqno);
    }

    #[test]
    fn a_tmux_pane_shows_no_mouse_grab_alternate_screen_or_cursor() {
        let mut t = terminal();
        t.advance_bytes(b"\x1b[?1049h\x1b[?1000h");
        let read = from_terminal(&mut t, true, 0);
        assert!(!read.mouse_grabbed);
        assert!(!read.alt_screen_active);
        assert_eq!(
            read.cursor_position.visibility,
            termwiz::surface::CursorVisibility::Hidden
        );
        let local = from_terminal(&mut t, false, 0);
        assert!(local.mouse_grabbed && local.alt_screen_active);
    }
}
