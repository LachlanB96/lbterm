//! A conversation's lists in pieces a snapshot shares (lbterm/SCROLL-STRIP.md, "How it's
//! built"). Each time a transcript grew, the reading thread used to copy every block's pointer,
//! build every mark and index every line of the conversation again, and then compare the whole
//! of it with the snapshot before under the lock each paint of the strip takes: about 1.2 ms an
//! append on a 5,800-row conversation, more the longer it got. Now the blocks, their marks and
//! where to find each are kept in pieces of [`CHUNK`] blocks: a snapshot copies the pieces'
//! pointers, and an append builds again only the piece it's in, so it costs what it adds.
//!
//! [`Found`] is a piece's index of what a row of the screen can be: a line by its key, Claude's
//! words by their first line's, a prompt by its key or its words. Each is found by the exact
//! words a match must start with, which the matching rules allow (see each kind's `find_*`): a
//! row's lookup is a few hash lookups a piece, where it was a pass over every block and every
//! line sharing the row's first six letters (2.9 ms a look on the long stand-in, up to eight
//! looks a second while Claude Code draws).

use super::{Block, BlockKind, Event};
use std::sync::Arc;

/// How many blocks a piece holds
pub const CHUNK: usize = 128;

/// A list in pieces of [`CHUNK`], each shared between snapshots until something in it changes
#[derive(Debug, Clone)]
pub struct Chunks<T> {
    pieces: Vec<Arc<Vec<T>>>,
    len: usize,
}

impl<T> Default for Chunks<T> {
    fn default() -> Self {
        Self { pieces: vec![], len: 0 }
    }
}

pub type ChunkIter<'a, T> =
    std::iter::FlatMap<std::slice::Iter<'a, Arc<Vec<T>>>, std::slice::Iter<'a, T>, fn(&'a Arc<Vec<T>>) -> std::slice::Iter<'a, T>>;

impl<T: Clone> Chunks<T> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.pieces.get(i / CHUNK)?.get(i % CHUNK)
    }

    pub fn iter(&self) -> ChunkIter<'_, T> {
        self.pieces.iter().flat_map(|p| p.iter())
    }

    /// The items from `from` on
    pub fn iter_from(&self, from: usize) -> impl Iterator<Item = &T> + '_ {
        let first = from / CHUNK;
        self.pieces
            .iter()
            .skip(first)
            .enumerate()
            .flat_map(move |(n, p)| p.iter().skip(if n == 0 { from % CHUNK } else { 0 }))
    }

    pub fn push(&mut self, item: T) {
        if self.len % CHUNK == 0 {
            self.pieces.push(Arc::new(Vec::with_capacity(CHUNK)));
        }
        if let Some(last) = self.pieces.last_mut() {
            Arc::make_mut(last).push(item);
        }
        self.len += 1;
    }

    /// Item `i` to change: its piece is copied first while a snapshot holds it
    pub fn make_mut(&mut self, i: usize) -> &mut T {
        &mut Arc::make_mut(&mut self.pieces[i / CHUNK])[i % CHUNK]
    }

    /// How many pieces there are
    pub fn pieces(&self) -> usize {
        self.pieces.len()
    }

    /// Piece `c`'s items
    pub fn piece(&self, c: usize) -> &[T] {
        self.pieces.get(c).map_or(&[], |p| p.as_slice())
    }

    /// Whether piece `c` is the same, shared, in both
    pub fn shares_piece(&self, other: &Self, c: usize) -> bool {
        matches!((self.pieces.get(c), other.pieces.get(c)), (Some(a), Some(b)) if Arc::ptr_eq(a, b))
    }
}

impl<U> Chunks<Arc<U>> {
    /// The first place where `self` and `other` hold another item, by its address, or where the
    /// shorter ends: what changed between two snapshots, found without looking at what the items
    /// hold
    pub fn first_difference(&self, other: &Self) -> usize {
        for (c, (a, b)) in self.pieces.iter().zip(&other.pieces).enumerate() {
            if Arc::ptr_eq(a, b) {
                continue;
            }
            match a.iter().zip(b.iter()).position(|(x, y)| !Arc::ptr_eq(x, y)) {
                Some(i) => return c * CHUNK + i,
                None if a.len() == b.len() => continue,
                None => return c * CHUNK + a.len().min(b.len()),
            }
        }
        self.len.min(other.len)
    }
}

