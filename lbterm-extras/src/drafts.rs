//! Voice drafts (lbterm/CONTROL.md, "Drafts"). Lachlan's pick (2026-10-25, 1a): an answer he says
//! to a Claude session isn't typed into its prompt as he says it. LBTerm holds it as a draft over
//! the pane it's for, drawn above Claude Code's input box, and types it whole when he says "send":
//! each new line as Shift+Enter makes it (ESC CR), then Enter; picks for an AskUserQuestion picker
//! as the keys that make them. "Scratch that" takes the newest piece back, and "edit" types it into
//! the prompt without sending, for him to finish by hand. Earshot hears the words and writes them
//! the way he types answers ("1a", "2. yes", a blank line, "also, ..."); this is what LBTerm does
//! with them.
//!
//! A pane only shows Claude Code's screen, so whether it's safe to type there is read off it
//! ([`bottom`], [`may_send`]): text only into the input box, picks only into the picker they were
//! made for, and never into a permission prompt.

use crate::questions::{is_rule, is_space, words, Glyph, Screen};
use std::ops::Range;

/// A new line in Claude Code's prompt without sending it: what Shift+Enter sends in LBTerm
pub const NEWLINE: &str = "\x1b\r";
/// Sends the prompt
pub const ENTER: &str = "\r";
/// The most a draft holds, in characters: a spoken answer is a few hundred
pub const MAX_CHARS: usize = 8000;
/// The longest a picker's keys go, between one and the next: the picker moves to its next tab
pub const KEY_GAP_MS: u64 = 150;
/// How far above the screen's last written row Claude Code's input box's lower rule can sit: the
/// status line's rows and Claude Code's mode line under it, with room to spare
const FOOTER_ROWS: usize = 12;
/// How tall the input box can grow, as a prompt's typed
const BOX_ROWS_MAX: usize = 30;
/// How many rows up from the screen's last written row a picker's question is looked for
const PICKER_ROWS: usize = 40;
/// How many of a picker question's first words have to be on screen, and the fewest that will do
const PICKER_WORDS: usize = 8;
const PICKER_WORDS_MIN: usize = 2;
/// Words in what a prompt that isn't Claude Code's input box says: a picker's footer, a permission
/// prompt's question
const NOT_A_PROMPT: [&str; 4] = ["Enter to select", "Esc to cancel", "Do you want to", "Tab/Arrow keys"];

/// What a draft holds
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// An answer for Claude Code's prompt: its pieces as they were said, newest last. Together
    /// they're the text, new lines included.
    Text { pieces: Vec<String> },
    /// Picks for an AskUserQuestion picker
    Picker {
        /// What they read as: "1a 2b"
        label: String,
        /// The keys that make them, typed [`KEY_GAP_MS`] apart
        keys: Vec<String>,
        /// The picker's questions, by which it's known on screen
        questions: Vec<String>,
        /// Its questions without a pick yet ("Q3"): a picker takes every answer at once
        missing: Vec<String>,
    },
}

/// One pane's draft
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    /// The pane, by the mux server's id
    pub pane: u64,
    /// The Claude session in the pane when it began: a draft doesn't outlive it
    pub session: Option<String>,
    pub body: Body,
}

/// A line of the draft as its card shows it, and the question it answers
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub question: Option<u32>,
}

/// The number a line of an answer starts with, the way he types them: "1a", "4ab", "2. yes",
/// "2 yes", "3) no"; not "2026 was" or "1.5 s"
pub fn answers(line: &str) -> Option<u32> {
    let t = line.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = &t[digits..];
    let letters = rest.chars().take_while(char::is_ascii_lowercase).count();
    let after = rest[letters..].chars().next();
    let fits = match after {
        None => true,
        Some(' ') | Some(':') => true,
        Some('.') | Some(')') => {
            // "2. yes", "1a." and the end; not "1.5"
            !rest[letters + 1..].starts_with(|c: char| c.is_ascii_digit())
        }
        _ => false,
    };
    fits.then(|| t[..digits].parse().ok()).flatten()
}

impl Draft {
    pub fn new(pane: u64, session: Option<String>) -> Self {
        Self {
            pane,
            session,
            body: Body::Text { pieces: vec![] },
        }
    }

