//! The scroll strip against what Claude Code 2.1.312 really draws: a conversation captured in a
//! test window (fixtures/claude-2.1.312), its transcript and every screen of it from the top a
//! PgDn apart, with their colours (`lbterm cli get-text --escapes`), the names that said whose
//! PC it was swapped for others of the same length. A stand-in written to the code's own idea of
//! Claude Code passed while the strip was wrong in the real one (2026-10-28); these hold it to
//! the real one.

use super::*;
use std::path::PathBuf;

/// The capture: the fixture, or another made the same way named by LBTERM_CONVERSATION_CAPTURE
/// (a real session of one's own, measured without putting it in the repo)
fn fixture() -> PathBuf {
    match std::env::var_os("LBTERM_CONVERSATION_CAPTURE") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/conversation/fixtures/claude-2.1.312"),
    }
}

fn own_capture() -> bool {
    std::env::var_os("LBTERM_CONVERSATION_CAPTURE").is_some()
}

/// A captured screen's rows as the GUI reads a pane's cells: each row's words, whether its first
/// mark is on a background (the band), and whether the words after the mark are dim on it
fn rows_of(escaped: &str) -> Vec<Row> {
    escaped.lines().map(row_of).collect()
}

fn row_of(line: &str) -> Row {
    type Rgb = Option<(u8, u8, u8)>;
    let mut cells: Vec<(char, Rgb, Rgb, bool)> = vec![];
    let (mut fg, mut bg, mut faint): (Rgb, Rgb, bool) = (None, None, false);
    let mut chars = line.chars().peekable();
    let rgb = |p: &str| -> Rgb {
        let v: Vec<u8> = p.split(':').skip(3).filter_map(|n| n.parse().ok()).collect();
        (v.len() == 3).then(|| (v[0], v[1], v[2]))
    };
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            cells.push((c, fg, bg, faint));
            continue;
        }
        match chars.next() {
            Some('[') => {
                let mut params = String::new();
                let mut end = ' ';
                for p in chars.by_ref() {
                    if p.is_ascii_alphabetic() {
                        end = p;
                        break;
                    }
                    params.push(p);
                }
                if end != 'm' {
                    continue;
                }
                for p in params.split(';') {
                    match p {
                        "" | "0" => (fg, bg, faint) = (None, None, false),
                        "2" => faint = true,
                        "22" => faint = false,
                        "39" => fg = None,
                        "49" => bg = None,
                        p if p.starts_with("38:2") => fg = rgb(p),
                        p if p.starts_with("48:2") => bg = rgb(p),
                        _ => {}
                    }
                }
            }
            // A character set: ESC ( B
            Some('(') => {
                chars.next();
            }
            // An OSC, a link's (ESC ] 8 ; ; url ESC \): to its BEL or ST, no cells of its own
            Some(']') => {
                while let Some(p) = chars.next() {
                    if p == '\x07' || (p == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    let text: String = cells.iter().map(|c| c.0).collect();
    let first = cells.iter().position(|c| !c.0.is_whitespace());
    let band = first.is_some_and(|i| cells[i].0 == prompts::MARK && cells[i].2.is_some());
    let dim = band && {
        let i = first.unwrap();
        let words = cells[i + 1..].iter().find(|c| !c.0.is_whitespace());
        words.is_some_and(|w| prompts::dim_words(w.1, w.2.or(cells[i].2).unwrap_or((0, 0, 0)), w.3))
    };
    Row { text, band, dim }
}

pub(super) fn screens() -> Vec<Vec<Row>> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixture())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("screen-")))
        .collect();
    // screen-9 before screen-10
    names.sort_by_key(|p| p.file_stem().and_then(|s| s.to_string_lossy().trim_start_matches("screen-").parse::<usize>().ok()));
    names.iter().map(|p| rows_of(&std::fs::read_to_string(p).unwrap())).collect()
}

pub(super) fn snapshot() -> Snapshot {
    let mut f = Follow::default();
    // Where Claude Code had the transcript, which says where the session's scratchpad was
    let at = std::fs::read_to_string(fixture().join("session.txt")).unwrap();
    f.conversation.scratchpad = scratchpad_of(std::path::Path::new(at.trim()));
    f.feed(&std::fs::read(fixture().join("transcript.jsonl")).unwrap());
    f.conversation.snapshot()
}

pub(super) const COLS: usize = 100;

