//! LBTerm: the ghost of a dragged tab (lbterm/TAB-MOUSE.md). Once a tab dragged out of the window
//! is far enough to tear off, a small card with its name follows the pointer, where its window
//! would open if let go there. It's a layered popup of its own: it never takes the keyboard or
//! comes in front of what Lachlan types into (no activation, a tool window), and the mouse goes
//! through it (transparent), so the drag keeps the pointer.

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use windows58::core::{w, PCWSTR};
    use windows58::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows58::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
        DrawTextW, EndPaint, FillRect, GetDC, GetStockObject, GetTextExtentPoint32W, ReleaseDC,
        RoundRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, DT_END_ELLIPSIS,
        DT_SINGLELINE, DT_VCENTER, HFONT, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
    };
    use windows58::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows58::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetLayeredWindowAttributes,
        SetWindowPos, ShowWindow, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE,
        SW_SHOWNOACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };

    const CLASS: PCWSTR = w!("LBTermTabGhost");
    /// The card: the panes' surface, the panel's accent hairline, the title's colour (sRGB)
    const SURFACE: (u8, u8, u8) = (0x12, 0x12, 0x12);
    const RING: (u8, u8, u8) = (0xFF, 0x3B, 0x30);
    const TEXT: (u8, u8, u8) = (0xEC, 0xEC, 0xF4);
    /// See-through enough to read what's under it, solid enough to read
    const ALPHA: u8 = 230;

    fn colour((r, g, b): (u8, u8, u8)) -> COLORREF {
        COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
    }

    struct Ghost {
        hwnd: HWND,
        title: Vec<u16>,
        font: HFONT,
        radius: i32,
        pad: i32,
    }

    thread_local! {
        static GHOST: RefCell<Option<Ghost>> = RefCell::new(None);
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            // The pointer goes through it, to whatever's under it
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                GHOST.with(|g| {
                    if let Some(g) = g.borrow().as_ref() {
                        let mut r = RECT::default();
                        let _ = windows58::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r);
                        let brush = CreateSolidBrush(colour(SURFACE));
                        FillRect(dc, &r, brush);
                        let _ = DeleteObject(brush);
                        let pen = CreatePen(PS_SOLID, 1, colour(RING));
                        let old_pen = SelectObject(dc, pen);
                        let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                        let _ = RoundRect(dc, 0, 0, r.right, r.bottom, 2 * g.radius, 2 * g.radius);
                        SelectObject(dc, old_brush);
                        SelectObject(dc, old_pen);
                        let _ = DeleteObject(pen);
                        let old_font = SelectObject(dc, g.font);
                        SetBkMode(dc, TRANSPARENT);
                        SetTextColor(dc, colour(TEXT));
                        let mut text = g.title.clone();
                        let mut inner = RECT {
                            left: g.pad,
                            top: 0,
                            right: r.right - g.pad,
                            bottom: r.bottom,
                        };
                        DrawTextW(dc, &mut text, &mut inner, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                        SelectObject(dc, old_font);
                    }
                });
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn register() -> bool {
        static REGISTERED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *REGISTERED.get_or_init(|| unsafe {
            let Ok(module) = GetModuleHandleW(None) else {
                return false;
            };
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: module.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassExW(&class) != 0
        })
    }

    /// Show the ghost of `title` with its top-left at `(x, y)` on the screen, at `scale` (the
    /// window's DPI over 96); again, it only moves
    pub fn show(title: &str, x: i32, y: i32, scale: f64) {
        if !register() {
            return;
        }
        GHOST.with(|cell| {
            let mut ghost = cell.borrow_mut();
            if let Some(g) = ghost.as_ref() {
                unsafe {
                    let _ = SetWindowPos(g.hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
                }
                return;
            }
            let px = |v: f64| (v * scale).round() as i32;
            let title: Vec<u16> = title.encode_utf16().collect();
            unsafe {
                let font = CreateFontW(
                    -px(13.0), 0, 0, 0, 500, 0, 0, 0, 0, 0, 0, 5, 0, w!("Segoe UI"),
                );
                // As wide as the name, within reason
                let dc = GetDC(None);
                let old = SelectObject(dc, font);
                let mut size = Default::default();
                let _ = GetTextExtentPoint32W(dc, &title, &mut size);
                SelectObject(dc, old);
                ReleaseDC(None, dc);
                let (pad, radius) = (px(12.0), px(8.0));
                let w = (size.cx + 2 * pad).clamp(px(80.0), px(320.0));
                let h = px(30.0);
                let Ok(hwnd) = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                    CLASS,
                    w!("LBTerm tab"),
                    WS_POPUP,
                    x,
                    y,
                    w,
                    h,
                    None,
                    None,
                    GetModuleHandleW(None).map(HINSTANCE::from).unwrap_or_default(),
                    None,
                ) else {
                    let _ = DeleteObject(HGDIOBJ(font.0));
                    return;
                };
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), ALPHA, LWA_ALPHA);
                let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, 2 * radius, 2 * radius);
                SetWindowRgn(hwnd, region, true);
                *ghost = Some(Ghost {
                    hwnd,
                    title,
                    font,
                    radius,
                    pad,
                });
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        });
    }

    /// Take the ghost away
    pub fn hide() {
        GHOST.with(|cell| {
            if let Some(g) = cell.borrow_mut().take() {
                unsafe {
                    let _ = DestroyWindow(g.hwnd);
                    let _ = DeleteObject(HGDIOBJ(g.font.0));
                }
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn show(_title: &str, _x: i32, _y: i32, _scale: f64) {}
    pub fn hide() {}
}

pub use imp::{hide, show};
