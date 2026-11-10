//! The new-tab pad at the tab rail's foot (lbterm/DESIGN.md, "The + pad"; Lachlan's ask,
//! 2026-10-31: "There should be a button for new tab, and right clicking it should show a context
//! menu. The new tab should ask where you want to open the claude agent"). A left click opens the
//! picker beside the rail: a box that finds a project the way Lachlan's @names do, the open tab's
//! own folder first, then Home, the last projects picked, then every other A to Z. A right click
//! opens the + menu: the launch menu's shells in the open tab's folder, a shell in a project, the
//! tabs closed lately to reopen, and a new window. Which project a word names, where the words
//! end and the first message begins, the order, where every row goes and what's kept are here,
//! where they can be tested; the drawing, the keys and what a pick fires are wezterm-gui's
//! render/new_tab.rs.

use crate::projects::key;
use crate::tabs::Rect;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------- matching

/// A word as the picker compares it: lowercase, without spaces or a leading `@`, as an @name is
/// resolved (spaces optional, any case)
pub fn squash(text: &str) -> String {
    text.trim_start_matches('@')
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// A name's words, each with the index of its first character in the name
fn words(name: &str) -> Vec<(usize, String)> {
    let mut out = vec![];
    let mut start = None;
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((s, chars[s..i].iter().collect()));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, chars[s..].iter().collect()));
    }
    out
}

/// How a word found a project, best first
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// The whole name (any case, spaces optional): `bargainhunter`, `Warden`
    Whole,
    /// The start of the name: `war`, `barg`, `lbt`
    Start,
    /// The start of one of its words: `hunt` for Bargain Hunter
    Word,
    /// Anywhere in it
    Inside,
}

/// A project the box's first word names
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Its index in the config's projects
    pub project: usize,
    pub tier: Tier,
    /// Which of the name's characters the word matched, by index: drawn at white 100%
    pub marks: Vec<usize>,
    /// The name's word a following word of the box must be, whole, to join the name (else the
    /// first message starts there)
    next: Option<usize>,
}

/// How `word` (the box's first word) names `name`, if it does
pub fn rank(word: &str, name: &str) -> Option<(Tier, Vec<usize>, Option<usize>)> {
    let word = squash(word);
    if word.is_empty() {
        return None;
    }
    let name_words = words(name);
    // The name's characters without its spaces, each with its index in the name
    let squashed: Vec<(usize, char)> = name
        .chars()
        .enumerate()
        .filter(|(_, c)| !c.is_whitespace())
        .flat_map(|(i, c)| c.to_lowercase().map(move |l| (i, l)))
        .collect();
    let flat: String = squashed.iter().map(|(_, c)| *c).collect();
    let n = word.chars().count();
    let marks_from = |from: usize| -> Vec<usize> {
        let mut m: Vec<usize> = squashed[from..from + n].iter().map(|(i, _)| *i).collect();
        m.dedup();
        m
    };
    if flat == word {
        return Some((Tier::Whole, marks_from(0), None));
    }
    if flat.starts_with(&word) {
        // Ends where a word of the name ends: the next whole word may follow it
        let last = squashed[n - 1].0;
        let next = name_words
            .iter()
            .position(|(s, w)| last + 1 == s + w.chars().count())
            .map(|k| k + 1);
        return Some((Tier::Start, marks_from(0), next));
    }
    for (k, (start, w)) in name_words.iter().enumerate().skip(1) {
        let lower: String = w.chars().flat_map(char::to_lowercase).collect();
        if lower.starts_with(&word) {
            let marks = (*start..*start + n).collect();
            let next = (lower == word).then_some(k + 1);
            return Some((Tier::Word, marks, next));
        }
    }
    let at = flat.find(&word)?;
    let from = flat[..at].chars().count();
    Some((Tier::Inside, marks_from(from), None))
}

/// The box's first word, without the rest
pub fn first_word(query: &str) -> &str {
    query.trim_start().split_whitespace().next().unwrap_or("")
}

/// Whether `word` is one of a project's other names whole (any case, spaces optional, a leading
/// `@` dropped): the root's `AI` and `~`, as Blueprint's `resolve` (projects.ts, `INDEX_NAMES`)
/// and Terminal's at-names take them. Only whole: an alias is never a prefix or a substring, as
/// `resolve` never takes the root by a prefix
pub fn alias_names(word: &str, aliases: &[String]) -> bool {
    let word = squash(word);
    !word.is_empty() && aliases.iter().any(|a| squash(a) == word)
}

/// The projects the box's first word names, best first: by how well, then by `base`, the
/// picker's order with nothing typed (projects not in it go last, by their index). A project's
/// other names (`aliases`, by its index; none past its end) count as its whole name. Each project
/// once.
pub fn find(query: &str, names: &[&str], aliases: &[Vec<String>], base: &[usize]) -> Vec<Found> {
    let word = first_word(query);
    let place = |p: usize| base.iter().position(|&b| b == p).unwrap_or(base.len() + p);
    let mut found: Vec<Found> = names
        .iter()
        .enumerate()
        .filter_map(|(project, name)| {
            let by_name = rank(word, name);
            // Another name of its whole is as good as its own whole: no letters of the name marked
            if by_name.as_ref().map_or(true, |(tier, _, _)| *tier != Tier::Whole)
                && aliases.get(project).map_or(false, |a| alias_names(word, a))
            {
                return Some(Found { project, tier: Tier::Whole, marks: vec![], next: None });
            }
            let (tier, marks, next) = by_name?;
            Some(Found { project, tier, marks, next })
        })
        .collect();
    found.sort_by_key(|f| (f.tier, place(f.project)));
    found
}

/// The first message the box holds for `found`'s project (`name`): what comes after the words
/// that name it. A following word joins the name only when it's the name's next word whole:
/// "bargain hunter find a kettle" is Bargain Hunter and "find a kettle", "bargain hunt for
/// kettles" Bargain Hunter and "hunt for kettles".
pub fn message(query: &str, name: &str, found: &Found) -> String {
    let name_words = words(name);
    let mut rest = query.trim_start();
    // Past the first word
    rest = rest[rest.find(char::is_whitespace).unwrap_or(rest.len())..].trim_start();
    let mut next = found.next;
    while let Some(k) = next {
        let Some((_, want)) = name_words.get(k) else {
            break;
        };
        let word = first_word(rest);
        if word.is_empty() || word.to_lowercase() != want.to_lowercase() {
            break;
        }
        rest = rest[word.len()..].trim_start();
        next = Some(k + 1);
    }
    rest.trim().to_string()
}

/// The highlighted name put in the box, a space after it (Tab)
pub fn complete(name: &str) -> String {
    format!("{name} ")
}

/// Whether a digit picks a row: only while the box is empty (once it isn't, digits are words:
/// BTD2), 1 to 9, and only a row that's there
pub fn digit_pick(box_empty: bool, digit: char, rows: usize) -> Option<usize> {
    if !box_empty {
        return None;
    }
    let n = digit.to_digit(10)? as usize;
    (1..=9).contains(&n).then(|| n - 1).filter(|&i| i < rows)
}

/// What the list says when the box's first word names nothing
pub fn no_match(query: &str) -> String {
    format!("No project called {}", first_word(query))
}

// ---------------------------------------------------------------------------------- the order

/// The picker's groups, in their order, a rule between each
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// The open tab's folder, named by the deepest project above it
    Here,
    /// The index, the project whose folder holds every other's
    Home,
    /// The last projects picked here, newest first
    Recent,
    /// Every other project A to Z, each repo under its project
    All,
}

/// One row of the picker with nothing typed
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub project: usize,
    pub group: Group,
    /// The folder it opens in: the open tab's own for Here, else the project's
    pub dir: String,
    /// Here's path past its project's folder, shown dim, when it isn't the project's own
    pub sub: Option<String>,
    /// A repo of another project (its blue mark), and listed under it (indented)
    pub repo: bool,
    pub indent: bool,
}

/// How many picks are kept
pub const RECENT: usize = 6;

/// The project whose folder holds every other project's: Home, the megaproject's index. None
/// when no one does.
pub fn home(dirs: &[String]) -> Option<usize> {
    if dirs.len() < 2 {
        return None;
    }
    let keys: Vec<String> = dirs.iter().map(|d| key(d)).collect();
    (0..keys.len()).find(|&h| {
        !keys[h].is_empty()
            && keys
                .iter()
                .enumerate()
                .all(|(i, k)| i == h || k.starts_with(&format!("{}\\", keys[h])))
    })
}

/// The picker's rows with nothing typed (lbterm/DESIGN.md, "The + pad"): Here, the open tab's
/// folder (`here`), named by the deepest project above it; Home, unless it's Here's; the last
/// `RECENT` picked (`recent`, names newest first); then every other project A to Z, each repo
/// under its project. Each project once. Sessions and mail never move a row: a pick moves its
/// project into the recent ones, nothing else does.
pub fn order(names: &[&str], dirs: &[String], parents: &[Option<usize>], here: Option<&str>, recent: &[String]) -> Vec<Entry> {
    let n = names.len().min(dirs.len());
    let repo = |i: usize| parents.get(i).copied().flatten().filter(|&p| p < n && p != i);
    let mut seen = HashSet::new();
    let mut out = vec![];
    if let Some(folder) = here.filter(|f| !key(f).is_empty()) {
        if let Some(p) = crate::projects::project_of(&dirs[..n], folder) {
            let root = key(&dirs[p]);
            let inside = key(folder);
            // Its path past the project's folder, as the folder spells it
            let sub = (inside != root).then(|| {
                let tail: String = folder.trim().chars().skip(dirs[p].trim().trim_end_matches(['\\', '/']).chars().count()).collect();
                tail.trim_start_matches(['\\', '/']).trim_end_matches(['\\', '/']).replace('\\', "/")
            });
            seen.insert(p);
            out.push(Entry {
                project: p,
                group: Group::Here,
                dir: folder.trim().trim_end_matches(['\\', '/']).to_string(),
                sub: sub.filter(|s| !s.is_empty()),
                repo: repo(p).is_some(),
                indent: false,
            });
        }
    }
    // Takes `seen` from its caller, so the tops below can look at it between pushes
    let plain = |p: usize, group: Group, seen: &mut HashSet<usize>, out: &mut Vec<Entry>, indent: bool| {
        if seen.insert(p) {
            out.push(Entry { project: p, group, dir: dirs[p].clone(), sub: None, repo: repo(p).is_some(), indent });
        }
    };
    if let Some(h) = home(&dirs[..n]) {
        plain(h, Group::Home, &mut seen, &mut out, false);
    }
    for name in recent.iter().take(RECENT) {
        if let Some(p) = (0..n).find(|&i| names[i].eq_ignore_ascii_case(name)) {
            plain(p, Group::Recent, &mut seen, &mut out, false);
        }
    }
    let by_name = |a: &usize, b: &usize| names[*a].to_lowercase().cmp(&names[*b].to_lowercase());
    // A repo goes under its project while that's listed here too; else it's one of the tops
    let mut tops: Vec<usize> = (0..n)
        .filter(|&i| !seen.contains(&i) && repo(i).map_or(true, |p| seen.contains(&p)))
        .collect();
    tops.sort_by(by_name);
    for top in tops {
        plain(top, Group::All, &mut seen, &mut out, false);
        let mut repos: Vec<usize> = (0..n).filter(|&i| repo(i) == Some(top)).collect();
        repos.sort_by(by_name);
        for r in repos {
            plain(r, Group::All, &mut seen, &mut out, true);
        }
    }
    out
}

/// Where the groups change down a list: the rows a rule goes before
pub fn breaks(groups: &[Group]) -> Vec<usize> {
    (1..groups.len()).filter(|&i| groups[i] != groups[i - 1]).collect()
}

/// `name` picked: first among the recent ones, `RECENT` kept
pub fn remember(recent: &mut Vec<String>, name: &str) {
    recent.retain(|n| !n.eq_ignore_ascii_case(name));
    recent.insert(0, name.to_string());
    recent.truncate(RECENT);
}

