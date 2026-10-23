//! LBTerm: OSC 7501 through the whole terminal: the answer to a query, the
//! records reports leave, and what ends them.

use super::*;
use crate::program_status::ProgramState;
use k9::assert_equal as assert_eq;
use std::time::{Duration, Instant};

/// A pty writer whose bytes the test can read.
#[derive(Clone, Default)]
struct Written(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Written {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Written {
    /// The terminal writes from a thread of its own: wait for `want`.
    fn wait_for(&self, want: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let text = String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned();
            if text.contains(want) || Instant::now() > deadline {
                return text;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[derive(Clone, Default)]
struct Alerts(Arc<Mutex<Vec<Alert>>>);

impl AlertHandler for Alerts {
    fn alert(&mut self, alert: Alert) {
        self.0.lock().unwrap().push(alert);
    }
}

impl Alerts {
    fn program_status(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|a| **a == Alert::ProgramStatus)
            .count()
    }
}

fn terminal() -> (Terminal, Written, Alerts) {
    let written = Written::default();
    let mut term = Terminal::new(
        TerminalSize {
            rows: 5,
            cols: 20,
            pixel_width: 160,
            pixel_height: 80,
            dpi: 0,
        },
        Arc::new(TestTermConfig { scrollback: 0 }),
        "WezTerm",
        "O_o",
        Box::new(written.clone()),
    );
    let alerts = Alerts::default();
    term.set_notification_handler(Box::new(alerts.clone()));
    (term, written, alerts)
}

#[test]
fn a_query_gets_the_answer_and_nothing_else() {
    let (mut term, written, _) = terminal();
    term.advance_bytes("\x1b]7501;?\x1b\\");
    assert_eq!(written.wait_for("7501"), "\x1b]7501;?\x1b\\");
    // With BEL too, as Claude Code sends it
    term.advance_bytes("\x1b]7501;?\x07");
    assert_eq!(written.wait_for("?\x1b\\\x1b]"), "\x1b]7501;?\x1b\\\x1b]7501;?\x1b\\");
}

#[test]
fn reports_become_records_and_tell_the_gui() {
    let (mut term, _, alerts) = terminal();
    // "Allow?" in base64
    term.advance_bytes("\x1b]7501;state=blocked:kind=permission:app=claude-code:msg=QWxsb3c/\x07");
    term.advance_bytes("\x1b]7501;state=working:id=agent/1\x1b\\");
    let records = term.program_status();
    assert_eq!(records.len(), 2);
    assert_eq!((records[0].id.as_str(), records[0].state), ("", ProgramState::Blocked));
    assert_eq!(records[0].msg.as_deref(), Some("Allow?"));
    // Inherited from the root
    assert_eq!(records[1].app.as_deref(), Some("claude-code"));
    assert_eq!(alerts.program_status(), 2);
    // A report that breaks the rules changes nothing and says nothing
    term.advance_bytes("\x1b]7501;state=napping\x07");
    assert_eq!(term.program_status().len(), 2);
    assert_eq!(alerts.program_status(), 2);
}

#[test]
fn a_new_prompt_ends_what_was_working_or_blocked() {
    let (mut term, _, alerts) = terminal();
    term.advance_bytes("\x1b]7501;state=working\x07\x1b]7501;state=done:id=build\x07");
    term.advance_bytes("\x1b]133;A\x1b\\");
    let records = term.program_status();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, ProgramState::Done);
    assert_eq!(alerts.program_status(), 3);
    // A prompt with nothing to end says nothing
    term.advance_bytes("\x1b]133;A\x1b\\");
    assert_eq!(alerts.program_status(), 3);
}

#[test]
fn a_full_reset_ends_everything_and_clear_ends_a_branch() {
    let (mut term, _, _) = terminal();
    term.advance_bytes("\x1b]7501;state=done\x07\x1b]7501;state=error:id=a/b\x07");
    term.advance_bytes("\x1b]7501;state=clear:id=a\x07");
    assert_eq!(term.program_status().len(), 1);
    term.advance_bytes("\x1bc");
    assert!(term.program_status().is_empty());
}
