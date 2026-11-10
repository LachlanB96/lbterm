//! LBTerm's budgets in the daily build (lbterm/PERF.md, "Warnings in the daily build"): the GUI
//! judging itself as it runs, in every LBTerm, Lachlan's daily one included, and saying in its log
//! when it costs more than it should. Two kinds of judgement, both WARN lines on target
//! `lbterm::budget` (`lbterm.ps1 logs -Budget` reads them):
//!
//! - **At rest, nothing runs.** A window is at rest when nothing has happened to it for
//!   `quiet_ms`: no input, no output from its panes, nothing the mux said about its tabs, and no
//!   animation, fade or wait in flight (the state file's `animating` and `settled`, read only when
//!   a frame would otherwise be at rest). A frame it paints then is counted against its site, the
//!   request that asked for it (counters.rs). So are timers that fire and tasks that run while
//!   every window is at rest. What repaints at rest on purpose ([`Purpose`]: upstream's own
//!   animation frames, a clock in view, update-status's tick) says so ahead of time with a
//!   [`tick`] when it arms its timer, at the rate it may: what runs in the 100 ms after a tick due
//!   then is that purpose's, a frame or two and its timers and tasks, no more, and ticks past the
//!   rate count as at rest. A window that paints more than `rest_frames` frames at rest in a span
//!   of 10 s is said at once, naming who asked:
//!
//!   ```text
//!   window-0 painted 312 frames in 10 s at rest, asked by wezterm-gui/src/termwindow/render/conductor_view.rs:338 (300), wezterm-gui/src/termwindow/render/lights.rs:120 (12)
//!   ```
//!
//!   and while it goes on, the spans after it once a minute (`... in 5 more spans of 10 s in the
//!   last 60 s ...`). The same for timers (`rest_timers`) and tasks (`rest_tasks`, `rest_task_ms`).
//! - **Expected times.** An [`Action`] (a key's echo, a tab switch, a new tab, a split, the rail
//!   opening, a paint ...) is timed from what started it to the end of the first frame that shows
//!   it, and every run of what slow.rs times in between is traced. One over its time says by how
//!   much and what the time went on:
//!
//!   ```text
//!   tab took 84.0 ms, 54.0 ms over its 30 ms: task wezterm-gui/src/termwindow/mod.rs:1867 51.2 ms, paint window-0/pass 20.1 ms (2), the rest 10.7 ms
//!   ```
//!
//!   at once the first time in 10 s, and the rest of those 10 s as one line at their end
//!   (`tab: 4 times over its 30 ms in the last 10 s, max 120.0 ms`). Every one, over or not, is in
//!   the GUI's histograms (`lbterm.action{action=tab}`, wezterm-gui's side) and in the counts
//!   `metrics` replies with ([`report_json`]).
//!
//! The judging is [`Budget`], tested with the time handed in. The GUI thread has one, through the
//! free functions here; wezterm-gui's termwindow/budget.rs says what happened to its windows.
//! Cheap where it's hot: a timed run checks one thread-local flag and, for a task, one atomic
//! compare; a frame that isn't at rest, one compare. Everything else happens at rest, when there's
//! nothing else to do, or once an action is shown.

use crate::hist::Histogram;
use crate::slow::{source_name, Kind, Site};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The log target the lines go to
pub const TARGET: &str = "lbterm::budget";

/// What an at-rest line covers: from the first thing at rest
pub const SPAN: Duration = Duration::from_secs(10);

/// After a kind's first line, what more there is is said together this long after
pub const LINE_EVERY: Duration = Duration::from_secs(60);

/// What runs from a purpose's tick being due until this long after is the purpose's
pub const TICK_WINDOW: Duration = Duration::from_millis(100);

/// And from a little before: a timer due at a time can be woken a clock tick early
pub const TICK_EARLY: Duration = Duration::from_millis(2);

/// What one tick of a purpose may bring: its frames, its timers, its tasks. A frame comes of a
/// timer firing, the task that waited on it running, and the notify it sends; update-status's
/// tick also runs two Lua events and their ends
pub const TICK_FRAMES: u32 = 2;
pub const TICK_TIMERS: u32 = 4;
pub const TICK_TASKS: u32 = 16;

/// How long an action may wait for the frame that shows it before it's given up on (a split that
/// failed, a tab that never came)
pub const SHOWN_AT_MOST: Duration = Duration::from_secs(10);

/// A key waits for its echo this many times its time, then is given up on without a line: the
/// program it went to doesn't echo (a password prompt) or isn't reading, and what it prints
/// later isn't the key's, nor LBTerm's time
pub const KEY_WAITS: f64 = 3.0;

/// What a background tab's news (its title, its status, a light: the mux's alerts for a tab that
/// isn't open) may repaint: the frames from it to this long after are its, up to
/// `rest_news_frames` in a span
pub const NEWS_WINDOW: Duration = Duration::from_millis(100);

/// The most actions waiting to be shown at once; past it the oldest is given up on
const MAX_PENDING: usize = 16;

/// The most sites an action's trace keeps apart; the rest are added up as "more"
const TRACE_SITES: usize = 24;

/// The most sites a line names; the rest are added up
const LINE_SITES: usize = 6;

type At = &'static Location<'static>;

/// What repaints at rest on purpose, each allowed the rate it says with each [`tick`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// Upstream's own next frame (render/paint.rs): a blinking cursor or blinking text, a bell's
    /// fade, an animated image, a tab's progress. Only while the window has the keyboard; at
    /// `animation_fps`
    Animation,
    /// Ages and times on screen that move on with the clock: the rail's rows and links, the card
    /// of what a tab is running. One a second for each that counts seconds
    Clock,
    /// update-status's tick (`status_update_interval`) and what it sets
    Status,
}

impl Purpose {
    pub const ALL: [Purpose; 3] = [Purpose::Animation, Purpose::Clock, Purpose::Status];

    pub fn name(self) -> &'static str {
        match self {
            Purpose::Animation => "animation",
            Purpose::Clock => "clock",
            Purpose::Status => "status",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What the GUI times from what starts it to the first frame that shows it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// A key (or a paste) sent to a pane, to the frame after the pane's next output
    Key,
    /// A tab switch: a key, a click on the rail or the tab bar, Lua
    Tab,
    /// A new tab, to the first frame with it in the window
    NewTab,
    /// A tab closed, to the first frame without it
    CloseTab,
    /// A split, to the first frame with the new pane
    Split,
    /// The tab rail opening, from the end of the pointer's rest on it
    Rail,
    /// The InMail button's list opening, from the click
    Inbox,
    /// The agenda's list opening, from the click on its button
    Agenda,
    /// The Conductor's view opening, from the click on its pad
    ConductorView,
    /// The new-tab picker opening, from the click on the + pad or the key (lbterm/NEW-TAB.md)
    Picker,
    /// A pick in the picker, from the key or click to the first frame with the Starting row
    Pick,
    /// A config reload, from when it started reading the file
    Reload,
    /// A paint: building the frame, drawing it and presenting it
    Paint,
}

impl Action {
    pub const COUNT: usize = 13;
    pub const ALL: [Action; Action::COUNT] = [
        Action::Key,
        Action::Tab,
        Action::NewTab,
        Action::CloseTab,
        Action::Split,
        Action::Rail,
        Action::Inbox,
        Action::Agenda,
        Action::ConductorView,
        Action::Picker,
        Action::Pick,
        Action::Reload,
        Action::Paint,
    ];

    /// Its name in the lines, `metrics` and config's `lbterm_budget.actions` (with `_` for `-`)
    pub fn name(self) -> &'static str {
        match self {
            Action::Key => "key",
            Action::Tab => "tab",
            Action::NewTab => "new-tab",
            Action::CloseTab => "close-tab",
            Action::Split => "split",
            Action::Rail => "rail",
            Action::Inbox => "inbox",
            Action::Agenda => "agenda",
            Action::ConductorView => "conductor-view",
            Action::Picker => "picker",
            Action::Pick => "pick",
            Action::Reload => "reload",
            Action::Paint => "paint",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The rate upstream's next frame comes at, as its [`Purpose::Animation`] tick says it: what its
/// wait `left` gives, no less than `fps` (`animation_fps`) and no more than `fastest` (that or
/// the fastest of the blinks' rates). With `animation_fps` 1 a blink turns at its own rate, 1.25
/// a second at `cursor_blink_rate` 800, where an easing comes at `animation_fps`
pub fn animation_rate(left: Duration, fps: f64, fastest: f64) -> f64 {
    let secs = left.as_secs_f64();
    if secs > 0.0 {
        (1.0 / secs).clamp(fps, fastest.max(fps))
    } else {
        fastest.max(fps)
    }
}

/// How the budgets are set (config's `lbterm_budget`, which the GUI hands over; its names and
/// defaults are in config/src/lbterm.rs)
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Off: nothing judged and nothing logged; the counters still count
    pub enabled: bool,
    /// How long a window stays busy after anything happens to it
    pub quiet: Duration,
    /// The frames a window may paint at rest in a span, beyond its purposes'
    pub rest_frames: u32,
    /// The frames a window may paint in a span for its background tabs' news (their titles,
    /// statuses and lights, which show on their rail rows), each within [`NEWS_WINDOW`] of it
    pub rest_news_frames: u32,
    /// The timers that may fire, and the tasks that may run, at rest in a span
    pub rest_timers: u32,
    pub rest_tasks: u32,
    /// And the time those tasks may take
    pub rest_task_ms: f64,
    /// Each action's time, in milliseconds, by [`Action::index`]; 0 times nothing
    pub actions: [f64; Action::COUNT],
}

impl Settings {
    pub fn off() -> Self {
        Self {
            enabled: false,
            quiet: Duration::from_secs(1),
            rest_frames: 0,
            rest_news_frames: 0,
            rest_timers: 0,
            rest_tasks: 0,
            rest_task_ms: 0.0,
            actions: [0.0; Action::COUNT],
        }
    }

    pub fn action_ms(&self, action: Action) -> f64 {
        self.actions[action.index()]
    }
}

/// The sites a frame was asked for by (counters.rs's [`crate::counters::frame`]): at most eight
/// named, and how many more
#[derive(Debug, Clone, Copy, Default)]
pub struct Reasons {
    sites: [Option<At>; 8],
    len: usize,
    more: u32,
}

impl Reasons {
    pub fn push(&mut self, at: At) {
        if self.len < self.sites.len() {
            self.sites[self.len] = Some(at);
            self.len += 1;
        } else {
            self.more += 1;
        }
    }

    /// One that asked whose site isn't known (past the counters' table)
    pub fn unnamed(&mut self) {
        self.more += 1;
    }

    pub fn iter(&self) -> impl Iterator<Item = At> + '_ {
        self.sites[..self.len].iter().flatten().copied()
    }

    /// Nobody asked: Windows' own WM_PAINT
    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.more == 0
    }
}

/// A site as the lines name it: `<file>:<line>`, as slow.rs and counters.rs do
fn at_name(at: At) -> String {
    format!("{}:{}", source_name(at.file()), at.line())
}