    /// What it says: the text, or a picker's picks
    pub fn text(&self) -> String {
        match &self.body {
            Body::Text { pieces } => pieces.concat(),
            Body::Picker { label, .. } => label.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.body {
            Body::Text { pieces } => pieces.concat().trim().is_empty(),
            Body::Picker { keys, missing, .. } => keys.is_empty() && missing.is_empty(),
        }
    }

    pub fn is_picker(&self) -> bool {
        matches!(self.body, Body::Picker { .. })
    }

    /// The lines its card shows, each with the question it answers
    pub fn lines(&self) -> Vec<Line> {
        match &self.body {
            Body::Text { pieces } => pieces
                .concat()
                .split('\n')
                .map(|text| Line {
                    text: text.to_string(),
                    question: answers(text),
                })
                .collect(),
            Body::Picker { label, .. } => label
                .split_whitespace()
                .map(|pick| Line {
                    text: pick.to_string(),
                    question: answers(pick),
                })
                .collect(),
        }
    }

    /// The questions it answers, by number, each once, in its order
    pub fn answered(&self) -> Vec<u32> {
        let mut out = vec![];
        for n in self.lines().into_iter().filter_map(|l| l.question) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out
    }

    /// Adds a piece of an answer, as Earshot wrote it: new lines as `\n`. A picker's picks give
    /// way to it. Refused when the draft would grow past [`MAX_CHARS`].
    pub fn add(&mut self, piece: &str) -> Result<(), String> {
        let piece = piece.replace("\r\n", "\n").replace('\r', "\n");
        if piece.chars().any(|c| c.is_control() && c != '\n') {
            return Err("a piece of an answer can't hold control characters".to_string());
        }
        if !matches!(self.body, Body::Text { .. }) {
            self.body = Body::Text { pieces: vec![] };
        }
        let Body::Text { pieces } = &mut self.body else {
            unreachable!()
        };
        let have: usize = pieces.iter().map(|p| p.chars().count()).sum();
        if have + piece.chars().count() > MAX_CHARS {
            return Err(format!("a draft holds {MAX_CHARS} characters at most"));
        }
        if !piece.is_empty() {
            pieces.push(piece);
        }
        Ok(())
    }

    /// "Scratch that": the newest piece taken back, or a picker's picks dropped; what went
    pub fn scratch(&mut self) -> Option<String> {
        match &mut self.body {
            Body::Text { pieces } => pieces.pop(),
            Body::Picker { label, .. } => {
                let went = std::mem::take(label);
                self.body = Body::Text { pieces: vec![] };
                Some(went)
            }
        }
    }

    /// What "send" types, each written on its own: the text with Shift+Enter's ESC CR for each
    /// new line, then Enter in a write of its own; a picker's keys, one at a time. Claude Code
    /// takes a long write as a paste, and an Enter inside a paste is a new line, not a send: on
    /// 2026-10-25 an 85-character draft sat in the box with "Removed 1 invisible character", and
    /// Enter written after the text, even under 45 ms after, sent 1,061 characters clean (Earshot's
    /// InMail; measured 2026-10-27 in a Haiku session on 2.1.311)
    pub fn send_keys(&self) -> Vec<String> {
        match &self.body {
            Body::Text { pieces } => {
                let text = pieces.concat().trim_end_matches('\n').replace('\n', NEWLINE);
                vec![text, ENTER.to_string()]
            }
            Body::Picker { keys, .. } => keys.clone(),
        }
    }

    /// What "edit" types: the text without Enter, for him to finish by hand. A picker's picks
    /// have no text to edit.
    pub fn edit_text(&self) -> Option<String> {
        match &self.body {
            Body::Text { pieces } => Some(pieces.concat().trim_end_matches('\n').replace('\n', NEWLINE)),
            Body::Picker { .. } => None,
        }
    }
}

/// What's at the bottom of a pane, for typing into it
#[derive(Debug, Clone, PartialEq)]
pub enum Bottom {
    /// Claude Code's input box: the rows between its two rules
    Prompt(Range<usize>),
    /// Something else: a picker, a permission prompt, another program
    Other,
}

/// The last row with anything written on it
fn last_ink(screen: &Screen) -> Option<usize> {
    screen.rows.iter().rposition(|row| row.iter().any(|g| !is_space(g.ch)))
}

fn text_of(row: &[Glyph]) -> String {
    row.iter().map(|g| g.ch).collect()
}

/// The two rules nearest the bottom, the lower within [`FOOTER_ROWS`] of the last written row and
/// the upper within [`BOX_ROWS_MAX`] above it: Claude Code's input box, or what took its place
fn box_rules(screen: &Screen) -> Option<(usize, usize)> {
    let last = last_ink(screen)?;
    let floor = (last + 1).saturating_sub(FOOTER_ROWS);
    let lower = (floor..=last).rev().find(|&r| is_rule(&screen.rows[r]))?;
    let top = lower.saturating_sub(BOX_ROWS_MAX);
    let upper = (top..lower).rev().find(|&r| is_rule(&screen.rows[r]))?;
    (lower > upper + 1).then_some((upper, lower))
}

/// Whether Claude Code's input box is at the bottom of the screen: two rules with its prompt mark
/// (❯) starting the first row between them, and nothing a picker or a permission prompt says
pub fn bottom(screen: &Screen) -> Bottom {
    let Some((upper, lower)) = box_rules(screen) else {
        return Bottom::Other;
    };
    let first = text_of(&screen.rows[upper + 1]);
    let first = first.trim_start_matches(|c: char| is_space(c) || c == '\u{2502}');
    if !(first.starts_with('\u{276f}') || first.starts_with('>')) {
        return Bottom::Other;
    }
    let last = last_ink(screen).unwrap_or(lower);
    // From under the upper rule, which can hold the conversation's name: a name with a picker's
    // words in it isn't a picker
    let tail: String = screen.rows[upper + 1..=last].iter().map(|r| text_of(r) + "\n").collect();
    if NOT_A_PROMPT.iter().any(|w| tail.contains(w)) {
        return Bottom::Other;
    }
    Bottom::Prompt(upper + 1..lower)
}

/// The row a draft's card sits above: the input box's upper rule, or what took its place (a
/// picker's, a permission prompt's), else under the last written row
pub fn anchor_row(screen: &Screen) -> usize {
    match box_rules(screen) {
        Some((upper, _)) => upper,
        None => last_ink(screen).map_or(screen.rows.len(), |r| r + 1),
    }
}

/// Whether the screen shows the picker asking `questions`, at its first question: no input box,
/// and that question's first words near the bottom. The picks are typed from the first question,
/// so a picker moved on by hand doesn't count.
pub fn shows_picker(screen: &Screen, questions: &[String]) -> bool {
    if matches!(bottom(screen), Bottom::Prompt(_)) {
        return false;
    }
    let Some(first) = questions.first() else {
        return false;
    };
    let want: Vec<String> = words(first).into_iter().take(PICKER_WORDS).collect();
    if want.len() < PICKER_WORDS_MIN {
        return false;
    }
    let Some(last) = last_ink(screen) else {
        return false;
    };
    let from = (last + 1).saturating_sub(PICKER_ROWS);
    let on: Vec<String> = screen.rows[from..=last]
        .iter()
        .flat_map(|r| words(&text_of(r)))
        .collect();
    on.windows(want.len()).any(|w| w == want.as_slice())
}

/// Whether "send" may type `draft` into a pane showing `screen`, or why not
pub fn may_send(draft: &Draft, screen: &Screen) -> Result<(), String> {
    if draft.is_empty() {
        return Err("there's nothing in the draft to send".to_string());
    }
    match &draft.body {
        Body::Text { .. } => match bottom(screen) {
            Bottom::Prompt(_) => Ok(()),
            Bottom::Other => Err("Claude Code's prompt isn't showing there: a picker or a permission prompt has its place".to_string()),
        },
        Body::Picker {
            missing, questions, ..
        } => {
            if !missing.is_empty() {
                return Err(format!(
                    "no pick yet for {}: a picker takes every answer at once",
                    missing.join(", ")
                ));
            }
            if shows_picker(screen, questions) {
                Ok(())
            } else {
                Err("the picker those picks are for isn't on screen at its first question".to_string())
            }
        }
    }
}

/// Whether "edit" may type `draft` into a pane showing `screen`, or why not
pub fn may_edit(draft: &Draft, screen: &Screen) -> Result<(), String> {
    if draft.is_picker() {
        return Err("a picker's picks have no text to edit: say send, or scratch that".to_string());
    }
    if draft.is_empty() {
        return Err("there's nothing in the draft to edit".to_string());
    }
    match bottom(screen) {
        Bottom::Prompt(_) => Ok(()),
        Bottom::Other => Err("Claude Code's prompt isn't showing there".to_string()),
    }
}

/// Whether a draft whose send failed part way (a key of it didn't get there) goes back over its
/// pane (Lachlan's 3a of 2026-11-08: a lost dictation is the costliest thing to retype): unless
/// another has been started over the pane since, which is his newer word
pub fn put_back(over_the_pane_now: Option<&Draft>, sent: &Draft) -> bool {
    !sent.is_empty() && over_the_pane_now.map_or(true, |now| now.is_empty())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_failed_send_puts_the_draft_back_unless_a_newer_one_is_there() {
        let mut sent = Draft::new(3, Some("s".into()));
        sent.add("fix the build").unwrap();
        // Nothing over the pane since: back it goes
        assert!(put_back(None, &sent));
        // An empty draft over the pane is nothing of his
        assert!(put_back(Some(&Draft::new(3, Some("s".into()))), &sent));
        // He has begun another since: that one stays
        let mut newer = Draft::new(3, Some("s".into()));
        newer.add("and the tests").unwrap();
        assert!(!put_back(Some(&newer), &sent));
        // An empty send has nothing to put back
        assert!(!put_back(None, &Draft::new(3, None)));
    }

    const RULE: &str = "──────────────────────────────────────────";

    fn screen(lines: &[&str]) -> Screen {
        Screen::from_lines(lines)
    }

    /// Claude Code with its prompt showing: the reply, the box, the status line, and empty rows
    /// under them, as a short conversation leaves them
    fn prompt() -> Vec<&'static str> {
        vec![
            "● Two things.",
            "",
            "  Q1. Rename it?",
            "  Q2. Ship it?",
            "",
            RULE,
            "❯ Try \"edit <filepath> to...\"",
            RULE,
            "  [Opus 5.5] LBTerm | two things",
            "  ⏵⏵ auto mode on",
            "",
            "",
        ]
    }

