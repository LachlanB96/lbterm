//! LBTerm: a mux pane out of the open tab that has gone quiet has the rows it would show scanned
//! for links then, once, so a switch to it doesn't wait on the scan (lbterm/MUX.md, "Tabs out of
//! view"; when and why: `lbterm_extras::hidden_scan`). Its output is noted in the window's mux
//! subscription, where budget.rs's `lbterm_hidden_output` takes it with no task. A push that
//! ended its server's hold of it (the server of this build pushes a pane out of view once it has
//! been quiet: lbterm/MUX.md, "Tabs out of view: output held at the server") is scanned on the
//! GUI thread's next turn, one task for every such push meanwhile; any other push that brought
//! rows (a pane another window shows, or a server of an older build, which pushes as it prints)
//! waits for the window's one timer, which looks at the panes noted when the first is due. Only a
//! push that brought the pane rows is noted: one of its title alone finds its screen scanned
//! already.

use crate::termwindow::{TermWindow, TermWindowNotif};
use ::window::{Window, WindowOps};
use lbterm_extras::hidden_scan::{QuietPanes, Scan};
use mux::pane::Pane;
use mux::Mux;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use wezterm_term::StableRowIndex;

/// The window's panes out of view with output since their screen was last scanned
pub(crate) type Quiet = Arc<Mutex<QuietPanes>>;

/// The window's panes out of view, none noted yet. Their quiet span is the one a mux server holds
/// a pane's lines for before it pushes them (`config::lbterm::HELD_QUIET`), so a push that ended
/// a hold is one of a pane quiet that long and is scanned at once: one number for both, so they
/// can't come apart
pub(crate) fn new_quiet() -> Quiet {
    Arc::new(Mutex::new(QuietPanes::new(config::lbterm::HELD_QUIET)))
}

/// A mux pane's output out of the open tab, which `lbterm_hidden_output` took: noted when it
/// brought the pane rows (`ClientPane::lbterm_rows_changed`), scanned at the next turn when it
/// ended a hold (`ClientPane::lbterm_holds_ended`), else the window's timer armed if it isn't.
/// A lock and two map inserts a push
pub(crate) fn output(quiet: &Quiet, pane: &Arc<dyn Pane>, window: &Window) {
    let Some(client) = pane.downcast_ref::<wezterm_client::pane::ClientPane>() else {
        return;
    };
    let rows = client.lbterm_rows_changed();
    let ended = client.lbterm_holds_ended();
    let scan = quiet
        .lock()
        .unwrap()
        .output(pane.pane_id(), rows, ended, Instant::now());
    match scan {
        Scan::Nothing => {}
        Scan::Arm(at) => arm(window.clone(), at),
        // Window::notify queues its own task to reach the window, so nothing runs here, inside
        // the push's PaneOutput with its pane locked (PITFALLS 89)
        Scan::Soon => window.notify(TermWindowNotif::Apply(Box::new(|tw: &mut TermWindow| {
            let due = tw.lbterm_quiet.lock().unwrap().due_now(Instant::now());
            tw.lbterm_scan_hidden(due)
        }))),
    }
}

fn arm(window: Window, at: Instant) {
    promise::spawn::spawn_into_main_thread(async move {
        lbterm_extras::counters::timer(smol::Timer::at(at)).await;
        window.notify(TermWindowNotif::Apply(Box::new(|tw: &mut TermWindow| {
            tw.lbterm_hidden_scan()
        })));
    })
    .detach();
}

impl TermWindow {
    /// The timer for the panes out of view fired: each that has printed nothing for
    /// its quiet span (`config::lbterm::HELD_QUIET`) has the rows it would show scanned for links, as its first frame in
    /// view would have; the timer is armed again for those still printing
    pub(crate) fn lbterm_hidden_scan(&mut self) {
        let (due, next) = self.lbterm_quiet.lock().unwrap().due(Instant::now());
        if let (Some(at), Some(window)) = (next, self.window.clone()) {
            arm(window, at);
        }
        self.lbterm_scan_hidden(due);
    }

    /// `due`, panes out of view gone quiet: each this window still has out of view has the rows
    /// it would show scanned for links
    fn lbterm_scan_hidden(&mut self, due: Vec<usize>) {
        if due.is_empty() || self.config.hyperlink_rules.is_empty() {
            return;
        }
        let mux = Mux::get();
        let open = mux.get_active_tab_for_window(self.mux_window_id);
        for pane_id in due {
            // A pane brought into view since is scanned by its paint, and another window's by
            // that window, which knows where it's scrolled to
            if open.as_ref().map_or(false, |tab| tab.contains_pane(pane_id)) {
                continue;
            }
            match mux.resolve_pane_id(pane_id) {
                Some((_, window, _)) if window == self.mux_window_id => {}
                _ => continue,
            }
            let Some(pane) = mux.get_pane(pane_id) else {
                continue;
            };
            let dims = pane.get_dimensions();
            let top = self.get_viewport(pane_id).unwrap_or(dims.physical_top);
            pane.apply_hyperlinks(
                top..top + dims.viewport_rows as StableRowIndex,
                &self.config.hyperlink_rules,
            );
        }
    }
}
