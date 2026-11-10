//! LBTerm: the Conductor's seat (Lachlan's ~\AI\Conductor, its SEAT.md) on its named pipe
//! (`config.lbterm_conductor.seat_pipe`), for the Conductor's view (termwindow/render/
//! conductor_view.rs), whose lines are lbterm_extras::seat::protocol's.
//!
//! - One connection, on a thread of its own (lbterm-seat), from the rail's first paint with a seat
//!   in the config: it subscribes (`seat.subscribe`) and from then on keeps the last `seat.view`,
//!   the conversation (items replaced by id, the reply being written) and what the seat answered
//!   each command. It's kept with the view closed too, for `seat.show`.
//! - What the seat says repaints the windows whose view is open and no others
//!   (lbterm_extras::seat::Views): nothing else shows it (the pad's asks are its state file's,
//!   render/conductor_view.rs). Until 2026-10-30 every line repainted every window: a frame for
//!   each new view (any session's status, the hub's guesses, an event) and ten a second while the
//!   Conductor wrote a reply, with the view closed everywhere.
//! - `seat.show` opens the view in the window that last had the keyboard, without taking it.
//! - Commands go on the same connection (SEAT.md lets a subscriber send them), written in the order
//!   they were sent, and each waits for its `seat.reply` at most [`reply_wait`]. The thread waits
//!   on the pipe, for a command, for the next reply's deadline and for a config reload at once
//!   (lbterm_extras::seat::link), so every wait has an end. Until 2026-10-30 each command had a
//!   connection and two threads of its own, and the one reading its reply stayed blocked for as
//!   long as a seat that never answered kept the connection.
//! - When the connection closes or can't open, the view says the seat isn't running, and the
//!   thread tries again after 2 s, doubling to 30 s while the seat hangs up at once, as the hub's
//!   client does (conductor_hub.rs), and at once for a command or a config reload. With no seat in
//!   the config it sleeps until a reload names one.

use crate::termwindow::TermWindowNotif;
use ::window::WindowOps;
use lbterm_extras::seat::link::{Event, Link, Waiting, Woke};
use lbterm_extras::seat::protocol::{self, Command, Conversation, Incoming, Reply, View};
use lbterm_extras::seat::Views;
use mux::window::WindowId as MuxWindowId;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};

/// The first wait before connecting again, and the longest
const RETRY_FIRST: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(30);
/// The longest the pipe may take to take a line: a seat that takes nothing that long isn't
/// reading, and the connection is given up
const WRITE_WAIT: Duration = Duration::from_secs(5);
/// The longest a command waits for its reply: a control answers once the Conductor's Claude has
/// started again (an `on`, a `restart`, a mode change), an answer once it's typed and the
/// question has gone (about 4 s at most), the rest at once (Conductor's SEAT.md)
fn reply_wait(command: &Command) -> Duration {
    match command {
        Command::Control { .. } => Duration::from_secs(120),
        Command::Answer { .. } => Duration::from_secs(30),
        _ => Duration::from_secs(15),
    }
}
/// The replies kept, newest last
const REPLIES: usize = 32;

/// Everything the seat has said, for every window's view
#[derive(Default)]
pub struct Seat {
    pub connected: bool,
    pub view: Option<View>,
    pub conversation: Conversation,
    /// The answers to the commands sent, newest last
    pub replies: VecDeque<Reply>,
    /// Goes up with every change
    pub generation: u64,
}

static SEAT: Mutex<Option<Seat>> = Mutex::new(None);
static START: Once = Once::new();
/// The thread started: until then, and if it couldn't, a command is refused at once
static RUNNING: AtomicBool = AtomicBool::new(false);
/// The windows whose view is open: the only ones what the seat says repaints
static VIEWS: Mutex<Views<MuxWindowId>> = Mutex::new(Views::new());
/// Each command's id, its line and how long it waits for its reply, until the thread writes it
static QUEUE: Mutex<VecDeque<(String, String, Duration)>> = Mutex::new(VecDeque::new());
/// Wakes the thread: a command queued, or the config reloaded
static WAKE: OnceLock<Option<Event>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// The window he was last in (it took the keyboard, or was clicked in): `seat.show` opens the
/// view there
static LAST_FOCUSED: Mutex<Option<MuxWindowId>> = Mutex::new(None);

/// One of the locks above, taken even if a thread panicked holding it: what's in it is what the
/// seat said, still good to show, and the seat thread's end ([`Stopped`]) answers the commands
/// queued through them while it unwinds
fn locked<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The configured pipe, if any
pub fn pipe() -> Option<String> {
    config::configuration()
        .lbterm_conductor
        .as_ref()
        .and_then(|c| c.seat_pipe.clone())
        .filter(|p| !p.is_empty())
}

fn wake() -> Option<&'static Event> {
    WAKE.get_or_init(|| match Event::new() {
        Ok(event) => Some(event),
        Err(err) => {
            log::error!("seat: no event to wake its thread with, so LBTerm can't talk to the seat: {err:#}");
            None
        }
    })
    .as_ref()
}

/// Starts listening to the seat, once: from the rail's first paint with a seat in the config, and
/// as the view opens
pub fn listen() {
    START.call_once(start);
}