/// The capture's transcript, line by line
pub(super) fn transcript_lines() -> Vec<String> {
    std::fs::read_to_string(fixture().join("transcript.jsonl")).unwrap().lines().map(String::from).collect()
}
/// What a PgDn moved between the captures: half the 43-row area
const PAGE: usize = 21;

/// The conversation as Claude Code laid it out, every row, stitched from the captures (each row
/// taken from a screen where neither the header nor "Jump to bottom" covers it), with the rows
/// under the area every screen has, and where each capture was
struct Real {
    content: Vec<Row>,
    bottom: Vec<Row>,
    area: usize,
    offsets: Vec<usize>,
}

impl Real {
    fn new(screens: &[Vec<Row>]) -> Self {
        let first = Screen::read(&screens[0]).expect("Claude Code's input box");
        let area = first.area;
        let n = screens.len();
        // The last capture is at the latest: where it overlaps the one before says where it is
        let before = PAGE * (n - 2);
        let text = |s: &Vec<Row>, i: usize| s[i].text.clone();
        let shift = (0..=PAGE)
            .find(|&d| (1..area - 1 - d).all(|i| text(&screens[n - 1], i) == text(&screens[n - 2], i + d)))
            .expect("the last two captures overlap");
        let mut offsets: Vec<usize> = (0..n - 1).map(|k| PAGE * k).collect();
        offsets.push(before + shift);
        let total = before + shift + area;
        let mut content: Vec<Option<Row>> = vec![None; total];
        for (k, s) in screens.iter().enumerate() {
            let sc = Screen::read(s).unwrap();
            for i in 0..area {
                if (sc.header && i == 0) || (sc.scrolled && i == area - 1) {
                    continue;
                }
                content[offsets[k] + i].get_or_insert_with(|| s[i].clone());
            }
        }
        Self {
            content: content.into_iter().map(|r| r.expect("every row captured clean")).collect(),
            bottom: screens[0][area..].to_vec(),
            area,
            offsets,
        }
    }

    fn total(&self) -> usize {
        self.content.len()
    }

    fn last_top(&self) -> usize {
        self.total() - self.area
    }

    fn is_prompt(row: &Row) -> bool {
        row.band && !row.dim && row.text.trim_start().starts_with(prompts::MARK)
    }

    /// The screen at `top`, as Claude Code draws it: the header over the first row when a
    /// message is above the view and none starts it, "Jump to bottom" over the last while
    /// scrolled up
    fn screen(&self, top: usize) -> Vec<Row> {
        let top = top.min(self.last_top());
        let mut rows: Vec<Row> = self.content[top..top + self.area].to_vec();
        let scrolled = top < self.last_top();
        if scrolled {
            if !Self::is_prompt(&rows[0]) {
                if let Some(p) = self.content[..top].iter().rev().find(|r| Self::is_prompt(r)) {
                    rows[0] = Row { text: p.text.clone(), band: true, dim: true };
                }
            }
            let left: String = rows[self.area - 1].text.chars().take(36).collect();
            rows[self.area - 1] = Row { text: format!("{left} Jump to bottom (ctrl+End) \u{2193}"), band: false, dim: false };
        }
        rows.extend(self.bottom.iter().cloned());
        rows
    }

    /// Where `key` moves a view at `top`
    fn press(&self, top: usize, key: Key, n: u32) -> usize {
        let half = self.area / 2;
        let n = n as usize;
        match key {
            Key::PageUp => top.saturating_sub(half * n),
            Key::PageDown => (top + half * n).min(self.last_top()),
            Key::Top => 0,
            Key::Bottom => self.last_top(),
        }
    }
}

#[test]
fn the_captures_read_as_claude_code_drew_them() {
    let screens = screens();
    if !own_capture() {
        assert_eq!(screens.len(), 15);
    }
    for (k, rows) in screens.iter().enumerate() {
        let s = Screen::read(rows).unwrap_or_else(|| panic!("screen {k} has no input box"));
        assert_eq!(s.area, 43, "screen {k}: the area above the input box");
        let last = k == screens.len() - 1;
        assert_eq!(s.scrolled, !last, "screen {k}: only the last is at the latest");
        // The header from the second page on, naming the prompt above (the first page shows the
        // banner, with nothing above it)
        assert_eq!(s.header, k >= 1 && !last, "screen {k}: the header");
    }
    // The header is on the band in dim words; a message is on it in white
    let header = &screens[1][0];
    assert!(header.band && header.dim, "{header:?}");
    let message = screens[0].iter().find(|r| r.band && !r.text.trim().is_empty()).unwrap();
    assert!(message.band && !message.dim, "{message:?}");
}

