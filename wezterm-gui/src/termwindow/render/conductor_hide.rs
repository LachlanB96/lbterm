//! LBTerm: the tab menu's Hide from Conductor and Show to Conductor (lbterm/NEW-TAB.md, section
//! 8), for a Claude session Lachlan wants to handle himself, and Show on a session in the
//! Conductor's view's Hidden group. LBTerm hides nothing: a pick fires
//! `lbterm-conductor-hide (window, pane, {sessions, label, hide, source})` and his config's handler
//! (Terminal's lbterm.lua) runs `conductor hide|show` once with every session (never a call a
//! session side by side: lbterm/PITFALLS.md, 243). The Conductor then writes
//! its button.json again, `hidden` naming every session it's hidden from, which the
//! `lbterm-state-files` thread reads (wezterm-gui/src/state_files.rs) and every window repaints
//! for.
//!
//! Between the pick and that write the window keeps the pick (`TabPanelState::hides`, a
//! `lbterm_extras::conductor::hide::Picks`): the tab's row looks as asked from the next paint,
//! and stays so for up to 5 s (`hide::PENDING`, the tab links'). Each paint of the panel settles
//! it first (`lbterm_hides_settle`, at the top of render/tab_panel.rs's `paint_tab_panel`): the
//! file says it, and the tab's pad glints; 5 s go by without, and the row goes back to what the
//! file says while the strip says "The Conductor didn't hide <label>" for its notice's 4 s
//! (`lbterm_strip_notice`; the window strip is there only with the tab bar on, so without it the
//! row going back is all that says it). A paint is woken for the end (`TabPanelState::hide_due`,
//! in `tab_panel_wake`'s list), so nothing polls. While a pick for a tab waits, nothing asks for
//! that tab again: its menu's row is greyed, saying "Hiding from Conductor…" or "Showing to
//! Conductor…", and picks nothing (Lachlan's 11b of 2026-11-09; a menu open as the pick settles
//! brings it alive in place, render/tab_menu.rs's `lbterm_menu_hide_again`), and the view's Show
//! does nothing, so two opposite requests never race to the Conductor (PITFALLS 244).
//!
//! A pick also sends a light up the rail from the tab's pad to the Conductor's, the request
//! going to it (Lachlan's 6a of 2026-11-01; `TabPanelState::sends`, played by render/tab_panel.rs's
//! `tab_panel_sends` once no menu's card holds the rail, as the compositor's `Light::Send` on
//! `send`): the answer coming back is the tab's own pad's glint.

use crate::scripting::guiwin::GuiWin;
use crate::termwindow::TermWindow;
use ::window::WindowOps;
use lbterm_extras::conductor::hide::{self, Answer, Look, Pick};
use lbterm_extras::conductor::Offer;
use mux::tab::{Tab, TabId};
use mux::Mux;
use mux_lua::MuxPane;
use std::sync::Arc;
use std::time::Instant;

/// Where a pick came from, as the event's `source` says it: the tab menu's row, or Show in the
/// Conductor's view's Hidden group
const TAB_MENU: &str = "tab menu";
const CONDUCTOR_VIEW: &str = "conductor view";

/// Every Claude session in `tab`'s panes, each once, the active pane's first: what a Hide or a
/// Show from its menu acts on ("every claude_session in the tab's panes", section 8)
pub(crate) fn tab_sessions(tab: &Tab) -> Vec<String> {
    let active = tab.get_active_pane().map(|p| p.pane_id());
    let mut sessions: Vec<(bool, String)> = vec![];
    for pos in tab.iter_panes_ignoring_zoom() {
        let Some(sid) = pos.pane.copy_user_vars().remove("claude_session").filter(|s| !s.is_empty()) else {
            continue;
        };
        let is_active = Some(pos.pane.pane_id()) == active;
        match sessions.iter_mut().find(|(_, s)| *s == sid) {
            Some(had) => had.0 |= is_active,
            None => sessions.push((is_active, sid)),
        }
    }
    sessions.sort_by_key(|(is_active, _)| !*is_active);
    sessions.into_iter().map(|(_, sid)| sid).collect()
}

impl TermWindow {
    /// The tab menu's Hide from Conductor row for `tab`, as the menu opens (and again when a pick
    /// for it begins or settles while the menu's open, `lbterm_menu_hide_again`): none without the
    /// Conductor in the config, on the Conductor's own tab, on a tab with no Claude session or
    /// while its file has no `hidden`; greyed, saying it's hiding or showing, while a pick for the
    /// tab waits for the Conductor's word (`Offer::Waiting`, Lachlan's 11b of 2026-11-09); Show
    /// to Conductor while the file hides every session of the tab (`hide::Picks::offer`)
    pub(crate) fn lbterm_hide_offer(&self, tab: &Tab) -> Option<Offer> {
        if Some(tab.tab_id()) == self.lbterm_panel.conductor_tab {
            return None;
        }
        let said = self.lbterm_conductor_said()?;
        self.lbterm_panel.hides.offer(tab.tab_id(), &tab_sessions(tab), Some(&said))
    }

