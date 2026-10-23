//! The scrollbar while a program has the whole screen. A fullscreen program (Claude Code with
//! `"tui": "fullscreen"`, an editor, a pager) draws on the alternate screen or takes the mouse
//! wheel for itself, so the terminal's viewport never moves when its content scrolls, and the
//! thumb would only lead to the few lines printed before it started. While the window's active
//! pane is in that state its scrollbar is hidden, and it's back when the pane is at a shell
//! again. No escape sequence says where such a program is scrolled, so the bar can't follow it.

/// Who has the active pane's screen, as the terminal (or, for a mux pane, its server) knows it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaneScreen {
    /// On the alternate screen (DECSET 47, 1047 or 1049)
    pub alt_screen: bool,
    /// Mouse reporting on (DECSET 1000, 1002 or 1003)
    pub mouse_grabbed: bool,
}

impl PaneScreen {
    /// Whether a program owns the screen, either way
    pub fn program_owns_it(self) -> bool {
        self.alt_screen || self.mouse_grabbed
    }
}

/// Whether the window draws its scrollbar down the active pane: `enabled` is upstream's
/// `enable_scroll_bar`, `hide_in_apps` LBTerm's `lbterm_hide_scroll_bar_in_apps`
pub fn shows(enabled: bool, hide_in_apps: bool, screen: PaneScreen) -> bool {
    enabled && !(hide_in_apps && screen.program_owns_it())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELL: PaneScreen = PaneScreen {
        alt_screen: false,
        mouse_grabbed: false,
    };
    const ALT: PaneScreen = PaneScreen {
        alt_screen: true,
        mouse_grabbed: false,
    };
    const MOUSE: PaneScreen = PaneScreen {
        alt_screen: false,
        mouse_grabbed: true,
    };
    const BOTH: PaneScreen = PaneScreen {
        alt_screen: true,
        mouse_grabbed: true,
    };

    #[test]
    fn a_shell_has_its_scrollbar() {
        assert!(shows(true, true, SHELL));
    }

    #[test]
    fn the_alternate_screen_hides_it() {
        assert!(!shows(true, true, ALT));
    }

    #[test]
    fn mouse_reporting_hides_it() {
        assert!(!shows(true, true, MOUSE));
    }

    #[test]
    fn both_hide_it() {
        assert!(!shows(true, true, BOTH));
    }

    #[test]
    fn turned_off_it_shows_whoever_has_the_screen() {
        for screen in [SHELL, ALT, MOUSE, BOTH] {
            assert!(shows(true, false, screen));
        }
    }

    #[test]
    fn without_enable_scroll_bar_there_is_none() {
        for hide in [true, false] {
            for screen in [SHELL, ALT, MOUSE, BOTH] {
                assert!(!shows(false, hide, screen));
            }
        }
    }
}
