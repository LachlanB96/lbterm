//! LBTerm: what a Claude Code conversation has cost at list price and what spent it, for the cost
//! pane the context pill's press opens (lbterm/CONTEXT.md, "The cost pane"; Lachlan's 7c of
//! 2026-11-09).
//!
//! Counted the way Terminal's statusline counts the dollars it writes to `<session>.usage.json`,
//! which the pill shows (the Claude project's count), so the two totals agree:
//!
//! - Every `assistant` record with a `message.usage`, in the conversation's transcript and in every
//!   subagent transcript in the folder beside it (`<sid>/`: any `.jsonl` but a workflow's
//!   `journal.jsonl`, in that folder or up to [`AGENT_DEPTH`] folders under it).
//! - A record whose `message.id` is the last assistant record's (one message written as a record a
//!   content block, other records between them or not) replaces that one's count; one without an
//!   id is a request of its own.
//! - Priced per million tokens at [`PRICES`] by the model's family (`opus`, `fable`, `sonnet`,
//!   `haiku` anywhere in its name, any case; any other model costs nothing), but for a model that
//!   lists apart from its family, which has a row of its own in [`MODEL_PRICES`] (Fable 5.2 and
//!   Sonnet 5.5): input,
//!   output and cache reads at their prices, cache writes at 1.25 times input for the 5-minute cache
//!   and twice it for the hour's, by `usage.cache_creation`'s two counts when it has them, else all
//!   of `cache_creation_input_tokens` at the 5-minute price.
//!
//! What spent it ([`Spender`]): the conversation's own transcript is his turns, a transcript under
//! a `workflows` folder is its workflow's (each folder after `workflows` one workflow), and any
//! other is a subagent's. Compactions are counted (`compact_boundary` records) but cost nothing
//! here: Claude Code records no usage for a compaction's request, so neither count has it. Each
//! one's sizes are kept ([`Compaction`]): the context it compacted (the record's
//! `compactMetadata.preTokens`) and the context of the first request after it, the one the
//! conversation carried on with, as the Claude project's split has them (`before`, `next`).
//!
//! [`Fold`] keeps what's been read of one file and reads on from there, so the pane counts only
//! what's new each time; [`Cost`] is made from the folds, and [`pane`] lays the pane out.
//!
//! All that is LBTerm's own count, which runs only with `lbterm_context.count_cost` on. Off (the
//! default; Lachlan's 2b of 2026-11-10) the pane shows the Claude project's split instead,
//! `@Claude:cost`: `<sid>.cost.json`, which Terminal's statusline writes beside `<sid>.usage.json`
//! in the summaries folder. [`read_published`] reads it and [`published`] makes the same [`Cost`]
//! of it, which [`pane`] lays out as it lays out the count: nothing in it is counted here.

