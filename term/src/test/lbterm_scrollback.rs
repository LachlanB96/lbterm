//! LBTerm: a screen has room for its viewport when it's made and grows its scrollback as lines
//! go into it, never past what the scrollback holds (Screen::lbterm_reserve). Upstream reserved
//! the whole scrollback as each pane opened: 50,040 lines of room for Lachlan's 50,000 lines and
//! 40 rows, about 6.8 MB a pane before it printed anything.

use super::*;

#[test]
fn a_new_screen_has_room_for_its_viewport_not_its_scrollback() {
    let term = TestTerm::new(40, 80, 50_000);
    let room = term.screen().lbterm_capacity();
    std::assert!(room < 1_000, "room for {} lines in a new screen", room);
}

#[test]
fn the_scrollback_grows_as_it_fills_and_no_further() {
    let mut term = TestTerm::new(40, 80, 50_000);
    let mut text = String::new();
    for i in 0..5_000 {
        text.push_str(&format!("line {i}
"));
    }
    term.print(&text);
    let room = term.screen().lbterm_capacity();
    let rows = term.screen().scrollback_rows();
    std::assert!(
        room >= rows && room <= 2 * rows,
        "room for {} lines with {}",
        room,
        rows
    );

    for _ in 0..10 {
        term.print(&text);
    }
    std::assert_eq!(term.screen().scrollback_rows(), 50_040);
    std::assert_eq!(term.screen().lbterm_capacity(), 50_040, "a full scrollback has the room upstream reserved");

    // Kept full, it doesn't grow again
    term.print(&text);
    std::assert_eq!(term.screen().lbterm_capacity(), 50_040);
}

#[test]
fn a_resize_keeps_what_the_scrollback_has() {
    let mut term = TestTerm::new(40, 80, 50_000);
    let mut text = String::new();
    for i in 0..3_000 {
        text.push_str(&format!("line {i}
"));
    }
    term.print(&text);
    term.resize(TerminalSize {
        rows: 50,
        cols: 100,
        pixel_width: 800,
        pixel_height: 800,
        dpi: 0,
    });
    let rows = term.screen().scrollback_rows();
    std::assert!(rows >= 3_000, "{} lines after the resize", rows);
    term.print(&text);
    std::assert!(term.screen().scrollback_rows() >= rows + 3_000 - 50);
}
