//! LBTerm: the words of the cards, menus and lists LBTerm draws itself, laid out once while they
//! show (lbterm/PERF.md, "What the cards cost"). compute_element shapes a word with HarfBuzz and
//! looks up each of its glyphs every time it's asked, with no cache, and every card asked again
//! for each of its words on every frame it was open, so a card open while a Claude session
//! streamed in the open tab shaped its words 60 to 120 times a second unchanged. Here each card
//! keeps its last frame's words ([`Words`], lbterm_extras::kept) by what each was laid out from,
//! and a frame lays out only those that are new: a row whose count changed, a highlighted row's
//! words drawn again over its highlight. Their glyphs' quads are kept too (render/kept_quads.rs),
//! made again only when a word is new, gone or moved, and copied into the card's z-layer each
//! frame in one go, which the compositor's look at what changed needs (render/composited.rs):
//! render_element made a layer's allocator for every word, three mappings of its vertex buffers,
//! and each of its glyphs' quads again.

use crate::quad::{QuadTrait, TripleLayerQuadAllocator, TripleLayerQuadAllocatorTrait};
use crate::termwindow::box_model::{
    BorderColor, ComputedElement, ComputedElementContent, Element, ElementCell, ElementColors, ElementContent,
    InheritableColor, LayoutContext,
};
use crate::termwindow::render::kept_quads::{KeptLayers, KeptQuads};
use crate::utilsprites::RenderMetrics;
use ::window::RectF;
use config::DimensionContext;
use lbterm_extras::kept::{Kept, WORDS};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use wezterm_font::LoadedFont;
use window::color::LinearRgba;

/// What a word was laid out from, besides what's in [`WordsAt`]: compute_element's whole input
/// for a run of text in one colour
#[derive(Hash, PartialEq, Eq)]
pub(crate) struct WordKey {
    text: String,
    colour: [u32; 4],
    /// Where it starts and the room it has, as f32 bits: compute_element cuts what doesn't fit,
    /// and a box-drawing character's room is measured from the window's edge, so where matters
    bounds: [u32; 4],
    zindex: i8,
    /// The font, by its address (a reload or a new scale makes another, and the shape generation
    /// goes up with it), and the cell its lines are measured in
    font: usize,
    cell: [u32; 2],
}

/// What every word kept was laid out at: the shape generation (a reload, a font fallback, a new
/// glyph atlas), the DPI, and the window's size (compute_element cuts a word at its edge)
pub(crate) type WordsAt = (usize, usize, usize, usize);

/// A card's words as its last frame laid them out, and their glyphs' quads as it drew them
#[derive(Default)]
pub(crate) struct Words {
    laid: Kept<WordsAt, WordKey, ComputedElement>,
    quads: KeptQuads,
}

impl Words {
    /// This frame's words are laid out at `at` (lbterm_extras::kept::Kept::at)
    pub(crate) fn at(&mut self, at: WordsAt) {
        self.laid.at(at);
    }

    /// This frame's words, drawn, kept for the next
    pub(crate) fn keep(&mut self, words: impl IntoIterator<Item = (WordKey, ComputedElement)>) {
        self.laid.keep(words, &WORDS);
    }

    /// Nothing kept: the glyph atlas they point into has gone (gpu_lost.rs)
    pub(crate) fn forget(&mut self) {
        self.laid.forget();
        self.quads.forget();
    }
}

/// One word to draw: its text, where it goes and the room it has, its colour
pub(crate) type Word = (String, RectF, LinearRgba);

/// A word's colour as plain text has it: the text's, nothing behind it
fn text_colors(colour: LinearRgba) -> ElementColors {
    ElementColors {
        border: BorderColor::default(),
        bg: LinearRgba::TRANSPARENT.into(),
        text: colour.into(),
    }
}

/// A hash of a frame's words in order: the same words in the same places draw the same quads
fn words_hash<'a>(keys: impl Iterator<Item = &'a WordKey>) -> u64 {
    let mut hasher = DefaultHasher::new();
    for key in keys {
        key.hash(&mut hasher);
    }
    hasher.finish()
}

