//! A keyboard layout's probe, made once per GUI process for each layout, not once per window.
//!
//! On Windows the window crate learns whether a keyboard layout has AltGr, and what its dead keys
//! make, by probing the layout with thousands of `ToUnicode` calls (`KeyboardLayoutInfo` in
//! window/src/os/windows/window.rs): 40 to 65 ms of the GUI thread on this PC. Upstream keeps the
//! result in each window, so every new window made the probe again, inside the WM_KEYDOWN of the
//! first key typed into it. What the probe finds depends on nothing but the layout, so it's kept
//! here by layout (its HKL, as a number) and shared by every window of the GUI thread: the first
//! key in the process pays for it, and the first key after a switch to a layout not seen before.
//! A switch back to one already probed costs nothing.
//!
//! The probe stays where upstream makes it, on the key path, while a window of this thread has the
//! keyboard, and is made the same way. `ToUnicode` keeps a pending dead key inside the keyboard
//! layout (Microsoft's words), not in the call, and the probe clocks that state thousands of
//! times: made at startup, while another program has the keyboard, it could swallow a dead key
//! typed there, and made on a thread of its own it would clock the state under the GUI thread's
//! own `ToUnicode` calls. `ToUnicode` also reads the calling thread's layout, which is the GUI
//! thread's.
//!
//! The cache is the GUI thread's (a `thread_local!` in window.rs): every window lives on it, and
//! a keyboard layout is a thread's.

use std::collections::HashMap;

/// The layouts probed on a thread, by HKL.
#[derive(Debug)]
pub struct Probed<T> {
    layouts: HashMap<usize, T>,
}

impl<T> Default for Probed<T> {
    fn default() -> Self {
        Self {
            layouts: HashMap::new(),
        }
    }
}

impl<T: Clone> Probed<T> {
    /// A window that last took the probe of layout `seen` (0, a null HKL, before its first key)
    /// meets a key with the thread's layout `current`.
    ///
    /// - The same layout: `None`, nothing to do (upstream's own check).
    /// - Another, probed before (by this window or any other): `Some` of that probe, without
    ///   probing.
    /// - Another, never probed: `probe` runs, and what it finds is kept for every window after.
    ///   When it can't be made (`None`, the keyboard state couldn't be saved), nothing is kept and
    ///   `None` comes back: the window keeps what it had, and its next key tries again, as
    ///   upstream's does.
    pub fn update(
        &mut self,
        seen: usize,
        current: usize,
        probe: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        if current == seen {
            return None;
        }
        if let Some(found) = self.layouts.get(&current) {
            return Some(found.clone());
        }
        let found = probe()?;
        self.layouts.insert(current, found.clone());
        Some(found)
    }

    /// Whether `layout` has been probed
    pub fn has(&self, layout: usize) -> bool {
        self.layouts.contains_key(&layout)
    }

    /// How many layouts have been probed
    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const US: usize = 0x0409_0c09;
    const DE: usize = 0x0407_0407;

    /// A window as window.rs keeps one: the layout it last took a probe of, and that probe
    struct Window {
        seen: usize,
        probe: &'static str,
    }

    impl Window {
        fn new() -> Self {
            Self {
                seen: 0,
                probe: "nothing yet",
            }
        }

        /// A key with the thread's layout `current`; the probe, when one is made, finds `finds`
        fn key(
            &mut self,
            probed: &mut Probed<&'static str>,
            current: usize,
            finds: Option<&'static str>,
            probes: &Cell<u32>,
        ) {
            if let Some(found) = probed.update(self.seen, current, || {
                probes.set(probes.get() + 1);
                finds
            }) {
                self.probe = found;
                self.seen = current;
            }
        }
    }

    #[test]
    fn the_first_key_probes_and_the_next_does_not() {
        let mut probed = Probed::default();
        let probes = Cell::new(0);
        let mut w = Window::new();
        w.key(&mut probed, US, Some("us"), &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (1, "us", US));
        w.key(&mut probed, US, Some("us again"), &probes);
        assert_eq!((probes.get(), w.probe), (1, "us"));
    }

    #[test]
    fn a_second_window_takes_the_first_ones_probe() {
        let mut probed = Probed::default();
        let probes = Cell::new(0);
        let mut first = Window::new();
        first.key(&mut probed, US, Some("us"), &probes);
        let mut second = Window::new();
        second.key(&mut probed, US, Some("us, probed again"), &probes);
        assert_eq!((probes.get(), second.probe, second.seen), (1, "us", US));
        let mut third = Window::new();
        third.key(&mut probed, US, Some("us, probed again"), &probes);
        assert_eq!((probes.get(), third.probe), (1, "us"));
        assert_eq!(probed.len(), 1);
    }

    #[test]
    fn a_new_layout_is_probed_and_a_layout_seen_before_is_not() {
        let mut probed = Probed::default();
        let probes = Cell::new(0);
        let mut w = Window::new();
        w.key(&mut probed, US, Some("us"), &probes);
        // A switch to a layout never seen: probed, once
        w.key(&mut probed, DE, Some("de"), &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (2, "de", DE));
        w.key(&mut probed, DE, Some("de again"), &probes);
        assert_eq!(probes.get(), 2);
        // And back: the first probe, kept
        w.key(&mut probed, US, Some("us, probed again"), &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (2, "us", US));
        // Another window that never typed meets the switched layout: kept too
        let mut other = Window::new();
        other.key(&mut probed, DE, Some("de, probed again"), &probes);
        assert_eq!((probes.get(), other.probe), (2, "de"));
        assert!(probed.has(US) && probed.has(DE));
        assert_eq!(probed.len(), 2);
    }

    #[test]
    fn a_window_that_saw_one_layout_takes_another_windows_probe_of_the_next() {
        let mut probed = Probed::default();
        let probes = Cell::new(0);
        let mut a = Window::new();
        let mut b = Window::new();
        a.key(&mut probed, US, Some("us"), &probes);
        b.key(&mut probed, US, None, &probes);
        // The layout changes; a meets it first, b after
        a.key(&mut probed, DE, Some("de"), &probes);
        b.key(&mut probed, DE, Some("de, probed again"), &probes);
        assert_eq!(
            (probes.get(), a.probe, b.probe, b.seen),
            (2, "de", "de", DE)
        );
    }

    #[test]
    fn a_probe_that_fails_keeps_nothing_and_the_next_key_tries_again() {
        let mut probed = Probed::default();
        let probes = Cell::new(0);
        let mut w = Window::new();
        w.key(&mut probed, US, None, &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (1, "nothing yet", 0));
        assert!(probed.is_empty());
        w.key(&mut probed, US, Some("us"), &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (2, "us", US));
        // After a switch, a failed probe leaves the window with the layout it had
        w.key(&mut probed, DE, None, &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (3, "us", US));
        assert!(!probed.has(DE));
        w.key(&mut probed, DE, Some("de"), &probes);
        assert_eq!((probes.get(), w.probe, w.seen), (4, "de", DE));
    }

    #[test]
    fn a_null_layout_is_never_probed() {
        // GetKeyboardLayout(0) giving a null HKL: the window's own starting value, as upstream
        // never probed it
        let mut probed = Probed::default();
        let probes = Cell::new(0);
        let mut w = Window::new();
        w.key(&mut probed, 0, Some("null"), &probes);
        assert_eq!((probes.get(), w.probe), (0, "nothing yet"));
        assert!(probed.is_empty());
    }
}
