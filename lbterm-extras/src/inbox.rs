//! The InMail button on the window strip (`config.lbterm_inbox`, lbterm/INBOX.md): the messages
//! waiting in a set of folders, one Markdown file each, with a front matter block of `key: value`
//! lines, as Lachlan's projects send each other (his InMail):
//!
//! ```text
//! ---
//! from: Terminal
//! to: LBTerm
//! sent: 2026-10-23T14:02
//! subject: "The rail's question light"
//! ---
//!
//! The body.
//! ```
//!
//! A message is waiting while its file is in the folder: whatever deals with it moves it out.
//! [`Scan`] reads the folders each time it's asked, opening only the files that are new or
//! changed since it last looked, and [`Watch`] says when something in them has changed, so the
//! GUI's thread of its own looks only then and sleeps while nothing does.

use chrono::NaiveDateTime;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One waiting message.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Its file
    pub file: PathBuf,
    /// The file's name, `2026-10-23-1402-from-Terminal-the-rails-question-light.md`
    pub name: String,
    /// The folder it waits in
    pub folder: PathBuf,
    /// The folder above that: the project it was sent to, where a tab opens to deal with it
    pub dir: PathBuf,
    /// Who it's for: its `to`, else the name of `dir`
    pub project: String,
    /// Who sent it: its `from`, else nothing
    pub from: String,
    /// Its `subject`, else the file's name without `.md`
    pub subject: String,
    /// Its `sent` as written, else nothing
    pub sent: String,
    /// When it was sent: its `sent`, else the file's time
    pub at: Option<NaiveDateTime>,
    /// The conversation working on it, from the claim beside it ([`claim`]), while one holds it
    pub taken: Option<Taken>,
}

/// A conversation's claim on a message: `<message>.taken` beside it, which InMail writes when a
/// conversation reads it (`@Blueprint:inmail-files`, 2026-11-08)
#[derive(Debug, Clone, PartialEq)]
pub struct Taken {
    /// The Claude Code session id of the conversation working on it
    pub session: String,
    /// When it took it, local time
    pub at: Option<NaiveDateTime>,
}

/// The file a message's claim would be: its own path with `.taken` after the `.md`
pub fn claim_file(message: &Path) -> PathBuf {
    let mut name = message.as_os_str().to_owned();
    name.push(".taken");
    PathBuf::from(name)
}

/// What a claim's JSON says, `{ "session": "<id>", "at": "2026-11-08T15:02" }`, while it holds:
/// nothing once it's been handed back (`released` there), or for a session that isn't a Claude
/// Code id (36 characters of hex and dashes, 8-4-4-4-12), which nothing should start a
/// conversation from. The file alone says it: whether that conversation is still open is
/// Claude's to say (its liveConversations, 0.65 to 1.4 s to read), so it isn't asked.
pub fn claim(text: &str) -> Option<Taken> {
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let object = value.as_object()?;
    if object.get("released").map_or(false, |r| !r.is_null() && r != &serde_json::Value::Bool(false)) {
        return None;
    }
    let session = object.get("session")?.as_str()?.trim().to_lowercase();
    let groups: Vec<&str> = session.split('-').collect();
    let shaped = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|g| g.chars().all(|c| c.is_ascii_hexdigit()));
    if !shaped {
        return None;
    }
    let at = object.get("at").and_then(|a| a.as_str()).and_then(parse_sent);
    Some(Taken { session, at })
}

/// A taken message's words in the list, `taken 15:02`: the time today, as the prompt list says
/// it, else the day or date it was taken; `taken` alone when the claim doesn't say when
pub fn taken_words(taken: &Taken, now: NaiveDateTime) -> String {
    match taken.at {
        Some(at) => format!("taken {}", crate::prompts::stamp(at, now)),
        None => "taken".to_string(),
    }
}

/// The `key: value` lines between a file's opening `---` and the next, as InMail writes them
/// (Life\Projects' inmail.ts, `parse`): a value in double quotes is a JSON string, one in
/// single quotes has its `''` made `'`. Nothing without a front matter block.
pub fn front_matter(text: &str) -> HashMap<String, String> {
    let mut meta = HashMap::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return meta;
    };
    for line in rest.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim_end() == "---" {
            return meta;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            continue;
        }
        let value = value.trim();
        let value = if value.starts_with('"') {
            serde_json::from_str::<String>(value).unwrap_or_else(|_| value.to_string())
        } else if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
            value[1..value.len() - 1].replace("''", "'")
        } else {
            value.to_string()
        };
        meta.insert(key.to_string(), value);
    }
    // No closing line: not a front matter block after all
    HashMap::new()
}

