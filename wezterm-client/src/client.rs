use crate::domain::{ClientDomain, ClientDomainConfig};
use crate::lbterm_backlog::{Applied, Backlog};
use crate::pane::ClientPane;
use anyhow::{anyhow, bail, Context};
use async_ossl::AsyncSslStream;
use async_trait::async_trait;
use codec::*;
use config::{configuration, SshDomain, TlsDomainClient, UnixDomain, UnixTarget};
use filedescriptor::FileDescriptor;
use mux::client::ClientId;
use mux::connui::ConnectionUI;
use mux::domain::DomainId;
use mux::pane::PaneId;
use mux::ssh::ssh_connect_with_ui;
use mux::Mux;
use openssl::ssl::{SslConnector, SslFiletype, SslMethod};
use openssl::x509::X509;
use portable_pty::Child;
use smol::channel::{bounded, unbounded, Receiver, Sender};
use smol::prelude::*;
use smol::{block_on, Async};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::marker::Unpin;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, AsSocket, BorrowedSocket, RawSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use thiserror::Error;
use wezterm_uds::UnixStream;

#[derive(Error, Debug)]
#[error("Timeout")]
struct Timeout;

/// LBTerm: what's told of each PDU the client sends or receives: its type, its payload bytes, and
/// whether it was sent. wezterm-gui sets it to its counters (lbterm_extras::counters)
pub type TrafficHook = fn(name: &'static str, bytes: usize, sent: bool);
static LBTERM_TRAFFIC: std::sync::OnceLock<TrafficHook> = std::sync::OnceLock::new();

/// LBTerm: tell `hook` of every PDU from now on
pub fn lbterm_set_traffic_hook(hook: TrafficHook) {
    let _ = LBTERM_TRAFFIC.set(hook);
}

#[derive(Error, Debug)]
#[error("ChannelSendError")]
struct ChannelSendError;

enum ReaderMessage {
    SendPdu {
        pdu: Pdu,
        promise: Sender<anyhow::Result<Pdu>>,
        /// LBTerm: a read's ask of a held pane's push, which the GUI thread may wait on with
        /// nothing else running: read on for its answer (lbterm_current.rs, `Input::awaited`)
        awaited: bool,
    },
    Readable,
    /// LBTerm: the GUI thread has applied enough of what it was handed to read on
    /// (lbterm_backlog.rs)
    Room,
    /// LBTerm: a notice of the pushes a frame read (codec's PanesDrawn), sent at serial 0 with no
    /// promise, since the server answers nothing, and only while this connection's server knows
    /// it (lbterm_drawn.rs, `GateWatch`)
    Drawn(codec::PanesDrawn),
}

#[derive(Clone)]
pub struct Client {
    sender: Sender<ReaderMessage>,
    /// LBTerm: whether the connection is up, for the scroll strip's seek (lbterm_link.rs)
    link: Arc<crate::lbterm_link::Link>,
    /// LBTerm: what a read of a held pane shares with the client thread: the backlog, and
    /// whether an ask ran out its bound (lbterm_current.rs)
    lbterm_reads: Arc<crate::lbterm_current::Reads>,
    /// LBTerm: whether this connection's server knows PanesDrawn, which the client thread says
    /// (lbterm_drawn.rs)
    lbterm_gate: Arc<crate::lbterm_drawn::Gate>,
    local_domain_id: Option<DomainId>,
    pub client_id: ClientId,
    client_domain_config: ClientDomainConfig,
    pub is_reconnectable: bool,
    pub is_local: bool,
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error(
    "Please install the same version of wezterm on both the client and server!\n\
     The server version is {} (codec version {}),\n\
     which is not compatible with our version \n\
     {} (codec version {}).",
    version,
    codec_vers,
    config::wezterm_version(),
    CODEC_VERSION
)]
pub struct IncompatibleVersionError {
    pub version: String,
    pub codec_vers: usize,
}

macro_rules! rpc {
    ($method_name:ident, $request_type:ident, $response_type:ident) => {
        pub async fn $method_name(&self, pdu: $request_type) -> anyhow::Result<$response_type> {
            let start = std::time::Instant::now();
            let result = self.send_pdu(Pdu::$request_type(pdu)).await;
            let elapsed = start.elapsed();
            metrics::histogram!("rpc", "method" => stringify!($method_name)).record(elapsed);
            metrics::counter!("rpc.count", "method" => stringify!($method_name)).increment(1);
            match result {
                Ok(Pdu::$response_type(res)) => Ok(res),
                Ok(_) => bail!("unexpected response {:?}", result),
                Err(err) => Err(err),
            }
        }
    };

    // This variant allows omitting the request parameter; this is useful
    // in the case where the struct is empty and present only for the purpose
    // of typing the request.
    ($method_name:ident, $request_type:ident=(), $response_type:ident) => {
        #[allow(dead_code)]
        pub async fn $method_name(&self) -> anyhow::Result<$response_type> {
            let start = std::time::Instant::now();
            let result = self.send_pdu(Pdu::$request_type($request_type{})).await;
            let elapsed = start.elapsed();
            metrics::histogram!("rpc", "method" => stringify!($method_name)).record(elapsed);
            metrics::counter!("rpc.count", "method" => stringify!($method_name)).increment(1);
            match result {
                Ok(Pdu::$response_type(res)) => Ok(res),
                Ok(_) => bail!("unexpected response {:?}", result),
                Err(err) => Err(err),
            }
        }
    };
}

fn process_unilateral_inner(
    pane_id: PaneId,
    local_domain_id: DomainId,
    decoded: DecodedPdu,
    mut applied: Applied,
) {
    // LBTerm: the pane waits for it until it's applied, even by a task that goes on (below), so a
    // push of its title alone isn't applied on the client thread ahead of it (lbterm_names.rs)
    let waits = applied.goes_on();
    let mut task = Box::pin(async move {
        let _waits = waits;
        process_unilateral_inner_async(pane_id, local_domain_id, decoded).await?;
        Ok::<(), anyhow::Error>(())
    });
    // LBTerm: applied here, on the GUI thread, as the backlog's task takes it (lbterm_backlog.rs);
    // one that waits on the server (a resync, an image's data) goes on as a task of its own, as
    // upstream's each did, and counts as applied, since the client thread must read on for it
    if futures::FutureExt::now_or_never(&mut task).is_none() {
        promise::spawn::spawn(task).detach();
    }
    drop(applied);
}

async fn process_unilateral_inner_async(
    pane_id: PaneId,
    local_domain_id: DomainId,
    decoded: DecodedPdu,
) -> anyhow::Result<()> {
    let mux = match Mux::try_get() {
        Some(mux) => mux,
        None => {
            // This can happen for some client scenarios; it is ok to ignore it.
            return Ok(());
        }
    };

    let client_domain = mux
        .get_domain(local_domain_id)
        .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
    let client_domain = client_domain
        .downcast_ref::<ClientDomain>()
        .ok_or_else(|| anyhow!("domain {} is not a ClientDomain instance", local_domain_id))?;

    // If we get a push for a pane that we don't yet know about,
    // it means that some other client has manipulated the mux
    // topology; we need to re-sync.
    let local_pane_id = match client_domain.remote_to_local_pane_id(pane_id) {
        Some(p) => p,
        None => {
            log::debug!("got {decoded:?}, pane not found locally, resync");
            client_domain.resync().await?;
            client_domain
                .remote_to_local_pane_id(pane_id)
                .ok_or_else(|| {
                    anyhow!("remote pane id {} does not have a local pane id", pane_id)
                })?
        }
    };

    let pane = match mux.get_pane(local_pane_id) {
        Some(p) => p,
        None => {
            log::debug!("got {decoded:?}, but local pane {local_pane_id} no longer exists; resync");
            client_domain.resync().await?;

            let local_pane_id =
                client_domain
                    .remote_to_local_pane_id(pane_id)
                    .ok_or_else(|| {
                        anyhow!("remote pane id {} does not have a local pane id", pane_id)
                    })?;

            mux.get_pane(local_pane_id)
                .ok_or_else(|| anyhow!("local pane {local_pane_id} not found"))?
        }
    };
    let client_pane = pane.downcast_ref::<ClientPane>().ok_or_else(|| {
        log::error!(
            "received unilateral PDU for pane {} which is \
                     not an instance of ClientPane: {:?}",
            local_pane_id,
            decoded.pdu
        );
        anyhow!(
            "received unilateral PDU for pane {} which is \
                     not an instance of ClientPane: {:?}",
            local_pane_id,
            decoded.pdu
        )
    })?;
    client_pane.process_unilateral(decoded.pdu).await
}

fn process_unilateral(
    local_domain_id: Option<DomainId>,
    decoded: DecodedPdu,
    backlog: &Arc<Backlog>,
    first: Option<PaneId>,
) -> anyhow::Result<()> {
    let local_domain_id = match local_domain_id {
        Some(id) => id,
        None => {
            // FIXME: We currently get a bunch of these; we'll need
            // to do something to advise the server when we want them.
            // For now, we just ignore them.
            log::trace!(
                "client doesn't have a real local domain, \
                 so unilateral message cannot be processed by it"
            );
            return Ok(());
        }
    };
    // LBTerm: handed to the GUI thread through the connection's backlog, which merges a pane's
    // pushes as they come and gives them to the GUI thread in order, which hands each on as
    // upstream did here (lbterm_backlog.rs); one this can't handle still ends the connection
    // here, as it did
    match &decoded.pdu {
        Pdu::WindowWorkspaceChanged(_)
        | Pdu::WindowTitleChanged(_)
        | Pdu::RenameWorkspace(_)
        | Pdu::TabTitleChanged(_)
        | Pdu::TabResized(_)
        | Pdu::TabParked(_)
        | Pdu::TabAddedToWindow(_) => {}
        pdu if pdu.pane_id().is_some() => {}
        _ => bail!("don't know how to handle {:?}", decoded),
    }
    // LBTerm: a push of a pane's title and folder alone (a tab out of view's title) is applied
    // here as it's read, when nothing older of the pane waits, and handed to nothing: no take,
    // no wake, no task on the GUI thread (lbterm_names.rs; lbterm/FLOWS.md, "A title out of
    // view", What could go 13, part 1)
    if crate::lbterm_names::applied_here(local_domain_id, &decoded.pdu, backlog) {
        return Ok(());
    }
    // LBTerm: its title alert, dropped here when no window would do anything for it, and the
    // server's mux window title, kept here while nothing reads it as it changes: neither handed
    // (lbterm_titles.rs; What could go 13, parts 2 and 3)
    if crate::lbterm_titles::decided_here(local_domain_id, &decoded.pdu, backlog) {
        return Ok(());
    }
    let handed = backlog.hand(decoded, first);
    if let (true, Some(pane_id)) = (handed.take_first, first) {
        // The pushes of the pane typed into, ahead of the rest and their gap: its echo. Only its
        // pushes: everything else keeps its place among the other panes' (lbterm_backlog.rs)
        let backlog = Arc::clone(backlog);
        promise::spawn::spawn_into_main_thread(async move {
            for (decoded, applied) in backlog.take_first(pane_id) {
                process_unilateral_now(local_domain_id, decoded, applied);
            }
            // LBTerm: the mux window titles the client thread kept (lbterm_titles.rs)
            crate::lbterm_titles::set_kept();
        })
        .detach();
    }
    if handed.take_all {
        let backlog = Arc::clone(backlog);
        promise::spawn::spawn_into_main_thread(async move {
            // Right after a long one, the loop turns first: a frame waiting paints
            // (lbterm_backlog::GAP, GAP_AFTER)
            if let Some(wait) = backlog.gap(std::time::Instant::now()) {
                async_io::Timer::after(wait).await;
            }
            let started = std::time::Instant::now();
            for (decoded, applied) in backlog.take() {
                process_unilateral_now(local_domain_id, decoded, applied);
            }
            // LBTerm: the mux window titles the client thread kept (lbterm_titles.rs)
            crate::lbterm_titles::set_kept();
            backlog.drained(started, std::time::Instant::now());
        })
        .detach();
    }
    Ok(())
}

/// What upstream's process_unilateral did with a unilateral PDU, now on the GUI thread as its
/// backlog's task takes it
fn process_unilateral_now(local_domain_id: DomainId, decoded: DecodedPdu, applied: Applied) {
    match &decoded.pdu {
        Pdu::WindowWorkspaceChanged(WindowWorkspaceChanged {
            window_id,
            workspace,
        }) => {
            let window_id = *window_id;
            let workspace = workspace.to_string();
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                let local_window_id = client_domain
                    .remote_to_local_window_id(window_id)
                    .ok_or_else(|| anyhow!("no local window for remote window id {}", window_id))?;
                if let Some(mut window) = mux.get_window_mut(local_window_id) {
                    window.set_workspace(&workspace);
                }

                anyhow::Result::<()>::Ok(())
            })
            .detach();

            return;
        }
        Pdu::WindowTitleChanged(WindowTitleChanged { window_id, title }) => {
            let title = title.to_string();
            let window_id = *window_id;
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                client_domain.process_remote_window_title_change(window_id, title);
                anyhow::Result::<()>::Ok(())
            })
            .detach();
            return;
        }
        Pdu::RenameWorkspace(RenameWorkspace {
            old_workspace,
            new_workspace,
        }) => {
            let old_workspace = old_workspace.to_string();
            let new_workspace = new_workspace.to_string();
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                log::debug!("got a rename {old_workspace} -> {new_workspace}");
                mux.rename_workspace(&old_workspace, &new_workspace);
                anyhow::Result::<()>::Ok(())
            })
            .detach();
            return;
        }
        Pdu::TabTitleChanged(TabTitleChanged { tab_id, title }) => {
            let title = title.to_string();
            let tab_id = *tab_id;
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                client_domain.process_remote_tab_title_change(tab_id, title);
                anyhow::Result::<()>::Ok(())
            })
            .detach();
            return;
        }
        // LBTerm: a tab parked or unparked at the server, this window's own park included: the
        // local tab takes its flag from here alone (lbterm/MUX.md, "Parked tabs"). Already on
        // the GUI thread, outside any notification
        Pdu::TabParked(TabParked { tab_id, parked }) => {
            match Mux::try_get()
                .and_then(|mux| mux.get_domain(local_domain_id))
                .as_ref()
                .and_then(|d| d.downcast_ref::<ClientDomain>())
            {
                Some(domain) => domain.lbterm_remote_tab_parked(*tab_id, *parked),
                None => log::error!("parked: no client domain {local_domain_id} for {:?}", decoded.pdu),
            }
            return;
        }
        Pdu::TabResized(_) | Pdu::TabAddedToWindow(_) => {
            log::trace!("resync due to {:?}", decoded.pdu);
            promise::spawn::spawn_into_main_thread(async move {
                let mux = Mux::try_get().ok_or_else(|| anyhow!("no more mux"))?;
                let client_domain = mux
                    .get_domain(local_domain_id)
                    .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                let client_domain =
                    client_domain
                        .downcast_ref::<ClientDomain>()
                        .ok_or_else(|| {
                            anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                        })?;

                client_domain.resync().await
            })
            .detach();

            return;
        }
        _ => {}
    }

    if let Some(pane_id) = decoded.pdu.pane_id() {
        // LBTerm: on the GUI thread already, as upstream's task here was
        process_unilateral_inner(pane_id, local_domain_id, decoded, applied);
    }
}

