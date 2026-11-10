//! The strip's last prompt (lbterm/DESIGN.md, "The window strip"). With Claude Code's prompt
//! history (`lbterm_prompt_history`), the strip's second row shows the active pane's last
//! prompt, claudehook.exe's `claude_prompt` user var, and a menu lists every prompt the session
//! has sent, from Claude Code's `history.jsonl`, each a way back to where it is in the
//! conversation. The GUI side is wezterm-gui/src/termwindow/render/prompt.rs.
//!
//! Claude Code's fullscreen renderer keeps the conversation to itself: the pane holds only the
//! screen. So LBTerm can't scroll to a message; it asks Claude Code to, with the keys Claude Code
//! already knows (Ctrl+End to the latest message, then PgUp half a screen at a time), reading the
//! screen after each step ([`Jump`]) until the message is on it, a row on the grey band Claude
//! Code draws your messages on, starting "❯ ", or in the dim header it keeps at the top for the
//! last prompt scrolled out of view, which jumps to that prompt when clicked.

use crate::tabs::Rect;
use chrono::NaiveDateTime;

/// What Claude Code draws before a message you sent, and before what you're typing
pub const MARK: char = '\u{276f}';
/// How much of a prompt's first line is matched on screen
pub const KEY_CHARS: usize = 40;
/// A row shorter than this that a prompt only starts with doesn't count: too likely by chance
const MIN_MATCH: usize = 12;
/// The most PgUp presses a jump makes before it gives up
pub const MAX_STEPS: u32 = 400;
/// How many rows from the top Claude Code's header can be in
const HEADER_ROWS: usize = 3;
/// How many rows at the bottom are Claude Code's input box and status, never the header
const BOTTOM_ROWS: usize = 6;

/// One prompt a session sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    /// As typed, pasted text standing in as Claude Code shows it ("[Pasted text #1 +20 lines]")
    pub text: String,
    /// When, in Unix milliseconds
    pub at_ms: i64,
}

/// The prompts session `sid` sent, oldest first, from `history`, the text of Claude Code's
/// history.jsonl: one record a line, `display` what was typed, `timestamp` when, `sessionId`
/// whose. Slash commands aren't messages and are left out; so is anything unreadable.
pub fn sent(history: &str, sid: &str) -> Vec<Sent> {
    let mut out: Vec<Sent> = history
        .lines()
        // Most lines are other sessions': cheap to pass over before parsing
        .filter(|line| line.contains(sid))
        .filter_map(record)
        .filter(|(whose, _)| whose == sid)
        .map(|(_, sent)| sent)
        .collect();
    out.sort_by_key(|s| s.at_ms);
    out
}

/// A line of history.jsonl: whose prompt, and the prompt, if it's one the list shows
fn record(line: &str) -> Option<(String, Sent)> {
    let record = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let sid = record.get("sessionId")?.as_str()?.to_string();
    let text = record.get("display")?.as_str()?.trim().to_string();
    let at_ms = match record.get("timestamp")? {
        serde_json::Value::Number(n) => n.as_i64()?,
        serde_json::Value::String(s) => s.parse().ok()?,
        _ => return None,
    };
    (!text.is_empty() && !text.starts_with('/')).then_some((sid, Sent { text, at_ms }))
}

/// How many bytes before where a read stopped are kept, to tell the next read whether the file
/// is still the one read (appended to) or was written anew
const KEEP: usize = 256;

/// Claude Code's history.jsonl as read so far: where each session's prompts are in it. The file
/// is every session's (2 MB and 6,600 lines on Lachlan's PC on 2026-10-31, and it only grows), so
/// it's read whole once and after that only what was added since ([`History::read_on`]), not
/// whole again for each click on the strip's prompt or its list. What's kept is where each
/// session's records are, not the prompts (pasted text and all): a session's are read from the
/// file when they're asked for ([`History::sent`]), so what the GUI holds for its whole life
/// stays small however long the file grows
#[derive(Debug, Default)]
pub struct History {
    /// The bytes read: up to the end of the last whole line
    read: u64,
    /// The last of them, up to [`KEEP`]
    tail: Vec<u8>,
    /// Where each session's records are in the file, in its order: each line's start and length
    sessions: std::collections::HashMap<String, Vec<(u64, u32)>>,
}

