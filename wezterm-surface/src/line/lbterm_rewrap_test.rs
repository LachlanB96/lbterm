//! LBTerm: a resize's rewrap a run at a time (`Line::wrap`, `Line::append_line`,
//! `Line::last_cell_was_wrapped` on a clustered line; clusterline.rs's `lbterm_*`) held to the
//! cell-at-a-time ways upstream used, which are copied here, on lines of ASCII, double-wide and
//! combining characters in several attributes, links among them, some marked wrapped

use super::*;
use crate::hyperlink::Hyperlink;
use wezterm_cell::color::{ColorAttribute, SrgbaTuple};
use wezterm_cell::Intensity;

/// A small deterministic generator, so a failure names its seed
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn attrs(rng: &mut Rng) -> CellAttributes {
    let mut a = CellAttributes::default();
    match rng.below(5) {
        0 => {}
        1 => {
            a.set_intensity(Intensity::Bold);
        }
        2 => {
            a.set_foreground(ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
                0.2, 0.8, 0.4, 1.0,
            )));
        }
        3 => {
            a.set_background(ColorAttribute::TrueColorWithDefaultFallback(SrgbaTuple(
                0.1, 0.1, 0.3, 1.0,
            )));
        }
        _ => {
            a.set_hyperlink(Some(Arc::new(Hyperlink::new("https://example.com/a"))));
        }
    }
    a
}

/// A line of `cells` cells at most, written a grapheme at a time as the terminal writes one,
/// runs of each attribute, a space or two at its end at times, and marked wrapped at times
fn line(rng: &mut Rng, cells: usize, seqno: SequenceNo) -> Line {
    line_of(rng, cells, seqno, false)
}

/// The same, of printable ASCII alone with `ascii`, as a log or a build's output is
fn line_of(rng: &mut Rng, cells: usize, seqno: SequenceNo, ascii: bool) -> Line {
    let mut line = Line::new(seqno);
    let mut pen = attrs(rng);
    let mut idx = 0;
    while idx < cells {
        if rng.below(8) == 0 {
            pen = attrs(rng);
        }
        let (text, width) = match if ascii { 3 + rng.below(9) } else { rng.below(12) } {
            0 => ("中", 2),
            1 => ("😍", 2),
            2 => ("e\u{301}", 1),
            3 | 4 => (" ", 1),
            5 if !ascii => ("─", 1),
            _ => (["a", "b", "x", "7", "/"][rng.below(5)], 1),
        };
        if idx + width > cells {
            break;
        }
        line.set_cell_grapheme_ref(idx, text, width, &pen, seqno);
        idx += width;
    }
    for _ in 0..rng.below(3) {
        line.set_cell_grapheme_ref(idx, " ", 1, &pen, seqno);
        idx += 1;
    }
    if rng.below(3) == 0 {
        line.set_last_cell_was_wrapped(true, seqno);
    }
    line
}

fn is_clustered(line: &Line) -> bool {
    matches!(line.cells, CellStorage::C(_))
}

/// Upstream's Line::wrap
fn upstream_wrap(line: Line, width: usize, seqno: SequenceNo) -> Vec<Line> {
    let mut cells: Vec<CellRef> = line.visible_cells().collect();
    if let Some(end_idx) = cells.iter().rposition(|c| c.str() != " ") {
        cells.truncate(end_idx + 1);
        let mut lines: Vec<Line> = vec![];
        let mut delta = 0;
        for cell in cells {
            let need_new_line = lines
                .last_mut()
                .map(|line| line.len() + cell.width() > width)
                .unwrap_or(true);
            if need_new_line {
                lines
                    .last_mut()
                    .map(|line| line.set_last_cell_was_wrapped(true, seqno));
                lines.push(Line::new(seqno));
                delta = cell.cell_index();
            }
            let line = lines.last_mut().unwrap();
            line.set_cell_grapheme(
                cell.cell_index() - delta,
                cell.str(),
                cell.width(),
                (*cell.attrs()).clone(),
                seqno,
            );
        }
        lines
    } else {
        vec![line]
    }
}

/// Upstream's Line::append_line onto a clustered line
fn upstream_append(line: &mut Line, other: Line, seqno: SequenceNo) {
    match &mut line.cells {
        CellStorage::C(cl) => {
            for cell in other.visible_cells() {
                cl.append(cell.as_cell());
            }
        }
        CellStorage::V(_) => unreachable!("the test's lines are clustered"),
    }
    line.update_last_change_seqno(seqno);
    line.invalidate_zones();
}

fn upstream_last_wrapped(line: &Line) -> bool {
    line.visible_cells()
        .last()
        .map(|c| c.attrs().wrapped())
        .unwrap_or(false)
}