/// What the seat has said, under its lock
pub fn seat() -> std::sync::MutexGuard<'static, Option<Seat>> {
    locked(&SEAT)
}

/// How many times what the seat has said has changed: the view is laid out again when it has,
/// and not otherwise (termwindow/render/conductor_view.rs)
pub fn generation() -> u64 {
    seat().as_ref().map_or(0, |s| s.generation)
}

/// The view opened in `window`: what the seat says repaints it from now on
pub fn view_opened(window: MuxWindowId) {
    locked(&VIEWS).open(window);
}

/// The view closed in `window`: what the seat says no longer repaints it
pub fn view_closed(window: MuxWindowId) {
    locked(&VIEWS).close(window);
}

/// He's in this window: it took the keyboard, or was clicked in
pub fn focused(window: MuxWindowId) {
    *locked(&LAST_FOCUSED) = Some(window);
}

/// The window he was last in, if he's been in one: the control pipe's requests go there too
/// (control.rs)
pub fn last_focused() -> Option<MuxWindowId> {
    *locked(&LAST_FOCUSED)
}

/// Which LBTerm this is, for the seat: a try window isn't the daily one
fn client() -> &'static str {
    if std::env::var_os("LBTERM_TRY").is_some() {
        "lbterm-try"
    } else {
        "lbterm"
    }
}

/// What the seat has said changes: the windows with the view open repaint, and no others
fn changed(update: impl FnOnce(&mut Seat)) {
    {
        let mut seat = locked(&SEAT);
        let seat = seat.get_or_insert_with(Seat::default);
        update(seat);
        seat.generation += 1;
    }
    if !locked(&VIEWS).any() {
        return;
    }
    promise::spawn::spawn_into_main_thread(async { repaint_views() }).detach();
}

/// Repaints the windows whose view is open, for what the seat said
fn repaint_views() {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    let windows = fe.gui_windows();
    let alive: Vec<MuxWindowId> = windows.iter().map(|w| w.mux_window_id).collect();
    let open = locked(&VIEWS).to_repaint(&alive);
    for win in windows.iter().filter(|w| open.contains(&w.mux_window_id)) {
        win.window.invalidate();
    }
}

/// Sends `command` to the seat: its id, which its reply carries
pub fn send(command: Command) -> String {
    let id = format!("lbterm-{}-{}", std::process::id(), NEXT_ID.fetch_add(1, Ordering::Relaxed));
    if pipe().is_none() {
        record(Reply {
            id: id.clone(),
            ok: false,
            text: None,
            error: Some("No seat in the config (lbterm_conductor.seat_pipe)".to_string()),
        });
        return id;
    }
    let line = command.line(&id);
    let wait = reply_wait(&command);
    // Its kind and id only: his words stay out of the log
    log::info!("seat: {} {id}", command.kind());
    listen();
    locked(&QUEUE).push_back((id.clone(), line, wait));
    match wake().filter(|_| RUNNING.load(Ordering::Acquire)) {
        Some(event) => event.set(),
        None => refuse_queued("LBTerm's seat thread isn't running (its log says why)"),
    }
    id
}

fn record(reply: Reply) {
    if !reply.ok {
        log::warn!("seat: {} refused: {}", reply.id, reply.words().unwrap_or(""));
    }
    changed(|seat| {
        seat.replies.push_back(reply);
        while seat.replies.len() > REPLIES {
            seat.replies.pop_front();
        }
    });
}

fn refusal(id: String, why: impl std::fmt::Display) -> Reply {
    Reply {
        id,
        ok: false,
        text: None,
        error: Some(format!("The seat didn't answer: {why}")),
    }
}

/// Every command still queued gets `why` as its answer: there's no seat to send it to
fn refuse_queued(why: &str) {
    let queued: Vec<_> = locked(&QUEUE).drain(..).collect();
    for (id, _, _) in queued {
        record(refusal(id, why));
    }
}

/// The reply to the command `id`, once it's come
pub fn reply(id: &str) -> Option<Reply> {
    locked(&SEAT).as_ref()?.replies.iter().rev().find(|r| r.id == id).cloned()
}

/// The seat's thread ending, however it ends (a panic included): commands are refused from then
/// on, those queued are answered so, and the log says it. It never ends otherwise, so until
/// 2026-10-31 a thread gone left every command queued unanswered, its button saying "…" for good
struct Stopped;

impl Drop for Stopped {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
        log::error!(
            "seat: the lbterm-seat thread stopped{}, so LBTerm no longer hears the seat or sends it anything",
            if std::thread::panicking() { " (it panicked)" } else { "" }
        );
        refuse_queued("LBTerm's seat thread stopped (its log says why)");
    }
}

