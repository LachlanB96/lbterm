//! LBTerm: OSC 7501, the Program Status Protocol
//! (<https://mitchellh.com/writing/program-status-osc7501>; wire format
//! <https://www.superlogical.com/rex/docs/build/program-status>).
//!
//! A program tells the terminal what it's doing (idle, working, blocked on
//! the user and why, done, failed) for itself and for parts of itself under
//! ids. It first asks whether the terminal understands (`OSC 7501 ; ?`) and
//! only reports once it has an answer.

use crate::allocate::*;
use crate::{bail, ensure, Result};
use base64::Engine;
use core::fmt::{Display, Formatter, Result as FmtResult};
use core::str;
#[cfg(feature = "use_serde")]
use serde::{Deserialize, Serialize};

/// The whole sequence, `OSC` to `ST`, is at most this long
const MAX_SEQUENCE: usize = 4096;
/// `ESC ] 7501 ;` and `ESC \` around the body
const FRAMING: usize = 8;
const MAX_KEY: usize = 16;
const MAX_ID: usize = 128;
const MAX_SEGMENT: usize = 32;
const MAX_DEPTH: usize = 8;
const MAX_APP: usize = 32;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE: usize = 192;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG: usize = 2048;

/// What the terminal sends back to a query: it speaks the protocol, with
/// the default states and kinds.
pub const REPLY: &str = "\x1b]7501;?\x1b\\";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub enum ProgramState {
    /// At rest, waiting for the next instruction
    Idle,
    Working,
    /// Finished, with a result the user hasn't seen yet
    Done,
    /// Needs the user
    Blocked,
    /// Failed and stopped
    Error,
    /// Not a state: removes the record and everything under it
    Clear,
}

impl ProgramState {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "error" => Self::Error,
            "clear" => Self::Clear,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Error => "error",
            Self::Clear => "clear",
        }
    }
}

/// Why a program is blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "use_serde", derive(Serialize, Deserialize))]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

impl BlockedKind {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "permission" => Self::Permission,
            "question" => Self::Question,
            "auth" => Self::Auth,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

/// One report, checked against the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusReport {
    pub state: ProgramState,
    /// `None` for the program's root record; else a `/`-separated path
    pub id: Option<String>,
    /// Only with `blocked`
    pub kind: Option<BlockedKind>,
    /// 0-100, only with `working` or `blocked`; `None` is indeterminate
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

impl ProgramStatusReport {
    pub fn new(state: ProgramState) -> Self {
        Self {
            state,
            id: None,
            kind: None,
            progress: None,
            app: None,
            title: None,
            msg: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramStatus {
    /// Does the terminal speak the protocol? (A reply looks the same.)
    Query,
    Report(ProgramStatusReport),
}

fn value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b)
}

fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.+-".contains(&b)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id.split('/').count() <= MAX_DEPTH
        && id
            .split('/')
            .all(|s| !s.is_empty() && s.len() <= MAX_SEGMENT && s.bytes().all(name_byte))
}

/// Base64 UTF-8 with optional padding and no control characters.
fn decode_text(encoded: &str, max_encoded: usize, max_decoded: usize) -> Result<String> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    ensure!(encoded.len() <= max_encoded, "encoded text too long");
    let bytes = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    )
    .decode(encoded)
    .map_err(|err| crate::format_err!("base64: {:#}", err))?;
    ensure!(bytes.len() <= max_decoded, "decoded text too long");
    let text = String::from_utf8(bytes)?;
    ensure!(
        !text
            .chars()
            .any(|c| matches!(c as u32, 0..=0x1f | 0x7f | 0x80..=0x9f)),
        "control character in text"
    );
    Ok(text)
}

impl ProgramStatus {
    /// `osc` is the sequence split at `;`, its first part `7501`. An error
    /// means the report is to be ignored.
    pub fn parse(osc: &[&[u8]]) -> Result<Self> {
        ensure!(osc.len() == 2, "OSC 7501 takes one parameter");
        let body = osc[1];
        if body.first() == Some(&b'?') {
            return Ok(Self::Query);
        }
        ensure!(body.len() + FRAMING <= MAX_SEQUENCE, "OSC 7501 too long");
        let body = str::from_utf8(body)?;

        let (mut state, mut id, mut kind, mut progress, mut app, mut title, mut msg) =
            (None, None, None, None, None, None, None);
        for pair in body.split(':') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            ensure!(key.len() <= MAX_KEY, "key too long");
            if key.is_empty()
                || !key.bytes().all(|b| b.is_ascii_lowercase())
                || !value.bytes().all(value_byte)
            {
                continue;
            }
            match key {
                "state" => state = Some(value),
                "id" => id = Some(value),
                "kind" => kind = Some(value),
                "progress" => progress = Some(value),
                "app" => app = Some(value),
                "title" => title = Some(value),
                "msg" => msg = Some(value),
                _ => {}
            }
        }

        let Some(state) = state.and_then(ProgramState::parse) else {
            bail!("OSC 7501 without a known state");
        };
        if let Some(id) = id {
            ensure!(valid_id(id), "OSC 7501 id {id:?} is malformed");
        }
        let mut report = ProgramStatusReport::new(state);
        report.id = id.map(ToString::to_string);
        if state == ProgramState::Blocked {
            report.kind = kind.and_then(BlockedKind::parse);
        }
        if matches!(state, ProgramState::Working | ProgramState::Blocked) {
            report.progress = progress
                .and_then(|p| p.parse::<u8>().ok())
                .filter(|p| *p <= 100);
        }
        report.app = app
            .filter(|a| !a.is_empty() && a.len() <= MAX_APP && a.bytes().all(name_byte))
            .map(ToString::to_string);
        report.title = title
            .map(|t| decode_text(t, MAX_TITLE_ENCODED, MAX_TITLE))
            .transpose()?;
        report.msg = msg
            .map(|m| decode_text(m, MAX_MSG_ENCODED, MAX_MSG))
            .transpose()?;
        Ok(Self::Report(report))
    }
}

