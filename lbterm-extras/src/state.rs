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
    /// The message loop's and the paints' counts, all windows, since the process started
    /// (crate::pace)
    pub pace: crate::pace::Counts,
    /// What the GUI thread did besides painting, all windows, since the process started
    /// (crate::work)
    pub work: crate::work::Counts,
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
    /// The modifiers it takes as held, `ctrl`, `shift` and `alt` in that order: what its keys
    /// and pointer last said, none without the keyboard. They tint the active outline.
    pub held: Vec<String>,
    /// Those of them the active outline shows: `held`, but Shift while a tab is right-dragged on
    /// the rail, where it says the drag connects tabs (crate::links::outline_held)
    pub tint: Vec<String>,
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
    /// The tab whose panes `panes` are: the open tab's id, but while a switch's held pane hasn't
    /// had its push the tab drawn before it, whose panes stand in for the open tab's as they were
    /// (wezterm-gui's termwindow/frame_wait.rs; Lachlan's 2b of 2026-11-08), or `None` when
    /// nothing was drawn before the wait and no pane is
    pub panes_tab: Option<u64>,
    /// Panes whose visuals are still on screen after leaving it, fading out: a closed pane's
    /// under the rest, a tab's switched from or closed over the next tab's. Each leaves once
    /// its fade is done
    pub fading_panes: Vec<u64>,
    /// How many pictures of the strips down the window's right (the scroll strip and the
    /// objectives strip) as a tab left showed them are still on screen, fading out over the next
    /// tab's with its panes. Each leaves once its fade is done
    pub fading_sides: usize,
    /// How many of the strip's words in flight are older than the newest, still fading out over
    /// it (a tab switched through while the one before it still faded; six in flight at most).
    /// Each leaves once its fade is done
    pub fading_swaps: usize,
    /// How many pane surfaces the compositor keeps for panes to come: once nothing fades, no
    /// more than the most panes the window has had on screen at once (one at least)
    pub spare_panes: usize,
    pub scrollbar: Scrollbar,
    /// The tab panel, with `lbterm_tab_panel` on
    pub rail: Option<Rail>,
    /// The window strip's words, with the tab panel hiding the tabs
    pub strip: Option<Strip>,
    /// The objectives panel down the right, with `lbterm_objectives` on (lbterm/OBJECTIVES.md)
    pub objectives: Option<Objectives>,
    /// The scroll strip left of it, with `lbterm_scroll_strip` on (lbterm/SCROLL-STRIP.md)
    pub scroll_strip: Option<ScrollStrip>,
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
    /// The Conductor's view, while it's open in this window (lbterm/TAB-PANEL.md, "The
    /// Conductor's view")
    pub conductor_view: Option<ConductorView>,
    /// The card of what a tab's Claude session has running, while it's open beside its row
    pub tasks: Option<TasksCard>,
    /// The thumbnails of images the panes link to, as last painted (lbterm/THUMBNAILS.md)
    pub thumbnails: Vec<Thumbnail>,
    /// The voice drafts' cards over the panes, as last painted (lbterm/CONTROL.md, "Drafts")
    pub drafts: Vec<DraftCard>,
    /// The questions a voice draft has ticked on the pane whose colours show, by number
    pub ticked: Vec<u32>,
    /// The red ring round the next question the draft hasn't answered, while there is one
    pub next: Option<NextQuestion>,
    /// The new-tab pad at the rail's foot and its picker (lbterm/NEW-TAB.md); none without the pad
    pub new_tab: Option<NewTab>,
    /// What the window last copied (the tab menu's Copy link, a selection, CopyTo, the Conductor's
    /// view): with the state file on every copy comes here and never to the clipboard, which a
    /// test window shares with Lachlan's (lbterm/PITFALLS.md, 151). Written `null` until one is
    /// made, so a script can tell a build that keeps copies here from one that doesn't
    pub copied: Option<String>,
    /// With the compositor, how many of its visuals the last frame drew, and how many it kept as
    /// they were, their swapchains showing what it would have drawn already (lbterm/COMPOSITOR.md,
    /// "Only what changed is drawn")
    pub visuals: Option<Visuals>,
}

/// The visuals a frame drew and kept
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Visuals {
    pub drawn: usize,
    pub kept: usize,
    /// Which it drew, as the compositor names them (lbterm_motion's `Target`, `Over`, `Swap`,
    /// `Pane(3)`, `Panel(Rows)`), in the order they were drawn
    pub which: Vec<String>,
    /// What drawing them made on the GPU: buffers and bind groups (wezterm-gui's gpu_work.rs).
    /// None once the window's layers and visuals are made, while nothing grows or moves
    pub made: usize,
    /// The bind groups the window keeps after the frame (wezterm-gui's gpu_work.rs, `kept`): the
    /// atlas's two and a uniform's for each visual's place drawn lately, at most config's
    /// `GPU_UNIFORMS_KEPT` of those. With D3D12 each takes a view in the device's view heap, the
    /// atlas's two each, so the heap holds this and two more, and the groups a frame on the GPU
    /// still holds
    pub bind_groups: usize,
    /// The vertex buffers it wrote, and how many bytes: only those a visual it drew is drawn
    /// from, and only when what they held was out of date
    pub uploads: usize,
    pub upload_bytes: usize,
    /// The templates it made again (wezterm-gui's render/kept.rs: `card-mask-wide`, `ring-spectrum`,
    /// `panel-glow`...), each made only when what it's made from changes
    pub built: Vec<String>,
    /// The render passes it recorded and the submits it made of them (wezterm-gui's
    /// render/composited.rs), and in microseconds how long it took to gather what each visual
    /// draws (writing the layers' GPU copies), to record the passes, and to finish and submit them
    pub passes: usize,
    pub submits: usize,
    pub upload_us: u64,
    pub record_us: u64,
    pub submit_us: u64,
    /// Quads a vertex buffer wasn't written with because their hashes matched what its GPU copy
    /// held, though their vertices didn't: a write to a quad its hash doesn't fold in, which would
    /// leave the old vertices on screen. Checked only in a test window that asks
    /// (LBTERM_CHECK_UPLOADS, wezterm-gui's gpu_work.rs), else none
    pub stale: Option<usize>,
}

