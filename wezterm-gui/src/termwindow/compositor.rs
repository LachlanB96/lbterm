//! LBTerm: a window's DirectComposition visual tree (lbterm/COMPOSITOR.md).
//!
//! With `lbterm_compositor` on, the window has no swapchain of its own.
//! Frames are presented into swapchains that are the content of visuals,
//! and DWM composites the visuals, so it can move, clip and fade them on
//! its own thread. The tree, bottom to top:
//!
//! - `under`: the window background, the size of the window. Its surface is
//!   `WebGpuState::surface`, configured and resized with the window.
//! - `cards`: the card templates every pane's edges and the active outline
//!   are cut from (render/card.rs), never seen themselves.
//! - `panes`: a group holding a container per pane on screen, each holding
//!   the pane's visual at its rectangle, with a surface just that size, and
//!   over its content the nine pieces of its card's edges (`Edges`). A pane
//!   without the keyboard is its container at 85%.
//! - `focus`: the outline of the pane with the keyboard, which glides to the
//!   next when the keyboard moves (`FocusOutline`).
//! - `over`: tab bar, borders, scrollbar and modals, the size of the
//!   window, with premultiplied alpha so the panes show through.
//! - `hover`: the strip's two hover slots (render/hover_fade.rs), a window
//!   button's or the restart pill's look under the pointer, faded by DWM.
//! - `strip_card`: the strip's detail card (render/strip.rs), a visual the
//!   card's size, faded in by DWM as it opens and out as it closes.
//! - `panel`: the tab panel (lbterm/TAB-PANEL.md), drawn open. The body,
//!   its glow and the words sit in a group clipped at the seam, the cap and
//!   its glow in a group that rides on the edge; opening and closing it is
//!   DWM moving the clip and the cap together, brightening the glow and
//!   fading the words. While it's open the panes show pressed to its right,
//!   their cards' outlines riding on their cut-off edges. Under it all, with
//!   `lbterm_tab_panel.wave`, the wave that washes out ahead of the edge
//!   (`WaveLayers`).
//! - `panel_lights`: over the panel, a pad's glint (render/lights.rs) and
//!   the lights running along suggested tab connections
//!   (render/connections.rs).
//! - `tasks_card`: the card of what a tab's Claude session has running,
//!   beside its row on the open panel (render/tasks_card.rs), faded as the
//!   strip's card is; under `top`, the tab menu.
//!
//! When the panes' rectangles change, `lbterm_motion::plan` says what moves
//! and the pane visuals get DComp animations: cubics sampled from the
//! `lbterm_motion` curves, played by DWM whatever LBTerm's GUI thread is
//! doing. The rules for designing motion are in the `lbterm-motion` skill.

use lbterm_motion::{Curve, PixelRect, Target};
use mux::pane::PaneId;

/// One visual's texture for this frame, and where the visual sits.
pub struct Frame {
    pub target: Target<PaneId>,
    pub rect: PixelRect,
    pub texture: wgpu::SurfaceTexture,
    /// LBTerm: what the frame draws into it (lbterm_motion::content), which its swapchain shows
    /// once it's presented (`Frame::present`)
    pub content: Option<u64>,
    /// ...and what its swapchain shows, as far as anyone knows
    pub shows: std::rc::Rc<std::cell::Cell<Option<u64>>>,
    /// LBTerm: its swapchain, for when the frame reaches the screen (present.rs)
    pub chain: Option<windows58::Win32::Graphics::Dxgi::IDXGISwapChain3>,
}

impl Frame {
    /// Present it: from now on its swapchain shows what it was drawn from
    pub fn present(self) {
        self.texture.present();
        self.shows.set(self.content);
    }
}

/// LBTerm: what this frame would draw into the visual for a target at a rectangle, as
/// lbterm_motion::content's key (render/composited.rs, `lbterm_content`), or nothing to draw it
/// whatever its swapchain shows. A visual whose swapchain shows that already isn't acquired,
/// drawn or presented: DWM keeps showing its last frame (lbterm/COMPOSITOR.md, "Only what changed
/// is drawn").
pub type Content<'a> = &'a dyn Fn(Target<PaneId>, PixelRect) -> Option<u64>;

/// The tab panel's visuals for this frame.
#[derive(Debug, Clone)]
pub struct PanelPlan {
    /// Where the body, its glow and the words sit: the open panel and its
    /// glow, under the tab bar
    pub surface: PixelRect,
    /// Where the cap and its glow sit when the panel is open
    pub cap: PixelRect,
    /// The seam (where the body ends and the cap begins) when open
    pub seam: i32,
    /// How far the edge moves between closed and open
    pub travel: u32,
    /// Where the panes' visuals start while the panel is closed, and where
    /// they end: what opening it presses into the room left
    pub panes_left: i32,
    pub panes_right: i32,
    pub open: bool,
    /// How the edge gets there
    pub edge: Curve,
    /// How the ring's light and glow brighten or dim as the edge moves
    pub glow: Curve,
    /// How lit the ring and its glow are, 0 at rest to 1 open, and
    /// pending between while the panel waits on the pointer
    /// (`lbterm_panel::Light`)
    pub light: f64,
    /// How the light gets there when only the pointer moved it, the edge
    /// staying where it is
    pub hover: Curve,
    /// How the words fade, and how long they wait for the edge before
    /// fading in
    pub words: Curve,
    pub words_delay: f64,
    /// Where the running light's halo and middle sit: round where it starts
    pub spot: PixelRect,
    pub spot_core: PixelRect,
    /// How the light runs round the ring as the panel opens
    pub sweep: Curve,
    /// The panel's layout, for the light's path
    pub layout: lbterm_panel::Layout,
    /// Where a turning arc sits, one per pad whose program waits on tasks
    /// of its own (lbterm/TAB-PANEL.md). The arc is drawn once, at the
    /// first, and shown on each
    pub spinners: Vec<PixelRect>,
    /// The arcs turn; with motion off they hold still
    pub spin: bool,
    /// LBTerm: the white arc on the Starting row's pad while a new tab is on its way
    /// (render/tab_panel.rs, `Starting`), turning as the others do
    pub start_spinner: Option<PixelRect>,
    /// The tabs' rows on the rail, in order, and how they move
    pub rows: Vec<RowPlan>,
    pub row_motion: RowMotion,
    /// The open tab's row and the top of its slot: its ring and bar are drawn there, in a
    /// visual that glides from the last
    pub marker: Option<(usize, i32)>,
    /// Each row's bars on the open panel (its tab, which bar: 0 the program's or the hub's guess
    /// of it, then each subagent's; where, how far along), whose fill glides on `slide` when the
    /// program or the guess says it's further on
    pub progress: Vec<(usize, u8, PixelRect, f64)>,
    /// The row a drag has picked up, and how far from its slot the pointer has it: it
    /// follows, over the others, and settles into its slot on `slide` when let go
    pub dragged: Option<(usize, f64)>,
    /// The Conductor's outlines (render/conductor.rs), one per pad it shows on, their hues
    /// turning; with motion off they hold still
    pub rings: Vec<RingPlan>,
    /// Where their templates are drawn: the outline's, round one of the pills, and the light's
    pub ring_template: PixelRect,
    pub light_template: PixelRect,
    /// The wave that runs ahead of the edge as it opens (`lbterm_tab_panel.wave`), with motion on
    pub wave: Option<WavePlan>,
    /// A menu's card shows over the window (render/menu_fade.rs, `lbterm_menu_cards_show`): the
    /// panel holds still under it, and a wave still washing as one comes stops where it is and
    /// fades, as it does turned round, so none of it washes through the card
    pub cards: bool,
}

/// The tab panel's wave (lbterm_panel::wave): characters washing out over the panes just ahead of
/// the edge and on past where it stops, a cell at a time, then breaking into spray
/// (`lbterm_motion::wash`).
#[derive(Debug, Clone, PartialEq)]
pub struct WavePlan {
    /// Where the water is drawn: its front where its run ends, `beyond` past the open ring
    pub rect: PixelRect,
    /// Where the spray is drawn, from just short of where the run ends
    pub spray: PixelRect,
    /// The room it shows in, from the closed ring's right edge to as far as the spray goes
    pub band: PixelRect,
    /// The open ring's right edge, where the edge stops
    pub stop: i32,
    /// A cell's width: it moves a cell at a time
    pub cell: f64,
    /// How far ahead of the edge it sets off, and how far past the edge's stop it runs, in pixels
    pub lead: f64,
    pub beyond: f64,
    /// How far the water pulls back as it fades, and the spray scatters on, in pixels
    pub recede: f64,
    pub drift: f64,
    /// The water pulling back and fading, the spray lighting up and scattering on as it fades
    pub fade: Curve,
    pub rise: Curve,
    pub splash: Curve,
    /// What it looks like, so it's drawn only when that changes; and whether this frame draws it
    pub look: u64,
    pub redraw: bool,
}

/// One of the Conductor's outlines: where it goes (its template's size), its look, the light
/// going round it, if any, and how it comes and goes, if it fades.
#[derive(Debug, Clone, PartialEq)]
pub struct RingPlan {
    pub rect: PixelRect,
    pub style: crate::termwindow::render::conductor::RingStyle,
    pub light: Option<RingLight>,
    /// The Conductor's own pad's (Lachlan's 7a): lit or fading out, on its curve. `None` for a
    /// tab's, which is there whole or not at all
    pub fade: Option<RingFade>,
}

impl RingPlan {
    /// The same outline whether it's lit or fading: one the compositor keeps and fades, not one
    /// it makes again
    pub fn same_ring(&self, other: &RingPlan) -> bool {
        (self.rect, self.style, &self.light) == (other.rect, other.style, &other.light)
    }
}

/// How the Conductor's pad's spectrum comes and goes (lbterm_extras::conductor::PadRing): its
/// opacity going to 1 while `lit` and to 0 once not, from wherever it is, on `curve`
/// (`panel_glow`, or at once with motion off and in read only)
#[derive(Debug, Clone, PartialEq)]
pub struct RingFade {
    pub lit: bool,
    pub curve: Curve,
}

/// An opacity animation has settled once it's this close to where it's going (the compositor's
/// springs end there)
pub const OPACITY_EPSILON: f64 = 0.002;

/// How long a fade from 1 to 0 on `curve` plays in DWM, to its settling: how long something
/// faded out has to be kept for its fade to be seen whole (the Conductor's pad's ring,
/// render/tab_panel.rs). Nothing for `Curve::Instant`
pub fn fade_out_lasts(curve: &Curve) -> std::time::Duration {
    let end = lbterm_motion::Motion::new(1.0, 0.0, 0.0, curve.clone()).end(OPACITY_EPSILON);
    std::time::Duration::from_secs_f64(end.max(0.0))
}

/// The light going round an outline.
#[derive(Debug, Clone, PartialEq)]
pub struct RingLight {
    /// Seconds a lap
    pub lap: f64,
    pub opacity: f32,
    /// Where its visual goes, evenly through a lap
    pub offsets: Vec<(f32, f32)>,
}

/// The lights running round a tab link's matched blocks while a tab is dragged onto the tab its
/// wait matches (render/link_drag.rs; Lachlan's E6): the light's template, where it was drawn, and
/// for each block where its visual goes, evenly through a lap of `lap` seconds
#[derive(Debug, Clone, PartialEq)]
pub struct LampPlan {
    pub template: PixelRect,
    pub lap: f64,
    pub paths: Vec<Vec<(f32, f32)>>,
}

/// The lights running to and fro along the rail's suggested tab connections (render/connections.rs;
/// lbterm/LINKS.md, "Connections"): the light's template, where it was drawn, and each line's
#[derive(Debug, Clone, PartialEq)]
pub struct GlowPlan {
    pub template: PixelRect,
    pub lines: Vec<GlowLine>,
}

/// One suggested line's light: the connection's id, a key of what the file says of it (a change
/// runs its laps again: lbterm_extras::glow), a lap's seconds, and where its visual goes, evenly
/// through the lap from the line's middle
#[derive(Debug, Clone, PartialEq)]
pub struct GlowLine {
    pub id: String,
    pub says: u64,
    pub lap: f64,
    pub path: Vec<(f32, f32)>,
}

/// The easter egg's hearts for DWM to play (render/triggers.rs): their sheet, one heart in each
/// pink (lbterm_triggers::hearts::PINKS) in squares `cell` pixels across side by side, each heart
/// `inner` across within its square, where it was drawn this frame; the bursts new since the
/// last frame; and where each pane with a burst in the air is this frame, none while it isn't
/// shown, so its hearts follow it
#[derive(Debug, Clone, PartialEq)]
pub struct HeartsPlan {
    pub sheet: PixelRect,
    pub cell: f32,
    pub inner: f32,
    pub bursts: Vec<HeartsBurst>,
    pub panes: Vec<(PaneId, Option<lbterm_triggers::hearts::Place>)>,
}

/// A burst of hearts: its pane, the corner they rise from and the pane's top they're cut off
/// at, in the window's pixels; how many of those a pixel at 96 DPI is (the hearts' own unit);
/// and its seed
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeartsBurst {
    pub pane: PaneId,
    pub place: lbterm_triggers::hearts::Place,
    pub scale: f32,
    pub seed: u32,
}

/// One tab's row on the rail (render/tab_panel.rs): which tab, so a row that moves is followed
/// and one that's new or gone is seen; its slot in the window; a hash of what it shows, so a
/// row whose look changes dissolves from its old one; and whether the pointer is on it. What the
/// closed rail shows (`look`) and what only the open panel's words show (`words_look`: its title,
/// age, project, status, bars) are hashed apart: while the panel is closed a change to the words
/// alone is drawn without a dissolve nobody would see (it dissolved an unchanged rail, 200 ms of
/// DWM's work and a frame more, for each new status or age).
#[derive(Debug, Clone, PartialEq)]
pub struct RowPlan {
    pub key: usize,
    pub slot: PixelRect,
    pub look: u64,
    pub words_look: u64,
    pub hovered: bool,
    /// LBTerm: the row this one takes over, by its key: the Starting row a new tab's row grows out
    /// of, so it dissolves into the real one on `swap` rather than folding away as the new one
    /// grows in (render/tab_panel.rs, `Starting`)
    pub was: Option<usize>,
}

/// A row's looks as its surfaces hold it: the rail's, the open panel's words', and whether the
/// pointer was on it
pub(crate) type RowLooks = (u64, u64, bool);

impl RowPlan {
    pub(crate) fn looks(&self) -> RowLooks {
        (self.look, self.words_look, self.hovered)
    }

    /// Whether `self` shows otherwise than `was` did: the rail's look, the hover, or with the
    /// panel open its words
    pub(crate) fn restyles(&self, was: &RowPlan, open: bool) -> bool {
        self.look != was.look || self.hovered != was.hovered || (open && self.words_look != was.words_look)
    }
}

/// How the rail's rows and the open tab's marker move (lbterm/DESIGN.md, "The tab panel")
#[derive(Debug, Clone)]
pub struct RowMotion {
    /// Rows sliding to their new slots, and a new row's slot opening
    pub slide: Curve,
    /// A new row fading in, and a row that's gone fading out
    pub enter: Curve,
    pub exit: Curve,
    /// A row's old look dissolving into its new one, and a hover's
    pub swap: Curve,
    pub hover: Curve,
    /// The open tab's ring and bar gliding to the tab that opens
    pub marker: Curve,
}

/// The two card templates of this frame (render/card.rs): where each is
/// drawn in the window, the side they share, and a corner piece's side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPlan {
    /// As wide as the window, `size` tall: the corners, the top and bottom
    pub wide: PixelRect,
    /// `size` wide, as tall as the window: the left and right
    pub tall: PixelRect,
    pub size: u32,
    pub corner: u32,
    /// LBTerm: how far in from a corner piece's outer edges its sides' lines lie, and how far
    /// back along them from its inner end its curve lies on them (lbterm-panel's `Template`), for
    /// what of an outline left with its tab fades (`LeftOutline`)
    pub line: u32,
    pub reach: f32,
}

/// How panes move, from `config.lbterm_motion`.
#[derive(Debug, Clone)]
pub struct MotionSettings {
    /// False for "off", or for "system" while Windows' Animation effects
    /// are off: layouts change at once
    pub enabled: bool,
    pub layout: Curve,
    pub enter: Curve,
    pub exit: Curve,
    /// The panes of a tab switched from or closed fading out over the next tab's
    pub tab_switch: Curve,
    pub panel_open: Curve,
    pub panel_close: Curve,
    pub panel_glow: Curve,
    pub panel_sweep: Curve,
    /// The panel's wave pulling back and fading as it breaks, and its spray scattering on
    pub panel_wash: Curve,
    pub panel_splash: Curve,
    /// The active outline gliding to the pane that takes the keyboard
    pub focus_move: Curve,
    /// A pane dimming as it loses the keyboard, and back
    pub dim: Curve,
    /// The strip's hover slots fading (render/hover_fade.rs)
    pub button_hover: Curve,
    /// Light answering the pointer before what it waits for: the tab
    /// panel's ring going to pending and back
    pub hover: Curve,
    /// The active outline's light rising as the keyboard arrives, and settling
    pub arrive_in: Curve,
    pub arrive_out: Curve,
    /// A click's ripple growing and fading
    pub press: Curve,
    /// Copied or opened text lighting up, and fading
    pub copy_in: Curve,
    pub copy_out: Curve,
    /// Words changing in place on the strip: the old fading out over the new
    pub swap: Curve,
    /// The light running up the rail to the Conductor's pad on a hide or a show (`Light::Send`)
    pub send: Curve,
}

/// LBTerm: how many menu slots there are (render/menu_fade.rs, `Slot`)
pub const MENU_SLOTS: usize = 4;

/// LBTerm: the Conductor's menu's slot, whose card the turning arc of an item that waits sits in
#[cfg(windows)]
const CONDUCTOR_SLOT: usize = crate::termwindow::render::menu_fade::Slot::Conductor as usize;

/// LBTerm: one menu's card this frame, for its slot's visuals (render/menu_fade.rs): where it is,
/// whether it's open (or fading out, a picture of its last frame), and the row it lights, its
/// corners' radius and its light, all in window pixels
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuVisual {
    pub rect: PixelRect,
    pub open: bool,
    pub light: Option<lbterm_motion::Rect>,
    pub radius: f64,
    pub level: f64,
    /// Its hairline, to brighten
    pub ring: MenuRing,
    /// How many keys have had nowhere to go in the slot's menus: each one more brightens the
    /// hairline once
    pub bumps: u64,
    /// How many times the slot's menus have turned their page (the tab menu's Wait on… and
    /// back): each one more dissolves the card's last frame into the new page's on `swap`
    pub page: u64,
}

/// LBTerm: a menu card's hairline, in window pixels: its outer edge and corners, and the card's
/// fill inside it and its corners (render/menu_fade.rs, `lbterm_menu_card`)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuRing {
    pub outer: lbterm_motion::Rect,
    pub outer_radius: f64,
    pub inner: lbterm_motion::Rect,
    pub inner_radius: f64,
}

/// Whether the window has the keyboard, and how many times it has arrived from outside it
/// (`lbterm_extras::arrival`): the active outline goes out without it and pulses as it arrives.
/// `held` is the colour of the modifiers held (`palette::held`), which the outline takes at its
/// full light while they are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyboard {
    pub has: bool,
    pub arrivals: u64,
    pub held: Option<u32>,
}

/// A brief light over the panes, asked for this frame (render/lights.rs): white, its shape a
/// clip of a swatch, and only the clip and its opacity move.
#[derive(Debug, Clone, PartialEq)]
pub enum Light {
    /// A click's ripple from `x, y`, a circle growing from `start` to `reach` pixels as it
    /// fades, on `press`. With `within` (a pad's rectangle and its corners) it's over the tab
    /// panel, held inside the pad (the new-tab pad's, render/new_tab.rs)
    Press { x: f64, y: f64, reach: f64, start: f64, within: Option<(PixelRect, f64)> },
    /// A copied or opened stretch of text lighting up and fading, on `copy_in` and `copy_out`:
    /// a rectangle per row, with corners of `radius`
    Text { rects: Vec<PixelRect>, radius: f64 },
    /// A tab's pad on the rail glinting, over the tab panel: a program typed into its tab,
    /// or it was renamed. Up to the press's white and back, on `copy_in` and `copy_out`
    Pad { rect: PixelRect, radius: f64 },
    /// A row picked in the menu in slot `slot` (render/menu_fade.rs, `lbterm_menu_flash`) flashing
    /// as the picker's rows do: up to the press's white and back on `copy_in` and `copy_out`
    /// (`lbterm_motion::menu::flash`), in `menu_lights` over every menu's card, not in the card's
    /// group, so the card fading out as the pick goes doesn't take the flash with it
    Row { rect: PixelRect, radius: f64, slot: usize },
    /// A tab hidden from the Conductor or shown to it again (render/conductor_hide.rs): its pad's
    /// glint, `from` (the glint's rectangle over the tab's pad, its corners `radius`), sent up the
    /// rail to `to` over the Conductor's pad, on `send`, lit on `copy_in` as it sets off and put
    /// out there on `copy_out` (`lbterm_motion::send`, Lachlan's 6a of 2026-11-01)
    Send { from: PixelRect, to: PixelRect, radius: f64 },
}

impl MotionSettings {
    pub fn from_config(motion: &config::lbterm::LbtermMotion) -> Self {
        // The config checked the curves when it loaded
        let curve = |text: &str| {
            text.parse::<Curve>()
                .unwrap_or(Curve::Instant)
                .slowed(motion.slowdown)
        };
        Self {
            enabled: match motion.enabled.as_str() {
                "on" => true,
                "off" => false,
                _ => imp::system_animations_on(),
            },
            layout: curve(&motion.pane_layout),
            enter: curve(&motion.pane_enter),
            exit: curve(&motion.pane_exit),
            tab_switch: curve(&motion.tab_switch),
            panel_open: curve(&motion.panel_open),
            panel_close: curve(&motion.panel_close),
            panel_glow: curve(&motion.panel_glow),
            panel_sweep: curve(&motion.panel_sweep),
            panel_wash: curve(&motion.panel_wash),
            panel_splash: curve(&motion.panel_splash),
            focus_move: curve(&motion.focus_move),
            dim: curve(&motion.dim),
            button_hover: curve(&motion.button_hover),
            hover: curve(&motion.hover),
            arrive_in: curve(&motion.arrive_in),
            arrive_out: curve(&motion.arrive_out),
            press: curve(&motion.press),
            copy_in: curve(&motion.copy_in),
            copy_out: curve(&motion.copy_out),
            swap: curve(&motion.swap),
            send: curve(&motion.send),
        }
    }
}

/// LBTerm: a window's motion settings, made from its config once (lbterm/COMPOSITOR.md, "What a
/// frame costs"). Eight places in each paint asked for them, and each made them afresh: twenty
/// curves parsed from their CSS, each with its strings. They're made again for another config (a
/// reload, or the window's own overrides; the config they were made from is held, so no other can
/// take its place in memory and pass for it) and, with `enabled = 'system'`, when Windows says a
/// setting changed, as Animation effects does: read once a change, where every paint read it
/// eight times.
#[derive(Default)]
pub struct MotionCache {
    made: std::cell::RefCell<Option<(config::ConfigHandle, u64, std::rc::Rc<MotionSettings>)>>,
}

impl MotionCache {
    /// `config`'s motion settings: those made last when it's the config they were made from and,
    /// for `enabled = 'system'`, Windows' settings haven't changed since (`settings_changes`, how
    /// many times it's said they have)
    /// (lbterm_motion::content's `settings_seen` and `motion_holds`, tested there)
    pub fn get(&self, config: &config::ConfigHandle, settings_changes: u64) -> std::rc::Rc<MotionSettings> {
        use lbterm_motion::content::{motion_holds, settings_seen};
        let settings = settings_seen(&config.lbterm_motion.enabled, settings_changes);
        if let Some((held, at, motion)) = self.made.borrow().as_ref() {
            if motion_holds(std::ptr::eq::<config::Config>(&**held, &**config), *at, settings) {
                return std::rc::Rc::clone(motion);
            }
        }
        let motion = std::rc::Rc::new(MotionSettings::from_config(&config.lbterm_motion));
        *self.made.borrow_mut() = Some((config.clone(), settings, std::rc::Rc::clone(&motion)));
        motion
    }
}

impl crate::TermWindow {
    /// LBTerm: how this window's panes, panel and lights move (`config.lbterm_motion`), made once
    /// for its config (`MotionCache`)
    pub(crate) fn lbterm_motion(&self) -> std::rc::Rc<MotionSettings> {
        #[cfg(windows)]
        let settings = window::settings_changes();
        #[cfg(not(windows))]
        let settings = 0;
        self.lbterm_motion_cache.get(&self.config, settings)
    }

    /// LBTerm: whether this window can't be seen now (minimized, or for Lachlan's windows the
    /// session locked or the screens off: window's `window_unseen`), which its loops go by
    /// (lbterm_extras::work::loops_play)
    pub(crate) fn lbterm_unseen(&self) -> bool {
        #[cfg(windows)]
        {
            self.webgpu
                .as_ref()
                .map_or(false, |w| window::window_unseen(w.handle.hwnd()))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{CardPlan, Content, Frame, Keyboard, Light, MenuRing, MenuVisual, MotionSettings, CONDUCTOR_SLOT, MENU_SLOTS, OPACITY_EPSILON, PanelPlan, WavePlan};
    use crate::termwindow::render::conductor::{RingStyle, HUE_PERIOD};
    use anyhow::Context;
    use lbterm_extras::hover::{Fade, SLOTS};
    use lbterm_motion::{
        CardPart, Chain, Change, Cubic, Curve, Motion, PanelPart, PixelRect, Rect, Segments, Target,
    };
    use mux::pane::PaneId;
    use std::cell::RefCell;
    use std::time::{Duration, Instant};
    use windows58::core::{IUnknown, Interface};
    use windows58::Win32::Foundation::{BOOL, HWND};
    use windows58::Win32::Graphics::DirectComposition::{
        DCompositionCreateDevice3, IDCompositionAnimation, IDCompositionClip,
        DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR,
        IDCompositionColorMatrixEffect, IDCompositionDesktopDevice, IDCompositionDevice3,
        IDCompositionEffect, IDCompositionHueRotationEffect,
        IDCompositionRectangleClip, IDCompositionRotateTransform, IDCompositionTarget,
        IDCompositionVisual, IDCompositionVisual2, IDCompositionVisual3,
    };

    /// Pane visuals kept for reuse after their pane leaves the screen, so a
    /// tab switch resizes surfaces instead of making new swapchains.
    const SPARE_LAYERS: usize = 4;
    /// DWM gets a cubic per this much of an animation (halved where the
    /// curve is steep), well under a pixel from the curve
    const STEP: f64 = 1.0 / 120.0;
    /// A spring has settled once it's this many pixels from its target, or `OPACITY_EPSILON`
    /// close in opacity
    const PIXEL_EPSILON: f64 = 0.1;
    /// Far enough off to hide anything
    const FAR: f32 = 1.0e6;

    /// Windows' Settings > Accessibility > Visual effects > Animation effects.
    pub fn system_animations_on() -> bool {
        use windows58::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        };
        let mut on = BOOL(1);
        let read = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some(&mut on as *mut BOOL as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        read.is_err() || on.as_bool()
    }

    /// A visual and the surface presenting into it.
    struct Layer {
        // Dropped before the visual: the surface holds a reference to it
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        visual: IDCompositionVisual2,
        /// What the surface holds: this rectangle of the window
        rect: PixelRect,
        /// LBTerm: what its swapchain shows (lbterm_motion::content's key of the frame last
        /// presented into it): nothing known while it's new or once it's resized
        shows: std::rc::Rc<std::cell::Cell<Option<u64>>>,
    }

    impl Layer {
        fn new(
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            visual: IDCompositionVisual2,
            base: &wgpu::SurfaceConfiguration,
            alpha_mode: wgpu::CompositeAlphaMode,
            rect: PixelRect,
        ) -> anyhow::Result<Self> {
            // LBTerm: each counted with its time, metrics' start.surfaces (lbterm_extras::start)
            let made = std::time::Instant::now();
            // wgpu takes its own reference to the visual, and makes its
            // swapchain the visual's content when the surface is configured
            let surface = unsafe {
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(
                    visual.as_raw(),
                ))?
            };
            let config = wgpu::SurfaceConfiguration {
                width: rect.w.max(1),
                height: rect.h.max(1),
                alpha_mode,
                ..base.clone()
            };
            surface.configure(device, &config);
            unsafe {
                visual.SetOffsetX2(rect.x as f32)?;
                visual.SetOffsetY2(rect.y as f32)?;
            }
            let took = made.elapsed();
            lbterm_extras::start::surface_made(took);
            log::debug!(
                target: "lbterm::start",
                "surface {}x{} at {},{} made in {:.1} ms",
                rect.w,
                rect.h,
                rect.x,
                rect.y,
                took.as_secs_f64() * 1000.0
            );
            Ok(Self {
                surface,
                config,
                visual,
                rect,
                shows: Default::default(),
            })
        }

        /// This frame's texture for `target`, unless its swapchain shows what the frame would
        /// draw into it already, which DWM goes on showing
        fn frame(&self, target: Target<PaneId>, content: Content<'_>) -> anyhow::Result<Option<Frame>> {
            let wants = content(target, self.rect);
            if !lbterm_motion::content::must_draw(self.shows.get(), wants) {
                return Ok(None);
            }
            Ok(Some(Frame {
                target,
                rect: self.rect,
                texture: self.surface.get_current_texture()?,
                content: wants,
                shows: self.shows.clone(),
                chain: crate::present::chain(&self.surface),
            }))
        }

        /// Make the swapchain `rect`'s size. The PTY already has it, so
        /// this happens once per layout change, never once per frame.
        /// LBTerm: whether it was configured again, which can put a new swapchain behind it
        fn resize(&mut self, device: &wgpu::Device, rect: PixelRect) -> bool {
            let (w, h) = (rect.w.max(1), rect.h.max(1));
            if (w, h) != (self.config.width, self.config.height) {
                self.config.width = w;
                self.config.height = h;
                self.surface.configure(device, &self.config);
                // Its buffers are new: what they hold is anyone's guess
                self.shows.set(None);
                return true;
            }
            false
        }

        /// Move and resize to `rect` at once. LBTerm: whether its surface was configured again
        /// (a new size), which can put a new swapchain behind it: one moved keeps its own
        fn place(&mut self, device: &wgpu::Device, rect: PixelRect) -> anyhow::Result<bool> {
            let resized = self.resize(device, rect);
            if (rect.x, rect.y) != (self.rect.x, self.rect.y) {
                unsafe {
                    self.visual.SetOffsetX2(rect.x as f32)?;
                    self.visual.SetOffsetY2(rect.y as f32)?;
                }
            }
            self.rect = rect;
            Ok(resized)
        }
    }

    /// One number animating on a visual: kept so a retarget can start from
    /// where it is and how fast it's going.
    struct Tween {
        motion: Motion,
        start: Instant,
        end: f64,
        /// LBTerm: played from samples, not `motion`'s curve: the opacity of a layer of tabs in
        /// flight (`lbterm_motion::flight`), which no one curve is
        sampled: Option<Segments>,
    }

    impl Tween {
        /// What DComp plays from `start`: `segments`
        fn sampled(segments: Segments, start: Instant) -> Self {
            Tween {
                motion: Motion::new(segments.value(0.0), segments.end_value as f64, 0.0, Curve::Instant),
                start,
                end: segments.end,
                sampled: Some(segments),
            }
        }

        fn elapsed(&self, now: Instant) -> f64 {
            now.saturating_duration_since(self.start).as_secs_f64()
        }

        fn value(&self, now: Instant) -> f64 {
            let t = self.elapsed(now);
            if let Some(s) = &self.sampled {
                return s.value(t);
            }
            if t >= self.end {
                self.motion.to
            } else {
                self.motion.value(t)
            }
        }

        fn velocity(&self, now: Instant) -> f64 {
            let t = self.elapsed(now);
            if t >= self.end {
                0.0
            } else if let Some(s) = &self.sampled {
                (s.value(t + 0.001) - s.value(t)) / 0.001
            } else {
                self.motion.velocity(t)
            }
        }

        fn finishes(&self) -> Instant {
            self.start + Duration::from_secs_f64(self.end)
        }
    }

    /// Where a tween is, or `rest` without one.
    fn at(tween: &Option<Tween>, rest: f64, now: Instant) -> (f64, f64) {
        match tween {
            Some(t) => (t.value(now), t.velocity(now)),
            None => (rest, 0.0),
        }
    }

    /// The swapchain a surface presents into, to show in other visuals too.
    fn swap_chain(surface: &wgpu::Surface<'static>) -> anyhow::Result<IUnknown> {
        let chain = unsafe {
            surface.as_hal::<wgpu::hal::api::Dx12, _, _>(|s| s.and_then(|s| s.swap_chain()))
        }
        .context("no D3D12 swapchain behind the surface")?;
        Ok(chain.cast::<IUnknown>()?)
    }

    /// One card template, as wide as the window and as tall, and their
    /// swapchains: what the pieces show.
    struct Template {
        wide: Layer,
        tall: Layer,
        wide_content: IUnknown,
        tall_content: IUnknown,
    }

    impl Template {
        fn new(
            dcomp: &Compositor,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: &CardPlan,
        ) -> anyhow::Result<Self> {
            let layer = |rect: PixelRect| -> anyhow::Result<Layer> {
                let visual = unsafe { dcomp.device.CreateVisual()? };
                unsafe { dcomp.cards.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                Layer::new(
                    instance,
                    device,
                    visual,
                    base,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                    rect,
                )
            };
            let (wide, tall) = (layer(plan.wide)?, layer(plan.tall)?);
            Ok(Self {
                wide_content: swap_chain(&wide.surface)?,
                tall_content: swap_chain(&tall.surface)?,
                wide,
                tall,
            })
        }

        /// To `plan`'s sizes; true when either swapchain is a new one.
        fn place(&mut self, device: &wgpu::Device, plan: &CardPlan) -> anyhow::Result<bool> {
            self.wide.place(device, plan.wide)?;
            self.tall.place(device, plan.tall)?;
            let (wide, tall) = (swap_chain(&self.wide.surface)?, swap_chain(&self.tall.surface)?);
            let changed = wide != self.wide_content || tall != self.tall_content;
            self.wide_content = wide;
            self.tall_content = tall;
            Ok(changed)
        }
    }

    /// The card templates (render/card.rs), drawn every frame into visuals
    /// that are never seen: the panes' edges and the active outline show
    /// their swapchains instead.
    struct Cards {
        /// What's outside a card: every pane's edges
        mask: Template,
        /// The outline alone: the active pane's
        line: Template,
        plan: CardPlan,
    }

    impl Cards {
        fn new(
            dcomp: &Compositor,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: &CardPlan,
        ) -> anyhow::Result<Self> {
            Ok(Self {
                mask: Template::new(dcomp, instance, device, base, plan)?,
                line: Template::new(dcomp, instance, device, base, plan)?,
                plan: *plan,
            })
        }

        /// To `plan`'s sizes. True when the pieces need fitting again: new
        /// sizes, or a new swapchain behind a surface.
        fn place(&mut self, device: &wgpu::Device, plan: &CardPlan) -> anyhow::Result<bool> {
            let mask = self.mask.place(device, plan)?;
            let line = self.line.place(device, plan)?;
            let changed = *plan != self.plan || mask || line;
            self.plan = *plan;
            Ok(changed)
        }

        fn frames(&self, frames: &mut Vec<Frame>, content: Content<'_>) -> anyhow::Result<()> {
            for (part, layer) in [
                (CardPart::MaskWide, &self.mask.wide),
                (CardPart::MaskTall, &self.mask.tall),
                (CardPart::LineWide, &self.line.wide),
                (CardPart::LineTall, &self.line.tall),
            ] {
                frames.extend(layer.frame(Target::Card(part), content)?);
            }
            Ok(())
        }
    }

    /// One piece of a card's outline: a visual showing a template's
    /// swapchain, clipped to the piece.
    struct Piece {
        visual: IDCompositionVisual2,
        clip: IDCompositionRectangleClip,
    }

    impl Piece {
        fn new(dcomp: &Compositor, parent: &IDCompositionVisual2) -> anyhow::Result<Self> {
            let visual = unsafe { dcomp.device.CreateVisual()? };
            let clip = unsafe { dcomp.device.CreateRectangleClip()? };
            unsafe {
                visual.SetClip(&clip)?;
                parent.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
            }
            Ok(Self { visual, clip })
        }

        /// Show `content`, clipped to `[x0, x1) × [y0, y1)` of it.
        fn fit(&self, content: &IUnknown, x0: f32, y0: f32, x1: f32, y1: f32) -> anyhow::Result<()> {
            unsafe {
                self.visual.SetContent(content)?;
                self.clip.SetLeft2(x0)?;
                self.clip.SetTop2(y0)?;
                self.clip.SetRight2(x1)?;
                self.clip.SetBottom2(y1)?;
            }
            Ok(())
        }
    }

    /// A card's edges, in nine pieces of a card template: children of a
    /// visual (a pane's, or the active outline's group), so they move, clip
    /// and fade with it.
    /// The top-left corner sits still; the right-hand pieces ride on the
    /// visual's shown width and the bottom row on its height, each a fixed
    /// distance from that edge, so each is driven by the pane's own motion,
    /// shifted. So the card stays whole and rounded at every size DWM shows
    /// the visual at, while the content inside keeps its size and is cut off.
    struct Edges {
        top_left: Piece,
        top: Piece,
        top_right: Piece,
        left: Piece,
        right: Piece,
        /// The bottom-left corner, the bottom edge and the bottom-right
        /// corner, moved down together
        row: IDCompositionVisual2,
        bottom_left: Piece,
        bottom: Piece,
        bottom_right: Piece,
    }

    impl Edges {
        fn new(
            dcomp: &Compositor,
            parent: &IDCompositionVisual2,
            template: &Template,
            plan: &CardPlan,
        ) -> anyhow::Result<Self> {
            let row = unsafe { dcomp.device.CreateVisual()? };
            let edges = Self {
                top_left: Piece::new(dcomp, parent)?,
                top: Piece::new(dcomp, parent)?,
                top_right: Piece::new(dcomp, parent)?,
                left: Piece::new(dcomp, parent)?,
                right: Piece::new(dcomp, parent)?,
                bottom_left: Piece::new(dcomp, &row)?,
                bottom: Piece::new(dcomp, &row)?,
                bottom_right: Piece::new(dcomp, &row)?,
                row,
            };
            unsafe { parent.AddVisual(&edges.row, false, None::<&IDCompositionVisual>)? };
            edges.fit(template, plan)?;
            Ok(edges)
        }

        /// Cut each piece from the templates as they are. The edges' far
        /// ends, which ride on the visual's size, are set by `set` and
        /// `drive`.
        fn fit(&self, template: &Template, plan: &CardPlan) -> anyhow::Result<()> {
            let (wide, tall) = (&template.wide_content, &template.tall_content);
            let (s, c) = (plan.size as f32, plan.corner as f32);
            let w = plan.wide.w as f32;
            self.top_left.fit(wide, 0.0, 0.0, c, c)?;
            self.top.fit(wide, c, 0.0, c, c)?;
            self.top_right.fit(wide, w - c, 0.0, w, c)?;
            self.bottom_left.fit(wide, 0.0, s - c, c, s)?;
            self.bottom.fit(wide, c, s - c, c, s)?;
            self.bottom_right.fit(wide, w - c, s - c, w, s)?;
            self.left.fit(tall, 0.0, c, c, c)?;
            self.right.fit(tall, s - c, c, s, c)?;
            Ok(())
        }

        /// Each riding property, and how far it sits from the visual's
        /// width (`along` true) or height.
        fn riders(&self, cards: &CardPlan) -> [(bool, f64, Rider<'_>); 8] {
            let (s, c) = (cards.size as f64, cards.corner as f64);
            let wide = cards.wide.w as f64;
            [
                (true, -c, Rider::Right(&self.top.clip)),
                (true, -c, Rider::Right(&self.bottom.clip)),
                (true, -wide, Rider::X(&self.top_right.visual)),
                (true, -wide, Rider::X(&self.bottom_right.visual)),
                (true, -s, Rider::X(&self.right.visual)),
                (false, -c, Rider::Bottom(&self.left.clip)),
                (false, -c, Rider::Bottom(&self.right.clip)),
                (false, -s, Rider::Y(&self.row)),
            ]
        }

        /// Straight to a visual `w` by `h`.
        fn set(&self, cards: &CardPlan, w: f64, h: f64) -> anyhow::Result<()> {
            for (along, shift, rider) in self.riders(cards) {
                rider.set((if along { w } else { h } + shift) as f32)?;
            }
            Ok(())
        }

        /// On the pane's own motions of its width and height.
        fn drive(&self, dcomp: &Compositor, cards: &CardPlan, w: &Motion, h: &Motion) -> anyhow::Result<()> {
            for (along, shift, rider) in self.riders(cards) {
                let m = if along { w } else { h };
                dcomp.drive(
                    Motion::new(m.from + shift, m.to + shift, m.v0, m.curve.clone()),
                    PIXEL_EPSILON,
                    0.0,
                    |a| rider.animate(a),
                    |v| rider.set(v),
                )?;
            }
            Ok(())
        }
    }

    /// A property of a piece that rides on the visual's size.
    enum Rider<'a> {
        Right(&'a IDCompositionRectangleClip),
        Bottom(&'a IDCompositionRectangleClip),
        X(&'a IDCompositionVisual2),
        Y(&'a IDCompositionVisual2),
    }

    impl Rider<'_> {
        fn set(&self, v: f32) -> windows58::core::Result<()> {
            unsafe {
                match self {
                    Rider::Right(clip) => clip.SetRight2(v),
                    Rider::Bottom(clip) => clip.SetBottom2(v),
                    Rider::X(visual) => visual.SetOffsetX2(v),
                    Rider::Y(visual) => visual.SetOffsetY2(v),
                }
            }
        }

        fn animate(&self, a: &IDCompositionAnimation) -> windows58::core::Result<()> {
            unsafe {
                match self {
                    Rider::Right(clip) => clip.SetRight(a),
                    Rider::Bottom(clip) => clip.SetBottom(a),
                    Rider::X(visual) => visual.SetOffsetX(a),
                    Rider::Y(visual) => visual.SetOffsetY(a),
                }
            }
        }
    }

    /// A pane on screen: a container (which clips a pane riding in to the
    /// rectangle it came out of, and dims a pane without the keyboard)
    /// holding the pane's visual, whose offset, clip and opacity animate,
    /// and over its content its card's edges, riding on the clip. The
    /// surface holds the pane's rectangle of the window (`layer.rect`); the
    /// visual shows it at `shown`, which differs while the tab panel presses
    /// the panes over.
    struct PaneLayer {
        layer: Layer,
        edges: Edges,
        /// The card templates the edges were last cut from
        cards: CardPlan,
        /// The container again, for the tint that dims it to 85% without the keyboard: a colour
        /// matrix toward the window's background (`lbterm_motion::dim_tint`), not its opacity,
        /// so it stays opaque over whatever is under it, the next tab's panes as it dissolves
        dim: IDCompositionVisual3,
        tint: IDCompositionColorMatrixEffect,
        /// The tint is on the container: only while it's dimmed or on its way back
        tinted: bool,
        /// Where the dimming is going, and how, and the window background it dims toward
        level: f64,
        dimming: Option<Tween>,
        backdrop: [f64; 3],
        /// Where the visual is going: its offset, and its clip's size
        shown: PixelRect,
        container: IDCompositionVisual2,
        /// The pane's visual again, for its opacity
        opacity: IDCompositionVisual3,
        /// The pane's visible size, on the pane's visual
        clip: IDCompositionRectangleClip,
        /// The container's clip while the pane rides in
        bounds: IDCompositionRectangleClip,
        x: Option<Tween>,
        y: Option<Tween>,
        w: Option<Tween>,
        h: Option<Tween>,
        alpha: Option<Tween>,
        /// LBTerm: how much of itself it showed as its tab was left (less while it was still
        /// fading in): its tab's share of the screen in flight is scaled by it
        own: f64,
    }

    impl PaneLayer {
        fn new(
            dcomp: &Compositor,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            rect: PixelRect,
            shown: PixelRect,
            cards: &Cards,
        ) -> anyhow::Result<Self> {
            let container = unsafe { dcomp.device.CreateVisual()? };
            let visual = unsafe { dcomp.device.CreateVisual()? };
            unsafe { container.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
            let layer = Layer::new(
                instance,
                device,
                visual,
                base,
                wgpu::CompositeAlphaMode::Opaque,
                rect,
            )?;
            let opacity: IDCompositionVisual3 = layer
                .visual
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            let clip = unsafe { dcomp.device.CreateRectangleClip()? };
            unsafe { layer.visual.SetClip(&clip)? };
            let bounds = unsafe { dcomp.device.CreateRectangleClip()? };
            let edges = Edges::new(dcomp, &layer.visual, &cards.mask, &cards.plan)?;
            let dim: IDCompositionVisual3 = container
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            let effects: IDCompositionDevice3 = dcomp.device.cast().context("IDCompositionDevice3")?;
            let tint = unsafe { effects.CreateColorMatrixEffect()? };
            unsafe {
                // Undimmed to start with; alpha stays as it is throughout, the pane's fades being
                // its visual's opacity
                for i in 0..4 {
                    tint.SetMatrixElement2(i, i, 1.0)?;
                }
            }
            let mut pane = Self {
                layer,
                edges,
                cards: cards.plan,
                dim,
                tint,
                tinted: false,
                level: 1.0,
                dimming: None,
                backdrop: [0.0; 3],
                shown,
                container,
                opacity,
                clip,
                bounds,
                x: None,
                y: None,
                w: None,
                h: None,
                alpha: None,
                own: 1.0,
            };
            pane.place(device, rect, shown)?;
            Ok(pane)
        }

        /// Straight there: nothing animating, fully opaque, unclipped.
        fn place(
            &mut self,
            device: &wgpu::Device,
            rect: PixelRect,
            shown: PixelRect,
        ) -> anyhow::Result<()> {
            self.layer.resize(device, rect);
            self.layer.rect = rect;
            self.shown = shown;
            unsafe {
                self.layer.visual.SetOffsetX2(shown.x as f32)?;
                self.layer.visual.SetOffsetY2(shown.y as f32)?;
                self.clip.SetLeft2(0.0)?;
                self.clip.SetTop2(0.0)?;
                self.clip.SetRight2(shown.w as f32)?;
                self.clip.SetBottom2(shown.h as f32)?;
                self.opacity.SetOpacity2(1.0)?;
                self.container.SetClip(None::<&IDCompositionClip>)?;
            }
            self.edges.set(&self.cards, shown.w as f64, shown.h as f64)?;
            self.x = None;
            self.y = None;
            self.w = None;
            self.h = None;
            self.alpha = None;
            Ok(())
        }

        /// Where the pane is on screen at `now` (offset and visible size,
        /// mid-animation or not), and how fast each of those is changing.
        fn current(&self, now: Instant) -> ([f64; 4], [f64; 4]) {
            let s = self.shown;
            let (x, vx) = at(&self.x, s.x as f64, now);
            let (y, vy) = at(&self.y, s.y as f64, now);
            let (w, vw) = at(&self.w, s.w as f64, now);
            let (h, vh) = at(&self.h, s.h as f64, now);
            ([x, y, w, h], [vx, vy, vw, vh])
        }

        fn current_alpha(&self, now: Instant) -> f64 {
            self.alpha.as_ref().map_or(1.0, |t| t.value(now))
        }

        /// From wherever it is to `shown` on `curve`, each edge keeping its
        /// speed. The swapchain takes `rect`'s size at once; the clip and
        /// offset get there on the curve, with the content anchored at the
        /// visual's top-left.
        fn move_to(
            &mut self,
            dcomp: &Compositor,
            device: &wgpu::Device,
            rect: PixelRect,
            shown: PixelRect,
            curve: &Curve,
            now: Instant,
        ) -> anyhow::Result<()> {
            let ([x, y, w, h], [vx, vy, vw, vh]) = self.current(now);
            dcomp.name("pane.layout");
            self.layer.resize(device, rect);
            self.layer.rect = rect;
            self.shown = shown;
            unsafe { self.container.SetClip(None::<&IDCompositionClip>)? };
            let visual = &self.layer.visual;
            let clip = &self.clip;
            self.x = dcomp.drive(
                Motion::new(x, shown.x as f64, vx, curve.clone()),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { visual.SetOffsetX(a) },
                |v| unsafe { visual.SetOffsetX2(v) },
            )?;
            self.y = dcomp.drive(
                Motion::new(y, shown.y as f64, vy, curve.clone()),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { visual.SetOffsetY(a) },
                |v| unsafe { visual.SetOffsetY2(v) },
            )?;
            let (along, down) = (
                Motion::new(w, shown.w as f64, vw, curve.clone()),
                Motion::new(h, shown.h as f64, vh, curve.clone()),
            );
            self.w = dcomp.drive(
                along.clone(),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { clip.SetRight(a) },
                |v| unsafe { clip.SetRight2(v) },
            )?;
            self.h = dcomp.drive(
                down.clone(),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { clip.SetBottom(a) },
                |v| unsafe { clip.SetBottom2(v) },
            )?;
            // The card's outline rides on the clip: whole and rounded at
            // every size on the way, the content inside cut off
            self.edges.drive(dcomp, &self.cards, &along, &down)?;
            Ok(())
        }

        /// Cut the edges from new card templates, where it is now.
        fn refit(&mut self, cards: &Cards, now: Instant) -> anyhow::Result<()> {
            self.cards = cards.plan;
            self.edges.fit(&cards.mask, &cards.plan)?;
            let ([_, _, w, h], _) = self.current(now);
            self.edges.set(&self.cards, w, h)
        }

        /// To `level` (1 with the keyboard, 85% without) toward the window's `backdrop` (each
        /// channel 0 to 1, as stored): on `curve` from wherever it is, or at once without one.
        /// The tint plays the level's curve on each channel's scale and offset, so it dims as
        /// the container's opacity did and stays opaque (`lbterm_motion::dim_tint`).
        fn dim_to(
            &mut self,
            dcomp: &Compositor,
            level: f64,
            curve: Option<&Curve>,
            backdrop: [f64; 3],
            now: Instant,
        ) -> anyhow::Result<()> {
            let (from, _) = at(&self.dimming, self.level, now);
            self.level = level;
            self.backdrop = backdrop;
            let motion = Motion::new(from, level, 0.0, curve.cloned().unwrap_or(Curve::Instant));
            let segments = motion.segments(STEP, OPACITY_EPSILON);
            let undimmed = |l: f64| l >= 1.0 - OPACITY_EPSILON;
            if !self.tinted && !(undimmed(level) && segments.cubics.is_empty()) {
                unsafe { self.dim.SetEffect(&self.tint)? };
                self.tinted = true;
            }
            dcomp.name("pane.dim");
            let tint = &self.tint;
            // `dim_tint` all the way: each channel's scale is the level, and its offset the
            // backdrop less the level's share of it, both on the level's curve
            for (i, b) in backdrop.iter().copied().enumerate() {
                let (i, offset) = (i as i32, b.clamp(0.0, 1.0));
                dcomp.play(
                    &segments,
                    |a| unsafe { tint.SetMatrixElement(i, i, a) },
                    |v| unsafe { tint.SetMatrixElement2(i, i, v) },
                )?;
                dcomp.play(
                    &segments.affine(offset, -offset),
                    |a| unsafe { tint.SetMatrixElement(4, i, a) },
                    |v| unsafe { tint.SetMatrixElement2(4, i, v) },
                )?;
            }
            self.dimming = (!segments.cubics.is_empty()).then(|| Tween {
                start: now,
                end: segments.end,
                motion,
                sampled: None,
            });
            if self.tinted && undimmed(level) && self.dimming.is_none() {
                self.untint()?;
            }
            Ok(())
        }

        /// Back to undimmed at rest: the tint comes off, so DWM has no effect to run for it
        fn untint(&mut self) -> anyhow::Result<()> {
            unsafe { self.dim.SetEffect(None::<&IDCompositionEffect>)? };
            self.tinted = false;
            Ok(())
        }

        /// The tint taken off once it has brightened back to undimmed: true when it was
        fn untint_if_done(&mut self, now: Instant) -> anyhow::Result<bool> {
            let done = self.dimming.as_ref().map_or(true, |t| t.finishes() <= now);
            if !self.tinted || self.level < 1.0 - OPACITY_EPSILON || !done {
                return Ok(false);
            }
            self.dimming = None;
            self.untint()?;
            Ok(true)
        }

        /// A pane split off another: in from `from` (just past the edge of
        /// the source's old rectangle) to `shown` on the layout curve, kept
        /// inside `bounds` on the way, fading in on the enter curve.
        fn ride_in(
            &mut self,
            dcomp: &Compositor,
            device: &wgpu::Device,
            from: Rect,
            rect: PixelRect,
            shown: PixelRect,
            bounds: Rect,
            motion: &MotionSettings,
            alpha: f64,
        ) -> anyhow::Result<()> {
            self.place(device, rect, shown)?;
            unsafe {
                self.bounds.SetLeft2(bounds.x as f32)?;
                self.bounds.SetTop2(bounds.y as f32)?;
                self.bounds.SetRight2(bounds.right() as f32)?;
                self.bounds.SetBottom2(bounds.bottom() as f32)?;
                self.container.SetClip(&self.bounds)?;
            }
            dcomp.name("pane.layout");
            let visual = &self.layer.visual;
            self.x = dcomp.drive(
                Motion::new(from.x, shown.x as f64, 0.0, motion.layout.clone()),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { visual.SetOffsetX(a) },
                |v| unsafe { visual.SetOffsetX2(v) },
            )?;
            self.y = dcomp.drive(
                Motion::new(from.y, shown.y as f64, 0.0, motion.layout.clone()),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { visual.SetOffsetY(a) },
                |v| unsafe { visual.SetOffsetY2(v) },
            )?;
            self.fade(dcomp, "pane.enter", alpha, 1.0, &motion.enter)
        }

        /// Its opacity from `from` to `to` on `curve`, named `name` for the state file
        fn fade(
            &mut self,
            dcomp: &Compositor,
            name: &'static str,
            from: f64,
            to: f64,
            curve: &Curve,
        ) -> anyhow::Result<()> {
            dcomp.name(name);
            let opacity = &self.opacity;
            self.alpha = dcomp.drive(
                Motion::new(from, to, 0.0, curve.clone()),
                OPACITY_EPSILON,
                0.0,
                |a| unsafe { opacity.SetOpacity(a) },
                |v| unsafe { opacity.SetOpacity2(v) },
            )?;
            Ok(())
        }

        /// LBTerm: its opacity from now as `segments` play it, named `name` for the state file:
        /// its tab's share of the screen in flight (`lbterm_motion::flight`), which no one curve
        /// is
        fn play_alpha(&mut self, dcomp: &Compositor, name: &'static str, segments: Segments, now: Instant) -> anyhow::Result<()> {
            dcomp.name(name);
            let opacity = &self.opacity;
            dcomp.play(&segments, |a| unsafe { opacity.SetOpacity(a) }, |v| unsafe { opacity.SetOpacity2(v) })?;
            self.alpha = Some(Tween::sampled(segments, now));
            Ok(())
        }

        /// Fade out where it is, from `from` (where its opacity is, unless a dissolve says
        /// otherwise); when it'll be done.
        fn fade_out(
            &mut self,
            dcomp: &Compositor,
            name: &'static str,
            from: Option<f64>,
            curve: &Curve,
            now: Instant,
        ) -> anyhow::Result<Instant> {
            let from = from.unwrap_or_else(|| self.current_alpha(now));
            self.fade(dcomp, name, from, 0.0, curve)?;
            Ok(self.alpha.as_ref().map_or(now, Tween::finishes))
        }
    }

    /// The outline of the pane with the keyboard: nine pieces of the line
    /// template in a group of its own over the panes (lbterm/DESIGN.md,
    /// "Panes"). It follows that pane wherever its own motion takes it, and
    /// when the keyboard moves it glides to the next pane on `focus_move`,
    /// from wherever it is, keeping its speed.
    ///
    /// The group sits in a lamp whose opacity is the outline's light: the
    /// template is drawn at its brightest (`palette::card_outline_lit`), and
    /// the lamp shows it at rest while the window has the keyboard, goes out
    /// on `dim` when the window loses it, and pulses up and back on
    /// `arrive_in` and `arrive_out` as the keyboard arrives from outside.
    ///
    /// While modifiers are held the lamp goes to full on `arrive_in` and a
    /// colour matrix on it turns the white to their colour (`palette::held`),
    /// both settling back on `arrive_out` as the last is let go.
    struct FocusOutline {
        lamp: IDCompositionVisual3,
        /// The lamp's pulse or fade under way, and when it started
        lit: Option<(Chain, Instant)>,
        /// Where the lamp's light rests
        light: f64,
        /// The tint: a colour matrix on the lamp, made the first time a
        /// modifier is held, mixing the template's white to `colour` by the
        /// mix (0 white, 1 the colour)
        tint: Option<IDCompositionColorMatrixEffect>,
        colour: u32,
        /// The mix under way, and when it started
        mixing: Option<(Chain, Instant)>,
        /// Where the mix rests
        mix: f64,
        /// What the window said last: whether it had the keyboard, and
        /// its arrivals so far
        keyboard: Option<Keyboard>,
        group: IDCompositionVisual2,
        opacity: IDCompositionVisual3,
        /// LBTerm: the group's opacity coming back up from nothing after a tab switched back to
        /// while its outline faded, what showed of it going as it comes
        /// (`lbterm_motion::outline::switch`)
        alpha: Option<Tween>,
        edges: Edges,
        cards: CardPlan,
        /// The pane it's on, and where that pane is going
        pane: Option<PaneId>,
        shown: PixelRect,
        x: Option<Tween>,
        y: Option<Tween>,
        w: Option<Tween>,
        h: Option<Tween>,
    }

    impl FocusOutline {
        fn new(dcomp: &Compositor, cards: &Cards) -> anyhow::Result<Self> {
            let lamp: IDCompositionVisual3 = unsafe { dcomp.device.CreateVisual()? }
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            let light = lbterm_panel::palette::outline_rest_opacity();
            let group = unsafe { dcomp.device.CreateVisual()? };
            unsafe {
                lamp.SetOpacity2(light as f32)?;
                dcomp.focus.AddVisual(&lamp, false, None::<&IDCompositionVisual>)?;
                lamp.AddVisual(&group, false, None::<&IDCompositionVisual>)?;
            }
            let edges = Edges::new(dcomp, &group, &cards.line, &cards.plan)?;
            let opacity: IDCompositionVisual3 = group
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            unsafe { opacity.SetOpacity2(0.0)? };
            Ok(Self {
                lamp,
                lit: None,
                light,
                tint: None,
                colour: 0xFFFFFF,
                mixing: None,
                mix: 0.0,
                keyboard: None,
                group,
                opacity,
                alpha: None,
                edges,
                cards: cards.plan,
                pane: None,
                shown: PixelRect::default(),
                x: None,
                y: None,
                w: None,
                h: None,
            })
        }

        /// How much of it shows: nothing while hidden, all of it at rest, less while it comes
        /// back up after a tab switched back to
        fn opacity_at(&self, now: Instant) -> f64 {
            if self.pane.is_none() {
                return 0.0;
            }
            at(&self.alpha, 1.0, now).0
        }

        /// All of it, at once: the next tab's outline is there with the keyboard
        fn show_at_once(&mut self) -> anyhow::Result<()> {
            unsafe { self.opacity.SetOpacity2(1.0)? };
            self.alpha = None;
            Ok(())
        }

        /// Up to all of it from `from`: a tab switched back to while its outline still faded,
        /// from nothing, what showed of it going as this comes (`switch_outline`), as its panes
        /// come back (`coming`, how far up they've come, 0 to 1, from now: with other tabs still
        /// in flight they come up as those go, on no one curve), or on `curve`
        fn rise(&mut self, dcomp: &Compositor, from: f64, curve: &Curve, coming: Option<&Segments>) -> anyhow::Result<()> {
            dcomp.name("outline.return");
            let opacity = &self.opacity;
            match coming {
                Some(coming) => {
                    let segments = coming.affine(from, 1.0 - from);
                    dcomp.play(&segments, |a| unsafe { opacity.SetOpacity(a) }, |v| unsafe { opacity.SetOpacity2(v) })?;
                    self.alpha = Some(Tween::sampled(segments, Instant::now()));
                }
                None => {
                    self.alpha = dcomp.drive(
                        Motion::new(from, 1.0, 0.0, curve.clone()),
                        OPACITY_EPSILON,
                        0.0,
                        |a| unsafe { opacity.SetOpacity(a) },
                        |v| unsafe { opacity.SetOpacity2(v) },
                    )?;
                }
            }
            Ok(())
        }

        fn current(&self, now: Instant) -> ([f64; 4], [f64; 4]) {
            let s = self.shown;
            let (x, vx) = at(&self.x, s.x as f64, now);
            let (y, vy) = at(&self.y, s.y as f64, now);
            let (w, vw) = at(&self.w, s.w as f64, now);
            let (h, vh) = at(&self.h, s.h as f64, now);
            ([x, y, w, h], [vx, vy, vw, vh])
        }

        /// Straight onto `pane`, shown at `shown`; shown whole if it was hidden, its opacity
        /// otherwise as it is (coming back up after a tab switched back to)
        fn place(&mut self, pane: PaneId, shown: PixelRect) -> anyhow::Result<()> {
            if self.pane.is_none() {
                self.show_at_once()?;
            }
            unsafe {
                self.group.SetOffsetX2(shown.x as f32)?;
                self.group.SetOffsetY2(shown.y as f32)?;
            }
            self.edges.set(&self.cards, shown.w as f64, shown.h as f64)?;
            self.pane = Some(pane);
            self.shown = shown;
            self.x = None;
            self.y = None;
            self.w = None;
            self.h = None;
            Ok(())
        }

        /// From wherever it is to `pane` at `shown`, on `curve`, keeping its
        /// speed.
        fn move_to(
            &mut self,
            dcomp: &Compositor,
            pane: PaneId,
            shown: PixelRect,
            curve: &Curve,
            now: Instant,
        ) -> anyhow::Result<()> {
            let ([x, y, w, h], [vx, vy, vw, vh]) = self.current(now);
            dcomp.name("outline.move");
            if self.pane.is_none() {
                self.show_at_once()?;
            }
            let group = &self.group;
            self.x = dcomp.drive(
                Motion::new(x, shown.x as f64, vx, curve.clone()),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { group.SetOffsetX(a) },
                |v| unsafe { group.SetOffsetX2(v) },
            )?;
            self.y = dcomp.drive(
                Motion::new(y, shown.y as f64, vy, curve.clone()),
                PIXEL_EPSILON,
                0.0,
                |a| unsafe { group.SetOffsetY(a) },
                |v| unsafe { group.SetOffsetY2(v) },
            )?;
            let (along, down) = (
                Motion::new(w, shown.w as f64, vw, curve.clone()),
                Motion::new(h, shown.h as f64, vh, curve.clone()),
            );
            self.edges.drive(dcomp, &self.cards, &along, &down)?;
            // Kept for the next retarget; the riders play the same motions
            self.w = Some(Tween {
                end: along.end(PIXEL_EPSILON),
                motion: along,
                start: now,
                sampled: None,
            });
            self.h = Some(Tween {
                end: down.end(PIXEL_EPSILON),
                motion: down,
                start: now,
                sampled: None,
            });
            self.pane = Some(pane);
            self.shown = shown;
            Ok(())
        }

        fn hide(&mut self) -> anyhow::Result<()> {
            unsafe { self.opacity.SetOpacity2(0.0)? };
            self.alpha = None;
            self.pane = None;
            Ok(())
        }

        fn refit(&mut self, cards: &Cards, now: Instant) -> anyhow::Result<()> {
            self.cards = cards.plan;
            self.edges.fit(&cards.line, &cards.plan)?;
            let ([_, _, w, h], _) = self.current(now);
            self.edges.set(&self.cards, w, h)
        }

        /// The lamp's light now, and how fast it's changing.
        fn light_at(&self, now: Instant) -> (f64, f64) {
            match &self.lit {
                Some((chain, start)) => {
                    let t = now.saturating_duration_since(*start).as_secs_f64();
                    (chain.value(t), chain.velocity(t))
                }
                None => (self.light, 0.0),
            }
        }

        /// LBTerm: its light and held colour now, for an outline left with its tab to keep
        fn look_at(&self, now: Instant) -> OutlineLook {
            let (light, _) = self.light_at(now);
            let (mix, _) = self.mix_at(now);
            OutlineLook {
                light,
                held: (self.tint.is_some() && mix > OPACITY_EPSILON).then_some((self.colour, mix)),
            }
        }

        /// The tint's mix now, and how fast it's changing.
        fn mix_at(&self, now: Instant) -> (f64, f64) {
            match &self.mixing {
                Some((chain, start)) => {
                    let t = now.saturating_duration_since(*start).as_secs_f64();
                    (chain.value(t), chain.velocity(t))
                }
                None => (self.mix, 0.0),
            }
        }

        /// The tint toward `held`'s colour, or back to white without it, on
        /// `curve` from wherever the mix is. The colour itself changes at
        /// once: another modifier added or let go answers in the next frame.
        fn tint(
            &mut self,
            dcomp: &Compositor,
            held: Option<u32>,
            curve: &Curve,
            animate: bool,
            now: Instant,
        ) -> anyhow::Result<()> {
            if self.tint.is_none() {
                if held.is_none() {
                    return Ok(());
                }
                let device: IDCompositionDevice3 =
                    dcomp.device.cast().context("IDCompositionDevice3")?;
                let effect = unsafe { device.CreateColorMatrixEffect()? };
                unsafe {
                    for i in 0..4 {
                        effect.SetMatrixElement2(i, i, 1.0)?;
                    }
                    self.lamp.SetEffect(&effect)?;
                }
                self.tint = Some(effect);
            }
            if let Some(colour) = held {
                self.colour = colour;
            }
            let to = if held.is_some() { 1.0 } else { 0.0 };
            let (from, v) = self.mix_at(now);
            let curve = if animate { curve.clone() } else { Curve::Instant };
            let chain = Chain::new(vec![Motion::new(from, to, v, curve)], OPACITY_EPSILON);
            let mix = chain.segments(STEP, OPACITY_EPSILON);
            let (r, g, b) = lbterm_panel::palette::bytes(self.colour);
            let effect = self.tint.as_ref().unwrap();
            dcomp.name("outline.held");
            for (i, c) in [r, g, b].iter().copied().enumerate() {
                let i = i as i32;
                // Each channel: the template's white fading out, the colour's light coming in
                dcomp.play(
                    &mix.affine(1.0, -1.0),
                    |a| unsafe { effect.SetMatrixElement(i, i, a) },
                    |v| unsafe { effect.SetMatrixElement2(i, i, v) },
                )?;
                dcomp.play(
                    &mix.affine(0.0, c as f64 / 255.0),
                    |a| unsafe { effect.SetMatrixElement(4, i, a) },
                    |v| unsafe { effect.SetMatrixElement2(4, i, v) },
                )?;
            }
            self.mixing = Some((chain, now));
            self.mix = to;
            Ok(())
        }

        /// The light for what the window says about its keyboard: at rest
        /// while it has it, out without it, and a pulse up to full and back
        /// as it arrives from outside, each from wherever the light is. Full
        /// and tinted while modifiers are held (`tint`).
        /// True when the tree changed.
        fn light(
            &mut self,
            dcomp: &Compositor,
            keyboard: Keyboard,
            motion: &MotionSettings,
            animate: bool,
            now: Instant,
        ) -> anyhow::Result<bool> {
            let held = keyboard.held.filter(|_| keyboard.has);
            let rest = if !keyboard.has {
                0.0
            } else if held.is_some() {
                1.0
            } else {
                lbterm_panel::palette::outline_rest_opacity()
            };
            let was = self.keyboard.replace(keyboard);
            let was_held = was.and_then(|was| was.held.filter(|_| was.has));
            let arrived = keyboard.has
                && was.map_or(false, |was| was.arrivals != keyboard.arrivals);
            let turned = was.map_or(true, |was| was.has != keyboard.has);
            // Held or let go: the light goes up on `arrive_in` and back on `arrive_out`
            let pressed = was_held.is_some() != held.is_some();
            let mut changed = false;
            if was_held != held {
                let curve = match (turned, held) {
                    (true, _) => &motion.dim,
                    (false, Some(_)) => &motion.arrive_in,
                    (false, None) => &motion.arrive_out,
                };
                self.tint(dcomp, held, curve, animate && was.is_some(), now)?;
                changed = true;
            }
            if !arrived && !turned && !pressed {
                return Ok(changed);
            }
            let lamp = &self.lamp;
            // The window's first word, or no motion: straight there
            if was.is_none() || !animate {
                unsafe { lamp.SetOpacity2(rest as f32)? };
                self.lit = None;
                self.light = rest;
                return Ok(true);
            }
            let (from, v) = self.light_at(now);
            let chain = if arrived {
                Chain::pulse(
                    from,
                    v,
                    1.0,
                    rest,
                    motion.arrive_in.clone(),
                    motion.arrive_out.clone(),
                    OPACITY_EPSILON,
                )
            } else if pressed && !turned {
                let curve = if held.is_some() { &motion.arrive_in } else { &motion.arrive_out };
                Chain::new(vec![Motion::new(from, rest, v, curve.clone())], OPACITY_EPSILON)
            } else {
                Chain::new(vec![Motion::new(from, rest, v, motion.dim.clone())], OPACITY_EPSILON)
            };
            dcomp.name(if arrived {
                "outline.arrive"
            } else if pressed && !turned {
                "outline.held"
            } else {
                "outline.dim"
            });
            dcomp.play(
                &chain.segments(STEP, OPACITY_EPSILON),
                |a| unsafe { lamp.SetOpacity(a) },
                |v| unsafe { lamp.SetOpacity2(v) },
            )?;
            self.lit = Some((chain, now));
            self.light = rest;
            Ok(true)
        }
    }

    /// LBTerm: what of a tab's active outline fades out with its panes once the tab is left
    /// (`lbterm_motion::outline`): the parts the window's outline, on the next tab's pane at
    /// once, doesn't draw too, as pieces of the line template where the outline was when the tab
    /// was left, in a lamp at the light it had then, its group fading on `tab_switch` from the
    /// share its panes start from. Under the window's outline and over the panes, where the
    /// outline was. Until 2026-10-30 it went at once while its panes faded (Lachlan's 2a).
    struct LeftOutline {
        lamp: IDCompositionVisual3,
        /// The light and colour the lamp has, held still from the window's outline
        look: OutlineLook,
        /// The held modifiers' colour the lamp had, kept on it
        _tint: Option<IDCompositionColorMatrixEffect>,
        group: IDCompositionVisual2,
        opacity: IDCompositionVisual3,
        pieces: Vec<Piece>,
        outline: lbterm_motion::outline::Outline,
        /// The panes its tab had on screen: one back on screen is that tab switched back to
        panes: Vec<PaneId>,
        alpha: Option<Tween>,
        done: Instant,
    }

    /// LBTerm: an outline's light and the held modifiers' colour mixed into it, as a
    /// `LeftOutline` holds them still from the window's outline at the moment its tab was left
    #[derive(Clone, Copy)]
    struct OutlineLook {
        light: f64,
        /// The colour and its mix, when a modifier's colour was mixed in at all
        held: Option<(u32, f64)>,
    }

    impl LeftOutline {
        /// `outline` in `look`, fading out from `from` on `curve`, or as `going` goes from 1 to 0
        /// (as a tab switched back to comes up); under the window's outline `focus`
        fn new(
            dcomp: &Compositor,
            focus: &FocusOutline,
            look: OutlineLook,
            cards: &Cards,
            outline: lbterm_motion::outline::Outline,
            from: f64,
            curve: &Curve,
            going: Option<&Segments>,
            panes: Vec<PaneId>,
            now: Instant,
        ) -> anyhow::Result<Self> {
            let lamp: IDCompositionVisual3 = unsafe { dcomp.device.CreateVisual()? }
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            let light = look.light;
            let tint = if let Some((colour, mix)) = look.held {
                // As `FocusOutline::tint` mixes it, held still where it is
                let device: IDCompositionDevice3 = dcomp.device.cast().context("IDCompositionDevice3")?;
                let effect = unsafe { device.CreateColorMatrixEffect()? };
                let (r, g, b) = lbterm_panel::palette::bytes(colour);
                unsafe {
                    effect.SetMatrixElement2(3, 3, 1.0)?;
                    for (i, c) in [r, g, b].iter().copied().enumerate() {
                        let i = i as i32;
                        effect.SetMatrixElement2(i, i, (1.0 - mix) as f32)?;
                        effect.SetMatrixElement2(4, i, (mix * c as f64 / 255.0) as f32)?;
                    }
                    lamp.SetEffect(&effect)?;
                }
                Some(effect)
            } else {
                None
            };
            let group = unsafe { dcomp.device.CreateVisual()? };
            let opacity: IDCompositionVisual3 = group
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            unsafe {
                lamp.SetOpacity2(light as f32)?;
                lamp.AddVisual(&group, false, None::<&IDCompositionVisual>)?;
                group.SetOffsetX2(outline.rect.x as f32)?;
                group.SetOffsetY2(outline.rect.y as f32)?;
                dcomp.focus.AddVisual(&lamp, false, &focus.lamp)?;
            }
            let mut left = Self {
                lamp,
                look,
                _tint: tint,
                group,
                opacity,
                pieces: vec![],
                outline,
                panes,
                alpha: None,
                done: now,
            };
            left.fit(dcomp, cards)?;
            dcomp.name("outline.dissolve");
            let opacity = &left.opacity;
            left.alpha = match going {
                Some(going) => {
                    let segments = going.affine(0.0, from);
                    dcomp.play(&segments, |a| unsafe { opacity.SetOpacity(a) }, |v| unsafe { opacity.SetOpacity2(v) })?;
                    Some(Tween::sampled(segments, now))
                }
                None => dcomp.drive(
                    Motion::new(from, 0.0, 0.0, curve.clone()),
                    OPACITY_EPSILON,
                    0.0,
                    |a| unsafe { opacity.SetOpacity(a) },
                    |v| unsafe { opacity.SetOpacity2(v) },
                )?,
            };
            left.done = left.alpha.as_ref().map_or(now, Tween::finishes);
            Ok(left)
        }

        /// A piece of the line template for each run and each corner's part of its outline,
        /// where that part of the whole outline round its rectangle is drawn (as `Edges` cuts
        /// them, still). A corner's box is cut in three: the strip on its top or bottom line's
        /// rows, the strip on its left or right line's columns below or above that, and the
        /// curve in the rest (`lbterm_motion::outline::Parts`)
        fn fit(&mut self, dcomp: &Compositor, cards: &Cards) -> anyhow::Result<()> {
            use lbterm_motion::outline::{Corner, Side};
            let plan = &cards.plan;
            let (wide, tall) = (&cards.line.wide_content, &cards.line.tall_content);
            let (s, c, ww) = (plan.size as f32, plan.corner as f32, plan.wide.w as f32);
            let t = (plan.line as f32).min(c);
            let r = self.outline.rect;
            let (gx, gy, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
            // Each piece: the template, its clip of it, and where the piece sits in the group
            let mut want: Vec<(&IUnknown, [f32; 4], (f32, f32))> = vec![];
            for (side, runs) in Side::ALL.iter().zip(&self.outline.runs) {
                for &(a, b) in runs {
                    let (a, b) = (a as f32, b as f32);
                    want.push(match side {
                        Side::Top => (wide, [a - gx, 0.0, b - gx, c], (0.0, 0.0)),
                        Side::Bottom => (wide, [a - gx, s - c, b - gx, s], (0.0, h - s)),
                        Side::Left => (tall, [0.0, a - gy, c, b - gy], (0.0, 0.0)),
                        Side::Right => (tall, [s - c, a - gy, s, b - gy], (w - s, 0.0)),
                    });
                }
            }
            for (corner, parts) in Corner::ALL.iter().zip(self.outline.corners) {
                // The corner's box in the wide template, where it sits, and which edges of the
                // box its lines lie along
                let ([x0, y0, x1, y1], at, top, left) = match corner {
                    Corner::TopLeft => ([0.0, 0.0, c, c], (0.0, 0.0), true, true),
                    Corner::TopRight => ([ww - c, 0.0, ww, c], (w - ww, 0.0), true, false),
                    Corner::BottomLeft => ([0.0, s - c, c, s], (0.0, h - s), false, true),
                    Corner::BottomRight => ([ww - c, s - c, ww, s], (w - ww, h - s), false, false),
                };
                let (ry0, ry1) = if top { (y0 + t, y1) } else { (y0, y1 - t) };
                if parts.horizontal {
                    let (hy0, hy1) = if top { (y0, y0 + t) } else { (y1 - t, y1) };
                    want.push((wide, [x0, hy0, x1, hy1], at));
                }
                if parts.vertical {
                    let (vx0, vx1) = if left { (x0, x0 + t) } else { (x1 - t, x1) };
                    want.push((wide, [vx0, ry0, vx1, ry1], at));
                }
                if parts.curve {
                    let (cx0, cx1) = if left { (x0 + t, x1) } else { (x0, x1 - t) };
                    want.push((wide, [cx0, ry0, cx1, ry1], at));
                }
            }
            while self.pieces.len() < want.len() {
                self.pieces.push(Piece::new(dcomp, &self.group)?);
            }
            for extra in self.pieces.drain(want.len()..) {
                unsafe { self.group.RemoveVisual(&extra.visual)? };
            }
            for (piece, (content, [x0, y0, x1, y1], (ox, oy))) in self.pieces.iter().zip(want) {
                piece.fit(content, x0, y0, x1, y1)?;
                unsafe {
                    piece.visual.SetOffsetX2(ox)?;
                    piece.visual.SetOffsetY2(oy)?;
                }
            }
            Ok(())
        }

        fn current_alpha(&self, now: Instant) -> f64 {
            at(&self.alpha, 0.0, now).0
        }
    }

    /// The tab panel: the body group (glow, body, words), clipped at the
    /// seam, and the cap group (glow, cap), moved along with the edge.
    /// Every surface is drawn as the open panel; the seam and the cap's
    /// offset decide how much shows.
    ///
    /// Over them, the light that runs round the ring once as it opens: its
    /// halo and middle drawn where it starts, the group carried along the
    /// ring's middle line.
    /// One turn of the waiting arc
    const SPIN_PERIOD: f64 = 1.4;

    /// A turning arc on one pad: the arc template's swapchain, offset to
    /// the pad's corner and turning about its middle in DWM, for as long as
    /// the pad waits.
    struct Spinner {
        visual: IDCompositionVisual2,
        rotate: IDCompositionRotateTransform,
        rect: PixelRect,
        /// Whether it turns (the plan's `spin`): held still with motion off or while the window
        /// can't be seen
        spinning: bool,
    }

    impl Spinner {
        /// A turn every SPIN_PERIOD, for ever, DWM playing it, from upright; or still, upright
        fn turn(&mut self, dcomp: &Compositor, spin: bool) -> anyhow::Result<()> {
            if spin {
                let animation = unsafe { dcomp.device.CreateAnimation()? };
                unsafe {
                    animation.AddCubic(0.0, 0.0, (360.0 / SPIN_PERIOD) as f32, 0.0, 0.0)?;
                    animation.AddRepeat(SPIN_PERIOD, SPIN_PERIOD)?;
                    self.rotate.SetAngle(&animation)?;
                }
            } else {
                unsafe { self.rotate.SetAngle2(0.0)? };
            }
            self.spinning = spin;
            Ok(())
        }
    }

    /// One of the Conductor's outlines on a pad (render/conductor.rs): its template's swapchain,
    /// offset to the pad, its hues turned by DWM for as long as it shows; and the light going
    /// round it, a disc DWM carries along the pad's outline lap after lap.
    struct Ring {
        visual: IDCompositionVisual2,
        /// Keeps the hue rotation alive while it's on the visual
        _effect: Option<IDCompositionHueRotationEffect>,
        light: Option<IDCompositionVisual2>,
        plan: super::RingPlan,
        /// Whether its hues turn and its light goes round (the plan's `spin`): held still with
        /// motion off or while the window can't be seen
        spinning: bool,
        /// The Conductor's pad's (the plan's `fade`): where its opacity goes, 1 lit and 0 fading
        /// out, `None` before it's been told; its opacity with no fade playing (a tab's is 1, the
        /// visual's own), and the fade that plays, kept to turn round from where it is
        target: Option<f64>,
        rest: f64,
        alpha: Option<Tween>,
    }

    impl Ring {
        /// How much of it shows at `now`, and how fast that's changing
        fn opacity(&self, now: Instant) -> (f64, f64) {
            at(&self.alpha, self.rest, now)
        }

        /// Fades it to what `want`'s fade says, from where it is and keeping its speed, unless it's
        /// on its way there already; a ring without a fade is left whole
        fn fade_to(&mut self, dcomp: &Compositor, want: &super::RingPlan, now: Instant) -> anyhow::Result<()> {
            let Some(fade) = &want.fade else {
                return Ok(());
            };
            let to = if fade.lit { 1.0 } else { 0.0 };
            if self.target == Some(to) {
                return Ok(());
            }
            let (from, v0) = self.opacity(now);
            let visual3: IDCompositionVisual3 = self.visual.cast().context("IDCompositionVisual3")?;
            dcomp.name("conductor.outline.fade");
            self.alpha = dcomp.drive(
                Motion::new(from, to, v0, fade.curve.clone()),
                OPACITY_EPSILON,
                0.0,
                |a| unsafe { visual3.SetOpacity(a) },
                |v| unsafe { visual3.SetOpacity2(v) },
            )?;
            self.rest = to;
            self.target = Some(to);
            Ok(())
        }
    }

    /// The two surfaces a row shows its slot of: the rows' (the rail) and the row words' (the
    /// open panel)
    #[derive(Clone)]
    struct RowContent {
        rail: IUnknown,
        words: IUnknown,
    }

    /// The groups a row's two visuals go in: over the body, and under the words' fade
    struct RowGroups {
        rail: IDCompositionVisual2,
        words: IDCompositionVisual2,
    }

    /// How far a row's new look is in when its old one starts going (`RowVisual::dissolve`).
    /// The rows' surfaces are clear where nothing's drawn, so the old over the new, fading,
    /// hides only what goes (a row's open fill showed at once), and the two fading at once dim
    /// what they have in common, a quarter of the way to what's under them halfway. Holding the
    /// old until the new is this far in keeps that under 5% (a level or two on the rail): with
    /// `swap` the old starts going at 47 ms and is gone by 250, with `hover` at 107 ms.
    const DISSOLVE_IN: f64 = 0.75;

    /// One tab's row: a visual for its part of the rail and one for its open part (under the
    /// words' fade), moving together: sliding to the slot when it moves, growing in, fading
    /// out. Each holds a face per surface of the rows' pair, showing it through the row's slot,
    /// under the row's progress bars. The front surface's face shows and the other's is out,
    /// unless the row's look has just changed: then the other's, its surface kept with the
    /// frame before, holds the old look as the new one comes in (its ghost).
    struct RowVisual {
        plan: super::RowPlan,
        visual: IDCompositionVisual3,
        clip: IDCompositionRectangleClip,
        words: IDCompositionVisual3,
        words_clip: IDCompositionRectangleClip,
        /// Each surface's faces: in the rail's visual, and in the open part's
        faces: [[IDCompositionVisual3; 2]; 2],
        /// Each surface's faces' opacity: going there, or resting there
        face_alpha: [Option<Tween>; 2],
        face_rest: [f64; 2],
        /// How it looked, and where, when the surfaces were last swapped: what the kept ones
        /// hold of it (none for a row that came after)
        kept: Option<(super::RowLooks, PixelRect)>,
        /// How far it shows from its slot, sliding there
        dy: Option<Tween>,
        /// Its old look going, as its new one comes in
        ghost: Option<Ghost>,
    }

    /// A row's old look: the faces of the surface that holds it, offset so the slot the row had
    /// there lands on its slot now, going once the new look is `DISSOLVE_IN`
    struct Ghost {
        surface: usize,
        from: PixelRect,
        /// What it shows, so a row going back to it can be told
        looks: super::RowLooks,
        done: Instant,
    }

    impl RowVisual {
        /// Showing `content[front]`, with a face on each
        fn new(dcomp: &Compositor, content: &[RowContent; 2], front: usize) -> anyhow::Result<Self> {
            let visual = || -> anyhow::Result<IDCompositionVisual3> {
                Ok(unsafe { dcomp.device.CreateVisual()? }
                    .cast()
                    .context("IDCompositionVisual3 (Windows 10 or later)")?)
            };
            let clipped = || -> anyhow::Result<(IDCompositionVisual3, IDCompositionRectangleClip)> {
                let visual = visual()?;
                let clip = unsafe { dcomp.device.CreateRectangleClip()? };
                unsafe { visual.SetClip(&clip)? };
                Ok((visual, clip))
            };
            let (rail, clip) = clipped()?;
            let (words, words_clip) = clipped()?;
            let mut row = Self {
                plan: super::RowPlan {
                    key: usize::MAX,
                    slot: PixelRect::default(),
                    look: 0,
                    words_look: 0,
                    hovered: false,
                    was: None,
                },
                visual: rail,
                clip,
                words,
                words_clip,
                faces: [[visual()?, visual()?], [visual()?, visual()?]],
                face_alpha: [None, None],
                face_rest: [0.0, 0.0],
                kept: None,
                dy: None,
                ghost: None,
            };
            for (surface, content) in content.iter().enumerate() {
                row.bind(surface, content)?;
                // Before any progress bar, so under them all
                for (face, parent) in row.faces[surface].iter().zip([&row.visual, &row.words]) {
                    unsafe { parent.AddVisual(face, false, None::<&IDCompositionVisual>)? };
                }
            }
            row.front(front)?;
            Ok(row)
        }

        fn both(&self) -> [(&IDCompositionVisual3, &IDCompositionRectangleClip); 2] {
            [(&self.visual, &self.clip), (&self.words, &self.words_clip)]
        }

        /// Into `groups`, at the top, or the bottom with `bottom`
        fn attach(&self, groups: &RowGroups, bottom: bool) -> windows58::core::Result<()> {
            unsafe {
                groups.rail.AddVisual(&self.visual, bottom, None::<&IDCompositionVisual>)?;
                groups.words.AddVisual(&self.words, bottom, None::<&IDCompositionVisual>)
            }
        }

        fn detach(&self, groups: &RowGroups) -> windows58::core::Result<()> {
            unsafe {
                groups.rail.RemoveVisual(&self.visual)?;
                groups.words.RemoveVisual(&self.words)
            }
        }

        /// `surface`'s faces onto `content`, its new swapchains
        fn bind(&self, surface: usize, content: &RowContent) -> windows58::core::Result<()> {
            unsafe {
                self.faces[surface][0].SetContent(&content.rail)?;
                self.faces[surface][1].SetContent(&content.words)
            }
        }

        /// The panel's surface through `slot`, `dy` from it
        fn show(&self, surface: PixelRect, slot: PixelRect, dy: f32) -> windows58::core::Result<()> {
            for (visual, clip) in self.both() {
                unsafe {
                    visual.SetOffsetX2(surface.x as f32)?;
                    visual.SetOffsetY2(surface.y as f32 + dy)?;
                    clip.SetLeft2(0.0)?;
                    clip.SetRight2(surface.w as f32)?;
                    clip.SetTop2((slot.y - surface.y) as f32)?;
                    clip.SetBottom2((slot.y - surface.y) as f32 + slot.h as f32)?;
                }
            }
            self.follow(slot)
        }

        /// `surface`'s faces at `alpha` at once
        fn face_at(&mut self, surface: usize, alpha: f64) -> windows58::core::Result<()> {
            self.face_alpha[surface] = None;
            self.face_rest[surface] = alpha;
            for face in &self.faces[surface] {
                unsafe { face.SetOpacity2(alpha as f32)? };
            }
            Ok(())
        }

        /// `surface`'s faces from where they are to `to` on `curve`, `delay` seconds from now
        fn face_to(&mut self, dcomp: &Compositor, surface: usize, to: f64, curve: &Curve, delay: f64, now: Instant) -> anyhow::Result<()> {
            let (from, v) = at(&self.face_alpha[surface], self.face_rest[surface], now);
            let motion = Motion::new(from, to, v, curve.clone());
            self.face_rest[surface] = to;
            for face in &self.faces[surface] {
                self.face_alpha[surface] = dcomp.drive(
                    motion.clone(),
                    OPACITY_EPSILON,
                    delay,
                    |a| unsafe { face.SetOpacity(a) },
                    |v| unsafe { face.SetOpacity2(v) },
                )?;
            }
            Ok(())
        }

        /// `surface` alone shows, at once: the new front's, or the kept one's for a row going.
        /// It's drawn where the row is, and the other's go out, its ghost with them
        fn front(&mut self, surface: usize) -> windows58::core::Result<()> {
            self.ghost = None;
            self.face_at(surface, 1.0)?;
            self.face_at(1 - surface, 0.0)?;
            for face in &self.faces[surface] {
                unsafe { face.SetOffsetY2(0.0)? };
            }
            Ok(())
        }

        /// The front surface, `coming`, drawn with the row's new look, comes in from where its
        /// faces are, over the other's, which hold its old look (drawn there at `from`, and
        /// `looks`) and go once the new is `DISSOLVE_IN`, from where they are: a turn back
        /// carries on from where it got to. `fresh` starts it from the old look alone, which
        /// both surfaces hold as it shows now.
        fn dissolve(
            &mut self,
            dcomp: &Compositor,
            coming: usize,
            from: PixelRect,
            looks: super::RowLooks,
            curve: &Curve,
            fresh: bool,
            now: Instant,
        ) -> anyhow::Result<()> {
            let going = 1 - coming;
            if fresh {
                self.face_at(coming, 0.0)?;
                self.face_at(going, 1.0)?;
            }
            for (part, parent) in [&self.visual, &self.words].iter().enumerate() {
                let (face, under) = (&self.faces[coming][part], &self.faces[going][part]);
                unsafe {
                    parent.RemoveVisual(face)?;
                    parent.AddVisual(face, true, under)?;
                    face.SetOffsetY2(0.0)?;
                }
            }
            let (alpha, _) = at(&self.face_alpha[coming], self.face_rest[coming], now);
            let hold = if alpha < DISSOLVE_IN {
                Motion::new(alpha, 1.0, 0.0, curve.clone()).reaches((DISSOLVE_IN - alpha) / (1.0 - alpha), OPACITY_EPSILON)
            } else {
                0.0
            };
            let (left, _) = at(&self.face_alpha[going], self.face_rest[going], now);
            self.face_to(dcomp, coming, 1.0, curve, 0.0, now)?;
            self.face_to(dcomp, going, 0.0, curve, hold, now)?;
            let end = hold + Motion::new(left, 0.0, 0.0, curve.clone()).end(OPACITY_EPSILON);
            self.ghost = Some(Ghost {
                surface: going,
                from,
                looks,
                done: now + Duration::from_secs_f64(end),
            });
            self.follow(self.plan.slot)?;
            Ok(())
        }

        /// Its old look gone: the front's alone, at once
        fn drop_ghost(&mut self) -> windows58::core::Result<()> {
            match self.ghost.as_ref().map(|g| 1 - g.surface) {
                Some(front) => self.front(front),
                None => Ok(()),
            }
        }

        /// Its ghost's slot onto the row's `slot`
        fn follow(&self, slot: PixelRect) -> windows58::core::Result<()> {
            if let Some(ghost) = &self.ghost {
                for face in &self.faces[ghost.surface] {
                    unsafe { face.SetOffsetY2((slot.y - ghost.from.y) as f32)? };
                }
            }
            Ok(())
        }

        /// Both visuals' opacity on `motion`, from its start
        fn fade(&self, dcomp: &Compositor, motion: Motion) -> anyhow::Result<()> {
            for (visual, _) in self.both() {
                unsafe { visual.SetOpacity2(motion.from as f32)? };
                dcomp.drive(
                    motion.clone(),
                    OPACITY_EPSILON,
                    0.0,
                    |a| unsafe { visual.SetOpacity(a) },
                    |v| unsafe { visual.SetOpacity2(v) },
                )?;
            }
            Ok(())
        }

        /// Both clips' bottom edges on `motion`
        fn open(&self, dcomp: &Compositor, motion: Motion) -> anyhow::Result<()> {
            for (_, clip) in self.both() {
                dcomp.drive(
                    motion.clone(),
                    PIXEL_EPSILON,
                    0.0,
                    |a| unsafe { clip.SetBottom(a) },
                    |v| unsafe { clip.SetBottom2(v) },
                )?;
            }
            Ok(())
        }

        fn dy_at(&self, now: Instant) -> (f64, f64) {
            at(&self.dy, 0.0, now)
        }

        /// From `from` off its slot (moving at `v`) to the slot, on `curve`, for a surface whose
        /// top is `top`. DWM plays the offsets itself; the tween keeps how far off the slot.
        fn slide(&mut self, dcomp: &Compositor, top: f64, from: f64, v: f64, curve: &Curve) -> anyhow::Result<()> {
            dcomp.name("rail.row.slide");
            drive_off(dcomp, &self.words, top, from, v, curve)?;
            self.dy = drive_off(dcomp, &self.visual, top, from, v, curve)?;
            Ok(())
        }
    }

    /// `visual`'s y offset from `top + from` to `top`, moving at `v`, on `curve`; the tween
    /// returned keeps how far it is from `top`, for the next retarget.
    fn drive_off(
        dcomp: &Compositor,
        visual: &IDCompositionVisual2,
        top: f64,
        from: f64,
        v: f64,
        curve: &Curve,
    ) -> anyhow::Result<Option<Tween>> {
        let tween = dcomp.drive(
            Motion::new(top + from, top, v, curve.clone()),
            PIXEL_EPSILON,
            0.0,
            |a| unsafe { visual.SetOffsetY(a) },
            |y| unsafe { visual.SetOffsetY2(y) },
        )?;
        Ok(tween.map(|mut t| {
            t.motion = Motion::new(from, 0.0, v, curve.clone());
            t
        }))
    }

    /// A row's progress bar on the open panel: a visual showing the whole fill through a clip
    /// as long as the program says. A row can have several (the hub's guesses for its subagents).
    struct ProgressBar {
        key: usize,
        /// Which of its row's bars
        bar: u8,
        visual: IDCompositionVisual2,
        clip: IDCompositionRectangleClip,
        /// Its row's words visual, which it rides in
        parent: IDCompositionVisual3,
        track: PixelRect,
        fraction: f64,
        /// The clip's right edge, while it glides
        right: Option<Tween>,
    }

    /// The tab panel's wave (lbterm_panel::wave): under the rest of the panel, a group clipped to
    /// the room it washes over, the clip's left edge riding on the ring's right edge, holding the
    /// water and its spray, each drawn once where it ends, which DWM carries a cell at a time
    /// (`lbterm_motion::wash`). Shown only while it runs.
    struct WaveLayers {
        group: IDCompositionVisual2,
        opacity: IDCompositionVisual3,
        clip: IDCompositionRectangleClip,
        water: Layer,
        water_opacity: IDCompositionVisual3,
        spray: Layer,
        spray_opacity: IDCompositionVisual3,
        plan: WavePlan,
        /// The look their surfaces hold, once drawn
        drawn: Option<u64>,
        /// This frame draws them
        redraw: bool,
        /// The clip's left edge riding on the ring's, while it runs
        edge: Option<Tween>,
        /// When it set off, the water's and the spray's offsets, and how shown it all is: where
        /// it is when it's turned round
        running: Option<(Instant, Segments, Segments, Segments)>,
    }

    /// `segments` moved by `by`.
    fn shifted(segments: &Segments, by: f32) -> Segments {
        Segments {
            cubics: segments
                .cubics
                .iter()
                .map(|c| {
                    let [c0, c1, c2, c3] = c.coefficients;
                    Cubic {
                        begin: c.begin,
                        coefficients: [c0 + by, c1, c2, c3],
                    }
                })
                .collect(),
            end: segments.end,
            end_value: segments.end_value + by,
        }
    }

    impl WaveLayers {
        fn new(
            dcomp: &Compositor,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: &WavePlan,
        ) -> anyhow::Result<Self> {
            let group = unsafe { dcomp.device.CreateVisual()? };
            // Behind everything else the panel shows, so the edge covers it as it arrives
            unsafe { dcomp.panel.AddVisual(&group, true, None::<&IDCompositionVisual>)? };
            let layer = |rect: PixelRect| -> anyhow::Result<Layer> {
                let visual = unsafe { dcomp.device.CreateVisual()? };
                unsafe { group.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                Layer::new(instance, device, visual, base, wgpu::CompositeAlphaMode::PreMultiplied, rect)
            };
            let water = layer(plan.rect)?;
            let spray = layer(plan.spray)?;
            let clip = unsafe { dcomp.device.CreateRectangleClip()? };
            unsafe { group.SetClip(&clip)? };
            let opacity = |visual: &IDCompositionVisual2| -> anyhow::Result<IDCompositionVisual3> {
                visual.cast().context("IDCompositionVisual3 (Windows 10 or later)")
            };
            let wave = Self {
                opacity: opacity(&group)?,
                water_opacity: opacity(&water.visual)?,
                spray_opacity: opacity(&spray.visual)?,
                group,
                clip,
                water,
                spray,
                plan: plan.clone(),
                drawn: None,
                redraw: false,
                edge: None,
                running: None,
            };
            wave.rest()?;
            Ok(wave)
        }

        /// Hidden, its clip shut on the band's right edge and each drawing where it ends.
        fn rest(&self) -> anyhow::Result<()> {
            let band = self.plan.band;
            unsafe {
                self.opacity.SetOpacity2(0.0)?;
                self.clip.SetTop2(band.y as f32)?;
                self.clip.SetBottom2((band.y + band.h as i32) as f32)?;
                self.clip.SetRight2((band.x + band.w as i32) as f32)?;
                self.clip.SetLeft2((band.x + band.w as i32) as f32)?;
                self.water.visual.SetOffsetX2(self.plan.rect.x as f32)?;
                self.spray.visual.SetOffsetX2(self.plan.spray.x as f32)?;
            }
            Ok(())
        }

        fn place(&mut self, device: &wgpu::Device, plan: &WavePlan) -> anyhow::Result<()> {
            self.water.place(device, plan.rect)?;
            self.spray.place(device, plan.spray)?;
            self.plan = plan.clone();
            self.edge = None;
            self.running = None;
            self.rest()
        }

        /// The panel opening from `from` to `to` on the seam, moving at `v`: the clip's left edge
        /// rides on the ring's right edge, the water runs out ahead of it and on past its stop a
        /// cell at a time, then pulls back as it fades while its spray scatters on.
        fn run(&mut self, dcomp: &Compositor, from: f64, to: f64, v: f64, plan: &PanelPlan, now: Instant) -> anyhow::Result<()> {
            let wave = &self.plan;
            // From the seam to the ring's right edge
            let ring = (wave.stop - plan.seam) as f64;
            let edge = Motion::new(from + ring, to + ring, v, plan.edge.clone());
            let settings = lbterm_motion::wash::Settings {
                lead: wave.lead,
                beyond: wave.beyond,
                cell: wave.cell,
                recede: wave.recede,
                drift: wave.drift,
                fade: wave.fade.clone(),
                rise: wave.rise.clone(),
                splash: wave.splash.clone(),
            };
            let Some(wash) = lbterm_motion::wash::wash(&edge, &settings, PIXEL_EPSILON) else {
                self.rest()?;
                return Ok(());
            };
            let clip = &self.clip;
            dcomp.name("rail.wave");
            self.edge = dcomp.drive(edge.clone(), PIXEL_EPSILON, 0.0, |a| unsafe { clip.SetLeft(a) }, |x| unsafe {
                clip.SetLeft2(x)
            })?;
            // Each from where it's drawn
            let water_x = shifted(&wash.wave_x, wave.rect.x as f32);
            let spray_x = shifted(&wash.splash_x, wave.spray.x as f32);
            let (water, spray) = (&self.water.visual, &self.spray.visual);
            dcomp.play(&water_x, |a| unsafe { water.SetOffsetX(a) }, |x| unsafe { water.SetOffsetX2(x) })?;
            dcomp.play(&spray_x, |a| unsafe { spray.SetOffsetX(a) }, |x| unsafe { spray.SetOffsetX2(x) })?;
            let (water, spray) = (&self.water_opacity, &self.spray_opacity);
            dcomp.play(&wash.wave_opacity, |a| unsafe { water.SetOpacity(a) }, |o| unsafe { water.SetOpacity2(o) })?;
            dcomp.play(&wash.splash_opacity, |a| unsafe { spray.SetOpacity(a) }, |o| unsafe {
                spray.SetOpacity2(o)
            })?;
            // All of it shown at once, until the last of the spray has gone
            let shown = Segments {
                cubics: vec![Cubic {
                    begin: 0.0,
                    coefficients: [1.0, 0.0, 0.0, 0.0],
                }],
                end: wash.end.max(edge.end(PIXEL_EPSILON)),
                end_value: 0.0,
            };
            let opacity = &self.opacity;
            dcomp.play(&shown, |a| unsafe { opacity.SetOpacity(a) }, |o| unsafe { opacity.SetOpacity2(o) })?;
            self.running = Some((now, water_x, spray_x, shown));
            Ok(())
        }

        /// The panel closing, or turned round as it opens: whatever's left of the wave stays
        /// where it is and fades on `fade`.
        fn stop(&mut self, dcomp: &Compositor, fade: &Curve, now: Instant) -> anyhow::Result<()> {
            let edge = self.edge.take();
            let Some((start, water_x, spray_x, shown)) = self.running.take() else {
                return Ok(());
            };
            let t = now.saturating_duration_since(start).as_secs_f64();
            if t >= shown.end {
                return Ok(());
            }
            let band = self.plan.band;
            let (left, _) = at(&edge, (band.x + band.w as i32) as f64, now);
            unsafe {
                self.clip.SetLeft2(left as f32)?;
                // Held on the steps they're on
                self.water.visual.SetOffsetX2(water_x.value(t) as f32)?;
                self.spray.visual.SetOffsetX2(spray_x.value(t) as f32)?;
            }
            let opacity = &self.opacity;
            // What DWM played of it ends here, its numbers set where they are: the state file and
            // the budget stop counting its run to the end it had (InFlight::cut), and the fade, if
            // there's anything to fade, counts on its own
            dcomp.cut("rail.wave", now);
            dcomp.name("rail.wave");
            dcomp.drive(
                Motion::new(shown.value(t), 0.0, 0.0, fade.clone()),
                OPACITY_EPSILON,
                0.0,
                |a| unsafe { opacity.SetOpacity(a) },
                |o| unsafe { opacity.SetOpacity2(o) },
            )?;
            Ok(())
        }
    }

    struct PanelLayers {
        /// The wave washing ahead of the edge as it opens, with `lbterm_tab_panel.wave` on
        wave: Option<WaveLayers>,
        /// Its seam clip is set as the panel opens; the turning arcs ride in it
        body_group: IDCompositionVisual2,
        /// The tabs' rows (render/tab_panel.rs): two surfaces drawn into by turns, never seen
        /// themselves (their group is clipped to nothing), and a visual per row over the body
        /// showing the front one through its slot. The other keeps the last frame drawn before
        /// a row left or changed its look, which that row's old self shows as it fades
        rows: [Layer; 2],
        rows_content: [IUnknown; 2],
        /// The rows' open parts (render/tab_panel.rs: the long highlight, the words, the chips,
        /// the progress track), drawn and kept the same way, shown by each row's second visual
        /// in `row_words_group`, under the words' fade
        row_words: [Layer; 2],
        row_words_content: [IUnknown; 2],
        row_words_group: IDCompositionVisual2,
        front: usize,
        _rows_hidden: IDCompositionVisual2,
        rows_group: IDCompositionVisual2,
        row_visuals: Vec<RowVisual>,
        /// Rows gone, their old selves fading out, and when each is done
        old_rows: Vec<(RowVisual, Instant)>,
        /// The open tab's ring and bar, drawn where its row is, over the rows; the visual
        /// glides there from where the last one was
        marker: Layer,
        marker_at: Option<(usize, i32)>,
        marker_dy: Option<Tween>,
        /// The row a drag held last frame, and whether the marker went with it
        dragging: Option<usize>,
        marker_held: bool,
        /// The rows' progress fills drawn whole into a surface never seen itself, and a visual
        /// per bar over the words showing as much of its fill as the program says
        progress: Layer,
        progress_content: IUnknown,
        bars: Vec<ProgressBar>,
        /// The Conductor's outline templates, spectrum and iridescent, and its light's, drawn
        /// once a frame into surfaces never seen themselves (their group is clipped to nothing),
        /// and the outlines that show them, in a group of their own (`rings_group`)
        ring_spectrum: Layer,
        ring_iris: Layer,
        ring_light: Layer,
        _ring_group: IDCompositionVisual2,
        rings: Vec<Ring>,
        /// The outlines themselves, over the body and the cap and unclipped: an outline's glow
        /// reaches past the seam, and the pads never move
        rings_group: IDCompositionVisual2,
        /// The arc template, drawn once a frame into a surface that's never
        /// seen itself (its group is clipped to nothing), and the arcs that
        /// show it
        spinner: Layer,
        _spinner_group: IDCompositionVisual2,
        spinners: Vec<Spinner>,
        /// LBTerm: the white arc's template, and the arc on the Starting row's pad that shows it
        start_spinner: Layer,
        start: Option<Spinner>,
        cap_group: IDCompositionVisual2,
        spot_group: IDCompositionVisual2,
        spot_opacity: IDCompositionVisual3,
        spot: Layer,
        spot_core: Layer,
        /// The light's opacity since it set off, while it runs
        sweep: Option<(Instant, Segments)>,
        seam_clip: IDCompositionRectangleClip,
        glow: Layer,
        body: Layer,
        lit: Layer,
        words: Layer,
        cap_glow: Layer,
        cap: Layer,
        cap_lit: Layer,
        glow_opacity: IDCompositionVisual3,
        cap_glow_opacity: IDCompositionVisual3,
        lit_opacity: IDCompositionVisual3,
        cap_lit_opacity: IDCompositionVisual3,
        words_opacity: IDCompositionVisual3,
        /// The lit ring over the grey one and the glow, all four at the
        /// same level: 0 at rest, 1 open
        light: Option<Tween>,
        /// The last plan applied
        plan: PanelPlan,
        /// The seam's x in the window
        edge: Option<Tween>,
        words_tween: Option<Tween>,
        /// Off in the config: nothing shows and nothing is drawn
        hidden: bool,
        /// LBTerm: the rows, the pads' arcs and the Conductor's outlines held still under a menu's
        /// card (Lachlan's 7b of 2026-11-08): while it holds, nothing of them is applied or drawn
        hold: lbterm_motion::rail::Hold,
    }

    impl PanelLayers {
        fn new(
            dcomp: &Compositor,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: &PanelPlan,
        ) -> anyhow::Result<Self> {
            // Bottom to top: the panel, the light
            let group = |parent: &IDCompositionVisual2| -> anyhow::Result<IDCompositionVisual2> {
                let visual = unsafe { dcomp.device.CreateVisual()? };
                unsafe { parent.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                Ok(visual)
            };
            let body_group = group(&dcomp.panel)?;
            let cap_group = group(&dcomp.panel)?;
            let rings_group = group(&dcomp.panel)?;
            let spot_group = group(&dcomp.panel)?;
            // Each one on top of the last in its group
            let layer = |group: &IDCompositionVisual2, rect: PixelRect| -> anyhow::Result<Layer> {
                let visual = unsafe { dcomp.device.CreateVisual()? };
                unsafe { group.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                Layer::new(
                    instance,
                    device,
                    visual,
                    base,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                    rect,
                )
            };
            let glow = layer(&body_group, plan.surface)?;
            let body = layer(&body_group, plan.surface)?;
            let lit = layer(&body_group, plan.surface)?;
            let words = layer(&body_group, plan.surface)?;
            let cap_glow = layer(&cap_group, plan.cap)?;
            let cap = layer(&cap_group, plan.cap)?;
            let cap_lit = layer(&cap_group, plan.cap)?;
            let spot = layer(&spot_group, plan.spot)?;
            let spot_core = layer(&spot_group, plan.spot_core)?;
            let spinner_group = group(&dcomp.panel)?;
            // A new clip's edges are at infinity, which clips nothing: an empty one hides the
            // templates' own visuals, which only the visuals showing their swapchains are for
            let nothing = unsafe { dcomp.device.CreateRectangleClip()? };
            unsafe {
                nothing.SetLeft2(0.0)?;
                nothing.SetTop2(0.0)?;
                nothing.SetRight2(0.0)?;
                nothing.SetBottom2(0.0)?;
                spinner_group.SetClip(&nothing)?;
            }
            let spinner = layer(
                &spinner_group,
                plan.spinners.first().copied().unwrap_or(PixelRect::new(0, 0, 1, 1)),
            )?;
            let start_spinner = layer(&spinner_group, plan.start_spinner.unwrap_or(PixelRect::new(0, 0, 1, 1)))?;
            // The rows' surfaces, shown only through the rows' visuals, which sit over the body;
            // the marker over them
            let rows_hidden = group(&dcomp.panel)?;
            unsafe { rows_hidden.SetClip(&nothing)? };
            let rows = [layer(&rows_hidden, plan.surface)?, layer(&rows_hidden, plan.surface)?];
            let rows_content = [swap_chain(&rows[0].surface)?, swap_chain(&rows[1].surface)?];
            let row_words = [layer(&rows_hidden, plan.surface)?, layer(&rows_hidden, plan.surface)?];
            let row_words_content = [swap_chain(&row_words[0].surface)?, swap_chain(&row_words[1].surface)?];
            // The rows' open parts fade with the words: a group in the words' visual, counting
            // from the panel's origin as the rows' group does (`place_surfaces` sets its offset)
            let row_words_group = unsafe { dcomp.device.CreateVisual()? };
            unsafe { words.visual.AddVisual(&row_words_group, false, None::<&IDCompositionVisual>)? };
            let rows_group = unsafe { dcomp.device.CreateVisual()? };
            let marker_visual = unsafe { dcomp.device.CreateVisual()? };
            unsafe {
                body_group.AddVisual(&rows_group, true, &body.visual)?;
                body_group.AddVisual(&marker_visual, true, &rows_group)?;
            }
            let progress = layer(&rows_hidden, plan.surface)?;
            let progress_content = swap_chain(&progress.surface)?;
            let marker = Layer::new(
                instance,
                device,
                marker_visual,
                base,
                wgpu::CompositeAlphaMode::PreMultiplied,
                plan.surface,
            )?;
            let ring_group = group(&dcomp.panel)?;
            unsafe { ring_group.SetClip(&nothing)? };
            let at_least = |r: PixelRect| if r.w == 0 || r.h == 0 { PixelRect::new(0, 0, 1, 1) } else { r };
            let ring_spectrum = layer(&ring_group, at_least(plan.ring_template))?;
            let ring_iris = layer(&ring_group, at_least(plan.ring_template))?;
            let ring_light = layer(&ring_group, at_least(plan.light_template))?;
            let seam_clip = unsafe { dcomp.device.CreateRectangleClip()? };
            unsafe { body_group.SetClip(&seam_clip)? };
            let opacity = |visual: &IDCompositionVisual2| -> anyhow::Result<IDCompositionVisual3> {
                visual
                    .cast()
                    .context("IDCompositionVisual3 (Windows 10 or later)")
            };
            let mut panel = Self {
                wave: None,
                glow_opacity: opacity(&glow.visual)?,
                cap_glow_opacity: opacity(&cap_glow.visual)?,
                lit_opacity: opacity(&lit.visual)?,
                cap_lit_opacity: opacity(&cap_lit.visual)?,
                words_opacity: opacity(&words.visual)?,
                light: None,
                spot_opacity: opacity(&spot_group)?,
                rows,
                rows_content,
                row_words,
                row_words_content,
                row_words_group,
                front: 0,
                _rows_hidden: rows_hidden,
                rows_group,
                row_visuals: vec![],
                old_rows: vec![],
                marker,
                marker_at: None,
                marker_dy: None,
                dragging: None,
                marker_held: false,
                progress,
                progress_content,
                bars: vec![],
                body_group,
                ring_spectrum,
                ring_iris,
                ring_light,
                _ring_group: ring_group,
                rings: vec![],
                rings_group,
                spinner,
                _spinner_group: spinner_group,
                spinners: vec![],
                start_spinner,
                start: None,
                cap_group,
                spot_group,
                spot,
                spot_core,
                sweep: None,
                seam_clip,
                glow,
                body,
                lit,
                words,
                cap_glow,
                cap,
                cap_lit,
                plan: plan.clone(),
                edge: None,
                words_tween: None,
                hidden: false,
                hold: Default::default(),
            };
            panel.place(device, plan)?;
            Ok(panel)
        }

        /// The surfaces where `plan` puts them, sized to match.
        fn place_surfaces(&mut self, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<()> {
            self.glow.place(device, plan.surface)?;
            self.body.place(device, plan.surface)?;
            self.lit.place(device, plan.surface)?;
            self.words.place(device, plan.surface)?;
            self.cap_glow.place(device, plan.cap)?;
            self.cap.place(device, plan.cap)?;
            self.cap_lit.place(device, plan.cap)?;
            self.spot.place(device, plan.spot)?;
            self.spot_core.place(device, plan.spot_core)?;
            self.marker.place(device, plan.surface)?;
            self.progress.place(device, plan.surface)?;
            let content = swap_chain(&self.progress.surface)?;
            if content != self.progress_content {
                self.progress_content = content;
                for bar in &self.bars {
                    unsafe { bar.visual.SetContent(&self.progress_content)? };
                }
            }
            // The rows' surfaces: a new size keeps the swapchain (wgpu resizes its buffers) but
            // not what it held, or makes a new one the rows show instead. Either way what the
            // kept ones held is gone
            for i in 0..2 {
                let size = |layer: &Layer| (layer.config.width, layer.config.height);
                let was = (size(&self.rows[i]), size(&self.row_words[i]));
                self.rows[i].place(device, plan.surface)?;
                self.row_words[i].place(device, plan.surface)?;
                let rail = swap_chain(&self.rows[i].surface)?;
                let words = swap_chain(&self.row_words[i].surface)?;
                let resized = was != (size(&self.rows[i]), size(&self.row_words[i]));
                if resized || rail != self.rows_content[i] || words != self.row_words_content[i] {
                    log::debug!("rail: surfaces {i} at {:?}, the kept ones' frame gone", plan.surface);
                    self.rows_content[i] = rail;
                    self.row_words_content[i] = words;
                    let groups = self.row_groups();
                    for (old, _) in std::mem::take(&mut self.old_rows) {
                        old.detach(&groups)?;
                    }
                    let content = self.row_content(i);
                    for row in &mut self.row_visuals {
                        row.drop_ghost()?;
                        row.bind(i, &content)?;
                        row.show(plan.surface, row.plan.slot, 0.0)?;
                        row.kept = None;
                    }
                }
            }
            unsafe {
                self.row_words_group.SetOffsetX2((-plan.surface.x) as f32)?;
                self.row_words_group.SetOffsetY2((-plan.surface.y) as f32)?;
            }
            Ok(())
        }

        fn row_groups(&self) -> RowGroups {
            RowGroups {
                rail: self.rows_group.clone(),
                words: self.row_words_group.clone(),
            }
        }

        /// What the rows show from surface pair `i`
        fn row_content(&self, i: usize) -> RowContent {
            RowContent {
                rail: self.rows_content[i].clone(),
                words: self.row_words_content[i].clone(),
            }
        }

        /// Where the seam is, open or closed.
        fn seam(plan: &PanelPlan, open: bool) -> f64 {
            plan.seam as f64 - if open { 0.0 } else { plan.travel as f64 }
        }

        /// Straight to `plan`, nothing animating.
        fn place(&mut self, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<()> {
            self.place_surfaces(device, plan)?;
            let seam = Self::seam(plan, plan.open);
            let light = plan.light as f32;
            unsafe {
                self.seam_clip.SetLeft2(-FAR)?;
                self.seam_clip.SetTop2(-FAR)?;
                self.seam_clip.SetBottom2(FAR)?;
                self.seam_clip.SetRight2(seam as f32)?;
                self.cap_group.SetOffsetX2((seam - plan.seam as f64) as f32)?;
                for opacity in self.light_opacities() {
                    opacity.SetOpacity2(light)?;
                }
                self.words_opacity
                    .SetOpacity2(if plan.open { 1.0 } else { 0.0 })?;
                // The light only runs while opening
                self.spot_opacity.SetOpacity2(0.0)?;
                self.spot_group.SetOffsetX2(0.0)?;
                self.spot_group.SetOffsetY2(0.0)?;
            }
            if let Some(wave) = self.wave.as_mut() {
                wave.edge = None;
                wave.running = None;
                wave.rest()?;
            }
            self.plan = plan.clone();
            self.edge = None;
            self.light = None;
            self.words_tween = None;
            self.sweep = None;
            self.hidden = false;
            Ok(())
        }

        /// The ring lit over the grey one and the glow, in the body and the cap.
        fn light_opacities(&self) -> [&IDCompositionVisual3; 4] {
            [
                &self.glow_opacity,
                &self.cap_glow_opacity,
                &self.lit_opacity,
                &self.cap_lit_opacity,
            ]
        }

        /// Open or close from wherever it is: the seam and the cap move
        /// together on the edge curve, keeping their speed, the words
        /// follow on theirs, and the light on the glow curve. When only the
        /// light changed (the pointer arrived or left while the edge waits)
        /// the light alone moves, on `hover`.
        fn show(
            &mut self,
            dcomp: &Compositor,
            device: &wgpu::Device,
            plan: &PanelPlan,
            now: Instant,
        ) -> anyhow::Result<()> {
            let edge_moves = plan.open != self.plan.open;
            let (from, velocity) = at(&self.edge, Self::seam(&self.plan, self.plan.open), now);
            let (words_from, _) = at(
                &self.words_tween,
                if self.plan.open { 1.0 } else { 0.0 },
                now,
            );
            let (light_from, light_velocity) = at(&self.light, self.plan.light, now);
            self.place_surfaces(device, plan)?;

            if edge_moves {
                let to = Self::seam(plan, plan.open);
                let seam = plan.seam as f64;
                let clip = &self.seam_clip;
                let cap = &self.cap_group;
                dcomp.name("rail.edge");
                self.edge = dcomp.drive(
                    Motion::new(from, to, velocity, plan.edge.clone()),
                    PIXEL_EPSILON,
                    0.0,
                    |a| unsafe { clip.SetRight(a) },
                    |v| unsafe { clip.SetRight2(v) },
                )?;
                dcomp.drive(
                    Motion::new(from - seam, to - seam, velocity, plan.edge.clone()),
                    PIXEL_EPSILON,
                    0.0,
                    |a| unsafe { cap.SetOffsetX(a) },
                    |v| unsafe { cap.SetOffsetX2(v) },
                )?;

                let words = &self.words_opacity;
                let delay = if plan.open { plan.words_delay } else { 0.0 };
                dcomp.name("rail.words");
                self.words_tween = dcomp.drive(
                    Motion::new(words_from, if plan.open { 1.0 } else { 0.0 }, 0.0, plan.words.clone()),
                    OPACITY_EPSILON,
                    delay,
                    |a| unsafe { words.SetOpacity(a) },
                    |v| unsafe { words.SetOpacity2(v) },
                )?;
            }

            // The ring lights up from grey as its glow swells, and goes
            // back to grey with it: with the edge on the glow's curve,
            // without it on `hover`
            let curve = if edge_moves { &plan.glow } else { &plan.hover };
            let mut light = None;
            dcomp.name("rail.light");
            for opacity in self.light_opacities() {
                light = dcomp.drive(
                    Motion::new(light_from, plan.light, light_velocity, curve.clone()),
                    OPACITY_EPSILON,
                    0.0,
                    |a| unsafe { opacity.SetOpacity(a) },
                    |v| unsafe { opacity.SetOpacity2(v) },
                )?;
            }
            self.light = light;

            if edge_moves {
                self.run_light(dcomp, plan, now)?;
                // The wave washes ahead of the edge as it opens, from wherever the edge is and
                // as fast as it's going, in the same commit; turned round, it fades where it is
                if let Some(wave) = self.wave.as_mut() {
                    if plan.open && !matches!(plan.edge, Curve::Instant) {
                        wave.run(dcomp, from, Self::seam(plan, true), velocity, plan, now)?;
                    } else {
                        wave.stop(dcomp, &plan.words, now)?;
                    }
                }
            }
            self.plan = plan.clone();
            Ok(())
        }

        /// The wave as `plan` says: made, moved or let go, and drawn this frame when its look
        /// changed. True when the tree changed.
        fn sync_wave(
            &mut self,
            dcomp: &Compositor,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: &PanelPlan,
        ) -> anyhow::Result<bool> {
            let Some(want) = plan.wave.as_ref() else {
                let Some(gone) = self.wave.take() else {
                    return Ok(false);
                };
                unsafe { dcomp.panel.RemoveVisual(&gone.group)? };
                return Ok(true);
            };
            let changed = match self.wave.as_mut() {
                None => {
                    self.wave = Some(WaveLayers::new(dcomp, instance, device, base, want)?);
                    true
                }
                Some(wave) if (want.rect, want.spray, want.band) != (wave.plan.rect, wave.plan.spray, wave.plan.band) => {
                    wave.place(device, want)?;
                    true
                }
                Some(wave) => {
                    wave.plan = want.clone();
                    false
                }
            };
            // The paint drew it when the look it asked for (which holds its size) wasn't the one
            // drawn, a new wave's included: only then does its surface take a frame, which keeps
            // it the rest of the time
            let wave = self.wave.as_mut().expect("made above");
            wave.redraw = want.redraw;
            if want.redraw {
                wave.drawn = Some(want.look);
            }
            Ok(changed)
        }

        /// Opening: the light sets off from the top-left corner, down the
        /// left side, and once round, fading in as it goes and out as it
        /// gets back. Closing: whatever's left of it fades out.
        fn run_light(&mut self, dcomp: &Compositor, plan: &PanelPlan, now: Instant) -> anyhow::Result<()> {
            let shown = match &self.sweep {
                Some((start, fade)) => fade.value(now.saturating_duration_since(*start).as_secs_f64()) as f64,
                None => 0.0,
            };
            let spot = &self.spot_opacity;
            dcomp.name("rail.sweep");
            if !plan.open || matches!(plan.sweep, Curve::Instant) {
                self.sweep = None;
                dcomp.drive(
                    Motion::new(shown, 0.0, 0.0, plan.words.clone()),
                    OPACITY_EPSILON,
                    0.0,
                    |a| unsafe { spot.SetOpacity(a) },
                    |v| unsafe { spot.SetOpacity2(v) },
                )?;
                return Ok(());
            }
            let layout = plan.layout;
            let [x, y] = lbterm_motion::path::along(&plan.sweep, STEP, |p| {
                let (x, y) = layout.sweep_point(p);
                (x as f64, y as f64)
            });
            let end = x.end;
            let fade = lbterm_motion::path::linear(&[
                (0.0, shown),
                (end * 0.08, 1.0),
                (end * 0.7, 1.0),
                (end, 0.0),
            ]);
            let group = &self.spot_group;
            dcomp.play(&x, |a| unsafe { group.SetOffsetX(a) }, |v| unsafe { group.SetOffsetX2(v) })?;
            dcomp.play(&y, |a| unsafe { group.SetOffsetY(a) }, |v| unsafe { group.SetOffsetY2(v) })?;
            dcomp.play(&fade, |a| unsafe { spot.SetOpacity(a) }, |v| unsafe { spot.SetOpacity2(v) })?;
            self.sweep = Some((now, fade));
            Ok(())
        }

        /// A turning arc on each pad `plan` names, and none elsewhere: the
        /// arcs a plan keeps go on turning where they are, and are held still
        /// or set turning again when the plan's `spin` changes (motion, or the
        /// window being seen: lbterm_extras::work::loops_play). True when the
        /// tree changed.
        fn sync_spinners(&mut self, dcomp: &Compositor, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<bool> {
            let start = self.sync_start_spinner(dcomp, device, plan)?;
            Ok(self.sync_task_spinners(dcomp, device, plan)? | start)
        }

        /// A turning arc's visual, in the body: a turn every SPIN_PERIOD, for ever, DWM playing
        /// it, or still, as `spin` says (Spinner::turn)
        fn new_spinner(&self, dcomp: &Compositor, spin: bool) -> anyhow::Result<Spinner> {
            let visual = unsafe { dcomp.device.CreateVisual()? };
            let rotate = unsafe { dcomp.device.CreateRotateTransform()? };
            unsafe {
                visual.SetTransform(&rotate)?;
                self.body_group.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
            }
            let mut spinner = Spinner {
                visual,
                rotate,
                rect: PixelRect::default(),
                spinning: false,
            };
            spinner.turn(dcomp, spin)?;
            Ok(spinner)
        }

        /// LBTerm: the Starting row's white arc (render/tab_panel.rs, `Starting`): made as the
        /// row comes, its template drawn where it turns, gone with the row
        fn sync_start_spinner(&mut self, dcomp: &Compositor, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<bool> {
            let Some(rect) = plan.start_spinner else {
                return match self.start.take() {
                    Some(gone) => {
                        unsafe { self.body_group.RemoveVisual(&gone.visual)? };
                        Ok(true)
                    }
                    None => Ok(false),
                };
            };
            if let Some(spinner) = self.start.as_mut().filter(|s| s.rect == rect) {
                if spinner.spinning == plan.spin {
                    return Ok(false);
                }
                spinner.turn(dcomp, plan.spin)?;
                return Ok(true);
            }
            if self.start_spinner.rect != rect {
                self.start_spinner.place(device, rect)?;
            }
            let content = swap_chain(&self.start_spinner.surface)?;
            if self.start.is_none() {
                self.start = Some(self.new_spinner(dcomp, plan.spin)?);
            }
            let spinner = self.start.as_mut().expect("made above");
            unsafe {
                spinner.visual.SetContent(&content)?;
                spinner.visual.SetOffsetX2(rect.x as f32)?;
                spinner.visual.SetOffsetY2(rect.y as f32)?;
                spinner.rotate.SetCenterX2(rect.w as f32 / 2.0)?;
                spinner.rotate.SetCenterY2(rect.h as f32 / 2.0)?;
            }
            spinner.rect = rect;
            Ok(true)
        }

        fn sync_task_spinners(&mut self, dcomp: &Compositor, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<bool> {
            let wanted = &plan.spinners;
            let same = wanted.len() == self.spinners.len()
                && wanted
                    .iter()
                    .zip(&self.spinners)
                    .all(|(r, s)| *r == s.rect && s.spinning == plan.spin);
            if same && wanted.first().map_or(true, |r| *r == self.spinner.rect) {
                return Ok(false);
            }
            if let Some(&template) = wanted.first() {
                self.spinner.place(device, template)?;
            }
            let content = swap_chain(&self.spinner.surface)?;
            while self.spinners.len() > wanted.len() {
                let gone = self.spinners.pop().expect("longer than wanted");
                unsafe { self.body_group.RemoveVisual(&gone.visual)? };
            }
            for (i, &rect) in wanted.iter().enumerate() {
                if i == self.spinners.len() {
                    let spinner = self.new_spinner(dcomp, plan.spin)?;
                    self.spinners.push(spinner);
                }
                let spinner = &mut self.spinners[i];
                if spinner.spinning != plan.spin {
                    spinner.turn(dcomp, plan.spin)?;
                }
                unsafe {
                    spinner.visual.SetContent(&content)?;
                    spinner.visual.SetOffsetX2(rect.x as f32)?;
                    spinner.visual.SetOffsetY2(rect.y as f32)?;
                    spinner.rotate.SetCenterX2(rect.w as f32 / 2.0)?;
                    spinner.rotate.SetCenterY2(rect.h as f32 / 2.0)?;
                }
                spinner.rect = rect;
            }
            Ok(true)
        }

        /// The rail's rows as `plan` has them (lbterm/DESIGN.md, "The tab panel"): a row that
        /// moved slides to its slot from where it shows, keeping its speed; a new one fades in
        /// as its slot opens; a row that's gone, or whose look changed, fades its old self out
        /// from the frame drawn before, kept in the rows' other surface. The open tab's marker
        /// glides to its row. `animate` false (a resize, the panel placed afresh) puts all of
        /// it straight there. True when the tree changed.
        fn sync_rows(&mut self, dcomp: &Compositor, plan: &PanelPlan, animate: bool, now: Instant) -> anyhow::Result<bool> {
            let mut changed = false;
            let surface = plan.surface;
            let motion = &plan.row_motion;
            let groups = self.row_groups();
            // Old selves and ghosts that are done go
            let mut keeping = vec![];
            for (old, done) in std::mem::take(&mut self.old_rows) {
                if done <= now || !animate {
                    old.detach(&groups)?;
                    changed = true;
                } else {
                    keeping.push((old, done));
                }
            }
            self.old_rows = keeping;
            for row in &mut self.row_visuals {
                if row.ghost.as_ref().is_some_and(|g| g.done <= now || !animate) {
                    row.drop_ghost()?;
                    changed = true;
                }
            }

            let mut old: Vec<RowVisual> = std::mem::take(&mut self.row_visuals);
            // LBTerm: a new tab's row takes over the row it came out of (the Starting row), whose
            // look then dissolves into its own, where it is
            for want in plan.rows.iter().filter(|r| r.was.is_some()) {
                let fresh = !old.iter().any(|r| r.plan.key == want.key);
                if let (true, Some(from)) = (fresh, old.iter_mut().find(|r| Some(r.plan.key) == want.was)) {
                    from.plan.key = want.key;
                }
            }
            let leaving: Vec<usize> = (0..old.len())
                .filter(|&i| !plan.rows.iter().any(|r| r.key == old[i].plan.key))
                .collect();
            let restyled: Vec<usize> = (0..old.len())
                .filter(|&i| plan.rows.iter().any(|r| r.key == old[i].plan.key && r.restyles(&old[i].plan, plan.open)))
                .collect();
            let first = old.is_empty() && self.old_rows.is_empty();
            // A row that changes needs its old look, and one that goes its last. Drawing on into
            // the front, nothing fading is cut short: a row still dissolving carries on through
            // a new look, its ghost holding the one before, and a row the kept surfaces hold as
            // it shows now dissolves from them. Otherwise (a row going, a row going back to its
            // ghost's look, or one the kept surfaces don't hold) the front surfaces, which hold
            // the frame drawn before this one, are kept and the others drawn on, and a dissolve
            // that showed those is cut short.
            let new_looks = |row: &RowVisual| plan.rows.iter().find(|r| r.key == row.plan.key).map(|r| r.looks());
            let drawing_on = leaving.is_empty()
                && restyled.iter().all(|&i| {
                    let row = &old[i];
                    match &row.ghost {
                        Some(ghost) => Some(ghost.looks) != new_looks(row),
                        None => row.kept.map(|(looks, _)| looks) == Some(row.plan.looks()),
                    }
                });
            let flip = animate && !first && (!leaving.is_empty() || !restyled.is_empty()) && !drawing_on;
            if !leaving.is_empty() || !restyled.is_empty() {
                log::debug!(
                    "rail: rows {:?} changed, {:?} gone, {}",
                    restyled.iter().map(|&i| old[i].plan.key).collect::<Vec<_>>(),
                    leaving.iter().map(|&i| old[i].plan.key).collect::<Vec<_>>(),
                    if flip { "drawn on the other surfaces" } else { "drawn on" },
                );
            }
            let curve_of = |row: &RowVisual| {
                // A hover alone lights on its own curve
                if new_looks(row).map(|(look, words, _)| (look, words)) == Some((row.plan.look, row.plan.words_look)) {
                    ("rail.row.hover", &motion.hover)
                } else {
                    ("rail.row.swap", &motion.swap)
                }
            };
            if flip {
                for (gone, _) in std::mem::take(&mut self.old_rows) {
                    gone.detach(&groups)?;
                }
                self.front = 1 - self.front;
                for (i, row) in old.iter_mut().enumerate() {
                    if leaving.contains(&i) {
                        // Its last frame, on the surfaces kept now, as it goes
                        row.front(1 - self.front)?;
                        continue;
                    }
                    if restyled.contains(&i) {
                        let (name, curve) = curve_of(row);
                        dcomp.name(name);
                        let looks = row.plan.looks();
                        let slot = row.plan.slot;
                        row.dissolve(dcomp, self.front, slot, looks, curve, false, now)?;
                    } else {
                        row.front(self.front)?;
                    }
                    row.kept = Some((row.plan.looks(), row.plan.slot));
                }
                changed = true;
            } else if animate && !first {
                for &i in &restyled {
                    let row = &mut old[i];
                    // Still dissolving, it carries on
                    if row.ghost.is_some() {
                        continue;
                    }
                    let (looks, from) = row.kept.expect("drawn on only when the kept surfaces hold it");
                    let (name, curve) = curve_of(row);
                    dcomp.name(name);
                    row.dissolve(dcomp, self.front, from, looks, curve, true, now)?;
                    changed = true;
                }
            }

            let contents = [self.row_content(0), self.row_content(1)];
            // A row that's gone: its old self, under the rest, fading out where it was
            for &i in leaving.iter().rev() {
                let gone = old.remove(i);
                gone.detach(&groups)?;
                if flip {
                    gone.attach(&groups, true)?;
                    let (dy, _) = gone.dy_at(now);
                    gone.show(surface, gone.plan.slot, dy as f32)?;
                    let fade = Motion::new(1.0, 0.0, 0.0, motion.exit.clone());
                    let end = fade.end(OPACITY_EPSILON);
                    dcomp.name("rail.row.exit");
                    gone.fade(dcomp, fade)?;
                    self.old_rows.push((gone, now + Duration::from_secs_f64(end)));
                }
                changed = true;
            }

            for want in &plan.rows {
                let at = old.iter().position(|r| r.plan.key == want.key);
                let mut row = match at {
                    Some(i) => old.remove(i),
                    None => {
                        let mut row = RowVisual::new(dcomp, &contents, self.front)?;
                        row.attach(&groups, false)?;
                        changed = true;
                        if animate && !first {
                            // Its slot opens from the top as the rows below make room, and it
                            // fades in with it
                            row.show(surface, want.slot, 0.0)?;
                            let top = (want.slot.y - surface.y) as f64;
                            dcomp.name("rail.row.grow");
                            row.open(dcomp, Motion::new(top, top + want.slot.h as f64, 0.0, motion.slide.clone()))?;
                            dcomp.name("rail.row.enter");
                            row.fade(dcomp, Motion::new(0.0, 1.0, 0.0, motion.enter.clone()))?;
                            row.plan = want.clone();
                            self.row_visuals.push(row);
                            continue;
                        }
                        row
                    }
                };
                let moved = row.plan.slot.y != want.slot.y;
                let held = plan.dragged.filter(|(key, _)| *key == want.key).map(|(_, dy)| dy);
                if let Some(dy) = held {
                    // Under the pointer, over the others, at once: one move each time it moves
                    row.show(surface, want.slot, dy as f32)?;
                    if row.dy.is_none() && self.dragging != Some(want.key) {
                        row.detach(&groups)?;
                        row.attach(&groups, false)?;
                    }
                    row.dy = Some(Tween {
                        motion: Motion::new(dy, dy, 0.0, Curve::Instant),
                        start: now,
                        end: 0.0,
                        sampled: None,
                    });
                    changed = true;
                } else if self.dragging == Some(want.key) && animate {
                    // Let go: into its slot from where the pointer left it
                    let (dy, v) = row.dy_at(now);
                    let from = if moved { dy + (row.plan.slot.y - want.slot.y) as f64 } else { dy };
                    row.show(surface, want.slot, from as f32)?;
                    row.slide(dcomp, surface.y as f64, from, v, &motion.slide)?;
                    changed = true;
                } else if moved && animate && !first && row.plan.key != usize::MAX {
                    // From where it shows now to its new slot, keeping its speed
                    let (dy, v) = row.dy_at(now);
                    let from = dy + (row.plan.slot.y - want.slot.y) as f64;
                    row.show(surface, want.slot, from as f32)?;
                    row.slide(dcomp, surface.y as f64, from, v, &motion.slide)?;
                    changed = true;
                } else if moved || row.plan.slot != want.slot || at.is_none() {
                    let (dy, _) = if moved { (0.0, 0.0) } else { row.dy_at(now) };
                    if moved {
                        row.dy = None;
                    }
                    row.show(surface, want.slot, dy as f32)?;
                    changed = true;
                }
                row.plan = want.clone();
                self.row_visuals.push(row);
            }
            self.dragging = plan.dragged.map(|(key, _)| key);
            // Whatever's left had no row in the plan: gone without its old self
            for gone in old {
                gone.detach(&groups)?;
                changed = true;
            }

            changed |= self.sync_marker(dcomp, plan, animate && !first, now)?;
            changed |= self.sync_progress(dcomp, plan, animate, now)?;
            Ok(changed)
        }

        /// A visual per progress bar `plan` names, in its row's words visual (so it fades with
        /// the words and moves with its row), its clip showing as much of the whole fill as the
        /// program says: gliding on `slide` from where it shows when that changes, straight there
        /// when the bar is new or moved.
        fn sync_progress(&mut self, dcomp: &Compositor, plan: &PanelPlan, animate: bool, now: Instant) -> anyhow::Result<bool> {
            let parents: Vec<(usize, IDCompositionVisual3)> =
                self.row_visuals.iter().map(|r| (r.plan.key, r.words.clone())).collect();
            let parent_of = |key: usize| parents.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
            let same = plan.progress.len() == self.bars.len()
                && plan.progress.iter().zip(&self.bars).all(|((k, n, r, f), b)| {
                    *k == b.key && *n == b.bar && *r == b.track && *f == b.fraction && parent_of(*k).as_ref() == Some(&b.parent)
                });
            if same {
                return Ok(false);
            }
            let s = plan.surface;
            let mut old = std::mem::take(&mut self.bars);
            for &(key, which, track, fraction) in &plan.progress {
                let Some(parent) = parent_of(key) else {
                    continue;
                };
                let right = (track.x - s.x) as f64 + track.w as f64 * fraction.clamp(0.0, 1.0);
                let mut bar = match old.iter().position(|b| b.key == key && b.bar == which) {
                    Some(i) => old.remove(i),
                    None => {
                        let visual = unsafe { dcomp.device.CreateVisual()? };
                        let clip = unsafe { dcomp.device.CreateRectangleClip()? };
                        unsafe {
                            visual.SetClip(&clip)?;
                            visual.SetContent(&self.progress_content)?;
                            parent.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                        }
                        ProgressBar {
                            key,
                            bar: which,
                            visual,
                            clip,
                            parent: parent.clone(),
                            track: PixelRect::default(),
                            fraction: -1.0,
                            right: None,
                        }
                    }
                };
                // Its row's visual was made again: into the new one
                if bar.parent != parent {
                    unsafe {
                        bar.parent.RemoveVisual(&bar.visual)?;
                        parent.AddVisual(&bar.visual, false, None::<&IDCompositionVisual>)?;
                    }
                    bar.parent = parent;
                }
                let clip = &bar.clip;
                // The row's words visual is at the surface's place, so its children count from there
                let glide = animate && bar.track == track && bar.fraction >= 0.0 && bar.fraction != fraction;
                unsafe {
                    clip.SetLeft2((track.x - s.x) as f32)?;
                    clip.SetTop2((track.y - s.y) as f32)?;
                    clip.SetBottom2((track.y - s.y) as f32 + track.h as f32)?;
                }
                if glide {
                    let rest = (track.x - s.x) as f64 + track.w as f64 * bar.fraction.clamp(0.0, 1.0);
                    let (from, v) = at(&bar.right, rest, now);
                    dcomp.name("rail.progress");
                    bar.right = dcomp.drive(
                        Motion::new(from, right, v, plan.row_motion.slide.clone()),
                        PIXEL_EPSILON,
                        0.0,
                        |a| unsafe { clip.SetRight(a) },
                        |v| unsafe { clip.SetRight2(v) },
                    )?;
                } else {
                    unsafe { clip.SetRight2(right as f32)? };
                    bar.right = None;
                }
                bar.track = track;
                bar.fraction = fraction;
                self.bars.push(bar);
            }
            for gone in old {
                unsafe { gone.parent.RemoveVisual(&gone.visual)? };
            }
            Ok(true)
        }

        /// The open tab's ring and bar: drawn at its row, the visual gliding from the row it
        /// was at on `marker` when another tab opens, or with its row when that slides
        fn sync_marker(&mut self, dcomp: &Compositor, plan: &PanelPlan, animate: bool, now: Instant) -> anyhow::Result<bool> {
            let y0 = plan.surface.y as f64;
            let visual = &self.marker.visual;
            // Held by a drag, it follows the pointer with its row, at once
            let held = plan
                .dragged
                .filter(|(key, _)| Some(*key) == plan.marker.map(|(k, _)| k))
                .map(|(_, dy)| dy);
            if let Some(dy) = held {
                unsafe { visual.SetOffsetY2((y0 + dy) as f32)? };
                self.marker_dy = Some(Tween {
                    motion: Motion::new(dy, dy, 0.0, Curve::Instant),
                    start: now,
                    end: 0.0,
                    sampled: None,
                });
                self.marker_at = plan.marker;
                self.marker_held = true;
                return Ok(true);
            }
            let released = std::mem::take(&mut self.marker_held);
            if plan.marker == self.marker_at && !released {
                return Ok(false);
            }
            // Another tab: a glide; the same one moving with its row (or let go): a slide; the open
            // tab closed as the row opening in its place slides up into its slot: on that row, as
            // it moves (lbterm_motion::rail, Lachlan's 3b). The rows were placed first, this frame
            let was_gone = self.marker_at.is_some_and(|(key, _)| !plan.rows.iter().any(|r| r.key == key));
            let row_shows = plan
                .marker
                .and_then(|(key, _)| self.row_visuals.iter().find(|r| r.plan.key == key))
                .map_or((0.0, 0.0), |row| row.dy_at(now));
            let shows = at(&self.marker_dy, 0.0, now);
            match lbterm_motion::rail::marker(self.marker_at, plan.marker, animate, shows, was_gone, row_shows) {
                lbterm_motion::rail::MarkerMove::Move { from, velocity, curve } => {
                    let curve = match curve {
                        lbterm_motion::rail::MarkerCurve::Glide => &plan.row_motion.marker,
                        lbterm_motion::rail::MarkerCurve::Slide => &plan.row_motion.slide,
                    };
                    dcomp.name("rail.marker");
                    self.marker_dy = drive_off(dcomp, visual, y0, from, velocity, curve)?;
                }
                lbterm_motion::rail::MarkerMove::Place => {
                    unsafe { visual.SetOffsetY2(y0 as f32)? };
                    self.marker_dy = None;
                }
            }
            self.marker_at = plan.marker;
            Ok(true)
        }

        /// The Conductor's outlines `plan` names, and none elsewhere: those it keeps go on as they
        /// are, their hues and lights where they've got to; when the plan's `spin` changes (motion,
        /// or the window being seen: lbterm_extras::work::loops_play) each is made again, turning
        /// from the start or held still. True when the tree changed.
        fn sync_rings(&mut self, dcomp: &Compositor, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<bool> {
            let wanted = &plan.rings;
            let same = wanted.len() == self.rings.len()
                && wanted.iter().zip(&self.rings).all(|(w, r)| *w == r.plan && r.spinning == plan.spin)
                && (wanted.is_empty()
                    || (plan.ring_template, plan.light_template) == (self.ring_spectrum.rect, self.ring_light.rect));
            if same {
                return Ok(false);
            }
            if !wanted.is_empty() {
                self.ring_spectrum.place(device, plan.ring_template)?;
                self.ring_iris.place(device, plan.ring_template)?;
                self.ring_light.place(device, plan.light_template)?;
            }
            let spectrum = swap_chain(&self.ring_spectrum.surface)?;
            let iris = swap_chain(&self.ring_iris.surface)?;
            let light = swap_chain(&self.ring_light.surface)?;
            let now = Instant::now();
            let mut old = std::mem::take(&mut self.rings);
            for want in wanted {
                // The pad's ring lit or fading is the same ring, kept and faded (Lachlan's 7a)
                let mut ring = match old.iter().position(|r| r.plan.same_ring(want) && r.spinning == plan.spin) {
                    Some(at) => old.remove(at),
                    None => {
                        // Made again for a change of `spin`, it carries on from as much of the
                        // old one as showed; new, it fades in from nothing
                        let from = old
                            .iter()
                            .find(|r| r.plan.same_ring(want))
                            .map_or(0.0, |r| r.opacity(now).0);
                        let mut ring = self.new_ring(dcomp, want, plan.spin)?;
                        if want.fade.is_some() {
                            let visual3: IDCompositionVisual3 = ring.visual.cast().context("IDCompositionVisual3")?;
                            unsafe { visual3.SetOpacity2(from as f32)? };
                            ring.rest = from;
                        }
                        ring
                    }
                };
                ring.fade_to(dcomp, want, now)?;
                ring.plan = want.clone();
                let content = match want.style {
                    RingStyle::Spectrum => &spectrum,
                    RingStyle::Iridescent => &iris,
                };
                unsafe {
                    ring.visual.SetContent(content)?;
                    if let Some(visual) = &ring.light {
                        visual.SetContent(&light)?;
                    }
                }
                self.rings.push(ring);
            }
            for gone in old {
                self.remove_ring(gone)?;
            }
            Ok(true)
        }

        /// An outline round `want`'s pad, its hues turning (still with motion off), and its light
        fn new_ring(&self, dcomp: &Compositor, want: &super::RingPlan, spin: bool) -> anyhow::Result<Ring> {
            let visual = unsafe { dcomp.device.CreateVisual()? };
            unsafe {
                self.rings_group.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                visual.SetOffsetX2(want.rect.x as f32)?;
                visual.SetOffsetY2(want.rect.y as f32)?;
            }
            let effect = if spin {
                // Once round every hue each HUE_PERIOD, for ever: DWM plays it
                let device: IDCompositionDevice3 = dcomp.device.cast().context("IDCompositionDevice3")?;
                let effect = unsafe { device.CreateHueRotationEffect()? };
                let animation = unsafe { dcomp.device.CreateAnimation()? };
                unsafe {
                    animation.AddCubic(0.0, 0.0, (360.0 / HUE_PERIOD) as f32, 0.0, 0.0)?;
                    animation.AddRepeat(HUE_PERIOD, HUE_PERIOD)?;
                    effect.SetAngle(&animation)?;
                    let visual3: IDCompositionVisual3 = visual.cast().context("IDCompositionVisual3")?;
                    visual3.SetEffect(&effect)?;
                }
                Some(effect)
            } else {
                None
            };
            let light = match &want.light {
                Some(light) if !light.offsets.is_empty() => {
                    let lamp = unsafe { dcomp.device.CreateVisual()? };
                    unsafe {
                        self.rings_group.AddVisual(&lamp, false, None::<&IDCompositionVisual>)?;
                        let lamp3: IDCompositionVisual3 = lamp.cast().context("IDCompositionVisual3")?;
                        lamp3.SetOpacity2(light.opacity)?;
                    }
                    if spin {
                        let xs: Vec<f32> = light.offsets.iter().map(|o| o.0).collect();
                        let ys: Vec<f32> = light.offsets.iter().map(|o| o.1).collect();
                        let x = dcomp.lap(&xs, light.lap)?;
                        let y = dcomp.lap(&ys, light.lap)?;
                        unsafe {
                            lamp.SetOffsetX(&x)?;
                            lamp.SetOffsetY(&y)?;
                        }
                    } else {
                        unsafe {
                            lamp.SetOffsetX2(light.offsets[0].0)?;
                            lamp.SetOffsetY2(light.offsets[0].1)?;
                        }
                    }
                    Some(lamp)
                }
                _ => None,
            };
            Ok(Ring {
                visual,
                _effect: effect,
                light,
                plan: want.clone(),
                spinning: spin,
                target: None,
                rest: 1.0,
                alpha: None,
            })
        }

        fn remove_ring(&self, ring: Ring) -> anyhow::Result<()> {
            unsafe {
                self.rings_group.RemoveVisual(&ring.visual)?;
                if let Some(light) = &ring.light {
                    self.rings_group.RemoveVisual(light)?;
                }
            }
            Ok(())
        }

        fn hide(&mut self) -> anyhow::Result<()> {
            for gone in self.spinners.drain(..).chain(self.start.take()) {
                unsafe { self.body_group.RemoveVisual(&gone.visual)? };
            }
            for gone in std::mem::take(&mut self.rings) {
                self.remove_ring(gone)?;
            }
            unsafe {
                self.seam_clip.SetRight2(-FAR)?;
                self.cap_group.SetOffsetX2(-FAR)?;
                self.spot_opacity.SetOpacity2(0.0)?;
            }
            if let Some(wave) = self.wave.as_mut() {
                wave.edge = None;
                wave.running = None;
                wave.rest()?;
            }
            self.sweep = None;
            self.edge = None;
            self.light = None;
            self.words_tween = None;
            self.hidden = true;
            Ok(())
        }

        fn frames(&self, frames: &mut Vec<Frame>, content: Content<'_>) -> anyhow::Result<()> {
            for (part, layer) in [
                (PanelPart::Glow, &self.glow),
                (PanelPart::Body, &self.body),
                (PanelPart::Lit, &self.lit),
                (PanelPart::Words, &self.words),
                (PanelPart::CapGlow, &self.cap_glow),
                (PanelPart::Cap, &self.cap),
                (PanelPart::CapLit, &self.cap_lit),
                (PanelPart::Spot, &self.spot),
                (PanelPart::SpotCore, &self.spot_core),
            ] {
                frames.extend(layer.frame(Target::Panel(part), content)?);
            }
            // LBTerm: under a menu's card the rows, the marker, the progress fills, the arcs'
            // templates and the outlines' keep the frames they last showed, as their visuals keep
            // their places (`hold`, Lachlan's 7b): the paint draws the rail as it is now, which
            // the tree shows only once the card has gone
            let held = self.hold.held();
            if !held {
                for (part, layer) in [
                    // The front of the rows' two surfaces only: the other keeps an old frame
                    (PanelPart::Rows, &self.rows[self.front]),
                    (PanelPart::RowWords, &self.row_words[self.front]),
                    (PanelPart::Marker, &self.marker),
                    (PanelPart::Progress, &self.progress),
                ] {
                    frames.extend(layer.frame(Target::Panel(part), content)?);
                }
            }
            // The arc template, only while a pad shows it
            if !held && !self.spinners.is_empty() {
                frames.extend(self.spinner.frame(Target::Panel(PanelPart::Spinner), content)?);
            }
            // The Starting row's white arc, only while it shows
            if !held && self.start.is_some() {
                frames.extend(self.start_spinner.frame(Target::Panel(PanelPart::StartSpinner), content)?);
            }
            // The wave's water and spray, only in the frame that draws them: they keep that
            // drawing after
            if let Some(wave) = self.wave.as_ref().filter(|w| w.redraw) {
                for (part, layer) in [(PanelPart::Wave, &wave.water), (PanelPart::Spray, &wave.spray)] {
                    frames.extend(layer.frame(Target::Panel(part), content)?);
                }
            }
            // The Conductor's templates, while an outline shows them
            if !held && !self.rings.is_empty() {
                for (part, layer) in [
                    (PanelPart::RingSpectrum, &self.ring_spectrum),
                    (PanelPart::RingIris, &self.ring_iris),
                    (PanelPart::RingLight, &self.ring_light),
                ] {
                    frames.extend(layer.frame(Target::Panel(part), content)?);
                }
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct Layers {
        over: Option<Layer>,
        panel: Option<PanelLayers>,
        /// The active pane's outline, and the pane with the keyboard last frame
        focus: Option<FocusOutline>,
        active: Option<PaneId>,
        /// LBTerm: what of the outlines of tabs left fades out with their panes, oldest first,
        /// under the window's outline (`LeftOutline`)
        left_outlines: Vec<LeftOutline>,
        panes: Vec<(PaneId, PaneLayer)>,
        /// Panes gone from the layout, fading out under the rest, or over it for a tab that
        /// was left (`over`), and when each is done; oldest first
        exiting: Vec<(PaneId, PaneLayer, Instant, bool)>,
        spare: Vec<PaneLayer>,
        /// The most panes on screen in one frame so far: what the spare pool keeps once nothing
        /// fades (`trim_spares`)
        most_panes: usize,
        /// The window's size at the last frame: a change means the user is
        /// resizing it, and nothing animates
        window: Option<PixelRect>,
        /// This frame's window is a new size: nothing animates
        resizing: bool,
        /// The panes are pressed over by the open tab panel
        squeezed: bool,
        /// The tree changed since the last commit
        changed: bool,
        /// The tab menu's visual, made the first time a menu opens
        top: Option<Layer>,
        /// Something is drawn in the tab menu's z-layers this frame
        top_wanted: bool,
        /// `top` showed something in the last frame, so it needs one cleared
        /// frame when the menu closes
        top_shown: bool,
        /// LBTerm: this frame's panes that left with their tab and dissolve over the next
        /// tab's, and the panes of a tab switched back to that came back whole: the strips down
        /// the window's right and the strip's words fade the same way (`prepare_sides`,
        /// `prepare_swap`)
        dissolved: Vec<PaneId>,
        returned: Vec<PaneId>,
        /// LBTerm: the tabs in flight, each by the panes that left with it, oldest first and on
        /// top (`lbterm_motion::flight`): their panes and the strips' pictures go out by the
        /// shares it keeps
        flight: lbterm_motion::Flight<Vec<PaneId>>,
        /// This frame's: the tabs in flight whose opacity was played again from now, which the
        /// strips' pictures follow
        redriven: Vec<Vec<PaneId>>,
    }

    /// LBTerm: one picture of the strips down the window's right as the tab left showed them
    /// (render/sides.rs), its one frame kept while it fades out over the next tab's.
    struct SidePicture {
        layer: Layer,
        opacity: IDCompositionVisual3,
        /// The panes that left with it: one coming back on screen takes it away
        panes: Vec<PaneId>,
        /// When its fade is done
        done: Instant,
    }

    /// LBTerm: what one of the strip's words in flight are (render/strip_swap.rs): a tab's, by
    /// the panes that left with it, or words that changed in the tab on screen, by their swap
    #[derive(Debug, Clone, PartialEq)]
    enum Words {
        Tab(Vec<PaneId>),
        Swap(u64),
    }

    /// LBTerm: one of the strip's words in flight: a visual of the strip as it was, showing the
    /// rows whose words changed through a clip, for the swap `generation`.
    struct SwapLayer {
        words: Words,
        generation: u64,
        layer: Layer,
        opacity: IDCompositionVisual3,
        clip: IDCompositionRectangleClip,
    }

    /// LBTerm: the strip's words in flight, a stack of their own by the panes' rules
    /// (`lbterm_motion::flight`): each swap the strip as it was, going out by its share of the
    /// strip, the oldest on top, a tab switched back to taking its words out of it, kept to
    /// `WORDS_DEPTH`. With one visual for the swap a swap that came while another faded took its
    /// place, so the words of the oldest of three tabs switched through in a row left the strip at
    /// once while its panes still showed it (about 30% of it, 40 ms apart; the review of
    /// 2026-10-30); with a visual for each but each fading on `swap` itself, a tab's words showed
    /// more of themselves as an older one went, and a switch back to the oldest left the newer
    /// ones as they were.
    #[derive(Default)]
    struct SwapStack {
        flight: lbterm_motion::Flight<Words>,
        /// In the flight's order: the newest is drawn each frame while it shows, the rest keep
        /// their last frame
        layers: Vec<SwapLayer>,
        /// Surfaces kept for the next (two at most: a run of quick switches makes the rest)
        spare: Vec<(Layer, IDCompositionVisual3, IDCompositionRectangleClip)>,
        /// The swap the newest shows, or was last asked for
        shown: u64,
    }

    /// LBTerm: the pictures of the strips still fading, and the surfaces they leave.
    #[derive(Default)]
    struct SideLayers {
        /// Oldest first, and the oldest on top, as the panes of tabs left in a row stack
        fading: Vec<SidePicture>,
        spare: Vec<(Layer, IDCompositionVisual3)>,
        /// The newest was made this frame and gets its one frame
        drawn: bool,
    }

    pub struct Compositor {
        device: IDCompositionDesktopDevice,
        /// LBTerm: whether `record` works out what each animation plays as numbers (`played`):
        /// in a window with a state file, and in the tests over the stand-in device
        /// (`with_device`)
        numbers: bool,
        /// With a state file (lbterm/STATE.md): what the next animation started is, set where
        /// what's animated is known, and what's in flight, by that name
        naming: std::cell::Cell<&'static str>,
        in_flight: RefCell<lbterm_extras::state::InFlight>,
        /// When this frame's `prepare` worked its curves out, for the commit's trace: DWM starts
        /// every animation at the Commit, so each shows late by the time between (PITFALLS 304;
        /// `lbterm::compositor::lag` at trace, Lachlan's 2a of 2026-11-10)
        prepared: std::cell::Cell<Option<Instant>>,
        // Dropping the target takes the tree off the window
        _target: IDCompositionTarget,
        _root: IDCompositionVisual2,
        under: IDCompositionVisual2,
        panes: IDCompositionVisual2,
        /// Over the panes: the active pane's outline
        focus: IDCompositionVisual2,
        over: IDCompositionVisual2,
        panel: IDCompositionVisual2,
        /// Over the panel: the tab menu (render/tab_menu.rs)
        top: IDCompositionVisual2,
        /// Just over `over`: the strip's hover slots (render/hover_fade.rs)
        hover: IDCompositionVisual2,
        hover_layers: RefCell<HoverLayers>,
        /// Over the hover slots, under the panel: the strip's detail card (render/strip.rs)
        strip_card: IDCompositionVisual2,
        strip_card_layer: RefCell<StripCard>,
        /// Over the strip's card, under the panel: the objectives panel's four cards, the list of
        /// each tab's first objective, one session's whole list, the projects card and the scroll
        /// strip's card
        /// (render/objectives.rs, render/projects.rs, render/scroll_strip.rs)
        objectives: IDCompositionVisual2,
        objectives_layers: RefCell<[StripCard; 4]>,
        /// Over the tab menu: the turning arc on a Conductor item that waits (render/conductor.rs)
        menu_spinner: RefCell<MenuSpinner>,
        /// Over a tab link drag's cards: the lights running round its matched blocks
        /// (render/link_drag.rs)
        link_lamps: RefCell<LinkLamps>,
        /// In `panel_lights`, over the rail and under the tab menu: the lights running along the
        /// suggested tab connections' lines (render/connections.rs)
        connection_lamps: RefCell<ConnectionLamps>,
        /// In `lights`: the easter egg's hearts, and their sheet (render/triggers.rs)
        hearts_layers: RefCell<HeartsLayers>,
        /// Over `over` and the strip's swap: brief light, a click's ripple and a copied
        /// or opened stretch of text (render/lights.rs)
        lights: IDCompositionVisual2,
        light_layers: RefCell<LightLayers>,
        /// Over the strip's card, under the panel: the readout of a pane's size in cells
        /// while the window is resized or the font changes (render/lights.rs)
        readout: IDCompositionVisual2,
        readout_layer: RefCell<StripCard>,
        /// Over the tab panel, under the tab menu: a pad's glint (render/tab_panel.rs)
        panel_lights: IDCompositionVisual2,
        /// Over `over`, under the lights: the strip as it was, fading out over its new words
        /// (render/strip_swap.rs), a visual for each of the words in flight
        swap: IDCompositionVisual2,
        swaps: RefCell<SwapStack>,
        /// Just over `over`, under the strip's swap: the strips down the window's right as each
        /// tab left showed them, fading out with its panes (render/sides.rs)
        sides: IDCompositionVisual2,
        sides_layers: RefCell<SideLayers>,
        /// LBTerm: the clock the tabs in flight go by (`lbterm_motion::flight` takes seconds)
        epoch: Instant,
        /// Over the tab panel and a pad's glint, under the tab menu: the card of what a tab's
        /// Claude session has running, beside its row (render/tasks_card.rs)
        tasks_card: IDCompositionVisual2,
        tasks_card_layer: RefCell<StripCard>,
        /// LBTerm: over the tab panel, under a pad's glint: the new-tab pad's look under the
        /// pointer, and over it the picker beside the rail (render/new_tab.rs), each faded by DWM
        plus: IDCompositionVisual2,
        plus_layer: RefCell<StripCard>,
        picker: IDCompositionVisual2,
        picker_layer: RefCell<StripCard>,
        /// LBTerm: over the tab panel and the card of what a tab runs, under a tab link drag: every
        /// menu's card, a slot each, faded in and out with its highlight and words
        /// (render/menu_fade.rs)
        menus: IDCompositionVisual2,
        menu_cards: RefCell<[MenuCard; MENU_SLOTS]>,
        /// LBTerm: over every menu's card, under a tab link drag: a picked row's flash (render/lights.rs,
        /// `Light::Row`), a light of its own so the card's fade doesn't carry it
        menu_lights: IDCompositionVisual2,
        /// LBTerm: every pane dimmed, the one with the keyboard too, while something of LBTerm's
        /// own has the keys (the new-tab picker): as the window losing the keyboard dims them
        dim_all: std::cell::Cell<bool>,
        /// The card templates' visuals, clipped to nothing: only their
        /// pieces on the panes are seen
        cards: IDCompositionVisual2,
        layers: RefCell<Layers>,
        /// Apart from `layers`, so a pane can be cut from them while the
        /// panes change
        card_layers: RefCell<Option<Cards>>,
    }

    impl Compositor {
        pub fn new(hwnd: *mut std::ffi::c_void) -> anyhow::Result<Self> {
            // No rendering device: the visuals' content is swapchains,
            // which DComp doesn't draw into itself
            let device: IDCompositionDesktopDevice =
                unsafe { DCompositionCreateDevice3(None::<&IUnknown>) }
                    .context("DCompositionCreateDevice3")?;
            Self::with_device(device, hwnd, lbterm_extras::state::path().is_some())
        }

        /// LBTerm: the tree on `device`, for the window `hwnd`; `numbers` says whether `record`
        /// works out what each animation plays (a window with a state file does). `new` comes
        /// here with DirectComposition's device, and the tests (compositor/imp/tests.rs) with the
        /// stand-in that records what it's given (compositor/recorder.rs), so both take the one
        /// path (lbterm/COMPOSITOR.md, "Checking it without a window")
        fn with_device(
            device: IDCompositionDesktopDevice,
            hwnd: *mut std::ffi::c_void,
            numbers: bool,
        ) -> anyhow::Result<Self> {
            unsafe {
                // Topmost: above anything GDI might still draw
                let target = device
                    .CreateTargetForHwnd(HWND(hwnd), true)
                    .context("CreateTargetForHwnd")?;
                let root = device.CreateVisual().context("CreateVisual")?;
                target.SetRoot(&root).context("SetRoot")?;
                let under = device.CreateVisual()?;
                let panes = device.CreateVisual()?;
                let focus = device.CreateVisual()?;
                let over = device.CreateVisual()?;
                let panel = device.CreateVisual()?;
                let top = device.CreateVisual()?;
                let hover = device.CreateVisual()?;
                let strip_card = device.CreateVisual()?;
                let objectives = device.CreateVisual()?;
                let lights = device.CreateVisual()?;
                let readout = device.CreateVisual()?;
                let swap = device.CreateVisual()?;
                let sides = device.CreateVisual()?;
                let panel_lights = device.CreateVisual()?;
                let tasks_card = device.CreateVisual()?;
                let plus = device.CreateVisual()?;
                let picker = device.CreateVisual()?;
                let menus = device.CreateVisual()?;
                let menu_lights = device.CreateVisual()?;
                let cards = device.CreateVisual()?;
                let nothing = device.CreateRectangleClip()?;
                cards.SetClip(&nothing)?;
                // With no reference visual, FALSE puts each one on top
                for visual in [
                    &cards, &under, &panes, &focus, &over, &sides, &swap, &lights, &hover, &strip_card, &objectives, &readout, &panel,
                    &plus, &picker, &panel_lights, &tasks_card, &menus, &menu_lights, &top,
                ] {
                    root.AddVisual(visual, false, None::<&IDCompositionVisual>)
                        .context("AddVisual")?;
                }
                device.Commit().context("Commit")?;
                Ok(Self {
                    device,
                    numbers,
                    naming: std::cell::Cell::new("other"),
                    in_flight: RefCell::new(Default::default()),
                    prepared: std::cell::Cell::new(None),
                    _target: target,
                    _root: root,
                    under,
                    panes,
                    focus,
                    over,
                    panel,
                    top,
                    hover,
                    hover_layers: RefCell::new(HoverLayers::default()),
                    strip_card,
                    strip_card_layer: RefCell::new(StripCard::default()),
                    objectives,
                    objectives_layers: RefCell::new(Default::default()),
                    menu_spinner: RefCell::new(MenuSpinner::default()),
                    link_lamps: RefCell::new(LinkLamps::default()),
                    connection_lamps: RefCell::new(ConnectionLamps::default()),
                    hearts_layers: RefCell::new(HeartsLayers::default()),
                    lights,
                    light_layers: RefCell::new(LightLayers::default()),
                    readout,
                    readout_layer: RefCell::new(StripCard::default()),
                    panel_lights,
                    swap,
                    swaps: RefCell::new(SwapStack::default()),
                    sides,
                    sides_layers: RefCell::new(SideLayers::default()),
                    epoch: Instant::now(),
                    tasks_card,
                    tasks_card_layer: RefCell::new(StripCard::default()),
                    plus,
                    plus_layer: RefCell::new(StripCard::default()),
                    picker,
                    picker_layer: RefCell::new(StripCard::default()),
                    menus,
                    menu_cards: RefCell::new(Default::default()),
                    menu_lights,
                    dim_all: std::cell::Cell::new(false),
                    cards,
                    layers: RefCell::new(Layers::default()),
                    card_layers: RefCell::new(None),
                })
            }
        }

        /// The surface for the window background, on the bottom visual.
        /// It's `WebGpuState::surface`, configured and resized with the
        /// window as the HWND surface would be.
        pub fn create_under_surface(
            &self,
            instance: &wgpu::Instance,
        ) -> anyhow::Result<wgpu::Surface<'static>> {
            Ok(unsafe {
                instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(
                    self.under.as_raw(),
                ))?
            })
        }

        /// Play `motion` on one property, `delay` seconds from now: DComp
        /// gets its cubics through `animate`, or the end value through
        /// `set` when there's nothing to animate. The tween is kept for
        /// retargeting.
        fn drive(
            &self,
            motion: Motion,
            epsilon: f64,
            delay: f64,
            animate: impl FnOnce(&IDCompositionAnimation) -> windows58::core::Result<()>,
            set: impl FnOnce(f32) -> windows58::core::Result<()>,
        ) -> anyhow::Result<Option<Tween>> {
            let segments = motion.segments(STEP, epsilon);
            if segments.cubics.is_empty() {
                set(segments.end_value)?;
                return Ok(None);
            }
            let animation = unsafe { self.device.CreateAnimation()? };
            if delay > 0.0 {
                // Holds where it starts until the delay is up
                unsafe { animation.AddCubic(0.0, motion.from as f32, 0.0, 0.0, 0.0)? };
            }
            for cubic in &segments.cubics {
                let [c0, c1, c2, c3] = cubic.coefficients;
                unsafe { animation.AddCubic(cubic.begin + delay, c0, c1, c2, c3)? };
            }
            unsafe { animation.End(segments.end + delay, segments.end_value)? };
            animate(&animation)?;
            let start = Instant::now() + Duration::from_secs_f64(delay);
            self.record(start, segments.end, &segments);
            Ok(Some(Tween {
                start,
                end: segments.end,
                motion,
                sampled: None,
            }))
        }

        /// LBTerm: `now` on the clock the tabs in flight go by, and back
        fn clock(&self, now: Instant) -> f64 {
            now.saturating_duration_since(self.epoch).as_secs_f64()
        }

        fn at_clock(&self, t: f64) -> Instant {
            self.epoch + Duration::from_secs_f64(t.max(0.0))
        }

        /// What the animations started from here on are, for the state file: a name from
        /// lbterm/STATE.md's list
        fn name(&self, name: &'static str) {
            self.naming.set(name);
        }

        /// An animation started under the current name, from `start` for `seconds`: for the state
        /// file, and for the budget, which takes a window as busy while one plays (what ended a
        /// while before is let go as each goes in, lbterm_extras::state::InFlight). In a window
        /// with a state file, what `segments` plays as numbers too (`played`, lbterm/STATE.md:
        /// Lachlan's 3a of 2026-11-09); without one nothing is worked out
        fn record(&self, start: Instant, seconds: f64, segments: &Segments) {
            let end = start + Duration::from_secs_f64(seconds.max(0.0));
            let name = self.naming.get();
            let mut in_flight = self.in_flight.borrow_mut();
            in_flight.start(name, start, end);
            if self.numbers {
                let span = segments.span();
                in_flight.played(
                    name,
                    start,
                    lbterm_extras::state::Played {
                        from: span.from,
                        to: span.to,
                        low: span.low,
                        high: span.high,
                        ms: lbterm_extras::state::millis(end.saturating_duration_since(start)),
                    },
                );
            }
        }

        /// `name`'s animations stopped at `at`, their numbers set where they were: no longer in
        /// flight from then (lbterm_extras::state::InFlight::cut)
        fn cut(&self, name: &'static str, at: Instant) {
            self.in_flight.borrow_mut().cut(name, at);
        }
        /// What DWM is playing at `now`, by name (lbterm/STATE.md)
        pub fn animations(&self, now: Instant) -> Vec<lbterm_extras::state::Animation> {
            self.in_flight.borrow_mut().at(now)
        }

        /// The animations DWM plays for ever that are in the window's tree now, by the names the
        /// state file gives them (lbterm/STATE.md, `loops`): what the compositor gave DWM, read
        /// from its own spinners, outlines and lamps, not what the panel's plan asked for
        /// (lbterm/PITFALLS.md, "Measuring"). Each keeps DWM composing every refresh while it's
        /// there (lbterm/COMPOSITOR.md, "Loops")
        pub fn loops(&self) -> Vec<&'static str> {
            let mut loops = vec![];
            if let Some(panel) = self.layers.borrow().panel.as_ref() {
                if panel.spinners.iter().any(|s| s.spinning) {
                    loops.push("rail.spinner");
                }
                // The Starting row's white arc, while a new tab is on its way
                if panel.start.as_ref().is_some_and(|s| s.spinning) {
                    loops.push("rail.starting");
                }
                // An outline's hues turn only while it spins: no effect is made for one held still
                if panel.rings.iter().any(|r| r.spinning && r._effect.is_some()) {
                    loops.push("conductor.outline");
                }
                if panel.rings.iter().any(|r| r.spinning && r.light.is_some()) {
                    loops.push("conductor.light");
                }
            }
            if self.menu_spinner.borrow().turning {
                loops.push("menu.spinner");
            }
            if self.link_lamps.borrow().turning {
                loops.push("link.lamp");
            }
            {
                // What DWM was given: a lamp's laps while they run
                let now = Instant::now();
                let lamps = self.connection_lamps.borrow();
                if lamps.lamps.iter().any(|l| l.glow.running(l.lap, now)) {
                    loops.push("connection.light");
                }
            }
            loops
        }

        /// When the last of what DWM is playing ends
        pub fn settles(&self) -> Option<Instant> {
            self.in_flight.borrow().settles()
        }

        /// What DWM is playing at `now`, by name, with when each starts and ends (present.rs,
        /// only while it times frames to the screen); what ended long ago is let go
        pub fn playing(&self, now: Instant) -> Vec<(&'static str, Instant, Instant)> {
            self.in_flight.borrow_mut().playing(now).collect()
        }

        /// Play ready-made `segments` on one property from now, or set its
        /// end value when there's nothing to animate.
        /// `values`, evenly through `lap` seconds, straight from each to the next and from the
        /// last back to the first, lap after lap for ever: DWM plays it
        fn lap(&self, values: &[f32], lap: f64) -> anyhow::Result<IDCompositionAnimation> {
            let animation = unsafe { self.device.CreateAnimation()? };
            let n = values.len().max(1);
            let dt = lap / n as f64;
            for (k, &v) in values.iter().enumerate() {
                let next = values[(k + 1) % n];
                unsafe { animation.AddCubic(k as f64 * dt, v, ((next - v) as f64 / dt) as f32, 0.0, 0.0)? };
            }
            unsafe { animation.AddRepeat(lap, lap)? };
            Ok(animation)
        }

        fn play(
            &self,
            segments: &Segments,
            animate: impl FnOnce(&IDCompositionAnimation) -> windows58::core::Result<()>,
            set: impl FnOnce(f32) -> windows58::core::Result<()>,
        ) -> anyhow::Result<()> {
            if segments.cubics.is_empty() {
                set(segments.end_value)?;
                return Ok(());
            }
            let animation = unsafe { self.device.CreateAnimation()? };
            for cubic in &segments.cubics {
                let [c0, c1, c2, c3] = cubic.coefficients;
                unsafe { animation.AddCubic(cubic.begin, c0, c1, c2, c3)? };
            }
            unsafe { animation.End(segments.end, segments.end_value)? };
            animate(&animation)?;
            self.record(Instant::now(), segments.end, segments);
            Ok(())
        }

        fn retire(&self, layers: &mut Layers, pane: PaneLayer) -> anyhow::Result<()> {
            unsafe { self.panes.RemoveVisual(&pane.container)? };
            layers.changed = true;
            if layers.spare.len() < SPARE_LAYERS {
                layers.spare.push(pane);
            }
            Ok(())
        }

        /// Once nothing fades, the pool keeps as many spare surfaces as the most panes on
        /// screen at once so far (one at least, `SPARE_LAYERS` at most): what a switch to any tab
        /// seen needs without making one, the next tab's panes taking theirs before the last
        /// tab's give theirs back. A run of quick switches holds a window-sized surface for each
        /// tab still fading, and
        /// they went back to the pool for good (the review of 2026-10-30: 413 MB of GPU memory at
        /// rest after two bursts of Ctrl+Tab in a 120×36 window, against 384 MB with motion off)
        fn trim_spares(layers: &mut Layers) {
            if !layers.exiting.is_empty() {
                return;
            }
            let keep = layers.most_panes.clamp(1, SPARE_LAYERS);
            if layers.spare.len() > keep {
                log::debug!("panes: {} spare surfaces kept of {}, nothing fading", keep, layers.spare.len());
                layers.spare.truncate(keep);
            }
        }

        /// LBTerm: how many surfaces the pool keeps for panes to come, for the state file
        pub fn spare_panes(&self) -> usize {
            self.layers.borrow().spare.len()
        }

        /// Make the tree match this frame: `over` the size of the window,
        /// the tab panel as `panel` says, and a visual per pane in `panes`
        /// at its rectangle (pressed to the right while the panel is open).
        /// When the rectangles changed and motion is on, the panes that
        /// moved, arrived or left get their animations here, all starting
        /// in the same commit; `left` says whether a pane that went from the
        /// screen isn't in the tab shown now (another tab is, or its own
        /// closed), so the panes of a tab switched from or closed fade into
        /// the next tab's. Panes without the keyboard dim toward `backdrop`,
        /// the window background as stored (each channel 0 to 1). Pane
        /// surfaces take `base`'s format and present mode.
        pub fn prepare(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            window: PixelRect,
            panes: &[(PaneId, PixelRect)],
            motion: &MotionSettings,
            panel: Option<&PanelPlan>,
            top: bool,
            cards: &CardPlan,
            active: Option<PaneId>,
            left: &dyn Fn(PaneId) -> bool,
            backdrop: [f64; 3],
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            self.prepared.set(Some(now));
            let mut layers = self.layers.borrow_mut();
            let layers = &mut *layers;
            // What this frame's panes do that the strips beside them follow: none yet
            layers.dissolved.clear();
            layers.returned.clear();
            layers.redriven.clear();

            // The card templates first: every pane's outline is cut from them
            let mut card_layers = self.card_layers.borrow_mut();
            let refit = match &mut *card_layers {
                Some(c) => c.place(device, cards)?,
                None => {
                    card_layers.replace(Cards::new(self, instance, device, base, cards)?);
                    true
                }
            };
            let card_layers = card_layers.as_ref().expect("made above");
            if refit {
                let exiting = layers.exiting.iter_mut().map(|(_, p, ..)| p);
                let spare = layers.spare.iter_mut();
                for pane in layers.panes.iter_mut().map(|(_, p)| p).chain(exiting).chain(spare) {
                    pane.refit(card_layers, now)?;
                }
                if let Some(focus) = &mut layers.focus {
                    focus.refit(card_layers, now)?;
                }
                // LBTerm: the outlines of tabs left show the templates as they were: done with
                self.put_away_outlines(layers, |_| true)?;
                layers.changed = true;
            }
            if layers.focus.is_none() {
                layers.focus = Some(FocusOutline::new(self, card_layers)?);
                layers.changed = true;
            }

            match &mut layers.over {
                Some(over) => {
                    over.place(device, window)?;
                }
                None => {
                    layers.over.replace(Layer::new(
                        instance,
                        device,
                        self.over.clone(),
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        window,
                    )?);
                    layers.changed = true;
                }
            }

            // LBTerm: `top` (the tab menu) only while something's drawn there, and for
            // one cleared frame after
            layers.top_wanted = top;
            if top || layers.top_shown {
                match &mut layers.top {
                    Some(layer) => {
                        layer.place(device, window)?;
                    }
                    None => {
                        layers.top.replace(Layer::new(
                            instance,
                            device,
                            self.top.clone(),
                            base,
                            wgpu::CompositeAlphaMode::PreMultiplied,
                            window,
                        )?);
                        layers.changed = true;
                    }
                }
            }

            // A window being resized by its frame doesn't animate, and
            // anything fading out is done with
            let resized = layers
                .window
                .map_or(true, |w| (w.w, w.h) != (window.w, window.h));
            layers.window = Some(window);
            layers.resizing = resized;

            // LBTerm: the panel placed afresh this frame (made, shown again, or the window resized
            // by its frame): its rows go straight where they are, with nothing to hold
            let mut placed = resized || layers.panel.as_ref().map_or(true, |layer| layer.hidden);
            match (panel, &mut layers.panel) {
                (Some(plan), None) => {
                    let mut layer = PanelLayers::new(self, instance, device, base, plan)?;
                    layer.sync_wave(self, instance, device, base, plan)?;
                    layers.panel = Some(layer);
                    layers.changed = true;
                }
                (Some(plan), Some(layer)) => {
                    // The wave first, so an open in this frame carries it
                    if layer.sync_wave(self, instance, device, base, plan)? {
                        layers.changed = true;
                    }
                    // A menu's card coming over the window while the wave washes: it stops where
                    // it is and fades on the words' curve, as turned round. Asked every frame, not
                    // in `show` alone: the panel holds still under the card (Hover::hold), so its
                    // open and light don't change and `show` isn't called (until 2026-11-08 the
                    // stop sat there and never ran: PITFALLS 187)
                    if plan.cards != layer.plan.cards {
                        if plan.cards && plan.open == layer.plan.open {
                            if let Some(wave) = layer.wave.as_mut() {
                                wave.stop(self, &plan.words, now)?;
                                layers.changed = true;
                            }
                        }
                        layer.plan.cards = plan.cards;
                    }
                    if layer.hidden || resized {
                        layer.place(device, plan)?;
                        layers.changed = true;
                    } else if plan.open != layer.plan.open || plan.light != layer.plan.light {
                        log::debug!("tab panel: shown open {}, light {}", plan.open, plan.light);
                        layer.show(self, device, plan, now)?;
                        layers.changed = true;
                    } else if (plan.surface, plan.cap, plan.seam, plan.travel)
                        != (layer.plan.surface, layer.plan.cap, layer.plan.seam, layer.plan.travel)
                        || (plan.spot, plan.spot_core) != (layer.plan.spot, layer.plan.spot_core)
                    {
                        // A new tab bar height, DPI or setting: no motion, and the rows'
                        // surfaces made again, so nothing of them is held
                        layer.place(device, plan)?;
                        placed = true;
                        layers.changed = true;
                    }
                }
                (None, Some(layer)) if !layer.hidden => {
                    layer.hide()?;
                    layers.changed = true;
                }
                (None, _) => {}
            }
            // The pads waiting on tasks, with their turning arcs, and the Conductor's outlines
            if let (Some(plan), Some(layer)) = (panel, layers.panel.as_mut()) {
                // LBTerm: under a menu's card none of it, the rows and the marker below included,
                // is applied or drawn (`frames`), and the first frame after the card has gone
                // takes what changed meanwhile whole, from what the tree still shows: a tab closed
                // by Close tab as the card fades out leaves, and the rows below slide up, once
                // the card has gone (Lachlan's 7b of 2026-11-08; lbterm_motion::rail::Hold; FLOWS.md,
                // "A menu, from what opens it to the end of its fade", step 10). The paint after
                // the card's fade is asked for by render/composited.rs's `tab_panel_hold_wake`,
                // as for the panel's edge, held by its hover through the same cards
                let rows = !layer.row_visuals.is_empty();
                let under = layer.hold.step(plan.cards && !layer.hidden, placed, rows, plan.spin);
                match under {
                    lbterm_motion::rail::Under::Held => {
                        log::trace!("rail: held under a menu's card ({} rows planned, {} shown)", plan.rows.len(), layer.row_visuals.len())
                    }
                    lbterm_motion::rail::Under::Released => log::debug!("rail: let go, the menu's card gone"),
                    lbterm_motion::rail::Under::Apply => {}
                }
                let held = under == lbterm_motion::rail::Under::Held;
                if !held && !layer.hidden && layer.sync_spinners(self, device, plan)? {
                    layers.changed = true;
                }
                if !held && !layer.hidden && layer.sync_rings(self, device, plan)? {
                    layers.changed = true;
                }
                // LBTerm: the tabs' rows and the open tab's marker, moving as tabs come, go and
                // change places (render/tab_panel.rs); straight there while the window resizes
                if !held && !layer.hidden && layer.sync_rows(self, plan, motion.enabled && !resized, now)? {
                    layers.changed = true;
                }
            }

            let exiting = std::mem::take(&mut layers.exiting);
            for (id, pane, done, over) in exiting {
                if resized || done <= now {
                    if done > now {
                        log::debug!("panes: pane {id}'s fade cut short, the window resized");
                    }
                    self.retire(layers, pane)?;
                } else {
                    layers.exiting.push((id, pane, done, over));
                }
            }
            // LBTerm: the tabs in flight go with their panes, all of them as the window resizes
            if resized {
                layers.flight.clear();
            } else {
                layers.flight.put_away_done(self.clock(now));
            }
            // LBTerm: the outlines of tabs left done fading with their panes, or cut short with
            // them as the window resizes or motion goes off
            if self.put_away_outlines(layers, |f| resized || !motion.enabled || f.done <= now)? {
                layers.changed = true;
            }

            // While the panel is open the panes show pressed into the room
            // right of it; the panel's edge curve carries them there and back
            let press = panel.filter(|p| p.open);
            let squeezed = press.is_some();
            let pressing = squeezed != layers.squeezed;
            layers.squeezed = squeezed;
            let display = |r: PixelRect| match press {
                Some(p) => r.squeeze(p.panes_left, p.travel as i32, p.panes_right),
                None => r,
            };
            let layout_curve = match (pressing, panel) {
                (true, Some(p)) => p.edge.clone(),
                _ => motion.layout.clone(),
            };

            let panes: Vec<(PaneId, PixelRect, PixelRect)> = panes
                .iter()
                .copied()
                .filter(|(_, r)| r.w > 0 && r.h > 0)
                .map(|(id, r)| (id, r, display(r)))
                .collect();
            layers.most_panes = layers.most_panes.max(panes.len());
            // Planned against where panes were going, not where they are
            // mid-animation, so a frame painted during motion changes nothing
            let changes = if motion.enabled && !resized {
                let old: Vec<(PaneId, Rect)> = layers
                    .panes
                    .iter()
                    .map(|(id, p)| (*id, p.shown.to_rect()))
                    .collect();
                let new: Vec<(PaneId, Rect)> =
                    panes.iter().map(|(id, _, s)| (*id, s.to_rect())).collect();
                let changes = lbterm_motion::plan(&old, &new);
                if changes.is_empty() {
                    // Another tab's panes: the panes of the tab left fade into them
                    let changes = lbterm_motion::dissolve(&old, &new, left);
                    if changes.is_empty()
                        && log::log_enabled!(log::Level::Debug)
                        && !old.is_empty()
                        && !new.is_empty()
                        && old.iter().all(|(id, _)| !new.iter().any(|(n, _)| n == id))
                    {
                        log::debug!("panes: {old:?} gave way to {new:?} at once: still in the tab shown, or under the Conductor's view");
                    }
                    changes
                } else {
                    changes
                }
            } else {
                vec![]
            };

            // Who was on screen, for the outline and the dimming
            let was_on_screen: Vec<PaneId> = layers.panes.iter().map(|(id, _)| *id).collect();
            // How the pane with the keyboard moved this frame, for its outline
            let mut active_curve = None;
            let mut gone = std::mem::take(&mut layers.panes);
            for &(id, rect, shown) in &panes {
                let change = changes.iter().find(|c| c.id() == id);
                if Some(id) == active {
                    active_curve = match change {
                        Some(Change::Layout { .. }) => Some(layout_curve.clone()),
                        Some(Change::Enter { .. } | Change::FadeIn { .. }) => Some(motion.layout.clone()),
                        _ => None,
                    };
                }
                let mut returning = false;
                let mut pane = match gone.iter().position(|(pane, _)| *pane == id) {
                    Some(i) => gone.swap_remove(i).1,
                    None => {
                        // Back before it finished fading out: pick it up
                        // where it is, otherwise a spare or a new visual.
                        // `remove`, so the rest stay oldest first
                        let fading = layers.exiting.iter().position(|(pane, ..)| *pane == id);
                        let pane = match fading {
                            Some(i) => {
                                let (_, pane, _, over) = layers.exiting.remove(i);
                                unsafe { self.panes.RemoveVisual(&pane.container)? };
                                if over {
                                    layers.returned.push(id);
                                }
                                returning = true;
                                pane
                            }
                            None => match layers.spare.pop() {
                                Some(mut pane) => {
                                    pane.place(device, rect, shown)?;
                                    pane
                                }
                                None => PaneLayer::new(self, instance, device, base, rect, shown, card_layers)?,
                            },
                        };
                        unsafe {
                            self.panes.AddVisual(
                                &pane.container,
                                false,
                                None::<&IDCompositionVisual>,
                            )?
                        };
                        layers.changed = true;
                        pane
                    }
                };
                match change {
                    Some(Change::Layout { .. }) => {
                        pane.move_to(self, device, rect, shown, &layout_curve, now)?;
                        layers.changed = true;
                    }
                    Some(Change::Enter { from, bounds, .. }) => {
                        // A pane coming back mid-fade starts from its opacity
                        let alpha = if pane.alpha.is_some() {
                            pane.current_alpha(now)
                        } else {
                            0.0
                        };
                        pane.ride_in(self, device, *from, rect, shown, *bounds, motion, alpha)?;
                        layers.changed = true;
                    }
                    Some(Change::FadeIn { .. }) => {
                        let alpha = if pane.alpha.is_some() {
                            pane.current_alpha(now)
                        } else {
                            0.0
                        };
                        pane.place(device, rect, shown)?;
                        pane.fade(self, "pane.enter", alpha, 1.0, &motion.enter)?;
                        layers.changed = true;
                    }
                    _ => {
                        // Unchanged (or still heading where it's going), a
                        // tab switch, a resize, or motion off; back from a
                        // fade with nothing to carry it in, whole at once
                        if pane.layer.rect != rect
                            || pane.shown != shown
                            || (!motion.enabled && pane.alpha.is_some())
                            || returning
                        {
                            pane.place(device, rect, shown)?;
                            layers.changed = true;
                        }
                    }
                }
                // 85% without the keyboard: dimming as it loses it, at once
                // for a pane that wasn't on screen (a split, a tab switch) or
                // a new window background
                let level = lbterm_motion::dim_level(
                    Some(id) == active && !self.dim_all.get(),
                    lbterm_panel::palette::INACTIVE,
                );
                let on_screen = was_on_screen.contains(&id);
                if (level - pane.level).abs() > f64::EPSILON || !on_screen || pane.backdrop != backdrop {
                    let animate = motion.enabled && !resized && on_screen && pane.backdrop == backdrop;
                    pane.dim_to(self, level, animate.then_some(&motion.dim), backdrop, now)?;
                    layers.changed = true;
                } else if pane.untint_if_done(now)? {
                    layers.changed = true;
                }
                layers.panes.push((id, pane));
            }

            // The outline: onto the pane with the keyboard, gliding there
            // when the keyboard moved, following it when its pane moves
            let new_ids: Vec<PaneId> = panes.iter().map(|(id, ..)| *id).collect();
            let target = |id: PaneId| panes.iter().find(|(p, ..)| *p == id).map(|(_, _, shown)| *shown);
            // LBTerm: the panes of the tab left this frame, dissolving over the next tab's
            let leaving: Vec<PaneId> = changes
                .iter()
                .filter_map(|c| match c {
                    Change::Dissolve { id, .. } => Some(*id),
                    _ => None,
                })
                .collect();
            let focus = layers.focus.as_mut().expect("made above");
            match lbterm_motion::focus(layers.active, active, &was_on_screen, &new_ids) {
                lbterm_motion::Focus::Hide => {
                    if focus.pane.is_some() {
                        focus.hide()?;
                        layers.changed = true;
                    }
                }
                lbterm_motion::Focus::Place(id) => {
                    if let Some(shown) = target(id) {
                        if leaving.is_empty() {
                            focus.place(id, shown)?;
                            focus.show_at_once()?;
                        } else {
                            // LBTerm: the tab on screen was left and its panes dissolve: what of
                            // its outline the next tab's doesn't draw fades with them
                            self.switch_outline(
                                focus,
                                &mut layers.left_outlines,
                                &layers.returned,
                                &layers.flight,
                                card_layers,
                                id,
                                shown,
                                &leaving,
                                &motion.tab_switch,
                                now,
                            )?;
                        }
                        layers.changed = true;
                    }
                }
                lbterm_motion::Focus::Glide(id) => {
                    if let Some(shown) = target(id) {
                        if motion.enabled && !resized {
                            focus.move_to(self, id, shown, &motion.focus_move, now)?;
                        } else {
                            focus.place(id, shown)?;
                        }
                        layers.changed = true;
                    }
                }
                lbterm_motion::Focus::Follow(id) => {
                    if let Some(shown) = target(id).filter(|s| *s != focus.shown || focus.pane != Some(id)) {
                        match active_curve.filter(|_| motion.enabled && !resized) {
                            Some(curve) => focus.move_to(self, id, shown, &curve, now)?,
                            None => focus.place(id, shown)?,
                        }
                        layers.changed = true;
                    }
                }
            }
            // LBTerm: coming back up after a tab switched back to, but the window resized or
            // motion went off: all of it at once
            if focus.alpha.is_some() && focus.pane.is_some() && (resized || !motion.enabled) {
                focus.show_at_once()?;
                layers.changed = true;
            }
            layers.active = active;

            let fading = layers.exiting.len();
            for (id, mut pane) in gone {
                match changes.iter().find(|c| c.id() == id) {
                    Some(Change::Exit { .. }) => {
                        // Under the panes growing over where it was
                        unsafe {
                            self.panes.RemoveVisual(&pane.container)?;
                            self.panes
                                .AddVisual(&pane.container, true, None::<&IDCompositionVisual>)?;
                        }
                        let done = pane.fade_out(self, "pane.exit", None, &motion.exit, now)?;
                        layers.exiting.push((id, pane, done, false));
                        layers.changed = true;
                    }
                    Some(Change::Dissolve { .. }) => {
                        // Its tab was left: over the next tab's panes, showing its last frame
                        // where it was, going out by its tab's share of the screen
                        // (`switch_flight`, below), scaled by as much of itself as it showed
                        unsafe {
                            self.panes.RemoveVisual(&pane.container)?;
                            self.panes
                                .AddVisual(&pane.container, false, None::<&IDCompositionVisual>)?;
                        }
                        pane.own = pane.current_alpha(now);
                        layers.exiting.push((id, pane, now, true));
                        layers.dissolved.push(id);
                        layers.changed = true;
                    }
                    _ => self.retire(layers, pane)?,
                }
            }
            // A tab left while one left before it still fades: that one stays over this one, as
            // it was over its panes, and the oldest over them all
            if layers.exiting[fading..].iter().any(|(.., over)| *over) {
                for (.., pane, _, over) in layers.exiting[..fading].iter().rev() {
                    if *over {
                        unsafe {
                            self.panes.RemoveVisual(&pane.container)?;
                            self.panes
                                .AddVisual(&pane.container, false, None::<&IDCompositionVisual>)?;
                        }
                    }
                }
            }
            // LBTerm: the tabs in flight: a tab come back comes out of the stack and the tab left
            // goes in, and each whose opacity changed plays it again from now
            if !layers.dissolved.is_empty() || !layers.returned.is_empty() {
                self.switch_flight(layers, &motion.tab_switch, now)?;
            }
            Self::trim_spares(layers);
            Ok(())
        }

        /// LBTerm: the tabs in flight as the tab on screen gives way (`lbterm_motion::flight`):
        /// the tab whose panes came back (`returned`) comes out of the stack, whole, under it,
        /// and the panes that dissolved (`dissolved`) go in at the bottom as one, from what showed
        /// of their tab; the panes of every tab whose opacity has to change to keep its share
        /// play it again from now, each scaled by how much of itself it showed
        /// (`PaneLayer::own`), and are done when their tab is. What of a tab come back isn't on
        /// screen (a pane of it closed while it faded) goes with it, at once, and so does a tab
        /// left that showed nothing.
        fn switch_flight(&self, layers: &mut Layers, curve: &Curve, now: Instant) -> anyhow::Result<()> {
            let t = self.clock(now);
            let left = layers.dissolved.clone();
            let returned = layers.returned.clone();
            let switched = layers.flight.switch(
                (!left.is_empty()).then(|| left.clone()),
                |key| key.iter().any(|p| returned.contains(p)),
                t,
                curve,
                usize::MAX,
            );
            let mut gone: Vec<PaneId> = switched.back.iter().flat_map(|(key, _)| key.iter().copied()).collect();
            if let Some((_, share)) = &switched.back {
                log::debug!("panes: the tab of {returned:?} back from {share:.2}, {} still in flight", layers.flight.len());
            }
            if switched.left.is_none() && !left.is_empty() {
                log::debug!("panes: {left:?} showed nothing as their tab was left: gone at once");
                gone.extend(left.iter().copied());
                layers.dissolved.clear();
            }
            if !gone.is_empty() {
                for (id, pane, done, over) in std::mem::take(&mut layers.exiting) {
                    if over && gone.contains(&id) {
                        self.retire(layers, pane)?;
                    } else {
                        layers.exiting.push((id, pane, done, over));
                    }
                }
            }
            for &i in &switched.redrive {
                let leaving = &layers.flight.leaving()[i];
                let key = leaving.key.clone();
                let done = self.at_clock(leaving.end());
                let shown = leaving.share(t).0;
                let segments = layers.flight.opacity_segments(i, t, STEP, OPACITY_EPSILON);
                log::debug!(
                    "panes: {key:?} in flight, {} of {}, shown {shown:.2}, its opacity from {:.2}",
                    i + 1,
                    layers.flight.len(),
                    segments.value(0.0)
                );
                for (id, pane, end, over) in layers.exiting.iter_mut() {
                    if *over && key.contains(id) {
                        let own = pane.own;
                        pane.play_alpha(self, "pane.dissolve", segments.affine(0.0, own), now)?;
                        *end = done;
                    }
                }
                layers.redriven.push(key);
            }
            layers.changed = true;
            Ok(())
        }

        /// LBTerm: the panes whose visuals are still in the tree after leaving the layout,
        /// fading out (a closed pane's under the rest, a left tab's over them), for the state
        /// file; and when the soonest is done, to put it away then (`put_away_faded`), the
        /// left tabs' outlines included
        pub fn fading_panes(&self) -> (Vec<PaneId>, Option<Instant>) {
            let layers = self.layers.borrow();
            let outlines = layers.left_outlines.iter().map(|f| f.done);
            (
                layers.exiting.iter().map(|(id, ..)| *id).collect(),
                layers.exiting.iter().map(|(_, _, done, _)| *done).chain(outlines).min(),
            )
        }

        /// LBTerm: the outlines of tabs left that `gone` picks leave the tree; whether any did
        fn put_away_outlines(
            &self,
            layers: &mut Layers,
            mut gone: impl FnMut(&LeftOutline) -> bool,
        ) -> anyhow::Result<bool> {
            let mut any = false;
            for f in std::mem::take(&mut layers.left_outlines) {
                if gone(&f) {
                    unsafe { self.focus.RemoveVisual(&f.lamp)? };
                    any = true;
                } else {
                    layers.left_outlines.push(f);
                }
            }
            Ok(any)
        }

        /// LBTerm: the window's outline onto `pane` at `shown` as the tab on screen gives way to
        /// another whose panes dissolve over it (`leaving`, the panes going): there at once at
        /// full, or for a tab switched back to while its outline faded, coming up from nothing as
        /// its panes do, by what the tabs still in flight leave of the screen (`flight`, as it is
        /// before this switch), and everything that lies on its lines going as it comes from what
        /// it showed; what of the outline that was on screen the next one doesn't draw fades with
        /// its panes on `curve`, and the outlines already fading lose what lies on the window's
        /// lines (`lbterm_motion::outline::switch`)
        fn switch_outline(
            &self,
            focus: &mut FocusOutline,
            fading: &mut Vec<LeftOutline>,
            returned: &[PaneId],
            flight: &lbterm_motion::Flight<Vec<PaneId>>,
            cards: &Cards,
            pane: PaneId,
            shown: PixelRect,
            leaving: &[PaneId],
            curve: &Curve,
            now: Instant,
        ) -> anyhow::Result<()> {
            use lbterm_motion::outline::{switch, Fading};
            let ([x, y, w, h], _) = focus.current(now);
            let was = focus.pane.map(|_| (Rect::new(x, y, w, h), focus.opacity_at(now)));
            let seen: Vec<Fading> = fading
                .iter()
                .map(|f| Fading {
                    outline: f.outline.clone(),
                    alpha: f.current_alpha(now),
                    back: f.panes.iter().any(|p| returned.contains(p)),
                })
                .collect();
            let card = lbterm_motion::outline::Card {
                corner: cards.plan.corner as f64,
                reach: cards.plan.reach as f64,
            };
            let plan = switch(was, shown.to_rect(), &seen, card);
            // A tab switched back to comes up as the tabs still in flight leave it more of the
            // screen, each on its own (`Flight::uncovered_segments`, after this switch as
            // `switch_flight` makes it): how far up, 0 to 1, and how far down what goes as it
            // comes, with what its panes show now. With only the tab left in flight it's `curve`
            // itself
            let mut panes_from = None;
            let rise = plan.rises.then(|| {
                let t = self.clock(now);
                let mut after = flight.clone();
                after.switch(
                    (!leaving.is_empty()).then(|| leaving.to_vec()),
                    |k| k.iter().any(|p| returned.contains(p)),
                    t,
                    curve,
                    usize::MAX,
                );
                let up = after.uncovered_segments(t, STEP, OPACITY_EPSILON);
                let from = up.value(0.0);
                panes_from = Some(from);
                (from < 0.99 && up.end > 0.0).then(|| {
                    let k = 1.0 / (1.0 - from);
                    (up.affine(-from * k, k), up.affine(k, -k))
                })
            });
            let rise = rise.flatten();
            let coming = rise.as_ref().map(|r| &r.0);
            let going = rise.as_ref().map(|r| &r.1);
            // A tab switched back to: what of the outlines still fading lies on the window's
            // lines (its own tab's pieces too) goes on from what it showed as the window's comes
            // up, in its own look
            let split: Vec<_> = plan
                .split
                .iter()
                .map(|(i, part, from)| (part.clone(), *from, fading[*i].look, fading[*i].panes.clone()))
                .collect();
            for (mut f, keep) in std::mem::take(fading).into_iter().zip(plan.keep) {
                if keep.is_empty() {
                    // All of it drawn by the window's outline now, or its tab came back
                    unsafe { self.focus.RemoveVisual(&f.lamp)? };
                } else {
                    if keep != f.outline {
                        f.outline = keep;
                        f.fit(self, cards)?;
                    }
                    fading.push(f);
                }
            }
            focus.place(pane, shown)?;
            if plan.rises {
                log::debug!(
                    "outline: turned round, its tab switched back to, its panes showing {:.2}: coming up from nothing {}",
                    panes_from.unwrap_or(1.0),
                    if coming.is_some() { "with them" } else { "on tab_switch" }
                );
                focus.rise(self, 0.0, curve, coming)?;
            } else {
                focus.show_at_once()?;
            }
            for (outline, from, look, panes) in split {
                log::debug!("outline: {} pieces of one still fading on its lines go on from {from:.2} as it comes", outline.pieces());
                fading.push(LeftOutline::new(self, focus, look, cards, outline, from, curve, going, panes, now)?);
            }
            // Nothing shows of a lamp that's out (the window without the keyboard)
            let look = focus.look_at(now);
            if look.light > OPACITY_EPSILON {
                for (outline, from) in plan.under {
                    log::debug!("outline: {} pieces of it on the lines of the one come back go from {from:.2} as it comes", outline.pieces());
                    fading.push(LeftOutline::new(self, focus, look, cards, outline, from, curve, going, leaving.to_vec(), now)?);
                }
                for (outline, from) in plan.leave {
                    log::debug!("outline: {} pieces of it fade with the tab left, from {from:.2}", outline.pieces());
                    fading.push(LeftOutline::new(self, focus, look, cards, outline, from, curve, None, leaving.to_vec(), now)?);
                }
            }
            Ok(())
        }

        /// LBTerm: the panes done fading out leave the tree, and their surfaces go back to the
        /// pool, without a frame: nothing on screen changes (they're at nothing), so there's
        /// nothing to draw or present, only the tree to commit. So do the strips' pictures and
        /// the strip's words in flight done fading with them. `prepare` does the same on
        /// any frame painted after they're done. True when a frame should do it instead: the
        /// tree holds changes no frame has committed yet (a paint that failed between `prepare`
        /// and its present), which a commit here would show with visuals never presented into
        pub fn put_away_faded(&self) -> anyhow::Result<bool> {
            let now = Instant::now();
            if self.layers.borrow().changed {
                log::debug!("panes: the faded panes left for the next frame, which has a change to commit");
                return Ok(true);
            }
            // The strips' pictures done fading with them (render/sides.rs), and the strip's
            // older swaps (render/strip_swap.rs)
            let sides = self.put_away_sides(|p| p.done <= now)?;
            let swaps = self.put_away_done_swaps(now)?;
            let mut layers = self.layers.borrow_mut();
            let layers = &mut *layers;
            layers.flight.put_away_done(self.clock(now));
            let mut gone = vec![];
            for (id, pane, done, over) in std::mem::take(&mut layers.exiting) {
                if done <= now {
                    gone.push(id);
                    self.retire(layers, pane)?;
                } else {
                    layers.exiting.push((id, pane, done, over));
                }
            }
            // And what fades of the left tabs' outlines with their panes
            let outlines = self.put_away_outlines(layers, |f| f.done <= now)?;
            if gone.is_empty() && !sides && !swaps && !outlines {
                return Ok(false);
            }
            Self::trim_spares(layers);
            log::debug!(
                "panes: put away without a frame: panes {gone:?}{}{}{}",
                if sides { ", the strips' pictures" } else { "" },
                if swaps { ", the strip's words" } else { "" },
                if outlines { ", the outlines of tabs left" } else { "" }
            );
            // Committed here, so the next frame needn't
            layers.changed = false;
            self.commit()?;
            Ok(false)
        }

        /// LBTerm: the active outline's light for the window's keyboard (`Keyboard`): out
        /// without it, a pulse as it arrives from outside. Call after `prepare`.
        pub fn prepare_keyboard(&self, keyboard: Keyboard, motion: &MotionSettings) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut layers = self.layers.borrow_mut();
            let animate = motion.enabled && !layers.resizing;
            if let Some(focus) = layers.focus.as_mut() {
                if focus.light(self, keyboard, motion, animate, now)? {
                    layers.changed = true;
                }
            }
            Ok(())
        }

        /// This frame's textures for the card templates, `over`, the panel's
        /// visuals and every pane visual. The window background's comes from
        /// `WebGpuState::surface`.
        pub fn acquire(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let mut layers = self.layers.borrow_mut();
            let mut frames = vec![];
            if let Some(cards) = &*self.card_layers.borrow() {
                cards.frames(&mut frames, content)?;
            }
            if let Some(over) = &layers.over {
                frames.extend(over.frame(Target::Over, content)?);
            }
            // LBTerm: the tab menu, while it's open and once more to clear it
            if layers.top_wanted || layers.top_shown {
                if let Some(top) = &layers.top {
                    frames.extend(top.frame(Target::Top, content)?);
                }
            }
            layers.top_shown = layers.top_wanted;
            let layers = &*layers;
            if let Some(panel) = layers.panel.as_ref().filter(|p| !p.hidden) {
                panel.frames(&mut frames, content)?;
            }
            for (id, pane) in &layers.panes {
                frames.extend(pane.layer.frame(Target::Pane(*id), content)?);
            }
            Ok(frames)
        }

        /// Whether the panes show anywhere but where they are: the panel is
        /// pressing them over. The mouse can't be trusted to land where it
        /// looks until it stops.
        pub fn panes_pressed(&self) -> bool {
            self.layers.borrow().squeezed
        }

        /// The look the tab panel's wave holds (`WavePlan::look`), once it's been drawn: the
        /// paint draws it again only when the look it wants is another
        pub fn panel_wave_drawn(&self) -> Option<u64> {
            self.layers.borrow().panel.as_ref()?.wave.as_ref()?.drawn
        }

        /// Commit the tree if this frame changed it. Call after presenting,
        /// so new visuals appear with their first frame and every animation
        /// of a layout change starts together.
        pub fn commit_if_changed(&self) -> anyhow::Result<()> {
            let changed = std::mem::take(&mut self.layers.borrow_mut().changed);
            if changed {
                self.commit()?;
            }
            Ok(())
        }

        /// Apply changes to the visual tree. Presents to the swapchains
        /// don't need this; new visuals, offsets and animations do.
        pub fn commit(&self) -> anyhow::Result<()> {
            unsafe { self.device.Commit() }.context("IDCompositionDevice::Commit")?;
            // How late this commit's animations start against the curves `prepare` worked out
            // (PITFALLS 304): a line a commit that follows a prepare, with the names started
            // since it, only at trace
            if log::log_enabled!(target: "lbterm::compositor::lag", log::Level::Trace) {
                if let Some(at) = self.prepared.take() {
                    let started: Vec<&'static str> = self
                        .in_flight
                        .borrow()
                        .list()
                        .iter()
                        .filter(|(_, start, _)| *start >= at)
                        .map(|(name, _, _)| *name)
                        .collect();
                    log::trace!(
                        target: "lbterm::compositor::lag",
                        "commit {:.3} ms after prepare; started: {}",
                        at.elapsed().as_secs_f64() * 1000.0,
                        if started.is_empty() { "none".to_string() } else { started.join(" ") }
                    );
                }
            }
            Ok(())
        }

        /// LBTerm: the strip's hover slots for this frame (render/hover_fade.rs), over the
        /// strip at `strip`, each slot's opacity starting what `fades` says on
        /// `button_hover`. Without a strip they go dark. Made the first time a hover fades;
        /// call after `prepare`.
        pub fn prepare_hover(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            strip: Option<PixelRect>,
            fades: [Option<Fade>; SLOTS],
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut hover = self.hover_layers.borrow_mut();
            let rect = match strip {
                Some(rect) => rect,
                None => {
                    if hover.wanted {
                        for slot in &mut hover.slots {
                            slot.settle(0.0)?;
                        }
                        hover.wanted = false;
                        self.layers.borrow_mut().changed = true;
                    }
                    return Ok(());
                }
            };
            if hover.slots.is_empty() {
                if fades.iter().all(Option::is_none) {
                    return Ok(());
                }
                for _ in 0..SLOTS {
                    let visual = unsafe { self.device.CreateVisual()? };
                    unsafe { self.hover.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                    let layer = Layer::new(
                        instance,
                        device,
                        visual,
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        rect,
                    )?;
                    let opacity: IDCompositionVisual3 = layer
                        .visual
                        .cast()
                        .context("IDCompositionVisual3 (Windows 10 or later)")?;
                    // Unseen until a fade says otherwise, after its first frame
                    unsafe { opacity.SetOpacity2(0.0)? };
                    hover.slots.push(HoverSlot {
                        layer,
                        opacity,
                        alpha: None,
                        rest: 0.0,
                    });
                }
            }
            // The strip changing size is the window being resized: nothing fades then
            let resized = hover.slots.iter().any(|s| s.layer.rect != rect);
            let mut changed = resized;
            for slot in &mut hover.slots {
                slot.layer.place(device, rect)?;
                if resized {
                    slot.settle(slot.rest)?;
                }
            }
            for (slot, fade) in hover.slots.iter_mut().zip(fades) {
                let fade = match fade {
                    Some(fade) => fade,
                    None => continue,
                };
                let to = if fade == Fade::Out { 0.0 } else { 1.0 };
                if !motion.enabled || resized {
                    slot.settle(to)?;
                } else {
                    let from = if fade == Fade::InFromZero {
                        0.0
                    } else {
                        slot.current(now)
                    };
                    let opacity = &slot.opacity;
                    self.name("strip.hover");
                    slot.alpha = self.drive(
                        Motion::new(from, to, 0.0, motion.button_hover.clone()),
                        OPACITY_EPSILON,
                        0.0,
                        |a| unsafe { opacity.SetOpacity(a) },
                        |v| unsafe { opacity.SetOpacity2(v) },
                    )?;
                    slot.rest = to;
                }
                changed = true;
            }
            hover.wanted = true;
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: this frame's textures for the hover slots, while there's a strip
        pub fn acquire_hover(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let hover = self.hover_layers.borrow();
            if !hover.wanted {
                return Ok(vec![]);
            }
            let mut frames = vec![];
            for (i, slot) in hover.slots.iter().enumerate() {
                frames.extend(slot.layer.frame(Target::Hover(i), content)?);
            }
            Ok(frames)
        }

        /// LBTerm: the strip's detail card for this frame (render/strip.rs): its visual at
        /// `card`, fading in on `pane_enter` when it's opening and out on `pane_exit` when it's
        /// closing, from wherever it is. `None` puts it away. Made the first time the card
        /// opens; call after `prepare`.
        pub fn prepare_strip_card(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            card: Option<(PixelRect, bool)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut strip = self.strip_card_layer.borrow_mut();
            let (rect, open) = match card {
                Some(card) => card,
                None => {
                    if strip.wanted {
                        strip.settle(0.0)?;
                        strip.wanted = false;
                        self.layers.borrow_mut().changed = true;
                    }
                    strip.open = false;
                    return Ok(());
                }
            };
            let mut changed = false;
            match strip.layer.as_mut() {
                Some(layer) => {
                    if layer.rect != rect {
                        layer.place(device, rect)?;
                        changed = true;
                    }
                }
                None => {
                    let visual = unsafe { self.device.CreateVisual()? };
                    unsafe {
                        self.strip_card
                            .AddVisual(&visual, false, None::<&IDCompositionVisual>)?
                    };
                    let layer = Layer::new(
                        instance,
                        device,
                        visual,
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        rect,
                    )?;
                    let opacity: IDCompositionVisual3 = layer
                        .visual
                        .cast()
                        .context("IDCompositionVisual3 (Windows 10 or later)")?;
                    // Unseen until it fades in, after its first frame
                    unsafe { opacity.SetOpacity2(0.0)? };
                    strip.layer = Some(layer);
                    strip.opacity = Some(opacity);
                    strip.rest = 0.0;
                    strip.open = false;
                    changed = true;
                }
            }
            if open != strip.open {
                let to = if open { 1.0 } else { 0.0 };
                if motion.enabled {
                    let from = strip.current(now);
                    let curve = if open {
                        motion.enter.clone()
                    } else {
                        motion.exit.clone()
                    };
                    let opacity = strip.opacity.clone().context("the card's visual")?;
                    self.name("strip.card");
                    strip.alpha = self.drive(
                        Motion::new(from, to, 0.0, curve),
                        OPACITY_EPSILON,
                        0.0,
                        |a| unsafe { opacity.SetOpacity(a) },
                        |v| unsafe { opacity.SetOpacity2(v) },
                    )?;
                    strip.rest = to;
                } else {
                    strip.settle(to)?;
                }
                strip.open = open;
                changed = true;
            }
            strip.wanted = true;
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: the turning arc on a Conductor menu item waiting for its answer
        /// (render/conductor.rs), over the menu at `rect`, or none. Made the first time an item
        /// waits: a template in a group clipped to nothing, and a visual that shows it turning, a
        /// turn every SPIN_PERIOD, in DWM, while `turning` (motion on and the window seen:
        /// lbterm_extras::work::loops_play); still otherwise. Both are children of the Conductor
        /// slot's card group (`MenuCard`, made by `prepare_menus` earlier in the same frame), the
        /// arc over its highlight, so the arc fades in and out with the card (PITFALLS 155). Once
        /// nothing waits it stops, since a turn DWM plays for ever keeps it composing every refresh
        /// whether the arc shows or not (lbterm/COMPOSITOR.md, "Loops"): held where it is while the
        /// card fades out, so it goes with the card, and hidden at once while the card stays open
        pub fn prepare_menu_spinner(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            rect: Option<PixelRect>,
            turning: bool,
        ) -> anyhow::Result<()> {
            let mut spin = self.menu_spinner.borrow_mut();
            let Some(rect) = rect else {
                // The Conductor's card fading out: the arc stays, still where it is, and fades
                // with it; it's never drawn into again (PITFALLS 147)
                let closing = !self.menu_cards.borrow()[CONDUCTOR_SLOT].open;
                if closing {
                    spin.hold()?;
                } else if spin.showing {
                    if let Some(opacity) = &spin.opacity {
                        unsafe { opacity.SetOpacity2(0.0)? };
                    }
                    spin.showing = false;
                    self.layers.borrow_mut().changed = true;
                }
                spin.wanted = false;
                spin.turn(self, false)?;
                return Ok(());
            };
            if spin.template.is_none() {
                // Over the Conductor's card and its highlight, in its group, so it fades with it
                let card = self.menu_cards.borrow()[CONDUCTOR_SLOT]
                    .group
                    .clone()
                    .context("the Conductor menu's card")?;
                let group = unsafe { self.device.CreateVisual()? };
                let nothing = unsafe { self.device.CreateRectangleClip()? };
                let held = unsafe { self.device.CreateVisual()? };
                let visual = unsafe { self.device.CreateVisual()? };
                let rotate = unsafe { self.device.CreateRotateTransform()? };
                unsafe {
                    nothing.SetLeft2(0.0)?;
                    nothing.SetTop2(0.0)?;
                    nothing.SetRight2(0.0)?;
                    nothing.SetBottom2(0.0)?;
                    group.SetClip(&nothing)?;
                    card.AddVisual(&group, false, None::<&IDCompositionVisual>)?;
                    group.AddVisual(&held, false, None::<&IDCompositionVisual>)?;
                    card.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                    visual.SetTransform(&rotate)?;
                }
                let template = Layer::new(
                    instance,
                    device,
                    held,
                    base,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                    rect,
                )?;
                spin.opacity = Some(visual.cast().context("IDCompositionVisual3 (Windows 10 or later)")?);
                spin.template = Some(template);
                spin.group = Some(group);
                spin.visual = Some(visual);
                spin.rotate = Some(rotate);
                spin.rect = PixelRect::default();
            }
            if spin.rect != rect || !spin.wanted {
                let template = spin.template.as_mut().context("the arc's template")?;
                template.place(device, rect)?;
                let content = swap_chain(&template.surface)?;
                let (visual, rotate, opacity) = (
                    spin.visual.as_ref().context("the arc's visual")?,
                    spin.rotate.as_ref().context("the arc's turn")?,
                    spin.opacity.as_ref().context("the arc's opacity")?,
                );
                unsafe {
                    visual.SetContent(&content)?;
                    visual.SetOffsetX2(rect.x as f32)?;
                    visual.SetOffsetY2(rect.y as f32)?;
                    rotate.SetCenterX2(rect.w as f32 / 2.0)?;
                    rotate.SetCenterY2(rect.h as f32 / 2.0)?;
                    opacity.SetOpacity2(1.0)?;
                }
                spin.rect = rect;
                spin.wanted = true;
                spin.showing = true;
                self.layers.borrow_mut().changed = true;
            }
            spin.turn(self, turning)?;
            Ok(())
        }

        /// LBTerm: this frame's texture for the menu's turning arc, while an item waits
        pub fn acquire_menu_spinner(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let spin = self.menu_spinner.borrow();
            match (&spin.template, spin.wanted) {
                (Some(template), true) => Ok(template.frame(Target::MenuSpinner, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: the lights running round a tab link drag's matched blocks (render/link_drag.rs;
        /// Lachlan's E6), or none. The light is drawn once into a template in a group clipped to
        /// nothing; each block has a visual in `top`, over the drag's cards, showing it, carried
        /// round the block lap after lap by DWM (`lap`), still at its first place with motion off.
        /// A plan the same as the last changes nothing, so the lights carry on where they are.
        pub fn prepare_link_lamps(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: Option<super::LampPlan>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let mut lamps = self.link_lamps.borrow_mut();
            if lamps.plan == plan {
                return Ok(());
            }
            for gone in lamps.lamps.drain(..) {
                unsafe { self.top.RemoveVisual(&gone)? };
            }
            self.layers.borrow_mut().changed = true;
            let Some(want) = plan else {
                lamps.plan = None;
                lamps.turning = false;
                return Ok(());
            };
            if lamps.template.is_none() {
                let group = unsafe { self.device.CreateVisual()? };
                let nothing = unsafe { self.device.CreateRectangleClip()? };
                let held = unsafe { self.device.CreateVisual()? };
                unsafe {
                    nothing.SetLeft2(0.0)?;
                    nothing.SetTop2(0.0)?;
                    nothing.SetRight2(0.0)?;
                    nothing.SetBottom2(0.0)?;
                    group.SetClip(&nothing)?;
                    self.top.AddVisual(&group, false, None::<&IDCompositionVisual>)?;
                    group.AddVisual(&held, false, None::<&IDCompositionVisual>)?;
                }
                lamps.template = Some(Layer::new(
                    instance,
                    device,
                    held,
                    base,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                    want.template,
                )?);
                lamps.group = Some(group);
            }
            let template = lamps.template.as_mut().context("the lamp's template")?;
            template.place(device, want.template)?;
            let content = swap_chain(&template.surface)?;
            for path in want.paths.iter().filter(|p| !p.is_empty()) {
                let lamp = unsafe { self.device.CreateVisual()? };
                unsafe {
                    self.top.AddVisual(&lamp, false, None::<&IDCompositionVisual>)?;
                    lamp.SetContent(&content)?;
                }
                if motion.enabled {
                    let xs: Vec<f32> = path.iter().map(|o| o.0).collect();
                    let ys: Vec<f32> = path.iter().map(|o| o.1).collect();
                    let x = self.lap(&xs, want.lap)?;
                    let y = self.lap(&ys, want.lap)?;
                    unsafe {
                        lamp.SetOffsetX(&x)?;
                        lamp.SetOffsetY(&y)?;
                    }
                } else {
                    unsafe {
                        lamp.SetOffsetX2(path[0].0)?;
                        lamp.SetOffsetY2(path[0].1)?;
                    }
                }
                lamps.lamps.push(lamp);
            }
            lamps.turning = motion.enabled && !lamps.lamps.is_empty();
            lamps.plan = Some(want);
            Ok(())
        }

        /// LBTerm: this frame's texture for a tab link drag's light, while it shows
        pub fn acquire_link_lamps(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let lamps = self.link_lamps.borrow();
            match (&lamps.template, lamps.plan.is_some()) {
                (Some(template), true) => Ok(template.frame(Target::LinkLamp, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: the lights running to and fro along the rail's suggested tab connections
        /// (render/connections.rs; lbterm/LINKS.md, "Connections"), or none. As a drag's lamps
        /// are: the light drawn once into a template in a group clipped to nothing, and a visual
        /// per line in `panel_lights` (over the rail's rows, under the tasks card and the tab
        /// menu) showing it. Each runs its laps (lbterm_extras::glow, Lachlan's 1a: three, when a
        /// suggestion comes or the file changes what it says of one, and when the pointer comes
        /// onto the rail), carried along its line by DWM and ending at its middle, and is still
        /// there between them: a lap repeated for as long as a suggestion showed kept DWM
        /// composing every refresh for hours. Laps run only while `play`
        /// (lbterm_extras::work::loops_play: motion on and the window seen), as the rail's loops
        /// do (lbterm/COMPOSITOR.md, "Loops"). A line whose light does what it did is left as it
        /// is, so DWM carries on with it; LBTerm paints nothing for them. `loops` names them while
        /// laps run. A lamp whose laps have ended is set still at its line's middle by value at
        /// the next paint (`settle_lamps`), so the tree keeps no animation for it.
        pub fn prepare_connection_lamps(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: Option<super::GlowPlan>,
            play: bool,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut lamps = self.connection_lamps.borrow_mut();
            let lamps = &mut *lamps;
            // Nothing new (the plan and play as they were, and no laps owed that could start
            // now): DWM goes on with what it was given, and only laps that have ended are let go.
            // The rest (the template placed, its swapchain asked for, a key of each path, the
            // lamps taken and kept again) runs only when something changed, not every frame
            // while a suggestion shows, which on Lachlan's rail is hours (PITFALLS 15 and 62)
            if lamps.plan == plan && lamps.play == play && !(play && lamps.laps.owed()) {
                if settle_lamps(&mut lamps.lamps, now)? {
                    self.layers.borrow_mut().changed = true;
                }
                return Ok(());
            }
            lamps.play = play;
            let Some(want) = plan else {
                if !lamps.lamps.is_empty() {
                    for gone in lamps.lamps.drain(..) {
                        unsafe { self.panel_lights.RemoveVisual(&gone.visual)? };
                    }
                    self.layers.borrow_mut().changed = true;
                }
                lamps.laps.update(&[], play, now);
                lamps.shown = false;
                lamps.plan = None;
                return Ok(());
            };
            lamps.shown = true;
            if lamps.template.is_none() {
                let group = unsafe { self.device.CreateVisual()? };
                let nothing = unsafe { self.device.CreateRectangleClip()? };
                let held = unsafe { self.device.CreateVisual()? };
                unsafe {
                    nothing.SetLeft2(0.0)?;
                    nothing.SetTop2(0.0)?;
                    nothing.SetRight2(0.0)?;
                    nothing.SetBottom2(0.0)?;
                    group.SetClip(&nothing)?;
                    self.panel_lights.AddVisual(&group, false, None::<&IDCompositionVisual>)?;
                    group.AddVisual(&held, false, None::<&IDCompositionVisual>)?;
                }
                lamps.template = Some(Layer::new(
                    instance,
                    device,
                    held,
                    base,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                    want.template,
                )?);
                lamps.group = Some(group);
            }
            let template = lamps.template.as_mut().context("the connection light's template")?;
            let mut changed = false;
            // Its swapchain asked for (a QueryInterface) only when the surface is new or was
            // configured again, which can make another: one moved keeps its own. Made again,
            // every lamp shows the new one, its laps going on as they are
            if template.place(device, want.template)? || lamps.content.is_none() {
                let content = swap_chain(&template.surface)?;
                if lamps.content.as_ref() != Some(&content) {
                    for lamp in &lamps.lamps {
                        unsafe { lamp.visual.SetContent(&content)? };
                    }
                    lamps.content = Some(content);
                    changed |= !lamps.lamps.is_empty();
                }
            }
            let lines: Vec<lbterm_extras::glow::Line> = want
                .lines
                .iter()
                .map(|l| lbterm_extras::glow::Line {
                    id: &l.id,
                    says: l.says,
                    place: place_key(&l.path, l.lap),
                    lap: l.lap,
                })
                .collect();
            let glows = lamps.laps.update(&lines, play, now);
            let mut kept = std::mem::take(&mut lamps.lamps);
            for (line, glow) in want.lines.iter().zip(glows) {
                if line.path.is_empty() {
                    continue;
                }
                if let Some(i) = kept.iter().position(|k| k.id == line.id) {
                    let k = &kept[i];
                    if k.path == line.path && k.lap == line.lap && k.glow.shows_as(&glow, k.lap, now) {
                        lamps.lamps.push(kept.swap_remove(i));
                        continue;
                    }
                    let gone = kept.swap_remove(i);
                    unsafe { self.panel_lights.RemoveVisual(&gone.visual)? };
                }
                let visual = unsafe { self.device.CreateVisual()? };
                let ys: Vec<f32> = line.path.iter().map(|o| o.1).collect();
                unsafe {
                    self.panel_lights.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                    visual.SetContent(lamps.content.as_ref().context("the light's swapchain")?)?;
                    // Its line is upright: only the light's height along it moves
                    visual.SetOffsetX2(line.path[0].0)?;
                }
                match glow {
                    lbterm_extras::glow::Glow::Laps { laps, .. } => {
                        let (segments, end) = lbterm_extras::glow::segments(&ys, line.lap, laps);
                        let y = unsafe { self.device.CreateAnimation()? };
                        for (begin, value, slope) in segments {
                            unsafe { y.AddCubic(begin, value, slope, 0.0, 0.0)? };
                        }
                        // Then still at the middle, where it began, until the first paint after
                        // sets it there by value (`settle_lamps`)
                        unsafe {
                            y.End(end, ys[0])?;
                            visual.SetOffsetY(&y)?;
                        }
                    }
                    lbterm_extras::glow::Glow::Still => unsafe { visual.SetOffsetY2(ys[0])? },
                }
                lamps.lamps.push(ConnectionLamp {
                    id: line.id.clone(),
                    lap: line.lap,
                    path: line.path.clone(),
                    glow,
                    visual,
                });
                changed = true;
            }
            for gone in kept {
                unsafe { self.panel_lights.RemoveVisual(&gone.visual)? };
                changed = true;
            }
            changed |= settle_lamps(&mut lamps.lamps, now)?;
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            lamps.plan = Some(want);
            Ok(())
        }

        /// LBTerm: the pointer came onto the rail: every suggested line's light that's still runs
        /// its laps (lbterm_extras::glow). Whether that owes laps that can play, so a frame should
        /// come to start them: none with motion off or the window unseen (PITFALLS 66)
        pub fn connection_lamps_pointer(&self) -> bool {
            let mut lamps = self.connection_lamps.borrow_mut();
            lamps.shown && lamps.laps.pointer_came(Instant::now())
        }

        /// LBTerm: when the suggested lines' laps DWM is playing end, for the state file's frame
        /// that says so (termwindow/state.rs)
        pub fn loops_until(&self) -> Option<Instant> {
            let now = Instant::now();
            let lamps = self.connection_lamps.borrow();
            lamps.lamps.iter().filter_map(|l| l.glow.until(l.lap)).filter(|&u| u > now).max()
        }

        /// LBTerm: this frame's texture for the suggested tab connections' light, while it shows
        pub fn acquire_connection_lamps(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let lamps = self.connection_lamps.borrow();
            match (&lamps.template, lamps.shown) {
                (Some(template), true) => Ok(template.frame(Target::ConnectionLamp, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: the easter egg's bursts of hearts new this frame handed to DWM, those in the
        /// air moved with their panes, and those over put away (render/triggers.rs). Their sheet
        /// is drawn once into a template never seen itself; each heart is a visual in `lights`
        /// showing its pink's square of it, a burst's hearts in a group cut off at their pane's
        /// top, and DWM plays each one's rise, sway, growth and fade from
        /// lbterm_triggers::hearts::Path::tracks, where the GUI drew the whole window again
        /// every 16 ms for the burst's two seconds to move them. A burst's group moves with its
        /// pane (a split, a zoom, the window resized) and is cut to nothing while its pane isn't
        /// shown (its tab switched from, another pane zoomed), as the GUI's own drawing of them
        /// does (lbterm_triggers::hearts::follow). With motion off the GUI draws them (only a
        /// burst risen with motion on is handed over).
        pub fn prepare_hearts(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plan: Option<super::HeartsPlan>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut hearts = self.hearts_layers.borrow_mut();
            let hearts = &mut *hearts;
            let mut changed = false;
            for shown in std::mem::take(&mut hearts.shown) {
                if shown.done <= now {
                    unsafe { self.lights.RemoveVisual(&shown.group)? };
                    changed = true;
                } else {
                    hearts.shown.push(shown);
                }
            }
            // Each burst in the air where its pane is this frame: a pane the plan doesn't name
            // has no burst the GUI still counts in the air, and is left as it is to its end
            if let Some(plan) = plan.as_ref() {
                for shown in hearts.shown.iter_mut() {
                    let Some((_, at)) = plan.panes.iter().find(|(id, _)| *id == shown.pane) else {
                        continue;
                    };
                    if shown.at == *at {
                        continue;
                    }
                    match lbterm_triggers::hearts::follow(shown.rose, *at) {
                        Some((dx, dy, top)) => unsafe {
                            shown.group.SetOffsetX2(dx)?;
                            shown.group.SetOffsetY2(dy)?;
                            shown.cut.SetLeft2(-FAR)?;
                            shown.cut.SetTop2(top)?;
                            shown.cut.SetRight2(FAR)?;
                            shown.cut.SetBottom2(FAR)?;
                        },
                        // Its pane isn't shown: cut to nothing, its hearts going on rising
                        // unseen, as they do in the GUI's drawing of them
                        None => unsafe {
                            shown.cut.SetLeft2(0.0)?;
                            shown.cut.SetTop2(0.0)?;
                            shown.cut.SetRight2(0.0)?;
                            shown.cut.SetBottom2(0.0)?;
                        },
                    }
                    shown.at = *at;
                    changed = true;
                }
            }
            let plan = plan.filter(|p| motion.enabled && !p.bursts.is_empty());
            if let Some(plan) = plan {
                match hearts.sheet.as_mut() {
                    Some(sheet) if sheet.rect == plan.sheet => {}
                    Some(sheet) => {
                        sheet.place(device, plan.sheet)?;
                        hearts.content = Some(swap_chain(&sheet.surface)?);
                    }
                    None => {
                        // Never seen itself, only in the hearts: with the card templates
                        let held = unsafe { self.device.CreateVisual()? };
                        unsafe { self.cards.AddVisual(&held, false, None::<&IDCompositionVisual>)? };
                        let sheet = Layer::new(
                            instance,
                            device,
                            held,
                            base,
                            wgpu::CompositeAlphaMode::PreMultiplied,
                            plan.sheet,
                        )?;
                        hearts.content = Some(swap_chain(&sheet.surface)?);
                        hearts.sheet = Some(sheet);
                    }
                }
                let content = hearts.content.clone().context("the hearts' sheet")?;
                for burst in &plan.bursts {
                    self.name("hearts");
                    let group = unsafe { self.device.CreateVisual()? };
                    let cut = unsafe { self.device.CreateRectangleClip()? };
                    unsafe {
                        cut.SetLeft2(-FAR)?;
                        cut.SetTop2(burst.place.top)?;
                        cut.SetRight2(FAR)?;
                        cut.SetBottom2(FAR)?;
                        group.SetClip(&cut)?;
                    }
                    let mut end = 0.0f64;
                    let half = plan.cell / 2.0;
                    for path in lbterm_triggers::hearts::paths(burst.seed) {
                        let tracks = path.tracks();
                        let heart: IDCompositionVisual3 = unsafe { self.device.CreateVisual()? }
                            .cast()
                            .context("IDCompositionVisual3 (Windows 10 or later)")?;
                        // Its pink's square of the sheet, the square at the heart's own place
                        let picture = unsafe { self.device.CreateVisual()? };
                        let square = unsafe { self.device.CreateRectangleClip()? };
                        let grow = unsafe { self.device.CreateScaleTransform()? };
                        let left = path.colour as f32 * plan.cell;
                        unsafe {
                            square.SetLeft2(left)?;
                            square.SetTop2(0.0)?;
                            square.SetRight2(left + plan.cell)?;
                            square.SetBottom2(plan.cell)?;
                            picture.SetContent(&content)?;
                            picture.SetClip(&square)?;
                            picture.SetOffsetX2(-left)?;
                            picture.SetOffsetY2(0.0)?;
                            heart.AddVisual(&picture, false, None::<&IDCompositionVisual>)?;
                            grow.SetCenterX2(half)?;
                            grow.SetCenterY2(half)?;
                            heart.SetTransform(&grow)?;
                            heart.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR)?;
                            heart.SetOpacity2(0.0)?;
                        }
                        // Its centre: left of and up from the corner, in pixels at 96 DPI
                        self.play(
                            &hearts_segments(&tracks.left, -burst.scale, burst.place.corner.0 - half),
                            |a| unsafe { heart.SetOffsetX(a) },
                            |v| unsafe { heart.SetOffsetX2(v) },
                        )?;
                        self.play(
                            &hearts_segments(&tracks.up, -burst.scale, burst.place.corner.1 - half),
                            |a| unsafe { heart.SetOffsetY(a) },
                            |v| unsafe { heart.SetOffsetY2(v) },
                        )?;
                        let by = burst.scale / plan.inner;
                        self.play(
                            &hearts_segments(&tracks.size, by, 0.0),
                            |a| unsafe { grow.SetScaleX(a) },
                            |v| unsafe { grow.SetScaleX2(v) },
                        )?;
                        self.play(
                            &hearts_segments(&tracks.size, by, 0.0),
                            |a| unsafe { grow.SetScaleY(a) },
                            |v| unsafe { grow.SetScaleY2(v) },
                        )?;
                        self.play(
                            &hearts_segments(&tracks.alpha, 1.0, 0.0),
                            |a| unsafe { heart.SetOpacity(a) },
                            |v| unsafe { heart.SetOpacity2(v) },
                        )?;
                        unsafe { group.AddVisual(&heart, false, None::<&IDCompositionVisual>)? };
                        end = end.max(path.end());
                    }
                    unsafe { self.lights.AddVisual(&group, false, None::<&IDCompositionVisual>)? };
                    hearts.shown.push(ShownHearts {
                        group,
                        cut,
                        pane: burst.pane,
                        rose: burst.place,
                        at: Some(burst.place),
                        done: now + Duration::from_secs_f64(end),
                    });
                    changed = true;
                }
            }
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: this frame's texture for the hearts' sheet, while a burst is in the air: drawn
        /// into only when what it shows changes, which is when it's new or a new size
        pub fn acquire_hearts(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let hearts = self.hearts_layers.borrow();
            match (&hearts.sheet, hearts.shown.is_empty()) {
                (Some(sheet), false) => Ok(sheet.frame(Target::Hearts, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: this frame's texture for the strip's detail card, while it shows one
        pub fn acquire_strip_card(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let strip = self.strip_card_layer.borrow();
            match (&strip.layer, strip.wanted) {
                (Some(layer), true) => Ok(layer.frame(Target::StripCard, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: whether the strip's detail card still shows anything at `now`: open, or
        /// still fading out
        pub fn strip_card_showing(&self, now: Instant) -> bool {
            let strip = self.strip_card_layer.borrow();
            strip.wanted
                && (strip.rest > 0.0 || strip.alpha.as_ref().map_or(false, |t| t.finishes() > now))
        }

        /// LBTerm: when the strip's detail card's fade is done, if one is under way
        pub fn strip_card_settles(&self) -> Option<Instant> {
            self.strip_card_layer
                .borrow()
                .alpha
                .as_ref()
                .map(|t| t.finishes())
        }

        /// LBTerm: the objectives panel's cards for this frame (render/objectives.rs, render/projects.rs):
        /// the list, the detail, the projects card, each faded in on `pane_enter` as it opens and out on
        /// `pane_exit` as it closes, from wherever it is, as the tasks card is (`prepare_faded_card`).
        /// `None` puts one away. Each is made the first time it opens; call after `prepare`.
        pub fn prepare_objectives(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            cards: [Option<(PixelRect, bool)>; 4],
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let mut layers = self.objectives_layers.borrow_mut();
            for (i, (card, want)) in layers.iter_mut().zip(cards).enumerate() {
                let name = ["objectives.list", "objectives.detail", "objectives.projects", "scroll.card"][i];
                self.prepare_faded_card(&self.objectives, card, name, instance, device, base, want, motion)?;
            }
            Ok(())
        }

        /// LBTerm: the card of what a tab's Claude session has running, for this frame
        /// (render/tasks_card.rs): its visual at `card`, fading in on `pane_enter` as it opens
        /// and out on `pane_exit` as it closes, from wherever it is, as the strip's card does.
        /// `None` puts it away. Made the first time it opens; call after `prepare`.
        pub fn prepare_tasks_card(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            card: Option<(PixelRect, bool)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let mut layer = self.tasks_card_layer.borrow_mut();
            self.prepare_faded_card(&self.tasks_card, &mut layer, "rail.tasks", instance, device, base, card, motion)
        }

        /// LBTerm: the new-tab pad's look under the pointer (`plus`: at its rectangle, lit or
        /// not), faded on `button_hover` as the strip's hover slots are, and the picker beside
        /// the rail (`picker`: at its card, open or closing), faded in on `pane_enter` and out on
        /// `pane_exit` from wherever it is (render/new_tab.rs). `None` puts one away. Each is made
        /// the first time it's wanted; call after `prepare`.
        pub fn prepare_new_tab(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            plus: Option<(PixelRect, bool)>,
            picker: Option<(PixelRect, bool)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let mut layer = self.plus_layer.borrow_mut();
            self.prepare_faded(
                &self.plus,
                &mut layer,
                "rail.plus.hover",
                instance,
                device,
                base,
                plus,
                (&motion.button_hover, &motion.button_hover),
                motion.enabled,
            )?;
            drop(layer);
            let mut layer = self.picker_layer.borrow_mut();
            self.prepare_faded(
                &self.picker,
                &mut layer,
                "newtab.picker",
                instance,
                device,
                base,
                picker,
                (&motion.enter, &motion.exit),
                motion.enabled,
            )
        }

        /// LBTerm: this frame's textures for the new-tab pad's look and the picker, while each shows
        pub fn acquire_new_tab(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let mut frames = vec![];
            for (card, target) in [(&self.plus_layer, Target::PlusHover), (&self.picker_layer, Target::Picker)] {
                let card = card.borrow();
                if let (Some(layer), true) = (&card.layer, card.wanted) {
                    frames.extend(layer.frame(target, content)?);
                }
            }
            Ok(frames)
        }

        /// LBTerm: whether the picker still shows anything at `now` (open, or still fading out),
        /// and when its fade is done, if one is under way
        pub fn picker_showing(&self, now: Instant) -> (bool, Option<Instant>) {
            let card = self.picker_layer.borrow();
            let settles = card.alpha.as_ref().map(|t| t.finishes()).filter(|t| *t > now);
            (card.wanted && (card.rest > 0.0 || settles.is_some()), settles)
        }

        /// LBTerm: every menu's card for this frame (render/menu_fade.rs), a slot each, in the
        /// order `MENU_NAMES` names them: its group faded in on `pane_enter` as it opens and out on
        /// `pane_exit` as it closes, from wherever it is (`lbterm_motion::menu::fade`), and its
        /// highlight gliding between rows on `hover` (`lbterm_motion::menu::light`). A card that's
        /// closing is its last frame: nothing in it is placed, drawn or moved again, so it takes
        /// nothing from the GUI thread while it fades. `None` puts a slot away. A slot's two
        /// surfaces (its card, and the white its highlight shows over it) are made the first time
        /// it opens and only ever grow, so a menu of another size moves them rather than
        /// making them again. Call after `prepare`.
        pub fn prepare_menus(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            visuals: [Option<MenuVisual>; MENU_SLOTS],
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut cards = self.menu_cards.borrow_mut();
            let mut changed = false;
            for (slot, (card, want)) in cards.iter_mut().zip(visuals).enumerate() {
                changed |= match want {
                    Some(want) => self.prepare_menu(instance, device, base, slot, card, want, motion, now)?,
                    None if card.wanted => {
                        card.put_away()?;
                        true
                    }
                    None => false,
                };
            }
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// One slot's card this frame: whether anything in the tree changed
        #[allow(clippy::too_many_arguments)]
        fn prepare_menu(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            slot: usize,
            card: &mut MenuCard,
            want: MenuVisual,
            motion: &MotionSettings,
            now: Instant,
        ) -> anyhow::Result<bool> {
            use lbterm_motion::menu::{self, Step};
            let mut changed = false;
            let (fade_name, glide_name, light_name, hairline_name) = MENU_NAMES[slot];
            if card.group.is_none() {
                let group = unsafe { self.device.CreateVisual()? };
                unsafe { self.menus.AddVisual(&group, false, None::<&IDCompositionVisual>)? };
                let opacity: IDCompositionVisual3 = group.cast().context("IDCompositionVisual3 (Windows 10 or later)")?;
                // Unseen until it fades in, after its first frame
                unsafe { opacity.SetOpacity2(0.0)? };
                card.group = Some(group);
                card.opacity = Some(opacity);
                card.rest = 0.0;
                card.open = false;
                changed = true;
            }
            // The page turned since the last frame: the card's rows dissolve into the new page's
            // (`menu_turn`), before the surfaces are placed for the new page
            let turned = want.page > card.pages && want.open && card.open && card.card.is_some();
            card.pages = want.page;
            if turned {
                changed |= self.menu_turn(instance, device, base, slot, card, motion)?;
            }
            // The card moved: its highlight's edges, in the card's own pixels, are placed afresh
            let mut moved = false;
            if want.open {
                let rect = match &card.card {
                    Some(layer) => PixelRect::new(want.rect.x, want.rect.y, want.rect.w.max(layer.rect.w), want.rect.h.max(layer.rect.h)),
                    None => want.rect,
                };
                let group = card.group.clone().context("the menu's group")?;
                for which in 0..2 {
                    let layer = if which == 0 { &mut card.card } else { &mut card.light };
                    match layer {
                        Some(layer) => {
                            if layer.rect != rect {
                                moved |= (layer.rect.x, layer.rect.y) != (rect.x, rect.y);
                                card.chains_moved |= layer.place(device, rect)?;
                                changed = true;
                            }
                        }
                        None => {
                            // Bottom to top as they're made: the card, its highlight
                            let visual = unsafe { self.device.CreateVisual()? };
                            unsafe { group.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                            let made = Layer::new(instance, device, visual, base, wgpu::CompositeAlphaMode::PreMultiplied, rect)?;
                            if which == 1 {
                                let clip = unsafe { self.device.CreateRectangleClip()? };
                                unsafe { made.visual.SetClip(&clip)? };
                                let lit: IDCompositionVisual3 =
                                    made.visual.cast().context("IDCompositionVisual3 (Windows 10 or later)")?;
                                unsafe { lit.SetOpacity2(0.0)? };
                                card.clip = Some(clip);
                                card.lit_opacity = Some(lit);
                                card.edges = Default::default();
                                card.edges_rest = [0.0; 4];
                                card.lit = None;
                                card.lit_rest = 0.0;
                                card.radius = -1.0;
                            }
                            *layer = Some(made);
                            card.chains_moved = true;
                            changed = true;
                        }
                    }
                }
            }

            // The card's fade, from wherever it is
            let opening = want.open && !card.open;
            let fade = menu::fade(
                menu::Now { value: card.current(now), velocity: 0.0, rest: card.rest },
                card.open,
                want.open,
                motion.enabled,
                &motion.enter,
                &motion.exit,
            );
            match fade {
                Step::Keep => {}
                Step::Set(to) => {
                    card.settle(to)?;
                    changed = true;
                }
                Step::Play(m) => {
                    let opacity = card.opacity.clone().context("the menu's group")?;
                    let to = m.to;
                    self.name(fade_name);
                    card.alpha = self.drive(
                        m,
                        OPACITY_EPSILON,
                        0.0,
                        |a| unsafe { opacity.SetOpacity(a) },
                        |v| unsafe { opacity.SetOpacity2(v) },
                    )?;
                    card.rest = to;
                    changed = true;
                }
            }
            card.open = want.open;
            card.wanted = true;
            if !want.open {
                // Fading out: its last frame, the highlight where it was
                return Ok(changed);
            }

            // The highlight, in the card's own pixels
            let (clip, lit_opacity, origin) = match (&card.clip, &card.lit_opacity, &card.light) {
                (Some(clip), Some(lit), Some(layer)) => (clip.clone(), lit.clone(), (layer.rect.x as f64, layer.rect.y as f64)),
                _ => return Ok(changed),
            };
            if card.radius != want.radius {
                for corner in Corner::ALL {
                    corner.set(&clip, want.radius as f32)?;
                }
                card.radius = want.radius;
                changed = true;
            }
            let was = menu::Light {
                edges: std::array::from_fn(|i| {
                    let (value, velocity) = at(&card.edges[i], card.edges_rest[i], now);
                    menu::Now { value, velocity, rest: card.edges_rest[i] }
                }),
                lit: {
                    let (value, velocity) = at(&card.lit, card.lit_rest, now);
                    menu::Now { value, velocity, rest: card.lit_rest }
                },
            };
            let row = want.light.map(|r| [r.x - origin.0, r.y - origin.1, r.x + r.w - origin.0, r.y + r.h - origin.1]);
            let plan = menu::light(&was, row, want.level, opening || moved, motion.enabled, &motion.hover);
            let [left, top, right, bottom] = plan.edges;
            let edges: [(Step, fn(&IDCompositionRectangleClip, &IDCompositionAnimation) -> windows58::core::Result<()>, fn(&IDCompositionRectangleClip, f32) -> windows58::core::Result<()>); 4] = [
                (left, |c, a| unsafe { c.SetLeft(a) }, |c, v| unsafe { c.SetLeft2(v) }),
                (top, |c, a| unsafe { c.SetTop(a) }, |c, v| unsafe { c.SetTop2(v) }),
                (right, |c, a| unsafe { c.SetRight(a) }, |c, v| unsafe { c.SetRight2(v) }),
                (bottom, |c, a| unsafe { c.SetBottom(a) }, |c, v| unsafe { c.SetBottom2(v) }),
            ];
            for (i, (step, animate, set)) in IntoIterator::into_iter(edges).enumerate() {
                if matches!(step, Step::Play(_)) {
                    self.name(glide_name);
                }
                changed |= self.menu_step(step, &mut card.edges[i], &mut card.edges_rest[i], PIXEL_EPSILON, |a| animate(&clip, a), |v| set(&clip, v))?;
            }
            if matches!(plan.lit, Step::Play(_)) {
                self.name(light_name);
            }
            changed |= self.menu_step(
                plan.lit,
                &mut card.lit,
                &mut card.lit_rest,
                OPACITY_EPSILON,
                |a| unsafe { lit_opacity.SetOpacity(a) },
                |v| unsafe { lit_opacity.SetOpacity2(v) },
            )?;

            // A key with nowhere to go since the last frame: the card's edge flares once. A card
            // just opened has none to answer (the count is the slot's, kept across its menus)
            let bumped = want.bumps > card.bumps && !opening;
            card.bumps = want.bumps;
            if bumped || card.lamp.is_some() {
                changed |= self.menu_lamp(card, want.ring, bumped, hairline_name, motion, now)?;
            }
            Ok(changed)
        }

        /// A menu card's lamp this frame (`MenuLamp`): made the first time a key has nowhere to
        /// go, placed again only when the hairline, the slot's surfaces or their swap chains
        /// changed, and flared when `bumped`. Whether the tree changed
        fn menu_lamp(
            &self,
            card: &mut MenuCard,
            ring: MenuRing,
            bumped: bool,
            name: &'static str,
            motion: &MotionSettings,
            now: Instant,
        ) -> anyhow::Result<bool> {
            let (Some(group), Some(under), Some(white)) = (card.group.clone(), card.card.as_ref(), card.light.as_ref()) else {
                return Ok(false);
            };
            let mut changed = false;
            if card.lamp.is_none() {
                // Made out of the tree: it goes in when a flare plays (below)
                let lamp = unsafe { self.device.CreateVisual()? };
                let opacity: IDCompositionVisual3 = lamp.cast().context("IDCompositionVisual3 (Windows 10 or later)")?;
                unsafe { opacity.SetOpacity2(0.0)? };
                let made = |parent: &IDCompositionVisual2| -> anyhow::Result<(IDCompositionVisual2, IDCompositionRectangleClip)> {
                    let visual = unsafe { self.device.CreateVisual()? };
                    let clip = unsafe { self.device.CreateRectangleClip()? };
                    unsafe {
                        visual.SetClip(&clip)?;
                        parent.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                    }
                    Ok((visual, clip))
                };
                // Bottom to top: the white, then the card over it
                let white_part = made(&lamp)?;
                let fill_part = made(&lamp)?;
                // The light round the card, white: the card's surface again just under the card
                // (`false` with the card for reference), every pixel of it white at its alpha
                // scaled, in the matrix's rows (in) and columns (out): the colours' offsets 1,
                // nothing of them from the input, the alpha from the alpha alone. The default
                // alpha mode works on straight colour and premultiplies after, so a pixel of the
                // light at alpha a comes out white at k x a, clamped at a whole white
                let halo: IDCompositionVisual3 = unsafe { self.device.CreateVisual()? }
                    .cast()
                    .context("IDCompositionVisual3 (Windows 10 or later)")?;
                let effects: IDCompositionDevice3 = self.device.cast().context("IDCompositionDevice3")?;
                let whiten = unsafe { effects.CreateColorMatrixEffect()? };
                let k = lbterm_panel::palette::menu_bump_light_scale() as f32;
                unsafe {
                    for row in 0..4 {
                        for column in 0..4 {
                            let value = if row == 3 && column == 3 { k } else { 0.0 };
                            whiten.SetMatrixElement2(row, column, value)?;
                        }
                    }
                    for column in 0..3 {
                        whiten.SetMatrixElement2(4, column, 1.0)?;
                    }
                    whiten.SetMatrixElement2(4, 3, 0.0)?;
                    whiten.SetClampOutput(BOOL(1))?;
                    halo.SetEffect(&whiten)?;
                    halo.SetOpacity2(0.0)?;
                }
                card.lamp = Some(MenuLamp {
                    opacity,
                    white: white_part,
                    fill: fill_part,
                    halo,
                    _whiten: whiten,
                    placed: None,
                    lit: None,
                    attached: false,
                });
                changed = true;
            }
            let lamp = card.lamp.as_mut().context("the menu's lamp")?;
            // A flare played out leaves the tree at the first frame after its end, so at rest
            // DWM composes nothing of it (no visual at opacity 0, no colour matrix): it costs
            // only while it plays. No frame is asked for that: whatever next paints the window
            // takes it out before DWM composes for it
            if lamp.attached && lamp.lit.as_ref().is_some_and(|(chain, start)| now.saturating_duration_since(*start).as_secs_f64() >= chain.end()) {
                unsafe {
                    group.RemoveVisual(&lamp.opacity)?;
                    group.RemoveVisual(&lamp.halo)?;
                }
                lamp.attached = false;
                lamp.lit = None;
                changed = true;
            }
            let origin = (under.rect.x, under.rect.y);
            // The swap chains are looked up (as_hal, then a QueryInterface each) only when a
            // surface may stand on a new one: on a frame where nothing changed, none is (PITFALLS 62)
            let chains = if card.chains_moved || lamp.placed.is_none() {
                Some((swap_chain(&white.surface)?, swap_chain(&under.surface)?))
            } else {
                None
            };
            card.chains_moved = false;
            if let Some((white_chain, card_chain)) = &chains {
                for ((visual, _), content) in [(&lamp.white, white_chain), (&lamp.fill, card_chain)] {
                    unsafe { visual.SetContent(content)? };
                }
                unsafe { lamp.halo.SetContent(card_chain)? };
                changed = true;
            }
            if lamp.placed != Some((ring, origin)) {
                let (x, y) = (origin.0 as f64, origin.1 as f64);
                for ((visual, clip), r, radius) in [
                    (&lamp.white, ring.outer, ring.outer_radius),
                    (&lamp.fill, ring.inner, ring.inner_radius),
                ] {
                    unsafe {
                        visual.SetOffsetX2(x as f32)?;
                        visual.SetOffsetY2(y as f32)?;
                        clip.SetLeft2((r.x - x) as f32)?;
                        clip.SetTop2((r.y - y) as f32)?;
                        clip.SetRight2((r.x + r.w - x) as f32)?;
                        clip.SetBottom2((r.y + r.h - y) as f32)?;
                    }
                    for corner in Corner::ALL {
                        corner.set(clip, radius as f32)?;
                    }
                }
                unsafe {
                    lamp.halo.SetOffsetX2(x as f32)?;
                    lamp.halo.SetOffsetY2(y as f32)?;
                }
                lamp.placed = Some((ring, origin));
                changed = true;
            }
            if bumped {
                let (value, velocity) = match &lamp.lit {
                    Some((chain, start)) => {
                        let t = now.saturating_duration_since(*start).as_secs_f64();
                        (chain.value(t), chain.velocity(t))
                    }
                    None => (0.0, 0.0),
                };
                let was = lbterm_motion::menu::Now { value, velocity, rest: 0.0 };
                let peak = lbterm_panel::palette::MENU_BUMP_HAIRLINE;
                if let Some(chain) = lbterm_motion::menu::bump(was, motion.enabled, peak, &motion.arrive_in, &motion.panel_glow) {
                    self.name(name);
                    // Into the tree for the flare: the hairline's group just over the card,
                    // under its highlight; the light just under the card. Placed relative to the
                    // card's surface as it is now, the front one after a page turn
                    if !lamp.attached {
                        unsafe {
                            group.AddVisual(&lamp.opacity, true, &under.visual)?;
                            group.AddVisual(&lamp.halo, false, &under.visual)?;
                        }
                        lamp.attached = true;
                    }
                    // One animation on both: the hairline's group and the light under the card
                    let (opacity, halo) = (&lamp.opacity, &lamp.halo);
                    self.play(
                        &chain.segments(STEP, OPACITY_EPSILON),
                        |a| unsafe {
                            opacity.SetOpacity(a)?;
                            halo.SetOpacity(a)
                        },
                        |v| unsafe {
                            opacity.SetOpacity2(v)?;
                            halo.SetOpacity2(v)
                        },
                    )?;
                    lamp.lit = Some((chain, now));
                    changed = true;
                }
            }
            Ok(changed)
        }

        /// A menu card's page turned (the tab menu's Wait on… and back, lbterm/NEW-TAB.md, section
        /// 10): the card's two surfaces swap round, so the new page is drawn into the other one,
        /// and the one holding the old page's last frame goes over it, where it was, and fades out
        /// on `swap` (`MENU_SWAP_NAMES`). Nothing is drawn for the old page: its swap chain keeps
        /// what it last showed, as a closing card's does (PITFALLS 147). The other surface is made
        /// the first time the slot turns a page, the card's size, and is placed again only once
        /// it's the front and drawn into. With motion off the new page is just there. Whether the
        /// tree changed
        fn menu_turn(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            slot: usize,
            card: &mut MenuCard,
            motion: &MotionSettings,
        ) -> anyhow::Result<bool> {
            if !motion.enabled {
                return Ok(false);
            }
            let group = card.group.clone().context("the menu's group")?;
            if card.ghost.is_none() {
                let under = card.card.as_ref().context("the menu's card")?;
                let rect = under.rect;
                let visual = unsafe { self.device.CreateVisual()? };
                // Just over the card, under the lamp and the highlight (`false` with no reference
                // would put it over them all)
                unsafe { group.AddVisual(&visual, true, &under.visual)? };
                card.ghost = Some(Layer::new(instance, device, visual, base, wgpu::CompositeAlphaMode::PreMultiplied, rect)?);
            }
            // The page before is the ghost now, and the surface the new page draws into the card
            std::mem::swap(&mut card.card, &mut card.ghost);
            card.chains_moved = true;
            let (front, ghost) = match (&card.card, &card.ghost) {
                (Some(front), Some(ghost)) => (front, ghost),
                _ => return Ok(false),
            };
            // The old page over the new, just above it and under the lamp and the highlight
            unsafe {
                group.RemoveVisual(&ghost.visual)?;
                group.AddVisual(&ghost.visual, true, &front.visual)?;
            }
            let front_opacity: IDCompositionVisual3 = front.visual.cast().context("IDCompositionVisual3 (Windows 10 or later)")?;
            let ghost_opacity: IDCompositionVisual3 = ghost.visual.cast().context("IDCompositionVisual3 (Windows 10 or later)")?;
            // The new page shows whole under it at once: it was the ghost of a turn before, faded out
            unsafe { front_opacity.SetOpacity2(1.0)? };
            self.name(MENU_SWAP_NAMES[slot]);
            card.ghost_alpha = self.drive(
                Motion::new(1.0, 0.0, 0.0, motion.swap.clone()),
                OPACITY_EPSILON,
                0.0,
                |a| unsafe { ghost_opacity.SetOpacity(a) },
                |v| unsafe { ghost_opacity.SetOpacity2(v) },
            )?;
            Ok(true)
        }

        /// One of a menu's numbers this frame (`lbterm_motion::menu::Step`): whether it changed
        fn menu_step(
            &self,
            step: lbterm_motion::menu::Step,
            tween: &mut Option<Tween>,
            rest: &mut f64,
            epsilon: f64,
            animate: impl FnOnce(&IDCompositionAnimation) -> windows58::core::Result<()>,
            set: impl FnOnce(f32) -> windows58::core::Result<()>,
        ) -> anyhow::Result<bool> {
            use lbterm_motion::menu::Step;
            match step {
                Step::Keep => Ok(false),
                Step::Set(to) => {
                    set(to as f32)?;
                    *tween = None;
                    *rest = to;
                    Ok(true)
                }
                Step::Play(m) => {
                    let to = m.to;
                    *tween = self.drive(m, epsilon, 0.0, animate, set)?;
                    *rest = to;
                    Ok(true)
                }
            }
        }

        /// LBTerm: this frame's textures for the menus' cards, while each is open: one fading out
        /// is its last frame, and nothing is drawn into it again
        pub fn acquire_menus(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let cards = self.menu_cards.borrow();
            let mut frames = vec![];
            for (slot, card) in cards.iter().enumerate() {
                if !(card.wanted && card.open) {
                    continue;
                }
                for (layer, target) in [
                    (&card.card, Target::MenuCard(slot)),
                    (&card.light, Target::MenuLight(slot)),
                ] {
                    if let Some(layer) = layer {
                        frames.extend(layer.frame(target, content)?);
                    }
                }
            }
            Ok(frames)
        }

        /// LBTerm: whether menu slot `slot`'s card still shows anything at `now`: open, or still
        /// fading out
        pub fn menu_showing(&self, slot: usize, now: Instant) -> bool {
            let cards = self.menu_cards.borrow();
            cards.get(slot).map_or(false, |c| {
                c.wanted && (c.rest > 0.0 || c.alpha.as_ref().map_or(false, |t| t.finishes() > now))
            })
        }

        /// LBTerm: when the last menu card fading out has gone, while one is: the tab panel holds
        /// still till then (render/tab_panel.rs, `Hover::hold`)
        pub fn menus_fading_until(&self, now: Instant) -> Option<Instant> {
            let cards = self.menu_cards.borrow();
            cards
                .iter()
                .filter(|c| c.wanted && c.rest == 0.0)
                .filter_map(|c| c.alpha.as_ref().map(|t| t.finishes()))
                .filter(|end| *end > now)
                .max()
        }

        /// LBTerm: every pane dimmed from the next `prepare`, the one with the keyboard too, while
        /// something of LBTerm's own has the keys (render/new_tab.rs)
        pub fn dim_all(&self, all: bool) {
            self.dim_all.set(all);
        }

        /// One card's visual, a child of `parent`, at `want`'s rectangle: faded in on
        /// `pane_enter` when it's opening and out on `pane_exit` when it's closing, from wherever
        /// it is, its fade named `name` for the state file. `None` puts it away. Made the first
        /// time it's wanted, unseen until its first frame has faded it in.
        #[allow(clippy::too_many_arguments)]
        fn prepare_faded_card(
            &self,
            parent: &IDCompositionVisual2,
            card: &mut StripCard,
            name: &'static str,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            want: Option<(PixelRect, bool)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            self.prepare_faded(parent, card, name, instance, device, base, want, (&motion.enter, &motion.exit), motion.enabled)
        }

        /// `prepare_faded_card` on curves of its own: `fade.0` in, `fade.1` out
        #[allow(clippy::too_many_arguments)]
        fn prepare_faded(
            &self,
            parent: &IDCompositionVisual2,
            card: &mut StripCard,
            name: &'static str,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            want: Option<(PixelRect, bool)>,
            fade: (&Curve, &Curve),
            enabled: bool,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let (rect, open) = match want {
                Some(want) => want,
                None => {
                    if card.wanted {
                        card.settle(0.0)?;
                        card.wanted = false;
                        self.layers.borrow_mut().changed = true;
                    }
                    card.open = false;
                    return Ok(());
                }
            };
            let mut changed = false;
            match card.layer.as_mut() {
                Some(layer) => {
                    if layer.rect != rect {
                        layer.place(device, rect)?;
                        changed = true;
                    }
                }
                None => {
                    let visual = unsafe { self.device.CreateVisual()? };
                    unsafe { parent.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                    let layer = Layer::new(
                        instance,
                        device,
                        visual,
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        rect,
                    )?;
                    let opacity: IDCompositionVisual3 = layer
                        .visual
                        .cast()
                        .context("IDCompositionVisual3 (Windows 10 or later)")?;
                    // Unseen until it fades in, after its first frame
                    unsafe { opacity.SetOpacity2(0.0)? };
                    card.layer = Some(layer);
                    card.opacity = Some(opacity);
                    card.rest = 0.0;
                    card.open = false;
                    changed = true;
                }
            }
            if open != card.open {
                let to = if open { 1.0 } else { 0.0 };
                if enabled {
                    let from = card.current(now);
                    let curve = if open { fade.0.clone() } else { fade.1.clone() };
                    let opacity = card.opacity.clone().context("the card's visual")?;
                    self.name(name);
                    card.alpha = self.drive(
                        Motion::new(from, to, 0.0, curve),
                        OPACITY_EPSILON,
                        0.0,
                        |a| unsafe { opacity.SetOpacity(a) },
                        |v| unsafe { opacity.SetOpacity2(v) },
                    )?;
                    card.rest = to;
                } else {
                    card.settle(to)?;
                }
                card.open = open;
                changed = true;
            }
            card.wanted = true;
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: this frame's textures for the objectives panel's cards, while they show
        pub fn acquire_objectives(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let layers = self.objectives_layers.borrow();
            let mut frames = vec![];
            for (i, card) in layers.iter().enumerate() {
                if let (Some(layer), true) = (&card.layer, card.wanted) {
                    frames.extend(layer.frame(Target::Objectives(i), content)?);
                }
            }
            Ok(frames)
        }

        /// LBTerm: whether each of the objectives panel's cards still shows anything at `now`:
        /// open, or still fading out; and when the last fade under way is done
        pub fn objectives_showing(&self, now: Instant) -> ([bool; 4], Option<Instant>) {
            let layers = self.objectives_layers.borrow();
            let showing = |c: &StripCard| {
                c.wanted && (c.rest > 0.0 || c.alpha.as_ref().map_or(false, |t| t.finishes() > now))
            };
            let settles = layers
                .iter()
                .filter_map(|c| c.alpha.as_ref().map(|t| t.finishes()).filter(|t| *t > now))
                .max();
            (std::array::from_fn(|i| showing(&layers[i])), settles)
        }

        /// LBTerm: this frame's texture for the card of what a tab is running, while it shows
        pub fn acquire_tasks_card(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let card = self.tasks_card_layer.borrow();
            match (&card.layer, card.wanted) {
                (Some(layer), true) => Ok(layer.frame(Target::TasksCard, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: whether the card of what a tab is running still shows anything at `now`:
        /// open, or still fading out; and when its fade is done, if one is under way
        pub fn tasks_card_showing(&self, now: Instant) -> (bool, Option<Instant>) {
            let card = self.tasks_card_layer.borrow();
            let settles = card.alpha.as_ref().map(|t| t.finishes()).filter(|t| *t > now);
            (card.wanted && (card.rest > 0.0 || settles.is_some()), settles)
        }

        /// LBTerm: the container a brief light over `over` goes in
        fn lights_in(&self, over: Over) -> &IDCompositionVisual2 {
            match over {
                Over::Panes => &self.lights,
                Over::Panel => &self.panel_lights,
                Over::Menus => &self.menu_lights,
            }
        }

        /// LBTerm: start this frame's `lights` (render/lights.rs), and put away the ones that
        /// are done. They show a white swatch as wide as the window, made the first time and
        /// cleared to white only when it's new or a new size. With motion off there are none.
        pub fn prepare_lights(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            window: PixelRect,
            lights: Vec<Light>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut layers = self.light_layers.borrow_mut();
            let layers = &mut *layers;
            let mut changed = false;
            for (lit, done) in std::mem::take(&mut layers.shown) {
                if done <= now {
                    unsafe { self.lights_in(lit.over).RemoveVisual(&lit.visual)? };
                    layers.spare.push(lit);
                    changed = true;
                } else {
                    layers.shown.push((lit, done));
                }
            }
            if lights.is_empty() || !motion.enabled {
                if changed {
                    self.layers.borrow_mut().changed = true;
                }
                return Ok(());
            }

            // The swatch: as wide as the window, as tall as the biggest light
            let tallest = lights
                .iter()
                .map(|light| match light {
                    Light::Press { reach, .. } => (2.0 * reach).ceil() as u32,
                    Light::Text { rects, .. } => rects.iter().map(|r| r.h).max().unwrap_or(0),
                    Light::Pad { rect, .. } | Light::Row { rect, .. } => rect.h,
                    Light::Send { from, .. } => from.h,
                })
                .max()
                .unwrap_or(0);
            let (w, h) = (window.w.max(1), tallest.max(64));
            let fits = |s: &Layer| s.rect.w >= w && s.rect.h >= h;
            match layers.swatch.as_mut() {
                Some(swatch) if fits(swatch) => {}
                Some(swatch) => {
                    let rect = PixelRect::new(0, 0, w.max(swatch.rect.w), h.max(swatch.rect.h));
                    swatch.place(device, rect)?;
                    layers.content = Some(swap_chain(&swatch.surface)?);
                    layers.draw = true;
                }
                None => {
                    let visual = unsafe { self.device.CreateVisual()? };
                    // Never seen itself, only in the lights: with the card templates, and far off
                    unsafe { self.cards.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                    let swatch = Layer::new(
                        instance,
                        device,
                        visual,
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        PixelRect::new(0, 0, w, h),
                    )?;
                    unsafe { swatch.visual.SetOffsetX2(-FAR)? };
                    layers.content = Some(swap_chain(&swatch.surface)?);
                    layers.swatch = Some(swatch);
                    layers.draw = true;
                }
            }
            let content = layers.content.clone().context("the lights' swatch")?;
            let take = |spare: &mut Vec<Lit>| -> anyhow::Result<Lit> {
                match spare.pop() {
                    Some(lit) => Ok(lit),
                    None => Lit::new(self),
                }
            };

            for light in lights {
                match light {
                    Light::Press { x, y, reach, start, within } => {
                        self.name("light.press");
                        let mut lit = take(&mut layers.spare)?;
                        lit.over = if within.is_some() { Over::Panel } else { Over::Panes };
                        let clip = &lit.clip;
                        unsafe {
                            lit.visual.SetContent(&content)?;
                            lit.visual.SetOffsetX2((x - reach) as f32)?;
                            lit.visual.SetOffsetY2((y - reach) as f32)?;
                        }
                        // A circle about (reach, reach) in the visual, from `start` across to
                        // `reach`: every edge and corner rides on the one curve
                        let grow = |from: f64, to: f64| Motion::new(from, to, 0.0, motion.press.clone());
                        let mut end = 0.0f64;
                        let mut drive = |m: Motion,
                                         animate: &dyn Fn(&IDCompositionAnimation) -> windows58::core::Result<()>,
                                         set: &dyn Fn(f32) -> windows58::core::Result<()>|
                         -> anyhow::Result<()> {
                            end = end.max(m.end(PIXEL_EPSILON));
                            self.drive(m, PIXEL_EPSILON, 0.0, |a| animate(a), |v| set(v))?;
                            Ok(())
                        };
                        // Held inside a pad, every edge stops at the pad's, its corners at the pad's
                        let (x0, y0) = (x - reach, y - reach);
                        let (left, top, right, bottom, round) = match within {
                            Some((r, radius)) => (
                                (r.x as f64 - x0).clamp(0.0, reach - start),
                                (r.y as f64 - y0).clamp(0.0, reach - start),
                                ((r.x + r.w as i32) as f64 - x0).clamp(reach + start, 2.0 * reach),
                                ((r.y + r.h as i32) as f64 - y0).clamp(reach + start, 2.0 * reach),
                                radius.min(reach).max(start),
                            ),
                            None => (0.0, 0.0, 2.0 * reach, 2.0 * reach, reach),
                        };
                        drive(grow(reach - start, left), &|a| unsafe { clip.SetLeft(a) }, &|v| unsafe { clip.SetLeft2(v) })?;
                        drive(grow(reach - start, top), &|a| unsafe { clip.SetTop(a) }, &|v| unsafe { clip.SetTop2(v) })?;
                        drive(grow(reach + start, right), &|a| unsafe { clip.SetRight(a) }, &|v| unsafe { clip.SetRight2(v) })?;
                        drive(grow(reach + start, bottom), &|a| unsafe { clip.SetBottom(a) }, &|v| unsafe { clip.SetBottom2(v) })?;
                        for corner in Corner::ALL {
                            drive(grow(start, round), &|a| corner.animate(clip, a), &|v| corner.set(clip, v))?;
                        }
                        let fade = Motion::new(lbterm_panel::palette::PRESS, 0.0, 0.0, motion.press.clone());
                        end = end.max(fade.end(OPACITY_EPSILON));
                        let opacity = &lit.visual;
                        self.drive(
                            fade,
                            OPACITY_EPSILON,
                            0.0,
                            |a| unsafe { opacity.SetOpacity(a) },
                            |v| unsafe { opacity.SetOpacity2(v) },
                        )?;
                        unsafe { self.lights_in(lit.over).AddVisual(&lit.visual, false, None::<&IDCompositionVisual>)? };
                        layers.shown.push((lit, now + Duration::from_secs_f64(end)));
                    }
                    Light::Text { rects, radius } => {
                        self.name("light.copy");
                        for rect in rects {
                            let mut lit = take(&mut layers.spare)?;
                            lit.over = Over::Panes;
                            let clip = &lit.clip;
                            unsafe {
                                lit.visual.SetContent(&content)?;
                                lit.visual.SetOffsetX2(0.0)?;
                                lit.visual.SetOffsetY2(rect.y as f32)?;
                                clip.SetLeft2(rect.x as f32)?;
                                clip.SetTop2(0.0)?;
                                clip.SetRight2((rect.x + rect.w as i32) as f32)?;
                                clip.SetBottom2(rect.h as f32)?;
                            }
                            for corner in Corner::ALL {
                                corner.set(clip, radius as f32)?;
                            }
                            let chain = Chain::pulse(
                                0.0,
                                0.0,
                                lbterm_panel::palette::COPY_FLASH,
                                0.0,
                                motion.copy_in.clone(),
                                motion.copy_out.clone(),
                                OPACITY_EPSILON,
                            );
                            let opacity = &lit.visual;
                            self.play(
                                &chain.segments(STEP, OPACITY_EPSILON),
                                |a| unsafe { opacity.SetOpacity(a) },
                                |v| unsafe { opacity.SetOpacity2(v) },
                            )?;
                            unsafe { self.lights.AddVisual(&lit.visual, false, None::<&IDCompositionVisual>)? };
                            layers.shown.push((lit, now + Duration::from_secs_f64(chain.end())));
                        }
                    }
                    Light::Pad { rect, radius } => {
                        self.name("light.glint");
                        let mut lit = take(&mut layers.spare)?;
                        lit.over = Over::Panel;
                        let clip = &lit.clip;
                        unsafe {
                            lit.visual.SetContent(&content)?;
                            lit.visual.SetOffsetX2(rect.x as f32)?;
                            lit.visual.SetOffsetY2(rect.y as f32)?;
                            clip.SetLeft2(0.0)?;
                            clip.SetTop2(0.0)?;
                            clip.SetRight2(rect.w as f32)?;
                            clip.SetBottom2(rect.h as f32)?;
                        }
                        for corner in Corner::ALL {
                            corner.set(clip, radius as f32)?;
                        }
                        let chain = Chain::pulse(
                            0.0,
                            0.0,
                            lbterm_panel::palette::PRESS,
                            0.0,
                            motion.copy_in.clone(),
                            motion.copy_out.clone(),
                            OPACITY_EPSILON,
                        );
                        let opacity = &lit.visual;
                        self.play(
                            &chain.segments(STEP, OPACITY_EPSILON),
                            |a| unsafe { opacity.SetOpacity(a) },
                            |v| unsafe { opacity.SetOpacity2(v) },
                        )?;
                        unsafe { self.panel_lights.AddVisual(&lit.visual, false, None::<&IDCompositionVisual>)? };
                        layers.shown.push((lit, now + Duration::from_secs_f64(chain.end())));
                    }
                    Light::Row { rect, radius, slot } => {
                        let Some(chain) = lbterm_motion::menu::flash(
                            motion.enabled,
                            lbterm_panel::palette::PRESS,
                            &motion.copy_in,
                            &motion.copy_out,
                        ) else {
                            continue;
                        };
                        self.name(MENU_FLASH_NAMES[slot.min(MENU_SLOTS - 1)]);
                        let mut lit = take(&mut layers.spare)?;
                        lit.over = Over::Menus;
                        let clip = &lit.clip;
                        unsafe {
                            lit.visual.SetContent(&content)?;
                            lit.visual.SetOffsetX2(rect.x as f32)?;
                            lit.visual.SetOffsetY2(rect.y as f32)?;
                            clip.SetLeft2(0.0)?;
                            clip.SetTop2(0.0)?;
                            clip.SetRight2(rect.w as f32)?;
                            clip.SetBottom2(rect.h as f32)?;
                        }
                        for corner in Corner::ALL {
                            corner.set(clip, radius as f32)?;
                        }
                        let opacity = &lit.visual;
                        self.play(
                            &chain.segments(STEP, OPACITY_EPSILON),
                            |a| unsafe { opacity.SetOpacity(a) },
                            |v| unsafe { opacity.SetOpacity2(v) },
                        )?;
                        unsafe { self.menu_lights.AddVisual(&lit.visual, false, None::<&IDCompositionVisual>)? };
                        layers.shown.push((lit, now + Duration::from_secs_f64(chain.end())));
                    }
                    Light::Send { from, to, radius } => {
                        let Some(send) = lbterm_motion::send::send(
                            motion.enabled,
                            (from.x as f64, from.y as f64),
                            (to.x as f64, to.y as f64),
                            lbterm_panel::palette::PRESS,
                            &motion.send,
                            &motion.copy_in,
                            &motion.copy_out,
                            STEP,
                            PIXEL_EPSILON,
                        ) else {
                            continue;
                        };
                        self.name("light.send");
                        let mut lit = take(&mut layers.spare)?;
                        lit.over = Over::Panel;
                        let clip = &lit.clip;
                        // The glint's size all the way: every pad is the same size
                        unsafe {
                            lit.visual.SetContent(&content)?;
                            clip.SetLeft2(0.0)?;
                            clip.SetTop2(0.0)?;
                            clip.SetRight2(from.w as f32)?;
                            clip.SetBottom2(from.h as f32)?;
                        }
                        for corner in Corner::ALL {
                            corner.set(clip, radius as f32)?;
                        }
                        // Its corner carried up the rail and its light, all given to DWM now
                        let visual = &lit.visual;
                        self.drive(
                            send.x.clone(),
                            PIXEL_EPSILON,
                            0.0,
                            |a| unsafe { visual.SetOffsetX(a) },
                            |v| unsafe { visual.SetOffsetX2(v) },
                        )?;
                        self.drive(
                            send.y.clone(),
                            PIXEL_EPSILON,
                            0.0,
                            |a| unsafe { visual.SetOffsetY(a) },
                            |v| unsafe { visual.SetOffsetY2(v) },
                        )?;
                        self.play(
                            &send.opacity,
                            |a| unsafe { visual.SetOpacity(a) },
                            |v| unsafe { visual.SetOpacity2(v) },
                        )?;
                        unsafe { self.panel_lights.AddVisual(&lit.visual, false, None::<&IDCompositionVisual>)? };
                        layers.shown.push((lit, now + Duration::from_secs_f64(send.end())));
                    }
                }
            }
            self.layers.borrow_mut().changed = true;
            Ok(())
        }

        /// LBTerm: the lights' swatch, in a frame only when it's new or a new size: it's
        /// cleared to white (render/composited.rs)
        pub fn acquire_lights(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let mut layers = self.light_layers.borrow_mut();
            if !layers.draw {
                return Ok(vec![]);
            }
            layers.draw = false;
            match &layers.swatch {
                Some(swatch) => Ok(swatch.frame(Target::Swatch, content)?.into_iter().collect()),
                None => Ok(vec![]),
            }
        }

        /// LBTerm: the readout of a pane's size (render/lights.rs): its visual at `readout`,
        /// there at once while it shows and fading out on `pane_exit` after, from wherever it
        /// is. Nothing fades while the window is being resized, so it's there at once then too.
        /// `None` puts it away. Made the first time it shows; call after `prepare`.
        pub fn prepare_readout(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            readout: Option<(PixelRect, bool)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut card = self.readout_layer.borrow_mut();
            let (rect, showing) = match readout {
                Some(readout) => readout,
                None => {
                    if card.wanted {
                        card.settle(0.0)?;
                        card.wanted = false;
                        self.layers.borrow_mut().changed = true;
                    }
                    card.open = false;
                    return Ok(());
                }
            };
            let mut changed = false;
            match card.layer.as_mut() {
                Some(layer) => {
                    if layer.rect != rect {
                        layer.place(device, rect)?;
                        changed = true;
                    }
                }
                None => {
                    let visual = unsafe { self.device.CreateVisual()? };
                    unsafe { self.readout.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                    let layer = Layer::new(
                        instance,
                        device,
                        visual,
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        rect,
                    )?;
                    let opacity: IDCompositionVisual3 = layer
                        .visual
                        .cast()
                        .context("IDCompositionVisual3 (Windows 10 or later)")?;
                    // Unseen until it shows, after its first frame
                    unsafe { opacity.SetOpacity2(0.0)? };
                    card.layer = Some(layer);
                    card.opacity = Some(opacity);
                    card.rest = 0.0;
                    card.open = false;
                    changed = true;
                }
            }
            if showing != card.open {
                if showing || !motion.enabled {
                    card.settle(if showing { 1.0 } else { 0.0 })?;
                } else {
                    let from = card.current(now);
                    let opacity = card.opacity.clone().context("the readout's visual")?;
                    self.name("readout");
                    card.alpha = self.drive(
                        Motion::new(from, 0.0, 0.0, motion.exit.clone()),
                        OPACITY_EPSILON,
                        0.0,
                        |a| unsafe { opacity.SetOpacity(a) },
                        |v| unsafe { opacity.SetOpacity2(v) },
                    )?;
                    card.rest = 0.0;
                }
                card.open = showing;
                changed = true;
            }
            card.wanted = true;
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: this frame's texture for the readout, while it shows or fades
        pub fn acquire_readout(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let card = self.readout_layer.borrow();
            match (&card.layer, card.wanted) {
                (Some(layer), true) => Ok(layer.frame(Target::Readout, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: whether the readout still shows anything at `now`: showing, or fading out;
        /// and when its fade is done, if one is under way
        pub fn readout_fading(&self, now: Instant) -> (bool, Option<Instant>) {
            let card = self.readout_layer.borrow();
            let fading = card.alpha.as_ref().filter(|t| t.finishes() > now);
            (
                card.wanted && (card.rest > 0.0 || fading.is_some()),
                fading.map(|t| t.finishes()),
            )
        }

        /// LBTerm: the strip as it was over its new words (render/strip_swap.rs): `swap`'s
        /// area, the rows of it that changed, which swap it is, and whether it goes back to the
        /// words the one before it still fades out. Each swap goes into the strip's words in
        /// flight (`SwapStack`), at the bottom, there at once and going out by its share of the
        /// strip on `swap`, as the panes go by theirs: a tab's words when its panes dissolved this
        /// frame, and words that changed in the tab on screen otherwise. A tab come back takes its
        /// words out, and so does a swap going back to the words the one before still fades;
        /// every one whose opacity has to change to keep its share plays it again from now.
        /// Motion off puts them all away, and so does a window being resized. `None` (the
        /// newest's words done, render/strip_swap.rs) leaves the rest to go on until they're
        /// done: the newest needn't end last (a spring for `swap` ends sooner from less, and a
        /// tab left showing nothing goes in with no words at all), and until 2026-10-31 `None`
        /// put away words still showing, at once. Call after `prepare`.
        ///
        /// Each one's swapchain holds the whole strip, `area`, which stays the same from one swap
        /// to the next, and a clip shows only the rows that changed: a swapchain made the size of
        /// each change's rows was made again for nearly every change (the InMail button with its
        /// count and without, one row or two), and remaking one waits for the GPU to finish its
        /// work, 4 to 25 ms of the GUI thread each time (lbterm/INBOX.md, "What a change costs").
        pub fn prepare_swap(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            swap: Option<(PixelRect, PixelRect, u64, bool)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let t = self.clock(now);
            let (resizing, dissolved, returned) = {
                let layers = self.layers.borrow();
                (layers.resizing, layers.dissolved.clone(), layers.returned.clone())
            };
            let mut stack = self.swaps.borrow_mut();
            let stack = &mut *stack;
            // Done ones out; all of them while the window resizes or with motion off
            if motion.enabled && !resizing {
                stack.flight.put_away_done(t);
            } else {
                stack.flight.clear();
            }
            let mut changed = self.sync_swaps(stack)?;
            match swap.filter(|_| motion.enabled) {
                Some((area, rows, generation, back)) if generation != stack.shown => {
                    stack.shown = generation;
                    let words = if dissolved.is_empty() {
                        Words::Swap(generation)
                    } else {
                        Words::Tab(dissolved.clone())
                    };
                    // Back to words in flight: a tab come back's, or the ones the swap before
                    // still fades
                    let back_to = stack
                        .flight
                        .leaving()
                        .iter()
                        .map(|l| &l.key)
                        .find(|k| matches!(k, Words::Tab(panes) if panes.iter().any(|p| returned.contains(p))))
                        .or_else(|| back.then(|| stack.flight.leaving().last().map(|l| &l.key)).flatten())
                        .cloned();
                    let switched = stack.flight.switch(
                        Some(words.clone()),
                        |k| Some(k) == back_to.as_ref(),
                        t,
                        &motion.swap,
                        lbterm_motion::flight::WORDS_DEPTH,
                    );
                    log::debug!(
                        "strip: swap {generation} ({words:?}) in from {:?}, {} in flight{}{}",
                        switched.left,
                        stack.flight.len(),
                        match &switched.back {
                            Some((k, s)) => format!(", {k:?} back from {s:.2}"),
                            None => String::new(),
                        },
                        if switched.dropped.is_empty() {
                            String::new()
                        } else {
                            format!(", past its depth {:?}", switched.dropped)
                        }
                    );
                    self.sync_swaps(stack)?;
                    if switched.left.is_some() {
                        let (layer, opacity, clip) = match stack.spare.pop() {
                            Some((mut layer, opacity, clip)) => {
                                layer.place(device, area)?;
                                (layer, opacity, clip)
                            }
                            None => {
                                let visual = unsafe { self.device.CreateVisual()? };
                                let layer = Layer::new(
                                    instance,
                                    device,
                                    visual,
                                    base,
                                    wgpu::CompositeAlphaMode::PreMultiplied,
                                    area,
                                )?;
                                let clip = unsafe { self.device.CreateRectangleClip()? };
                                unsafe { layer.visual.SetClip(&clip)? };
                                let opacity: IDCompositionVisual3 = layer
                                    .visual
                                    .cast()
                                    .context("IDCompositionVisual3 (Windows 10 or later)")?;
                                (layer, opacity, clip)
                            }
                        };
                        // Under the words still in flight, over the strip's new words
                        unsafe { self.swap.AddVisual(&layer.visual, true, None::<&IDCompositionVisual>)? };
                        // Only the rows that changed show: the rest of the strip as it was is
                        // the strip as it is, but for what the pointer lights, which the old
                        // copy doesn't
                        let x = (rows.x - area.x) as f32;
                        let y = (rows.y - area.y) as f32;
                        unsafe {
                            clip.SetLeft2(x)?;
                            clip.SetTop2(y)?;
                            clip.SetRight2(x + rows.w as f32)?;
                            clip.SetBottom2(y + rows.h as f32)?;
                        }
                        stack.layers.push(SwapLayer {
                            words,
                            generation,
                            layer,
                            opacity,
                            clip,
                        });
                    }
                    // Its opacity from now, and every other's that changed to keep its share
                    for &i in &switched.redrive {
                        let key = &stack.flight.leaving()[i].key;
                        let Some(layer) = stack.layers.iter().find(|s| s.words == *key) else {
                            continue;
                        };
                        let segments = stack.flight.opacity_segments(i, t, STEP, OPACITY_EPSILON);
                        let opacity = &layer.opacity;
                        self.name("strip.swap");
                        self.play(
                            &segments,
                            |a| unsafe { opacity.SetOpacity(a) },
                            |v| unsafe { opacity.SetOpacity2(v) },
                        )?;
                    }
                    changed = true;
                }
                Some(_) => {}
                None => {
                    if let Some((_, _, generation, _)) = swap {
                        stack.shown = generation;
                    }
                }
            }
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: the strip's words in flight keep a visual each, in the flight's order: those no
        /// longer in it leave the tree, their surfaces kept for the next (two at most); whether
        /// any did
        fn sync_swaps(&self, stack: &mut SwapStack) -> anyhow::Result<bool> {
            let mut old = std::mem::take(&mut stack.layers);
            for l in stack.flight.leaving() {
                if let Some(i) = old.iter().position(|s| s.words == l.key) {
                    stack.layers.push(old.remove(i));
                }
            }
            let any = !old.is_empty();
            for s in old {
                unsafe { self.swap.RemoveVisual(&s.layer.visual)? };
                if stack.spare.len() < 2 {
                    stack.spare.push((s.layer, s.opacity, s.clip));
                }
            }
            Ok(any)
        }

        /// LBTerm: this frame's texture for the newest of the strip's words in flight, the strip
        /// as it was before this swap, while it shows
        pub fn acquire_swap(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let stack = self.swaps.borrow();
            match stack.layers.last() {
                Some(s) if s.generation == stack.shown => Ok(s.layer.frame(Target::Swap, content)?.into_iter().collect()),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: the strip's words in flight done by `now` leave the tree; whether any did
        fn put_away_done_swaps(&self, now: Instant) -> anyhow::Result<bool> {
            let mut stack = self.swaps.borrow_mut();
            let stack = &mut *stack;
            stack.flight.put_away_done(self.clock(now));
            self.sync_swaps(stack)
        }

        /// LBTerm: how many of the strip's words in flight are older than the newest swap, which
        /// still fade out over it (all of them once the newest's are done), and when the soonest
        /// of them all is done
        pub fn fading_swaps(&self) -> (usize, Option<Instant>) {
            let stack = self.swaps.borrow();
            (
                stack.layers.iter().filter(|s| s.generation != stack.shown).count(),
                stack.flight.leaving().iter().map(|l| self.at_clock(l.end())).min(),
            )
        }

        /// LBTerm: whether swap `generation` still shows at `now`: new, or the newest in flight
        pub fn swap_showing(&self, generation: u64, now: Instant) -> bool {
            let stack = self.swaps.borrow();
            generation != stack.shown
                || stack.layers.last().map_or(false, |s| s.generation == generation)
                    && stack
                        .flight
                        .leaving()
                        .last()
                        .map_or(false, |l| self.at_clock(l.end()) > now)
        }

        /// LBTerm: the strips down the window's right as the tab left showed them
        /// (render/sides.rs), by the tabs in flight the panes go by (`lbterm_motion::flight`, from
        /// what `prepare` did this frame). When the panes on screen dissolved this frame,
        /// `picture` (where the strips' picture from the frame before is drawn this frame, if
        /// they looked different) goes over the next tab's strips, under the pictures still
        /// fading, and its opacity is its tab's in flight, as its panes' is; and every picture
        /// whose tab's opacity was played again this frame plays it again too. A picture whose
        /// tab is no longer in flight goes at once (its panes came back whole), and so does one
        /// done fading, and every one while the window resizes or with motion off. A new one has
        /// its first frame before the tree shows it, and only that one: its visual keeps it. Call
        /// after `prepare`.
        pub fn prepare_sides(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            picture: Option<PixelRect>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let t = self.clock(now);
            let (flight, dissolved, redriven, resizing) = {
                let layers = self.layers.borrow();
                (layers.flight.clone(), layers.dissolved.clone(), layers.redriven.clone(), layers.resizing)
            };
            let animate = motion.enabled && !resizing;
            let mut changed = self.put_away_sides(|p| {
                !animate || p.done <= now || flight.position(|k| *k == p.panes).is_none()
            })?;
            let mut sides = self.sides_layers.borrow_mut();
            sides.drawn = false;
            for p in sides.fading.iter_mut() {
                let Some(i) = flight.position(|k| *k == p.panes).filter(|_| redriven.contains(&p.panes)) else {
                    continue;
                };
                self.name("sides.dissolve");
                let segments = flight.opacity_segments(i, t, STEP, OPACITY_EPSILON);
                let opacity = &p.opacity;
                self.play(&segments, |a| unsafe { opacity.SetOpacity(a) }, |v| unsafe { opacity.SetOpacity2(v) })?;
                p.done = self.at_clock(flight.leaving()[i].end());
                changed = true;
            }
            let new = flight
                .position(|k| !dissolved.is_empty() && *k == dissolved)
                .filter(|_| animate)
                .zip(picture);
            if let Some((i, rect)) = new {
                let (layer, opacity) = match sides.spare.pop() {
                    Some((mut layer, opacity)) => {
                        layer.place(device, rect)?;
                        (layer, opacity)
                    }
                    None => {
                        let visual = unsafe { self.device.CreateVisual()? };
                        let layer = Layer::new(
                            instance,
                            device,
                            visual,
                            base,
                            wgpu::CompositeAlphaMode::PreMultiplied,
                            rect,
                        )?;
                        let opacity: IDCompositionVisual3 = layer
                            .visual
                            .cast()
                            .context("IDCompositionVisual3 (Windows 10 or later)")?;
                        (layer, opacity)
                    }
                };
                // Under the older pictures, as the panes of the tab left go under the older
                // tabs' still fading, and over the next tab's strips in `over`
                unsafe { self.sides.AddVisual(&layer.visual, true, None::<&IDCompositionVisual>)? };
                self.name("sides.dissolve");
                let segments = flight.opacity_segments(i, t, STEP, OPACITY_EPSILON);
                self.play(&segments, |a| unsafe { opacity.SetOpacity(a) }, |v| unsafe { opacity.SetOpacity2(v) })?;
                let done = self.at_clock(flight.leaving()[i].end());
                log::debug!(
                    "sides: the strips as they were over {rect:?} go with panes {dissolved:?}, shown {:.2}, under {} more",
                    flight.leaving()[i].share(t).0,
                    sides.fading.len()
                );
                sides.fading.push(SidePicture {
                    layer,
                    opacity,
                    panes: dissolved,
                    done,
                });
                sides.drawn = true;
                changed = true;
            }
            if changed {
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: the strips' pictures `gone` picks leave the tree, their surfaces kept for the
        /// next; whether any did
        fn put_away_sides(&self, mut gone: impl FnMut(&SidePicture) -> bool) -> anyhow::Result<bool> {
            let mut sides = self.sides_layers.borrow_mut();
            let sides = &mut *sides;
            let mut any = false;
            for p in std::mem::take(&mut sides.fading) {
                if gone(&p) {
                    unsafe { self.sides.RemoveVisual(&p.layer.visual)? };
                    // Enough for a switch and the next before it's done; a run of quick switches
                    // makes the rest
                    if sides.spare.len() < 2 {
                        sides.spare.push((p.layer, p.opacity));
                    }
                    any = true;
                } else {
                    sides.fading.push(p);
                }
            }
            Ok(any)
        }

        /// LBTerm: the frame for the strips' picture made this frame, its only one, unless its
        /// surface, kept from an earlier picture, shows it already
        pub fn acquire_sides(&self, content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            let mut sides = self.sides_layers.borrow_mut();
            let drawn = std::mem::take(&mut sides.drawn);
            match sides.fading.last().filter(|_| drawn) {
                Some(p) => Ok(p.layer.frame(Target::Sides, content)?.into_iter().collect()),
                None => Ok(vec![]),
            }
        }

        /// LBTerm: how many pictures of the strips are still in the tree, fading out (or done and
        /// not yet put away), and when the soonest is done
        pub fn fading_sides(&self) -> (usize, Option<Instant>) {
            let sides = self.sides_layers.borrow();
            (sides.fading.len(), sides.fading.iter().map(|p| p.done).min())
        }
    }

    /// LBTerm: one brief light (render/lights.rs): a visual showing the white swatch through a
    /// rounded clip, its opacity the light.
    struct Lit {
        visual: IDCompositionVisual3,
        clip: IDCompositionRectangleClip,
        /// What it's over, and so which container holds it
        over: Over,
    }

    /// Where a brief light goes: over the panes and the strip (`lights`), over the tab panel
    /// (`panel_lights`: a pad's glint, a press on the + pad), or over every menu's card
    /// (`menu_lights`: a picked row's flash)
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Over {
        Panes,
        Panel,
        Menus,
    }

    impl Lit {
        fn new(dcomp: &Compositor) -> anyhow::Result<Self> {
            let visual: IDCompositionVisual3 = unsafe { dcomp.device.CreateVisual()? }
                .cast()
                .context("IDCompositionVisual3 (Windows 10 or later)")?;
            let clip = unsafe { dcomp.device.CreateRectangleClip()? };
            unsafe {
                visual.SetClip(&clip)?;
                visual.SetOpacity2(0.0)?;
            }
            Ok(Self {
                visual,
                clip,
                over: Over::Panes,
            })
        }
    }

    /// LBTerm: the lights over the panes, and their swatch.
    #[derive(Default)]
    struct LightLayers {
        swatch: Option<Layer>,
        /// The swatch's swapchain, which every light shows
        content: Option<IUnknown>,
        /// The swatch is new or a new size, so it's cleared to white this frame
        draw: bool,
        /// The lights showing, and when each is done
        shown: Vec<(Lit, Instant)>,
        spare: Vec<Lit>,
    }

    /// One corner of a rectangle clip, both its radii.
    #[derive(Clone, Copy)]
    enum Corner {
        TopLeft,
        TopRight,
        BottomLeft,
        BottomRight,
    }

    impl Corner {
        const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

        fn set(self, clip: &IDCompositionRectangleClip, r: f32) -> windows58::core::Result<()> {
            unsafe {
                match self {
                    Corner::TopLeft => {
                        clip.SetTopLeftRadiusX2(r)?;
                        clip.SetTopLeftRadiusY2(r)
                    }
                    Corner::TopRight => {
                        clip.SetTopRightRadiusX2(r)?;
                        clip.SetTopRightRadiusY2(r)
                    }
                    Corner::BottomLeft => {
                        clip.SetBottomLeftRadiusX2(r)?;
                        clip.SetBottomLeftRadiusY2(r)
                    }
                    Corner::BottomRight => {
                        clip.SetBottomRightRadiusX2(r)?;
                        clip.SetBottomRightRadiusY2(r)
                    }
                }
            }
        }

        fn animate(self, clip: &IDCompositionRectangleClip, a: &IDCompositionAnimation) -> windows58::core::Result<()> {
            unsafe {
                match self {
                    Corner::TopLeft => {
                        clip.SetTopLeftRadiusX(a)?;
                        clip.SetTopLeftRadiusY(a)
                    }
                    Corner::TopRight => {
                        clip.SetTopRightRadiusX(a)?;
                        clip.SetTopRightRadiusY(a)
                    }
                    Corner::BottomLeft => {
                        clip.SetBottomLeftRadiusX(a)?;
                        clip.SetBottomLeftRadiusY(a)
                    }
                    Corner::BottomRight => {
                        clip.SetBottomRightRadiusX(a)?;
                        clip.SetBottomRightRadiusY(a)
                    }
                }
            }
        }
    }

    /// LBTerm: the turning arc on a Conductor menu item waiting for its answer
    /// (render/conductor.rs). Made the first time one waits.
    #[derive(Default)]
    struct MenuSpinner {
        template: Option<Layer>,
        /// Holds the template's own visual, clipped to nothing
        group: Option<IDCompositionVisual2>,
        visual: Option<IDCompositionVisual2>,
        rotate: Option<IDCompositionRotateTransform>,
        opacity: Option<IDCompositionVisual3>,
        rect: PixelRect,
        /// It shows this frame: an item waits, and its template is drawn
        wanted: bool,
        /// Its visual's opacity is up: while an item waits, and after, while the Conductor's card
        /// fades out with it held where it was
        showing: bool,
        /// DWM turns it, a turn every SPIN_PERIOD for ever
        turning: bool,
        /// When the turn now playing started, to hold the arc where it is
        started: Option<Instant>,
    }

    impl MenuSpinner {
        /// Turning from upright, DWM playing it, or still and upright, as `on` says; nothing done
        /// when it's that already, so a turn carries on where it is
        fn turn(&mut self, dcomp: &Compositor, on: bool) -> anyhow::Result<()> {
            let Some(rotate) = self.rotate.as_ref() else {
                return Ok(());
            };
            if self.turning == on {
                return Ok(());
            }
            if on {
                let animation = unsafe { dcomp.device.CreateAnimation()? };
                unsafe {
                    animation.AddCubic(0.0, 0.0, (360.0 / SPIN_PERIOD) as f32, 0.0, 0.0)?;
                    animation.AddRepeat(SPIN_PERIOD, SPIN_PERIOD)?;
                    rotate.SetAngle(&animation)?;
                }
                self.started = Some(Instant::now());
            } else {
                unsafe { rotate.SetAngle2(0.0)? };
                self.started = None;
            }
            self.turning = on;
            Ok(())
        }

        /// Still where the turn has it now, while its card fades out: no jump back to upright
        fn hold(&mut self) -> anyhow::Result<()> {
            let (Some(rotate), Some(started)) = (self.rotate.as_ref(), self.started.take()) else {
                return Ok(());
            };
            let turns = started.elapsed().as_secs_f64() / SPIN_PERIOD;
            unsafe { rotate.SetAngle2((turns.fract() * 360.0) as f32)? };
            self.turning = false;
            Ok(())
        }
    }

    /// LBTerm: the easter egg's hearts (render/triggers.rs): their sheet and its swapchain, made
    /// the first time a burst rises, and each burst in the air
    #[derive(Default)]
    struct HeartsLayers {
        sheet: Option<Layer>,
        content: Option<IUnknown>,
        shown: Vec<ShownHearts>,
    }

    /// A burst in the air: its group and the clip that cuts it off at its pane's top, its pane,
    /// where the pane was as it rose and where it is now (none while it isn't shown), and when
    /// it's over
    struct ShownHearts {
        group: IDCompositionVisual2,
        cut: IDCompositionRectangleClip,
        pane: PaneId,
        rose: lbterm_triggers::hearts::Place,
        at: Option<lbterm_triggers::hearts::Place>,
        done: Instant,
    }

    /// One of a heart's tracks (lbterm_triggers::hearts::Track) as DWM plays it, `a` times it and
    /// `b` added: in the window's pixels from the hearts' own, or a scale from a size
    fn hearts_segments(track: &lbterm_triggers::hearts::Track, a: f32, b: f32) -> Segments {
        Segments {
            cubics: track
                .pieces
                .iter()
                .map(|p| {
                    let [c0, c1, c2, c3] = p.coefficients;
                    Cubic {
                        begin: p.begin,
                        coefficients: [a * c0 + b, a * c1, a * c2, a * c3],
                    }
                })
                .collect(),
            end: track.end,
            end_value: a * track.end_value + b,
        }
    }

    /// LBTerm: the lights round a tab link drag's matched blocks (render/link_drag.rs). Made the
    /// first time a drag matches.
    #[derive(Default)]
    struct LinkLamps {
        template: Option<Layer>,
        /// Holds the template's own visual, clipped to nothing
        group: Option<IDCompositionVisual2>,
        lamps: Vec<IDCompositionVisual2>,
        plan: Option<super::LampPlan>,
        /// DWM carries its lamps round lap after lap
        turning: bool,
    }

    /// LBTerm: the lights along the rail's suggested tab connections (render/connections.rs).
    /// Made the first time one is suggested.
    #[derive(Default)]
    struct ConnectionLamps {
        template: Option<Layer>,
        /// Holds the template's own visual, clipped to nothing
        group: Option<IDCompositionVisual2>,
        /// The template's swapchain, which every lamp shows
        content: Option<IUnknown>,
        lamps: Vec<ConnectionLamp>,
        /// When each line's light runs its laps (Lachlan's 1a)
        laps: lbterm_extras::glow::Laps,
        /// A suggestion shows on the rail
        shown: bool,
        /// The plan and whether loops played at the last prepare that did anything: a frame
        /// with both the same and no laps owed leaves the lamps as they are
        plan: Option<super::GlowPlan>,
        play: bool,
    }

    /// Each lamp whose laps have ended set still at its line's middle by value, on the same
    /// visual, letting the animation go, so the window's tree keeps none for a light that's
    /// still. The animation's `End` has it there already, so nothing moves on screen. Whether
    /// any was, so the tree is committed
    fn settle_lamps(lamps: &mut [ConnectionLamp], now: Instant) -> anyhow::Result<bool> {
        let mut any = false;
        for lamp in lamps {
            if matches!(lamp.glow, lbterm_extras::glow::Glow::Laps { .. }) && !lamp.glow.running(lamp.lap, now) {
                unsafe { lamp.visual.SetOffsetY2(lamp.path[0].1)? };
                lamp.glow = lbterm_extras::glow::Glow::Still;
                any = true;
            }
        }
        Ok(any)
    }

    /// LBTerm: one suggested line's light, as DWM was given it
    struct ConnectionLamp {
        id: String,
        lap: f64,
        path: Vec<(f32, f32)>,
        glow: lbterm_extras::glow::Glow,
        visual: IDCompositionVisual2,
    }

    /// A key of where a light goes: its path and its lap
    fn place_key(path: &[(f32, f32)], lap: f64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (x, y) in path {
            (x.to_bits(), y.to_bits()).hash(&mut h);
        }
        lap.to_bits().hash(&mut h);
        h.finish()
    }

    /// LBTerm: the strip's detail card (render/strip.rs): a visual the card's size, its shadow
    /// included, its opacity faded by DWM. Made the first time the card opens.
    #[derive(Default)]
    struct StripCard {
        layer: Option<Layer>,
        opacity: Option<IDCompositionVisual3>,
        alpha: Option<Tween>,
        /// Where its opacity ends up once the fade is done
        rest: f64,
        /// Which way it's going: open, or closing
        open: bool,
        /// It shows a frame this time
        wanted: bool,
    }

    impl StripCard {
        fn current(&self, now: Instant) -> f64 {
            self.alpha.as_ref().map_or(self.rest, |t| t.value(now))
        }

        /// Straight to `to`, ending any fade
        fn settle(&mut self, to: f64) -> anyhow::Result<()> {
            if let Some(opacity) = &self.opacity {
                unsafe { opacity.SetOpacity2(to as f32)? };
            }
            self.alpha = None;
            self.rest = to;
            Ok(())
        }
    }

    /// LBTerm: each menu slot's names for the state file's motion: its fade, its highlight's
    /// glide and its highlight's light (render/menu_fade.rs, `Slot`)
    const MENU_NAMES: [(&str, &str, &str, &str); MENU_SLOTS] = [
        ("menu.tab", "menu.tab.glide", "menu.tab.light", "menu.tab.hairline"),
        ("menu.prompt", "menu.prompt.glide", "menu.prompt.light", "menu.prompt.hairline"),
        ("menu.conductor", "menu.conductor.glide", "menu.conductor.light", "menu.conductor.hairline"),
        ("menu.list", "menu.list.glide", "menu.list.light", "menu.list.hairline"),
    ];
    /// Each slot's name for its card's page turning, its old frame fading out over the new
    const MENU_SWAP_NAMES: [&str; MENU_SLOTS] = ["menu.tab.swap", "menu.prompt.swap", "menu.conductor.swap", "menu.list.swap"];
    /// Each slot's picked row's flash (`Light::Row`), as the state file's animations name it
    const MENU_FLASH_NAMES: [&str; MENU_SLOTS] = ["menu.tab.flash", "menu.prompt.flash", "menu.conductor.flash", "menu.list.flash"];

    /// LBTerm: one menu slot's card (render/menu_fade.rs): a group whose opacity DWM fades, over
    /// the card (its light, fill and words) and a white surface over it seen through a rounded clip
    /// as its highlight. Made the first time a menu in the slot opens.
    struct MenuCard {
        group: Option<IDCompositionVisual2>,
        opacity: Option<IDCompositionVisual3>,
        alpha: Option<Tween>,
        /// Where the group's opacity ends up once its fade is done
        rest: f64,
        /// Which way it's going: open, or closing
        open: bool,
        /// It's in use this frame: open or still fading out
        wanted: bool,
        card: Option<Layer>,
        light: Option<Layer>,
        /// The highlight: its clip's edges (left, top, right, bottom) and its light
        clip: Option<IDCompositionRectangleClip>,
        lit_opacity: Option<IDCompositionVisual3>,
        edges: [Option<Tween>; 4],
        edges_rest: [f64; 4],
        lit: Option<Tween>,
        lit_rest: f64,
        /// Its corners as last set (below 0 before any)
        radius: f64,
        /// The hairline's lamp, made the first time a key has nowhere to go
        lamp: Option<MenuLamp>,
        /// The keys that had nowhere to go, as far as the lamp has answered them
        bumps: u64,
        /// The card's or the highlight's surface may stand on a new swap chain since the lamp
        /// last showed them: one was made (`Layer::new`), configured again for a new size
        /// (`Layer::place`), or the card's two surfaces swapped round at a page turn
        /// (`menu_turn`). The lamp looks its chains up again only then
        chains_moved: bool,
        /// The card's other surface: the page before its last turn, its last frame faded out over
        /// the new page's on `swap` (`ghost_alpha`), then unseen until the next turn swaps the two
        /// round. Made the first time the slot's menu turns its page
        ghost: Option<Layer>,
        ghost_alpha: Option<Tween>,
        /// The page turns, as far as the card has answered them
        pages: u64,
    }

    /// LBTerm: a menu card's edge flaring once, a key no row took (lbterm_motion::menu::bump,
    /// Lachlan's 9a of 2026-11-08): its hairline and the light round it going white, played by
    /// DWM on two opacities from one chain. The hairline: a group over the card and under its
    /// highlight, holding the slot's white seen through the hairline's outer edge and over that
    /// the card itself again, seen through its fill, so only the hairline between them brightens.
    /// The light: the card's own surface again just under the card, through a colour matrix that
    /// makes each pixel white at its alpha times `palette::menu_bump_light_scale` (`halo`), so
    /// it's the card's light gone white, falling away as the red does: the card is opaque inside
    /// its hairline, so of the white only what's round it shows, under the card's red light. All
    /// show the slot's own surfaces: nothing is drawn for it
    struct MenuLamp {
        /// The group itself, as the interface its opacity is set through
        opacity: IDCompositionVisual3,
        white: (IDCompositionVisual2, IDCompositionRectangleClip),
        fill: (IDCompositionVisual2, IDCompositionRectangleClip),
        /// The light round the card, white: just under the card, its opacity played with the
        /// group's; its colour matrix kept with it
        halo: IDCompositionVisual3,
        _whiten: IDCompositionColorMatrixEffect,
        /// Where it's placed: the hairline and the surfaces' corner. Its visuals show the slot's
        /// swap chains, set again only when `MenuCard::chains_moved` says they may have changed,
        /// never looked up on a frame where nothing did (PITFALLS 62)
        placed: Option<(MenuRing, (i32, i32))>,
        /// Its flare under way, and when it started
        lit: Option<(Chain, Instant)>,
        /// Whether its two visuals (the hairline's group and the light) are in the card's group:
        /// only from a flare's start to the first frame after its end, so a menu at rest has
        /// nothing of the lamp in DWM's tree (no visual at opacity 0 to compose, no colour matrix
        /// to evaluate; COMPOSITOR.md, `menus`)
        attached: bool,
    }

    impl Default for MenuCard {
        fn default() -> Self {
            MenuCard {
                group: None,
                opacity: None,
                alpha: None,
                rest: 0.0,
                open: false,
                wanted: false,
                card: None,
                light: None,
                clip: None,
                lit_opacity: None,
                edges: Default::default(),
                edges_rest: [0.0; 4],
                lit: None,
                lit_rest: 0.0,
                radius: -1.0,
                lamp: None,
                bumps: 0,
                chains_moved: true,
                ghost: None,
                ghost_alpha: None,
                pages: 0,
            }
        }
    }

    impl MenuCard {
        fn current(&self, now: Instant) -> f64 {
            self.alpha.as_ref().map_or(self.rest, |t| t.value(now))
        }

        /// The group straight to `to`, ending any fade
        fn settle(&mut self, to: f64) -> anyhow::Result<()> {
            if let Some(opacity) = &self.opacity {
                unsafe { opacity.SetOpacity2(to as f32)? };
            }
            self.alpha = None;
            self.rest = to;
            Ok(())
        }

        /// Gone: unseen, its highlight out, so the next menu in the slot starts afresh
        fn put_away(&mut self) -> anyhow::Result<()> {
            self.settle(0.0)?;
            if let Some(lit) = &self.lit_opacity {
                unsafe { lit.SetOpacity2(0.0)? };
            }
            self.lit = None;
            self.lit_rest = 0.0;
            if let Some(lamp) = self.lamp.as_mut() {
                unsafe { lamp.opacity.SetOpacity2(0.0)? };
                lamp.lit = None;
            }
            if let Some(ghost) = &self.ghost {
                let opacity: IDCompositionVisual3 = ghost.visual.cast().context("IDCompositionVisual3 (Windows 10 or later)")?;
                unsafe { opacity.SetOpacity2(0.0)? };
            }
            self.ghost_alpha = None;
            self.wanted = false;
            self.open = false;
            Ok(())
        }
    }

    /// LBTerm: one of the strip's hover slots (render/hover_fade.rs): a visual the size of
    /// the strip showing one element's look under the pointer, its opacity faded by DWM.
    struct HoverSlot {
        layer: Layer,
        opacity: IDCompositionVisual3,
        alpha: Option<Tween>,
        /// Where its opacity ends up once the fade is done
        rest: f64,
    }

    impl HoverSlot {
        fn current(&self, now: Instant) -> f64 {
            self.alpha.as_ref().map_or(self.rest, |t| t.value(now))
        }

        /// Straight to `to`, ending any fade
        fn settle(&mut self, to: f64) -> anyhow::Result<()> {
            unsafe { self.opacity.SetOpacity2(to as f32)? };
            self.alpha = None;
            self.rest = to;
            Ok(())
        }
    }

    /// LBTerm: the strip's hover slots, made the first time a hover fades.
    #[derive(Default)]
    struct HoverLayers {
        slots: Vec<HoverSlot>,
        /// The slots show a frame this time: there's a strip
        wanted: bool,
    }

    // LBTerm: the compositor's tests, over the stand-in device (lbterm/COMPOSITOR.md, "Checking it
    // without a window"): inside `imp`, so they reach its fields
    #[cfg(test)]
    mod tests;
}

#[cfg(not(windows))]
mod imp {
    use super::{CardPlan, Content, Frame, Keyboard, Light, MenuVisual, MotionSettings, MENU_SLOTS, PanelPlan};
    use lbterm_motion::PixelRect;
    use mux::pane::PaneId;

    pub fn system_animations_on() -> bool {
        true
    }

    /// Only Windows has DirectComposition; `config::lbterm::uses_compositor`
    /// is always false elsewhere, so this is never made.
    pub enum Compositor {}

    impl Compositor {
        pub fn new(_hwnd: *mut std::ffi::c_void) -> anyhow::Result<Self> {
            anyhow::bail!("DirectComposition is Windows only")
        }

        pub fn create_under_surface(
            &self,
            _instance: &wgpu::Instance,
        ) -> anyhow::Result<wgpu::Surface<'static>> {
            match *self {}
        }

        pub fn prepare(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _window: PixelRect,
            _panes: &[(PaneId, PixelRect)],
            _motion: &MotionSettings,
            _panel: Option<&PanelPlan>,
            _top: bool,
            _cards: &CardPlan,
            _active: Option<PaneId>,
            _left: &dyn Fn(PaneId) -> bool,
            _backdrop: [f64; 3],
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn fading_panes(&self) -> (Vec<PaneId>, Option<std::time::Instant>) {
            match *self {}
        }

        pub fn put_away_faded(&self) -> anyhow::Result<bool> {
            match *self {}
        }

        pub fn spare_panes(&self) -> usize {
            match *self {}
        }

        pub fn fading_swaps(&self) -> (usize, Option<std::time::Instant>) {
            match *self {}
        }

        pub fn prepare_keyboard(&self, _keyboard: Keyboard, _motion: &MotionSettings) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn panes_pressed(&self) -> bool {
            match *self {}
        }

        pub fn panel_wave_drawn(&self) -> Option<u64> {
            match *self {}
        }

        pub fn commit_if_changed(&self) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn commit(&self) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn prepare_hover(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _strip: Option<PixelRect>,
            _fades: [Option<lbterm_extras::hover::Fade>; lbterm_extras::hover::SLOTS],
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_hover(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_strip_card(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _card: Option<(PixelRect, bool)>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_strip_card(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_menu_spinner(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _rect: Option<PixelRect>,
            _turning: bool,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_menu_spinner(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_link_lamps(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _plan: Option<super::LampPlan>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_link_lamps(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_connection_lamps(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _plan: Option<super::GlowPlan>,
            _play: bool,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn connection_lamps_pointer(&self) -> bool {
            match *self {}
        }

        pub fn loops_until(&self) -> Option<std::time::Instant> {
            match *self {}
        }


        pub fn acquire_connection_lamps(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_hearts(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _plan: Option<super::HeartsPlan>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_hearts(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn strip_card_showing(&self, _now: std::time::Instant) -> bool {
            match *self {}
        }

        pub fn strip_card_settles(&self) -> Option<std::time::Instant> {
            match *self {}
        }

        pub fn prepare_objectives(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _cards: [Option<(PixelRect, bool)>; 4],
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_objectives(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn objectives_showing(
            &self,
            _now: std::time::Instant,
        ) -> ([bool; 4], Option<std::time::Instant>) {
            match *self {}
        }

        pub fn prepare_tasks_card(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _card: Option<(PixelRect, bool)>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_tasks_card(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_new_tab(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _plus: Option<(PixelRect, bool)>,
            _picker: Option<(PixelRect, bool)>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_new_tab(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn picker_showing(&self, _now: std::time::Instant) -> (bool, Option<std::time::Instant>) {
            match *self {}
        }

        pub fn dim_all(&self, _all: bool) {
            match *self {}
        }

        pub fn prepare_menus(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _visuals: [Option<MenuVisual>; MENU_SLOTS],
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_menus(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn menus_fading_until(&self, _now: std::time::Instant) -> Option<std::time::Instant> {
            match *self {}
        }

        pub fn menu_showing(&self, _slot: usize, _now: std::time::Instant) -> bool {
            match *self {}
        }

        pub fn tasks_card_showing(
            &self,
            _now: std::time::Instant,
        ) -> (bool, Option<std::time::Instant>) {
            match *self {}
        }

        pub fn prepare_lights(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _window: PixelRect,
            _lights: Vec<Light>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_lights(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_readout(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _readout: Option<(PixelRect, bool)>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_readout(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn readout_fading(
            &self,
            _now: std::time::Instant,
        ) -> (bool, Option<std::time::Instant>) {
            match *self {}
        }

        pub fn prepare_swap(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _swap: Option<(PixelRect, PixelRect, u64, bool)>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_swap(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn swap_showing(&self, _generation: u64, _now: std::time::Instant) -> bool {
            match *self {}
        }

        pub fn prepare_sides(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _picture: Option<PixelRect>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_sides(&self, _content: Content<'_>) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn fading_sides(&self) -> (usize, Option<std::time::Instant>) {
            match *self {}
        }

        pub fn animations(&self, _now: std::time::Instant) -> Vec<lbterm_extras::state::Animation> {
            match *self {}
        }

        pub fn loops(&self) -> Vec<&'static str> {
            match *self {}
        }

        pub fn settles(&self) -> Option<std::time::Instant> {
            match *self {}
        }

        pub fn playing(&self, _now: std::time::Instant) -> Vec<(&'static str, std::time::Instant, std::time::Instant)> {
            match *self {}
        }
    }
}

pub use imp::Compositor;

// LBTerm: a stand-in for DirectComposition that records what the compositor gives it, for tests that
// check the visual tree and its animations with no window (lbterm/COMPOSITOR.md, "Checking it
// without a window")
#[cfg(all(test, windows))]
mod recorder;
