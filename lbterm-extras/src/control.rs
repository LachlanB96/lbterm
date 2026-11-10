//! LBTerm's control pipe (lbterm/CONTROL.md): what other programs on this PC ask the GUI to do,
//! one JSON object a line each way, on `config::lbterm::control_pipe(<window class>)`. A request
//! is `{"id": <anything>, "op": "<op>", ...}`; its reply carries the same `id` and `"ok": true`
//! with what was asked for, or `"ok": false` and `error`, saying why. A connection carries as
//! many requests as it likes, one at a time. Earshot drives LBTerm by voice through it, and
//! `lbterm control` is the same from a shell.
//!
//! - `focus`: where he is. The window he was last in (it had the keyboard, or was clicked in),
//!   its active tab's active pane by the id `lbterm cli` takes (the mux server's), the Claude
//!   session in it, its title, and whether LBTerm has the keyboard now.
//! - `emit`, with `event`: runs a Lua event in that window with its active pane, as a key bound
//!   to `EmitEvent` would: `lbterm-tab-forward` is the mouse's Forward button.
//! - `draft.get`, `draft.add`, `draft.picker`, `draft.scratch`, `draft.clear`, `draft.send` and
//!   `draft.edit`: a voice draft over a pane (lbterm_extras::drafts), the open pane's unless the
//!   request names one with `pane` (the mux server's id). Each reply carries the draft as it is
//!   after it, or null.
//! - `metrics`: the counters (counters.rs) and the process's CPU, memory and handles, since the
//!   process started or the last request with `"reset": true`, which starts them again after
//!   its reply (lbterm/PERF.md).

use crate::drafts::{Body, Draft};
use crate::mic::MicState;
use serde_json::{json, Map, Value};
use std::time::Duration;

/// The longest request line read: a request is a few hundred bytes
pub const MAX_LINE: usize = 64 * 1024;

/// The longest event name taken
pub const MAX_EVENT: usize = 128;

/// WezTerm's own events, which it calls with arguments of their own (a tab, a URI, a name and a
/// value) or at a moment of its own: emitted from outside, their handlers would get a window and
/// a pane they don't expect, so they're refused
pub const BUILT_IN_EVENTS: &[&str] = &[
    "augment-command-palette",
    "bell",
    "format-tab-title",
    "format-window-title",
    "gui-attached",
    "gui-startup",
    "mux-is-process-stateful",
    "mux-startup",
    "new-tab-button-click",
    "open-uri",
    "update-right-status",
    "update-status",
    "user-var-changed",
    "window-config-reloaded",
    "window-focus-changed",
    "window-resized",
];

/// The most keys a picker's picks are typed with, and the longest one
pub const MAX_KEYS: usize = 64;
pub const MAX_KEY: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Focus,
    Emit { event: String },
    /// A voice draft, over `pane` (the mux server's id) or the open pane
    Draft { pane: Option<u64>, act: DraftAct },
    /// The mic mark on the strip (lbterm_extras::mic): its state for `ttl`, and what was heard,
    /// said on the strip of the window he was last in
    Mic {
        state: Option<MicState>,
        ttl: Duration,
        heard: Option<String>,
    },
    /// The counters and the process, since they started; `reset` starts them again after
    /// (`force` for the daily LBTerm's, whose counts every reader shares), `present` turns the
    /// timing to the screen on or off (lbterm_extras::present) before the reply
    Metrics { reset: bool, force: bool, present: Option<bool> },
}

/// What a `draft.*` request does
#[derive(Debug, Clone, PartialEq)]
pub enum DraftAct {
    /// What's there
    Get,
    /// A piece of an answer added; `fresh` starts the draft again
    Add { text: String, fresh: bool },
    /// Picks for a picker, in place of what's there
    Picker {
        label: String,
        keys: Vec<String>,
        questions: Vec<String>,
        missing: Vec<String>,
    },
    /// The newest piece taken back
    Scratch,
    /// Let go
    Clear,
    /// Typed and sent
    Send,
    /// Typed into the prompt, not sent
    Edit,
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::Focus => "focus",
            Op::Emit { .. } => "emit",
            Op::Draft { act, .. } => match act {
                DraftAct::Get => "draft.get",
                DraftAct::Add { .. } => "draft.add",
                DraftAct::Picker { .. } => "draft.picker",
                DraftAct::Scratch => "draft.scratch",
                DraftAct::Clear => "draft.clear",
                DraftAct::Send => "draft.send",
                DraftAct::Edit => "draft.edit",
            },
            Op::Mic { .. } => "mic",
            Op::Metrics { .. } => "metrics",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The caller's, handed back in the reply as it came (null when it sent none)
    pub id: Value,
    pub op: Op,
}

