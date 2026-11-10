//! What the scroll strip's card says in place of its marks for a few seconds (lbterm/SCROLL-STRIP.md,
//! "Notes"): why a click on the strip didn't get there, or that a write to the pane didn't
//! (Lachlan's 15b of 2026-11-07: a binding's `SendString`, a voice draft, the strip's own keys, an
//! IME's text, Lua's `pane:send_text`). The GUI side is wezterm-gui's render/scroll_strip.rs; a
//! failed write reaches it through mux's lbterm_writes.rs.

use std::time::Instant;

/// What a note is about, which its head says
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A click whose seek didn't get there, or that had nowhere to go
    NotThere,
    /// A write to the pane that didn't get there
    NotSent,
    /// A write whose connection ended before the server answered: it may have got there
    /// (Lachlan's 4a of 2026-11-08: "Not sent" was wrong for it)
    MayNotHaveArrived,
}

impl Why {
    /// The card's head
    pub fn head(self) -> &'static str {
        match self {
            Why::NotThere => "Not there",
            Why::NotSent => "Not sent",
            Why::MayNotHaveArrived => "May not have arrived",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub why: Why,
    pub text: String,
    /// When it goes
    pub until: Instant,
    /// Level with what it's about, in the window's pixels
    pub at: f64,
    /// The tab it's about. A click's note is the strip's it was clicked on, shown on whichever
    /// strip shows while it lasts, as before (None). A write's is the tab of the pane written
    /// to: the strip stands beside the tab's active pane, or an overlay over it (copy mode,
    /// search), so a write to the other pane of a split, or under an overlay, shows there too,
    /// and a write to a pane in another tab (Lua's `pane:send_text`) says nothing on this one's
    /// and shows if that tab is switched to before it goes
    pub tab: Option<u64>,
}

impl Note {
    pub fn not_there(text: &str, at: f64, until: Instant) -> Note {
        Note { why: Why::NotThere, text: text.to_string(), until, at, tab: None }
    }

    /// A write to a pane of `tab` failed for `failure`, as the log says it ("the mux server
    /// refused it: ..."), said on the card as a sentence, headed "May not have arrived" when
    /// `maybe` it got there anyway (the connection ended before the server answered), else
    /// "Not sent"
    pub fn not_sent(tab: u64, failure: &str, maybe: bool, at: f64, until: Instant) -> Note {
        let mut chars = failure.chars();
        let text = match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => "It didn't reach the pane".to_string(),
        };
        let why = if maybe { Why::MayNotHaveArrived } else { Why::NotSent };
        Note { why, text, until, at, tab: Some(tab) }
    }

    /// Whether it shows at `now` on the strip of the window whose active tab is `tab`
    pub fn shows(&self, tab: Option<u64>, now: Instant) -> bool {
        now < self.until && self.tab.map_or(true, |t| Some(t) == tab)
    }

    pub fn gone(&self, now: Instant) -> bool {
        now >= self.until
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_write_s_note_shows_on_its_own_tab_s_strip_alone_until_it_goes() {
        let now = Instant::now();
        let until = now + Duration::from_secs(4);
        let note = Note::not_sent(7, "the mux server isn't answering", false, 10.0, until);
        assert_eq!(note.why.head(), "Not sent");
        assert_eq!(note.text, "The mux server isn't answering");
        // Tab 7's strip, whichever of its panes (or an overlay over one) the strip stands by
        assert!(note.shows(Some(7), now));
        assert!(!note.shows(Some(8), now), "another tab's strip says nothing of it");
        assert!(!note.shows(None, now));
        assert!(!note.shows(Some(7), until));
        assert!(note.gone(until) && !note.gone(now));
    }

    #[test]
    fn a_click_s_note_shows_on_whichever_strip_shows() {
        let now = Instant::now();
        let note = Note::not_there("Too far to scroll to", 0.0, now + Duration::from_secs(4));
        assert_eq!(note.why.head(), "Not there");
        assert!(note.shows(Some(3), now) && note.shows(None, now));
    }

    #[test]
    fn a_failure_with_no_words_still_says_something() {
        let note = Note::not_sent(1, "", false, 0.0, Instant::now());
        assert!(!note.text.is_empty());
    }

    #[test]
    fn a_write_that_may_have_arrived_says_so_in_its_head() {
        let note = Note::not_sent(3, "the connection to the mux server ended before it answered", true, 0.0, Instant::now());
        assert_eq!(note.why, Why::MayNotHaveArrived);
        assert_eq!(note.why.head(), "May not have arrived");
        assert_eq!(note.text, "The connection to the mux server ended before it answered");
        assert_eq!(note.tab, Some(3));
    }
}