impl History {
    /// The prompts session `sid` sent, oldest first, as [`sent`] gives them: its records read
    /// from `path`, the file [`Self::read_on`] last read
    pub fn sent(&self, path: &std::path::Path, sid: &str) -> std::io::Result<Vec<Sent>> {
        use std::io::{Read, Seek, SeekFrom};
        let Some(lines) = self.sessions.get(sid) else {
            return Ok(vec![]);
        };
        let mut file = std::fs::File::open(path)?;
        let mut out = vec![];
        let mut line = vec![];
        for &(at, len) in lines {
            line.resize(len as usize, 0);
            file.seek(SeekFrom::Start(at))?;
            file.read_exact(&mut line)?;
            if let Some((whose, sent)) = record(&String::from_utf8_lossy(&line)) {
                if whose == sid {
                    out.push(sent);
                }
            }
        }
        out.sort_by_key(|s| s.at_ms);
        Ok(out)
    }

    /// How many sessions it knows of, and how many records it knows where they are
    pub fn size(&self) -> (usize, usize) {
        (self.sessions.len(), self.sessions.values().map(Vec::len).sum())
    }

    /// How many bytes of the file have been read
    pub fn read(&self) -> u64 {
        self.read
    }

    /// Takes in `bytes`, the file from where the last read stopped: its whole lines, the part
    /// after the last waiting for the rest of it. How many bytes it took
    pub fn feed(&mut self, bytes: &[u8]) -> usize {
        let Some(last) = bytes.iter().rposition(|&b| b == b'\n') else {
            return 0;
        };
        let whole = &bytes[..=last];
        let mut at = self.read;
        for line in whole.split(|&b| b == b'\n') {
            let start = at;
            at += line.len() as u64 + 1;
            if line.is_empty() || line.len() > u32::MAX as usize {
                continue;
            }
            if let Some((sid, _)) = record(&String::from_utf8_lossy(line)) {
                self.sessions.entry(sid).or_default().push((start, line.len() as u32));
            }
        }
        self.read += whole.len() as u64;
        let mut tail = std::mem::take(&mut self.tail);
        tail.extend_from_slice(whole);
        let from = tail.len().saturating_sub(KEEP);
        self.tail = tail.split_off(from);
        whole.len()
    }

    /// Reads `path` on from where the last read stopped; from its start again when it's shorter
    /// than that, or the bytes before that aren't the ones read (written anew, not appended to)
    pub fn read_on(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        let kept = self.tail.len() as u64;
        let same = len >= self.read && {
            let mut before = vec![0u8; kept as usize];
            file.seek(SeekFrom::Start(self.read - kept))?;
            file.read_exact(&mut before)?;
            before == self.tail
        };
        if !same {
            *self = History::default();
        }
        file.seek(SeekFrom::Start(self.read))?;
        let mut bytes = vec![];
        file.read_to_end(&mut bytes)?;
        self.feed(&bytes);
        Ok(())
    }
}

/// `text` on one line, its whitespace run together
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the row a prompt starts on shows after "❯ ": its first line with text, run together,
/// at most [`KEY_CHARS`] of it
pub fn key(prompt: &str) -> String {
    let first = prompt.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    one_line(first).chars().take(KEY_CHARS).collect()
}

/// A row of a pane's screen, as a jump reads it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Row {
    pub text: String,
    /// The cell under its first mark is on Claude Code's grey message band
    pub band: bool,
    /// The words after its mark are dim against the band: Claude Code's sticky header, which
    /// 2.1.312 draws on the band too (grey on grey, where a message you sent is white)
    pub dim: bool,
}

