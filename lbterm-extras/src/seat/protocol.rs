//! The seat's pipe (Conductor's SEAT.md, "The pipe"): JSON, one object a line. The seat sends
//! `seat.view` (all of what the view shows besides the conversation, every time), `seat.items`
//! (the conversation, replaced by id), `seat.delta` (the reply being written), `seat.show` and
//! `seat.reply`; LBTerm sends commands, each with an id of its own and `by: "lbterm"`.
//!
//! Everything is read leniently: a field missing or of another type reads as empty, so a seat
//! newer than this build still shows what this build knows.

use serde_json::{json, Value};

/// Who LBTerm says it is, in every command's `by` and `seat.say`'s `from`
pub const BY: &str = "lbterm";

/// The conversation kept: the seat sends the last 200 on subscribe
pub const KEEP: usize = 400;

fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn some_text(v: &Value, key: &str) -> Option<String> {
    Some(text(v, key)).filter(|s| !s.trim().is_empty())
}

fn number(v: &Value, key: &str) -> Option<u64> {
    let n = v.get(key)?;
    n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
}

fn percent(v: &Value, key: &str) -> Option<u8> {
    number(v, key).map(|p| p.min(100) as u8)
}

fn seconds(v: &Value, key: &str) -> Option<u32> {
    number(v, key).map(|s| s.min(u32::MAX as u64) as u32)
}

fn flag(v: &Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// The Conductor itself, as the view's `conductor` says
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Conductor {
    /// `off`, `starting`, `on`, `stopping` or `error`
    pub state: String,
    /// `read`, `most` or `all`
    pub mode: String,
    pub listening: bool,
    pub paused_at: Option<u64>,
    pub catch_up: bool,
    /// A turn is under way
    pub busy: bool,
    /// His messages waiting for it to end
    pub queued: u32,
    pub session_id: Option<String>,
    pub started_at: Option<u64>,
    pub model: Option<String>,
    pub error: Option<String>,
}

impl Conductor {
    fn read(v: &Value) -> Self {
        Self {
            state: some_text(v, "state").unwrap_or_else(|| "off".to_string()),
            mode: some_text(v, "mode").unwrap_or_else(|| "read".to_string()),
            listening: flag(v, "listening", true),
            paused_at: number(v, "pausedAt"),
            catch_up: flag(v, "catchUp", false),
            busy: flag(v, "busy", false),
            queued: number(v, "queued").unwrap_or(0).min(u32::MAX as u64) as u32,
            session_id: some_text(v, "sessionId"),
            started_at: number(v, "startedAt"),
            model: some_text(v, "model"),
            error: some_text(v, "error"),
        }
    }

    /// Whether it takes his words: on, or coming on (the seat queues them)
    pub fn takes_words(&self) -> bool {
        matches!(self.state.as_str(), "on" | "starting")
    }
}

/// Something the Conductor wants to do that waits on his Allow or Deny
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ask {
    pub id: String,
    pub at: u64,
    pub tool: String,
    /// One line
    pub title: String,
    /// The exact thing it would do
    pub detail: String,
    /// The messaging name, for agents: never shown (Conductor's SEAT.md, "Names")
    pub session: Option<String>,
    /// What he calls the session, none before its first step
    pub label: Option<String>,
    pub project: Option<String>,
}

/// A subagent's or a background command's bar, as the hub guesses it
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Agent {
    pub id: String,
    /// `agent` or `shell`
    pub kind: String,
    pub what: String,
    pub percent: Option<u8>,
    pub left: Option<u32>,
}

/// Another Claude session
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub session_id: String,
    /// The messaging name, for agents: never shown (Conductor's SEAT.md, "Names")
    pub name: String,
    pub project: String,
    /// `working`, `waiting`, `idle`, `blocked`, `failed`, `done`, `stopped`, `leftover` or
    /// `unknown`
    pub status: String,
    /// What he calls it, empty before its first step
    pub label: String,
    pub summary: String,
    pub question: Option<String>,
    /// Its LBTerm mux pane, when known
    pub pane: Option<String>,
    pub percent: Option<u8>,
    /// Seconds
    pub left: Option<u32>,
    pub agents: Vec<Agent>,
    /// `thinking` or `acting` while the Conductor is on it
    pub conductor: Option<String>,
    /// Lachlan hid it from the Conductor (`conductor hide`; Conductor's SEAT.md): the view lists
    /// it in the folded Hidden group at the foot of Sessions, with a Show button
    /// (lbterm/NEW-TAB.md, section 8). False when the seat doesn't say, as a seat from before
    /// 2026-11-02 doesn't
    pub hidden: bool,
}

