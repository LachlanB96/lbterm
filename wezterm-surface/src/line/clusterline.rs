use crate::line::CellRef;
use core::convert::TryInto;
use core::num::NonZeroU8;
use finl_unicode::grapheme_clusters::Graphemes;
use fixedbitset::FixedBitSet;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use wezterm_cell::{Cell, CellAttributes};

extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
struct Cluster {
    cell_width: u16,
    attrs: CellAttributes,
}

/// Stores line data as a contiguous string and a series of
/// clusters of attribute data describing attributed ranges
/// within the line
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClusteredLine {
    pub text: String,
    #[cfg_attr(
        feature = "use_serde",
        serde(
            deserialize_with = "deserialize_bitset",
            serialize_with = "serialize_bitset"
        )
    )]
    is_double_wide: Option<Box<FixedBitSet>>,
    clusters: Vec<Cluster>,
    /// Length, measured in cells
    len: u32,
    last_cell_width: Option<NonZeroU8>,
}

#[cfg(feature = "use_serde")]
fn deserialize_bitset<'de, D>(deserializer: D) -> Result<Option<Box<FixedBitSet>>, D::Error>
where
    D: Deserializer<'de>,
{
    let wide_indices = <Vec<usize>>::deserialize(deserializer)?;
    if wide_indices.is_empty() {
        Ok(None)
    } else {
        let max_idx = wide_indices.iter().max().unwrap_or(&1);
        let mut bitset = FixedBitSet::with_capacity(max_idx + 1);
        for idx in wide_indices {
            bitset.set(idx, true);
        }
        Ok(Some(Box::new(bitset)))
    }
}

/// Serialize the bitset as a vector of the indices of just the 1 bits;
/// the thesis is that most of the cells on a given line are single width.
/// That may not be strictly true for users that heavily use asian scripts,
/// but we'll start with this and see if we need to improve it.
#[cfg(feature = "use_serde")]
fn serialize_bitset<S>(value: &Option<Box<FixedBitSet>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut wide_indices: Vec<usize> = vec![];
    if let Some(bits) = value {
        for idx in bits.ones() {
            wide_indices.push(idx);
        }
    }
    wide_indices.serialize(serializer)
}

impl ClusteredLine {
    pub fn new() -> Self {
        Self {
            text: String::with_capacity(80),
            is_double_wide: None,
            clusters: vec![],
            len: 0,
            last_cell_width: None,
        }
    }

    pub fn to_cell_vec(&self) -> Vec<Cell> {
        let mut cells = vec![];

        for c in self.iter() {
            cells.push(c.as_cell());
            for _ in 1..c.width() {
                cells.push(Cell::blank_with_attrs(c.attrs().clone()));
            }
        }

        cells
    }

