//! LBTerm: Windows' side of lbterm_extras::present (lbterm/PERF.md, "What's hard to time"), on
//! the GUI thread. Each frame the compositor draws hands over its visuals' swapchains as it
//! presents them; the frame's end says which actions it showed (the budget's), and each is
//! watched on the swapchain that shows it until DXGI's frame statistics say its present is on
//! screen. A frame that starts motion opens an episode, which DirectComposition's compositor
//! clock statistics for the window's screen are walked over once it settles. Polled at each
//! frame's start and by a timer of this file's own, which the budget leaves out of what runs at
//! rest; a window drawn without the compositor (OpenGL, `lbterm_compositor = false`) presents
//! no swapchain here and so is timed to its present only.
//!
//! All of it only while lbterm_extras::present is on (a test window's environment, or a metrics
//! request): off, a frame hands over no swapchain, nothing is watched, looked at or walked, no
//! timer is armed, and whatever was under way is let go at the next frame.

use lbterm_extras::budget::{Action, Done};
use lbterm_extras::present::{
    self as p, Available, Episode, FrameStats, OnScreen, Qpc, Sample, Seen, Walk,
};
use lbterm_motion::Target;
use mux::pane::PaneId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows58::core::Interface;
use windows58::Win32::Graphics::DirectComposition::{
    COMPOSITION_FRAME_STATS, COMPOSITION_TARGET_ID, COMPOSITION_TARGET_STATS,
};
use windows58::Win32::Graphics::Dxgi::{IDXGISwapChain, IDXGISwapChain3, DXGI_FRAME_STATISTICS};

/// The swapchain a surface presents into (wgpu-hal's D3D12 surface), for its frame statistics;
/// none while the timing is off, which costs a frame nothing (no lock on wgpu-hal's swapchain,
/// no reference taken)
pub fn chain(surface: &wgpu::Surface<'static>) -> Option<IDXGISwapChain3> {
    if !p::on() {
        return None;
    }
    unsafe { surface.as_hal::<wgpu::hal::api::Dx12, _, _>(|s| s.and_then(|s| s.swap_chain())) }
}

/// The most presents watched on one swapchain at once: past it the oldest is given up on (lost).
/// A swapchain whose statistics never answer (a window nobody can see, DXGI saying DISJOINT) would
/// otherwise keep every frame's watch for [`p::LOST_MS`]
const WATCHED_A_CHAIN: usize = 4;

/// The compositor's history goes back about this many of its frames (256 on 2026-10-30); a walk
/// further behind than this starts again here
const HISTORY: u64 = 240;
/// How often a long episode is walked while it plays, so the history doesn't run out under it
const WALK_EVERY: Duration = Duration::from_millis(800);

/// A frame's swapchains as presented: which visual each is
struct FrameSeen {
    paint: Instant,
    presented: Option<Instant>,
    chains: Vec<(Target<PaneId>, IDXGISwapChain3)>,
}

/// A present watched until it's on screen, and the actions it shows. A visual's swapchain is a
/// composition swapchain (wgpu-hal's for a DirectComposition visual), whose GetFrameStatistics
/// says DXGI_ERROR_FRAME_STATISTICS_DISJOINT at first and then the last present DWM put on
/// screen, all zeros while none has been (a window nobody can see); IDXGISwapChainMedia, the
/// media swapchains' statistics, it hasn't (E_NOINTERFACE, 2026-10-30)
struct Watch {
    chain: IDXGISwapChain,
    id: u32,
    presented: Instant,
    refresh: Qpc,
    actions: Vec<(Action, Instant)>,
}

/// An episode of motion in a window
struct Playing {
    target: Option<COMPOSITION_TARGET_ID>,
    episode: Episode,
    opened: Instant,
    walked: Instant,
}

/// DirectComposition's compositor clock statistics (dcomp.dll, Windows 11), found by name so an
/// LBTerm without them still starts
#[derive(Clone, Copy)]
struct Clock {
    frame_id: unsafe extern "system" fn(i32, *mut u64) -> i32,
    statistics: unsafe extern "system" fn(u64, *mut COMPOSITION_FRAME_STATS, u32, *mut COMPOSITION_TARGET_ID, *mut u32) -> i32,
    target: unsafe extern "system" fn(u64, *const COMPOSITION_TARGET_ID, *mut COMPOSITION_TARGET_STATS) -> i32,
}

