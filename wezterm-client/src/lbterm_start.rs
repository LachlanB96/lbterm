//! LBTerm: a client's first connect to a unix domain's socket, when it may start the domain's
//! server (lbterm/MUX.md, "Starting the server"; lbterm/PERF.md, finding 25).
//!
//! Upstream tried the socket ten times, 50 to 450 ms apart, before it started the server, so
//! LBTerm opened from nothing on its mux domain waited 2.25 s for a socket nobody was going to
//! answer before the server's tab could start coming, and then up to 450 ms more between tries of
//! the server it had started. Here the client holds the socket's `starting` lock while it
//! connects, so a second client starting at once waits for it and then finds the server up; tries
//! the socket once; and when that's refused, starts the server at once unless one holds the
//! socket (mux/src/lbterm_serve.rs: one starting, or running with its file removed or taken,
//! which it binds again within seconds), whose answer it waits for instead and never starts a
//! second over. The server it starts is tried every 10 ms. A socket a live server holds is never
//! taken in any case: a server refuses to bind one that answers (wezterm-mux-server-impl's
//! `safely_create_sock_path`) or that another server has claimed (the server's `spawn_listener`).
//! Windows only, where a connect refused can't say a server is coming; elsewhere upstream's way.

use config::UnixTarget;
use mux::connui::ConnectionUI;
use wezterm_uds::UnixStream;

/// What a first try at the socket found
pub(crate) enum First {
    /// A server answered
    Connected(UnixStream),
    /// Refused, and no server holds it, so none can be listening: start one now. The connect's
    /// error, as upstream's tries said it
    NoServer(anyhow::Error),
}

/// A client's start at a socket: its `starting` lock, held until it's dropped (once connected, or
/// given up)
pub(crate) struct Start {
    #[cfg(windows)]
    path: std::path::PathBuf,
    #[cfg(windows)]
    _starting: mux::lbterm_serve::Starting,
}

/// The start of a connect to `target` that may start its server (`may_start`: a first connect
/// with auto-start allowed): `Some` for a socket on Windows, once it has the socket's `starting`
/// lock, waited for while another client starts the server (said on `ui`). `None` otherwise, for
/// upstream's tries.
pub(crate) fn begin(
    target: &UnixTarget,
    may_start: bool,
    ui: &ConnectionUI,
) -> anyhow::Result<Option<Start>> {
    #[cfg(windows)]
    if let (true, UnixTarget::Socket(path)) = (may_start, target) {
        let starting =
            mux::lbterm_serve::Starting::take(path, config::lbterm::MUX_STARTING_WAIT, || {
                ui.output_str("Another LBTerm is starting the server: waiting for it.\n")
            })?;
        if !starting.waited.is_zero() {
            log::info!(
                target: "lbterm::serve",
                "{}: waited {:?} for another client starting its server",
                path.display(),
                starting.waited
            );
        }
        return Ok(Some(Start {
            path: path.clone(),
            _starting: starting,
        }));
    }
    let _ = (target, may_start, ui);
    Ok(None)
}

#[cfg(windows)]
impl Start {
    /// One try. Refused while a server holds the socket (starting, or its file removed or taken
    /// from it), it tries again every `MUX_SERVED_LOOK` for that server, until it answers or lets
    /// go: `First::NoServer` once nobody holds it (at once, when nobody did). An error when it
    /// holds it and hasn't answered in `MUX_SERVED_WAIT`: no second server is started over it.
    pub(crate) fn first_try(&self) -> anyhow::Result<First> {
        use config::lbterm::{MUX_SERVED_LOOK, MUX_SERVED_WAIT};
        let mut refused = match UnixStream::connect(&self.path) {
            Ok(stream) => return Ok(First::Connected(stream)),
            Err(err) => err,
        };
        let began = std::time::Instant::now();
        let mut waiting = false;
        loop {
            if !mux::lbterm_serve::served(&self.path)? {
                if waiting {
                    log::info!(
                        target: "lbterm::serve",
                        "{}: the server holding it let go after {:?} without answering",
                        self.path.display(),
                        began.elapsed()
                    );
                }
                let err = anyhow::Error::new(refused)
                    .context(format!("connecting to {}", self.path.display()));
                return Ok(First::NoServer(err));
            }
            if !waiting {
                log::info!(
                    target: "lbterm::serve",
                    "{}: refused while a server holds it: waiting for that one ({refused})",
                    self.path.display()
                );
                waiting = true;
            }
            if began.elapsed() >= MUX_SERVED_WAIT {
                let pid = mux::lbterm_serve::server_pid(&self.path);
                anyhow::bail!(
                    "{}",
                    unanswered(
                        &self.path,
                        pid,
                        pid.map(mux::lbterm_procs::running),
                        began.elapsed(),
                        &refused
                    )
                );
            }
            std::thread::sleep(MUX_SERVED_LOOK);
            match UnixStream::connect(&self.path) {
                Ok(stream) => {
                    log::info!(
                        target: "lbterm::serve",
                        "{}: the server holding it answered after {:?}",
                        self.path.display(),
                        began.elapsed()
                    );
                    return Ok(First::Connected(stream));
                }
                Err(err) => refused = err,
            }
        }
    }

