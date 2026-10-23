//! LBTerm: the Conductor's hub (Lachlan's Life\Projects\Conductor\hub.ts) on its named pipe
//! (`config.lbterm_conductor.hub_pipe`), whose lines are lbterm_extras::conductor's.
//!
//! - A connection of LBTerm's own stays open, on a thread of its own. Its first line asks for
//!   the hub's focus lines (`lbterm.subscribe`), its second says where the Claude sessions are
//!   (`lbterm.panes`), and from then on the hub writes a `conductor.focus` line whenever the
//!   sessions the Conductor is thinking about or acting on change. The rail outlines their tabs
//!   (render/conductor.rs). When the connection closes or can't open, the outlines go and the
//!   thread tries again, waiting longer each time, up to [`RETRY_MAX`].
//! - Where the sessions are goes again each time it changes, on a connection of its own, as
//!   claudehook's relay sends hook events: skipped when no hub is listening, never waited on by
//!   the GUI thread.

use crate::state_files::repaint_windows;
use lbterm_extras::conductor::{self, Doing, Focus, Named, PaneFact};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

/// The first wait before connecting again, and the longest
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(30);
/// A busy pipe (every instance taken) is tried this many times, this far apart
const BUSY_TRIES: u32 = 5;
const BUSY_WAIT: Duration = Duration::from_millis(40);

/// What the hub last said, each outline lingering a moment after
static FOCUS: Mutex<Option<Focus>> = Mutex::new(None);
/// Where the sessions are, as last sent: sent again on each new connection
static PANES: Mutex<Option<String>> = Mutex::new(None);
static START: Once = Once::new();
static WRITER: Mutex<Option<Sender<(String, String)>>> = Mutex::new(None);

/// Which LBTerm this is, for the hub: a try window's tabs aren't the daily ones'
fn terminal() -> &'static str {
    if std::env::var_os("LBTERM_TRY").is_some() {
        "lbterm-try"
    } else {
        "lbterm"
    }
}

/// The configured pipe, if any
fn pipe() -> Option<String> {
    config::configuration()
        .lbterm_conductor
        .as_ref()
        .and_then(|c| c.hub_pipe.clone())
        .filter(|p| !p.is_empty())
}

/// What the Conductor is doing with session `sid`, as the hub last said; starts listening to
/// the hub the first time it's asked
pub fn doing(sid: &str) -> Option<Doing> {
    START.call_once(start);
    FOCUS.lock().unwrap().as_ref()?.get(sid, Instant::now())
}

/// When an outline next goes: a repaint then. Drops those already gone.
pub fn next_change() -> Option<Instant> {
    let mut focus = FOCUS.lock().unwrap();
    let focus = focus.as_mut()?;
    focus.prune(Instant::now());
    focus.next_change()
}

/// Where the Claude sessions are in this GUI: sent to the hub when it changed since last sent
pub fn panes(facts: &[PaneFact]) {
    let Some(pipe) = pipe() else {
        return;
    };
    let line = conductor::panes_message(terminal(), facts);
    {
        let mut last = PANES.lock().unwrap();
        if last.as_deref() == Some(line.as_str()) {
            return;
        }
        *last = Some(line.clone());
    }
    let mut writer = WRITER.lock().unwrap();
    let sender = writer.get_or_insert_with(|| {
        let (tx, rx) = channel::<(String, String)>();
        let spawned = std::thread::Builder::new()
            .name("lbterm-hub-writer".into())
            .spawn(move || {
                while let Ok(mut next) = rx.recv() {
                    // Only the newest matters
                    while let Ok(newer) = rx.try_recv() {
                        next = newer;
                    }
                    let (pipe, line) = next;
                    if let Err(err) = send_once(&pipe, &line) {
                        log::trace!("conductor hub: {err:#}");
                    }
                }
            });
        if let Err(err) = spawned {
            log::error!("starting the lbterm-hub-writer thread: {err:#}");
        }
        tx
    });
    if sender.send((pipe, line)).is_err() {
        *writer = None;
    }
}

/// Opens the pipe for reading and writing, trying again a few times while every instance is busy
fn open(pipe: &str) -> std::io::Result<std::fs::File> {
    let mut tries = 0;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(pipe) {
            // ERROR_PIPE_BUSY: the hub is between instances
            Err(err) if err.raw_os_error() == Some(231) && tries < BUSY_TRIES => {
                tries += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            result => return result,
        }
    }
}

/// One line on a connection of its own
fn send_once(pipe: &str, line: &str) -> anyhow::Result<()> {
    let mut file = open(pipe)?;
    file.write_all(format!("{line}\n").as_bytes())?;
    file.flush()?;
    Ok(())
}

fn set_focus(sessions: Option<Vec<Named>>) {
    let changed = {
        let mut focus = FOCUS.lock().unwrap();
        let focus = focus.get_or_insert_with(Focus::default);
        match sessions {
            Some(sessions) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                focus.set(&sessions, Instant::now(), now_ms)
            }
            None => focus.clear(),
        }
    };
    if changed {
        promise::spawn::spawn_into_main_thread(async { repaint_windows() }).detach();
    }
}

fn start() {
    let spawned = std::thread::Builder::new()
        .name("lbterm-hub".into())
        .spawn(|| {
            let mut wait = RETRY_FIRST;
            loop {
                if let Some(pipe) = pipe() {
                    let began = Instant::now();
                    if let Err(err) = listen(&pipe) {
                        log::trace!("conductor hub: {err:#}");
                    }
                    // Up a while, then gone (the hub restarting): the next try comes soon. A
                    // hub that hangs up at once is tried less and less often.
                    if began.elapsed() > RETRY_MAX {
                        wait = RETRY_FIRST;
                    }
                    set_focus(None);
                }
                std::thread::sleep(wait);
                wait = (wait * 2).min(RETRY_MAX);
            }
        });
    if let Err(err) = spawned {
        log::error!("starting the lbterm-hub thread: {err:#}");
    }
}

/// One connection, until the hub closes it
fn listen(pipe: &str) -> anyhow::Result<()> {
    let mut file = open(pipe)?;
    let hello = conductor::hello_message(std::process::id(), config::wezterm_version(), terminal());
    file.write_all(format!("{hello}\n").as_bytes())?;
    if let Some(panes) = PANES.lock().unwrap().clone() {
        file.write_all(format!("{panes}\n").as_bytes())?;
    }
    file.flush()?;
    log::info!("conductor hub: listening on {pipe}");
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            log::info!("conductor hub: {pipe} closed");
            return Ok(());
        }
        if let Some(sessions) = conductor::focus_message(&line) {
            set_focus(Some(sessions));
        }
    }
}
