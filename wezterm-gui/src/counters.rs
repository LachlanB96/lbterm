//! LBTerm: the control pipe's `metrics` (lbterm/PERF.md, lbterm/CONTROL.md): the counters
//! (lbterm_extras::counters), the process as Windows counts it, and upstream's histograms and
//! counters (stats.rs), over one span: since the process started, or since the last request that
//! asked for a reset.
//!
//! Answered on the pipe's own thread (control.rs), never the GUI thread's: asking adds no task, no
//! wake and no frame to what it measures, and it answers while the GUI thread is held, which is
//! when it's wanted most. The GUI thread's own CPU is read through a handle to it, opened when the
//! counts start, and every thread's, by name, from a snapshot of the process's threads.

use lbterm_extras::counters::{self, Process, ThreadTime};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// The process as it was when the counts last started, and each thread's CPU then, by id; none is
/// all zeros, its start
static BASE: Mutex<Option<(Process, HashMap<u32, u64>)>> = Mutex::new(None);

/// A handle to the GUI thread, for its CPU time and cycles
static GUI_THREAD: OnceLock<usize> = OnceLock::new();

/// The counts start: on the GUI thread, before its message loop, as the GUI starts
pub fn start() {
    counters::COUNTERS.start();
    #[cfg(windows)]
    GUI_THREAD.get_or_init(|| unsafe {
        use winapi::um::processthreadsapi::{GetCurrentThreadId, OpenThread};
        use winapi::um::winnt::THREAD_QUERY_LIMITED_INFORMATION;
        let handle = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, GetCurrentThreadId());
        if handle.is_null() {
            log::error!(
                "counters: no handle to the GUI thread, so no CPU of its own in metrics: {}",
                std::io::Error::last_os_error()
            );
        }
        handle as usize
    });
    // A request for a window that has gone (from a task that outlived it) claims no window's slot
    #[cfg(windows)]
    counters::set_window_alive(|hwnd| unsafe { winapi::um::winuser::IsWindow(hwnd as _) != 0 });
    // Timing frames to the screen: off unless a test window asks as it starts, or a metrics
    // request later (lbterm_extras::present)
    if std::env::var_os(lbterm_extras::present::ENV).is_some() {
        lbterm_extras::present::set_on(true);
    }
    // The budget's waker, which ends its spans and says what's held on time (termwindow/budget.rs)
    crate::termwindow::budget::start();
    // The mux client's PDUs, by type, each way
    wezterm_client::client::lbterm_set_traffic_hook(|name, bytes, sent| {
        if sent {
            counters::pdu_sent(name, bytes)
        } else {
            counters::pdu_received(name, bytes)
        }
    });
}

