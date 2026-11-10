//! LBTerm's connection to the seat (Conductor's SEAT.md, "The pipe"): the subscription it keeps
//! open, and the commands it sends, on that one connection, by one thread
//! (wezterm-gui/src/seat_client.rs).
//!
//! The pipe is opened for overlapped I/O, so a command can be written while a read waits for the
//! seat's next line, and every wait has an end: [`Link::wait`] returns with the lines that came,
//! when [`Event`] is set (a command to send, the config reloaded), at the deadline the commands
//! waiting for their replies have ([`Waiting`]), or when the seat hangs up. Until 2026-10-30 each
//! command had a connection of its own and two threads, one blocked reading its reply and one
//! watching the time; when the time ran out nothing ended the read, so a seat that kept the
//! connection without answering kept the thread and its handle for as long as it ran.

use std::io;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_BUSY,
    GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, OPEN_EXISTING};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject, INFINITE};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// A pipe whose every instance is taken (the seat between instances) is waited for this many
/// times, this long each, by Windows (WaitNamedPipeW, which returns as one comes free)
const BUSY_TRIES: u32 = 5;
const BUSY_WAIT_MS: u32 = 200;
/// What one read takes at most; a line longer than that comes in several
const READ: usize = 64 * 1024;
/// The longest line kept while its end hasn't come: past it the link is broken off
const LONGEST_LINE: usize = 64 * 1024 * 1024;

fn last_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// Milliseconds from now until `until`, rounded up so a wait never ends before it, or for ever
fn millis_until(until: Option<Instant>) -> u32 {
    match until {
        None => INFINITE,
        Some(at) => {
            let left = at.saturating_duration_since(Instant::now());
            let ms = left.as_micros().div_ceil(1000);
            ms.min((INFINITE - 1) as u128) as u32
        }
    }
}

/// A Windows event that wakes a thread waiting on a [`Link`], or waiting for the time to try the
/// seat again: set from any thread, taken by the one wait it ends (auto-reset)
pub struct Event(HANDLE);

