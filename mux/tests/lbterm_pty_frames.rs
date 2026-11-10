//! LBTerm: a pane's pty recording (`mux/src/lbterm_ptyrec.rs`, `LBTERM_PTY_RECORD`) read back
//! through the terminal model as the mux's parser thread does it (lbterm/FLOWS.md, the
//! pane-output flow, rows 1 to 13), to say three things about how a program writes:
//!
//! - its frames: the reads that come together, bursts of reads less than a frame gap apart
//!   (`LBTERM_PTY_FRAME_GAP_US`, 2,000 µs unless set), each with its reads, bytes, span and the
//!   gaps between its pieces, and every gap between two reads;
//! - what each batch the parser would have performed does to the screen: the 3 ms coalescing,
//!   the synchronized-output hold and the 128 KB cap of `parse_buffered_data` played over the
//!   reads' times, each batch performed on a terminal of the pane's size (and every resize), then
//!   each row of the screen compared with the screen before by what it shows (its shape hash,
//!   which the GUI's line quad cache keys a line's look by): unchanged, blank, seen here (what the
//!   row showed in an earlier frame, a hit for main's key, which holds the row's position), moved
//!   (shown at another row of the screen before) or seen elsewhere (drawn at another row
//!   earlier), both hits only for a key by content, within the cache's 4,096 lines, or new; and
//!   the commonest offset of the moved rows, so a scroll shows as rows moved by one offset; and
//!   the same at most once a 16.7 ms, as a window draws them;
//! - synchronized output (DECSET 2026): the sets, resets and DECRQM queries in the stream, and
//!   each frame they bracket, with its reads, span and gaps.
//!
//! Its tests hold the analysis to recordings of known frames made here. With
//! `LBTERM_PTY_FRAMES=<folder>` set, `pty_frames_of_the_recordings_in_a_folder` reads every
//! `*.ptyrec` there (and the folder's `marks.tsv`, a script's phases: a line each of nanoseconds
//! since the Unix epoch, a tab and the phase's name) and writes beside each `<name>.frames.tsv`,
//! `<name>.batches.tsv` and `<name>.summary.json`: `lbterm/bench/pty-frames.ps1` runs it.

use mux::lbterm_ptyrec::{self, Event, Recording};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use termwiz::escape::csi::{Cursor, DecPrivateMode, DecPrivateModeCode, Device, Edit, Mode};
use termwiz::escape::parser::Parser;
use termwiz::escape::{Action, ControlCode, Esc, EscCode, CSI};
use wezterm_term::color::{ColorAttribute, ColorPalette};
use wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};

/// `mux_output_parser_coalesce_delay_ms`'s default, which Lachlan's config doesn't change
const COALESCE_NS: u64 = 3_000_000;
/// `mux_output_parser_buffer_size`'s default: a recv's most, and the batch's cap
const PARSER_BUF: usize = 128 * 1024;
/// The GUI line quad cache's entries (`render/mod.rs`), for "seen before"
const CACHE_LINES: usize = 4096;
/// A window's frame at 60 Hz
const TICK_NS: u64 = 16_666_667;
/// Gap buckets, in µs: each bucket is from its bound to the next
const GAP_BOUNDS_US: [u64; 13] = [
    0, 50, 100, 200, 500, 1_000, 2_000, 3_000, 5_000, 10_000, 20_000, 50_000, 100_000,
];

#[derive(Debug)]
struct Config;

impl TerminalConfiguration for Config {
    fn scrollback_size(&self) -> usize {
        // Lachlan's
        50_000
    }
    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

fn terminal(rows: usize, cols: usize) -> Terminal {
    // As LocalDomain::spawn_pane makes a pane's, with ConPTY's quirks on
    let mut term = Terminal::new(
        size(rows, cols),
        Arc::new(Config),
        "WezTerm",
        "lbterm-test",
        Box::new(std::io::sink()),
    );
    term.enable_conpty_quirks();
    term
}

fn size(rows: usize, cols: usize) -> TerminalSize {
    TerminalSize {
        rows,
        cols,
        pixel_width: cols * 8,
        pixel_height: rows * 16,
        dpi: 0,
    }
}

// ---- What each read holds ----

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sync {
    Set,
    Reset,
    Query,
    SoftReset,
}

fn sync_of(action: &Action) -> Option<Sync> {
    let code = |m: &DecPrivateMode| matches!(m, DecPrivateMode::Code(DecPrivateModeCode::SynchronizedOutput));
    match action {
        Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(m))) if code(m) => Some(Sync::Set),
        Action::CSI(CSI::Mode(Mode::ResetDecPrivateMode(m))) if code(m) => Some(Sync::Reset),
        Action::CSI(CSI::Mode(Mode::QueryDecPrivateMode(m))) if code(m) => Some(Sync::Query),
        Action::CSI(CSI::Device(dev)) if matches!(**dev, Device::SoftReset) => Some(Sync::SoftReset),
        _ => None,
    }
}

/// What a read (or a frame's reads) held, as the parser takes it
#[derive(Default, Clone, Debug)]
struct Seen {
    sync_set: u32,
    sync_reset: u32,
    sync_query: u32,
    line_feeds: u32,
    /// Scrolls asked for directly: SU, SD, IL, DL, RI
    scroll_ops: u32,
    /// DECSTBM
    margins: u32,
    cursor: u32,
    /// EL, ED, ECH
    erases: u32,
    sgr: u32,
    printed: u32,
    osc: u32,
    alt_screen: u32,
}

