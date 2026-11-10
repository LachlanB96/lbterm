//! LBTerm's counters, always on (lbterm/PERF.md): what the GUI does, counted as it does it, in
//! every LBTerm, Lachlan's daily one included, and handed out by the control pipe's `metrics`
//! (lbterm/CONTROL.md). The slow-frame log (slow.rs) says when one thing held the GUI thread
//! too long; these say how often everything ran and what it all cost, so a test can say "this
//! window painted 300 frames at rest, asked for by that line" and a budget can be held to it.
//!
//! What's counted:
//!
//! - `messages`, `tasks`, `paints`: every run of what slow.rs times (a window message by
//!   number, a task from the main thread's queue by where it was spawned, a window's paint by
//!   its parts), with how many, their total time and the longest. The time is each one's own, as
//!   slow.rs charges it: less what ran inside it.
//! - `windows` and `requests`: every frame a window paints, and every repaint asked for
//!   (`Window::invalidate`, by its caller's source line). A frame's reasons are the requests made
//!   for its window since its last frame, and each of those sites is credited with it; a frame
//!   nobody asked for (Windows' own WM_PAINT, as a window is shown, uncovered or sized) is
//!   `unasked`. Many requests make one frame, so a site's `requests` can be far above its
//!   `frames`.
//! - `timers`: timers armed through [`timer`], by where, and how many fired or were dropped
//!   before they did.
//! - `gui`: the GUI thread's wakes (each time it comes back from waiting for a message or a
//!   task) and the time it spent waiting.
//! - `mux`: the PDUs the mux client sent and received, by type, with their bytes (the
//!   serialized payload: a frame's header adds a few bytes each).
//!
//! The process's CPU, memory and handles, and upstream's histograms (wezterm-gui's stats.rs), are
//! wezterm-gui's to add (its `counters.rs`): they need Windows' API and the recorder. [`Process`]
//! and [`Histogram`] are their shape.
//!
//! Cheap enough to leave on: fixed tables of atomics, all there from the start, a site claimed
//! the first time it's seen. No event allocates or takes a lock: a site's lookup is a multiply
//! and, nearly always, one compare, and each count a relaxed atomic add. [`Counters::report`]
//! allocates, once per `metrics` request, and can run on any thread: the control pipe asks it
//! on its own, so asking adds nothing to what the GUI thread does. A report that resets takes
//! each count as it zeroes it, so nothing counted in between is lost.

use crate::slow::{message_name, source_name, Part, Site};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::panic::Location;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// How far a lookup looks past a site's own slot before it gives the site the table's overflow
const PROBES: usize = 32;

/// The windows whose frames can be told apart: one bit each in a request's pending mask
pub const WINDOWS: usize = 64;

/// The name a table's overflow is reported under
pub const OVERFLOW: &str = "(overflow)";

/// What a table holds for each site: atomics that start at zero
pub trait Count: Sync {
    const ZERO: Self;
}

/// A count as it is, or taken and zeroed in one step
#[inline]
fn take(count: &AtomicU64, reset: bool) -> u64 {
    if reset {
        count.swap(0, Relaxed)
    } else {
        count.load(Relaxed)
    }
}

/// Fixed slots of counts by site, found by a key the caller makes from the site (a pointer, a
/// number), never 0: open addressing over `N` slots (a power of two), each claimed with a
/// compare-and-swap the first time its key is seen and never given back. A key that finds no slot
/// within [`PROBES`] of its own counts in the overflow.
pub struct Table<S: 'static, V, const N: usize> {
    keys: [AtomicUsize; N],
    sites: [OnceLock<S>; N],
    vals: [V; N],
    overflow: V,
}

impl<S: Copy + Send + Sync + 'static, V: Count, const N: usize> Table<S, V, N> {
    pub const fn new() -> Self {
        assert!(N.is_power_of_two() && N >= 2);
        Self {
            keys: [const { AtomicUsize::new(0) }; N],
            sites: [const { OnceLock::new() }; N],
            vals: [const { V::ZERO }; N],
            overflow: V::ZERO,
        }
    }

    /// The slot a key starts looking in: Fibonacci hashing, so pointers' low bits (always 0)
    /// don't matter
    #[inline]
    fn home(key: usize) -> usize {
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (usize::BITS - N.trailing_zeros())) as usize
    }

    /// The counts for `site`, whose key is `key`, claiming a slot the first time
    #[inline]
    pub fn get(&self, key: usize, site: S) -> &V {
        debug_assert_ne!(key, 0, "0 is a free slot's key");
        let mut i = Self::home(key);
        for _ in 0..PROBES.min(N) {
            let k = self.keys[i].load(Relaxed);
            if k == key {
                return &self.vals[i];
            }
            if k == 0 {
                match self.keys[i].compare_exchange(0, key, Relaxed, Relaxed) {
                    Ok(_) => {
                        let _ = self.sites[i].set(site);
                        return &self.vals[i];
                    }
                    Err(k) if k == key => return &self.vals[i],
                    Err(_) => {}
                }
            }
            i = (i + 1) & (N - 1);
        }
        &self.overflow
    }

    /// The counts for `key` if it has a slot, without claiming one
    pub fn find(&self, key: usize) -> Option<&V> {
        let mut i = Self::home(key);
        for _ in 0..PROBES.min(N) {
            match self.keys[i].load(Relaxed) {
                0 => return None,
                k if k == key => return Some(&self.vals[i]),
                _ => i = (i + 1) & (N - 1),
            }
        }
        None
    }

    /// Every claimed slot's site and counts (a slot claimed this instant, its site not yet set,
    /// is left out)
    pub fn entries(&self) -> impl Iterator<Item = (S, &V)> + '_ {
        (0..N).filter_map(move |i| {
            if self.keys[i].load(Relaxed) == 0 {
                return None;
            }
            self.sites[i].get().map(|s| (*s, &self.vals[i]))
        })
    }

    /// Every claimed slot's counts, its site set or not
    fn claimed(&self) -> impl Iterator<Item = &V> + '_ {
        (0..N).filter_map(move |i| (self.keys[i].load(Relaxed) != 0).then(|| &self.vals[i]))
    }

    /// Every claimed slot's site (none for one claimed this instant) and counts
    fn claimed_with_sites(&self) -> impl Iterator<Item = (Option<S>, &V)> + '_ {
        (0..N).filter_map(move |i| {
            (self.keys[i].load(Relaxed) != 0).then(|| (self.sites[i].get().copied(), &self.vals[i]))
        })
    }

    pub fn overflow(&self) -> &V {
        &self.overflow
    }

    /// How many sites have a slot
    pub fn len(&self) -> usize {
        self.keys.iter().filter(|k| k.load(Relaxed) != 0).count()
    }
}

