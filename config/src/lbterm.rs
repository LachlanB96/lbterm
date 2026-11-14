//! LBTerm, Lachlan's fork of WezTerm: the names it uses in place of WezTerm's,
//! and the switches for what only LBTerm does.
//!
//! Only names that would let LBTerm and an installed WezTerm find each other,
//! or that a person sees, change. Names that scripts and other programs
//! depend on stay WezTerm's: the `wezterm` Lua module, the `WEZTERM_*`
//! environment variables, `TERM_PROGRAM` and the terminfo entry.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wezterm_dynamic::{FromDynamic, ToDynamic};

/// Shown where WezTerm shows its own name; `wezterm.app_name` in Lua.
pub const APP_NAME: &str = "LBTerm";

/// Window class, Windows AppUserModelID (taskbar grouping, toasts) and the
/// key a GUI publishes its socket under. WezTerm's is `org.wezfurlong.wezterm`.
pub const APP_ID: &str = "com.lachlanb.lbterm";

/// The folder LBTerm uses under the runtime, data and cache dirs.
pub const DIR_NAME: &str = "lbterm";

/// Executable names without `.exe`. The cargo packages keep upstream's
/// names; each package's `[[bin]]` sets these.
pub const CLI_EXE: &str = "lbterm";
pub const GUI_EXE: &str = "lbterm-gui";
pub const MUX_SERVER_EXE: &str = "lbterm-mux-server";

/// The `about` line of each executable's `--help`.
pub const ABOUT: &str =
    "LBTerm, a fork of WezTerm (Wez's Terminal Emulator)\nhttp://github.com/wezterm/wezterm";

/// Config names in the order they're looked for: LBTerm's own, then
/// WezTerm's, so LBTerm starts out with the WezTerm config.
pub const CONFIG_NAMES: [&str; 2] = ["lbterm", "wezterm"];

/// Variables an LBTerm process takes out of its own environment as it starts
/// (`env_bootstrap::bootstrap`, the first thing the GUI, the mux server and the cli run), so
/// nothing it starts inherits them: a tab's environment is whichever of those built its command
/// (lbterm/MUX.md, "A tab's environment"), so each has to drop them itself.
///
/// `__COMPAT_LAYER`: Windows' Program Compatibility Assistant starts a program it has flagged
/// (lbterm-gui.exe, after abnormal exits such as `lbterm.ps1 restart`'s kill) with
/// `__COMPAT_LAYER=DetectorsAppHealth` when Explorer launches it, and every child inherits a
/// layer named there. Headless Edge relaunches itself on one, and its stdout never reaches whoever
/// started it (seen 2026-11-09, from a session started in LBTerm).
pub const DROPPED_ENV: [&str; 1] = ["__COMPAT_LAYER"];

/// Takes `DROPPED_ENV` out of this process's environment.
pub fn drop_inherited_env() {
    for name in DROPPED_ENV.iter() {
        std::env::remove_var(name);
    }
}

/// The control pipe the GUI of window class `class` listens on (lbterm/CONTROL.md): one a
/// class, so a try or test window never answers for the daily LBTerm. `lbterm control --class`
/// finds it the same way. A pipe's name can't hold a backslash after its prefix.
pub fn control_pipe(class: &str) -> String {
    format!(r"\\.\pipe\lbterm-control-{}", class.replace('\\', "-"))
}

/// The two named mutexes a mux domain's socket has (lbterm/MUX.md, "Starting the server";
/// mux/src/lbterm_serve.rs), which say what a connect refused can't: whether a server is there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuxSocketLock {
    /// The server's, taken just before it binds the socket and held for as long as it runs: a
    /// second server for the socket refuses to start, and a client the socket refuses waits for
    /// that server rather than start another
    Serving,
    /// A client's, while it connects to the socket and, when no server holds it, starts one and
    /// waits for it to answer: two clients starting at once (two windows opened together) start
    /// one server
    Starting,
}

/// The name of `lock` for the socket at `path`, `Local\lbterm-mux-serving-<hash>`: one session's,
/// like the socket. A socket's path is too long for a name and has backslashes, which a name
/// can't hold after its namespace, so it's named by the path's FNV-1a hash, lower-cased and its
/// slashes made one kind first, since the same socket is written either way.
pub fn mux_socket_lock(lock: MuxSocketLock, path: &Path) -> String {
    let kind = match lock {
        MuxSocketLock::Serving => "serving",
        MuxSocketLock::Starting => "starting",
    };
    mux_socket_name(kind, path)
}

/// The name of the section the server holding a socket's `Serving` lock says its process id in,
/// `Local\lbterm-mux-pid-<hash>`, for as long as it runs (mux/src/lbterm_serve.rs), so a client
/// that a server holding the socket never answers can say which process that is
pub fn mux_server_pid(path: &Path) -> String {
    mux_socket_name("pid", path)
}

fn mux_socket_name(kind: &str, path: &Path) -> String {
    let spelled = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in spelled.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!(r"Local\lbterm-mux-{kind}-{hash:016x}")
}

/// How long a client that has started a mux server tries its socket for: what upstream's tries
/// after a start came to (200 ms, then nine more 50 to 450 ms apart). A server that can't start
/// says so in its own log; the window says it didn't answer
pub const MUX_START_WAIT: std::time::Duration = std::time::Duration::from_millis(2450);

/// How often it tries it meanwhile. Upstream waited 200 ms before its first try and up to 450 ms
/// between the rest, so the tab came up to 450 ms after the server could give it; a refused
/// connect costs a fraction of a millisecond
pub const MUX_START_LOOK: std::time::Duration = std::time::Duration::from_millis(10);

/// How long a client waits on a socket that refuses it while a server holds it: its file
/// removed or taken from a live server, which binds it again at its next look (every 3 s,
/// wezterm-mux-server-impl's `keep_socket`), so two of those looks and a second
pub const MUX_SERVED_WAIT: std::time::Duration = std::time::Duration::from_secs(7);

/// How often it tries it meanwhile
pub const MUX_SERVED_LOOK: std::time::Duration = std::time::Duration::from_millis(50);

/// How long a client waits for another that's starting the same socket's server: twice the
/// longest that can take (`MUX_SERVED_WAIT`, or a start and `MUX_START_WAIT`)
pub const MUX_STARTING_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// How long a dead key's or an IME's composition that has ended waits for the composed
/// character's echo to paint its end, before asking for a frame of its own (wezterm-gui's
/// termwindow/echo.rs): longer than an echo takes at rest (5 to 13 ms to arrive, about 10 to its
/// frame) and under most load, and short enough that a character the pane doesn't echo, or a
/// composition cancelled, clears from the cursor with no wait anyone sees
pub const COMPOSITION_CLEAR_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// How long a pane out of a window's view prints nothing before a mux server pushes the lines it
/// held of it to that window (wezterm-mux-server-impl's lbterm_flow.rs; lbterm/MUX.md, "Tabs out
/// of view: output held at the server"), and the span the window's hidden_scan takes for a pane
/// out of view gone quiet, given it here (wezterm-gui's `hidden_scan::new_quiet`), since a push
/// that ended a hold is taken there as one of a pane quiet this long. Claude Code at work draws its spinner 5.5 to 14 frames a second, so a
/// session working is held for its whole turn, and one done and waiting has its screen pushed a
/// quarter of a second after it stops, before a switch to it from the rail's light
pub const HELD_QUIET: std::time::Duration = std::time::Duration::from_millis(250);

/// How long after a window first tries to paint a pane its mux server holds (a switch to a tab
/// working out of view) the frame waits for that pane's push before it draws what it has
/// (lbterm-extras' frame_wait; lbterm/MUX.md, "Tabs out of view: output held at the server",
/// Lachlan's 11a of 2026-11-04). The push is asked at the switch: every round trip measured here
/// was 0.3 to 5 ms and the slowest path measured beside a flood 25 to 28 ms, so it waits for the
/// push in each, and a server that doesn't answer makes a switch at most about twice today's
/// 31 to 41 ms, under the 100 ms at which a switch reads as slow
pub const SWITCH_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// How long a read of a pane its mux server holds (Lua's `get_lines_as_text`, `cli get-text`
/// through the window's socket, a voice draft sent to a pane out of view) waits, from its ask,
/// for the pane's push before it reads what the window has (wezterm-client's lbterm_current.rs;
/// lbterm/MUX.md, "What reads a held pane's lines", Lachlan's 6b of 2026-11-08). What it waits
/// for is what a switch does (`SWITCH_WAIT`): the same push, 0.3 to 5 ms a round trip as
/// measured and 25 to 30 ms behind the slowest work measured on the server. A Lua read waits on
/// the GUI thread, which takes no key and draws no frame meanwhile: 50 ms is three frames at
/// 60 Hz, once, when the server doesn't answer, after which its connection is late and no read
/// waits on it until it answers
pub const HELD_READ_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

/// The blocks a window's D3D12 allocator takes to place its buffers and textures in (wgpu's
/// `MemoryHints::Manual`, wezterm-gui's termwindow/webgpu.rs, asked for only on D3D12, the
/// compositor's backend): blocks of this in the GPU's own memory and upload blocks of half it in
/// RAM. wgpu's default, Performance, takes
/// 256 MB and 64 MB, committed whole for the few MB a terminal draws with: 192 MB more of the
/// GPU's memory and 32 MB more RAM a window, 224 MB more private (lbterm/PERF.md, finding 95).
/// MemoryUsage's 8 MB and 4 MB saved more but made a burst of glyphs the atlas hadn't seen 27%
/// slower to paint, 32 MB blocks 20%, and 64 MB 5.5% (a paired test over 20 rounds): each glyph
/// written stages at least 64 KB in an upload block, so one atlas's glyphs took dozens of small
/// blocks, each made and let go again. 128 MB measured no cost, so it's 128 (Lachlan's rule in
/// LBTerm, speed over memory: about 97 MB a window more than 64 MB, 155 MB less than wgpu's
/// default). The flow's fix is a frame's new glyphs written in one upload (lbterm/FLOWS.md, the
/// pane-output flow's proposal 11), after which this can be as small as memory wants.
/// gpu-allocator wants both sizes in whole 4 MB, from 4 to 256 MB
pub const GPU_MEMORY_BLOCK: u64 = 128 << 20;

/// The views a window's bind groups can hold at once (wgpu's `max_non_sampler_bindings`, asked
/// for in wezterm-gui's termwindow/webgpu.rs): with D3D12 the size of the shader-visible view heap
/// wgpu-hal makes whole with the device, 32 bytes a view here (Vulkan and OpenGL take nothing of
/// it). wgpu's default, 1,000,000, was 30.5 MB of the GPU's own memory a window, mapped into the
/// process and all zeros, in its private bytes but not its working set (lbterm/PERF.md, finding
/// 96); this is 2 MB. Each bind group takes a view for each texture or buffer in it and one more
/// when it has a sampler: the atlas's two groups two each, and a uniform's one, kept to
/// `GPU_UNIFORMS_KEPT`
pub const GPU_VIEW_DESCRIPTORS: u32 = 65_536;

/// What D3D12 takes of the GPU's memory for a buffer however small: a placed buffer is aligned and
/// sized to 64 KB (`GetResourceAllocationInfo`'s size for a uniform's 80 bytes), so each uniform
/// a window keeps holds this much (lbterm/PITFALLS.md, trap 111)
pub const GPU_BUFFER_PLACED: u64 = 64 << 10;

/// The most uniforms' bind groups a window keeps (wezterm-gui's termwindow/gpu_work.rs), bounded
/// by what they hold: a buffer of `GPU_BUFFER_PLACED` each, so 32 MB, a quarter of one of the
/// allocator's `GPU_MEMORY_BLOCK`s, and a view each, far under `GPU_VIEW_DESCRIPTORS`. One is
/// kept for each place a visual is drawn at while it's been drawn in the last 600 frames, at the
/// window's size and the one before (a uniform's projection is made from the size), so a window
/// keeps two sizes' places, about 22 with four panes, and what its motion drew at its size lately;
/// past this many, the half drawn longest ago go. Set from the view heap at 16,384 it allowed 1 GB,
/// and a window resized 600 times held 490 MB of the GPU's memory (lbterm/PERF.md, finding 96)
pub const GPU_UNIFORMS_KEPT: usize = (GPU_MEMORY_BLOCK / 4 / GPU_BUFFER_PLACED) as usize;

/// How much of the title bar is kept clear just left of the window's minimise button, in pixels
/// at 96 dpi (the window's dpi scales it): ITMan's always-on-top pin sits there, a window of its
/// own drawn as a fourth caption button 46 px wide with its right edge against the minimise
/// button, on the window in front, on any window kept on top and on a title bar under the mouse.
/// ITMan places it from DWMWA_CAPTION_BUTTON_BOUNDS, so it follows the buttons wherever they are;
/// LBTerm draws its title bar up to its own buttons, and without the gap the strip's caption and
/// the "Restart to update" pill ran under the pin (ITMan's InMail of 2026-11-08, Lachlan's E1)
pub const CAPTION_PIN_GAP: f32 = 46.0;

/// What `LbtermQuestions` does with the question colours on the pane it's run in
/// (lbterm/QUESTIONS.md). Bound to the right button's press and release, they show while it's
/// held; they also go when that button's let go anywhere else. A config shared with older builds
/// binds it only where `wezterm.has_action('LbtermQuestions')`: a build that doesn't know it
/// refuses the whole config, and the mux server keeps the build it started with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum LbtermQuestions {
    Show,
    Hide,
    Toggle,
}

/// What `LbtermConductorView` does with the Conductor's view in the window it's run in
/// (lbterm/TAB-PANEL.md, "The Conductor's view"): needs `lbterm_conductor.seat_pipe`. A config
/// shared with older builds uses it only where `wezterm.has_action('LbtermConductorView')`, as
/// for `LbtermQuestions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum LbtermConductorView {
    Show,
    Hide,
    Toggle,
}

/// What `LbtermNewTab` opens (lbterm/DESIGN.md, "The + pad"): the new-tab picker (Ctrl+T in
/// Lachlan's config), the default program in the open pane's folder (Ctrl+Alt+T), or the tab
/// closed last (Ctrl+Shift+T). A config shared with older builds binds it only where
/// `wezterm.has_action('LbtermNewTab')`, as for `LbtermQuestions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum LbtermNewTab {
    Picker,
    Shell,
    Reopen,
}

