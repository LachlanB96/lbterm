//! LBTerm: the images `lbterm://image` links name, decoded small for their thumbnails
//! (render/thumbnails.rs; lbterm/THUMBNAILS.md). A thread of its own decodes each the first time a
//! window asks, at the size it's drawn at (lbterm_extras::thumbnails::decode, with its limits),
//! and watches every file it holds through its folder (lbterm_extras::watch): one that goes is
//! drawn as gone and one that changes is decoded again, as soon as Windows says so. When one is
//! ready or changes, every window repaints. Nothing is read on the GUI thread. One no window has
//! asked for in [`KEEP`] is let go and its folder no longer watched, and with none held the thread
//! sleeps until a window asks. Until 2026-10-30 it looked at every file it held every 2 s, and woke
//! that often for good once one had been shown, so a file deleted showed as gone up to 2 s late.
//!
//! Each decoding has a version of its own, so a window that drew an earlier one of the same file
//! in the same box draws the new one into the same place in its atlas (render/thumbnails.rs): a
//! file rewritten every few seconds (a dashboard's live tile) never fills the atlas.

use ::window::bitmaps::Image;
use lbterm_extras::thumbnails::{decode, stamp};
use lbterm_extras::watch::{Seen, Target, Watch};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant, SystemTime};

/// How long a thumbnail no window draws is kept
const KEEP: Duration = Duration::from_secs(300);
/// A file written is several changes in a row: they're taken together, this long after the first
const SETTLE: Duration = Duration::from_millis(25);
/// How many times the thread follows the files' folders before it looks at them: a folder made
/// can have the next one under it already
const FOLLOWS: usize = 8;

/// What a thumbnail is now
#[derive(Clone)]
pub enum Thumb {
    /// Not decoded yet
    Loading,
    /// Decoded, at the size it's drawn at: its pixels as the atlas takes them (RGBA), and which
    /// decoding it is, a number no other has
    Shown { image: Arc<Image>, width: u32, height: u32, version: u64 },
    /// Its file isn't there
    Gone,
    /// Its file isn't an image this can read, or is over the limits
    Unreadable,
}

/// A file's thumbnail for a box of a size, in pixels
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    path: String,
    width: u32,
    height: u32,
}

struct Entry {
    thumb: Thumb,
    /// The file's time and size when it was last read, `None` while it isn't there
    stamp: Option<(SystemTime, u64)>,
    wanted: Instant,
    due: bool,
}

/// What wakes the thread
enum Wake {
    /// A window asked for one it hasn't had
    Due,
    Seen(Seen),
}

static CACHE: Mutex<Option<HashMap<Key, Entry>>> = Mutex::new(None);
static WAKE: Mutex<Option<Sender<Wake>>> = Mutex::new(None);
/// How many times a file has gone or come back: a window that drew its captions at another count
/// draws them again
static GONE: AtomicUsize = AtomicUsize::new(0);
/// The last decoding's version
static VERSION: AtomicU64 = AtomicU64::new(0);
static START: Once = Once::new();

/// The thumbnail of the file at `path` for a box of `width` × `height` pixels, asked for now:
/// [`Thumb::Loading`] the first time, and the thread told to decode it
pub fn get(path: &str, width: u32, height: u32) -> Thumb {
    START.call_once(start);
    let key = Key { path: path.to_string(), width, height };
    let mut cache = CACHE.lock().unwrap();
    let entry = cache.get_or_insert_with(HashMap::new).entry(key).or_insert_with(|| {
        if let Some(wake) = WAKE.lock().unwrap().as_ref() {
            let _ = wake.send(Wake::Due);
        }
        Entry { thumb: Thumb::Loading, stamp: None, wanted: Instant::now(), due: true }
    });
    entry.wanted = Instant::now();
    entry.thumb.clone()
}

/// Whether the file at `path` is known to be gone: a thumbnail of it found it missing
pub fn gone(path: &str) -> bool {
    CACHE
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|cache| cache.iter().any(|(k, e)| k.path == path && matches!(e.thumb, Thumb::Gone)))
}

/// How many times a file has gone or come back
pub fn gone_changes() -> usize {
    GONE.load(Ordering::Acquire)
}

fn load(key: &Key) -> (Thumb, Option<(SystemTime, u64)>) {
    let path = Path::new(&key.path);
    let Some(stamp) = stamp(path) else {
        return (Thumb::Gone, None);
    };
    let thumb = match decode(path, key.width, key.height) {
        Ok(d) => Thumb::Shown {
            width: d.width,
            height: d.height,
            image: Arc::new(Image::from_raw(d.width as usize, d.height as usize, d.rgba)),
            version: VERSION.fetch_add(1, Ordering::AcqRel) + 1,
        },
        Err(err) => {
            log::warn!("thumbnail of {}: {err:#}", key.path);
            Thumb::Unreadable
        }
    };
    (thumb, Some(stamp))
}

