//! The conversation's words, wrapped to a width, with the light markdown the Conductor's replies
//! use: paragraphs, headings, bullet and numbered lists, `**bold**`, `` `code` ``, fenced code
//! blocks and `[links](lbterm://session/…)`. Anything else (tables, emphasis with one star) shows
//! as written. A line break in the text stays a line break.
//!
//! Widths are in units the caller chooses, from a function giving each character's width in the
//! style it's drawn in: the view draws prose in a proportional font, its bold in a wider weight
//! and code in a monospaced one (layout.rs measures them in sixteenths of a pixel).

/// How a run of words is drawn
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    Plain,
    Bold,
    /// Inline code: on a chip of its own
    Code,
    /// A heading's words
    Heading,
    /// A link's words: the run's `link` says where it goes
    Link,
}

/// Words in one style, from `col` units into their line
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub col: usize,
    pub text: String,
    pub style: Style,
    /// A link's target (`lbterm://session/<id>`, a web address)
    pub link: Option<String>,
}

/// What kind of line it is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// A line of a fenced code block: drawn on a block of its own
    Code,
    /// The space between paragraphs: half a line
    Blank,
}

/// One line as it's drawn
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub runs: Vec<Run>,
    pub kind: Kind,
    /// The line's words as they're copied, from its start
    pub text: String,
    /// It was wrapped: copied, it runs on into the next line without a break
    pub wraps: bool,
}

impl Line {
    fn blank() -> Self {
        Self {
            runs: vec![],
            kind: Kind::Blank,
            text: String::new(),
            wraps: false,
        }
    }

    /// How wide it is
    pub fn columns(&self, width: impl Fn(char, Style) -> usize) -> usize {
        self.runs
            .iter()
            .map(|r| r.col + r.text.chars().map(|c| width(c, r.style)).sum::<usize>())
            .max()
            .unwrap_or(0)
    }
}

/// A piece of a paragraph in one style
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

fn span(text: &str, style: Style) -> Span {
    Span {
        text: text.to_string(),
        style,
        link: None,
    }
}

/// Tabs as four spaces, carriage returns and other control characters gone
pub fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\t' => Some("    ".to_string()),
            '\n' => Some("\n".to_string()),
            c if c.is_control() => None,
            c => Some(c.to_string()),
        })
        .collect()
}

/// A `[words](target)` link at the start of `rest`: its words, its target and how long it is
fn link_at(rest: &str) -> Option<(&str, &str, usize)> {
    let close = rest.find("](")?;
    let words = &rest[1..close];
    if words.is_empty() || words.contains('[') || words.contains('\n') {
        return None;
    }
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let target = &after[..end];
    if target.is_empty() || target.contains(char::is_whitespace) {
        return None;
    }
    Some((words, target, close + 2 + end + 1))
}

/// `**bold**`, `` `code` `` and `[links](target)` in a line; markers without a partner show as
/// written
pub fn spans(line: &str, base: Style) -> Vec<Span> {
    let mut out: Vec<Span> = vec![];
    let mut plain = String::new();
    let mut rest = line;
    let push = |out: &mut Vec<Span>, plain: &mut String| {
        if !plain.is_empty() {
            out.push(span(plain, base));
            plain.clear();
        }
    };
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            if let Some(end) = rest[1..].find('`') {
                if end > 0 {
                    push(&mut out, &mut plain);
                    out.push(span(&rest[1..1 + end], Style::Code));
                    rest = &rest[end + 2..];
                    continue;
                }
            }
        }
        if c == '[' {
            if let Some((words, target, len)) = link_at(rest) {
                push(&mut out, &mut plain);
                out.push(Span {
                    text: words.to_string(),
                    style: Style::Link,
                    link: Some(target.to_string()),
                });
                rest = &rest[len..];
                continue;
            }
        }
        if rest.starts_with("**") {
            if let Some(end) = rest[2..].find("**") {
                if end > 0 {
                    push(&mut out, &mut plain);
                    let inner = &rest[2..2 + end];
                    for s in spans(inner, Style::Bold) {
                        out.push(s);
                    }
                    rest = &rest[end + 4..];
                    continue;
                }
            }
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    push(&mut out, &mut plain);
    out
}