impl Seen {
    fn note(&mut self, action: &Action) {
        match sync_of(action) {
            Some(Sync::Set) => self.sync_set += 1,
            Some(Sync::Reset) => self.sync_reset += 1,
            Some(Sync::Query) => self.sync_query += 1,
            _ => {}
        }
        let alt = |m: &DecPrivateMode| {
            matches!(
                m,
                DecPrivateMode::Code(
                    DecPrivateModeCode::ClearAndEnableAlternateScreen
                        | DecPrivateModeCode::EnableAlternateScreen
                        | DecPrivateModeCode::OptEnableAlternateScreen
                )
            )
        };
        match action {
            Action::Print(_) => self.printed += 1,
            Action::PrintString(s) => self.printed += s.chars().count() as u32,
            Action::Control(ControlCode::LineFeed) => self.line_feeds += 1,
            Action::OperatingSystemCommand(_) => self.osc += 1,
            Action::Esc(Esc::Code(EscCode::ReverseIndex)) => self.scroll_ops += 1,
            Action::CSI(CSI::Sgr(_)) => self.sgr += 1,
            Action::CSI(CSI::Cursor(Cursor::SetTopAndBottomMargins { .. })) => self.margins += 1,
            Action::CSI(CSI::Cursor(_)) => self.cursor += 1,
            Action::CSI(CSI::Edit(
                Edit::ScrollUp(_) | Edit::ScrollDown(_) | Edit::InsertLine(_) | Edit::DeleteLine(_),
            )) => self.scroll_ops += 1,
            Action::CSI(CSI::Edit(
                Edit::EraseInLine(_) | Edit::EraseInDisplay(_) | Edit::EraseCharacter(_),
            )) => self.erases += 1,
            Action::CSI(CSI::Mode(Mode::SetDecPrivateMode(m) | Mode::ResetDecPrivateMode(m)))
                if alt(m) =>
            {
                self.alt_screen += 1
            }
            _ => {}
        }
    }

    fn add(&mut self, o: &Seen) {
        self.sync_set += o.sync_set;
        self.sync_reset += o.sync_reset;
        self.sync_query += o.sync_query;
        self.line_feeds += o.line_feeds;
        self.scroll_ops += o.scroll_ops;
        self.margins += o.margins;
        self.cursor += o.cursor;
        self.erases += o.erases;
        self.sgr += o.sgr;
        self.printed += o.printed;
        self.osc += o.osc;
        self.alt_screen += o.alt_screen;
    }
}

/// A read, with what it held and the synchronized-output edges in it, in order
struct ReadInfo {
    at_ns: u64,
    len: usize,
    seen: Seen,
    edges: Vec<Sync>,
    /// Its first action set synchronized output, and its last edge reset it
    opens: bool,
    closes: bool,
}

fn reads_of(rec: &Recording) -> Vec<ReadInfo> {
    let mut parser = Parser::new();
    parser.lbterm_print_runs(true);
    let mut out = vec![];
    for e in &rec.events {
        if let Event::Read { at_ns, bytes } = e {
            let mut info = ReadInfo {
                at_ns: *at_ns,
                len: bytes.len(),
                seen: Seen::default(),
                edges: vec![],
                opens: false,
                closes: false,
            };
            let mut first = true;
            let mut last = None;
            parser.parse(bytes, |action| {
                let edge = sync_of(&action);
                if first {
                    info.opens = edge == Some(Sync::Set);
                    first = false;
                }
                if let Some(edge) = edge {
                    info.edges.push(edge);
                }
                info.seen.note(&action);
                if edge.is_some() {
                    last = edge;
                }
            });
            info.closes = last == Some(Sync::Reset);
            out.push(info);
        }
    }
    out
}

// ---- Frames: bursts of reads ----

#[derive(Debug)]
struct Frame {
    first: usize,
    reads: usize,
    bytes: usize,
    start_ns: u64,
    span_ns: u64,
    max_gap_ns: u64,
    /// From the last read of the frame before
    gap_before_ns: Option<u64>,
    /// Its first read opened with DECSET 2026 and its last closed with DECRST 2026
    bracketed: bool,
    seen: Seen,
}

fn frames_of(reads: &[ReadInfo], gap_ns: u64) -> Vec<Frame> {
    let mut frames: Vec<Frame> = vec![];
    for (i, r) in reads.iter().enumerate() {
        let join = match frames.last() {
            Some(f) => r.at_ns - (f.start_ns + f.span_ns) < gap_ns,
            None => false,
        };
        if join {
            let f = frames.last_mut().unwrap();
            let gap = r.at_ns - (f.start_ns + f.span_ns);
            f.max_gap_ns = f.max_gap_ns.max(gap);
            f.reads += 1;
            f.bytes += r.len;
            f.span_ns = r.at_ns - f.start_ns;
            f.seen.add(&r.seen);
        } else {
            let gap_before_ns = frames.last().map(|f| r.at_ns - (f.start_ns + f.span_ns));
            frames.push(Frame {
                first: i,
                reads: 1,
                bytes: r.len,
                start_ns: r.at_ns,
                span_ns: 0,
                max_gap_ns: 0,
                gap_before_ns,
                bracketed: false,
                seen: r.seen.clone(),
            });
        }
    }
    for f in &mut frames {
        f.bracketed = reads[f.first].opens && reads[f.first + f.reads - 1].closes;
    }
    frames
}

/// A synchronized frame, from its DECSET 2026 to its DECRST 2026
#[derive(Debug)]
struct Span {
    start_ns: u64,
    reads: usize,
    span_ns: u64,
    max_gap_ns: u64,
}

fn spans_of(reads: &[ReadInfo]) -> Vec<Span> {
    let mut spans = vec![];
    let mut open: Option<usize> = None;
    for (i, r) in reads.iter().enumerate() {
        for e in &r.edges {
            match e {
                Sync::Set => {
                    if open.is_none() {
                        open = Some(i);
                    }
                }
                Sync::Reset | Sync::SoftReset => {
                    if let Some(first) = open.take() {
                        let max_gap_ns = (first..i)
                            .map(|k| reads[k + 1].at_ns - reads[k].at_ns)
                            .max()
                            .unwrap_or(0);
                        spans.push(Span {
                            start_ns: reads[first].at_ns,
                            reads: i - first + 1,
                            span_ns: r.at_ns - reads[first].at_ns,
                            max_gap_ns,
                        });
                    }
                }
                Sync::Query => {}
            }
        }
    }
    spans
}

// ---- Batches: what the parser would have performed, and what each did to the screen ----

