//! LBTerm: a recording of what a pane's program writes, as the pane's reader thread reads it from
//! the pty, for measuring how a program writes its frames (lbterm/FLOWS.md, the pane-output flow,
//! rows 1 to 4; `lbterm/perf/claude-output.ps1` records one, and `mux/tests/lbterm_pty_frames.rs`
//! reads it back through the terminal model).
//!
//! Off unless `LBTERM_PTY_RECORD` names a folder in the process's environment (a test window's,
//! which the mux server it starts inherits). Then each pane's reader writes every read it makes,
//! with the time the read returned and its bytes, to `pane-<pid>-<pane id>.ptyrec` in that folder,
//! and the pane's size when its reader starts and at every resize. Off, a pane's reader holds
//! `None` and checks it once a read, and a resize reads one `OnceLock`: nothing else runs. On,
//! each read costs the reader a WriteFile of its bytes before it hands them to the parser, a few
//! µs on top of its 30 to 45 (FLOWS.md row 4), so console writes that come closer together than
//! that are a little more likely to be read together than without it.
//!
//! The file, little-endian: `LBPTYRC1`, the recording's start in nanoseconds since the Unix epoch
//! (u64, the system's precise clock, so a script's marks in the same clock can be laid on it), the
//! process id (u32) and the pane id (u64); then records, each a kind byte, nanoseconds since the
//! start by the monotonic clock (u64), and for a read (1) its length (u32) and bytes, for a size
//! (2) its rows and columns (u32 each). Each record goes to the file in one write, unbuffered, so
//! a server ended mid-run leaves every record but perhaps its last; [`load`] stops at a record
//! cut short.

use crate::pane::PaneId;
use anyhow::{bail, Context};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::convert::TryInto;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The environment variable that turns recording on: the folder the recordings go in
pub const ENV: &str = "LBTERM_PTY_RECORD";

const MAGIC: &[u8; 8] = b"LBPTYRC1";
const READ: u8 = 1;
const SIZE: u8 = 2;

/// The folder recordings go in, read from the environment once a process
fn folder() -> Option<&'static Path> {
    static FOLDER: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOLDER
        .get_or_init(|| folder_from(std::env::var_os(ENV)))
        .as_deref()
}

/// `LBTERM_PTY_RECORD`'s value as a folder: unset or empty is off
fn folder_from(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The recorders open, by pane, so a resize on the main thread finds its pane's. Only touched
/// while recording.
fn open() -> &'static Mutex<HashMap<PaneId, Arc<Recorder>>> {
    static OPEN: OnceLock<Mutex<HashMap<PaneId, Arc<Recorder>>>> = OnceLock::new();
    OPEN.get_or_init(Default::default)
}

/// A recording's file name in its folder
pub fn file_name(pid: u32, pane_id: PaneId) -> String {
    format!("pane-{pid}-{pane_id}.ptyrec")
}

/// One pane's recording, written by its reader thread (its reads) and the main thread (resizes)
pub struct Recorder {
    start: Instant,
    pane_id: PaneId,
    out: Mutex<Out>,
}

struct Out {
    file: File,
    /// A record put together before its one write, kept for the next
    scratch: Vec<u8>,
    /// A write failed: said once, and nothing more is written
    failed: bool,
}

impl Recorder {
    /// The pane's recorder when `LBTERM_PTY_RECORD` is set, its size asked of `size` (rows,
    /// columns) only then; else `None`, and nothing else runs
    pub fn start(pane_id: PaneId, size: impl FnOnce() -> (usize, usize)) -> Option<Arc<Recorder>> {
        let folder = folder()?;
        let (rows, cols) = size();
        match Recorder::create(folder, pane_id, rows, cols) {
            Ok(r) => {
                let r = Arc::new(r);
                open().lock().insert(pane_id, Arc::clone(&r));
                Some(r)
            }
            Err(err) => {
                log::error!("{}: pane {}: {:#}", ENV, pane_id, err);
                None
            }
        }
    }