/// Counts by site, most first, the same names added up
fn by_name<I: Iterator<Item = (String, u64, u64)>>(items: I) -> Vec<(String, u64, u64)> {
    let mut by: HashMap<String, (u64, u64)> = HashMap::new();
    for (name, n, ns) in items {
        let e = by.entry(name).or_default();
        e.0 += n;
        e.1 += ns;
    }
    let mut list: Vec<(String, u64, u64)> = by.into_iter().map(|(k, (n, ns))| (k, n, ns)).collect();
    list.sort_by(|a, b| (b.1, b.2, &a.0).cmp(&(a.1, a.2, &b.0)));
    list
}

/// Milliseconds, as the lines say them
fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

/// `a (300), b (12)`, or with times `a (400, 20.1 ms)`; past [`LINE_SITES`], `and 3 more (9)`
fn site_list(list: &[(String, u64, u64)], times: bool) -> String {
    let mut parts: Vec<String> = list
        .iter()
        .take(LINE_SITES)
        .map(|(site, n, ns)| {
            if times {
                format!("{site} ({n}, {:.1} ms)", ms(*ns))
            } else {
                format!("{site} ({n})")
            }
        })
        .collect();
    if list.len() > LINE_SITES {
        let rest = &list[LINE_SITES..];
        let n: u64 = rest.iter().map(|r| r.1).sum();
        parts.push(format!("and {} more ({n})", rest.len()));
    }
    parts.join(", ")
}

/// One kind of at-rest line's pace: the first span over is said at once; what more there is is
/// held and said together [`LINE_EVERY`] after, once a minute while it goes on
#[derive(Debug)]
struct Pace<H> {
    said: Option<Instant>,
    held: Option<(H, u32)>,
}

trait Held: Sized {
    fn merge(&mut self, other: Self);
}

impl<H: Held> Pace<H> {
    /// A span over, ending at `now`: the held lines due first, then `Some` when it's said now
    fn over(&mut self, span: H, now: Instant, out: &mut Vec<(H, Option<u32>)>) {
        self.due(now, out);
        match self.said {
            Some(said) if now < said + LINE_EVERY => match self.held.as_mut() {
                Some((held, spans)) => {
                    held.merge(span);
                    *spans += 1;
                }
                None => self.held = Some((span, 1)),
            },
            _ => {
                self.said = Some(now);
                out.push((span, None));
            }
        }
    }

    /// What's held and due by `now`, said as one with how many spans it covers
    fn due(&mut self, now: Instant, out: &mut Vec<(H, Option<u32>)>) {
        let Some(said) = self.said else {
            return;
        };
        if now < said + LINE_EVERY {
            return;
        }
        match self.held.take() {
            Some((held, spans)) => {
                self.said = Some(now);
                out.push((held, Some(spans)));
            }
            None => self.said = None,
        }
    }

    fn next_due(&self) -> Option<Instant> {
        self.held.as_ref().and(self.said.map(|s| s + LINE_EVERY))
    }
}

/// A window's frames at rest in a span
#[derive(Debug, Clone)]
struct Frames {
    start: Instant,
    /// Asked for, counted
    frames: u32,
    /// Nobody asked for: Windows' own WM_PAINT. Counted too, named as that
    unasked: u32,
    sites: Vec<(At, u64)>,
    more: u64,
    /// Frames in ticks past their purpose's rate, and the most ticks it may make in a span
    past: [(u32, u32); 3],
    /// Frames painted on purpose, allowed
    on_purpose: [u32; 3],
    /// Frames for background tabs' news: allowed, and past the allowance (counted)
    news: u32,
    news_past: u32,
}

/// What a frame nobody asked for is named by in the lines: Windows' own
pub const UNASKED: &str = "WM_PAINT";

impl Frames {
    fn new(start: Instant) -> Self {
        Self {
            start,
            frames: 0,
            unasked: 0,
            sites: vec![],
            more: 0,
            past: [(0, 0); 3],
            on_purpose: [0; 3],
            news: 0,
            news_past: 0,
        }
    }

    /// Every frame counted against the budget: those asked for and Windows' own
    fn counted(&self) -> u32 {
        self.frames + self.unasked
    }

    fn named(&self) -> Vec<(String, u64, u64)> {
        let mut list = by_name(self.sites.iter().map(|(at, n)| (at_name(*at), *n, 0)));
        if self.unasked > 0 {
            list.push((UNASKED.to_string(), self.unasked as u64, 0));
            // Most first; of as many, the code's own sites before Windows'
            list.sort_by(|a, b| b.1.cmp(&a.1).then((a.0 == UNASKED).cmp(&(b.0 == UNASKED))).then(a.0.cmp(&b.0)));
        }
        list
    }
}

impl Held for Frames {
    fn merge(&mut self, other: Self) {
        self.frames += other.frames;
        self.unasked += other.unasked;
        self.news += other.news;
        self.news_past += other.news_past;
        for (at, n) in other.sites {
            match self.sites.iter_mut().find(|(a, _)| std::ptr::eq(*a, at)) {
                Some(e) => e.1 += n,
                None => self.sites.push((at, n)),
            }
        }
        self.more += other.more;
        for i in 0..3 {
            self.past[i].0 += other.past[i].0;
            self.past[i].1 = self.past[i].1.max(other.past[i].1);
            self.on_purpose[i] += other.on_purpose[i];
        }
    }
}

/// Timers fired and tasks run at rest in a span, the whole process's
#[derive(Debug, Clone)]
struct Runs {
    start: Instant,
    /// By site: how many, and for tasks their time
    sites: Vec<(At, u64, u64)>,
}

impl Runs {
    fn new(start: Instant) -> Self {
        Self { start, sites: vec![] }
    }

    fn add(&mut self, at: At, ns: u64) {
        match self.sites.iter_mut().find(|(a, _, _)| std::ptr::eq(*a, at)) {
            Some(e) => {
                e.1 += 1;
                e.2 += ns;
            }
            None => self.sites.push((at, 1, ns)),
        }
    }

    fn count(&self) -> u64 {
        self.sites.iter().map(|s| s.1).sum()
    }

    fn ns(&self) -> u64 {
        self.sites.iter().map(|s| s.2).sum()
    }

    fn named(&self) -> Vec<(String, u64, u64)> {
        by_name(self.sites.iter().map(|(at, n, ns)| (at_name(*at), *n, *ns)))
    }
}

impl Held for Runs {
    fn merge(&mut self, other: Self) {
        for (at, n, ns) in other.sites {
            match self.sites.iter_mut().find(|(a, _, _)| std::ptr::eq(*a, at)) {
                Some(e) => {
                    e.1 += n;
                    e.2 += ns;
                }
                None => self.sites.push((at, n, ns)),
            }
        }
    }
}

/// A purpose's tick: due when, whether its rate allowed it, and what it has brought so far
#[derive(Debug, Clone, Copy)]
struct Tick {
    purpose: Purpose,
    due: Instant,
    allowed: bool,
    /// The most ticks the purpose may make in a span, as it said with this one
    most: u32,
    frames: u32,
    timers: u32,
    tasks: u32,
}

impl Tick {
    fn covers(&self, now: Instant) -> bool {
        now + TICK_EARLY >= self.due && now <= self.due + TICK_WINDOW
    }
}

/// One window's watch
#[derive(Debug)]
struct Watch {
    /// Busy until then: what happened to it, and what's in flight, `quiet` on. Its frames then
    /// aren't judged
    busy_until: Option<Instant>,
    /// Its background tabs said something until then, `quiet` on: its frames are judged all the
    /// same, but the process isn't at rest for its timers and tasks (the news's own)
    news_until: Option<Instant>,
    /// The frames until then are the news's, up to `rest_news_frames` a span
    news_frames_until: Option<Instant>,
    ticks: Vec<Tick>,
    /// Each purpose's ticks due in the last span and allowed, for its rate
    dues: [VecDeque<Instant>; 3],
    span: Option<Frames>,
    pace: Pace<Frames>,
}

impl Watch {
    fn new() -> Self {
        Self {
            busy_until: None,
            news_until: None,
            news_frames_until: None,
            ticks: vec![],
            dues: Default::default(),
            span: None,
            pace: Pace { said: None, held: None },
        }
    }

    fn busy(&self, now: Instant) -> bool {
        self.busy_until.map_or(false, |until| now < until)
    }

    /// Busy, or its background tabs' news still under way: the process isn't at rest
    fn stirring(&self, now: Instant) -> bool {
        self.busy(now) || self.news_until.map_or(false, |until| now < until)
    }

    /// When it stops stirring
    fn stirring_until(&self) -> Option<Instant> {
        match (self.busy_until, self.news_until) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// The tick whose window covers `now`, the latest due first
    fn tick_at(&mut self, now: Instant) -> Option<&mut Tick> {
        self.ticks
            .iter_mut()
            .filter(|t| t.covers(now))
            .max_by_key(|t| t.due)
    }
}

/// An action's trace: every run of what slow.rs times while it waited, by site. In place, so an
/// action begun and shown (every frame's own paint is one) allocates nothing
#[derive(Debug, Clone, Default)]
struct Trace {
    sites: [Option<(Site, u64, u32)>; TRACE_SITES],
    len: usize,
    more_ns: u64,
    more: u32,
}

impl Trace {
    fn add(&mut self, site: Site, ns: u64) {
        if let Some((_, t, n)) = self.sites[..self.len].iter_mut().flatten().find(|e| e.0 == site) {
            *t += ns;
            *n += 1;
        } else if self.len < TRACE_SITES {
            self.sites[self.len] = Some((site, ns, 1));
            self.len += 1;
        } else {
            self.more_ns += ns;
            self.more += 1;
        }
    }

    fn entries(&self) -> impl Iterator<Item = &(Site, u64, u32)> {
        self.sites[..self.len].iter().flatten()
    }

    fn busy_ns(&self) -> u64 {
        self.entries().map(|e| e.1).sum::<u64>() + self.more_ns
    }

