//! LBTerm: a key sent to a pane asks for a frame of its own only when that frame draws something
//! new (lbterm_extras::echo; lbterm/PERF.md, finding 35). A key typed at a shell or into Claude
//! Code changes nothing LBTerm draws until the pane echoes it, and the echo's own notification
//! (`mux_pane_output_event`) asks for the frame that shows it. Upstream's frame for the key came
//! first and drew nothing, and the pacing held the echo's frame back a period behind it. A dead
//! key's or an IME's composition ending as its composed character goes to the pane is the same:
//! its frame waits for the echo's (`lbterm_composition_frame`).

use crate::termwindow::{TermWindow, TermWindowNotif};
use ::window::{DeadKeyStatus, Window, WindowOps};
use mux::pane::Pane;
use smol::Timer;
use std::sync::Arc;
use wezterm_term::StableRowIndex;

/// What a key's frame turns on that the key itself can change, read before it's sent
pub(crate) struct KeyMark {
    viewport: Option<StableRowIndex>,
}

impl TermWindow {
    /// Before a key goes to `pane`
    pub(crate) fn lbterm_key_mark(&self, pane: &Arc<dyn Pane>) -> KeyMark {
        KeyMark {
            viewport: self.get_viewport(pane.pane_id()),
        }
    }

    /// After it went: whether it asks for a frame of its own (lbterm_extras::echo::key_frame)
    pub(crate) fn lbterm_key_frame(&self, pane: &Arc<dyn Pane>, mark: KeyMark) -> bool {
        // Copy mode, search, quick select or a tab's overlay (the launcher) took it: the pane the
        // key went to isn't the tab's own
        let overlay =
            self.get_active_pane_no_overlay().map(|p| p.pane_id()) != Some(pane.pane_id());
        let client = pane.downcast_ref::<wezterm_client::pane::ClientPane>();
        let predicts = client
            .map(|c| c.renderable.lock().inner.borrow().should_predict())
            .unwrap_or(false);
        let tells_output =
            client.is_some() || pane.downcast_ref::<mux::localpane::LocalPane>().is_some();
        lbterm_extras::echo::key_frame(lbterm_extras::echo::KeySent {
            overlay,
            scrolled: self.get_viewport(pane.pane_id()) != mark.viewport,
            predicts,
            tells_output,
        })
    }

    /// Before a dead key's or an IME's composition becomes `status`: whether it asks for a frame
    /// at once (lbterm_extras::echo::composition_frame). Its end doesn't: Windows sends the
    /// composed character next, whose echo's frame clears it, and a frame painted first would
    /// draw nothing more and hold that one a pacing period. A frame is asked for
    /// COMPOSITION_CLEAR_WAIT on only when none has painted by then (a character the pane doesn't
    /// echo, a composition cancelled, a key a binding took)
    pub(crate) fn lbterm_composition_frame(
        &mut self,
        status: &DeadKeyStatus,
        window: &Window,
    ) -> bool {
        let was_composing = self.dead_key_status != DeadKeyStatus::None;
        let composing = *status != DeadKeyStatus::None;
        if lbterm_extras::echo::composition_frame(was_composing, composing) {
            return true;
        }
        self.lbterm_composition_ended = true;
        let window = window.clone();
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(Timer::after(config::lbterm::COMPOSITION_CLEAR_WAIT))
                .await;
            let win = window.clone();
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if std::mem::take(&mut tw.lbterm_composition_ended) {
                    win.invalidate();
                }
            })));
        })
        .detach();
        false
    }
}
