//! LBTerm: kitty's notifications (OSC 99) through the whole terminal: the answer to a query,
//! and the commands the GUI is handed

use super::*;
use k9::assert_equal as assert_eq;
use lbterm_notify::Command;
use std::time::{Duration, Instant};

/// A pty writer whose bytes the test can read
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
    /// The terminal writes from a thread of its own: wait for `want`
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
    fn commands(&self) -> Vec<Command> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|a| match a {
                Alert::KittyNotification(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
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
fn a_query_is_answered_by_the_terminal() {
    let (mut term, written, alerts) = terminal();
    term.advance_bytes("\x1b]99;i=q1:p=?;\x1b\\");
    let reply = written.wait_for("\x1b\\");
    assert!(reply.starts_with("\x1b]99;i=q1:p=?;"), "{reply:?}");
    assert!(reply.contains("a=focus,report"), "{reply:?}");
    assert!(alerts.commands().is_empty());
}

#[test]
fn a_notification_in_chunks_reaches_the_gui_once_whole() {
    let (mut term, _, alerts) = terminal();
    term.advance_bytes("\x1b]99;i=n1:d=0:a=report;Claude Code\x1b\\");
    assert!(alerts.commands().is_empty());
    term.advance_bytes("\x1b]99;i=n1:p=body;Waiting for you\x1b\\");
    let commands = alerts.commands();
    assert_eq!(commands.len(), 1);
    match &commands[0] {
        Command::Show(n) => {
            assert_eq!(n.title, "Claude Code");
            assert_eq!(n.body, "Waiting for you");
            assert!(n.actions.report && n.actions.focus);
        }
        other => panic!("expected Show, got {other:?}"),
    }
    // Nothing is written back until the person does something with it
}

#[test]
fn close_and_alive_go_to_the_gui() {
    let (mut term, _, alerts) = terminal();
    term.advance_bytes("\x1b]99;i=n1:p=close;\x1b\\\x1b]99;i=me:p=alive;\x07");
    assert_eq!(
        alerts.commands(),
        vec![Command::Close("n1".into()), Command::Alive("me".into())]
    );
}

#[test]
fn other_unknown_oscs_are_left_alone() {
    let (mut term, written, alerts) = terminal();
    term.advance_bytes("\x1b]98;nothing\x1b\\hello");
    assert!(alerts.commands().is_empty());
    assert_eq!(written.wait_for("never"), "");
}
