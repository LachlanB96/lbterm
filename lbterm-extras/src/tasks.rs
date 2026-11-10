//! What a Claude Code session has running in the background, for the card the tab panel shows
//! while the pointer rests on a tab's row (lbterm/TAB-PANEL.md, "What's running"): its
//! subagents, its commands run in the background and its Monitors.
//!
//! Terminal's statusline keeps the list in `<sid>.tasks.json` in the `lbterm_summaries` folder
//! (its `rememberTasks`), which summaries.rs reads with the session's other files: each task's
//! id, kind, description, when it started and, for a subagent, its own transcript. What a
//! subagent is doing now and how many tokens it has used come from that transcript, read only
//! while the card is open and never on the GUI thread ([`Transcripts`]), from where the last read
//! stopped each time it changes ([`Follow`]).

use crate::strip::elide;
use crate::tabs::Rect;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What a task is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A subagent (Claude Code's Agent tool)
    Agent,
    /// A command run in the background (Bash or PowerShell with `run_in_background`)
    Command,
    /// A Monitor
    Monitor,
    /// A Workflow run (Claude Code's Workflow tool), named by its summary: the statusline counts
    /// one from its launch to its task notification (Terminal's bf35e34 of 2026-11-09). Its
    /// agents' transcripts are its own, so it has no second line, as a command has none
    Workflow,
}

impl Kind {
    /// As the statusline names it; anything else is a command, the kind with least to say
    pub fn from_name(name: &str) -> Self {
        match name {
            "agent" => Kind::Agent,
            "monitor" => Kind::Monitor,
            "workflow" => Kind::Workflow,
            _ => Kind::Command,
        }
    }

    /// The card's word for it
    pub fn word(self) -> &'static str {
        match self {
            Kind::Agent => "Agent",
            Kind::Command => "Command",
            Kind::Monitor => "Monitor",
            Kind::Workflow => "Workflow",
        }
    }
}

/// One task still running, as the statusline wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// The id its start named: the OSC 7501 record's `bg/<id>`
    pub id: String,
    pub kind: Kind,
    /// The Agent call's description, the command's own (else its first line), the Monitor's
    pub description: String,
    /// When it was asked for, Unix ms
    pub started_at: f64,
    /// A subagent's type, as the Agent call named it
    pub agent_type: Option<String>,
    /// A subagent's own transcript
    pub transcript: Option<PathBuf>,
}

/// A session's tasks, oldest first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct List {
    pub session_id: String,
    pub tasks: Vec<Task>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTask {
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    started_at: f64,
    #[serde(default)]
    agent_type: Option<String>,
    #[serde(default)]
    transcript: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawList {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    tasks: Vec<RawTask>,
}

/// `<sid>.tasks.json` as the statusline writes it; `None` when it isn't one.
pub fn parse(text: &str) -> Option<List> {
    let raw: RawList = serde_json::from_str(text).ok()?;
    Some(List {
        session_id: raw.session_id,
        tasks: raw
            .tasks
            .into_iter()
            .filter(|t| !t.id.is_empty())
            .map(|t| Task {
                kind: Kind::from_name(&t.kind),
                description: one_line(&t.description),
                started_at: t.started_at,
                agent_type: t.agent_type.filter(|a| !a.is_empty()),
                transcript: t.transcript.filter(|p| !p.is_empty()).map(PathBuf::from),
                id: t.id,
            })
            .collect(),
    })
}

/// The first line of `text` with words in it, its spaces single
fn one_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What's known of a subagent from its own transcript.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Progress {
    /// Its latest tool call, in a few words ([`describe`])
    pub doing: Option<String>,
    /// How many tool calls it has made
    pub tools: usize,
    /// Its latest request's tokens, in and out, cache included: what Claude Code counts for a
    /// subagent
    pub tokens: Option<u64>,
}

/// A transcript read as it grows: each read takes up where the last stopped, and a line cut
/// short waits for the rest.
#[derive(Debug, Default)]
pub struct Follow {
    offset: u64,
    partial: Vec<u8>,
    progress: Progress,
}

/// What an assistant record says of itself: only lines holding it are parsed
const ASSISTANT: &[u8] = br#""role":"assistant""#;
/// The most read in one go: a long transcript is read in pieces of this, one after another
const READ_MAX: u64 = 16 * 1024 * 1024;

fn has(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

impl Follow {
    /// What's been read so far says
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// Take in more of the transcript, as it was written
    pub fn feed(&mut self, bytes: &[u8]) {
        self.offset += bytes.len() as u64;
        let mut rest = bytes;
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            let (head, tail) = rest.split_at(end);
            if self.partial.is_empty() {
                self.line(head);
            } else {
                let mut whole = std::mem::take(&mut self.partial);
                whole.extend_from_slice(head);
                self.line(&whole);
            }
            rest = &tail[1..];
        }
        self.partial.extend_from_slice(rest);
    }

    fn line(&mut self, line: &[u8]) {
        if !has(line, ASSISTANT) {
            return;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if record.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(message) = record.get("message") else {
            return;
        };
        if let Some(usage) = message.get("usage") {
            let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
            let total = n("input_tokens")
                + n("cache_creation_input_tokens")
                + n("cache_read_input_tokens")
                + n("output_tokens");
            if total > 0 {
                self.progress.tokens = Some(total);
            }
        }
        let blocks = message.get("content").and_then(Value::as_array);
        for block in blocks.into_iter().flatten() {
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                let input = block.get("input").unwrap_or(&Value::Null);
                self.progress.tools += 1;
                self.progress.doing = Some(describe(name, input));
            }
        }
    }

    /// Read what's been added to the file at `path` since the last look, to its end as it is
    /// now, [`READ_MAX`] at a time. `None` while there's no file; a file that got shorter is read
    /// again from its start.
    pub fn read_more(&mut self, path: &Path) -> Option<Progress> {
        let mut file = std::fs::File::open(path).ok()?;
        let len = file.metadata().ok()?.len();
        if len < self.offset {
            *self = Follow::default();
        }
        let mut buf = vec![];
        while len > self.offset {
            let want = (len - self.offset).min(READ_MAX);
            buf.clear();
            if file.seek(SeekFrom::Start(self.offset)).is_err() {
                break;
            }
            match file.by_ref().take(want).read_to_end(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => self.feed(&buf),
            }
        }
        Some(self.progress.clone())
    }
}

