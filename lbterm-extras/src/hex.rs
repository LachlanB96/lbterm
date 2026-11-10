//! `#RRGGBB` colour codes in a pane's rows, each shown with a bar in its own colour along the
//! foot of its cells (lbterm/HEX.md): what counts as a code ([`find`]), where its bar goes
//! ([`bar`]) and whether its colour is too near the background under it to be seen
//! ([`too_near`]). The GUI's `render/screen_line.rs` calls them as it lays a pane's row out, so a
//! row's codes are found once for what it shows and kept with its quads in the line quad cache.

use std::ops::Range;

/// A colour code in a row: the cells its seven characters are in, `#` to the last digit, and
/// its colour (sRGB)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub cells: Range<usize>,
    pub rgb: [u8; 3],
}

impl Code {
    /// Its colour as linear RGB, as the renderer draws
    pub fn linear(&self) -> [f32; 3] {
        self.rgb.map(linear)
    }
}

/// One of a row's cells, as [`find`] reads it
#[derive(Debug, Clone, Copy)]
pub struct Cell<'a> {
    /// Its column in the row (a wide character before it counts two)
    pub index: usize,
    /// What it shows: a grapheme, with any marks combined with it
    pub text: &'a str,
    /// The program underlined it (any style)
    pub underlined: bool,
    /// The program hid it (SGR 8), so its text isn't drawn
    pub hidden: bool,
    /// The row wraps on to the next after it: set on a wrapped row's last cell. The row's last
    /// cell handed over says it, so the walk that reads the cells needn't look at the row again
    /// (`Line::last_cell_was_wrapped` walks a whole row of a terminal's screen; PITFALLS 290)
    pub wraps: bool,
}

/// What can't come right before a code's `#`: a letter of a script written with spaces between
/// words, a digit, `#` or `&`
fn joins_before(c: char) -> bool {
    word(c) || c == '#' || c == '&'
}

/// What can't come right after a code's last digit: a letter of a script written with spaces
/// between words, a digit or `_`
fn joins_after(c: char) -> bool {
    word(c) || c == '_'
}

/// A character that makes a code next to it part of a word: any digit, and a letter of a
/// script that puts spaces between its words. A letter of a script that doesn't (Chinese,
/// Japanese, Thai and the rest of [`spaceless`]) has nothing between it and a code but the
/// sentence going on, so `颜色#FFBF00` is a code (Lachlan's 7b of 2026-11-10). Korean puts
/// spaces between words, so a code glued to Hangul is inside a word, as one glued to Latin is
fn word(c: char) -> bool {
    c.is_numeric() || (c.is_alphabetic() && !spaceless(c))
}

/// A letter of a script written without spaces between words: Han (Chinese characters, in
/// Chinese and Japanese alike), Hiragana and Katakana (full and half width), Bopomofo, Thai,
/// Lao, Khmer and Myanmar, by their Unicode blocks
fn spaceless(c: char) -> bool {
    matches!(c as u32,
        0x0E00..=0x0EFF // Thai, Lao
        | 0x1000..=0x109F // Myanmar
        | 0x1780..=0x17FF | 0x19E0..=0x19FF // Khmer, its symbols
        | 0x2E80..=0x2FDF // CJK radicals, Kangxi radicals
        | 0x3005..=0x3007 | 0x3021..=0x3029 | 0x3031..=0x3035 | 0x3038..=0x303C // 々 〆 〇, Hangzhou numerals, kana repeat marks
        | 0x3040..=0x30FF // Hiragana, Katakana
        | 0x3100..=0x312F | 0x31A0..=0x31BF // Bopomofo
        | 0x31F0..=0x31FF // Katakana phonetic extensions
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF // CJK ideographs, extension A
        | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F // Myanmar extensions
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFF66..=0xFF9F // half-width Katakana
        | 0x1B000..=0x1B16F // Kana supplement and extended
        | 0x20000..=0x323AF // CJK ideographs, extensions B to H
    )
}

/// The cell's digit, when it holds one hex digit and nothing else
fn digit(text: &str) -> Option<u32> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    c.to_digit(16)
}

