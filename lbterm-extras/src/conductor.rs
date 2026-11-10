//! The Conductor (Lachlan's Life\Projects\Conductor, one Claude Code session that directs the
//! others) on LBTerm's tab rail (lbterm/DESIGN.md, "The tab panel"; `config.lbterm_conductor`).
//!
//! A pad at the top of the rail shows what Conductor writes to its state file: off, or on in a
//! mode (read only, accept most, accept all), and whether its remote control is on. A click on it
//! drops a menu of actions, offered by state and ticked by mode. While the Conductor thinks about
//! or acts on a Claude session, as its hub says over its pipe, the pad of the tab holding that
//! session gets a moving outline. LBTerm tells the hub which tab holds which session.
//!
//! The GUI side is wezterm-gui/src/state_files.rs (the state file), conductor_hub.rs (the hub)
//! and render/conductor.rs (the pad, its menu and the outlines).

use crate::tabs::Rect;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

/// The tab menu's Hide from Conductor and Show to Conductor, from the pick until the Conductor's
/// file says it ([`offer`] and [`confirmed`] over the picks a window has waiting)
pub mod hide;

/// What a state file says: its fields, by name, numbers and true or false as their JSON text
/// (Conductor's `updatedAt`, which tells a rewrite of the same words from no write at all).
/// A null, a list or an object is left out, as if the field weren't there, but for `hidden`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Said {
    pub fields: HashMap<String, String>,
    /// The sessions Lachlan hid from the Conductor, its button.json's `hidden` (lbterm/NEW-TAB.md,
    /// section 8). Nothing when the file has no list there, a Conductor from before 2026-11-02 that
    /// can't hide one: then the tab menu offers no Hide from Conductor. Empty is a Conductor that
    /// can, with none hidden.
    pub hidden: Option<Hidden>,
}

impl Said {
    /// A field's value, if it has one that isn't blank
    pub fn get(&self, field: &str) -> Option<&str> {
        self.fields.get(field).map(|s| s.trim()).filter(|s| !s.is_empty())
    }

    /// What it says about the Conductor, for the open panel's row
    pub fn tooltip(&self) -> Option<&str> {
        self.get("tooltip")
    }

    /// The state: what the file says, else off (no file, or none in it)
    pub fn state(&self) -> &str {
        self.get("state").unwrap_or("off")
    }
}

/// A state file's contents, or nothing when they aren't a JSON object. A byte-order mark
/// (Windows PowerShell 5.1's) is passed over. Read on the `lbterm-state-files` thread whenever
/// the file changes (wezterm-gui/src/state_files.rs), never on the GUI thread.
pub fn read(text: &str) -> Option<Said> {
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let object = value.as_object()?;
    let fields = object
        .iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k.clone(), s.clone())),
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => Some((k.clone(), v.to_string())),
            _ => None,
        })
        .collect();
    let hidden = object.get("hidden").and_then(Hidden::read);
    Some(Said { fields, hidden })
}

/// The sessions Lachlan hid from the Conductor, by Claude session id, as its button.json's
/// `hidden` lists them (`@Conductor:button`; `conductor hide|show` writes it). LBTerm hides
/// nothing itself: this is only what the Conductor says, for the tab menu's row and the rail's
/// mark (lbterm/NEW-TAB.md, section 8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hidden(BTreeSet<String>);

impl Hidden {
    /// The list as the file has it: every entry a session id. Anything else there (null, a word,
    /// a list with something not a string in it) is no list, so nothing is offered, and it's
    /// said in the log: the Conductor's contract says it's always a list of ids. A blank id names
    /// no session and is passed over.
    fn read(value: &serde_json::Value) -> Option<Hidden> {
        let read = value.as_array().and_then(|ids| {
            ids.iter()
                .map(|id| id.as_str().map(str::trim))
                .collect::<Option<Vec<&str>>>()
        });
        match read {
            Some(ids) => Some(Hidden(
                ids.into_iter().filter(|id| !id.is_empty()).map(str::to_string).collect(),
            )),
            None => {
                if !value.is_null() {
                    log::warn!("conductor: its state file's hidden isn't a list of session ids: {value}");
                }
                None
            }
        }
    }

    /// Whether the Conductor lists `session` as hidden from it
    pub fn has(&self, session: &str) -> bool {
        self.0.contains(session)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many of a tab's Claude sessions it lists. A tab with none is [`Cover::NoneOf`].
    pub fn cover<S: AsRef<str>>(&self, sessions: &[S]) -> Cover {
        let listed = sessions.iter().filter(|s| self.has(s.as_ref())).count();
        if listed == 0 {
            Cover::NoneOf
        } else if listed == sessions.len() {
            Cover::AllOf
        } else {
            Cover::SomeOf
        }
    }
}

/// How many of a tab's Claude sessions are hidden from the Conductor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cover {
    NoneOf,
    /// Some but not all: one moved in beside a hidden one, or hidden with `conductor hide` alone
    SomeOf,
    AllOf,
}

/// What the tab menu's Hide from Conductor row says (lbterm/NEW-TAB.md, section 7, row 6)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// "Hide from Conductor": hides every session of the tab, those hidden already rewritten
    Hide,
    /// "Show to Conductor": every session of the tab is hidden. It's also when the tab's pad
    /// carries the hidden mark, so the mark and the row always agree
    Show,
    /// "Hiding from Conductor…" (`hide`) or "Showing to Conductor…": a pick for the tab waits for
    /// the Conductor's word, and the row is there greyed, saying so, and picks nothing (Lachlan's
    /// 11b of 2026-11-09; `menu::inert`). Only [`hide::Picks::offer`] says it, never the file
    Waiting { hide: bool },
}

/// The row for a tab whose Claude sessions are `sessions`, as the Conductor's state file says
/// now: none on a tab with no Claude session, and none while the file has no `hidden` list (no
/// file, or a Conductor that can't hide sessions: no half version). The menu asks
/// [`hide::Picks::offer`], which makes it [`Offer::Waiting`] while a pick for the tab waits for
/// the Conductor's word
pub fn offer<S: AsRef<str>>(said: &Said, sessions: &[S]) -> Option<Offer> {
    let hidden = said.hidden.as_ref()?;
    if sessions.is_empty() {
        return None;
    }
    Some(match hidden.cover(sessions) {
        Cover::AllOf => Offer::Show,
        Cover::NoneOf | Cover::SomeOf => Offer::Hide,
    })
}

/// Whether the Conductor has done what a pick asked of it for `sessions`: a hide once its list
/// names every one of them, a show once it names none. A file without the list confirms nothing.
pub fn confirmed<S: AsRef<str>>(said: &Said, sessions: &[S], hide: bool) -> bool {
    let Some(hidden) = said.hidden.as_ref() else {
        return false;
    };
    match hidden.cover(sessions) {
        Cover::AllOf => hide,
        Cover::NoneOf => !hide,
        Cover::SomeOf => false,
    }
}

/// Whether a menu item is offered: each field `when` names holds one of its values (a field
/// the file doesn't have holds `""`, but `state` holds `off`). An empty `when` offers it always.
pub fn offered(when: &HashMap<String, Vec<String>>, said: &Said) -> bool {
    when.iter().all(|(field, values)| {
        let have = if field == "state" {
            said.state()
        } else {
            said.get(field).unwrap_or("")
        };
        values.iter().any(|v| v == have)
    })
}

/// Whether a menu item is ticked: it names fields, and each holds the value named
pub fn ticked(checked: &HashMap<String, String>, said: &Said) -> bool {
    !checked.is_empty()
        && checked
            .iter()
            .all(|(field, value)| said.get(field) == Some(value.as_str()))
}

