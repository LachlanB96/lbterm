//! The context bar's sections (lbterm/CONTEXT.md, "The context bar"): the open tab's Claude Code
//! conversation's whole context window, top to bottom. Each file loaded into it, then Claude
//! Code's own parts (its system prompt, its tools, its skills list), then the conversation, then
//! free space, so how far the bar is filled says how full the context is. Lachlan's ask and
//! answers of 2026-11-08.
//!
//! Made from what the conversation reader keeps ([`crate::context::Context`]: the files, what's in
//! use, the scale, Claude Code's parts) and the window's size from the session's `usage.json`
//! ([`crate::summaries::Usage::window`]), on the GUI thread, only when either changes: a few
//! dozen numbers.
//!
//! What's in use is exact: the last main-chain reply's usage. Every other section is an estimate:
//! its characters over four (Claude Code's own rough count), times the conversation's
//! [`Scale`](crate::context::Scale), but for the tools' JSON, which packs more characters a token
//! and isn't scaled. The conversation is what's in use less all of them.

use crate::context::{Context, InUse, Kind};
use crate::strip::{card_draw, CardDraw};

/// What a section is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// The user's own CLAUDE.md
    User,
    /// A project's CLAUDE.md, from the conversation's folder up
    Project,
    /// A file a loaded file names on an `@` line
    Import,
    /// A `CLAUDE.local.md`
    Local,
    /// One an administrator put in place
    Managed,
    /// The auto memory's index, `MEMORY.md`
    MemoryIndex,
    /// A memory file Claude read
    Memory,
    /// A CLAUDE.md pulled in later from the conversation's own folder or one above it
    PulledIn,
    /// A CLAUDE.md pulled in from another folder: another project's
    Elsewhere,
    /// A file Claude Code attached again after a compaction's summary (lbterm/CONTEXT.md, "The
    /// buttons"): beside its own section when it's loaded again too
    Attached,
    /// Claude Code's system prompt
    System,
    /// Claude Code's tools
    Tools,
    /// Claude Code's skills list
    Skills,
    /// Everything else in use: the messages, tool calls and results, and Claude Code's other lists
    Conversation,
    /// The window less what's in use
    Free,
}

/// The strip's turn-end grey (lbterm_extras::conversation's `Kind::TurnEnd`)
const GREY: u32 = 0x8A8A9E;
/// The rail's idle yellow (lbterm_panel::palette's `YELLOW`)
const YELLOW: u32 = 0xF2E86D;
/// The working reply gone quiet's amber, as the context list drew another project's file
/// (lbterm_panel::palette::AMBER)
const AMBER: u32 = 0xFFBF00;
/// The rail's outline grey (lbterm_panel::palette::OUTLINE): the part nobody can change
const CLAUDE_CODE: u32 = 0x5E5E6E;
/// The strip's edit mark's blue-grey
const CONVERSATION: u32 = 0x7A9CC6;
/// The rail's track (lbterm_panel::palette::TRACK)
const FREE: u32 = 0x2A2A36;

impl What {
    /// The pane's word for its kind
    pub fn word(self) -> &'static str {
        match self {
            What::User => "user",
            What::Project => "project",
            What::Import => "import",
            What::Local => "local",
            What::Managed => "managed",
            What::MemoryIndex => "memory index",
            What::Memory => "memory",
            What::PulledIn | What::Elsewhere => "pulled in",
            What::Attached => "attached",
            What::System => "system prompt",
            What::Tools => "tools",
            What::Skills => "skills",
            What::Conversation => "conversation",
            What::Free => "free",
        }
    }

    /// Its colour on the bar, from the palette the strips and the rail use
    pub fn colour(self) -> u32 {
        match self {
            What::User | What::Project | What::Import | What::Local | What::Managed | What::PulledIn | What::Attached => GREY,
            What::MemoryIndex | What::Memory => YELLOW,
            What::Elsewhere => AMBER,
            What::System | What::Tools | What::Skills => CLAUDE_CODE,
            What::Conversation => CONVERSATION,
            What::Free => FREE,
        }
    }
}

/// What a section's Remove button takes out: a line, or an import's `@`, never a file (Lachlan's
/// 3a of 2026-11-08 and his answer to Q2 of 2026-11-09)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// A memory file's line in the memory index
    MemoryLine { index: String, file: String },
    /// The `@` of each import naming `file` in the file that imports it, the line's words and the
    /// path staying as plain text
    Import { importer: String, file: String },
}

impl Removal {
    /// The file it stops loading: the memory file, or the file imported
    pub fn file(&self) -> &str {
        match self {
            Removal::MemoryLine { file, .. } | Removal::Import { file, .. } => file,
        }
    }

    /// The kind of section it was made for: a memory file Claude read, or an import
    pub fn what(&self) -> What {
        match self {
            Removal::MemoryLine { .. } => What::Memory,
            Removal::Import { .. } => What::Import,
        }
    }
}

/// One section of the bar
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub what: What,
    /// A file's path as Claude Code wrote it; none for Claude Code's parts, the conversation, free
    pub path: Option<String>,
    pub tokens: u64,
    pub removal: Option<Removal>,
    /// The pane's second line for it
    pub note: Option<&'static str>,
}

/// A pulled-in file's note: it has no button
pub const LEAVES: &str = "Leaves at the next compaction";

/// A file attached again after a compaction's summary: Claude Code attaches it again at every
/// compaction while it's among the five files it read with the newest changes, so no more is said
pub const ATTACHED: &str = "Attached again after the summary";

/// The bar, top to bottom
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// Every section in the bar's order, free last
    pub sections: Vec<Section>,
    /// What's in use: exact, or after a compaction an estimate until the next reply
    pub in_use: u64,
    /// The window's size, at least what's in use
    pub window: u64,
    /// Whether what's in use is an estimate
    pub estimate: bool,
}

/// What the bar waits for before it can say anything: it draws only its track meanwhile
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waiting {
    /// No reply yet: nothing in use, and no scale
    FirstReply,
    /// The session's `usage.json` hasn't said the window's size
    Window,
    /// Replies have come but the transcript has no first request to scale by: a Claude Code that
    /// doesn't record its requests, or a transcript that doesn't start at the conversation's start
    NoRequests,
}

