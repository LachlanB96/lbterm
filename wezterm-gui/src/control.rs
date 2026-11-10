//! LBTerm: the control pipe (lbterm/CONTROL.md), on `config::lbterm::control_pipe(<this GUI's
//! window class>)`, whose lines are lbterm_extras::control's. Earshot drives LBTerm by voice
//! through it, and `lbterm control` is the same from a shell.
//!
//! - One thread makes the pipe's instances and waits for each to be connected; each connection is
//!   served on a thread of its own, a request at a time. The pipe takes no clients from other
//!   machines.
//! - What a request needs from a window runs on the GUI thread (`TermWindowNotif::Apply`), in the
//!   window he was last in (seat_client::last_focused), else the first, and the connection's
//!   thread waits for it at most [`WAIT`]. Nothing here takes the keyboard or raises a window.

use crate::termwindow::TermWindowNotif;
use ::window::WindowOps;
use lbterm_extras::control::{self, DraftAct, Op};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

/// The longest a request waits for the GUI thread
const WAIT: Duration = Duration::from_secs(5);
/// After the pipe can't be made, the first wait before trying again, and the longest
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// Starts listening, once the GUI knows its window class
pub fn start() {
    let pipe = config::lbterm::control_pipe(&crate::termwindow::get_window_class());
    let spawned = std::thread::Builder::new()
        .name("lbterm-control".into())
        .spawn(move || listen(&pipe));
    if let Err(err) = spawned {
        log::error!("starting the lbterm-control thread: {err:#}");
    }
}

#[cfg(windows)]
fn listen(pipe: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use winapi::shared::winerror::ERROR_PIPE_CONNECTED;
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::namedpipeapi::{ConnectNamedPipe, CreateNamedPipeW};
    use winapi::um::winbase::{
        PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let wide: Vec<u16> = OsStr::new(pipe).encode_wide().chain(Some(0)).collect();
    let mut wait = RETRY_FIRST;
    let mut said = false;
    loop {
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            log::error!(
                "control: can't make {pipe}: {}",
                std::io::Error::last_os_error()
            );
            std::thread::sleep(wait);
            wait = (wait * 2).min(RETRY_MAX);
            continue;
        }
        wait = RETRY_FIRST;
        if !said {
            log::info!("control: listening on {pipe}");
            said = true;
        }
        // Owned from here, so it's closed however this goes
        let file = unsafe { std::fs::File::from_raw_handle(handle as _) };
        let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0
            || std::io::Error::last_os_error().raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32);
        if !connected {
            continue;
        }
        let spawned = std::thread::Builder::new()
            .name("lbterm-control-client".into())
            .spawn(move || serve(file));
        if let Err(err) = spawned {
            log::error!("control: starting a client's thread: {err:#}");
        }
    }
}

#[cfg(not(windows))]
fn listen(_pipe: &str) {}

/// One connection: a request a line, each answered before the next is read
fn serve(file: std::fs::File) {
    let mut writer = match file.try_clone() {
        Ok(writer) => writer,
        Err(err) => {
            log::error!("control: {err:#}");
            return;
        }
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        let limit = control::MAX_LINE as u64 + 1;
        match (&mut reader).take(limit).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let too_long = line.len() > control::MAX_LINE && !line.ends_with('\n');
        let reply = if too_long {
            control::refused(&Value::Null, "the line is too long")
        } else if line.trim().is_empty() {
            continue;
        } else {
            answer(&line)
        };
        if writer.write_all(format!("{reply}\n").as_bytes()).is_err() {
            return;
        }
        if too_long {
            // The reply is read before the pipe closes
            let _ = writer.sync_all();
            return;
        }
    }
}

/// The reply to one request line
fn answer(line: &str) -> String {
    let request = match control::parse(line) {
        Ok(request) => request,
        Err(reply) => return reply,
    };
    // Needs no window, and is answered here on the pipe's thread, so asking adds nothing to what
    // the GUI thread does; a test asks it often, so it isn't logged (counters.rs)
    if let Op::Metrics { reset, force, present } = request.op {
        return metrics(&request.id, reset, force, present);
    }
    log::info!("control: {}", request.op.name());
    let (tx, rx) = channel();
    let (id, op) = (request.id.clone(), request.op);
    promise::spawn::spawn_into_main_thread(async move {
        match draft_pane_wait(&op) {
            None => in_window(id, op, tx),
            // In a task of its own: the wait isn't Send
            Some(wait) => promise::spawn::spawn(async move {
                wait.await;
                in_window(id, op, tx)
            })
            .detach(),
        }
    })
    .detach();
    match rx.recv_timeout(WAIT) {
        Ok(reply) => reply,
        Err(_) => control::refused(
            &request.id,
            &format!("LBTerm didn't answer in {} s", WAIT.as_secs()),
        ),
    }
}

