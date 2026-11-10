//! LBTerm: frame pacing on the GUI thread, Windows' side (lbterm_extras::pace has the rules and
//! the counts).
//!
//! wm_paint holds a window's next frame back for a period (lbterm_extras::pace::hold_until). A
//! WM_PAINT during the hold validates the window, so Windows doesn't send it again, and asks for
//! a wake at the hold's end ([`Holds::wake`]). The message loop (connection.rs) waits on a
//! high-resolution waitable timer (CREATE_WAITABLE_TIMER_HIGH_RESOLUTION) beside its events, set
//! for the soonest wake ([`Holds::arm`]), and at each turn ends the holds that are up
//! ([`Connection::end_paint_holds`]): their windows repaint. So no task, timer or thread is spent
//! on a frame, and only a hold something asked for during it wakes the thread.
//!
//! The timer is high-resolution on its own, without timeBeginPeriod, which would raise the timer
//! interrupt's rate for the whole PC. On 2026-10-30, in a process that doesn't raise it, an 8 ms
//! wait on an ordinary timer, an event or an I/O completion port (async-io's) took 8.1 to 23.7 ms,
//! 13.8 to 14.5 on average, ending on Windows' 15.6 ms tick; on this timer 8.06 to 8.58 ms, 8.27
//! on average.

use super::connection::Connection;
use super::window::{end_paint_hold, HWindow};
use lbterm_extras::pace::{Arm, Wakes};
use std::cell::RefCell;
use std::io::Error as IoError;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use winapi::shared::minwindef::BOOL;
use winapi::shared::ntdef::LARGE_INTEGER;
use winapi::shared::windef::{HMONITOR, HWND};
use winapi::shared::winerror::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use winapi::um::handleapi::CloseHandle;
use winapi::um::synchapi::{CreateWaitableTimerExW, SetWaitableTimerEx};
use winapi::um::wingdi::QDC_ONLY_ACTIVE_PATHS;
use winapi::um::winnt::{HANDLE, SYNCHRONIZE, TIMER_MODIFY_STATE};
use winapi::um::winuser::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME,
};

/// CreateWaitableTimerExW's flag for a timer that ends on time whatever the system's timer
/// resolution (Windows 10 1803 and later); winapi 0.3 doesn't have it
const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThread() -> HANDLE;
    fn QueryThreadCycleTime(thread: HANDLE, cycles: *mut u64) -> BOOL;
}

/// The cycles this thread has run, for what a paint costs it (lbterm_extras::pace)
pub(crate) fn thread_cycles() -> u64 {
    let mut cycles = 0;
    unsafe {
        QueryThreadCycleTime(GetCurrentThread(), &mut cycles);
    }
    cycles
}

/// The holds a frame was asked for during (lbterm_extras::pace::Wakes), and the timer that wakes
/// the message loop for the soonest.
pub(crate) struct Holds {
    timer: HANDLE,
    wakes: RefCell<Wakes<HWindow>>,
}

impl Drop for Holds {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.timer);
        }
    }
}

impl Holds {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let timer = unsafe {
            CreateWaitableTimerExW(
                null_mut(),
                null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_MODIFY_STATE | SYNCHRONIZE,
            )
        };
        if timer.is_null() {
            anyhow::bail!(
                "the frame pacing's high-resolution waitable timer couldn't be made: {}",
                IoError::last_os_error()
            );
        }
        Ok(Self {
            timer,
            wakes: RefCell::new(Wakes::default()),
        })
    }

    pub(crate) fn timer(&self) -> HANDLE {
        self.timer
    }

    /// `window`'s hold, which ends at `until`, had a frame asked for during it: it repaints then
    pub(crate) fn wake(&self, window: HWindow, until: Instant) {
        self.wakes.borrow_mut().wake(window, until);
    }

    /// Sets the timer for the soonest wake, when it isn't set for it already. False with none, or
    /// when it couldn't be set: there's nothing for the loop to wait on but its events.
    pub(crate) fn arm(&self) -> bool {
        let at = match self.wakes.borrow_mut().arm() {
            Arm::Idle => return false,
            Arm::Armed => return true,
            Arm::Set(at) => at,
        };
        let mut due: LARGE_INTEGER = unsafe { std::mem::zeroed() };
        unsafe {
            *due.QuadPart_mut() = lbterm_extras::pace::due_time(Instant::now(), at);
        }
        let set =
            unsafe { SetWaitableTimerEx(self.timer, &due, 0, None, null_mut(), null_mut(), 0) };
        if set == 0 {
            // Not taken as set, so the next turn asks again; till then a held frame shows when
            // something else wakes the loop
            log::error!(
                "frame pacing: the waitable timer couldn't be set: {}",
                IoError::last_os_error()
            );
            self.wakes.borrow_mut().disarm();
            return false;
        }
        true
    }
}

