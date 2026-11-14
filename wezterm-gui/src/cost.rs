//! LBTerm: what each Claude Code conversation has cost and what spent it, for the cost pane the
//! context pill's press opens (render/cost.rs; lbterm/CONTEXT.md, "The cost pane"; Lachlan's 7c
//! of 2026-11-09), from one of two places, as `lbterm_context.count_cost` says:
//!
//! - Off (the default; Lachlan's 2b of 2026-11-10): the Claude project's split, `@Claude:cost`,
//!   `<sid>.cost.json` in the summaries folder (`lbterm_summaries`), which Terminal's statusline
//!   writes whole beside `<sid>.usage.json` (lbterm_extras::cost::read_published). One file read
//!   whole a time; no transcript is read and nothing is counted, and there's no falling back to
//!   the own count: its absence says the split isn't there yet.
//! - On, for someone without the Claude project's: LBTerm's own count, which finds the session's
//!   transcript and its subagents' and reads only what each has gained since it last read it
//!   (`Fold`, kept for the session while LBTerm runs).
//!
//! Either way on a thread of its own, `lbterm-cost`, started at the first press and then kept:
//! the GUI thread asks for a session's split ([`want`]), the thread reads or counts it and, only
//! when its answer differs from the one kept, stores it and wakes the windows that asked and the
//! panes open on that session; [`get`] answers from memory. Nothing is read while no pane is open,
//! and an opening where nothing moved paints nothing new (round 5's stage 2; lbterm/FLOWS.md, the
//! cost pane):
//!
//! - Off, the split is asked for only when its file was written since it was last asked for: the
//!   summaries thread hears every write of `<sid>.cost.json` and reads none
//!   (crate::summaries::split_written), so an opening on a split that hasn't been written since
//!   asks nothing, and a write while a pane is open on it asks at once ([`split_written`], from
//!   that thread), with no frame of its own.
//! - On, nothing hears the transcripts while the pane is shut, so it asks as it opens and again
//!   when the pill's dollars move while it's open, and the thread, finding no transcript moved
//!   (`Fold::read`), answers nothing.
//!
//! The thread is one way's for its life: an ask the other way, or a reload that turns the count
//! on or off, ends it once it has answered what it holds (and its folds with it) and drops every
//! answer kept ([`configured`]).

use lbterm_extras::cost::{self, Cost, Fold, Spender};
use mux::window::WindowId as MuxWindowId;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use window::WindowOps;

/// A session's split, or why there's none
#[derive(Debug, Clone)]
pub enum Counted {
    /// LBTerm's own count
    Cost(Arc<Cost>),
    /// Why the own count has none
    Failed(Arc<str>),
    /// The Claude project's split
    Split(Arc<Cost>),
    /// Why there's no split from the Claude project (not published yet for this conversation,
    /// a version this doesn't read, what went wrong)
    Unpublished(Arc<str>),
}

impl Counted {
    /// The own count's answer, not the Claude project's
    fn counts(&self) -> bool {
        matches!(self, Counted::Cost(_) | Counted::Failed(_))
    }

    /// The same answer, by what it says: a pane laid out from one is the pane the other makes
    fn same(&self, other: &Counted) -> bool {
        match (self, other) {
            (Counted::Cost(a), Counted::Cost(b)) | (Counted::Split(a), Counted::Split(b)) => Arc::ptr_eq(a, b) || a == b,
            (Counted::Failed(a), Counted::Failed(b)) | (Counted::Unpublished(a), Counted::Unpublished(b)) => a == b,
            _ => false,
        }
    }
}

/// Where a split comes from
#[derive(Debug, Clone, PartialEq)]
enum Source {
    /// LBTerm's own count, from this transcripts folder
    Count(PathBuf),
    /// The Claude project's `<sid>.cost.json` in this summaries folder
    Published(PathBuf),
}

impl Source {
    fn counts(&self) -> bool {
        matches!(self, Source::Count(_))
    }
}