/// When `sent` says, as InMail writes it: `2026-10-23T14:02`, local time
pub fn parse_sent(sent: &str) -> Option<NaiveDateTime> {
    let sent = sent.trim();
    NaiveDateTime::parse_from_str(sent, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(sent, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(sent, "%Y-%m-%d %H:%M"))
        .ok()
}

fn local(time: SystemTime) -> NaiveDateTime {
    chrono::DateTime::<chrono::Local>::from(time).naive_local()
}

/// The message in `file`, which reads `text` and was last written at `modified`
pub fn message(file: &Path, text: &str, modified: Option<SystemTime>) -> Message {
    let meta = front_matter(text);
    let get = |key: &str| meta.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = folder.parent().map(Path::to_path_buf).unwrap_or_default();
    let project = get("to").unwrap_or_else(|| {
        dir.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let subject = get("subject").unwrap_or_else(|| {
        let stem = name.strip_suffix(".md").or_else(|| name.strip_suffix(".MD")).unwrap_or(&name);
        stem.to_string()
    });
    let sent = get("sent").unwrap_or_default();
    let at = parse_sent(&sent).or_else(|| modified.map(local));
    Message {
        file: file.to_path_buf(),
        name,
        folder,
        dir,
        project,
        from: get("from").unwrap_or_default(),
        subject,
        sent,
        at,
        taken: None,
    }
}

/// Oldest first, as InMail lists them; the list shows them newest at the top
pub fn order(messages: &mut [Message]) {
    messages.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.name.cmp(&b.name)));
}

/// What the button says after its glyph: nothing with none waiting, else how many, however many
/// that is (Lachlan's ask, 2026-10-28: it stopped at "99+" until then)
pub fn badge(count: usize) -> String {
    match count {
        0 => String::new(),
        n => n.to_string(),
    }
}

/// The count's colour: the rail's waiting orange, since mail waiting is something that needs you
/// (lbterm/DESIGN.md, "Colour")
pub const COUNT: u32 = 0xFF8A3D;

/// The button's colours, as sRGB 0..1, from the strip's own colour: white laid over it, as the
/// restart pill's are (restart::pill_colors). With nothing waiting it sits back, a hairline at
/// white 14% round a glyph at 45%; with mail waiting the hairline is at 35% and the glyph at 92%,
/// the count beside it in the waiting orange; under the pointer, or with its list open, the
/// hairline is at 50% and the glyph white. Its fill is always the strip's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonColors {
    pub outline: [f32; 3],
    pub glyph: [f32; 3],
    pub count: [f32; 3],
}

pub fn button_colors(surface: [f32; 3], waiting: bool, lit: bool) -> ButtonColors {
    let over = |alpha: f32| surface.map(|c| c + (1. - c) * alpha);
    let (glyph, outline) = match (waiting, lit) {
        (_, true) => (1.0, 0.50),
        (true, false) => (0.92, 0.35),
        (false, false) => (0.45, 0.14),
    };
    let byte = |shift: u32| ((COUNT >> shift) & 0xFF) as f32 / 255.;
    ButtonColors {
        outline: over(outline),
        glyph: over(glyph),
        count: [byte(16), byte(8), byte(0)],
    }
}

/// How a link to one message starts: `lbterm://inmail/<project>?message=<file name>`, each part
/// percent-encoded, as Lachlan's InMail writes it after sending one (Life\Projects' inmail.ts,
/// `readUrl`). What a click on it does is the config's `open-uri` handler's.
pub const LINK: &str = "lbterm://inmail/";

/// A link's words while its message waits: the waiting orange, as the button's count
pub const LINK_WAITING: u32 = COUNT;
/// ...and once it's gone: white at 40% over the surface (`#121212`), struck through
pub const LINK_READ: u32 = 0x717171;
/// ...and while a conversation has taken it: the rail's working green (lbterm-panel's palette),
/// since a session is working on it (Lachlan's E1 of 2026-11-08). The list's `taken 15:02` too
pub const LINK_TAKEN: u32 = 0x3DDC84;

/// Whether the message a link names still waits
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// In its project's folder still
    Waiting,
    /// In its folder still, with a conversation's claim on it ([`claim`])
    Taken,
    /// Moved out: dealt with
    Read,
}

/// `%XX` made the byte it stands for; a `%` without two hex digits stays as it is
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The project and file name a link to one message names, decoded, or nothing for any other link:
/// a project and a `message` that's a name alone, no folder in it
pub fn link_message(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix(LINK)?;
    let (project, query) = rest.split_once('?')?;
    let name = query.strip_prefix("message=")?;
    if project.is_empty() || name.is_empty() || name.contains(['&', '#']) {
        return None;
    }
    let (project, name) = (percent_decode(project), percent_decode(name));
    if name.contains(['/', '\\']) {
        return None;
    }
    Some((project, name))
}

