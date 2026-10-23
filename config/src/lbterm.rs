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
}

impl Default for LbtermMotion {
    fn default() -> Self {
        Self {
            enabled: default_motion_enabled(),
            slowdown: default_motion_slowdown(),
            pane_layout: default_pane_layout(),
            pane_enter: default_pane_enter(),
            pane_exit: default_pane_exit(),
            panel_open: default_panel_open(),
            panel_close: default_panel_close(),
            panel_glow: default_panel_glow(),
            panel_sweep: default_panel_sweep(),
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
        }
    }
}

/// Whether the tab bar leaves its tabs to the tab panel: the panel on, and
/// `lbterm_tab_panel.tab_bar` not asked for.
pub fn hides_tabs(config: &crate::Config) -> bool {
    config.lbterm_tab_panel.enabled && !config.lbterm_tab_panel.tab_bar
}

/// 68 px at 96 DPI: the ring, and a pill round each pad clear of its corners
fn default_panel_rail() -> crate::Dimension {
    crate::Dimension::Points(51.)
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

/// `config.lbterm_slow_ms`'s default: what holds the GUI thread longer than this is logged.
/// Two refreshes of a 60 Hz screen, so a hold this long skips a frame of typing even there.
/// Not 8: a present waits on DWM for up to a refresh, so on a 60 Hz screen a third of
/// ordinary typing frames' presents took over 8 ms (lbterm\perf\slow.ps1 -Measure, 2026-10-22)
pub fn default_slow_ms() -> u64 {
    33
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
            &m.panel_open,
            &m.panel_close,
            &m.panel_glow,
            &m.panel_sweep,
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
        assert_eq!(px(config.lbterm_tab_panel.width), 290.);
        assert!(validate_idle_full_minutes(&0.0).is_err());
        // Labels under the pads, two lines of 7pt
        let panel = &config.lbterm_tab_panel;
        assert!(panel.labels);
        assert_eq!((panel.label_lines, panel.label_font_size), (2, 7.));
        assert!(validate_label_lines(&0).is_err() && validate_label_lines(&4).is_err());
        assert!(validate_label_font_size(&2.0).is_err());
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
    fn the_conductor_comes_from_lua() -> anyhow::Result<()> {
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua
            .load(
                r#"
                local wezterm = require 'wezterm'
                return {
                  state_file = [[C:\conductor\button.json]],
                  hub_pipe = [[\\.\pipe\conductor-hub]],
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
    fn the_slow_frame_log_is_on_unless_turned_off() -> anyhow::Result<()> {
        assert_eq!(crate::Config::default_config().lbterm_slow_ms, default_slow_ms());
        let lua = crate::lua::make_lua_context(Path::new("testing"))?;
        let value: mlua::Value = lua.load("return { lbterm_slow_ms = 0 }").eval()?;
        let config: crate::Config = luahelper::from_lua_value_dynamic(value)?;
        assert_eq!(config.lbterm_slow_ms, 0);
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
}