#[test]
fn a_clustered_line_is_cut_as_a_cell_at_a_time_cut_it() {
    for seed in 1..400u64 {
        let mut rng = Rng(seed * 0x9E37_79B9_7F4A_7C15);
        let cells = rng.below(260);
        let l = line(&mut rng, cells, 3);
        assert!(is_clustered(&l));
        for width in [2usize, 3, 7, 79, 80, 119, 120, 121, 300] {
            let ours = l.clone().wrap(width, 9);
            let theirs = upstream_wrap(l.clone(), width, 9);
            assert_eq!(
                ours, theirs,
                "seed {seed}, {cells} cells cut at {width}: {:?}",
                l.as_str()
            );
            for (a, b) in ours.iter().zip(theirs.iter()) {
                assert_eq!(a.last_cell_was_wrapped(), upstream_last_wrapped(b));
            }
        }
    }
}

#[test]
fn a_line_of_ascii_is_cut_as_a_cell_at_a_time_cut_it() {
    for seed in 1..400u64 {
        let mut rng = Rng(seed * 0x6C07_8965_D5A4_1B33);
        let cells = rng.below(300);
        let l = line_of(&mut rng, cells, 3, true);
        assert!(l.as_str().is_ascii());
        for width in [2usize, 3, 79, 80, 119, 120, 300] {
            assert_eq!(
                l.clone().wrap(width, 9),
                upstream_wrap(l.clone(), width, 9),
                "seed {seed}, {cells} cells cut at {width}: {:?}",
                l.as_str()
            );
        }
    }
}

#[test]
fn a_width_under_two_is_cut_a_cell_at_a_time() {
    let mut rng = Rng(7);
    let l = line(&mut rng, 40, 3);
    assert_eq!(l.clone().wrap(1, 9), upstream_wrap(l, 1, 9));
}

#[test]
fn a_line_of_spaces_is_left_as_it_was() {
    let mut l = Line::new(3);
    for idx in 0..10 {
        l.set_cell_grapheme_ref(idx, " ", 1, &CellAttributes::default(), 3);
    }
    let rows = l.clone().wrap(4, 9);
    assert_eq!(rows, vec![l]);
}

#[test]
fn a_clustered_line_is_joined_as_a_cell_at_a_time_joined_it() {
    for seed in 1..400u64 {
        let mut rng = Rng(seed * 0xD1B5_4A32_D192_ED03);
        let (na, nb) = (rng.below(200), rng.below(200));
        let a = line(&mut rng, na, 3);
        let b = line(&mut rng, nb, 4);
        let mut ours = a.clone();
        let mut theirs = a.clone();
        // As the rewrap does: the head's wrapped mark cleared, then the next row joined
        ours.set_last_cell_was_wrapped(false, 9);
        theirs.set_last_cell_was_wrapped(false, 9);
        ours.append_line(b.clone(), 9);
        upstream_append(&mut theirs, b.clone(), 9);
        assert_eq!(ours, theirs, "seed {seed}: {:?} + {:?}", a.as_str(), b.as_str());
        assert_eq!(ours.last_cell_was_wrapped(), upstream_last_wrapped(&theirs));
        // and cut again at another width
        for width in [5usize, 80, 119] {
            assert_eq!(ours.clone().wrap(width, 10), upstream_wrap(theirs.clone(), width, 10));
        }
    }
}

#[test]
fn whether_the_last_cell_is_wrapped_is_read_without_the_walk() {
    for seed in 1..400u64 {
        let mut rng = Rng(seed * 0x2545_F491_4F6C_DD1D);
        let n = rng.below(130);
        let l = line(&mut rng, n, 3);
        assert_eq!(l.last_cell_was_wrapped(), upstream_last_wrapped(&l), "seed {seed}");
        let mut cleared = l.clone();
        cleared.set_last_cell_was_wrapped(false, 4);
        assert_eq!(cleared.last_cell_was_wrapped(), upstream_last_wrapped(&cleared));
    }
    assert!(!Line::new(1).last_cell_was_wrapped());
}

#[test]
fn a_line_of_u16_max_cells_is_joined_a_cell_at_a_time() {
    let mut a: Line = "x".repeat(40_000).as_str().into();
    let mut b: Line = "y".repeat(40_000).as_str().into();
    a.compress_for_scrollback();
    b.compress_for_scrollback();
    assert!(is_clustered(&a) && is_clustered(&b));
    let mut ours = a.clone();
    let mut theirs = a.clone();
    ours.append_line(b.clone(), 2);
    upstream_append(&mut theirs, b, 2);
    assert_eq!(ours, theirs);
    assert_eq!(ours.len(), 80_000);
}
