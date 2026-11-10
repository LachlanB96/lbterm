//! LBTerm: the GUI thread's work besides painting, Windows' side (lbterm_extras::work has the
//! rules and the counts): whether a message waits for the window, the mouse cursor's shape as it
//! was last given to Windows, the accent colour as it was last read, and whether the session can
//! be seen.

use crate::CursorIcon;
use lbterm_extras::work::{cursor_may_have_changed, CursorNews, SessionWatch, Told, WORK};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use wezterm_color_types::LinearRgba;
use winapi::shared::guiddef::IsEqualGUID;
use winapi::shared::minwindef::{BOOL, DWORD, LPARAM, UINT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::winnt::{GUID_SESSION_DISPLAY_STATUS, HANDLE};
use winapi::um::winuser::{
    GetQueueStatus, IsIconic, RegisterPowerSettingNotification, UnregisterPowerSettingNotification,
    DEVICE_NOTIFY_WINDOW_HANDLE, HPOWERNOTIFY, PBT_POWERSETTINGCHANGE,
    POWERBROADCAST_SETTING, QS_INPUT, QS_POSTMESSAGE, WM_CAPTURECHANGED, WM_KEYDOWN,
    WM_MOUSELEAVE, WM_NCMOUSELEAVE, WM_POWERBROADCAST, WM_SETCURSOR, WM_SYSKEYDOWN,
    WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::UI::ViewManagement::{UIColorType, UISettings};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

/// Whether a message waits for the thread's windows: a key, the pointer, or a posted message (a
/// test's posted keys and moves are posted messages, not input). GetQueueStatus's high word is
/// what's in the queue now. Asking resets its low word, what came since it was last asked, as
/// PeekMessage does; the message loop takes the message with PeekMessage before it waits again,
/// so the wait still wakes for whatever comes after.
pub(crate) fn message_waiting() -> bool {
    let status = unsafe { GetQueueStatus(QS_INPUT | QS_POSTMESSAGE) };
    (status >> 16) & (QS_INPUT | QS_POSTMESSAGE) != 0
}

#[link(name = "kernel32")]
extern "system" {
    fn QueryPerformanceCounter(count: *mut i64) -> BOOL;
}

/// The message loop took `msg` off the queue: a key press is counted with when, on the clock a
/// test's posts read too (QueryPerformanceCounter), so the test knows how long it waited
/// (lbterm_extras::work's `key_qpc`)
pub(crate) fn message_taken(msg: UINT) {
    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
        let mut qpc = 0i64;
        unsafe {
            QueryPerformanceCounter(&mut qpc);
        }
        WORK.key(qpc as u64);
    }
}

thread_local! {
    /// The shape each window last gave Windows for the mouse cursor, on the GUI thread
    static CURSORS: RefCell<HashMap<usize, Told<Option<CursorIcon>>>> =
        RefCell::new(HashMap::new());
}

/// Whether `cursor` is to go to Windows for `hwnd`, asked on the GUI thread: it isn't the shape
/// given last. It's the one given from here on. Every pointer move over a pane gave the same shape
/// again, in a task of its own (window.rs's `set_cursor`).
pub(crate) fn tell_cursor(hwnd: HWND, cursor: Option<CursorIcon>) -> bool {
    CURSORS.with(|c| c.borrow_mut().entry(hwnd as usize).or_default().tell(cursor))
}

/// `hwnd` gave Windows `cursor`: the task that sets it, whichever thread asked
pub(crate) fn cursor_given(hwnd: HWND, cursor: Option<CursorIcon>) {
    CURSORS.with(|c| {
        let mut told = Told::new();
        told.tell(cursor);
        c.borrow_mut().insert(hwnd as usize, told);
    });
}

/// What `msg` says of the mouse cursor `hwnd` gave: another may have set it since, so the next is
/// given whatever it is (lbterm_extras::work::cursor_may_have_changed)
pub(crate) fn cursor_message(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) {
    let news = match msg {
        WM_SETCURSOR => CursorNews::SetCursor {
            own: wparam as HWND == hwnd,
            hit: (lparam & 0xffff) as u16 as i16,
        },
        WM_MOUSELEAVE | WM_NCMOUSELEAVE => CursorNews::Left,
        WM_CAPTURECHANGED => CursorNews::CaptureChanged,
        _ => CursorNews::Other,
    };
    if cursor_may_have_changed(news) {
        CURSORS.with(|c| {
            if let Some(told) = c.borrow_mut().get_mut(&(hwnd as usize)) {
                told.forget();
            }
        });
    }
}

/// ColorPrevalence ("Show accent colour on title bars and window borders") and the accent colour,
/// as they were last read, for the 1 px border at the window's top. Every focus change read them
/// again, the registry and a UISettings object (a WinRT activation); now once, and again after
/// Windows says a setting changed ([`accent_changed`]).
static ACCENT: Mutex<(Option<u32>, Option<LinearRgba>)> = Mutex::new((None, None));

/// ColorPrevalence, read once
pub(crate) fn accent_prevalence() -> anyhow::Result<u32> {
    let mut accent = ACCENT.lock().unwrap();
    if let Some(prevalence) = accent.0 {
        return Ok(prevalence);
    }
    WORK.accent();
    let prevalence = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("SOFTWARE\\Microsoft\\Windows\\DWM")?
        .get_value::<u32, _>("ColorPrevalence")?;
    accent.0 = Some(prevalence);
    Ok(prevalence)
}

/// The accent colour, read once
pub(crate) fn accent_colour() -> anyhow::Result<LinearRgba> {
    let mut accent = ACCENT.lock().unwrap();
    if let Some(colour) = accent.1 {
        return Ok(colour);
    }
    WORK.accent();
    let colour = UISettings::new()?.GetColorValue(UIColorType::Accent)?;
    let colour = LinearRgba::with_srgba(colour.R, colour.G, colour.B, 255);
    accent.1 = Some(colour);
    Ok(colour)
}

/// A setting or the colorization colour changed (WM_SETTINGCHANGE,
/// WM_DWMCOLORIZATIONCOLORCHANGED): they're read again when next asked, as every focus change did
pub(crate) fn accent_changed() {
    *ACCENT.lock().unwrap() = (None, None);
    SETTINGS_CHANGED.fetch_add(1, Ordering::Relaxed);
}

/// How many times Windows has said a setting changed since the process started (WM_SETTINGCHANGE,
/// WM_DWMCOMPOSITIONCHANGED, WM_DWMCOLORIZATIONCOLORCHANGED)
static SETTINGS_CHANGED: AtomicU64 = AtomicU64::new(0);

/// How many times Windows has said a setting changed, so what's read from Windows' settings once
/// can be read again when this moves: wezterm-gui's motion, whose `enabled = 'system'` follows
/// Animation effects (compositor.rs, `MotionCache`), read once a change, not on every paint
pub fn settings_changes() -> u64 {
    SETTINGS_CHANGED.load(Ordering::Relaxed)
}

#[link(name = "wtsapi32")]
extern "system" {
    fn WTSRegisterSessionNotification(hwnd: HWND, flags: DWORD) -> BOOL;
    fn WTSUnRegisterSessionNotification(hwnd: HWND) -> BOOL;
    fn WTSQuerySessionInformationW(
        server: HANDLE,
        session: DWORD,
        class: i32,
        buffer: *mut *mut u16,
        bytes: *mut DWORD,
    ) -> BOOL;
    fn WTSFreeMemory(memory: *mut c_void);
}

const NOTIFY_FOR_THIS_SESSION: DWORD = 0;
const WTS_CURRENT_SESSION: DWORD = 0xFFFF_FFFF;
/// WTS_INFO_CLASS's WTSSessionInfoEx
const WTS_SESSION_INFO_EX: i32 = 25;
/// Where WTSINFOEXW's WTSINFOEX_LEVEL1_W has SessionFlags: after its Level (a DWORD) the union is
/// 8-byte aligned (it holds LARGE_INTEGERs), and SessionFlags follows SessionId and SessionState
const SESSION_FLAGS_AT: usize = 16;

static LOCKED: AtomicBool = AtomicBool::new(false);
static SCREENS_OFF: AtomicBool = AtomicBool::new(false);
/// The windows told of the lock: LOCKED counts only while one is (lbterm_extras::work::SessionWatch)
static SESSION: SessionWatch = SessionWatch::new();

/// Whether the session is locked now, asked of Windows: when a window is told of the lock with
/// none before it, whose notifications say each change after (WTSQuerySessionInformation, about
/// 0.6 ms on 2026-10-30)
fn read_locked() -> Option<bool> {
    let mut buffer = std::ptr::null_mut();
    let mut bytes = 0;
    let ok = unsafe {
        WTSQuerySessionInformationW(
            std::ptr::null_mut(),
            WTS_CURRENT_SESSION,
            WTS_SESSION_INFO_EX,
            &mut buffer,
            &mut bytes,
        )
    };
    if ok == 0 || buffer.is_null() {
        log::warn!(
            "WTSQuerySessionInformation: {}; the session is taken as unlocked until it says otherwise",
            std::io::Error::last_os_error()
        );
        return None;
    }
    let flags = if bytes as usize >= SESSION_FLAGS_AT + 4 {
        Some(unsafe { std::ptr::read_unaligned((buffer as *const u8).add(SESSION_FLAGS_AT) as *const i32) })
    } else {
        None
    };
    unsafe { WTSFreeMemory(buffer as *mut c_void) };
    flags.and_then(lbterm_extras::work::session_locked)
}

/// What a window was told of, for [`unwatch`]
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Watch {
    /// RegisterPowerSettingNotification's handle, 0 for none
    power: usize,
    /// Whether WTSRegisterSessionNotification took it
    session: bool,
}

/// `hwnd` is told when the session locks and unlocks and when the screens go off and on: what
/// [`unseen`] goes by.
pub(crate) fn watch(hwnd: HWND) -> Watch {
    unsafe {
        let session = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) != 0;
        if !session {
            log::warn!(
                "WTSRegisterSessionNotification: {}; this window paints while the session is locked",
                std::io::Error::last_os_error()
            );
        } else if SESSION.watched() {
            // Told from now, so read after: no window was told of a lock or an unlock till now,
            // the first's or since the last went
            LOCKED.store(read_locked().unwrap_or(false), Ordering::Relaxed);
        }
        // Windows says the screens' state at once, then each change
        let power = RegisterPowerSettingNotification(
            hwnd as HANDLE,
            &GUID_SESSION_DISPLAY_STATUS,
            DEVICE_NOTIFY_WINDOW_HANDLE,
        );
        if power.is_null() {
            log::warn!(
                "RegisterPowerSettingNotification: {}; this window paints while the screens are off",
                std::io::Error::last_os_error()
            );
        }
        Watch {
            power: power as usize,
            session,
        }
    }
}