use crate::tabs::Rect;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Dollars a million tokens by model family: input, output, cache reads (the Claude project's
/// list prices, which Terminal's statusline counts with; `.claude/tests/cost-prices.test.ts`
/// holds this to them)
pub const PRICES: [(&str, [f64; 3]); 4] = [
    ("opus", [4.0, 20.0, 0.2]),
    ("fable", [10.0, 50.0, 0.25]),
    ("sonnet", [2.0, 10.0, 0.2]),
    ("haiku", [0.1, 0.5, 0.01]),
];
/// The models that list apart from their family, the Claude project's `MODEL_PRICES`: a model whose
/// name holds the row's, any case, with no digit straight after it (`fable-5-2` is Fable 5.2 and
/// its dated and `[1m]` names, never a `fable-5-20`). Fable 5.2 lists a fifth under Fable 5 and 5.1
/// (a `claude -p` run of 2026-11-11 came to these prices to the last digit, and the family's row
/// read it a quarter high), and Sonnet 5.5's cache reads are half its family's (their runs of
/// 2026-11-12). The same test holds these rows, and how a name is matched, to theirs
pub const MODEL_PRICES: [(&str, [f64; 3]); 2] = [
    ("fable-5-2", [8.0, 40.0, 0.2]),
    ("sonnet-5-5", [2.0, 10.0, 0.1]),
];
/// A cache write's price over input's: the 5-minute cache's, the hour's
pub const WRITE_5M: f64 = 1.25;
pub const WRITE_1H: f64 = 2.0;
/// How many folders under `<sid>/` a subagent transcript may be found in (a workflow's agents are
/// three down, `subagents/workflows/<run>/`)
pub const AGENT_DEPTH: usize = 5;

/// Tokens of each type
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tokens {
    pub input: u64,
    pub writes: u64,
    pub reads: u64,
    pub output: u64,
}

/// Dollars for each type of token
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Dollars {
    pub input: f64,
    pub writes: f64,
    pub reads: f64,
    pub output: f64,
}

impl Dollars {
    pub fn total(&self) -> f64 {
        self.input + self.writes + self.reads + self.output
    }
}

/// One request: its tokens, their dollars, and when its reply was recorded (seconds since 1970;
/// `None` for a record with no timestamp)
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Request {
    pub tokens: Tokens,
    pub usd: Dollars,
    pub at: Option<f64>,
}

/// A model's prices: its own row when it lists apart from its family, else its family's (the
/// Claude project's `priceOf`); `None` for a model neither table knows
fn price(model: &str) -> Option<[f64; 3]> {
    let model = model.to_ascii_lowercase();
    let own = MODEL_PRICES.iter().find(|(name, _)| {
        model
            .match_indices(name)
            .any(|(at, _)| !model[at + name.len()..].starts_with(|c: char| c.is_ascii_digit()))
    });
    own.or_else(|| PRICES.iter().find(|(f, _)| model.contains(f))).map(|(_, p)| *p)
}

fn count(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// A record's `message.usage` priced for `model`
pub fn priced(model: &str, usage: &Value) -> Request {
    let tokens = Tokens {
        input: count(usage, "input_tokens"),
        writes: count(usage, "cache_creation_input_tokens"),
        reads: count(usage, "cache_read_input_tokens"),
        output: count(usage, "output_tokens"),
    };
    let usd = match price(model) {
        None => Dollars::default(),
        Some([input, output, reads]) => {
            let m = 1e6;
            let writes = match usage.get("cache_creation").filter(|c| c.is_object()) {
                Some(c) => {
                    count(c, "ephemeral_5m_input_tokens") as f64 * input * WRITE_5M
                        + count(c, "ephemeral_1h_input_tokens") as f64 * input * WRITE_1H
                }
                None => tokens.writes as f64 * input * WRITE_5M,
            };
            Dollars {
                input: tokens.input as f64 * input / m,
                writes: writes / m,
                reads: tokens.reads as f64 * reads / m,
                output: tokens.output as f64 * output / m,
            }
        }
    };
    Request { tokens, usd, at: None }
}

fn seconds(v: &Value) -> Option<f64> {
    let t = v.get("timestamp")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(t).ok().map(|d| d.timestamp_millis() as f64 / 1000.0)
}

/// What's been read of one transcript: its requests and compactions, and where to read on from
#[derive(Debug, Default, Clone)]
pub struct Fold {
    /// Bytes taken: up to the end of the last whole line
    pub off: u64,
    pub requests: Vec<Request>,
    /// Each `compact_boundary`, in the order recorded
    pub compactions: Vec<Compaction>,
    /// The last assistant record's message id, and its request's index
    last: Option<(String, usize)>,
    /// The last compaction while no request has been made since it: the next one's context is
    /// its `next`
    waiting: Option<usize>,
}

/// A compaction: when it was recorded, the context it compacted, and the context of the first
/// request after it, the one the conversation carried on with (the context bar's in use then)
#[derive(Debug, Clone, PartialEq)]
pub struct Compaction {
    /// Seconds since 1970
    pub at: Option<f64>,
    /// The context it compacted: Claude Code's `compactMetadata.preTokens`, the split's `before`
    pub before: Option<u64>,
    /// The first request's input, cache writes and cache reads after it, the split's `next`;
    /// `None` until that request is made (and in a split written before 2026-11-13)
    pub next: Option<u64>,
}

impl Compaction {
    /// Its name on the graph: `545k → 55k`, or `545k → …` until the request after it is made;
    /// none when Claude Code recorded no size for it
    pub fn label(&self) -> Option<String> {
        let before = tokens(self.before?);
        Some(match self.next {
            Some(next) => format!("{before} → {}", tokens(next)),
            None => format!("{before} → …"),
        })
    }
}

impl Fold {
    /// Fold `buf`'s whole lines; the bytes taken (a line not yet ended is left for next time)
    pub fn fold(&mut self, buf: &[u8]) -> usize {
        let Some(end) = memchr::memrchr(b'\n', buf).map(|i| i + 1) else {
            return 0;
        };
        for line in buf[..end].split(|b| *b == b'\n') {
            self.line(line);
        }
        self.off += end as u64;
        end
    }

    fn line(&mut self, line: &[u8]) {
        let assistant = memchr::memmem::find(line, b"\"assistant\"").is_some();
        let boundary = memchr::memmem::find(line, b"compact_boundary").is_some();
        if !assistant && !boundary {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let Some(message) = v.get("message") else {
                    return;
                };
                let Some(usage) = message.get("usage").filter(|u| u.is_object()) else {
                    return;
                };
                let model = message.get("model").and_then(Value::as_str).unwrap_or("");
                let mut request = priced(model, usage);
                request.at = seconds(&v);
                // A `<synthetic>` record was never sent, so it isn't the request carried on with
                if model != "<synthetic>" {
                    if let Some(i) = self.waiting.take() {
                        let t = &request.tokens;
                        self.compactions[i].next = Some(t.input + t.writes + t.reads);
                    }
                }
                let id = message.get("id").and_then(Value::as_str);
                match (id, self.last.as_ref()) {
                    (Some(id), Some((last, i))) if last == id => self.requests[*i] = request,
                    _ => {
                        self.requests.push(request);
                        self.last = id.map(|id| (id.to_string(), self.requests.len() - 1));
                    }
                }
            }
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") => {
                let before = v.pointer("/compactMetadata/preTokens").and_then(Value::as_u64);
                self.compactions.push(Compaction { at: seconds(&v), before, next: None });
                self.waiting = Some(self.compactions.len() - 1);
            }
            _ => {}
        }
    }

    /// Read on from where it left off; the whole file again when it's shorter than that (written
    /// again). True when what it holds may have changed: anything was taken, or it was emptied
    /// for a file written again (even one written again empty). False only when the file is
    /// where it was left, so a count made from it needn't be made again (wezterm-gui's cost.rs)
    pub fn read(&mut self, path: &Path) -> std::io::Result<bool> {
        let mut file = std::fs::File::open(path)?;
        let size = file.metadata()?.len();
        let again = size < self.off;
        if again {
            *self = Fold::default();
        }
        if size == self.off {
            return Ok(again);
        }
        file.seek(SeekFrom::Start(self.off))?;
        let mut buf = Vec::with_capacity((size - self.off) as usize);
        file.take(size - self.off).read_to_end(&mut buf)?;
        Ok(self.fold(&buf) > 0 || again)
    }
}