/// One option of a picker's question
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Choice {
    pub label: String,
    pub description: String,
}

/// One of a picker's questions
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub multi_select: bool,
    pub options: Vec<Choice>,
}

/// A question a session waits on him for: a picker (`questions`) or a question in text (`text`)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Waiting {
    pub id: String,
    pub session_id: String,
    /// The messaging name, for agents: never shown
    pub session: String,
    /// What he calls the session, none before its first step
    pub label: Option<String>,
    pub project: String,
    pub kind: String,
    /// Its only question is the extras it offers
    pub extras: bool,
    pub gist: String,
    pub at: u64,
    pub questions: Vec<Question>,
    pub text: Option<String>,
    /// False when no LBTerm pane is known to type into, and `why`
    pub answerable: bool,
    pub why: Option<String>,
}

impl Waiting {
    /// Whether one click answers it: a picker of one question, not multi-select
    pub fn one_click(&self) -> bool {
        self.questions.len() == 1 && !self.questions[0].multi_select
    }
}

/// One of the hub's events
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Event {
    pub seq: u64,
    pub at: u64,
    pub kind: String,
    /// The messaging name, for agents: never shown
    pub session: String,
    /// What he calls the session, none before its first step
    pub label: Option<String>,
    pub project: String,
    pub text: String,
    /// Whether it would wake the Conductor
    pub wake: bool,
}

/// One of the RSI report's suggestions
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Suggestion {
    pub id: String,
    pub day: String,
    pub project: String,
    pub kind: String,
    pub title: String,
    pub change: String,
    pub evidence: String,
    pub saves: String,
    /// `open`, `sent` or `dismissed`
    pub status: String,
    /// `useful` or `not`
    pub rating: Option<String>,
}

/// What the Conductor told him with `notify`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Note {
    pub id: String,
    pub at: u64,
    pub text: String,
}

/// A part of the view the seat couldn't read (`seat.view`'s `failing`, the Conductor's InMail of
/// 2026-11-09, after its read of who's waiting failed from 19:47 to 21:02 that day and both views
/// showed nobody waiting): `part` is `sessions` (its list stays as last read), `waiting` or
/// `suggestions` (its list is empty), `error` what went wrong, `since` when it began (unix ms)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Failing {
    pub part: String,
    pub error: String,
    pub since: u64,
}

/// `seat.view`: all of it, every time
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    pub seq: u64,
    pub at: u64,
    pub conductor: Conductor,
    pub asks: Vec<Ask>,
    pub sessions: Vec<Session>,
    pub waiting: Vec<Waiting>,
    pub events: Vec<Event>,
    pub suggestions: Vec<Suggestion>,
    pub notes: Vec<Note>,
    /// The RSI report is being made now (`rsi.running`, after `seat.rsi` `run`)
    pub rsi_running: bool,
    /// The parts the seat couldn't read; empty when it read everything
    pub failing: Vec<Failing>,
}

impl View {
    /// What went wrong reading `part` (`sessions`, `waiting` or `suggestions`), if it failed
    pub fn failed(&self, part: &str) -> Option<&Failing> {
        self.failing.iter().find(|f| f.part == part)
    }

