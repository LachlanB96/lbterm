//! LBTerm: triggers (lbterm/TRIGGERS.md). The rules in lbterm-triggers look at each line as a
//! pane draws it. What they match is coloured on a copy of the line, so the text itself never
//! changes; what's new plays its effect and fires the `lbterm-trigger` Lua event. With the
//! compositor and motion on, DWM plays the hearts (compositor.rs, prepare_hearts): their picture
//! is drawn once into a sheet, each burst is handed over as it rises, and each frame says where
//! its pane is, so they follow it and hide while it isn't shown. Otherwise they're drawn here
//! frame by frame, over their pane as it is in each.

use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::quad::{QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::compositor::{HeartsBurst, HeartsPlan};
use crate::termwindow::{TermWindow, TermWindowNotif};
use crate::scripting::guiwin::GuiWin;
use config::lbterm::LbtermTrigger;
use lbterm_motion::PixelRect;
use lbterm_triggers::{colour, hearts, Effect, Fire, Hit, LineText, Rgb, Rules, Seen, Spec, Style};
use mux::pane::PaneId;
use mux::Mux;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};
use termwiz::surface::SequenceNo;
use wezterm_term::color::{ColorAttribute, ColorPalette, SrgbaTuple};
use wezterm_term::{Line, StableRowIndex};
use window::color::LinearRgba;
use window::WindowOps;

/// The hearts' z-layer: over the panes and the tab bar, under the tab panel (50) and modals.
/// While DWM plays them, their sheet's alone (render/composited.rs); otherwise `over`'s
pub(crate) const HEARTS_Z: i8 = 20;
/// Without the compositor the hearts are redrawn this often while they rise
const FRAME: Duration = Duration::from_millis(16);
/// How often old memories are let go
const TIDY_EVERY: Duration = Duration::from_secs(5);
/// Lines whose matches are kept, before the cache starts over (it holds only what's on the
/// screen, or was when a pane was last drawn)
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
    /// Handed to the compositor for DWM to play
    handed: bool,
}

/// The rows a pane drew last, so those it no longer draws are known to have gone, and the
/// palette it drew them in, with a number that goes up each time it changes
struct View {
    rows: Range<StableRowIndex>,
    palette: ColorPalette,
    palette_generation: u64,
}

/// What the rules made of a line, kept until it changes
struct Matched {
    seqno: SequenceNo,
    /// A hash of its text (lbterm_triggers::LineText::hash), for what's new
    text: u64,
    hits: Vec<Hit>,
    /// The line recoloured, drawn in its place: kept with the rest, so its first frame keeps its
    /// shape hash with it (its appdata) for every frame after, where a copy made again each frame
    /// hashed every cell again (lbterm/PERF.md, finding 62)
    styled: Option<Rc<Line>>,
    /// The pane's palette it was recoloured in (`View::palette_generation`): a tint is worked out
    /// from the colour the text has, so a program changing the palette (OSC 4, 10) recolours it
    palette: u64,
    /// The columns its rainbow rules matched, for screen_line to shade each glyph across its match
    rainbow: Option<Rc<Vec<Range<usize>>>>,
}

/// A window's triggers: the rules, what each pane's lines matched, and the hearts in flight
#[derive(Default)]
pub struct TriggersState {
    generation: Option<usize>,
    rules: Rc<Rules>,
    /// What the rules matched in each line on the screen, kept until the line changes or its row
    /// goes out of view: a frame drawing an unchanged line looks nothing up but this
    lines: HashMap<(PaneId, StableRowIndex), Matched>,
    /// The rows each pane drew last and their palette
    views: HashMap<PaneId, View>,
    seen: HashMap<PaneId, Seen>,
    /// New matches found while drawing, acted on once the panes are drawn
    fires: Vec<(PaneId, Fire)>,
    bursts: Vec<Burst>,
    bursts_started: u32,
    /// The columns rainbow rules matched in the line being drawn, for screen_line to shade
    /// each glyph across its match
    rainbow: Option<Rc<Vec<Range<usize>>>>,
    /// When the next frame of hearts is due
    wake: Option<Instant>,
    /// DWM plays the hearts: with the compositor and motion on, as this frame's paint found
    dwm: bool,
    tidied: Option<Instant>,
}

impl TriggersState {
    /// The line kept for `pane`'s `row` gone from the screen, and what it matched with it
    fn forget(&mut self, pane: PaneId, row: StableRowIndex) {
        let Some(matched) = self.lines.remove(&(pane, row)) else {
            return;
        };
        if !matched.hits.is_empty() {
            self.seen
                .entry(pane)
                .or_default()
                .went(row, matched.text, &matched.hits);
        }
    }

