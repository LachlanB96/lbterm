//! A box being typed in: its words, the caret, a selection, and how the words wrap in it. The
//! view's box at the bottom (Enter sends, Shift+Enter a new line) and the small boxes on a
//! waiting session's card are each one of these.
//!
//! Places are byte offsets into the words, always on a character's boundary.

/// One line of the box as it shows: `start..end` of the words, without the line break that ends
/// it (a wrapped line's end is the next one's start)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visual {
    pub start: usize,
    pub end: usize,
    /// It was wrapped, not ended by a line break
    pub soft: bool,
}

/// The words of `text` in lines `width` columns wide: broken after a space where one fits,
/// else where the line is full, and at every line break
pub fn wrap(text: &str, width: usize, cols: impl Fn(char) -> usize) -> Vec<Visual> {
    let width = width.max(1);
    let mut out = vec![];
    let mut start = 0;
    for (n, piece) in text.split('\n').enumerate() {
        let base = start;
        let mut line_start = base;
        let mut w = 0;
        // After the last space on the line: where a wrap would go
        let mut last_break: Option<(usize, usize)> = None;
        for (i, c) in piece.char_indices() {
            let at = base + i;
            let cw = cols(c);
            if w + cw > width && at > line_start {
                let (end, carried) = match last_break {
                    Some((b, bw)) if b > line_start => (b, w - bw),
                    _ => (at, 0),
                };
                out.push(Visual {
                    start: line_start,
                    end,
                    soft: true,
                });
                line_start = end;
                w = carried;
                last_break = None;
            }
            w += cw;
            if c == ' ' {
                last_break = Some((at + 1, w));
            }
        }
        out.push(Visual {
            start: line_start,
            end: base + piece.len(),
            soft: false,
        });
        let _ = n;
        start = base + piece.len() + 1;
    }
    out
}

/// The line and column `at` shows on: at a wrapped line's end, the next line's start
pub fn place_of(text: &str, lines: &[Visual], at: usize, cols: impl Fn(char) -> usize) -> (usize, usize) {
    for (i, line) in lines.iter().enumerate() {
        let last = i + 1 == lines.len();
        if at >= line.start && (at < line.end || (at == line.end && (!line.soft || last))) {
            let col = text[line.start..at].chars().map(&cols).sum();
            return (i, col);
        }
    }
    let i = lines.len().saturating_sub(1);
    let line = lines.get(i).copied().unwrap_or(Visual {
        start: 0,
        end: 0,
        soft: false,
    });
    (i, text[line.start..line.end.min(text.len())].chars().map(&cols).sum())
}

/// The place nearest column `col` of line `line`
pub fn index_at(text: &str, lines: &[Visual], line: usize, col: usize, cols: impl Fn(char) -> usize) -> usize {
    let Some(l) = lines.get(line.min(lines.len().saturating_sub(1))) else {
        return 0;
    };
    // A wrapped line's caret can't sit after its last character: that's the next line's start
    let end = if l.soft {
        text[l.start..l.end].char_indices().last().map_or(l.start, |(i, _)| l.start + i)
    } else {
        l.end
    };
    let mut w = 0;
    for (i, c) in text[l.start..end].char_indices() {
        let cw = cols(c);
        if w + cw / 2 >= col && w + cw > col {
            return l.start + i;
        }
        w += cw;
        if w > col {
            return l.start + i;
        }
    }
    end
}

/// Whether an answer's words hold `code` as a word of its own ("1b", "E2+"), a comma after it
/// or not
pub fn has_code(words: &str, code: &str) -> bool {
    words.split_whitespace().any(|w| w.trim_end_matches(',') == code)
}