/// How far the Conductor may act on its own, which sets its pad's outline and its label's colour
/// (lbterm/DESIGN.md: read green, accept most yellow, accept all red, picked 2026-10-21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Off: the pad dim, no outline
    Off,
    /// On, read only: a grey outline
    ReadOnly,
    /// On, accepting most or all: a still outline in the mode's colour, the spectrum's only while
    /// it acts or the pointer is on it (Lachlan's 7a of 2026-11-01; [`outline`])
    AcceptMost,
    AcceptAll,
}

/// The Conductor's pad, as its state file has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pad {
    pub level: Level,
    /// Under the pad: "off", or the mode ("read", "most", "all", or whatever new one it names)
    pub label: String,
    /// Its remote control is on: a mark in the pad's corner
    pub remote: bool,
}

pub fn pad(said: &Said) -> Pad {
    let state = said.state();
    if state == "off" {
        return Pad {
            level: Level::Off,
            label: "off".to_string(),
            remote: false,
        };
    }
    let mode = said.get("mode").unwrap_or("read");
    let level = match mode {
        "most" => Level::AcceptMost,
        "all" => Level::AcceptAll,
        _ => Level::ReadOnly,
    };
    Pad {
        level,
        label: mode.to_string(),
        remote: state == "remote",
    }
}

/// What the Conductor is doing with a session, as its hub says. Acting outranks thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Doing {
    /// Reading it, or working out what to do about it: the iridescent outline
    Thinking,
    /// Messaging, answering, starting, resuming or stopping it: the outline and a light running
    /// round it
    Acting,
}

/// An outline shows at least this long once it's up, so a quick action still shows: one lap of
/// the light acting, a moment thinking
pub const ACTING_AT_LEAST: Duration = Duration::from_millis(1400);
pub const THINKING_AT_LEAST: Duration = Duration::from_millis(600);
/// And stays this long after the hub stops naming it
pub const LINGER: Duration = Duration::from_millis(400);

/// A session the hub names: what the Conductor is doing with it, and when the hub says that ends
/// whether or not it's named again (`until`, epoch ms): a process that dies mid-action, or an
/// action whose end nobody sees, can't leave a pad lit for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub session_id: String,
    pub doing: Doing,
    pub until_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    doing: Doing,
    since: Instant,
    /// When the hub stopped naming it
    gone: Option<Instant>,
    /// When the hub said it ends
    until: Option<Instant>,
}

impl Entry {
    fn least(&self) -> Duration {
        match self.doing {
            Doing::Acting => ACTING_AT_LEAST,
            Doing::Thinking => THINKING_AT_LEAST,
        }
    }

    /// When it stops showing: a moment after the hub stops naming it, or at its `until`,
    /// whichever comes first, but never before it has shown its least
    fn ends(&self) -> Option<Instant> {
        let end = match (self.gone.map(|g| g + LINGER), self.until) {
            (Some(a), Some(b)) => a.min(b),
            (a, b) => a.or(b)?,
        };
        Some(end.max(self.since + self.least()))
    }
}

/// `until_ms` (epoch ms) as an instant, from what both clocks say now
fn instant(until_ms: Option<u64>, now: Instant, now_ms: u64) -> Option<Instant> {
    until_ms.map(|until| now + Duration::from_millis(until.saturating_sub(now_ms)))
}

/// Which sessions the Conductor is thinking about or acting on, as the hub last said, each
/// lingering a moment after.
#[derive(Debug, Clone, Default)]
pub struct Focus {
    entries: HashMap<String, Entry>,
}

impl Focus {
    /// The hub's whole list, as of `now` (`now_ms` since the epoch, for its `until`s): sessions it
    /// names show from now (or carry on, their `until` as it says now), the rest start to go.
    /// True when what shows changed.
    pub fn set(&mut self, sessions: &[Named], now: Instant, now_ms: u64) -> bool {
        let mut changed = false;
        for named in sessions {
            let until = instant(named.until_ms, now, now_ms);
            match self.entries.get_mut(&named.session_id) {
                Some(entry) if entry.doing == named.doing => {
                    entry.gone = None;
                    entry.until = until;
                }
                Some(entry) => {
                    *entry = Entry {
                        doing: named.doing,
                        since: now,
                        gone: None,
                        until,
                    };
                    changed = true;
                }
                None => {
                    self.entries.insert(
                        named.session_id.clone(),
                        Entry {
                            doing: named.doing,
                            since: now,
                            gone: None,
                            until,
                        },
                    );
                    changed = true;
                }
            }
        }
        for (sid, entry) in self.entries.iter_mut() {
            if entry.gone.is_none() && !sessions.iter().any(|n| &n.session_id == sid) {
                entry.gone = Some(now);
            }
        }
        changed | self.prune(now)
    }

    /// Lose every one: the hub went
    pub fn clear(&mut self) -> bool {
        let had = !self.entries.is_empty();
        self.entries.clear();
        had
    }

    /// Drop what has finished lingering by `now`: true when something went
    pub fn prune(&mut self, now: Instant) -> bool {
        let before = self.entries.len();
        self.entries.retain(|_, e| e.ends().map_or(true, |end| end > now));
        self.entries.len() != before
    }

    /// What the Conductor is doing with session `sid` at `now`
    pub fn get(&self, sid: &str, now: Instant) -> Option<Doing> {
        let entry = self.entries.get(sid)?;
        entry.ends().map_or(true, |end| end > now).then_some(entry.doing)
    }

    /// When an outline next goes, for a repaint then
    pub fn next_change(&self) -> Option<Instant> {
        self.entries.values().filter_map(Entry::ends).min()
    }

    /// Whether the Conductor is acting on any session at `now`, wherever that session is, its
    /// outline's least lap and linger included: what lights its own pad's spectrum in Accept most
    /// and Accept all ([`outline`]). Thinking about one isn't acting. It ends when the last acting
    /// outline does, so [`Focus::next_change`] covers when it goes, and [`Focus::set`]'s true when
    /// it comes
    pub fn acting(&self, now: Instant) -> bool {
        self.entries
            .values()
            .any(|e| e.doing == Doing::Acting && e.ends().map_or(true, |end| end > now))
    }
}

/// The hub's line saying which sessions the Conductor is on, every one each time:
/// `{"type":"conductor.focus","sessions":[{"sessionId":"…","doing":"thinking"|"acting",
/// "until":<epoch ms, optional>}]}`. Nothing for any other line; a session with a `doing`
/// LBTerm doesn't know is passed over, and anything else in an entry (an action's name) is left
/// alone.
pub fn focus_message(line: &str) -> Option<Vec<Named>> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("type")?.as_str()? != "conductor.focus" {
        return None;
    }
    let sessions = value.get("sessions")?.as_array()?;
    Some(
        sessions
            .iter()
            .filter_map(|s| {
                let sid = s.get("sessionId")?.as_str()?.trim();
                let doing = match s.get("doing")?.as_str()? {
                    "thinking" => Doing::Thinking,
                    "acting" => Doing::Acting,
                    _ => return None,
                };
                (!sid.is_empty()).then(|| Named {
                    session_id: sid.to_string(),
                    doing,
                    until_ms: s.get("until").and_then(|u| u.as_u64()),
                })
            })
            .collect(),
    )
}

/// How far along a Claude session's turn is, and each of its subagents, as the hub guesses it
/// (Conductor's estimate.ts: Haiku's guess every few minutes, the bar moving on between them).
/// A guess, not a report: the rail draws it dimmer than a program's own progress.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Estimate {
    /// The turn itself: none once it has handed back, while subagents still work
    pub percent: Option<u8>,
    /// Its working seconds still to go
    pub left_secs: Option<u32>,
    /// Its parts, oldest first: subagents and the commands it left running in the background,
    /// drawn alike
    pub agents: Vec<AgentEstimate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEstimate {
    pub id: String,
    /// What it was asked to do, in a few words
    pub what: String,
    pub percent: u8,
    pub left_secs: u32,
}

