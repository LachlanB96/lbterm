//! What each tab's Claude Code session is doing, and the row that says so.
//!
//! Best of all, the program says so itself: OSC 7501, the Program Status
//! Protocol, which Claude Code speaks from 2.1.295 to a terminal that
//! answers its query ([`Program`]). Its state, why it's blocked, its
//! message, its progress and its subagents come from there. Without it,
//! two signals already in the terminal:
//! - `claude_status`, the user var Terminal's claudehook.exe sets in the
//!   pane on every hook (OSC 1337 SetUserVar):
//!   `<rrggbb|default>|<stamp>`, `006400` working, `ff8c00` blocked on you,
//!   `default` stopped, the stamp being the Windows FILETIME the hook
//!   started. wezterm.lua reads the same value for its tab pills.
//! - The glyph Claude Code puts in front of its title: a braille spinner
//!   frame while it works, ✳ while it waits for you. It's there in a pane
//!   whose hooks aren't set up, and gone when Claude exits.

use crate::idle::{self, Bar};
use crate::text;

/// What a row's pad shows, as in APCommand's Terminal mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    NoClaude,
    Working,
    /// Blocked on you: a permission prompt or a question
    Waiting,
    /// Finished: your turn
    Idle,
    /// The program said it failed and stopped (OSC 7501 only)
    Error,
}

/// What a program reported about itself through OSC 7501, as the panel
/// needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// Empty for the program's own record; a `/` path for its parts
    /// (Claude Code's subagents and background tasks)
    pub id: String,
    pub state: ProgramState,
    pub kind: Option<Blocked>,
    pub progress: Option<u8>,
    pub title: Option<String>,
    pub msg: Option<String>,
    /// When the report arrived, in Unix seconds
    pub updated: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    /// Finished, with a result the user hasn't seen yet
    Done,
    Blocked,
    Error,
}

/// Why a program is blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    Permission,
    Question,
    Auth,
}

pub const WORKING: u32 = 0x006400;
pub const WAITING: u32 = 0xFF8C00;

/// One `claude_status` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// The light's colour; `None` for `default`
    pub colour: Option<u32>,
    /// When the hook that set it started, as a Windows FILETIME
    pub stamp: Option<u64>,
}

impl Status {
    pub fn parse(value: &str) -> Option<Status> {
        let (state, stamp) = match value.split_once('|') {
            Some((state, stamp)) => (state, Some(stamp.trim().parse::<u64>().ok()?)),
            // A value from before claudehook stamped them
            None => (value, None),
        };
        let state = state.trim().to_ascii_lowercase();
        let colour = match state.as_str() {
            "default" => None,
            "orange" => Some(WAITING),
            hex if hex.len() == 6 => Some(u32::from_str_radix(hex, 16).ok()?),
            _ => return None,
        };
        Some(Status { colour, stamp })
    }

    /// Seconds since the Unix epoch when the hook ran.
    pub fn unix_time(&self) -> Option<f64> {
        self.stamp.map(filetime_to_unix)
    }
}

/// A FILETIME (100 ns ticks since 1601) in Unix seconds.
pub fn filetime_to_unix(filetime: u64) -> f64 {
    filetime as f64 / 1e7 - 11_644_473_600.0
}

/// The newest of a tab's statuses. Hooks finish out of order, and Claude
/// can run in any pane of the tab, so the latest stamp wins, as in
/// wezterm.lua.
pub fn newest(statuses: impl IntoIterator<Item = Status>) -> Option<Status> {
    statuses.into_iter().max_by_key(|s| s.stamp)
}

/// The glyph Claude Code puts before its title, if `title` has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// A braille spinner frame: working
    Spinner,
    /// ✳ or another dingbat or shape: waiting for you
    Still,
}

pub fn claude_glyph(title: &str) -> Option<Glyph> {
    let mut chars = title.chars();
    match (chars.next(), chars.next()) {
        (Some(c), Some(' ')) => match c as u32 {
            0x2800..=0x28FF => Some(Glyph::Spinner),
            0x2580..=0x25FF | 0x2700..=0x27BF => Some(Glyph::Still),
            _ => None,
        },
        _ => None,
    }
}

/// The status light wins when there is one; the title glyph covers a pane
/// without hooks.
pub fn row_state(status: Option<&Status>, glyph: Option<Glyph>) -> RowState {
    match (status.and_then(|s| s.colour), glyph) {
        (Some(WAITING), _) => RowState::Waiting,
        (Some(_), _) => RowState::Working,
        (None, Some(Glyph::Spinner)) if status.is_none() => RowState::Working,
        (None, Some(_)) => RowState::Idle,
        (None, None) => RowState::NoClaude,
    }
}