/// A code being read: its `#`'s cell, the column its next digit must be in, the digits so far
/// and their value, and whether the program underlined or hid any of its cells
struct Open {
    start: usize,
    next: usize,
    digits: u8,
    value: u32,
    yields: bool,
}

impl Open {
    fn code(&self) -> Option<Code> {
        (!self.yields).then(|| Code {
            cells: self.start..self.start + 7,
            rgb: [(self.value >> 16) as u8, (self.value >> 8) as u8, self.value as u8],
        })
    }
}

/// Every code in a row, from its cells in order (a line's `visible_cells`). A code is `#` and
/// six hex digits, either case, in seven cells side by side, each holding just its character;
/// not after a letter, digit, `#` or `&`, and not followed by a letter, digit or `_`, where a
/// letter of a script written without spaces between words doesn't count ([`word`]). A code
/// ending at the row's last cell when that cell [`wraps`](Cell::wraps) may go on in the next row
/// (`#FFBF00` then `AA`), so it isn't counted. A code the program underlined or hid, in any of
/// its cells, isn't given: the bar yields to the program's own line, and shows nothing the
/// glyphs don't (PITFALLS 291)
pub fn find<'a>(cells: impl IntoIterator<Item = Cell<'a>>) -> Vec<Code> {
    let mut finder = Finder::default();
    for cell in cells {
        finder.cell(cell);
    }
    finder.end()
}

/// [`find`] a cell at a time, for cells whose text lives no longer than the call that hands
/// it over (wezterm's `CellRef::str` borrows the `CellRef`)
#[derive(Default)]
pub struct Finder {
    found: Vec<Code>,
    open: Option<Open>,
    /// The last character of the cell before, None at the row's start or after an empty cell
    before: Option<char>,
    /// The last cell handed over wraps on to the next row
    wraps: bool,
}

impl Finder {
    /// The row's next cell
    pub fn cell(&mut self, cell: Cell<'_>) {
        self.wraps = cell.wraps;
        if let Some(code) = self.open.as_mut() {
            if code.digits == 6 {
                // A whole code: what follows says whether it stands alone
                if !cell.text.chars().next().is_some_and(joins_after) {
                    self.found.extend(code.code());
                }
                self.open = None;
            } else if let Some(d) = digit(cell.text).filter(|_| cell.index == code.next) {
                code.digits += 1;
                code.value = code.value << 4 | d;
                code.next += 1;
                code.yields |= cell.underlined || cell.hidden;
                self.before = cell.text.chars().last();
                return;
            } else {
                self.open = None;
            }
        }
        if cell.text == "#" && !self.before.is_some_and(joins_before) {
            self.open = Some(Open {
                start: cell.index,
                next: cell.index + 1,
                digits: 0,
                value: 0,
                yields: cell.underlined || cell.hidden,
            });
        }
        self.before = cell.text.chars().last();
    }

    /// The row's codes, its cells all handed over: a code still open at the last cell counts
    /// unless that cell wraps, as for [`find`]
    pub fn end(mut self) -> Vec<Code> {
        if let Some(code) = self.open.take() {
            if code.digits == 6 && !self.wraps {
                self.found.extend(code.code());
            }
        }
        self.found
    }
}

/// A rectangle in a row, in pixels, `y` from the row's top
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Where a code's bar is drawn: the bar in the code's colour, and the outline in its text's
/// colour drawn first, under it, when the colour is too near its background
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub fill: Rect,
    pub outline: Option<Rect>,
}

/// A code's bar, its cells running from `left` to `right` (pixels) in a row `cell_height` tall,
/// at `dpi`: about 2 px at 96 DPI along the foot of the row, and with `outlined` a pixel of the
/// text's colour round it, all within the row (nothing may be drawn at another row's place)
pub fn bar(left: f32, right: f32, cell_height: f32, dpi: u32, outlined: bool) -> Bar {
    let scale = dpi as f32 / 96.0;
    let thick = (2.0 * scale).round().max(1.0).min(cell_height);
    if !outlined {
        return Bar {
            fill: Rect {
                x: left,
                y: cell_height - thick,
                width: right - left,
                height: thick,
            },
            outline: None,
        };
    }
    let edge = scale.round().max(1.0);
    let height = (thick + 2.0 * edge).min(cell_height);
    Bar {
        fill: Rect {
            x: left + edge,
            y: cell_height - edge - thick,
            width: (right - left - 2.0 * edge).max(0.0),
            height: thick,
        },
        outline: Some(Rect {
            x: left,
            y: cell_height - height,
            width: right - left,
            height,
        }),
    }
}

