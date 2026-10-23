//! LBTerm: tabs with the mouse (lbterm/TAB-MOUSE.md). Drag a tab along the tab panel (or the tab
//! bar, when it shows tabs) to move it; drag it out of the window and let go to give it a window
//! of its own, there, or onto another LBTerm window to move it into that one. A right-click on a
//! tab opens the tab menu (render/tab_menu.rs).

use crate::termwindow::TermWindow;
use ::window::{
    ConnectionOps, CursorIcon, MouseButtons, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps,
};
use lbterm_extras::tabs::{self, Drop, Rect};
use mux::tab::{Tab, TabId};
use mux::window::WindowId;
use mux::Mux;
use std::sync::Arc;

/// Where a dragged tab was picked up
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Panel,
    Bar,
}

struct Drag {
    tab: TabId,
    /// Where the press was, on the screen
    from: (f64, f64),
    started: bool,
    source: Source,
    /// Where the pointer is in the window while it drags the tab along the panel: the
    /// tab's row follows it, lifted (render/tab_panel.rs)
    along: Option<f64>,
    /// The other LBTerm window whose rail shows where the tab would land, while the
    /// pointer is over it
    hinted: Option<usize>,
}

#[derive(Default)]
pub struct TabMouseState {
    drag: Option<Drag>,
}

impl TermWindow {
    /// A left press on the tab at `tab_idx`: it may turn into a drag
    pub(crate) fn lbterm_tab_pressed(&mut self, tab_idx: usize, event: &MouseEvent, source: Source) {
        let tab = Mux::get()
            .get_window(self.mux_window_id)
            .and_then(|w| w.get_tab_at_idx(tab_idx).map(|t| t.tab_id()));
        if let Some(tab) = tab {
            self.lbterm_tab_mouse.drag = Some(Drag {
                tab,
                from: (event.screen_coords.x as f64, event.screen_coords.y as f64),
                started: false,
                source,
                along: None,
                hinted: None,
            });
        }
    }

    /// The slot the pointer is over, along the panel's rows or the tab bar's tabs, as the
    /// window's tab index
    fn lbterm_tab_slot(&self, source: Source, x: f64, y: f64) -> Option<usize> {
        match source {
            Source::Panel => {
                let layout = self.lbterm_panel.layout.as_ref()?;
                let spans: Vec<(f64, f64)> = (0..layout.rows)
                    .map(|i| {
                        let pill = layout.pill(i);
                        (pill.y as f64, pill.h as f64)
                    })
                    .collect();
                // A row's place on the rail, which leaves the Conductor's tab out (it stays first)
                self.tab_panel_tab_index(tabs::slot(&spans, y)?)
            }
            Source::Bar => {
                let mut spans: Vec<(usize, f64, f64)> = self
                    .ui_items
                    .iter()
                    .filter_map(|item| match item.item_type {
                        crate::termwindow::UIItemType::TabBar(
                            crate::tabbar::TabBarItem::Tab { tab_idx, .. },
                        ) => Some((tab_idx, item.x as f64, item.width as f64)),
                        _ => None,
                    })
                    .collect();
                spans.sort_by_key(|s| s.0);
                let spans: Vec<(f64, f64)> = spans.into_iter().map(|(_, x, w)| (x, w)).collect();
                tabs::slot(&spans, x)
            }
        }
    }

