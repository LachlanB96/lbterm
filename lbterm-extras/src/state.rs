//! The state file for test windows (lbterm/STATE.md): with `LBTERM_STATE_FILE` set to an
//! absolute path, the GUI writes a JSON snapshot of all its windows there after every frame a
//! window paints, so a test can wait on what LBTerm says it shows instead of on screenshots and
//! sleeps. Unset, nothing is collected.
//!
//! Here is what doesn't need a window: the snapshot's shape ([`Snapshot`] and what it holds),
//! the animations in flight ([`InFlight`]: what DWM is playing, by name, and when the last of it
//! ends), and the [`Writer`], a thread of its own that writes the newest snapshot to
//! `<path>.tmp` and renames it over `<path>`, so a reader never sees half a file and the GUI
//! thread never waits on the disk. The GUI side is wezterm-gui's `state_probe.rs` and
//! `termwindow/state.rs`.

use serde::Serialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The variable naming the file
pub const ENV: &str = "LBTERM_STATE_FILE";
/// The schema's version: a field taken away or changed in meaning bumps it, one added doesn't
pub const VERSION: u32 = 1;

/// How long a rename over the file keeps trying while a reader holds it open without
/// FILE_SHARE_DELETE (PowerShell's Get-Content and .NET's File.ReadAllText do)
const RENAME_FOR: Duration = Duration::from_millis(250);
const RENAME_PAUSE: Duration = Duration::from_millis(5);

/// The path from `LBTERM_STATE_FILE`, read once: `None` when it's unset, empty or not absolute.
pub fn path() -> Option<&'static Path> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let value = std::env::var_os(ENV);
        let path = from_env(value.clone());
        if path.is_none() && value.map_or(false, |v| !v.is_empty()) {
            log::warn!("{ENV} isn't an absolute path: no state file");
        }
        path
    })
    .as_deref()
}

/// The file a value of `LBTERM_STATE_FILE` names, if it names one: an absolute path.
pub fn from_env(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value?);
    path.is_absolute().then_some(path)
}

/// Where a snapshot is written before it's renamed over `path`: `<path>.tmp`.
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// The whole file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub version: u32,
    pub pid: u32,
    /// The build, as `lbterm --version` names it
    pub build: String,
    /// Frames painted by this process, all windows: only ever goes up
    pub frame: u64,
    /// When the snapshot was taken, in milliseconds since the Unix epoch
    pub time_ms: u64,
    pub windows: Vec<Window>,
}

/// One window, as its last frame left it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    /// The GUI's window id (Lua's `window:window_id()`; on a mux domain not the server's)
    pub id: u64,
    /// The window's HWND, for posting messages to it
    pub hwnd: u64,
    /// Frames this window has painted: 0 until its first
    pub frame: u64,
    /// Whether Windows gave it the keyboard
    pub focused: bool,
    /// Whether it shows as having the keyboard: focused, or a test window
    /// (`LBTERM_NO_ACTIVATE`) that hasn't been told otherwise
    pub keyboard: bool,
    /// True exactly while it has asked for another frame to carry an animation, fade or glow
    /// on; the frame an animation ends on says false
    pub animating: bool,
    /// What's moving, by name, soonest to end first
    pub animations: Vec<Animation>,
    /// Waits under way that will change what shows when they're up: a hover's rest before the
    /// tab panel or the strip's card opens, its grace before it closes, a notice's time
    pub waits: Vec<Wait>,
    /// Nothing animating and nothing waiting
    pub settled: bool,
    /// Animations that never end, carried by DWM while what they show is there: a waiting
    /// pad's arc, the Conductor's turning hues
    pub loops: Vec<String>,
    pub size: Size,
    /// Where the pointer last was in the window, and what's under it; `None` once it has left
    pub pointer: Option<Pointer>,
    /// The window's index of the open tab
    pub active_tab: Option<usize>,
    pub active_pane: Option<u64>,
    pub tabs: Vec<Tab>,
    /// The open tab's panes, as last painted
    pub panes: Vec<Pane>,
    pub scrollbar: Scrollbar,
    /// The tab panel, with `lbterm_tab_panel` on
    pub rail: Option<Rail>,
    /// The window strip's words, with the tab panel hiding the tabs
    pub strip: Option<Strip>,
    /// The menu that's open: the tab menu, the list of prompts or the Conductor's
    pub menu: Option<Menu>,
    /// An upstream overlay or modal over the panes: "launcher", "search", "copy_mode", ...
    pub overlay: Option<String>,
    /// The readout of the active pane's size, while it shows: "120 × 32"
    pub readout: Option<String>,
    /// The row a jump, a scrollbar mark or a search landed on, while it's marked
    pub landed: Option<Landed>,
    /// The pane whose question colours are shown
    pub questions: Option<u64>,
    /// Hearts in the air (the triggers' easter egg)
    pub hearts: usize,
}

