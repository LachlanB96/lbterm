//! The context bar's Remove, Undo and Compact now (lbterm/CONTEXT.md, "The buttons"). A button
//! stops a file loading again and never deletes one (Lachlan's 3a of 2026-11-08): a memory file's
//! line comes off its memory index (`MEMORY.md`), the file kept; a file another loaded file imports
//! has the `@` of each import naming it taken off, on whatever line it's on, the line's words and
//! the path staying as plain text, which Claude Code doesn't import (Lachlan's answer to Q2 of
//! 2026-11-09: an import is an import only by its `@`, so no line is deleted or refused). Undo puts
//! the line, or the `@`, back where it was. Neither takes the file out of the running
//! conversation. A compaction drops it with the rest, loads the memory index or the CLAUDE.md again
//! without it, and attaches the file itself again after its summary while it's among the five files
//! Claude Code restores, at every compaction (lbterm/CONTEXT.md, "What Claude Code restores after a
//! compaction"); nothing Claude Code offers keeps one file out of that for one conversation, so the
//! rows say exactly that ([`State::note`], [`State::attached_note`]). Compact now types `/compact`
//! into the tab, only while Claude Code is at its prompt with nothing typed ([`may_compact`]).
//!
//! Each edit reads the file whole, finds its line or its `@`s (refusing, with why, when a memory
//! file's line isn't there exactly once, or no import names the file), and writes the file anew
//! beside it and renames it over, so Claude Code never reads it half written. Every other line keeps
//! its own ending, a byte order mark stays, and a file that ended without a new line still does.
//! [`run`] does the reading and writing, on the `lbterm-context-edits` thread
//! (wezterm-gui/src/context_edits.rs), never the GUI thread; the edits themselves ([`take_out`],
//! [`put_back`]) are text in, text out, tested without a file.

use crate::context::{import_lines, normal, parts};
use crate::context_bar::{Bar, Removal, Section, What};
use crate::drafts::{bottom, Bottom};
use crate::questions::{is_space, Screen};
use std::path::Path;

/// What a Remove took out, and what Undo needs to put it back where it was
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// A memory file's line, off its index whole
    Line(Line),
    /// An imported file's `@`s, off the lines of the file importing it
    Marks(Marks),
}

impl Removed {
    /// The file it was taken out of, as Claude Code wrote its path
    pub fn file(&self) -> &str {
        match self {
            Removed::Line(l) => &l.file,
            Removed::Marks(m) => &m.file,
        }
    }
}

/// A line taken out whole (a memory file's, off its index)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The file it came out of, as Claude Code wrote its path
    pub file: String,
    /// The line, without its ending
    pub line: String,
    /// Its ending, `\r\n` or `\n`; for a last line that had none, the one the line before it had
    pub ending: String,
    /// Its number, from 0
    pub at: usize,
    /// The lines either side of it as they were, without their endings
    pub before: Option<String>,
    pub after: Option<String>,
}

/// The `@`s taken off an imported file's imports: every one naming it in the file importing it,
/// each line's words and the path left as plain text
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    /// The file they came off, as Claude Code wrote its path
    pub file: String,
    /// The lines they came off, in the file's order
    pub lines: Vec<Marked>,
}

/// A line an import's `@` came off
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    /// Its number, from 0
    pub at: usize,
    /// The line as it was, without its ending
    pub was: String,
    /// The bytes in `was` where each `@` taken off was, in order
    pub bytes: Vec<usize>,
}

impl Marked {
    /// The line as Remove left it: `was` without its `@`s
    pub fn left(&self) -> String {
        let mut out = String::with_capacity(self.was.len());
        let mut from = 0;
        for &b in &self.bytes {
            out.push_str(&self.was[from..b]);
            from = b + 1;
        }
        out.push_str(&self.was[from..]);
        out
    }

    /// The path its one import named, as written after the `@`; none with several `@`s off it
    fn path(&self) -> Option<&str> {
        let [byte] = self.bytes[..] else {
            return None;
        };
        self.was[byte + 1..].split(char::is_whitespace).next().filter(|p| !p.is_empty())
    }
}

/// Where Undo put a line, or an import's `@`, back
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
    /// At its old number, between the lines it was between
    Where,
    /// After the line it followed, which has moved
    After,
    /// At the file's end: the lines it was between aren't where they were, and the one it followed
    /// isn't there once
    End,
    /// An import's `@`, on its line found where it is now, or before its path where the line's
    /// words have changed
    Moved,
    /// The line, or the import, was there already: nothing written
    Already,
}

/// What a press of a section's button asks for
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Remove,
    Undo(Removed),
}

/// Where a section's removal stands, which its row shows
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The edit's under way
    Removing,
    /// Its line is out: Undo puts it back
    Removed(Removed),
    /// Undo's under way
    PuttingBack(Removed),
    /// Undone
    Back(Placed),
    /// The edit didn't happen: why, and the line still out when it was an Undo
    Refused { why: String, removed: Option<Removed> },
}