    fn read(v: &Value) -> Self {
        let empty = Value::Null;
        Self {
            failing: list(v, "failing")
                .iter()
                .filter_map(|f| {
                    Some(Failing {
                        part: some_text(f, "part")?,
                        error: text(f, "error"),
                        since: number(f, "since").unwrap_or(0),
                    })
                })
                .collect(),
            rsi_running: v.get("rsi").map_or(false, |r| flag(r, "running", false)),
            seq: number(v, "seq").unwrap_or(0),
            at: number(v, "at").unwrap_or(0),
            conductor: Conductor::read(v.get("conductor").unwrap_or(&empty)),
            asks: list(v, "asks")
                .iter()
                .filter_map(|a| {
                    Some(Ask {
                        id: some_text(a, "id")?,
                        at: number(a, "at").unwrap_or(0),
                        tool: text(a, "tool"),
                        title: text(a, "title"),
                        detail: text(a, "detail"),
                        session: some_text(a, "session"),
                        label: some_text(a, "label"),
                        project: some_text(a, "project"),
                    })
                })
                .collect(),
            sessions: list(v, "sessions")
                .iter()
                .filter_map(|s| {
                    Some(Session {
                        session_id: some_text(s, "sessionId")?,
                        name: text(s, "name"),
                        project: text(s, "project"),
                        status: some_text(s, "status").unwrap_or_else(|| "unknown".to_string()),
                        label: text(s, "label"),
                        summary: text(s, "summary"),
                        question: some_text(s, "question"),
                        pane: some_text(s, "pane"),
                        percent: percent(s, "percent"),
                        left: seconds(s, "left"),
                        agents: list(s, "agents")
                            .iter()
                            .map(|a| Agent {
                                id: text(a, "id"),
                                kind: text(a, "kind"),
                                what: text(a, "what"),
                                percent: percent(a, "percent"),
                                left: seconds(a, "left"),
                            })
                            .collect(),
                        conductor: some_text(s, "conductor"),
                        hidden: flag(s, "hidden", false),
                    })
                })
                .collect(),
            waiting: list(v, "waiting")
                .iter()
                .filter_map(|w| {
                    Some(Waiting {
                        id: some_text(w, "id")?,
                        session_id: text(w, "sessionId"),
                        session: text(w, "session"),
                        label: some_text(w, "label"),
                        project: text(w, "project"),
                        kind: text(w, "kind"),
                        extras: flag(w, "extras", false),
                        gist: text(w, "gist"),
                        at: number(w, "at").unwrap_or(0),
                        questions: list(w, "questions")
                            .iter()
                            .map(|q| Question {
                                question: text(q, "question"),
                                header: text(q, "header"),
                                multi_select: flag(q, "multiSelect", false),
                                options: list(q, "options")
                                    .iter()
                                    .map(|o| Choice {
                                        label: text(o, "label"),
                                        description: text(o, "description"),
                                    })
                                    .collect(),
                            })
                            .collect(),
                        text: some_text(w, "text"),
                        answerable: flag(w, "answerable", true),
                        why: some_text(w, "why"),
                    })
                })
                .collect(),
            events: list(v, "events")
                .iter()
                .map(|e| Event {
                    seq: number(e, "seq").unwrap_or(0),
                    at: number(e, "at").unwrap_or(0),
                    kind: text(e, "kind"),
                    session: text(e, "session"),
                    label: some_text(e, "label"),
                    project: text(e, "project"),
                    text: text(e, "text"),
                    wake: flag(e, "wake", false),
                })
                .collect(),
            suggestions: list(v, "suggestions")
                .iter()
                .filter_map(|s| {
                    Some(Suggestion {
                        id: some_text(s, "id")?,
                        day: text(s, "day"),
                        project: text(s, "project"),
                        kind: text(s, "kind"),
                        title: text(s, "title"),
                        change: text(s, "change"),
                        evidence: text(s, "evidence"),
                        saves: text(s, "saves"),
                        status: some_text(s, "status").unwrap_or_else(|| "open".to_string()),
                        rating: some_text(s, "rating"),
                    })
                })
                .collect(),
            notes: list(v, "notes")
                .iter()
                .map(|n| Note {
                    id: text(n, "id"),
                    at: number(n, "at").unwrap_or(0),
                    text: text(n, "text"),
                })
                .collect(),
        }
    }
}

/// Who an item of the conversation is from
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Who {
    /// Lachlan's words
    You,
    /// What the Conductor said
    Conductor,
    /// One of its tool calls
    Tool,
    /// A wake: what woke it
    Hub,
    /// A message from another session
    Peer,
    /// The seat's own lines
    Notice,
    /// One this build doesn't know, shown as a notice
    Other(String),
}

impl Who {
    fn read(s: &str) -> Self {
        match s {
            "you" => Who::You,
            "conductor" => Who::Conductor,
            "tool" => Who::Tool,
            "hub" => Who::Hub,
            "peer" => Who::Peer,
            "notice" => Who::Notice,
            other => Who::Other(other.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Who::You => "you",
            Who::Conductor => "conductor",
            Who::Tool => "tool",
            Who::Hub => "hub",
            Who::Peer => "peer",
            Who::Notice => "notice",
            Who::Other(s) => s,
        }
    }
}

/// A tool call's one line
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ToolCall {
    pub name: String,
    pub text: String,
    /// `running`, `done`, `error` or `denied`
    pub status: String,
}

/// One item of the conversation
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub at: u64,
    pub who: Who,
    /// Where his words were typed (`lbterm`, `phone`, `cli`), or a peer's messaging name (never drawn)
    pub from: Option<String>,
    /// A peer's sending session as he knows it: its label (none before its first step) and project
    pub label: Option<String>,
    pub project: Option<String>,
    pub text: String,
    /// His words waiting for the turn under way
    pub queued: bool,
    pub tool: Option<ToolCall>,
}