/// Where he is, for `focus`'s reply
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Focus {
    /// The mux window
    pub window: u64,
    /// The active tab's active pane, by the mux server's id
    pub pane: u64,
    /// The Claude session in that pane (its `claude_session` user var)
    pub session: Option<String>,
    pub title: String,
    /// An LBTerm window has the keyboard now
    pub foreground: bool,
}

/// A request line: what it asks, or the reply that refuses it
pub fn parse(line: &str) -> Result<Request, String> {
    let value: Value = match serde_json::from_str(line.trim()) {
        Ok(value) => value,
        Err(err) => return Err(refused(&Value::Null, &format!("not a JSON object: {err}"))),
    };
    let Some(obj) = value.as_object() else {
        return Err(refused(&Value::Null, "not a JSON object"));
    };
    let id = obj.get("id").cloned().unwrap_or(Value::Null);
    let op = match obj.get("op").and_then(Value::as_str) {
        Some("focus") => Op::Focus,
        Some("emit") => Op::Emit {
            event: event_name(obj).map_err(|why| refused(&id, &why))?,
        },
        Some(name) if name.starts_with("draft.") => draft_op(name, obj).map_err(|why| refused(&id, &why))?,
        Some("mic") => mic_op(obj).map_err(|why| refused(&id, &why))?,
        Some("metrics") => {
            let flag = |name: &str| -> Result<Option<bool>, String> {
                match obj.get(name) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::Bool(b)) => Ok(Some(*b)),
                    Some(_) => Err(refused(&id, &format!("{name} is true or false"))),
                }
            };
            let present = match obj.get("present") {
                None | Some(Value::Null) => None,
                Some(Value::Bool(b)) => Some(*b),
                Some(Value::String(s)) if s == "on" => Some(true),
                Some(Value::String(s)) if s == "off" => Some(false),
                Some(_) => return Err(refused(&id, "present is on or off")),
            };
            Op::Metrics {
                reset: flag("reset")?.unwrap_or(false),
                force: flag("force")?.unwrap_or(false),
                present,
            }
        }
        Some(other) => return Err(refused(&id, &format!("no op called {other:?}"))),
        None => return Err(refused(&id, "no op")),
    };
    Ok(Request { id, op })
}

fn mic_op(obj: &Map<String, Value>) -> Result<Op, String> {
    let Some(name) = obj.get("state").and_then(Value::as_str) else {
        return Err("mic needs a state: off, listening, hearing or muted".to_string());
    };
    let heard = match obj.get("heard") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => crate::mic::heard(s),
        Some(_) => return Err("heard is text".to_string()),
    };
    Ok(Op::Mic {
        state: MicState::parse(name)?,
        ttl: crate::mic::ttl(obj.get("ttl").and_then(Value::as_f64)),
        heard,
    })
}

/// `mic`'s reply: the state it shows now
pub fn mic_reply(id: &Value, state: Option<MicState>) -> String {
    let mut fields = Map::new();
    fields.insert("mic".to_string(), json!(state.map(MicState::name)));
    line(id, true, fields)
}

fn draft_op(name: &str, obj: &Map<String, Value>) -> Result<Op, String> {
    let pane = match obj.get("pane") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| "pane is a pane's id, a number".to_string())?),
    };
    let text = |key: &str| -> Result<String, String> {
        obj.get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("{name} needs {key}"))
    };
    let texts = |key: &str, max: usize, longest: usize| -> Result<Vec<String>, String> {
        let list = match obj.get(key) {
            None | Some(Value::Null) => return Ok(vec![]),
            Some(Value::Array(list)) => list,
            Some(_) => return Err(format!("{key} is a list of text")),
        };
        if list.len() > max {
            return Err(format!("{key} holds {max} at most"));
        }
        list.iter()
            .map(|v| match v.as_str() {
                Some(s) if s.chars().count() <= longest => Ok(s.to_string()),
                Some(_) => Err(format!("an item of {key} is longer than {longest}")),
                None => Err(format!("{key} is a list of text")),
            })
            .collect()
    };
    let act = match name {
        "draft.get" => DraftAct::Get,
        "draft.add" => DraftAct::Add {
            text: text("text")?,
            fresh: obj.get("fresh").and_then(Value::as_bool).unwrap_or(false),
        },
        "draft.picker" => {
            let keys = texts("keys", MAX_KEYS, MAX_KEY)?;
            if keys.iter().any(String::is_empty) {
                return Err("a key can't be empty".to_string());
            }
            DraftAct::Picker {
                label: text("label")?,
                keys,
                questions: texts("questions", MAX_KEYS, crate::drafts::MAX_CHARS)?,
                missing: texts("missing", MAX_KEYS, MAX_KEY)?,
            }
        }
        "draft.scratch" => DraftAct::Scratch,
        "draft.clear" => DraftAct::Clear,
        "draft.send" => DraftAct::Send,
        "draft.edit" => DraftAct::Edit,
        other => return Err(format!("no op called {other:?}")),
    };
    Ok(Op::Draft { pane, act })
}