#[derive(Error, Debug, Clone, PartialEq, Eq)]
enum NotReconnectableError {
    #[error("Client was destroyed")]
    ClientWasDestroyed,
}

/// What the client thread does once its connection to the server has ended.
#[derive(Debug, PartialEq, Eq)]
enum AfterDisconnect {
    /// Give up: the domain detaches and its panes leave the window
    Detach(&'static str),
    /// Connect again, at most this many times (None: until it works)
    Reconnect { attempts: Option<usize> },
}

/// LBTerm: how many times a unix domain tries to connect again before it
/// detaches, a second apart and doubling to 10 s: about a minute in all
const UNIX_RECONNECT_ATTEMPTS: usize = 8;

/// LBTerm: upstream detaches a unix domain at once, and any domain on a
/// plain EOF. LBTerm's tabs live in a long-running lbterm-mux-server, which
/// can hang up on a GUI it still has tabs for: on 2026-10-23 the PC hit its
/// commit limit four times in 25 minutes, the server failed to encode a PDU
/// each time and dropped the GUI (an EOF to the GUI), and the window closed
/// with all 22 of its tabs although the server and the tabs ran on. So a
/// unix domain tries again after any error, EOF included, and detaches only
/// when no server has answered `UNIX_RECONNECT_ATTEMPTS` times.
fn after_disconnect(
    unix: bool,
    reconnectable: bool,
    has_local_domain: bool,
    err: &anyhow::Error,
) -> AfterDisconnect {
    if !reconnectable || !has_local_domain {
        return AfterDisconnect::Detach("client thread ended");
    }
    if err
        .root_cause()
        .downcast_ref::<NotReconnectableError>()
        .is_some()
    {
        return AfterDisconnect::Detach("won't try to reconnect");
    }
    if unix {
        return AfterDisconnect::Reconnect {
            attempts: Some(UNIX_RECONNECT_ATTEMPTS),
        };
    }
    if let Some(ioerr) = err.root_cause().downcast_ref::<std::io::Error>() {
        if let std::io::ErrorKind::UnexpectedEof = ioerr.kind() {
            // Don't reconnect for a simple EOF
            return AfterDisconnect::Detach("server closed connection");
        }
    }
    AfterDisconnect::Reconnect { attempts: None }
}

fn client_thread(
    reconnectable: &mut Reconnectable,
    local_domain_id: Option<DomainId>,
    rx: &mut Receiver<ReaderMessage>,
    reads: &crate::lbterm_current::Reads,
    gate: &crate::lbterm_drawn::Gate,
) -> anyhow::Result<()> {
    block_on(client_thread_async(reconnectable, local_domain_id, rx, reads, gate))
}

async fn client_thread_async(
    reconnectable: &mut Reconnectable,
    local_domain_id: Option<DomainId>,
    rx: &mut Receiver<ReaderMessage>,
    reads: &crate::lbterm_current::Reads,
    gate: &crate::lbterm_drawn::Gate,
) -> anyhow::Result<()> {
    let mut next_serial = 1u64;

    struct Promises {
        map: HashMap<u64, Sender<anyhow::Result<Pdu>>>,
    }

    impl Promises {
        fn fail_all(&mut self, reason: &str) {
            log::trace!("failing all promises: {}", reason);
            for (_, promise) in self.map.drain() {
                let _ = promise.try_send(Err(anyhow!("{}", reason)));
            }
        }
    }

    impl Drop for Promises {
        fn drop(&mut self) {
            self.fail_all("Client was destroyed");
        }
    }
    let mut promises = Promises {
        map: HashMap::new(),
    };

    let mut stream = reconnectable.take_stream().unwrap();
    // LBTerm: the server's PDUs come off a read at a time, not a byte at a
    // time (codec's PduReader), so frames read already wait there, and the
    // stream's readiness says nothing of them
    let mut reader = PduReader::new();
    // LBTerm: what's handed to the GUI thread and not yet applied (lbterm_backlog.rs)
    let backlog = Backlog::new();
    // A read of a held pane applies its pushes from here (lbterm_current.rs)
    reads.connected(&backlog);
    // LBTerm: input waiting on its answer, and the echo after it, which reading isn't held for
    // (lbterm_backlog::Input)
    let mut input = crate::lbterm_backlog::Input::default();
    // LBTerm: whether this connection's server knows PanesDrawn, shut until it answers a
    // SetShownPanes (lbterm_drawn.rs)
    let mut drawn_gate = crate::lbterm_drawn::GateWatch::connected(gate);

    loop {
        let rx_msg = rx.recv();
        let buffered = reader.has_buffered();
        // LBTerm: once the GUI thread has the backlog's limit to apply, no reading until it has
        // applied them all, unless input waits on its answer or its echo; what's sent goes on
        // meanwhile
        let held = backlog.full() && !input.reads_on(std::time::Instant::now());
        let wait_for_read = async {
            if held {
                backlog.wait_until_applied().await;
                return Ok(ReaderMessage::Room);
            }
            if !buffered {
                let _ = stream.wait_for_readable().await;
            }
            Ok(ReaderMessage::Readable)
        };

        match smol::future::or(rx_msg, wait_for_read).await {
            Ok(ReaderMessage::SendPdu { pdu, promise, awaited }) => {
                let serial = next_serial;
                next_serial += 1;
                promises.map.insert(serial, promise);
                input.sent(serial, &pdu);
                drawn_gate.sent(serial, &pdu);
                if awaited {
                    input.awaited(serial);
                }

                pdu.encode_async(&mut stream, serial)
                    .await
                    .context("encoding a PDU to send to the server")?;
                if let Some(hook) = LBTERM_TRAFFIC.get() {
                    hook(pdu.pdu_name(), codec::lbterm_last_size(), true);
                }
                stream.flush().await.context("flushing PDU to server")?;
            }
            Ok(ReaderMessage::Readable) => {
                match reader.decode_async(&mut stream, Some(next_serial)).await {
                    Ok(decoded) => {
                        log::debug!(
                            "decoded serial {} {}",
                            decoded.serial,
                            decoded.pdu.pdu_name()
                        );
                        if let Some(hook) = LBTERM_TRAFFIC.get() {
                            hook(decoded.pdu.pdu_name(), codec::lbterm_last_size(), false);
                        }
                        if decoded.serial == 0 {
                            if let Pdu::GetPaneRenderChangesResponse(push) = &decoded.pdu {
                                input.pushed(push.pane_id);
                            }
                            process_unilateral(local_domain_id, decoded, &backlog, input.first())
                                .context("processing unilateral PDU from server")
                                .map_err(|e| {
                                    log::error!("process_unilateral: {:?}", e);
                                    e
                                })?;
                        } else if let Some(promise) = promises.map.remove(&decoded.serial) {
                            if input.answered(decoded.serial, std::time::Instant::now()) {
                                reads.answered();
                            }
                            drawn_gate.answered(decoded.serial, &decoded.pdu);
                            if promise.try_send(Ok(decoded.pdu)).is_err() {
                                return Err(NotReconnectableError::ClientWasDestroyed.into());
                            }
                        } else {
                            let reason =
                                format!("got serial {:?} without a corresponding promise", decoded);
                            promises.fail_all(&reason);
                            anyhow::bail!("{}", reason);
                        }
                    }
                    Err(err) => {
                        let reason = format!("Error while decoding response pdu: {:#}", err);
                        log::error!("{}", reason);
                        promises.fail_all(&reason);
                        return Err(err).context("Error while decoding response pdu");
                    }
                }
            }
            Ok(ReaderMessage::Room) => {}
            // LBTerm: a notice: at serial 0, nothing put in `promises` and `next_serial` left
            // where it is, since the server answers nothing (PITFALLS 227); dropped on a
            // connection whose server hasn't said it knows it, one queued before a reconnect
            // included
            Ok(ReaderMessage::Drawn(drawn)) => {
                if drawn_gate.open() {
                    Pdu::PanesDrawn(drawn)
                        .encode_async(&mut stream, 0)
                        .await
                        .context("encoding PanesDrawn to send to the server")?;
                    if let Some(hook) = LBTERM_TRAFFIC.get() {
                        hook("PanesDrawn", codec::lbterm_last_size(), true);
                    }
                    stream.flush().await.context("flushing PanesDrawn to server")?;
                } else {
                    log::debug!(
                        target: "lbterm::drawn",
                        "PanesDrawn for {} panes not sent: this connection's server hasn't said it knows it",
                        drawn.panes.len()
                    );
                }
            }
            Err(_) => {
                return Err(NotReconnectableError::ClientWasDestroyed.into());
            }
        }
    }
}

static LBTERM_STARTED_SERVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// LBTerm: whether this process started a mux server, finding none on its socket. When it
/// didn't and it has tabs in one, it reattached to tabs a GUI before it left running (a
/// restart, a crash): the window strip says so (wezterm-gui's restart.rs).
pub fn lbterm_started_server() -> bool {
    LBTERM_STARTED_SERVER.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn unix_connect_with_retry(
    target: &UnixTarget,
    just_spawned: bool,
    max_attempts: Option<u64>,
) -> anyhow::Result<UnixStream> {
    let mut error = None;

    if just_spawned {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    let max_attempts = max_attempts.unwrap_or(10);

    for iter in 0..max_attempts {
        if iter > 0 {
            std::thread::sleep(std::time::Duration::from_millis(iter * 50));
        }
        match target {
            UnixTarget::Socket(path) => match UnixStream::connect(path) {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    error =
                        Some(Err(err).with_context(|| format!("connecting to {}", path.display())))
                }
            },
            UnixTarget::Proxy(argv) => {
                let mut cmd = std::process::Command::new(&argv[0]);
                cmd.args(&argv[1..]);

                let (a, b) = filedescriptor::socketpair()?;

                cmd.stdin(b.as_stdio()?);
                cmd.stdout(b.as_stdio()?);
                cmd.stderr(std::process::Stdio::inherit());
                let mut child = cmd
                    .spawn()
                    .with_context(|| format!("spawning proxy command {:?}", cmd))?;

                error.take();

                // Grace period to detect whether connection failed
                for _ in 0..5 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            error = Some(Err(anyhow!(
                                "{:?} exited already with status {:?}",
                                cmd,
                                status
                            )));
                            continue;
                        }
                        Ok(None) => {
                            error.take();
                        }
                        Err(err) => {
                            error =
                                Some(Err(err).context(format!("spawning proxy command {:?}", cmd)));
                            continue;
                        }
                    }
                }

                if error.is_none() {
                    #[cfg(unix)]
                    unsafe {
                        use std::os::unix::io::{FromRawFd, IntoRawFd};
                        return Ok(UnixStream::from_raw_fd(a.into_raw_fd()));
                    }
                    #[cfg(windows)]
                    unsafe {
                        use std::os::windows::io::{FromRawSocket, IntoRawSocket};
                        return Ok(UnixStream::from_raw_socket(a.into_raw_socket()));
                    }
                }
            }
        }
    }

    error.expect("only get here after at least one unix fail")
}