/// Runs of something timed: how many, their total and the longest, in nanoseconds
pub struct Timing {
    count: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl Count for Timing {
    const ZERO: Self = Self {
        count: AtomicU64::new(0),
        total_ns: AtomicU64::new(0),
        max_ns: AtomicU64::new(0),
    };
}

impl Timing {
    /// One run of `ns`
    #[inline]
    pub fn add(&self, ns: u64) {
        self.count.fetch_add(1, Relaxed);
        self.total_ns.fetch_add(ns, Relaxed);
        if ns > self.max_ns.load(Relaxed) {
            self.max_ns.fetch_max(ns, Relaxed);
        }
    }

    /// What it holds, zeroed with `reset`
    pub fn read(&self, reset: bool) -> Runs {
        Runs {
            count: take(&self.count, reset),
            total_ns: take(&self.total_ns, reset),
            max_ns: take(&self.max_ns, reset),
        }
    }
}

/// What a [`Timing`] holds, read
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Runs {
    pub count: u64,
    pub total_ns: u64,
    pub max_ns: u64,
}

impl Runs {
    fn merge(&mut self, other: Runs) {
        self.count += other.count;
        self.total_ns += other.total_ns;
        self.max_ns = self.max_ns.max(other.max_ns);
    }
}

/// A site's repaint requests: how many, how many frames they were among the reasons for, and the
/// windows it has asked of since their last frame (a bit each)
pub struct Requests {
    requests: AtomicU64,
    frames: AtomicU64,
    pending: AtomicU64,
}

impl Count for Requests {
    const ZERO: Self = Self {
        requests: AtomicU64::new(0),
        frames: AtomicU64::new(0),
        pending: AtomicU64::new(0),
    };
}

/// A window's frames, and how many of them nobody asked for
pub struct Frames {
    frames: AtomicU64,
    unasked: AtomicU64,
}

impl Count for Frames {
    const ZERO: Self = Self {
        frames: AtomicU64::new(0),
        unasked: AtomicU64::new(0),
    };
}

/// A site's timers: armed, fired, and dropped before they fired
pub struct Timers {
    armed: AtomicU64,
    fired: AtomicU64,
    dropped: AtomicU64,
}

impl Count for Timers {
    const ZERO: Self = Self {
        armed: AtomicU64::new(0),
        fired: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    };
}

/// A PDU type's traffic, each way
pub struct Traffic {
    sent: AtomicU64,
    sent_bytes: AtomicU64,
    received: AtomicU64,
    received_bytes: AtomicU64,
}

impl Count for Traffic {
    const ZERO: Self = Self {
        sent: AtomicU64::new(0),
        sent_bytes: AtomicU64::new(0),
        received: AtomicU64::new(0),
        received_bytes: AtomicU64::new(0),
    };
}

type At = &'static Location<'static>;

/// Every count, in fixed tables. The process has one, [`COUNTERS`]; tests make their own.
pub struct Counters {
    pub messages: Table<u32, Timing, 512>,
    pub tasks: Table<At, Timing, 1024>,
    pub paints: Table<(usize, Part), Timing, 256>,
    pub requests: Table<At, Requests, 512>,
    pub frames: Table<usize, Frames, 256>,
    pub timers: Table<At, Timers, 256>,
    pub pdus: Table<&'static str, Traffic, 256>,
    /// Each window's key (its handle, plus one) in the slot whose bit it's given in a request's
    /// pending mask
    windows: [AtomicUsize; WINDOWS],
    /// Every slot has been taken once: said
    out_of_bits: AtomicBool,
    wakes: AtomicU64,
    waited_ns: AtomicU64,
    /// The message loop's drains of the main thread's tasks (window/src/spawn.rs): the longest is
    /// the longest the GUI thread went between looks at its messages, short of a message's own
    /// handling (lbterm/PERF.md, finding 33)
    drains: Timing,
    /// When the counts started: the process's start, or the last reset
    since: Mutex<Option<Instant>>,
}

/// The process's counters
pub static COUNTERS: Counters = Counters::new();

fn at_key(at: At) -> usize {
    at as *const Location as usize
}

fn paint_key(window: usize, part: Part) -> usize {
    let part = match part {
        Part::Pass => 0,
        Part::Draw => 1,
        Part::Present => 2,
    };
    ((window << 2) | part) + 1
}

/// A `&'static str`'s key: where it is and how long, as two literals could share a start
fn str_key(s: &'static str) -> usize {
    (s.as_ptr() as usize) ^ (s.len() << 48) ^ 1
}

impl Counters {
    pub const fn new() -> Self {
        Self {
            messages: Table::new(),
            tasks: Table::new(),
            paints: Table::new(),
            requests: Table::new(),
            frames: Table::new(),
            timers: Table::new(),
            pdus: Table::new(),
            windows: [const { AtomicUsize::new(0) }; WINDOWS],
            out_of_bits: AtomicBool::new(false),
            wakes: AtomicU64::new(0),
            waited_ns: AtomicU64::new(0),
            drains: Timing::ZERO,
            since: Mutex::new(None),
        }
    }

    /// The counts start now, unless they already have
    pub fn start(&self) {
        let mut since = self.since.lock().unwrap_or_else(|e| e.into_inner());
        since.get_or_insert_with(Instant::now);
    }

    /// One run of what slow.rs timed at `site`, `ns` of its own
    #[inline]
    pub fn timed(&self, site: Site, ns: u64) {
        match site {
            Site::Message(msg) => self.messages.get(msg as usize + 1, msg).add(ns),
            Site::Task(at) => self.tasks.get(at_key(at), at).add(ns),
            Site::Paint { window, part } => {
                self.paints.get(paint_key(window, part), (window, part)).add(ns)
            }
        }
    }