/// A tool call in a few words: what it's on, not how. "Read tab_panel.rs", "Grep owns_zindex",
/// a command's own description, "Agent Review the card".
pub fn describe(name: &str, input: &Value) -> String {
    let field = |k: &str| {
        input
            .get(k)
            .and_then(Value::as_str)
            .map(one_line)
            .filter(|s| !s.is_empty())
    };
    let file = |k: &str| {
        field(k).map(|p| {
            p.rsplit(|c| c == '/' || c == '\\')
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string()
        })
    };
    let with = |verb: &str, what: Option<String>| match what {
        Some(what) => format!("{verb} {what}"),
        None => verb.to_string(),
    };
    match name {
        "Read" => with("Read", file("file_path")),
        "Edit" | "MultiEdit" => with("Edit", file("file_path")),
        "Write" => with("Write", file("file_path")),
        "NotebookEdit" => with("Edit", file("notebook_path")),
        "Bash" | "PowerShell" => field("description")
            .or_else(|| field("command").map(|c| format!("$ {c}")))
            .unwrap_or_else(|| name.to_string()),
        "Monitor" => with("Monitor", field("description")),
        "Grep" => with("Grep", field("pattern")),
        "Glob" => with("Glob", field("pattern")),
        "WebFetch" => with(
            "Fetch",
            field("url").map(|u| {
                let u = u.split("://").nth(1).unwrap_or(&u).to_string();
                u.split('/').next().unwrap_or("").to_string()
            }),
        ),
        "WebSearch" => with("Search", field("query")),
        "Agent" | "Task" => with("Agent", field("description")),
        "Skill" => with("Skill", field("skill")),
        "TodoWrite" => "Todo list".to_string(),
        "SubagentHandback" => "Handing back".to_string(),
        _ => match name.strip_prefix("mcp__").map(|rest| rest.splitn(2, "__")) {
            Some(mut parts) => {
                let server = parts.next().unwrap_or("");
                match parts.next() {
                    Some(tool) => format!("{server} {tool}"),
                    None => server.to_string(),
                }
            }
            None => name.to_string(),
        },
    }
}

/// How long something has been running, `secs` seconds: "42s", "12m", "1h 5m".
pub fn age(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}

/// Seconds until [`age`] says something else, `secs` seconds in.
pub fn age_changes_in(secs: f64) -> f64 {
    let secs = secs.max(0.0);
    if secs < 60.0 {
        1.0 - secs.fract()
    } else {
        60.0 - secs % 60.0
    }
}

/// Tokens in a few characters, as Terminal's statusline counts them: "950", "84k", "1.2M"
pub fn tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 999_500 {
        format!("{}k", (n as f64 / 1000.0).round() as u64)
    } else {
        let m = format!("{:.1}", n as f64 / 1e6);
        format!("{}M", m.trim_end_matches(".0"))
    }
}

/// One task as the card lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// "Agent", "Command", "Monitor", "Workflow"
    pub kind: &'static str,
    pub description: String,
    /// How long it's been running
    pub age: String,
    /// For a subagent: its latest tool call, its tool calls and its tokens,
    /// "Edit tab_panel.rs · 31 tools · 84k tokens"; its type until its transcript says more
    pub detail: Option<String>,
}

/// Between the parts of a subagent's line
pub const SEPARATOR: &str = " \u{b7} ";

/// `list`'s tasks as the card shows them at `now` (Unix ms), `progress` saying what's known of
/// a subagent's transcript; and how long until an age changes, in seconds.
pub fn shown(
    list: &List,
    progress: impl Fn(&Path) -> Option<Progress>,
    now: f64,
) -> (Vec<Shown>, Option<f64>) {
    let mut next: Option<f64> = None;
    let shown = list
        .tasks
        .iter()
        .map(|task| {
            let secs = (now - task.started_at) / 1000.0;
            let change = age_changes_in(secs);
            next = Some(next.map_or(change, |n| n.min(change)));
            let detail = match task.kind {
                Kind::Agent => {
                    let p = task.transcript.as_deref().and_then(&progress).unwrap_or_default();
                    let mut parts = vec![];
                    if let Some(doing) = p.doing {
                        parts.push(doing);
                    }
                    if p.tools > 0 {
                        parts.push(format!("{} tool{}", p.tools, if p.tools == 1 { "" } else { "s" }));
                    }
                    if let Some(n) = p.tokens {
                        parts.push(format!("{} tokens", tokens(n)));
                    }
                    if parts.is_empty() {
                        task.agent_type.clone()
                    } else {
                        Some(parts.join(SEPARATOR))
                    }
                }
                Kind::Command | Kind::Monitor | Kind::Workflow => None,
            };
            Shown {
                kind: task.kind.word(),
                description: if task.description.is_empty() {
                    task.kind.word().to_lowercase()
                } else {
                    task.description.clone()
                },
                age: age(secs),
                detail,
            }
        })
        .collect();
    (shown, next)
}