// An event handle may be set and waited on from any thread
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    pub fn new() -> io::Result<Event> {
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(last_error());
        }
        Ok(Event(handle))
    }

    pub fn set(&self) {
        unsafe { SetEvent(self.0) };
    }

    /// Asleep until it's set (true) or `until` (false; never, without one)
    pub fn wait(&self, until: Option<Instant>) -> bool {
        loop {
            match unsafe { WaitForSingleObject(self.0, millis_until(until)) } {
                WAIT_OBJECT_0 => return true,
                // Ended before `until` all the same, as [`Link::wait`]'s can: it waits the rest
                WAIT_TIMEOUT if until.is_some_and(|at| Instant::now() < at) => continue,
                _ => return false,
            }
        }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// What ended a [`Link::wait`]
#[derive(Debug, PartialEq, Eq)]
pub enum Woke {
    /// Whole lines from the seat, oldest first, without their `\n`
    Lines(Vec<String>),
    /// The event was set
    Event,
    /// It was `until`
    Due,
    /// The seat hung up
    Closed,
}

/// A read under way: the kernel writes into both until it's done, so they're boxed, and never
/// freed before it is
struct Read {
    ov: OVERLAPPED,
    buf: [u8; READ],
}

/// One connection to a pipe, for whole lines both ways on the thread that holds it
pub struct Link {
    pipe: HANDLE,
    read: Box<Read>,
    /// A read is under way in `read`
    reading: bool,
    /// What came after the last whole line
    rest: Vec<u8>,
    /// Signalled as a write completes
    written: HANDLE,
}

impl Link {
    /// Opens `name` (`\\.\pipe\...`), waiting a little while every instance is taken. A pipe
    /// that isn't there is `NotFound`
    pub fn open(name: &str) -> io::Result<Link> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut tries = 0;
        let pipe = loop {
            let pipe = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if pipe != INVALID_HANDLE_VALUE {
                break pipe;
            }
            let err = unsafe { GetLastError() };
            if err == ERROR_PIPE_BUSY && tries < BUSY_TRIES {
                tries += 1;
                unsafe { WaitNamedPipeW(wide.as_ptr(), BUSY_WAIT_MS) };
                continue;
            }
            return Err(io::Error::from_raw_os_error(err as i32));
        };
        // Manual-reset, as overlapped I/O wants: each ReadFile and WriteFile resets its own
        let event = || {
            let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if handle.is_null() {
                Err(last_error())
            } else {
                Ok(handle)
            }
        };
        let read_event = match event() {
            Ok(e) => e,
            Err(err) => {
                unsafe { CloseHandle(pipe) };
                return Err(err);
            }
        };
        let written = match event() {
            Ok(e) => e,
            Err(err) => {
                unsafe {
                    CloseHandle(read_event);
                    CloseHandle(pipe);
                }
                return Err(err);
            }
        };
        let mut read = Box::new(Read {
            ov: OVERLAPPED::default(),
            buf: [0; READ],
        });
        read.ov.hEvent = read_event;
        Ok(Link {
            pipe,
            read,
            reading: false,
            rest: vec![],
            written,
        })
    }

    /// Writes `line` and its `\n`, waiting at most `within` for the pipe to take it: a seat that
    /// takes nothing that long isn't reading, and the write is given up (`TimedOut`)
    pub fn write_line(&mut self, line: &str, within: Duration) -> io::Result<()> {
        let data = format!("{line}\n");
        let mut sent = 0;
        let until = Instant::now() + within;
        while sent < data.len() {
            let mut ov = OVERLAPPED::default();
            ov.hEvent = self.written;
            let part = &data.as_bytes()[sent..];
            let len = part.len().min(u32::MAX as usize) as u32;
            let ok = unsafe { WriteFile(self.pipe, part.as_ptr(), len, std::ptr::null_mut(), &mut ov) };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                if err != ERROR_IO_PENDING {
                    return Err(io::Error::from_raw_os_error(err as i32));
                }
            }
            // `ov` and `part` stay where they are until the write is over, given up or not
            let mut n = 0u32;
            if unsafe { WaitForSingleObject(self.written, millis_until(Some(until))) } == WAIT_TIMEOUT {
                unsafe {
                    CancelIoEx(self.pipe, &ov);
                    GetOverlappedResult(self.pipe, &ov, &mut n, 1);
                }
                return Err(io::Error::new(io::ErrorKind::TimedOut, "the pipe took nothing"));
            }
            if unsafe { GetOverlappedResult(self.pipe, &ov, &mut n, 0) } == 0 {
                return Err(last_error());
            }
            sent += n as usize;
        }
        Ok(())
    }

    /// Starts a read if none is under way: false when the seat has hung up
    fn start_read(&mut self) -> io::Result<bool> {
        if self.reading {
            return Ok(true);
        }
        let read = &mut *self.read;
        let ok = unsafe { ReadFile(self.pipe, read.buf.as_mut_ptr(), READ as u32, std::ptr::null_mut(), &mut read.ov) };
        if ok == 0 {
            match unsafe { GetLastError() } {
                // Under way; or done at once, which signals its event all the same
                ERROR_IO_PENDING | ERROR_MORE_DATA => {}
                ERROR_BROKEN_PIPE => return Ok(false),
                err => return Err(io::Error::from_raw_os_error(err as i32)),
            }
        }
        self.reading = true;
        Ok(true)
    }

    /// The read that ended: how much it brought, none when the seat hung up
    fn finish_read(&mut self) -> io::Result<usize> {
        self.reading = false;
        let mut n = 0u32;
        if unsafe { GetOverlappedResult(self.pipe, &self.read.ov, &mut n, 0) } == 0 {
            match unsafe { GetLastError() } {
                ERROR_MORE_DATA => {}
                ERROR_BROKEN_PIPE => return Ok(0),
                err => return Err(io::Error::from_raw_os_error(err as i32)),
            }
        }
        Ok(n as usize)
    }

    /// The whole lines in what's come so far, the rest kept for the next
    fn lines(&mut self, n: usize) -> io::Result<Vec<String>> {
        self.rest.extend_from_slice(&self.read.buf[..n]);
        let mut lines = vec![];
        let mut start = 0;
        while let Some(end) = self.rest[start..].iter().position(|&b| b == b'\n') {
            let line = &self.rest[start..start + end];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            lines.push(String::from_utf8_lossy(line).into_owned());
            start += end + 1;
        }
        self.rest.drain(..start);
        if self.rest.len() > LONGEST_LINE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a line longer than 64 MB"));
        }
        Ok(lines)
    }

    /// Asleep until the seat says something (its whole lines), `event` is set, it's `until`
    /// (never, without one), or the seat hangs up, whichever is first
    pub fn wait(&mut self, event: &Event, until: Option<Instant>) -> io::Result<Woke> {
        loop {
            if !self.start_read()? {
                return Ok(Woke::Closed);
            }
            let handles = [self.read.ov.hEvent, event.0];
            let woke = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, millis_until(until)) };
            match woke {
                w if w == WAIT_OBJECT_0 => {
                    let n = self.finish_read()?;
                    if n == 0 {
                        return Ok(Woke::Closed);
                    }
                    let lines = self.lines(n)?;
                    if !lines.is_empty() {
                        return Ok(Woke::Lines(lines));
                    }
                }
                w if w == WAIT_OBJECT_0 + 1 => return Ok(Woke::Event),
                // A wait can end a fraction of a millisecond before its timeout by
                // Instant's clock (199.78 ms of 200 in the check, 2026-10-31; 199.38 on 2026-11-01,
                // in a branch without this): not due yet, so it waits the rest
                // (lbterm/PITFALLS.md, trap 59)
                WAIT_TIMEOUT if until.is_some_and(|at| Instant::now() < at) => continue,
                WAIT_TIMEOUT => return Ok(Woke::Due),
                _ => return Err(last_error()),
            }
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        unsafe {
            if self.reading {
                // The read given up, and waited for: the kernel is done with `read` before it goes
                CancelIoEx(self.pipe, &self.read.ov);
                let mut n = 0u32;
                GetOverlappedResult(self.pipe, &self.read.ov, &mut n, 1);
            }
            CloseHandle(self.pipe);
            CloseHandle(self.read.ov.hEvent);
            CloseHandle(self.written);
        }
    }
}