    /// The bit of the window with handle `hwnd`, claiming one the first time. A handle the
    /// GUI's check says is gone (a request from a task that outlived its window) claims none, so
    /// it can't hold a bit for ever; and once every bit is taken, that's said once, since every
    /// window without one paints frames nobody is credited with
    fn window_bit(&self, hwnd: usize) -> Option<u64> {
        let key = hwnd.wrapping_add(1).max(1);
        if let Some(bit) = self.find_window(key) {
            return Some(bit);
        }
        if WINDOW_ALIVE.get().map_or(false, |alive| !alive(hwnd)) {
            return None;
        }
        for (i, slot) in self.windows.iter().enumerate() {
            match slot.compare_exchange(0, key, Relaxed, Relaxed) {
                Ok(_) => return Some(1 << i),
                Err(k) if k == key => return Some(1 << i),
                Err(_) => {}
            }
        }
        if !self.out_of_bits.swap(true, Relaxed) {
            log::warn!(
                target: "lbterm::counters",
                "every one of the {WINDOWS} windows' slots is taken: frames of a window past them are credited to nobody, and the budget can't name who asks for them"
            );
        }
        None
    }

    fn find_window(&self, key: usize) -> Option<u64> {
        self.windows
            .iter()
            .position(|slot| slot.load(Relaxed) == key)
            .map(|i| 1 << i)
    }

    /// `at` asked the window with handle `hwnd` for a frame
    #[inline]
    pub fn request(&self, hwnd: usize, at: At) {
        let site = self.requests.get(at_key(at), at);
        site.requests.fetch_add(1, Relaxed);
        if let Some(bit) = self.window_bit(hwnd) {
            if site.pending.load(Relaxed) & bit == 0 {
                site.pending.fetch_or(bit, Relaxed);
            }
        }
    }

    /// The window with handle `hwnd`, mux window `window`, is painting a frame: each site that
    /// asked for one since its last is credited with it, and named in what it returns (the
    /// budget's, budget.rs)
    pub fn frame(&self, hwnd: usize, window: usize) -> crate::budget::Reasons {
        let mut reasons = crate::budget::Reasons::default();
        if let Some(bit) = self.find_window(hwnd.wrapping_add(1).max(1)) {
            let owed = |site: &Requests| {
                site.pending.load(Relaxed) & bit != 0 && site.pending.fetch_and(!bit, Relaxed) & bit != 0
            };
            for (at, site) in self.requests.claimed_with_sites() {
                if owed(site) {
                    site.frames.fetch_add(1, Relaxed);
                    match at {
                        Some(at) => reasons.push(at),
                        None => reasons.unnamed(),
                    }
                }
            }
            // Past the table's slots: owed its frame all the same, by nobody it can name
            let over = self.requests.overflow();
            if owed(over) {
                over.frames.fetch_add(1, Relaxed);
                reasons.unnamed();
            }
        }
        let frames = self.frames.get(window + 1, window);
        frames.frames.fetch_add(1, Relaxed);
        if reasons.is_empty() {
            frames.unasked.fetch_add(1, Relaxed);
        }
        reasons
    }

    /// The window with handle `hwnd` has gone: its bit is free for another, owed nothing
    pub fn window_gone(&self, hwnd: usize) {
        let key = hwnd.wrapping_add(1).max(1);
        let Some(bit) = self.find_window(key) else {
            return;
        };
        for site in self.requests.claimed() {
            site.pending.fetch_and(!bit, Relaxed);
        }
        let i = bit.trailing_zeros() as usize;
        let _ = self.windows[i].compare_exchange(key, 0, Relaxed, Relaxed);
    }

    /// A timer armed at `at`, counted as it fires or is dropped
    pub fn timer_at<F: Future + Unpin>(&'static self, at: At, timer: F) -> Armed<F> {
        let stat = self.timers.get(at_key(at), at);
        stat.armed.fetch_add(1, Relaxed);
        Armed {
            timer,
            stat,
            at,
            done: false,
        }
    }

    /// The mux client sent a `name` PDU of `bytes`
    #[inline]
    pub fn pdu_sent(&self, name: &'static str, bytes: usize) {
        let t = self.pdus.get(str_key(name), name);
        t.sent.fetch_add(1, Relaxed);
        t.sent_bytes.fetch_add(bytes as u64, Relaxed);
    }

    /// The mux client received a `name` PDU of `bytes`
    #[inline]
    pub fn pdu_received(&self, name: &'static str, bytes: usize) {
        let t = self.pdus.get(str_key(name), name);
        t.received.fetch_add(1, Relaxed);
        t.received_bytes.fetch_add(bytes as u64, Relaxed);
    }

    /// The GUI thread woke after waiting `ns` for a message or a task
    #[inline]
    pub fn woke(&self, ns: u64) {
        self.wakes.fetch_add(1, Relaxed);
        self.waited_ns.fetch_add(ns, Relaxed);
    }

    /// The message loop drained the main thread's tasks for `ns`
    pub fn drained(&self, ns: u64) {
        self.drains.add(ns);
    }

