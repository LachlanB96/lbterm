//! LBTerm: what the Conductor writes to its state file (`config.lbterm_conductor.state_file`,
//! lbterm_extras::conductor), kept here for its pad on the tab rail (render/conductor.rs). A
//! thread of its own looks at the file's size and time every [`POLL`] and reads it only when it
//! changed; when what it says changes, every window repaints. Nothing is read on the GUI thread.
//! The thread starts the first time the rail asks, and follows the config as it's reloaded.

use lbterm_extras::conductor::{self, Said};
use std::collections::HashMap;
use std::sync::{Mutex, Once};
use std::time::{Duration, SystemTime};
use window::WindowOps;

/// How often the file is looked at: a click's change shows within this
const POLL: Duration = Duration::from_millis(250);

/// What each file said when last read, by its path
static SAID: Mutex<Option<HashMap<String, Said>>> = Mutex::new(None);
static START: Once = Once::new();

/// What `file` says, as last read; nothing while there's no file
pub fn said(file: &str) -> Option<Said> {
    START.call_once(start);
    SAID.lock().unwrap().as_ref()?.get(file).cloned()
}

/// Repaints every window, for what a file or the hub said
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

fn start() {
    let spawned = std::thread::Builder::new()
        .name("lbterm-state-files".into())
        .spawn(|| {
            let mut seen: HashMap<String, Stamp> = HashMap::new();
            loop {
                if look(&mut seen) {
                    promise::spawn::spawn_into_main_thread(async { repaint_windows() }).detach();
                }
                std::thread::sleep(POLL);
            }
        });
    if let Err(err) = spawned {
        log::error!("starting the lbterm-state-files thread: {err:#}");
    }
}

/// Reads each configured file that changed since `seen`: true when what any says did
fn look(seen: &mut HashMap<String, Stamp>) -> bool {
    let files: Vec<String> = config::configuration()
        .lbterm_conductor
        .iter()
        .map(|c| c.state_file.clone())
        .collect();
    let mut changed = false;
    let mut guard = SAID.lock().unwrap();
    let said = guard.get_or_insert_with(HashMap::new);
    for file in &files {
        let now = stamp(file);
        if seen.get(file) == Some(&now) {
            continue;
        }
        seen.insert(file.clone(), now);
        if now.is_none() {
            changed |= said.remove(file).is_some();
            continue;
        }
        // A file that doesn't read keeps what it said last: its program writes it whole, and
        // the next write changes its time again
        match std::fs::read_to_string(file).ok().and_then(|text| conductor::read(&text)) {
            Some(new) => changed |= said.insert(file.clone(), new.clone()).as_ref() != Some(&new),
            None => log::warn!("conductor: {file} isn't a state file"),
        }
    }
    // A file taken out of the config
    seen.retain(|file, _| files.contains(file));
    let before = said.len();
    said.retain(|file, _| files.contains(file));
    changed | (said.len() != before)
}
