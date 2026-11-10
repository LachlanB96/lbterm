//! Crash restore: LBTerm keeps a note of its tabs, and when it starts after one that didn't
//! close cleanly (a crash, the process killed, Windows restarting) it offers to open them
//! again, all of them or the ones picked.
//!
//! A tab comes back as one pane in its folder. One that was running Claude Code comes back
//! as `claude --resume <session>` in a pwsh that stays open afterwards, as the Claude project's
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
    /// Tabs an earlier LBTerm left that this one offered and nobody has answered yet: still
    /// offered next time, whether this one ends cleanly or not
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<TabNote>,
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
    /// It was parked (the mux tab's flag, Lachlan's 8a; lbterm/NEW-TAB.md, "Landing 4"): a tab
    /// reopened from it is parked again. Left out of the note while false, and false in a note
    /// from before 8a
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parked: bool,
}

impl TabNote {
    /// The same tab, whether parked or not: what pairs one still waiting with one of a note's own
    fn same(&self, other: &TabNote) -> bool {
        self.title == other.title
            && self.cwd == other.cwd
            && self.claude_session == other.claude_session
    }
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
        Some(s) => !to_offer(s).is_empty() && !running(s.pid, s.started),
        None => false,
    }
}

/// The tabs a note offers: the ones still waiting from an earlier offer, then, if it didn't
/// end cleanly, its own. A tab is offered once, though it's in both: a Claude session by its id,
/// any other tab by pairing each of its own with one the same still waiting, one for one, so two
/// shell tabs alike (one folder, one title) are both offered, and once each though both wait too.
/// A pair is the same tab parked or not, and the note's own, the newer, says which.
pub fn to_offer(s: &Session) -> Vec<TabNote> {
    let own = s.tabs().filter(|_| !s.clean);
    let mut tabs: Vec<TabNote> = vec![];
    // Which of tabs came from pending and haven't been paired with one of the note's own yet
    let mut unpaired: Vec<bool> = vec![];
    let session_seen =
        |tabs: &[TabNote], id: &str| tabs.iter().any(|t| t.claude_session.as_deref() == Some(id));
    for tab in &s.pending {
        if tab.claude_session.as_deref().is_some_and(|id| session_seen(&tabs, id)) {
            continue;
        }
        tabs.push(tab.clone());
        unpaired.push(true);
    }
    for tab in own {
        if let Some(id) = tab.claude_session.as_deref() {
            if session_seen(&tabs, id) {
                continue;
            }
        } else if let Some(i) = (0..tabs.len()).find(|&i| unpaired[i] && tabs[i].same(tab)) {
            unpaired[i] = false;
            tabs[i].parked = tab.parked;
            continue;
        }
        tabs.push(tab.clone());
        unpaired.push(false);
    }
    tabs
}

/// When the note found at start-up is offered, and that it's answered once. It's offered once the
/// GUI's start-up is done (its mux domain attached, so its window has its final tabs: before
/// that, the window holds only the domain's passing connection status), in one window at a time.
/// A prompt that goes without an answer (its overlay taken away: the window moved to another mux
/// window, its tab went, another overlay opened over it, the window closed, or the answer never
/// got to the window) gives the note back, to be offered again; only an
/// answer (or nothing left to offer) ends it. LBTerm keeps its own note all the while, with the
/// offered tabs in it as `pending` until it's answered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Asking {
    /// The note, while it waits to be offered
    note: Option<Session>,
    /// The GUI's start-up is done
    started: bool,
    /// Taken to be offered: a prompt is up, or about to be
    out: bool,
    answered: bool,
}

impl Asking {
    /// With the note found at start-up if it's one to offer (`offer`): with none, it's answered
    /// from the start
    pub fn new(note: Option<Session>) -> Self {
        Self {
            answered: note.is_none(),
            note,
            ..Default::default()
        }
    }

    /// The GUI's start-up is done: its windows have their final tabs
    pub fn start(&mut self) {
        self.started = true;
    }