/// `metrics`'s reply: everything over the span, and with `reset` the next span starting now
pub fn reply(id: &Value, reset: bool) -> String {
    let started = Instant::now();
    let report = counters::COUNTERS.report(reset);
    let counted = Instant::now();
    let mut now = process();
    let read = Instant::now();
    let threads = threads();
    now.threads = threads.len() as u64;
    let listed = Instant::now();
    let (spent, threads_spent) = {
        let mut base = BASE.lock().unwrap_or_else(|e| e.into_inner());
        let (spent, threads_spent) = match base.as_ref() {
            Some((process, by_thread)) => (now.since(process), counters::threads_since(&threads, by_thread)),
            None => (now, counters::threads_since(&threads, &HashMap::new())),
        };
        if reset {
            *base = Some((now, threads.iter().map(|t| (t.tid, t.ns)).collect()));
        }
        (spent, threads_spent)
    };
    let (histograms, upstream) = crate::stats::lbterm_snapshot(reset);
    let snapped = Instant::now();
    let mut fields = report.to_json();
    fields.insert("pid".into(), json!(std::process::id()));
    fields.insert("build".into(), json!(config::wezterm_version()));
    fields.insert("reset".into(), json!(reset));
    fields.insert("process".into(), spent.to_json(report.span));
    fields.insert("threads".into(), counters::threads_json(&threads_spent, report.span));
    fields.insert(
        "histograms".into(),
        Value::Array(histograms.iter().map(|h| h.to_json()).collect()),
    );
    // What the budget judged (lbterm_extras::budget): at rest, on purpose, each action's times
    fields.insert("budget".into(), lbterm_extras::budget::report_json(reset));
    fields.insert("present".into(), lbterm_extras::present::report_json(reset));
    // LBTerm: first frames that waited for a held pane's push (termwindow/frame_wait.rs)
    fields.insert("frame_wait".into(), lbterm_extras::frame_wait::WAITS.report_json(reset));
    // What font setup did: wezterm-font's tallies and the GUI's glyph atlases and reshapes
    fields.insert("fonts".into(), fonts::report_json(reset));
    // How the last window's start went, step by step (lbterm_extras::start)
    fields.insert("start".into(), lbterm_extras::start::report_json());
    // What the caches did, by name: hits, misses and entries put out to make room
    fields.insert("caches".into(), caches_json(reset));
    // The cards' words laid out and kept from the frame before (lbterm_extras::kept)
    fields.insert("words".into(), lbterm_extras::kept::report_json(reset));
    // ...and the parts kept as quads, made and copied (render/kept_quads.rs)
    fields.insert("quads".into(), lbterm_extras::kept::quads_report_json(reset));
    fields.insert(
        "upstream_counters".into(),
        Value::Object(upstream.into_iter().map(|(k, v)| (k, json!(v))).collect()),
    );
    // What this reply cost, on the pipe's thread, by part
    let part = |a: Instant, b: Instant| counters::ms(b.saturating_duration_since(a).as_nanos() as u64);
    fields.insert(
        "reply_ms".into(),
        json!({
            "counters": part(started, counted),
            "process": part(counted, read),
            "threads": part(read, listed),
            "histograms": part(listed, snapped),
            "json": part(snapped, Instant::now()),
        }),
    );
    lbterm_extras::control::metrics_reply(id, fields)
}

/// `metrics`' `caches`: each cache name's lookups that found their entry, those that didn't, and
/// entries put out to make room, over the span (lfucache's counts: the line, shape and image
/// caches, every window's of a name together, and the glyph cache's); a name that did none left
/// out
fn caches_json(reset: bool) -> Value {
    Value::Object(
        lfucache::report(reset)
            .into_iter()
            .filter(|(_, r)| r.hits + r.misses + r.evictions > 0)
            .map(|(name, r)| {
                (
                    name.to_string(),
                    json!({ "hits": r.hits, "misses": r.misses, "evictions": r.evictions }),
                )
            })
            .collect(),
    )
}

/// LBTerm: the GUI's side of what font setup costs (lbterm/PERF.md, "What font setup costs");
/// wezterm-font's lbterm.rs counts the fonts' own. `metrics` reports both under `fonts`, each
/// `{count, ms}`.
pub mod fonts {
    use serde_json::{json, Map, Value};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    use wezterm_font::lbterm::Tally;

    /// Glyph atlases made: each window's first, a zoom's, a reload's that changed the fonts, and
    /// one made again when full
    pub static ATLAS_MADE: Tally = Tally::new();
    /// Full, and made again at the same size: every glyph rasterized again and every shape made
    /// again, to free what's no longer drawn
    pub static ATLAS_CLEARED: Tally = Tally::new();
    /// Full, and made again twice as big: the same, to hold more
    pub static ATLAS_GROWN: Tally = Tally::new();
    /// A window's shapes all cleared at once: a fallback found, an atlas made again, a GPU lost
    pub static RESHAPES: Tally = Tally::new();
    /// The largest side a window's atlas has had in this process: where the next window's starts
    static ATLAS_SIDE: AtomicUsize = AtomicUsize::new(0);

