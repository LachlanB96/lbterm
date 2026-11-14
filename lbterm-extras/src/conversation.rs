//! The scroll strip's Claude conversation (lbterm/SCROLL-STRIP.md). Lachlan's ask, 2026-10-28:
//! a scrollbar for the conversation in the open tab, with a mark where each of his prompts is
//! and where Claude did something worth finding again, and a click on one taking him there.
//!
//! Claude Code's fullscreen renderer keeps the conversation to itself: the pane holds only the
//! screen, and no escape sequence says where it's scrolled to. So the strip is built from the
//! session's transcript (`~/.claude/projects/<folder>/<sid>.jsonl`), read as it grows
//! ([`Follow`]): each thing Claude Code draws is a [`Block`], with the widths of its lines, so
//! how many rows it takes at the pane's width can be worked out ([`Layout`]); each thing worth a
//! mark is an [`Event`] on a block. Permission prompts aren't in the transcript: Terminal's
//! claudehook.exe writes `<sid>.events.jsonl` beside the session's summary, which
//! [`Conversation::permissions`] reads.
//!
//! Where Claude Code is scrolled to is read off the screen ([`Conversation::locate`]): your
//! messages on their grey band after "❯ ", Claude's after "● ", and the header Claude Code keeps
//! at the top for the last message scrolled out of view, each matched to its block. A click
//! scrolls Claude Code with the keys it knows, reading the screen after each ([`Seek`]), as the
//! strip's prompt menu does (prompts.rs).

use crate::prompts::{self, Found, Row};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

mod pieces;
pub use pieces::{Chunks, Events, CHUNK};
use pieces::{hash, head, By, PROMPT_HEAD, TEXT_HEAD};

/// What a mark stands for. Lachlan picked all ten kinds of event (3abcdefghij, 2026-10-28).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Something you sent
    Prompt,
    /// Claude asked you something, in text or on a picker
    Question,
    /// A permission prompt you answered yes
    Permission,
    /// A tool call you (or auto mode) turned down
    Denied,
    /// A tool call that failed, or an API error
    Error,
    /// The conversation was compacted: Claude Code can't scroll above it
    Compaction,
    /// A subagent, background command or monitor started
    Agent,
    /// A git commit, or a branch landed
    Commit,
    /// A file edited or written
    Edit,
    /// An InMail sent, or something filed for later
    Mail,
    /// Claude handed the turn back
    TurnEnd,
    /// You pressed Esc
    Interrupt,
}

/// How a mark is drawn on the strip
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Across the strip, two pixels tall: your prompts
    Bar,
    /// A dot in the middle
    Dot,
    /// A short hairline at the strip's left: frequent and quiet
    Tick,
    /// A hairline right across, the part above it dimmed
    Line,
}

impl Kind {
    pub const ALL: [Kind; 12] = [
        Kind::Prompt,
        Kind::Question,
        Kind::Permission,
        Kind::Denied,
        Kind::Error,
        Kind::Compaction,
        Kind::Agent,
        Kind::Commit,
        Kind::Edit,
        Kind::Mail,
        Kind::TurnEnd,
        Kind::Interrupt,
    ];

    /// For the state file
    pub fn name(self) -> &'static str {
        match self {
            Kind::Prompt => "prompt",
            Kind::Question => "question",
            Kind::Permission => "permission",
            Kind::Denied => "denied",
            Kind::Error => "error",
            Kind::Compaction => "compaction",
            Kind::Agent => "agent",
            Kind::Commit => "commit",
            Kind::Edit => "edit",
            Kind::Mail => "mail",
            Kind::TurnEnd => "turn-end",
            Kind::Interrupt => "interrupt",
        }
    }

    /// What the card calls it
    pub fn label(self) -> &'static str {
        match self {
            Kind::Prompt => "You",
            Kind::Question => "Asked you",
            Kind::Permission => "Permission",
            Kind::Denied => "Denied",
            Kind::Error => "Error",
            Kind::Compaction => "Compacted",
            Kind::Agent => "Started",
            Kind::Commit => "Commit",
            Kind::Edit => "Edited",
            Kind::Mail => "Sent",
            Kind::TurnEnd => "Done",
            Kind::Interrupt => "Esc",
        }
    }

    /// Its colour, 0xRRGGBB: the rail's where they mean the same (green working and done,
    /// orange waiting on you, red wrong, yellow the idle timer's), the old scrollbar marks' coral
    /// for your prompts
    pub fn colour(self) -> u32 {
        match self {
            Kind::Prompt => 0xFF8A80,
            Kind::Question => 0xFF8A3D,
            Kind::Permission => 0xFFC46B,
            Kind::Denied => 0xFF5449,
            Kind::Error => 0xFF5449,
            Kind::Compaction => 0xB4B4CC,
            Kind::Agent => 0x4EC9B0,
            Kind::Commit => 0x3DDC84,
            Kind::Edit => 0x7A9CC6,
            Kind::Mail => 0xC586C0,
            Kind::TurnEnd => 0x8A8A9E,
            Kind::Interrupt => 0xF2E86D,
        }
    }

    pub fn shape(self) -> Shape {
        match self {
            Kind::Prompt => Shape::Bar,
            Kind::Compaction => Shape::Line,
            Kind::Edit | Kind::TurnEnd => Shape::Tick,
            _ => Shape::Dot,
        }
    }

    /// Drawing order, quietest first, so the marks that matter most end up on top
    pub fn rank(self) -> u8 {
        match self {
            Kind::TurnEnd => 0,
            Kind::Edit => 1,
            Kind::Compaction => 2,
            Kind::Agent => 3,
            Kind::Mail => 4,
            Kind::Commit => 5,
            Kind::Permission => 6,
            Kind::Interrupt => 7,
            Kind::Denied => 8,
            Kind::Error => 9,
            Kind::Question => 10,
            Kind::Prompt => 11,
        }
    }
}

/// What a block is, as Claude Code draws it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// A message you sent, on the grey band after "❯ "
    Prompt,
    /// What Claude wrote, after "● "
    Text,
    /// A run of tool calls with nothing written between them, which Claude Code folds into a
    /// line ("Read 2 files, ran 6 shell commands"), less the ones it shows each of
    Tools,
    /// A tool call Claude Code shows on its own: "● Update(path)" and the diff, "● Write(path)"
    /// and the start of the file, a subagent, a skill, a picker's answers
    Tool,
    /// "✻ Worked for 2m 36s"
    TurnEnd,
    /// "Interrupted · What should Claude do instead?"
    Interrupt,
    /// "Conversation compacted"
    Compaction,
    /// Anything else Claude Code shows a line for: a task finishing, another session's message
    Note,
}

/// One thing Claude Code draws in the conversation
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// Each line it draws, with what its rows at a width need
    pub lines: Vec<Line>,
    /// What a seek looks for on screen: a prompt's [`prompts::key`] after "❯ ", a text's first
    /// line after "● ", a tool's header, run together
    pub key: Option<String>,
    /// When, in Unix milliseconds
    pub at_ms: i64,
    /// Its words, cut short, for the card
    pub text: String,
    /// Each line's start as [`bare`] has it, for finding a row inside the block on screen;
    /// empty for a line too short to tell apart
    pub line_keys: Vec<String>,
    /// The line its key is from: the first with words (a table's header, under its rule)
    pub key_line: usize,
}

/// One line Claude Code draws, as much of it as its rows at a width take (measured on 2.1.312,
/// lbterm/SCROLL-STRIP.md, "What Claude Code draws")
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// Words wrapped at spaces the way Ink wraps them: the width of each word between single
    /// spaces, `lead` columns before them on every row (the message's two, a list item's hang)
    Words { words: Vec<u16>, lead: u16 },
    /// Wrapped wherever the row ends, as a diff's code is: `width` columns after `lead`
    Cut { width: u16, lead: u16 },
    /// The start of something long (a new file, a command's output): the [`Line::Capped`] lines
    /// after it take no more than `most` rows together, and a row more saying how many lines
    /// more when they're cut ("… +172 lines"). It takes none itself.
    Cap { most: u16 },
    /// A line of what a [`Line::Cap`] cuts short, wrapped like [`Line::Cut`]
    Capped { width: u16, lead: u16 },
    /// Always one row: a tool's header, which Claude Code shortens to fit
    Row,
}

/// The rows each of `lines` takes at `cols` wide, in order: its own, and the row after a capped
/// run's last line saying how many more. A capped run's lines past its rows take none.
fn line_parts(lines: &[Line], cols: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut left: Option<usize> = None;
    let mut cut = false;
    lines.iter().enumerate().map(move |(i, line)| match line {
        Line::Cap { most } => {
            left = Some(*most as usize);
            cut = false;
            (0, 0)
        }
        Line::Capped { .. } => {
            let want = line.rows(cols);
            let room = left.unwrap_or(usize::MAX);
            let rows = want.min(room);
            cut |= want > room;
            left = Some(room - rows);
            let last = !matches!(lines.get(i + 1), Some(Line::Capped { .. }));
            (rows, usize::from(last && cut))
        }
        _ => (line.rows(cols), 0),
    })
}

/// The rows each of `lines` takes at `cols` wide, in order ([`line_parts`] added up)
fn line_rows(lines: &[Line], cols: usize) -> impl Iterator<Item = usize> + '_ {
    line_parts(lines, cols).map(|(own, more)| own + more)
}

/// The rows words take at `room` columns, wrapped as wrap-ansi wraps them with `hard` on and
/// `trim` off (Ink's wrap): a word that doesn't fit goes to the next row, one longer than a row
/// is broken where the rows end
fn word_rows(words: &[u16], room: usize) -> usize {
    let room = room.max(1);
    let (mut rows, mut len) = (1usize, 0usize);
    for (i, &w) in words.iter().enumerate() {
        let w = w as usize;
        if i > 0 {
            if len >= room {
                rows += 1;
                len = 0;
            }
            len += 1;
        }
        if w > room {
            let this_line = 1 + (w - room.saturating_sub(len)).saturating_sub(1) / room;
            let next_line = (w - 1) / room;
            if next_line < this_line {
                rows += 1;
                len = 0;
            }
            let space = room.saturating_sub(len);
            if w <= space {
                len += w;
                continue;
            }
            let mut left = w - space;
            rows += 1;
            while left > room {
                left -= room;
                rows += 1;
            }
            len = left;
            continue;
        }
        if len + w > room && len > 0 && w > 0 {
            rows += 1;
            len = 0;
        }
        len += w;
    }
    rows
}

fn cut_rows(width: usize, room: usize) -> usize {
    width.max(1).div_ceil(room.max(1))
}

impl Line {
    /// The rows it takes in a pane `cols` wide
    pub fn rows(&self, cols: usize) -> usize {
        let room = |lead: u16| cols.saturating_sub(lead as usize).max(8);
        match self {
            Line::Words { words, lead } => word_rows(words, room(*lead)),
            Line::Cut { width, lead } | Line::Capped { width, lead } => cut_rows(*width as usize, room(*lead)),
            Line::Cap { .. } => 0,
            Line::Row => 1,
        }
    }

    /// Words' line, `lead` columns before every row
    fn words(text: &str, lead: u16) -> Self {
        Line::Words { words: text.split(' ').map(|w| width(w)).collect(), lead }
    }
}

/// The columns `text` takes: its characters, a tab as four
fn width(text: &str) -> u16 {
    text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>().min(u16::MAX as usize) as u16
}

/// The most of a block's words kept for the card
pub const TEXT_MAX: usize = 400;
/// The rows Claude Code draws above the first message: a blank, its logo with the version,
/// model and folder beside it, and two blanks (the first message's blank line is its own)
pub const BANNER_ROWS: usize = 5;

impl Block {
    fn new(kind: BlockKind, at_ms: i64) -> Self {
        Self {
            kind,
            lines: vec![],
            key: None,
            at_ms,
            text: String::new(),
            line_keys: vec![],
            key_line: 0,
        }
    }

    /// A block of one row, its line's key empty: a turn's end, a fold of tool calls
    fn row(kind: BlockKind, at_ms: i64) -> Self {
        let mut block = Self::new(kind, at_ms);
        block.lines = vec![Line::Row];
        block.line_keys = vec![String::new()];
        block
    }

    /// How many rows it takes in a pane `cols` wide, the blank line before it included
    pub fn rows(&self, cols: usize) -> usize {
        1 + line_rows(&self.lines, cols).sum::<usize>().max(1)
    }

    /// The row line `line` starts on, from the block's first row (the blank line before it), in
    /// a pane `cols` wide
    pub fn row_of_line(&self, line: usize, cols: usize) -> usize {
        1 + line_rows(&self.lines, cols).take(line).sum::<usize>()
    }

    /// Whether line `line` is drawn at `cols` wide, with a key to find it by: not a line of a
    /// file's start past the rows Claude Code shows of it
    pub fn finds_line(&self, line: usize, cols: usize) -> bool {
        self.line_keys.get(line).is_some_and(|k| !k.is_empty())
            && line_parts(&self.lines, cols).nth(line).is_some_and(|(own, _)| own > 0)
    }
}

/// One mark: what it stands for, the block it's on, when, and what the card says of it
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: Kind,
    pub block: usize,
    pub at_ms: i64,
    pub text: String,
}

/// A stretch of the conversation that a message to Claude starts: one of his prompts, with the
/// questions it names by number, or a message of another's (a task finishing, another session's).
/// The questions pane reads them (crate::questions::pane): a reply's questions are answered by
/// his prompts in the turns after the one the reply ended in.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// When the message that starts it was sent
    pub at_ms: i64,
    /// When the turn last wrote something a snapshot shows: its message's time until Claude
    /// writes after it
    pub last_ms: i64,
    /// The questions his prompt names ([`crate::drafts::picks_in`]); none for another's message
    pub picks: Vec<crate::drafts::Pick>,
    /// It's started by his prompt, typed or queued while Claude worked: not a task finishing or
    /// another session's message. A prompt of his in words answers a reply that asked one
    /// question (crate::questions::pane::rows, Lachlan's 6a of 2026-11-14)
    pub his: bool,
}

