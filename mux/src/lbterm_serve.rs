//! LBTerm: who is there for a mux domain's socket (lbterm/MUX.md, "Starting the server";
//! lbterm/PERF.md, finding 25).
//!
//! Upstream's client tried a unix domain's socket ten times, 50 to 450 ms apart (2.25 s), before
//! it started the server, in case one was on its way up, so LBTerm started from nothing on its mux
//! domain waited all of that for a socket nobody was going to answer. On Windows a connect can't
//! tell a server on its way up from none: no file and a file nothing listens on are both refused
//! at once (WSAECONNREFUSED), and a live listener takes every connect, its backlog full or not
//! (measured 2026-10-31). So two named mutexes of the socket's path say who's there
//! (`config::lbterm::mux_socket_lock`):
//!
//! - `serving`: the server's, taken just before it binds the socket and held for as long as it
//!   runs. A second server for the same socket finds it owned and refuses to start, so two never
//!   share one (each one's `keep_socket` would take the socket back from the other every 3 s),
//!   and a client the socket refuses while it's there waits for that server rather than start
//!   another.
//! - `starting`: a client's, while it connects to the socket and, when no server holds it, starts
//!   one and waits for it to answer, so two clients starting at once (two windows opened
//!   together) start one server: the second waits for the first, then finds the server up.
//!
//! Neither handle is inheritable, so a server's shells and a client's server never hold them, and
//! a process that ends lets go of its own: a server that crashed leaves nothing saying it's
//! there. Windows only, as the clients that start a server here are.
//!
//! A server holding `serving` also says its process id in a named section of its own
//! (`config::lbterm::mux_server_pid`), held as long as the lock, so a client that such a server
//! never answers names the process to end (`server_pid`): a mutex can't say who owns it. Nothing
//! is written beside the socket, so nothing can outlive the server and name another process.

use anyhow::bail;
use config::lbterm::{mux_server_pid, mux_socket_lock, MuxSocketLock};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::path::Path;
use std::time::{Duration, Instant};

type Handle = *mut c_void;

const SYNCHRONIZE: u32 = 0x0010_0000;
const PAGE_READWRITE: u32 = 0x04;
const FILE_MAP_WRITE: u32 = 0x02;
const FILE_MAP_READ: u32 = 0x04;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED: u32 = 0x80;
const WAIT_TIMEOUT: u32 = 0x102;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_ACCESS_DENIED: i32 = 5;

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attributes: *const c_void, initial_owner: i32, name: *const u16) -> Handle;
    fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
    fn ReleaseMutex(handle: Handle) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn CreateFileMappingW(
        file: Handle,
        attributes: *const c_void,
        protect: u32,
        size_high: u32,
        size_low: u32,
        name: *const u16,
    ) -> Handle;
    fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn MapViewOfFile(
        mapping: Handle,
        access: u32,
        offset_high: u32,
        offset_low: u32,
        bytes: usize,
    ) -> *mut c_void;
    fn UnmapViewOfFile(view: *const c_void) -> i32;
}

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

fn millis(wait: Duration) -> u32 {
    wait.as_millis().min(u32::MAX as u128 - 1) as u32
}

/// A mutex this thread owns, let go and closed as it's dropped: on the thread that took it, which
/// alone can let it go (so not Send)
struct Owned {
    handle: Handle,
    _thread: PhantomData<*const ()>,
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle);
            CloseHandle(self.handle);
        }
    }
}

/// The mutex `name`, made or opened (not inheritable), and owned by this thread once nobody else
/// does, waiting up to `wait` for that. `Ok(None)`: another thread still owns it after `wait`. A
/// mutex whose owner ended without letting go (a process gone mid-start) is this thread's.
fn own(name: &str, wait: Duration) -> anyhow::Result<Option<Owned>> {
    let wname = wide(name);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wname.as_ptr()) };
    if handle.is_null() {
        bail!(
            "{name} couldn't be made: {}",
            std::io::Error::last_os_error()
        );
    }
    match unsafe { WaitForSingleObject(handle, millis(wait)) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(Owned {
            handle,
            _thread: PhantomData,
        })),
        WAIT_TIMEOUT => {
            unsafe { CloseHandle(handle) };
            Ok(None)
        }
        other => {
            let err = std::io::Error::last_os_error();
            unsafe { CloseHandle(handle) };
            bail!("waiting for {name} ended with {other:#x}: {err}");
        }
    }
}

