//! LBTerm: the context pill (`config.lbterm_context`; lbterm/CONTEXT.md). Lachlan's ask,
//! 2026-11-07: "LBTerm should be able to show me what context files this agent has loaded
//! currently. There should also be a $ amount of how much this agents tab has cost. It should go
//! under the inmail button."
//!
//! A hairline pill on the strip's second row, centred on the tab rail's column under the InMail
//! button, saying what the open tab's Claude conversation has cost at list price: every run of it
//! and its subagents, as Terminal's statusline writes it to `<session>.usage.json` (read by
//! crate::summaries, off this thread). Its list of the files in the conversation's context went
//! with Lachlan's 5a of 2026-11-08 (the context bar shows what fills the context,
//! render/context_bar.rs); since his 7c of 2026-11-09 a press opens the cost pane, what spent
//! those dollars (render/cost.rs). The projects pad sits right of it (render/projects.rs) in a
//! fixed place: the pill's room is kept in a tab without a Claude session too (his 6a).
//!
//! Nothing here reads a file: the paint reads memory.

use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::Element;
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use ::window::RectF;
use config::SrgbaTuple;
use std::rc::Rc;
use wezterm_font::LoadedFont;

/// On the pill before the statusline has said what the conversation cost
pub const UNKNOWN: &str = "$–";

#[derive(Default)]
pub struct ContextState {
    /// What the pill was last built showing
    shown: Option<Shown>,
}

/// What the pill shows: whose conversation, its words, and whether its cost pane is open, which
/// lights it (render/cost.rs)
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    sid: String,
    text: String,
    open: bool,
}

/// The widest dollars the pill says (lbterm_panel::text::dollars: `$99.99`, `$9999`, `$99.9k`),
/// which its kept room is as wide as
const WIDEST: &str = "$00.00";

impl TermWindow {
    /// The pill shows: asked for in the config, the strip holding only the window's buttons beside
    /// the tab panel with its second row, and not while the Conductor's view stands over the tabs
    pub(crate) fn lbterm_context_on(&self) -> bool {
        self.lbterm_context_room_on() && !self.lbterm_view_is_open()
    }

    /// The pill's room is kept on the strip's second row whether it shows or not: what the pill
    /// needs but for the Conductor's view, so the projects pad after it doesn't move as he goes
    /// between a tab with a Claude session and one without, or opens the view (Lachlan's 6a of
    /// 2026-11-09)
    pub(crate) fn lbterm_context_room_on(&self) -> bool {
        self.config.lbterm_context.is_some()
            && self.config.lbterm_tab_panel.enabled
            && config::lbterm::hides_tabs(&self.config)
            && self.config.use_fancy_tab_bar
            && self.show_tab_bar
            && !self.config.tab_bar_at_bottom
            && self.lbterm_prompt_row_on()
    }

    /// How wide the pill's kept room is: the pill laid out with the widest dollars it says (a
    /// layout of a few glyphs each time the strip is built). `None` when no room is kept
    pub(crate) fn lbterm_context_room(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        row: RectF,
    ) -> anyhow::Result<Option<f32>> {
        if !self.lbterm_context_room_on() {
            return Ok(None);
        }
        let white = [1.; 3];
        let kids = vec![super::inbox::pill_text(font, WIDEST.to_string(), white, white)];
        let (_, w) = self.lbterm_strip_pill(font, metrics, surface, row, TabBarItem::None, kids, (white, white), (white, white))?;
        Ok(Some(w))
    }

    /// Each paint, after the strip has found its tab's session (render/strip.rs): true when the
    /// strip must be built again, for other dollars on the pill or another session's
    pub(crate) fn lbterm_context_refresh(&mut self) -> bool {
        let sid = self.lbterm_strip.session.clone().filter(|_| self.lbterm_context_on());
        let shown = sid.map(|sid| {
            let usage = crate::summaries::get(&sid).and_then(|s| s.usage);
            Shown {
                text: usage.map_or_else(|| UNKNOWN.to_string(), |u| lbterm_panel::text::dollars(u.usd)),
                sid,
                open: self.lbterm_cost.is_open(),
            }
        });
        if shown == self.lbterm_context.shown {
            return false;
        }
        self.lbterm_context.shown = shown;
        true
    }

    /// The session whose cost the pill shows, while it shows: the cost pane's (render/cost.rs)
    pub(crate) fn lbterm_context_session(&self) -> Option<&str> {
        self.lbterm_context.shown.as_ref().map(|s| s.sid.as_str())
    }

    /// Whether this window shows what one of `sids` has cost: the pill's session is one, or the
    /// open tab panel has a row for one (crate::summaries repaints only such windows when a
    /// session's cost alone changes)
    pub(crate) fn lbterm_shows_usage(&self, sids: &[String]) -> bool {
        if self.lbterm_context.shown.as_ref().map_or(false, |s| sids.contains(&s.sid)) {
            return true;
        }
        // The context bar's window size comes from the same file (render/context_bar.rs)
        if self.lbterm_context_bar_session().map_or(false, |sid| sids.iter().any(|s| s == sid)) {
            return true;
        }
        if !self.lbterm_panel.hover.is_open() {
            return false;
        }
        mux::Mux::try_get()
            .and_then(|mux| mux.get_window(self.mux_window_id).map(|w| w.iter_tabs().cloned().collect::<Vec<_>>()))
            .map_or(false, |tabs| {
                tabs.iter()
                    .any(|tab| super::tab_panel::tab_session(tab).map_or(false, |sid| sids.contains(&sid)))
            })
    }

    /// The pill for the strip's second row, `row` the strip's bounds, and how wide it is laid out
    /// alone: what the conversation has cost in a hairline pill, which the strip places centred on
    /// the rail's column (render/strip.rs, `lbterm_prompt_row`). Nothing while it doesn't show.
    pub(crate) fn lbterm_context_pill(
        &self,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        surface: SrgbaTuple,
        row: RectF,
    ) -> anyhow::Result<Option<(Element, f32)>> {
        let Some(shown) = self.lbterm_context.shown.as_ref() else {
            return Ok(None);
        };
        let over = |alpha: f32| [surface.0, surface.1, surface.2].map(|c| c + (1. - c) * alpha);
        // It sits back as the InMail button does with nothing waiting, its words a little
        // brighter, since they're always something to read; it lights under the pointer as the
        // projects pad does, since a press opens the cost pane (render/cost.rs), and stays lit
        // while that's open
        let lit = (over(0.50), over(0.92));
        let rest = if shown.open { lit } else { (over(0.14), over(0.70)) };
        let kids = vec![super::inbox::pill_text(font, shown.text.clone(), rest.1, lit.1)];
        self.lbterm_strip_pill(font, metrics, surface, row, TabBarItem::LbtermContext, kids, (rest.0, lit.0), (rest.1, lit.1))
            .map(Some)
    }
}

/// LBTerm: the context pill, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// What the pill says, while it shows
    pub(crate) fn lbterm_state_context(&self) -> Option<String> {
        self.lbterm_context.shown.as_ref().map(|s| s.text.clone())
    }
}