impl State {
    /// The button's words
    pub fn button(&self) -> &'static str {
        if self.removed() {
            "Undo"
        } else {
            "Remove"
        }
    }

    /// Whether the section's line is out of the file now (or going back, which may not get there)
    pub fn removed(&self) -> bool {
        matches!(self, State::Removed(_) | State::PuttingBack(_) | State::Refused { removed: Some(_), .. })
    }

    /// What a press does now: nothing while an edit's under way
    pub fn press(&self) -> Option<Act> {
        match self {
            State::Removing | State::PuttingBack(_) => None,
            State::Removed(r) | State::Refused { removed: Some(r), .. } => Some(Act::Undo(r.clone())),
            State::Back(_) | State::Refused { removed: None, .. } => Some(Act::Remove),
        }
    }

    /// The state while `act` is under way
    pub fn busy(act: &Act) -> State {
        match act {
            Act::Remove => State::Removing,
            Act::Undo(r) => State::PuttingBack(r.clone()),
        }
    }

    /// Its row's second line
    pub fn note(&self, removal: &Removal) -> String {
        let name = name_of(edited(removal));
        let memory = matches!(removal, Removal::MemoryLine { .. });
        match self {
            State::Removing if memory => format!("Taking its line off {name}…"),
            State::Removing => format!("Taking the @ off its import in {name}…"),
            State::Removed(_) if memory => format!("Off {name}: a compaction can attach it again"),
            State::Removed(_) => format!("Import off in {name}: a compaction can attach it again"),
            State::PuttingBack(_) if memory => format!("Putting its line back in {name}…"),
            State::PuttingBack(_) => format!("Putting its @ back in {name}…"),
            State::Back(Placed::Already) if memory => format!("Its line was back in {name} already"),
            State::Back(Placed::Already) => format!("Its @ was back in {name} already"),
            State::Back(Placed::End) => format!("Back in {name}, at its end"),
            State::Back(_) if memory => format!("Back in {name}"),
            State::Back(_) => format!("Import back on in {name}"),
            State::Refused { why, .. } => why.clone(),
        }
    }

    /// Its row's second line on the section of the file attached again after a compaction's
    /// summary (Compact now's, one typed by hand or one Claude Code made itself: every compaction
    /// attaches it while it's among the five files Claude Code restores): with its line or its `@`
    /// still out, it's in the conversation again only by that attachment, and the next compaction
    /// may attach it again too
    pub fn attached_note(&self, removal: &Removal) -> String {
        let name = name_of(edited(removal));
        match self {
            State::Removed(_) if matches!(removal, Removal::MemoryLine { .. }) => format!("Attached again after the summary, off {name}"),
            State::Removed(_) => format!("Attached again after the summary, @ off in {name}"),
            _ => self.note(removal),
        }
    }

    /// Its row's second line once its section has gone from the bar (a compaction took the file
    /// out, or a reload left the import out): with its line or its `@` still out, that Undo puts
    /// it back
    pub fn gone_note(&self, removal: &Removal) -> String {
        let name = name_of(edited(removal));
        match self {
            State::Removed(_) if matches!(removal, Removal::MemoryLine { .. }) => format!("Off {name}: Undo puts it back"),
            State::Removed(_) => format!("Import off in {name}: Undo puts its @ back"),
            _ => self.note(removal),
        }
    }
}

/// The removals pressed in a pane (`pressed`, as wezterm-gui's context_edits keeps them) whose
/// section `bar` hasn't got any more, in the order pressed: the pane lists each with its Undo
/// until it's undone or the pane closes, since a compaction (Compact now's, the step Lachlan's 3a
/// pairs with Remove) drops the removed file's section and its button with it, the line still out
/// (lbterm/CONTEXT.md, "The buttons"). One undone, or with an edit under way, stays listed,
/// saying so, until its section is back or the pane closes; a Remove refused with nothing out has
/// nothing to undo, and goes
pub fn off_the_bar<'a>(bar: &Bar, pressed: &'a [(Removal, State)]) -> Vec<&'a (Removal, State)> {
    pressed
        .iter()
        .filter(|(_, state)| !matches!(state, State::Refused { removed: None, .. }))
        .filter(|(removal, _)| !bar.sections.iter().any(|s| section_removal(s, pressed) == Some(removal)))
        .collect()
}

/// The removal a section's row shows: its own, or for a file attached again after a compaction's
/// summary, the removal pressed in the pane of the same file (its import's or its memory line's,
/// which the attachment doesn't say), so its row has that Undo and says what's true of it
pub fn section_removal<'a>(section: &'a Section, pressed: &'a [(Removal, State)]) -> Option<&'a Removal> {
    if section.what != What::Attached {
        return section.removal.as_ref();
    }
    let path = parts(section.path.as_deref()?);
    pressed.iter().map(|(r, _)| r).find(|r| parts(r.file()) == path)
}

/// The file a removal edits: the memory index, or the file importing it
pub fn edited(removal: &Removal) -> &str {
    match removal {
        Removal::MemoryLine { index, .. } => index,
        Removal::Import { importer, .. } => importer,
    }
}

/// A path's last part, as a row names the file it edits
fn name_of(path: &str) -> &str {
    path.rsplit(['\\', '/']).find(|p| !p.is_empty()).unwrap_or(path)
}

/// A file's text as lines, each with its own ending
struct Lines {
    bom: bool,
    lines: Vec<(String, String)>,
}

impl Lines {
    fn parse(text: &str) -> Lines {
        let (bom, body) = match text.strip_prefix('\u{feff}') {
            Some(body) => (true, body),
            None => (false, text),
        };
        let lines = body
            .split_inclusive('\n')
            .map(|l| match l.strip_suffix("\r\n") {
                Some(b) => (b.to_string(), "\r\n".to_string()),
                None => match l.strip_suffix('\n') {
                    Some(b) => (b.to_string(), "\n".to_string()),
                    None => (l.to_string(), String::new()),
                },
            })
            .collect();
        Lines { bom, lines }
    }

    fn text(&self) -> String {
        let mut out = String::from(if self.bom { "\u{feff}" } else { "" });
        for (body, ending) in &self.lines {
            out.push_str(body);
            out.push_str(ending);
        }
        out
    }