    /// Everything counted since the counts started, by name; with `reset`, they start again,
    /// each count zeroed as it's read (what's pending is still owed its frame)
    pub fn report(&self, reset: bool) -> Report {
        let now = Instant::now();
        let since = {
            let mut since = self.since.lock().unwrap_or_else(|e| e.into_inner());
            let span = since.map(|s| now.saturating_duration_since(s));
            if reset {
                *since = Some(now);
            }
            span
        };

        let at_name = |at: At| format!("{}:{}", source_name(at.file()), at.line());
        let mut report = Report {
            span: since,
            messages: runs_by_name(
                self.messages.entries().map(|(m, t)| (message_name(m).into_owned(), t.read(reset))),
                self.messages.overflow().read(reset),
            ),
            tasks: runs_by_name(
                self.tasks.entries().map(|(at, t)| (at_name(at), t.read(reset))),
                self.tasks.overflow().read(reset),
            ),
            paints: runs_by_name(
                self.paints.entries().map(|((window, part), t)| {
                    (Site::Paint { window, part }.to_string(), t.read(reset))
                }),
                self.paints.overflow().read(reset),
            ),
            windows: self
                .frames
                .entries()
                .map(|(window, f)| WindowFrames {
                    window,
                    frames: take(&f.frames, reset),
                    unasked: take(&f.unasked, reset),
                })
                .filter(|w| w.frames > 0)
                .collect(),
            requests: Vec::new(),
            timers: Vec::new(),
            mux: Vec::new(),
            wakes: take(&self.wakes, reset),
            waited_ns: take(&self.waited_ns, reset),
            drains: self.drains.read(reset),
        };
        report.windows.sort_by_key(|w| w.window);

        let mut requests: HashMap<String, RequestCount> = HashMap::new();
        for (at, r) in self.requests.entries() {
            let name = at_name(at);
            let e = requests.entry(name.clone()).or_insert(RequestCount {
                site: name,
                requests: 0,
                frames: 0,
            });
            e.requests += take(&r.requests, reset);
            e.frames += take(&r.frames, reset);
        }
        let over = self.requests.overflow();
        requests.insert(
            OVERFLOW.to_string(),
            RequestCount {
                site: OVERFLOW.to_string(),
                requests: take(&over.requests, reset),
                frames: take(&over.frames, reset),
            },
        );
        report.requests = requests.into_values().filter(|r| r.requests > 0 || r.frames > 0).collect();
        report
            .requests
            .sort_by(|a, b| (b.frames, b.requests, &a.site).cmp(&(a.frames, a.requests, &b.site)));

        let mut timers: HashMap<String, TimerCount> = HashMap::new();
        let all_timers = self
            .timers
            .entries()
            .map(|(at, t)| (at_name(at), t))
            .chain(std::iter::once((OVERFLOW.to_string(), self.timers.overflow())));
        for (name, t) in all_timers {
            let e = timers.entry(name.clone()).or_insert(TimerCount {
                site: name,
                armed: 0,
                fired: 0,
                dropped: 0,
            });
            e.armed += take(&t.armed, reset);
            e.fired += take(&t.fired, reset);
            e.dropped += take(&t.dropped, reset);
        }
        report.timers = timers
            .into_values()
            .filter(|t| t.armed > 0 || t.fired > 0 || t.dropped > 0)
            .collect();
        report
            .timers
            .sort_by(|a, b| (b.fired, b.armed, &a.site).cmp(&(a.fired, a.armed, &b.site)));

        let all_pdus = self
            .pdus
            .entries()
            .map(|(name, t)| (name.to_string(), t))
            .chain(std::iter::once((OVERFLOW.to_string(), self.pdus.overflow())));
        report.mux = all_pdus
            .map(|(pdu, t)| PduCount {
                pdu,
                sent: take(&t.sent, reset),
                sent_bytes: take(&t.sent_bytes, reset),
                received: take(&t.received, reset),
                received_bytes: take(&t.received_bytes, reset),
            })
            .filter(|p| p.sent > 0 || p.received > 0)
            .collect();
        report.mux.sort_by(|a, b| {
            (b.sent_bytes + b.received_bytes, &a.pdu).cmp(&(a.sent_bytes + a.received_bytes, &b.pdu))
        });

        report
    }
}

/// Runs by name, the ones with the same name (a site's file and line, which two call sites can
/// share) added up, most total time first, the overflow last when it has any
fn runs_by_name(entries: impl Iterator<Item = (String, Runs)>, overflow: Runs) -> Vec<(String, Runs)> {
    let mut by: HashMap<String, Runs> = HashMap::new();
    for (name, runs) in entries {
        by.entry(name).or_default().merge(runs);
    }
    let mut list: Vec<(String, Runs)> = by.into_iter().filter(|(_, r)| r.count > 0).collect();
    list.sort_by(|(an, a), (bn, b)| (b.total_ns, an).cmp(&(a.total_ns, bn)));
    if overflow.count > 0 {
        list.push((OVERFLOW.to_string(), overflow));
    }
    list
}

/// One run of what slow.rs timed, in the process's counters
#[inline]
pub fn timed(site: Site, ns: u64) {
    COUNTERS.timed(site, ns)
}

/// `at` asked the window with handle `hwnd` for a frame (window.rs's `invalidate`, which carries
/// its caller)
#[inline]
pub fn request(hwnd: usize, at: &'static Location<'static>) {
    COUNTERS.request(hwnd, at)
}

/// The window with handle `hwnd`, mux window `window`, is painting a frame (paint_impl): the
/// sites that asked for it
#[inline]
pub fn frame(hwnd: usize, window: usize) -> crate::budget::Reasons {
    COUNTERS.frame(hwnd, window)
}

/// The window with handle `hwnd` has gone
pub fn window_gone(hwnd: usize) {
    COUNTERS.window_gone(hwnd)
}

/// Whether a window handle is still a window (the GUI's: IsWindow), asked only of one that has
/// no slot yet
static WINDOW_ALIVE: OnceLock<fn(usize) -> bool> = OnceLock::new();

/// The GUI starts: how to tell a window handle that's gone
pub fn set_window_alive(alive: fn(usize) -> bool) {
    WINDOW_ALIVE.set(alive).ok();
}

/// A timer, counted by where it's armed (this call's caller) as it fires or is dropped:
/// `counters::timer(smol::Timer::at(at)).await`
#[track_caller]
#[inline]
pub fn timer<F: Future + Unpin>(timer: F) -> Armed<F> {
    COUNTERS.timer_at(Location::caller(), timer)
}

/// The GUI thread woke after waiting `ns`
#[inline]
pub fn woke(ns: u64) {
    COUNTERS.woke(ns)
}

/// The message loop drained the main thread's tasks for `ns` (lbterm_extras::work::Drain)
#[inline]
pub fn drained(ns: u64) {
    COUNTERS.drained(ns)
}

/// The mux client sent a PDU (wezterm-client's client thread, through a hook wezterm-gui sets)
pub fn pdu_sent(name: &'static str, bytes: usize) {
    COUNTERS.pdu_sent(name, bytes)
}

/// The mux client received a PDU
pub fn pdu_received(name: &'static str, bytes: usize) {
    COUNTERS.pdu_received(name, bytes)
}

/// A timer armed through [`timer`]: the timer itself, counted
pub struct Armed<F> {
    timer: F,
    stat: &'static Timers,
    at: At,
    done: bool,
}

impl<F: Future + Unpin> Future for Armed<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<F::Output> {
        let this = &mut *self;
        match Pin::new(&mut this.timer).poll(cx) {
            Poll::Ready(out) => {
                if !this.done {
                    this.done = true;
                    this.stat.fired.fetch_add(1, Relaxed);
                    // A timer firing at rest is the budget's to judge (budget.rs)
                    crate::budget::fired(this.at);
                }
                Poll::Ready(out)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<F> Drop for Armed<F> {
    fn drop(&mut self) {
        if !self.done {
            self.stat.dropped.fetch_add(1, Relaxed);
        }
    }
}

/// A window's frames in a report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowFrames {
    pub window: usize,
    pub frames: u64,
    pub unasked: u64,
}

/// A site's requests in a report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestCount {
    pub site: String,
    pub requests: u64,
    pub frames: u64,
}

/// A site's timers in a report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerCount {
    pub site: String,
    pub armed: u64,
    pub fired: u64,
    pub dropped: u64,
}

/// A PDU type's traffic in a report
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PduCount {
    pub pdu: String,
    pub sent: u64,
    pub sent_bytes: u64,
    pub received: u64,
    pub received_bytes: u64,
}

/// Everything counted over a span, by name, most first
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Since the counts started (the process's start, or the last reset); none before
    /// [`Counters::start`]
    pub span: Option<Duration>,
    pub messages: Vec<(String, Runs)>,
    pub tasks: Vec<(String, Runs)>,
    pub paints: Vec<(String, Runs)>,
    pub windows: Vec<WindowFrames>,
    pub requests: Vec<RequestCount>,
    pub timers: Vec<TimerCount>,
    pub mux: Vec<PduCount>,
    pub wakes: u64,
    pub waited_ns: u64,
    /// The message loop's drains of the main thread's tasks
    pub drains: Runs,
}

/// Milliseconds, to the microsecond
pub fn ms(ns: u64) -> f64 {
    (ns as f64 / 1_000.0).round() / 1_000.0
}

fn runs_json(list: &[(String, Runs)]) -> Value {
    Value::Array(
        list.iter()
            .map(|(site, r)| {
                json!({
                    "site": site,
                    "count": r.count,
                    "total_ms": ms(r.total_ns),
                    "max_ms": ms(r.max_ns),
                })
            })
            .collect(),
    )
}

fn sum_count(list: &[(String, Runs)]) -> u64 {
    list.iter().map(|(_, r)| r.count).sum()
}

fn sum_ms(list: &[(String, Runs)]) -> f64 {
    ms(list.iter().map(|(_, r)| r.total_ns).sum())
}

impl Report {
    /// The report as `metrics` replies with it (lbterm/CONTROL.md), its fields in the reply
    pub fn to_json(&self) -> Map<String, Value> {
        let span_ns = self.span.map(|s| s.as_nanos() as u64);
        let frames: u64 = self.windows.iter().map(|w| w.frames).sum();
        let mut out = Map::new();
        out.insert("span_ms".into(), json!(span_ns.map(ms)));
        out.insert(
            "totals".into(),
            json!({
                "frames": frames,
                "unasked": self.windows.iter().map(|w| w.unasked).sum::<u64>(),
                "requests": self.requests.iter().map(|r| r.requests).sum::<u64>(),
                "messages": sum_count(&self.messages),
                "message_ms": sum_ms(&self.messages),
                "tasks": sum_count(&self.tasks),
                "task_ms": sum_ms(&self.tasks),
                "paints": sum_count(&self.paints),
                "paint_ms": sum_ms(&self.paints),
                "timers_fired": self.timers.iter().map(|t| t.fired).sum::<u64>(),
                "wakes": self.wakes,
                "pdus_sent": self.mux.iter().map(|p| p.sent).sum::<u64>(),
                "pdus_received": self.mux.iter().map(|p| p.received).sum::<u64>(),
            }),
        );
        out.insert(
            "gui".into(),
            json!({
                "wakes": self.wakes,
                "waited_ms": ms(self.waited_ns),
                "drains": self.drains.count,
                "drain_ms": ms(self.drains.total_ns),
                "longest_drain_ms": ms(self.drains.max_ns),
                // What the span wasn't spent waiting: handling messages and tasks, and painting
                "busy_ms": span_ns.map(|s| ms(s.saturating_sub(self.waited_ns))),
            }),
        );
        out.insert(
            "windows".into(),
            Value::Array(
                self.windows
                    .iter()
                    .map(|w| json!({ "window": w.window, "frames": w.frames, "unasked": w.unasked }))
                    .collect(),
            ),
        );
        out.insert(
            "requests".into(),
            Value::Array(
                self.requests
                    .iter()
                    .map(|r| json!({ "site": r.site, "requests": r.requests, "frames": r.frames }))
                    .collect(),
            ),
        );
        out.insert("messages".into(), runs_json(&self.messages));
        out.insert("tasks".into(), runs_json(&self.tasks));
        out.insert("paints".into(), runs_json(&self.paints));
        out.insert(
            "timers".into(),
            Value::Array(
                self.timers
                    .iter()
                    .map(|t| {
                        json!({ "site": t.site, "armed": t.armed, "fired": t.fired, "dropped": t.dropped })
                    })
                    .collect(),
            ),
        );
        out.insert(
            "mux".into(),
            Value::Array(
                self.mux
                    .iter()
                    .map(|p| {
                        json!({
                            "pdu": p.pdu,
                            "sent": p.sent, "sent_bytes": p.sent_bytes,
                            "received": p.received, "received_bytes": p.received_bytes,
                        })
                    })
                    .collect(),
            ),
        );
        out
    }
}

/// The process as Windows counts it: what it has spent (CPU, the GUI thread's own) and what it
/// holds (memory, handles, threads, GDI and USER objects). wezterm-gui's counters.rs reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Process {
    pub user_ns: u64,
    pub kernel_ns: u64,
    pub gui_thread_ns: u64,
    pub gui_thread_cycles: u64,
    pub private_bytes: u64,
    pub working_set: u64,
    pub peak_working_set: u64,
    pub handles: u64,
    pub threads: u64,
    pub gdi_objects: u64,
    pub user_objects: u64,
}

