//! What to call a folder: the projects' one name for it, the name every other tool on the PC
//! gives it (the statusline among them), so the rail never works one out of its own (Lachlan,
//! 2026-11-02: "the name only comes from 1 place"; this crate's `text::project` worked it out
//! from the path until then, so the root and a repo inside a project had names of their own).
//! It reads `names.json`, which the projects' `links sync` writes into the machine state
//! (`%LOCALAPPDATA%\Megaproject\names.json`) for code that can't ask the projects' own code:
//!
//! - a folder is the project whose folder holds it, the deepest one (`Seeds` for
//!   `Garden\code\Seeds\src`), and the root's for anything else under the root;
//! - in `<project>\.claude\worktrees\<name>`, `<project> · <name>`;
//! - under a rename's new name (a junction to the old folder until the swap), the project's
//!   under its old one;
//! - the user folder is `~`, and any other folder its own name.
//!
//! The rail asks for every row on every draw, on the GUI thread, so the file is looked at no
//! more than every 2 s and each folder's name is kept until the file changes. When `names.json`
//! can't be read the rail says so (`names.json?`) rather than guess a name.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// What the rail shows for a tab's project while `names.json` can't be read
pub const UNREAD: &str = "names.json?";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    /// the megaproject's root
    pub root: String,
    /// (name, folder), as names.json lists them, the root first
    pub projects: Vec<(String, String)>,
    /// (old, now): folders under the root while a rename is under way
    pub renaming: Vec<(String, String)>,
    /// each project's folder, trimmed and lowercased, in `projects`' order
    keys: Vec<String>,
}

fn trim(p: &str) -> &str {
    p.trim_end_matches(['\\', '/'])
}

/// `key` is `dir` or a path inside it (both lowercased, with no trailing separator)
fn is_under(key: &str, dir: &str) -> bool {
    key == dir || (key.len() > dir.len() && key.starts_with(dir) && key.as_bytes()[dir.len()] == b'\\')
}

impl Names {
    /// names.json's text, or None when it isn't the shape links sync writes
    pub fn parse(text: &str) -> Option<Names> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let root = v.get("root")?.as_str()?.to_string();
        let projects: Vec<(String, String)> = v
            .get("projects")?
            .as_array()?
            .iter()
            .filter_map(|p| Some((p.get("name")?.as_str()?.to_string(), p.get("dir")?.as_str()?.to_string())))
            .collect();
        let renaming = match v.get("renaming") {
            None => Vec::new(),
            Some(r) => r
                .as_array()?
                .iter()
                .filter_map(|r| Some((r.get("old")?.as_str()?.to_string(), r.get("now")?.as_str()?.to_string())))
                .collect(),
        };
        let keys = projects.iter().map(|(_, d)| trim(d).to_ascii_lowercase()).collect();
        Some(Names { root, projects, renaming, keys })
    }

    /// A path under a rename's new folder as the same path under the old one; any other path as
    /// it is
    fn through_rename(&self, p: &str) -> String {
        let root = trim(&self.root);
        for (old, now) in &self.renaming {
            let now_dir = format!("{root}\\{}", now.replace('/', "\\"));
            if is_under(&p.to_ascii_lowercase(), &now_dir.to_ascii_lowercase()) {
                return format!("{root}\\{}{}", old.replace('/', "\\"), &p[now_dir.len()..]);
            }
        }
        p.to_string()
    }

    /// What to call `path` (a Windows path, as `text::cwd_path` makes one), `home` being the
    /// user folder
    pub fn name_of(&self, path: &str, home: &str) -> String {
        let p = trim(path);
        if p.is_empty() {
            return String::new();
        }
        let p = self.through_rename(p);
        let key = p.to_ascii_lowercase();
        let mut best: Option<usize> = None;
        for (i, d) in self.keys.iter().enumerate() {
            if is_under(&key, d) && best.map_or(true, |b| d.len() > self.keys[b].len()) {
                best = Some(i);
            }
        }
        if let Some(i) = best {
            let name = &self.projects[i].0;
            let rest: Vec<&str> = p[self.keys[i].len()..].split('\\').filter(|s| !s.is_empty()).collect();
            if rest.len() >= 3 && rest[0].eq_ignore_ascii_case(".claude") && rest[1].eq_ignore_ascii_case("worktrees") {
                return format!("{name} · {}", rest[2]);
            }
            return name.clone();
        }
        if !home.is_empty() && key == trim(home).to_ascii_lowercase() {
            return "~".to_string();
        }
        p.rsplit('\\').next().filter(|s| !s.is_empty()).unwrap_or(&p).to_string()
    }
}

