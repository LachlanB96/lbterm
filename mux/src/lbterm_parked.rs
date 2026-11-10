//! LBTerm: parked tabs (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing 4"): which tabs are parked,
//! and the file the process holding the tabs publishes beside its socket for Terminal's
//! statusline, `<socket>.parked.json`: `{"pid":<its pid>,"panes":[<its id of every pane in a
//! parked tab>]}`.
//!
//! A tab's flag is `Tab::lbterm_parked`. The publisher ([`publish`]) is started once by the
//! process that holds tabs: the mux server as it starts serving (an empty file at once, so a
//! crashed server's never names the next one's panes), and the GUI for its own tabs, a window's
//! client tabs left out (it writes only once one of its own has been parked). Its mux
//! subscriber does nothing but note that the list may have changed: a tab parked or unparked,
//! or, while the file names any pane, a pane added or gone, a tab moved or resized (a split), a
//! window gone. One task on the main thread then works the list out (an atomic load a tab, the
//! panes of the parked ones) and, when it differs from the last written, hands it to the
//! publisher's thread, which writes it whole through a rename. Nothing runs at rest, and with no
//! tab parked a notification costs a load of one atomic.

use crate::pane::{Pane, PaneId};
use crate::tab::TabId;
use crate::{Mux, MuxNotification};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The file published beside `socket`
pub fn path_for(socket: &Path) -> PathBuf {
    let mut name = socket.as_os_str().to_os_string();
    name.push(".parked.json");
    PathBuf::from(name)
}

/// The file's words: `{"pid":12,"panes":[3,4]}`
pub fn contents(pid: u32, panes: &[PaneId]) -> String {
    let panes: Vec<String> = panes.iter().map(|p| p.to_string()).collect();
    format!("{{\"pid\":{pid},\"panes\":[{}]}}", panes.join(","))
}

/// Every parked tab of the mux's windows
pub fn parked_tabs(mux: &Mux) -> Vec<TabId> {
    let mut tabs = vec![];
    for window_id in mux.iter_windows() {
        if let Some(window) = mux.get_window(window_id) {
            tabs.extend(
                window
                    .iter_tabs()
                    .filter(|t| t.lbterm_parked())
                    .map(|t| t.tab_id()),
            );
        }
    }
    tabs.sort_unstable();
    tabs
}

/// Every pane of every parked tab that `holds`, sorted: what the file names
pub fn parked_panes(mux: &Mux, holds: &dyn Fn(&Arc<dyn Pane>) -> bool) -> Vec<PaneId> {
    let mut panes = vec![];
    for window_id in mux.iter_windows() {
        let tabs: Vec<_> = match mux.get_window(window_id) {
            Some(window) => window
                .iter_tabs()
                .filter(|t| t.lbterm_parked())
                .cloned()
                .collect(),
            None => continue,
        };
        for tab in tabs {
            panes.extend(
                tab.iter_panes_ignoring_zoom()
                    .into_iter()
                    .filter(|p| holds(&p.pane))
                    .map(|p| p.pane.pane_id()),
            );
        }
    }
    panes.sort_unstable();
    panes.dedup();
    panes
}

/// Whether a notification can change the list: a park or unpark always, and the rest only while
/// the file names a pane (`naming`)
pub fn may_change(notification: &MuxNotification, naming: bool) -> bool {
    match notification {
        MuxNotification::LbtermTabParked { .. } => true,
        MuxNotification::PaneAdded(_)
        | MuxNotification::PaneRemoved(_)
        | MuxNotification::TabAddedToWindow { .. }
        | MuxNotification::TabResized(_)
        | MuxNotification::WindowRemoved(_) => naming,
        _ => false,
    }
}

struct Publisher {
    holds: fn(&Arc<dyn Pane>) -> bool,
    /// What the file says, as last handed to the writer; `None` once a write failed
    last: Option<Vec<PaneId>>,
    to_write: std::sync::mpsc::Sender<String>,
}

static PUBLISHER: Mutex<Option<Publisher>> = Mutex::new(None);
/// The file names a pane
static NAMING: AtomicBool = AtomicBool::new(false);
/// A look at the list is on its way to the main thread
static DUE: AtomicBool = AtomicBool::new(false);