    /// Hide from Conductor (`hide`) or Show to Conductor picked from `tab`'s menu, its row's
    /// label `label`: kept until the Conductor says it, its row drawn so from the next paint, and
    /// `lbterm-conductor-hide` fired for the config to ask it
    pub(crate) fn lbterm_conductor_hide(&mut self, tab: &Arc<Tab>, label: String, hide: bool) {
        let sessions = tab_sessions(tab);
        if sessions.is_empty() {
            return;
        }
        // The menu's row is greyed and picks nothing while one waits (`hide::Picks::offer`,
        // `menu::inert`), but a menu opened before one began could still be up with its row
        // live: never a second request beside the first (244)
        if self.lbterm_panel.hides.waits(tab.tab_id()) {
            log::info!("conductor: {label:?} picked from its tab menu while a pick for it waits for the Conductor's word: nothing asked");
            return;
        }
        log::info!(
            "conductor: {} {label:?} picked from its tab menu, {} session{}",
            if hide { "hide" } else { "show" },
            sessions.len(),
            if sessions.len() == 1 { "" } else { "s" }
        );
        self.lbterm_hide_pick(tab.tab_id(), sessions.clone(), label.clone(), hide);
        // The tab's own pane, not the window's: the handler's `pane` is the tab it acts on
        let Some(pane) = tab.get_active_pane() else {
            return;
        };
        self.lbterm_fire_conductor_hide(pane.pane_id(), sessions, label, hide, TAB_MENU);
    }

    /// Show pressed on `session_id` in the Conductor's view's Hidden group (lbterm-extras's
    /// seat::layout, `Target::Show`), `label` what the view calls it: the same event as the tab
    /// menu's Show to Conductor, for that session alone. Where this window has its tab, that
    /// tab's row waits for the Conductor's word as a pick from its menu does (its row visible from
    /// the next paint, the glint, or the strip's notice after 5 s), and the event's `pane` is the
    /// tab's own; where it hasn't, nothing waits here, `pane` is the window's active one, and the
    /// view's next `seat.view` moving the session out of the group is the answer. While a pick for
    /// that tab waits it does nothing, as the tab's menu then greys its row: a second request
    /// would race the first (lbterm/PITFALLS.md, 244)
    pub(crate) fn lbterm_view_show(&mut self, session_id: &str, label: String) {
        let found = Mux::get().get_window(self.mux_window_id).and_then(|window| {
            window
                .iter_tabs()
                .find(|tab| tab_sessions(tab).iter().any(|s| s == session_id))
                .cloned()
        });
        if let Some(tab) = found.as_ref().filter(|tab| self.lbterm_panel.hides.waits(tab.tab_id())) {
            log::info!(
                "conductor: show {label:?} pressed in the Conductor's view while a pick for its tab {} waits for the Conductor's word: nothing asked",
                tab.tab_id()
            );
            return;
        }
        let sessions = vec![session_id.to_string()];
        log::info!(
            "conductor: show {label:?} pressed in the Conductor's view, {}",
            if found.is_some() { "its tab in this window" } else { "no tab of it in this window" }
        );
        let pane = match &found {
            Some(tab) => {
                self.lbterm_hide_pick(tab.tab_id(), sessions.clone(), label.clone(), false);
                tab.get_active_pane()
            }
            None => Mux::get().get_active_tab_for_window(self.mux_window_id).and_then(|tab| tab.get_active_pane()),
        };
        let Some(pane) = pane else {
            return;
        };
        self.lbterm_fire_conductor_hide(pane.pane_id(), sessions, label, false, CONDUCTOR_VIEW);
    }

