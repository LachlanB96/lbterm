//! LBTerm: what's hard to time (lbterm/PERF.md, "What's hard to time"): when a frame that shows
//! an action reached the screen, and whether every refresh got a frame while LBTerm's motion
//! played. What's made of the numbers Windows gives, tested with them handed in, and the totals
//! `metrics` reports under `present`. Windows' side is wezterm-gui/src/present.rs.
//!
//! - **To the screen.** Each action the budget times (lbterm_extras::budget: a key's echo, a tab
//!   switch, a split, every frame's own paint ...) is shown by a frame, which ends with its
//!   visuals' presents. The GUI notes the present id of the swapchain that shows it
//!   (GetLastPresentCount; for a key, its pane's visual, else the frame's last present) and reads
//!   that swapchain's DXGI frame statistics (GetFrameStatistics) at the next frame's start, or a
//!   refresh or two later when nothing else paints, until they say that present is on screen:
//!   [`seen`] gives the refresh it went up at, as a QueryPerformanceCounter time. So each action
//!   has three times from what started it (the key or click message's arrival, slow.rs's
//!   `cause`): to its frame's paint, to its present, and to the screen.
//! - **Frames missed during motion.** While DWM plays LBTerm's animations (lbterm/COMPOSITOR.md),
//!   every refresh of the window's screen should bring a new frame. DirectComposition's
//!   compositor clock keeps, for each of its frames (one a tick of the fastest screen's refresh,
//!   the last 256 or so), each screen's count of refreshes and of frames DWM presented to it
//!   (DCompositionGetStatistics, DCompositionGetTargetStatistics). An [`Episode`] runs from the
//!   frame that starts motion to when the last of it settles; its [`Walk`] goes through the
//!   compositor's frames in that time and counts the refreshes that came without a present. A
//!   refresh is counted by the screen's vblank times (each frame's `presentedStats.time`, a
//!   refresh apart), not by its refresh counter, which on some screens here jumps at a present,
//!   thousands forward or tens back, while the times stay on their grid (lbterm/PITFALLS.md,
//!   166). The episode also counts the paints LBTerm made meanwhile, and those longer than a
//!   refresh.
//!
//! Off until asked ([`on`]): a few calls into Windows for each frame shown, a timer a refresh or
//! two on while a present is watched, a look at what's over the window and a walk an episode, on
//! the GUI thread, whose cost is in the reply (`cost`), are what a window measuring itself pays,
//! and Lachlan's window pays none of it until he asks. A test window turns it on with
//! [`ENV`] in its environment (the budget runner's actions, present.ps1); any LBTerm, with
//! `lbterm control metrics --present on`, and off again with `off`.

use crate::budget::Action;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

/// What turns it on as the GUI starts, set to anything
pub const ENV: &str = "LBTERM_PRESENT_TIMING";

static ON: AtomicBool = AtomicBool::new(false);

/// Whether frames are watched to the screen and motion walked: one load
#[inline]
pub fn on() -> bool {
    ON.load(Relaxed)
}

/// Watch them, or stop (what's being watched is let go at the next frame)
pub fn set_on(on: bool) {
    ON.store(on, Relaxed);
}

/// A QueryPerformanceCounter time, in its ticks
pub type Qpc = i64;

/// How long after its present a frame is given to reach the screen before it's given up on
pub const LOST_MS: u64 = 500;

/// How many misses an episode keeps the times of
pub const MISSES_KEPT: usize = 16;

/// How long after an episode settles it's first looked at, its last frames composed by then on
/// a screen that's composing ([`Episode::next_look`])
pub const SETTLE_LOOK_MS: i64 = 30;

/// After an episode settles, how long its last frames are given to be completed before it's
/// finished anyway ([`Episode::finished`])
pub const SETTLE_GRACE_MS: i64 = 200;

/// DXGI_FRAME_STATISTICS as GetFrameStatistics fills it: the last present on screen
/// (`present_count`, the swapchain's present id), the refresh it went up at
/// (`present_refresh_count`), and a refresh whose time is known (`sync_refresh_count` at
/// `sync_qpc`), usually the same one
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub present_count: u32,
    pub present_refresh_count: u32,
    pub sync_refresh_count: u32,
    pub sync_qpc: Qpc,
}

/// What a swapchain's statistics say of one of its presents
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// Not on screen yet
    Later,
    /// On screen from the refresh at this time
    At(Qpc),
    /// A later present is on screen already: this one was by this time, but when isn't known
    Overtaken(Qpc),
}

/// What `stats` say of present `id`, with one refresh `refresh` ticks long. Counts wrap.
pub fn seen(stats: FrameStats, id: u32, refresh: Qpc) -> Seen {
    let ahead = stats.present_count.wrapping_sub(id) as i32;
    if ahead < 0 {
        return Seen::Later;
    }
    let refreshes = stats
        .present_refresh_count
        .wrapping_sub(stats.sync_refresh_count) as i32;
    let at = stats.sync_qpc + refreshes as i64 * refresh;
    if ahead == 0 {
        Seen::At(at)
    } else {
        Seen::Overtaken(at)
    }
}

/// One action's frame: from what started the action to its frame's paint starting and to its
/// presents done, in nanoseconds, said as the frame ends; then, once it's seen going up, to the
/// refresh it went up at, and when that was in Unix milliseconds, so a test can time it from its
/// own clock
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnScreen {
    pub action: Action,
    pub paint_ns: u64,
    pub present_ns: u64,
    pub screen_ns: Option<u64>,
    pub screen_unix_ms: Option<f64>,
}

