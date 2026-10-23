//! LBTerm: ConEmu's progress sequence (OSC 9;4), which the taskbar button shows

use super::*;
use k9::assert_equal as assert_eq;

#[test]
fn each_state_reaches_the_pane() {
    let mut term = TestTerm::new(3, 20, 0);
    term.print("\x1b]9;4;1;40\x1b\\");
    assert_eq!(term.get_progress(), Progress::Percentage(40));
    term.print("\x1b]9;4;2;75\x1b\\");
    assert_eq!(term.get_progress(), Progress::Error(75));
    term.print("\x1b]9;4;3\x1b\\");
    assert_eq!(term.get_progress(), Progress::Indeterminate);
    term.print("\x1b]9;4;0\x1b\\");
    assert_eq!(term.get_progress(), Progress::None);
}

#[test]
fn paused_is_kept_rather_than_dropped() {
    let mut term = TestTerm::new(3, 20, 0);
    term.print("\x1b]9;4;1;40\x1b\\");
    term.print("\x1b]9;4;4\x1b\\");
    assert_eq!(term.get_progress(), Progress::Paused);
}