    /// An atlas made, `side` pixels a side
    pub fn atlas_made(side: usize) {
        ATLAS_MADE.one();
        ATLAS_SIDE.fetch_max(side, Relaxed);
    }

    /// The side a new window's first atlas starts at: the largest any window's has grown to, and
    /// never under config's ATLAS_START or `least`
    pub fn atlas_start(least: usize) -> usize {
        least
            .max(config::lbterm::ATLAS_START)
            .max(ATLAS_SIDE.load(Relaxed))
    }

    fn entry(n: u64, ns: u64) -> Value {
        json!({ "count": n, "ms": lbterm_extras::counters::ms(ns) })
    }

    /// What `metrics` says under `fonts`, zeroed with `reset` (but the atlas's side, a high-water
    /// mark)
    pub fn report_json(reset: bool) -> Value {
        let mut o = Map::new();
        for (name, n, ns) in wezterm_font::lbterm::report(reset) {
            o.insert(name.into(), entry(n, ns));
        }
        for (name, t) in [
            ("atlas_made", &ATLAS_MADE),
            ("atlas_cleared", &ATLAS_CLEARED),
            ("atlas_grown", &ATLAS_GROWN),
            ("reshapes", &RESHAPES),
        ] {
            let (n, ns) = t.read(reset);
            o.insert(name.into(), entry(n, ns));
        }
        o.insert("atlas_side".into(), json!(ATLAS_SIDE.load(Relaxed)));
        Value::Object(o)
    }
}

// Not in winapi 0.3
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn QueryThreadCycleTime(
        thread: winapi::um::winnt::HANDLE,
        cycles: *mut u64,
    ) -> winapi::shared::minwindef::BOOL;
}
#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn GetGuiResources(
        process: winapi::um::winnt::HANDLE,
        flags: winapi::shared::minwindef::DWORD,
    ) -> winapi::shared::minwindef::DWORD;
}
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetThreadDescription(
        thread: winapi::um::winnt::HANDLE,
        description: *mut *mut u16,
    ) -> winapi::shared::winerror::HRESULT;
}
// Not in winapi either: every thread of a process, one handle after another (ntdll's since
// Windows Vista)
#[cfg(windows)]
#[link(name = "ntdll", kind = "raw-dylib")]
extern "system" {
    fn NtGetNextThread(
        process: winapi::um::winnt::HANDLE,
        thread: winapi::um::winnt::HANDLE,
        access: u32,
        attributes: u32,
        flags: u32,
        next: *mut winapi::um::winnt::HANDLE,
    ) -> i32;
}
#[cfg(windows)]
const GR_GDIOBJECTS: u32 = 0;
#[cfg(windows)]
const GR_USEROBJECTS: u32 = 1;

