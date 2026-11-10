//! LBTerm: while a pane its mux server holds hasn't had its push, a new view's panes wait for it,
//! up to a bound, and the rest of the window doesn't (Lachlan's 11a of 2026-11-04, his 1a and 2b
//! of 2026-11-08; when and why: `lbterm_extras::frame_wait`; lbterm/MUX.md, "Tabs out of view:
//! output held at the server").
//!
//! Asked once a paint, at `paint_impl`'s start (`lbterm_frame_panes`): a frame of the view drawn
//! last costs the active tab's lookup and its zoomed pane's, and only a new view (a switch, an
//! unzoom) asks its panes whether they're held, an atomic each (`ClientPane::lbterm_is_held`).
//! The answer is whose panes the frame draws (`FrameTab`): the open tab's, or during the wait the
//! tab drawn last's, as they were, or none when nothing was drawn before it. What draws in the
//! panes' room asks for them here rather than for the open tab's: the panes and their splits
//! (render/paint.rs), the question colours and the voice drafts' cards over them
//! (render/questions.rs, render/draft.rs), the scroll strip, the scrollbar of the screen beside it
//! whose picture fades with the panes (render/scroll_strip.rs, render/sides.rs), and the test
//! window's state file's `panes` (state.rs). The rail, the strip, the cards and every fade read
//! the open tab, as they always do, so their motion starts at the switch's first frame. Input,
//! titles and Lua's pane information read the open tab too: what's typed goes to it at once.

use crate::termwindow::TermWindow;
use ::window::WindowOps;
use lbterm_extras::frame_wait::Frame;
use mux::pane::{Pane, PaneId};
use mux::tab::{PositionedPane, PositionedSplit, Tab, TabId};
use mux::Mux;
use smol::Timer;
use std::sync::Arc;
use std::time::Instant;

/// What a frame shows: the window's tab, and the pane zoomed in it
pub(crate) type View = (TabId, Option<PaneId>);

/// Whose panes a frame draws
#[derive(Default)]
pub(crate) enum FrameTab {
    /// The window's open tab's, as ever
    #[default]
    Open,
    /// The tab drawn last's, in the open tab's place while a pane of it is held at its server
    /// and its push hasn't come (`Frame::Keep`)
    Kept(Arc<Tab>),
    /// None: the wait has nothing drawn before it to keep (the window's first frame, the
    /// Conductor's view just gone from over the panes, the tab drawn last closed or moved away),
    /// so the panes' room shows the window's background until the push or the bound
    Empty,
}

/// Whether a pane `tab` draws is held at its server, its push not yet come
fn held(tab: &Tab) -> bool {
    tab.iter_panes().iter().any(|pos| {
        pos.pane
            .downcast_ref::<wezterm_client::pane::ClientPane>()
            .map_or(false, |client| client.lbterm_is_held())
    })
}

impl TermWindow {
    /// Whose panes the paint about to run draws (the `lbterm_frame_tab` field), decided once at
    /// its start; and during a wait, the window's wake at the bound, armed by the wait's first
    /// frame only (a timer made each paint registers with the reactor each paint: PITFALLS 203)
    pub(crate) fn lbterm_frame_panes(&mut self) {
        self.lbterm_frame_tab = self.lbterm_frame_tab_now();
    }

    fn lbterm_frame_tab_now(&mut self) -> FrameTab {
        // The Conductor's view covers the panes: what's under it is drawn as it closes
        if self.lbterm_view_is_open() {
            self.lbterm_frame_wait.forget();
            return FrameTab::Open;
        }
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return FrameTab::Open;
        };
        let view = (tab.tab_id(), tab.get_zoomed_pane().map(|pane| pane.pane_id()));
        let now = Instant::now();
        let (until, first) = match self
            .lbterm_frame_wait
            .frame(view, now, config::lbterm::SWITCH_WAIT, || held(&tab))
        {
            Frame::Draw => return FrameTab::Open,
            Frame::Keep { until, first } => (until, first),
        };
        if first || self.lbterm_frame_wake != Some(until) {
            self.lbterm_frame_wake = Some(until);
            if let Some(window) = self.window.clone() {
                promise::spawn::spawn(async move {
                    lbterm_extras::counters::timer(Timer::at(until)).await;
                    window.invalidate();
                })
                .detach();
            }
        }
        // The tab drawn last, while it's still this window's
        let kept = self.lbterm_frame_wait.drawn().map(|(id, _)| *id).and_then(|id| {
            mux.get_window(self.mux_window_id)?.get_tab_idx_for_id(id)?;
            mux.get_tab(id)
        });
        if first {
            log::debug!(
                target: "lbterm::shown",
                "tab {}: its panes wait for a held pane's push, up to {:?}, tab {:?}'s in their place",
                tab.tab_id(),
                until.saturating_duration_since(now),
                kept.as_ref().map(|tab| tab.tab_id())
            );
        }
        match kept {
            Some(tab) => FrameTab::Kept(tab),
            None => FrameTab::Empty,
        }
    }

    /// Whether this frame draws another tab's panes, or none, in the open tab's place
    pub(crate) fn lbterm_panes_kept(&self) -> bool {
        !matches!(self.lbterm_frame_tab, FrameTab::Open)
    }

    /// The tab whose panes this frame draws, as the state file says it: None for none
    pub(crate) fn lbterm_painted_tab_id(&self) -> Option<TabId> {
        match &self.lbterm_frame_tab {
            FrameTab::Open => Mux::get()
                .get_active_tab_for_window(self.mux_window_id)
                .map(|tab| tab.tab_id()),
            FrameTab::Kept(tab) => Some(tab.tab_id()),
            FrameTab::Empty => None,
        }
    }

    /// The panes this frame draws, where they go: `get_panes_to_render`'s, but the kept tab's
    /// during a wait
    pub(crate) fn lbterm_panes_to_paint(&self) -> Vec<PositionedPane> {
        match &self.lbterm_frame_tab {
            FrameTab::Open => self.get_panes_to_render(),
            FrameTab::Kept(tab) => self.get_pos_panes_for_tab(tab),
            FrameTab::Empty => vec![],
        }
    }

    /// The pane this frame draws as active: `get_active_pane_or_overlay`'s, but the kept tab's
    /// during a wait
    pub(crate) fn lbterm_pane_to_paint(&self) -> Option<Arc<dyn Pane>> {
        match &self.lbterm_frame_tab {
            FrameTab::Open => self.get_active_pane_or_overlay(),
            FrameTab::Kept(tab) => {
                if let Some(overlay) = self.tab_state(tab.tab_id()).overlay.as_ref() {
                    return Some(overlay.pane.clone());
                }
                let pane = tab.get_active_pane()?;
                let overlay = self
                    .pane_state(pane.pane_id())
                    .overlay
                    .as_ref()
                    .map(|overlay| overlay.pane.clone());
                overlay.or(Some(pane))
            }
            FrameTab::Empty => None,
        }
    }

    /// The splits this frame draws between its panes: `get_splits`', but the kept tab's during a
    /// wait
    pub(crate) fn lbterm_splits_to_paint(&mut self) -> Vec<PositionedSplit> {
        let kept = match &self.lbterm_frame_tab {
            FrameTab::Open => None,
            FrameTab::Kept(tab) => Some(Arc::clone(tab)),
            FrameTab::Empty => return vec![],
        };
        match kept {
            None => self.get_splits(),
            Some(tab) if self.tab_state(tab.tab_id()).overlay.is_some() => vec![],
            Some(tab) => tab.iter_splits(),
        }
    }
}