#[test]
fn the_layout_comes_within_a_few_rows_of_claude_codes() {
    let real = Real::new(&screens());
    let s = snapshot();
    let layout = s.layout(COLS);
    // Every prompt's first row, the layout's and Claude Code's, from the top: within a dozen
    // rows, or 1% of the way down a long one
    let near = |row: usize| 12.max(row / 100);
    let real_prompts: Vec<usize> = (0..real.total()).filter(|&i| Real::is_prompt(&real.content[i])).collect();
    let prompts: Vec<usize> = (0..s.blocks.len()).filter(|&b| s.blocks[b].kind == BlockKind::Prompt).collect();
    assert_eq!(real_prompts.len(), prompts.len(), "the same prompts");
    for (&b, &r) in prompts.iter().zip(&real_prompts) {
        let at = layout.tops[b] + 1;
        assert!(at.abs_diff(r) <= near(r), "prompt {b}: the layout says row {at}, Claude Code drew it at {r}");
    }
    assert!(layout.total.abs_diff(real.total()) <= near(real.total()) + 3, "{} rows laid out, {} drawn", layout.total, real.total());
}

#[test]
fn the_view_is_found_where_claude_code_is() {
    let screens = screens();
    let real = Real::new(&screens);
    let s = snapshot();
    let layout = s.layout(COLS);
    let span = |t: usize, of: usize| t as f64 / of.max(1) as f64;
    // The captures themselves, then a screen every 5 rows
    let mut checked = 0;
    for (k, rows) in screens.iter().enumerate() {
        let view = s.locate(&layout, rows, None).unwrap_or_else(|| panic!("capture {k}: no view"));
        assert_eq!(view.rows, 43);
        let (got, want) = (span(view.top, layout.total - 43), span(real.offsets[k], real.last_top()));
        assert!((got - want).abs() < 0.05, "capture {k}: the thumb at {got:.3} of the track, Claude Code at {want:.3}");
        checked += 1;
    }
    let mut near = None;
    for top in (0..=real.last_top()).step_by(5) {
        let Some(view) = s.locate(&layout, &real.screen(top), near) else {
            continue;
        };
        near = Some(view.top);
        let (got, want) = (span(view.top, layout.total - 43), span(top, real.last_top()));
        assert!((got - want).abs() < 0.06, "row {top}: the thumb at {got:.3} of the track, Claude Code at {want:.3}");
        checked += 1;
    }
    assert!(checked > 60);
}

#[test]
fn a_seek_reaches_every_mark_in_a_few_bursts_from_anywhere() {
    let real = Real::new(&screens());
    let s = snapshot();
    let layout = s.layout(COLS);
    // Every message's first line, and the line every mark seeks (a turn's end after a long
    // answer by that answer's last line)
    let mut points: Vec<(usize, usize)> = (0..s.blocks.len()).filter_map(|b| s.blocks[b].key.as_ref().and(s.seek_point(b, COLS))).collect();
    points.extend(s.events.iter().filter_map(|e| s.seek_point(e.block, COLS)));
    points.sort_unstable();
    points.dedup();
    assert!(points.len() > 30, "{} places to go", points.len());
    for start in [real.last_top(), 0, real.last_top() / 2] {
        for &(target, line) in &points {
            let mut seek = Seek::new(&s, target, line).unwrap();
            let mut top = start;
            let mut near = None;
            let mut keys = 0;
            let what = format!("block {target} line {line} ({:?}) from row {start}", s.blocks[target].text.chars().take(24).collect::<String>());
            let row = loop {
                let rows = real.screen(top);
                match seek.look(&s, &layout, &rows, near) {
                    Step::Found(r) => break r,
                    Step::Press(key, n) => {
                        keys += n;
                        top = real.press(top, key, n);
                    }
                    step => panic!("{what}: {step:?} after {} bursts, at row {top} of {}", seek.rounds(), real.last_top()),
                }
                near = s.locate(&layout, &real.screen(top), near).map(|v| v.top);
            };
            assert!(seek.rounds() <= 4, "{what}: {} bursts, {keys} keys", seek.rounds());
            let rows = real.screen(top);
            // The row found shows the line: a prompt's words, or the line's own
            let b = &s.blocks[target];
            let words: String = if b.kind == BlockKind::Prompt { bare(b.key.as_deref().unwrap_or("")) } else { b.line_keys[line].clone() };
            let words: String = words.chars().take(10).collect();
            assert!(bare(&rows[row].text).contains(&words), "{what}: row {row} is {:?}, the line starts {words:?}", rows[row].text);
            assert!(row <= 43 / 2 || top == real.last_top(), "{what}: found at row {row}, below the middle");
        }
    }
}