impl crate::TermWindow {
    /// Every card's words forgotten: the glyph atlas they point into has gone (gpu_lost.rs)
    pub(crate) fn lbterm_forget_words(&mut self) {
        for words in &mut self.lbterm_objectives.words {
            words.get_mut().forget();
        }
        self.lbterm_strip.forget_card_words();
        self.lbterm_inbox.forget_words();
        self.lbterm_agenda.forget_words();
        self.lbterm_context.forget_words();
        self.lbterm_tasks.words.get_mut().forget();
        self.lbterm_menu.words.get_mut().forget();
        self.lbterm_drafts.words.get_mut().forget();
        self.lbterm_questions.words.get_mut().forget();
        self.lbterm_new_tab.forget_words();
    }

    /// What the words drawn this frame are laid out at ([`WordsAt`])
    pub(crate) fn lbterm_words_at(&self) -> WordsAt {
        (
            self.shape_generation,
            self.dimensions.dpi,
            self.dimensions.pixel_width,
            self.dimensions.pixel_height,
        )
    }

    /// `words` in `font` at `zindex`, laid out in `cell` (a column's width and a line's height)
    /// and `metrics`, drawn: each laid out only when `kept` doesn't have it from the last frame,
    /// their quads made only when a word is new, gone or moved, and kept for the next.
    pub(crate) fn lbterm_draw_words(
        &self,
        kept: &RefCell<Words>,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        cell: (f32, f32),
        zindex: i8,
        words: impl IntoIterator<Item = Word>,
    ) -> anyhow::Result<()> {
        let mut kept = kept.borrow_mut();
        kept.at(self.lbterm_words_at());
        let drawn = words
            .into_iter()
            .map(|word| self.lbterm_take_word(&mut kept, font, metrics, cell, zindex, word))
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.lbterm_draw_taken(&mut kept, zindex, &drawn)?;
        kept.keep(drawn);
        Ok(())
    }

    /// Words taken this frame (`lbterm_take_word`) into z-layer `zindex`: their quads `kept`'s
    /// while they're the same words in the same places, else made again
    pub(crate) fn lbterm_draw_taken(&self, kept: &mut Words, zindex: i8, drawn: &[(WordKey, ComputedElement)]) -> anyhow::Result<()> {
        let from = words_hash(drawn.iter().map(|(key, _)| key));
        self.lbterm_kept_quads_in(&mut kept.quads, zindex, from, |quads| {
            self.lbterm_render_words_into(drawn.iter().map(|(_, c)| c), quads)
        })
    }