    pub fn from_cell_vec<'a>(hint: usize, iter: impl Iterator<Item = CellRef<'a>>) -> Self {
        let mut last_cluster: Option<Cluster> = None;
        let mut is_double_wide = FixedBitSet::with_capacity(hint);
        let mut text = String::new();
        let mut clusters = vec![];
        let mut any_double = false;
        let mut len = 0;
        let mut last_cell_width = None;

        for cell in iter {
            len += cell.width();
            last_cell_width = NonZeroU8::new(1);

            if cell.width() > 1 {
                any_double = true;
                is_double_wide.set(cell.cell_index(), true);
            }

            text.push_str(cell.str());

            last_cluster = match last_cluster.take() {
                None => Some(Cluster {
                    cell_width: cell.width() as u16,
                    attrs: cell.attrs().clone(),
                }),
                Some(cluster) if cluster.attrs != *cell.attrs() => {
                    clusters.push(cluster);
                    Some(Cluster {
                        cell_width: cell.width() as u16,
                        attrs: cell.attrs().clone(),
                    })
                }
                Some(mut cluster) => {
                    cluster.cell_width += cell.width() as u16;
                    Some(cluster)
                }
            };
        }

        if let Some(cluster) = last_cluster.take() {
            clusters.push(cluster);
        }

        Self {
            text,
            is_double_wide: if any_double {
                Some(Box::new(is_double_wide))
            } else {
                None
            },
            clusters,
            len: len.try_into().unwrap(),
            last_cell_width,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    fn is_double_wide(&self, cell_index: usize) -> bool {
        match &self.is_double_wide {
            Some(bitset) => bitset.contains(cell_index),
            None => false,
        }
    }

    /// LBTerm: whether any run of cells has attributes `pred` holds for, a look at each run
    /// rather than a cell at a time (Line::has_semantic_type)
    pub fn any_attrs(&self, pred: impl Fn(&CellAttributes) -> bool) -> bool {
        self.clusters.iter().any(|cluster| pred(&cluster.attrs))
    }

    pub fn iter(&self) -> ClusterLineCellIter<'_> {
        let mut clusters = self.clusters.iter();
        let cluster = clusters.next();
        ClusterLineCellIter {
            rest: &self.text,
            clusters,
            cluster,
            idx: 0,
            cluster_total: 0,
            line: self,
        }
    }

    /// LBTerm: `attrs` by reference, copied only when they start a new cluster: the printer
    /// hands its pen over for every character, and a copy of it each time was an allocation for
    /// a pen with a 24-bit colour
    pub fn append_grapheme(&mut self, text: &str, cell_width: usize, attrs: &CellAttributes) {
        let cell_width = cell_width as u16;
        let new_cluster = match self.clusters.last() {
            Some(cluster) => {
                if cluster.attrs != *attrs {
                    true
                } else {
                    // If we overflow the max length of a run,
                    // then we need a new cluster
                    let (_, did_overflow) = cluster.cell_width.overflowing_add(cell_width);
                    did_overflow
                }
            }
            None => true,
        };
        let new_cell_index = self.len as usize;
        if new_cluster {
            self.clusters.push(Cluster {
                attrs: attrs.clone(),
                cell_width,
            });
        } else if let Some(cluster) = self.clusters.last_mut() {
            cluster.cell_width += cell_width;
        }
        self.text.push_str(text);

        if cell_width > 1 {
            let bitset = match self.is_double_wide.take() {
                Some(mut bitset) => {
                    bitset.grow(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    bitset
                }
                None => {
                    let mut bitset = FixedBitSet::with_capacity(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    Box::new(bitset)
                }
            };
            self.is_double_wide.replace(bitset);
        }
        self.last_cell_width = NonZeroU8::new(cell_width as u8);
        self.len += cell_width as u32;
    }

    /// LBTerm: `text`, printable ASCII, appended a cell a character, all of it in `attrs`: what
    /// append_grapheme does for each character, done for the run at once
    pub fn append_ascii(&mut self, text: &str, attrs: &CellAttributes) {
        debug_assert!(text.bytes().all(|b| (0x20..0x7f).contains(&b)));
        // A cluster's width is a u16: a run longer than that goes in pieces
        for piece in text.as_bytes().chunks(u16::MAX as usize) {
            let n = piece.len() as u16;
            let new_cluster = match self.clusters.last() {
                Some(cluster) => cluster.attrs != *attrs || cluster.cell_width.checked_add(n).is_none(),
                None => true,
            };
            if new_cluster {
                self.clusters.push(Cluster {
                    attrs: attrs.clone(),
                    cell_width: n,
                });
            } else if let Some(cluster) = self.clusters.last_mut() {
                cluster.cell_width += n;
            }
            self.len += n as u32;
        }
        self.text.push_str(text);
        self.last_cell_width = NonZeroU8::new(1);
    }

    pub fn append(&mut self, cell: Cell) {
        let cell_width = cell.width() as u16;
        let new_cluster = match self.clusters.last() {
            Some(cluster) => {
                if cluster.attrs != *cell.attrs() {
                    true
                } else {
                    // If we overflow the max length of a run,
                    // then we need a new cluster
                    let (_, did_overflow) = cluster.cell_width.overflowing_add(cell_width);
                    did_overflow
                }
            }
            None => true,
        };
        let new_cell_index = self.len as usize;
        if new_cluster {
            self.clusters.push(Cluster {
                attrs: (*cell.attrs()).clone(),
                cell_width,
            });
        } else if let Some(cluster) = self.clusters.last_mut() {
            cluster.cell_width += cell_width;
        }
        self.text.push_str(cell.str());

        if cell_width > 1 {
            let bitset = match self.is_double_wide.take() {
                Some(mut bitset) => {
                    bitset.grow(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    bitset
                }
                None => {
                    let mut bitset = FixedBitSet::with_capacity(new_cell_index + 1);
                    bitset.set(new_cell_index, true);
                    Box::new(bitset)
                }
            };
            self.is_double_wide.replace(bitset);
        }
        self.last_cell_width = NonZeroU8::new(cell_width as u8);
        self.len += cell_width as u32;
    }

    pub fn prune_trailing_blanks(&mut self) -> bool {
        let num_spaces = self.text.chars().rev().take_while(|&c| c == ' ').count();
        if num_spaces == 0 {
            return false;
        }

        let blank = CellAttributes::blank();
        let mut pruned = false;
        for _ in 0..num_spaces {
            let mut need_pop = false;
            if let Some(cluster) = self.clusters.last_mut() {
                if cluster.attrs != blank {
                    break;
                }
                cluster.cell_width -= 1;
                self.text.pop();
                self.len -= 1;
                self.last_cell_width.take();
                pruned = true;
                if cluster.cell_width == 0 {
                    need_pop = true;
                }
            }
            if need_pop {
                self.clusters.pop();
            }
        }

        pruned
    }

    fn compute_last_cell_width(&mut self) -> Option<NonZeroU8> {
        if self.last_cell_width.is_none() {
            if let Some(last_cell) = self.iter().last() {
                self.last_cell_width = NonZeroU8::new(last_cell.width() as u8);
            }
        }
        self.last_cell_width
    }

    pub fn set_last_cell_was_wrapped(&mut self, wrapped: bool) {
        if let Some(width) = self.compute_last_cell_width() {
            let width = width.get() as u16;
            if let Some(last_cluster) = self.clusters.last_mut() {
                let mut attrs = last_cluster.attrs.clone();
                attrs.set_wrapped(wrapped);

                if last_cluster.cell_width == width {
                    // Re-purpose final cluster
                    last_cluster.attrs = attrs;
                } else {
                    last_cluster.cell_width -= width;
                    self.clusters.push(Cluster {
                        cell_width: width,
                        attrs,
                    });
                }
            }
        }
    }
}

/// LBTerm: what a resize's rewrap (`Screen::rewrap_lines`) does to a clustered line, a cluster at
/// a time rather than a cell at a time (lbterm/MUX.md, "A window's resize"; the audit's core-9).
/// Upstream joined a wrapped line's rows by turning each cell of the next into a `Cell` and
/// appending it (`append`), and cut a line into rows by collecting its cells into a vector and
/// writing each into a new line (`Line::wrap`), and it found whether a line was wrapped by walking
/// to its last cell: with 50,000 lines of scrollback, a third of them longer than the window, about
/// 70 ms of the mux server's main thread a pane each time the window's width changed, most of it in
/// the allocator. These leave each line as those did, the same text, runs, double-wide cells,
/// length and last cell (line.rs's tests hold them to the cell-at-a-time ways), and a line of
/// `u16::MAX` cells or more, where a run's width could overflow, still goes a cell at a time
impl ClusteredLine {
    /// Whether the last cell is marked wrapped: a cell's attributes are its run's, and the runs
    /// cover the cells in order, so the last cell's are the last run's
    pub fn lbterm_last_wrapped(&self) -> bool {
        !self.text.is_empty() && self.clusters.last().map_or(false, |c| c.attrs.wrapped())
    }

    /// The last cell's width, as `compute_last_cell_width` finds it, without the walk: a
    /// double-wide cell two cells from the end is the last
    fn lbterm_last_width(&self) -> Option<NonZeroU8> {
        if self.last_cell_width.is_some() || self.len == 0 {
            return self.last_cell_width;
        }
        let wide = self.len >= 2 && self.is_double_wide(self.len as usize - 2);
        NonZeroU8::new(if wide { 2 } else { 1 })
    }

    /// A run of `width` cells in `attrs` after the last, as `append_grapheme` adds them a cell at a
    /// time: onto the last run when it has the same attributes, else a run of its own. The
    /// callers keep the line under `u16::MAX` cells, so a run can't overflow
    fn lbterm_push_run(&mut self, attrs: &CellAttributes, width: u16) {
        if width == 0 {
            return;
        }
        match self.clusters.last_mut() {
            Some(last) if last.attrs == *attrs => last.cell_width += width,
            _ => self.clusters.push(Cluster {
                attrs: attrs.clone(),
                cell_width: width,
            }),
        }
    }

    /// Cell `idx` marked double-wide, the set grown as `append` grows it
    fn lbterm_set_wide(&mut self, idx: usize) {
        let bitset = match self.is_double_wide.take() {
            Some(mut bitset) => {
                bitset.grow(idx + 1);
                bitset.set(idx, true);
                bitset
            }
            None => {
                let mut bitset = FixedBitSet::with_capacity(idx + 1);
                bitset.set(idx, true);
                Box::new(bitset)
            }
        };
        self.is_double_wide.replace(bitset);
    }

    /// `other`'s cells after this line's, as `append` a cell at a time leaves it
    pub fn lbterm_append_line(&mut self, other: &ClusteredLine) {
        if self.len as usize + other.len as usize >= u16::MAX as usize {
            for cell in other.iter() {
                self.append(cell.as_cell());
            }
            return;
        }
        if other.text.is_empty() {
            return;
        }
        let base = self.len as usize;
        for c in &other.clusters {
            self.lbterm_push_run(&c.attrs, c.cell_width);
        }
        self.text.push_str(&other.text);
        if let Some(bits) = &other.is_double_wide {
            for idx in bits.ones() {
                self.lbterm_set_wide(base + idx);
            }
        }
        self.len += other.len;
        self.last_cell_width = other.lbterm_last_width();
    }

    /// The line cut into rows of at most `width` cells, its trailing spaces left off, as
    /// `Line::wrap` cut it a cell at a time: each row's cells, runs and double-wide cells, none of
    /// them yet marked wrapped. Empty when the line is only spaces. None where the cell-at-a-time
    /// way is kept: a width under 2 (where a double-wide cell gets a row of its own and an empty
    /// one before it) or a line of `u16::MAX` cells or more
    pub fn lbterm_wrap(&self, width: usize) -> Option<Vec<ClusteredLine>> {
        if width < 2 || self.len as usize >= u16::MAX as usize {
            return None;
        }
        let base = self.text.as_ptr() as usize;
        let offset = |cell: &CellRef| cell.str().as_ptr() as usize - base;
        // Where the last cell that isn't a space ends, in bytes: a cell of a space alone is a
        // " " grapheme, and a space a combining mark follows is in a cell of its own that isn't,
        // so it's where the text ends without its trailing spaces
        let end = self.text.trim_end_matches(' ').len();
        if end == 0 {
            return Some(vec![]);
        }
        // Printable ASCII with no double-wide cell: a byte a cell, so a row is `width` bytes,
        // with no walk through the cells (the cell iterator's own fast path, for the whole line)
        if self.is_double_wide.is_none()
            && self.text.as_bytes()[..end]
                .iter()
                .all(|b| (0x20..0x7f).contains(b))
        {
            let mut rows = Vec::with_capacity((end + width - 1) / width);
            let mut at = 0;
            while at < end {
                let to = (at + width).min(end);
                rows.push(self.lbterm_cells(at..to, at, to - at, 1));
                at = to;
            }
            return Some(rows);
        }
        // Each row: its first byte and cell, and its cells' width so far and its last cell's
        let mut rows = vec![];
        let mut row: Option<(usize, usize, usize, usize)> = None;
        for cell in self.iter() {
            let at = offset(&cell);
            if at >= end {
                break;
            }
            let w = cell.width();
            row = match row {
                Some((byte, first, len, _)) if len + w <= width => Some((byte, first, len + w, w)),
                done => {
                    if let Some((byte, first, len, last)) = done {
                        rows.push(self.lbterm_cells(byte..at, first, len, last));
                    }
                    Some((at, cell.cell_index(), w, w))
                }
            };
        }
        if let Some((byte, first, len, last)) = row {
            rows.push(self.lbterm_cells(byte..end, first, len, last));
        }
        Some(rows)
    }

    /// The `len` cells from cell `first` (bytes `bytes` of the text) as a line of their own, the
    /// last `last` wide
    fn lbterm_cells(
        &self,
        bytes: core::ops::Range<usize>,
        first: usize,
        len: usize,
        last: usize,
    ) -> ClusteredLine {
        let mut line = ClusteredLine::new();
        line.text.push_str(&self.text[bytes]);
        let end = first + len;
        let mut at = 0usize;
        for c in &self.clusters {
            let (from, to) = (at, at + c.cell_width as usize);
            at = to;
            let width = to.min(end).saturating_sub(from.max(first));
            if width > 0 {
                line.lbterm_push_run(&c.attrs, width as u16);
            }
            if at >= end {
                break;
            }
        }
        if let Some(bits) = &self.is_double_wide {
            for idx in bits.ones() {
                if idx >= end {
                    break;
                }
                if idx >= first {
                    line.lbterm_set_wide(idx - first);
                }
            }
        }
        line.len = len as u32;
        line.last_cell_width = NonZeroU8::new(last as u8);
        line
    }
}

pub(crate) struct ClusterLineCellIter<'a> {
    /// LBTerm: the text not yet iterated, in place of a grapheme iterator over all of it
    rest: &'a str,
    clusters: core::slice::Iter<'a, Cluster>,
    cluster: Option<&'a Cluster>,
    idx: usize,
    cluster_total: usize,
    line: &'a ClusteredLine,
}

impl<'a> Iterator for ClusterLineCellIter<'a> {
    type Item = CellRef<'a>;

    fn next(&mut self) -> Option<CellRef<'a>> {
        // LBTerm: printable ASCII before more ASCII, or the line's end, is a grapheme of its own
        // (only CR LF joins two ASCII characters, and neither is printed), so only the rest asks
        // the segmenter. Every walk of a line's cells went through it, a character at a time:
        // its zones, its shape hash, its triggers, its drawing
        let bytes = self.rest.as_bytes();
        let first = *bytes.first()?;
        let len = if (0x20..0x7f).contains(&first) && bytes.get(1).map_or(true, |b| *b < 0x80) {
            1
        } else {
            Graphemes::new(self.rest).next()?.len()
        };
        let (text, rest) = self.rest.split_at(len);
        self.rest = rest;

        let cell_index = self.idx;
        let width = if self.line.is_double_wide(cell_index) {
            2
        } else {
            1
        };
        self.idx += width;
        self.cluster_total += width;
        let attrs = &self.cluster.as_ref()?.attrs;

        if self.cluster_total >= self.cluster.as_ref()?.cell_width as usize {
            self.cluster = self.clusters.next();
            self.cluster_total = 0;
        }

        Some(CellRef::ClusterRef {
            cell_index,
            width,
            text,
            attrs,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn memory_usage() {
        assert_eq!(core::mem::size_of::<ClusteredLine>(), 64);
        assert_eq!(core::mem::size_of::<String>(), 24);
        assert_eq!(core::mem::size_of::<Vec<Cluster>>(), 24);
        assert_eq!(core::mem::size_of::<Option<Box<FixedBitSet>>>(), 8);
        assert_eq!(core::mem::size_of::<Option<NonZeroU8>>(), 1);
    }
}