/// A tool call, kept until its result says how it went
#[derive(Debug, Clone)]
struct ToolUse {
    name: String,
    block: usize,
    at_ms: i64,
    /// What the card says of it: the command, the file, the description
    what: String,
    /// It was turned down: "The user doesn't want to proceed with this tool use"
    rejected: bool,
    /// It failed, with the first line of why
    error: Option<String>,
    /// Its kind of event, from the call itself (a commit, an edit, a subagent...)
    kind: Option<Kind>,
    /// Its input, kept until its result says what its block draws, for a call shown on its own
    input: Option<Value>,
}

/// A conversation as read so far: what Claude Code draws, and the marks on it.
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    pub blocks: Chunks<Arc<Block>>,
    /// Marks found in the transcript as it's read, by the piece of the blocks they're on
    /// ([`CHUNK`]); [`Conversation::events`] adds the tool calls' and the permission prompts'
    marks: Vec<Vec<Event>>,
    /// Each tool call in the order made, so in the order of their blocks
    tools: Vec<ToolUse>,
    by_id: HashMap<String, usize>,
    /// The block the last compaction is: Claude Code can't scroll above it
    pub compacted_at: Option<usize>,
    /// The files in its context now (crate::context), for the pill under the InMail button:
    /// shared with the snapshots, and copied only when one changes
    pub context: Arc<crate::context::Context>,
    /// What the records of Claude Code's requests say, for the context's scale and Claude Code's
    /// parts (crate::context::Requests)
    requests: crate::context::Requests,
    /// Each `Read` of a memory file waiting on its result, by its call's id: the file's path
    memory_reads: HashMap<String, String>,
    /// The tools block a run of calls is adding to, until something else is drawn
    open_tools: Option<usize>,
    /// The turn's last text block, for a question at its end
    last_text: Option<usize>,
    /// Permission prompts claudehook recorded: when, and for which tool
    permissions: Vec<(i64, String)>,
    /// The call each permission prompt was for, as [`Conversation::events`] matches them, kept as
    /// calls come; and how many each call has
    matched: Vec<Option<usize>>,
    asked: Vec<u32>,
    /// The session's scratchpad ([`scratchpad_of`]), where an edit folds; empty when unknown
    pub scratchpad: String,
    /// Each turn a message to Claude started, in order, in pieces a snapshot shares: a turn
    /// that's still writing copies only its own piece
    pub turns: Chunks<Turn>,
    /// The pieces of the last [`Snapshot`]'s marks and of where to find things in it, and the first
    /// block changed since (a snapshot builds again only the pieces from that one's on)
    events_pieces: Vec<Arc<Vec<Event>>>,
    found_pieces: Vec<Arc<pieces::Found>>,
    dirty: usize,
    /// Bumped by every change a snapshot would show
    revision: u64,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// When a record was written, in Unix milliseconds: its `timestamp`, "2026-10-28T02:52:40.143Z"
fn stamp(record: &Value) -> i64 {
    str_of(record, "timestamp")
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map_or(0, |t| t.timestamp_millis())
}

/// At most `max` characters of `text`, with an ellipsis when cut
pub fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// A markdown link's words without its address: "[e71c88f9](lbterm://session/…)" is drawn
/// "e71c88f9"
fn unlinked(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            let target = &after[close + 2..];
            target.find(')').map(|end| (close, close + 2 + end + 1))
        });
        match link {
            Some((close, past)) if !after[..close].contains('[') => {
                out.push_str(&rest[..open]);
                out.push_str(&after[..close]);
                rest = &after[past..];
            }
            _ => {
                out.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text as Claude Code shows it, near enough to count its width: markdown's marks taken out
/// (headings', emphasis's, code's, a link's address)
fn shown(line: &str) -> String {
    let line = line.trim_end();
    let trimmed = line.trim_start();
    let line = if trimmed.starts_with('#') {
        trimmed.trim_start_matches('#').trim_start()
    } else {
        line
    };
    // A list's "* " stays; every other star is emphasis
    let (lead, body) = match line.trim_start().strip_prefix("* ") {
        Some(body) => (&line[..line.len() - body.len()], body),
        None => ("", line),
    };
    format!("{lead}{}", unlinked(body).replace('*', "").replace('`', ""))
}

/// The columns a list item's words hang in from the line's start, past its marker: "- ",
/// "* ", "+ ", "12. ", "3) " and the spaces before it; 0 for a line that isn't one
fn hang(line: &str) -> usize {
    let t = line.trim_start();
    let before = line.len() - t.len();
    let marker = if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        2
    } else {
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        match t[digits..].chars().next() {
            Some('.' | ')') if digits > 0 && t[digits + 1..].starts_with(' ') => digits + 2,
            _ => return 0,
        }
    };
    before + marker
}

/// What Claude Code draws of a message's markdown, line by line, near enough to count its rows
/// and find its lines on screen (measured on 2.1.312): code fences dropped, the code between
/// them as it is; a table boxed, with a rule above it, between every row and below it; elsewhere
/// headings', emphasis's and links' marks taken out, and blank lines run together
fn rendered(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = vec![];
    let mut fenced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
            i += 1;
            continue;
        }
        if fenced {
            out.push(line.trim_end().to_string());
            i += 1;
            continue;
        }
        if t.is_empty() {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            i += 1;
            continue;
        }
        if t.starts_with('|') {
            let mut rows = vec![];
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                let row = lines[i].trim();
                // The line of dashes under the header is the box's own rule
                if !row.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                    rows.push(shown(row).replace('|', "\u{2502}"));
                }
                i += 1;
            }
            let rule = "\u{2500}".repeat(rows.iter().map(|r| r.chars().count()).max().unwrap_or(0));
            out.push(rule.clone());
            for row in rows {
                out.push(row);
                out.push(rule.clone());
            }
            continue;
        }
        out.push(shown(line));
        i += 1;
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

/// A message's lines as Claude Code wraps them: two columns in from the dot, a list item's
/// words hanging past its marker
fn text_lines(drawn: &[String]) -> Vec<Line> {
    drawn
        .iter()
        .map(|l| {
            let h = hang(l);
            Line::words(l.get(h..).unwrap_or(""), 2 + h as u16)
        })
        .collect()
}

/// Letters and digits, lowercased: what's compared between a block and the screen, since
/// Claude Code's markdown and wrapping change the rest
fn bare(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// How much of a text's first line a seek matches on screen
const TEXT_KEY: usize = 32;
/// A shorter row a key only starts with doesn't count: too likely by chance
const MIN_MATCH: usize = 10;

/// A text block's key: its first line with words, as [`bare`] has it
fn text_key(text: &str) -> Option<String> {
    let first = rendered(text).into_iter().find(|l| !bare(l).is_empty())?;
    Some(bare(&first).chars().take(TEXT_KEY).collect())
}

/// Whether a row shows the start of a block whose key is `key`: the row starts with it, or is
/// the start of it cut by the wrap. A key shorter than the most kept is the whole first line, so
/// a row saying more is another block's ("yes" isn't "yes, and tidy up").
fn shows_key(row: &str, key: &str, most: usize, least: usize) -> bool {
    let n = row.chars().count();
    let whole = key.chars().count() < most;
    !key.is_empty()
        && ((if whole { row == key } else { row.starts_with(key) }) || (n >= least && key.starts_with(row)))
}

/// Whether a row shows the start of Claude's words whose key is `key` ([`Snapshot::candidates`]
/// works out `bare` of the row once for every block it tries)
#[cfg(test)]
fn matches_text(row: &str, key: &str) -> bool {
    shows_key(&bare(row), key, TEXT_KEY, MIN_MATCH)
}

/// Whether a row after "❯ " shows the prompt whose key is `key`. A short prompt shows whole, so
/// it's the same words ("message number 1" isn't "message number 10"); one cut short with an
/// ellipsis (the header's) is any prompt it's the start of.
fn prompt_matches(shown: &str, key: &str) -> bool {
    let cut = shown.trim_end().ends_with('\u{2026}');
    let shown = shown.trim_end_matches(['\u{2026}', ' ']);
    shows_key(shown, key, prompts::KEY_CHARS, if cut { 12 } else { usize::MAX })
}

/// Whether Claude Code's sticky header, `shown` after "❯ ", names the prompt whose key is `key`
/// and whose words on one line are `whole` (its [`Block::text`]): the header shows all of a
/// prompt on one line, its lines run together, cut short with "…" when it doesn't fit
fn header_names(shown: &str, key: &str, whole: &str) -> bool {
    let cut = shown.trim_end().ends_with('\u{2026}');
    let words = shown.trim_end_matches(['\u{2026}', ' ']);
    prompt_matches(shown, key) || if cut { words.chars().count() >= 12 && whole.starts_with(words) } else { whole == words }
}

/// Where the prompt whose key is `key` (and whose words are `whole`) is on `rows`, the
/// conversation's area of the screen top to bottom: the lowest row on the band in bright words
/// that shows it, else the first row if it's Claude Code's sticky header and names it
/// (prompts.rs's `find`, with the stricter match)
fn find_prompt(rows: &[Row], key: &str, whole: &str) -> Option<Found> {
    let after = |row: &Row| row.text.trim_start().strip_prefix(prompts::MARK).map(prompts::one_line);
    if let Some(i) = rows
        .iter()
        .rposition(|row| row.band && !row.dim && after(row).is_some_and(|s| prompt_matches(&s, key)))
    {
        return Some(Found::Row(i));
    }
    rows.first()
        .filter(|row| is_header(row) && after(row).is_some_and(|s| header_names(&s, key, whole)))
        .map(|_| Found::Header(0))
}

/// Whether a reply asks you something: a line ending in "?" in its last paragraph, outside
/// code, quotes, tables and headings, or a question labelled Q1, or extras offered. A rougher
/// cut than claudehook's (Terminal's `text::TextQuestion`), for a mark rather than a light.
pub fn asks(text: &str) -> Option<String> {
    let mut fenced = false;
    let mut paragraphs: Vec<Vec<&str>> = vec![vec![]];
    let mut labelled: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.starts_with('>') || t.starts_with('|') || t.starts_with('#') {
            continue;
        }
        if t.is_empty() {
            if !paragraphs.last().map_or(true, Vec::is_empty) {
                paragraphs.push(vec![]);
            }
            continue;
        }
        let plain = t.trim_start_matches(['*', '-', ' ']);
        if labelled.is_none() && (plain.starts_with("Q1") || plain.starts_with("Extras")) {
            labelled = Some(t);
        }
        if let Some(p) = paragraphs.last_mut() {
            p.push(t);
        }
    }
    let last = paragraphs.iter().rev().find(|p| !p.is_empty())?;
    let asked = last
        .iter()
        .rev()
        .find(|l| l.trim_end_matches(['*', '_', ')']).ends_with('?'))
        .copied()
        .or(labelled)?;
    Some(cut(&shown(asked), TEXT_MAX))
}

/// The words after the first of `flags` in a command line, unquoted: a commit's `-m "..."`,
/// or its `-am "..."`, a one-letter flag at the end of a run of them
fn flag_value(command: &str, flags: &[&str]) -> Option<String> {
    let mut at = 0;
    for word in command.split_inclusive(char::is_whitespace) {
        let start = at;
        at += word.len();
        let token = word.trim_end();
        let hit = flags.iter().any(|&flag| {
            token == flag
                || (flag.len() == 2
                    && flag.starts_with('-')
                    && token.starts_with('-')
                    && !token.starts_with("--")
                    && token.len() > 2
                    && token.chars().skip(1).all(|c| c.is_ascii_alphabetic())
                    && token.ends_with(&flag[1..]))
        });
        if !hit || start + token.len() >= command.len() {
            continue;
        }
        let rest = command[start + token.len()..].trim_start();
        let quote = rest.chars().next()?;
        if quote == '"' || quote == '\'' {
            let body = &rest[1..];
            let end = body.find(quote).unwrap_or(body.len());
            return Some(body[..end].to_string());
        }
        return rest.split_whitespace().next().map(String::from);
    }
    None
}

/// The commands a shell line runs, split at `&&`, `||`, `;`, `|` and new lines, each as words
fn commands(line: &str) -> Vec<Vec<&str>> {
    line.split(|c| c == ';' || c == '\n' || c == '|' || c == '&')
        .map(|part| part.split_whitespace().collect::<Vec<_>>())
        .filter(|words| !words.is_empty())
        .collect()
}

/// What a shell command is, for a mark: a commit or a landing, an InMail sent or something
/// filed for later, else nothing
pub fn shell_kind(command: &str) -> Option<(Kind, String)> {
    for words in commands(command) {
        let name = |w: &str| {
            let w = w.trim_matches(['"', '\'', '(']);
            w.rsplit(['/', '\\']).next().unwrap_or(w).to_ascii_lowercase()
        };
        let first = name(words[0]);
        let has = |word: &str| words.iter().any(|w| name(w) == word);
        if first == "git" && has("commit") {
            let message = flag_value(command, &["-m", "--message"]).unwrap_or_else(|| "git commit".into());
            return Some((Kind::Commit, message));
        }
        if first == "land" || first == "land.ps1" {
            let message = flag_value(command, &["-m"]).unwrap_or_default();
            let branch = words.get(1).copied().unwrap_or("");
            return Some((Kind::Commit, format!("land {branch} {message}").trim().to_string()));
        }
        let tool = words.iter().map(|w| name(w)).find(|w| w == "inmail.ts" || w == "inmail" || w == "later" || w == "later.ts");
        if let Some(tool) = tool {
            if tool.starts_with("inmail") && has("send") {
                return Some((Kind::Mail, cut(command.trim(), TEXT_MAX)));
            }
            if tool.starts_with("later") && has("add") {
                return Some((Kind::Mail, cut(command.trim(), TEXT_MAX)));
            }
        }
    }
    None
}

