//! What a window's title update has to do, and how often (wezterm-gui's termwindow/titles.rs).
//!
//! Upstream's GUI updates a window's title (update_title) for every title, folder, unseen-output
//! and progress alert of any pane, in every window, and for every change of the mux window's own
//! title. Each update ran the update-status and update-right-status Lua events, handlers or not,
//! built every tab's and pane's information (each pane's user vars copied), walked every pane of
//! every tab for the taskbar's progress, and set the window's title. A working Claude Code session
//! sets its title every 960 ms, which on a mux domain brought three of them (two alerts and the
//! mux window's title) in every window: measured 2026-10-30 (perf/title-storm.ps1), 0.75 to 0.93 ms
//! of the GUI thread a title. Now the alerts a window gets before its update runs share one update,
//! which does what they changed: the taskbar only for progress and user vars, every tab's
//! information only for what reads it, and nothing for a background tab's title unless something
//! reads every tab (0.23 to 0.26 ms a title, the rest of it outside the update; 0.165 to 0.194 ms
//! against the installed build's 0.633 to 0.724 in the runs after the branch's review, with the
//! script as it is now). Its rail row is the GUI's to repaint, when the row changes
//! (lbterm_panel::claude::titles_move_row).

use std::ops::BitOr;

/// What changed, as far as a window's title update goes
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changed {
    /// The window's title, or the tab bar's words: a pane's title, folder, unseen output
    pub title: bool,
    /// What the taskbar button shows: a pane's progress, or a user var (the Conductor hub's
    /// guess for the open tab is keyed by its `claude_session`)
    pub progress: bool,
}

impl Changed {
    pub const TITLE: Changed = Changed { title: true, progress: false };
    pub const PROGRESS: Changed = Changed { title: true, progress: true };
}

impl BitOr for Changed {
    type Output = Changed;
    fn bitor(self, other: Changed) -> Changed {
        Changed {
            title: self.title || other.title,
            progress: self.progress || other.progress,
        }
    }
}

/// The notifications that update a window's title
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    /// A pane's title, icon title or tab title
    Title,
    /// A pane's folder (OSC 7)
    Folder,
    /// A pane printed while the window didn't have the keyboard
    UnseenOutput,
    /// A pane's progress (OSC 9;4)
    Progress,
    /// A pane's user var, once the user-var-changed handlers (if any) have run
    UserVar,
}

impl Alert {
    pub fn changed(self) -> Changed {
        match self {
            Alert::Title | Alert::Folder | Alert::UnseenOutput => Changed::TITLE,
            Alert::Progress | Alert::UserVar => Changed::PROGRESS,
        }
    }
}

/// The updates asked for that haven't run: every alert before one runs shares it
#[derive(Debug, Default)]
pub struct Soon {
    pending: Option<Changed>,
}

impl Soon {
    /// `changed` wants an update: true when none is on its way, so the caller sends one
    pub fn ask(&mut self, changed: Changed) -> bool {
        let first = self.pending.is_none();
        self.pending = Some(self.pending.unwrap_or_default() | changed);
        first
    }

    /// The update runs: what everything since the last asked of it
    pub fn take(&mut self) -> Option<Changed> {
        self.pending.take()
    }
}

/// Whether a title update needs every tab's and every pane's information: the tab bar's words
/// when it shows the tabs, or a format-window-title handler, which is handed all of it. Else the
/// title is the open pane's, with its zoom, the open tab's place and the number of tabs.
pub fn needs_every_tab(tab_bar_shows_tabs: bool, format_window_title: bool) -> bool {
    tab_bar_shows_tabs || format_window_title
}

/// Whether `alert` from a pane needs an update at all. The taskbar shows every tab's progress, so
/// any pane's does, and with every tab's information wanted (`needs_every_tab`) or a status
/// handler to run, any pane's alert does. Else only the open tab's: the window's title is its
/// open pane's, the taskbar's guess is keyed by its `claude_session`, and a background tab's
/// title shows only on its rail row, which no update paints: the window repaints for it when
/// the row would read otherwise (wezterm-gui's render/tab_panel.rs, lbterm_panel_title_changed).
pub fn wants_update(alert: Alert, in_open_tab: bool, every_tab: bool, status_handlers: bool) -> bool {
    alert == Alert::Progress || in_open_tab || every_tab || status_handlers
}

