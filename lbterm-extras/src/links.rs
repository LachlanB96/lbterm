//! Tab links (lbterm/LINKS.md): one tab's Claude session waiting on another's work, so with
//! twenty tabs open it's plain what waits on what (Lachlan's ask, 2026-10-29). A right-button
//! drag of a tab's row onto another's makes the first wait on the second; a line joins their
//! pads down the rail's left edge, hollow at the end that waits, solid at the end waited on, and
//! the waiting pad turns purple (lbterm_panel::RowState::Linked).
//!
//! LBTerm only draws them. The links live in a JSON file a program of the config's keeps
//! (`config.lbterm_links.file`; Terminal's shared/claude/links.ts), which decides when the tab
//! waited on has finished and tells the waiting one to carry on; what Lachlan does on the rail
//! goes to it through the `lbterm-tab-link` Lua event. The file:
//!
//! ```json
//! { "links": [{ "waiter": "<session>", "on": "<session>", "what": "…", "objective": 2,
//!               "state": "waiting|sending|sent|stuck|failed", "note": "…" }],
//!   "waits": [{ "session": "<session>", "what": "…", "on": "<session>", "objective": 2, "dismissed": false }] }
//! ```
//!
//! A wait is what a session says it waits for, and the open session whose objective would end it:
//! drawn as a suggested link, dashed, until it's made or dismissed (Lachlan's E7).
//!
//! The same file has the tab connections (Lachlan's ask, 2026-11-01): two or more tabs whose
//! sessions should be talking to each other, so the program tells each about the others and they
//! message each other. The program suggests them (both wrote the same file, or the objectives'
//! digest finds their work overlapping) and he makes them; a connection has no direction, so its
//! line runs from its topmost member's row to its lowest with the same mark at each member, hollow
//! while it's suggested and solid once it's made, in cyan (`palette::CONNECTED`):
//!
//! ```json
//! { "connections": [{ "id": "c-…", "members": ["<session>", "<session>"], "state": "suggested|connected|declined",
//!                     "at": 1793250000000, "stateAt": 1793250000000, "by": "files|digest|lbterm|cli", "why": "…",
//!                     "note": "", "messages": [{ "from": "<session>", "to": "<session>", "at": 1793250000000, "text": "…" }] }] }
//! ```
//!
//! What's here: reading the file ([`read`]), each row's link as the panel shows it ([`row_link`]),
//! the links' and connections' lines laid out in lanes together ([`lay_out`]) and what each row's
//! slot draws of them ([`Lanes::pieces`]), whether the pointer is on a line ([`on_line`]), the
//! cards a drag shows ([`waits_card`], [`aims_card`]) and a line's ([`line_card`],
//! [`connection_card`]), and what Lachlan does to a connection: the event's words ([`Act`]), the
//! tab menu's rows ([`connection_rows`]) and what the rail draws until the file says it
//! ([`ConnectionChange`]). The GUI side is wezterm-gui/src/state_files.rs (the file),
//! render/tab_panel.rs (the lines and pads), render/connections.rs (a suggested connection's
//! light), termwindow/tab_links.rs (the drag, the events, the menu's rows) and render/link_drag.rs
//! (the drag's look and the cards).

use crate::objectives::{Objectives, Status};
use crate::tabs::Rect;

/// Where a link stands, as the file says it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LinkState {
    /// What it waits on hasn't finished
    Waiting,
    /// Finished: the waiting session is being told
    Sending,
    /// Told
    Sent,
    /// Told, and stopped again still blocked
    Stuck,
    /// Couldn't be told
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub waiter: String,
    pub on: String,
    pub what: String,
    pub objective: Option<u64>,
    pub state: LinkState,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Wait {
    pub session: String,
    pub what: String,
    pub on: Option<String>,
    pub objective: Option<u64>,
    pub dismissed: bool,
}

/// Where a tab connection stands, as the file says it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionState {
    /// Found by the program and not yet his: drawn with a light running along it
    Suggested,
    /// His: each member's session has been told about the others. Drawn steady
    Connected,
    /// He said no: nothing drawn, nothing in the menu
    Declined,
}

impl ConnectionState {
    pub fn name(self) -> &'static str {
        match self {
            ConnectionState::Suggested => "suggested",
            ConnectionState::Connected => "connected",
            ConnectionState::Declined => "declined",
        }
    }
}

/// A message one member of a connection sent another, as the program recorded it (Lachlan's E7)
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub from: String,
    pub to: String,
    /// Unix milliseconds
    pub at: u64,
    pub text: String,
}

/// Two or more tabs' Claude sessions that should be talking to each other
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    /// What the `lbterm-tab-link` event names it by
    pub id: String,
    /// Two at least, each once, in the file's order. A suggestion has two; a connection made can
    /// have more (his E6: a tab connected to a connected one joins it)
    pub members: Vec<String>,
    pub state: ConnectionState,
    /// When it was found, and when it came to stand as it does, in Unix milliseconds
    pub at: u64,
    pub state_at: u64,
    /// What found or made it: "files", "digest", "lbterm" or "cli"
    pub by: String,
    /// Why it was suggested, [`WHY_CHARS`] at most
    pub why: String,
    /// Empty, or why an introduction couldn't be delivered, shown in red
    pub note: String,
    /// The last [`MESSAGES`] its members sent each other, oldest first: only on a connected one
    pub messages: Vec<Message>,
}

/// How long a connection's why may be, a message's words, and how many messages are kept
pub const WHY_CHARS: usize = 120;
pub const MESSAGE_CHARS: usize = 160;
pub const MESSAGES: usize = 5;

/// What the file says
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Links {
    pub links: Vec<Link>,
    pub waits: Vec<Wait>,
    pub connections: Vec<Connection>,
}

fn text(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|s| s.as_str()).unwrap_or_default().trim().to_string()
}

fn number(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|n| n.as_u64()).filter(|n| *n > 0)
}

/// A connection from the file: none without an id, with a state this doesn't know, or with fewer
/// than two members (a member named twice counts once)
fn connection(c: &serde_json::Value) -> Option<Connection> {
    let state = match c.get("state")?.as_str()? {
        "suggested" => ConnectionState::Suggested,
        "connected" => ConnectionState::Connected,
        "declined" => ConnectionState::Declined,
        _ => return None,
    };
    let id = text(c, "id");
    let mut members: Vec<String> = vec![];
    for m in c.get("members")?.as_array()? {
        if let Some(m) = m.as_str().map(str::trim).filter(|m| !m.is_empty()) {
            if !members.iter().any(|x| x == m) {
                members.push(m.to_string());
            }
        }
    }
    if id.is_empty() || members.len() < 2 {
        return None;
    }
    let messages = match (state, c.get("messages").and_then(|m| m.as_array())) {
        (ConnectionState::Connected, Some(list)) => {
            let all: Vec<Message> = list
                .iter()
                .filter_map(|m| {
                    let (from, to, words) = (text(m, "from"), text(m, "to"), text(m, "text"));
                    (!from.is_empty() && !to.is_empty() && !words.is_empty()).then(|| Message {
                        from,
                        to,
                        at: m.get("at").and_then(|n| n.as_u64()).unwrap_or(0),
                        text: elide(&words, MESSAGE_CHARS),
                    })
                })
                .collect();
            all[all.len().saturating_sub(MESSAGES)..].to_vec()
        }
        _ => vec![],
    };
    let stamp = |key: &str| c.get(key).and_then(|n| n.as_u64()).unwrap_or(0);
    Some(Connection {
        id,
        members,
        state,
        at: stamp("at"),
        state_at: stamp("stateAt"),
        by: text(c, "by"),
        why: elide(&text(c, "why"), WHY_CHARS),
        note: text(c, "note"),
        messages,
    })
}

/// The file's contents, or nothing when it isn't a JSON object. A link or a wait without its
/// sessions, or with a state this doesn't know, is left out, and so is a connection without its
/// id or two members, or with a state this doesn't know.
pub fn read(content: &str) -> Option<Links> {
    let value: serde_json::Value = serde_json::from_str(content.trim_start_matches('\u{feff}')).ok()?;
    value.as_object()?;
    let list = |key: &str| value.get(key).and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let links = list("links")
        .iter()
        .filter_map(|l| {
            let state = match l.get("state")?.as_str()? {
                "waiting" => LinkState::Waiting,
                "sending" => LinkState::Sending,
                "sent" => LinkState::Sent,
                "stuck" => LinkState::Stuck,
                "failed" => LinkState::Failed,
                _ => return None,
            };
            let (waiter, on) = (text(l, "waiter"), text(l, "on"));
            (!waiter.is_empty() && !on.is_empty()).then(|| Link {
                waiter,
                on,
                what: text(l, "what"),
                objective: number(l, "objective"),
                state,
                note: text(l, "note"),
            })
        })
        .collect();
    let waits = list("waits")
        .iter()
        .filter_map(|w| {
            let (session, what) = (text(w, "session"), text(w, "what"));
            (!session.is_empty() && !what.is_empty()).then(|| Wait {
                session,
                what,
                on: Some(text(w, "on")).filter(|s| !s.is_empty()),
                objective: number(w, "objective"),
                dismissed: w.get("dismissed").and_then(|d| d.as_bool()).unwrap_or(false),
            })
        })
        .collect();
    let connections = list("connections").iter().filter_map(connection).collect();
    Some(Links { links, waits, connections })
}

impl Links {
    /// The link from `waiter` to `on`, if they're linked
    pub fn between(&self, waiter: &str, on: &str) -> Option<&Link> {
        self.links.iter().find(|l| l.waiter == waiter && l.on == on)
    }

    /// The links a session waits by
    pub fn of(&self, waiter: &str) -> impl Iterator<Item = &Link> {
        let waiter = waiter.to_string();
        self.links.iter().filter(move |l| l.waiter == waiter)
    }

    /// What a session says it waits for, if it has said
    pub fn wait(&self, session: &str) -> Option<&Wait> {
        self.waits.iter().find(|w| w.session == session)
    }

    /// The link suggested for a session: what it says it waits for names an open session's
    /// objective, it hasn't been dismissed, and the session waits by no link yet
    pub fn suggestion(&self, session: &str) -> Option<&Wait> {
        self.wait(session)
            .filter(|w| w.on.is_some() && !w.dismissed && w.on.as_deref() != Some(session))
            .filter(|_| self.of(session).next().is_none())
    }

    /// The connection named `id`
    pub fn connection(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// The connections a session is in that show, suggested or made: not those declined
    pub fn connections_of<'a>(&'a self, session: &'a str) -> impl Iterator<Item = &'a Connection> + 'a {
        self.connections
            .iter()
            .filter(move |c| c.state != ConnectionState::Declined && c.members.iter().any(|m| m == session))
    }

    /// The connections made a session is in that the program knows: not one the rail draws under
    /// [`PENDING_ID`] before the file has it, which has no id the program could act on. The tab
    /// menu's Leave rows and the state file's rows say these
    pub fn made_of<'a>(&'a self, session: &'a str) -> impl Iterator<Item = &'a Connection> + 'a {
        self.connections_of(session).filter(|c| c.state == ConnectionState::Connected && c.id != PENDING_ID)
    }

    /// Whether two sessions are in a connection made together
    pub fn together(&self, a: &str, b: &str) -> bool {
        self.connections.iter().any(|c| c.state == ConnectionState::Connected && c.has(a) && c.has(b))
    }

    /// How many suggested connections have at least one member among `rail`, the sessions of a
    /// window's rail's tabs (a tab without one `None`): the count on the Conductor's pad (his E8).
    /// One made or declined doesn't count, nor one whose members are all in other windows or
    /// closed. One member is enough, so a suggestion with a tab here and one elsewhere, which draws
    /// no line, still counts
    pub fn suggested_on<S: AsRef<str>>(&self, rail: &[Option<S>]) -> usize {
        self.connections
            .iter()
            .filter(|c| c.state == ConnectionState::Suggested)
            .filter(|c| rail.iter().flatten().any(|s| c.has(s.as_ref())))
            .count()
    }
}

/// The Conductor's pad's badge for `suggested` connections suggested on its rail
/// ([`Links::suggested_on`]): the count, and none at 0
pub fn badge(suggested: usize) -> Option<String> {
    (suggested > 0).then(|| suggested.to_string())
}

