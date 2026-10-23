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
    fn number(&self) -> Option<u32> {
        self.number.or_else(|| {
            let digits: String = self.label.chars().filter(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
    }
}

/// One reply's questions.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub id: String,
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

/// What a held right button shows on one screen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub bands: Vec<Band>,
    pub markers: Vec<Marker>,
    /// The conversation's rows, above Claude Code's input box: the markers go at its edges
    pub area: Range<usize>,
}

impl Layout {
    pub fn is_empty(&self) -> bool {
        self.bands.is_empty() && self.markers.is_empty()
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

fn is_space(ch: char) -> bool {
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
        // A question's label: Q1. Q12:
        'Q' => {
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

/// A row Claude Code draws round its input box: a line across, nothing else
fn is_rule(row: &[Glyph]) -> bool {
    let ink = row.iter().filter(|g| !is_space(g.ch)).count();
    ink >= 20 && row.iter().all(|g| is_space(g.ch) || g.ch == '\u{2500}' || g.ch == '\u{2501}')
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

/// The screen's words, top to bottom, in its first `end` rows
fn screen_words(screen: &Screen, end: usize) -> Vec<Word> {
    let mut out = vec![];
    for (row, glyphs) in screen.rows.iter().take(end).enumerate() {
        let text: String = glyphs.iter().map(|g| g.ch).collect();
        out.extend(words(&text).into_iter().map(|text| Word { text, row }));
    }
    out
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
    let top_end = on.iter().position(|w| w.row >= EDGE_ROWS).unwrap_or(on.len());
    let bottom_start = on.iter().position(|w| w.row + EDGE_ROWS >= area_end).unwrap_or(on.len());
    for k in (least..n).rev() {
        // Its end, starting in the top rows
        if let Some(i) = (0..top_end).find(|&i| matches_at(on, i, &quote[n - k..])) {
            return Some(i..i + k);
        }
        // Its start, ending in the bottom rows
        if let Some(e) = (bottom_start..on.len()).find(|&e| e + 1 >= k && matches_at(on, e + 1 - k, &quote[..k])) {
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
    let lines = &info[rows.clone()];
    let start = lines.iter().filter(|i| !i.blank).map(|i| i.indent).min().unwrap_or(0);
    let end = lines.iter().map(|i| i.end).max().unwrap_or(start);
    bands.push(Band {
        rows,
        cols: start..end.max(start),
        colours: vec![colour],
        question,
    });
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
    let info: Vec<RowInfo> = screen.rows.iter().take(end).map(|r| row_info(r)).collect();
    let on = screen_words(screen, end);
    let mut layout = Layout {
        area: 0..end,
        ..Default::default()
    };
    if on.is_empty() {
        return layout;
    }

    for message in map.messages.iter().rev() {
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

        // Questions first, so their own blocks are theirs
        items.sort_by_key(|i| !i.own);
        for item in &items {
            let Some(found) = &item.found else {
                continue;
            };
            let block = block_of(&info, on[found.start].row, on[found.end - 1].row);
            let mut next = block.end;
            add_band(&mut layout.bands, &info, block, item.question, item.own);
            if !item.own {
                continue;
            }
            // Its options, in the blocks after it
            let Some(number) = message.questions[item.question].number() else {
                continue;
            };
            loop {
                while next < end && info[next].blank {
                    next += 1;
                }
                if next >= end || !is_option(&info[next], number) {
                    break;
                }
                let option = block_of(&info, next, next);
                next = option.end;
                add_band(&mut layout.bands, &info, option, item.question, true);
            }
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
                    });
                }
            }
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
        for yes in ["- a", "* a", "• a", "● Here", "❯ hi", "⎿  out", "│ cell", "4. Keep", "2) x", "1a. Yes", "3b) no", "Q1. Why?", "Q12: x"] {
            assert!(starts_block(yes), "{yes}");
        }
        for no in ["-5 degrees", "*args", "Question", "Q and A", "2026-10-21 was", "1.5 seconds", "4.", "anything", "(You said"] {
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
                Marker { colour: 0, label: "Q1".into(), up: true },
                Marker { colour: 1, label: "Q2".into(), up: true },
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
    fn a_question_with_nothing_on_screen_gets_no_marker() {
        // Only Q2's question and nothing of Q1
        let screen = Screen::from_lines(&SCREEN[12..]);
        let layout = lay_out(&reply(), &screen);
        assert_eq!(
            layout.markers,
            vec![Marker { colour: 1, label: "Q2".into(), up: true }]
        );
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
}