    /// `task <site> 51.2 ms, paint window-0/pass 20.1 ms (2), the rest 10.7 ms`: what took
    /// longest first, those under 0.1 ms left out, the time not traced as the rest
    fn said(&self, elapsed_ns: u64) -> String {
        let mut named: HashMap<(Kind, String), (u64, u32)> = HashMap::new();
        for (site, ns, n) in self.entries() {
            let e = named.entry((site.kind(), site.to_string())).or_default();
            e.0 += ns;
            e.1 += n;
        }
        let mut list: Vec<((Kind, String), (u64, u32))> = named.into_iter().collect();
        list.sort_by(|a, b| (b.1 .0, &a.0 .1).cmp(&(a.1 .0, &b.0 .1)));
        let mut parts: Vec<String> = list
            .iter()
            .filter(|(_, (ns, _))| *ns >= 100_000)
            .take(LINE_SITES)
            .map(|((kind, site), (ns, n))| {
                let times = if *n > 1 { format!(" ({n})") } else { String::new() };
                format!("{} {site} {:.1} ms{times}", kind.as_str(), ms(*ns))
            })
            .collect();
        let shown: u64 = list
            .iter()
            .filter(|(_, (ns, _))| *ns >= 100_000)
            .take(LINE_SITES)
            .map(|(_, (ns, _))| *ns)
            .sum();
        let others = self.busy_ns().saturating_sub(shown);
        if others >= 100_000 {
            parts.push(format!("others {:.1} ms", ms(others)));
        }
        parts.push(format!("the rest {:.1} ms", ms(elapsed_ns.saturating_sub(self.busy_ns()))));
        parts.join(", ")
    }
}

#[derive(Debug)]
struct Pending {
    ticket: u64,
    window: usize,
    action: Action,
    start: Instant,
    /// Not shown by then, it's given up on, said nowhere but in the counts
    give_up: Instant,
    trace: Trace,
}

/// An action shown: how long it took, and whether that was over its time
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Done {
    pub action: Action,
    pub window: usize,
    /// What started it (lbterm_extras::present times it to the screen from here)
    pub start: Instant,
    pub elapsed: Duration,
    pub over: bool,
}

/// An action's lines in a 10 s window, as slow.rs paces its own: the first over at once, the
/// rest added up at its end
#[derive(Debug, Clone, Copy)]
struct ActionPace {
    start: Instant,
    count: u32,
    max_ms: f64,
    budget_ms: f64,
}

/// What `metrics` reports of what ran at rest, by site, since the process started or the last
/// reset: shared with the pipe's thread, so behind a lock, taken only for what's counted at rest
/// (which shouldn't happen at all)
#[derive(Debug, Default, Clone)]
pub struct Totals {
    pub frames: u64,
    pub unasked: u64,
    pub frame_sites: HashMap<String, u64>,
    pub timers: u64,
    pub timer_sites: HashMap<String, u64>,
    pub tasks: u64,
    pub task_ns: u64,
    pub task_sites: HashMap<String, (u64, u64)>,
}

/// One action's counts: each shown in its histogram, those over their time, those given up on
pub struct ActionStat {
    pub hist: Histogram,
    pub over: AtomicU64,
    pub unshown: AtomicU64,
}

impl ActionStat {
    const fn new() -> Self {
        Self {
            hist: Histogram::new(),
            over: AtomicU64::new(0),
            unshown: AtomicU64::new(0),
        }
    }
}

/// What `metrics` reports of the actions, the purposes, the news and the lines, in atomics: what
/// the GUI thread adds to as each action is shown, each purpose ticks and each frame of news
/// comes takes no lock and allocates nothing
pub struct Stats {
    actions: [ActionStat; Action::COUNT],
    /// By purpose: ticks, ticks past the rate, frames, timers, tasks
    purposes: [[AtomicU64; 5]; 3],
    /// Background tabs' news: frames allowed, frames past the allowance
    news: [AtomicU64; 2],
    lines: AtomicU64,
}

impl Stats {
    pub const fn new() -> Self {
        Self {
            actions: [const { ActionStat::new() }; Action::COUNT],
            purposes: [const { [const { AtomicU64::new(0) }; 5] }; 3],
            news: [const { AtomicU64::new(0) }; 2],
            lines: AtomicU64::new(0),
        }
    }

