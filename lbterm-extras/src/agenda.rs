//! The agenda button on the window strip (`config.lbterm_agenda`, lbterm/AGENDA.md): what's to
//! do, coming up and failing, from a file of rows another program keeps up to date, with the
//! counts that program gives for what's urgent and what's failing:
//!
//! ```json
//! { "changed": 1793499785871,
//!   "counts": { "urgent": 15, "failing": 53 },
//!   "rows": [ { "source": "Index", "id": "fail:docs:git", "kind": "failing", "who": "agent",
//!               "level": "normal", "title": "Git: no repo", "detail": "Not a git repo…",
//!               "project": "docs", "whenText": "waiting 3 d", "open": "/d/projects",
//!               "group": "Unresolved incidents",
//!               "actions": [ { "label": "Recheck", "confirm": "Recheck it now?" } ] } ] }
//! ```
//!
//! `kind` is `todo`, `upcoming` or `failing`; `who` is `you` or `agent`; `level` is `urgent`,
//! `soon`, `normal` or `low`. The rows come in the order the program lists them, and the button
//! keeps it within each section. Its counts are the program's own, never counted here: the file
//! is the one place they come from.
//!
//! [`lines`] makes the list the button drops: what's urgent first, then what's failing, what's
//! coming up and the to-dos that are yours, each section under its heading, rows of one group (and
//! project) folded into one line until it's opened, and the agents' to-dos behind a filter at the
//! end. An opened row shows its detail and its buttons under it ([`presses`]): its own actions,
//! Claude on it, snoozing it and opening its page. A press is handed to Lua
//! (`lbterm-agenda-press`), which runs the program's own commands.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// What a row is
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Todo,
    Upcoming,
    Failing,
}

/// Whose it is: Lachlan's own, or work an agent can do
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Who {
    You,
    Agent,
}

/// How soon it matters, most first
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Urgent,
    Soon,
    Normal,
    Low,
}

/// One of a row's own buttons: its word, and what's asked before it runs, if anything
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub label: String,
    pub confirm: Option<String>,
}

/// One row
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Who gave it: with its id, what the program's commands take to name it
    pub source: String,
    pub id: String,
    pub kind: Kind,
    pub who: Who,
    pub level: Level,
    pub title: String,
    /// A second line, shown when the row is opened: empty without one
    pub detail: String,
    /// The project it's about, for its label: the source when it doesn't say
    pub project: String,
    /// When it's due, happens or has waited since, in the program's words: empty without one
    pub when: String,
    /// Where its page is (a dashboard path, a link)
    pub open: Option<String>,
    /// Rows of one group (and project) fold into one line
    pub group: Option<String>,
    pub buttons: Vec<Button>,
}

impl Row {
    /// What names it to the program: its source and id
    pub fn key(&self) -> RowKey {
        (self.source.clone(), self.id.clone())
    }
}

/// A row's source and id
pub type RowKey = (String, String);