/// The ring round the next question a voice draft should answer (lbterm/QUESTIONS.md)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NextQuestion {
    /// What its marker says: "Q2"
    pub label: String,
    /// The ring's round its marker at the edge of the screen, the question being off it
    pub off: bool,
    /// The ring, as drawn
    pub rect: Rect,
}

/// A voice draft's card over its pane
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DraftCard {
    /// The pane, by the mux server's id
    pub pane: u64,
    /// "text" or "picker"
    pub kind: String,
    /// What it says, new lines and all
    pub text: String,
    pub rect: Rect,
}

/// The Conductor's view while it's open: what the seat says, and where everything on it is.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct ConductorView {
    /// The seat answers on its pipe
    pub connected: bool,
    /// The Conductor: "off", "starting", "on", "stopping", "error"; "unknown" without the seat
    pub state: String,
    /// "read", "most", "all"
    pub mode: String,
    pub listening: bool,
    /// A turn is under way
    pub busy: bool,
    /// His messages waiting for it to end
    pub queued: u32,
    pub error: Option<String>,
    /// The view's card
    pub rect: Rect,
    /// The conversation's room
    pub conversation: Rect,
    /// How far it's scrolled up from the newest, and how far it can be, in pixels
    pub scroll: u32,
    pub scroll_max: u32,
    /// The conversation's items, in the order they show
    pub items: Vec<ViewItem>,
    /// The item whose reply is being written
    pub streaming: Option<String>,
    /// Every box that's typed in: "main" at the bottom, "answer:<waiting>" and
    /// "other:<waiting>:<question>" on waiting sessions' cards
    pub boxes: Vec<ViewBox>,
    /// The box typing goes to
    pub focus: String,
    /// The cards down its side, in order
    pub cards: Vec<ViewCard>,
    /// Every button, by name: "send", "stop", "control:mode most", "allow:ask-3",
    /// "pick:<waiting>:<question>:<option>", "chip:<waiting>:1a", "session:<id>", ...
    pub buttons: Vec<ViewButton>,
    /// Under the box: what the seat answered a command with
    pub notice: Option<String>,
    /// The words selected in the conversation
    pub selection: Option<String>,
    /// The button, card or box under the pointer, by its name
    pub hovered: Option<String>,
    /// When what it says about time next changes (a question's "just now" turning to "1 min
    /// ago", the day's name at midnight), Unix milliseconds: it paints then, and not before
    pub clock: Option<u64>,
}

/// One item of the conversation
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewItem {
    pub id: String,
    /// "you", "conductor", "tool", "hub", "peer", "notice"
    pub who: String,
    /// Its words, the first 200 characters
    pub text: String,
    pub queued: bool,
    /// The reply still being written
    pub streaming: bool,
    /// A tool call's status: "running", "done", "error", "denied"
    pub tool: Option<String>,
    /// What of it shows; `None` while it's scrolled out of sight
    pub rect: Option<Rect>,
}

/// A box that's typed in
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewBox {
    pub name: String,
    pub rect: Option<Rect>,
    pub text: String,
    /// The caret, in characters from the start
    pub caret: usize,
    pub focused: bool,
}

/// A card down the view's side
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewCard {
    /// "ask", "waiting", "session", "events", "suggestion", "notes"
    pub kind: String,
    pub id: String,
    pub title: String,
    pub rect: Option<Rect>,
}

/// A button on the view
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewButton {
    pub name: String,
    pub label: String,
    /// What of it shows; `None` while it's scrolled out of sight
    pub rect: Option<Rect>,
    /// Lit: the mode in force, a picked option, a rating given
    pub on: bool,
    pub enabled: bool,
    /// Pressed, waiting on the seat's answer
    pub pending: bool,
}

/// The card of what a tab's Claude session has running (lbterm/TAB-PANEL.md, "What's running")
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TasksCard {
    /// The rail's row it's beside
    pub row: usize,
    pub rect: Rect,
    /// Each line as it reads, its parts a space apart: "Agent Build the rail card 12m", a
    /// subagent's "Edit tab_panel.rs · 31 tools · 84k tokens" under it, "+3 more"
    pub lines: Vec<String>,
}

