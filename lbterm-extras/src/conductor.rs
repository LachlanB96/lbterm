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
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What a state file says: its fields, by name, numbers and true or false as their JSON text
/// (Conductor's `updatedAt`, which tells a rewrite of the same words from no write at all).
/// A null, a list or an object is left out, as if the field weren't there.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Said(pub HashMap<String, String>);

impl Said {
    /// A field's value, if it has one that isn't blank
    pub fn get(&self, field: &str) -> Option<&str> {
        self.0.get(field).map(|s| s.trim()).filter(|s| !s.is_empty())
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
/// (Windows PowerShell 5.1's) is passed over.
pub fn read(text: &str) -> Option<Said> {
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let fields = value
        .as_object()?
        .iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k.clone(), s.clone())),
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => Some((k.clone(), v.to_string())),
            _ => None,
        })
        .collect();
    Some(Said(fields))
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
    /// On, accepting most or all: the spectrum outline, moving
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

/// Every Claude session's pane in this GUI, for the hub, as one line
pub fn panes_message(terminal: &str, panes: &[PaneFact]) -> String {
    let panes: Vec<serde_json::Value> = panes
        .iter()
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
/// Conductor is in), and Conductor writes nothing for that, so there'd be nothing to wait on.
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

/// Whether the pad's outline is the spectrum: while it may act on its own, and while it only
/// reads and the pointer is on it (Lachlan, 2026-10-21); otherwise grey, or none while off
pub fn spectrum(level: Level, hovered: bool) -> bool {
    match level {
        Level::Off => false,
        Level::ReadOnly => hovered,
        Level::AcceptMost | Level::AcceptAll => true,
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
    fn a_left_click_opens_its_tab_or_else_the_menu() {
        assert_eq!(left_click(true), PadClick::Open);
        assert_eq!(left_click(false), PadClick::Menu);
    }

    #[test]
    fn the_spectrum_shows_acting_and_read_only_under_the_pointer() {
        assert!(!spectrum(Level::Off, false));
        assert!(!spectrum(Level::Off, true));
        assert!(!spectrum(Level::ReadOnly, false));
        assert!(spectrum(Level::ReadOnly, true));
        for level in [Level::AcceptMost, Level::AcceptAll] {
            assert!(spectrum(level, false));
            assert!(spectrum(level, true));
        }
    }

    fn said(fields: &[(&str, &str)]) -> Said {
        Said(fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
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