/// One compositor frame's statistics for the screen a window is on: its id and start, and the
/// screen's counts as it started (DCompositionGetTargetStatistics' `presentedStats`): frames DWM
/// had presented to it, its refresh counter, and the time of the last refresh DWM had seen
/// (`time`, which moves on a refresh at a time whether or not anything was presented; measured
/// 2026-11-08). The refresh counter isn't a count of refreshes on every screen: on two of this
/// PC's four it went back by 4 to 61 at a present after a quiet spell, and in the runs of
/// 2026-11-07 forward by thousands in a fade of 15 refreshes, while `time` kept a refresh apart
/// throughout (lbterm/PITFALLS.md, 166), so a walk counts refreshes by `present_time`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub id: u64,
    pub start: Qpc,
    pub presents: u32,
    pub refreshes: u32,
    pub present_time: Qpc,
}

/// The compositor's frames gone through for one episode of motion, in order, from the frame it
/// opened at (`base`): the first that has a present after it is where the motion went up, and
/// from there every refresh that came without a present is one missed. Frames that start after
/// `until` (when the motion settles) aren't its. Refreshes are counted by the screen's vblank
/// times, a `refresh` apart ([`Sample`]), never its refresh counter
#[derive(Debug, Clone, PartialEq)]
pub struct Walk {
    base: Sample,
    pub until: Qpc,
    /// One refresh of the screen, in QPC ticks (DCompositionGetTargetStatistics' `vblankDuration`)
    refresh: Qpc,
    first: Option<Sample>,
    prev: Option<Sample>,
    /// Refreshes and presents from the first present to the last frame walked, and the refreshes
    /// more than presents among them: a frame's counts can come a frame late, so what one frame
    /// seems short can be made up in the next, and only the whole span's say what was missed
    pub refreshes: u32,
    pub presents: u32,
    /// The same span's refreshes by the screen's own refresh counter, which isn't a count on
    /// every screen ([`Sample`]): kept beside `refreshes` so a walk says when the two disagree,
    /// and judged by nothing
    pub counted: u32,
    pub missed: u32,
    /// The most it has been short at any frame, for where the misses were
    worst: i64,
    /// When each miss was seen: the start of the frame that saw it, the first [`MISSES_KEPT`]
    pub misses: Vec<Qpc>,
    pub frames: u32,
    pub last_id: u64,
    /// A frame past `until` has been seen: nothing more is the motion's
    pub done: bool,
}

/// Refreshes from `a`'s last vblank to `b`'s, `refresh` apart, to the nearest (DWM's vblank times
/// stray from the grid by a fraction of a millisecond); none for times that go back
fn refreshes_between(a: &Sample, b: &Sample, refresh: Qpc) -> u32 {
    let d = (b.present_time - a.present_time).max(0);
    ((d + refresh / 2) / refresh) as u32
}

impl Walk {
    /// A walk from `base`, to `until`, on a screen whose refresh is `refresh` QPC ticks; none
    /// for a refresh that isn't known, since it can't count refreshes then
    pub fn new(base: Sample, until: Qpc, refresh: Qpc) -> Option<Self> {
        (refresh > 0).then(|| Self {
            base,
            until,
            refresh,
            first: None,
            prev: None,
            refreshes: 0,
            presents: 0,
            counted: 0,
            missed: 0,
            worst: 0,
            misses: vec![],
            frames: 0,
            last_id: base.id,
            done: false,
        })
    }

    /// The motion settles later than thought
    pub fn until(&mut self, until: Qpc) {
        self.until = self.until.max(until);
    }

    /// The next frame. A frame walked already is passed over; one past `until` ends the walk,
    /// and says so with false
    pub fn step(&mut self, s: Sample) -> bool {
        if self.done {
            return false;
        }
        if s.id <= self.last_id {
            return true;
        }
        self.last_id = s.id;
        if s.start > self.until {
            self.done = true;
            return false;
        }
        self.frames += 1;
        match self.prev {
            None => {
                if s.presents != self.base.presents {
                    self.first = Some(s);
                    self.prev = Some(s);
                }
            }
            Some(p) => {
                // By the vblank times, frame to frame, so frames the history lost (skip_to) stay
                // out as they do for the presents
                self.refreshes += refreshes_between(&p, &s, self.refresh);
                self.presents = self.presents.wrapping_add(s.presents.wrapping_sub(p.presents));
                self.counted = self.counted.wrapping_add(s.refreshes.wrapping_sub(p.refreshes));
                let short = self.refreshes as i64 - self.presents as i64;
                self.missed = short.max(0) as u32;
                if short > self.worst {
                    self.worst = short;
                    if self.misses.len() < MISSES_KEPT {
                        self.misses.push(s.start);
                    }
                }
                self.prev = Some(s);
            }
        }
        true
    }

    /// The frames before `s` are gone from the compositor's history: carry on from `s`, as the
    /// last frame walked, or, before the first present, the frame to look for it after
    pub fn skip_to(&mut self, s: Sample) {
        if self.done || s.id <= self.last_id {
            return;
        }
        self.last_id = s.id;
        if self.prev.is_some() {
            self.prev = Some(s);
        } else {
            self.base = s;
        }
    }

    /// When the motion's first frame went up
    pub fn first_present(&self) -> Option<Qpc> {
        self.first.map(|f| f.present_time)
    }

    /// Refreshes between the frame it opened at and the first present: how long the motion's
    /// first frame took to go up, in refreshes (by the vblank times, as `refreshes`)
    pub fn lead(&self) -> Option<u32> {
        self.first.map(|f| refreshes_between(&self.base, &f, self.refresh))
    }
}