impl<T> std::ops::Index<usize> for Chunks<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.pieces[i / CHUNK][i % CHUNK]
    }
}

impl<'a, T: Clone> IntoIterator for &'a Chunks<T> {
    type Item = &'a T;
    type IntoIter = ChunkIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T: Clone + PartialEq> PartialEq for Chunks<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

/// Every mark, in the order Claude Code draws them, in pieces: piece `c` has the marks on blocks
/// `c * CHUNK` to `(c + 1) * CHUNK`
#[derive(Debug, Clone, Default)]
pub struct Events {
    pieces: Vec<Arc<Vec<Event>>>,
    /// Where each piece's first mark is in the whole list
    starts: Vec<usize>,
    len: usize,
}

impl Events {
    pub fn new(pieces: Vec<Arc<Vec<Event>>>) -> Self {
        let mut starts = Vec::with_capacity(pieces.len());
        let mut len = 0;
        for p in &pieces {
            starts.push(len);
            len += p.len();
        }
        Self { pieces, starts, len }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<&Event> {
        let c = self.starts.partition_point(|&s| s <= i).checked_sub(1)?;
        // A piece without marks starts where the next does: the last piece starting at or
        // before `i` that has it
        let c = (0..=c).rev().find(|&c| i - self.starts[c] < self.pieces[c].len())?;
        self.pieces[c].get(i - self.starts[c])
    }

    pub fn iter(&self) -> ChunkIter<'_, Event> {
        self.pieces.iter().flat_map(|p| p.iter())
    }
}

impl std::ops::Index<usize> for Events {
    type Output = Event;
    fn index(&self, i: usize) -> &Event {
        self.get(i).expect("a mark past the end")
    }
}

impl<'a> IntoIterator for &'a Events {
    type Item = &'a Event;
    type IntoIter = ChunkIter<'a, Event>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl PartialEq for Events {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().eq(other.iter())
    }
}

/// What a [`Found`] entry is found by
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum By {
    /// A line's key, exactly
    Line = 1,
    /// Claude's words' key: its first [`TEXT_HEAD`] letters, or all of a shorter one
    Text,
    /// A prompt's key, exactly
    Prompt,
    /// A prompt's key's first [`PROMPT_HEAD`] characters, for one that long
    PromptHead,
    /// A prompt's words on one line, exactly
    Whole,
    /// Their first [`PROMPT_HEAD`] characters, for words that long
    WholeHead,
}

/// How much of Claude's words' key a row must show to be their start: a row cut by the wrap is
/// found from this many letters on (conversation.rs's `MIN_MATCH`)
pub const TEXT_HEAD: usize = 10;
/// How much of a prompt a row cut short with "…" must show to be it (conversation.rs's
/// `prompt_matches`, the header's `header_names`)
pub const PROMPT_HEAD: usize = 12;