/// What the card says of a tool call, and its kind of event when it has one of its own
fn describe_tool(name: &str, input: &Value) -> (String, Option<Kind>) {
    let s = |k: &str| str_of(input, k).unwrap_or("").to_string();
    let file = |k: &str| {
        let path = s(k);
        path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string()
    };
    match name {
        "Bash" | "PowerShell" => {
            let command = s("command");
            if input.get("run_in_background").and_then(Value::as_bool) == Some(true) {
                let what = if s("description").is_empty() { command.clone() } else { s("description") };
                return (cut(&what, TEXT_MAX), Some(Kind::Agent));
            }
            match shell_kind(&command) {
                Some((kind, what)) => (what, Some(kind)),
                None => (cut(command.trim(), TEXT_MAX), None),
            }
        }
        "Edit" | "Write" | "MultiEdit" => (file("file_path"), Some(Kind::Edit)),
        "NotebookEdit" => (file("notebook_path"), Some(Kind::Edit)),
        "Agent" | "Task" => {
            let what = [s("description"), s("subagent_type")].into_iter().find(|w| !w.is_empty()).unwrap_or_default();
            (cut(&what, TEXT_MAX), Some(Kind::Agent))
        }
        "Monitor" => (cut(&s("description"), TEXT_MAX), Some(Kind::Agent)),
        "AskUserQuestion" => {
            let questions: Vec<String> = input
                .get("questions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|q| str_of(q, "question").map(String::from))
                .collect();
            (cut(&questions.join(" \u{b7} "), TEXT_MAX), Some(Kind::Question))
        }
        _ => {
            let what = ["description", "file_path", "pattern", "url", "query", "path"]
                .iter()
                .map(|k| s(k))
                .find(|w| !w.is_empty())
                .unwrap_or_default();
            (cut(&format!("{name} {what}").trim().to_string(), TEXT_MAX), None)
        }
    }
}

/// Tool calls Claude Code shows on their own rather than folding into a run's line. An edit in
/// the session's own scratchpad folds ("Made 1 scratchpad edit +4"): `scratchpad` is the end
/// of its path, as [`scratchpad_of`] has it.
fn shows_own(name: &str, input: &Value, scratchpad: &str) -> bool {
    match name {
        "Edit" | "Write" | "MultiEdit" => {
            let path = str_of(input, "file_path").unwrap_or("").replace('/', "\\").to_lowercase();
            scratchpad.is_empty() || !path.contains(scratchpad)
        }
        "NotebookEdit" | "Agent" | "Task" | "Skill" | "AskUserQuestion" => true,
        _ => false,
    }
}

/// The session's scratchpad, from its transcript's path (`~/.claude/projects/<project>/<sid>.jsonl`):
/// Claude Code keeps it in `%TEMP%\claude\<project>\<sid>\scratchpad`, so this is its path from
/// `\claude\` on, lowercased, as a file in it is matched
pub fn scratchpad_of(transcript: &std::path::Path) -> String {
    let project = transcript.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
    let sid = transcript.file_stem().map(|n| n.to_string_lossy().to_string());
    match (project, sid) {
        (Some(project), Some(sid)) => format!("\\claude\\{project}\\{sid}\\scratchpad\\").to_lowercase(),
        _ => String::new(),
    }
}

/// A path as Claude Code writes it: the home folder as "~"
fn home_path(path: &str) -> String {
    let p = path.replace('/', "\\");
    let mut parts = p.splitn(4, '\\');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(drive), Some(users), Some(_), rest) if drive.ends_with(':') && users.eq_ignore_ascii_case("users") => {
            rest.map_or("~".to_string(), |r| format!("~\\{r}"))
        }
        _ => p,
    }
}

/// The header of a tool call Claude Code shows on its own, as it starts on screen after "● "
fn tool_header(name: &str, input: &Value) -> String {
    let s = |k: &str| str_of(input, k).unwrap_or("").to_string();
    match name {
        "Edit" | "MultiEdit" => format!("Update({})", home_path(&s("file_path"))),
        "Write" => format!("Write({})", home_path(&s("file_path"))),
        "NotebookEdit" => format!("Edit Notebook({})", home_path(&s("notebook_path"))),
        "Agent" | "Task" => {
            let kind = s("subagent_type");
            format!("{}({})", if kind.is_empty() { "Agent".into() } else { kind }, s("description"))
        }
        "Skill" => format!("Skill({})", s("skill")),
        "AskUserQuestion" => "User answered Claude's questions:".into(),
        _ => name.to_string(),
    }
}

/// The longest line of a diff Claude Code draws: the rest is " … [+N chars]"
const DIFF_LINE_MAX: usize = 2000;
/// Columns right of a diff and its file previews, where the code wraps
const DIFF_MARGIN: u16 = 7;

/// A diff as Claude Code draws it under an edit: each hunk's lines, numbered in a gutter as wide
/// as its highest number, the code wrapped where the row ends (a line past [`DIFF_LINE_MAX`]
/// cut there), "..." between hunks; no more than `most` lines, then a row saying how many more.
/// Each line's key starts with its number, as the row does.
fn diff_lines(hunks: &[Value], most: Option<usize>) -> (Vec<Line>, Vec<String>) {
    let n = |h: &Value, k: &str| h.get(k).and_then(Value::as_u64).unwrap_or(0) as usize;
    let high = hunks
        .iter()
        .map(|h| (n(h, "oldStart") + n(h, "oldLines")).max(n(h, "newStart") + n(h, "newLines")).saturating_sub(1))
        .max()
        .unwrap_or(1);
    let lead = DIFF_MARGIN + 6 + high.max(1).to_string().len() as u16 + 2;
    let (mut lines, mut keys) = (vec![], vec![]);
    let mut shown = 0;
    let mut left = 0;
    for (k, h) in hunks.iter().enumerate() {
        if k > 0 && most.map_or(true, |m| shown < m) {
            lines.push(Line::Row);
            keys.push(String::new());
        }
        let (mut old, mut new) = (n(h, "oldStart"), n(h, "newStart"));
        for l in h.get("lines").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            let mut chars = l.chars();
            let marker = chars.next().unwrap_or(' ');
            let code = chars.as_str();
            let number = match marker {
                '-' => {
                    old += 1;
                    old - 1
                }
                '+' => {
                    new += 1;
                    new - 1
                }
                '\\' => continue,
                _ => {
                    old += 1;
                    new += 1;
                    new - 1
                }
            };
            if most.is_some_and(|m| shown >= m) {
                left += 1;
                continue;
            }
            shown += 1;
            let mut w = width(code) as usize;
            if w > DIFF_LINE_MAX {
                w = DIFF_LINE_MAX + format!(" \u{2026} [+{} chars]", w - DIFF_LINE_MAX).chars().count();
            }
            lines.push(Line::Cut { width: w.min(u16::MAX as usize) as u16, lead });
            keys.push(line_key(&format!("{number}{code}")));
        }
    }
    if left > 0 {
        lines.push(Line::Row);
        keys.push(String::new());
    }
    (lines, keys)
}

/// The rows of a file's start Claude Code shows under a Write that made it
const PREVIEW_ROWS: u16 = 10;
/// The most lines of each file Claude Code shows changed under a shell command
const COMMAND_DIFF_LINES: usize = 40;

/// What a tool call Claude Code shows on its own draws once its result (`toolUseResult`) is in,
/// with each line's key: its header, then under "⎿ " what came of it
fn tool_lines(name: &str, input: &Value, result: Option<&Value>) -> (Vec<Line>, Vec<String>) {
    let header = line_key(&tool_header(name, input));
    let mut lines = vec![Line::Row];
    let mut keys = vec![header];
    let patch = result.and_then(|r| r.get("structuredPatch")).and_then(Value::as_array);
    let created = result.and_then(|r| str_of(r, "type")) == Some("create");
    match (name, patch) {
        ("Write", _) if created => {
            let content = result.and_then(|r| str_of(r, "content")).or_else(|| str_of(input, "content")).unwrap_or("");
            let file: Vec<&str> = content.strip_suffix('\n').unwrap_or(content).split('\n').collect();
            let path = home_path(str_of(input, "file_path").unwrap_or(""));
            lines.push(Line::words(&format!("Wrote {} lines to {path}", file.len()), 5));
            keys.push(String::new());
            // Numbered like a diff's lines, no more than ten rows: the lines that could show,
            // and one more to say there are more
            lines.push(Line::Cap { most: PREVIEW_ROWS });
            keys.push(String::new());
            let lead = DIFF_MARGIN + 9;
            for (n, l) in file.iter().take(PREVIEW_ROWS as usize + 1).enumerate() {
                lines.push(Line::Capped { width: width(l), lead });
                keys.push(line_key(&format!("{}{l}", n + 1)));
            }
        }
        ("Edit" | "MultiEdit" | "Write", Some(hunks)) => {
            // "⎿  Added 2 lines, removed 1 line"
            lines.push(Line::Row);
            keys.push(String::new());
            let (diff, diff_keys) = diff_lines(hunks, None);
            lines.extend(diff);
            keys.extend(diff_keys);
        }
        ("AskUserQuestion", _) => {
            let asked = input.get("questions").and_then(Value::as_array).map_or(1, Vec::len);
            for _ in 0..asked.max(1) {
                lines.push(Line::Row);
                keys.push(String::new());
            }
        }
        // What came of it in a line: "Backgrounded agent", "Successfully loaded skill", an error
        _ => {
            lines.push(Line::Row);
            keys.push(String::new());
        }
    }
    (lines, keys)
}

/// What a shell command that changed files draws, which Claude Code shows on its own (since
/// 2.1.310 it diffs the files that changed while it ran, `toolUseResult.bashEditDiff`): the
/// command in two rows, three of its output, each file's diff, and its note
fn command_lines(result: &Value) -> Option<(Vec<Line>, Vec<String>)> {
    let files = result.get("bashEditDiff")?.get("files")?.as_array()?;
    let mut lines = vec![Line::Row, Line::Row];
    let mut keys = vec![String::new(), String::new()];
    let out = str_of(result, "stdout").unwrap_or("");
    lines.push(Line::Cap { most: 3 });
    keys.push(String::new());
    for l in out.trim_end().split('\n').take(4) {
        lines.push(Line::Capped { width: width(l), lead: 5 });
        keys.push(String::new());
    }
    for f in files {
        // "⎿  Updated ~\AI\Terminal\CLAUDE.md (+2 -2)"
        lines.push(Line::Row);
        keys.push(String::new());
        let hunks = f.get("hunks").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let (diff, diff_keys) = diff_lines(hunks, Some(COMMAND_DIFF_LINES));
        lines.extend(diff);
        keys.extend(diff_keys);
    }
    lines.push(Line::words(
        "(files that changed while this command ran, by the command or by another process; a convenience view, not a review or audit of the command)",
        5,
    ));
    keys.push(String::new());
    Some((lines, keys))
}

/// The words of a tool result: its text, or its text parts
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| str_of(p, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A task notification's summary, the line Claude Code draws for it: "Background command "…"
/// completed (exit code 0)", "Agent "…" finished"
fn notification(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.as_str(),
        Value::Array(parts) => parts.iter().find_map(|p| str_of(p, "text"))?,
        _ => return None,
    };
    let rest = text.trim_start().strip_prefix("<task-notification>")?;
    let summary = rest.split("<summary>").nth(1)?.split("</summary>").next()?;
    Some(prompts::one_line(summary))
}

/// What Claude Code says when a tool call was turned down
fn is_rejection(text: &str) -> bool {
    text.contains("doesn't want to proceed with this tool use") || text.contains("tool use was rejected")
}

