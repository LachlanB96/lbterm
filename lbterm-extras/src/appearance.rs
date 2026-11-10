//! When the system's light or dark appearance changes, the config runs again once
//! (wezterm-gui's termwindow/mod.rs, `WindowEvent::AppearanceChanged`).
//!
//! Every window hears of the change, and upstream's handler ran the whole config again in each,
//! on the GUI thread (its own note: "we'll reload the global config here once per window"), then
//! applied it to its window, which the reload had already asked every window to do: with N
//! windows, N runs of the config and N more applies than there are windows. The first window to
//! hear of an appearance runs the config for it; the others find it done. It stays on the GUI
//! thread: what it's for is a config that asks `wezterm.gui.get_appearance()`, which only the GUI
//! thread can answer (anywhere else it says light).

/// The appearance the config last ran for, by an appearance change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reloads<A> {
    ran_for: Option<A>,
}

// By hand: derived, it would ask for an `A: Default`, which window's Appearance isn't
impl<A> Default for Reloads<A> {
    fn default() -> Self {
        Self { ran_for: None }
    }
}

impl<A: Copy + PartialEq> Reloads<A> {
    /// A window heard the appearance is now `appearance`: whether the config has to run again
    /// for it (true once for each change, whichever window hears it first)
    pub fn wants(&mut self, appearance: A) -> bool {
        if self.ran_for == Some(appearance) {
            return false;
        }
        self.ran_for = Some(appearance);
        true
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Look {
        Light,
        Dark,
    }

    #[test]
    fn three_windows_hearing_one_change_run_the_config_once() {
        let mut r = Reloads::default();
        let ran: Vec<bool> = (0..3).map(|_| r.wants(Look::Dark)).collect();
        assert_eq!(ran, [true, false, false]);
    }

    #[test]
    fn each_change_back_and_forth_runs_it_again() {
        let mut r = Reloads::default();
        assert!(r.wants(Look::Dark));
        assert!(!r.wants(Look::Dark));
        assert!(r.wants(Look::Light), "back to light");
        assert!(!r.wants(Look::Light));
        assert!(r.wants(Look::Dark), "and dark again");
    }

    #[test]
    fn the_first_change_heard_always_runs_it() {
        // The config's first run asked for the appearance the GUI had then, which no window has
        // told this of: the first change any window hears is a change
        let mut r = Reloads::default();
        assert!(r.wants(Look::Light));
    }
}