    /// The ending the file's lines use: the first one's, else `\n`
    fn ending(&self) -> String {
        self.lines.iter().map(|(_, e)| e).find(|e| !e.is_empty()).cloned().unwrap_or_else(|| "\n".to_string())
    }

    /// The one line that is `line`, if there's exactly one
    fn only(&self, line: &str) -> Option<usize> {
        let mut at = self.lines.iter().enumerate().filter(|(_, (b, _))| b == line).map(|(i, _)| i);
        match (at.next(), at.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    }
}

/// The targets of a line's Markdown links (`[words](target)`), without a title or an anchor
fn link_targets(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = line;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let target = rest[..end].trim();
        let target = match target.strip_prefix('<') {
            Some(t) => t.split('>').next().unwrap_or(""),
            None => target.split_whitespace().next().unwrap_or(""),
        };
        let target = target.split('#').next().unwrap_or("").replace("%20", " ");
        if !target.is_empty() && !target.contains("://") {
            out.push(target);
        }
        rest = &rest[end..];
    }
    out
}

/// The lines of the memory index (`index`, its lines `lines`) linking to memory file `file`
fn memory_lines(index: &str, file: &str, lines: &Lines) -> Vec<usize> {
    let mut folder = parts(index);
    folder.pop();
    let want = parts(file);
    let mut fenced = false;
    let mut out = vec![];
    for (i, (body, _)) in lines.lines.iter().enumerate() {
        if body.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let links = |t: &String| normal(folder.iter().cloned().chain(parts(t)).collect()) == want;
        if !fenced && link_targets(body).iter().any(links) {
            out.push(i);
        }
    }
    out
}

/// `text`, the file `removal` edits, with its line or its `@`s taken out, and what Undo needs.
/// `home` is the home folder, for an import from `~/`. A memory file's line comes out whole, and is
/// refused, with the row's words, when it isn't there exactly once. An import's `@` comes off every
/// import naming the file, on any line, as the reader finds them (`context::import_lines`: at a
/// line's start or after a space, outside code spans and fences), the rest of each line as it was;
/// refused only when none names it
pub fn take_out(removal: &Removal, text: &str, home: &str) -> Result<(String, Removed), String> {
    let mut lines = Lines::parse(text);
    let path = edited(removal);
    let name = name_of(path);
    match removal {
        Removal::MemoryLine { index, file } => {
            let found = memory_lines(index, file, &lines);
            let at = match found.len() {
                0 => return Err(format!("Its line isn't in {name} any more")),
                1 => found[0],
                n => return Err(format!("{name} links to it on {n} lines: take out the one you mean by hand")),
            };
            let before = at.checked_sub(1).map(|j| lines.lines[j].0.clone());
            let after = lines.lines.get(at + 1).map(|(b, _)| b.clone());
            let (line, mut ending) = lines.lines.remove(at);
            if ending.is_empty() {
                // The last line, with no ending: the one before it gives up its own, so the file
                // still ends without one, and Undo gives it back
                ending = match lines.lines.last_mut() {
                    Some(last) => std::mem::take(&mut last.1),
                    None => "\n".to_string(),
                };
            }
            Ok((lines.text(), Removed::Line(Line { file: path.to_string(), line, ending, at, before, after })))
        }
        Removal::Import { importer, file } => {
            let body = text.strip_prefix('\u{feff}').unwrap_or(text);
            let want = parts(file);
            let mut marked: Vec<Marked> = vec![];
            for (at, byte, named) in import_lines(importer, body, Some(&parts(home))) {
                if named != want {
                    continue;
                }
                // import_lines counts lines as str::lines does, and Lines as their endings split
                // them, which is the same: its byte is the line's `@`
                debug_assert_eq!(lines.lines[at].0.as_bytes()[byte], b'@');
                match marked.last_mut() {
                    Some(m) if m.at == at => m.bytes.push(byte),
                    _ => marked.push(Marked { at, was: lines.lines[at].0.clone(), bytes: vec![byte] }),
                }
            }
            if marked.is_empty() {
                return Err(format!("No @ in {name} imports it any more"));
            }
            for m in &marked {
                lines.lines[m.at].0 = m.left();
            }
            Ok((lines.text(), Removed::Marks(Marks { file: path.to_string(), lines: marked })))
        }
    }
}

/// `text`, the file `removed` came out of as it is now, with what Remove took out put back: a
/// memory file's line by [`put_line_back`], an import's `@`s by [`put_marks_back`].
/// [`Placed::Already`], and `text` unchanged, when it's there already; refused, with the row's
/// words, only when an import's line has changed past finding
pub fn put_back(text: &str, removed: &Removed) -> Result<(String, Placed), String> {
    match removed {
        Removed::Line(line) => Ok(put_line_back(text, line)),
        Removed::Marks(marks) => put_marks_back(text, marks),
    }
}

/// A memory file's line put back: at its old number when the lines either side are still the ones
/// it was between there, else after the line it followed when that's there once, else at the
/// file's end
fn put_line_back(text: &str, removed: &Line) -> (String, Placed) {
    let mut lines = Lines::parse(text);
    if lines.lines.iter().any(|(b, _)| *b == removed.line) {
        return (text.to_string(), Placed::Already);
    }
    let n = lines.lines.len();
    let body = |i: usize| lines.lines.get(i).map(|(b, _)| b);
    let before_ok = match (removed.at.checked_sub(1), &removed.before) {
        (None, None) => true,
        (Some(j), Some(b)) => body(j) == Some(b),
        _ => false,
    };
    let after_ok = body(removed.at) == removed.after.as_ref();
    let (at, placed) = if removed.at <= n && before_ok && after_ok {
        (removed.at, Placed::Where)
    } else if let Some(j) = removed.before.as_ref().and_then(|b| lines.only(b)) {
        (j + 1, Placed::After)
    } else {
        (n, Placed::End)
    };
    let ending = if removed.ending.is_empty() { lines.ending() } else { removed.ending.clone() };
    let line = removed.line.clone();
    if at == lines.lines.len() {
        // At the end: a last line without an ending gets one, and this one ends as the file did
        match lines.lines.last_mut() {
            Some(last) if last.1.is_empty() => {
                last.1 = ending;
                lines.lines.push((line, String::new()));
            }
            _ => lines.lines.push((line, ending)),
        }
    } else {
        lines.lines.insert(at, (line, ending));
    }
    (lines.text(), placed)
}