/// What a prompt's record holds: its text, a slash command as typed, or nothing for Claude
/// Code's own notes
fn prompt_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| str_of(p, "type") == Some("text"))
            .filter_map(|p| str_of(p, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("<command-name>") {
        let name = rest.split("</command-name>").next().unwrap_or("").trim();
        let args = text
            .split("<command-args>")
            .nth(1)
            .and_then(|a| a.split("</command-args>").next())
            .unwrap_or("")
            .trim();
        return (!name.is_empty()).then(|| format!("{name} {args}").trim().to_string());
    }
    if text.is_empty() || text.starts_with('<') {
        return None;
    }
    Some(text.to_string())
}

impl Conversation {
    /// Take in one record of the transcript
    pub fn record(&mut self, record: &Value) {
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let at_ms = stamp(record);
        // The first folder it records is where it started: its own, for the context's files
        if self.context.own.is_none() {
            if let Some(cwd) = str_of(record, "cwd") {
                self.context_change(|c| c.saw_folder(cwd));
            }
        }
        let was = self.revision;
        match str_of(record, "type") {
            Some("user") => self.user(record, at_ms),
            Some("assistant") => self.assistant(record, at_ms),
            Some("system") => self.system(record, at_ms),
            Some("attachment") => self.attachment(record, at_ms),
            // Claude Code's requests, for the context bar's scale and Claude Code's own parts
            // (crate::context::Requests)
            Some("api-request-shape") => self.requests.shape(record),
            Some("api-request-blob") => self.requests.blob(record),
            Some("api-request") => {
                if let Some(shape) = self.requests.request(record).filter(|s| self.context.requests_differ(*s)) {
                    self.context_change(|c| c.requested(shape));
                }
            }
            _ => {}
        }
        // The turn wrote what a snapshot shows: a record that changed nothing leaves its time,
        // so a snapshot's turns are as new as its blocks
        if self.revision != was && at_ms > 0 {
            if let Some(last) = self.turns.len().checked_sub(1) {
                if self.turns[last].last_ms < at_ms {
                    self.turns.make_mut(last).last_ms = at_ms;
                }
            }
        }
    }

    /// A message to Claude starts a turn: his prompt, with the questions it names, or another's
    fn turn(&mut self, at_ms: i64, text: Option<&str>) {
        let picks = text.map(crate::drafts::picks_in).unwrap_or_default();
        self.turns.push(Turn { at_ms, last_ms: at_ms, picks, his: text.is_some() });
    }

    /// Whether the records of message blobs are wanted: only until the first main request's
    /// reply has made the context's scale
    pub fn wants_blobs(&self) -> bool {
        !self.requests.done
    }

    /// Change the context with `change`, which says whether it changed anything, and the revision
    /// moves when it did so a snapshot takes it. Changed in place while no snapshot shares it (a
    /// transcript read from its start takes none until the read is done); else on a copy, kept
    /// only when it changed. The records that come often (each reply's usage, each request) ask
    /// the context whether they'd change it before they come here, so they copy nothing when not
    fn context_change(&mut self, change: impl FnOnce(&mut crate::context::Context) -> bool) {
        let changed = match Arc::get_mut(&mut self.context) {
            Some(context) => change(context),
            None => {
                let mut context = (*self.context).clone();
                let changed = change(&mut context);
                if changed {
                    self.context = Arc::new(context);
                }
                changed
            }
        };
        if changed {
            self.revision += 1;
        }
    }

    /// Something a snapshot shows changed on block `block`
    fn changed(&mut self, block: usize) {
        self.dirty = self.dirty.min(block);
        self.revision += 1;
    }

    /// Bumped by every change a snapshot would show: a new snapshot is needed only when it moved
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn push(&mut self, block: Block) -> usize {
        if block.kind != BlockKind::Tools {
            self.open_tools = None;
        }
        self.blocks.push(Arc::new(block));
        let i = self.blocks.len() - 1;
        self.changed(i);
        i
    }

    fn mark(&mut self, kind: Kind, block: usize, at_ms: i64, text: String) {
        let piece = block / CHUNK;
        if self.marks.len() <= piece {
            self.marks.resize_with(piece + 1, Vec::new);
        }
        self.marks[piece].push(Event { kind, block, at_ms, text });
        self.changed(block);
    }

    /// A tool call made: its kind's mark goes on its block, and a permission prompt recorded for
    /// that tool by then is this call's now if it's the latest before it ([`Conversation::events`])
    fn add_tool(&mut self, tool: ToolUse) {
        let (block, at_ms) = (tool.block, tool.at_ms);
        let i = self.tools.len();
        let name = tool.name.clone();
        self.tools.push(tool);
        self.asked.push(0);
        self.changed(block);
        for j in 0..self.permissions.len() {
            let (at, for_tool) = &self.permissions[j];
            if *for_tool == name && at_ms <= at + 1000 {
                let was = self.matched[j].replace(i);
                self.ask(was, Some(i));
            }
        }
    }

    /// A permission prompt moved from call `was` to call `now`
    fn ask(&mut self, was: Option<usize>, now: Option<usize>) {
        if was == now {
            return;
        }
        if let Some(w) = was {
            self.asked[w] -= 1;
            if self.asked[w] == 0 {
                self.changed(self.tools[w].block);
            }
        }
        if let Some(n) = now {
            self.asked[n] += 1;
            if self.asked[n] == 1 {
                self.changed(self.tools[n].block);
            }
        }
    }

    fn user(&mut self, record: &Value, at_ms: i64) {
        if record.get("isMeta").and_then(Value::as_bool) == Some(true)
            || record.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        let Some(content) = record.get("message").and_then(|m| m.get("content")) else {
            return;
        };
        // Tool results: how each call went
        if let Value::Array(parts) = content {
            let mut any = false;
            for part in parts.iter().filter(|p| str_of(p, "type") == Some("tool_result")) {
                any = true;
                // A memory file Claude read: its own section of the context bar
                if let Some(path) = str_of(part, "tool_use_id").and_then(|id| self.memory_reads.remove(id)) {
                    if part.get("is_error").and_then(Value::as_bool) != Some(true) {
                        let text = result_text(part.get("content").unwrap_or(&Value::Null));
                        self.context_change(|c| c.remembered(&path, &text));
                    }
                }
                let Some(&i) = str_of(part, "tool_use_id").and_then(|id| self.by_id.get(id)) else {
                    continue;
                };
                let text = result_text(part.get("content").unwrap_or(&Value::Null));
                let tool = &mut self.tools[i];
                let block = tool.block;
                let mut said = false;
                if is_rejection(&text) {
                    tool.rejected = true;
                    said = true;
                } else if part.get("is_error").and_then(Value::as_bool) == Some(true) {
                    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("failed");
                    tool.error = Some(cut(first, TEXT_MAX));
                    said = true;
                }
                // What its block draws, now its result is in
                let result = record.get("toolUseResult").filter(|r| r.is_object());
                let drawn = match tool.input.take() {
                    Some(input) => Some((tool_lines(&tool.name, &input, result), true)),
                    None => result.and_then(command_lines).map(|lines| (lines, false)),
                };
                if let Some(((lines, keys), replace)) = drawn {
                    let b = Arc::make_mut(self.blocks.make_mut(block));
                    if replace {
                        b.lines = lines;
                        b.line_keys = keys;
                    } else {
                        b.lines.extend(lines);
                        b.line_keys.extend(keys);
                    }
                    said = true;
                }
                if said {
                    self.changed(block);
                }
            }
            if any {
                return;
            }
        }
        // A background command or a subagent finishing: "● Background command "…" completed"
        if let Some(summary) = notification(content) {
            self.turn(at_ms, None);
            self.note(&summary, at_ms);
            return;
        }
        let Some(text) = prompt_text(content) else {
            return;
        };
        if text.starts_with("[Request interrupted by user") {
            let block = self.push(Block::new(BlockKind::Interrupt, at_ms));
            self.mark(Kind::Interrupt, block, at_ms, "Interrupted".into());
            return;
        }
        let origin = record.get("origin").and_then(|o| str_of(o, "kind"));
        if origin.is_some_and(|o| o != "human") {
            // A task finishing, another session's message: a line of Claude Code's, no mark
            self.turn(at_ms, None);
            self.push(Block::new(BlockKind::Note, at_ms));
            return;
        }
        self.turn(at_ms, Some(&text));
        self.prompt(&text, at_ms);
    }

    /// His prompt's block and its mark on the scroll strip: typed when Claude waited (a `user`
    /// record), or typed while Claude worked (a `queued_command` attachment), which Claude Code
    /// draws in the conversation with the same component (read from its bundle on 2026-11-14:
    /// both render through the one user-text message component)
    fn prompt(&mut self, text: &str, at_ms: i64) {
        let mut block = Block::new(BlockKind::Prompt, at_ms);
        block.lines = text.lines().map(|l| Line::words(l, 2)).collect();
        block.line_keys = text.lines().map(line_key).collect();
        block.key = Some(prompts::key(text));
        block.text = cut(&prompts::one_line(text), TEXT_MAX);
        let words = block.text.clone();
        let i = self.push(block);
        self.last_text = None;
        self.mark(Kind::Prompt, i, at_ms, words);
    }

    fn assistant(&mut self, record: &Value, at_ms: i64) {
        let Some(message) = record.get("message") else {
            return;
        };
        // What's in use: the size of the request this reply answers, and the first's makes the
        // context's scale (crate::context::InUse, Scale)
        if let Some(usage) = message.get("usage") {
            // Asked first, so a reply repeating the count (each of a turn's records) copies nothing
            if let Some((in_use, scale)) = self.context.answer(usage, self.requests.first()) {
                self.context_change(|c| {
                    (c.in_use, c.scale) = (in_use, scale);
                    c.replies += 1;
                    true
                });
            }
            if self.context.scale.is_some() && !self.requests.done {
                self.requests.finish();
            }
        }
        let api_error = record.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true);
        for part in message.get("content").and_then(Value::as_array).into_iter().flatten() {
            match str_of(part, "type") {
                // Claude's words, and its thinking where the transcript keeps its words (Claude
                // Code draws both after "● ")
                kind @ Some("text" | "thinking") => {
                    let thinking = kind == Some("thinking");
                    let text = str_of(part, if thinking { "thinking" } else { "text" }).unwrap_or("");
                    if text.trim().is_empty() {
                        continue;
                    }
                    let mut block = Block::new(BlockKind::Text, at_ms);
                    let drawn = rendered(text);
                    block.lines = text_lines(&drawn);
                    block.line_keys = drawn.iter().map(|l| line_key(l)).collect();
                    block.key_line = drawn.iter().position(|l| !bare(l).is_empty()).unwrap_or(0);
                    block.key = text_key(text);
                    block.text = cut(text.trim(), TEXT_MAX);
                    let i = self.push(block);
                    if thinking {
                        continue;
                    }
                    self.last_text = Some(i);
                    if api_error {
                        let first = text.lines().next().unwrap_or("").trim();
                        self.mark(Kind::Error, i, at_ms, cut(first, TEXT_MAX));
                    }
                }
                Some("tool_use") => {
                    let name = str_of(part, "name").unwrap_or("").to_string();
                    let input = part.get("input").unwrap_or(&Value::Null);
                    let own = shows_own(&name, input, &self.scratchpad);
                    let block = if own {
                        // Its header and a line under it until its result says what it draws
                        let mut b = Block::new(BlockKind::Tool, at_ms);
                        let header = line_key(&tool_header(&name, input));
                        b.key = (!header.is_empty()).then(|| header.clone());
                        b.lines = vec![Line::Row, Line::Row];
                        b.line_keys = vec![header, String::new()];
                        b.text = cut(&tool_header(&name, input), TEXT_MAX);
                        self.push(b)
                    } else {
                        match self.open_tools {
                            Some(i) => i,
                            None => {
                                let i = self.push(Block::row(BlockKind::Tools, at_ms));
                                self.open_tools = Some(i);
                                i
                            }
                        }
                    };
                    let (what, kind) = describe_tool(&name, input);
                    if let Some(id) = str_of(part, "id") {
                        self.by_id.insert(id.to_string(), self.tools.len());
                        // A memory file read: its result is its section of the context bar
                        if let Some(path) = (name == "Read").then(|| str_of(input, "file_path")).flatten() {
                            if self.context.is_memory_file(path) {
                                self.memory_reads.insert(id.to_string(), path.to_string());
                            }
                        }
                    }
                    self.add_tool(ToolUse {
                        name,
                        block,
                        at_ms,
                        what,
                        rejected: false,
                        error: None,
                        kind,
                        input: own.then(|| input.clone()),
                    });
                }
                _ => {}
            }
        }
    }

    /// A line of Claude Code's own in the conversation, no mark: a task finishing
    fn note(&mut self, words: &str, at_ms: i64) {
        let mut block = Block::new(BlockKind::Note, at_ms);
        block.lines = vec![Line::words(words, 2)];
        block.line_keys = vec![line_key(words)];
        block.text = cut(words, TEXT_MAX);
        self.push(block);
    }

    /// An attachment: a message to Claude delivered while Claude was working (`queued_command`).
    /// A subagent's notification Claude Code draws as a line ("● Agent "…" finished · 7m 9s").
    /// A prompt he typed while Claude worked is written only as this, never as a `user` record,
    /// so it starts a turn here, with the questions it names, and its block and mark on the
    /// scroll strip, as a typed prompt does in `user` (Lachlan's 7a of 2026-11-14); another
    /// session's message starts one with none, and no block (lbterm/PITFALLS.md 361)
    fn attachment(&mut self, record: &Value, at_ms: i64) {
        // The files Claude Code put in the context (crate::context): every one at the start and
        // after a compaction, a CLAUDE.md a file read pulled in later, and the files it attached
        // again after a compaction's summary
        match record.get("attachment").and_then(|a| str_of(a, "type").map(|t| (a, t))) {
            Some((a, "instructions")) => return self.context_change(|c| c.instructions(a)),
            Some((_, "file")) => return self.context_change(|c| c.attached(record)),
            Some((a, "nested_memory")) => return self.context_change(|c| c.nested(a)),
            Some((_, "skill_listing")) => return self.context_change(|c| c.skills(record)),
            _ => {}
        }
        let Some(a) = record.get("attachment").filter(|a| str_of(a, "type") == Some("queued_command")) else {
            return;
        };
        let Some(prompt) = a.get("prompt") else {
            return;
        };
        if let Some(summary) = notification(prompt) {
            self.turn(at_ms, None);
            self.note(&summary, at_ms);
            return;
        }
        // His prompt, or another session's message ("commandMode": "prompt", its origin "human"
        // or "peer"). His gets a prompt's block and mark, as a typed one does; another session's
        // turn alone moves the revision, so a snapshot takes it
        let Some(text) = prompt_text(prompt) else {
            return;
        };
        let origin = a.get("origin").and_then(|o| str_of(o, "kind"));
        let his = origin.map_or(true, |o| o == "human");
        self.turn(at_ms, his.then_some(text.as_str()));
        if his {
            self.prompt(&text, at_ms);
        } else {
            self.revision += 1;
        }
    }

    fn system(&mut self, record: &Value, at_ms: i64) {
        match str_of(record, "subtype") {
            Some("compact_boundary") => {
                // What it had loaded goes with what came before: the instructions record after
                // the boundary loads the start's files again
                let post = record.pointer("/compactMetadata/postTokens").and_then(Value::as_u64).unwrap_or(0);
                self.context_change(|c| c.compacted(post));
                let i = self.push(Block::new(BlockKind::Compaction, at_ms));
                self.compacted_at = Some(i);
                self.mark(Kind::Compaction, i, at_ms, "Conversation compacted".into());
            }
            Some("turn_duration") => {
                // The turn's last words ask something: a question waiting on you
                if let Some(t) = self.last_text.take() {
                    if let Some(asked) = asks(&self.blocks[t].text) {
                        let when = self.blocks[t].at_ms;
                        self.mark(Kind::Question, t, when, asked);
                    }
                }
                let ms = record.get("durationMs").and_then(Value::as_i64).unwrap_or(0);
                let i = self.push(Block::new(BlockKind::TurnEnd, at_ms));
                self.mark(Kind::TurnEnd, i, at_ms, format!("Worked for {}", duration(ms)));
            }
            _ => {}
        }
    }

    /// The permission prompts claudehook recorded for this session, `<sid>.events.jsonl`: one
    /// record a line, `{"at": <ms>, "event": "PermissionRequest", "tool": "Bash"}`. Replaces
    /// what was read before.
    pub fn permissions(&mut self, text: &str) {
        let permissions: Vec<(i64, String)> = text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|r| str_of(r, "event") == Some("PermissionRequest"))
            .filter_map(|r| Some((r.get("at")?.as_i64()?, str_of(&r, "tool")?.to_string())))
            .collect();
        if permissions == self.permissions {
            return;
        }
        for was in std::mem::take(&mut self.matched) {
            self.ask(was, None);
        }
        let matched: Vec<Option<usize>> = permissions.iter().map(|(at, tool)| self.call_asked(*at, tool)).collect();
        for &now in &matched {
            self.ask(None, now);
        }
        (self.permissions, self.matched) = (permissions, matched);
    }

    /// The call a permission prompt at `at` for `tool` was for: written before the prompt showed,
    /// so the latest of that tool's before it
    fn call_asked(&self, at: i64, tool: &str) -> Option<usize> {
        self.tools.iter().rposition(|t| t.name == tool && t.at_ms <= at + 1000)
    }

    /// Every mark, in the order Claude Code draws them: those read from the transcript, each
    /// tool call's (its own kind, failed, turned down), and each permission prompt's, on the
    /// latest call of that tool before it. Worked out from nothing; a [`Snapshot`]'s are the same,
    /// kept a piece at a time as the transcript grows.
    pub fn events(&self) -> Vec<Event> {
        let mut out: Vec<Event> = self.marks.iter().flatten().cloned().collect();
        let mut asked: Vec<bool> = vec![false; self.tools.len()];
        for (at, tool) in &self.permissions {
            if let Some(i) = self.call_asked(*at, tool) {
                asked[i] = true;
            }
        }
        for (i, t) in self.tools.iter().enumerate() {
            tool_events(t, asked[i], &mut out);
        }
        out.sort_by_key(|e| (e.block, e.at_ms));
        out
    }

    /// The marks on piece `c` of the blocks, in order: the same as [`Conversation::events`]'s there
    fn piece_events(&self, c: usize) -> Vec<Event> {
        let (from, to) = (c * CHUNK, (c + 1) * CHUNK);
        let mut out = self.marks.get(c).cloned().unwrap_or_default();
        let first = self.tools.partition_point(|t| t.block < from);
        let last = self.tools.partition_point(|t| t.block < to);
        for i in first..last {
            tool_events(&self.tools[i], self.asked[i] > 0, &mut out);
        }
        out.sort_by_key(|e| (e.block, e.at_ms));
        out
    }

    /// What the strip needs of it, cheap to share: its blocks, every mark, the last compaction,
    /// and where to find each block and line by words on screen. Only the pieces of them that
    /// changed since the last snapshot are made again ([`CHUNK`]); the rest are shared with it.
    pub fn snapshot(&mut self) -> Snapshot {
        let pieces = self.blocks.pieces();
        let from = (self.dirty / CHUNK).min(pieces).min(self.events_pieces.len());
        self.events_pieces.truncate(from);
        self.found_pieces.truncate(from);
        for c in from..pieces {
            self.events_pieces.push(Arc::new(self.piece_events(c)));
            self.found_pieces.push(Arc::new(pieces::Found::of(self.blocks.piece(c), c * CHUNK)));
        }
        self.dirty = usize::MAX;
        Snapshot {
            blocks: self.blocks.clone(),
            events: Events::new(self.events_pieces.clone()),
            compacted_at: self.compacted_at,
            context: Arc::clone(&self.context),
            found: self.found_pieces.clone(),
            turns: self.turns.clone(),
            revision: self.revision,
        }
    }
}