    /// A recording of pane `pane_id` in `folder`, its size first
    pub fn create(folder: &Path, pane_id: PaneId, rows: usize, cols: usize) -> anyhow::Result<Recorder> {
        std::fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
        let path = folder.join(file_name(std::process::id(), pane_id));
        let mut file = File::create(&path).with_context(|| format!("making {}", path.display()))?;
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("the clock is before 1970")?
            .as_nanos() as u64;
        let start = Instant::now();
        let mut head = Vec::with_capacity(28);
        head.extend_from_slice(MAGIC);
        head.extend_from_slice(&unix.to_le_bytes());
        head.extend_from_slice(&std::process::id().to_le_bytes());
        head.extend_from_slice(&(pane_id as u64).to_le_bytes());
        file.write_all(&head)
            .with_context(|| format!("writing {}", path.display()))?;
        let r = Recorder {
            start,
            pane_id,
            out: Mutex::new(Out {
                file,
                scratch: Vec::with_capacity(4096),
                failed: false,
            }),
        };
        r.size(rows, cols);
        Ok(r)
    }

    /// A read the reader just made: its time is now
    pub fn read(&self, bytes: &[u8]) {
        let at = self.start.elapsed().as_nanos() as u64;
        self.write(READ, at, |b| {
            b.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            b.extend_from_slice(bytes);
        });
    }

    /// The pane's size now
    pub fn size(&self, rows: usize, cols: usize) {
        let at = self.start.elapsed().as_nanos() as u64;
        self.write(SIZE, at, |b| {
            b.extend_from_slice(&(rows as u32).to_le_bytes());
            b.extend_from_slice(&(cols as u32).to_le_bytes());
        });
    }

    fn write(&self, kind: u8, at: u64, body: impl FnOnce(&mut Vec<u8>)) {
        let mut out = self.out.lock();
        if out.failed {
            return;
        }
        let Out { file, scratch, .. } = &mut *out;
        scratch.clear();
        scratch.push(kind);
        scratch.extend_from_slice(&at.to_le_bytes());
        body(scratch);
        if let Err(err) = file.write_all(scratch) {
            out.failed = true;
            log::error!("{}: pane {}: writing a record: {:#}", ENV, self.pane_id, err);
        }
    }
}

/// Pane `pane_id` was resized: its recording says so, when it's recorded
pub fn resized(pane_id: PaneId, rows: usize, cols: usize) {
    if folder().is_none() {
        return;
    }
    let r = open().lock().get(&pane_id).cloned();
    if let Some(r) = r {
        r.size(rows, cols);
    }
}

/// Pane `pane_id`'s reader has finished: its recording closes once nothing holds it
pub fn finished(pane_id: PaneId) {
    if folder().is_none() {
        return;
    }
    open().lock().remove(&pane_id);
}

/// A recording read back
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    /// When it started, in nanoseconds since the Unix epoch
    pub start_unix_ns: u64,
    pub pid: u32,
    pub pane_id: u64,
    pub events: Vec<Event>,
    /// The file ended inside a record (the process was ended while writing it)
    pub cut_short: bool,
}

/// A record, its time in nanoseconds since the recording's start
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Read { at_ns: u64, bytes: Vec<u8> },
    Size { at_ns: u64, rows: u32, cols: u32 },
}

impl Event {
    pub fn at_ns(&self) -> u64 {
        match self {
            Event::Read { at_ns, .. } | Event::Size { at_ns, .. } => *at_ns,
        }
    }
}

/// Reads a recording back
pub fn load(path: &Path) -> anyhow::Result<Recording> {
    let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&data).with_context(|| format!("reading {}", path.display()))
}

