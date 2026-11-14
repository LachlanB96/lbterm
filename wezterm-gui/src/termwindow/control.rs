//! LBTerm: what the control pipe (crate::control, lbterm/CONTROL.md) asks of a window, on the GUI
//! thread, and the `LbtermDraft` key action, which does the same to the pane's voice draft.

use super::render::questions::read_screen;
use super::render::tab_panel::server_pane_id;
use super::{TermWindow, TermWindowNotif};
use config::lbterm::LbtermDraft;
use lbterm_extras::control::{DraftAct, Focus};
use lbterm_extras::drafts::{self, Draft};
use lbterm_extras::questions::Screen;
use mux::pane::Pane;
use mux::Mux;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;
use wezterm_client::pane::ClientPane;
use window::WindowOps;

/// What the pane shows now, at the bottom of its scrollback: what Claude Code has drawn, wherever
/// the window's scrolled to
pub(crate) fn live_screen(pane: &Arc<dyn Pane>) -> Screen {
    let dims = pane.get_dimensions();
    read_screen(pane, dims.physical_top, dims.viewport_rows)
}

/// Types `keys` into `pane`, each written whole, [`drafts::KEY_GAP_MS`] apart: Claude Code takes
/// an Enter written with its text as a new line of a paste
///
/// A key that doesn't get there stops the rest, and `failed` hears it on the GUI thread, with the
/// window's id for the pane, why, and whether the connection ended before the server answered: a
/// voice draft goes back over the pane with why on its scroll strip (Lachlan's 3a of 2026-11-08: a
/// lost dictation is the costliest thing to retype), the context bar's Compact now says so in its
/// pane (render/context_bar.rs). On a mux pane each key's answer is waited for before the next
/// (`ClientPane::lbterm_write_answered`), which a `PaneWriter`'s write isn't, so the failure is
/// heard here, and only here; on the window's own pane a write that fails says so at once
pub(crate) fn type_keys(
    window: Option<::window::Window>,
    pane: &Arc<dyn Pane>,
    keys: Vec<String>,
    failed: impl FnOnce(&mut TermWindow, mux::pane::PaneId, String, bool) + Send + Sync + 'static,
) {
    let pane = Arc::clone(pane);
    let mut failed = Some(failed);
    promise::spawn::spawn(async move {
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                lbterm_extras::counters::timer(smol::Timer::after(Duration::from_millis(drafts::KEY_GAP_MS))).await;
            }
            // The request made before the await, so nothing of the pane is held across it
            let answer = pane
                .downcast_ref::<ClientPane>()
                .map(|client| client.lbterm_write_answered(key.as_bytes()));
            let answer_failed = match answer {
                Some(answer) => answer
                    .await
                    .map(|f| (f.to_string(), f.reached() == mux::lbterm_writes::Reached::Maybe)),
                None => pane.writer().write_all(key.as_bytes()).err().map(|err| (format!("{err:#}"), false)),
            };
            if let Some((why, maybe)) = answer_failed {
                log::warn!("control: typing keys into pane {}: {why}", pane.pane_id());
                let local = pane.pane_id();
                if let (Some(window), Some(failed)) = (window, failed.take()) {
                    window.notify(TermWindowNotif::Apply(Box::new(move |tw| failed(tw, local, why, maybe))));
                }
                break;
            }
        }
    })
    .detach();
}

impl TermWindow {
    /// `focus`: this window's active pane (never an overlay over it), the Claude session in it,
    /// and whether the window has the keyboard
    pub fn lbterm_control_focus(&self) -> Option<Focus> {
        let pane = self.get_active_pane_no_overlay()?;
        Some(Focus {
            window: self.mux_window_id as u64,
            pane: server_pane_id(&pane),
            session: pane
                .copy_user_vars()
                .remove("claude_session")
                .filter(|s| !s.is_empty()),
            title: pane.get_title(),
            foreground: self.focused.is_some(),
        })
    }

    /// `emit`: runs the Lua event `name` here with the active pane, as a key bound to
    /// `EmitEvent` would. The pane it runs with, by the mux server's id; None when the window
    /// has none to run it with.
    pub fn lbterm_control_emit(&mut self, name: &str) -> Option<u64> {
        let pane = self.get_active_pane_no_overlay()?;
        self.window.as_ref()?;
        self.emit_window_event(name, None);
        Some(server_pane_id(&pane))
    }

    /// The pane a request names by the mux server's id, else this window's open one
    fn lbterm_control_pane(&self, pane: Option<u64>) -> Result<Arc<dyn Pane>, String> {
        match pane {
            None => self
                .get_active_pane_no_overlay()
                .ok_or_else(|| "the window has no pane".to_string()),
            Some(id) => Mux::get()
                .iter_panes()
                .into_iter()
                .find(|p| server_pane_id(p) == id)
                .ok_or_else(|| format!("LBTerm has no pane {id}")),
        }
    }