/// COMPOSITION_FRAME_ID_COMPLETED: the newest frame whose statistics are final
const FRAME_COMPLETED: i32 = 2;

impl Clock {
    fn load() -> Option<Self> {
        use windows58::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        unsafe {
            let module = LoadLibraryW(windows58::core::w!("dcomp.dll")).ok()?;
            let frame_id = GetProcAddress(module, windows58::core::s!("DCompositionGetFrameId"))?;
            let statistics = GetProcAddress(module, windows58::core::s!("DCompositionGetStatistics"))?;
            let target = GetProcAddress(module, windows58::core::s!("DCompositionGetTargetStatistics"))?;
            Some(Self {
                frame_id: std::mem::transmute(frame_id),
                statistics: std::mem::transmute(statistics),
                target: std::mem::transmute(target),
            })
        }
    }

    fn latest(&self) -> Option<u64> {
        let mut id = 0u64;
        (unsafe { (self.frame_id)(FRAME_COMPLETED, &mut id) } >= 0).then_some(id)
    }

    /// The screens the compositor presents to, as of frame `id`
    fn targets(&self, id: u64) -> Vec<COMPOSITION_TARGET_ID> {
        let mut stats = COMPOSITION_FRAME_STATS::default();
        let mut ids = [COMPOSITION_TARGET_ID::default(); 16];
        let mut n = 0u32;
        let hr = unsafe { (self.statistics)(id, &mut stats, ids.len() as u32, ids.as_mut_ptr(), &mut n) };
        if hr < 0 {
            return vec![];
        }
        ids[..(n as usize).min(ids.len())].to_vec()
    }

    /// Frame `id` as `target` saw it, and one refresh of that screen
    fn sample(&self, id: u64, target: &COMPOSITION_TARGET_ID) -> Option<(Sample, Qpc)> {
        let mut stats = COMPOSITION_FRAME_STATS::default();
        let hr = unsafe { (self.statistics)(id, &mut stats, 0, std::ptr::null_mut(), std::ptr::null_mut()) };
        if hr < 0 {
            return None;
        }
        let mut t = COMPOSITION_TARGET_STATS::default();
        if unsafe { (self.target)(id, target, &mut t) } < 0 {
            return None;
        }
        // Every frame's raw counts, for tracing what a walk took them as (PITFALLS 166): off
        // unless the window's log asks for this target at trace
        if log::log_enabled!(target: "lbterm::present::walk", log::Level::Trace) {
            let (ps, cs) = (&t.presentedStats, &t.completedStats);
            log::trace!(
                target: "lbterm::present::walk",
                "frame {id} start {} target {} period {}: outstanding {} present_time {} vblank {}; presented {} {} {} {}; completed {} {} {} {}",
                stats.startTime,
                stats.targetTime,
                stats.framePeriod,
                t.outstandingPresents,
                t.presentTime,
                t.vblankDuration,
                ps.presentCount,
                ps.refreshCount,
                ps.virtualRefreshCount,
                ps.time,
                cs.presentCount,
                cs.refreshCount,
                cs.virtualRefreshCount,
                cs.time,
            );
        }
        Some((
            Sample {
                id,
                start: stats.startTime as Qpc,
                presents: t.presentedStats.presentCount,
                refreshes: t.presentedStats.refreshCount,
                present_time: t.presentedStats.time as Qpc,
            },
            t.vblankDuration as Qpc,
        ))
    }
}

#[derive(Default)]
struct Present {
    started: bool,
    frequency: i64,
    clock: Option<Clock>,
    frames: HashMap<usize, FrameSeen>,
    watches: Vec<Watch>,
    playing: HashMap<usize, Playing>,
    /// Each screen's compositor target and one refresh of it, by monitor
    targets: HashMap<isize, Option<(COMPOSITION_TARGET_ID, Qpc)>>,
    /// Whether each window was hidden, and when that was looked at
    hidden: HashMap<u64, (Instant, bool)>,
    wake_at: Option<Instant>,
    /// The timer armed last: an earlier one that fires after it was replaced does nothing, so
    /// only one chain of them re-arms
    timer: u64,
    available: Available,
}

