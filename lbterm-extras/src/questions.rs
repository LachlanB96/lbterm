//! Question colours (lbterm/QUESTIONS.md). While the right button is held on a Claude pane,
//! each question Claude's reply asks gets a colour, and every passage of the reply it's about
//! is highlighted in the same one; a question with a part scrolled out of view gets a marker at
//! that edge, pointing the way to it.
//!
//! Terminal's question worker (`shared/claude/question-worker.ts`, started by `claudehook stop`)
//! has Haiku find the questions and passages after each reply that asks something, and writes
//! them to `<sid>.questions.json` in the summaries folder, which [`parse`] reads. [`lay_out`]
//! finds them on the pane's screen, which is all a terminal has of Claude Code's fullscreen
//! conversation: the reply as Claude Code drew it, its markdown gone and its lines wrapped to the
//! pane. So quotes are matched by their words alone, letters and digits, and each is coloured
//! as the whole bullet or paragraph it's in.

use serde::Deserialize;

pub mod pane;
pub mod weights;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;

/// Each question's colour, by its place in the reply: Q1's first
pub const COLOURS: [u32; 6] = [0x5AA9FF, 0x4DD38A, 0xF5B841, 0xB98CFF, 0x3CCFCF, 0xFF7AB6];
/// The fewest words a passage is matched on, and a question: its label and a word will do
pub const MIN_WORDS: usize = 3;
pub const MIN_QUESTION_WORDS: usize = 2;
/// How many of a question's first words are matched
pub const QUESTION_WORDS: usize = 12;
/// The fewest words of a quote cut off by the screen's edge that still count as it
const MIN_EDGE_WORDS: usize = 5;
/// How near the edge a cut-off quote has to be: Claude Code's header can be over the top rows
const EDGE_ROWS: usize = 4;
/// How far up from the bottom Claude Code's input box can reach, as a prompt's typed
const INPUT_ROWS_MAX: usize = 30;

/// The colour of the question at `index` in its reply
pub fn colour(index: usize) -> u32 {
    COLOURS[index % COLOURS.len()]
}

/// A passage a question is about, as the reply has it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Quote {
    pub text: String,
    /// Where it starts in the reply, in words
    #[serde(default)]
    pub pos: usize,
}

/// A question a reply asks.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Question {
    /// As the reply labels it ("Q1"), or empty
    #[serde(default)]
    pub label: String,
    /// The number its options carry (1 for 1a, 1b), if it has one
    #[serde(default)]
    pub number: Option<u32>,
    /// Its first words, as the reply has them, starting with its label
    pub text: String,
    /// Where it starts in the reply, in words
    #[serde(default)]
    pub pos: usize,
    /// The passages it's about
    #[serde(default)]
    pub about: Vec<Quote>,
}

impl Question {
    /// What its marker says: its label, else Q and its number, else Q and its place
    fn name(&self, index: usize) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        format!("Q{}", self.number().unwrap_or(index as u32 + 1))
    }

    /// The number its options carry: as the worker gave it, else the label's
    pub fn number(&self) -> Option<u32> {
        self.number.or_else(|| {
            let digits: String = self.label.chars().filter(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
    }

    /// It's an extra offered after the questions, labelled on from them: E4
    pub fn is_extra(&self) -> bool {
        let mut chars = self.label.chars();
        chars.next() == Some('E') && chars.next().is_some_and(|c| c.is_ascii_digit())
    }
}

/// One reply's questions.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub id: String,
    /// When the worker wrote it, in Unix milliseconds: after the reply ended, 5 to 20 s after
    /// for the most part (lbterm/FLOWS.md, "A right hold, to the question colours", What could
    /// go 1); 0 when the file doesn't say
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub questions: Vec<Question>,
}

/// A session's `<sid>.questions.json`: its last replies that asked something.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Map {
    /// Oldest first
    #[serde(default)]
    pub messages: Vec<Message>,
}

/// A `<sid>.questions.json` as the worker wrote it, or `None` if it isn't one.
pub fn parse(text: &str) -> Option<Map> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// A text's words, as they're matched: its runs of letters and digits, lowercased. Markdown,
/// quote marks, bullets and where the lines wrap all fall away.
pub fn words(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            word.extend(ch.to_lowercase());
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// One cell of the screen: its column, its character, and how many columns it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Glyph {
    pub col: usize,
    pub ch: char,
    pub width: usize,
}

/// The pane's screen: a row of glyphs a line, top first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screen {
    pub rows: Vec<Vec<Glyph>>,
}

impl Screen {
    /// A screen of plain text, a column a character
    pub fn from_lines(lines: &[&str]) -> Self {
        Self {
            rows: lines
                .iter()
                .map(|line| {
                    line.chars()
                        .enumerate()
                        .map(|(col, ch)| Glyph { col, ch, width: 1 })
                        .collect()
                })
                .collect(),
        }
    }
}

/// A run of rows coloured for one or more questions.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    /// The screen rows it covers
    pub rows: Range<usize>,
    /// Its columns: from its rows' first glyph to the end of the longest
    pub cols: Range<usize>,
    /// The questions it goes with, by their place in the reply ([`colour`]); the first fills it
    pub colours: Vec<usize>,
    /// It's a question, or one of a question's options
    pub question: bool,
}

/// A question with a part off the screen, and which way that is.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// Its place in the reply ([`colour`])
    pub colour: usize,
    /// "Q5"
    pub label: String,
    /// Above the screen; else below it
    pub up: bool,
    /// It's in the newest reply laid out
    pub newest: bool,
}

/// Where a question's own words are against the screen, for the questions bar's arrow (Lachlan's
/// E15 of 2026-11-14: a question's block in the bar says which way to scroll to it)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// Found on the screen, or among what of its reply is (no arrow)
    On,
    Above,
    Below,
    /// Nothing of any reply in the file is on the screen, so which way isn't known
    Unknown,
}

impl Where {
    /// The arrow the bar draws: up, down, or none
    pub fn arrow(self) -> Option<bool> {
        match self {
            Where::Above => Some(true),
            Where::Below => Some(false),
            Where::On | Where::Unknown => None,
        }
    }
}

/// One question of the file and where its own words are ([`Where`]), by its reply's id and its
/// place in the reply, as the questions bar's rows name it (crate::questions::pane::Row)
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub reply: String,
    pub colour: usize,
    pub at: Where,
}

/// Where a question's own words are on the screen: a voice draft that answers it ticks it there
/// (lbterm/CONTROL.md, "Drafts"), and rings the next one it hasn't.
#[derive(Debug, Clone, PartialEq)]
pub struct Asked {
    /// Its place in the reply ([`colour`])
    pub colour: usize,
    /// The number its options carry
    pub number: Option<u32>,
    /// The row its words start on
    pub row: usize,
    /// The rows of its block and its options' after it
    pub rows: Range<usize>,
    /// Their columns: from their first glyph to the end of the longest
    pub cols: Range<usize>,
    /// It's in the newest reply laid out
    pub newest: bool,
}

