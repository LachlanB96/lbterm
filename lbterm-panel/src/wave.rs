//! The wave that runs ahead of the panel's edge as it opens (lbterm/TAB-PANEL.md, "The wave"):
//! characters washing out over the panes ahead of the edge and on past where it stops, then
//! breaking into spray, in the panes' font and on their rows. An experiment (Lachlan's ask,
//! 2026-10-24), off unless `lbterm_tab_panel.wave` says so.
//!
//! What it looks like, as cells counted back from its front: each row's front sags back up to
//! [`SAG`] columns, along two sines of different lengths so the line doesn't look ruled; at the
//! front a fleck of foam, then the crest, three columns at full brightness, then water thinning out
//! behind, hiding less and less of the text under it. Where the crest breaks, [`spray`] scatters
//! droplets on ahead of it. The compositor draws both once and carries them a cell at a time
//! (`lbterm_motion::wash`), so the characters step across the panes as text would.

use crate::palette;
use std::f64::consts::TAU;

/// How far back a row's front sags, in columns
pub const SAG: u32 = 3;
/// Columns behind its front past which the water shows nothing and hides nothing
pub const DEPTH: u32 = 22;
/// How many columns the water pulls back as it fades, and the spray scatters on
pub const RECEDE: u32 = 3;
pub const DRIFT: u32 = 6;
/// Columns of spray past each row's front
pub const SPRAY: u32 = 12;
/// Dimmer than this, a cell shows no character, only its cover
const DARK: f32 = 0.1;
/// Hiding less than this, a cell without a character isn't drawn
const FAINT: f32 = 0.04;

/// One character cell of the wave or its spray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Columns back from the wave's front, 0 the leading one; for the spray, columns on from
    /// where it's drawn
    pub col: u32,
    pub row: u32,
    /// A space for a cell that only covers
    pub ch: char,
    /// How bright its character is, 0 to 1: the crest is 1
    pub light: f32,
    /// How much of what's under it the cell hides, 0 to 1, before the config's `cover`
    pub cover: f32,
}

/// What a cell `d` columns behind its row's front is: the characters it picks from, its light and
/// its cover.
fn profile(d: u32) -> (&'static [char], f32, f32) {
    match d {
        // Foam, half over what's under it
        0 => (&['.', '\'', ',', '`'], 0.65, 0.5),
        1 => (&[':', '~', '-'], 0.9, 1.0),
        // The crest
        2 => (&['=', '~', '='], 1.0, 1.0),
        3 => (&['~', '=', '~'], 1.0, 1.0),
        4 => (&['~', '~', '-'], 0.9, 1.0),
        5 => (&['~', '-'], 0.75, 0.9),
        // Water, thinning out, the text showing through it more and more
        _ => (
            &['~', '-', '.', ' ', '~', '-'],
            0.65 * 0.8f32.powi((d - 6) as i32),
            0.9 * 0.82f32.powi((d - 5) as i32),
        ),
    }
}

/// The same scattering every time it's drawn.
fn hash(row: u32, col: u32) -> u32 {
    let mut h = row.wrapping_mul(0x9E37_79B1) ^ col.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h
}

/// How many columns row `row`'s front sags back from the wave's.
pub fn sag(row: u32) -> u32 {
    let r = row as f64;
    let s = 0.5 + 0.3 * (TAU * r / 11.0 + 0.7).sin() + 0.2 * (TAU * r / 4.3 + 2.1).sin();
    (s.clamp(0.0, 1.0) * SAG as f64).round() as u32
}

/// The wave `cols` columns back from its front and `rows` rows down: every cell that shows a
/// character or hides what's under it.
pub fn cells(cols: u32, rows: u32) -> Vec<Cell> {
    let mut out = vec![];
    for row in 0..rows {
        for col in sag(row)..cols {
            let (chars, light, cover) = profile(col - sag(row));
            let ch = if light < DARK {
                ' '
            } else {
                chars[hash(row, col) as usize % chars.len()]
            };
            if ch == ' ' && cover < FAINT {
                continue;
            }
            out.push(Cell { col, row, ch, light, cover });
        }
    }
    out
}