/// Spans wrapped to `width`: the first line from `first`, the rest from `hang` (a list item's
/// words under its words, not its marker). Words go whole where they fit; one longer than a line
/// is cut across lines.
pub fn wrap_spans(
    spans: &[Span],
    width: usize,
    first: usize,
    hang: usize,
    kind: Kind,
    cols: impl Fn(char, Style) -> usize + Copy,
) -> Vec<Line> {
    let width = width.max(hang + 1).max(first + 1);
    let mut lines: Vec<Line> = vec![];
    let mut runs: Vec<Run> = vec![];
    let mut col = first;
    let mut start = first;
    // A space waits until a word follows it on the same line
    let mut space: Option<(Style, Option<String>)> = None;
    let measure = |text: &str, style: Style| text.chars().map(|c| cols(c, style)).sum::<usize>();

    // Words join the last run where it ends (`end`, the column after it), so a long line's run
    // isn't measured again for every word put on it: that made a layout of the Conductor's view
    // measure about 340,000 characters
    let mut end = usize::MAX;
    let put = |runs: &mut Vec<Run>, col: &mut usize, end: &mut usize, text: &str, style: Style, link: &Option<String>| {
        match runs.last_mut() {
            Some(last) if last.style == style && last.link == *link && *end == *col => last.text.push_str(text),
            _ => runs.push(Run {
                col: *col,
                text: text.to_string(),
                style,
                link: link.clone(),
            }),
        }
        *col += measure(text, style);
        *end = *col;
    };
    let finish = |lines: &mut Vec<Line>, runs: &mut Vec<Run>, wraps: bool| {
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        lines.push(Line {
            runs: std::mem::take(runs),
            kind,
            text,
            wraps,
        });
    };

    for s in spans {
        // Inline code sits on a chip: half a space of it each side, so the chip doesn't crowd
        // the words round it
        let pad = if s.style == Style::Code && kind == Kind::Text { cols(' ', Style::Code) / 2 } else { 0 };
        let mut first_word = true;
        let mut word = String::new();
        let mut pieces: Vec<(bool, String)> = vec![];
        for c in s.text.chars() {
            if c == ' ' {
                if !word.is_empty() {
                    pieces.push((false, std::mem::take(&mut word)));
                }
                pieces.push((true, " ".to_string()));
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            pieces.push((false, word));
        }
        for (is_space, piece) in pieces {
            if is_space {
                // Leading spaces at the start of the text (an indent) are kept; elsewhere a
                // space only goes down once a word follows it on the line
                if col == start && lines.is_empty() && runs.is_empty() {
                    put(&mut runs, &mut col, &mut end, " ", s.style, &s.link);
                } else if col > start {
                    space = Some((s.style, s.link.clone()));
                }
                continue;
            }
            let lead = if first_word { pad } else { 0 };
            first_word = false;
            let w = measure(&piece, s.style) + lead;
            // A word that follows the last with no space between ("`ai.ts`'s", "**Bun**:")
            // stays on its line, past its end by a little if need be, rather than start the
            // next line alone
            let glued = space.is_none() && col > start && !runs.is_empty();
            if glued && col + w <= width + width / 12 {
                col += lead;
                put(&mut runs, &mut col, &mut end, &piece, s.style, &s.link);
                continue;
            }
            let gap = space.as_ref().map_or(0, |(style, _)| cols(' ', *style));
            if col > start && col + gap + w > width {
                finish(&mut lines, &mut runs, true);
                col = hang;
                start = hang;
                space = None;
            }
            if let Some((style, link)) = space.take() {
                put(&mut runs, &mut col, &mut end, " ", style, &link);
            }
            col += lead;
            if col + w - lead <= width {
                put(&mut runs, &mut col, &mut end, &piece, s.style, &s.link);
                continue;
            }
            // Longer than a line: cut it where it fills one
            let mut part = String::new();
            let mut part_w = 0;
            for c in piece.chars() {
                let cw = cols(c, s.style);
                if col + part_w + cw > width && !(part.is_empty() && col == start) {
                    put(&mut runs, &mut col, &mut end, &part, s.style, &s.link);
                    part.clear();
                    part_w = 0;
                    finish(&mut lines, &mut runs, true);
                    col = hang;
                    start = hang;
                }
                part.push(c);
                part_w += cw;
            }
            if !part.is_empty() {
                put(&mut runs, &mut col, &mut end, &part, s.style, &s.link);
            }
        }
        if !first_word {
            col += pad;
        }
    }
    finish(&mut lines, &mut runs, false);
    lines
}

/// Plain words: each line of the text wrapped on its own, an empty line kept as a blank
pub fn plain(text: &str, width: usize, cols: impl Fn(char) -> usize + Copy) -> Vec<Line> {
    let text = clean(text);
    let text = text.trim_end_matches('\n');
    let mut out = vec![];
    for line in text.split('\n') {
        if line.trim().is_empty() {
            out.push(Line::blank());
        } else {
            out.extend(wrap_spans(&[span(line, Style::Plain)], width, 0, 0, Kind::Text, |c, _| cols(c)));
        }
    }
    out
}

/// A list item's marker and its depth, if the line is one: `- `, `* `, `• `, `+ `, or `1. `,
/// `1) `
fn list_item(line: &str) -> Option<(usize, String, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    for bullet in ["- ", "* ", "• ", "+ "] {
        if let Some(words) = rest.strip_prefix(bullet) {
            return Some((indent / 2, "•".to_string(), words));
        }
    }
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 4 {
        let after = &rest[digits..];
        for end in [". ", ") "] {
            if let Some(words) = after.strip_prefix(end) {
                return Some((indent / 2, format!("{}{}", &rest[..digits], &end[..1]), words));
            }
        }
    }
    None
}

/// `words` at `col` before a line's runs (a list's marker, a quote's bar), its copied words too
fn lead(line: &mut Line, col: usize, words: &str) {
    line.runs.insert(
        0,
        Run {
            col,
            text: words.to_string(),
            style: Style::Plain,
            link: None,
        },
    );
    line.text = format!("{words}{}", line.text);
}

/// The Conductor's reply, its light markdown drawn: wrapped to `width`. A list's marker sits in a
/// gutter as wide as "00. ", four spaces in for each level, its words after the gutter.
pub fn markdown(text: &str, width: usize, cols: impl Fn(char, Style) -> usize + Copy) -> Vec<Line> {
    let text = clean(text);
    let mut out: Vec<Line> = vec![];
    let mut code = false;
    let mut last_blank = true;
    let measure = |s: &str| s.chars().map(|c| cols(c, Style::Plain)).sum::<usize>();
    for line in text.trim_end_matches('\n').split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            code = !code;
            continue;
        }
        if code {
            // A code block's lines as written, cut where they're too long
            if line.is_empty() {
                out.push(Line {
                    runs: vec![],
                    kind: Kind::Code,
                    text: String::new(),
                    wraps: false,
                });
            } else {
                let mut rest: Vec<char> = line.chars().collect();
                while !rest.is_empty() {
                    let mut taken = 0;
                    let mut w = 0;
                    while taken < rest.len() && w + cols(rest[taken], Style::Code) <= width.max(1) {
                        w += cols(rest[taken], Style::Code);
                        taken += 1;
                    }
                    let taken = taken.max(1);
                    let piece: String = rest.drain(..taken).collect();
                    out.push(Line {
                        runs: vec![Run {
                            col: 0,
                            text: piece.clone(),
                            style: Style::Code,
                            link: None,
                        }],
                        kind: Kind::Code,
                        text: piece,
                        wraps: !rest.is_empty(),
                    });
                }
            }
            last_blank = false;
            continue;
        }
        if trimmed.is_empty() || matches!(trimmed, "---" | "***" | "___") {
            if !last_blank {
                out.push(Line::blank());
            }
            last_blank = true;
            continue;
        }
        last_blank = false;
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let words = trimmed[hashes..].trim();
            out.extend(wrap_spans(&spans(words, Style::Heading), width, 0, 0, Kind::Text, cols));
            continue;
        }
        if let Some((depth, marker, words)) = list_item(line) {
            // The marker, then its words from a gutter wide enough for "10." whatever the font
            let indent = depth * measure("    ");
            let gutter = measure("00. ").max(measure(&format!("{marker} ")));
            let hang = indent + gutter;
            let mut lines = wrap_spans(&spans(words, Style::Plain), width, hang, hang, Kind::Text, cols);
            if let Some(first) = lines.first_mut() {
                lead(first, indent, &format!("{marker} "));
            }
            out.extend(lines);
            continue;
        }
        if let Some(quote) = trimmed.strip_prefix('>') {
            let bar = measure("│ ");
            let mut lines = wrap_spans(&spans(quote.trim_start(), Style::Plain), width, bar, bar, Kind::Text, cols);
            for l in lines.iter_mut() {
                lead(l, 0, "│ ");
            }
            out.extend(lines);
            continue;
        }
        out.extend(wrap_spans(&spans(line, Style::Plain), width, 0, 0, Kind::Text, cols));
    }
    while out.last().map_or(false, |l| l.kind == Kind::Blank) {
        out.pop();
    }
    out
}

