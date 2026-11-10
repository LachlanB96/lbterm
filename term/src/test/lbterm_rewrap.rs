//! LBTerm: a resize's rewrap marks changed only the lines it joins, cuts or moves to another row
//! (Screen::rewrap_lines), where upstream marked every line of the scrollback; every row whose
//! text is new is still said changed

use super::*;
use std::collections::{HashMap, HashSet};

fn size(cols: usize) -> TerminalSize {
    TerminalSize {
        rows: 20,
        cols,
        pixel_width: cols * 8,
        pixel_height: 20 * 16,
        dpi: 0,
    }
}

/// Each stable row's text, the whole scrollback and screen
fn rows(term: &TestTerm) -> HashMap<StableRowIndex, String> {
    let screen = term.screen();
    let n = screen.scrollback_rows();
    screen
        .lines_in_phys_range(0..n)
        .into_iter()
        .enumerate()
        .map(|(phys, line)| {
            (
                screen.phys_to_stable_row_index(phys),
                line.as_str().trim_end().to_string(),
            )
        })
        .collect()
}

/// The rows said changed since `seqno`, the whole scrollback asked
fn changed_since(term: &TestTerm, seqno: SequenceNo) -> HashSet<StableRowIndex> {
    let screen = term.screen();
    let all = screen.phys_to_stable_row_index(0)
        ..screen.phys_to_stable_row_index(screen.scrollback_rows());
    screen
        .get_changed_stable_rows(all, seqno)
        .into_iter()
        .collect()
}

/// Resized to `cols`: every row whose text isn't what that row showed before is said changed,
/// and how many rows are said changed
fn resize_and_check(term: &mut TestTerm, cols: usize, what: &str) -> usize {
    let before = rows(term);
    let seqno = term.current_seqno();
    term.resize(size(cols));
    let after = rows(term);
    let changed = changed_since(term, seqno);
    for (row, text) in &after {
        if before.get(row) != Some(text) {
            std::assert!(
                changed.contains(row),
                "{what}, to {cols} columns: row {row} reads {text:?} where it read {:?}, and isn't said changed",
                before.get(row)
            );
        }
    }
    changed.len()
}

#[test]
fn short_lines_keep_their_rows_and_arent_marked() {
    let mut term = TestTerm::new(20, 80, 5_000);
    let mut text = String::new();
    for i in 0..3_000 {
        text.push_str(&format!("line {i} of a log\r\n"));
    }
    term.print(&text);
    let rows = term.screen().scrollback_rows();
    let changed = resize_and_check(&mut term, 60, "short lines");
    // The screen at the bottom moves with the cursor; the scrollback above it stays
    std::assert!(
        changed <= 2 * 20,
        "{} of {} rows said changed where only the screen's could be",
        changed,
        rows
    );
    resize_and_check(&mut term, 80, "short lines, back");
}

#[test]
fn long_lines_cut_and_joined_mark_what_moved() {
    let mut term = TestTerm::new(20, 80, 5_000);
    let mut text = String::new();
    for i in 0..1_500 {
        if i % 3 == 0 {
            text.push_str(&format!("line {i} {}\r\n", "y".repeat(110)));
        } else {
            text.push_str(&format!("line {i} of a log\r\n"));
        }
    }
    term.print(&text);
    for cols in [79, 60, 100, 80, 81, 200, 80] {
        resize_and_check(&mut term, cols, "long lines");
    }
}

#[test]
fn rows_before_the_first_line_cut_keep_their_mark() {
    let mut term = TestTerm::new(20, 80, 5_000);
    let mut text = String::new();
    for i in 0..1_000 {
        text.push_str(&format!("line {i} of a log\r\n"));
    }
    // One long line, wrapped by the terminal at 80, and short ones after it
    text.push_str(&format!("{}\r\n", "z".repeat(150)));
    for i in 0..200 {
        text.push_str(&format!("after {i}\r\n"));
    }
    term.print(&text);
    let screen = term.screen();
    let first = screen.phys_to_stable_row_index(0);
    let long = first + 1_000;
    let seqno = term.current_seqno();
    resize_and_check(&mut term, 60, "one long line");
    let changed = changed_since(&term, seqno);
    std::assert!(
        changed.iter().all(|row| *row >= long),
        "a row before the long line said changed: {:?}",
        changed.iter().filter(|row| **row < long).take(5).collect::<Vec<_>>()
    );
    std::assert!(
        changed.contains(&(long + 5)),
        "a row after the long line, moved down a row, isn't said changed"
    );
}