/// What the file says
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Agenda {
    /// When its content last changed, as the program says (ms since 1970)
    pub changed: i64,
    pub urgent: usize,
    pub failing: usize,
    pub rows: Vec<Row>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn row(v: &Value) -> Option<Row> {
    let kind = match v.get("kind")?.as_str()? {
        "todo" => Kind::Todo,
        "upcoming" => Kind::Upcoming,
        "failing" => Kind::Failing,
        _ => return None,
    };
    let who = match v.get("who")?.as_str()? {
        "you" => Who::You,
        "agent" => Who::Agent,
        _ => return None,
    };
    let level = match v.get("level")?.as_str()? {
        "urgent" => Level::Urgent,
        "soon" => Level::Soon,
        "normal" => Level::Normal,
        "low" => Level::Low,
        _ => return None,
    };
    let source = text(v, "source")?;
    let buttons = v
        .get("actions")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    Some(Button {
                        label: text(b, "label").filter(|l| !l.is_empty())?,
                        confirm: text(b, "confirm").filter(|c| !c.is_empty()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Row {
        id: text(v, "id").filter(|id| !id.is_empty())?,
        kind,
        who,
        level,
        title: crate::prompts::one_line(&text(v, "title")?),
        detail: text(v, "detail").map(|d| crate::prompts::one_line(&d)).unwrap_or_default(),
        project: text(v, "project").filter(|p| !p.is_empty()).unwrap_or_else(|| source.clone()),
        source,
        when: text(v, "whenText").unwrap_or_default(),
        open: text(v, "open").filter(|o| !o.is_empty()),
        group: text(v, "group").filter(|g| !g.is_empty()),
        buttons,
    })
}

/// The file's text read: nothing when it isn't an agenda (no counts or no rows). A row that
/// doesn't read (a kind, whose or level it doesn't know, no id or title) is left out; the rest
/// are kept in their order.
pub fn read(text: &str) -> Option<Agenda> {
    let v: Value = serde_json::from_str(text).ok()?;
    let counts = v.get("counts")?;
    let count = |key: &str| counts.get(key).and_then(Value::as_u64).map(|n| n as usize);
    Some(Agenda {
        changed: v.get("changed").and_then(Value::as_i64).unwrap_or(0),
        urgent: count("urgent")?,
        failing: count("failing")?,
        rows: v.get("rows")?.as_array()?.iter().filter_map(row).collect(),
    })
}

/// The list's sections, in the order it shows them
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Urgent,
    Failing,
    Coming,
    Yours,
    /// Behind the filter
    Agents,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Urgent => "Urgent",
            Section::Failing => "Failing",
            Section::Coming => "Coming up",
            Section::Yours => "Yours to do",
            Section::Agents => "Agents' to-dos",
        }
    }
}

/// The section a row is in: anything urgent first, whatever it is; then by what it is, and a
/// to-do by whose it is
pub fn section(row: &Row) -> Section {
    if row.level == Level::Urgent {
        return Section::Urgent;
    }
    match (row.kind, row.who) {
        (Kind::Failing, _) => Section::Failing,
        (Kind::Upcoming, _) => Section::Coming,
        (Kind::Todo, Who::You) => Section::Yours,
        (Kind::Todo, Who::Agent) => Section::Agents,
    }
}

/// The group a row folds into in `section`, by its key, and what the group's line says: its
/// group and the project it's about, or for a low row with none, "Whenever". Urgent rows never
/// fold.
fn group_of(section: Section, row: &Row) -> Option<(String, String)> {
    if section == Section::Urgent {
        return None;
    }
    match &row.group {
        Some(group) => {
            let title = if group.contains(&row.project) {
                group.clone()
            } else {
                format!("{group} \u{b7} {}", row.project)
            };
            Some((format!("{section:?}\u{1f}{group}\u{1f}{}", row.project), title))
        }
        None if row.level == Level::Low => Some((format!("{section:?}\u{1f}\u{1f}low"), WHENEVER.to_string())),
        None => None,
    }
}

/// A group of low rows that have none of their own
pub const WHENEVER: &str = "Whenever";

/// What the person looking has opened: the agents' to-dos shown, the groups opened by their keys,
/// the one row opened by its key
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub agents: bool,
    pub groups: HashSet<String>,
    pub row: Option<RowKey>,
}

/// A line of the list
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A section's title and its number: the file's own for Urgent and Failing (the button's),
    /// how many rows are in it for the rest
    Heading { section: Section, count: usize },
    /// Rows folded together: the group's key, its words, how many, whether it's open, and the most
    /// pressing level among them
    Group { key: String, title: String, count: usize, open: bool, level: Level },
    /// A row, by its index; `inner` inside an opened group
    Row { row: usize, inner: bool },
    /// The opened row's detail
    Detail { row: usize },
    /// The opened row's buttons
    Buttons { row: usize },
    /// The agents' to-dos: shown or not, and how many
    Filter { on: bool, count: usize },
    /// Nothing on the list
    Empty,
}