/// `hwnd` is going: no more notifications for it
pub(crate) fn unwatch(hwnd: HWND, watch: Watch) {
    unsafe {
        if watch.session {
            WTSUnRegisterSessionNotification(hwnd);
            SESSION.unwatched();
        }
        if watch.power != 0 {
            UnregisterPowerSettingNotification(watch.power as HPOWERNOTIFY);
        }
    }
    CURSORS.with(|c| c.borrow_mut().remove(&(hwnd as usize)));
}

/// Whether `msg` is Windows saying the session locked or unlocked or the screens went off or on;
/// it's noted for [`unseen`]
pub(crate) fn session_message(msg: UINT, wparam: WPARAM, lparam: LPARAM) -> bool {
    match msg {
        WM_WTSSESSION_CHANGE if wparam == WTS_SESSION_LOCK || wparam == WTS_SESSION_UNLOCK => {
            LOCKED.store(wparam == WTS_SESSION_LOCK, Ordering::Relaxed);
            true
        }
        WM_POWERBROADCAST if wparam == PBT_POWERSETTINGCHANGE && lparam != 0 => {
            let setting = unsafe { &*(lparam as *const POWERBROADCAST_SETTING) };
            if !IsEqualGUID(&setting.PowerSetting, &GUID_SESSION_DISPLAY_STATUS)
                || setting.DataLength < 4
            {
                return false;
            }
            let status = unsafe { std::ptr::read_unaligned(setting.Data.as_ptr() as *const u32) };
            SCREENS_OFF.store(lbterm_extras::work::display_off(status), Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

/// Whether this process's windows go by the session's lock and the screens: a test window with
/// a state file doesn't, unless `LBTERM_UNSEEN=skip` (lbterm_extras::work::honours_session)
fn honours_session() -> bool {
    static HONOURS: OnceLock<bool> = OnceLock::new();
    *HONOURS.get_or_init(|| {
        lbterm_extras::work::honours_session(
            lbterm_extras::state::path().is_some(),
            std::env::var("LBTERM_UNSEEN").ok().as_deref(),
        )
    })
}

/// Whether `hwnd` can't be seen now: minimized, or the session locked or the screens off
pub(crate) fn unseen(hwnd: HWND) -> bool {
    let session =
        SESSION.locked(LOCKED.load(Ordering::Relaxed)) || SCREENS_OFF.load(Ordering::Relaxed);
    lbterm_extras::work::unseen(unsafe { IsIconic(hwnd) } != 0, session, honours_session())
}

/// Whether the window `hwnd` (a raw HWND) can't be seen now, as its paints go by ([`unseen`]):
/// what wezterm-gui's loops go by (lbterm_extras::work::loops_play). Each change of it comes with
/// a paint: a minimize or a restore changes the window's size, and the lock and the screens
/// invalidate it (window.rs)
pub fn window_unseen(hwnd: *mut c_void) -> bool {
    !hwnd.is_null() && unseen(hwnd as HWND)
}