/// `text` cut to `columns` wide, an ellipsis where it was cut; line breaks as spaces
pub fn fit(text: &str, columns: usize, cols: impl Fn(char) -> usize) -> String {
    let flat: String = clean(text).replace('\n', " ");
    let flat = flat.trim();
    // One pass, ending where it's past the room: a session's whole summary was measured to fit
    // its first line
    let dots = cols('…');
    let mut w = 0;
    let mut cut = None;
    for (i, c) in flat.char_indices() {
        let cw = cols(c);
        if cut.is_none() && w + cw + dots > columns {
            cut = Some(i);
        }
        w += cw;
        if w > columns {
            if columns == 0 {
                return String::new();
            }
            let at = cut.unwrap_or(i);
            return format!("{}…", flat[..at].trim_end());
        }
    }
    flat.to_string()
}

/// The `1a`-style options a question in text offers (Lachlan's convention: questions numbered,
/// their options 1a, 1b), each with the words after it: from lines that start with one, after
/// any indent, bullet or bold, followed by `.`, `)`, `:` or a space. Each code once, in order.
pub fn options(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in clean(text).lines() {
        let mut rest = line.trim_start();
        for lead in ["- ", "* ", "• ", "**", "("] {
            rest = rest.strip_prefix(lead).unwrap_or(rest).trim_start();
        }
        let rest = rest.strip_prefix('Q').unwrap_or(rest);
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 2 {
            continue;
        }
        let after = &rest[digits..];
        let letters = after.chars().take_while(|c| c.is_ascii_lowercase()).count();
        if letters != 1 {
            continue;
        }
        let tail = &after[letters..];
        let Some(sep) = tail.chars().next() else {
            continue;
        };
        if !matches!(sep, '.' | ')' | ':' | ' ' | '*') {
            continue;
        }
        let code = format!("{}{}", &rest[..digits], &after[..letters]);
        let label = tail
            .trim_start_matches(|c: char| matches!(c, '.' | ')' | ':' | '*' | ' '))
            .trim()
            .trim_end_matches("**")
            .trim()
            .to_string();
        if !out.iter().any(|(c, _)| *c == code) {
            out.push((code, label));
        }
    }
    out
}