/// The hub's line with every session's bars, the whole list each time, and when to drop it if
/// the hub says nothing more (`until`, epoch ms: the hub says it again every 45 s).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Estimates {
    pub until_ms: Option<u64>,
    pub sessions: HashMap<String, Estimate>,
}

/// `{"type":"conductor.estimates","until":<epoch ms>,"sessions":[{"sessionId":"…","percent":40,
/// "left":180,"agents":[{"id":"…","kind":"agent"|"shell","what":"…","percent":50,"left":120}]}]}`.
/// Nothing for any other line; a session without an id, or a part without a percent, is passed
/// over, a percent over 100 is 100, and anything else in an entry (`kind`) is left alone.
pub fn estimates_message(line: &str) -> Option<Estimates> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("type")?.as_str()? != "conductor.estimates" {
        return None;
    }
    let percent = |v: &serde_json::Value| v.as_u64().map(|p| p.min(100) as u8);
    let secs = |v: &serde_json::Value| v.as_u64().map(|s| s.min(u32::MAX as u64) as u32);
    let sessions = value
        .get("sessions")?
        .as_array()?
        .iter()
        .filter_map(|s| {
            let sid = s.get("sessionId")?.as_str()?.trim();
            if sid.is_empty() {
                return None;
            }
            let agents = s
                .get("agents")
                .and_then(|a| a.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|a| {
                            Some(AgentEstimate {
                                id: a.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                                what: a.get("what").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                                percent: a.get("percent").and_then(percent)?,
                                left_secs: a.get("left").and_then(secs).unwrap_or(0),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some((
                sid.to_string(),
                Estimate {
                    percent: s.get("percent").and_then(percent),
                    left_secs: s.get("left").and_then(secs),
                    agents,
                },
            ))
        })
        .collect();
    Some(Estimates {
        until_ms: value.get("until").and_then(|u| u.as_u64()),
        sessions,
    })
}

/// LBTerm's first line on the hub's pipe, asking for its focus lines from then on
pub fn hello_message(pid: u32, version: &str, terminal: &str) -> String {
    serde_json::json!({
        "type": "lbterm.subscribe",
        "pid": pid,
        "version": version,
        "terminal": terminal,
    })
    .to_string()
}

/// Where a Claude session is in LBTerm, for the hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFact {
    pub session_id: String,
    /// The pane id `lbterm cli` takes: the mux server's
    pub pane: u64,
    /// The tab's place in its window, from 1, as the rail numbers them
    pub tab: usize,
    /// It's the window's open tab
    pub active: bool,
}

/// Where the Claude sessions are in every window of this GUI, as the hub was last told. Each
/// window paints on its own and the panes line is all of them, so one window's paint mustn't
/// drop another's (the Conductor's report of 2026-10-24: with two windows, the hub lost one
/// window's panes at a time). Every paint gives its window's, and what it gave is compared with
/// what that window gave last before anything is built: a paint with its sessions where they
/// were costs that comparison and nothing more. Until 2026-10-30 every paint of every window
/// copied every window's sessions and built and wrote out the whole line, to find it the same.
#[derive(Debug, Default)]
pub struct Panes {
    /// Each window's sessions as it last painted them, by its mux window id
    by_window: std::collections::BTreeMap<usize, Vec<PaneFact>>,
    /// The line last sent, for the next connection
    line: Option<String>,
    /// How many lines have been built, for the tests
    built: u64,
}

impl Panes {
    pub const fn new() -> Panes {
        Panes {
            by_window: std::collections::BTreeMap::new(),
            line: None,
            built: 0,
        }
    }

    /// `window` painted with its sessions where `facts` says, and the windows `alive` says are
    /// gone are dropped: the line for the hub when the whole has changed, nothing when it hasn't.
    /// In window order, each window's as it gave them.
    pub fn update(
        &mut self,
        terminal: &str,
        window: usize,
        facts: Vec<PaneFact>,
        alive: impl Fn(usize) -> bool,
    ) -> Option<String> {
        let before = self.by_window.len();
        self.by_window.retain(|&id, _| id == window || alive(id));
        let same = self.by_window.get(&window).map_or(false, |had| *had == facts);
        if same && self.by_window.len() == before {
            return None;
        }
        self.by_window.insert(window, facts);
        let line = panes_message(terminal, self.by_window.values().flatten());
        self.built += 1;
        if self.line.as_deref() == Some(line.as_str()) {
            return None;
        }
        self.line = Some(line.clone());
        Some(line)
    }

    /// The line last sent: a new connection is told it first
    pub fn line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    /// How many lines have been built
    pub fn built(&self) -> u64 {
        self.built
    }
}

/// Every Claude session's pane in this GUI, for the hub, as one line
pub fn panes_message<'a>(terminal: &str, panes: impl IntoIterator<Item = &'a PaneFact>) -> String {
    let panes: Vec<serde_json::Value> = panes
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "sessionId": p.session_id,
                "pane": p.pane,
                "tab": p.tab,
                "active": p.active,
            })
        })
        .collect();
    serde_json::json!({ "type": "lbterm.panes", "terminal": terminal, "panes": panes }).to_string()
}

/// The Conductor's menu, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub card: Rect,
    /// A row an item, in order
    pub rows: Vec<Rect>,
    /// Where a tick goes, and where the items' words start
    pub tick_x: f64,
    pub text_x: f64,
}

impl Menu {
    /// The item under a point
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.rows.iter().position(|r| r.contains(x, y))
    }
}

/// Lay out a menu of `count` items, the longest `columns` columns, from `at` (where it was
/// clicked) down and right, kept inside a window `window` wide and tall. Rows are `row_h` high
/// and a column `cell_w` wide; `scale` is the DPI over 96. The tab menu's and the prompt list's
/// measures, with a column for ticks.
pub fn menu(
    count: usize,
    columns: usize,
    at: (f64, f64),
    window: (f64, f64),
    (row_h, cell_w): (f64, f64),
    scale: f64,
) -> Option<Menu> {
    if count == 0 || row_h <= 0.0 {
        return None;
    }
    let pad = 6.0 * scale;
    let inset = 12.0 * scale;
    let tick = 2.0 * cell_w;
    let w = (2.0 * inset + tick + columns as f64 * cell_w).max(160.0 * scale).ceil();
    let h = 2.0 * pad + count as f64 * row_h;
    let margin = 4.0 * scale;
    let x = at.0.min(window.0 - w - margin).max(margin);
    let y = at.1.min(window.1 - h - margin).max(margin);
    let rows = (0..count)
        .map(|i| Rect {
            x: x + pad,
            y: y + pad + i as f64 * row_h,
            w: w - 2.0 * pad,
            h: row_h,
        })
        .collect();
    Some(Menu {
        card: Rect { x, y, w, h },
        rows,
        tick_x: x + inset,
        text_x: x + inset + tick,
    })
}

/// The longest a menu item waits for its answer, its arc turning: `on` takes up to 30 s
pub const PENDING_AT_MOST: Duration = Duration::from_secs(45);

/// Whether an item picked when the state file says `said` waits for its answer: one with
/// `wait = true`, unless it's ticked. A ticked item asks for what's so already (the mode the
/// Conductor is in): Conductor rewrites the file in the same words for it, but nothing changes,
/// so the menu doesn't hold for that.
pub fn waits(wait: bool, checked: &HashMap<String, String>, said: &Said) -> bool {
    wait && !ticked(checked, said)
}

/// Whether an item picked when the state file said `then` has its answer: the file says
/// something else now (`now`, nothing for no file), or it has waited [`PENDING_AT_MOST`]
pub fn answered(then: &Said, now: &Said, waited: Duration) -> bool {
    then != now || waited >= PENDING_AT_MOST
}

