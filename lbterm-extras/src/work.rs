//! What the GUI thread does besides painting, counted: the main thread's tasks and how long the
//! message loop's drains of them keep a waiting message waiting, the IME's window and the mouse
//! cursor set, the accent colour read, and the panes' lines laid out, turned into runs and shaped
//! again. The Windows side is the message loop (window/src/os/windows/connection.rs), the spawn
//! queue (window/src/spawn.rs) and window.rs; the lines are wezterm-gui's
//! (termwindow/render/pane.rs, screen_line.rs and mod.rs).
//!
//! The message loop's drain stops for a message waiting for the window ([`Drain`]): until
//! 2026-10-30 it ran every task queued, the low-priority ones too, and any queued while it ran,
//! before the loop looked at a single message, so a key or a click waited behind whatever the
//! panes' output had queued. A value told to Windows is told again only when it changed
//! ([`Told`]): the IME's window was moved to the text cursor in a task of its own every frame, and
//! the mouse cursor set in one on every move over a pane, unchanged. And a window that can't be
//! seen ([`unseen`]: minimized, the session locked or the screens off) paints at most once every
//! [`UNSEEN_PERIOD`] ([`period`]).
//!
//! The counts are the process's since it started, every window together, and go to a test
//! window's state file as `work` (lbterm/STATE.md). They cost a relaxed atomic add each, and a
//! drain asks Windows whether a message waits at most once every [`CHECK_EVERY`] of its running
//! (GetQueueStatus, 1.7 us a call on 2026-10-30).

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How often a drain of the task queue asks Windows whether a message waits for the window: a
/// message that comes during a drain waits at most this, and the task it came in, before it's
/// seen. GetQueueStatus is a system call (1.7 us), too dear to ask after every task, which can
/// take 2 us.
pub const CHECK_EVERY: Duration = Duration::from_micros(500);

/// A drain, or a wait in one, longer than this is counted as long
pub const LONG: Duration = Duration::from_millis(2);

/// One drain of the main thread's task queue, as the message loop runs it: the tasks, and when a
/// check first found a message waiting for the window.
#[derive(Debug)]
pub struct Drain {
    started: Instant,
    checked: Instant,
    tasks: u64,
    waiting_since: Option<Instant>,
    cut: bool,
}

impl Drain {
    pub fn start(now: Instant) -> Self {
        Self {
            started: now,
            checked: now,
            tasks: 0,
            waiting_since: None,
            cut: false,
        }
    }

    /// A task ran, ending at `now`: whether it's time to ask Windows whether a message waits
    pub fn ran(&mut self, now: Instant) -> bool {
        self.tasks += 1;
        now.saturating_duration_since(self.checked) >= CHECK_EVERY
    }

    /// Windows said at `now` whether a message waits
    pub fn checked(&mut self, now: Instant, waiting: bool) {
        self.checked = now;
        if waiting && self.waiting_since.is_none() {
            self.waiting_since = Some(now);
        }
    }

    /// It stops with tasks left, for the message waiting: the loop takes that, then drains again
    pub fn cut(&mut self) {
        self.cut = true;
    }

