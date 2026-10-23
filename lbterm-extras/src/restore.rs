//! Crash restore: LBTerm keeps a note of its tabs, and when it starts after one that didn't
//! close cleanly (a crash, the process killed, Windows restarting) it offers to open them
//! again, all of them or the ones picked.
//!
//! A tab comes back as one pane in its folder. One that was running Claude Code comes back
//! as `claude --resume <session>` in a pwsh that stays open afterwards, as AI Dashboard's
//! resume does, so the conversation carries on where it stopped. The session id comes from
//! the `claude_session` user var Terminal's claudehook.exe sets in the pane.

use serde::{Deserialize, Serialize};

/// The note LBTerm keeps, rewritten as tabs change
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Session {
    /// The GUI process that wrote it, with its start time so a reused pid isn't mistaken
    /// for it
    pub pid: u32,
    pub started: u64,
    /// Set when LBTerm closes normally: nothing to offer next time
    pub clean: bool,
    pub windows: Vec<WindowNote>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WindowNote {
    pub tabs: Vec<TabNote>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TabNote {
    /// What the tab showed: Claude Code's topic, or the program
    pub title: String,
    pub cwd: Option<String>,
    /// The Claude Code session running in it, if one was
    pub claude_session: Option<String>,
}

impl Session {
    pub fn tabs(&self) -> impl Iterator<Item = &TabNote> {
        self.windows.iter().flat_map(|w| w.tabs.iter())
    }
}

/// Whether to offer to restore `previous`, the note found at start-up. `running(pid,
/// started)` says whether that process is still going (another LBTerm with the same note).
pub fn offer(previous: Option<&Session>, running: impl Fn(u32, u64) -> bool) -> bool {
    match previous {
        Some(s) => !s.clean && s.tabs().next().is_some() && !running(s.pid, s.started),
        None => false,
    }
}

/// A pane that outlived the last GUI: with panes in a mux server (a unix domain), a GUI crash
/// leaves them running, and the next GUI reattaches to them
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LivePane {
    pub cwd: Option<String>,
    pub claude_session: Option<String>,
}

fn same_folder(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase();
    trim(a) == trim(b)
}

/// The noted tabs that aren't open any more, by index: the ones to offer. A tab running Claude
/// Code is open if a live pane has its session; any other, if a live pane is in its folder.
/// Each live pane accounts for one tab at most.
pub fn still_closed(tabs: &[&TabNote], live: &[LivePane]) -> Vec<usize> {
    let mut used = vec![false; live.len()];
    let mut closed = vec![];
    // Sessions first, so a Claude pane is never spent on a shell tab in the same folder
    let mut open = vec![false; tabs.len()];
    for (i, tab) in tabs.iter().enumerate() {
        let Some(id) = tab.claude_session.as_deref().filter(|id| is_session_id(id)) else {
            continue;
        };
        if let Some(j) = (0..live.len()).find(|&j| !used[j] && live[j].claude_session.as_deref() == Some(id)) {
            used[j] = true;
            open[i] = true;
        }
    }
    for (i, tab) in tabs.iter().enumerate() {
        if open[i] || tab.claude_session.as_deref().is_some_and(is_session_id) {
            continue;
        }
        let Some(cwd) = tab.cwd.as_deref() else {
            continue;
        };
        if let Some(j) = (0..live.len()).find(|&j| {
            !used[j] && live[j].claude_session.is_none() && live[j].cwd.as_deref().is_some_and(|c| same_folder(c, cwd))
        }) {
            used[j] = true;
            open[i] = true;
        }
    }
    for (i, is_open) in open.iter().enumerate() {
        if !is_open {
            closed.push(i);
        }
    }
    closed
}

/// A Claude Code session id, as Claude Code writes them
pub fn is_session_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// The command a restored tab runs, or `None` for the default shell. `pwsh` is the full
/// path of PowerShell 7; `claude` is `claude`, or a stand-in for tests.
pub fn command(tab: &TabNote, pwsh: &str, claude: &str) -> Option<Vec<String>> {
    let id = tab.claude_session.as_deref().filter(|id| is_session_id(id))?;
    Some(
        [pwsh, "-NoLogo", "-NoExit", "-Command", claude, "--resume", id]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
}

/// One row of the prompt
pub fn describe(tab: &TabNote) -> String {
    // Claude Code's status glyph leads its titles; it means nothing after a restart
    let title = tab
        .title
        .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '(' && c != '[')
        .trim();
    let folder = tab
        .cwd
        .as_deref()
        .map(|c| c.trim_end_matches(['\\', '/']))
        .and_then(|c| c.rsplit(['\\', '/']).next())
        .unwrap_or("");
    let claude = command(tab, "", "").is_some();
    match (title.is_empty(), folder.is_empty(), claude) {
        (true, true, _) => "(a shell)".to_string(),
        (true, false, true) => format!("Claude in {folder}"),
        (true, false, false) => folder.to_string(),
        (false, true, _) => title.to_string(),
        (false, false, _) => format!("{title}  ·  {folder}"),
    }
}

/// Which tabs are picked in the prompt
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picks {
    picked: Vec<bool>,
    pub cursor: usize,
}

impl Picks {
    /// Everything picked to start with: restoring all is the usual answer
    pub fn new(count: usize) -> Self {
        Self {
            picked: vec![true; count],
            cursor: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.picked.len()
    }

    pub fn is_picked(&self, i: usize) -> bool {
        self.picked.get(i).copied().unwrap_or(false)
    }

    pub fn toggle(&mut self, i: usize) {
        if let Some(p) = self.picked.get_mut(i) {
            *p = !*p;
        }
    }

    pub fn set_all(&mut self, on: bool) {
        self.picked.iter_mut().for_each(|p| *p = on);
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.picked.len() {
            self.cursor += 1;
        }
    }

    pub fn chosen(&self) -> Vec<usize> {
        (0..self.picked.len()).filter(|&i| self.picked[i]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SID: &str = "0b6d7c2e-6a51-4c1e-9d3a-2f1e8b7a9c40";

    fn tab(title: &str, cwd: Option<&str>, sid: Option<&str>) -> TabNote {
        TabNote {
            title: title.into(),
            cwd: cwd.map(Into::into),
            claude_session: sid.map(Into::into),
        }
    }

    fn session(clean: bool, tabs: Vec<TabNote>) -> Session {
        Session {
            pid: 42,
            started: 1000,
            clean,
            windows: vec![WindowNote { tabs }],
        }
    }

    #[test]
    fn offered_after_an_unclean_end_only() {
        let tabs = vec![tab("x", None, None)];
        let gone = |_: u32, _: u64| false;
        assert!(offer(Some(&session(false, tabs.clone())), gone));
        assert!(!offer(Some(&session(true, tabs.clone())), gone));
        assert!(!offer(None, gone));
        // nothing to bring back
        assert!(!offer(Some(&session(false, vec![])), gone));
        // the LBTerm that wrote it is still running
        assert!(!offer(Some(&session(false, tabs)), |pid, started| pid == 42 && started == 1000));
    }

    #[test]
    fn a_claude_tab_resumes_its_session_in_pwsh() {
        let pwsh = r"C:\Program Files\PowerShell\7\pwsh.exe";
        let cmd = command(&tab("✳ Fix the build", Some(r"C:\Code\LBTerm"), Some(SID)), pwsh, "claude").unwrap();
        assert_eq!(cmd, vec![pwsh, "-NoLogo", "-NoExit", "-Command", "claude", "--resume", SID]);
        assert_eq!(command(&tab("pwsh", Some(r"C:\x"), None), pwsh, "claude"), None);
        // anything that isn't a session id is never put on a command line
        assert_eq!(command(&tab("x", None, Some("0; rm -rf /")), pwsh, "claude"), None);
        // a stand-in for tests
        let stub = command(&tab("x", None, Some(SID)), pwsh, r"C:\t\claude.cmd").unwrap();
        assert_eq!(stub[4], r"C:\t\claude.cmd");
    }

    #[test]
    fn rows_name_the_topic_and_folder() {
        assert_eq!(
            describe(&tab("✳ Fix the build", Some(r"C:\Code\LBTerm\"), Some(SID))),
            "Fix the build  ·  LBTerm"
        );
        assert_eq!(describe(&tab("", Some(r"C:\Vault\Garden"), Some(SID))), "Claude in Garden");
        assert_eq!(describe(&tab("", Some("/home/x/proj"), None)), "proj");
        assert_eq!(describe(&tab("", None, None)), "(a shell)");
    }

    #[test]
    fn picking() {
        let mut p = Picks::new(3);
        assert_eq!(p.chosen(), vec![0, 1, 2]);
        p.toggle(1);
        assert_eq!(p.chosen(), vec![0, 2]);
        p.set_all(false);
        assert!(p.chosen().is_empty());
        p.down();
        p.down();
        p.down();
        assert_eq!(p.cursor, 2);
        p.up();
        assert_eq!(p.cursor, 1);
        p.toggle(p.cursor);
        assert_eq!(p.chosen(), vec![1]);
    }

    fn live(cwd: &str, sid: Option<&str>) -> LivePane {
        LivePane {
            cwd: Some(cwd.into()),
            claude_session: sid.map(Into::into),
        }
    }

    #[test]
    fn with_nothing_live_every_tab_is_offered() {
        let a = tab("a", Some(r"C:\x"), Some(SID));
        let b = tab("b", Some(r"C:\y"), None);
        assert_eq!(still_closed(&[&a, &b], &[]), vec![0, 1]);
    }

    #[test]
    fn a_live_session_or_folder_is_not_offered_again() {
        let a = tab("a", Some(r"C:\x"), Some(SID));
        let b = tab("b", Some(r"C:\Y\"), None);
        let c = tab("c", Some(r"C:\z"), None);
        let open = [live(r"C:\x", Some(SID)), live("c:/y", None)];
        assert_eq!(still_closed(&[&a, &b, &c], &open), vec![2]);
    }

    #[test]
    fn a_session_only_matches_a_session() {
        // the Claude tab's session ended; a shell in the same folder doesn't count for it
        let a = tab("a", Some(r"C:\x"), Some(SID));
        assert_eq!(still_closed(&[&a], &[live(r"C:\x", None)]), vec![0]);
        // and a Claude pane doesn't count for a shell tab in its folder
        let b = tab("b", Some(r"C:\x"), None);
        assert_eq!(still_closed(&[&b], &[live(r"C:\x", Some(SID))]), vec![0]);
    }

    #[test]
    fn one_live_pane_covers_one_tab() {
        let a = tab("a", Some(r"C:\x"), None);
        let b = tab("b", Some(r"C:\x"), None);
        assert_eq!(still_closed(&[&a, &b], &[live(r"C:\x", None)]), vec![1]);
    }

    #[test]
    fn the_note_round_trips_as_json() {
        let s = session(false, vec![tab("a", Some("C:\\x"), Some(SID)), tab("b", None, None)]);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&text).unwrap(), s);
    }
}