/// The recent picks a file holds: a JSON list of names, newest first. Anything else holds none.
pub fn read_recent(text: &str) -> Vec<String> {
    let mut list: Vec<String> = serde_json::from_str(text).unwrap_or_default();
    list.truncate(RECENT);
    list
}

/// The recent picks as their file holds them
pub fn recent_text(recent: &[String]) -> String {
    serde_json::to_string(recent).unwrap_or_else(|_| "[]".to_string())
}

/// The picks, kept for every window of this GUI: in memory, read from `lbterm_projects.recent_file`
/// once at start on a thread of its own, and written through a rename on a thread of its own after
/// each pick. Nothing here touches the disk on the caller's thread.
static PICKS: Mutex<Picks> = Mutex::new(Picks { list: Vec::new(), file: None, read: false });

struct Picks {
    list: Vec<String>,
    file: Option<PathBuf>,
    read: bool,
}

/// The picks, newest first
pub fn recent() -> Vec<String> {
    PICKS.lock().map(|p| p.list.clone()).unwrap_or_default()
}

/// Where the picks are kept: read once, on a thread, the first time a file is named; a pick made
/// before it's read stays first. None keeps them in memory alone.
pub fn keep_in(file: Option<&Path>) {
    let Ok(mut picks) = PICKS.lock() else {
        return;
    };
    let file = file.map(Path::to_path_buf);
    if picks.file == file {
        return;
    }
    picks.file = file.clone();
    picks.read = false;
    let Some(file) = file else {
        return;
    };
    let spawned = std::thread::Builder::new().name("lbterm-newtab-recent".into()).spawn(move || {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let read = read_recent(&text);
        if let Ok(mut picks) = PICKS.lock() {
            if picks.file.as_deref() != Some(file.as_path()) {
                return;
            }
            // Picks made since come first
            let mut list = std::mem::take(&mut picks.list);
            for name in read {
                if !list.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    list.push(name);
                }
            }
            list.truncate(RECENT);
            picks.list = list;
            picks.read = true;
        }
    });
    if let Err(err) = spawned {
        log::error!("new tab: the recent picks' reader didn't start: {err:#}");
    }
}

/// `name` picked: first in memory at once, and its file written again on a thread of its own
pub fn pick(name: &str) {
    let (file, text) = {
        let Ok(mut picks) = PICKS.lock() else {
            return;
        };
        remember(&mut picks.list, name);
        (picks.file.clone(), recent_text(&picks.list))
    };
    let Some(file) = file else {
        return;
    };
    let spawned = std::thread::Builder::new().name("lbterm-newtab-recent".into()).spawn(move || {
        if let Err(err) = write_through_rename(&file, &text) {
            log::warn!("new tab: the recent picks weren't kept in {}: {err:#}", file.display());
        }
    });
    if let Err(err) = spawned {
        log::error!("new tab: the recent picks' writer didn't start: {err:#}");
    }
}

/// Write `text` to `<file>.tmp` and rename it over `file`, so a reader never sees half of it
pub fn write_through_rename(file: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = crate::state::tmp_path(file);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, file)
}

// ---------------------------------------------------------------------------------- the picker

/// What the picker opens in its project, as the `lbterm-project-action` event names it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Claude: Enter opens Claude, Ctrl+Enter a shell
    Claude,
    /// "Shell in…", from the + menu: Enter opens a shell, Ctrl+Enter Claude, and no first message
    Shell,
}

/// What a pick does
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Claude in the project, in front or behind
    New { behind: bool },
    /// A shell there, in front
    Shell,
    /// Its newest conversation that isn't open (Alt+R, the Resume chip)
    Resume,
    /// Claude told to read its InMail (Alt+M, the InMail chip), only with mail
    Inmail,
}

impl Pick {
    /// As the event's `action` names it
    pub fn action(self) -> &'static str {
        match self {
            Pick::New { .. } => "new",
            Pick::Shell => "shell",
            Pick::Resume => "resume",
            Pick::Inmail => "inmail",
        }
    }
}

/// What Enter does in `mode`, with Shift (behind) or Ctrl (the other kind)
pub fn enter(mode: Mode, shift: bool, ctrl: bool) -> Pick {
    match (mode, ctrl) {
        (Mode::Claude, false) | (Mode::Shell, true) => Pick::New { behind: shift && mode == Mode::Claude },
        (Mode::Claude, true) | (Mode::Shell, false) => Pick::Shell,
    }
}

/// The placeholder in the empty box
pub fn placeholder(mode: Mode) -> &'static str {
    match mode {
        Mode::Claude => "Open Claude in\u{2026}",
        Mode::Shell => "Shell in\u{2026}",
    }
}

/// The footer under the box: what the keys do
pub fn footer(mode: Mode) -> &'static str {
    match mode {
        Mode::Claude => "Enter open \u{b7} Shift+Enter behind \u{b7} Ctrl+Enter shell",
        Mode::Shell => "Enter shell \u{b7} Ctrl+Enter Claude",
    }
}

/// A chip on the highlighted row
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Resume,
    Inmail,
}

impl Chip {
    pub fn pick(self) -> Pick {
        match self {
            Chip::Resume => Pick::Resume,
            Chip::Inmail => Pick::Inmail,
        }
    }

    pub fn label(self, mail: usize) -> String {
        match self {
            Chip::Resume => "Resume".to_string(),
            Chip::Inmail => format!("InMail {mail}"),
        }
    }
}

/// Sizes at 96 DPI
const PICKER_W: f64 = 360.0;
const PICKER_ROW: f64 = 28.0;
const PICKER_BOX: f64 = 36.0;
const PICKER_RULE: f64 = 9.0;
const PICKER_PAD: f64 = 6.0;
/// At most this many rows show; the wheel scrolls the rest
pub const PICKER_ROWS: usize = 16;
/// At most this much of the window's height
const PICKER_TALL: f64 = 0.7;
/// The key chip on the first nine rows, and how far in the words start
const KEY_CHIP: f64 = 16.0;
const ROW_INSET: f64 = 8.0;

/// Where the picker's parts go
#[derive(Debug, Clone, PartialEq)]
pub struct PickerLayout {
    pub card: Rect,
    /// Each row shown: its place in the list, and where it is (row 0 nearest the box)
    pub rows: Vec<(usize, Rect)>,
    /// The rules between groups
    pub rules: Vec<Rect>,
    /// The box, the card's last row
    pub input: Rect,
    /// "First message: …", over the footer, when there's one
    pub message: Option<Rect>,
    /// The footer under the box
    pub footer: Rect,
    /// How far up the list the first row shown is, held inside it
    pub scroll: usize,
    pub scale: f64,
}

impl PickerLayout {
    /// The row under a point, by its place in the list
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.rows.iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| *i)
    }

    /// How many rows fit
    pub fn shown(&self) -> usize {
        self.rows.len()
    }

    /// Row `r`'s key chip, on the first nine rows of the list
    pub fn key_chip(&self, r: &Rect, place: usize) -> Option<Rect> {
        (place < 9).then(|| {
            let s = KEY_CHIP * self.scale;
            Rect { x: r.x + ROW_INSET * self.scale, y: r.y + (r.h - s) / 2.0, w: s, h: s }
        })
    }

    /// Where a row's name starts: past the key chip
    pub fn name_x(&self, r: &Rect) -> f64 {
        r.x + (ROW_INSET + KEY_CHIP + 8.0) * self.scale
    }
}

/// The picker: its card's left edge at `left` (a gutter past the rail, or past the open panel),
/// its bottom at `bottom` (the panel's), growing upward to at most 70% of the window's height
/// (`window_h`); `list` rows, a rule before each of `breaks`, scrolled `scroll` rows up; `line`
/// the height of a line of words, and a first message's line over the footer when `message`.
#[allow(clippy::too_many_arguments)]
pub fn picker_layout(
    left: f64,
    bottom: f64,
    window_h: f64,
    scale: f64,
    line: f64,
    list: usize,
    breaks: &[usize],
    scroll: usize,
    message: bool,
) -> PickerLayout {
    let pad = PICKER_PAD * scale;
    let row = (PICKER_ROW * scale).round();
    let rule = (PICKER_RULE * scale).round();
    let words = (line + 4.0 * scale).round();
    let w = (PICKER_W * scale).round();
    let x = left;
    let footer = Rect { x: x + pad, y: bottom - pad - words, w: w - 2.0 * pad, h: words };
    let message_line = message.then(|| Rect { x: x + pad, y: footer.y - words, w: w - 2.0 * pad, h: words });
    let input_bottom = message_line.map_or(footer.y, |m| m.y);
    let input = Rect { x: x + pad, y: input_bottom - (PICKER_BOX * scale).round(), w: w - 2.0 * pad, h: (PICKER_BOX * scale).round() };
    // The rows' room: up from over the box to 70% of the window at most, and 16 rows at most
    let ceiling = bottom - PICKER_TALL * window_h;
    let room = (input.y - pad - ceiling).max(row);
    let fit = ((room / row).floor() as usize).clamp(1, PICKER_ROWS);
    // A list of none still has a row, to say there's no match
    let count = list.max(1);
    let mut shown = fit.min(count);
    let mut scroll = scroll.min(count - shown);
    // The rules take room too: as many rows as fit with them
    loop {
        let rules = breaks.iter().filter(|&&b| b > scroll && b < scroll + shown).count();
        if shown <= 1 || shown as f64 * row + rules as f64 * rule <= room {
            break;
        }
        shown -= 1;
        scroll = scroll.min(count - shown);
    }
    let mut rows = vec![];
    let mut rules = vec![];
    let mut y = input.y - pad;
    for place in scroll..scroll + shown {
        if place > scroll && breaks.contains(&place) {
            y -= rule;
            rules.push(Rect { x: x + 2.0 * pad, y: (y + rule / 2.0).round(), w: w - 4.0 * pad, h: scale.max(1.0).round() });
        }
        y -= row;
        rows.push((place, Rect { x: x + pad, y, w: w - 2.0 * pad, h: row }));
    }
    let top = y - pad;
    PickerLayout {
        card: Rect { x, y: top, w, h: bottom - top },
        rows,
        rules,
        input,
        message: message_line,
        footer,
        scroll,
        scale,
    }
}

/// The chips on the highlighted row, right-aligned in it: Resume, and InMail with its mail
pub fn chips(row: &Rect, mail: usize, cell: f64, scale: f64) -> Vec<(Chip, Rect)> {
    let mut out = vec![];
    let h = (row.h - 8.0 * scale).round();
    let mut right = row.x + row.w - 6.0 * scale;
    let mut want = vec![Chip::Resume];
    if mail > 0 {
        want.push(Chip::Inmail);
    }
    for chip in want.into_iter().rev() {
        let w = ((chip.label(mail).chars().count() as f64 + 2.0) * cell).round();
        right -= w;
        out.push((chip, Rect { x: right, y: row.y + (row.h - h) / 2.0, w, h }));
        right -= 4.0 * scale;
    }
    out.reverse();
    out
}

/// What moves the highlight: Up goes up the card, away from the box (the next row of the list)
pub fn step(highlight: usize, by: isize, rows: usize) -> usize {
    if rows == 0 {
        return 0;
    }
    (highlight as isize + by).clamp(0, rows as isize - 1) as usize
}

/// The scroll that keeps the highlight in sight
pub fn follow(scroll: usize, highlight: usize, shown: usize) -> usize {
    if highlight < scroll {
        highlight
    } else if shown > 0 && highlight >= scroll + shown {
        highlight + 1 - shown
    } else {
        scroll
    }
}

// ---------------------------------------------------------------------------------- the + menu

/// A row of the + menu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlusItem {
    /// `config.launch_menu`'s entry, "<label> here", in the open tab's folder
    Launch(usize),
    /// The picker titled "Shell in…"
    ShellIn,
    /// A tab closed lately, newest first
    Reopen(usize),
    /// Said dim when nothing's been closed yet
    NothingClosed,
    NewWindow,
}

/// At most this many Reopen rows
pub const REOPEN_ROWS: usize = 5;