/// What `LbtermDraft` does with the voice draft over the pane it's run in (lbterm/CONTROL.md,
/// "Drafts"): type it and send it, type it into the prompt for him to finish by hand, or let it
/// go. A config shared with older builds binds it only where `wezterm.has_action('LbtermDraft')`,
/// as for `LbtermQuestions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum LbtermDraft {
    Send,
    Edit,
    Clear,
}

/// What `LbtermPark` does to the window's open tab (Lachlan's 8a, lbterm/NEW-TAB.md, "Landing
/// 4"): parks it if it isn't and unparks it if it is (Ctrl+Alt+D in Lachlan's config), or one or
/// the other. A parked tab is dimmed on the rail and passed over by the side buttons' hops. A
/// config shared with older builds binds it only where `wezterm.has_action('LbtermPark')`, as
/// for `LbtermQuestions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum LbtermPark {
    Toggle,
    Park,
    Unpark,
}

impl LbtermPark {
    /// Whether a tab `parked` now is to be parked
    pub fn parks(self, parked: bool) -> bool {
        match self {
            Self::Toggle => !parked,
            Self::Park => true,
            Self::Unpark => false,
        }
    }
}

/// Where `LbtermTab` goes from the window's open tab (Lachlan's 8a and 9a, lbterm/NEW-TAB.md,
/// "Landing 4"), the choice of tab being lbterm_extras::hop's: the side buttons in Lachlan's
/// config, plain for `Back` and `Forward` and with Shift for `Last` and `Waiting`. Every one
/// passes parked tabs by. A config shared with older builds binds it only where
/// `wezterm.has_action('LbtermTab')`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum LbtermTab {
    /// The nearest tab before the open one that isn't parked, wrapping
    Back,
    /// The nearest after it that isn't parked, wrapping
    Forward,
    /// The tab that's waited on him longest by the rail's status times, blocked on him first,
    /// then finished; from one of those, the next of them
    Waiting,
    /// The tab he was on last, still open and not parked
    Last,
}

/// `name`, plus `.exe` on Windows.
pub fn exe_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Whether windows draw through DirectComposition (lbterm/COMPOSITOR.md):
/// `lbterm_compositor` with the WebGpu front end, on Windows only.
pub fn uses_compositor(config: &crate::Config) -> bool {
    cfg!(windows)
        && config.lbterm_compositor
        && config.front_end == crate::FrontEndSelection::WebGpu
}

/// `config.lbterm_motion`: how panes move when the compositor is on. The
/// curves are text in CSS syntax (lbterm-motion's `Curve`), checked when
/// the config loads; the values come from the token table in the
/// `lbterm-motion` skill.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermMotion {
    /// `"on"`, `"off"`, or `"system"` to follow Windows' Animation effects
    #[dynamic(default = "default_motion_enabled", validate = "validate_motion_enabled")]
    pub enabled: String,
    /// Stretches every animation by this factor, for judging motion by eye
    #[dynamic(default = "default_motion_slowdown", validate = "validate_motion_slowdown")]
    pub slowdown: f64,
    /// Every pane rectangle in a split, close, swap, rotate or zoom
    #[dynamic(default = "default_pane_layout", validate = "validate_curve")]
    pub pane_layout: String,
    /// A new pane's fade in
    #[dynamic(default = "default_pane_enter", validate = "validate_curve")]
    pub pane_enter: String,
    /// A closing pane's fade out
    #[dynamic(default = "default_pane_exit", validate = "validate_curve")]
    pub pane_exit: String,
    /// The panes of a tab switched from or closed fading out over the next tab's
    /// (lbterm/DESIGN.md)
    #[dynamic(default = "default_tab_switch", validate = "validate_curve")]
    pub tab_switch: String,
    /// The tab panel opening over the panes
    #[dynamic(default = "default_panel_open", validate = "validate_curve")]
    pub panel_open: String,
    /// ...and closing
    #[dynamic(default = "default_panel_close", validate = "validate_curve")]
    pub panel_close: String,
    /// Its ring's light brightening as it opens, and dimming as it closes
    #[dynamic(default = "default_panel_glow", validate = "validate_curve")]
    pub panel_glow: String,
    /// The light that runs once round its ring as it opens
    #[dynamic(default = "default_panel_sweep", validate = "validate_curve")]
    pub panel_sweep: String,
    /// Its wave's water pulling back and fading once the crest breaks (`lbterm_tab_panel.wave`)
    #[dynamic(default = "default_panel_wash", validate = "validate_curve")]
    pub panel_wash: String,
    /// ...and the spray it throws scattering on and fading
    #[dynamic(default = "default_panel_splash", validate = "validate_curve")]
    pub panel_splash: String,
    /// The active pane's outline gliding to the pane that takes the
    /// keyboard (lbterm/DESIGN.md)
    #[dynamic(default = "default_focus_move", validate = "validate_curve")]
    pub focus_move: String,
    /// Panes dimming as they lose the keyboard, and back
    #[dynamic(default = "default_dim", validate = "validate_curve")]
    pub dim: String,
    /// The window buttons and the strip's pill brightening under the pointer, and back
    #[dynamic(default = "default_button_hover", validate = "validate_curve")]
    pub button_hover: String,
    /// Light answering the pointer the moment it arrives or leaves, before
    /// whatever it's waiting to do: the tab panel's ring going to pending
    /// on its rail, and back (lbterm/DESIGN.md, principle 1)
    #[dynamic(default = "default_hover", validate = "validate_curve")]
    pub hover: String,
    /// The active outline's light rising when the keyboard arrives from
    /// outside the window (lbterm/DESIGN.md, "Panes")...
    #[dynamic(default = "default_arrive_in", validate = "validate_curve")]
    pub arrive_in: String,
    /// ...and settling back
    #[dynamic(default = "default_arrive_out", validate = "validate_curve")]
    pub arrive_out: String,
    /// A click's ripple growing and fading
    #[dynamic(default = "default_press", validate = "validate_curve")]
    pub press: String,
    /// A copied or opened stretch of text lighting up...
    #[dynamic(default = "default_copy_in", validate = "validate_curve")]
    pub copy_in: String,
    /// ...and fading
    #[dynamic(default = "default_copy_out", validate = "validate_curve")]
    pub copy_out: String,
    /// Words changing in place on the window strip: the old ones fading out over the new
    #[dynamic(default = "default_swap", validate = "validate_curve")]
    pub swap: String,
    /// The light running up the rail from a tab's pad to the Conductor's as the tab is hidden
    /// from the Conductor or shown to it again (lbterm/NEW-TAB.md, section 8; Lachlan's 6a of
    /// 2026-11-01), lit and put out on `copy_in` and `copy_out`
    #[dynamic(default = "default_send", validate = "validate_curve")]
    pub send: String,
}

impl Default for LbtermMotion {
    fn default() -> Self {
        Self {
            enabled: default_motion_enabled(),
            slowdown: default_motion_slowdown(),
            pane_layout: default_pane_layout(),
            pane_enter: default_pane_enter(),
            pane_exit: default_pane_exit(),
            tab_switch: default_tab_switch(),
            panel_open: default_panel_open(),
            panel_close: default_panel_close(),
            panel_glow: default_panel_glow(),
            panel_sweep: default_panel_sweep(),
            panel_wash: default_panel_wash(),
            panel_splash: default_panel_splash(),
            focus_move: default_focus_move(),
            dim: default_dim(),
            button_hover: default_button_hover(),
            hover: default_hover(),
            arrive_in: default_arrive_in(),
            arrive_out: default_arrive_out(),
            press: default_press(),
            copy_in: default_copy_in(),
            copy_out: default_copy_out(),
            swap: default_swap(),
            send: default_send(),
        }
    }
}

fn default_motion_enabled() -> String {
    "system".to_string()
}

fn default_motion_slowdown() -> f64 {
    1.0
}

fn default_pane_layout() -> String {
    "spring(220ms, 0)".to_string()
}

fn default_pane_enter() -> String {
    "spring(160ms, 0)".to_string()
}

fn default_pane_exit() -> String {
    "120ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_tab_switch() -> String {
    "200ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_panel_open() -> String {
    "spring(220ms, 0)".to_string()
}

fn default_panel_close() -> String {
    "150ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_panel_glow() -> String {
    "spring(400ms, 0)".to_string()
}

fn default_panel_sweep() -> String {
    "1400ms cubic-bezier(0.33, 0.33, 0.4, 1)".to_string()
}

fn default_panel_wash() -> String {
    "450ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_panel_splash() -> String {
    "600ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_focus_move() -> String {
    "spring(300ms, 0)".to_string()
}

fn default_dim() -> String {
    "spring(300ms, 0)".to_string()
}

fn default_button_hover() -> String {
    "120ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_hover() -> String {
    "spring(250ms, 0)".to_string()
}

fn default_arrive_in() -> String {
    "80ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_arrive_out() -> String {
    "spring(220ms, 0)".to_string()
}

fn default_press() -> String {
    "320ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_copy_in() -> String {
    "180ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_copy_out() -> String {
    "260ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

fn default_swap() -> String {
    "200ms cubic-bezier(0.23, 1, 0.32, 1)".to_string()
}

/// The panel sweep's curve, another light running along a path: an even pace from the start
/// slowing to a soft stop. 600 ms is Lachlan's (his 6a of 2026-11-01)
fn default_send() -> String {
    "600ms cubic-bezier(0.33, 0.33, 0.4, 1)".to_string()
}

fn validate_motion_enabled(value: &String) -> Result<(), String> {
    match value.as_str() {
        "on" | "off" | "system" => Ok(()),
        other => Err(format!(
            "lbterm_motion.enabled is '{other}'; it takes 'on', 'off' or 'system'"
        )),
    }
}

fn validate_motion_slowdown(value: &f64) -> Result<(), String> {
    if *value > 0.0 && value.is_finite() {
        Ok(())
    } else {
        Err(format!(
            "lbterm_motion.slowdown is {value}; it must be above 0"
        ))
    }
}

fn validate_curve(value: &String) -> Result<(), String> {
    value
        .parse::<lbterm_motion::Curve>()
        .map(|_| ())
        .map_err(|err| format!("lbterm_motion: {err}"))
}

/// `config.lbterm_tab_panel`: the panel of tabs down the left of the window
/// (lbterm/TAB-PANEL.md). Closed, it's a rail of Claude status pads ringed
/// in red light, that the window gives up a strip for, like a border; while
/// the pointer rests on it, it opens and the panes move right to make room,
/// on `lbterm_motion.panel_open`, `panel_close` and `panel_glow`.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermTabPanel {
    #[dynamic(default)]
    pub enabled: bool,
    /// How wide the closed panel is, ring included
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_panel_rail")]
    pub rail_width: crate::Dimension,
    /// How wide the panel is open, ring included
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_panel_width")]
    pub width: crate::Dimension,
    /// How long the pointer rests on the rail before it opens
    #[dynamic(default = "default_panel_open_delay")]
    pub open_delay_ms: u64,
    /// How long after the pointer leaves it closes
    #[dynamic(default = "default_panel_close_delay")]
    pub close_delay_ms: u64,
    /// Minutes for the idle timer to go from green to all red, like
    /// TabPanel.exe's `idleFullMin`
    #[dynamic(default = "default_idle_full_minutes", validate = "validate_idle_full_minutes")]
    pub idle_full_minutes: f64,
    /// The tabs in the tab bar too. Off, the bar is a slim strip the panes'
    /// colour holding only the window's buttons, to drag the window by
    #[dynamic(default)]
    pub tab_bar: bool,
    /// A label under each pad on the closed rail, saying what the tab is
    /// for: the name it was given, else the label Terminal's summary worker
    /// wrote for its Claude session (`lbterm_summaries`), else its title
    #[dynamic(default = "crate::default_true")]
    pub labels: bool,
    /// How many lines a label may take
    #[dynamic(default = "default_label_lines", validate = "validate_label_lines")]
    pub label_lines: usize,
    /// The labels' size, in points, in the tab bar's font
    #[dynamic(default = "default_label_font_size", validate = "validate_label_font_size")]
    pub label_font_size: f64,
    /// A wave of characters washing over the panes just ahead of the edge as it opens
    #[dynamic(default)]
    pub wave: LbtermPanelWave,
    /// The new-tab pad at the rail's foot (lbterm/DESIGN.md, "The + pad"): a click opens the
    /// picker of `lbterm_projects` to start Claude in one, a right click the + menu (the launch
    /// menu's shells here, a shell in a project, tabs closed lately, a new window). False: no
    /// pad, and its slot goes to the tabs
    #[dynamic(default = "crate::default_true")]
    pub new_tab: bool,
}

/// `lbterm_tab_panel.wave` (lbterm/TAB-PANEL.md, "The wave"): an experiment, Lachlan's ask of
/// 2026-10-24. As the panel opens, columns of characters wash out over the panes just ahead of
/// its edge, a cell at a time, run on past where the edge stops and break into spray. It leaves
/// the edge's timing alone: both start in the same commit, the wave timed by
/// `lbterm_motion.panel_open`, `panel_wash` and `panel_splash`.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermPanelWave {
    #[dynamic(default)]
    pub enabled: bool,
    /// How far ahead of the edge it sets off: the edge's speed times `panel_open`'s time constant
    /// (its duration over 2π), times this
    #[dynamic(default = "default_wave_lead", validate = "validate_wave_lead")]
    pub lead: f64,
    /// How far past the open panel it runs out over the panes before it breaks, in multiples of
    /// the edge's travel (222 px at 96 DPI)
    #[dynamic(default = "default_wave_reach", validate = "validate_wave_reach")]
    pub reach: f64,
    /// How much of the panes' text it hides under its crest, 0 (its characters over the text) to 1
    #[dynamic(default = "default_wave_cover", validate = "validate_wave_cover")]
    pub cover: f64,
}

impl Default for LbtermPanelWave {
    fn default() -> Self {
        Self {
            enabled: false,
            lead: default_wave_lead(),
            reach: default_wave_reach(),
            cover: default_wave_cover(),
        }
    }
}

fn default_wave_lead() -> f64 {
    0.8
}

fn default_wave_reach() -> f64 {
    1.0
}

fn validate_wave_reach(value: &f64) -> Result<(), String> {
    if (0.0..=4.0).contains(value) {
        Ok(())
    } else {
        Err(format!("lbterm_tab_panel.wave.reach is {value}; it must be 0 to 4"))
    }
}

fn default_wave_cover() -> f64 {
    0.85
}

fn validate_wave_lead(value: &f64) -> Result<(), String> {
    if (0.0..=2.0).contains(value) {
        Ok(())
    } else {
        Err(format!("lbterm_tab_panel.wave.lead is {value}; it must be 0 to 2"))
    }
}

