//! LBTerm: triggers (lbterm/TRIGGERS.md). The rules in lbterm-triggers look at each line as a
//! pane draws it. What they match is coloured on a copy of the line, so the text itself never
//! changes; what's new plays its effect and fires the `lbterm-trigger` Lua event.

use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::quad::QuadTrait;
use crate::termwindow::{TermWindow, TermWindowNotif};
use crate::scripting::guiwin::GuiWin;
use config::lbterm::LbtermTrigger;
use lbterm_triggers::{colour, hearts, Cell, Effect, Fire, Hit, Rgb, Rules, Seen, Spec, Style};
use mux::pane::PaneId;
use mux::Mux;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use wezterm_term::color::{ColorAttribute, ColorPalette, SrgbaTuple};
use wezterm_term::{Line, StableRowIndex};
use window::color::LinearRgba;
use window::WindowOps;

/// The hearts' z-layer: over the panes and the tab bar, under the tab panel (50) and modals
const HEARTS_Z: i8 = 20;
/// The hearts are redrawn this often while they rise
const FRAME: Duration = Duration::from_millis(16);
/// How often old memories are let go
const TIDY_EVERY: Duration = Duration::from_secs(5);
/// Matches remembered per line, before the cache starts over
const MAX_LINES: usize = 20_000;

/// A heart, drawn as a filled path in a square: two lobes and a point
const HEART: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(1, 2), BlockCoord::Frac(23, 25)),
        PolyCommand::QuadTo {
            control: (BlockCoord::Frac(1, 25), BlockCoord::Frac(14, 25)),
            to: (BlockCoord::Frac(2, 25), BlockCoord::Frac(8, 25)),
        },
        PolyCommand::QuadTo {
            control: (BlockCoord::Frac(3, 25), BlockCoord::Frac(1, 25)),
            to: (BlockCoord::Frac(8, 25), BlockCoord::Frac(2, 25)),
        },
        PolyCommand::QuadTo {
            control: (BlockCoord::Frac(23, 50), BlockCoord::Frac(1, 10)),
            to: (BlockCoord::Frac(1, 2), BlockCoord::Frac(7, 25)),
        },
        PolyCommand::QuadTo {
            control: (BlockCoord::Frac(27, 50), BlockCoord::Frac(1, 10)),
            to: (BlockCoord::Frac(17, 25), BlockCoord::Frac(2, 25)),
        },
        PolyCommand::QuadTo {
            control: (BlockCoord::Frac(22, 25), BlockCoord::Frac(1, 25)),
            to: (BlockCoord::Frac(23, 25), BlockCoord::Frac(8, 25)),
        },
        PolyCommand::QuadTo {
            control: (BlockCoord::Frac(24, 25), BlockCoord::Frac(14, 25)),
            to: (BlockCoord::Frac(1, 2), BlockCoord::Frac(23, 25)),
        },
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::Fill,
}];

struct Burst {
    pane: PaneId,
    started: Instant,
    seed: u32,
}

/// A window's triggers: the rules, what each pane has matched, and the hearts in flight
#[derive(Default)]
pub struct TriggersState {
    generation: Option<usize>,
    rules: Rc<Rules>,
    /// What the rules matched in each line, kept until the line changes
    lines: HashMap<(PaneId, StableRowIndex), (usize, Rc<Vec<Hit>>)>,
    seen: HashMap<PaneId, Seen>,
    /// New matches found while drawing, acted on once the panes are drawn
    fires: Vec<(PaneId, Fire)>,
    bursts: Vec<Burst>,
    bursts_started: u32,
    /// The columns rainbow rules matched in the line being drawn, for screen_line to shade
    /// each glyph across its match
    rainbow: Vec<std::ops::Range<usize>>,
    /// When the next frame of hearts is due
    wake: Option<Instant>,
    tidied: Option<Instant>,
}

fn spec(t: &LbtermTrigger) -> Spec {
    Spec {
        name: t.name.clone(),
        regex: t.regex.clone(),
        tint: t.tint.clone(),
        tint_amount: t.tint_amount as f32,
        rainbow: t.rainbow,
        effect: t.effect.clone(),
        cooldown_ms: t.cooldown_ms,
    }
}

fn rgb(c: SrgbaTuple) -> Rgb {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb(byte(c.0), byte(c.1), byte(c.2))
}

fn srgba(c: Rgb) -> SrgbaTuple {
    SrgbaTuple(
        c.0 as f32 / 255.0,
        c.1 as f32 / 255.0,
        c.2 as f32 / 255.0,
        1.0,
    )
}

