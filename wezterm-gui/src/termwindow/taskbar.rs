//! LBTerm: the window's taskbar button shows the progress its panes report with OSC 9;4, as
//! Windows Terminal's does, and with none reported the open tab's Claude session as the
//! Conductor's hub guesses it (conductor_hub.rs). Which it shows: lbterm_extras::taskbar.

use crate::termwindow::TermWindow;
use lbterm_extras::taskbar::{choose, or_guess, TaskbarProgress};
use mux::Mux;
use wezterm_term::Progress;

fn from_pane(progress: Progress) -> TaskbarProgress {
    match progress {
        Progress::None => TaskbarProgress::None,
        Progress::Percentage(pct) => TaskbarProgress::Normal(pct),
        Progress::Error(pct) => TaskbarProgress::Error(pct),
        Progress::Indeterminate => TaskbarProgress::Indeterminate,
        Progress::Paused => TaskbarProgress::Paused,
    }
}

/// Whether window `id`'s button is due to be set again: setting it failed, because Explorer
/// hadn't made the button yet, and a second has gone since. Until titles.rs, every title alert of
/// any pane worked the progress out again, which tried again within a second (a working Claude
/// Code sets its title every 960 ms); now only progress, user vars and tab switches do, so the
/// title alerts ask this (titles.rs, lbterm_title_alert), and a value that doesn't change, as a
/// GUI attaching again is given each pane's, still reaches the button once it's there
pub(crate) fn retry_due(id: mux::window::WindowId) -> bool {
    #[cfg(windows)]
    {
        imp::retry_due(id)
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        false
    }
}

impl TermWindow {
    /// Runs with every update of the title that says progress may have changed (a progress
    /// report, a user var, a tab switch: titles.rs), with one whose button is due to be set
    /// again (retry_due), and whenever the hub's guesses change
    pub(crate) fn update_taskbar_progress(&self) {
        let hub = self
            .config
            .lbterm_conductor
            .as_ref()
            .map_or(false, |c| c.hub_pipe.as_deref().map_or(false, |p| !p.is_empty()));
        let progress = {
            let mux = Mux::get();
            let Some(window) = mux.get_window(self.mux_window_id) else {
                return;
            };
            let active = window
                .get_active_tab()
                .and_then(|tab| tab.get_active_pane())
                .map(|pane| from_pane(pane.get_progress()))
                .unwrap_or_default();
            let all: Vec<TaskbarProgress> = window
                .iter_tabs()
                .flat_map(|tab| tab.iter_panes_ignoring_zoom())
                .map(|pos| from_pane(pos.pane.get_progress()))
                .collect();
            or_guess(choose(active, all), || {
                let tab = window.get_active_tab().filter(|_| hub)?;
                tab.iter_panes_ignoring_zoom().into_iter().find_map(|pos| {
                    let sid = pos.pane.copy_user_vars().remove("claude_session").filter(|s| !s.is_empty())?;
                    crate::conductor_hub::estimate(&sid)?.percent
                })
            })
        };
        // The HWND the WebGpu front end kept: asking the window for it here can panic, because
        // this runs inside window callbacks that hold it borrowed. OpenGL windows get no
        // taskbar progress.
        #[cfg(windows)]
        if let Some(webgpu) = self.webgpu.as_ref() {
            imp::show(webgpu.handle.hwnd(), self.mux_window_id, progress);
        }
        #[cfg(not(windows))]
        let _ = progress;
    }
}

#[cfg(windows)]
mod imp {
    use super::TaskbarProgress;
    use lbterm_extras::taskbar::Retries;
    use mux::window::WindowId as MuxWindowId;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::Instant;
    use windows58::Win32::Foundation::HWND;
    use windows58::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows58::Win32::UI::Shell::{
        ITaskbarList3, TaskbarList, TBPFLAG, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS,
        TBPF_NORMAL, TBPF_PAUSED,
    };

    thread_local! {
        static LIST: RefCell<Option<ITaskbarList3>> = RefCell::new(None);
        /// What each window's button shows, so an unchanged report costs no COM call
        static SHOWN: RefCell<HashMap<MuxWindowId, TaskbarProgress>> = RefCell::new(HashMap::new());
    }

    /// The windows whose button couldn't be set, and when (lbterm_extras::taskbar::Retries): the
    /// process's, not the GUI thread's, since a window's mux subscription asks it wherever it runs
    /// (titles.rs, TitleWatch). Taken on each try to set a button, which only a change of what it
    /// shows makes
    static FAILED: std::sync::LazyLock<std::sync::Mutex<Retries<MuxWindowId>>> =
        std::sync::LazyLock::new(Default::default);

    pub fn retry_due(id: MuxWindowId) -> bool {
        FAILED.lock().unwrap().due(&id, Instant::now())
    }

    fn list() -> Option<ITaskbarList3> {
        LIST.with(|cell| {
            let mut cell = cell.borrow_mut();
            if cell.is_none() {
                unsafe {
                    // Nothing else on the GUI thread joins COM. S_FALSE or RPC_E_CHANGED_MODE
                    // means something did first, which does just as well.
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    match CoCreateInstance::<_, ITaskbarList3>(
                        &TaskbarList,
                        None,
                        CLSCTX_INPROC_SERVER,
                    ) {
                        Ok(list) => match list.HrInit() {
                            Ok(()) => *cell = Some(list),
                            Err(err) => log::warn!("taskbar progress: HrInit: {err:#}"),
                        },
                        Err(err) => log::warn!("taskbar progress: TaskbarList: {err:#}"),
                    }
                }
            }
            cell.clone()
        })
    }

    fn state(progress: TaskbarProgress) -> TBPFLAG {
        match progress {
            TaskbarProgress::None => TBPF_NOPROGRESS,
            TaskbarProgress::Normal(_) => TBPF_NORMAL,
            TaskbarProgress::Error(_) => TBPF_ERROR,
            TaskbarProgress::Indeterminate => TBPF_INDETERMINATE,
            TaskbarProgress::Paused => TBPF_PAUSED,
        }
    }

    pub fn show(hwnd: *mut std::ffi::c_void, id: MuxWindowId, progress: TaskbarProgress) {
        let shown = SHOWN.with(|s| s.borrow().get(&id).copied().unwrap_or_default());
        if shown == progress || hwnd.is_null() {
            return;
        }
        let hwnd = HWND(hwnd);
        let Some(list) = list() else {
            return;
        };
        let result = unsafe {
            // A value set while the state is "none" or "indeterminate" switches it to normal,
            // so the value goes first and the state after
            match progress.value() {
                Some(value) => list.SetProgressValue(hwnd, value, 100),
                None => Ok(()),
            }
            .and_then(|()| list.SetProgressState(hwnd, state(progress)))
        };
        // Until Explorer has made the window's button these fail, and the next report tries
        // again, as does the next title alert a second on (retry_due)
        match result {
            Ok(()) => {
                log::debug!("taskbar progress for window {id}: {progress:?}");
                SHOWN.with(|s| {
                    s.borrow_mut().insert(id, progress);
                });
                FAILED.lock().unwrap().shown(&id);
            }
            Err(err) => {
                log::debug!("taskbar progress: {err:#}");
                FAILED.lock().unwrap().failed(id, Instant::now());
            }
        }
    }
}