/// A recording's bytes read back
pub fn parse(data: &[u8]) -> anyhow::Result<Recording> {
    if data.len() < 28 || &data[..8] != MAGIC {
        bail!("not a pty recording (no {} header)", String::from_utf8_lossy(MAGIC));
    }
    let u32_at = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    let u64_at = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
    let mut rec = Recording {
        start_unix_ns: u64_at(8),
        pid: u32_at(16),
        pane_id: u64_at(20),
        events: vec![],
        cut_short: false,
    };
    let mut at = 28;
    while at < data.len() {
        if at + 9 > data.len() {
            rec.cut_short = true;
            break;
        }
        let kind = data[at];
        let time = u64_at(at + 1);
        let body = at + 9;
        match kind {
            READ => {
                if body + 4 > data.len() {
                    rec.cut_short = true;
                    break;
                }
                let len = u32_at(body) as usize;
                if body + 4 + len > data.len() {
                    rec.cut_short = true;
                    break;
                }
                rec.events.push(Event::Read {
                    at_ns: time,
                    bytes: data[body + 4..body + 4 + len].to_vec(),
                });
                at = body + 4 + len;
            }
            SIZE => {
                if body + 8 > data.len() {
                    rec.cut_short = true;
                    break;
                }
                rec.events.push(Event::Size {
                    at_ns: time,
                    rows: u32_at(body),
                    cols: u32_at(body + 4),
                });
                at = body + 8;
            }
            other => bail!("a record of kind {} at byte {}", other, at),
        }
    }
    Ok(rec)
}

#[cfg(test)]
mod test {
    use super::*;

    fn scratch_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lbterm-ptyrec-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn unset_or_empty_records_nothing() {
        assert_eq!(folder_from(None), None);
        assert_eq!(folder_from(Some(OsString::new())), None);
        assert_eq!(
            folder_from(Some(OsString::from("C:/x/rec"))),
            Some(PathBuf::from("C:/x/rec"))
        );
    }

    #[test]
    fn a_recording_reads_back_as_it_was_written() {
        let dir = scratch_folder("back");
        let r = Recorder::create(&dir, 7, 40, 120).unwrap();
        r.read(b"\x1b[?2026hhello");
        r.read(b"");
        r.size(92, 110);
        let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        r.read(&big);
        drop(r);
        let path = dir.join(file_name(std::process::id(), 7));
        let rec = load(&path).unwrap();
        assert_eq!(rec.pid, std::process::id());
        assert_eq!(rec.pane_id, 7);
        assert!(!rec.cut_short);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        assert!(rec.start_unix_ns <= now && now - rec.start_unix_ns < 60_000_000_000);
        assert_eq!(rec.events.len(), 5);
        assert!(matches!(rec.events[0], Event::Size { rows: 40, cols: 120, .. }));
        assert!(matches!(&rec.events[1], Event::Read { bytes, .. } if bytes == b"\x1b[?2026hhello"));
        assert!(matches!(&rec.events[2], Event::Read { bytes, .. } if bytes.is_empty()));
        assert!(matches!(rec.events[3], Event::Size { rows: 92, cols: 110, .. }));
        assert!(matches!(&rec.events[4], Event::Read { bytes, .. } if *bytes == big));
        // In the order written, by a clock that never goes back
        let times: Vec<u64> = rec.events.iter().map(Event::at_ns).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]), "{:?}", times);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_cut_short_ends_the_recording_there() {
        let dir = scratch_folder("cut");
        let r = Recorder::create(&dir, 3, 30, 80).unwrap();
        r.read(b"first");
        r.read(b"second read");
        drop(r);
        let path = dir.join(file_name(std::process::id(), 3));
        let data = std::fs::read(&path).unwrap();
        // The last read lost its last 4 bytes, as when the server is ended mid-write
        let rec = parse(&data[..data.len() - 4]).unwrap();
        assert!(rec.cut_short);
        assert_eq!(rec.events.len(), 2);
        assert!(matches!(&rec.events[1], Event::Read { bytes, .. } if bytes == b"first"));
        assert!(parse(b"not a recording at all, no").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_resize_reaches_the_panes_recording_through_the_open_list() {
        // As `start` and `resized` do it, without the environment, which the tests share
        let dir = scratch_folder("resize");
        let r = Arc::new(Recorder::create(&dir, 11, 30, 80).unwrap());
        open().lock().insert(11, Arc::clone(&r));
        open().lock().get(&11).cloned().unwrap().size(50, 100);
        open().lock().remove(&11);
        drop(r);
        let rec = load(&dir.join(file_name(std::process::id(), 11))).unwrap();
        assert!(matches!(rec.events[1], Event::Size { rows: 50, cols: 100, .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