    fn text_draft(pieces: &[&str]) -> Draft {
        Draft {
            pane: 3,
            session: Some("s".into()),
            body: Body::Text {
                pieces: pieces.iter().map(|p| p.to_string()).collect(),
            },
        }
    }

    fn picker_draft(missing: &[&str]) -> Draft {
        Draft {
            pane: 3,
            session: None,
            body: Body::Picker {
                label: "1b 2a".into(),
                keys: vec!["2".into(), "1".into(), "1".into()],
                questions: vec!["Which colour should the pad take?".into(), "And its ring?".into()],
                missing: missing.iter().map(|m| m.to_string()).collect(),
            },
        }
    }

    #[test]
    fn an_answer_line_starts_with_its_question_number() {
        for (line, n) in [("1a", 1), ("4ab", 4), ("2. yes", 2), ("2 yes", 2), ("3) no", 3), ("12: later", 12), ("  6. yes", 6), ("1a.", 1), ("7", 7)] {
            assert_eq!(answers(line), Some(n), "{line}");
        }
        for line in ["also, check the build", "2026 was", "1.5 s", "", "x1", "1A", "1234 rows"] {
            assert_eq!(answers(line), None, "{line}");
        }
    }

    #[test]
    fn pieces_make_the_text_its_lines_and_what_it_answers() {
        let mut d = Draft::new(3, Some("s".into()));
        assert!(d.is_empty());
        d.add("1a").unwrap();
        d.add("\n2. yes").unwrap();
        d.add("\n\nalso, check the build").unwrap();
        assert_eq!(d.text(), "1a\n2. yes\n\nalso, check the build");
        assert_eq!(
            d.lines(),
            vec![
                Line { text: "1a".into(), question: Some(1) },
                Line { text: "2. yes".into(), question: Some(2) },
                Line { text: "".into(), question: None },
                Line { text: "also, check the build".into(), question: None },
            ]
        );
        assert_eq!(d.answered(), vec![1, 2]);
        // Windows' new lines are one, and nothing else that's a control character gets in
        d.add("\r\n3. no").unwrap();
        assert_eq!(d.answered(), vec![1, 2, 3]);
        assert!(d.add("\x1b[2J").is_err());
        assert!(d.add(&"x".repeat(MAX_CHARS)).is_err());
    }

