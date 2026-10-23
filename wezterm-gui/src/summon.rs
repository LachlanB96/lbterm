//! LBTerm: the summon key (`lbterm_summon`, lbterm_extras::summon). One key brings LBTerm
//! forward from any other app, un-minimised if need be, at whatever size and place it was.
//!
//! A message-only window on the GUI thread holds a RegisterHotKey, and holds it only while no
//! LBTerm window has the keyboard: inside LBTerm the key goes to the terminal as usual.
//!
//! The window to bring forward is the one that last had the keyboard. Its HWND is read from
//! Windows when it gains focus (it's the foreground window then): TermWindow code runs inside
//! the window's own callbacks, where asking the window for its handle panics on a borrowed
//! RefCell.

use lbterm_extras::summon::Hotkey;

/// The key from the config: `None` when it isn't set or doesn't parse (the error is logged)
pub fn configured(config: &config::ConfigHandle) -> Option<Hotkey> {
    let summon = config.lbterm_summon.as_ref()?;
    match lbterm_extras::summon::parse(&summon.key, &summon.mods) {
        Ok(key) => Some(key),
        Err(err) => {
            log::error!("{err}");
            None
        }
    }
}

#[cfg(windows)]
pub use imp::{focus_changed, set_key};

#[cfg(not(windows))]
pub fn set_key(_: Option<Hotkey>) {}
#[cfg(not(windows))]
pub fn focus_changed(_: bool) {}

#[cfg(windows)]
mod imp {
    use lbterm_extras::summon::Hotkey;
    use std::cell::RefCell;
    use windows58::core::w;
    use windows58::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, WPARAM};
    use windows58::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows58::Win32::System::Threading::GetCurrentThreadId;
    use windows58::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
    };
    use windows58::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, EnumThreadWindows, GetForegroundWindow,
        GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, RegisterClassW,
        SetForegroundWindow, ShowWindow, HWND_MESSAGE, SW_RESTORE, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_HOTKEY, WNDCLASSW,
    };

    /// The hotkey's id on the message window: "LB"
    const HOTKEY_ID: i32 = 0x4c42;

    #[derive(Default)]
    struct State {
        /// The message-only window the hotkey is registered to
        sink: Option<isize>,
        key: Option<Hotkey>,
        registered: bool,
        /// An LBTerm window has the keyboard
        focused: bool,
        /// The LBTerm window that had the keyboard last
        target: Option<isize>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::new(State::default());
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_HOTKEY && wp.0 as i32 == HOTKEY_ID {
            summon();
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    fn sink(state: &mut State) -> Option<HWND> {
        if let Some(h) = state.sink {
            return Some(HWND(h as *mut std::ffi::c_void));
        }
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: w!("LBTermSummon"),
                ..Default::default()
            };
            // Fails harmlessly if the class is already registered
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("LBTermSummon"),
                w!("LBTerm summon"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                None,
                instance,
                None,
            )
            .ok()?;
            state.sink = Some(hwnd.0 as isize);
            Some(hwnd)
        }
    }

    /// Hold the hotkey exactly while there's a key and LBTerm hasn't got the keyboard
    fn sync(state: &mut State) {
        let want = state.key.is_some() && !state.focused;
        if want == state.registered {
            return;
        }
        let Some(sink) = sink(state) else {
            return;
        };
        unsafe {
            if want {
                let key = state.key.unwrap();
                match RegisterHotKey(sink, HOTKEY_ID, HOT_KEY_MODIFIERS(key.mods), key.vk) {
                    Ok(()) => state.registered = true,
                    // Another program holds the same key
                    Err(err) => log::warn!("lbterm_summon: RegisterHotKey: {err:#}"),
                }
            } else {
                let _ = UnregisterHotKey(sink, HOTKEY_ID);
                state.registered = false;
            }
        }
    }

    /// A top-level window of this GUI thread that can be shown, for when the last focused one
    /// has closed
    fn any_window() -> Option<HWND> {
        unsafe extern "system" fn first(hwnd: HWND, found: LPARAM) -> BOOL {
            if IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                *(found.0 as *mut isize) = hwnd.0 as isize;
                return BOOL(0);
            }
            BOOL(1)
        }
        let mut found: isize = 0;
        unsafe {
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(first),
                LPARAM(&mut found as *mut isize as isize),
            );
        }
        (found != 0).then(|| HWND(found as *mut std::ffi::c_void))
    }

    fn summon() {
        let target = STATE.with(|s| s.borrow().target);
        let hwnd = target
            .map(|t| HWND(t as *mut std::ffi::c_void))
            .filter(|h| unsafe { IsWindow(*h).as_bool() })
            .or_else(any_window);
        let Some(hwnd) = hwnd else {
            return;
        };
        unsafe {
            // A hotkey's owner may take the foreground, so this works from any app
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// The config's key, on every config load
    pub fn set_key(key: Option<Hotkey>) {
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            if state.key == key {
                return;
            }
            if state.registered {
                state.key = None;
                sync(&mut state);
            }
            state.key = key;
            sync(&mut state);
        });
    }

    /// An LBTerm window took or lost the keyboard
    pub fn focus_changed(focused: bool) {
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            state.focused = focused;
            if focused {
                unsafe {
                    let fg = GetForegroundWindow();
                    if GetWindowThreadProcessId(fg, None) == GetCurrentThreadId() {
                        state.target = Some(fg.0 as isize);
                    }
                }
            }
            sync(&mut state);
        });
    }
}
