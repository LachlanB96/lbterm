use crate::lbterm_flow::{Outbox, Pending, Wait};
use crate::sessionhandler::{PduSender, SessionHandler};
use anyhow::Context;
use async_ossl::AsyncSslStream;
use codec::{DecodedPdu, Pdu};
use mux::{Mux, MuxNotification};
use smol::Async;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use wezterm_uds::UnixStream;

/// LBTerm: how many connections this process serves are open, each with its subscription to the
/// mux. In a GUI these are its own socket's (a cli run against it), and its title decider reads
/// the count (wezterm-gui's termwindow/titles.rs): while one is open no title alert is dropped
/// and no window title kept on the client thread, so the connection hears each as it did
static LBTERM_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

/// LBTerm: the connections open (LBTERM_CONNECTIONS)
pub fn lbterm_connections() -> usize {
    LBTERM_CONNECTIONS.load(Ordering::Acquire)
}

/// LBTerm: a connection counted in LBTERM_CONNECTIONS while it's served
struct Counted;

impl Counted {
    fn new() -> Self {
        LBTERM_CONNECTIONS.fetch_add(1, Ordering::AcqRel);
        Counted
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        LBTERM_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(unix)]
pub trait AsRawDesc: std::os::unix::io::AsRawFd + std::os::fd::AsFd {}
#[cfg(windows)]
pub trait AsRawDesc: std::os::windows::io::AsRawSocket + std::os::windows::io::AsSocket {}

impl AsRawDesc for UnixStream {}
impl AsRawDesc for AsyncSslStream {}

#[derive(Debug)]
enum Item {
    Notif(MuxNotification),
    WritePdu(DecodedPdu),
    /// LBTerm: the mux subscriber noted a pane's output or alert in the connection's Pending, or
    /// the connection goes round again for what it owes there (lbterm_flow::Wait::Now)
    Owed,
    Readable,
    /// LBTerm: the socket has room for what the outbox holds
    Writable,
    HangupTick,
}

/// LBTerm: a test server hangs up on a client when the file
/// `LBTERM_TEST_HANGUP` names appears, taking the file, as the daily server
/// did on 2026-10-23 each time it failed to encode a PDU
/// (lbterm/perf/mux-reconnect.ps1). Nothing sets it for Lachlan's server.
fn take_hangup(path: &Option<std::path::PathBuf>) -> bool {
    path.as_ref()
        .map_or(false, |path| std::fs::remove_file(path).is_ok())
}

pub async fn process<T>(stream: T) -> anyhow::Result<()>
where
    T: 'static,
    T: std::io::Read,
    T: std::io::Write,
    T: AsRawDesc,
    T: std::fmt::Debug,
    T: async_io::IoSafe,
{
    let stream = smol::Async::new(stream)?;
    process_async(stream).await
}

/// LBTerm: what a notification the connection isn't owed a push for sends the client, as
/// upstream's loop wrote it straight to the socket; a pane's output and alerts go in `pending`
pub(crate) fn notification_pdu(n: MuxNotification, pending: &Mutex<Pending>) -> Option<Pdu> {
    match n {
        MuxNotification::PaneOutput(pane_id) => {
            pending.lock().unwrap().output(pane_id);
            None
        }
        MuxNotification::Alert { pane_id, alert } => {
            pending.lock().unwrap().alert(pane_id, alert);
            None
        }
        MuxNotification::PaneAdded(_pane_id) => None,
        MuxNotification::PaneRemoved(pane_id) => {
            // LBTerm: nothing more is owed or held for it
            pending.lock().unwrap().removed(pane_id);
            Some(Pdu::PaneRemoved(codec::PaneRemoved { pane_id }))
        }
        MuxNotification::SaveToDownloads { .. } => None,
        MuxNotification::AssignClipboard {
            pane_id,
            selection,
            clipboard,
        } => Some(Pdu::SetClipboard(codec::SetClipboard {
            pane_id,
            clipboard,
            selection,
        })),
        MuxNotification::TabAddedToWindow { tab_id, window_id } => {
            Some(Pdu::TabAddedToWindow(codec::TabAddedToWindow { tab_id, window_id }))
        }
        MuxNotification::WindowRemoved(_window_id) => None,
        MuxNotification::WindowCreated(_window_id) => None,
        MuxNotification::WindowInvalidated(_window_id) => None,
        MuxNotification::WindowWorkspaceChanged(window_id) => {
            let workspace = {
                let mux = Mux::get();
                mux.get_window(window_id)
                    .map(|w| w.get_workspace().to_string())
            };
            workspace.map(|workspace| {
                Pdu::WindowWorkspaceChanged(codec::WindowWorkspaceChanged {
                    window_id,
                    workspace,
                })
            })
        }
        MuxNotification::PaneFocused(pane_id) => {
            Some(Pdu::PaneFocused(codec::PaneFocused { pane_id }))
        }
        MuxNotification::TabResized(tab_id) => Some(Pdu::TabResized(codec::TabResized { tab_id })),
        MuxNotification::TabTitleChanged { tab_id, title } => {
            Some(Pdu::TabTitleChanged(codec::TabTitleChanged { tab_id, title }))
        }
        MuxNotification::WindowTitleChanged { window_id, title } => {
            pending.lock().unwrap().window_title(window_id, title);
            None
        }
        MuxNotification::WorkspaceRenamed {
            old_workspace,
            new_workspace,
        } => Some(Pdu::RenameWorkspace(codec::RenameWorkspace {
            old_workspace,
            new_workspace,
        })),
        MuxNotification::ActiveWorkspaceChanged(_) => None,
        MuxNotification::Empty => None,
        // LBTerm: a tab parked or unparked, told only to a client that asked which are, which an
        // older window never does: one would end its connection on it (PITFALLS 188). Its own
        // park included: a window's tab takes its flag from this push, never from its own word
        // (lbterm/MUX.md, "Parked tabs")
        MuxNotification::LbtermTabParked { tab_id, parked } => pending
            .lock()
            .unwrap()
            .knows_parked()
            .then(|| Pdu::TabParked(codec::TabParked { tab_id, parked })),
    }
}

/// LBTerm: an item from the connection's channel into the outbox, in the order they came
fn take_item(item: Item, pending: &Mutex<Pending>, outbox: &mut Outbox) -> anyhow::Result<()> {
    match item {
        Item::WritePdu(decoded) => outbox.add(&decoded).context("encoding PDU to client"),
        Item::Notif(n) => match notification_pdu(n, pending) {
            Some(pdu) => outbox.add_pdu(pdu).context("encoding PDU to client"),
            None => Ok(()),
        },
        Item::Owed | Item::Readable | Item::Writable | Item::HangupTick => Ok(()),
    }
}

pub async fn process_async<T>(mut stream: Async<T>) -> anyhow::Result<()>
where
    T: 'static,
    T: std::io::Read,
    T: std::io::Write,
    T: std::fmt::Debug,
    T: async_io::IoSafe,
{
    log::trace!("process_async called");
    // LBTerm: counted from before its subscription is made until it's done (LBTERM_CONNECTIONS)
    let _counted = Counted::new();

    let (item_tx, item_rx) = smol::channel::unbounded::<Item>();

    let pdu_sender = PduSender::new({
        let item_tx = item_tx.clone();
        move |pdu| {
            item_tx
                .try_send(Item::WritePdu(pdu))
                .map_err(|e| anyhow::anyhow!("{:?}", e))
        }
    });
    let mut handler = SessionHandler::new(pdu_sender);
    // LBTerm: this connection, whose own Resizes' TabResized it isn't told (lbterm_resize.rs)
    let conn = handler.lbterm_conn();

    // LBTerm: what this connection owes its client (lbterm_flow.rs). A pane's output and its
    // alerts, and a mux window's title, are noted there as they come, each pane and window once,
    // and the connection is woken once until it looks; upstream queued every notification in the
    // channel and made each pane's a task that worked out and queued a push. The handler keeps
    // it too, for the panes the client says it shows (SetShownPanes)
    let pending = handler.lbterm_pending();
    {
        let mux = Mux::get();
        let tx = item_tx.clone();
        let pending = Arc::clone(&pending);
        mux.subscribe(move |n| {
            let ring = match n {
                MuxNotification::PaneOutput(pane_id) => pending.lock().unwrap().output(pane_id),
                MuxNotification::Alert { pane_id, alert } => {
                    pending.lock().unwrap().alert(pane_id, alert)
                }
                MuxNotification::WindowTitleChanged { window_id, title } => {
                    pending.lock().unwrap().window_title(window_id, title)
                }
                // LBTerm: a layout this connection's own Resizes alone made is no news to it, and
                // a resync it would answer with sends back the sizes it's listed (lbterm_resize.rs)
                MuxNotification::TabResized(tab_id)
                    if crate::lbterm_resize::is_own(tab_id, conn) =>
                {
                    return !tx.is_closed()
                }
                n => return tx.try_send(Item::Notif(n)).is_ok(),
            };
            if ring {
                tx.try_send(Item::Owed).is_ok()
            } else {
                !tx.is_closed()
            }
        });
    }

    let hangup = std::env::var_os("LBTERM_TEST_HANGUP").map(std::path::PathBuf::from);

    // LBTerm: the client's PDUs come off a read at a time, not a byte at a
    // time (codec's PduReader), so frames read already wait there, and the
    // socket's readiness says nothing of them
    let mut reader = codec::PduReader::new();
    // LBTerm: what's encoded for the client and not yet taken by the socket (lbterm_flow.rs).
    // Upstream awaited each PDU's write in turn, so a client slow to read held the loop, and
    // with it the client's own keys, while everything behind queued
    let mut outbox = Outbox::default();
    // LBTerm: the timer for the soonest held pane's quiet deadline, with the deadline it was
    // armed for, kept across the loop's passes. An async-io Timer registers with the reactor the
    // first time it's polled, and `insert_timer` ends with `notify()`, waking the reactor's
    // thread (async-io 2.6.0's reactor.rs), so one made afresh each pass cost a wake each pass;
    // kept and polled by the same task it registers once. A held pane printing moves its
    // deadline later and leaves this as it is: it fires at the old one, the round finds nothing
    // quiet, and the next pass arms the new one (PITFALLS 107 and 203)
    let mut quiet_timer: Option<(std::time::Instant, smol::Timer)> = None;

    loop {
        if take_hangup(&hangup) {
            anyhow::bail!("hanging up on the client (LBTERM_TEST_HANGUP)");
        }

        // LBTerm: the answers and other news that came meanwhile, in order; then the windows'
        // titles and the pushes owed, the pane last typed into first, worked out only while the
        // client takes what's sent; then as much as the socket takes now
        while let Ok(item) = item_rx.try_recv() {
            take_item(item, &pending, &mut outbox)?;
        }
        // The client's next PDU stops the round: a key waits for no more pushes than the one
        // being worked out when it came. A flooding tab's push takes 1 to 3 ms, its terminal's
        // lock waited on behind the parser, and a round of six pushes 7 to 12 ms, 19 at the most
        // (2026-10-31): a key that came during a round waited for it, and then for the next,
        // before its own task ran
        let first = handler.lbterm_input_pane();
        crate::lbterm_flow::fill(
            &pending,
            &mut outbox,
            first,
            std::time::Instant::now(),
            || reader.read_now(&mut stream),
            |pane_id, owed, outbox| handler.lbterm_push(pane_id, owed, outbox),
        )
        .context("encoding PDU to client")?;
        match outbox.write_now(&mut stream) {
            Ok(()) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                ) =>
            {
                // Client disconnected: no need to make a noise (on Windows a socket whose
                // other end has gone says it was reset: an `lbterm cli` call that has its
                // answer and left before a window title or the like went out to it)
                return Ok(());
            }
            Err(err) => return Err(err).context("writing PDUs to client"),
        }

        // LBTerm: what's owed is noted in Pending, which tells the connection once until it next
        // waits, so it never waits while it owes what the outbox has room for: a socket that took
        // the whole outbox at once says nothing more, and no news may come (lbterm_flow.rs). And
        // while it holds a pane out of its client's view, the soonest that pane could have been
        // quiet long enough to be pushed: nothing else would tell it then
        let (wait, quiet) = {
            let mut pending = pending.lock().unwrap();
            (pending.wait(&outbox), pending.quiet_deadline())
        };
        handler.lbterm_round(wait);
        let rx_msg = item_rx.recv();
        let buffered = reader.has_buffered();
        let wait_for_read = async {
            if !buffered {
                let _ = stream.readable().await;
            }
            Ok(Item::Readable)
        };
        let wait_for_write = async {
            if wait == Wait::Writable {
                let _ = stream.writable().await;
            } else {
                futures::future::pending::<()>().await;
            }
            Ok(Item::Writable)
        };
        // Round again once other tasks have had a turn, a key's own among them
        let go_on = async {
            if wait == Wait::Now {
                smol::future::yield_now().await;
            } else {
                futures::future::pending::<()>().await;
            }
            Ok(Item::Owed)
        };
        // LBTerm: a held pane's quiet deadline; the round it wakes ends the holds gone quiet
        // (lbterm_flow::fill), and one that printed since waits for its new deadline. Armed
        // only when nothing is, or what's armed has fired, or the soonest deadline is earlier
        // than what's armed; dropped once nothing is held (a drop queues the timer's removal,
        // with no wake)
        quiet_timer = match (quiet, quiet_timer.take()) {
            (None, _) => None,
            (Some(at), Some((armed, timer))) if armed <= at && timer.will_fire() => {
                Some((armed, timer))
            }
            (Some(at), _) => Some((at, smol::Timer::at(at))),
        };
        let quiet_tick = async {
            match quiet_timer.as_mut() {
                Some((_, timer)) => {
                    timer.await;
                }
                None => futures::future::pending::<()>().await,
            }
            Ok(Item::Owed)
        };
        let hangup_tick = async {
            match &hangup {
                Some(_) => {
                    smol::Timer::after(std::time::Duration::from_millis(200)).await;
                }
                None => futures::future::pending::<()>().await,
            }
            Ok(Item::HangupTick)
        };

        // LBTerm: the client's next PDU first, so its keys are taken however much is going the
        // other way, then news, then the rounds owed
        match smol::future::or(
            smol::future::or(wait_for_read, rx_msg),
            smol::future::or(
                wait_for_write,
                smol::future::or(go_on, smol::future::or(quiet_tick, hangup_tick)),
            ),
        )
        .await
        {
            Ok(Item::Readable) => {
                let decoded = match reader.decode_async(&mut stream, None).await {
                    Ok(data) => data,
                    Err(err) => {
                        if let Some(err) = err.root_cause().downcast_ref::<std::io::Error>() {
                            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                                // Client disconnected: no need to make a noise
                                return Ok(());
                            }
                        }
                        return Err(err).context("reading Pdu from client");
                    }
                };
                handler.process_one(decoded);
                // LBTerm: the task process_one queued for it (a key, a paste) runs before this
                // loop works out the pushes it owes again
                smol::future::yield_now().await;
            }
            Ok(item) => take_item(item, &pending, &mut outbox)?,
            Err(err) => {
                log::error!("process_async Err {}", err);
                return Ok(());
            }
        }
    }
}