impl Waiting {
    /// The pane's words for it
    pub fn words(self) -> &'static str {
        match self {
            Waiting::FirstReply => "Waiting for Claude's first reply",
            Waiting::Window => "Waiting for the context window's size",
            Waiting::NoRequests => "This transcript doesn't record Claude Code's first request",
        }
    }
}

/// The bar's sections for `context`, in a window of `window` tokens
pub fn sections(context: &Context, window: Option<u64>) -> Result<Bar, Waiting> {
    let scale = match (context.in_use, context.scale) {
        (InUse::Unknown, _) => return Err(Waiting::FirstReply),
        (InUse::Known(_), None) => return Err(Waiting::NoRequests),
        (InUse::Compacted { .. }, None) => return Err(Waiting::FirstReply),
        (_, Some(scale)) => scale,
    };
    let window = window.filter(|w| *w > 0).ok_or(Waiting::Window)?;
    let index = context.files.iter().find(|f| f.kind == Kind::Memory).map(|f| f.path.clone());
    let mut sections: Vec<Section> = context
        .files
        .iter()
        .map(|f| {
            let what = match f.kind {
                Kind::Memory => What::MemoryIndex,
                Kind::Remembered => What::Memory,
                Kind::Later if f.elsewhere => What::Elsewhere,
                Kind::Later => What::PulledIn,
                Kind::Attached => What::Attached,
                _ if f.imported_by.is_some() => What::Import,
                Kind::User => What::User,
                Kind::Project => What::Project,
                Kind::Local => What::Local,
                Kind::Managed => What::Managed,
            };
            let removal = match (what, &f.imported_by, &index) {
                (What::Memory, _, Some(index)) => Some(Removal::MemoryLine { index: index.clone(), file: f.path.clone() }),
                (What::Import, Some(by), _) => Some(Removal::Import { importer: by.clone(), file: f.path.clone() }),
                _ => None,
            };
            let note = match what {
                What::PulledIn | What::Elsewhere => Some(LEAVES),
                What::Attached => Some(ATTACHED),
                _ => None,
            };
            Section { what, path: Some(f.path.clone()), tokens: scale.apply(f.tokens), removal, note }
        })
        .collect();
    let parts = context.parts.unwrap_or_default();
    let own = |what, tokens| Section { what, path: None, tokens, removal: None, note: None };
    sections.push(own(What::System, scale.apply(parts.system)));
    sections.push(own(What::Tools, parts.tools as u64));
    if parts.skills > 0 {
        sections.push(own(What::Skills, scale.apply(parts.skills)));
    }
    let known: u64 = sections.iter().map(|s| s.tokens).sum();
    let (in_use, conversation, estimate) = match context.in_use {
        InUse::Known(n) => {
            if known > n {
                // The estimates come to more than what's in use, the one exact number: they're
                // drawn as shares of it, and the conversation as nothing
                for s in &mut sections {
                    s.tokens = (s.tokens as u128 * n as u128 / known as u128) as u64;
                }
            }
            let left = n - sections.iter().map(|s| s.tokens).sum::<u64>();
            (n, left, false)
        }
        InUse::Compacted { post } => (known + post, post, true),
        InUse::Unknown => unreachable!("waited for above"),
    };
    sections.push(own(What::Conversation, conversation));
    let window = window.max(in_use);
    sections.push(own(What::Free, window - in_use));
    Ok(Bar { sections, in_use, window, estimate })
}

impl What {
    /// Its name in the state file (lbterm/STATE.md, `context_bar`): the pane's word, but another
    /// project's pulled-in file apart from one of the conversation's own
    pub fn name(self) -> &'static str {
        match self {
            What::User => "user",
            What::Project => "project",
            What::Import => "import",
            What::Local => "local",
            What::Managed => "managed",
            What::MemoryIndex => "memory_index",
            What::Memory => "memory",
            What::PulledIn => "pulled_in",
            What::Elsewhere => "elsewhere",
            What::Attached => "attached",
            What::System => "system",
            What::Tools => "tools",
            What::Skills => "skills",
            What::Conversation => "conversation",
            What::Free => "free",
        }
    }
}

/// What the bar draws at rest of one section, or of neighbouring sections of the same colour
/// drawn as one when every section's least height doesn't fit (lbterm/CONTEXT.md, "Small
/// sections"). Free space has none: it's the track the runs are drawn over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// The sections it draws: `Bar::sections[first..=last]`
    pub first: usize,
    pub last: usize,
    /// Its top, down from the track's, and its height, in pixels
    pub y: u32,
    pub h: u32,
    /// How much of it from its top is its colour; the rest, the gap, is the track's
    pub coloured: u32,
    pub colour: u32,
}

/// Where the filled part of a track `height` pixels tall ends, down from its top: what's in use
/// of the window, to the nearest pixel
pub fn filled(bar: &Bar, height: u32) -> u32 {
    if bar.window == 0 {
        return 0;
    }
    let px = (bar.in_use.min(bar.window) as u128 * height as u128 + bar.window as u128 / 2) / bar.window as u128;
    px as u32
}