/// What a window's handlers and taskbar ask of a pane's title alert, as its GUI thread left them
/// for its mux subscription (wezterm-gui's termwindow/titles.rs, TitleWatch): asked at each
/// config load, and as the taskbar's button failed or showed
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readers {
    /// The window's taskbar button is due to be set again: any alert tries it (taskbar.rs)
    pub retry_due: bool,
    /// An update-status or update-right-status handler, which can show any window's tabs
    pub status_handlers: bool,
    /// The window's title update reads every tab ([`needs_every_tab`])
    pub every_tab: bool,
}

/// Where a title alert's pane is, from the window deciding it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// In another window, or none
    Elsewhere,
    /// In one of the window's tabs, not the open one
    Background,
    /// In the window's open tab
    OpenTab,
}

/// Whether a pane's title alert (a title, icon title or tab title) has anything to do in a window,
/// decided in the window's mux subscription before any task: what wezterm-gui's
/// `lbterm_title_alert` would do with it, which is a taskbar retry, an update for another window's
/// pane only with a status handler, and for its own pane an update ([`wants_update`]) or a repaint
/// of its tab's rail row when the row, as the last paint drew it, would read otherwise
/// (`row_moves`, lbterm_panel::claude::titles_move_row). `row_moves` is asked last, and only for
/// a background tab's pane with nothing else to say: it's the dear part (a row cloned and worked
/// out twice). A title that goes on goes as before; one that doesn't is dropped: Claude Code's ◐
/// and ◑ in a tab out of view, which change no row and no caption
pub fn title_alert_goes(readers: Readers, place: Place, row_moves: impl FnOnce() -> bool) -> bool {
    if readers.retry_due {
        return true;
    }
    match place {
        Place::Elsewhere => readers.status_handlers,
        Place::OpenTab => true,
        Place::Background => {
            wants_update(Alert::Title, false, readers.every_tab, readers.status_handlers) || row_moves()
        }
    }
}

/// What the window's title is made of without a format-window-title handler
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    /// The open pane's title
    pub title: String,
    /// Its tab is zoomed on it
    pub zoomed: bool,
    /// The open tab's place, from 0
    pub tab_index: usize,
}