/// One of the newest reply's questions, as a voice draft goes through them
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// The number its options carry, which an answer starts with
    pub number: Option<u32>,
    /// An extra offered after the questions (E4)
    pub extra: bool,
    /// Where it starts in the reply, in words
    pub pos: usize,
    /// What its marker says
    pub label: String,
}

/// Where the next question a voice draft hasn't answered is
#[derive(Debug, Clone, PartialEq)]
pub enum Next<'a> {
    /// Its own words are on the screen
    On(&'a Asked),
    /// It's off the screen that way: the marker at that edge, one of the layout's or one of its
    /// own when none of it is on the screen
    Off(Marker),
}

/// What a held right button shows on one screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub bands: Vec<Band>,
    pub markers: Vec<Marker>,
    /// The questions whose own words are on the screen
    pub asked: Vec<Asked>,
    /// The conversation's rows, above Claude Code's input box: the markers go at its edges
    pub area: Range<usize>,
    /// The newest reply's questions in its order, on the screen or not
    pub newest: Vec<Ask>,
    /// What of the newest reply is on the screen, from where in it to where, in words
    pub shown: Option<Range<usize>>,
    /// Every question in the file, oldest reply first, and where its own words are: a reply
    /// with something on the screen tells by where each starts against what of it is there;
    /// one with nothing there is above the screen when a newer reply is on it, below when an
    /// older one is, else unknown
    pub places: Vec<Place>,
}

impl Layout {
    pub fn is_empty(&self) -> bool {
        self.bands.is_empty() && self.markers.is_empty()
    }

    /// The question in the newest reply a voice draft answering `answered` should answer next,
    /// by its place in the reply ([`colour`]): the first, in the reply's order, that it hasn't,
    /// extras (E4) among them only with `extras`. One with no number can't be answered by one,
    /// so it's passed by. None once every one's answered.
    pub fn next_to_answer(&self, answered: &[u32], extras: bool) -> Option<usize> {
        self.newest
            .iter()
            .position(|q| (extras || !q.extra) && q.number.is_some_and(|n| !answered.contains(&n)))
    }

    /// Where that question is: on the screen, or the edge it's off. None when every one's
    /// answered, or when nothing of the newest reply is on the screen to say which way it is.
    pub fn next(&self, answered: &[u32], extras: bool) -> Option<Next<'_>> {
        let colour = self.next_to_answer(answered, extras)?;
        if let Some(asked) = self.asked.iter().find(|a| a.newest && a.colour == colour) {
            return Some(Next::On(asked));
        }
        let shown = self.shown.as_ref()?;
        let ask = &self.newest[colour];
        let up = ask.pos < shown.start;
        if !up && ask.pos <= shown.end {
            // Among what's on the screen, but its words weren't found there
            return None;
        }
        let marker = self
            .markers
            .iter()
            .find(|m| m.newest && m.colour == colour && m.up == up)
            .cloned()
            .unwrap_or(Marker {
                colour,
                label: ask.label.clone(),
                up,
                newest: true,
            });
        Some(Next::Off(marker))
    }
}

/// A word on the screen, and the row it's on
struct Word {
    text: String,
    row: usize,
}

/// What a block needs to know of a row
#[derive(Debug, Clone, Default)]
struct RowInfo {
    blank: bool,
    /// The column of its first glyph that isn't a space
    indent: usize,
    /// The column after its last
    end: usize,
    /// It begins a block of its own: a bullet, a numbered item, a question's label, Claude
    /// Code's marks, a table's border
    starts: bool,
    /// Its first word
    first_word: Option<String>,
}

pub(crate) fn is_space(ch: char) -> bool {
    ch == ' ' || ch == '\u{a0}' || ch == '\0'
}

fn row_info(row: &[Glyph]) -> RowInfo {
    let mut ink = row.iter().filter(|g| !is_space(g.ch));
    let Some(first) = ink.next() else {
        return RowInfo {
            blank: true,
            ..Default::default()
        };
    };
    let last = row.iter().rev().find(|g| !is_space(g.ch)).unwrap_or(first);
    let text: String = row
        .iter()
        .skip_while(|g| is_space(g.ch))
        .map(|g| g.ch)
        .collect();
    RowInfo {
        blank: false,
        indent: first.col,
        end: last.col + last.width.max(1),
        starts: starts_block(&text),
        first_word: words(&text).into_iter().next(),
    }
}

/// Whether a row's text (from its first glyph) begins a block of its own
fn starts_block(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let second = chars.next();
    match first {
        // Bullets, which need the space after them
        '-' | '*' | '+' => second == Some(' '),
        // Bullets, Claude Code's marks before a message, your prompt and a tool's output, its
        // spinners, a quote's bar, and tables' borders
        '\u{2022}' | '\u{00b7}' | '\u{25cf}' | '\u{276f}' | '\u{23bf}' | '\u{273b}' | '\u{273d}'
        | '\u{2736}' | '\u{2733}' | '\u{2722}' | '\u{258e}' | '\u{2500}'..='\u{257f}' => true,
        // A question's label, Q1. Q12:, or an extra's numbered on from them, E4. (2026-10-25)
        'Q' | 'E' => {
            let rest: String = text[1..].chars().take_while(|c| !c.is_whitespace()).collect();
            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            digits > 0 && rest[digits..].chars().all(|c| !c.is_alphanumeric())
        }
        // A numbered item or an option: 4. 2) 1a. 3b)
        '0'..='9' => {
            let head: String = text.chars().take_while(|c| !c.is_whitespace()).collect();
            let digits = head.chars().take_while(char::is_ascii_digit).count();
            let rest = &head[digits..];
            let rest = rest
                .strip_prefix(|c: char| c.is_ascii_lowercase())
                .unwrap_or(rest);
            (rest == "." || rest == ")") && text.len() > head.len()
        }
        _ => false,
    }
}

/// The fewest of the line's glyphs a rule has
const RULE_MIN: usize = 20;

fn is_line(ch: char) -> bool {
    ch == '\u{2500}' || ch == '\u{2501}'
}

/// Whether a row's text is a rule Claude Code draws round its input box: a line across and
/// nothing else, or a named conversation's upper rule, which carries its name near the right end
/// (`──…── LBTerm InMail ─`, since 2.1.314): at least [`RULE_MIN`] of the line, a space, the
/// name, a space and the line to the end. The same shape as LyreBird's
/// `^\s*─{20,}(?: \S.*? ─+)?\s*$`.
pub fn is_rule_text(text: &str) -> bool {
    let t = text.trim_matches(|c: char| c.is_whitespace() || is_space(c));
    if t.chars().all(|c| is_line(c) || is_space(c)) {
        return t.chars().filter(|&c| is_line(c)).count() >= RULE_MIN;
    }
    if t.chars().take_while(|&c| is_line(c)).count() < RULE_MIN {
        return false;
    }
    let body = t.trim_start_matches(is_line);
    let name = body.trim_end_matches(is_line);
    name.len() < body.len()
        && name
            .strip_prefix(' ')
            .and_then(|n| n.strip_suffix(' '))
            .is_some_and(|n| n.starts_with(|c: char| !c.is_whitespace()) && n.ends_with(|c: char| !c.is_whitespace()))
}

