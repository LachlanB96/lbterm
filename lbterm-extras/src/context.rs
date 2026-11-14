//! What a Claude Code conversation has in its context from files, for the pill under the InMail
//! button (lbterm/CONTEXT.md). Lachlan's ask, 2026-11-07: "LBTerm should be able to show me what
//! context files this agent has loaded currently."
//!
//! The files are the instruction files Claude Code loads: every CLAUDE.md from the conversation's
//! folder up, the user's own, their `@` imports, rules, and the auto memory's MEMORY.md. Its
//! transcript says which (read by [`crate::conversation`], on the scroll strip's thread):
//!
//! - an `instructions` attachment when a session starts, and again after each compaction, listing
//!   every file it put in the context, each with its type (`User`, `Project`, `Local`, `Managed`,
//!   `AutoMem`) and its whole content;
//! - a `nested_memory` attachment for each CLAUDE.md a file read in another folder pulled in
//!   later, the file's content under `content.content`;
//! - a `compact_boundary` system record for a compaction, which drops everything before it.
//!
//! So what's loaded now is every file recorded since the last compaction, by its path, the last
//! record of a path giving its size. A resume writes another `instructions` record, and what was
//! pulled in before it is still in the conversation's history, so it stays.
//!
//! A file's tokens are its characters (UTF-16 units, as Claude Code counts a string) over four,
//! rounded: Claude Code's own estimate of a memory file's size.
//!
//! A file pulled in from a folder that isn't the conversation's own or one above it is
//! [`File::elsewhere`] (Lachlan's E4, 2026-11-07): another project's CLAUDE.md, loaded whole
//! because a file of that project was read. The conversation's own folder is the first `cwd` its
//! transcript records, where it started, which a `cd` or a resume doesn't change.
//!
//! For the context bar (lbterm/CONTEXT.md, "The context bar") it keeps more of what the transcript
//! says: the memory files Claude read ([`Kind::Remembered`]), which file imports which
//! ([`File::imported_by`]), how much of the window is in use ([`InUse`], from the last main-chain
//! assistant record's usage), the conversation's [`Scale`] (its first request's real size over its
//! characters over four) and Claude Code's own [`Parts`] (its system prompt, tools and skills list).
//! The requests themselves are read by a [`Requests`], which the conversation keeps apart.

use serde_json::Value;

/// How Claude Code came to load a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The user's own: `~/.claude/CLAUDE.md`, and what it imports
    User,
    /// A project's: a CLAUDE.md from the conversation's folder up, and what it imports
    Project,
    /// A `CLAUDE.local.md`
    Local,
    /// One an administrator put in place for every user
    Managed,
    /// The auto memory's index, `MEMORY.md`
    Memory,
    /// A CLAUDE.md a file read in its folder pulled in after the start
    Later,
    /// A memory file Claude read: a `Read` of a file in the memory index's folder
    Remembered,
    /// A file Claude Code attached again after a compaction's summary, read again from disk (a
    /// `file` attachment between the `compact_boundary` and the next turn's `instructions`):
    /// beside its own entry when it's loaded again too (lbterm/CONTEXT.md, "The buttons")
    Attached,
}

impl Kind {
    /// The type an `instructions` record gives a file; none for one it doesn't know
    fn from_type(t: &str) -> Option<Kind> {
        Some(match t {
            "User" => Kind::User,
            "Project" => Kind::Project,
            "Local" => Kind::Local,
            "Managed" => Kind::Managed,
            "AutoMem" => Kind::Memory,
            _ => return None,
        })
    }

    /// The list's word for it
    pub fn word(self) -> &'static str {
        match self {
            Kind::User => "user",
            Kind::Project => "project",
            Kind::Local => "local",
            Kind::Managed => "managed",
            Kind::Memory => "memory",
            Kind::Later => "pulled in",
            Kind::Remembered => "memory",
            Kind::Attached => "attached",
        }
    }
}

/// One file in the context
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// As Claude Code wrote it
    pub path: String,
    pub kind: Kind,
    /// Claude Code's estimate of its size: its characters over four
    pub tokens: usize,
    /// Pulled in from a folder that isn't the conversation's own or one above it
    pub elsewhere: bool,
    /// The loaded file that names this one on an `@` line, as the record that loaded both says
    pub imported_by: Option<String>,
}

/// How much of the context window is in use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InUse {
    /// No reply in yet
    #[default]
    Unknown,
    /// The size of the request the last main-chain reply answered: its input, cache read and cache
    /// creation tokens together, as Claude Code's /context counts it
    Known(u64),
    /// Compacted since: the next request is a new size, and until its reply is in only the
    /// compaction's own size (its `postTokens`) is known
    Compacted { post: u64 },
}

/// The conversation's measure of how many tokens its characters make: its first main request's
/// real size against that request's characters over four
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    pub real: u64,
    pub estimated: u64,
}

impl Scale {
    /// Tokens for `estimated` characters-over-four of the same kind of text
    pub fn apply(self, estimated: usize) -> u64 {
        if self.estimated == 0 {
            return estimated as u64;
        }
        let (e, real, of) = (estimated as u128, self.real as u128, self.estimated as u128);
        ((e * real + of / 2) / of) as u64
    }
}