impl Item {
    fn read(v: &Value) -> Option<Self> {
        let tool = v.get("tool").filter(|t| t.is_object()).map(|t| ToolCall {
            name: text(t, "name"),
            text: text(t, "text"),
            status: some_text(t, "status").unwrap_or_else(|| "done".to_string()),
        });
        Some(Self {
            id: some_text(v, "id")?,
            at: number(v, "at").unwrap_or(0),
            who: Who::read(&text(v, "who")),
            from: some_text(v, "from"),
            label: some_text(v, "label"),
            project: some_text(v, "project"),
            text: text(v, "text"),
            queued: flag(v, "queued", false),
            tool,
        })
    }
}

/// `seat.reply`: the answer to one command, in words for him
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
    pub id: String,
    pub ok: bool,
    pub text: Option<String>,
    pub error: Option<String>,
}

impl Reply {
    /// What to tell him: the error, or the reply's words
    pub fn words(&self) -> Option<&str> {
        if self.ok {
            self.text.as_deref()
        } else {
            Some(self.error.as_deref().or(self.text.as_deref()).unwrap_or("The seat refused it"))
        }
    }
}

/// A line from the seat
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    View(Box<View>),
    Items { reset: bool, items: Vec<Item> },
    Delta { id: String, text: String },
    /// Lachlan asked to see the Conductor
    Show,
    Reply(Reply),
}

/// A line from the seat, or nothing for a line that isn't one of these
pub fn parse(line: &str) -> Option<Incoming> {
    let v: Value = serde_json::from_str(line.trim().trim_start_matches('\u{feff}')).ok()?;
    match v.get("type")?.as_str()? {
        "seat.view" => Some(Incoming::View(Box::new(View::read(&v)))),
        "seat.items" => Some(Incoming::Items {
            reset: flag(&v, "reset", false),
            items: list(&v, "items").iter().filter_map(Item::read).collect(),
        }),
        "seat.delta" => Some(Incoming::Delta {
            id: some_text(&v, "id")?,
            text: text(&v, "text"),
        }),
        "seat.show" => Some(Incoming::Show),
        "seat.reply" => Some(Incoming::Reply(Reply {
            id: text(&v, "id"),
            ok: flag(&v, "ok", false),
            text: some_text(&v, "text"),
            error: some_text(&v, "error"),
        })),
        _ => None,
    }
}

/// One item as the view shows it: the item, or a reply still being written that has no item yet
#[derive(Debug, Clone, PartialEq)]
pub struct Shown<'a> {
    pub id: &'a str,
    pub at: u64,
    pub who: &'a Who,
    pub from: Option<&'a str>,
    pub label: Option<&'a str>,
    pub project: Option<&'a str>,
    pub text: &'a str,
    pub queued: bool,
    pub tool: Option<&'a ToolCall>,
    /// Still being written: the reply so far
    pub streaming: bool,
}

/// The conversation with the Conductor, as the seat has sent it
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    items: Vec<Item>,
    /// Replies being written, by item id, oldest first
    streaming: Vec<(String, String)>,
    /// Goes up with every change
    pub generation: u64,
}

const CONDUCTOR: Who = Who::Conductor;

impl Conversation {
    /// `seat.items`: on `reset`, these are all of it; otherwise each replaces the item of its id
    /// where it is, or comes after the rest. One of his messages that was queued and now isn't
    /// has started: it moves after everything said so far, where its turn's reply will follow.
    pub fn apply(&mut self, reset: bool, items: Vec<Item>) {
        if reset {
            self.items.clear();
            self.streaming.clear();
        }
        for item in items {
            self.streaming.retain(|(id, _)| *id != item.id);
            match self.items.iter().position(|i| i.id == item.id) {
                Some(at) if self.items[at].queued && !item.queued => {
                    self.items.remove(at);
                    self.items.push(item);
                }
                Some(at) => self.items[at] = item,
                None => self.items.push(item),
            }
        }
        if self.items.len() > KEEP {
            let extra = self.items.len() - KEEP;
            self.items.drain(..extra);
        }
        self.generation += 1;
    }