/// The most tasks the card lists; past it, the last line says how many more
pub const MAX_TASKS: usize = 8;
/// The most columns a description or a subagent's line takes
pub const CARD_COLUMNS: usize = 60;

/// How a line of the card is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A task: its kind, its description, its age
    Task,
    /// A subagent's line under it
    Detail,
    /// "+3 more"
    More,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The line's top
    pub y: f64,
    /// The task's kind, in the label column
    pub label: Option<String>,
    pub text: String,
    /// The age, right-aligned: its words and where they start
    pub right: Option<(String, f64)>,
    pub tone: Tone,
}

/// The card, laid out: where it goes and where each line of it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub rect: Rect,
    /// Where the labels start
    pub label_x: f64,
    /// Where the descriptions start
    pub text_x: f64,
    pub lines: Vec<Line>,
}

fn columns(text: &str) -> usize {
    text.chars().count()
}

/// Lay out the card for `shown` beside a row: its left edge at `left` and its top level with
/// `top`, moved up when that would take it past `bottom` but never above `min_top`, and no
/// wider than reaches `max_right`. `cell` is a column's width and a line's height, in pixels;
/// `scale` is the DPI over 96. `None` with nothing to show.
pub fn card(
    shown: &[Shown],
    left: f64,
    top: f64,
    (min_top, bottom): (f64, f64),
    max_right: f64,
    cell: (f64, f64),
    scale: f64,
) -> Option<Card> {
    if shown.is_empty() || cell.0 <= 0.0 {
        return None;
    }
    let pad = 12.0 * scale;
    let gap = 6.0 * scale;
    let (listed, more) = if shown.len() > MAX_TASKS {
        (&shown[..MAX_TASKS - 1], shown.len() - (MAX_TASKS - 1))
    } else {
        (shown, 0)
    };
    let label_columns = listed.iter().map(|s| columns(s.kind)).max().unwrap_or(0) + 2;
    let age_columns = listed.iter().map(|s| columns(&s.age)).max().unwrap_or(0);
    let room = ((max_right - left - 2.0 * pad) / cell.0).floor().max(0.0) as usize;
    let text_columns = room
        .saturating_sub(label_columns + 2 + age_columns)
        .min(CARD_COLUMNS)
        .max(12);
    // A subagent's line has the age's column too: nothing is right of it
    let detail_columns = text_columns + 2 + age_columns;

    struct Laid {
        label: Option<String>,
        text: String,
        age: Option<String>,
        tone: Tone,
        first: bool,
    }
    let mut laid = vec![];
    for s in listed {
        laid.push(Laid {
            label: Some(s.kind.to_string()),
            text: elide(&s.description, text_columns),
            age: Some(s.age.clone()),
            tone: Tone::Task,
            first: true,
        });
        if let Some(detail) = &s.detail {
            laid.push(Laid {
                label: None,
                text: elide(detail, detail_columns),
                age: None,
                tone: Tone::Detail,
                first: false,
            });
        }
    }
    if more > 0 {
        laid.push(Laid {
            label: None,
            text: format!("+{more} more"),
            age: None,
            tone: Tone::More,
            first: true,
        });
    }
    let widest = laid
        .iter()
        .map(|l| match l.age {
            Some(_) => columns(&l.text) + 2 + age_columns,
            None => columns(&l.text),
        })
        .max()
        .unwrap_or(0);
    let width = (2.0 * pad + (label_columns + widest) as f64 * cell.0).ceil();
    let tasks = laid.iter().filter(|l| l.first).count();
    let height = (2.0 * pad + laid.len() as f64 * cell.1 + tasks.saturating_sub(1) as f64 * gap).ceil();
    let top = if top + height > bottom {
        (bottom - height).max(min_top)
    } else {
        top
    };
    let label_x = left + pad;
    let text_x = label_x + label_columns as f64 * cell.0;
    let right_edge = left + width - pad;

    let mut lines = vec![];
    let mut y = top + pad;
    for (i, l) in laid.into_iter().enumerate() {
        if i > 0 && l.first {
            y += gap;
        }
        lines.push(Line {
            y,
            label: l.label,
            right: l.age.map(|a| {
                let x = right_edge - columns(&a) as f64 * cell.0;
                (a, x)
            }),
            text: l.text,
            tone: l.tone,
        });
        y += cell.1;
    }
    Some(Card {
        rect: Rect {
            x: left,
            y: top,
            w: width,
            h: height,
        },
        label_x,
        text_x,
        lines,
    })
}

/// Keep `card` at least `width` wide, but no further right than `max_right`: the width it had a
/// moment ago, so its edge doesn't twitch narrower each time a subagent's line gets shorter. Its
/// ages move with its right edge.
pub fn widen(card: &mut Card, width: f64, max_right: f64) {
    let by = width.min(max_right - card.rect.x) - card.rect.w;
    if by <= 0.0 {
        return;
    }
    card.rect.w += by;
    for line in &mut card.lines {
        if let Some((_, x)) = &mut line.right {
            *x += by;
        }
    }
}