/// `metrics`: the counts are the process's, one set every reader shares, so the daily LBTerm's
/// are started again only when the request says it means to (`force`), and a test window's (a
/// class of its own) whenever it asks. `present` turns the timing to the screen on or off first
fn metrics(id: &Value, reset: bool, force: bool, present: Option<bool>) -> String {
    if reset && !force && crate::termwindow::get_window_class() == wezterm_gui_subcommands::DEFAULT_WINDOW_CLASS {
        return control::refused(
            id,
            "the daily LBTerm's counts are shared by everything that reads them: a reset would start another reader's span again without it knowing (add \"force\": true, or diff two replies)",
        );
    }
    if let Some(on) = present {
        lbterm_extras::present::set_on(on);
    }
    crate::counters::reply(id, reset)
}

/// On the GUI thread, before a voice draft is sent or typed into a pane named by id: what to
/// await first for the pane to be current, since its tab can be out of view and its mux server
/// holding its output, and the draft goes only where the screen shows its place
/// (`drafts::may_send`, `may_edit`): Lachlan's 6b of 2026-11-08, lbterm/MUX.md, "What reads a
/// held pane's lines". Up to config's `HELD_READ_WAIT`, the GUI thread running its other work
/// meanwhile; None for any other request, and for a pane that isn't held (the pane open in the
/// window, a draft's usual place, never is)
fn draft_pane_wait(op: &Op) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>> {
    let Op::Draft {
        pane: Some(id),
        act: DraftAct::Send | DraftAct::Edit,
    } = op
    else {
        return None;
    };
    mux::Mux::get()
        .iter_panes()
        .into_iter()
        .find(|p| crate::termwindow::render::tab_panel::server_pane_id(p) == *id)
        .and_then(|pane| pane.lbterm_current())
}

/// On the GUI thread: the request, in the window he was last in
fn in_window(id: Value, op: Op, tx: Sender<String>) {
    let Some(fe) = crate::frontend::try_front_end() else {
        let _ = tx.send(control::refused(&id, "LBTerm's windows aren't up yet"));
        return;
    };
    let windows = fe.gui_windows();
    let last = crate::seat_client::last_focused();
    let Some(win) = windows
        .iter()
        .find(|w| Some(w.mux_window_id) == last)
        .or_else(|| windows.first())
    else {
        let _ = tx.send(control::refused(&id, "no LBTerm window is open"));
        return;
    };
    win.window
        .notify(TermWindowNotif::Apply(Box::new(move |tw| {
            let reply = match op {
                Op::Focus => match tw.lbterm_control_focus() {
                    Some(focus) => control::focus_reply(&id, &focus),
                    None => control::refused(&id, "the window has no pane"),
                },
                Op::Emit { event } => match tw.lbterm_control_emit(&event) {
                    Some(pane) => {
                        control::emitted_reply(&id, &event, tw.mux_window_id as u64, pane)
                    }
                    None => control::refused(&id, "the window has no pane to run it with"),
                },
                Op::Draft { pane, act } => {
                    // A send or an edit that didn't happen says why where he's looking too
                    let types = matches!(act, DraftAct::Send | DraftAct::Edit);
                    match tw.lbterm_control_draft(pane, act) {
                        Ok((draft, did)) => control::draft_reply(&id, draft.as_ref(), did),
                        Err(why) => {
                            if types {
                                tw.lbterm_draft_refused(&why);
                            }
                            control::refused(&id, &why)
                        }
                    }
                }
                Op::Mic { state, ttl, heard } => {
                    // What Earshot heard, and what it did with it, on this window's strip
                    if let Some(heard) = heard {
                        tw.lbterm_strip_notice(format!("{} {heard}", lbterm_extras::mic::GLYPH));
                    }
                    control::mic_reply(&id, crate::mic::set(state, ttl))
                }
                // answer() takes it before it gets here
                Op::Metrics { reset, force, present } => metrics(&id, reset, force, present),
            };
            let _ = tx.send(reply);
        })));
}
