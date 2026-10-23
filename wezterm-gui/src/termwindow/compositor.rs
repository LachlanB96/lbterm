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
//!   their cards' outlines riding on their cut-off edges.
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
}

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
    /// The tabs' rows on the rail, in order, and how they move
    pub rows: Vec<RowPlan>,
    pub row_motion: RowMotion,
    /// The open tab's row and the top of its slot: its ring and bar are drawn there, in a
    /// visual that glides from the last
    pub marker: Option<(usize, i32)>,
    /// Each row's progress bar on the open panel (its tab, where, how far along), whose
    /// fill glides on `slide` when the program says it's further on
    pub progress: Vec<(usize, PixelRect, f64)>,
    /// The row a drag has picked up, and how far from its slot the pointer has it: it
    /// follows, over the others, and settles into its slot on `slide` when let go
    pub dragged: Option<(usize, f64)>,
    /// The Conductor's outlines (render/conductor.rs), one per pad it shows on, their hues
    /// turning; with motion off they hold still
    pub rings: Vec<RingPlan>,
    /// Where their templates are drawn: the outline's, round one of the pills, and the light's
    pub ring_template: PixelRect,
    pub light_template: PixelRect,
}

/// One of the Conductor's outlines: where it goes (its template's size), its look, and the light
/// going round it, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct RingPlan {
    pub rect: PixelRect,
    pub style: crate::termwindow::render::conductor::RingStyle,
    pub light: Option<RingLight>,
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

/// One tab's row on the rail (render/tab_panel.rs): which tab, so a row that moves is followed
/// and one that's new or gone is seen; its slot in the window; a hash of what it shows, so a
/// row whose look changes dissolves from its old one; and whether the pointer is on it.
#[derive(Debug, Clone, PartialEq)]
pub struct RowPlan {
    pub key: usize,
    pub slot: PixelRect,
    pub look: u64,
    pub hovered: bool,
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
    pub panel_open: Curve,
    pub panel_close: Curve,
    pub panel_glow: Curve,
    pub panel_sweep: Curve,
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
}

/// Whether the window has the keyboard, and how many times it has arrived from outside it
/// (`lbterm_extras::arrival`): the active outline goes out without it and pulses as it arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyboard {
    pub has: bool,
    pub arrivals: u64,
}

/// A brief light over the panes, asked for this frame (render/lights.rs): white, its shape a
/// clip of a swatch, and only the clip and its opacity move.
#[derive(Debug, Clone, PartialEq)]
pub enum Light {
    /// A click's ripple from `x, y`, a circle growing from `start` to `reach` pixels as it
    /// fades, on `press`
    Press { x: f64, y: f64, reach: f64, start: f64 },
    /// A copied or opened stretch of text lighting up and fading, on `copy_in` and `copy_out`:
    /// a rectangle per row, with corners of `radius`
    Text { rects: Vec<PixelRect>, radius: f64 },
    /// A tab's pad on the rail glinting, over the tab panel: a program typed into its tab,
    /// or it was renamed. Up to the press's white and back, on `copy_in` and `copy_out`
    Pad { rect: PixelRect, radius: f64 },
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
            panel_open: curve(&motion.panel_open),
            panel_close: curve(&motion.panel_close),
            panel_glow: curve(&motion.panel_glow),
            panel_sweep: curve(&motion.panel_sweep),
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
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{CardPlan, Frame, Keyboard, Light, MotionSettings, PanelPlan};
    use crate::termwindow::render::conductor::{RingStyle, HUE_PERIOD};
    use anyhow::Context;
    use lbterm_extras::hover::{Fade, SLOTS};
    use lbterm_motion::{
        CardPart, Chain, Change, Curve, Motion, PanelPart, PixelRect, Rect, Segments, Target,
    };
    use mux::pane::PaneId;
    use std::cell::RefCell;
    use std::time::{Duration, Instant};
    use windows58::core::{IUnknown, Interface};
    use windows58::Win32::Foundation::{BOOL, HWND};
    use windows58::Win32::Graphics::DirectComposition::{
        DCompositionCreateDevice3, IDCompositionAnimation, IDCompositionClip,
        IDCompositionDesktopDevice, IDCompositionDevice3, IDCompositionHueRotationEffect,
        IDCompositionRectangleClip, IDCompositionRotateTransform, IDCompositionTarget,
        IDCompositionVisual, IDCompositionVisual2, IDCompositionVisual3,
    };