    fn bump(a: &AtomicU64, n: u64) {
        a.fetch_add(n, Relaxed);
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

/// `metrics`' `budget`: the settings, what ran at rest, on purpose and for news, each action's
/// counts and percentiles, the lines said; with `reset`, each zeroed as it's read
pub fn report(settings: &Settings, totals: &Mutex<Totals>, stats: &Stats, reset: bool) -> Value {
    let take = |a: &AtomicU64| if reset { a.swap(0, Relaxed) } else { a.load(Relaxed) };
    let at_rest = {
        let mut t = totals.lock().unwrap_or_else(|e| e.into_inner());
        let sites = |m: &HashMap<String, u64>, key: &str| {
            let mut list: Vec<(&String, &u64)> = m.iter().collect();
            list.sort_by(|a, b| (b.1, a.0).cmp(&(a.1, b.0)));
            Value::Array(list.into_iter().map(|(s, n)| json!({ "site": s, key: n })).collect())
        };
        let mut tasks: Vec<(&String, &(u64, u64))> = t.task_sites.iter().collect();
        tasks.sort_by(|a, b| (b.1 .0, a.0).cmp(&(a.1 .0, b.0)));
        let json = json!({
            "frames": t.frames,
            "unasked": t.unasked,
            "timers": t.timers,
            "tasks": t.tasks,
            "task_ms": crate::counters::ms(t.task_ns),
            "frame_sites": sites(&t.frame_sites, "frames"),
            "timer_sites": sites(&t.timer_sites, "fired"),
            "task_sites": Value::Array(tasks.into_iter().map(|(s, (n, ns))| {
                json!({ "site": s, "runs": n, "ms": crate::counters::ms(*ns) })
            }).collect()),
        });
        if reset {
            *t = Totals::default();
        }
        json
    };
    let purposes: Vec<Value> = Purpose::ALL
        .iter()
        .filter_map(|p| {
            let row = &stats.purposes[p.index()];
            let [ticks, past, frames, timers, tasks] = [0, 1, 2, 3, 4].map(|i| take(&row[i]));
            (ticks > 0).then(|| {
                json!({
                    "purpose": p.name(), "ticks": ticks, "past_rate": past,
                    "frames": frames, "timers": timers, "tasks": tasks,
                })
            })
        })
        .collect();
    let actions: Vec<Value> = Action::ALL
        .iter()
        .filter_map(|a| {
            let s = &stats.actions[a.index()];
            let h = s.hist.read(reset);
            let (over, unshown) = (take(&s.over), take(&s.unshown));
            let budget = settings.action_ms(*a);
            (h.count > 0 || unshown > 0 || budget > 0.0).then(|| {
                json!({
                    "action": a.name(),
                    "budget_ms": budget,
                    "count": h.count,
                    "over": over,
                    "unshown": unshown,
                    "mean_ms": round3(h.mean_ms),
                    "p50_ms": round3(h.p50_ms),
                    "p95_ms": round3(h.p95_ms),
                    "p99_ms": round3(h.p99_ms),
                    "max_ms": round3(h.max_ms),
                })
            })
        })
        .collect();
    json!({
        "enabled": settings.enabled,
        "quiet_ms": settings.quiet.as_millis() as u64,
        "at_rest": at_rest,
        "on_purpose": purposes,
        "news": { "frames": take(&stats.news[0]), "past": take(&stats.news[1]), "allowed": settings.rest_news_frames },
        "actions": actions,
        "lines": take(&stats.lines),
    })
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Adds to the totals `metrics` reports, when there are any to add to
fn add(totals: Option<&'static Mutex<Totals>>, f: impl FnOnce(&mut Totals)) {
    if let Some(totals) = totals {
        f(&mut totals.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// The judging, for the GUI thread: every window's watch, the process's runs at rest, the
/// actions waiting to be shown, and the lines' pace. Every call takes the time, so tests hand it
/// in; each returns the lines it says, which the free functions log.
pub struct Budget {
    settings: Settings,
    windows: HashMap<usize, Watch>,
    timers: Option<Runs>,
    tasks: Option<Runs>,
    timers_pace: Pace<Runs>,
    tasks_pace: Pace<Runs>,
    pending: Vec<Pending>,
    paces: [Option<ActionPace>; Action::COUNT],
    next_ticket: u64,
    /// The files whose tasks and timers are the measuring's own (the budget's waker, the slow
    /// log's, the state file's and present.rs's), not counted at rest
    own_files: Vec<&'static str>,
    totals: Option<&'static Mutex<Totals>>,
    stats: Option<&'static Stats>,
    /// When the soonest action waiting is given up on
    next_give_up: Option<Instant>,
}

impl Budget {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            windows: HashMap::new(),
            timers: None,
            tasks: None,
            timers_pace: Pace { said: None, held: None },
            tasks_pace: Pace { said: None, held: None },
            pending: vec![],
            paces: [None; Action::COUNT],
            next_ticket: 1,
            own_files: vec![],
            totals: None,
            stats: None,
            next_give_up: None,
        }
    }

    /// Adds `n` to one of the atomics `metrics` reports, when there are any
    fn stat(&self, f: impl FnOnce(&'static Stats) -> &'static AtomicU64, n: u64) {
        if let Some(stats) = self.stats {
            Stats::bump(f(stats), n);
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// New settings: off, everything open is forgotten
    pub fn configure(&mut self, settings: Settings) {
        if !settings.enabled {
            self.windows.clear();
            self.timers = None;
            self.tasks = None;
            self.pending.clear();
        }
        self.settings = settings;
    }

    fn total(&self, f: impl FnOnce(&mut Totals)) {
        add(self.totals, f)
    }

    fn watch(&mut self, window: usize) -> &mut Watch {
        self.windows.entry(window).or_insert_with(Watch::new)
    }

    /// When the last window stops being busy or hearing its background tabs' news: none while
    /// none is
    pub fn busy_until(&self) -> Option<Instant> {
        self.windows.values().filter_map(|w| w.stirring_until()).max()
    }

    /// Whether every window is at rest at `now`, its background tabs quiet too
    pub fn all_at_rest(&self, now: Instant) -> bool {
        self.settings.enabled && !self.windows.is_empty() && !self.windows.values().any(|w| w.stirring(now))
    }

    /// Something happened to `window` (input, its open tab's output, the mux): busy for `quiet`
    pub fn activity(&mut self, window: usize, now: Instant) {
        if !self.settings.enabled {
            return;
        }
        let until = now + self.settings.quiet;
        let w = self.watch(window);
        w.busy_until = Some(w.busy_until.map_or(until, |b| b.max(until)));
    }

    /// A background tab of `window` said something (its output, or with `repaints` an alert its
    /// rail row may show: a title, a status, a light). Its frames are still judged, so a window
    /// with busy tabs behind it isn't kept from being seen at rest; the frames in the
    /// [`NEWS_WINDOW`] after an alert are the news's, up to `rest_news_frames` a span. The
    /// process isn't at rest for `quiet`: the news's own tasks run meanwhile
    pub fn news(&mut self, window: usize, repaints: bool, now: Instant) {
        if !self.settings.enabled {
            return;
        }
        let until = now + self.settings.quiet;
        let w = self.watch(window);
        w.news_until = Some(w.news_until.map_or(until, |b| b.max(until)));
        if repaints {
            let frames = now + NEWS_WINDOW;
            w.news_frames_until = Some(w.news_frames_until.map_or(frames, |b| b.max(frames)));
        }
    }

    /// Whether `window` is busy at `now`: a frame then is judged no further
    pub fn busy(&self, window: usize, now: Instant) -> bool {
        !self.settings.enabled || self.windows.get(&window).map_or(false, |w| w.busy(now))
    }

    /// How long `window` stays busy from `now`
    pub fn busy_left(&self, window: usize, now: Instant) -> Duration {
        self.windows
            .get(&window)
            .and_then(|w| w.busy_until)
            .map_or(Duration::ZERO, |until| until.saturating_duration_since(now))
    }

    /// `window` has gone
    pub fn forget(&mut self, window: usize) {
        self.windows.remove(&window);
        self.pending.retain(|p| p.window != window);
    }

    /// `purpose` will repaint `window` at `due` on purpose, at most `per_second` a second. Of a
    /// purpose ticking faster than it says, only the ticks past its rate bring nothing: the ones
    /// allowed are what's kept for the rate, so a blink a little faster than said loses a tick
    /// now and then, not all of them once it's over once
    pub fn tick(&mut self, window: usize, purpose: Purpose, due: Instant, per_second: f64, now: Instant) {
        if !self.settings.enabled {
            return;
        }
        let most = (per_second.max(0.0) * SPAN.as_secs_f64()).ceil() as u32 + 1;
        let w = self.watch(window);
        w.ticks.retain(|t| t.due + TICK_WINDOW >= now);
        if w.ticks.iter().any(|t| t.purpose == purpose && t.due == due) {
            return;
        }
        let dues = &mut w.dues[purpose.index()];
        while dues.front().map_or(false, |d| *d + SPAN <= due) {
            dues.pop_front();
        }
        let allowed = (dues.len() as u32) < most;
        if allowed {
            dues.push_back(due);
        }
        w.ticks.push(Tick {
            purpose,
            due,
            allowed,
            most,
            frames: 0,
            timers: 0,
            tasks: 0,
        });
        let p = purpose.index();
        self.stat(|s| &s.purposes[p][0], 1);
        if !allowed {
            self.stat(|s| &s.purposes[p][1], 1);
        }
    }

    /// `window` is painting a frame at `now`, asked for by `reasons`. `in_flight` says until when
    /// an animation, fade or wait is under way; it's asked only when the window would otherwise
    /// be at rest
    pub fn frame(
        &mut self,
        window: usize,
        now: Instant,
        reasons: &Reasons,
        in_flight: impl FnOnce() -> Option<Instant>,
    ) -> Vec<String> {
        let mut out = vec![];
        if !self.settings.enabled {
            return out;
        }
        let quiet = self.settings.quiet;
        let news_allowed = self.settings.rest_news_frames;
        let totals = self.totals;
        let stats = self.stats;
        {
            let w = self.watch(window);
            if w.busy(now) {
                return out;
            }
            if let Some(end) = in_flight() {
                let until = end + quiet;
                if until > now {
                    w.busy_until = Some(w.busy_until.map_or(until, |b| b.max(until)));
                    return out;
                }
            }
        }
        // At rest: the spans that have ended first, then this frame is a purpose's, the news's,
        // or counted
        self.close_spans(now, &mut out);
        let w = self.watch(window);
        let mut past = None;
        if let Some(tick) = w.tick_at(now) {
            if tick.allowed && tick.frames < TICK_FRAMES {
                tick.frames += 1;
                let p = tick.purpose.index();
                let span = w.span.get_or_insert_with(|| Frames::new(now));
                span.on_purpose[p] += 1;
                if let Some(s) = stats {
                    Stats::bump(&s.purposes[p][2], 1);
                }
                return out;
            }
            if !tick.allowed {
                past = Some((tick.purpose.index(), tick.most));
            }
        }
        let news = w.news_frames_until.map_or(false, |until| now <= until);
        let span = w.span.get_or_insert_with(|| Frames::new(now));
        if news {
            if span.news < news_allowed {
                span.news += 1;
                if let Some(s) = stats {
                    Stats::bump(&s.news[0], 1);
                }
                return out;
            }
            span.news_past += 1;
            if let Some(s) = stats {
                Stats::bump(&s.news[1], 1);
            }
        }
        // Nobody asked: Windows' own WM_PAINT, counted all the same (a paint Windows sends again
        // and again, unvalidated, is a loop like any other)
        if reasons.is_empty() {
            span.unasked += 1;
            add(totals, |t| t.unasked += 1);
            return out;
        }
        span.frames += 1;
        for at in reasons.iter() {
            match span.sites.iter_mut().find(|(a, _)| std::ptr::eq(*a, at)) {
                Some(e) => e.1 += 1,
                None => span.sites.push((at, 1)),
            }
        }
        span.more += reasons.more as u64;
        if let Some((p, most)) = past {
            span.past[p].0 += 1;
            span.past[p].1 = most;
        }
        let names: Vec<String> = reasons.iter().map(at_name).collect();
        add(totals, |t| {
            t.frames += 1;
            for name in names {
                *t.frame_sites.entry(name).or_default() += 1;
            }
        });
        out
    }

    /// Whether a timer (or a task) run at `now` is a purpose's: a tick covers it and has room
    fn tick_for(&mut self, now: Instant, timer: bool) -> bool {
        let stats = self.stats;
        let bump = |p: usize, i: usize| {
            if let Some(s) = stats {
                Stats::bump(&s.purposes[p][i], 1);
            }
        };
        for w in self.windows.values_mut() {
            if let Some(tick) = w.tick_at(now) {
                if !tick.allowed {
                    continue;
                }
                if timer && tick.timers < TICK_TIMERS {
                    tick.timers += 1;
                    bump(tick.purpose.index(), 3);
                    return true;
                }
                if !timer && tick.tasks < TICK_TASKS {
                    tick.tasks += 1;
                    bump(tick.purpose.index(), 4);
                    return true;
                }
            }
        }
        false
    }

    fn own(&self, at: At) -> bool {
        self.own_files.iter().any(|f| at.file() == *f)
    }

    /// A timer armed at `at` fired at `now`, with every window at rest
    pub fn timer_fired(&mut self, at: At, now: Instant) -> Vec<String> {
        let mut out = vec![];
        if !self.all_at_rest(now) || self.own(at) {
            return out;
        }
        self.close_spans(now, &mut out);
        if self.tick_for(now, true) {
            return out;
        }
        self.timers.get_or_insert_with(|| Runs::new(now)).add(at, 0);
        let name = at_name(at);
        self.total(|t| {
            t.timers += 1;
            *t.timer_sites.entry(name).or_default() += 1;
        });
        out
    }

    /// A task spawned at `at` ran `ns` at `now`, with every window at rest
    pub fn task_ran(&mut self, at: At, ns: u64, now: Instant) -> Vec<String> {
        let mut out = vec![];
        if !self.all_at_rest(now) || self.own(at) {
            return out;
        }
        self.close_spans(now, &mut out);
        if self.tick_for(now, false) {
            return out;
        }
        self.tasks.get_or_insert_with(|| Runs::new(now)).add(at, ns);
        let name = at_name(at);
        self.total(|t| {
            t.tasks += 1;
            t.task_ns += ns;
            let e = t.task_sites.entry(name).or_default();
            e.0 += 1;
            e.1 += ns;
        });
        out
    }

    /// Whether a window's span is over its budget, as it stands
    fn frames_over(&self, span: &Frames) -> bool {
        span.counted() > self.settings.rest_frames
    }

    fn timers_over(&self, runs: &Runs) -> bool {
        runs.count() > self.settings.rest_timers as u64
    }

    fn tasks_over(&self, runs: &Runs) -> bool {
        runs.count() > self.settings.rest_tasks as u64 || ms(runs.ns()) > self.settings.rest_task_ms
    }

    /// Ends the spans that are 10 s old by `now`, saying those over, and says what's held and due
    fn close_spans(&mut self, now: Instant, out: &mut Vec<String>) {
        let rest_frames = self.settings.rest_frames;
        let mut said: Vec<(usize, Frames, Option<u32>)> = vec![];
        let mut ids: Vec<usize> = self.windows.keys().copied().collect();
        ids.sort();
        for id in ids {
            let w = self.windows.get_mut(&id).unwrap();
            let mut lines = vec![];
            if w.span.as_ref().map_or(false, |s| now >= s.start + SPAN) {
                let span = w.span.take().unwrap();
                if span.counted() > rest_frames {
                    w.pace.over(span, now, &mut lines);
                }
            }
            w.pace.due(now, &mut lines);
            said.extend(lines.into_iter().map(|(f, spans)| (id, f, spans)));
        }
        for (id, frames, spans) in said {
            out.push(frames_line(id, &frames, spans));
        }

        let mut lines = vec![];
        if self.timers.as_ref().map_or(false, |r| now >= r.start + SPAN) {
            let runs = self.timers.take().unwrap();
            if self.timers_over(&runs) {
                self.timers_pace.over(runs, now, &mut lines);
            }
        }
        self.timers_pace.due(now, &mut lines);
        out.extend(lines.iter().map(|(r, spans)| runs_line("timers fired", r, *spans, false)));

        let mut lines = vec![];
        if self.tasks.as_ref().map_or(false, |r| now >= r.start + SPAN) {
            let runs = self.tasks.take().unwrap();
            if self.tasks_over(&runs) {
                self.tasks_pace.over(runs, now, &mut lines);
            }
        }
        self.tasks_pace.due(now, &mut lines);
        out.extend(lines.iter().map(|(r, spans)| runs_line("tasks ran", r, *spans, true)));

        for (i, pace) in self.paces.iter_mut().enumerate() {
            if let Some(p) = pace {
                if now >= p.start + SPAN {
                    if p.count > 1 {
                        out.push(format!(
                            "{}: {} times over its {} ms in the last {} s, max {:.1} ms",
                            Action::ALL[i].name(),
                            p.count,
                            p.budget_ms,
                            SPAN.as_secs(),
                            p.max_ms
                        ));
                    }
                    *pace = None;
                }
            }
        }
        if !out.is_empty() {
            let n = out.len() as u64;
            self.stat(|s| &s.lines, n);
        }
    }

    /// When something is next due to be said: a span over its budget ending, held lines, an
    /// action's 10 s with more than one over
    pub fn next_due(&self) -> Option<Instant> {
        let mut due: Option<Instant> = None;
        let mut at = |t: Option<Instant>| {
            if let Some(t) = t {
                due = Some(due.map_or(t, |d| d.min(t)));
            }
        };
        for w in self.windows.values() {
            at(w.span.as_ref().filter(|s| self.frames_over(s)).map(|s| s.start + SPAN));
            at(w.pace.next_due());
        }
        at(self.timers.as_ref().filter(|r| self.timers_over(r)).map(|r| r.start + SPAN));
        at(self.tasks.as_ref().filter(|r| self.tasks_over(r)).map(|r| r.start + SPAN));
        at(self.timers_pace.next_due());
        at(self.tasks_pace.next_due());
        for p in self.paces.iter().flatten().filter(|p| p.count > 1) {
            at(Some(p.start + SPAN));
        }
        due
    }

    /// The waker's timer: what's due by `now`
    pub fn wake(&mut self, now: Instant) -> Vec<String> {
        let mut out = vec![];
        self.close_spans(now, &mut out);
        self.expire(now);
        out
    }

    fn expire(&mut self, now: Instant) {
        if self.next_give_up.map_or(true, |at| now < at) {
            return;
        }
        let stats = self.stats;
        self.pending.retain(|p| {
            let keep = now < p.give_up;
            if !keep {
                if let Some(s) = stats {
                    Stats::bump(&s.actions[p.action.index()].unshown, 1);
                }
            }
            keep
        });
        self.next_give_up = self.pending.iter().map(|p| p.give_up).min();
    }

    /// `action` in `window`, started at `start`: a ticket to hand [`Budget::done`] with the
    /// frame that shows it, none when it isn't timed
    pub fn begin(&mut self, window: usize, action: Action, start: Instant, now: Instant) -> Option<u64> {
        let budget_ms = self.settings.action_ms(action);
        if !self.settings.enabled || budget_ms <= 0.0 {
            return None;
        }
        self.expire(now);
        if self.pending.len() >= MAX_PENDING {
            let old = self.pending.remove(0);
            self.stat(|s| &s.actions[old.action.index()].unshown, 1);
        }
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        let start = start.min(now);
        let waits = match action {
            Action::Key => Duration::from_secs_f64(budget_ms * KEY_WAITS / 1000.0).min(SHOWN_AT_MOST),
            _ => SHOWN_AT_MOST,
        };
        let give_up = start + waits;
        self.next_give_up = Some(self.next_give_up.map_or(give_up, |at| at.min(give_up)));
        self.pending.push(Pending {
            ticket,
            window,
            action,
            start,
            give_up,
            trace: Trace::default(),
        });
        Some(ticket)
    }

    /// What ran between `ticket`'s start and its being begun (a slow Lua handler before the
    /// tab switch it asks for), from the thread's recent runs: in its trace too
    pub fn seed(&mut self, ticket: u64, runs: impl Iterator<Item = (Site, u64)>) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.ticket == ticket) {
            for (site, ns) in runs {
                p.trace.add(site, ns);
            }
        }
    }

    /// Whether anything is waiting to be shown (the runs timed meanwhile are traced)
    pub fn tracing(&self) -> bool {
        !self.pending.is_empty()
    }

    /// When the oldest action waiting was begun
    pub fn oldest_pending(&self) -> Option<Instant> {
        self.pending.iter().map(|p| p.start).min()
    }

    /// A run of what slow.rs times, `ns` of its own, while actions wait
    pub fn trace(&mut self, site: Site, ns: u64, now: Instant) {
        self.expire(now);
        for p in &mut self.pending {
            p.trace.add(site, ns);
        }
    }

    /// The action `ticket` is waiting for won't be shown (its tab went, the window closed)
    pub fn drop_ticket(&mut self, ticket: u64) {
        if let Some(i) = self.pending.iter().position(|p| p.ticket == ticket) {
            let p = self.pending.remove(i);
            self.stat(|s| &s.actions[p.action.index()].unshown, 1);
        }
    }

    /// Whether `ticket` still waits (it may have been given up on)
    pub fn waiting(&self, ticket: u64) -> bool {
        self.pending.iter().any(|p| p.ticket == ticket)
    }

    /// The frame that shows `ticket`'s action ended at `now`: how long it took, and its line
    /// when that was over its time
    pub fn done(&mut self, ticket: u64, now: Instant, out: &mut Vec<String>) -> Option<Done> {
        let i = self.pending.iter().position(|p| p.ticket == ticket)?;
        let p = self.pending.remove(i);
        let a = p.action.index();
        // Shown after it was given up on (a key's program printing at last): not its time
        if now >= p.give_up {
            self.stat(|s| &s.actions[a].unshown, 1);
            return None;
        }
        let elapsed = now.saturating_duration_since(p.start);
        let budget_ms = self.settings.action_ms(p.action);
        let took_ms = elapsed.as_secs_f64() * 1000.0;
        let over = took_ms > budget_ms;
        let ns = elapsed.as_nanos() as u64;
        if let Some(s) = self.stats {
            s.actions[a].hist.record(elapsed);
            if over {
                Stats::bump(&s.actions[a].over, 1);
            }
        }
        if over {
            self.close_spans(now, out);
            match self.paces[a].as_mut() {
                Some(pace) => {
                    pace.count += 1;
                    pace.max_ms = pace.max_ms.max(took_ms);
                }
                None => {
                    self.paces[a] = Some(ActionPace {
                        start: now,
                        count: 1,
                        max_ms: took_ms,
                        budget_ms,
                    });
                    out.push(format!(
                        "{} took {:.1} ms, {:.1} ms over its {} ms: {}",
                        p.action.name(),
                        took_ms,
                        took_ms - budget_ms,
                        budget_ms,
                        p.trace.said(ns)
                    ));
                    self.stat(|s| &s.lines, 1);
                }
            }
        }
        Some(Done {
            action: p.action,
            window: p.window,
            start: p.start,
            elapsed,
            over,
        })
    }
}

/// A window's frames line: the first of a run of spans, or `spans` more said together
fn frames_line(window: usize, f: &Frames, spans: Option<u32>) -> String {
    let mut sites = f.named();
    if f.more > 0 {
        sites.push(("more".to_string(), f.more, 0));
    }
    let mut line = match spans {
        None => format!(
            "window-{window} painted {} frames in {} s at rest, asked by {}",
            f.counted(),
            SPAN.as_secs(),
            site_list(&sites, false)
        ),
        Some(n) => format!(
            "window-{window} painted {} frames at rest in {n} more span{} of {} s in the last {} s, asked by {}",
            f.counted(),
            if n == 1 { "" } else { "s" },
            SPAN.as_secs(),
            LINE_EVERY.as_secs(),
            site_list(&sites, false)
        ),
    };
    if f.unasked > 0 {
        line.push_str(&format!("; {} of them nobody asked for, Windows' own {UNASKED}", f.unasked));
    }
    if f.news_past > 0 {
        line.push_str(&format!(
            "; {} of them after background tabs' news, past the {} it may bring in {} s",
            f.news_past,
            f.news,
            SPAN.as_secs()
        ));
    }
    for p in Purpose::ALL {
        let (frames, most) = f.past[p.index()];
        if frames > 0 {
            line.push_str(&format!(
                "; {frames} of them on {} ticks past the {most} it may make in {} s",
                p.name(),
                SPAN.as_secs()
            ));
        }
    }
    let mut purposed: Vec<String> = Purpose::ALL
        .iter()
        .filter(|p| f.on_purpose[p.index()] > 0)
        .map(|p| format!("{} {}", p.name(), f.on_purpose[p.index()]))
        .collect();
    if f.news > 0 {
        purposed.push(format!("news {}", f.news));
    }
    if !purposed.is_empty() {
        line.push_str(&format!("; on purpose, allowed: {}", purposed.join(", ")));
    }
    line
}

/// The timers' or the tasks' line
fn runs_line(what: &str, r: &Runs, spans: Option<u32>, times: bool) -> String {
    let all = if times { format!(", {:.1} ms in all", ms(r.ns())) } else { String::new() };
    match spans {
        None => format!(
            "{what} {} times in {} s at rest{all}: {}",
            r.count(),
            SPAN.as_secs(),
            site_list(&r.named(), times)
        ),
        Some(n) => format!(
            "{what} {} times at rest in {n} more span{} of {} s in the last {} s{all}: {}",
            r.count(),
            if n == 1 { "" } else { "s" },
            SPAN.as_secs(),
            LINE_EVERY.as_secs(),
            site_list(&r.named(), times)
        ),
    }
}

// ---- The GUI thread's budget, and the hot paths into it ----

static BASE: OnceLock<Instant> = OnceLock::new();

/// When the process stops being busy, in nanoseconds from [`BASE`]; `u64::MAX` (never) while
/// the budget is off
static BUSY_UNTIL: AtomicU64 = AtomicU64::new(u64::MAX);

static WAKER: OnceLock<fn(Duration)> = OnceLock::new();

static TOTALS: OnceLock<Mutex<Totals>> = OnceLock::new();

fn totals() -> &'static Mutex<Totals> {
    TOTALS.get_or_init(Default::default)
}

/// The process's actions, purposes, news and lines, in atomics
static STATS: Stats = Stats::new();

/// The settings the GUI thread's budget was last handed, for `metrics`
static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);

thread_local! {
    static BUDGET: RefCell<Budget> = RefCell::new(Budget::new(Settings::off()));
    /// Whether an action waits, so every run timed is traced
    static TRACING: Cell<bool> = const { Cell::new(false) };
    /// When the waker's timer is due, while one is set
    static WAKE_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    /// The thread's last runs, for what an action begun late was waiting on before it
    static RECENT_RUNS: RefCell<Recent> = const { RefCell::new(Recent::new()) };
}

/// How many of the thread's last runs are kept
const RECENT: usize = 256;

/// The thread's last runs of what slow.rs times, each with when it ended, oldest overwritten
pub struct Recent {
    runs: [(Option<Site>, u64, Option<Instant>); RECENT],
    next: usize,
}

impl Recent {
    pub const fn new() -> Self {
        Self {
            runs: [(None, 0, None); RECENT],
            next: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, site: Site, ns: u64, end: Instant) {
        self.runs[self.next] = (Some(site), ns, Some(end));
        self.next = (self.next + 1) % RECENT;
    }

    /// The runs that ended after `start`, oldest first
    pub fn since(&self, start: Instant) -> Vec<(Site, u64)> {
        (0..RECENT)
            .map(|k| &self.runs[(self.next + k) % RECENT])
            .filter_map(|(site, ns, end)| match (site, end) {
                (Some(site), Some(end)) if *end > start => Some((*site, *ns)),
                _ => None,
            })
            .collect()
    }
}

fn nanos(t: Instant) -> u64 {
    let base = *BASE.get_or_init(Instant::now);
    t.saturating_duration_since(base).as_nanos() as u64
}

/// Whether the whole process is at rest at `now`: one compare
#[inline]
pub fn at_rest(now: Instant) -> bool {
    let until = BUSY_UNTIL.load(Relaxed);
    until != u64::MAX && nanos(now) >= until
}

fn emit(lines: &[String]) {
    for line in lines {
        log::warn!(target: TARGET, "{line}");
    }
}

/// After a change: the process's busy time, whether runs are traced, and the waker's timer
fn settle(b: &Budget, now: Instant) {
    let until = if b.settings.enabled && !b.windows.is_empty() {
        nanos(b.busy_until().unwrap_or(now))
    } else {
        u64::MAX
    };
    BUSY_UNTIL.store(until, Relaxed);
    TRACING.with(|t| t.set(b.tracing()));
    if let (Some(due), Some(waker)) = (b.next_due(), WAKER.get()) {
        let set = WAKE_AT.with(|w| w.get());
        if set.map_or(true, |at| at <= now || due < at) {
            WAKE_AT.with(|w| w.set(Some(due)));
            waker(due.saturating_duration_since(now));
        }
    }
}

fn with<R>(now: Instant, f: impl FnOnce(&mut Budget) -> (R, Vec<String>)) -> Option<R> {
    let (r, lines) = BUDGET
        .try_with(|b| {
            let mut b = b.try_borrow_mut().ok()?;
            let (r, lines) = f(&mut b);
            settle(&b, now);
            Some((r, lines))
        })
        .ok()
        .flatten()?;
    emit(&lines);
    Some(r)
}

/// The GUI starts: what sets a timer to call [`wake_up`] after a while, and the file its task is
/// spawned in, whose tasks and timers aren't counted at rest
pub fn set_waker(waker: fn(Duration), own_file: &'static str) {
    WAKER.set(waker).ok();
    own(own_file);
}

/// What's spawned or armed in `file` is the measuring's own (the slow log's waker, in
/// window/src/spawn.rs): not counted at rest
pub fn own(file: &'static str) {
    let _ = BUDGET.try_with(|b| {
        if let Ok(mut b) = b.try_borrow_mut() {
            if !b.own_files.contains(&file) {
                b.own_files.push(file);
            }
            b.totals = Some(totals());
            b.stats = Some(&STATS);
        }
    });
}

/// The settings (config's `lbterm_budget`), on the GUI thread, at start and on each reload
pub fn configure(settings: Settings) {
    let now = Instant::now();
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(settings.clone());
    with(now, |b| {
        let was = b.settings.enabled;
        b.configure(settings);
        b.totals = Some(totals());
        b.stats = Some(&STATS);
        // Starting is busy: every window's first frames
        if b.settings.enabled && !was {
            for id in b.windows.keys().copied().collect::<Vec<_>>() {
                b.activity(id, now);
            }
        }
        ((), vec![])
    });
}

/// Something happened to `window`
pub fn activity(window: usize) {
    let now = Instant::now();
    with(now, |b| (b.activity(window, now), vec![]));
}

/// A background tab of `window` said something; `repaints` when it's an alert its rail row may
/// show ([`Budget::news`])
pub fn news(window: usize, repaints: bool) {
    let now = Instant::now();
    with(now, |b| (b.news(window, repaints, now), vec![]));
}

/// Whether `window` is busy now (so a frame needs no more judging)
pub fn busy(window: usize, now: Instant) -> bool {
    BUDGET
        .try_with(|b| b.try_borrow().map_or(true, |b| b.busy(window, now)))
        .unwrap_or(true)
}

/// How long `window` stays busy from `now`
pub fn busy_left(window: usize, now: Instant) -> Duration {
    BUDGET
        .try_with(|b| b.try_borrow().map_or(Duration::ZERO, |b| b.busy_left(window, now)))
        .unwrap_or(Duration::ZERO)
}

pub fn forget(window: usize) {
    let now = Instant::now();
    with(now, |b| (b.forget(window), vec![]));
}

/// `purpose` repaints `window` at `due`, at most `per_second` a second
pub fn tick(window: usize, purpose: Purpose, due: Instant, per_second: f64) {
    let now = Instant::now();
    with(now, |b| (b.tick(window, purpose, due, per_second, now), vec![]));
}

/// `window` paints a frame at `now`
pub fn frame(window: usize, now: Instant, reasons: &Reasons, in_flight: impl FnOnce() -> Option<Instant>) {
    with(now, |b| ((), b.frame(window, now, reasons, in_flight)));
}

/// What slow.rs timed, `ns` of its own, ending at `now` (slow.rs's scopes call this)
#[inline]
pub fn timed(site: Site, ns: u64, now: Instant) {
    if BUSY_UNTIL.load(Relaxed) == u64::MAX {
        return;
    }
    let _ = RECENT_RUNS.try_with(|r| {
        if let Ok(mut r) = r.try_borrow_mut() {
            r.push(site, ns, now);
        }
    });
    if TRACING.with(|t| t.get()) {
        let _ = BUDGET.try_with(|b| {
            if let Ok(mut b) = b.try_borrow_mut() {
                b.trace(site, ns, now);
                if !b.tracing() {
                    TRACING.with(|t| t.set(false));
                }
            }
        });
    }
    if let Site::Task(at) = site {
        if at_rest(now) {
            task_ran(at, ns, now);
        }
    }
}

#[cold]
fn task_ran(at: At, ns: u64, now: Instant) {
    with(now, |b| ((), b.task_ran(at, ns, now)));
}

/// A timer armed at `at` fired (counters.rs's timers call this)
#[inline]
pub fn fired(at: At) {
    if BUSY_UNTIL.load(Relaxed) == u64::MAX {
        return;
    }
    let now = Instant::now();
    if at_rest(now) {
        timer_fired(at, now);
    }
}

#[cold]
fn timer_fired(at: At, now: Instant) {
    with(now, |b| ((), b.timer_fired(at, now)));
}

/// The waker's timer went off
pub fn wake_up() {
    let now = Instant::now();
    WAKE_AT.with(|w| w.set(None));
    with(now, |b| ((), b.wake(now)));
}

/// `action` in `window` started at `start`: what ran on this thread since then is in its trace
pub fn begin(window: usize, action: Action, start: Instant) -> Option<u64> {
    let now = Instant::now();
    let ticket = with(now, |b| (b.begin(window, action, start, now), vec![])).flatten()?;
    // Begun where it started (a key in its own message, a paint), nothing ran before it
    if now.saturating_duration_since(start) > Duration::from_millis(1) {
        let earlier = RECENT_RUNS
            .try_with(|r| r.try_borrow().map(|r| r.since(start)).unwrap_or_default())
            .unwrap_or_default();
        with(now, |b| (b.seed(ticket, earlier.into_iter()), vec![]));
    }
    Some(ticket)
}

/// The frame showing `ticket`'s action has ended
pub fn done(ticket: u64, now: Instant) -> Option<Done> {
    with(now, |b| {
        let mut out = vec![];
        let d = b.done(ticket, now, &mut out);
        (d, out)
    })
    .flatten()
}

/// `ticket`'s action won't be shown
pub fn drop_ticket(ticket: u64) {
    let now = Instant::now();
    with(now, |b| (b.drop_ticket(ticket), vec![]));
}

/// Whether `ticket` still waits
pub fn waiting(ticket: u64) -> bool {
    BUDGET
        .try_with(|b| b.try_borrow().map_or(false, |b| b.waiting(ticket)))
        .unwrap_or(false)
}

/// What `metrics` replies with under `budget`, since the process started or the last reset
pub fn report_json(reset: bool) -> Value {
    let settings = SETTINGS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(Settings::off);
    report(&settings, totals(), &STATS, reset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slow::Part;

    #[track_caller]
    fn here() -> At {
        Location::caller()
    }

    fn name(at: At) -> String {
        format!("lbterm-extras/src/budget.rs:{}", at.line())
    }

    fn settings() -> Settings {
        let mut actions = [0.0; Action::COUNT];
        actions[Action::Tab.index()] = 30.0;
        actions[Action::Key.index()] = 20.0;
        Settings {
            enabled: true,
            quiet: Duration::from_millis(1000),
            rest_frames: 5,
            rest_news_frames: 3,
            rest_timers: 10,
            rest_tasks: 20,
            rest_task_ms: 20.0,
            actions,
        }
    }

    fn ms_(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    fn asked(sites: &[At]) -> Reasons {
        let mut r = Reasons::default();
        for at in sites {
            r.push(at);
        }
        r
    }

    /// A budget whose window 0 was last busy at `t0`
    fn rested(t0: Instant) -> Budget {
        let mut b = Budget::new(settings());
        b.activity(0, t0);
        b
    }

    #[test]
    fn a_window_painting_at_rest_names_who_asked_when_its_10_s_end() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let a = here();
        let c = here();
        let mut out = vec![];
        // 1 s on, at rest: 30 frames a second asked by a, every tenth by c too
        for i in 0..300u64 {
            let r = if i % 10 == 0 { asked(&[a, c]) } else { asked(&[a]) };
            out.extend(b.frame(0, ms_(t0, 1000 + i * 33), &r, || None));
        }
        assert!(out.is_empty(), "said only as the span ends: {out:?}");
        assert_eq!(b.next_due(), Some(ms_(t0, 11_000)), "a span over its budget ends then");
        let out = b.wake(ms_(t0, 11_000));
        assert_eq!(
            out,
            [format!("window-0 painted 300 frames in 10 s at rest, asked by {} (300), {} (30)", name(a), name(c))]
        );
    }

    #[test]
    fn a_few_frames_at_rest_are_not_said() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let a = here();
        for i in 0..5u64 {
            b.frame(0, ms_(t0, 2000 + i * 1000), &asked(&[a]), || None);
        }
        assert_eq!(b.next_due(), None, "five is the budget");
        assert!(b.wake(ms_(t0, 20_000)).is_empty());
    }

    #[test]
    fn busy_windows_and_what_is_in_flight_are_not_at_rest() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let a = here();
        // Within quiet of the activity: busy, in_flight not even asked
        b.frame(0, ms_(t0, 500), &asked(&[a]), || panic!("not asked while busy"));
        // A wait under way until 3 s: busy until quiet after it, without asking again
        b.frame(0, ms_(t0, 1500), &asked(&[a]), || Some(ms_(t0, 3000)));
        b.frame(0, ms_(t0, 3500), &asked(&[a]), || panic!("busy until 4 s"));
        assert!(b.busy(0, ms_(t0, 3999)));
        assert!(!b.busy(0, ms_(t0, 4000)));
        // In flight once, long ago: at rest
        for i in 0..10u64 {
            b.frame(0, ms_(t0, 4000 + i * 10), &asked(&[a]), || Some(t0));
        }
        let out = b.wake(ms_(t0, 15_000));
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].starts_with("window-0 painted 10 frames in 10 s at rest"), "{}", out[0]);
    }

    #[test]
    fn a_purpose_s_ticks_bring_their_frames_timers_and_tasks_up_to_its_rate() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let inv = here();
        let timer = here();
        let task = here();
        let mut out = vec![];
        // A clock at one a second for 10 s: 11 ticks allowed in a span
        for s in 2..14u64 {
            let due = ms_(t0, s * 1000);
            b.tick(0, Purpose::Clock, due, 1.0, ms_(t0, s * 1000 - 990));
            out.extend(b.timer_fired(timer, ms_(t0, s * 1000 + 1)));
            for k in 0..3 {
                out.extend(b.task_ran(task, 10_000, ms_(t0, s * 1000 + 2 + k)));
            }
            out.extend(b.frame(0, ms_(t0, s * 1000 + 5), &asked(&[inv]), || None));
        }
        out.extend(b.wake(ms_(t0, 30_000)));
        assert!(out.is_empty(), "all on purpose: {out:?}");

        // Ticking 30 times a second while saying one: past its rate, the rest at rest
        let mut b = rested(t0);
        for i in 0..300u64 {
            let due = ms_(t0, 2000 + i * 33);
            b.tick(0, Purpose::Clock, due, 1.0, due);
            out.extend(b.timer_fired(timer, due));
            out.extend(b.task_ran(task, 10_000, due));
            out.extend(b.frame(0, ms_(t0, 2000 + i * 33 + 2), &asked(&[inv]), || None));
        }
        out.extend(b.wake(ms_(t0, 13_000)));
        let frames = out.iter().find(|l| l.starts_with("window-0")).expect("frames said");
        assert!(
            frames.starts_with(&format!("window-0 painted 289 frames in 10 s at rest, asked by {} (289)", name(inv))),
            "{frames}"
        );
        assert!(frames.contains("; 289 of them on clock ticks past the 11 it may make in 10 s"), "{frames}");
        assert!(frames.ends_with("; on purpose, allowed: clock 11"), "{frames}");
        let timers = out.iter().find(|l| l.starts_with("timers")).expect("timers said");
        assert_eq!(timers, &format!("timers fired 289 times in 10 s at rest: {} (289)", name(timer)));
        let tasks = out.iter().find(|l| l.starts_with("tasks")).expect("tasks said");
        assert_eq!(tasks, &format!("tasks ran 289 times in 10 s at rest, 2.9 ms in all: {} (289, 2.9 ms)", name(task)));
    }

