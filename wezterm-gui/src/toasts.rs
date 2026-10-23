//! LBTerm: Windows toasts for kitty's notifications (OSC 99, lbterm-notify). Richer than
//! wezterm-toast-notification's: buttons, an id that replaces or closes a toast, no sound for
//! low urgency, and what the person did with it (clicked it, clicked a button, dismissed it)
//! handed back so the GUI can focus the pane and report to the program.
//!
//! WinRT runs the toast's events on threads of its own; `on_event` is called there, and the
//! GUI's handler moves to the main thread itself.

/// What to show
#[derive(Debug, Clone)]
pub struct Toast {
    /// Replaces a toast with the same tag that's still up; also how it's closed
    pub tag: String,
    pub title: String,
    pub body: String,
    pub buttons: Vec<String>,
    /// 0 low (no sound), 1 normal, 2 critical (stays on screen longer)
    pub urgency: u8,
    pub silent: bool,
}

/// What happened to a toast
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastEvent {
    /// Clicked: on its body (`None`) or on button n, counting from 1
    Activated(Option<usize>),
    /// Gone without a click: dismissed, timed out or closed by LBTerm
    Dismissed,
}

#[cfg(windows)]
pub use imp::{hide, open, show};

#[cfg(not(windows))]
pub fn show(_: Toast, _: impl Fn(ToastEvent) + Send + Sync + 'static) {}
#[cfg(not(windows))]
pub fn hide(_: &str) {}
#[cfg(not(windows))]
pub fn open() -> Vec<String> {
    vec![]
}

#[cfg(windows)]
mod imp {
    use super::{Toast, ToastEvent};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use windows::core::{IInspectable, Interface, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastDismissedEventArgs, ToastNotification,
        ToastNotificationManager, ToastNotifier,
    };
    use xml::escape::escape_str_attribute;
    use xml::escape::escape_str_pcdata;

    lazy_static::lazy_static! {
        /// The toasts up now, by tag, each with its serial number, so one can be replaced or
        /// closed, and a replaced toast's late events are told apart from its successor's
        static ref OPEN: Mutex<HashMap<String, (u64, ToastNotification)>> = Mutex::new(HashMap::new());
    }
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    /// Forget the toast `tag` if it's still this one (`serial`): true when it was
    fn forget(tag: &str, serial: u64) -> bool {
        let mut open = OPEN.lock().unwrap();
        if matches!(open.get(tag), Some((s, _)) if *s == serial) {
            open.remove(tag);
            true
        } else {
            false
        }
    }

    fn notifier() -> windows::core::Result<ToastNotifier> {
        ToastNotificationManager::CreateToastNotifierWithId(HSTRING::from(config::lbterm::APP_ID))
    }

    fn xml(toast: &Toast) -> String {
        let mut actions = String::new();
        if !toast.buttons.is_empty() {
            actions.push_str("<actions>");
            for (i, label) in toast.buttons.iter().take(5).enumerate() {
                actions.push_str(&format!(
                    r#"<action content="{}" arguments="button={}" />"#,
                    escape_str_attribute(label),
                    i + 1
                ));
            }
            actions.push_str("</actions>");
        }
        let audio = if toast.silent || toast.urgency == 0 {
            r#"<audio silent="true" />"#
        } else {
            ""
        };
        let duration = if toast.urgency >= 2 { "long" } else { "short" };
        let body = if toast.body.is_empty() {
            String::new()
        } else {
            format!("<text>{}</text>", escape_str_pcdata(&toast.body))
        };
        format!(
            r#"<toast duration="{duration}" launch="click"><visual><binding template="ToastGeneric"><text>{}</text>{body}</binding></visual>{actions}{audio}</toast>"#,
            escape_str_pcdata(&toast.title),
        )
    }

    fn show_impl(
        toast: Toast,
        on_event: std::sync::Arc<dyn Fn(ToastEvent) + Send + Sync>,
    ) -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(HSTRING::from(xml(&toast)))?;
        let notif = ToastNotification::CreateToastNotification(doc)?;
        // The tag must be short (64 chars) and the group names LBTerm's own
        let tag: String = toast.tag.chars().take(64).collect();
        notif.SetTag(HSTRING::from(tag.as_str()))?;
        notif.SetGroup(HSTRING::from("lbterm"))?;

        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let activated = on_event.clone();
        let tag_a = tag.clone();
        notif.Activated(TypedEventHandler::new(
            move |_: &Option<ToastNotification>, args: &Option<IInspectable>| {
                let button = args
                    .as_ref()
                    .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                    .and_then(|a| a.Arguments().ok())
                    .and_then(|a| a.to_string().strip_prefix("button=").and_then(|n| n.parse().ok()));
                if forget(&tag_a, serial) {
                    activated(ToastEvent::Activated(button));
                }
                Ok(())
            },
        ))?;
        let dismissed = on_event.clone();
        let tag_d = tag.clone();
        notif.Dismissed(TypedEventHandler::new(
            move |_: &Option<ToastNotification>, _: &Option<ToastDismissedEventArgs>| {
                if forget(&tag_d, serial) {
                    dismissed(ToastEvent::Dismissed);
                }
                Ok(())
            },
        ))?;

        let notifier = notifier()?;
        // Showing a tag again replaces what's up: the old one goes quietly, because it's no
        // longer the one in OPEN when its Dismissed arrives
        let old = OPEN.lock().unwrap().insert(tag.clone(), (serial, notif.clone()));
        if let Some((_, old)) = old {
            let _ = notifier.Hide(&old);
        }
        notifier.Show(&notif)?;
        // Enabled, or why Windows won't show LBTerm's toasts (its app id unregistered, turned
        // off in Settings, Focus Assist)
        log::debug!("toast {tag} shown; Windows says {:?}", notifier.Setting().map(|s| s.0));
        Ok(())
    }

    /// Show `toast`; `on_event` hears what happens to it, on a WinRT thread
    pub fn show(toast: Toast, on_event: impl Fn(ToastEvent) + Send + Sync + 'static) {
        let on_event: std::sync::Arc<dyn Fn(ToastEvent) + Send + Sync> = std::sync::Arc::new(on_event);
        // Off the GUI thread: WinRT can pump messages while it shows a toast
        std::thread::spawn(move || {
            if let Err(err) = show_impl(toast, on_event) {
                log::error!("toast: {err:#}");
            }
        });
    }

    /// Take down the toast with this tag, if it's up. Its Dismissed handler reports it.
    pub fn hide(tag: &str) {
        let tag = tag.to_string();
        std::thread::spawn(move || {
            let notif = OPEN.lock().unwrap().get(&tag).map(|(_, n)| n.clone());
            if let Some(notif) = notif {
                if let Ok(notifier) = notifier() {
                    let _ = notifier.Hide(&notif);
                }
            }
        });
    }

    /// The tags of the toasts up now
    pub fn open() -> Vec<String> {
        OPEN.lock().unwrap().keys().cloned().collect()
    }
}
