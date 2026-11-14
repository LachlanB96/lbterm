//! The weights (lbterm/QUESTIONS.md, "The weights"). Lachlan's ask of 2026-11-12: "when
//! questions have answers with weights attached, the weights should be coloured. so close to 0
//! would be red and close to 100 would be green." A weight is the share of 100 Claude gives an
//! option it recommends, in brackets after the option's words and before its reason (Blueprint's
//! rules/asking.md): `  1a. Monthly JSONL (70): the dashboard reads a month at a time`.
//!
//! One rule finds it, whoever gives the text: [`share_in`] over an option's words, and [`share`]
//! over an option's line, its label first. A harness session's question block carries its
//! options as structure, label and text apart, so LBTerm runs [`share_in`] on each option's text
//! as it makes the option's cells. A Claude Code tab has only its pane's screen, the reply as
//! Claude Code drew it, so [`on_row`] reads it a row at a time, top first, each row handed what
//! the row above left open ([`Head`]: an option's line Claude Code wrapped onto it), and gives the
//! columns of the share's digits that landed on it: that's the Claude Code producer, to go when
//! Claude Code's tabs do. The GUI runs it on a Claude pane's row as the row changes and keeps it
//! with the row (wezterm-gui's render/triggers.rs); [`on_screen`] runs it down a whole screen.
//! Its colour is `config::lbterm::weight_colour`'s.

use super::{conversation_end, is_space, starts_block, Glyph, Screen};
use std::ops::Range;

/// The most a share is: it's of 100
pub const SHARE_MOST: u32 = 100;
/// The most digits an option's label has before its letter: 1a, 12c
const LABEL_DIGITS: usize = 3;
/// The most digits a share has: 100
const SHARE_DIGITS: usize = 3;
/// A bullet Claude Code may draw before an option's label, though his rules say never one
const BULLETS: [char; 3] = ['-', '*', '\u{2022}'];

/// An option's share of 100, and where its digits are in the text it was found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// 0 to 100
    pub share: u8,
    /// Its digits, by char, not byte: its brackets are the chars either side
    pub digits: Range<usize>,
}

/// A share on the pane's screen: the row its digits landed on and their columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// 0 to 100
    pub share: u8,
    pub row: usize,
    /// Its digits' columns: its brackets are the columns either side
    pub cols: Range<usize>,
}

/// The share in an option's words, as a harness session's question block gives them, its label
/// apart (`Monthly JSONL (70): the dashboard reads a month at a time`): the bracketed digits its
/// words end with, before its reason. `digits` is by char of `words`.
pub fn share_in(words: &str) -> Option<Share> {
    let chars: Vec<char> = words.chars().collect();
    share_from(&chars, 0)
}

/// The share on an option's line, as a reply has it (`  1a. Monthly JSONL (70): ...`): its label
/// at the line's start after its indent, then [`share_in`]'s rule on its words. `digits` is by
/// char of `line`. `None` for a line that isn't an option's, or an option with no share.
pub fn share(line: &str) -> Option<Share> {
    let chars: Vec<char> = line.chars().collect();
    share_from(&chars, after_label(&chars)?)
}

/// Where an option line's words start: past its indent, a bullet if one was drawn, its label
/// (a number of up to [`LABEL_DIGITS`] digits, one lowercase letter and a full stop: 1a., 12c.)
/// and the space after it. `None` when it's no option's line: a question's (`Q1.`, `2.`), an
/// extra's (`E4.`), a table cell's (`│`), inline code's (a backtick), a sentence.
fn after_label(chars: &[char]) -> Option<usize> {
    let space = |i: usize| chars.get(i).is_some_and(|c| c.is_whitespace());
    let mut i = 0;
    while space(i) {
        i += 1;
    }
    if chars.get(i).is_some_and(|c| BULLETS.contains(c)) && space(i + 1) {
        i += 1;
        while space(i) {
            i += 1;
        }
    }
    let number = i;
    while chars.get(i).is_some_and(char::is_ascii_digit) {
        i += 1;
    }
    if i == number || i - number > LABEL_DIGITS {
        return None;
    }
    if !chars.get(i).is_some_and(char::is_ascii_lowercase) || chars.get(i + 1) != Some(&'.') {
        return None;
    }
    i += 2;
    space(i).then_some(i)
}

/// Where an option's head ends: at its line's first colon that a space or the end follows,
/// where its reason starts. `None` while there's none, as on a row whose line wraps onto the next
fn head_end(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == ':' && chars.get(i + 1).map_or(true, |c| c.is_whitespace()))
}