/// A thumbnail in a pane: the image's file, what's drawn of it, and where
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Thumbnail {
    pub pane: u64,
    pub path: String,
    /// "shown", "loading", "gone" (the file isn't there) or "unreadable"
    pub state: String,
    /// The picture as drawn, or its box while there's no picture: the part in view
    pub rect: Rect,
    /// The pointer's over it
    pub hovered: bool,
    /// Which decoding of its file is drawn (each has a number of its own), 0 while none is
    pub version: u64,
    /// How many places in the window's atlas it has taken: a new decoding of its file is drawn
    /// over the last one's, so this stays 1 while the file changes, and grows only when the atlas
    /// is made again or the picture's size changes
    pub places: u64,
}

/// An animation in flight: what moves, how far through its time it is (0 to 1), and how long
/// it has left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Animation {
    pub name: String,
    pub progress: f64,
    pub left_ms: u64,
    /// What played under the name in this animation, as numbers, the newest
    /// [`PLAYED_KEPT`]: left out of the file when the compositor gave none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub played: Vec<Played>,
}

/// What one animation the compositor gave DWM played, as numbers: where it started and ended,
/// the least and most it reached on the way, and how long it lasts (lbterm/STATE.md, `played`).
/// A test checks a look's values here rather than reading pixels (Lachlan's 3a of 2026-11-09):
/// a menu's flare reaching its full light and going back to none, a dissolve going from its share
/// to nothing. Several numbers under one name (a pane's x, y, width and height) are one each, in
/// the order they started
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Played {
    pub from: f64,
    pub to: f64,
    pub low: f64,
    pub high: f64,
    pub ms: u64,
}

/// The most of what played under one name an animation says, the newest kept
pub const PLAYED_KEPT: usize = 8;

/// The most played numbers [`InFlight`] keeps across every name, the oldest let go first
const PLAYED_HELD: usize = 256;

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
    /// "strip.card", "strip", "projects.card", "projects.pad", "objectives.detail",
    /// "objectives.list", "objectives.strip", "scrollbar", "split", "pane", or "window"
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
    /// Its cells as its text is drawn: the first cell's top left, `cols` cells wide and `rows`
    /// tall, so a cell is `w / cols` by `h / rows` exactly. Not `rect`: a card holds padding and
    /// what's left over of a cell at its edges, so `rect.h / rows` isn't a row's height
    pub cells: Rect,
    pub cols: usize,
    pub rows: usize,
    pub active: bool,
    pub zoomed: bool,
    pub title: String,
    pub alt_screen: bool,
    pub mouse_reporting: bool,
    /// The cursor, in cells from the top-left of its screen
    pub cursor: Cell,
    /// The cursor as this frame drew it, when it drew the pane: `cursor` is the pane's as the
    /// snapshot is taken, after the frame, and output that came while the frame was being drawn
    /// moves it there though the frame didn't draw it
    pub drawn_cursor: Option<Cell>,
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

/// The scroll strip: what it's the scrollbar of, its thumb, its marks, the card resting on one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScrollStrip {
    /// The strip, its ring included
    pub strip: Rect,
    /// Where the thumb and the marks go
    pub track: Rect,
    /// "claude" (the conversation in Claude Code's fullscreen view), "shell" (the pane's
    /// scrollback) or "none"
    pub mode: String,
    /// The pane it's the scrollbar of
    pub pane: Option<u64>,
    /// The thumb, where the view is; none while Claude Code's isn't known yet
    pub thumb: Option<Rect>,
    /// Every mark drawn, top to bottom
    pub marks: Vec<ScrollMark>,
    /// The conversation's rows, as laid out at the pane's width, and the row at the top of the
    /// view (Claude only)
    pub rows: Option<usize>,
    pub view_top: Option<usize>,
    /// The card resting on a mark shows, while it's open
    pub card: Option<ScrollCard>,
    /// A seek through Claude Code's conversation is under way
    pub seeking: bool,
    /// Why the last click didn't get there, or that a write to this pane didn't, while that's
    /// still said (lbterm_extras::scroll_note)
    pub note: Option<String>,
    /// The note's head: "Not there" (a click), "Not sent" (a write) or "May not have arrived" (a
    /// write lost with the connection)
    pub note_head: Option<String>,
    /// How many times the window has read a Claude pane's screen for where its view is: only
    /// when the conversation, its layout or the conversation's rows on screen changed
    pub looks: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScrollMark {
    /// "prompt", "question", "edit", ... (lbterm_extras::conversation::Kind), "prompt" at a shell
    pub kind: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScrollCard {
    pub rect: Rect,
    pub lines: Vec<String>,
}

/// The objectives panel: its strip and the marks on it, and its cards while they're open.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Objectives {
    /// The closed strip, its ring included
    pub strip: Rect,
    /// A mark per visible rail row, level with its pad
    pub marks: Vec<ObjectiveMark>,
    /// The card of each tab's first objective, while it's open
    pub list: Option<ObjectivesList>,
    /// The card of one session's whole list, while it's open
    pub detail: Option<ObjectivesDetail>,
    /// The rail row under the pointer, whose detail shows
    pub hovered: Option<usize>,
    /// The projects pad on the strip and its card, with `lbterm_projects` set
    pub projects: Option<ProjectsCard>,
}