/// A transcript nobody has asked for in this long is no longer watched (a subagent still writing
/// it would wake the watch for every line with no card to show it). The open card asks at least
/// once a minute, as its ages read differently each minute at most
const FORGET: Duration = Duration::from_secs(120);
/// What was read of a transcript no longer watched is kept this long since it was last asked
/// for, so a card opened again reads only what was added to it: let go of when the thread next
/// wakes after that, never by a wake of its own. Until 2026-10-31 a transcript no longer watched
/// was forgotten with it, and a card opened again more than two minutes after it closed read each
/// subagent's transcript again from its start, 16 MB at a time
const KEEP: Duration = Duration::from_secs(3600);
/// A transcript written is several changes in a row: they're taken together, this long after the
/// first
const SETTLE: Duration = Duration::from_millis(25);
/// How many times the thread follows the transcripts' folders before it reads them: a folder made
/// can have the next one under it already
const FOLLOWS: usize = 8;

struct Known {
    progress: Option<Progress>,
    asked: Instant,
    /// Watched: asked for in the last [`FORGET`] (the thread says, as it watches what's asked)
    watched: bool,
}

struct Shared {
    known: Mutex<HashMap<PathBuf, Known>>,
    generation: AtomicU64,
    wake: Box<dyn Fn() + Send + Sync>,
    /// [`FORGET`], shorter in the tests
    forget: Duration,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What wakes the reading thread
enum Msg {
    /// A transcript asked for the first time
    Asked(PathBuf),
    Seen(crate::watch::Seen),
}

/// Subagents' transcripts, read on a thread of their own while the card asks for them: each the
/// first time it's asked for, and again whenever Windows says it changed (crate::watch, through
/// its folder). While nothing changes the thread sleeps: until 2026-10-30 the card looked again
/// every second it was open, repainting the window each time to ask.
pub struct Transcripts {
    shared: Arc<Shared>,
    tx: Mutex<Sender<Msg>>,
}

impl Transcripts {
    /// Start the reading thread; `wake` is called from it whenever what's known of a
    /// transcript asked for changes.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::start_forgetting(FORGET, wake)
    }

    fn start_forgetting(forget: Duration, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            known: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            wake: Box::new(wake),
            forget,
        });
        let (tx, rx) = channel();
        let thread_shared = Arc::clone(&shared);
        let events = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("lbterm-transcripts".into())
            .spawn(move || follow(thread_shared, rx, events));
        if let Err(err) = spawned {
            log::warn!("tasks: no thread to read transcripts on: {err:#}");
        }
        Self {
            shared,
            tx: Mutex::new(tx),
        }
    }

    /// What's known of the transcript at `path`, from memory: `None` until it's first been read,
    /// on the other thread, which is told the first time it's asked for and reads it again
    /// whenever it changes. One no longer watched answers what was read of it and is watched
    /// again, the thread reading on from where it stopped.
    pub fn get(&self, path: &Path) -> Option<Progress> {
        let now = Instant::now();
        let mut known = lock(&self.shared.known);
        if let Some(k) = known.get_mut(path) {
            k.asked = now;
            if !std::mem::replace(&mut k.watched, true) {
                let _ = lock(&self.tx).send(Msg::Asked(path.to_path_buf()));
            }
            return k.progress.clone();
        }
        known.insert(path.to_path_buf(), Known { progress: None, asked: now, watched: true });
        let _ = lock(&self.tx).send(Msg::Asked(path.to_path_buf()));
        None
    }

    /// Bumped each time what's known of a transcript changes
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }
}