/// How near in Oklab a code's colour may come to its background before its bar gets an outline:
/// `#242424` on LBTerm's `#121212` does (0.078), `#252525` doesn't (0.082)
pub const NEAR: f32 = 0.08;

/// Whether `colour` is too near `background` (both linear RGB) for a thin bar of it to be seen:
/// nearer than [`NEAR`] in Oklab, a space where distance follows what the eye tells apart, hue
/// as well as lightness (a red bar on a green background as light is far from it)
pub fn too_near(colour: [f32; 3], background: [f32; 3]) -> bool {
    let (a, b) = (oklab(colour), oklab(background));
    let d = (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt();
    d < NEAR
}

/// How a code's bar shows, by the background under the code's first cell
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// The bar in the code's colour
    Bar,
    /// The bar inside an outline of the text's colour: the code's colour is too near the
    /// background to be seen ([`too_near`])
    Outlined,
    /// No bar: the program painted the code's own colour behind it, as Neovim's colour
    /// highlighters do, so the colour shows already, and an outlined bar would only be a box
    /// at the foot of the program's own swatch
    Shown,
}

/// How a code's bar shows over `background` (linear RGB, the code's first cell's as the layout
/// resolved it for its glyphs), `painted` when it isn't the pane's own (the program set the
/// cell's background, or reversed it): the program's own background in the code's colour, each
/// sRGB channel within 1 of it, is [`Look::Shown`]; any background too near the colour, the
/// pane's own included, [`Look::Outlined`]
pub fn look(code: &Code, background: [f32; 3], painted: bool) -> Look {
    let same = || {
        background
            .iter()
            .zip(code.rgb)
            .all(|(&c, want)| (srgb8(c) - want as f32).abs() <= 1.0)
    };
    if painted && same() {
        Look::Shown
    } else if too_near(code.linear(), background) {
        Look::Outlined
    } else {
        Look::Bar
    }
}

/// A linear channel in sRGB, 0 to 255
fn srgb8(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round()
}

