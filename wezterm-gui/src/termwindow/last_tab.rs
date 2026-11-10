//! LBTerm: the last tab (Lachlan, 2026-10-25). The Conductor's pad tops the tab rail and stands
//! for its tab, or with a seat for its view, so to him it's a tab: closing every other one mustn't
//! close LBTerm. When the tab being closed is the last of the last window and the pad is on the
//! rail (`conductor::keeps_window`), a new tab opens first, in `default_cwd` and the closing tab's
//! domain, and the old one goes once it's there: the mux prunes an empty window at once, and a
//! mux server ends a second after its last pane. With a seat, the Conductor's view then opens over
//! the new tab, since the Conductor is what he kept. Closing the window or quitting still ends
//! everything.
//!
//! Everything the GUI closes comes through here: the tab menu's Close tab, CloseCurrentTab
//! (Ctrl+W), CloseCurrentPane on a tab's only pane (Ctrl+Shift+W), the tab bar's close button,
//! and the Yes of their questions. So does a last tab whose program ends by itself (`exit`, or
//! `lbterm cli kill-pane` in the mux server; Lachlan's yes, 2026-10-25): the mux keeps a window's
//! last dead tab for the keeper installed here (mux's `lbterm_keep`) instead of pruning it, and
//! the keeper hands it to its window's `lbterm_remove_tab`. In a mux server the server has
//! dropped its own window by then, so the new tab opens in a new one there that stands for this
//! window (wezterm-client's `ClientDomain::spawn`).

use crate::spawn::SpawnWhere;
use crate::termwindow::{TermWindow, TermWindowNotif};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::TermConfig;
use lbterm_extras::{closing, conductor};
use mux::activity::Activity;
use mux::pane::{Pane, PaneId};
use mux::tab::TabId;
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;
use window::WindowOps;

thread_local! {
    /// The last tabs waiting for the new tab that takes their place, so closing one again in the
    /// meantime (Ctrl+W twice) doesn't open a second
    static REPLACING: RefCell<HashSet<TabId>> = RefCell::new(HashSet::new());
}

/// Have the mux leave a window's last tab whose program ended to its window, which decides
/// whether a new tab takes its place (`lbterm_remove_tab`). Only while the config could put the
/// Conductor's pad on the rail: otherwise the mux prunes it as upstream does. An Activity is held
/// until the window has it, since the GUI quits once the mux is empty and none is.
pub(crate) fn install_keeper() {
    mux::lbterm_keep::set_keeper(Box::new(|window: MuxWindowId, tab: TabId| {
        let config = config::configuration();
        if !config.lbterm_tab_panel.enabled || config.lbterm_conductor.is_none() {
            return false;
        }
        let hold = Activity::new();
        promise::spawn::spawn_into_main_thread(async move {
            let gui = crate::frontend::try_front_end().and_then(|fe| fe.gui_window_for_mux_window(window));
            match gui {
                Some(gui) => gui.window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                    tw.lbterm_remove_tab(tab);
                    drop(hold);
                }))),
                None => {
                    Mux::get().remove_tab(tab);
                    drop(hold);
                }
            }
        })
        .detach();
        true
    }));
}

impl TermWindow {
    /// The new tab that takes `tab`'s place, when closing it would otherwise close LBTerm while
    /// the Conductor's pad stays on the rail
    fn lbterm_last_tab_spawn(&self, tab: TabId) -> Option<SpawnCommand> {
        let mux = Mux::get();
        let pad = self.config.lbterm_tab_panel.enabled && self.lbterm_conductor_head().is_some();
        // A tab just closed can sit in its window until its server has the kill: only the live
        // ones count
        let live = |window: &mux::window::Window| {
            window
                .iter_tabs()
                .filter(|t| mux.get_tab(t.tab_id()).is_some())
                .count()
        };
        let windows = mux
            .iter_windows()
            .into_iter()
            .filter(|&id| mux.get_window(id).map_or(false, |w| live(&*w) > 0))
            .count();
        let window = mux.get_window(self.mux_window_id)?;
        window.get_tab_idx_for_id(tab)?;
        if !conductor::keeps_window(pad, windows, live(&*window)) {
            return None;
        }
        drop(window);
        let domain = mux.get_tab(tab)?.get_active_pane()?.domain_id();
        Some(SpawnCommand {
            cwd: self.config.default_cwd.clone(),
            domain: SpawnTabDomain::DomainId(domain),
            ..Default::default()
        })
    }