/// For a link to one message, whether it still waits among `waiting`: a message of that file name
/// for that project, both in any case (Windows' names, and a project as a link writes it). Nothing
/// for any other link, and nothing for a link to a project none of `folders` is the InMail of
/// ([`looks_in`]) with no message of its waiting: whether its message waits can't be known, so it's
/// drawn as any link is (until 2026-11-03 it was drawn read, and a project made after the config
/// was read had every link to its mail drawn grey while the mail waited: Relay's InMail that day).
pub fn link_state(uri: &str, waiting: &[Message], folders: &[String]) -> Option<LinkState> {
    let (project, name) = link_message(uri)?;
    let (lower, name) = (project.to_lowercase(), name.to_lowercase());
    let waits = waiting
        .iter()
        .find(|m| m.name.to_lowercase() == name && m.project.to_lowercase() == lower);
    if let Some(m) = waits {
        return Some(if m.taken.is_some() { LinkState::Taken } else { LinkState::Waiting });
    }
    looks_in(&project, folders).then_some(LinkState::Read)
}

/// Whether one of `folders` is `project`'s InMail: the folder above it ends with the project's
/// parts, in any case, as a link names a project (its folder's name, `Atlas`; a code base by its
/// own name, `LBTerm`, or by its path under the root, `Terminal/code/LBTerm`)
pub fn looks_in(project: &str, folders: &[String]) -> bool {
    let parts: Vec<String> = project
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .map(str::to_lowercase)
        .collect();
    if parts.is_empty() {
        return false;
    }
    folders.iter().any(|folder| {
        let above: Vec<String> = Path::new(folder)
            .parent()
            .map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        above.len() >= parts.len() && above[above.len() - parts.len()..] == parts[..]
    })
}

/// What a message's row in the list says: when it was sent (the prompt list's stamps), who it's
/// for, its subject, who sent it ("from Terminal") or nothing, and whether a conversation has
/// taken it ([`taken_words`], "taken 15:02") or nothing
pub fn row(m: &Message, now: NaiveDateTime) -> (String, String, String, String, String) {
    let stamp = m.at.map(|at| crate::prompts::stamp(at, now)).unwrap_or_default();
    let from = if m.from.is_empty() {
        String::new()
    } else {
        format!("from {}", m.from)
    };
    let taken = m.taken.as_ref().map(|t| taken_words(t, now)).unwrap_or_default();
    (stamp, m.project.clone(), crate::prompts::one_line(&m.subject), from, taken)
}

/// A file's size and time, which say when it must be read again
type Stamp = (Option<SystemTime>, u64);

/// The folders' messages, kept between looks so only new and changed files are read.
#[derive(Default)]
pub struct Scan {
    seen: HashMap<PathBuf, (Stamp, Message)>,
    /// The claims beside them ([`claim`]), read again only when one's file changes
    claims: HashMap<PathBuf, (Stamp, Option<Taken>)>,
    /// Message files the last look couldn't read
    unread: usize,
}

impl Scan {
    /// Every message waiting in `folders` now, oldest first. A folder that isn't there has none;
    /// a file that can't be read now (still being written, held open by a scanner) is left for
    /// the next look, and counted in [`Self::unread`].
    pub fn look(&mut self, folders: &[String]) -> Vec<Message> {
        let mut found = vec![];
        let mut keep: HashMap<PathBuf, (Stamp, Message)> = HashMap::new();
        let mut claims: HashMap<PathBuf, (Stamp, Option<Taken>)> = HashMap::new();
        self.unread = 0;
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            let entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
            // The folder's claims first, by the message they're beside: a message's claim is read
            // again only when its file has changed, and one that can't be read now holds nothing
            // until the next look (the watch brings one when it's written)
            for entry in &entries {
                let path = entry.path();
                let is_claim = path
                    .extension()
                    .map_or(false, |e| e.to_string_lossy().eq_ignore_ascii_case("taken"));
                let Some(meta) = is_claim.then(|| entry.metadata().ok()).flatten() else {
                    continue;
                };
                let stamp = (meta.modified().ok(), meta.len());
                let taken = match self.claims.remove(&path) {
                    Some((was, taken)) if was == stamp => taken,
                    _ => std::fs::read(&path).ok().and_then(|b| claim(&String::from_utf8_lossy(&b))),
                };
                claims.insert(path, (stamp, taken));
            }
            for entry in &entries {
                let path = entry.path();
                let is_md = path
                    .extension()
                    .map_or(false, |e| e.to_string_lossy().eq_ignore_ascii_case("md"));
                if !is_md {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    self.unread += 1;
                    continue;
                };
                if !meta.is_file() || keep.contains_key(&path) {
                    continue;
                }
                let stamp = (meta.modified().ok(), meta.len());
                let mut msg = match self.seen.remove(&path) {
                    Some((was, msg)) if was == stamp => msg,
                    _ => match std::fs::read(&path) {
                        Ok(bytes) => message(&path, &String::from_utf8_lossy(&bytes), stamp.0),
                        Err(_) => {
                            self.unread += 1;
                            continue;
                        }
                    },
                };
                msg.taken = claims.get(&claim_file(&path)).and_then(|(_, taken)| taken.clone());
                found.push(msg.clone());
                keep.insert(path, (stamp, msg));
            }
        }
        self.seen = keep;
        self.claims = claims;
        order(&mut found);
        found
    }

    /// How many message files the last look couldn't read: no change may come to say when they
    /// can be, so they're looked for again soon while there are any
    pub fn unread(&self) -> usize {
        self.unread
    }
}