impl Connection {
    /// The holds that are up end: each window a frame was asked for during its hold repaints.
    /// Every turn of the message loop, and inside a live resize's modal loop (WM_SIZING), where
    /// upstream ran the main thread's tasks, its hold's timer task among them.
    pub(crate) fn end_paint_holds(&self) {
        let now = Instant::now();
        let due = self.paint_holds.wakes.borrow_mut().take_due(now);
        for (window, until) in due {
            let Some(inner) = self.get_window(window) else {
                continue;
            };
            match inner.try_borrow_mut() {
                Ok(mut inner) => end_paint_hold(&mut inner, until, now),
                // Busy (it can't be at a turn of the loop): the next turn tries again
                Err(_) => self.paint_holds.wake(window, until),
            };
        }
    }
}

/// Displays changed (WM_DISPLAYCHANGE) this many times: a screen's refresh rate is looked up again
/// after each
static DISPLAYS: AtomicU64 = AtomicU64::new(0);

/// WM_DISPLAYCHANGE came: a screen's mode, and so its refresh rate, may be another
pub(crate) fn displays_changed() {
    DISPLAYS.fetch_add(1, Ordering::Relaxed);
}

/// The screen a window was last painted on, and one refresh of it then
#[derive(Clone, Copy)]
pub(crate) struct Screen {
    monitor: HMONITOR,
    displays: u64,
    refresh: Option<Duration>,
}

/// One refresh of the screen `hwnd` is on, at its exact rate, if Windows says: looked up only when
/// the window is on another screen than last time, or the displays changed since.
pub(crate) fn refresh_period(hwnd: HWND, screen: &mut Option<Screen>) -> Option<Duration> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let displays = DISPLAYS.load(Ordering::Relaxed);
    if let Some(known) = *screen {
        if known.monitor == monitor && known.displays == displays {
            return known.refresh;
        }
    }
    let refresh = monitor_refresh(monitor);
    *screen = Some(Screen {
        monitor,
        displays,
        refresh,
    });
    refresh
}

/// One refresh of `monitor`: the rate of the screen its display path ends in, as QueryDisplayConfig
/// gives it, a fraction (lbterm_extras::pace::refresh_period), the fastest when it's cloned onto
/// several. EnumDisplaySettings' dmDisplayFrequency, read until 2026-10-30, is a whole number
/// Windows rounds: on 2026-10-30 the main screen here was 159952/1000 Hz, which it called 160, and
/// a 59.94 Hz mode it calls 59. 19 to 21 us with four screens, once a window's screen or the
/// displays change.
fn monitor_refresh(monitor: HMONITOR) -> Option<Duration> {
    let device = unsafe {
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        ) == 0
        {
            log::error!(
                "frame pacing: GetMonitorInfoW: {}; frames go by max_fps alone",
                IoError::last_os_error()
            );
            return None;
        }
        info.szDevice
    };
    let paths = match active_paths() {
        Ok(paths) => paths,
        Err(err) => {
            log::error!("frame pacing: {err:#}; frames go by max_fps alone");
            return None;
        }
    };
    let mut fastest: Option<Duration> = None;
    for path in &paths {
        let mut source: DISPLAYCONFIG_SOURCE_DEVICE_NAME = unsafe { std::mem::zeroed() };
        source.header.adapterId = path.sourceInfo.adapterId;
        source.header.id = path.sourceInfo.id;
        source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != ERROR_SUCCESS as i32 {
            continue;
        }
        if until_nul(&source.viewGdiDeviceName) != until_nul(&device) {
            continue;
        }
        let rate = path.targetInfo.refreshRate;
        if let Some(refresh) =
            lbterm_extras::pace::refresh_period(rate.Numerator, rate.Denominator)
        {
            fastest = Some(fastest.map_or(refresh, |f| f.min(refresh)));
        }
    }
    fastest
}

/// The active display paths (QueryDisplayConfig, as connection.rs's screen names read them)
fn active_paths() -> anyhow::Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
    loop {
        let mut path_count = 0u32;
        let mut mode_count = 0u32;
        let result = unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        };
        if result != ERROR_SUCCESS as i32 {
            anyhow::bail!("GetDisplayConfigBufferSizes: {}", IoError::from_raw_os_error(result));
        }
        let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![];
        let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![];
        paths.resize_with(path_count as usize, || unsafe { std::mem::zeroed() });
        modes.resize_with(mode_count as usize, || unsafe { std::mem::zeroed() });
        let result = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                null_mut(),
            )
        };
        // The paths changed between the two calls
        if result == ERROR_INSUFFICIENT_BUFFER as i32 {
            continue;
        }
        if result != ERROR_SUCCESS as i32 {
            anyhow::bail!("QueryDisplayConfig: {}", IoError::from_raw_os_error(result));
        }
        paths.truncate(path_count as usize);
        return Ok(paths);
    }
}

/// A wide string up to its NUL
fn until_nul(s: &[u16]) -> &[u16] {
    &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]
}