/// A tool call's marks: turned down, else asked about, failed and its own kind
fn tool_events(t: &ToolUse, asked: bool, out: &mut Vec<Event>) {
    if t.rejected {
        let what = format!("{}: {}", t.name, t.what);
        out.push(Event { kind: Kind::Denied, block: t.block, at_ms: t.at_ms, text: cut(&what, TEXT_MAX) });
        return;
    }
    if asked {
        let what = format!("{}: {}", t.name, t.what);
        out.push(Event { kind: Kind::Permission, block: t.block, at_ms: t.at_ms, text: cut(&what, TEXT_MAX) });
    }
    if let Some(err) = &t.error {
        let what = format!("{}: {}", t.name, err);
        out.push(Event { kind: Kind::Error, block: t.block, at_ms: t.at_ms, text: cut(&what, TEXT_MAX) });
    }
    match t.kind {
        // A commit that failed isn't one
        Some(Kind::Commit) if t.error.is_some() => {}
        // A picker put to you, an edit, a subagent...
        Some(kind) => out.push(Event { kind, block: t.block, at_ms: t.at_ms, text: t.what.clone() }),
        None => {}
    }
}

/// A conversation as the strip draws it: what Claude Code draws, and the marks on it, in pieces
/// shared with the snapshots before it ([`CHUNK`]).
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub blocks: Chunks<Arc<Block>>,
    pub events: Events,
    /// The block the last compaction is: Claude Code can't scroll above it
    pub compacted_at: Option<usize>,
    /// The files in its context now (crate::context), shared with the conversation
    pub context: Arc<crate::context::Context>,
    /// Where to find each block and line by the words on a screen row, a piece of the blocks each
    found: Vec<Arc<pieces::Found>>,
    /// Each turn a message to Claude started ([`Conversation::turns`]), shared with it
    pub turns: Chunks<Turn>,
    /// The conversation's [`Conversation::revision`] when it was taken
    pub revision: u64,
}

/// Two snapshots of the same conversation, whenever taken
impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.blocks == other.blocks
            && self.events == other.events
            && self.compacted_at == other.compacted_at
            && self.context == other.context
            && self.found.len() == other.found.len()
            && self.found.iter().zip(&other.found).all(|(a, b)| a == b)
            && self.turns == other.turns
    }
}

/// How much of a line's start is kept to find it on screen, and the least that tells it apart
const LINE_KEY: usize = 24;
const LINE_MIN: usize = 6;

/// A line's key: its start as [`bare`] has it, or nothing for one too short to tell apart
fn line_key(line: &str) -> String {
    let key: String = bare(line).chars().take(LINE_KEY).collect();
    if key.chars().count() < LINE_MIN {
        String::new()
    } else {
        key
    }
}

impl Snapshot {
    /// Where each block starts, in rows from the top, at `cols` wide
    pub fn layout(&self, cols: usize) -> Layout {
        self.layout_from(&Layout { cols, ..Layout::default() }, 0)
    }

    /// The layout `was` laid out again from block `from` on, at its width: `was` is this
    /// conversation's layout at an earlier snapshot, the same as this one before block `from`
    /// ([`Chunks::first_difference`]), so an append lays out only what it added
    pub fn layout_from(&self, was: &Layout, from: usize) -> Layout {
        let cols = was.cols;
        let from = from.min(was.tops.len()).min(self.blocks.len());
        let mut tops = Vec::with_capacity(self.blocks.len());
        tops.extend_from_slice(&was.tops[..from]);
        let mut row = match from.checked_sub(1) {
            Some(last) => was.tops[last] + self.blocks[last].rows(cols),
            None => BANNER_ROWS,
        };
        for b in self.blocks.iter_from(from) {
            tops.push(row);
            row += b.rows(cols);
        }
        let reachable = self.compacted_at.map_or(0, |i| tops[i]);
        Layout { tops, total: row, reachable, cols }
    }
}

/// "2m 36s", "45s", "1h 3m"
pub fn duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

/// Where each block of a conversation is, in rows, at one width
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// Each block's first row (the blank line before it)
    pub tops: Vec<usize>,
    /// Every row
    pub total: usize,
    /// The first row Claude Code can scroll to: the last compaction's, else the top
    pub reachable: usize,
    /// The width it's laid out at
    pub cols: usize,
}

impl Layout {
    /// The block a row is in
    pub fn block_at(&self, row: usize) -> Option<usize> {
        match self.tops.binary_search(&row) {
            Ok(i) => Some(i),
            Err(0) => None,
            Err(i) => Some(i - 1),
        }
    }
}

/// Where Claude Code's view is, in the layout's rows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// The row at the top of the screen
    pub top: usize,
    /// How many rows the conversation shows: its area of the screen, above the input box
    pub rows: usize,
}

/// What a screen of Claude Code's fullscreen view says of the conversation's part of it,
/// measured on Claude Code 2.1.312 (lbterm/SCROLL-STRIP.md, "What Claude Code draws")
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    /// How many rows at the top are the conversation's: down to the row above the input box's
    /// upper rule, which is Claude Code's own (its effort and its spinner). PgUp and PgDn scroll
    /// half of it
    pub area: usize,
    /// Scrolled up from the latest: "Jump to bottom (ctrl+End) ↓" (or "1 new message") on the
    /// area's last row ([`JUMP`])
    pub scrolled: bool,
    /// Claude Code's sticky header on the first row, naming the prompt above the view: only
    /// while scrolled up, on the band in dim words
    pub header: bool,
}

/// How far apart two rows' views can be and still agree: the layout's rows can be a few out
/// inside a long block, where Claude Code's wrap and the layout's part
const AGREE: usize = 6;

/// What Claude Code says on the area's last row while scrolled up: "Jump to bottom (ctrl+End) ↓",
/// or once a turn has ended meanwhile, "1 new message (ctrl+End) ↓" (2.1.314), which a screen
/// read by "Jump to bottom" alone took for the latest
const JUMP: &str = "(ctrl+End)";
/// The glyphs Claude Code's spinner turns through while it works (2.1.314 on Windows)
const SPINNER: [char; 7] = ['\u{b7}', '\u{2722}', '\u{2733}', '\u{2736}', '\u{273b}', '\u{273d}', '*'];

/// Whether a row is Claude Code's spinner while it works: one of its glyphs at the row's start, a
/// space, and a word ending in an ellipsis ("✻ Infusing…"), maybe more after it. It turns several
/// times a second, its glyph and its word's shimmer, at the latest only, under the conversation
/// with a tip under it, all in the area (2.1.314); a turn's end in its place ("✻ Crunched for 0s ·
/// done 8:03") has no ellipsis
pub fn is_spinner(text: &str) -> bool {
    let mut chars = text.chars();
    if !chars.next().is_some_and(|c| SPINNER.contains(&c)) {
        return false;
    }
    let Some(rest) = chars.as_str().strip_prefix(' ') else {
        return false;
    };
    let word = rest.split_whitespace().next().unwrap_or("");
    word.chars().next().is_some_and(char::is_alphabetic) && word.ends_with('\u{2026}')
}
/// The most rows the input box takes between its rules: a long draft grows it
const INPUT_MAX: usize = 24;

/// Whether a row is one of the rules above and below Claude Code's input box, the upper one of a
/// named conversation with its name in it (questions.rs's [`crate::questions::is_rule_text`])
pub fn is_rule(row: &Row) -> bool {
    crate::questions::is_rule_text(&row.text)
}

/// A row that's Claude Code's sticky header: the mark, on the band in dim words (off the band
/// before 2.1.312)
fn is_header(row: &Row) -> bool {
    row.text.trim_start().starts_with(prompts::MARK) && (row.dim || !row.band)
}

impl Screen {
    /// What `rows` (the pane's screen, top to bottom) say; `None` without the input box's two
    /// rules near the bottom, when nothing on screen says where the conversation ends (a
    /// permission dialog or a picker in its place)
    pub fn read(rows: &[Row]) -> Option<Self> {
        let (upper, _) = Self::rules(rows)?;
        let area = upper.checked_sub(1).filter(|&a| a > 0)?;
        let scrolled = rows[area - 1].text.contains(JUMP);
        let header = scrolled && is_header(&rows[0]);
        Some(Self { area, scrolled, header })
    }

    /// The rows of the input box's two rules, upper and lower, near the bottom of `rows`: where
    /// the conversation ends is the row above the upper's, less Claude Code's own line there
    pub fn rules(rows: &[Row]) -> Option<(usize, usize)> {
        let lower = rows.iter().rposition(is_rule)?;
        let upper = rows[..lower].iter().rposition(is_rule).filter(|&u| lower - u <= INPUT_MAX + 1)?;
        Some((upper, lower))
    }

    /// Half the area: what a PgUp or PgDn scrolls
    pub fn half(&self) -> usize {
        (self.area / 2).max(1)
    }

    /// The rows a block can be found on: the area, less the header's row
    fn body(&self) -> std::ops::Range<usize> {
        usize::from(self.header)..self.area
    }
}

