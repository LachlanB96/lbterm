//! Each Claude Code session's summary, as the summary worker writes it to
//! `~/.claude/summaries`: `<sid>.txt`, where the conversation is up to, in a
//! line; `<sid>.detail.txt`, a few lines more; and `<sid>.label.txt`, a word or
//! three naming what the session is for, which the tab rail shows under each
//! pad. The statusline adds `<sid>.cwd.txt`, the folder the session is in now,
//! which the tab panel names the tab's project from. The question worker
//! adds `<sid>.questions.json`, the questions its last replies asked and the
//! passages each is about, for the question colours (questions.rs). Whoever
//! writes the summaries keeps `<sid>.objectives.json` too, what Lachlan asked
//! the session for and whether each got done, for the objectives panel
//! (objectives.rs). The statusline keeps `<sid>.tasks.json`, the background
//! tasks the session still has running, for the card the tab panel shows on a
//! tab's row (tasks.rs), and `<sid>.usage.json`, what the conversation has cost
//! and its context window, for the pill under the InMail button and the open
//! tab panel's rows (lbterm/CONTEXT.md). `<sid>.prompt.json` is when Lachlan
//! last sent the session a prompt, `{ "at": <Unix ms> }`, which the statusline
//! was asked on 2026-11-14 to keep beside it, for the time since his last input
//! on the open tab panel's rows (lbterm/TAB-PANEL.md, "What a row says"):
//! anything that shows it takes it from the summary with [`Summary::last_prompt`],
//! the seam a session of the harness that's to take Claude Code's place will give
//! its time through instead. The session is the `claude_session` user var the
//! Claude hook (claudehook.exe) sets in its pane.
//!
//! The folder holds a thousand sessions and more, so only the ones asked for
//! are read, and never on the GUI thread: [`Summaries::get`] answers from
//! memory, a summary shared rather than copied (the rail asks for every tab's
//! on every frame, and until 2026-10-30 each look copied its words and its
//! detail's lines), and asks a thread of its own to read a session it hasn't
//! seen. That thread watches the folder and reads again only the file of a
//! session's that changed (the statusline writes `<sid>.cwd.txt` and
//! `<sid>.tasks.json` as it goes, and each of those read all seven files);
//! each change bumps [`Summaries::generation`] and calls the wake the GUI gave
//! it, so the windows repaint. The statusline rewrites `<sid>.usage.json` as
//! often as a working session's cost moves a cent, so a change to that alone
//! wakes the GUI with [`Changed::Usage`] and the sessions it was for: only a
//! window showing one of them repaints, where any other change repaints them
//! all. When a session was last prompted shows only on the open panel's rows,
//! where its cost shows too, so a change to it, alone or with the cost, wakes
//! the same way, its session on the same `Usage` list, and lays nothing out
//! again. A `<sid>.prompt.json` that can't be read is no time, as no file is,
//! and the log says why once, not at every read.
//!
//! The same thread follows `expanded.flag` in the folder, which Terminal's
//! Alt+S (its lbterm.lua, lbterm-summary-detail) toggles to show each summary's detail.
//!
//! It hears `<sid>.cost.json` too, the Claude project's split of what a conversation
//! cost, which the statusline writes whole each time it moves, and reads none of it:
//! the cost pane reads it (wezterm-gui's cost.rs) and only while it's open. For a
//! session asked for, each write heard counts one more on
//! [`Summaries::split_written`], and wakes the GUI with [`Changed::Split`], so a pane
//! open on that session reads the file on its own write and one shut reads it as it
//! opens only when it was written meanwhile (lbterm/CONTEXT.md, "The cost pane").

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use crate::objectives::{self, Objectives};
use crate::questions;
use crate::restore::is_session_id;
use crate::tasks;

/// The most of a label kept: the rail shows two short lines of it
pub const LABEL_MAX: usize = 24;
/// There while the summaries' detail is shown (Alt+S)
pub const EXPANDED_FLAG: &str = "expanded.flag";
/// How long a burst of file events settles before the sessions are read
const SETTLE: Duration = Duration::from_millis(60);
/// How many times the thread follows the folder before it reads: a folder made can have the next
/// one under it already
const FOLLOWS: usize = 8;

/// What the summary worker last wrote for one session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    /// What the session is for, in a word or three
    pub label: Option<String>,
    /// Where it's up to, in a line
    pub short: Option<String>,
    /// When that line was written
    pub written: Option<SystemTime>,
    /// A few lines more, blanks dropped
    pub detail: Vec<String>,
    /// The folder the session is in now, as Claude Code last told the
    /// statusline
    pub cwd: Option<String>,
    /// The questions its last replies asked, shared rather than copied:
    /// the rail asks for every tab's summary on every frame
    pub questions: Option<Arc<questions::Map>>,
    /// What it was asked for and what became of each, shared as the questions are
    pub objectives: Option<Arc<Objectives>>,
    /// The background tasks it still has running, shared as the questions are
    pub tasks: Option<Arc<tasks::List>>,
    /// What the conversation has cost and its context window, as the statusline last wrote them
    pub usage: Option<Usage>,
    /// When Lachlan last sent it a prompt, from `<sid>.prompt.json`: none with no file or one
    /// that can't be read. Asked for only through [`Summary::last_prompt`]
    prompted: Option<SystemTime>,
    /// Why `<sid>.prompt.json` can't be read, while it can't: kept so the log says it once
    prompt_unread: Option<String>,
}

/// What a conversation has used, from `<sid>.usage.json` (the statusline's `rememberUsage`):
/// `{ "usd": 12.34, "window": 1000000 }`
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Usage {
    /// What it has cost at list price, to the cent: every run of it and its subagents, as the
    /// Claude project counts its transcripts, not Claude Code's own figure for one process
    pub usd: f64,
    /// Its context window's size in tokens, once Claude Code has said it
    pub window: Option<u64>,
}

/// A `<sid>.usage.json`'s text read, or none when it isn't one (no `usd`, or not a number)
pub fn parse_usage(text: &str) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let usd = v.get("usd")?.as_f64().filter(|u| u.is_finite() && *u >= 0.0)?;
    Some(Usage {
        usd,
        window: v.get("window").and_then(serde_json::Value::as_u64).filter(|w| *w > 0),
    })
}