    /// The drain ended at `now`, counted in `work`
    pub fn end(self, now: Instant, work: &Work) {
        if self.tasks == 0 {
            return;
        }
        let took = now.saturating_duration_since(self.started);
        let waited = self
            .waiting_since
            .map(|since| now.saturating_duration_since(since));
        work.drained(self.tasks, took, waited);
        // LBTerm: and in metrics, each drain's time, so the longest shows (counters::drained)
        crate::counters::drained(micros(took) * 1_000);
        if self.cut {
            work.cut.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn micros(d: Duration) -> u64 {
    d.as_micros().min(u64::MAX as u128) as u64
}

/// What the state file's `work` holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Tasks the message loop ran from the main thread's queue
    pub tasks: u64,
    /// Drains of the queue that ran at least one task
    pub drains: u64,
    /// Their time in all, in microseconds
    pub drain_us: u64,
    /// Those over [`LONG`]
    pub long_drains: u64,
    /// Drains in which a check found a message waiting for the window (a key, the pointer, a
    /// posted message) before they ended
    pub waited: u64,
    /// How long those messages waited in their drains after they were seen, in all, in
    /// microseconds
    pub waited_us: u64,
    /// Those that waited over [`LONG`]
    pub long_waits: u64,
    /// Times the IME's window was moved to the text cursor (ImmSetCandidateWindow or
    /// ImmSetCompositionWindow)
    pub ime: u64,
    /// Times the mouse cursor was set (SetCursor, through `set_cursor`)
    pub cursors: u64,
    /// Times ColorPrevalence and the accent colour were read for the window's border (the
    /// registry and a UISettings object)
    pub accent: u64,
    /// Panes' lines laid out as quads again, missing the line quad cache
    pub lines: u64,
    /// Lines turned into shaped runs again: panes' lines missing the line-to-element cache (each
    /// of the `lines` whose words no cached line had), and the retro tab bar's every time
    pub line_shapes: u64,
    /// Runs of a pane's text (or the retro tab bar's) shaped again by the font, missing the
    /// shape cache
    pub shapes: u64,
    /// Drains that stopped with tasks left for a message waiting for the window
    pub cut: u64,
    /// Frames painted while the window couldn't be seen ([`unseen`]), one every
    /// [`UNSEEN_PERIOD`] at most
    pub unseen: u64,
    /// Key presses (WM_KEYDOWN, WM_SYSKEYDOWN) the message loop took off the queue
    pub keys: u64,
    /// When it took the last, QueryPerformanceCounter's count: a test that posts a key reads the
    /// same clock as it posts, so the difference is all the key waited in the queue, behind a
    /// paint, a task or a drain, where `waited` is only what a drain kept it after it was seen
    pub key_qpc: u64,
}

/// The counts, as atomics: one for the process, [`WORK`].
#[derive(Debug, Default)]
pub struct Work {
    tasks: AtomicU64,
    drains: AtomicU64,
    drain_us: AtomicU64,
    long_drains: AtomicU64,
    waited: AtomicU64,
    waited_us: AtomicU64,
    long_waits: AtomicU64,
    ime: AtomicU64,
    cursors: AtomicU64,
    accent: AtomicU64,
    lines: AtomicU64,
    line_shapes: AtomicU64,
    shapes: AtomicU64,
    cut: AtomicU64,
    unseen: AtomicU64,
    keys: AtomicU64,
    key_qpc: AtomicU64,
}

impl Work {
    pub const fn new() -> Self {
        Self {
            tasks: AtomicU64::new(0),
            drains: AtomicU64::new(0),
            drain_us: AtomicU64::new(0),
            long_drains: AtomicU64::new(0),
            waited: AtomicU64::new(0),
            waited_us: AtomicU64::new(0),
            long_waits: AtomicU64::new(0),
            ime: AtomicU64::new(0),
            cursors: AtomicU64::new(0),
            accent: AtomicU64::new(0),
            lines: AtomicU64::new(0),
            line_shapes: AtomicU64::new(0),
            shapes: AtomicU64::new(0),
            cut: AtomicU64::new(0),
            unseen: AtomicU64::new(0),
            keys: AtomicU64::new(0),
            key_qpc: AtomicU64::new(0),
        }
    }

    /// A drain ran `tasks` in `took`, and a message it saw waited `waited` in it
    pub fn drained(&self, tasks: u64, took: Duration, waited: Option<Duration>) {
        self.tasks.fetch_add(tasks, Ordering::Relaxed);
        self.drains.fetch_add(1, Ordering::Relaxed);
        self.drain_us.fetch_add(micros(took), Ordering::Relaxed);
        if took > LONG {
            self.long_drains.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(waited) = waited {
            self.waited.fetch_add(1, Ordering::Relaxed);
            self.waited_us.fetch_add(micros(waited), Ordering::Relaxed);
            if waited > LONG {
                self.long_waits.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn ime(&self) {
        self.ime.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cursor(&self) {
        self.cursors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn accent(&self) {
        self.accent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn line(&self) {
        self.lines.fetch_add(1, Ordering::Relaxed);
    }

    pub fn line_shape(&self) {
        self.line_shapes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn shape(&self) {
        self.shapes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn unseen(&self) {
        self.unseen.fetch_add(1, Ordering::Relaxed);
    }

    /// The message loop took a key press off the queue at `qpc` (QueryPerformanceCounter)
    pub fn key(&self, qpc: u64) {
        self.key_qpc.store(qpc, Ordering::Relaxed);
        self.keys.fetch_add(1, Ordering::Relaxed);
    }

    pub fn counts(&self) -> Counts {
        Counts {
            tasks: self.tasks.load(Ordering::Relaxed),
            drains: self.drains.load(Ordering::Relaxed),
            drain_us: self.drain_us.load(Ordering::Relaxed),
            long_drains: self.long_drains.load(Ordering::Relaxed),
            waited: self.waited.load(Ordering::Relaxed),
            waited_us: self.waited_us.load(Ordering::Relaxed),
            long_waits: self.long_waits.load(Ordering::Relaxed),
            ime: self.ime.load(Ordering::Relaxed),
            cursors: self.cursors.load(Ordering::Relaxed),
            accent: self.accent.load(Ordering::Relaxed),
            lines: self.lines.load(Ordering::Relaxed),
            line_shapes: self.line_shapes.load(Ordering::Relaxed),
            shapes: self.shapes.load(Ordering::Relaxed),
            cut: self.cut.load(Ordering::Relaxed),
            unseen: self.unseen.load(Ordering::Relaxed),
            keys: self.keys.load(Ordering::Relaxed),
            key_qpc: self.key_qpc.load(Ordering::Relaxed),
        }
    }
}

/// The process's counts.
pub static WORK: Work = Work::new();

/// A value last told to Windows (the IME's window's place, the mouse cursor's shape), so it's
/// told again only when it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Told<T>(Option<T>);

impl<T> Default for Told<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: PartialEq> Told<T> {
    pub const fn new() -> Self {
        Self(None)
    }

    /// Whether `now` is to be told: nothing has been, or it differs from what was told last. It's
    /// what was told from here on.
    pub fn tell(&mut self, now: T) -> bool {
        if self.0.as_ref() == Some(&now) {
            return false;
        }
        self.0 = Some(now);
        true
    }

    /// What was told last
    pub fn told(&self) -> Option<&T> {
        self.0.as_ref()
    }

    /// Something else may have changed it since: the next is told whatever it is
    pub fn forget(&mut self) {
        self.0 = None;
    }
}

/// What a window message says of the mouse cursor (window/src/os/windows/work.rs)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorNews {
    /// WM_SETCURSOR: whether it's for the window itself (not a child), and the hit-test code
    /// (LOWORD of its lParam)
    SetCursor { own: bool, hit: i16 },
    /// WM_MOUSELEAVE or WM_NCMOUSELEAVE: the pointer left the window or its frame
    Left,
    /// WM_CAPTURECHANGED: the mouse was taken, by a sizing or moving loop among others
    CaptureChanged,
    /// Anything else
    Other,
}

/// HTCLIENT, the client area's hit-test code
pub const HIT_CLIENT: i16 = 1;

/// Whether the shape a window gave Windows for the mouse cursor may be another's after `news`,
/// so the next is given whatever it is. Windows' cursor is the thread's own, and only Windows'
/// default handling on this thread changes it besides LBTerm: over the frame (WM_SETCURSOR for
/// anything but the client area, which LBTerm's window class leaves alone), and in a sizing or
/// moving loop. Leaving the window counts too, so a shape another thread's window shows can't
/// stand.
pub fn cursor_may_have_changed(news: CursorNews) -> bool {
    match news {
        CursorNews::SetCursor { own, hit } => !own || hit != HIT_CLIENT,
        CursorNews::Left | CursorNews::CaptureChanged => true,
        CursorNews::Other => false,
    }
}

/// What a line's key in the line quad cache says of the keyboard (wezterm-gui's
/// render/pane.rs): whether its pane has it, and whether the window does. A line looks the same
/// either way but the cursor's (compute_cell_fg_bg), and, without the compositor, a pane without
/// the keyboard draws dimmed (screen_line.rs's hsv). So the rest are keyed as the active pane's
/// with the window's keyboard left out, and a move of the keyboard between panes or windows
/// makes only the cursor's lines again: every line of every pane in view was made again for one.
pub fn line_keyboard(pane_active: bool, cursor_line: bool, composited: bool, focused: bool) -> (bool, bool) {
    let pane = pane_active || (!cursor_line && composited);
    (pane, cursor_line && focused)
}

/// How often at most a window that can't be seen paints. Not never: what a paint does besides
/// drawing still happens within it (the rail's sessions sent to the Conductor's hub, a test's
/// state file), and the first frame after the window shows again is painted at once. One frame
/// in this long never queues up in front of DWM, which on a locked desktop took a frame about
/// every 260 ms (2026-10-30), so a window asking for more waited on it, on the GUI thread.
pub const UNSEEN_PERIOD: Duration = Duration::from_secs(1);

/// Whether a window can't be seen for now. A minimized window's frames show nowhere, and still
/// came, every one: a pane printing every 250 ms painted 24 frames in 6 s minimized
/// (2026-10-30). Nor do any show while the session is locked or the screens are off. A window
/// that `honours_session` goes by those too.
pub fn unseen(iconic: bool, session_unseen: bool, honours_session: bool) -> bool {
    iconic || (honours_session && session_unseen)
}

/// Whether a window's loops play: the animations DWM carries for as long as what they show is
/// there (a waiting pad's arc, the Conductor's turning hues and the light going round them,
/// compositor.rs). One in a window's visual tree keeps DWM composing a frame every refresh of the
/// fastest screen for as long as it plays, the window minimized, the session locked or the
/// screens off as much as on screen (lbterm/COMPOSITOR.md, "Loops"). So they play with motion on
/// while the window can be seen, and otherwise hold still, as with motion off, until it can be
/// seen again, when they start again from their beginning: nobody saw where they'd got to.
pub fn loops_play(motion: bool, unseen: bool) -> bool {
    motion && !unseen
}

/// How long a window's next frame is held back after a paint: `seen`, a frame of max_fps or a
/// refresh (crate::pace::period), or while it can't be seen [`UNSEEN_PERIOD`], if that's longer
pub fn period(seen: Duration, unseen: bool) -> Duration {
    if unseen {
        seen.max(UNSEEN_PERIOD)
    } else {
        seen
    }
}

/// Whether a window goes by the session's lock and the screens being off. A daily window does. A
/// window with a state file is a test's, whose frames the harness reads with the PC locked as
/// often as not, so it paints on, unless `LBTERM_UNSEEN=skip` asks it to do as a daily one.
pub fn honours_session(state_file: bool, unseen_var: Option<&str>) -> bool {
    !state_file || unseen_var == Some("skip")
}

/// The windows told when the session locks and unlocks (WTSRegisterSessionNotification), as many
/// as there are. The lock is read from Windows when one is watched with none before it, and
/// counts only while one is: with none, nothing says when it changes, so what was read or told
/// last may be stale (a window that opens after a lock and unlock with none watching, or every
/// registration failing, which Windows documents early at logon), and a session taken as locked
/// would paint a window that can be seen once a second.
#[derive(Debug, Default)]
pub struct SessionWatch {
    watched: AtomicU64,
}

impl SessionWatch {
    pub const fn new() -> Self {
        Self {
            watched: AtomicU64::new(0),
        }
    }

    /// A window is told from here on: whether the lock is to be read now, none having been
    pub fn watched(&self) -> bool {
        self.watched.fetch_add(1, Ordering::Relaxed) == 0
    }

    /// A window told of the lock is going
    pub fn unwatched(&self) {
        let _ = self
            .watched
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1));
    }

    /// Whether the session counts as locked, `locked` being what was read or told last
    pub fn locked(&self, locked: bool) -> bool {
        locked && self.watched.load(Ordering::Relaxed) > 0
    }
}

/// The session's lock, from WTSINFOEX_LEVEL1's SessionFlags: WTS_SESSIONSTATE_LOCK (0) or
/// WTS_SESSIONSTATE_UNLOCK (1); anything else, WTS_SESSIONSTATE_UNKNOWN, says nothing.
pub fn session_locked(flags: i32) -> Option<bool> {
    match flags {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// The screens' state, from GUID_SESSION_DISPLAY_STATUS's value: off (0), on (1) or dimmed (2,
/// which still shows)
pub fn display_off(status: u32) -> bool {
    status == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1000.0)
    }

    #[test]
    fn a_drain_asks_windows_only_once_every_check() {
        let t0 = Instant::now();
        let mut drain = Drain::start(t0);
        assert!(!drain.ran(t0 + ms(0.1)));
        assert!(!drain.ran(t0 + ms(0.4)));
        assert!(drain.ran(t0 + ms(0.5)));
        drain.checked(t0 + ms(0.5), false);
        assert!(!drain.ran(t0 + ms(0.9)));
        assert!(drain.ran(t0 + ms(1.2)));
    }

    #[test]
    fn a_drain_that_ran_nothing_counts_nothing() {
        let work = Work::new();
        let t0 = Instant::now();
        Drain::start(t0).end(t0 + ms(1.0), &work);
        assert_eq!(work.counts(), Counts::default());
    }

    #[test]
    fn a_drain_counts_its_tasks_and_time() {
        let work = Work::new();
        let t0 = Instant::now();
        let mut drain = Drain::start(t0);
        drain.ran(t0 + ms(1.0));
        drain.ran(t0 + ms(3.0));
        drain.end(t0 + ms(3.0), &work);
        let mut short = Drain::start(t0 + ms(5.0));
        short.ran(t0 + ms(5.5));
        short.end(t0 + ms(5.5), &work);
        let c = work.counts();
        assert_eq!((c.tasks, c.drains, c.drain_us, c.long_drains), (3, 2, 3500, 1));
        assert_eq!((c.waited, c.waited_us, c.long_waits), (0, 0, 0));
    }

    #[test]
    fn a_message_waits_from_the_check_that_first_saw_it_to_the_drains_end() {
        let work = Work::new();
        let t0 = Instant::now();
        let mut drain = Drain::start(t0);
        drain.ran(t0 + ms(0.6));
        drain.checked(t0 + ms(0.6), false);
        drain.ran(t0 + ms(1.2));
        drain.checked(t0 + ms(1.2), true);
        drain.ran(t0 + ms(1.8));
        drain.checked(t0 + ms(1.8), true);
        drain.ran(t0 + ms(4.0));
        drain.end(t0 + ms(4.0), &work);
        let c = work.counts();
        assert_eq!((c.waited, c.waited_us, c.long_waits), (1, 2800, 1));
    }

    #[test]
    fn a_cut_drain_is_counted_as_one() {
        let work = Work::new();
        let t0 = Instant::now();
        let mut drain = Drain::start(t0);
        drain.ran(t0 + ms(0.6));
        drain.checked(t0 + ms(0.6), true);
        drain.cut();
        drain.end(t0 + ms(0.6), &work);
        let c = work.counts();
        assert_eq!((c.tasks, c.drains, c.cut, c.waited, c.waited_us), (1, 1, 1, 1, 0));
    }

    #[test]
    fn a_value_is_told_again_only_when_it_changed() {
        let mut told = Told::new();
        assert!(told.tell((3, true)));
        assert!(!told.tell((3, true)));
        assert!(told.tell((3, false)));
        assert!(told.tell((4, false)));
        assert!(!told.tell((4, false)));
        assert_eq!(told.told(), Some(&(4, false)));
    }

    #[test]
    fn a_forgotten_value_is_told_again() {
        let mut told = Told::new();
        assert!(told.tell(Some(7)));
        told.forget();
        assert_eq!(told.told(), None);
        assert!(told.tell(Some(7)));
        assert!(!told.tell(Some(7)));
        assert!(told.tell(None));
    }

    #[test]
    fn the_cursor_is_anothers_off_the_client_area_and_after_leaving() {
        let set = |own, hit| cursor_may_have_changed(CursorNews::SetCursor { own, hit });
        assert!(!set(true, HIT_CLIENT));
        // HTLEFT, HTCAPTION, HTMAXBUTTON, HTERROR
        assert!(set(true, 10));
        assert!(set(true, 2));
        assert!(set(true, 9));
        assert!(set(true, -2));
        assert!(set(false, HIT_CLIENT));
        assert!(cursor_may_have_changed(CursorNews::Left));
        assert!(cursor_may_have_changed(CursorNews::CaptureChanged));
        assert!(!cursor_may_have_changed(CursorNews::Other));
    }

    #[test]
    fn with_the_compositor_only_the_cursors_line_goes_by_the_keyboard() {
        // pane_active, cursor_line, composited, focused
        let active_line = line_keyboard(true, false, true, true);
        assert_eq!(active_line, (true, false));
        assert_eq!(line_keyboard(false, false, true, true), active_line);
        assert_eq!(line_keyboard(false, false, true, false), active_line);
        assert_eq!(line_keyboard(true, false, true, false), active_line);
        // The cursor's line goes by both
        assert_eq!(line_keyboard(true, true, true, true), (true, true));
        assert_eq!(line_keyboard(true, true, true, false), (true, false));
        assert_eq!(line_keyboard(false, true, true, true), (false, true));
    }

    #[test]
    fn without_the_compositor_a_pane_without_the_keyboard_draws_apart() {
        assert_eq!(line_keyboard(true, false, false, true), (true, false));
        assert_eq!(line_keyboard(false, false, false, true), (false, false));
        assert_eq!(line_keyboard(false, false, false, false), (false, false));
        assert_eq!(line_keyboard(false, true, false, true), (false, true));
    }

    #[test]
    fn a_minimized_window_cant_be_seen() {
        assert!(unseen(true, false, false));
        assert!(unseen(true, false, true));
        assert!(!unseen(false, false, true));
    }

    #[test]
    fn loops_play_only_with_motion_on_in_a_window_that_can_be_seen() {
        assert!(loops_play(true, false));
        assert!(!loops_play(true, true), "minimized, locked or the screens off: still");
        assert!(!loops_play(false, false), "motion off: still");
        assert!(!loops_play(false, true));
        // The same rule a window's paints go by: what makes it unseen stills its loops
        assert!(!loops_play(true, unseen(true, false, false)));
        assert!(!loops_play(true, unseen(false, true, true)));
        assert!(loops_play(true, unseen(false, true, false)), "a test window paints on, and plays on, locked");
    }

    #[test]
    fn an_unseen_window_paints_once_a_second_at_most() {
        let frame = Duration::from_micros(16_667);
        assert_eq!(period(frame, false), frame);
        assert_eq!(period(frame, true), UNSEEN_PERIOD);
        assert_eq!(period(Duration::from_secs(2), true), Duration::from_secs(2));
    }

    #[test]
    fn the_lock_and_the_screens_stop_only_windows_that_honour_them() {
        assert!(unseen(false, true, true));
        assert!(!unseen(false, true, false));
    }

    #[test]
    fn a_test_window_paints_on_with_the_session_locked_unless_asked() {
        assert!(honours_session(false, None));
        assert!(!honours_session(true, None));
        assert!(!honours_session(true, Some("paint")));
        assert!(honours_session(true, Some("skip")));
        assert!(honours_session(false, Some("skip")));
    }

    #[test]
    fn the_session_flags_say_locked_unlocked_or_nothing() {
        assert_eq!(session_locked(0), Some(true));
        assert_eq!(session_locked(1), Some(false));
        assert_eq!(session_locked(-1), None);
    }

    #[test]
    fn the_lock_is_read_when_a_window_is_watched_with_none_before_it() {
        let session = SessionWatch::new();
        assert!(session.watched());
        assert!(!session.watched());
        session.unwatched();
        session.unwatched();
        // All gone: nothing told it of a lock or unlock since, so the next reads it again
        assert!(session.watched());
    }

    #[test]
    fn with_no_window_told_of_the_lock_the_session_isnt_taken_as_locked() {
        let session = SessionWatch::new();
        // None watched yet, or every registration failed: what was read last says nothing
        assert!(!session.locked(true));
        session.watched();
        assert!(session.locked(true));
        assert!(!session.locked(false));
        session.unwatched();
        assert!(!session.locked(true));
        // Going more times than watched (it can't) leaves none, not a wrapped count
        session.unwatched();
        assert!(!session.locked(true));
        assert!(session.watched());
    }

    #[test]
    fn only_screens_that_are_off_dont_show() {
        assert!(display_off(0));
        assert!(!display_off(1));
        assert!(!display_off(2));
    }

    #[test]
    fn the_counts_add_up() {
        let work = Work::new();
        work.ime();
        work.ime();
        work.cursor();
        work.accent();
        work.line();
        work.line();
        work.line();
        work.line_shape();
        work.line_shape();
        work.shape();
        work.unseen();
        let c = work.counts();
        assert_eq!((c.ime, c.cursors, c.accent, c.lines, c.unseen), (2, 1, 1, 3, 1));
        assert_eq!((c.line_shapes, c.shapes), (2, 1));
    }

    #[test]
    fn a_key_taken_is_counted_with_when_the_last_was() {
        let work = Work::new();
        work.key(1_000);
        work.key(25_000);
        let c = work.counts();
        assert_eq!((c.keys, c.key_qpc), (2, 25_000));
    }

    #[test]
    fn the_counts_serialise_as_the_state_file_names_them() {
        let json = serde_json::to_value(Counts::default()).unwrap();
        let mut names: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "accent",
                "cursors",
                "cut",
                "drain_us",
                "drains",
                "ime",
                "key_qpc",
                "keys",
                "line_shapes",
                "lines",
                "long_drains",
                "long_waits",
                "shapes",
                "tasks",
                "unseen",
                "waited",
                "waited_us"
            ]
        );
    }
}
