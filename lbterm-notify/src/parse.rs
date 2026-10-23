//! Reading one OSC 99 sequence

use base64::Engine;

/// What a sequence's payload is (`p=`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PayloadType {
    Title,
    Body,
    /// Close the notification with this id
    Close,
    /// Icon data; LBTerm takes it as a chunk but doesn't show it
    Icon,
    /// What the terminal supports (`p=?`)
    Query,
    /// Which notifications are still open
    Alive,
    /// Button labels, separated by U+2028
    Buttons,
}

impl PayloadType {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "title" => Self::Title,
            "body" => Self::Body,
            "close" => Self::Close,
            "icon" => Self::Icon,
            "?" => Self::Query,
            "alive" => Self::Alive,
            "buttons" => Self::Buttons,
            _ => return None,
        })
    }
}

/// What a click on the notification does (`a=`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Actions {
    /// Bring the window and the pane that sent it forward
    pub focus: bool,
    /// Tell the program, which reads `ESC ] 99 ; i=<id> ; ESC \`
    pub report: bool,
}

impl Default for Actions {
    fn default() -> Self {
        Self {
            focus: true,
            report: false,
        }
    }
}

/// When the notification is shown at all (`o=`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "use_serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Occasion {
    #[default]
    Always,
    /// Only while the window hasn't got the keyboard
    Unfocused,
    /// Only while the pane can't be seen: the window is unfocused, or the pane is in a tab
    /// that isn't showing
    Invisible,
}

/// One OSC 99 sequence. Each key is `None` when the sequence didn't give it, so a later chunk
/// only changes what it mentions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Request {
    /// `i=`, already stripped to the characters the protocol allows
    pub id: Option<String>,
    /// `d=`: false while more chunks follow
    pub done: Option<bool>,
    pub payload_type: Option<PayloadType>,
    pub actions: Option<Actions>,
    pub occasion: Option<Occasion>,
    /// `u=`: 0 low, 1 normal, 2 critical
    pub urgency: Option<u8>,
    /// `c=1`: tell the program when it closes
    pub close_event: Option<bool>,
    /// `w=`: -1 the system's choice, 0 never, otherwise milliseconds
    pub expire_ms: Option<i64>,
    /// `f=`, decoded
    pub app_name: Option<String>,
    /// `s=`, decoded: `system`, `silent`, `error`, `warning`, `info`, `question` or a name
    pub sound: Option<String>,
    /// The payload, base64-decoded when `e=1`
    pub payload: Vec<u8>,
}

/// The characters an id may hold; anything else is dropped before it's echoed back
pub fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '.'))
        .collect()
}

fn decode_b64(value: &str) -> Option<Vec<u8>> {
    let engine = &base64::engine::general_purpose::STANDARD;
    let trimmed = value.trim_end_matches('=');
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(trimmed)
        .or_else(|_| engine.decode(value))
        .ok()
}

fn decode_text(value: &str) -> Option<String> {
    String::from_utf8(decode_b64(value)?).ok()
}