impl Line {
    /// Whether a click or the arrow keys land on it
    pub fn takes_focus(&self) -> bool {
        matches!(self, Line::Group { .. } | Line::Row { .. } | Line::Filter { .. })
    }
}

/// The list's lines for `agenda` as `view` has it opened
pub fn lines(agenda: &Agenda, view: &View) -> Vec<Line> {
    let mut by: HashMap<Section, Vec<usize>> = HashMap::new();
    for (i, row) in agenda.rows.iter().enumerate() {
        by.entry(section(row)).or_default().push(i);
    }
    let mut out = vec![];
    for s in [Section::Urgent, Section::Failing, Section::Coming, Section::Yours] {
        push_section(agenda, view, s, by.get(&s).map_or(&[][..], Vec::as_slice), true, &mut out);
    }
    let agents = by.get(&Section::Agents).map_or(&[][..], Vec::as_slice);
    if !agents.is_empty() {
        out.push(Line::Filter {
            on: view.agents,
            count: agents.len(),
        });
        // The filter's line is their heading
        if view.agents {
            push_section(agenda, view, Section::Agents, agents, false, &mut out);
        }
    }
    if out.is_empty() {
        out.push(Line::Empty);
    }
    out
}

fn push_section(agenda: &Agenda, view: &View, section: Section, rows: &[usize], heading: bool, out: &mut Vec<Line>) {
    if rows.is_empty() {
        return;
    }
    if heading {
        // Urgent's and Failing's are the file's numbers, the button's, never counted here: its
        // program's failing number takes in the urgent failing rows listed under Urgent and leaves
        // out the low ones, so the section's rows needn't add up to it
        let count = match section {
            Section::Urgent => agenda.urgent,
            Section::Failing => agenda.failing,
            _ => rows.len(),
        };
        out.push(Line::Heading { section, count });
    }
    // Each group's rows, and where it first comes
    let mut members: HashMap<String, (String, Vec<usize>)> = HashMap::new();
    for &i in rows {
        if let Some((key, title)) = group_of(section, &agenda.rows[i]) {
            members.entry(key).or_insert_with(|| (title, vec![])).1.push(i);
        }
    }
    let push_row = |out: &mut Vec<Line>, i: usize, inner: bool| {
        out.push(Line::Row { row: i, inner });
        let row = &agenda.rows[i];
        if view.row.as_ref() == Some(&row.key()) {
            if !row.detail.is_empty() {
                out.push(Line::Detail { row: i });
            }
            out.push(Line::Buttons { row: i });
        }
    };
    let mut done: HashSet<String> = HashSet::new();
    for &i in rows {
        let Some((key, _)) = group_of(section, &agenda.rows[i]) else {
            push_row(out, i, false);
            continue;
        };
        if !done.insert(key.clone()) {
            continue;
        }
        let (title, group) = &members[&key];
        // A group of one is its row
        if group.len() == 1 {
            push_row(out, group[0], false);
            continue;
        }
        let open = view.groups.contains(&key);
        out.push(Line::Group {
            key: key.clone(),
            title: title.clone(),
            count: group.len(),
            open,
            level: group.iter().map(|&j| agenda.rows[j].level).min().unwrap_or(Level::Normal),
        });
        if open {
            for &j in group {
                push_row(out, j, true);
            }
        }
    }
}

/// What a button under an opened row does
#[derive(Debug, Clone, PartialEq)]
pub enum Press {
    /// One of the row's own, by its index
    Action(usize),
    /// A Claude session on it
    Claude,
    /// Hidden for so many days
    Snooze(u32),
    /// Its page
    Open,
}

impl Press {
    /// What the Lua event calls it
    pub fn what(&self) -> &'static str {
        match self {
            Press::Action(_) => "do",
            Press::Claude => "claude",
            Press::Snooze(_) => "snooze",
            Press::Open => "open",
        }
    }
}