/// An animation in flight: what moves, how far through its time it is (0 to 1), and how long
/// it has left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Animation {
    pub name: String,
    pub progress: f64,
    pub left_ms: u64,
}

/// A wait under way, and how long it has left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wait {
    pub name: String,
    pub left_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

/// A rectangle in the window's pixels, as WM_MOUSEMOVE and PrintWindow count them.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// The whole pixels a rectangle of fractional ones covers most of: its edges rounded
    pub fn round(x: f64, y: f64, w: f64, h: f64) -> Self {
        let (left, top) = (x.round(), y.round());
        let (right, bottom) = ((x + w).round(), (y + h).round());
        Self {
            x: left as i32,
            y: top as i32,
            w: (right - left).max(0.0) as u32,
            h: (bottom - top).max(0.0) as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Cell {
    pub x: usize,
    pub y: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    /// What it's over: "rail.row", "rail.conductor", "rail", "menu.item", "menu",
    /// "strip.button", "strip.pill", "strip.words", "strip.prompt", "strip.arrow",
    /// "strip.card", "strip", "scrollbar", "split", "pane", or "window"
    pub over: String,
    /// The rail's row, a menu's item, the pane, or the button ("close", "maximize", ...)
    pub row: Option<usize>,
    pub item: Option<usize>,
    pub pane: Option<u64>,
    pub button: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tab {
    /// The window's index of it, from 0
    pub index: usize,
    pub id: u64,
    pub title: String,
    pub active: bool,
    pub panes: Vec<u64>,
    pub zoomed: bool,
    /// The colour the tab menu gave it, "#rrggbb"
    pub colour: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pane {
    pub id: u64,
    /// On a mux domain, the server's id for it (`lbterm cli`'s)
    pub server_id: Option<u64>,
    /// Its card, where it's laid out (while the tab panel is open, DWM shows it pressed over)
    pub rect: Rect,
    pub cols: usize,
    pub rows: usize,
    pub active: bool,
    pub zoomed: bool,
    pub title: String,
    pub alt_screen: bool,
    pub mouse_reporting: bool,
    /// The cursor, in cells from the top-left of its screen
    pub cursor: Cell,
    /// Scrolled back: the stable row at the top of the view; `None` at the bottom
    pub scroll_top: Option<i64>,
    /// Its Claude Code session (the `claude_session` user var)
    pub session: Option<String>,
    /// What it reports with OSC 9;4: "42%", "error 42%", "indeterminate", "paused 42%"
    pub progress: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Scrollbar {
    /// `enable_scroll_bar`
    pub enabled: bool,
    /// Whether it shows beside the active pane (not while its program has the whole screen)
    pub shown: bool,
    /// Prompt marks on it
    pub marks: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rail {
    /// Open, or still open while it waits out its grace
    pub open: bool,
    /// "closed", "opening" (waiting out its rest), "open", "closing" (waiting out its grace)
    pub phase: String,
    /// How lit its ring is: "rest", "pending", "full"
    pub light: String,
    /// The panel's card, open or closed as `open` says
    pub rect: Rect,
    pub rows: Vec<RailRow>,
    /// The row under the pointer
    pub hovered: Option<usize>,
    /// The row with the open tab's ring and bar; `None` while they're on the Conductor's pad
    pub open_row: Option<usize>,
    pub conductor: Option<ConductorPad>,
    /// A tab being dragged along the rail, by its row
    pub dragging: Option<usize>,
    /// A tab dragged from another window is over this one: an empty slot shows where it'd land
    pub drop_hint: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RailRow {
    pub row: usize,
    pub tab: u64,
    /// The window's index of its tab
    pub tab_index: usize,
    /// Whether the rail has room to show it
    pub visible: bool,
    /// Its slot, across the open panel, and its pad on the rail
    pub rect: Rect,
    pub pad: Rect,
    pub label: String,
    pub title: String,
    pub project: String,
    /// "none", "working", "waiting", "idle", "error"
    pub state: String,
    /// "Working", "Question", "Your turn", ...
    pub status: String,
    pub message: String,
    pub progress: Option<u8>,
    pub agents: usize,
    /// Its program's own tasks still running while it has handed the turn back
    pub waiting: usize,
    pub unread: bool,
    pub age: String,
    pub active: bool,
    pub hovered: bool,
    /// The tab menu's colour round its pad, "#rrggbb"
    pub colour: Option<String>,
    /// What the Conductor is doing with it: "thinking", "acting"
    pub conductor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConductorPad {
    /// "off", "read", "most", "all"
    pub level: String,
    /// What its pad says under the glyph
    pub label: String,
    /// Its remote control is on
    pub remote: bool,
    /// The tab holding its session
    pub tab: Option<u64>,
    /// Its tab is the open one: the open tab's ring and bar are on the pad
    pub open: bool,
    pub hovered: bool,
    pub pad: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Strip {
    pub title: String,
    pub summary: Option<String>,
    /// When the summary was written: "22:22 · 2m ago"
    pub caption: Option<String>,
    /// In the caption's place for a moment: "Config reloaded"
    pub notice: Option<String>,
    /// The second row's prompt
    pub prompt: Option<String>,
    /// Why a jump didn't get there, in the prompt's place
    pub note: Option<String>,
    /// "Zoomed · 2 of 3"
    pub badge: Option<String>,
    /// The card under the strip: "summary" or "prompt", while it's open
    pub card: Option<String>,
    /// The restart pill shows
    pub restart: bool,
    /// A jump back to a prompt is under way: Claude Code's view is being scrolled to it
    pub jumping: bool,
    /// What's lit under the pointer: "close", "maximize", "minimize", "restart", "words",
    /// "prompt", "arrow"
    pub lit: Vec<String>,
    /// Where each of those is
    pub items: Vec<Item>,
}

/// Something on the strip and where it is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub name: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Menu {
    /// "tab", "prompts", "conductor"
    pub kind: String,
    /// The tab menu's tab
    pub tab: Option<u64>,
    /// Its card
    pub rect: Option<Rect>,
    pub items: Vec<MenuItem>,
    /// The item under the pointer or picked with the arrow keys
    pub focus: Option<usize>,
    /// The tab menu's new name, while one is typed
    pub renaming: Option<String>,
    /// The Conductor's item waiting for its answer
    pub waiting: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MenuItem {
    pub label: String,
    pub checked: bool,
    /// Where it is, while it's shown (a long list shows some of its items)
    pub rect: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Landed {
    pub pane: u64,
    /// The row on the pane's screen, when it's there
    pub row: Option<usize>,
    pub left_ms: u64,
}

/// What DWM is playing, by name: kept per window as animations start, and asked what's still
/// in flight when a snapshot is taken.
///
/// Several numbers moving under one name (a pane's x, y, width and height) are one animation,
/// from the first one's start to the last one's end. An animation turned round before it ended
/// is still counted until the old one would have, so `animating` can stay true for at most
/// what it had left; it never goes false early.
#[derive(Debug, Default, Clone)]
pub struct InFlight {
    list: Vec<(&'static str, Instant, Instant)>,
}

impl InFlight {
    /// `name` plays from `start` to `end`.
    pub fn start(&mut self, name: &'static str, start: Instant, end: Instant) {
        if end <= start {
            return;
        }
        match self.list.iter_mut().find(|(n, _, until)| *n == name && *until >= start) {
            Some((_, from, until)) => {
                *from = (*from).min(start);
                *until = (*until).max(end);
            }
            None => {
                self.list.retain(|(n, _, _)| *n != name);
                self.list.push((name, start, end));
            }
        }
    }

    /// What's still playing at `now`, the rest forgotten, soonest to end first.
    pub fn at(&mut self, now: Instant) -> Vec<Animation> {
        self.list.retain(|(_, _, end)| *end > now);
        let mut list: Vec<Animation> = self
            .list
            .iter()
            .map(|&(name, start, end)| {
                let whole = end.saturating_duration_since(start).as_secs_f64();
                let done = now.saturating_duration_since(start).as_secs_f64();
                Animation {
                    name: name.to_string(),
                    progress: if whole > 0.0 { (done / whole).clamp(0.0, 1.0) } else { 1.0 },
                    left_ms: millis(end.saturating_duration_since(now)),
                }
            })
            .collect();
        list.sort_by(|a, b| a.left_ms.cmp(&b.left_ms).then_with(|| a.name.cmp(&b.name)));
        list
    }

    /// When the last of it ends.
    pub fn settles(&self) -> Option<Instant> {
        self.list.iter().map(|(_, _, end)| *end).max()
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }
}

/// A duration in whole milliseconds, rounded up so something left never reads as 0.
pub fn millis(d: Duration) -> u64 {
    let ms = d.as_micros().div_ceil(1000);
    ms.min(u64::MAX as u128) as u64
}

/// "#rrggbb" for a 0xRRGGBB colour.
pub fn hex(rgb: u32) -> String {
    format!("#{:06x}", rgb & 0xFF_FFFF)
}

#[derive(Default)]
struct Slot {
    latest: Option<Snapshot>,
    written: u64,
    submitted: u64,
}

/// Writes snapshots on a thread of its own, the newest winning: one submitted while another is
/// being written replaces any still waiting, so a burst of frames costs one write after the
/// one under way.
pub struct Writer {
    shared: Arc<(Mutex<Slot>, Condvar)>,
}

impl Writer {
    pub fn spawn(path: PathBuf) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Slot::default()), Condvar::new()));
        let theirs = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("lbterm-state".into())
            .spawn(move || run(&path, &theirs))?;
        Ok(Self { shared })
    }

    /// Write `snapshot` soon, in place of any not written yet.
    pub fn submit(&self, snapshot: Snapshot) {
        let (slot, wake) = &*self.shared;
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.latest = Some(snapshot);
        slot.submitted += 1;
        wake.notify_one();
    }

    /// Until everything submitted so far is on disk, or `timeout` passes: for tests, and for a
    /// window that's going. True if it was.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (slot, wake) = &*self.shared;
        let deadline = Instant::now() + timeout;
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        while slot.written < slot.submitted {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            slot = wake.wait_timeout(slot, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        true
    }
}

fn run(path: &Path, shared: &(Mutex<Slot>, Condvar)) {
    let (slot, wake) = shared;
    loop {
        let (snapshot, number) = {
            let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
            while slot.latest.is_none() {
                slot = wake.wait(slot).unwrap_or_else(|e| e.into_inner());
            }
            let number = slot.submitted;
            (slot.latest.take().expect("waited for one"), number)
        };
        match serde_json::to_vec_pretty(&snapshot) {
            Ok(bytes) => {
                if let Err(err) = write_atomic(path, &bytes) {
                    log::warn!("state file {}: {err:#}", path.display());
                }
            }
            Err(err) => log::warn!("state file: {err:#}"),
        }
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        slot.written = slot.written.max(number);
        wake.notify_all();
    }
}

/// `bytes` to `<path>.tmp`, then renamed over `path`, so a reader sees the old file or the
/// new one, whole. A reader holding `path` open without FILE_SHARE_DELETE makes the rename
/// fail until it lets go, so it's tried again for a moment.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    std::fs::write(&tmp, bytes)?;
    let started = Instant::now();
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(err) if started.elapsed() >= RENAME_FOR => return Err(err),
            Err(_) => std::thread::sleep(RENAME_PAUSE),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn window(id: u64) -> Window {
        Window {
            id,
            hwnd: 0x1234,
            frame: 3,
            focused: false,
            keyboard: true,
            animating: false,
            animations: vec![],
            waits: vec![],
            settled: true,
            loops: vec![],
            size: Size {
                width: 800,
                height: 600,
                dpi: 96,
            },
            pointer: None,
            active_tab: Some(0),
            active_pane: Some(0),
            tabs: vec![],
            panes: vec![],
            scrollbar: Scrollbar {
                enabled: true,
                shown: true,
                marks: 0,
            },
            rail: None,
            strip: None,
            menu: None,
            overlay: None,
            readout: None,
            landed: None,
            questions: None,
            hearts: 0,
        }
    }

    fn snapshot(frame: u64) -> Snapshot {
        Snapshot {
            version: VERSION,
            pid: 42,
            build: "test".into(),
            frame,
            time_ms: 0,
            windows: vec![window(0)],
        }
    }

    #[test]
    fn the_variable_names_an_absolute_path_or_nothing() {
        assert_eq!(from_env(None), None);
        assert_eq!(from_env(Some("".into())), None);
        assert_eq!(from_env(Some("state.json".into())), None);
        assert_eq!(from_env(Some(r"..\state.json".into())), None);
        let abs = if cfg!(windows) { r"C:\x\state.json" } else { "/x/state.json" };
        assert_eq!(from_env(Some(abs.into())), Some(PathBuf::from(abs)));
    }

    #[test]
    fn the_tmp_file_sits_beside_it() {
        assert_eq!(tmp_path(Path::new("/a/state.json")), PathBuf::from("/a/state.json.tmp"));
    }

    #[test]
    fn the_contract_fields_are_named_as_agreed() {
        let json = serde_json::to_value(snapshot(7)).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["pid"], 42);
        assert_eq!(json["frame"], 7);
        let w = &json["windows"][0];
        for key in ["id", "frame", "focused", "animating"] {
            assert!(w.get(key).is_some(), "{key} missing: {w}");
        }
        assert_eq!(w["animating"], false);
        assert_eq!(w["hwnd"], 0x1234);
        // Absent parts are there, as null, so a reader can tell "off" from a typo
        assert!(w["rail"].is_null());
        assert!(w.get("rail").is_some());
    }

    #[test]
    fn one_name_is_one_animation_from_first_start_to_last_end() {
        let t = Instant::now();
        let mut f = InFlight::default();
        f.start("pane.layout", t, t + ms(220));
        f.start("pane.layout", t, t + ms(200));
        f.start("pane.layout", t + ms(10), t + ms(250));
        let now = f.at(t + ms(125));
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].name, "pane.layout");
        assert!((now[0].progress - 0.5).abs() < 1e-9, "{now:?}");
        assert_eq!(now[0].left_ms, 125);
        assert_eq!(f.settles(), Some(t + ms(250)));
    }

    #[test]
    fn a_name_played_again_after_it_ended_starts_afresh() {
        let t = Instant::now();
        let mut f = InFlight::default();
        f.start("rail.marker", t, t + ms(100));
        f.start("rail.marker", t + ms(500), t + ms(800));
        assert_eq!(f.settles(), Some(t + ms(800)));
        let now = f.at(t + ms(650));
        assert!((now[0].progress - 0.5).abs() < 1e-9, "{now:?}");
    }

    #[test]
    fn what_has_ended_is_forgotten_and_the_rest_comes_soonest_first() {
        let t = Instant::now();
        let mut f = InFlight::default();
        f.start("strip.card", t, t + ms(160));
        f.start("outline.move", t, t + ms(300));
        f.start("light.press", t, t + ms(50));
        let names: Vec<String> = f.at(t + ms(100)).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["strip.card", "outline.move"]);
        assert!(f.at(t + ms(300)).is_empty(), "an animation that ends at now has ended");
        assert_eq!(f.settles(), None);
    }

    #[test]
    fn a_delayed_animation_is_in_flight_before_it_starts_moving() {
        let t = Instant::now();
        let mut f = InFlight::default();
        f.start("rail.words", t + ms(60), t + ms(220));
        let now = f.at(t);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].progress, 0.0);
        assert_eq!(now[0].left_ms, 220);
    }

    #[test]
    fn nothing_to_play_is_not_an_animation() {
        let t = Instant::now();
        let mut f = InFlight::default();
        f.start("pane.dim", t, t);
        assert!(f.at(t).is_empty());
        assert_eq!(f.settles(), None);
    }

    #[test]
    fn time_left_rounds_up() {
        assert_eq!(millis(Duration::from_micros(1)), 1);
        assert_eq!(millis(Duration::from_micros(1000)), 1);
        assert_eq!(millis(Duration::from_micros(1001)), 2);
        assert_eq!(millis(Duration::ZERO), 0);
    }

    #[test]
    fn rectangles_round_their_edges() {
        // Right at 40.6 and bottom at 25.6 round to 41 and 26
        assert_eq!(Rect::round(10.4, 20.6, 30.2, 5.0), Rect { x: 10, y: 21, w: 31, h: 5 });
        assert_eq!(Rect::round(0.0, 0.0, -3.0, 2.0).w, 0);
    }

    #[test]
    fn colours_are_hex() {
        assert_eq!(hex(0x3DDC84), "#3ddc84");
        assert_eq!(hex(0x00_0001), "#000001");
    }

    #[test]
    fn the_writer_leaves_the_newest_snapshot_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let writer = Writer::spawn(path.clone()).unwrap();
        for frame in 1..=50 {
            writer.submit(snapshot(frame));
        }
        assert!(writer.flush(Duration::from_secs(5)));
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["frame"], 50);
        assert!(!tmp_path(&path).exists(), "the tmp file was renamed over it");
    }

    #[test]
    fn a_reader_holding_the_file_only_delays_the_rename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_atomic(&path, b"{\"frame\":1}").unwrap();
        // Held open as .NET's File.ReadAllText holds it, sharing read alone
        let held = hold(&path);
        let reader = std::thread::spawn(move || {
            std::thread::sleep(ms(40));
            drop(held);
        });
        write_atomic(&path, b"{\"frame\":2}").unwrap();
        reader.join().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"frame\":2}");
    }

    #[test]
    fn a_reader_reading_while_frames_are_written_never_sees_half_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let writer = Writer::spawn(path.clone()).unwrap();
        writer.submit(snapshot(0));
        assert!(writer.flush(Duration::from_secs(5)));
        let reading = {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut last = 0;
                for _ in 0..200 {
                    // Shared for delete, as Wait-State reads it, so the writer is never held up
                    if let Ok(text) = read_shared(&path) {
                        let json: serde_json::Value = serde_json::from_str(&text).expect("a whole file");
                        let frame = json["frame"].as_u64().unwrap();
                        assert!(frame >= last, "frames only go up");
                        last = frame;
                    }
                }
                last
            })
        };
        for frame in 1..=300 {
            writer.submit(snapshot(frame));
            if frame % 30 == 0 {
                std::thread::sleep(ms(1));
            }
        }
        assert!(writer.flush(Duration::from_secs(5)));
        reading.join().unwrap();
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["frame"], 300);
    }

    #[cfg(windows)]
    fn hold(path: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ alone: a rename over it fails until it's closed
        std::fs::OpenOptions::new().read(true).share_mode(1).open(path).unwrap()
    }

    #[cfg(not(windows))]
    fn hold(path: &Path) -> std::fs::File {
        std::fs::File::open(path).unwrap()
    }

    #[cfg(windows)]
    fn read_shared(path: &Path) -> std::io::Result<String> {
        use std::io::Read;
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        let mut file = std::fs::OpenOptions::new().read(true).share_mode(7).open(path)?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    #[cfg(not(windows))]
    fn read_shared(path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}