/// A screen's rows by what each shows
#[derive(Clone, Default)]
struct Shot {
    rows: Vec<[u8; 16]>,
    blank: Vec<bool>,
}

fn shot(term: &Terminal) -> Shot {
    let screen = term.screen();
    let lines = screen.lines_in_phys_range(screen.phys_range(&(0..screen.physical_rows as i64)));
    Shot {
        rows: lines.iter().map(|l| l.compute_shape_hash()).collect(),
        blank: lines
            .iter()
            .map(|l| l.visible_cells().all(|c| c.str() == " " && c.attrs().background() == ColorAttribute::Default))
            .collect(),
    }
}

/// The lines drawn lately, as many as the GUI's line quad cache holds: by what each showed (a
/// cache keyed by content), and by what and where (one keyed by the row's position too, as
/// `LineQuadCacheKey` is on main)
#[derive(Default)]
struct Recent {
    by_content: Bounded<[u8; 16]>,
    by_row: Bounded<([u8; 16], usize)>,
}

#[derive(Default)]
struct Bounded<K: std::hash::Hash + Eq + Copy> {
    set: HashSet<K>,
    order: VecDeque<K>,
}

impl<K: std::hash::Hash + Eq + Copy> Bounded<K> {
    fn add(&mut self, k: K) {
        if self.set.insert(k) {
            self.order.push_back(k);
            if self.order.len() > CACHE_LINES {
                let old = self.order.pop_front().unwrap();
                self.set.remove(&old);
            }
        }
    }
}

impl Recent {
    fn add(&mut self, shot: &Shot) {
        for (i, h) in shot.rows.iter().enumerate() {
            self.by_content.add(*h);
            self.by_row.add((*h, i));
        }
    }
}

/// One screen against the one before it, row by row
#[derive(Default, Clone, Debug, PartialEq)]
struct Change {
    rows: usize,
    /// Rows that show something other than they did
    changed: usize,
    /// Changed rows that are blank now
    blank: usize,
    /// Changed rows showing what this row showed in an earlier frame: a hit for a cache keyed by
    /// the row's position too, as on main
    seen_here: usize,
    /// Changed rows showing what another row of the screen before showed: a hit only for a cache
    /// keyed by content
    moved: usize,
    /// Changed rows showing a line drawn earlier at another row, not on the screen before: a hit
    /// only for a cache keyed by content
    seen_elsewhere: usize,
    /// Changed rows showing what wasn't drawn lately: laid out whatever the key
    new: usize,
    /// The commonest offset of the rows moved (rows up), and how many moved by it
    shift: i64,
    shifted: usize,
}

impl Change {
    fn kind(&self) -> &'static str {
        if self.changed == 0 {
            "same"
        } else if self.shift != 0 && self.shifted >= 2 {
            "scroll"
        } else {
            "in place"
        }
    }
}

fn compare(before: &Shot, after: &Shot, recent: &Recent) -> Change {
    let mut c = Change {
        rows: after.rows.len(),
        ..Default::default()
    };
    // Where each line was on the screen before: None when on more than one row
    let mut at: HashMap<[u8; 16], Option<usize>> = HashMap::new();
    for (i, h) in before.rows.iter().enumerate() {
        at.entry(*h).and_modify(|e| *e = None).or_insert(Some(i));
    }
    let mut shifts: HashMap<i64, usize> = HashMap::new();
    for (i, h) in after.rows.iter().enumerate() {
        if before.rows.get(i) == Some(h) {
            continue;
        }
        c.changed += 1;
        if after.blank[i] {
            c.blank += 1;
            continue;
        }
        if recent.by_row.set.contains(&(*h, i)) {
            c.seen_here += 1;
            continue;
        }
        match at.get(h) {
            Some(Some(j)) => {
                c.moved += 1;
                *shifts.entry(*j as i64 - i as i64).or_default() += 1;
            }
            Some(None) => c.moved += 1,
            None if recent.by_content.set.contains(h) => c.seen_elsewhere += 1,
            None => c.new += 1,
        }
    }
    if let Some((s, n)) = shifts.into_iter().max_by_key(|&(s, n)| (n, -s.abs())) {
        c.shift = s;
        c.shifted = n;
    }
    c
}

#[derive(Debug)]
struct Batch {
    /// When the parser performed it
    at_ns: u64,
    /// Its first read's time
    first_ns: u64,
    reads: usize,
    bytes: usize,
    /// Performed at a synchronized frame's edge, not at the coalescing's end
    edge: bool,
    change: Change,
    /// Rows the model marked changed (what the server's next push carries)
    dirty: usize,
    alt: bool,
    resized: bool,
}

