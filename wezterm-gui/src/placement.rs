//! LBTerm: each window opens where it was (lbterm_extras::placement). A restart, `lbterm.ps1
//! restart` or a crash used to bring the window back where Windows puts new ones, sized for its
//! tabs: on a smaller screen it was cut down and every tab's program redrew at the new size, and
//! again when it was put back (Lachlan, 2026-10-25).
//!
//! The places are kept for Lachlan's own LBTerm, the window class LBTerm has by default, in
//! `placement` in the data folder (`~\.local\share\lbterm`). Try and test windows keep none
//! unless `LBTERM_PLACEMENT_DIR` names a folder for them: they open on the right-most screen by
//! themselves, and a place one left would move the next.

use lbterm_extras::placement::{self as logic, Placement};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this GUI has opened a window yet: its first takes the place saved last when it has
/// none of its own
static OPENED: AtomicBool = AtomicBool::new(false);

/// The folder the places are kept in, if this GUI keeps them
fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(logic::DIR_VAR) {
        return Some(dir.into());
    }
    let class = crate::termwindow::get_window_class();
    let own = class == wezterm_gui_subcommands::DEFAULT_WINDOW_CLASS
        && std::env::var_os("LBTERM_TRY").is_none();
    own.then(|| config::DATA_DIR.join("placement"))
}

/// The id a window's place is kept by: the one its mux server knows it by, which stays the same
/// across a restart of the GUI; its own outside a mux domain
fn key(window: MuxWindowId) -> u64 {
    let mux = Mux::get();
    for domain in mux.iter_domains() {
        if let Some(client) = domain.downcast_ref::<wezterm_client::domain::ClientDomain>() {
            if let Some(remote) = client.local_to_remote_window_id(window) {
                return remote as u64;
            }
        }
    }
    window as u64
}

fn read(path: &std::path::Path) -> Option<Placement> {
    Placement::parse(&std::fs::read_to_string(path).ok()?)
}

/// The place saved last, by any window
fn newest(dir: &std::path::Path) -> Option<Placement> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("window-"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .max_by_key(|(modified, _)| *modified)
        .and_then(|(_, path)| read(&path))
}

/// Where a new window was last, and the file its place is kept in from now on; None when this
/// GUI keeps no places
pub fn find(window: MuxWindowId) -> Option<(Option<Placement>, PathBuf)> {
    let dir = dir()?;
    let first = !OPENED.swap(true, Ordering::Relaxed);
    let file = dir.join(logic::file_name(key(window)));
    let place = logic::pick(read(&file), newest(&dir), first);
    Some((place, file))
}

/// Shows `window` at `place` and keeps its place in `file` from now on, written off the GUI
/// thread
pub fn keep(window: &::window::Window, place: Option<Placement>, file: PathBuf) {
    #[cfg(windows)]
    window.lbterm_keep_placement(
        place,
        Box::new(move |now: Placement| {
            let file = file.clone();
            let text = now.format();
            let spawned = std::thread::Builder::new()
                .name("lbterm-placement".into())
                .spawn(move || {
                    let write = || -> std::io::Result<()> {
                        if let Some(dir) = file.parent() {
                            std::fs::create_dir_all(dir)?;
                        }
                        let tmp = file.with_extension("tmp");
                        std::fs::write(&tmp, text.as_bytes())?;
                        std::fs::rename(&tmp, &file)
                    };
                    if let Err(err) = write() {
                        log::warn!("placement: keeping it in {}: {err:#}", file.display());
                    }
                });
            if let Err(err) = spawned {
                log::warn!("placement: {err:#}");
            }
        }),
    );
    #[cfg(not(windows))]
    let _ = (window, place, file);
}