#[async_trait(?Send)]
pub trait AsyncReadAndWrite: Unpin + AsyncRead + AsyncWrite + std::fmt::Debug + Send {
    async fn wait_for_readable(&self) -> anyhow::Result<()>;
}

#[async_trait(?Send)]
impl<T> AsyncReadAndWrite for Async<T>
where
    T: std::fmt::Debug,
    T: std::io::Write,
    T: std::io::Read,
    T: Send,
    T: async_io::IoSafe,
{
    async fn wait_for_readable(&self) -> anyhow::Result<()> {
        Ok(self.readable().await?)
    }
}

#[derive(Debug)]
struct Reconnectable {
    config: ClientDomainConfig,
    stream: Option<Box<dyn AsyncReadAndWrite>>,
    tls_creds: Option<GetTlsCredsResponse>,
}

struct SshStream {
    stdin: FileDescriptor,
    stdout: FileDescriptor,
}

unsafe impl async_io::IoSafe for SshStream {}

impl std::fmt::Debug for SshStream {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(fmt, "SshStream {{...}}")
    }
}

#[cfg(unix)]
impl AsFd for SshStream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stdout.as_fd()
    }
}

#[cfg(unix)]
impl AsRawFd for SshStream {
    fn as_raw_fd(&self) -> RawFd {
        self.stdout.as_raw_fd()
    }
}

