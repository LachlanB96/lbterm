//! LBTerm: each `#RRGGBB` code in a pane's row gets a bar in its own colour along the foot of its
//! cells, drawn as the row is laid out (lbterm/HEX.md). The codes are found from the row's own
//! cells (`lbterm_extras::hex::find`), which the line quad cache's key holds through the row's
//! shape hash, so the bars are kept with the row's quads by what the row shows: a still screen and
//! a row that only moved draw nothing new, and a row is looked at once for what it shows.

use crate::quad::{QuadTrait, TripleLayerQuadAllocator};
use crate::termwindow::render::{LineToElementShape, RenderScreenLineParams};
use ::window::RectF;
use config::HsbTransform;
use lbterm_extras::hex;
use termwiz::cell::Underline;
use termwiz::color::ColorAttribute;
use termwiz::color::LinearRgba;
use wezterm_bidi::Direction;

impl crate::TermWindow {
    /// The bars of the codes in `params.line`, in layer 0 after the row's backgrounds and its
    /// selection's (so a selected code shows its bar as at rest), under its glyphs. `shaped` is
    /// the row's clusters as the layout coloured them, for the text's colour and the background
    /// under a code; `bounds` the row's rectangle, which nothing drawn leaves, a cell's height
    /// tall; `cell_width` a cell's width on this row (twice the font's on a double-width line)
    pub(crate) fn lbterm_hex_bars(
        &self,
        params: &RenderScreenLineParams,
        shaped: &[LineToElementShape],
        layers: &mut TripleLayerQuadAllocator,
        bounds: RectF,
        cell_width: f32,
        direction: Direction,
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<()> {
        // One walk of the row's cells: the last one's wrapped bit is taken here too, since
        // Line::last_cell_was_wrapped would walk a screen row's cells again (PITFALLS 290)
        let mut finder = hex::Finder::default();
        for cell in params.line.visible_cells() {
            let attrs = cell.attrs();
            finder.cell(hex::Cell {
                index: cell.cell_index(),
                text: cell.str(),
                underlined: attrs.underline() != Underline::None,
                hidden: attrs.invisible(),
                wraps: attrs.wrapped(),
            });
        }
        let codes = finder.end();
        if codes.is_empty() {
            return Ok(());
        }
        let cell_height = bounds.size.height;
        let num_cols = params.dims.cols;
        for code in codes {
            // Its columns, as render_screen_line's `phys` places the underlines
            let (first, last) = match direction {
                Direction::LeftToRight => (code.cells.start, code.cells.end - 1),
                Direction::RightToLeft => (
                    num_cols.saturating_sub(code.cells.end - 1),
                    num_cols.saturating_sub(code.cells.start),
                ),
            };
            let left = params.left_pixel_x + first as f32 * cell_width;
            let right = params.left_pixel_x + (last + 1) as f32 * cell_width;
            let cells = |item: &&LineToElementShape| {
                let start = item.cluster.first_cell_idx;
                start..start + item.cluster.width.max(1)
            };
            // A cluster of the code's drawn in its background's colour is skipped by the glyph
            // pass as it skips SGR 8 (screen_line.rs), so the bar shows nothing there either
            // (PITFALLS 291)
            if shaped
                .iter()
                .filter(|item| {
                    let span = cells(item);
                    span.start < code.cells.end && code.cells.start < span.end
                })
                .any(|item| item.fg_color == item.bg_color)
            {
                continue;
            }
            // The text's colour and the background under the code, as its glyphs are drawn
            let item = shaped
                .iter()
                .find(|item| cells(item).contains(&code.cells.start));
            let look = match item {
                Some(item) => {
                    let (r, g, b, _) = item.bg_color.tuple();
                    let attrs = &item.cluster.attrs;
                    let painted = attrs.background() != ColorAttribute::Default || attrs.reverse();
                    hex::look(&code, [r, g, b], painted)
                }
                None => hex::Look::Bar,
            };
            if look == hex::Look::Shown {
                continue;
            }
            let colour = code.linear();
            let bar = hex::bar(
                left,
                right,
                cell_height,
                self.dimensions.dpi as u32,
                look == hex::Look::Outlined,
            );
            if let (Some(outline), Some(item)) = (bar.outline, item) {
                self.lbterm_hex_rect(layers, bounds, outline, item.fg_color, hsv)?;
            }
            let [r, g, b] = colour;
            let fill = LinearRgba::with_components(r, g, b, 1.0);
            self.lbterm_hex_rect(layers, bounds, bar.fill, fill, hsv)?;
        }
        Ok(())
    }

    /// One of a bar's rectangles, `rect`'s y from the row's top, cut to the row
    fn lbterm_hex_rect(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        bounds: RectF,
        rect: hex::Rect,
        color: LinearRgba,
        hsv: Option<HsbTransform>,
    ) -> anyhow::Result<()> {
        let rect = euclid::rect(rect.x, bounds.origin.y + rect.y, rect.width, rect.height);
        if let Some(rect) = rect.intersection(&bounds) {
            let mut quad = self.filled_rectangle(layers, 0, rect, color)?;
            quad.set_hsv(hsv);
        }
        Ok(())
    }
}
