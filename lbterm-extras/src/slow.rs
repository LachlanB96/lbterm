//! The slow-frame log: what held the GUI thread long enough to be felt, by name, in the GUI's
//! log. That thread also takes every keypress, so anything that holds it shows up as typing
//! arriving in bursts. Three kinds of thing are timed on it, each with an `Instant` pair and
//! nothing allocated unless it went over:
//!
//! - `message`: each window message the window procedure handles (`window/src/os/windows/
//!   window.rs`), named by message: `WM_KEYDOWN`, `WM_USER+3`, `WM_0xC0F1`. Windows' own default
//!   handling (`DefWindowProcW`, where a title-bar drag's modal loop runs) isn't counted.
//! - `task`: each run of a task from the main-thread queue (`window/src/spawn.rs`), named by where
//!   it was spawned, `wezterm-gui/src/termwindow/mod.rs:1754`: promise's spawn functions,
//!   `Window::notify` and `Connection::with_window_inner` carry `#[track_caller]`.
//! - `paint`: a window's paint in its big parts, by the window's mux id: building the frame's
//!   quads (`window-0/pass`), drawing them into the swapchains and submitting (`window-0/draw`)
//!   and, with the compositor, presenting every visual and committing (`window-0/present`), which
//!   waits on DWM for up to a refresh.
//!
//! Time counts against the innermost of them: a paint inside WM_PAINT, or a task run inside
//! WM_SIZING, is the paint's or the task's, and only what's left over is the message's. One stall
//! is one line, under the name of what held the thread.
//!
//! The lines (WARN, target `lbterm::slow`; `lbterm.ps1 logs -Slow` reads them):
//!
//! ```text
//! slow task wezterm-gui/src/termwindow/mod.rs:1754: 61.4 ms
//! slow paint window-0/present: 3 times over 33 ms in the last 10 s, max 58.0 ms
//! ```
//!
//! The first is a site's first time over in a 10 s window, logged at once; the second ends a
//! window that had more. A site has no spaces and no `: `, so a line splits on its last `: `.
//! `lbterm_slow_ms` (default 33, 0 off) is the threshold; until a window hands it over, nothing
//! is timed. `LBTERM_SLOW_EVERY` in the GUI's environment logs every time over instead, with no
//! windows, for measuring (`lbterm\perf\slow.ps1 -Measure`).

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long a site's window lasts: its first time over is logged at once, the rest are counted
/// until it ends
pub const WINDOW: Duration = Duration::from_secs(10);

/// The log target the lines go to
pub const TARGET: &str = "lbterm::slow";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Message,
    Task,
    Paint,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Task => "task",
            Self::Paint => "paint",
        }
    }
}

/// A paint's big parts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// Building the frame: the panes' lines shaped and laid out as quads, the tab bar, the rail,
    /// every overlay
    Pass,
    /// Handing the quads to the GPU: with the compositor, every visual's swapchain made ready,
    /// drawn into and submitted; without it, presented too
    Draw,
    /// With the compositor, every visual presented and DWM's tree committed
    Present,
}

/// What held the thread
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    /// A window message, by number
    Message(u32),
    /// A task, by where it was spawned
    Task(&'static Location<'static>),
    /// A part of a window's paint, by the window's mux id
    Paint { window: usize, part: Part },
}

impl Site {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Message(_) => Kind::Message,
            Self::Task(_) => Kind::Task,
            Self::Paint { .. } => Kind::Paint,
        }
    }
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Message(msg) => f.write_str(&message_name(*msg)),
            Self::Task(at) => write!(f, "{}:{}", source_name(at.file()), at.line()),
            Self::Paint { window, part } => write!(
                f,
                "window-{window}/{}",
                match part {
                    Part::Pass => "pass",
                    Part::Draw => "draw",
                    Part::Present => "present",
                }
            ),
        }
    }
}

