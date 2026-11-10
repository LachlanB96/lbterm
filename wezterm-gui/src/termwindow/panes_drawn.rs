//! LBTerm: a window says to its mux server which push of each pane it shows its frames have
//! read (codec's PanesDrawn), so a server that paces a shown pane sends the next push once a
//! frame has read the last, one push travelling while a frame draws, where it pushed one a parser
//! batch whatever the frames took (Lachlan's 12b of 2026-11-07 and 2a of 2026-11-08; lbterm/MUX.md,
//! "A shown pane's pace"; wezterm-client's lbterm_drawn.rs).
//!
//! The paint says what it drew. As `paint_pass` reaches each pane it draws, just before reading
//! it, `lbterm_frame_read` asks a client pane whether a push of it has been applied since it was
//! last said (an atomic load; nothing else for a pane with nothing new, a local pane or a server
//! that doesn't know the PDU), and notes it in a list the window keeps, so a frame allocates
//! nothing until it has something to say. At `paint_impl`'s end `lbterm_say_read` sends one
//! PanesDrawn a server for what the frame noted. The GUI thread applies pushes and paints, so no
//! push is applied between a pane's note and its read, and the seqno said is the one drawn.
//!
//! WezTerm has two kinds of overlay, and a frame reads a pane under each differently
//! (lbterm/CODEMAP.md, "A frame"). A pane overlay (`pane_state(pane).overlay`,
//! `assign_overlay_for_pane`) takes one pane's place in `get_pos_panes_for_tab`: copy mode,
//! search and quick select are overlays that report the id of the pane under them and read
//! their lines from it (overlay/copy.rs's and quickselect.rs's `delegate`), so the frame reads
//! that pane and `lbterm_frame_read` notes it, the overlay resolved to the pane under it
//! (`lbterm_under_overlay`); the close-pane confirmation is a pane of its own, with its own id,
//! drawn over the pane. A tab overlay (`tab_state(tab).overlay`, `assign_overlay`: the launcher,
//! the tab navigator, the palette) replaces the whole tab with its one pane.
//!
//! A push of a pane the window shows that no frame will read soon is said at once
//! (`lbterm_output_unpainted`, from `mux_pane_output_event`), since the server would otherwise
//! hold the pane's next push for a word that doesn't come: under the Conductor's view, whose
//! window doesn't paint for pane output; under a tab overlay, or under a pane overlay of its own
//! (the close-pane confirmation), whose frames read the overlay's pane, not this one; and in a
//! window that can't be seen (minimized, the session locked, the screens off), which paints once
//! a second, so its shown panes are pushed as they print and its restore draws them current
//! (Lachlan's 4a). A push that asks for no frame at all is said by the client pane as it's
//! applied (`RenderableInner::lbterm_say_drawn`).

use crate::termwindow::TermWindow;
use lbterm_extras::overlays;
use mux::pane::{Pane, PaneId};
use mux::Mux;
use std::sync::Arc;
use wezterm_client::pane::ClientPane;

/// Whether a window's frames read a pane that printed (`lbterm_pane_shows`; lbterm-extras'
/// overlays.rs, where it's decided and tested)
pub(crate) use lbterm_extras::overlays::Shows;

/// What a paint has read and not yet said: each pane with its newest push applied
pub(crate) type Read = Vec<(Arc<dyn Pane>, codec::PaneDrawn)>;

impl TermWindow {
    /// The frame is about to read `pane`: noted, when it's a client pane with a push applied
    /// since it was last said, for `lbterm_say_read` at the paint's end. A pane overlay that
    /// reads the pane under it (copy mode, search, quick select) stands in the paint with that
    /// pane's id, so the pane under it is what's noted (`lbterm_under_overlay`). Costs nothing
    /// more for a client pane, and a lookup that finds nothing for a local one
    pub(crate) fn lbterm_frame_read(&mut self, pane: &Arc<dyn Pane>) {
        if let Some(client) = pane.downcast_ref::<ClientPane>() {
            if let Some(drawn) = client.lbterm_frame_read() {
                self.lbterm_read.push((Arc::clone(pane), drawn));
            }
            return;
        }
        let Some(under) = self.lbterm_under_overlay(pane) else {
            return;
        };
        let Some(client) = under.downcast_ref::<ClientPane>() else {
            return;
        };
        if let Some(drawn) = client.lbterm_frame_read() {
            self.lbterm_read.push((Arc::clone(&under), drawn));
        }
    }