/// One episode of motion in a window: from the frame that starts it until the last of it
/// settles, what names played, the paints LBTerm made meanwhile, and its walk (None when the
/// compositor's statistics for its screen couldn't be read)
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub names: Vec<&'static str>,
    /// When it opened (the end of the frame that started it, just after its commit), as a QPC
    /// time
    pub opened: Qpc,
    /// When the last of it settles, as a QPC time
    pub settles: Qpc,
    /// One refresh of its screen, in QPC ticks (0 unknown)
    pub refresh: Qpc,
    pub frequency: i64,
    pub paints: u32,
    pub slow_paints: u32,
    pub longest_paint: Qpc,
    pub walk: Option<Walk>,
    /// The compositor's history no longer reached back to where the walk had got to: frames
    /// were passed over
    pub partial: bool,
    /// Another window was over the window as it opened: its motion went to nobody, so the screen
    /// got no frames for it, and it isn't walked
    pub covered: bool,
}

impl Episode {
    pub fn new(names: Vec<&'static str>, opened: Qpc, settles: Qpc, refresh: Qpc, frequency: i64, walk: Option<Walk>) -> Self {
        Self {
            names,
            opened,
            settles,
            refresh,
            frequency,
            paints: 0,
            slow_paints: 0,
            longest_paint: 0,
            walk,
            partial: false,
            covered: false,
        }
    }

    /// More plays, or what plays settles later
    pub fn extend(&mut self, names: impl IntoIterator<Item = &'static str>, settles: Qpc) {
        for name in names {
            if !self.names.contains(&name) {
                self.names.push(name);
            }
        }
        if settles > self.settles {
            self.settles = settles;
            if let Some(walk) = self.walk.as_mut() {
                walk.until(settles);
            }
        }
    }

    /// A paint LBTerm made while it played, `ticks` long
    pub fn paint(&mut self, ticks: Qpc) {
        self.paints += 1;
        self.longest_paint = self.longest_paint.max(ticks);
        if self.refresh > 0 && ticks > self.refresh {
            self.slow_paints += 1;
        }
    }

    fn ticks_ms(&self, ms: i64) -> Qpc {
        ms * self.frequency / 1000
    }

    /// Whether it's over at `now` (a QPC time): settled, and its walk has stepped a compositor
    /// frame begun after the settle (or it has no walk), or [`SETTLE_GRACE_MS`] after the
    /// settle whatever
    pub fn finished(&self, now: Qpc) -> bool {
        let done = self.walk.as_ref().map_or(true, |w| w.done);
        now >= self.settles && (done || now >= self.settles + self.ticks_ms(SETTLE_GRACE_MS))
    }

    /// When it's next looked at, from `now` (a QPC time): its settle plus [`SETTLE_LOOK_MS`]
    /// while that's to come. Past it, with the walk not done, nothing can change but the
    /// compositor completing another frame, so a refresh on, the soonest one can (with the
    /// refresh unknown, the grace's end), and the grace's end at the latest, when
    /// [`finished`](Self::finished) says it's over whatever. Always after `now`: a timer armed
    /// for a time already past runs at once, finds nothing changed and is armed for the same
    /// time again, a spin through the GUI's queue until a frame comes or the grace ends, which
    /// on a locked PC was up to 58,000 turns and 148 ms of the GUI thread (PITFALLS 164)
    pub fn next_look(&self, now: Qpc) -> Qpc {
        let first = self.settles + self.ticks_ms(SETTLE_LOOK_MS);
        if first > now {
            return first;
        }
        let grace = self.settles + self.ticks_ms(SETTLE_GRACE_MS);
        let next = if self.refresh > 0 {
            (now + self.refresh).min(grace)
        } else {
            grace
        };
        next.max(now + 1)
    }

    fn ms(&self, ticks: Qpc) -> f64 {
        if self.frequency > 0 {
            ticks as f64 * 1000.0 / self.frequency as f64
        } else {
            0.0
        }
    }

    /// What it came to, once it has settled
    pub fn done(&self) -> MotionDone {
        let walk = self.walk.as_ref();
        let first = walk.and_then(|w| w.first_present());
        MotionDone {
            names: self.names.clone(),
            ms: self.ms(self.settles - self.opened),
            refresh_ms: self.ms(self.refresh),
            measured: first.is_some(),
            partial: self.partial,
            covered: self.covered,
            refreshes: walk.map_or(0, |w| w.refreshes),
            counted: walk.map_or(0, |w| w.counted),
            presents: walk.map_or(0, |w| w.presents),
            missed: walk.map_or(0, |w| w.missed),
            misses_ms: walk.map_or(vec![], |w| w.misses.iter().map(|&t| self.ms(t - self.opened)).collect()),
            first_ms: first.map(|t| self.ms(t - self.opened)),
            lead: walk.and_then(|w| w.lead()),
            paints: self.paints,
            slow_paints: self.slow_paints,
            longest_paint_ms: self.ms(self.longest_paint),
        }
    }
}

/// An episode of motion once it has settled
#[derive(Debug, Clone, PartialEq)]
pub struct MotionDone {
    pub names: Vec<&'static str>,
    /// From the frame that started it to its settling
    pub ms: f64,
    pub refresh_ms: f64,
    /// Whether a present of the motion's was seen at all (none: the compositor's statistics
    /// couldn't be read, or nothing went up, the window hidden or the screen locked)
    pub measured: bool,
    pub partial: bool,
    pub covered: bool,
    pub refreshes: u32,
    /// The screen's refresh counter over the same span ([`Walk`]'s `counted`), judged by nothing
    pub counted: u32,
    pub presents: u32,
    pub missed: u32,
    pub misses_ms: Vec<f64>,
    /// When its first frame went up, from the frame that started it
    pub first_ms: Option<f64>,
    /// ... in refreshes
    pub lead: Option<u32>,
    pub paints: u32,
    pub slow_paints: u32,
    pub longest_paint_ms: f64,
}