#[test]
fn a_mark_without_words_of_its_own_goes_to_the_words_just_above_it() {
    if own_capture() {
        return;
    }
    let s = snapshot();
    // The turn's end after the 120 lines: their last line, not their first
    let long = (0..s.blocks.len()).find(|&b| s.blocks[b].text.starts_with("line 1\n")).expect("the 120 lines");
    let end = (long + 1..s.blocks.len()).find(|&b| s.blocks[b].kind == BlockKind::TurnEnd).unwrap();
    assert_eq!(s.seek_point(end, COLS), Some((long, 119)));
    assert_eq!(s.seek_point(long, COLS), Some((long, 0)));
    // A point on the track in the middle of them: the line there
    let layout = s.layout(COLS);
    let (b, line) = s.point_at(&layout, layout.tops[long] + 61).unwrap();
    assert_eq!(b, long);
    assert!(line.abs_diff(60) <= 1, "line {line}");
}

/// With LBTERM_CONVERSATION_CAPTURE set: where the layout and Claude Code part, prompt by prompt,
/// said by failing (a passing test's output isn't shown)
#[test]
fn report_on_an_own_capture() {
    if !own_capture() {
        return;
    }
    let real = Real::new(&screens());
    let s = snapshot();
    let layout = s.layout(COLS);
    let real_prompts: Vec<usize> = (0..real.total()).filter(|&i| Real::is_prompt(&real.content[i])).collect();
    let prompts: Vec<usize> = (0..s.blocks.len()).filter(|&b| s.blocks[b].kind == BlockKind::Prompt).collect();
    let mut out = format!("{} rows drawn, {} laid out; {} prompts drawn, {} in the transcript\n", real.total(), layout.total, real_prompts.len(), prompts.len());
    for (&b, &r) in prompts.iter().zip(&real_prompts) {
        out += &format!("  block {b:4} at {:5} drawn at {r:5}  {:?}\n", layout.tops[b] + 1, s.blocks[b].text.chars().take(40).collect::<String>());
    }
    // Kinds of block and the rows each kind is given
    let mut by: HashMap<String, (usize, usize)> = HashMap::new();
    for b in &s.blocks {
        let e = by.entry(format!("{:?}", b.kind)).or_default();
        e.0 += 1;
        e.1 += b.rows(COLS);
    }
    out += &format!("{by:?}\n");
    // The captures the view is found far from, and what placed it there
    let screens = screens();
    let span = |t: usize, of: usize| t as f64 / of.max(1) as f64;
    let mut bad = 0;
    for (k, rows) in screens.iter().enumerate() {
        let Some(view) = s.locate(&layout, rows, None) else {
            out += &format!("capture {k}: no view\n");
            continue;
        };
        let (got, want) = (span(view.top, layout.total - 43), span(real.offsets[k], real.last_top()));
        if (got - want).abs() < 0.05 || bad >= 4 {
            continue;
        }
        bad += 1;
        let sc = Screen::read(rows).unwrap();
        let part = if sc.header { s.header_prompt(&layout, &rows[0], None) } else { None };
        out += &format!("capture {k}: at {got:.3}, Claude Code {want:.3}; header {:?} names {part:?}\n", rows[0].text.trim_end());
        for (i, b, top) in s.placed(&layout, rows, &sc, None) {
            out += &format!("    row {i:2} -> block {b} ({:?}) top {top}: {:?}\n", s.blocks[b].kind, rows[i].text.chars().take(50).collect::<String>());
        }
    }
    // Each block's top in the layout and where its first words are drawn, from LBTERM_REPORT_FROM
    // for LBTERM_REPORT_BLOCKS blocks: where the rows part
    let from: usize = std::env::var("LBTERM_REPORT_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let count: usize = std::env::var("LBTERM_REPORT_BLOCKS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    for b in from..(from + count).min(s.blocks.len()) {
        let block = &s.blocks[b];
        let key = block.line_keys.get(block.key_line).filter(|k| !k.is_empty()).or(block.key.as_ref());
        let drawn = key.and_then(|k| {
            let k: String = k.chars().take(16).collect();
            (layout.tops[b].saturating_sub(400)..real.total()).find(|&r| bare(&real.content[r].text).starts_with(&k))
        });
        out += &format!(
            "  block {b:4} {:?} top {} rows {}, drawn {:?}: {:?}\n",
            block.kind,
            layout.tops[b],
            block.rows(COLS),
            drawn.map(|r| r as i64 - 1 - layout.tops[b] as i64),
            block.text.chars().take(40).collect::<String>()
        );
    }
    panic!("{out}");
}

/// Claude Code 2.1.314 working, captured in a test window against a stand-in API on 2026-10-31
/// (`fixtures/claude-2.1.314-working`, the names that said whose PC it was swapped for others of
/// the same length): a long answer at the latest (answered), the next turn's spinner turning at
/// the latest (working-a, -b and -c, 130 ms and so apart), the same scrolled up half a screen
/// (up-a, -b, -c), and the turn done while scrolled up (done-up)
fn working(name: &str) -> Vec<Row> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/conversation/fixtures/claude-2.1.314-working")
        .join(format!("{name}.txt"));
    rows_of(&std::fs::read_to_string(&path).unwrap())
}

