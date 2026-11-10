//! What a Claude Code conversation has in its context from files, for the pill under the InMail
//! button (lbterm/CONTEXT.md). Lachlan's ask, 2026-11-07: "LBTerm should be able to show me what
//! context files this agent has loaded currently."
//!
//! The files are the instruction files Claude Code loads: every CLAUDE.md from the conversation's
//! folder up, the user's own, their `@` imports, rules, and the auto memory's MEMORY.md. Its
//! transcript says which (read by [`crate::conversation`], on the scroll strip's thread):
//!
//! - an `instructions` attachment when a session starts, and again after each compaction, listing
//!   every file it put in the context, each with its type (`User`, `Project`, `Local`, `Managed`,
//!   `AutoMem`) and its whole content;
//! - a `nested_memory` attachment for each CLAUDE.md a file read in another folder pulled in
//!   later, the file's content under `content.content`;
//! - a `compact_boundary` system record for a compaction, which drops everything before it.
//!
//! So what's loaded now is every file recorded since the last compaction, by its path, the last
//! record of a path giving its size. A resume writes another `instructions` record, and what was
//! pulled in before it is still in the conversation's history, so it stays.
//!
//! A file's tokens are its characters (UTF-16 units, as Claude Code counts a string) over four,
//! rounded: Claude Code's own estimate of a memory file's size.
//!
//! A file pulled in from a folder that isn't the conversation's own or one above it is
//! [`File::elsewhere`] (Lachlan's E4, 2026-11-07): another project's CLAUDE.md, loaded whole
//! because a file of that project was read. The conversation's own folder is the first `cwd` its
//! transcript records, where it started, which a `cd` or a resume doesn't change.

use serde_json::Value;

/// How Claude Code came to load a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The user's own: `~/.claude/CLAUDE.md`, and what it imports
    User,
    /// A project's: a CLAUDE.md from the conversation's folder up, and what it imports
    Project,
    /// A `CLAUDE.local.md`
    Local,
    /// One an administrator put in place for every user
    Managed,
    /// The auto memory's index, `MEMORY.md`
    Memory,
    /// A CLAUDE.md a file read in its folder pulled in after the start
    Later,
}

impl Kind {
    /// The type an `instructions` record gives a file; none for one it doesn't know
    fn from_type(t: &str) -> Option<Kind> {
        Some(match t {
            "User" => Kind::User,
            "Project" => Kind::Project,
            "Local" => Kind::Local,
            "Managed" => Kind::Managed,
            "AutoMem" => Kind::Memory,
            _ => return None,
        })
    }

    /// The list's word for it
    pub fn word(self) -> &'static str {
        match self {
            Kind::User => "user",
            Kind::Project => "project",
            Kind::Local => "local",
            Kind::Managed => "managed",
            Kind::Memory => "memory",
            Kind::Later => "pulled in",
        }
    }
}

/// One file in the context
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// As Claude Code wrote it
    pub path: String,
    pub kind: Kind,
    /// Claude Code's estimate of its size: its characters over four
    pub tokens: usize,
    /// Pulled in from a folder that isn't the conversation's own or one above it
    pub elsewhere: bool,
}

/// The files in a conversation's context now, in the order they were loaded
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    pub files: Vec<File>,
    /// The folder the conversation started in: the first `cwd` its transcript records
    pub own: Option<String>,
}

/// Claude Code's estimate of a text's tokens: its UTF-16 units over four, rounded
pub fn tokens(text: &str) -> usize {
    (text.encode_utf16().count() + 2) / 4
}

/// A path's parts, for comparing folders as Windows does: either slash, any case, no empty parts
fn parts(path: &str) -> Vec<String> {
    path.split(['\\', '/']).filter(|p| !p.is_empty()).map(str::to_lowercase).collect()
}

/// The folder a loaded file stands for: the one it's in, or the one above its `.claude` folder
/// for a file in one (`<folder>\.claude\CLAUDE.md`, a rule in `<folder>\.claude\rules\`)
fn folder_of(path: &str) -> Vec<String> {
    let mut p = parts(path);
    p.pop();
    if let Some(i) = p.iter().position(|part| part == ".claude") {
        p.truncate(i);
    }
    p
}