#[cfg(windows)]
impl AsRawSocket for SshStream {
    fn as_raw_socket(&self) -> RawSocket {
        self.stdout.as_raw_socket()
    }
}

#[cfg(windows)]
impl AsSocket for SshStream {
    fn as_socket(&self) -> BorrowedSocket<'_> {
        self.stdout.as_socket()
    }
}

impl Read for SshStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.stdout.read(buf)
    }
}

impl Write for SshStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.stdin.write(buf)
    }
    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.stdin.flush()
    }
}

impl Reconnectable {
    fn new(config: ClientDomainConfig, stream: Option<Box<dyn AsyncReadAndWrite>>) -> Self {
        Self {
            config,
            stream,
            tls_creds: None,
        }
    }

    fn tls_creds_path(&self) -> anyhow::Result<PathBuf> {
        let path = config::pki_dir()?.join(self.config.name());
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn tls_creds_ca_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.tls_creds_path()?.join("ca.pem"))
    }

    fn tls_creds_cert_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.tls_creds_path()?.join("cert.pem"))
    }

    fn take_stream(&mut self) -> Option<Box<dyn AsyncReadAndWrite>> {
        self.stream.take()
    }

    fn is_local(&mut self) -> bool {
        matches!(&self.config, ClientDomainConfig::Unix(_))
    }

    fn reconnectable(&mut self) -> bool {
        match &self.config {
            // LBTerm: upstream never reconnects a unix domain, reasoning that
            // its server only goes away by dying. LBTerm's server can hang up
            // on a live GUI (`after_disconnect`), so it reconnects, never
            // starting a server, and gives up when none answers
            ClientDomainConfig::Unix(_) => true,
            ClientDomainConfig::Tls(_) => true,
            // It *does* make sense to reconnect with an ssh session, but we
            // need to grow some smarts about whether the disconnect was because
            // we sent CTRL-D to close the last session, or whether it was a network
            // level disconnect, because we will otherwise throw up authentication
            // dialogs that would be annoying
            ClientDomainConfig::Ssh(_) => false,
        }
    }

    fn connect(
        &mut self,
        initial: bool,
        ui: &mut ConnectionUI,
        no_auto_start: bool,
    ) -> anyhow::Result<()> {
        match self.config.clone() {
            ClientDomainConfig::Unix(unix_dom) => {
                self.unix_connect(unix_dom, initial, ui, no_auto_start)
            }
            ClientDomainConfig::Tls(tls) => self.tls_connect(tls, initial, ui),
            ClientDomainConfig::Ssh(ssh) => self.ssh_connect(ssh, initial, ui),
        }
    }

    /// Resolve the path to wezterm for the remote system.
    /// We can't simply derive this from the current executable because
    /// we are being asked to produce a path for the remote system and
    /// we don't really know anything about it.
    /// `path` comes from the SshDoman::remote_wezterm_path option; if set
    /// then the user has told us where to look.
    /// Otherwise, we have to rely on the `PATH` environment for the remote
    /// system, and we don't know if it is even running unix, or whether
    /// any given shell syntax will help us provide a more meaningful
    /// message to the user.
    fn wezterm_bin_path(path: &Option<String>) -> String {
        path.as_deref().unwrap_or("wezterm").to_string()
    }

    fn ssh_connect(
        &mut self,
        ssh_dom: SshDomain,
        initial: bool,
        ui: &mut ConnectionUI,
    ) -> anyhow::Result<()> {
        let ssh_config = mux::ssh::ssh_domain_to_ssh_config(&ssh_dom)?;

        let sess = ssh_connect_with_ui(ssh_config, ui)?;
        let proxy_bin = Self::wezterm_bin_path(&ssh_dom.remote_wezterm_path);

        let cmd = if let Some(cmd) = ssh_dom.override_proxy_command.clone() {
            cmd
        } else if initial {
            format!("{} cli --prefer-mux proxy", proxy_bin)
        } else {
            format!("{} cli --prefer-mux --no-auto-start proxy", proxy_bin)
        };
        ui.output_str(&format!("Running: {}\n", cmd));
        log::debug!("going to run {}", cmd);

        let exec = smol::block_on(sess.exec(&cmd, None))?;

        let mut stderr = exec.stderr;
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while let Ok(len) = stderr.read(&mut buf) {
                if len == 0 {
                    break;
                } else {
                    let stderr = &buf[0..len];
                    log::error!("ssh stderr: {}", String::from_utf8_lossy(stderr));
                }
            }
        });

        // This is a bit gross, but it helps to surface errors in running
        // the proxy, and prevents us from hanging forever after the process
        // has died
        let mut child = exec.child;
        std::thread::spawn(move || match child.wait() {
            Err(err) => log::error!("waiting on {} failed: {:#}", cmd, err),
            Ok(status) if !status.success() => log::error!("{}: {}", cmd, status),
            _ => {}
        });

        let stream: Box<dyn AsyncReadAndWrite> = Box::new(Async::new(SshStream {
            stdin: exec.stdin,
            stdout: exec.stdout,
        })?);
        self.stream.replace(stream);
        Ok(())
    }

    fn unix_connect(
        &mut self,
        unix_dom: UnixDomain,
        initial: bool,
        ui: &mut ConnectionUI,
        no_auto_start: bool,
    ) -> anyhow::Result<()> {
        let target = unix_dom.target();
        ui.output_str(&format!("Connect to {:?}\n", target));
        log::trace!("connect to {:?}", target);

        let max_attempts = if no_auto_start { Some(1) } else { None };

        // LBTerm: a first connect that may start the server holds the socket's lock for starting
        // it until it's connected, tries the socket once, and starts the server at once when
        // none holds it, where upstream tried ten times over 2.25 s first (lbterm_start.rs;
        // lbterm/PERF.md, finding 25)
        let lbterm = crate::lbterm_start::begin(
            &target,
            initial && !no_auto_start && !unix_dom.no_serve_automatically,
            ui,
        )?;
        let tried = match &lbterm {
            Some(start) => match start.first_try()? {
                crate::lbterm_start::First::Connected(stream) => Ok(stream),
                crate::lbterm_start::First::NoServer(e) => Err(e),
            },
            None => unix_connect_with_retry(&target, false, max_attempts),
        };

        let stream = match tried {
            Ok(stream) => stream,
            Err(e) => {
                if no_auto_start || unix_dom.no_serve_automatically || !initial {
                    bail!("failed to connect to {:?}: {}", target, e);
                }
                log::warn!(
                    "While connecting to {:?}: {}.  Will try spawning the server.",
                    target,
                    e
                );
                ui.output_str(&format!("Error: {}.  Will try spawning server.\n", e));

                let argv = unix_dom.serve_command()?;

                let mut cmd = std::process::Command::new(&argv[0]);
                cmd.args(&argv[1..]);

                // LBTerm: the mux server is a console program, and a GUI has no console to
                // lend it, so Windows opened one for it (a Windows Terminal window, in front
                // and taking the keyboard) for the moment before it detached itself
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }

                #[cfg(unix)]
                if let Some(mask) = umask::UmaskSaver::saved_umask() {
                    unsafe {
                        cmd.pre_exec(move || {
                            libc::umask(mask);
                            Ok(())
                        });
                    }
                }

                log::warn!("Running: {:?}", cmd);
                ui.output_str(&format!("Running: {:?}\n", cmd));
                // LBTerm: a GUI that started its server found no tabs running there to
                // reattach to (lbterm_started_server)
                LBTERM_STARTED_SERVER.store(true, std::sync::atomic::Ordering::Relaxed);

                let child = cmd
                    .spawn()
                    .with_context(|| format!("while spawning {:?}", cmd))?;
                std::thread::spawn(move || match child.wait_with_output() {
                    Ok(out) => {
                        if let Ok(stdout) = std::str::from_utf8(&out.stdout) {
                            if !stdout.is_empty() {
                                log::warn!("stdout: {}", stdout);
                            }
                        }
                        if let Ok(stderr) = std::str::from_utf8(&out.stderr) {
                            if !stderr.is_empty() {
                                log::warn!("stderr: {}", stderr);
                            }
                        }
                    }
                    Err(err) => {
                        log::error!("spawn: {:#}", err);
                    }
                });

                // LBTerm: tried every 10 ms, where upstream waited 200 ms and then 50 to 450
                match &lbterm {
                    Some(start) => start.after_start(),
                    None => unix_connect_with_retry(&target, true, None),
                }
                .with_context(|| {
                    format!("(after spawning server) failed to connect to {:?}", target)
                })?
            }
        };
        drop(lbterm);

        ui.output_str("Connected!\n");
        stream.set_read_timeout(Some(unix_dom.read_timeout))?;
        stream.set_write_timeout(Some(unix_dom.write_timeout))?;
        let stream: Box<dyn AsyncReadAndWrite> = Box::new(Async::new(stream)?);
        self.stream.replace(stream);
        Ok(())
    }

    pub fn tls_connect(
        &mut self,
        tls_client: TlsDomainClient,
        _initial: bool,
        ui: &mut ConnectionUI,
    ) -> anyhow::Result<()> {
        openssl::init();

        let remote_address = &tls_client.remote_address;

        let remote_host_name = remote_address.split(':').next().ok_or_else(|| {
            anyhow!(
                "expected mux_server_remote_address to have the form 'host:port', but have {}",
                remote_address
            )
        })?;

        // If we are reconnecting and already bootstrapped via SSH, let's see if
        // we can connect using those same credentials and avoid running through
        // the SSH authentication flow.
        if let Some(Ok(_)) = tls_client.ssh_parameters() {
            match self.try_connect(&tls_client, ui, &remote_address, remote_host_name) {
                Ok(stream) => {
                    self.stream.replace(stream);
                    return Ok(());
                }
                Err(err) => {
                    if let Some(ioerr) = err.root_cause().downcast_ref::<std::io::Error>() {
                        match ioerr.kind() {
                            std::io::ErrorKind::ConnectionRefused => {
                                // Server isn't up yet; let's proceed with bootstrap
                            }
                            _ => {
                                // If it is an IO error that implies that we had an issue
                                // reaching or otherwise talking to the remote host.
                                // Re-attempting the SSH bootstrap most likely will not
                                // succeed so we let this bubble up.
                                return Err(err);
                            }
                        }
                    }
                    ui.output_str(&format!(
                        "Failed to reuse creds: {:?}\nWill retry bootstrap via SSH\n",
                        err
                    ));
                }
            }
        }

        if let Some(Ok(ssh_params)) = tls_client.ssh_parameters() {
            if self.tls_creds.is_none() {
                // We need to bootstrap via an ssh session

                let mut ssh_config = wezterm_ssh::Config::new();
                ssh_config.add_default_config_files();

                let mut fields = ssh_params.host_and_port.split(':');
                let host = fields
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("no host component somehow"))?;
                let port = fields.next();

                let mut ssh_config = ssh_config.for_host(host);
                if let Some(username) = &ssh_params.username {
                    ssh_config.insert("user".to_string(), username.to_string());
                }
                if let Some(port) = port {
                    ssh_config.insert("port".to_string(), port.to_string());
                }

                let sess = ssh_connect_with_ui(ssh_config, ui)?;

                let creds = ui.run_and_log_error(|| {
                    // The `tlscreds` command will start the server if needed and then
                    // obtain client credentials that we can use for tls.
                    let cmd = format!(
                        "{} cli tlscreds",
                        Self::wezterm_bin_path(&tls_client.remote_wezterm_path)
                    );

                    ui.output_str(&format!("Running: {}\n", cmd));
                    let mut exec = smol::block_on(sess.exec(&cmd, None))
                        .with_context(|| format!("executing `{}` on remote host", cmd))?;

                    log::debug!("waiting for command to finish");
                    let status = exec.child.wait()?;
                    if !status.success() {
                        anyhow::bail!("{} failed", cmd);
                    }

                    drop(exec.stdin);

                    let mut stderr = exec.stderr;
                    thread::spawn(move || {
                        // stderr is ideally empty
                        let mut err = String::new();
                        let _ = stderr.read_to_string(&mut err);
                        if !err.is_empty() {
                            log::error!("remote: `{}` stderr -> `{}`", cmd, err);
                        }
                    });

                    let creds = match Pdu::decode(exec.stdout)
                        .context("reading tlscreds response")?
                        .pdu
                    {
                        Pdu::GetTlsCredsResponse(creds) => creds,
                        _ => bail!("unexpected response to tlscreds"),
                    };

                    // Save the credentials to disk, as that is currently the easiest
                    // way to get them into openssl.  Ideally we'd keep these entirely
                    // in memory.
                    std::fs::write(&self.tls_creds_ca_path()?, creds.ca_cert_pem.as_bytes())?;
                    std::fs::write(
                        &self.tls_creds_cert_path()?,
                        creds.client_cert_pem.as_bytes(),
                    )?;
                    log::info!("got TLS creds");
                    Ok(creds)
                })?;
                self.tls_creds.replace(creds);
            }
        }

        let cloned_ui = ui.clone();
        let stream = cloned_ui.run_and_log_error({
            || self.try_connect(&tls_client, ui, &remote_address, remote_host_name)
        })?;
        self.stream.replace(stream);
        Ok(())
    }

    fn try_connect(
        &mut self,
        tls_client: &TlsDomainClient,
        ui: &mut ConnectionUI,
        remote_address: &str,
        remote_host_name: &str,
    ) -> anyhow::Result<Box<dyn AsyncReadAndWrite>> {
        let mut connector = SslConnector::builder(SslMethod::tls())?;

        let cert_file = match tls_client.pem_cert.clone() {
            Some(cert) => cert,
            None => self.tls_creds_cert_path()?,
        };

        connector
            .set_certificate_file(&cert_file, SslFiletype::PEM)
            .context(format!(
                "set_certificate_file to {} for TLS client",
                cert_file.display()
            ))?;

        if let Some(chain_file) = tls_client.pem_ca.as_ref() {
            connector
                .set_certificate_chain_file(&chain_file)
                .context(format!(
                    "set_certificate_chain_file to {} for TLS client",
                    chain_file.display()
                ))?;
        }

        let key_file = match tls_client.pem_private_key.clone() {
            Some(key) => key,
            None => self.tls_creds_cert_path()?,
        };
        connector
            .set_private_key_file(&key_file, SslFiletype::PEM)
            .context(format!(
                "set_private_key_file to {} for TLS client",
                key_file.display()
            ))?;

        fn load_cert(name: &Path) -> anyhow::Result<X509> {
            let cert_bytes = std::fs::read(name)?;
            log::trace!("loaded {}", name.display());
            Ok(X509::from_pem(&cert_bytes)?)
        }
        for name in &tls_client.pem_root_certs {
            if name.is_dir() {
                for entry in std::fs::read_dir(name)? {
                    if let Ok(cert) = load_cert(&entry?.path()) {
                        connector.cert_store_mut().add_cert(cert).ok();
                    }
                }
            } else {
                connector.cert_store_mut().add_cert(load_cert(name)?)?;
            }
        }

        if let Ok(ca_path) = self.tls_creds_ca_path() {
            if ca_path.exists() {
                connector.cert_store_mut().add_cert(load_cert(&ca_path)?)?;
            }
        }

        let connector = connector.build();
        let connector = connector
            .configure()?
            .verify_hostname(!tls_client.accept_invalid_hostnames);

        ui.output_str(&format!("Connecting to {} using TLS\n", remote_address));
        let stream = TcpStream::connect(remote_address)
            .with_context(|| format!("connecting to {}", remote_address))?;
        stream.set_nodelay(true)?;
        stream.set_write_timeout(Some(tls_client.write_timeout))?;
        stream.set_read_timeout(Some(tls_client.read_timeout))?;

        let stream = Box::new(Async::new(AsyncSslStream::new(
            connector
                .connect(
                    tls_client
                        .expected_cn
                        .as_deref()
                        .unwrap_or(remote_host_name),
                    stream,
                )
                .with_context(|| {
                    format!(
                        "SslConnector for {} with host name {}",
                        remote_address, remote_host_name,
                    )
                })?,
        ))?);
        ui.output_str("TLS Connected!\n");
        Ok(stream)
    }
}