    /// `seat.delta`: the reply as it's written, all of it so far
    pub fn delta(&mut self, id: String, text: String) {
        match self.streaming.iter_mut().find(|(i, _)| *i == id) {
            Some(entry) => entry.1 = text,
            None => self.streaming.push((id, text)),
        }
        self.generation += 1;
    }

    /// The seat went: a reply it was writing won't finish
    pub fn stop_streaming(&mut self) {
        if !self.streaming.is_empty() {
            self.streaming.clear();
            self.generation += 1;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.streaming.is_empty()
    }

    /// In the order they show: what's been said, the replies still being written, then his
    /// words waiting for the turn under way
    pub fn shown(&self) -> Vec<Shown<'_>> {
        let mut out: Vec<Shown> = self.items.iter().filter(|i| !i.queued).map(|i| self.show(i)).collect();
        out.extend(
            self.streaming
                .iter()
                .filter(|(id, _)| !self.items.iter().any(|i| i.id == *id))
                .map(|(id, text)| Shown {
                    id,
                    at: 0,
                    who: &CONDUCTOR,
                    from: None,
                    label: None,
                    project: None,
                    text,
                    queued: false,
                    tool: None,
                    streaming: true,
                }),
        );
        out.extend(self.items.iter().filter(|i| i.queued).map(|i| self.show(i)));
        out
    }

    /// An item as it shows: the words being written in place of its own while it streams
    fn show<'s>(&'s self, item: &'s Item) -> Shown<'s> {
        let streaming = self
            .streaming
            .iter()
            .find(|(i, _)| *i == item.id)
            .map(|(_, t)| t.as_str());
        Shown {
            id: &item.id,
            at: item.at,
            who: &item.who,
            from: item.from.as_deref(),
            label: item.label.as_deref(),
            project: item.project.as_deref(),
            text: streaming.unwrap_or(&item.text),
            queued: item.queued,
            tool: item.tool.as_ref(),
            streaming: streaming.is_some(),
        }
    }
}

/// What LBTerm asks of the seat
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Everything from now on; `client` is `lbterm`, or `lbterm-try` in a try window
    Subscribe { client: String, pid: u32, version: String },
    /// His words to the Conductor
    Say { text: String },
    Allow { ask: String },
    Deny { ask: String, why: String },
    /// End the turn under way
    Interrupt,
    /// `on`, `off`, `restart`, `mode read|most|all`, `listen off`, `listen catch up`,
    /// `listen fresh`
    Control { action: String },
    /// A waiting session's question: `picks` one list per picker question, counting from 1,
    /// `other` his own words per question, `text` for a question in text
    Answer {
        question: String,
        picks: Vec<Vec<u32>>,
        other: Vec<Option<String>>,
        text: Option<String>,
    },
    /// `send`, `dismiss` or `rate` (`rating` `useful` or `not`)
    Rsi {
        action: String,
        suggestion: String,
        rating: Option<String>,
    },
}