/// What spent it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Spender {
    Turns,
    Subagents,
    Workflows,
}

impl Spender {
    pub const ALL: [Spender; 3] = [Spender::Turns, Spender::Subagents, Spender::Workflows];

    pub fn name(self) -> &'static str {
        match self {
            Spender::Turns => "turns",
            Spender::Subagents => "subagents",
            Spender::Workflows => "workflows",
        }
    }

    /// Its colour: his turns the conversation's blue-grey on the context bar, subagents the
    /// working green's teal, workflows the rail's idle yellow
    pub fn colour(self) -> u32 {
        match self {
            Spender::Turns => 0x7A9CC6,
            Spender::Subagents => 0x4FC9B0,
            Spender::Workflows => 0xF2E86D,
        }
    }
}

/// The compactions' grey on the pane
pub const COMPACTIONS: u32 = 0x8A8A9E;

/// A subagent transcript under `<sid>/` (`rel` its path from there): a workflow's (with the
/// workflow's folder) or a subagent's
pub fn spender_of(rel: &Path) -> (Spender, Option<String>) {
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match parts.iter().position(|p| p == "workflows") {
        Some(i) if i + 2 < parts.len() => (Spender::Workflows, Some(parts[i + 1].clone())),
        _ => (Spender::Subagents, None),
    }
}

/// Every subagent transcript of the conversation whose transcript is `main`: the `.jsonl` files
/// in `<sid>/` and up to [`AGENT_DEPTH`] folders under it but `journal.jsonl`, sorted
pub fn agent_transcripts(main: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        let Ok(list) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in list.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => {
                    if depth < AGENT_DEPTH {
                        walk(&path, depth + 1, found);
                    }
                }
                Ok(_) => {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.ends_with(".jsonl") && name != "journal.jsonl" {
                        found.push(path);
                    }
                }
                Err(_) => {}
            }
        }
    }
    let mut found = vec![];
    if let (Some(dir), Some(stem)) = (main.parent(), main.file_stem()) {
        walk(&dir.join(stem), 0, &mut found);
    }
    found.sort();
    found
}

/// One spender's share
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Part {
    pub tokens: Tokens,
    pub usd: Dollars,
    /// Subagents or workflows that spent anything (none for his turns)
    pub runs: usize,
}

/// Where a subagent or a workflow started, on the graph
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub spender: Spender,
    pub at: f64,
}

/// The conversation's cost, split
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cost {
    /// By `Spender::ALL`'s order
    pub parts: [Part; 3],
    /// The main transcript's compactions, in time order
    pub compactions: Vec<Compaction>,
    /// The total after each request, in time order: (seconds since 1970, dollars so far); from
    /// the Claude project's split one a minute something was spent in, the minute's start with
    /// the total at its end
    pub steps: Vec<(f64, f64)>,
    pub marks: Vec<Mark>,
    /// The total as the Claude project's split gives it (its `usd`, to the cent, the pill's);
    /// `None` for LBTerm's own count, whose total is its parts'
    pub usd: Option<f64>,
}

impl Cost {
    pub fn total(&self) -> f64 {
        self.usd.unwrap_or_else(|| self.parts.iter().map(|p| p.usd.total()).sum())
    }

    pub fn part(&self, spender: Spender) -> &Part {
        &self.parts[Spender::ALL.iter().position(|s| *s == spender).unwrap_or(0)]
    }

    /// The total at `t`: the last step at or before it
    pub fn at(&self, t: f64) -> f64 {
        match self.steps.partition_point(|(at, _)| *at <= t) {
            0 => 0.0,
            i => self.steps[i - 1].1,
        }
    }
}

/// Add `fold`'s requests to `spender`'s part, and each to `all` with its time
fn add(c: &mut Cost, spender: Spender, fold: &Fold, all: &mut Vec<(Option<f64>, f64)>) {
    let part = &mut c.parts[Spender::ALL.iter().position(|s| *s == spender).unwrap_or(0)];
    let mut at = None;
    for r in &fold.requests {
        part.tokens.input += r.tokens.input;
        part.tokens.writes += r.tokens.writes;
        part.tokens.reads += r.tokens.reads;
        part.tokens.output += r.tokens.output;
        part.usd.input += r.usd.input;
        part.usd.writes += r.usd.writes;
        part.usd.reads += r.usd.reads;
        part.usd.output += r.usd.output;
        // A record without a time is placed at the last one before it in its file
        at = r.at.or(at);
        all.push((at, r.usd.total()));
    }
}