thread_local! {
    static PRESENT: RefCell<Present> = RefCell::new(Present::default());
}

fn qpc() -> Qpc {
    let mut q = 0i64;
    let _ = unsafe { windows58::Win32::System::Performance::QueryPerformanceCounter(&mut q) };
    q
}

/// A QPC time and the Instant it is, read together, to put one in terms of the other
#[derive(Clone, Copy)]
struct Now {
    qpc: Qpc,
    at: Instant,
    frequency: i64,
}

impl Now {
    fn read(frequency: i64) -> Self {
        Self {
            qpc: qpc(),
            at: Instant::now(),
            frequency,
        }
    }

    fn ticks(&self, d: Duration) -> Qpc {
        (d.as_nanos() as i128 * self.frequency as i128 / 1_000_000_000) as Qpc
    }

    fn qpc_of(&self, t: Instant) -> Qpc {
        if t >= self.at {
            self.qpc + self.ticks(t - self.at)
        } else {
            self.qpc - self.ticks(self.at - t)
        }
    }

    fn instant_of(&self, q: Qpc) -> Instant {
        let ns = |ticks: Qpc| Duration::from_nanos((ticks as i128 * 1_000_000_000 / self.frequency.max(1) as i128) as u64);
        if q >= self.qpc {
            self.at + ns(q - self.qpc)
        } else {
            self.at.checked_sub(ns(self.qpc - q)).unwrap_or(self.at)
        }
    }
}

fn unix_ms(t: Instant, now: Instant) -> f64 {
    let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() * 1000.0;
    if t <= now {
        wall - (now - t).as_secs_f64() * 1000.0
    } else {
        wall + (t - now).as_secs_f64() * 1000.0
    }
}

fn with<R>(f: impl FnOnce(&mut Present) -> R) -> Option<R> {
    PRESENT
        .try_with(|p| {
            let mut p = p.try_borrow_mut().ok()?;
            if !p.started {
                p.started = true;
                let mut f = 0i64;
                let _ = unsafe { windows58::Win32::System::Performance::QueryPerformanceFrequency(&mut f) };
                p.frequency = f;
                p.clock = Clock::load();
                p.available.compositor_clock = p.clock.is_some();
                p::set_available(p.available);
                // This file's timer and task are the measuring's own, not counted at rest
                lbterm_extras::budget::own(file!());
            }
            Some(f(&mut p))
        })
        .ok()
        .flatten()
}

/// A frame of `window` starts painting: what's watched is looked at
pub fn frame_start(window: usize, now: Instant) {
    with(|p| {
        p.frames.insert(
            window,
            FrameSeen {
                paint: now,
                presented: None,
                chains: vec![],
            },
        );
        p.poll();
    });
}

/// The timing is off: what was under way, let go (one check a frame once it's gone)
pub fn off() {
    let _ = PRESENT.try_with(|p| {
        if let Ok(mut p) = p.try_borrow_mut() {
            if !p.watches.is_empty() || !p.playing.is_empty() || !p.frames.is_empty() {
                p.watches.clear();
                p.playing.clear();
                p.frames.clear();
                p.hidden.clear();
                p.wake_at = None;
                p.timer += 1;
            }
        }
    });
}

/// `window`'s frame has presented these visuals' swapchains (render/composited.rs), and
/// committed
pub fn presented(window: usize, chains: Vec<(Target<PaneId>, IDXGISwapChain3)>) {
    if !p::on() {
        return;
    }
    let now = Instant::now();
    with(|p| {
        if let Some(f) = p.frames.get_mut(&window) {
            f.presented = Some(now);
            f.chains = chains;
        }
    });
}