/// The + menu's rows top to bottom, `None` a rule: the launch menu's `launch` entries, Shell in a
/// project…, a rule, up to five tabs closed lately (`closed`) or a dim row saying none, a rule,
/// New window
pub fn plus_rows(launch: usize, closed: usize) -> Vec<Option<PlusItem>> {
    let mut rows: Vec<Option<PlusItem>> = (0..launch).map(|i| Some(PlusItem::Launch(i))).collect();
    rows.push(Some(PlusItem::ShellIn));
    rows.push(None);
    if closed == 0 {
        rows.push(Some(PlusItem::NothingClosed));
    } else {
        rows.extend((0..closed.min(REOPEN_ROWS)).map(|i| Some(PlusItem::Reopen(i))));
    }
    rows.push(None);
    rows.push(Some(PlusItem::NewWindow));
    rows
}

/// The keys the + menu's own rows take, so a launch entry's letter can't be one of them
const PLUS_OWN_KEYS: [char; 3] = ['I', 'R', 'N'];

/// Each launch entry's key: its label's first letter, where no other entry's starts with it and
/// the menu's own rows don't take it
pub fn launch_keys(labels: &[&str]) -> Vec<Option<char>> {
    let first: Vec<Option<char>> = labels
        .iter()
        .map(|l| l.trim().chars().next().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_uppercase()))
        .collect();
    first
        .iter()
        .map(|c| c.filter(|c| !PLUS_OWN_KEYS.contains(c) && first.iter().filter(|o| **o == Some(*c)).count() == 1))
        .collect()
}

/// A row's key: a launch entry's from `launch_keys`, I, R for the newest closed tab, N
pub fn plus_key(item: PlusItem, launch: &[Option<char>]) -> Option<char> {
    match item {
        PlusItem::Launch(i) => launch.get(i).copied().flatten(),
        PlusItem::ShellIn => Some('I'),
        PlusItem::Reopen(0) => Some('R'),
        PlusItem::NewWindow => Some('N'),
        _ => None,
    }
}

/// The keys bound to `LbtermNewTab`'s three, as the window's key table has them (Lachlan's
/// lbterm.lua binds Ctrl+T, Ctrl+Alt+T and Ctrl+Shift+T, but only in a build with the action, and
/// a config of anyone's may bind other keys or none): what the open panel's pad row and the +
/// menu say, nothing where nothing's bound (lbterm/PITFALLS.md, 119)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyWords {
    pub picker: Option<String>,
    pub shell: Option<String>,
    pub reopen: Option<String>,
}

/// Whether a paste key pastes into the picker's box, by its words ([`key_words`]): Ctrl+V alone
/// (Lachlan's 4b of 2026-11-08). Every other key his config binds to a paste (Ctrl+Shift+V,
/// Shift+Insert) goes on to the picker as a key no row takes, as any bound key does (his 7b)
pub fn pastes_into_picker(words: &str) -> bool {
    words == "Ctrl+V"
}

/// A key as the pad and the + menu name it: its modifiers in Windows' order, then the key,
/// joined with +, "Ctrl+Alt+T"
pub fn key_words(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> String {
    let mut words: Vec<&str> = vec![];
    for (on, word) in [(ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift"), (win, "Win")] {
        if on {
            words.push(word);
        }
    }
    words.push(key);
    words.join("+")
}

/// A row's words left of its key: the key bound to a shell here for the first launch entry
/// (PowerShell here, Ctrl+Alt+T in his config) and to Reopen for the newest closed tab
/// (Ctrl+Shift+T), when one is
pub fn plus_hint(item: PlusItem, keys: &KeyWords) -> Option<&str> {
    match plus_hinted(item)? {
        Hinted::Shell => keys.shell.as_deref(),
        Hinted::Reopen => keys.reopen.as_deref(),
    }
}

/// The two of `LbtermNewTab`'s actions the + menu names a key for beside a row (`plus_hint`): a
/// shell here, beside the first launch entry, and Reopen, beside the newest closed tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hinted {
    Shell,
    Reopen,
}

/// Which action's key a row shows beside it, if any
fn plus_hinted(item: PlusItem) -> Option<Hinted> {
    match item {
        PlusItem::Launch(0) => Some(Hinted::Shell),
        PlusItem::Reopen(0) => Some(Hinted::Reopen),
        _ => None,
    }
}

/// The row of the open + menu a bound key picks (Lachlan's 5a of 2026-11-08): the one that shows
/// that very key beside it as its shortcut, so the menu does what it says. `action` is what the key
/// table binds the key to and `words` the key named as the menu names keys (`key_words`). None
/// when no row shows it, a second key bound to the same action included: that's a key no row
/// takes, which only flares the card's edge (`lbterm_motion::menu::bump`), as every other bound key over an open menu (his 7b
/// of 2026-11-07; lbterm/PITFALLS.md, 185)
pub fn plus_by_shortcut(rows: &[Option<PlusItem>], keys: &KeyWords, action: Hinted, words: &str) -> Option<PlusItem> {
    rows.iter()
        .flatten()
        .copied()
        .find(|&item| plus_hinted(item) == Some(action) && plus_hint(item, keys) == Some(words))
}

/// What a paste puts in the picker's box, which holds one line (Lachlan's 11a of 2026-11-08): the
/// clipboard's first line with something in it, without the spaces round it, so a name copied
/// with its line break or a selection's indent searches as typed. The box takes it as typed
/// words (`Input::insert`: over the selection, tabs as spaces, control characters left out)
pub fn pasted(clip: &str) -> &str {
    clip.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("")
}

/// The part of the picker's one-line box's line that's measured and drawn, `from..to` in
/// characters, of a line `len` long with its caret at `caret`; `fits(from, to)` says whether the
/// words `from..to` fit the box. `from` is the earliest start whose words up to the caret fit (as
/// much as fits before the caret, the caret at the box's right once the line is longer); `to` is
/// the first end past the caret whose words from `from` no longer fit, or the line's end: the
/// word drawn is cut there, and what of it passes the box's edge is clipped as before.
///
/// Both are found by galloping out from the caret, 1, 2, 4... characters, then halving between
/// the last span that fit and the first that didn't, so no span measured is more than twice what
/// the box holds, and there are a few times the log of that of them (nine for a box of ten
/// characters part way into a line of 5,000): the cost is bounded by the
/// box, wherever the caret is in however long a line (a paste of thousands of characters, then
/// Left or Home). A line that fits is measured in about its length's log of short suffixes, where
/// it was one measure of the line: the price of never measuring a long one. It measured the whole
/// line before the caret first and drew the line's whole tail, which `compute_element` shapes
/// before clipping, so each Left press after a long paste shaped the line twice (lbterm/PITFALLS.md,
/// 192 and 209); before that it walked forward from the line's start, a measure at each character.
/// `fits` must be monotonic: a span that fits fits shorter
pub fn box_span(caret: usize, len: usize, mut fits: impl FnMut(usize, usize) -> bool) -> (usize, usize) {
    let caret = caret.min(len);
    // Back from the caret: the earliest start that fits, between `low` (none fit from it) and
    // `fit` (it fits from there)
    let mut fit = caret;
    let mut low = None;
    let mut step = 1;
    while fit > 0 {
        let probe = caret.saturating_sub(step);
        if fits(probe, caret) {
            fit = probe;
            step *= 2;
        } else {
            low = Some(probe);
            break;
        }
    }
    if let Some(mut low) = low {
        while fit - low > 1 {
            let mid = low + (fit - low) / 2;
            if fits(mid, caret) {
                fit = mid;
            } else {
                low = mid;
            }
        }
    }
    let from = fit;
    // On from the caret: the first end that no longer fits, which covers the box, between `fit`
    // (it fits to there) and `high` (it doesn't)
    let mut fit = caret;
    let mut high = None;
    let mut step = 1;
    while fit < len {
        let probe = (caret + step).min(len);
        if fits(from, probe) {
            fit = probe;
            step *= 2;
        } else {
            high = Some(probe);
            break;
        }
    }
    let to = match high {
        None => len,
        Some(mut high) => {
            while high - fit > 1 {
                let mid = fit + (high - fit) / 2;
                if fits(from, mid) {
                    fit = mid;
                } else {
                    high = mid;
                }
            }
            high
        }
    };
    (from, to)
}

/// The item a typed key picks, if any row takes it
pub fn plus_by_key(rows: &[Option<PlusItem>], launch: &[Option<char>], key: char) -> Option<PlusItem> {
    let key = key.to_ascii_uppercase();
    rows.iter().flatten().copied().find(|&item| plus_key(item, launch) == Some(key))
}

/// Sizes at 96 DPI: the tab menu's
const PLUS_WIDTH: f64 = 260.0;
const PLUS_ROW: f64 = 30.0;
/// A Reopen row holds two lines: what it was, and when and where
const PLUS_REOPEN_ROW: f64 = 42.0;
const PLUS_RULE: f64 = 9.0;
const PLUS_PAD: f64 = 6.0;
const PLUS_MARGIN: f64 = 6.0;

#[derive(Debug, Clone, PartialEq)]
pub struct PlusLayout {
    pub card: Rect,
    pub rows: Vec<(Option<PlusItem>, Rect)>,
    pub scale: f64,
}

impl PlusLayout {
    /// The row under a point; a dim row does nothing
    pub fn hit(&self, x: f64, y: f64) -> Option<PlusItem> {
        self.rows
            .iter()
            .find(|(item, r)| item.is_some() && r.contains(x, y))
            .and_then(|(item, _)| *item)
            .filter(|item| *item != PlusItem::NothingClosed)
    }
}

/// The + menu opened at `at` (the pointer, on the + at the rail's foot) in a window `size` big:
/// up and to the right of the pointer, flipped to stay inside the window
pub fn plus_layout(at: (f64, f64), size: (f64, f64), scale: f64, rows: &[Option<PlusItem>]) -> PlusLayout {
    let width = (PLUS_WIDTH * scale).min((size.0 - 2.0 * PLUS_MARGIN * scale).max(PLUS_WIDTH * scale));
    let tall = |r: &Option<PlusItem>| match r {
        None => PLUS_RULE,
        Some(PlusItem::Reopen(_)) => PLUS_REOPEN_ROW,
        Some(_) => PLUS_ROW,
    } * scale;
    let height = 2.0 * PLUS_PAD * scale + rows.iter().map(tall).sum::<f64>();
    let margin = PLUS_MARGIN * scale;
    let mut x = at.0;
    if x + width + margin > size.0 {
        x = (at.0 - width).max(margin);
    }
    let mut y = at.1 - height;
    if y < margin {
        y = at.1.min(size.1 - height - margin).max(margin);
    }
    let mut top = y + PLUS_PAD * scale;
    let rows = rows
        .iter()
        .map(|r| {
            let h = tall(r);
            let rect = Rect { x: x + PLUS_PAD * scale, y: top, w: width - 2.0 * PLUS_PAD * scale, h };
            top += h;
            (*r, rect)
        })
        .collect();
    PlusLayout { card: Rect { x, y, w: width, h: height }, rows, scale }
}

/// The keys step along the rows that do something, round the ends
pub fn plus_step(rows: &[Option<PlusItem>], from: Option<PlusItem>, by: isize) -> Option<PlusItem> {
    let items: Vec<PlusItem> = rows.iter().flatten().copied().filter(|i| *i != PlusItem::NothingClosed).collect();
    if items.is_empty() {
        return None;
    }
    let len = items.len() as isize;
    let next = match from.and_then(|f| items.iter().position(|i| *i == f)) {
        Some(at) => (at as isize + by).rem_euclid(len),
        None if by < 0 => len - 1,
        None => 0,
    };
    Some(items[next as usize])
}

// ---------------------------------------------------------------------------------- Reopen

/// A tab Lachlan closed himself (the closes that write close marks), kept to open again
#[derive(Debug, Clone, PartialEq)]
pub struct Closed {
    /// Every pane's Claude session, the active pane's first
    pub sessions: Vec<String>,
    /// Its row's label
    pub label: String,
    /// Its folder: its session's (cwd.txt), else its pane's
    pub dir: String,
    /// The project that folder is in, as the picker names it
    pub project: String,
    /// When it closed, in Unix seconds
    pub at: f64,
}

/// How many closed tabs are kept, in memory alone
pub const CLOSED_KEPT: usize = 10;

/// What's kept of a tab as Lachlan closes it: `sessions` (each pane's Claude session, the active
/// pane's first) each once, its row's `label`, its folder as its session's statusline last said it
/// (`said`, cwd.txt), or where that said none as its pane last did (`pane_said`, a `file://` URL
/// or a path), the `project` that folder is in, and when, `at` in Unix seconds
pub fn closed(sessions: Vec<String>, label: &str, said: &str, pane_said: &str, project: String, at: f64) -> Closed {
    let mut once: Vec<String> = Vec::with_capacity(sessions.len());
    for sid in sessions {
        let sid = sid.trim();
        if !sid.is_empty() && !once.iter().any(|s| s == sid) {
            once.push(sid.to_string());
        }
    }
    let own = folder_of(said);
    let dir = if own.is_empty() { folder_of(pane_said) } else { own };
    Closed { sessions: once, label: label.trim().to_string(), dir, project, at }
}

/// `tab` closed: first in the list, ten kept
pub fn keep_closed(list: &mut Vec<Closed>, tab: Closed) {
    list.insert(0, tab);
    list.truncate(CLOSED_KEPT);
}

/// How long ago, for a Reopen row: "now", "4 min", "2 h", "3 d"
pub fn age(at: f64, now: f64) -> String {
    let s = (now - at).max(0.0);
    if s < 60.0 {
        "now".to_string()
    } else if s < 3600.0 {
        format!("{} min", (s / 60.0).floor())
    } else if s < 86400.0 {
        format!("{} h", (s / 3600.0).floor())
    } else {
        format!("{} d", (s / 86400.0).floor())
    }
}

/// A Reopen row's two lines: "Reopen <label>", and "<age> · <project>"
pub fn reopen_words(c: &Closed, now: f64) -> (String, String) {
    let label = if c.label.trim().is_empty() { "tab" } else { c.label.trim() };
    let when = age(c.at, now);
    let second = if c.project.is_empty() { when } else { format!("{when} \u{b7} {}", c.project) };
    (format!("Reopen {label}"), second)
}

/// What the Starting row says on the open panel
pub const STARTING: &str = "Starting\u{2026}";

/// What the strip says when Reopen (Ctrl+Shift+T in his config) has nothing kept to open
pub const NOTHING_CLOSED: &str = "Nothing closed to reopen";

/// What the strip says when the picker is asked for (Ctrl+T in his config) with the pad off
/// (`lbterm_tab_panel.enabled` or its `new_tab` false)
pub const PICKER_OFF: &str = "The new-tab picker is off";

/// What the strip says for a start that opened nothing
pub fn nothing_opened(what: &str) -> String {
    format!("Nothing opened for {}", what.trim_end_matches(['.', '\u{2026}']))
}

/// How long the Starting row waits for its tab before it folds and the strip says nothing
/// opened: the strip's own wait for a tab on its way (strip::OPENING_FOR)
pub const STARTING_FOR: Duration = crate::strip::OPENING_FOR;

/// The Starting row (lbterm/NEW-TAB.md, "The Starting row"): a tab asked for in this window, shown
/// at the end of the rail's tabs until the first tab the window didn't have comes, which it
/// dissolves into, or `STARTING_FOR` is up
#[derive(Debug, Clone, PartialEq)]
pub struct Starting {
    /// Which start it is, for its record and for taking it back (render/new_tab.rs)
    pub id: u64,
    /// Its label: the project's name, or the shell's
    pub label: String,
    pub since: Instant,
    /// The window's tabs when it was asked for, by id
    pub tabs: Vec<usize>,
    /// Opened behind: the window stays on its tab, and the new tab's pad glints as it comes
    pub behind: bool,
}

/// Where a Starting row is at `now`, the window having `tabs`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartingNow {
    /// Still on its way: look again by the instant
    Waiting(Instant),
    /// The first tab the window didn't have came: this one, which the row dissolves into
    Arrived(usize),
    /// Nothing came in time: the row folds and the strip says so
    Expired,
}