/// Where the arrow keys move the focus in a menu of `count` items, from `focus`
pub fn step(focus: Option<usize>, count: usize, down: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (focus, down) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(i), true) => (i + 1).min(count - 1),
        (Some(i), false) => i.saturating_sub(1),
    })
}

/// The Conductor's own Claude session while it runs: its state file's `sessionId`. Off, it has
/// none, even if the file still names the last one.
pub fn session(said: &Said) -> Option<&str> {
    (said.state() != "off").then(|| said.get("sessionId")).flatten()
}

/// Which of a window's tabs is the Conductor's (Lachlan, 2026-10-21: the pad is its tab, so it
/// takes no row of its own): the first holding its session. `tabs` is each tab's Claude sessions,
/// in the window's order.
pub fn tab_of<S: AsRef<str>>(session: &str, tabs: &[Vec<S>]) -> Option<usize> {
    tabs.iter()
        .position(|sessions| sessions.iter().any(|s| s.as_ref() == session))
}

/// The rail's rows: the window's tabs by index, in order, all but the Conductor's
pub fn rail(tabs: usize, conductor: Option<usize>) -> Vec<usize> {
    (0..tabs).filter(|&i| Some(i) != conductor).collect()
}

/// Where the Conductor's tab is kept: first in its window, so Alt+Left from the first tab on the
/// rail goes up into it, as the pad sits above that tab (Lachlan's pick, 2026-10-21). The index
/// to move it from, when it isn't there.
pub fn keep_first(conductor: Option<usize>) -> Option<usize> {
    conductor.filter(|&i| i != 0)
}

/// Whether closing a tab has a new tab take its place first: the pad counts as a tab, so closing
/// the last one while the pad is on the rail mustn't close LBTerm (Lachlan, 2026-10-25). Only the
/// last tab of the last window: a window of several still closes with its last tab, LBTerm
/// staying open in the others. `windows` and `tabs` count only what's still alive.
pub fn keeps_window(pad: bool, windows: usize, tabs: usize) -> bool {
    pad && windows == 1 && tabs == 1
}

/// What a left click on the pad does
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadClick {
    /// Switch to the Conductor's tab
    Open,
    /// Drop the menu, as a right click does: there's no tab to open while it's off, and starting
    /// it is the menu's to do (Lachlan's pick, 2026-10-21)
    Menu,
}

pub fn left_click(has_tab: bool) -> PadClick {
    if has_tab {
        PadClick::Open
    } else {
        PadClick::Menu
    }
}

/// What's round the Conductor's pad (lbterm/NEW-TAB.md, "Landing 4", 7a)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outline {
    /// Off: nothing
    None,
    /// Read only: a grey hairline
    Grey,
    /// Accept most or Accept all at rest: a still hairline in the mode's colour, its label's
    /// (yellow, red)
    Mode,
    /// The spectrum's colours, their glow, and with the compositor their hues turning
    Spectrum,
}

impl Outline {
    /// Its name in the state file (lbterm/STATE.md, the rail's `conductor`)
    pub fn name(self) -> &'static str {
        match self {
            Outline::None => "none",
            Outline::Grey => "grey",
            Outline::Mode => "mode",
            Outline::Spectrum => "spectrum",
        }
    }
}

/// The pad's outline: the spectrum while it only reads and the pointer is on it (Lachlan,
/// 2026-10-21), and in Accept most and Accept all while it acts on a session (`acting`, its hub's
/// word: [`Focus::acting`]) or the pointer is on it, a still hairline in the mode's colour
/// otherwise (his 7a of 2026-11-01: the spectrum turning all the time was the brightest thing on
/// the rail at rest); grey while it only reads, nothing while it's off. Read only ignores
/// `acting`, as it did: only what may act on its own lights by itself.
pub fn outline(level: Level, hovered: bool, acting: bool) -> Outline {
    match level {
        Level::Off => Outline::None,
        Level::ReadOnly if hovered => Outline::Spectrum,
        Level::ReadOnly => Outline::Grey,
        Level::AcceptMost | Level::AcceptAll if hovered || acting => Outline::Spectrum,
        Level::AcceptMost | Level::AcceptAll => Outline::Mode,
    }
}

/// The still hairline round the pad, drawn in the panel's body under whatever the compositor
/// shows over it: the grey of the open panel's ring, or the mode's colour, its label's
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hairline {
    Grey,
    Most,
    All,
}

/// The pad's still hairline for its `outline`: grey while it only reads; in Accept most and Accept
/// all the mode's colour whatever the outline, so the spectrum's ring fades in over it and back out
/// to it (his 7a: "the still outline under it throughout"); none while it's off, and none in read
/// only while the pointer's spectrum shows in its place, as before. Without the compositor the
/// spectrum is drawn still over the hairline's place and the hairline isn't drawn
/// (render/conductor.rs)
pub fn hairline(level: Level, outline: Outline) -> Option<Hairline> {
    match (level, outline) {
        (_, Outline::Grey) => Some(Hairline::Grey),
        (Level::AcceptMost, _) => Some(Hairline::Most),
        (Level::AcceptAll, _) => Some(Hairline::All),
        _ => None,
    }
}

/// Whether the spectrum's ring on the pad fades in and out (on `panel_glow`) rather than coming
/// and going at once: in Accept most and Accept all, where it lights over the still outline as
/// the Conductor acts (his 7a); read only's comes with the pointer and goes with it at once, as it
/// did
pub fn ring_fades(level: Level) -> bool {
    matches!(level, Level::AcceptMost | Level::AcceptAll)
}

/// The spectrum's ring on the Conductor's pad as it comes and goes, with the compositor: it's in
/// the panel's plan while the pad's outline is the spectrum and, once it isn't, until it has had
/// time to fade out, so the compositor fades it from wherever it is (it removes at once a ring
/// the plan stops naming, and the ring's templates are drawn only while one is named). One a
/// window, kept by its paint (render/tab_panel.rs, `tab_panel_rings`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadRing {
    lit: bool,
    /// When a ring let go of has faded out
    fades_until: Option<Instant>,
}

/// Where the pad's ring is at a paint
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingNow {
    /// Not in the plan
    Gone,
    /// In it, fading in or there
    Lit,
    /// In it, fading out
    Fading,
}

impl PadRing {
    /// Where the ring is at `now`, its outline the spectrum or not (`spectrum`), a fade out taking
    /// `fade` (to its settling; nothing with motion off): lit while the spectrum shows, fading for
    /// `fade` after, then gone. Lit again while it fades, it fades back in from where it is.
    pub fn at(&mut self, spectrum: bool, now: Instant, fade: Duration) -> RingNow {
        if spectrum {
            self.lit = true;
            self.fades_until = None;
            return RingNow::Lit;
        }
        if std::mem::take(&mut self.lit) {
            self.fades_until = Some(now + fade);
        }
        match self.fades_until {
            Some(end) if end > now => RingNow::Fading,
            _ => {
                self.fades_until = None;
                RingNow::Gone
            }
        }
    }

