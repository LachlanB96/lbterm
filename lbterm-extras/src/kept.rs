//! LBTerm: what a frame laid out, kept for the next (lbterm/PERF.md, "What the cards cost"). The
//! cards, menus and lists LBTerm draws itself put each of their words through wezterm-gui's
//! `compute_element`, which shapes it with HarfBuzz and looks up every glyph each time it's asked,
//! with no cache of its own: so a card open while anything else painted (a Claude session
//! streaming in the open tab, 60 to 120 frames a second) shaped every one of its words again on
//! every frame, though none of them had changed (the projects card's 150 to 250, the objectives
//! list's two a row, the InMail list's four). [`Kept`] holds what the last frame laid out by what
//! it was laid out from (a word's text, colour, place and font), so a frame lays out only what's
//! new and takes the rest as it was; what a frame didn't ask for goes, so it never holds more
//! than one frame's. Anything that changes every word at once (the fonts, the shape generation,
//! the window's size) is its [`Kept::at`], which drops everything when it moves.
//!
//! Every window's cards count into [`WORDS`]: how many words were laid out and how many were
//! taken as they were, which `metrics` replies with under `words` (lbterm/PERF.md, "The reply").

use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// How many words the cards laid out and how many they took from the frame before, since the
/// process started or `metrics` last reset them: a relaxed atomic add a frame each
#[derive(Default)]
pub struct Tally {
    laid_out: AtomicU64,
    kept: AtomicU64,
}

impl Tally {
    pub const fn new() -> Self {
        Self {
            laid_out: AtomicU64::new(0),
            kept: AtomicU64::new(0),
        }
    }

    /// One frame's words of one card: `laid_out` of them new, `kept` as the frame before had them
    pub fn add(&self, laid_out: u64, kept: u64) {
        if laid_out > 0 {
            self.laid_out.fetch_add(laid_out, Relaxed);
        }
        if kept > 0 {
            self.kept.fetch_add(kept, Relaxed);
        }
    }

    /// (laid out, kept), zeroed as they're read with `reset`
    pub fn read(&self, reset: bool) -> (u64, u64) {
        if reset {
            (self.laid_out.swap(0, Relaxed), self.kept.swap(0, Relaxed))
        } else {
            (self.laid_out.load(Relaxed), self.kept.load(Relaxed))
        }
    }
}

/// Every window's cards' words
pub static WORDS: Tally = Tally::new();

/// Every window's parts kept as quads (wezterm-gui's render/kept_quads.rs: the tab panel's ring,
/// glow and lights): how many were made and how many copied as they were
pub static QUADS: Tally = Tally::new();

/// What `metrics` replies with under `words`
pub fn report_json(reset: bool) -> Value {
    let (laid_out, kept) = WORDS.read(reset);
    json!({ "laid_out": laid_out, "kept": kept })
}

/// What `metrics` replies with under `quads`
pub fn quads_report_json(reset: bool) -> Value {
    let (made, kept) = QUADS.read(reset);
    json!({ "made": made, "kept": kept })
}

/// What one frame laid out of something drawn each frame (a card's words), by what each was laid
/// out from, kept for the next. A frame takes each of its words with [`Kept::take`] (the last
/// frame's, or laid out now), draws them and hands them back with [`Kept::keep`]; whatever it
/// didn't take goes then.
pub struct Kept<A, K, V> {
    /// What everything kept was laid out at (the shape generation, the window's size and DPI):
    /// anything else drops it all
    at: Option<A>,
    last: HashMap<K, V>,
    /// This frame's so far: laid out, and taken from the last
    laid_out: u64,
    kept: u64,
}

impl<A, K, V> Default for Kept<A, K, V> {
    fn default() -> Self {
        Self {
            at: None,
            last: HashMap::new(),
            laid_out: 0,
            kept: 0,
        }
    }
}

impl<A: PartialEq, K: Hash + Eq, V> Kept<A, K, V> {
    /// This frame's words are laid out at `at`: what was kept for any other goes
    pub fn at(&mut self, at: A) {
        if self.at.as_ref() != Some(&at) {
            self.last.clear();
            self.at = Some(at);
        }
        self.laid_out = 0;
        self.kept = 0;
    }

    /// The word laid out from `key`: the last frame's when it laid one out from the same, else
    /// `make`'s
    pub fn take<E>(&mut self, key: &K, make: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        if let Some(v) = self.last.remove(key) {
            self.kept += 1;
            return Ok(v);
        }
        let v = make()?;
        self.laid_out += 1;
        Ok(v)
    }

