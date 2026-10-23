//! Putting a pane's OSC 99 chunks together into notifications and commands

use crate::parse::{Actions, Occasion, PayloadType, Request};
use std::collections::HashMap;

/// A title or body longer than this is cut short (the protocol lets a terminal set a limit)
const MAX_TEXT: usize = 8192;
/// Notifications being put together at once, per pane; past this the oldest is dropped
const MAX_DRAFTS: usize = 32;

/// A notification ready to show
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "use_serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Notification {
    /// `None` for one sent without an id, which can't be updated or closed
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    pub buttons: Vec<String>,
    pub actions: Actions,
    pub occasion: Occasion,
    /// 0 low, 1 normal, 2 critical
    pub urgency: u8,
    pub close_event: bool,
    /// -1 the system's choice, 0 never, otherwise milliseconds
    pub expire_ms: i64,
    pub app_name: Option<String>,
    pub sound: Option<String>,
}

impl Notification {
    /// The id to put in replies: `0` when it had none, as the protocol says
    pub fn reply_id(&self) -> &str {
        self.id.as_deref().unwrap_or("0")
    }
}

/// What the GUI should do after a sequence
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Command {
    /// Show this notification, replacing one with the same id if it's still up
    Show(Notification),
    /// Close the notification with this id
    Close(String),
    /// Reply with the ids still open (the program's own id for the reply)
    Alive(String),
    /// Reply with what LBTerm supports
    Query(String),
}

#[derive(Debug, Default)]
struct Draft {
    n: Notification,
    title: Vec<u8>,
    body: Vec<u8>,
    order: u64,
}

/// One pane's notifications in progress
#[derive(Debug, Default)]
pub struct Assembler {
    drafts: HashMap<Option<String>, Draft>,
    counter: u64,
}

fn apply_keys(n: &mut Notification, r: &Request) {
    if let Some(a) = r.actions {
        n.actions = a;
    }
    if let Some(o) = r.occasion {
        n.occasion = o;
    }
    if let Some(u) = r.urgency {
        n.urgency = u;
    }
    if let Some(c) = r.close_event {
        n.close_event = c;
    }
    if let Some(w) = r.expire_ms {
        n.expire_ms = w;
    }
    if r.app_name.is_some() {
        n.app_name = r.app_name.clone();
    }
    if r.sound.is_some() {
        n.sound = r.sound.clone();
    }
}

fn push_limited(into: &mut Vec<u8>, more: &[u8]) {
    let room = MAX_TEXT.saturating_sub(into.len());
    into.extend_from_slice(&more[..more.len().min(room)]);
}

