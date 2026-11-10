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
//!   `5b7cff` finished with only extras on offer, `default` stopped, the
//!   stamp being the Windows FILETIME the hook started. wezterm.lua reads
//!   the same value for its tab pills.
//!
//! The light also knows what the program's own report can't say: after it
//! has handed the turn back, Claude Code reports done, but a reply that asks
//! Lachlan something in text lights orange, and one that only offers extras
//! blue. So those two win over a done or idle report (and blue over a
//! picker's blocked, when every question on it is extras).
//! - The glyph Claude Code puts in front of its title: a braille spinner
//!   frame while it works, ✳ while it waits for you. It's there in a pane
//!   whose hooks aren't set up, and gone when Claude exits.
//!
//! And one thing nothing else says: `claude_quiet`, the user var Terminal's
//! statusline sets (through claudehook) when a working reply has written
//! nothing to its transcript for 3 minutes with no tool running, or 7
//! ([`Quiet`]): `quiet|<since unix ms>|<stamp>`, `stuck|...`, or
//! `none|0|<stamp>`. Claude Code still reports working all the while (on
//! 2026-11-02 a reply ran 11 min 44 s generating ~110k tokens that never
//! showed, and its tab looked like any other working one), so a quiet reply
//! gets an amber ring round its green pad and a stuck one an amber pad saying
//! "Stuck? 8m" (Lachlan's 2a, 2026-11-02).

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
    /// The program said it failed and stopped (OSC 7501 only), or a tab link went wrong
    Error,
    /// Handed back, waiting on another tab's session (a tab link, lbterm/LINKS.md): purple
    Linked,
    /// Working, but its reply has written nothing for 7 minutes with no tool running
    /// (`claude_quiet`, [`Quiet`]): amber. The fix is Esc, then "continue"
    Stuck,
}

/// Where a tab's link to the tab it waits on stands (lbterm_extras::links)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkState {
    /// The tab it's on hasn't finished what it waits for
    Waiting,
    /// That tab finished: it's being told to carry on
    Telling,
    /// It was told; whether it carried on isn't known yet
    Told,
    /// It was told, and stopped again still blocked
    Stuck,
    /// It couldn't be told
    Failed,
    /// The tab it waits on closed before it finished
    Closed,
}

/// A tab waiting on another tab: the worst of its links, as the rail shows it
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabLink {
    pub state: LinkState,
    /// The tab it waits on, by its label
    pub on: String,
    /// What it waits for
    pub what: String,
    /// What showed it finished, or why it went wrong
    pub note: String,
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
/// Finished, and all it asks is whether you want extras (Lachlan's ask,
/// 2026-10-23): the blue pad, with a faint orange ring round it
pub const EXTRAS: u32 = 0x5B7CFF;

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

/// How far a working reply has gone without a word, as `claude_quiet` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietLevel {
    /// It isn't quiet (any more)
    None,
    /// 3 minutes with nothing written and no tool running
    Quiet,
    /// 7 minutes
    Stuck,
}

/// One `claude_quiet` value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quiet {
    pub level: QuietLevel,
    /// When its transcript was last written, in Unix seconds (0 for none)
    pub since: f64,
    /// When the statusline's claudehook run that set it started, as a Windows FILETIME
    pub stamp: u64,
}

impl Quiet {
    pub fn parse(value: &str) -> Option<Quiet> {
        let mut parts = value.trim().split('|');
        let level = match parts.next()? {
            "none" => QuietLevel::None,
            "quiet" => QuietLevel::Quiet,
            "stuck" => QuietLevel::Stuck,
            _ => return None,
        };
        let since = parts.next()?.parse::<u64>().ok()? as f64 / 1000.0;
        let stamp = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Quiet { level, since, stamp })
    }
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
        (Some(EXTRAS), _) => RowState::Idle,
        (Some(_), _) => RowState::Working,
        (None, Some(Glyph::Spinner)) if status.is_none() => RowState::Working,
        (None, Some(_)) => RowState::Idle,
        (None, None) => RowState::NoClaude,
    }
}

/// How far along a tab's Claude session is, as the Conductor's hub guesses it: its turn, and
/// each subagent it has running (lbterm_extras::conductor::Estimate).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estimate {
    /// The turn: none once it has handed back
    pub percent: Option<u8>,
    /// The turn's working seconds still to go
    pub left: Option<u32>,
    /// Each subagent's percent, oldest first
    pub agents: Vec<u8>,
}

/// The most subagents a row has bars for: the rest still count in "N agents"
pub const AGENT_BARS: usize = 4;

/// How much later than a report the hooks' stop has to be to make it a leftover, in seconds: the
/// hooks run async, so a stop and the program's last word can land a moment apart either way
const STALE_GRACE: f64 = 2.0;

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
    /// Every `claude_quiet` in the tab's panes
    pub quiets: Vec<Quiet>,
    /// Every OSC 7501 record in the tab's panes
    pub programs: Vec<Program>,
    /// When the tab was last on screen, in Unix seconds (0 if never): a
    /// result that arrived since is unread
    pub seen: f64,
    pub active: bool,
    /// What its Claude session is for, as Terminal's summary worker put it
    /// (`<session>.label.txt`)
    pub label: Option<String>,
    /// How far along its Claude session is, as the Conductor's hub guesses it
    pub estimate: Option<Estimate>,
    /// The tab it waits on, when it's linked to one
    pub link: Option<TabLink>,
    /// What its Claude conversation has cost at list price, as Terminal's statusline last wrote
    /// it (`<session>.usage.json`), once it has
    pub usd: Option<f64>,
    /// Its Claude sessions are hidden from the Conductor, as its button.json says, or a Hide from
    /// Conductor picked for it waits for that (wezterm-gui's render/conductor_hide.rs;
    /// lbterm/NEW-TAB.md, section 8)
    pub hidden: bool,
    /// Parked (the mux tab's flag, Lachlan's 8a; lbterm/NEW-TAB.md, "Landing 4")
    pub parked: bool,
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
    /// Its progress, 0-100, while it's working or blocked and says so, else the hub's guess
    pub progress: Option<u8>,
    /// The progress is the hub's guess, not the program's word: its bar is dimmer
    pub estimated: bool,
    /// Its turn's time left as guessed, "~4 min", while the bar is a guess
    pub left: String,
    /// Its subagents' guessed progress, oldest first, at most [`AGENT_BARS`]
    pub agent_bars: Vec<u8>,
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
    /// Finished, and all it asks is whether you want extras: its blue pad
    /// gets a faint orange ring
    pub extras: bool,
    /// Waiting on another tab, and asking you something too: its purple (or red) pad gets an
    /// orange ring (Lachlan's 4a, 2026-10-29)
    pub asks: bool,
    /// Where its link stands, while the link decides its pad
    pub link: Option<LinkState>,
    /// Working, but its reply has written nothing for 3 minutes with no tool running: its green
    /// pad gets an amber ring (Lachlan's 2a, 2026-11-02). At 7 minutes it's [`RowState::Stuck`]
    pub quiet: bool,
    /// What its conversation has cost, "$12.34" (text::dollars), at the second line's end on the
    /// open panel (Lachlan's E6, 2026-11-07); empty until it's known
    pub cost: String,
    /// Hidden from the Conductor ([`Tab::hidden`]): the eye-off mark at its pad's bottom-right
    /// on the rail, and a "Hidden" chip on the open panel (lbterm/NEW-TAB.md, section 8, "The
    /// look"), so both looks count it and the row dissolves into it on `swap`
    pub hidden: bool,
    /// Parked ([`Tab::parked`], Lachlan's 8a): its pad, bars, number, label and marks dimmed on
    /// the rail ([`palette::parked`]) and a "Parked" chip on the open panel, so both looks count
    /// it and the row dissolves into it on `swap` (lbterm/NEW-TAB.md, "Landing 4")
    pub parked: bool,
}