/// `window`'s frame has ended (termwindow/budget.rs): the actions it showed, each with the pane
/// whose output a key waited on, are watched to the screen, and motion in flight
/// (`playing`, the compositor's animations: name, start, end) opens or carries on an episode
pub fn frame_end(
    window: usize,
    hwnd: u64,
    shown: &[(Done, Option<PaneId>)],
    playing: Vec<(&'static str, Instant, Instant)>,
    now: Instant,
) {
    with(|p| {
        let frame = p.frames.remove(&window);
        if let Some(frame) = &frame {
            p.watch(hwnd, frame, shown);
        }
        p.motion(window, hwnd, frame.as_ref().map(|f| f.paint), playing, now);
        p.arm(now);
    });
}

/// The window has gone
pub fn forget(window: usize) {
    if !p::on() {
        return;
    }
    with(|p| {
        p.frames.remove(&window);
        if let Some(playing) = p.playing.remove(&window) {
            p.finish(playing);
        }
    });
}

fn wake(timer: u64) {
    if !p::on() {
        off();
        return;
    }
    let now = Instant::now();
    with(|p| {
        if timer != p.timer {
            return;
        }
        p.wake_at = None;
        p.poll();
        p.walk_all(now);
        p.arm(now);
    });
}

/// How long whether a window is hidden is taken as known
const HIDDEN_FOR: Duration = Duration::from_millis(250);

impl Present {
    /// Whether others hide `hwnd` ([`covered`]), looked at again once [`HIDDEN_FOR`] has gone by
    fn hidden(&mut self, hwnd: u64, now: Instant) -> bool {
        if let Some((at, hidden)) = self.hidden.get(&hwnd) {
            if now.saturating_duration_since(*at) < HIDDEN_FOR {
                return *hidden;
            }
        }
        let began = Instant::now();
        let hidden = covered(hwnd);
        let ns = began.elapsed().as_nanos() as u64;
        p::with_totals(|t| t.looked_over(ns));
        self.hidden.insert(hwnd, (now, hidden));
        hidden
    }

    fn watch(&mut self, hwnd: u64, frame: &FrameSeen, shown: &[(Done, Option<PaneId>)]) {
        let Some(presented) = frame.presented else {
            return;
        };
        if shown.is_empty() {
            return;
        }
        // Each one's times to its frame's paint and its presents. A frame that presented no
        // visual (only a commit: the rail opening, which DWM plays) has nothing to watch
        let hidden = !frame.chains.is_empty() && self.hidden(hwnd, presented);
        p::with_totals(|t| {
            for (done, _) in shown {
                let since = |at: Instant| at.saturating_duration_since(done.start).as_nanos() as u64;
                t.shown(OnScreen {
                    action: done.action,
                    paint_ns: since(frame.paint),
                    present_ns: since(presented),
                    screen_ns: None,
                    screen_unix_ms: None,
                });
                // A window others hide shows its frames to nobody, so none goes up: not watched
                if hidden {
                    t.hidden(done.action);
                }
            }
        });
        let Some(last) = frame.chains.last().filter(|_| !hidden) else {
            return;
        };
        let refresh = self.screen(hwnd).map_or(0, |(_, r)| r);
        let mut watches: Vec<Watch> = vec![];
        for (done, pane) in shown {
            let chain = pane
                .and_then(|pane| frame.chains.iter().find(|(t, _)| *t == Target::Pane(pane)))
                .unwrap_or(last);
            let raw = chain.1.as_raw();
            if let Some(w) = watches.iter_mut().find(|w| w.chain.as_raw() == raw) {
                w.actions.push((done.action, done.start));
                continue;
            }
            let Ok(base) = chain.1.cast::<IDXGISwapChain>() else {
                continue;
            };
            let Ok(id) = (unsafe { base.GetLastPresentCount() }) else {
                continue;
            };
            watches.push(Watch {
                chain: base,
                id,
                presented,
                refresh,
                actions: vec![(done.action, done.start)],
            });
        }
        // At most WATCHED_A_CHAIN a swapchain, the oldest given up on
        for w in watches {
            let raw = w.chain.as_raw();
            let on_it = self.watches.iter().filter(|x| x.chain.as_raw() == raw).count();
            if on_it >= WATCHED_A_CHAIN {
                if let Some(i) = self.watches.iter().position(|x| x.chain.as_raw() == raw) {
                    let old = self.watches.remove(i);
                    p::with_totals(|t| old.actions.iter().for_each(|(a, _)| t.lost(*a)));
                }
            }
            self.watches.push(w);
        }
    }

    /// Read each watched swapchain's frame statistics: what's on screen is timed, what's
    /// overtaken or too long gone is counted
    fn poll(&mut self) {
        if self.watches.is_empty() {
            return;
        }
        let began = Instant::now();
        let now = Now::read(self.frequency);
        let mut polls = 0u64;
        let mut kept = vec![];
        let mut on_screen = vec![];
        let mut overtaken = vec![];
        let mut lost = vec![];
        // One look at each swapchain, whatever's watched on it
        let mut looked: Vec<(*mut std::ffi::c_void, Option<FrameStats>)> = vec![];
        for w in std::mem::take(&mut self.watches) {
            let raw = w.chain.as_raw();
            let stats = match looked.iter().find(|(r, _)| *r == raw) {
                Some((_, stats)) => *stats,
                None => {
                    let mut stats = DXGI_FRAME_STATISTICS::default();
                    polls += 1;
                    let answer = unsafe { w.chain.GetFrameStatistics(&mut stats) };
                    if log::log_enabled!(target: "lbterm::present::stats", log::Level::Trace) {
                        log::trace!(
                            target: "lbterm::present::stats",
                            "chain {raw:?} watched present {} ({} ms after it): {}",
                            w.id,
                            now.at.saturating_duration_since(w.presented).as_millis(),
                            match &answer {
                                Ok(()) => format!(
                                    "present {} at refresh {}, sync refresh {}",
                                    stats.PresentCount, stats.PresentRefreshCount, stats.SyncRefreshCount
                                ),
                                Err(err) => format!("{:#010x}", err.code().0 as u32),
                            }
                        );
                    }
                    let read = match answer {
                        Ok(()) => {
                            self.available.frame_statistics = true;
                            Some(FrameStats {
                                present_count: stats.PresentCount,
                                present_refresh_count: stats.PresentRefreshCount,
                                sync_refresh_count: stats.SyncRefreshCount,
                                sync_qpc: stats.SyncQPCTime,
                            })
                        }
                        // DXGI_ERROR_FRAME_STATISTICS_DISJOINT and the like: not yet. What it
                        // was is kept for the reply
                        Err(err) => {
                            self.available.frame_statistics_error = Some(err.code().0);
                            None
                        }
                    };
                    looked.push((raw, read));
                    read
                }
            };
            let seen = match stats {
                Some(stats) => p::seen(stats, w.id, w.refresh),
                None => Seen::Later,
            };
            match seen {
                Seen::At(q) => {
                    let screen = now.instant_of(q).max(w.presented);
                    for (action, start) in &w.actions {
                        let since = |t: Instant| t.saturating_duration_since(*start);
                        on_screen.push((
                            *action,
                            since(w.presented).as_nanos() as u64,
                            since(screen).as_nanos() as u64,
                            unix_ms(screen, now.at),
                        ));
                    }
                }
                Seen::Overtaken(_) => overtaken.extend(w.actions.iter().map(|(a, _)| *a)),
                Seen::Later if now.at.saturating_duration_since(w.presented) > Duration::from_millis(p::LOST_MS) => {
                    lost.extend(w.actions.iter().map(|(a, _)| *a))
                }
                Seen::Later => kept.push(w),
            }
        }
        self.watches = kept;
        let cost = began.elapsed().as_nanos() as u64;
        p::set_available(self.available);
        p::with_totals(|t| {
            for (action, present_ns, screen_ns, unix) in on_screen {
                t.on_screen(action, present_ns, screen_ns, unix);
            }
            for a in overtaken {
                t.overtaken(a);
            }
            for a in lost {
                t.lost(a);
            }
            for _ in 0..polls {
                t.polled(cost / polls.max(1));
            }
        });
    }

    /// The compositor target of the screen `hwnd` is on and one refresh of it: its display
    /// adapter and source, as D3DKMTOpenAdapterFromGdiDisplayName gives them for the monitor's
    /// device name, looked up once a monitor
    fn screen(&mut self, hwnd: u64) -> Option<(COMPOSITION_TARGET_ID, Qpc)> {
        let clock = self.clock?;
        let monitor = unsafe {
            winapi::um::winuser::MonitorFromWindow(hwnd as _, winapi::um::winuser::MONITOR_DEFAULTTONEAREST)
        } as isize;
        if let Some(known) = self.targets.get(&monitor) {
            return *known;
        }
        let id = clock.latest()?;
        if log::log_enabled!(target: "lbterm::present::walk", log::Level::Trace) {
            for t in clock.targets(id) {
                log::trace!(
                    target: "lbterm::present::walk",
                    "target: display adapter {:#x}:{:#x}, render adapter {:#x}:{:#x}, source {}, target {}, unique {}; monitor {monitor:#x} is {:?}",
                    t.displayAdapterLuid.HighPart,
                    t.displayAdapterLuid.LowPart,
                    t.renderAdapterLuid.HighPart,
                    t.renderAdapterLuid.LowPart,
                    t.vidPnSourceId,
                    t.vidPnTargetId,
                    t.uniqueId,
                    source_of(monitor),
                );
            }
        }
        let found = source_of(monitor)
            .and_then(|(luid_low, luid_high, source)| {
                clock.targets(id).into_iter().find(|t| {
                    t.displayAdapterLuid.LowPart == luid_low
                        && t.displayAdapterLuid.HighPart == luid_high
                        && t.vidPnSourceId == source
                })
            })
            .and_then(|t| clock.sample(id, &t).map(|(_, refresh)| (t, refresh)));
        if found.is_none() {
            log::debug!(target: "lbterm::present", "no compositor target for monitor {monitor:#x}: motion there isn't measured");
        }
        self.targets.insert(monitor, found);
        found
    }

    /// Motion in `window`: an episode opened by the frame that starts it, its paints counted
    /// while it plays
    fn motion(
        &mut self,
        window: usize,
        hwnd: u64,
        paint: Option<Instant>,
        playing: Vec<(&'static str, Instant, Instant)>,
        now: Instant,
    ) {
        let live: Vec<_> = playing.into_iter().filter(|(_, _, end)| *end > now).collect();
        let clock_now = Now::read(self.frequency);
        if let Some(open) = self.playing.get_mut(&window) {
            if let Some(paint) = paint.filter(|p| *p >= open.opened) {
                open.episode.paint(clock_now.ticks(now.saturating_duration_since(paint)));
            }
            if let Some(settles) = live.iter().map(|(_, _, end)| *end).max() {
                open.episode
                    .extend(live.iter().map(|(n, _, _)| *n), clock_now.qpc_of(settles));
            }
            return;
        }
        let Some(settles) = live.iter().map(|(_, _, end)| *end).max() else {
            return;
        };
        let names: Vec<&'static str> = {
            let mut names = vec![];
            for (n, _, _) in &live {
                if !names.contains(n) {
                    names.push(*n);
                }
            }
            names
        };
        let target = self.screen(hwnd).map(|(t, _)| t);
        // A window others hide shows its motion to nobody, so its screen gets no frame for it: not
        // walked
        let covered = self.hidden(hwnd, now);
        let base = match (self.clock, target) {
            (Some(c), Some(t)) if !covered => c.latest().and_then(|id| c.sample(id, &t)),
            _ => None,
        };
        let settles_qpc = clock_now.qpc_of(settles);
        let refresh = base.map_or(0, |(_, r)| r);
        // Its refreshes counted by the screen's vblank times, a refresh apart (lbterm-extras'
        // Walk, PITFALLS 166): no walk where the refresh isn't known
        let walk = base.and_then(|(s, refresh)| Walk::new(s, settles_qpc, refresh));
        log::trace!(
            target: "lbterm::present::walk",
            "episode {names:?} opened at {} settling at {}, base frame {:?}, covered {covered}",
            clock_now.qpc,
            settles_qpc,
            base.map(|(s, _)| s.id)
        );
        let mut episode = Episode::new(names, clock_now.qpc, settles_qpc, refresh, self.frequency, walk);
        episode.covered = covered;
        self.playing.insert(
            window,
            Playing {
                target,
                episode,
                opened: now,
                walked: now,
            },
        );
    }

    /// Walk every episode's frames so far, and finish those that have settled
    fn walk_all(&mut self, now: Instant) {
        let clock_now = Now::read(self.frequency);
        let windows: Vec<usize> = self.playing.keys().copied().collect();
        for window in windows {
            let settled = {
                let Some(playing) = self.playing.get_mut(&window) else {
                    continue;
                };
                let ended = clock_now.qpc >= playing.episode.settles;
                if ended || now.saturating_duration_since(playing.walked) >= WALK_EVERY {
                    walk(self.clock, playing, now);
                }
                playing.episode.finished(clock_now.qpc)
            };
            if settled {
                if let Some(playing) = self.playing.remove(&window) {
                    self.finish(playing);
                }
            }
        }
    }

    fn finish(&mut self, playing: Playing) {
        let done = playing.episode.done();
        log::debug!(target: "lbterm::present", "{}", done.line());
        p::with_totals(|t| t.motion(done));
    }

    /// The timer for what's watched or playing: the soonest of a watched present's next look
    /// (a refresh or two on), an episode's next look (`Episode::next_look`: its settle + 30 ms,
    /// then a refresh apart until its walk is done or its grace ends, never a time already
    /// past, PITFALLS 164) and a long one's next walk
    fn arm(&mut self, now: Instant) {
        let mut due: Option<Instant> = None;
        let mut soonest = |t: Instant| due = Some(due.map_or(t, |d| d.min(t)));
        if !self.watches.is_empty() {
            soonest(now + Duration::from_millis(20));
        }
        let frequency = self.frequency.max(1);
        let clock_now = Now::read(frequency);
        for playing in self.playing.values() {
            let look = clock_now.instant_of(playing.episode.next_look(clock_now.qpc));
            soonest(look.min(playing.walked + WALK_EVERY));
        }
        let Some(due) = due else {
            return;
        };
        if self.wake_at.map_or(true, |at| at <= now || due < at) {
            self.wake_at = Some(due);
            self.timer += 1;
            let timer = self.timer;
            let after = due.saturating_duration_since(now);
            promise::spawn::spawn_into_main_thread(async move {
                smol::Timer::after(after).await;
                wake(timer);
            })
            .detach();
        }
    }
}

/// Go through the compositor's frames since the episode's last walk, up to its end
fn walk(clock: Option<Clock>, playing: &mut Playing, now: Instant) {
    playing.walked = now;
    let (Some(clock), Some(target)) = (clock, playing.target) else {
        return;
    };
    let Some(walk) = playing.episode.walk.as_mut() else {
        return;
    };
    let Some(latest) = clock.latest() else {
        return;
    };
    let began = Instant::now();
    let mut from = walk.last_id + 1;
    if latest > walk.last_id + HISTORY {
        from = latest - HISTORY;
        if let Some((s, _)) = clock.sample(from, &target) {
            walk.skip_to(s);
            playing.episode.partial = true;
        }
        from += 1;
    }
    let mut frames = 0u64;
    for id in from..=latest {
        let Some((s, _)) = clock.sample(id, &target) else {
            continue;
        };
        frames += 1;
        if !walk.step(s) {
            break;
        }
    }
    let ns = began.elapsed().as_nanos() as u64;
    log::trace!(
        target: "lbterm::present::walk",
        "walked {from}..={latest}: {frames} frames, last {}, refreshes {} presents {} missed {}, done {}",
        walk.last_id,
        walk.refreshes,
        walk.presents,
        walk.missed,
        walk.done
    );
    p::with_totals(|t| t.walked(ns, frames));
}

/// Whether the windows over `hwnd` in the z-order hide it: together they cover at least
/// [`HIDDEN`] of the part of it on its screen (a window wider than its screen has a part on none),
/// sampled at 16 by 16 points. Each visible, not minimized and not cloaked (another desktop's, a
/// suspended app's), by DWM's frame bounds, so the invisible resize border doesn't count; found by
/// a walk up from it, one window at a time. Part of it in view is enough for DWM to present what
/// moves there
fn covered(hwnd: u64) -> bool {
    use winapi::shared::windef::{HWND, RECT};
    use winapi::um::winuser::{
        GetMonitorInfoW, GetWindow, GetWindowRect, IsIconic, IsWindowVisible, MonitorFromWindow,
        GW_HWNDPREV, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    fn bounds(h: HWND) -> Option<RECT> {
        let mut r: RECT = unsafe { std::mem::zeroed() };
        let hr = unsafe {
            DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32)
        };
        if hr >= 0 || unsafe { GetWindowRect(h, &mut r) } != 0 {
            Some(r)
        } else {
            None
        }
    }
    let me = hwnd as HWND;
    let Some(mut mine) = bounds(me) else {
        return false;
    };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(MonitorFromWindow(me, MONITOR_DEFAULTTONEAREST), &mut info) } != 0 {
        let m = info.rcMonitor;
        mine = RECT {
            left: mine.left.max(m.left),
            top: mine.top.max(m.top),
            right: mine.right.min(m.right),
            bottom: mine.bottom.min(m.bottom),
        };
        if mine.right <= mine.left || mine.bottom <= mine.top {
            return true;
        }
    }
    let mut over: Vec<RECT> = vec![];
    let mut h = unsafe { GetWindow(me, GW_HWNDPREV) };
    while !h.is_null() {
        let shown = unsafe { IsWindowVisible(h) != 0 && IsIconic(h) == 0 };
        let mut cloaked = 0u32;
        let hidden = unsafe {
            DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4) >= 0 && cloaked != 0
        };
        if shown && !hidden {
            if let Some(r) = bounds(h) {
                if r.left < mine.right && mine.left < r.right && r.top < mine.bottom && mine.top < r.bottom {
                    over.push(r);
                }
            }
        }
        h = unsafe { GetWindow(h, GW_HWNDPREV) };
    }
    if over.is_empty() {
        return false;
    }
    let (w, ht) = ((mine.right - mine.left).max(1), (mine.bottom - mine.top).max(1));
    let mut hit = 0;
    for i in 0..16 {
        for j in 0..16 {
            let x = mine.left + (w * (2 * i + 1)) / 32;
            let y = mine.top + (ht * (2 * j + 1)) / 32;
            if over.iter().any(|r| r.left <= x && x < r.right && r.top <= y && y < r.bottom) {
                hit += 1;
            }
        }
    }
    hit as f64 / 256.0 >= HIDDEN
}