impl Client {
    fn new(local_domain_id: Option<DomainId>, mut reconnectable: Reconnectable) -> Self {
        let client_domain_config = reconnectable.config.clone();
        let is_reconnectable = reconnectable.reconnectable();
        let is_local = reconnectable.is_local();
        let (sender, mut receiver) = unbounded();
        let client_id = ClientId::new();
        let link = Arc::new(crate::lbterm_link::Link::default());
        let thread_link = Arc::clone(&link);
        let lbterm_reads = Arc::new(crate::lbterm_current::Reads::default());
        let thread_reads = Arc::clone(&lbterm_reads);
        let lbterm_gate = Arc::new(crate::lbterm_drawn::Gate::default());
        let thread_gate = Arc::clone(&lbterm_gate);

        thread::spawn(move || {
            const BASE_INTERVAL: Duration = Duration::from_secs(1);
            const MAX_INTERVAL: Duration = Duration::from_secs(10);

            let mut backoff = BASE_INTERVAL;
            loop {
                if let Err(e) = client_thread(&mut reconnectable, local_domain_id, &mut receiver, &thread_reads, &thread_gate) {
                    // LBTerm: down from here: every request waits in the channel for the next
                    // connection, a write as a key does, and the scroll strip seeks nowhere
                    // meanwhile (lbterm_link.rs)
                    thread_link.went_down();
                    let attempts = match after_disconnect(
                        is_local,
                        reconnectable.reconnectable(),
                        local_domain_id.is_some(),
                        &e,
                    ) {
                        AfterDisconnect::Detach(why) => {
                            if local_domain_id.is_some() {
                                log::error!("{} ({:#})", why, e);
                            } else {
                                log::debug!("{}: {}", why, e);
                            }
                            break;
                        }
                        AfterDisconnect::Reconnect { attempts } => attempts,
                    };
                    // LBTerm: said only of a connection that's tried again, not at every cli's end
                    log::warn!(
                        "the connection to the mux server ended ({:#}): {} requests, keys and \
                         writes alike, wait for it to come back",
                        e,
                        receiver.len()
                    );

                    let local_domain_id = local_domain_id.expect("checked above");

                    // LBTerm: a unix domain reconnects without a window. The
                    // connection window would come up in front and take the
                    // keyboard while Lachlan types elsewhere; the log has it
                    let mut ui = if is_local {
                        ConnectionUI::new_headless()
                    } else {
                        ConnectionUI::new()
                    };
                    ui.title("wezterm: Reconnecting...");

                    let mut failures = 0;
                    let reconnected = loop {
                        ui.sleep_with_reason(
                            &format!("client disconnected {}; will reconnect", e),
                            backoff,
                        )
                        .ok();
                        let initial = false;
                        let no_auto_start = true; // Don't auto-start on a reconnect
                        match reconnectable.connect(initial, &mut ui, no_auto_start) {
                            Ok(_) => {
                                backoff = BASE_INTERVAL;
                                log::error!("Reconnected!");
                                // LBTerm: what waited goes first, in its order (lbterm_link.rs)
                                thread_link.came_up();
                                promise::spawn::spawn_into_main_thread(async move {
                                    if let Err(err) =
                                        ClientDomain::reattach(local_domain_id, ui).await
                                    {
                                        log::error!("reattaching after a reconnect: {:#}", err);
                                    }
                                })
                                .detach();
                                break true;
                            }
                            Err(err) => {
                                failures += 1;
                                if attempts.map_or(false, |most| failures >= most) {
                                    log::error!(
                                        "problem reconnecting: {}; no server answered {} times, giving up",
                                        err,
                                        failures
                                    );
                                    ui.close();
                                    break false;
                                }
                                backoff = (backoff + backoff).min(MAX_INTERVAL);
                                let what = format!(
                                    "problem reconnecting: {}; will reconnect in {:?}",
                                    err, backoff
                                );
                                if is_local {
                                    log::warn!("{}", what);
                                }
                                ui.output_str(&format!("{}\n", what));
                            }
                        }
                    };
                    if !reconnected {
                        break;
                    }
                } else {
                    log::error!("client_thread returned without any error condition");
                    break;
                }
            }
            // LBTerm: what waited for a connection that won't come is failed as never sent,
            // rather than dropped, so a write says it never went, not that it may have
            // (lbterm_link.rs, GaveUp)
            while let Ok(request) = receiver.try_recv() {
                if let ReaderMessage::SendPdu { promise, .. } = request {
                    let _ = promise.try_send(Err(crate::lbterm_link::GaveUp.into()));
                }
            }

            async fn detach(local_domain_id: DomainId) -> anyhow::Result<()> {
                if let Some(mux) = Mux::try_get() {
                    let client_domain = mux
                        .get_domain(local_domain_id)
                        .ok_or_else(|| anyhow!("no such domain {}", local_domain_id))?;
                    let client_domain =
                        client_domain
                            .downcast_ref::<ClientDomain>()
                            .ok_or_else(|| {
                                anyhow!("domain {} is not a ClientDomain instance", local_domain_id)
                            })?;
                    client_domain.perform_detach();
                }
                Ok(())
            }
            if let Some(domain_id) = local_domain_id {
                promise::spawn::spawn_into_main_thread(async move {
                    detach(domain_id).await.ok();
                })
                .detach();
            }
        });

        Self {
            sender,
            link,
            lbterm_reads,
            lbterm_gate,
            local_domain_id,
            is_reconnectable,
            is_local,
            client_id,
            client_domain_config,
        }
    }