/// The bar at rest in a track `height` pixels tall, top to bottom: a run for each section in use
/// (each with tokens; free has none), together exactly [`filled`] tall, so the line where the
/// filled part ends is where what's in use puts it, to the pixel. Each but the biggest is its
/// share to the nearest pixel and at least `floor` tall (`config::lbterm::CONTEXT_SECTION_MIN`,
/// scaled), its last `gap` pixels the track's so neighbours read apart; the biggest (the
/// conversation, nearly always) is what's left, so the floors' pixels and the rounding's come
/// off it alone. When the floors don't fit in the filled part, the smallest run with a
/// neighbour of the same colour joins it, again until they do or none has one; then the floor is
/// what the filled part has for each, without the gap, and under a pixel each the runs are their
/// shares, some none. Made only when the sections or the track change, never on a frame of its
/// own.
pub fn runs(bar: &Bar, height: u32, floor: u32, gap: u32) -> Vec<Run> {
    let fill = filled(bar, height) as u64;
    // (first, last, tokens, colour) of each run, top first
    let mut items: Vec<(usize, usize, u64, u32)> = bar
        .sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.what != What::Free && s.tokens > 0)
        .map(|(i, s)| (i, i, s.tokens, s.what.colour()))
        .collect();
    if fill == 0 || items.is_empty() {
        return vec![];
    }
    let fits = |n: usize, floor: u32| n as u64 * floor as u64 <= fill;
    while !fits(items.len(), floor) {
        let joins = |i: usize| -> Option<usize> {
            let before = (i > 0 && items[i - 1].3 == items[i].3).then(|| i - 1);
            let after = (i + 1 < items.len() && items[i + 1].3 == items[i].3).then(|| i + 1);
            match (before, after) {
                (Some(b), Some(a)) => Some(if items[a].2 < items[b].2 { a } else { b }),
                (b, a) => b.or(a),
            }
        };
        let Some((i, j)) = (0..items.len())
            .filter_map(|i| joins(i).map(|j| (i, j)))
            .min_by_key(|&(i, _)| (items[i].2, i))
        else {
            break;
        };
        let (a, b) = (i.min(j), i.max(j));
        items[a] = (items[a].0, items[b].1, items[a].2 + items[b].2, items[a].3);
        items.remove(b);
    }
    let (floor, gap) = if fits(items.len(), floor) {
        (floor as u64, gap as u64)
    } else {
        (fill / items.len() as u64, 0)
    };
    let total = items.iter().map(|i| i.2).sum::<u64>() as u128;
    // Each run its share of the filled part to the nearest pixel, at least the floor, but the
    // biggest, which is what's left: the floors' pixels and the rounding's come off it alone
    let mut h: Vec<u64> = items
        .iter()
        .map(|i| (((i.2 as u128 * fill as u128 * 2 + total) / (2 * total)) as u64).max(floor))
        .collect();
    let big = (0..items.len()).max_by_key(|&i| (items[i].2, std::cmp::Reverse(i))).expect("runs");
    let others: u64 = h.iter().enumerate().filter(|&(i, _)| i != big).map(|(_, h)| h).sum();
    h[big] = fill.saturating_sub(others).max(floor);
    let mut sum = others + h[big];
    // When even that leaves it under its floor, the next biggest give a pixel at a time
    while sum > fill {
        let i = (0..h.len())
            .filter(|&i| h[i] > floor)
            .max_by_key(|&i| (h[i], std::cmp::Reverse(i)))
            .expect("the floors fit, so a run is over its floor");
        h[i] -= 1;
        sum -= 1;
    }
    let mut y = 0;
    items
        .iter()
        .zip(h)
        .map(|(&(first, last, _, colour), h)| {
            let coloured = if gap > 0 && h >= floor && h > gap { h - gap } else { h };
            let run = Run { first, last, y: y as u32, h: h as u32, coloured: coloured as u32, colour };
            y += h;
            run
        })
        .filter(|r| r.h > 0)
        .collect()
}

/// One of the widened bar's labels: a section's tokens beside it (lbterm/CONTEXT.md, "At rest and
/// widened")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Its section, in `Bar::sections`
    pub section: usize,
    pub text: String,
    /// Its line's top, down from the track's, in pixels
    pub y: u32,
    /// Where its section's middle is, down from the track's: a label that isn't level with it
    /// (pushed apart from its neighbours) has a hairline to it
    pub to: u32,
}

impl Label {
    /// Whether it was pushed away from its section, so a hairline joins them
    pub fn moved(&self, line: u32) -> bool {
        self.to.abs_diff(self.y + line / 2) > line / 4
    }
}

/// The widened bar's labels in a track `height` pixels tall, `line` pixels a line, from the runs
/// [`runs`] laid out for it: each section with a run gets its tokens (`context::count`) level with
/// the middle of its run, the sections of a joined run share its middle, and free space its own,
/// the middle of the track below the filled part. Each is pushed down from the one above it so
/// none overlap, and back up from the track's foot when they run off it. When they can't all fit,
/// the smallest sections' go first (the pane names every one). Made with the runs, never on a
/// frame of its own.
pub fn labels(bar: &Bar, runs: &[Run], height: u32, line: u32) -> Vec<Label> {
    if line == 0 || height < line {
        return vec![];
    }
    let fill = filled(bar, height);
    let mut wanted: Vec<(usize, u32)> = vec![];
    for r in runs {
        for i in r.first..=r.last {
            if bar.sections.get(i).map_or(false, |s| s.tokens > 0) {
                wanted.push((i, r.y + r.h / 2));
            }
        }
    }
    if let Some(free) = bar.sections.iter().position(|s| s.what == What::Free && s.tokens > 0) {
        if height > fill {
            wanted.push((free, fill + (height - fill) / 2));
        }
    }
    place(bar, wanted, height, line)
}

/// Labels for `wanted` (each a section and where its middle is, down from the track's top, in
/// the bar's order) in a track `height` pixels tall, `line` a line: each level with its middle,
/// pushed down from the one above so none overlap and back up from the track's foot; when they
/// can't all fit, the smallest sections' go first
fn place(bar: &Bar, mut wanted: Vec<(usize, u32)>, height: u32, line: u32) -> Vec<Label> {
    if line == 0 || height < line {
        return vec![];
    }
    // The ones that fit, the biggest first, back in the bar's order
    let room = (height / line) as usize;
    if wanted.len() > room {
        let mut by_size: Vec<usize> = (0..wanted.len()).collect();
        by_size.sort_by_key(|&k| std::cmp::Reverse(bar.sections[wanted[k].0].tokens));
        let mut keep: Vec<usize> = by_size.into_iter().take(room).collect();
        keep.sort_unstable();
        wanted = keep.into_iter().map(|k| wanted[k]).collect();
    }
    let top = |to: u32| to.saturating_sub(line / 2).min(height - line);
    let mut ys: Vec<u32> = Vec::with_capacity(wanted.len());
    for &(_, to) in &wanted {
        let y = match ys.last() {
            Some(&above) => top(to).max(above + line),
            None => top(to),
        };
        ys.push(y);
    }
    // Back up from the foot
    let mut limit = height - line;
    for y in ys.iter_mut().rev() {
        if *y > limit {
            *y = limit;
        }
        limit = y.saturating_sub(line);
    }
    wanted
        .into_iter()
        .zip(ys)
        .map(|((section, to), y)| Label { section, text: crate::context::count(bar.sections[section].tokens as usize), y, to })
        .collect()
}

