//! What the seat's words say, read for the view (Conductor's SEAT.md, `seat.items` and
//! `events`): a hub wake's events without the instructions to the Conductor after them, a notice
//! that a session needs him split into who and what, and an event's words without the session it
//! names first, which the view shows beside them already.

use super::protocol::Event;

/// One of a hub wake's events: "19:17" and what happened
#[derive(Debug, Clone, PartialEq)]
pub struct WakeEvent {
    pub time: Option<String>,
    pub text: String,
}

/// A hub wake: its headline ("1 thing since you last looked") and its events
#[derive(Debug, Clone, PartialEq)]
pub struct Wake {
    pub headline: String,
    pub events: Vec<WakeEvent>,
}

/// A time of day at the start of `text`, "19:17 ", and the rest
fn time_first(text: &str) -> (Option<String>, &str) {
    let b = text.as_bytes();
    let clock = b.len() >= 5 && b[2] == b':' && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit());
    if clock && (b.len() == 5 || b[5] == b' ') {
        (Some(text[..5].to_string()), text[5..].trim_start())
    } else {
        (None, text)
    }
}

/// A hub wake (`who: "hub"`): "Conductor hub: 2 things since you last looked." and a "- " line
/// for each thing. The lines after them are the hub's instructions to the Conductor (what to
/// read, the mode's rules), which are the Conductor's and not shown.
pub fn wake(text: &str) -> Wake {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim();
    let headline = first
        .strip_prefix("Conductor hub:")
        .unwrap_or(first)
        .trim()
        .trim_end_matches('.')
        .to_string();
    let mut events: Vec<WakeEvent> = vec![];
    for line in lines {
        if let Some(rest) = line.strip_prefix("- ") {
            let (time, words) = time_first(rest.trim());
            events.push(WakeEvent {
                time,
                text: words.to_string(),
            });
        } else if line.starts_with("  ") && !events.is_empty() {
            // A thing's words running on
            let last = events.last_mut().unwrap();
            last.text = format!("{} {}", last.text, line.trim());
        } else {
            break;
        }
    }
    Wake { headline, events }
}

/// A notice that a session needs him: who it is as he knows it, and what for
#[derive(Debug, Clone, PartialEq)]
pub struct Needs {
    pub label: String,
    pub project: String,
    /// What it needs him to do, its first sentence
    pub what: String,
    /// The rest: where it's got to, the risk
    pub detail: String,
}

/// "Site publish in Website needs you: Pick a layout. Both are built." (or the hub's
/// older "site-a6 (Website) is waiting on you and needs you: …")
pub fn needs(text: &str) -> Option<Needs> {
    let at = text.find(" needs you:")?;
    let who = text[..at].trim();
    let who = who.strip_suffix(" is waiting on you and").unwrap_or(who).trim();
    let rest = text[at + " needs you:".len()..].trim();
    let (label, project) = if let Some(open) = who.rfind(" (").filter(|_| who.ends_with(')')) {
        (who[..open].trim(), who[open + 2..who.len() - 1].trim())
    } else if let Some(i) = who.rfind(" in ") {
        (who[..i].trim(), who[i + 4..].trim())
    } else {
        (who, "")
    };
    if label.is_empty() {
        return None;
    }
    // Its first sentence, then the rest ("commit.. Repos" has two stops)
    let (what, detail) = match rest.find(". ") {
        Some(i) => (rest[..i].trim_end_matches('.'), rest[i + 1..].trim_start_matches('.').trim()),
        None => (rest.trim_end_matches('.'), ""),
    };
    Some(Needs {
        label: label.to_string(),
        project: project.to_string(),
        what: what.to_string(),
        detail: detail.to_string(),
    })
}

