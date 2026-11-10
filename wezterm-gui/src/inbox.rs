//! LBTerm: the messages waiting in `config.lbterm_inbox`'s folders, kept here for the InMail
//! button on the window strip and the links to one message in the panes (render/inbox.rs;
//! lbterm/INBOX.md). A thread of its own watches the folders (lbterm_extras::inbox::Watch) and
//! looks in them when something there changes, opening only the files that are new or changed
//! (lbterm_extras::inbox::Scan); when what waits changes, every window repaints. While nothing
//! changes it sleeps: it looked every second until 2026-10-30, so a message showed up to a second
//! late. Nothing is read on the GUI thread. The thread starts the first time a window asks, and
//! follows the config as it's reloaded.

use lbterm_extras::inbox::{Message, Scan, Seen, Watch};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

/// A message written or moved is several changes in a row: they're taken together, this long
/// after the first
const SETTLE: Duration = Duration::from_millis(25);
/// A message file that couldn't be read (still being written, held open by a scanner) is looked
/// for again this soon, while there is one
const RETRY: Duration = Duration::from_millis(250);
/// How many times a look follows the folders before it reads them: a folder made can have
/// another under it already
const FOLLOWS: usize = 8;

/// What waits, oldest first, as last looked
static WAITING: Mutex<Option<Arc<Vec<Message>>>> = Mutex::new(None);
/// How many times what waits has changed, the first look included: 0 before it
static CHANGES: AtomicUsize = AtomicUsize::new(0);
static START: Once = Once::new();

/// The messages waiting now, oldest first: none before the first look
pub fn waiting() -> Arc<Vec<Message>> {
    looked().unwrap_or_default()
}

/// The messages waiting now, or nothing before the first look, when nobody knows yet
pub fn looked() -> Option<Arc<Vec<Message>>> {
    START.call_once(start);
    WAITING.lock().unwrap().clone()
}

/// How many times what waits has changed: a window that drew its links to messages at another
/// count draws them again
pub fn changes() -> usize {
    START.call_once(start);
    CHANGES.load(Ordering::Acquire)
}

/// What wakes the thread
enum Wake {
    Seen(Seen),
    /// The config was reloaded: its folders may be others
    Config,
}

fn start() {
    let spawned = std::thread::Builder::new().name("lbterm-inbox".into()).spawn(|| {
        let (tx, rx) = channel();
        let seen = tx.clone();
        let mut watch = match Watch::new(move |s| {
            let _ = seen.send(Wake::Seen(s));
        }) {
            Ok(watch) => watch,
            Err(err) => {
                log::error!("lbterm-inbox: can't watch the InMail folders, so the button won't change: {err:#}");
                return;
            }
        };
        let _reloads = config::subscribe_to_config_reload(move || tx.send(Wake::Config).is_ok());
        let mut scan = Scan::default();
        loop {
            let folders = config::configuration()
                .lbterm_inbox
                .as_ref()
                .map(|inbox| inbox.folders.clone())
                .unwrap_or_default();
            // Watched before they're read, so nothing written in between goes unseen
            for _ in 0..FOLLOWS {
                if !watch.follow(&folders) {
                    break;
                }
            }
            let found = scan.look(&folders);
            let changed = {
                let mut waiting = WAITING.lock().unwrap();
                let changed = waiting.as_deref() != Some(&found);
                if changed {
                    *waiting = Some(Arc::new(found));
                    CHANGES.fetch_add(1, Ordering::AcqRel);
                }
                changed
            };
            if changed {
                promise::spawn::spawn_into_main_thread(async { crate::state_files::repaint_windows() }).detach();
            }

            // Asleep until something changes that can change what waits
            let retry = (scan.unread() > 0).then(|| Instant::now() + RETRY);
            loop {
                let wake = if let Some(at) = retry {
                    match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(wake) => wake,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    match rx.recv() {
                        Ok(wake) => wake,
                        Err(_) => return,
                    }
                };
                match wake {
                    Wake::Config => break,
                    Wake::Seen(Seen::Changed(paths)) if watch.matters(&paths) => break,
                    Wake::Seen(Seen::Changed(_)) => {}
                    Wake::Seen(Seen::Failed(err)) => {
                        log::error!("lbterm-inbox: the folders' watch: {err}; looking in them again");
                        break;
                    }
                }
            }
            std::thread::sleep(SETTLE);
            while rx.try_recv().is_ok() {}
        }
    });
    if let Err(err) = spawned {
        log::error!("starting the lbterm-inbox thread: {err:#}");
    }
}