/// The batches the parser thread makes of the reads, as `parse_buffered_data` makes them: a
/// recv takes every read that has come (up to its buffer); a batch is performed at a
/// synchronized frame's edges, or when no read has come by the coalescing's deadline, which is
/// set at its first pass and slept out whole (PITFALLS 82 and 84); and each batch's screen
fn batches_of(rec: &Recording) -> (Vec<Batch>, Vec<(u64, Shot)>) {
    let reads: Vec<(u64, &[u8])> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Read { at_ns, bytes } => Some((*at_ns, bytes.as_slice())),
            _ => None,
        })
        .collect();
    let sizes: Vec<(u64, usize, usize)> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Size { at_ns, rows, cols } => Some((*at_ns, *rows as usize, *cols as usize)),
            _ => None,
        })
        .collect();
    let (rows, cols) = sizes.first().map(|s| (s.1, s.2)).unwrap_or((24, 80));
    let mut term = terminal(rows, cols);
    let mut before = shot(&term);
    let mut recent = Recent::default();
    recent.add(&before);
    let mut batches = vec![];
    let mut shots = vec![];

    let mut parser = Parser::new();
    parser.lbterm_print_runs(true);
    let mut actions: Vec<Action> = vec![];
    let mut hold = false;
    let mut action_size = 0usize;
    let mut deadline: Option<u64> = None;
    let mut now = 0u64;
    let mut next = 0usize;
    let mut next_size = 1usize;
    let mut resized = false;
    // The batch being gathered: its first read's time, its reads and bytes
    let mut gathering: Option<(u64, usize, usize)> = None;

    // Performs a batch on the terminal and compares its screen with the last
    let mut perform = |actions: Vec<Action>,
                       at_ns: u64,
                       edge: bool,
                       gathered: (u64, usize, usize),
                       resized: &mut bool,
                       term: &mut Terminal| {
        let seqno = term.current_seqno();
        term.perform_actions(actions);
        let after = shot(term);
        let screen = term.screen();
        let dirty = screen
            .lines_in_phys_range(screen.phys_range(&(0..screen.physical_rows as i64)))
            .iter()
            .filter(|l| l.current_seqno() > seqno)
            .count();
        let change = compare(&before, &after, &recent);
        recent.add(&after);
        batches.push(Batch {
            at_ns,
            first_ns: gathered.0,
            reads: gathered.1,
            bytes: gathered.2,
            edge,
            change,
            dirty,
            alt: term.is_alt_screen_active(),
            resized: std::mem::take(resized),
        });
        shots.push((at_ns, after.clone()));
        before = after;
    };

    while next < reads.len() {
        // Blocked in recv until the next read comes, unless one has
        now = now.max(reads[next].0);
        // The main thread's resizes by then
        while next_size < sizes.len() && sizes[next_size].0 <= now {
            let (_, r, c) = sizes[next_size];
            term.resize(size(r, c));
            resized = true;
            next_size += 1;
        }
        // One recv: every read come by now, up to the buffer
        let mut got = 0;
        let mut flushed: Vec<(Vec<Action>, (u64, usize, usize))> = vec![];
        while next < reads.len()
            && reads[next].0 <= now
            && (got == 0 || got + reads[next].1.len() <= PARSER_BUF)
        {
            let (at, bytes) = reads[next];
            got += bytes.len();
            next += 1;
            parser.parse(bytes, |action| {
                let mut flush = false;
                match sync_of(&action) {
                    Some(Sync::Set) => {
                        hold = true;
                        flush = true;
                    }
                    Some(Sync::Reset) | Some(Sync::SoftReset) => {
                        hold = false;
                        flush = true;
                    }
                    _ => {}
                }
                action.append_to(&mut actions);
                if flush && !actions.is_empty() {
                    // This read in the batch: one an edge splits counts in the batch after too
                    let mut g = gathering.take().unwrap_or((at, 0, 0));
                    g.1 += 1;
                    g.2 += bytes.len();
                    flushed.push((std::mem::take(&mut actions), g));
                    action_size = 0;
                }
            });
            if !actions.is_empty() {
                let g = gathering.get_or_insert((at, 0, 0));
                g.1 += 1;
                g.2 += bytes.len();
            }
        }
        for (a, g) in flushed {
            perform(a, now, true, g, &mut resized, &mut term);
        }
        action_size += got;
        if !actions.is_empty() && !hold {
            if action_size < PARSER_BUF {
                let poll_delay = match deadline {
                    None => {
                        deadline = Some(now + COALESCE_NS);
                        Some(COALESCE_NS)
                    }
                    Some(d) => d.checked_sub(now),
                };
                if let Some(wait) = poll_delay {
                    // Slept out whole, then the socket asked
                    now += wait;
                    if next < reads.len() && reads[next].0 <= now {
                        continue;
                    }
                }
            }
            let g = gathering.take().unwrap_or((now, 0, 0));
            perform(std::mem::take(&mut actions), now, false, g, &mut resized, &mut term);
            deadline = None;
            action_size = 0;
        }
    }
    if !actions.is_empty() {
        let g = gathering.take().unwrap_or((now, 0, 0));
        perform(std::mem::take(&mut actions), now, false, g, &mut resized, &mut term);
    }
    (batches, shots)
}

/// The screens a window drawing at most once a 16.7 ms would have shown: the last batch's of
/// each tick that had one, each against the one drawn before
fn ticks_of(shots: &[(u64, Shot)]) -> Vec<(u64, Change)> {
    let mut out = vec![];
    let mut recent = Recent::default();
    let mut drawn: Option<&Shot> = None;
    let mut i = 0;
    while i < shots.len() {
        let tick = shots[i].0 / TICK_NS;
        let mut last = i;
        while last + 1 < shots.len() && shots[last + 1].0 / TICK_NS == tick {
            last += 1;
        }
        let s = &shots[last].1;
        if let Some(d) = drawn {
            out.push(((tick + 1) * TICK_NS, compare(d, s, &recent)));
        }
        recent.add(s);
        drawn = Some(s);
        i = last + 1;
    }
    out
}

// ---- Phases, and the report ----

/// A script's marks: (ns since the recording's start, name), in order
fn marks_for(folder: &Path, rec: &Recording) -> Vec<(u64, String)> {
    let text = std::fs::read_to_string(folder.join("marks.tsv")).unwrap_or_default();
    let mut marks: Vec<(u64, String)> = text
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(2, '\t');
            let ns: u64 = parts.next()?.trim().parse().ok()?;
            let name = parts.next()?.trim().to_string();
            Some((ns.saturating_sub(rec.start_unix_ns), name))
        })
        .collect();
    marks.sort_by_key(|m| m.0);
    marks
}

fn phase_at(marks: &[(u64, String)], at_ns: u64) -> &str {
    marks
        .iter()
        .rev()
        .find(|m| m.0 <= at_ns)
        .map(|m| m.1.as_str())
        .unwrap_or("start")
}

fn percentile(v: &mut Vec<u64>, p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v[i]
}

fn dist(mut v: Vec<u64>) -> String {
    let n = v.len();
    let sum: u64 = v.iter().sum();
    format!(
        "{{ \"n\": {}, \"mean\": {:.1}, \"p50\": {}, \"p90\": {}, \"p99\": {}, \"max\": {} }}",
        n,
        if n > 0 { sum as f64 / n as f64 } else { 0.0 },
        percentile(&mut v, 0.5),
        percentile(&mut v, 0.9),
        percentile(&mut v, 0.99),
        percentile(&mut v, 1.0)
    )
}