/// The widened bar's numbers (lbterm/CONTEXT.md, "At rest and widened"; Lachlan's 4b of
/// 2026-11-09), from the runs [`runs`] laid out for the widened bar's track with a line and its
/// gap as their floor, so most sections have a line of their own. `on`: each section alone in a
/// run whose colour is at least `line` tall gets its tokens on itself, its line's top centred in
/// that colour, and free space its own in the middle of the track below the filled part when
/// that's a line tall. `thin`: the rest, a section sharing a run with neighbours of its colour or
/// in a run under a line (the floors didn't fit the filled part), and free space with under a
/// line of room, placed as [`labels`] places them, level with their runs and pushed apart, for
/// the pane beside the bar as wide as it. Made with the runs, never on a frame of its own.
pub fn numbers(bar: &Bar, runs: &[Run], height: u32, line: u32) -> Numbers {
    if line == 0 || height < line {
        return Numbers::default();
    }
    let fill = filled(bar, height);
    let count = |i: usize| crate::context::count(bar.sections[i].tokens as usize);
    let mut on = vec![];
    let mut thin: Vec<(usize, u32)> = vec![];
    for r in runs {
        if r.first == r.last && r.coloured >= line {
            let mid = r.y + r.coloured / 2;
            on.push(Label { section: r.first, text: count(r.first), y: r.y + (r.coloured - line) / 2, to: mid });
            continue;
        }
        for i in r.first..=r.last {
            if bar.sections.get(i).map_or(false, |s| s.tokens > 0) {
                thin.push((i, r.y + r.h / 2));
            }
        }
    }
    if let Some(free) = bar.sections.iter().position(|s| s.what == What::Free && s.tokens > 0) {
        if height >= fill + line {
            let mid = fill + (height - fill) / 2;
            on.push(Label { section: free, text: count(free), y: mid - line / 2, to: mid });
        } else if height > fill {
            thin.push((free, fill + (height - fill) / 2));
        }
    }
    Numbers { on, thin: place(bar, thin, height, line) }
}

/// The widened bar's numbers: those on their sections, and the thin sections' for the pane beside
/// it ([`numbers`])
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Numbers {
    pub on: Vec<Label>,
    pub thin: Vec<Label>,
}

/// The pane's heading (lbterm/CONTEXT.md, "The pane"): what's in use of the window and its share,
/// `≈` while it's an estimate after a compaction
pub fn heading(bar: &Bar) -> String {
    let share = if bar.window > 0 { bar.in_use as f64 * 100.0 / bar.window as f64 } else { 0.0 };
    let share = if share < 10.0 { format!("{share:.1}") } else { format!("{share:.0}") };
    format!(
        "{}{} of {} in use · {}%",
        if bar.estimate { "≈" } else { "" },
        crate::context::count(bar.in_use as usize),
        crate::context::count(bar.window as usize),
        share.trim_end_matches(".0")
    )
}

/// A section's tokens as the pane's row says them: `≈` on every estimate, which is every section
/// but free space while what's in use is exact
pub fn row_tokens(bar: &Bar, section: &Section) -> String {
    let exact = section.what == What::Free && !bar.estimate;
    format!("{}{}", if exact { "" } else { "≈" }, crate::context::count(section.tokens as usize))
}

/// The first of the pane's rows shown when the wheel has asked for `scroll` (lbterm/CONTEXT.md,
/// "The pane"): the rows' `heights` in the bar's order, `room` the height they have between the
/// heading and Compact now, and `line` the one line a count of the rows scrolled out above takes
/// once any are. 0 while every row fits; else no further than the first that leaves the rest
/// filling the room under that line, so the wheel can't scroll the last row off the foot and a
/// wheel turned on past the end keeps nothing in hand for the way back
pub fn first_shown(heights: &[f64], room: f64, line: f64, scroll: usize) -> usize {
    if scroll == 0 || heights.is_empty() || heights.iter().sum::<f64>() <= room {
        return 0;
    }
    let mut used = line;
    let mut first = heights.len();
    while first > 0 && used + heights[first - 1] <= room {
        used += heights[first - 1];
        first -= 1;
    }
    scroll.min(first.min(heights.len() - 1))
}

/// The widened bar and its thin numbers' pane this frame (wezterm-gui's render/context_bar.rs;
/// lbterm/FLOWS.md, "The context bar's widening and its pane's opening"), from whether anything
/// holds them out (`held`: the bar's own rest, the pane open, or the pane still sliding back) and
/// what the compositor said still shows of each (`bar`, `thin`: out, or on its way back, a wait
/// before its slide included). Going back is one commit (since round 3, 2026-11-10): the frame
/// `held` ends starts the thin numbers' pane's slide back and the bar's, which waits for it
/// ([`slide_wait`]), so no wake, frame or commit comes between the two slides, and the bar is
/// drawn from its last frame from then until its slide has ended, whatever a layout would say
/// meanwhile. Before, the bar waited for the compositor to say the thin pane was in, and a wake
/// and a frame of its own started its slide 25 to 40 ms after
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Out {
    /// The widened bar's card
    pub bar: CardDraw,
    /// The thin numbers' pane
    pub thin: CardDraw,
    /// The bar's slide goes out (else back, or it stays in)
    pub bar_open: bool,
    /// The card takes the pointer: it's out on screen and its slide back hasn't begun, so a
    /// pointer back on it while the thin pane goes in opens them again
    pub takes_pointer: bool,
    /// The scroll strip rides the card's left edge; else it goes home with the thin pane
    pub ride_on_card: bool,
    /// Laid out again if what it's laid out from changed: only a card drawn fresh needs it
    pub lay_out: bool,
}

/// See [`Out`]
pub fn out(held: bool, bar: bool, thin: bool) -> Out {
    Out {
        bar: card_draw(held, bar),
        thin: card_draw(held, thin),
        bar_open: held,
        takes_pointer: held || thin,
        ride_on_card: held,
        lay_out: held,
    }
}

/// How long one of the bar's slides waits before it starts, in seconds. Coming out nothing waits
/// (since round 4's stage 6, Lachlan's E9 of 2026-11-10): the thin numbers' pane comes out from
/// under the bar's moving left edge in the bar's own slide, its clip in a visual DWM moves on the
/// bar's own animation (compositor.rs, `Slid::follow`), and the strip riding its left edge goes
/// with it, all started in one commit and settled together; until then the thin pane and the
/// strip waited for the bar's slide to be done and slid out after it. Going back from out, the bar
/// (`after_thin`) waits for what's left of the thin numbers' pane's slide back (`thin_left`),
/// started in the same commit, so the strip glides home under the widened bar before it narrows
/// (his 3a of 2026-11-10). Turned round while it's still coming out (`bar_moving`), the bar goes
/// back at once with the thin pane, each from where it is with its speed, one motion back as it
/// came: held for the thin pane there, it would stop dead where it was
pub fn slide_wait(open: bool, after_thin: bool, bar_moving: bool, thin_left: f64) -> f64 {
    if !open && after_thin && !bar_moving {
        thin_left
    } else {
        0.0
    }
}

