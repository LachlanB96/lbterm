//! Voice drafts (lbterm/CONTROL.md, "Drafts"). Lachlan's pick (2026-10-25, 1a): an answer he says
//! to a Claude session isn't typed into its prompt as he says it. LBTerm holds it as a draft over
//! the pane it's for, drawn above Claude Code's input box, and types it whole when he says "send":
//! each new line as Shift+Enter makes it (ESC CR), then Enter. "Scratch that" takes the newest piece back, and "edit" types it into
//! the prompt without sending, for him to finish by hand. LyreBird hears the words and writes them
//! the way he types answers ("1a", "2. yes", a blank line, "also, ..."); this is what LBTerm does
//! with them.
//!
//! A pane only shows Claude Code's screen, so whether it's safe to type there is read off it
//! ([`bottom`], [`may_send`]): text only into the input box, never into a permission prompt or
//! a menu. Drafts made picks for Claude Code's AskUserQuestion picker until 2026-11-14, when
//! Lachlan had the picker's code taken out everywhere (his 2a; AskUserQuestion is denied in his
//! settings since 2026-11-12).

use crate::questions::{is_rule, is_space, Glyph, Screen};
use std::ops::Range;

/// A new line in Claude Code's prompt without sending it: what Shift+Enter sends in LBTerm
pub const NEWLINE: &str = "\x1b\r";
/// Sends the prompt
pub const ENTER: &str = "\r";
/// The most a draft holds, in characters: a spoken answer is a few hundred
pub const MAX_CHARS: usize = 8000;
/// The gap between one write of typed keys and the next (a draft's text and its Enter, Compact
/// now's `/compact` and its Enter): Claude Code takes an Enter written with its text as a new
/// line of a paste
pub const KEY_GAP_MS: u64 = 150;
/// How far above the screen's last written row Claude Code's input box's lower rule can sit: the
/// status line's rows and Claude Code's mode line under it, with room to spare
const FOOTER_ROWS: usize = 12;
/// How tall the input box can grow, as a prompt's typed
const BOX_ROWS_MAX: usize = 30;
/// Words in what a prompt that isn't Claude Code's input box says: a menu's footer, a permission
/// prompt's question
const NOT_A_PROMPT: [&str; 4] = ["Enter to select", "Esc to cancel", "Do you want to", "Tab/Arrow keys"];

/// One pane's draft
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    /// The pane, by the mux server's id
    pub pane: u64,
    /// The Claude session in the pane when it began: a draft doesn't outlive it
    pub session: Option<String>,
    /// An answer for Claude Code's prompt: its pieces as they were said, newest last. Together
    /// they're the text, new lines included.
    pub pieces: Vec<String>,
}

/// A line of the draft as its card shows it, and the question it answers
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub question: Option<u32>,
}

/// The number a line of an answer starts with, the way he types them: "1a", "4ab", "2. yes",
/// "2 yes", "3) no", "2+", "4-", "1+ but more salt"; not "2026 was", "1.5 s" or "2-3 days"
pub fn answers(line: &str) -> Option<u32> {
    pick(line).map(|p| p.number)
}

/// What he says of a question by a mark after its number (Blueprint's rules for asking him)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// `+`
    Yes,
    /// `-`: an extra answered so is declined
    No,
    /// `?`: he wants more said, and a recommendation
    Unsure,
    /// `??`: he has no idea what it's about
    Lost,
    /// `?!`: something said looks badly wrong to him
    Shocked,
    /// `//`: filed for later
    Later,
}

impl Mark {
    /// The mark `text` starts with, and how many bytes it takes
    fn read(text: &str) -> Option<(Self, usize)> {
        let marks = [("??", Mark::Lost), ("?!", Mark::Shocked), ("//", Mark::Later), ("?", Mark::Unsure), ("+", Mark::Yes), ("-", Mark::No)];
        marks.iter().find(|(m, _)| text.starts_with(m)).map(|(m, mark)| (*mark, m.len()))
    }
}