/// When a `<sid>.prompt.json`'s text says Lachlan last sent the session a prompt, `{ "at": <Unix
/// ms> }` as the statusline was asked on 2026-11-14 to write it, or why it says no time: it isn't
/// JSON, it has no `at`, or its `at` isn't a whole number of milliseconds after 1970.
pub fn parse_prompt(text: &str) -> Result<SystemTime, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|err| format!("it isn't JSON ({err})"))?;
    let at = v.get("at").ok_or_else(|| "it has no `at`".to_string())?;
    at.as_u64()
        .filter(|ms| *ms > 0)
        .and_then(|ms| SystemTime::UNIX_EPOCH.checked_add(Duration::from_millis(ms)))
        .ok_or_else(|| format!("its `at` is {at}, not a time in Unix milliseconds"))
}

/// When session `sid`'s `<sid>.prompt.json` in `dir` says it was last prompted: `Ok(None)` with no
/// file, and why it says no time when it can't be read or isn't one.
fn read_prompt(dir: &Path, sid: &str) -> Result<Option<SystemTime>, String> {
    match std::fs::read_to_string(dir.join(format!("{sid}.prompt.json"))) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("it can't be read ({err})")),
        Ok(text) => parse_prompt(&text).map(Some),
    }
}

/// Why `now`'s `<sid>.prompt.json` can't be read, when that's news since `was`: it could be read
/// then, or there was none, or it couldn't for another reason. What the log says, once a spell.
fn prompt_news<'a>(was: &Summary, now: &'a Summary) -> Option<&'a str> {
    now.prompt_unread.as_deref().filter(|why| was.prompt_unread.as_deref() != Some(*why))
}

impl Summary {
    /// When Lachlan last sent this session a prompt, typed, a voice draft LBTerm typed into Claude
    /// Code's box, or from his phone, which the open tab panel's row counts the time since his last
    /// input from: the one call anything asks it by, from a summary it already holds, so showing
    /// it costs no second lookup (render/tab_panel.rs' `panel_tab` runs for every tab the rail
    /// paints, and for the strip's tab on every paint). Today it's what the watching thread last
    /// read of `<sid>.prompt.json`; a session of the harness that's to take Claude Code's place
    /// will give it from that harness's session list instead, behind this same call, and wake the
    /// windows as a change to the file does ([`Changed::Usage`]). None with no file, or one that
    /// can't be read: never a zero.
    pub fn last_prompt(&self) -> Option<SystemTime> {
        self.prompted
    }
}

/// Which of a session's files a file is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Part {
    Short,
    Detail,
    Label,
    Cwd,
    Questions,
    Objectives,
    Tasks,
    Usage,
    Prompt,
}

/// What changed in the sessions asked for, told to the wake
#[derive(Debug, Clone, PartialEq)]
pub enum Changed {
    /// Anything a window may show: every window repaints
    Any,
    /// Only what these sessions have cost, or when they were last prompted (which the open tab
    /// panel's rows alone show, beside the cost): a window repaints when it shows one of them
    Usage(Vec<String>),
    /// These sessions' `<sid>.cost.json` was written (none of it read here): what a cost pane
    /// open on one of them reads, and nothing else shows. Told apart from the others, before
    /// them, so nothing wakes while no pane is open
    Split(Vec<String>),
}

/// Some of a session's files: those a burst of changes was to, a bit for each [`Part`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Parts(u16);

impl Parts {
    /// Every file: a session read for the first time
    pub const ALL: Parts = Parts(u16::MAX);

    pub fn of(part: Part) -> Parts {
        Parts(1 << part as u8)
    }

    pub fn has(self, part: Part) -> bool {
        self.0 & Parts::of(part).0 != 0
    }

    pub fn add(&mut self, other: Parts) {
        self.0 |= other.0;
    }
}

// Each part's bit is inside Parts' word. Part::Prompt is the last part, and a part one past the
// word would take the first part's bit in a release build, where the shift is masked to the word
// and the tests run (PITFALLS 380): a part added after it moves this to it
const _: () = assert!((Part::Prompt as u32) < u16::BITS);

/// The session a file in the folder belongs to, and which of its files it is.
pub fn parse_name(name: &str) -> Option<(&str, Part)> {
    [
        (".detail.txt", Part::Detail),
        (".label.txt", Part::Label),
        (".cwd.txt", Part::Cwd),
        (".questions.json", Part::Questions),
        (".objectives.json", Part::Objectives),
        (".tasks.json", Part::Tasks),
        (".usage.json", Part::Usage),
        (".prompt.json", Part::Prompt),
        (".txt", Part::Short),
    ]
        .into_iter()
        .find_map(|(suffix, part)| {
            name.strip_suffix(suffix)
                .filter(|sid| is_session_id(sid))
                .map(|sid| (sid, part))
        })
}

/// The session a `<sid>.cost.json` in the folder is the Claude project's split of (`@Claude:cost`)
pub fn split_of(name: &str) -> Option<&str> {
    name.strip_suffix(".cost.json").filter(|sid| is_session_id(sid))
}

/// A label as Haiku wrote it, tidied: its first line, without quotes or a
/// closing full stop, its spaces single, at most [`LABEL_MAX`] characters.
pub fn clean_label(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.strip_prefix("LABEL:").unwrap_or(line);
    let quotes: &[char] = &['"', '\'', '`', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}'];
    let line = line.trim().trim_matches(quotes).trim().trim_end_matches('.');
    let words: Vec<&str> = line.split_whitespace().collect();
    let label: String = words.join(" ").chars().take(LABEL_MAX).collect();
    let label = label.trim_end().to_string();
    (!label.is_empty()).then_some(label)
}

/// The summary line, its spaces single.
pub fn clean_short(text: &str) -> Option<String> {
    let short = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!short.is_empty()).then_some(short)
}

/// The detail's lines, trimmed, blanks dropped.
pub fn detail_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// One session's files, as they are now.
pub fn read(dir: &Path, sid: &str) -> Summary {
    read_parts(dir, sid, Parts::ALL, &Summary::default())
}