    /// The note to offer, taken out until it's answered or given back; nothing before start-up
    /// is done, while it's out, or once it's answered
    pub fn take(&mut self) -> Option<Session> {
        if !self.waiting() {
            return None;
        }
        self.out = true;
        self.note.take()
    }

    /// The prompt went without an answer, or found nothing to ask over: offered again at the next
    /// chance
    pub fn give_back(&mut self, note: Session) {
        self.out = false;
        if !self.answered {
            self.note = Some(note);
        }
    }

    /// Answered, or nothing in the note was left to offer
    pub fn answer(&mut self) {
        self.answered = true;
        self.out = false;
        self.note = None;
    }

    /// Answered (or there was nothing to ask): nothing is pending in LBTerm's own note from now on
    pub fn answered(&self) -> bool {
        self.answered
    }

    /// Ready to be offered now: start-up done, not out, not answered
    pub fn waiting(&self) -> bool {
        self.started && !self.out && !self.answered && self.note.is_some()
    }
}

/// What became of a prompt
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// He answered: the tabs to open, by index (none for Esc or "Don't open")
    Answered(Vec<usize>),
    /// It went without an answer: its input ended because its overlay was taken away. The note
    /// stands, given back to be offered again
    Cancelled,
}

/// A window's tab, as the prompt sees it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    /// One of the window's own with nothing over it: the prompt can go over it
    Free,
    /// One of the window's own with an overlay of its own up (the launcher, a close
    /// confirmation): the prompt going over it would take that down. One opened over the prompt
    /// takes the prompt down, and the prompt, asked again over the same tab, took each one down
    /// in turn, so none of them could be used while it was up
    Covered,
    /// One of the GUI's passing tabs, in no window of its own: a mux domain's connection status,
    /// there until the domain has attached (a prompt over it went with it on 2026-10-30), or a
    /// configuration error's
    Passing,
}

/// Why a window has no tab for the prompt now. Either way it's asked again when a tab or a pane
/// comes or goes (an overlay's pane leaving the mux as it closes included)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoTab {
    /// Its open tab has something else over it: the prompt neither takes that down nor opens
    /// another tab from under it
    Covered,
    /// It has none of its own with nothing over it
    NoneFree,
}

/// Which of a window's tabs the prompt goes over, by index: the open one when it's free, else
/// the first free one, opened; nothing while the open tab has something else over it
pub fn prompt_tab(tabs: &[TabKind], active: usize) -> Result<usize, NoTab> {
    match tabs.get(active) {
        Some(TabKind::Free) => Ok(active),
        Some(TabKind::Covered) => Err(NoTab::Covered),
        _ => tabs
            .iter()
            .position(|&t| t == TabKind::Free)
            .ok_or(NoTab::NoneFree),
    }
}

/// Which window the note is offered in, by index, of the GUI's windows in their order: the first
/// with a tab of its own (`Free` or `Covered`), so a window of passing tabs alone, a
/// configuration error's above all, doesn't keep it from the real one
pub fn window_to_ask(windows: &[Vec<TabKind>]) -> Option<usize> {
    windows
        .iter()
        .position(|tabs| tabs.iter().any(|&t| t != TabKind::Passing))
}

/// The live panes a new GUI found in its mux server that may stand for noted tabs: all of a
/// server it attached to; of a server it started itself, only the panes with a Claude session.
/// That server's first tab is new (in `default_cwd`) and mustn't pass for a noted shell tab in
/// the same folder, but a session found in it was resumed there since (by hand, while the
/// prompt was put aside), and offering it again would start a second `claude --resume` on it
pub fn survivors(started_server: bool, server_panes: Vec<LivePane>) -> Vec<LivePane> {
    server_panes
        .into_iter()
        .filter(|p| !started_server || p.claude_session.as_deref().is_some_and(is_session_id))
        .map(|p| LivePane {
            survived: !started_server,
            ..p
        })
        .collect()
}

/// An answer on its way to the window that asked, as a value something must claim. A
/// notification to a window that has gone is dropped without being run (Enter and then the
/// window closing at once, or its domain giving up just then), and the note went with it: never
/// answered, never offered again, and the GUI writing no note of its own for the rest of its
/// life. Dropped unclaimed, it hands the value to `back` instead, to be asked again
pub struct Handed<T, F: FnOnce(T)> {
    value: Option<T>,
    back: Option<F>,
}

