//! LBTerm: the "Restart to update" pill and Ctrl+Shift+F5 (`LbtermRestart`), whose logic is
//! lbterm_extras::restart.
//!
//! A thread reads version.txt beside the running exe every 10 s. While it names another build
//! than this one, the window strip shows the pill (fancy_tab_bar.rs). Restarting starts the
//! installed lbterm-gui.exe with this process's arguments and its pid in LBTERM_RESTART_AFTER,
//! then ends this process the way `lbterm.ps1 restart` does, as a crash would: the tabs live on
//! in lbterm-mux-server and the new GUI attaches to them (lbterm/MUX.md). The new GUI waits for
//! this one to be gone before it does anything else (wait_for_predecessor), so it can't hand its
//! window to the GUI that's leaving.

use crate::termwindow::TermWindowNotif;
use anyhow::Context;
use lbterm_extras::restart as logic;
use mux::Mux;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wezterm_dynamic::Value;
use window::WindowOps;

static OTHER_BUILD: AtomicBool = AtomicBool::new(false);
/// A restart is under way: the pill and the strip say so until the window goes
static RESTARTING: AtomicBool = AtomicBool::new(false);
/// The version of the GUI whose restart started this one, if one did
static RESTARTED_FROM: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// Whether the installed build is another one than this: the pill shows while it is
pub fn other_build_installed() -> bool {
    OTHER_BUILD.load(Ordering::Relaxed)
}

/// Whether a restart is under way: the pill says so
pub fn restarting() -> bool {
    RESTARTING.load(Ordering::Relaxed)
}

/// The version of the GUI whose restart started this one, when one did
pub fn restarted_from() -> Option<String> {
    RESTARTED_FROM.get().cloned().flatten()
}

/// The folder this GUI runs from: the install, target\release, a try window's copy
fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

/// The first thing a GUI does: when a restarting GUI started it, wait (up to 10 s) for that one
/// to be gone. The variable goes too, so nothing this GUI starts inherits it.
pub fn wait_for_predecessor() {
    // LBTerm: what the GUI before was, for the strip to say what this one came back as; the
    // variable goes too, so nothing this GUI starts inherits it
    let from = std::env::var(logic::RESTARTED_FROM_VAR).ok();
    std::env::remove_var(logic::RESTARTED_FROM_VAR);
    let _ = RESTARTED_FROM.set(from);
    let value = match std::env::var(logic::PREDECESSOR_VAR) {
        Ok(value) => value,
        Err(_) => return,
    };
    std::env::remove_var(logic::PREDECESSOR_VAR);
    let pid = match logic::predecessor(Some(&value)) {
        Some(pid) => pid,
        None => return,
    };
    #[cfg(windows)]
    unsafe {
        use windows58::Win32::Foundation::CloseHandle;
        use windows58::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(process, logic::PREDECESSOR_WAIT_MS);
            let _ = CloseHandle(process);
        }
    }
}

/// Starts reading version.txt, when this GUI runs from a folder that has one: an install made
/// by lbterm.ps1, not target\release or a try window
pub fn watch() {
    let file = match exe_dir() {
        Some(dir) => dir.join(logic::VERSION_FILE),
        None => return,
    };
    if !file.exists() {
        return;
    }
    let running = config::wezterm_version().to_string();
    let spawned = std::thread::Builder::new()
        .name("lbterm-version".into())
        .spawn(move || loop {
            let installed = std::fs::read_to_string(&file).ok();
            let other = logic::other_build_installed(installed.as_deref(), &running);
            if OTHER_BUILD.swap(other, Ordering::Relaxed) != other {
                log::info!(
                    "{} names {:?}, this is {running}: the restart pill {}",
                    file.display(),
                    installed.as_deref().map(str::trim),
                    if other { "shows" } else { "goes" }
                );
                promise::spawn::spawn_into_main_thread(async move { redraw_strips() }).detach();
            }
            std::thread::sleep(Duration::from_secs(logic::POLL_SECS));
        });
    if let Err(err) = spawned {
        log::error!("starting the lbterm-version thread: {err:#}");
    }
}

/// Rebuilds every window's strip, to show or take away the pill
fn redraw_strips() {
    let fe = match crate::frontend::try_front_end() {
        Some(fe) => fe,
        None => return,
    };
    for win in fe.gui_windows() {
        win.window.notify(TermWindowNotif::Apply(Box::new(|tw| {
            tw.invalidate_fancy_tab_bar();
            if let Some(window) = tw.window.as_ref() {
                window.invalidate();
            }
        })));
    }
}