/// The share in `chars` from `from`, where the option's words start: the head (up to
/// [`head_end`], else the end) ends with `(` digits `)`, the bracket after a space and after at
/// least one letter or digit of the option's words, the digits no more than [`SHARE_MOST`]. So a
/// bracketed number among the option's words (`Keep (3) copies:`) or in its reason isn't one
fn share_from(chars: &[char], from: usize) -> Option<Share> {
    let mut end = head_end(chars, from).unwrap_or(chars.len());
    while end > from && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    if end == from || chars[end - 1] != ')' {
        return None;
    }
    let close = end - 1;
    let mut open = close;
    while open > from && chars[open - 1].is_ascii_digit() {
        open -= 1;
    }
    let digits = open..close;
    if digits.is_empty() || digits.len() > SHARE_DIGITS || open == from || chars[open - 1] != '(' {
        return None;
    }
    let bracket = open - 1;
    if bracket == from || !chars[bracket - 1].is_whitespace() {
        return None;
    }
    if !chars[from..bracket].iter().any(|c| c.is_alphanumeric()) {
        return None;
    }
    let share: u32 = chars[digits.clone()].iter().collect::<String>().parse().ok()?;
    (share <= SHARE_MOST).then(|| Share {
        share: share as u8,
        digits,
    })
}

/// A row's glyphs from its first ink to its last, the spaces between as `' '`
fn ink(row: &[Glyph]) -> Vec<Glyph> {
    let Some(first) = row.iter().position(|g| !is_space(g.ch)) else {
        return vec![];
    };
    let last = row.iter().rposition(|g| !is_space(g.ch)).unwrap_or(first);
    row[first..=last]
        .iter()
        .map(|g| Glyph {
            ch: if is_space(g.ch) { ' ' } else { g.ch },
            ..*g
        })
        .collect()
}

/// What the rows above leave the row under them: whether an option's line runs on into it.
/// [`on_row`] takes it from the row above and gives it to the row below, top first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Head {
    /// No option's line runs on into the row: the screen's top row, or the row above closed
    /// its option's head, was blank, or was no option's
    #[default]
    Closed,
    /// The row above is an option's line whose head hasn't closed (no colon then a space yet), so
    /// this row is the rest of it unless it's blank or starts a block. `words`: the head has a
    /// letter or digit after its label already
    Open { words: bool },
}

/// A share's digits on one row: their columns, its brackets the columns either side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnRow {
    /// 0 to 100
    pub share: u8,
    pub cols: Range<usize>,
}

/// One row of a Claude Code pane's screen, as a frame draws its rows, top first: the share whose
/// digits are on it, and what it leaves the row under it. The Claude Code producer: the GUI
/// reads each row of a Claude pane as it changes (wezterm-gui's render/triggers.rs), keeps what
/// it found with the row, and hands the [`Head`] down from row to row.
///
/// A row whose first ink is an option's label ([`after_label`]) starts an option's line. Claude
/// Code wraps a line longer than the pane onto the rows under it, back to the paragraph's left or
/// under a list item's words, so with the head `Open` a row that's neither blank nor starts a
/// block of its own (`starts_block`: another option, a question, a bullet, Claude Code's marks, a
/// table's or a rule's line) is the rest of that line, whatever its indent: it's read as if the
/// wrap's space and the head's words came before it. A head closes at a colon then a space, or a
/// row that ends with one ([`head_end`]), since the share is before it, so the reason's rows are
/// never read as the head's.
///
/// What a row can't tell: what's under it. A row with no colon whose words end with a bracketed
/// number is taken as ending its head there (an option with no reason), so in the rare line
/// Claude Code wrapped just after a bracketed number among its words (`Keep (3)` then `copies
/// (60): ...` on the next row) the `3` is coloured too. Only a row that starts with a digit or a
/// bullet, or follows an open head, is read past its first glyph.
pub fn on_row(row: &[Glyph], head: Head) -> (Option<OnRow>, Head) {
    let Some(first) = row.iter().find(|g| !is_space(g.ch)).map(|g| g.ch) else {
        return (None, Head::Closed);
    };
    let starts = first.is_ascii_digit() || BULLETS.contains(&first);
    if head == Head::Closed && !starts {
        return (None, Head::Closed);
    }
    let ink = ink(row);
    let text: Vec<char> = ink.iter().map(|g| g.ch).collect();
    // The rest of the line above: read after the wrap's space, and a stand-in for the words the
    // head has already, so the rule that a share follows some of an option's words holds across
    // the wrap
    let (chars, prefix, from) = match head {
        Head::Open { words } if !starts_block(&text.iter().collect::<String>()) => {
            let prefix: &[char] = if words { &['x', ' '] } else { &[' '] };
            let chars: Vec<char> = prefix.iter().chain(text.iter()).copied().collect();
            (chars, prefix.len(), 0)
        }
        _ => {
            if !starts {
                return (None, Head::Closed);
            }
            let Some(from) = after_label(&text) else {
                return (None, Head::Closed);
            };
            (text, 0, from)
        }
    };
    let next = match head_end(&chars, from) {
        Some(_) => Head::Closed,
        None => Head::Open {
            words: chars[from..].iter().any(|c| c.is_alphanumeric()),
        },
    };
    let found = share_from(&chars, from).and_then(|found| {
        let first = ink.get(found.digits.start.checked_sub(prefix)?)?;
        let last = ink.get(found.digits.end.checked_sub(prefix + 1)?)?;
        Some(OnRow {
            share: found.share,
            cols: first.col..last.col + last.width.max(1),
        })
    });
    (found, next)
}

