//! The strip's hover fade (`lbterm_motion.button_hover`, lbterm/DESIGN.md "The window strip").
//!
//! With the compositor, a window button's look under the pointer (or the restart pill's) isn't
//! drawn over its resting look in the strip. It's drawn on a visual of its own, a slot, whose
//! opacity DWM fades, so the fade runs whatever the GUI thread is doing. There are two slots:
//! the button the pointer left fades out on one while the one it reached fades in on the other.
//! At most one slot is lit (fading in, or shown) at a time. The GUI side is
//! wezterm-gui/src/termwindow/render/hover_fade.rs.

/// How many slots there are
pub const SLOTS: usize = 2;

/// What a slot's opacity does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fade {
    /// From wherever it is to fully shown: the same element, the pointer back on it
    In,
    /// From nothing to fully shown: the slot now shows another element
    InFromZero,
    /// From wherever it is to nothing
    Out,
}

/// Which element each slot shows, and which slot is lit.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverSlots<K> {
    /// The element whose hover look is drawn on each slot. It stays after the slot fades out, so
    /// the fade has something to show, until another element takes the slot.
    keys: [Option<K>; SLOTS],
    lit: [bool; SLOTS],
}

impl<K: PartialEq + Clone> Default for HoverSlots<K> {
    fn default() -> Self {
        Self {
            keys: [None, None],
            lit: [false; SLOTS],
        }
    }
}

impl<K: PartialEq + Clone> HoverSlots<K> {
    /// The slots drawing `key`'s hover look
    pub fn slots_for<'a>(&'a self, key: &'a K) -> impl Iterator<Item = usize> + 'a {
        (0..SLOTS).filter(move |&s| self.keys[s].as_ref() == Some(key))
    }

    pub fn is_lit(&self, slot: usize) -> bool {
        self.lit[slot]
    }

    /// What's lit under the pointer: its look fading in or shown (lbterm/STATE.md)
    pub fn lit(&self) -> impl Iterator<Item = &K> + '_ {
        (0..SLOTS).filter(|&s| self.lit[s]).filter_map(|s| self.keys[s].as_ref())
    }

    /// The pointer is over `key` (or over nothing that fades): which slots fade which way.
    /// Nothing, when that's where it already was.
    pub fn hover(&mut self, key: Option<&K>) -> [Option<Fade>; SLOTS] {
        let mut fades = [None; SLOTS];
        let target = key.map(|key| match (0..SLOTS).find(|&s| self.keys[s].as_ref() == Some(key)) {
            Some(slot) => (slot, false),
            None => {
                // An empty slot, else the one not lit: that's the one fading out, if any
                let slot = (0..SLOTS)
                    .find(|&s| self.keys[s].is_none())
                    .or_else(|| (0..SLOTS).find(|&s| !self.lit[s]))
                    .unwrap_or(0);
                self.keys[slot] = Some(key.clone());
                (slot, true)
            }
        });
        for slot in 0..SLOTS {
            match target {
                Some((t, fresh)) if t == slot => {
                    if fresh {
                        self.lit[slot] = true;
                        fades[slot] = Some(Fade::InFromZero);
                    } else if !self.lit[slot] {
                        self.lit[slot] = true;
                        fades[slot] = Some(Fade::In);
                    }
                }
                _ => {
                    if self.lit[slot] {
                        self.lit[slot] = false;
                        fades[slot] = Some(Fade::Out);
                    }
                }
            }
        }
        fades
    }

    /// Everything off at once: the window was resized, or motion is off
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const CLOSE: &str = "close";
    const MAX: &str = "maximize";
    const PILL: &str = "pill";

    #[test]
    fn the_pointer_arriving_lights_a_slot() {
        let mut s = HoverSlots::default();
        assert_eq!(s.hover(Some(&CLOSE)), [Some(Fade::InFromZero), None]);
        assert_eq!(s.slots_for(&CLOSE).collect::<Vec<_>>(), vec![0]);
        assert!(s.is_lit(0));
    }

    #[test]
    fn what_is_lit_is_what_the_pointer_is_on() {
        let mut s = HoverSlots::default();
        assert_eq!(s.lit().count(), 0);
        s.hover(Some(&MAX));
        s.hover(Some(&CLOSE));
        assert_eq!(s.lit().collect::<Vec<_>>(), vec![&CLOSE], "maximize is fading out, not lit");
        s.hover(None);
        assert_eq!(s.lit().count(), 0);
    }

    #[test]
    fn staying_put_changes_nothing() {
        let mut s = HoverSlots::default();
        s.hover(Some(&CLOSE));
        assert_eq!(s.hover(Some(&CLOSE)), [None, None]);
        assert_eq!(s.hover(None), [Some(Fade::Out), None]);
        assert_eq!(s.hover(None), [None, None]);
    }

    #[test]
    fn moving_across_fades_one_out_as_the_next_fades_in() {
        let mut s = HoverSlots::default();
        s.hover(Some(&MAX));
        assert_eq!(s.hover(Some(&CLOSE)), [Some(Fade::Out), Some(Fade::InFromZero)]);
        assert_eq!(s.slots_for(&MAX).collect::<Vec<_>>(), vec![0], "still drawn while it fades");
        assert_eq!(s.slots_for(&CLOSE).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn coming_back_picks_the_fade_up_where_it_is() {
        let mut s = HoverSlots::default();
        s.hover(Some(&CLOSE));
        s.hover(None);
        assert_eq!(s.hover(Some(&CLOSE)), [Some(Fade::In), None]);
        s.hover(Some(&MAX));
        assert_eq!(s.hover(Some(&CLOSE)), [Some(Fade::In), Some(Fade::Out)]);
    }

    #[test]
    fn a_third_element_takes_the_slot_fading_out() {
        let mut s = HoverSlots::default();
        s.hover(Some(&PILL));
        s.hover(Some(&MAX));
        // PILL fades out on 0, MAX is lit on 1: CLOSE takes 0 from nothing
        assert_eq!(s.hover(Some(&CLOSE)), [Some(Fade::InFromZero), Some(Fade::Out)]);
        assert_eq!(s.slots_for(&PILL).count(), 0);
        assert_eq!(s.slots_for(&CLOSE).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn at_most_one_slot_is_lit() {
        let mut s = HoverSlots::default();
        for key in [Some(&PILL), Some(&MAX), None, Some(&CLOSE), Some(&MAX), Some(&PILL), None] {
            s.hover(key);
            assert!((0..SLOTS).filter(|&i| s.is_lit(i)).count() <= 1, "{s:?}");
        }
    }

    #[test]
    fn clear_forgets_everything() {
        let mut s = HoverSlots::default();
        s.hover(Some(&CLOSE));
        s.clear();
        assert_eq!(s, HoverSlots::default());
        assert_eq!(s.hover(None), [None, None]);
    }
}