    /// The socket of the server this client has just started, tried every `MUX_START_LOOK` until
    /// it answers, for up to `MUX_START_WAIT`
    pub(crate) fn after_start(&self) -> anyhow::Result<UnixStream> {
        use config::lbterm::{MUX_START_LOOK, MUX_START_WAIT};
        let began = std::time::Instant::now();
        loop {
            match UnixStream::connect(&self.path) {
                Ok(stream) => {
                    log::info!(
                        target: "lbterm::serve",
                        "{}: the server started answered after {:?}",
                        self.path.display(),
                        began.elapsed()
                    );
                    return Ok(stream);
                }
                Err(err) if began.elapsed() >= MUX_START_WAIT => {
                    return Err(anyhow::Error::new(err)
                        .context(format!("connecting to {}", self.path.display())));
                }
                Err(_) => std::thread::sleep(MUX_START_LOOK),
            }
        }
    }
}

/// What a client says when the server holding the socket at `path` never answered in `waited`:
/// which process that is (`pid`, as it said; `running`, whether it still runs), for Lachlan to
/// end, since another server isn't started over it
#[cfg(windows)]
fn unanswered(
    path: &std::path::Path,
    pid: Option<u32>,
    running: Option<bool>,
    waited: std::time::Duration,
    refused: &std::io::Error,
) -> String {
    let who = match (pid, running) {
        (Some(pid), Some(true)) => format!(
            "the mux server with pid {pid}, still running, holds {}",
            path.display()
        ),
        (Some(pid), _) => format!(
            "the mux server with pid {pid} held {}, and has just ended",
            path.display()
        ),
        (None, _) => format!(
            "a mux server holds {} but doesn't say its pid (a build from before servers said it)",
            path.display()
        ),
    };
    format!(
        "{who}, and hasn't answered on it in {waited:?}, so another isn't started over it: end \
         that one and open LBTerm again ({refused})"
    )
}

#[cfg(not(windows))]
impl Start {
    pub(crate) fn first_try(&self) -> anyhow::Result<First> {
        unreachable!("begin starts nothing but on Windows")
    }

