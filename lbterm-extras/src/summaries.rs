//! Each Claude Code session's summary, as Terminal's summary worker
//! (`shared/claude/summary-worker.js`) writes it to `~/.claude/summaries`:
//! `<sid>.txt`, where the conversation is up to, in a line; `<sid>.detail.txt`,
//! a few lines more; and `<sid>.label.txt`, a word or three naming what the
//! session is for, which the tab rail shows under each pad. Terminal's
//! statusline adds `<sid>.cwd.txt`, the folder the session is in now, which
//! the tab panel names the tab's project from. Terminal's question worker
//! adds `<sid>.questions.json`, the questions its last replies asked and the
//! passages each is about, for the question colours (questions.rs). The
//! session is the `claude_session` user var Terminal's claudehook.exe sets in
//! its pane.
//!
//! The folder holds a thousand sessions and more, so only the ones asked for
//! are read, and never on the GUI thread: [`Summaries::get`] answers from
//! memory and asks a thread of its own to read a session it hasn't seen. That
//! thread watches the folder and reads a session again when one of its files
//! changes; each change bumps [`Summaries::generation`] and calls the wake the
//! GUI gave it, so the windows repaint.
//!
//! The same thread follows `expanded.flag` in the folder, which Terminal's
//! Alt+S (terminal.ahk) toggles to show each summary's detail.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use crate::questions;
use crate::restore::is_session_id;

/// The most of a label kept: the rail shows two short lines of it
pub const LABEL_MAX: usize = 24;
/// There while the summaries' detail is shown (Alt+S)
pub const EXPANDED_FLAG: &str = "expanded.flag";
/// How long a burst of file events settles before the sessions are read
const SETTLE: Duration = Duration::from_millis(60);

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
}

/// Which of a session's files a file is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Part {
    Short,
    Detail,
    Label,
    Cwd,
    Questions,
}

/// The session a file in the folder belongs to, and which of its files it is.
pub fn parse_name(name: &str) -> Option<(&str, Part)> {
    [
        (".detail.txt", Part::Detail),
        (".label.txt", Part::Label),
        (".cwd.txt", Part::Cwd),
        (".questions.json", Part::Questions),
        (".txt", Part::Short),
    ]
        .into_iter()
        .find_map(|(suffix, part)| {
            name.strip_suffix(suffix)
                .filter(|sid| is_session_id(sid))
                .map(|sid| (sid, part))
        })
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
    let file = |suffix: &str| dir.join(format!("{sid}{suffix}"));
    let text = |suffix: &str| std::fs::read_to_string(file(suffix)).ok();
    let short_path = file(".txt");
    Summary {
        label: text(".label.txt").as_deref().and_then(clean_label),
        short: text(".txt").as_deref().and_then(clean_short),
        written: std::fs::metadata(&short_path).and_then(|m| m.modified()).ok(),
        detail: text(".detail.txt").as_deref().map(detail_lines).unwrap_or_default(),
        cwd: text(".cwd.txt")
            .as_deref()
            .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(String::from),
        questions: text(".questions.json")
            .as_deref()
            .and_then(questions::parse)
            .filter(|q| !q.messages.is_empty())
            .map(Arc::new),
    }
}

enum Msg {
    /// Read this session, asked for the first time
    Read(String),
    /// Something changed in the folder
    Changed(Vec<PathBuf>),
}