/// Claude Code's own parts of the context, each its characters over four: the latest main
/// request's system prompt and tools (as JSON), and the latest skills list
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Parts {
    pub system: usize,
    pub tools: usize,
    pub skills: usize,
}

/// The files in a conversation's context now, in the order they were loaded, and what the bar
/// needs besides
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    pub files: Vec<File>,
    /// The folder the conversation started in: the first `cwd` its transcript records
    pub own: Option<String>,
    pub in_use: InUse,
    /// Set once, by the first main request and its reply
    pub scale: Option<Scale>,
    /// Since the first main request
    pub parts: Option<Parts>,
    /// How many compactions the transcript has recorded: Compact now holds its "sent" until this
    /// moves past what it was when `/compact` was typed (wezterm-gui's context_edits, `sent`)
    pub compactions: u32,
    /// Since a compaction and until the next turn's `instructions`: a `file` attachment now is
    /// one Claude Code attached again after the summary ([`Kind::Attached`])
    pub after_summary: bool,
    /// How many requests the transcript has recorded a reply to: one more each time a reply's
    /// usage gives a new size (`answer`), so a turn of many records counts once and costs no wake of
    /// its own. Compact now's "sent" ends when this moves past what it was when `/compact` was
    /// typed with no compaction recorded: the compaction was cancelled or failed, and a turn came
    /// after it (wezterm-gui's context_edits, `compact_waits`)
    pub replies: u32,
}

/// Claude Code's estimate of a text's tokens: its UTF-16 units over four, rounded
pub fn tokens(text: &str) -> usize {
    (text.encode_utf16().count() + 2) / 4
}

/// A path's parts, for comparing folders as Windows does: either slash, any case, no empty parts
pub(crate) fn parts(path: &str) -> Vec<String> {
    path.split(['\\', '/']).filter(|p| !p.is_empty()).map(str::to_lowercase).collect()
}

/// The folder a loaded file stands for: the one it's in, or the one above its `.claude` folder
/// for a file in one (`<folder>\.claude\CLAUDE.md`, a rule in `<folder>\.claude\rules\`)
fn folder_of(path: &str) -> Vec<String> {
    let mut p = parts(path);
    p.pop();
    if let Some(i) = p.iter().position(|part| part == ".claude") {
        p.truncate(i);
    }
    p
}

/// Whether a file loaded from `path` comes from a folder that isn't `own` or one above it
pub fn elsewhere(own: &str, path: &str) -> bool {
    let (own, folder) = (parts(own), folder_of(path));
    !(folder.len() <= own.len() && own[..folder.len()] == folder[..])
}

/// A path's parts with `.` and `..` worked out
pub(crate) fn normal(parts: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for p in parts {
        match p.as_str() {
            "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(p),
        }
    }
    out
}

/// The paths a loaded file's `@` imports name, as parts: each `@` at a line's start or after a
/// space, up to the next space, outside code fences and backticks; relative to the importing
/// file's folder, or to `home` for `~/`
fn imports(path: &str, content: &str, home: Option<&[String]>) -> Vec<Vec<String>> {
    import_lines(path, content, home).into_iter().map(|(_, _, name)| name).collect()
}

/// The `@` imports of a loaded file's `content` as [`imports`] finds them, each with the number
/// of the line it's on (from 0, as `str::lines` counts them) and the byte in that line where its
/// `@` is: the context bar's Remove takes that `@` off, leaving the path as plain text, which
/// Claude Code doesn't import (crate::context_edits)
pub(crate) fn import_lines(path: &str, content: &str, home: Option<&[String]>) -> Vec<(usize, usize, Vec<String>)> {
    let mut folder = parts(path);
    folder.pop();
    let mut out = vec![];
    let mut fenced = false;
    for (number, line) in content.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut ticks = 0;
        let mut prev = ' ';
        for (i, c) in line.char_indices() {
            if c == '`' {
                ticks += 1;
            } else if c == '@' && ticks % 2 == 0 && prev.is_whitespace() {
                let name = line[i + 1..].split(char::is_whitespace).next().unwrap_or("");
                let named = parts(name);
                if named.is_empty() {
                    prev = c;
                    continue;
                }
                let whole: Vec<String> = if name.starts_with('~') {
                    match home {
                        Some(home) => home.iter().cloned().chain(named.into_iter().skip(1)).collect(),
                        None => vec![],
                    }
                } else if name.as_bytes().get(1) == Some(&b':') {
                    named
                } else {
                    folder.iter().cloned().chain(named).collect()
                };
                if !whole.is_empty() {
                    out.push((number, i, normal(whole)));
                }
            }
            prev = c;
        }
    }
    out
}

/// What the transcript's records of Claude Code's requests say, for the context's [`Scale`] and
/// [`Parts`]: kept by the conversation, apart from the [`Context`] its snapshots share.
///
/// Claude Code records each request it makes in the transcript: an `api-request-shape` record
/// with its system prompt and tools (one each time they change, by `shapeHash`), an
/// `api-request-blob` record for each message the first time it's sent (by `hash`), and an
/// `api-request` record naming its shape and the new messages (`tail`) after those it keeps from
/// the request before (`keep`). So the first main request (`querySource` `repl_main_thread`, or
/// `sdk` for `claude -p`) is its shape and its tail's blobs, and its reply's usage is its real size.
#[derive(Debug, Clone, Default)]
pub struct Requests {
    /// Each shape's system prompt and tools, characters over four, by its hash
    shapes: std::collections::HashMap<String, (usize, usize)>,
    /// Each message's characters over four, by its hash, until the first main request is in
    blobs: std::collections::HashMap<String, usize>,
    /// The first main request's characters over four, until its reply gives the scale
    first: Option<u64>,
    /// The scale is set: blobs aren't wanted any more
    pub done: bool,
}