    /// Moves and the release while a tab is being dragged: true when the drag took the event
    pub(crate) fn lbterm_tab_drag_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(drag) = self.lbterm_tab_mouse.drag.as_mut() else {
            return false;
        };
        let at = (event.screen_coords.x as f64, event.screen_coords.y as f64);
        let scale = self.dimensions.dpi as f64 / 96.0;
        match event.kind {
            WMEK::Move => {
                if !event.mouse_buttons.contains(MouseButtons::LEFT) {
                    self.lbterm_tab_mouse.drag = None;
                    return false;
                }
                if !drag.started {
                    if !tabs::dragging(drag.from, at, scale) {
                        return false;
                    }
                    drag.started = true;
                }
                let (tab, source) = (drag.tab, drag.source);
                // Along the panel or the bar: the tab moves to the slot under the pointer. Off
                // past the window it's on its way out, and stays where it was
                let (x, y) = (event.coords.x as f64, event.coords.y as f64);
                let near = self.lbterm_own_rect(event).distance(at.0, at.1) < tabs::TEAR_OFF * scale;
                if let Some(drag) = self.lbterm_tab_mouse.drag.as_mut() {
                    drag.along = (near && source == Source::Panel).then_some(y);
                }
                // Far enough out to tear off: its ghost follows the pointer, and another
                // LBTerm window under it shows where it would land
                if near {
                    crate::tab_ghost::hide();
                    self.lbterm_drop_hint(None);
                } else {
                    let title = Mux::get().get_tab(tab).map(|t| t.get_title()).unwrap_or_default();
                    let offset = 14.0 * scale;
                    crate::tab_ghost::show(&title, (at.0 + offset) as i32, (at.1 + offset) as i32, scale);
                    let over = self
                        .lbterm_other_windows()
                        .into_iter()
                        .find(|(_, r, _)| r.contains(at.0, at.1));
                    self.lbterm_drop_hint(over.map(|(id, _, window)| (id, window)));
                }
                if let Some(slot) = self.lbterm_tab_slot(source, x, y).filter(|_| near) {
                    let now = Mux::get()
                        .get_window(self.mux_window_id)
                        .and_then(|w| w.get_tab_idx_for_id(tab));
                    if now.is_some() && now != Some(slot) {
                        // move_tab moves the active tab, which the press made this one
                        if let Err(err) = self.move_tab(slot) {
                            log::error!("tab drag: {err:#}");
                        }
                        context.invalidate();
                    }
                }
                context.set_cursor(Some(CursorIcon::Grabbing));
                true
            }
            WMEK::Release(MousePress::Left) => {
                crate::tab_ghost::hide();
                self.lbterm_drop_hint(None);
                let drag = self.lbterm_tab_mouse.drag.take().unwrap();
                if !drag.started {
                    return false;
                }
                let own = self.lbterm_own_rect(event);
                let others = self.lbterm_other_windows();
                let ids: Vec<(usize, Rect)> = others.iter().map(|(id, r, _)| (*id, *r)).collect();
                match tabs::drop_at(own, &ids, at, scale) {
                    Drop::Stay => {}
                    Drop::NewWindow { x, y } => self.lbterm_tear_off(drag.tab, Some((x, y))),
                    Drop::OtherWindow(id) => {
                        self.lbterm_move_tab_to_window(drag.tab, id);
                        if let Some((_, _, window)) = others.into_iter().find(|(i, _, _)| *i == id) {
                            window.focus();
                        }
                    }
                }
                context.set_cursor(Some(CursorIcon::Default));
                context.invalidate();
                true
            }
            _ => false,
        }
    }

    /// The drop hint on the other window `over` names (its mux window and window), and off
    /// the one that had it
    fn lbterm_drop_hint(&mut self, over: Option<(usize, ::window::Window)>) {
        let Some(drag) = self.lbterm_tab_mouse.drag.as_mut() else {
            return;
        };
        let want = over.as_ref().map(|(id, _)| *id);
        if drag.hinted == want {
            return;
        }
        let had = std::mem::replace(&mut drag.hinted, want);
        let others = self.lbterm_other_windows();
        if let Some((_, _, window)) = others.iter().find(|(id, _, _)| Some(*id) == had) {
            window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(|tw| tw.lbterm_panel_drop_hint(false))));
        }
        if let Some((_, window)) = over {
            window.notify(crate::termwindow::TermWindowNotif::Apply(Box::new(|tw| tw.lbterm_panel_drop_hint(true))));
        }
    }

    /// The tab being dragged along the panel, and where the pointer is in the window, for
    /// its row to follow
    pub(crate) fn lbterm_dragged_row(&self) -> Option<(TabId, f64)> {
        let drag = self.lbterm_tab_mouse.drag.as_ref().filter(|d| d.started)?;
        Some((drag.tab, drag.along?))
    }

    /// This window's client area on the screen
    fn lbterm_own_rect(&self, event: &MouseEvent) -> Rect {
        Rect {
            x: (event.screen_coords.x - event.coords.x) as f64,
            y: (event.screen_coords.y - event.coords.y) as f64,
            w: self.dimensions.pixel_width as f64,
            h: self.dimensions.pixel_height as f64,
        }
    }

    /// LBTerm's other windows, front-most first: their mux window, where they are, and the
    /// window to bring forward
    fn lbterm_other_windows(&self) -> Vec<(usize, Rect, ::window::Window)> {
        let Some(fe) = crate::frontend::try_front_end() else {
            return vec![];
        };
        fe.gui_windows()
            .into_iter()
            .filter(|g| g.mux_window_id != self.mux_window_id)
            .filter_map(|g| Some((g.mux_window_id, screen_rect(&g.window)?, g.window.clone())))
            .collect()
    }

    /// Give a tab a window of its own, its top-left at `at` on the screen (or where Windows puts
    /// new windows). A tab in a mux server's domain goes through the server, which owns its
    /// windows; that moves its active pane, and the GUI makes the window when the server says
    /// it's there, at the position left for the next new window.
    pub(crate) fn lbterm_tear_off(&mut self, tab_id: TabId, at: Option<(f64, f64)>) {
        let mux = Mux::get();
        let Some(tab) = mux.get_tab(tab_id) else {
            return;
        };
        let count = mux.get_window(self.mux_window_id).map_or(0, |w| w.count_tabs());
        if count < 2 {
            // The only tab already has the window to itself
            return;
        }
        let local = tab.iter_panes_ignoring_zoom().iter().all(|p| {
            mux.get_domain(p.pane.domain_id())
                .map_or(true, |d| d.domain_name() == "local")
        });
        let workspace = mux.active_workspace();
        // ScreenCoordinateSystem counts from the whole desktop's top-left, which is left of and
        // above the main screen's corner when a monitor sits there
        let desktop = ::window::Connection::get()
            .and_then(|conn| conn.screens().ok())
            .map_or((0.0, 0.0), |s| {
                (s.virtual_rect.origin.x as f64, s.virtual_rect.origin.y as f64)
            });
        let position = at.map(|(x, y)| config::GuiPosition {
            x: config::Dimension::Pixels((x - desktop.0) as f32),
            y: config::Dimension::Pixels((y - desktop.1) as f32),
            origin: config::GeometryOrigin::ScreenCoordinateSystem,
        });
        if !local {
            if let Some(position) = position {
                crate::termwindow::set_window_position(position);
            }
            move_through_server(&tab, None, Some(workspace));
            return;
        }
        if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
            window.remove_tab_id(tab_id);
        }
        let builder = mux.new_empty_window(Some(workspace), position);
        if let Err(err) = mux.add_tab_to_window(&tab, *builder) {
            log::error!("tab tear-off: {err:#}");
        }
        // Dropping the builder tells the GUI there's a new window to show
        drop(builder);
        self.update_title();
    }

    /// Move a tab into another of LBTerm's windows, as its active tab
    fn lbterm_move_tab_to_window(&mut self, tab_id: TabId, window_id: usize) {
        let mux = Mux::get();
        let Some(tab) = mux.get_tab(tab_id) else {
            return;
        };
        let local = tab.iter_panes_ignoring_zoom().iter().all(|p| {
            mux.get_domain(p.pane.domain_id())
                .map_or(true, |d| d.domain_name() == "local")
        });
        if !local {
            move_through_server(&tab, Some(window_id), None);
            return;
        }
        if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
            if window.count_tabs() < 2 {
                return;
            }
            window.remove_tab_id(tab_id);
        }
        if let Err(err) = mux.add_tab_to_window(&tab, window_id) {
            log::error!("tab move: {err:#}");
            return;
        }
        if let Some(mut window) = mux.get_window_mut(window_id) {
            if let Some(idx) = window.get_tab_idx_for_id(tab_id) {
                window.remember_and_set_active_tab_idx(idx);
            }
        }
        self.update_title();
    }
}

