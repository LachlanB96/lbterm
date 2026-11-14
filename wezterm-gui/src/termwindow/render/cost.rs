//! LBTerm: the cost pane (lbterm/CONTEXT.md, "The cost pane"; Lachlan's 7c of 2026-11-09). A
//! press on the context pill opens it, a pane hanging under the strip as wide as the panes under
//! it, fading in as the objectives' cards do (the fifth of their visuals, `Target::Objectives(4)`):
//! what the open tab's Claude conversation has cost at list price, split by what spent it (his
//! turns, subagents, workflows, and the compactions, which Claude Code records no cost for), the
//! token types under each, and a graph of the cost over time with a mark where each subagent and
//! workflow started. A press on the pill again closes it, and so does the pointer leaving the pill
//! and the pane for `COST_CLOSE_MS`; resting on the pill opens nothing (his rule: the press).
//!
//! The split is crate::cost's, read or counted on its own thread and read here from memory, and
//! asked for only when what it's made from may have moved: with the count off, when its file was
//! written since it was last asked for (crate::cost::want decides, and a write while the pane is
//! open asks without a frame); with the count on, as the pane opens and when the pill's dollars
//! move while it's open. An answer the same as the one kept repaints nothing. The pane is laid out
//! (lbterm_extras::cost::pane) only when the split, the session or its place changes, and drawn
//! from what was laid out each paint while it shows. So an opening where nothing moved is the
//! press's frame and no other (round 5's stage 2; lbterm/FLOWS.md, the cost pane).
//!
//! Whose split it is, `lbterm_context.count_cost` says (off unless asked for; Lachlan's 2b of
//! 2026-11-10): off, the Claude project's (`@Claude:cost`, `<session>.cost.json`), laid out as the
//! own count is; while it isn't there to show, what the pill has from `<session>.usage.json`
//! (lbterm_extras::cost::usage_pane, its dollars and context window) and why. On, LBTerm's own
//! count.