    /// When a ring fading out will have gone, for a paint then to take it out of the plan
    pub fn due(&self) -> Option<Instant> {
        self.fades_until
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_session_is_named_only_while_it_runs() {
        assert_eq!(session(&said(&[("state", "on"), ("sessionId", "4a27")])), Some("4a27"));
        assert_eq!(session(&said(&[("state", "remote"), ("sessionId", "4a27")])), Some("4a27"));
        assert_eq!(session(&said(&[("state", "off"), ("sessionId", "4a27")])), None);
        assert_eq!(session(&said(&[("state", "on")])), None);
        assert_eq!(session(&said(&[("state", "on"), ("sessionId", " ")])), None);
        assert_eq!(session(&Said::default()), None);
    }

    #[test]
    fn its_tab_is_the_first_holding_its_session() {
        let tabs = vec![vec!["a"], vec![], vec!["b", "c"], vec!["c"]];
        assert_eq!(tab_of("c", &tabs), Some(2));
        assert_eq!(tab_of("a", &tabs), Some(0));
        assert_eq!(tab_of("z", &tabs), None);
        assert_eq!(tab_of::<&str>("a", &[]), None);
    }

    #[test]
    fn the_rail_leaves_its_tab_out() {
        assert_eq!(rail(4, Some(0)), vec![1, 2, 3]);
        assert_eq!(rail(4, Some(2)), vec![0, 1, 3]);
        assert_eq!(rail(3, None), vec![0, 1, 2]);
        assert_eq!(rail(1, Some(0)), Vec::<usize>::new());
        assert_eq!(rail(0, None), Vec::<usize>::new());
    }

    #[test]
    fn its_tab_is_kept_first() {
        assert_eq!(keep_first(Some(6)), Some(6));
        assert_eq!(keep_first(Some(0)), None);
        assert_eq!(keep_first(None), None);
    }

    #[test]
    fn the_last_tab_gives_way_while_the_pad_stays() {
        assert!(keeps_window(true, 1, 1));
        // Another tab, or another window, is still there
        assert!(!keeps_window(true, 1, 2));
        assert!(!keeps_window(true, 2, 1));
        // No pad: the last tab closes LBTerm, as WezTerm does
        assert!(!keeps_window(false, 1, 1));
        assert!(!keeps_window(true, 1, 0));
    }

    #[test]
    fn a_left_click_opens_its_tab_or_else_the_menu() {
        assert_eq!(left_click(true), PadClick::Open);
        assert_eq!(left_click(false), PadClick::Menu);
    }

    #[test]
    fn the_spectrum_shows_while_it_acts_or_under_the_pointer_and_the_mode_is_still_at_rest() {
        // Off: nothing, whatever the hub or the pointer says
        for (hovered, acting) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(outline(Level::Off, hovered, acting), Outline::None);
        }
        // Read only: grey, the spectrum under the pointer; acting doesn't light it
        assert_eq!(outline(Level::ReadOnly, false, false), Outline::Grey);
        assert_eq!(outline(Level::ReadOnly, false, true), Outline::Grey);
        assert_eq!(outline(Level::ReadOnly, true, false), Outline::Spectrum);
        assert_eq!(outline(Level::ReadOnly, true, true), Outline::Spectrum);
        // Accept most and all (his 7a): still in the mode's colour at rest, the spectrum while it
        // acts or the pointer is on it
        for level in [Level::AcceptMost, Level::AcceptAll] {
            assert_eq!(outline(level, false, false), Outline::Mode);
            assert_eq!(outline(level, false, true), Outline::Spectrum);
            assert_eq!(outline(level, true, false), Outline::Spectrum);
            assert_eq!(outline(level, true, true), Outline::Spectrum);
        }
        let names: Vec<_> = [Outline::None, Outline::Grey, Outline::Mode, Outline::Spectrum]
            .iter()
            .map(|o| o.name())
            .collect();
        assert_eq!(names, ["none", "grey", "mode", "spectrum"]);
        // The still hairline: the mode's colour under the spectrum too, so it fades back to it;
        // read only's grey goes while the pointer's spectrum is in its place
        let line = |level, hovered, acting| hairline(level, outline(level, hovered, acting));
        assert_eq!(line(Level::Off, true, true), None);
        assert_eq!(line(Level::ReadOnly, false, true), Some(Hairline::Grey));
        assert_eq!(line(Level::ReadOnly, true, false), None);
        assert_eq!(line(Level::AcceptMost, false, false), Some(Hairline::Most));
        assert_eq!(line(Level::AcceptMost, false, true), Some(Hairline::Most));
        assert_eq!(line(Level::AcceptMost, true, false), Some(Hairline::Most));
        assert_eq!(line(Level::AcceptAll, false, false), Some(Hairline::All));
        assert_eq!(line(Level::AcceptAll, true, true), Some(Hairline::All));
        // Only where it lights by itself does the ring fade
        assert!(!ring_fades(Level::Off));
        assert!(!ring_fades(Level::ReadOnly));
        assert!(ring_fades(Level::AcceptMost));
        assert!(ring_fades(Level::AcceptAll));
    }

    #[test]
    fn the_pads_ring_stays_in_the_plan_until_it_has_faded_out() {
        let t0 = Instant::now();
        let fade = Duration::from_millis(600);
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut ring = PadRing::default();
        // At rest: never in the plan, nothing to wake for
        assert_eq!(ring.at(false, t0, fade), RingNow::Gone);
        assert_eq!(ring.due(), None);
        // It acts: lit, for as long as it does
        assert_eq!(ring.at(true, ms(100), fade), RingNow::Lit);
        assert_eq!(ring.at(true, ms(900), fade), RingNow::Lit);
        assert_eq!(ring.due(), None);
        // Let go at 1000: fading until 1600, a paint asked for then, and gone from it
        assert_eq!(ring.at(false, ms(1000), fade), RingNow::Fading);
        assert_eq!(ring.due(), Some(ms(1600)));
        assert_eq!(ring.at(false, ms(1300), fade), RingNow::Fading);
        assert_eq!(ring.due(), Some(ms(1600)), "the end is fixed when it lets go");
        assert_eq!(ring.at(false, ms(1600), fade), RingNow::Gone);
        assert_eq!(ring.due(), None);
        assert_eq!(ring.at(false, ms(5000), fade), RingNow::Gone);
        // Lit again while it fades: back in from where it is, then a fade of its own from the
        // second let-go
        assert_eq!(ring.at(true, ms(6000), fade), RingNow::Lit);
        assert_eq!(ring.at(false, ms(6100), fade), RingNow::Fading);
        assert_eq!(ring.at(true, ms(6200), fade), RingNow::Lit);
        assert_eq!(ring.due(), None);
        assert_eq!(ring.at(false, ms(6300), fade), RingNow::Fading);
        assert_eq!(ring.due(), Some(ms(6900)));
        // Motion off (or read only's, which comes and goes at once): gone in the next paint
        let mut at_once = PadRing::default();
        assert_eq!(at_once.at(true, t0, Duration::ZERO), RingNow::Lit);
        assert_eq!(at_once.at(false, ms(10), Duration::ZERO), RingNow::Gone);
        assert_eq!(at_once.due(), None);
    }

    fn said(fields: &[(&str, &str)]) -> Said {
        Said {
            fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            hidden: None,
        }
    }

    /// A state file whose `hidden` lists `ids`
    fn hiding(ids: &[&str]) -> Said {
        Said {
            hidden: Some(Hidden(ids.iter().map(|id| id.to_string()).collect())),
            ..said(&[("state", "off")])
        }
    }

    #[test]
    fn the_hidden_list_reads() {
        // The Conductor's button.json as it writes it since 2026-11-02, off with none hidden
        let off = r#"{
  "state": "off",
  "mode": null,
  "listening": null,
  "busy": false,
  "asks": 0,
  "sessionId": null,
  "label": "Conductor",
  "tooltip": "Conductor is off",
  "hidden": [],
  "updatedAt": 1794049289706
}"#;
        let s = read(off).unwrap();
        assert_eq!(s.hidden, Some(Hidden::default()), "empty: a Conductor that can hide, none hidden");
        assert!(s.hidden.as_ref().unwrap().is_empty());
        assert_eq!(s.state(), "off");
        assert_eq!(s.get("hidden"), None, "not a field of words");