/// What's on a screen row, as the conversation's anchors go
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor<'a> {
    /// A message you sent, on the band in bright words
    Prompt(&'a str),
    /// Claude's words after "● "
    Text(&'a str),
}

const DOT: char = '\u{25cf}';

fn anchor(row: &Row) -> Option<Anchor<'_>> {
    let t = row.text.trim_start();
    if row.band {
        return if row.dim { None } else { t.strip_prefix(prompts::MARK).map(|s| Anchor::Prompt(s.trim_start())) };
    }
    t.strip_prefix(DOT).map(|s| Anchor::Text(s.trim_start()))
}

impl Snapshot {
    /// The blocks found by `by` that `keep` says are it, in order
    fn found_blocks(&self, by: u64, keep: impl Fn(&Block) -> bool) -> Vec<usize> {
        let mut out: Vec<usize> = vec![];
        for piece in &self.found {
            for (b, _) in piece.get(by) {
                let b = b as usize;
                if out.last() != Some(&b) && self.blocks.get(b).is_some_and(|block| keep(block)) {
                    out.push(b);
                }
            }
        }
        out
    }

    /// The prompts a row after "❯ " shows, `shown` its words on one line ([`prompt_matches`]):
    /// whole, the prompt whose key is the row's first [`prompts::KEY_CHARS`]; cut short with "…",
    /// any whose key starts with what's shown, so shares its first [`PROMPT_HEAD`]
    fn prompts_shown(&self, shown: &str) -> Vec<usize> {
        let cut = shown.trim_end().ends_with('\u{2026}');
        let words = shown.trim_end_matches(['\u{2026}', ' ']);
        let by = if cut && words.chars().count() >= PROMPT_HEAD {
            hash(By::PromptHead, head(words, PROMPT_HEAD))
        } else {
            hash(By::Prompt, head(words, prompts::KEY_CHARS))
        };
        self.found_blocks(by, |b| b.kind == BlockKind::Prompt && b.key.as_deref().is_some_and(|k| prompt_matches(shown, k)))
    }

    /// The prompts Claude Code's sticky header, `shown` after "❯ ", names ([`header_names`]): by
    /// its key, or by all its words, whole or the start of them
    fn prompts_named(&self, shown: &str) -> Vec<usize> {
        let cut = shown.trim_end().ends_with('\u{2026}');
        let words = shown.trim_end_matches(['\u{2026}', ' ']);
        let mut with = self.prompts_shown(shown);
        let by = match (cut, words.chars().count() >= PROMPT_HEAD) {
            (true, true) => Some(hash(By::WholeHead, head(words, PROMPT_HEAD))),
            (true, false) => None,
            (false, _) => Some(hash(By::Whole, words)),
        };
        if let Some(by) = by {
            with.extend(self.found_blocks(by, |b| {
                b.kind == BlockKind::Prompt && b.key.as_deref().is_some_and(|k| header_names(shown, k, &b.text))
            }));
            with.sort_unstable();
            with.dedup();
        }
        with
    }

    /// The blocks a screen row could be the first row of, best first: its anchor matched to
    /// every block with that key. Claude's words are found by their key's first [`TEXT_HEAD`]
    /// letters (a row as short as that or longer must show them, [`matches_text`]), or all of it
    fn candidates(&self, row: &Row) -> Vec<usize> {
        match anchor(row) {
            Some(Anchor::Prompt(text)) => self.prompts_shown(&prompts::one_line(text)),
            Some(Anchor::Text(text)) => {
                let words = bare(text);
                let by = hash(By::Text, head(&words, TEXT_HEAD));
                self.found_blocks(by, |b| {
                    b.kind == BlockKind::Text && b.key.as_deref().is_some_and(|k| shows_key(&words, k, TEXT_KEY, MIN_MATCH))
                })
            }
            None => vec![],
        }
    }

    /// Every place a screen row could be in the layout: a block's first row when it's one's
    /// anchor, else any line of a message whose start it shows ([`Snapshot::lines`]). Each is
    /// (block, the row from the block's top).
    fn places(&self, layout: &Layout, row: &Row) -> Vec<(usize, usize)> {
        let firsts = self.candidates(row);
        if !firsts.is_empty() {
            return firsts.into_iter().map(|b| (b, 1)).collect();
        }
        if row.band {
            return vec![];
        }
        let text = bare(&row.text);
        if text.chars().count() < LINE_MIN {
            return vec![];
        }
        // A line's key matches a row exactly when it's the row's first LINE_KEY letters (a
        // shorter key is a whole line, [`shows_key`]), so it's found by them
        let by = hash(By::Line, head(&text, LINE_KEY));
        let mut out = vec![];
        for piece in &self.found {
            for (b, k) in piece.get(by) {
                let (b, k) = (b as usize, k as usize);
                let block = &self.blocks[b];
                if block.line_keys.get(k).is_some_and(|key| shows_key(&text, key, LINE_KEY, LINE_KEY)) {
                    out.push((b, block.row_of_line(k, layout.cols)));
                }
            }
        }
        out
    }

    /// The prompt Claude Code's sticky header names, nearest `near` (the view last found), else
    /// the latest with those words
    fn header_prompt(&self, layout: &Layout, row: &Row, near: Option<usize>) -> Option<usize> {
        let text = prompts::one_line(row.text.trim_start().strip_prefix(prompts::MARK)?);
        let with = self.prompts_named(&text);
        match near {
            Some(n) => with.into_iter().min_by_key(|&b| layout.tops[b].abs_diff(n)),
            None => with.last().copied(),
        }
    }

    /// What each row of the conversation's area places: (screen row, block, the view's top it
    /// implies). A row can be in several places (the same lines of a file in two diffs, "yes"
    /// sent twice): the view is where the most rows agree it is, give or take [`AGREE`] rows,
    /// and each row takes its place nearest that. Between places as many rows agree on, the one
    /// nearest `near` (the view last found), else nearest the start of the prompt the header
    /// names, else the latest. The header's prompt rules out every block above it.
    fn placed(&self, layout: &Layout, rows: &[Row], screen: &Screen, near: Option<usize>) -> Vec<(usize, usize, usize)> {
        let part = if screen.header { self.header_prompt(layout, &rows[0], near) } else { None };
        let reference = near.or(part.map(|p| layout.tops[p] + 1));
        let mut all: Vec<(usize, usize, usize)> = vec![];
        for i in screen.body() {
            for (b, at) in self.places(layout, &rows[i]) {
                if part.is_some_and(|p| b < p) {
                    continue;
                }
                all.push((i, b, (layout.tops[b] + at).saturating_sub(i)));
            }
        }
        // How many rows have a place near a top, and how near they are all told: the more rows,
        // then the closer they agree
        let agree = |top: usize| {
            let (mut rows, mut off) = (0usize, 0usize);
            for row in all.chunk_by(|a, b| a.0 == b.0) {
                let d = row.iter().map(|c| c.2.abs_diff(top)).min().unwrap_or(usize::MAX);
                if d <= AGREE {
                    rows += 1;
                    off += d;
                }
            }
            (rows, std::cmp::Reverse(off))
        };
        let better = |a: usize, b: usize| match reference {
            Some(r) => b.abs_diff(r).cmp(&a.abs_diff(r)),
            None => a.cmp(&b),
        };
        let Some(view) = all
            .iter()
            .map(|c| c.2)
            .max_by(|&a, &b| agree(a).cmp(&agree(b)).then_with(|| better(a, b)))
        else {
            return vec![];
        };
        let mut out: Vec<(usize, usize, usize)> = vec![];
        for c in all {
            match out.last_mut() {
                Some(last) if last.0 == c.0 => {
                    if c.2.abs_diff(view) < last.2.abs_diff(view) {
                        *last = c;
                    }
                }
                _ => out.push(c),
            }
        }
        out
    }

    /// The first and last block that `rows` show (the header's prompt counting as the first):
    /// what a seek goes by, since the order of the blocks can't be wrong where the layout's
    /// rows can
    fn shown_blocks(&self, layout: &Layout, rows: &[Row], screen: &Screen, near: Option<usize>) -> Option<(usize, usize)> {
        let mut found: Vec<usize> = self.placed(layout, rows, screen, near).into_iter().map(|(_, b, _)| b).collect();
        if screen.header {
            found.extend(self.header_prompt(layout, &rows[0], near));
        }
        Some((*found.iter().min()?, *found.iter().max()?))
    }

    /// Where Claude Code's view is, from the screen's `rows` (top to bottom, the whole pane).
    /// Not scrolled up, it's at the latest. Scrolled up, every row of the conversation that's a
    /// line of a message says where the view's top would be; the middle of what they say is
    /// where it is (one row matched by chance can't move it). With none, the sticky header names
    /// the prompt the view is under, and it stays near `near` within that prompt's part. `None`
    /// when the screen isn't Claude Code's view or nothing on it says.
    pub fn locate(&self, layout: &Layout, rows: &[Row], near: Option<usize>) -> Option<View> {
        let screen = Screen::read(rows)?;
        let shows = screen.area;
        let fit = |top: usize| top.min(layout.total.saturating_sub(shows));
        let placed = self.placed(layout, rows, &screen, near);
        if !screen.scrolled {
            // At the latest, unless what's on screen is none of the last screen's: Claude Code has
            // been seen to leave "Jump to bottom" off a screen scrolled halfway up, for seconds
            // (2.1.312, after a burst of 120 PgDns). Which blocks are near the end the layout
            // gets right however far out its rows are.
            let last = placed.iter().map(|&(_, b, _)| b).max();
            let ends = |b: usize| layout.tops.get(b + 1).copied().unwrap_or(layout.total);
            let away = last.is_some_and(|b| ends(b) + 2 * shows < layout.total);
            if !(away && placed.len() >= 3) {
                return Some(View { top: fit(layout.total), rows: shows });
            }
        }
        let mut tops: Vec<usize> = placed.into_iter().map(|(_, _, t)| t).collect();
        if !tops.is_empty() {
            tops.sort_unstable();
            return Some(View { top: fit(tops[tops.len() / 2]), rows: shows });
        }
        if !screen.header {
            return None;
        }
        let b = self.header_prompt(layout, &rows[0], near)?;
        let from = layout.tops[b] + 1;
        let to = self
            .blocks
            .iter_from(b + 1)
            .position(|x| x.kind == BlockKind::Prompt)
            .map_or(layout.total, |n| layout.tops[b + 1 + n]);
        let top = near.unwrap_or(from).clamp(from, to.saturating_sub(1).max(from));
        Some(View { top: fit(top), rows: shows })
    }
}

// A key a seek presses, which Claude Code's fullscreen view scrolls by: PgUp and PgDn half the
/// conversation's area, Ctrl+Home to the top and Ctrl+End to the latest
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    PageUp,
    PageDown,
    Top,
    Bottom,
}

/// What a seek does next
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// There, on screen at this row
    Found(usize),
    /// Press this key this many times, all at once (Claude Code takes a burst in one go, within
    /// about 150 ms), then look again
    Press(Key, u32),
    /// At an end and it wasn't there: before a compaction, or never drawn; or the screen isn't
    /// Claude Code's view (a dialog over it)
    NotThere,
    /// [`MAX_ROUNDS`] bursts and still not there
    TooFar,
}

/// The most bursts a seek sends before it gives up
pub const MAX_ROUNDS: u32 = 12;

/// One seek: scrolling Claude Code until block `target` is on screen, a burst of keys at a time.
/// Each look works out where the view is and how far the target is, in pages, and sends that
/// many PgUps or PgDns at once (or Ctrl+Home or Ctrl+End first, when an end is nearer); the
/// blocks on screen say which way, should the layout's rows be off. Once it's on screen below
/// the middle, one PgDn more brings it up the screen.
#[derive(Debug, Clone)]
pub struct Seek {
    target: usize,
    /// The line of it that's looked for (a prompt's is its row on the band)
    line: usize,
    prompt: bool,
    key: String,
    rounds: u32,
    /// The screen before the last burst, to tell an end that didn't move
    last: Option<Vec<Row>>,
    /// The last burst's way: up or down
    went_up: Option<bool>,
    /// Brought up the screen already, and put back once when that took it off the top
    raised: bool,
    backed: bool,
}

impl Seek {
    /// A seek to line `line` of block `target` of `conversation`; `None` for a block a seek can't
    /// find on screen (one without words of its own). [`Snapshot::seek_point`] and
    /// [`Snapshot::point_at`] pick one.
    pub fn new(conversation: &Snapshot, target: usize, line: usize) -> Option<Self> {
        let block = conversation.blocks.get(target)?;
        Some(Self {
            target,
            line: if block.kind == BlockKind::Prompt { 0 } else { line.min(block.lines.len().saturating_sub(1)) },
            prompt: block.kind == BlockKind::Prompt,
            key: block.key.clone()?,
            rounds: 0,
            last: None,
            went_up: None,
            raised: false,
            backed: false,
        })
    }

    /// The block and line it's going to
    pub fn target(&self) -> (usize, usize) {
        (self.target, self.line)
    }

    /// The row its line is on, from its block's top
    fn at(&self, conversation: &Snapshot, layout: &Layout) -> usize {
        if self.prompt {
            return 1;
        }
        conversation.blocks.get(self.target).map_or(1, |b| b.row_of_line(self.line, layout.cols))
    }

    pub fn rounds(&self) -> u32 {
        self.rounds
    }

    /// The row its line is on, within the conversation's area of `rows`, if it's there: a
    /// prompt's row on the band; a line of Claude's words after "● " or as a line of it
    pub fn on_screen(&self, conversation: &Snapshot, layout: &Layout, rows: &[Row]) -> Option<usize> {
        let screen = Screen::read(rows)?;
        let body = screen.body();
        if self.prompt {
            let whole = conversation.blocks.get(self.target).map_or("", |b| b.text.as_str());
            return match find_prompt(&rows[..screen.area], &self.key, whole) {
                Some(Found::Row(r)) if body.contains(&r) => Some(r),
                _ => None,
            };
        }
        let at = self.at(conversation, layout);
        body.clone().find(|&i| conversation.places(layout, &rows[i]).contains(&(self.target, at)))
    }