fn gap_buckets(gaps_ns: impl Iterator<Item = u64>) -> String {
    let mut counts = [0usize; GAP_BOUNDS_US.len()];
    for g in gaps_ns {
        let us = g / 1000;
        let b = GAP_BOUNDS_US.iter().rposition(|&lo| us >= lo).unwrap_or(0);
        counts[b] += 1;
    }
    let parts: Vec<String> = GAP_BOUNDS_US
        .iter()
        .enumerate()
        .map(|(i, lo)| {
            let label = match GAP_BOUNDS_US.get(i + 1) {
                Some(hi) => format!("{}-{}", lo, hi),
                None => format!("{}+", lo),
            };
            format!("\"{}\": {}", label, counts[i])
        })
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

fn change_totals(changes: &[&Change]) -> String {
    let sum = |f: fn(&Change) -> usize| changes.iter().map(|c| f(c)).sum::<usize>();
    let kinds = ["same", "in place", "scroll"];
    let kinds: Vec<String> = kinds
        .iter()
        .map(|k| format!("\"{}\": {}", k, changes.iter().filter(|c| c.kind() == *k).count()))
        .collect();
    let mut shifts: HashMap<i64, usize> = HashMap::new();
    for c in changes.iter().filter(|c| c.kind() == "scroll") {
        *shifts.entry(c.shift).or_default() += 1;
    }
    let mut shifts: Vec<(i64, usize)> = shifts.into_iter().collect();
    shifts.sort();
    let shifts: Vec<String> = shifts.iter().map(|(s, n)| format!("\"{}\": {}", s, n)).collect();
    let scrolls: Vec<&&Change> = changes.iter().filter(|c| c.kind() == "scroll").collect();
    let in_scrolls = |f: fn(&Change) -> usize| scrolls.iter().map(|c| f(c)).sum::<usize>();
    format!(
        "{{ \"n\": {}, \"kinds\": {{ {} }}, \"rows\": {}, \"changed\": {}, \"blank\": {}, \"seen_here\": {}, \"moved\": {}, \"seen_elsewhere\": {}, \"new\": {}, \"laid_out_by_row\": {}, \"laid_out_by_content\": {}, \"scroll_shifts\": {{ {} }}, \"in_scrolls\": {{ \"n\": {}, \"changed\": {}, \"blank\": {}, \"seen_here\": {}, \"moved\": {}, \"seen_elsewhere\": {}, \"new\": {} }} }}",
        changes.len(),
        kinds.join(", "),
        sum(|c| c.rows),
        sum(|c| c.changed),
        sum(|c| c.blank),
        sum(|c| c.seen_here),
        sum(|c| c.moved),
        sum(|c| c.seen_elsewhere),
        sum(|c| c.new),
        // Rows a frame lays out again: keyed by row, every changed row its row's own history
        // doesn't hold; keyed by content, only what wasn't drawn lately (a blank line always was)
        sum(|c| c.changed - c.seen_here),
        sum(|c| c.new),
        shifts.join(", "),
        scrolls.len(),
        in_scrolls(|c| c.changed),
        in_scrolls(|c| c.blank),
        in_scrolls(|c| c.seen_here),
        in_scrolls(|c| c.moved),
        in_scrolls(|c| c.seen_elsewhere),
        in_scrolls(|c| c.new),
    )
}

struct Analysis {
    reads: Vec<ReadInfo>,
    frames: Vec<Frame>,
    spans: Vec<Span>,
    batches: Vec<Batch>,
    ticks: Vec<(u64, Change)>,
}

fn analyse(rec: &Recording, gap_ns: u64) -> Analysis {
    let reads = reads_of(rec);
    let frames = frames_of(&reads, gap_ns);
    let spans = spans_of(&reads);
    let (batches, shots) = batches_of(rec);
    let ticks = ticks_of(&shots);
    Analysis {
        reads,
        frames,
        spans,
        batches,
        ticks,
    }
}

fn ms(ns: u64) -> String {
    format!("{:.3}", ns as f64 / 1e6)
}

/// The per-frame and per-batch tables, and the summary, of one recording
fn report(rec: &Recording, a: &Analysis, marks: &[(u64, String)], gap_ns: u64) -> (String, String, String) {
    let mut frames = String::from(
        "frame\tphase\tstart_ms\tgap_before_ms\treads\tbytes\tspan_us\tmax_gap_us\tbracketed\tsync_set\tsync_reset\tsync_query\tline_feeds\tscroll_ops\tmargins\tcursor\terases\tsgr\tprinted\tosc\n",
    );
    for (n, f) in a.frames.iter().enumerate() {
        let s = &f.seen;
        let _ = writeln!(
            frames,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            n,
            phase_at(marks, f.start_ns),
            ms(f.start_ns),
            f.gap_before_ns.map(ms).unwrap_or_default(),
            f.reads,
            f.bytes,
            f.span_ns / 1000,
            f.max_gap_ns / 1000,
            f.bracketed as u8,
            s.sync_set,
            s.sync_reset,
            s.sync_query,
            s.line_feeds,
            s.scroll_ops,
            s.margins,
            s.cursor,
            s.erases,
            s.sgr,
            s.printed,
            s.osc
        );
    }
    let mut batches = String::from(
        "batch\tphase\tat_ms\twait_us\treads\tbytes\tedge\trows\tchanged\tblank\tseen_here\tmoved\tseen_elsewhere\tnew\tshift\tshifted\tdirty\tkind\talt\tresized\n",
    );
    for (n, b) in a.batches.iter().enumerate() {
        let c = &b.change;
        let _ = writeln!(
            batches,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            n,
            phase_at(marks, b.at_ns),
            ms(b.at_ns),
            b.at_ns.saturating_sub(b.first_ns) / 1000,
            b.reads,
            b.bytes,
            b.edge as u8,
            c.rows,
            c.changed,
            c.blank,
            c.seen_here,
            c.moved,
            c.seen_elsewhere,
            c.new,
            c.shift,
            c.shifted,
            b.dirty,
            c.kind(),
            b.alt as u8,
            b.resized as u8
        );
    }

    // The summary, the whole recording and each phase
    let mut names: Vec<String> = vec!["all".to_string()];
    for m in marks {
        if !names.contains(&m.1) {
            names.push(m.1.clone());
        }
    }
    if a.reads.iter().any(|r| phase_at(marks, r.at_ns) == "start") && !names.iter().any(|n| n == "start") {
        names.insert(1, "start".to_string());
    }
    let in_phase = |name: &str, at: u64| name == "all" || phase_at(marks, at) == name;
    let mut phases = vec![];
    for name in &names {
        let reads: Vec<&ReadInfo> = a.reads.iter().filter(|r| in_phase(name, r.at_ns)).collect();
        if reads.is_empty() {
            continue;
        }
        let first = reads.first().unwrap().at_ns;
        let last = reads.last().unwrap().at_ns;
        let seconds = (last - first) as f64 / 1e9;
        let bytes: usize = reads.iter().map(|r| r.len).sum();
        let gaps = reads.windows(2).map(|w| w[1].at_ns - w[0].at_ns);
        let frames: Vec<&Frame> = a.frames.iter().filter(|f| in_phase(name, f.start_ns)).collect();
        let spans: Vec<&Span> = a.spans.iter().filter(|s| in_phase(name, s.start_ns)).collect();
        let batches: Vec<&Batch> = a
            .batches
            .iter()
            .filter(|b| in_phase(name, b.at_ns) && !b.resized)
            .collect();
        let ticks: Vec<&Change> = a
            .ticks
            .iter()
            .filter(|t| in_phase(name, t.0))
            .map(|t| &t.1)
            .collect();
        let mut seen = Seen::default();
        for r in &reads {
            seen.add(&r.seen);
        }
        let in_frame_gaps: Vec<u64> = frames.iter().filter(|f| f.reads > 1).map(|f| f.max_gap_ns / 1000).collect();
        let changes: Vec<&Change> = batches.iter().map(|b| &b.change).collect();
        phases.push(format!(
            "\"{}\": {{ \"seconds\": {:.3}, \"reads\": {}, \"bytes\": {}, \"reads_per_s\": {:.1}, \"bytes_per_s\": {:.0}, \"read_bytes\": {}, \"read_gaps_us\": {}, \"frames\": {}, \"frames_per_s\": {:.1}, \"frame_reads\": {}, \"frame_bytes\": {}, \"frame_span_us\": {}, \"frame_max_gap_us\": {}, \"frame_gap_before_us\": {}, \"bracketed_frames\": {}, \"sync\": {{ \"set\": {}, \"reset\": {}, \"query\": {}, \"frames\": {}, \"frame_reads\": {}, \"frame_span_us\": {}, \"frame_max_gap_us\": {} }}, \"seen\": {{ \"line_feeds\": {}, \"scroll_ops\": {}, \"margins\": {}, \"cursor\": {}, \"erases\": {}, \"sgr\": {}, \"printed\": {}, \"osc\": {}, \"alt_screen\": {} }}, \"batches\": {}, \"batch_wait_us\": {}, \"batch_edges\": {}, \"batch_dirty\": {}, \"ticks\": {} }}",
            name,
            seconds,
            reads.len(),
            bytes,
            if seconds > 0.0 { reads.len() as f64 / seconds } else { 0.0 },
            if seconds > 0.0 { bytes as f64 / seconds } else { 0.0 },
            dist(reads.iter().map(|r| r.len as u64).collect()),
            gap_buckets(gaps),
            frames.len(),
            if seconds > 0.0 { frames.len() as f64 / seconds } else { 0.0 },
            dist(frames.iter().map(|f| f.reads as u64).collect()),
            dist(frames.iter().map(|f| f.bytes as u64).collect()),
            dist(frames.iter().map(|f| f.span_ns / 1000).collect()),
            dist(in_frame_gaps),
            dist(frames.iter().filter_map(|f| f.gap_before_ns).map(|g| g / 1000).collect()),
            frames.iter().filter(|f| f.bracketed).count(),
            seen.sync_set,
            seen.sync_reset,
            seen.sync_query,
            spans.len(),
            dist(spans.iter().map(|s| s.reads as u64).collect()),
            dist(spans.iter().map(|s| s.span_ns / 1000).collect()),
            dist(spans.iter().map(|s| s.max_gap_ns / 1000).collect()),
            seen.line_feeds,
            seen.scroll_ops,
            seen.margins,
            seen.cursor,
            seen.erases,
            seen.sgr,
            seen.printed,
            seen.osc,
            seen.alt_screen,
            change_totals(&changes),
            dist(batches.iter().map(|b| b.at_ns.saturating_sub(b.first_ns) / 1000).collect()),
            batches.iter().filter(|b| b.edge).count(),
            dist(batches.iter().map(|b| b.dirty as u64).collect()),
            change_totals(&ticks),
        ));
    }
    let sizes: Vec<String> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Size { at_ns, rows, cols } => Some(format!("[{}, {}, {}]", ms(*at_ns), rows, cols)),
            _ => None,
        })
        .collect();
    let summary = format!(
        "{{\n  \"pid\": {}, \"pane\": {}, \"start_unix_ns\": {}, \"cut_short\": {}, \"frame_gap_us\": {}, \"coalesce_us\": {},\n  \"sizes_ms_rows_cols\": [{}],\n  \"phases\": {{\n    {}\n  }}\n}}\n",
        rec.pid,
        rec.pane_id,
        rec.start_unix_ns,
        rec.cut_short,
        gap_ns / 1000,
        COALESCE_NS / 1000,
        sizes.join(", "),
        phases.join(",\n    ")
    );
    (frames, batches, summary)
}