/// The cost of a conversation: `main` its own transcript's fold, `agents` each subagent
/// transcript's with what spent it
pub fn cost(main: &Fold, agents: &[(Spender, Option<String>, &Fold)]) -> Cost {
    let mut c = Cost::default();
    let mut all: Vec<(Option<f64>, f64)> = vec![];
    let mut workflows: Vec<(String, Option<f64>)> = vec![];
    add(&mut c, Spender::Turns, main, &mut all);
    c.compactions = main.compactions.clone();
    for (spender, workflow, fold) in agents {
        if fold.requests.is_empty() {
            continue;
        }
        add(&mut c, *spender, fold, &mut all);
        let first = fold.requests.iter().find_map(|r| r.at);
        match (spender, workflow) {
            (Spender::Workflows, Some(w)) => match workflows.iter_mut().find(|(name, _)| name == w) {
                Some((_, at)) => {
                    *at = match (*at, first) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    }
                }
                None => workflows.push((w.clone(), first)),
            },
            _ => {
                c.parts[1].runs += 1;
                if let Some(at) = first {
                    c.marks.push(Mark { spender: Spender::Subagents, at });
                }
            }
        }
    }
    c.parts[2].runs = workflows.len();
    c.marks.extend(workflows.iter().filter_map(|(_, at)| at.map(|at| Mark { spender: Spender::Workflows, at })));
    c.marks.sort_by(|a, b| a.at.total_cmp(&b.at));
    // Requests with no time at all go at the conversation's start
    let start = all.iter().filter_map(|(at, _)| *at).fold(f64::INFINITY, f64::min);
    let mut timed: Vec<(f64, f64)> = all.into_iter().map(|(at, usd)| (at.unwrap_or(start), usd)).filter(|(at, _)| at.is_finite()).collect();
    timed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut sum = 0.0;
    c.steps = timed
        .into_iter()
        .map(|(at, usd)| {
            sum += usd;
            (at, sum)
        })
        .collect();
    c
}

/// Whether `at` keeps a column clear of every rect of `placed`, or is above or below it
fn free(at: &Rect, placed: &[Rect], col: f64) -> bool {
    placed.iter().all(|r| r.x + r.w + col <= at.x || at.x + at.w + col <= r.x || r.y + r.h <= at.y || at.y + at.h <= r.y)
}

/// The version of the Claude project's split this reads (`@Claude:cost`'s `SPLIT_VERSION`: a
/// change of the file's shape bumps it, and is told to LBTerm by InMail)
pub const SPLIT_VERSION: u64 = 1;

/// One spender in the Claude project's split: the fields the pane shows (its id, label, end,
/// requests and own `usd` are read past)
#[derive(Debug, serde::Deserialize)]
struct Spent {
    kind: String,
    run: Option<String>,
    /// Its first request's time, ms
    start: f64,
    /// Input, cache writes, cache reads, output
    tokens: [u64; 4],
    dollars: [f64; 4],
}

/// `<sid>.cost.json` as version 1 has it (`api Claude cost`): each compaction's time and sizes (its
/// trigger and `after`, the summary's own size, are read past)
#[derive(Debug, serde::Deserialize)]
struct Split {
    usd: f64,
    spenders: Vec<Spent>,
    compactions: Vec<Compacted>,
    /// The minute's start (ms) and the running total at its end
    steps: Vec<(f64, f64)>,
}

/// One compaction in the Claude project's split: `at` in ms, `before` the context it compacted,
/// `next` the first request's after it (null until it's made; absent from a split written before
/// 2026-11-13, a field added, so no new version)
#[derive(Debug, serde::Deserialize)]
struct Compacted {
    at: f64,
    #[serde(default)]
    before: Option<u64>,
    #[serde(default)]
    next: Option<u64>,
}

/// The start of what the pane says when the Claude project's split isn't in the shape it reads
pub const UNREADABLE: &str = "The Claude project's split can't be read";

/// The Claude project's split of a conversation's cost, `<sid>.cost.json`'s text, made into the
/// [`Cost`] the pane lays out as it lays out LBTerm's own count: his turns (`main`), the
/// subagents (each one a run), the workflows (each `run` one, however many agents it ran; agents
/// with no run are one between them), each with its tokens and dollars by type added up from its
/// spenders; each compaction's time and sizes; the steps; a mark where each subagent and each run started;
/// and its total the file's `usd`, to the cent, as the pill has it. Nothing else is counted. A
/// version other than [`SPLIT_VERSION`], or text not in its shape, is the words saying so
pub fn published(text: &str) -> Result<Cost, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("{UNREADABLE}: {e}"))?;
    match value.get("v").and_then(Value::as_u64) {
        Some(SPLIT_VERSION) => {}
        Some(v) => return Err(format!("The Claude project's split is version {v}, and this LBTerm reads version {SPLIT_VERSION}")),
        None => return Err(format!("{UNREADABLE}: it says no version")),
    }
    let split: Split = serde_json::from_value(value).map_err(|e| format!("{UNREADABLE}: {e}"))?;
    let mut compactions: Vec<Compaction> = split.compactions.iter().map(|k| Compaction { at: Some(k.at / 1000.0), before: k.before, next: k.next }).collect();
    compactions.sort_by(|a, b| a.at.unwrap_or(0.0).total_cmp(&b.at.unwrap_or(0.0)));
    let mut c = Cost { usd: Some(split.usd), compactions, ..Cost::default() };
    let mut runs: Vec<(Option<String>, f64)> = vec![];
    for s in &split.spenders {
        let spender = match s.kind.as_str() {
            "main" => Spender::Turns,
            "subagent" => Spender::Subagents,
            "workflow" => Spender::Workflows,
            other => return Err(format!("{UNREADABLE}: a spender of kind {other:?}")),
        };
        let part = &mut c.parts[Spender::ALL.iter().position(|x| *x == spender).unwrap_or(0)];
        let [input, writes, reads, output] = s.tokens;
        part.tokens.input += input;
        part.tokens.writes += writes;
        part.tokens.reads += reads;
        part.tokens.output += output;
        let [input, writes, reads, output] = s.dollars;
        part.usd.input += input;
        part.usd.writes += writes;
        part.usd.reads += reads;
        part.usd.output += output;
        let at = s.start / 1000.0;
        match spender {
            Spender::Turns => {}
            Spender::Subagents => {
                part.runs += 1;
                c.marks.push(Mark { spender, at });
            }
            Spender::Workflows => match runs.iter_mut().find(|(run, _)| *run == s.run) {
                Some((_, first)) => *first = first.min(at),
                None => runs.push((s.run.clone(), at)),
            },
        }
    }
    c.parts[2].runs = runs.len();
    c.marks.extend(runs.iter().map(|(_, at)| Mark { spender: Spender::Workflows, at: *at }));
    c.marks.sort_by(|a, b| a.at.total_cmp(&b.at));
    c.steps = split.steps.iter().map(|(at, usd)| (at / 1000.0, *usd)).collect();
    c.steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(c)
}