/// The scroll strip is drawn in a visual of its own, for DWM to glide, while it rides away from
/// its own place (`offset`), while its glide is under way (`gliding`), and on the frame it starts
/// home from where it rode (`was_away`: away the frame before), so it glides home with the thin
/// pane going in rather than going at once from where it was to under the widened bar
pub fn ride_own(moves: bool, offset: f64, gliding: bool, was_away: bool) -> bool {
    moves && (offset != 0.0 || gliding || was_away)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::context::{File, Parts, Scale};

    /// The widened bar goes back in the commit that starts the thin pane's slide back, waiting
    /// for it there, and shows its last frame from then to its slide's end; the strip goes home
    /// with the thin pane, and the card takes the pointer until its own slide begins
    #[test]
    fn the_bar_goes_back_with_the_thin_pane() {
        // Out, held by the pointer's rest
        let o = out(true, true, true);
        assert_eq!((o.bar, o.thin, o.bar_open, o.takes_pointer, o.ride_on_card, o.lay_out), (CardDraw::Fresh, CardDraw::Fresh, true, true, true, true));
        // The frame the rest ends: both go back at once, drawn as they were, nothing laid out
        let o = out(false, true, true);
        assert_eq!((o.bar, o.thin, o.bar_open), (CardDraw::Last, CardDraw::Last, false));
        assert!(o.takes_pointer, "the card is still out while the thin pane goes in");
        assert!(!o.ride_on_card && !o.lay_out);
        // The thin pane in, the bar on its way: still drawn as it was, whatever a layout would say
        let o = out(false, true, false);
        assert_eq!((o.bar, o.thin, o.takes_pointer), (CardDraw::Last, CardDraw::None, false));
        // In
        let o = out(false, false, false);
        assert_eq!((o.bar, o.thin, o.takes_pointer), (CardDraw::None, CardDraw::None, false));
        // The bar waits for the thin pane coming back from out, and nothing waits going out: the
        // thin pane comes out from under the bar's moving edge in the bar's own slide (E9)
        assert_eq!(slide_wait(false, true, false, 0.15), 0.15);
        assert_eq!(slide_wait(true, true, false, 0.15), 0.0, "going out, the bar waits for nothing");
        assert_eq!(slide_wait(true, false, false, 0.0), 0.0, "going out, the thin pane waits for nothing");
        assert_eq!(slide_wait(false, false, false, 0.15), 0.0, "coming back, the thin pane waits for nothing");
        // Turned round while it's still coming out, the bar goes back at once with the thin pane,
        // from where it is with its speed, rather than stopping dead for it
        assert_eq!(slide_wait(false, true, true, 0.15), 0.0);
        // With no thin pane there's nothing to wait for
        assert_eq!(slide_wait(false, true, false, 0.0), 0.0);
    }

    /// The strip glides home from where it rode: its own visual on the frame it starts home
    /// (offset 0, not yet gliding), through the glide, and put away once home
    #[test]
    fn the_strip_glides_home() {
        assert!(ride_own(true, -121.0, false, false), "riding away");
        assert!(ride_own(true, 0.0, false, true), "the frame it starts home");
        assert!(ride_own(true, 0.0, true, false), "on its way");
        assert!(!ride_own(true, 0.0, false, false), "home");
        assert!(!ride_own(false, -121.0, false, true), "without motion it's never in a visual of its own");
    }

    fn file(path: &str, kind: Kind, tokens: usize) -> File {
        File { path: path.into(), kind, tokens, elsewhere: false, imported_by: None }
    }

    /// A conversation in a project with a user CLAUDE.md importing a rule, a project CLAUDE.md,
    /// its memory index, a memory file read, and two files pulled in, one from another project
    fn context() -> Context {
        let mut asking = file("C:/Users/me/Work/rules/asking.md", Kind::User, 1000);
        asking.imported_by = Some("C:/Users/me/.claude/CLAUDE.md".into());
        let mut ledger = file("C:/Users/me/Work/Ledger/CLAUDE.md", Kind::Later, 5000);
        ledger.elsewhere = true;
        Context {
            files: vec![
                file("C:/Users/me/.claude/CLAUDE.md", Kind::User, 6000),
                asking,
                file("C:/Users/me/Work/Harbor/CLAUDE.md", Kind::Project, 4000),
                file("C:/Users/me/.claude/projects/H/memory/MEMORY.md", Kind::Memory, 2000),
                file("C:/Users/me/.claude/projects/H/memory/feedback_x.md", Kind::Remembered, 300),
                file("C:/Users/me/Work/Harbor/code/CLAUDE.md", Kind::Later, 200),
                ledger,
            ],
            own: Some("C:/Users/me/Work/Harbor".into()),
            in_use: InUse::Known(100_000),
            // Lachlan's files counted about 1.5 times their characters over four
            scale: Some(Scale { real: 3, estimated: 2 }),
            parts: Some(Parts { system: 2000, tools: 10_000, skills: 4000 }),
            compactions: 0,
            after_summary: false,
            replies: 0,
        }
    }

    #[test]
    fn the_bar_waits_for_the_first_reply_and_the_windows_size_and_says_why() {
        let mut c = context();
        c.in_use = InUse::Unknown;
        assert_eq!(sections(&c, Some(1_000_000)), Err(Waiting::FirstReply));
        let mut c = context();
        c.scale = None;
        assert_eq!(sections(&c, Some(1_000_000)), Err(Waiting::NoRequests), "replies but no first request");
        assert_eq!(sections(&context(), None), Err(Waiting::Window));
        assert_eq!(sections(&context(), Some(0)), Err(Waiting::Window));
        assert!(Waiting::Window.words().contains("window"));
    }

    #[test]
    fn each_file_is_its_own_section_scaled_then_claude_codes_parts_the_conversation_and_free() {
        let bar = sections(&context(), Some(1_000_000)).unwrap();
        let got: Vec<(What, u64)> = bar.sections.iter().map(|s| (s.what, s.tokens)).collect();
        assert_eq!(
            got,
            vec![
                (What::User, 9000),
                (What::Import, 1500),
                (What::Project, 6000),
                (What::MemoryIndex, 3000),
                (What::Memory, 450),
                (What::PulledIn, 300),
                (What::Elsewhere, 7500),
                (What::System, 3000),
                (What::Tools, 10_000),
                (What::Skills, 6000),
                (What::Conversation, 100_000 - 46_750),
                (What::Free, 900_000),
            ],
            "the tools aren't scaled; the conversation is what's in use less the rest"
        );
        assert_eq!((bar.in_use, bar.window, bar.estimate), (100_000, 1_000_000, false));
        assert_eq!(bar.sections.iter().map(|s| s.tokens).sum::<u64>(), bar.window, "the bar is the whole window");
    }

    #[test]
    fn colours_say_memory_yellow_claude_md_grey_and_another_projects_amber() {
        let bar = sections(&context(), Some(1_000_000)).unwrap();
        let colour = |w: What| bar.sections.iter().find(|s| s.what == w).unwrap().what.colour();
        assert_eq!(colour(What::MemoryIndex), 0xF2E86D);
        assert_eq!(colour(What::Memory), 0xF2E86D);
        assert_eq!(colour(What::User), colour(What::Project));
        assert_eq!(colour(What::PulledIn), colour(What::Project), "pulled in from its own folder up");
        assert_eq!(colour(What::Elsewhere), 0xFFBF00);
        assert_ne!(colour(What::System), colour(What::Conversation));
        assert_eq!(What::Elsewhere.word(), "pulled in");
    }

    #[test]
    fn only_a_memory_file_and_an_import_have_a_remove_button_and_pulled_in_files_say_when_they_go() {
        let bar = sections(&context(), Some(1_000_000)).unwrap();
        let by = |w: What| bar.sections.iter().find(|s| s.what == w).unwrap();
        assert_eq!(
            by(What::Memory).removal,
            Some(Removal::MemoryLine {
                index: "C:/Users/me/.claude/projects/H/memory/MEMORY.md".into(),
                file: "C:/Users/me/.claude/projects/H/memory/feedback_x.md".into()
            })
        );
        assert_eq!(
            by(What::Import).removal,
            Some(Removal::Import {
                importer: "C:/Users/me/.claude/CLAUDE.md".into(),
                file: "C:/Users/me/Work/rules/asking.md".into()
            })
        );
        let with: Vec<What> = bar.sections.iter().filter(|s| s.removal.is_some()).map(|s| s.what).collect();
        assert_eq!(with, vec![What::Import, What::Memory], "the project's own CLAUDE.md, the index and the rest have none");
        let noted: Vec<What> = bar.sections.iter().filter(|s| s.note == Some(LEAVES)).map(|s| s.what).collect();
        assert_eq!(noted, vec![What::PulledIn, What::Elsewhere]);
        // A file attached again after a compaction's summary: its own grey section, saying so,
        // with no removal of its own (its file's, when one was pressed, is the pane's to find)
        let mut c = context();
        c.files.push(file("C:/Users/me/Work/rules/check-rule.md", Kind::Attached, 400));
        let bar = sections(&c, Some(1_000_000)).unwrap();
        let attached = bar.sections.iter().find(|s| s.what == What::Attached).unwrap();
        assert_eq!((attached.note, attached.removal.as_ref(), What::Attached.word(), What::Attached.colour()), (Some(ATTACHED), None, "attached", GREY));
        assert_eq!(What::Attached.name(), "attached");
        // A memory file with no index to take its line off has no button
        let mut c = context();
        c.files.retain(|f| f.kind != Kind::Memory);
        let bar = sections(&c, Some(1_000_000)).unwrap();
        assert!(bar.sections.iter().find(|s| s.what == What::Memory).unwrap().removal.is_none());
    }

    #[test]
    fn after_a_compaction_whats_in_use_is_an_estimate_until_the_next_reply() {
        let mut c = context();
        c.files.retain(|f| f.kind != Kind::Later && f.kind != Kind::Remembered);
        c.in_use = InUse::Compacted { post: 20_000 };
        let bar = sections(&c, Some(1_000_000)).unwrap();
        let known: u64 = bar.sections.iter().filter(|s| !matches!(s.what, What::Conversation | What::Free)).map(|s| s.tokens).sum();
        assert_eq!(bar.sections.iter().find(|s| s.what == What::Conversation).unwrap().tokens, 20_000);
        assert_eq!((bar.in_use, bar.estimate), (known + 20_000, true));
    }

    #[test]
    fn estimates_over_whats_in_use_are_shares_of_it_and_the_conversation_nothing() {
        let mut c = context();
        c.in_use = InUse::Known(23_375);
        let bar = sections(&c, Some(200_000)).unwrap();
        let conversation = bar.sections.iter().find(|s| s.what == What::Conversation).unwrap().tokens;
        let filled: u64 = bar.sections.iter().filter(|s| s.what != What::Free).map(|s| s.tokens).sum();
        assert_eq!(filled, 23_375, "what's in use is exact, and fills the bar to where it says");
        assert!(conversation <= 7, "only the rounding: {conversation}");
        assert_eq!(bar.sections[0].tokens, 4500, "each estimate halved with the rest");
    }

    #[test]
    fn a_window_smaller_than_whats_in_use_is_full() {
        let bar = sections(&context(), Some(50_000)).unwrap();
        assert_eq!((bar.window, bar.sections.last().unwrap().tokens), (100_000, 0));
    }

    /// A bar of these sections (what, tokens), free the window's rest
    fn bar_of(parts: &[(What, u64)], window: u64) -> Bar {
        let mut sections: Vec<Section> = parts
            .iter()
            .map(|&(what, tokens)| Section { what, path: None, tokens, removal: None, note: None })
            .collect();
        let in_use: u64 = parts.iter().map(|p| p.1).sum();
        sections.push(Section { what: What::Free, path: None, tokens: window - in_use, removal: None, note: None });
        Bar { sections, in_use, window, estimate: false }
    }

    /// Each run's (first, last, height): the sections it draws and how tall
    fn heights(runs: &[Run]) -> Vec<(usize, usize, u32)> {
        runs.iter().map(|r| (r.first, r.last, r.h)).collect()
    }

    /// The runs lie one under the next from the track's top, and end where the filled part does
    fn assert_stacked(bar: &Bar, runs: &[Run], height: u32) {
        let mut y = 0;
        for r in runs {
            assert_eq!(r.y, y, "{runs:?}");
            y += r.h;
        }
        assert_eq!(y, filled(bar, height), "the runs fill to where what's in use puts it: {runs:?}");
    }

    #[test]
    fn the_filled_part_ends_where_whats_in_use_puts_it_to_the_pixel() {
        let bar = bar_of(&[(What::User, 30_000), (What::Conversation, 282_000)], 1_000_000);
        assert_eq!(filled(&bar, 1700), 530, "312k of 1M of 1,700 px: 530.4");
        assert_eq!(filled(&bar, 1000), 312);
        let full = bar_of(&[(What::Conversation, 200_000)], 200_000);
        assert_eq!(filled(&full, 999), 999);
        let nothing = Bar { sections: vec![], in_use: 0, window: 0, estimate: false };
        assert_eq!(filled(&nothing, 500), 0);
    }

    #[test]
    fn big_sections_are_their_share_and_neighbours_read_apart_by_a_gap() {
        let bar = bar_of(&[(What::User, 100_000), (What::Project, 50_000), (What::Conversation, 250_000)], 1_000_000);
        let runs = runs(&bar, 1000, 3, 1);
        assert_eq!(heights(&runs), vec![(0, 0, 100), (1, 1, 50), (2, 2, 250)]);
        assert_stacked(&bar, &runs, 1000);
        assert!(runs.iter().all(|r| r.coloured == r.h - 1), "each with the track's pixel under it: {runs:?}");
        assert_eq!(runs.iter().map(|r| r.colour).collect::<Vec<_>>(), vec![GREY, GREY, CONVERSATION]);
    }

    #[test]
    fn a_small_section_is_drawn_at_its_floor_and_its_pixels_come_off_the_biggest() {
        // Lachlan's later.md, 303 tokens of a 1M window, is half a pixel of a 1,700 px bar
        let bar = bar_of(
            &[(What::User, 9800), (What::Import, 303), (What::MemoryIndex, 4000), (What::System, 3300), (What::Conversation, 300_000)],
            1_000_000,
        );
        let runs = runs(&bar, 1700, 3, 1);
        assert_stacked(&bar, &runs, 1700);
        let h = heights(&runs);
        assert_eq!(filled(&bar, 1700), 540);
        assert_eq!(h[1], (1, 1, 3), "the import at the floor, not its half pixel");
        assert_eq!(h[0], (0, 0, 17), "the others their shares to the nearest pixel: 16.7");
        assert_eq!(h[2], (2, 2, 7), "6.8");
        assert_eq!(h[3], (3, 3, 6), "5.6");
        assert_eq!(h[4], (4, 4, 507), "the conversation, 510.4, what's left: the floor's pixels and the rounding's come off it");
        assert_eq!((runs[1].coloured, runs[1].colour), (2, GREY), "2 px of colour and a pixel's gap");
    }

    #[test]
    fn sections_with_nothing_in_them_and_free_space_get_no_run() {
        let bar = bar_of(&[(What::User, 50_000), (What::Skills, 0), (What::Conversation, 50_000)], 200_000);
        let runs = runs(&bar, 400, 3, 1);
        assert_eq!(heights(&runs), vec![(0, 0, 100), (2, 2, 100)]);
        assert_stacked(&bar, &runs, 400);
    }

    #[test]
    fn when_the_floors_dont_fit_neighbours_of_one_colour_join_the_smallest_first() {
        // 40k of 1M in a short window's 300 px bar: 12 px filled, six sections
        let bar = bar_of(
            &[
                (What::User, 9000),
                (What::Import, 300),
                (What::Project, 8000),
                (What::MemoryIndex, 3000),
                (What::Memory, 700),
                (What::Conversation, 19_000),
            ],
            1_000_000,
        );
        assert_eq!(filled(&bar, 300), 12);
        let runs = runs(&bar, 300, 3, 1);
        assert_stacked(&bar, &runs, 300);
        let spans: Vec<(usize, usize)> = runs.iter().map(|r| (r.first, r.last)).collect();
        // The import (300) joins the project's file, the smaller of its grey neighbours, then the
        // memory file (700) its index: four runs of 3 px fit in 12
        assert_eq!(spans, vec![(0, 0), (1, 2), (3, 4), (5, 5)]);
        assert!(runs.iter().all(|r| r.h >= 3), "{runs:?}");
        assert_eq!(runs[2].colour, YELLOW);
    }

    #[test]
    fn with_no_neighbours_of_one_colour_left_the_floor_is_what_each_can_have() {
        let bar = bar_of(&[(What::User, 1000), (What::MemoryIndex, 1000), (What::System, 1000), (What::Conversation, 7000)], 1_000_000);
        // 10k of 1M in 800 px: 8 px for four runs, no two alike: 2 px each, no gap
        let runs = runs(&bar, 800, 3, 1);
        assert_stacked(&bar, &runs, 800);
        assert_eq!(heights(&runs), vec![(0, 0, 2), (1, 1, 2), (2, 2, 2), (3, 3, 2)]);
        assert!(runs.iter().all(|r| r.coloured == r.h), "no gap under the floor: {runs:?}");
        // 3 px for four: the biggest shares win, the rest none
        let tiny = super::runs(&bar, 300, 3, 1);
        assert_stacked(&bar, &tiny, 300);
        assert_eq!(tiny.iter().map(|r| r.first).collect::<Vec<_>>(), vec![3], "the conversation's 2.1 px and the rest's 0.3: {tiny:?}");
    }

    #[test]
    fn labels_sit_level_with_their_sections_and_free_space_with_the_track_below_the_fill() {
        let bar = bar_of(&[(What::User, 100_000), (What::Conversation, 300_000)], 1_000_000);
        let runs = runs(&bar, 1000, 3, 1);
        let labels = labels(&bar, &runs, 1000, 16);
        let got: Vec<_> = labels.iter().map(|l| (l.section, l.text.as_str(), l.y, l.to)).collect();
        // The user's file is 0..100, the conversation 100..400, free 400..1000
        assert_eq!(got, vec![(0, "100k", 42, 50), (1, "300k", 242, 250), (2, "600k", 692, 700)]);
        assert!(labels.iter().all(|l| !l.moved(16)), "none pushed: {labels:?}");
    }

    #[test]
    fn labels_are_pushed_apart_down_then_back_up_from_the_foot_with_a_hairline_when_moved() {
        // Three small files on top of each other, each 3 px at its floor
        let bar = bar_of(
            &[(What::User, 1000), (What::Import, 1000), (What::MemoryIndex, 1000), (What::Conversation, 97_000)],
            100_000,
        );
        let runs = runs(&bar, 200, 3, 1);
        let labels = labels(&bar, &runs, 200, 16);
        let ys: Vec<u32> = labels.iter().map(|l| l.y).collect();
        for pair in ys.windows(2) {
            assert!(pair[1] >= pair[0] + 16, "no two overlap: {labels:?}");
        }
        assert_eq!(ys[0], 0, "the first as high as the track lets it");
        assert!(labels[1].moved(16) && labels[2].moved(16), "the pushed ones get a hairline: {labels:?}");
        // A full bar: the last labels run off the foot and come back up
        let full = bar_of(&[(What::Conversation, 96_000), (What::User, 2000), (What::Skills, 2000)], 100_000);
        let runs = super::runs(&full, 100, 3, 1);
        let labels = super::labels(&full, &runs, 100, 16);
        let last = labels.last().unwrap();
        assert_eq!(last.y, 84, "the foot's line: {labels:?}");
        assert!(labels.windows(2).all(|p| p[1].y >= p[0].y + 16), "{labels:?}");
    }

    #[test]
    fn when_the_labels_dont_fit_the_smallest_sections_go_first() {
        let bar = bar_of(
            &[(What::User, 3000), (What::Import, 300), (What::Project, 5000), (What::MemoryIndex, 400), (What::Conversation, 50_000)],
            100_000,
        );
        let runs = runs(&bar, 64, 3, 1);
        let labels = labels(&bar, &runs, 64, 16);
        // Four lines' room: free 41k, the conversation, the project's, the user's; in the bar's order
        assert_eq!(labels.iter().map(|l| l.section).collect::<Vec<_>>(), vec![0, 2, 4, 5], "{labels:?}");
        assert!(labels.windows(2).all(|p| p[1].y >= p[0].y + 16), "{labels:?}");
        assert!(super::labels(&bar, &runs, 10, 16).is_empty(), "no line's room, no labels");
    }

    #[test]
    fn the_widened_bar_writes_each_sections_number_on_it_and_a_thin_ones_in_the_pane_beside_it() {
        // A line and its gap the floor: every section has a line of its own and its number on it
        let bar = bar_of(
            &[(What::User, 100_000), (What::Import, 1000), (What::MemoryIndex, 1000), (What::Conversation, 300_000)],
            1_000_000,
        );
        let runs = runs(&bar, 1000, 17, 1);
        let n = numbers(&bar, &runs, 1000, 16);
        let got: Vec<_> = n.on.iter().map(|l| (l.section, l.text.as_str(), l.y)).collect();
        // The user's file 0..100 (99 coloured), the import and the index 17 each at the floor, the
        // conversation what's left of the 402 filled, free 402..1000
        assert_eq!(got, vec![(0, "100k", 41), (1, "1k", 100), (2, "1k", 117), (3, "300k", 259), (4, "598k", 693)]);
        assert!(n.thin.is_empty(), "{n:?}");
        for (l, r) in n.on.iter().zip(&runs) {
            assert!(l.y >= r.y && l.y + 16 <= r.y + r.coloured, "{l:?} inside its colour {r:?}");
        }
        // A short filled part: the user's file and its import join, and the floors still don't fit,
        // so every section's number goes beside the bar, pushed apart; free space keeps its own
        let bar = bar_of(&[(What::User, 1000), (What::Import, 1000), (What::MemoryIndex, 1000), (What::Conversation, 37_000)], 100_000);
        let runs = super::runs(&bar, 100, 17, 1);
        let n = numbers(&bar, &runs, 100, 16);
        assert_eq!(n.on.iter().map(|l| l.section).collect::<Vec<_>>(), vec![4], "{n:?}");
        assert_eq!(n.thin.iter().map(|l| l.section).collect::<Vec<_>>(), vec![0, 1, 2, 3], "{n:?}");
        assert!(n.thin.windows(2).all(|p| p[1].y >= p[0].y + 16), "{n:?}");
        assert_eq!(numbers(&bar, &runs, 10, 16), Numbers::default(), "no line's room, no numbers");
    }

    #[test]
    fn the_panes_heading_and_tokens_say_estimate_where_they_are() {
        let mut bar = bar_of(&[(What::User, 9_800), (What::Conversation, 302_200)], 1_000_000);
        assert_eq!(heading(&bar), "312k of 1M in use · 31%");
        assert_eq!(row_tokens(&bar, &bar.sections[0]), "≈9.8k");
        assert_eq!(row_tokens(&bar, &bar.sections[2]), "688k", "free is exact while what's in use is");
        bar.estimate = true;
        assert_eq!(heading(&bar), "≈312k of 1M in use · 31%");
        assert_eq!(row_tokens(&bar, &bar.sections[2]), "≈688k");
        let small = bar_of(&[(What::Conversation, 5_000)], 200_000);
        assert_eq!(heading(&small), "5k of 200k in use · 2.5%");
    }

    #[test]
    fn the_wheel_scrolls_the_panes_rows_only_as_far_as_fills_the_room() {
        // Ten rows of 20, one of them 36 with its note, in 120 of room, a line of 16 for the count
        let mut heights = vec![20.0; 10];
        heights[8] = 36.0;
        assert_eq!(first_shown(&heights, 400.0, 16.0, 3), 0, "every row fits: nothing to scroll");
        assert_eq!(first_shown(&heights, 120.0, 16.0, 0), 0);
        assert_eq!(first_shown(&heights, 120.0, 16.0, 3), 3);
        // From the foot up under the count's line: 16 + 20 + 36 + 20 + 20 = 112 of 120, rows 6 to 9
        assert_eq!(first_shown(&heights, 120.0, 16.0, 5), 5);
        assert_eq!(first_shown(&heights, 120.0, 16.0, 99), 6, "past the end the last page, nothing kept in hand");
        assert_eq!(first_shown(&[], 0.0, 16.0, 3), 0);
        assert_eq!(first_shown(&[50.0, 50.0], 40.0, 16.0, 5), 1, "no row fits under the line: the last row first");
    }
}
