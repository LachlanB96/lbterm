//! LBTerm: thumbnails of the images a pane links to (lbterm/THUMBNAILS.md; the links and how the
//! pictures lie in their rows are lbterm_extras::thumbnails, the decoding crate::thumbnails). Each
//! paint of a pane notes the runs of cells that link to a picture row as its lines go by
//! (`lbterm_thumbnail_runs`), and after the lines draws each picture into the rows it covers, over
//! the blank cells there, in the pane's own layers so it moves with the pane: the image decoded
//! for its box, and a hairline round it that brightens while the pointer's on one of its links. A
//! file that's gone has its box drawn grey and its caption struck through (`lbterm_thumbnail_link`).
//!
//! A picture keeps its place in the window's atlas: a new decoding of its file at the same size is
//! written over the last one there (`place`), never put in a place of its own, so a file rewritten
//! every few seconds (a dashboard's live tile) takes one place for as long as it's drawn. Through
//! the glyph cache's `cached_image`, as until 2026-11-01, each decoding took a new place, and the
//! atlas filled and was made again (every glyph drawn again) every few minutes of a live tile.

use crate::glyphcache::GlyphCache;
use crate::quad::{QuadTrait, TripleLayerQuadAllocator, TripleLayerQuadAllocatorTrait};
use crate::termwindow::render::paint::AllowImage;
use crate::termwindow::TermWindow;
use crate::thumbnails::Thumb;
use ::window::bitmaps::atlas::Sprite;
use ::window::bitmaps::{BitmapImage, Image, Texture2d, TextureCoord, TextureRect, TextureSize};
use anyhow::Context;
use config::HsbTransform;
use lbterm_extras::state;
use lbterm_extras::thumbnails::{self, Run};
use lbterm_panel::palette;
use mux::pane::PaneId;
use mux::tab::PositionedPane;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use termwiz::hyperlink::Hyperlink;
use wezterm_term::Line;
use window::color::LinearRgba;

fn srgb(rgb: u32) -> LinearRgba {
    let (r, g, b) = palette::bytes(rgb);
    LinearRgba::with_srgba(r, g, b, 255)
}

/// How long a picture no pane has drawn keeps its place: as long as the thumbnails' thread keeps
/// its decoding (crate::thumbnails' `KEEP`)
const PLACE_KEEP: Duration = Duration::from_secs(300);

/// A picture's file and its box in pixels
type PlaceKey = (String, u32, u32);

/// Where a picture lies in the window's atlas, and which decoding of its file is there
struct Placed {
    sprite: Sprite,
    version: u64,
    drawn: Instant,
    /// How many places it has taken (state::Thumbnail's `places`)
    places: u64,
}

/// What the window keeps of its thumbnails between paints
#[derive(Default)]
pub struct ThumbnailsState {
    /// `crate::thumbnails::gone_changes()` when the lines were last shaped
    gone_seen: usize,
    /// What the panes drew, for the state file (lbterm/STATE.md)
    said: Vec<state::Thumbnail>,
    /// Each picture's place in the atlas
    placed: HashMap<PlaceKey, Placed>,
}

/// The decoding of a picture its quad draws, as the quad's alt colour: written over its last place
/// in the atlas, a new decoding has quads like the last one's, and the compositor draws a visual
/// again only when its quads change (render/composited.rs, `lbterm_content`), so without this the
/// screen kept the old picture (perf/thumbnails.ps1 caught it). The shader mixes the alt colour in
/// by `mix_value`, 0 here, which takes none of it, so it changes the quad's hash and no pixel. The
/// version's low 32 bits, 12, 12 and 8 to a channel: whole numbers an f32 holds exactly.
fn version_colour(version: u64) -> LinearRgba {
    let v = version as u32;
    LinearRgba::with_components((v & 0xFFF) as f32, ((v >> 12) & 0xFFF) as f32, (v >> 24) as f32, 0.)
}

/// Whether two textures are the same one
fn same_texture(a: &Rc<dyn Texture2d>, b: &Rc<dyn Texture2d>) -> bool {
    std::ptr::eq(Rc::as_ptr(a) as *const (), Rc::as_ptr(b) as *const ())
}