    /// A `draft.*` request: the draft over the pane as it is after it, and what it did. A draft
    /// doesn't outlive the Claude session it began with: once the pane's is another, it's gone.
    pub fn lbterm_control_draft(
        &mut self,
        pane: Option<u64>,
        act: DraftAct,
    ) -> Result<(Option<Draft>, Map<String, Value>), String> {
        let pane = self.lbterm_control_pane(pane)?;
        let id = server_pane_id(&pane);
        let session = pane
            .copy_user_vars()
            .remove("claude_session")
            .filter(|s| !s.is_empty());
        let mut current = crate::drafts::get(id);
        if current.as_ref().map_or(false, |d| d.session != session) {
            crate::drafts::take(id);
            current = None;
        }
        let mut did = Map::new();
        let send = act == DraftAct::Send;
        let keep = |d: Draft| -> Option<Draft> {
            crate::drafts::put(d.clone());
            Some(d).filter(|d| !d.is_empty())
        };
        let current = match act {
            DraftAct::Get => current,
            DraftAct::Add { text, fresh } => {
                let mut d = match current {
                    Some(d) if !fresh => d,
                    _ => Draft::new(id, session),
                };
                d.add(&text)?;
                keep(d)
            }
            DraftAct::Scratch => {
                let Some(mut d) = current else {
                    return Err("there's no draft there to take back".to_string());
                };
                did.insert("removed".to_string(), json!(d.scratch()));
                keep(d)
            }
            DraftAct::Clear => {
                let went = crate::drafts::take(id);
                did.insert("removed".to_string(), json!(went.map(|d| d.text())));
                None
            }
            DraftAct::Send | DraftAct::Edit => {
                let Some(d) = current else {
                    return Err("there's no draft there".to_string());
                };
                let screen = live_screen(&pane);
                let keys = if send {
                    drafts::may_send(&d, &screen)?;
                    d.send_keys()
                } else {
                    drafts::may_edit(&d, &screen)?;
                    vec![d.edit_text()]
                };
                log::info!(
                    "control: voice draft {} into pane {id}",
                    if send { "sent" } else { "typed" }
                );
                let sent = d.clone();
                type_keys(self.window.clone(), &pane, keys, move |tw, local, why, maybe| {
                    tw.lbterm_draft_failed(local, sent, why, maybe)
                });
                crate::drafts::take(id);
                did.insert((if send { "sent" } else { "typed" }).to_string(), json!(d.text()));
                None
            }
        };
        Ok((current, did))
    }

    /// `LbtermDraft`: the voice draft over `pane` sent, typed in for editing, or let go; what
    /// stopped it on the window strip
    pub fn lbterm_draft_key(&mut self, pane: &Arc<dyn Pane>, what: LbtermDraft) {
        let act = match what {
            LbtermDraft::Send => DraftAct::Send,
            LbtermDraft::Edit => DraftAct::Edit,
            LbtermDraft::Clear => DraftAct::Clear,
        };
        if let Err(why) = self.lbterm_control_draft(Some(server_pane_id(pane)), act) {
            self.lbterm_draft_refused(&why);
        }
    }

    /// Why a draft wasn't typed, where he's looking: the window strip's notice
    pub fn lbterm_draft_refused(&mut self, why: &str) {
        self.lbterm_strip_notice(format!("Voice draft: {why}"));
    }

    /// A key of a voice draft's send or edit didn't get to `pane` (the window's id for it): the
    /// draft goes back over the pane unless another has been begun there since
    /// (`drafts::put_back`), and the pane's scroll strip says why, headed "Not sent" or, when the
    /// connection ended before the server answered, "May not have arrived" (Lachlan's 3a and 4a of
    /// 2026-11-08). The keys before the one that failed did get there, so a draft put back may be
    /// partly typed already: the note says to look first
    pub fn lbterm_draft_failed(&mut self, pane: mux::pane::PaneId, sent: Draft, why: String, maybe: bool) {
        let back = drafts::put_back(crate::drafts::get(sent.pane).as_ref(), &sent);
        if back {
            crate::drafts::put(sent);
        }
        let what = match (back, maybe) {
            (true, false) => "your voice draft is held over the pane again",
            (true, true) => "your voice draft is held over the pane again: see what reached the prompt before sending it",
            (false, _) => "a newer voice draft is over the pane, so that one wasn't put back",
        };
        self.lbterm_scroll_write_failed(pane, format!("{why}; {what}"), maybe);
    }
}
