//! LBTerm: letting go of a folder on request (lbterm/LET-GO.md). A thread of its own watches the
//! folder of requests `config.lbterm_let_go` names (lbterm_extras::let_go) and, when one comes,
//! goes or runs out, has every watch in this process let go of the folders they name, or take them
//! back (lbterm_extras::watch::let_go): each watch's own thread steps out of such a folder, so
//! another program can rename or remove it. It sleeps while nothing changes and no request is
//! waiting to run out, so at rest it costs nothing; it follows the config as it's reloaded.

use lbterm_extras::let_go::{asked, now_ms};
use lbterm_extras::watch::{let_go, Seen, Target, Watch};
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::{Duration, Instant};

/// A request written is several changes in a row: they're taken together, this long after the
/// first
const SETTLE: Duration = Duration::from_millis(25);

/// What wakes the thread
enum Wake {
    Seen(Seen),
    /// The config was reloaded: its folder may be another, or none
    Config,
}

fn folder() -> Option<PathBuf> {
    config::configuration()
        .lbterm_let_go
        .as_ref()
        .map(|l| PathBuf::from(&l.folder))
}

/// Starts the thread, once, when the GUI starts
pub fn start() {
    let spawned = std::thread::Builder::new().name("lbterm-let-go".into()).spawn(|| {
        let (tx, rx) = channel();
        let seen = tx.clone();
        let mut watch = match Watch::new(move |s| {
            let _ = seen.send(Wake::Seen(s));
        }) {
            Ok(watch) => watch,
            Err(err) => {
                log::error!("lbterm-let-go: can't watch for requests, so no folder is let go of: {err:#}");
                return;
            }
        };
        let _reloads = config::subscribe_to_config_reload(move || tx.send(Wake::Config).is_ok());
        let mut said: Vec<PathBuf> = vec![];
        loop {
            let dir = folder();
            let targets: Vec<Target> = dir.iter().map(|d| Target::Folder(d.clone())).collect();
            // Watched before it's read, so a request written in between isn't missed
            for _ in 0..8 {
                if !watch.follow(&targets) {
                    break;
                }
            }
            let now = dir.as_deref().map(|d| asked(d, now_ms())).unwrap_or_default();
            for (file, why) in &now.refused {
                if !said.contains(file) {
                    log::error!("lbterm-let-go: {} {why}: nothing let go of for it", file.display());
                }
            }
            said = now.refused.iter().map(|(f, _)| f.clone()).collect();
            if let_go(now.folders()) {
                let names: Vec<String> = now
                    .requests
                    .iter()
                    .map(|r| format!("{} (by {:?}, until {})", r.folder.display(), r.by, r.until))
                    .collect();
                log::info!("lbterm-let-go: letting go of [{}]", names.join(", "));
            }

            // Asleep until a request comes or goes, or the soonest runs out (a millisecond past its
            // until, which is when it no longer holds)
            let until = now
                .next
                .map(|n| Instant::now() + Duration::from_millis((n - now_ms()).max(0) as u64 + 1));
            loop {
                let wake = match until {
                    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(wake) => wake,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    },
                    None => match rx.recv() {
                        Ok(wake) => wake,
                        Err(_) => return,
                    },
                };
                match wake {
                    Wake::Config => break,
                    Wake::Seen(Seen::Changed(paths)) if watch.matters(&paths) => break,
                    Wake::Seen(Seen::Changed(_)) => {}
                    Wake::Seen(Seen::Failed(err)) => {
                        log::error!("lbterm-let-go: the requests' watch: {err}; reading them again");
                        break;
                    }
                }
            }
            std::thread::sleep(SETTLE);
            while rx.try_recv().is_ok() {}
        }
    });
    if let Err(err) = spawned {
        log::error!("starting the lbterm-let-go thread: {err:#}");
    }
}