/// An event's kind as he reads it: "needsYou" is "Needs you"
pub fn kind(kind: &str) -> String {
    let mut out = String::new();
    for (i, c) in kind.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// What an event says beyond its kind and its session: "Pad colours in Terminal is doing
/// something outward (git push)" is "Doing something outward (git push)", "… needs you: Decide"
/// is "Decide", and "… finished a step" says nothing its kind doesn't, so none
pub fn event(e: &Event) -> Option<String> {
    let text = e.text.trim();
    let mut rest = text;
    if let Some(label) = e.label.as_deref().filter(|l| !l.is_empty()) {
        let named = if e.project.is_empty() {
            label.to_string()
        } else {
            format!("{label} in {}", e.project)
        };
        if let Some(after) = rest.strip_prefix(&named) {
            rest = after.trim_start();
        }
    }
    if rest.len() == text.len() {
        return Some(text.to_string()).filter(|t| !t.is_empty());
    }
    let rest = rest.strip_prefix("needs you:").or_else(|| rest.strip_prefix("is ")).unwrap_or(rest).trim();
    let plain = |s: &str| s.to_lowercase().replace(' ', "");
    let says = plain(rest);
    let kind = plain(&e.kind);
    if says.is_empty() || says == format!("{kind}astep") || says == kind || says == format!("{kind}.") {
        return None;
    }
    let mut chars = rest.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_wake_keeps_its_events_and_leaves_the_conductor_its_instructions() {
        let w = wake(
            "Conductor hub: 2 things since you last looked.\n- 19:17 Site publish in Website is doing something outward (git push): Bash: git push\n- Pad colours in Terminal finished a step\nUse read <session> for detail. Decide whether Lachlan should hear about it now.\nYour mode: accept most.",
        );
        assert_eq!(w.headline, "2 things since you last looked");
        assert_eq!(w.events.len(), 2);
        assert_eq!(w.events[0].time.as_deref(), Some("19:17"));
        assert!(w.events[0].text.starts_with("Site publish in Website is doing"));
        assert_eq!(w.events[1].time, None);
        assert!(w.events.iter().all(|e| !e.text.contains("Your mode")));
        // Not the hub's shape: its first line, no events
        assert_eq!(wake("something else").headline, "something else");
    }

    #[test]
    fn a_notice_that_a_session_needs_him_says_who_and_what() {
        let n = needs("Site publish in Website needs you: Decide whether the hooks come off so the branch can commit.. Repos created and origin moved. Risk: none").unwrap();
        assert_eq!((n.label.as_str(), n.project.as_str()), ("Site publish", "Website"));
        assert_eq!(n.what, "Decide whether the hooks come off so the branch can commit");
        assert_eq!(n.detail, "Repos created and origin moved. Risk: none");
        let n = needs("Pad colours in Terminal needs you: Restart LBTerm").unwrap();
        assert_eq!((n.label.as_str(), n.what.as_str(), n.detail.as_str()), ("Pad colours", "Restart LBTerm", ""));
        let n = needs("site-a6 (Website) is waiting on you and needs you: pick a layout").unwrap();
        assert_eq!((n.label.as_str(), n.project.as_str()), ("site-a6", "Website"));
        // A label with "in" in it: the project is after the last
        assert_eq!(needs("Sign in flow in Website needs you: OK").unwrap().label, "Sign in flow");
        assert_eq!(needs("Allowed: Start a session in Website (LBTerm)"), None);
    }

    #[test]
    fn an_events_words_leave_out_its_session_and_what_its_kind_says() {
        let e = |kind: &str, text: &str| Event {
            kind: kind.into(),
            label: Some("Pad colours".into()),
            project: "Terminal".into(),
            text: text.into(),
            ..Default::default()
        };
        assert_eq!(event(&e("outward", "Pad colours in Terminal is doing something outward (git push)")).as_deref(), Some("Doing something outward (git push)"));
        assert_eq!(event(&e("needsYou", "Pad colours in Terminal needs you: Decide Q1. More")).as_deref(), Some("Decide Q1. More"));
        assert_eq!(event(&e("finished", "Pad colours in Terminal finished a step")), None);
        assert_eq!(event(&e("started", "Pad colours in Terminal started")), None);
        assert_eq!(event(&e("ended", "Pad colours in Terminal ended")), None);
        assert_eq!(event(&e("waiting", "asks which layout")).as_deref(), Some("asks which layout"), "words that don't name it stay");
        assert_eq!(kind("needsYou"), "Needs you");
        assert_eq!(kind("outward"), "Outward");
    }
}