impl Row {
    /// Its arc turns ([`Row::waiting`], with the compositor): not on a parked row, whose arc is
    /// drawn still and dimmed with its pad. The turning arc is one template the compositor places
    /// on every row that turns, in the Working green at full strength, so a parked row's would
    /// come undimmed, and keep a loop of DWM's going for a tab he's put aside (lbterm/PITFALLS.md,
    /// 255)
    pub fn arc_turns(&self) -> bool {
        self.waiting > 0 && !self.parked
    }
}

/// When a row next reads differently, in seconds from now, by what shows it: its age the open
/// panel alone shows, its idle bars the rail shows open or closed. A closed rail woke for every
/// finished tab's age each minute until 2026-10-30, to paint what nobody could see.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Changes {
    /// Its age ("now", then "1m", "2m" and on), on the open panel
    pub age: Option<f64>,
    /// Its idle bars' next step, on the rail
    pub step: Option<f64>,
}

impl Changes {
    /// When the rail, open (`open`) or closed, next shows the row otherwise
    pub fn next(&self, open: bool) -> Option<f64> {
        let age = self.age.filter(|_| open);
        match (age, self.step) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The sooner of each of two rows' changes
    pub fn min(self, other: Changes) -> Changes {
        let sooner = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Changes {
            age: sooner(self.age, other.age),
            step: sooner(self.step, other.step),
        }
    }
}

/// The row for `tab` at `now` (Unix seconds), the timer filling over
/// `idle_full` seconds. Also when what it shows next changes, if it will.
pub fn row(tab: &Tab, now: f64, idle_full: f64) -> (Row, Changes) {
    let (row, changes, _) = row_since(tab, now, idle_full);
    (row, changes)
}

/// Whether `tab`'s row waits on you at `now`, and since when (Unix seconds), for the side
/// buttons' Shift+Forward (lbterm_extras::hop, Lachlan's 9a; lbterm/NEW-TAB.md, "Landing 4"):
/// `Some((true, since))` blocked on you (orange: a permission prompt, a question, a login, or
/// waiting on another tab and asking you something too), `Some((false, since))` finished (your
/// turn, done, extras, failed, or its link gone wrong), `None` at work, stuck, without Claude,
/// waiting on another tab, or handed back with its own tasks still running, which carries on by
/// itself. `since` is what its age and idle timer count from, kept out of [`Row`] so a program's
/// record sent again with a newer time doesn't count as a row that moved; `now` when it has none
pub fn waits_on_you(tab: &Tab, now: f64, idle_full: f64) -> Option<(bool, f64)> {
    let (row, _, since) = row_since(tab, now, idle_full);
    let asks = match row.state {
        RowState::Waiting => true,
        RowState::Linked if row.asks => true,
        RowState::Idle if row.waiting == 0 => false,
        RowState::Error => false,
        _ => return None,
    };
    Some((asks, since.unwrap_or(now)))
}

/// All a row reads of a tab's titles ([`row_since`] reads them through this alone): the tab's own
/// title without its glyphs, the active pane's program name, and the first Claude glyph of the
/// tab's title, then its panes'. Claude Code's ◐ and ◑ read the same: both are `Glyph::Still`,
/// and stripped from the words
#[derive(Debug, PartialEq, Eq)]
struct TitlesRead<'a> {
    named: &'a str,
    program: &'a str,
    glyph: Option<Glyph>,
}

fn titles_read<'a>(tab_title: &'a str, pane_titles: &'a [String]) -> TitlesRead<'a> {
    let active_title = pane_titles.first().map(String::as_str).unwrap_or("");
    TitlesRead {
        named: text::strip_glyphs(tab_title),
        program: text::program_name(text::strip_glyphs(active_title)),
        glyph: std::iter::once(tab_title)
            .chain(pane_titles.iter().map(String::as_str))
            .find_map(claude_glyph),
    }
}