/// Session `sid` as `was` says but for `parts`, which are read from their files as they are now:
/// a change to one file reads that file alone.
pub fn read_parts(dir: &Path, sid: &str, parts: Parts, was: &Summary) -> Summary {
    let file = |suffix: &str| dir.join(format!("{sid}{suffix}"));
    let text = |suffix: &str| std::fs::read_to_string(file(suffix)).ok();
    let mut s = was.clone();
    if parts.has(Part::Label) {
        s.label = text(".label.txt").as_deref().and_then(clean_label);
    }
    if parts.has(Part::Short) {
        let short_path = file(".txt");
        s.short = std::fs::read_to_string(&short_path).ok().as_deref().and_then(clean_short);
        s.written = std::fs::metadata(&short_path).and_then(|m| m.modified()).ok();
    }
    if parts.has(Part::Detail) {
        s.detail = text(".detail.txt").as_deref().map(detail_lines).unwrap_or_default();
    }
    if parts.has(Part::Cwd) {
        s.cwd = text(".cwd.txt")
            .as_deref()
            .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(String::from);
    }
    if parts.has(Part::Questions) {
        s.questions = text(".questions.json")
            .as_deref()
            .and_then(questions::parse)
            .filter(|q| !q.messages.is_empty())
            .map(Arc::new);
    }
    if parts.has(Part::Objectives) {
        s.objectives = text(".objectives.json")
            .as_deref()
            .and_then(objectives::parse)
            .map(Arc::new);
    }
    if parts.has(Part::Tasks) {
        s.tasks = text(".tasks.json")
            .as_deref()
            .and_then(tasks::parse)
            .filter(|t| !t.tasks.is_empty())
            .map(Arc::new);
    }
    if parts.has(Part::Usage) {
        s.usage = text(".usage.json").as_deref().and_then(parse_usage);
    }
    if parts.has(Part::Prompt) {
        let read = read_prompt(dir, sid);
        s.prompt_unread = read.as_ref().err().cloned();
        s.prompted = read.ok().flatten();
        if let Some(why) = prompt_news(was, &s) {
            log::warn!("summaries: no time from {sid}.prompt.json, as {why}");
        }
    }
    s
}

enum Msg {
    /// Read this session, asked for the first time
    Read(String),
    /// Something changed in the folder, or on the way to it
    Changed(Vec<PathBuf>),
    /// The folder's watch went wrong (crate::watch): what changed can only be found by reading
    Failed(String),
}

struct Shared {
    known: Mutex<HashMap<String, Arc<Summary>>>,
    asked: Mutex<HashSet<String>>,
    expanded: AtomicBool,
    generation: AtomicU64,
    /// How many times each session's `<sid>.cost.json` has been heard written, for the sessions
    /// asked for
    splits: Mutex<HashMap<String, u64>>,
    wake: Box<dyn Fn(Changed) + Send + Sync>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The summaries of the sessions asked for, kept up to date on a thread of
/// their own.
pub struct Summaries {
    shared: Arc<Shared>,
    tx: Mutex<Sender<Msg>>,
}

impl Summaries {
    /// Start watching `dir`; `wake` is called, from the watching thread,
    /// whenever a session asked for changes, with what changed.
    pub fn start(dir: PathBuf, wake: impl Fn(Changed) + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            known: Mutex::new(HashMap::new()),
            asked: Mutex::new(HashSet::new()),
            expanded: AtomicBool::new(dir.join(EXPANDED_FLAG).exists()),
            generation: AtomicU64::new(0),
            splits: Mutex::new(HashMap::new()),
            wake: Box::new(wake),
        });
        let (tx, rx) = channel();
        let events = tx.clone();
        let thread_shared = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("lbterm-summaries".into())
            .spawn(move || watch(dir, thread_shared, rx, events));
        if let Err(err) = spawned {
            log::warn!("summaries: no thread to watch them on: {err:#}");
        }
        Self {
            shared,
            tx: Mutex::new(tx),
        }
    }

    /// What's known of session `sid`, shared: nothing of it is copied. The
    /// first time a session is asked for, this is `None` and it's read on the
    /// watching thread, which then wakes the GUI.
    pub fn get(&self, sid: &str) -> Option<Arc<Summary>> {
        if let Some(summary) = lock(&self.shared.known).get(sid) {
            return Some(Arc::clone(summary));
        }
        if is_session_id(sid) && lock(&self.shared.asked).insert(sid.to_string()) {
            let _ = lock(&self.tx).send(Msg::Read(sid.to_string()));
        }
        None
    }

    /// Bumped each time a summary asked for changes, but for what it has cost alone
    /// ([`Changed::Usage`]), or [`Self::expanded`].
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    /// How many times session `sid`'s `<sid>.cost.json` has been heard written since it was
    /// first asked for (0 before any), counted one more for every session asked for when the
    /// folder came, went or its watch went wrong, since a write may have gone unheard then. The
    /// file is never read here: two looks that agree mean it hasn't been written between them
    pub fn split_written(&self, sid: &str) -> u64 {
        lock(&self.shared.splits).get(sid).copied().unwrap_or(0)
    }

    /// Whether the summaries' detail is to be shown: `expanded.flag` is in
    /// the folder.
    pub fn expanded(&self) -> bool {
        self.shared.expanded.load(Ordering::Acquire)
    }
}