    pub fn into_client_domain_config(self) -> ClientDomainConfig {
        self.client_domain_config
    }

    pub async fn verify_version_compat(
        &self,
        ui: &ConnectionUI,
    ) -> anyhow::Result<GetCodecVersionResponse> {
        match self
            .get_codec_version(GetCodecVersion {})
            .or(async {
                smol::Timer::after(Duration::from_secs(60)).await;
                Err(Timeout).context("Timeout")
            })
            .await
        {
            Ok(info) if info.codec_vers == CODEC_VERSION => {
                log::trace!(
                    "Server version is {} (codec version {})",
                    info.version_string,
                    info.codec_vers
                );
                self.set_client_id(SetClientId {
                    client_id: self.client_id.clone(),
                    is_proxy: false,
                })
                .await?;
                Ok(info)
            }
            Ok(info) => {
                let err = IncompatibleVersionError {
                    version: info.version_string,
                    codec_vers: info.codec_vers,
                };
                ui.output_str(&err.to_string());
                log::error!("{:?}", err);
                return Err(err.into());
            }
            Err(err) => {
                log::trace!("{:?}", err);
                let msg = if err.root_cause().is::<Timeout>() {
                    "Timed out while parsing the response from the server. \
                    This may be due to network connectivity issues"
                        .to_string()
                } else if err.root_cause().is::<CorruptResponse>() {
                    "Received an implausible and likely corrupt response from \
                    the server. This can happen if the remote host outputs \
                    to stdout prior to running commands. \
                    Check your shell startup!"
                        .to_string()
                } else if err.root_cause().is::<ChannelSendError>() {
                    "Internal channel was closed prior to sending request. \
                    This may indicate that the remote host output invalid data \
                    to stdout prior to running the requested command. \
                    Check your shell startup!"
                        .to_string()
                } else {
                    format!(
                        "Please install the same version of wezterm on both \
                     the client and server! \
                     The server reported error '{err}' while being asked for its \
                     version.  This likely means that the server is older \
                     than the client, but it could also happen if the remote \
                     host outputs to stdout prior to running commands. \
                     Check your shell startup!",
                    )
                };
                ui.output_str(&msg);
                bail!("{}", msg);
            }
        }
    }

