//! LBTerm: the window's taskbar button shows the progress its panes report with OSC 9;4, as
//! Windows Terminal's does. Which pane's progress it shows: lbterm_extras::taskbar.

use crate::termwindow::TermWindow;
use lbterm_extras::taskbar::{choose, TaskbarProgress};
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

impl TermWindow {
    /// Runs with every title update, which every progress report and tab switch brings
    pub(crate) fn update_taskbar_progress(&self) {
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
            choose(active, all)
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
    use mux::window::WindowId as MuxWindowId;
    use std::cell::RefCell;
    use std::collections::HashMap;
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
        // Until Explorer has made the window's button these fail, and the next report tries again
        match result {
            Ok(()) => {
                log::debug!("taskbar progress for window {id}: {progress:?}");
                SHOWN.with(|s| {
                    s.borrow_mut().insert(id, progress);
                })
            }
            Err(err) => log::debug!("taskbar progress: {err:#}"),
        }
    }
}