/// What of the modifiers held, `[ctrl, shift, alt]`, the active pane's outline shows
/// (wezterm-gui's termwindow/held.rs): all of them, but Shift while a tab is right-dragged on the
/// rail (`dragging`), where Shift says the drag connects the two tabs and the drag turns cyan for
/// it, so the pane's outline needn't go green as well (Lachlan's 2a, 2026-11-01). Still held once
/// the drag ends, it shows again
pub fn outline_held(held: [bool; 3], dragging: bool) -> [bool; 3] {
    let [ctrl, shift, alt] = held;
    [ctrl, shift && !dragging, alt]
}

impl Connection {
    /// Whether a session is one of its members
    pub fn has(&self, session: &str) -> bool {
        self.members.iter().any(|m| m == session)
    }
}

// ---------- what Lachlan does to a connection ----------

/// One of Lachlan's acts on a tab connection, as the `lbterm-tab-link` event tells the config's
/// handler (lbterm/LINKS.md, "Connections"), which has the file's program do it
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// A suggestion made: a left click on its line, or the tab menu's "Connect with …, as suggested"
    Accept { connection: String },
    /// A suggestion declined, from the tab menu
    Decline { connection: String },
    /// A tab's session taken out of a connection, from its tab menu
    Leave { connection: String, session: String },
    /// A right-drag of `waiter`'s row let go on `on`'s with Shift held: the program connects them
    /// (a new connection, or one joining the other's), or takes `waiter` out where they're
    /// connected together already
    Connect { waiter: String, on: String },
}

impl Act {
    /// The event's `action`
    pub fn action(&self) -> &'static str {
        match self {
            Act::Accept { .. } => "accept",
            Act::Decline { .. } => "decline",
            Act::Leave { .. } => "leave",
            Act::Connect { .. } => "connect",
        }
    }

    /// The event table's other fields, by name: `connection` and `session`, or `waiter` and `on`
    pub fn fields(&self) -> Vec<(&'static str, String)> {
        match self {
            Act::Accept { connection } | Act::Decline { connection } => vec![("connection", connection.clone())],
            Act::Leave { connection, session } => vec![("connection", connection.clone()), ("session", session.clone())],
            Act::Connect { waiter, on } => vec![("waiter", waiter.clone()), ("on", on.clone())],
        }
    }
}

/// What the rail draws of an act on a connection until the file says it (for up to the GUI's
/// `PENDING`, as for links): made shows connected, declined goes, left goes or shrinks, and a
/// Shift-drag shows the two connected, or the one dragged gone from the connections they share
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionChange {
    Accepted { connection: String },
    Declined { connection: String },
    Left { connection: String, session: String },
    /// Connected together: the one dragged joins the other's connection made, else the other
    /// joins the dragged one's, else a suggestion between them is made, else a new one
    Joined { waiter: String, on: String },
    /// They were connected together: the one dragged is taken out of each connection they share
    Parted { waiter: String, on: String },
}

/// The id a connection the rail draws before the file has it goes by
pub const PENDING_ID: &str = "pending";

impl ConnectionChange {
    /// What `act` changes, the file standing as `links` says: a Shift-drag onto a tab it's
    /// connected with already takes it out, as the program's toggle does
    pub fn of(act: &Act, links: &Links) -> ConnectionChange {
        match act {
            Act::Accept { connection } => ConnectionChange::Accepted { connection: connection.clone() },
            Act::Decline { connection } => ConnectionChange::Declined { connection: connection.clone() },
            Act::Leave { connection, session } => ConnectionChange::Left { connection: connection.clone(), session: session.clone() },
            Act::Connect { waiter, on } if links.together(waiter, on) => ConnectionChange::Parted { waiter: waiter.clone(), on: on.clone() },
            Act::Connect { waiter, on } => ConnectionChange::Joined { waiter: waiter.clone(), on: on.clone() },
        }
    }

    /// Whether the file says it already (a connection it's about that the file no longer has
    /// counts: the file has had its word)
    pub fn said(&self, links: &Links) -> bool {
        match self {
            ConnectionChange::Accepted { connection } | ConnectionChange::Declined { connection } => {
                links.connection(connection).map_or(true, |c| c.state != ConnectionState::Suggested)
            }
            ConnectionChange::Left { connection, session } => links.connection(connection).map_or(true, |c| !c.has(session)),
            ConnectionChange::Joined { waiter, on } => links.together(waiter, on),
            ConnectionChange::Parted { waiter, on } => !links.together(waiter, on),
        }
    }

    /// The file's word with this laid over it
    pub fn apply(&self, links: &mut Links) {
        let list = &mut links.connections;
        let pair = |c: &Connection, a: &str, b: &str| c.has(a) && c.has(b);
        match self {
            ConnectionChange::Accepted { connection } => {
                for c in list.iter_mut().filter(|c| c.id == *connection) {
                    c.state = ConnectionState::Connected;
                }
            }
            ConnectionChange::Declined { connection } => {
                for c in list.iter_mut().filter(|c| c.id == *connection) {
                    c.state = ConnectionState::Declined;
                }
            }
            ConnectionChange::Left { connection, session } => {
                for c in list.iter_mut().filter(|c| c.id == *connection) {
                    c.members.retain(|m| m != session);
                }
            }
            ConnectionChange::Joined { waiter, on } => {
                let made = |s: &str| list.iter().position(|c| c.state == ConnectionState::Connected && c.has(s));
                let suggested = list.iter().position(|c| c.state == ConnectionState::Suggested && pair(c, waiter, on));
                match (made(on), made(waiter), suggested) {
                    (Some(k), _, _) => list[k].members.push(waiter.clone()),
                    (None, Some(k), _) => list[k].members.push(on.clone()),
                    (None, None, Some(k)) => list[k].state = ConnectionState::Connected,
                    (None, None, None) => list.push(Connection {
                        id: PENDING_ID.into(),
                        members: vec![waiter.clone(), on.clone()],
                        state: ConnectionState::Connected,
                        at: 0,
                        state_at: 0,
                        by: "lbterm".into(),
                        why: String::new(),
                        note: String::new(),
                        messages: vec![],
                    }),
                }
                // A suggestion between the two says nothing more once they're connected
                list.retain(|c| !(c.state == ConnectionState::Suggested && pair(c, waiter, on)));
            }
            ConnectionChange::Parted { waiter, on } => {
                for c in list.iter_mut().filter(|c| c.state == ConnectionState::Connected && pair(c, waiter, on)) {
                    c.members.retain(|m| m != waiter);
                }
            }
        }
        // One left with fewer than two members is no connection
        list.retain(|c| c.members.len() >= 2);
    }

    /// Whether this says again what `other` said, so the later of the two is the one that shows: an
    /// accept or a decline of the same connection, a tab leaving the same connection, or a
    /// Shift-drag between the same two tabs. One tab leaving doesn't replace another's leaving the
    /// same connection, nor anything else done to it: both show until the file says them
    pub fn replaces(&self, other: &ConnectionChange) -> bool {
        use ConnectionChange::*;
        fn pair<'a>(waiter: &'a str, on: &'a str) -> [&'a str; 2] {
            let mut p = [waiter, on];
            p.sort();
            p
        }
        match (self, other) {
            (Accepted { connection: a } | Declined { connection: a }, Accepted { connection: b } | Declined { connection: b }) => a == b,
            (Left { connection: a, session: s }, Left { connection: b, session: t }) => a == b && s == t,
            (Joined { waiter: a, on: b } | Parted { waiter: a, on: b }, Joined { waiter: c, on: d } | Parted { waiter: c, on: d }) => {
                pair(a, b) == pair(c, d)
            }
            _ => false,
        }
    }
}

/// Labels as a list in words: "A", "A and B", "A, B and C"
pub fn names(list: &[String]) -> String {
    match list {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The tab menu's rows for a session's connections, under its links' rows (lbterm/LINKS.md): for
/// each connection made that it's in, "Leave the connection with <the others>"; for each one
/// suggested, "Connect with <the other>, as suggested" and "Decline the suggested connection with
/// <the other>". `name(s)` is session `s`'s label. A connection the rail draws before the file has
/// it ([`PENDING_ID`]) has none: its id is the rail's own, and a Leave would name it to the program.
pub fn connection_rows(links: &Links, session: &str, name: impl Fn(&str) -> String) -> Vec<(String, Act)> {
    let others = |c: &Connection| names(&c.members.iter().filter(|m| *m != session).map(|m| name(m)).collect::<Vec<_>>());
    let mut rows = vec![];
    for c in links.made_of(session) {
        rows.push((
            format!("Leave the connection with {}", others(c)),
            Act::Leave { connection: c.id.clone(), session: session.to_string() },
        ));
    }
    for c in links.connections_of(session).filter(|c| c.state == ConnectionState::Suggested) {
        let other = others(c);
        rows.push((format!("Connect with {other}, as suggested"), Act::Accept { connection: c.id.clone() }));
        rows.push((format!("Decline the suggested connection with {other}"), Act::Decline { connection: c.id.clone() }));
    }
    rows
}

// ---------- a row's link, as its pad and words show it ----------

/// The panel's word for where a tab's link stands, worst first: what went wrong outranks what's
/// going right, so a row with two links says the one that needs him
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Standing {
    Stuck,
    Failed,
    /// The session it waits on isn't in any tab
    Closed,
    Waiting,
    Telling,
    Told,
}

impl Standing {
    pub fn of(state: LinkState, on_open: bool) -> Standing {
        match state {
            LinkState::Stuck => Standing::Stuck,
            LinkState::Failed => Standing::Failed,
            LinkState::Waiting if !on_open => Standing::Closed,
            LinkState::Waiting => Standing::Waiting,
            LinkState::Sending => Standing::Telling,
            LinkState::Sent => Standing::Told,
        }
    }

    pub fn error(self) -> bool {
        matches!(self, Standing::Stuck | Standing::Failed | Standing::Closed)
    }

    pub fn name(self) -> &'static str {
        match self {
            Standing::Stuck => "stuck",
            Standing::Failed => "failed",
            Standing::Closed => "closed",
            Standing::Waiting => "waiting",
            Standing::Telling => "telling",
            Standing::Told => "told",
        }
    }
}

/// A tab's link as its row shows it: the worst of the links its session waits by
#[derive(Debug, Clone, PartialEq)]
pub struct RowLink {
    pub standing: Standing,
    /// The session it waits on
    pub on: String,
    pub what: String,
    pub note: String,
}

/// The link row `session` shows, if it waits by any: `open(s)` says whether session `s` is in
/// any tab of any window, so a link whose session waited on is gone says so
pub fn row_link(links: &Links, session: &str, open: impl Fn(&str) -> bool) -> Option<RowLink> {
    links
        .of(session)
        .map(|l| RowLink {
            standing: Standing::of(l.state, open(&l.on)),
            on: l.on.clone(),
            what: l.what.clone(),
            note: l.note.clone(),
        })
        .min_by_key(|r| r.standing)
}

// ---------- the lines down the rail ----------

/// How a line is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    /// Waiting: purple
    Waiting,
    /// What it waits on finished: green until it has carried on
    Done,
    /// Something went wrong: red
    Error,
    /// Suggested, not made: faint purple, dashed
    Suggested,
    /// A connection suggested: faint cyan, its marks hollow, a light running along it with the
    /// compositor and dashed without
    ConnectionSuggested,
    /// A connection made: steady cyan, its marks solid
    Connected,
}

impl Look {
    pub fn of(state: LinkState) -> Look {
        match state {
            LinkState::Waiting => Look::Waiting,
            LinkState::Sending | LinkState::Sent => Look::Done,
            LinkState::Stuck | LinkState::Failed => Look::Error,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Look::Waiting => "waiting",
            Look::Done => "done",
            Look::Error => "error",
            Look::Suggested => "suggested",
            Look::ConnectionSuggested => "connection-suggested",
            Look::Connected => "connected",
        }
    }

    /// A connection's line, as it stands: none for one declined
    pub fn of_connection(state: ConnectionState) -> Option<Look> {
        match state {
            ConnectionState::Suggested => Some(Look::ConnectionSuggested),
            ConnectionState::Connected => Some(Look::Connected),
            ConnectionState::Declined => None,
        }
    }

