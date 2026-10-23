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

#[cfg(test)]
mod tests {
    use super::TaskbarProgress::*;
    use super::*;

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
    fn values() {
        assert_eq!(Normal(42).value(), Some(42));
        assert_eq!(Normal(250).value(), Some(100));
        assert_eq!(Error(0).value(), Some(100));
        assert_eq!(Error(64).value(), Some(64));
        assert_eq!(Paused.value(), Option::None);
        assert_eq!(Indeterminate.value(), Option::None);
    }
}