impl Process {
    /// What it spent since `base`, and what it holds now
    pub fn since(&self, base: &Process) -> Process {
        Process {
            user_ns: self.user_ns.saturating_sub(base.user_ns),
            kernel_ns: self.kernel_ns.saturating_sub(base.kernel_ns),
            gui_thread_ns: self.gui_thread_ns.saturating_sub(base.gui_thread_ns),
            gui_thread_cycles: self.gui_thread_cycles.saturating_sub(base.gui_thread_cycles),
            ..*self
        }
    }

    /// As `metrics` replies with it: over `span`, with the share of one core each took
    pub fn to_json(&self, span: Option<Duration>) -> Value {
        let cpu_ns = self.user_ns + self.kernel_ns;
        let percent = |ns: u64| {
            span.filter(|s| !s.is_zero())
                .map(|s| (ns as f64 / s.as_nanos() as f64 * 1_000.0).round() / 10.0)
        };
        json!({
            "cpu_ms": ms(cpu_ns),
            "user_ms": ms(self.user_ns),
            "kernel_ms": ms(self.kernel_ns),
            "cpu_percent": percent(cpu_ns),
            "gui_thread_ms": ms(self.gui_thread_ns),
            "gui_thread_percent": percent(self.gui_thread_ns),
            "gui_thread_cycles": self.gui_thread_cycles,
            "private_bytes": self.private_bytes,
            "working_set": self.working_set,
            "peak_working_set": self.peak_working_set,
            "handles": self.handles,
            "threads": self.threads,
            "gdi_objects": self.gdi_objects,
            "user_objects": self.user_objects,
        })
    }
}