    /// Whether the mark at an end is hollow: the waiting tab's, and every member's of a suggested
    /// connection
    pub fn hollow(self, end: End) -> bool {
        match end {
            End::Waiter => true,
            End::On => false,
            End::Member => self == Look::ConnectionSuggested,
        }
    }
}

/// A line between two rows of the rail: from the row that waits to the row it waits on
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Line {
    pub waiter: usize,
    pub on: usize,
    /// Its lane down the rail's edge, 0 nearest the pills
    pub lane: usize,
    pub look: Look,
}

impl Line {
    fn span(&self) -> (usize, usize) {
        (self.waiter.min(self.on), self.waiter.max(self.on))
    }
}

/// A tab connection's line on this rail: from its topmost member's row to its lowest, with a mark
/// at each member's row and the rows between passed by
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Join {
    /// Its place in the file's connections (`Links::connections`)
    pub connection: usize,
    /// Its members' rows on this rail, top to bottom: two at least
    pub rows: Vec<usize>,
    /// Its lane down the rail's edge, shared out with the links' lines
    pub lane: usize,
    /// `Look::ConnectionSuggested` or `Look::Connected`
    pub look: Look,
}

impl Join {
    /// Its topmost row and its lowest
    pub fn span(&self) -> (usize, usize) {
        (self.rows[0], self.rows[self.rows.len() - 1])
    }
}

/// The lines between this rail's rows, the links' and the connections', each in its lane
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lanes {
    pub lines: Vec<Line>,
    pub joins: Vec<Join>,
}

/// The lines between this rail's rows, `rows[i]` being row `i`'s session: one per link and per
/// suggested link whose two sessions are both on it, and one per connection, suggested or made,
/// with two members or more on it (spanning those). Each takes a lane of `lanes` that no other
/// line on the rows it spans has (rows it ends on count, so two lines never join into one). The
/// links' lines are laid out first, the shortest nearest the pills, so their lanes are what they
/// always were whatever connections there are; then the connections', the shortest first, in the
/// lanes the links leave. A line that finds no lane free shares the last.
pub fn lay_out(rows: &[Option<&str>], links: &Links, lanes: usize) -> Lanes {
    let row_of = |s: &str| rows.iter().position(|r| *r == Some(s));
    let mut out: Vec<Line> = links
        .links
        .iter()
        .filter_map(|l| {
            let (waiter, on) = (row_of(&l.waiter)?, row_of(&l.on)?);
            (waiter != on).then_some(Line { waiter, on, lane: 0, look: Look::of(l.state) })
        })
        .collect();
    for w in &links.waits {
        let Some(on) = links.suggestion(&w.session).and_then(|w| w.on.as_deref()) else {
            continue;
        };
        if let (Some(waiter), Some(on)) = (row_of(&w.session), row_of(on)) {
            if waiter != on {
                out.push(Line { waiter, on, lane: 0, look: Look::Suggested });
            }
        }
    }
    let joins = links.connections.iter().enumerate().filter_map(|(k, c)| {
        let look = Look::of_connection(c.state)?;
        let rows: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.map_or(false, |s| c.members.iter().any(|m| m == s)))
            .map(|(i, _)| i)
            .collect();
        (rows.len() >= 2).then_some(Join { connection: k, rows, lane: 0, look })
    });
    // Every line, the links' first, then the connections': each by length, then top, then the
    // waiting row or the connection's place in the file. A connection shorter than a link it
    // overlaps doesn't take the link's lane (the links' lanes stay as they were: lbterm/LINKS.md),
    // and a stable sort keeps the links' order as it was
    enum Item {
        Line(Line),
        Join(Join),
    }
    let mut items: Vec<((usize, usize), (u8, usize, usize, usize), Item)> = out
        .into_iter()
        .map(|l| (l.span(), (0, l.span().1 - l.span().0, l.span().0, l.waiter), Item::Line(l)))
        .chain(joins.map(|j| (j.span(), (1, j.span().1 - j.span().0, j.span().0, j.connection), Item::Join(j))))
        .collect();
    items.sort_by_key(|(_, key, _)| *key);
    let lanes = lanes.max(1);
    let mut placed: Vec<((usize, usize), usize)> = vec![];
    let mut laid = Lanes::default();
    for ((top, bottom), _, item) in items {
        let lane = (0..lanes)
            .find(|&lane| placed.iter().filter(|(_, l)| *l == lane).all(|&((t, b), _)| b < top || t > bottom))
            .unwrap_or(lanes - 1);
        placed.push(((top, bottom), lane));
        match item {
            Item::Line(line) => laid.lines.push(Line { lane, ..line }),
            Item::Join(join) => laid.joins.push(Join { lane, ..join }),
        }
    }
    laid
}

/// The links' lines alone, as [`lay_out`] lays them out
pub fn lines(rows: &[Option<&str>], links: &Links, lanes: usize) -> Vec<Line> {
    lay_out(rows, links, lanes).lines
}

/// Where a line ends on a row
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum End {
    /// The row that waits: a hollow mark
    Waiter,
    /// The row waited on: a solid mark
    On,
    /// A connection's member: hollow while it's suggested, solid once it's made ([`Look::hollow`])
    Member,
}

/// What a row's slot draws of a line: its lane, whether the line runs on up out of the slot and
/// down out of it, and its mark where it ends here. Drawn in the row's own slot, so it moves with
/// the row when tabs come, go or change places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece {
    pub lane: usize,
    pub look: Look,
    pub up: bool,
    pub down: bool,
    pub end: Option<End>,
}

/// What row `row`'s slot draws of `lines`
pub fn pieces(lines: &[Line], row: usize) -> Vec<Piece> {
    lines
        .iter()
        .filter_map(|l| {
            let (top, bottom) = l.span();
            (top..=bottom).contains(&row).then(|| Piece {
                lane: l.lane,
                look: l.look,
                up: row > top,
                down: row < bottom,
                end: if row == l.waiter {
                    Some(End::Waiter)
                } else if row == l.on {
                    Some(End::On)
                } else {
                    None
                },
            })
        })
        .collect()
}

impl Lanes {
    /// What row `row`'s slot draws of the lines: the connections' pieces, the suggested ones' before
    /// those made, then the links', so a link's across and its mark are drawn over a connection's
    /// where they meet, as they were before connections, and a connection made over a suggestion.
    /// A connection's piece ends at each of its members' rows with a mark and runs on past the rows
    /// between, which aren't its
    pub fn pieces(&self, row: usize) -> Vec<Piece> {
        let mut out: Vec<Piece> = self.joins.iter().filter_map(|j| {
            let (top, bottom) = j.span();
            (top..=bottom).contains(&row).then(|| Piece {
                lane: j.lane,
                look: j.look,
                up: row > top,
                down: row < bottom,
                end: j.rows.contains(&row).then_some(End::Member),
            })
        }).collect();
        // A stable sort: the joins' own order among the suggested ones, and among those made
        out.sort_by_key(|p| p.look != Look::ConnectionSuggested);
        out.extend(pieces(&self.lines, row));
        out
    }

    /// How many pixels bigger all round each of a row's marks is drawn, piece by piece: every
    /// mark on a row sits in the same place, left of its pad, so a connection's member mark is one
    /// bigger for each member mark drawn over it, and one more when the row ends a link's line, so
    /// each shows as a rim round the next (a suggestion's hollow cyan round a connection made, the
    /// cyan round a link's mark). A link's own marks are as they always were: 0
    pub fn rims(pieces: &[Piece]) -> Vec<usize> {
        let link_end = pieces.iter().any(|p| matches!(p.end, Some(End::Waiter | End::On)));
        pieces
            .iter()
            .enumerate()
            .map(|(k, p)| match p.end {
                Some(End::Member) => pieces[k + 1..].iter().filter(|q| q.end == Some(End::Member)).count() + usize::from(link_end),
                _ => 0,
            })
            .collect()
    }
}

/// Whether the point (`x`, `y`) is on a line down the rail: on its lane at `lane` between its
/// topmost end and its lowest, or across from the lane to the mark at any of its `ends` (each
/// mark's middle), `slack` either side counting, and a mark's `half` across and `px` past it. A
/// link's line has its two ends; a connection's an end at each member, the rows between passed by.
pub fn on_line(at: (f32, f32), lane: f32, ends: &[(f32, f32)], slack: f32, half: f32, px: f32) -> bool {
    line_distance(at, lane, ends, slack, half, px).is_some()
}

/// How far the point is from what's drawn of a line, its lane and the pieces across to its marks,
/// when it's on the line as [`on_line`] says: the rail's lanes are a few pixels apart, closer
/// than the slack either side of each, so a point between two lines is taken to the nearer
pub fn line_distance((x, y): (f32, f32), lane: f32, ends: &[(f32, f32)], slack: f32, half: f32, px: f32) -> Option<f32> {
    let (Some(top), Some(bottom)) = (ends.iter().map(|e| e.1).reduce(f32::min), ends.iter().map(|e| e.1).reduce(f32::max)) else {
        return None;
    };
    let down = ((x - lane).abs() <= slack && y >= top - slack && y <= bottom + slack)
        .then(|| (x - lane).hypot((top - y).max(y - bottom).max(0.0)));
    let across = ends
        .iter()
        .filter(|&&(end_x, end_y)| (y - end_y).abs() <= slack.max(half) && x >= lane - slack && x <= end_x + half + px)
        .map(|&(end_x, end_y)| (lane - x).max(x - (end_x + half)).max(0.0).hypot(y - end_y))
        .reduce(f32::min);
    match (down, across) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Where the dashes of a suggested line go between `y0` and `y1`: `on` long, `off` apart, counted
/// from the window's top so the pieces in neighbouring slots carry the same pattern on
pub fn dashes(y0: f32, y1: f32, on: f32, off: f32) -> Vec<(f32, f32)> {
    let period = (on + off).max(1.0);
    let mut out = vec![];
    let mut start = (y0 / period).floor() * period;
    while start < y1 {
        let (a, b) = (start.max(y0), (start + on).min(y1));
        if b > a {
            out.push((a, b));
        }
        start += period;
    }
    out
}

// ---------- a suggested connection's light ----------

/// A run from one end of a line to the other takes its length at this many pixels a second (at
/// 96 DPI), and this many seconds at least: the light goes there and back each lap, easing at the
/// ends, since a connection has no direction
const SPEED: f64 = 70.0;
const RUN_MIN: f64 = 0.9;
/// Where the light is this many times a lap, on a sine: DWM goes straight between them
const STEPS: usize = 32;
/// Where a light goes along a line from `top` to `bottom` at `x`, its template `size` across and
/// tall: offsets for its visual through one lap, starting at the middle going down, there and back
/// on a sine, and the lap's seconds. `scale` is the DPI over 96.
pub fn glow_path(x: f32, (top, bottom): (f32, f32), size: (u32, u32), scale: f64) -> (f64, Vec<(f32, f32)>) {
    let length = (bottom - top).abs() as f64;
    let run = (length / (SPEED * scale.max(0.1))).max(RUN_MIN);
    let (mid, half) = ((top + bottom) / 2.0, (bottom - top) / 2.0);
    let (dx, dy) = (size.0 as f32 / 2.0, size.1 as f32 / 2.0);
    let path = (0..STEPS)
        .map(|k| {
            let t = k as f32 / STEPS as f32;
            let y = mid + half * (t * std::f32::consts::TAU).sin();
            ((x - dx).round(), (y - dy).round())
        })
        .collect();
    (2.0 * run, path)
}

// ---------- the cards a drag shows ----------

/// One line of a card a drag shows
#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tone {
    /// The tab the card is about
    Title,
    /// "waits for", "Its objectives", dim
    Heading,
    /// What it waits for, an objective's words
    Text,
    /// An objective's status word, in its colour
    Status(Status),
    /// Nothing said: dim
    Quiet,
    /// Where a link stands, in its line's colour
    Look(Look),
}

/// A card a drag shows: its outline, its lines, and the block that matches the other card's, the
/// two lit green with a light running round them (Lachlan's E6)
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub rect: Rect,
    pub lines: Vec<CardLine>,
    pub matched: Option<Rect>,
}

const PAD: f64 = 10.0;
const GAP: f64 = 4.0;