/// A row Claude Code draws round its input box ([`is_rule_text`]). A row whose first ink isn't
/// the line is turned down before its text is put together.
pub(crate) fn is_rule(row: &[Glyph]) -> bool {
    if !row.iter().find(|g| !is_space(g.ch)).is_some_and(|g| is_line(g.ch)) {
        return false;
    }
    let text: String = row.iter().map(|g| if is_space(g.ch) { ' ' } else { g.ch }).collect();
    is_rule_text(&text)
}

/// Where the conversation ends: above Claude Code's input box, the lowest rule near the bottom
/// and the one above it, or the whole screen when there's no box
fn conversation_end(screen: &Screen) -> usize {
    let n = screen.rows.len();
    let floor = n.saturating_sub(INPUT_ROWS_MAX);
    let Some(lowest) = (floor..n).rev().find(|&r| is_rule(&screen.rows[r])) else {
        return n;
    };
    (floor..lowest)
        .rev()
        .find(|&r| is_rule(&screen.rows[r]))
        .unwrap_or(lowest)
}

/// A hash of all [`lay_out`] reads of `screen`: where the conversation ends, above Claude Code's
/// input box, and every row above that. A screen whose conversation is the same lays out the
/// same, however its input box and status line under it change, so a layout is kept until this
/// is another (wezterm-gui's render/questions.rs): it was laid out again for any output in the
/// pane, the status line's turning included (lbterm/PERF.md, finding 63)
pub fn conversation_hash(screen: &Screen) -> u64 {
    let end = conversation_end(screen);
    let mut hasher = DefaultHasher::new();
    end.hash(&mut hasher);
    screen.rows[..end].hash(&mut hasher);
    hasher.finish()
}

/// The rows of each table row Claude Code draws in the screen's first `end` rows: a run of rows
/// whose first ink is the table's `│`, under its top border or a rule between its rows (`┌`,
/// `├`) or the screen's top, and over a rule or its bottom border (`├`, `└`) or `end`. Claude
/// Code's renderer (2026-11-14) draws a rule under the header and between every two rows, wraps
/// each cell in its own column and centres it in its row's height, so a cell of more than one
/// line isn't one run of words read row by row (lbterm/FLOWS.md, "A right hold, to the question
/// colours", What could go 4). A box (`╭`, `╰`) isn't a table
fn table_rows(screen: &Screen, end: usize) -> Vec<Range<usize>> {
    let first = |row: usize| screen.rows[row].iter().find(|g| !is_space(g.ch)).map(|g| g.ch);
    let mut out = vec![];
    let mut row = 0;
    while row < end {
        if first(row) != Some('\u{2502}') {
            row += 1;
            continue;
        }
        let start = row;
        while row < end && first(row) == Some('\u{2502}') {
            row += 1;
        }
        let above = start == 0 || matches!(first(start - 1), Some('\u{250c}' | '\u{251c}'));
        let below = row == end || matches!(first(row), Some('\u{251c}' | '\u{2514}'));
        if above && below {
            out.push(start..row);
        }
    }
    out
}

/// The screen's words, top to bottom, in its first `end` rows; a table row's ([`table_rows`]) a
/// cell at a time, left to right, each cell's lines top to bottom, as the reply's markdown has
/// them. A word keeps the row it's on, so in a table a word can be on a row above the one before
fn screen_words(screen: &Screen, end: usize, tables: &[Range<usize>]) -> Vec<Word> {
    let text = |row: usize| -> String { screen.rows[row].iter().map(|g| g.ch).collect() };
    let mut tables = tables.iter().peekable();
    let mut out = vec![];
    let mut row = 0;
    while row < end {
        if let Some(table) = tables.next_if(|t| t.start == row) {
            let cells: Vec<Vec<String>> = table
                .clone()
                .map(|r| text(r).split('\u{2502}').map(str::to_owned).collect())
                .collect();
            let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
            for column in 0..columns {
                for (r, line) in table.clone().zip(&cells) {
                    if let Some(cell) = line.get(column) {
                        out.extend(words(cell).into_iter().map(|text| Word { text, row: r }));
                    }
                }
            }
            row = table.end;
            continue;
        }
        out.extend(words(&text(row)).into_iter().map(|text| Word { text, row }));
        row += 1;
    }
    out
}

/// The first and last rows `found`'s words are on: in a table they needn't be its first word's
/// and its last's
fn rows_of(found: &[Word]) -> (usize, usize) {
    let rows = found.iter().map(|w| w.row);
    (rows.clone().min().unwrap_or(0), rows.max().unwrap_or(0))
}

/// Whether `quote` is among `on` from index `at`
fn matches_at(on: &[Word], at: usize, quote: &[String]) -> bool {
    on.len() >= at + quote.len() && on[at..at + quote.len()].iter().zip(quote).all(|(a, b)| a.text == *b)
}

/// Where `quote` is among the screen's words, by index. A quote cut off by the top or bottom of
/// the conversation counts by the part that's on it, when that part is right at the edge: at
/// least two fifths of it, and [`MIN_EDGE_WORDS`].
fn find(on: &[Word], quote: &[String], least_words: usize, area_end: usize) -> Option<Range<usize>> {
    let n = quote.len();
    if n < least_words || on.is_empty() {
        return None;
    }
    if let Some(i) = (0..on.len()).find(|&i| matches_at(on, i, quote)) {
        return Some(i..i + n);
    }
    let least = MIN_EDGE_WORDS.max((n * 2 + 4) / 5);
    // By each word's row, not by where the rows start among the words: a table's are read a
    // cell at a time ([`screen_words`]), so a later cell's words come back up to the top rows
    let top: Vec<usize> = (0..on.len()).filter(|&i| on[i].row < EDGE_ROWS).collect();
    let bottom: Vec<usize> = (0..on.len()).filter(|&i| on[i].row + EDGE_ROWS >= area_end).collect();
    for k in (least..n).rev() {
        // Its end, starting in the top rows
        if let Some(&i) = top.iter().find(|&&i| matches_at(on, i, &quote[n - k..])) {
            return Some(i..i + k);
        }
        // Its start, ending in the bottom rows
        if let Some(&e) = bottom.iter().find(|&&e| e + 1 >= k && matches_at(on, e + 1 - k, &quote[..k])) {
            return Some(e + 1 - k..e + 1);
        }
    }
    None
}

/// The block rows `first..=last` are in: up to the row it starts on, down to the row before
/// the next block, a blank row, or one indented less than its start
fn block_of(info: &[RowInfo], first: usize, last: usize) -> Range<usize> {
    let mut top = first;
    while !info[top].starts && top > 0 && !info[top - 1].blank && info[top - 1].indent <= info[top].indent {
        top -= 1;
    }
    let base = info[top].indent;
    let mut bottom = last;
    while bottom + 1 < info.len()
        && !info[bottom + 1].blank
        && !info[bottom + 1].starts
        && info[bottom + 1].indent >= base
    {
        bottom += 1;
    }
    top..bottom + 1
}