#[test]
fn pty_frames_of_the_recordings_in_a_folder() {
    let folder = match std::env::var("LBTERM_PTY_FRAMES") {
        Ok(f) if !f.is_empty() => std::path::PathBuf::from(f),
        _ => return,
    };
    let gap_ns = std::env::var("LBTERM_PTY_FRAME_GAP_US")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(2_000)
        * 1000;
    let mut found = 0;
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "ptyrec").unwrap_or(false) {
            let rec = lbterm_ptyrec::load(&path).unwrap();
            let marks = marks_for(&folder, &rec);
            let a = analyse(&rec, gap_ns);
            let (frames, batches, summary) = report(&rec, &a, &marks, gap_ns);
            let stem = path.with_extension("");
            let stem = stem.to_string_lossy();
            std::fs::write(format!("{}.frames.tsv", stem), frames).unwrap();
            std::fs::write(format!("{}.batches.tsv", stem), batches).unwrap();
            std::fs::write(format!("{}.summary.json", stem), summary).unwrap();
            found += 1;
        }
    }
    assert!(found > 0, "no .ptyrec in {}", folder.display());
}

// ---- The analysis held to recordings of known frames ----

/// A recording of `reads` (µs from the start, bytes) on a screen of `rows` by `cols`
fn recording(rows: u32, cols: u32, reads: Vec<(u64, Vec<u8>)>) -> Recording {
    let mut events = vec![Event::Size { at_ns: 0, rows, cols }];
    for (us, bytes) in reads {
        events.push(Event::Read { at_ns: us * 1000, bytes });
    }
    Recording {
        start_unix_ns: 1_000_000_000,
        pid: 1,
        pane_id: 1,
        events,
        cut_short: false,
    }
}