pub use crate::watch::Seen;
use crate::watch::Target;

/// The folders watched for changes (crate::watch: ReadDirectoryChangesW, through notify), so
/// they're looked in when something in one changes and not at all while nothing does. Each folder
/// that's there is watched itself; one that isn't, from the nearest folder above it that is,
/// which sees it made.
pub struct Watch(crate::watch::Watch);

/// Each of `folders` that's there, and for each that isn't, the nearest folder above it that is,
/// each once
pub fn watch_targets(folders: &[String]) -> Vec<PathBuf> {
    crate::watch::folders(&targets(folders))
}

fn targets(folders: &[String]) -> Vec<Target> {
    folders.iter().map(|f| Target::Folder(PathBuf::from(f))).collect()
}

impl Watch {
    /// A watch telling `seen`, from a thread of its own, of every change in what it watches and
    /// of its failures. It watches nothing until [`Self::follow`].
    pub fn new(seen: impl FnMut(Seen) + Send + 'static) -> notify::Result<Self> {
        Ok(Self(crate::watch::Watch::new(seen)?))
    }

    /// Watches what `folders` needs now ([`watch_targets`]) and nothing else, and says whether
    /// that changed what's watched: then a folder may have come with another under it already,
    /// so it's asked again. A folder that can't be watched is said in the log and tried again at
    /// the next change.
    pub fn follow(&mut self, folders: &[String]) -> bool {
        self.0.follow(&targets(folders))
    }

    /// What's watched now
    pub fn watched(&self) -> &[PathBuf] {
        self.0.watched()
    }

    /// Whether a change at `paths` can change what waits: a file in one of the folders, or a
    /// folder on the way to one (made, removed or renamed). A folder that isn't there yet is
    /// watched from one above it, which sees every other change there too.
    pub fn matters(&self, paths: &[PathBuf]) -> bool {
        self.0.matters(paths)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::NaiveDate;

    const SENT: &str = "---\nfrom: Terminal\nto: LBTerm\nsent: 2026-10-23T14:02\nsubject: \"The rail's \\\"question\\\" light\"\n---\n\nThe body.\n";

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 23).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn the_front_matter_is_read_as_inmail_writes_it() {
        let meta = front_matter(SENT);
        assert_eq!(meta["from"], "Terminal");
        assert_eq!(meta["to"], "LBTerm");
        assert_eq!(meta["sent"], "2026-10-23T14:02");
        assert_eq!(meta["subject"], "The rail's \"question\" light", "a quoted value is a JSON string");
        let single = front_matter("---\r\nsubject: 'Lachlan''s ask'\r\nfrom: Life/Projects\r\n---\r\n");
        assert_eq!(single["subject"], "Lachlan's ask");
        assert_eq!(single["from"], "Life/Projects", "CRLF lines, and a value with a slash");
        let bom = front_matter("\u{feff}---\nfrom: A\n---\n");
        assert_eq!(bom["from"], "A");
    }

    #[test]
    fn no_block_no_front_matter() {
        assert!(front_matter("Just a note.\n").is_empty());
        assert!(front_matter("---\nfrom: A\nnever closed\n").is_empty());
        assert!(front_matter("").is_empty());
        let skips = front_matter("---\nnot a pair\n: no key\nok: yes\n---\n");
        assert_eq!(skips.len(), 1);
        assert_eq!(skips["ok"], "yes");
    }

    #[test]
    fn a_message_takes_what_its_file_says() {
        let file = Path::new(r"C:\Work\Projects\LBTerm\InMail\2026-10-23-1402-from-Terminal-light.md");
        let m = message(file, SENT, None);
        assert_eq!(m.name, "2026-10-23-1402-from-Terminal-light.md");
        assert_eq!(m.folder, Path::new(r"C:\Work\Projects\LBTerm\InMail"));
        assert_eq!(m.dir, Path::new(r"C:\Work\Projects\LBTerm"));
        assert_eq!(m.project, "LBTerm");
        assert_eq!(m.from, "Terminal");
        assert_eq!(m.subject, "The rail's \"question\" light");
        assert_eq!(m.at, Some(at(14, 2)));
    }