use crate::cost::Counted;
use mux::window::WindowId as MuxWindowId;
use crate::tabbar::TabBarItem;
use crate::termwindow::render::objectives::{over, CARD, NAME_ALPHA, TEXT_ALPHA, TITLE_ALPHA};
use crate::termwindow::render::tab_panel::srgb;
use crate::termwindow::{TermWindowNotif, UIItemType};
use ::window::{CursorIcon, MouseEvent, MouseEventKind as WMEK, MousePress, WindowOps};
use config::SrgbaTuple;
use lbterm_extras::cost::{self, Cost, Pane, Spender, Tone};
use lbterm_extras::strip::{self as strip_card, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_motion::PixelRect;
use lbterm_panel::{Hover, Phase};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use window::color::LinearRgba;

/// Which of the objectives' cards this is (render/objectives.rs, `CARD`)
const WHICH: usize = 4;
/// The graph's grid: white this far over the pane
const GRID_ALPHA: f32 = 0.10;

pub struct CostState {
    /// Opened only by a press (`open_now`); while it's open its grace closes it
    hover: Hover,
    /// The pill, as the strip last laid it out
    zone: Option<Rect>,
    /// The pane as drawn while open, and the last one drawn, which a closing pane fades showing
    pane: Option<Rc<Pane>>,
    last: Option<Rc<Pane>>,
    /// What the pane was laid out from
    laid: Option<(LaidFrom, Rc<Pane>)>,
    /// With the count on, the session and the pill's dollars the count was last asked for with,
    /// while the pane is open: shut (or the count off), none, so it asks as it next opens, since
    /// nothing hears the transcripts meanwhile (the thread answers nothing when none moved). With
    /// it off crate::cost::want keeps what each split was asked for at
    counting: Option<(String, Option<u64>)>,
    /// The window and the session its pane is open on with the count off, as crate::cost::showing
    /// was last told: a write of that split asks for it and its answer repaints this window
    split: Option<(MuxWindowId, String)>,
    visual: Option<(PixelRect, bool)>,
    wake: Option<Instant>,
    pressed: bool,
    on_pill: bool,
}

impl Default for CostState {
    fn default() -> Self {
        let grace = Duration::from_millis(config::lbterm::COST_CLOSE_MS);
        Self {
            hover: Hover::new(grace, grace),
            zone: None,
            pane: None,
            last: None,
            laid: None,
            counting: None,
            split: None,
            visual: None,
            wake: None,
            pressed: false,
            on_pill: false,
        }
    }
}

/// What a pane was laid out from: the session, its split (by address) or why there's none, and
/// where it goes (left, top, width, a column and a line, the scale, the clock's offset)
struct LaidFrom {
    sid: String,
    counted: Option<CountedAt>,
    geometry: [u64; 7],
}

enum CountedAt {
    /// LBTerm's own count
    Cost(Arc<Cost>),
    Failed(Arc<str>),
    /// The Claude project's split
    Split(Arc<Cost>),
    /// The count off and no split from the Claude project: why, and what `usage.json` says, the
    /// dollars in cents and the window in tokens
    Usage(Arc<str>, Option<u64>, Option<u64>),
}

impl PartialEq for LaidFrom {
    fn eq(&self, other: &Self) -> bool {
        let same = match (&self.counted, &other.counted) {
            (None, None) => true,
            (Some(CountedAt::Cost(a)), Some(CountedAt::Cost(b))) => Arc::ptr_eq(a, b),
            (Some(CountedAt::Split(a)), Some(CountedAt::Split(b))) => Arc::ptr_eq(a, b),
            (Some(CountedAt::Failed(a)), Some(CountedAt::Failed(b))) => a == b,
            (Some(CountedAt::Usage(a, c, w)), Some(CountedAt::Usage(b, d, v))) => (a, c, w) == (b, d, v),
            _ => false,
        };
        same && self.sid == other.sid && self.geometry == other.geometry
    }
}

impl CostState {
    pub(crate) fn visual(&self) -> Option<(PixelRect, bool)> {
        self.visual
    }

    pub(crate) fn is_open(&self) -> bool {
        self.hover.is_open()
    }

    fn reset(&mut self) {
        self.hover.close();
        (self.zone, self.pane, self.visual) = (None, None, None);
        self.on_pill = false;
        self.follow_split(None);
    }

    /// Tell crate::cost which session's split this window's pane follows (`None`: none) when that
    /// changes
    fn follow_split(&mut self, split: Option<(MuxWindowId, &str)>) {
        let same = match (&self.split, split) {
            (Some((w, s)), Some((window, sid))) => *w == window && s == sid,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some((window, _)) = self.split.take() {
            crate::cost::showing(window, None);
        }
        if let Some((window, sid)) = split {
            crate::cost::showing(window, Some(sid));
            self.split = Some((window, sid.to_string()));
        }
    }
}

impl Drop for CostState {
    /// The window's gone: no split is asked for for it
    fn drop(&mut self) {
        self.follow_split(None);
    }
}

/// A tone's colour on `surface`
fn colour(tone: Tone, surface: SrgbaTuple) -> LinearRgba {
    match tone {
        Tone::Total => over(surface, TITLE_ALPHA),
        Tone::Text => over(surface, TEXT_ALPHA),
        Tone::Dim => over(surface, NAME_ALPHA),
        Tone::Part(s) => srgb(s.colour()),
        Tone::Compactions => srgb(cost::COMPACTIONS),
        Tone::Grid => over(surface, GRID_ALPHA),
        Tone::Line => srgb(Spender::Turns.colour()),
    }
}

impl crate::TermWindow {
    /// Each paint, after the tab bar has laid out the pill: the pane while it's open or fading.
    /// Nothing while the pill doesn't show (no Claude session in the tab, the pill off)
    pub(crate) fn paint_lbterm_cost(&mut self) -> anyhow::Result<()> {
        let pill = self
            .ui_items
            .iter()
            .find(|i| i.item_type == UIItemType::TabBar(TabBarItem::LbtermContext))
            .map(|i| Rect { x: i.x as f64, y: i.y as f64, w: i.width as f64, h: i.height as f64 });
        let sid = self.lbterm_context_session().map(str::to_string);
        let (Some(pill), Some(sid)) = (pill, sid) else {
            self.lbterm_cost.reset();
            return Ok(());
        };
        let motion = self.lbterm_motion();
        let composited = self.lbterm_composited();
        let closing = if composited && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        self.lbterm_cost.hover.set_motion(Duration::ZERO, closing);
        self.lbterm_cost.zone = Some(pill);
        let open = self.lbterm_cost.hover.is_open();
        let on = config::lbterm::counts_cost(&self.config);
        if !open || !on {
            // The count asked for again as it next opens
            self.lbterm_cost.counting = None;
        }
        // Open with the count off, it follows its split's own writes (crate::cost::split_written)
        let window_id = self.mux_window_id;
        self.lbterm_cost.follow_split((open && !on).then_some((window_id, sid.as_str())));
        let scale = self.dimensions.dpi as f64 / 96.0;
        let surface = self.palette().background;
        let (fading, settles) = if composited {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or((false, None), |c| {
                    let (showing, settles) = c.objectives_showing(Instant::now());
                    (showing[WHICH], settles)
                })
        } else {
            (false, None)
        };
        let pane = match strip_card::card_draw(open, fading) {
            CardDraw::Fresh => {
                let usage = crate::summaries::get(&sid).and_then(|s| s.usage);
                let usd = usage.map(|u| (u.usd * 100.0).round() as u64);
                if on {
                    // Counted again as it opens, and when the pill's dollars move while it's open
                    if self.lbterm_cost.counting.as_ref() != Some(&(sid.clone(), usd)) {
                        crate::cost::want(&self.config, &sid, self.mux_window_id);
                        self.lbterm_cost.counting = Some((sid.clone(), usd));
                    }
                } else {
                    // The Claude project's split: asked for only when its file was written since
                    // it was last asked for, so an opening on one that wasn't asks nothing
                    crate::cost::want(&self.config, &sid, self.mux_window_id);
                }
                let counted = crate::cost::get(&sid, on).map(|c| match c {
                    Counted::Cost(c) => CountedAt::Cost(c),
                    Counted::Failed(why) => CountedAt::Failed(why),
                    Counted::Split(c) => CountedAt::Split(c),
                    Counted::Unpublished(why) => CountedAt::Usage(why, usd, usage.and_then(|u| u.window)),
                });
                let (font, metrics, cell) = self.lbterm_objectives_font()?;
                let room = self.lbterm_room()?;
                let gutter = self.lbterm_shape().gutter() as f64;
                // Under the strip, from the panes' left (the closed rail is over anything left of
                // it, PITFALLS 218) to a gutter short of their right: as wide as what's under it
                let top = self.get_os_border().top.get() as f64 + self.tab_bar_pixel_height()? as f64 + 2.0 * scale;
                let left = room.left as f64;
                let width = (room.right as f64 - gutter - left).max(0.0).round();
                let offset = chrono::Local::now().offset().local_minus_utc() as i64;
                let from = LaidFrom {
                    sid: sid.clone(),
                    counted,
                    geometry: [left, top, width, cell.0, cell.1, scale, offset as f64].map(f64::to_bits),
                };
                match self.lbterm_cost.laid.as_ref().filter(|(was, _)| *was == from) {
                    Some((_, pane)) => Some(Rc::clone(pane)),
                    None => {
                        // Its words that have no more room than themselves are measured as the
                        // words are laid out to be drawn, once a layout
                        let measure = |text: &str| self.lbterm_context_width(&font, &metrics, text);
                        let pane = Rc::new(match &from.counted {
                            Some(CountedAt::Cost(c) | CountedAt::Split(c)) => cost::pane(Some(c), left, top, width, cell, scale, offset, &measure),
                            Some(CountedAt::Failed(why)) => cost::note(why, left, top, width, cell),
                            Some(CountedAt::Usage(why, cents, window)) => {
                                cost::usage_pane(cents.map(|c| c as f64 / 100.0), *window, why, left, top, width, cell, &measure)
                            }
                            None if on => cost::pane(None, left, top, width, cell, scale, offset, &measure),
                            None => cost::note(cost::READING, left, top, width, cell),
                        });
                        self.lbterm_cost.laid = Some((from, Rc::clone(&pane)));
                        Some(pane)
                    }
                }
            }
            CardDraw::Last => self.lbterm_cost.last.clone(),
            CardDraw::None => None,
        };
        let window = (self.dimensions.pixel_width as u32, self.dimensions.pixel_height as u32);
        let state = &mut self.lbterm_cost;
        state.visual = pane.as_ref().and_then(|p| {
            strip_card::visual_bounds(&p.rect, scale, window).map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open))
        });
        state.pane = if open { pane.clone() } else { None };
        if let Some(pane) = pane {
            self.lbterm_cost_draw(&pane, surface, scale)?;
            self.lbterm_cost.last = Some(pane);
        }
        // A fade's end wakes it only while it closes, when the last pane drawn goes. Open, nothing
        // drawn changes at its end, and the wake was a frame drawing nothing new whenever a paint
        // came while it faded in (the count's answer did each opening until round 5's stage 2)
        if !open {
            self.lbterm_cost_wake(settles);
        }
        let due = self.lbterm_cost.hover.deadline();
        self.lbterm_cost_wake(due);
        Ok(())
    }

    fn lbterm_cost_draw(&self, pane: &Pane, surface: SrgbaTuple, scale: f64) -> anyhow::Result<()> {
        let shape = self.lbterm_shape();
        let (radius, stroke) = (shape.radius() as f64, shape.hairline() as f64);
        self.lbterm_objectives_card(WHICH, pane.rect, surface, radius, stroke, scale)?;
        {
            let gl_state = self.render_state.as_ref().unwrap();
            let layer = gl_state.layer_for_zindex(CARD[WHICH][1])?;
            let mut quads = layer.quad_allocator();
            for fill in &pane.fills {
                let r = if fill.round { fill.rect.w / 2.0 } else { 0.0 };
                self.lbterm_menu_rounded(&mut quads, fill.rect, r, colour(fill.tone, surface))?;
            }
        }
        let words: Vec<(f64, f64, f64, String, LinearRgba)> =
            pane.words.iter().map(|w| (w.x, w.y, w.w, w.text.clone(), colour(w.tone, surface))).collect();
        self.lbterm_objectives_words(WHICH, &words)
    }

    /// The config was read again: with the count turned on or off, the thread ends and what it
    /// answered goes (crate::cost::configured, which also forgets what each split was asked for
    /// at), so an open pane asks the new way at its next paint and is laid out again from that
    /// answer
    pub(crate) fn lbterm_cost_configure(&mut self) {
        crate::cost::configured(&self.config);
        self.lbterm_cost.counting = None;
    }

    /// Repaint at `at`: when the grace is up, or the pane has faded out
    fn lbterm_cost_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        if matches!(self.lbterm_cost.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_cost.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_cost.wake == Some(at) {
                    tw.lbterm_cost.wake = None;
                }
                tw.lbterm_cost.hover.tick(Instant::now());
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// The pointer moved, pressed or released: a left press on the pill opens the pane, or closes
    /// it when it's open; while it's open the pointer leaving the pill and the pane closes it after
    /// its grace. True when it took the event: the pill's and the pane's, so nothing under the
    /// pane gets a click (mouseevent.rs, before the projects pad and the strip's own items)
    pub(crate) fn lbterm_cost_mouse(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let busy = self.current_mouse_capture.is_some() || self.window_drag_position.is_some() || self.dragging.is_some();
        let state = &mut self.lbterm_cost;
        let Some(zone) = state.zone else {
            return false;
        };
        if let WMEK::Release(_) = event.kind {
            if std::mem::take(&mut state.pressed) {
                return true;
            }
        }
        let (x, y) = (event.coords.x as f64, event.coords.y as f64);
        let on_pill = !busy && zone.contains(x, y);
        let on_pane = !busy && state.pane.as_ref().map_or(false, |p| p.rect.contains(x, y));
        let now = Instant::now();
        // Only an open pane hears where the pointer is: resting on the pill opens nothing
        let mut changed = matches!(state.hover.phase(), Phase::Open | Phase::Closing) && state.hover.pointer(on_pill || on_pane, now);
        if on_pill != state.on_pill {
            state.on_pill = on_pill;
            changed = true;
        }
        if let (WMEK::Press(MousePress::Left), true) = (&event.kind, on_pill) {
            if state.hover.is_open() {
                state.hover.close();
            } else {
                state.hover.open_now(now);
            }
            changed = true;
        }
        if changed {
            context.lbterm_invalidate();
        }
        let due = self.lbterm_cost.hover.deadline();
        self.lbterm_cost_wake(due);
        if !(on_pill || on_pane) {
            return false;
        }
        match event.kind {
            WMEK::Press(_) => self.lbterm_cost.pressed = true,
            WMEK::Move => context.set_cursor(Some(if on_pill { CursorIcon::Pointer } else { CursorIcon::Default })),
            _ => {}
        }
        true
    }

    /// The pointer left the window: an open pane closes after its grace
    pub(crate) fn lbterm_cost_mouse_leave(&mut self) {
        let state = &mut self.lbterm_cost;
        let mut changed = std::mem::take(&mut state.on_pill);
        if matches!(state.hover.phase(), Phase::Open | Phase::Closing) {
            changed |= state.hover.left_window(Instant::now());
        }
        if changed {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        let due = self.lbterm_cost.hover.deadline();
        self.lbterm_cost_wake(due);
    }

    /// The pane, for the state file (lbterm/STATE.md, `cost`)
    pub(crate) fn lbterm_state_cost(&self) -> Option<lbterm_extras::state::CostPane> {
        use lbterm_extras::state as S;
        let at = |r: &Rect| S::Rect::round(r.x, r.y, r.w, r.h);
        let state = &self.lbterm_cost;
        let zone = state.zone?;
        let pane = state.pane.as_ref();
        let (cost, source) = match state.laid.as_ref().map(|(from, _)| &from.counted) {
            Some(Some(CountedAt::Cost(c))) if pane.is_some() => (Some(Arc::clone(c)), Some("count")),
            Some(Some(CountedAt::Split(c))) if pane.is_some() => (Some(Arc::clone(c)), Some("claude")),
            _ => (None, None),
        };
        let rows = match (&cost, pane) {
            (Some(c), Some(p)) => Spender::ALL
                .iter()
                .zip(&p.rows)
                .map(|(s, rect)| {
                    let part = c.part(*s);
                    S::CostRow {
                        spender: s.name().to_string(),
                        runs: part.runs,
                        usd: part.usd.total(),
                        input: part.tokens.input,
                        writes: part.tokens.writes,
                        reads: part.tokens.reads,
                        output: part.tokens.output,
                        rect: at(rect),
                    }
                })
                .collect(),
            _ => vec![],
        };
        Some(S::CostPane {
            pill: at(&zone),
            count: config::lbterm::counts_cost(&self.config),
            source,
            open: pane.is_some(),
            rect: pane.map(|p| at(&p.rect)),
            note: pane.and_then(|p| p.note.clone()),
            total: cost.as_ref().map(|c| c.total()),
            rows,
            compactions: cost.as_ref().map_or(0, |c| c.compactions.len()),
            compacted: cost.as_ref().map_or(vec![], |c| c.compactions.iter().map(|k| k.label()).collect()),
            graph: pane.and_then(|p| p.graph.as_ref().map(at)),
            steps: cost.as_ref().map_or(0, |c| c.steps.len()),
            marks: cost.as_ref().map_or(vec![], |c| c.marks.iter().map(|m| m.spender.name().to_string()).collect()),
            words: pane.map_or(vec![], |p| p.words.iter().map(|w| w.text.clone()).collect()),
        })
    }

    /// What of the pill or the pane is at `x`, `y`, for the state file's pointer
    pub(crate) fn lbterm_cost_under(&self, x: f64, y: f64) -> Option<&'static str> {
        let state = &self.lbterm_cost;
        if state.pane.as_ref().map_or(false, |p| p.rect.contains(x, y)) {
            return Some("cost.pane");
        }
        None
    }

    /// The pane's grace under way at `now`
    pub(crate) fn lbterm_cost_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let hover = &self.lbterm_cost.hover;
        if !matches!(hover.phase(), Phase::Closing) {
            return vec![];
        }
        hover
            .deadline()
            .map(|due| lbterm_extras::state::Wait {
                name: "cost.close".to_string(),
                left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
            })
            .into_iter()
            .collect()
    }
}