impl MotionDone {
    pub fn to_json(&self) -> Value {
        json!({
            "names": self.names,
            "ms": round(self.ms),
            "refresh_ms": round(self.refresh_ms),
            "measured": self.measured,
            "partial": self.partial,
            "covered": self.covered,
            "refreshes": self.refreshes,
            "counted": self.counted,
            "presents": self.presents,
            "missed": self.missed,
            "misses_ms": self.misses_ms.iter().map(|&m| round(m)).collect::<Vec<_>>(),
            "first_ms": self.first_ms.map(round),
            "lead": self.lead,
            "paints": self.paints,
            "slow_paints": self.slow_paints,
            "longest_paint_ms": round(self.longest_paint_ms),
        })
    }

    /// Its debug line (target lbterm::present)
    pub fn line(&self) -> String {
        let names = self.names.join(", ");
        if self.covered {
            return format!("motion {names}, {:.0} ms: not measured, another window was over it", self.ms);
        }
        if !self.measured {
            return format!("motion {names}, {:.0} ms: no present seen on its screen", self.ms);
        }
        let mut line = format!(
            "motion {names}, {:.0} ms: {} refreshes, {} missed, first frame up at {:.1} ms; {} paints, {} over a refresh ({:.1} ms), the longest {:.1} ms",
            self.ms,
            self.refreshes,
            self.missed,
            self.first_ms.unwrap_or(0.0),
            self.paints,
            self.slow_paints,
            self.refresh_ms,
            self.longest_paint_ms
        );
        if !self.misses_ms.is_empty() {
            let at: Vec<String> = self.misses_ms.iter().map(|m| format!("{m:.0}")).collect();
            line.push_str(&format!("; missed at {} ms", at.join(", ")));
        }
        if self.counted != self.refreshes {
            line.push_str(&format!("; the screen's refresh counter said {}", self.counted));
        }
        if self.partial {
            line.push_str("; frames passed over, the compositor's history ran out");
        }
        line
    }
}

fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// A sum, a count and the most
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Stat {
    n: u64,
    sum: f64,
    max: f64,
}

impl Stat {
    fn add(&mut self, v: f64) {
        self.n += 1;
        self.sum += v;
        self.max = self.max.max(v);
    }

    fn to_json(&self) -> Value {
        json!({
            "mean": if self.n > 0 { round(self.sum / self.n as f64) } else { 0.0 },
            "max": round(self.max),
        })
    }
}

#[derive(Debug, Clone, Default)]
struct ActionTotals {
    count: u64,
    seen: u64,
    overtaken: u64,
    lost: u64,
    hidden: u64,
    paint: Stat,
    present: Stat,
    screen: Stat,
    after_present: Stat,
    last: Option<OnScreen>,
}

#[derive(Debug, Clone, Default)]
struct NameTotals {
    episodes: u64,
    refreshes: u64,
    missed: u64,
    slow_paints: u64,
    longest_paint_ms: f64,
}

/// What `metrics` reports under `present`, since the process started or the last reset
#[derive(Debug, Clone, Default)]
pub struct Totals {
    actions: [ActionTotals; Action::COUNT],
    episodes: u64,
    measured: u64,
    partial: u64,
    covered: u64,
    refreshes: u64,
    counted: u64,
    presents: u64,
    missed: u64,
    with_misses: u64,
    paints: u64,
    slow_paints: u64,
    longest_paint_ms: f64,
    first: Stat,
    names: HashMap<&'static str, NameTotals>,
    last_motion: Option<MotionDone>,
    polls: u64,
    poll_ns: u64,
    walks: u64,
    walk_ns: u64,
    frames_walked: u64,
    covers: u64,
    cover_ns: u64,
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

impl Totals {
    /// An action's frame ended: its times to its paint and its presents
    pub fn shown(&mut self, s: OnScreen) {
        let a = self.action(s.action);
        a.count += 1;
        a.paint.add(ms(s.paint_ns));
        a.present.add(ms(s.present_ns));
        a.last = Some(s);
    }

    /// An action's frame was seen going up, `screen_ns` after what started it, `present_ns` after
    /// it its presents were done, at `screen_unix_ms`: the last one shown, when it's still waiting
    pub fn on_screen(&mut self, action: Action, present_ns: u64, screen_ns: u64, screen_unix_ms: f64) {
        let a = self.action(action);
        a.seen += 1;
        a.screen.add(ms(screen_ns));
        a.after_present.add(ms(screen_ns.saturating_sub(present_ns)));
        if let Some(last) = a.last.as_mut().filter(|l| l.screen_ns.is_none() && l.present_ns == present_ns) {
            last.screen_ns = Some(screen_ns);
            last.screen_unix_ms = Some(screen_unix_ms);
        }
    }

    /// An action's frame went up after a later one, so its time isn't known
    pub fn overtaken(&mut self, action: Action) {
        self.action(action).overtaken += 1;
    }

    /// An action's frame wasn't seen on screen within [`LOST_MS`]
    pub fn lost(&mut self, action: Action) {
        self.action(action).lost += 1;
    }

    /// An action's frame went to a window others hid, so to nobody: not watched
    pub fn hidden(&mut self, action: Action) {
        self.action(action).hidden += 1;
    }

    fn action(&mut self, action: Action) -> &mut ActionTotals {
        &mut self.actions[Action::ALL.iter().position(|x| *x == action).unwrap_or(0)]
    }

    pub fn motion(&mut self, m: MotionDone) {
        self.episodes += 1;
        if m.partial {
            self.partial += 1;
        }
        if m.covered {
            self.covered += 1;
        }
        self.paints += m.paints as u64;
        self.slow_paints += m.slow_paints as u64;
        self.longest_paint_ms = self.longest_paint_ms.max(m.longest_paint_ms);
        if m.measured {
            self.measured += 1;
            self.refreshes += m.refreshes as u64;
            self.counted += m.counted as u64;
            self.presents += m.presents as u64;
            self.missed += m.missed as u64;
            if m.missed > 0 {
                self.with_misses += 1;
            }
            if let Some(first) = m.first_ms {
                self.first.add(first);
            }
        }
        for name in &m.names {
            let n = self.names.entry(name).or_default();
            n.episodes += 1;
            n.slow_paints += m.slow_paints as u64;
            n.longest_paint_ms = n.longest_paint_ms.max(m.longest_paint_ms);
            if m.measured {
                n.refreshes += m.refreshes as u64;
                n.missed += m.missed as u64;
            }
        }
        self.last_motion = Some(m);
    }