/// A window message's name: WM_ and its name for the ones a window sees, WM_USER+n and WM_APP+n
/// for private ones, and WM_0x followed by its number in hex for the rest (registered messages,
/// from 0xC000)
pub fn message_name(msg: u32) -> Cow<'static, str> {
    let name = match msg {
        0x0000 => "WM_NULL",
        0x0001 => "WM_CREATE",
        0x0002 => "WM_DESTROY",
        0x0003 => "WM_MOVE",
        0x0005 => "WM_SIZE",
        0x0006 => "WM_ACTIVATE",
        0x0007 => "WM_SETFOCUS",
        0x0008 => "WM_KILLFOCUS",
        0x000A => "WM_ENABLE",
        0x000B => "WM_SETREDRAW",
        0x000C => "WM_SETTEXT",
        0x000D => "WM_GETTEXT",
        0x000E => "WM_GETTEXTLENGTH",
        0x000F => "WM_PAINT",
        0x0010 => "WM_CLOSE",
        0x0012 => "WM_QUIT",
        0x0014 => "WM_ERASEBKGND",
        0x0015 => "WM_SYSCOLORCHANGE",
        0x0018 => "WM_SHOWWINDOW",
        0x001A => "WM_SETTINGCHANGE",
        0x001C => "WM_ACTIVATEAPP",
        0x0020 => "WM_SETCURSOR",
        0x0021 => "WM_MOUSEACTIVATE",
        0x0024 => "WM_GETMINMAXINFO",
        0x003D => "WM_GETOBJECT",
        0x0046 => "WM_WINDOWPOSCHANGING",
        0x0047 => "WM_WINDOWPOSCHANGED",
        0x004E => "WM_NOTIFY",
        0x0050 => "WM_INPUTLANGCHANGEREQUEST",
        0x0051 => "WM_INPUTLANGCHANGE",
        0x007B => "WM_CONTEXTMENU",
        0x007E => "WM_DISPLAYCHANGE",
        0x007F => "WM_GETICON",
        0x0080 => "WM_SETICON",
        0x0081 => "WM_NCCREATE",
        0x0082 => "WM_NCDESTROY",
        0x0083 => "WM_NCCALCSIZE",
        0x0084 => "WM_NCHITTEST",
        0x0085 => "WM_NCPAINT",
        0x0086 => "WM_NCACTIVATE",
        0x0087 => "WM_GETDLGCODE",
        0x00A0 => "WM_NCMOUSEMOVE",
        0x00A1 => "WM_NCLBUTTONDOWN",
        0x00A2 => "WM_NCLBUTTONUP",
        0x00A3 => "WM_NCLBUTTONDBLCLK",
        0x00A4 => "WM_NCRBUTTONDOWN",
        0x00A5 => "WM_NCRBUTTONUP",
        0x00FF => "WM_INPUT",
        0x0100 => "WM_KEYDOWN",
        0x0101 => "WM_KEYUP",
        0x0102 => "WM_CHAR",
        0x0103 => "WM_DEADCHAR",
        0x0104 => "WM_SYSKEYDOWN",
        0x0105 => "WM_SYSKEYUP",
        0x0106 => "WM_SYSCHAR",
        0x010D => "WM_IME_STARTCOMPOSITION",
        0x010E => "WM_IME_ENDCOMPOSITION",
        0x010F => "WM_IME_COMPOSITION",
        0x0111 => "WM_COMMAND",
        0x0112 => "WM_SYSCOMMAND",
        0x0113 => "WM_TIMER",
        0x0116 => "WM_INITMENU",
        0x0117 => "WM_INITMENUPOPUP",
        0x011F => "WM_MENUSELECT",
        0x0121 => "WM_ENTERIDLE",
        0x0200 => "WM_MOUSEMOVE",
        0x0201 => "WM_LBUTTONDOWN",
        0x0202 => "WM_LBUTTONUP",
        0x0203 => "WM_LBUTTONDBLCLK",
        0x0204 => "WM_RBUTTONDOWN",
        0x0205 => "WM_RBUTTONUP",
        0x0206 => "WM_RBUTTONDBLCLK",
        0x0207 => "WM_MBUTTONDOWN",
        0x0208 => "WM_MBUTTONUP",
        0x0209 => "WM_MBUTTONDBLCLK",
        0x020A => "WM_MOUSEWHEEL",
        0x020B => "WM_XBUTTONDOWN",
        0x020C => "WM_XBUTTONUP",
        0x020D => "WM_XBUTTONDBLCLK",
        0x020E => "WM_MOUSEHWHEEL",
        0x0210 => "WM_PARENTNOTIFY",
        0x0211 => "WM_ENTERMENULOOP",
        0x0212 => "WM_EXITMENULOOP",
        0x0214 => "WM_SIZING",
        0x0215 => "WM_CAPTURECHANGED",
        0x0216 => "WM_MOVING",
        0x0218 => "WM_POWERBROADCAST",
        0x0219 => "WM_DEVICECHANGE",
        0x0231 => "WM_ENTERSIZEMOVE",
        0x0232 => "WM_EXITSIZEMOVE",
        0x0233 => "WM_DROPFILES",
        0x0281 => "WM_IME_SETCONTEXT",
        0x0282 => "WM_IME_NOTIFY",
        0x0286 => "WM_IME_CHAR",
        0x02A0 => "WM_NCMOUSEHOVER",
        0x02A1 => "WM_MOUSEHOVER",
        0x02A2 => "WM_NCMOUSELEAVE",
        0x02A3 => "WM_MOUSELEAVE",
        0x02E0 => "WM_DPICHANGED",
        0x0312 => "WM_HOTKEY",
        0x031A => "WM_THEMECHANGED",
        0x031D => "WM_CLIPBOARDUPDATE",
        0x031E => "WM_DWMCOMPOSITIONCHANGED",
        0x031F => "WM_DWMNCRENDERINGCHANGED",
        0x0320 => "WM_DWMCOLORIZATIONCOLORCHANGED",
        0x0321 => "WM_DWMWINDOWMAXIMIZEDCHANGE",
        0x033F => "WM_GETTITLEBARINFOEX",
        0x0400 => "WM_USER",
        0x8000 => "WM_APP",
        0x0401..=0x7FFF => return Cow::Owned(format!("WM_USER+{}", msg - 0x0400)),
        0x8001..=0xBFFF => return Cow::Owned(format!("WM_APP+{}", msg - 0x8000)),
        _ => return Cow::Owned(format!("WM_0x{msg:04X}")),
    };
    Cow::Borrowed(name)
}