/// Whether a file loaded from `path` comes from a folder that isn't `own` or one above it
pub fn elsewhere(own: &str, path: &str) -> bool {
    let (own, folder) = (parts(own), folder_of(path));
    !(folder.len() <= own.len() && own[..folder.len()] == folder[..])
}

impl Context {
    /// A record's `cwd`: the first one is the conversation's own folder
    pub fn saw_folder(&mut self, cwd: &str) -> bool {
        if self.own.is_some() || cwd.is_empty() {
            return false;
        }
        self.own = Some(cwd.to_string());
        true
    }

    /// A compaction: what was loaded before it has gone. Whether anything went
    pub fn compacted(&mut self) -> bool {
        let had = !self.files.is_empty();
        self.files.clear();
        had
    }

    /// File `file` loaded, in its place if it's there already. Whether that changed anything
    fn add(&mut self, file: File) -> bool {
        let key = parts(&file.path);
        match self.files.iter_mut().find(|f| parts(&f.path) == key) {
            Some(was) if *was == file => false,
            Some(was) => {
                *was = file;
                true
            }
            None => {
                self.files.push(file);
                true
            }
        }
    }

    /// An `instructions` attachment: every file the session put in its context as it started, or
    /// after a compaction. Whether anything changed
    pub fn instructions(&mut self, attachment: &Value) -> bool {
        let Some(files) = attachment.get("files").and_then(Value::as_array) else {
            return false;
        };
        let mut changed = false;
        for f in files {
            let (Some(path), Some(kind)) = (
                f.get("path").and_then(Value::as_str),
                f.get("type").and_then(Value::as_str).and_then(Kind::from_type),
            ) else {
                continue;
            };
            let content = f.get("content").and_then(Value::as_str).unwrap_or("");
            changed |= self.add(File {
                path: path.to_string(),
                kind,
                tokens: tokens(content),
                elsewhere: false,
            });
        }
        changed
    }