/// Read one sequence from the parts of the OSC split at `;`, the first being `99`. `None`
/// when it isn't OSC 99 or is too broken to use. Unknown keys and values are ignored, as the
/// protocol asks.
pub fn parse(parts: &[&[u8]]) -> Option<Request> {
    if parts.first() != Some(&&b"99"[..]) {
        return None;
    }
    let metadata = std::str::from_utf8(parts.get(1).copied().unwrap_or(b"")).ok()?;
    // The payload is everything after the second ';', semicolons and all
    let payload: Vec<u8> = match parts.get(2..) {
        Some(rest) if !rest.is_empty() => rest.join(&b';'),
        _ => vec![],
    };

    let mut request = Request::default();
    let mut encoded = false;
    for pair in metadata.split(':').filter(|p| !p.is_empty()) {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "i" => request.id = Some(sanitize_id(value)),
            "d" => request.done = Some(value != "0"),
            "e" => encoded = value == "1",
            "p" => request.payload_type = PayloadType::parse(value).or(request.payload_type),
            "a" => {
                let mut actions = request.actions.unwrap_or_default();
                for action in value.split(',') {
                    match action {
                        "focus" => actions.focus = true,
                        "-focus" => actions.focus = false,
                        "report" => actions.report = true,
                        "-report" => actions.report = false,
                        _ => {}
                    }
                }
                request.actions = Some(actions);
            }
            "o" => {
                request.occasion = match value {
                    "always" => Some(Occasion::Always),
                    "unfocused" => Some(Occasion::Unfocused),
                    "invisible" => Some(Occasion::Invisible),
                    _ => request.occasion,
                }
            }
            "u" => request.urgency = value.parse::<u8>().ok().filter(|u| *u <= 2),
            "c" => request.close_event = Some(value == "1"),
            "w" => request.expire_ms = value.parse::<i64>().ok().filter(|w| *w >= -1),
            "f" => request.app_name = decode_text(value),
            "s" => request.sound = decode_text(value),
            _ => {}
        }
    }
    request.payload = if encoded {
        decode_b64(std::str::from_utf8(&payload).ok()?)?
    } else {
        payload
    };
    Some(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osc(text: &str) -> Option<Request> {
        let parts: Vec<&[u8]> = text.as_bytes().split(|b| *b == b';').collect();
        parse(&parts)
    }

    #[test]
    fn a_bare_title() {
        let r = osc("99;;Hello world").unwrap();
        assert_eq!(r.payload, b"Hello world");
        assert_eq!(r.payload_type, None);
        assert_eq!(r.done, None);
        assert_eq!(r.id, None);
    }

    #[test]
    fn keys() {
        let r = osc("99;i=build-42:d=0:p=body:a=report,-focus:o=unfocused:u=2:c=1:w=5000;Done").unwrap();
        assert_eq!(r.id.as_deref(), Some("build-42"));
        assert_eq!(r.done, Some(false));
        assert_eq!(r.payload_type, Some(PayloadType::Body));
        assert_eq!(r.actions, Some(Actions { focus: false, report: true }));
        assert_eq!(r.occasion, Some(Occasion::Unfocused));
        assert_eq!(r.urgency, Some(2));
        assert_eq!(r.close_event, Some(true));
        assert_eq!(r.expire_ms, Some(5000));
        assert_eq!(r.payload, b"Done");
    }

    #[test]
    fn base64_payload_and_names() {
        // "Claude Code", "silent", "héllo"
        let r = osc("99;e=1:f=Q2xhdWRlIENvZGU=:s=c2lsZW50;aMOpbGxv").unwrap();
        assert_eq!(r.app_name.as_deref(), Some("Claude Code"));
        assert_eq!(r.sound.as_deref(), Some("silent"));
        assert_eq!(String::from_utf8(r.payload).unwrap(), "héllo");
        // padding may be left off
        let r = osc("99;e=1;aMOpbGxv").unwrap();
        assert_eq!(String::from_utf8(r.payload).unwrap(), "héllo");
    }

    #[test]
    fn semicolons_in_the_payload_are_kept() {
        let r = osc("99;p=body;a; b; c").unwrap();
        assert_eq!(r.payload, b"a; b; c");
    }

    #[test]
    fn ids_are_stripped_to_what_may_be_echoed() {
        let r = osc("99;i=ok_1.2+x-y\x1b]evil;x").unwrap();
        assert_eq!(r.id.as_deref(), Some("ok_1.2+x-yevil"));
    }

    #[test]
    fn unknown_keys_and_values_are_ignored() {
        let r = osc("99;z=1:p=sparkles:u=7:o=sometimes:w=-5;x").unwrap();
        assert_eq!(r.payload_type, None);
        assert_eq!(r.urgency, None);
        assert_eq!(r.occasion, None);
        assert_eq!(r.expire_ms, None);
    }

    #[test]
    fn not_osc_99() {
        assert_eq!(osc("9;hello"), None);
        assert_eq!(osc("777;notify;a;b"), None);
    }

    #[test]
    fn queries() {
        assert_eq!(osc("99;i=q1:p=?;").unwrap().payload_type, Some(PayloadType::Query));
        assert_eq!(osc("99;i=q1:p=alive;").unwrap().payload_type, Some(PayloadType::Alive));
    }
}