/// An answer's words with a chip's `code` toggled: there, it goes, and its line with it when
/// nothing else is on it; not there, it goes on a line of its own at the end, as Relay's phone
/// puts each tapped code (Lachlan's E3 in Relay, 2026-10-29). An extra's yes or no (`E2+`,
/// `E2-`) replaces the other one for the same extra where it is. Lines it doesn't touch stay as
/// they were typed.
pub fn toggle_code(words: &str, code: &str) -> String {
    let had = has_code(words, code);
    let other = match code.strip_suffix('+') {
        Some(stem) => Some(format!("{stem}-")),
        None => code.strip_suffix('-').map(|stem| format!("{stem}+")),
    };
    let is_it = |w: &str| {
        let w = w.trim_end_matches(',');
        w == code || other.as_deref() == Some(w)
    };
    // Pressed again, every one of it goes; else an extra's other answer takes it, in its place
    let mut placed = had;
    let mut lines: Vec<String> = vec![];
    for line in words.split('\n') {
        if !line.split_whitespace().any(is_it) {
            lines.push(line.to_string());
            continue;
        }
        let mut kept: Vec<String> = vec![];
        for w in line.split_whitespace() {
            if !is_it(w) {
                kept.push(w.to_string());
            } else if !placed {
                kept.push(format!("{code}{}", &w[w.trim_end_matches(',').len()..]));
                placed = true;
            }
        }
        if !kept.is_empty() {
            lines.push(kept.join(" "));
        }
    }
    let text = lines.join("\n");
    if had {
        // Its line gone from the end leaves no empty line after the rest
        return text.trim_end_matches('\n').to_string();
    }
    if placed {
        return text;
    }
    let body = text.trim_end();
    if body.is_empty() {
        code.to_string()
    } else {
        format!("{body}\n{code}")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Input {
    text: String,
    caret: usize,
    /// Where a selection started; the caret is its other end
    anchor: Option<usize>,
    /// The column up and down keep to
    goal: Option<usize>,
}

fn prev(text: &str, at: usize) -> usize {
    text[..at].char_indices().last().map_or(0, |(i, _)| i)
}

fn next(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Input {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The selection, start before end, when there's one
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    pub fn selected(&self) -> Option<&str> {
        self.selection().map(|(a, b)| &self.text[a..b])
    }

    /// The words, all of them, the caret at their end
    pub fn set(&mut self, text: &str) {
        self.text = crate::seat::text::clean(text);
        self.caret = self.text.len();
        self.anchor = None;
        self.goal = None;
    }

    /// The words, the box emptied
    pub fn take(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        *self = Self::default();
        text
    }

    fn cut_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.text.replace_range(a..b, "");
                self.caret = a;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    /// Typed or pasted words, over the selection: line breaks kept as one kind, tabs as spaces,
    /// other control characters left out
    pub fn insert(&mut self, words: &str) {
        let words = crate::seat::text::clean(words);
        self.cut_selection();
        self.text.insert_str(self.caret, &words);
        self.caret += words.len();
        self.goal = None;
    }

    pub fn backspace(&mut self) {
        if !self.cut_selection() && self.caret > 0 {
            let from = prev(&self.text, self.caret);
            self.text.replace_range(from..self.caret, "");
            self.caret = from;
        }
        self.goal = None;
    }

    pub fn delete(&mut self) {
        if !self.cut_selection() && self.caret < self.text.len() {
            let to = next(&self.text, self.caret);
            self.text.replace_range(self.caret..to, "");
        }
        self.goal = None;
    }

    /// Back to the start of the word before the caret
    pub fn backspace_word(&mut self) {
        if !self.cut_selection() {
            let from = self.word_start(self.caret);
            self.text.replace_range(from..self.caret, "");
            self.caret = from;
        }
        self.goal = None;
    }

    fn word_start(&self, mut at: usize) -> usize {
        while at > 0 && !word_char(self.text[..at].chars().last().unwrap_or(' ')) {
            at = prev(&self.text, at);
        }
        while at > 0 && word_char(self.text[..at].chars().last().unwrap_or(' ')) {
            at = prev(&self.text, at);
        }
        at
    }

    fn word_end(&self, mut at: usize) -> usize {
        while at < self.text.len() && !word_char(self.text[at..].chars().next().unwrap_or(' ')) {
            at = next(&self.text, at);
        }
        while at < self.text.len() && word_char(self.text[at..].chars().next().unwrap_or(' ')) {
            at = next(&self.text, at);
        }
        at
    }

    /// The caret to `at`, the selection reaching there with `select`, else none
    pub fn place(&mut self, at: usize, select: bool) {
        let at = at.min(self.text.len());
        if select {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = at;
        self.goal = None;
    }

    pub fn left(&mut self, select: bool) {
        let to = match (self.selection(), select) {
            (Some((a, _)), false) => a,
            _ => prev(&self.text, self.caret),
        };
        self.place(to, select);
    }

    pub fn right(&mut self, select: bool) {
        let to = match (self.selection(), select) {
            (Some((_, b)), false) => b,
            _ => next(&self.text, self.caret),
        };
        self.place(to, select);
    }

    pub fn word_left(&mut self, select: bool) {
        self.place(self.word_start(self.caret), select);
    }

    pub fn word_right(&mut self, select: bool) {
        self.place(self.word_end(self.caret), select);
    }

    /// To the start of the line the caret shows on
    pub fn home(&mut self, select: bool, lines: &[Visual], cols: impl Fn(char) -> usize) {
        let (line, _) = place_of(&self.text, lines, self.caret, cols);
        self.place(lines.get(line).map_or(0, |l| l.start), select);
    }

    /// To the end of the line the caret shows on
    pub fn end(&mut self, select: bool, lines: &[Visual], cols: impl Fn(char) -> usize + Copy) {
        let (line, _) = place_of(&self.text, lines, self.caret, cols);
        let to = index_at(&self.text, lines, line, usize::MAX, cols);
        self.place(to, select);
    }

    /// Up a line (`by` -1) or down one (1), keeping to the column it started from; past the
    /// first or last line, to the start or the end
    pub fn vertical(&mut self, by: isize, select: bool, lines: &[Visual], cols: impl Fn(char) -> usize + Copy) {
        let (line, col) = place_of(&self.text, lines, self.caret, cols);
        let goal = self.goal.unwrap_or(col);
        let target = line as isize + by;
        let to = if target < 0 {
            0
        } else if target as usize >= lines.len() {
            self.text.len()
        } else {
            index_at(&self.text, lines, target as usize, goal, cols)
        };
        self.place(to, select);
        self.goal = Some(goal);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Whether the caret is on the box's first line, or its last: up and down go past it then
    pub fn on_edge(&self, by: isize, lines: &[Visual], cols: impl Fn(char) -> usize) -> bool {
        let (line, _) = place_of(&self.text, lines, self.caret, cols);
        if by < 0 {
            line == 0
        } else {
            line + 1 >= lines.len()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn one(_: char) -> usize {
        1
    }

    fn shown(text: &str, width: usize) -> Vec<&str> {
        wrap(text, width, one).iter().map(|l| &text[l.start..l.end]).collect()
    }

    #[test]
    fn a_chip_toggles_its_code_in_an_answer() {
        assert_eq!(toggle_code("", "1b"), "1b");
        assert_eq!(toggle_code("1b", "1b"), "", "pressed again: gone");
        assert_eq!(toggle_code("1b but blue-ish", "E2+"), "1b but blue-ish\nE2+", "a line of its own");
        assert_eq!(toggle_code("1b E2+", "E2-"), "1b E2-", "an extra's no replaces its yes");
        assert_eq!(toggle_code("1b, E2-", "E2-"), "1b,");
        assert!(has_code("1a, E2+", "1a") && has_code("1a, E2+", "E2+") && !has_code("1ab", "1a"));
        assert!(has_code("1a\nE2+", "E2+"), "on a line of its own");
    }

    #[test]
    fn each_chip_pressed_goes_on_a_line_of_its_own() {
        let mut t = String::new();
        for code in ["1a", "2a", "3b", "E4+"] {
            t = toggle_code(&t, code);
        }
        assert_eq!(t, "1a\n2a\n3b\nE4+");
        // Pressed again: it goes, and its line with it
        assert_eq!(toggle_code(&t, "2a"), "1a\n3b\nE4+");
        assert_eq!(toggle_code(&t, "E4+"), "1a\n2a\n3b", "no empty line left at the end");
        // An extra's other answer takes its line
        assert_eq!(toggle_code(&t, "E4-"), "1a\n2a\n3b\nE4-");
        assert_eq!(toggle_code("E2+, 1a\n3a", "E2-"), "E2-, 1a\n3a", "in its place, its comma kept");
        // After a new line he typed, no blank line between
        assert_eq!(toggle_code("1a\n", "2b"), "1a\n2b");
        // What he typed stays as he typed it, and a code he typed among his words goes alone
        assert_eq!(toggle_code("1b but  blue-ish\n  and tidy up", "E5+"), "1b but  blue-ish\n  and tidy up\nE5+");
        assert_eq!(toggle_code("1b but blue-ish\nE5+", "1b"), "but blue-ish\nE5+");
    }

    #[test]
    fn words_wrap_after_a_space_and_at_line_breaks() {
        assert_eq!(shown("hello there world", 11), ["hello ", "there world"]);
        assert_eq!(shown("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(shown("one\ntwo", 10), ["one", "two"]);
        assert_eq!(shown("", 10), [""]);
        assert_eq!(shown("end\n", 10), ["end", ""]);
        let lines = wrap("hello there", 7, one);
        assert!(lines[0].soft && !lines[1].soft);
    }

    #[test]
    fn the_caret_shows_where_it_is() {
        let text = "hello there world";
        let lines = wrap(text, 11, one);
        assert_eq!(place_of(text, &lines, 0, one), (0, 0));
        assert_eq!(place_of(text, &lines, 3, one), (0, 3));
        // At the wrap: the next line's start
        assert_eq!(place_of(text, &lines, 6, one), (1, 0));
        assert_eq!(place_of(text, &lines, text.len(), one), (1, 11));
        let text = "ab\n";
        let lines = wrap(text, 10, one);
        assert_eq!(place_of(text, &lines, 3, one), (1, 0), "after a line break: the empty line");
        assert_eq!(index_at("hello there world", &wrap("hello there world", 11, one), 1, 3, one), 9);
        assert_eq!(index_at("hello there world", &wrap("hello there world", 11, one), 0, 99, one), 5, "a wrapped line ends before its wrap");
    }

    #[test]
    fn typing_editing_and_selecting() {
        let mut b = Input::default();
        b.insert("hello world");
        assert_eq!((b.text(), b.caret()), ("hello world", 11));
        b.backspace();
        assert_eq!(b.text(), "hello worl");
        b.word_left(false);
        assert_eq!(b.caret(), 6);
        b.word_right(true);
        assert_eq!(b.selected(), Some("worl"));
        b.insert("there");
        assert_eq!(b.text(), "hello there");
        b.backspace_word();
        assert_eq!(b.text(), "hello ");
        b.insert("a\r\nb\tc\u{7}");
        assert_eq!(b.text(), "hello a\nb    c", "line breaks one kind, tabs as spaces, the bell gone");
        b.select_all();
        assert_eq!(b.selected(), Some("hello a\nb    c"));
        b.left(false);
        assert_eq!((b.caret(), b.selection()), (0, None), "left with a selection goes to its start");
        b.delete();
        assert_eq!(b.text(), "ello a\nb    c");
        assert_eq!(b.take(), "ello a\nb    c");
        assert!(b.is_empty() && b.caret() == 0);
        // Characters wider than a byte
        b.insert("é界");
        b.left(false);
        assert_eq!(b.caret(), 2);
        b.backspace();
        assert_eq!(b.text(), "界");
    }

    #[test]
    fn up_and_down_keep_their_column() {
        let mut b = Input::default();
        b.insert("first line\nab\nthird line");
        let lines = wrap(b.text(), 40, one);
        b.place(8, false); // "first li|ne"
        b.vertical(1, false, &lines, one);
        assert_eq!(b.caret(), 13, "the end of the short line");
        b.vertical(1, false, &lines, one);
        assert_eq!(b.caret(), 14 + 8, "back at column 8 on the third");
        b.vertical(1, false, &lines, one);
        assert_eq!(b.caret(), b.text().len(), "past the last: the end");
        b.vertical(-1, true, &lines, one);
        assert!(b.selection().is_some());
        b.home(false, &lines, one);
        assert_eq!(b.caret(), 11);
        b.end(false, &lines, one);
        assert_eq!(b.caret(), 13);
        assert!(!b.on_edge(-1, &lines, one) && !b.on_edge(1, &lines, one));
    }
}
