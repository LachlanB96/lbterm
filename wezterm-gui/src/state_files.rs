//! LBTerm: what the Conductor writes to its state file (`config.lbterm_conductor.state_file`,
//! lbterm_extras::conductor), kept here for its pad on the tab rail (render/conductor.rs) and
//! for the sessions it lists as hidden from it (its `hidden`, `Said::hidden`: the tab menu's Hide
//! from Conductor and the rail's mark, lbterm/NEW-TAB.md, section 8; read with the rest, so a
//! hide or a show is one more write of the file this thread already follows), and
//! the tab links' file (`config.lbterm_links.file`, lbterm_extras::links) for the rail's lines,
//! and the agenda's file (`config.lbterm_agenda.file`, lbterm_extras::agenda) for its button on
//! the strip (render/agenda.rs). A thread of its own watches each file through its folder (lbterm_extras::watch) and reads it when
//! Windows says it changed; when what it says changes, every window repaints. While nothing
//! changes it sleeps: until 2026-10-30 it looked at both files every 250 ms, so a click's change
//! showed up to a quarter of a second late, and it held the locks every rail paint takes while it
//! looked and read. Nothing is read on the GUI thread, and a lock is held only to put in what was
//! read. The thread starts the first time the rail asks, and follows the config as it's reloaded.

use lbterm_extras::agenda::{self, Agenda};
use lbterm_extras::conductor::{self, Said};
use lbterm_extras::links::{self, Links};
use lbterm_extras::watch::{Seen, Target, Watch};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, SystemTime};
use window::WindowOps;

/// A file written is several changes in a row (its writer's temporary file made, written and
/// renamed over it): they're taken together, this long after the first
const SETTLE: Duration = Duration::from_millis(25);
/// How many times the thread follows the files' folders before it reads them: a folder made can
/// have the next one under it already
const FOLLOWS: usize = 8;

/// What each file said when last read, by its path: handed out shared, since every rail paint
/// asks for it more than once
static SAID: Mutex<Option<HashMap<String, Arc<Said>>>> = Mutex::new(None);
/// The links file's path and what it said when last read
static LINKS: Mutex<Option<(String, Arc<Links>)>> = Mutex::new(None);
/// The agenda's file and what it said when last read: replaced only when what it says changes,
/// so a window that laid its list out from one can tell by the pointer
static AGENDA: Mutex<Option<(String, Arc<Agenda>)>> = Mutex::new(None);
static START: Once = Once::new();

/// What `file` says, as last read; nothing while there's no file
pub fn said(file: &str) -> Option<Arc<Said>> {
    START.call_once(start);
    SAID.lock().unwrap().as_ref()?.get(file).cloned()
}

/// The tab links, as `file` last said them; none while there's no file
pub fn links(file: &str) -> Arc<Links> {
    START.call_once(start);
    match LINKS.lock().unwrap().as_ref() {
        Some((read, links)) if read == file => links.clone(),
        _ => Arc::new(Links::default()),
    }
}

/// What the agenda's `file` says, as last read; nothing while there's no file, or before it's read
pub fn agenda(file: &str) -> Option<Arc<Agenda>> {
    START.call_once(start);
    match AGENDA.lock().unwrap().as_ref() {
        Some((read, agenda)) if read == file => Some(agenda.clone()),
        _ => None,
    }
}

/// Repaints every window, for what a file or the hub said. The frames are counted as asked for
/// by whoever called this, not by this (lbterm_extras::counters)
#[track_caller]
pub fn repaint_windows() {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    for win in fe.gui_windows() {
        win.window.invalidate();
    }
}

/// A file's size and time, or nothing while there's no file
type Stamp = Option<(SystemTime, u64)>;

fn stamp(file: &str) -> Stamp {
    let meta = std::fs::metadata(file).ok()?;
    Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len()))
}

/// The files the config names now: the Conductor's state files, the links file and the agenda's
fn files() -> (Vec<String>, Option<String>, Option<String>) {
    let config = config::configuration();
    let states = config.lbterm_conductor.iter().map(|c| c.state_file.clone()).collect();
    let links = config.lbterm_links.as_ref().map(|l| l.file.clone());
    let agenda = config.lbterm_agenda.as_ref().map(|a| a.file.clone());
    (states, links, agenda)
}

/// What wakes the thread
enum Wake {
    Seen(Seen),
    /// The config was reloaded: its files may be others
    Config,
}