/// The server's hold on its socket, from `claim`. Dropped, it lets go; a server keeps it for as
/// long as it runs (`hold`), and its process ending lets go of it.
pub struct Serving {
    _owned: Owned,
    _pid: Option<Said>,
}

/// The section a server says its pid in, closed as it's dropped
struct Said(Handle);

impl Drop for Said {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// This process's id said in the section `name` (`server_pid` reads it), held while it's kept
fn say_pid(name: &str) -> anyhow::Result<Said> {
    let wname = wide(name);
    let section = unsafe {
        CreateFileMappingW(
            -1isize as Handle,
            std::ptr::null(),
            PAGE_READWRITE,
            0,
            4,
            wname.as_ptr(),
        )
    };
    if section.is_null() {
        bail!(
            "{name} couldn't be made: {}",
            std::io::Error::last_os_error()
        );
    }
    let said = Said(section);
    let view = unsafe { MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, 4) };
    if view.is_null() {
        bail!(
            "{name} couldn't be written: {}",
            std::io::Error::last_os_error()
        );
    }
    unsafe {
        (view as *mut u32).write_unaligned(std::process::id());
        UnmapViewOfFile(view);
    }
    Ok(said)
}

/// The process id of the server holding the socket at `path`, as it said (`claim`): `None`
/// while no server holds it, or one of a build from before it said one
pub fn server_pid(path: &Path) -> Option<u32> {
    let wname = wide(&mux_server_pid(path));
    let section = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wname.as_ptr()) };
    if section.is_null() {
        return None;
    }
    let said = Said(section);
    let view = unsafe { MapViewOfFile(said.0, FILE_MAP_READ, 0, 0, 4) };
    if view.is_null() {
        return None;
    }
    let pid = unsafe {
        let pid = (view as *const u32).read_unaligned();
        UnmapViewOfFile(view);
        pid
    };
    (pid != 0).then_some(pid)
}

impl Serving {
    /// Kept until the process ends, by the thread that claimed it: the server's main thread,
    /// which runs as long as the server does. A thread that ended would leave it abandoned, and
    /// a second server would take it
    pub fn hold(self) {
        std::mem::forget(self);
    }
}

/// The server's claim on the socket at `path`, made just before it binds it: refused while
/// another server holds it, starting or running, so one socket never has two servers. A client
/// looking (`served`) never owns it, and a server that ended without letting go (killed) left it
/// to whoever comes next.
pub fn claim(path: &Path) -> anyhow::Result<Serving> {
    let name = mux_socket_lock(MuxSocketLock::Serving, path);
    match own(&name, Duration::ZERO)? {
        // Its pid is only said: a server that can't say it serves all the same, and a client it
        // never answers says it doesn't know which process holds the socket
        Some(owned) => Ok(Serving {
            _owned: owned,
            _pid: say_pid(&mux_server_pid(path))
                .map_err(|err| {
                    log::error!("{}: this server's pid can't be said: {err:#}", path.display())
                })
                .ok(),
        }),
        None => bail!(
            "another mux server is starting or running at {} (it holds {name}{}): not taking its \
             socket",
            path.display(),
            server_pid(path).map_or(String::new(), |pid| format!(", pid {pid}"))
        ),
    }
}

/// Whether a server holds the socket at `path`: one is starting there (it claims the socket just
/// before it binds it) or running, whether or not the socket answers now. Looked at without
/// taking it, so a server claiming it meanwhile isn't turned away.
pub fn served(path: &Path) -> anyhow::Result<bool> {
    let name = mux_socket_lock(MuxSocketLock::Serving, path);
    let wname = wide(&name);
    let handle = unsafe { OpenMutexW(SYNCHRONIZE, 0, wname.as_ptr()) };
    if !handle.is_null() {
        unsafe { CloseHandle(handle) };
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(ERROR_FILE_NOT_FOUND) => Ok(false),
        // There, but not this process's to open
        Some(ERROR_ACCESS_DENIED) => Ok(true),
        _ => bail!("looking for {name}: {err}"),
    }
}

/// A client's hold on starting the socket's server, from `Starting::take`, let go as it's
/// dropped: once the client has connected, or given up
pub struct Starting {
    _owned: Owned,
    /// How long it waited for another client that was starting the server
    pub waited: Duration,
}