/// An opened row's buttons, in order, with their words: its own, then Claude, a day's and a
/// week's snooze, and its page when it has one
pub fn presses(row: &Row) -> Vec<(Press, String)> {
    let mut out: Vec<(Press, String)> = row
        .buttons
        .iter()
        .enumerate()
        .map(|(i, b)| (Press::Action(i), b.label.clone()))
        .collect();
    out.push((Press::Claude, "Claude".to_string()));
    out.push((Press::Snooze(1), "Snooze 1 d".to_string()));
    out.push((Press::Snooze(7), "7 d".to_string()));
    if row.open.is_some() {
        out.push((Press::Open, "Open".to_string()));
    }
    out
}

/// What a press asks first, when it's one of the row's own that asks
pub fn confirm<'a>(row: &'a Row, press: &Press) -> Option<&'a str> {
    match press {
        Press::Action(i) => row.buttons.get(*i)?.confirm.as_deref(),
        _ => None,
    }
}

/// What's urgent: the rail's waiting orange, since it needs you (lbterm/DESIGN.md, "Colour")
pub const URGENT: u32 = crate::inbox::COUNT;
/// What's failing: the panel's red for failed
pub const FAILING: u32 = 0xFF5449;

/// The colour a row's mark is drawn in, when it has one: urgent orange, failing red
pub fn mark(row: &Row) -> Option<u32> {
    if row.level == Level::Urgent {
        Some(URGENT)
    } else if row.kind == Kind::Failing {
        Some(FAILING)
    } else {
        None
    }
}