fn start() {
    let spawned = std::thread::Builder::new()
        .name("lbterm-state-files".into())
        .spawn(|| {
            let (tx, rx) = channel();
            let seen = tx.clone();
            let mut watch = match Watch::new(move |s| {
                let _ = seen.send(Wake::Seen(s));
            }) {
                Ok(watch) => watch,
                Err(err) => {
                    log::error!("lbterm-state-files: can't watch the Conductor's state file, the links file and the agenda's, so the rail and the strip won't follow them: {err:#}");
                    return;
                }
            };
            let _reloads = config::subscribe_to_config_reload(move || tx.send(Wake::Config).is_ok());
            let mut seen: HashMap<String, Stamp> = HashMap::new();
            let mut links_seen: Option<(String, Stamp)> = None;
            let mut agenda_seen: Option<(String, Stamp)> = None;
            loop {
                let (states, links_file, agenda_file) = files();
                let targets: Vec<Target> = states
                    .iter()
                    .chain(links_file.iter())
                    .chain(agenda_file.iter())
                    .map(|f| Target::File(PathBuf::from(f)))
                    .collect();
                // Watched before they're read, so nothing written in between goes unseen
                for _ in 0..FOLLOWS {
                    if !watch.follow(&targets) {
                        break;
                    }
                }
                if look(&mut seen, &states)
                    | look_links(&mut links_seen, links_file)
                    | look_agenda(&mut agenda_seen, agenda_file)
                {
                    promise::spawn::spawn_into_main_thread(async { repaint_windows() }).detach();
                }

                // Asleep until one of them may have changed
                loop {
                    match rx.recv() {
                        Err(_) => return,
                        Ok(Wake::Config) => break,
                        Ok(Wake::Seen(Seen::Changed(paths))) if watch.matters(&paths) => break,
                        Ok(Wake::Seen(Seen::Changed(_))) => {}
                        Ok(Wake::Seen(Seen::Failed(err))) => {
                            log::error!("lbterm-state-files: the files' watch: {err}; looking at them again");
                            break;
                        }
                    }
                }
                std::thread::sleep(SETTLE);
                while rx.try_recv().is_ok() {}
            }
        });
    if let Err(err) = spawned {
        log::error!("starting the lbterm-state-files thread: {err:#}");
    }
}

/// Reads each of the state `files` that changed since `seen`: true when what any says did. A
/// file is looked at and read with no lock held; the lock is taken only to put in what it said.
fn look(seen: &mut HashMap<String, Stamp>, files: &[String]) -> bool {
    let mut read: Vec<(String, Option<Said>)> = vec![];
    for file in files {
        let now = stamp(file);
        if seen.get(file) == Some(&now) {
            continue;
        }
        seen.insert(file.clone(), now);
        if now.is_none() {
            read.push((file.clone(), None));
            continue;
        }
        // A file that doesn't read keeps what it said last: its program writes it whole, and
        // the next write is seen again
        match std::fs::read_to_string(file).ok().and_then(|text| conductor::read(&text)) {
            Some(new) => read.push((file.clone(), Some(new))),
            None => log::warn!("conductor: {file} isn't a state file"),
        }
    }
    let mut guard = SAID.lock().unwrap();
    let said = guard.get_or_insert_with(HashMap::new);
    let mut changed = false;
    for (file, new) in read {
        changed |= match new {
            Some(new) => {
                let differs = said.get(&file).map_or(true, |old| **old != new);
                if differs {
                    said.insert(file, Arc::new(new));
                }
                differs
            }
            None => said.remove(&file).is_some(),
        };
    }
    // A file taken out of the config
    let before = said.len();
    said.retain(|file, _| files.contains(file));
    changed |= said.len() != before;
    drop(guard);
    seen.retain(|file, _| files.contains(file));
    changed
}

/// Reads the links file when it changed since `seen` (the file and its stamp when last looked
/// at): true when what it says did. No file is no links; one that doesn't read keeps what it said
/// last, as the state file does. Looked at and read with no lock held, as `look` does
fn look_links(seen: &mut Option<(String, Stamp)>, file: Option<String>) -> bool {
    let Some(file) = file else {
        *seen = None;
        return LINKS.lock().unwrap().take().is_some();
    };
    let now = stamp(&file);
    if seen.as_ref() == Some(&(file.clone(), now)) {
        return false;
    }
    *seen = Some((file.clone(), now));
    let new = match now {
        None => Links::default(),
        Some(_) => match std::fs::read_to_string(&file).ok().and_then(|text| links::read(&text)) {
            Some(new) => new,
            None => {
                log::warn!("tab links: {file} isn't a links file");
                return false;
            }
        },
    };
    let mut held = LINKS.lock().unwrap();
    let changed = held.as_ref().map_or(true, |(f, old)| *f != file || **old != new);
    *held = Some((file, Arc::new(new)));
    changed
}

/// Reads the agenda's file when it changed since `seen`, as `look_links` does the links file: true
/// when what it says did. No file is no agenda, and the button goes; one that doesn't read keeps
/// what it said last (its program writes it whole, and the next write is seen again)
fn look_agenda(seen: &mut Option<(String, Stamp)>, file: Option<String>) -> bool {
    let Some(file) = file else {
        *seen = None;
        return AGENDA.lock().unwrap().take().is_some();
    };
    let now = stamp(&file);
    if seen.as_ref() == Some(&(file.clone(), now)) {
        return false;
    }
    *seen = Some((file.clone(), now));
    if now.is_none() {
        return AGENDA.lock().unwrap().take().is_some();
    }
    let Some(new) = std::fs::read_to_string(&file).ok().and_then(|text| agenda::read(&text)) else {
        log::warn!("agenda: {file} isn't an agenda");
        return false;
    };
    let mut held = AGENDA.lock().unwrap();
    let changed = held.as_ref().map_or(true, |(f, old)| *f != file || **old != new);
    if changed {
        *held = Some((file, Arc::new(new)));
    }
    changed
}