        let on = read(r#"{"state":"on","mode":"most","hidden":["4a27","b9e1"," c0de "],"updatedAt":2}"#).unwrap();
        let hidden = on.hidden.as_ref().unwrap();
        assert_eq!(hidden.len(), 3);
        assert!(hidden.has("4a27") && hidden.has("b9e1"));
        assert!(hidden.has("c0de"), "an id's spaces trimmed");
        assert!(!hidden.has("d00d"), "a session not in it");
        assert!(!hidden.has(""));
        assert_eq!(read(&format!("\u{feff}{off}")).unwrap().hidden, Some(Hidden::default()), "after a BOM");

        // Absent: a Conductor from before 2026-11-02, or no list
        assert_eq!(read(r#"{"state":"on","mode":"read"}"#).unwrap().hidden, None);
        assert_eq!(read(r#"{"hidden":null}"#).unwrap().hidden, None);
        assert_eq!(read(r#"{"hidden":"4a27"}"#).unwrap().hidden, None, "a word is no list");
        assert_eq!(read(r#"{"hidden":{"4a27":true}}"#).unwrap().hidden, None);
        assert_eq!(read(r#"{"hidden":["4a27",7]}"#).unwrap().hidden, None, "something not an id in it: no list");
        assert_eq!(Said::default().hidden, None, "no file");
        // Blank ids name no session
        assert_eq!(read(r#"{"hidden":["", "  "]}"#).unwrap().hidden, Some(Hidden::default()));
        // The same ids in another order, or twice, say the same
        assert_eq!(
            read(r#"{"hidden":["b","a","a"]}"#).unwrap(),
            read(r#"{"hidden":["a","b"]}"#).unwrap()
        );
        // A hide changes what the file says even before its updatedAt does
        assert_ne!(read(r#"{"hidden":[]}"#).unwrap(), read(r#"{"hidden":["a"]}"#).unwrap());
    }

    #[test]
    fn a_tabs_sessions_are_hidden_none_some_or_all() {
        let h = hiding(&["a", "b"]).hidden.unwrap();
        assert_eq!(h.cover(&["c"]), Cover::NoneOf);
        assert_eq!(h.cover(&["a"]), Cover::AllOf);
        assert_eq!(h.cover(&["a", "b"]), Cover::AllOf, "every session of the tab");
        assert_eq!(h.cover(&["a", "c"]), Cover::SomeOf, "some of them");
        assert_eq!(h.cover(&["c", "b"]), Cover::SomeOf);
        assert_eq!(h.cover::<&str>(&[]), Cover::NoneOf, "a shell tab");
        assert_eq!(Hidden::default().cover(&["a"]), Cover::NoneOf);
    }

    #[test]
    fn hide_is_offered_only_on_a_claude_tab_with_the_list() {
        // No list: never, hidden or not (no half version)
        for s in [Said::default(), said(&[("state", "on"), ("mode", "all")])] {
            assert_eq!(offer(&s, &["a"]), None);
            assert_eq!(offer::<&str>(&s, &[]), None);
        }
        // An empty list: Hide on a Claude tab, nothing on a shell tab
        let none = hiding(&[]);
        assert_eq!(offer(&none, &["a"]), Some(Offer::Hide));
        assert_eq!(offer::<&str>(&none, &[]), None);
        // Show once every session of the tab is hidden; Hide while any isn't
        let some = hiding(&["a"]);
        assert_eq!(offer(&some, &["a"]), Some(Offer::Show));
        assert_eq!(offer(&some, &["b"]), Some(Offer::Hide));
        assert_eq!(offer(&some, &["a", "b"]), Some(Offer::Hide), "some hidden: Hide hides the rest");
        assert_eq!(offer(&hiding(&["a", "b", "z"]), &["b", "a"]), Some(Offer::Show));
    }

    #[test]
    fn a_hide_or_show_is_confirmed_by_the_list() {
        // A hide: once it lists every session of the tab
        assert!(confirmed(&hiding(&["a", "b"]), &["a", "b"], true));
        assert!(!confirmed(&hiding(&["a"]), &["a", "b"], true), "one still shown");
        assert!(!confirmed(&hiding(&[]), &["a"], true));
        // A show: once it lists none of them
        assert!(confirmed(&hiding(&["z"]), &["a", "b"], false));
        assert!(!confirmed(&hiding(&["b"]), &["a", "b"], false), "one still hidden");
        assert!(!confirmed(&hiding(&["a"]), &["a"], false));
        // No list (no file, or a Conductor gone back to one from before): nothing confirmed
        assert!(!confirmed(&Said::default(), &["a"], true));
        assert!(!confirmed(&Said::default(), &["a"], false));
    }

    fn when(fields: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        fields
            .iter()
            .map(|(k, vs)| (k.to_string(), vs.iter().map(|v| v.to_string()).collect()))
            .collect()
    }

    #[test]
    fn the_conductors_file_reads() {
        let text = r#"{
  "state": "remote",
  "mode": "most",
  "label": "Conductor",
  "tooltip": "Conductor is on (accept most), with remote control since 09:05",
  "updatedAt": 1792504916817
}"#;
        let s = read(text).unwrap();
        assert_eq!(s.state(), "remote");
        assert_eq!(s.get("mode"), Some("most"));
        assert_eq!(s.tooltip(), Some("Conductor is on (accept most), with remote control since 09:05"));
        assert_eq!(s.get("updatedAt"), Some("1792504916817"), "a number, as its text");
        assert_eq!(read(r#"{"remote":true}"#).unwrap().get("remote"), Some("true"));
        assert_eq!(read(r#"{"seen":[1],"by":{"a":1}}"#), Some(Said::default()), "lists and objects: left out");
        // As Windows PowerShell 5.1 writes it
        assert_eq!(read(&format!("\u{feff}{text}")), Some(s));
        // Off, its mode null
        assert_eq!(read(r#"{"state":"off","mode":null}"#).unwrap().get("mode"), None);
    }

    #[test]
    fn a_file_that_isnt_one_says_nothing() {
        assert_eq!(read(""), None);
        assert_eq!(read("{\"state\": \"on\""), None);
        assert_eq!(read("[]"), None);
        assert_eq!(read("\"on\""), None);
        assert_eq!(read("{}"), Some(Said::default()));
        assert_eq!(Said::default().state(), "off", "no state: off");
    }

    #[test]
    fn the_pad_follows_the_state_and_mode() {
        let off = pad(&Said::default());
        assert_eq!((off.level, off.label.as_str(), off.remote), (Level::Off, "off", false));
        let read_only = pad(&said(&[("state", "on"), ("mode", "read")]));
        assert_eq!((read_only.level, read_only.label.as_str()), (Level::ReadOnly, "read"));
        let most = pad(&said(&[("state", "remote"), ("mode", "most")]));
        assert_eq!((most.level, most.label.as_str(), most.remote), (Level::AcceptMost, "most", true));
        assert_eq!(pad(&said(&[("state", "on"), ("mode", "all")])).level, Level::AcceptAll);
        // On without a mode is read only; a mode LBTerm doesn't know shows its name, read only
        assert_eq!(pad(&said(&[("state", "on")])).level, Level::ReadOnly);
        let new = pad(&said(&[("state", "on"), ("mode", "plan")]));
        assert_eq!((new.level, new.label.as_str()), (Level::ReadOnly, "plan"));
    }

    #[test]
    fn items_are_offered_by_state_and_ticked_by_mode() {
        let on_most = said(&[("state", "on"), ("mode", "most")]);
        assert!(offered(&when(&[("state", &["on", "remote"])]), &on_most));
        assert!(!offered(&when(&[("state", &["off"])]), &on_most));
        assert!(offered(&HashMap::new(), &on_most), "no conditions: always");
        // No file: off, so the items for turning it on show
        assert!(offered(&when(&[("state", &["off"])]), &Said::default()));
        // A field the file hasn't got holds ""
        assert!(!offered(&when(&[("mode", &["read"])]), &Said::default()));
        let most: HashMap<String, String> = [("mode".to_string(), "most".to_string())].into();
        let all: HashMap<String, String> = [("mode".to_string(), "all".to_string())].into();
        assert!(ticked(&most, &on_most) && !ticked(&all, &on_most));
        assert!(!ticked(&HashMap::new(), &on_most), "nothing named: never ticked");
    }

    fn named(sid: &str, doing: Doing, until_ms: Option<u64>) -> Named {
        Named {
            session_id: sid.to_string(),
            doing,
            until_ms,
        }
    }

    #[test]
    fn the_hubs_focus_line_reads() {
        let line = r#"{"type":"conductor.focus","sessions":[
            {"sessionId":"a1","doing":"thinking"},{"sessionId":"b2","doing":"acting","what":"answering","until":1792570000000},
            {"sessionId":"c3","doing":"dreaming"},{"sessionId":"","doing":"acting"}]}"#;
        assert_eq!(
            focus_message(&line.replace('\n', " ")),
            Some(vec![
                named("a1", Doing::Thinking, None),
                named("b2", Doing::Acting, Some(1_792_570_000_000)),
            ])
        );
        assert_eq!(focus_message(r#"{"type":"something.else"}"#), None);
        assert_eq!(focus_message("not json"), None);
        assert_eq!(focus_message(r#"{"type":"conductor.focus","sessions":[]}"#), Some(vec![]));
        assert!(Doing::Acting > Doing::Thinking);
    }

    #[test]
    fn an_outline_shows_a_lap_at_least_and_lingers() {
        let t0 = Instant::now();
        let ms = 1_000_000;
        let mut focus = Focus::default();
        assert!(focus.set(&[named("a", Doing::Acting, None)], t0, ms));
        assert_eq!(focus.get("a", t0), Some(Doing::Acting));
        assert_eq!(focus.next_change(), None, "named, with no until: it shows till the hub lets go");
        // Named again: no change
        assert!(!focus.set(&[named("a", Doing::Acting, None)], t0 + Duration::from_millis(100), ms + 100));
        // Gone from the list at once: it still shows its lap
        focus.set(&[], t0 + Duration::from_millis(200), ms + 200);
        assert_eq!(focus.get("a", t0 + Duration::from_millis(1300)), Some(Doing::Acting));
        assert_eq!(focus.next_change(), Some(t0 + ACTING_AT_LEAST));
        assert_eq!(focus.get("a", t0 + ACTING_AT_LEAST), None);
        assert!(focus.prune(t0 + ACTING_AT_LEAST));
        // A long one lingers a moment after the hub lets go
        let t1 = t0 + Duration::from_secs(10);
        focus.set(&[named("b", Doing::Thinking, None)], t1, ms);
        focus.set(&[], t1 + Duration::from_secs(5), ms);
        assert_eq!(focus.get("b", t1 + Duration::from_secs(5) + LINGER / 2), Some(Doing::Thinking));
        assert_eq!(focus.get("b", t1 + Duration::from_secs(5) + LINGER), None);
        // Thinking to acting is a change, and starts its own lap
        let t2 = t1 + Duration::from_secs(20);
        focus.set(&[named("c", Doing::Thinking, None)], t2, ms);
        assert!(focus.set(&[named("c", Doing::Acting, None)], t2 + Duration::from_millis(50), ms));
        assert!(focus.clear());
        assert_eq!(focus.get("c", t2), None);
    }

    #[test]
    fn it_acts_while_any_session_it_acts_on_shows_and_thinking_isnt_acting() {
        let t0 = Instant::now();
        let ms = 2_000_000;
        let mut focus = Focus::default();
        assert!(!focus.acting(t0), "nothing named");
        focus.set(&[named("a", Doing::Thinking, None)], t0, ms);
        assert!(!focus.acting(t0), "thinking about one isn't acting");
        // Acting on a second: from that moment, and changed, so the window repaints
        assert!(focus.set(&[named("a", Doing::Thinking, None), named("b", Doing::Acting, None)], t0, ms));
        assert!(focus.acting(t0 + Duration::from_millis(10)));
        // Let go at once: it still acts for its least lap, and next_change says when that ends
        focus.set(&[named("a", Doing::Thinking, None)], t0 + Duration::from_millis(100), ms + 100);
        assert!(focus.acting(t0 + ACTING_AT_LEAST - Duration::from_millis(1)));
        assert!(!focus.acting(t0 + ACTING_AT_LEAST));
        assert_eq!(focus.next_change(), Some(t0 + ACTING_AT_LEAST));
        // An until past ends it while still named
        let t1 = t0 + Duration::from_secs(30);
        focus.set(&[named("c", Doing::Acting, Some(ms + 2000))], t1, ms);
        assert!(focus.acting(t1 + Duration::from_millis(1999)));
        assert!(!focus.acting(t1 + Duration::from_secs(2)));
        // The hub gone: nothing
        focus.set(&[named("d", Doing::Acting, None)], t1, ms);
        assert!(focus.clear());
        assert!(!focus.acting(t1));
    }

    #[test]
    fn an_outline_ends_at_its_until_even_while_named() {
        let t0 = Instant::now();
        let ms = 5_000_000;
        let mut focus = Focus::default();
        // A message's: seen to start, never to end, so it says when it's surely over
        focus.set(&[named("m", Doing::Acting, Some(ms + 3000))], t0, ms);
        assert_eq!(focus.next_change(), Some(t0 + Duration::from_secs(3)));
        assert_eq!(focus.get("m", t0 + Duration::from_millis(2900)), Some(Doing::Acting));
        assert_eq!(focus.get("m", t0 + Duration::from_secs(3)), None, "still named, but past its until");
        // Named again with a later one: it carries on
        focus.set(&[named("m", Doing::Acting, Some(ms + 6000))], t0 + Duration::from_secs(1), ms + 1000);
        assert_eq!(focus.get("m", t0 + Duration::from_secs(4)), Some(Doing::Acting));
        // An until already past still shows a lap
        let t1 = t0 + Duration::from_secs(60);
        focus.set(&[named("p", Doing::Acting, Some(ms))], t1, ms + 60_000);
        assert_eq!(focus.get("p", t1 + Duration::from_millis(1000)), Some(Doing::Acting));
        assert_eq!(focus.get("p", t1 + ACTING_AT_LEAST), None);
        // Let go before its until: the linger, not the until, ends it
        let t2 = t0 + Duration::from_secs(120);
        focus.set(&[named("q", Doing::Thinking, Some(ms + 600_000))], t2, ms);
        focus.set(&[], t2 + Duration::from_secs(2), ms);
        assert_eq!(focus.get("q", t2 + Duration::from_secs(2) + LINGER), None);
    }

    #[test]
    fn the_hubs_estimates_line_reads() {
        let line = r#"{"type":"conductor.estimates","until":1792570000000,"sessions":[
            {"sessionId":"a1","percent":40,"left":180,"agents":[{"id":"a93f","kind":"agent","what":"Survey the dashboards","percent":50,"left":120},{"id":"x","what":"no percent"}]},
            {"sessionId":"b2","agents":[{"id":"k","kind":"shell","what":"Long job","percent":130,"left":600}]},
            {"sessionId":"","percent":10},{"percent":20}]}"#;
        let e = estimates_message(&line.replace('\n', " ")).unwrap();
        assert_eq!(e.until_ms, Some(1_792_570_000_000));
        assert_eq!(e.sessions.len(), 2, "no id: passed over");
        assert_eq!(
            e.sessions["a1"],
            Estimate {
                percent: Some(40),
                left_secs: Some(180),
                agents: vec![AgentEstimate {
                    id: "a93f".into(),
                    what: "Survey the dashboards".into(),
                    percent: 50,
                    left_secs: 120,
                }],
            }
        );
        // Handed back, a subagent still at it; a percent past 100 is 100
        let b2 = &e.sessions["b2"];
        assert_eq!((b2.percent, b2.left_secs), (None, None));
        assert_eq!(b2.agents[0].percent, 100);
        assert_eq!(estimates_message(r#"{"type":"conductor.focus","sessions":[]}"#), None);
        assert_eq!(estimates_message("{"), None);
        assert_eq!(
            estimates_message(r#"{"type":"conductor.estimates","sessions":[]}"#),
            Some(Estimates::default())
        );
        // And a focus line isn't read as one
        assert_eq!(focus_message(&line.replace('\n', " ")), None);
    }

    #[test]
    fn lbterms_lines_for_the_hub() {
        let hello: serde_json::Value = serde_json::from_str(&hello_message(42, "20261021", "lbterm")).unwrap();
        assert_eq!(hello["type"], "lbterm.subscribe");
        assert_eq!(hello["pid"], 42);
        let line = panes_message(
            "lbterm",
            &[PaneFact {
                session_id: "a1".into(),
                pane: 17,
                tab: 3,
                active: false,
            }],
        );
        assert!(!line.contains('\n'), "one line");
        let panes: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(panes["type"], "lbterm.panes");
        assert_eq!(panes["panes"][0]["sessionId"], "a1");
        assert_eq!(panes["panes"][0]["pane"], 17);
        assert_eq!(panes["panes"][0]["tab"], 3);
    }

    #[test]
    fn the_panes_line_holds_every_windows_sessions() {
        let fact = |sid: &str, pane: u64| PaneFact { session_id: sid.into(), pane, tab: 1, active: true };
        let mut panes = Panes::new();
        let sids = |line: Option<String>| -> Vec<String> {
            let v: serde_json::Value = serde_json::from_str(&line.expect("a line")).unwrap();
            v["panes"].as_array().unwrap().iter().map(|p| p["sessionId"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(sids(panes.update("lbterm", 3, vec![fact("a", 1)], |_| true)), ["a"]);
        // The second window paints: the first's stay
        assert_eq!(sids(panes.update("lbterm", 7, vec![fact("b", 2), fact("c", 4)], |_| true)), ["a", "b", "c"]);
        // The first paints again with what changed in it
        assert_eq!(sids(panes.update("lbterm", 3, vec![fact("d", 5)], |_| true)), ["d", "b", "c"]);
        // The second window closes: its sessions go at the next paint, the first's unchanged
        assert_eq!(sids(panes.update("lbterm", 3, vec![fact("d", 5)], |id| id != 7)), ["d"]);
        assert_eq!(panes.by_window.len(), 1);
        assert!(panes.line().unwrap().contains(r#""d""#), "kept for the next connection");
    }

    #[test]
    fn a_paint_builds_no_line_unless_the_panes_changed() {
        let fact = |sid: &str, tab: usize, active: bool| PaneFact { session_id: sid.into(), pane: 9, tab, active };
        let mut panes = Panes::new();
        assert!(panes.update("lbterm", 1, vec![fact("a", 1, true), fact("b", 2, false)], |_| true).is_some());
        assert_eq!(panes.built(), 1);
        // A thousand frames with the sessions where they were: nothing built, nothing sent
        for _ in 0..1000 {
            assert_eq!(panes.update("lbterm", 1, vec![fact("a", 1, true), fact("b", 2, false)], |_| true), None);
        }
        assert_eq!(panes.built(), 1, "not one line built for an unchanged paint");
        // Another window, holding nothing of Claude's: built once, to find the line the same
        assert!(panes.update("lbterm", 2, vec![], |_| true).is_none());
        assert_eq!(panes.built(), 2);
        assert_eq!(panes.update("lbterm", 2, vec![], |_| true), None);
        assert_eq!(panes.built(), 2);
        // The open tab changing is a new line
        let line = panes
            .update("lbterm", 1, vec![fact("a", 1, false), fact("b", 2, true)], |_| true)
            .expect("the open tab changed");
        assert!(line.contains(r#""active":true"#));
        assert_eq!(panes.built(), 3);
        // A window going is looked at again, though the painting window's sessions didn't move
        assert!(panes
            .update("lbterm", 1, vec![fact("a", 1, false), fact("b", 2, true)], |id| id != 2)
            .is_none(), "the gone window held none");
        assert_eq!(panes.built(), 4);
    }

    #[test]
    fn the_menu_drops_from_the_click_and_stays_in_the_window() {
        let m = menu(3, 23, (40.0, 60.0), (1200.0, 800.0), (20.0, 8.0), 1.0).unwrap();
        assert_eq!((m.card.x, m.card.y), (40.0, 60.0));
        assert_eq!(m.card.w, 2.0 * 12.0 + 16.0 + 23.0 * 8.0);
        assert_eq!(m.card.h, 6.0 * 2.0 + 3.0 * 20.0);
        assert_eq!(m.tick_x, 52.0);
        assert_eq!(m.text_x, 52.0 + 16.0);
        assert_eq!(m.hit(m.text_x + 1.0, 60.0 + 6.0 + 25.0), Some(1));
        assert_eq!(m.hit(m.text_x + 1.0, 62.0), None, "the card's padding is no item");
        // Near the bottom right it moves in
        let m = menu(3, 23, (1150.0, 790.0), (1200.0, 800.0), (20.0, 8.0), 1.0).unwrap();
        assert!(m.card.x + m.card.w <= 1196.0 && m.card.y + m.card.h <= 796.0);
        assert_eq!(menu(0, 10, (0.0, 0.0), (1200.0, 800.0), (20.0, 8.0), 1.0), None);
    }

    #[test]
    fn a_waiting_item_has_its_answer_when_the_state_changes_or_it_gives_up() {
        let off = said(&[("state", "off"), ("tooltip", "Conductor is off")]);
        let on = said(&[("state", "on"), ("mode", "read"), ("tooltip", "Conductor is on")]);
        assert!(!answered(&off, &off, Duration::from_secs(5)), "nothing yet");
        assert!(answered(&off, &on, Duration::from_millis(300)));
        // A changed tooltip alone is an answer too: the file was written for it
        let later = said(&[("state", "off"), ("tooltip", "Conductor is off, since 21:00")]);
        assert!(answered(&off, &later, Duration::from_secs(1)));
        // The ticked mode picked again: Conductor writes the same words, only `updatedAt` new
        let before = read(r#"{"state":"on","mode":"read","tooltip":"Conductor is on (read only)","updatedAt":1}"#).unwrap();
        let again = read(r#"{"state":"on","mode":"read","tooltip":"Conductor is on (read only)","updatedAt":2}"#).unwrap();
        assert!(answered(&before, &again, Duration::from_millis(300)));
        // The file going is an answer; and nothing at all ends it after the most it waits
        assert!(answered(&off, &Said::default(), Duration::from_secs(1)));
        assert!(answered(&off, &off, PENDING_AT_MOST));
    }

    #[test]
    fn a_ticked_item_waits_for_nothing() {
        let read = said(&[("state", "on"), ("mode", "read")]);
        let mode = |m: &str| HashMap::from([("mode".to_string(), m.to_string())]);
        assert!(waits(true, &mode("most"), &read));
        assert!(!waits(true, &mode("read"), &read), "already read only: Conductor writes nothing");
        assert!(waits(true, &HashMap::new(), &read), "nothing to tick: it waits");
        assert!(!waits(false, &mode("most"), &read));
    }

    #[test]
    fn the_arrow_keys_walk_the_menu() {
        assert_eq!(step(None, 3, true), Some(0));
        assert_eq!(step(None, 3, false), Some(2));
        assert_eq!(step(Some(2), 3, true), Some(2));
        assert_eq!(step(Some(0), 3, false), Some(0));
        assert_eq!(step(None, 0, true), None);
    }
}