/// [`row`], and when its status last changed (`since`), while it's blocked or finished
fn row_since(tab: &Tab, now: f64, idle_full: f64) -> (Row, Changes, Option<f64>) {
    let titles = titles_read(&tab.tab_title, &tab.pane_titles);
    let mut title = titles.named.to_string();
    // (A program's own title, below, wins over all of these)
    if title.is_empty() {
        title = titles.program.to_string();
    }
    if title.is_empty() {
        title = format!("tab {}", tab.index);
    }

    let reported = tab
        .programs
        .iter()
        .filter(|p| p.id.is_empty())
        .max_by(|a, b| a.updated.total_cmp(&b.updated));
    let reports = reported.is_some();
    // The hooks' light, the newest in the tab
    let hooks = newest(tab.statuses.iter().copied());
    let light = hooks.and_then(|s| s.colour);
    // When the hooks last said the turn ended (any light but working), and whether a report is
    // from before that: Claude Code can leave its own record at working and a part's record up
    // after it hands back (seen 2026-10-25 on a forked session: "1 agent" and Working an hour
    // after it stopped, with nothing running), so the newer word wins
    let stopped_at = hooks
        .filter(|s| s.colour != Some(WORKING))
        .and_then(|s| s.unix_time());
    let before_stop = |p: &Program| stopped_at.is_some_and(|t| t > p.updated + STALE_GRACE);
    let reported = reported.filter(|p| !(p.state == ProgramState::Working && before_stop(p)));
    // gated: held at a permission prompt or a login, which no tab link speaks for
    let (state, since, status, message, progress, unread, extras, gated) = match reported {
        // The program's own word
        Some(p) => {
            if let Some(own) = p.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                title = own.to_string();
            }
            // ...but for what only the hooks know once it has handed the turn back: a question
            // asked in text, or only extras on offer (in text, or on a picker of extras alone)
            let handed_back = matches!(p.state, ProgramState::Done | ProgramState::Idle);
            let asks = handed_back && light == Some(WAITING);
            let extras = light == Some(EXTRAS)
                && (handed_back
                    || (p.state == ProgramState::Blocked
                        && matches!(p.kind, Some(Blocked::Question) | None)));
            let state = match p.state {
                _ if asks => RowState::Waiting,
                _ if extras => RowState::Idle,
                ProgramState::Working => RowState::Working,
                ProgramState::Blocked => RowState::Waiting,
                ProgramState::Done | ProgramState::Idle => RowState::Idle,
                ProgramState::Error => RowState::Error,
            };
            // Short, to fit beside the project on a narrow panel
            let words = match (p.state, p.kind) {
                _ if asks => "Question",
                _ if extras => "Extras",
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
                ProgramState::Working | ProgramState::Blocked if !extras => p.progress,
                _ => None,
            };
            let gated = p.state == ProgramState::Blocked
                && matches!(p.kind, Some(Blocked::Permission | Blocked::Auth))
                && !asks
                && !extras;
            (
                state,
                (p.state != ProgramState::Working).then_some(p.updated),
                words.to_string(),
                p.msg.as_deref().map(str::trim).unwrap_or_default().to_string(),
                progress,
                p.state == ProgramState::Done && p.updated > tab.seen,
                extras,
                gated,
            )
        }
        // The hooks' light and the title's glyph
        None => {
            // The tab's own title, then the active pane's: the title the row shows
            let glyph = titles.glyph;
            let state = row_state(hooks.as_ref(), glyph);
            let since = match state {
                RowState::Waiting | RowState::Idle => hooks.and_then(|s| s.unix_time()),
                _ => None,
            };
            let extras = light == Some(EXTRAS);
            let words = match state {
                RowState::Working => "Working",
                RowState::Waiting => "Blocked",
                RowState::Idle if extras => "Extras",
                RowState::Idle => "Your turn",
                RowState::Error => "Failed",
                RowState::NoClaude | RowState::Linked | RowState::Stuck => "",
            };
            // The hooks' orange can't tell a permission prompt from a question: a question
            (state, since, words.to_string(), String::new(), None, false, extras, false)
        }
    };
    // Its parts still at it: Claude Code's subagents and background tasks. One from before the
    // turn ended is left over, unless it's one of claudehook's `bg/` records, which say what
    // still runs after the turn and are sent again every minute while it does
    let agents = tab
        .programs
        .iter()
        .filter(|p| !p.id.is_empty())
        .filter(|p| matches!(p.state, ProgramState::Working | ProgramState::Blocked))
        .filter(|p| p.id.starts_with("bg/") || !before_stop(p))
        .count();
    let (age, level, wake) = match since {
        Some(since) => {
            let age = (now - since).max(0.0);
            let changes = Changes {
                age: Some(text::ago_changes_in(age)),
                step: idle::next_step_in(age, idle_full),
            };
            (text::ago(age), idle::level(age, idle_full), changes)
        }
        None => (String::new(), 0, Changes::default()),
    };

    // Handed back, with parts still at it: waiting on them, not stopped
    let waiting = if reports && state == RowState::Idle { agents } else { 0 };
    let (status, message) = if waiting > 0 {
        let s = if waiting == 1 { "" } else { "s" };
        (format!("Waiting on {waiting} task{s}"), String::new())
    } else {
        (status, message)
    };
    // Waiting on another tab (a tab link), once it has handed the turn back: purple, or red when
    // the link went wrong, with an orange ring when it asks you something too (Lachlan's 4a,
    // 2026-10-29). Working, a permission prompt and a login stay the program's
    let (state, status, message, extras, asks, link) = match &tab.link {
        Some(l) if state != RowState::Working && state != RowState::NoClaude && !gated => {
            let asks = state == RowState::Waiting;
            let on = if l.on.is_empty() { "another tab" } else { l.on.as_str() };
            let then = |head: String, tail: &str| if tail.is_empty() { head } else { format!("{head}: {tail}") };
            let (state, words) = match l.state {
                LinkState::Waiting => (RowState::Linked, then(format!("Waiting on {on}"), &l.what)),
                LinkState::Telling => (RowState::Linked, format!("{on} is done, telling it")),
                LinkState::Told => (RowState::Linked, format!("{on} is done, told it to carry on")),
                LinkState::Stuck => (RowState::Error, then("Still blocked".to_string(), &l.note)),
                LinkState::Failed => (RowState::Error, then("Couldn't tell it".to_string(), &l.note)),
                LinkState::Closed => (RowState::Error, format!("{on} closed before it finished")),
            };
            (state, words, String::new(), false, asks, Some(l.state))
        }
        _ => (state, status, message, extras, false, None),
    };
    // The hub's guesses: the turn's bar only where the program gives none of its own and the
    // turn is still going, and its subagents' whatever the turn is doing
    let estimate = tab.estimate.as_ref();
    let guessed = estimate
        .and_then(|e| e.percent)
        .filter(|_| progress.is_none() && matches!(state, RowState::Working | RowState::Waiting));
    let left = guessed
        .and(estimate.and_then(|e| e.left))
        .map(text::left)
        .unwrap_or_default();
    let agent_bars = estimate.map_or(vec![], |e| e.agents.iter().take(AGENT_BARS).copied().collect());
    let named = titles.named.trim();
    let label = if !named.is_empty() {
        named.to_string()
    } else {
        match tab.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            Some(label) => label.to_string(),
            None => title.clone(),
        }
    };
    // A working reply gone quiet or stuck (claude_quiet): the newest in the tab, believed only
    // while it's newer than the hooks' last light, since a light set after it says the session
    // has moved on, whatever the statusline has yet to clear (it looks every 10 s)
    let newest_light = hooks.and_then(|s| s.stamp);
    let quiet = tab
        .quiets
        .iter()
        .max_by_key(|q| q.stamp)
        .filter(|q| state == RowState::Working && newest_light.map_or(true, |s| q.stamp > s));
    let mut wake = wake;
    let (state, status, message, quiet) = match quiet {
        Some(q) if q.level == QuietLevel::Stuck => {
            let secs = (now - q.since).max(0.0);
            // Its minutes, counted on the open panel
            wake.age = Some(60.0 - secs % 60.0);
            (RowState::Stuck, format!("Stuck? {}m", (secs / 60.0) as u64), String::new(), false)
        }
        Some(q) if q.level == QuietLevel::Quiet => (state, status, message, true),
        _ => (state, status, message, false),
    };
    let row = Row {
        index: tab.index,
        title,
        project: text::project(&tab.cwd),
        state,
        status,
        message,
        agents,
        progress: progress.or(guessed),
        estimated: guessed.is_some(),
        left,
        agent_bars,
        unread,
        age,
        level,
        bars: idle::bars(level),
        active: tab.active,
        label,
        waiting,
        extras,
        asks,
        link,
        quiet,
        cost: tab.usd.map(text::dollars).unwrap_or_default(),
        hidden: tab.hidden,
        parked: tab.parked,
    };
    (row, wake, since)
}