fn validate_wave_cover(value: &f64) -> Result<(), String> {
    if (0.0..=1.0).contains(value) {
        Ok(())
    } else {
        Err(format!("lbterm_tab_panel.wave.cover is {value}; it must be 0 to 1"))
    }
}

impl Default for LbtermTabPanel {
    fn default() -> Self {
        Self {
            enabled: false,
            rail_width: default_panel_rail(),
            width: default_panel_width(),
            open_delay_ms: default_panel_open_delay(),
            close_delay_ms: default_panel_close_delay(),
            idle_full_minutes: default_idle_full_minutes(),
            tab_bar: false,
            labels: true,
            label_lines: default_label_lines(),
            label_font_size: default_label_font_size(),
            wave: LbtermPanelWave::default(),
            new_tab: true,
        }
    }
}

/// Whether the tab bar leaves its tabs to the tab panel: the panel on, and
/// `lbterm_tab_panel.tab_bar` not asked for.
pub fn hides_tabs(config: &crate::Config) -> bool {
    config.lbterm_tab_panel.enabled && !config.lbterm_tab_panel.tab_bar
}

/// The closed tab rail's width, ring included: 68 px at 96 DPI, the ring and a pill round each pad
/// clear of its corners. `lbterm_tab_panel.rail_width`'s default
pub const RAIL_WIDTH: crate::Dimension = crate::Dimension::Points(51.);

/// The questions pane's width, ring included (lbterm/QUESTIONS.md, "The questions pane"): the
/// closed tab rail's, Lachlan's 3c of 2026-11-14 ("all the other designs would hide the
/// conversation text"), so it holds each question's chip, label and mark and no words of it
pub const QUESTIONS_PANE_WIDTH: crate::Dimension = RAIL_WIDTH;

/// The weights' colours (lbterm/QUESTIONS.md, "The weights"; lbterm/DESIGN.md, "Colour"): an
/// option's share of 100 runs from red at 0 through amber at 50 to green at 100 (Lachlan,
/// 2026-11-12: "close to 0 would be red and close to 100 would be green"). Each reads on the
/// panes' #121212, red the dimmest at about 6:1, and stands apart from the question colours'
/// six (`lbterm_extras::questions::COLOURS`): the green leans yellow (a hue of about 95 degrees)
/// where Q2's green and Q5's teal lean blue, the amber is a full gold where Q3's is a soft one,
/// and the red is a red, not Q6's pink
pub const WEIGHT_RED: u32 = 0xFF5A4A;
pub const WEIGHT_AMBER: u32 = 0xFFD426;
pub const WEIGHT_GREEN: u32 = 0x8BDB4A;

/// A share's colour, 0xRRGGBB: straight between the stops either side of it, channel by channel
/// as they're written, rounded, so 70 is 0xD1D734, a yellow-green. A share past 100 is 100's
pub fn weight_colour(share: u8) -> u32 {
    let share = u32::from(share.min(100));
    let (from, to, at) = if share <= 50 {
        (WEIGHT_RED, WEIGHT_AMBER, share)
    } else {
        (WEIGHT_AMBER, WEIGHT_GREEN, share - 50)
    };
    let channel = |shift: u32| {
        let (a, b) = ((from >> shift) & 0xFF, (to >> shift) & 0xFF);
        ((a * (50 - at) + b * at + 25) / 50) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn default_panel_rail() -> crate::Dimension {
    RAIL_WIDTH
}

/// 290 px at 96 DPI
fn default_panel_width() -> crate::Dimension {
    crate::Dimension::Points(217.5)
}

fn default_panel_open_delay() -> u64 {
    150
}

fn default_panel_close_delay() -> u64 {
    300
}

fn default_idle_full_minutes() -> f64 {
    60.
}

/// Two lines of seven characters fit the 68 px rail at 7pt (Lachlan's pick,
/// 2026-10-20)
fn default_label_lines() -> usize {
    2
}

fn default_label_font_size() -> f64 {
    7.
}

fn validate_label_lines(value: &usize) -> Result<(), String> {
    if (1..=3).contains(value) {
        Ok(())
    } else {
        Err(format!("lbterm_tab_panel.label_lines is {value}; it must be 1, 2 or 3"))
    }
}

fn validate_label_font_size(value: &f64) -> Result<(), String> {
    if *value >= 4.0 && value.is_finite() {
        Ok(())
    } else {
        Err(format!(
            "lbterm_tab_panel.label_font_size is {value}; it must be 4 or more"
        ))
    }
}

fn validate_idle_full_minutes(value: &f64) -> Result<(), String> {
    if *value > 0.0 && value.is_finite() {
        Ok(())
    } else {
        Err(format!(
            "lbterm_tab_panel.idle_full_minutes is {value}; it must be above 0"
        ))
    }
}

/// One of `config.lbterm_triggers` (lbterm/TRIGGERS.md): a pattern that watches the text
/// panes show, what it colours, and what plays when it matches something new. Without
/// `lbterm_triggers` LBTerm has its two easter eggs (lbterm-triggers' `easter_eggs`); an empty
/// list has none.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermTrigger {
    /// What the `lbterm-trigger` Lua event calls it
    pub name: String,
    /// A Rust regex: `(?i)` for any case, `\b` for a word boundary
    pub regex: String,
    /// `#rrggbb` to tint what it matches towards
    #[dynamic(default)]
    pub tint: Option<String>,
    /// How far to tint, 0 to 1
    #[dynamic(default = "default_tint_amount")]
    pub tint_amount: f64,
    /// Red to violet across what it matches
    #[dynamic(default)]
    pub rainbow: bool,
    /// `"hearts"`
    #[dynamic(default)]
    pub effect: Option<String>,
    /// The effect plays at most once in this long, per pane
    #[dynamic(default = "default_trigger_cooldown")]
    pub cooldown_ms: u64,
}

fn default_tint_amount() -> f64 {
    0.45
}

fn default_trigger_cooldown() -> u64 {
    4000
}

/// `config.lbterm_conductor`: the Conductor (Lachlan's Life\Projects\Conductor, one Claude Code
/// session that directs the others) on the tab rail: its pad above the tabs, showing its state
/// file, a menu of actions on it, and a moving outline on the pads of the tabs it's thinking about
/// or acting on, as its hub says (lbterm/DESIGN.md, "The tab panel"; lbterm/TAB-PANEL.md). Needs
/// the tab panel.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermConductor {
    /// `{ "state": "off"|"on"|"remote", "mode": "read"|"most"|"all", "tooltip": "..." }`, every
    /// field optional, read off the GUI thread whenever it changes. No file: off.
    pub state_file: String,
    /// The hub's named pipe (`\\.\pipe\conductor-hub`): it tells LBTerm which sessions the
    /// Conductor is thinking about or acting on, and LBTerm tells it which tab holds which
    /// session. Unset: no outlines on the tabs.
    #[dynamic(default)]
    pub hub_pipe: Option<String>,
    /// The seat's named pipe (`\\.\pipe\conductor-seat`, Conductor's SEAT.md): the process that
    /// holds the Conductor's Claude and serves its interfaces. Set, a left click on the pad opens
    /// the Conductor's view, drawn here (lbterm/TAB-PANEL.md, "The Conductor's view"), and no tab
    /// stands for it. Unset: no view, and the tab holding its session is its tab, as before.
    #[dynamic(default)]
    pub seat_pipe: Option<String>,
    /// On the pad, where a tab has its number
    #[dynamic(default = "default_conductor_glyph")]
    pub glyph: String,
    /// On the pad's corner while the Conductor's remote control is on (state `remote`)
    #[dynamic(default = "default_conductor_remote_glyph")]
    pub remote_glyph: String,
    /// What a click on the pad offers, in order
    #[dynamic(default)]
    pub menu: Vec<LbtermConductorItem>,
}

fn default_conductor_glyph() -> String {
    "\u{266b}".to_string()
}

fn default_conductor_remote_glyph() -> String {
    "\u{25c9}".to_string()
}

/// An item in the Conductor's menu.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermConductorItem {
    pub label: String,
    /// The state file's fields and the values it's offered at, `{ state = { 'on', 'remote' } }`;
    /// a field the file hasn't got is `''`, but `state` is `'off'`. Empty: always.
    #[dynamic(default)]
    pub when: HashMap<String, Vec<String>>,
    /// Ticked while the state file's fields hold these: `{ mode = 'most' }`
    #[dynamic(default)]
    pub checked: HashMap<String, String>,
    /// Picked, the menu stays open with a turning arc on it until the state file changes (at
    /// most 45 s), for an item whose answer is the Conductor's new state. Unset, it closes at once.
    #[dynamic(default)]
    pub wait: bool,
    /// What it does: any action, `wezterm.action_callback` included
    pub action: Box<crate::keyassignment::KeyAssignment>,
}

/// `config.lbterm_links`: tab links (lbterm/LINKS.md). One tab's Claude session waits on
/// another's: a right-button drag of a tab's row onto another's makes the link, a line joins
/// their pads down the rail's left edge and the waiting pad turns purple. The links are what
/// `file` says, which a program of the config's keeps (lbterm_extras::links has its shape); what
/// Lachlan does on the rail goes to it through the `lbterm-tab-link` Lua event.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermLinks {
    /// The links file, read on a thread of its own whenever it changes
    pub file: String,
}

/// `config.lbterm_objectives`: the objectives panel down the right of the window
/// (lbterm/OBJECTIVES.md). Closed, it's a strip the window gives up like a border, with a mark
/// level with each tab's pad on the tab rail saying what became of what its Claude session was
/// started for, from `<sid>.objectives.json` in `lbterm_summaries`; while the pointer rests on it,
/// a card lists each tab's first objective, and resting on a row there shows that session's whole
/// list. Needs the tab panel and `lbterm_summaries`.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermObjectives {
    /// How wide the closed strip is, its ring included
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_objectives_width")]
    pub width: crate::Dimension,
    /// How wide each card is
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_objectives_card_width")]
    pub card_width: crate::Dimension,
    /// How long the pointer rests on the strip before the card opens
    #[dynamic(default = "default_objectives_open_delay")]
    pub open_delay_ms: u64,
    /// How long after the pointer leaves the cards they close
    #[dynamic(default = "default_objectives_close_delay")]
    pub close_delay_ms: u64,
}

impl Default for LbtermObjectives {
    fn default() -> Self {
        Self {
            width: default_objectives_width(),
            card_width: default_objectives_card_width(),
            open_delay_ms: default_objectives_open_delay(),
            close_delay_ms: default_objectives_close_delay(),
        }
    }
}

/// 14 px at 96 DPI: a hairline ring round a mark 8 px across
fn default_objectives_width() -> crate::Dimension {
    crate::Dimension::Points(10.5)
}

/// 360 px at 96 DPI: about fifty columns of the frame's font
fn default_objectives_card_width() -> crate::Dimension {
    crate::Dimension::Points(270.)
}

/// The strip's detail card's (render/strip.rs): a rest long enough that passing over the strip
/// on the way to the scrollbar doesn't open it
fn default_objectives_open_delay() -> u64 {
    300
}

fn default_objectives_close_delay() -> u64 {
    200
}

/// `config.lbterm_scroll_strip`: the scroll strip down the right of the window, left of the
/// objectives panel when that's on (lbterm/SCROLL-STRIP.md). It's the active pane's scrollbar:
/// at a shell, its scrollback with a mark at each prompt (OSC 133); in Claude Code's fullscreen
/// view, the conversation, read from the session's transcript under `transcripts`, with a mark at
/// each of your prompts and each thing Claude did worth finding again. A click on a mark takes
/// the pane there, and resting on one shows what it is. With it on, the panes' own scrollbar
/// goes, and its room with it. Needs the tab panel.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermScrollStrip {
    /// How wide the strip is, its ring included
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_objectives_width")]
    pub width: crate::Dimension,
    /// How wide the card resting on a mark opens
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_objectives_card_width")]
    pub card_width: crate::Dimension,
    /// How long the pointer rests on a mark before its card opens
    #[dynamic(default = "default_scroll_strip_open_delay")]
    pub open_delay_ms: u64,
    /// How long after the pointer leaves the mark and the card it closes
    #[dynamic(default = "default_objectives_close_delay")]
    pub close_delay_ms: u64,
    /// Claude Code's transcripts, a folder per project; `~/.claude/projects` when unset
    #[dynamic(default)]
    pub transcripts: Option<String>,
}

impl Default for LbtermScrollStrip {
    fn default() -> Self {
        Self {
            width: default_objectives_width(),
            card_width: default_objectives_card_width(),
            open_delay_ms: default_scroll_strip_open_delay(),
            close_delay_ms: default_objectives_close_delay(),
            transcripts: None,
        }
    }
}

/// Shorter than the objectives' rest: the pointer is put on a mark to read it
fn default_scroll_strip_open_delay() -> u64 {
    150
}

