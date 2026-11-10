//! Letting go of a folder on request (lbterm/LET-GO.md). Windows won't rename or remove a folder
//! while a handle inside it is open, and every watch LBTerm keeps (crate::watch) holds one in the
//! folder it watches and one in that folder's parent, so while LBTerm ran, a folder with a watched
//! folder in it (every project of Lachlan's, for its InMail) couldn't be renamed. A program about
//! to rename or remove a folder writes a request into a folder of requests LBTerm watches
//! (`config.lbterm_let_go`), one JSON file each:
//!
//! ```text
//! { "folder": "C:\\Work\\Projects\\Resume", "until": 1793612345678, "by": "rename" }
//! ```
//!
//! While the file is there and `until` (unix milliseconds) hasn't come, every watch in the GUI
//! lets go of that folder ([`crate::watch::let_go`]); once the file goes, or its time does, they
//! take it back. LBTerm writes nothing back: the program tries its rename again until it goes
//! through, since every watch lets go on its own thread a moment after it's told.

use std::path::{Path, PathBuf};

/// One request: a folder to let go of, until when, and who asked
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Its file, in the folder of requests
    pub file: PathBuf,
    /// The folder to let go of
    pub folder: PathBuf,
    /// Until when, in unix milliseconds
    pub until: i64,
    /// Who asked: for the log
    pub by: String,
}

/// The request a file holds, read from its text: an object with a `folder` (a path, not empty)
/// and an `until` (whole unix milliseconds), and a `by` if it says. Nothing for anything else
pub fn request(file: &Path, text: &str) -> Option<Request> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let folder = value.get("folder")?.as_str()?.trim();
    let until = value.get("until")?.as_i64()?;
    if folder.is_empty() {
        return None;
    }
    let by = value.get("by").and_then(|b| b.as_str()).unwrap_or_default().to_string();
    Some(Request {
        file: file.to_path_buf(),
        folder: PathBuf::from(folder),
        until,
        by,
    })
}

/// What the folder of requests asks now
#[derive(Debug, Default, PartialEq)]
pub struct Asked {
    /// The requests whose time hasn't come, by file name
    pub requests: Vec<Request>,
    /// The `.json` files that couldn't be read or aren't a request: said in the log, and left
    pub refused: Vec<(PathBuf, String)>,
    /// When the soonest of them runs out, to look again then
    pub next: Option<i64>,
}

impl Asked {
    /// The folders to let go of, each once, in the order their requests were found
    pub fn folders(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = vec![];
        for r in &self.requests {
            if !folders.iter().any(|f| crate::watch::same_path(f, &r.folder)) {
                folders.push(r.folder.clone());
            }
        }
        folders
    }
}

/// Every request in `dir` whose time hasn't come at `now` (unix milliseconds), by file name. A
/// folder that isn't there asks nothing; only `.json` files are read (the writer's temporary file
/// before its rename isn't one).
pub fn asked(dir: &Path, now: i64) -> Asked {
    let mut asked = Asked::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return asked;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) && p.is_file())
        .collect();
    files.sort();
    for file in files {
        let text = match std::fs::read(&file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(err) => {
                asked.refused.push((file, format!("can't be read: {err}")));
                continue;
            }
        };
        match request(&file, &text) {
            Some(r) if r.until > now => {
                asked.next = Some(asked.next.map_or(r.until, |n| n.min(r.until)));
                asked.requests.push(r);
            }
            Some(_) => {}
            None => asked.refused.push((file, "isn't { folder, until }".into())),
        }
    }
    asked
}

/// Now, in unix milliseconds
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_request_names_a_folder_and_until_when() {
        let file = Path::new(r"C:\State\let-go\rename.json");
        let r = request(file, r#"{ "folder": "C:\\Work\\Projects\\Resume", "until": 1793612345678, "by": "rename" }"#)
            .expect("a request");
        assert_eq!(r.folder, PathBuf::from(r"C:\Work\Projects\Resume"));
        assert_eq!(r.until, 1793612345678);
        assert_eq!(r.by, "rename");
        assert_eq!(r.file, file);
        let bom = request(file, "\u{feff}{\"folder\":\"C:/Work/A\",\"until\":5}").expect("a BOM before it");
        assert_eq!((bom.folder, bom.by.as_str()), (PathBuf::from("C:/Work/A"), ""), "who asked is optional");
        for not_one in [
            r#"{ "folder": "C:\\A" }"#,
            r#"{ "until": 5 }"#,
            r#"{ "folder": "", "until": 5 }"#,
            r#"{ "folder": "C:\\A", "until": "soon" }"#,
            r#"{ "folder": "C:\\A", "until": 5.5 }"#,
            r#"["C:\\A", 5]"#,
            "not json",
        ] {
            assert_eq!(request(file, not_one), None, "{not_one}");
        }
    }

    #[test]
    fn the_folder_of_requests_asks_what_its_requests_whose_time_has_not_come_ask() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("let-go");
        assert_eq!(asked(&dir, 100), Asked::default(), "no folder, nothing asked");
        std::fs::create_dir(&dir).unwrap();
        let put = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        put("b-rename.json", r#"{ "folder": "C:\\P\\Resume", "until": 500, "by": "rename" }"#);
        put("a-swap.json", r#"{ "folder": "C:\\P\\Career", "until": 300 }"#);
        put("c-again.json", r#"{ "folder": "c:\\p\\RESUME", "until": 900 }"#);
        put("d-old.json", r#"{ "folder": "C:\\P\\Old", "until": 100 }"#);
        put("e-bad.json", r#"{ "folder": "C:\\P\\Bad" }"#);
        put("f-rename.json.4512.tmp", r#"{ "folder": "C:\\P\\Tmp", "until": 900 }"#);
        put("notes.txt", "not a request");
        std::fs::create_dir(dir.join("sub.json")).unwrap();

        let now = asked(&dir, 100);
        let files: Vec<String> = now
            .requests
            .iter()
            .map(|r| r.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, ["a-swap.json", "b-rename.json", "c-again.json"], "by file name; d's time has come");
        assert_eq!(now.next, Some(300), "the soonest to run out");
        assert_eq!(
            now.folders(),
            vec![PathBuf::from(r"C:\P\Career"), PathBuf::from(r"C:\P\Resume")],
            "each folder once, in any case"
        );
        assert_eq!(now.refused.len(), 1);
        assert!(now.refused[0].0.ends_with("e-bad.json"));

        let later = asked(&dir, 500);
        assert_eq!(later.folders(), vec![PathBuf::from(r"c:\p\RESUME")], "b's time has come too, at its until");
        assert_eq!(later.next, Some(900));
        assert!(asked(&dir, 900).requests.is_empty());
    }
}