impl Display for ProgramStatus {
    /// The body after `OSC`: `7501;...`
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::Query => write!(f, "7501;?"),
            Self::Report(r) => {
                let encode = |s: &String| base64::engine::general_purpose::STANDARD.encode(s);
                write!(f, "7501;state={}", r.state.as_str())?;
                if let Some(id) = &r.id {
                    write!(f, ":id={id}")?;
                }
                if let Some(kind) = r.kind {
                    write!(f, ":kind={}", kind.as_str())?;
                }
                if let Some(progress) = r.progress {
                    write!(f, ":progress={progress}")?;
                }
                if let Some(app) = &r.app {
                    write!(f, ":app={app}")?;
                }
                if let Some(title) = &r.title {
                    write!(f, ":title={}", encode(title))?;
                }
                if let Some(msg) = &r.msg {
                    write!(f, ":msg={}", encode(msg))?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse(body: &str) -> Result<ProgramStatus> {
        ProgramStatus::parse(&[b"7501", body.as_bytes()])
    }

    fn report(body: &str) -> ProgramStatusReport {
        match parse(body).unwrap() {
            ProgramStatus::Report(r) => r,
            ProgramStatus::Query => panic!("{body} is a query"),
        }
    }

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[test]
    fn a_query_or_a_reply_starts_with_a_question_mark() {
        assert_eq!(parse("?").unwrap(), ProgramStatus::Query);
        assert_eq!(parse("?:states=idle,working").unwrap(), ProgramStatus::Query);
        assert_eq!(REPLY, "\x1b]7501;?\x1b\\");
    }

    #[test]
    fn claude_codes_reports_parse() {
        let r = report(&format!(
            "state=blocked:kind=permission:app=claude-code:title={}:msg={}",
            b64("Tab panel"),
            b64("Allow Bash: cargo test?")
        ));
        assert_eq!(r.state, ProgramState::Blocked);
        assert_eq!(r.kind, Some(BlockedKind::Permission));
        assert_eq!(r.app.as_deref(), Some("claude-code"));
        assert_eq!(r.title.as_deref(), Some("Tab panel"));
        assert_eq!(r.msg.as_deref(), Some("Allow Bash: cargo test?"));
        assert_eq!(r.id, None);
        let sub = report("state=working:id=agent/7:progress=40");
        assert_eq!((sub.id.as_deref(), sub.progress), (Some("agent/7"), Some(40)));
    }

    #[test]
    fn a_report_without_a_known_state_is_ignored() {
        assert!(parse("app=x").is_err());
        assert!(parse("state=sleeping").is_err());
        assert_eq!(report("state=clear").state, ProgramState::Clear);
    }

    #[test]
    fn bad_pairs_are_skipped_and_the_rest_kept() {
        // No '=', an uppercase key, a value byte outside the set
        let r = report("junk:state=working:Kind=question:app=a b:progress=5");
        assert_eq!(r.state, ProgramState::Working);
        assert_eq!(r.progress, Some(5));
        assert_eq!(r.app, None);
        // Whitespace round keys and values goes, and the last of a key wins
        let r = report(" state = idle : state=done ");
        assert_eq!(r.state, ProgramState::Done);
    }

    #[test]
    fn values_out_of_range_are_dropped_not_fatal() {
        let r = report("state=blocked:kind=coffee:progress=101:app=way-too-long-an-app-name-for-the-limit-x");
        assert_eq!((r.kind, r.progress, r.app), (None, None, None));
        // kind and progress belong to some states only
        let r = report("state=done:kind=question:progress=50");
        assert_eq!((r.kind, r.progress), (None, None));
    }

    #[test]
    fn malformed_ids_lose_the_report() {
        assert!(parse("state=working:id=a//b").is_err());
        assert!(parse("state=working:id=a/b/c/d/e/f/g/h/i").is_err(), "nine levels");
        assert!(parse(&format!("state=working:id={}", "x".repeat(33))).is_err());
        assert!(parse("state=working:id=a/b/c/d/e/f/g/h").is_ok());
    }

    #[test]
    fn text_that_wont_decode_or_carries_controls_loses_the_report() {
        // A single base64 character is no whole byte
        assert!(parse("state=done:msg=A").is_err());
        // A byte outside the value set only loses its pair
        assert_eq!(report("state=done:msg=!!!").msg, None);
        assert!(parse(&format!("state=done:msg={}", b64("two\nlines"))).is_err());
        assert!(parse(&format!("state=done:title={}", b64(&"t".repeat(193)))).is_err());
        // Padding is optional
        let unpadded = b64("ab").trim_end_matches('=').to_string();
        assert_eq!(report(&format!("state=done:msg={unpadded}")).msg.as_deref(), Some("ab"));
    }

    #[test]
    fn size_limits_lose_the_report() {
        assert!(parse(&format!("state=idle:msg={}", "A".repeat(4100))).is_err());
        assert!(parse("state=idle:averyveryverylongkey=1").is_err());
    }

    #[test]
    fn reports_write_back_as_they_were_read() {
        let mut r = ProgramStatusReport::new(ProgramState::Blocked);
        r.id = Some("deploy/eu".into());
        r.kind = Some(BlockedKind::Question);
        r.progress = Some(12);
        r.app = Some("claude-code".into());
        r.title = Some("Tab panel".into());
        r.msg = Some("Which region?".into());
        let text = ProgramStatus::Report(r.clone()).to_string();
        let body = text.strip_prefix("7501;").unwrap();
        assert_eq!(report(body), r);
    }
}