    fn press(&mut self, rows: &[Row], key: Key, n: u32) -> Step {
        self.rounds += 1;
        self.last = Some(rows.to_vec());
        self.went_up = match key {
            Key::PageUp | Key::Top => Some(true),
            Key::PageDown | Key::Bottom => Some(false),
        };
        Step::Press(key, n.max(1))
    }

    /// Once the screen has settled after the last burst (or before the first): what to do next.
    /// `near` is where the view was last found.
    pub fn look(&mut self, conversation: &Snapshot, layout: &Layout, rows: &[Row], near: Option<usize>) -> Step {
        let Some(screen) = Screen::read(rows) else {
            return Step::NotThere;
        };
        if let Some(r) = self.on_screen(conversation, layout, rows) {
            // Below the middle: one PgDn more brings it up, unless it's at the latest already
            if !self.raised && screen.scrolled && r >= screen.half() + usize::from(screen.header) {
                self.raised = true;
                return self.press(rows, Key::PageDown, 1);
            }
            return Step::Found(r);
        }
        if self.raised && !self.backed {
            // It went off the top after all: back
            self.backed = true;
            return self.press(rows, Key::PageUp, 1);
        }
        let moved = self.last.as_deref() != Some(rows);
        if !moved && self.went_up == Some(true) {
            // Pressed up and nothing moved: the top, or the last compaction, and it's not there
            return Step::NotThere;
        }
        if self.rounds >= MAX_ROUNDS {
            return Step::TooFar;
        }
        let half = screen.half();
        let shows = screen.area;
        let Some(view) = conversation.locate(layout, rows, near) else {
            // Nothing on screen to go by: from the latest
            return self.press(rows, Key::Bottom, 1);
        };
        let Some(&target_top) = layout.tops.get(self.target) else {
            return Step::NotThere;
        };
        let last_top = layout.total.saturating_sub(shows);
        // Where the target would sit a quarter of the way down the screen
        let want = (target_top + self.at(conversation, layout)).saturating_sub(shows / 4).min(last_top);
        // Which way, by the blocks on screen; by the rows where none says
        let up = match conversation.shown_blocks(layout, rows, &screen, near) {
            Some((lo, _)) if self.target < lo => true,
            Some((_, hi)) if self.target > hi => false,
            _ => want < view.top,
        };
        if !moved && self.went_up == Some(false) && !up {
            // Pressed down and nothing moved: the latest, and it's not there
            return Step::NotThere;
        }
        let pages = |rows: usize| ((rows + half / 2) / half).max(1) as u32;
        if up {
            let from_here = pages(view.top.saturating_sub(want));
            // From the top: Ctrl+Home, then down from there next time
            if from_here > 1 && (want < half || 1 + pages(want) < from_here) {
                return self.press(rows, Key::Top, 1);
            }
            self.press(rows, Key::PageUp, from_here)
        } else {
            let from_here = pages(want.saturating_sub(view.top));
            let from_end = pages(last_top.saturating_sub(want));
            // From the latest: Ctrl+End, then up from there next time
            if from_here > 1 && (want + half >= last_top || 1 + from_end < from_here) {
                return self.press(rows, Key::Bottom, 1);
            }
            self.press(rows, Key::PageDown, from_here)
        }
    }
}

impl Snapshot {
    /// The block a seek for block `block` looks for: itself when it has words of its own, else
    /// the nearest before it that has (a run of tool calls is found by the words above it)
    pub fn anchor_for(&self, block: usize) -> Option<usize> {
        (0..=block.min(self.blocks.len().checked_sub(1)?)).rev().find(|&i| self.blocks[i].key.is_some())
    }

    /// Whether block `block` is above the last compaction, where Claude Code can't scroll
    pub fn before_compaction(&self, block: usize) -> bool {
        self.compacted_at.is_some_and(|c| block < c)
    }

    /// The block a seek for row `row` of `layout` looks for: the one with words at or above it,
    /// and no higher than the last compaction (the first with words after it, for a row above)
    pub fn anchor_at(&self, layout: &Layout, row: usize) -> Option<usize> {
        let block = layout.block_at(row).unwrap_or(0);
        match self.compacted_at {
            Some(c) if block <= c => (c..self.blocks.len()).find(|&i| self.blocks[i].key.is_some()),
            _ => self.anchor_for(block).filter(|&a| !self.before_compaction(a)).or_else(|| {
                let from = self.compacted_at.unwrap_or(0);
                (from..self.blocks.len()).find(|&i| self.blocks[i].key.is_some())
            }),
        }
    }
}

/// A block's last line it can be found by on screen at `cols` wide
fn last_keyed(b: &Block, cols: usize) -> usize {
    (0..b.lines.len()).rev().find(|&k| b.finds_line(k, cols)).unwrap_or(b.key_line)
}

impl Snapshot {
    /// Where a seek goes for a mark on block `block`, at `cols` wide: (block, line), a line with
    /// words that's on screen with it. A block with words of its own, its first line; one
    /// without (a run of tool calls, a turn's end), the last line of the words before it, which
    /// Claude Code draws just above it, where a long answer's first line could be screens away.
    /// A prompt is found by its row on the band.
    pub fn seek_point(&self, block: usize, cols: usize) -> Option<(usize, usize)> {
        let at = self.anchor_for(block)?;
        let b = &self.blocks[at];
        if b.kind == BlockKind::Prompt {
            return Some((at, 0));
        }
        Some((at, if at == block { b.key_line } else { last_keyed(b, cols) }))
    }

    /// Where a seek goes for row `row` of `layout`, a point on the track: the line with words
    /// at or above it, no higher than the last compaction
    pub fn point_at(&self, layout: &Layout, row: usize) -> Option<(usize, usize)> {
        let at = self.anchor_at(layout, row)?;
        let b = &self.blocks[at];
        if b.kind == BlockKind::Prompt {
            return Some((at, 0));
        }
        let into = row.saturating_sub(layout.tops[at]).max(1);
        let line = (0..b.line_keys.len())
            .rev()
            .find(|&k| b.finds_line(k, layout.cols) && b.row_of_line(k, layout.cols) <= into)
            .unwrap_or(b.key_line);
        Some((at, line))
    }
}

/// Where a block's mark goes on a track `height` pixels tall that stands for every row of the
/// layout: its first row's y from the track's top
pub fn mark_y(layout: &Layout, block: usize, height: f64) -> Option<f64> {
    let row = *layout.tops.get(block)?;
    (layout.total > 0).then(|| row as f64 / layout.total as f64 * height)
}

/// Where the thumb goes on that track for `view`: its top and height, at least `min` tall
pub fn thumb(layout: &Layout, view: View, height: f64, min: f64) -> (f64, f64) {
    if layout.total == 0 {
        return (0.0, height);
    }
    let h = (view.rows as f64 / layout.total as f64 * height).clamp(min.min(height), height);
    let room = height - h;
    let span = layout.total.saturating_sub(view.rows).max(1) as f64;
    let y = (view.top as f64 / span).clamp(0.0, 1.0) * room;
    (y, h)
}

/// The row a point `y` down the track stands for: the inverse of [`thumb`] for the thumb's top
pub fn row_at(layout: &Layout, view_rows: usize, y: f64, height: f64, thumb_h: f64) -> usize {
    let room = (height - thumb_h).max(1.0);
    let span = layout.total.saturating_sub(view_rows) as f64;
    ((y / room).clamp(0.0, 1.0) * span).round() as usize
}

/// One mark on the card resting on the strip shows: its colour, its head ("You · 14:02"), its
/// words and how many lines of them it may take
#[derive(Debug, Clone, PartialEq)]
pub struct CardItem {
    pub colour: u32,
    pub head: String,
    pub text: String,
    pub lines: usize,
}

/// How a line on the card is drawn
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CardTone {
    /// A mark's head, in its colour
    Head(u32),
    /// Its words
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub tone: CardTone,
}

/// The card, laid out: where it is, each line, and each mark's part of it (a click there goes to
/// that mark)
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub rect: crate::tabs::Rect,
    pub lines: Vec<CardLine>,
    pub items: Vec<crate::tabs::Rect>,
}

/// The card's padding and the gap between its marks, at 96 DPI
const CARD_PAD: f64 = 10.0;
const CARD_GAP: f64 = 6.0;

/// The card for `items`, its right edge at `right`, `width` wide, level with `at` (the marks'
/// y) and kept between `ceiling` and `bottom`; `cell` is a column's width and a line's height in
/// the card's font. `None` with nothing to show or no room.
pub fn card(
    items: &[CardItem],
    right: f64,
    (at, ceiling, bottom): (f64, f64, f64),
    width: f64,
    cell: (f64, f64),
    scale: f64,
) -> Option<Card> {
    if items.is_empty() || cell.0 <= 0.0 || width < 12.0 * cell.0 {
        return None;
    }
    let pad = CARD_PAD * scale;
    let gap = CARD_GAP * scale;
    let x = (right - width).round();
    let columns = (((width - 2.0 * pad) / cell.0).floor() as usize).max(8);
    let mut lines = vec![];
    let mut parts = vec![];
    let mut y = pad;
    for item in items {
        let start = y;
        lines.push(CardLine { x: x + pad, y, text: crate::strip::elide(&item.head, columns), tone: CardTone::Head(item.colour) });
        y += cell.1;
        let mut words = crate::strip::wrap(&item.text, columns);
        words.retain(|w| !w.is_empty());
        if words.len() > item.lines {
            words.truncate(item.lines.max(1));
            if let Some(last) = words.last_mut() {
                *last = crate::strip::elide(&format!("{last}\u{2026}"), columns);
            }
        }
        for w in words {
            lines.push(CardLine { x: x + pad, y, text: w, tone: CardTone::Text });
            y += cell.1;
        }
        parts.push((start, y));
        y += gap;
    }
    let h = (y - gap + pad).ceil();
    let top = (at - pad - cell.1 / 2.0).min(bottom - h).max(ceiling).floor();
    for line in &mut lines {
        line.y = (line.y + top).round();
    }
    let items = parts
        .into_iter()
        .map(|(a, b)| crate::tabs::Rect { x, y: top + a - gap / 2.0, w: width.ceil(), h: b - a + gap })
        .collect();
    Some(Card { rect: crate::tabs::Rect { x, y: top, w: width.ceil(), h }, lines, items })
}

/// When, for a card's head: the time today, the day this week, the date before that
pub fn when(at_ms: i64, now: chrono::NaiveDateTime) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(at_ms).single() {
        Some(at) => prompts::stamp(at.naive_local(), now),
        None => String::new(),
    }
}

/// Follows a transcript as it grows: each read takes up where the last stopped, and a line cut
/// short waits for the rest.
#[derive(Debug, Default)]
pub struct Follow {
    offset: u64,
    partial: Vec<u8>,
    pub conversation: Conversation,
}

/// The kinds of record the conversation reads, and the attachments it does (a subagent's
/// notification delivered mid-turn, and the files Claude Code put in the context, crate::context):
/// other lines aren't parsed. Inside a JSON string every quote is escaped, so these match only a
/// record's own keys, never quoted text. For the context bar (crate::context): the skills list,
/// and the records of Claude Code's requests, their shapes (a few a transcript, about 50 KB each)
/// and the requests themselves (one a reply, about 1.6 KB); the messages' blobs only until the
/// first request's reply is in ([`BLOB`], [`Conversation::wants_blobs`]), since they're most of a
/// transcript's bytes after that; and a `file` attachment, a file attached again after a
/// compaction's summary (or named in a prompt, which the context leaves to the conversation)
const WANTED: [&[u8]; 10] = [
    br#""type":"user""#,
    br#""type":"assistant""#,
    br#""type":"system""#,
    br#""type":"queued_command""#,
    br#""type":"instructions""#,
    br#""type":"nested_memory""#,
    br#""type":"skill_listing""#,
    br#""type":"file""#,
    br#""type":"api-request-shape""#,
    br#""type":"api-request""#,
];
/// A message blob's record: read only while [`Conversation::wants_blobs`]
const BLOB: &[u8] = br#""type":"api-request-blob""#;
/// The most read at a time: a long transcript is read to its end in pieces of this
const READ_MAX: u64 = 32 * 1024 * 1024;

/// Whether a line holds one of [`WANTED`]: each searched for as memchr does, where a byte at a
/// time took 2 ns a byte of a 46 MB transcript for the first alone (Claude Code writes a record's
/// type after its message)
fn wanted(line: &[u8]) -> bool {
    static FINDERS: std::sync::OnceLock<Vec<memchr::memmem::Finder<'static>>> = std::sync::OnceLock::new();
    FINDERS
        .get_or_init(|| WANTED.iter().map(|w| memchr::memmem::Finder::new(*w)).collect())
        .iter()
        .any(|f| f.find(line).is_some())
}