/// The place in `cache`'s atlas that `image`, decoding `version` of the picture `key`, is drawn
/// from: the one it had, written over when this is a newer decoding of the same size, or a new
/// one when it had none in this atlas or its size changed. The atlas running out of room is
/// `OutOfTextureSpace`, as for any image, which the paint answers by making the atlas again.
fn place(
    placed: &mut HashMap<PlaceKey, Placed>,
    cache: &mut GlyphCache,
    key: PlaceKey,
    image: &Image,
    version: u64,
) -> anyhow::Result<Sprite> {
    let atlas = cache.atlas.texture();
    let size = image.image_dimensions();
    let had = placed.get_mut(&key).filter(|p| same_texture(&p.sprite.texture, &atlas));
    if let Some(p) = had {
        if (p.sprite.coords.size.width as usize, p.sprite.coords.size.height as usize) == size {
            if p.version != version {
                p.sprite.texture.write(p.sprite.coords, image);
                p.version = version;
            }
            p.drawn = Instant::now();
            return Ok(p.sprite.clone());
        }
    }
    let sprite = cache.atlas.allocate(image).context("atlas.allocate")?;
    let places = placed.get(&key).map_or(0, |p| p.places) + 1;
    placed.insert(key, Placed { sprite: sprite.clone(), version, drawn: Instant::now(), places });
    Ok(sprite)
}

/// The runs of `line`'s cells that link to a picture row, `row` being its row in the view
pub(crate) fn lbterm_thumbnail_runs(row: usize, line: &Line, out: &mut Vec<Run>) {
    if !line.has_hyperlink() {
        return;
    }
    let links: Vec<(usize, usize, Arc<Hyperlink>)> = line
        .visible_cells()
        .filter_map(|cell| {
            let link = cell.attrs().hyperlink()?;
            link.uri()
                .starts_with(thumbnails::LINK)
                .then(|| (cell.cell_index(), cell.width(), Arc::clone(link)))
        })
        .collect();
    if links.is_empty() {
        return;
    }
    out.extend(thumbnails::runs_in_row(row, links.iter().map(|(col, width, link)| (*col, *width, link.uri()))));
}

/// The part of `y0..y1` inside `lo..hi`
fn clip(y0: f32, y1: f32, lo: f32, hi: f32) -> Option<(f32, f32)> {
    let (a, b) = (y0.max(lo), y1.min(hi));
    (b > a).then_some((a, b))
}

impl TermWindow {
    /// Each paint, before anything's drawn: the lines holding a caption are shaped again when a
    /// file has gone or come back since this window last shaped them, the rest keeping their
    /// shapes (lbterm_extras::link_looks), and what the panes drew is forgotten. A place in an
    /// atlas made since, or of a picture no pane has drawn in [`PLACE_KEEP`], is let go (the
    /// atlas's own room comes back when it's next made), so no old atlas is kept alive by one
    pub(crate) fn lbterm_thumbnails_refresh(&mut self) {
        let gone = crate::thumbnails::gone_changes();
        if gone != self.lbterm_thumbnails.gone_seen {
            self.lbterm_thumbnails.gone_seen = gone;
            self.lbterm_link_states += 1;
        }
        self.lbterm_thumbnails.said.clear();
        if self.lbterm_thumbnails.placed.is_empty() {
            return;
        }
        match self.render_state.as_ref() {
            Some(rs) => {
                let atlas = rs.glyph_cache.borrow().atlas.texture();
                self.lbterm_thumbnails
                    .placed
                    .retain(|_, p| same_texture(&p.sprite.texture, &atlas) && p.drawn.elapsed() < PLACE_KEEP);
            }
            None => self.lbterm_thumbnails.placed.clear(),
        }
    }

    /// The colour a link's words are drawn in, and that they're struck through, when it's an
    /// image's caption whose file has gone; nothing for any other link
    pub(crate) fn lbterm_thumbnail_link(&self, link: &Hyperlink) -> Option<(LinearRgba, bool)> {
        if !link.uri().starts_with(thumbnails::LINK) {
            return None;
        }
        let image = thumbnails::parse(link.uri())?;
        (image.row.is_none() && crate::thumbnails::gone(&image.path)).then(|| (srgb(thumbnails::GONE_CAPTION), true))
    }