/// A tab as the GUI sees it.
#[derive(Debug, Clone, Default)]
pub struct Tab {
    /// 1-based, as the tab bar and the keys count
    pub index: usize,
    /// The tab's own title, if one was set
    pub tab_title: String,
    /// Every pane's title, the active pane's first
    pub pane_titles: Vec<String>,
    /// The active pane's working directory, a path or `file://` URL
    pub cwd: String,
    /// Every `claude_status` in the tab's panes
    pub statuses: Vec<Status>,
    /// Every OSC 7501 record in the tab's panes
    pub programs: Vec<Program>,
    /// When the tab was last on screen, in Unix seconds (0 if never): a
    /// result that arrived since is unread
    pub seen: f64,
    pub active: bool,
    /// What its Claude session is for, as Terminal's summary worker put it
    /// (`<session>.label.txt`)
    pub label: Option<String>,
}

/// What a tab's row shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub index: usize,
    pub title: String,
    pub project: String,
    pub state: RowState,
    /// Next to the project: "Working", "Question", "Your turn", ...
    pub status: String,
    /// The program's own message, which takes the status's place when it
    /// fits whole (the pad's colour already says the state)
    pub message: String,
    /// The program's parts still working or blocked: Claude Code's
    /// subagents and background tasks
    pub agents: usize,
    /// Its progress, 0-100, while it's working or blocked and says so
    pub progress: Option<u8>,
    /// A result arrived since the tab was last on screen
    pub unread: bool,
    /// How long it's been blocked or finished: "now", "12m"
    pub age: String,
    /// The idle timer, 1-9, 0 while working or without Claude
    pub level: u8,
    pub bars: [Bar; 3],
    pub active: bool,
    /// Under the pad on the closed rail, what the tab is for: the name it was
    /// given, else its Claude session's label, else its title
    pub label: String,
    /// The program has handed the turn back, but this many of its parts
    /// still run (Claude Code's background tasks): it will carry on by
    /// itself when they finish. A turning arc on the pad says so
    pub waiting: usize,
}

/// The row for `tab` at `now` (Unix seconds), the timer filling over
/// `idle_full` seconds. Also when its text next changes, if it will.
pub fn row(tab: &Tab, now: f64, idle_full: f64) -> (Row, Option<f64>) {
    let active_title = tab.pane_titles.first().map(String::as_str).unwrap_or("");
    let mut title = text::strip_glyphs(&tab.tab_title).to_string();
    // (A program's own title, below, wins over all of these)
    if title.is_empty() {
        title = text::program_name(text::strip_glyphs(active_title)).to_string();
    }
    if title.is_empty() {
        title = format!("tab {}", tab.index);
    }

    let reported = tab
        .programs
        .iter()
        .filter(|p| p.id.is_empty())
        .max_by(|a, b| a.updated.total_cmp(&b.updated));
    let (state, since, status, message, progress, unread) = match reported {
        // The program's own word
        Some(p) => {
            if let Some(own) = p.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                title = own.to_string();
            }
            let state = match p.state {
                ProgramState::Working => RowState::Working,
                ProgramState::Blocked => RowState::Waiting,
                ProgramState::Done | ProgramState::Idle => RowState::Idle,
                ProgramState::Error => RowState::Error,
            };
            // Short, to fit beside the project on a narrow panel
            let words = match (p.state, p.kind) {
                (ProgramState::Working, _) => "Working",
                (ProgramState::Blocked, Some(Blocked::Permission)) => "Permission",
                (ProgramState::Blocked, Some(Blocked::Question)) => "Question",
                (ProgramState::Blocked, Some(Blocked::Auth)) => "Login",
                (ProgramState::Blocked, None) => "Blocked",
                (ProgramState::Done, _) => "Done",
                (ProgramState::Idle, _) => "Your turn",
                (ProgramState::Error, _) => "Failed",
            };
            let progress = match p.state {
                ProgramState::Working | ProgramState::Blocked => p.progress,
                _ => None,
            };
            (
                state,
                (p.state != ProgramState::Working).then_some(p.updated),
                words.to_string(),
                p.msg.as_deref().map(str::trim).unwrap_or_default().to_string(),
                progress,
                p.state == ProgramState::Done && p.updated > tab.seen,
            )
        }
        // The hooks' light and the title's glyph
        None => {
            let status = newest(tab.statuses.iter().copied());
            // The tab's own title, then the active pane's: the title the row shows
            let glyph = std::iter::once(&tab.tab_title)
                .chain(tab.pane_titles.iter())
                .find_map(|t| claude_glyph(t));
            let state = row_state(status.as_ref(), glyph);
            let since = match state {
                RowState::Waiting | RowState::Idle => status.and_then(|s| s.unix_time()),
                _ => None,
            };
            let words = match state {
                RowState::Working => "Working",
                RowState::Waiting => "Blocked",
                RowState::Idle => "Your turn",
                RowState::Error => "Failed",
                RowState::NoClaude => "",
            };
            (state, since, words.to_string(), String::new(), None, false)
        }
    };
    // Its parts still at it: Claude Code's subagents and background tasks
    let agents = tab
        .programs
        .iter()
        .filter(|p| !p.id.is_empty())
        .filter(|p| matches!(p.state, ProgramState::Working | ProgramState::Blocked))
        .count();
    let (age, level, wake) = match since {
        Some(since) => {
            let age = (now - since).max(0.0);
            let next = [
                Some(text::ago_changes_in(age)),
                idle::next_step_in(age, idle_full),
            ]
            .into_iter()
            .flatten()
            .fold(f64::INFINITY, f64::min);
            (text::ago(age), idle::level(age, idle_full), Some(next))
        }
        None => (String::new(), 0, None),
    };

    // Handed back, with parts still at it: waiting on them, not stopped
    let waiting = if reported.is_some() && state == RowState::Idle { agents } else { 0 };
    let (status, message) = if waiting > 0 {
        let s = if waiting == 1 { "" } else { "s" };
        (format!("Waiting on {waiting} task{s}"), String::new())
    } else {
        (status, message)
    };
    let named = text::strip_glyphs(&tab.tab_title).trim();
    let label = if !named.is_empty() {
        named.to_string()
    } else {
        match tab.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            Some(label) => label.to_string(),
            None => title.clone(),
        }
    };
    let row = Row {
        index: tab.index,
        title,
        project: text::project(&tab.cwd),
        state,
        status,
        message,
        agents,
        progress,
        unread,
        age,
        level,
        bars: idle::bars(level),
        active: tab.active,
        label,
        waiting,
    };
    (row, wake)
}