/// The commands sent on a link that wait for their replies, each until its deadline
#[derive(Debug, Default)]
pub struct Waiting {
    list: Vec<(String, Instant, Duration)>,
}

impl Waiting {
    /// `id` sent at `now`, waiting at most `wait`
    pub fn add(&mut self, id: String, wait: Duration, now: Instant) {
        self.list.push((id, now + wait, wait));
    }

    /// `id`'s reply came: true when it was still waiting (a reply after its wait is over is
    /// nobody's)
    pub fn answered(&mut self, id: &str) -> bool {
        let before = self.list.len();
        self.list.retain(|(i, _, _)| i != id);
        self.list.len() != before
    }

    /// Those whose wait is over at `now`, each with how long it was, oldest first
    pub fn due(&mut self, now: Instant) -> Vec<(String, Duration)> {
        let (due, left): (Vec<_>, Vec<_>) = self.list.drain(..).partition(|(_, at, _)| *at <= now);
        self.list = left;
        due.into_iter().map(|(id, _, wait)| (id, wait)).collect()
    }

    /// When the next wait is over
    pub fn next(&self) -> Option<Instant> {
        self.list.iter().map(|(_, at, _)| *at).min()
    }

    /// Every one still waiting, which won't be answered now (the seat hung up)
    pub fn take_all(&mut self) -> Vec<String> {
        self.list.drain(..).map(|(id, _, _)| id).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::channel;
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A pipe of the test's own, its name
    fn name() -> String {
        format!(r"\\.\pipe\lbterm-test-link-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// A seat played on `name`'s one instance, by `play` on a thread of its own once a client
    /// connects, with the server's end as a File; what it returns comes back on the channel
    fn serve<T: Send + 'static>(
        name: &str,
        play: impl FnOnce(std::fs::File) -> T + Send + 'static,
    ) -> std::sync::mpsc::Receiver<T> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let pipe = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                65536,
                65536,
                0,
                std::ptr::null(),
            )
        };
        assert_ne!(pipe, INVALID_HANDLE_VALUE, "the test's pipe: {}", last_error());
        let pipe = pipe as usize;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            use std::os::windows::io::FromRawHandle;
            let pipe = pipe as HANDLE;
            unsafe { ConnectNamedPipe(pipe, std::ptr::null_mut()) };
            let file = unsafe { std::fs::File::from_raw_handle(pipe as _) };
            let _ = tx.send(play(file));
        });
        rx
    }

    fn read_line(file: &mut std::fs::File) -> Option<String> {
        use std::io::Read;
        let mut line = vec![];
        let mut b = [0u8; 1];
        loop {
            match file.read(&mut b) {
                Ok(0) | Err(_) => return None,
                Ok(_) if b[0] == b'\n' => return Some(String::from_utf8(line).unwrap()),
                Ok(_) => line.push(b[0]),
            }
        }
    }

    #[test]
    fn a_line_goes_out_and_its_reply_comes_back() {
        let name = name();
        let seat = serve(&name, |mut file| {
            use std::io::Write;
            let got = read_line(&mut file);
            // In two writes, the second with a line and the start of the next
            file.write_all(b"{\"type\":\"seat.reply\",").unwrap();
            file.flush().unwrap();
            std::thread::sleep(Duration::from_millis(30));
            file.write_all(b"\"id\":\"a\"}\nhalf").unwrap();
            file.write_all(b" a line\n").unwrap();
            got
        });
        let event = Event::new().unwrap();
        let mut link = Link::open(&name).unwrap();
        link.write_line("{\"type\":\"seat.say\",\"id\":\"a\"}", Duration::from_secs(5)).unwrap();
        let mut lines = vec![];
        while lines.len() < 2 {
            match link.wait(&event, Some(Instant::now() + Duration::from_secs(5))).unwrap() {
                Woke::Lines(more) => lines.extend(more),
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(lines, ["{\"type\":\"seat.reply\",\"id\":\"a\"}", "half a line"]);
        assert_eq!(seat.recv().unwrap().as_deref(), Some("{\"type\":\"seat.say\",\"id\":\"a\"}"));
    }

    #[test]
    fn a_seat_that_never_answers_is_given_up_at_the_deadline_on_this_thread() {
        let name = name();
        // It reads the command and keeps the connection, answering nothing, until the link is
        // dropped: then its read ends, the connection closed
        let seat = serve(&name, move |mut file| {
            let got = read_line(&mut file);
            let closed = read_line(&mut file).is_none();
            (got, closed)
        });
        let event = Event::new().unwrap();
        let mut link = Link::open(&name).unwrap();
        let mut waiting = Waiting::default();
        let sent = Instant::now();
        link.write_line("{\"type\":\"seat.say\",\"id\":\"b\"}", Duration::from_secs(5)).unwrap();
        waiting.add("b".into(), Duration::from_millis(200), sent);
        assert_eq!(link.wait(&event, waiting.next()).unwrap(), Woke::Due);
        let waited = sent.elapsed();
        assert!(waited >= Duration::from_millis(200) && waited < Duration::from_secs(2), "{waited:?}");
        assert_eq!(waiting.due(Instant::now()), [("b".to_string(), Duration::from_millis(200))]);
        assert!(waiting.is_empty());
        // Dropped with its read under way: the read is given up and the handle closed, so the
        // seat sees the connection go and nothing is left waiting on it
        drop(link);
        let (got, closed) = seat.recv_timeout(Duration::from_secs(5)).expect("the seat saw the connection go");
        assert_eq!(got.as_deref(), Some("{\"type\":\"seat.say\",\"id\":\"b\"}"));
        assert!(closed);
    }

    #[test]
    fn the_event_wakes_a_wait_and_a_hang_up_ends_it() {
        let name = name();
        let (go_tx, go_rx) = channel::<()>();
        let _seat = serve(&name, move |file| {
            let _ = go_rx.recv_timeout(Duration::from_secs(10));
            drop(file);
        });
        let event = std::sync::Arc::new(Event::new().unwrap());
        let mut link = Link::open(&name).unwrap();
        let set = std::sync::Arc::clone(&event);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            set.set();
        });
        assert_eq!(link.wait(&event, None).unwrap(), Woke::Event);
        go_tx.send(()).unwrap();
        assert_eq!(link.wait(&event, Some(Instant::now() + Duration::from_secs(5))).unwrap(), Woke::Closed);
    }

    #[test]
    fn no_seat_is_not_found() {
        let err = Link::open(&name()).err().expect("no pipe");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn an_event_waited_for_until_a_time_says_whether_it_came() {
        let event = Event::new().unwrap();
        assert!(!event.wait(Some(Instant::now() + Duration::from_millis(20))));
        event.set();
        assert!(event.wait(Some(Instant::now() + Duration::from_millis(20))));
        assert!(!event.wait(Some(Instant::now())), "taken by the wait it ended");
    }

    /// How many short waits the tests of a wait's end make, each 1.5 to 3.25 ms. Windows ends a
    /// timed wait on its own clock, which now and then is a fraction of a millisecond before the
    /// time asked by Instant's, under load (lbterm/PITFALLS.md, trap 59): these hold the code to
    /// never saying so early, but can't make it happen; on 2026-11-02 6,000 of them never ended
    /// early on the code without the check, where 3 of 150 waits of 200 ms did
    const SHORT_WAITS: usize = 40;

    fn short_wait(k: usize) -> Instant {
        Instant::now() + Duration::from_micros(1500 + 250 * (k % 8) as u64)
    }

    #[test]
    fn a_link_never_says_its_due_before_its_time() {
        let name = name();
        // A seat that keeps the connection and says nothing, until the link is dropped
        let _seat = serve(&name, |mut file| read_line(&mut file));
        let event = Event::new().unwrap();
        let mut link = Link::open(&name).unwrap();
        let mut early = vec![];
        for k in 0..SHORT_WAITS {
            let until = short_wait(k);
            assert_eq!(link.wait(&event, Some(until)).unwrap(), Woke::Due);
            let now = Instant::now();
            if now < until {
                early.push(until - now);
            }
        }
        assert!(early.is_empty(), "{} of {SHORT_WAITS} waits said due early, by {early:?}", early.len());
    }

    #[test]
    fn an_event_never_says_it_didnt_come_before_its_time() {
        let event = Event::new().unwrap();
        let mut early = vec![];
        for k in 0..SHORT_WAITS {
            let until = short_wait(k);
            assert!(!event.wait(Some(until)));
            let now = Instant::now();
            if now < until {
                early.push(until - now);
            }
        }
        assert!(early.is_empty(), "{} of {SHORT_WAITS} waits ended early, by {early:?}", early.len());
    }

    #[test]
    fn commands_wait_for_their_replies_until_their_deadlines() {
        let t0 = Instant::now();
        let mut w = Waiting::default();
        assert_eq!(w.next(), None);
        w.add("a".into(), Duration::from_secs(15), t0);
        w.add("b".into(), Duration::from_secs(120), t0);
        w.add("c".into(), Duration::from_secs(30), t0);
        assert_eq!(w.next(), Some(t0 + Duration::from_secs(15)));
        assert!(w.answered("c"));
        assert!(!w.answered("c"), "answered once");
        assert!(!w.answered("z"), "never sent");
        assert_eq!(w.due(t0 + Duration::from_secs(14)), vec![]);
        assert_eq!(w.due(t0 + Duration::from_secs(15)), [("a".to_string(), Duration::from_secs(15))]);
        assert_eq!(w.next(), Some(t0 + Duration::from_secs(120)));
        assert_eq!(w.take_all(), ["b"]);
        assert!(w.is_empty());
    }
}