/// Where a split is to come from with the count `on`: the transcripts folder for the count, the
/// summaries folder for the Claude project's (none without the folder it needs)
fn source(on: bool, transcripts: Option<PathBuf>, summaries: Option<PathBuf>) -> Option<Source> {
    if on {
        transcripts.map(Source::Count)
    } else {
        summaries.map(Source::Published)
    }
}

struct Job {
    sid: String,
    source: Source,
    window: MuxWindowId,
}

static COUNTS: Mutex<Option<HashMap<String, Counted>>> = Mutex::new(None);
static COUNTER: Mutex<Counter> = Mutex::new(Counter { queue: None, counting: None });
/// With the count off, how many times each session's `<sid>.cost.json` had been heard written when
/// its split was last asked for (crate::summaries::split_written): asked again only once it moves
static ASKED: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
/// The cost panes open with the count off, by window, and the session each shows: a write of that
/// session's split asks for it for them ([`split_written`]), and its answer repaints them
static SHOWING: Mutex<Vec<(MuxWindowId, String)>> = Mutex::new(Vec::new());

/// The thread's end of the channel: none until the first ask, and none again once a reload
/// turns the count on or off
struct Counter {
    queue: Option<Sender<Job>>,
    /// Which way what's kept is (the thread and every answer): counting, or the Claude project's
    counting: Option<bool>,
}

impl Counter {
    /// What's kept is to be `on`'s way: kept the other way, the thread's channel is dropped, so
    /// it ends once it has answered what it holds. True when it was the other way's, and every
    /// answer kept is to go (the caller drops them: `counts`)
    fn switch(&mut self, on: bool) -> bool {
        if self.counting == Some(on) {
            return false;
        }
        self.queue = None;
        self.counting = Some(on);
        true
    }

    /// Ask for `sid`'s split from `source`, the thread started at the first ask. True when the
    /// ask went to the thread
    fn ask(&mut self, source: Source, sid: &str, window: MuxWindowId) -> bool {
        if self.switch(source.counts()) {
            forget();
        }
        if self.queue.is_none() {
            let (tx, rx) = channel();
            if let Err(e) = std::thread::Builder::new().name("lbterm-cost".into()).spawn(move || run(rx)) {
                log::error!("cost: couldn't start its thread: {e:#}");
                return false;
            }
            self.queue = Some(tx);
        }
        self.queue.as_ref().map_or(false, |tx| tx.send(Job { sid: sid.to_string(), source, window }).is_ok())
    }

    /// The thread's channel dropped, so it ends once it has answered what it holds, and its reads
    /// with it
    fn stop(&mut self) {
        self.queue = None;
        self.counting = None;
    }
}

fn counter() -> MutexGuard<'static, Counter> {
    COUNTER.lock().unwrap_or_else(|e| e.into_inner())
}

fn counts() -> MutexGuard<'static, Option<HashMap<String, Counted>>> {
    COUNTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn asked() -> MutexGuard<'static, Option<HashMap<String, u64>>> {
    ASKED.lock().unwrap_or_else(|e| e.into_inner())
}

fn showing_panes() -> MutexGuard<'static, Vec<(MuxWindowId, String)>> {
    SHOWING.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every answer kept goes, and what each split was asked for at with them, so the next look asks
fn forget() {
    counts().take();
    asked().take();
}

/// With the count off, whether a look asks for the split again: the session's `<sid>.cost.json`
/// heard written `written` times, and its split last asked for at `asked` (`None`: never, or
/// forgotten since by a switch of the count or a reload). It asks unless it was asked for at that
/// very count, so a split written while the pane was shut is read at its next opening
fn asks(asked: Option<u64>, written: u64) -> bool {
    asked != Some(written)
}

/// The count session `sid`'s split was last asked for at, if it's still kept ([`ASKED`])
fn asked_at(sid: &str) -> Option<u64> {
    asked().as_ref().and_then(|a| a.get(sid).copied())
}