/// The alternate screen, ten rows, rows 1 to 8 a conversation and 10 a status line
fn setup() -> Vec<u8> {
    let mut s = b"\x1b[?1049h\x1b[2J".to_vec();
    for r in 1..=8 {
        s.extend_from_slice(format!("\x1b[{};1Hold line {}", r, r).as_bytes());
    }
    s.extend_from_slice(b"\x1b[10;1Hstatus 0");
    s
}

/// The conversation moved up a row in its region, a new line at its foot, the status rewritten
fn scroll_frame(n: usize) -> Vec<u8> {
    format!(
        "\x1b[1;8r\x1b[8;1H\n\x1b[8;1H\x1b[38;2;200;100;50mnew line {}\x1b[0m\x1b[r\x1b[10;1H\x1b[2Kstatus {}",
        n, n
    )
    .into_bytes()
}

/// The spinner's tick: the status line rewritten in place
fn spinner_frame(n: usize) -> Vec<u8> {
    format!("\x1b[10;1H\x1b[2K\x1b[38;5;208m*\x1b[0m working {}", n).into_bytes()
}

/// `bytes` written in `pieces` reads `gap_us` apart from `at_us`
fn pieces(at_us: u64, bytes: &[u8], pieces: usize, gap_us: u64) -> Vec<(u64, Vec<u8>)> {
    let step = (bytes.len() + pieces - 1) / pieces;
    bytes
        .chunks(step)
        .enumerate()
        .map(|(i, c)| (at_us + i as u64 * gap_us, c.to_vec()))
        .collect()
}

#[test]
fn frames_written_in_pieces_are_told_apart_by_their_gaps() {
    let mut reads = vec![(0, setup())];
    for n in 1..=6 {
        let frame = if n % 2 == 1 { scroll_frame(n) } else { spinner_frame(n) };
        reads.extend(pieces(n as u64 * 50_000, &frame, 3, 100));
    }
    let a = analyse(&recording(10, 40, reads), 2_000_000);
    assert_eq!(a.frames.len(), 7, "{:?}", a.frames);
    for f in &a.frames[1..] {
        assert_eq!(f.reads, 3, "{:?}", f);
        assert_eq!(f.span_ns, 200_000, "{:?}", f);
        assert_eq!(f.max_gap_ns, 100_000, "{:?}", f);
        assert!(!f.bracketed);
    }
    assert_eq!(a.frames[2].gap_before_ns, Some(49_800_000));
    // The coalescing gathers each frame's three pieces into one batch, 3 ms after its first
    assert_eq!(a.batches.len(), 7, "{:?}", a.batches);
    for b in &a.batches[1..] {
        assert_eq!(b.reads, 3, "{:?}", b);
        assert_eq!(b.at_ns - b.first_ns, 3_000_000, "{:?}", b);
        assert!(!b.edge);
    }
    assert!(a.spans.is_empty());
}

#[test]
fn a_scroll_shows_as_rows_moved_and_a_spinner_as_a_row_in_place() {
    let mut reads = vec![(0, setup())];
    for n in 1..=4 {
        let frame = if n % 2 == 1 { scroll_frame(n) } else { spinner_frame(n) };
        reads.push((n as u64 * 50_000, frame));
    }
    let a = analyse(&recording(10, 40, reads), 2_000_000);
    let scroll = &a.batches[1].change;
    // Rows 1 to 7 show what rows 2 to 8 did; row 8 is new, as is the status
    assert_eq!(scroll.kind(), "scroll", "{:?}", scroll);
    assert_eq!(
        (scroll.changed, scroll.moved, scroll.new, scroll.shift, scroll.shifted),
        (9, 7, 2, 1, 7),
        "{:?}",
        scroll
    );
    let spin = &a.batches[2].change;
    assert_eq!(spin.kind(), "in place", "{:?}", spin);
    assert_eq!((spin.changed, spin.moved, spin.new), (1, 0, 1), "{:?}", spin);
    // The model marked the region and the status dirty for the scroll, the status for the tick
    assert_eq!(a.batches[1].dirty, 9);
    assert_eq!(a.batches[2].dirty, 1);
    assert!(a.batches[1].alt);
    // A screen shown before, at another row, is a hit for a cache keyed by content
    let again = &a.batches[3].change;
    assert_eq!(again.kind(), "scroll", "{:?}", again);
}