struct Shared {
    known: Mutex<HashMap<String, Summary>>,
    asked: Mutex<HashSet<String>>,
    expanded: AtomicBool,
    generation: AtomicU64,
    wake: Box<dyn Fn() + Send + Sync>,
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
    /// whenever a session asked for changes.
    pub fn start(dir: PathBuf, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            known: Mutex::new(HashMap::new()),
            asked: Mutex::new(HashSet::new()),
            expanded: AtomicBool::new(dir.join(EXPANDED_FLAG).exists()),
            generation: AtomicU64::new(0),
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

    /// What's known of session `sid`. The first time a session is asked
    /// for, this is `None` and it's read on the watching thread, which then
    /// wakes the GUI.
    pub fn get(&self, sid: &str) -> Option<Summary> {
        if let Some(summary) = lock(&self.shared.known).get(sid) {
            return Some(summary.clone());
        }
        if is_session_id(sid) && lock(&self.shared.asked).insert(sid.to_string()) {
            let _ = lock(&self.tx).send(Msg::Read(sid.to_string()));
        }
        None
    }

    /// Bumped each time a summary asked for changes, or [`Self::expanded`].
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    /// Whether the summaries' detail is to be shown: `expanded.flag` is in
    /// the folder.
    pub fn expanded(&self) -> bool {
        self.shared.expanded.load(Ordering::Acquire)
    }
}

/// The watching thread: reads the sessions asked for, and reads one again
/// when its files change.
fn watch(dir: PathBuf, shared: Arc<Shared>, rx: Receiver<Msg>, events: Sender<Msg>) {
    use notify::Watcher;
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = events.send(Msg::Changed(event.paths));
        }
    });
    match watcher.as_mut() {
        Ok(w) => {
            if let Err(err) = w.watch(&dir, notify::RecursiveMode::NonRecursive) {
                log::warn!("summaries: can't watch {}: {err:#}", dir.display());
            }
        }
        Err(err) => log::warn!("summaries: no watcher: {err:#}"),
    }

    while let Ok(first) = rx.recv() {
        let mut due = HashSet::new();
        let mut flag = false;
        let mut take = |msg: Msg| match msg {
            Msg::Read(sid) => {
                due.insert(sid);
            }
            Msg::Changed(paths) => {
                let asked = lock(&shared.asked);
                for path in paths {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == EXPANDED_FLAG {
                        flag = true;
                    } else if let Some((sid, _)) = parse_name(name).filter(|(sid, _)| asked.contains(*sid)) {
                        due.insert(sid.to_string());
                    }
                }
            }
        };
        take(first);
        // A worker writes two or three files at once: read them together
        std::thread::sleep(SETTLE);
        while let Ok(msg) = rx.try_recv() {
            take(msg);
        }

        let mut changed = false;
        if flag {
            let expanded = dir.join(EXPANDED_FLAG).exists();
            changed |= shared.expanded.swap(expanded, Ordering::AcqRel) != expanded;
        }
        for sid in due {
            let summary = read(&dir, &sid);
            let mut known = lock(&shared.known);
            if known.get(&sid) != Some(&summary) {
                changed |= summary != Summary::default() || known.contains_key(&sid);
                known.insert(sid, summary);
            }
        }
        if changed {
            shared.generation.fetch_add(1, Ordering::AcqRel);
            (shared.wake)();
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
        assert_eq!(parse_name(&format!("{SID}.json")), None);
        assert_eq!(parse_name(&format!("{SID}.worker")), None);
        assert_eq!(parse_name("expanded.flag"), None);
        assert_eq!(parse_name("notes.txt"), None);
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

    #[test]
    fn sessions_asked_for_are_read_off_thread_and_kept_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let label = |sid: &str| dir.path().join(format!("{sid}.label.txt"));
        std::fs::write(label(SID), "Tab bar bugs").unwrap();
        std::fs::write(label(OTHER), "Lodestone").unwrap();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let summaries = Summaries::start(dir.path().to_path_buf(), move || {
            w.fetch_add(1, Ordering::SeqCst);
        });

        // Not known yet: asked for, and read on the other thread
        assert_eq!(summaries.get(SID), None);
        assert!(until(|| summaries.get(SID).and_then(|s| s.label).as_deref() == Some("Tab bar bugs")));
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let generation = summaries.generation();

        // A new label is picked up when the worker writes it
        std::fs::write(label(SID), "Rail labels").unwrap();
        assert!(until(|| summaries.get(SID).and_then(|s| s.label).as_deref() == Some("Rail labels")));
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
        assert!(until(|| summaries.get(empty) == Some(Summary::default())));
        std::fs::write(dir.path().join(format!("{empty}.txt")), "first summary").unwrap();
        assert!(until(|| summaries.get(empty).and_then(|s| s.short).as_deref() == Some("first summary")));
    }

    #[test]
    fn alt_s_flag_is_followed() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(EXPANDED_FLAG);
        std::fs::write(&flag, "").unwrap();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let summaries = Summaries::start(dir.path().to_path_buf(), move || {
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
}