/// The process now, as Windows counts it, but its threads
#[cfg(windows)]
fn process() -> Process {
    use std::mem::{size_of, zeroed};
    use winapi::shared::minwindef::{DWORD, FILETIME};
    use winapi::um::processthreadsapi::{
        GetCurrentProcess, GetProcessHandleCount, GetProcessTimes, GetThreadTimes,
    };
    use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};

    fn ns(t: &FILETIME) -> u64 {
        (((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64) * 100
    }
    fn failed(what: &str) {
        log::error!("metrics: {what} failed: {}", std::io::Error::last_os_error());
    }

    let mut p = Process::default();
    unsafe {
        let me = GetCurrentProcess();
        let (mut created, mut exited, mut kernel, mut user): (FILETIME, FILETIME, FILETIME, FILETIME) =
            (zeroed(), zeroed(), zeroed(), zeroed());
        if GetProcessTimes(me, &mut created, &mut exited, &mut kernel, &mut user) != 0 {
            p.kernel_ns = ns(&kernel);
            p.user_ns = ns(&user);
        } else {
            failed("GetProcessTimes");
        }
        if let Some(&thread) = GUI_THREAD.get().filter(|&&t| t != 0) {
            let thread = thread as winapi::um::winnt::HANDLE;
            if GetThreadTimes(thread, &mut created, &mut exited, &mut kernel, &mut user) != 0 {
                p.gui_thread_ns = ns(&kernel) + ns(&user);
            } else {
                failed("GetThreadTimes");
            }
            let mut cycles = 0u64;
            if QueryThreadCycleTime(thread, &mut cycles) != 0 {
                p.gui_thread_cycles = cycles;
            } else {
                failed("QueryThreadCycleTime");
            }
        }
        let mut mem: PROCESS_MEMORY_COUNTERS_EX = zeroed();
        mem.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as DWORD;
        if GetProcessMemoryInfo(me, &mut mem as *mut _ as *mut PROCESS_MEMORY_COUNTERS, mem.cb) != 0 {
            p.private_bytes = mem.PrivateUsage as u64;
            p.working_set = mem.WorkingSetSize as u64;
            p.peak_working_set = mem.PeakWorkingSetSize as u64;
        } else {
            failed("GetProcessMemoryInfo");
        }
        let mut handles: DWORD = 0;
        if GetProcessHandleCount(me, &mut handles) != 0 {
            p.handles = handles as u64;
        } else {
            failed("GetProcessHandleCount");
        }
        p.gdi_objects = GetGuiResources(me, GR_GDIOBJECTS) as u64;
        p.user_objects = GetGuiResources(me, GR_USEROBJECTS) as u64;
    }
    p
}

/// The process's threads, each with its name and the CPU it has spent, walked one handle at a
/// time with ntdll's NtGetNextThread. Not Toolhelp's snapshot: that lists every thread on the
/// machine, and Thread32Next maps the snapshot afresh at each, about 140 ms of CPU for 8,500
/// threads here (2026-10-29), where this takes a fraction of a millisecond.
#[cfg(windows)]
fn threads() -> Vec<ThreadTime> {
    use std::mem::zeroed;
    use winapi::shared::minwindef::FILETIME;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetCurrentProcess, GetThreadId, GetThreadTimes};
    use winapi::um::winbase::LocalFree;
    use winapi::um::winnt::{HANDLE, THREAD_QUERY_LIMITED_INFORMATION};

    let mut out = vec![];
    unsafe {
        let me = GetCurrentProcess();
        let mut prev: HANDLE = std::ptr::null_mut();
        loop {
            let mut next: HANDLE = std::ptr::null_mut();
            let status = NtGetNextThread(me, prev, THREAD_QUERY_LIMITED_INFORMATION, 0, 0, &mut next);
            if !prev.is_null() {
                CloseHandle(prev);
            }
            // STATUS_NO_MORE_ENTRIES once the last has been given
            if status < 0 || next.is_null() {
                break;
            }
            prev = next;
            let (mut created, mut exited, mut kernel, mut user): (FILETIME, FILETIME, FILETIME, FILETIME) =
                (zeroed(), zeroed(), zeroed(), zeroed());
            let ns = if GetThreadTimes(next, &mut created, &mut exited, &mut kernel, &mut user) != 0 {
                let ns = |t: &FILETIME| (((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64) * 100;
                ns(&kernel) + ns(&user)
            } else {
                0
            };
            let mut name = String::new();
            let mut text: *mut u16 = std::ptr::null_mut();
            if GetThreadDescription(next, &mut text) >= 0 && !text.is_null() {
                let len = (0..).take_while(|&i| *text.add(i) != 0).count();
                name = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
                LocalFree(text as _);
            }
            out.push(ThreadTime { tid: GetThreadId(next), name, ns });
        }
    }
    out
}

#[cfg(not(windows))]
fn process() -> Process {
    Process::default()
}

#[cfg(not(windows))]
fn threads() -> Vec<ThreadTime> {
    vec![]
}