/// `config.lbterm_projects`: a pad on the window strip's second row, right of the context pill
/// under the InMail button (in the pill's place without it), that opens a card of every project in
/// `projects` while the pointer rests on it: how many Claude sessions are open in each, working,
/// waiting on you or idle, their subagents and background tasks, the InMail waiting for it
/// (counted from `lbterm_inbox`'s folders), and buttons that fire the `lbterm-project-action` Lua
/// event to start a session there, one told to read its InMail, or resume its last
/// (lbterm/OBJECTIVES.md, "The projects card"). A project with a `parent` is a repo of that one,
/// listed under it. Needs the tab panel and the strip's second row (`lbterm_prompt_history`), and
/// no longer `lbterm_objectives` (Lachlan's 1 of 2026-11-08: it moved off the objectives strip
/// when the context bar took that strip's room).
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermProjects {
    /// The projects, in any order: the card puts the busiest first
    pub projects: Vec<LbtermProject>,
    /// How wide the card is
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_projects_card_width")]
    pub card_width: crate::Dimension,
    /// How long the pointer rests on the pad before the card opens (a left press opens it at once)
    #[dynamic(default = "default_objectives_open_delay")]
    pub open_delay_ms: u64,
    /// And how long after it leaves the pad and the card they close
    #[dynamic(default = "default_objectives_close_delay")]
    pub close_delay_ms: u64,
    /// Where the new-tab picker keeps the last projects picked from it (a JSON list of names,
    /// newest first), read once at start and written through a rename, both off the GUI thread
    /// (lbterm_extras::newtab). Unset, the picks are kept in memory alone, for as long as the GUI
    /// runs
    #[dynamic(default)]
    pub recent_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermProject {
    /// What the card calls it
    pub name: String,
    /// Its folder: a session in it or below is its, unless it's in another project's folder further down
    pub dir: String,
    /// The project it's a repo of, by name (`Musica` for `Trainer`): the card lists it under that
    /// one's row, indented, with the repo mark. None: a project of its own
    #[dynamic(default)]
    pub parent: Option<String>,
    /// Its other names, which the new-tab picker takes whole as its name (any case, spaces
    /// optional), never as a prefix: Lachlan's lbterm.lua gives the root `AI` and `~`, as his
    /// @names take them (lbterm/NEW-TAB.md, "The picker")
    #[dynamic(default)]
    pub aliases: Vec<String>,
}

/// 480 px at 96 DPI: a project's name, its sessions, its mail and three buttons
fn default_projects_card_width() -> crate::Dimension {
    crate::Dimension::Points(360.)
}

/// `config.lbterm_inbox`: a button on the window strip, above the tab rail, saying how many
/// messages wait in `folders` (lbterm/INBOX.md). A click lists them; picking one fires the
/// `lbterm-inbox-open` Lua event with the message (with `taken`, the session id of the
/// conversation working on it, when one has taken it), which by default opens a new tab in the
/// folder above the message's. Needs the tab panel.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermInbox {
    /// Folders of waiting messages, one `.md` file each with a front matter block (`from`, `to`,
    /// `sent`, `subject`), as Lachlan's InMail writes them, and a `<message>.taken` claim beside
    /// one a conversation has taken; watched and read off the GUI thread when something in them
    /// changes. A folder that isn't there has none.
    pub folders: Vec<String>,
    /// On the button, before the count
    #[dynamic(default = "default_inbox_glyph")]
    pub glyph: String,
}

/// nf-md-email_outline, from the Symbols Nerd Font every build carries
fn default_inbox_glyph() -> String {
    "\u{f01f0}".to_string()
}

/// `config.lbterm_let_go`: a folder of requests to let go of a folder (lbterm/LET-GO.md), one
/// JSON file each, `{ folder, until, by }`: while one is there and its `until` (unix ms) hasn't
/// come, every folder LBTerm watches that is that folder or in it gives way to the folder above
/// it, so another program can rename or remove it. Without it, LBTerm lets go of nothing.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermLetGo {
    /// The folder the requests are written in: a quiet one, since LBTerm watches it
    pub folder: String,
}

/// `config.lbterm_agenda`: a button on the window strip beside the InMail button's place, with the
/// counts of what's urgent and what's failing from `file`, which drops a list of what's to do,
/// coming up and failing (lbterm/AGENDA.md). A button under an opened row fires the
/// `lbterm-agenda-press` Lua event, which runs what it does. Needs the tab panel.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermAgenda {
    /// The file of rows and counts another program keeps (lbterm_extras::agenda has its shape),
    /// read off the GUI thread when it changes; best in a folder of its own, since Windows says
    /// when a folder changes. No file, no button
    pub file: String,
    /// On the button, before the counts
    #[dynamic(default = "default_agenda_glyph")]
    pub glyph: String,
}

/// nf-md-format_list_checks, from the Symbols Nerd Font every build carries
fn default_agenda_glyph() -> String {
    "\u{f076e}".to_string()
}

/// `config.lbterm_context`: a pill on the window strip's second row, under the InMail button, with
/// what the open tab's Claude conversation has cost, which drops a list of the files in its
/// context (lbterm/CONTEXT.md). The dollars come from `<session>.usage.json` in the folder
/// `lbterm_summaries` names, the files from the session's transcript. Needs the tab panel and the
/// strip's second row (`lbterm_prompt_history`).
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermContext {
    /// Claude Code's transcripts, a folder per project: `~/.claude/projects` unless set here or in
    /// `lbterm_scroll_strip.transcripts`, which wins, since one reader serves both
    #[dynamic(default)]
    pub transcripts: Option<String>,
    /// LBTerm counts what each conversation has cost itself, from its transcript and its
    /// subagents', for the cost pane the pill's press opens: the split by what spent it, the token
    /// types and the graph (lbterm/CONTEXT.md, "The cost pane"). Off unless asked for (Lachlan's
    /// 2b of 2026-11-10, his kept off): then nothing of the count runs (no thread, no transcript
    /// read for it). Here, not in `lbterm_context_bar`, since the pane hangs from the pill, which
    /// works with the bar off (his 2b of that night). For someone without the Claude project's
    /// count
    #[dynamic(default)]
    pub count_cost: bool,
}

/// `config.lbterm_context_bar`: the context bar down the window's right, in the objectives
/// strip's room right of the scroll strip (lbterm/CONTEXT.md, "The context bar"; Lachlan's ask of
/// 2026-11-08): the open tab's Claude Code conversation's whole context window, top to bottom,
/// each file loaded into it a section of its own (memory files yellow), then Claude Code's own
/// parts, the conversation, and free space. Needs `lbterm_context` (the transcripts, and the
/// reader it shares with the scroll strip), `lbterm_summaries` (the window's size, from
/// `<session>.usage.json`) and the tab panel, whose rail's top and bottom it lines up with; with
/// `lbterm_objectives` set too, the bar takes the room and the objectives strip isn't drawn.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermContextBar {
    /// How wide the bar is at rest, its ring included
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_context_bar_width")]
    pub width: crate::Dimension,
    /// How wide the pane that slides out from under it is, less when the window hasn't the room
    #[dynamic(try_from = "crate::units::PixelUnit", default = "default_context_bar_pane_width")]
    pub pane_width: crate::Dimension,
    /// How long the pointer rests on the bar before it widens
    #[dynamic(default = "default_context_bar_widen_delay")]
    pub widen_delay_ms: u64,
    /// How long it rests on a section before the pane opens
    #[dynamic(default = "default_context_bar_open_delay")]
    pub open_delay_ms: u64,
    /// How long after the pointer leaves both the bar and the pane they close
    #[dynamic(default = "default_context_bar_close_delay")]
    pub close_delay_ms: u64,
}

impl Default for LbtermContextBar {
    fn default() -> Self {
        Self {
            width: default_context_bar_width(),
            pane_width: default_context_bar_pane_width(),
            widen_delay_ms: default_context_bar_widen_delay(),
            open_delay_ms: default_context_bar_open_delay(),
            close_delay_ms: default_context_bar_close_delay(),
        }
    }
}

/// Whether LBTerm counts conversations' cost itself (`lbterm_context.count_cost`): off unless the
/// pill is asked for with it, the bar on or off
pub fn counts_cost(config: &crate::Config) -> bool {
    config.lbterm_context.as_ref().map_or(false, |context| context.count_cost)
}

/// 14 px at 96 DPI: the objectives strip's width, whose room the bar takes
fn default_context_bar_width() -> crate::Dimension {
    crate::Dimension::Points(10.5)
}

/// 400 px at 96 DPI
fn default_context_bar_pane_width() -> crate::Dimension {
    crate::Dimension::Points(300.)
}

fn default_context_bar_widen_delay() -> u64 {
    150
}

fn default_context_bar_open_delay() -> u64 {
    300
}

fn default_context_bar_close_delay() -> u64 {
    250
}

/// The least a context bar section is drawn at rest, in pixels at 96 DPI (scaled with the
/// window's): `CONTEXT_SECTION_GAP` of the track under its colour, and the rest its colour. In a
/// 1M window a 300-token file is half a pixel of a 1,700 px bar, so every section is drawn at
/// least this tall, the pixels taken from the biggest, and the filled part still ends where
/// what's in use puts it (lbterm_extras::context_bar::runs; lbterm/CONTEXT.md, "Small sections")
pub const CONTEXT_SECTION_MIN: f64 = 3.0;

/// The track's colour under each context bar section that's drawn at least
/// `CONTEXT_SECTION_MIN` tall, so neighbours read apart, in pixels at 96 DPI
pub const CONTEXT_SECTION_GAP: f64 = 1.0;

/// How long the cost pane the context pill's press opens stays open after the pointer has left
/// the pill and the pane (lbterm/CONTEXT.md, "The cost pane"): the context bar's pane's grace
/// (`lbterm_context_bar.close_delay_ms`' default), since the two are panes of one kind
pub const COST_CLOSE_MS: u64 = 250;

/// `config.lbterm_summon`: a key that brings LBTerm forward from any other app (held with
/// RegisterHotKey only while LBTerm hasn't got the keyboard). Lachlan's is PageUp.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermSummon {
    /// A key as wezterm names them: `PageUp`, `F12`, `Insert`, a letter or digit
    pub key: String,
    /// `CTRL|SHIFT`, `ALT`, `SUPER`, or empty
    #[dynamic(default)]
    pub mods: String,
}

/// `config.lbterm_budget`: the budgets the GUI holds itself to as it runs, and says in its log
/// (target `lbterm::budget`, `lbterm.ps1 logs -Budget`) when it's over one (lbterm/PERF.md,
/// "Warnings in the daily build"; lbterm-extras/src/budget.rs). At rest, nothing should run: a
/// window with nothing happening to it that paints more than `rest_frames` frames in 10 s, past
/// what it paints on purpose (a blinking cursor at `animation_fps`, update-status's tick, a clock
/// in view), is named with who asked for them; so are timers and tasks while every window rests.
/// And each action has a time it should take, from what starts it to the end of the first frame
/// that shows it; one over says by how much and what the time went on.
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermBudget {
    /// Off: nothing is judged or logged (the counters `metrics` reports still count)
    #[dynamic(default = "default_true")]
    pub enabled: bool,
    /// How long a window stays busy after anything happens to it: input, its panes' output,
    /// the mux saying something of its tabs, a config reload, the end of an animation or a wait
    #[dynamic(default = "default_budget_quiet_ms")]
    pub quiet_ms: u64,
    /// The frames a window may paint at rest in 10 s, past what it paints on purpose
    #[dynamic(default = "default_budget_rest_frames")]
    pub rest_frames: u32,
    /// The frames a window may paint in 10 s for what its background tabs say (a title, a
    /// status, a light on a rail row), each in the 100 ms after it: past them, at rest
    #[dynamic(default = "default_budget_rest_news_frames")]
    pub rest_news_frames: u32,
    /// The timers that may fire in 10 s while every window rests
    #[dynamic(default = "default_budget_rest_timers")]
    pub rest_timers: u32,
    /// The tasks that may run in 10 s while every window rests, and how long they may take
    #[dynamic(default = "default_budget_rest_tasks")]
    pub rest_tasks: u32,
    #[dynamic(default = "default_budget_rest_task_ms", validate = "validate_budget_ms")]
    pub rest_task_ms: f64,
    /// Each action's time, in milliseconds
    #[dynamic(default)]
    pub actions: LbtermActionTimes,
}

impl Default for LbtermBudget {
    fn default() -> Self {
        Self {
            enabled: true,
            quiet_ms: default_budget_quiet_ms(),
            rest_frames: default_budget_rest_frames(),
            rest_news_frames: default_budget_rest_news_frames(),
            rest_timers: default_budget_rest_timers(),
            rest_tasks: default_budget_rest_tasks(),
            rest_task_ms: default_budget_rest_task_ms(),
            actions: LbtermActionTimes::default(),
        }
    }
}

/// `lbterm_budget.actions`: what each action should take at most, in milliseconds, from what
/// starts it (the key or click message, the end of a rest, a reload starting) to the end of the
/// first frame that shows it, presented. 0 times nothing for that one. The defaults are what a
/// test window drawn as Lachlan's took (lbterm/PERF.md, "Expected times"), with room: over one,
/// something is wrong
#[derive(Debug, Clone, PartialEq, FromDynamic, ToDynamic)]
pub struct LbtermActionTimes {
    /// A key or a paste sent to a pane, to the frame after the pane's next output: its echo
    #[dynamic(default = "default_action_key", validate = "validate_budget_ms")]
    pub key: f64,
    /// A tab switch by a key, a click or Lua
    #[dynamic(default = "default_action_tab", validate = "validate_budget_ms")]
    pub tab: f64,
    /// A new tab, to the first frame with it
    #[dynamic(default = "default_action_new_tab", validate = "validate_budget_ms")]
    pub new_tab: f64,
    /// A tab closed, to the first frame without it
    #[dynamic(default = "default_action_close_tab", validate = "validate_budget_ms")]
    pub close_tab: f64,
    /// A split, to the first frame with the new pane
    #[dynamic(default = "default_action_split", validate = "validate_budget_ms")]
    pub split: f64,
    /// The tab rail opening, from the end of the pointer's rest on it
    #[dynamic(default = "default_action_rail", validate = "validate_budget_ms")]
    pub rail: f64,
    /// The InMail button's list opening, from the click
    #[dynamic(default = "default_action_inbox", validate = "validate_budget_ms")]
    pub inbox: f64,
    /// The agenda's list opening, from the click on its button
    #[dynamic(default = "default_action_agenda", validate = "validate_budget_ms")]
    pub agenda: f64,
    /// The Conductor's view opening, from the click on its pad
    #[dynamic(default = "default_action_conductor_view", validate = "validate_budget_ms")]
    pub conductor_view: f64,
    /// The new-tab picker opening, from the click on the + pad or its key
    #[dynamic(default = "default_action_picker", validate = "validate_budget_ms")]
    pub picker: f64,
    /// A pick in the new-tab picker, to the first frame with the Starting row
    #[dynamic(default = "default_action_pick", validate = "validate_budget_ms")]
    pub pick: f64,
    /// A config reload, from when it started reading the file
    #[dynamic(default = "default_action_reload", validate = "validate_budget_ms")]
    pub reload: f64,
    /// A paint: building the frame, drawing it and presenting it
    #[dynamic(default = "default_action_paint", validate = "validate_budget_ms")]
    pub paint: f64,
}

impl Default for LbtermActionTimes {
    fn default() -> Self {
        Self {
            key: default_action_key(),
            tab: default_action_tab(),
            new_tab: default_action_new_tab(),
            close_tab: default_action_close_tab(),
            split: default_action_split(),
            rail: default_action_rail(),
            inbox: default_action_inbox(),
            agenda: default_action_agenda(),
            conductor_view: default_action_conductor_view(),
            picker: default_action_picker(),
            pick: default_action_pick(),
            reload: default_action_reload(),
            paint: default_action_paint(),
        }
    }
}