impl<T, F: FnOnce(T)> Handed<T, F> {
    pub fn new(value: T, back: F) -> Self {
        Self {
            value: Some(value),
            back: Some(back),
        }
    }

    /// It got there: the value is the claimer's, and `back` never runs
    pub fn claim(mut self) -> T {
        self.back = None;
        self.value.take().expect("a Handed value is claimed once")
    }
}

impl<T, F: FnOnce(T)> Drop for Handed<T, F> {
    fn drop(&mut self) {
        if let (Some(value), Some(back)) = (self.value.take(), self.back.take()) {
            back(value);
        }
    }
}

/// A pane that outlived the last GUI: with panes in a mux server (a unix domain), a GUI crash
/// leaves them running, and the next GUI reattaches to them
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LivePane {
    pub cwd: Option<String>,
    pub claude_session: Option<String>,
    /// It was there before the GUI died: its server was found running (`survivors`), not
    /// started by this GUI, where every pane with a session was resumed by hand since
    pub survived: bool,
}

fn same_folder(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase();
    trim(a) == trim(b)
}

/// The noted tabs that aren't open any more, by index: the ones to offer. A tab running Claude
/// Code is open if a live pane has its session; any other, if a live pane with no session is in
/// its folder, else one that outlived the GUI with a session no noted tab has: Claude Code
/// started in that shell tab within the note's SAVE_AFTER of the GUI dying, so the note still
/// had it as a shell tab, and offering it would open a second tab in that folder. Each live pane
/// accounts for one tab at most.
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
    // Then a shell tab Claude Code started in since the note: a pane that outlived the GUI, in its
    // folder, with a session no noted tab has. After the plain shells, so a shell pane is spent first
    let noted = |id: &str| tabs.iter().any(|t| t.claude_session.as_deref() == Some(id));
    for (i, tab) in tabs.iter().enumerate() {
        if open[i] || tab.claude_session.as_deref().is_some_and(is_session_id) {
            continue;
        }
        let Some(cwd) = tab.cwd.as_deref() else {
            continue;
        };
        if let Some(j) = (0..live.len()).find(|&j| {
            !used[j]
                && live[j].survived
                && live[j].claude_session.as_deref().is_some_and(|id| !noted(id))
                && live[j].cwd.as_deref().is_some_and(|c| same_folder(c, cwd))
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
            parked: false,
        }
    }

    fn session(clean: bool, tabs: Vec<TabNote>) -> Session {
        Session {
            pid: 42,
            started: 1000,
            clean,
            windows: vec![WindowNote { tabs }],
            pending: vec![],
        }
    }

    #[test]
    fn tabs_still_waiting_are_offered_after_a_clean_end_too() {
        let waiting = tab("old", Some(r"C:\x"), Some(SID));
        let mut s = session(true, vec![tab("mine", Some(r"C:\y"), None)]);
        s.pending = vec![waiting.clone()];
        let gone = |_: u32, _: u64| false;
        assert!(offer(Some(&s), gone));
        // the clean run's own tabs aren't
        assert_eq!(to_offer(&s), vec![waiting.clone()]);
        // after an unclean end, the waiting ones first, then its own
        s.clean = false;
        assert_eq!(to_offer(&s), vec![waiting, tab("mine", Some(r"C:\y"), None)]);
    }

    #[test]
    fn a_tab_both_waiting_and_open_is_offered_once() {
        // the same session, retitled since; and the same shell tab twice
        let mut s = session(
            false,
            vec![tab("new title", Some(r"C:\x"), Some(SID)), tab("pwsh", Some(r"C:\y"), None)],
        );
        s.pending = vec![tab("old title", Some(r"C:\x"), Some(SID)), tab("pwsh", Some(r"C:\y"), None)];
        assert_eq!(
            to_offer(&s),
            vec![tab("old title", Some(r"C:\x"), Some(SID)), tab("pwsh", Some(r"C:\y"), None)]
        );
    }

    #[test]
    fn two_shell_tabs_alike_are_both_offered() {
        // two pwsh tabs in one folder: both are tabs to bring back
        let pwsh = tab("pwsh", Some(r"C:\y"), None);
        let s = session(false, vec![pwsh.clone(), pwsh.clone()]);
        assert_eq!(to_offer(&s), vec![pwsh.clone(), pwsh.clone()]);
        // both still waiting and both the note's own: each once
        let mut s = session(false, vec![pwsh.clone(), pwsh.clone()]);
        s.pending = vec![pwsh.clone(), pwsh.clone()];
        assert_eq!(to_offer(&s), vec![pwsh.clone(), pwsh.clone()]);
        // one waiting, two its own: the second is new
        s.pending = vec![pwsh.clone()];
        assert_eq!(to_offer(&s), vec![pwsh.clone(), pwsh.clone()]);
        // a session is still offered once, whatever it's titled
        let mut s = session(false, vec![tab("a", Some(r"C:\x"), Some(SID)), tab("b", Some(r"C:\x"), Some(SID))]);
        assert_eq!(to_offer(&s).len(), 1);
        s.pending = vec![tab("c", Some(r"C:\x"), Some(SID))];
        assert_eq!(to_offer(&s), vec![tab("c", Some(r"C:\x"), Some(SID))]);
        // and each of two shells alike takes one live pane: one live in the folder, one offered
        let s = session(false, vec![pwsh.clone(), pwsh.clone()]);
        let offered = to_offer(&s);
        let noted: Vec<&TabNote> = offered.iter().collect();
        assert_eq!(still_closed(&noted, &[live(r"C:\y", None)]), vec![1]);
    }

    #[test]
    fn a_parked_tab_is_noted_parked_and_pairs_with_itself_unparked() {
        let mut parked = tab("pwsh", Some(r"C:\y"), None);
        parked.parked = true;
        let json = serde_json::to_string(&parked).unwrap();
        assert!(json.contains("\"parked\":true"), "{json}");
        let shell = tab("pwsh", Some(r"C:\y"), None);
        let json = serde_json::to_string(&shell).unwrap();
        assert!(!json.contains("parked"), "nothing said of a tab that wasn't: {json}");
        // A note from before 8a reads unparked
        let old: TabNote = serde_json::from_str(r#"{"title":"pwsh","cwd":null,"claude_session":null}"#).unwrap();
        assert!(!old.parked);
        // Waiting unparked from an earlier offer, parked since in the note's own: offered once, as
        // the note's own says
        let mut s = session(false, vec![parked.clone()]);
        s.pending = vec![shell.clone()];
        assert_eq!(to_offer(&s), vec![parked.clone()]);
    }

    #[test]
    fn an_old_note_without_pending_still_reads() {
        let text = r#"{"pid":1,"started":2,"clean":false,"windows":[{"tabs":[{"title":"a","cwd":null,"claude_session":null}]}]}"#;
        let s: Session = serde_json::from_str(text).unwrap();
        assert!(s.pending.is_empty());
        // and a note with nothing waiting doesn't write the field
        assert!(!serde_json::to_string(&s).unwrap().contains("pending"));
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
            survived: true,
        }
    }

    const SID2: &str = "7e1f2a3b-4c5d-4e6f-8a9b-0c1d2e3f4a5b";

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
        // and a Claude pane whose session is a noted tab's counts for that tab, not for a shell
        // tab in its folder
        let b = tab("b", Some(r"C:\x"), None);
        assert_eq!(still_closed(&[&b, &a], &[live(r"C:\x", Some(SID))]), vec![0]);
    }

    #[test]
    fn claude_started_in_a_shell_tab_since_the_note_keeps_it_open() {
        // the note was written before Claude Code started in the shell tab, and the GUI died
        // within SAVE_AFTER: the pane that outlived it, with a session no noted tab has, is that tab
        let b = tab("b", Some(r"C:\x"), None);
        assert_eq!(still_closed(&[&b], &[live(r"C:\x", Some(SID))]), Vec::<usize>::new());
        // not for a shell tab in another folder
        let c = tab("c", Some(r"C:\y"), None);
        assert_eq!(still_closed(&[&c], &[live(r"C:\x", Some(SID))]), vec![0]);
        // a plain shell pane is spent first, then the Claude one: two shell tabs in the folder,
        // both open
        let d = tab("d", Some(r"C:\x"), None);
        assert_eq!(still_closed(&[&b, &d], &[live(r"C:\x", Some(SID2)), live(r"C:\x", None)]), Vec::<usize>::new());
        // one pane, one tab
        assert_eq!(still_closed(&[&b, &d], &[live(r"C:\x", Some(SID2))]), vec![1]);
    }

    #[test]
    fn a_session_resumed_by_hand_in_a_new_server_never_stands_for_a_shell_tab() {
        // the GUI started the server: every session in it was resumed since, none outlived the
        // GUI, and the shell tab in its folder is offered
        let b = tab("b", Some(r"C:\Code"), None);
        let resumed = survivors(true, vec![live(r"C:\Code", Some(SID))]);
        assert_eq!(resumed.len(), 1);
        assert!(!resumed[0].survived);
        assert_eq!(still_closed(&[&b], &resumed), vec![0]);
        // a server found running: its panes survived
        assert!(survivors(false, vec![live(r"C:\Code", Some(SID))])[0].survived);
    }

    #[test]
    fn one_live_pane_covers_one_tab() {
        let a = tab("a", Some(r"C:\x"), None);
        let b = tab("b", Some(r"C:\x"), None);
        assert_eq!(still_closed(&[&a, &b], &[live(r"C:\x", None)]), vec![1]);
    }

    #[test]
    fn nothing_is_offered_before_start_up_is_done() {
        let note = session(false, vec![tab("a", None, None)]);
        let mut asking = Asking::new(Some(note.clone()));
        // The window's first frame comes before its mux domain has attached: not yet
        assert!(!asking.waiting());
        assert_eq!(asking.take(), None);
        assert!(!asking.answered());
        asking.start();
        assert!(asking.waiting());
        assert_eq!(asking.take(), Some(note));
    }

    #[test]
    fn it_is_offered_once_at_a_time_and_never_after_an_answer() {
        let note = session(false, vec![tab("a", None, None)]);
        let mut asking = Asking::new(Some(note.clone()));
        asking.start();
        let taken = asking.take().unwrap();
        // A second window, a reconnect or a resync while the prompt is up: nothing
        assert!(!asking.waiting());
        assert_eq!(asking.take(), None);
        asking.give_back(taken);
        asking.take().unwrap();
        asking.answer();
        assert!(asking.answered());
        assert!(!asking.waiting());
        assert_eq!(asking.take(), None);
        // A prompt's thread giving it back after the answer changes nothing
        asking.give_back(note);
        assert_eq!(asking.take(), None);
        assert!(asking.answered());
    }

    #[test]
    fn a_prompt_gone_without_an_answer_is_asked_again() {
        let note = session(false, vec![tab("a", None, None), tab("b", None, None)]);
        let mut asking = Asking::new(Some(note.clone()));
        asking.start();
        let taken = asking.take().unwrap();
        // Its overlay taken away (the window moved to another mux window, its tab went)
        asking.give_back(taken);
        assert!(!asking.answered(), "a cancel isn't an answer: the note stands");
        assert!(asking.waiting());
        assert_eq!(asking.take(), Some(note));
    }

    #[test]
    fn with_no_note_it_is_answered_from_the_start() {
        let mut asking = Asking::new(None);
        assert!(asking.answered());
        asking.start();
        assert!(!asking.waiting());
        assert_eq!(asking.take(), None);
        // As the GUI starts when crash restore is off: nothing known, nothing written
        assert!(!Asking::default().answered());
    }

    #[test]
    fn the_prompt_goes_over_a_tab_of_the_windows_own() {
        use TabKind::{Free, Passing};
        // the open tab, when it's one
        assert_eq!(prompt_tab(&[Free, Free], 1), Ok(1));
        // the connection status open over the server's first tab: the server's
        assert_eq!(prompt_tab(&[Passing, Free], 0), Ok(1));
        assert_eq!(prompt_tab(&[Free, Passing], 1), Ok(0));
        // only the connection status: nowhere yet
        assert_eq!(prompt_tab(&[Passing], 0), Err(NoTab::NoneFree));
        assert_eq!(prompt_tab(&[], 0), Err(NoTab::NoneFree));
        assert_eq!(prompt_tab(&[Free], 3), Ok(0));
    }

    #[test]
    fn the_prompt_never_goes_over_another_overlay() {
        use TabKind::{Covered, Free, Passing};
        // The launcher or a close confirmation opened over the prompt took it down, and the
        // prompt asked again over the same tab took that down in turn: it waits now, in that tab
        assert_eq!(prompt_tab(&[Covered, Free], 0), Err(NoTab::Covered));
        assert_eq!(prompt_tab(&[Free, Covered], 1), Err(NoTab::Covered));
        // a tab in the background with an overlay up is passed over too
        assert_eq!(prompt_tab(&[Passing, Covered, Free], 0), Ok(2));
        assert_eq!(prompt_tab(&[Passing, Covered], 0), Err(NoTab::NoneFree));
    }

    #[test]
    fn the_note_is_offered_in_the_first_window_of_its_own() {
        use TabKind::{Covered, Free, Passing};
        // a configuration error's window, its one tab a passing one, sorted first
        assert_eq!(window_to_ask(&[vec![Passing], vec![Free, Free]]), Some(1));
        assert_eq!(window_to_ask(&[vec![Free], vec![Free]]), Some(0));
        // a window whose open tab has an overlay up is still its own: asked there once it's gone
        assert_eq!(window_to_ask(&[vec![Covered], vec![Free]]), Some(0));
        assert_eq!(window_to_ask(&[vec![Passing], vec![]]), None);
        assert_eq!(window_to_ask(&[]), None);
    }

    #[test]
    fn a_server_the_gui_started_has_only_its_sessions_as_survivors() {
        // its first tab, in default_cwd, mustn't pass for the noted tab in the same folder
        let first = live(r"C:\Windows", None);
        let noted = tab("shell", Some(r"C:\Windows"), None);
        assert_eq!(still_closed(&[&noted], &survivors(true, vec![first.clone()])), vec![0]);
        // a server found running: its panes are the noted tabs, still open
        let attached = survivors(false, vec![first.clone()]);
        assert_eq!(still_closed(&[&noted], &attached), Vec::<usize>::new());
        // A session resumed by hand in a server the GUI started, while the prompt was put aside,
        // is open: asked again, it isn't offered (Enter would resume it a second time)
        let claude = tab("Fix the build", Some(r"C:\Code"), Some(SID));
        let resumed = live(r"C:\Code", Some(SID));
        let found = survivors(true, vec![first, resumed.clone()]);
        assert_eq!(found, vec![LivePane { survived: false, ..resumed }]);
        assert_eq!(still_closed(&[&noted, &claude], &found), vec![0]);
        // anything else in a claude_session isn't a session
        assert_eq!(survivors(true, vec![live(r"C:\Code", Some("not a session"))]), vec![]);
    }

    #[test]
    fn an_answer_that_never_gets_there_gives_the_note_back() {
        use std::cell::RefCell;
        let back = RefCell::new(vec![]);
        // claimed: the window opened the tabs, nothing comes back
        let handed = Handed::new(1, |n| back.borrow_mut().push(n));
        assert_eq!(handed.claim(), 1);
        assert!(back.borrow().is_empty());
        // dropped unclaimed, as a notification to a window that has gone is: handed back
        drop(Handed::new(2, |n| back.borrow_mut().push(n)));
        assert_eq!(*back.borrow(), vec![2]);
    }

    #[test]
    fn the_note_round_trips_as_json() {
        let s = session(false, vec![tab("a", Some("C:\\x"), Some(SID)), tab("b", None, None)]);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&text).unwrap(), s);
    }
}