    #[allow(dead_code)]
    pub fn local_domain_id(&self) -> Option<DomainId> {
        self.local_domain_id
    }

    fn compute_unix_domain(
        prefer_mux: bool,
        class_name: &str,
    ) -> anyhow::Result<config::UnixDomain> {
        match std::env::var_os("WEZTERM_UNIX_SOCKET") {
            Some(path) if !path.is_empty() => Ok(config::UnixDomain {
                socket_path: Some(path.into()),
                ..Default::default()
            }),
            Some(_) | None => {
                if !prefer_mux {
                    if let Ok(gui) = crate::discovery::resolve_gui_sock_path(class_name) {
                        return Ok(config::UnixDomain {
                            socket_path: Some(gui),
                            no_serve_automatically: true,
                            ..Default::default()
                        });
                    }
                }

                let config = configuration();
                Ok(config
                    .unix_domains
                    .first()
                    .ok_or_else(|| {
                        anyhow!(
                            "no default unix domain is configured and WEZTERM_UNIX_SOCKET \
                             is not set in the environment"
                        )
                    })?
                    .clone())
            }
        }
    }

    pub fn new_default_unix_domain(
        initial: bool,
        ui: &mut ConnectionUI,
        no_auto_start: bool,
        prefer_mux: bool,
        class_name: &str,
    ) -> anyhow::Result<Self> {
        let unix_dom = Self::compute_unix_domain(prefer_mux, class_name)?;
        Self::new_unix_domain(None, &unix_dom, initial, ui, no_auto_start)
    }

    pub fn new_unix_domain(
        local_domain_id: Option<DomainId>,
        unix_dom: &UnixDomain,
        initial: bool,
        ui: &mut ConnectionUI,
        no_auto_start: bool,
    ) -> anyhow::Result<Self> {
        let mut reconnectable =
            Reconnectable::new(ClientDomainConfig::Unix(unix_dom.clone()), None);
        reconnectable.connect(initial, ui, no_auto_start)?;
        Ok(Self::new(local_domain_id, reconnectable))
    }

    pub fn new_tls(
        local_domain_id: DomainId,
        tls_client: &TlsDomainClient,
        ui: &mut ConnectionUI,
    ) -> anyhow::Result<Self> {
        let mut reconnectable =
            Reconnectable::new(ClientDomainConfig::Tls(tls_client.clone()), None);
        let no_auto_start = true;
        reconnectable.connect(true, ui, no_auto_start)?;
        Ok(Self::new(Some(local_domain_id), reconnectable))
    }

    pub fn new_ssh(
        local_domain_id: DomainId,
        ssh_dom: &SshDomain,
        ui: &mut ConnectionUI,
    ) -> anyhow::Result<Self> {
        let mut reconnectable = Reconnectable::new(ClientDomainConfig::Ssh(ssh_dom.clone()), None);
        let no_auto_start = true;
        reconnectable.connect(true, ui, no_auto_start)?;
        Ok(Self::new(Some(local_domain_id), reconnectable))
    }

    pub async fn send_pdu(&self, pdu: Pdu) -> anyhow::Result<Pdu> {
        let (promise, rx) = bounded(1);
        self.sender
            .send(ReaderMessage::SendPdu {
                pdu,
                promise,
                awaited: false,
            })
            .await
            .map_err(|_| ChannelSendError)
            .context("send_pdu send")?;
        rx.recv().await.context("send_pdu recv")?
    }

    /// LBTerm: whether the connection is down, its requests waiting for the next (lbterm_link.rs)
    pub fn lbterm_link_down(&self) -> bool {
        self.link.is_down()
    }

    /// LBTerm: what reads of held panes share with the client thread (lbterm_current.rs)
    pub fn lbterm_reads(&self) -> &crate::lbterm_current::Reads {
        &self.lbterm_reads
    }

    /// LBTerm: whether this connection's server knows PanesDrawn (lbterm_drawn.rs), an atomic
    /// load: the GUI thread builds no notice for one that doesn't
    pub fn lbterm_says_drawn(&self) -> bool {
        self.lbterm_gate.is_open()
    }

    /// LBTerm: the pushes a frame read, each pane by the server's id and the seqno of its newest
    /// push applied, sent as one PanesDrawn without waiting (the server answers nothing): the
    /// client thread sends it only while its server knows it (lbterm_drawn.rs)
    pub fn lbterm_panes_drawn(&self, panes: Vec<codec::PaneDrawn>) {
        if panes.is_empty() || !self.lbterm_gate.is_open() {
            return;
        }
        let _ = self
            .sender
            .try_send(ReaderMessage::Drawn(codec::PanesDrawn { panes }));
    }

    /// LBTerm: a held pane's push asked of the server for a read (GetPaneRenderChanges, which
    /// the server answers after the push), sent without waiting, so from Lua's synchronous
    /// methods too; the client thread reads on for its answer whatever its backlog holds. Where
    /// the answer comes, which must be kept until it does (PITFALLS 224); None when the client
    /// thread has ended (it gave up reconnecting)
    pub fn lbterm_ask_changes(&self, pane_id: PaneId) -> Option<Receiver<anyhow::Result<Pdu>>> {
        let (promise, answer) = bounded(1);
        self.sender
            .try_send(ReaderMessage::SendPdu {
                pdu: Pdu::GetPaneRenderChanges(GetPaneRenderChanges { pane_id }),
                promise,
                awaited: true,
            })
            .ok()?;
        Some(answer)
    }

