//! LBTerm: get_changed_stable_rows reads only the lines that can have changed since a seqno
//! (lbterm_changes.rs), and finds the same rows reading every line does.

use super::*;

/// The rows upstream's get_changed_stable_rows found: every line of the range read
fn changed_reading_every_line(
    screen: &Screen,
    range: Range<StableRowIndex>,
    seqno: SequenceNo,
) -> Vec<StableRowIndex> {
    let phys = screen.stable_range(&range);
    screen
        .lines_in_phys_range(phys.clone())
        .iter()
        .enumerate()
        .filter(|(_, line)| line.changed_since(seqno))
        .map(|(i, _)| screen.phys_to_stable_row_index(phys.start + i))
        .collect()
}

/// What the mux server asks: from the top of the scrollback to the bottom of the viewport
fn all_rows(term: &TestTerm) -> Range<StableRowIndex> {
    let screen = term.screen();
    0..screen.visible_row_to_stable_row(screen.physical_rows as VisibleRowIndex)
}

fn assert_same_rows(term: &TestTerm, since: &[SequenceNo], step: &str) {
    let screen = term.screen();
    let range = all_rows(term);
    for &seqno in since {
        let got = screen.get_changed_stable_rows(range.clone(), seqno);
        let want = changed_reading_every_line(screen, range.clone(), seqno);
        std::assert!(
            got == want,
            "after {step}, the rows changed since {seqno} (now {}): {got:?}, reading every line {want:?}",
            term.current_seqno()
        );
    }
}

