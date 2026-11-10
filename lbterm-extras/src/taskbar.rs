//! The progress a window's taskbar button shows, from what its panes report with ConEmu's
//! OSC 9;4 (`ESC ] 9 ; 4 ; state ; percent ST`): winget, PowerShell's Write-Progress and
//! others send it. WezTerm only draws it in the default tab title; Windows Terminal also
//! puts it on the taskbar button, and so does LBTerm.

/// What one pane reports, or what the taskbar button shows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskbarProgress {
    #[default]
    None,
    /// Green, at a percentage
    Normal(u8),
    /// Red. ConEmu's percentage is optional, and a red bar at 0% can't be seen, so 0 shows full
    Error(u8),
    /// Green, sweeping
    Indeterminate,
    /// Yellow, at whatever percentage was last shown
    Paused,
}

impl TaskbarProgress {
    pub fn is_some(self) -> bool {
        self != Self::None
    }

    /// The value for ITaskbarList3::SetProgressValue, out of 100, when this state has one
    pub fn value(self) -> Option<u64> {
        match self {
            Self::Normal(pct) => Some(pct.min(100) as u64),
            Self::Error(0) => Some(100),
            Self::Error(pct) => Some(pct.min(100) as u64),
            Self::None | Self::Indeterminate | Self::Paused => None,
        }
    }
}

/// A window has one taskbar button and may have many panes reporting. It shows the active
/// pane's progress when that reports one, so what's in front is what's on the button.
/// Otherwise it shows the first pane reporting one, in tab order, so a download in a
/// background tab still shows.
pub fn choose(
    active: TaskbarProgress,
    all_panes: impl IntoIterator<Item = TaskbarProgress>,
) -> TaskbarProgress {
    if active.is_some() {
        return active;
    }
    all_panes
        .into_iter()
        .find(|p| p.is_some())
        .unwrap_or(TaskbarProgress::None)
}

/// With nothing reported at all, the open tab's Claude session as the Conductor's hub guesses it
/// (lbterm_extras::conductor::Estimate): green at its percentage. Anything a program reports wins
/// over a guess.
pub fn or_guess(chosen: TaskbarProgress, guess: impl FnOnce() -> Option<u8>) -> TaskbarProgress {
    if chosen.is_some() {
        return chosen;
    }
    guess().map_or(TaskbarProgress::None, TaskbarProgress::Normal)
}

/// The windows whose button couldn't be set (until Explorer has made a window's button,
/// ITaskbarList3 fails), each with when it last failed: wezterm-gui's termwindow/taskbar.rs. Every
/// title alert of any pane used to work the progress out again, which tried again within a
/// second while a Claude Code worked (its title every 960 ms); now only progress, user vars and
/// tab switches do, so a title alert asks `due` and tries again, a second after the last try
#[derive(Debug)]
pub struct Retries<K> {
    failed: std::collections::HashMap<K, std::time::Instant>,
}

impl<K> Default for Retries<K> {
    fn default() -> Self {
        Self { failed: std::collections::HashMap::new() }
    }
}

impl<K: std::hash::Hash + Eq> Retries<K> {
    /// How long after a failed try the next is due: as soon as a working Claude Code's next title
    pub const AFTER: std::time::Duration = std::time::Duration::from_secs(1);

    /// Setting `window`'s button failed at `now`
    pub fn failed(&mut self, window: K, now: std::time::Instant) {
        self.failed.insert(window, now);
    }

    /// `window`'s button shows what it should
    pub fn shown(&mut self, window: &K) {
        self.failed.remove(window);
    }

    /// Whether `window`'s button is due to be tried again at `now`
    pub fn due(&self, window: &K, now: std::time::Instant) -> bool {
        self.failed
            .get(window)
            .map_or(false, |at| now.saturating_duration_since(*at) >= Self::AFTER)
    }
}

#[cfg(test)]
mod tests {
    use super::TaskbarProgress::*;
    use super::*;

    #[test]
    fn a_button_that_failed_is_tried_again_a_second_on_until_it_shows() {
        let start = std::time::Instant::now();
        let at = |ms| start + std::time::Duration::from_millis(ms);
        let mut retries = Retries::default();
        assert!(!retries.due(&3, at(5000)), "nothing failed");
        retries.failed(3, at(0));
        assert!(!retries.due(&3, at(999)));
        assert!(retries.due(&3, at(1000)));
        assert!(!retries.due(&4, at(1000)), "another window's");
        // Tried again and failed again: a second from then
        retries.failed(3, at(1200));
        assert!(!retries.due(&3, at(2000)));
        assert!(retries.due(&3, at(2200)));
        retries.shown(&3);
        assert!(!retries.due(&3, at(9000)), "it shows: nothing to try");
    }

    #[test]
    fn the_active_pane_wins() {
        assert_eq!(choose(Normal(40), [Error(10), Normal(40)]), Normal(40));
    }

    #[test]
    fn a_background_pane_shows_when_the_active_one_reports_nothing() {
        assert_eq!(choose(None, [None, Indeterminate, Normal(5)]), Indeterminate);
    }

    #[test]
    fn nothing_reported_clears_the_button() {
        assert_eq!(choose(None, [None, None]), None);
        assert_eq!(choose(None, []), None);
    }

    #[test]
    fn a_guess_shows_only_when_nothing_is_reported() {
        assert_eq!(or_guess(None, || Some(40)), Normal(40));
        assert_eq!(or_guess(None, || Option::None), None);
        assert_eq!(or_guess(Paused, || Some(40)), Paused);
        // Not even asked for while something reports
        assert_eq!(or_guess(Normal(5), || panic!("asked")), Normal(5));
    }

    #[test]
    fn values() {
        assert_eq!(Normal(42).value(), Some(42));
        assert_eq!(Normal(250).value(), Some(100));
        assert_eq!(Error(0).value(), Some(100));
        assert_eq!(Error(64).value(), Some(64));
        assert_eq!(Paused.value(), Option::None);
        assert_eq!(Indeterminate.value(), Option::None);
    }
}