/// Colour `rows` for question `colour`: a question's own block is its alone, and a passage two
/// questions are about carries both
fn add_band(bands: &mut Vec<Band>, info: &[RowInfo], rows: Range<usize>, colour: usize, question: bool) {
    if let Some(band) = bands.iter_mut().find(|b| b.rows == rows) {
        if question && !band.question {
            band.question = true;
            band.colours.retain(|c| *c != colour);
            band.colours.insert(0, colour);
        } else if !band.question && !band.colours.contains(&colour) {
            band.colours.push(colour);
        }
        return;
    }
    bands.push(Band {
        cols: cols_of(info, rows.clone()),
        rows,
        colours: vec![colour],
        question,
    });
}

/// The columns of `rows`: from their first glyph to the end of the longest
fn cols_of(info: &[RowInfo], rows: Range<usize>) -> Range<usize> {
    let lines = &info[rows];
    let start = lines.iter().filter(|i| !i.blank).map(|i| i.indent).min().unwrap_or(0);
    let end = lines.iter().map(|i| i.end).max().unwrap_or(start);
    start..end.max(start)
}

/// Whether a block's first word is one of question `number`'s options: 1a, 1b
fn is_option(info: &RowInfo, number: u32) -> bool {
    info.starts
        && info.first_word.as_deref().is_some_and(|w| {
            w.strip_prefix(&number.to_string())
                .is_some_and(|rest| rest.len() == 1 && rest.chars().all(|c| c.is_ascii_lowercase()))
        })
}