    #[test]
    fn a_bare_file_falls_back_on_its_name_folder_and_time() {
        let file = Path::new(r"C:\Work\Projects\Website\InMail\hello there.md");
        let when = SystemTime::now();
        let m = message(file, "Hello.\n", Some(when));
        assert_eq!(m.project, "Website", "the folder above the message's");
        assert_eq!(m.subject, "hello there");
        assert_eq!(m.from, "");
        assert_eq!(m.at, Some(local(when)), "the file's time stands in for sent");
        let empty_to = message(file, "---\nto:\nsubject: Hi\n---\n", None);
        assert_eq!(empty_to.project, "Website", "an empty to counts as none");
        assert_eq!(empty_to.at, None);
    }

    #[test]
    fn sent_reads_with_or_without_seconds() {
        assert_eq!(parse_sent("2026-10-23T14:02"), Some(at(14, 2)));
        assert_eq!(parse_sent(" 2026-10-23T14:02:00 "), Some(at(14, 2)));
        assert_eq!(parse_sent("2026-10-23 14:02"), Some(at(14, 2)));
        assert_eq!(parse_sent("yesterday"), None);
    }

    #[test]
    fn the_badge_says_the_real_count() {
        assert_eq!(badge(0), "");
        assert_eq!(badge(1), "1");
        assert_eq!(badge(99), "99");
        assert_eq!(badge(100), "100", "no cap");
        assert_eq!(badge(1234), "1234");
    }

    #[test]
    fn the_button_lights_with_mail_and_under_the_pointer() {
        let surface = [0.07, 0.07, 0.07];
        let quiet = button_colors(surface, false, false);
        let waiting = button_colors(surface, true, false);
        let lit = button_colors(surface, true, true);
        assert!(waiting.glyph[0] > quiet.glyph[0] && waiting.outline[0] > quiet.outline[0]);
        assert!(lit.glyph[0] > waiting.glyph[0] && lit.outline[0] > waiting.outline[0]);
        assert_eq!(button_colors(surface, false, true), lit, "lit the same with or without mail");
        assert!((lit.glyph[0] - 1.0).abs() < 1e-6, "white under the pointer");
        // 14% of the way from the strip to white, as the restart pill's hairline at rest
        assert!((quiet.outline[0] - (0.07 + 0.93 * 0.14)).abs() < 1e-6);
        assert_eq!(waiting.count, [1.0, 138. / 255., 61. / 255.], "the waiting orange");
    }

    #[test]
    fn a_row_says_when_who_for_what_and_who_from() {
        let file = Path::new(r"C:\Work\Projects\LBTerm\InMail\m.md");
        let mut m = message(file, "---\nfrom: Terminal\nsent: 2026-10-23T14:02\nsubject: \"Two\\nlines\"\n---\n", None);
        m.project = "LBTerm".into();
        let (stamp, project, subject, from, taken) = row(&m, at(18, 0));
        assert_eq!((stamp.as_str(), project.as_str()), ("14:02", "LBTerm"));
        assert_eq!(subject, "Two lines", "on one line");
        assert_eq!(from, "from Terminal");
        assert_eq!(taken, "", "nobody has taken it");
        m.from.clear();
        assert_eq!(row(&m, at(18, 0)).3, "");
        let next_week = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap().and_hms_opt(9, 0, 0).unwrap();
        assert_eq!(row(&m, next_week).0, "23 Oct");
        // Taken: when, as the prompt list's stamps say it
        m.taken = Some(Taken { session: SESSION.into(), at: Some(at(15, 2)) });
        assert_eq!(row(&m, at(18, 0)).4, "taken 15:02");
        m.taken = Some(Taken { session: SESSION.into(), at: None });
        assert_eq!(row(&m, at(18, 0)).4, "taken", "a claim that doesn't say when");
    }

    const SESSION: &str = "2a2332b9-b6e4-4c04-9bb1-ade0f7edbf11";

