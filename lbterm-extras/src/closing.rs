//! Close marks: when Lachlan closes a tab or pane himself (Ctrl+W, Ctrl+Shift+W once its question
//! is answered yes, the tab bar's close button, the tab menu's Close tab and Close other tabs),
//! LBTerm leaves `<session id>.closing` in `lbterm_close_marks` for each Claude Code session in it
//! (its `claude_session` user var), just before the close ends Claude. Closing a tab ends Claude
//! mid-run, so Claude Code's SessionEnd says `other`, as it does for a window closing or LBTerm
//! going down. The Claude project's live-sessions hook keeps those as "tab closed" with a resume,
//! and forgets one with a fresh mark like /exit (its InMail, 2026-10-24; Lachlan's pick). The
//! file's write time is the mark; it holds the epoch seconds for a human reading it.
//!
//! What closes on its own writes none: a window closing, LBTerm quitting, the mux server going
//! down, and a last tab whose program ended by itself (last_tab.rs' keeper).

use crate::restore::is_session_id;
use std::path::{Path, PathBuf};

/// The mark files for `sessions` in `dir`: one per real session id, each once, in order
pub fn mark_paths<'a>(dir: &Path, sessions: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = vec![];
    for id in sessions {
        if !is_session_id(id) {
            continue;
        }
        let path = dir.join(format!("{id}.closing"));
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// Writes the marks for `sessions` in `dir` (made if it's missing), each holding `now_secs`: the
/// files written, and the errors of those that couldn't be
pub fn write_marks<'a>(
    dir: &Path,
    sessions: impl IntoIterator<Item = &'a str>,
    now_secs: u64,
) -> (Vec<PathBuf>, Vec<String>) {
    let paths = mark_paths(dir, sessions);
    let mut written = vec![];
    let mut errors = vec![];
    if paths.is_empty() {
        return (written, errors);
    }
    if let Err(err) = std::fs::create_dir_all(dir) {
        errors.push(format!("{}: {err}", dir.display()));
        return (written, errors);
    }
    for path in paths {
        match std::fs::write(&path, now_secs.to_string()) {
            Ok(()) => written.push(path),
            Err(err) => errors.push(format!("{}: {err}", path.display())),
        }
    }
    (written, errors)
}

#[cfg(test)]
mod test {
    use super::*;

    const A: &str = "f4daa898-d621-4a23-9830-c05090154adb";
    const B: &str = "03bd0996-164e-4def-a7c3-3e3439a41482";

    #[test]
    fn a_mark_for_each_real_session_once() {
        let dir = Path::new(r"C:\x\live-sessions");
        let paths = mark_paths(dir, [A, "", "not-a-session", B, A, "f4daa898"]);
        assert_eq!(
            paths,
            vec![dir.join(format!("{A}.closing")), dir.join(format!("{B}.closing"))]
        );
    }

    #[test]
    fn writes_them_with_the_time_in_a_folder_it_makes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Megaproject").join("live-sessions");
        let (written, errors) = write_marks(&dir, [A, B], 1_792_570_000);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(written.len(), 2);
        assert_eq!(std::fs::read_to_string(dir.join(format!("{A}.closing"))).unwrap(), "1792570000");
        assert!(dir.join(format!("{B}.closing")).exists());
    }

    #[test]
    fn writes_nothing_without_a_session() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("none");
        let (written, errors) = write_marks(&dir, ["", "x"], 1);
        assert!(written.is_empty() && errors.is_empty());
        assert!(!dir.exists(), "no folder made for nothing");
    }
}
