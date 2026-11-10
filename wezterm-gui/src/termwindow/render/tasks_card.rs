//! LBTerm: what a tab's Claude session has running (lbterm/TAB-PANEL.md, "What's running").
//! Resting the pointer on a row of the open tab panel whose session still has subagents,
//! background commands or Monitors going drops a card beside the row listing each one: its
//! kind, its description and how long it's been running, and for a subagent what it's doing
//! now (its latest tool call), its tool calls and its tokens.
//!
//! The list is Terminal's statusline's (`<sid>.tasks.json`, read with the session's summaries
//! off this thread: crate::summaries). A subagent's progress comes from its own transcript,
//! read only while the card is open, on a thread of its own, whenever it changes
//! (lbterm_extras::tasks::Transcripts). What goes where is lbterm_extras::tasks's. The card
//! paints when what it says changes, and when an age it shows reads differently (each second for
//! a task under a minute old, else each minute), and not otherwise.
//!
//! The card is drawn in z-layers of its own, the strip card's way: with the compositor they're
//! a visual of their own, over the panel and under the tab menu, which DWM fades in on
//! `pane_enter` as the card opens and out on `pane_exit` as it closes, from wherever it is; the
//! card that's going is drawn as it was until it has faded. Without it the card comes and goes
//! at once. It's read only: a click on it does nothing, and it gives way to a menu.

use crate::termwindow::render::tab_panel::tab_session;
use crate::termwindow::TermWindowNotif;
use crate::utilsprites::RenderMetrics;
use ::window::WindowOps;
use config::SrgbaTuple;
use lbterm_extras::strip::{self, CardDraw};
use lbterm_extras::tabs::Rect;
use lbterm_extras::tasks::{self, Card, Tone, Transcripts};
use lbterm_motion::PixelRect;
use lbterm_panel::Hover;
use mux::tab::TabId;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};
use window::color::LinearRgba;

/// The card's shadow, the card, and its words. With the compositor they're the card's visual's
/// alone (`Target::TasksCard`), which DWM fades. Without it they're over the panel and the menus;
/// the card closes while a menu is open.
const SHADOW_Z: i8 = 80;
const CARD_Z: i8 = 81;
const TEXT_Z: i8 = 82;

/// Whether a z-layer is the card's
pub fn owns_zindex(zindex: i8) -> bool {
    (SHADOW_Z..=TEXT_Z).contains(&zindex)
}

/// The pointer rests on a row this long before its card drops, so one running down the rows
/// doesn't fling a card open at each; and it closes this long after it leaves the row and the
/// card. The strip's card's.
const OPEN_DELAY: Duration = Duration::from_millis(300);
const CLOSE_DELAY: Duration = Duration::from_millis(200);

/// White over the card, as the eye sees it: the strip card's labels and text
const LABEL_ALPHA: f32 = 0.50;
const TEXT_ALPHA: f32 = 0.85;
const AGE_ALPHA: f32 = 0.50;
const DETAIL_ALPHA: f32 = 0.60;
const MORE_ALPHA: f32 = 0.50;
/// The card's hairline: the outline on hover
const RING_ALPHA: f32 = 0.14;

pub struct TasksCardState {
    hover: Hover,
    /// The card's words as its last frame laid them out (render/words.rs)
    pub(crate) words: std::cell::RefCell<super::words::Words>,
    /// The tab the card shows: the last row with tasks the pointer was on, kept while the
    /// card waits out its grace
    shown: Option<TabId>,
    /// The card while it's open, as last drawn: what the pointer can be on
    card: Option<Card>,
    /// The card last drawn, which a closing card fades out showing
    last_card: Option<Card>,
    /// Where the card's visual goes this frame, and whether it's opening
    visual: Option<(PixelRect, bool)>,
    /// When a repaint is already on its way
    wake: Option<Instant>,
    /// How wide the open card was last drawn, and for which tab: it doesn't get narrower while
    /// it stays open on that tab (tasks::widen)
    width: Option<(TabId, f64)>,
    /// What the open card lists, with a state file (lbterm/STATE.md)
    said: Option<lbterm_extras::state::TasksCard>,
}