/// An import's `@`s put back, each line's in turn: on the line as Remove left it at its old number
/// ([`Placed::Where`]); left as it is when the line as it was is there, or the path is imported
/// again (put back by hand); on the line as Remove left it where that is now, when it's in the file
/// once; else, for a line with one `@` off it, before its path where that stands as plain text once
/// in the file (the line's words changed since; [`Placed::Moved`] for either). Refused, writing
/// nothing, when a line can't be found any of those ways
fn put_marks_back(text: &str, marks: &Marks) -> Result<(String, Placed), String> {
    let mut lines = Lines::parse(text);
    let name = name_of(&marks.file);
    let changed = || format!("Its import's line in {name} has changed: put the @ back by hand");
    let mut placed = Placed::Already;
    for m in &marks.lines {
        let left = m.left();
        let here = lines.lines.get(m.at).map(|(b, _)| b.as_str());
        let (at, line, how) = if here == Some(left.as_str()) {
            (m.at, m.was.clone(), Placed::Where)
        } else if here == Some(m.was.as_str()) || lines.only(&m.was).is_some() {
            continue;
        } else if let Some(j) = lines.only(&left) {
            (j, m.was.clone(), Placed::Moved)
        } else {
            let path = m.path().ok_or_else(changed)?;
            let (plain, imported) = path_words(&lines, path);
            if imported {
                continue;
            }
            let [(j, k)] = plain[..] else {
                return Err(changed());
            };
            let b = &lines.lines[j].0;
            (j, format!("{}@{}", &b[..k], &b[k..]), Placed::Moved)
        };
        lines.lines[at].0 = line;
        placed = if placed == Placed::Moved || how == Placed::Moved { Placed::Moved } else { Placed::Where };
    }
    match placed {
        Placed::Already => Ok((text.to_string(), Placed::Already)),
        _ => Ok((lines.text(), placed)),
    }
}

/// Where `path` stands as a word of its own (at a line's start or after a space, with a space or
/// the line's end after it) outside code fences: each place it's plain text, as line and byte, and
/// whether it's imported anywhere (an `@` before it, itself at a line's start or after a space)
fn path_words(lines: &Lines, path: &str) -> (Vec<(usize, usize)>, bool) {
    let mut plain = vec![];
    let mut imported = false;
    let mut fenced = false;
    for (i, (body, _)) in lines.lines.iter().enumerate() {
        if body.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for (k, _) in body.match_indices(path) {
            if !body[k + path.len()..].chars().next().map_or(true, char::is_whitespace) {
                continue;
            }
            match body[..k].chars().next_back() {
                None => plain.push((i, k)),
                Some(c) if c.is_whitespace() => plain.push((i, k)),
                Some('@') if body[..k - 1].chars().next_back().map_or(true, char::is_whitespace) => imported = true,
                Some(_) => {}
            }
        }
    }
    (plain, imported)
}

/// `path`'s text, or the row's words for why not
fn read(path: &str) -> Result<String, String> {
    let name = name_of(path);
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{name} isn't there any more"),
        _ => format!("Couldn't read {name}: {e}"),
    })?;
    String::from_utf8(bytes).map_err(|_| format!("{name} isn't UTF-8 text"))
}

/// Writes `text` to a file beside `path` and renames it over `path` (on Windows, `MoveFileExW`
/// replacing it), so a reader sees the old file or the new one, never half of one. The file beside
/// it goes if the rename fails
pub fn write_over(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let beside = path.with_file_name(format!(".{name}.lbterm-{}.tmp", std::process::id()));
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&beside);
    })
}

/// Does `act` for `removal`'s section, reading and writing its file: the state it leaves the
/// section in. Off the GUI thread
pub fn run(act: &Act, removal: &Removal, home: &str) -> State {
    match act {
        Act::Remove => {
            let path = edited(removal);
            let done = read(path).and_then(|text| take_out(removal, &text, home)).and_then(|(text, removed)| {
                write_over(Path::new(path), &text).map_err(|e| format!("Couldn't write {}: {e}", name_of(path)))?;
                Ok(removed)
            });
            match done {
                Ok(removed) => State::Removed(removed),
                Err(why) => State::Refused { why, removed: None },
            }
        }
        Act::Undo(removed) => {
            let path = removed.file();
            let done = read(path).and_then(|text| {
                let (new, placed) = put_back(&text, removed)?;
                if placed != Placed::Already {
                    write_over(Path::new(path), &new).map_err(|e| format!("Couldn't write {}: {e}", name_of(path)))?;
                }
                Ok(placed)
            });
            match done {
                Ok(placed) => State::Back(placed),
                Err(why) => State::Refused { why, removed: Some(removed.clone()) },
            }
        }
    }
}