    /// Every line kept gone from the screen: the rules changed, or there are too many
    fn forget_all(&mut self) {
        let keys: Vec<(PaneId, StableRowIndex)> = self.lines.keys().copied().collect();
        for (pane, row) in keys {
            self.forget(pane, row);
        }
        self.views.clear();
    }
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

/// `line` with what `hits` style recoloured, and the columns its rainbow rules matched; nothing
/// when no rule that matched styles anything
fn style(
    rules: &Rules,
    line: &Line,
    hits: &[Hit],
    palette: &ColorPalette,
) -> (Option<Rc<Line>>, Option<Rc<Vec<Range<usize>>>>) {
    let styled: Vec<(&Hit, Style)> = hits
        .iter()
        .filter_map(|h| rules.get(h.rule).style.map(|s| (h, s)))
        .collect();
    if styled.is_empty() {
        return (None, None);
    }
    let mut rainbow = vec![];
    let mut copy = line.clone();
    let cells = copy.cells_mut_for_attr_changes_only();
    for (hit, style) in styled {
        if style == Style::Rainbow {
            rainbow.push(hit.cols.clone());
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
    (Some(Rc::new(copy)), (!rainbow.is_empty()).then(|| Rc::new(rainbow)))
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
            // What the old rules matched goes with them; the same matches found again by the new
            // ones aren't new
            state.forget_all();
            state.rules = Rc::new(rules);
            state.generation = Some(generation);
        }
        Rc::clone(&state.rules)
    }

    /// Before a pane's lines are drawn: the rows it draws, in `palette`. What it drew last time
    /// and doesn't now has gone from the screen, and what those lines matched with them
    pub(crate) fn lbterm_triggers_view(
        &mut self,
        pane_id: PaneId,
        rows: Range<StableRowIndex>,
        palette: &ColorPalette,
    ) {
        let state = &mut self.lbterm_triggers;
        let Some(view) = state.views.get_mut(&pane_id) else {
            state.views.insert(
                pane_id,
                View {
                    rows,
                    palette: palette.clone(),
                    palette_generation: 0,
                },
            );
            return;
        };
        if view.palette != *palette {
            view.palette = palette.clone();
            view.palette_generation += 1;
        }
        if view.rows == rows {
            return;
        }
        let old = std::mem::replace(&mut view.rows, rows.clone());
        for row in old {
            if !rows.contains(&row) {
                state.forget(pane_id, row);
            }
        }
    }

    /// Runs for every line a pane draws. Returns a recoloured copy to draw instead when a rule
    /// styles part of it, and notes new matches for [`Self::lbterm_triggers_fire`]. A line is
    /// matched when it's new or has changed, and what it matched (and its copy) kept until then:
    /// an unchanged line costs a lookup, where each frame copied, recoloured and hashed every
    /// line with a match again, and told what it matched again to what's new (PERF.md, finding 62)
    pub(crate) fn lbterm_trigger_line(
        &mut self,
        pane_id: PaneId,
        row: StableRowIndex,
        line: &Line,
        palette: &ColorPalette,
    ) -> Option<Rc<Line>> {
        self.lbterm_triggers.rainbow = None;
        let rules = self.lbterm_trigger_rules();
        if rules.is_empty() {
            return None;
        }
        let seqno = line.current_seqno();
        let state = &mut self.lbterm_triggers;
        let palette_now = state.views.get(&pane_id).map_or(0, |v| v.palette_generation);
        if let Some(matched) = state.lines.get_mut(&(pane_id, row)) {
            if matched.seqno == seqno {
                if matched.palette != palette_now && !matched.hits.is_empty() {
                    // The same line in another palette: recoloured, nothing new about it
                    let (styled, rainbow) = style(&rules, line, &matched.hits, palette);
                    matched.styled = styled;
                    matched.rainbow = rainbow;
                    matched.palette = palette_now;
                }
                state.rainbow = matched.rainbow.clone();
                return matched.styled.clone();
            }
        }

        // New, or changed: its text made once, matched, and what's new among its matches noted
        let mut text = LineText::with_capacity(160);
        for cell in line.visible_cells() {
            text.push(cell.cell_index(), cell.str(), cell.width());
        }
        let hits = rules.find_in(&text);
        let hash = if hits.is_empty() { 0 } else { text.hash() };
        if state.lines.len() >= MAX_LINES {
            state.forget_all();
        }
        state.forget(pane_id, row);
        if !hits.is_empty() {
            state.seen.entry(pane_id).or_default().came(row, hash, &hits);
        }
        let (styled, rainbow) = style(&rules, line, &hits, palette);
        state.rainbow = rainbow.clone();
        state.lines.insert(
            (pane_id, row),
            Matched {
                seqno,
                text: hash,
                hits,
                styled: styled.clone(),
                palette: palette_now,
                rainbow,
            },
        );
        styled
    }

    /// For screen_line: the rainbow at a glyph's left and right edges (`x0`, `x1`, pixels from
    /// the line's start), when `cell` is in a rainbow match of the line being drawn
    pub(crate) fn lbterm_rainbow_gradient(
        &self,
        cell: usize,
        x0: f32,
        x1: f32,
    ) -> Option<(LinearRgba, LinearRgba)> {
        let cols = self
            .lbterm_triggers
            .rainbow
            .as_ref()?
            .iter()
            .find(|r| r.contains(&cell))?;
        let cell_width = self.render_metrics.cell_size.width as f32;
        let start = cols.start as f32 * cell_width;
        let width = (cols.len().max(1) as f32 * cell_width).max(1.0);
        let at = |x: f32| {
            let c = colour::rainbow((x - start) / width);
            LinearRgba::with_srgba(c.0, c.1, c.2, 0xff)
        };
        Some((at(x0), at(x1)))
    }

    /// Once the panes are drawn: what came onto the screen and went from it settled
    /// (`Seen::settle`), the effects of what was new started and Lua told, when it has a handler
    /// for `lbterm-trigger` (without one, each was a task and Lua objects for nothing)
    pub(crate) fn lbterm_triggers_fire(&mut self) {
        // Nothing drawn after the panes is a pane's line
        self.lbterm_triggers.rainbow = None;
        let now = Instant::now();
        {
            let state = &mut self.lbterm_triggers;
            for (pane, seen) in state.seen.iter_mut().filter(|(_, s)| s.unsettled()) {
                let fires = seen.settle(&state.rules, now);
                state.fires.extend(fires.into_iter().map(|f| (*pane, f)));
            }
        }
        let fires = std::mem::take(&mut self.lbterm_triggers.fires);
        let tell_lua = !fires.is_empty() && crate::termwindow::titles::has_handler("lbterm-trigger");
        for (pane_id, fire) in fires {
            if fire.effect == Some(Effect::Hearts) {
                let state = &mut self.lbterm_triggers;
                state.bursts_started = state.bursts_started.wrapping_add(1);
                state.bursts.push(Burst {
                    pane: pane_id,
                    started: now,
                    seed: state.bursts_started,
                    handed: false,
                });
            }
            if tell_lua {
                let name = self.lbterm_triggers.rules.get(fire.rule).name.clone();
                self.emit_lbterm_trigger_event(pane_id, name, fire.text);
            }
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
            state.views.retain(|pane, _| mux.get_pane(*pane).is_some());
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

    /// The hearts' sheet: one heart in each of their pinks, in squares side by side at the
    /// window's top left, each heart as big as one grows with a pixel clear round it: where it
    /// is, its squares' size and the hearts' within them
    fn lbterm_hearts_sheet(&self) -> (PixelRect, f32, f32) {
        let scale = self.dimensions.dpi as f32 / 96.0;
        let inner = (hearts::MAX_SIZE * scale).ceil();
        let cell = inner + 2.0;
        let sheet = PixelRect::new(0, 0, (cell as u32) * hearts::PINKS.len() as u32, cell as u32);
        (sheet, cell, inner)
    }

    /// The hearts' sheet drawn into `quads`
    fn lbterm_draw_hearts_sheet(&self, quads: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        let (_, cell, inner) = self.lbterm_hearts_sheet();
        for (k, pink) in hearts::PINKS.iter().enumerate() {
            let colour = LinearRgba::with_srgba(pink.0, pink.1, pink.2, 0xff);
            self.poly_quad(
                quads,
                0,
                euclid::point2(k as f32 * cell + 1.0, 1.0),
                HEART,
                1,
                euclid::size2(inner, inner),
                colour,
            )?
            .set_grayscale();
        }
        Ok(())
    }

    /// Whether DWM plays the hearts this frame (with the compositor and motion on), as this
    /// frame's paint found: their z-layer is then their sheet's alone
    pub(crate) fn lbterm_hearts_by_dwm(&self) -> bool {
        self.lbterm_triggers.dwm
    }

    /// Where the hearts of a burst over `pane` rise from this frame, by its rectangle in
    /// lbterm_spans: its bottom right corner (clear of the scrollbar, which sits in the right
    /// padding) and its top; none when it isn't shown
    fn lbterm_hearts_place(&self, pane: PaneId) -> Option<hearts::Place> {
        let rect = self
            .lbterm_spans
            .panes()
            .iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, r)| *r)?;
        let right_inset = self.effective_right_padding(&self.config) as f32;
        Some(hearts::Place {
            corner: ((rect.x + rect.w as i32) as f32 - right_inset, (rect.y + rect.h as i32) as f32),
            top: rect.y as f32,
        })
    }

    /// For the compositor, while DWM plays the hearts and a burst is in the air: the bursts
    /// risen since the last frame, each handed over once, the sheet they're cut from, and where
    /// each pane with a burst in the air is this frame, so its hearts follow it
    pub(crate) fn lbterm_hearts_plan(&mut self) -> Option<HeartsPlan> {
        if !self.lbterm_triggers.dwm || self.lbterm_triggers.bursts.is_empty() {
            return None;
        }
        let scale = self.dimensions.dpi as f32 / 96.0;
        let mut panes: Vec<(PaneId, Option<hearts::Place>)> = vec![];
        for burst in &self.lbterm_triggers.bursts {
            if !panes.iter().any(|(id, _)| *id == burst.pane) {
                panes.push((burst.pane, self.lbterm_hearts_place(burst.pane)));
            }
        }
        let mut bursts = vec![];
        for burst in self.lbterm_triggers.bursts.iter_mut().filter(|b| !b.handed) {
            burst.handed = true;
            let Some(Some(place)) = panes.iter().find(|(id, _)| *id == burst.pane).map(|(_, p)| *p) else {
                continue;
            };
            bursts.push(HeartsBurst {
                pane: burst.pane,
                place,
                scale,
                seed: burst.seed,
            });
        }
        let (sheet, cell, inner) = self.lbterm_hearts_sheet();
        Some(HeartsPlan {
            sheet,
            cell,
            inner,
            bursts,
            panes,
        })
    }

    /// The hearts in flight, over the panes they rose from. While DWM plays them (with the
    /// compositor and motion on), only their sheet while a burst is in the air, which is drawn
    /// into only when it's new or a new size; otherwise every heart, every 16 ms, into `over`
    /// with the compositor (motion off, Windows' animation effects off under the default
    /// `lbterm_motion.enabled = 'system'` among them), as before DWM played them
    pub(crate) fn paint_lbterm_hearts(&mut self) -> anyhow::Result<()> {
        let now = Instant::now();
        self.lbterm_triggers
            .bursts
            .retain(|b| now.saturating_duration_since(b.started) < hearts::DURATION);
        if self.lbterm_triggers.bursts.is_empty() {
            return Ok(());
        }
        // The config's motion as the window keeps it, not its curves parsed again every paint
        // (compositor.rs, `MotionCache`)
        let dwm = self.lbterm_composited() && self.lbterm_motion().enabled;
        self.lbterm_triggers.dwm = dwm;
        if dwm {
            let gl_state = self.render_state.as_ref().unwrap();
            let layer = gl_state.layer_for_zindex(HEARTS_Z)?;
            let mut quads = layer.quad_allocator();
            return self.lbterm_draw_hearts_sheet(&mut quads);
        }
        let scale = self.dimensions.dpi as f32 / 96.0;
        let gl_state = self.render_state.as_ref().unwrap();
        let layer = gl_state.layer_for_zindex(HEARTS_Z)?;
        let mut quads = layer.quad_allocator();
        // A burst DWM took (motion on as it rose) plays on there
        for burst in self.lbterm_triggers.bursts.iter().filter(|b| !b.handed) {
            let Some(place) = self.lbterm_hearts_place(burst.pane) else {
                continue;
            };
            let (corner_x, corner_y) = place.corner;
            for heart in hearts::frame(now.saturating_duration_since(burst.started), burst.seed) {
                let size = (heart.size * scale).round().max(4.0);
                let x = corner_x - heart.left * scale - size / 2.0;
                let y = corner_y - heart.up * scale - size / 2.0;
                if y + size < place.top {
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
            lbterm_extras::counters::timer(smol::Timer::at(at)).await;
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
    /// the last will have gone while the GUI draws them (while DWM plays them, its animations
    /// say how long they've left)
    pub(crate) fn lbterm_state_hearts(&self, now: Instant) -> (usize, Option<(Instant, Instant)>) {
        let flying: Vec<Instant> = self
            .lbterm_triggers
            .bursts
            .iter()
            .map(|b| b.started)
            .filter(|started| now.saturating_duration_since(*started) < hearts::DURATION)
            .collect();
        let span = match (flying.iter().min(), flying.iter().max()) {
            _ if self.lbterm_triggers.dwm => None,
            (Some(&first), Some(&last)) => Some((first, last + hearts::DURATION)),
            _ => None,
        };
        (flying.len(), span)
    }
}
