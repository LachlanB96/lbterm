//! Links drawn by the state of what they point at: a link to an InMail message, in the waiting
//! orange while it waits and grey, struck through, once it's gone (`inbox.rs`), and an image's
//! caption, struck through once its file has gone (`thumbnails.rs`). How a line looks is shaped
//! once and kept, so a line holding one of these is shaped again when those states change, and
//! every other line keeps its shapes. Until 2026-10-29 each change shaped every line of every
//! pane in every window again.
//!
//! A line holding links looks different with the pointer on one of them (it's underlined), so the
//! line caches keep the links each line holds and lay a line out again only when which of its
//! own links is under the pointer changes ([`hover_changed`]).

use std::sync::Arc;

/// Whether `uri` is a link drawn by the state of what it points at
pub fn by_state(uri: &str) -> bool {
    uri.starts_with(crate::inbox::LINK) || uri.starts_with(crate::thumbnails::LINK)
}

/// The shape hash of a line holding such a link, with the states' `generation` in it: each new
/// generation is a new hash, so the line is shaped again
pub fn shape_hash(hash: [u8; 16], generation: u64) -> [u8; 16] {
    let mut out = hash;
    for (byte, g) in out.iter_mut().zip(generation.to_le_bytes()) {
        *byte ^= g;
    }
    out
}

/// The link under the pointer as a line holding `links` draws it: that link when it's one of
/// them (the same link, not one alike: a link is underlined by being the one the pointer's on,
/// as screen_line asks), else none
fn seen<'a, T>(links: &[Arc<T>], highlight: Option<&'a Arc<T>>) -> Option<&'a Arc<T>> {
    highlight.filter(|h| links.iter().any(|link| Arc::ptr_eq(link, h)))
}

/// Whether a line holding `links`, laid out while `then` was under the pointer, looks different
/// with `now` under it: only when which of its own links the pointer's on has changed. wezterm's
/// own test asked whether `then` and `now` were the same link and took no link at either as a
/// change, so every line holding a link was laid out again on every frame while the pointer was
/// on no link, and all of them whenever it moved from one link to another (lbterm/PERF.md,
/// finding 61)
pub fn hover_changed<T>(links: &[Arc<T>], then: Option<&Arc<T>>, now: Option<&Arc<T>>) -> bool {
    match (seen(links, then), seen(links, now)) {
        (None, None) => false,
        (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
        _ => true,
    }
}

/// `link` added to `links` unless it's there already (the same link)
pub fn add_link<T>(links: &mut Vec<Arc<T>>, link: &Arc<T>) {
    if !links.iter().any(|l| Arc::ptr_eq(l, link)) {
        links.push(Arc::clone(link));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_line_looks_different_only_when_the_pointer_comes_onto_or_off_its_own_links() {
        let (a, b, c) = (Arc::new("a"), Arc::new("b"), Arc::new("c"));
        let line = vec![Arc::clone(&a), Arc::clone(&b)];
        // The pointer on no link, before and now: the same look (wezterm's test said changed)
        assert!(!hover_changed(&line, None, None));
        // Onto one of its links, off it, from one of its links to the other
        assert!(hover_changed(&line, None, Some(&a)));
        assert!(hover_changed(&line, Some(&a), None));
        assert!(hover_changed(&line, Some(&a), Some(&b)));
        assert!(!hover_changed(&line, Some(&b), Some(&b)));
        // A link elsewhere, coming or going or another: this line looks the same
        assert!(!hover_changed(&line, None, Some(&c)));
        assert!(!hover_changed(&line, Some(&c), None));
        assert!(!hover_changed(&line, Some(&c), Some(&Arc::new("d"))));
        // Off its own onto one elsewhere: its link isn't underlined any more
        assert!(hover_changed(&line, Some(&a), Some(&c)));
        // A link alike but not the same isn't the one under the pointer
        assert!(!hover_changed(&line, None, Some(&Arc::new("a"))));
        // A line with no links never changes
        assert!(!hover_changed::<&str>(&[], Some(&a), Some(&b)));
    }

    #[test]
    fn a_link_is_added_once() {
        let (a, b) = (Arc::new(1), Arc::new(1));
        let mut links = vec![];
        add_link(&mut links, &a);
        add_link(&mut links, &a);
        add_link(&mut links, &b);
        assert_eq!(links.len(), 2, "the same link once, one alike but not the same apart");
    }

    #[test]
    fn inmail_and_image_links_are_drawn_by_state() {
        assert!(by_state("lbterm://inmail/Terminal?message=2026-10-29-1201-x.md"));
        assert!(by_state("lbterm://image/C:/Users/x/shot.png?row=2&of=5"));
        assert!(!by_state("https://lachlanb.com"));
        assert!(!by_state("lbterm://session/6f1c0d4e-0000-0000-0000-000000000000"));
    }

    #[test]
    fn each_new_generation_shapes_such_a_line_again() {
        let hash = [7u8; 16];
        // The first generation leaves it as it was
        assert_eq!(shape_hash(hash, 0), hash);
        assert_ne!(shape_hash(hash, 1), hash);
        assert_ne!(shape_hash(hash, 1), shape_hash(hash, 2));
        // The same generation, the same hash: shaped once
        assert_eq!(shape_hash(hash, 3), shape_hash(hash, 3));
    }
}