/// The reading thread: each transcript asked for is read from where its last read stopped, the
/// first time and whenever it changes; one asked for no more in [`FORGET`] is no longer watched,
/// and what was read of it let go after [`KEEP`].
fn follow(shared: Arc<Shared>, rx: Receiver<Msg>, events: Sender<Msg>) {
    use crate::watch::{Seen, Target, Watch};
    let mut watch = match Watch::new(move |s| {
        let _ = events.send(Msg::Seen(s));
    }) {
        Ok(watch) => watch,
        Err(err) => {
            log::error!("tasks: can't watch the subagents' transcripts, so the card won't follow them: {err:#}");
            return;
        }
    };
    let mut follows: HashMap<PathBuf, Follow> = HashMap::new();
    let mut msgs: Vec<Msg> = vec![];
    loop {
        // Watch what's been asked for lately, and let go of what nobody has asked for in a long
        // while: a folder newly watched may hold more of its transcript already, so each is read
        // again then
        let targets: Vec<Target> = {
            let now = Instant::now();
            let mut known = lock(&shared.known);
            known.retain(|_, k| now.duration_since(k.asked) < KEEP);
            follows.retain(|p, _| known.contains_key(p));
            known
                .iter_mut()
                .filter_map(|(p, k)| {
                    k.watched = now.duration_since(k.asked) < shared.forget;
                    k.watched.then(|| Target::File(p.clone()))
                })
                .collect()
        };
        let mut moved = false;
        for _ in 0..FOLLOWS {
            if !watch.follow(&targets) {
                break;
            }
            moved = true;
        }
        let mut due: Vec<PathBuf> = vec![];
        let mut all = moved;
        for msg in msgs.drain(..) {
            match msg {
                Msg::Asked(path) => due.push(path),
                Msg::Seen(Seen::Changed(paths)) => {
                    due.extend(watch.which(&paths).iter().map(|t| t.path().to_path_buf()));
                }
                Msg::Seen(Seen::Failed(err)) => {
                    log::error!("tasks: the transcripts' watch: {err}; reading every one again");
                    all = true;
                }
            }
        }
        if all {
            due = targets.iter().map(|t| t.path().to_path_buf()).collect();
        }
        due.sort();
        due.dedup();

        let mut changed = false;
        for path in due {
            if !lock(&shared.known).contains_key(&path) {
                continue;
            }
            let progress = follows.entry(path.clone()).or_default().read_more(&path);
            let mut known = lock(&shared.known);
            if let Some(k) = known.get_mut(&path) {
                if k.progress != progress {
                    k.progress = progress;
                    changed = true;
                }
            }
        }
        if changed {
            shared.generation.fetch_add(1, Ordering::AcqRel);
            (shared.wake)();
        }

        // Asleep until one is asked for or changes, or the first watched is due to be no longer
        let forget = lock(&shared.known).values().filter(|k| k.watched).map(|k| k.asked + shared.forget).min();
        let first = match forget {
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => return,
            },
        };
        if let Some(first) = first {
            let matters = match &first {
                Msg::Asked(_) | Msg::Seen(Seen::Failed(_)) => true,
                Msg::Seen(Seen::Changed(paths)) => watch.matters(paths),
            };
            msgs.push(first);
            if matters {
                std::thread::sleep(SETTLE);
                msgs.extend(rx.try_iter());
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;

    /// A file as Terminal's statusline writes it (statusline.ts, rememberTasks)
    const FILE: &str = r#"{"sessionId":"5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35","tasks":[
        {"id":"a3f09c2b7d4e1a856","kind":"agent","description":"Build subagent list on rail","startedAt":1761291000000,"agentType":"general-purpose","transcript":"C:\\x\\5d1e7f20\\subagents\\agent-a3f09c2b7d4e1a856.jsonl"},
        {"id":"b1aaa","kind":"command","description":"Run the LBTerm check","startedAt":1761291060000},
        {"id":"m1ddd","kind":"monitor","description":"Wait for the build\nqueue","startedAt":1761291120000},
        {"id":"x1","kind":"something new","startedAt":1761291180000},
        {"id":"","kind":"agent"}]}"#;

    #[test]
    fn the_statuslines_list_is_read_as_it_writes_it() {
        let list = parse(FILE).unwrap();
        assert_eq!(list.session_id, "5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35");
        assert_eq!(list.tasks.len(), 4, "one without an id is left out");
        let agent = &list.tasks[0];
        assert_eq!(
            (agent.id.as_str(), agent.kind, agent.description.as_str()),
            ("a3f09c2b7d4e1a856", Kind::Agent, "Build subagent list on rail")
        );
        assert_eq!(agent.started_at, 1761291000000.0);
        assert_eq!(agent.agent_type.as_deref(), Some("general-purpose"));
        assert_eq!(
            agent.transcript.as_deref(),
            Some(Path::new("C:\\x\\5d1e7f20\\subagents\\agent-a3f09c2b7d4e1a856.jsonl"))
        );
        assert_eq!((list.tasks[1].kind, list.tasks[1].transcript.as_ref()), (Kind::Command, None));
        // One line only
        assert_eq!(list.tasks[2].description, "Wait for the build");
        // A kind it doesn't know is a command, with nothing to say
        assert_eq!((list.tasks[3].kind, list.tasks[3].description.as_str()), (Kind::Command, ""));
        assert_eq!(parse("{"), None);
        assert_eq!(parse("{}"), Some(List::default()));
    }

    #[test]
    fn a_workflow_run_is_a_workflow_with_no_second_line() {
        // Terminal's statusline names a Workflow run by its summary from 2026-11-09 (bf35e34)
        let list = parse(
            r#"{"sessionId":"s","tasks":[{"id":"ww49q3qsy","kind":"workflow","description":"New-tab landing 4","startedAt":1761291000000}]}"#,
        )
        .unwrap();
        assert_eq!(list.tasks[0].kind, Kind::Workflow);
        assert_eq!(Kind::from_name("workflow").word(), "Workflow");
        let (shown, _) = shown(&list, |_| panic!("a workflow has no transcript to read"), 1761291060000.0);
        assert_eq!((shown[0].kind, shown[0].description.as_str(), shown[0].detail.as_deref()), ("Workflow", "New-tab landing 4", None));
    }

    fn assistant(content: Value, usage: Value) -> String {
        json!({"type": "assistant", "isSidechain": true, "message": {"role": "assistant", "content": content, "usage": usage}})
            .to_string()
    }

    fn tool(name: &str, input: Value) -> Value {
        json!([{"type": "tool_use", "id": "toolu_x", "name": name, "input": input}])
    }

    fn usage(input: u64, read: u64, output: u64) -> Value {
        json!({"input_tokens": input, "cache_creation_input_tokens": 100, "cache_read_input_tokens": read, "output_tokens": output})
    }

    #[test]
    fn a_subagents_transcript_says_its_latest_tool_call_its_calls_and_tokens() {
        let lines = [
            json!({"type": "user", "message": {"role": "user", "content": "You're building..."}}).to_string(),
            assistant(tool("Read", json!({"file_path": "C:\\x\\lbterm\\TAB-PANEL.md"})), usage(2, 30000, 50)),
            json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "\"role\":\"assistant\" quoted"}]}}).to_string(),
            assistant(json!([{"type": "text", "text": "Now the code."}]), usage(2, 30500, 20)),
            assistant(tool("Grep", json!({"pattern": "owns_zindex", "path": "C:\\x"})), usage(2, 31000, 194)),
            json!({"type": "api-request", "params": {"model": "claude-opus-5-5"}}).to_string(),
        ];
        let text = lines.join("\n") + "\n";
        let mut follow = Follow::default();
        // Fed in pieces cut anywhere, a line at a time or across lines
        for chunk in text.as_bytes().chunks(37) {
            follow.feed(chunk);
        }
        assert_eq!(
            follow.progress(),
            &Progress {
                doing: Some("Grep owns_zindex".into()),
                tools: 2,
                tokens: Some(2 + 100 + 31000 + 194),
            }
        );
        // A line still being written waits for its end
        let more = assistant(tool("Edit", json!({"file_path": "/c/x/tab_panel.rs"})), usage(2, 32000, 10));
        let (head, tail) = more.as_bytes().split_at(40);
        follow.feed(head);
        assert_eq!(follow.progress().tools, 2);
        follow.feed(tail);
        follow.feed(b"\n");
        assert_eq!(follow.progress().doing.as_deref(), Some("Edit tab_panel.rs"));
        assert_eq!(follow.progress().tools, 3);
    }

    #[test]
    fn a_transcript_is_read_a_little_more_each_look_and_again_when_it_shrinks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-a1.jsonl");
        let mut follow = Follow::default();
        assert_eq!(follow.read_more(&path), None, "no file yet");
        std::fs::write(&path, assistant(tool("Read", json!({"file_path": "a.rs"})), usage(1, 1, 1)) + "\n").unwrap();
        assert_eq!(follow.read_more(&path).unwrap().doing.as_deref(), Some("Read a.rs"));
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        writeln!(f, "{}", assistant(tool("Read", json!({"file_path": "b.rs"})), usage(1, 1, 1))).unwrap();
        drop(f);
        let p = follow.read_more(&path).unwrap();
        assert_eq!((p.doing.as_deref(), p.tools), (Some("Read b.rs"), 2));
        // Written again from nothing: read again from its start
        std::fs::write(&path, assistant(tool("Glob", json!({"pattern": "*.rs"})), usage(1, 1, 1)) + "\n").unwrap();
        let p = follow.read_more(&path).unwrap();
        assert_eq!((p.doing.as_deref(), p.tools), (Some("Glob *.rs"), 1));
    }

    #[test]
    fn a_tool_call_in_a_few_words() {
        let d = |name: &str, input: Value| describe(name, &input);
        assert_eq!(d("Read", json!({"file_path": "C:\\x\\y\\tab_panel.rs"})), "Read tab_panel.rs");
        assert_eq!(d("Edit", json!({"file_path": "/c/x/lib.rs", "old_string": "a"})), "Edit lib.rs");
        assert_eq!(d("Write", json!({"file_path": "notes.md"})), "Write notes.md");
        assert_eq!(d("NotebookEdit", json!({"notebook_path": "a\\b.ipynb"})), "Edit b.ipynb");
        assert_eq!(d("Bash", json!({"command": "cargo test", "description": "Run the tests"})), "Run the tests");
        assert_eq!(d("PowerShell", json!({"command": "\n  Get-ChildItem x\nmore"})), "$ Get-ChildItem x");
        assert_eq!(d("Bash", json!({})), "Bash");
        assert_eq!(d("Grep", json!({"pattern": "fn  card"})), "Grep fn card");
        assert_eq!(d("Glob", json!({"pattern": "**/*.rs"})), "Glob **/*.rs");
        assert_eq!(d("WebFetch", json!({"url": "https://omp.sh/docs/subagents"})), "Fetch omp.sh");
        assert_eq!(d("WebSearch", json!({"query": "agent hub"})), "Search agent hub");
        assert_eq!(d("Agent", json!({"description": "Review the card", "prompt": "..."})), "Agent Review the card");
        assert_eq!(d("Skill", json!({"skill": "lbterm-motion"})), "Skill lbterm-motion");
        assert_eq!(d("Monitor", json!({"command": "tail -f x"})), "Monitor");
        assert_eq!(d("mcp__zigbee__set_light", json!({})), "zigbee set_light");
        assert_eq!(d("TodoWrite", json!({"todos": []})), "Todo list");
        assert_eq!(d("ToolSearch", json!({"query": "select:Monitor"})), "ToolSearch");
    }

    #[test]
    fn ages_and_tokens_in_a_few_characters() {
        assert_eq!(age(0.0), "0s");
        assert_eq!(age(42.7), "42s");
        assert_eq!(age(-3.0), "0s", "a clock that went back");
        assert_eq!(age(60.0), "1m");
        assert_eq!(age(12.0 * 60.0 + 59.0), "12m");
        assert_eq!(age(3600.0 + 5.0 * 60.0), "1h 5m");
        assert!((age_changes_in(42.25) - 0.75).abs() < 1e-9);
        assert_eq!(age_changes_in(61.0), 59.0);
        assert_eq!(age_changes_in(3661.0), 59.0);
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(84_400), "84k");
        assert_eq!(tokens(999_499), "999k");
        assert_eq!(tokens(1_200_000), "1.2M");
        assert_eq!(tokens(2_000_000), "2M");
    }

    #[test]
    fn the_card_lists_each_task_and_a_subagent_says_what_its_doing() {
        let list = parse(FILE).unwrap();
        let now = 1761291000000.0 + 12.0 * 60_000.0 + 30_000.0;
        let progress = |p: &Path| {
            p.ends_with("agent-a3f09c2b7d4e1a856.jsonl").then(|| Progress {
                doing: Some("Edit tab_panel.rs".into()),
                tools: 31,
                tokens: Some(84_400),
            })
        };
        let (shown, next) = shown(&list, progress, now);
        assert_eq!(shown.len(), 4);
        assert_eq!(
            shown[0],
            Shown {
                kind: "Agent",
                description: "Build subagent list on rail".into(),
                age: "12m".into(),
                detail: Some("Edit tab_panel.rs \u{b7} 31 tools \u{b7} 84k tokens".into()),
            }
        );
        assert_eq!((shown[1].kind, shown[1].age.as_str(), shown[1].detail.as_ref()), ("Command", "11m", None));
        assert_eq!(shown[2].kind, "Monitor");
        // No description: its kind
        assert_eq!(shown[3].description, "command");
        // Every age is in minutes here, 30 s into one
        assert_eq!(next, Some(30.0));

        // Its transcript not read yet: its type, until it says more
        let (shown, _) = super::shown(&list, |_| None, now);
        assert_eq!(shown[0].detail.as_deref(), Some("general-purpose"));
        let (shown, _) = super::shown(&list, |_| Some(Progress::default()), now);
        assert_eq!(shown[0].detail.as_deref(), Some("general-purpose"));
    }

    fn task(kind: &'static str, description: &str, age: &str, detail: Option<&str>) -> Shown {
        Shown {
            kind,
            description: description.into(),
            age: age.into(),
            detail: detail.map(String::from),
        }
    }

    #[test]
    fn the_card_lays_out_beside_the_row_its_ages_on_the_right() {
        let shown = [
            task("Agent", "Build subagent list on rail", "12m", Some("Edit tab_panel.rs \u{b7} 31 tools \u{b7} 84k tokens")),
            task("Command", "Run the LBTerm check", "1h 5m", None),
        ];
        let cell = (8.0, 16.0);
        let card = card(&shown, 300.0, 100.0, (40.0, 900.0), 1600.0, cell, 1.0).unwrap();
        // Labels two columns past the widest kind ("Command")
        assert_eq!(card.label_x, 312.0);
        assert_eq!(card.text_x, 312.0 + 9.0 * 8.0);
        assert_eq!(card.rect.x, 300.0);
        assert_eq!(card.rect.y, 100.0);
        // The widest line is the subagent's: 41 columns past the labels
        assert_eq!(card.rect.w, 24.0 + (9 + 41) as f64 * 8.0);
        // Three lines, and a gap between the tasks
        assert_eq!(card.rect.h, 24.0 + 3.0 * 16.0 + 6.0);
        let ys: Vec<f64> = card.lines.iter().map(|l| l.y).collect();
        assert_eq!(ys, vec![112.0, 128.0, 150.0]);
        assert_eq!(card.lines[0].label.as_deref(), Some("Agent"));
        assert_eq!(card.lines[1].tone, Tone::Detail);
        assert_eq!(card.lines[1].label, None);
        // Each age ends at the card's right padding
        let right = card.rect.x + card.rect.w - 12.0;
        let (age, x) = card.lines[0].right.clone().unwrap();
        assert_eq!((age.as_str(), x), ("12m", right - 3.0 * 8.0));
        let (age, x) = card.lines[2].right.clone().unwrap();
        assert_eq!((age.as_str(), x), ("1h 5m", right - 5.0 * 8.0));
        assert_eq!(card.lines[1].right, None);
    }

    #[test]
    fn the_card_stays_in_the_window_and_cuts_what_doesnt_fit() {
        let long = "a description far longer than the card has room for on one line of it, and more";
        let shown = [task("Agent", long, "3m", Some(long))];
        let cell = (8.0, 16.0);
        // Room for 40 columns between its left and the window's right
        let max_right = 300.0 + 24.0 + 40.0 * 8.0;
        let c = card(&shown, 300.0, 100.0, (40.0, 900.0), max_right, cell, 1.0).unwrap();
        assert!(c.rect.x + c.rect.w <= max_right);
        assert!(c.lines[0].text.ends_with('\u{2026}'));
        // Labels (7) + description + gap (2) + age (2) come to the room
        assert_eq!(c.lines[0].text.chars().count(), 40 - 7 - 2 - 2);
        // The subagent's line takes the age's columns too
        assert_eq!(c.lines[1].text.chars().count(), 40 - 7);
        // Low in the window it moves up, its foot at the bottom, never above min_top
        let c = card(&shown, 300.0, 880.0, (40.0, 900.0), 1600.0, cell, 1.0).unwrap();
        assert_eq!(c.rect.y + c.rect.h, 900.0);
        let c = card(&shown, 300.0, 50.0, (40.0, 70.0), 1600.0, cell, 1.0).unwrap();
        assert_eq!(c.rect.y, 40.0);
        // Many tasks: the last line says how many more
        let many: Vec<Shown> = (0..11).map(|i| task("Command", &format!("job {i}"), "1m", None)).collect();
        let c = card(&many, 0.0, 0.0, (0.0, 2000.0), 1600.0, cell, 1.0).unwrap();
        assert_eq!(c.lines.len(), MAX_TASKS);
        assert_eq!(c.lines.last().unwrap().text, "+4 more");
        assert_eq!(c.lines.last().unwrap().tone, Tone::More);
        assert_eq!(card(&[], 0.0, 0.0, (0.0, 900.0), 1600.0, cell, 1.0), None);
    }

    #[test]
    fn a_card_kept_open_doesnt_get_narrower_and_its_ages_keep_to_its_edge() {
        let cell = (8.0, 16.0);
        let shown = [task("Agent", "Review", "3m", Some("Read a.rs"))];
        let mut c = card(&shown, 300.0, 100.0, (40.0, 900.0), 1600.0, cell, 1.0).unwrap();
        let (w, age_x) = (c.rect.w, c.lines[0].right.as_ref().unwrap().1);
        widen(&mut c, w + 40.0, 1600.0);
        assert_eq!(c.rect.w, w + 40.0);
        assert_eq!(c.lines[0].right.as_ref().unwrap().1, age_x + 40.0);
        // Never past the window's room, and never narrower than it is
        widen(&mut c, 5000.0, 1000.0);
        assert_eq!(c.rect.x + c.rect.w, 1000.0);
        widen(&mut c, 10.0, 1600.0);
        assert_eq!(c.rect.x + c.rect.w, 1000.0);
    }

    /// Waits for `done`, up to five seconds.
    fn until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        done()
    }

    #[test]
    fn transcripts_asked_for_are_read_off_thread_and_again_when_they_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-a1.jsonl");
        std::fs::write(&path, assistant(tool("Read", json!({"file_path": "a.rs"})), usage(1, 1, 1)) + "\n").unwrap();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let transcripts = Transcripts::start(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });
        // Not read yet: asked for, and read on the other thread
        assert_eq!(transcripts.get(&path), None);
        assert!(until(|| transcripts.get(&path).and_then(|p| p.doing).as_deref() == Some("Read a.rs")));
        // Woken once it's known (just after: the wake follows the store)
        assert!(until(|| woken.load(Ordering::SeqCst) == 1));
        let generation = transcripts.generation();
        assert!(generation > 0);

        // It grows: read again from where it stopped, with nobody asking, within moments
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        writeln!(f, "{}", assistant(tool("Read", json!({"file_path": "b.rs"})), usage(1, 1, 1))).unwrap();
        drop(f);
        let grew = Instant::now();
        assert!(until(|| transcripts.generation() > generation && woken.load(Ordering::SeqCst) == 2));
        assert!(grew.elapsed() < Duration::from_secs(1), "read {:?} after it grew", grew.elapsed());
        assert_eq!(transcripts.get(&path).map(|p| p.tools), Some(2));

        // A file beside it changing: nobody woken
        let before = woken.load(Ordering::SeqCst);
        std::fs::write(dir.path().join("agent-b2.jsonl"), "{}\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(woken.load(Ordering::SeqCst), before);

        // A file that isn't there yet is known as nothing, without a wake, and read once it comes
        let missing = dir.path().join("agent-none.jsonl");
        assert_eq!(transcripts.get(&missing), None);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(transcripts.get(&missing), None);
        assert_eq!(woken.load(Ordering::SeqCst), before);
        std::fs::write(&missing, assistant(tool("Grep", json!({"pattern": "x"})), usage(1, 1, 1)) + "\n").unwrap();
        assert!(until(|| transcripts.get(&missing).map(|p| p.tools) == Some(1)));
    }

    #[test]
    fn a_transcript_no_longer_watched_is_read_on_from_where_it_stopped_when_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-a1.jsonl");
        let first = assistant(tool("Read", json!({"file_path": "a.rs"})), usage(1, 1, 1));
        std::fs::write(&path, first.clone() + "\n").unwrap();
        let transcripts = Transcripts::start_forgetting(Duration::from_millis(200), || {});
        assert_eq!(transcripts.get(&path), None);
        assert!(until(|| transcripts.get(&path).map(|p| p.tools) == Some(1)));

        // Nobody asks for it a while (the card closed): it's no longer watched, and a line added
        // then isn't read
        std::thread::sleep(Duration::from_millis(600));
        // Its first line made into one the reader passes over, its length kept, and a second
        // added: read on from where it stopped, the first is still counted; read from its start,
        // it wouldn't be
        let passed = first.replacen(r#""role":"assistant""#, r#""role":"xssistant""#, 1);
        assert_eq!(passed.len(), first.len());
        let second = assistant(tool("Edit", json!({"file_path": "b.rs"})), usage(1, 1, 1));
        std::fs::write(&path, format!("{passed}\n{second}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(transcripts.get(&path).map(|p| p.tools), Some(1), "what was read of it, while it wasn't watched");

        // Asked for again (the card opened): watched again, and read on
        assert!(until(|| transcripts.get(&path).is_some_and(|p| p.tools == 2 && p.doing.as_deref() == Some("Edit b.rs"))));
    }

    #[test]
    fn a_long_transcript_is_read_to_its_end_in_one_look() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-long.jsonl");
        // Over READ_MAX of records the reader passes over, then a tool call
        let filler = format!("{{\"type\":\"progress\",\"data\":\"{}\"}}\n", "x".repeat(1000));
        let mut text = filler.repeat((READ_MAX as usize / filler.len()) + 10);
        text.push_str(&(assistant(tool("Edit", json!({"file_path": "c.rs"})), usage(1, 1, 1)) + "\n"));
        std::fs::write(&path, text).unwrap();
        let mut follow = Follow::default();
        let p = follow.read_more(&path).unwrap();
        assert_eq!((p.tools, p.doing.as_deref()), (1, Some("Edit c.rs")));
    }
}