    /// Pane visuals kept for reuse after their pane leaves the screen, so a
    /// tab switch resizes surfaces instead of making new swapchains.
    const SPARE_LAYERS: usize = 4;
    /// DWM gets a cubic per this much of an animation (halved where the
    /// curve is steep), well under a pixel from the curve
    const STEP: f64 = 1.0 / 120.0;
    /// A spring has settled once it's this many pixels from its target...
    const PIXEL_EPSILON: f64 = 0.1;
    /// ...or this close in opacity
    const OPACITY_EPSILON: f64 = 0.002;
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
            Ok(Self {
                surface,
                config,
                visual,
                rect,
            })
        }

        /// Make the swapchain `rect`'s size. The PTY already has it, so
        /// this happens once per layout change, never once per frame.
        fn resize(&mut self, device: &wgpu::Device, rect: PixelRect) {
            let (w, h) = (rect.w.max(1), rect.h.max(1));
            if (w, h) != (self.config.width, self.config.height) {
                self.config.width = w;
                self.config.height = h;
                self.surface.configure(device, &self.config);
            }
        }

        /// Move and resize to `rect` at once.
        fn place(&mut self, device: &wgpu::Device, rect: PixelRect) -> anyhow::Result<()> {
            self.resize(device, rect);
            if (rect.x, rect.y) != (self.rect.x, self.rect.y) {
                unsafe {
                    self.visual.SetOffsetX2(rect.x as f32)?;
                    self.visual.SetOffsetY2(rect.y as f32)?;
                }
            }
            self.rect = rect;
            Ok(())
        }
    }

    /// One number animating on a visual: kept so a retarget can start from
    /// where it is and how fast it's going.
    struct Tween {
        motion: Motion,
        start: Instant,
        end: f64,
    }

    impl Tween {
        fn elapsed(&self, now: Instant) -> f64 {
            now.saturating_duration_since(self.start).as_secs_f64()
        }

        fn value(&self, now: Instant) -> f64 {
            let t = self.elapsed(now);
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

        fn frames(&self, frames: &mut Vec<Frame>) -> anyhow::Result<()> {
            for (part, layer) in [
                (CardPart::MaskWide, &self.mask.wide),
                (CardPart::MaskTall, &self.mask.tall),
                (CardPart::LineWide, &self.line.wide),
                (CardPart::LineTall, &self.line.tall),
            ] {
                frames.push(Frame {
                    target: Target::Card(part),
                    rect: layer.rect,
                    texture: layer.surface.get_current_texture()?,
                });
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
        /// The container again, for its opacity: 85% without the keyboard
        dim: IDCompositionVisual3,
        /// Where the dimming is going, and how
        level: f64,
        dimming: Option<Tween>,
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
            let mut pane = Self {
                layer,
                edges,
                cards: cards.plan,
                dim,
                level: 1.0,
                dimming: None,
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

        /// To `level` (1 with the keyboard, 85% without): on `curve` from
        /// wherever it is, or at once without one.
        fn dim_to(&mut self, dcomp: &Compositor, level: f64, curve: Option<&Curve>, now: Instant) -> anyhow::Result<()> {
            let (from, _) = at(&self.dimming, self.level, now);
            self.level = level;
            dcomp.name("pane.dim");
            let dim = &self.dim;
            self.dimming = match curve {
                Some(curve) => dcomp.drive(
                    Motion::new(from, level, 0.0, curve.clone()),
                    OPACITY_EPSILON,
                    0.0,
                    |a| unsafe { dim.SetOpacity(a) },
                    |v| unsafe { dim.SetOpacity2(v) },
                )?,
                None => {
                    unsafe { dim.SetOpacity2(level as f32)? };
                    None
                }
            };
            Ok(())
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
            self.fade(dcomp, alpha, 1.0, &motion.enter)
        }

        fn fade(
            &mut self,
            dcomp: &Compositor,
            from: f64,
            to: f64,
            curve: &Curve,
        ) -> anyhow::Result<()> {
            dcomp.name(if to >= from { "pane.enter" } else { "pane.exit" });
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

        /// Fade out where it is; when it'll be done.
        fn fade_out(
            &mut self,
            dcomp: &Compositor,
            curve: &Curve,
            now: Instant,
        ) -> anyhow::Result<Instant> {
            let from = self.current_alpha(now);
            self.fade(dcomp, from, 0.0, curve)?;
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
    struct FocusOutline {
        lamp: IDCompositionVisual3,
        /// The lamp's pulse or fade under way, and when it started
        lit: Option<(Chain, Instant)>,
        /// Where the lamp's light rests
        light: f64,
        /// What the window said last: whether it had the keyboard, and
        /// its arrivals so far
        keyboard: Option<Keyboard>,
        group: IDCompositionVisual2,
        opacity: IDCompositionVisual3,
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
                keyboard: None,
                group,
                opacity,
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

        fn current(&self, now: Instant) -> ([f64; 4], [f64; 4]) {
            let s = self.shown;
            let (x, vx) = at(&self.x, s.x as f64, now);
            let (y, vy) = at(&self.y, s.y as f64, now);
            let (w, vw) = at(&self.w, s.w as f64, now);
            let (h, vh) = at(&self.h, s.h as f64, now);
            ([x, y, w, h], [vx, vy, vw, vh])
        }

        /// Straight onto `pane`, shown at `shown`.
        fn place(&mut self, pane: PaneId, shown: PixelRect) -> anyhow::Result<()> {
            unsafe {
                self.group.SetOffsetX2(shown.x as f32)?;
                self.group.SetOffsetY2(shown.y as f32)?;
                self.opacity.SetOpacity2(1.0)?;
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
            unsafe { self.opacity.SetOpacity2(1.0)? };
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
            });
            self.h = Some(Tween {
                end: down.end(PIXEL_EPSILON),
                motion: down,
                start: now,
            });
            self.pane = Some(pane);
            self.shown = shown;
            Ok(())
        }

        fn hide(&mut self) -> anyhow::Result<()> {
            unsafe { self.opacity.SetOpacity2(0.0)? };
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

        /// The light for what the window says about its keyboard: at rest
        /// while it has it, out without it, and a pulse up to full and back
        /// as it arrives from outside, each from wherever the light is.
        /// True when the tree changed.
        fn light(
            &mut self,
            dcomp: &Compositor,
            keyboard: Keyboard,
            motion: &MotionSettings,
            animate: bool,
            now: Instant,
        ) -> anyhow::Result<bool> {
            let rest = if keyboard.has {
                lbterm_panel::palette::outline_rest_opacity()
            } else {
                0.0
            };
            let was = self.keyboard.replace(keyboard);
            let arrived = keyboard.has
                && was.map_or(false, |was| was.arrivals != keyboard.arrivals);
            let turned = was.map_or(true, |was| was.has != keyboard.has);
            if !arrived && !turned {
                return Ok(false);
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
            } else {
                Chain::new(vec![Motion::new(from, rest, v, motion.dim.clone())], OPACITY_EPSILON)
            };
            dcomp.name(if arrived { "outline.arrive" } else { "outline.dim" });
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

    /// One tab's row: a visual showing the rows' front surface through the row's slot, and one
    /// showing the row words' (its open part, under the words' fade), moving together: sliding
    /// to the slot when it moves, growing in, fading out, dissolving.
    struct RowVisual {
        plan: super::RowPlan,
        visual: IDCompositionVisual3,
        clip: IDCompositionRectangleClip,
        words: IDCompositionVisual3,
        words_clip: IDCompositionRectangleClip,
        /// How far it shows from its slot, sliding there
        dy: Option<Tween>,
    }

    impl RowVisual {
        fn new(dcomp: &Compositor) -> anyhow::Result<Self> {
            let visual = || -> anyhow::Result<(IDCompositionVisual3, IDCompositionRectangleClip)> {
                let visual: IDCompositionVisual3 = unsafe { dcomp.device.CreateVisual()? }
                    .cast()
                    .context("IDCompositionVisual3 (Windows 10 or later)")?;
                let clip = unsafe { dcomp.device.CreateRectangleClip()? };
                unsafe { visual.SetClip(&clip)? };
                Ok((visual, clip))
            };
            let (rail, clip) = visual()?;
            let (words, words_clip) = visual()?;
            Ok(Self {
                plan: super::RowPlan {
                    key: usize::MAX,
                    slot: PixelRect::default(),
                    look: 0,
                    hovered: false,
                },
                visual: rail,
                clip,
                words,
                words_clip,
                dy: None,
            })
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

        /// Into `groups` just above `row`
        fn attach_above(&self, groups: &RowGroups, row: &RowVisual) -> windows58::core::Result<()> {
            unsafe {
                groups.rail.AddVisual(&self.visual, true, &row.visual)?;
                groups.words.AddVisual(&self.words, true, &row.words)
            }
        }

        fn detach(&self, groups: &RowGroups) -> windows58::core::Result<()> {
            unsafe {
                groups.rail.RemoveVisual(&self.visual)?;
                groups.words.RemoveVisual(&self.words)
            }
        }

        /// Shows `content` (surfaces of the panel's surface) through `slot`, `dy` from it.
        fn show(&self, content: &RowContent, surface: PixelRect, slot: PixelRect, dy: f32) -> windows58::core::Result<()> {
            let [rail, words] = self.both();
            for ((visual, clip), content) in [(rail, &content.rail), (words, &content.words)] {
                unsafe {
                    visual.SetContent(content)?;
                    visual.SetOffsetX2(surface.x as f32)?;
                    visual.SetOffsetY2(surface.y as f32 + dy)?;
                    clip.SetLeft2(0.0)?;
                    clip.SetRight2(surface.w as f32)?;
                    clip.SetTop2((slot.y - surface.y) as f32)?;
                    clip.SetBottom2((slot.y - surface.y) as f32 + slot.h as f32)?;
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
    /// as long as the program says
    struct ProgressBar {
        key: usize,
        visual: IDCompositionVisual2,
        clip: IDCompositionRectangleClip,
        /// Its row's words visual, which it rides in
        parent: IDCompositionVisual3,
        track: PixelRect,
        fraction: f64,
        /// The clip's right edge, while it glides
        right: Option<Tween>,
    }

    struct PanelLayers {
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
        /// Rows gone or changed, their old selves fading out, and when each is done
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
            // The rows' surfaces: a new size is a new swapchain, so the rows show the new
            // ones, and what the old ones kept is gone
            for i in 0..2 {
                self.rows[i].place(device, plan.surface)?;
                self.row_words[i].place(device, plan.surface)?;
                let rail = swap_chain(&self.rows[i].surface)?;
                let words = swap_chain(&self.row_words[i].surface)?;
                if rail != self.rows_content[i] || words != self.row_words_content[i] {
                    self.rows_content[i] = rail;
                    self.row_words_content[i] = words;
                    let groups = self.row_groups();
                    for (old, _) in std::mem::take(&mut self.old_rows) {
                        old.detach(&groups)?;
                    }
                    let content = self.row_content(self.front);
                    for row in &self.row_visuals {
                        row.show(&content, plan.surface, row.plan.slot, 0.0)?;
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
            }
            self.plan = plan.clone();
            Ok(())
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
        /// arcs a plan keeps go on turning where they are. True when the
        /// tree changed.
        fn sync_spinners(&mut self, dcomp: &Compositor, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<bool> {
            let wanted = &plan.spinners;
            let same = wanted.len() == self.spinners.len()
                && wanted.iter().zip(&self.spinners).all(|(r, s)| *r == s.rect);
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
                    let visual = unsafe { dcomp.device.CreateVisual()? };
                    let rotate = unsafe { dcomp.device.CreateRotateTransform()? };
                    unsafe {
                        visual.SetTransform(&rotate)?;
                        self.body_group.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                    }
                    if plan.spin {
                        // A turn every SPIN_PERIOD, for ever: DWM plays it
                        let animation = unsafe { dcomp.device.CreateAnimation()? };
                        unsafe {
                            animation.AddCubic(0.0, 0.0, (360.0 / SPIN_PERIOD) as f32, 0.0, 0.0)?;
                            animation.AddRepeat(SPIN_PERIOD, SPIN_PERIOD)?;
                            rotate.SetAngle(&animation)?;
                        }
                    } else {
                        unsafe { rotate.SetAngle2(0.0)? };
                    }
                    self.spinners.push(Spinner {
                        visual,
                        rotate,
                        rect: PixelRect::default(),
                    });
                }
                let spinner = &mut self.spinners[i];
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
            // Old rows that are done go; then nothing shows the kept surface
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

            let mut old: Vec<RowVisual> = std::mem::take(&mut self.row_visuals);
            let leaving: Vec<usize> = (0..old.len())
                .filter(|&i| !plan.rows.iter().any(|r| r.key == old[i].plan.key))
                .collect();
            let restyled: Vec<usize> = (0..old.len())
                .filter(|&i| {
                    plan.rows.iter().any(|r| {
                        r.key == old[i].plan.key && (r.look != old[i].plan.look || r.hovered != old[i].plan.hovered)
                    })
                })
                .collect();
            let first = old.is_empty() && self.old_rows.is_empty();
            // The front surfaces hold the frame drawn before this one: keep them for the old
            // selves, and draw on into the others. Old selves still showing those are cut short
            let keep = animate && !first && (!leaving.is_empty() || !restyled.is_empty());
            let kept = self.front;
            if keep {
                for (gone, _) in std::mem::take(&mut self.old_rows) {
                    gone.detach(&groups)?;
                }
                self.front = 1 - self.front;
                changed = true;
            }
            let content = self.row_content(self.front);
            let kept_content = self.row_content(kept);

            // A changed row's old self over it, dissolving into the new
            for &i in &restyled {
                let row = &old[i];
                let new = plan.rows.iter().find(|r| r.key == row.plan.key).expect("restyled rows are in the plan");
                let only_hover = new.look == row.plan.look;
                let ghost = RowVisual::new(dcomp)?;
                let (dy, _) = row.dy_at(now);
                ghost.show(&kept_content, surface, row.plan.slot, dy as f32)?;
                // Just above the row it was
                ghost.attach_above(&groups, row)?;
                let curve = if only_hover { &motion.hover } else { &motion.swap };
                let fade = Motion::new(1.0, 0.0, 0.0, curve.clone());
                let end = fade.end(OPACITY_EPSILON);
                dcomp.name(if only_hover { "rail.row.hover" } else { "rail.row.swap" });
                ghost.fade(dcomp, fade)?;
                self.old_rows.push((ghost, now + Duration::from_secs_f64(end)));
            }
            // A row that's gone: its old self, under the rest, fading out where it was
            for &i in leaving.iter().rev() {
                let gone = old.remove(i);
                gone.detach(&groups)?;
                if keep {
                    gone.attach(&groups, true)?;
                    let (dy, _) = gone.dy_at(now);
                    gone.show(&kept_content, surface, gone.plan.slot, dy as f32)?;
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
                        let mut row = RowVisual::new(dcomp)?;
                        row.attach(&groups, false)?;
                        changed = true;
                        if animate && !first {
                            // Its slot opens from the top as the rows below make room, and it
                            // fades in with it
                            row.show(&content, surface, want.slot, 0.0)?;
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
                    row.show(&content, surface, want.slot, dy as f32)?;
                    if row.dy.is_none() && self.dragging != Some(want.key) {
                        row.detach(&groups)?;
                        row.attach(&groups, false)?;
                    }
                    row.dy = Some(Tween {
                        motion: Motion::new(dy, dy, 0.0, Curve::Instant),
                        start: now,
                        end: 0.0,
                    });
                    changed = true;
                } else if self.dragging == Some(want.key) && animate {
                    // Let go: into its slot from where the pointer left it
                    let (dy, v) = row.dy_at(now);
                    let from = if moved { dy + (row.plan.slot.y - want.slot.y) as f64 } else { dy };
                    row.show(&content, surface, want.slot, from as f32)?;
                    row.slide(dcomp, surface.y as f64, from, v, &motion.slide)?;
                    changed = true;
                } else if moved && animate && !first && row.plan.key != usize::MAX {
                    // From where it shows now to its new slot, keeping its speed
                    let (dy, v) = row.dy_at(now);
                    let from = dy + (row.plan.slot.y - want.slot.y) as f64;
                    row.show(&content, surface, want.slot, from as f32)?;
                    row.slide(dcomp, surface.y as f64, from, v, &motion.slide)?;
                    changed = true;
                } else if moved || row.plan.slot != want.slot || keep || at.is_none() {
                    let (dy, _) = if moved { (0.0, 0.0) } else { row.dy_at(now) };
                    if moved {
                        row.dy = None;
                    }
                    row.show(&content, surface, want.slot, dy as f32)?;
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
                && plan.progress.iter().zip(&self.bars).all(|((k, r, f), b)| {
                    *k == b.key && *r == b.track && *f == b.fraction && parent_of(*k).as_ref() == Some(&b.parent)
                });
            if same {
                return Ok(false);
            }
            let s = plan.surface;
            let mut old = std::mem::take(&mut self.bars);
            for &(key, track, fraction) in &plan.progress {
                let Some(parent) = parent_of(key) else {
                    continue;
                };
                let right = (track.x - s.x) as f64 + track.w as f64 * fraction.clamp(0.0, 1.0);
                let mut bar = match old.iter().position(|b| b.key == key) {
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
                });
                self.marker_at = plan.marker;
                self.marker_held = true;
                return Ok(true);
            }
            let released = std::mem::take(&mut self.marker_held);
            if plan.marker == self.marker_at && !released {
                return Ok(false);
            }
            match (self.marker_at, plan.marker, animate) {
                (Some((was_key, was_y)), Some((key, y)), true) => {
                    let (dy, v) = at(&self.marker_dy, 0.0, now);
                    let from = dy + (was_y - y) as f64;
                    // Another tab: a glide; the same one moving with its row (or let go): a slide
                    let curve = if key == was_key {
                        &plan.row_motion.slide
                    } else {
                        &plan.row_motion.marker
                    };
                    dcomp.name("rail.marker");
                    self.marker_dy = drive_off(dcomp, visual, y0, from, v, curve)?;
                }
                _ => {
                    unsafe { visual.SetOffsetY2(y0 as f32)? };
                    self.marker_dy = None;
                }
            }
            self.marker_at = plan.marker;
            Ok(true)
        }

        /// The Conductor's outlines `plan` names, and none elsewhere: those it keeps go on as they
        /// are, their hues and lights where they've got to. True when the tree changed.
        fn sync_rings(&mut self, dcomp: &Compositor, device: &wgpu::Device, plan: &PanelPlan) -> anyhow::Result<bool> {
            let wanted = &plan.rings;
            let same = wanted.len() == self.rings.len()
                && wanted.iter().zip(&self.rings).all(|(w, r)| *w == r.plan)
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
            let mut old = std::mem::take(&mut self.rings);
            for want in wanted {
                let ring = match old.iter().position(|r| r.plan == *want) {
                    Some(at) => old.remove(at),
                    None => self.new_ring(dcomp, want, plan.spin)?,
                };
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
            for gone in self.spinners.drain(..) {
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
            self.sweep = None;
            self.edge = None;
            self.light = None;
            self.words_tween = None;
            self.hidden = true;
            Ok(())
        }

        fn frames(&self, frames: &mut Vec<Frame>) -> anyhow::Result<()> {
            for (part, layer) in [
                (PanelPart::Glow, &self.glow),
                (PanelPart::Body, &self.body),
                // The front of the rows' two surfaces only: the other keeps an old frame
                (PanelPart::Rows, &self.rows[self.front]),
                (PanelPart::RowWords, &self.row_words[self.front]),
                (PanelPart::Marker, &self.marker),
                (PanelPart::Progress, &self.progress),
                (PanelPart::Lit, &self.lit),
                (PanelPart::Words, &self.words),
                (PanelPart::CapGlow, &self.cap_glow),
                (PanelPart::Cap, &self.cap),
                (PanelPart::CapLit, &self.cap_lit),
                (PanelPart::Spot, &self.spot),
                (PanelPart::SpotCore, &self.spot_core),
            ] {
                frames.push(Frame {
                    target: Target::Panel(part),
                    rect: layer.rect,
                    texture: layer.surface.get_current_texture()?,
                });
            }
            // The arc template, only while a pad shows it
            if !self.spinners.is_empty() {
                frames.push(Frame {
                    target: Target::Panel(PanelPart::Spinner),
                    rect: self.spinner.rect,
                    texture: self.spinner.surface.get_current_texture()?,
                });
            }
            // The Conductor's templates, while an outline shows them
            if !self.rings.is_empty() {
                for (part, layer) in [
                    (PanelPart::RingSpectrum, &self.ring_spectrum),
                    (PanelPart::RingIris, &self.ring_iris),
                    (PanelPart::RingLight, &self.ring_light),
                ] {
                    frames.push(Frame {
                        target: Target::Panel(part),
                        rect: layer.rect,
                        texture: layer.surface.get_current_texture()?,
                    });
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
        panes: Vec<(PaneId, PaneLayer)>,
        /// Panes gone from the layout, fading out under the rest
        exiting: Vec<(PaneId, PaneLayer, Instant)>,
        spare: Vec<PaneLayer>,
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
    }

    pub struct Compositor {
        device: IDCompositionDesktopDevice,
        /// With a state file (lbterm/STATE.md): what the next animation started is, set where
        /// what's animated is known, and what's in flight, by that name
        naming: std::cell::Cell<&'static str>,
        in_flight: RefCell<lbterm_extras::state::InFlight>,
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
        /// Over the tab menu: the turning arc on a Conductor item that waits (render/conductor.rs)
        menu_spinner: RefCell<MenuSpinner>,
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
        /// (render/strip_swap.rs), and which swap it shows
        swap: IDCompositionVisual2,
        swap_layer: RefCell<(StripCard, u64)>,
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
            unsafe {
                // No rendering device: the visuals' content is swapchains,
                // which DComp doesn't draw into itself
                let device: IDCompositionDesktopDevice =
                    DCompositionCreateDevice3(None::<&IUnknown>)
                        .context("DCompositionCreateDevice3")?;
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
                let lights = device.CreateVisual()?;
                let readout = device.CreateVisual()?;
                let swap = device.CreateVisual()?;
                let panel_lights = device.CreateVisual()?;
                let cards = device.CreateVisual()?;
                let nothing = device.CreateRectangleClip()?;
                cards.SetClip(&nothing)?;
                // With no reference visual, FALSE puts each one on top
                for visual in [
                    &cards, &under, &panes, &focus, &over, &swap, &lights, &hover, &strip_card, &readout, &panel, &panel_lights, &top,
                ] {
                    root.AddVisual(visual, false, None::<&IDCompositionVisual>)
                        .context("AddVisual")?;
                }
                device.Commit().context("Commit")?;
                Ok(Self {
                    device,
                    naming: std::cell::Cell::new("other"),
                    in_flight: RefCell::new(Default::default()),
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
                    menu_spinner: RefCell::new(MenuSpinner::default()),
                    lights,
                    light_layers: RefCell::new(LightLayers::default()),
                    readout,
                    readout_layer: RefCell::new(StripCard::default()),
                    panel_lights,
                    swap,
                    swap_layer: RefCell::new((StripCard::default(), 0)),
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
            self.record(start, segments.end);
            Ok(Some(Tween {
                start,
                end: segments.end,
                motion,
            }))
        }

        /// What the animations started from here on are, for the state file: a name from
        /// lbterm/STATE.md's list
        fn name(&self, name: &'static str) {
            self.naming.set(name);
        }

        /// An animation started under the current name, from `start` for `seconds`
        fn record(&self, start: Instant, seconds: f64) {
            if crate::state_probe::on() {
                let end = start + Duration::from_secs_f64(seconds.max(0.0));
                self.in_flight.borrow_mut().start(self.naming.get(), start, end);
            }
        }

        /// What DWM is playing at `now`, by name (lbterm/STATE.md)
        pub fn animations(&self, now: Instant) -> Vec<lbterm_extras::state::Animation> {
            self.in_flight.borrow_mut().at(now)
        }

        /// When the last of what DWM is playing ends
        pub fn settles(&self) -> Option<Instant> {
            self.in_flight.borrow().settles()
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
            self.record(Instant::now(), segments.end);
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

        /// Make the tree match this frame: `over` the size of the window,
        /// the tab panel as `panel` says, and a visual per pane in `panes`
        /// at its rectangle (pressed to the right while the panel is open).
        /// When the rectangles changed and motion is on, the panes that
        /// moved, arrived or left get their animations here, all starting
        /// in the same commit. Pane surfaces take `base`'s format and
        /// present mode.
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
        ) -> anyhow::Result<()> {
            let now = Instant::now();
            let mut layers = self.layers.borrow_mut();
            let layers = &mut *layers;

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
                let exiting = layers.exiting.iter_mut().map(|(_, p, _)| p);
                let spare = layers.spare.iter_mut();
                for pane in layers.panes.iter_mut().map(|(_, p)| p).chain(exiting).chain(spare) {
                    pane.refit(card_layers, now)?;
                }
                if let Some(focus) = &mut layers.focus {
                    focus.refit(card_layers, now)?;
                }
                layers.changed = true;
            }
            if layers.focus.is_none() {
                layers.focus = Some(FocusOutline::new(self, card_layers)?);
                layers.changed = true;
            }

            match &mut layers.over {
                Some(over) => over.place(device, window)?,
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
                    Some(layer) => layer.place(device, window)?,
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

            match (panel, &mut layers.panel) {
                (Some(plan), None) => {
                    layers.panel = Some(PanelLayers::new(self, instance, device, base, plan)?);
                    layers.changed = true;
                }
                (Some(plan), Some(layer)) => {
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
                        // A new tab bar height, DPI or setting: no motion
                        layer.place(device, plan)?;
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
                if !layer.hidden && layer.sync_spinners(self, device, plan)? {
                    layers.changed = true;
                }
                if !layer.hidden && layer.sync_rings(self, device, plan)? {
                    layers.changed = true;
                }
                // LBTerm: the tabs' rows and the open tab's marker, moving as tabs come, go and
                // change places (render/tab_panel.rs); straight there while the window resizes
                if !layer.hidden && layer.sync_rows(self, plan, motion.enabled && !resized, now)? {
                    layers.changed = true;
                }
            }

            let exiting = std::mem::take(&mut layers.exiting);
            for (id, pane, done) in exiting {
                if resized || done <= now {
                    self.retire(layers, pane)?;
                } else {
                    layers.exiting.push((id, pane, done));
                }
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
                lbterm_motion::plan(&old, &new)
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
                let mut pane = match gone.iter().position(|(pane, _)| *pane == id) {
                    Some(i) => gone.swap_remove(i).1,
                    None => {
                        // Back before it finished fading out: pick it up
                        // where it is, otherwise a spare or a new visual
                        let back = layers.exiting.iter().position(|(pane, ..)| *pane == id);
                        let pane = match back {
                            Some(i) => {
                                let (_, pane, _) = layers.exiting.swap_remove(i);
                                unsafe { self.panes.RemoveVisual(&pane.container)? };
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
                        pane.fade(self, alpha, 1.0, &motion.enter)?;
                        layers.changed = true;
                    }
                    _ => {
                        // Unchanged (or still heading where it's going), a
                        // tab switch, a resize, or motion off
                        if pane.layer.rect != rect
                            || pane.shown != shown
                            || (!motion.enabled && pane.alpha.is_some())
                        {
                            pane.place(device, rect, shown)?;
                            layers.changed = true;
                        }
                    }
                }
                // 85% without the keyboard: dimming as it loses it, at once
                // for a pane that wasn't on screen (a split, a tab switch)
                let level = lbterm_motion::dim_level(Some(id) == active, lbterm_panel::palette::INACTIVE);
                if (level - pane.level).abs() > f64::EPSILON || !was_on_screen.contains(&id) {
                    let animate = motion.enabled && !resized && was_on_screen.contains(&id);
                    pane.dim_to(self, level, animate.then_some(&motion.dim), now)?;
                    layers.changed = true;
                }
                layers.panes.push((id, pane));
            }

            // The outline: onto the pane with the keyboard, gliding there
            // when the keyboard moved, following it when its pane moves
            let new_ids: Vec<PaneId> = panes.iter().map(|(id, ..)| *id).collect();
            let target = |id: PaneId| panes.iter().find(|(p, ..)| *p == id).map(|(_, _, shown)| *shown);
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
                        focus.place(id, shown)?;
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
            layers.active = active;

            for (id, mut pane) in gone {
                let exits = changes
                    .iter()
                    .any(|c| matches!(c, Change::Exit { id: gone_id, .. } if *gone_id == id));
                if exits {
                    // Under the panes growing over where it was
                    unsafe {
                        self.panes.RemoveVisual(&pane.container)?;
                        self.panes
                            .AddVisual(&pane.container, true, None::<&IDCompositionVisual>)?;
                    }
                    let done = pane.fade_out(self, &motion.exit, now)?;
                    layers.exiting.push((id, pane, done));
                    layers.changed = true;
                } else {
                    self.retire(layers, pane)?;
                }
            }
            Ok(())
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
        pub fn acquire(&self) -> anyhow::Result<Vec<Frame>> {
            let mut layers = self.layers.borrow_mut();
            let mut frames = vec![];
            if let Some(cards) = &*self.card_layers.borrow() {
                cards.frames(&mut frames)?;
            }
            if let Some(over) = &layers.over {
                frames.push(Frame {
                    target: Target::Over,
                    rect: over.rect,
                    texture: over.surface.get_current_texture()?,
                });
            }
            // LBTerm: the tab menu, while it's open and once more to clear it
            if layers.top_wanted || layers.top_shown {
                if let Some(top) = &layers.top {
                    frames.push(Frame {
                        target: Target::Top,
                        rect: top.rect,
                        texture: top.surface.get_current_texture()?,
                    });
                }
            }
            layers.top_shown = layers.top_wanted;
            let layers = &*layers;
            if let Some(panel) = layers.panel.as_ref().filter(|p| !p.hidden) {
                panel.frames(&mut frames)?;
            }
            for (id, pane) in &layers.panes {
                frames.push(Frame {
                    target: Target::Pane(*id),
                    rect: pane.layer.rect,
                    texture: pane.layer.surface.get_current_texture()?,
                });
            }
            Ok(frames)
        }

        /// Whether the panes show anywhere but where they are: the panel is
        /// pressing them over. The mouse can't be trusted to land where it
        /// looks until it stops.
        pub fn panes_pressed(&self) -> bool {
            self.layers.borrow().squeezed
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
            unsafe { self.device.Commit() }.context("IDCompositionDevice::Commit")
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
        pub fn acquire_hover(&self) -> anyhow::Result<Vec<Frame>> {
            let hover = self.hover_layers.borrow();
            if !hover.wanted {
                return Ok(vec![]);
            }
            let mut frames = vec![];
            for (i, slot) in hover.slots.iter().enumerate() {
                frames.push(Frame {
                    target: Target::Hover(i),
                    rect: slot.layer.rect,
                    texture: slot.layer.surface.get_current_texture()?,
                });
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
        /// waits: a template in a group clipped to nothing, and a visual in `top`, over the menu,
        /// that shows it turning, a turn every SPIN_PERIOD, in DWM; still with motion off.
        pub fn prepare_menu_spinner(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            rect: Option<PixelRect>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let mut spin = self.menu_spinner.borrow_mut();
            let Some(rect) = rect else {
                if spin.wanted {
                    if let Some(opacity) = &spin.opacity {
                        unsafe { opacity.SetOpacity2(0.0)? };
                    }
                    spin.wanted = false;
                    self.layers.borrow_mut().changed = true;
                }
                return Ok(());
            };
            if spin.template.is_none() {
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
                    self.top.AddVisual(&group, false, None::<&IDCompositionVisual>)?;
                    group.AddVisual(&held, false, None::<&IDCompositionVisual>)?;
                    self.top.AddVisual(&visual, false, None::<&IDCompositionVisual>)?;
                    visual.SetTransform(&rotate)?;
                }
                if motion.enabled {
                    let animation = unsafe { self.device.CreateAnimation()? };
                    unsafe {
                        animation.AddCubic(0.0, 0.0, (360.0 / SPIN_PERIOD) as f32, 0.0, 0.0)?;
                        animation.AddRepeat(SPIN_PERIOD, SPIN_PERIOD)?;
                        rotate.SetAngle(&animation)?;
                    }
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
                self.layers.borrow_mut().changed = true;
            }
            Ok(())
        }

        /// LBTerm: this frame's texture for the menu's turning arc, while an item waits
        pub fn acquire_menu_spinner(&self) -> anyhow::Result<Vec<Frame>> {
            let spin = self.menu_spinner.borrow();
            match (&spin.template, spin.wanted) {
                (Some(template), true) => Ok(vec![Frame {
                    target: Target::MenuSpinner,
                    rect: template.rect,
                    texture: template.surface.get_current_texture()?,
                }]),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: this frame's texture for the strip's detail card, while it shows one
        pub fn acquire_strip_card(&self) -> anyhow::Result<Vec<Frame>> {
            let strip = self.strip_card_layer.borrow();
            match (&strip.layer, strip.wanted) {
                (Some(layer), true) => Ok(vec![Frame {
                    target: Target::StripCard,
                    rect: layer.rect,
                    texture: layer.surface.get_current_texture()?,
                }]),
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
                    let parent = if lit.over_panel { &self.panel_lights } else { &self.lights };
                    unsafe { parent.RemoveVisual(&lit.visual)? };
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
                    Light::Pad { rect, .. } => rect.h,
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
                    Light::Press { x, y, reach, start } => {
                        self.name("light.press");
                        let mut lit = take(&mut layers.spare)?;
                        lit.over_panel = false;
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
                        drive(grow(reach - start, 0.0), &|a| unsafe { clip.SetLeft(a) }, &|v| unsafe { clip.SetLeft2(v) })?;
                        drive(grow(reach - start, 0.0), &|a| unsafe { clip.SetTop(a) }, &|v| unsafe { clip.SetTop2(v) })?;
                        drive(grow(reach + start, 2.0 * reach), &|a| unsafe { clip.SetRight(a) }, &|v| unsafe { clip.SetRight2(v) })?;
                        drive(grow(reach + start, 2.0 * reach), &|a| unsafe { clip.SetBottom(a) }, &|v| unsafe { clip.SetBottom2(v) })?;
                        for corner in Corner::ALL {
                            drive(grow(start, reach), &|a| corner.animate(clip, a), &|v| corner.set(clip, v))?;
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
                        unsafe { self.lights.AddVisual(&lit.visual, false, None::<&IDCompositionVisual>)? };
                        layers.shown.push((lit, now + Duration::from_secs_f64(end)));
                    }
                    Light::Text { rects, radius } => {
                        self.name("light.copy");
                        for rect in rects {
                            let mut lit = take(&mut layers.spare)?;
                            lit.over_panel = false;
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
                        lit.over_panel = true;
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
                }
            }
            self.layers.borrow_mut().changed = true;
            Ok(())
        }

        /// LBTerm: the lights' swatch, in a frame only when it's new or a new size: it's
        /// cleared to white (render/composited.rs)
        pub fn acquire_lights(&self) -> anyhow::Result<Vec<Frame>> {
            let mut layers = self.light_layers.borrow_mut();
            if !layers.draw {
                return Ok(vec![]);
            }
            layers.draw = false;
            match &layers.swatch {
                Some(swatch) => Ok(vec![Frame {
                    target: Target::Swatch,
                    rect: swatch.rect,
                    texture: swatch.surface.get_current_texture()?,
                }]),
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
        pub fn acquire_readout(&self) -> anyhow::Result<Vec<Frame>> {
            let card = self.readout_layer.borrow();
            match (&card.layer, card.wanted) {
                (Some(layer), true) => Ok(vec![Frame {
                    target: Target::Readout,
                    rect: layer.rect,
                    texture: layer.surface.get_current_texture()?,
                }]),
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
        /// rectangle and which swap it is. A new one shows at once and fades out on `swap`;
        /// `None`, or motion off, puts it away. Made the first time; call after `prepare`.
        pub fn prepare_swap(
            &self,
            instance: &wgpu::Instance,
            device: &wgpu::Device,
            base: &wgpu::SurfaceConfiguration,
            swap: Option<(PixelRect, u64)>,
            motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            let mut state = self.swap_layer.borrow_mut();
            let (card, shown) = &mut *state;
            let (rect, generation) = match swap.filter(|_| motion.enabled) {
                Some(swap) => swap,
                None => {
                    if card.wanted {
                        card.settle(0.0)?;
                        card.wanted = false;
                        self.layers.borrow_mut().changed = true;
                    }
                    if let Some((_, generation)) = swap {
                        *shown = generation;
                    }
                    return Ok(());
                }
            };
            if generation == *shown {
                return Ok(());
            }
            match card.layer.as_mut() {
                Some(layer) => layer.place(device, rect)?,
                None => {
                    let visual = unsafe { self.device.CreateVisual()? };
                    unsafe { self.swap.AddVisual(&visual, false, None::<&IDCompositionVisual>)? };
                    let layer = Layer::new(
                        instance,
                        device,
                        visual,
                        base,
                        wgpu::CompositeAlphaMode::PreMultiplied,
                        rect,
                    )?;
                    card.opacity = Some(
                        layer
                            .visual
                            .cast()
                            .context("IDCompositionVisual3 (Windows 10 or later)")?,
                    );
                    card.layer = Some(layer);
                }
            }
            // There at once with its first frame, then fading
            let opacity = card.opacity.clone().context("the swap's visual")?;
            self.name("strip.swap");
            card.alpha = self.drive(
                Motion::new(1.0, 0.0, 0.0, motion.swap.clone()),
                OPACITY_EPSILON,
                0.0,
                |a| unsafe { opacity.SetOpacity(a) },
                |v| unsafe { opacity.SetOpacity2(v) },
            )?;
            card.rest = 0.0;
            card.wanted = true;
            *shown = generation;
            self.layers.borrow_mut().changed = true;
            Ok(())
        }

        /// LBTerm: this frame's texture for the strip as it was, while it fades
        pub fn acquire_swap(&self) -> anyhow::Result<Vec<Frame>> {
            let state = self.swap_layer.borrow();
            match (&state.0.layer, state.0.wanted) {
                (Some(layer), true) => Ok(vec![Frame {
                    target: Target::Swap,
                    rect: layer.rect,
                    texture: layer.surface.get_current_texture()?,
                }]),
                _ => Ok(vec![]),
            }
        }

        /// LBTerm: whether swap `generation` still shows at `now`: new, or still fading
        pub fn swap_showing(&self, generation: u64, now: Instant) -> bool {
            let state = self.swap_layer.borrow();
            let (card, shown) = &*state;
            generation != *shown
                || (card.wanted && card.alpha.as_ref().map_or(false, |t| t.finishes() > now))
        }
    }

    /// LBTerm: one brief light (render/lights.rs): a visual showing the white swatch through a
    /// rounded clip, its opacity the light.
    struct Lit {
        visual: IDCompositionVisual3,
        clip: IDCompositionRectangleClip,
        /// Over the tab panel (a pad's glint), not under it
        over_panel: bool,
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
                over_panel: false,
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
        /// It shows this frame
        wanted: bool,
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
}

#[cfg(not(windows))]
mod imp {
    use super::{CardPlan, Frame, Keyboard, Light, MotionSettings, PanelPlan};
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
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn prepare_keyboard(&self, _keyboard: Keyboard, _motion: &MotionSettings) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire(&self) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn panes_pressed(&self) -> bool {
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

        pub fn acquire_hover(&self) -> anyhow::Result<Vec<Frame>> {
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

        pub fn acquire_strip_card(&self) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn prepare_menu_spinner(
            &self,
            _instance: &wgpu::Instance,
            _device: &wgpu::Device,
            _base: &wgpu::SurfaceConfiguration,
            _rect: Option<PixelRect>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_menu_spinner(&self) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn strip_card_showing(&self, _now: std::time::Instant) -> bool {
            match *self {}
        }

        pub fn strip_card_settles(&self) -> Option<std::time::Instant> {
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

        pub fn acquire_lights(&self) -> anyhow::Result<Vec<Frame>> {
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

        pub fn acquire_readout(&self) -> anyhow::Result<Vec<Frame>> {
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
            _swap: Option<(PixelRect, u64)>,
            _motion: &MotionSettings,
        ) -> anyhow::Result<()> {
            match *self {}
        }

        pub fn acquire_swap(&self) -> anyhow::Result<Vec<Frame>> {
            match *self {}
        }

        pub fn swap_showing(&self, _generation: u64, _now: std::time::Instant) -> bool {
            match *self {}
        }

        pub fn animations(&self, _now: std::time::Instant) -> Vec<lbterm_extras::state::Animation> {
            match *self {}
        }

        pub fn settles(&self) -> Option<std::time::Instant> {
            match *self {}
        }
    }
}

pub use imp::Compositor;