/// The button's counts, as it shows them: the urgent first, then the failing, each in its colour
/// and only when it isn't nought
pub fn badges(urgent: usize, failing: usize) -> Vec<(String, u32)> {
    let mut out = vec![];
    if urgent > 0 {
        out.push((urgent.to_string(), URGENT));
    }
    if failing > 0 {
        out.push((failing.to_string(), FAILING));
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    fn file(rows: Vec<Value>) -> String {
        json!({ "changed": 5, "counts": { "urgent": 2, "failing": 3 }, "rows": rows }).to_string()
    }

    fn r(id: &str, kind: &str, who: &str, level: &str) -> Value {
        json!({ "source": "Index", "id": id, "kind": kind, "who": who, "level": level, "title": format!("Title {id}") })
    }

    fn with(mut v: Value, key: &str, value: Value) -> Value {
        v[key] = value;
        v
    }

    #[test]
    fn the_file_is_read_and_a_row_it_doesnt_know_left_out() {
        let text = file(vec![
            with(
                with(r("a", "failing", "agent", "normal"), "actions", json!([{ "label": "Recheck" }, { "label": "Drop", "confirm": "Drop this for good?" }, { "label": "" }])),
                "detail",
                json!("Line one\nline two"),
            ),
            r("b", "todo", "nobody", "normal"),
            r("c", "chore", "you", "normal"),
            with(with(r("d", "todo", "you", "low"), "project", json!("Orchard")), "whenText", json!("65 d late")),
            json!({ "source": "Index", "kind": "todo", "who": "you", "level": "low", "title": "no id" }),
        ]);
        let a = read(&text).unwrap();
        assert_eq!((a.changed, a.urgent, a.failing), (5, 2, 3));
        assert_eq!(a.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["a", "d"]);
        assert_eq!(a.rows[0].detail, "Line one line two", "on one line");
        assert_eq!(a.rows[0].project, "Index", "the source when it says no project");
        assert_eq!(a.rows[1].project, "Orchard");
        // Its words for when are the file's whenText, as the program that writes it names them
        assert_eq!((a.rows[0].when.as_str(), a.rows[1].when.as_str()), ("", "65 d late"));
        assert_eq!(
            a.rows[0].buttons,
            vec![
                Button { label: "Recheck".into(), confirm: None },
                Button { label: "Drop".into(), confirm: Some("Drop this for good?".into()) },
            ],
            "a button without a word is left out"
        );
        // Not an agenda: no counts, or no rows
        assert!(read(r#"{ "rows": [] }"#).is_none());
        assert!(read(r#"{ "counts": { "urgent": 1, "failing": 0 } }"#).is_none());
        assert!(read("not json").is_none());
    }

    fn agenda(rows: Vec<Value>) -> Agenda {
        read(&file(rows)).unwrap()
    }

    fn ids(a: &Agenda, lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| match l {
                Line::Heading { section, count } => format!("# {} {count}", section.title()),
                Line::Group { title, count, open, .. } => format!("{} {title} {count}", if *open { "v" } else { ">" }),
                Line::Row { row, inner } => format!("{}{}", if *inner { "  " } else { "" }, a.rows[*row].id),
                Line::Detail { row } => format!("detail {}", a.rows[*row].id),
                Line::Buttons { row } => format!("buttons {}", a.rows[*row].id),
                Line::Filter { on, count } => format!("filter {on} {count}"),
                Line::Empty => "empty".into(),
            })
            .collect()
    }

    #[test]
    fn urgent_and_failing_first_then_whats_coming_and_yours_the_agents_behind_the_filter() {
        let a = agenda(vec![
            r("mine", "todo", "you", "normal"),
            r("agents", "todo", "agent", "normal"),
            r("broken", "failing", "agent", "normal"),
            r("event", "upcoming", "you", "normal"),
            r("fire", "todo", "you", "urgent"),
            r("down", "failing", "agent", "urgent"),
        ]);
        let mut view = View::default();
        assert_eq!(
            ids(&a, &lines(&a, &view)),
            [
                "# Urgent 2", "fire", "down",
                "# Failing 3", "broken",
                "# Coming up 1", "event",
                "# Yours to do 1", "mine",
                "filter false 1",
            ]
        );
        view.agents = true;
        let shown = ids(&a, &lines(&a, &view));
        assert_eq!(shown[shown.len() - 2..], ["filter true 1", "agents"], "the filter's line their heading");
        // Nothing at all
        assert_eq!(lines(&Agenda::default(), &View::default()), vec![Line::Empty]);
        // Only the agents': the filter alone
        let only = agenda(vec![r("agents", "todo", "agent", "normal")]);
        assert_eq!(ids(&only, &lines(&only, &View::default())), ["filter false 1"]);
    }

    #[test]
    fn a_groups_rows_fold_into_one_line_where_the_first_was_until_its_opened() {
        let g = |id: &str, project: &str, group: &str| with(with(r(id, "todo", "you", "low"), "project", json!(project)), "group", json!(group));
        let a = agenda(vec![
            g("x1", "Orchard", "Extras to answer"),
            r("plain", "todo", "you", "normal"),
            g("y1", "Quarry", "Extras to answer"),
            g("x2", "Orchard", "Extras to answer"),
            g("t1", "Bazaar", "Its dates in the calendar (Bazaar)"),
            g("t2", "Bazaar", "Its dates in the calendar (Bazaar)"),
            r("low1", "todo", "you", "low"),
            r("low2", "todo", "you", "low"),
            with(with(r("burning", "todo", "you", "urgent"), "group", json!("Extras to answer")), "project", json!("Orchard")),
        ]);
        let mut view = View::default();
        assert_eq!(
            ids(&a, &lines(&a, &view)),
            [
                "# Urgent 2", "burning",
                "# Yours to do 8",
                "> Extras to answer \u{b7} Orchard 2",
                "plain",
                "y1",
                "> Its dates in the calendar (Bazaar) 2",
                "> Whenever 2",
            ],
            "by group and project, a group of one as its row, the project left out when the group names it, urgent never folded"
        );
        let Line::Group { key, level, .. } = lines(&a, &view)[3].clone() else { panic!() };
        assert_eq!(level, Level::Low);
        view.groups.insert(key);
        assert_eq!(ids(&a, &lines(&a, &view))[3..6], ["v Extras to answer \u{b7} Orchard 2", "  x1", "  x2"]);
    }

    #[test]
    fn urgents_and_failings_headings_are_the_files_numbers_the_rest_count_their_rows() {
        // As its program counts them: failing takes in the urgent failing row and leaves out the
        // low one, so the Failing section's two rows (one of them low) say 2
        let text = json!({
            "changed": 5,
            "counts": { "urgent": 1, "failing": 2 },
            "rows": [
                r("down", "failing", "agent", "urgent"),
                r("broken", "failing", "agent", "normal"),
                r("flaky", "failing", "agent", "low"),
                r("event", "upcoming", "you", "normal"),
                r("mine", "todo", "you", "normal"),
                r("also", "todo", "you", "normal"),
            ]
        })
        .to_string();
        let a = read(&text).unwrap();
        assert_eq!(
            ids(&a, &lines(&a, &View::default())),
            ["# Urgent 1", "down", "# Failing 2", "broken", "flaky", "# Coming up 1", "event", "# Yours to do 2", "mine", "also"]
        );
    }

    #[test]
    fn an_opened_row_shows_its_detail_and_buttons_under_it() {
        let a = agenda(vec![
            with(r("a", "failing", "agent", "normal"), "detail", json!("What's wrong")),
            r("b", "todo", "you", "normal"),
        ]);
        let mut view = View { row: Some(("Index".into(), "a".into())), ..View::default() };
        assert_eq!(ids(&a, &lines(&a, &view)), ["# Failing 3", "a", "detail a", "buttons a", "# Yours to do 1", "b"]);
        view.row = Some(("Index".into(), "b".into()));
        assert_eq!(ids(&a, &lines(&a, &view))[1..], ["a", "# Yours to do 1", "b", "buttons b"], "no detail line without one, and only the opened row's");
        assert!(Line::Row { row: 0, inner: false }.takes_focus() && !Line::Buttons { row: 0 }.takes_focus());
    }

    #[test]
    fn a_rows_buttons_are_its_own_then_claude_snoozes_and_its_page() {
        let a = agenda(vec![with(
            with(r("a", "todo", "you", "normal"), "actions", json!([{ "label": "Tick" }, { "label": "Drop", "confirm": "Drop this for good?" }])),
            "open",
            json!("/d/chronos"),
        )]);
        let row = &a.rows[0];
        let words: Vec<String> = presses(row).into_iter().map(|(_, w)| w).collect();
        assert_eq!(words, ["Tick", "Drop", "Claude", "Snooze 1 d", "7 d", "Open"]);
        assert_eq!(presses(row)[3].0, Press::Snooze(1));
        assert_eq!(confirm(row, &Press::Action(1)), Some("Drop this for good?"));
        assert_eq!(confirm(row, &Press::Action(0)), None);
        assert_eq!(confirm(row, &Press::Claude), None);
        let plain = agenda(vec![r("b", "todo", "you", "normal")]);
        assert_eq!(presses(&plain.rows[0]).len(), 3, "no page, no Open");
        assert_eq!([Press::Action(0).what(), Press::Claude.what(), Press::Snooze(7).what(), Press::Open.what()], ["do", "claude", "snooze", "open"]);
    }

    #[test]
    fn the_counts_are_the_files_and_colours_say_what_they_are() {
        let a = agenda(vec![r("a", "todo", "you", "normal")]);
        assert_eq!(badges(a.urgent, a.failing), vec![("2".to_string(), URGENT), ("3".to_string(), FAILING)]);
        assert_eq!(badges(0, a.failing), vec![("3".to_string(), FAILING)], "nought isn't shown");
        assert!(badges(0, 0).is_empty());
        assert_eq!(URGENT, 0xFF8A3D, "the waiting orange");
        let b = agenda(vec![r("f", "failing", "agent", "normal"), r("u", "todo", "you", "urgent"), r("t", "todo", "you", "normal")]);
        assert_eq!(b.rows.iter().map(mark).collect::<Vec<_>>(), [Some(FAILING), Some(URGENT), None]);
    }
}
