//! LBTerm: one notification of a pane's output, and one task for its alerts, on its way to the
//! main thread at a time.
//!
//! A pane's parser thread applies each batch of output to the terminal itself, then tells the
//! mux's subscribers on the main thread (a window's renderer, the mux server's connections) with
//! a task of its own, upstream one for every batch, and one more for every alert the batch
//! raised (a title, a user var, a bell, progress). When the main thread was behind, as a mux
//! server's was with six tabs printing without a pause (2026-10-29), those tasks queued without
//! bound, each to say the same thing, and a key's task waited behind all of them.
//!
//! The output notification only says "this pane has new output": whoever hears it reads the pane
//! as it is then, so one still waiting covers all the output that comes before it's delivered.
//! [`OutputNotice`] lets a batch queue a notification only when none is waiting, and marks it
//! delivered as the main thread takes it, before the subscribers read the pane, so output after
//! that queues the next. A pane's alerts wait in [`HeldAlerts`], one task queued for all of them,
//! each that the receiver acts on kept and each that only says something changed held once, its
//! latest ([`note_alert`]), as the mux server holds them for each client's next push of the pane
//! (wezterm-mux-server-impl's lbterm_flow.rs).

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use wezterm_term::Alert;

/// Whether a pane's PaneOutput notification is queued for the main thread and not yet delivered
#[derive(Default, Debug)]
pub struct OutputNotice {
    queued: AtomicBool,
}

impl OutputNotice {
    /// Whether the batch just applied must queue a notification: none is waiting already
    pub fn queue(&self) -> bool {
        !self.queued.swap(true, Ordering::AcqRel)
    }

    /// The queued notification is being delivered: output applied from now on queues another
    pub fn delivering(&self) {
        self.queued.store(false, Ordering::Release);
    }
}

/// How many alerts are held for a pane before they're cut down to the latest of each kind (a user
/// var by name), toasts and kitty notifications each kept, the oldest of them let go past it: what
/// a pane raises between two deliveries (a main-thread turn, or a client's next push of it) only
/// reaches it in a flood. Each push's alerts go to the window as PDUs of their own, so the bound is
/// what a flood costs: six tabs each setting 200 user vars a block without a pause (perf/
/// mux-flood.ps1 -Kind alerts, two runs each, 2026-10-31) echoed a key in 22 to 23 ms at the median
/// with every alert held once a kind, 27 to 28 with 16 held and 34 to 35 with 64
pub const MAX_HELD_ALERTS: usize = 16;

/// Whether the receiver acts on `alert` each time it comes, so each is kept: a bell rings (and runs
/// Lua's `bell`), a user var runs Lua's `user-var-changed` with its value (and glints the tab's pad
/// when the cli typed), a toast or a kitty notification shows. The rest only say something changed,
/// or give a state the receiver keeps or reads (the title, the progress, the palette, the
/// working directory, a program's status records), so an earlier one of a kind says nothing the
/// latest doesn't
fn is_event(alert: &Alert) -> bool {
    matches!(
        alert,
        Alert::Bell
            | Alert::SetUserVar { .. }
            | Alert::ToastNotification { .. }
            | Alert::KittyNotification(_)
    )
}

/// Whether a mux client acting on `alert` reads what a push of the pane carries, so a mux server
/// owing a pane for it works the pane's changes out and sends them first, as it does for output:
/// a title alert has the window read the pane's title, and the cwd's its working dir, which a
/// client pane answers from its last push (wezterm-client's clientpane.rs keeps no title or cwd
/// of an alert's), and a pane's foreground process changing them raises the alert with no output
/// (lbterm_proclist's `tell`). Every other alert carries what it says (a user var's value, the
/// progress, a bell, a toast) or has a PDU of its own sent for it (a program's status records,
/// the palette), so an owe of those alone is sent without the pane's changes worked out: under
/// mux-flood's alerts flood, about half of the asks a second, each reading the pane under its
/// terminal lock for nothing (perf/mux-client's flood trace, run 6; FLOWS.md, row 29)
pub fn reads_the_push(alert: &Alert) -> bool {
    matches!(
        alert,
        Alert::WindowTitleChanged(_)
            | Alert::TabTitleChanged(_)
            | Alert::IconTitleChanged(_)
            | Alert::CurrentWorkingDirectoryChanged
    )
}