impl Default for TasksCardState {
    fn default() -> Self {
        Self {
            hover: Hover::new(OPEN_DELAY, CLOSE_DELAY),
            words: Default::default(),
            shown: None,
            card: None,
            last_card: None,
            visual: None,
            wake: None,
            width: None,
            said: None,
        }
    }
}

/// The subagents' transcripts, read on a thread of their own and shared by every window. When
/// one asked for says more, the windows with the card open draw again: the others show nothing of
/// it (the store goes on following a transcript a while after the card that asked closes)
fn transcripts() -> &'static Transcripts {
    static STORE: OnceLock<Transcripts> = OnceLock::new();
    STORE.get_or_init(|| {
        Transcripts::start(|| {
            promise::spawn::spawn_into_main_thread(async move {
                let Some(fe) = crate::frontend::try_front_end() else {
                    return;
                };
                for win in fe.gui_windows() {
                    win.window.notify(TermWindowNotif::Apply(Box::new(|tw| {
                        if tw.lbterm_tasks.card.is_some() {
                            if let Some(window) = tw.window.as_ref() {
                                window.invalidate();
                            }
                        }
                    })));
                }
            })
            .detach();
        })
    })
}

fn over(surface: SrgbaTuple, alpha: f32) -> LinearRgba {
    let c = |v: f32| v + (1. - v) * alpha;
    SrgbaTuple(c(surface.0), c(surface.1), c(surface.2), 1.).to_linear()
}

fn unix_ms(t: SystemTime) -> f64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

impl crate::TermWindow {
    /// Whether `x`, `y` is on the open card
    pub(crate) fn lbterm_tasks_card_at(&self, x: f64, y: f64) -> bool {
        self.lbterm_tasks
            .card
            .as_ref()
            .map_or(false, |card| card.rect.contains(x, y))
    }

    /// The pointer moved over the tab panel (render/tab_panel.rs): `hovered` the row it's on,
    /// `on_card` whether it's on the open card. A row whose tab has tasks listed opens its card
    /// once the pointer has rested on it, or shows its own at once if a card is already open;
    /// the card stays while the pointer is on such a row or on the card.
    pub(crate) fn lbterm_tasks_pointer(
        &mut self,
        hovered: Option<usize>,
        on_card: bool,
        now: Instant,
        context: &dyn WindowOps,
    ) {
        let row_tab = hovered.and_then(|i| self.lbterm_panel.tasks_rows.get(i).copied().flatten());
        let state = &mut self.lbterm_tasks;
        let mut changed = false;
        if let Some(tab) = row_tab {
            changed |= state.shown.replace(tab) != Some(tab);
        }
        changed |= state.hover.pointer(row_tab.is_some() || on_card, now);
        if changed {
            context.lbterm_invalidate();
        }
        self.lbterm_tasks_wake(self.lbterm_tasks.hover.deadline());
    }