    /// A `nested_memory` attachment: a CLAUDE.md a file read pulled in. Whether anything changed
    pub fn nested(&mut self, attachment: &Value) -> bool {
        let Some(path) = attachment.get("path").and_then(Value::as_str) else {
            return false;
        };
        let content = attachment
            .get("content")
            .and_then(|c| c.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let elsewhere = self.own.as_deref().map_or(false, |own| elsewhere(own, path));
        self.add(File {
            path: path.to_string(),
            kind: Kind::Later,
            tokens: tokens(content),
            elsewhere,
        })
    }

    /// Every file's tokens together
    pub fn tokens(&self) -> usize {
        self.files.iter().map(|f| f.tokens).sum()
    }
}

/// Tokens in a few characters, as the statusline writes them: `912`, `6.1k`, `31k`, `1.2M`
pub fn count(n: usize) -> String {
    let n = n as f64;
    let short = |v: f64, unit: &str| {
        let s = if v < 10.0 { format!("{v:.1}") } else { format!("{v:.0}") };
        format!("{}{unit}", s.trim_end_matches(".0"))
    };
    if n < 1000.0 {
        format!("{n:.0}")
    } else if n < 999_500.0 {
        short(n / 1000.0, "k")
    } else {
        short(n / 1_000_000.0, "M")
    }
}

/// The list's heading (Lachlan's E5, 2026-11-07): how many files, the tokens they take together,
/// and their share of the context window when it's known
pub fn heading(context: &Context, window: Option<u64>) -> String {
    let n = context.files.len();
    if n == 0 {
        return NONE.to_string();
    }
    let tokens = context.tokens();
    let mut words = format!("{n} context file{} · ≈{} tokens", if n == 1 { "" } else { "s" }, count(tokens));
    if let Some(window) = window.filter(|w| *w > 0) {
        let share = tokens as f64 * 100.0 / window as f64;
        let share = if share < 10.0 { format!("{share:.1}") } else { format!("{share:.0}") };
        words.push_str(&format!(" · {share}% of the {} window", count(window as usize)));
    }
    words
}

/// What the list says before the transcript has said what's loaded
pub const NONE: &str = "No context files read yet";

/// A path as the list shows it: the home folder as `~`
pub fn short_path(path: &str, home: &str) -> String {
    let (p, h) = (parts(path), parts(home));
    if !h.is_empty() && p.len() > h.len() && p[..h.len()] == h[..] {
        let rest: Vec<&str> = path.split(['\\', '/']).filter(|s| !s.is_empty()).skip(h.len()).collect();
        format!("~\\{}", rest.join("\\"))
    } else {
        path.to_string()
    }
}

/// A path cut to `cols` columns from its start, keeping its end: the file's name and the folders
/// nearest it say the most
pub fn fit_path(path: &str, cols: usize) -> String {
    let n = path.chars().count();
    if n <= cols {
        return path.to_string();
    }
    if cols == 0 {
        return String::new();
    }
    let keep = cols - 1;
    let tail: String = path.chars().skip(n - keep).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    fn instructions() -> Value {
        json!({ "type": "instructions", "files": [
            { "path": "C:\\Users\\me\\.claude\\CLAUDE.md", "type": "User", "content": "x".repeat(4000) },
            { "path": "C:\\Users\\me\\Work\\rules\\asking.md", "type": "User", "content": "y".repeat(401) },
            { "path": "C:\\Users\\me\\Work\\CLAUDE.md", "type": "Project", "content": "z".repeat(8000) },
            { "path": "C:\\Users\\me\\Work\\Harbor\\CLAUDE.md", "type": "Project", "content": "h".repeat(2000) },
            { "path": "C:\\Users\\me\\.claude\\projects\\C--Users-me-Work-Harbor\\memory\\MEMORY.md", "type": "AutoMem", "content": "m".repeat(800) },
            { "path": "C:\\odd.md", "type": "Something new", "content": "?" },
        ]})
    }

    fn nested(path: &str, chars: usize) -> Value {
        json!({ "type": "nested_memory", "path": path, "displayPath": "CLAUDE.md",
                "content": { "path": path, "type": "Project", "content": "n".repeat(chars) } })
    }

    #[test]
    fn a_files_tokens_are_its_characters_over_four_as_claude_code_counts_them() {
        assert_eq!(tokens(""), 0);
        assert_eq!(tokens("abcd"), 1);
        assert_eq!(tokens(&"x".repeat(4001)), 1000);
        assert_eq!(tokens(&"x".repeat(4002)), 1001, "rounded half up, as Math.round");
        // A character outside the BMP is two UTF-16 units to JavaScript
        assert_eq!(tokens("😀😀"), 1);
    }

    #[test]
    fn the_start_loads_every_file_the_instructions_record_lists_with_its_kind_and_size() {
        let mut c = Context::default();
        assert!(c.saw_folder("C:\\Users\\me\\Work\\Harbor"));
        assert!(!c.saw_folder("C:\\Users\\me\\Elsewhere"), "the first folder is the conversation's own");
        assert!(c.instructions(&instructions()));
        let kinds: Vec<(Kind, usize)> = c.files.iter().map(|f| (f.kind, f.tokens)).collect();
        assert_eq!(
            kinds,
            vec![(Kind::User, 1000), (Kind::User, 100), (Kind::Project, 2000), (Kind::Project, 500), (Kind::Memory, 200)],
            "a type it doesn't know is left out"
        );
        assert_eq!(c.tokens(), 3800);
        assert!(c.files.iter().all(|f| !f.elsewhere));
        assert!(!c.instructions(&instructions()), "the same files again change nothing");
    }

    #[test]
    fn a_claude_md_pulled_in_from_another_project_is_elsewhere_and_one_in_its_own_folder_is_not() {
        let mut c = Context::default();
        c.saw_folder("C:\\Users\\me\\Work\\Harbor");
        c.instructions(&instructions());
        assert!(c.nested(&nested("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", 20000)));
        assert!(c.nested(&nested("C:/users/ME/work/harbor/.claude/CLAUDE.md", 40)));
        let later: Vec<(&str, bool, usize)> = c
            .files
            .iter()
            .filter(|f| f.kind == Kind::Later)
            .map(|f| (f.path.as_str(), f.elsewhere, f.tokens))
            .collect();
        assert_eq!(
            later,
            vec![("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", true, 5000), ("C:/users/ME/work/harbor/.claude/CLAUDE.md", false, 10)]
        );
        // Below its own folder is another folder's: a code base inside the project has its own
        assert!(elsewhere("C:\\Users\\me\\Work", "C:\\Users\\me\\Work\\Harbor\\code\\site\\CLAUDE.md"));
        assert!(!elsewhere("C:\\Users\\me\\Work\\Harbor\\code", "C:\\Users\\me\\Work\\CLAUDE.md"), "one above it");
        assert!(!elsewhere("C:\\Users\\me\\Work", "C:\\Users\\me\\Work\\.claude\\rules\\style.md"), "its own rules");
    }

    #[test]
    fn a_compaction_drops_what_was_loaded_and_the_record_after_it_loads_the_start_again() {
        let mut c = Context::default();
        c.saw_folder("C:\\Users\\me\\Work\\Harbor");
        c.instructions(&instructions());
        c.nested(&nested("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", 20000));
        assert_eq!(c.files.len(), 6);
        assert!(c.compacted());
        assert!(c.files.is_empty());
        assert!(!c.compacted(), "nothing more to drop");
        c.instructions(&instructions());
        assert_eq!(c.files.len(), 5, "the pulled-in one isn't back until a file reads it in again");
        assert_eq!(c.own.as_deref(), Some("C:\\Users\\me\\Work\\Harbor"), "still the conversation's own folder");
    }

    #[test]
    fn a_resume_keeps_what_was_pulled_in_and_takes_each_files_new_size() {
        let mut c = Context::default();
        c.saw_folder("C:\\Users\\me\\Work\\Harbor");
        c.instructions(&instructions());
        c.nested(&nested("C:\\Users\\me\\Work\\Ledger\\CLAUDE.md", 20000));
        let mut again = instructions();
        again["files"][2]["content"] = json!("z".repeat(9000));
        assert!(c.instructions(&again));
        assert_eq!(c.files.len(), 6, "the pulled-in file is still in the history");
        assert_eq!(c.files[2].tokens, 2250, "in its place, at its new size");
    }

    #[test]
    fn the_heading_counts_the_files_and_their_tokens_and_says_their_share_of_the_window() {
        let mut c = Context::default();
        assert_eq!(heading(&c, Some(1_000_000)), NONE);
        c.instructions(&instructions());
        assert_eq!(heading(&c, None), "5 context files · ≈3.8k tokens");
        assert_eq!(heading(&c, Some(1_000_000)), "5 context files · ≈3.8k tokens · 0.4% of the 1M window");
        assert_eq!(heading(&c, Some(20_000)), "5 context files · ≈3.8k tokens · 19% of the 20k window");
        c.compacted();
        c.nested(&nested("C:\\a\\CLAUDE.md", 4));
        assert_eq!(heading(&c, None), "1 context file · ≈1 tokens");
    }

    #[test]
    fn counts_are_short() {
        assert_eq!(count(912), "912");
        assert_eq!(count(6120), "6.1k");
        assert_eq!(count(31_234), "31k");
        assert_eq!(count(1_000_000), "1M");
        assert_eq!(count(200_000), "200k");
        assert_eq!(count(1_260_000), "1.3M");
    }

    #[test]
    fn paths_show_home_as_a_tilde_and_keep_their_end_when_cut() {
        assert_eq!(short_path("C:\\Users\\me\\Work\\CLAUDE.md", "C:\\Users\\me"), "~\\Work\\CLAUDE.md");
        assert_eq!(short_path("C:/users/ME/Work/CLAUDE.md", "C:\\Users\\me"), "~\\Work\\CLAUDE.md");
        assert_eq!(short_path("D:\\Other\\CLAUDE.md", "C:\\Users\\me"), "D:\\Other\\CLAUDE.md");
        assert_eq!(short_path("C:\\Users\\meant\\CLAUDE.md", "C:\\Users\\me"), "C:\\Users\\meant\\CLAUDE.md", "a folder that only starts like it");
        assert_eq!(fit_path("~\\Work\\Harbor\\CLAUDE.md", 40), "~\\Work\\Harbor\\CLAUDE.md");
        assert_eq!(fit_path("~\\Work\\Harbor\\CLAUDE.md", 12), "…r\\CLAUDE.md");
        assert_eq!(fit_path("abc", 0), "");
    }
}