/// Adds `alert` to what's held, in the order they were raised. One that only says something
/// changed is held once, its latest last (an earlier one of its kind is taken out); every other is
/// kept, as upstream delivered each, until [`MAX_HELD_ALERTS`] are held, when a flood of them (a
/// program setting a user var or ringing the bell without a pause, faster than the main thread or
/// the client takes them) is cut down to the latest bell and the latest value of each user var,
/// so what's held stays bounded and the last of each still goes.
pub fn note_alert(held: &mut Vec<Alert>, alert: Alert) {
    if !is_event(&alert) {
        let kind = std::mem::discriminant(&alert);
        held.retain(|h| std::mem::discriminant(h) != kind);
    }
    held.push(alert);
    if held.len() > MAX_HELD_ALERTS {
        cut_down(held);
    }
}

/// What's held past the bound: each bell and each user var by name held once, its latest, where
/// it is; then the oldest toasts and kitty notifications let go while it's still past it
fn cut_down(held: &mut Vec<Alert>) {
    let keep: Vec<bool> = {
        let mut bell = false;
        let mut names = std::collections::HashSet::new();
        let mut keep = vec![true; held.len()];
        for (i, alert) in held.iter().enumerate().rev() {
            keep[i] = match alert {
                Alert::Bell => !std::mem::replace(&mut bell, true),
                Alert::SetUserVar { name, .. } => names.insert(name.as_str()),
                _ => true,
            };
        }
        keep
    };
    let mut i = 0;
    held.retain(|_| {
        i += 1;
        keep[i - 1]
    });
    while held.len() > MAX_HELD_ALERTS {
        match held.iter().position(|a| {
            matches!(
                a,
                Alert::ToastNotification { .. } | Alert::KittyNotification(_)
            )
        }) {
            Some(i) => {
                held.remove(i);
            }
            None => break,
        }
    }
}

/// A pane's alerts raised on its parser thread and not yet delivered on the main thread, and a
/// title it set again unchanged and not yet given to its window (LocalPaneNotifHandler's
/// title_set_again, which raises no alert but gives the window the title again): one task on its
/// way for them all
#[derive(Default, Debug)]
pub struct HeldAlerts {
    held: Mutex<Held>,
}

#[derive(Default, Debug)]
struct Held {
    alerts: Vec<Alert>,
    /// Newer than any WindowTitleChanged in `alerts`, which takes it away when it comes
    title_again: Option<String>,
}

impl Held {
    fn is_empty(&self) -> bool {
        self.alerts.is_empty() && self.title_again.is_none()
    }
}

impl HeldAlerts {
    /// Holds `alert` with the rest: whether a task must be queued to deliver them (none is
    /// queued: nothing was held)
    pub fn add(&self, alert: Alert) -> bool {
        let mut held = self.held.lock();
        let first = held.is_empty();
        if matches!(alert, Alert::WindowTitleChanged(_)) {
            held.title_again = None;
        }
        note_alert(&mut held.alerts, alert);
        first
    }

    /// Holds `title`, set again: whether a task must be queued to give it
    pub fn set_again(&self, title: &str) -> bool {
        let mut held = self.held.lock();
        let first = held.is_empty();
        held.title_again = Some(title.to_string());
        first
    }