/// Whether words in `fg` on `bg` (sRGB) are dim: close to the band in brightness, as Claude
/// Code's sticky header's grey on grey is (80 on 55), where a message's are white on it. `None`
/// is the terminal's own foreground, never dim; `faint` is SGR 2.
pub fn dim_words(fg: Option<(u8, u8, u8)>, bg: (u8, u8, u8), faint: bool) -> bool {
    let light = |(r, g, b): (u8, u8, u8)| (0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64) / 255.0;
    faint || fg.is_some_and(|fg| (light(fg) - light(bg)).abs() < 0.3)
}

/// Where a prompt was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// On screen, at this row
    Row(usize),
    /// In Claude Code's header at this row: a click there jumps to it
    Header(usize),
}

/// What follows the mark on a row that starts with it
fn after_mark(row: &str) -> Option<String> {
    let rest = row.trim_start().strip_prefix(MARK)?;
    Some(one_line(rest))
}

fn matches(shown: &str, key: &str) -> bool {
    !key.is_empty()
        && (shown.starts_with(key)
            || (shown.chars().count() >= MIN_MATCH && key.starts_with(shown)))
}

/// Where the prompt whose [`key`] is `key` is on `rows`, the pane's screen top to bottom: the
/// lowest row on the band that starts with the mark and then it, else a row near the top
/// that does with its words dim, or off the band as before 2.1.312, which is Claude Code's header. The input box at the bottom starts
/// with the mark too, but isn't on the band and isn't near the top.
pub fn find(rows: &[Row], key: &str) -> Option<Found> {
    let on_band = rows
        .iter()
        .enumerate()
        .rev()
        .find(|(_, row)| row.band && !row.dim && after_mark(&row.text).map_or(false, |s| matches(&s, key)));
    if let Some((i, _)) = on_band {
        return Some(Found::Row(i));
    }
    let top = HEADER_ROWS.min(rows.len().saturating_sub(BOTTOM_ROWS));
    rows[..top]
        .iter()
        .position(|row| (!row.band || row.dim) && after_mark(&row.text).map_or(false, |s| matches(&s, key)))
        .map(Found::Header)
}

/// What a jump does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// There, on screen at this row
    Found(usize),
    /// In Claude Code's header at this row: click it, then look again. Only once: should the
    /// click not bring it into view, the jump carries on as if the header weren't there.
    Click(usize),
    /// Not yet: PgUp, then look again
    Up,
    /// The screen stopped changing: the conversation's start, or a compaction, which Claude Code
    /// doesn't scroll past
    Top,
    /// [`MAX_STEPS`] presses and still not there
    TooFar,
}

/// One jump to a prompt: after Ctrl+End and after each PgUp or click, once the screen has
/// settled, [`Jump::look`] says what to do next.
#[derive(Debug, Clone)]
pub struct Jump {
    key: String,
    steps: u32,
    last: Option<Vec<Row>>,
    clicked: bool,
}

impl Jump {
    pub fn new(prompt: &str) -> Self {
        Self {
            key: key(prompt),
            steps: 0,
            last: None,
            clicked: false,
        }
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }

    pub fn look(&mut self, rows: &[Row]) -> Step {
        match find(rows, &self.key) {
            Some(Found::Row(row)) => return Step::Found(row),
            Some(Found::Header(row)) if !self.clicked => {
                self.clicked = true;
                return Step::Click(row);
            }
            _ => {}
        }
        if self.last.as_deref() == Some(rows) {
            return Step::Top;
        }
        if self.steps >= MAX_STEPS {
            return Step::TooFar;
        }
        self.last = Some(rows.to_vec());
        self.steps += 1;
        Step::Up
    }
}

/// When a prompt was sent, for its row in the list: the time today, the day and time this week,
/// the date before that
pub fn stamp(at: NaiveDateTime, now: NaiveDateTime) -> String {
    let days = (now.date() - at.date()).num_days();
    if days <= 0 {
        at.format("%H:%M").to_string()
    } else if days < 7 {
        at.format("%a %H:%M").to_string()
    } else {
        at.format("%-d %b").to_string()
    }
}