fn wrap(text: &str, columns: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut line = String::new();
    for word in text.split_whitespace() {
        let need = line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
        if !line.is_empty() && need > columns {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        while line.chars().count() > columns.max(1) {
            let rest: String = line.chars().skip(columns.max(1)).collect();
            out.push(line.chars().take(columns.max(1)).collect());
            line = rest;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn elide(text: &str, columns: usize) -> String {
    if text.chars().count() <= columns {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(columns.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

/// The card that follows a dragged tab (Lachlan's E5): what its session waits for, in its own
/// words, `width` wide with its top left at `at`. `matched` lights it as the match for the tab
/// under the pointer. `cell` is a column's width and a line's height; `scale` the DPI over 96.
pub fn waits_card(title: &str, what: Option<&str>, at: (f64, f64), width: f64, cell: (f64, f64), scale: f64, matched: bool) -> Card {
    let pad = PAD * scale;
    let columns = (((width - 2.0 * pad) / cell.0).floor() as usize).max(8);
    let (x, mut y) = (at.0 + pad, at.1 + pad);
    let mut lines = vec![CardLine { x, y, text: elide(title, columns), tone: Tone::Title }];
    y += cell.1;
    let body_top = y;
    match what {
        Some(what) => {
            lines.push(CardLine { x, y, text: "waits for".into(), tone: Tone::Heading });
            y += cell.1;
            for w in wrap(what, columns) {
                lines.push(CardLine { x, y, text: w, tone: Tone::Text });
                y += cell.1;
            }
        }
        None => {
            lines.push(CardLine { x, y, text: "hasn't said what it waits for".into(), tone: Tone::Quiet });
            y += cell.1;
        }
    }
    let rect = Rect { x: at.0, y: at.1, w: width, h: y - at.1 + pad };
    let matched = (matched && what.is_some()).then(|| Rect {
        x: at.0 + pad / 2.0,
        y: body_top - GAP * scale / 2.0,
        w: width - pad,
        h: y - body_top + GAP * scale,
    });
    Card { rect, lines, matched }
}

/// The card beside the row a dragged tab is over (Lachlan's E5): that tab's objectives, the
/// unfinished first, each its status word and its words wrapped beside it. `matched` is the
/// objective the dragged tab's wait matches, lit. Its right side at `left` plus `width`, its top
/// level with `top`, moved up to end by `bottom`.
pub fn aims_card(
    title: &str,
    aims: Option<&Objectives>,
    matched: Option<u64>,
    left: f64,
    (top, bottom): (f64, f64),
    width: f64,
    cell: (f64, f64),
    scale: f64,
) -> Card {
    let pad = PAD * scale;
    let gap = GAP * scale;
    let label_columns = 8;
    let text_x = left + pad + label_columns as f64 * cell.0;
    let columns = (((left + width - pad - text_x) / cell.0).floor() as usize).max(8);
    let title_columns = (((width - 2.0 * pad) / cell.0).floor() as usize).max(8);
    let mut lines = vec![CardLine { x: left + pad, y: pad, text: elide(title, title_columns), tone: Tone::Title }];
    let mut y = pad + cell.1 + gap;
    let mut lit = None;
    let order: Vec<_> = match aims {
        Some(o) => {
            let open = |s: Status| matches!(s, Status::Open | Status::Waiting);
            let mut list: Vec<_> = o.list.iter().filter(|x| x.status != Status::Dropped).collect();
            list.sort_by_key(|x| !open(x.status));
            list
        }
        None => vec![],
    };
    if order.is_empty() {
        lines.push(CardLine { x: left + pad, y, text: "no objectives yet".into(), tone: Tone::Quiet });
        y += cell.1;
    }
    for item in order {
        let block_top = y;
        lines.push(CardLine { x: left + pad, y, text: item.status.word().to_string(), tone: Tone::Status(item.status) });
        let words = wrap(&item.text, columns);
        let rows = words.len().max(1);
        for (k, w) in words.into_iter().enumerate() {
            lines.push(CardLine { x: text_x, y: y + k as f64 * cell.1, text: w, tone: Tone::Text });
        }
        y += rows as f64 * cell.1;
        if Some(item.id) == matched {
            lit = Some((block_top, y));
        }
        y += gap;
    }
    let h = y - gap + pad;
    let shift = (top + h - bottom).max(0.0).min(top);
    let at = top - shift;
    for l in &mut lines {
        l.y += at;
    }
    Card {
        rect: Rect { x: left, y: at, w: width, h },
        lines,
        matched: lit.map(|(t, b)| Rect { x: left + pad / 2.0, y: at + t - gap / 2.0, w: width - pad, h: b - t + gap }),
    }
}

/// Whether a second card `width` wide fits beside `first`, `gap` after it, with the window's right
/// edge `gap` past it at `right`
pub fn fits_beside(first: &Rect, width: f64, right: f64, gap: f64) -> bool {
    first.x + first.w + gap + width <= right - gap
}

/// Where the top of a second card `h` tall goes when it doesn't fit beside `first` (a Shift-drag's
/// two objectives cards in a narrow window, lbterm/LINKS.md): under it, `gap` apart, where that
/// ends by `bottom`; else over it, where that starts by `top`; else, in a window too short for
/// both, under it as far as it goes, moved up to end by `bottom`
pub fn stacked_top(first: &Rect, h: f64, (top, bottom): (f64, f64), gap: f64) -> f64 {
    let under = first.y + first.h + gap;
    let over = first.y - gap - h;
    if under + h <= bottom {
        under
    } else if over >= top {
        over
    } else {
        under.min(bottom - h).max(top)
    }
}

/// Where a link stands, in the words its line's card says them: a suggested one says how to make it
pub fn stands(link: Option<&Link>) -> String {
    let Some(l) = link else {
        return "Suggested: a click on the line makes it".into();
    };
    let why = |lead: &str| if l.note.is_empty() { lead.to_string() } else { format!("{lead}: {}", l.note) };
    match l.state {
        LinkState::Waiting => "Waiting".into(),
        LinkState::Sending => why("Done, telling it to carry on"),
        LinkState::Sent => "Done, told it to carry on".into(),
        LinkState::Stuck => why("Still blocked"),
        LinkState::Failed => why("Couldn't tell it"),
    }
}

/// What a line's card says (Lachlan's E3): who waits on whom, where it stands, what it waits for
/// and the objective of the tab waited on that it waits for, when that's more than the same words
pub struct LineSays<'a> {
    pub waiter: &'a str,
    pub on: &'a str,
    pub look: Look,
    /// Where it stands, in words: "Waiting", "Still blocked: why", ...
    pub stands: &'a str,
    pub what: &'a str,
    pub objective: Option<(u64, &'a str)>,
}

/// The card that opens resting on a link's line (Lachlan's E3), `width` wide with its left at
/// `left`, its middle level with `middle` as far as it fits between `top` and `bottom`
pub fn line_card(says: &LineSays, left: f64, middle: f64, (top, bottom): (f64, f64), width: f64, cell: (f64, f64), scale: f64) -> Card {
    let pad = PAD * scale;
    let columns = (((width - 2.0 * pad) / cell.0).floor() as usize).max(8);
    let x = left + pad;
    let mut lines = vec![];
    let mut y = pad;
    let mut push = |text: String, tone: Tone, y: &mut f64| {
        lines.push(CardLine { x, y: *y, text, tone });
        *y += cell.1;
    };
    push(elide(&format!("{} waits on {}", says.waiter, says.on), columns), Tone::Title, &mut y);
    for w in wrap(says.stands, columns) {
        push(w, Tone::Look(says.look), &mut y);
    }
    if !says.what.is_empty() {
        y += GAP * scale;
        push("waits for".into(), Tone::Heading, &mut y);
        for w in wrap(says.what, columns) {
            push(w, Tone::Text, &mut y);
        }
    }
    if let Some((id, text)) = says.objective.filter(|(_, t)| !t.is_empty() && *t != says.what) {
        y += GAP * scale;
        push(elide(&format!("{}'s objective {id}", says.on), columns), Tone::Heading, &mut y);
        for w in wrap(text, columns) {
            push(w, Tone::Text, &mut y);
        }
    }
    let h = y + pad;
    let at = (middle - h / 2.0).clamp(top, (bottom - h).max(top));
    for l in &mut lines {
        l.y += at;
    }
    Card { rect: Rect { x: left, y: at, w: width, h }, lines, matched: None }
}

/// How many of a connection's messages its card shows, the newest, and how many lines each may take
pub const CARD_MESSAGES: usize = 3;
pub const MESSAGE_LINES: usize = 4;
/// How many lines a connection's note may take on its card: the file sets no length on it, and an
/// error with its output would wrap on past the window
pub const NOTE_LINES: usize = 3;

/// `text` wrapped to `columns`, at most `lines` of it, the last ending in "…" where it's cut
fn clip(text: &str, columns: usize, lines: usize) -> Vec<String> {
    let mut all = wrap(text, columns);
    if all.len() > lines {
        all.truncate(lines);
        if let Some(last) = all.last_mut() {
            let mut cut: String = last.chars().take(columns.max(2) - 1).collect();
            cut.push('…');
            *last = cut;
        }
    }
    all
}

/// A message on a connection's card (Lachlan's E7): "Tab Wave to Scroll strip, 14:02: <text>",
/// without the time when it has none
pub fn message_line(from: &str, to: &str, clock: &str, text: &str) -> String {
    if clock.is_empty() {
        format!("{from} to {to}: {text}")
    } else {
        format!("{from} to {to}, {clock}: {text}")
    }
}

/// What a tab connection's line card says: its members, why it was suggested, why an introduction
/// couldn't be delivered, and the messages its members sent each other
pub struct ConnectionSays<'a> {
    /// Its members' labels, in the file's order
    pub members: &'a [String],
    /// `Look::ConnectionSuggested` or `Look::Connected`
    pub look: Look,
    pub why: &'a str,
    pub note: &'a str,
    /// Its messages, oldest first, each as [`message_line`] says it: the card shows the last
    /// [`CARD_MESSAGES`]
    pub messages: &'a [String],
}

/// The card that opens resting on a connection's line (Lachlan's ask, 2026-11-01), `width` wide
/// with its left at `left`, its middle level with `middle` as far as it fits between `top` and
/// `bottom`. Suggested: "Tab Wave and Scroll strip could be talking", the why, and "A click on the
/// line connects them" in the line's colour. Made: "Connected: Tab Wave, Scroll strip, InMail", the
/// why, the note in red when there is one, cut at [`NOTE_LINES`], and the last three messages, each
/// wrapped to the card and cut at [`MESSAGE_LINES`].
pub fn connection_card(says: &ConnectionSays, left: f64, middle: f64, (top, bottom): (f64, f64), width: f64, cell: (f64, f64), scale: f64) -> Card {
    let pad = PAD * scale;
    let columns = (((width - 2.0 * pad) / cell.0).floor() as usize).max(8);
    let x = left + pad;
    let mut lines = vec![];
    let mut y = pad;
    let mut push = |text: String, tone: Tone, y: &mut f64| {
        lines.push(CardLine { x, y: *y, text, tone });
        *y += cell.1;
    };
    let suggested = says.look == Look::ConnectionSuggested;
    let title = if suggested {
        format!("{} could be talking", names(says.members))
    } else {
        format!("Connected: {}", says.members.join(", "))
    };
    for w in wrap(&title, columns) {
        push(w, Tone::Title, &mut y);
    }
    for w in wrap(says.why, columns) {
        push(w, Tone::Text, &mut y);
    }
    if suggested {
        y += GAP * scale;
        push("A click on the line connects them".into(), Tone::Look(Look::ConnectionSuggested), &mut y);
    } else {
        if !says.note.is_empty() {
            y += GAP * scale;
            for w in clip(says.note, columns, NOTE_LINES) {
                push(w, Tone::Look(Look::Error), &mut y);
            }
        }
        for m in &says.messages[says.messages.len().saturating_sub(CARD_MESSAGES)..] {
            y += GAP * scale;
            for w in clip(m, columns, MESSAGE_LINES) {
                push(w, Tone::Text, &mut y);
            }
        }
    }
    let h = y + pad;
    let at = (middle - h / 2.0).clamp(top, (bottom - h).max(top));
    for l in &mut lines {
        l.y += at;
    }
    Card { rect: Rect { x: left, y: at, w: width, h }, lines, matched: None }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::objectives::{From, Objective};

    #[test]
    fn where_a_link_stands_in_words() {
        let link = |state, note: &str| Link {
            waiter: "a".into(),
            on: "b".into(),
            what: "the build".into(),
            objective: None,
            state,
            note: note.into(),
        };
        assert_eq!(stands(Some(&link(LinkState::Waiting, ""))), "Waiting");
        assert_eq!(
            stands(Some(&link(LinkState::Sending, "Lachlan marked it complete"))),
            "Done, telling it to carry on: Lachlan marked it complete"
        );
        assert_eq!(stands(Some(&link(LinkState::Sent, "told at 14:02"))), "Done, told it to carry on");
        assert_eq!(stands(Some(&link(LinkState::Stuck, "still no build"))), "Still blocked: still no build");
        assert_eq!(stands(Some(&link(LinkState::Failed, ""))), "Couldn't tell it");
        assert_eq!(stands(None), "Suggested: a click on the line makes it");
    }

    #[test]
    fn a_lines_card_says_who_waits_on_whom_where_it_stands_and_for_what() {
        let says = LineSays {
            waiter: "Scroll strip",
            on: "Tab Wave",
            look: Look::Error,
            stands: "Still blocked: The rail still has no lines",
            what: "LBTerm to land the links",
            objective: Some((2, "Land feat/tab-links in LBTerm")),
        };
        let card = line_card(&says, 80.0, 100.0, (10.0, 900.0), 400.0, (8.0, 16.0), 1.0);
        let texts: Vec<_> = card.lines.iter().map(|l| (l.text.as_str(), l.tone)).collect();
        assert_eq!(texts[0], ("Scroll strip waits on Tab Wave", Tone::Title));
        assert_eq!(texts[1], ("Still blocked: The rail still has no lines", Tone::Look(Look::Error)));
        assert!(texts.contains(&("waits for", Tone::Heading)) && texts.contains(&("LBTerm to land the links", Tone::Text)));
        assert!(texts.contains(&("Tab Wave's objective 2", Tone::Heading)));
        assert!((card.rect.y + card.rect.h / 2.0 - 100.0).abs() < 1.0, "level with the pointer");
        // The objective's words when they're what it waits for already: once
        let same = LineSays { what: "Land feat/tab-links in LBTerm", ..says };
        let card = line_card(&same, 0.0, 5.0, (10.0, 900.0), 320.0, (8.0, 16.0), 1.0);
        assert!(card.lines.iter().all(|l| !l.text.contains("objective")));
        assert_eq!(card.rect.y, 10.0, "kept in the room");
    }

    const A: &str = "aaaaaaaa-1111-4222-8333-444444444444";
    const B: &str = "bbbbbbbb-1111-4222-8333-444444444444";
    const C: &str = "cccccccc-1111-4222-8333-444444444444";
    const D: &str = "dddddddd-1111-4222-8333-444444444444";

    fn link(waiter: &str, on: &str, state: LinkState) -> Link {
        Link { waiter: waiter.into(), on: on.into(), what: "w".into(), objective: None, state, note: String::new() }
    }

    #[test]
    fn the_file_reads_what_it_can_and_leaves_out_the_rest() {
        let links = read(&format!(
            r#"{{ "updatedAt": 5, "links": [
                {{ "id": "x", "waiter": "{A}", "on": "{B}", "what": " the rail ", "objective": 2, "state": "waiting", "note": "" }},
                {{ "waiter": "{A}", "on": "{C}", "state": "bogus" }},
                {{ "waiter": "", "on": "{C}", "state": "sent" }}
              ], "waits": [
                {{ "session": "{C}", "what": "the heat", "on": "{D}", "objective": 1, "matchedAt": 9 }},
                {{ "session": "{D}", "what": "" }}
              ] }}"#
        ))
        .unwrap();
        assert_eq!(links.links, vec![Link { what: "the rail".into(), objective: Some(2), ..link(A, B, LinkState::Waiting) }]);
        assert_eq!(links.waits, vec![Wait { session: C.into(), what: "the heat".into(), on: Some(D.into()), objective: Some(1), dismissed: false }]);
        assert_eq!(read("\u{feff}{}"), Some(Links::default()));
        assert_eq!(read("[1]"), None);
        assert_eq!(read("not json"), None);
    }

    #[test]
    fn a_suggestion_is_a_wait_naming_a_session_with_no_link_made_or_dismissed() {
        let wait = |dismissed| Wait { session: A.into(), what: "x".into(), on: Some(B.into()), objective: Some(1), dismissed };
        let mut links = Links { links: vec![], waits: vec![wait(false)], connections: vec![] };
        assert!(links.suggestion(A).is_some());
        links.waits = vec![wait(true)];
        assert!(links.suggestion(A).is_none());
        links.waits = vec![wait(false)];
        links.links = vec![link(A, C, LinkState::Waiting)];
        assert!(links.suggestion(A).is_none(), "it waits by a link already");
        links.links.clear();
        links.waits[0].on = None;
        assert!(links.suggestion(A).is_none(), "the matcher found nothing");
    }

    #[test]
    fn a_row_shows_the_worst_of_its_links_and_a_closed_one_says_so() {
        let links = Links { links: vec![link(A, B, LinkState::Sent), link(A, C, LinkState::Waiting)], waits: vec![], connections: vec![] };
        let all_open = |_: &str| true;
        assert_eq!(row_link(&links, A, all_open).unwrap().standing, Standing::Waiting);
        assert_eq!(row_link(&links, A, |s| s != C).unwrap().standing, Standing::Closed);
        let stuck = Links { links: vec![link(A, B, LinkState::Stuck), link(A, C, LinkState::Waiting)], waits: vec![], connections: vec![] };
        assert_eq!(row_link(&stuck, A, all_open).unwrap().standing, Standing::Stuck);
        assert!(row_link(&links, B, all_open).is_none(), "waited on is no link of its own");
        assert!(Standing::Closed.error() && !Standing::Told.error());
    }

    #[test]
    fn lines_take_lanes_no_other_line_on_their_rows_has() {
        let rows = [Some(A), Some(B), None, Some(C), Some(D)];
        let links = Links {
            links: vec![link(A, D, LinkState::Waiting), link(B, C, LinkState::Sent), link(C, D, LinkState::Stuck)],
            waits: vec![],
            connections: vec![],
        };
        let l = lines(&rows, &links, 3);
        let find = |w: usize, o: usize| *l.iter().find(|x| x.waiter == w && x.on == o).unwrap();
        // The shortest nearest the pills; B-C and C-D meet on C's row, so they can't share
        assert_eq!(find(3, 4).lane, 0);
        assert_eq!(find(1, 3).lane, 1);
        assert_eq!(find(0, 4).lane, 2);
        assert_eq!((find(1, 3).look, find(3, 4).look, find(0, 4).look), (Look::Done, Look::Error, Look::Waiting));
        // Out of lanes: the last is shared
        assert_eq!(lines(&rows, &links, 2).iter().map(|x| x.lane).max(), Some(1));
        // A session not on this rail draws no line
        assert!(lines(&[Some(A), Some(C)], &Links { links: vec![link(A, B, LinkState::Waiting)], waits: vec![], connections: vec![] }, 3).is_empty());
    }

    #[test]
    fn a_suggested_link_is_a_line_of_its_own_look() {
        let links = Links {
            links: vec![],
            waits: vec![Wait { session: C.into(), what: "x".into(), on: Some(A.into()), objective: Some(1), dismissed: false }],
            connections: vec![],
        };
        let l = lines(&[Some(A), Some(B), Some(C)], &links, 3);
        assert_eq!(l, vec![Line { waiter: 2, on: 0, lane: 0, look: Look::Suggested }]);
    }

    #[test]
    fn each_row_draws_its_own_piece_of_a_line() {
        let l = [Line { waiter: 3, on: 1, lane: 1, look: Look::Waiting }];
        let piece = |row| pieces(&l, row);
        assert_eq!(piece(0), vec![]);
        assert_eq!(piece(1), vec![Piece { lane: 1, look: Look::Waiting, up: false, down: true, end: Some(End::On) }]);
        assert_eq!(piece(2), vec![Piece { lane: 1, look: Look::Waiting, up: true, down: true, end: None }]);
        assert_eq!(piece(3), vec![Piece { lane: 1, look: Look::Waiting, up: true, down: false, end: Some(End::Waiter) }]);
        assert_eq!(piece(4), vec![]);
    }

    // ---------- connections ----------

    const E: &str = "eeeeeeee-1111-4222-8333-444444444444";

    fn conn(id: &str, members: &[&str], state: ConnectionState) -> Connection {
        Connection {
            id: id.into(),
            members: members.iter().map(|m| m.to_string()).collect(),
            state,
            at: 0,
            state_at: 0,
            by: String::new(),
            why: String::new(),
            note: String::new(),
            messages: vec![],
        }
    }

    #[test]
    fn connections_read_from_the_file_beside_the_links_and_the_rest_left_out() {
        let long_why = "w".repeat(200);
        let long_text = "t".repeat(400);
        let messages: Vec<String> = (1..=7)
            .map(|k| format!(r#"{{ "from": "{A}", "to": "{B}", "at": {k}, "text": "{}" }}"#, if k == 7 { long_text.as_str() } else { "hi" }))
            .collect();
        let links = read(&format!(
            r#"{{ "links": [], "waits": [], "connections": [
                {{ "id": "c-1", "members": ["{A}", " {B} "], "state": "suggested", "at": 1793250000000, "stateAt": 1793250000001,
                   "by": "files", "why": "both write lbterm/render/thumbnails.rs", "note": "", "messages": [{m0}] }},
                {{ "id": "c-2", "members": ["{A}", "{B}", "{C}"], "state": "connected", "by": "lbterm", "why": "{long_why}",
                   "note": "Couldn't tell Tab Wave: its session isn't running", "messages": [{all}] }},
                {{ "id": "c-3", "members": ["{C}", "{D}"], "state": "declined", "by": "digest" }},
                {{ "id": "c-4", "members": ["{C}", "{D}"], "state": "bogus" }},
                {{ "id": "c-5", "members": ["{C}"], "state": "suggested" }},
                {{ "id": "c-6", "members": ["{C}", "{C}", ""], "state": "connected" }},
                {{ "id": "", "members": ["{C}", "{D}"], "state": "suggested" }},
                {{ "id": "c-8", "state": "suggested" }},
                {{ "id": "c-9", "members": "{C}", "state": "connected" }}
              ] }}"#,
            m0 = messages[0],
            all = messages.join(", "),
        ))
        .unwrap();
        let ids: Vec<_> = links.connections.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["c-1", "c-2", "c-3"], "unknown states, one member, no id and no members left out");
        let c1 = &links.connections[0];
        assert_eq!(c1.members, [A, B], "trimmed");
        assert_eq!((c1.state, c1.at, c1.state_at, c1.by.as_str()), (ConnectionState::Suggested, 1793250000000, 1793250000001, "files"));
        assert_eq!(c1.why, "both write lbterm/render/thumbnails.rs");
        assert!(c1.messages.is_empty(), "messages only on a connected one");
        let c2 = &links.connections[1];
        assert_eq!(c2.members.len(), 3);
        assert_eq!(c2.why.chars().count(), WHY_CHARS);
        assert!(c2.note.starts_with("Couldn't tell Tab Wave"));
        // The last five, oldest first, the words clipped
        assert_eq!(c2.messages.iter().map(|m| m.at).collect::<Vec<_>>(), [3, 4, 5, 6, 7]);
        assert_eq!(c2.messages[4].text.chars().count(), MESSAGE_CHARS);
        assert_eq!((c2.messages[0].from.as_str(), c2.messages[0].to.as_str()), (A, B));
        assert_eq!(links.connections[2].state, ConnectionState::Declined);
        // Declined ones are read but show nowhere
        assert_eq!(links.connections_of(C).map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c-2"]);
        assert_eq!(links.connections_of(A).count(), 2);
        assert_eq!(links.connection("c-3").map(|c| c.state), Some(ConnectionState::Declined));
        // A file of links alone has none
        assert!(read(r#"{ "links": [] }"#).unwrap().connections.is_empty());
    }

    #[test]
    fn connections_share_the_lanes_with_links() {
        let rows = [Some(A), Some(B), Some(C), Some(D)];
        let links = Links {
            links: vec![link(A, B, LinkState::Waiting)],
            waits: vec![],
            connections: vec![conn("c-1", &[A, C], ConnectionState::Suggested), conn("c-2", &[B, C], ConnectionState::Connected)],
        };
        let laid = lay_out(&rows, &links, 3);
        // A-B and B-C are one row long, A-C two: the link first nearest the pills, B-C beside it
        // since they meet on B's row, A-C past both
        assert_eq!(laid.lines, vec![Line { waiter: 0, on: 1, lane: 0, look: Look::Waiting }]);
        let lane = |k: usize| laid.joins.iter().find(|j| j.connection == k).unwrap().lane;
        assert_eq!((lane(1), lane(0)), (1, 2));
        assert_eq!(laid.joins.iter().find(|j| j.connection == 0).unwrap().look, Look::ConnectionSuggested);
        assert_eq!(laid.joins.iter().find(|j| j.connection == 1).unwrap().look, Look::Connected);
        // A link and a connection apart share lane 0
        let apart = Links { connections: vec![conn("c-3", &[C, D], ConnectionState::Connected)], ..links.clone() };
        let laid = lay_out(&rows, &apart, 3);
        assert_eq!((laid.lines[0].lane, laid.joins[0].lane), (0, 0));
        // With no connections the links' lanes are the old lines()
        let only = Links { connections: vec![], ..links.clone() };
        assert_eq!(lay_out(&rows, &only, 3).lines, lines(&rows, &only, 3));
        // Out of lanes, the last is shared
        let laid = lay_out(&rows, &links, 2);
        assert_eq!(laid.joins.iter().map(|j| j.lane).max(), Some(1));
    }

    #[test]
    fn a_connection_shorter_than_a_link_it_overlaps_leaves_the_link_its_lane() {
        // A waits on C two rows down, in lane 0. A suggestion between A and B, one row long, would
        // win the shortest-first rule: it takes the next lane, and the link keeps its own
        let rows = [Some(A), Some(B), Some(C), Some(D)];
        let without = Links { links: vec![link(A, C, LinkState::Waiting)], waits: vec![], connections: vec![] };
        let links = Links {
            connections: vec![
                conn("short", &[A, B], ConnectionState::Suggested),
                conn("shorter", &[C, D], ConnectionState::Connected),
                conn("long", &[A, D], ConnectionState::Connected),
            ],
            ..without.clone()
        };
        let laid = lay_out(&rows, &links, 3);
        assert_eq!(laid.lines, lines(&rows, &without, 3), "the links' lanes as with no connections");
        assert_eq!(laid.lines[0].lane, 0);
        let lane = |id: &str| laid.joins.iter().find(|j| links.connections[j.connection].id == id).unwrap().lane;
        // The connections take the lanes the link leaves, the shortest first
        assert_eq!((lane("short"), lane("shorter"), lane("long")), (1, 1, 2));
        // Whatever the connections, however many, the links' lanes don't move
        for n in 0..=links.connections.len() {
            let some = Links { connections: links.connections[..n].to_vec(), ..without.clone() };
            assert_eq!(lay_out(&rows, &some, 3).lines, lines(&rows, &without, 3), "with {n} connections");
            assert_eq!(lay_out(&rows, &some, 1).lines, lines(&rows, &without, 1), "with {n} connections in one lane");
        }
    }

    #[test]
    fn a_connection_spans_its_members_on_this_rail_and_a_declined_one_draws_nothing() {
        let rows = [Some(A), Some(E), Some(B), None, Some(C)];
        let links = Links {
            links: vec![],
            waits: vec![],
            connections: vec![
                conn("three", &[C, A, B], ConnectionState::Connected),
                conn("away", &[A, D], ConnectionState::Suggested),
                conn("no", &[A, B], ConnectionState::Declined),
            ],
        };
        let laid = lay_out(&rows, &links, 3);
        assert_eq!(laid.joins, vec![Join { connection: 0, rows: vec![0, 2, 4], lane: 0, look: Look::Connected }]);
        // A third member off this rail: the line covers the two on it
        let off = Links { connections: vec![conn("c", &[A, D, B], ConnectionState::Suggested)], ..Links::default() };
        assert_eq!(lay_out(&rows, &off, 3).joins[0].rows, vec![0, 2]);
        // One member on this rail: no line
        let one = Links { connections: vec![conn("c", &[A, D], ConnectionState::Connected)], ..Links::default() };
        assert!(lay_out(&rows, &one, 3).joins.is_empty());
    }

    #[test]
    fn the_conductors_badge_counts_the_suggestions_with_a_member_on_this_rail() {
        // A, B and C on the rail, a row with no session between; D and E elsewhere
        let rail = [Some(A.to_string()), None, Some(B.to_string()), Some(C.to_string())];
        let mut links = Links {
            links: vec![link(A, B, LinkState::Waiting)],
            waits: vec![],
            connections: vec![
                conn("both", &[A, B], ConnectionState::Suggested),
                conn("one", &[C, D], ConnectionState::Suggested),
                conn("none", &[D, E], ConnectionState::Suggested),
                conn("made", &[A, C], ConnectionState::Connected),
                conn("no", &[B, C], ConnectionState::Declined),
            ],
        };
        // Both members here or one: a suggestion with one here draws no line but counts
        assert_eq!(links.suggested_on(&rail), 2);
        assert!(lay_out(&[Some(A), None, Some(B), Some(C)], &links, 3).joins.iter().all(|j| j.connection != 1));
        assert_eq!(badge(links.suggested_on(&rail)).as_deref(), Some("2"));
        // The rail's sessions as the lanes take them count the same
        assert_eq!(links.suggested_on(&[Some(A), None, Some(B), Some(C)]), 2);
        // Accepted, the rail drawing it before the file says so: one fewer at once
        ConnectionChange::of(&Act::Accept { connection: "both".into() }, &links).apply(&mut links);
        assert_eq!(links.suggested_on(&rail), 1);
        // Declined, none: no badge
        ConnectionChange::of(&Act::Decline { connection: "one".into() }, &links).apply(&mut links);
        assert_eq!(links.suggested_on(&rail), 0);
        assert_eq!(badge(0), None, "no badge at 0");
        // A rail with no Claude tabs, or no rail
        let empty: [Option<String>; 2] = [None, None];
        assert_eq!(Links { connections: vec![conn("x", &[A, B], ConnectionState::Suggested)], ..Links::default() }.suggested_on(&empty), 0);
        assert_eq!(Links::default().suggested_on::<&str>(&[]), 0);
        assert_eq!(badge(12).as_deref(), Some("12"), "the count as it is");
    }

    #[test]
    fn the_outline_leaves_out_shift_while_a_tab_is_dragged_on_the_rail() {
        let shift = [false, true, false];
        // Shift held: green, as ever
        assert_eq!(outline_held(shift, false), shift);
        // A tab right-dragged with it held: the drag is cyan for it, the outline left white
        assert_eq!(outline_held(shift, true), [false; 3]);
        // The drag let go with Shift still held: green again
        assert_eq!(outline_held(shift, false), shift);
        // Ctrl and Alt still show while it drags; Shift alone goes
        assert_eq!(outline_held([true, true, true], true), [true, false, true]);
        assert_eq!(outline_held([true, false, false], true), [true, false, false]);
        assert_eq!(outline_held([false; 3], true), [false; 3]);
    }

    #[test]
    fn a_three_member_line_draws_a_mark_at_each_member_and_passes_the_rows_between() {
        let laid = Lanes {
            lines: vec![Line { waiter: 2, on: 1, lane: 0, look: Look::Waiting }],
            joins: vec![Join { connection: 0, rows: vec![0, 2, 4], lane: 1, look: Look::Connected }],
        };
        let joined = |row: usize| laid.pieces(row).into_iter().filter(|p| p.look == Look::Connected).collect::<Vec<_>>();
        let piece = |up, down, end| vec![Piece { lane: 1, look: Look::Connected, up, down, end }];
        assert_eq!(joined(0), piece(false, true, Some(End::Member)));
        assert_eq!(joined(1), piece(true, true, None), "passed by");
        assert_eq!(joined(2), piece(true, true, Some(End::Member)), "a member between its ends");
        assert_eq!(joined(3), piece(true, true, None));
        assert_eq!(joined(4), piece(true, false, Some(End::Member)));
        assert!(joined(5).is_empty());
        // A row with a link's end and a connection's draws both, the link's last, over it, the
        // connection's a pixel bigger
        let both = laid.pieces(2);
        assert_eq!((both.len(), both[0].end, both[1].end), (2, Some(End::Member), Some(End::Waiter)));
        assert_eq!(Lanes::rims(&both), [1, 0]);
        assert_eq!(Lanes::rims(&laid.pieces(0)), [0]);
        assert_eq!(Lanes::rims(&laid.pieces(1)), [0, 0], "passed by, and the link's mark as it was");
    }

    #[test]
    fn a_row_in_a_connection_made_and_a_suggestion_shows_both_marks() {
        // A is connected with B, a row down, and suggested with C, two rows down: the suggestion
        // in the file first or last, A's row draws its hollow mark first, a pixel bigger, and the
        // connection's solid mark over it, so A reads connected with the suggestion's rim round it
        for order in [[0, 1], [1, 0]] {
            let joins = [
                Join { connection: 0, rows: vec![0, 1], lane: 0, look: Look::Connected },
                Join { connection: 1, rows: vec![0, 2], lane: 1, look: Look::ConnectionSuggested },
            ];
            let laid = Lanes { lines: vec![], joins: order.iter().map(|&k| joins[k].clone()).collect() };
            let row = laid.pieces(0);
            let looks: Vec<Look> = row.iter().map(|p| p.look).collect();
            assert_eq!(looks, [Look::ConnectionSuggested, Look::Connected], "file order {order:?}");
            assert!(row.iter().all(|p| p.end == Some(End::Member)));
            assert_eq!(Lanes::rims(&row), [1, 0]);
            // B's row and C's have one mark each, at its own size
            assert_eq!(Lanes::rims(&laid.pieces(1)), [0, 0], "B's mark, and the suggestion passing by");
            assert_eq!(Lanes::rims(&laid.pieces(2)), [0]);
        }
        // With a link's end too: the suggestion's rim round the connection's round the link's mark
        let laid = Lanes {
            lines: vec![Line { waiter: 0, on: 3, lane: 2, look: Look::Waiting }],
            joins: vec![
                Join { connection: 0, rows: vec![0, 1], lane: 0, look: Look::Connected },
                Join { connection: 1, rows: vec![0, 2], lane: 1, look: Look::ConnectionSuggested },
            ],
        };
        let row = laid.pieces(0);
        assert_eq!(row.iter().map(|p| p.end).collect::<Vec<_>>(), [Some(End::Member), Some(End::Member), Some(End::Waiter)]);
        assert_eq!(Lanes::rims(&row), [2, 1, 0]);
    }

    #[test]
    fn a_connections_marks_are_hollow_while_suggested_and_solid_once_made() {
        assert!(Look::ConnectionSuggested.hollow(End::Member));
        assert!(!Look::Connected.hollow(End::Member));
        // A link's as they were: hollow at the tab that waits, solid at the tab waited on
        for look in [Look::Waiting, Look::Done, Look::Error, Look::Suggested] {
            assert!(look.hollow(End::Waiter) && !look.hollow(End::On));
        }
        assert_eq!(Look::of_connection(ConnectionState::Declined), None);
        assert_eq!(Look::ConnectionSuggested.name(), "connection-suggested");
    }

    #[test]
    fn dashes_carry_on_from_one_slot_to_the_next() {
        assert_eq!(dashes(10.0, 22.0, 3.0, 3.0), vec![(12.0, 15.0), (18.0, 21.0)]);
        assert_eq!(dashes(22.0, 30.0, 3.0, 3.0), vec![(24.0, 27.0)]);
        assert_eq!(dashes(13.0, 14.0, 3.0, 3.0), vec![(13.0, 14.0)]);
    }

    fn aims() -> Objectives {
        let o = |id, text: &str, status| Objective { id, text: text.into(), from: From::Prompt, status, evidence: String::new() };
        Objectives {
            current: 2,
            list: vec![
                o(1, "Build the tab wave", Status::Done),
                o(2, "Land feat/tab-links in LBTerm, with the rail drawing the lines", Status::Open),
                o(3, "Drop the old panel", Status::Dropped),
            ],
        }
    }

    #[test]
    fn the_card_of_the_tab_under_the_drag_lists_the_unfinished_first_and_lights_the_match() {
        let o = aims();
        let card = aims_card("Tab Wave", Some(&o), Some(2), 100.0, (50.0, 1000.0), 300.0, (8.0, 16.0), 1.0);
        let texts: Vec<_> = card.lines.iter().map(|l| (l.text.as_str(), l.tone)).collect();
        assert_eq!(texts[0], ("Tab Wave", Tone::Title));
        assert_eq!(texts[1], ("Open", Tone::Status(Status::Open)));
        assert!(texts.iter().all(|(t, _)| !t.contains("old panel")), "dropped ones aren't offered");
        let done_at = card.lines.iter().position(|l| l.text == "Done").unwrap();
        assert!(done_at > 1, "the finished after the unfinished");
        let lit = card.matched.unwrap();
        let open_line = card.lines.iter().find(|l| l.text == "Open").unwrap();
        assert!(lit.y <= open_line.y && lit.y + lit.h >= open_line.y + 16.0);
        assert!(lit.x > card.rect.x && lit.x + lit.w < card.rect.x + card.rect.w);
        assert_eq!(card.rect.y, 50.0);
        // Moved up to fit, no higher than the top
        let low = aims_card("Tab Wave", Some(&o), None, 100.0, (990.0, 1000.0), 300.0, (8.0, 16.0), 1.0);
        assert!((low.rect.y + low.rect.h - 1000.0).abs() < 0.01 && low.matched.is_none());
        let none = aims_card("Shell", None, None, 0.0, (0.0, 500.0), 300.0, (8.0, 16.0), 1.0);
        assert_eq!(none.lines[1].text, "no objectives yet");
    }

    #[test]
    fn a_second_card_that_doesnt_fit_beside_the_first_goes_under_it_or_over_it() {
        // The dragged tab's objectives card, 320 wide past a panel ending at 60
        let first = Rect { x: 64.0, y: 200.0, w: 320.0, h: 120.0 };
        assert!(fits_beside(&first, 320.0, 800.0, 4.0), "64 + 320 + 4 + 320 = 708, by 796");
        assert!(!fits_beside(&first, 320.0, 700.0, 4.0));
        assert!(fits_beside(&first, 320.0, 712.0, 4.0) && !fits_beside(&first, 320.0, 711.0, 4.0), "to the pixel");
        // Under it where there's room, gap apart: the two don't overlap
        let top = stacked_top(&first, 100.0, (4.0, 600.0), 4.0);
        assert_eq!(top, 324.0);
        // Over it where under runs out and over doesn't
        let low = Rect { y: 420.0, ..first };
        assert_eq!(stacked_top(&low, 100.0, (4.0, 600.0), 4.0), 316.0);
        // Neither: under, moved up to end by the bottom
        let tall = Rect { y: 50.0, h: 400.0, ..first };
        assert_eq!(stacked_top(&tall, 300.0, (4.0, 600.0), 4.0), 300.0);
        // A window shorter than the card: from its top
        assert_eq!(stacked_top(&tall, 900.0, (4.0, 600.0), 4.0), 4.0);
    }

    #[test]
    fn the_card_that_follows_the_drag_says_what_it_waits_for() {
        let card = waits_card("Scroll strip", Some("LBTerm to land the links on the rail"), (10.0, 20.0), 176.0, (8.0, 16.0), 1.0, true);
        let texts: Vec<_> = card.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Scroll strip", "waits for", "LBTerm to land the", "links on the rail"]);
        assert!(card.matched.is_some());
        let quiet = waits_card("Shell", None, (0.0, 0.0), 160.0, (8.0, 16.0), 1.0, true);
        assert_eq!((quiet.lines[1].tone, quiet.matched), (Tone::Quiet, None));
    }

    #[test]
    fn each_act_on_a_connection_names_what_the_event_says() {
        let acts = [
            (Act::Accept { connection: "c-1".into() }, "accept", vec![("connection", "c-1")]),
            (Act::Decline { connection: "c-1".into() }, "decline", vec![("connection", "c-1")]),
            (Act::Leave { connection: "c-2".into(), session: A.into() }, "leave", vec![("connection", "c-2"), ("session", A)]),
            (Act::Connect { waiter: A.into(), on: B.into() }, "connect", vec![("waiter", A), ("on", B)]),
        ];
        for (act, action, fields) in acts {
            assert_eq!(act.action(), action);
            let got: Vec<(&str, String)> = act.fields();
            assert_eq!(got, fields.iter().map(|(k, v)| (*k, v.to_string())).collect::<Vec<_>>(), "{action}");
        }
    }

    fn file(connections: Vec<Connection>) -> Links {
        Links { links: vec![], waits: vec![], connections }
    }

    fn members(links: &Links, id: &str) -> Option<(ConnectionState, Vec<String>)> {
        links.connection(id).map(|c| (c.state, c.members.clone()))
    }

    #[test]
    fn a_suggestion_accepted_shows_connected_and_one_declined_goes_until_the_file_says_so() {
        let said = file(vec![conn("c-1", &[A, B], ConnectionState::Suggested)]);
        let accept = ConnectionChange::of(&Act::Accept { connection: "c-1".into() }, &said);
        assert!(!accept.said(&said));
        let mut shown = said.clone();
        accept.apply(&mut shown);
        assert_eq!(members(&shown, "c-1"), Some((ConnectionState::Connected, vec![A.into(), B.into()])));
        assert_eq!(lay_out(&[Some(A), Some(B)], &shown, 3).joins[0].look, Look::Connected, "drawn steady at once");
        // The file connected it, or dropped it: said
        assert!(accept.said(&file(vec![conn("c-1", &[A, B], ConnectionState::Connected)])));
        assert!(accept.said(&file(vec![])));
        let decline = ConnectionChange::of(&Act::Decline { connection: "c-1".into() }, &said);
        let mut shown = said.clone();
        decline.apply(&mut shown);
        assert!(lay_out(&[Some(A), Some(B)], &shown, 3).joins.is_empty(), "declined: drawn nowhere");
        assert!(!decline.said(&said) && decline.said(&file(vec![conn("c-1", &[A, B], ConnectionState::Declined)])));
    }

    #[test]
    fn a_tab_leaving_shrinks_its_connection_or_ends_it() {
        let said = file(vec![conn("g", &[A, B, C], ConnectionState::Connected), conn("p", &[A, D], ConnectionState::Connected)]);
        let leave = |id: &str, s: &str| ConnectionChange::of(&Act::Leave { connection: id.into(), session: s.into() }, &said);
        let mut shown = said.clone();
        leave("g", B).apply(&mut shown);
        assert_eq!(members(&shown, "g"), Some((ConnectionState::Connected, vec![A.into(), C.into()])), "shrinks");
        leave("p", D).apply(&mut shown);
        assert_eq!(members(&shown, "p"), None, "one member left: gone");
        assert!(!leave("g", B).said(&said));
        assert!(leave("g", B).said(&file(vec![conn("g", &[A, C], ConnectionState::Connected)])));
    }

    #[test]
    fn a_shift_drag_connects_two_tabs_or_takes_the_dragged_one_out() {
        let connect = |w: &str, o: &str, said: &Links| ConnectionChange::of(&Act::Connect { waiter: w.into(), on: o.into() }, said);
        // Neither connected: a new one, drawn at once
        let none = file(vec![]);
        let change = connect(A, B, &none);
        assert_eq!(change, ConnectionChange::Joined { waiter: A.into(), on: B.into() });
        let mut shown = none.clone();
        change.apply(&mut shown);
        assert_eq!(members(&shown, PENDING_ID), Some((ConnectionState::Connected, vec![A.into(), B.into()])));
        assert!(!change.said(&none) && change.said(&file(vec![conn("c-9", &[B, A], ConnectionState::Connected)])));
        // Onto a tab in a connection made: the dragged one joins it (his E6), and the other way round
        let group = file(vec![conn("g", &[B, C], ConnectionState::Connected)]);
        let mut shown = group.clone();
        connect(A, B, &group).apply(&mut shown);
        assert_eq!(members(&shown, "g").unwrap().1, [B, C, A]);
        let mut shown = group.clone();
        connect(C, D, &group).apply(&mut shown);
        assert_eq!(members(&shown, "g").unwrap().1, [B, C, D]);
        // A suggestion between them is made, not drawn beside a new one
        let suggested = file(vec![conn("s", &[A, B], ConnectionState::Suggested)]);
        let mut shown = suggested.clone();
        connect(B, A, &suggested).apply(&mut shown);
        assert_eq!(shown.connections.len(), 1);
        assert_eq!(members(&shown, "s").unwrap().0, ConnectionState::Connected);
        // Joining a group drops the suggestion between the two
        let both = file(vec![conn("g", &[B, C], ConnectionState::Connected), conn("s", &[A, B], ConnectionState::Suggested)]);
        let mut shown = both.clone();
        connect(A, B, &both).apply(&mut shown);
        assert_eq!(shown.connections.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["g"]);
        // Already connected together: the dragged one out, the group shrinking or ending
        let three = file(vec![conn("g", &[A, B, C], ConnectionState::Connected)]);
        let change = connect(C, A, &three);
        assert_eq!(change, ConnectionChange::Parted { waiter: C.into(), on: A.into() });
        let mut shown = three.clone();
        change.apply(&mut shown);
        assert_eq!(members(&shown, "g").unwrap().1, [A, B]);
        let mut shown = file(vec![conn("p", &[A, B], ConnectionState::Connected)]);
        connect(A, B, &shown.clone()).apply(&mut shown);
        assert!(shown.connections.is_empty());
        assert!(!change.said(&three) && change.said(&file(vec![conn("g", &[A, B], ConnectionState::Connected)])));
        // Only a connection made counts as together: a suggestion isn't
        assert_eq!(connect(A, B, &suggested), ConnectionChange::Joined { waiter: A.into(), on: B.into() });
    }

    #[test]
    fn a_later_change_replaces_one_about_the_same_connection_or_the_same_two_tabs() {
        let accept = ConnectionChange::Accepted { connection: "c-1".into() };
        assert!(ConnectionChange::Declined { connection: "c-1".into() }.replaces(&accept));
        assert!(accept.replaces(&ConnectionChange::Declined { connection: "c-1".into() }));
        assert!(!ConnectionChange::Declined { connection: "c-2".into() }.replaces(&accept));
        let left = |id: &str, s: &str| ConnectionChange::Left { connection: id.into(), session: s.into() };
        assert!(left("c-1", A).replaces(&left("c-1", A)), "the same tab leaving the same connection again");
        assert!(!left("c-1", A).replaces(&accept), "a leave doesn't replace an accept");
        assert!(!accept.replaces(&left("c-1", A)));
        let joined = ConnectionChange::Joined { waiter: A.into(), on: B.into() };
        assert!(ConnectionChange::Parted { waiter: B.into(), on: A.into() }.replaces(&joined), "either way round");
        assert!(!ConnectionChange::Joined { waiter: A.into(), on: C.into() }.replaces(&joined));
        assert!(!accept.replaces(&joined) && !left("c-1", A).replaces(&joined));
    }

    #[test]
    fn two_tabs_leaving_one_connection_both_show_until_the_file_says_them() {
        // B leaves the group before the file has said A left: A's leaving still shows
        let said = file(vec![conn("g", &[A, B, C, D], ConnectionState::Connected)]);
        let a = ConnectionChange::of(&Act::Leave { connection: "g".into(), session: A.into() }, &said);
        let b = ConnectionChange::of(&Act::Leave { connection: "g".into(), session: B.into() }, &said);
        assert!(!b.replaces(&a) && !a.replaces(&b));
        // The rail keeps every change that isn't replaced, in order, as lbterm_links_pending does
        let mut pending: Vec<ConnectionChange> = vec![];
        for change in [a.clone(), b.clone()] {
            pending.retain(|was| !change.replaces(was));
            pending.push(change);
        }
        let mut shown = said.clone();
        for change in &pending {
            change.apply(&mut shown);
        }
        assert_eq!(members(&shown, "g"), Some((ConnectionState::Connected, vec![C.into(), D.into()])));
        // The file saying A's, B's still shows
        let half = file(vec![conn("g", &[B, C, D], ConnectionState::Connected)]);
        assert!(a.said(&half) && !b.said(&half));
    }

    #[test]
    fn the_tab_menu_offers_to_leave_each_connection_made_and_to_make_or_decline_each_suggested() {
        let links = file(vec![
            conn("g", &[A, B, C], ConnectionState::Connected),
            conn("s", &[D, A], ConnectionState::Suggested),
            conn("p", &[A, E], ConnectionState::Connected),
            conn("no", &[A, C], ConnectionState::Declined),
        ]);
        let name = |s: &str| match s {
            A => "Scroll strip",
            B => "Tab Wave",
            C => "InMail",
            D => "Usage chart",
            _ => "Heat steps",
        }
        .to_string();
        let rows = connection_rows(&links, A, name);
        let labels: Vec<_> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Leave the connection with Tab Wave and InMail",
                "Leave the connection with Heat steps",
                "Connect with Usage chart, as suggested",
                "Decline the suggested connection with Usage chart",
            ]
        );
        assert_eq!(rows[0].1, Act::Leave { connection: "g".into(), session: A.into() });
        assert_eq!(rows[2].1, Act::Accept { connection: "s".into() });
        assert_eq!(rows[3].1, Act::Decline { connection: "s".into() });
        // A tab in none: no rows; three others listed with commas
        assert!(connection_rows(&links, "nobody", name).is_empty());
        let four = file(vec![conn("big", &[E, A, B, C], ConnectionState::Connected)]);
        assert_eq!(connection_rows(&four, E, name)[0].0, "Leave the connection with Scroll strip, Tab Wave and InMail");
        assert_eq!(names(&[]), "");
    }

    #[test]
    fn a_connection_the_rail_draws_before_the_file_has_it_has_no_menu_rows() {
        // A Shift-drag between two tabs in no connection: drawn at once under PENDING_ID, which
        // the program never made, so neither tab's menu offers to leave it until the file says it
        let said = file(vec![conn("s", &[A, C], ConnectionState::Suggested)]);
        let mut shown = said.clone();
        ConnectionChange::of(&Act::Connect { waiter: A.into(), on: B.into() }, &said).apply(&mut shown);
        assert_eq!(members(&shown, PENDING_ID), Some((ConnectionState::Connected, vec![A.into(), B.into()])));
        assert!(!lay_out(&[Some(A), Some(B)], &shown, 3).joins.is_empty(), "drawn");
        let name = |s: &str| s[..1].to_string();
        let rows = connection_rows(&shown, A, name);
        assert!(rows.iter().all(|(_, act)| !act.fields().iter().any(|(_, v)| v == PENDING_ID)), "{rows:?}");
        assert_eq!(rows.len(), 2, "the suggestion's two rows only");
        assert!(connection_rows(&shown, B, name).is_empty());
        assert!(shown.made_of(A).all(|c| c.id != PENDING_ID));
        // The file saying it, under the program's own id: Leave it
        let made = file(vec![conn("c-7", &[A, B], ConnectionState::Connected), conn("s", &[A, C], ConnectionState::Suggested)]);
        assert_eq!(connection_rows(&made, B, name)[0].1, Act::Leave { connection: "c-7".into(), session: B.into() });
        assert_eq!(made.made_of(B).map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c-7"]);
    }

    #[test]
    fn a_point_is_on_a_groups_lane_between_its_ends_or_across_to_a_members_mark() {
        // A group on rows at y 100, 160 and 220, its lane at x 10, the marks' middles at x 24
        let ends = [(24.0, 100.0), (24.0, 160.0), (24.0, 220.0)];
        let on = |x: f32, y: f32| on_line((x, y), 10.0, &ends, 2.5, 3.0, 1.0);
        assert!(on(10.0, 130.0) && on(12.0, 190.0), "down its lane, passing the row between");
        assert!(on(10.0, 98.0) && on(10.0, 222.0), "a little past its ends");
        assert!(!on(10.0, 90.0) && !on(10.0, 230.0), "not past its ends");
        assert!(!on(14.0, 130.0), "beside its lane");
        assert!(on(20.0, 160.0) && on(27.5, 161.0), "across to a member's mark between its ends");
        assert!(!on(20.0, 130.0), "no across where a row is passed by");
        assert!(!on(29.0, 160.0), "past the mark");
        // A link's two ends are the same rule
        assert!(on_line((24.0, 50.0), 10.0, &[(24.0, 50.0), (24.0, 80.0)], 2.5, 3.0, 1.0));
        assert!(!on_line((10.0, 50.0), 10.0, &[], 2.5, 3.0, 1.0));
    }

    #[test]
    fn a_point_between_two_lanes_is_taken_to_the_nearer_line() {
        // As on the rail at 96 DPI: lanes 3 px apart, 3 px of slack either side of each. A group
        // down lane 8 from y 96 to 396, passing row 171, where a pair down lane 11 has a mark
        let (slack, half, px) = (3.0, 3.0, 1.0);
        let group = [(20.0, 96.0), (20.0, 246.0), (20.0, 396.0)];
        let pair = [(20.0, 171.0), (20.0, 321.0)];
        let d = |at: (f32, f32), lane: f32, ends: &[(f32, f32)]| line_distance(at, lane, ends, slack, half, px);
        // On the group's lane, level with the pair's mark: on both, the group's nearer
        assert_eq!(d((8.0, 171.0), 8.0, &group), Some(0.0));
        assert_eq!(d((8.0, 171.0), 11.0, &pair), Some(3.0), "3 px short of where the pair's piece across starts");
        // On the pair's piece across to its mark, the pair's
        assert_eq!(d((15.0, 172.0), 11.0, &pair), Some(1.0));
        assert_eq!(d((15.0, 172.0), 8.0, &group), None);
        // Past a line's end, the distance is to the end
        assert_eq!(d((8.0, 94.0), 8.0, &group), Some(2.0));
        assert_eq!(d((8.0, 90.0), 8.0, &group), None);
    }

    #[test]
    fn a_suggested_connections_card_says_who_could_be_talking_why_and_how_to_connect_them() {
        let members = ["Tab Wave".to_string(), "Scroll strip".to_string()];
        let says = ConnectionSays {
            members: &members,
            look: Look::ConnectionSuggested,
            why: "both write lbterm/render/thumbnails.rs",
            note: "",
            messages: &[],
        };
        let card = connection_card(&says, 80.0, 200.0, (10.0, 900.0), 360.0, (8.0, 16.0), 1.0);
        let texts: Vec<_> = card.lines.iter().map(|l| (l.text.as_str(), l.tone)).collect();
        assert_eq!(
            texts,
            [
                ("Tab Wave and Scroll strip could be talking", Tone::Title),
                ("both write lbterm/render/thumbnails.rs", Tone::Text),
                ("A click on the line connects them", Tone::Look(Look::ConnectionSuggested)),
            ]
        );
        assert!((card.rect.y + card.rect.h / 2.0 - 200.0).abs() < 1.0, "level with the pointer");
        assert_eq!((card.rect.x, card.rect.w), (80.0, 360.0));
        // Narrower, the names wrap rather than being cut
        let narrow = connection_card(&says, 0.0, 200.0, (10.0, 900.0), 300.0, (8.0, 16.0), 1.0);
        assert_eq!((narrow.lines[0].text.as_str(), narrow.lines[1].text.as_str()), ("Tab Wave and Scroll strip could be", "talking"));
    }

    #[test]
    fn a_connections_card_lists_its_members_the_note_in_red_and_the_last_three_messages_cut_to_fit() {
        let members = ["Tab Wave".to_string(), "Scroll strip".to_string(), "InMail".to_string()];
        let messages: Vec<String> = (1..=5)
            .map(|k| message_line("Tab Wave", "Scroll strip", &format!("14:0{k}"), &format!("message {k}")))
            .chain([message_line("Scroll strip", "Tab Wave", "", &"word ".repeat(40))])
            .collect();
        let says = ConnectionSays {
            members: &members,
            look: Look::Connected,
            why: "all three draw on the rail",
            note: "Couldn't tell Tab Wave: its session isn't running",
            messages: &messages,
        };
        let card = connection_card(&says, 0.0, 300.0, (10.0, 900.0), 360.0, (8.0, 16.0), 1.0);
        let texts: Vec<_> = card.lines.iter().map(|l| (l.text.as_str(), l.tone)).collect();
        assert_eq!(texts[0], ("Connected: Tab Wave, Scroll strip, InMail", Tone::Title));
        assert_eq!(texts[1], ("all three draw on the rail", Tone::Text));
        // The note in red, wrapped
        let red: Vec<_> = texts.iter().filter(|(_, tone)| *tone == Tone::Look(Look::Error)).map(|(t, _)| *t).collect();
        assert_eq!(red.join(" "), "Couldn't tell Tab Wave: its session isn't running");
        assert_eq!(texts[2].1, Tone::Look(Look::Error), "after the why");
        let rest: Vec<_> = texts[2 + red.len()..].iter().map(|(t, _)| *t).collect();
        // The last three, oldest first: the fourth and fifth, then the long one cut to four lines
        assert_eq!(rest[0], "Tab Wave to Scroll strip, 14:04: message 4");
        assert_eq!(rest[1], "Tab Wave to Scroll strip, 14:05: message 5");
        assert_eq!(rest.len(), 2 + MESSAGE_LINES);
        assert!(rest[2].starts_with("Scroll strip to Tab Wave: word"));
        assert!(rest.last().unwrap().ends_with('…') && rest.iter().all(|t| t.chars().count() <= 42));
        assert!(texts.iter().all(|(t, _)| !t.contains("message 3")));
        // A connection made with nothing to add says who's in it
        let bare = ConnectionSays { why: "", note: "", messages: &[], ..says };
        let card = connection_card(&bare, 0.0, 0.0, (10.0, 900.0), 360.0, (8.0, 16.0), 1.0);
        assert_eq!(card.lines.len(), 1);
        assert_eq!(card.rect.y, 10.0, "kept in the room");
        // A note the file gives with no end, an error and its output, cut at three lines
        let error = format!("Couldn't tell Tab Wave: {}", "SendMessage failed: the session isn't running. ".repeat(40));
        let long = ConnectionSays { note: &error, messages: &[], ..says };
        let card = connection_card(&long, 0.0, 300.0, (10.0, 900.0), 360.0, (8.0, 16.0), 1.0);
        let red: Vec<_> = card.lines.iter().filter(|l| l.tone == Tone::Look(Look::Error)).map(|l| l.text.as_str()).collect();
        assert_eq!(red.len(), NOTE_LINES, "{red:?}");
        assert!(red[0].starts_with("Couldn't tell Tab Wave:") && red[NOTE_LINES - 1].ends_with('…'));
        assert!(card.rect.h <= (2 + NOTE_LINES) as f64 * 16.0 + 2.0 * PAD + GAP, "{}", card.rect.h);
    }

    #[test]
    fn the_light_goes_there_and_back_along_its_line_starting_at_the_middle() {
        let (lap, path) = glow_path(20.0, (100.0, 240.0), (6, 18), 1.0);
        assert_eq!(path.len(), STEPS);
        // Its visual's offset puts the light's middle on the line
        assert!(path.iter().all(|p| p.0 == 17.0));
        let ys: Vec<f32> = path.iter().map(|p| p.1 + 9.0).collect();
        assert_eq!(ys[0], 170.0, "the middle, where it sits with motion off");
        let (lo, hi) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        assert_eq!((lo, hi), (100.0, 240.0), "from end to end");
        assert!(ys[STEPS / 4] > ys[0] && ys[3 * STEPS / 4] < ys[0], "down first, then up past the middle");
        // 140 px at 70 a second: two seconds a run, there and back
        assert!((lap - 4.0).abs() < 1e-9);
        // A short line still takes its time; at 200% DPI the same line is twice as many pixels
        assert!((glow_path(0.0, (0.0, 20.0), (6, 18), 1.0).0 - 2.0 * RUN_MIN).abs() < 1e-9);
        assert!((glow_path(0.0, (0.0, 280.0), (12, 36), 2.0).0 - 4.0).abs() < 1e-9);
    }
}