/// `text`'s first `n` characters
pub fn head(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// FNV-1a over what it's found by and the words
pub fn hash(by: By, text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in std::iter::once(by as u8).chain(text.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Where to find what a screen row can be, in one piece of the blocks: each [`By`] and its words'
/// hash with a block (and line) it can be, sorted, so a lookup is a binary search and making one
/// allocates once. A hash says only where to look: each is checked against the block's own words.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    entries: Vec<(u64, u32, u32)>,
}

impl Found {
    /// The piece of `blocks` that starts at block `first`
    pub fn of(blocks: &[Arc<Block>], first: usize) -> Self {
        let mut found = Found::default();
        found.entries.reserve(blocks.iter().map(|b| b.line_keys.len() + 4).sum());
        for (i, b) in blocks.iter().enumerate() {
            let at = (first + i) as u32;
            for (k, key) in b.line_keys.iter().enumerate() {
                if !key.is_empty() {
                    found.add(hash(By::Line, key), at, k as u32);
                }
            }
            let Some(key) = b.key.as_deref().filter(|k| !k.is_empty()) else {
                continue;
            };
            match b.kind {
                BlockKind::Text => found.add(hash(By::Text, head(key, TEXT_HEAD)), at, 0),
                BlockKind::Prompt => {
                    found.add(hash(By::Prompt, key), at, 0);
                    if key.chars().count() >= PROMPT_HEAD {
                        found.add(hash(By::PromptHead, head(key, PROMPT_HEAD)), at, 0);
                    }
                    found.add(hash(By::Whole, &b.text), at, 0);
                    if b.text.chars().count() >= PROMPT_HEAD {
                        found.add(hash(By::WholeHead, head(&b.text, PROMPT_HEAD)), at, 0);
                    }
                }
                _ => {}
            }
        }
        // In order of block and line for each hash, a block found twice the same way once
        found.entries.sort_unstable();
        found.entries.dedup();
        found
    }

    fn add(&mut self, h: u64, block: u32, line: u32) {
        self.entries.push((h, block, line));
    }

    /// The blocks and lines found by `h`, in order
    pub fn get(&self, h: u64) -> impl Iterator<Item = (u32, u32)> + '_ {
        let from = self.entries.partition_point(|e| e.0 < h);
        self.entries[from..].iter().take_while(move |e| e.0 == h).map(|e| (e.1, e.2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_in_pieces_is_shared_but_for_the_piece_that_changed() {
        let mut list: Chunks<Arc<usize>> = Chunks::default();
        for i in 0..CHUNK * 2 + 5 {
            list.push(Arc::new(i));
        }
        assert_eq!(list.len(), CHUNK * 2 + 5);
        assert_eq!((list.pieces(), *list[CHUNK + 3], **list.get(CHUNK * 2 + 4).unwrap()), (3, CHUNK + 3, CHUNK * 2 + 4));
        assert_eq!(list.iter().map(|x| **x).collect::<Vec<_>>(), (0..CHUNK * 2 + 5).collect::<Vec<_>>());
        assert_eq!(list.iter_from(CHUNK - 1).map(|x| **x).take(3).collect::<Vec<_>>(), vec![CHUNK - 1, CHUNK, CHUNK + 1]);
        assert_eq!(list.iter_from(CHUNK * 2 + 5).count(), 0);
        let held = list.clone();
        assert_eq!(list.first_difference(&held), list.len());
        // An item pushed: only the last piece is copied, and the difference is where it went
        list.push(Arc::new(999));
        assert!(list.shares_piece(&held, 0) && list.shares_piece(&held, 1) && !list.shares_piece(&held, 2));
        assert_eq!(list.first_difference(&held), CHUNK * 2 + 5);
        // An item changed in the first piece: that piece copied, the others shared
        let held = list.clone();
        *list.make_mut(7) = Arc::new(70);
        assert!(!list.shares_piece(&held, 0) && list.shares_piece(&held, 1));
        assert_eq!((list.first_difference(&held), held.first_difference(&list)), (7, 7));
        assert_eq!(*held[7], 7, "what a snapshot holds doesn't change");
    }

    #[test]
    fn marks_in_pieces_are_one_list() {
        let mark = |block: usize| Event { kind: super::super::Kind::Edit, block, at_ms: 0, text: String::new() };
        let events = Events::new(vec![
            Arc::new(vec![mark(0), mark(1)]),
            Arc::new(vec![]),
            Arc::new(vec![mark(CHUNK * 2), mark(CHUNK * 2 + 1), mark(CHUNK * 2 + 2)]),
            Arc::new(vec![]),
        ]);
        assert_eq!(events.len(), 5);
        assert_eq!(events.iter().map(|e| e.block).collect::<Vec<_>>(), vec![0, 1, CHUNK * 2, CHUNK * 2 + 1, CHUNK * 2 + 2]);
        for i in 0..5 {
            assert_eq!(events.get(i), events.iter().nth(i));
        }
        assert_eq!(events.get(5), None);
        assert_eq!(Events::default().get(0), None);
    }

    #[test]
    fn a_head_is_its_first_characters() {
        assert_eq!(head("message number 1", 7), "message");
        assert_eq!(head("short", 12), "short");
        assert_eq!(head("\u{276f}\u{276f}ab", 3), "\u{276f}\u{276f}a");
        assert_ne!(hash(By::Line, "abc"), hash(By::Text, "abc"));
    }
}