/// The list of prompts, laid out: newest at the top, as many as fit, the rest a scroll away.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub card: Rect,
    /// Each row shown: the index into the prompts it shows, and where it is
    pub rows: Vec<(usize, Rect)>,
    /// Where the stamps end and the prompts begin
    pub text_x: f64,
    /// How far down the newest-first list the first row shown is
    pub scroll: usize,
}

impl List {
    /// The prompt under a point, by its index
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.rows.iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| *i)
    }
}

/// Lay out the list of `count` prompts (oldest first, so the newest is `count - 1`) hanging
/// from `top` with its text at `left`, in a window `window` wide and tall, rows `row_h` high,
/// `stamp_w` for the stamps and at most `width` for the card; scrolled `scroll` rows down from
/// the newest. `scale` is the DPI over 96.
pub fn list(
    count: usize,
    left: f64,
    top: f64,
    window: (f64, f64),
    (row_h, stamp_w, width): (f64, f64, f64),
    scroll: usize,
    scale: f64,
) -> Option<List> {
    if count == 0 || row_h <= 0.0 {
        return None;
    }
    let pad = 6.0 * scale;
    let inset = 12.0 * scale;
    let room = (window.1 - top - 2.0 * pad - 16.0 * scale).max(row_h);
    let shown = ((room / row_h).floor() as usize).clamp(1, count);
    let scroll = scroll.min(count - shown);
    let x = (left - inset).max(4.0 * scale);
    let w = width.min(window.0 - x - 4.0 * scale).max(row_h);
    let rows = (0..shown)
        .map(|i| {
            let index = count - 1 - (scroll + i);
            let r = Rect {
                x: x + pad,
                y: top + pad + i as f64 * row_h,
                w: w - 2.0 * pad,
                h: row_h,
            };
            (index, r)
        })
        .collect();
    Some(List {
        card: Rect {
            x,
            y: top,
            w,
            h: 2.0 * pad + shown as f64 * row_h,
        },
        rows,
        text_x: x + inset + stamp_w,
        scroll,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::NaiveDate;

    const SID: &str = "5b16ebf8-2193-4c69-b96a-6861c16978f5";
    const OTHER: &str = "e4951d29-7131-41b7-b387-77a8effcf15a";

    fn record(display: &str, at: i64, sid: &str) -> String {
        serde_json::json!({
            "display": display,
            "pastedContents": {},
            "timestamp": at,
            "project": "C:\\Users\\me\\Code\\LBTerm",
            "sessionId": sid,
        })
        .to_string()
    }

    #[test]
    fn a_sessions_prompts_come_from_the_history_oldest_first() {
        let history = [
            record("Lets work on the summary panel", 1000, SID),
            record("1 yes\n\nand give me a new prompt", 1500, OTHER),
            record("/model", 1700, SID),
            record("show me the layouts in browser please", 2000, SID),
            "not json".to_string(),
            record("  ", 2500, SID),
            // A timestamp written as a string still counts
            r#"{"display":"1c","timestamp":"1800","sessionId":"5b16ebf8-2193-4c69-b96a-6861c16978f5"}"#
                .to_string(),
        ]
        .join("\n");
        let got = sent(&history, SID);
        let texts: Vec<&str> = got.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "Lets work on the summary panel",
                "1c",
                "show me the layouts in browser please"
            ]
        );
        assert_eq!(got[0].at_ms, 1000);
        assert!(sent(&history, "11111111-2222-3333-4444-555555555555").is_empty());
    }

    #[test]
    fn the_history_read_on_gives_what_a_whole_read_gives() {
        let lines = [
            record("Lets work on the summary panel", 1000, SID),
            record("1 yes\n\nand give me a new prompt", 1500, OTHER),
            record("/model", 1700, SID),
            // Written out of order: still oldest first
            record("show me the layouts in browser please", 900, SID),
            "not json".to_string(),
        ];
        let whole = lines.join("\n") + "\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let mut h = History::default();
        // Written a piece at a time, a line cut in two among them
        let cut = whole.len() / 2 + 7;
        std::fs::write(&path, &whole[..cut]).unwrap();
        h.read_on(&path).unwrap();
        assert!(h.read() <= cut as u64);
        std::fs::write(&path, &whole).unwrap();
        h.read_on(&path).unwrap();
        assert_eq!(h.read(), whole.len() as u64);
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&whole, SID));
        assert_eq!(h.sent(&path, OTHER).unwrap(), sent(&whole, OTHER));
        assert_eq!(h.sent(&path, SID).unwrap()[0].text, "show me the layouts in browser please");
        // Read again with nothing added: the same prompts
        h.read_on(&path).unwrap();
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&whole, SID));
        // A line added: read on
        let more = whole.clone() + &record("and one more thing", 3000, SID) + "\n";
        std::fs::write(&path, &more).unwrap();
        h.read_on(&path).unwrap();
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&more, SID));
        assert_eq!(h.sent(&path, SID).unwrap().len(), 3);
    }

    #[test]
    fn the_history_keeps_where_prompts_are_not_the_prompts() {
        // A long paste among them: what's kept is a place for each record, whatever its size
        let paste = "x".repeat(100_000);
        let lines = [
            record("first", 1000, SID),
            record(&paste, 1100, OTHER),
            record("/clear", 1200, SID),
            record("second", 1300, SID),
        ];
        let whole = lines.join("\n") + "\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        std::fs::write(&path, &whole).unwrap();
        let mut h = History::default();
        h.read_on(&path).unwrap();
        // Two sessions, three records (the slash command isn't one), read back from the file
        assert_eq!(h.size(), (2, 3));
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&whole, SID));
        assert_eq!(h.sent(&path, OTHER).unwrap()[0].text, paste);
        assert!(h.sent(&path, "11111111-2222-3333-4444-555555555555").unwrap().is_empty());
        // Read on: the new record's place is past the old ones'
        let more = whole.clone() + &record("third", 1400, SID) + "\n";
        std::fs::write(&path, &more).unwrap();
        h.read_on(&path).unwrap();
        assert_eq!(h.size(), (2, 4));
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&more, SID));
    }

    #[test]
    fn a_history_written_anew_is_read_from_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let first = [record("old prompt one", 1000, SID), record("old prompt two", 1100, SID)].join("\n") + "\n";
        std::fs::write(&path, &first).unwrap();
        let mut h = History::default();
        h.read_on(&path).unwrap();
        assert_eq!(h.sent(&path, SID).unwrap().len(), 2);
        // Trimmed to its last line and appended to, longer than before: not the bytes read
        let anew = record("old prompt two", 1100, SID) + "\n" + &record("a newer prompt, long enough to be longer", 1200, SID) + "\n";
        assert!(anew.len() > first.len());
        std::fs::write(&path, &anew).unwrap();
        h.read_on(&path).unwrap();
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&anew, SID));
        // Shorter than what was read
        let short = record("just this", 1300, SID) + "\n";
        std::fs::write(&path, &short).unwrap();
        h.read_on(&path).unwrap();
        assert_eq!(h.sent(&path, SID).unwrap(), sent(&short, SID));
    }

    #[test]
    fn a_prompts_key_is_its_first_line() {
        assert_eq!(key("1 yes\n\nand give me a new prompt"), "1 yes");
        assert_eq!(key("\n  first   words\nsecond"), "first words");
        let long = "can we work on the previous user sent message feature? Currently";
        assert_eq!(key(long).chars().count(), KEY_CHARS);
        assert!(long.starts_with(&key(long)));
    }

    fn row(text: &str, band: bool) -> Row {
        Row {
            text: text.to_string(),
            band,
            dim: false,
        }
    }

    /// A fullscreen Claude Code screen: a message on the band, its answer, the input box
    fn screen(message: &str) -> Vec<Row> {
        let mut rows = vec![
            row("● Update(CLAUDE.md)", false),
            row("  L Added 1 line, removed 1 line", false),
            row("", false),
            row(&format!("\u{276f} {message}"), true),
            row("  and the rest of it, wrapped", true),
            row("", false),
            row("● Here's how I'll build it:", false),
        ];
        rows.extend((0..4).map(|_| row("  - a point", false)));
        rows.push(row("", false));
        rows.push(row("\u{276f} ", false));
        rows.push(row("  [Opus 5.5] Projects | main", false));
        rows.push(row("  >> auto mode on", false));
        rows
    }

    #[test]
    fn a_message_on_the_band_is_found_where_it_is() {
        let prompt = "1c and if I press the arrow on the left of the text, it will open a line";
        let rows = screen(prompt);
        assert_eq!(find(&rows, &key(prompt)), Some(Found::Row(3)));
        // Not another message, and never the input box, which starts with the mark too
        assert_eq!(find(&rows, &key("so what does the summary pannel currently do?")), None);
        assert_eq!(find(&rows, ""), None);
    }

    #[test]
    fn a_short_prompt_matches_only_itself() {
        let rows = screen("yes");
        assert_eq!(find(&rows, &key("yes")), Some(Found::Row(3)));
        assert_eq!(find(&rows, &key("yes please, all of it")), None);
    }

    #[test]
    fn a_message_cut_at_the_panes_edge_still_matches() {
        // The pane is narrower than the key: the row shows only the start
        let rows = vec![row("\u{276f} can we work on the previous", true)];
        assert_eq!(
            find(&rows, &key("can we work on the previous user sent message feature?")),
            Some(Found::Row(0))
        );
    }

    #[test]
    fn claude_codes_header_is_found_near_the_top() {
        let prompt = "show me the layouts in browser please";
        let mut rows = screen("something else entirely, sent later");
        rows[0] = row(&format!("\u{276f} {prompt}"), false);
        assert_eq!(find(&rows, &key(prompt)), Some(Found::Header(0)));
        // The band wins: the message itself on screen
        let mut both = screen(prompt);
        both[0] = row(&format!("\u{276f} {prompt}"), false);
        assert_eq!(find(&both, &key(prompt)), Some(Found::Row(3)));
    }

    #[test]
    fn the_header_on_the_band_is_told_from_a_message_by_its_dim_words() {
        // Claude Code 2.1.312 draws its sticky header on the band, its words grey on grey
        let prompt = "Write a 12-line poem about terminals.";
        let mut rows = screen("something else entirely, sent later");
        rows[0] = Row { text: format!("\u{276f} {prompt}"), band: true, dim: true };
        assert_eq!(find(&rows, &key(prompt)), Some(Found::Header(0)));
        // A message on the band in white is the message itself
        rows[0].dim = false;
        assert_eq!(find(&rows, &key(prompt)), Some(Found::Row(0)));
    }

    #[test]
    fn a_jump_presses_page_up_until_it_finds_the_prompt() {
        let prompt = "Lets work on the summary panel";
        let mut jump = Jump::new(prompt);
        let page = |n: usize| -> Vec<Row> {
            (0..20).map(|i| row(&format!("● answer {n}.{i}"), false)).collect()
        };
        assert_eq!(jump.look(&page(0)), Step::Up);
        assert_eq!(jump.look(&page(1)), Step::Up);
        let mut there = page(2);
        there[5] = row(&format!("\u{276f} {prompt}"), true);
        assert_eq!(jump.look(&there), Step::Found(5));
        assert_eq!(jump.steps(), 2);
    }

    #[test]
    fn a_jump_clicks_claude_codes_header_once() {
        let prompt = "show me the layouts in browser please";
        let mut jump = Jump::new(prompt);
        let mut rows: Vec<Row> = (0..20).map(|i| row(&format!("\u{25cf} answer {i}"), false)).collect();
        rows[0] = row(&format!("\u{276f} {prompt}"), false);
        assert_eq!(jump.look(&rows), Step::Click(0));
        // The click did nothing: on as before, up the conversation, until it stops moving
        assert_eq!(jump.look(&rows), Step::Up);
        assert_eq!(jump.look(&rows), Step::Top);
        // The click worked: there it is
        let mut jump = Jump::new(prompt);
        assert_eq!(jump.look(&rows), Step::Click(0));
        let mut landed = rows.clone();
        landed[0] = row("\u{25cf} an answer", false);
        landed[2] = row(&format!("\u{276f} {prompt}"), true);
        assert_eq!(jump.look(&landed), Step::Found(2));
    }

    #[test]
    fn a_jump_stops_where_the_screen_stops_changing() {
        let mut jump = Jump::new("not in this conversation any more");
        let start: Vec<Row> = vec![row("● Conversation compacted", false), row("● older", false)];
        assert_eq!(jump.look(&start), Step::Up);
        assert_eq!(jump.look(&start), Step::Top);
    }

    #[test]
    fn a_jump_gives_up_after_its_most_steps() {
        let mut jump = Jump::new("never here");
        for n in 0..MAX_STEPS {
            let rows = vec![row(&format!("page {n}"), false)];
            assert_eq!(jump.look(&rows), Step::Up);
        }
        assert_eq!(jump.look(&[row("one more", false)]), Step::TooFar);
    }

    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn stamps_say_the_time_then_the_day_then_the_date() {
        let now = at(21, 13, 38);
        assert_eq!(stamp(at(21, 9, 5), now), "09:05");
        // 20 October 2026 is a Tuesday
        assert_eq!(stamp(at(20, 22, 22), now), "Tue 22:22");
        assert_eq!(stamp(at(12, 22, 22), now), "12 Oct");
    }

    #[test]
    fn the_list_shows_the_newest_first_and_scrolls_to_the_rest() {
        // 30 prompts, rows 24 px, room for 10 under a strip in a 330 px window
        let l = list(30, 90.0, 56.0, (1080.0, 330.0), (24.0, 60.0, 720.0), 0, 1.0).unwrap();
        assert_eq!(l.rows.len(), 10);
        assert_eq!(l.rows[0].0, 29, "the newest at the top");
        assert_eq!(l.rows[9].0, 20);
        assert_eq!(l.card.x, 78.0);
        assert_eq!(l.card.h, 12.0 + 240.0);
        assert_eq!(l.text_x, 78.0 + 12.0 + 60.0);
        // A row knows which prompt it is
        let (_, r) = l.rows[2];
        assert_eq!(l.hit(r.x + 5.0, r.y + 5.0), Some(27));
        assert_eq!(l.hit(5.0, 5.0), None);
        // Scrolled to the oldest, and no further
        let l = list(30, 90.0, 56.0, (1080.0, 330.0), (24.0, 60.0, 720.0), 99, 1.0).unwrap();
        assert_eq!(l.scroll, 20);
        assert_eq!(l.rows.last().unwrap().0, 0);
        // A few prompts take only the rows they need; none, no list
        assert_eq!(list(3, 90.0, 56.0, (1080.0, 900.0), (24.0, 60.0, 720.0), 0, 1.0).unwrap().rows.len(), 3);
        assert!(list(0, 90.0, 56.0, (1080.0, 900.0), (24.0, 60.0, 720.0), 0, 1.0).is_none());
        // Kept inside a narrow window
        let narrow = list(3, 90.0, 56.0, (500.0, 900.0), (24.0, 60.0, 720.0), 0, 1.0).unwrap();
        assert!(narrow.card.x + narrow.card.w <= 500.0);
    }
}