    /// A pick kept until the Conductor's file says it (`hide::Picks`), the window painted for it,
    /// and a light sent up the rail from the tab's pad to the Conductor's (his 6a of 2026-11-01:
    /// `Light::Send`, on `send`; render/tab_panel.rs's `tab_panel_sends`), from the paint that
    /// first shows the row as asked: under a menu's card that's once the card has gone (his 7b).
    /// One a tab while it waits to start, so two picks under one card send one light
    fn lbterm_hide_pick(&mut self, tab: TabId, sessions: Vec<String>, label: String, hide: bool) {
        let said = self.lbterm_conductor_said();
        self.lbterm_panel.hides.ask(Pick::new(tab, sessions, label, hide, said.as_deref(), Instant::now()));
        // Played only with the compositor and the panel on: the composited panel's paint is the
        // only thing that takes it off the list (`tab_panel_glints`), and a light kept while the
        // panel is off would set off with every other kept when it came back
        if self.config.lbterm_tab_panel.enabled && self.lbterm_composited() && !self.lbterm_panel.sends.contains(&tab) {
            self.lbterm_panel.sends.push(tab);
        }
        // A tab menu still open on the tab (a Show pressed in the Conductor's view) greys its row
        self.lbterm_menu_hide_again(tab);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// `lbterm-conductor-hide (window, pane, {sessions, label, hide, source})` fired on the GUI
    /// thread, for the config's handler to ask the Conductor; `source` says where it was picked,
    /// "tab menu" or "conductor view"
    fn lbterm_fire_conductor_hide(&mut self, pane: mux::pane::PaneId, sessions: Vec<String>, label: String, hide: bool, source: &'static str) {
        let window = GuiWin::new(self);
        let pane = MuxPane(pane);

        async fn fire(
            lua: Option<std::rc::Rc<mlua::Lua>>,
            window: GuiWin,
            pane: MuxPane,
            sessions: Vec<String>,
            label: String,
            hide: bool,
            source: &'static str,
        ) -> anyhow::Result<()> {
            let Some(lua) = lua else {
                return Ok(());
            };
            let table = lua.create_table()?;
            table.set("sessions", lua.create_sequence_from(sessions)?)?;
            table.set("label", label)?;
            table.set("hide", hide)?;
            table.set("source", source)?;
            let args = lua.pack_multi((window, pane, table))?;
            config::lua::emit_event(&lua, ("lbterm-conductor-hide".to_string(), args))
                .await
                .map_err(|e| {
                    log::error!("while processing lbterm-conductor-hide event: {:#}", e);
                    e
                })?;
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            fire(lua, window, pane, sessions, label, hide, source)
        }))
        .detach();
    }

    /// The Conductor's word on the picks waiting, at the top of each paint of the panel and
    /// before anything reads the rows: one its file says glints its tab's pad in this paint (the
    /// glints are played after the rows are drawn, render/tab_panel.rs's `tab_panel_glints`, once
    /// no menu's card holds the rail), one that has waited 5 s goes and the strip says so. When
    /// the next still waiting runs out, for the paint's wake. Nothing at all while none waits
    pub(crate) fn lbterm_hides_settle(&mut self) -> Option<Instant> {
        if self.lbterm_panel.hides.is_empty() {
            return None;
        }
        let said = self.lbterm_conductor_said();
        let answers = self.lbterm_panel.hides.settle(said.as_deref(), Instant::now());
        for answer in &answers {
            // A tab menu open on the tab brings its greyed row alive in its place
            let (Answer::Done { tab, .. } | Answer::NotDone { tab, .. }) = answer;
            self.lbterm_menu_hide_again(*tab);
        }
        for answer in answers {
            match answer {
                Answer::Done { tab, hide } => {
                    log::info!("conductor: tab {tab} {} as asked", if hide { "hidden" } else { "shown" });
                    if !self.lbterm_panel.glints.contains(&tab) {
                        self.lbterm_panel.glints.push(tab);
                    }
                }
                Answer::NotDone { tab, hide, label } => {
                    log::warn!(
                        "conductor: tab {tab} not {} within {:?} of the pick: its file doesn't say it",
                        if hide { "hidden" } else { "shown" },
                        hide::PENDING
                    );
                    self.lbterm_strip_notice(hide::not_done(&label, hide));
                }
            }
        }
        self.lbterm_panel.hides.due()
    }

    /// How each of the rail's rows (`rail`, their Claude sessions `sessions` as
    /// render/tab_panel.rs's `tab_panel_tabs` found them, none without the Conductor in the
    /// config) looks for the Conductor's hiding: as a pick waiting for it asked, else as its file
    /// says. Every row visible, at once, while the file hides nothing and no pick waits
    pub(crate) fn lbterm_hide_looks(&self, rail: &[TabId], sessions: &[Vec<(String, u64)>]) -> Vec<Look> {
        let said = self.lbterm_conductor_said();
        let none = said.as_ref().and_then(|s| s.hidden.as_ref()).map_or(true, |h| h.is_empty());
        if none && self.lbterm_panel.hides.is_empty() {
            return vec![Look::default(); rail.len()];
        }
        rail.iter()
            .enumerate()
            .map(|(i, &tab)| {
                let ids: Vec<&str> = sessions.get(i).map_or_else(Vec::new, |s| s.iter().map(|(sid, _)| sid.as_str()).collect());
                self.lbterm_panel.hides.look(tab, &ids, said.as_deref())
            })
            .collect()
    }
}