    #[test]
    fn scratch_takes_the_newest_piece_back() {
        let mut d = text_draft(&["1a", "\n2. yes"]);
        assert_eq!(d.scratch().as_deref(), Some("\n2. yes"));
        assert_eq!(d.text(), "1a");
        assert_eq!(d.scratch().as_deref(), Some("1a"));
        assert!(d.is_empty());
        assert_eq!(d.scratch(), None);

        let mut p = picker_draft(&[]);
        assert_eq!(p.scratch().as_deref(), Some("1b 2a"));
        assert!(p.is_empty() && !p.is_picker());
    }

    #[test]
    fn send_types_shift_enter_between_lines_then_enter() {
        let d = text_draft(&["1a", "\n2. yes", "\n\nalso, check the build\n"]);
        assert_eq!(d.send_keys(), vec!["1a\x1b\r2. yes\x1b\r\x1b\ralso, check the build", "\r"], "Enter in a write of its own");
        assert_eq!(d.edit_text().as_deref(), Some("1a\x1b\r2. yes\x1b\r\x1b\ralso, check the build"));
        let p = picker_draft(&[]);
        assert_eq!(p.send_keys(), vec!["2", "1", "1"]);
        assert_eq!(p.edit_text(), None);
        assert_eq!(p.answered(), vec![1, 2]);
        assert_eq!(p.lines().len(), 2);
    }

