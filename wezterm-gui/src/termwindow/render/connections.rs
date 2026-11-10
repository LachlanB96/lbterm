//! LBTerm: the glow of a suggested tab connection (lbterm/LINKS.md, "Connections"). The program
//! that keeps the links file suggests two tabs whose sessions should be talking to each other; the
//! rail draws the line between them in faint cyan with hollow marks (render/tab_panel.rs, the
//! rows' pieces), and with the compositor a light runs to and fro along it, carried by DWM as a
//! tab link drag's lamps are (compositor.rs, `ConnectionLamps`), so it costs LBTerm no frames
//! while it runs: three laps when the suggestion comes onto the rail, when the file says something
//! new of it and when the pointer comes onto the rail, and between them still, glowing at the
//! line's middle (lbterm_extras::glow, Lachlan's 1a). The light is drawn once into a template in
//! z-layer 97 (`Target::ConnectionLamp`), never seen itself; a visual per line over the rail shows
//! it. With motion off it sits still at the line's middle; without the compositor there's no
//! light and the line is dashed, as a suggested link's is.

use crate::quad::TripleLayerQuadAllocator;
use crate::termwindow::compositor::{GlowLine, GlowPlan};
use crate::termwindow::render::tab_panel::{light_bands, Band};
use crate::termwindow::TermWindow;
use lbterm_extras::links::{glow_path, Look};
use lbterm_motion::content::pair;
use lbterm_motion::PixelRect;
use lbterm_panel::{palette, Layout};

/// The light's template, with the compositor: drawn while a suggestion shows, and shown along each
/// suggested line
pub const LAMP_Z: i8 = 97;

/// A key of what the file says of a suggestion: when it changes, its light runs its laps again
/// (lbterm_extras::glow). Its members, its why, who found it and when, and when it came to stand
/// so; not its note or messages, which a suggestion has none of
fn says(c: &lbterm_extras::links::Connection) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&c.members, &c.why, &c.by, c.at, c.state_at).hash(&mut h);
    h.finish()
}

/// How far the light reaches across its line and along it, either side of its middle, at 96 DPI
const REACH_ACROSS: f32 = 3.0;
const REACH_ALONG: f32 = 9.0;
/// Its alpha at its middle
const PEAK: f64 = 0.95;

impl TermWindow {
    /// The lights along this paint's suggested connections, with the compositor: the template
    /// drawn into its z-layer and where each line's light goes (none without a suggestion on this
    /// rail with two members or more in view). Called by the tab panel's paint once its layout is
    /// known
    pub(crate) fn tab_panel_connection_glow(&mut self, layout: &Layout) -> anyhow::Result<()> {
        let scale = self.dimensions.dpi as f64 / 96.0;
        let said = self.lbterm_panel.links.said.clone();
        let lines: Vec<(f32, f32, f32, String, u64)> = self
            .lbterm_panel
            .links
            .joins
            .iter()
            .filter(|j| j.look == Look::ConnectionSuggested)
            .filter_map(|j| {
                let shown: Vec<usize> = j.rows.iter().copied().filter(|&r| layout.row_visible(r)).collect();
                let c = said.connections.get(j.connection)?;
                (shown.len() >= 2).then(|| {
                    let top = layout.link_end(shown[0]).1;
                    let bottom = layout.link_end(shown[shown.len() - 1]).1;
                    (layout.link_lane(j.lane), top, bottom, c.id.clone(), says(c))
                })
            })
            .collect();
        let Some(&(x0, y0, ..)) = lines.first() else {
            self.lbterm_panel.glow = None;
            return Ok(());
        };
        let across = (REACH_ACROSS * scale as f32).round().max(1.0);
        let along = (REACH_ALONG * scale as f32).round().max(2.0);
        // Drawn round the first line's top end: its visual is never seen, so where doesn't matter
        // but that it's in the window
        let (cx, cy) = (x0.round(), y0.round());
        let template = PixelRect::new((cx - across) as i32, (cy - along) as i32, (2.0 * across) as u32, (2.0 * along) as u32);
        // Made again only when it moves or changes size, its layer kept as it is otherwise
        // (render/kept.rs): z-layer 97 is this template's alone (the parked feat/new-tab takes
        // 93 to 96)
        let key = self.lbterm_template_key(&[pair(cx, cy), pair(across, along)]);
        self.lbterm_kept_layer(LAMP_Z, "connection-light", key, |quads| {
            let bands = light_bands(palette::CONNECTION_LIGHT, along, 8, |d| {
                let t = (1.0 - d / along as f64).clamp(0.0, 1.0);
                PEAK * t * t
            });
            self.lbterm_connection_light(quads, (cx, cy), across / along, &bands)
        })?;
        let size = (template.w, template.h);
        self.lbterm_panel.glow = Some(GlowPlan {
            template,
            lines: lines
                .into_iter()
                .map(|(x, top, bottom, id, says)| {
                    let (lap, path) = glow_path(x, (top, bottom), size, scale);
                    GlowLine { id, says, lap, path }
                })
                .collect(),
        });
        Ok(())
    }

    /// The light: `bands` out from `centre` along the line, `across` as far across it, an ellipse
    /// of wedges as render/tab_panel.rs's discs are
    fn lbterm_connection_light(
        &self,
        quads: &mut TripleLayerQuadAllocator,
        centre: (f32, f32),
        across: f32,
        bands: &[Band],
    ) -> anyhow::Result<()> {
        use std::f32::consts::TAU;
        const WEDGES: usize = 32;
        let (half_w, half_h) = (
            self.dimensions.pixel_width as f32 / 2.0,
            self.dimensions.pixel_height as f32 / 2.0,
        );
        let at = |radius: f32, angle: f32| {
            [centre.0 + radius * across * angle.cos() - half_w, centre.1 + radius * angle.sin() - half_h]
        };
        for &(near, inner, far, outer) in bands {
            for i in 0..WEDGES {
                let a0 = TAU * i as f32 / WEDGES as f32;
                let a1 = TAU * (i + 1) as f32 / WEDGES as f32;
                let mut q = self.filled_rectangle(quads, 0, euclid::rect(0.0, 0.0, 0.0, 0.0), inner)?;
                q.set_corners([at(near, a0), at(near, a1), at(far, a0), at(far, a1)], [inner, inner, outer, outer]);
            }
        }
        Ok(())
    }

    /// The lights along the suggested connections, for the compositor (render/composited.rs)
    pub(crate) fn lbterm_connection_glow(&self) -> Option<GlowPlan> {
        self.lbterm_panel.glow.clone()
    }

    /// The pointer came onto the rail (render/tab_panel.rs): every suggested line's light that's
    /// still runs its laps again (lbterm_extras::glow). Whether that owes laps not owed before
    /// that can play, so a frame should come to start them: none with motion off or the window
    /// unseen, where a frame would start nothing (lbterm/PITFALLS.md, trap 66)
    pub(crate) fn lbterm_connection_lights_owed(&self) -> bool {
        self.webgpu
            .as_ref()
            .and_then(|w| w.compositor.as_ref())
            .map_or(false, |c| c.connection_lamps_pointer())
    }
}