fn event_name(obj: &Map<String, Value>) -> Result<String, String> {
    let Some(event) = obj.get("event").and_then(Value::as_str) else {
        return Err("emit needs an event".to_string());
    };
    if event.is_empty() || event.len() > MAX_EVENT || event.chars().any(char::is_control) {
        return Err(format!("{event:?} isn't an event name"));
    }
    if BUILT_IN_EVENTS.contains(&event) {
        return Err(format!(
            "{event} is WezTerm's own event, called with arguments of its own"
        ));
    }
    Ok(event.to_string())
}

fn line(id: &Value, ok: bool, fields: Map<String, Value>) -> String {
    let mut obj = Map::new();
    obj.insert("id".to_string(), id.clone());
    obj.insert("ok".to_string(), Value::Bool(ok));
    obj.extend(fields);
    Value::Object(obj).to_string()
}

/// The reply that says no, and why
pub fn refused(id: &Value, error: &str) -> String {
    let mut fields = Map::new();
    fields.insert("error".to_string(), Value::String(error.to_string()));
    line(id, false, fields)
}

/// `focus`'s reply
pub fn focus_reply(id: &Value, focus: &Focus) -> String {
    let fields = json!({
        "window": focus.window,
        "pane": focus.pane,
        "session": focus.session,
        "title": focus.title,
        "foreground": focus.foreground,
    });
    line(id, true, fields.as_object().cloned().unwrap_or_default())
}

/// A draft as replies carry it: null when there's none
pub fn draft_json(draft: Option<&Draft>) -> Value {
    let Some(d) = draft else {
        return Value::Null;
    };
    let (kind, pieces, missing) = match &d.body {
        Body::Text { pieces } => ("text", pieces.len(), vec![]),
        Body::Picker { keys, missing, .. } => ("picker", keys.len(), missing.clone()),
    };
    json!({
        "pane": d.pane,
        "session": d.session,
        "kind": kind,
        "text": d.text(),
        "pieces": pieces,
        "answered": d.answered(),
        "missing": missing,
    })
}

/// A `draft.*` request's reply: the draft as it is after it, and what it did (`removed`, `sent`,
/// `typed`)
pub fn draft_reply(id: &Value, draft: Option<&Draft>, did: Map<String, Value>) -> String {
    let mut fields = Map::new();
    fields.insert("draft".to_string(), draft_json(draft));
    fields.extend(did);
    line(id, true, fields)
}

/// `metrics`'s reply: what the counters and the process said, as their fields
pub fn metrics_reply(id: &Value, fields: Map<String, Value>) -> String {
    line(id, true, fields)
}

/// `emit`'s reply: the event was handed to the window, with the pane it ran with
pub fn emitted_reply(id: &Value, event: &str, window: u64, pane: u64) -> String {
    let fields = json!({ "event": event, "window": window, "pane": pane });
    line(id, true, fields.as_object().cloned().unwrap_or_default())
}

#[cfg(test)]
mod test {
    use super::*;