    pub(crate) fn after_start(&self) -> anyhow::Result<UnixStream> {
        unreachable!("begin starts nothing but on Windows")
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use wezterm_uds::UnixListener;

    /// A socket in a folder of its own, short enough for AF_UNIX's 108 characters, which goes
    /// with it
    struct TempSock(PathBuf);

    impl TempSock {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("lbst-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join("s"))
        }

        fn start(&self) -> Start {
            begin(
                &UnixTarget::Socket(self.0.clone()),
                true,
                &ConnectionUI::new_headless(),
            )
            .unwrap()
            .expect("a socket on Windows that may start its server")
        }
    }

    impl Drop for TempSock {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                std::fs::remove_dir_all(dir).ok();
            }
        }
    }

    fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
        let began = Instant::now();
        let t = f();
        (t, began.elapsed())
    }

    #[test]
    fn no_server_is_said_at_once() {
        // No file, then a file nothing listens on (a server that was killed leaves it)
        let sock = TempSock::new("none");
        for what in ["no file", "a dead server's file"] {
            let (first, took) = timed(|| sock.start().first_try().unwrap());
            assert!(matches!(first, First::NoServer(_)), "{}: no server", what);
            eprintln!("{what}: no server said in {took:?}");
            assert!(
                took < Duration::from_millis(250),
                "{}: took {:?}",
                what,
                took
            );
            if what == "no file" {
                // A listener gone, its file left behind
                drop(UnixListener::bind(&sock.0).unwrap());
            }
        }
    }

    #[test]
    fn a_live_server_is_connected_to() {
        let sock = TempSock::new("live");
        let _listener = UnixListener::bind(&sock.0).unwrap();
        assert!(matches!(
            sock.start().first_try().unwrap(),
            First::Connected(_)
        ));
    }

    #[test]
    fn a_server_holding_the_socket_is_waited_for() {
        let sock = TempSock::new("held");
        let path = sock.0.clone();
        // A server that has claimed the socket and binds it 300 ms on, as one whose file was
        // removed binds it again at its next look; its own thread, as its own process's would be
        let server = std::thread::spawn(move || {
            let serving = mux::lbterm_serve::claim(&path).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            let listener = UnixListener::bind(&path).unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(listener);
            drop(serving);
        });
        std::thread::sleep(Duration::from_millis(50));
        let (first, took) = timed(|| sock.start().first_try().unwrap());
        assert!(
            matches!(first, First::Connected(_)),
            "never started a second"
        );
        assert!(
            took >= Duration::from_millis(150) && took < Duration::from_secs(2),
            "{:?}",
            took
        );
        server.join().unwrap();
    }

    #[test]
    fn a_server_that_lets_go_without_answering_leaves_it_to_be_started() {
        let sock = TempSock::new("gone");
        let path = sock.0.clone();
        let server = std::thread::spawn(move || {
            let serving = mux::lbterm_serve::claim(&path).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            drop(serving);
        });
        std::thread::sleep(Duration::from_millis(50));
        let (first, took) = timed(|| sock.start().first_try().unwrap());
        assert!(matches!(first, First::NoServer(_)));
        assert!(
            took >= Duration::from_millis(150) && took < Duration::from_secs(2),
            "{:?}",
            took
        );
        server.join().unwrap();
    }

    #[test]
    fn a_server_that_never_answers_is_named() {
        let path = PathBuf::from(r"C:\Users\someone\.local\share\lbterm\sock");
        let refused = std::io::Error::from_raw_os_error(10061);
        let waited = Duration::from_secs(7);
        let said = unanswered(&path, Some(4242), Some(true), waited, &refused);
        assert!(said.contains("pid 4242, still running"), "{}", said);
        assert!(said.contains(r"lbterm\sock"), "{}", said);
        assert!(said.contains("end that one"), "{}", said);
        let said = unanswered(&path, Some(4242), Some(false), waited, &refused);
        assert!(said.contains("pid 4242 held") && said.contains("has just ended"), "{}", said);
        let said = unanswered(&path, None, None, waited, &refused);
        assert!(said.contains("doesn't say its pid"), "{}", said);
    }

    #[test]
    fn a_server_holding_the_socket_says_its_pid() {
        let sock = TempSock::new("pid");
        let path = sock.0.clone();
        assert_eq!(mux::lbterm_serve::server_pid(&path), None);
        let p = path.clone();
        let (said, heard) = std::sync::mpsc::channel();
        let (done, end) = std::sync::mpsc::channel::<()>();
        // Its own thread, as its own process's main thread would hold it
        let server = std::thread::spawn(move || {
            let serving = mux::lbterm_serve::claim(&p).unwrap();
            said.send(()).unwrap();
            end.recv().ok();
            drop(serving);
        });
        heard.recv().unwrap();
        assert_eq!(
            mux::lbterm_serve::server_pid(&path),
            Some(std::process::id())
        );
        done.send(()).unwrap();
        server.join().unwrap();
        assert_eq!(mux::lbterm_serve::server_pid(&path), None);
    }

    #[test]
    fn a_started_server_is_connected_to_as_soon_as_it_listens() {
        let sock = TempSock::new("started");
        let path = sock.0.clone();
        let server = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            let listener = UnixListener::bind(&path).unwrap();
            std::thread::sleep(Duration::from_secs(1));
            drop(listener);
        });
        let (stream, took) = timed(|| sock.start().after_start());
        assert!(stream.is_ok());
        eprintln!("a server listening 120 ms on was connected to in {took:?}");
        // Upstream's next try after 120 ms was at 250 ms (200, then 50 more)
        assert!(took < Duration::from_millis(200), "{:?}", took);
        server.join().unwrap();
    }
}