/// The window's title as upstream makes it without a handler: `[Z] ` while zoomed, `[2/5] ` with
/// more than one tab, then the open pane's title
pub fn window_title(open: Option<&Open>, tabs: usize) -> String {
    let Some(open) = open else {
        return String::new();
    };
    let zoomed = if open.zoomed { "[Z] " } else { "" };
    if tabs == 1 {
        format!("{zoomed}{}", open.title)
    } else {
        format!("{zoomed}[{}/{tabs}] {}", open.tab_index + 1, open.title)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn titles_folders_and_unseen_output_leave_the_taskbar_alone() {
        for alert in [Alert::Title, Alert::Folder, Alert::UnseenOutput] {
            assert_eq!(alert.changed(), Changed { title: true, progress: false }, "{alert:?}");
        }
        for alert in [Alert::Progress, Alert::UserVar] {
            assert_eq!(alert.changed(), Changed { title: true, progress: true }, "{alert:?}");
        }
    }

    #[test]
    fn alerts_before_the_update_runs_share_it() {
        let mut soon = Soon::default();
        assert!(soon.ask(Alert::Title.changed()), "the first sends one");
        assert!(!soon.ask(Alert::Title.changed()), "one is on its way");
        assert!(!soon.ask(Alert::Folder.changed()));
        assert_eq!(soon.take(), Some(Changed::TITLE));
        assert_eq!(soon.take(), None, "it ran: nothing waits");
        assert!(soon.ask(Alert::Title.changed()), "after it ran, the next sends another");
    }

    #[test]
    fn the_update_does_what_any_of_its_alerts_changed() {
        let mut soon = Soon::default();
        soon.ask(Alert::Title.changed());
        soon.ask(Alert::Progress.changed());
        soon.ask(Alert::Title.changed());
        assert_eq!(soon.take(), Some(Changed { title: true, progress: true }));
    }

    #[test]
    fn a_background_tab_s_alert_needs_no_update_unless_something_reads_every_tab() {
        for alert in [Alert::Title, Alert::Folder, Alert::UnseenOutput, Alert::UserVar] {
            assert!(!wants_update(alert, false, false, false), "{alert:?}");
            assert!(wants_update(alert, true, false, false), "{alert:?} in the open tab");
            assert!(wants_update(alert, false, true, false), "{alert:?}: the tab bar's words or format-window-title");
            assert!(wants_update(alert, false, false, true), "{alert:?}: update-status runs on every update");
        }
        // The taskbar shows every tab's progress
        assert!(wants_update(Alert::Progress, false, false, false));
    }

    #[test]
    fn every_tab_only_for_the_tab_bar_s_words_or_a_handler() {
        assert!(!needs_every_tab(false, false));
        assert!(needs_every_tab(true, false));
        assert!(needs_every_tab(false, true));
    }

    #[test]
    fn the_title_is_upstream_s() {
        let open = |zoomed, tab_index| Open { title: "\u{25d0} work".to_string(), zoomed, tab_index };
        assert_eq!(window_title(Some(&open(false, 0)), 1), "\u{25d0} work");
        assert_eq!(window_title(Some(&open(true, 0)), 1), "[Z] \u{25d0} work");
        assert_eq!(window_title(Some(&open(false, 2)), 12), "[3/12] \u{25d0} work");
        assert_eq!(window_title(Some(&open(true, 0)), 2), "[Z] [1/2] \u{25d0} work");
        assert_eq!(window_title(None, 3), "");
    }

    /// The decision, and whether it asked the row
    fn goes(readers: Readers, place: Place, row: bool) -> (bool, bool) {
        let asked = std::cell::Cell::new(false);
        let goes = title_alert_goes(readers, place, || {
            asked.set(true);
            row
        });
        (goes, asked.get())
    }

    #[test]
    fn a_background_tabs_title_that_moves_no_row_and_nothing_reads_is_dropped() {
        let none = Readers::default();
        assert_eq!(goes(none, Place::Background, false), (false, true), "◐ to ◑: dropped");
        assert_eq!(goes(none, Place::Background, true), (true, true), "a new title moves its row");
    }

    #[test]
    fn the_open_tabs_title_always_goes_and_never_asks_the_row() {
        assert_eq!(goes(Readers::default(), Place::OpenTab, false), (true, false));
    }

    #[test]
    fn what_reads_every_tab_takes_a_background_title_without_asking_the_row() {
        let every = Readers { every_tab: true, ..Readers::default() };
        assert_eq!(goes(every, Place::Background, false), (true, false));
        let status = Readers { status_handlers: true, ..Readers::default() };
        assert_eq!(goes(status, Place::Background, false), (true, false));
    }

    #[test]
    fn another_windows_title_goes_only_to_a_status_handler() {
        assert_eq!(goes(Readers::default(), Place::Elsewhere, true), (false, false));
        let every = Readers { every_tab: true, ..Readers::default() };
        assert_eq!(goes(every, Place::Elsewhere, true), (false, false), "every tab is the window's own");
        let status = Readers { status_handlers: true, ..Readers::default() };
        assert_eq!(goes(status, Place::Elsewhere, false), (true, false));
    }

    #[test]
    fn a_taskbar_retry_due_takes_any_title_to_the_window() {
        let retry = Readers { retry_due: true, ..Readers::default() };
        for place in [Place::Elsewhere, Place::Background, Place::OpenTab] {
            assert_eq!(goes(retry, place, false), (true, false), "{place:?}");
        }
    }
}
