//! LBTerm: each Claude Code session's conversation, for the scroll strip
//! (render/scroll_strip.rs) and the context pill's list of the files in it
//! (render/context.rs), read from its transcript on a thread of its own
//! (lbterm_extras::conversation) and shared by every window.
//!
//! A pane's session is its `claude_session` user var. Each window's strip, and
//! its context list while it's open, says which session's conversation it
//! shows ([`shows`]), and the reading thread follows those as their transcripts
//! grow; [`get`] answers from memory. Each says it for itself (a [`Reader`]), so
//! one saying none doesn't stop what the other shows being followed. When what's
//! read of a session changes, the windows showing it repaint, and no others:
//! until 2026-10-30 every window repainted for every session's growth.

use lbterm_extras::conversation::{Conversations, Snapshot};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use window::WindowOps;

/// The transcripts' folder and the summaries' (claudehook's permission records)
type Folders = (PathBuf, Option<PathBuf>);

/// What in a window reads a conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reader {
    /// The scroll strip, for the active pane's Claude Code in its full-screen view
    ScrollStrip,
    /// The context pill's list, while it's open
    Context,
}

struct Store {
    folders: Folders,
    conversations: Arc<Conversations>,
    /// The session each window's readers show
    shown: HashMap<(MuxWindowId, Reader), String>,
}

/// The store for the folders last configured
static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn lock() -> MutexGuard<'static, Option<Store>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The configured folders; `None` with neither `lbterm_scroll_strip` nor `lbterm_context` set,
/// the two that read the conversations (the context pill's list is the files in one,
/// render/context.rs). The scroll strip's transcripts folder wins, since one reader serves both
fn folders() -> Option<Folders> {
    let config = config::configuration();
    if config.lbterm_scroll_strip.is_none() && config.lbterm_context.is_none() {
        return None;
    }
    let named = |t: Option<&String>| t.map(String::as_str).filter(|d| !d.is_empty()).map(PathBuf::from);
    let projects = named(config.lbterm_scroll_strip.as_ref().and_then(|s| s.transcripts.as_ref()))
        .or_else(|| named(config.lbterm_context.as_ref().and_then(|c| c.transcripts.as_ref())))
        .unwrap_or_else(|| config::HOME_DIR.join(".claude").join("projects"));
    let summaries = config
        .lbterm_summaries
        .as_deref()
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    Some((projects, summaries))
}

/// The store for the configured folders, started the first time it's needed
/// and again if they change
fn store<'a>(guard: &'a mut MutexGuard<'static, Option<Store>>, folders: Folders) -> &'a mut Store {
    if guard.as_ref().map_or(true, |s| s.folders != folders) {
        let conversations = Arc::new(Conversations::start(folders.0.clone(), folders.1.clone(), |sid: &str| {
            let sid = sid.to_string();
            promise::spawn::spawn_into_main_thread(async move { repaint_showing(&sid) }).detach();
        }));
        **guard = Some(Store {
            folders,
            conversations,
            shown: HashMap::new(),
        });
    }
    guard.as_mut().expect("started above")
}

/// What's known of session `sid`'s conversation, if a reader is on: from
/// memory, `None` until it's been read (which it is once a window shows it)
pub fn get(sid: &str) -> Option<Arc<Snapshot>> {
    let folders = folders()?;
    let mut guard = lock();
    store(&mut guard, folders).conversations.get(sid)
}

/// Window `window`'s `reader` shows session `sid`'s conversation now, or none.
/// Each paint of the strip says so, and the context pill as its list opens and
/// shuts; only a change reaches the reading thread, with every session a pane
/// names, so a session not shown any more keeps what's been read of it while
/// its pane is there.
pub fn shows(window: MuxWindowId, reader: Reader, sid: Option<&str>) {
    let Some(folders) = folders() else {
        return;
    };
    let mut guard = lock();
    let store = store(&mut guard, folders);
    if store.shown.get(&(window, reader)).map(String::as_str) == sid {
        return;
    }
    match sid {
        Some(sid) => store.shown.insert((window, reader), sid.to_string()),
        None => store.shown.remove(&(window, reader)),
    };
    tell(store);
}

/// The reading thread is told what's shown now, and every session a pane names
fn tell(store: &Store) {
    let mut shown: Vec<String> = store.shown.values().cloned().collect();
    shown.sort();
    shown.dedup();
    let mut live: Vec<String> = Mux::try_get()
        .map(|mux| {
            mux.iter_panes()
                .iter()
                .filter_map(|p| p.copy_user_vars().remove("claude_session"))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    live.sort();
    live.dedup();
    store.conversations.show(shown, live);
}

/// The windows showing session `sid` repaint, for what's new in it. A window
/// that's gone is forgotten here, the only time it costs anything.
fn repaint_showing(sid: &str) {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    let windows = fe.gui_windows();
    let showing: Vec<MuxWindowId> = {
        let mut guard = lock();
        let Some(store) = guard.as_mut() else {
            return;
        };
        let alive: Vec<MuxWindowId> = windows.iter().map(|w| w.mux_window_id).collect();
        let before = store.shown.len();
        store.shown.retain(|(w, _), _| alive.contains(w));
        if store.shown.len() != before {
            tell(store);
        }
        store
            .shown
            .iter()
            .filter(|(_, s)| s.as_str() == sid)
            .map(|((w, _), _)| *w)
            .collect()
    };
    for win in windows.iter().filter(|w| showing.contains(&w.mux_window_id)) {
        win.window.invalidate();
    }
}