impl Follow {
    /// Take in more of the transcript, as it was written
    pub fn feed(&mut self, bytes: &[u8]) {
        self.offset += bytes.len() as u64;
        let mut rest = bytes;
        while let Some(end) = memchr::memchr(b'\n', rest) {
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
        if !wanted(line) && !(self.conversation.wants_blobs() && memchr::memmem::find(line, BLOB).is_some()) {
            return;
        }
        if let Ok(record) = serde_json::from_slice::<Value>(line) {
            self.conversation.record(&record);
        }
    }

    /// Read what's been added to the file at `path` since the last read, to its end as it is
    /// now, [`READ_MAX`] at a time; true when anything was. A file that got shorter is read again
    /// from its start.
    pub fn read_more(&mut self, path: &std::path::Path) -> bool {
        use std::io::{Read, Seek as _, SeekFrom};
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            // Gone: nothing more to read, and its watch says when one of its name comes back
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return false,
            Err(err) => {
                log::warn!("conversation: can't open {}: {err:#}", path.display());
                return false;
            }
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // Read again from nothing, its revision going on from where it was, so the snapshot
            // given out last is never taken for the one read again
            let revision = self.conversation.revision;
            *self = Self::default();
            self.conversation.revision = revision + 1;
        }
        if self.offset == 0 {
            self.conversation.scratchpad = scratchpad_of(path);
        }
        let mut bytes = vec![];
        let mut read = false;
        while len > self.offset {
            bytes.clear();
            if file.seek(SeekFrom::Start(self.offset)).is_err() {
                break;
            }
            match file.by_ref().take((len - self.offset).min(READ_MAX)).read_to_end(&mut bytes) {
                Ok(0) => break,
                Err(err) => {
                    log::warn!("conversation: can't read {} past byte {}: {err:#}", path.display(), self.offset);
                    break;
                }
                Ok(_) => {
                    self.feed(&bytes);
                    read = true;
                }
            }
        }
        read
    }

    /// How far into the transcript it's read
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

/// A transcript written is a change or two in a row: they're read together, this long after the
/// first
const SETTLE: std::time::Duration = std::time::Duration::from_millis(15);
/// How many times the thread follows the folders before it reads: a folder made can have the
/// next one under it already
const FOLLOWS: usize = 8;

struct Shared {
    /// Each session's conversation as last read, for the sessions shown and those kept
    known: std::sync::Mutex<HashMap<String, Arc<Snapshot>>>,
    generation: std::sync::atomic::AtomicU64,
    /// Told the session whose conversation changed, and whether its context did (a new
    /// `Snapshot::context`): a window only the context bar reads it for repaints only then
    wake: Box<dyn Fn(&str, bool) + Send + Sync>,
    /// Claude Code's transcripts, one folder per project: `~/.claude/projects`
    projects: std::path::PathBuf,
    /// Where claudehook writes `<sid>.events.jsonl`, the summaries' folder
    summaries: Option<std::path::PathBuf>,
    /// Run on the reading thread as it's about to follow the folders, after it has looked for
    /// the transcripts not found yet: a test writes one there, in the gap a loaded PC can leave
    #[cfg(test)]
    before_follow: Box<dyn Fn() + Send + Sync>,
}

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What wakes the reading thread
enum Msg {
    /// The sessions whose conversations show now, and every session a pane names
    Show { shown: Vec<String>, live: Vec<String> },
    Seen(crate::watch::Seen),
}

/// Each session's conversation, read on a thread of its own while a window's strip shows it:
/// when it's first shown and whenever Windows says its transcript changed (crate::watch), from
/// where the last read stopped, so a read costs what was added. A session not shown any more is
/// kept while a pane names it, and showing it again reads only what was added since. Nothing
/// is read or woken while nothing changes: until 2026-10-30 every paint of the strip asked, the
/// transcript was looked at again each 500 ms while anything painted, a session not asked for in
/// ten minutes was forgotten and read again from its start (32 MB a look), and each snapshot was
/// compared whole with the last under the lock each paint takes.
pub struct Conversations {
    shared: Arc<Shared>,
    tx: std::sync::Mutex<std::sync::mpsc::Sender<Msg>>,
}

impl Conversations {
    /// Start the reading thread for transcripts under `projects`, with claudehook's permission
    /// records in `summaries`; `wake` is called from it with the session whose conversation, shown,
    /// changed, and whether its context did.
    pub fn start(
        projects: std::path::PathBuf,
        summaries: Option<std::path::PathBuf>,
        wake: impl Fn(&str, bool) + Send + Sync + 'static,
    ) -> Self {
        Self::start_shared(Shared {
            known: std::sync::Mutex::new(HashMap::new()),
            generation: std::sync::atomic::AtomicU64::new(0),
            wake: Box::new(wake),
            projects,
            summaries,
            #[cfg(test)]
            before_follow: Box::new(|| {}),
        })
    }

    /// As [`Self::start`], with `before_follow` run each time the reading thread is about to
    /// follow the folders
    #[cfg(test)]
    fn start_probed(
        projects: std::path::PathBuf,
        wake: impl Fn(&str, bool) + Send + Sync + 'static,
        before_follow: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::start_shared(Shared {
            known: std::sync::Mutex::new(HashMap::new()),
            generation: std::sync::atomic::AtomicU64::new(0),
            wake: Box::new(wake),
            projects,
            summaries: None,
            before_follow: Box::new(before_follow),
        })
    }

    fn start_shared(shared: Shared) -> Self {
        let shared = Arc::new(shared);
        let (tx, rx) = std::sync::mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let events = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("lbterm-conversations".into())
            .spawn(move || follow(thread_shared, rx, events));
        if let Err(err) = spawned {
            log::warn!("conversation: no thread to read transcripts on: {err:#}");
        }
        Self {
            shared,
            tx: std::sync::Mutex::new(tx),
        }
    }

    /// What's known of session `sid`'s conversation, from memory: `None` until it's been read,
    /// which it is while shown ([`Self::show`])
    pub fn get(&self, sid: &str) -> Option<Arc<Snapshot>> {
        lock(&self.shared.known).get(sid).cloned()
    }

    /// The sessions whose conversations the windows' strips show now (`shown`) and every session
    /// a pane names (`live`): each shown is read as its transcript grows; one not shown keeps what
    /// was read of it while it's live, and any other is let go
    pub fn show(&self, shown: Vec<String>, live: Vec<String>) {
        let _ = lock(&self.tx).send(Msg::Show { shown, live });
    }

    /// Bumped each time a conversation shown changes
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Session `sid`'s transcript: `<sid>.jsonl` in one of the project folders under `projects`
pub fn find_transcript(projects: &std::path::Path, sid: &str) -> Option<std::path::PathBuf> {
    let name = format!("{sid}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|dir| dir.path().join(&name))
        .find(|path| path.is_file())
}

/// One session as the reading thread follows it
#[derive(Default)]
struct Session {
    path: Option<std::path::PathBuf>,
    follow: Follow,
    /// When the permission records were last read, by their file's time
    events_at: Option<std::time::SystemTime>,
    /// The conversation's revision when its snapshot was last given out
    given: Option<u64>,
}

/// What's watched for the sessions shown: each one's transcript, or while it isn't there yet, a
/// file of its name in any project's folder; and its permission records
fn targets(shared: &Shared, shown: &[String], sessions: &HashMap<String, Session>) -> Vec<crate::watch::Target> {
    use crate::watch::Target;
    let mut out = vec![];
    for sid in shown {
        match sessions.get(sid).and_then(|s| s.path.clone()) {
            Some(path) => out.push(Target::File(path)),
            None => out.push(Target::Within(shared.projects.clone(), format!("{sid}.jsonl"))),
        }
        if let Some(dir) = shared.summaries.as_deref() {
            out.push(Target::File(dir.join(format!("{sid}.events.jsonl"))));
        }
    }
    out
}

/// The session a transcript (`<sid>.jsonl`) or permission records' file (`<sid>.events.jsonl`)
/// is for, and whether it's the records'
fn file_of(path: &std::path::Path) -> Option<(String, bool)> {
    let name = path.file_name()?.to_str()?;
    if let Some(sid) = name.strip_suffix(".events.jsonl") {
        return Some((sid.to_string(), true));
    }
    name.strip_suffix(".jsonl").filter(|sid| !sid.contains('.')).map(|sid| (sid.to_string(), false))
}

/// The reading thread: each session shown is read when it's first shown and whenever Windows
/// says one of its files changed, from where its last read stopped; asleep the rest of the time.
fn follow(shared: Arc<Shared>, rx: std::sync::mpsc::Receiver<Msg>, events: std::sync::mpsc::Sender<Msg>) {
    use crate::watch::{Seen, Watch};
    let mut watch = match Watch::new(move |s| {
        let _ = events.send(Msg::Seen(s));
    }) {
        Ok(watch) => watch,
        Err(err) => {
            log::error!("conversation: can't watch the transcripts, so the scroll strip won't follow them: {err:#}");
            return;
        }
    };
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut shown: Vec<String> = vec![];
    let mut followed: Vec<crate::watch::Target> = vec![];
    let mut msgs: Vec<Msg> = vec![];
    loop {
        // Each session shown that's due a read, and of which: its transcript, its permission
        // records. A change is read and nothing else is looked at: until 2026-10-30 every look
        // opened and read the transcript and looked at the permission records' file
        let mut due: HashMap<String, (bool, bool)> = HashMap::new();
        let mut all = false;
        let mut refollow = false;
        for msg in msgs.drain(..) {
            match msg {
                Msg::Show { shown: now, live } => {
                    for sid in now.iter().filter(|s| !shown.contains(s)) {
                        due.insert(sid.clone(), (true, true));
                    }
                    shown = now;
                    // What no pane names any more is let go; the rest keep what was read of them
                    sessions.retain(|sid, _| shown.contains(sid) || live.contains(sid));
                    lock(&shared.known).retain(|sid, _| sessions.contains_key(sid) || shown.contains(sid));
                }
                // Only a change that can be about a session's files is passed on (crate::watch):
                // one of them, or a folder on the way to them made, removed or renamed
                Msg::Seen(Seen::Changed(paths)) => {
                    for path in &paths {
                        match file_of(path) {
                            Some((sid, events)) => {
                                if shown.contains(&sid) {
                                    let d = due.entry(sid).or_default();
                                    if events {
                                        d.1 = true;
                                    } else {
                                        d.0 = true;
                                    }
                                }
                            }
                            None => refollow = true,
                        }
                    }
                }
                Msg::Seen(Seen::Failed(err)) => {
                    log::error!("conversation: the transcripts' watch: {err}; reading each shown again");
                    all = true;
                }
            }
        }
        // A transcript not found yet is looked for when it's shown and when one of its name comes
        for sid in &shown {
            let session = sessions.entry(sid.clone()).or_default();
            if session.path.is_none() && (due.contains_key(sid) || all) {
                session.path = find_transcript(&shared.projects, sid);
            }
        }
        let want = targets(&shared, &shown, &sessions);
        if refollow || all || want != followed {
            #[cfg(test)]
            (shared.before_follow)();
            let mut want = want;
            for _ in 0..FOLLOWS {
                let moved = watch.follow(&want);
                let held = std::mem::replace(&mut followed, want);
                // A folder came with its files, or one went: what's shown is read again
                all |= moved;
                if !moved && held == followed {
                    break;
                }
                // What the watch is for changed, its folders or only its targets (another session
                // not written yet, in a folder watched already): a transcript not found yet is
                // looked for again now that the watch holds its name. One written after the look
                // above and before then said nothing to the watch, whose thread passes on only
                // what its targets name, and until 2026-11-02 the strip didn't show it until it
                // was written to again (lbterm/PITFALLS.md, traps 60 and 63)
                for sid in &shown {
                    let session = sessions.entry(sid.clone()).or_default();
                    if session.path.is_none() {
                        session.path = find_transcript(&shared.projects, sid);
                        if session.path.is_some() {
                            due.insert(sid.clone(), (true, true));
                        }
                    }
                }
                want = targets(&shared, &shown, &sessions);
            }
        }
        if all {
            for sid in &shown {
                due.insert(sid.clone(), (true, true));
            }
        }

        let mut woke = vec![];
        for (sid, (transcript, events)) in due {
            let Some(session) = sessions.get_mut(&sid).filter(|_| shown.contains(&sid)) else {
                continue;
            };
            let Some(path) = session.path.clone() else {
                continue;
            };
            if transcript {
                session.follow.read_more(&path);
            }
            if let Some(dir) = shared.summaries.as_deref().filter(|_| events) {
                let events_path = dir.join(format!("{sid}.events.jsonl"));
                let at = std::fs::metadata(&events_path).and_then(|m| m.modified()).ok();
                if at != session.events_at {
                    session.events_at = at;
                    let text = std::fs::read_to_string(&events_path).unwrap_or_default();
                    session.follow.conversation.permissions(&text);
                }
            }
            // A first snapshot once the transcript is found, then one each time what it shows
            // changed
            let revision = session.follow.conversation.revision();
            if session.given != Some(revision) {
                let snapshot = Arc::new(session.follow.conversation.snapshot());
                session.given = Some(revision);
                let context = snapshot.context.clone();
                let was = lock(&shared.known).insert(sid.clone(), snapshot);
                // Its context moved when the snapshot's is another Arc than the last one given
                // (Conversation::context_change makes a new one only for a change)
                woke.push((sid, was.map_or(true, |w| !Arc::ptr_eq(&w.context, &context))));
            }
        }
        if !woke.is_empty() {
            shared.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            for (sid, context) in &woke {
                (shared.wake)(sid, *context);
            }
        }

        // Asleep until a window shows another session or a file shown changes
        let Ok(first) = rx.recv() else {
            return;
        };
        let matters = match &first {
            Msg::Show { .. } | Msg::Seen(Seen::Failed(_)) => true,
            Msg::Seen(Seen::Changed(paths)) => watch.matters(paths),
        };
        match &first {
            Msg::Show { shown, .. } => log::debug!("conversation: shown {shown:?}"),
            Msg::Seen(seen) => log::debug!("conversation: {seen:?}, matters {matters}"),
        }
        msgs.push(first);
        if matters {
            std::thread::sleep(SETTLE);
            msgs.extend(rx.try_iter());
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod real;
#[cfg(test)]
mod long;
#[cfg(test)]
mod pieces_tests;