pub fn starting_now(starting: &Starting, now: Instant, tabs: &[usize]) -> StartingNow {
    if let Some(&tab) = tabs.iter().find(|t| !starting.tabs.contains(t)) {
        return StartingNow::Arrived(tab);
    }
    let until = starting.since + STARTING_FOR;
    if now >= until {
        StartingNow::Expired
    } else {
        StartingNow::Waiting(until)
    }
}

/// The tabs asked for in a window that haven't come, oldest first (Shift+Enter in the picker starts
/// one behind and leaves the picker open, so several can be on their way at once). The rail shows
/// the oldest's Starting row, and each tab the window didn't have goes to the oldest still
/// waiting, in the window's order: a tab one start took counts as had for every later one, so
/// the second start's tab is never the first's (lbterm/PITFALLS.md, 120).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Starts {
    waiting: Vec<Starting>,
}

/// What came of a start, from `Starts::look`
#[derive(Debug, Clone, PartialEq)]
pub enum Came {
    /// The tab it was waiting for
    Arrived { start: Starting, tab: usize },
    /// Nothing in `STARTING_FOR`: its row folds and the strip says so
    Expired(Starting),
}

impl Starts {
    pub fn ask(&mut self, start: Starting) {
        self.waiting.push(start);
    }

    /// The start whose Starting row the rail shows: the oldest waiting
    pub fn showing(&self) -> Option<&Starting> {
        self.waiting.first()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    /// The newest, taken back: Lua's `expect_tab(nil)`, from the handler of the pick just made
    pub fn take_back(&mut self) -> Option<Starting> {
        self.waiting.pop()
    }

    /// The start `id`, taken back, if it's still waiting
    pub fn take(&mut self, id: u64) -> Option<Starting> {
        let at = self.waiting.iter().position(|s| s.id == id)?;
        Some(self.waiting.remove(at))
    }

    /// Where each start is at `now`, the window having `tabs`: what came of those done, in the
    /// order they were asked, and when to look again for the rest. Never early: a start expires
    /// only once its own `STARTING_FOR` is up, however late it's looked at (starting_now).
    pub fn look(&mut self, now: Instant, tabs: &[usize]) -> (Vec<Came>, Option<Instant>) {
        let mut came = vec![];
        let mut claimed: Vec<usize> = vec![];
        let mut left = vec![];
        for mut start in self.waiting.drain(..) {
            // A tab an earlier start took is one this start's window had
            for t in &claimed {
                if !start.tabs.contains(t) {
                    start.tabs.push(*t);
                }
            }
            match starting_now(&start, now, tabs) {
                StartingNow::Arrived(tab) => {
                    claimed.push(tab);
                    came.push(Came::Arrived { start, tab });
                }
                StartingNow::Expired => came.push(Came::Expired(start)),
                StartingNow::Waiting(_) => left.push(start),
            }
        }
        self.waiting = left;
        let due = self.waiting.iter().map(|s| s.since + STARTING_FOR).min();
        (came, due)
    }
}

/// A Starting row's label from what Lua's `window:lbterm_expect_tab` says ("Opening Claude in
/// Warden", "Reopening Rail fix", "Opening Chronos's InMail"): what's being opened, without the
/// verb
pub fn starting_label(text: &str) -> String {
    let mut s = text.trim().trim_end_matches(['.', '\u{2026}']).trim();
    for verb in ["Reopening ", "Opening ", "Starting "] {
        if let Some(rest) = s.strip_prefix(verb) {
            s = rest;
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("Claude in ") {
        s = rest;
    }
    s.trim().to_string()
}

/// A folder as a pane or a statusline says it (a `file://` URL, or a path) as a path in Windows'
/// spelling, its case kept, without a slash at its end: where the picker's Here row opens and
/// what the event hands Lua. A drive's path has backslashes whatever said it (a pane's URL has
/// forward slashes, a statusline's cwd.txt and the config's projects backslashes), and a host's
/// is `\\host\share`: a handler that compares folders as text meets one spelling (lbterm/
/// PITFALLS.md, 115)
pub fn folder_of(said: &str) -> String {
    let mut s = said.trim();
    let mut host = false;
    if let Some(rest) = s.strip_prefix("file://") {
        // file:///C:/x and file://host/x: the drive's path, or the host's
        match rest.strip_prefix('/').filter(|r| r.get(1..2) == Some(":")) {
            Some(drive) => s = drive,
            // file:///x, a path with no drive and no host: as it is
            None if rest.starts_with('/') => s = rest,
            None => {
                s = rest;
                host = !s.is_empty();
            }
        }
    }
    let bytes = s.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut raw = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                raw.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        raw.push(bytes[i]);
        i += 1;
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let trimmed = text.trim_end_matches(['\\', '/']);
    if host {
        return format!("\\\\{}", trimmed.replace('/', "\\"));
    }
    // A drive's root keeps its slash
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return format!("{trimmed}\\");
    }
    if trimmed.get(1..2) == Some(":") {
        return trimmed.replace('/', "\\");
    }
    trimmed.to_string()
}

// ---------------------------------------------------------------------------------- the picker, typed in

/// One row the picker lists
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub project: usize,
    /// Its group with nothing typed; none once something is (then the rows are what it names)
    pub group: Option<Group>,
    /// The folder it opens in, and its path past its project's, shown dim (Here's)
    pub dir: String,
    pub sub: Option<String>,
    pub repo: bool,
    pub indent: bool,
    /// How the box's first word found it, its letters to mark; none with nothing typed
    pub found: Option<Found>,
}

/// What the picker lists for what's typed: with nothing typed, the order (`order`) with a rule
/// between groups; typed, the projects its first word names, best first, with no rules, or a row
/// saying none is called that
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Listed {
    pub rows: Vec<Listing>,
    pub breaks: Vec<usize>,
    /// "No project called xyz", in the list's one row
    pub none: Option<String>,
}

pub fn listed(query: &str, names: &[&str], aliases: &[Vec<String>], entries: &[Entry]) -> Listed {
    if first_word(query).is_empty() {
        let groups: Vec<Group> = entries.iter().map(|e| e.group).collect();
        return Listed {
            rows: entries
                .iter()
                .map(|e| Listing {
                    project: e.project,
                    group: Some(e.group),
                    dir: e.dir.clone(),
                    sub: e.sub.clone(),
                    repo: e.repo,
                    indent: e.indent,
                    found: None,
                })
                .collect(),
            breaks: breaks(&groups),
            none: None,
        };
    }
    let base: Vec<usize> = entries.iter().map(|e| e.project).collect();
    let rows: Vec<Listing> = find(query, names, aliases, &base)
        .into_iter()
        .filter_map(|f| {
            let e = entries.iter().find(|e| e.project == f.project)?;
            Some(Listing {
                project: f.project,
                group: None,
                dir: e.dir.clone(),
                sub: e.sub.clone(),
                repo: e.repo,
                indent: false,
                found: Some(f),
            })
        })
        .collect();
    let none = rows.is_empty().then(|| no_match(query));
    Listed { rows, breaks: vec![], none }
}

/// A key the picker acts on (render/new_tab.rs maps the window's keys to these)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKey {
    /// Up the card, away from the box: the next row of the list
    Up,
    Down,
    PageUp,
    PageDown,
    Enter { shift: bool, ctrl: bool },
    /// The highlighted name into the box
    Tab,
    /// A digit, which picks a row only while the box is empty
    Digit(char),
    /// Alt+R and Alt+M
    Resume,
    Inmail,
    Escape,
}

/// What a key did
#[derive(Debug, Clone, PartialEq)]
pub enum PickerDid {
    /// Nothing: a digit that picks no row is a word (render/new_tab.rs types it)
    Nothing,
    /// The highlight, the scroll or the box changed: draw it again
    Changed,
    Close,
    /// Do `pick` in the project of the list's row `row`
    Pick { row: usize, pick: Pick },
    /// Enter with nothing named: a light round the box once (`Light::Pad`, render/new_tab.rs); the
    /// picker has a hairline but no lamp, not being one of the compositor's menu slots
    NoMatch,
}