/// The rows of `b` whose words differ from `a`'s
fn changed(a: &[Row], b: &[Row]) -> Vec<usize> {
    (0..a.len().max(b.len())).filter(|&i| a.get(i).map(|r| &r.text) != b.get(i).map(|r| &r.text)).collect()
}

#[test]
fn claude_code_working_is_read_as_it_draws_it() {
    // At the latest the spinner is under the conversation, a tip under it, both in the area;
    // scrolled up the spinner isn't drawn, and the area's last row says how to get back
    for name in ["working-a", "working-b", "working-c"] {
        let rows = working(name);
        let screen = Screen::read(&rows).unwrap_or_else(|| panic!("{name}: no input box"));
        assert!(!screen.scrolled, "{name}");
        let spinners: Vec<usize> = (0..rows.len()).filter(|&i| is_spinner(&rows[i].text)).collect();
        assert_eq!(spinners, vec![screen.area - 2], "{name}: the spinner, in the area, its tip under it");
    }
    for name in ["up-a", "up-b", "up-c", "done-up"] {
        let rows = working(name);
        let screen = Screen::read(&rows).unwrap_or_else(|| panic!("{name}: no input box"));
        assert!(screen.scrolled, "{name}: scrolled up");
        assert!(screen.header, "{name}: the sticky header on the first row");
        assert!(!rows.iter().any(|r| is_spinner(&r.text)), "{name}: no spinner while scrolled up");
    }
    let answered = working("answered");
    assert!(!Screen::read(&answered).unwrap().scrolled);
    // Neither the turn's end nor a line of the conversation is a spinner
    assert!(answered.iter().all(|r| !is_spinner(&r.text)));
    assert!(!is_spinner("\u{273b} Crunched for 0s \u{b7} done 8:03"));
    assert!(!is_spinner("  * Loading\u{2026} the list"), "a list item's words");
    assert!(is_spinner("\u{2736} Forging\u{2026} "));
    assert!(is_spinner("\u{b7} Infusing\u{2026} (thinking)"));
}

#[test]
fn the_spinner_turning_changes_only_its_own_row_so_nothing_is_looked_at_again() {
    // Each tick changes its glyph and its word's shimmer, several times a second while Claude
    // Code works: only that row, which says nothing of where the view is
    let mut turned = 0;
    for (a, b) in [("working-a", "working-b"), ("working-b", "working-c"), ("working-a", "working-c")] {
        let (ra, rb) = (working(a), working(b));
        let screen = Screen::read(&rb).unwrap();
        let rows = changed(&ra, &rb);
        turned += usize::from(!rows.is_empty());
        for i in rows {
            assert!(i < screen.area && is_spinner(&rb[i].text), "{a} to {b}: row {i} changed: {:?}", rb[i].text);
        }
    }
    assert!(turned > 0, "the spinner turned between the captures");
    // Scrolling changes the conversation's rows, which aren't spinners
    let (latest, up) = (working("working-c"), working("up-a"));
    let screen = Screen::read(&up).unwrap();
    assert!(changed(&latest, &up).into_iter().any(|i| i < screen.area && !is_spinner(&up[i].text)));
}