/// What a held right button shows on `screen`: each question of the replies in `map` that's on
/// it, coloured with its options and every passage it's about, and a marker for each part of a
/// question on screen that's scrolled off it. The newest reply comes first, so where two claim
/// the same rows it keeps them.
pub fn lay_out(map: &Map, screen: &Screen) -> Layout {
    let end = conversation_end(screen);
    let mut info: Vec<RowInfo> = screen.rows.iter().take(end).map(|r| row_info(r)).collect();
    // A table row is one block, its first line starting it: a cell's passage is coloured
    // across its row's height, its other cells' lines with it
    let tables = table_rows(screen, end);
    for table in &tables {
        for row in table.start + 1..table.end {
            info[row].starts = false;
        }
    }
    let on = screen_words(screen, end, &tables);
    let mut layout = Layout {
        area: 0..end,
        newest: map
            .messages
            .last()
            .map(|m| {
                m.questions
                    .iter()
                    .enumerate()
                    .map(|(qi, q)| Ask {
                        number: q.number(),
                        extra: q.is_extra(),
                        pos: q.pos,
                        label: q.name(qi),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    };
    if on.is_empty() {
        return layout;
    }

    // Per reply, oldest first: what of it is on the screen (from where to where in it, in
    // words), and which of its questions' own words are
    let count = map.messages.len();
    let mut seen: Vec<Option<(usize, usize)>> = vec![None; count];
    let mut own_found: Vec<Vec<bool>> = map.messages.iter().map(|m| vec![false; m.questions.len()]).collect();
    for (age, message) in map.messages.iter().rev().enumerate() {
        let j = count - 1 - age;
        // Each question's own words and each passage: where it is in the reply, and on screen
        struct Item {
            question: usize,
            own: bool,
            pos: usize,
            found: Option<Range<usize>>,
        }
        let mut items = vec![];
        for (qi, q) in message.questions.iter().enumerate() {
            let own: Vec<String> = words(&q.text).into_iter().take(QUESTION_WORDS).collect();
            items.push(Item {
                question: qi,
                own: true,
                pos: q.pos,
                found: find(&on, &own, MIN_QUESTION_WORDS, end),
            });
            for quote in &q.about {
                items.push(Item {
                    question: qi,
                    own: false,
                    pos: quote.pos,
                    found: find(&on, &words(&quote.text), MIN_WORDS, end),
                });
            }
        }
        let shown: Vec<usize> = items.iter().filter(|i| i.found.is_some()).map(|i| i.pos).collect();
        let (Some(&lo), Some(&hi)) = (shown.iter().min(), shown.iter().max()) else {
            continue;
        };
        seen[j] = Some((lo, hi));
        for item in items.iter().filter(|i| i.own && i.found.is_some()) {
            own_found[j][item.question] = true;
        }

        // Questions first, so their own blocks are theirs
        items.sort_by_key(|i| !i.own);
        for item in &items {
            let Some(found) = &item.found else {
                continue;
            };
            let (first, last) = rows_of(&on[found.clone()]);
            let block = block_of(&info, first, last);
            let mut next = block.end;
            add_band(&mut layout.bands, &info, block.clone(), item.question, item.own);
            if !item.own {
                continue;
            }
            // Its options, in the blocks after it
            let number = message.questions[item.question].number();
            let mut rows = block;
            if let Some(number) = number {
                loop {
                    while next < end && info[next].blank {
                        next += 1;
                    }
                    if next >= end || !is_option(&info[next], number) {
                        break;
                    }
                    let option = block_of(&info, next, next);
                    next = option.end;
                    rows.end = option.end;
                    add_band(&mut layout.bands, &info, option, item.question, true);
                }
            }
            layout.asked.push(Asked {
                colour: item.question,
                number,
                row: on[found.start].row,
                cols: cols_of(&info, rows.clone()),
                rows,
                newest: age == 0,
            });
        }
        if age == 0 {
            layout.shown = Some(lo..hi);
        }

        // A marker for each way a question on screen goes off it
        for (qi, q) in message.questions.iter().enumerate() {
            let mine = || items.iter().filter(move |i| i.question == qi);
            if !mine().any(|i| i.found.is_some()) {
                continue;
            }
            for up in [true, false] {
                let off = mine().any(|i| i.found.is_none() && if up { i.pos < lo } else { i.pos > hi });
                if off && !layout.markers.iter().any(|m| m.colour == qi && m.up == up) {
                    layout.markers.push(Marker {
                        colour: qi,
                        label: q.name(qi),
                        up,
                        newest: age == 0,
                    });
                }
            }
        }
    }
    for (j, message) in map.messages.iter().enumerate() {
        for (qi, q) in message.questions.iter().enumerate() {
            let at = match seen[j] {
                Some(_) if own_found[j][qi] => Where::On,
                Some((lo, _)) if q.pos < lo => Where::Above,
                Some((_, hi)) if q.pos > hi => Where::Below,
                Some(_) => Where::On,
                None => {
                    let newer = seen[j + 1..].iter().any(Option::is_some);
                    let older = seen[..j].iter().any(Option::is_some);
                    match (newer, older) {
                        (true, false) => Where::Above,
                        (false, true) => Where::Below,
                        _ => Where::Unknown,
                    }
                }
            };
            layout.places.push(Place {
                reply: message.id.clone(),
                colour: qi,
                at,
            });
        }
    }
    layout
}

#[cfg(test)]
mod test {
    use super::*;

    fn quote(text: &str, pos: usize) -> Quote {
        Quote {
            text: text.into(),
            pos,
        }
    }

    fn question(label: &str, text: &str, pos: usize, about: Vec<Quote>) -> Question {
        Question {
            label: label.into(),
            number: None,
            text: text.into(),
            pos,
            about,
        }
    }

    fn map(questions: Vec<Question>) -> Map {
        Map {
            messages: vec![Message {
                id: "a".into(),
                at: 0,
                questions,
            }],
        }
    }

    /// The bands as (first row, rows, colours, question)
    fn bands(layout: &Layout) -> Vec<(usize, usize, Vec<usize>, bool)> {
        layout
            .bands
            .iter()
            .map(|b| (b.rows.start, b.rows.len(), b.colours.clone(), b.question))
            .collect()
    }

    /// A reply as Claude Code draws it fullscreen: its mark, two columns in, wrapped, its
    /// markdown gone, the input box and status under it
    const SCREEN: &[&str] = &[
        "● Here's what I found.",                                              // 0
        "",                                                                    // 1
        "  - The fleet still lists two old background agents that aren't",     // 2
        "    running: \"Keep going\" (lamp-sketch, 16 days old).",             // 3
        "  - The tab rail's labels come from lbterm_summaries, read on a",     // 4
        "    thread of its own.",                                              // 5
        "",                                                                    // 6
        "  The check passes on main.",                                         // 7
        "",                                                                    // 8
        "  Q1. Should the rail show labels for try windows too?",              // 9
        "  - 1a. Yes, the same way",                                           // 10
        "  - 1b. No",                                                          // 11
        "",                                                                    // 12
        "  Q2. Remove the two old background agents with claude rm? That",     // 13
        "  deletes them.",                                                     // 14
        "",                                                                    // 15
        "──────────────────────────────────────────",                          // 16
        "❯ ",                                                                  // 17
        "──────────────────────────────────────────",                          // 18
        "  [Opus 5.5] LBTerm | the fleet still lists two old background agents", // 19
    ];

    fn reply() -> Map {
        map(vec![
            question(
                "Q1",
                "Should the rail show labels for try windows too?",
                40,
                vec![quote("The tab rail's labels come from `lbterm_summaries`", 20)],
            ),
            question(
                "Q2",
                "Remove the two old background agents with `claude rm`",
                55,
                vec![quote("The fleet still lists two old background agents that aren't running", 4)],
            ),
        ])
    }

    #[test]
    fn a_rule_is_a_line_across_or_a_named_conversations_upper_one() {
        let line = "\u{2500}".repeat(60);
        for rule in [
            line.clone(),
            format!("  {line}  "),
            "\u{2501}".repeat(30),
            // Claude Code 2.1.314 on: the conversation's name near the right end
            format!("{line} LBTerm InMail \u{2500}"),
            format!("{line} ITMan InMail \u{2500}\u{2500}\u{2500}"),
            format!("{line} x \u{2500}"),
        ] {
            assert!(is_rule_text(&rule), "{rule}");
            let glyphs = Screen::from_lines(&[rule.as_str()]).rows.remove(0);
            assert!(is_rule(&glyphs), "glyphs of {rule}");
        }
        for not in [
            "\u{2500}".repeat(19),
            format!("{} name \u{2500}", "\u{2500}".repeat(19)),
            // No line after the name, or no space round it
            format!("{line} LBTerm InMail"),
            format!("{line} LBTerm InMail \u{2500} more"),
            format!("{line}LBTerm \u{2500}"),
            format!("{line} LBTerm\u{2500}"),
            format!("{line}  LBTerm \u{2500}"),
            format!("x {line}"),
            "\u{276f} ".to_string(),
            String::new(),
        ] {
            assert!(!is_rule_text(&not), "{not}");
            let glyphs = Screen::from_lines(&[not.as_str()]).rows.remove(0);
            assert!(!is_rule(&glyphs), "glyphs of {not}");
        }
    }

    #[test]
    fn a_named_conversation_ends_where_an_unnamed_one_does() {
        let mut named = SCREEN.to_vec();
        named[16] = "──────────────────────────── LBTerm InMail ─";
        let (plain, named) = (Screen::from_lines(SCREEN), Screen::from_lines(&named));
        assert_eq!(conversation_end(&named), 16);
        assert_eq!(conversation_end(&named), conversation_end(&plain));
        assert_eq!(lay_out(&reply(), &named), lay_out(&reply(), &plain));
    }

    #[test]
    fn a_screen_lays_out_the_same_while_its_conversation_is() {
        let screen = Screen::from_lines(SCREEN);
        let base = conversation_hash(&screen);
        // The status line turning and the prompt typed into: the same conversation
        let mut below = SCREEN.to_vec();
        below[19] = "  [Opus 5.5] LBTerm | working 12";
        below[17] = "❯ 1a";
        assert_eq!(conversation_hash(&Screen::from_lines(&below)), base);
        assert_eq!(
            lay_out(&reply(), &Screen::from_lines(&below)),
            lay_out(&reply(), &screen),
            "what the hash keeps is what lay_out gives"
        );
        // A row of the reply changed, or the box gone so the conversation runs on: another
        let mut above = SCREEN.to_vec();
        above[7] = "  The check fails on main.";
        assert_ne!(conversation_hash(&Screen::from_lines(&above)), base);
        let mut no_box = SCREEN.to_vec();
        no_box[16] = "";
        no_box[18] = "";
        assert_ne!(conversation_hash(&Screen::from_lines(&no_box)), base);
    }

    #[test]
    fn the_file_is_read() {
        let m = parse(
            r#"{"sessionId":"x","messages":[{"id":"m1","at":1,"questions":[
                {"label":"Q1","number":1,"text":"Which way?","pos":30,"about":[{"text":"the left road","pos":4}]},
                {"text":"And then?"}]}]}"#,
        )
        .unwrap();
        let q = &m.messages[0].questions;
        assert_eq!(q[0].number, Some(1));
        assert_eq!(q[0].about, vec![quote("the left road", 4)]);
        assert_eq!((q[1].label.as_str(), q[1].pos, q[1].about.len()), ("", 0, 0));
        assert_eq!(parse("\u{feff}{}"), Some(Map::default()));
        assert_eq!(parse("not json"), None);
    }

    #[test]
    fn words_are_letters_and_digits_lowercased() {
        assert_eq!(words("**Check:** `lbterm.ps1` -> don't"), vec!["check", "lbterm", "ps1", "don", "t"]);
        assert_eq!(words("Résumé, 2026-10-21!"), vec!["résumé", "2026", "10", "21"]);
        assert!(words("— ** ``").is_empty());
    }

    #[test]
    fn blocks_start_at_bullets_numbers_labels_and_marks() {
        for yes in ["- a", "* a", "• a", "● Here", "❯ hi", "⎿  out", "│ cell", "4. Keep", "2) x", "1a. Yes", "3b) no", "Q1. Why?", "Q12: x", "E4. Tidy it?", "E12) x"] {
            assert!(starts_block(yes), "{yes}");
        }
        for no in ["-5 degrees", "*args", "Question", "Q and A", "2026-10-21 was", "1.5 seconds", "4.", "anything", "(You said", "Every", "E4x"] {
            assert!(!starts_block(no), "{no}");
        }
    }

    #[test]
    fn a_question_its_options_and_its_passages_share_a_colour() {
        let layout = lay_out(&reply(), &Screen::from_lines(SCREEN));
        assert_eq!(
            bands(&layout),
            vec![
                (9, 1, vec![0], true),   // Q1
                (10, 1, vec![0], true),  // 1a
                (11, 1, vec![0], true),  // 1b
                (13, 2, vec![1], true),  // Q2, wrapped onto a second row
                (4, 2, vec![0], false),  // the rail's labels, the whole bullet
                (2, 2, vec![1], false),  // the old agents, the whole bullet
            ]
        );
        // Everything's on screen
        assert!(layout.markers.is_empty());
        // Where each question's own words start, for a voice draft's ticks: the labels' numbers;
        // and its rows with its options', for its ring
        assert_eq!(
            layout.asked,
            vec![
                Asked { colour: 0, number: Some(1), row: 9, rows: 9..12, cols: 2..SCREEN[9].chars().count(), newest: true },
                Asked { colour: 1, number: Some(2), row: 13, rows: 13..15, cols: 2..SCREEN[13].chars().count(), newest: true },
            ]
        );
        // The input box and the status under it are left out, though the status says it too
        assert_eq!(layout.area, 0..16);
        // A band runs from its rows' first glyph to the end of the longest
        let fleet = layout.bands.iter().find(|b| b.rows.start == 2).unwrap();
        assert_eq!(fleet.cols, 2..SCREEN[2].chars().count());
    }

    #[test]
    fn a_passage_two_questions_are_about_carries_both() {
        let mut m = reply();
        m.messages[0].questions[0]
            .about
            .push(quote("two old background agents that aren't running", 6));
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        // In the questions' order: Q1's bar first, the fill Q1's
        let fleet = layout.bands.iter().find(|b| b.rows.start == 2).unwrap();
        assert_eq!(fleet.colours, vec![0, 1]);
    }

    #[test]
    fn a_part_scrolled_off_gets_a_marker_pointing_to_it() {
        // Scrolled down: the findings are off the top, the questions on screen
        let screen = Screen::from_lines(&SCREEN[7..]);
        let layout = lay_out(&reply(), &screen);
        assert_eq!(
            layout.markers,
            vec![
                Marker { colour: 0, label: "Q1".into(), up: true, newest: true },
                Marker { colour: 1, label: "Q2".into(), up: true, newest: true },
            ]
        );
        // Scrolled up: the findings on screen, the questions off the bottom
        let mut top: Vec<&str> = SCREEN[..8].to_vec();
        top.extend_from_slice(&SCREEN[16..]);
        let layout = lay_out(&reply(), &Screen::from_lines(&top));
        assert_eq!(
            layout.markers.iter().map(|m| (m.label.as_str(), m.up)).collect::<Vec<_>>(),
            vec![("Q1", false), ("Q2", false)]
        );
        assert_eq!(bands(&layout), vec![(4, 2, vec![0], false), (2, 2, vec![1], false)]);
    }

    #[test]
    fn every_question_in_the_file_has_a_place_for_the_bars_arrow() {
        // Scrolled down: Q1 and Q2's questions on the screen, their findings off its top
        let at = |layout: &Layout| layout.places.iter().map(|p| (p.reply.clone(), p.colour, p.at)).collect::<Vec<_>>();
        let layout = lay_out(&reply(), &Screen::from_lines(&SCREEN[7..]));
        assert_eq!(at(&layout), vec![("a".into(), 0, Where::On), ("a".into(), 1, Where::On)]);
        // Only Q2's question on the screen: Q1's starts before it in the reply, so it's above
        let layout = lay_out(&reply(), &Screen::from_lines(&SCREEN[12..]));
        assert_eq!(at(&layout), vec![("a".into(), 0, Where::Above), ("a".into(), 1, Where::On)]);
        assert_eq!(Where::Above.arrow(), Some(true));
        // An older reply with nothing on the screen is above the newer one that is; with a
        // newer one off the screen too, that's below
        let mut map = reply();
        let mut older = map.messages[0].clone();
        older.id = "m0".into();
        older.questions.iter_mut().for_each(|q| {
            q.text = "Nothing like anything on the screen at all here".into();
            q.about.clear();
        });
        let mut newer = older.clone();
        newer.id = "m2".into();
        map.messages = vec![older, map.messages[0].clone(), newer];
        let layout = lay_out(&map, &Screen::from_lines(SCREEN));
        let places = at(&layout);
        assert_eq!(places[0].2, Where::Above, "{places:?}");
        assert_eq!(places.last().unwrap().2, Where::Below, "{places:?}");
        assert_eq!(Where::Below.arrow(), Some(false));
        // Nothing of any reply on the screen: which way isn't known, so no arrow
        let blank: Vec<&str> = vec![""; 10];
        let layout = lay_out(&reply(), &Screen::from_lines(&blank));
        assert!(layout.places.iter().all(|p| p.at.arrow().is_none()));
    }

    #[test]
    fn a_question_with_nothing_on_screen_gets_no_marker() {
        // Only Q2's question and nothing of Q1
        let screen = Screen::from_lines(&SCREEN[12..]);
        let layout = lay_out(&reply(), &screen);
        assert_eq!(
            layout.markers,
            vec![Marker { colour: 1, label: "Q2".into(), up: true, newest: true }]
        );
    }

    #[test]
    fn labels_from_e_on_are_extras() {
        for (label, extra) in [("E4", true), ("E12", true), ("Q1", false), ("", false), ("Every", false), ("4", false)] {
            assert_eq!(question(label, "Tidy it?", 0, vec![]).is_extra(), extra, "{label}");
        }
    }

    #[test]
    fn the_next_question_is_the_first_the_draft_hasnt_answered() {
        let layout = lay_out(&reply(), &Screen::from_lines(SCREEN));
        let on = |answered: &[u32]| match layout.next(answered, true) {
            Some(Next::On(a)) => Some((a.colour, a.rows.clone())),
            other => panic!("{other:?}"),
        };
        // Q1 with its two options, then Q2 on its two rows
        assert_eq!(on(&[]), Some((0, 9..12)));
        assert_eq!(on(&[1]), Some((1, 13..15)));
        assert_eq!(on(&[2]), Some((0, 9..12)), "an answer to a later one doesn't move it on");
        assert_eq!(layout.next(&[1, 2], true), None, "every one answered: no ring");
        assert_eq!(layout.next(&[2, 1, 7], true), None);
    }

    #[test]
    fn extras_count_only_when_the_setting_says_so() {
        let mut m = reply();
        m.messages[0].questions.push(question("E3", "Ring it in red?", 70, vec![]));
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        assert_eq!(layout.next_to_answer(&[1, 2], true), Some(2));
        assert_eq!(layout.next_to_answer(&[1, 2], false), None);
        assert_eq!(layout.next_to_answer(&[1], false), Some(1));
        // An extra before a question, answered or not, is passed by without them
        m.messages[0].questions.insert(0, question("E9", "Something first?", 1, vec![]));
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        assert_eq!(layout.next_to_answer(&[], false), Some(1));
        assert_eq!(layout.next_to_answer(&[], true), Some(0));
    }

    #[test]
    fn a_question_with_no_number_is_passed_by() {
        let mut m = reply();
        m.messages[0].questions.insert(0, question("", "Anything else?", 1, vec![]));
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        assert_eq!(layout.next_to_answer(&[], true), Some(1));
    }

    #[test]
    fn the_next_question_off_the_screen_is_its_marker_at_that_edge() {
        // Scrolled down: Q1's passage is off the top, so its marker's there already
        let layout = lay_out(&reply(), &Screen::from_lines(&SCREEN[7..]));
        let q1_up = Marker { colour: 0, label: "Q1".into(), up: true, newest: true };
        assert!(layout.markers.contains(&q1_up));
        // Q1's own words are still on the screen, so it's ringed there
        assert!(matches!(layout.next(&[], true), Some(Next::On(a)) if a.colour == 0));
        // Only Q2's on the screen and nothing of Q1: a marker of its own, above
        let layout = lay_out(&reply(), &Screen::from_lines(&SCREEN[12..]));
        assert!(!layout.markers.iter().any(|m| m.colour == 0));
        assert_eq!(layout.next(&[], true), Some(Next::Off(q1_up)));
        assert!(matches!(layout.next(&[1], true), Some(Next::On(a)) if a.colour == 1));
        // Scrolled up: the passages on the screen, both questions off the bottom
        let mut top: Vec<&str> = SCREEN[..8].to_vec();
        top.extend_from_slice(&SCREEN[16..]);
        let layout = lay_out(&reply(), &Screen::from_lines(&top));
        let q2_down = Marker { colour: 1, label: "Q2".into(), up: false, newest: true };
        assert_eq!(layout.next(&[1], true), Some(Next::Off(q2_down)));
    }

    #[test]
    fn with_nothing_of_the_newest_reply_on_the_screen_there_is_no_ring() {
        // The reply on the screen is an older one; the newest asked about something else
        let mut m = reply();
        m.messages.push(Message {
            id: "b".into(),
            at: 0,
            questions: vec![question("Q1", "Drop the summary pane as well?", 12, vec![])],
        });
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        assert!(layout.asked.iter().all(|a| !a.newest));
        assert_eq!(layout.shown, None);
        assert_eq!(layout.next(&[], true), None);
        assert_eq!(layout.newest.len(), 1);
    }

    #[test]
    fn a_passage_cut_off_by_the_top_still_counts() {
        // The first row of the bullet about the old agents is above the screen
        let screen = Screen::from_lines(&SCREEN[3..]);
        let mut m = reply();
        m.messages[0].questions[1].about =
            vec![quote("aren't running: \"Keep going\" (lamp-sketch, 16 days old)", 9)];
        let layout = lay_out(&m, &screen);
        assert!(bands(&layout).contains(&(0, 1, vec![1], false)), "{:?}", bands(&layout));
    }

    #[test]
    fn quotes_too_short_or_not_on_screen_colour_nothing() {
        let m = map(vec![question(
            "",
            "Is this on screen at all?",
            3,
            vec![quote("the check", 0), quote("nowhere on this screen", 1)],
        )]);
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        assert!(layout.is_empty(), "{layout:?}");
    }

    #[test]
    fn a_question_without_a_label_is_named_by_its_number_or_place() {
        let q = question("", "Which?", 0, vec![]);
        assert_eq!(q.name(2), "Q3");
        assert_eq!(Question { number: Some(7), ..q.clone() }.name(0), "Q7");
        assert_eq!(question("Q4", "x", 0, vec![]).number(), Some(4));
    }

    #[test]
    fn the_newest_reply_keeps_rows_two_claim() {
        let mut m = reply();
        // An older reply asking the same thing first
        m.messages.insert(
            0,
            Message {
                id: "old".into(),
                at: 0,
                questions: vec![question(
                    "Q1",
                    "Remove the two old background agents with claude rm",
                    0,
                    vec![],
                )],
            },
        );
        let layout = lay_out(&m, &Screen::from_lines(SCREEN));
        let q2 = layout.bands.iter().find(|b| b.rows.start == 13).unwrap();
        assert_eq!(q2.colours, vec![1]);
    }

    #[test]
    fn a_short_labelled_question_is_found_by_its_label_and_a_word() {
        let m = map(vec![question("Q3", "Q3. Which?", 0, vec![])]);
        let layout = lay_out(&m, &Screen::from_lines(&["", "  Q3. Which?", "  - 3a. This one", "  - 3b. That one", ""]));
        assert_eq!(
            bands(&layout),
            vec![(1, 1, vec![0], true), (2, 1, vec![0], true), (3, 1, vec![0], true)]
        );
    }

    /// A real reply's last rows as Claude Code 2.1.306 drew them in LBTerm (`lbterm cli get-text`,
    /// 2026-10-21), and what Haiku found in it through the question worker
    #[test]
    fn a_real_reply_as_haiku_found_it() {
        let screen = Screen::from_lines(&[
            "     backticks, drawn bullets, wrapped to the pane's width). So the matching has to ignore those differences",
            "     and follow a passage across rows. It can also only colour what's on screen. If the bullet a question is",
            "     about has scrolled out of Claude's own view, the terminal can't reach it.",
            "  4. Keep a map for each message, not just the latest. Then holding right-click on an older reply you've",
            "     scrolled back to works too.",
            "",
            "  Terminal would write the file from the Stop hook. LBTerm would handle the hold, watching the file, matching",
            "  it on screen and drawing. (You said on 2026-10-21 that either side can edit the other's files.)",
            "",
            "  Questions",
            "",
            "  Q1. When should Haiku run?",
            "  - 1a. After every reply that asks something, so it's instant on press (recommended)",
            "  - 1b. Only when you press, so nothing runs until you use it, but you'd hold for a few seconds before seeing",
            "    anything",
            "",
            "  Q2. What gets coloured?",
            "  - 2a. The whole bullet or paragraph a quote is in, like a highlighter, with the question line in the same",
            "    colour (recommended: it fits your example and handles wrapping better)",
            "  - 2b. Only the exact words Haiku quotes",
            "",
            "  Q3. When a related passage is off screen:",
            "  - 3a. Put a small \"Q5 \u{2191}\" marker at the top or bottom edge (recommended)",
            "  - 3b. Show nothing",
            "",
            "  Q4. Who builds it?",
            "  - 4a. This session builds both halves on a branch, then sends Terminal an InMail about what changed",
            "  - 4b. Leave it with Terminal to design first",
            "",
            "  Separately, LBTerm's own inbox has two other messages from Terminal waiting: OSC 7501 plus a **Check:**",
            "  line, and hiding the scrollbar while Claude Code owns the pane. The Check line is already in CLAUDE.md, so",
            "  that half is done. I haven't touched either one.",
            "",
            "\u{273b} Cooked for 2m 0s \u{b7} done 20:26 \u{b7} 1 shell still running",
            "",
            "\u{276f} 1a",
        ]);
        let m = parse(r#"{"sessionId":"0e5f1c2a-7b3d-4e8f-9a6c-5d4b3a2f1e0d","messages":[{"id":"60f0b4dc87e4cb50","at":1792578767317,"questions":[
            {"label":"Q1","number":1,"text":"Q1. When should Haiku run?","pos":511,"about":[
                {"text":"Run Haiku when a reply ends, not when you press.","pos":242},
                {"text":"A `claude -p` Haiku call takes seconds, so a hold would show nothing for a while before the colours came in.","pos":252}]},
            {"label":"Q2","number":2,"text":"Q2. What gets coloured?","pos":553,"about":[
                {"text":"Haiku only has to return, for each question, short quotes copied word for word from the reply.","pos":357},
                {"text":"So the matching has to ignore those differences and follow a passage across rows.","pos":402}]},
            {"label":"Q3","number":3,"text":"Q3. When a related passage is off screen:","pos":594,"about":[
                {"text":"It can also only colour what's on screen. If the bullet a question is about has scrolled out of Claude's own view, the terminal can't reach it.","pos":416}]},
            {"label":"Q4","number":4,"text":"Q4. Who builds it?","pos":618,"about":[
                {"text":"Most of it would be built in LBTerm.","pos":52},
                {"text":"Terminal would write the file from the Stop hook. LBTerm would handle the hold, watching the file, matching it on screen and drawing.","pos":472}]}]}]}"#)
        .unwrap();
        let layout = lay_out(&m, &screen);
        assert_eq!(
            bands(&layout),
            vec![
                (11, 1, vec![0], true),
                (12, 1, vec![0], true),
                (13, 2, vec![0], true),
                (16, 1, vec![1], true),
                (17, 2, vec![1], true),
                (19, 1, vec![1], true),
                (21, 1, vec![2], true),
                (22, 1, vec![2], true),
                (23, 1, vec![2], true),
                (25, 1, vec![3], true),
                (26, 1, vec![3], true),
                (27, 1, vec![3], true),
                // Item 3, its start above the screen: what Q2 and Q3 are about
                (0, 3, vec![1, 2], false),
                (6, 2, vec![3], false),
            ]
        );
        // Q1's passages, one of Q2's and one of Q4's are higher up the reply; all of Q3's is here
        assert_eq!(
            layout.markers.iter().map(|m| (m.label.as_str(), m.up)).collect::<Vec<_>>(),
            vec![("Q1", true), ("Q2", true), ("Q4", true)]
        );
    }

    #[test]
    fn colours_go_round() {
        assert_eq!(colour(0), COLOURS[0]);
        assert_eq!(colour(COLOURS.len() + 1), COLOURS[1]);
    }

    /// lbterm/FLOWS.md, "A right hold, to the question colours", What could go 1: the worker
    /// writes a reply's questions 8 to 19 s after the reply ends, so a hold before then finds
    /// only the session's older replies in the file. With none of them on the screen nothing is
    /// drawn, and what the layout calls the newest reply is the one before, whose questions a
    /// voice draft's ring and ticks would then go by
    #[test]
    fn a_hold_before_the_worker_has_written_the_reply_shows_nothing() {
        let older = Message {
            id: "older".into(),
            at: 0,
            questions: vec![question("Q1", "Which build should the try window run?", 10, vec![])],
        };
        let screen = Screen::from_lines(SCREEN);
        let before = Map {
            messages: vec![older.clone()],
        };
        let layout = lay_out(&before, &screen);
        assert!(layout.is_empty());
        assert_eq!(layout.shown, None);
        let labels = |l: &Layout| l.newest.iter().map(|a| a.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&layout), ["Q1"]);
        assert_eq!(layout.newest[0].pos, 10, "the older reply's Q1");

        // Once the worker has written it, the same screen is coloured
        let mut after = reply();
        after.messages.insert(0, older);
        let layout = lay_out(&after, &screen);
        assert!(!layout.is_empty());
        assert_eq!(labels(&layout), ["Q1", "Q2"]);
    }

    /// The table of the tests below as Claude Code draws it (its renderer, 2026-11-14): a rule
    /// across under the header and between every two rows, each cell wrapped in its own column
    /// and centred in its row's height
    const TABLE: &[&str] = &[
        "● Two builds differ:",                          // 0
        "",                                              // 1
        "  ┌─────────────────────┬────────────┐",        // 2
        "  │ What it changes     │ Build      │",        // 3
        "  ├─────────────────────┼────────────┤",        // 4
        "  │ the rail's labels   │ slot-1     │",        // 5
        "  │ come from summaries │            │",        // 6
        "  └─────────────────────┴────────────┘",        // 7
        "",                                              // 8
        "  Q1. Which build should the try window run?", // 9
        "",                                              // 10
        "──────────────────────────────────────────",    // 11
        "❯ ",                                            // 12
    ];

    /// What could go 4: a passage quoted from a cell longer than its column isn't one run of
    /// words read row by row. Until stage 2 of feat/questions-pane it wasn't coloured, and since
    /// it wasn't found it counted as scrolled off: a marker said it was above, though it was on
    /// the screen (25 of 1,921 passages over the ten days to 2026-11-14 were in a table's row).
    /// The table is read a column at a time now, and the band is the cell's table row
    #[test]
    fn a_passage_wrapped_in_a_table_cell_is_found() {
        let map = map(vec![question(
            "Q1",
            "Which build should the try window run?",
            20,
            vec![quote("the rail's labels come from summaries", 8)],
        )]);
        let layout = lay_out(&map, &Screen::from_lines(TABLE));
        assert_eq!(bands(&layout), vec![(9, 1, vec![0], true), (5, 2, vec![0], false)]);
        assert_eq!(layout.markers, vec![], "nothing of it is off the screen");
    }

    /// A passage that runs on from a cell into the next of its row, as the reply's markdown
    /// has it, is found too, and its band is the whole table row, though the shorter cell's
    /// last word is on a row above its first
    #[test]
    fn a_passage_from_one_cell_into_the_next_is_found() {
        let map = map(vec![question(
            "Q1",
            "Which build should the try window run?",
            20,
            vec![quote("labels | come from summaries | slot-1", 8)],
        )]);
        let layout = lay_out(&map, &Screen::from_lines(TABLE));
        assert_eq!(bands(&layout), vec![(9, 1, vec![0], true), (5, 2, vec![0], false)]);
    }

    /// A table row cut off by the top: its second cell's passage starts in the top rows, after
    /// the first cell's words, which reach past them
    #[test]
    fn a_passage_in_a_table_cut_off_by_the_top_still_counts() {
        let screen = Screen::from_lines(&[
            "  │ one         │ the summaries   │",          // 0
            "  │ two         │ thread reads    │",          // 1
            "  │ three       │ every label the │",          // 2
            "  │ four        │ rail shows once │",          // 3
            "  │ five        │ a second        │",          // 4
            "  └─────────────┴─────────────────┘",          // 5
            "",                                              // 6
            "  Q1. Which build should the try window run?", // 7
            "",                                              // 8
            "──────────────────────────────────────────",    // 9
            "❯ ",                                            // 10
        ]);
        let map = map(vec![question(
            "Q1",
            "Which build should the try window run?",
            20,
            vec![quote(
                "The cache keeps the summaries thread reads every label the rail shows once a second",
                8,
            )],
        )]);
        let layout = lay_out(&map, &screen);
        assert_eq!(bands(&layout), vec![(7, 1, vec![0], true), (0, 5, vec![0], false)]);
        assert_eq!(layout.markers, vec![]);
    }
}