    /// This frame's words, drawn, kept for the next frame; what it didn't take goes. Its counts go
    /// to `tally`.
    pub fn keep(&mut self, words: impl IntoIterator<Item = (K, V)>, tally: &Tally) {
        self.last.clear();
        self.last.extend(words);
        tally.add(self.laid_out, self.kept);
        self.laid_out = 0;
        self.kept = 0;
    }

    /// Nothing kept: what it was laid out in has gone (the glyph atlas a lost device took)
    pub fn forget(&mut self) {
        self.last.clear();
        self.at = None;
    }

    /// How many words are kept
    pub fn len(&self) -> usize {
        self.last.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// One frame of words: each laid out by `made` when it isn't kept
    fn frame(kept: &mut Kept<u32, String, String>, at: u32, words: &[&str], made: &Cell<u32>, tally: &Tally) -> Vec<String> {
        kept.at(at);
        let drawn: Vec<(String, String)> = words
            .iter()
            .map(|w| {
                let key = w.to_string();
                let v = kept
                    .take(&key, || {
                        made.set(made.get() + 1);
                        Ok::<_, ()>(format!("laid out {w}"))
                    })
                    .unwrap();
                (key, v)
            })
            .collect();
        let out = drawn.iter().map(|(_, v)| v.clone()).collect();
        kept.keep(drawn, tally);
        out
    }

    #[test]
    fn a_word_is_laid_out_once_while_it_shows() {
        let tally = Tally::new();
        let mut kept = Kept::default();
        let made = Cell::new(0);
        let first = frame(&mut kept, 1, &["Projects", "Terminal", "2 mail"], &made, &tally);
        assert_eq!(made.get(), 3);
        assert_eq!(first[1], "laid out Terminal");
        // The same words again: nothing laid out, all taken as they were
        let again = frame(&mut kept, 1, &["Projects", "Terminal", "2 mail"], &made, &tally);
        assert_eq!((made.get(), again), (3, first));
        assert_eq!(tally.read(false), (3, 3));
        // One changes: only it is laid out
        frame(&mut kept, 1, &["Projects", "Terminal", "3 mail"], &made, &tally);
        assert_eq!(made.get(), 4);
        assert_eq!(tally.read(true), (4, 5), "a reset reads them");
        assert_eq!(tally.read(false), (0, 0), "and zeroes them");
    }

    #[test]
    fn what_a_frame_doesnt_show_goes() {
        let tally = Tally::new();
        let mut kept = Kept::default();
        let made = Cell::new(0);
        frame(&mut kept, 1, &["a", "b", "c"], &made, &tally);
        frame(&mut kept, 1, &["a"], &made, &tally);
        assert_eq!(kept.len(), 1, "only the last frame's");
        frame(&mut kept, 1, &["a", "b"], &made, &tally);
        assert_eq!(made.get(), 4, "b was gone, so laid out again");
    }

    #[test]
    fn a_new_place_to_lay_out_at_drops_everything() {
        let tally = Tally::new();
        let mut kept = Kept::default();
        let made = Cell::new(0);
        frame(&mut kept, 1, &["a", "b"], &made, &tally);
        frame(&mut kept, 2, &["a", "b"], &made, &tally);
        assert_eq!(made.get(), 4, "another shape generation or window size lays out all of it again");
        kept.forget();
        frame(&mut kept, 2, &["a"], &made, &tally);
        assert_eq!(made.get(), 5, "nothing kept once it's forgotten");
    }

    #[test]
    fn a_word_shown_twice_in_a_frame_is_laid_out_for_the_second() {
        let tally = Tally::new();
        let mut kept = Kept::default();
        let made = Cell::new(0);
        frame(&mut kept, 1, &["x", "x"], &made, &tally);
        assert_eq!(made.get(), 2);
        // One of them is kept (the same key twice keeps one): the second is laid out again
        frame(&mut kept, 1, &["x", "x"], &made, &tally);
        assert_eq!(made.get(), 3);
    }

    #[test]
    fn a_failed_lay_out_keeps_nothing_for_it() {
        let mut kept: Kept<u32, &str, u32> = Kept::default();
        kept.at(1);
        assert_eq!(kept.take(&"a", || Err::<u32, &str>("no font")), Err("no font"));
        kept.keep(vec![], &Tally::new());
        assert_eq!(kept.take(&"a", || Ok::<u32, ()>(7)), Ok(7));
    }
}