    /// Close `tab`, a new tab taking its place first when it's the last (above)
    pub(crate) fn lbterm_remove_tab(&mut self, tab: TabId) {
        if REPLACING.with(|r| r.borrow().contains(&tab)) {
            return;
        }
        // Timed to the first frame without it (termwindow/budget.rs)
        self.lbterm_budget_begin(
            lbterm_extras::budget::Action::CloseTab,
            super::budget::Shown::TabGone(tab),
        );
        let Some(spawn) = self.lbterm_last_tab_spawn(tab) else {
            Mux::get().remove_tab(tab);
            return;
        };
        log::info!("last tab: {tab} closes, a new tab taking its place first: the Conductor's pad stays");
        REPLACING.with(|r| r.borrow_mut().insert(tab));
        // Held from now, before the spawn's own: the GUI quits on an empty mux while none is
        let hold = Activity::new();
        let size = self.terminal_size;
        let term_config = Arc::new(TermConfig::with_config(self.config.clone()));
        let window_id = self.mux_window_id;
        let gui = self.window.clone();
        let view = self.lbterm_view_on();
        promise::spawn::spawn(async move {
            if let Err(err) =
                crate::spawn::spawn_command_internal(spawn, SpawnWhere::NewTab, size, Some(window_id), term_config)
                    .await
            {
                // It closes all the same: that's what he asked for
                log::error!("last tab: the new tab didn't open: {err:#}");
            }
            Mux::get().remove_tab(tab);
            REPLACING.with(|r| r.borrow_mut().remove(&tab));
            if let (true, Some(gui)) = (view, gui) {
                gui.notify(TermWindowNotif::Apply(Box::new(|tw| tw.lbterm_view_open())));
            }
            drop(hold);
        })
        .detach();
    }

    /// LBTerm: a close mark for each Claude Code session in `panes`, which Lachlan is closing
    /// himself (lbterm_extras::closing). Nothing without `lbterm_close_marks`.
    fn lbterm_mark_closing(&self, panes: &[Arc<dyn Pane>]) {
        let Some(dir) = self.config.lbterm_close_marks.as_deref().filter(|d| !d.is_empty()) else {
            return;
        };
        let sessions: Vec<String> = panes
            .iter()
            .filter_map(|p| p.copy_user_vars().get("claude_session").cloned())
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let (written, errors) =
            closing::write_marks(std::path::Path::new(dir), sessions.iter().map(String::as_str), now);
        for path in written {
            log::info!("close mark: {}", path.display());
        }
        for err in errors {
            log::warn!("close mark not written: {err}");
        }
    }

    /// The close marks for every pane of `tab`, which Lachlan is closing himself: called just
    /// before it closes, after any question is answered yes, and never for what closes on its
    /// own (the keeper above, a window closing, quitting). The tab is kept to reopen too
    /// (render/new_tab.rs, lbterm/NEW-TAB.md's "Reopen"), with or without `lbterm_close_marks`.
    pub(crate) fn lbterm_mark_tab(&mut self, tab: TabId) {
        if let Some(tab) = Mux::get().get_tab(tab) {
            self.lbterm_new_tab_closing(&tab);
            let panes: Vec<_> = tab.iter_panes_ignoring_zoom().into_iter().map(|p| p.pane).collect();
            self.lbterm_mark_closing(&panes);
        }
    }

    /// The close mark for `pane`, which Lachlan is closing himself, as for a tab. A tab's only
    /// pane closes the tab, which is kept to reopen as a tab closed is; a pane split off from
    /// others isn't a tab, and isn't.
    pub(crate) fn lbterm_mark_pane(&mut self, pane: PaneId) {
        let mux = Mux::get();
        if let Some(pane) = mux.get_pane(pane) {
            let alone = mux
                .resolve_pane_id(pane.pane_id())
                .and_then(|(_, _, tab)| mux.get_tab(tab))
                .filter(|t| t.iter_panes_ignoring_zoom().len() == 1);
            if let Some(tab) = alone {
                self.lbterm_new_tab_closing(&tab);
            }
            self.lbterm_mark_closing(&[pane]);
        }
    }

    /// Close `pane` as its tab, through [`Self::lbterm_remove_tab`], when it's the only pane of
    /// the last tab and a new tab takes its place: true when it did, and otherwise the caller
    /// closes it its own way
    pub(crate) fn lbterm_close_last_pane(&mut self, pane: PaneId) -> bool {
        let mux = Mux::get();
        let Some((_, _, tab)) = mux.resolve_pane_id(pane) else {
            return false;
        };
        let alone = mux
            .get_tab(tab)
            .map_or(false, |t| t.iter_panes_ignoring_zoom().len() == 1);
        if !alone || self.lbterm_last_tab_spawn(tab).is_none() {
            return false;
        }
        self.lbterm_remove_tab(tab);
        true
    }
}