/// Whether the tab's row, as `drawn` last made it, would show anything else with `programs` for
/// its OSC 7501 records, everything else as it was and at the same `now`. Most of what a working
/// Claude Code reports changes its parts' words or nothing a row shows, and each report repainted
/// the whole window, every pane walked. Asked without the tab's link as well: the window strip
/// and the projects card read the row without one, and a link speaks over the program's state
pub fn programs_move_row(drawn: &Tab, programs: &[Program], now: f64, idle_full: f64) -> bool {
    if drawn.programs == programs {
        return false;
    }
    let mut after = drawn.clone();
    after.programs = programs.to_vec();
    moves_row(drawn, after, now, idle_full)
}

/// Whether the tab's row, as `drawn` last made it, would show anything else with these titles,
/// everything else as it was and at the same `now`: a background tab's title reaches its row only
/// by a paint, and nothing else paints for it (its pane's output repaints only the open tab).
/// Claude Code's ◐ and ◑, turned every 960 ms while it works, are stripped from the row's words
/// and both say it's still there, so they paint nothing. Asked without the link too, as
/// programs_move_row is. Titles a row reads the same as the drawn ones ([`titles_read`]) answer
/// at once, with no row made: ◐ to ◑ is three strips and two glyphs, where making the rows cloned
/// the tab and worked out two to four rows (most of a title alert's 45 µs in the window, by
/// lbterm/FLOWS.md's row t14's estimate) for each of a working Claude Code's
/// titles, which a window's mux subscription now asks before any task (wezterm-gui's
/// termwindow/titles.rs, lbterm_title_alert_dropped)
pub fn titles_move_row(drawn: &Tab, tab_title: &str, pane_titles: &[String], now: f64, idle_full: f64) -> bool {
    if drawn.tab_title == tab_title && drawn.pane_titles == pane_titles {
        return false;
    }
    if titles_read(&drawn.tab_title, &drawn.pane_titles) == titles_read(tab_title, pane_titles) {
        return false;
    }
    let mut after = drawn.clone();
    after.tab_title = tab_title.to_string();
    after.pane_titles = pane_titles.to_vec();
    moves_row(drawn, after, now, idle_full)
}

