//! LBTerm: a title or folder sequence raises an alert only for what it changed. Each alert is a
//! PDU to every client of a mux server and an update of every window's title, and Claude Code
//! sets its title every 960 ms while it works (OSC 0, which raised two), ConPTY passing on each
//! title set again unchanged too.

use super::*;
use k9::assert_equal as assert_eq;

#[derive(Clone, Default)]
struct Alerts(Arc<Mutex<Vec<Alert>>>, Arc<Mutex<Vec<String>>>);

impl AlertHandler for Alerts {
    fn alert(&mut self, alert: Alert) {
        self.0.lock().unwrap().push(alert);
    }

    fn title_set_again(&mut self, title: &str) {
        self.1.lock().unwrap().push(title.to_string());
    }
}

impl Alerts {
    /// The alerts raised since the last take
    fn take(&self) -> Vec<Alert> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    /// The titles set again unchanged since the last take
    fn again(&self) -> Vec<String> {
        std::mem::take(&mut *self.1.lock().unwrap())
    }
}

fn terminal() -> (TestTerm, Alerts) {
    let mut term = TestTerm::new(5, 20, 0);
    let alerts = Alerts::default();
    term.set_notification_handler(Box::new(alerts.clone()));
    (term, alerts)
}

#[test]
fn osc_0_raises_one_alert_for_a_new_title_and_none_for_the_same() {
    let (mut term, alerts) = terminal();
    term.print("\x1b]0;\u{25d0} work\x07");
    assert_eq!(alerts.take(), vec![Alert::WindowTitleChanged("\u{25d0} work".to_string())]);
    assert_eq!(term.get_title(), "\u{25d0} work");
    term.print("\x1b]0;\u{25d0} work\x07");
    assert_eq!(alerts.take(), vec![]);
    term.print("\x1b]0;\u{25d1} work\x07");
    assert_eq!(alerts.take(), vec![Alert::WindowTitleChanged("\u{25d1} work".to_string())]);
}

#[test]
fn osc_2_raises_an_alert_only_for_a_new_title() {
    let (mut term, alerts) = terminal();
    term.print("\x1b]2;one\x07");
    term.print("\x1b]2;one\x07");
    term.print("\x1b]2;two\x07");
    assert_eq!(
        alerts.take(),
        vec![
            Alert::WindowTitleChanged("one".to_string()),
            Alert::WindowTitleChanged("two".to_string()),
        ]
    );
    assert_eq!(term.get_title(), "two");
}

#[test]
fn an_icon_title_shows_until_osc_0_clears_it_whatever_the_title() {
    let (mut term, alerts) = terminal();
    term.print("\x1b]2;title\x07");
    term.print("\x1b]1;icon\x07");
    assert_eq!(
        alerts.take(),
        vec![
            Alert::WindowTitleChanged("title".to_string()),
            Alert::IconTitleChanged(Some("icon".to_string())),
        ]
    );
    assert_eq!(term.get_title(), "icon");
    // The same icon title again changes nothing
    term.print("\x1b]1;icon\x07");
    assert_eq!(alerts.take(), vec![]);
    // OSC 0 with the title it has: the icon title goes, so what get_title() says changes
    term.print("\x1b]0;title\x07");
    assert_eq!(alerts.take(), vec![Alert::IconTitleChanged(Some("title".to_string()))]);
    assert_eq!(term.get_title(), "title");
    // An empty OSC 1 takes an icon title away: none to take now
    term.print("\x1b]1;\x07");
    assert_eq!(alerts.take(), vec![]);
}

#[test]
fn a_title_set_again_is_said_without_an_alert() {
    // Upstream's mux window takes the title of whichever pane set one last, so a title set again
    // still reaches the pane's handler (mux's LocalPane), as no alert
    let (mut term, alerts) = terminal();
    term.print("\x1b]0;work\x07");
    // A new title is an alert
    assert_eq!(alerts.again(), Vec::<String>::new());
    term.print("\x1b]0;work\x07");
    term.print("\x1b]2;work\x07");
    assert_eq!(alerts.take(), vec![Alert::WindowTitleChanged("work".to_string())]);
    assert_eq!(alerts.again(), vec!["work".to_string(), "work".to_string()]);
    // OSC 0 clearing an icon title: its alert, and the title set again
    term.print("\x1b]1;icon\x07");
    alerts.take();
    term.print("\x1b]0;work\x07");
    assert_eq!(alerts.take(), vec![Alert::IconTitleChanged(Some("work".to_string()))]);
    assert_eq!(alerts.again(), vec!["work".to_string()]);
    // An icon title alone says nothing of the title
    term.print("\x1b]1;icon\x07");
    term.print("\x1b]1;icon\x07");
    assert_eq!(alerts.again(), Vec::<String>::new());
}

#[test]
fn osc_7_raises_an_alert_only_for_a_new_folder() {
    let (mut term, alerts) = terminal();
    term.print("\x1b]7;file://host/home/work\x07");
    term.print("\x1b]7;file://host/home/work\x07");
    assert_eq!(alerts.take(), vec![Alert::CurrentWorkingDirectoryChanged]);
    term.print("\x1b]7;file://host/home/play\x07");
    assert_eq!(alerts.take(), vec![Alert::CurrentWorkingDirectoryChanged]);
    assert_eq!(
        term.get_current_dir().map(|u| u.to_string()),
        Some("file://host/home/play".to_string())
    );
}
