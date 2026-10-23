//! LBTerm: kitty's desktop notifications (OSC 99, lbterm-notify) as Windows toasts
//! (crate::toasts). The window holding the pane decides whether to show one (`o=`), and
//! what a click does: bring the pane forward (`a=focus`, the default) and tell the program
//! (`a=report`), which reads `ESC ] 99 ; i=<id> ; ESC \` on its input.

use crate::termwindow::TermWindow;
use crate::toasts::{self, Toast, ToastEvent};
use lbterm_notify::{reply, Command, Notification, Occasion};
use mux::pane::PaneId;
use mux::Mux;
use std::sync::atomic::{AtomicU64, Ordering};
use window::WindowOps;

/// Notifications sent without an id are each their own toast
static ANONYMOUS: AtomicU64 = AtomicU64::new(1);

/// A toast's tag: the pane and the program's id, so programs in two panes can't collide
fn tag(pane_id: PaneId, id: &str) -> String {
    format!("{pane_id}-{id}")
}

/// Write a reply into the pane's input, as the program reads it
fn tell(pane_id: PaneId, text: &str) {
    if let Some(pane) = Mux::get().get_pane(pane_id) {
        let mut writer = pane.writer();
        let _ = writer.write_all(text.as_bytes());
        let _ = writer.flush();
    }
}

/// Bring the pane's window, tab and pane forward
fn focus(pane_id: PaneId) {
    let mux = Mux::get();
    let Some((_domain, window_id, tab_id)) = mux.resolve_pane_id(pane_id) else {
        return;
    };
    if let Some(mut window) = mux.get_window_mut(window_id) {
        if let Some(idx) = window.get_tab_idx_for_id(tab_id) {
            window.remember_and_set_active_tab_idx(idx);
        }
    }
    if let Some(tab) = mux.get_tab(tab_id) {
        if let Some(pane) = mux.get_pane(pane_id) {
            tab.set_active_pane(&pane);
        }
    }
    if let Some(fe) = crate::frontend::try_front_end() {
        for gui in fe.gui_windows() {
            if gui.mux_window_id == window_id {
                gui.window.focus();
            }
        }
    }
}

/// What happened to a toast, back on the GUI thread
fn acted(pane_id: PaneId, n: &Notification, event: ToastEvent) {
    let id = n.reply_id();
    match event {
        ToastEvent::Activated(button) => {
            if n.actions.focus {
                focus(pane_id);
            }
            if n.actions.report {
                match button {
                    Some(b) => tell(pane_id, &reply::button(id, b)),
                    None => tell(pane_id, &reply::activated(id)),
                }
            }
        }
        ToastEvent::Dismissed => {}
    }
    if n.close_event {
        tell(pane_id, &reply::closed(id));
    }
}

impl TermWindow {
    /// An OSC 99 command from a pane in this window
    pub(crate) fn lbterm_notification(&mut self, pane_id: PaneId, command: Command) {
        match command {
            Command::Show(n) => self.lbterm_show_notification(pane_id, n),
            Command::Close(id) => toasts::hide(&tag(pane_id, &id)),
            Command::Alive(id) => {
                let prefix = format!("{pane_id}-");
                let open: Vec<String> = toasts::open()
                    .into_iter()
                    .filter_map(|t| t.strip_prefix(&prefix).map(str::to_string))
                    .collect();
                tell(pane_id, &reply::alive(&id, open.iter().map(String::as_str)));
            }
            // The terminal answers queries itself, before the GUI hears of them
            Command::Query(_) => {}
        }
    }

    fn lbterm_show_notification(&mut self, pane_id: PaneId, n: Notification) {
        let focused = self.focused.is_some();
        let showing = self
            .get_panes_to_render()
            .iter()
            .any(|p| p.pane.pane_id() == pane_id);
        let wanted = match n.occasion {
            Occasion::Always => true,
            Occasion::Unfocused => !focused,
            Occasion::Invisible => !focused || !showing,
        };
        if !wanted {
            // Not shown is closed, as far as the program is concerned
            if n.close_event {
                tell(pane_id, &reply::closed(n.reply_id()));
            }
            return;
        }
        let toast_tag = match &n.id {
            Some(id) => tag(pane_id, id),
            None => tag(pane_id, &format!("anon{}", ANONYMOUS.fetch_add(1, Ordering::Relaxed))),
        };
        let silent = matches!(n.sound.as_deref(), Some("silent"));
        let toast = Toast {
            tag: toast_tag.clone(),
            title: n.title.clone(),
            body: n.body.clone(),
            buttons: n.buttons.clone(),
            urgency: n.urgency,
            silent,
        };
        let expire_ms = n.expire_ms;
        toasts::show(toast, move |event| {
            let n = n.clone();
            promise::spawn::spawn_into_main_thread(async move { acted(pane_id, &n, event) })
                .detach();
        });
        // w > 0: LBTerm takes it down itself, so it's sure to go
        if expire_ms > 0 {
            promise::spawn::spawn(async move {
                smol::Timer::after(std::time::Duration::from_millis(expire_ms as u64)).await;
                toasts::hide(&toast_tag);
            })
            .detach();
        }
    }
}