/// Publish this process's parked panes beside `socket`, those `holds` says are its own, from
/// now on. `now`: write the empty file at once (a server starting to serve)
pub fn publish(socket: PathBuf, holds: fn(&Arc<dyn Pane>) -> bool, now: bool) {
    let path = path_for(&socket);
    let (to_write, written) = std::sync::mpsc::channel::<String>();
    let spawned = std::thread::Builder::new()
        .name("lbterm-parked".into())
        .spawn(move || {
            while let Ok(mut text) = written.recv() {
                // Only the newest: the ones before it say less
                while let Ok(newer) = written.try_recv() {
                    text = newer;
                }
                if let Err(err) = write(&path, &text) {
                    log::error!("parked: writing {}: {err:#}", path.display());
                    // Not written: the next look writes its list whatever it is, rather than
                    // taking the one it failed with for what the file says
                    if let Some(publisher) = PUBLISHER.lock().as_mut() {
                        publisher.last = None;
                    }
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("parked: no thread to publish with: {err:#}");
        return;
    }
    if now {
        to_write
            .send(contents(std::process::id(), &[]))
            .ok();
    }
    *PUBLISHER.lock() = Some(Publisher {
        holds,
        last: Some(vec![]),
        to_write,
    });
    if let Some(mux) = Mux::try_get() {
        mux.subscribe(|n| {
            if may_change(&n, NAMING.load(Ordering::Relaxed)) && !DUE.swap(true, Ordering::AcqRel) {
                promise::spawn::spawn_into_main_thread(async { look() }).detach();
            }
            true
        });
    }
}

/// The list worked out again, on the main thread, and handed to the publisher's thread when it
/// changed
fn look() {
    DUE.store(false, Ordering::Release);
    let Some(mux) = Mux::try_get() else {
        return;
    };
    let mut publisher = PUBLISHER.lock();
    let Some(publisher) = publisher.as_mut() else {
        return;
    };
    let panes = parked_panes(&mux, &publisher.holds);
    if publisher.last.as_ref() == Some(&panes) {
        return;
    }
    NAMING.store(!panes.is_empty(), Ordering::Relaxed);
    log::info!("parked: panes {panes:?}");
    publisher
        .to_write
        .send(contents(std::process::id(), &panes))
        .ok();
    publisher.last = Some(panes);
}

/// How long a rename over the file keeps trying while a reader holds it open without
/// FILE_SHARE_DELETE (.NET's File.ReadAllText, PowerShell's Get-Content, an antivirus scan of a
/// new file): lbterm-extras'
/// `state::write_atomic`'s, copied, as mux can't depend on lbterm-extras (lbterm/PITFALLS.md,
/// 256)
const RENAME_FOR: std::time::Duration = std::time::Duration::from_millis(250);
const RENAME_PAUSE: std::time::Duration = std::time::Duration::from_millis(5);

/// `text` into `path` whole: a reader sees the old file or the new, never half of one
fn write(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    let started = std::time::Instant::now();
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(err) if started.elapsed() >= RENAME_FOR => return Err(err),
            Err(_) => std::thread::sleep(RENAME_PAUSE),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_file_is_named_beside_the_socket_and_says_the_pid_and_panes() {
        let sock = Path::new("C:/Users/someone/.local/share/lbterm/sock");
        assert_eq!(
            path_for(sock),
            PathBuf::from("C:/Users/someone/.local/share/lbterm/sock.parked.json")
        );
        assert_eq!(
            path_for(Path::new("/tmp/gui-sock-42")),
            PathBuf::from("/tmp/gui-sock-42.parked.json")
        );
        assert_eq!(contents(7, &[]), r#"{"pid":7,"panes":[]}"#);
        assert_eq!(contents(7, &[3, 12]), r#"{"pid":7,"panes":[3,12]}"#);
    }

    #[test]
    fn a_park_always_looks_and_the_rest_only_while_the_file_names_a_pane() {
        let parked = MuxNotification::LbtermTabParked {
            tab_id: 1,
            parked: true,
        };
        assert!(may_change(&parked, false));
        assert!(may_change(&parked, true));
        for n in [
            MuxNotification::PaneAdded(3),
            MuxNotification::PaneRemoved(3),
            MuxNotification::TabAddedToWindow {
                tab_id: 1,
                window_id: 0,
            },
            MuxNotification::TabResized(1),
            MuxNotification::WindowRemoved(0),
        ] {
            assert!(!may_change(&n, false), "{:?} with nothing parked", n);
            assert!(may_change(&n, true), "{:?} while the file names a pane", n);
        }
        assert!(!may_change(&MuxNotification::PaneOutput(3), true));
        assert!(!may_change(&MuxNotification::PaneFocused(3), true));
    }

    #[test]
    fn a_write_replaces_the_file_whole() {
        let dir = std::env::temp_dir().join(format!("lbterm-parked-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = path_for(&dir.join("sock"));
        write(&path, &contents(1, &[])).unwrap();
        write(&path, &contents(1, &[5])).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"pid":1,"panes":[5]}"#);
        assert!(!dir.join("sock.parked.json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_write_waits_out_a_reader_holding_the_file_without_share_delete() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("lbterm-parked-held-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = path_for(&dir.join("sock"));
        write(&path, &contents(1, &[])).unwrap();
        // FILE_SHARE_READ alone, as .NET's File.ReadAllText and PowerShell's Get-Content open it:
        // a bare rename over it fails
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let reader = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            drop(held);
        });
        write(&path, &contents(1, &[5])).unwrap();
        reader.join().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"pid":1,"panes":[5]}"#);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