/// Whether Compact now may type `/compact` into a pane showing `screen`, its Claude Code `working`
/// or not, or the words for why not: only at Claude Code's prompt with nothing typed, as a voice
/// draft's send looks (crate::drafts::bottom), so it neither lands in a dialog nor joins what he's
/// typing. `working` is the pane's `claude_status`, which Terminal's hooks set for a turn
/// (UserPromptSubmit) and for a compaction (PreCompact, a /compact typed by hand included, then
/// PostCompact putting back what it said before: Terminal's `claudehook compact`, Lachlan's E9 of
/// 2026-11-09), so the screen isn't read for it
pub fn may_compact(working: bool, screen: &Screen) -> Result<(), &'static str> {
    if working {
        return Err("Claude is working");
    }
    let Bottom::Prompt(rows) = bottom(screen) else {
        return Err("A dialog has the box's place");
    };
    let blank = |c: char| is_space(c) || c == '\u{2502}';
    for (i, row) in screen.rows[rows].iter().enumerate() {
        let text: String = row.iter().map(|g| g.ch).collect();
        let text = text.trim_matches(blank);
        // The first row starts with the prompt mark, which bottom() found
        let text = if i == 0 { text.trim_start_matches(['\u{276f}', '>']) } else { text };
        if !text.trim_matches(blank).is_empty() {
            return Err("Something is typed in the box");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::questions::Glyph;

    const INDEX: &str = "C:\\Users\\someone\\.claude\\projects\\C--work\\memory\\MEMORY.md";
    const FILE: &str = "C:\\Users\\someone\\.claude\\projects\\C--work\\memory\\feedback_b.md";

    fn memory() -> Removal {
        Removal::MemoryLine { index: INDEX.to_string(), file: FILE.to_string() }
    }

    /// A memory file's line that Remove took out
    fn line(removed: &Removed) -> &Line {
        match removed {
            Removed::Line(l) => l,
            Removed::Marks(_) => panic!("an import's marks, not a line"),
        }
    }

    /// Undo's text and where it put it back, when it isn't refused
    fn back(text: &str, removed: &Removed) -> (String, Placed) {
        put_back(text, removed).unwrap()
    }

    #[test]
    fn a_memory_files_line_comes_off_its_index_and_only_that_line() {
        let text = "# Memory\r\n- [A](feedback_a.md) — a\r\n- [B](feedback_b.md) — b\r\n- [Also B, said apart](feedback_b2.md) — c\r\n";
        let (new, removed) = take_out(&memory(), text, "C:/Users/someone").unwrap();
        assert_eq!(new, "# Memory\r\n- [A](feedback_a.md) — a\r\n- [Also B, said apart](feedback_b2.md) — c\r\n");
        assert_eq!(line(&removed).line, "- [B](feedback_b.md) — b");
        assert_eq!((line(&removed).at, line(&removed).ending.as_str()), (2, "\r\n"));
        assert_eq!(line(&removed).before.as_deref(), Some("- [A](feedback_a.md) — a"));
        // Undo with nothing changed since gives the file back byte for byte
        assert_eq!(back(&new, &removed), (text.to_string(), Placed::Where));
        // A link with ./, any case, an anchor or angle brackets is the same file
        for line in ["- [B](./Feedback_B.md#why)", "- [B](<feedback_b.md> \"title\")"] {
            let text = format!("x\n{line}\ny\n");
            assert_eq!(take_out(&memory(), &text, "").unwrap().0, "x\ny\n", "{line}");
        }
    }

    #[test]
    fn each_line_keeps_its_own_ending_and_a_byte_order_mark_stays() {
        let text = "\u{feff}one\nB: [b](feedback_b.md)\r\nthree\n";
        let (new, removed) = take_out(&memory(), text, "").unwrap();
        assert_eq!(new, "\u{feff}one\nthree\n");
        assert_eq!(back(&new, &removed).0, text);
    }

    #[test]
    fn a_last_line_without_an_ending_leaves_the_file_still_without_one() {
        let text = "one\r\n[b](feedback_b.md)";
        let (new, removed) = take_out(&memory(), text, "").unwrap();
        assert_eq!(new, "one");
        assert_eq!(line(&removed).ending, "\r\n");
        assert_eq!(back(&new, &removed), (text.to_string(), Placed::Where));
        // The only line
        let (new, removed) = take_out(&memory(), "[b](feedback_b.md)", "").unwrap();
        assert_eq!(new, "");
        assert_eq!(back(&new, &removed).0, "[b](feedback_b.md)\n");
    }

    #[test]
    fn a_file_changed_since_the_conversation_loaded_it_still_loses_the_right_line() {
        // Lines added above it since: found where it is now
        let text = "# Memory\nnew one\nnew two\n- [A](feedback_a.md)\n- [B](feedback_b.md)\n- [C](feedback_c.md)\n";
        let (new, removed) = take_out(&memory(), text, "").unwrap();
        assert_eq!(new, "# Memory\nnew one\nnew two\n- [A](feedback_a.md)\n- [C](feedback_c.md)\n");
        assert_eq!(line(&removed).at, 4);
        // Changed again before Undo: a line in above it moves its number, so it goes back after
        // the line it followed
        let (again, placed) = back("# Memory\nnewer\nnew one\nnew two\n- [A](feedback_a.md)\n- [C](feedback_c.md)\n", &removed);
        assert_eq!(placed, Placed::After);
        assert_eq!(again, "# Memory\nnewer\nnew one\nnew two\n- [A](feedback_a.md)\n- [B](feedback_b.md)\n- [C](feedback_c.md)\n");
        // The line it followed gone too: at the end
        let (again, placed) = back("# Memory\n- [C](feedback_c.md)\n", &removed);
        assert_eq!((again.as_str(), placed), ("# Memory\n- [C](feedback_c.md)\n- [B](feedback_b.md)\n", Placed::End));
        // Put back by hand since: nothing to do
        assert_eq!(back(text, &removed), (text.to_string(), Placed::Already));
    }

    #[test]
    fn an_index_line_thats_gone_already_or_there_twice_is_refused_with_why() {
        let gone = take_out(&memory(), "# Memory\n- [A](feedback_a.md)\n", "").unwrap_err();
        assert_eq!(gone, "Its line isn't in MEMORY.md any more");
        let twice = take_out(&memory(), "[b](feedback_b.md)\nagain [b](feedback_b.md)\n", "").unwrap_err();
        assert_eq!(twice, "MEMORY.md links to it on 2 lines: take out the one you mean by hand");
        // In a code fence it's an example, not a link
        let fenced = "```\n[b](feedback_b.md)\n```\n[b](feedback_b.md)\n";
        assert_eq!(take_out(&memory(), fenced, "").unwrap().0, "```\n[b](feedback_b.md)\n```\n");
    }

    fn import(importer: &str, file: &str) -> Removal {
        Removal::Import { importer: importer.to_string(), file: file.to_string() }
    }

    /// Whether the reader finds `file` imported in `text`, the importer's
    fn imported(importer: &str, text: &str, file: &str) -> bool {
        let home = parts("C:/Users/someone");
        crate::context::import_lines(importer, text, Some(&home)).iter().any(|(_, _, named)| *named == parts(file))
    }

    #[test]
    fn an_imports_at_comes_off_on_any_line_the_words_and_the_path_staying() {
        let importer = "C:/Users/someone/.claude/CLAUDE.md";
        let home = "C:\\Users\\someone";
        let text = "# Global\r\n\r\n@C:/work/rules/asking.md\r\n- @~/notes/later.md\r\n\r\nSee @C:/work/rules/other.md for more.\r\n";
        // A line that's only the import: its path stays as a plain line, and nothing else changes
        let (new, removed) = take_out(&import(importer, "C:\\work\\rules\\asking.md"), text, home).unwrap();
        assert_eq!(new, text.replace("\n@C:/work/rules/asking.md", "\nC:/work/rules/asking.md"));
        assert!(!imported(importer, &new, "C:/work/rules/asking.md"), "the reader still finds it imported");
        assert!(imported(importer, &new, "C:/work/rules/other.md"));
        assert_eq!(back(&new, &removed), (text.to_string(), Placed::Where));
        // From ~/, after a list's bullet
        let (new, _) = take_out(&import(importer, "C:/Users/someone/notes/later.md"), text, home).unwrap();
        assert_eq!(new, text.replace("- @~/", "- ~/"));
        // On a line with words round it: only the @ comes off, the words and the path staying
        let (new, removed) = take_out(&import(importer, "C:/work/rules/other.md"), text, home).unwrap();
        assert_eq!(new, text.replace("See @C:", "See C:"));
        assert!(!imported(importer, &new, "C:/work/rules/other.md"));
        let Removed::Marks(marks) = &removed else { panic!("a line, not marks") };
        assert_eq!((marks.lines.len(), marks.lines[0].at, marks.lines[0].bytes.as_slice()), (1, 5, &[4][..]));
        assert_eq!(marks.lines[0].left(), "See C:/work/rules/other.md for more.");
        assert_eq!(back(&new, &removed), (text.to_string(), Placed::Where));
        // Named by no import: refused, with why
        let gone = take_out(&import(importer, "C:/work/rules/gone.md"), text, home).unwrap_err();
        assert_eq!(gone, "No @ in CLAUDE.md imports it any more");
        // A path in a code span or a fence isn't an import, so its @ stays
        let ticks = "Write `@C:/work/rules/other.md` to import it.\n```\n@C:/work/rules/other.md\n```\nThen @C:/work/rules/other.md\n";
        let (new, _) = take_out(&import(importer, "C:/work/rules/other.md"), ticks, home).unwrap();
        assert_eq!(new, ticks.replace("Then @", "Then "));
        // A byte order mark, and a last line without an ending, stay
        let bom = "\u{feff}x\nSee @C:/work/rules/other.md";
        let (new, removed) = take_out(&import(importer, "C:/work/rules/other.md"), bom, home).unwrap();
        assert_eq!(new, "\u{feff}x\nSee C:/work/rules/other.md");
        assert_eq!(back(&new, &removed).0, bom);
    }

    #[test]
    fn every_import_naming_the_file_comes_off_and_undo_puts_each_back() {
        let importer = "C:/w/CLAUDE.md";
        let text = "@a.md and @./a.md again\nthen @b.md\n- @a.md\n";
        let (new, removed) = take_out(&import(importer, "C:/w/a.md"), text, "").unwrap();
        assert_eq!(new, "a.md and ./a.md again\nthen @b.md\n- a.md\n");
        assert!(!imported(importer, &new, "C:/w/a.md"));
        assert_eq!(back(&new, &removed), (text.to_string(), Placed::Where));
    }

    #[test]
    fn undo_puts_the_at_back_where_its_line_is_now_or_before_its_path() {
        let importer = "C:/w/CLAUDE.md";
        let removal = import(importer, "C:/w/rules/r.md");
        let text = "# Work\nRead @rules/r.md before you ask.\nMore.\n";
        let (new, removed) = take_out(&removal, text, "").unwrap();
        assert_eq!(new, "# Work\nRead rules/r.md before you ask.\nMore.\n");
        // A line in above it since: found by its words where it is now
        let (again, placed) = back("# Work\nNew.\nRead rules/r.md before you ask.\nMore.\n", &removed);
        assert_eq!((again.as_str(), placed), ("# Work\nNew.\nRead @rules/r.md before you ask.\nMore.\n", Placed::Moved));
        // Its words changed since: before its path, the one place it stands as plain text
        let (again, placed) = back("# Work\nAlways read rules/r.md first.\nMore.\n", &removed);
        assert_eq!((again.as_str(), placed), ("# Work\nAlways read @rules/r.md first.\nMore.\n", Placed::Moved));
        // Put back by hand, as it was or on a changed line: nothing to do
        assert_eq!(back(text, &removed), (text.to_string(), Placed::Already));
        assert_eq!(back("# Work\nAlways read @rules/r.md first.\n", &removed).1, Placed::Already);
        // Its path gone, there twice as plain text, or only inside another word: refused, with why
        let why = "Its import's line in CLAUDE.md has changed: put the @ back by hand";
        for changed in ["# Work\nMore.\n", "Read rules/r.md\nand rules/r.md\n", "Read myrules/r.md now\n"] {
            assert_eq!(put_back(changed, &removed), Err(why.to_string()), "{changed}");
        }
        let state = State::Removed(removed.clone());
        assert_eq!((state.button(), state.note(&removal).as_str()), ("Undo", "Import off in CLAUDE.md: a compaction can attach it again"));
        assert_eq!(State::Back(Placed::Moved).note(&removal), "Import back on in CLAUDE.md");
        assert_eq!(State::Back(Placed::Already).note(&removal), "Its @ was back in CLAUDE.md already");
        assert_eq!(State::PuttingBack(removed).note(&removal), "Putting its @ back in CLAUDE.md…");
    }

    #[test]
    fn an_imports_at_comes_off_and_goes_back_in_the_file_itself() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("CLAUDE.md");
        let text = "\u{feff}# Work\r\nRead @rules/r.md before you ask.\r\nMore.";
        std::fs::write(&md, text).unwrap();
        let removal = Removal::Import {
            importer: md.to_string_lossy().to_string(),
            file: dir.path().join("rules").join("r.md").to_string_lossy().to_string(),
        };
        let State::Removed(removed) = run(&Act::Remove, &removal, "") else {
            panic!("not removed");
        };
        assert_eq!(std::fs::read_to_string(&md).unwrap(), text.replace("Read @", "Read "));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "something left beside it");
        assert_eq!(run(&Act::Remove, &removal, ""), State::Refused { why: "No @ in CLAUDE.md imports it any more".into(), removed: None });
        assert_eq!(run(&Act::Undo(removed.clone()), &removal, ""), State::Back(Placed::Where));
        assert_eq!(std::fs::read_to_string(&md).unwrap(), text);
        // Its line taken out by hand: Undo refused, writing nothing, and still offered
        std::fs::write(&md, "# Work\r\nMore.").unwrap();
        let refused = run(&Act::Undo(removed.clone()), &removal, "");
        assert_eq!(refused.press(), Some(Act::Undo(removed)));
        assert_eq!(refused.note(&removal), "Its import's line in CLAUDE.md has changed: put the @ back by hand");
        assert_eq!(std::fs::read_to_string(&md).unwrap(), "# Work\r\nMore.");
    }

    #[test]
    fn the_files_are_written_whole_beside_and_renamed_over() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("MEMORY.md");
        std::fs::write(&index, "# Memory\r\n- [B](feedback_b.md)\r\n- [C](feedback_c.md)\r\n").unwrap();
        let removal = Removal::MemoryLine {
            index: index.to_string_lossy().to_string(),
            file: dir.path().join("feedback_b.md").to_string_lossy().to_string(),
        };
        let State::Removed(removed) = run(&Act::Remove, &removal, "") else {
            panic!("not removed");
        };
        assert_eq!(std::fs::read_to_string(&index).unwrap(), "# Memory\r\n- [C](feedback_c.md)\r\n");
        // Nothing left beside it
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // A second Remove finds the line gone: refused, and the row says why
        let again = run(&Act::Remove, &removal, "");
        assert_eq!(again, State::Refused { why: "Its line isn't in MEMORY.md any more".to_string(), removed: None });
        assert_eq!(again.button(), "Remove");
        let state = State::Removed(removed.clone());
        assert_eq!((state.button(), state.note(&removal).as_str()), ("Undo", "Off MEMORY.md: a compaction can attach it again"));
        assert_eq!(run(&Act::Undo(removed.clone()), &removal, ""), State::Back(Placed::Where));
        assert_eq!(std::fs::read_to_string(&index).unwrap(), "# Memory\r\n- [B](feedback_b.md)\r\n- [C](feedback_c.md)\r\n");
        // The index gone: Undo refused, the line still out, Undo still offered
        std::fs::remove_file(&index).unwrap();
        let refused = run(&Act::Undo(removed.clone()), &removal, "");
        assert_eq!(refused, State::Refused { why: "MEMORY.md isn't there any more".to_string(), removed: Some(removed.clone()) });
        assert_eq!(refused.press(), Some(Act::Undo(removed)));
        assert_eq!(State::Removing.press(), None);
    }

    fn screen(rows: &[&str]) -> Screen {
        Screen {
            rows: rows
                .iter()
                .map(|r| r.chars().enumerate().map(|(col, ch)| Glyph { col, ch, width: 1 }).collect())
                .collect(),
        }
    }

    #[test]
    fn compact_now_only_at_an_empty_prompt_and_never_while_claude_works() {
        let rule = "─".repeat(30);
        let at = |line: &str| screen(&["● Done.", "", &rule, line, &rule, "  status line"]);
        assert_eq!(may_compact(false, &at("❯ ")), Ok(()));
        assert_eq!(may_compact(true, &at("❯ ")), Err("Claude is working"));
        assert_eq!(may_compact(false, &at("❯ half a thou")), Err("Something is typed in the box"));
        let dialog = screen(&["● Done.", &rule, " Do you want to proceed?", " ❯ 1. Yes", &rule]);
        assert_eq!(may_compact(false, &dialog), Err("A dialog has the box's place"));
        // A second row typed with Shift+Enter
        let two = screen(&["● Done.", &rule, "❯ ", "  more", &rule]);
        assert_eq!(may_compact(false, &two), Err("Something is typed in the box"));
        // Compacting: Claude Code's working line over the box, and PreCompact's hook saying working
        let compacting = screen(&["● Done.", "✻ Compacting conversation… (esc to interrupt)", "", &rule, "❯ ", &rule, "  status line"]);
        assert_eq!(may_compact(true, &compacting), Err("Claude is working"));
    }

    #[test]
    fn a_removal_whose_section_went_is_listed_off_the_bar_with_its_undo() {
        use crate::context_bar::{Section, What};
        let removed = Removed::Line(Line { file: INDEX.into(), line: "- [B](feedback_b.md)".into(), ending: "\r\n".into(), at: 2, before: None, after: None });
        let import = Removal::Import { importer: "C:/u/.claude/CLAUDE.md".into(), file: "C:/u/rules/a.md".into() };
        let pressed = vec![(memory(), State::Removed(removed.clone())), (import.clone(), State::Removed(removed.clone()))];
        let section = |removal: Option<Removal>| Section { what: What::Import, path: Some("p".into()), tokens: 5, removal, note: None };
        // The import is still on the bar: only the memory file is off it
        let bar = Bar { sections: vec![section(None), section(Some(import.clone()))], in_use: 10, window: 100, estimate: false };
        let off = off_the_bar(&bar, &pressed);
        assert_eq!(off.len(), 1);
        assert_eq!(off[0].0, memory());
        assert_eq!((memory().what(), memory().file()), (What::Memory, FILE));
        assert_eq!((import.what(), import.file()), (What::Import, "C:/u/rules/a.md"));
        // Its row: Undo, and that Undo puts it back, where a section on the bar says a compaction
        // can attach it again
        assert_eq!(off[0].1.button(), "Undo");
        assert_eq!(off[0].1.gone_note(&memory()), "Off MEMORY.md: Undo puts it back");
        assert_eq!(State::Removed(removed.clone()).gone_note(&import), "Import off in CLAUDE.md: Undo puts its @ back");
        assert_eq!(State::Back(Placed::Where).gone_note(&memory()), "Back in MEMORY.md");
        // After a compaction neither is on the bar; one refused with nothing out isn't listed
        let bare = Bar { sections: vec![section(None)], in_use: 10, window: 100, estimate: false };
        assert_eq!(off_the_bar(&bare, &pressed).len(), 2);
        let refused = vec![(memory(), State::Refused { why: "gone".into(), removed: None }), (import, State::Back(Placed::Moved))];
        let off = off_the_bar(&bare, &refused);
        assert_eq!(off.len(), 1);
        assert_eq!(off[0].1.gone_note(&off[0].0), "Import back on in CLAUDE.md");
    }

    #[test]
    fn a_removed_file_attached_again_after_a_summary_is_its_rows_section_with_its_undo() {
        let removed = Removed::Line(Line { file: INDEX.into(), line: "- [B](feedback_b.md)".into(), ending: "\n".into(), at: 2, before: None, after: None });
        let import = Removal::Import { importer: "C:/u/.claude/CLAUDE.md".into(), file: "C:/u/rules/a.md".into() };
        let pressed = vec![(import.clone(), State::Removed(removed.clone()))];
        // Claude Code writes the path its own way: the same file, by its parts
        let attached = Section { what: What::Attached, path: Some("C:\\u\\rules\\A.md".into()), tokens: 5, removal: None, note: Some(crate::context_bar::ATTACHED) };
        assert_eq!(section_removal(&attached, &pressed), Some(&import));
        let bar = Bar { sections: vec![attached.clone()], in_use: 10, window: 100, estimate: false };
        assert!(off_the_bar(&bar, &pressed).is_empty(), "on the bar again, so not listed off it");
        assert_eq!(pressed[0].1.attached_note(&import), "Attached again after the summary, @ off in CLAUDE.md");
        assert_eq!(pressed[0].1.note(&import), "Import off in CLAUDE.md: a compaction can attach it again");
        assert_eq!(State::Back(Placed::Where).attached_note(&import), "Import back on in CLAUDE.md");
        // A memory file's: its line is off the index, and the file attached again all the same
        assert_eq!(State::Removed(removed.clone()).attached_note(&memory()), "Attached again after the summary, off MEMORY.md");
        // No row's words say anything takes it out of the conversation: nothing does
        for state in [State::Removed(removed.clone()), State::Removing, State::PuttingBack(removed.clone())] {
            for removal in [memory(), import.clone()] {
                for note in [state.note(&removal), state.attached_note(&removal), state.gone_note(&removal)] {
                    assert!(!note.contains("takes it out") && !note.contains("Compact now"), "{note}");
                }
            }
        }
        // Another file attached again has no removal; nor has a section of another kind by path
        let other = Section { path: Some("C:/u/rules/b.md".into()), ..attached.clone() };
        assert_eq!(section_removal(&other, &pressed), None);
        let loaded = Section { what: What::Project, ..attached };
        assert_eq!(section_removal(&loaded, &pressed), None);
    }
}