/// How many columns the wave needs for a room `room` pixels wide (the edge's travel and how far
/// past it the wave runs) in cells `cell_w` wide: its depth and the sag, or the room if that's less.
pub fn columns(room: f32, cell_w: f32) -> u32 {
    if room <= 0.0 || cell_w <= 0.0 {
        return 0;
    }
    ((room / cell_w).ceil() as u32 + SAG).min(DEPTH + SAG)
}

/// The spray thrown where the crest breaks, `rows` rows down, drawn from [`SAG`] columns short of
/// the wave's furthest front: droplets past each row's front, thick near it and thinning out, big
/// and bright near it, flecks further out. Each hides the letter under it, so it shows over the
/// text. Its columns count on from where it's drawn.
pub fn spray(rows: u32) -> Vec<Cell> {
    const DROPS: [char; 6] = ['*', 'o', '+', '*', 'O', 'o'];
    const FLECKS: [char; 7] = ['.', '\'', '`', ',', ':', '.', '*'];
    let mut out = vec![];
    for row in 0..rows {
        let front = SAG - sag(row);
        for e in 0..SPRAY {
            let chance = 0.9 * 0.85f64.powi(e as i32);
            let h = hash(row.wrapping_add(7919), e.wrapping_add(104_729));
            if (h as f64 / u32::MAX as f64) >= chance {
                continue;
            }
            let pick = hash(row.wrapping_add(15_485_863), e) as usize;
            let (ch, cover) = if e < 4 {
                (DROPS[pick % DROPS.len()], 1.0)
            } else {
                (FLECKS[pick % FLECKS.len()], 0.8)
            };
            out.push(Cell {
                col: front + e,
                row,
                ch,
                light: 0.92f32.powi(e as i32).max(0.5),
                cover,
            });
        }
    }
    out
}

/// A cell's character's colour `y` of the way down the panel, over a pane of colour `card`: the
/// ring's light at that height, whitening towards the crest, as bright as the cell.
pub fn colour(card: u32, y: f64, light: f32) -> u32 {
    let l = light.clamp(0.0, 1.0) as f64;
    let hue = palette::blend(palette::ring(y), 0xFFFFFF, 0.5 * l * l);
    palette::blend(card, hue, l)
}

#[cfg(test)]
mod test {
    use super::*;

    fn at(cells: &[Cell], row: u32, col: u32) -> Option<Cell> {
        cells.iter().copied().find(|c| c.row == row && c.col == col)
    }

    #[test]
    fn each_row_starts_at_its_sagging_front() {
        let cells = cells(12, 40);
        for row in 0..40 {
            let s = sag(row);
            assert!(s <= SAG);
            assert!((0..s).all(|col| at(&cells, row, col).is_none()), "nothing ahead of row {row}'s front");
            let front = at(&cells, row, s).expect("a front");
            assert_eq!(front.cover, 0.5, "foam at the front");
            assert!(".',`".contains(front.ch));
            assert!((s..12).all(|col| at(&cells, row, col).is_some()), "no holes behind it");
        }
        // Not ruled: the fronts wander over every column the sag allows
        let fronts: std::collections::BTreeSet<u32> = (0..40).map(sag).collect();
        assert_eq!(fronts.len(), SAG as usize + 1, "{fronts:?}");
    }

