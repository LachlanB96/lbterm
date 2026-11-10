//! LBTerm: what printing costs the terminal model (lbterm/PERF.md, "What printing costs").
//!
//! This test binary counts every allocation its threads make, through a global allocator of its
//! own that keeps the count per thread, so tests running side by side don't add to each other's.
//! The tests hold what printing and a line's copies allocate to what they need: a 24-bit colour
//! pen shared by the cells it writes rather than copied into each (core-3), a line with a link
//! kept in its clustered form (core-8), a run of ASCII written to a line in one go (core-12).
//!
//! With `LBTERM_TERM_BENCH=<file>` set, `measured_throughput` also measures the budget runner's
//! output patterns (lbterm/perf/budgets.ps1's: plain lines, Claude Code's 24-bit colours and the
//! same in the 256-colour palette, lbterm/bench's dense SGR and its Unicode) through the model as
//! the mux does it, parsed into actions on one side and performed on the terminal on the other,
//! and a line's hyperlink scan with Lachlan's rules, and writes what each costs to the file as
//! JSON: `lbterm.ps1 test -Package wezterm-term` with it set is the bench.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;
use wezterm_escape_parser::parser::Parser;
use wezterm_escape_parser::Action;
use wezterm_surface::hyperlink::Rule;
use wezterm_term::color::ColorPalette;
use wezterm_term::{Line, Terminal, TerminalConfiguration, TerminalSize};

struct Counting;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