/// One question named in an answer: its number, the options picked by their letters ("deg" for
/// 3deg, which picks 3d, 3e and 3g), and the mark after them. Whatever follows is his words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub number: u32,
    pub options: String,
    pub mark: Option<Mark>,
}

impl Pick {
    /// It settles the question: anything but asking back (`?`, `??`, `?!`), which leaves it
    /// waiting on Claude to say more and ask again
    pub fn settles(&self) -> bool {
        !matches!(self.mark, Some(Mark::Unsure | Mark::Lost | Mark::Shocked))
    }
}

/// The question a line of an answer names first, and how many bytes of it that took, the way he
/// types them: a number of up to three digits, the options' letters, a mark, then the line's end,
/// a space or a stop (`2.`, `2:`, `2,`, `3)`), never a digit after a stop or a dash
fn pick_at(t: &str) -> Option<(Pick, usize)> {
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = &t[digits..];
    let letters = rest.chars().take_while(char::is_ascii_lowercase).count();
    let after = &rest[letters..];
    let (mark, marked) = Mark::read(after).map_or((None, 0), |(m, n)| (Some(m), n));
    let tail = &after[marked..];
    let fits = match tail.chars().next() {
        None => true,
        Some(c) if c.is_whitespace() => true,
        Some(':' | ',' | ';') => true,
        // "2. yes", "1a." and the end; not "1.5"
        Some('.' | ')') => !tail[1..].starts_with(|c: char| c.is_ascii_digit()),
        // "2-3 days" is a range, not 2 declined, and "1/2" a half
        _ => false,
    };
    fits.then_some(())?;
    let number = t[..digits].parse().ok()?;
    let pick = Pick { number, options: rest[..letters].to_string(), mark };
    Some((pick, digits + letters + marked))
}

/// The question a line of an answer names first: "1a", "2. yes", "2+", "3deg", "4-",
/// "1+ but more salt", "2b, and keep it"
pub fn pick(line: &str) -> Option<Pick> {
    pick_at(line.trim_start()).map(|(p, _)| p)
}

/// The questions a line of an answer names: its first, and every one after it when the line is
/// nothing but them ("1a 2b", "1a, 2+"), as a picker's picks read. His words after one end it,
/// so "1+ but use 2 cores" names 1 alone.
pub fn picks(line: &str) -> Vec<Pick> {
    let mut out = vec![];
    let mut rest = line.trim_start();
    while let Some((pick, len)) = pick_at(rest) {
        out.push(pick);
        let after = rest[len..].trim_start_matches(|c: char| c == ',' || c == ';' || c.is_whitespace());
        if !only_picks(after) {
            break;
        }
        rest = after;
    }
    out
}

/// `text` is one or more picks and nothing else, apart by spaces or commas
fn only_picks(text: &str) -> bool {
    let mut rest = text;
    if rest.is_empty() {
        return false;
    }
    loop {
        let Some((_, len)) = pick_at(rest) else {
            return false;
        };
        let tail = &rest[len..];
        // A stop or a colon after a pick ("2. yes", "1:") starts his words
        if tail.starts_with(['.', ':', ')']) {
            return tail[1..].trim().is_empty();
        }
        rest = tail.trim_start_matches(|c: char| c == ',' || c == ';' || c.is_whitespace());
        if rest.is_empty() {
            return true;
        }
    }
}

/// Every question an answer names, line by line, in its order: what a prompt of his answers
pub fn picks_in(text: &str) -> Vec<Pick> {
    text.lines().flat_map(picks).collect()
}

impl Draft {
    pub fn new(pane: u64, session: Option<String>) -> Self {
        Self {
            pane,
            session,
            pieces: vec![],
        }
    }

    /// What it says
    pub fn text(&self) -> String {
        self.pieces.concat()
    }

    pub fn is_empty(&self) -> bool {
        self.text().trim().is_empty()
    }