    /// The pane under `pane` when `pane` is a pane overlay that reads its lines from it: the
    /// overlay given for that pane id (`assign_overlay_for_pane`) reports the same id, which only
    /// copy mode, search and quick select do (overlay/copy.rs's and quickselect.rs's `pane_id`,
    /// their `delegate`'s). The close-pane confirmation has an id of its own, with no overlay
    /// in that id's pane state, so it gives None
    fn lbterm_under_overlay(&self, pane: &Arc<dyn Pane>) -> Option<Arc<dyn Pane>> {
        let pane_id = pane.pane_id();
        if !overlays::reads_under(pane_id, self.lbterm_pane_overlay(pane_id)) {
            return None;
        }
        Mux::get().get_pane(pane_id)
    }

    /// The id the pane overlay over `pane_id` reports, if one is open: looked up without adding
    /// a pane state, which `pane_state` would
    fn lbterm_pane_overlay(&self, pane_id: PaneId) -> Option<PaneId> {
        self.pane_state
            .borrow()
            .get(&pane_id)
            .and_then(|state| state.overlay.as_ref())
            .map(|overlay| overlay.pane.pane_id())
    }

    /// The paint's end: what its frame read, one PanesDrawn a server. A pass that failed part way
    /// leaves what it noted here, said by the paint that ends it, since each pane's push is taken
    /// once as it's noted
    pub(crate) fn lbterm_say_read(&mut self) {
        if self.lbterm_read.is_empty() {
            return;
        }
        let mut read = std::mem::take(&mut self.lbterm_read);
        while let Some((lead, first)) = read.pop() {
            let Some(lead) = lead.downcast_ref::<ClientPane>() else {
                continue;
            };
            let mut panes = vec![first];
            read.retain(|(pane, drawn)| {
                let same = pane
                    .downcast_ref::<ClientPane>()
                    .map_or(false, |client| client.lbterm_same_server(lead));
                if same {
                    panes.push(*drawn);
                }
                !same
            });
            log::trace!(target: "lbterm::drawn", "a frame read {:?}", panes);
            lead.lbterm_say_drawn(panes);
        }
        // The list goes back empty with its room, so the next frame that notes allocates nothing
        self.lbterm_read = read;
    }

    /// `is_pane_visible`, for `mux_pane_output_event`, with what it leaves out: a pane of the open
    /// tab whose frames read another pane in its place, under a tab overlay (`Covered`) or under
    /// a pane overlay of its own (`Overlaid`). A pane under copy mode, search or quick select is
    /// `Drawn`: those read its lines, and the frame notes it (`lbterm_frame_read`). The same
    /// lookups as upstream's, and for a pane of the open tab its pane state looked up, without
    /// adding one
    pub(crate) fn lbterm_pane_shows(&mut self, pane_id: PaneId) -> Shows {
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return Shows::No;
        };
        let tab_overlay = self
            .tab_state(tab.tab_id())
            .overlay
            .as_ref()
            .map(|overlay| overlay.pane.pane_id());
        overlays::shows(
            pane_id,
            tab_overlay,
            || tab.contains_pane(pane_id),
            || self.lbterm_pane_overlay(pane_id),
        )
    }

    /// Pane `pane_id` printed and no frame of this window will read it soon (`mux_pane_output_event`):
    /// under the Conductor's view, under a tab overlay or a pane overlay of its own, or in a
    /// window that can't be seen. Said read now when this window shows it (`shown`: known
    /// already, or else asked of the active tab), so its server doesn't hold its next push for a
    /// frame. A pane with nothing unsaid, or whose server doesn't know PanesDrawn, costs a pane
    /// lookup and two atomic loads (`ClientPane::lbterm_unsaid`)
    pub(crate) fn lbterm_output_unpainted(&mut self, pane_id: PaneId, shown: bool) {
        let mux = Mux::get();
        let Some(pane) = mux.get_pane(pane_id) else {
            return;
        };
        let Some(client) = pane.downcast_ref::<ClientPane>() else {
            return;
        };
        if !client.lbterm_unsaid() {
            return;
        }
        let shown = shown
            || mux
                .get_active_tab_for_window(self.mux_window_id)
                .map_or(false, |tab| tab.contains_pane(pane_id));
        if shown {
            log::trace!(target: "lbterm::drawn", "pane {pane_id}: no frame will draw it soon, said read now");
            client.lbterm_say_now();
        }
    }
}