/// Marks due every thumbnail of a file `seen` says may have changed: all of them when the watch
/// went wrong
fn take(seen: Seen, watch: &Watch) {
    match seen {
        Seen::Changed(paths) if watch.matters(&paths) => recheck(Some(paths.as_slice())),
        Seen::Changed(_) => {}
        Seen::Failed(err) => {
            log::error!("lbterm-thumbnails: the images' watch: {err}; looking at every one again");
            recheck(None);
        }
    }
}

/// Marks due every thumbnail of a file a change at `paths` can be about (all of them for `None`)
/// whose time or size isn't what it was read at
fn recheck(paths: Option<&[PathBuf]>) {
    let held: Vec<(Key, Option<(SystemTime, u64)>)> = {
        let cache = CACHE.lock().unwrap();
        cache
            .iter()
            .flatten()
            .filter(|(k, _)| {
                paths.map_or(true, |paths| {
                    let file = Target::File(PathBuf::from(&k.path));
                    paths.iter().any(|p| file.matters(p))
                })
            })
            .map(|(k, e)| (k.clone(), e.stamp))
            .collect()
    };
    for (key, was) in held {
        if stamp(Path::new(&key.path)) != was {
            if let Some(entry) = CACHE.lock().unwrap().as_mut().and_then(|c| c.get_mut(&key)) {
                entry.due = true;
            }
        }
    }
}

fn start() {
    let (tx, rx) = channel();
    *WAKE.lock().unwrap() = Some(tx.clone());
    let spawned = std::thread::Builder::new()
        .name("lbterm-thumbnails".into())
        .spawn(move || run(tx, rx));
    if let Err(err) = spawned {
        log::error!("starting the lbterm-thumbnails thread: {err:#}");
    }
}

fn run(tx: Sender<Wake>, rx: Receiver<Wake>) {
    let mut watch = match Watch::new(move |s| {
        let _ = tx.send(Wake::Seen(s));
    }) {
        Ok(watch) => watch,
        Err(err) => {
            log::error!("lbterm-thumbnails: can't watch the images, so a thumbnail won't follow its file: {err:#}");
            return;
        }
    };
    let mut wakes: Vec<Wake> = vec![];
    loop {
        for wake in wakes.drain(..) {
            if let Wake::Seen(seen) = wake {
                take(seen, &watch);
            }
        }
        // Let go of what no window has asked for in a while, and watch what's held: a folder
        // newly watched may hold its file already, so each is looked at again then
        let targets: Vec<Target> = {
            let mut cache = CACHE.lock().unwrap();
            let cache = cache.get_or_insert_with(HashMap::new);
            cache.retain(|_, e| e.wanted.elapsed() < KEEP);
            let mut paths: Vec<&str> = cache.keys().map(|k| k.path.as_str()).collect();
            paths.sort_unstable();
            paths.dedup();
            paths.into_iter().map(|p| Target::File(PathBuf::from(p))).collect()
        };
        let mut moved = false;
        for _ in 0..FOLLOWS {
            if !watch.follow(&targets) {
                break;
            }
            moved = true;
        }
        if moved {
            recheck(None);
        }

        let due: Vec<Key> = {
            let cache = CACHE.lock().unwrap();
            cache.iter().flatten().filter(|(_, e)| e.due).map(|(k, _)| k.clone()).collect()
        };
        let mut changed = false;
        let mut gone_changed = false;
        for key in due {
            let (thumb, stamp) = load(&key);
            let mut cache = CACHE.lock().unwrap();
            if let Some(entry) = cache.as_mut().and_then(|c| c.get_mut(&key)) {
                gone_changed |= matches!(entry.thumb, Thumb::Gone) != matches!(thumb, Thumb::Gone);
                entry.thumb = thumb;
                entry.stamp = stamp;
                entry.due = false;
                changed = true;
            }
        }
        if gone_changed {
            GONE.fetch_add(1, Ordering::AcqRel);
        }
        if changed {
            promise::spawn::spawn_into_main_thread(async { crate::state_files::repaint_windows() }).detach();
        }

        // Asleep until a window asks for one, a file may have changed, or the first held is
        // due to be let go; with none held, until a window asks
        let expires = CACHE
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .map(|(_, e)| e.wanted + KEEP)
            .min();
        let first = match expires {
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(wake) => Some(wake),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(wake) => Some(wake),
                Err(_) => return,
            },
        };
        if let Some(first) = first {
            // A change that matters, or a window asking: taken together with what follows it
            let matters = match &first {
                Wake::Due => true,
                Wake::Seen(Seen::Changed(paths)) => watch.matters(paths),
                Wake::Seen(Seen::Failed(_)) => true,
            };
            wakes.push(first);
            if !matters {
                continue;
            }
            std::thread::sleep(SETTLE);
            wakes.extend(rx.try_iter());
        }
    }
}