/// Session `sid`'s last split from memory, the own count's with the count `on`, the Claude
/// project's with it off: `None` until one has been asked for and answered that way
pub fn get(sid: &str, on: bool) -> Option<Counted> {
    counts().as_ref()?.get(sid).filter(|c| c.counts() == on).cloned()
}

/// Session `sid`'s split, for a pane open in `window`: with the count on, counted again (only what
/// its transcripts gained since the last); with it off, read again only when its file has been
/// written since it was last asked for, so a look while nothing was written asks nothing. The
/// answer repaints `window` once it's in, and only when it differs from the one kept. Without the
/// folder it needs (the conversation reader's transcripts folder for the count, `lbterm_summaries`
/// for the Claude project's), the answer is why, at once
pub fn want(config: &config::Config, sid: &str, window: MuxWindowId) {
    let on = config::lbterm::counts_cost(config);
    let mut counter = counter();
    if counter.switch(on) {
        forget();
    }
    // The count off: written since it was asked for, or never asked for
    let written = (!on).then(|| crate::summaries::split_written(sid));
    if let Some(written) = written {
        if !asks(asked_at(sid), written) {
            return;
        }
    }
    let transcripts = if on { crate::conversations::transcripts_folder() } else { None };
    let summaries = config.lbterm_summaries.as_deref().filter(|d| !d.is_empty()).map(PathBuf::from);
    let source = source(on, transcripts, summaries);
    if let Some(written) = written {
        asked().get_or_insert_with(HashMap::new).insert(sid.to_string(), written);
    }
    match source {
        Some(source) => {
            counter.ask(source, sid, window);
        }
        None => {
            let why: Arc<str> = if on {
                "No transcripts folder to count this conversation from".into()
            } else {
                cost::NO_SUMMARIES.into()
            };
            let answer = if on { Counted::Failed(why) } else { Counted::Unpublished(why) };
            counts().get_or_insert_with(HashMap::new).insert(sid.to_string(), answer);
        }
    }
}

/// The cost pane in `window` is open on session `sid` with the count off, or (`None`) isn't: the
/// panes a write of the split asks for it for ([`split_written`])
pub fn showing(window: MuxWindowId, sid: Option<&str>) {
    let mut panes = showing_panes();
    panes.retain(|(w, _)| *w != window);
    if let Some(sid) = sid {
        panes.push((window, sid.to_string()));
    }
}

/// These sessions' `<sid>.cost.json` was written (from the summaries thread, which reads none of
/// it): the split is asked for again for each pane open on one of them, so the pane follows the
/// file's own write, at once and with no frame of its own. Nothing runs while no pane is open on
/// one of them
pub fn split_written(sids: &[String]) {
    let open: Vec<(MuxWindowId, String)> = showing_panes().iter().filter(|(_, sid)| sids.contains(sid)).cloned().collect();
    if open.is_empty() {
        return;
    }
    let config = config::configuration();
    for (window, sid) in open {
        want(&config, &sid, window);
    }
}

/// The config was read again: with the count turned on or off, the thread ends and every answer
/// kept goes, the other way's folds with them
pub fn configured(config: &config::Config) {
    let on = config::lbterm::counts_cost(config);
    let mut counter = counter();
    if counter.counting.is_some_and(|was| was != on) {
        counter.stop();
        counts().take();
    }
    // What each split was asked for at goes, so it's read once more: the summaries folder may be
    // another, whose writes are counted from none (a reload is rare, and an answer the same as the
    // one kept repaints nothing)
    asked().take();
}

/// What's been read of one session's transcripts
#[derive(Default)]
struct Counting {
    main: Option<(PathBuf, Fold)>,
    agents: BTreeMap<PathBuf, (Spender, Option<String>, Fold)>,
}