    #[test]
    fn a_claim_holds_until_its_handed_back_and_names_a_real_session() {
        let taken = claim(&format!(r#"{{ "session": "{SESSION}", "at": "2026-10-23T15:02" }}"#)).unwrap();
        assert_eq!(taken.session, SESSION);
        assert_eq!(taken.at, Some(at(15, 2)));
        assert_eq!(
            claim(&format!("\u{feff}{{\"session\":\"{}\"}}", SESSION.to_uppercase())),
            Some(Taken { session: SESSION.into(), at: None }),
            "a BOM, the id in capitals, no time"
        );
        // Handed back: InMail adds released and why, and it holds nothing
        assert_eq!(
            claim(&format!(r#"{{ "session": "{SESSION}", "at": "2026-10-23T15:02", "released": "2026-10-23T15:40", "why": "not mine" }}"#)),
            None
        );
        assert!(claim(&format!(r#"{{ "session": "{SESSION}", "released": false }}"#)).is_some(), "released: false holds");
        for not_one in [
            "",
            "not json",
            "[]",
            r#"{ "at": "2026-10-23T15:02" }"#,
            r#"{ "session": 12 }"#,
            r#"{ "session": "ff2a9a76" }"#,
            r#"{ "session": "2a2332b9-b6e4-4c04-9bb1-ade0f7edbf1z" }"#,
            r#"{ "session": "2a2332b9-b6e4-4c04-9bb1ade0f7edbf11-" }"#,
            r#"{ "session": "--new-window" }"#,
        ] {
            assert_eq!(claim(not_one), None, "{not_one}");
        }
        assert_eq!(
            claim_file(Path::new(r"C:\P\A\InMail\2026-11-08-1555-from-B-hi.md")),
            PathBuf::from(r"C:\P\A\InMail\2026-11-08-1555-from-B-hi.md.taken")
        );
    }

    #[test]
    fn a_taken_messages_link_is_drawn_taken_until_its_gone() {
        let file = PathBuf::from(r"C:\P\Website\InMail\hi.md");
        let mut m = message(&file, "---\nto: Website\n---\n", None);
        m.taken = Some(Taken { session: SESSION.into(), at: None });
        let folders = vec![r"C:\P\Website\InMail".to_string()];
        assert_eq!(link_state("lbterm://inmail/Website?message=hi.md", &[m], &folders), Some(LinkState::Taken));
        assert_eq!(link_state("lbterm://inmail/Website?message=hi.md", &[], &folders), Some(LinkState::Read), "archived: read");
        assert_eq!(LINK_TAKEN, 0x3DDC84, "the rail's working green");
    }

    #[test]
    fn a_link_to_one_message_names_its_project_and_file() {
        let name = "2026-10-23-2124-from-Web-the-journal.md";
        assert_eq!(
            link_message(&format!("lbterm://inmail/Website?message={name}")),
            Some(("Website".into(), name.into()))
        );
        assert_eq!(
            link_message("lbterm://inmail/Odd%20Jobs/Sub?message=a%20%28b%29.md"),
            Some(("Odd Jobs/Sub".into(), "a (b).md".into())),
            "each part decoded, a repo inside a project kept whole"
        );
        assert_eq!(link_message("lbterm://inmail/Website?message=50%.md").unwrap().1, "50%.md", "a lone %");
        for not_one in [
            "lbterm://inmail/Website",
            "lbterm://inmail/Website?other=a.md",
            "lbterm://inmail/Website?message=",
            "lbterm://inmail/?message=a.md",
            "lbterm://inmail/Website?message=a.md&x=1",
            "lbterm://inmail/Website?message=a.md#top",
            "lbterm://inmail/Website?message=..%5Csecret.md",
            "lbterm://inmail/Website?message=sub/a.md",
            "https://example.com/inmail/Website?message=a.md",
        ] {
            assert_eq!(link_message(not_one), None, "{not_one}");
        }
    }

    #[test]
    fn a_link_waits_while_its_message_does_for_its_project() {
        let file = |dir: &str, name: &str| PathBuf::from(format!(r"C:\P\{dir}\InMail\{name}"));
        let waiting = vec![
            message(&file("Website", "hi.md"), "---\nto: Website\n---\n", None),
            message(&file("Odd Jobs", "dup.md"), "---\nto: Odd Jobs\n---\n", None),
        ];
        let folders: Vec<String> = ["Website", "Odd Jobs"]
            .iter()
            .map(|p| format!(r"C:\P\{p}\InMail"))
            .collect();
        let state = |uri: &str| link_state(uri, &waiting, &folders);
        assert_eq!(state("lbterm://inmail/Website?message=hi.md"), Some(LinkState::Waiting));
        assert_eq!(state("lbterm://inmail/website?message=HI.md"), Some(LinkState::Waiting), "in any case");
        assert_eq!(state("lbterm://inmail/Odd%20Jobs?message=dup.md"), Some(LinkState::Waiting));
        assert_eq!(
            state("lbterm://inmail/Website?message=dup.md"),
            Some(LinkState::Read),
            "the same name waiting for another project"
        );
        assert_eq!(state("lbterm://inmail/Website?message=gone.md"), Some(LinkState::Read));
        assert_eq!(state("lbterm://inmail/Website"), None, "a project's whole inbox");
        assert_eq!(state("https://example.com/"), None);
        // A project whose InMail isn't looked in (one made after the config was read): its
        // message may well wait, so the link is drawn as any link, not as read
        assert_eq!(state("lbterm://inmail/Atlas?message=hi.md"), None);
        assert_eq!(
            link_state("lbterm://inmail/Atlas?message=hi.md", &waiting, &[]),
            None,
            "no folders: nothing known"
        );
        assert_eq!(
            link_state("lbterm://inmail/Website?message=hi.md", &waiting, &[]),
            Some(LinkState::Waiting),
            "a message waiting for it says it waits, whatever the folders"
        );
        assert_eq!(LINK_WAITING, COUNT, "the button's orange");
        // white at 40% over the surface
        assert_eq!(LINK_READ, {
            let c = (0x12 as f64 + (255. - 0x12 as f64) * 0.40).round() as u32;
            c << 16 | c << 8 | c
        });
    }

    #[test]
    fn a_folder_is_a_projects_inmail_by_the_folder_above_it() {
        let folders = vec![
            r"C:\P\Atlas\InMail".to_string(),
            r"C:\P\Terminal\code\LBTerm\InMail".to_string(),
            "C:/P/Bargain Hunter/InMail".to_string(),
        ];
        assert!(looks_in("Atlas", &folders));
        assert!(looks_in("atlas", &folders), "in any case");
        assert!(looks_in("LBTerm", &folders), "a code base by its own name");
        assert!(looks_in("Terminal/code/LBTerm", &folders), "or by its path under the root");
        assert!(looks_in(r"Terminal\code\LBTerm", &folders));
        assert!(looks_in("Bargain Hunter", &folders), "a name with a space, written with forward slashes");
        assert!(!looks_in("Terminal", &folders), "the project a code base is in has an InMail of its own");
        assert!(!looks_in("Other/code/LBTerm", &folders));
        assert!(!looks_in("Career", &folders));
        assert!(!looks_in("", &folders));
        assert!(!looks_in("/", &folders));
    }

    #[test]
    fn oldest_first_then_by_name() {
        let file = |name: &str| PathBuf::from(format!(r"C:\P\A\InMail\{name}"));
        let mk = |name: &str, sent: &str| message(&file(name), &format!("---\nsent: {sent}\n---\n"), None);
        let mut list = vec![
            mk("c.md", "2026-10-23T09:00"),
            mk("b.md", "2026-10-22T09:00"),
            mk("a.md", "2026-10-23T09:00"),
        ];
        order(&mut list);
        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["b.md", "a.md", "c.md"]);
    }

    #[test]
    fn a_scan_finds_the_waiting_files_and_reads_only_what_changed() {
        let root = tempfile::tempdir().unwrap();
        let one = root.path().join("One").join("InMail");
        let two = root.path().join("Two").join("InMail");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let missing = root.path().join("Three").join("InMail");
        let folders: Vec<String> = [&one, &two, &missing]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let mut scan = Scan::default();
        assert!(scan.look(&folders).is_empty(), "empty folders and a missing one: nothing waiting");

        std::fs::write(one.join("2026-10-23-0900-from-Two-hi.md"), "---\nfrom: Two\nsent: 2026-10-23T09:00\nsubject: Hi\n---\n").unwrap();
        std::fs::write(two.join("2026-10-22-0800-from-One-yo.md"), "---\nfrom: One\nsent: 2026-10-22T08:00\nsubject: Yo\n---\n").unwrap();
        std::fs::write(two.join("notes.txt"), "not a message").unwrap();
        std::fs::create_dir_all(two.join("sub.md")).unwrap();
        let found = scan.look(&folders);
        let subjects: Vec<&str> = found.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, ["Yo", "Hi"], "only .md files, oldest first");
        assert_eq!(found[1].project, "One");

        // Kept, not read again: a stamp that matches keeps the message as it was
        let again = scan.look(&folders);
        assert_eq!(again, found);

        // Moved out once it's dealt with
        std::fs::remove_file(one.join("2026-10-23-0900-from-Two-hi.md")).unwrap();
        let left = scan.look(&folders);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].subject, "Yo");

        // Rewritten: read again
        std::fs::write(two.join("2026-10-22-0800-from-One-yo.md"), "---\nfrom: One\nsent: 2026-10-22T08:00\nsubject: Yo again, longer\n---\n").unwrap();
        assert_eq!(scan.look(&folders)[0].subject, "Yo again, longer");

        // The same folder named twice counts its messages once
        let twice = vec![folders[1].clone(), folders[1].clone()];
        assert_eq!(scan.look(&twice).len(), 1);
        assert_eq!(scan.unread(), 0, "every message read");

        // A conversation takes it: the claim beside it, counted as nothing of its own
        let yo = two.join("2026-10-22-0800-from-One-yo.md");
        let claim_path = claim_file(&yo);
        std::fs::write(&claim_path, format!(r#"{{"session":"{SESSION}","at":"2026-10-23T15:02"}}"#)).unwrap();
        let taken = scan.look(&folders);
        assert_eq!(taken.len(), 1, "a claim isn't a message");
        assert_eq!(taken[0].taken.as_ref().map(|t| t.session.as_str()), Some(SESSION));
        // Handed back: InMail rewrites it with released, and it holds nothing
        std::fs::write(&claim_path, format!(r#"{{"session":"{SESSION}","at":"2026-10-23T15:02","released":"2026-10-23T15:40","why":"not mine, longer"}}"#)).unwrap();
        assert_eq!(scan.look(&folders)[0].taken, None);
        // Taken again, then the claim goes with the message's archiving
        std::fs::write(&claim_path, format!(r#"{{"session":"{SESSION}"}}"#)).unwrap();
        assert!(scan.look(&folders)[0].taken.is_some());
        std::fs::remove_file(&claim_path).unwrap();
        assert_eq!(scan.look(&folders)[0].taken, None);
        // A claim beside no message changes nothing
        std::fs::write(two.join("gone.md.taken"), format!(r#"{{"session":"{SESSION}"}}"#)).unwrap();
        assert_eq!(scan.look(&folders).len(), 1);
    }

    #[test]
    fn a_folder_is_watched_itself_and_a_missing_one_from_above() {
        let root = tempfile::tempdir().unwrap();
        let one = root.path().join("One").join("InMail");
        std::fs::create_dir_all(&one).unwrap();
        let two = root.path().join("Two");
        std::fs::create_dir_all(&two).unwrap();
        let folders: Vec<String> = [
            one.clone(),
            two.join("InMail"),
            root.path().join("Three").join("deeper").join("InMail"),
            root.path().join("Four").join("InMail"),
        ]
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
        assert_eq!(
            watch_targets(&folders),
            vec![one.clone(), two.clone(), root.path().to_path_buf()],
            "one's own folder, two's project folder, and the root once for the two whose projects aren't there"
        );
        // Named in another case, it's the same folder
        let shouty = vec![one.to_string_lossy().to_uppercase(), one.to_string_lossy().into_owned()];
        assert_eq!(watch_targets(&shouty).len(), 1);
    }

    #[test]
    fn what_matters_is_a_file_in_a_folder_or_a_folder_on_the_way_to_one() {
        let root = tempfile::tempdir().unwrap();
        let inmail = root.path().join("One").join("InMail");
        let mut watch = Watch::new(|_| {}).unwrap();
        watch.follow(&[inmail.to_string_lossy().into_owned()]);
        assert_eq!(watch.watched(), &[root.path().to_path_buf()], "One isn't there: the root is watched");
        assert!(watch.matters(&[inmail.join("2026-10-30-0900-from-Home-hi.md")]));
        assert!(watch.matters(&[inmail.join("archive")]), "a folder in it: what's archived goes there");
        assert!(watch.matters(&[root.path().join("One")]), "its project's folder made");
        assert!(watch.matters(&[inmail.clone()]), "the folder itself made or gone");
        let upper = PathBuf::from(inmail.to_string_lossy().to_uppercase()).join("A.MD");
        assert!(watch.matters(&[upper]), "in any case");
        assert!(!watch.matters(&[root.path().join("Two")]), "another project");
        assert!(!watch.matters(&[root.path().join("notes.txt")]), "a file beside the project");
        assert!(!watch.matters(&[inmail.join("archive").join("old.md")]), "inside a folder in it");
        assert!(!watch.matters(&[]));
    }

    #[test]
    fn the_watch_sees_a_message_come_even_into_a_folder_made_after_it_started() {
        use std::sync::mpsc::channel;
        use std::time::Duration;
        let root = tempfile::tempdir().unwrap();
        let inmail = root.path().join("One").join("InMail");
        let folders = vec![inmail.to_string_lossy().into_owned()];
        let (tx, rx) = channel();
        let mut watch = Watch::new(move |seen| {
            let _ = tx.send(seen);
        })
        .unwrap();
        // Waits for a change that matters, as the GUI's thread does, following the folders after
        // each: what it's told before then doesn't count
        let changed = |watch: &mut Watch, what: &str| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(left) {
                    Ok(Seen::Changed(paths)) if watch.matters(&paths) => break,
                    Ok(Seen::Changed(_)) => continue,
                    Ok(Seen::Failed(err)) => panic!("the watch failed while waiting for {what}: {err}"),
                    Err(_) => panic!("no change seen in 10 s: {what}"),
                }
            }
            while watch.follow(&folders) {}
        };
        assert!(watch.follow(&folders), "the first follow watches something");
        assert!(!watch.follow(&folders), "and the next, nothing new");

        std::fs::create_dir_all(&inmail).unwrap();
        changed(&mut watch, "One and its InMail made");
        assert_eq!(watch.watched(), &[inmail.clone()], "the folder watched itself once it's there");
        // Anything that came before its own watch is read by the look that follows; from now on
        // its own watch says
        std::thread::sleep(Duration::from_millis(100));
        while rx.try_recv().is_ok() {}
        let message = inmail.join("2026-10-30-0900-from-Home-hi.md");
        std::fs::write(&message, "---
from: Home
subject: Hi
---
").unwrap();
        changed(&mut watch, "a message written");
        let mut scan = Scan::default();
        assert_eq!(scan.look(&folders).len(), 1);
        std::fs::remove_file(&message).unwrap();
        changed(&mut watch, "the message archived");
        assert!(scan.look(&folders).is_empty());
    }
}