/// names.json in the machine state
pub fn names_file() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Megaproject").join("names.json"))
}

#[cfg_attr(test, allow(dead_code))]
struct Cache {
    looked: Instant,
    stamp: Option<SystemTime>,
    names: Option<Names>,
    /// each folder's name, worked out once for the file as it is
    named: HashMap<String, String>,
}

#[cfg_attr(test, allow(dead_code))]
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
/// How often the file is looked at again: links sync rewrites it rarely, and the rail asks for
/// every row on every draw
#[cfg_attr(test, allow(dead_code))]
const LOOK_EVERY: Duration = Duration::from_secs(2);

#[cfg_attr(test, allow(dead_code))]
fn stamp_of(f: &Option<PathBuf>) -> Option<SystemTime> {
    f.as_ref().and_then(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
}

/// A made-up megaproject's names for this crate's tests, which never read the machine state:
/// `made_up()` puts one in place (the root `C:\Users\me\AI`, with Terminal and Garden's Seeds),
/// and `project()` does on its first call too, so a test gets the same names whichever runs
/// first. Until 2026-11-05 only the tests that called `made_up()` put them there, and
/// `a_link_hides_no_change_from_the_strip_and_the_projects_card`, which doesn't, got
/// "names.json?" when it ran before them (PITFALLS 146)
#[cfg(test)]
static MADE_UP: Mutex<Option<Names>> = Mutex::new(None);
#[cfg(test)]
const MADE_UP_NAMES: &str = r#"{ "root": "C:\\Users\\me\\AI", "projects": [
      { "name": "Root", "dir": "C:\\Users\\me\\AI" }, { "name": "Terminal", "dir": "C:\\Users\\me\\AI\\Terminal" },
      { "name": "Garden", "dir": "C:\\Users\\me\\AI\\Garden" }, { "name": "Seeds", "dir": "C:\\Users\\me\\AI\\Garden\\code\\Seeds" } ] }"#;
#[cfg(test)]
pub(crate) fn made_up() {
    *MADE_UP.lock().unwrap_or_else(|e| e.into_inner()) = Names::parse(MADE_UP_NAMES);
}

/// The project a tab's folder is in, as the rail shows it (a Windows path, or "" for none)
pub fn project(path: &str) -> String {
    if trim(path).is_empty() {
        return String::new();
    }
    #[cfg(test)]
    {
        let mut made = MADE_UP.lock().unwrap_or_else(|e| e.into_inner());
        if made.is_none() {
            *made = Names::parse(MADE_UP_NAMES);
        }
        match made.as_ref() {
            Some(n) => n.name_of(path, r"C:\Users\me"),
            None => UNREAD.to_string(),
        }
    }
    #[cfg(not(test))]
    {
        from_file(path)
    }
}