/// How much of a window others must cover for it to count as hidden
const HIDDEN: f64 = 0.95;

const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;
const DWMWA_CLOAKED: u32 = 14;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmGetWindowAttribute(hwnd: winapi::shared::windef::HWND, attribute: u32, value: *mut std::ffi::c_void, size: u32) -> i32;
}

/// The display adapter and source of `monitor`'s screen (its LUID's parts and its VidPn source)
fn source_of(monitor: isize) -> Option<(u32, i32, u32)> {
    use winapi::um::winuser::{GetMonitorInfoW, MONITORINFO, MONITORINFOEXW};
    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if unsafe { GetMonitorInfoW(monitor as _, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) } == 0 {
        return None;
    }
    let mut open = OpenAdapter {
        device_name: [0; 32],
        adapter: 0,
        luid_low: 0,
        luid_high: 0,
        source: 0,
    };
    let n = info.szDevice.len().min(31);
    open.device_name[..n].copy_from_slice(&info.szDevice[..n]);
    if unsafe { D3DKMTOpenAdapterFromGdiDisplayName(&mut open) } < 0 {
        return None;
    }
    unsafe {
        D3DKMTCloseAdapter(&CloseAdapter { adapter: open.adapter });
    }
    Some((open.luid_low, open.luid_high, open.source))
}

/// D3DKMT_OPENADAPTERFROMGDIDISPLAYNAME
#[repr(C)]
struct OpenAdapter {
    device_name: [u16; 32],
    adapter: u32,
    luid_low: u32,
    luid_high: i32,
    source: u32,
}

/// D3DKMT_CLOSEADAPTER
#[repr(C)]
struct CloseAdapter {
    adapter: u32,
}

#[link(name = "gdi32")]
extern "system" {
    fn D3DKMTOpenAdapterFromGdiDisplayName(open: *mut OpenAdapter) -> i32;
    fn D3DKMTCloseAdapter(close: *const CloseAdapter) -> i32;
}