    #[test]
    fn a_tick_s_window_holds_only_so_much() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let inv = here();
        let due = ms_(t0, 5000);
        b.tick(0, Purpose::Status, due, 1.0, ms_(t0, 4000));
        b.tick(0, Purpose::Status, due, 1.0, ms_(t0, 4500));
        for i in 0..8u64 {
            b.frame(0, ms_(t0, 5000 + i * 10), &asked(&[inv]), || None);
        }
        let out = b.wake(ms_(t0, 20_000));
        assert_eq!(out.len(), 1, "two frames a tick, six at rest: {out:?}");
        assert!(out[0].starts_with("window-0 painted 6 frames in 10 s at rest"), "{}", out[0]);
        assert!(out[0].ends_with("; on purpose, allowed: status 2"), "{}", out[0]);
    }

    #[test]
    fn work_at_rest_only_counts_while_every_window_rests() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let task = here();
        b.activity(1, ms_(t0, 5000));
        for i in 0..100u64 {
            b.task_ran(task, 1_000, ms_(t0, 5000 + i * 10));
        }
        assert!(b.wake(ms_(t0, 30_000)).is_empty(), "window 1 was busy");
        // The budget's own waker isn't counted
        b.own_files = vec![task.file()];
        for i in 0..100u64 {
            b.task_ran(task, 1_000, ms_(t0, 30_000 + i * 10));
        }
        assert!(b.wake(ms_(t0, 60_000)).is_empty(), "its own");
    }

    #[test]
    fn tasks_over_their_time_are_said_though_few() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let task = here();
        for i in 0..3u64 {
            b.task_ran(task, 9_000_000, ms_(t0, 2000 + i * 1000));
        }
        let out = b.wake(ms_(t0, 12_000));
        assert_eq!(out, [format!("tasks ran 3 times in 10 s at rest, 27.0 ms in all: {} (3, 27.0 ms)", name(task))]);
    }

    #[test]
    fn the_first_span_over_is_said_at_once_the_rest_once_a_minute() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let a = here();
        let mut out = vec![];
        // Painting 10 frames a second at rest for 3 minutes: spans end at 11 s, 21 s, ...
        for i in 0..1800u64 {
            out.extend(b.frame(0, ms_(t0, 1000 + i * 100), &asked(&[a]), || None));
        }
        assert_eq!(out[0], format!("window-0 painted 100 frames in 10 s at rest, asked by {} (100)", name(a)));
        // A minute after the first, the five spans that ended since
        assert_eq!(
            out[1],
            format!("window-0 painted 500 frames at rest in 5 more spans of 10 s in the last 60 s, asked by {} (500)", name(a))
        );
        assert_eq!(
            out[2],
            format!("window-0 painted 600 frames at rest in 6 more spans of 10 s in the last 60 s, asked by {} (600)", name(a))
        );
        assert_eq!(out.len(), 3, "{out:#?}");
        // Then it stops: what's held is said at its minute
        let mut rest = vec![];
        while let Some(due) = b.next_due() {
            rest.extend(b.wake(due));
        }
        assert_eq!(
            rest,
            [format!("window-0 painted 600 frames at rest in 6 more spans of 10 s in the last 60 s, asked by {} (600)", name(a))]
        );
        // And quiet a minute, the next is a first again
        for i in 0..100u64 {
            b.frame(0, ms_(t0, 400_000 + i * 50), &asked(&[a]), || None);
        }
        let again = b.wake(ms_(t0, 410_000));
        assert!(again[0].starts_with("window-0 painted 100 frames in 10 s"), "{again:?}");
    }

    #[test]
    fn frames_nobody_asked_for_are_counted_as_windows_own_wm_paint() {
        let t0 = Instant::now();
        let mut b = rested(t0);
        let a = here();
        // A few, as a window shown or sized brings: under the budget
        for i in 0..5u64 {
            b.frame(0, ms_(t0, 2000 + i * 100), &Reasons::default(), || None);
        }
        assert!(b.wake(ms_(t0, 13_000)).is_empty(), "five is the budget");
        // A paint Windows sends again and again (one never validated) is said like any other
        for i in 0..20u64 {
            b.frame(0, ms_(t0, 14_000 + i * 100), &Reasons::default(), || None);
        }
        let out = b.wake(ms_(t0, 25_000));
        assert_eq!(out, ["window-0 painted 20 frames in 10 s at rest, asked by WM_PAINT (20); 20 of them nobody asked for, Windows' own WM_PAINT"]);
        for i in 0..8u64 {
            b.frame(0, ms_(t0, 30_000 + i * 100), &asked(&[a]), || None);
            b.frame(0, ms_(t0, 30_050 + i * 100), &Reasons::default(), || None);
        }
        assert!(b.wake(ms_(t0, 41_000)).is_empty(), "held: said a minute after the first");
        let out = b.wake(ms_(t0, 85_000));
        assert_eq!(
            out,
            [format!(
                "window-0 painted 16 frames at rest in 1 more span of 10 s in the last 60 s, asked by {} (8), WM_PAINT (8); 8 of them nobody asked for, Windows' own WM_PAINT",
                name(a)
            )]
        );
    }

    #[test]
    fn a_purpose_a_little_faster_than_it_says_loses_only_its_extra_ticks() {
        let t0 = Instant::now();
        let inv = here();
        let timer = here();
        let task = here();
        let mut out = vec![];
        // A blink every 833 ms (1.2 a second, 20% over) said at 1 a second, for a minute: about
        // one tick in ten past the rate, its frame at rest, and no span over rest_frames
        let mut b = rested(t0);
        for k in 0..72u64 {
            let due = ms_(t0, 2000 + k * 833);
            b.tick(0, Purpose::Animation, due, 1.0, due - Duration::from_millis(800));
            out.extend(b.timer_fired(timer, due));
            out.extend(b.task_ran(task, 10_000, due + Duration::from_millis(1)));
            out.extend(b.task_ran(task, 10_000, due + Duration::from_millis(2)));
            out.extend(b.frame(0, due + Duration::from_millis(3), &asked(&[inv]), || None));
        }
        out.extend(b.wake(ms_(t0, 75_000)));
        assert!(out.is_empty(), "only its extra ticks at rest: {out:?}");
        // 10% over, the same
        let mut b = rested(t0);
        for k in 0..66u64 {
            let due = ms_(t0, 2000 + k * 909);
            b.tick(0, Purpose::Animation, due, 1.0, due - Duration::from_millis(800));
            out.extend(b.frame(0, due + Duration::from_millis(3), &asked(&[inv]), || None));
        }
        out.extend(b.wake(ms_(t0, 75_000)));
        assert!(out.is_empty(), "{out:?}");
        // Twice its rate is still said: half its frames at rest
        let mut b = rested(t0);
        for k in 0..40u64 {
            let due = ms_(t0, 2000 + k * 500);
            b.tick(0, Purpose::Animation, due, 1.0, due - Duration::from_millis(400));
            out.extend(b.frame(0, due + Duration::from_millis(3), &asked(&[inv]), || None));
        }
        out.extend(b.wake(ms_(t0, 30_000)));
        let frames = out.iter().find(|l| l.starts_with("window-0")).expect("said");
        assert!(frames.contains("of them on animation ticks past the 11 it may make in 10 s"), "{frames}");
    }

    #[test]
    fn a_blink_s_tick_says_its_own_rate_not_animation_fps() {
        // Lachlan's animation_fps 1, a cursor blinking at 800 ms (the fastest blink 4 a second)
        assert!((animation_rate(Duration::from_millis(800), 1.0, 4.0) - 1.25).abs() < 1e-9);
        // A frame painted late in a phase waits less: no faster than the fastest blink
        assert_eq!(animation_rate(Duration::from_millis(10), 1.0, 4.0), 4.0);
        // An easing at 60 waits 16 ms: 60, never under animation_fps
        assert_eq!(animation_rate(Duration::from_millis(16), 60.0, 60.0), 60.0);
        assert_eq!(animation_rate(Duration::from_secs(5), 1.0, 4.0), 1.0);
        assert_eq!(animation_rate(Duration::ZERO, 1.0, 4.0), 4.0);
    }

    #[test]
    fn a_background_tab_s_news_keeps_the_process_busy_but_not_its_window_s_frames() {
        let t0 = Instant::now();
        let seat = here();
        let rail = here();
        let task = here();
        // A background tab printing all along: the process isn't at rest, so its tasks (the
        // news's own) aren't counted, but a frame nothing showed news for is judged
        let mut b = rested(t0);
        let mut out = vec![];
        for i in 0..100u64 {
            let at = ms_(t0, 2000 + i * 100);
            b.news(0, false, at);
            out.extend(b.task_ran(task, 10_000, at + Duration::from_millis(1)));
            if i % 4 == 0 {
                out.extend(b.frame(0, at + Duration::from_millis(50), &asked(&[seat]), || None));
            }
        }
        out.extend(b.wake(ms_(t0, 13_000)));
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].starts_with(&format!("window-0 painted 25 frames in 10 s at rest, asked by {} (25)", name(seat))), "{}", out[0]);
        // A background tab's alert: the frames just after it are its, up to rest_news_frames
        let mut b = rested(t0);
        let mut out = vec![];
        for i in 0..5u64 {
            let at = ms_(t0, 2000 + i * 1000);
            b.news(0, true, at);
            out.extend(b.frame(0, at + Duration::from_millis(20), &asked(&[rail]), || None));
        }
        out.extend(b.wake(ms_(t0, 13_000)));
        assert!(out.is_empty(), "three allowed, two at rest: under rest_frames: {out:?}");
        // Past the allowance they're counted, and said so
        let mut b = rested(t0);
        let mut out = vec![];
        for i in 0..12u64 {
            let at = ms_(t0, 2000 + i * 500);
            b.news(0, true, at);
            out.extend(b.frame(0, at + Duration::from_millis(20), &asked(&[rail]), || None));
        }
        out.extend(b.wake(ms_(t0, 13_000)));
        assert_eq!(
            out,
            [format!(
                "window-0 painted 9 frames in 10 s at rest, asked by {} (9); 9 of them after background tabs' news, past the 3 it may bring in 10 s; on purpose, allowed: news 3",
                name(rail)
            )]
        );
        // And a frame well after the alert isn't its
        let mut b = rested(t0);
        let mut out = vec![];
        for i in 0..8u64 {
            let at = ms_(t0, 2000 + i * 1000);
            b.news(0, true, at);
            out.extend(b.frame(0, at + NEWS_WINDOW + Duration::from_millis(50), &asked(&[seat]), || None));
        }
        out.extend(b.wake(ms_(t0, 13_000)));
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn a_key_whose_program_never_echoes_is_given_up_on_without_a_line() {
        let t0 = Instant::now();
        let stats: &'static Stats = Box::leak(Box::default());
        let mut b = Budget::new(settings());
        b.stats = Some(stats);
        let mut out = vec![];
        // The key's time is 20 ms: given up at 60 ms, and what its pane prints after isn't its
        let k = b.begin(0, Action::Key, t0, t0).unwrap();
        assert_eq!(b.done(k, ms_(t0, 3000), &mut out), None);
        assert!(out.is_empty(), "{out:?}");
        // Nor said when a run is traced after it
        let k = b.begin(0, Action::Key, ms_(t0, 4000), ms_(t0, 4000)).unwrap();
        b.trace(Site::Message(0x0100), 1_000, ms_(t0, 4061));
        assert!(!b.waiting(k));
        // One echoed within its time is timed as before, and one over it (but not given up) said
        let k = b.begin(0, Action::Key, ms_(t0, 5000), ms_(t0, 5000)).unwrap();
        assert!(b.done(k, ms_(t0, 5015), &mut out).is_some());
        let k = b.begin(0, Action::Key, ms_(t0, 6000), ms_(t0, 6000)).unwrap();
        assert!(b.done(k, ms_(t0, 6045), &mut out).unwrap().over);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].starts_with("key took 45.0 ms, 25.0 ms over its 20 ms"), "{}", out[0]);
        let j = report(b.settings(), &Mutex::new(Totals::default()), stats, false);
        let key = j["actions"].as_array().unwrap().iter().find(|x| x["action"] == "key").unwrap().clone();
        assert_eq!((key["count"].clone(), key["over"].clone(), key["unshown"].clone()), (json!(2), json!(1), json!(2)));
    }

    #[test]
    fn an_action_under_its_time_says_nothing_and_one_over_says_what_the_time_went_on() {
        let t0 = Instant::now();
        let mut b = Budget::new(settings());
        let at = here();
        let mut out = vec![];
        let k = b.begin(0, Action::Tab, t0, t0).unwrap();
        b.trace(Site::Task(at), 2_000_000, ms_(t0, 5));
        let d = b.done(k, ms_(t0, 10), &mut out).unwrap();
        assert_eq!((d.action, d.over), (Action::Tab, false));
        assert!(out.is_empty());

        let k = b.begin(0, Action::Tab, ms_(t0, 1000), ms_(t0, 1000)).unwrap();
        assert!(b.tracing());
        b.trace(Site::Task(at), 51_200_000, ms_(t0, 1060));
        b.trace(Site::Paint { window: 0, part: Part::Pass }, 10_000_000, ms_(t0, 1070));
        b.trace(Site::Paint { window: 0, part: Part::Pass }, 10_100_000, ms_(t0, 1080));
        b.trace(Site::Message(0x000F), 2_000_000, ms_(t0, 1081));
        b.trace(Site::Message(0x0200), 30_000, ms_(t0, 1082));
        let d = b.done(k, ms_(t0, 1084), &mut out).unwrap();
        assert!(d.over);
        assert!(!b.tracing());
        assert_eq!(
            out,
            [format!(
                "tab took 84.0 ms, 54.0 ms over its 30 ms: task {} 51.2 ms, paint window-0/pass 20.1 ms (2), message WM_PAINT 2.0 ms, the rest 10.7 ms",
                name(at)
            )]
        );
    }

    #[test]
    fn what_ran_before_an_action_was_begun_is_in_its_trace() {
        let t0 = Instant::now();
        let at = here();
        let mut recent = Recent::new();
        recent.push(Site::Message(0x0200), 1_000_000, ms_(t0, 5));
        recent.push(Site::Task(at), 60_000_000, ms_(t0, 70));
        // The ring keeps the last 256: 255 more and the move falls out, one more and the task
        for i in 0..RECENT as u64 - 1 {
            recent.push(Site::Message(0x0113), 1_000, ms_(t0, 80 + i));
        }
        assert_eq!(recent.since(ms_(t0, 1)).len(), RECENT, "the move fell out");
        recent.push(Site::Message(0x0113), 1_000, ms_(t0, 400));
        assert!(recent.since(ms_(t0, 1)).iter().all(|(site, _)| *site == Site::Message(0x0113)), "and the task");
        let mut recent = Recent::new();
        recent.push(Site::Message(0x0200), 1_000_000, ms_(t0, 5));
        recent.push(Site::Task(at), 60_000_000, ms_(t0, 70));
        assert_eq!(recent.since(ms_(t0, 10)), [(Site::Task(at), 60_000_000)], "only what ended after the start");

        let mut b = Budget::new(settings());
        let k = b.begin(0, Action::Tab, ms_(t0, 10), ms_(t0, 71)).unwrap();
        b.seed(k, recent.since(ms_(t0, 10)).into_iter());
        let mut out = vec![];
        b.done(k, ms_(t0, 85), &mut out);
        assert_eq!(out, [format!("tab took 75.0 ms, 45.0 ms over its 30 ms: task {} 60.0 ms, the rest 15.0 ms", name(at))]);
    }

    #[test]
    fn more_actions_over_in_10_s_are_added_up_at_their_end() {
        let t0 = Instant::now();
        let mut b = Budget::new(settings());
        let mut out = vec![];
        for (i, took) in [40u64, 90, 35].into_iter().enumerate() {
            let start = ms_(t0, i as u64 * 2000);
            let k = b.begin(0, Action::Tab, start, start).unwrap();
            b.done(k, start + Duration::from_millis(took), &mut out);
        }
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(b.next_due(), Some(ms_(t0, 10_040)));
        let more = b.wake(ms_(t0, 10_040));
        assert_eq!(more, ["tab: 3 times over its 30 ms in the last 10 s, max 90.0 ms"]);
    }

    #[test]
    fn untimed_actions_and_ones_never_shown() {
        let t0 = Instant::now();
        let mut b = Budget::new(settings());
        assert_eq!(b.begin(0, Action::Split, t0, t0), None, "0 ms: not timed");
        let k = b.begin(0, Action::Key, t0, t0).unwrap();
        b.trace(Site::Message(0x0100), 1_000, ms_(t0, 10_001));
        assert!(!b.waiting(k), "given up on after 10 s");
        let mut out = vec![];
        assert_eq!(b.done(k, ms_(t0, 10_002), &mut out), None);
        let k = b.begin(3, Action::Key, t0, t0).unwrap();
        b.forget(3);
        assert!(!b.waiting(k), "its window went");
    }

    #[test]
    fn off_judges_nothing() {
        let t0 = Instant::now();
        let mut b = Budget::new(Settings::off());
        let a = here();
        b.activity(0, t0);
        for i in 0..500u64 {
            b.frame(0, ms_(t0, 2000 + i * 8), &asked(&[a]), || None);
            b.task_ran(a, 1_000_000, ms_(t0, 2000 + i * 8));
        }
        assert!(b.wake(ms_(t0, 60_000)).is_empty());
        assert_eq!(b.begin(0, Action::Tab, t0, t0), None);
        assert!(b.busy(0, t0), "off is never at rest");
    }

    #[test]
    fn the_totals_metrics_reports() {
        let totals: &'static Mutex<Totals> = Box::leak(Box::default());
        let stats: &'static Stats = Box::leak(Box::default());
        let t0 = Instant::now();
        let mut b = rested(t0);
        b.totals = Some(totals);
        b.stats = Some(stats);
        let a = here();
        b.tick(0, Purpose::Clock, ms_(t0, 2000), 1.0, ms_(t0, 1500));
        b.frame(0, ms_(t0, 2001), &asked(&[a]), || None);
        b.frame(0, ms_(t0, 3000), &asked(&[a]), || None);
        b.task_ran(a, 2_000_000, ms_(t0, 3001));
        let k = b.begin(0, Action::Tab, ms_(t0, 4000), ms_(t0, 4000)).unwrap();
        let mut out = vec![];
        b.done(k, ms_(t0, 4040), &mut out);
        let j = report(b.settings(), totals, stats, true);
        assert_eq!(j["at_rest"]["frames"], json!(1));
        assert_eq!(j["at_rest"]["frame_sites"], json!([{ "site": name(a), "frames": 1 }]));
        assert_eq!(j["at_rest"]["task_sites"], json!([{ "site": name(a), "runs": 1, "ms": 2.0 }]));
        assert_eq!(j["on_purpose"], json!([{ "purpose": "clock", "ticks": 1, "past_rate": 0, "frames": 1, "timers": 0, "tasks": 0 }]));
        let tab = j["actions"].as_array().unwrap().iter().find(|x| x["action"] == "tab").unwrap().clone();
        assert_eq!(tab["action"], json!("tab"));
        assert_eq!((tab["count"].clone(), tab["over"].clone(), tab["unshown"].clone()), (json!(1), json!(1), json!(0)));
        assert_eq!((tab["mean_ms"].clone(), tab["max_ms"].clone()), (json!(40.0), json!(40.0)));
        for p in ["p50_ms", "p95_ms", "p99_ms"] {
            let v = tab[p].as_f64().unwrap();
            assert!(v > 38.0 && v <= 40.0, "{p} {v}: within its bucket, never past the longest");
        }
        assert_eq!(j["lines"], json!(1));
        assert_eq!(j["news"], json!({ "frames": 0, "past": 0, "allowed": 3 }));
        // Read with a reset: all of it starts again
        let again = report(b.settings(), totals, stats, false);
        assert_eq!(again["at_rest"]["frames"], json!(0));
        assert_eq!(again["on_purpose"], json!([]));
        assert_eq!(again["lines"], json!(0));
        let tab = again["actions"].as_array().unwrap().iter().find(|x| x["action"] == "tab").unwrap().clone();
        assert_eq!(tab["count"], json!(0));
    }

    /// What the hot paths cost, through the thread's own budget, measured: printed, held only to
    /// a loose ceiling so a slow test machine doesn't fail it (lbterm/PERF.md has the numbers)
    #[test]
    fn what_the_hot_paths_cost() {
        let mut s = settings();
        s.quiet = Duration::from_secs(3600);
        configure(s);
        // Busy for an hour: the path every run takes while anything happens
        activity(0);
        let at = here();
        let n = 1_000_000u32;
        let now = Instant::now();
        let per = |t: Instant| t.elapsed().as_nanos() as f64 / n as f64;
        let t = Instant::now();
        for i in 0..n {
            timed(Site::Message(0x0100), i as u64, now);
        }
        let message = per(t);
        let t = Instant::now();
        for i in 0..n {
            timed(Site::Task(at), i as u64, now);
        }
        let task = per(t);
        let t = Instant::now();
        for _ in 0..n {
            fired(at);
        }
        let timer = per(t);
        // An action waiting: every run is traced
        let k = begin(0, Action::Tab, Instant::now()).expect("timed");
        let t = Instant::now();
        for i in 0..n {
            timed(Site::Task(at), i as u64, now);
        }
        let traced = per(t);
        drop_ticket(k);
        // An action begun and shown, as each frame's paint is
        let m = 100_000u32;
        let t = Instant::now();
        for _ in 0..m {
            let k = begin(0, Action::Tab, Instant::now()).expect("timed");
            done(k, Instant::now());
        }
        let action = t.elapsed().as_nanos() as f64 / m as f64;
        configure(Settings::off());
        eprintln!(
            "a run timed {message:.1} ns (a task {task:.1}, traced {traced:.1}), a timer fired {timer:.1} ns, an action begun and shown {action:.0} ns"
        );
        assert!(message < 1_000.0 && task < 1_000.0 && traced < 2_000.0 && timer < 2_000.0, "{message} {task} {traced} {timer}");
        assert!(action < 50_000.0, "{action}");
    }

    #[test]
    fn names_are_words_and_dashes() {
        // lbterm-lib.ps1 reads an action's line by its name: `[a-z-]+`
        for a in Action::ALL {
            assert!(a.name().chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{}", a.name());
        }
        assert_eq!(Action::ALL.iter().map(|a| a.index()).collect::<Vec<_>>(), (0..Action::COUNT).collect::<Vec<_>>());
        assert_eq!(Purpose::ALL.iter().map(|p| p.name()).collect::<Vec<_>>(), ["animation", "clock", "status"]);
    }
}