impl LbtermActionTimes {
    /// Each action's time by its name in lbterm-extras's budget.rs (`new-tab`), in its order
    pub fn by_name(&self) -> [(&'static str, f64); 13] {
        [
            ("key", self.key),
            ("tab", self.tab),
            ("new-tab", self.new_tab),
            ("close-tab", self.close_tab),
            ("split", self.split),
            ("rail", self.rail),
            ("inbox", self.inbox),
            ("agenda", self.agenda),
            ("conductor-view", self.conductor_view),
            ("picker", self.picker),
            ("pick", self.pick),
            ("reload", self.reload),
            ("paint", self.paint),
        ]
    }
}

fn default_true() -> bool {
    true
}

/// Long enough for what follows input by itself (a hover's fade, the rail closing after the
/// pointer left) to be done; a frame within it is never judged
fn default_budget_quiet_ms() -> u64 {
    1000
}

/// Half a frame a second: a summary, an InMail message, the hub's word arriving now and then
fn default_budget_rest_frames() -> u32 {
    5
}

/// Three a second: a dozen Claude sessions behind the open tab, each changing its light or its
/// status every few seconds, with room. A row repainted for every line a background tab prints
/// (a spinner) is many more
fn default_budget_rest_news_frames() -> u32 {
    30
}

fn default_budget_rest_timers() -> u32 {
    10
}

fn default_budget_rest_tasks() -> u32 {
    20
}

/// 0.2% of a core
fn default_budget_rest_task_ms() -> f64 {
    20.0
}

// The actions' defaults: about one and a half times the slowest 95th percentile six rounds of
// `perf\budget-log.ps1 -Measure -Runs 15` saw (three local, three on a mux domain, 2026-10-30, a
// test window drawn as Lachlan's, other sessions' test windows and builds running beside it),
// rounded up: over one, something is wrong. lbterm/PERF.md, "Expected times", has each one's
// numbers.

/// p50 20 to 41 ms locally, 41 to 44 on a mux domain; p95 at most 76
fn default_action_key() -> f64 {
    100.0
}

/// p50 15 to 22 ms; p95 at most 31
fn default_action_tab() -> f64 {
    50.0
}

/// p50 56 to 61 ms; p95 at most 99
fn default_action_new_tab() -> f64 {
    150.0
}

/// p50 18 to 26 ms; p95 at most 34
fn default_action_close_tab() -> f64 {
    50.0
}

/// p50 60 to 73 ms; p95 at most 92
fn default_action_split() -> f64 {
    150.0
}

/// From the end of the pointer's rest: p50 16 to 26 ms; p95 at most 39
fn default_action_rail() -> f64 {
    60.0
}

/// p50 32 to 36 ms; p95 at most 58
fn default_action_inbox() -> f64 {
    100.0
}

/// Not measured yet: the InMail list's, the list it's built like
fn default_action_agenda() -> f64 {
    100.0
}

/// Not measured here (it needs a seat played to it, and another branch is changing it): the
/// InMail list's, the other card a click opens over the panes
fn default_action_conductor_view() -> f64 {
    100.0
}

/// Warm: p50 2.1 ms, p95 at most 4.5 in a test window listing two projects (perf/budgets.json's
/// picker, two rounds, 2026-11-04), and 1.6 to 2.4 ms listing Lachlan's 49 by a click and by a key
/// bound to it (the review's probes, 2026-11-04). The first open in a GUI's life is cold, its words
/// laid out and drawn and its visual made for the first time: 42 to 95 ms in the review's probes,
/// over this, so it's logged as over every time a GUI starts (lbterm/NEW-TAB.md, "What the picker
/// costs"; budgets.json's picker-first measures it)
fn default_action_picker() -> f64 {
    10.0
}

/// p50 4.6 to 5.7 ms, p95 at most 9.9 in a test window (perf/budgets.json's pick, two rounds,
/// 2026-11-04, and the review's 16 runs)
fn default_action_pick() -> f64 {
    15.0
}

/// From when it starts reading the file, Lachlan's whole config run again: p50 79 to 90 ms; p95
/// at most 136
fn default_action_reload() -> f64 {
    200.0
}

/// Building, drawing and presenting a frame: p50 12 to 14 ms (a present waits on DWM for up to a
/// refresh); p95 at most 32, the longest 76
fn default_action_paint() -> f64 {
    50.0
}

fn validate_budget_ms(value: &f64) -> Result<(), String> {
    if value.is_finite() && *value >= 0.0 {
        Ok(())
    } else {
        Err(format!("a time in milliseconds can't be {value}: 0 times nothing"))
    }
}

static RELOAD: std::sync::Mutex<(u64, Option<std::time::Instant>)> = std::sync::Mutex::new((0, None));

/// A config reload starts reading the file (lib.rs's `reload`): the budget's `reload` action
/// is timed from here
pub fn note_reload_start() {
    let mut r = RELOAD.lock().unwrap_or_else(|e| e.into_inner());
    r.0 += 1;
    r.1 = Some(std::time::Instant::now());
}

/// The last reload's number and when it started, if one has
pub fn reload_started() -> Option<(u64, std::time::Instant)> {
    let r = RELOAD.lock().unwrap_or_else(|e| e.into_inner());
    r.1.map(|at| (r.0, at))
}

/// Everything a window's fonts, their glyphs and its cells' metrics are made from, as one value:
/// two configs with the same key make the same fonts, so a reload that leaves it as it was keeps
/// them, what they found as fallbacks, and the glyph atlas (wezterm-gui's `config_was_reloaded`;
/// lbterm/PERF.md, "What font setup costs"). Upstream threw them all away on every reload and found each font
/// again through the system's font lookup, which for a family Windows hasn't installed (the
/// bundled JetBrains Mono, Noto Color Emoji, Symbols Nerd Font Mono) read and parsed the whole
/// file of the font GDI put in its place: 11 of them a save in a window drawn as Lachlan's.
/// A field left out here that changes how text is drawn would leave a save's change unseen until
/// the next change that is in it, so the list errs on the side of more: the font settings, the
/// sizes of every font the GUI makes, the cell's metrics, the glyph cache's own settings, and
/// what the kept shapes were made with that the shape cache's key leaves out: the bidi settings,
/// which give a run its direction (upstream cleared the shapes on every reload, so a bidi change
/// took effect; PITFALLS 55).
pub fn font_key(config: &crate::Config) -> FontKey {
    let c = config;
    wezterm_dynamic::Value::Array(
        vec![
            c.font.to_dynamic(),
            c.font_rules.to_dynamic(),
            c.font_size.to_dynamic(),
            c.font_dirs.to_dynamic(),
            c.dpi.to_dynamic(),
            c.dpi_by_screen.to_dynamic(),
            c.window_frame.font.to_dynamic(),
            c.window_frame.font_size.to_dynamic(),
            c.char_select_font.to_dynamic(),
            c.char_select_font_size.to_dynamic(),
            c.command_palette_font.to_dynamic(),
            c.command_palette_font_size.to_dynamic(),
            c.pane_select_font.to_dynamic(),
            c.pane_select_font_size.to_dynamic(),
            c.lbterm_tab_panel.label_font_size.to_dynamic(),
            c.font_locator.to_dynamic(),
            c.font_rasterizer.to_dynamic(),
            c.font_colr_rasterizer.to_dynamic(),
            c.font_shaper.to_dynamic(),
            c.display_pixel_geometry.to_dynamic(),
            c.freetype_load_target.to_dynamic(),
            c.freetype_render_target.to_dynamic(),
            c.freetype_load_flags.to_dynamic(),
            c.freetype_interpreter_version.to_dynamic(),
            c.freetype_pcf_long_family_names.to_dynamic(),
            c.harfbuzz_features.to_dynamic(),
            c.ignore_svg_fonts.to_dynamic(),
            c.sort_fallback_fonts_by_coverage.to_dynamic(),
            c.search_font_dirs_for_fallback.to_dynamic(),
            c.use_cap_height_to_scale_fallback_fonts.to_dynamic(),
            c.line_height.to_dynamic(),
            c.cell_width.to_dynamic(),
            c.underline_thickness.to_dynamic(),
            c.underline_position.to_dynamic(),
            c.strikethrough_position.to_dynamic(),
            c.allow_square_glyphs_to_overflow_width.to_dynamic(),
            c.custom_block_glyphs.to_dynamic(),
            c.anti_alias_custom_block_glyphs.to_dynamic(),
            c.unicode_version.to_dynamic(),
            c.treat_east_asian_ambiguous_width_as_wide.to_dynamic(),
            c.cell_widths.to_dynamic(),
            c.experimental_pixel_positioning.to_dynamic(),
            c.use_box_model_render.to_dynamic(),
            c.bidi_enabled.to_dynamic(),
            c.bidi_direction.to_dynamic(),
        ]
        .into(),
    )
}

/// The side, in pixels, of the first glyph atlas a process's first window makes: one that holds a
/// window of text drawn as Lachlan's, so its first frame doesn't fill it and grow it again and
/// again, drawing everything again each time (upstream starts at 128 and doubles: four times for a
/// window of text). Later windows start at the largest any window of the process has grown to
/// (wezterm-gui's counters.rs). 1024 a side is 4 MB of the GPU's memory.
pub const ATLAS_START: usize = 1024;

/// What [`font_key`] gives: a value to compare
pub type FontKey = wezterm_dynamic::Value;

/// What a search for a fallback face finds depends on: where it looks and how it picks, not on
/// which fonts are asked for or their sizes (it asks for the faces at 16 px). Fonts with the same
/// fallback key share what their searches found (wezterm-font's lbterm.rs), across a zoom, a
/// font size changed and the process's windows.
pub fn fallback_key(config: &crate::Config) -> FontKey {
    let c = config;
    wezterm_dynamic::Value::Array(
        vec![
            c.font_dirs.to_dynamic(),
            c.font_locator.to_dynamic(),
            c.search_font_dirs_for_fallback.to_dynamic(),
            c.sort_fallback_fonts_by_coverage.to_dynamic(),
            c.ignore_svg_fonts.to_dynamic(),
            c.freetype_pcf_long_family_names.to_dynamic(),
        ]
        .into(),
    )
}

/// Whether two configs make the same fonts ([`font_key`])
pub fn fonts_alike(a: &crate::Config, b: &crate::Config) -> bool {
    font_key(a) == font_key(b)
}

/// A handle on a config of a test's own, so a test of fonts doesn't share the process's config
/// with the tests running beside it
#[doc(hidden)]
pub fn handle_for_test(config: crate::Config) -> crate::ConfigHandle {
    crate::ConfigHandle {
        config: std::sync::Arc::new(config),
        generation: 0,
    }
}

/// `config.lbterm_slow_ms`'s default: what holds the GUI thread longer than this is logged.
/// Two refreshes of a 60 Hz screen, so a hold this long skips a frame of typing even there.
/// Not 8: a present waits on DWM for up to a refresh, so on a 60 Hz screen a third of
/// ordinary typing frames' presents took over 8 ms (lbterm\perf\slow.ps1 -Measure, 2026-10-22)
pub fn default_slow_ms() -> u64 {
    33
}

/// The default size of the four caches a window keeps of what its lines looked like drawn:
/// `line_quad_cache_size` (each line's quads, by pane, what it shows, cursor and selection,
/// laid out at row 0 and moved to their row as they're copied in, so a line keeps its entry
/// as it scrolls and lines that look the same share one: wezterm-gui's render/pane.rs),
/// `line_to_ele_shape_cache_size` (each line's shaped runs, by what it says),
/// `shape_cache_size` (each run of text shaped) and `line_state_cache_size` (each line's hash
/// of what it says). Upstream's is 1024 each. A tab switched back to is drawn from them, so
/// with more lines in all the tabs a window shows than they hold, every hop laid its tab's
/// lines out and shaped them again (Lachlan's 1a, 2026-10-30): with 20 tabs of 91 rows of
/// varied output a side-button hop took 35.6 to 38.1 ms of the GUI thread at 1024 and 19.1 to
/// 20.9 at 4096, its frame 27 to 29 ms after the press at the median against 11 to 12.
/// An entry is kept until a newer one needs its room, so a window whose panes print fills
/// them whatever its tabs, and their memory is what they hold full. In a window of 90 rows,
/// 20,000 varied lines printed grew it by 118 to 122 MB, against 62 to 78 at 1024 (the
/// installed build), so about 45 to 55 MB more; lines the pane's width in runs of colour by
/// 298 to 306 MB against 191 to 192, about 110 more (perf/gui-thread.ps1's fill, interleaved,
/// 2026-10-31). That's with a cached quad's rare corners and gradient boxed (wezterm-gui's
/// `BoxedQuad`, 104 bytes for 220): before, 4096 cost about twice that over 1024 (160 to 171
/// and 402 to 412 MB). At 2048 the varied lines grew it by 70 and 81 MB, about what 1024 cost
/// before, and a hop through 20 tabs of 91 rows missed 0.9 to 2.9 lines. Since 2026-11-04 a
/// cached quad is the four vertices it's copied in as and its hash, 280 bytes, where a boxed
/// quad was about 130 with its allocation and its place in the line's list (perf/line-quads-by-
/// content): full, 20,000 varied lines grew the window by 167 and 170 MB against main's 105 and
/// 106, and -FillWide by 332 and 333 against 205 and 206 (the fill, interleaved, 2026-11-04).
/// Keyed by what a line shows, the cache keeps no copy of a line for each row it scrolled
/// through, so it fills more slowly, but full it holds about 1.6 times as much.
pub fn default_line_cache_size() -> usize {
    4096
}

/// The longest click streak a press counts to when no mouse binding names a longer one: a
/// triple click, the longest upstream's own bindings name ([`count_click`])
pub const CLICK_STREAK_MOST: usize = 3;

/// A press counted onto the last one (`last`, taken from the window), as upstream's
/// `LastMouseClick::add` counts it: the same button on the same cell within its 500 ms adds one
/// to the streak, anything else starts one at 1. Upstream's streak has no end, so a mouse
/// binding made for streaks 1 to 3 missed a fourth quick press, which then went to the pane's
/// program: Terminal's lbterm.lua binds the right button's hold, the question colours, for
/// streaks 1 to 3, and a fourth quick right press in Claude Code pasted instead
/// (lbterm/FLOWS.md, "A right hold, to the question colours", What could go 3; PITFALLS 356).
/// So the streak is held at the longest any of `bound`'s triggers for the button names, and at
/// [`CLICK_STREAK_MOST`] at least: every quick press past it counts as that one again, which a
/// binding of that streak takes, and a binding of a longer streak still gets its streak. The
/// window's other readers of the streak (the tab bar's double click) see 2 as before. `bound`
/// is the window's mouse bindings' triggers (`InputMap::mouse`'s keys): a few dozen compares a
/// press, on the GUI thread
pub fn count_click<'a>(
    last: Option<wezterm_term::LastMouseClick>,
    button: wezterm_term::input::MouseButton,
    position: wezterm_term::ClickPosition,
    bound: impl Iterator<Item = &'a crate::keyassignment::MouseEventTrigger>,
) -> wezterm_term::LastMouseClick {
    let mut click = match last {
        None => wezterm_term::LastMouseClick::new(button, position),
        Some(click) => click.add(button, position),
    };
    use crate::keyassignment::MouseEventTrigger::*;
    let longest = bound
        .filter_map(|trigger| match trigger {
            Down { streak, button: b } | Drag { streak, button: b } | Up { streak, button: b } => {
                (*b == button).then(|| *streak)
            }
        })
        .fold(CLICK_STREAK_MOST, usize::max);
    click.streak = click.streak.min(longest);
    click
}