    fn reply(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn focus_and_emit_are_read() {
        let r = parse(r#"{"id": 7, "op": "focus"}"#).unwrap();
        assert_eq!(r, Request { id: json!(7), op: Op::Focus });
        let r = parse(r#"{"id":"a","op":"emit","event":"lbterm-tab-forward"}"#).unwrap();
        assert_eq!(r.op, Op::Emit { event: "lbterm-tab-forward".to_string() });
        assert_eq!(r.id, json!("a"));
        assert_eq!(parse(r#"{"op":"focus"}"#).unwrap().id, Value::Null, "an id is optional");
    }

    #[test]
    fn what_isnt_a_request_is_refused_with_its_id() {
        let r = reply(&parse("nonsense").unwrap_err());
        assert_eq!(r["ok"], json!(false));
        assert_eq!(r["id"], Value::Null);
        assert!(r["error"].as_str().unwrap().starts_with("not a JSON object"));

        let r = reply(&parse(r#"{"id":3,"op":"dance"}"#).unwrap_err());
        assert_eq!(r["id"], json!(3));
        assert_eq!(r["error"], json!(r#"no op called "dance""#));

        assert_eq!(reply(&parse(r#"[1]"#).unwrap_err())["error"], json!("not a JSON object"));
        assert_eq!(reply(&parse(r#"{"id":1}"#).unwrap_err())["error"], json!("no op"));
    }

    #[test]
    fn emit_needs_a_name_wezterm_doesnt_call_itself() {
        let error = |line: &str| reply(&parse(line).unwrap_err())["error"].as_str().unwrap().to_string();
        assert_eq!(error(r#"{"op":"emit"}"#), "emit needs an event");
        assert_eq!(error(r#"{"op":"emit","event":""}"#), r#""" isn't an event name"#);
        assert!(error(r#"{"op":"emit","event":"a\nb"}"#).ends_with("isn't an event name"));
        let long = "x".repeat(MAX_EVENT + 1);
        assert!(error(&format!(r#"{{"op":"emit","event":"{long}"}}"#)).ends_with("isn't an event name"));
        for name in BUILT_IN_EVENTS {
            let e = error(&format!(r#"{{"op":"emit","event":"{name}"}}"#));
            assert!(e.contains("WezTerm's own event"), "{name}: {e}");
        }
        // Any other name is the config's to answer, even one it doesn't handle
        assert!(parse(r#"{"op":"emit","event":"my-event"}"#).is_ok());
    }

    #[test]
    fn replies_carry_the_id_and_what_was_asked() {
        let focus = Focus {
            window: 2,
            pane: 14,
            session: Some("ff2a9a76-0000-4000-8000-000000000000".to_string()),
            title: "Claude".to_string(),
            foreground: true,
        };
        let r = reply(&focus_reply(&json!("q1"), &focus));
        assert_eq!(
            r,
            json!({
                "id": "q1", "ok": true, "window": 2, "pane": 14,
                "session": "ff2a9a76-0000-4000-8000-000000000000",
                "title": "Claude", "foreground": true
            })
        );
        let r = reply(&focus_reply(&Value::Null, &Focus::default()));
        assert_eq!(r["session"], Value::Null, "no Claude in the pane");

        let r = reply(&emitted_reply(&json!(5), "lbterm-tab-back", 0, 3));
        assert_eq!(r, json!({ "id": 5, "ok": true, "event": "lbterm-tab-back", "window": 0, "pane": 3 }));
    }

    #[test]
    fn draft_requests_are_read() {
        let op = |line: &str| parse(line).unwrap().op;
        assert_eq!(op(r#"{"op":"draft.get"}"#), Op::Draft { pane: None, act: DraftAct::Get });
        assert_eq!(
            op(r#"{"op":"draft.add","pane":14,"text":"1a\n2. yes"}"#),
            Op::Draft {
                pane: Some(14),
                act: DraftAct::Add { text: "1a\n2. yes".into(), fresh: false }
            }
        );
        assert_eq!(
            op(r#"{"op":"draft.add","text":"1a","fresh":true}"#),
            Op::Draft { pane: None, act: DraftAct::Add { text: "1a".into(), fresh: true } }
        );
        assert_eq!(
            op(r#"{"op":"draft.picker","label":"1b 2a","keys":["2","1","1"],"questions":["Which?","And?"]}"#),
            Op::Draft {
                pane: None,
                act: DraftAct::Picker {
                    label: "1b 2a".into(),
                    keys: vec!["2".into(), "1".into(), "1".into()],
                    questions: vec!["Which?".into(), "And?".into()],
                    missing: vec![],
                }
            }
        );
        for (name, act) in [
            ("draft.scratch", DraftAct::Scratch),
            ("draft.clear", DraftAct::Clear),
            ("draft.send", DraftAct::Send),
            ("draft.edit", DraftAct::Edit),
        ] {
            let got = op(&format!(r#"{{"op":"{name}","pane":null}}"#));
            assert_eq!(got, Op::Draft { pane: None, act });
            assert_eq!(got.name(), name);
        }
    }

    #[test]
    fn a_draft_request_missing_what_it_needs_is_refused() {
        let error = |line: &str| reply(&parse(line).unwrap_err())["error"].as_str().unwrap().to_string();
        assert_eq!(error(r#"{"op":"draft.add"}"#), "draft.add needs text");
        assert_eq!(error(r#"{"op":"draft.get","pane":"x"}"#), "pane is a pane's id, a number");
        assert_eq!(error(r#"{"op":"draft.picker","keys":["1"]}"#), "draft.picker needs label");
        assert_eq!(error(r#"{"op":"draft.picker","label":"1a","keys":"1"}"#), "keys is a list of text");
        assert_eq!(error(r#"{"op":"draft.picker","label":"1a","keys":[""]}"#), "a key can't be empty");
        let many = vec!["1"; MAX_KEYS + 1];
        let line = json!({ "op": "draft.picker", "label": "1a", "keys": many }).to_string();
        assert_eq!(error(&line), format!("keys holds {MAX_KEYS} at most"));
        assert_eq!(error(r#"{"op":"draft.dance"}"#), r#"no op called "draft.dance""#);
    }

    #[test]
    fn the_mic_is_told_its_state_ttl_and_what_was_heard() {
        let op = |line: &str| parse(line).unwrap().op;
        assert_eq!(
            op(r#"{"op":"mic","state":"listening"}"#),
            Op::Mic { state: Some(MicState::Listening), ttl: crate::mic::TTL, heard: None }
        );
        assert_eq!(
            op(r#"{"op":"mic","state":"hearing","ttl":5,"heard":"typed 1a"}"#),
            Op::Mic {
                state: Some(MicState::Hearing),
                ttl: Duration::from_secs(5),
                heard: Some("typed 1a".into())
            }
        );
        assert_eq!(op(r#"{"op":"mic","state":"off"}"#).name(), "mic");
        let error = |line: &str| reply(&parse(line).unwrap_err())["error"].as_str().unwrap().to_string();
        assert!(error(r#"{"op":"mic"}"#).starts_with("mic needs a state"));
        assert!(error(r#"{"op":"mic","state":"loud"}"#).contains("not \"loud\""));
        assert_eq!(error(r#"{"op":"mic","state":"off","heard":3}"#), "heard is text");
        assert_eq!(reply(&mic_reply(&json!(2), Some(MicState::Muted))), json!({ "id": 2, "ok": true, "mic": "muted" }));
        assert_eq!(reply(&mic_reply(&json!(2), None))["mic"], Value::Null);
    }

    #[test]
    fn metrics_is_read_with_or_without_a_reset() {
        let op = |line: &str| parse(line).unwrap().op;
        let plain = Op::Metrics { reset: false, force: false, present: None };
        assert_eq!(op(r#"{"op":"metrics"}"#), plain);
        assert_eq!(op(r#"{"op":"metrics","reset":true}"#), Op::Metrics { reset: true, force: false, present: None });
        assert_eq!(op(r#"{"op":"metrics","reset":null}"#), plain);
        assert_eq!(
            op(r#"{"op":"metrics","reset":true,"force":true}"#),
            Op::Metrics { reset: true, force: true, present: None }
        );
        assert_eq!(op(r#"{"op":"metrics","present":"on"}"#), Op::Metrics { reset: false, force: false, present: Some(true) });
        assert_eq!(op(r#"{"op":"metrics","present":false}"#), Op::Metrics { reset: false, force: false, present: Some(false) });
        assert_eq!(op(r#"{"op":"metrics"}"#).name(), "metrics");
        let r = reply(&parse(r#"{"id":4,"op":"metrics","reset":"yes"}"#).unwrap_err());
        assert_eq!(r, json!({ "id": 4, "ok": false, "error": "reset is true or false" }));
        let r = reply(&parse(r#"{"id":4,"op":"metrics","present":"maybe"}"#).unwrap_err());
        assert_eq!(r["error"], "present is on or off");
        let r = reply(&parse(r#"{"id":4,"op":"metrics","force":1}"#).unwrap_err());
        assert_eq!(r["error"], "force is true or false");
        let mut fields = Map::new();
        fields.insert("totals".into(), json!({ "frames": 3 }));
        assert_eq!(
            reply(&metrics_reply(&json!(4), fields)),
            json!({ "id": 4, "ok": true, "totals": { "frames": 3 } })
        );
    }

    #[test]
    fn a_draft_reply_carries_the_draft_and_what_was_done() {
        let mut d = Draft::new(14, Some("s1".into()));
        d.add("1a").unwrap();
        d.add("\n2. yes").unwrap();
        let mut did = Map::new();
        did.insert("removed".into(), json!("\n3. no"));
        let r = reply(&draft_reply(&json!(9), Some(&d), did));
        assert_eq!(
            r,
            json!({
                "id": 9, "ok": true, "removed": "\n3. no",
                "draft": {
                    "pane": 14, "session": "s1", "kind": "text", "text": "1a\n2. yes",
                    "pieces": 2, "answered": [1, 2], "missing": []
                }
            })
        );
        assert_eq!(reply(&draft_reply(&json!(1), None, Map::new()))["draft"], Value::Null);
    }
}