/// One of the process's threads, as Windows counts it: its id, its name (Rust names its threads;
/// empty for one without), and the CPU it has spent
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadTime {
    pub tid: u32,
    pub name: String,
    pub ns: u64,
}

/// What each thread spent since `base` (each thread's CPU when the counts last started, by id;
/// a thread not in it started since), the busiest first, the idle left out
pub fn threads_since(now: &[ThreadTime], base: &HashMap<u32, u64>) -> Vec<ThreadTime> {
    let mut spent: Vec<ThreadTime> = now
        .iter()
        .map(|t| ThreadTime {
            ns: t.ns.saturating_sub(base.get(&t.tid).copied().unwrap_or(0)),
            ..t.clone()
        })
        .filter(|t| t.ns > 0)
        .collect();
    spent.sort_by(|a, b| (b.ns, a.tid).cmp(&(a.ns, b.tid)));
    spent
}

/// Threads' CPU as `metrics` replies with it, over `span`
pub fn threads_json(threads: &[ThreadTime], span: Option<Duration>) -> Value {
    Value::Array(
        threads
            .iter()
            .map(|t| {
                let percent = span
                    .filter(|s| !s.is_zero())
                    .map(|s| (t.ns as f64 / s.as_nanos() as f64 * 1_000.0).round() / 10.0);
                json!({ "tid": t.tid, "name": t.name, "cpu_ms": ms(t.ns), "cpu_percent": percent })
            })
            .collect(),
    )
}

/// What one of upstream's histograms says (wezterm-gui's stats.rs): its key, what it measures,
/// how many it has and where they fall
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    /// The key, its labels in braces: `pdu.size{pdu=GetPaneRenderChanges}`
    pub name: String,
    pub unit: Unit,
    pub count: u64,
    pub mean: f64,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// A time, recorded in nanoseconds and reported in milliseconds
    Nanoseconds,
    /// A size or a count, as it was recorded (`.size` keys)
    Plain,
    /// Events a second, a value for each second that had any (`.rate` keys)
    PerSecond,
}

impl Histogram {
    pub fn to_json(&self) -> Value {
        match self.unit {
            Unit::Nanoseconds => json!({
                "name": self.name, "unit": "ms", "count": self.count,
                "mean": ms(self.mean as u64), "p50": ms(self.p50), "p95": ms(self.p95),
                "p99": ms(self.p99), "max": ms(self.max),
            }),
            Unit::Plain | Unit::PerSecond => json!({
                "name": self.name,
                "unit": if self.unit == Unit::PerSecond { "per_second" } else { "value" },
                "count": self.count,
                "mean": (self.mean * 10.0).round() / 10.0,
                "p50": self.p50, "p95": self.p95, "p99": self.p99, "max": self.max,
            }),
        }
    }
}

