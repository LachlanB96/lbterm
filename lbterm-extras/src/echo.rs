//! Whether a key sent to a pane asks for a frame of its own (lbterm/PERF.md, finding 35).
//!
//! Upstream asks for a frame on every key it sends a pane, down and up, before the pane has
//! echoed it (`termwindow/keyevent.rs`). For a key typed at a shell or into Claude Code that frame
//! draws nothing new: what the key does shows when the pane prints its echo, whose own
//! notification asks for the frame that draws it. Since main's perf/frame-loop the next frame
//! after any paint is held back one period from that paint's start (`pace`, a refresh of a 60 Hz
//! screen, 16.7 ms, or `max_fps`' 8.3 ms on a faster one), so the key's own frame held the echo's
//! back a period though the echo was in well before it ended: 21 to 28 ms to show an echo that
//! came in 5 to 13.
//!
//! So a key asks for no frame of its own when nothing LBTerm draws itself changes with it and its
//! pane says when what it shows changes. It still asks when:
//! - it went to an overlay (copy mode, search, quick select, the launcher, any pane or tab
//!   overlay), which draws what the key does itself and says nothing of it;
//! - it scrolled the pane back to the bottom (`scroll_to_bottom_on_input`), which is the GUI's
//!   own viewport;
//! - its pane draws a guess at the echo itself (a mux client pane's local echo, which upstream
//!   turns on when its server answers slowly: `local_echo_threshold_ms`);
//! - its pane isn't one known to say when its output changes it (a local pane's parser and a mux
//!   client pane's renderer both notify `PaneOutput`; anything else might not).
//!
//! Everything else a key changes that LBTerm draws asks for its own frame where it changes: the
//! held modifiers' outline (`termwindow/held.rs`), a dead key's composing text
//! (`AdviseDeadKeyStatus`), a leader key's end, a key assignment (a tab switch, a split, a font
//! size), the menus and the Conductor's view that take keys before a pane does.
//!
//! But not a dead key's or an IME's composition ending (`composition_frame`). Windows says it
//! ends just before it sends the composed character, in the same message, so its frame would come
//! first and draw nothing the character's echo won't (the composing text at the cursor gone, where
//! the echo draws the character), and hold the echo's frame a pacing period behind it, as the
//! key's own frame did. The echo's frame clears it; a frame is asked for a while on
//! (`config::lbterm::COMPOSITION_CLEAR_WAIT`) only when none has painted by then: a character the
//! pane doesn't echo, a composition cancelled.

/// What a key sent to a pane did, as far as its frame goes
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeySent {
    /// It went to an overlay over the pane, not the pane itself
    pub overlay: bool,
    /// The pane's viewport moved with it (scrolled back to the bottom)
    pub scrolled: bool,
    /// The pane draws a guess at the key's echo itself, at once
    pub predicts: bool,
    /// The pane says when what it shows changes (`MuxNotification::PaneOutput`)
    pub tells_output: bool,
}

/// Whether the key asks for a frame of its own: only when that frame would draw something new.
/// Otherwise the pane's echo, when it comes, asks for the frame that shows it, and that frame
/// isn't held back behind one that drew nothing.
pub fn key_frame(key: KeySent) -> bool {
    key.overlay || key.scrolled || key.predicts || !key.tells_output
}

/// Whether a dead key's or an IME's composition going from `was_composing` to `composing` asks
/// for a frame at once: its text shown or changed does; its end waits for the composed
/// character's echo to paint it (the module's doc)
pub fn composition_frame(was_composing: bool, composing: bool) -> bool {
    composing || !was_composing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed() -> KeySent {
        KeySent {
            tells_output: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_key_typed_into_a_pane_asks_for_no_frame_of_its_own() {
        // A shell's prompt or Claude Code: the echo asks for the frame that shows it
        assert!(!key_frame(typed()));
    }

    #[test]
    fn a_key_to_an_overlay_paints_at_once() {
        // Copy mode's cursor, search's line, the launcher's filter: drawn by LBTerm, said by
        // nobody
        assert!(key_frame(KeySent {
            overlay: true,
            ..typed()
        }));
    }

    #[test]
    fn a_key_that_scrolled_back_to_the_bottom_paints_at_once() {
        assert!(key_frame(KeySent {
            scrolled: true,
            ..typed()
        }));
    }

    #[test]
    fn a_pane_that_guesses_its_echo_paints_the_guess_at_once() {
        assert!(key_frame(KeySent {
            predicts: true,
            ..typed()
        }));
    }

    #[test]
    fn a_composition_shown_paints_at_once_and_its_end_waits_for_the_echo() {
        // A dead key pressed (´ at the cursor), an IME's text changing
        assert!(composition_frame(false, true));
        assert!(composition_frame(true, true));
        // Its end, the composed character on its way to the pane
        assert!(!composition_frame(true, false));
        // Nothing was composing: as upstream, a frame
        assert!(composition_frame(false, false));
    }

    #[test]
    fn a_pane_that_may_not_say_when_it_changes_paints_at_once() {
        assert!(key_frame(KeySent::default()));
        assert!(key_frame(KeySent {
            tells_output: false,
            ..typed()
        }));
    }
}