impl TermWindow {
    fn lbterm_trigger_rules(&mut self) -> Rc<Rules> {
        let generation = self.config.generation();
        let state = &mut self.lbterm_triggers;
        if state.generation != Some(generation) {
            let specs: Vec<Spec> = match &self.config.lbterm_triggers {
                None => lbterm_triggers::easter_eggs(),
                Some(list) => list.iter().map(spec).collect(),
            };
            let (rules, errors) = Rules::compile(&specs);
            for err in errors {
                log::error!("{err}");
            }
            state.rules = Rc::new(rules);
            state.generation = Some(generation);
            state.lines.clear();
        }
        Rc::clone(&state.rules)
    }

    /// Runs for every line a pane draws. Returns a recoloured copy to draw instead when a rule
    /// styles part of it, and notes new matches for [`Self::lbterm_triggers_fire`].
    pub(crate) fn lbterm_trigger_line(
        &mut self,
        pane_id: PaneId,
        row: StableRowIndex,
        line: &Line,
        palette: &ColorPalette,
    ) -> Option<Line> {
        self.lbterm_triggers.rainbow.clear();
        let rules = self.lbterm_trigger_rules();
        if rules.is_empty() {
            return None;
        }
        let seqno = line.current_seqno();
        let state = &mut self.lbterm_triggers;
        let hits = match state.lines.get(&(pane_id, row)) {
            Some((s, hits)) if *s == seqno => Rc::clone(hits),
            _ => {
                let owned: Vec<(usize, String, usize)> = line
                    .visible_cells()
                    .map(|c| (c.cell_index(), c.str().to_string(), c.width()))
                    .collect();
                let cells: Vec<Cell> = owned
                    .iter()
                    .map(|(col, text, width)| Cell {
                        col: *col,
                        text,
                        width: *width,
                    })
                    .collect();
                let hits = Rc::new(rules.find(&cells));
                if state.lines.len() >= MAX_LINES {
                    state.lines.clear();
                }
                state.lines.insert((pane_id, row), (seqno, Rc::clone(&hits)));
                hits
            }
        };
        if hits.is_empty() {
            return None;
        }

        let fires = state
            .seen
            .entry(pane_id)
            .or_default()
            .look(&rules, row, &hits, Instant::now());
        state.fires.extend(fires.into_iter().map(|f| (pane_id, f)));

        let styled: Vec<(&Hit, Style)> = hits
            .iter()
            .filter_map(|h| rules.get(h.rule).style.map(|s| (h, s)))
            .collect();
        if styled.is_empty() {
            return None;
        }
        let mut copy = line.clone();
        let cells = copy.cells_mut_for_attr_changes_only();
        for (hit, style) in styled {
            if style == Style::Rainbow {
                state.rainbow.push(hit.cols.clone());
            }
            let len = hit.cols.len().max(1);
            for (i, col) in hit.cols.clone().enumerate() {
                let Some(cell) = cells.get_mut(col) else {
                    break;
                };
                let colour = match style {
                    Style::Tint { colour, amount } => {
                        let fg = rgb(palette.resolve_fg(cell.attrs().foreground()));
                        colour::tint(fg, colour, amount)
                    }
                    Style::Rainbow => colour::rainbow((i as f32 + 0.5) / len as f32),
                };
                cell.attrs_mut()
                    .set_foreground(ColorAttribute::TrueColorWithDefaultFallback(srgba(colour)));
            }
        }
        Some(copy)
    }

    /// For screen_line: the rainbow at a glyph's left and right edges (`x0`, `x1`, pixels from
    /// the line's start), when `cell` is in a rainbow match of the line being drawn
    pub(crate) fn lbterm_rainbow_gradient(
        &self,
        cell: usize,
        x0: f32,
        x1: f32,
    ) -> Option<(LinearRgba, LinearRgba)> {
        let cols = self.lbterm_triggers.rainbow.iter().find(|r| r.contains(&cell))?;
        let cell_width = self.render_metrics.cell_size.width as f32;
        let start = cols.start as f32 * cell_width;
        let width = (cols.len().max(1) as f32 * cell_width).max(1.0);
        let at = |x: f32| {
            let c = colour::rainbow((x - start) / width);
            LinearRgba::with_srgba(c.0, c.1, c.2, 0xff)
        };
        Some((at(x0), at(x1)))
    }

    /// Once the panes are drawn: start the effects of what was new and tell Lua
    pub(crate) fn lbterm_triggers_fire(&mut self) {
        // Nothing drawn after the panes is a pane's line
        self.lbterm_triggers.rainbow.clear();
        let now = Instant::now();
        let fires = std::mem::take(&mut self.lbterm_triggers.fires);
        for (pane_id, fire) in fires {
            if fire.effect == Some(Effect::Hearts) {
                let state = &mut self.lbterm_triggers;
                state.bursts_started = state.bursts_started.wrapping_add(1);
                state.bursts.push(Burst {
                    pane: pane_id,
                    started: now,
                    seed: state.bursts_started,
                });
            }
            let name = self.lbterm_triggers.rules.get(fire.rule).name.clone();
            self.emit_lbterm_trigger_event(pane_id, name, fire.text);
        }

        let state = &mut self.lbterm_triggers;
        if state.tidied.map_or(true, |t| now - t >= TIDY_EVERY) {
            state.tidied = Some(now);
            let mux = Mux::get();
            state.seen.retain(|pane, _| mux.get_pane(*pane).is_some());
            for seen in state.seen.values_mut() {
                seen.tidy(now);
            }
            state.lines.retain(|(pane, _), _| mux.get_pane(*pane).is_some());
        }
    }

