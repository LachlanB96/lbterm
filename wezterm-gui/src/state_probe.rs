//! LBTerm: the state file for test windows (lbterm/STATE.md). With `LBTERM_STATE_FILE` set,
//! every window's paint ends by saying what it shows ([`publish`], termwindow/state.rs gathers
//! it), and the whole process's windows go to the file from a thread of its own
//! (lbterm_extras::state::Writer), the newest snapshot winning. Unset, [`on`] is false and
//! nothing is gathered: the variable is read once, the first time anything asks.

use lbterm_extras::state::{self, Snapshot, Window, Writer};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::SystemTime;

/// Frames painted by every window of this process
static FRAMES: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Each window as its last frame left it, by id: windows live on the GUI thread
    static WINDOWS: RefCell<BTreeMap<u64, Window>> = RefCell::new(BTreeMap::new());
}

/// Whether there's a state file to write.
pub fn on() -> bool {
    state::path().is_some()
}

fn writer() -> Option<&'static Writer> {
    static WRITER: OnceLock<Option<Writer>> = OnceLock::new();
    WRITER
        .get_or_init(|| {
            let path = state::path()?;
            match Writer::spawn(path.to_path_buf()) {
                Ok(writer) => {
                    log::info!("state file: {}", path.display());
                    Some(writer)
                }
                Err(err) => {
                    log::error!("state file {}: no writer thread: {err:#}", path.display());
                    None
                }
            }
        })
        .as_ref()
}

/// A window painted a frame: the process's count of them, after it.
pub fn painted() -> u64 {
    FRAMES.fetch_add(1, Ordering::Relaxed) + 1
}

/// `window` as it is now, written with the others.
pub fn publish(window: Window) {
    WINDOWS.with(|windows| {
        windows.borrow_mut().insert(window.id, window);
    });
    submit();
}

/// The window `id` has gone.
pub fn forget(id: u64) {
    let was = WINDOWS.with(|windows| windows.borrow_mut().remove(&id).is_some());
    if was {
        submit();
    }
}

fn submit() {
    let Some(writer) = writer() else {
        return;
    };
    let windows = WINDOWS.with(|windows| windows.borrow().values().cloned().collect());
    writer.submit(Snapshot {
        version: state::VERSION,
        pid: std::process::id(),
        build: config::wezterm_version().to_string(),
        frame: FRAMES.load(Ordering::Relaxed),
        time_ms: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        windows,
    });
}