/// The Claude project's split of session `sid`'s cost, read from `folder`, the summaries folder
/// (`lbterm_summaries`), where Terminal's statusline writes `<sid>.cost.json` whole (a rename)
/// beside `<sid>.usage.json`. One file read whole and parsed, no transcript: its absence is
/// [`NOT_PUBLISHED`] (not written yet for this conversation, never $0), and anything else it
/// can't give is what went wrong
pub fn read_published(folder: &Path, sid: &str) -> Result<Cost, String> {
    if !crate::restore::is_session_id(sid) {
        return Err(format!("{UNREADABLE}: {sid:?} isn't a session's id"));
    }
    match std::fs::read_to_string(folder.join(format!("{sid}.cost.json"))) {
        Ok(text) => published(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(NOT_PUBLISHED.to_string()),
        Err(e) => Err(format!("{UNREADABLE}: {e}")),
    }
}

/// Tokens as the pane writes them: `950`, `1.2k`, `340k`, `8.2M`, `12M`
pub fn tokens(n: u64) -> String {
    let n = n as f64;
    if n < 1e3 {
        format!("{n}")
    } else if n < 1e4 {
        format!("{:.1}k", (n / 1e2).floor() / 10.0)
    } else if n < 1e6 {
        format!("{}k", (n / 1e3).floor())
    } else if n < 1e7 {
        format!("{:.1}M", (n / 1e5).floor() / 10.0)
    } else {
        format!("{}M", (n / 1e6).floor())
    }
}

/// Dollars as the pane writes them, to the cent (the pill's way, which drops the cents from $100)
pub fn dollars(usd: f64) -> String {
    let usd = if usd.is_finite() { usd.max(0.0) } else { 0.0 };
    format!("${:.2}", (usd * 100.0 + 1e-6).floor() / 100.0)
}

/// What a word or a fill on the pane is, for its colour
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tone {
    /// The total, the brightest
    Total,
    /// A row's words
    Text,
    /// Headings, notes, the token counts, the axes' labels
    Dim,
    /// A spender's colour: its square, its share of the bar, its marks
    Part(Spender),
    Compactions,
    /// The graph's grid lines
    Grid,
    /// The graph's line
    Line,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub x: f64,
    pub y: f64,
    /// Room for it
    pub w: f64,
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    pub rect: Rect,
    pub tone: Tone,
    /// Drawn round (a mark's dot)
    pub round: bool,
}

/// The pane laid out
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pane {
    pub rect: Rect,
    pub words: Vec<Word>,
    pub fills: Vec<Fill>,
    /// Each spender's row, then the compactions'
    pub rows: Vec<Rect>,
    /// The graph's plot, when there's room for one and anything to draw
    pub graph: Option<Rect>,
    /// The words the pane says while it has nothing to show (`COUNTING`)
    pub note: Option<String>,
}

pub const COUNTING: &str = "Counting what this conversation has cost…";
pub const HEADING: &str = "this conversation, at list price";
pub const OVER_TIME: &str = "Over time";
pub const NOT_RECORDED: &str = "not recorded: Claude Code keeps no usage for them";
/// With LBTerm's own count off (the default; Lachlan's 2b of 2026-11-10): while the Claude
/// project's split is read, the first time the pane opens on a conversation
pub const READING: &str = "Reading what spent it from the Claude project…";
/// In place of the split while the Claude project hasn't written `<sid>.cost.json`
pub const NOT_PUBLISHED: &str = "The Claude project hasn't published what spent it for this conversation yet";
/// In place of the split without a summaries folder (`lbterm_summaries`) to read it from
pub const NO_SUMMARIES: &str = "No summaries folder (lbterm_summaries) to read the Claude project's split from";
/// After the context window's size, on the pane with the count off
pub const WINDOW: &str = "context window";
/// The table's columns, in columns of the font from its left: the name, its dollars, its share,
/// then the four token types
const COLUMNS: [f64; 7] = [0.0, 19.0, 28.0, 34.0, 49.0, 64.0, 79.0];
const TYPES: [&str; 4] = ["input", "cache writes", "cache reads", "output"];
/// The table's width in columns
const TABLE: f64 = 93.0;
/// The graph beside the table needs this many columns, else it goes under it
const GRAPH_MIN: f64 = 36.0;

fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * p).find(|s| *s >= raw - 1e-12).unwrap_or(10.0 * p)
}

/// The time between the graph's ticks, the first of these giving no more than `most` of them
fn time_step(span: f64, most: f64) -> f64 {
    const STEPS: [f64; 13] = [60.0, 120.0, 300.0, 600.0, 900.0, 1800.0, 3600.0, 7200.0, 10800.0, 21600.0, 43200.0, 86400.0, 172800.0];
    STEPS.iter().copied().find(|s| span / s <= most.max(1.0)).unwrap_or(604800.0)
}