/// Whether `after` makes another row than `drawn` did at `now`, with the tab's link and without
/// it: the window strip and the projects card read the row without one, and a link speaks over
/// the program's state
fn moves_row(drawn: &Tab, mut after: Tab, now: f64, idle_full: f64) -> bool {
    let moved = |a: &Tab, b: &Tab| row(a, now, idle_full).0 != row(b, now, idle_full).0;
    if moved(drawn, &after) {
        return true;
    }
    if drawn.link.is_none() {
        return false;
    }
    let mut before = drawn.clone();
    before.link = None;
    after.link = None;
    moved(&before, &after)
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
            cwd: "file:///C:/Users/me/AI/Terminal/".into(),
            statuses: vec![Status { colour: None, stamp: Some(STAMP) }],
            programs: vec![],
            seen: 0.0,
            active: true,
            label: None,
            estimate: None,
            link: None,
            quiets: vec![],
            usd: None,
            hidden: false,
            parked: false,
        }
    }

    #[test]
    fn a_tabs_row_says_what_its_conversation_cost_once_its_known() {
        assert_eq!(row(&tab(), 1_000.0, 3_600.0).0.cost, "");
        let costed = Tab { usd: Some(12.349), ..tab() };
        assert_eq!(row(&costed, 1_000.0, 3_600.0).0.cost, "$12.34");
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
        crate::names::made_up();   // the row's project is the megaproject's name for its folder (names.rs)
        let (row, wake) = row(&tab(), UNIX + 20.0 * 60.0 + 5.0, 3600.0);
        assert_eq!(row.title, "Tab panel for LBTerm");
        assert_eq!(row.project, "Terminal");
        assert_eq!(row.state, RowState::Idle);
        assert_eq!(row.status, "Your turn");
        assert_eq!(row.age, "20m");
        assert_eq!(row.level, 3);
        assert_eq!(row.bars, [Bar::Green; 3]);
        // Next change: the age text at 21m (55 s), before the next timer step; the closed rail,
        // which shows no age, at that step (22m 30s, 145 s on)
        assert_eq!(wake.next(true), Some(55.0));
        assert_eq!(wake.next(false), Some(145.0));
        assert!(row.active);
    }

    #[test]
    fn a_closed_rail_wakes_for_the_idle_steps_alone() {
        // Two rows: the rail open wakes for the sooner age, closed for the sooner step
        let a = Changes { age: Some(30.0), step: Some(200.0) };
        let b = Changes { age: Some(10.0), step: None };
        let both = a.min(b);
        assert_eq!(both, Changes { age: Some(10.0), step: Some(200.0) });
        assert_eq!((both.next(true), both.next(false)), (Some(10.0), Some(200.0)));
        // Ages alone (the bars full): nothing for a closed rail to wake for
        assert_eq!(b.next(false), None);
        assert_eq!(Changes::default().min(Changes::default()).next(true), None);
    }

    #[test]
    fn a_working_session_has_no_age_and_no_timer() {
        let mut t = tab();
        t.statuses = vec![Status { colour: Some(WORKING), stamp: Some(STAMP) }];
        let (row, wake) = row(&t, UNIX + 999.0, 3600.0);
        assert_eq!(row.state, RowState::Working);
        assert_eq!((row.age.as_str(), row.level, wake.next(true), wake.next(false)), ("", 0, None, None));
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
    fn a_parked_rows_arc_is_still_while_its_tasks_run() {
        let mut t = tab();
        t.programs = vec![
            program("", ProgramState::Done, UNIX),
            program("bg/b1", ProgramState::Working, UNIX),
        ];
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert!(r.arc_turns());
        t.parked = true;
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        // Still waiting on its task, said the same, but its arc is drawn still with its pad
        assert_eq!((r.waiting, r.parked, r.arc_turns()), (1, true, false));
        t.parked = false;
        t.programs[1].state = ProgramState::Done;
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert!(!r.arc_turns());
    }

    #[test]
    fn what_the_program_left_up_from_before_the_stop_is_over() {
        // The report of 2026-10-25: a session that handed back at 12:38 still said Working and
        // "1 agent" at 13:50, with nothing running. The hooks' stop (the tab's light, at UNIX)
        // came a minute after Claude Code's last working words, its own and a part's
        let mut t = tab();
        t.programs = vec![
            program("", ProgramState::Working, UNIX - 60.0),
            program("/fork/a1", ProgramState::Working, UNIX - 60.0),
        ];
        let (r, _) = row(&t, UNIX + 4320.0, 3600.0);
        assert_eq!((r.state, r.agents, r.status.as_str()), (RowState::Idle, 0, "Your turn"));
        // A claudehook bg/ record still counts after the stop: it's sent again while it runs
        t.programs.push(program("bg/b1", ProgramState::Working, UNIX - 60.0));
        let (r, _) = row(&t, UNIX + 60.0, 3600.0);
        assert_eq!((r.agents, r.waiting, r.status.as_str()), (1, 1, "Waiting on 1 task"));
        // A report a moment either side of the stop is the program's word, as before
        t.programs = vec![
            program("", ProgramState::Working, UNIX - 1.0),
            program("/fork/a1", ProgramState::Working, UNIX - 1.0),
        ];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.agents), (RowState::Working, 1));
        // So is one after it, and a working light makes nothing stale
        t.programs = vec![program("", ProgramState::Working, UNIX + 30.0)];
        assert_eq!(row(&t, UNIX + 40.0, 3600.0).0.state, RowState::Working);
        t.statuses = vec![lit(WORKING)];
        t.programs = vec![program("", ProgramState::Working, UNIX - 600.0)];
        assert_eq!(row(&t, UNIX, 3600.0).0.state, RowState::Working);
    }

    #[test]
    fn the_hubs_guess_fills_in_for_a_program_that_gives_no_progress() {
        let mut t = tab();
        t.programs = vec![program("", ProgramState::Working, UNIX)];
        t.estimate = Some(Estimate { percent: Some(40), left: Some(240), agents: vec![50, 10] });
        let (r, _) = row(&t, UNIX, 3600.0);
        assert_eq!((r.progress, r.estimated, r.left.as_str()), (Some(40), true, "~4 min"));
        assert_eq!(r.agent_bars, vec![50, 10]);
        // The program's own word wins, and then no time is guessed; the subagents' bars stay
        t.programs[0].progress = Some(70);
        let (r, _) = row(&t, UNIX, 3600.0);
        assert_eq!((r.progress, r.estimated, r.left.as_str()), (Some(70), false, ""));
        assert_eq!(r.agent_bars, vec![50, 10]);
        // Waiting on Lachlan still shows the guess (it stands still meanwhile)
        t.programs = vec![Program { kind: Some(Blocked::Permission), ..program("", ProgramState::Blocked, UNIX) }];
        assert_eq!(row(&t, UNIX, 3600.0).0.progress, Some(40));
        // Handed back: no turn bar, but a subagent still at it keeps its own
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        t.estimate = Some(Estimate { percent: Some(40), left: Some(240), agents: vec![80] });
        let (r, _) = row(&t, UNIX, 3600.0);
        assert_eq!((r.progress, r.estimated, r.left.as_str()), (None, false, ""));
        assert_eq!(r.agent_bars, vec![80]);
        // At most four subagents' bars
        t.estimate = Some(Estimate { percent: None, left: None, agents: vec![1, 2, 3, 4, 5, 6] });
        assert_eq!(row(&t, UNIX, 3600.0).0.agent_bars, vec![1, 2, 3, 4]);
        // No hub, no guesses: as before
        t.estimate = None;
        let (r, _) = row(&t, UNIX, 3600.0);
        assert_eq!((r.progress, r.estimated, r.agent_bars.len()), (None, false, 0));
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

    fn lit(colour: u32) -> Status {
        Status { colour: Some(colour), stamp: Some(STAMP) }
    }

    #[test]
    fn a_row_waits_on_you_blocked_or_finished_since_its_status_changed() {
        let now = UNIX + 600.0;
        // Finished, by the hooks' stop: since the light went out
        let mut t = tab();
        assert_eq!(waits_on_you(&t, now, 3600.0), Some((false, UNIX)));
        // Blocked on you: orange
        t.statuses = vec![lit(WAITING)];
        assert_eq!(waits_on_you(&t, now, 3600.0), Some((true, UNIX)));
        // The program's own word: blocked since its record, done since its record
        t.statuses.clear();
        t.programs = vec![program("", ProgramState::Blocked, UNIX + 30.0)];
        assert_eq!(waits_on_you(&t, now, 3600.0), Some((true, UNIX + 30.0)));
        t.programs = vec![program("", ProgramState::Error, UNIX + 40.0)];
        assert_eq!(waits_on_you(&t, now, 3600.0), Some((false, UNIX + 40.0)));
        // At work, or handed back with its own tasks still running: not on you
        t.programs = vec![program("", ProgramState::Working, UNIX)];
        assert_eq!(waits_on_you(&t, now, 3600.0), None);
        t.programs = vec![
            program("", ProgramState::Done, UNIX),
            program("bg/b1", ProgramState::Working, UNIX),
        ];
        assert_eq!(waits_on_you(&t, now, 3600.0), None);
        // Waiting on another tab: not on you, unless it asks you something too
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        t.link = Some(linked(LinkState::Waiting));
        assert_eq!(waits_on_you(&t, now, 3600.0), None);
        t.statuses = vec![lit(WAITING)];
        assert_eq!(waits_on_you(&t, now, 3600.0).map(|w| w.0), Some(true));
        // Without Claude
        let mut shell = tab();
        shell.pane_titles = vec!["pwsh".into()];
        shell.statuses.clear();
        assert_eq!(waits_on_you(&shell, now, 3600.0), None);
    }

    #[test]
    fn a_question_asked_in_text_waits_on_you_over_claudes_done() {
        let mut t = tab();
        t.statuses = vec![lit(WAITING)];
        for state in [ProgramState::Done, ProgramState::Idle] {
            t.programs = vec![program("", state, UNIX + 10.0)];
            let (r, _) = row(&t, UNIX + 70.0, 3600.0);
            assert_eq!((r.state, r.status.as_str(), r.extras), (RowState::Waiting, "Question", false));
            assert_eq!(r.age, "1m", "counted from Claude Code's report");
            assert!(!r.unread || state == ProgramState::Done);
        }
        // His next message: Claude Code works again, whatever the light last said
        t.programs = vec![program("", ProgramState::Working, UNIX + 80.0)];
        assert_eq!(row(&t, UNIX + 90.0, 3600.0).0.state, RowState::Working);
    }

    #[test]
    fn extras_alone_are_blue_with_their_ring() {
        let mut t = tab();
        t.statuses = vec![lit(EXTRAS)];
        // Offered in text, after the turn
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.status.as_str(), r.extras), (RowState::Idle, "Extras", true));
        // On a picker of extras alone, which Claude Code reports as blocked on a question
        let mut picker = program("", ProgramState::Blocked, UNIX);
        picker.kind = Some(Blocked::Question);
        picker.progress = Some(40);
        t.programs = vec![picker];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.status.as_str(), r.extras, r.progress), (RowState::Idle, "Extras", true, None));
        // A permission prompt still waits on you, and working is working
        let mut permission = program("", ProgramState::Blocked, UNIX);
        permission.kind = Some(Blocked::Permission);
        t.programs = vec![permission];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.extras), (RowState::Waiting, false));
        t.programs = vec![program("", ProgramState::Working, UNIX)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.extras), (RowState::Working, false));
        // A pane with no reports: the light alone
        t.programs.clear();
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.status.as_str(), r.extras), (RowState::Idle, "Extras", true));
        assert_eq!(row_state(Some(&lit(EXTRAS)), None), RowState::Idle);
    }

    #[test]
    fn a_plain_done_stays_done() {
        let mut t = tab();
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.status.as_str(), r.extras), (RowState::Idle, "Done", false));
        // Working's green under a done report: the light hasn't caught up yet, and done wins
        t.statuses = vec![lit(WORKING)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.status.as_str()), (RowState::Idle, "Done"));
    }

    fn linked(state: LinkState) -> TabLink {
        TabLink {
            state,
            on: "Tab Wave".into(),
            what: "LBTerm to land the links".into(),
            note: "The rail still has no lines".into(),
        }
    }

    #[test]
    fn a_tab_waiting_on_another_is_purple_once_it_has_handed_back() {
        let mut t = tab();
        t.link = Some(linked(LinkState::Waiting));
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!(
            (r.state, r.status.as_str(), r.message.as_str(), r.asks, r.link),
            (RowState::Linked, "Waiting on Tab Wave: LBTerm to land the links", "", false, Some(LinkState::Waiting))
        );
        assert_eq!(r.age, "now", "how long it's waited, from when it handed back");
        // ...asking him something too: an orange ring round the purple (his 4a)
        t.statuses = vec![lit(WAITING)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.asks), (RowState::Linked, true));
        // ...and extras alone are the link's purple, without the extras' ring
        t.statuses = vec![lit(EXTRAS)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.extras, r.asks), (RowState::Linked, false, false));
        // Working, a permission prompt and a login stay the program's
        t.statuses.clear();
        t.programs = vec![program("", ProgramState::Working, UNIX)];
        assert_eq!(row(&t, UNIX + 5.0, 3600.0).0.state, RowState::Working);
        for kind in [Blocked::Permission, Blocked::Auth] {
            let mut gate = program("", ProgramState::Blocked, UNIX);
            gate.kind = Some(kind);
            t.programs = vec![gate];
            let (r, _) = row(&t, UNIX + 5.0, 3600.0);
            assert_eq!((r.state, r.link), (RowState::Waiting, None));
        }
        // A picker's question is a question: purple with the orange ring
        let mut picker = program("", ProgramState::Blocked, UNIX);
        picker.kind = Some(Blocked::Question);
        t.programs = vec![picker];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.asks), (RowState::Linked, true));
    }

    #[test]
    fn a_link_says_where_it_stands_and_goes_red_when_it_went_wrong() {
        let mut t = tab();
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        let says = |t: &mut Tab, state: LinkState| {
            t.link = Some(linked(state));
            let (r, _) = row(t, UNIX + 5.0, 3600.0);
            (r.state, r.status)
        };
        assert_eq!(says(&mut t, LinkState::Telling), (RowState::Linked, "Tab Wave is done, telling it".into()));
        assert_eq!(says(&mut t, LinkState::Told), (RowState::Linked, "Tab Wave is done, told it to carry on".into()));
        assert_eq!(says(&mut t, LinkState::Stuck), (RowState::Error, "Still blocked: The rail still has no lines".into()));
        assert_eq!(says(&mut t, LinkState::Failed), (RowState::Error, "Couldn't tell it: The rail still has no lines".into()));
        assert_eq!(says(&mut t, LinkState::Closed), (RowState::Error, "Tab Wave closed before it finished".into()));
        // Nothing said of what it waits for: just whom
        t.link = Some(TabLink { what: String::new(), ..linked(LinkState::Waiting) });
        assert_eq!(row(&t, UNIX + 5.0, 3600.0).0.status, "Waiting on Tab Wave");
        // Told, and at it again: working
        t.link = Some(linked(LinkState::Told));
        t.programs = vec![program("", ProgramState::Working, UNIX + 1.0)];
        assert_eq!(row(&t, UNIX + 5.0, 3600.0).0.state, RowState::Working);
    }

    #[test]
    fn handed_back_with_tasks_and_a_link_turns_the_arc_but_says_the_link() {
        let mut t = tab();
        t.link = Some(linked(LinkState::Waiting));
        t.programs = vec![program("", ProgramState::Done, UNIX), program("bg/1", ProgramState::Working, UNIX)];
        let (r, _) = row(&t, UNIX + 5.0, 3600.0);
        assert_eq!((r.state, r.waiting, r.status.as_str()), (RowState::Linked, 1, "Waiting on Tab Wave: LBTerm to land the links"));
    }

    #[test]
    fn only_records_that_change_the_row_repaint() {
        let mut t = tab();
        let mut own = program("", ProgramState::Working, UNIX);
        own.msg = Some("Reading the files".into());
        let part = Program { msg: Some("Grep".into()), ..program("agent/1", ProgramState::Working, UNIX) };
        t.programs = vec![own.clone(), part.clone()];
        let moves = |t: &Tab, programs: &[Program]| programs_move_row(t, programs, UNIX + 30.0, 3600.0);
        // The same records, or the same with a later time: nothing
        assert!(!moves(&t, &t.programs.clone()));
        assert!(!moves(&t, &[Program { updated: UNIX + 20.0, ..own.clone() }, part.clone()]));
        // A part's words, which no row shows: nothing
        assert!(!moves(&t, &[own.clone(), Program { msg: Some("Read".into()), ..part.clone() }]));
        // Its own words, its state, its progress, its title, a part more or fewer: the row
        assert!(moves(&t, &[Program { msg: Some("Editing the code".into()), ..own.clone() }, part.clone()]));
        assert!(moves(&t, &[Program { state: ProgramState::Done, ..own.clone() }, part.clone()]));
        assert!(moves(&t, &[Program { progress: Some(30), ..own.clone() }, part.clone()]));
        assert!(moves(&t, &[Program { title: Some("Deploy".into()), ..own.clone() }, part.clone()]));
        assert!(moves(&t, &[own.clone()]));
        assert!(moves(&t, &[own.clone(), part.clone(), program("bg/b1", ProgramState::Working, UNIX)]));
        // A finished one's time is its age
        let done = program("", ProgramState::Done, UNIX);
        t.programs = vec![done.clone()];
        assert!(moves(&t, &[Program { updated: UNIX - 600.0, ..done.clone() }]));
    }

    #[test]
    fn a_link_hides_no_change_from_the_strip_and_the_projects_card() {
        // Handed back, seen, and linked: the rail says the link whether the program is done or
        // failed, but without the link (the strip's and the projects card's rows,
        // render/strip.rs and render/projects.rs) the program's own state shows
        let mut t = tab();
        t.link = Some(linked(LinkState::Stuck));
        t.seen = UNIX + 1.0;
        t.programs = vec![program("", ProgramState::Done, UNIX)];
        let mut failed = t.clone();
        failed.programs = vec![program("", ProgramState::Error, UNIX)];
        assert_eq!(row(&t, UNIX + 5.0, 3600.0).0, row(&failed, UNIX + 5.0, 3600.0).0, "the rail's row is the link's");
        assert!(programs_move_row(&t, &failed.programs, UNIX + 5.0, 3600.0));
        // And without a link, the same records that leave the row be are nothing
        t.link = None;
        assert!(!programs_move_row(&t, &[program("", ProgramState::Done, UNIX + 0.5)], UNIX + 5.0, 3600.0));
    }

    #[test]
    fn only_titles_that_change_the_row_repaint() {
        let mut t = tab();
        t.pane_titles = vec!["\u{25d0} Tab panel for LBTerm".into(), "pwsh".into()];
        let moves = |t: &Tab, tab_title: &str, panes: &[&str]| {
            let panes: Vec<String> = panes.iter().map(|p| p.to_string()).collect();
            titles_move_row(t, tab_title, &panes, UNIX + 30.0, 3600.0)
        };
        // The same, or Claude Code's other half circle, which a row strips: nothing
        assert!(!moves(&t, "", &["\u{25d0} Tab panel for LBTerm", "pwsh"]));
        assert!(!moves(&t, "", &["\u{25d1} Tab panel for LBTerm", "pwsh"]));
        // Another pane's title, which no row shows: nothing
        assert!(!moves(&t, "", &["\u{25d1} Tab panel for LBTerm", "vim"]));
        // New words, a name given to the tab, the glyph gone: the row
        assert!(moves(&t, "", &["\u{25d1} Land the branch", "pwsh"]));
        assert!(moves(&t, "Mine", &["\u{25d0} Tab panel for LBTerm", "pwsh"]));
        assert!(moves(&t, "", &["pwsh.exe", "pwsh"]));
        // A spinner in a pane without hooks is working, a still glyph idle
        t.statuses.clear();
        assert!(moves(&t, "", &["\u{2802} Tab panel for LBTerm", "pwsh"]));
        // A program's own title speaks over the pane's
        let mut own = program("", ProgramState::Working, UNIX);
        own.title = Some("Deploy".into());
        t.programs = vec![own];
        assert!(!moves(&t, "", &["\u{25d1} Land the branch", "pwsh"]));
    }

    #[test]
    fn titles_a_row_reads_the_same_never_make_another_row() {
        // The shortcut titles_move_row takes before making any row: for every pair of these
        // titles that a row reads the same (titles_read), the rows made from them are the same,
        // linked or not, with hooks or without
        let titles = [
            "\u{25d0} Tab panel for LBTerm",
            "\u{25d1} Tab panel for LBTerm",
            "\u{2733} Tab panel for LBTerm",
            "\u{2802} Tab panel for LBTerm",
            "\u{25d0}  Tab panel for LBTerm ",
            "Tab panel for LBTerm",
            "\u{25d1} Land the branch",
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            "pwsh.exe",
            "",
        ];
        let tabs = ["", "Mine", "\u{2733} Mine"];
        let mut bases = vec![tab()];
        let mut bare = tab();
        bare.statuses.clear();
        bases.push(bare.clone());
        bare.link = Some(linked(LinkState::Waiting));
        bases.push(bare);
        let mut pairs = 0;
        for base in &bases {
            for tab_a in tabs {
                for tab_b in tabs {
                    for a in titles {
                        for b in titles {
                            let (pa, pb) = (vec![a.to_string(), "pwsh".into()], vec![b.to_string(), "vim".into()]);
                            if titles_read(tab_a, &pa) != titles_read(tab_b, &pb) {
                                continue;
                            }
                            pairs += 1;
                            let mut drawn = base.clone();
                            drawn.tab_title = tab_a.into();
                            drawn.pane_titles = pa.clone();
                            let mut after = base.clone();
                            after.tab_title = tab_b.into();
                            after.pane_titles = pb.clone();
                            assert!(
                                !moves_row(&drawn, after, UNIX + 30.0, 3600.0),
                                "{tab_a:?} {pa:?} against {tab_b:?} {pb:?}"
                            );
                        }
                    }
                }
            }
        }
        assert!(pairs > 30, "only {pairs} pairs read the same");
    }

    #[test]
    fn a_linked_tab_s_title_moves_its_row_only_when_it_shows() {
        // Waiting on another tab, in a pane without hooks: Claude Code's still glyph keeps the
        // link's purple pad, another still glyph changes nothing, and a spinner (working) takes
        // the pad back from the link
        let mut t = tab();
        t.statuses.clear();
        t.link = Some(linked(LinkState::Waiting));
        t.pane_titles = vec!["\u{2733} Tab panel for LBTerm".into()];
        let now = UNIX + 5.0;
        assert_eq!(row(&t, now, 3600.0).0.state, RowState::Linked);
        assert!(!titles_move_row(&t, "", &["\u{2736} Tab panel for LBTerm".to_string()], now, 3600.0));
        assert!(titles_move_row(&t, "", &["\u{2802} Tab panel for LBTerm".to_string()], now, 3600.0));
    }

    #[test]
    fn quiet_values_parse_as_claudehook_writes_them() {
        assert_eq!(
            Quiet::parse("quiet|1793601839400|134380754393600000"),
            Some(Quiet { level: QuietLevel::Quiet, since: 1_793_601_839.4, stamp: 134_380_754_393_600_000 })
        );
        assert_eq!(Quiet::parse("stuck|1000|7").map(|q| (q.level, q.since)), Some((QuietLevel::Stuck, 1.0)));
        assert_eq!(Quiet::parse("none|0|7").map(|q| q.level), Some(QuietLevel::None));
        for bad in ["", "loud|1|2", "quiet|x|2", "quiet|1", "quiet|1|2|3", "quiet|-1|2", "006400|1"] {
            assert_eq!(Quiet::parse(bad), None, "{bad}");
        }
    }

    /// A working tab, as Claude Code and the hooks leave one mid-reply, with `claude_quiet` values
    fn quiet_tab(quiets: Vec<Quiet>) -> Tab {
        let mut t = tab();
        t.statuses = vec![Status { colour: Some(WORKING), stamp: Some(STAMP) }];
        let mut p = program("", ProgramState::Working, UNIX);
        p.msg = Some("Thinking".into());
        t.programs = vec![p];
        t.quiets = quiets;
        t
    }

    fn quiet(level: QuietLevel, after_light: u64) -> Quiet {
        Quiet { level, since: UNIX, stamp: STAMP + after_light }
    }

    #[test]
    fn a_quiet_reply_rings_its_green_pad_and_says_working_still() {
        let (r, wake) = row(&quiet_tab(vec![quiet(QuietLevel::Quiet, 1)]), UNIX + 200.0, 3600.0);
        assert_eq!((r.state, r.quiet, r.status.as_str(), r.message.as_str()), (RowState::Working, true, "Working", "Thinking"));
        assert_eq!(wake.next(true), None);
    }

    #[test]
    fn a_stuck_reply_turns_its_pad_amber_with_its_minutes() {
        let (r, wake) = row(&quiet_tab(vec![quiet(QuietLevel::Stuck, 1)]), UNIX + 8.0 * 60.0 + 5.0, 3600.0);
        assert_eq!((r.state, r.quiet, r.status.as_str()), (RowState::Stuck, false, "Stuck? 8m"));
        assert_eq!(r.message, "", "the program's own message would hide the minutes");
        // The minutes tick on the open panel; the closed rail shows no words
        assert_eq!((wake.next(true), wake.next(false)), (Some(55.0), None));
        let (r, _) = row(&quiet_tab(vec![quiet(QuietLevel::Stuck, 1)]), UNIX + 11.7 * 60.0, 3600.0);
        assert_eq!(r.status, "Stuck? 11m");
    }

    #[test]
    fn no_mark_once_it_is_none_older_than_the_light_or_not_working() {
        let plain = |t: &Tab| {
            let r = row(t, UNIX + 600.0, 3600.0).0;
            (r.state, r.quiet)
        };
        assert_eq!(plain(&quiet_tab(vec![quiet(QuietLevel::None, 1)])), (RowState::Working, false));
        // The newest value in the tab wins: none after stuck
        assert_eq!(plain(&quiet_tab(vec![quiet(QuietLevel::Stuck, 1), quiet(QuietLevel::None, 2)])), (RowState::Working, false));
        // A light set after it: the session moved on, whatever the statusline has yet to clear
        let mut t = quiet_tab(vec![quiet(QuietLevel::Stuck, 1)]);
        t.statuses.push(Status { colour: Some(WORKING), stamp: Some(STAMP + 2) });
        assert_eq!(plain(&t), (RowState::Working, false));
        // Handed back: the program's done wins
        let mut t = quiet_tab(vec![quiet(QuietLevel::Stuck, 1)]);
        t.programs = vec![program("", ProgramState::Done, UNIX + 500.0)];
        assert_eq!(plain(&t), (RowState::Idle, false));
        // Blocked on a permission prompt
        let mut t = quiet_tab(vec![quiet(QuietLevel::Quiet, 1)]);
        t.programs[0].state = ProgramState::Blocked;
        t.programs[0].kind = Some(Blocked::Permission);
        assert_eq!(plain(&t), (RowState::Waiting, false));
    }

    #[test]
    fn a_working_tab_without_reports_is_marked_from_the_light_too() {
        let mut t = quiet_tab(vec![quiet(QuietLevel::Stuck, 1)]);
        t.programs.clear();
        assert_eq!(row(&t, UNIX + 420.0, 3600.0).0.status, "Stuck? 7m");
    }
}