impl Command {
    /// Its type on the pipe, `seat.say` and the rest
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Subscribe { .. } => "seat.subscribe",
            Command::Say { .. } => "seat.say",
            Command::Allow { .. } => "seat.allow",
            Command::Deny { .. } => "seat.deny",
            Command::Interrupt => "seat.interrupt",
            Command::Control { .. } => "seat.control",
            Command::Answer { .. } => "seat.answer",
            Command::Rsi { .. } => "seat.rsi",
        }
    }

    /// The line for the pipe, without its newline
    pub fn line(&self, id: &str) -> String {
        let v = match self {
            Command::Subscribe { client, pid, version } => json!({
                "type": "seat.subscribe", "id": id, "client": client, "pid": pid, "version": version,
            }),
            Command::Say { text } => json!({ "type": "seat.say", "id": id, "text": text, "from": BY }),
            Command::Allow { ask } => json!({ "type": "seat.allow", "id": id, "ask": ask, "by": BY }),
            Command::Deny { ask, why } => {
                json!({ "type": "seat.deny", "id": id, "ask": ask, "why": why, "by": BY })
            }
            Command::Interrupt => json!({ "type": "seat.interrupt", "id": id, "by": BY }),
            Command::Control { action } => {
                json!({ "type": "seat.control", "id": id, "action": action, "by": BY })
            }
            Command::Answer {
                question,
                picks,
                other,
                text,
            } => json!({
                "type": "seat.answer", "id": id, "question": question, "picks": picks,
                "other": other, "text": text, "by": BY,
            }),
            Command::Rsi {
                action,
                suggestion,
                rating,
            } => json!({
                "type": "seat.rsi", "id": id, "action": action, "suggestion": suggestion,
                "rating": rating, "by": BY,
            }),
        };
        v.to_string()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const VIEW: &str = r#"{
      "type": "seat.view", "seq": 12, "at": 1792828000000,
      "conductor": {
        "state": "on", "mode": "most", "listening": false, "pausedAt": 1792820000000, "catchUp": true,
        "busy": true, "queued": 2, "sessionId": "c0", "startedAt": 1792820000000, "model": "claude-opus-5-5",
        "error": null
      },
      "asks": [
        { "id": "ask-3", "at": 1792828000000, "tool": "SendMessage", "title": "Message Publishing the résumé in Website",
          "detail": "the message, in full", "session": "website-a6", "label": "Publishing the résumé", "project": "Website" },
        { "title": "no id: passed over" }
      ],
      "sessions": [
        { "sessionId": "s1", "name": "website-a6", "project": "Website", "status": "working",
          "label": "Publishing the résumé", "summary": "…", "question": null, "pane": "17",
          "percent": 140, "left": 240, "agents": [{ "id": "a", "kind": "agent", "what": "Survey", "percent": 50, "left": 120 }],
          "conductor": "thinking" },
        { "sessionId": "s2", "name": "x", "pane": 9, "hidden": true }
      ],
      "waiting": [
        { "id": "s1:toolu_1", "sessionId": "s1", "session": "website-a6", "label": "Publishing the résumé", "project": "Website",
          "kind": "picker", "extras": false, "gist": "Which layout?", "at": 1792828000000,
          "questions": [{ "question": "Which layout?", "header": "Layout", "multiSelect": false,
                          "options": [{ "label": "Two columns", "description": "Side by side" }, { "label": "One" }] }],
          "text": null, "answerable": true, "why": null },
        { "id": "s2:1792828000000", "sessionId": "s2", "kind": "text", "text": "1a or 1b?", "answerable": false, "why": "no pane" }
      ],
      "events": [ { "seq": 812, "at": 1792828000000, "kind": "waiting", "session": "website-a6", "label": null, "project": "Website", "text": "…", "wake": true } ],
      "suggestions": [ { "id": "g1", "day": "2026-10-23", "project": "Terminal", "kind": "tooling", "title": "T", "change": "C",
          "evidence": "E", "saves": "S", "status": "open", "rating": null } ],
      "notes": [ { "id": "note-4", "at": 1792828000000, "text": "website-a6 needs your OK to push" } ]
    }"#;

    #[test]
    fn the_view_reads() {
        let Some(Incoming::View(v)) = parse(&VIEW.replace('\n', " ")) else {
            panic!("not a view")
        };
        assert_eq!((v.seq, v.at), (12, 1_792_828_000_000));
        let c = &v.conductor;
        assert_eq!((c.state.as_str(), c.mode.as_str()), ("on", "most"));
        assert!(!c.listening && c.catch_up && c.busy);
        assert_eq!((c.queued, c.paused_at), (2, Some(1_792_820_000_000)));
        assert_eq!(c.error, None, "null reads as none");
        assert_eq!(c.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(v.asks.len(), 1, "an ask without an id is passed over");
        assert_eq!(v.asks[0].detail, "the message, in full");
        assert_eq!(v.sessions[0].percent, Some(100), "a percent past 100 is 100");
        assert_eq!(v.sessions[0].pane.as_deref(), Some("17"));
        assert_eq!(v.sessions[1].pane.as_deref(), Some("9"), "a pane as a number reads too");
        assert_eq!(v.sessions[1].status, "unknown");
        assert_eq!(v.sessions[0].agents[0].percent, Some(50));
        assert_eq!(v.sessions[0].conductor.as_deref(), Some("thinking"));
        // Hidden from the Conductor where the seat says so, not where it doesn't
        assert!(!v.sessions[0].hidden && v.sessions[1].hidden);
        let w = &v.waiting[0];
        assert!(w.one_click() && w.answerable);
        assert_eq!(w.questions[0].options[0].description, "Side by side");
        assert_eq!(w.questions[0].options[1].description, "");
        assert_eq!(v.waiting[1].text.as_deref(), Some("1a or 1b?"));
        assert!(!v.waiting[1].answerable);
        assert_eq!(v.waiting[1].why.as_deref(), Some("no pane"));
        assert!(v.events[0].wake);
        assert_eq!(v.asks[0].label.as_deref(), Some("Publishing the résumé"));
        assert_eq!(w.label.as_deref(), Some("Publishing the résumé"));
        assert_eq!(v.waiting[1].label, None, "no label: none");
        assert_eq!(v.events[0].label, None, "a null label: none");
        assert_eq!(v.suggestions[0].rating, None);
        assert_eq!(v.notes[0].text, "website-a6 needs your OK to push");
    }

    #[test]
    fn a_view_with_little_in_it_reads_as_off() {
        let Some(Incoming::View(v)) = parse(r#"{"type":"seat.view"}"#) else {
            panic!()
        };
        assert_eq!(v.conductor.state, "off");
        assert!(v.conductor.listening, "listening unless it says not");
        assert!(!v.conductor.takes_words());
        assert!(v.asks.is_empty() && v.sessions.is_empty());
    }

    #[test]
    fn the_other_lines_read() {
        assert_eq!(parse(r#"{"type":"seat.show"}"#), Some(Incoming::Show));
        assert_eq!(
            parse(r#"{"type":"seat.delta","id":"msg_1","text":"So far"}"#),
            Some(Incoming::Delta {
                id: "msg_1".into(),
                text: "So far".into()
            })
        );
        let Some(Incoming::Reply(r)) = parse(r#"{"type":"seat.reply","id":"lbterm-1","ok":false,"error":"It's off"}"#)
        else {
            panic!()
        };
        assert_eq!((r.id.as_str(), r.ok, r.words()), ("lbterm-1", false, Some("It's off")));
        let Some(Incoming::Reply(r)) = parse(r#"{"type":"seat.reply","id":"x","ok":true,"text":"Allowed"}"#) else {
            panic!()
        };
        assert_eq!(r.words(), Some("Allowed"));
        assert_eq!(parse(r#"{"type":"conductor.focus","sessions":[]}"#), None);
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"type":"seat.delta"}"#), None, "a delta needs its item's id");
    }

    fn items(line: &str) -> (bool, Vec<Item>) {
        match parse(line) {
            Some(Incoming::Items { reset, items }) => (reset, items),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn items_read() {
        let (reset, list) = items(
            r#"{ "type": "seat.items", "reset": true, "items": [
              { "id": "6f0", "at": 1, "who": "you", "from": "lbterm", "text": "what's waiting on me?", "queued": false },
              { "id": "msg_01", "at": 2, "who": "conductor", "text": "website-a6 wants…" },
              { "id": "toolu_01", "at": 3, "who": "tool", "tool": { "name": "fleet", "text": "fleet", "status": "running" } },
              { "id": "h", "who": "hub", "text": "Conductor hub: 2 things" },
              { "id": "p", "who": "peer", "from": "website-a6", "label": "Site publish", "project": "Website", "text": "hi" },
              { "id": "n", "who": "notice", "text": "Accept most from now on (Lachlan, LBTerm)" },
              { "id": "z", "who": "dream", "text": "?" },
              { "who": "you", "text": "no id" } ] }"#,
        );
        assert!(reset);
        assert_eq!(list.len(), 7, "an item without an id is passed over");
        assert_eq!(list[0].who, Who::You);
        assert_eq!(list[0].from.as_deref(), Some("lbterm"));
        assert_eq!(list[2].tool.as_ref().map(|t| t.status.as_str()), Some("running"));
        assert_eq!(list[4].from.as_deref(), Some("website-a6"));
        assert_eq!((list[4].label.as_deref(), list[4].project.as_deref()), (Some("Site publish"), Some("Website")));
        assert_eq!(list[0].label, None, "only a peer's says who sent it");
        assert_eq!(list[6].who, Who::Other("dream".into()));
    }

    fn item(id: &str, who: Who, text: &str, queued: bool) -> Item {
        Item {
            id: id.into(),
            at: 0,
            who,
            from: None,
            label: None,
            project: None,
            text: text.into(),
            queued,
            tool: None,
        }
    }

    fn order(c: &Conversation) -> Vec<String> {
        c.shown()
            .iter()
            .map(|s| format!("{}{}{}", s.id, if s.queued { "?" } else { "" }, if s.streaming { "~" } else { "" }))
            .collect()
    }

    #[test]
    fn the_conversation_replaces_by_id_and_streams() {
        let mut c = Conversation::default();
        c.apply(true, vec![item("a", Who::You, "hello", false)]);
        // He says more while the turn runs: queued, shown last
        c.apply(false, vec![item("b", Who::You, "and this", true)]);
        // The reply to the first, written as it goes
        c.delta("r1".into(), "Hel".into());
        assert_eq!(order(&c), ["a", "r1~", "b?"]);
        c.delta("r1".into(), "Hello there".into());
        assert_eq!(c.shown()[1].text, "Hello there");
        assert_eq!(c.shown()[1].who, &Who::Conductor);
        // The finished reply replaces what was streaming
        c.apply(false, vec![item("r1", Who::Conductor, "Hello there.", false)]);
        assert_eq!(order(&c), ["a", "r1", "b?"]);
        // The queued message starts: after everything said
        c.apply(false, vec![item("b", Who::You, "and this", false)]);
        assert_eq!(order(&c), ["a", "r1", "b"]);
        // An item sent again in place keeps its place
        c.apply(false, vec![item("a", Who::You, "hello!", false)]);
        assert_eq!(order(&c), ["a", "r1", "b"]);
        assert_eq!(c.shown()[0].text, "hello!");
        // A delta for an item that's there already shows the new words in its place
        c.delta("r1".into(), "Hello there. More".into());
        assert_eq!(order(&c), ["a", "r1~", "b"]);
        c.stop_streaming();
        assert_eq!(order(&c), ["a", "r1", "b"]);
        // Reset: all of it again
        let g = c.generation;
        c.apply(true, vec![item("x", Who::Notice, "on", false)]);
        assert_eq!(order(&c), ["x"]);
        assert!(c.generation > g);
    }

    #[test]
    fn the_conversation_keeps_the_last_ones() {
        let mut c = Conversation::default();
        let many: Vec<Item> = (0..KEEP + 10).map(|i| item(&i.to_string(), Who::Hub, "w", false)).collect();
        c.apply(true, many);
        let shown = c.shown();
        assert_eq!(shown.len(), KEEP);
        assert_eq!(shown[0].id, "10");
    }

    fn json(line: &str) -> Value {
        assert!(!line.contains('\n'), "one line");
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn commands_carry_their_id_and_who_sent_them() {
        let v = json(&Command::Say { text: "what's up?\nsecond".into() }.line("lbterm-4"));
        assert_eq!((v["type"].as_str(), v["id"].as_str()), (Some("seat.say"), Some("lbterm-4")));
        assert_eq!((v["text"].as_str(), v["from"].as_str()), (Some("what's up?\nsecond"), Some("lbterm")));
        let v = json(&Command::Allow { ask: "ask-3".into() }.line("1"));
        assert_eq!((v["type"].as_str(), v["ask"].as_str(), v["by"].as_str()), (Some("seat.allow"), Some("ask-3"), Some("lbterm")));
        let v = json(&Command::Deny { ask: "ask-3".into(), why: "".into() }.line("2"));
        assert_eq!((v["type"].as_str(), v["why"].as_str()), (Some("seat.deny"), Some("")));
        assert_eq!(json(&Command::Interrupt.line("3"))["type"], "seat.interrupt");
        let v = json(&Command::Control { action: "mode most".into() }.line("4"));
        assert_eq!((v["action"].as_str(), v["by"].as_str()), (Some("mode most"), Some("lbterm")));
        let v = json(
            &Command::Answer {
                question: "s1:toolu_1".into(),
                picks: vec![vec![2], vec![]],
                other: vec![None, Some("my own".into())],
                text: None,
            }
            .line("5"),
        );
        assert_eq!(v["picks"], serde_json::json!([[2], []]));
        assert_eq!(v["other"], serde_json::json!([null, "my own"]));
        assert!(v["text"].is_null());
        let v = json(
            &Command::Rsi {
                action: "rate".into(),
                suggestion: "g1".into(),
                rating: Some("useful".into()),
            }
            .line("6"),
        );
        assert_eq!((v["type"].as_str(), v["rating"].as_str()), (Some("seat.rsi"), Some("useful")));
        let v = json(
            &Command::Subscribe {
                client: "lbterm".into(),
                pid: 42,
                version: "20261024".into(),
            }
            .line("0"),
        );
        assert_eq!((v["type"].as_str(), v["client"].as_str(), v["pid"].as_u64()), (Some("seat.subscribe"), Some("lbterm"), Some(42)));
    }
}