/// The config files tried in order when no override or `WEZTERM_CONFIG_FILE`
/// is given; the first that exists wins. `exe_dir` is set on Windows, where a
/// config next to the executable comes first for portable installs.
/// `config_dirs` gives the XDG-style directories for one config name.
pub fn config_file_candidates(
    home: &Path,
    exe_dir: Option<&Path>,
    config_dirs: impl Fn(&str) -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut paths = vec![];
    if let Some(dir) = exe_dir {
        for name in CONFIG_NAMES {
            paths.push(dir.join(format!("{name}.lua")));
        }
    }
    for name in CONFIG_NAMES {
        paths.push(home.join(format!(".{name}.lua")));
        for dir in config_dirs(name) {
            paths.push(dir.join(format!("{name}.lua")));
        }
    }
    paths
}

#[cfg(test)]
mod test {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_compatibility_layer_windows_hands_lbterm_is_dropped_and_nothing_else() {
        std::env::set_var("__COMPAT_LAYER", "DetectorsAppHealth");
        std::env::set_var("LBTERM_DROP_TEST_KEPT", "1");
        drop_inherited_env();
        assert_eq!(std::env::var_os("__COMPAT_LAYER"), None);
        assert_eq!(std::env::var("LBTERM_DROP_TEST_KEPT").as_deref(), Ok("1"));
        std::env::remove_var("LBTERM_DROP_TEST_KEPT");
    }

    #[test]
    fn the_gpu_blocks_are_sizes_the_allocator_takes_as_they_are() {
        // gpu-allocator clamps each block to 4..=256 MB and rounds one that isn't whole 4 MB up,
        // with a warning; D3D12's upload blocks are half the device's
        const MB: u64 = 1 << 20;
        for size in [GPU_MEMORY_BLOCK, GPU_MEMORY_BLOCK / 2] {
            assert!((4 * MB..=256 * MB).contains(&size), "{} MB", size / MB);
            assert_eq!(size % (4 * MB), 0, "{} MB", size / MB);
        }
        // Under wgpu's default (Performance: 256 MB), or it saves nothing
        assert!(GPU_MEMORY_BLOCK < 256 * MB);
    }

    #[test]
    fn a_windows_view_heap_holds_the_bind_groups_it_keeps_with_room() {
        // Under wgpu's default and D3D12's most for a shader-visible heap at resource binding
        // tier 1 (1,000,000), or it saves nothing or the device can't be made
        assert!(GPU_VIEW_DESCRIPTORS < 1_000_000);
        // The uniforms kept (a view each) at most a quarter of it, which leaves the atlas's two
        // groups (two views each, and the old atlas's while a frame still holds them) and the
        // groups let go that a frame on the GPU still holds more room than the uniforms have
        assert!(GPU_UNIFORMS_KEPT * 4 <= GPU_VIEW_DESCRIPTORS as usize);
    }

    #[test]
    fn the_uniforms_a_window_keeps_are_bounded_by_their_memory() {
        // D3D12 places a buffer on 64 KB however small: what each uniform kept holds
        assert_eq!(GPU_BUFFER_PLACED, 64 * 1024);
        // All of them kept hold no more than a quarter of one of the allocator's blocks, so they
        // never take one of their own. Set from the view heap at 16,384 they could hold 1 GB
        assert!(GPU_UNIFORMS_KEPT as u64 * GPU_BUFFER_PLACED <= GPU_MEMORY_BLOCK / 4);
        // ...with room for more than a window draws with at once: every visual drawn, 23 to 25
        // with four panes and the rail, at its size and the one before, four times over
        assert!(GPU_UNIFORMS_KEPT >= 4 * 2 * 25);
    }

    #[test]
    fn fonts_alike_follow_what_makes_the_fonts() {
        let base = crate::Config::default_config();
        assert!(fonts_alike(&base, &base.clone()), "a config word for word");
        let mut other = base.clone();
        other.window_background_opacity = 0.5;
        other.max_fps = 30;
        other.scrollback_lines = 1234;
        assert!(fonts_alike(&base, &other), "opacity, frames and scrollback make no font");
        let changes: Vec<(&str, fn(&mut crate::Config))> = vec![
            ("font_size", |c| c.font_size += 1.0),
            ("font", |c| {
                c.font = crate::TextStyle {
                    font: vec![crate::FontAttributes::new("Consolas")],
                    foreground: None,
                }
            }),
            ("harfbuzz_features", |c| c.harfbuzz_features = vec!["calt=0".into()]),
            ("window_frame.font_size", |c| c.window_frame.font_size = Some(13.0)),
            ("line_height", |c| c.line_height = 1.2),
            ("freetype_load_target", |c| c.freetype_load_target = crate::FreeTypeLoadTarget::Mono),
            ("label_font_size", |c| c.lbterm_tab_panel.label_font_size += 1.0),
            ("custom_block_glyphs", |c| c.custom_block_glyphs = !c.custom_block_glyphs),
            ("dpi", |c| c.dpi = Some(144.0)),
            // Not fonts, but the shapes kept with them were made in their direction
            ("bidi_enabled", |c| c.bidi_enabled = !c.bidi_enabled),
            ("bidi_direction", |c| {
                c.bidi_direction = match c.bidi_direction {
                    wezterm_bidi::ParagraphDirectionHint::RightToLeft => {
                        wezterm_bidi::ParagraphDirectionHint::LeftToRight
                    }
                    _ => wezterm_bidi::ParagraphDirectionHint::RightToLeft,
                }
            }),
        ];
        for (what, change) in changes {
            let mut changed = base.clone();
            change(&mut changed);
            assert!(!fonts_alike(&base, &changed), "{} changes the fonts", what);
        }
    }

    #[test]
    fn a_fallback_search_is_keyed_by_where_it_looks_not_by_sizes() {
        let base = crate::Config::default_config();
        let mut sized = base.clone();
        sized.font_size += 3.0;
        sized.font = crate::TextStyle {
            font: vec![crate::FontAttributes::new("Consolas")],
            foreground: None,
        };
        assert!(
            fallback_key(&base) == fallback_key(&sized),
            "another font or size finds the same fallbacks"
        );
        let mut dirs = base.clone();
        dirs.font_dirs = vec!["C:/fonts".into()];
        assert!(fallback_key(&base) != fallback_key(&dirs), "font_dirs is where it looks");
        let mut sorted = base.clone();
        sorted.sort_fallback_fonts_by_coverage = !sorted.sort_fallback_fonts_by_coverage;
        assert!(fallback_key(&base) != fallback_key(&sorted));
    }

    #[test]
    fn the_first_atlas_holds_a_window_of_text() {
        assert!(ATLAS_START.is_power_of_two() && ATLAS_START >= 512);
    }

    #[test]
    fn each_window_class_has_its_own_control_pipe() {
        assert_eq!(control_pipe(APP_ID), r"\\.\pipe\lbterm-control-com.lachlanb.lbterm");
        assert_ne!(control_pipe(APP_ID), control_pipe("com.lachlanb.lbterm.try"));
        assert_eq!(control_pipe(r"a\b"), r"\\.\pipe\lbterm-control-a-b");
    }