    /// LBTerm: the pushes of `pane_id` (the server's id) the client thread has handed over and
    /// the GUI thread hasn't taken, applied now, as the backlog's task would apply them, ahead of
    /// what's for other panes, as the typed pane's are (lbterm_backlog.rs, `Backlog::take_pane`):
    /// a read that asked for the pane's push and has its answer, on the GUI thread
    pub fn lbterm_apply_handed(&self, pane_id: PaneId) {
        let (Some(domain_id), Some(backlog)) = (self.local_domain_id, self.lbterm_reads.backlog()) else {
            return;
        };
        for (decoded, applied) in backlog.take_pane(pane_id) {
            process_unilateral_now(domain_id, decoded, applied);
        }
    }

    /// LBTerm: write to a pane and hear what came of it (lbterm_link.rs), counted as
    /// `write_to_pane` was
    pub async fn lbterm_write_to_pane(&self, pdu: WriteToPane) -> Option<crate::lbterm_link::Failure> {
        let start = std::time::Instant::now();
        // A client whose thread has ended (it gave up reconnecting) can't queue it: never sent
        let answer = self.send_pdu(Pdu::WriteToPane(pdu)).await.map_err(|err| {
            if err.root_cause().is::<ChannelSendError>() {
                crate::lbterm_link::GaveUp.into()
            } else {
                err
            }
        });
        metrics::histogram!("rpc", "method" => "write_to_pane").record(start.elapsed());
        metrics::counter!("rpc.count", "method" => "write_to_pane").increment(1);
        crate::lbterm_link::Failure::of(answer)
    }

    pub async fn resolve_pane_id(&self, pane_id: Option<PaneId>) -> anyhow::Result<PaneId> {
        let pane_id: PaneId = match pane_id {
            Some(p) => p,
            None => {
                if let Ok(pane) = std::env::var("WEZTERM_PANE") {
                    pane.parse()?
                } else {
                    let mut clients = self.list_clients().await?.clients;
                    clients.retain(|client| client.focused_pane_id.is_some());
                    clients.sort_by(|a, b| b.last_input.cmp(&a.last_input));
                    if clients.is_empty() {
                        anyhow::bail!(
                            "--pane-id was not specified and $WEZTERM_PANE
                         is not set in the environment, and I couldn't
                         determine which pane was currently focused"
                        );
                    }

                    clients[0]
                        .focused_pane_id
                        .expect("to have filtered out above")
                }
            }
        };
        Ok(pane_id)
    }

    rpc!(ping, Ping = (), Pong);
    rpc!(list_panes, ListPanes = (), ListPanesResponse);
    rpc!(spawn_v2, SpawnV2, SpawnResponse);
    rpc!(spawn_no_activate, SpawnNoActivate, SpawnResponse);
    rpc!(split_pane, SplitPane, SpawnResponse);
    rpc!(
        move_pane_to_new_tab,
        MovePaneToNewTab,
        MovePaneToNewTabResponse
    );
    rpc!(write_to_pane, WriteToPane, UnitResponse);
    rpc!(send_paste, SendPaste, UnitResponse);
    rpc!(key_down, SendKeyDown, UnitResponse);
    rpc!(mouse_event, SendMouseEvent, UnitResponse);
    rpc!(resize, Resize, UnitResponse);
    rpc!(set_zoomed, SetPaneZoomed, UnitResponse);
    rpc!(activate_pane_direction, ActivatePaneDirection, UnitResponse);
    rpc!(
        get_pane_render_changes,
        GetPaneRenderChanges,
        LivenessResponse
    );
    rpc!(get_lines, GetLines, GetLinesResponse);
    rpc!(get_prompt_rows, GetPromptRows, GetPromptRowsResponse);
    rpc!(
        get_dimensions,
        GetPaneRenderableDimensions,
        GetPaneRenderableDimensionsResponse
    );
    rpc!(get_codec_version, GetCodecVersion, GetCodecVersionResponse);
    rpc!(get_tls_creds, GetTlsCreds = (), GetTlsCredsResponse);
    rpc!(
        search_scrollback,
        SearchScrollbackRequest,
        SearchScrollbackResponse
    );
    rpc!(kill_pane, KillPane, UnitResponse);
    rpc!(set_client_id, SetClientId, UnitResponse);
    rpc!(list_clients, GetClientList = (), GetClientListResponse);
    rpc!(set_window_workspace, SetWindowWorkspace, UnitResponse);
    rpc!(set_focused_pane_id, SetFocusedPane, UnitResponse);
    // LBTerm: what the windows show (lbterm_shown.rs)
    rpc!(set_shown_panes, SetShownPanes, UnitResponse);
    // LBTerm: parked tabs (lbterm/MUX.md, "Parked tabs")
    rpc!(get_parked_tabs, GetParkedTabs, ParkedTabs);
    rpc!(set_tab_parked, TabParked, UnitResponse);
    rpc!(get_image_cell, GetImageCell, GetImageCellResponse);
    rpc!(set_configured_palette_for_pane, SetPalette, UnitResponse);
    rpc!(set_tab_title, TabTitleChanged, UnitResponse);
    rpc!(set_window_title, WindowTitleChanged, UnitResponse);
    rpc!(rename_workspace, RenameWorkspace, UnitResponse);
    rpc!(erase_scrollback, EraseScrollbackRequest, UnitResponse);
    rpc!(
        get_pane_direction,
        GetPaneDirection,
        GetPaneDirectionResponse
    );
    rpc!(adjust_pane_size, AdjustPaneSize, UnitResponse);
}

#[cfg(test)]
impl Client {
    /// LBTerm: a client with no thread and no connection, for tests of what its panes keep: what
    /// it would send goes nowhere
    pub(crate) fn lbterm_unconnected() -> Self {
        let (sender, _receiver) = unbounded();
        Self {
            sender,
            link: Default::default(),
            lbterm_reads: Default::default(),
            lbterm_gate: Default::default(),
            local_domain_id: None,
            client_id: ClientId::new(),
            client_domain_config: ClientDomainConfig::Unix(UnixDomain::default()),
            is_reconnectable: false,
            is_local: true,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn eof() -> anyhow::Error {
        // What the GUI got each time the server hung up on 2026-10-23
        anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "EOF while reading leb128 encoded value",
        ))
        .context("decode_raw_async failed to read PDU length")
        .context("decoding a PDU")
    }

    fn reset() -> anyhow::Error {
        anyhow::Error::new(std::io::Error::from_raw_os_error(10054))
            .context("decode_raw_async failed to read PDU length")
    }

    const UNIX: bool = true;
    const TLS: bool = false;

    #[test]
    fn lbterm_unix_domain_reconnects_when_the_server_hangs_up() {
        for err in [eof(), reset(), anyhow!("got serial 7 without a corresponding promise")] {
            assert_eq!(
                after_disconnect(UNIX, true, true, &err),
                AfterDisconnect::Reconnect {
                    attempts: Some(UNIX_RECONNECT_ATTEMPTS)
                },
                "{err:#}"
            );
        }
    }

    #[test]
    fn lbterm_unix_domain_detaches_when_the_client_is_gone() {
        let err = anyhow::Error::new(NotReconnectableError::ClientWasDestroyed);
        assert!(matches!(
            after_disconnect(UNIX, true, true, &err),
            AfterDisconnect::Detach(_)
        ));
    }

    #[test]
    fn lbterm_cli_connections_never_reconnect() {
        // `lbterm cli` has no domain of its own: it ends with its connection
        assert!(matches!(
            after_disconnect(UNIX, true, false, &eof()),
            AfterDisconnect::Detach(_)
        ));
    }

    #[test]
    fn tls_and_ssh_keep_upstream_behaviour() {
        assert!(matches!(
            after_disconnect(TLS, true, true, &eof()),
            AfterDisconnect::Detach("server closed connection")
        ));
        assert_eq!(
            after_disconnect(TLS, true, true, &reset()),
            AfterDisconnect::Reconnect { attempts: None }
        );
        // ssh isn't reconnectable
        assert!(matches!(
            after_disconnect(TLS, false, true, &reset()),
            AfterDisconnect::Detach(_)
        ));
    }

    #[test]
    fn lbterm_unix_domain_is_reconnectable() {
        let mut r = Reconnectable::new(ClientDomainConfig::Unix(UnixDomain::default()), None);
        assert!(r.reconnectable());
        assert!(r.is_local());
    }
}