/// A small xorshift, so the steps are the same each run
struct Steps(u64);
impl Steps {
    fn next(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

fn size(rows: usize, cols: usize) -> TerminalSize {
    TerminalSize {
        rows,
        cols,
        pixel_width: cols * 8,
        pixel_height: rows * 16,
        dpi: 0,
    }
}

#[test]
fn the_rows_changed_are_those_reading_every_line_finds() {
    // Each a run of 4,000 steps of its own. The first found the alternate screen left taller by
    // a resize, whose lines move up a row as one leaves its top
    for seed in [0x9E37_79B9_7F4A_7C15, 0x2545_F491_4F6C_DD1D, 0xD6E8_FEB8_6659_FD93] {
        steps_against_reading_every_line(seed);
    }
}

fn steps_against_reading_every_line(seed: u64) {
    // Scrollback small enough to fill, so lines leave its top too
    let mut term = TestTerm::new(8, 20, 60);
    let mut steps = Steps(seed);
    let mut seen: Vec<SequenceNo> = vec![0, 1];
    for i in 0..4000 {
        let step = match steps.next(17) {
            0..=3 => {
                term.print(format!("line {i} of output\r\n"));
                "a line"
            }
            4 => {
                term.print("a long line that wraps past the edge of the screen more than once\r\n");
                "a wrapped line"
            }
            5 => {
                let (x, y) = (steps.next(20) as isize, steps.next(8) as isize);
                term.cup(x, y);
                term.print("ab");
                "a write in the viewport"
            }
            6 => {
                term.set_scroll_region(2, 6);
                term.cup(0, 6);
                term.print("in a region\r\n\r\n");
                term.print(format!("{}", CSI));
                term.print("r");
                "a scroll region"
            }
            7 => {
                term.cup(0, 0);
                term.print("\x1bM");
                "a reverse index"
            }
            8 => {
                term.cup(0, steps.next(8) as isize);
                term.print(format!("{}{}L", CSI, 1 + steps.next(3)));
                term.print(format!("{}{}M", CSI, 1 + steps.next(3)));
                "lines inserted and deleted"
            }
            9 => {
                term.print(format!("{}?1049h", CSI));
                term.print("on the alternate screen\r\n");
                "the alternate screen"
            }
            10 => {
                term.print(format!("{}?1049l", CSI));
                "the primary screen"
            }
            11 => {
                let (rows, cols) = (6 + steps.next(5) as usize, 14 + steps.next(12) as usize);
                term.resize(size(rows, cols));
                "a resize"
            }
            12 => {
                term.print(format!("{}2J", CSI));
                "the display erased"
            }
            13 => {
                if steps.next(4) == 0 {
                    term.print(format!("{}3J", CSI));
                    "the scrollback erased"
                } else {
                    term.print("\x1b]4;1;rgb:ff/00/00\x1b\\");
                    "a palette change"
                }
            }
            14 => {
                // Lines handed out above the viewport, as a hyperlink rule's pass can change them
                let top = term.screen().visible_row_to_stable_row(0);
                let row = top - 1 - steps.next(30) as StableRowIndex;
                let seqno = term.current_seqno();
                let screen = term.screen_mut();
                let phys = screen.stable_range(&(row..row + 2));
                screen.with_phys_lines_mut(phys, |lines| {
                    if let Some(line) = lines.first_mut() {
                        line.update_last_change_seqno(seqno);
                    }
                });
                "a line changed above the viewport"
            }
            15 => {
                let seqno = term.current_seqno();
                let pick = steps.next(40) as usize;
                term.screen_mut().for_each_phys_line_mut(|idx, line| {
                    if idx == pick {
                        line.update_last_change_seqno(seqno);
                    }
                });
                "one line changed by a pass over all of them"
            }
            _ => {
                term.print("x");
                "a character"
            }
        };
        assert_same_rows(&term, &seen, step);
        // Asked since seqnos old and new, as clients that last looked then would
        let now = term.current_seqno();
        seen.push(now);
        if seen.len() > 40 {
            seen.remove(2);
        }
    }
}

#[test]
fn a_push_after_a_line_reads_the_viewport_not_the_scrollback() {
    let mut term = TestTerm::new(40, 80, 50_000);
    let mut text = String::new();
    for i in 0..50_100 {
        text.push_str(&format!("history line {i}\r\n"));
    }
    term.print(text);
    let screen = term.screen();
    std::assert_eq!(screen.scrollback_rows(), 50_040, "the scrollback is full");

    // A client sent everything up to here, then a line is printed
    let seqno = term.current_seqno();
    term.print("a new line\r\n");
    let range = all_rows(&term);
    let screen = term.screen();
    let read = screen.lbterm_rows_to_read(&range, seqno);
    std::assert!(
        read.end - read.start <= 41,
        "read {} lines ({read:?}) for a line, of {}",
        read.end - read.start,
        screen.scrollback_rows()
    );
    std::assert_eq!(
        screen.get_changed_stable_rows(range.clone(), seqno),
        changed_reading_every_line(screen, range, seqno)
    );

    // A key's echo, the cursor's line changed and nothing scrolled
    let seqno = term.current_seqno();
    term.print("x");
    let range = all_rows(&term);
    let screen = term.screen();
    let read = screen.lbterm_rows_to_read(&range, seqno);
    assert!(read.end - read.start <= 40, "read {read:?} for a key's echo");
    let changed = screen.get_changed_stable_rows(range.clone(), seqno);
    std::assert_eq!(changed, changed_reading_every_line(screen, range, seqno));
    std::assert_eq!(changed.len(), 1, "only the cursor's line changed: {changed:?}");
}

#[test]
fn a_seqno_older_than_what_is_kept_reads_everything() {
    let mut term = TestTerm::new(4, 20, 2_000);
    let seqno = term.current_seqno();
    for i in 0..(crate::lbterm_changes::KEPT + 10) {
        term.print(format!("line {i}\r\n"));
    }
    let range = all_rows(&term);
    let screen = term.screen();
    let read = screen.lbterm_rows_to_read(&range, seqno);
    std::assert_eq!(read, screen.stable_range(&range));
    std::assert_eq!(
        screen.get_changed_stable_rows(range.clone(), seqno),
        changed_reading_every_line(screen, range, seqno)
    );
}

#[test]
fn switching_back_from_the_alternate_screen_reads_the_top_of_the_scrollback() {
    // Upstream marks the primary's first rows changed as it comes back, for the mux's clients'
    // caches (dirty_top_phys_rows): those are read and found as before
    let mut term = TestTerm::new(4, 20, 100);
    for i in 0..50 {
        term.print(format!("line {i}\r\n"));
    }
    let seqno = term.current_seqno();
    term.print(format!("{}?1049h", CSI));
    term.print("alt\r\n");
    term.print(format!("{}?1049l", CSI));
    let range = all_rows(&term);
    let screen = term.screen();
    let changed = screen.get_changed_stable_rows(range.clone(), seqno);
    std::assert_eq!(changed, changed_reading_every_line(screen, range, seqno));
    std::assert!(
        changed.contains(&screen.phys_to_stable_row_index(0)),
        "the top of the scrollback is among them: {changed:?}"
    );
}