/// The projects card (lbterm/OBJECTIVES.md, "The projects card").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectsCard {
    /// The pad's zone across the strip: what the pointer rests on to open the card
    pub pad: Rect,
    pub open: bool,
    /// The card's rows while it's open, busiest first
    pub rows: Vec<ProjectRow>,
    /// "15 more, quieter", when they don't all fit
    pub more: Option<String>,
    /// The row under the pointer, by its place in `rows`
    pub hovered: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectRow {
    pub name: String,
    /// The project a repo's row is listed under, as its config names it; None on a project's row
    pub parent: Option<String>,
    pub mail: usize,
    pub working: usize,
    pub waiting: usize,
    pub idle: usize,
    /// Subagents and background tasks
    pub parts: usize,
    pub rect: Rect,
    /// Its buttons, drawn while the pointer's on the row: `new`, `inmail` (with mail), `resume`
    pub chips: Vec<ProjectChip>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectChip {
    pub action: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObjectiveMark {
    pub row: usize,
    /// "done", "first-done", "drifted", "waiting" or "open"; none for a tab without objectives
    pub mark: Option<String>,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObjectivesList {
    pub rect: Rect,
    pub rows: Vec<ObjectivesRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObjectivesRow {
    pub row: usize,
    /// Its first objective, or the tab's name without one
    pub text: String,
    /// "2/3", or empty with one objective
    pub tally: String,
    pub mark: Option<String>,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObjectivesDetail {
    pub row: Option<usize>,
    pub rect: Rect,
    /// Its lines, top to bottom: the tab's name, then each objective's status, words and evidence
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rail {
    /// Open, or still open while it waits out its grace
    pub open: bool,
    /// "closed", "opening" (waiting out its rest), "open", "closing" (waiting out its grace), as
    /// it shows: while `held`, the held state's, not the one underneath
    pub phase: String,
    /// Held still under a menu's card, open or fading out (lbterm_panel's `Hover::hold`): `open`,
    /// `phase` and `light` say what shows, the pointer and its delays going on underneath
    pub held: bool,
    /// How lit its ring is: "rest", "pending", "full"
    pub light: String,
    /// The panel's card, open or closed as `open` says
    pub rect: Rect,
    /// The labels under the pads: "shown", or "faded" while the panel is open, where each row's
    /// title says the same name (Lachlan's 12a, lbterm/NEW-TAB.md, "Landing 4": covered by the
    /// open panel's words, so with the compositor they fade out as those fade in, `rail.words`)
    pub labels: String,
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
    /// The tab links' lines between rows (lbterm/LINKS.md)
    pub links: Vec<RailLine>,
    /// The tab connections' lines between rows (lbterm/LINKS.md, "Connections")
    pub connections: Vec<RailConnection>,
    /// A tab being right-dragged onto another to wait on it
    pub link_drag: Option<LinkDrag>,
    /// The card of the line the pointer rests on
    pub line_card: Option<LineCard>,
}

/// A tab connection's line on the rail: from its topmost member's row to its lowest, a mark at
/// each member's
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RailConnection {
    pub id: String,
    /// "suggested" or "connected"
    pub state: String,
    pub lane: usize,
    /// Its members' rows on this rail, top to bottom
    pub rows: Vec<usize>,
    /// Its lane's middle, and the y of each row's mark
    pub x: f64,
    pub ys: Vec<f64>,
    /// The marks' middle across: left of each pad, where a link's marks are
    pub mark_x: f64,
}

/// A tab link's line between two rows of the rail
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RailLine {
    /// The row that waits, and the row it waits on
    pub waiter: usize,
    pub on: usize,
    pub lane: usize,
    /// "waiting", "done", "error", "suggested"
    pub look: String,
    /// Where its lane runs, its middle, and how far: from the waiting row's end down or up to the
    /// other's
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
}

/// The card resting the pointer on a tab link's line, or a tab connection's, opens
/// (lbterm/LINKS.md): which line, where the card is, and its lines of words
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineCard {
    /// A link's line, by its place in `links`; none for a connection's
    pub line: Option<usize>,
    /// A connection's line, by its id; none for a link's
    pub connection: Option<String>,
    pub rect: Rect,
    pub lines: Vec<String>,
}

/// A tab being right-dragged onto another to wait on it, or with Shift held to connect them: the
/// row picked up, the row under the pointer it would wait on or connect with (a tab with a Claude
/// session that isn't itself), and the cards
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkDrag {
    pub row: usize,
    pub over: Option<usize>,
    /// The tab under the pointer has the objective its wait matches: both light green
    pub matched: bool,
    /// Shift is held: let go, it connects the two, the drag cyan and its cards both tabs'
    /// objectives
    pub connect: bool,
    /// The card following the pointer (what it waits for, or connecting its objectives) and the
    /// one beside the row it's over (that tab's objectives), with the matched block's place in each
    pub waits: Option<Rect>,
    pub aims: Option<Rect>,
    pub waits_match: Option<Rect>,
    pub aims_match: Option<Rect>,
    /// Each card's lines of words, top to bottom
    pub waits_words: Vec<String>,
    pub aims_words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RailRow {
    pub row: usize,
    /// Its tab, by id; none for the Starting row (`pending`), which has no tab yet: LBTerm's ids
    /// start at 0, so a made-up one would be a real tab's (lbterm/PITFALLS.md, 52)
    pub tab: Option<u64>,
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
    /// "none", "working", "waiting", "idle", "error", "linked"
    pub state: String,
    /// "Working", "Question", "Your turn", ...
    pub status: String,
    pub message: String,
    pub progress: Option<u8>,
    /// The progress is the Conductor's hub's guess, not the program's word
    pub estimated: bool,
    /// The guessed time left, "~4 min", while the progress is a guess
    pub left: String,
    /// Its subagents' guessed progress, oldest first
    pub agent_bars: Vec<u8>,
    pub agents: usize,
    /// Its program's own tasks still running while it has handed the turn back
    pub waiting: usize,
    /// Finished, with only extras on offer: the blue pad's orange ring
    pub extras: bool,
    /// Waiting on another tab and asking him something too: the orange ring round its pad
    pub asks: bool,
    /// Working, but its reply has written nothing for 3 minutes with no tool running
    /// (`claude_quiet`): the amber ring round its green pad. At 7 its state is "stuck"
    pub quiet: bool,
    /// Where its tab link stands while the link decides its pad: "waiting", "telling", "told",
    /// "stuck", "failed", "closed"
    pub link: Option<String>,
    /// The ids of the tab connections made that its session is in, those whose other members
    /// are on no row of this rail too: a connection leaves its pad and words as they are
    pub connected: Vec<String>,
    pub unread: bool,
    pub age: String,
    /// The idle timer's bars drawn over its pad, bottom first: "green", "yellow", "red". Only the
    /// lit ones are drawn (Lachlan's 12a), so none while it works or has no Claude, and none on
    /// a row too short for the timer
    pub bars: Vec<String>,
    /// What its Claude conversation has cost, "$12.34", at its second line's end on the open
    /// panel; empty until the statusline has said
    pub cost: String,
    pub active: bool,
    pub hovered: bool,
    /// The tab menu's colour round its pad, "#rrggbb"
    pub colour: Option<String>,
    /// What the Conductor is doing with it: "thinking", "acting"
    pub conductor: Option<String>,
    /// The Starting row: a tab asked for and not come yet, its label the project's or the
    /// shell's (lbterm/NEW-TAB.md, "The Starting row"); `tab` 0 and `state` "starting"
    pub pending: bool,
    /// It looks hidden from the Conductor: every one of its Claude sessions is on the
    /// Conductor's button.json `hidden`, or a Hide from Conductor picked for it waits for that
    /// (lbterm/NEW-TAB.md, section 8; `lbterm_extras::conductor::hide::Look`)
    pub hidden: bool,
    /// What `hidden` says is a tab menu pick still waiting for the Conductor's word, for up to
    /// its 5 s: true for a Show waiting too, with `hidden` false
    pub hidden_pending: bool,
    /// Parked (the mux tab's flag, Lachlan's 8a; lbterm/NEW-TAB.md, "Landing 4"): dimmed on the
    /// rail, a "Parked" chip on the open panel
    pub parked: bool,
}

/// The new-tab pad at the rail's foot (lbterm/NEW-TAB.md), and its picker while it's open
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewTab {
    /// The pad's pill
    pub rect: Rect,
    /// Where a press lands on it: its slot across the rail, on into the screen's corner while the
    /// window is maximized
    pub zone: Rect,
    /// The pointer is on it
    pub hover: bool,
    /// Its picker or its + menu is open: it shows lit
    pub open: bool,
    /// How many tabs the rail has no room for, "+N" over the pad
    pub hidden: usize,
    pub picker: Option<Picker>,
    /// The last tab asked for in this window, kept after it's done, so a test judges by frames
    /// and order what came of it, whatever frames it read
    pub start: Option<StartSaid>,
    /// Every tab asked for lately, oldest first, `start` the last of them: the eight newest,
    /// those still on their way among them (several can be, after Shift+Enter in the picker)
    pub starts: Vec<StartSaid>,
    /// The keys the open panel's pad row and the + menu name, as the key table binds them: none
    /// where nothing's bound
    pub keys: NewTabKeys,
    /// The tabs Lachlan closed lately, newest first, that Reopen and Ctrl+Shift+T open again (ten
    /// at most, in memory alone: lbterm/NEW-TAB.md, "Reopen")
    pub closed: Vec<ClosedSaid>,
}

/// The keys bound to `LbtermNewTab`'s three, "Ctrl+T" and so on
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewTabKeys {
    pub picker: Option<String>,
    pub shell: Option<String>,
    pub reopen: Option<String>,
}

/// A tab closed by hand, kept to reopen
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClosedSaid {
    pub label: String,
    /// Where Reopen opens it: its session's folder, else its pane's; "" when neither said one
    pub dir: String,
    /// The project that folder is in, as the picker names it; "" for none
    pub project: String,
    /// Each pane's Claude session, the active pane's first
    pub sessions: Vec<String>,
    /// When it closed, in Unix seconds
    pub at: f64,
}

/// A tab asked for (the Starting row's), by the window's frames: the last one painted when it was
/// asked for, the first that showed its row, and the one that showed how it ended
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StartSaid {
    /// Which start, counting from 1 in the window
    pub id: u64,
    pub label: String,
    pub asked: u64,
    /// When it was asked for and when it ended, in Unix milliseconds by the GUI's clock: a test
    /// holds the fold to never before its 20 s (and the strip's notice, put up as it folds, to
    /// never before its 4 s), however late either comes on a loaded PC
    pub asked_ms: u64,
    pub ended_ms: Option<u64>,
    /// None while its row hasn't shown: a tab that came before the next frame took its place
    pub shown: Option<u64>,
    /// "arrived" (the tab came) or "folded" (20 s up, or Lua took it back); none while it waits
    pub ended: Option<String>,
    pub ended_frame: Option<u64>,
    /// The tab that came, by id
    pub tab: Option<u64>,
    pub behind: bool,
}

/// The new-tab picker, while it's open
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picker {
    /// "claude", or "shell" (the + menu's Shell in a project…)
    pub mode: String,
    pub rect: Rect,
    /// The box, and what's typed in it
    pub input: Rect,
    pub query: String,
    /// The list, row 0 nearest the box, each with where it is while it's shown
    pub rows: Vec<PickerRow>,
    /// The highlighted row, by its place in `rows`
    pub highlight: Option<usize>,
    /// The first message the words after the name make
    pub message: Option<String>,
    /// "No project called …", with nothing named
    pub none: Option<String>,
    /// "Started …", after Shift+Enter started one behind
    pub started: Option<String>,
    /// What the keys do, under the box
    pub footer: String,
    /// The last frame painted when it was opened, and the first that drew it
    pub asked: u64,
    pub shown: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PickerRow {
    pub name: String,
    /// The folder a pick opens in
    pub dir: String,
    /// "here", "home", "recent", "all"; none once something's typed
    pub group: Option<String>,
    /// A repo of another project, listed under it
    pub repo: bool,
    /// The name's characters the box's first word matched, by index
    pub marks: Vec<usize>,
    pub rect: Option<Rect>,
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
    /// Its tab is the open one, or its view is open: the open tab's ring and bar are on the pad
    pub open: bool,
    pub hovered: bool,
    /// What's round its pad (lbterm_extras::conductor::Outline): "none" (off), "grey" (read only),
    /// "mode" (Accept most or Accept all at rest, a still hairline in the mode's colour) or
    /// "spectrum" (while it acts there, or the pointer's on it; with the compositor its ring fades
    /// in and out, `conductor.outline.fade` in flight, Lachlan's 7a)
    pub outline: String,
    /// Its hub says it's acting on a session, any, here or not, its least lap and linger included
    /// (conductor_hub::acting); false without a hub
    pub acting: bool,
    pub pad: Rect,
    /// What the Conductor asks to do, waiting on his Allow or Deny: the pad is orange while any
    /// wait
    pub asks: usize,
    /// How many tab connections are suggested with at least one member on this window's rail
    /// (crate::links::Links::suggested_on; lbterm/LINKS.md, "Connections"), the count on its
    /// cyan badge
    pub suggested: usize,
    /// Where that badge is drawn, its count in the pads' numbers' font: `None` at 0, when there's
    /// none
    pub badge: Option<Rect>,
    /// Where the remote control's mark is drawn, on the pad's other corner, while it shows
    /// (`remote`): the badge stays clear of it
    pub mark: Option<Rect>,
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
    /// How many messages the InMail button says wait, while it shows (render/inbox.rs)
    pub inbox: Option<usize>,
    /// What the agenda's button counts, while it shows (render/agenda.rs)
    pub agenda: Option<AgendaCounts>,
    /// What the context pill under the InMail button says, while it shows: the open tab's Claude
    /// cost, "$12.34", or "$–" before it's known (render/context.rs)
    pub context: Option<String>,
    /// The mic mark, while it shows: "listening", "hearing" or "muted" (lbterm/CONTROL.md)
    pub mic: Option<String>,
    /// A jump back to a prompt is under way: Claude Code's view is being scrolled to it
    pub jumping: bool,
    /// What's lit under the pointer: "close", "maximize", "minimize", "restart", "words",
    /// "prompt", "arrow", "inbox", "agenda", "context"
    pub lit: Vec<String>,
    /// Where each of those is
    pub items: Vec<Item>,
}

/// The agenda button's counts: what's urgent and what's failing, as its file gives them
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AgendaCounts {
    pub urgent: usize,
    pub failing: usize,
}

/// Something on the strip and where it is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub name: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Menu {
    /// "tab", "prompts", "conductor", "inbox", "agenda", "plus" (the new-tab pad's)
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
    /// The one-letter key it shows dim at its right, which picks it while the menu is open
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Its second line as drawn, when it has one: the tab menu's Close finished tabs lists the
    /// tabs it closes by label, cut to the card's width
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Where it is, while it's shown (a long list shows some of its items)
    pub rect: Option<Rect>,
    /// Shown greyed, doing nothing: no key, no highlight, no pick (the tab menu's Hide from
    /// Conductor while a pick for the tab waits, "Hiding from Conductor…"; Lachlan's 11b of
    /// 2026-11-09). Left out when false
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inert: bool,
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
/// what it had left; it never goes false early. One stopped where it is, its numbers set at once
/// in place of what DWM was playing, is cut there ([`InFlight::cut`]): DWM plays none of it after.
/// How long after an animation ended it's still kept: past the budget's quiet after it, which a
/// window waits out before it's at rest (lbterm-extras/src/budget.rs)
pub const KEPT_AFTER: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone)]
pub struct InFlight {
    list: Vec<(&'static str, Instant, Instant)>,
    /// What each animation started under a name played, with when it started ([`Played`]):
    /// only in a window with a state file, whose compositor works them out
    played: Vec<(&'static str, Instant, Played)>,
}

impl InFlight {
    /// `name` plays from `start` to `end`. What ended [`KEPT_AFTER`] before it started is
    /// forgotten as it goes in, so the list holds what plays now, not every name ever played.
    pub fn start(&mut self, name: &'static str, start: Instant, end: Instant) {
        if end <= start {
            return;
        }
        self.list.retain(|(_, _, until)| *until + KEPT_AFTER > start);
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
        // What played under a name before its animation now began is let go with it
        let list = &self.list;
        self.played
            .retain(|(n, at, _)| list.iter().any(|(m, from, _)| m == n && at >= from));
    }

    /// What the animation `name` started at `start` plays, as numbers ([`Played`]), said under
    /// that name while it's in flight. Called after [`InFlight::start`] for it
    pub fn played(&mut self, name: &'static str, start: Instant, played: Played) {
        if self.played.len() >= PLAYED_HELD {
            self.played.remove(0);
        }
        self.played.push((name, start, played));
    }

    /// `name` stops at `at`: what DWM played under it was replaced by values set at once (the tab
    /// panel's wave stopped where it is as a menu's card comes, lbterm/STATE.md's `rail.wave`),
    /// so it ends there rather than at the end it had, and what's started under the name after
    /// counts on its own. Without it a stopped animation stayed in flight to its old end, its
    /// `left_ms` never falling at the stop, and a window read as animating for what DWM no
    /// longer played (PITFALLS 187)
    pub fn cut(&mut self, name: &'static str, at: Instant) {
        for (n, _, until) in self.list.iter_mut() {
            if *n == name && *until > at {
                *until = at;
            }
        }
    }

    /// What plays at `now`, the rest forgotten, as name, start and end, without making a list
    pub fn playing(&mut self, now: Instant) -> impl Iterator<Item = (&'static str, Instant, Instant)> + '_ {
        self.list.retain(|(_, _, end)| *end + KEPT_AFTER > now);
        self.list.iter().copied().filter(move |(_, _, end)| *end > now)
    }

    /// What's still playing at `now`, the rest forgotten, soonest to end first.
    pub fn at(&mut self, now: Instant) -> Vec<Animation> {
        self.list.retain(|(_, _, end)| *end > now);
        let played = &self.played;
        let mut list: Vec<Animation> = self
            .list
            .iter()
            .map(|&(name, start, end)| {
                let whole = end.saturating_duration_since(start).as_secs_f64();
                let done = now.saturating_duration_since(start).as_secs_f64();
                let mine: Vec<Played> = played
                    .iter()
                    .filter(|(n, at, _)| *n == name && *at >= start && *at <= end)
                    .map(|(_, _, p)| *p)
                    .collect();
                Animation {
                    name: name.to_string(),
                    progress: if whole > 0.0 { (done / whole).clamp(0.0, 1.0) } else { 1.0 },
                    left_ms: millis(end.saturating_duration_since(now)),
                    played: mine[mine.len().saturating_sub(PLAYED_KEPT)..].to_vec(),
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

    /// Every animation kept: its name, start and end (those ended go at the next `at`)
    pub fn list(&self) -> &[(&'static str, Instant, Instant)] {
        &self.list
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.played.clear();
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
            held: vec![],
            tint: vec![],
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
            panes_tab: Some(0),
            fading_panes: vec![],
            fading_sides: 0,
            fading_swaps: 0,
            spare_panes: 0,
            scrollbar: Scrollbar {
                enabled: true,
                shown: true,
                marks: 0,
            },
            rail: None,
            strip: None,
            objectives: None,
            scroll_strip: None,
            menu: None,
            overlay: None,
            readout: None,
            landed: None,
            questions: None,
            hearts: 0,
            conductor_view: None,
            tasks: None,
            thumbnails: vec![],
            drafts: vec![],
            ticked: vec![],
            next: None,
            new_tab: None,
            copied: None,
            visuals: None,
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
            pace: Default::default(),
            work: Default::default(),
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
    fn a_frame_names_the_visuals_it_drew() {
        let mut w = window(0);
        w.visuals = Some(Visuals {
            drawn: 2,
            kept: 22,
            which: vec!["Over".into(), "Swap".into()],
            made: 1,
            bind_groups: 26,
            uploads: 3,
            upload_bytes: 8160,
            built: vec!["ring-light".into()],
            passes: 2,
            submits: 1,
            upload_us: 30,
            record_us: 85,
            submit_us: 240,
            stale: Some(0),
        });
        let json = serde_json::to_value(&w).unwrap();
        assert_eq!(json["visuals"]["drawn"], 2);
        assert_eq!(json["visuals"]["kept"], 22);
        assert_eq!(json["visuals"]["which"], serde_json::json!(["Over", "Swap"]));
        assert_eq!(json["visuals"]["made"], 1);
        assert_eq!(json["visuals"]["bind_groups"], 26);
        assert_eq!(json["visuals"]["uploads"], 3);
        assert_eq!(json["visuals"]["upload_bytes"], 8160);
        assert_eq!(json["visuals"]["built"], serde_json::json!(["ring-light"]));
        assert_eq!(json["visuals"]["passes"], 2);
        assert_eq!(json["visuals"]["submits"], 1);
        assert_eq!(json["visuals"]["upload_us"], 30);
        assert_eq!(json["visuals"]["record_us"], 85);
        assert_eq!(json["visuals"]["submit_us"], 240);
        assert_eq!(json["visuals"]["stale"], 0);
        // Not checked: null, not 0, so a check that never ran can't pass for one that found none
        w.visuals.as_mut().unwrap().stale = None;
        assert!(serde_json::to_value(&w).unwrap()["visuals"]["stale"].is_null());
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
    fn what_played_is_said_under_its_animation_while_it_plays() {
        let t = Instant::now();
        let p = |high: f64| Played { from: 0.0, to: 0.0, low: 0.0, high, ms: 300 };
        let mut f = InFlight::default();
        f.start("menu.tab.hairline", t, t + ms(300));
        f.played("menu.tab.hairline", t, p(1.0));
        f.start("pane.dim", t, t + ms(100));
        let now = f.at(t + ms(50));
        let flare = now.iter().find(|a| a.name == "menu.tab.hairline").unwrap();
        assert_eq!(flare.played, vec![p(1.0)]);
        // A name the compositor gave no numbers for says none, and the file leaves the field out
        let dim = now.iter().find(|a| a.name == "pane.dim").unwrap();
        assert!(dim.played.is_empty());
        let json = serde_json::to_value(dim).unwrap();
        assert!(json.get("played").is_none(), "{json}");
        assert_eq!(serde_json::to_value(flare).unwrap()["played"][0]["high"], 1.0);
    }

    #[test]
    fn what_played_goes_with_its_animation_and_only_the_newest_are_said() {
        let t = Instant::now();
        let p = |k: u64| Played { from: k as f64, to: 0.0, low: 0.0, high: k as f64, ms: k };
        let mut f = InFlight::default();
        f.start("rail.marker", t, t + ms(100));
        f.played("rail.marker", t, p(1));
        // Played again once the first had ended: the first's numbers go with it
        f.start("rail.marker", t + ms(500), t + ms(800));
        f.played("rail.marker", t + ms(500), p(2));
        assert_eq!(f.at(t + ms(600))[0].played, vec![p(2)]);
        // Many numbers under one name: the newest PLAYED_KEPT, in the order they started
        let mut g = InFlight::default();
        for k in 0..12 {
            g.start("pane.layout", t + ms(k), t + ms(300));
            g.played("pane.layout", t + ms(k), p(k));
        }
        let said: Vec<u64> = g.at(t + ms(100))[0].played.iter().map(|x| x.ms).collect();
        assert_eq!(said, (12 - PLAYED_KEPT as u64..12).collect::<Vec<_>>());
        g.clear();
        assert!(g.at(t).is_empty());
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
    fn an_animation_cut_where_it_is_ends_there_and_what_follows_counts_on_its_own() {
        let t = Instant::now();
        let mut f = InFlight::default();
        f.start("rail.wave", t, t + ms(900));
        f.start("pane.layout", t, t + ms(400));
        f.cut("rail.wave", t + ms(100));
        let now = f.at(t + ms(150));
        assert_eq!(now.len(), 1, "the other name plays on");
        assert_eq!(now[0].name, "pane.layout");
        // The fade it stops with, started at the cut, is all that's left of it
        f.start("rail.wave", t + ms(100), t + ms(260));
        let wave: Vec<_> = f.at(t + ms(150)).into_iter().filter(|a| a.name == "rail.wave").collect();
        assert_eq!(wave.len(), 1);
        assert_eq!(wave[0].left_ms, 110);
        // A cut after the end changes nothing
        f.cut("pane.layout", t + ms(500));
        assert_eq!(f.at(t + ms(300)).iter().filter(|a| a.name == "pane.layout").count(), 1);
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
    fn the_list_holds_what_plays_not_every_name_ever_played() {
        let t = Instant::now();
        let mut f = InFlight::default();
        // A hundred names played one after another, each over long before the next
        let names: Vec<&'static str> = (0..100).map(|i| &*Box::leak(format!("n{i}").into_boxed_str())).collect();
        for (i, name) in names.iter().enumerate() {
            let at = t + Duration::from_secs(6 * i as u64);
            f.start(name, at, at + ms(200));
        }
        assert_eq!(f.list().len(), 1, "only the last: the rest ended over {KEPT_AFTER:?} before it");
        // What plays now, without the one that ended
        let last = t + Duration::from_secs(6 * 99);
        f.start("rail.edge", last + ms(100), last + ms(400));
        let playing: Vec<_> = f.playing(last + ms(300)).map(|(n, _, _)| n).collect();
        assert_eq!(playing, ["rail.edge"]);
        assert_eq!(f.list().len(), 2, "the ended one kept a while, for the budget's quiet after it");
        assert_eq!(f.playing(last + Duration::from_secs(10)).count(), 0);
        assert!(f.list().is_empty());
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