    #[test]
    fn a_sockets_locks_are_its_own_however_its_path_is_written() {
        let daily = Path::new(r"C:\Users\someone\.local\share\lbterm\sock");
        let serving = mux_socket_lock(MuxSocketLock::Serving, daily);
        assert!(serving.starts_with(r"Local\lbterm-mux-serving-"), "{}", serving);
        assert_eq!(serving.len(), r"Local\lbterm-mux-serving-".len() + 16);
        // Nothing past the namespace a name can't hold
        assert!(!serving[r"Local\".len()..].contains('\\'), "{}", serving);
        // The same socket written another way
        for same in [
            "C:/Users/someone/.local/share/lbterm/sock",
            r"c:\users\SOMEONE\.local\share\lbterm\sock",
        ] {
            assert_eq!(mux_socket_lock(MuxSocketLock::Serving, Path::new(same)), serving);
        }
        // Another socket, and the other lock
        assert_ne!(
            mux_socket_lock(
                MuxSocketLock::Serving,
                Path::new(r"C:\Users\someone\.local\share\lbterm\e2e\4a16ddb1\sock")
            ),
            serving
        );
        let starting = mux_socket_lock(MuxSocketLock::Starting, daily);
        assert!(starting.starts_with(r"Local\lbterm-mux-starting-"), "{}", starting);
        assert_eq!(&starting[starting.len() - 16..], &serving[serving.len() - 16..]);
        // The server's pid beside them
        let pid = mux_server_pid(daily);
        assert!(pid.starts_with(r"Local\lbterm-mux-pid-"), "{}", pid);
        assert_eq!(&pid[pid.len() - 16..], &serving[serving.len() - 16..]);
    }

    #[test]
    fn a_client_waits_for_another_starting_longer_than_a_start_takes() {
        assert!(MUX_STARTING_WAIT >= MUX_SERVED_WAIT * 2);
        assert!(MUX_STARTING_WAIT >= MUX_START_WAIT * 2);
        assert!(MUX_START_LOOK < MUX_START_WAIT && MUX_SERVED_LOOK < MUX_SERVED_WAIT);
    }

    #[test]
    fn compositor_needs_the_flag_and_webgpu() {
        let mut config = crate::Config::default_config();
        assert!(!uses_compositor(&config), "off by default");
        config.lbterm_compositor = true;
        config.front_end = crate::FrontEndSelection::OpenGL;
        assert!(!uses_compositor(&config), "OpenGL can't draw into a visual");
        config.front_end = crate::FrontEndSelection::WebGpu;
        assert_eq!(uses_compositor(&config), cfg!(windows));
    }

    #[test]
    fn motion_defaults_are_the_skill_tokens_and_parse() {
        let m = LbtermMotion::default();
        assert_eq!(m.enabled, "system");
        assert_eq!(m.pane_layout, "spring(220ms, 0)");
        // Frequent actions take 300 ms or less (lbterm/DESIGN.md)
        assert_eq!((m.focus_move.as_str(), m.dim.as_str()), ("spring(300ms, 0)", "spring(300ms, 0)"));
        for curve in [
            &m.pane_layout,
            &m.pane_enter,
            &m.pane_exit,
            &m.tab_switch,
            &m.panel_open,
            &m.panel_close,
            &m.panel_glow,
            &m.panel_sweep,
            &m.panel_wash,
            &m.panel_splash,
            &m.focus_move,
            &m.dim,
            &m.button_hover,
            &m.hover,
            &m.arrive_in,
            &m.arrive_out,
            &m.press,
            &m.copy_in,
            &m.copy_out,
            &m.swap,
            &m.send,
        ] {
            assert_eq!(validate_curve(curve), Ok(()), "{curve}");
        }
        assert_eq!(m.button_hover, "120ms cubic-bezier(0.23, 1, 0.32, 1)");
        assert_eq!(m.hover, "spring(250ms, 0)");
        // The arrival pulse answers a key (the summon key, Alt+Tab): its rise and its fall's
        // perceptual duration come to 300 ms
        assert_eq!(
            (m.arrive_in.as_str(), m.arrive_out.as_str()),
            ("80ms cubic-bezier(0.23, 1, 0.32, 1)", "spring(220ms, 0)")
        );
        assert_eq!(m.press, "320ms cubic-bezier(0.23, 1, 0.32, 1)");
        assert_eq!(m.swap, "200ms cubic-bezier(0.23, 1, 0.32, 1)");
        // Decoration after a hide's answer (the row's look on `swap`), so longer than a key's 300 ms:
        // his 6a, on the sweep's curve
        assert_eq!(m.send, "600ms cubic-bezier(0.33, 0.33, 0.4, 1)");
        // A key closes the tab: 300 ms or less
        assert_eq!(m.tab_switch, "200ms cubic-bezier(0.23, 1, 0.32, 1)");
        assert_eq!(
            (m.copy_in.as_str(), m.copy_out.as_str()),
            ("180ms cubic-bezier(0.23, 1, 0.32, 1)", "260ms cubic-bezier(0.23, 1, 0.32, 1)")
        );
        assert_eq!(crate::Config::default_config().lbterm_motion, m);
    }

    #[test]
    fn motion_settings_are_checked() {
        assert!(validate_motion_enabled(&"On".to_string()).is_err());
        assert!(validate_motion_slowdown(&0.0).is_err());
        assert!(validate_curve(&"220ms wobble".to_string()).is_err());
        assert!(validate_curve(&"none".to_string()).is_ok());
    }

    #[test]
    fn the_tab_panel_is_off_unless_asked_for() {
        let config = crate::Config::default_config();
        assert_eq!(config.lbterm_tab_panel, LbtermTabPanel::default());
        assert!(!config.lbterm_tab_panel.enabled);
        let px = |d: crate::Dimension| {
            d.evaluate_as_pixels(crate::DimensionContext {
                dpi: 96.,
                pixel_max: 1000.,
                pixel_cell: 10.,
            })
        };
        assert_eq!(px(config.lbterm_tab_panel.rail_width), 68.);
        // The questions pane is the closed rail's width (Lachlan's 3c of 2026-11-14)
        assert_eq!(px(QUESTIONS_PANE_WIDTH), 68.);
        assert_eq!(px(config.lbterm_tab_panel.width), 290.);
        assert!(validate_idle_full_minutes(&0.0).is_err());
        // Labels under the pads, two lines of 7pt
        let panel = &config.lbterm_tab_panel;
        assert!(panel.labels);
        assert_eq!((panel.label_lines, panel.label_font_size), (2, 7.));
        assert!(validate_label_lines(&0).is_err() && validate_label_lines(&4).is_err());
        assert!(validate_label_font_size(&2.0).is_err());
        // The wave is an experiment, off unless asked for
        assert!(!panel.wave.enabled);
        assert_eq!((panel.wave.lead, panel.wave.reach, panel.wave.cover), (0.8, 1.0, 0.85));
        assert!(validate_wave_lead(&-0.1).is_err() && validate_wave_lead(&2.5).is_err());
        assert!(validate_wave_reach(&-1.0).is_err() && validate_wave_reach(&5.0).is_err());
        assert!(validate_wave_cover(&1.2).is_err() && validate_wave_cover(&0.0).is_ok());
        // The new-tab pad, on unless turned off
        assert!(panel.new_tab);
    }

    #[test]
    fn the_new_tab_pad_and_its_recent_file_come_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let panel: LbtermTabPanel = luahelper::from_lua_value_dynamic(
            lua.load("return { enabled = true, new_tab = false }").eval()?,
        )?;
        assert!(!panel.new_tab, "turned off: no pad and no slot");
        let projects: LbtermProjects = luahelper::from_lua_value_dynamic(
            lua.load("return { projects = { { name = 'A', dir = 'C:/A' } }, recent_file = 'C:/x/recent.json' }").eval()?,
        )?;
        assert_eq!(projects.recent_file.as_deref(), Some("C:/x/recent.json"));
        let projects: LbtermProjects = luahelper::from_lua_value_dynamic(lua.load("return { projects = { { name = 'A', dir = 'C:/A' } } }").eval()?)?;
        assert_eq!(projects.recent_file, None, "unset: in memory alone");
        assert!(projects.projects[0].aliases.is_empty(), "no other names unless given");
        let projects: LbtermProjects = luahelper::from_lua_value_dynamic(
            lua.load("return { projects = { { name = 'Root', dir = 'C:/AI', aliases = { 'AI', '~' } } } }").eval()?,
        )?;
        assert_eq!(projects.projects[0].aliases, vec!["AI".to_string(), "~".to_string()]);
        Ok(())
    }

    #[test]
    fn the_park_action_comes_from_lua_and_toggles() -> anyhow::Result<()> {
        use crate::keyassignment::KeyAssignment;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: bool = lua.load("return require('wezterm').has_action('LbtermPark')").eval()?;
        assert!(known, "a config can see this build parks tabs");
        for (what, want) in [
            ("Toggle", LbtermPark::Toggle),
            ("Park", LbtermPark::Park),
            ("Unpark", LbtermPark::Unpark),
        ] {
            let value: mlua::Value = lua
                .load(format!("return require('wezterm').action.LbtermPark '{what}'"))
                .eval()?;
            let action: KeyAssignment = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(action, KeyAssignment::LbtermPark(want));
        }
        assert!(LbtermPark::Toggle.parks(false));
        assert!(!LbtermPark::Toggle.parks(true));
        assert!(LbtermPark::Park.parks(true) && LbtermPark::Park.parks(false));
        assert!(!LbtermPark::Unpark.parks(true) && !LbtermPark::Unpark.parks(false));
        Ok(())
    }

    #[test]
    fn the_tab_hop_action_comes_from_lua() -> anyhow::Result<()> {
        use crate::keyassignment::KeyAssignment;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: bool = lua.load("return require('wezterm').has_action('LbtermTab')").eval()?;
        assert!(known, "a config can see this build hops tabs");
        for (what, want) in [
            ("Back", LbtermTab::Back),
            ("Forward", LbtermTab::Forward),
            ("Waiting", LbtermTab::Waiting),
            ("Last", LbtermTab::Last),
        ] {
            let value: mlua::Value = lua
                .load(format!("return require('wezterm').action.LbtermTab '{what}'"))
                .eval()?;
            let action: KeyAssignment = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(action, KeyAssignment::LbtermTab(want));
        }
        Ok(())
    }

    #[test]
    fn the_new_tab_action_comes_from_lua() -> anyhow::Result<()> {
        use crate::keyassignment::KeyAssignment;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: bool = lua.load("return require('wezterm').has_action('LbtermNewTab')").eval()?;
        assert!(known, "a config can see this build has the new-tab pad");
        for (what, want) in [
            ("Picker", LbtermNewTab::Picker),
            ("Shell", LbtermNewTab::Shell),
            ("Reopen", LbtermNewTab::Reopen),
        ] {
            let value: mlua::Value = lua
                .load(format!("return require('wezterm').action.LbtermNewTab '{what}'"))
                .eval()?;
            let action: KeyAssignment = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(action, KeyAssignment::LbtermNewTab(want));
        }
        Ok(())
    }

    #[test]
    fn the_panel_takes_over_from_the_tabs_unless_asked_not_to() {
        let mut config = crate::Config::default_config();
        assert!(!hides_tabs(&config), "no panel, the tab bar as ever");
        config.lbterm_tab_panel.enabled = true;
        assert!(hides_tabs(&config));
        config.lbterm_tab_panel.tab_bar = true;
        assert!(!hides_tabs(&config));
    }

    #[test]
    fn lbterm_config_comes_before_wezterm_config() {
        let home = Path::new("/home/lb");
        let dirs = |name: &str| vec![home.join(".config").join(name)];
        assert_eq!(
            config_file_candidates(home, Some(Path::new("/opt/lbterm")), dirs),
            paths(&[
                "/opt/lbterm/lbterm.lua",
                "/opt/lbterm/wezterm.lua",
                "/home/lb/.lbterm.lua",
                "/home/lb/.config/lbterm/lbterm.lua",
                "/home/lb/.wezterm.lua",
                "/home/lb/.config/wezterm/wezterm.lua",
            ])
        );
    }

    #[test]
    fn without_an_exe_dir_the_home_config_comes_first() {
        let home = Path::new("/home/lb");
        let got = config_file_candidates(home, None, |_| vec![]);
        assert_eq!(
            got,
            paths(&["/home/lb/.lbterm.lua", "/home/lb/.wezterm.lua"])
        );
    }

    #[test]
    fn config_dirs_list_lbterm_before_wezterm() {
        let dirs = &*crate::CONFIG_DIRS;
        let lbterm = dirs.iter().position(|d| d.ends_with("lbterm"));
        let wezterm = dirs.iter().position(|d| d.ends_with("wezterm"));
        // Positional arguments: this crate is edition 2018, where a lone
        // assert message string isn't formatted.
        assert!(
            matches!((lbterm, wezterm), (Some(l), Some(w)) if l < w),
            "{:?}",
            dirs
        );
    }

    #[test]
    fn runtime_data_and_cache_dirs_are_lbterms() -> anyhow::Result<()> {
        for dir in [
            crate::compute_runtime_dir()?,
            crate::compute_data_dir()?,
            crate::compute_cache_dir()?,
        ] {
            assert!(dir.ends_with(DIR_NAME), "{}", dir.display());
        }
        Ok(())
    }

    #[test]
    fn lua_sees_the_app_name() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let name: String = lua.load("return require('wezterm').app_name").eval()?;
        assert_eq!(name, APP_NAME);
        Ok(())
    }