/// `project()` from the machine state's names.json, looked at no more than every LOOK_EVERY
#[cfg_attr(test, allow(dead_code))]
fn from_file(path: &str) -> String {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if guard.as_ref().map_or(true, |c| now.duration_since(c.looked) >= LOOK_EVERY) {
        let file = names_file();
        let stamp = stamp_of(&file);
        let same = guard.as_ref().map_or(false, |c| c.stamp == stamp && c.names.is_some());
        if same {
            guard.as_mut().unwrap().looked = now;
        } else {
            let names = file.and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| Names::parse(&t));
            *guard = Some(Cache { looked: now, stamp, names, named: HashMap::new() });
        }
    }
    let c = guard.as_mut().unwrap();
    let Some(names) = c.names.as_ref() else { return UNREAD.to_string() };
    if let Some(n) = c.named.get(path) {
        return n.clone();
    }
    let n = names.name_of(path, &std::env::var("USERPROFILE").unwrap_or_default());
    c.named.insert(path.to_string(), n.clone());
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
      "_": "made up",
      "root": "C:\\Users\\me\\AI",
      "bun": "C:\\Users\\me\\AI\\.typescript\\node_modules\\@oven\\bun-windows-x64\\bin\\bun.exe",
      "projects": [
        { "name": "Root", "dir": "C:\\Users\\me\\AI" },
        { "name": "Garden", "dir": "C:\\Users\\me\\AI\\Garden" },
        { "name": "Seeds", "dir": "C:\\Users\\me\\AI\\Garden\\code\\Seeds" },
        { "name": "Kitchen", "dir": "C:\\Users\\me\\AI\\Kitchen" },
        { "name": "Terminal", "dir": "C:\\Users\\me\\AI\\Terminal" }
      ],
      "renaming": [{ "old": "Kitchen", "now": "Pantry" }],
      "commands": {},
      "published": {}
    }"#;

    #[test]
    fn a_folder_is_named_as_blueprints_name_of_names_it() {
        let n = Names::parse(JSON).unwrap();
        let home = r"C:\Users\me";
        assert_eq!(n.name_of(r"C:\Users\me\AI\Terminal\shared\claude", home), "Terminal");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Garden\code\Seeds\src", home), "Seeds", "the deepest project");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Garden\notes", home), "Garden");
        assert_eq!(n.name_of(r"C:\Users\me\AI", home), "Root", "the root by its name, never AI");
        assert_eq!(n.name_of(r"C:\Users\me\AI\", home), "Root");
        assert_eq!(n.name_of(r"c:\users\me\ai\terminal", home), "Terminal", "Windows paths in any case");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Terminal\.claude\worktrees\fix-x", home), "Terminal · fix-x");
        assert_eq!(n.name_of(r"C:\Users\me\AI\.claude\worktrees\names\sub", home), "Root · names", "the root's worktree");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Garden\code\Seeds\.claude\worktrees\ui", home), "Seeds · ui");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Pantry\Shelf", home), "Kitchen", "under a rename's new name, the project under its old one");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Pantrys", home), "Root", "a folder that only starts with the new name");
        assert_eq!(n.name_of(r"C:\Users\me\AI\Terminals", home), "Root", "a folder that only starts with a project's");
        assert_eq!(n.name_of(r"C:\Users\me", home), "~");
        assert_eq!(n.name_of(r"C:\Users\me\Code\LBTerm", home), "LBTerm", "a folder in no project by its own name");
        assert_eq!(n.name_of(r"D:\AI", home), "AI");
        assert_eq!(n.name_of("", home), "");
    }

    #[test]
    fn the_rail_asks_through_project_and_a_file_url_is_a_path() {
        made_up();
        assert_eq!(project(r"C:\Users\me\AI\Garden\code\Seeds"), "Seeds");
        assert_eq!(crate::text::project("file:///C:/Users/me/AI/Terminal/.claude/worktrees/slot-2/"), "Terminal · slot-2");
        assert_eq!(project(""), "");
    }

    #[test]
    fn names_json_without_renames_or_in_another_shape() {
        let n = Names::parse(r#"{ "root": "C:\\AI", "projects": [{ "name": "Root", "dir": "C:\\AI" }] }"#).unwrap();
        assert!(n.renaming.is_empty());
        assert_eq!(n.name_of(r"C:\AI\x", ""), "Root");
        assert_eq!(Names::parse("not json"), None);
        assert_eq!(Names::parse(r#"{ "projects": [] }"#), None, "no root");
        assert_eq!(Names::parse(r#"{ "root": "C:\\AI", "projects": [], "renaming": 3 }"#), None);
    }
}