impl Starting {
    /// The `starting` lock of the socket at `path`: at once, or once the client starting its
    /// server now has connected or given up, `waiting` called first to say so. After `wait`, an
    /// error naming the socket: a client stuck that long has something wrong with it.
    pub fn take(path: &Path, wait: Duration, waiting: impl FnOnce()) -> anyhow::Result<Self> {
        let name = mux_socket_lock(MuxSocketLock::Starting, path);
        if let Some(owned) = own(&name, Duration::ZERO)? {
            return Ok(Self {
                _owned: owned,
                waited: Duration::ZERO,
            });
        }
        waiting();
        let began = Instant::now();
        match own(&name, wait)? {
            Some(owned) => Ok(Self {
                _owned: owned,
                waited: began.elapsed(),
            }),
            None => bail!(
                "another LBTerm has been starting the mux server at {} for {wait:?} (it holds {name})",
                path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;

    /// A socket path of the test's own: only its name matters here, nothing is made there
    fn socket(test: &str) -> PathBuf {
        PathBuf::from(format!(
            r"C:\lbterm-serve-tests\{test}-{}\sock",
            std::process::id()
        ))
    }

    /// `f` run on a thread of its own, its answer back
    fn on_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(f).join().unwrap()
    }

    #[test]
    fn one_server_a_socket() {
        let path = socket("one-server");
        assert!(!served(&path).unwrap(), "nobody holds it yet");
        let first = claim(&path).unwrap();
        assert!(served(&path).unwrap());
        // A second server, its own thread as its own process's would be
        let p = path.clone();
        let second = on_thread(move || claim(&p).map(|_| ()));
        let err = format!("{:#}", second.expect_err("a second server is refused"));
        assert!(
            err.contains("another mux server is starting or running"),
            "{}",
            err
        );
        assert!(err.contains("not taking its socket"), "{}", err);
        // A client looking takes nothing: the first still holds it
        assert!(served(&path).unwrap());
        let p = path.clone();
        assert!(on_thread(move || claim(&p).is_err()), "still refused");
        // It says its pid, this process's here
        assert_eq!(server_pid(&path), Some(std::process::id()));
        drop(first);
        assert!(!served(&path).unwrap(), "let go, it's gone");
        assert_eq!(server_pid(&path), None, "its pid goes with it");
        let p = path.clone();
        assert!(
            on_thread(move || claim(&p).is_ok()),
            "the next server takes it"
        );
        assert!(!served(&path).unwrap());
    }

    #[test]
    fn a_server_that_ended_without_letting_go_leaves_the_socket_to_the_next() {
        let path = socket("abandoned");
        let p = path.clone();
        // Its thread ends holding it, as a killed server's process would; the handle stays open
        // here (a killed process's would close, and the name go with it)
        on_thread(move || claim(&p).unwrap().hold());
        let p = path.clone();
        assert!(
            on_thread(move || claim(&p).is_ok()),
            "abandoned, so the next takes it"
        );
    }

    #[test]
    fn two_clients_starting_at_once_take_turns() {
        let path = socket("starting");
        let first =
            Starting::take(&path, Duration::from_secs(5), || panic!("nobody else")).unwrap();
        assert_eq!(first.waited, Duration::ZERO);
        let (said, heard) = mpsc::channel();
        let p = path.clone();
        let second = std::thread::spawn(move || {
            Starting::take(&p, Duration::from_secs(5), move || said.send(()).unwrap())
                .map(|s| s.waited)
        });
        heard
            .recv_timeout(Duration::from_secs(5))
            .expect("the second says it waits");
        std::thread::sleep(Duration::from_millis(200));
        drop(first);
        let waited = second.join().unwrap().unwrap();
        assert!(
            waited >= Duration::from_millis(150) && waited < Duration::from_secs(2),
            "waited {:?} for the first to let go 200 ms on",
            waited
        );
    }

    #[test]
    fn a_client_stuck_starting_is_waited_for_no_longer_than_its_wait() {
        let path = socket("stuck");
        let _first = Starting::take(&path, Duration::ZERO, || {}).unwrap();
        let p = path.clone();
        let began = Instant::now();
        let err =
            on_thread(move || Starting::take(&p, Duration::from_millis(100), || {}).map(|_| ()))
                .expect_err("still held after its wait");
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(
            format!("{err:#}").contains("another LBTerm has been starting the mux server"),
            "{:#}",
            err
        );
    }
}