/// The pill, for the strip's right end, left of the window buttons: a hairline round the words,
/// white over the strip's own colour, brighter under the pointer (lbterm/DESIGN.md, "The
/// window strip"). Shorter than the buttons, so the strip keeps its height and the panes'
/// cards don't move.
pub fn pill_element(
    font: &std::rc::Rc<wezterm_font::LoadedFont>,
    metrics: &crate::utilsprites::RenderMetrics,
    surface: config::SrgbaTuple,
) -> crate::termwindow::box_model::Element {
    use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
    use crate::tabbar::TabBarItem;
    use crate::termwindow::box_model::*;
    use crate::termwindow::UIItemType;
    use config::Dimension;

    // The box model fills its corner shapes with the border colour, so the hairline's rounded
    // ends are arcs, drawn as outlines (corners.rs's ovals, stroked); their insides are left to
    // the strip, which is why the pill's fill never leaves the strip's colour
    macro_rules! arc {
        ($x:ident, $y:ident) => {
            &[Poly {
                path: &[PolyCommand::Oval {
                    center: (BlockCoord::$x, BlockCoord::$y),
                    radiuses: (BlockCoord::One, BlockCoord::One),
                }],
                intensity: BlockAlpha::Full,
                style: PolyStyle::Outline,
            }]
        };
    }
    const TOP_LEFT: &[Poly] = arc!(One, One);
    const TOP_RIGHT: &[Poly] = arc!(Zero, One);
    const BOTTOM_LEFT: &[Poly] = arc!(One, Zero);
    const BOTTOM_RIGHT: &[Poly] = arc!(Zero, Zero);

    let colors = |hover| {
        let c = logic::pill_colors([surface.0, surface.1, surface.2], hover);
        let linear = |rgb: [f32; 3]| config::SrgbaTuple(rgb[0], rgb[1], rgb[2], 1.).to_linear();
        ElementColors {
            border: BorderColor::new(linear(c.outline)),
            bg: linear(c.fill).into(),
            text: linear(c.text).into(),
        }
    };
    // Rounded ends: corners half the pill's height (a line of text, its padding and the hairlines)
    let radius = Dimension::Pixels((metrics.cell_size.height as f32 * 0.6).round());
    let corner = |poly| SizedPoly {
        width: radius,
        height: radius,
        poly,
    };
    let text = if restarting() {
        logic::PILL_RESTARTING
    } else {
        logic::PILL_TEXT
    };
    Element::new(font, ElementContent::Text(text.to_string()))
        .item_type(UIItemType::TabBar(TabBarItem::LbtermRestart))
        .vertical_align(VerticalAlign::Middle)
        .zindex(1)
        .padding(BoxDimension {
            left: Dimension::Cells(0.75),
            right: Dimension::Cells(0.75),
            top: Dimension::Cells(0.1),
            bottom: Dimension::Cells(0.1),
        })
        .margin(BoxDimension {
            left: Dimension::Cells(0.),
            right: Dimension::Cells(0.5),
            top: Dimension::Cells(0.),
            bottom: Dimension::Cells(0.),
        })
        .border(BoxDimension::new(Dimension::Pixels(1.)))
        .border_corners(Some(Corners {
            top_left: corner(TOP_LEFT),
            top_right: corner(TOP_RIGHT),
            bottom_left: corner(BOTTOM_LEFT),
            bottom_right: corner(BOTTOM_RIGHT),
        }))
        .colors(colors(false))
        .hover_colors(Some(colors(true)))
}

/// Restarts into the installed build: Ctrl+Shift+F5, and a click on the pill. Says why in a
/// toast when it can't.
pub fn restart() {
    if let Err(err) = try_restart() {
        log::error!("restart: {err:#}");
        wezterm_toast_notification::persistent_toast_notification(
            "LBTerm didn't restart",
            &format!("{err:#}"),
        );
    }
}

fn try_restart() -> anyhow::Result<()> {
    let local = Mux::get()
        .iter_panes()
        .iter()
        .filter(|pane| !in_mux_server(pane.get_metadata()))
        .count();
    if let Some(why) = logic::refusal(local) {
        anyhow::bail!(why);
    }
    let dir = exe_dir().context("finding the folder LBTerm runs from")?;
    let gui = dir.join(format!("{}.exe", config::lbterm::GUI_EXE));
    std::process::Command::new(&gui)
        .args(std::env::args_os().skip(1))
        .env(logic::PREDECESSOR_VAR, std::process::id().to_string())
        .env(logic::RESTARTED_FROM_VAR, config::wezterm_version())
        .spawn()
        .with_context(|| format!("starting {}", gui.display()))?;
    // The note crash restore keeps: with every pane in the server it still offers them, should
    // the server die before the new GUI attaches (restore.rs)
    crate::restore::clean_exit();
    // LBTerm: the pill and the strip say it's restarting, and the window stays a moment to
    // show it; the new GUI waits for this one to be gone before it does anything
    RESTARTING.store(true, Ordering::Relaxed);
    if let Some(fe) = crate::frontend::try_front_end() {
        for win in fe.gui_windows() {
            win.window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                tw.lbterm_strip_notice(logic::RESTARTING_NOTICE);
                tw.invalidate_fancy_tab_bar();
            })));
        }
    }
    promise::spawn::spawn(async {
        smol::Timer::after(Duration::from_millis(logic::RESTART_SHOWN_MS)).await;
        end_this_process()
    })
    .detach();
    Ok(())
}

/// A ClientPane says where it lives in the server; a pane the GUI holds itself doesn't
fn in_mux_server(metadata: Value) -> bool {
    match metadata {
        Value::Object(obj) => obj.get_by_str("remote_pane_id").is_some(),
        _ => false,
    }
}

/// Ends the process as `lbterm.ps1 restart`'s Stop-Process does, without closing its windows,
/// which would end their tabs in the server (lbterm/MUX.md), and without DLL teardown, which
/// can wait on the GPU driver's threads
fn end_this_process() -> ! {
    #[cfg(windows)]
    unsafe {
        use windows58::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
    std::process::exit(0)
}