    #[test]
    fn the_prompt_is_found_between_its_rules_under_empty_rows() {
        let s = screen(&prompt());
        assert_eq!(bottom(&s), Bottom::Prompt(6..7));
        assert_eq!(anchor_row(&s), 5);
        // Grown with what's typed
        let mut tall = prompt();
        tall.splice(7..7, ["  second line", "  third line"]);
        assert_eq!(bottom(&screen(&tall)), Bottom::Prompt(6..9));
    }

    /// A named conversation's upper rule as Claude Code 2.1.314 to 2.1.316 draws it: the name
    /// near the right end, one dash after it (read off a real pane, 2026-11-01)
    const NAMED: &str = "──────────────────────────────────────────────────────────── LBTerm InMail ─";

    #[test]
    fn a_named_conversations_prompt_is_still_the_prompt() {
        let mut named = prompt();
        named[5] = NAMED;
        let s = screen(&named);
        assert_eq!(bottom(&s), Bottom::Prompt(6..7));
        assert_eq!(anchor_row(&s), 5, "the card sits above the named rule");
        assert!(may_send(&text_draft(&["1a", "\n2a"]), &s).is_ok());
        assert!(may_edit(&text_draft(&["1a"]), &s).is_ok());
        assert!(!shows_picker(&s, &["Which colour should the pad take?".into()]));
        // A name with a picker's words in it is still a name
        named[5] = "──────────────────────────────────────── Do you want to rename it ─";
        assert_eq!(bottom(&screen(&named)), Bottom::Prompt(6..7));
    }

    #[test]
    fn a_permission_prompt_or_a_picker_isnt_the_prompt() {
        let permission = screen(&[
            "● I'll remove it.",
            RULE,
            " Bash command",
            "   rm -rf build",
            " Do you want to proceed?",
            " ❯ 1. Yes",
            "   2. No, and tell Claude what to do differently (esc)",
        ]);
        assert_eq!(bottom(&permission), Bottom::Other);
        assert!(may_send(&text_draft(&["1a"]), &permission).is_err());

        // Its own rules, one above it and one before "Chat about this"
        let picker = screen(&picker_lines());
        assert_eq!(bottom(&picker), Bottom::Other);
        assert_eq!(anchor_row(&picker), 1);

        // Two rules round something starting with ❯ but saying a picker's words
        let sneaky = screen(&[RULE, "❯ 1. Yes", RULE, "Enter to select · Esc to cancel"]);
        assert_eq!(bottom(&sneaky), Bottom::Other);
        // No rules at all: another program
        assert_eq!(bottom(&screen(&["PS C:\\> ", ""])), Bottom::Other);
    }

    fn picker_lines() -> Vec<&'static str> {
        vec![
            "● Two choices.",
            RULE,
            " ←  ☐ Colour  ☐ Ring  ✔ Submit  →",
            "",
            "Which colour should the pad take?",
            "",
            "❯ 1. Teal",
            "  2. Amber",
            "  3. Type something.",
            RULE,
            "  4. Chat about this",
            "",
            "Enter to select · Tab/Arrow keys to navigate · Esc to cancel",
        ]
    }

    #[test]
    fn text_goes_only_into_the_prompt_and_picks_only_into_their_picker() {
        let at_prompt = screen(&prompt());
        let at_picker = screen(&picker_lines());
        assert_eq!(may_send(&text_draft(&["1a"]), &at_prompt), Ok(()));
        assert!(may_send(&text_draft(&["1a"]), &at_picker).unwrap_err().contains("prompt isn't showing"));
        assert_eq!(may_send(&picker_draft(&[]), &at_picker), Ok(()));
        assert!(may_send(&picker_draft(&[]), &at_prompt).unwrap_err().contains("isn't on screen"));
        assert!(may_send(&picker_draft(&["Q2"]), &at_picker).unwrap_err().contains("no pick yet for Q2"));
        assert!(may_send(&text_draft(&[]), &at_prompt).unwrap_err().contains("nothing"));

        // Moved on to its second question by hand: the picks would land on the wrong ones
        let mut moved = picker_lines();
        moved[4] = "And its ring?";
        assert!(may_send(&picker_draft(&[]), &screen(&moved)).is_err());

        assert_eq!(may_edit(&text_draft(&["1a"]), &at_prompt), Ok(()));
        assert!(may_edit(&text_draft(&["1a"]), &at_picker).is_err());
        assert!(may_edit(&picker_draft(&[]), &at_picker).is_err());
    }
}