/// The extras a question in text offers (Lachlan's convention: numbered on from its questions,
/// `E4. An extra`, each answered yes or no), each with the words after it: from lines that start
/// with one, after any indent, bullet or bold. Each code once, in order.
pub fn extras(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in clean(text).lines() {
        let mut rest = line.trim_start();
        for lead in ["- ", "* ", "• ", "**"] {
            rest = rest.strip_prefix(lead).unwrap_or(rest).trim_start();
        }
        let Some(after) = rest.strip_prefix('E') else {
            continue;
        };
        let digits = after.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 2 {
            continue;
        }
        let tail = &after[digits..];
        if !matches!(tail.chars().next(), Some('.' | ')' | ':' | ' ' | '*')) {
            continue;
        }
        let code = format!("E{}", &after[..digits]);
        let label = tail
            .trim_start_matches(|c: char| matches!(c, '.' | ')' | ':' | '*' | ' '))
            .trim()
            .trim_end_matches("**")
            .trim()
            .to_string();
        if !out.iter().any(|(c, _)| *c == code) {
            out.push((code, label));
        }
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;

    fn one(_: char) -> usize {
        1
    }

    fn one_styled(_: char, _: Style) -> usize {
        1
    }

    fn texts(lines: &[Line]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    fn sp(text: &str, style: Style) -> Span {
        span(text, style)
    }

    #[test]
    fn words_wrap_whole_and_long_ones_are_cut() {
        let lines = plain("the quick brown fox jumps", 10, one);
        assert_eq!(texts(&lines), ["the quick", "brown fox", "jumps"]);
        assert!(lines[0].wraps && lines[1].wraps && !lines[2].wraps);
        let lines = plain("abcdefghijklmnop end", 6, one);
        assert_eq!(texts(&lines), ["abcdef", "ghijkl", "mnop", "end"]);
        // Line breaks stay, an empty line is a blank
        let lines = plain("one\n\ntwo\r\n", 20, one);
        assert_eq!(lines.iter().map(|l| l.kind).collect::<Vec<_>>(), [Kind::Text, Kind::Blank, Kind::Text]);
        assert_eq!(lines[2].text, "two");
        // Leading spaces at the start are kept
        assert_eq!(plain("  indented", 20, one)[0].text, "  indented");
        // A wide character takes its columns
        let wide = |c: char| if c == '界' { 2 } else { 1 };
        assert_eq!(texts(&plain("ab 界界界", 5, wide)), ["ab", "界界", "界"]);
    }

    #[test]
    fn bold_and_code_are_runs_of_their_own() {
        let s = spans("a **big** `x = 1` and `` ok", Style::Plain);
        assert_eq!(
            s,
            vec![
                sp("a ", Style::Plain),
                sp("big", Style::Bold),
                sp(" ", Style::Plain),
                sp("x = 1", Style::Code),
                sp(" and `` ok", Style::Plain),
            ]
        );
        // A marker with no partner shows as written
        assert_eq!(spans("2 ** 3", Style::Plain), vec![sp("2 ** 3", Style::Plain)]);
        let lines = markdown("Run `cargo test` **now**", 40, one_styled);
        let runs = &lines[0].runs;
        assert_eq!(runs[1].text, "cargo test");
        assert_eq!((runs[1].col, runs[1].style), (4, Style::Code));
        assert_eq!(runs.last().map(|r| (r.text.as_str(), r.style)), Some(("now", Style::Bold)));
        assert_eq!(lines[0].text, "Run cargo test now");
    }

    #[test]
    fn a_style_wider_than_the_rest_is_measured_as_drawn() {
        // Bold two units a character, the rest one: "a **bc** d" puts "d" after 1 + 1 + 4 + 1
        let wide_bold = |_: char, s: Style| if s == Style::Bold { 2 } else { 1 };
        let lines = markdown("a **bc** d", 40, wide_bold);
        let d = lines[0].runs.last().unwrap();
        assert_eq!((d.text.as_str(), d.col), (" d", 6));
        assert_eq!(lines[0].columns(wide_bold), 8);
        // And it wraps where the bold words fill the line
        let lines = markdown("**abcd** e", 9, wide_bold);
        assert_eq!(texts(&lines), ["abcd", "e"]);
    }

    #[test]
    fn code_keeps_half_a_space_each_side_and_what_follows_it_stays_with_it() {
        // Code's space two units wide: a unit of room each side of its chip
        let w = |_: char, s: Style| if s == Style::Code { 2 } else { 1 };
        let lines = markdown("a `bc` d", 40, w);
        let runs = &lines[0].runs;
        assert_eq!((runs[1].text.as_str(), runs[1].col), ("bc", 3), "after \"a \" and its unit of room");
        assert_eq!((runs[2].text.as_str(), runs[2].col), (" d", 8), "after the code's four units and its unit of room");
        // "`abcd`'s" at a line's end keeps its "'s" rather than start the next line with it
        let lines = markdown("one two three four `abcd`'s and more", 24, one_styled);
        assert_eq!(texts(&lines), ["one two three four abcd's", "and more"]);
    }

    #[test]
    fn links_show_their_words_and_keep_where_they_go() {
        let s = spans("see [Site publish](lbterm://session/f41e) in Website", Style::Plain);
        assert_eq!(s.len(), 3);
        assert_eq!((s[1].text.as_str(), s[1].style), ("Site publish", Style::Link));
        assert_eq!(s[1].link.as_deref(), Some("lbterm://session/f41e"));
        let lines = markdown("- [Pad colours](lbterm://session/fc) has handled it", 80, one_styled);
        assert_eq!(lines[0].text, "• Pad colours has handled it");
        let link = lines[0].runs.iter().find(|r| r.style == Style::Link).unwrap();
        assert_eq!((link.text.as_str(), link.link.as_deref()), ("Pad colours", Some("lbterm://session/fc")));
        // A link's words wrap like any, each piece keeping its target
        let lines = markdown("[one two three](x://y)", 8, one_styled);
        assert_eq!(texts(&lines), ["one two", "three"]);
        assert!(lines.iter().all(|l| l.runs.iter().all(|r| r.link.as_deref() == Some("x://y"))));
        // Not links: brackets alone, a space in the target, nothing in the brackets
        for text in ["[1a] pick", "[x](a b)", "[](x)", "a [b] (c)"] {
            assert!(spans(text, Style::Plain).iter().all(|s| s.style == Style::Plain), "{text}");
        }
    }

    #[test]
    fn lists_hang_under_their_words() {
        let lines = markdown("Two things:\n- the first one is long\n  - nested\n12. twelfth", 18, one_styled);
        assert_eq!(texts(&lines), ["Two things:", "• the first one", "is long", "• nested", "12. twelfth"]);
        // The words sit in a gutter four wide, nested ones four further in
        assert_eq!(lines[1].runs[0].col, 0, "the marker");
        assert_eq!(lines[1].runs[1].col, 4, "the words after the gutter");
        assert_eq!(lines[2].runs[0].col, 4, "the wrapped words under the item's words");
        assert_eq!((lines[3].runs[0].col, lines[3].runs[1].col), (4, 8));
    }

    #[test]
    fn headings_paragraphs_and_code_blocks() {
        let text = "## Done\n\n\nIt worked.\n\n```\nlet x = 12345678901;\n```\nAfter";
        let lines = markdown(text, 10, one_styled);
        assert_eq!(lines[0].runs[0].style, Style::Heading);
        assert_eq!(lines[0].text, "Done");
        assert_eq!(lines[1].kind, Kind::Blank, "blank lines between paragraphs fold into one");
        assert_eq!(lines[2].text, "It worked.");
        assert_eq!(lines[3].kind, Kind::Blank);
        assert_eq!(lines[4].kind, Kind::Code);
        assert_eq!((lines[4].text.as_str(), lines[4].wraps), ("let x = 12", true));
        assert_eq!(lines[5].text, "345678901;");
        assert_eq!(lines[6].text, "After");
        // No blank at the end
        assert!(markdown("hi\n\n", 10, one_styled).iter().all(|l| l.kind != Kind::Blank));
    }

    #[test]
    fn fitting_cuts_with_an_ellipsis() {
        assert_eq!(fit("short", 10, one), "short");
        assert_eq!(fit("a longer line\nof words", 10, one), "a longer…");
        assert_eq!(fit("anything", 0, one), "");
        assert_eq!(fit("", 0, one), "");
        assert_eq!(fit("ten chars!", 10, one), "ten chars!", "exactly the room");
        assert_eq!(fit("eleven char", 10, one), "eleven ch…");
        assert_eq!(fit("  spaced   \n", 10, one), "spaced", "trimmed");
        // It stops measuring once it's past the room
        let calls = std::cell::Cell::new(0usize);
        let counted = |_c: char| {
            calls.set(calls.get() + 1);
            1
        };
        assert_eq!(fit(&"word ".repeat(1000), 20, counted), "word word word word…");
        assert!(calls.get() < 30, "{} measured to fit 20 columns", calls.get());
    }

    #[test]
    fn a_questions_options_are_found() {
        let text = "Q1. Which layout?\n  1a. Two columns\n  1b) One column\n\nQ2. Extras, yes or no each:\n- **2a.** A dark theme\n2b: later\n1a. again\n12. not one\nab. nor this";
        assert_eq!(
            options(text),
            vec![
                ("1a".to_string(), "Two columns".to_string()),
                ("1b".to_string(), "One column".to_string()),
                ("2a".to_string(), "A dark theme".to_string()),
                ("2b".to_string(), "later".to_string()),
            ]
        );
        assert!(options("Nothing to pick here.").is_empty());
    }

    #[test]
    fn a_questions_extras_are_found_apart_from_its_options() {
        let text = "Q1. Which?\n  1a. This\n\nExtras, yes or no each:\nE2. A test for the old names\n- **E3.** A dark theme\nE4: later\nEach line\nE. not one\nE123. nor this";
        assert_eq!(
            extras(text),
            vec![
                ("E2".to_string(), "A test for the old names".to_string()),
                ("E3".to_string(), "A dark theme".to_string()),
                ("E4".to_string(), "later".to_string()),
            ]
        );
        assert_eq!(options(text), vec![("1a".to_string(), "This".to_string())], "extras aren't options");
    }

    #[test]
    fn wrapping_measures_each_character_a_few_times_at_most() {
        // A long paragraph and a word longer than a line: the run a word joins isn't measured
        // again for each word, nor the part of a long word for each character (they were, and a
        // layout of the Conductor's view measured about 340,000 characters)
        let calls = std::cell::Cell::new(0usize);
        let cols = |_c: char, _s: Style| {
            calls.set(calls.get() + 1);
            16
        };
        let paragraph = "the quick brown fox jumps over the lazy dog ".repeat(60);
        let long_word = "x".repeat(3000);
        for text in [paragraph.as_str(), long_word.as_str()] {
            calls.set(0);
            let lines = wrap_spans(&[span(text, Style::Plain)], 16 * 100, 0, 0, Kind::Text, cols);
            let chars = text.chars().count();
            assert!(calls.get() <= 4 * chars, "{} measurements for {chars} characters", calls.get());
            // And the lines are what they were: a hundred columns at most, every word kept
            assert!(lines.iter().all(|l| l.text.chars().count() <= 100), "a line past 100 columns");
            let back: String = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(if text == long_word { "" } else { " " });
            assert_eq!(back.split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
            assert!(lines.iter().all(|l| l.runs.len() == 1), "each line one run of plain words");
        }
    }
}