fn start() {
    // Running before the thread is, so a command sent at once is queued for it, not refused; it
    // says when it stops
    RUNNING.store(true, Ordering::Release);
    let spawned = std::thread::Builder::new()
        .name("lbterm-seat".into())
        .spawn(|| {
            let _stopped = Stopped;
            // Without it, send() refuses every command, saying so
            let Some(event) = wake() else {
                return;
            };
            let _reloads = config::subscribe_to_config_reload(move || {
                event.set();
                true
            });
            let mut wait = RETRY_FIRST;
            loop {
                let Some(pipe) = pipe() else {
                    refuse_queued("there's no seat in the config (lbterm_conductor.seat_pipe)");
                    // Asleep until a reload may have named one
                    event.wait(None);
                    continue;
                };
                let began = Instant::now();
                match Link::open(&pipe) {
                    Ok(link) => {
                        if let Err(err) = subscription(link, &pipe, event) {
                            log::warn!("seat: {pipe}: {err:#}");
                        }
                        // Up a while, then gone (the seat restarting): the next try comes soon.
                        // A seat that hangs up at once is tried less and less often.
                        if began.elapsed() > RETRY_MAX {
                            wait = RETRY_FIRST;
                        }
                        let was = locked(&SEAT).as_ref().map_or(false, |s| s.connected);
                        if was {
                            changed(|seat| {
                                seat.connected = false;
                                seat.conversation.stop_streaming();
                            });
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        log::trace!("seat: {pipe} isn't there");
                        refuse_queued("it isn't running");
                    }
                    Err(err) => {
                        log::warn!("seat: opening {pipe}: {err:#}");
                        refuse_queued(&format!("{err:#}"));
                    }
                }
                // Tried again then, or at once for a command or a reload
                if !event.wait(Some(Instant::now() + wait)) {
                    wait = (wait * 2).min(RETRY_MAX);
                }
            }
        });
    if let Err(err) = spawned {
        RUNNING.store(false, Ordering::Release);
        log::error!("starting the lbterm-seat thread: {err:#}");
    }
}

/// One subscription, until the seat closes it, the config names another pipe, or the pipe fails.
/// The commands queued are written on it as they come, and each waits for its reply until its
/// deadline; those still waiting when it ends are told the seat hung up.
fn subscription(mut link: Link, pipe: &str, event: &Event) -> anyhow::Result<()> {
    let hello = Command::Subscribe {
        client: client().to_string(),
        pid: std::process::id(),
        version: config::wezterm_version().to_string(),
    }
    .line(&format!("lbterm-{}-0", std::process::id()));
    link.write_line(&hello, WRITE_WAIT)?;
    log::info!("seat: subscribed on {pipe}");
    changed(|seat| seat.connected = true);
    let mut waiting = Waiting::default();
    let ended = talk(&mut link, pipe, event, &mut waiting);
    for id in waiting.take_all() {
        record(refusal(id, "it hung up"));
    }
    ended
}

fn talk(link: &mut Link, pipe: &str, event: &Event, waiting: &mut Waiting) -> anyhow::Result<()> {
    loop {
        // Whatever's queued, in the order it was sent
        loop {
            let next = locked(&QUEUE).pop_front();
            let Some((id, line, wait)) = next else {
                break;
            };
            if let Err(err) = link.write_line(&line, WRITE_WAIT) {
                record(refusal(id, "it isn't reading"));
                return Err(err.into());
            }
            waiting.add(id, wait, Instant::now());
        }
        for (id, wait) in waiting.due(Instant::now()) {
            record(refusal(id, format!("no reply in {} s", wait.as_secs())));
        }
        match link.wait(event, waiting.next())? {
            Woke::Lines(lines) => {
                for line in lines {
                    heard(&line, waiting);
                }
            }
            // A command queued, or the config reloaded: another pipe (or none) is connected again
            Woke::Event => {
                if self::pipe().as_deref() != Some(pipe) {
                    log::info!("seat: the config names another seat than {pipe}");
                    return Ok(());
                }
            }
            Woke::Due => {}
            Woke::Closed => {
                log::info!("seat: {pipe} closed");
                return Ok(());
            }
        }
    }
}

/// One line from the seat
fn heard(line: &str, waiting: &mut Waiting) {
    match protocol::parse(line) {
        Some(Incoming::View(view)) => changed(|seat| seat.view = Some(*view)),
        Some(Incoming::Items { reset, items }) => changed(|seat| seat.conversation.apply(reset, items)),
        Some(Incoming::Delta { id, text }) => changed(|seat| seat.conversation.delta(id, text)),
        // A reply after its wait is over is nobody's: it was told the seat didn't answer
        Some(Incoming::Reply(reply)) => {
            if waiting.answered(&reply.id) {
                record(reply);
            }
        }
        Some(Incoming::Show) => promise::spawn::spawn_into_main_thread(async { show() }).detach(),
        None => {}
    }
}

/// `seat.show`: the view opens in the window he was last in, else the first, and nothing takes
/// the keyboard
fn show() {
    let Some(fe) = crate::frontend::try_front_end() else {
        return;
    };
    let windows = fe.gui_windows();
    let last = *locked(&LAST_FOCUSED);
    let win = windows
        .iter()
        .find(|w| Some(w.mux_window_id) == last)
        .or_else(|| windows.first());
    if let Some(win) = win {
        log::info!("seat: show, in window {}", win.mux_window_id);
        win.window
            .notify(TermWindowNotif::Apply(Box::new(|tw| tw.lbterm_view_open())));
    }
}