    fn emit_lbterm_trigger_event(&mut self, pane_id: PaneId, name: String, text: String) {
        let Some(pane) = Mux::get().get_pane(pane_id) else {
            return;
        };
        let window = GuiWin::new(self);
        let pane = mux_lua::MuxPane(pane.pane_id());

        async fn do_event(
            lua: Option<Rc<mlua::Lua>>,
            window: GuiWin,
            pane: mux_lua::MuxPane,
            name: String,
            text: String,
        ) -> anyhow::Result<()> {
            if let Some(lua) = lua {
                let args = lua.pack_multi((window, pane, name, text))?;
                if let Err(err) =
                    config::lua::emit_event(&lua, ("lbterm-trigger".to_string(), args)).await
                {
                    log::error!("while processing lbterm-trigger event: {:#}", err);
                }
            }
            Ok(())
        }

        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            do_event(lua, window, pane, name, text)
        }))
        .detach();
    }

    /// The hearts in flight, over the panes they rose from
    pub(crate) fn paint_lbterm_hearts(&mut self) -> anyhow::Result<()> {
        let now = Instant::now();
        self.lbterm_triggers
            .bursts
            .retain(|b| now.saturating_duration_since(b.started) < hearts::DURATION);
        if self.lbterm_triggers.bursts.is_empty() {
            return Ok(());
        }
        let scale = self.dimensions.dpi as f32 / 96.0;
        // Clear of the scrollbar, which sits in the right padding
        let right_inset = self.effective_right_padding(&self.config) as f32;
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(HEARTS_Z)?;
        let mut quads = layer.quad_allocator();
        for burst in &self.lbterm_triggers.bursts {
            let Some(rect) = self
                .lbterm_spans
                .panes()
                .iter()
                .find(|(id, _)| *id == burst.pane)
                .map(|(_, r)| *r)
            else {
                continue;
            };
            let corner_x = (rect.x + rect.w as i32) as f32 - right_inset;
            let corner_y = (rect.y + rect.h as i32) as f32;
            for heart in hearts::frame(now.saturating_duration_since(burst.started), burst.seed) {
                let size = (heart.size * scale).round().max(4.0);
                let x = corner_x - heart.left * scale - size / 2.0;
                let y = corner_y - heart.up * scale - size / 2.0;
                if y + size < rect.y as f32 {
                    continue;
                }
                let colour = LinearRgba::with_srgba(
                    heart.colour.0,
                    heart.colour.1,
                    heart.colour.2,
                    (heart.alpha * 255.0).round() as u8,
                );
                self.poly_quad(
                    &mut quads,
                    0,
                    euclid::point2(x, y),
                    HEART,
                    1,
                    euclid::size2(size, size),
                    colour,
                )?
                .set_grayscale();
            }
        }
        // Their own timer: the GUI's animation frames stop while the window is unfocused, and
        // hearts from a background pane shouldn't hang in the air
        self.lbterm_hearts_wake(now + FRAME);
        Ok(())
    }

    fn lbterm_hearts_wake(&mut self, at: Instant) {
        // One already due sooner repaints, and the paint asks again
        if matches!(self.lbterm_triggers.wake, Some(due) if due > Instant::now() && due <= at) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.lbterm_triggers.wake = Some(at);
        promise::spawn::spawn(async move {
            smol::Timer::at(at).await;
            window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                if tw.lbterm_triggers.wake == Some(at) {
                    tw.lbterm_triggers.wake = None;
                }
                if let Some(window) = tw.window.as_ref() {
                    window.invalidate();
                }
            })));
        })
        .detach();
    }
}


/// LBTerm: the hearts, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// How many bursts of hearts are in the air at `now`, and from when the first rose to when
    /// the last will have gone
    pub(crate) fn lbterm_state_hearts(&self, now: Instant) -> (usize, Option<(Instant, Instant)>) {
        let flying: Vec<Instant> = self
            .lbterm_triggers
            .bursts
            .iter()
            .map(|b| b.started)
            .filter(|started| now.saturating_duration_since(*started) < hearts::DURATION)
            .collect();
        let span = match (flying.iter().min(), flying.iter().max()) {
            (Some(&first), Some(&last)) => Some((first, last + hearts::DURATION)),
            _ => None,
        };
        (flying.len(), span)
    }
}