    /// Whether a link is a row of a picture: it isn't underlined under the pointer, since its
    /// cells can run past the picture, which says it's hovered by its hairline instead
    pub(crate) fn lbterm_thumbnail_row(&self, link: &Hyperlink) -> bool {
        link.uri().starts_with(thumbnails::LINK)
            && thumbnails::parse(link.uri()).is_some_and(|image| image.row.is_some())
    }

    /// The pictures `runs` make, drawn over the pane's lines: `top_pixel_y` and `left_pixel_x` are
    /// where the pane's first row and column start, as its lines are drawn
    pub(crate) fn lbterm_paint_thumbnails(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        pos: &PositionedPane,
        runs: &[Run],
        top_pixel_y: f32,
        left_pixel_x: f32,
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<()> {
        let pane_id: PaneId = pos.pane.pane_id();
        self.lbterm_thumbnails.said.retain(|t| t.pane != pane_id as u64);
        if runs.is_empty() {
            return Ok(());
        }
        let cell_w = self.render_metrics.cell_size.width as f32;
        let cell_h = self.render_metrics.cell_size.height as f32;
        let scale = self.dimensions.dpi as f32 / 96.0;
        let hairline = scale.round().max(1.0);
        let pad = (2.0 * scale).round();
        let row_y = |row: isize| (top_pixel_y + (pos.top as isize + row) as f32 * cell_h).round();
        let col_x = |col: usize| (left_pixel_x + col as f32 * cell_w).round();
        let hovered_path = self
            .current_highlight
            .as_ref()
            .and_then(|link| thumbnails::parse(link.uri()))
            .map(|link| link.path);

        for block in thumbnails::blocks(runs) {
            let x0 = col_x(block.cols.start);
            let box_w = col_x(block.cols.end) - x0;
            let y0 = row_y(block.top) + pad;
            let box_h = block.rows as f32 * cell_h - 2.0 * pad;
            if box_w < 1.0 || box_h < 1.0 {
                continue;
            }
            // The rows it's drawn in: those its links were found on
            let (lo, hi) = (row_y(block.shown.start as isize), row_y(block.shown.end as isize));
            let hovered = hovered_path.as_deref() == Some(block.path.as_str());

            let thumb = crate::thumbnails::get(&block.path, box_w as u32, box_h as u32);
            let key: PlaceKey = (block.path.clone(), box_w as u32, box_h as u32);
            let (state, drawn) = match &thumb {
                Thumb::Loading => ("loading", None),
                Thumb::Shown { .. } if self.allow_images == AllowImage::No => ("loading", None),
                Thumb::Shown { image, width, height, version } => {
                    let (w, h) = (*width as f32, *height as f32);
                    let drawn = self
                        .lbterm_thumbnail_image(layers, key.clone(), image, *version, (x0, y0, w, h), (lo, hi), hsv)
                        .context("lbterm_thumbnail_image")?;
                    if let Some((top, bottom)) = drawn {
                        let colour = srgb(if hovered { thumbnails::HAIRLINE_HOVER } else { thumbnails::HAIRLINE });
                        self.lbterm_thumbnail_outline(layers, (x0, y0, w, h), (top, bottom), hairline, colour, hsv)?;
                    }
                    ("shown", drawn.map(|(top, bottom)| (x0, top, w, bottom - top)))
                }
                Thumb::Gone | Thumb::Unreadable => {
                    let state = if matches!(thumb, Thumb::Gone) { "gone" } else { "unreadable" };
                    let drawn = clip(y0, y0 + box_h, lo, hi);
                    if let Some((top, bottom)) = drawn {
                        self.filled_rectangle(layers, 2, euclid::rect(x0, top, box_w, bottom - top), srgb(thumbnails::GONE_FILL))?
                            .set_hsv(hsv);
                        self.lbterm_thumbnail_outline(
                            layers,
                            (x0, y0, box_w, box_h),
                            (top, bottom),
                            hairline,
                            srgb(thumbnails::GONE_OUTLINE),
                            hsv,
                        )?;
                    }
                    (state, drawn.map(|(top, bottom)| (x0, top, box_w, bottom - top)))
                }
            };
            let (x, y, w, h) = drawn.unwrap_or((x0, lo, box_w, 0.0));
            let (version, places) = self
                .lbterm_thumbnails
                .placed
                .get(&key)
                .filter(|_| state == "shown")
                .map_or((0, 0), |p| (p.version, p.places));
            self.lbterm_thumbnails.said.push(state::Thumbnail {
                pane: pane_id as u64,
                path: block.path.clone(),
                state: state.to_string(),
                rect: state::Rect::round(x as f64, y as f64, w as f64, h as f64),
                hovered,
                version,
                places,
            });
        }
        Ok(())
    }

    /// `image`, decoding `version` of the picture `key`, drawn at `pic` (x, y, width, height in the
    /// window's pixels) from its place in the atlas ([`place`]), only the part of it between `lo`
    /// and `hi`: the top and bottom of what was drawn, or nothing when none of it is in view
    fn lbterm_thumbnail_image(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        key: PlaceKey,
        image: &Image,
        version: u64,
        (x, y, w, h): (f32, f32, f32, f32),
        (lo, hi): (f32, f32),
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<Option<(f32, f32)>> {
        let Some((top, bottom)) = clip(y, y + h, lo, hi) else {
            return Ok(None);
        };
        let gl_state = self.render_state.as_ref().unwrap();
        let sprite = place(
            &mut self.lbterm_thumbnails.placed,
            &mut gl_state.glyph_cache.borrow_mut(),
            key,
            image,
            version,
        )?;
        let (tex_w, tex_h) = (sprite.texture.width() as f32, sprite.texture.height() as f32);
        let (sprite_w, sprite_h) = (sprite.coords.size.width as f32, sprite.coords.size.height as f32);
        let (v0, v1) = ((top - y) / h, (bottom - y) / h);
        let texture = TextureRect::new(
            TextureCoord::new(
                sprite.coords.origin.x as f32 / tex_w,
                (sprite.coords.origin.y as f32 + v0 * sprite_h) / tex_h,
            ),
            TextureSize::new(sprite_w / tex_w, (v1 - v0) * sprite_h / tex_h),
        );
        let (half_w, half_h) = (self.dimensions.pixel_width as f32 / 2.0, self.dimensions.pixel_height as f32 / 2.0);
        let mut quad = layers.allocate(2)?;
        quad.set_position(x - half_w, top - half_h, x + w - half_w, bottom - half_h);
        quad.set_texture(texture);
        quad.set_fg_color(LinearRgba::with_components(1.0, 1.0, 1.0, 1.0));
        quad.set_alt_color_and_mix_value(version_colour(version), 0.);
        quad.set_has_color(true);
        quad.set_hsv(hsv);
        Ok(Some((top, bottom)))
    }

    /// A hairline `stroke` wide inside the edge of `area` (x, y, width, height), only the part
    /// between `top` and `bottom`
    fn lbterm_thumbnail_outline(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        (x, y, w, h): (f32, f32, f32, f32),
        (top, bottom): (f32, f32),
        stroke: f32,
        colour: LinearRgba,
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<()> {
        let mut rect = |rx: f32, ry0: f32, rw: f32, ry1: f32| -> anyhow::Result<()> {
            if let Some((a, b)) = clip(ry0, ry1, top, bottom) {
                self.filled_rectangle(layers, 2, euclid::rect(rx, a, rw, b - a), colour)?.set_hsv(hsv);
            }
            Ok(())
        };
        rect(x, y, w, y + stroke)?;
        rect(x, y + h - stroke, w, y + h)?;
        rect(x, y + stroke, stroke, y + h - stroke)?;
        rect(x + w - stroke, y + stroke, stroke, y + h - stroke)?;
        Ok(())
    }
}

/// LBTerm: the thumbnails, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// What the panes drew in the last paint
    pub(crate) fn lbterm_state_thumbnails(&self) -> Vec<state::Thumbnail> {
        self.lbterm_thumbnails.said.clone()
    }
}