/// The picker as it's typed in: what it opens, the box, the highlighted row (0 nearest the box)
/// and how far up the list the rows shown start
#[derive(Debug, Clone, PartialEq)]
pub struct PickerState {
    pub mode: Mode,
    pub input: crate::seat::input::Input,
    pub highlight: usize,
    pub scroll: usize,
    /// "Started <name>" under the box, after Shift+Enter started one behind, until the next edit
    pub started: Option<String>,
}

impl PickerState {
    pub fn new(mode: Mode) -> Self {
        Self { mode, input: Default::default(), highlight: 0, scroll: 0, started: None }
    }

    pub fn query(&self) -> &str {
        self.input.text()
    }

    /// The box was typed in: the first row is highlighted again (the best match)
    pub fn edited(&mut self) {
        self.highlight = 0;
        self.scroll = 0;
        self.started = None;
    }

    /// The highlight held inside a list of `rows`, and the scroll keeping it in the `shown` rows
    pub fn hold(&mut self, rows: usize, shown: usize) {
        self.highlight = self.highlight.min(rows.saturating_sub(1));
        self.scroll = follow(self.scroll, self.highlight, shown);
    }

    /// `key`, with `listed` what the list holds now, `names` the projects' names, `mail` a
    /// project's InMail waiting, and `shown` how many rows the card has room for
    pub fn key(&mut self, key: PickerKey, listed: &Listed, names: &[&str], mail: impl Fn(usize) -> usize, shown: usize) -> PickerDid {
        let rows = listed.rows.len();
        let moved = |s: &mut Self, by: isize| {
            if rows == 0 {
                return PickerDid::Nothing;
            }
            let to = step(s.highlight, by, rows);
            if to == s.highlight {
                return PickerDid::Nothing;
            }
            s.highlight = to;
            s.scroll = follow(s.scroll, to, shown);
            PickerDid::Changed
        };
        let page = shown.max(1) as isize;
        match key {
            PickerKey::Escape => PickerDid::Close,
            PickerKey::Up => moved(self, 1),
            PickerKey::Down => moved(self, -1),
            PickerKey::PageUp => moved(self, page),
            PickerKey::PageDown => moved(self, -page),
            PickerKey::Enter { shift, ctrl } => match rows {
                0 => PickerDid::NoMatch,
                _ => PickerDid::Pick { row: self.highlight.min(rows - 1), pick: enter(self.mode, shift, ctrl) },
            },
            PickerKey::Tab => {
                let Some(row) = listed.rows.get(self.highlight) else {
                    return PickerDid::Nothing;
                };
                let Some(name) = names.get(row.project) else {
                    return PickerDid::Nothing;
                };
                self.input.set(&complete(name));
                self.edited();
                PickerDid::Changed
            }
            PickerKey::Digit(c) => match digit_pick(self.input.is_empty(), c, rows) {
                Some(row) => PickerDid::Pick { row, pick: enter(self.mode, false, false) },
                None => PickerDid::Nothing,
            },
            PickerKey::Resume => match listed.rows.get(self.highlight) {
                Some(_) => PickerDid::Pick { row: self.highlight, pick: Pick::Resume },
                None => PickerDid::Nothing,
            },
            PickerKey::Inmail => match listed.rows.get(self.highlight) {
                Some(row) if mail(row.project) > 0 => PickerDid::Pick { row: self.highlight, pick: Pick::Inmail },
                _ => PickerDid::Nothing,
            },
        }
    }

    /// The first message for the highlighted row: the words after those naming it, in Claude's
    /// picker only (the shell's has none)
    pub fn message(&self, listed: &Listed, names: &[&str]) -> Option<String> {
        if self.mode != Mode::Claude {
            return None;
        }
        let row = listed.rows.get(self.highlight)?;
        let found = row.found.as_ref()?;
        let text = message(self.query(), names.get(row.project)?, found);
        (!text.is_empty()).then_some(text)
    }