/// An sRGB channel made linear
pub fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear RGB in Oklab (Björn Ottosson's matrices, 2020)
fn oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of one-cell characters, as most rows are
    fn row(text: &str) -> Vec<(usize, String, bool)> {
        text.chars()
            .enumerate()
            .map(|(i, c)| (i, c.to_string(), false))
            .collect()
    }

    /// The row's codes, its last cell wrapping when `wraps`
    fn codes(cells: &[(usize, String, bool)], wraps: bool) -> Vec<Code> {
        let last = cells.len().saturating_sub(1);
        find(cells.iter().enumerate().map(|(i, (index, text, underlined))| Cell {
            index: *index,
            text,
            underlined: *underlined,
            hidden: false,
            wraps: wraps && i == last,
        }))
    }

    fn found(text: &str) -> Vec<(Range<usize>, [u8; 3])> {
        codes(&row(text), false)
            .into_iter()
            .map(|code| (code.cells, code.rgb))
            .collect()
    }

    #[test]
    fn a_code_in_text_is_its_seven_cells_and_its_colour() {
        assert_eq!(found("#FFBF00,"), vec![(0..7, [0xff, 0xbf, 0x00])]);
        assert_eq!(found("(#8a8a9e)"), vec![(1..8, [0x8a, 0x8a, 0x9e])]);
        assert_eq!(found("color: #F2E86D;"), vec![(7..14, [0xf2, 0xe8, 0x6d])]);
        assert_eq!(found("#ffbf00"), vec![(0..7, [0xff, 0xbf, 0x00])], "a row's start and end");
        assert_eq!(found("x = #FfBf00"), vec![(4..11, [0xff, 0xbf, 0x00])], "either case");
        assert_eq!(found("#FFBF00-x #FFBF00.5"), vec![(0..7, [0xff, 0xbf, 0]), (10..17, [0xff, 0xbf, 0])]);
        assert_eq!(found("_#FFBF00"), vec![(1..8, [0xff, 0xbf, 0])], "an underscore before is no letter");
    }

    #[test]
    fn a_code_inside_a_longer_word_or_number_isnt_one() {
        for text in [
            "abc#FFBF00",
            "#FFBF00AA",
            "#1234567",
            "#FFBF00_x",
            "&#x27",
            "&#123456;",
            "##FFBF00",
            "9#FFBF00",
            "#fff",
            "#fff;",
            "#FFBF0",
            "#FFBG00",
            "# FFBF00",
            "é#FFBF00",
            "#FFBF00é",
        ] {
            assert_eq!(found(text), vec![], "{text}");
        }
    }

    #[test]
    fn codes_side_by_side() {
        assert_eq!(found("#000000 #FFFFFF"), vec![(0..7, [0, 0, 0]), (8..15, [0xff, 0xff, 0xff])]);
        // The second's # follows the first's last digit, so only the first stands alone
        assert_eq!(found("#FFBF00#00FF00"), vec![(0..7, [0xff, 0xbf, 0])]);
        // A code that breaks off doesn't hide the next
        assert_eq!(found("#12 #345678"), vec![(4..11, [0x34, 0x56, 0x78])]);
    }

    #[test]
    fn a_wide_character_before_a_code_moves_its_cells_by_two() {
        // 日 and 本 take two cells each: the space is cell 4 and the code starts at 5
        let mut cells = vec![(0, "日".to_string(), false), (2, "本".to_string(), false)];
        cells.extend(row(" #FFBF00").into_iter().map(|(i, t, u)| (i + 4, t, u)));
        assert_eq!(codes(&cells, false), vec![Code { cells: 5..12, rgb: [0xff, 0xbf, 0] }]);
        // Glued to them it's a code all the same: Japanese puts no spaces between words
        let mut glued = vec![(0, "日".to_string(), false), (2, "本".to_string(), false)];
        glued.extend(row("#FFBF00").into_iter().map(|(i, t, u)| (i + 4, t, u)));
        assert_eq!(codes(&glued, false), vec![Code { cells: 4..11, rgb: [0xff, 0xbf, 0] }]);
    }

    /// A row whose characters are each as wide as `wide` says, two cells or one
    fn wide_row(text: &str, wide: impl Fn(char) -> bool) -> Vec<(usize, String, bool)> {
        let mut at = 0;
        text.chars()
            .map(|c| {
                let cell = (at, c.to_string(), false);
                at += if wide(c) { 2 } else { 1 };
                cell
            })
            .collect()
    }

    #[test]
    fn a_letter_of_a_script_without_spaces_doesnt_glue_a_code() {
        // Two cells: the CJK letters here; one: Thai and the rest, and half-width Katakana
        let cjk = |c: char| ('\u{2E80}'..='\u{FAFF}').contains(&c) || c >= '\u{1B000}';
        let at = |text: &str| -> Vec<Range<usize>> {
            codes(&wide_row(text, cjk), false).into_iter().map(|code| code.cells).collect()
        };
        // Chinese before and after: 颜色 is four cells, the code five to eleven
        assert_eq!(at("颜色#FFBF00"), vec![4..11]);
        assert_eq!(at("#FFBF00色"), vec![0..7]);
        // Japanese kana, full and half width, and the iteration mark 々
        assert_eq!(at("いろ#00FF00です"), vec![4..11]);
        assert_eq!(at("カラー#00FF00"), vec![6..13]);
        assert_eq!(at("ｶﾗｰ#00FF00"), vec![3..10], "half-width kana, a cell each");
        assert_eq!(at("色々#00FF00"), vec![4..11]);
        // Thai, Lao, Khmer and Myanmar letters, one cell each
        assert_eq!(found("สี#FF0000"), vec![(2..9, [0xff, 0, 0])]);
        assert_eq!(found("ສີ#FF0000"), vec![(2..9, [0xff, 0, 0])]);
        assert_eq!(found("ពណ៌#FF0000").len(), 1);
        assert_eq!(found("အရောင်#FF0000").len(), 1);
        // Korean and Latin put spaces between words: glued to them it's inside a word
        assert_eq!(at("색#FF0000"), vec![]);
        assert_eq!(at("#FF0000색"), vec![]);
        assert_eq!(found("abc#FF0000"), vec![]);
        assert_eq!(found("#FF0000x"), vec![]);
        // A digit of any script still glues: a code doesn't run on into a number
        assert_eq!(found("#FF0000๑"), vec![], "a Thai digit after it");
        assert_eq!(found("๑#FF0000"), vec![], "a Thai digit before it");
        assert_eq!(at("#FF0000１"), vec![], "a full-width digit after it");
    }

    #[test]
    fn a_cell_with_a_mark_on_its_digit_breaks_the_code() {
        let mut cells = row("#FFBF0 ");
        cells[6].1 = "0\u{301}".to_string();
        assert_eq!(codes(&cells, false), vec![]);
    }

    #[test]
    fn a_gap_between_cells_breaks_the_code() {
        let mut cells = row("#FFBF00");
        for cell in &mut cells[4..] {
            cell.0 += 1;
        }
        assert_eq!(codes(&cells, false), vec![]);
    }

    #[test]
    fn a_code_ending_a_wrapped_row_isnt_counted_and_one_before_its_end_is() {
        assert_eq!(codes(&row("x #FFBF00"), true), vec![], "the next row could go on with AA");
        assert_eq!(codes(&row("x #FFBF00"), false).len(), 1);
        assert_eq!(codes(&row("#FFBF00 x"), true).len(), 1);
        // Cut short by the wrap: no bar on this part
        assert_eq!(codes(&row("x #FFB"), true), vec![]);
    }

    #[test]
    fn the_rows_last_cell_handed_over_says_whether_it_wraps_with_no_second_look_at_the_row() {
        // As render/hex.rs hands a row over: each cell with its own wrapped bit, the last's
        // deciding, so nothing walks the row again for Line::last_cell_was_wrapped
        let cell = |index: usize, text: &'static str, wraps: bool| Cell {
            index,
            text,
            underlined: false,
            hidden: false,
            wraps,
        };
        let text = ["x", " ", "#", "F", "F", "B", "F", "0", "0"];
        let mut wrapped = Finder::default();
        for (i, t) in text.iter().enumerate() {
            wrapped.cell(cell(i, t, i == text.len() - 1));
        }
        assert_eq!(wrapped.end(), vec![], "a six-digit code open at a wrapped row's end");
        // A wrapped bit on a cell before the last says nothing about the row's end
        let mut not = Finder::default();
        for (i, t) in text.iter().enumerate() {
            not.cell(cell(i, t, i == 3));
        }
        assert_eq!(not.end(), vec![Code { cells: 2..9, rgb: [0xff, 0xbf, 0] }]);
    }

    #[test]
    fn a_code_the_program_hid_gets_no_bar() {
        // SGR 8 on any of its cells: the glyph pass draws none of that text, and the bar mustn't
        // show a colour, or that a code is there, under blank cells
        for hidden in [0, 4, 6] {
            let cells: Vec<_> = row("#FFBF00 #00FF00").into_iter().collect();
            let found = find(cells.iter().enumerate().map(|(i, (index, text, _))| Cell {
                index: *index,
                text,
                underlined: false,
                hidden: i == hidden,
                wraps: false,
            }));
            assert_eq!(found, vec![Code { cells: 8..15, rgb: [0, 0xff, 0] }], "cell {hidden} hidden");
        }
    }

    #[test]
    fn a_code_on_its_own_colour_the_program_painted_shows_no_bar_and_one_near_the_panes_is_outlined() {
        let code = |rgb: [u8; 3]| Code { cells: 0..7, rgb };
        let lbterm = srgb(0x121212);
        // A colour highlighter's swatch: the program painted the code's colour behind it
        assert_eq!(look(&code([0x40, 0xc0, 0xff]), srgb(0x40c0ff), true), Look::Shown);
        assert_eq!(look(&code([0x12, 0x12, 0x12]), lbterm, true), Look::Shown);
        // The pane's own background in the code's colour: outlined, so it's seen (Q1)
        assert_eq!(look(&code([0x12, 0x12, 0x12]), lbterm, false), Look::Outlined);
        // A background the program painted near the code's colour but not it: outlined, as on the pane's
        assert_eq!(look(&code([0x1e, 0x1e, 0x1e]), srgb(0x1e1e2e), true), Look::Outlined);
        // Far from what's under it, painted or not: the bar
        assert_eq!(look(&code([0xff, 0xbf, 0x00]), lbterm, false), Look::Bar);
        assert_eq!(look(&code([0xff, 0xbf, 0x00]), srgb(0x404040), true), Look::Bar);
        // Within a channel's rounding of the code's colour counts as it
        assert_eq!(look(&code([0x40, 0xc0, 0xff]), srgb(0x41c0fe), true), Look::Shown);
        assert_eq!(look(&code([0x40, 0xc0, 0xff]), srgb(0x42c0ff), true), Look::Outlined);
    }

    #[test]
    fn the_bar_yields_to_a_programs_own_underline() {
        let mut cells = row("#FFBF00 #00FF00");
        cells[3].2 = true;
        assert_eq!(codes(&cells, false), vec![Code { cells: 8..15, rgb: [0, 0xff, 0] }]);
        // An underline elsewhere in the row changes nothing
        let mut beside = row("#FFBF00 x");
        beside[8].2 = true;
        assert_eq!(codes(&beside, false).len(), 1);
        // On its # alone too
        let mut hash = row("#FFBF00");
        hash[0].2 = true;
        assert_eq!(codes(&hash, false), vec![]);
    }

    #[test]
    fn the_bar_is_at_the_foot_of_the_row_about_two_pixels_at_96_dpi() {
        let plain = bar(10.0, 73.0, 20.0, 96, false);
        assert_eq!(plain.fill, Rect { x: 10.0, y: 18.0, width: 63.0, height: 2.0 });
        assert_eq!(plain.outline, None);
        assert_eq!(bar(10.0, 73.0, 40.0, 192, false).fill.height, 4.0);
        // 2.5 px at 120 DPI is drawn as 3, its foot kept
        assert_eq!(bar(10.0, 73.0, 25.0, 120, false).fill, Rect { x: 10.0, y: 22.0, width: 63.0, height: 3.0 });
        // Never more than the row
        assert_eq!(bar(0.0, 7.0, 1.0, 96, false).fill, Rect { x: 0.0, y: 0.0, width: 7.0, height: 1.0 });
    }

    #[test]
    fn an_outlined_bar_is_the_bar_inside_a_pixel_of_the_texts_colour_within_the_row() {
        let outlined = bar(10.0, 73.0, 20.0, 96, true);
        assert_eq!(outlined.outline, Some(Rect { x: 10.0, y: 16.0, width: 63.0, height: 4.0 }));
        assert_eq!(outlined.fill, Rect { x: 11.0, y: 17.0, width: 61.0, height: 2.0 });
        let big = bar(10.0, 136.0, 40.0, 192, true);
        assert_eq!(big.outline, Some(Rect { x: 10.0, y: 32.0, width: 126.0, height: 8.0 }));
        assert_eq!(big.fill, Rect { x: 12.0, y: 34.0, width: 122.0, height: 4.0 });
        let tiny = bar(0.0, 7.0, 3.0, 96, true);
        assert!(tiny.outline.unwrap().y >= 0.0, "within the row");
    }

    fn srgb(hex: u32) -> [f32; 3] {
        [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8].map(linear)
    }

    #[test]
    fn a_colour_too_near_its_background_is_told_apart_by_eye_not_by_numbers() {
        let lbterm = srgb(0x121212);
        assert!(too_near(srgb(0x121212), lbterm), "the background itself");
        assert!(too_near(srgb(0x242424), lbterm));
        assert!(!too_near(srgb(0x252525), lbterm));
        assert!(!too_near(srgb(0xffbf00), lbterm));
        assert!(!too_near(srgb(0x000000), lbterm), "black is darker enough");
        assert!(too_near(srgb(0xffffff), srgb(0xfefefe)));
        // A hue apart at the same lightness is seen
        assert!(!too_near(srgb(0xd04040), srgb(0x40a040)));
        assert_eq!(Code { cells: 0..7, rgb: [0x12, 0x12, 0x12] }.linear(), lbterm);
    }
}