    /// What reading a swapchain's statistics cost
    pub fn polled(&mut self, ns: u64) {
        self.polls += 1;
        self.poll_ns += ns;
    }

    /// What looking at whether a window is hidden cost (the windows over it, walked)
    pub fn looked_over(&mut self, ns: u64) {
        self.covers += 1;
        self.cover_ns += ns;
    }

    /// What walking the compositor's frames cost, and how many it went through
    pub fn walked(&mut self, ns: u64, frames: u64) {
        self.walks += 1;
        self.walk_ns += ns;
        self.frames_walked += frames;
    }

    pub fn to_json(&self, available: Available) -> Value {
        let actions: Vec<Value> = Action::ALL
            .iter()
            .zip(self.actions.iter())
            .filter(|(_, t)| t.count + t.overtaken + t.lost + t.hidden > 0)
            .map(|(a, t)| {
                json!({
                    "action": a.name(),
                    "count": t.count,
                    "seen": t.seen,
                    "overtaken": t.overtaken,
                    "lost": t.lost,
                    "hidden": t.hidden,
                    "paint_ms": t.paint.to_json(),
                    "present_ms": t.present.to_json(),
                    "screen_ms": t.screen.to_json(),
                    "after_present_ms": t.after_present.to_json(),
                    "last": t.last.map(|l| json!({
                        "paint_ms": round(ms(l.paint_ns)),
                        "present_ms": round(ms(l.present_ns)),
                        "screen_ms": l.screen_ns.map(|n| round(ms(n))),
                        "screen_unix_ms": l.screen_unix_ms.map(round),
                    })),
                })
            })
            .collect();
        let mut names: Vec<(&&'static str, &NameTotals)> = self.names.iter().collect();
        names.sort_by(|a, b| (b.1.episodes, a.0).cmp(&(a.1.episodes, b.0)));
        json!({
            "on": on(),
            "available": {
                "frame_statistics": available.frame_statistics,
                "frame_statistics_error": available.frame_statistics_error.map(|e| format!("{:#010x}", e as u32)),
                "compositor_clock": available.compositor_clock,
            },
            "actions": actions,
            "motion": {
                "episodes": self.episodes,
                "measured": self.measured,
                "partial": self.partial,
                "covered": self.covered,
                "refreshes": self.refreshes,
                "counted": self.counted,
                "presents": self.presents,
                "missed": self.missed,
                "with_misses": self.with_misses,
                "paints": self.paints,
                "slow_paints": self.slow_paints,
                "longest_paint_ms": round(self.longest_paint_ms),
                "first_ms": self.first.to_json(),
                "names": names.into_iter().map(|(name, n)| json!({
                    "name": name,
                    "episodes": n.episodes,
                    "refreshes": n.refreshes,
                    "missed": n.missed,
                    "slow_paints": n.slow_paints,
                    "longest_paint_ms": round(n.longest_paint_ms),
                })).collect::<Vec<_>>(),
                "last": self.last_motion.as_ref().map(|m| m.to_json()),
            },
            "cost": {
                "polls": self.polls,
                "poll_ms": round(ms(self.poll_ns)),
                "walks": self.walks,
                "walk_ms": round(ms(self.walk_ns)),
                "frames_walked": self.frames_walked,
                "covers": self.covers,
                "cover_ms": round(ms(self.cover_ns)),
            },
        })
    }
}

/// What Windows gives here: DXGI's frame statistics off a swapchain, and the compositor clock's
/// statistics (Windows 11)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Available {
    pub frame_statistics: bool,
    /// The last error reading them gave, an HRESULT
    pub frame_statistics_error: Option<i32>,
    pub compositor_clock: bool,
}

static TOTALS: OnceLock<Mutex<Totals>> = OnceLock::new();
static AVAILABLE: Mutex<Available> = Mutex::new(Available {
    frame_statistics: false,
    frame_statistics_error: None,
    compositor_clock: false,
});

/// The process's totals, which the GUI thread adds to as frames go up and episodes settle
pub fn with_totals<R>(f: impl FnOnce(&mut Totals) -> R) -> R {
    let mut t = TOTALS
        .get_or_init(|| Mutex::new(Totals::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    f(&mut t)
}

/// What the GUI found it can read
pub fn set_available(available: Available) {
    *AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()) = available;
}

/// What `metrics` replies with under `present`, since the process started or the last reset
pub fn report_json(reset: bool) -> Value {
    let available = *AVAILABLE.lock().unwrap_or_else(|e| e.into_inner());
    with_totals(|t| {
        let json = t.to_json(available);
        if reset {
            *t = Totals::default();
        }
        json
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(count: u32, present_refresh: u32, sync_refresh: u32, qpc: Qpc) -> FrameStats {
        FrameStats {
            present_count: count,
            present_refresh_count: present_refresh,
            sync_refresh_count: sync_refresh,
            sync_qpc: qpc,
        }
    }

    #[test]
    fn a_present_is_seen_at_its_refresh() {
        // Present 7 went up at refresh 100, whose time is known
        assert_eq!(seen(stats(7, 100, 100, 5_000), 7, 166_666), Seen::At(5_000));
        // The time known is a refresh after it went up
        assert_eq!(seen(stats(7, 100, 101, 5_000), 7, 1_000), Seen::At(4_000));
        // Not up yet
        assert_eq!(seen(stats(6, 100, 100, 5_000), 7, 1_000), Seen::Later);
        // A later one is up already
        assert_eq!(seen(stats(9, 102, 102, 5_000), 7, 1_000), Seen::Overtaken(5_000));
        // Counts wrap
        assert_eq!(seen(stats(1, 3, 3, 9), u32::MAX, 1), Seen::Overtaken(9));
        assert_eq!(seen(stats(u32::MAX, 3, 3, 9), 1, 1), Seen::Later);
    }

    /// A test screen's refresh, in ticks
    const R: Qpc = 100;

    /// A frame on a screen whose refresh counter and vblank times agree: its last vblank at
    /// `refreshes` refreshes of `R`
    fn sample(id: u64, start: Qpc, presents: u32, refreshes: u32) -> Sample {
        Sample {
            id,
            start,
            presents,
            refreshes,
            present_time: refreshes as Qpc * R,
        }
    }

    #[test]
    fn a_walk_counts_refreshes_without_a_present() {
        // The compositor ticks twice a refresh; the motion's first frame goes up two refreshes
        // after it opened, then one refresh in four comes without a present
        let base = sample(10, 1_000, 50, 200);
        let mut walk = Walk::new(base, 10_000, R).unwrap();
        let mut presents = 50;
        let mut refreshes = 200;
        let mut id = 10;
        let mut start = 1_000;
        // Two refreshes before the first present
        for _ in 0..4 {
            id += 1;
            start += 100;
            if id % 2 == 0 {
                refreshes += 1;
            }
            assert!(walk.step(sample(id, start, presents, refreshes)));
        }
        assert_eq!(walk.first_present(), None);
        // The first present
        id += 1;
        start += 100;
        refreshes += 1;
        presents += 1;
        assert!(walk.step(sample(id, start, presents, refreshes)));
        assert_eq!(walk.first_present(), Some(refreshes as Qpc * R));
        assert_eq!(walk.lead(), Some(3));
        // Eight refreshes, the fourth and eighth without a present
        for r in 1..=8 {
            id += 1;
            start += 100;
            assert!(walk.step(sample(id, start, presents, refreshes)));
            id += 1;
            start += 100;
            refreshes += 1;
            if r % 4 != 0 {
                presents += 1;
            }
            assert!(walk.step(sample(id, start, presents, refreshes)));
        }
        assert_eq!((walk.refreshes, walk.presents, walk.missed), (8, 6, 2));
        assert_eq!(walk.counted, 8);
        assert_eq!(walk.misses.len(), 2);
        // A present that comes a frame late makes up the frame before it
        let mut late = Walk::new(sample(1, 0, 0, 0), 10_000, R).unwrap();
        late.step(sample(2, 100, 1, 1));
        late.step(sample(3, 200, 1, 2));
        assert_eq!(late.missed, 1);
        late.step(sample(4, 300, 3, 3));
        assert_eq!((late.refreshes, late.presents, late.missed), (2, 2, 0));
        assert_eq!(late.misses.len(), 1);
        // A frame walked already is passed over
        assert!(walk.step(sample(id - 3, 0, 0, 0)));
        assert_eq!(walk.missed, 2);
        // One past `until` ends it, and nothing after counts
        assert!(!walk.step(sample(id + 1, 10_001, presents, refreshes + 5)));
        assert!(walk.done);
        assert!(!walk.step(sample(id + 2, 10_002, presents, refreshes + 9)));
        assert_eq!(walk.missed, 2);
    }

    #[test]
    fn a_walk_carries_on_past_frames_the_history_lost() {
        // Lost before the first present: it's looked for after where the walk picks up
        let mut walk = Walk::new(sample(1, 0, 10, 10), 100_000, R).unwrap();
        walk.skip_to(sample(50, 5_000, 12, 30));
        assert!(walk.step(sample(51, 5_100, 12, 31)));
        assert_eq!(walk.first_present(), None);
        assert!(walk.step(sample(52, 5_200, 13, 32)));
        assert_eq!(walk.lead(), Some(2));
        // Lost after it: the refreshes and presents skipped aren't counted, so not missed
        walk.skip_to(sample(90, 9_000, 40, 80));
        assert!(walk.step(sample(91, 9_100, 41, 81)));
        assert_eq!((walk.refreshes, walk.presents, walk.missed), (1, 1, 0));
        // Never backwards
        walk.skip_to(sample(60, 6_000, 0, 0));
        assert_eq!(walk.last_id, 91);
    }

    #[test]
    fn a_walk_counts_refreshes_by_the_screens_vblank_times_not_its_counter() {
        // This PC's right-most screen (DCompositionGetTargetStatistics, 2026-11-08, its counts
        // as read frame by frame; 60 Hz, 166,666 ticks a refresh): a present after a quiet spell,
        // at which its refresh counter goes back 39 while the vblank times keep a refresh apart
        let refresh = 166_666;
        let f = |id, start, presents, refreshes, present_time| Sample { id, start, presents, refreshes, present_time };
        let mut walk = Walk::new(f(2350034, 466401301682, 19751, 8432279, 466401278352), 466404091055, refresh).unwrap();
        for s in [
            f(2350035, 466402538073, 19752, 8432286, 466402445014),
            f(2350036, 466402721682, 19752, 8432247, 466402616807),
            f(2350037, 466402782238, 19752, 8432248, 466402783473),
            f(2350038, 466403391555, 19752, 8432251, 466403283471),
            f(2350039, 466403417589, 19752, 8432251, 466403283471),
            f(2350040, 466404091055, 19752, 8432255, 466403950135),
        ] {
            assert!(walk.step(s));
        }
        // Seven refreshes from the frame it opened at to the present, and nine after it, with
        // nothing presented to that screen since: nine missed by the times. By the counter the
        // nine were 31 fewer, a count below zero that wrapped to four billion
        assert_eq!(walk.lead(), Some(7));
        assert_eq!((walk.refreshes, walk.presents, walk.missed), (9, 0, 9));
        assert_eq!(walk.counted, 8432255u32.wrapping_sub(8432286));
        // A fade of 15 refreshes with 13 presents, its counter jumping 2,142 forward a frame after
        // its first present, as the 2026-11-07 runs' counts say it did (2,003 to 2,157 refreshes
        // against 12 to 14 presents in 250 ms): 15 refreshes and 2 missed, the counter's 2,157
        let mut walk = Walk::new(f(1, 0, 100, 1_000, 0), 3_000_000, refresh).unwrap();
        let (mut presents, mut counter) = (100, 1_000);
        for i in 1..=16u32 {
            if i == 1 || (i != 5 && i != 9) {
                presents += 1;
            }
            counter += if i == 2 { 2_143 } else { 1 };
            assert!(walk.step(f(i as u64 + 1, i as Qpc * refresh + 5_000, presents, counter, i as Qpc * refresh)));
        }
        assert_eq!((walk.refreshes, walk.presents, walk.missed), (15, 13, 2));
        assert_eq!(walk.counted, 2_157);
        let ep = Episode::new(vec!["menu.tab"], 0, 3_000_000, refresh, 10_000_000, Some(walk));
        let line = ep.done().line();
        assert!(line.contains("15 refreshes, 2 missed"), "{line}");
        assert!(line.ends_with("; the screen's refresh counter said 2157"), "{line}");
        assert_eq!(ep.done().to_json()["counted"], 2_157);
        // A walk can't count refreshes without the refresh's length
        assert!(Walk::new(f(1, 0, 0, 0, 0), 1_000, 0).is_none());
    }

    #[test]
    fn a_walk_that_sees_no_present_measured_nothing() {
        let base = sample(1, 0, 5, 5);
        let mut walk = Walk::new(base, 1_000, R).unwrap();
        for i in 2..10 {
            walk.step(sample(i, i as Qpc * 100, 5, 5 + i as u32));
        }
        assert_eq!(walk.first_present(), None);
        assert_eq!((walk.refreshes, walk.missed), (0, 0));
        let mut ep = Episode::new(vec!["pane.layout"], 0, 1_000, 100, 1_000, Some(walk));
        ep.paint(50);
        let done = ep.done();
        assert!(!done.measured);
        assert!(done.line().contains("no present seen"), "{}", done.line());
    }

    #[test]
    fn an_episode_counts_its_paints_and_what_it_came_to() {
        // 10 MHz, a 60 Hz screen
        let f = 10_000_000;
        let refresh = 166_667;
        let base = sample(1, 0, 0, 0);
        let mut walk = Walk::new(base, 3_000_000, refresh).unwrap();
        walk.step(Sample { id: 2, start: 170_000, presents: 0, refreshes: 1, present_time: 166_667 });
        walk.step(Sample { id: 3, start: 340_000, presents: 1, refreshes: 2, present_time: 333_334 });
        walk.step(Sample { id: 4, start: 510_000, presents: 2, refreshes: 3, present_time: 500_000 });
        walk.step(Sample { id: 5, start: 680_000, presents: 2, refreshes: 4, present_time: 666_667 });
        let mut ep = Episode::new(vec!["pane.layout"], 0, 2_000_000, refresh, f, Some(walk));
        ep.extend(["outline.move", "pane.layout"], 3_000_000);
        assert_eq!(ep.walk.as_ref().unwrap().until, 3_000_000);
        ep.paint(100_000);
        ep.paint(200_000);
        let done = ep.done();
        assert_eq!(done.names, vec!["pane.layout", "outline.move"]);
        assert!(done.measured);
        assert_eq!((done.refreshes, done.presents, done.missed), (2, 1, 1));
        assert_eq!(done.first_ms.map(|m| (m * 10.0).round() / 10.0), Some(33.3));
        assert_eq!(done.lead, Some(2));
        assert_eq!((done.paints, done.slow_paints), (2, 1));
        assert!((done.longest_paint_ms - 20.0).abs() < 1e-9);
        assert!((done.ms - 300.0).abs() < 1e-9);
        assert_eq!(done.misses_ms, vec![68.0]);
        let line = done.line();
        assert!(line.starts_with("motion pane.layout, outline.move, 300 ms: 2 refreshes, 1 missed"), "{line}");
        assert!(line.contains("missed at 68 ms"), "{line}");
    }

    #[test]
    fn an_episode_whose_last_frame_never_comes_is_looked_at_a_refresh_apart_to_its_grace() {
        // 10 MHz, a 60 Hz screen; it settles at 1 s, and the compositor completes no frame after
        // (a locked PC's lock screen, which DWM rarely composes)
        let f = 10_000_000;
        let refresh = 166_667;
        let settles = 10_000_000;
        let ep = Episode::new(vec!["menu.tab"], 0, settles, refresh, f, Walk::new(sample(1, 0, 0, 0), settles, R));
        // Before its settle and up to 30 ms after, the look is at the settle + 30 ms
        assert_eq!(ep.next_look(0), settles + 300_000);
        assert_eq!(ep.next_look(settles), settles + 300_000);
        assert!(!ep.finished(settles));
        // From there each look is a refresh on, never at or before now, to the grace's end
        let grace = settles + 2_000_000;
        let mut now = settles + 300_000;
        let mut looks = 0;
        while !ep.finished(now) {
            let next = ep.next_look(now);
            assert!(next > now && next <= grace, "{now} then {next}");
            now = next;
            looks += 1;
            assert!(looks < 100, "still looking at {now}");
        }
        // The 170 ms to the grace's end in 11 looks, where a look armed for the settle + 30 ms
        // itself ran again on every turn of the queue: 1,784 to 57,686 of them
        assert_eq!((looks, now), (11, grace));
    }

    #[test]
    fn an_episode_ends_at_the_frame_after_its_settle_and_looks_never_in_the_past() {
        let f = 10_000_000;
        let settles = 10_000_000;
        let mut walk = Walk::new(sample(1, 0, 0, 0), settles, R).unwrap();
        let mut ep = Episode::new(vec!["menu.tab"], 0, settles, 166_667, f, None);
        // Without a walk it's over as it settles
        assert!(!ep.finished(settles - 1));
        assert!(ep.finished(settles));
        // With one, once a frame begun after the settle is stepped
        walk.step(sample(2, settles - 100_000, 1, 1));
        ep.walk = Some(walk.clone());
        assert!(!ep.finished(settles + 400_000));
        walk.step(sample(3, settles + 50_000, 2, 2));
        ep.walk = Some(walk);
        assert!(ep.finished(settles + 400_000));
        // With the refresh unknown, the next look after the first is the grace's end
        ep.walk = Walk::new(sample(1, 0, 0, 0), settles, R);
        ep.refresh = 0;
        assert_eq!(ep.next_look(settles + 300_000), settles + 2_000_000);
        // Asked again past the grace (a frame's end, before a wake has finished it), the look is
        // the next tick, once, not a time gone
        assert_eq!(ep.next_look(settles + 2_500_000), settles + 2_500_001);
        assert!(ep.finished(settles + 2_500_000));
    }

    #[test]
    fn the_totals_say_each_action_and_each_motion() {
        let mut t = Totals::default();
        let key = |paint_ns, present_ns| OnScreen {
            action: Action::Key,
            paint_ns,
            present_ns,
            screen_ns: None,
            screen_unix_ms: None,
        };
        t.shown(key(2_000_000, 6_000_000));
        t.on_screen(Action::Key, 6_000_000, 20_000_000, 1_700_000_000_123.5);
        t.shown(key(4_000_000, 8_000_000));
        // Seen after the next was shown: counted, but the last stays the next's
        t.shown(key(3_000_000, 7_000_000));
        t.on_screen(Action::Key, 8_000_000, 30_000_000, 1_700_000_000_223.5);
        t.overtaken(Action::Paint);
        t.lost(Action::Tab);
        t.hidden(Action::Tab);
        t.motion(MotionDone {
            names: vec!["pane.layout", "pane.enter"],
            ms: 300.0,
            refresh_ms: 16.667,
            measured: true,
            partial: false,
            covered: false,
            refreshes: 18,
            counted: 2_160,
            presents: 17,
            missed: 1,
            misses_ms: vec![100.0],
            first_ms: Some(30.0),
            lead: Some(2),
            paints: 3,
            slow_paints: 1,
            longest_paint_ms: 21.0,
        });
        t.polled(3_000);
        t.walked(250_000, 40);
        t.looked_over(40_000);
        t.motion(MotionDone {
            names: vec!["rail.edge"],
            ms: 400.0,
            refresh_ms: 16.667,
            measured: false,
            partial: false,
            covered: true,
            refreshes: 0,
            counted: 0,
            presents: 0,
            missed: 0,
            misses_ms: vec![],
            first_ms: None,
            lead: None,
            paints: 1,
            slow_paints: 0,
            longest_paint_ms: 4.0,
        });
        let j = t.to_json(Available { frame_statistics: true, frame_statistics_error: Some(0x887A0001u32 as i32), compositor_clock: true });
        let actions = j["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        let key = &actions[0];
        assert_eq!(key["action"], "key");
        assert_eq!(key["count"], 3);
        assert_eq!(key["seen"], 2);
        assert_eq!(key["paint_ms"]["mean"], 3.0);
        assert_eq!(key["screen_ms"]["mean"], 25.0);
        assert_eq!(key["screen_ms"]["max"], 30.0);
        assert_eq!(key["after_present_ms"]["mean"], 18.0);
        assert_eq!(key["last"]["paint_ms"], 3.0);
        assert_eq!(key["last"]["screen_ms"], Value::Null);
        // The last shown, seen
        t.on_screen(Action::Key, 7_000_000, 25_000_000, 1_700_000_000_300.0);
        let j2 = t.to_json(Available::default());
        assert_eq!(j2["actions"][0]["last"]["screen_unix_ms"], 1_700_000_000_300.0);
        assert_eq!(j2["actions"][0]["last"]["screen_ms"], 25.0);
        assert_eq!(actions[1]["action"], "tab");
        assert_eq!(actions[1]["lost"], 1);
        assert_eq!(actions[1]["hidden"], 1);
        assert_eq!(actions[2]["action"], "paint");
        assert_eq!(actions[2]["overtaken"], 1);
        assert_eq!(j["available"]["frame_statistics_error"], "0x887a0001");
        let m = &j["motion"];
        assert_eq!(m["episodes"], 2);
        assert_eq!(m["covered"], 1);
        assert_eq!(m["measured"], 1);
        assert_eq!(m["missed"], 1);
        assert_eq!(m["with_misses"], 1);
        assert_eq!(m["slow_paints"], 1);
        assert_eq!(m["first_ms"]["mean"], 30.0);
        assert_eq!(m["names"].as_array().unwrap().len(), 3);
        assert_eq!(m["last"]["covered"], true);
        assert_eq!(j["cost"]["polls"], 1);
        assert_eq!(j["cost"]["frames_walked"], 40);
        assert_eq!(j["cost"]["covers"], 1);
        assert_eq!(j["cost"]["cover_ms"], 0.04);
        assert_eq!(j["available"]["compositor_clock"], true);
    }

    #[test]
    fn a_reset_starts_the_totals_again() {
        with_totals(|t| t.lost(Action::Split));
        let first = report_json(true);
        assert!(first["actions"].as_array().unwrap().iter().any(|a| a["action"] == "split"));
        let second = report_json(false);
        assert!(second["actions"].as_array().unwrap().is_empty());
    }
}