fn note(size: usize) {
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
    let _ = BYTES.try_with(|c| c.set(c.get() + size as u64));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        System.realloc(ptr, layout, new_size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// What `f` allocated on this thread: how many times, and how many bytes
fn allocations<R>(f: impl FnOnce() -> R) -> (R, u64, u64) {
    let (a, b) = (ALLOCS.with(Cell::get), BYTES.with(Cell::get));
    let r = f();
    (r, ALLOCS.with(Cell::get) - a, BYTES.with(Cell::get) - b)
}

#[derive(Debug)]
struct Config {
    scrollback: usize,
}

impl TerminalConfiguration for Config {
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

/// A terminal the size of the budget runner's windows (110 by 30), with Lachlan's scrollback
fn terminal(scrollback: usize) -> Terminal {
    Terminal::new(
        TerminalSize {
            rows: 30,
            cols: 110,
            pixel_width: 880,
            pixel_height: 480,
            dpi: 0,
        },
        Arc::new(Config { scrollback }),
        "WezTerm",
        "lbterm-test",
        Box::new(std::io::sink()),
    )
}

/// Lachlan's hyperlink rules (Terminal/lbterm/lbterm.lua): wezterm's defaults
/// (config::default_hyperlink_rules), a Windows path to a viewable file and a Claude session id
fn rules() -> Vec<Rule> {
    vec![
        Rule::with_highlight(r"\((\w+://\S+)\)", "$1", 1).unwrap(),
        Rule::with_highlight(r"\[(\w+://\S+)\]", "$1", 1).unwrap(),
        Rule::with_highlight(r"<(\w+://\S+)>", "$1", 1).unwrap(),
        Rule::new(wezterm_surface::hyperlink::CLOSING_PARENTHESIS_HYPERLINK_PATTERN, "$0").unwrap(),
        Rule::new(wezterm_surface::hyperlink::GENERIC_HYPERLINK_PATTERN, "$0").unwrap(),
        Rule::new(r"\b\w+@[\w-]+(\.[\w-]+)+\b", "mailto:$0").unwrap(),
        Rule::new(
            r#"\b[A-Za-z]:[\\/](?:[^\\/:*?"<>|\s]+[\\/])*[^\\/:*?"<>|\s]+\.(?:png|jpe?g|gif|bmp|webp|svg|pdf|md|txt|log|json|html?)\b"#,
            "file:///$0",
        )
        .unwrap(),
        Rule::new(
            r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
            "lbterm://session/$0",
        )
        .unwrap(),
    ]
}

/// The terminal's visible lines, copied out
fn visible(term: &Terminal) -> Vec<Line> {
    let screen = term.screen();
    screen.lines_in_phys_range(screen.phys_range(&(0..screen.physical_rows as i64)))
}

/// The visible line `row` of the terminal, copied out
fn line(term: &Terminal, row: usize) -> Line {
    visible(term).swap_remove(row)
}

// ---- The patterns: the budget runner's (lbterm/perf/budgets.ps1's LbtermBudgets.Output) ----

const E: &str = "\x1b[";

/// The throughput writer's lines: a number and 98 bytes of text, 104 bytes with CRLF
fn plain(target: usize) -> Vec<u8> {
    let text = " the quick brown fox jumps over the lazy dog 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijk";
    let mut s = String::new();
    let mut i = 0;
    while s.len() < target {
        s.push_str(&format!("{:>6}{}\r\n", i, text));
        i += 1;
    }
    s.into_bytes()
}

/// Claude Code's look: 24-bit colours, a diff's rows on a row-wide background, the boxes'
/// borders and the file paths its tools print; with `palette`, the same in the 256-colour palette
fn rgb(target: usize, palette: bool) -> Vec<u8> {
    let c = |rgb: &str, idx: u8, bg: bool| {
        if palette {
            format!("{}{};5;{}m", E, if bg { 48 } else { 38 }, idx)
        } else {
            format!("{}{};2;{}m", E, if bg { 48 } else { 38 }, rgb)
        }
    };
    let dim = c("153;153;153", 246, false);
    let white = c("255;255;255", 231, false);
    let border = c("136;136;136", 102, false);
    let green = c("78;186;101", 71, false);
    let accent = c("177;185;249", 147, false);
    let added = c("34;92;43", 22, true);
    let removed = c("122;41;54", 52, true);
    let rule: String = std::iter::repeat('\u{2500}').take(96).collect();
    let mut s = String::new();
    let mut k = 0usize;
    while s.len() < target {
        match k % 8 {
            0 => s.push_str(&format!("{border}\u{256D}{rule}\u{256E}{E}39m")),
            1 => s.push_str(&format!(
                "{green}\u{25CF} {E}1m{white}Read{E}22m{dim}(C:/Users/demo/project/src/module{}/notes{k}.md){E}39m",
                k % 997
            )),
            2 => s.push_str(&format!("{dim}  \u{2514}  Read {k} lines (ctrl+o to expand){E}39m")),
            3 | 4 => s.push_str(&format!(
                "{added}{dim}{:>6} {white}{:<90}{E}39m{E}49m",
                k % 99991,
                format!("+   let value{k} = compute(input, &options[{}]);", k % 97)
            )),
            5 => s.push_str(&format!(
                "{removed}{dim}{:>6} {white}{:<90}{E}39m{E}49m",
                k % 99991,
                format!("-   let value{k} = compute(input, options[{}]);", k % 97)
            )),
            6 => s.push_str(&format!(
                "{white}The change in {accent}C:/Users/demo/project/CLAUDE.md{white} keeps line {k}, as {E}3mnoted{E}23m before.{E}39m"
            )),
            _ => s.push_str(&format!("{border}\u{2570}{rule}\u{256F}{E}39m")),
        }
        s.push_str("\r\n");
        k += 1;
    }
    s.into_bytes()
}

/// lbterm/bench's dense SGR: 80 cells a line, each with its own 256-colour foreground and
/// background and, in turn, none, bold, italic or underline
fn sgr(target: usize) -> Vec<u8> {
    let attrs = [0, 1, 3, 4];
    let mut s = String::new();
    let mut lines = 0u64;
    while s.len() < target {
        for i in 0..80u64 {
            let n = lines * 80 + i;
            s.push_str(&format!(
                "{E}{};38;5;{};48;5;{}m{}",
                attrs[(n % 4) as usize],
                (n * 7) % 256,
                (n * 13 + 100) % 256,
                (33 + (n % 94) as u8) as char
            ));
        }
        s.push_str(&format!("{E}0m\r\n"));
        lines += 1;
    }
    s.into_bytes()
}

/// lbterm/bench's Unicode: lines 80 columns wide of wide CJK and Hangul, emoji, accented and
/// combining Latin, Greek, Cyrillic and box drawing
fn unicode(target: usize) -> Vec<u8> {
    let wide = ["漢", "字", "表", "示", "中", "文", "日", "本", "語", "한", "국", "어", "テ", "ス", "ト"];
    let emoji = ["\u{1F600}", "\u{1F680}", "\u{1F389}", "\u{1F30F}", "\u{1F525}", "\u{1F4BB}", "\u{1F40D}", "\u{1F355}"];
    let narrow = [
        "α", "β", "γ", "δ", "Ж", "З", "И", "é", "ü", "ñ", "ç", "e\u{301}", "a\u{308}", "─", "│", "┌", "┐", "└",
        "┘", "├", "┤", "€", "£",
    ];
    let mut s = String::new();
    let mut k: i64 = 0;
    while s.len() < target {
        let mut col = 0;
        while col < 80 {
            let pick = (k.wrapping_mul(2654435761)) % 7;
            k += 1;
            let at = |n: usize| (k as usize) % n;
            if pick < 2 && col + 2 <= 80 {
                s.push_str(wide[at(wide.len())]);
                col += 2;
            } else if pick == 2 && col + 2 <= 80 {
                s.push_str(emoji[at(emoji.len())]);
                col += 2;
            } else if pick == 3 {
                s.push(' ');
                col += 1;
            } else {
                s.push_str(narrow[at(narrow.len())]);
                col += 1;
            }
        }
        s.push_str("\r\n");
    }
    s.into_bytes()
}

// ---- What printing allocates ----

/// Lines of `n` characters in one pen, printed into a fresh terminal: the allocations a line
fn allocations_a_line(pen: &str, n: usize) -> f64 {
    let mut term = terminal(1000);
    let text: String = (0..n).map(|i| (b'a' + (i % 26) as u8) as char).collect();
    let mut bytes = Vec::new();
    for _ in 0..200 {
        bytes.extend_from_slice(format!("{pen}{text}{E}0m\r\n").as_bytes());
    }
    // Once to scroll the screen full, so every line measured scrolls one into the scrollback
    term.advance_bytes(&bytes);
    let ((), allocs, _) = allocations(|| term.advance_bytes(&bytes));
    allocs as f64 / 200.0
}

#[test]
fn a_truecolour_pen_is_shared_by_the_cells_it_writes() {
    // Upstream copied a pen with a 24-bit colour, and its heap block, for every character it
    // printed: 100 allocations more a line of 100 than the same in a palette colour
    let palette = allocations_a_line(&format!("{E}38;5;153m"), 100);
    let truecolour = allocations_a_line(&format!("{E}38;2;153;153;153m"), 100);
    let both = allocations_a_line(&format!("{E}38;2;153;153;153;48;2;34;92;43m"), 100);
    assert!(
        truecolour <= palette + 2.0 && both <= palette + 2.0,
        "a line of 100 in a 24-bit pen allocated {} times, with a 24-bit background too {}, in a palette pen {}",
        truecolour,
        both,
        palette
    );
}

#[test]
fn a_line_of_truecolour_cells_is_copied_without_a_heap_block_a_cell() {
    let mut term = terminal(0);
    // Written over from the line's start, so the line is held a cell at a time
    term.advance_bytes(format!("{}\r{E}38;2;10;20;30;48;2;40;50;60m{}", " ".repeat(100), "x".repeat(100)).as_bytes());
    let line = line(&term, 0);
    let (copy, allocs, _) = allocations(|| line.clone());
    assert_eq!(copy.len(), line.len());
    assert!(allocs <= 3, "a copy of a line of 100 truecolour cells allocated {} times", allocs);
}

/// The visible cells of each line, with their attributes and whether the line wrapped (which a
/// resize reflows by), and the cursor
fn cells_and_cursor(term: &Terminal) -> (Vec<String>, (usize, i64)) {
    let lines = visible(term)
        .iter()
        .map(|l| {
            format!(
                "{:?} wrapped {}",
                l.visible_cells()
                    .map(|c| (c.str().to_string(), c.attrs().clone()))
                    .collect::<Vec<_>>(),
                l.last_cell_was_wrapped()
            )
        })
        .collect();
    let at = term.cursor_pos();
    (lines, (at.x, at.y))
}

#[test]
fn a_run_of_ascii_writes_the_same_cells_as_upstream() {
    // The reference is upstream's own printing (lbterm_print_as_upstream: the parser's characters
    // one at a time, each grapheme into its line on its own), held to that by the count of runs
    // written in one go: none. A run of many must write what it writes, and so must the same
    // text a character at a time, which takes the run path as runs of one (PITFALLS 56)
    let cases: Vec<(&str, String)> = vec![
        ("a line that wraps", "x".repeat(250)),
        ("autowrap off: the last column overwritten", format!("{E}?7l{}{E}?7h", "y".repeat(130))),
        ("insert mode", format!("abcdefgh\r{E}4hXYZ{E}4l")),
        ("DEC line drawing", "\x1b(0qqqx\x1b(B ascii".to_string()),
        ("over a wide character", "\u{6F22}\u{5B57}\u{8868}\r ab".to_string()),
        ("over a line held a cell at a time", format!("{}\r{E}31mred{E}0m", "z".repeat(40))),
        ("past the end, after a cursor move", format!("ab{E}20Gcd")),
        ("left and right margins", format!("{E}?69h{E}5;20s{E}6G{}{E}?69l", "m".repeat(40))),
        ("a 24-bit pen and a link", format!("{E}38;2;1;2;3m\x1b]8;;https://example.com\x1b\\linked\x1b]8;;\x1b\\ text")),
    ];
    for (what, text) in cases {
        let mut upstream = terminal(100);
        upstream.lbterm_print_as_upstream();
        upstream.advance_bytes(text.as_bytes());
        assert_eq!(upstream.lbterm_ascii_runs(), 0, "{}: the reference printed as upstream", what);
        let mut run = terminal(100);
        run.advance_bytes(text.as_bytes());
        let mut one = terminal(100);
        for c in text.chars() {
            let mut b = [0u8; 4];
            one.advance_bytes(c.encode_utf8(&mut b).as_bytes());
        }
        let expected = cells_and_cursor(&upstream);
        let (cells, at) = cells_and_cursor(&run);
        assert_eq!(cells, expected.0, "{}: a run wrote other cells than upstream", what);
        assert_eq!(at, expected.1, "{}: a run left the cursor elsewhere", what);
        let (cells, at) = cells_and_cursor(&one);
        assert_eq!(cells, expected.0, "{}: runs of one wrote other cells than upstream", what);
        assert_eq!(at, expected.1, "{}: runs of one left the cursor elsewhere", what);
        assert!(run.lbterm_ascii_runs() > 0, "{}: the run took the run path", what);
    }
    // A combining mark joins the ASCII letter before it, in one cell
    let mut term = terminal(0);
    term.advance_bytes("cafe\u{301} e\u{301}".as_bytes());
    let cells: Vec<String> = line(&term, 0).visible_cells().map(|c| c.str().to_string()).collect();
    assert_eq!(cells, ["c", "a", "f", "e\u{301}", " ", "e\u{301}"]);
}

#[test]
fn a_line_with_a_link_stays_clustered() {
    let mut term = terminal(0);
    term.advance_bytes(
        format!("{E}38;2;153;153;153m  Read(C:/Users/demo/project/src/notes.md) and C:/Users/demo/CLAUDE.md{E}0m").as_bytes(),
    );
    let mut line = line(&term, 0);
    let rules = rules();
    let ((), _, scan_bytes) = allocations(|| line.scan_and_create_hyperlinks(&rules));
    let linked: Vec<_> = line.visible_cells().filter(|c| c.attrs().hyperlink().is_some()).map(|c| c.cell_index()).collect();
    assert_eq!(linked.len(), "C:/Users/demo/project/src/notes.md".len() + "C:/Users/demo/CLAUDE.md".len(), "the cells linked");
    assert_eq!(linked[0], 7, "the first link's first cell");
    let (_, _, copy_bytes) = allocations(|| line.clone());
    // Clustered, its copy is the text and a few runs of attributes; a cell at a time it's 24
    // bytes a cell (2,040 here), more with the links' heap blocks
    assert!(
        copy_bytes < 800,
        "a copy of the scanned line allocated {} bytes (the scan {}): it's held a cell at a time",
        copy_bytes,
        scan_bytes
    );
}

// ---- The bench ----

/// Each pattern's bytes, parsed in 128 KB reads as the mux's parser thread does them
/// (mux_output_parser_buffer_size), then performed on a terminal: what each side costs
fn measure(name: &str, bytes: &[u8]) -> String {
    let mut term = terminal(50000);
    // Warm: the scrollback full, as a busy pane's is
    let warm = &bytes[..bytes.len().min(4 << 20)];
    term.advance_bytes(warm);
    let mut parser = Parser::new();
    // As mux's parse_buffered_data sets it
    parser.lbterm_print_runs(true);
    let mut batches: Vec<Vec<Action>> = Vec::new();
    let t = Instant::now();
    let ((), parse_allocs, _) = allocations(|| {
        for chunk in bytes.chunks(128 * 1024) {
            let mut actions = Vec::new();
            parser.parse(chunk, |a| a.append_to(&mut actions));
            batches.push(actions);
        }
    });
    let parse_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let ((), perform_allocs, _) = allocations(|| {
        for actions in batches {
            term.perform_actions(actions);
        }
    });
    let perform_ms = t.elapsed().as_secs_f64() * 1000.0;
    let mb = bytes.len() as f64 / (1024.0 * 1024.0);
    format!(
        "\"{name}\": {{ \"mb\": {mb:.2}, \"parse_ms\": {parse_ms:.1}, \"perform_ms\": {perform_ms:.1}, \"parse_ns_per_byte\": {:.2}, \"perform_ns_per_byte\": {:.2}, \"mb_per_s\": {:.1}, \"parse_allocs_per_mb\": {:.0}, \"perform_allocs_per_mb\": {:.0} }}",
        parse_ms * 1e6 / bytes.len() as f64,
        perform_ms * 1e6 / bytes.len() as f64,
        mb / ((parse_ms + perform_ms) / 1000.0),
        parse_allocs as f64 / mb,
        perform_allocs as f64 / mb,
    )
}

/// Lachlan's hyperlink rules over the lines a pattern leaves on the screen, as the GUI scans each
/// line that reaches it: the time and allocations a line, and what a copy of it allocates after
/// A walk of the cells of the lines a pattern leaves on the screen, as the GUI's drawing, its
/// shape hash, its zones and its triggers walk them: ns a cell
fn measure_cells(name: &str, bytes: &[u8]) -> String {
    let mut term = terminal(50000);
    term.advance_bytes(&bytes[..bytes.len().min(1 << 20)]);
    let lines = visible(&term);
    let rounds = 2000;
    let mut cells = 0usize;
    let t = Instant::now();
    for _ in 0..rounds {
        for l in &lines {
            cells += l.visible_cells().map(|c| std::hint::black_box(c.str().len())).count();
        }
    }
    format!("\"{name}\": {:.2}", t.elapsed().as_secs_f64() * 1e9 / cells as f64)
}

fn measure_links(name: &str, bytes: &[u8]) -> String {
    let mut term = terminal(50000);
    term.advance_bytes(&bytes[..bytes.len().min(1 << 20)]);
    let rules = rules();
    let lines = visible(&term);
    let mut scanned = Vec::new();
    let rounds = 50;
    let t = Instant::now();
    let ((), allocs, _) = allocations(|| {
        for _ in 0..rounds {
            scanned.clear();
            for l in &lines {
                let mut l = l.clone();
                l.scan_and_create_hyperlinks(&rules);
                scanned.push(l);
            }
        }
    });
    let n = (rounds * lines.len()) as f64;
    let us = t.elapsed().as_secs_f64() * 1e6 / n;
    let linked = scanned.iter().filter(|l| l.has_hyperlink()).count();
    // Each rule on its own over the same lines: what its scan costs a line, and its regex's
    // is_match alone
    let texts: Vec<String> = lines.iter().map(|l| l.as_str().into_owned()).collect();
    let mut per_rule = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        let one = std::slice::from_ref(rule);
        let t = Instant::now();
        for _ in 0..rounds {
            for text in &texts {
                std::hint::black_box(Rule::match_hyperlinks(text, one));
            }
        }
        let all = t.elapsed().as_secs_f64() * 1e6 / n;
        let t = Instant::now();
        for _ in 0..rounds {
            for text in &texts {
                std::hint::black_box(rule.regex.is_match(text).ok());
            }
        }
        let is_match = t.elapsed().as_secs_f64() * 1e6 / n;
        per_rule.push(format!("\"{}\": [{:.2}, {:.2}]", i, all, is_match));
    }
    let (_, copy_allocs, copy_bytes) = allocations(|| scanned.iter().map(|l| l.clone()).collect::<Vec<_>>());
    format!(
        "\"{name}\": {{ \"lines\": {}, \"linked\": {linked}, \"scan_us_a_line\": {us:.2}, \"scan_allocs_a_line\": {:.1}, \"copy_allocs_a_line\": {:.1}, \"copy_bytes_a_line\": {:.0}, \"rule_us_a_line\": {{ {} }} }}",
        lines.len(),
        allocs as f64 / n,
        copy_allocs as f64 / scanned.len() as f64,
        copy_bytes as f64 / scanned.len() as f64,
        per_rule.join(", "),
    )
}

#[test]
fn measured_throughput() {
    let out = match std::env::var("LBTERM_TERM_BENCH") {
        Ok(path) if !path.is_empty() => path,
        _ => return,
    };
    let size = 16 << 20;
    let mut parts = vec![
        measure("plain", &plain(size)),
        measure("rgb", &rgb(size, false)),
        measure("rgb-palette", &rgb(size, true)),
        measure("sgr", &sgr(size)),
        measure("unicode", &unicode(size)),
    ];
    let links = vec![measure_links("rgb", &rgb(1 << 20, false)), measure_links("plain", &plain(1 << 20))];
    parts.push(format!("\"links\": {{ {} }}", links.join(", ")));
    let cells = vec![
        measure_cells("plain", &plain(1 << 20)),
        measure_cells("rgb", &rgb(1 << 20, false)),
        measure_cells("unicode", &unicode(1 << 20)),
    ];
    parts.push(format!("\"cells\": {{ {} }}", cells.join(", ")));
    std::fs::write(&out, format!("{{ {} }}\n", parts.join(",\n  "))).unwrap();
}