#[test]
fn a_line_shown_again_is_a_hit_by_row_where_it_was_and_by_content_anywhere() {
    // The spinner's row back to a state it showed: a hit for main's key. Then a line written at
    // row 3 that row 1 showed two frames ago: a hit only for a key by content. Written over
    // "old line 3" with no erase, as row 1's was written: with an EL 2 first its shape hash wasn't
    // row 1's, though it read the same (the erase leaves the line's cells otherwise)
    let reads = vec![
        (0, setup()),
        (50_000, spinner_frame(1)),
        (100_000, spinner_frame(2)),
        (150_000, spinner_frame(1)),
        (200_000, b"\x1b[1;1H\x1b[2KY".to_vec()),
        (250_000, b"\x1b[3;1Hold line 1".to_vec()),
    ];
    let a = analyse(&recording(10, 40, reads), 2_000_000);
    let again = &a.batches[3].change;
    assert_eq!((again.changed, again.seen_here, again.new), (1, 1, 0), "{:?}", again);
    let elsewhere = &a.batches[5].change;
    assert_eq!(
        (elsewhere.changed, elsewhere.seen_elsewhere, elsewhere.moved, elsewhere.new),
        (1, 1, 0, 0),
        "{:?}",
        elsewhere
    );
}

#[test]
fn synchronized_frames_are_counted_and_performed_at_their_edges() {
    let mut reads = vec![(0, b"\x1b[?2026$p".to_vec()), (1_000, setup())];
    for n in 1..=5 {
        let mut frame = b"\x1b[?2026h".to_vec();
        frame.extend(spinner_frame(n));
        frame.extend_from_slice(b"\x1b[?2026l");
        // Two pieces 1 ms apart: the hold keeps them one batch, performed at the reset
        reads.extend(pieces(n as u64 * 40_000, &frame, 2, 1_000));
    }
    let a = analyse(&recording(10, 40, reads), 2_000_000);
    let mut seen = Seen::default();
    for r in &a.reads {
        seen.add(&r.seen);
    }
    assert_eq!((seen.sync_set, seen.sync_reset, seen.sync_query), (5, 5, 1));
    assert_eq!(a.spans.len(), 5, "{:?}", a.spans);
    for s in &a.spans {
        assert_eq!((s.reads, s.span_ns, s.max_gap_ns), (2, 1_000_000, 1_000_000), "{:?}", s);
    }
    let bracketed = a.frames.iter().filter(|f| f.bracketed).count();
    assert_eq!(bracketed, 5, "{:?}", a.frames);
    // Two batches a frame, as parse_buffered_data performs them: at the set, what came before
    // and the set itself, which changes nothing; at the reset, the frame, with no coalescing wait
    let at_edges: Vec<&Batch> = a.batches.iter().filter(|b| b.edge).collect();
    assert_eq!(at_edges.len(), 10, "{:?}", a.batches);
    for (n, pair) in at_edges.chunks(2).enumerate() {
        let at = (n as u64 + 1) * 40_000_000;
        assert_eq!((pair[0].at_ns, pair[0].change.kind()), (at, "same"), "{:?}", pair[0]);
        assert_eq!((pair[1].at_ns, pair[1].change.kind()), (at + 1_000_000, "in place"), "{:?}", pair[1]);
        assert_eq!(pair[1].reads, 2, "{:?}", pair[1]);
    }
}

#[test]
fn the_coalescing_sleeps_out_its_deadline_and_takes_what_came() {
    // Reads at 0, 1, 2.5, 4 and 10 ms: the first three in one batch at 3 ms, then one at 7 ms
    // and one at 13 ms
    let reads = vec![
        (0, b"a".to_vec()),
        (1_000, b"b".to_vec()),
        (2_500, b"c".to_vec()),
        (4_000, b"d".to_vec()),
        (10_000, b"e".to_vec()),
    ];
    let a = analyse(&recording(10, 40, reads), 2_000_000);
    let got: Vec<(usize, u64)> = a.batches.iter().map(|b| (b.reads, b.at_ns / 1000)).collect();
    assert_eq!(got, vec![(3, 3_000), (1, 7_000), (1, 13_000)]);
    // Frames by a 2 ms gap: the first four (each under 2 ms after the last), then the fifth
    let frames: Vec<usize> = a.frames.iter().map(|f| f.reads).collect();
    assert_eq!(frames, vec![4, 1]);
}

#[test]
fn a_resize_reaches_the_model_and_its_batch_says_so() {
    let mut rec = recording(10, 40, vec![(0, setup()), (50_000, spinner_frame(1))]);
    rec.events.insert(2, Event::Size { at_ns: 20_000_000, rows: 12, cols: 50 });
    let a = analyse(&rec, 2_000_000);
    assert_eq!(a.batches.len(), 2);
    assert!(a.batches[1].resized);
    assert_eq!(a.batches[1].change.rows, 12);
}

#[test]
fn the_report_has_a_line_a_frame_and_a_batch_and_a_phase_a_mark() {
    let mut reads = vec![(0, setup())];
    for n in 1..=4 {
        reads.push((n as u64 * 50_000, scroll_frame(n)));
    }
    let rec = recording(10, 40, reads);
    let a = analyse(&rec, 2_000_000);
    let marks = vec![(0, "setup".to_string()), (40_000_000, "streaming".to_string())];
    let (frames, batches, summary) = report(&rec, &a, &marks, 2_000_000);
    assert_eq!(frames.lines().count(), 1 + a.frames.len());
    assert_eq!(batches.lines().count(), 1 + a.batches.len());
    assert!(frames.lines().nth(2).unwrap().split('\t').nth(1) == Some("streaming"));
    assert!(summary.contains("\"all\": {"), "{}", summary);
    assert!(summary.contains("\"streaming\": {"), "{}", summary);
    assert!(summary.contains("\"scroll\": 4"), "{}", summary);
    // Each scroll: seven rows moved, which only a key by content keeps; two new
    assert!(summary.contains("\"laid_out_by_row\": 36, \"laid_out_by_content\": 8"), "{}", summary);
}
