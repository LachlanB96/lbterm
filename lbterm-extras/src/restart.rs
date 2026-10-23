//! Restarting into the installed build. `lbterm.ps1 install` writes the build's version beside
//! the exes (version.txt); a running LBTerm whose own version is another one shows a "Restart to
//! update" pill on its window strip. A click on it, or Ctrl+Shift+F5 (`LbtermRestart`), starts
//! the installed GUI and ends this one; the tabs live on in lbterm-mux-server and the new GUI
//! attaches to them (lbterm/MUX.md). The GUI side is wezterm-gui/src/restart.rs.

/// The file beside the exes that names the installed build, as `wezterm.version` says it
pub const VERSION_FILE: &str = "version.txt";

/// What the pill says
pub const PILL_TEXT: &str = "\u{21bb} Restart to update";
/// ...and once it's been clicked, until the window goes
pub const PILL_RESTARTING: &str = "\u{21bb} Restarting\u{2026}";
/// What the strip says meanwhile, in the caption's place
pub const RESTARTING_NOTICE: &str = "Restarting\u{2026}";
/// How long a restarting window stays to show it, in milliseconds
pub const RESTART_SHOWN_MS: u64 = 150;

/// The version a restarting GUI hands the one it starts, so the new one can say what it came
/// back as (lbterm_extras::strip::arrival_notice)
pub const RESTARTED_FROM_VAR: &str = "LBTERM_RESTARTED_FROM";

/// The pid a restarting GUI hands the one it starts. The new GUI waits for that process to end
/// before anything else, so it can't hand its window to the GUI that's leaving.
pub const PREDECESSOR_VAR: &str = "LBTERM_RESTART_AFTER";

/// How long the new GUI waits for the old one, in milliseconds
pub const PREDECESSOR_WAIT_MS: u32 = 10_000;

/// How often a running GUI reads version.txt, in seconds
pub const POLL_SECS: u64 = 10;

/// Whether version.txt's contents name another build than the running one. A missing or empty
/// file names none: a GUI run from target\release or a try window has no pill.
pub fn other_build_installed(installed: Option<&str>, running: &str) -> bool {
    match installed.map(str::trim) {
        Some(version) if !version.is_empty() => version != running.trim(),
        _ => false,
    }
}

/// The pid in PREDECESSOR_VAR's value, if it holds one
pub fn predecessor(value: Option<&str>) -> Option<u32> {
    value?.trim().parse().ok().filter(|&pid| pid != 0)
}

/// Why a restart can't go ahead, when it can't. Panes the GUI holds itself, outside a mux
/// server, would end with it.
pub fn refusal(local_panes: usize) -> Option<String> {
    match local_panes {
        0 => None,
        1 => Some(
            "A pane runs in this window rather than in the mux server, and would end with it. \
             Close and open LBTerm instead."
                .to_string(),
        ),
        n => Some(format!(
            "{n} panes run in this window rather than in the mux server, and would end with it. \
             Close and open LBTerm instead."
        )),
    }
}

/// The strip's colours for the pill, as sRGB 0..1, from the strip's own colour: white laid over
/// it at the design's alphas (lbterm/DESIGN.md: a hairline at rest is white 14%, under the
/// pointer 35%; the words sit back at 75% until the pointer comes). Mixed in sRGB, as the eye
/// sees it. The fill stays the strip's: the rounded ends are drawn as arcs whose insides are
/// the strip's, and a fill of its own would show at the corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PillColors {
    pub text: [f32; 3],
    pub outline: [f32; 3],
    pub fill: [f32; 3],
}

pub fn pill_colors(surface: [f32; 3], hover: bool) -> PillColors {
    let over = |alpha: f32| surface.map(|c| c + (1. - c) * alpha);
    let (text, outline) = if hover { (1., 0.35) } else { (0.75, 0.14) };
    PillColors {
        text: over(text),
        outline: over(outline),
        fill: surface,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn another_build_installed_shows_the_pill() {
        assert!(other_build_installed(
            Some("20261021-090000-aaaa1111"),
            "20261020-194410-04e8e722"
        ));
    }

    #[test]
    fn the_same_build_shows_none() {
        assert!(!other_build_installed(
            Some("20261020-194410-04e8e722\r\n"),
            "20261020-194410-04e8e722"
        ));
    }

    #[test]
    fn an_older_build_counts_too() {
        // After `lbterm.ps1 rollback` while this GUI kept running
        assert!(other_build_installed(
            Some("20261019-120000-0000aaaa"),
            "20261020-194410-04e8e722"
        ));
    }

    #[test]
    fn no_file_or_an_empty_one_shows_none() {
        assert!(!other_build_installed(None, "20261020-194410-04e8e722"));
        assert!(!other_build_installed(Some("  \n"), "20261020-194410-04e8e722"));
    }

    #[test]
    fn the_predecessor_is_a_pid() {
        assert_eq!(predecessor(Some("4242")), Some(4242));
        assert_eq!(predecessor(Some(" 4242\n")), Some(4242));
        assert_eq!(predecessor(Some("0")), None);
        assert_eq!(predecessor(Some("x")), None);
        assert_eq!(predecessor(None), None);
    }

    #[test]
    fn panes_outside_the_server_refuse() {
        assert_eq!(refusal(0), None);
        assert!(refusal(1).unwrap().starts_with("A pane runs"));
        assert!(refusal(3).unwrap().starts_with("3 panes run"));
    }

    #[test]
    fn the_pill_is_white_over_the_strip() {
        let surface = [18. / 255.; 3];
        let rest = pill_colors(surface, false);
        assert_eq!(rest.fill, surface);
        // White 14% over #121212 is about #333333
        assert!((rest.outline[0] * 255. - 51.2).abs() < 0.5, "{:?}", rest.outline);
        let hover = pill_colors(surface, true);
        assert_eq!(hover.text, [1.; 3]);
        assert!(hover.outline[0] > rest.outline[0] && hover.text[0] > rest.text[0]);
        // The rounded ends' insides are the strip's, so the fill never leaves it
        assert_eq!(hover.fill, surface);
    }
}