/// Text as the protocol allows it: no control characters (it forbids them; a newline is
/// turned into a space rather than lost)
fn clean(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

impl Assembler {
    /// Take one parsed sequence; a command when there's something to do
    pub fn feed(&mut self, r: Request) -> Option<Command> {
        let kind = r.payload_type.unwrap_or(PayloadType::Title);
        match kind {
            PayloadType::Query => return Some(Command::Query(r.id.unwrap_or_else(|| "0".into()))),
            PayloadType::Alive => return Some(Command::Alive(r.id.unwrap_or_else(|| "0".into()))),
            PayloadType::Close => {
                // A close for a notification still being put together drops it
                if let Some(id) = r.id {
                    self.drafts.remove(&Some(id.clone()));
                    return Some(Command::Close(id));
                }
                return None;
            }
            _ => {}
        }

        self.counter += 1;
        let order = self.counter;
        if !self.drafts.contains_key(&r.id) && self.drafts.len() >= MAX_DRAFTS {
            if let Some(oldest) = self
                .drafts
                .iter()
                .min_by_key(|(_, d)| d.order)
                .map(|(k, _)| k.clone())
            {
                self.drafts.remove(&oldest);
            }
        }
        let draft = self.drafts.entry(r.id.clone()).or_insert_with(|| Draft {
            n: Notification {
                id: r.id.clone(),
                urgency: 1,
                expire_ms: -1,
                ..Default::default()
            },
            ..Default::default()
        });
        draft.order = order;
        apply_keys(&mut draft.n, &r);
        match kind {
            PayloadType::Title => push_limited(&mut draft.title, &r.payload),
            PayloadType::Body => push_limited(&mut draft.body, &r.payload),
            PayloadType::Buttons => {
                draft.n.buttons = String::from_utf8_lossy(&r.payload)
                    .split('\u{2028}')
                    .map(|b| clean(b.as_bytes()))
                    .filter(|b| !b.is_empty())
                    .collect();
            }
            PayloadType::Icon => {}
            PayloadType::Query | PayloadType::Alive | PayloadType::Close => unreachable!(),
        }

        if r.done == Some(false) {
            return None;
        }
        let mut draft = self.drafts.remove(&r.id)?;
        draft.n.title = clean(&draft.title);
        draft.n.body = clean(&draft.body);
        if draft.n.title.is_empty() {
            // No title: the body takes its place; with neither there's nothing to show
            draft.n.title = std::mem::take(&mut draft.n.body);
        }
        if draft.n.title.is_empty() {
            return None;
        }
        Some(Command::Show(draft.n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn feed(a: &mut Assembler, text: &str) -> Option<Command> {
        let parts: Vec<&[u8]> = text.as_bytes().split(|b| *b == b';').collect();
        a.feed(parse(&parts).unwrap())
    }

    fn shown(c: Option<Command>) -> Notification {
        match c {
            Some(Command::Show(n)) => n,
            other => panic!("expected Show, got {other:?}"),
        }
    }

    #[test]
    fn a_one_shot_notification() {
        let mut a = Assembler::default();
        let n = shown(feed(&mut a, "99;;Hello world"));
        assert_eq!(n.title, "Hello world");
        assert_eq!(n.body, "");
        assert_eq!(n.id, None);
        assert_eq!(n.reply_id(), "0");
        assert_eq!(n.actions, Actions { focus: true, report: false });
        assert_eq!(n.urgency, 1);
        assert_eq!(n.expire_ms, -1);
    }

    #[test]
    fn title_and_body_in_chunks() {
        let mut a = Assembler::default();
        assert_eq!(feed(&mut a, "99;i=1:d=0:a=report;Claude "), None);
        assert_eq!(feed(&mut a, "99;i=1:d=0;needs you"), None);
        let n = shown(feed(&mut a, "99;i=1:p=body:u=2;Allow Bash(cargo test)?"));
        assert_eq!(n.title, "Claude needs you");
        assert_eq!(n.body, "Allow Bash(cargo test)?");
        assert_eq!(n.urgency, 2);
        assert!(n.actions.report && n.actions.focus);
    }

    #[test]
    fn claude_codes_three_chunks_make_one_toast() {
        // What Claude Code 2.1 writes with preferredNotifChannel "kitty": the title, the body
        // (no d=, so done), then an empty chunk naming a=focus, which arrives after the
        // notification is complete and must not make a second, empty toast
        let mut a = Assembler::default();
        assert_eq!(feed(&mut a, "99;i=7:d=0:p=title;Claude Code"), None);
        let n = shown(feed(&mut a, "99;i=7:p=body;Claude needs your permission to use Bash"));
        assert_eq!(n.title, "Claude Code");
        assert_eq!(n.body, "Claude needs your permission to use Bash");
        assert_eq!(n.reply_id(), "7");
        assert!(n.actions.focus, "a click brings the pane forward");
        assert_eq!(feed(&mut a, "99;i=7:d=1:a=focus;"), None);
    }

    #[test]
    fn the_body_stands_in_for_a_missing_title() {
        let mut a = Assembler::default();
        let n = shown(feed(&mut a, "99;p=body;only a body"));
        assert_eq!(n.title, "only a body");
        assert_eq!(n.body, "");
        // and with neither, nothing shows
        assert_eq!(feed(&mut a, "99;p=body;"), None);
    }

    #[test]
    fn buttons() {
        let mut a = Assembler::default();
        assert_eq!(feed(&mut a, "99;i=b:d=0:p=buttons;Allow\u{2028}Deny"), None);
        let n = shown(feed(&mut a, "99;i=b:a=report;Permission"));
        assert_eq!(n.buttons, vec!["Allow", "Deny"]);
    }

    #[test]
    fn close_alive_and_query() {
        let mut a = Assembler::default();
        assert_eq!(feed(&mut a, "99;i=x:p=close;"), Some(Command::Close("x".into())));
        assert_eq!(feed(&mut a, "99;p=close;"), None);
        assert_eq!(feed(&mut a, "99;i=me:p=alive;"), Some(Command::Alive("me".into())));
        assert_eq!(feed(&mut a, "99;i=q:p=?;"), Some(Command::Query("q".into())));
        assert_eq!(feed(&mut a, "99;p=?;"), Some(Command::Query("0".into())));
    }

    #[test]
    fn closing_a_draft_drops_it() {
        let mut a = Assembler::default();
        feed(&mut a, "99;i=d:d=0;half");
        feed(&mut a, "99;i=d:p=close;");
        // a later chunk starts over
        assert_eq!(shown(feed(&mut a, "99;i=d;fresh")).title, "fresh");
    }

    #[test]
    fn control_characters_are_cleaned_out() {
        let mut a = Assembler::default();
        let n = shown(feed(&mut a, "99;e=1;bGluZQpicmVha8KZ"));
        // "line\nbreak" plus a C1 control: the newline becomes a space, the C1 goes
        assert_eq!(n.title, "line break");
    }

    #[test]
    fn drafts_are_capped() {
        let mut a = Assembler::default();
        for i in 0..100 {
            feed(&mut a, &format!("99;i=n{i}:d=0;x"));
        }
        assert!(a.drafts.len() <= MAX_DRAFTS);
    }
}