fn run(rx: Receiver<Job>) {
    let mut sessions: HashMap<String, Counting> = HashMap::new();
    while let Ok(job) = rx.recv() {
        // Every ask waiting is answered by one read or count a session
        let mut asked: BTreeMap<String, (Source, Vec<MuxWindowId>)> = BTreeMap::new();
        for job in std::iter::once(job).chain(rx.try_iter()) {
            let Job { sid, source, window } = job;
            let entry = asked.entry(sid).or_insert_with(|| (source, vec![]));
            if !entry.1.contains(&window) {
                entry.1.push(window);
            }
        }
        for (sid, (source, mut windows)) in asked {
            let kept = counts().as_ref().and_then(|c| c.get(&sid).cloned());
            // The same as the answer kept: nothing stored, nothing repainted
            let Some(counted) = answer(&mut sessions, &source, &sid, kept.as_ref()) else {
                continue;
            };
            counts().get_or_insert_with(HashMap::new).insert(sid.clone(), counted);
            for (window, _) in showing_panes().iter().filter(|(_, s)| *s == sid) {
                if !windows.contains(window) {
                    windows.push(*window);
                }
            }
            promise::spawn::spawn_into_main_thread(async move { repaint(&windows) }).detach();
        }
    }
}

/// `sid`'s split from `source`: the Claude project's file read whole, which touches nothing of
/// `sessions` (the own count's folds), or the own count read on from them. `None` when it's the
/// answer `kept` already: the count's folds took nothing since it was made, or it says the same
fn answer(sessions: &mut HashMap<String, Counting>, source: &Source, sid: &str, kept: Option<&Counted>) -> Option<Counted> {
    let counted = match source {
        Source::Published(folder) => match cost::read_published(folder, sid) {
            Ok(split) => Counted::Split(Arc::new(split)),
            Err(why) => Counted::Unpublished(why.into()),
        },
        Source::Count(projects) => {
            let c = sessions.entry(sid.to_string()).or_default();
            match read_on(c, projects, sid) {
                // Every transcript where it was: the count kept is this one's (only this thread
                // makes one, from these folds)
                Ok(false) if matches!(kept, Some(Counted::Cost(_))) => return None,
                Ok(_) => Counted::Cost(Arc::new(count(c))),
                Err(why) => Counted::Failed(why.into()),
            }
        }
    };
    match kept {
        Some(kept) if kept.same(&counted) => None,
        _ => Some(counted),
    }
}

/// Read on in the session's transcript and its subagents': true when any of them moved (a line
/// taken, a file written again, a transcript or a subagent not read before), so the count is to be
/// made again
fn read_on(c: &mut Counting, projects: &Path, sid: &str) -> Result<bool, String> {
    let main = match c.main.as_ref().filter(|(p, _)| p.exists()) {
        Some((p, _)) => p.clone(),
        None => lbterm_extras::conversation::find_transcript(projects, sid)
            .ok_or_else(|| format!("No transcript for this conversation in {}", projects.display()))?,
    };
    let mut moved = false;
    if c.main.as_ref().map(|(p, _)| p) != Some(&main) {
        c.main = Some((main.clone(), Fold::default()));
        c.agents.clear();
        moved = true;
    }
    let Some((_, fold)) = c.main.as_mut() else {
        return Err("No transcript".into());
    };
    moved |= fold.read(&main).map_err(|e| format!("Couldn't read the transcript: {e}"))?;
    let dir = main.with_extension("");
    for path in cost::agent_transcripts(&main) {
        let entry = c.agents.entry(path.clone()).or_insert_with(|| {
            moved = true;
            let (spender, workflow) = cost::spender_of(path.strip_prefix(&dir).unwrap_or(&path));
            (spender, workflow, Fold::default())
        });
        // One subagent's transcript that can't be read is left at what was read of it, and said
        match entry.2.read(&path) {
            Ok(took) => moved |= took,
            Err(e) => log::warn!("cost: couldn't read {}: {e}", path.display()),
        }
    }
    Ok(moved)
}

/// The count, from what's been read of the session's transcripts
fn count(c: &Counting) -> Cost {
    let none = Fold::default();
    let main = c.main.as_ref().map_or(&none, |(_, f)| f);
    let agents: Vec<(Spender, Option<String>, &Fold)> = c.agents.values().map(|(s, w, f)| (*s, w.clone(), f)).collect();
    cost::cost(main, &agents)
}