/// A key's name with its labels, as a report names it
pub fn key_name<'a>(name: &str, labels: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let labels: Vec<String> = labels.map(|(k, v)| format!("{k}={v}")).collect();
    if labels.is_empty() {
        name.to_string()
    } else {
        format!("{name}{{{}}}", labels.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where it's called from: each call on a line of its own is a site of its own
    #[track_caller]
    fn here() -> At {
        Location::caller()
    }

    #[test]
    fn a_site_is_claimed_once_and_found_again() {
        static T: Table<u32, Timing, 8> = Table::new();
        let a = T.get(11, 10) as *const Timing;
        let b = T.get(21, 20) as *const Timing;
        assert_ne!(a, b);
        assert_eq!(T.get(11, 10) as *const Timing, a, "the same key, the same slot");
        assert_eq!(T.len(), 2);
        assert!(T.find(11).is_some());
        assert!(T.find(31).is_none(), "find claims nothing");
        assert_eq!(T.len(), 2);
        let mut sites: Vec<u32> = T.entries().map(|(s, _)| s).collect();
        sites.sort();
        assert_eq!(sites, [10, 20]);
    }

    #[test]
    fn a_full_table_counts_the_rest_in_its_overflow() {
        static T: Table<u32, Timing, 4> = Table::new();
        for key in 1..=4 {
            T.get(key, key as u32).add(1);
        }
        assert_eq!(T.len(), 4);
        T.get(99, 99).add(5);
        T.get(98, 98).add(7);
        let over = T.overflow().read(false);
        assert_eq!(over, Runs { count: 2, total_ns: 12, max_ns: 7 });
        assert_eq!(T.entries().count(), 4, "nothing more claimed");
    }

    #[test]
    fn runs_count_total_and_longest() {
        let t = Timing::ZERO;
        for ns in [5, 30, 10] {
            t.add(ns);
        }
        assert_eq!(t.read(true), Runs { count: 3, total_ns: 45, max_ns: 30 });
        assert_eq!(t.read(false), Runs::default(), "taken and zeroed");
    }

    #[test]
    fn what_slow_times_lands_in_its_kinds_table() {
        static C: Counters = Counters::new();
        let at = here();
        C.timed(Site::Message(0x0100), 2_000);
        C.timed(Site::Message(0x0100), 4_000);
        C.timed(Site::Task(at), 1_500_000);
        C.timed(Site::Paint { window: 3, part: Part::Present }, 9_000_000);
        C.timed(Site::Paint { window: 3, part: Part::Pass }, 1_000);
        C.start();
        let r = C.report(false);
        assert_eq!(r.messages, [("WM_KEYDOWN".to_string(), Runs { count: 2, total_ns: 6_000, max_ns: 4_000 })]);
        assert_eq!(r.tasks[0].0, format!("lbterm-extras/src/counters.rs:{}", at.line()));
        assert_eq!(r.tasks[0].1.count, 1);
        let paints: Vec<&str> = r.paints.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(paints, ["window-3/present", "window-3/pass"], "most time first");
    }

    #[test]
    fn a_frame_is_credited_to_the_requests_since_the_last() {
        static C: Counters = Counters::new();
        let a = here();
        let b = here();
        let (w1, w2) = (0x1000, 0x2000);
        C.request(w1, a);
        C.request(w1, a);
        C.request(w1, b);
        C.request(w2, b);
        C.frame(w1, 0);
        // b's request for the other window is still owed its frame
        C.frame(w1, 0);
        C.frame(w2, 1);
        let r = C.report(false);
        let req = |at: At| {
            let name = format!("lbterm-extras/src/counters.rs:{}", at.line());
            r.requests.iter().find(|q| q.site == name).cloned().unwrap()
        };
        assert_eq!((req(a).requests, req(a).frames), (2, 1), "two requests, one frame");
        assert_eq!((req(b).requests, req(b).frames), (2, 2), "one frame in each window");
        assert_eq!(
            r.windows,
            [
                WindowFrames { window: 0, frames: 2, unasked: 1 },
                WindowFrames { window: 1, frames: 1, unasked: 0 },
            ],
            "window 0's second frame had nobody asking"
        );
    }

    #[test]
    fn a_frame_names_who_asked_for_it() {
        static C: Counters = Counters::new();
        let (a, b) = (here(), here());
        C.request(0x70, a);
        C.request(0x70, b);
        C.request(0x70, a);
        let mut sites: Vec<u32> = C.frame(0x70, 0).iter().map(|at| at.line()).collect();
        sites.sort();
        assert_eq!(sites, [a.line(), b.line()]);
        assert!(C.frame(0x70, 0).is_empty(), "nobody asked again");
    }

    #[test]
    fn a_window_that_goes_owes_nothing_and_frees_its_bit() {
        static C: Counters = Counters::new();
        let a = here();
        C.request(0x10, a);
        C.window_gone(0x10);
        C.frame(0x10, 7);
        let r = C.report(false);
        assert_eq!(r.windows, [WindowFrames { window: 7, frames: 1, unasked: 1 }]);
        assert_eq!(r.requests[0].frames, 0);
        // Its bit is free: every other window can still be told apart
        for hwnd in 0..WINDOWS {
            assert!(C.window_bit(0x100 + hwnd).is_some(), "window {hwnd}");
        }
        assert_eq!(C.window_bit(0x9999), None, "all {WINDOWS} bits taken");
        assert!(C.out_of_bits.load(Relaxed), "and that's said, once");
    }

    #[test]
    fn a_request_for_a_window_that_has_gone_claims_no_bit() {
        static C: Counters = Counters::new();
        // The GUI's check: in the tests, handles from 0xDEAD00 up are gone
        set_window_alive(|hwnd| hwnd < 0xDEAD00);
        let a = here();
        C.request(0xDEAD01, a);
        assert!(C.windows.iter().all(|slot| slot.load(Relaxed) == 0), "no slot taken");
        assert!(C.frame(0xDEAD01, 3).is_empty());
        // A live one is told apart as before
        C.request(0x20, a);
        assert!(!C.frame(0x20, 4).is_empty());
    }

    #[test]
    fn a_reset_reports_then_starts_again_but_keeps_what_is_owed() {
        static C: Counters = Counters::new();
        let a = here();
        C.start();
        C.timed(Site::Message(0x000F), 100);
        C.request(0x30, a);
        C.woke(1_000);
        let first = C.report(true);
        assert_eq!(first.messages.len(), 1);
        assert_eq!(first.wakes, 1);
        assert!(first.span.is_some());
        let empty = C.report(false);
        assert!(empty.messages.is_empty() && empty.requests.is_empty() && empty.wakes == 0);
        // The request made before the reset still asks for the frame after it
        C.frame(0x30, 0);
        let after = C.report(false);
        assert_eq!(after.requests[0].frames, 1);
        assert_eq!(after.windows[0].unasked, 0);
    }

    #[test]
    fn timers_are_counted_as_they_fire_or_are_dropped() {
        static C: Counters = Counters::new();
        struct Ready(bool);
        impl Future for Ready {
            type Output = ();
            fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<()> {
                if self.0 {
                    Poll::Ready(())
                } else {
                    self.0 = true;
                    Poll::Pending
                }
            }
        }
        let at = here();
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(&waker);
        let mut fired = C.timer_at(at, Ready(false));
        assert!(Pin::new(&mut fired).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut fired).poll(&mut cx).is_ready());
        drop(fired);
        drop(C.timer_at(at, Ready(false)));
        let r = C.report(false);
        assert_eq!(
            r.timers,
            [TimerCount {
                site: format!("lbterm-extras/src/counters.rs:{}", at.line()),
                armed: 2,
                fired: 1,
                dropped: 1
            }]
        );
    }

    #[test]
    fn pdus_are_counted_each_way_by_type() {
        static C: Counters = Counters::new();
        C.pdu_sent("WriteToPane", 40);
        C.pdu_received("GetPaneRenderChangesResponse", 3_000);
        C.pdu_received("GetPaneRenderChangesResponse", 1_000);
        C.pdu_sent("Ping", 2);
        let r = C.report(false);
        assert_eq!(
            r.mux[0],
            PduCount {
                pdu: "GetPaneRenderChangesResponse".into(),
                sent: 0,
                sent_bytes: 0,
                received: 2,
                received_bytes: 4_000
            },
            "most bytes first"
        );
        assert_eq!(r.mux.len(), 3);
    }

    #[test]
    fn sites_on_one_line_are_added_up_and_the_overflow_comes_last() {
        let list = runs_by_name(
            [
                ("a.rs:1".to_string(), Runs { count: 1, total_ns: 10, max_ns: 10 }),
                ("b.rs:2".to_string(), Runs { count: 1, total_ns: 50, max_ns: 50 }),
                ("a.rs:1".to_string(), Runs { count: 2, total_ns: 60, max_ns: 40 }),
                ("c.rs:3".to_string(), Runs::default()),
            ]
            .into_iter(),
            Runs { count: 1, total_ns: 1, max_ns: 1 },
        );
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a.rs:1", "b.rs:2", OVERFLOW], "c ran none: left out");
        assert_eq!(list[0].1, Runs { count: 3, total_ns: 70, max_ns: 40 });
    }

    #[test]
    fn the_report_as_json() {
        static C: Counters = Counters::new();
        let a = here();
        C.start();
        C.request(0x50, a);
        C.frame(0x50, 0);
        C.timed(Site::Paint { window: 0, part: Part::Pass }, 2_500_000);
        C.timed(Site::Message(0x0100), 1_234_567);
        C.woke(10_000_000);
        C.drained(3_000_000);
        C.drained(1_000_000);
        let mut r = C.report(false);
        r.span = Some(Duration::from_millis(100));
        let j = Value::Object(r.to_json());
        assert_eq!(j["span_ms"], json!(100.0));
        assert_eq!(j["totals"]["frames"], json!(1));
        assert_eq!(j["totals"]["requests"], json!(1));
        assert_eq!(j["totals"]["messages"], json!(1));
        assert_eq!(j["totals"]["paint_ms"], json!(2.5));
        assert_eq!(
            j["gui"],
            json!({
                "wakes": 1,
                "waited_ms": 10.0,
                "busy_ms": 90.0,
                "drains": 2,
                "drain_ms": 4.0,
                "longest_drain_ms": 3.0
            })
        );
        assert_eq!(j["windows"], json!([{ "window": 0, "frames": 1, "unasked": 0 }]));
        assert_eq!(
            j["requests"],
            json!([{ "site": format!("lbterm-extras/src/counters.rs:{}", a.line()), "requests": 1, "frames": 1 }])
        );
        assert_eq!(
            j["messages"],
            json!([{ "site": "WM_KEYDOWN", "count": 1, "total_ms": 1.235, "max_ms": 1.235 }])
        );
        assert_eq!(j["mux"], json!([]));
    }

    #[test]
    fn the_process_over_a_span() {
        let base = Process { user_ns: 1_000_000, kernel_ns: 0, gui_thread_ns: 500_000, gui_thread_cycles: 10, handles: 400, ..Default::default() };
        let now = Process {
            user_ns: 21_000_000,
            kernel_ns: 10_000_000,
            gui_thread_ns: 10_500_000,
            gui_thread_cycles: 1_010,
            private_bytes: 50 << 20,
            handles: 410,
            ..Default::default()
        };
        let d = now.since(&base);
        assert_eq!((d.user_ns, d.kernel_ns, d.gui_thread_ns, d.gui_thread_cycles), (20_000_000, 10_000_000, 10_000_000, 1_000));
        assert_eq!((d.handles, d.private_bytes), (410, 50 << 20), "what it holds is now's");
        let j = d.to_json(Some(Duration::from_millis(100)));
        assert_eq!(j["cpu_ms"], json!(30.0));
        assert_eq!(j["cpu_percent"], json!(30.0), "30 ms of 100 is 30% of a core");
        assert_eq!(j["gui_thread_percent"], json!(10.0));
        assert_eq!(d.to_json(None)["cpu_percent"], Value::Null);
    }

    #[test]
    fn each_thread_s_cpu_over_the_span_the_busiest_first() {
        let t = |tid, name: &str, ns| ThreadTime { tid, name: name.into(), ns };
        let now = [t(1, "main", 50_000_000), t(2, "lbterm-state", 4_000_000), t(3, "", 9_000_000), t(4, "new", 2_000_000)];
        let base: HashMap<u32, u64> = [(1, 40_000_000), (2, 4_000_000), (3, 1_000_000)].into_iter().collect();
        let spent = threads_since(&now, &base);
        assert_eq!(spent, [t(1, "main", 10_000_000), t(3, "", 8_000_000), t(4, "new", 2_000_000)], "lbterm-state spent nothing");
        let j = threads_json(&spent, Some(Duration::from_millis(100)));
        assert_eq!(j[0], json!({ "tid": 1, "name": "main", "cpu_ms": 10.0, "cpu_percent": 10.0 }));
        assert_eq!(j[2]["cpu_percent"], json!(2.0));
    }

    #[test]
    fn histograms_and_their_names() {
        let h = Histogram { name: "gui.paint.impl".into(), unit: Unit::Nanoseconds, count: 3, mean: 2_000_000.0, p50: 1_500_000, p95: 4_000_000, p99: 4_000_000, max: 4_100_000 };
        assert_eq!(
            h.to_json(),
            json!({ "name": "gui.paint.impl", "unit": "ms", "count": 3, "mean": 2.0, "p50": 1.5, "p95": 4.0, "p99": 4.0, "max": 4.1 })
        );
        let s = Histogram { name: "pdu.size".into(), unit: Unit::Plain, count: 2, mean: 12.25, p50: 10, p95: 15, p99: 15, max: 15 };
        assert_eq!(s.to_json()["unit"], json!("value"));
        assert_eq!(s.to_json()["mean"], json!(12.3));
        assert_eq!(key_name("pdu.size", [("pdu", "Ping")].into_iter()), "pdu.size{pdu=Ping}");
        assert_eq!(key_name("rpc", std::iter::empty()), "rpc");
    }

    /// What an event costs, measured: printed, held only to a loose ceiling so a slow test machine
    /// doesn't fail it (lbterm/PERF.md has the numbers)
    #[test]
    fn an_event_costs_well_under_a_microsecond() {
        static C: Counters = Counters::new();
        let at = here();
        let n = 1_000_000u32;
        let t = Instant::now();
        for i in 0..n {
            C.timed(Site::Task(at), i as u64);
        }
        let task_ns = t.elapsed().as_nanos() as f64 / n as f64;
        let t = Instant::now();
        for _ in 0..n {
            C.request(0x60, at);
        }
        let request_ns = t.elapsed().as_nanos() as f64 / n as f64;
        let t = Instant::now();
        for _ in 0..n {
            let _ = std::hint::black_box(Instant::now());
        }
        let now_ns = t.elapsed().as_nanos() as f64 / n as f64;
        eprintln!("a timed run {task_ns:.1} ns, a request {request_ns:.1} ns, Instant::now {now_ns:.1} ns");
        assert!(task_ns < 1_000.0 && request_ns < 1_000.0, "{task_ns} {request_ns}");
    }
}