    #[test]
    fn a_config_can_see_this_build_knows_the_question_colours() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: (bool, bool) = lua
            .load("local w = require('wezterm') return w.has_action('LbtermQuestions'), w.has_action('NoSuchAction')")
            .eval()?;
        assert_eq!(known, (true, false));
        Ok(())
    }

    #[test]
    fn a_config_can_see_this_build_knows_the_side_buttons() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: (bool, bool, bool, bool) = lua
            .load(
                "local w = require('wezterm') return w.has_mouse_button('Back'), \
                 w.has_mouse_button('Forward'), w.has_mouse_button('Right'), \
                 w.has_mouse_button('Sideways')",
            )
            .eval()?;
        assert_eq!(known, (true, true, true, false));
        Ok(())
    }

    #[test]
    fn a_side_button_binding_comes_from_lua() -> anyhow::Result<()> {
        use crate::keyassignment::{KeyAssignment, MouseEventTrigger};
        use wezterm_term::input::MouseButton;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        for (name, want) in [("Back", MouseButton::Back), ("Forward", MouseButton::Forward)] {
            let value: mlua::Value = lua
                .load(format!(
                    "return {{ event = {{ Down = {{ streak = 1, button = '{name}' }} }}, \
                     mods = 'NONE', action = require('wezterm').action.ActivateTabRelative(1) }}"
                ))
                .eval()?;
            let binding: crate::keys::Mouse = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(
                binding.event,
                MouseEventTrigger::Down {
                    streak: 1,
                    button: want
                }
            );
            assert_eq!(binding.action, KeyAssignment::ActivateTabRelative(1));
        }
        Ok(())
    }

    #[test]
    fn the_question_colours_action_comes_from_lua() -> anyhow::Result<()> {
        use crate::keyassignment::KeyAssignment;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        for (mode, want) in [
            ("Show", LbtermQuestions::Show),
            ("Hide", LbtermQuestions::Hide),
            ("Toggle", LbtermQuestions::Toggle),
        ] {
            let value: mlua::Value = lua
                .load(format!("return require('wezterm').action.LbtermQuestions '{mode}'"))
                .eval()?;
            let action: KeyAssignment = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(action, KeyAssignment::LbtermQuestions(want));
        }
        Ok(())
    }

    #[test]
    fn the_conductors_view_action_comes_from_lua() -> anyhow::Result<()> {
        use crate::keyassignment::KeyAssignment;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: bool = lua
            .load("return require('wezterm').has_action('LbtermConductorView')")
            .eval()?;
        assert!(known, "a config can see this build has the view");
        for (mode, want) in [
            ("Show", LbtermConductorView::Show),
            ("Hide", LbtermConductorView::Hide),
            ("Toggle", LbtermConductorView::Toggle),
        ] {
            let value: mlua::Value = lua
                .load(format!("return require('wezterm').action.LbtermConductorView '{mode}'"))
                .eval()?;
            let action: KeyAssignment = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(action, KeyAssignment::LbtermConductorView(want));
        }
        Ok(())
    }

    #[test]
    fn the_voice_draft_action_comes_from_lua() -> anyhow::Result<()> {
        use crate::keyassignment::KeyAssignment;
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let known: bool = lua
            .load("return require('wezterm').has_action('LbtermDraft')")
            .eval()?;
        assert!(known, "a config can see this build has voice drafts");
        for (mode, want) in [
            ("Send", LbtermDraft::Send),
            ("Edit", LbtermDraft::Edit),
            ("Clear", LbtermDraft::Clear),
        ] {
            let value: mlua::Value = lua
                .load(format!("return require('wezterm').action.LbtermDraft '{mode}'"))
                .eval()?;
            let action: KeyAssignment = luahelper::from_lua_value_dynamic(value)?;
            assert_eq!(action, KeyAssignment::LbtermDraft(want));
        }
        Ok(())
    }

    #[test]
    fn the_conductor_comes_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(
                r#"
                local wezterm = require 'wezterm'
                return {
                  state_file = [[C:\conductor\button.json]],
                  hub_pipe = [[\\.\pipe\conductor-hub]],
                  seat_pipe = [[\\.\pipe\conductor-seat]],
                  menu = {
                    { label = 'Accept most', when = { state = { 'on', 'remote' } }, checked = { mode = 'most' },
                      wait = true, action = wezterm.action_callback(function(window, pane) end) },
                    { label = 'Say hello', action = wezterm.action.EmitEvent 'hello' },
                  },
                }
                "#,
            )
            .eval()?;
        let conductor: LbtermConductor = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(conductor.state_file, r"C:\conductor\button.json");
        assert_eq!(conductor.hub_pipe.as_deref(), Some(r"\\.\pipe\conductor-hub"));
        assert_eq!(conductor.seat_pipe.as_deref(), Some(r"\\.\pipe\conductor-seat"));
        assert_eq!(conductor.glyph, "\u{266b}", "a note unless the config names a glyph");
        assert_eq!(conductor.menu.len(), 2);
        assert_eq!(conductor.menu[0].when["state"], vec!["on", "remote"]);
        assert_eq!(conductor.menu[0].checked["mode"], "most");
        // An action_callback is an event of its own, which the GUI emits
        assert!(matches!(
            *conductor.menu[0].action,
            crate::keyassignment::KeyAssignment::EmitEvent(_)
        ));
        assert!(conductor.menu[1].when.is_empty() && conductor.menu[1].checked.is_empty());
        // Only the item that says so waits for its answer
        assert!(conductor.menu[0].wait && !conductor.menu[1].wait);
        Ok(())
    }

    #[test]
    fn no_conductor_unless_asked_for() {
        assert!(crate::Config::default_config().lbterm_conductor.is_none());
    }

    #[test]
    fn the_inbox_comes_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(r#"return { lbterm_inbox = { folders = { [[C:\Work\One\InMail]], [[C:\Work\Two\InMail]] } } }"#)
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let inbox = config.lbterm_inbox.expect("the inbox, as asked for");
        assert_eq!(inbox.folders, vec![r"C:\Work\One\InMail", r"C:\Work\Two\InMail"]);
        assert_eq!(inbox.glyph, "\u{f01f0}", "an envelope unless the config names a glyph");
        assert!(crate::Config::default_config().lbterm_inbox.is_none(), "none unless asked for");
        Ok(())
    }

    #[test]
    fn the_folder_of_requests_to_let_go_comes_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(r#"return { lbterm_let_go = { folder = [[C:\Work\State\let-go]] } }"#)
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(config.lbterm_let_go.expect("as asked for").folder, r"C:\Work\State\let-go");
        assert!(crate::Config::default_config().lbterm_let_go.is_none(), "nothing let go of unless asked for");
        Ok(())
    }

    #[test]
    fn the_context_pill_comes_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua.load(r#"return { lbterm_context = {} }"#).eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(config.lbterm_context.expect("the pill, as asked for").transcripts, None);
        let value: mlua::Value = lua
            .load(r#"return { lbterm_context = { transcripts = [[C:\Work\transcripts]] } }"#)
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(config.lbterm_context.unwrap().transcripts.as_deref(), Some(r"C:\Work\transcripts"));
        assert!(crate::Config::default_config().lbterm_context.is_none(), "none unless asked for");
        Ok(())
    }

    #[test]
    fn the_context_bar_comes_from_lua_and_is_off_unless_asked_for() -> anyhow::Result<()> {
        assert!(crate::Config::default_config().lbterm_context_bar.is_none());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua.load(r#"return { lbterm_context_bar = {} }"#).eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let bar = config.lbterm_context_bar.expect("the bar, as asked for");
        assert_eq!(bar, LbtermContextBar::default());
        let px = |d: crate::Dimension| {
            d.evaluate_as_pixels(crate::DimensionContext { dpi: 96., pixel_max: 1000., pixel_cell: 10. })
        };
        assert_eq!((px(bar.width), px(bar.pane_width)), (14., 400.));
        assert_eq!((bar.widen_delay_ms, bar.open_delay_ms, bar.close_delay_ms), (150, 300, 250));
        let value: mlua::Value = lua.load(r#"return { lbterm_context_bar = { width = '12pt', open_delay_ms = 400 } }"#).eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let bar = config.lbterm_context_bar.unwrap();
        assert_eq!((px(bar.width), bar.open_delay_ms, bar.close_delay_ms), (16., 400, 250));
        Ok(())
    }

    #[test]
    fn the_cost_is_counted_only_when_asked_for() -> anyhow::Result<()> {
        assert!(!counts_cost(&crate::Config::default_config()), "no pill, no count");
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let counts = |text: &str| -> anyhow::Result<bool> {
            let value: mlua::Value = lua.load(text).eval()?;
            let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
            Ok(counts_cost(&config))
        };
        assert!(!counts(r#"return { lbterm_context = {} }"#)?, "the pill alone doesn't count");
        assert!(!counts(r#"return { lbterm_context = { count_cost = false } }"#)?);
        assert!(counts(r#"return { lbterm_context = { count_cost = true } }"#)?);
        // The pane hangs from the pill, so the bar is neither needed nor where it's asked for: its
        // old place counts nothing (and warns of a field the bar doesn't have)
        assert!(counts(r#"return { lbterm_context = { count_cost = true }, lbterm_context_bar = {} }"#)?);
        assert!(!counts(r#"return { lbterm_context = {}, lbterm_context_bar = { count_cost = true } }"#)?);
        Ok(())
    }

    #[test]
    fn the_agenda_comes_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(r#"return { lbterm_agenda = { file = [[C:\Work\State\agenda\awake.json]] } }"#)
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let agenda = config.lbterm_agenda.expect("the agenda, as asked for");
        assert_eq!(agenda.file, r"C:\Work\State\agenda\awake.json");
        assert_eq!(agenda.glyph, "\u{f076e}", "a checklist unless the config names a glyph");
        assert!(crate::Config::default_config().lbterm_agenda.is_none(), "none unless asked for");
        Ok(())
    }

    #[test]
    fn tab_links_come_from_lua_and_are_off_unless_asked_for() -> anyhow::Result<()> {
        assert!(crate::Config::default_config().lbterm_links.is_none());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua.load(r#"return { lbterm_links = { file = [[C:\State\tab-links.json]] } }"#).eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(config.lbterm_links, Some(LbtermLinks { file: r"C:\State\tab-links.json".into() }));
        Ok(())
    }

    #[test]
    fn the_objectives_panel_comes_from_lua_and_is_off_unless_asked_for() -> anyhow::Result<()> {
        assert!(crate::Config::default_config().lbterm_objectives.is_none());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua.load(r#"return { lbterm_objectives = { open_delay_ms = 250 } }"#).eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let objectives = config.lbterm_objectives.expect("the panel, as asked for");
        assert_eq!(objectives.open_delay_ms, 250);
        assert_eq!(objectives.close_delay_ms, 200);
        let px = |d: crate::Dimension| {
            d.evaluate_as_pixels(crate::DimensionContext { dpi: 96., pixel_max: 1000., pixel_cell: 10. })
        };
        assert_eq!((px(objectives.width), px(objectives.card_width)), (14., 360.));
        assert_eq!(LbtermObjectives { open_delay_ms: 250, ..Default::default() }, objectives);
        Ok(())
    }

    #[test]
    fn the_scroll_strip_comes_from_lua_and_is_off_unless_asked_for() -> anyhow::Result<()> {
        assert!(crate::Config::default_config().lbterm_scroll_strip.is_none());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(r#"return { lbterm_scroll_strip = { transcripts = [[C:\Users\someone\.claude\projects]] } }"#)
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let strip = config.lbterm_scroll_strip.expect("the strip, as asked for");
        assert_eq!(strip.transcripts.as_deref(), Some(r"C:\Users\someone\.claude\projects"));
        assert_eq!((strip.open_delay_ms, strip.close_delay_ms), (150, 200));
        let px = |d: crate::Dimension| {
            d.evaluate_as_pixels(crate::DimensionContext { dpi: 96., pixel_max: 1000., pixel_cell: 10. })
        };
        assert_eq!((px(strip.width), px(strip.card_width)), (14., 360.));
        assert_eq!(LbtermScrollStrip { transcripts: strip.transcripts.clone(), ..Default::default() }, strip);
        Ok(())
    }

    #[test]
    fn the_projects_card_comes_from_lua() -> anyhow::Result<()> {
        assert!(crate::Config::default_config().lbterm_projects.is_none());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(
                r#"return { lbterm_projects = { projects = {
                    { name = 'Terminal', dir = [[C:\Work\Terminal]] },
                    { name = 'Shell', dir = [[C:\Work\Terminal\code\Shell]], parent = 'Terminal' },
                } } }"#,
            )
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let projects = config.lbterm_projects.expect("the card, as asked for");
        assert_eq!(
            projects.projects,
            vec![
                LbtermProject { name: "Terminal".into(), dir: r"C:\Work\Terminal".into(), parent: None, aliases: vec![] },
                LbtermProject { name: "Shell".into(), dir: r"C:\Work\Terminal\code\Shell".into(), parent: Some("Terminal".into()), aliases: vec![] },
            ],
            "a repo names the project it's in"
        );
        let px = projects.card_width.evaluate_as_pixels(crate::DimensionContext { dpi: 96., pixel_max: 1000., pixel_cell: 10. });
        assert_eq!(px, 480.);
        // Its own rest and grace, the objectives strip's until the pad left it
        assert_eq!((projects.open_delay_ms, projects.close_delay_ms), (300, 200));
        let value: mlua::Value = lua
            .load(r#"return { lbterm_projects = { projects = {}, open_delay_ms = 0, close_delay_ms = 50 } }"#)
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let projects = config.lbterm_projects.expect("the card, as asked for");
        assert_eq!((projects.open_delay_ms, projects.close_delay_ms), (0, 50));
        Ok(())
    }

    #[test]
    fn the_budgets_are_on_with_their_defaults_and_each_can_be_set() -> anyhow::Result<()> {
        let budget = crate::Config::default_config().lbterm_budget;
        assert_eq!(budget, LbtermBudget::default());
        assert!(budget.enabled);
        assert_eq!((budget.quiet_ms, budget.rest_frames, budget.rest_timers, budget.rest_tasks), (1000, 5, 10, 20));
        assert_eq!(budget.rest_news_frames, 30);
        let names: Vec<&str> = budget.actions.by_name().iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ["key", "tab", "new-tab", "close-tab", "split", "rail", "inbox", "agenda", "conductor-view", "picker", "pick", "reload", "paint"],
            "lbterm-extras's Action::ALL, in its order"
        );
        assert!(budget.actions.by_name().iter().all(|(_, ms)| *ms > 0.0), "every action timed");

        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load("return { lbterm_budget = { rest_frames = 2, actions = { tab = 12.5, new_tab = 0 } } }")
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        let budget = config.lbterm_budget;
        assert_eq!(budget.rest_frames, 2);
        assert_eq!((budget.actions.tab, budget.actions.new_tab), (12.5, 0.0));
        assert_eq!(budget.actions.key, default_action_key(), "the rest keep their defaults");
        assert_eq!(budget.quiet_ms, 1000);

        let value: mlua::Value = lua.load("return { lbterm_budget = { actions = { tab = -1 } } }").eval()?;
        assert!(luahelper::from_lua_value_dynamic::<crate::Config>(value).is_err(), "no time is negative");
        Ok(())
    }

    #[test]
    fn a_reload_says_when_it_started() {
        let before = reload_started().map_or(0, |(n, _)| n);
        let t = std::time::Instant::now();
        note_reload_start();
        let (n, at) = reload_started().expect("one");
        assert!(n > before && at >= t);
    }

    #[test]
    fn the_slow_frame_log_is_on_unless_turned_off() -> anyhow::Result<()> {
        assert_eq!(crate::Config::default_config().lbterm_slow_ms, default_slow_ms());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua.load("return { lbterm_slow_ms = 0 }").eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(config.lbterm_slow_ms, 0);
        Ok(())
    }

    #[test]
    fn the_line_caches_hold_4096_unless_the_config_says() -> anyhow::Result<()> {
        let config = crate::Config::default_config();
        let sizes = |c: &crate::Config| {
            [
                c.shape_cache_size,
                c.line_state_cache_size,
                c.line_quad_cache_size,
                c.line_to_ele_shape_cache_size,
            ]
        };
        assert_eq!(sizes(&config), [default_line_cache_size(); 4]);
        assert_eq!(default_line_cache_size(), 4096);
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(
                "return { shape_cache_size = 1024, line_state_cache_size = 1024, \
                 line_quad_cache_size = 1024, line_to_ele_shape_cache_size = 1024 }",
            )
            .eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(sizes(&config), [1024; 4]);
        Ok(())
    }

    #[test]
    fn exe_file_name_adds_exe_on_windows() {
        let want = if cfg!(windows) {
            "lbterm-gui.exe"
        } else {
            "lbterm-gui"
        };
        assert_eq!(exe_file_name(GUI_EXE), want);
    }

    /// has_event_handler: what update_title asks before it builds format-tab-title's and
    /// format-window-title's arguments
    #[test]
    fn has_event_handler_says_whether_wezterm_on_named_one() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        assert!(!crate::lua::has_event_handler(&lua, "format-window-title"));
        lua.load("local wezterm = require 'wezterm'; wezterm.on('format-tab-title', function() return 'x' end)")
            .exec()?;
        assert!(crate::lua::has_event_handler(&lua, "format-tab-title"));
        assert!(!crate::lua::has_event_handler(&lua, "format-window-title"));
        Ok(())
    }

    fn cell(column: usize) -> wezterm_term::ClickPosition {
        wezterm_term::ClickPosition {
            column,
            row: 5,
            x_pixel_offset: 0,
            y_pixel_offset: 0,
        }
    }

    /// The streaks quick presses of `button` on one cell count to, with `bound` the bindings
    fn streaks(
        presses: usize,
        button: wezterm_term::input::MouseButton,
        bound: &[crate::keyassignment::MouseEventTrigger],
    ) -> Vec<usize> {
        let mut last = None;
        (0..presses)
            .map(|_| {
                let click = count_click(last.take(), button, cell(3), bound.iter());
                let streak = click.streak;
                last = Some(click);
                streak
            })
            .collect()
    }

    /// Terminal's lbterm.lua's right-button bindings: a press and a release for each of streaks
    /// 1 to 3 (its loop over the hold's Show and Hide, the paste and the Nop)
    fn right_hold() -> Vec<crate::keyassignment::MouseEventTrigger> {
        use crate::keyassignment::MouseEventTrigger;
        use wezterm_term::input::MouseButton;
        (1..=3)
            .flat_map(|streak| {
                [
                    MouseEventTrigger::Down {
                        streak,
                        button: MouseButton::Right,
                    },
                    MouseEventTrigger::Up {
                        streak,
                        button: MouseButton::Right,
                    },
                ]
            })
            .collect()
    }

    /// FLOWS.md's "A right hold, to the question colours", What could go 3: a fourth quick
    /// right press found no binding (upstream counts it streak 4), so it went to Claude Code,
    /// which pasted, and the colours didn't show. Every quick press now finds the hold's
    #[test]
    fn every_quick_press_finds_a_binding_made_for_streaks_1_to_3() {
        use crate::keyassignment::MouseEventTrigger;
        use wezterm_term::input::MouseButton;
        let bound = right_hold();
        let counted = streaks(6, MouseButton::Right, &bound);
        for (press, streak) in counted.iter().enumerate() {
            let down = MouseEventTrigger::Down {
                streak: *streak,
                button: MouseButton::Right,
            };
            assert!(bound.contains(&down), "press {} counted streak {streak}, which nothing binds", press + 1);
        }
        assert_eq!(counted, [1, 2, 3, 3, 3, 3]);
    }

    /// A binding of a longer streak than a triple click still gets its streak, and is where
    /// that button's streak stops; another button, or another cell, starts a streak at 1
    #[test]
    fn a_streak_counts_as_far_as_a_binding_names() {
        use crate::keyassignment::MouseEventTrigger;
        use wezterm_term::input::MouseButton;
        let mut bound = right_hold();
        bound.push(MouseEventTrigger::Down {
            streak: 5,
            button: MouseButton::Left,
        });
        assert_eq!(streaks(7, MouseButton::Left, &bound), [1, 2, 3, 4, 5, 5, 5]);
        // A button nothing binds counts to a triple click: the tab bar's double click is read
        // from the streak whatever's bound
        assert_eq!(streaks(5, MouseButton::Middle, &[]), [1, 2, 3, 3, 3]);

        let first = count_click(None, MouseButton::Right, cell(3), bound.iter());
        let other = count_click(Some(first.clone()), MouseButton::Left, cell(3), bound.iter());
        assert_eq!(other.streak, 1);
        let moved = count_click(Some(first), MouseButton::Right, cell(4), bound.iter());
        assert_eq!(moved.streak, 1);
    }

    /// lbterm/QUESTIONS.md, "The weights": red at 0, amber at 50, green at 100, and between them
    /// each step of 10 another colour, the red falling and the green rising
    #[test]
    fn a_share_runs_from_red_at_0_through_amber_at_50_to_green_at_100() {
        assert_eq!(weight_colour(0), WEIGHT_RED);
        assert_eq!(weight_colour(50), WEIGHT_AMBER);
        assert_eq!(weight_colour(70), 0xD1D734);
        assert_eq!(weight_colour(100), WEIGHT_GREEN);
        assert_eq!(weight_colour(255), WEIGHT_GREEN);
        let steps: Vec<u32> = (0..=10).map(|s| weight_colour(s * 10)).collect();
        for pair in steps.windows(2) {
            let red = |c: u32| c >> 16;
            let green = |c: u32| (c >> 8) & 0xFF;
            assert_ne!(pair[0], pair[1]);
            assert!(red(pair[1]) <= red(pair[0]), "{:06X} then {:06X}", pair[0], pair[1]);
            assert!(green(pair[1]) >= green(pair[0]), "{:06X} then {:06X}", pair[0], pair[1]);
        }
    }
}