/// Characters over four of everything a message holds that the model reads: its words, its
/// thinking, its tool calls' input and its tool results
fn message_estimate(message: &Value) -> usize {
    fn walk(v: &Value, n: &mut usize) {
        match v {
            Value::String(s) => *n += s.encode_utf16().count(),
            Value::Array(a) => a.iter().for_each(|x| walk(x, n)),
            Value::Object(o) => {
                for (k, x) in o {
                    match k.as_str() {
                        "text" | "thinking" | "content" => walk(x, n),
                        "input" => *n += x.to_string().encode_utf16().count(),
                        _ if x.is_array() || x.is_object() => walk(x, n),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let mut n = 0;
    walk(message.get("content").unwrap_or(&Value::Null), &mut n);
    (n + 2) / 4
}

impl Requests {
    /// An `api-request-shape` record
    pub fn shape(&mut self, record: &Value) {
        let (Some(hash), Some(shape)) = (record.get("shapeHash").and_then(Value::as_str), record.get("shape")) else {
            return;
        };
        let system: String = shape
            .get("system")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.get("text").and_then(Value::as_str))
            .collect();
        let tools = shape.get("tools").map_or(0, |t| tokens(&t.to_string()));
        self.shapes.insert(hash.to_string(), (tokens(&system), tools));
    }

    /// An `api-request-blob` record, wanted only until the first main request is in
    pub fn blob(&mut self, record: &Value) {
        if self.done || self.first.is_some() {
            return;
        }
        if let (Some(hash), Some(message)) = (record.get("hash").and_then(Value::as_str), record.get("message")) {
            self.blobs.insert(hash.to_string(), message_estimate(message));
        }
    }

    /// An `api-request` record: the system prompt and tools of a main request, for Claude Code's
    /// parts; none for another kind of request, or one whose shape wasn't recorded
    pub fn request(&mut self, record: &Value) -> Option<(usize, usize)> {
        let source = record.get("querySource").and_then(Value::as_str);
        if !matches!(source, Some("repl_main_thread" | "sdk")) {
            return None;
        }
        let shape = *self.shapes.get(record.get("shapeHash").and_then(Value::as_str)?)?;
        if !self.done && self.first.is_none() {
            let keep = record.get("keep").and_then(Value::as_u64).unwrap_or(0);
            let tail: Option<usize> = record
                .get("tail")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|h| h.as_str().and_then(|h| self.blobs.get(h)).copied())
                .sum();
            // The first request this transcript has: anything it kept came from one it doesn't, and
            // a blob it names that isn't here leaves it unknown. Either way no blob is wanted after
            match (keep, tail) {
                (0, Some(tail)) => self.first = Some((shape.0 + shape.1 + tail) as u64),
                _ => self.done = true,
            }
            self.blobs.clear();
        }
        Some(shape)
    }

    /// The first main request's characters over four, for its reply's usage to make the scale of
    pub fn first(&self) -> Option<u64> {
        self.first
    }

    /// The scale is set: nothing more is wanted for it
    pub fn finish(&mut self) {
        self.first = None;
        self.blobs.clear();
        self.done = true;
    }
}

impl Context {
    /// A record's `cwd`: the first one is the conversation's own folder
    pub fn saw_folder(&mut self, cwd: &str) -> bool {
        if self.own.is_some() || cwd.is_empty() {
            return false;
        }
        self.own = Some(cwd.to_string());
        true
    }

    /// A compaction: what was loaded before it has gone, and what's in use is only the
    /// compaction's own size (`post`, its `postTokens`) until the next reply. Whether anything
    /// changed
    pub fn compacted(&mut self, post: u64) -> bool {
        self.files.clear();
        self.in_use = InUse::Compacted { post };
        self.compactions += 1;
        self.after_summary = true;
        true
    }

    /// A main-chain assistant record's `usage`: what's in use now, and the scale when it answers
    /// the first main request (`first`, that request's characters over four). Whether anything
    /// changed. A usage with nothing in it (an API error's) says nothing
    pub fn answered(&mut self, usage: &Value, first: Option<u64>) -> bool {
        match self.answer(usage, first) {
            Some((in_use, scale)) => {
                (self.in_use, self.scale) = (in_use, scale);
                true
            }
            None => false,
        }
    }

    /// What [`Context::answered`] would make what's in use and the scale, none when that's what
    /// they are: the reader asks this before it copies the shared context, so a reply that
    /// changes nothing (most of a turn's records repeat the last count) copies nothing
    pub fn answer(&self, usage: &Value, first: Option<u64>) -> Option<(InUse, Option<Scale>)> {
        let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        let total = n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
        if total == 0 {
            return None;
        }
        let scale = match self.scale {
            None => first.filter(|e| *e > 0).map(|estimated| Scale { real: total, estimated }),
            kept => kept,
        };
        let new = (InUse::Known(total), scale);
        (new != (self.in_use, self.scale)).then_some(new)
    }

    /// Whether [`Context::requested`] would change anything
    pub fn requests_differ(&self, (system, tools): (usize, usize)) -> bool {
        self.parts.map(|p| (p.system, p.tools)) != Some((system, tools))
    }

    /// Claude Code's system prompt and tools as the latest main request has them. Whether they
    /// changed
    pub fn requested(&mut self, (system, tools): (usize, usize)) -> bool {
        let was = self.parts;
        let parts = self.parts.get_or_insert_with(Parts::default);
        parts.system = system;
        parts.tools = tools;
        was != self.parts
    }

    /// A `skill_listing` attachment: the whole list when it's the first (`isInitial`), else added
    /// to it. Whether anything changed
    pub fn skills(&mut self, record: &Value) -> bool {
        let Some(attachment) = record.get("attachment") else {
            return false;
        };
        // What was sent: the list in its reminder, as the record's `rendered` has it
        let text: String = record
            .get("rendered")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| r.get("content").and_then(Value::as_str))
            .collect();
        let initial = attachment.get("isInitial").and_then(Value::as_bool) != Some(false);
        let was = self.parts;
        let parts = self.parts.get_or_insert_with(Parts::default);
        parts.skills = if initial { tokens(&text) } else { parts.skills + tokens(&text) };
        was != self.parts
    }

    /// The memory index's folder: where the memory files are
    fn memory_folder(&self) -> Option<Vec<String>> {
        let index = self.files.iter().find(|f| f.kind == Kind::Memory)?;
        let mut p = parts(&index.path);
        p.pop();
        Some(p)
    }

    /// Whether `path` is a memory file: in the memory index's folder, and not the index itself
    pub fn is_memory_file(&self, path: &str) -> bool {
        let Some(folder) = self.memory_folder() else {
            return false;
        };
        let p = parts(path);
        p.len() == folder.len() + 1
            && p[..folder.len()] == folder[..]
            && !self.files.iter().any(|f| f.kind == Kind::Memory && parts(&f.path) == p)
    }

    /// A memory file Claude read, `text` the tool's result. Whether anything changed
    pub fn remembered(&mut self, path: &str, text: &str) -> bool {
        self.add(File {
            path: path.to_string(),
            kind: Kind::Remembered,
            tokens: tokens(text),
            elsewhere: false,
            imported_by: None,
        })
    }

    /// A `file` attachment (`record` the whole record): after a compaction's summary, a file
    /// Claude Code attached again, its tokens what was sent (the record's `rendered`, the Read
    /// tool's call and the file with its line numbers); otherwise (a file named with `@` in a
    /// prompt) the conversation's, as before. Whether anything changed
    pub fn attached(&mut self, record: &Value) -> bool {
        let Some(path) = record.pointer("/attachment/filename").and_then(Value::as_str) else {
            return false;
        };
        if !self.after_summary {
            return false;
        }
        let text: String = record
            .get("rendered")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| r.get("content").and_then(Value::as_str))
            .collect();
        self.add(File { path: path.to_string(), kind: Kind::Attached, tokens: tokens(&text), elsewhere: false, imported_by: None })
    }

    /// File `file` loaded, in its place if it's there already (an attached copy and the file
    /// loaded again are two entries). Whether that changed anything
    fn add(&mut self, file: File) -> bool {
        let key = parts(&file.path);
        let copy = file.kind == Kind::Attached;
        match self.files.iter_mut().find(|f| parts(&f.path) == key && (f.kind == Kind::Attached) == copy) {
            Some(was) if *was == file => false,
            Some(was) => {
                *was = file;
                true
            }
            None => {
                self.files.push(file);
                true
            }
        }
    }

    /// An `instructions` attachment: every file the session put in its context as it started, or
    /// after a compaction. Whether anything changed
    pub fn instructions(&mut self, attachment: &Value) -> bool {
        let Some(files) = attachment.get("files").and_then(Value::as_array) else {
            return false;
        };
        // The turn after a compaction: the files attached after its summary stay, and later
        // `file` attachments are a prompt's
        let mut changed = std::mem::take(&mut self.after_summary);
        // Which file imports which, from the contents the record holds: the user's CLAUDE.md,
        // `<home>/.claude/CLAUDE.md`, says where home is for an import from `~/`
        let named = |f: &Value, k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let home: Option<Vec<String>> = files.iter().find_map(|f| {
            let p = parts(&named(f, "path"));
            let user = named(f, "type") == "User" && p.len() > 2;
            (user && p[p.len() - 2] == ".claude" && p[p.len() - 1] == "claude.md").then(|| p[..p.len() - 2].to_vec())
        });
        let named_by: Vec<(String, Vec<String>, Vec<Vec<String>>)> = files
            .iter()
            .map(|f| {
                let path = named(f, "path");
                let names = imports(&path, &named(f, "content"), home.as_deref());
                (path.clone(), parts(&path), names)
            })
            .collect();
        // A CLAUDE.md or CLAUDE.local.md loads for where it sits (the user's, a project's, a
        // local one), whatever names it: it's never an import, so its Remove can't edit the other
        // file and leave it loading (lbterm/CONTEXT.md, "The buttons")
        let importer = |path: &str| {
            let p = parts(path);
            if matches!(p.last().map(String::as_str), Some("claude.md" | "claude.local.md")) {
                return None;
            }
            named_by
                .iter()
                .find(|(_, by, names)| *by != p && names.iter().any(|n| *n == p))
                .map(|(by, _, _)| by.clone())
        };
        for f in files {
            let (Some(path), Some(kind)) = (
                f.get("path").and_then(Value::as_str),
                f.get("type").and_then(Value::as_str).and_then(Kind::from_type),
            ) else {
                continue;
            };
            let content = f.get("content").and_then(Value::as_str).unwrap_or("");
            changed |= self.add(File {
                path: path.to_string(),
                kind,
                tokens: tokens(content),
                elsewhere: false,
                imported_by: importer(path),
            });
        }
        changed
    }

    /// A `nested_memory` attachment: a CLAUDE.md a file read pulled in. Whether anything changed
    pub fn nested(&mut self, attachment: &Value) -> bool {
        let Some(path) = attachment.get("path").and_then(Value::as_str) else {
            return false;
        };
        let content = attachment
            .get("content")
            .and_then(|c| c.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let elsewhere = self.own.as_deref().map_or(false, |own| elsewhere(own, path));
        self.add(File {
            path: path.to_string(),
            kind: Kind::Later,
            tokens: tokens(content),
            elsewhere,
            imported_by: None,
        })
    }

    /// Every file's tokens together
    pub fn tokens(&self) -> usize {
        self.files.iter().map(|f| f.tokens).sum()
    }
}

/// Tokens in a few characters, as the statusline writes them: `912`, `6.1k`, `31k`, `1.2M`
pub fn count(n: usize) -> String {
    let n = n as f64;
    let short = |v: f64, unit: &str| {
        let s = if v < 10.0 { format!("{v:.1}") } else { format!("{v:.0}") };
        format!("{}{unit}", s.trim_end_matches(".0"))
    };
    if n < 1000.0 {
        format!("{n:.0}")
    } else if n < 999_500.0 {
        short(n / 1000.0, "k")
    } else {
        short(n / 1_000_000.0, "M")
    }
}

/// A path as the context bar's pane shows it: the home folder as `~`
pub fn short_path(path: &str, home: &str) -> String {
    let (p, h) = (parts(path), parts(home));
    if !h.is_empty() && p.len() > h.len() && p[..h.len()] == h[..] {
        let rest: Vec<&str> = path.split(['\\', '/']).filter(|s| !s.is_empty()).skip(h.len()).collect();
        format!("~\\{}", rest.join("\\"))
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    fn instructions() -> Value {
        json!({ "type": "instructions", "files": [
            { "path": "C:\\Users\\me\\.claude\\CLAUDE.md", "type": "User", "content": "x".repeat(4000) },
            { "path": "C:\\Users\\me\\Work\\rules\\asking.md", "type": "User", "content": "y".repeat(401) },
            { "path": "C:\\Users\\me\\Work\\CLAUDE.md", "type": "Project", "content": "z".repeat(8000) },
            { "path": "C:\\Users\\me\\Work\\Harbor\\CLAUDE.md", "type": "Project", "content": "h".repeat(2000) },
            { "path": "C:\\Users\\me\\.claude\\projects\\C--Users-me-Work-Harbor\\memory\\MEMORY.md", "type": "AutoMem", "content": "m".repeat(800) },
            { "path": "C:\\odd.md", "type": "Something new", "content": "?" },
        ]})
    }

    fn nested(path: &str, chars: usize) -> Value {
        json!({ "type": "nested_memory", "path": path, "displayPath": "CLAUDE.md",
                "content": { "path": path, "type": "Project", "content": "n".repeat(chars) } })
    }

    #[test]
    fn a_files_tokens_are_its_characters_over_four_as_claude_code_counts_them() {
        assert_eq!(tokens(""), 0);
        assert_eq!(tokens("abcd"), 1);
        assert_eq!(tokens(&"x".repeat(4001)), 1000);
        assert_eq!(tokens(&"x".repeat(4002)), 1001, "rounded half up, as Math.round");
        // A character outside the BMP is two UTF-16 units to JavaScript
        assert_eq!(tokens("😀😀"), 1);
    }

    #[test]
    fn the_start_loads_every_file_the_instructions_record_lists_with_its_kind_and_size() {
        let mut c = Context::default();
        assert!(c.saw_folder("C:\\Users\\me\\Work\\Harbor"));
        assert!(!c.saw_folder("C:\\Users\\me\\Elsewhere"), "the first folder is the conversation's own");
        assert!(c.instructions(&instructions()));
        let kinds: Vec<(Kind, usize)> = c.files.iter().map(|f| (f.kind, f.tokens)).collect();
        assert_eq!(
            kinds,
            vec![(Kind::User, 1000), (Kind::User, 100), (Kind::Project, 2000), (Kind::Project, 500), (Kind::Memory, 200)],
            "a type it doesn't know is left out"
        );
        assert_eq!(c.tokens(), 3800);
        assert!(c.files.iter().all(|f| !f.elsewhere));
        assert!(!c.instructions(&instructions()), "the same files again change nothing");
    }

    #[test]
    fn a_claude_md_pulled_in_from_another_project_is_elsewhere_and_one_in_its_own_folder_is_not() {
        let mut c = Context::default();
        c.saw_folder("C:\\Users\\me\\Work\\Harbor");
        c.instructions(&instructions());
        assert!(c.nested(&nested("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", 20000)));
        assert!(c.nested(&nested("C:/users/ME/work/harbor/.claude/CLAUDE.md", 40)));
        let later: Vec<(&str, bool, usize)> = c
            .files
            .iter()
            .filter(|f| f.kind == Kind::Later)
            .map(|f| (f.path.as_str(), f.elsewhere, f.tokens))
            .collect();
        assert_eq!(
            later,
            vec![("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", true, 5000), ("C:/users/ME/work/harbor/.claude/CLAUDE.md", false, 10)]
        );
        // Below its own folder is another folder's: a code base inside the project has its own
        assert!(elsewhere("C:\\Users\\me\\Work", "C:\\Users\\me\\Work\\Harbor\\code\\site\\CLAUDE.md"));
        assert!(!elsewhere("C:\\Users\\me\\Work\\Harbor\\code", "C:\\Users\\me\\Work\\CLAUDE.md"), "one above it");
        assert!(!elsewhere("C:\\Users\\me\\Work", "C:\\Users\\me\\Work\\.claude\\rules\\style.md"), "its own rules");
    }

    fn file_attachment(path: &str, chars: usize) -> Value {
        json!({ "type": "attachment", "attachment": { "type": "file", "filename": path, "content": { "type": "text" } },
                "rendered": [{ "content": "c".repeat(chars) }] })
    }

    #[test]
    fn a_file_attached_after_a_compactions_summary_is_a_section_of_its_own_till_the_next() {
        let rule = "C:\\Users\\me\\Work\\Harbor\\check-rule.md";
        let mut c = Context::default();
        c.instructions(&instructions());
        assert!(!c.attached(&file_attachment(rule, 400)), "before any compaction a file attachment is a prompt's");
        c.compacted(100);
        assert!(c.attached(&file_attachment(rule, 400)));
        assert!(c.attached(&file_attachment("C:\\Users\\me\\Work\\CLAUDE.md", 800)));
        assert_eq!(c.files.iter().map(|f| (f.kind, f.tokens)).collect::<Vec<_>>(), vec![(Kind::Attached, 100), (Kind::Attached, 200)]);
        // The next turn loads the start's files again: the attached copy of one stays beside it,
        // and a file attached after that is a prompt's, the conversation's
        assert!(c.instructions(&instructions()));
        assert!(!c.after_summary);
        assert_eq!(c.files.iter().filter(|f| f.path == "C:\\Users\\me\\Work\\CLAUDE.md").count(), 2);
        assert!(!c.attached(&file_attachment("C:\\Users\\me\\elsewhere.md", 40)));
        // The next compaction drops them all, and attaches what it attaches
        c.compacted(100);
        assert!(c.files.is_empty());
    }

    #[test]
    fn a_compaction_drops_what_was_loaded_and_the_record_after_it_loads_the_start_again() {
        let mut c = Context::default();
        c.saw_folder("C:\\Users\\me\\Work\\Harbor");
        c.instructions(&instructions());
        c.nested(&nested("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", 20000));
        assert_eq!(c.files.len(), 6);
        assert!(c.compacted(0));
        assert!(c.files.is_empty());
        assert!(c.compacted(0), "a compaction over one with nothing between still counts");
        assert_eq!(c.compactions, 2);
        c.instructions(&instructions());
        assert_eq!(c.files.len(), 5, "the pulled-in one isn't back until a file reads it in again");
        assert_eq!(c.own.as_deref(), Some("C:\\Users\\me\\Work\\Harbor"), "still the conversation's own folder");
    }

    #[test]
    fn a_resume_keeps_what_was_pulled_in_and_takes_each_files_new_size() {
        let mut c = Context::default();
        c.saw_folder("C:\\Users\\me\\Work\\Harbor");
        c.instructions(&instructions());
        c.nested(&nested("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", 20000));
        let mut again = instructions();
        again["files"][2]["content"] = json!("z".repeat(9000));
        assert!(c.instructions(&again));
        assert_eq!(c.files.len(), 6, "the pulled-in file is still in the history");
        assert_eq!(c.files[2].tokens, 2250, "in its place, at its new size");
    }

    #[test]
    fn counts_are_short() {
        assert_eq!(count(912), "912");
        assert_eq!(count(6120), "6.1k");
        assert_eq!(count(31_234), "31k");
        assert_eq!(count(1_000_000), "1M");
        assert_eq!(count(200_000), "200k");
        assert_eq!(count(1_260_000), "1.3M");
    }

    #[test]
    fn paths_show_home_as_a_tilde() {
        assert_eq!(short_path("C:\\Users\\me\\Work\\CLAUDE.md", "C:\\Users\\me"), "~\\Work\\CLAUDE.md");
        assert_eq!(short_path("C:/users/ME/Work/CLAUDE.md", "C:\\Users\\me"), "~\\Work\\CLAUDE.md");
        assert_eq!(short_path("D:\\Other\\CLAUDE.md", "C:\\Users\\me"), "D:\\Other\\CLAUDE.md");
        assert_eq!(short_path("C:\\Users\\meant\\CLAUDE.md", "C:\\Users\\me"), "C:\\Users\\meant\\CLAUDE.md", "a folder that only starts like it");
    }

    #[test]
    fn a_claude_md_loaded_for_where_it_sits_is_never_an_import() {
        // The user's CLAUDE.md names a project's and a local one on @ lines: both load because of
        // where they sit, so neither is the user file's import (its Remove would edit the user
        // file and the file would go on loading); a rule it names still is
        let user = "@C:/Users/me/Work/Harbor/CLAUDE.md\n@C:/Users/me/Work/Harbor/CLAUDE.local.md\n@C:/Users/me/Work/rules/asking.md\n";
        let record = json!({ "type": "instructions", "files": [
            { "path": "C:\\Users\\me\\.claude\\CLAUDE.md", "type": "User", "content": user },
            { "path": "C:\\Users\\me\\Work\\rules\\asking.md", "type": "User", "content": "a" },
            { "path": "C:\\Users\\me\\Work\\Harbor\\CLAUDE.md", "type": "Project", "content": "p" },
            { "path": "C:\\Users\\me\\Work\\Harbor\\CLAUDE.local.md", "type": "Local", "content": "l" },
        ]});
        let mut c = Context::default();
        c.instructions(&record);
        let by: Vec<(&str, bool)> = c.files.iter().map(|f| (f.path.rsplit('\\').next().unwrap(), f.imported_by.is_some())).collect();
        assert_eq!(by, vec![("CLAUDE.md", false), ("asking.md", true), ("CLAUDE.md", false), ("CLAUDE.local.md", false)]);
    }

    #[test]
    fn a_reply_that_changes_nothing_is_no_change() {
        let mut c = Context::default();
        let usage = json!({ "input_tokens": 10, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 90 });
        assert_eq!(c.answer(&usage, Some(500)), Some((InUse::Known(1000), Some(Scale { real: 1000, estimated: 500 }))));
        assert!(c.answered(&usage, Some(500)));
        assert_eq!(c.answer(&usage, Some(500)), None, "the same count again");
        assert!(!c.answered(&usage, None));
        assert!(c.requests_differ((3, 4)));
        c.requested((3, 4));
        assert!(!c.requests_differ((3, 4)));
        // Each compaction counts, even one over another with nothing between
        assert!(c.compacted(70) && c.compacted(70));
        assert_eq!(c.compactions, 2);
    }

    #[test]
    fn a_file_named_on_an_at_line_of_another_loaded_file_is_imported_by_it() {
        let user = "Rules:\n\n@C:/Users/me/Work/rules/asking.md\n@~/Work/rules/later.md and more\nSee `@C:/Users/me/Work/rules/quoted.md` too\n```\n@C:/Users/me/Work/rules/fenced.md\n```\nmail me@C:/Users/me/Work/rules/glued.md\n";
        let project = "@rules/../style.md\n@./notes.md";
        let record = json!({ "type": "instructions", "files": [
            { "path": "C:\\Users\\me\\.claude\\CLAUDE.md", "type": "User", "content": user },
            { "path": "C:\\Users\\me\\Work\\rules\\asking.md", "type": "User", "content": "a" },
            { "path": "C:\\Users\\me\\Work\\rules\\later.md", "type": "User", "content": "l" },
            { "path": "C:\\Users\\me\\Work\\rules\\quoted.md", "type": "User", "content": "q" },
            { "path": "C:\\Users\\me\\Work\\rules\\fenced.md", "type": "User", "content": "f" },
            { "path": "C:\\Users\\me\\Work\\rules\\glued.md", "type": "User", "content": "g" },
            { "path": "C:\\Users\\me\\Work\\Harbor\\CLAUDE.md", "type": "Project", "content": project },
            { "path": "C:\\Users\\me\\Work\\Harbor\\style.md", "type": "Project", "content": "s" },
            { "path": "C:\\Users\\me\\Work\\Harbor\\notes.md", "type": "Project", "content": "n" },
        ]});
        let mut c = Context::default();
        c.instructions(&record);
        let by: Vec<(&str, Option<&str>)> = c
            .files
            .iter()
            .map(|f| (f.path.rsplit('\\').next().unwrap(), f.imported_by.as_deref()))
            .collect();
        let user_md = Some("C:\\Users\\me\\.claude\\CLAUDE.md");
        let harbor = Some("C:\\Users\\me\\Work\\Harbor\\CLAUDE.md");
        assert_eq!(
            by,
            vec![
                ("CLAUDE.md", None),
                ("asking.md", user_md),
                ("later.md", user_md),
                ("quoted.md", None),
                ("fenced.md", None),
                ("glued.md", None),
                ("CLAUDE.md", None),
                ("style.md", harbor),
                ("notes.md", harbor),
            ],
            "absolute, from home and relative names import; quoted, fenced and mid-word ones don't"
        );
    }

    #[test]
    fn whats_in_use_is_the_replys_usage_and_its_first_reply_makes_the_scale_once() {
        let mut c = Context::default();
        assert_eq!(c.in_use, InUse::Unknown);
        let usage = |i: u64, r: u64, w: u64| json!({ "input_tokens": i, "cache_read_input_tokens": r, "cache_creation_input_tokens": w, "output_tokens": 99 });
        assert!(!c.answered(&usage(0, 0, 0), Some(40_000)), "an API error's empty usage says nothing");
        assert_eq!((c.in_use, c.scale), (InUse::Unknown, None));
        assert!(c.answered(&usage(2, 19_268, 57_295), Some(51_304)));
        assert_eq!(c.in_use, InUse::Known(76_565));
        assert_eq!(c.scale, Some(Scale { real: 76_565, estimated: 51_304 }));
        assert!(!c.answered(&usage(2, 19_268, 57_295), None), "the same reply's next record changes nothing");
        assert!(c.answered(&usage(1, 80_000, 500), Some(1)));
        assert_eq!(c.scale, Some(Scale { real: 76_565, estimated: 51_304 }), "set once");
        assert_eq!(Scale { real: 3, estimated: 2 }.apply(301), 452, "rounded");
        assert!(c.compacted(20_893));
        assert_eq!(c.in_use, InUse::Compacted { post: 20_893 });
    }

    #[test]
    fn the_first_main_request_is_its_shape_and_its_tails_blobs() {
        let shape = |hash: &str, system: usize| {
            json!({ "type": "api-request-shape", "shapeHash": hash, "shape": {
                "system": [{ "type": "text", "text": "s".repeat(system) }],
                "tools": [{ "name": "Read", "description": "d".repeat(390) }] } })
        };
        let blob = |hash: &str, chars: usize| {
            json!({ "type": "api-request-blob", "hash": hash, "message": { "role": "user",
                "content": [{ "type": "text", "text": "u".repeat(chars) }] } })
        };
        let request = |source: &str, shape: &str, keep: u64, tail: &[&str]| {
            json!({ "type": "api-request", "querySource": source, "shapeHash": shape, "keep": keep, "tail": tail })
        };
        let tools = tokens(&json!([{ "name": "Read", "description": "d".repeat(390) }]).to_string());
        let mut r = Requests::default();
        r.shape(&shape("a", 4000));
        r.blob(&blob("m1", 8000));
        r.blob(&blob("m2", 400));
        assert_eq!(r.request(&request("compact", "a", 0, &["m1"])), None, "not a main request");
        assert_eq!(r.request(&request("repl_main_thread", "a", 0, &["m1", "m2"])), Some((1000, tools)));
        assert_eq!(r.first(), Some((1000 + tools + 2000 + 100) as u64));
        r.blob(&blob("m3", 4000));
        assert_eq!(r.first(), Some((1000 + tools + 2100) as u64), "blobs after it aren't its");
        r.finish();
        assert!(r.done && r.first().is_none());
        r.shape(&shape("b", 8000));
        assert_eq!(r.request(&request("repl_main_thread", "b", 3, &["m4"])), Some((2000, tools)), "a later shape for the parts");

        // A transcript whose first request kept messages from one before it has no first request
        let mut r = Requests::default();
        r.shape(&shape("a", 4000));
        r.request(&request("sdk", "a", 2, &[]));
        assert!(r.done && r.first().is_none(), "no blob is wanted any more");
        // A request whose shape wasn't recorded says nothing
        assert_eq!(Requests::default().request(&request("sdk", "z", 0, &[])), None);
    }

    #[test]
    fn the_skills_list_is_the_first_listing_and_what_later_ones_add() {
        let listing = |chars: usize, initial: bool| {
            json!({ "type": "attachment", "attachment": { "type": "skill_listing", "isInitial": initial, "content": "x" },
                    "rendered": [{ "content": "k".repeat(chars) }] })
        };
        let mut c = Context::default();
        assert!(c.skills(&listing(4000, true)));
        assert_eq!(c.parts.unwrap().skills, 1000);
        assert!(c.skills(&listing(400, false)));
        assert_eq!(c.parts.unwrap().skills, 1100);
        assert!(c.skills(&listing(2000, true)));
        assert_eq!(c.parts.unwrap().skills, 500, "a new first listing, after a compaction, is the whole list");
        assert!(c.requested((700, 9000)));
        assert_eq!(c.parts, Some(Parts { system: 700, tools: 9000, skills: 500 }));
        assert!(!c.requested((700, 9000)));
    }

    #[test]
    fn a_memory_file_is_one_in_the_memory_indexs_folder_and_not_the_index() {
        let mut c = Context::default();
        assert!(!c.is_memory_file("C:\\Users\\me\\.claude\\projects\\C--Users-me-Work-Harbor\\memory\\x.md"), "no index yet");
        c.instructions(&instructions());
        assert!(c.is_memory_file("C:/users/me/.claude/projects/C--Users-me-Work-Harbor/memory/feedback_x.md"));
        assert!(!c.is_memory_file("C:\\Users\\me\\.claude\\projects\\C--Users-me-Work-Harbor\\memory\\MEMORY.md"));
        assert!(!c.is_memory_file("C:\\Users\\me\\.claude\\projects\\C--Users-me-Work-Harbor\\memory\\old\\y.md"));
        assert!(!c.is_memory_file("C:\\Users\\me\\Work\\Harbor\\notes.md"));
        assert!(c.remembered("C:\\Users\\me\\.claude\\projects\\C--Users-me-Work-Harbor\\memory\\feedback_x.md", &"r".repeat(800)));
        assert_eq!((c.files.last().unwrap().kind, c.files.last().unwrap().tokens), (Kind::Remembered, 200));
    }
}