    /// Laid-out words, each into its own z-layer, their quads `kept`'s for each layer while
    /// `from` (what they were laid out from) is the same, else made again
    pub(crate) fn lbterm_draw_elements<'a>(
        &self,
        kept: &RefCell<KeptLayers>,
        from: impl Hash + Copy,
        elements: impl IntoIterator<Item = &'a ComputedElement>,
    ) -> anyhow::Result<()> {
        // By z-layer, each in the order given
        let mut by_z: Vec<(i8, Vec<&ComputedElement>)> = vec![];
        for element in elements {
            match by_z.iter_mut().find(|(z, _)| *z == element.zindex) {
                Some((_, list)) => list.push(element),
                None => by_z.push((element.zindex, vec![element])),
            }
        }
        let mut kept = kept.borrow_mut();
        for (zindex, list) in by_z {
            self.lbterm_kept_quads_in(kept.layer(zindex), zindex, (from, list.len()), |quads| {
                self.lbterm_render_words_into(list.iter().copied(), quads)
            })?;
        }
        Ok(())
    }

    /// Plain words' glyphs into `quads`, as render_element draws a text element (box_model.rs)
    /// with nothing behind it, no border and one colour, which is all a word here is
    pub(crate) fn lbterm_render_words_into<'a>(
        &self,
        elements: impl IntoIterator<Item = &'a ComputedElement>,
        quads: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        let left = self.dimensions.pixel_width as f32 / -2.0;
        let top = self.dimensions.pixel_height as f32 / -2.0;
        for element in elements {
            let ComputedElementContent::Text(cells) = &element.content else {
                anyhow::bail!("a word that isn't text (render/words.rs)");
            };
            let InheritableColor::Color(colour) = element.colors.text else {
                anyhow::bail!("a word whose colour isn't one colour (render/words.rs)");
            };
            if element.colors.bg != InheritableColor::Color(LinearRgba::TRANSPARENT)
                || element.border_corners.is_some()
                || element.hover_colors.is_some()
            {
                anyhow::bail!("a word with something behind it, a border or a hover colour (render/words.rs)");
            }
            let mut pos_x = element.content_rect.min_x();
            for cell in cells {
                if pos_x >= element.content_rect.max_x() {
                    break;
                }
                match cell {
                    ElementCell::Sprite(sprite) => {
                        let width = sprite.coords.width();
                        let height = sprite.coords.height();
                        let pos_y = top + element.content_rect.min_y();
                        if pos_x + width as f32 > element.content_rect.max_x() {
                            break;
                        }
                        let mut quad = quads.allocate(2)?;
                        quad.set_position(pos_x + left, pos_y, pos_x + left + width as f32, pos_y + height as f32);
                        quad.set_fg_color(colour);
                        quad.set_alt_color_and_mix_value(colour, 0.);
                        quad.set_texture(sprite.texture_coords());
                        quad.set_hsv(None);
                        pos_x += width as f32;
                    }
                    ElementCell::Glyph(glyph) => {
                        if let Some(texture) = glyph.texture.as_ref() {
                            let pos_y = element.content_rect.min_y() as f32 + top
                                - (glyph.y_offset + glyph.bearing_y).get() as f32
                                + element.baseline;
                            if pos_x + glyph.x_advance.get() as f32 > element.content_rect.max_x() {
                                break;
                            }
                            let pos_x = pos_x + (glyph.x_offset + glyph.bearing_x).get() as f32;
                            let width = texture.coords.size.width as f32 * glyph.scale as f32;
                            let height = texture.coords.size.height as f32 * glyph.scale as f32;
                            let mut quad = quads.allocate(1)?;
                            quad.set_position(pos_x + left, pos_y, pos_x + left + width, pos_y + height);
                            quad.set_fg_color(colour);
                            quad.set_alt_color_and_mix_value(colour, 0.);
                            quad.set_texture(texture.texture_coords());
                            quad.set_has_color(glyph.has_color);
                            quad.set_hsv(None);
                        }
                        pos_x += glyph.x_advance.get() as f32;
                    }
                }
            }
        }
        Ok(())
    }

    /// One word as [`Self::lbterm_draw_words`] lays it out: `kept`'s from the last frame when it
    /// has one laid out from the same, else laid out now. `kept` is at this frame's
    /// [`Self::lbterm_words_at`] already; what's taken goes back with `kept.keep`.
    pub(crate) fn lbterm_take_word(
        &self,
        kept: &mut Words,
        font: &Rc<LoadedFont>,
        metrics: &RenderMetrics,
        cell: (f32, f32),
        zindex: i8,
        (text, bounds, colour): Word,
    ) -> anyhow::Result<(WordKey, ComputedElement)> {
        let key = WordKey {
            text,
            colour: [colour.0, colour.1, colour.2, colour.3].map(f32::to_bits),
            bounds: [bounds.min_x(), bounds.min_y(), bounds.width(), bounds.height()].map(f32::to_bits),
            zindex,
            font: Rc::as_ptr(font) as *const () as usize,
            cell: [cell.0.to_bits(), cell.1.to_bits()],
        };
        let computed = kept.laid.take(&key, || {
            let gl_state = self.render_state.as_ref().unwrap();
            let element = Element::new(font, ElementContent::Text(key.text.clone())).colors(text_colors(colour));
            self.compute_element(&self.lbterm_words_context(gl_state, metrics, cell, bounds, zindex), &element)
        })?;
        Ok((key, computed))
    }

    /// What compute_element lays a word out in: `bounds` in a window of this one's size and DPI
    pub(crate) fn lbterm_words_context<'a>(
        &self,
        gl_state: &'a crate::renderstate::RenderState,
        metrics: &'a RenderMetrics,
        cell: (f32, f32),
        bounds: RectF,
        zindex: i8,
    ) -> LayoutContext<'a> {
        LayoutContext {
            width: DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.dimensions.pixel_width as f32,
                pixel_cell: cell.0,
            },
            height: DimensionContext {
                dpi: self.dimensions.dpi as f32,
                pixel_max: self.dimensions.pixel_height as f32,
                pixel_cell: cell.1,
            },
            bounds,
            metrics,
            gl_state,
            zindex,
        }
    }
}