#[cfg(test)]
mod test {
    use super::*;

    /// 2026-10-20 10:00:00 UTC as a FILETIME, and in Unix seconds.
    const STAMP: u64 = 134_369_640_000_000_000;
    const UNIX: f64 = 1_792_490_400.0;

    #[test]
    fn filetimes_convert_to_unix_seconds() {
        assert_eq!(filetime_to_unix(STAMP), UNIX);
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0.0);
    }

    #[test]
    fn status_values_parse_like_wezterm_lua_reads_them() {
        assert_eq!(
            Status::parse(&format!("006400|{STAMP}")),
            Some(Status { colour: Some(WORKING), stamp: Some(STAMP) })
        );
        assert_eq!(
            Status::parse("default|5"),
            Some(Status { colour: None, stamp: Some(5) })
        );
        assert_eq!(Status::parse("FF8C00").unwrap().colour, Some(WAITING));
        assert_eq!(Status::parse("orange|1").unwrap().colour, Some(WAITING));
        assert_eq!(Status::parse("purple|1"), None);
        assert_eq!(Status::parse("006400|soon"), None);
    }

    #[test]
    fn the_latest_stamp_wins() {
        let a = Status { colour: Some(WORKING), stamp: Some(10) };
        let b = Status { colour: None, stamp: Some(20) };
        assert_eq!(newest([a, b]), Some(b));
        assert_eq!(newest([b, a]), Some(b));
        assert_eq!(newest([]), None);
    }

    #[test]
    fn claude_title_glyphs_are_recognised() {
        assert_eq!(claude_glyph("✳ Tab panel"), Some(Glyph::Still));
        assert_eq!(claude_glyph("⠐ Tab panel"), Some(Glyph::Spinner));
        assert_eq!(claude_glyph("◐ Thinking"), Some(Glyph::Still));
        assert_eq!(claude_glyph("pwsh"), None);
        assert_eq!(claude_glyph("✳Tab"), None, "no space: not Claude's prefix");
    }

    #[test]
    fn the_light_wins_over_the_glyph() {
        let working = Status { colour: Some(WORKING), stamp: Some(1) };
        let waiting = Status { colour: Some(WAITING), stamp: Some(1) };
        let stopped = Status { colour: None, stamp: Some(1) };
        assert_eq!(row_state(Some(&waiting), Some(Glyph::Spinner)), RowState::Waiting);
        assert_eq!(row_state(Some(&working), Some(Glyph::Still)), RowState::Working);
        assert_eq!(row_state(Some(&stopped), Some(Glyph::Still)), RowState::Idle);
        // A stale spinner after the Stop hook: the hook knows better
        assert_eq!(row_state(Some(&stopped), Some(Glyph::Spinner)), RowState::Idle);
        // Claude exited: the summary pane reset the light, the glyph is gone
        assert_eq!(row_state(Some(&stopped), None), RowState::NoClaude);
        // No hooks in this pane: the glyph alone
        assert_eq!(row_state(None, Some(Glyph::Spinner)), RowState::Working);
        assert_eq!(row_state(None, Some(Glyph::Still)), RowState::Idle);
        assert_eq!(row_state(None, None), RowState::NoClaude);
    }

    fn tab() -> Tab {
        Tab {
            index: 3,
            tab_title: String::new(),
            pane_titles: vec!["✳ Tab panel for LBTerm".into(), "pwsh".into()],
            cwd: "file:///C:/Users/me/Notes/Life/Projects/Terminal/".into(),
            statuses: vec![Status { colour: None, stamp: Some(STAMP) }],
            programs: vec![],
            seen: 0.0,
            active: true,
            label: None,
        }
    }

    fn program(id: &str, state: ProgramState, updated: f64) -> Program {
        Program {
            id: id.into(),
            state,
            kind: None,
            progress: None,
            title: None,
            msg: None,
            updated,
        }
    }

    #[test]
    fn a_programs_own_report_wins_over_the_hooks() {
        let mut t = tab();
        let mut own = program("", ProgramState::Blocked, UNIX + 10.0);
        own.kind = Some(Blocked::Question);
        own.title = Some("Pick a region".into());
        own.msg = Some("Which region should the deploy use?".into());
        own.progress = Some(40);
        t.programs = vec![own];
        let (r, _) = row(&t, UNIX + 70.0, 3600.0);
        assert_eq!(r.state, RowState::Waiting);
        assert_eq!(r.status, "Question");
        assert_eq!(r.message, "Which region should the deploy use?");
        assert_eq!(r.title, "Pick a region");
        assert_eq!((r.progress, r.age.as_str()), (Some(40), "1m"));
    }

    #[test]
    fn each_state_says_what_it_is() {
        let says = |state, kind| {
            let mut t = tab();
            let mut p = program("", state, UNIX);
            p.kind = kind;
            t.programs = vec![p];
            let (r, _) = row(&t, UNIX + 5.0, 3600.0);
            (r.state, r.status)
        };
        assert_eq!(says(ProgramState::Working, None), (RowState::Working, "Working".into()));
        assert_eq!(says(ProgramState::Blocked, Some(Blocked::Permission)), (RowState::Waiting, "Permission".into()));
        assert_eq!(says(ProgramState::Blocked, Some(Blocked::Question)), (RowState::Waiting, "Question".into()));
        assert_eq!(says(ProgramState::Blocked, Some(Blocked::Auth)), (RowState::Waiting, "Login".into()));
        assert_eq!(says(ProgramState::Blocked, None), (RowState::Waiting, "Blocked".into()));
        assert_eq!(says(ProgramState::Done, None), (RowState::Idle, "Done".into()));
        assert_eq!(says(ProgramState::Idle, None), (RowState::Idle, "Your turn".into()));
        assert_eq!(says(ProgramState::Error, None), (RowState::Error, "Failed".into()));
    }

    #[test]
    fn a_result_is_unread_until_the_tab_is_seen() {
        let mut t = tab();
        t.programs = vec![program("", ProgramState::Done, UNIX + 10.0)];
        t.seen = UNIX;
        assert!(row(&t, UNIX + 20.0, 3600.0).0.unread);
        t.seen = UNIX + 15.0;
        assert!(!row(&t, UNIX + 20.0, 3600.0).0.unread);
        t.programs = vec![program("", ProgramState::Idle, UNIX + 10.0)];
        t.seen = 0.0;
        assert!(!row(&t, UNIX + 20.0, 3600.0).0.unread, "only done is a result");
    }

    #[test]
    fn subagents_still_at_it_are_counted() {
        let mut t = tab();
        t.programs = vec![
            program("", ProgramState::Working, UNIX),
            program("agent/1", ProgramState::Working, UNIX),
            program("agent/2", ProgramState::Blocked, UNIX),
            program("agent/3", ProgramState::Done, UNIX),
        ];
        assert_eq!(row(&t, UNIX, 3600.0).0.agents, 2);
        t.programs.truncate(2);
        assert_eq!(row(&t, UNIX, 3600.0).0.agents, 1);
    }

    #[test]
    fn a_finished_session_shows_your_turn_and_its_age() {
        let (row, wake) = row(&tab(), UNIX + 20.0 * 60.0 + 5.0, 3600.0);
        assert_eq!(row.title, "Tab panel for LBTerm");
        assert_eq!(row.project, "Terminal");
        assert_eq!(row.state, RowState::Idle);
        assert_eq!(row.status, "Your turn");
        assert_eq!(row.age, "20m");
        assert_eq!(row.level, 3);
        assert_eq!(row.bars, [Bar::Green; 3]);
        // Next change: the age text at 21m (55 s), before the next timer step
        assert_eq!(wake, Some(55.0));
        assert!(row.active);
    }

    #[test]
    fn a_working_session_has_no_age_and_no_timer() {
        let mut t = tab();
        t.statuses = vec![Status { colour: Some(WORKING), stamp: Some(STAMP) }];
        let (row, wake) = row(&t, UNIX + 999.0, 3600.0);
        assert_eq!(row.state, RowState::Working);
        assert_eq!((row.age.as_str(), row.level, wake), ("", 0, None));
        assert_eq!(row.bars, [Bar::Off; 3]);
    }

    #[test]
    fn titles_fall_back_from_tab_to_pane_to_number() {
        let mut t = tab();
        t.tab_title = "Mine".into();
        assert_eq!(row(&t, UNIX, 3600.0).0.title, "Mine");
        t.tab_title.clear();
        t.pane_titles = vec![r"C:\Program Files\PowerShell\7\pwsh.exe".into()];
        t.statuses.clear();
        let (r, _) = row(&t, UNIX, 3600.0);
        assert_eq!((r.title.as_str(), r.state), ("pwsh", RowState::NoClaude));
        t.pane_titles = vec!["✳ ".into()];
        assert_eq!(row(&t, UNIX, 3600.0).0.title, "tab 3");
    }

    #[test]
    fn a_tab_title_with_claudes_glyph_counts_too() {
        let mut t = tab();
        t.tab_title = "✳ Weekly review".into();
        t.pane_titles = vec!["pwsh.exe".into()];
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert_eq!((r.title.as_str(), r.state), ("Weekly review", RowState::Idle));
    }

    #[test]
    fn handed_back_with_tasks_still_running_is_waiting_not_stopped() {
        let mut t = tab();
        // Claude Code done, two background tasks still at it (statusline.ts reports them)
        t.programs = vec![
            program("", ProgramState::Done, UNIX),
            program("bg/b1", ProgramState::Working, UNIX),
            program("bg/b2", ProgramState::Working, UNIX),
        ];
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert_eq!((r.state, r.waiting), (RowState::Idle, 2));
        assert_eq!(r.status, "Waiting on 2 tasks");
        // One finishes
        t.programs[2].state = ProgramState::Done;
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert_eq!((r.waiting, r.status.as_str()), (1, "Waiting on 1 task"));
        // Both done: plainly done
        t.programs[1].state = ProgramState::Done;
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert_eq!((r.waiting, r.status.as_str()), (0, "Done"));
        // While Claude itself works, its tasks are agents, not waiting
        t.programs[0].state = ProgramState::Working;
        t.programs[1].state = ProgramState::Working;
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert_eq!((r.waiting, r.agents, r.status.as_str()), (0, 1, "Working"));
    }

    #[test]
    fn a_label_says_what_the_tab_is_for() {
        let mut t = tab();
        // Nothing better: the title
        assert_eq!(row(&t, UNIX, 3600.0).0.label, "Tab panel for LBTerm");
        // The summary worker's label
        t.label = Some("Tab panel".into());
        assert_eq!(row(&t, UNIX, 3600.0).0.label, "Tab panel");
        t.label = Some("  ".into());
        assert_eq!(row(&t, UNIX, 3600.0).0.label, "Tab panel for LBTerm");
        // A name given in the tab menu wins over both, even over a program's own title
        t.label = Some("Tab panel".into());
        t.tab_title = "Mine".into();
        t.programs = vec![Program {
            title: Some("Claude's title".into()),
            ..program("", ProgramState::Working, UNIX)
        }];
        let (r, _) = row(&t, UNIX, 3600.0);
        assert_eq!((r.title.as_str(), r.label.as_str()), ("Claude's title", "Mine"));
    }
}