/// Whether a row whose first ink is `ch` is read on with `head`: only one that starts with a digit
/// or a bullet, or follows an open head, can hold a share ([`on_row`]). `None` while `ch` is a
/// space, so a caller walking a row's cells keeps them only until its first ink says no
pub fn reads_on(ch: char, head: Head) -> Option<bool> {
    if is_space(ch) {
        return None;
    }
    Some(head != Head::Closed || ch.is_ascii_digit() || BULLETS.contains(&ch))
}

/// Every option's share on a Claude Code pane's screen, top first: [`on_row`] down its rows above
/// Claude Code's input box (`conversation_end`, as the colours read it), each handed the head
/// the row above left. The GUI reads a pane a row at a time as its rows change; this reads a
/// whole [`Screen`] the same way, for the tests and anything holding a screen.
///
/// What it can't tell: a code block's lines, which Claude Code draws bare (no fence, bar or
/// indent: only their colour says, and a [`Screen`] keeps none), so an option's line in one is
/// read as an option's; an option whose label's row is above the screen, since the row its
/// share's on doesn't say it's an option's; a share broken across two rows, which only a word
/// wider than the pane is, isn't placed; and what [`on_row`] can't (a row's words ending with a
/// bracketed number where Claude Code wrapped the head on).
pub fn on_screen(screen: &Screen) -> Vec<Placed> {
    let end = conversation_end(screen);
    let mut head = Head::Closed;
    let mut out = vec![];
    for (row, glyphs) in screen.rows[..end].iter().enumerate() {
        let (found, next) = on_row(glyphs, head);
        head = next;
        if let Some(OnRow { share, cols }) = found {
            out.push(Placed { share, row, cols });
        }
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;

    /// The chars of `text` in `range`
    fn chars(text: &str, range: Range<usize>) -> String {
        text.chars().skip(range.start).take(range.len()).collect()
    }

    /// The columns of the digits in `bracket` ("(70)") on an ASCII screen line
    fn digits(line: &str, bracket: &str) -> Range<usize> {
        let at = line.find(bracket).unwrap() + 1;
        at..at + bracket.len() - 2
    }

    /// Blueprint's rules/asking.md, its third rule: "give each option a share of 100 for how
    /// strongly you recommend it, in brackets after the option's words and before its one-line
    /// reason: `  1a. Monthly JSONL (70): the dashboard reads a month at a time`"
    #[test]
    fn his_format_gives_each_option_its_share() {
        let line = "  1a. Monthly JSONL (70): the dashboard reads a month at a time";
        let found = share(line).unwrap();
        assert_eq!(found.share, 70);
        assert_eq!(chars(line, found.digits), "70");
        for (line, want) in [
            ("  1b. Daily files (30): one more file to read each time", 30),
            ("12c. Keep it as it is (100): it's what you asked for", 100),
            ("  2a. Leave it (0): nothing to gain", 0),
            ("  3a. Somewhere between (35): steps of 10 are Claude's rule, not this one's", 35),
            // No reason after it, or its reason on the line after
            ("  3b. With no reason (40)", 40),
            ("  3c. Its reason on the next line (20):", 20),
            ("  3d. A space before the colon (60) : and more", 60),
            // The share is what the option's words end with, so a bracketed number among them
            // isn't, and a time's colon isn't the reason's
            ("  4a. Use (2) workers (60): the share ends the option's words", 60),
            ("  4b. At 10:30 (50): a time's colon has no space after it", 50),
            // A bullet Claude Code drew, though his rules say never one
            ("  - 5a. Under a bullet (80): read as well", 80),
            ("  \u{2022} 5b. Under a drawn bullet (20): read as well", 20),
        ] {
            let found = share(line).unwrap_or_else(|| panic!("no share in {line:?}"));
            assert_eq!(found.share, want, "{line}");
            assert_eq!(chars(line, found.digits), want.to_string(), "{line}");
        }
    }

    #[test]
    fn nothing_else_is_a_share() {
        for line in [
            // A question's own line, labelled or numbered, and an extra
            "Q1. Which store (70)?",
            "  Q2. Which store (70): a question's line has no share",
            "  2. Which store (70): a question numbered alone",
            "  E4. Dim the answered chips (70): an extra has none",
            // A bracketed number in a sentence, or after the colon, in the reason
            "  We ran it (3) times and kept (70)",
            "  1a. Monthly JSONL: the dashboard reads (70)",
            "  1a. Monthly JSONL: the dashboard reads (70): a second colon",
            // One among the option's words, or with words after it before the reason
            "  1a. Keep (3) copies: a number among its words",
            "  1a. Monthly JSONL (70) and more words: the share ends its words",
            // In inline code, a fence, a table's cell as drawn and as written
            "  `1a. Monthly JSONL (70): an example in code`",
            "  ```",
            "  \u{2502} 1a. Monthly JSONL (70): in a table's cell \u{2502}",
            "  | 1a. Monthly JSONL (70): in a table's cell |",
            // 101 or more, and digits alone
            "  1a. Monthly JSONL (101): past 100",
            "  1a. Monthly JSONL (1000): past 100",
            "  1a. Monthly JSONL (70%): digits alone",
            "  1a. Monthly JSONL (7 0): digits alone",
            "  1a. Monthly JSONL (): digits",
            "  1a. Monthly JSONL (-10): digits",
            // Words before it, and a space before its bracket
            "  1a. (70): no words before it",
            "  1a. Call f(70): no space before its bracket",
            // The label: a number, one lowercase letter, a full stop, a space
            "  1a) Monthly JSONL (70): its stop is a full stop",
            "  1A. Monthly JSONL (70): its letter is lowercase",
            "  1ab. Monthly JSONL (70): one letter",
            "  a. Monthly JSONL (70): a number first",
            "  1234a. Monthly JSONL (70): three digits at most",
            "  1a.Monthly JSONL (70): a space after it",
            "  -1a. Monthly JSONL (70): a bullet has a space after it",
            "",
        ] {
            assert_eq!(share(line), None, "{line:?}");
        }
    }

    /// A harness session's question block gives an option's label and text apart
    /// (@ToolBelt:session-entries, `QuestionItem`'s options), so the share is found in its text
    #[test]
    fn an_options_words_given_apart_from_its_label_give_its_share() {
        let words = "Monthly JSONL (70): the dashboard reads a month at a time";
        assert_eq!(
            share_in(words),
            Some(Share {
                share: 70,
                digits: 15..17
            })
        );
        assert_eq!(share_in("Monthly JSONL: the dashboard reads (70)"), None);
        assert_eq!(share_in("(70): no words before it"), None);
        // Chars, not bytes, for the cells made from them
        let found = share_in("Caf\u{e9} \u{2014} cr\u{e8}me (40): reason").unwrap();
        assert_eq!(found.digits, 14..16);
    }

    fn rule() -> String {
        "\u{2500}".repeat(40)
    }

    /// Claude Code wraps a line longer than the pane onto the rows under it: a paragraph's back
    /// to its left, a list item's under its words. The rows after an option's label are its line
    /// until a blank row or one that starts a block, and its share is placed on the row it
    /// landed on; nothing under Claude Code's input box is read
    #[test]
    fn a_share_claude_code_wrapped_onto_the_next_row_is_found_on_that_row() {
        let lines = vec![
            "\u{25cf} Two ways to keep the counts.".to_owned(),
            "".to_owned(),
            "  Q1. Where should the counts go?".to_owned(),
            "  1a. Monthly JSONL, one file a month, which the dashboard reads".to_owned(),
            "  a month at a time (70): it reads a month at a time and the".to_owned(),
            "  files stay small".to_owned(),
            "  1b. One file for all of it, rewritten each time a count".to_owned(),
            "      changes (30): simpler, but it grows".to_owned(),
            "  1c. A table with a long name and nothing much else (0):".to_owned(),
            "  its reason on the row under it (90)".to_owned(),
            "".to_owned(),
            "  E2. Dim the answered chips (60): an extra has none".to_owned(),
            "  \u{2502} 1a. In a table's cell (70): x \u{2502}".to_owned(),
            "  1d. Its share on a row that starts a block".to_owned(),
            "  - (40): isn't its".to_owned(),
            rule(),
            "  1a. Typed in the box (70): not the reply's".to_owned(),
            rule(),
            "  ? for shortcuts".to_owned(),
        ];
        let screen = Screen::from_lines(&lines.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(
            on_screen(&screen),
            vec![
                Placed {
                    share: 70,
                    row: 4,
                    cols: digits(&lines[4], "(70)"),
                },
                Placed {
                    share: 30,
                    row: 7,
                    cols: digits(&lines[7], "(30)"),
                },
                Placed {
                    share: 0,
                    row: 8,
                    cols: digits(&lines[8], "(0)"),
                },
            ]
        );
    }

    /// What the screen can't tell: an option whose label's row is scrolled off the top has its
    /// share on a row that doesn't say it's an option's; and a code block's lines, which Claude
    /// Code draws bare, are read as the reply's (lbterm/QUESTIONS.md, "The weights")
    #[test]
    fn what_the_screen_cant_tell() {
        let screen = Screen::from_lines(&[
            "  a month at a time (70): it reads a month at a time",
            "  1b. One file (30): simpler",
        ]);
        assert_eq!(
            on_screen(&screen),
            vec![Placed {
                share: 30,
                row: 1,
                cols: 16..18
            }]
        );
        // A fenced block's line, as Claude Code draws it: its fences gone
        let screen = Screen::from_lines(&["  An example of his format:", "", "  1a. Monthly JSONL (70): reason"]);
        assert_eq!(on_screen(&screen).len(), 1);
    }

    /// A row is read with what the row above left it, as the GUI hands it down a pane's rows:
    /// the rest of a wrapped option's line is read as its line's, and a row read alone can't see
    /// the row under it
    #[test]
    fn a_row_is_read_with_what_the_row_above_left_open() {
        let lines = [
            "  1a. Monthly JSONL, one file a month, which the dashboard reads",
            "  a month at a time (70): it reads a month at a time and the",
            "  1b. Keep (3)",
            "  copies (60): the 3 is taken too, since the row can't see the row under it",
            "  1c. No reason (40)",
            "  1d. \u{2014}",
            "  (50): no words before it, so no share",
        ];
        let screen = Screen::from_lines(&lines);
        let row = |i: usize, head| on_row(&screen.rows[i], head);
        let words = Head::Open { words: true };
        let at = |i: usize, share: u8, bracket: &str| {
            Some(OnRow {
                share,
                cols: digits(lines[i], bracket),
            })
        };
        assert_eq!(row(0, Head::Closed), (None, words));
        // The same row with nothing open above it is no option's
        assert_eq!(row(1, Head::Closed), (None, Head::Closed));
        assert_eq!(row(1, words), (at(1, 70, "(70)"), Head::Closed));
        assert_eq!(row(2, Head::Closed), (at(2, 3, "(3)"), words));
        assert_eq!(row(3, words), (at(3, 60, "(60)"), Head::Closed));
        assert_eq!(row(4, Head::Closed), (at(4, 40, "(40)"), words));
        // An open head doesn't run on into a row that starts a block: it's read as its own
        assert_eq!(row(2, words), (at(2, 3, "(3)"), words));
        assert_eq!(row(5, Head::Closed), (None, Head::Open { words: false }));
        assert_eq!(row(6, Head::Open { words: false }), (None, Head::Closed));
        // A blank row closes it
        assert_eq!(on_row(&[], words), (None, Head::Closed));
        // A row's walk keeps its cells only as far as its first ink says it's read on
        assert_eq!(reads_on(' ', Head::Closed), None);
        assert_eq!(reads_on('a', Head::Closed), Some(false));
        assert_eq!(reads_on('1', Head::Closed), Some(true));
        assert_eq!(reads_on('-', Head::Closed), Some(true));
        assert_eq!(reads_on('a', words), Some(true));
        // Run down the screen, as the GUI hands it from row to row
        let found: Vec<u8> = on_screen(&screen).iter().map(|p| p.share).collect();
        assert_eq!(found, vec![70, 3, 60, 40]);
    }

    /// Glyphs keep their columns: a wide glyph before the share moves its columns, not its chars
    #[test]
    fn a_share_is_placed_by_its_glyphs_columns() {
        let mut screen = Screen::from_lines(&["  1a. Wide (70): reason"]);
        // "Wide" drawn as one glyph two columns wide, the rest a column on
        let row = &mut screen.rows[0];
        row[6] = Glyph {
            col: 6,
            ch: '\u{5bbd}',
            width: 2,
        };
        row.remove(7);
        row.remove(7);
        row.remove(7);
        for (i, g) in row.iter_mut().enumerate().skip(7) {
            g.col = i + 1;
        }
        assert_eq!(on_screen(&screen)[0].cols, 10..12);
    }
}