/// A source file as a site names it: forward slashes, from the workspace (`wezterm-gui/src/
/// main.rs`, as cargo hands rustc a member's path) or from a dependency's own folder
/// (`smol-2.0.2/src/lib.rs`), never a drive's colon or a space
pub fn source_name(file: &str) -> String {
    let path = file.replace('\\', "/");
    let mut name = path.as_str();
    if let Some(i) = name.find("/registry/src/") {
        // past the index's folder, to the crate's own
        let rest = &name[i + "/registry/src/".len()..];
        name = rest
            .split_once('/')
            .map_or(rest, |(_, crate_path)| crate_path);
    } else if let Some(i) = name.find("/git/checkouts/") {
        name = &name[i + "/git/checkouts/".len()..];
    } else if let Some(i) = name.find(':') {
        name = name[i + 1..].trim_start_matches('/');
    }
    name.replace([' ', ':'], "_")
}

/// One line of the log, as its message
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// A site's first time over in a window
    First { site: Site, ms: f64 },
    /// A window that ended with more than one
    Summary {
        site: Site,
        count: u32,
        threshold_ms: u64,
        max_ms: f64,
    },
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::First { site, ms } => {
                write!(f, "slow {} {site}: {ms:.1} ms", site.kind().as_str())
            }
            Self::Summary {
                site,
                count,
                threshold_ms,
                max_ms,
            } => write!(
                f,
                "slow {} {site}: {count} times over {threshold_ms} ms in the last {} s, max {max_ms:.1} ms",
                site.kind().as_str(),
                WINDOW.as_secs(),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Open {
    start: Instant,
    threshold_ms: u64,
    count: u32,
    max_ms: f64,
}

/// The sites that went over in the last 10 s, each with its own window from its first time over:
/// what's logged at once, what's counted, and when a window ends with more
#[derive(Debug, Default)]
pub struct Windows {
    open: HashMap<Site, Open>,
    /// When the timer that ends windows is due, while one is set
    wake_at: Option<Instant>,
}

impl Windows {
    /// `site` held the thread `ms` at `now`, over `threshold_ms`: the windows that ended by now
    /// go into `out` first, then its first line when this starts a window
    pub fn over(
        &mut self,
        site: Site,
        ms: f64,
        threshold_ms: u64,
        now: Instant,
        out: &mut Vec<Line>,
    ) {
        self.end(now, out);
        match self.open.get_mut(&site) {
            Some(open) => {
                open.count += 1;
                open.max_ms = open.max_ms.max(ms);
            }
            None => {
                self.open.insert(
                    site,
                    Open {
                        start: now,
                        threshold_ms,
                        count: 1,
                        max_ms: ms,
                    },
                );
                out.push(Line::First { site, ms });
            }
        }
    }

    /// Ends the windows that are 10 s old by `now`, a summary in `out` for each that had more
    /// than its first, oldest first
    pub fn end(&mut self, now: Instant, out: &mut Vec<Line>) {
        let mut ended: Vec<(Site, Open)> = Vec::new();
        self.open.retain(|site, open| {
            if now.saturating_duration_since(open.start) >= WINDOW {
                ended.push((*site, *open));
                false
            } else {
                true
            }
        });
        ended.sort_by_key(|(_, open)| open.start);
        for (site, open) in ended {
            if open.count > 1 {
                out.push(Line::Summary {
                    site,
                    count: open.count,
                    threshold_ms: open.threshold_ms,
                    max_ms: open.max_ms,
                });
            }
        }
    }

    /// When the next window ends
    pub fn next_end(&self) -> Option<Instant> {
        self.open.values().map(|open| open.start + WINDOW).min()
    }

    /// How long until a timer should end the next window, when one should be set now: none while
    /// one is already due by then
    pub fn wake(&mut self, now: Instant) -> Option<Duration> {
        let next = self.next_end()?;
        if self.wake_at.map_or(false, |at| at <= next) {
            return None;
        }
        self.wake_at = Some(next);
        Some(next.saturating_duration_since(now))
    }

    /// The timer went off
    pub fn woke(&mut self) {
        self.wake_at = None;
    }
}

/// Off (0) until the GUI hands over `lbterm_slow_ms` (`set_threshold_ms`)
static THRESHOLD_MS: AtomicU64 = AtomicU64::new(0);
static WAKER: OnceLock<fn(Duration)> = OnceLock::new();

/// The threshold in milliseconds; 0 turns the log off
pub fn set_threshold_ms(ms: u64) {
    THRESHOLD_MS.store(ms, Ordering::Relaxed);
}

pub fn threshold_ms() -> u64 {
    THRESHOLD_MS.load(Ordering::Relaxed)
}

/// What sets a timer on the GUI thread to call `wake_up` after a while, so a window's summary is
/// logged when it ends rather than with the next thing timed
pub fn set_waker(waker: fn(Duration)) {
    WAKER.set(waker).ok();
}

thread_local! {
    /// The time nested scopes took, inside the scope running now
    static NESTED: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    /// When the next window ends
    static DUE: Cell<Option<Instant>> = const { Cell::new(None) };
    static WINDOWS: RefCell<Windows> = RefCell::new(Windows::default());
}

/// Times what runs until it's dropped, less what nested scopes took
#[must_use]
pub struct Scope {
    site: Site,
    start: Option<Instant>,
    /// NESTED of the scope this one is inside
    outer: Duration,
}

impl Scope {
    #[inline]
    fn new(site: Site) -> Self {
        if THRESHOLD_MS.load(Ordering::Relaxed) == 0 {
            return Self {
                site,
                start: None,
                outer: Duration::ZERO,
            };
        }
        let outer = NESTED.with(|n| n.replace(Duration::ZERO));
        Self {
            site,
            start: Some(Instant::now()),
            outer,
        }
    }
}

/// A window message, `msg`, being handled
#[inline]
pub fn message(msg: u32) -> Scope {
    Scope::new(Site::Message(msg))
}

/// A task spawned `at` being run
#[inline]
pub fn task(at: &'static Location<'static>) -> Scope {
    Scope::new(Site::Task(at))
}

/// A part of the paint of the window with mux id `window`
#[inline]
pub fn paint(window: usize, part: Part) -> Scope {
    Scope::new(Site::Paint { window, part })
}

impl Drop for Scope {
    #[inline]
    fn drop(&mut self) {
        let Some(start) = self.start else { return };
        let now = Instant::now();
        let total = now.saturating_duration_since(start);
        let nested = NESTED.with(|n| n.replace(self.outer + total));
        let own = total.saturating_sub(nested);
        let threshold = THRESHOLD_MS.load(Ordering::Relaxed);
        if threshold != 0 && own > Duration::from_millis(threshold) {
            record(self.site, own, threshold, now);
        } else if DUE.with(|d| d.get()).map_or(false, |due| now >= due) {
            wake_up();
        }
    }
}

/// `LBTERM_SLOW_EVERY` set: every time over is logged as a first line, with no 10 s windows, to
/// measure what an ordinary session does (`lbterm\perf\slow.ps1 -Measure`)
fn every() -> bool {
    static EVERY: OnceLock<bool> = OnceLock::new();
    *EVERY.get_or_init(|| std::env::var_os("LBTERM_SLOW_EVERY").is_some())
}

#[cold]
fn record(site: Site, own: Duration, threshold_ms: u64, now: Instant) {
    let ms = own.as_secs_f64() * 1000.0;
    if every() {
        emit(&[Line::First { site, ms }]);
        return;
    }
    let mut lines = Vec::new();
    let wake = WINDOWS.try_with(|w| {
        let mut w = w.borrow_mut();
        w.over(site, ms, threshold_ms, now, &mut lines);
        DUE.with(|d| d.set(w.next_end()));
        w.wake(now)
    });
    emit(&lines);
    if let (Ok(Some(after)), Some(waker)) = (wake, WAKER.get()) {
        waker(after);
    }
}

/// Ends the windows that are due, logging their summaries: the waker's timer calls this
#[cold]
pub fn wake_up() {
    let now = Instant::now();
    let mut lines = Vec::new();
    let wake = WINDOWS.try_with(|w| {
        let mut w = w.borrow_mut();
        w.woke();
        w.end(now, &mut lines);
        DUE.with(|d| d.set(w.next_end()));
        w.wake(now)
    });
    emit(&lines);
    if let (Ok(Some(after)), Some(waker)) = (wake, WAKER.get()) {
        waker(after);
    }
}

fn emit(lines: &[Line]) {
    for line in lines {
        log::warn!(target: TARGET, "{line}");
        #[cfg(test)]
        tests::EMITTED.with(|e| e.borrow_mut().push(line.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub(super) static EMITTED: RefCell<Vec<String>> = RefCell::new(Vec::new());
    }

    const PAINT: Site = Site::Paint {
        window: 0,
        part: Part::Pass,
    };
    const KEY: Site = Site::Message(0x0100);

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    /// Splits a line's message the way a reader does: on the last `: ` that a number follows
    fn split(line: &str) -> (&str, &str) {
        let i = line
            .match_indices(": ")
            .filter(|(i, _)| line[i + 2..].starts_with(|c: char| c.is_ascii_digit()))
            .last()
            .expect("a `: ` before a number")
            .0;
        (&line[..i], &line[i + 2..])
    }

    #[test]
    fn the_first_time_over_is_logged_at_once() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        w.over(PAINT, 12.34, 8, t0, &mut out);
        assert_eq!(
            out.iter().map(|l| l.to_string()).collect::<Vec<_>>(),
            ["slow paint window-0/pass: 12.3 ms"]
        );
    }

    #[test]
    fn more_in_the_same_10_s_are_counted_then_summed_up_when_it_ends() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        w.over(KEY, 9.0, 8, t0, &mut out);
        w.over(KEY, 31.25, 8, at(t0, 4_000), &mut out);
        w.over(KEY, 10.0, 8, at(t0, 9_999), &mut out);
        assert_eq!(out.len(), 1, "only the first: {out:?}");
        w.end(at(t0, 9_999), &mut out);
        assert_eq!(out.len(), 1, "the window hasn't ended: {out:?}");
        w.end(at(t0, 10_000), &mut out);
        assert_eq!(
            out[1].to_string(),
            "slow message WM_KEYDOWN: 3 times over 8 ms in the last 10 s, max 31.2 ms"
        );
        assert_eq!(w.next_end(), None);
    }

    #[test]
    fn a_window_with_only_its_first_ends_quietly() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        w.over(KEY, 9.0, 8, t0, &mut out);
        w.end(at(t0, 20_000), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(w.next_end(), None);
    }

    #[test]
    fn over_again_after_the_window_ends_sums_it_up_and_starts_another() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        w.over(KEY, 9.0, 8, t0, &mut out);
        w.over(KEY, 15.0, 8, at(t0, 1_000), &mut out);
        w.over(KEY, 20.0, 8, at(t0, 12_000), &mut out);
        let lines: Vec<String> = out.iter().map(|l| l.to_string()).collect();
        assert_eq!(
            lines,
            [
                "slow message WM_KEYDOWN: 9.0 ms",
                "slow message WM_KEYDOWN: 2 times over 8 ms in the last 10 s, max 15.0 ms",
                "slow message WM_KEYDOWN: 20.0 ms",
            ]
        );
        assert_eq!(w.next_end(), Some(at(t0, 22_000)));
    }

    #[test]
    fn each_site_has_a_window_of_its_own() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        w.over(KEY, 9.0, 8, t0, &mut out);
        w.over(PAINT, 9.0, 8, at(t0, 5_000), &mut out);
        w.over(KEY, 9.5, 8, at(t0, 6_000), &mut out);
        w.over(PAINT, 9.5, 8, at(t0, 6_000), &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(w.next_end(), Some(at(t0, 10_000)));
        w.end(at(t0, 10_000), &mut out);
        assert_eq!(out.len(), 3);
        assert!(out[2]
            .to_string()
            .starts_with("slow message WM_KEYDOWN: 2 times"));
        assert_eq!(w.next_end(), Some(at(t0, 15_000)));
        w.end(at(t0, 15_000), &mut out);
        assert!(out[3]
            .to_string()
            .starts_with("slow paint window-0/pass: 2 times"));
    }

    #[test]
    fn a_summary_keeps_the_threshold_its_window_started_with() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        w.over(KEY, 20.0, 16, t0, &mut out);
        w.over(KEY, 9.0, 8, at(t0, 1_000), &mut out);
        w.end(at(t0, 10_000), &mut out);
        assert!(
            out[1].to_string().contains("2 times over 16 ms"),
            "{}",
            out[1]
        );
    }

    #[test]
    fn one_timer_at_a_time_for_the_next_window_to_end() {
        let t0 = Instant::now();
        let mut w = Windows::default();
        let mut out = Vec::new();
        assert_eq!(w.wake(t0), None, "nothing open");
        w.over(KEY, 9.0, 8, t0, &mut out);
        assert_eq!(w.wake(t0), Some(WINDOW));
        w.over(PAINT, 9.0, 8, at(t0, 3_000), &mut out);
        assert_eq!(w.wake(at(t0, 3_000)), None, "the first timer is due sooner");
        w.woke();
        w.end(at(t0, 10_000), &mut out);
        assert_eq!(w.wake(at(t0, 10_000)), Some(Duration::from_secs(3)));
        w.woke();
        w.end(at(t0, 13_000), &mut out);
        assert_eq!(w.wake(at(t0, 13_000)), None, "nothing left open");
    }

    #[test]
    fn every_line_splits_on_its_last_colon_space_before_a_number() {
        let at = Location::caller();
        let lines = [
            Line::First {
                site: Site::Task(at),
                ms: 30.04,
            },
            Line::Summary {
                site: Site::Task(at),
                count: 4,
                threshold_ms: 8,
                max_ms: 41.0,
            },
            Line::First {
                site: Site::Message(0xC0F1),
                ms: 8.06,
            },
        ];
        let want_site = format!("slow task lbterm-extras/src/slow.rs:{}", at.line());
        let s = lines[0].to_string();
        assert_eq!(split(&s), (want_site.as_str(), "30.0 ms"));
        let s = lines[1].to_string();
        let (site, rest) = split(&s);
        assert_eq!(site, want_site);
        assert_eq!(rest, "4 times over 8 ms in the last 10 s, max 41.0 ms");
        assert_eq!(lines[2].to_string(), "slow message WM_0xC0F1: 8.1 ms");
        for line in &lines {
            let s = line.to_string();
            let (head, _) = split(&s);
            let site = head.splitn(3, ' ').nth(2).unwrap();
            assert!(!site.contains(' ') && !site.contains(": "), "{site}");
        }
    }

    #[test]
    fn message_names() {
        assert_eq!(message_name(0x000F), "WM_PAINT");
        assert_eq!(message_name(0x0100), "WM_KEYDOWN");
        assert_eq!(message_name(0x0214), "WM_SIZING");
        assert_eq!(message_name(0x0400), "WM_USER");
        assert_eq!(message_name(0x0403), "WM_USER+3");
        assert_eq!(message_name(0x8001), "WM_APP+1");
        assert_eq!(message_name(0xC0F1), "WM_0xC0F1");
        assert_eq!(message_name(0x0009), "WM_0x0009");
    }

    #[test]
    fn source_names() {
        assert_eq!(
            source_name(r"wezterm-gui\src\termwindow\mod.rs"),
            "wezterm-gui/src/termwindow/mod.rs"
        );
        assert_eq!(
            source_name("wezterm-gui/src\\termwindow/mod.rs"),
            "wezterm-gui/src/termwindow/mod.rs"
        );
        assert_eq!(
            source_name(
                r"C:\Users\me\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\smol-2.0.2\src\lib.rs"
            ),
            "smol-2.0.2/src/lib.rs"
        );
        assert_eq!(
            source_name(
                r"C:\Users\me\.cargo\git\checkouts\xcb-imdkit-rs-5f3c0ad3c0b81b6b\9b6c1a8\src\lib.rs"
            ),
            "xcb-imdkit-rs-5f3c0ad3c0b81b6b/9b6c1a8/src/lib.rs"
        );
        assert_eq!(source_name(r"D:\some where\x.rs"), "some_where/x.rs");
    }

    /// Scopes, timed for real: the global threshold makes this one test
    #[test]
    fn scopes_charge_the_innermost() {
        let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
        let emitted = || EMITTED.with(|e| e.borrow_mut().drain(..).collect::<Vec<_>>());

        set_threshold_ms(0);
        {
            let _off = message(0x0100);
            sleep(30);
        }
        assert_eq!(emitted(), Vec::<String>::new(), "0 is off");

        set_threshold_ms(20);
        {
            let _quick = message(0x0101);
        }
        assert_eq!(emitted(), Vec::<String>::new(), "under the threshold");

        // A paint inside WM_PAINT: the paint's, not the message's
        {
            let _msg = message(0x000F);
            let _paint = paint(3, Part::Draw);
            sleep(40);
        }
        let lines = emitted();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("slow paint window-3/draw: "),
            "{lines:?}"
        );

        // A message whose own time is over, around a quick task
        {
            let _msg = message(0x0214);
            sleep(40);
            let _task = task(Location::caller());
        }
        let lines = emitted();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("slow message WM_SIZING: "),
            "{lines:?}"
        );

        set_threshold_ms(0);
    }
}