    /// The pointer left the window: the card closes as if it had left the row
    pub(crate) fn lbterm_tasks_mouse_leave(&mut self) {
        if self.lbterm_tasks.hover.left_window(Instant::now()) {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
        self.lbterm_tasks_wake(self.lbterm_tasks.hover.deadline());
    }

    /// Repaint at `at`: the card's rest or grace is up, an age moves on, or a fade is done
    fn lbterm_tasks_wake(&mut self, at: Option<Instant>) {
        let Some(at) = at else {
            return;
        };
        // One already due sooner repaints, and the paint asks again
        if matches!(self.lbterm_tasks.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_tasks.wake = Some(at);
        promise::spawn::spawn(async move {
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                // One a sooner wake took the place of repaints nothing: that one's paint asked
                // for whatever was still to come
                if tw.lbterm_tasks.wake != Some(at) {
                    return;
                }
                tw.lbterm_tasks.wake = None;
                tw.lbterm_tasks.hover.tick(Instant::now());
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }

    /// The card beside its row, after the tab panel has painted this frame: laid out afresh
    /// while it's open, and with the compositor drawn as it was while its visual fades out
    pub(crate) fn paint_lbterm_tasks_card(&mut self) -> anyhow::Result<()> {
        let now = Instant::now();
        self.lbterm_tasks.said = None;
        let composited = self.lbterm_composited();
        let motion = self.lbterm_motion();
        // Coming back while it fades out opens it again at once (lbterm_panel::hover)
        let closing = if composited && motion.enabled {
            Duration::from_secs_f64(motion.exit.duration())
        } else {
            Duration::ZERO
        };
        self.lbterm_tasks.hover.set_motion(Duration::ZERO, closing);
        self.lbterm_tasks.hover.tick(now);

        // It goes with the open panel and its tab's tasks, and gives way to a menu
        let row = self
            .lbterm_tasks
            .shown
            .and_then(|tab| self.lbterm_panel.tasks_rows.iter().position(|t| *t == Some(tab)));
        let menu = self.lbterm_tab_menu_card().is_some() || self.lbterm_conductor_menu_card().is_some();
        if !self.lbterm_panel.hover.is_open() || menu || row.is_none() {
            self.lbterm_tasks.hover.close();
        }
        let laid = match row.filter(|_| self.lbterm_tasks.hover.is_open()) {
            Some(row) => self.lbterm_tasks_lay_out(row)?,
            None => None,
        };
        // Open with nothing to list any more (its tasks ended): shut. Still resting, it waits on
        if laid.is_none() && self.lbterm_tasks.hover.is_open() {
            self.lbterm_tasks.hover.close();
        }
        // No narrower than a moment ago while it stays open on the same tab
        let max_right = self.lbterm_tasks_max_right();
        let laid = laid.map(|(mut card, next)| {
            let tab = self.lbterm_tasks.shown;
            if let Some((was, width)) = self.lbterm_tasks.width {
                if Some(was) == tab {
                    tasks::widen(&mut card, width, max_right);
                }
            }
            self.lbterm_tasks.width = tab.map(|tab| (tab, card.rect.w));
            (card, next)
        });
        if laid.is_none() {
            self.lbterm_tasks.width = None;
            self.lbterm_budget.borrow_mut().tasks_ticking = 0;
        }
        let open = laid.is_some();
        let (fading, settles) = if composited {
            self.webgpu
                .as_ref()
                .and_then(|w| w.compositor.as_ref())
                .map_or((false, None), |c| c.tasks_card_showing(now))
        } else {
            (false, None)
        };
        let (card, next) = match strip::card_draw(open, fading) {
            CardDraw::Fresh => match laid {
                Some((card, next)) => (Some(card), next),
                None => (None, None),
            },
            CardDraw::Last => (self.lbterm_tasks.last_card.clone(), None),
            CardDraw::None => (None, None),
        };

        let scale = self.dimensions.dpi as f64 / 96.0;
        let window = (
            self.dimensions.pixel_width as u32,
            self.dimensions.pixel_height as u32,
        );
        self.lbterm_tasks.visual = card.as_ref().and_then(|c| {
            strip::visual_bounds(&c.rect, scale, window)
                .map(|(x, y, w, h)| (PixelRect::new(x, y, w, h), open))
        });
        // Only an open card takes the pointer
        self.lbterm_tasks.card = if open { card.clone() } else { None };
        if let Some(card) = card {
            let surface = self.palette().background;
            self.lbterm_tasks_draw(&card, surface)?;
            if open && crate::state_probe::on() {
                self.lbterm_tasks.said = row.map(|row| said(row, &card));
            }
            self.lbterm_tasks.last_card = Some(card);
        }

        let due = [
            self.lbterm_tasks.hover.deadline(),
            next,
            if !open && fading { settles } else { None },
        ]
        .iter()
        .flatten()
        .min()
        .copied();
        // The card's ages repaint at rest on purpose (termwindow/budget.rs), each at its minute
        // or second, and its wake is nothing else's: a subagent's transcript growing repaints
        // by itself (lbterm/PERF.md, finding 15)
        let aged = self.lbterm_budget.borrow().tasks_aged;
        if let Some(at) = next.filter(|at| open && Some(*at) == due && Some(*at) == aged) {
            let per_second = self.lbterm_budget.borrow().clock_rate();
            self.lbterm_budget_tick(lbterm_extras::budget::Purpose::Clock, at, per_second);
        }
        self.lbterm_tasks_wake(due);
        Ok(())
    }

    /// The card for row `row`'s tab, beside the row on the open panel, and when it next needs
    /// another look: an age moving on. `None` when
    /// the tab's session has nothing listed any more.
    fn lbterm_tasks_lay_out(&self, row: usize) -> anyhow::Result<Option<(Card, Option<Instant>)>> {
        let Some(layout) = self.lbterm_panel.layout else {
            return Ok(None);
        };
        let Some(tab) = self.tab_panel_rail().get(row).cloned() else {
            return Ok(None);
        };
        let Some(list) = tab_session(&tab)
            .and_then(|sid| crate::summaries::get(&sid))
            .and_then(|s| s.tasks.clone())
        else {
            return Ok(None);
        };
        let store = transcripts();
        let (shown, age_change) = tasks::shown(&list, |p| store.get(p), unix_ms(SystemTime::now()));
        let scale = self.dimensions.dpi as f64 / 96.0;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let cell = (
            font.metrics().cell_width.get(),
            metrics.cell_size.height as f64,
        );
        let room = self.lbterm_room()?;
        let gutter = self.lbterm_shape().gutter() as f64;
        let card = tasks::card(
            &shown,
            // A gutter past the open panel, where the first pane's card starts while pressed
            layout.panel(true).right() as f64 + gutter,
            layout.highlight(row).y as f64,
            (room.top as f64 + gutter, room.bottom as f64 - gutter),
            self.lbterm_tasks_max_right(),
            cell,
            scale,
        );
        // Again when an age reads differently. A subagent's transcript growing repaints by itself
        // (transcripts()'s wake), so nothing looks for it: until 2026-10-30 the card repainted
        // every second a subagent was listed, to ask
        let aged = age_change.map(|secs| Instant::now() + Duration::from_secs_f64(secs + 0.05));
        // The clock's rate for the budget: each task's age by the second, and when the next is
        let mut budget = self.lbterm_budget.borrow_mut();
        budget.tasks_ticking = list.tasks.len();
        budget.tasks_aged = aged;
        drop(budget);
        Ok(card.map(|card| (card, aged)))
    }

    /// How far right the card may reach: the window's right edge, less its border and a gutter
    fn lbterm_tasks_max_right(&self) -> f64 {
        let scale = self.dimensions.dpi as f64 / 96.0;
        self.dimensions.pixel_width as f64 - self.get_os_border().right.get() as f64 - 4.0 * scale
    }

    /// Where the card's visual goes this frame and whether it's opening (true) or closing, for
    /// the compositor; `None` while there's no card to show
    pub(crate) fn lbterm_tasks_card_visual(&self) -> Option<(PixelRect, bool)> {
        self.lbterm_tasks.visual
    }

    /// Draw `card` over `surface`: its shadow, its hairline and fill, its words
    fn lbterm_tasks_draw(&self, card: &Card, surface: SrgbaTuple) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let font = self.fonts.title_font()?;
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let line_height = metrics.cell_size.height as f64;
        let shape = self.lbterm_shape();
        let radius = shape.radius() as f64;
        let stroke = shape.hairline() as f64;
        let gl_state = self.render_state.as_ref().unwrap();
        let grow = |r: Rect, by: f64, down: f64| Rect {
            x: r.x - by,
            y: r.y - by + down,
            w: r.w + 2.0 * by,
            h: r.h + 2.0 * by,
        };

        // A soft shadow under it, so it reads over the panes beside the panel
        {
            let layer = gl_state.layer_for_zindex(SHADOW_Z)?;
            let mut quads = layer.quad_allocator();
            // Out to strip::shadow_reach, which the card's visual holds
            let (reach, down) = strip::shadow_reach(scale);
            for (i, alpha) in [0.30f32, 0.18, 0.10, 0.05].iter().enumerate() {
                let by = reach * (2.0 + 3.0 * i as f64) / 11.0;
                let shadow = LinearRgba::with_components(0., 0., 0., *alpha);
                self.lbterm_menu_rounded(&mut quads, grow(card.rect, by, down), radius + by, shadow)?;
            }
        }
        // The hairline, then the card inside it
        {
            let layer = gl_state.layer_for_zindex(CARD_Z)?;
            let mut quads = layer.quad_allocator();
            self.lbterm_menu_rounded(&mut quads, grow(card.rect, stroke, 0.), radius + stroke, over(surface, RING_ALPHA))?;
            self.lbterm_menu_rounded(&mut quads, card.rect, radius, surface.to_linear())?;
        }

        // Each laid out only when the card's last frame didn't have it (render/words.rs)
        let right = card.rect.x + card.rect.w;
        let mut words: Vec<super::words::Word> = vec![];
        for line in &card.lines {
            let mut place = |text: &str, x: f64, alpha: f32| {
                let bounds = euclid::rect(x as f32, line.y as f32, (right - x).max(0.0) as f32, line_height as f32);
                words.push((text.to_string(), bounds, over(surface, alpha)));
            };
            if let Some(label) = &line.label {
                place(label, card.label_x, LABEL_ALPHA);
            }
            let alpha = match line.tone {
                Tone::Task => TEXT_ALPHA,
                Tone::Detail => DETAIL_ALPHA,
                Tone::More => MORE_ALPHA,
            };
            place(&line.text, card.text_x, alpha);
            if let Some((age, x)) = &line.right {
                place(age, *x, AGE_ALPHA);
            }
        }
        let room = (metrics.cell_size.width as f32, metrics.cell_size.height as f32);
        self.lbterm_draw_words(&self.lbterm_tasks.words, &font, &metrics, room, TEXT_Z, words)
    }

    /// What the open card lists, for the state file (lbterm/STATE.md)
    pub(crate) fn lbterm_tasks_said(&self) -> Option<lbterm_extras::state::TasksCard> {
        self.lbterm_tasks.said.clone()
    }

    /// The card's waits under way at `now`: its rest before it opens and its grace before it
    /// closes (lbterm/STATE.md)
    pub(crate) fn lbterm_tasks_waits(&self, now: Instant) -> Vec<lbterm_extras::state::Wait> {
        let hover = &self.lbterm_tasks.hover;
        let name = match hover.phase() {
            lbterm_panel::Phase::Opening => "rail.tasks.open",
            lbterm_panel::Phase::Closing => "rail.tasks.close",
            _ => return vec![],
        };
        hover
            .deadline()
            .map(|due| lbterm_extras::state::Wait {
                name: name.to_string(),
                left_ms: lbterm_extras::state::millis(due.saturating_duration_since(now)),
            })
            .into_iter()
            .collect()
    }
}

/// The open card as the state file says it: each line's parts a space apart
fn said(row: usize, card: &Card) -> lbterm_extras::state::TasksCard {
    lbterm_extras::state::TasksCard {
        row,
        rect: lbterm_extras::state::Rect {
            x: card.rect.x.round() as i32,
            y: card.rect.y.round() as i32,
            w: card.rect.w.round().max(0.0) as u32,
            h: card.rect.h.round().max(0.0) as u32,
        },
        lines: card
            .lines
            .iter()
            .map(|l| {
                let mut parts: Vec<&str> = vec![];
                if let Some(label) = &l.label {
                    parts.push(label);
                }
                parts.push(&l.text);
                if let Some((age, _)) = &l.right {
                    parts.push(age);
                }
                parts.join(" ")
            })
            .collect(),
    }
}