fn clock(t: f64, offset: i64) -> String {
    let local = (t as i64 + offset).rem_euclid(86400);
    format!("{:02}:{:02}", local / 3600, (local % 3600) / 60)
}

fn axis_dollars(v: f64, step: f64) -> String {
    if step >= 1.0 {
        format!("${}", v.round() as i64)
    } else {
        format!("${v:.2}")
    }
}

/// The pane with only `text` in it: while the count is made, or why there's none
pub fn note(text: &str, left: f64, top: f64, width: f64, cell: (f64, f64)) -> Pane {
    let (col, line) = cell;
    let (pad_x, pad_y) = ((1.5 * col).round(), (0.75 * line).round());
    Pane {
        rect: Rect { x: left, y: top, w: width, h: line + 2.0 * pad_y },
        words: vec![Word { x: left + pad_x, y: top + pad_y, w: width - 2.0 * pad_x, text: text.to_string(), tone: Tone::Dim }],
        note: Some(text.to_string()),
        ..Pane::default()
    }
}

/// The pane while LBTerm doesn't count the cost itself (`lbterm_context.count_cost` off) and the
/// Claude project's split isn't there to show: only what the pill has from `<session>.usage.json`,
/// which Terminal's statusline writes from the Claude project's count. `usd` what the
/// conversation has cost (none until it's written), with what it is after it, as the counted
/// pane's first line; `window` its context window's size in tokens (none until Claude Code has
/// said it); then `why` there's no split ([`NOT_PUBLISHED`], a version it doesn't read, what
/// went wrong), which is its `note`. Nothing in it is counted here, and nothing counts it a
/// second way
#[allow(clippy::too_many_arguments)]
pub fn usage_pane(usd: Option<f64>, window: Option<u64>, why: &str, left: f64, top: f64, width: f64, cell: (f64, f64), measure: &dyn Fn(&str) -> f64) -> Pane {
    let (col, line) = cell;
    let pad_x = (1.5 * col).round();
    let pad_y = (0.75 * line).round();
    let x0 = left + pad_x;
    // The card's own edge: the room each word may draw into (PITFALLS 76)
    let edge = left + width;
    let mut p = Pane::default();
    let mut y = top + pad_y;
    if let Some(usd) = usd {
        let total_text = dollars(usd);
        let after = (x0 + measure(&total_text) + 2.0 * col).round();
        p.words.push(Word { x: x0, y, w: after - x0, text: total_text, tone: Tone::Total });
        p.words.push(Word { x: after, y, w: (edge - after).max(0.0), text: HEADING.to_string(), tone: Tone::Dim });
        y += (1.5 * line).round();
    }
    if let Some(window) = window {
        p.words.push(Word { x: x0, y, w: edge - x0, text: format!("{} {WINDOW}", tokens(window)), tone: Tone::Text });
        y += line;
    }
    p.words.push(Word { x: x0, y, w: edge - x0, text: why.to_string(), tone: Tone::Dim });
    y += line;
    p.note = Some(why.to_string());
    p.rect = Rect { x: left, y: top, w: width, h: (y - top + pad_y).round() };
    p
}