/// Move a tab in a mux server's domain: the server moves its active pane into a new tab, in
/// `window` or a window of its own. That's a new tab, so it's given the old one's name and
/// colour, and made its window's active tab.
fn move_through_server(tab: &Arc<Tab>, window: Option<WindowId>, workspace: Option<String>) {
    let Some(pane) = tab.get_active_pane() else {
        return;
    };
    let pane_id = pane.pane_id();
    let title = tab.get_title();
    let colour = crate::tabcolors::get(tab.tab_id());
    let old = Arc::clone(tab);
    promise::spawn::spawn(async move {
        let mux = Mux::get();
        match mux.move_pane_to_new_tab(pane_id, window, workspace).await {
            Ok((tab, window_id)) => {
                // The client domain's resync adds the new tab but leaves the pane in the old
                // one too. Take it out (remove_pane doesn't kill it), and the old tab with it
                // once it's empty: an empty tab has no panes to kill
                if old.tab_id() != tab.tab_id() {
                    old.remove_pane(pane_id);
                    if old.count_panes() == Some(0) {
                        mux.remove_tab(old.tab_id());
                    }
                }
                if !title.is_empty() {
                    tab.set_title(&title);
                }
                crate::tabcolors::set(tab.tab_id(), colour);
                if let Some(mut window) = mux.get_window_mut(window_id) {
                    if let Some(idx) = window.get_tab_idx_for_id(tab.tab_id()) {
                        window.remember_and_set_active_tab_idx(idx);
                    }
                }
            }
            Err(err) => {
                log::error!("moving a tab through its server: {err:#}");
                // No window came of it, so the next new one mustn't open where this was let go
                if window.is_none() {
                    crate::termwindow::take_window_position();
                }
            }
        }
    })
    .detach();
}

/// A window's rectangle on the screen. Only for windows other than the one whose callback is
/// running: asking that one for its handle panics.
#[cfg(windows)]
fn screen_rect(window: &::window::Window) -> Option<Rect> {
    use ::window::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows58::Win32::Foundation::{HWND, RECT};
    use windows58::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let hwnd = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut std::ffi::c_void),
        _ => return None,
    };
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r).ok()? };
    Some(Rect {
        x: r.left as f64,
        y: r.top as f64,
        w: (r.right - r.left) as f64,
        h: (r.bottom - r.top) as f64,
    })
}

#[cfg(not(windows))]
fn screen_rect(_: &::window::Window) -> Option<Rect> {
    None
}