    /// The lines its card shows, each with the question it answers
    pub fn lines(&self) -> Vec<Line> {
        self.text()
            .split('\n')
            .map(|text| Line {
                text: text.to_string(),
                question: answers(text),
            })
            .collect()
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

    /// Adds a piece of an answer, as LyreBird wrote it: new lines as `\n`. Refused when the draft
    /// would grow past [`MAX_CHARS`].
    pub fn add(&mut self, piece: &str) -> Result<(), String> {
        let piece = piece.replace("\r\n", "\n").replace('\r', "\n");
        if piece.chars().any(|c| c.is_control() && c != '\n') {
            return Err("a piece of an answer can't hold control characters".to_string());
        }
        let pieces = &mut self.pieces;
        let have: usize = pieces.iter().map(|p| p.chars().count()).sum();
        if have + piece.chars().count() > MAX_CHARS {
            return Err(format!("a draft holds {MAX_CHARS} characters at most"));
        }
        if !piece.is_empty() {
            pieces.push(piece);
        }
        Ok(())
    }

    /// "Scratch that": the newest piece taken back; what went
    pub fn scratch(&mut self) -> Option<String> {
        self.pieces.pop()
    }

    /// What "send" types, each written on its own: the text with Shift+Enter's ESC CR for each
    /// new line, then Enter in a write of its own. Claude Code
    /// takes a long write as a paste, and an Enter inside a paste is a new line, not a send: on
    /// 2026-10-25 an 85-character draft sat in the box with "Removed 1 invisible character", and
    /// Enter written after the text, even under 45 ms after, sent 1,061 characters clean (Earshot's
    /// InMail; measured 2026-10-27 in a Haiku session on 2.1.311)
    pub fn send_keys(&self) -> Vec<String> {
        vec![self.edit_text(), ENTER.to_string()]
    }

    /// What "edit" types: the text without Enter, for him to finish by hand
    pub fn edit_text(&self) -> String {
        self.text().trim_end_matches('\n').replace('\n', NEWLINE)
    }
}

/// What's at the bottom of a pane, for typing into it
#[derive(Debug, Clone, PartialEq)]
pub enum Bottom {
    /// Claude Code's input box: the rows between its two rules
    Prompt(Range<usize>),
    /// Something else: a permission prompt, a menu, another program
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
/// (❯) starting the first row between them, and nothing a menu or a permission prompt says
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
    // From under the upper rule, which can hold the conversation's name: a name with a menu's
    // words in it isn't a menu
    let tail: String = screen.rows[upper + 1..=last].iter().map(|r| text_of(r) + "\n").collect();
    if NOT_A_PROMPT.iter().any(|w| tail.contains(w)) {
        return Bottom::Other;
    }
    Bottom::Prompt(upper + 1..lower)
}

/// The row a draft's card sits above: the input box's upper rule, or what took its place (a
/// menu's, a permission prompt's), else under the last written row
pub fn anchor_row(screen: &Screen) -> usize {
    match box_rules(screen) {
        Some((upper, _)) => upper,
        None => last_ink(screen).map_or(screen.rows.len(), |r| r + 1),
    }
}

/// Whether "send" may type `draft` into a pane showing `screen`, or why not
pub fn may_send(draft: &Draft, screen: &Screen) -> Result<(), String> {
    if draft.is_empty() {
        return Err("there's nothing in the draft to send".to_string());
    }
    match bottom(screen) {
        Bottom::Prompt(_) => Ok(()),
        Bottom::Other => Err("Claude Code's prompt isn't showing there: a permission prompt or a menu has its place".to_string()),
    }
}

/// Whether "edit" may type `draft` into a pane showing `screen`, or why not
pub fn may_edit(draft: &Draft, screen: &Screen) -> Result<(), String> {
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
            pieces: pieces.iter().map(|p| p.to_string()).collect(),
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

    fn p(number: u32, options: &str, mark: Option<Mark>) -> Pick {
        Pick { number, options: options.into(), mark }
    }

    #[test]
    fn his_shorthand_names_the_question_its_options_and_his_mark() {
        use Mark::*;
        for (line, want) in [
            ("1a", p(1, "a", None)),
            ("2.", p(2, "", None)),
            ("2. explain more", p(2, "", None)),
            ("2+", p(2, "", Some(Yes))),
            ("3deg", p(3, "deg", None)),
            ("1+ but add more salt", p(1, "", Some(Yes))),
            ("4-", p(4, "", Some(No))),
            ("12- not now", p(12, "", Some(No))),
            ("2?", p(2, "", Some(Unsure))),
            ("2??", p(2, "", Some(Lost))),
            ("2?! that can't be right", p(2, "", Some(Shocked))),
            ("5//", p(5, "", Some(Later))),
            ("2b, but also allow it as a setting", p(2, "b", None)),
            ("2, I did not change them", p(2, "", None)),
            ("  11b", p(11, "b", None)),
        ] {
            assert_eq!(pick(line), Some(want.clone()), "{line}");
            assert_eq!(answers(line), Some(want.number), "{line}");
        }
        for line in ["2-3 days", "1/2 of them", "2+3", "1.5x", "Q1 yes", "yes"] {
            assert_eq!(pick(line), None, "{line}");
        }
        assert!(p(1, "a", None).settles() && p(4, "", Some(No)).settles() && p(5, "", Some(Later)).settles());
        assert!(!p(2, "", Some(Unsure)).settles() && !p(2, "", Some(Lost)).settles() && !p(2, "", Some(Shocked)).settles());
    }

    #[test]
    fn a_line_of_picks_names_each_and_his_words_end_them() {
        use Mark::*;
        assert_eq!(picks("1a 2b"), vec![p(1, "a", None), p(2, "b", None)]);
        assert_eq!(picks("1a, 2+, 3-"), vec![p(1, "a", None), p(2, "", Some(Yes)), p(3, "", Some(No))]);
        assert_eq!(picks("1+ but use 2 cores"), vec![p(1, "", Some(Yes))]);
        assert_eq!(picks("1a 2b and more"), vec![p(1, "a", None)]);
        assert_eq!(picks("1. 2 more tests"), vec![p(1, "", None)]);
        assert_eq!(picks("we said 2b"), vec![]);
        // A prompt: each line's, in order, past his words before them
        let prompt = "If I use a reset, the limit should go up too\n\n1+\n2- not now\n4. explain more\n3deg";
        assert_eq!(
            picks_in(prompt),
            vec![p(1, "", Some(Yes)), p(2, "", Some(No)), p(4, "", None), p(3, "deg", None)]
        );
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
    }

    #[test]
    fn send_types_shift_enter_between_lines_then_enter() {
        let d = text_draft(&["1a", "\n2. yes", "\n\nalso, check the build\n"]);
        assert_eq!(d.send_keys(), vec!["1a\x1b\r2. yes\x1b\r\x1b\ralso, check the build", "\r"], "Enter in a write of its own");
        assert_eq!(d.edit_text(), "1a\x1b\r2. yes\x1b\r\x1b\ralso, check the build");
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
        // A name with a menu's words in it is still a name
        named[5] = "──────────────────────────────────────── Do you want to rename it ─";
        assert_eq!(bottom(&screen(&named)), Bottom::Prompt(6..7));
    }

    #[test]
    fn a_permission_prompt_or_a_menu_isnt_the_prompt() {
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

        // A menu with its own rules, one above it and one before "Chat about this" (AskUserQuestion's
        // picker, as Claude Code 2.1.311 drew it)
        let picker = screen(&picker_lines());
        assert_eq!(bottom(&picker), Bottom::Other);
        assert_eq!(anchor_row(&picker), 1);

        // Two rules round something starting with ❯ but saying a menu's words
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
    fn text_goes_only_into_the_prompt() {
        let at_prompt = screen(&prompt());
        let at_menu = screen(&picker_lines());
        assert_eq!(may_send(&text_draft(&["1a"]), &at_prompt), Ok(()));
        assert!(may_send(&text_draft(&["1a"]), &at_menu).unwrap_err().contains("prompt isn't showing"));
        assert!(may_send(&text_draft(&[]), &at_prompt).unwrap_err().contains("nothing"));
        assert_eq!(may_edit(&text_draft(&["1a"]), &at_prompt), Ok(()));
        assert!(may_edit(&text_draft(&["1a"]), &at_menu).is_err());
    }
}