    /// Started behind (Shift+Enter): the box cleared, the picker kept open, saying so
    pub fn started_behind(&mut self, name: &str) {
        self.input.set("");
        self.edited();
        self.started = Some(format!("Started {name}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 9] = ["Home", "Warden", "Bargain Hunter", "Terminal", "LBTerm", "BTD2 UI Help", "Trainer", "Musica", "War Room"];

    fn dirs() -> Vec<String> {
        [r"C:\AI", r"C:\AI\Warden", r"C:\AI\Bargain Hunter", r"C:\AI\Terminal", r"C:\AI\Terminal\code\LBTerm", r"C:\AI\BTD2 UI Help", r"C:\AI\Musica\code\Trainer", r"C:\AI\Musica", r"C:\AI\War Room"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn parents() -> Vec<Option<usize>> {
        vec![None, None, None, None, Some(3), None, Some(7), None, None]
    }

    fn top(query: &str) -> Option<&'static str> {
        let base: Vec<usize> = (0..NAMES.len()).collect();
        find(query, &NAMES, &[], &base).first().map(|f| NAMES[f.project])
    }

    #[test]
    fn a_word_finds_a_project_as_an_at_name_does() {
        assert_eq!(top("war"), Some("Warden"), "the start of a name, before another's word");
        assert_eq!(top("barg"), Some("Bargain Hunter"));
        assert_eq!(top("hunt"), Some("Bargain Hunter"), "the start of any word in the name");
        assert_eq!(top("lbt"), Some("LBTerm"), "a repo by its own name");
        assert_eq!(top("btd"), Some("BTD2 UI Help"));
        assert_eq!(top("@Warden"), Some("Warden"), "a leading @ dropped");
        assert_eq!(top("BARGAINHUNTER"), Some("Bargain Hunter"), "any case, spaces optional");
        assert_eq!(top("warroom"), Some("War Room"), "the whole name first");
        assert_eq!(top("erm"), Some("Terminal"), "then anywhere in it");
        assert_eq!(top("xyz"), None);
        assert_eq!(top(""), None);
        // The whole name beats a start, a start a word's, a word's inside
        let base: Vec<usize> = (0..NAMES.len()).collect();
        let tiers: Vec<Tier> = find("war", &NAMES, &[], &base).iter().map(|f| f.tier).collect();
        assert_eq!(tiers, vec![Tier::Start, Tier::Start], "Warden and War Room, in the picker's order");
        assert_eq!(find("room", &NAMES, &[], &base)[0].tier, Tier::Word);
    }

    #[test]
    fn the_matched_letters_are_marked() {
        let (_, marks, _) = rank("barg", "Bargain Hunter").unwrap();
        assert_eq!(marks, vec![0, 1, 2, 3]);
        let (_, marks, _) = rank("hunt", "Bargain Hunter").unwrap();
        assert_eq!(marks, vec![8, 9, 10, 11]);
        let (_, marks, _) = rank("bargainh", "Bargain Hunter").unwrap();
        assert_eq!(marks, vec![0, 1, 2, 3, 4, 5, 6, 8], "past the space");
    }

    #[test]
    fn the_words_after_the_name_are_the_first_message() {
        let found = |q| {
            let base: Vec<usize> = (0..NAMES.len()).collect();
            let f = find(q, &NAMES, &[], &base).into_iter().next().unwrap();
            (NAMES[f.project], message(q, NAMES[f.project], &f))
        };
        assert_eq!(found("bargain hunter find a kettle"), ("Bargain Hunter", "find a kettle".to_string()));
        assert_eq!(found("bargain hunt for kettles"), ("Bargain Hunter", "hunt for kettles".to_string()));
        assert_eq!(found("war fix the queue"), ("Warden", "fix the queue".to_string()));
        assert_eq!(found("war"), ("Warden", String::new()));
        assert_eq!(found("bargainhunter  two  spaces "), ("Bargain Hunter", "two  spaces".to_string()), "the message as typed, trimmed");
        assert_eq!(found("btd2 ui help me"), ("BTD2 UI Help", "me".to_string()));
        assert_eq!(found("btd2 ui place a tower"), ("BTD2 UI Help", "place a tower".to_string()));
    }

    #[test]
    fn digits_pick_only_with_an_empty_box() {
        assert_eq!(digit_pick(true, '1', 5), Some(0));
        assert_eq!(digit_pick(true, '5', 5), Some(4));
        assert_eq!(digit_pick(true, '6', 5), None, "no sixth row");
        assert_eq!(digit_pick(true, '0', 5), None);
        assert_eq!(digit_pick(false, '2', 5), None, "once it isn't empty, digits are words (BTD2)");
        assert_eq!(digit_pick(true, 'x', 5), None);
    }

    #[test]
    fn tab_completes_and_no_match_says_so() {
        assert_eq!(complete("Bargain Hunter"), "Bargain Hunter ");
        assert_eq!(no_match("xyz find it"), "No project called xyz");
    }

    #[test]
    fn home_is_the_project_holding_every_other() {
        assert_eq!(home(&dirs()), Some(0));
        assert_eq!(home(&dirs()[1..]), None);
        assert_eq!(home(&[]), None);
    }

    #[test]
    fn here_then_home_then_recent_then_a_to_z_each_once() {
        let names = NAMES;
        let recent = vec!["Musica".to_string(), "warden".to_string(), "Terminal".to_string(), "Nowhere".to_string()];
        let o = order(&names, &dirs(), &parents(), Some(r"c:\ai\terminal\shared\claude\"), &recent);
        let shown: Vec<(&str, Group)> = o.iter().map(|e| (names[e.project], e.group)).collect();
        assert_eq!(
            shown,
            vec![
                ("Terminal", Group::Here),
                ("Home", Group::Home),
                ("Musica", Group::Recent),
                ("Warden", Group::Recent),
                ("Bargain Hunter", Group::All),
                ("BTD2 UI Help", Group::All),
                ("LBTerm", Group::All),
                ("Trainer", Group::All),
                ("War Room", Group::All),
            ]
        );
        // Here opens where the tab is, its path past the project's folder shown dim
        assert_eq!(o[0].dir, r"c:\ai\terminal\shared\claude");
        assert_eq!(o[0].sub.as_deref(), Some("shared/claude"));
        // Each project once
        let mut ps: Vec<usize> = o.iter().map(|e| e.project).collect();
        ps.sort();
        ps.dedup();
        assert_eq!(ps.len(), o.len());
        assert_eq!(ps.len(), names.len());
        // A repo whose project is listed with it goes under it, indented
        let o = order(&names, &dirs(), &parents(), None, &[]);
        let shown: Vec<(&str, bool)> = o.iter().map(|e| (names[e.project], e.indent)).collect();
        assert_eq!(
            shown,
            vec![
                ("Home", false),
                ("Bargain Hunter", false),
                ("BTD2 UI Help", false),
                ("Musica", false),
                ("Trainer", true),
                ("Terminal", false),
                ("LBTerm", true),
                ("War Room", false),
                ("Warden", false),
            ]
        );
        assert!(o.iter().find(|e| names[e.project] == "LBTerm").unwrap().repo);
        // Here at a project's own folder: no path; and Home isn't listed twice
        let o = order(&names, &dirs(), &parents(), Some(r"C:\AI"), &[]);
        assert_eq!((names[o[0].project], o[0].group, o[0].sub.clone()), ("Home", Group::Here, None));
        assert_eq!(o.iter().filter(|e| names[e.project] == "Home").count(), 1);
        // A folder in no project: no Here
        let o = order(&names, &dirs(), &parents(), Some(r"D:\elsewhere"), &[]);
        assert_eq!(o[0].group, Group::Home);
    }

    #[test]
    fn sessions_and_mail_never_move_a_row() {
        // The order takes no counts at all: it's the same whatever runs
        let a = order(&NAMES, &dirs(), &parents(), Some(r"C:\AI\Warden"), &["Musica".to_string()]);
        let b = order(&NAMES, &dirs(), &parents(), Some(r"C:\AI\Warden"), &["Musica".to_string()]);
        assert_eq!(a, b);
        assert_eq!(breaks(&a.iter().map(|e| e.group).collect::<Vec<_>>()), vec![1, 2, 3]);
    }

    #[test]
    fn the_last_six_picks_are_kept_newest_first() {
        let mut r = vec![];
        for name in ["A", "B", "C", "D", "E", "F", "G"] {
            remember(&mut r, name);
        }
        assert_eq!(r, vec!["G", "F", "E", "D", "C", "B"]);
        remember(&mut r, "d");
        assert_eq!(r, vec!["d", "G", "F", "E", "C", "B"], "picked again, it moves up, once");
        assert_eq!(read_recent(&recent_text(&r)), r);
        assert_eq!(read_recent("not json"), Vec::<String>::new());
        assert_eq!(read_recent(r#"["a","b","c","d","e","f","g","h"]"#).len(), RECENT);
    }

    #[test]
    fn the_recent_file_is_written_through_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deep").join("recent.json");
        write_through_rename(&file, r#"["Warden"]"#).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), r#"["Warden"]"#);
        assert!(!crate::state::tmp_path(&file).exists());
    }

    #[test]
    fn enter_with_shift_and_ctrl() {
        assert_eq!(enter(Mode::Claude, false, false), Pick::New { behind: false });
        assert_eq!(enter(Mode::Claude, true, false), Pick::New { behind: true });
        assert_eq!(enter(Mode::Claude, false, true), Pick::Shell);
        assert_eq!(enter(Mode::Shell, false, false), Pick::Shell);
        assert_eq!(enter(Mode::Shell, false, true), Pick::New { behind: false });
        assert_eq!(Pick::New { behind: true }.action(), "new");
        assert_eq!(Pick::Shell.action(), "shell");
    }

    #[test]
    fn the_picker_grows_up_from_the_panels_foot() {
        // Lachlan's window: 1880 tall, the panel's bottom at 1876, past the rail at 76
        let l = picker_layout(76.0, 1876.0, 1880.0, 1.0, 16.0, 70, &[1, 2, 8], 0, false);
        assert_eq!((l.card.x, l.card.w), (76.0, 360.0));
        assert_eq!(l.card.y + l.card.h, 1876.0, "its bottom on the panel's");
        assert_eq!(l.input.h, 36.0);
        assert!(l.footer.y >= l.input.y + l.input.h, "the footer under the box");
        assert_eq!(l.shown(), PICKER_ROWS, "16 rows show");
        // Row 1 nearest the box, the next up
        assert!(l.rows[0].1.y + l.rows[0].1.h <= l.input.y);
        assert!(l.rows[1].1.y < l.rows[0].1.y);
        assert_eq!(l.rows[0].0, 0);
        assert_eq!(l.rows[0].1.h, 28.0);
        // A rule before each group's first row
        assert_eq!(l.rules.len(), 3);
        assert!(l.rules[0].y < l.rows[0].1.y && l.rules[0].y > l.rows[1].1.y + l.rows[1].1.h - 1.0);
        // At most 70% of the window's height
        assert!(l.card.h <= 0.7 * 1880.0 + 1.0);
        let small = picker_layout(76.0, 476.0, 480.0, 1.0, 16.0, 70, &[], 0, false);
        assert!(small.card.h <= 0.7 * 480.0 + 1.0, "{}", small.card.h);
        assert!(small.shown() < PICKER_ROWS);
        // Scrolled: held inside the list
        let end = picker_layout(76.0, 1876.0, 1880.0, 1.0, 16.0, 20, &[], 99, false);
        assert_eq!(end.scroll, 4);
        assert_eq!(end.rows.last().unwrap().0, 19);
        // A first message's line over the footer, the box over that
        let m = picker_layout(76.0, 1876.0, 1880.0, 1.0, 16.0, 5, &[], 0, true);
        let line = m.message.unwrap();
        assert!(line.y >= m.input.y + m.input.h && line.y + line.h <= m.footer.y);
        // Nothing listed still has its row, for "No project called …"
        assert_eq!(picker_layout(76.0, 1876.0, 1880.0, 1.0, 16.0, 0, &[], 0, false).shown(), 1);
        // Key chips on the first nine
        let r = l.rows[0].1;
        assert!(l.key_chip(&r, 8).is_some() && l.key_chip(&r, 9).is_none());
        assert!(l.name_x(&r) > l.key_chip(&r, 0).unwrap().x + 16.0);
    }

    #[test]
    fn the_highlights_chips_and_steps() {
        let row = Rect { x: 82.0, y: 100.0, w: 348.0, h: 28.0 };
        let c = chips(&row, 3, 7.0, 1.0);
        assert_eq!(c.iter().map(|(c, _)| *c).collect::<Vec<_>>(), vec![Chip::Resume, Chip::Inmail]);
        assert!(c[1].1.x + c[1].1.w <= row.x + row.w && c[0].1.x + c[0].1.w < c[1].1.x);
        assert_eq!(Chip::Inmail.label(3), "InMail 3");
        assert_eq!(chips(&row, 0, 7.0, 1.0).len(), 1, "InMail only with mail");
        assert_eq!(step(0, 1, 5), 1);
        assert_eq!(step(4, 1, 5), 4);
        assert_eq!(step(0, -1, 5), 0);
        assert_eq!(follow(0, 20, 16), 5);
        assert_eq!(follow(5, 2, 16), 2);
        assert_eq!(follow(5, 10, 16), 5);
    }

    #[test]
    fn the_plus_menu_from_a_launch_menu_of_four_and_of_none() {
        let labels = ["PowerShell 7", "Windows PowerShell 5.1", "Command Prompt", "Git Bash"];
        let keys = launch_keys(&labels);
        assert_eq!(keys, vec![Some('P'), Some('W'), Some('C'), Some('G')]);
        let rows = plus_rows(4, 2);
        assert_eq!(
            rows,
            vec![
                Some(PlusItem::Launch(0)),
                Some(PlusItem::Launch(1)),
                Some(PlusItem::Launch(2)),
                Some(PlusItem::Launch(3)),
                Some(PlusItem::ShellIn),
                None,
                Some(PlusItem::Reopen(0)),
                Some(PlusItem::Reopen(1)),
                None,
                Some(PlusItem::NewWindow),
            ]
        );
        // Every key unique
        let taken: Vec<char> = rows.iter().flatten().filter_map(|i| plus_key(*i, &keys)).collect();
        let mut unique = taken.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(taken.len(), unique.len(), "{taken:?}");
        assert_eq!(taken, vec!['P', 'W', 'C', 'G', 'I', 'R', 'N']);
        assert_eq!(plus_by_key(&rows, &keys, 'g'), Some(PlusItem::Launch(3)));
        assert_eq!(plus_by_key(&rows, &keys, 'r'), Some(PlusItem::Reopen(0)));
        assert_eq!(plus_by_key(&rows, &keys, 'z'), None);
        // The shortcuts are the keys the key table binds, and none where nothing is
        let bound = KeyWords { picker: Some("Ctrl+T".into()), shell: Some("Ctrl+Alt+T".into()), reopen: Some("Ctrl+Shift+T".into()) };
        assert_eq!(plus_hint(PlusItem::Launch(0), &bound), Some("Ctrl+Alt+T"));
        assert_eq!(plus_hint(PlusItem::Reopen(0), &bound), Some("Ctrl+Shift+T"));
        assert_eq!(plus_hint(PlusItem::Launch(1), &bound), None);
        assert_eq!(plus_hint(PlusItem::Launch(0), &KeyWords::default()), None, "nothing bound: no hint");
        assert_eq!(plus_hint(PlusItem::Reopen(0), &KeyWords::default()), None);
        // None in the launch menu, nothing closed
        let rows = plus_rows(0, 0);
        assert_eq!(rows, vec![Some(PlusItem::ShellIn), None, Some(PlusItem::NothingClosed), None, Some(PlusItem::NewWindow)]);
        // Five Reopen rows at most
        assert_eq!(plus_rows(1, 9).iter().filter(|r| matches!(r, Some(PlusItem::Reopen(_)))).count(), REOPEN_ROWS);
        // Letters that clash or that the menu's own rows take get no key
        assert_eq!(launch_keys(&["PowerShell", "pwsh", "Irb", "Nu", "Bash"]), vec![None, None, None, None, Some('B')]);
    }

    /// Lachlan's 5a of 2026-11-08: a key the open + menu shows beside a row picks that row, and
    /// every other bound key stays a key no row takes (his 7b; lbterm/PITFALLS.md, 185)
    #[test]
    fn a_key_the_plus_menu_shows_beside_a_row_picks_that_row_and_no_other_key_does() {
        let bound = KeyWords { picker: Some("Ctrl+T".into()), shell: Some("Ctrl+Alt+T".into()), reopen: Some("Ctrl+Shift+T".into()) };
        let rows = plus_rows(2, 2);
        assert_eq!(plus_by_shortcut(&rows, &bound, Hinted::Shell, "Ctrl+Alt+T"), Some(PlusItem::Launch(0)));
        assert_eq!(plus_by_shortcut(&rows, &bound, Hinted::Reopen, "Ctrl+Shift+T"), Some(PlusItem::Reopen(0)), "the newest closed tab's row, not the older one's");
        // A second key bound to the same action isn't the one the menu shows
        assert_eq!(plus_by_shortcut(&rows, &bound, Hinted::Shell, "F6"), None);
        assert_eq!(plus_by_shortcut(&rows, &bound, Hinted::Reopen, "Ctrl+Alt+T"), None, "the shell's key named, but bound to Reopen");
        // Nothing closed: Reopen's row is "Nothing closed yet", which shows no key
        let empty = plus_rows(2, 0);
        assert!(empty.contains(&Some(PlusItem::NothingClosed)));
        assert_eq!(plus_by_shortcut(&empty, &bound, Hinted::Reopen, "Ctrl+Shift+T"), None);
        // No launch menu: no row shows the shell's key
        assert_eq!(plus_by_shortcut(&plus_rows(0, 1), &bound, Hinted::Shell, "Ctrl+Alt+T"), None);
        assert_eq!(plus_by_shortcut(&plus_rows(0, 1), &bound, Hinted::Reopen, "Ctrl+Shift+T"), Some(PlusItem::Reopen(0)));
        // Nothing bound: nothing shown, nothing picked
        assert_eq!(plus_by_shortcut(&rows, &KeyWords::default(), Hinted::Shell, "Ctrl+Alt+T"), None);
        // Each row a key picks is a row that shows it
        for item in rows.iter().flatten() {
            if let Some(words) = plus_hint(*item, &bound) {
                let action = plus_hinted(*item).expect("a row with a hint is a hinted row");
                assert_eq!(plus_by_shortcut(&rows, &bound, action, words), Some(*item));
            }
        }
    }

    /// Lachlan's 4b of 2026-11-08: Ctrl+V alone pastes into the picker's box
    #[test]
    fn only_ctrl_v_pastes_into_the_picker() {
        assert!(pastes_into_picker(&key_words(true, false, false, false, "V")));
        for other in [
            key_words(true, false, true, false, "V"),
            key_words(false, false, true, false, "Insert"),
            key_words(true, true, false, false, "V"),
            key_words(false, false, false, false, "V"),
        ] {
            assert!(!pastes_into_picker(&other), "{other}");
        }
    }

    /// Lachlan's 11a of 2026-11-08: the picker's box holds one line, so a paste is the clipboard's
    /// first line with something in it, trimmed
    #[test]
    fn a_paste_into_the_picker_is_its_first_line_with_words_trimmed() {
        assert_eq!(pasted("Terminal"), "Terminal");
        assert_eq!(pasted("Terminal\r\n"), "Terminal", "a line copied with its line break");
        assert_eq!(pasted("  bargain hunter  \nfind a kettle"), "bargain hunter");
        assert_eq!(pasted("\r\n\n  \nWarden\nsecond"), "Warden", "blank lines first");
        assert_eq!(pasted(""), "");
        assert_eq!(pasted(" \r\n\t\n"), "");
        assert_eq!(pasted("a\tb"), "a\tb", "a tab inside is the box's to make a space");
    }

    /// The box's span, by `box_span`, starts where a walk from the line's start would and ends
    /// once it covers the box, and what it measures and hands the paint is bounded by what the box
    /// holds, never by the line's length, with the caret anywhere in a long line: at its end
    /// straight after a paste, part way in after Left presses, at its start after Home
    /// (lbterm/PITFALLS.md, 192 and 209)
    #[test]
    fn the_box_measures_and_draws_only_what_it_holds_wherever_the_caret_is_in_a_long_line() {
        // What a walk from the line's start gives: the earliest start that fits up to the caret,
        // and the line cut where it first passes the box
        let brute = |len: usize, caret: usize, width: &dyn Fn(usize, usize) -> usize, room: usize| {
            let from = (0..=caret).find(|&f| width(f, caret) <= room).unwrap();
            let to = (caret..=len).find(|&t| t > caret && width(from, t) > room).unwrap_or(len);
            (from, to)
        };
        // Each character a unit wide, a box of 10, a pasted line of 5,000
        let room = 10;
        let unit = |from: usize, to: usize| to - from;
        let len = 5000;
        for caret in [0, 1, 9, 10, 11, 25, 2500, 4990, 4999, 5000] {
            let (mut measures, mut measured) = (0, 0);
            let (from, to) = box_span(caret, len, |from, to| {
                measures += 1;
                measured += to - from;
                unit(from, to) <= room
            });
            assert_eq!((from, to), brute(len, caret, &unit, room), "caret at {caret}");
            assert!(to - from <= 2 * room + 2, "caret at {caret}: {} characters handed to the paint", to - from);
            assert!(measures <= 12, "caret at {caret}: {measures} measures for a box of {room}");
            assert!(measured <= 12 * (2 * room + 2), "caret at {caret}: {measured} characters measured");
        }
        // A line that fits, the caret at its end as typing leaves it: short suffixes, the longest
        // the line, and nothing past the caret
        let mut spans = vec![];
        let (from, to) = box_span(7, 7, |from, to| {
            spans.push((from, to));
            unit(from, to) <= room
        });
        assert_eq!((from, to), (0, 7));
        assert_eq!(spans, vec![(6, 7), (5, 7), (3, 7), (0, 7)]);
        // Wide and narrow characters, and a box narrower than one: the same as the walk
        let widths = [1, 3, 1, 1, 3, 1, 1, 1, 3, 1, 2, 2, 1, 3, 1];
        let width = |from: usize, to: usize| widths[from..to].iter().sum::<usize>();
        for room in [2, 5, 8, 30] {
            for caret in 0..=widths.len() {
                let got = box_span(caret, widths.len(), |from, to| width(from, to) <= room);
                assert_eq!(got, brute(widths.len(), caret, &width, room), "room {room}, caret at {caret}");
            }
        }
        let got = box_span(4, 15, |from, to| width(from, to) == 0);
        assert_eq!(got, (4, 5), "nothing fits: the caret at the box's start, its character cut");
    }

    #[test]
    fn the_plus_menu_opens_upward_from_the_pointer() {
        let rows = plus_rows(4, 1);
        let l = plus_layout((40.0, 1860.0), (1080.0, 1880.0), 1.0, &rows);
        assert_eq!(l.card.y + l.card.h, 1860.0, "up from the pointer at the window's foot");
        assert_eq!((l.card.x, l.card.w), (40.0, 260.0));
        let ps = l.rows.iter().find(|(r, _)| *r == Some(PlusItem::Launch(0))).unwrap().1;
        assert_eq!(ps.h, 30.0);
        assert_eq!(l.hit(ps.x + 5.0, ps.y + 5.0), Some(PlusItem::Launch(0)));
        let reopen = l.rows.iter().find(|(r, _)| *r == Some(PlusItem::Reopen(0))).unwrap().1;
        assert_eq!(reopen.h, 42.0, "two lines");
        // Near the top it hangs down instead
        let l = plus_layout((40.0, 20.0), (1080.0, 1880.0), 1.0, &rows);
        assert!(l.card.y >= 6.0);
        // Nothing closed: its row does nothing
        let rows = plus_rows(1, 0);
        let l = plus_layout((40.0, 1860.0), (1080.0, 1880.0), 1.0, &rows);
        let dim = l.rows.iter().find(|(r, _)| *r == Some(PlusItem::NothingClosed)).unwrap().1;
        assert_eq!(l.hit(dim.x + 5.0, dim.y + 5.0), None);
        // The keys step over it, round the ends
        assert_eq!(plus_step(&rows, None, 1), Some(PlusItem::Launch(0)));
        assert_eq!(plus_step(&rows, Some(PlusItem::ShellIn), 1), Some(PlusItem::NewWindow));
        assert_eq!(plus_step(&rows, Some(PlusItem::NewWindow), 1), Some(PlusItem::Launch(0)));
        assert_eq!(plus_step(&rows, None, -1), Some(PlusItem::NewWindow));
    }

    #[test]
    fn closed_tabs_are_kept_newest_first_ten_at_most() {
        let tab = |i: usize| Closed {
            sessions: vec![format!("s{i}")],
            label: format!("Tab {i}"),
            dir: format!(r"C:\AI\P{i}"),
            project: format!("P{i}"),
            at: 1000.0 + i as f64,
        };
        let mut list = vec![];
        for i in 0..12 {
            keep_closed(&mut list, tab(i));
        }
        assert_eq!(list.len(), CLOSED_KEPT);
        assert_eq!(list[0].label, "Tab 11");
        assert_eq!(list[9].label, "Tab 2");
        let (first, second) = reopen_words(&list[0], 1011.0 + 300.0);
        assert_eq!(first, "Reopen Tab 11");
        assert_eq!(second, "5 min \u{b7} P11");
        assert_eq!(age(0.0, 30.0), "now");
        assert_eq!(age(0.0, 7300.0), "2 h");
        assert_eq!(age(0.0, 200000.0), "2 d");
        assert_eq!(nothing_opened("Opening Claude in Warden\u{2026}"), "Nothing opened for Opening Claude in Warden");
    }

    #[test]
    fn a_closed_tab_keeps_its_sessions_once_and_its_sessions_folder_before_its_panes() {
        let sessions = vec!["a".to_string(), " ".to_string(), "b".to_string(), "a".to_string()];
        let c = closed(sessions, "  Rail fix ", r"C:\AI\Warden\", "file:///C:/AI/Other", "Warden".into(), 12.5);
        assert_eq!(c.sessions, vec!["a", "b"], "the active pane's first, each once, none empty");
        assert_eq!(c.label, "Rail fix");
        assert_eq!(c.dir, r"C:\AI\Warden", "its session's folder, as cwd.txt says it");
        assert_eq!((c.project.as_str(), c.at), ("Warden", 12.5));
        // A shell's tab: no session, its pane's folder
        let c = closed(vec![], "pwsh", "", "file:///C:/AI/Bargain%20Hunter/", String::new(), 0.0);
        assert!(c.sessions.is_empty());
        assert_eq!(c.dir, r"C:\AI\Bargain Hunter", "in Windows' spelling");
        // Nothing said where it was: no folder, and Reopen opens where the default program does
        assert_eq!(closed(vec![], "", "", "", String::new(), 0.0).dir, "");
    }

    #[test]
    fn the_starting_row_waits_for_the_first_new_tab_or_its_time() {
        let t0 = Instant::now();
        let s = Starting { id: 1, label: "Warden".into(), since: t0, tabs: vec![1, 4, 7], behind: false };
        assert_eq!(starting_now(&s, t0, &[1, 4, 7]), StartingNow::Waiting(t0 + STARTING_FOR));
        // A tab closing while it waits isn't an arrival
        assert_eq!(starting_now(&s, t0 + Duration::from_secs(3), &[1, 7]), StartingNow::Waiting(t0 + STARTING_FOR));
        // The first tab the window didn't have, wherever it lands in the window's order
        assert_eq!(starting_now(&s, t0 + Duration::from_secs(1), &[1, 9, 4, 7, 12]), StartingNow::Arrived(9));
        // Its time: never before it, however late the clock is read (a loaded PC reads it late)
        assert_eq!(starting_now(&s, t0 + STARTING_FOR - Duration::from_millis(1), &[1, 4, 7]), StartingNow::Waiting(t0 + STARTING_FOR));
        assert_eq!(starting_now(&s, t0 + STARTING_FOR, &[1, 4, 7]), StartingNow::Expired);
        assert_eq!(starting_now(&s, t0 + STARTING_FOR * 3, &[1, 4, 7]), StartingNow::Expired);
        // A tab come by the time it's looked at counts, however late the look: it's what the row
        // was for
        assert_eq!(starting_now(&s, t0 + STARTING_FOR * 3, &[1, 4, 7, 8]), StartingNow::Arrived(8));
        assert_eq!(STARTING_FOR, Duration::from_secs(20));
    }

    #[test]
    fn two_starts_in_a_row_each_get_their_own_tab_in_order() {
        // Shift+Enter twice before the first tab has come: the second's snapshot of the window
        // lacks the first's tab, which mustn't count as the second's (PITFALLS 120)
        let t0 = Instant::now();
        let mut starts = Starts::default();
        starts.ask(Starting { id: 1, label: "Warden".into(), since: t0, tabs: vec![1, 4], behind: true });
        starts.ask(Starting { id: 2, label: "Musica".into(), since: t0 + Duration::from_secs(1), tabs: vec![1, 4], behind: true });
        assert_eq!(starts.showing().map(|s| s.id), Some(1), "the oldest's row shows");
        let (came, due) = starts.look(t0 + Duration::from_secs(2), &[1, 4]);
        assert!(came.is_empty());
        assert_eq!(due, Some(t0 + STARTING_FOR), "the oldest's time first");
        // The first tab comes: the first start's, and the second waits on
        let (came, due) = starts.look(t0 + Duration::from_secs(3), &[1, 4, 9]);
        assert_eq!(came.len(), 1);
        assert!(matches!(&came[0], Came::Arrived { start, tab: 9 } if start.id == 1));
        assert_eq!(starts.showing().map(|s| s.id), Some(2), "then the second's row shows");
        assert_eq!(due, Some(t0 + Duration::from_secs(1) + STARTING_FOR));
        // Looked at again with only the first's tab: still nothing for the second
        let (came, _) = starts.look(t0 + Duration::from_secs(4), &[1, 4, 9]);
        assert!(came.is_empty(), "the first's tab is never the second's");
        let (came, due) = starts.look(t0 + Duration::from_secs(5), &[1, 4, 9, 12]);
        assert!(matches!(&came[0], Came::Arrived { start, tab: 12 } if start.id == 2));
        assert!(starts.is_empty());
        assert_eq!(due, None);
        // Both come before anyone looks: each its own, in the window's order
        let mut starts = Starts::default();
        starts.ask(Starting { id: 3, label: "A".into(), since: t0, tabs: vec![1], behind: false });
        starts.ask(Starting { id: 4, label: "B".into(), since: t0, tabs: vec![1], behind: false });
        let (came, _) = starts.look(t0 + Duration::from_secs(1), &[1, 5, 6]);
        let got: Vec<(u64, usize)> = came
            .iter()
            .map(|c| match c {
                Came::Arrived { start, tab } => (start.id, *tab),
                Came::Expired(s) => (s.id, usize::MAX),
            })
            .collect();
        assert_eq!(got, vec![(3, 5), (4, 6)]);
        // Each expires on its own time, never early, and the newest is what Lua takes back
        let mut starts = Starts::default();
        starts.ask(Starting { id: 5, label: "A".into(), since: t0, tabs: vec![1], behind: false });
        starts.ask(Starting { id: 6, label: "B".into(), since: t0 + Duration::from_secs(5), tabs: vec![1], behind: false });
        let (came, _) = starts.look(t0 + STARTING_FOR - Duration::from_millis(1), &[1]);
        assert!(came.is_empty());
        let (came, due) = starts.look(t0 + STARTING_FOR, &[1]);
        assert!(matches!(&came[0], Came::Expired(s) if s.id == 5));
        assert_eq!(due, Some(t0 + Duration::from_secs(5) + STARTING_FOR));
        starts.ask(Starting { id: 7, label: "C".into(), since: t0 + STARTING_FOR, tabs: vec![1], behind: false });
        assert_eq!(starts.take_back().map(|s| s.id), Some(7));
        assert_eq!(starts.take(6).map(|s| s.id), Some(6));
        assert_eq!(starts.take(6), None);
        assert!(starts.is_empty());
    }

    #[test]
    fn a_starting_rows_label_is_what_opens() {
        assert_eq!(starting_label("Opening Claude in Warden"), "Warden");
        assert_eq!(starting_label("Opening Claude in Bargain Hunter\u{2026}"), "Bargain Hunter");
        assert_eq!(starting_label("Opening Chronos's InMail"), "Chronos's InMail");
        assert_eq!(starting_label("Reopening Rail fix"), "Rail fix");
        assert_eq!(starting_label("PowerShell 7"), "PowerShell 7");
        assert_eq!(nothing_opened(&starting_label("Opening Claude in Warden")), "Nothing opened for Warden");
    }

    #[test]
    fn a_folder_is_a_path_in_windows_spelling_whatever_says_it() {
        // A pane's URL, a statusline's path and a path with forward slashes: one spelling, the
        // config's, so a handler comparing folders as text finds them (PITFALLS 115)
        assert_eq!(folder_of("file:///C:/Users/me/AI/Bargain%20Hunter/"), r"C:\Users\me\AI\Bargain Hunter");
        assert_eq!(folder_of(r"C:\AI\Warden\"), r"C:\AI\Warden");
        assert_eq!(folder_of("  C:/AI  "), r"C:\AI");
        assert_eq!(folder_of("c:/ai/warden/src"), r"c:\ai\warden\src", "its case kept");
        assert_eq!(folder_of("file:///C:/"), "C:\\");
        assert_eq!(folder_of("file://server/share/x/"), r"\\server\share\x", "a host's share");
        assert_eq!(folder_of(""), "");
        // And the order's Here takes it as it is: its case kept, its path past the project's
        let o = order(&NAMES, &dirs(), &parents(), Some(&folder_of("file:///C:/AI/Warden/src/")), &[]);
        assert_eq!(o[0].group, Group::Here);
        assert_eq!(o[0].dir, r"C:\AI\Warden\src");
        assert_eq!(o[0].sub.as_deref(), Some("src"));
        // Typed, its own project's row opens there too: the Here row is that project's entry
        let l = listed("warden", &NAMES, &[], &o);
        assert_eq!((NAMES[l.rows[0].project], l.rows[0].dir.as_str()), ("Warden", r"C:\AI\Warden\src"));
    }

    #[test]
    fn the_roots_other_names_find_it_whole_ahead_of_any_other_match() {
        // Blueprint's resolve takes Root, AI and ~ for the root (INDEX_NAMES, and at-names' ~),
        // never by a prefix; "ai" is inside Bargain Hunter and Trainer, which it found before
        let names = ["Root", "Bargain Hunter", "Trainer", "Warden", "Atlas"];
        let aliases = vec![vec!["AI".to_string(), "~".to_string()]];
        let base: Vec<usize> = (0..names.len()).collect();
        let top = |q: &str| find(q, &names, &aliases, &base).first().map(|f| (names[f.project], f.tier));
        assert_eq!(top("ai"), Some(("Root", Tier::Whole)));
        assert_eq!(top("AI"), Some(("Root", Tier::Whole)));
        assert_eq!(top("@ai"), Some(("Root", Tier::Whole)));
        assert_eq!(top("~"), Some(("Root", Tier::Whole)));
        assert_eq!(top("root"), Some(("Root", Tier::Whole)));
        assert_eq!(top("ai fix the home page"), Some(("Root", Tier::Whole)));
        // Only whole: a's prefix is Atlas's, not the root's by its alias
        assert_eq!(top("a"), Some(("Atlas", Tier::Start)));
        // The others still come after it
        let all: Vec<&str> = find("ai", &names, &aliases, &base).iter().map(|f| names[f.project]).collect();
        assert_eq!(all, vec!["Root", "Bargain Hunter", "Trainer"]);
        // The words after an alias are the first message, and no letter of the name is marked
        let f = find("ai fix the home page", &names, &aliases, &base).remove(0);
        assert!(f.marks.is_empty());
        assert_eq!(message("ai fix the home page", "Root", &f), "fix the home page");
        // Without aliases it's as before
        assert_eq!(find("ai", &names, &[], &base).first().map(|f| names[f.project]), Some("Bargain Hunter"));
    }

    #[test]
    fn a_key_is_named_as_windows_names_it() {
        assert_eq!(key_words(true, false, false, false, "T"), "Ctrl+T");
        assert_eq!(key_words(true, true, false, false, "T"), "Ctrl+Alt+T");
        assert_eq!(key_words(true, false, true, false, "T"), "Ctrl+Shift+T");
        assert_eq!(key_words(false, false, false, false, "F7"), "F7");
    }

    fn entries() -> Vec<Entry> {
        order(&NAMES, &dirs(), &parents(), Some(r"C:\AI\Terminal"), &["Musica".to_string()])
    }

    #[test]
    fn the_list_with_nothing_typed_is_the_order_with_its_rules() {
        let e = entries();
        let l = listed("", &NAMES, &[], &e);
        let names: Vec<&str> = l.rows.iter().map(|r| NAMES[r.project]).collect();
        assert_eq!(names[..3], ["Terminal", "Home", "Musica"]);
        assert_eq!(l.breaks, vec![1, 2, 3], "a rule between Here, Home, the recent picks and the rest");
        assert!(l.rows.iter().all(|r| r.found.is_none()));
        assert_eq!(l.none, None);
        // Spaces alone are nothing typed
        assert_eq!(listed("   ", &NAMES, &[], &e).rows.len(), NAMES.len());
    }

    #[test]
    fn typed_the_list_is_what_the_word_names_best_first() {
        let e = entries();
        let l = listed("war", &NAMES, &[], &e);
        let names: Vec<&str> = l.rows.iter().map(|r| NAMES[r.project]).collect();
        assert_eq!(names, vec!["War Room", "Warden"], "both start with it: the picker's order, A to Z, between them");
        assert!(l.breaks.is_empty(), "no rules among matches");
        assert_eq!(l.rows[0].found.as_ref().unwrap().marks, vec![0, 1, 2]);
        // Here's row keeps where it opens
        let l = listed("term", &NAMES, &[], &e);
        assert_eq!(NAMES[l.rows[0].project], "Terminal");
        assert_eq!(l.rows[0].dir, r"C:\AI\Terminal");
        let l = listed("xyz find it", &NAMES, &[], &e);
        assert!(l.rows.is_empty());
        assert_eq!(l.none.as_deref(), Some("No project called xyz"));
    }

    #[test]
    fn the_pickers_keys() {
        let e = entries();
        let none = |_: usize| 0;
        let mut p = PickerState::new(Mode::Claude);
        let l = listed("", &NAMES, &[], &e);
        // Up goes away from the box, held at the ends; Down back
        assert_eq!(p.key(PickerKey::Down, &l, &NAMES, none, 4), PickerDid::Nothing, "already nearest the box");
        assert_eq!(p.key(PickerKey::Up, &l, &NAMES, none, 4), PickerDid::Changed);
        assert_eq!(p.highlight, 1);
        assert_eq!(p.key(PickerKey::PageUp, &l, &NAMES, none, 4), PickerDid::Changed);
        assert_eq!((p.highlight, p.scroll), (5, 2), "the scroll keeps it in the four shown");
        assert_eq!(p.key(PickerKey::PageUp, &l, &NAMES, none, 4), PickerDid::Changed);
        assert_eq!(p.highlight, NAMES.len() - 1);
        assert_eq!(p.key(PickerKey::PageDown, &l, &NAMES, none, 4), PickerDid::Changed);
        // Enter: Claude in front; Shift behind; Ctrl a shell
        p.highlight = 2;
        assert_eq!(p.key(PickerKey::Enter { shift: false, ctrl: false }, &l, &NAMES, none, 4), PickerDid::Pick { row: 2, pick: Pick::New { behind: false } });
        assert_eq!(p.key(PickerKey::Enter { shift: true, ctrl: false }, &l, &NAMES, none, 4), PickerDid::Pick { row: 2, pick: Pick::New { behind: true } });
        assert_eq!(p.key(PickerKey::Enter { shift: false, ctrl: true }, &l, &NAMES, none, 4), PickerDid::Pick { row: 2, pick: Pick::Shell });
        // Digits pick a row with the box empty
        assert_eq!(p.key(PickerKey::Digit('3'), &l, &NAMES, none, 4), PickerDid::Pick { row: 2, pick: Pick::New { behind: false } });
        assert_eq!(p.key(PickerKey::Digit('0'), &l, &NAMES, none, 4), PickerDid::Nothing, "a word, typed");
        // Alt+M only with mail
        assert_eq!(p.key(PickerKey::Inmail, &l, &NAMES, none, 4), PickerDid::Nothing);
        assert_eq!(p.key(PickerKey::Inmail, &l, &NAMES, |_| 2, 4), PickerDid::Pick { row: 2, pick: Pick::Inmail });
        assert_eq!(p.key(PickerKey::Resume, &l, &NAMES, none, 4), PickerDid::Pick { row: 2, pick: Pick::Resume });
        assert_eq!(p.key(PickerKey::Escape, &l, &NAMES, none, 4), PickerDid::Close);
        // Typed: digits are words, and the first message shows
        p.input.set("bargain hunter find a kettle 2");
        p.edited();
        let l = listed(p.query(), &NAMES, &[], &e);
        assert_eq!(p.key(PickerKey::Digit('2'), &l, &NAMES, none, 4), PickerDid::Nothing);
        assert_eq!(p.message(&l, &NAMES).as_deref(), Some("find a kettle 2"));
        // Tab puts the highlighted name in the box
        p.input.set("barg");
        p.edited();
        let l = listed(p.query(), &NAMES, &[], &e);
        assert_eq!(p.message(&l, &NAMES), None);
        assert_eq!(p.key(PickerKey::Tab, &l, &NAMES, none, 4), PickerDid::Changed);
        assert_eq!(p.query(), "Bargain Hunter ");
        // Nothing named: Enter says so
        p.input.set("xyz");
        p.edited();
        let l = listed(p.query(), &NAMES, &[], &e);
        assert_eq!(p.key(PickerKey::Enter { shift: false, ctrl: false }, &l, &NAMES, none, 4), PickerDid::NoMatch);
        assert_eq!(p.key(PickerKey::Up, &l, &NAMES, none, 4), PickerDid::Nothing);
        // Started behind: the box cleared, the picker saying so until the next edit
        p.started_behind("Warden");
        assert_eq!((p.query(), p.started.as_deref(), p.highlight), ("", Some("Started Warden"), 0));
        p.edited();
        assert_eq!(p.started, None);
        // The shell's picker: Enter a shell, Ctrl+Enter Claude, no first message
        let mut s = PickerState::new(Mode::Shell);
        s.input.set("war now");
        let l = listed(s.query(), &NAMES, &[], &e);
        assert_eq!(s.key(PickerKey::Enter { shift: false, ctrl: false }, &l, &NAMES, none, 4), PickerDid::Pick { row: 0, pick: Pick::Shell });
        assert_eq!(s.key(PickerKey::Enter { shift: false, ctrl: true }, &l, &NAMES, none, 4), PickerDid::Pick { row: 0, pick: Pick::New { behind: false } });
        assert_eq!(s.message(&l, &NAMES), None);
        // A list grown shorter under the highlight holds it inside
        let mut h = PickerState::new(Mode::Claude);
        h.highlight = 7;
        h.scroll = 5;
        h.hold(3, 2);
        assert_eq!((h.highlight, h.scroll), (2, 2));
    }
}