    #[test]
    fn the_crest_is_brightest_and_the_water_thins_out_behind() {
        let cells = cells(DEPTH + SAG, 8);
        for row in 0..8 {
            let s = sag(row);
            let cell = |d: u32| at(&cells, row, s + d);
            let light = |d: u32| cell(d).unwrap().light;
            assert_eq!((light(2), light(3)), (1.0, 1.0), "three columns of crest");
            assert!(light(0) < light(1) && light(1) < light(2));
            assert!((3..14).all(|d| light(d + 1) <= light(d)), "row {row}");
            // Hiding the text under the crest, less and less behind it
            assert!((1..=4).all(|d| cell(d).unwrap().cover == 1.0));
            let mut last = 1.0;
            for d in 5..DEPTH {
                if let Some(c) = cell(d) {
                    assert!(c.cover <= last);
                    last = c.cover;
                }
            }
            // Far back it shows nothing and hides little
            for d in 16..DEPTH {
                if let Some(c) = cell(d) {
                    assert_eq!(c.ch, ' ');
                    assert!(c.cover < 0.15);
                }
            }
        }
    }

    #[test]
    fn it_is_ascii_and_the_same_every_time() {
        let a = cells(12, 30);
        assert!(a.iter().all(|c| c.ch.is_ascii() && !c.ch.is_ascii_control()));
        assert_eq!(a, cells(12, 30));
        // Scattered, not one character down a column
        let crest: std::collections::BTreeSet<char> = (0..30).map(|r| at(&a, r, sag(r) + 2).unwrap().ch).collect();
        assert!(crest.len() > 1);
    }

    #[test]
    fn it_is_as_wide_as_its_depth_or_the_room() {
        assert_eq!(columns(444.0, 9.0), DEPTH + SAG);
        assert_eq!(columns(50.0, 9.0), 6 + SAG);
        assert_eq!(columns(0.0, 9.0), 0);
        assert_eq!(columns(222.0, 0.0), 0);
    }

    #[test]
    fn the_spray_is_thrown_on_past_each_row_and_thins_out() {
        let rows = 60;
        let drops = spray(rows);
        assert_eq!(drops, spray(rows));
        assert!(drops.iter().all(|c| c.ch.is_ascii() && c.ch != ' ' && c.cover >= 0.8));
        // Big drops by the crest, hiding the letter under them; flecks further out
        let past = |c: &Cell| c.col - (SAG - sag(c.row));
        assert!(drops.iter().filter(|c| past(c) < 4).all(|c| "*o+O".contains(c.ch) && c.cover == 1.0));
        assert!(drops.iter().filter(|c| past(c) >= 4).all(|c| ".'`,:*".contains(c.ch)));
        // Thick enough to read as spray: a couple of drops a row by the crest
        assert!(drops.iter().filter(|c| past(c) < 4).count() as u32 > 2 * rows);
        for c in &drops {
            let front = SAG - sag(c.row);
            assert!(c.col >= front && c.col < front + SPRAY, "past its row's front");
        }
        // Thick near the front, thin far out, and dimmer
        let near = drops.iter().filter(|c| c.col - (SAG - sag(c.row)) < 3).count();
        let far = drops.iter().filter(|c| c.col - (SAG - sag(c.row)) >= SPRAY - 3).count();
        assert!(near > 3 * far.max(1), "{near} near, {far} far");
        let light = |e: u32| drops.iter().filter(|c| c.col - (SAG - sag(c.row)) == e).map(|c| c.light).next();
        if let (Some(a), Some(b)) = (light(0), light(7)) {
            assert!(a > b);
        }
    }

    #[test]
    fn its_colour_comes_up_from_the_pane_to_the_rings_light() {
        let card = 0x16161E;
        assert_eq!(colour(card, 0.0, 0.0), card);
        // The crest is the ring's red whitened, the water near the pane
        let crest = colour(card, 0.0, 1.0);
        assert_eq!(crest, palette::blend(palette::RING_TOP, 0xFFFFFF, 0.5));
        let dim = colour(card, 0.0, 0.15);
        let (r, g, b) = palette::bytes(dim);
        assert!(r > 0x16 && r < 0x50 && g < 0x30 && b < 0x30, "{dim:06x}");
        // Down the panel it follows the ring
        assert_ne!(colour(card, 0.5, 1.0), crest);
    }
}
