//! LBTerm: the context bar's Remove and Undo (lbterm/CONTEXT.md, "The buttons"; the edits are
//! lbterm_extras::context_edits). Where each section's removal stands, by the pane its
//! conversation runs in (the window's id for it), shared by every window, so each window showing
//! the pane draws it, and forgotten once the pane has closed. A press is the GUI thread's
//! (render/context_bar.rs), which only marks the section busy and hands the edit on: the file is
//! read and written on `lbterm-context-edits`, a thread started at the first press that takes the
//! edits one at a time in the order they were pressed (so a quick Remove then Undo can't cross),
//! and what it did comes back to the GUI thread, which repaints the windows. Nothing here runs
//! at rest: no press, no thread, and the bar asks only for [`generation`] each paint.
//!
//! Compact now's `/compact` is kept here too, by pane, with the session it was typed for and how
//! many compactions and replies its transcript had recorded then ([`compact_sent`]): every window
//! showing that pane dims Compact now with "Sent" until the reader has seen the compaction, or a
//! reply with no compaction (it was cancelled or failed, and a turn came after it), or the pane
//! runs another conversation ([`compact_waits`]), so neither a second look nor another window can
//! queue a second compaction while Claude Code is still on the first (lbterm/PITFALLS.md, 230).
//! Compact now writes nothing anywhere but `/compact` into the pane: no settings file, no rule
//! (Lachlan's 1b of 2026-11-10; lbterm/CONTEXT.md, "Compact now").

use crate::state_files::repaint_windows;
use lbterm_extras::context_bar::Removal;
use lbterm_extras::context_edits::{self, Act, State};
use mux::pane::PaneId;
use mux::Mux;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, MutexGuard};

/// An edit of a file, for the edits' thread
type Job = Box<dyn FnOnce() + Send>;

/// Compact now's `/compact`, typed into a pane
struct Sent {
    /// The session it's for, and the compactions its transcript had recorded when it was typed
    sid: String,
    at: u32,
    /// The replies its transcript had recorded then (`Context::replies`): more with no compaction
    /// means it was cancelled or failed, and a turn came after it
    replies: u32,
}

struct Store {
    /// One more for every change, so a window lays its bar out again only when one came
    generation: u64,
    panes: BTreeMap<PaneId, Vec<(Removal, State)>>,
    /// To the edits' thread, once it's started
    jobs: Option<Sender<Job>>,
    /// Compact now's `/compact`, by the pane it's typed into
    sent: BTreeMap<PaneId, Sent>,
}

static STORE: Mutex<Store> = Mutex::new(Store { generation: 0, panes: BTreeMap::new(), jobs: None, sent: BTreeMap::new() });

fn lock() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// How many changes there have been: a window's bar compares it to the one it laid out from
pub fn generation() -> u64 {
    lock().generation
}

/// Where each removal pressed in `pane` stands
pub fn states(pane: PaneId) -> Vec<(Removal, State)> {
    lock().panes.get(&pane).cloned().unwrap_or_default()
}

/// The thread the edits are made on: started at the first press, its end with the process's
fn start() -> Result<Sender<Job>, String> {
    let (tx, rx) = channel::<Job>();
    std::thread::Builder::new()
        .name("lbterm-context-edits".into())
        .spawn(move || {
            for job in rx {
                job();
            }
        })
        .map_err(|e| format!("Couldn't start the thread that edits the file: {e}"))?;
    Ok(tx)
}

/// Hands `job` to the edits' thread, started if it isn't
fn queue(store: &mut Store, job: Job) -> Result<(), String> {
    if store.jobs.is_none() {
        store.jobs = Some(start()?);
    }
    if store.jobs.as_ref().map_or(false, |tx| tx.send(job).is_ok()) {
        return Ok(());
    }
    store.jobs = None;
    Err("The thread that edits the file has stopped".to_string())
}

fn act_name(act: &Act) -> &'static str {
    match act {
        Act::Remove => "removed",
        Act::Undo(_) => "put back",
    }
}

/// A press of `removal`'s button in `pane`: Remove, or Undo once its line is out, nothing while an
/// edit of it is under way. The row says it's busy at once; the thread does the rest
pub fn press(pane: PaneId, removal: &Removal) {
    let mut store = lock();
    // The panes that have closed since, and what was pressed in them, go
    let mux = Mux::get();
    store.panes.retain(|id, _| mux.get_pane(*id).is_some());
    let now = store.panes.get(&pane).and_then(|list| list.iter().find(|(r, _)| r == removal)).map(|(_, s)| s.clone());
    let act = match now {
        None => Act::Remove,
        Some(state) => match state.press() {
            Some(act) => act,
            None => return,
        },
    };
    // An Undo that can't be made leaves the line out, and Undo offered again
    let out = match &act {
        Act::Undo(removed) => Some(removed.clone()),
        Act::Remove => None,
    };
    set(&mut store, pane, removal, State::busy(&act));
    let home = config::HOME_DIR.to_string_lossy().to_string();
    let edit = removal.clone();
    let job: Job = Box::new(move || {
        let state = context_edits::run(&act, &edit, &home);
        match &state {
            State::Refused { why, .. } => log::warn!("context bar: {}: {why}", context_edits::edited(&edit)),
            _ => log::info!("context bar: {}: a line {}", context_edits::edited(&edit), act_name(&act)),
        }
        promise::spawn::spawn_into_main_thread(async move { done(pane, edit, state) }).detach();
    });
    if let Err(why) = queue(&mut store, job) {
        log::error!("context bar: {why}");
        set(&mut store, pane, removal, State::Refused { why, removed: out });
    }
}

fn set(store: &mut Store, pane: PaneId, removal: &Removal, state: State) {
    let list = store.panes.entry(pane).or_default();
    match list.iter_mut().find(|(r, _)| r == removal) {
        Some(entry) => entry.1 = state,
        None => list.push((removal.clone(), state)),
    }
    store.generation += 1;
    promise::spawn::spawn_into_main_thread(async { repaint_windows() }).detach();
}

/// Compact now typed `/compact` into `pane` for session `sid`, whose transcript had recorded
/// `compactions` compactions and `replies` replies
pub fn compact_sent(pane: PaneId, sid: &str, compactions: u32, replies: u32) {
    let mut store = lock();
    let mux = Mux::get();
    store.sent.retain(|id, _| mux.get_pane(*id).is_some());
    store.sent.insert(pane, Sent { sid: sid.to_string(), at: compactions, replies });
}

/// A key of it didn't get there: nothing waits for a compaction
pub fn compact_unsent(pane: PaneId) {
    lock().sent.remove(&pane);
}

/// Whether a `/compact` typed into `pane` still waits for its compaction, `sid`'s transcript
/// having recorded `compactions` and `replies` now: forgotten once it has recorded a compaction
/// more, or a reply more with none (the compaction was cancelled with Esc, or its request failed,
/// and a turn came after it: a compaction's own summary request writes no assistant record, so a
/// compaction never moves `replies`), or once the pane runs another conversation
pub fn compact_waits(pane: PaneId, sid: &str, compactions: u32, replies: u32) -> bool {
    let mut store = lock();
    match store.sent.get(&pane) {
        Some(s) if s.sid == sid && compactions <= s.at && replies <= s.replies => true,
        Some(_) => {
            store.sent.remove(&pane);
            false
        }
        None => false,
    }
}

/// On the GUI thread: what the edits' thread did, kept while the pane's open
fn done(pane: PaneId, removal: Removal, state: State) {
    if Mux::get().get_pane(pane).is_none() {
        lock().panes.remove(&pane);
        return;
    }
    set(&mut lock(), pane, &removal, state);
}