/// Lay the pane out: `cost` (`None` while it's being counted) in a pane at `left`, `top`, `width`
/// wide, `cell` the font's column and line, `scale` the DPI over 96, `offset` the local time's
/// seconds east of UTC for the graph's clock, `measure` how wide a word lays out in the pane's
/// font, which places the words whose places turn on their widths (the heading after the total,
/// the plot after its dollars, each time centred on its tick, a mark's name where it fits). A
/// word's `w` is its room, what it may draw into, never only its measured width: the box model
/// draws a glyph only while its ink ends short of the room, and a glyph's ink can pass its
/// advance, so a word given exactly its advances loses its last glyph (PITFALLS 76)
#[allow(clippy::too_many_arguments)]
pub fn pane(cost: Option<&Cost>, left: f64, top: f64, width: f64, cell: (f64, f64), scale: f64, offset: i64, measure: &dyn Fn(&str) -> f64) -> Pane {
    let (col, line) = cell;
    let pad_x = (1.5 * col).round();
    let pad_y = (0.75 * line).round();
    let x0 = left + pad_x;
    let right = left + width - pad_x;
    // The card's own edge: the room a word near the right may draw into, its pad past `right`
    let edge = left + width;
    let mut p = Pane::default();
    let Some(cost) = cost else {
        return note(COUNTING, left, top, width, cell);
    };
    let total = cost.total();
    let table_w = (TABLE * col).min(right - x0);
    let mut y = top + pad_y;
    // The total and what it is
    let total_text = dollars(total);
    let after = (x0 + measure(&total_text) + 2.0 * col).round();
    p.words.push(Word { x: x0, y, w: after - x0, text: total_text, tone: Tone::Total });
    p.words.push(Word { x: after, y, w: (x0 + table_w - after).max(0.0), text: HEADING.to_string(), tone: Tone::Dim });
    y += (1.5 * line).round();
    // The bar of shares, each spender's in its colour, a pixel's gap between them
    let bar_h = (0.6 * line).round().max(2.0);
    let gap = scale.round().max(1.0);
    let spent: Vec<(Spender, f64)> = Spender::ALL.iter().map(|s| (*s, cost.part(*s).usd.total())).filter(|(_, d)| *d > 0.0).collect();
    if total > 0.0 {
        let mut x = x0;
        for (i, (s, d)) in spent.iter().enumerate() {
            let w = if i + 1 == spent.len() { x0 + table_w - x } else { (table_w * d / total).round() };
            let w = if i + 1 < spent.len() { (w - gap).max(1.0) } else { w };
            p.fills.push(Fill { rect: Rect { x, y, w, h: bar_h }, tone: Tone::Part(*s), round: false });
            x += w + gap;
        }
    } else {
        p.fills.push(Fill { rect: Rect { x: x0, y, w: table_w, h: bar_h }, tone: Tone::Grid, round: false });
    }
    y += bar_h + (0.75 * line).round();
    // The table: its heading, a row a spender, the compactions'
    let at = |c: usize| x0 + COLUMNS[c] * col;
    let room = |c: usize| (if c + 1 < COLUMNS.len() { COLUMNS[c + 1] - COLUMNS[c] - 1.0 } else { TABLE - COLUMNS[c] }) * col;
    for (i, t) in TYPES.iter().enumerate() {
        p.words.push(Word { x: at(3 + i), y, w: room(3 + i), text: t.to_string(), tone: Tone::Dim });
    }
    y += line;
    let square = (0.6 * line).round();
    let name_x = x0 + square + col;
    for s in Spender::ALL {
        let part = cost.part(s);
        let row = Rect { x: x0, y, w: table_w, h: line };
        p.fills.push(Fill { rect: Rect { x: x0, y: y + ((line - square) / 2.0).round(), w: square, h: square }, tone: Tone::Part(s), round: false });
        let name = match s {
            Spender::Turns => "Your turns".to_string(),
            Spender::Subagents => format!("Subagents ({})", part.runs),
            Spender::Workflows => format!("Workflows ({})", part.runs),
        };
        p.words.push(Word { x: name_x, y, w: at(1) - name_x - col, text: name, tone: Tone::Text });
        let d = part.usd.total();
        p.words.push(Word { x: at(1), y, w: room(1), text: dollars(d), tone: Tone::Text });
        let share = if total > 0.0 { format!("{}%", (100.0 * d / total).round()) } else { "–".to_string() };
        p.words.push(Word { x: at(2), y, w: room(2), text: share, tone: Tone::Dim });
        let types = [
            (part.tokens.input, part.usd.input),
            (part.tokens.writes, part.usd.writes),
            (part.tokens.reads, part.usd.reads),
            (part.tokens.output, part.usd.output),
        ];
        for (i, (n, d)) in types.iter().enumerate() {
            p.words.push(Word { x: at(3 + i), y, w: room(3 + i), text: format!("{} {}", tokens(*n), dollars(*d)), tone: Tone::Dim });
        }
        p.rows.push(row);
        y += line;
    }
    p.fills.push(Fill { rect: Rect { x: x0, y: y + ((line - square) / 2.0).round(), w: square, h: square }, tone: Tone::Compactions, round: false });
    p.words.push(Word { x: name_x, y, w: at(1) - name_x - col, text: format!("Compactions ({})", cost.compactions.len()), tone: Tone::Text });
    let note = if cost.compactions.is_empty() { "–" } else { NOT_RECORDED };
    p.words.push(Word { x: at(1), y, w: x0 + table_w - at(1), text: note.to_string(), tone: Tone::Dim });
    p.rows.push(Rect { x: x0, y, w: table_w, h: line });
    y += line;
    let table_bottom = y;
    // The graph: beside the table when there's room, else under it
    let beside = right - (x0 + table_w) >= GRAPH_MIN * col + 3.0 * col;
    let (gx, gy, gr, gb) = if beside {
        (x0 + table_w + 3.0 * col, top + pad_y, right, table_bottom)
    } else {
        (x0, table_bottom + (0.75 * line).round(), right, table_bottom + (0.75 * line).round() + 8.0 * line)
    };
    let mut bottom = table_bottom;
    if !cost.steps.is_empty() && gr - gx >= 16.0 * col {
        p.words.push(Word { x: gx, y: gy, w: gr - gx, text: OVER_TIME.to_string(), tone: Tone::Dim });
        let step = nice_step(total.max(0.01) / 4.0);
        let top_v = (total / step).ceil().max(1.0) * step;
        let labels: Vec<String> = (0..=((top_v / step).round() as usize)).map(|i| axis_dollars(i as f64 * step, step)).collect();
        let label_w = labels.iter().map(|l| measure(l)).fold(2.0 * col, f64::max).ceil();
        let plot = Rect {
            x: (gx + label_w + col).round(),
            y: (gy + 1.5 * line).round(),
            w: (gr - gx - label_w - col).round(),
            h: (gb - gy - 1.5 * line - 1.25 * line).round().max(2.0),
        };
        let y_of = |v: f64| (plot.y + plot.h - (v / top_v) * plot.h).round();
        // The grid, its dollars beside it
        for (i, label) in labels.iter().enumerate() {
            let v = i as f64 * step;
            let gyv = y_of(v).min(plot.y + plot.h - 1.0);
            p.fills.push(Fill { rect: Rect { x: plot.x, y: gyv, w: plot.w, h: 1.0 }, tone: Tone::Grid, round: false });
            p.words.push(Word { x: gx, y: (gyv - line / 2.0).round(), w: plot.x - gx, text: label.clone(), tone: Tone::Dim });
        }
        // Time along the bottom
        let t0 = cost.steps[0].0;
        // To the last step, or a compaction after it (one no request has followed yet)
        let last = cost.compactions.iter().filter_map(|k| k.at).fold(cost.steps[cost.steps.len() - 1].0, f64::max);
        let t1 = last.max(t0 + 60.0);
        let x_of = |t: f64| (plot.x + (t - t0) / (t1 - t0) * (plot.w - 1.0)).round();
        let tick = time_step(t1 - t0, (plot.w / (7.0 * col)).floor());
        let local0 = t0 + offset as f64;
        let mut t = (local0 / tick).ceil() * tick - offset as f64;
        // Where the last time's words end: a time kept inside the card at the plot's right end
        // can come back over the one before it, and then only its tick is drawn
        let mut said_to = f64::NEG_INFINITY;
        while t <= t1 {
            let x = x_of(t);
            p.fills.push(Fill { rect: Rect { x, y: plot.y + plot.h, w: 1.0, h: (0.25 * line).round() }, tone: Tone::Grid, round: false });
            let text = clock(t, offset);
            let w = measure(&text).ceil();
            let at = (x - w / 2.0).round().min(right - w).max(plot.x - label_w);
            if at >= said_to + col {
                p.words.push(Word { x: at, y: plot.y + plot.h + (0.25 * line).round(), w: edge - at, text, tone: Tone::Dim });
                said_to = at + w;
            }
            t += tick;
        }
        // The line: a run along each flat stretch, a column up each rise
        let thick = scale.round().max(1.0);
        // Each compaction, a grey rule down the plot at its time, under the line, named in the
        // heading's row with its sizes (`545k → 55k`): right of the rule, or left of it where the
        // right has no room, never over the heading, another's name or another's rule (one that
        // can't be placed is left off, its rule kept)
        let gap = (0.5 * col).round();
        let rules: Vec<(f64, &Compaction)> = cost.compactions.iter().filter_map(|k| k.at.filter(|t| *t >= t0).map(|t| (x_of(t), k))).collect();
        for (x, _) in &rules {
            p.fills.push(Fill { rect: Rect { x: *x, y: gy, w: thick, h: plot.y + plot.h - gy }, tone: Tone::Compactions, round: false });
        }
        let mut named: Vec<Rect> = vec![Rect { x: gx, y: gy, w: measure(OVER_TIME).ceil(), h: line }];
        for (i, (x, k)) in rules.iter().enumerate() {
            let Some(text) = k.label() else { continue };
            let w = measure(&text).ceil();
            let others: Vec<Rect> = rules.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, (rx, _))| Rect { x: *rx, y: gy, w: thick, h: line }).collect();
            let sides = [Rect { x: x + thick + gap, y: gy, w, h: line }, Rect { x: x - gap - w, y: gy, w, h: line }];
            if let Some(at) = sides.into_iter().find(|at| at.x >= gx && at.x + at.w <= gr && free(at, &named, col) && free(at, &others, gap)) {
                p.words.push(Word { x: at.x, y: gy, w: edge - at.x, text, tone: Tone::Compactions });
                named.push(at);
            }
        }
        let mut run: Option<(f64, f64)> = None; // (x where it started, y)
        let mut prev = y_of(0.0) - thick;
        for i in 0..(plot.w as usize) {
            let x = plot.x + i as f64;
            let tx = t0 + (i as f64 / (plot.w - 1.0).max(1.0)) * (t1 - t0);
            let yv = (y_of(cost.at(tx)) - thick).max(plot.y);
            if (yv - prev).abs() >= 1.0 {
                if let Some((sx, sy)) = run.take() {
                    p.fills.push(Fill { rect: Rect { x: sx, y: sy, w: x - sx, h: thick }, tone: Tone::Line, round: false });
                }
                let (a, b) = (yv.min(prev), yv.max(prev));
                p.fills.push(Fill { rect: Rect { x, y: a, w: thick, h: b - a + thick }, tone: Tone::Line, round: false });
                prev = yv;
            }
            if run.is_none() {
                run = Some((x, yv));
            }
        }
        if let Some((sx, sy)) = run {
            p.fills.push(Fill { rect: Rect { x: sx, y: sy, w: plot.x + plot.w - sx, h: thick }, tone: Tone::Line, round: false });
        }
        let dot = (0.5 * line).round().max(4.0);
        let mut placed: Vec<Rect> = vec![];
        // Each subagent's and workflow's start, a dot on the line, named where its name fits
        for m in &cost.marks {
            let (x, yv) = (x_of(m.at), y_of(cost.at(m.at)));
            p.fills.push(Fill { rect: Rect { x: x - dot / 2.0, y: yv - dot / 2.0, w: dot, h: dot }, tone: Tone::Part(m.spender), round: true });
            let text = match m.spender {
                Spender::Workflows => "workflow",
                _ => "subagent",
            };
            let w = measure(text).ceil();
            let at = Rect { x: x + dot, y: yv - line / 2.0 + line * 0.6, w, h: line };
            if at.x + at.w <= plot.x + plot.w && free(&at, &placed, col) {
                p.words.push(Word { x: at.x, y: at.y.round(), w: edge - at.x, text: text.to_string(), tone: Tone::Part(m.spender) });
                placed.push(at);
            }
        }
        p.graph = Some(plot);
        bottom = bottom.max(gb);
    }
    p.rect = Rect { x: left, y: top, w: width, h: (bottom - top + pad_y).round() };
    p
}

#[cfg(test)]
mod tests;