fn repaint(windows: &[MuxWindowId]) {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    for win in fe.gui_windows().iter().filter(|w| windows.contains(&w.mux_window_id)) {
        win.window.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests that go through the statics every window shares (`want`, `get`, `configured`,
    /// `forget`, `showing`) take this first, so one's reload can't forget what another kept
    static GLOBALS: Mutex<()> = Mutex::new(());

    fn globals() -> MutexGuard<'static, ()> {
        GLOBALS.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// With the count off a split comes only from the Claude project's file in the summaries
    /// folder, whatever transcripts folder there is; on, only from the transcripts; and from
    /// neither without the folder it needs (lbterm/CONTEXT.md, "The cost pane")
    #[test]
    fn the_count_off_reads_the_claude_projects_file_and_on_counts_the_transcripts() {
        let (t, s) = (PathBuf::from("projects"), PathBuf::from("summaries"));
        assert_eq!(source(false, Some(t.clone()), Some(s.clone())), Some(Source::Published(s.clone())));
        assert_eq!(source(false, Some(t.clone()), None), None);
        assert_eq!(source(true, Some(t.clone()), Some(s.clone())), Some(Source::Count(t.clone())));
        assert_eq!(source(true, None, Some(s)), None);
    }

    /// Reading the Claude project's split counts nothing and keeps no fold: the own count's
    /// sessions stay empty, and its absence is an answer that says so, never $0
    #[test]
    fn a_published_answer_reads_no_transcript() {
        let sid = "5d2c8e17-3a41-4b6f-9c0e-2f7a1b8d4e60";
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lbterm-extras/src/cost/published");
        let mut sessions = HashMap::new();
        match answer(&mut sessions, &Source::Published(folder), sid, None) {
            Some(Counted::Split(c)) => assert_eq!(c.total(), 0.67),
            other => panic!("not the split: {:?}", other),
        }
        let empty = std::env::temp_dir().join("lbterm-cost-test-none");
        match answer(&mut sessions, &Source::Published(empty), sid, None) {
            Some(Counted::Unpublished(why)) => assert_eq!(&*why, cost::NOT_PUBLISHED),
            other => panic!("not unpublished: {:?}", other),
        }
        assert!(sessions.is_empty(), "the own count's folds were touched");
    }

    /// The Claude project's split read again unchanged is no answer: nothing stored and nothing
    /// repainted, so an ask for a file written with the same split paints nothing new; a split
    /// that says something else, or its absence, is (round 5's stage 2)
    #[test]
    fn a_split_the_same_as_the_one_kept_is_no_answer() {
        let sid = "5d2c8e17-3a41-4b6f-9c0e-2f7a1b8d4e60";
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lbterm-extras/src/cost/published");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{sid}.cost.json"));
        std::fs::copy(fixture.join(format!("{sid}.cost.json")), &file).unwrap();
        let source = Source::Published(dir.path().to_path_buf());
        let mut sessions = HashMap::new();
        let kept = answer(&mut sessions, &source, sid, None).expect("the first read is an answer");
        assert!(answer(&mut sessions, &source, sid, Some(&kept)).is_none(), "the same split again");
        // Written again with the same split: a new file, the same answer
        let text = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, &text).unwrap();
        assert!(answer(&mut sessions, &source, sid, Some(&kept)).is_none(), "written again the same");
        // Another total is another answer
        std::fs::write(&file, text.replacen("\"usd\":0.67", "\"usd\":0.68", 1)).unwrap();
        match answer(&mut sessions, &source, sid, Some(&kept)) {
            Some(Counted::Split(c)) => assert_eq!(c.total(), 0.68),
            other => panic!("not the new split: {:?}", other),
        }
        std::fs::remove_file(&file).unwrap();
        match answer(&mut sessions, &source, sid, Some(&kept)) {
            Some(Counted::Unpublished(why)) => assert_eq!(&*why, cost::NOT_PUBLISHED),
            other => panic!("not unpublished: {:?}", other),
        }
        let gone = Counted::Unpublished(cost::NOT_PUBLISHED.into());
        assert!(answer(&mut sessions, &source, sid, Some(&gone)).is_none(), "still not published");
    }

    /// The own count asked again with no transcript moved answers nothing, and never makes the
    /// count again; a line that spends nothing moves the folds but makes the same count, no
    /// answer either; a request, and a subagent's first, are answers (round 5's stage 2)
    #[test]
    fn the_count_asked_again_answers_only_what_moved() {
        use std::io::Write as _;
        let sid = "7a4e1c39-5d63-4e81-9f20-4b9c3d0a6f82";
        let projects = tempfile::tempdir().unwrap();
        let folder = projects.path().join("C--work");
        std::fs::create_dir_all(folder.join(sid).join("subagents")).unwrap();
        let main = folder.join(format!("{sid}.jsonl"));
        let request = |id: &str| format!(r#"{{"type":"assistant","timestamp":"2026-11-14T10:00:00Z","message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":1000,"output_tokens":100}}}}}}"#);
        std::fs::write(&main, format!("{}
", request("m1"))).unwrap();
        let append = |path: &Path, line: &str| {
            let mut f = std::fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
            writeln!(f, "{line}").unwrap();
        };
        let source = Source::Count(projects.path().to_path_buf());
        let mut sessions = HashMap::new();
        let first = answer(&mut sessions, &source, sid, None).expect("the first count");
        let Counted::Cost(ref c1) = first else { panic!("{:?}", first) };
        assert!(answer(&mut sessions, &source, sid, Some(&first)).is_none(), "nothing moved");
        append(&main, r#"{"type":"user","message":{"content":"spends nothing"}}"#);
        assert!(answer(&mut sessions, &source, sid, Some(&first)).is_none(), "moved, the same count");
        append(&main, &request("m2"));
        let second = answer(&mut sessions, &source, sid, Some(&first)).expect("a request");
        let Counted::Cost(ref c2) = second else { panic!("{:?}", second) };
        assert!(c2.total() > c1.total());
        append(&folder.join(sid).join("subagents").join("agent-a1.jsonl"), &request("a1"));
        match answer(&mut sessions, &source, sid, Some(&second)) {
            Some(Counted::Cost(c3)) => assert_eq!(c3.part(Spender::Subagents).runs, 1),
            other => panic!("not the subagent's: {:?}", other),
        }
    }

    /// With the count off, a look with nothing written since keeps its answer: the default
    /// config has no summaries folder (so no write is ever heard), and the answer is why, made
    /// once and kept (the same `Arc` at the next look, `ASKED` holding the count it was asked for
    /// at), and made again only once a reload has dropped `ASKED`. The split moving between two
    /// looks is `asks`'s (below), since no write is heard here
    #[test]
    fn the_count_off_keeps_its_answer_till_a_reload_forgets_what_it_was_asked_for_at() {
        let _globals = globals();
        let config = config::Config::default_config();
        let sid = "8b5f2d4a-6e74-4f92-a031-5cad4e1b7093";
        want(&config, sid, 0);
        let Some(Counted::Unpublished(first)) = get(sid, false) else { panic!("no answer") };
        want(&config, sid, 0);
        let Some(Counted::Unpublished(again)) = get(sid, false) else { panic!("no answer") };
        assert!(Arc::ptr_eq(&first, &again), "asked again with nothing written");
        configured(&config);
        want(&config, sid, 0);
        let Some(Counted::Unpublished(after)) = get(sid, false) else { panic!("no answer") };
        assert!(!Arc::ptr_eq(&first, &after), "not asked again after a reload");
    }

    /// With the count off, the gate on a look: never asked for asks; asked for at the count heard
    /// now doesn't; a write heard since (the pane shut or open) does, so a split written while the
    /// pane was shut is read as it opens; and once a switch of the count or a reload has dropped
    /// what it was asked for at (`forget`, `configured`), it asks again (the review of 2026-11-14:
    /// the test above never moves the count)
    #[test]
    fn a_look_with_the_count_off_asks_only_once_the_split_was_written_since() {
        assert!(asks(None, 0), "never asked for, no write heard");
        assert!(asks(None, 3), "never asked for");
        assert!(!asks(Some(3), 3), "asked for at this count, nothing written since");
        assert!(asks(Some(3), 4), "written since it was asked for");
        assert!(asks(Some(3), 9), "written several times since");
        let _globals = globals();
        let sid = "4d7b1f3c-8a26-4e95-b0c4-7e1f2a9d3b58";
        let kept = |at: u64| {
            asked().get_or_insert_with(HashMap::new).insert(sid.to_string(), at);
        };
        kept(5);
        assert_eq!(asked_at(sid), Some(5));
        assert!(!asks(asked_at(sid), 5), "kept: nothing written since");
        assert!(asks(asked_at(sid), 6), "kept, written since");
        forget();
        assert_eq!(asked_at(sid), None);
        assert!(asks(asked_at(sid), 5), "forgotten by a switch of the count");
        kept(5);
        configured(&config::Config::default_config());
        assert!(asks(asked_at(sid), 5), "forgotten by a reload");
    }

    /// A pane open with the count off is told apart by window, and a write of a split no open pane
    /// shows asks for nothing
    #[test]
    fn a_split_written_asks_only_for_the_panes_open_on_it() {
        let _globals = globals();
        let sid = "9c6a3e5b-7f85-4a03-b142-6dbe5f2c81a4";
        showing(41, Some(sid));
        showing(42, Some(sid));
        showing(41, None);
        assert_eq!(
            showing_panes().iter().filter(|(_, s)| s == sid).map(|(w, _)| *w).collect::<Vec<_>>(),
            vec![42]
        );
        showing(42, None);
        assert!(!showing_panes().iter().any(|(_, s)| s == sid));
        // No pane open on it: nothing asked, no answer made
        split_written(&[sid.to_string()]);
        assert!(get(sid, false).is_none());
    }

    /// The thread is one way's: switched, its channel goes, so it ends at its next wait; an answer
    /// the other way never shows
    #[test]
    fn switching_the_count_ends_the_thread_and_answers_only_their_own_way() {
        let mut counter = Counter { queue: None, counting: None };
        let (tx, rx) = channel::<Job>();
        counter.queue = Some(tx);
        counter.counting = Some(true);
        assert!(!counter.switch(true));
        assert!(counter.queue.is_some(), "the same way keeps it");
        assert!(counter.switch(false));
        assert!(counter.queue.is_none() && counter.counting == Some(false));
        assert!(rx.recv().is_err(), "the thread's channel is closed once the count is off");
        assert!(Counted::Cost(Arc::new(Cost::default())).counts() && Counted::Failed("x".into()).counts());
        assert!(!Counted::Split(Arc::new(Cost::default())).counts() && !Counted::Unpublished("x".into()).counts());
    }

    /// The default config: the count off and no summaries folder, so no thread, and the answer
    /// is why
    #[test]
    fn the_default_config_starts_no_thread_and_says_there_is_no_summaries_folder() {
        let _globals = globals();
        let config = config::Config::default_config();
        assert!(!config::lbterm::counts_cost(&config));
        assert!(config.lbterm_summaries.is_none());
        let sid = "6e3d9f28-4b52-4c70-8d1f-3a8b2c9e5f71";
        want(&config, sid, 0);
        assert!(counter().queue.is_none(), "the default config started the thread");
        match get(sid, false) {
            Some(Counted::Unpublished(why)) => assert_eq!(&*why, cost::NO_SUMMARIES),
            other => panic!("{:?}", other),
        }
        assert!(get(sid, true).is_none(), "the other way's answer shown");
    }
}
