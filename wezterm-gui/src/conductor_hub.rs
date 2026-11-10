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
//! - On the same connection the hub says how far along each Claude session and its subagents
//!   are, its guesses (`conductor.estimates`, Conductor's estimate.ts), whenever they change and
//!   every 45 s besides. The rail draws them as bars (render/tab_panel.rs) and the taskbar
//!   button shows the open tab's when nothing reports progress of its own (taskbar.rs). They go
//!   with the connection, or at the line's `until` if the hub stops talking without closing it.

use crate::state_files::repaint_windows;
use crate::termwindow::TermWindowNotif;
use ::window::WindowOps;
use lbterm_extras::conductor::{self, Doing, Estimate, Estimates, Focus, Named, PaneFact, Panes};
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
/// The hub's last bars, and when they run out
static ESTIMATES: Mutex<Option<(Estimates, Option<Instant>)>> = Mutex::new(None);
/// Where the sessions are in every window, as each last painted them, and the line last sent:
/// sent again on each new connection
static PANES: Mutex<Panes> = Mutex::new(Panes::new());
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

/// Whether the Conductor is acting on any session, as the hub last said, wherever that session
/// is: its pad's spectrum in Accept most and Accept all (Lachlan's 7a; render/tab_panel.rs asks
/// once a paint of the rail). A lock and a look through the few sessions named; starts listening
/// to the hub the first time it's asked
pub fn acting() -> bool {
    START.call_once(start);
    FOCUS
        .lock()
        .unwrap()
        .as_ref()
        .map_or(false, |focus| focus.acting(Instant::now()))
}

/// How far along session `sid` is, as the hub last guessed; starts listening to the hub the
/// first time it's asked, if the config names one
pub fn estimate(sid: &str) -> Option<Estimate> {
    pipe()?;
    START.call_once(start);
    let estimates = ESTIMATES.lock().unwrap();
    let (estimates, until) = estimates.as_ref()?;
    if until.map_or(false, |u| Instant::now() >= u) {
        return None;
    }
    estimates.sessions.get(sid).cloned()
}

/// When an outline next goes, or the bars run out: a repaint then. Drops outlines already gone.
pub fn next_change() -> Option<Instant> {
    let bars = ESTIMATES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|(e, until)| (!e.sessions.is_empty()).then_some(*until).flatten())
        .filter(|u| *u > Instant::now());
    let mut focus = FOCUS.lock().unwrap();
    let outlines = focus.as_mut().and_then(|focus| {
        focus.prune(Instant::now());
        focus.next_change()
    });
    [bars, outlines].iter().flatten().min().copied()
}

/// Whether what the rail shows of the hub changes by `at`: its bars run out, or an outline goes.
/// [`next_change`]'s times, which it drops once they're past: a rail woken at one asks this
/// whether it came or was moved on (the hub resends its guesses with a later end)
pub fn changes_by(at: Instant) -> bool {
    let bars = ESTIMATES
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|(e, until)| !e.sessions.is_empty() && until.is_some_and(|u| u <= at));
    let outlines = FOCUS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(Focus::next_change)
        .is_some_and(|end| end <= at);
    bars || outlines
}

/// Where the Claude sessions are in this GUI, in mux window `window` (as that window just painted
/// them) and in every other window still open: sent to the hub when it changed since last sent.
/// Every paint of the rail gives it, and one whose window's sessions are where they were builds
/// nothing (lbterm_extras::conductor::Panes)
pub fn panes(window: usize, facts: Vec<PaneFact>) {
    let line = {
        let mux = mux::Mux::get();
        PANES
            .lock()
            .unwrap()
            .update(terminal(), window, facts, |id| mux.get_window(id).is_some())
    };
    let (Some(line), Some(pipe)) = (line, pipe()) else {
        return;
    };
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

/// The hub's list, or none (it went). A change in what shows repaints every window. A change only
/// in when an outline goes (the hub letting go of a session, which still shows its least lap and
/// its linger, or naming one again with a new `until`) paints nothing now, but sets every rail's
/// wake again for then (`TermWindow::tab_panel_hub_wake`): until 2026-11-09 nothing did, so an
/// outline let go of, and from landing 4 the Conductor's pad's spectrum with its hues turning in
/// DWM, stayed until some other paint came (PITFALLS 249)
fn set_focus(sessions: Option<Vec<Named>>) {
    let (changed, ends_moved) = {
        let mut focus = FOCUS.lock().unwrap();
        let focus = focus.get_or_insert_with(Focus::default);
        let before = focus.next_change();
        let changed = match sessions {
            Some(sessions) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                focus.set(&sessions, Instant::now(), now_ms)
            }
            None => focus.clear(),
        };
        (changed, focus.next_change() != before)
    };
    if changed {
        promise::spawn::spawn_into_main_thread(async { repaint_windows() }).detach();
    } else if ends_moved {
        promise::spawn::spawn_into_main_thread(async {
            let Some(fe) = crate::frontend::try_front_end() else {
                return;
            };
            for win in fe.gui_windows() {
                win.window.notify(TermWindowNotif::Apply(Box::new(|tw| tw.tab_panel_hub_wake())));
            }
        })
        .detach();
    }
}

/// The hub's bars, or none (it went): every window repaints and its taskbar button looks again
fn set_estimates(estimates: Option<Estimates>) {
    let changed = {
        let mut held = ESTIMATES.lock().unwrap();
        let was = held.as_ref().map(|(e, _)| e.sessions.clone());
        *held = estimates.map(|e| {
            let until = e.until_ms.map(|until| {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                Instant::now() + Duration::from_millis(until.saturating_sub(now_ms))
            });
            (e, until)
        });
        held.as_ref().map(|(e, _)| e.sessions.clone()) != was
    };
    if changed {
        promise::spawn::spawn_into_main_thread(async {
            let Some(fe) = crate::frontend::try_front_end() else {
                return;
            };
            for win in fe.gui_windows() {
                win.window.notify(TermWindowNotif::Apply(Box::new(|tw| tw.update_taskbar_progress())));
                win.window.invalidate();
            }
        })
        .detach();
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
                    set_estimates(None);
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
    let panes = PANES.lock().unwrap().line().map(str::to_string);
    if let Some(panes) = panes {
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
        } else if let Some(estimates) = conductor::estimates_message(&line) {
            set_estimates(Some(estimates));
        }
    }
}