    /// What's held, for the task delivering it: the alerts in the order they were raised, then
    /// the title set again after them, if any; what's raised from now on queues another
    pub fn take(&self) -> (Vec<Alert>, Option<String>) {
        let held = std::mem::take(&mut *self.held.lock());
        (held.alerts, held.title_again)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wezterm_term::Progress;

    #[test]
    fn one_notification_waits_at_a_time() {
        let n = OutputNotice::default();
        assert!(n.queue(), "the first batch queues one");
        for _ in 0..100 {
            assert!(!n.queue(), "the ones after are covered by it");
        }
        n.delivering();
        assert!(n.queue(), "output after delivery queues the next");
        assert!(!n.queue());
    }

    /// Batches applied on one thread while another delivers: every batch is applied before some
    /// delivery that follows it, so none goes unannounced
    #[test]
    fn no_batch_goes_unannounced() {
        use std::sync::atomic::AtomicUsize;
        let notice = Arc::new(OutputNotice::default());
        let applied = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let parser = {
            let notice = Arc::clone(&notice);
            let applied = Arc::clone(&applied);
            std::thread::spawn(move || {
                for _ in 0..20_000 {
                    applied.fetch_add(1, Ordering::SeqCst);
                    if notice.queue() {
                        tx.send(()).unwrap();
                    }
                }
            })
        };
        // The main thread: each delivery marks itself delivered, then reads what's applied
        let mut seen = 0;
        let mut deliveries = 0;
        while let Ok(()) = rx.recv() {
            notice.delivering();
            seen = seen.max(applied.load(Ordering::SeqCst));
            deliveries += 1;
        }
        parser.join().unwrap();
        assert_eq!(seen, 20_000, "the last batch was read by a delivery after it");
        assert!(deliveries >= 1 && deliveries <= 20_000);
    }

    fn var(name: &str, value: &str) -> Alert {
        Alert::SetUserVar {
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    fn count(held: &[Alert], f: impl Fn(&Alert) -> bool) -> usize {
        held.iter().filter(|a| f(a)).count()
    }

    #[test]
    fn a_state_is_held_once_and_each_event_kept() {
        let mut held = vec![];
        for i in 0..4 {
            note_alert(&mut held, Alert::WindowTitleChanged(format!("block {i}")));
            note_alert(&mut held, var("claude_status", &format!("{i}")));
            note_alert(&mut held, var("other", "x"));
            note_alert(&mut held, Alert::Progress(Progress::Percentage(i)));
            note_alert(&mut held, Alert::PaletteChanged);
            note_alert(&mut held, Alert::Bell);
        }
        // The title, the progress and the palette once each, their latest
        assert_eq!(count(&held, |a| matches!(a, Alert::WindowTitleChanged(_))), 1);
        assert!(held.contains(&Alert::WindowTitleChanged("block 3".to_string())));
        assert_eq!(count(&held, |a| matches!(a, Alert::Progress(_))), 1);
        assert!(held.contains(&Alert::Progress(Progress::Percentage(3))));
        assert_eq!(count(&held, |a| *a == Alert::PaletteChanged), 1);
        // Every bell rings, and Lua sees every value of a user var, in turn, repeats too
        assert_eq!(count(&held, |a| *a == Alert::Bell), 4);
        let status: Vec<_> = held
            .iter()
            .filter_map(|a| match a {
                Alert::SetUserVar { name, value } if name == "claude_status" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(status, (0..4).map(|i| format!("{i}")).collect::<Vec<_>>());
        assert_eq!(count(&held, |a| *a == var("other", "x")), 4);
        assert_eq!(held.len(), 3 + 4 * 3);
        assert!(held.len() <= MAX_HELD_ALERTS, "all of it under the bound");
        assert_eq!(held.last(), Some(&Alert::Bell), "in the order they were raised");
    }

    #[test]
    fn a_flood_past_the_bound_is_cut_down_to_the_latest_of_each() {
        let mut held = vec![];
        for i in 0..1000 {
            note_alert(&mut held, Alert::WindowTitleChanged(format!("block {i}")));
            note_alert(&mut held, var("claude_status", &format!("{i}")));
            note_alert(&mut held, var("other", "x"));
            note_alert(&mut held, Alert::Bell);
            assert!(held.len() <= MAX_HELD_ALERTS, "{} held", held.len());
        }
        assert!(held.contains(&Alert::WindowTitleChanged("block 999".to_string())));
        assert!(held.contains(&var("claude_status", "999")), "the last value still goes");
        assert!(held.contains(&var("other", "x")));
        assert_eq!(held.last(), Some(&Alert::Bell));
        // What's kept of a user var is in the order it was set
        let status: Vec<usize> = held
            .iter()
            .filter_map(|a| match a {
                Alert::SetUserVar { name, value } if name == "claude_status" => value.parse().ok(),
                _ => None,
            })
            .collect();
        assert!(status.windows(2).all(|w| w[0] < w[1]), "{:?}", status);
        // Up to the bound every bell is kept, past it the latest
        let mut bells = vec![];
        for _ in 0..MAX_HELD_ALERTS {
            note_alert(&mut bells, Alert::Bell);
        }
        assert_eq!(bells.len(), MAX_HELD_ALERTS);
        note_alert(&mut bells, Alert::Bell);
        assert_eq!(bells, vec![Alert::Bell]);
    }

    #[test]
    fn toasts_are_each_kept_up_to_the_limit() {
        let mut held = vec![Alert::Bell];
        for i in 0..MAX_HELD_ALERTS + 10 {
            note_alert(
                &mut held,
                Alert::ToastNotification {
                    title: None,
                    body: format!("{i}"),
                    focus: false,
                },
            );
        }
        assert_eq!(held.len(), MAX_HELD_ALERTS);
        assert_eq!(held[0], Alert::Bell, "only toasts are let go");
        let first_body = match &held[1] {
            Alert::ToastNotification { body, .. } => body.clone(),
            _ => panic!(),
        };
        assert_eq!(first_body, "11", "the oldest went first");
    }

    #[test]
    fn one_task_delivers_a_panes_alerts() {
        let held = HeldAlerts::default();
        assert!(held.add(var("a", "1")), "the first queues a task");
        for i in 0..3 {
            assert!(!held.add(Alert::WindowTitleChanged(format!("{i}"))));
            assert!(!held.add(var("a", &format!("{i}"))));
        }
        assert_eq!(
            held.take(),
            (
                vec![
                    var("a", "1"),
                    var("a", "0"),
                    var("a", "1"),
                    Alert::WindowTitleChanged("2".to_string()),
                    var("a", "2")
                ],
                None
            ),
            "the title once, its latest, every value of the user var, in the order they came"
        );
        assert!(held.add(Alert::Bell), "after the take, the next queues another");
        assert_eq!(held.take(), (vec![Alert::Bell], None));
        assert_eq!(held.take(), (vec![], None));
    }

    #[test]
    fn a_title_set_again_goes_with_them_and_the_newer_wins() {
        let held = HeldAlerts::default();
        assert!(held.set_again("x"), "a title set again queues a task of its own");
        assert!(!held.set_again("x"));
        assert!(!held.add(Alert::WindowTitleChanged("y".to_string())));
        assert_eq!(
            held.take(),
            (vec![Alert::WindowTitleChanged("y".to_string())], None),
            "the new title came after it, so the window takes that"
        );
        assert!(held.add(Alert::WindowTitleChanged("y".to_string())));
        assert!(!held.set_again("x"));
        assert_eq!(
            held.take(),
            (
                vec![Alert::WindowTitleChanged("y".to_string())],
                Some("x".to_string())
            ),
            "set again after it: given to the window last"
        );
    }

    /// Alerts raised on one thread while another delivers: the last of each is delivered
    #[test]
    fn no_alert_goes_undelivered() {
        let held = Arc::new(HeldAlerts::default());
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let parser = {
            let held = Arc::clone(&held);
            std::thread::spawn(move || {
                for i in 0..20_000 {
                    if held.add(var("n", &format!("{i}"))) {
                        tx.send(()).unwrap();
                    }
                }
            })
        };
        let mut last = None;
        while let Ok(()) = rx.recv() {
            for alert in held.take().0 {
                last = Some(alert);
            }
        }
        parser.join().unwrap();
        assert_eq!(last, Some(var("n", "19999")), "every add after a take queued a task");
        assert_eq!(held.take(), (vec![], None));
    }

    #[test]
    fn only_a_title_or_a_cwd_has_the_push_worked_out() {
        for alert in [
            Alert::WindowTitleChanged("t".to_string()),
            Alert::TabTitleChanged(None),
            Alert::IconTitleChanged(None),
            Alert::CurrentWorkingDirectoryChanged,
        ] {
            assert!(reads_the_push(&alert), "{:?}", alert);
        }
        for alert in [
            Alert::Bell,
            var("a", "1"),
            Alert::Progress(Progress::Percentage(5)),
            Alert::ProgramStatus,
            Alert::PaletteChanged,
            Alert::OutputSinceFocusLost,
            Alert::ToastNotification {
                title: None,
                body: "b".to_string(),
                focus: false,
            },
        ] {
            assert!(!reads_the_push(&alert), "{:?}", alert);
        }
    }
}
