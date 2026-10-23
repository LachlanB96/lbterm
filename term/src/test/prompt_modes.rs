//! LBTerm: a shell's prompt turns off the mouse and focus reporting a
//! program left on (TerminalState::release_mouse_and_focus).

use super::*;
use k9::assert_equal as assert_eq;

/// What a TUI asks for: any-event mouse in SGR, and focus reports
const TUI_MODES: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?1004h";

fn reporting(term: &TestTerm) -> (bool, bool, bool, bool, MouseEncoding) {
    (
        term.mouse_tracking_enabled(),
        term.button_event_mouse_enabled(),
        term.any_event_mouse_enabled(),
        term.focus_tracking_enabled(),
        term.get_mouse_encoding(),
    )
}

const OFF: (bool, bool, bool, bool, MouseEncoding) = (false, false, false, false, MouseEncoding::X10);

#[test]
fn a_prompt_turns_off_what_a_dead_program_left_on() {
    let mut term = TestTerm::new(5, 20, 0);
    term.print(TUI_MODES);
    assert_eq!(reporting(&term), (true, true, true, true, MouseEncoding::SGR));
    assert!(term.is_mouse_grabbed());
    // The program dies without turning them off; the shell's prompt comes back
    term.print("\x1b]133;A\x1b\\");
    assert_eq!(reporting(&term), OFF);
    assert!(!term.is_mouse_grabbed());
}

#[test]
fn the_end_of_a_command_with_a_new_prompt_does_it_too() {
    let mut term = TestTerm::new(5, 20, 0);
    term.print(TUI_MODES);
    // An exit status alone isn't a prompt yet
    term.print("\x1b]133;D;0\x1b\\");
    assert!(term.any_event_mouse_enabled());
    term.print("\x1b]133;N\x1b\\");
    assert_eq!(reporting(&term), OFF);
}

#[test]
fn a_program_that_is_still_running_keeps_them() {
    let mut term = TestTerm::new(5, 20, 0);
    // The prompt, then the command starts and asks for the mouse
    term.print("\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\claude\r\n\x1b]133;C\x1b\\");
    term.print(TUI_MODES);
    term.print("output while it runs\r\n");
    assert_eq!(reporting(&term), (true, true, true, true, MouseEncoding::SGR));
}