/// The watching thread: reads the sessions asked for, and reads again the
/// files of one that change. The folder is watched through crate::watch: one
/// not there yet from the folder above it, and one that goes (removed, renamed)
/// watched again, every session asked for read again then, as after a change
/// Windows couldn't keep.
fn watch(dir: PathBuf, shared: Arc<Shared>, rx: Receiver<Msg>, events: Sender<Msg>) {
    use crate::watch::{same_path, Seen, Target, Watch};
    let mut watch = match Watch::new(move |seen| {
        let _ = events.send(match seen {
            Seen::Changed(paths) => Msg::Changed(paths),
            Seen::Failed(err) => Msg::Failed(err),
        });
    }) {
        Ok(watch) => Some(watch),
        Err(err) => {
            log::error!("summaries: can't watch {}, so a summary won't change once read: {err:#}", dir.display());
            None
        }
    };
    let targets = [Target::Folder(dir.clone())];
    let follow = |watch: &mut Option<Watch>| {
        let mut moved = false;
        for _ in 0..FOLLOWS {
            if !watch.as_mut().is_some_and(|w| w.follow(&targets)) {
                break;
            }
            moved = true;
        }
        moved
    };
    follow(&mut watch);

    while let Ok(first) = rx.recv() {
        match &first {
            Msg::Read(sid) => log::debug!("summaries: asked for {sid}"),
            Msg::Changed(paths) => log::debug!("summaries: changed {paths:?}"),
            Msg::Failed(_) => {}
        }
        // Each session due a read, and which of its files
        let mut due: HashMap<String, Parts> = HashMap::new();
        // Each session whose split was written: counted, never read
        let mut splits: HashSet<String> = HashSet::new();
        let mut flag = false;
        // Every session asked for, and the flag: the folder came, went or moved, or its watch
        // went wrong
        let mut all = false;
        let mut take = |msg: Msg| match msg {
            Msg::Read(sid) => {
                due.entry(sid).or_default().add(Parts::ALL);
            }
            Msg::Changed(paths) => {
                let asked = lock(&shared.asked);
                for path in paths {
                    // The folder itself, or one on the way to it
                    if !path.parent().is_some_and(|parent| same_path(parent, &dir)) {
                        all = true;
                        continue;
                    }
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == EXPANDED_FLAG {
                        flag = true;
                    } else if let Some(sid) = split_of(name).filter(|sid| asked.contains(*sid)) {
                        splits.insert(sid.to_string());
                    } else if let Some((sid, part)) = parse_name(name).filter(|(sid, _)| asked.contains(*sid)) {
                        due.entry(sid.to_string()).or_default().add(Parts::of(part));
                    }
                }
            }
            Msg::Failed(err) => {
                log::error!("summaries: the watch of {}: {err}; every summary asked for is read again", dir.display());
                all = true;
            }
        };
        take(first);
        // A worker writes two or three files at once: read them together
        std::thread::sleep(SETTLE);
        while let Ok(msg) = rx.try_recv() {
            take(msg);
        }
        // Watched again before it's read, so nothing written in between goes unseen
        if all {
            follow(&mut watch);
            flag = true;
            for sid in lock(&shared.asked).iter() {
                due.entry(sid.clone()).or_default().add(Parts::ALL);
                splits.insert(sid.clone());
            }
        }
        // Counted before anything wakes, so a window woken for this batch sees them
        if !splits.is_empty() {
            let mut written = lock(&shared.splits);
            for sid in &splits {
                *written.entry(sid.clone()).or_default() += 1;
            }
        }

        // Anything a window may show changed, or only these sessions' usage
        let mut any = false;
        let mut usage = vec![];
        if flag {
            let expanded = dir.join(EXPANDED_FLAG).exists();
            any |= shared.expanded.swap(expanded, Ordering::AcqRel) != expanded;
        }
        for (sid, parts) in due {
            let was = lock(&shared.known).get(&sid).cloned();
            let summary = read_parts(&dir, &sid, parts, was.as_deref().unwrap_or(&Summary::default()));
            if was.as_deref() != Some(&summary) {
                if summary != Summary::default() || was.is_some() {
                    // What it cost and when it was last prompted wake only the windows showing them
                    // (the pill and the open panel's rows), and why its prompt's file can't be read
                    // shows nowhere
                    let alone = was.as_deref().filter(|was| {
                        let others = Summary {
                            usage: summary.usage,
                            prompted: summary.prompted,
                            prompt_unread: summary.prompt_unread.clone(),
                            ..(*was).clone()
                        };
                        others == summary
                    });
                    match alone {
                        Some(was) => {
                            if (was.usage, was.prompted) != (summary.usage, summary.prompted) {
                                usage.push(sid.clone());
                            }
                        }
                        None => any = true,
                    }
                }
                lock(&shared.known).insert(sid, Arc::new(summary));
            }
        }
        // A split written: told first and apart, so an open cost pane asks for it before a window
        // woken for the same batch paints, and nothing else wakes for it
        if !splits.is_empty() {
            let mut splits: Vec<String> = splits.into_iter().collect();
            splits.sort();
            (shared.wake)(Changed::Split(splits));
        }
        // The generation keys what's laid out from the summaries (the question colours, the
        // drafts), none of which shows what a session has cost or when it was last prompted: a
        // cent moving lays out nothing
        if any {
            shared.generation.fetch_add(1, Ordering::AcqRel);
            (shared.wake)(Changed::Any);
        } else if !usage.is_empty() {
            (shared.wake)(Changed::Usage(usage));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    const SID: &str = "5f0c2d7e-1b3a-4e6f-9a8b-7c6d5e4f3a21";
    const OTHER: &str = "0091c67b-1db2-470d-9e29-9bb1ffbda293";

    #[test]
    fn a_files_name_says_its_session_and_part() {
        assert_eq!(parse_name(&format!("{SID}.txt")), Some((SID, Part::Short)));
        assert_eq!(parse_name(&format!("{SID}.detail.txt")), Some((SID, Part::Detail)));
        assert_eq!(parse_name(&format!("{SID}.label.txt")), Some((SID, Part::Label)));
        assert_eq!(parse_name(&format!("{SID}.cwd.txt")), Some((SID, Part::Cwd)));
        assert_eq!(parse_name(&format!("{SID}.questions.json")), Some((SID, Part::Questions)));
        assert_eq!(parse_name(&format!("{SID}.objectives.json")), Some((SID, Part::Objectives)));
        assert_eq!(parse_name(&format!("{SID}.tasks.json")), Some((SID, Part::Tasks)));
        assert_eq!(parse_name(&format!("{SID}.usage.json")), Some((SID, Part::Usage)));
        assert_eq!(parse_name(&format!("{SID}.prompt.json")), Some((SID, Part::Prompt)));
        assert_eq!(parse_name(&format!("{SID}.prompt.json.tmp")), None);
        assert_eq!(parse_name(&format!("{SID}.json")), None);
        assert_eq!(parse_name(&format!("{SID}.worker")), None);
        assert_eq!(parse_name("expanded.flag"), None);
        assert_eq!(parse_name("notes.txt"), None);
    }

    /// Every part, in the order `Part` declares them: a part added there is added here too
    const EVERY: [Part; 9] = [
        Part::Short,
        Part::Detail,
        Part::Label,
        Part::Cwd,
        Part::Questions,
        Part::Objectives,
        Part::Tasks,
        Part::Usage,
        Part::Prompt,
    ];

    /// Each part has a bit of its own in `Parts`, compared with every other part: "every file has
    /// the new part" and "some other part lacks it" both still hold when a part one past the word
    /// takes the first part's bit, as it does in a release build, where `lbterm.ps1 test` runs
    /// these (PITFALLS 380)
    #[test]
    fn each_part_has_a_bit_of_its_own() {
        for (i, a) in EVERY.into_iter().enumerate() {
            assert_eq!(a as usize, i, "EVERY lists the parts in Part's order");
            assert!(Parts::ALL.has(a), "{a:?}");
            for b in EVERY.into_iter().filter(|b| *b != a) {
                assert!(!Parts::of(a).has(b), "{a:?} has {b:?}'s bit");
            }
        }
    }

    #[test]
    fn labels_are_tidied() {
        assert_eq!(clean_label("Tab bar bugs\n").as_deref(), Some("Tab bar bugs"));
        assert_eq!(clean_label("\n  \"Lodestone.\"  \nmore").as_deref(), Some("Lodestone"));
        assert_eq!(clean_label("LABEL: Scene   realism").as_deref(), Some("Scene realism"));
        assert_eq!(clean_label("\u{201c}InMail perms\u{201d}").as_deref(), Some("InMail perms"));
        assert_eq!(
            clean_label("a label far longer than the rail could ever show").as_deref(),
            Some("a label far longer than")
        );
        assert_eq!(clean_label("  \n\"\"\n"), None);
    }

    #[test]
    fn detail_drops_blank_lines() {
        assert_eq!(
            detail_lines("Goal: x\n\n  Finding: y  \r\nNext: z\n"),
            vec!["Goal: x", "Finding: y", "Next: z"]
        );
    }

    #[test]
    fn a_session_is_read_from_its_three_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(format!("{SID}.txt")), "APC mini LEDs:  mapping fixed\n").unwrap();
        std::fs::write(dir.path().join(format!("{SID}.detail.txt")), "Goal: a\n\nNext: b\n").unwrap();
        std::fs::write(dir.path().join(format!("{SID}.label.txt")), "APC LEDs\n").unwrap();
        std::fs::write(dir.path().join(format!("{SID}.cwd.txt")), "C:\\Life\\Projects\\APCommand").unwrap();
        let s = read(dir.path(), SID);
        assert_eq!(s.cwd.as_deref(), Some("C:\\Life\\Projects\\APCommand"));
        assert_eq!(s.short.as_deref(), Some("APC mini LEDs: mapping fixed"));
        assert_eq!(s.detail, vec!["Goal: a", "Next: b"]);
        assert_eq!(s.label.as_deref(), Some("APC LEDs"));
        assert!(s.written.is_some());
        assert_eq!(s.questions, None);
        assert_eq!(read(dir.path(), OTHER), Summary::default());
    }

    #[test]
    fn a_sessions_questions_are_read_when_there_are_any() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{SID}.questions.json"));
        std::fs::write(&file, r#"{"messages":[{"id":"m","questions":[{"label":"Q1","text":"Which?"}]}]}"#).unwrap();
        let q = read(dir.path(), SID).questions.unwrap();
        assert_eq!(q.messages[0].questions[0].label, "Q1");
        // None asked, or not the worker's: as if there were no file
        std::fs::write(&file, r#"{"messages":[]}"#).unwrap();
        assert_eq!(read(dir.path(), SID).questions, None);
        std::fs::write(&file, "{").unwrap();
        assert_eq!(read(dir.path(), SID).questions, None);
    }

    #[test]
    fn a_sessions_objectives_are_read_when_it_has_any() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{SID}.objectives.json"));
        std::fs::write(&file, r#"{"current":1,"objectives":[{"id":1,"text":"Fix the rail","from":"first","status":"open"}]}"#).unwrap();
        let o = read(dir.path(), SID).objectives.unwrap();
        assert_eq!((o.current, o.list[0].text.as_str()), (1, "Fix the rail"));
        std::fs::write(&file, r#"{"objectives":[]}"#).unwrap();
        assert_eq!(read(dir.path(), SID).objectives, None);
        std::fs::write(&file, "{").unwrap();
        assert_eq!(read(dir.path(), SID).objectives, None);
    }

    #[test]
    fn a_sessions_tasks_are_read_while_any_run() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{SID}.tasks.json"));
        assert_eq!(read(dir.path(), SID).tasks, None);
        std::fs::write(&file, r#"{"sessionId":"x","tasks":[{"id":"b1","kind":"command","description":"Run the check","startedAt":1}]}"#).unwrap();
        let t = read(dir.path(), SID).tasks.unwrap();
        assert_eq!(t.tasks[0].description, "Run the check");
        // None running, or not the statusline's: as if there were no file
        std::fs::write(&file, r#"{"tasks":[]}"#).unwrap();
        assert_eq!(read(dir.path(), SID).tasks, None);
        std::fs::write(&file, "{").unwrap();
        assert_eq!(read(dir.path(), SID).tasks, None);
    }

    /// `<sid>.prompt.json` as the statusline was asked to write it, `{ "at": <Unix ms> }`: its time,
    /// and no time, never a zero, with no file or one that isn't that, saying why
    #[test]
    fn when_a_session_was_last_prompted_is_read_as_the_statusline_writes_it() {
        let at = |ms: u64| SystemTime::UNIX_EPOCH + Duration::from_millis(ms);
        assert_eq!(parse_prompt(r#"{"at":1763100000000}"#), Ok(at(1_763_100_000_000)));
        assert_eq!(parse_prompt(r#"{ "at": 1763100000123, "by": "statusline" }"#), Ok(at(1_763_100_000_123)));
        for text in ["", "{", "{}", "[1763100000000]", r#"{"at":null}"#, r#"{"at":"1763100000000"}"#, r#"{"at":-5}"#, r#"{"at":0}"#, r#"{"at":1.5e12}"#] {
            assert!(parse_prompt(text).is_err(), "{text}");
        }
        assert_eq!(parse_prompt("{}"), Err("it has no `at`".to_string()));
        assert!(parse_prompt("{").is_err_and(|why| why.starts_with("it isn't JSON")));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{SID}.prompt.json"));
        // Missing: no time, and nothing to say
        assert_eq!(read(dir.path(), SID), Summary::default());
        // Present
        std::fs::write(&file, r#"{"at":1763100000000}"#).unwrap();
        let one = read(dir.path(), SID);
        assert_eq!((one.last_prompt(), one.prompt_unread.as_deref()), (Some(at(1_763_100_000_000)), None));
        // Written again: read alone
        std::fs::write(&file, r#"{"at":1763100060000}"#).unwrap();
        let two = read_parts(dir.path(), SID, Parts::of(Part::Prompt), &one);
        assert_eq!(two.prompted, Some(at(1_763_100_060_000)));
        // Malformed: no time, and why kept
        std::fs::write(&file, r#"{"at":"soon"}"#).unwrap();
        let bad = read_parts(dir.path(), SID, Parts::of(Part::Prompt), &two);
        assert_eq!(bad.prompted, None);
        assert_eq!(bad.prompt_unread.as_deref(), Some(r#"its `at` is "soon", not a time in Unix milliseconds"#));
        // Removed: no time, and nothing to say
        std::fs::remove_file(&file).unwrap();
        let gone = read_parts(dir.path(), SID, Parts::of(Part::Prompt), &bad);
        assert_eq!((gone.prompted, gone.prompt_unread.as_deref()), (None, None));
    }

    /// The log says why a prompt's file can't be read once a spell: when it could before, or there
    /// was none, or it couldn't for another reason; not again while it stays the same
    #[test]
    fn a_prompt_file_that_cant_be_read_is_logged_once() {
        let fine = Summary { prompted: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)), ..Summary::default() };
        let bad = |why: &str| Summary { prompt_unread: Some(why.to_string()), ..Summary::default() };
        let none = Summary::default();
        assert_eq!(prompt_news(&none, &bad("it has no `at`")), Some("it has no `at`"));
        assert_eq!(prompt_news(&fine, &bad("it has no `at`")), Some("it has no `at`"));
        assert_eq!(prompt_news(&bad("it has no `at`"), &bad("it has no `at`")), None);
        assert_eq!(prompt_news(&bad("it has no `at`"), &bad("it isn't JSON")), Some("it isn't JSON"));
        assert_eq!(prompt_news(&bad("it has no `at`"), &fine), None);
        assert_eq!(prompt_news(&bad("it has no `at`"), &none), None);
    }

    /// When a session was last prompted, followed as the statusline writes it, whole through a
    /// rename: read with its other files, and a change to it alone, or with its cost, wakes as a
    /// cost does, `Changed::Usage` alone (only the open panel's rows and the pill show either), and
    /// moves no generation; written again the same, or malformed again, it wakes nothing; malformed
    /// or removed, it gives no time
    #[test]
    fn when_a_session_was_last_prompted_is_followed_and_wakes_as_its_cost_does() {
        let dir = tempfile::tempdir().unwrap();
        let file = |suffix: &str| dir.path().join(format!("{SID}{suffix}"));
        let at = |ms: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_millis(ms));
        let write = |text: &str| {
            let tmp = file(".prompt.json.tmp");
            std::fs::write(&tmp, text).unwrap();
            std::fs::rename(&tmp, file(".prompt.json")).unwrap();
        };
        std::fs::write(file(".label.txt"), "Tab bar bugs").unwrap();
        write(r#"{"at":1763100000000}"#);
        let woke = Arc::new(Mutex::new(vec![]));
        let w = Arc::clone(&woke);
        let summaries = Summaries::start(dir.path().to_path_buf(), move |changed| lock(&w).push(changed));
        let since = |wakes: usize| lock(&woke)[wakes..].to_vec();
        let last = || summaries.get(SID).and_then(|s| s.last_prompt());
        let shown = Changed::Usage(vec![SID.to_string()]);

        // Present: asked for, and read off thread with the session's other files
        assert_eq!(last(), None);
        assert!(until(|| last() == at(1_763_100_000_000)));
        assert_eq!(lock(&woke).last(), Some(&Changed::Any), "a session read for the first time");
        // Give the watcher a moment to start before the folder changes
        std::thread::sleep(Duration::from_millis(200));
        let generation = summaries.generation();

        // Written again: read again, and only the windows showing it wake
        let wakes = lock(&woke).len();
        write(r#"{"at":1763100060000}"#);
        assert!(until(|| last() == at(1_763_100_060_000)));
        assert!(until(|| since(wakes) == vec![shown.clone()]));
        assert_eq!(summaries.get(SID).and_then(|s| s.label.clone()).as_deref(), Some("Tab bar bugs"));
        assert_eq!(summaries.generation(), generation, "nothing laid out from the summaries shows it");

        // With its cost, as the statusline may write them: told as a cost is, one batch or two,
        // nothing else
        std::thread::sleep(Duration::from_millis(400));
        let wakes = lock(&woke).len();
        std::fs::write(file(".usage.json"), r#"{"usd":1.25,"window":200000}"#).unwrap();
        write(r#"{"at":1763100120000}"#);
        assert!(until(|| {
            last() == at(1_763_100_120_000) && summaries.get(SID).and_then(|s| s.usage).map(|u| u.usd) == Some(1.25)
        }));
        std::thread::sleep(Duration::from_millis(400));
        assert!(!since(wakes).is_empty() && since(wakes).iter().all(|c| *c == shown), "{:?}", since(wakes));

        // The same time written again changes nothing, and wakes nothing
        std::thread::sleep(Duration::from_millis(400));
        let wakes = lock(&woke).len();
        write(r#"{"at":1763100120000}"#);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(since(wakes), vec![]);

        // Malformed: no time, never a zero, and the windows showing it wake to show none
        let wakes = lock(&woke).len();
        write(r#"{"at":"soon"}"#);
        assert!(until(|| last().is_none()));
        assert!(until(|| since(wakes) == vec![shown.clone()]));
        assert!(summaries.get(SID).is_some_and(|s| s.prompt_unread.as_deref().is_some_and(|why| why.contains("soon"))));
        // Malformed again the same way: nothing to show, and nothing new to log
        std::thread::sleep(Duration::from_millis(400));
        let wakes = lock(&woke).len();
        write(r#"{"at":"soon"}"#);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(since(wakes), vec![]);

        // Fine again, then removed: no time
        write(r#"{"at":1763100180000}"#);
        assert!(until(|| last() == at(1_763_100_180_000)));
        std::thread::sleep(Duration::from_millis(400));
        let wakes = lock(&woke).len();
        std::fs::remove_file(file(".prompt.json")).unwrap();
        assert!(until(|| last().is_none()));
        assert!(until(|| since(wakes) == vec![shown.clone()]));
        assert_eq!(summaries.generation(), generation);
    }

    #[test]
    fn what_a_conversation_has_used_is_read_as_the_statusline_writes_it() {
        assert_eq!(parse_usage(r#"{"usd":12.34,"window":1000000}"#), Some(Usage { usd: 12.34, window: Some(1_000_000) }));
        assert_eq!(parse_usage(r#"{"usd":0.84,"window":null}"#), Some(Usage { usd: 0.84, window: None }));
        assert_eq!(parse_usage(r#"{"usd":3}"#), Some(Usage { usd: 3.0, window: None }));
        // Not the statusline's: as if there were no file
        for text in [r#"{"window":200000}"#, r#"{"usd":"lots"}"#, r#"{"usd":-1}"#, "{"] {
            assert_eq!(parse_usage(text), None, "{text}");
        }
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path(), SID).usage, None);
        std::fs::write(dir.path().join(format!("{SID}.usage.json")), r#"{"usd":7.5,"window":200000}"#).unwrap();
        assert_eq!(read(dir.path(), SID).usage, Some(Usage { usd: 7.5, window: Some(200_000) }));
    }

    #[test]
    fn a_change_to_what_a_session_cost_alone_wakes_the_windows_showing_it_and_any_other_wakes_them_all() {
        let dir = tempfile::tempdir().unwrap();
        let file = |suffix: &str| dir.path().join(format!("{SID}{suffix}"));
        std::fs::write(file(".label.txt"), "Tab bar bugs").unwrap();
        std::fs::write(file(".usage.json"), r#"{"usd":1.25,"window":200000}"#).unwrap();
        let woke = Arc::new(Mutex::new(vec![]));
        let w = Arc::clone(&woke);
        let summaries = Summaries::start(dir.path().to_path_buf(), move |changed| lock(&w).push(changed));
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.usage.is_some())));
        assert_eq!(lock(&woke).last(), Some(&Changed::Any), "a session read for the first time");
        // Give the watcher a moment to start before the folder changes
        std::thread::sleep(Duration::from_millis(200));

        let generation = summaries.generation();
        std::fs::write(file(".usage.json"), r#"{"usd":1.26,"window":200000}"#).unwrap();
        assert!(until(|| summaries.get(SID).and_then(|s| s.usage).is_some_and(|u| u.usd == 1.26)));
        assert!(until(|| lock(&woke).last() == Some(&Changed::Usage(vec![SID.to_string()]))));
        assert_eq!(summaries.get(SID).and_then(|s| s.label.clone()).as_deref(), Some("Tab bar bugs"));
        assert_eq!(summaries.generation(), generation, "nothing laid out from the summaries shows it");

        // The same cost written again changes nothing, and wakes nothing
        let wakes = lock(&woke).len();
        std::fs::write(file(".usage.json"), r#"{"usd":1.26,"window":200000}"#).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(lock(&woke).len(), wakes);

        std::fs::write(file(".label.txt"), "Rail labels").unwrap();
        assert!(until(|| lock(&woke).last() == Some(&Changed::Any)));
    }

    /// The Claude project's split, `<sid>.cost.json`, is heard and never read: a write to an asked
    /// session's counts one more and wakes only with `Changed::Split`, before anything else the
    /// same batch wakes for; one to a session not asked for counts and wakes nothing; and a cost
    /// moving a cent without it counts nothing (lbterm/CONTEXT.md, "The cost pane")
    #[test]
    fn a_split_written_is_counted_and_told_apart_but_never_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = |sid: &str, suffix: &str| dir.path().join(format!("{sid}{suffix}"));
        std::fs::write(file(SID, ".usage.json"), r#"{"usd":1.25,"window":200000}"#).unwrap();
        let woke = Arc::new(Mutex::new(vec![]));
        let w = Arc::clone(&woke);
        let summaries = Summaries::start(dir.path().to_path_buf(), move |changed| lock(&w).push(changed));
        assert!(until(|| summaries.get(SID).is_some()));
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(summaries.split_written(SID), 0);
        assert_eq!(split_of(&format!("{SID}.cost.json")), Some(SID));
        assert_eq!(split_of("journal.cost.json"), None);

        let wakes = lock(&woke).len();
        std::fs::write(file(SID, ".cost.json"), "not read").unwrap();
        assert!(until(|| summaries.split_written(SID) >= 1));
        assert!(until(|| lock(&woke).len() > wakes));
        assert_eq!(lock(&woke)[wakes], Changed::Split(vec![SID.to_string()]));
        assert!(summaries.get(SID).is_some_and(|s| s.usage.is_some_and(|u| u.usd == 1.25)), "the summary as it was");

        // Written with the cost moving a cent, as the statusline writes them: the split told first
        let written = summaries.split_written(SID);
        let wakes = lock(&woke).len();
        std::fs::write(file(SID, ".usage.json"), r#"{"usd":1.26,"window":200000}"#).unwrap();
        std::fs::write(file(SID, ".cost.json"), "still not read").unwrap();
        assert!(until(|| lock(&woke).last() == Some(&Changed::Usage(vec![SID.to_string()]))));
        assert!(summaries.split_written(SID) > written);
        assert_eq!(lock(&woke)[wakes], Changed::Split(vec![SID.to_string()]));

        // The cost alone: no split counted. Windows can tell one write as two changes, the second
        // in a later batch (counted twice, which asks once more and reads the same split), so
        // what's left of the last is let through first
        std::thread::sleep(Duration::from_millis(400));
        let written = summaries.split_written(SID);
        std::fs::write(file(SID, ".usage.json"), r#"{"usd":1.27,"window":200000}"#).unwrap();
        assert!(until(|| lock(&woke).last() == Some(&Changed::Usage(vec![SID.to_string()]))));
        assert_eq!(summaries.split_written(SID), written);

        // A session nobody asked for: nothing
        std::thread::sleep(Duration::from_millis(400));
        let wakes = lock(&woke).len();
        std::fs::write(file(OTHER, ".cost.json"), "{}").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert!(!lock(&woke)[wakes..].iter().any(|c| matches!(c, Changed::Split(sids) if sids.iter().any(|s| s == OTHER))));
        assert_eq!(summaries.split_written(OTHER), 0);
    }

    /// Waits for `done`, up to five seconds.
    fn until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        done()
    }

    /// ToolBelt's question worker writes `<sid>.questions.json` through a rename: a file renamed
    /// over a known session's is read again like one written in place, and bumps the generation
    /// the question colours' layout is keyed by (lbterm/FLOWS.md, "A right hold, to the question
    /// colours", steps 4 and 5)
    #[test]
    fn a_questions_file_renamed_over_is_read_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{SID}.questions.json"));
        let one = r#"{"messages":[{"id":"a","questions":[{"label":"Q1","text":"Which?"}]}]}"#;
        let two = r#"{"messages":[{"id":"a","questions":[{"label":"Q1","text":"Which?"}]},{"id":"b","questions":[{"label":"Q1","text":"And now?"}]}]}"#;
        std::fs::write(&file, one).unwrap();
        let summaries = Summaries::start(dir.path().to_path_buf(), |_| {});
        let replies = |s: &Summaries| s.get(SID).and_then(|s| s.questions.clone()).map(|q| q.messages.len());
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| replies(&summaries) == Some(1)));
        let generation = summaries.generation();
        // Give the watcher a moment to start before the folder changes
        std::thread::sleep(Duration::from_millis(200));
        let tmp = dir.path().join(format!("{SID}.questions.json.tmp"));
        std::fs::write(&tmp, two).unwrap();
        std::fs::rename(&tmp, &file).unwrap();
        assert!(until(|| replies(&summaries) == Some(2)));
        assert!(summaries.generation() > generation);
    }

    #[test]
    fn sessions_asked_for_are_read_off_thread_and_kept_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let label = |sid: &str| dir.path().join(format!("{sid}.label.txt"));
        std::fs::write(label(SID), "Tab bar bugs").unwrap();
        std::fs::write(label(OTHER), "Lodestone").unwrap();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let summaries = Summaries::start(dir.path().to_path_buf(), move |_| {
            w.fetch_add(1, Ordering::SeqCst);
        });

        // Not known yet: asked for, and read on the other thread
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| summaries.get(SID).and_then(|s| s.label.clone()).as_deref() == Some("Tab bar bugs")));
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let generation = summaries.generation();

        // A new label is picked up when the worker writes it
        std::fs::write(label(SID), "Rail labels").unwrap();
        assert!(until(|| summaries.get(SID).and_then(|s| s.label.clone()).as_deref() == Some("Rail labels")));
        assert!(summaries.generation() > generation);

        // A session nobody asked for is never read, and wakes nothing
        let woken_before = woken.load(Ordering::SeqCst);
        std::fs::write(label(OTHER), "Retiring Lodestone").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(woken.load(Ordering::SeqCst), woken_before);
        assert!(lock(&summaries.shared.known).get(OTHER).is_none());

        // A session with no files yet is known as empty, and asked for only once
        let empty = "11111111-2222-3333-4444-555555555555";
        assert_eq!(summaries.get(empty), None);
        assert!(until(|| summaries.get(empty).as_deref() == Some(&Summary::default())));
        std::fs::write(dir.path().join(format!("{empty}.txt")), "first summary").unwrap();
        assert!(until(|| summaries.get(empty).and_then(|s| s.short.clone()).as_deref() == Some("first summary")));
    }

    #[test]
    fn a_change_to_one_file_reads_that_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let file = |suffix: &str| dir.path().join(format!("{SID}{suffix}"));
        std::fs::write(file(".label.txt"), "Tab bar bugs").unwrap();
        std::fs::write(file(".txt"), "Fixing the rail").unwrap();
        std::fs::write(file(".cwd.txt"), "C:\\work\\demo").unwrap();
        let was = read(dir.path(), SID);
        // The label's file says otherwise now, but only the folder changed: the label stays as
        // it was read, and the folder is the new one
        std::fs::write(file(".label.txt"), "Something else").unwrap();
        std::fs::write(file(".cwd.txt"), "C:\\work\\other").unwrap();
        let now = read_parts(dir.path(), SID, Parts::of(Part::Cwd), &was);
        assert_eq!(now.cwd.as_deref(), Some("C:\\work\\other"));
        assert_eq!(now.label.as_deref(), Some("Tab bar bugs"));
        assert_eq!((&now.short, now.written), (&was.short, was.written));
        // Every part, as a first read
        assert_eq!(read_parts(dir.path(), SID, Parts::ALL, &Summary::default()).label.as_deref(), Some("Something else"));
        let mut two = Parts::of(Part::Label);
        two.add(Parts::of(Part::Tasks));
        assert!(two.has(Part::Label) && two.has(Part::Tasks) && !two.has(Part::Cwd));
    }

    #[test]
    fn a_session_is_shared_by_every_look_and_known_again_when_a_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(format!("{SID}.label.txt")), "Tab bar bugs").unwrap();
        let summaries = Summaries::start(dir.path().to_path_buf(), |_| {});
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| summaries.get(SID).is_some()));
        // Every look is the same summary, shared
        let (a, b) = (summaries.get(SID).unwrap(), summaries.get(SID).unwrap());
        assert!(Arc::ptr_eq(&a, &b));
        // The statusline writing the session's folder: a new summary, with the label it had
        std::fs::write(dir.path().join(format!("{SID}.cwd.txt")), "C:\\work\\demo").unwrap();
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.cwd.is_some())));
        let c = summaries.get(SID).unwrap();
        assert_eq!(c.label.as_deref(), Some("Tab bar bugs"));
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn alt_s_flag_is_followed() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(EXPANDED_FLAG);
        std::fs::write(&flag, "").unwrap();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let summaries = Summaries::start(dir.path().to_path_buf(), move |_| {
            w.fetch_add(1, Ordering::SeqCst);
        });
        // As it was at the start, without a wake
        assert!(summaries.expanded());
        // Give the watcher a moment to start before the folder changes
        std::thread::sleep(Duration::from_millis(200));

        std::fs::remove_file(&flag).unwrap();
        assert!(until(|| !summaries.expanded()));
        assert!(until(|| woken.load(Ordering::SeqCst) == 1));
        let generation = summaries.generation();

        std::fs::write(&flag, "").unwrap();
        assert!(until(|| summaries.expanded()));
        assert!(until(|| summaries.generation() > generation));
    }

    #[test]
    fn the_folder_removed_and_made_again_is_read_again() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("summaries");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(format!("{SID}.label.txt")), "Before").unwrap();
        let summaries = Summaries::start(dir.clone(), |_| {});
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.as_deref() == Some("Before"))));
        // Windows ends the folder's watch as it goes: the thread watches it again once it's
        // back, and reads what it holds
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.is_none())));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(format!("{SID}.label.txt")), "After").unwrap();
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.as_deref() == Some("After"))));
        // And the new folder's watch follows it as the old one did
        std::fs::write(dir.join(format!("{SID}.label.txt")), "Later").unwrap();
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.as_deref() == Some("Later"))));
    }

    #[test]
    fn the_folder_renamed_away_and_made_again_is_read_again() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("summaries");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(format!("{SID}.label.txt")), "Before").unwrap();
        let summaries = Summaries::start(dir.clone(), |_| {});
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.as_deref() == Some("Before"))));
        // Its watch would follow it to its new name, Windows telling it nothing
        std::fs::rename(&dir, root.path().join("summaries.old")).unwrap();
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.is_none())));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(format!("{SID}.label.txt")), "After").unwrap();
        assert!(until(|| summaries.get(SID).is_some_and(|s| s.label.as_deref() == Some("After"))));
    }
}
