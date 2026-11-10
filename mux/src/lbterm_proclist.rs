//! LBTerm: a local pane's process list, brought up to date only when its processes move
//! (lbterm_procs has when, and the job that says so; lbterm/PERF.md, finding 49).
//!
//! A module of localpane.rs's own (`#[path]` there), so the pane's `CachedProcInfo` stays
//! upstream's and private, and localpane.rs keeps only the hooks that feed this: output reaching
//! the pane's terminal (`printed`) and whether its title and cwd come from its processes as that
//! output left them (`shows`), its asks for the list (`list`) and the job made with it
//! (`ProcShared::new`).
//!
//! Whether its title and cwd are its processes' is taken from its terminal each time output has
//! been put in (perform_actions), where they change: a pane's own title (OSC 0) or folder (OSC 7)
//! set once is its own until it's set again. Until 2026-11-01 it was taken as a title or cwd was
//! asked for, so a background tab nobody asked since its start kept the answer from before its
//! program had said either, and its first programs walked every process for nothing.

use super::CachedProcInfo;
use crate::lbterm_procs::{self, Move, Next, Refresh};
use crate::pane::{CachePolicy, PaneId};
use crate::{Mux, MuxNotification};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use portable_pty::Child;
use procinfo::LocalProcessInfo;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use wezterm_term::Alert;

/// How many of the pane's processes that ended are remembered, for a walk that may have caught
/// one as it went (`ProcShared::put`)
const ENDED_KEPT: usize = 16;

/// A pane's list as walked, with what the asks and the thread go by
struct Listed {
    info: CachedProcInfo,
    /// The job's count as its walk began (lbterm_procs::count; None without one)
    count: Option<u64>,
    /// The pane's output count (ProcShared::outputs) as the foreground's cwd was read
    cwd_output: u64,
}

impl Listed {
    /// A walk's list, which began at the job's count `count` and the output count `output`
    fn walked(info: CachedProcInfo, count: Option<u64>, output: u64) -> Self {
        Self {
            info,
            count,
            cwd_output: output,
        }
    }

    /// Reads the foreground process's cwd again alone, one process opened (`output`: the pane's
    /// output count before the read); whether it changed. A process gone, or one LBTerm may not
    /// read, keeps the cwd it had
    fn read_cwd(&mut self, output: u64) -> bool {
        self.cwd_output = output;
        let foreground = &mut self.info.foreground;
        match LocalProcessInfo::current_working_dir(foreground.pid) {
            Some(cwd) if cwd != foreground.cwd => {
                foreground.cwd = cwd;
                true
            }
            _ => false,
        }
    }
}

/// What an ask for a pane's process list wants
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ask {
    /// The list itself: the foreground process's name, Lua's process info
    List,
    /// The foreground's cwd alone, read again there and then when the pane printed since it was
    /// read (lbterm_procs::Refresh::ReadCwd): the GUI's own asks, `cli list`, a new tab's folder
    Cwd,
    /// The foreground's cwd as last listed, for a mux server's push: when the pane printed since
    /// it was read, the lbterm-procs thread reads it, a pace after its last read, and says what
    /// changed (`tell`), so the server's main thread never reads a process's memory, and a
    /// flooding pane's cwd is read at most every lbterm_procs::PACE rather than at every push
    Listed,
}

/// What a pane shares with the thread that brings its process list up to date and with its
/// job's messages (lbterm_procs has when that is)
pub(super) struct ProcShared {
    pane_id: PaneId,
    /// Its program's
    pid: Option<u32>,
    list: Mutex<Option<Listed>>,
    /// The listed foreground's pid (0: no list yet), which a job's message saying a process ended
    /// is matched against
    foreground: AtomicU32,
    /// The pids of the pane's processes that ended last, newest last
    ended: Mutex<VecDeque<u32>>,
    /// The foreground a walk last found gone as it was put in place, walked again for once
    dead: AtomicU32,
    /// The foreground whose end a walk waits for before it looks: Windows takes a process out of
    /// the list of processes a moment after its job says it ended
    ending: AtomicU32,
    /// The thread is at it
    refreshing: AtomicBool,
    /// Something asked for the cwd since the thread last read it
    cwd_asked: AtomicBool,
    /// A walk is wanted since the last: a process started in a pane titled by its foreground,
    /// the foreground ended, or something asked for the list itself and found the processes moved
    walk_asked: AtomicBool,
    /// How many times output reached the pane's terminal (perform_actions)
    outputs: AtomicU64,
    /// When the thread's last walk or read began, for lbterm_procs::wait
    last: Mutex<Option<Instant>>,
    /// Whether the pane's title is its foreground process's name (its terminal's is the default)
    /// and its cwd its foreground process's (its shell has said none), as its output left them:
    /// both, till it has printed
    shows_name: AtomicBool,
    shows_cwd: AtomicBool,
    /// The pane's processes, as a job of their own
    #[cfg(windows)]
    watch: Option<lbterm_procs::Watch>,
    /// Walks, for the tests
    #[cfg(test)]
    walks: std::sync::atomic::AtomicUsize,
}

impl ProcShared {
    /// A new pane's, with its program (`process`, just started) put in a job of its own, before
    /// its waiter takes the process
    pub(super) fn new(pane_id: PaneId, process: &mut Box<dyn Child + Send>) -> Arc<Self> {
        Arc::new_cyclic(|_me| Self {
            pane_id,
            pid: process.process_id(),
            list: Mutex::new(None),
            foreground: AtomicU32::new(0),
            ended: Mutex::new(VecDeque::with_capacity(ENDED_KEPT)),
            dead: AtomicU32::new(0),
            ending: AtomicU32::new(0),
            refreshing: AtomicBool::new(false),
            cwd_asked: AtomicBool::new(false),
            walk_asked: AtomicBool::new(false),
            outputs: AtomicU64::new(0),
            last: Mutex::new(None),
            shows_name: AtomicBool::new(true),
            shows_cwd: AtomicBool::new(true),
            #[cfg(windows)]
            watch: watch(pane_id, process, _me.clone()),
            #[cfg(test)]
            walks: Default::default(),
        })
    }

    /// Output reached the pane's terminal: its shell may have moved to another folder, read again
    /// once asked for
    pub(super) fn printed(&self) {
        self.outputs.fetch_add(1, Ordering::AcqRel);
    }

    /// Whether the pane's title is its foreground process's name (`name`: its terminal's title is
    /// the default, which get_title answers with that name) and its cwd its foreground process's
    /// (`cwd`: its shell has said none), as output just put into its terminal left them
    pub(super) fn shows(&self, name: bool, cwd: bool) {
        self.shows_name.store(name, Ordering::Relaxed);
        self.shows_cwd.store(cwd, Ordering::Relaxed);
    }

    /// The pane's list, its program `pid`'s, for an ask with `policy` for what `ask` says.
    /// Walked again for an ask for the list itself only when the pane's processes moved since it
    /// was taken (its job's count); never for an ask for the cwd alone, whose foreground's end
    /// comes as its job's message, but read again alone when the pane printed since, there and
    /// then for `Ask::Cwd` and on the thread for `Ask::Listed`; output alone walks nothing
    /// (lbterm_procs). AllowStale callers run on the GUI thread (`cli list`, the tab bar's Lua,
    /// the tab panel) and in a mux server's pushes, so only FetchImmediate, or a pane with no
    /// list yet, waits for a walk: the rest are answered with the list there is while the thread
    /// brings it up to date. The job's count is asked only for what it decides: an ask for the
    /// list itself, or a walk (an ask for the cwd alone asked it at every push before)
    pub(super) fn list(
        self: &Arc<Self>,
        pid: u32,
        policy: CachePolicy,
        ask: Ask,
    ) -> Option<MappedMutexGuard<'_, CachedProcInfo>> {
        let cwd = ask != Ask::List;
        let count = (!cwd).then(|| self.count());
        let output = self.outputs.load(Ordering::Acquire);
        let mut list = self.list.lock();
        let listed = list
            .as_ref()
            .map(|l| if cwd { None } else { l.count });
        let count = match count {
            Some(count) => count,
            // A walk takes the count as it begins
            None if list.is_none() || policy == CachePolicy::FetchImmediate => self.count(),
            None => None,
        };
        let cwd_stale = cwd && list.as_ref().map_or(false, |l| l.cwd_output != output);
        // Before the look at the thread, so one ending now sees it
        if cwd_stale {
            self.cwd_asked.store(true, Ordering::Release);
        }
        let refreshing = self.refreshing.load(Ordering::Acquire);
        // The processes moved since the list: a walk, unless the thread is at it already, whose
        // walk began after the move it was told of (asks during a walk found the list it
        // replaces, and each had it walked again a pace on); one that ends without walking
        // leaves the next ask to find it moved
        if !refreshing && listed.map_or(false, |listed| lbterm_procs::moved(listed, count)) {
            self.walk_asked.store(true, Ordering::Release);
        }
        match lbterm_procs::refresh(policy, listed, count, refreshing, cwd_stale) {
            Refresh::Now => {
                log::debug!(target: "lbterm::procs", "pane {}: walk, asked {policy:?}", self.pane_id);
                self.walk_asked.store(false, Ordering::Release);
                #[cfg(test)]
                self.walks.fetch_add(1, Ordering::Relaxed);
                let mut fresh = Listed::walked(CachedProcInfo::fetch(pid)?, count, output);
                if let Some(old) = list.as_ref() {
                    let (name, cwd) = shown(&old.info.foreground, &fresh.info.foreground);
                    self.tell(name, cwd);
                }
                if unread(&fresh.info.foreground) {
                    // The next ask for the cwd reads it alone
                    keep_cwd(&mut fresh, list.as_ref());
                }
                if self.put(&mut list, fresh) {
                    self.refresh_in_background();
                }
            }
            Refresh::Background => self.refresh_in_background(),
            // cwd_asked is set: the thread reads it, a pace after its last read
            Refresh::ReadCwd if ask == Ask::Listed => self.refresh_in_background(),
            Refresh::ReadCwd => {
                log::debug!(target: "lbterm::procs", "pane {}: cwd read, asked", self.pane_id);
                self.cwd_asked.store(false, Ordering::Release);
                if let Some(l) = list.as_mut() {
                    if l.read_cwd(output) {
                        self.tell(false, true);
                    }
                }
            }
            Refresh::No => {}
        }
        Some(MutexGuard::map(list, |l| &mut l.as_mut().unwrap().info))
    }

    /// Its job's message (on the lbterm-jobs thread): one of its processes started or ended
    /// (`moved`). A walk if that changes what's shown (lbterm_procs::walks_for): a process
    /// starting in a pane titled by its foreground process, or the listed foreground ending in a
    /// pane that shows its name or its cwd. A Claude Code session names itself, so the statusline
    /// and hooks it starts walk nothing, and once its shell or claudehook has said its cwd (OSC 7)
    /// shows neither
    fn moved(self: &Arc<Self>, moved: Move) {
        if let Move::Ended(pid) = moved {
            let mut ended = self.ended.lock();
            if ended.len() == ENDED_KEPT {
                ended.pop_front();
            }
            ended.push_back(pid);
        }
        let foreground = self.foreground.load(Ordering::Acquire);
        let (name, cwd) = (
            self.shows_name.load(Ordering::Relaxed),
            self.shows_cwd.load(Ordering::Relaxed),
        );
        let walk = foreground != 0
            && self.pid.is_some()
            && lbterm_procs::walks_for(moved, Some(foreground), name, cwd);
        // perf/proc-title.ps1 and perf/at-rest.ps1 count these and the walks and reads below
        // (WEZTERM_LOG)
        log::debug!(
            target: "lbterm::procs",
            "pane {}: its processes moved: {moved:?}, its foreground {foreground} (shown: {}){}",
            self.pane_id,
            name || cwd,
            if walk { ", to walk" } else { "" }
        );
        if walk {
            if let Move::Ended(pid) = moved {
                self.ending.store(pid, Ordering::Release);
            }
            self.walk_asked.store(true, Ordering::Release);
            self.refresh_in_background();
        }
    }

    /// The pane's job's count now (lbterm_procs::count), None without a job
    fn count(&self) -> Option<u64> {
        #[cfg(windows)]
        return self.watch.as_ref().and_then(|watch| watch.count());
        #[cfg(not(windows))]
        None
    }

    /// Puts a walk's list in place, its foreground's pid where the job's messages are matched
    /// against it. Whether that foreground has gone already (a process that ended, or no longer
    /// runs): one the walk found as it ended, whose end's message was matched against the
    /// foreground before, wants the list walked again, once for the same process
    fn put(&self, list: &mut Option<Listed>, fresh: Listed) -> bool {
        let foreground = fresh.info.foreground.pid;
        *list = Some(fresh);
        self.foreground.store(foreground, Ordering::Release);
        let gone =
            self.ended.lock().contains(&foreground) || !lbterm_procs::running(foreground);
        if gone && self.dead.swap(foreground, Ordering::AcqRel) != foreground {
            log::debug!(target: "lbterm::procs", "pane {}: its foreground {foreground} has gone: walk again", self.pane_id);
            self.walk_asked.store(true, Ordering::Release);
            return true;
        }
        false
    }

    /// What the thread does next: a walk when one is wanted, else a read of the cwd when it was
    /// asked for while the thread was at it and the pane printed since (an ask with the thread at
    /// rest reads it itself)
    fn next(&self) -> Next {
        let list = self.list.lock();
        match list.as_ref() {
            Some(l) => lbterm_procs::next(
                self.walk_asked.load(Ordering::Acquire),
                self.cwd_asked.load(Ordering::Acquire)
                    && l.cwd_output != self.outputs.load(Ordering::Acquire),
            ),
            None => Next::Done,
        }
    }

    /// Brings the list up to date on a thread of its own, unless the thread is at it already
    fn refresh_in_background(self: &Arc<Self>) {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("lbterm-procs".into())
            .spawn(move || shared.refresh());
        if let Err(err) = spawned {
            log::error!(
                "pane {}: no thread to bring its process list up to date: {err:#}",
                self.pane_id
            );
            self.refreshing.store(false, Ordering::Release);
        }
    }

    /// The thread: walks while walks are wanted, at most every lbterm_procs::PACE, reads the cwd
    /// alone when it was asked for meanwhile, says what changed that consumers show, and ends
    /// when there's nothing more to do
    fn refresh(&self) {
        let Some(pid) = self.pid else {
            self.refreshing.store(false, Ordering::Release);
            return;
        };
        loop {
            let next = match self.next() {
                Next::Done => {
                    self.refreshing.store(false, Ordering::Release);
                    // An ask between the look and that saw the thread at it and left it to it
                    if self.next() == Next::Done || self.refreshing.swap(true, Ordering::AcqRel) {
                        return;
                    }
                    continue;
                }
                next => next,
            };
            let wait = lbterm_procs::wait(self.last.lock().map(|began| began.elapsed()));
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            *self.last.lock() = Some(Instant::now());
            // Each reads the cwd
            self.cwd_asked.store(false, Ordering::Release);
            let output = self.outputs.load(Ordering::Acquire);
            let shown = match next {
                Next::Walk => self.walk(pid, output),
                // A cwd asked for while it walked
                Next::ReadCwd => {
                    log::debug!(target: "lbterm::procs", "pane {}: cwd read, after a walk", self.pane_id);
                    let mut list = self.list.lock();
                    let changed = list.as_mut().map_or(false, |l| l.read_cwd(output));
                    changed.then(|| (false, true))
                }
                Next::Done => None,
            };
            if let Some((name, cwd)) = shown {
                self.tell(name, cwd);
            }
        }
    }

    /// The thread's walk of the processes under `pid`; what changed that consumers show
    fn walk(&self, pid: u32, output: u64) -> Option<(bool, bool)> {
        self.walk_asked.store(false, Ordering::Release);
        let ending = self.ending.swap(0, Ordering::AcqRel);
        if ending != 0 {
            lbterm_procs::wait_ended(ending, lbterm_procs::PACE);
        }
        let count = self.count();
        log::debug!(target: "lbterm::procs", "pane {}: walk, its processes moved", self.pane_id);
        #[cfg(test)]
        self.walks.fetch_add(1, Ordering::Relaxed);
        let fresh = CachedProcInfo::fetch(pid);
        let mut list = self.list.lock();
        match fresh {
            // A FetchImmediate walk that began after this one is the newer
            Some(fresh)
                if list
                    .as_ref()
                    .map_or(false, |old| old.info.updated > fresh.updated) =>
            {
                None
            }
            Some(fresh) => {
                let mut fresh = Listed::walked(fresh, count, output);
                let shown = list
                    .as_ref()
                    .map(|old| shown(&old.info.foreground, &fresh.info.foreground));
                if unread(&fresh.info.foreground) {
                    // Read again alone once it can be, a pace on, the cwd there was standing for
                    // it till then
                    keep_cwd(&mut fresh, list.as_ref());
                    self.cwd_asked.store(true, Ordering::Release);
                }
                // A foreground gone already is walked again a pace on, by the loop
                self.put(&mut list, fresh);
                shown
            }
            // Its program is gone, the pane with it: what's listed stays, as of now
            None => {
                if let Some(old) = list.as_mut() {
                    old.count = count;
                }
                None
            }
        }
    }

    /// Has the consumers of what a new list changed read it again: the window's title and the
    /// rail, and a mux server's clients, whose push carries the title and cwd. The name's for a
    /// pane titled by it, the cwd for one whose shell doesn't say it (OSC 7); a title alert is
    /// all a consumer needs to read get_title() again (term's performer)
    fn tell(&self, name: bool, cwd: bool) {
        let pane_id = self.pane_id;
        let mut alerts = vec![];
        if name && self.shows_name.load(Ordering::Relaxed) {
            alerts.push(Alert::IconTitleChanged(None));
        }
        if cwd && self.shows_cwd.load(Ordering::Relaxed) {
            alerts.push(Alert::CurrentWorkingDirectoryChanged);
        }
        if alerts.is_empty() {
            return;
        }
        // Never at once, even on the main thread: an ask that walked holds the pane's locks, and
        // a subscriber may ask the pane again
        promise::spawn::spawn_into_main_thread(async move {
            let Some(mux) = Mux::try_get() else {
                return;
            };
            for alert in alerts {
                // perf/proc-title.ps1 counts them
                log::debug!(target: "lbterm::title", "alert pane {pane_id} {alert:?} (its processes)");
                mux.notify(MuxNotification::Alert { pane_id, alert });
            }
        })
        .detach();
    }
}

/// A job for a new pane's processes (lbterm_procs), whose messages reach `procs`. None for a pane
/// without a process of its own here (an ssh or tmux pane, or one whose program couldn't start);
/// one that can't be made is said, and that pane's process list then stays as first walked, but
/// for FetchImmediate
#[cfg(windows)]
fn watch(
    pane_id: PaneId,
    process: &mut Box<dyn Child + Send>,
    procs: std::sync::Weak<ProcShared>,
) -> Option<lbterm_procs::Watch> {
    let handle = process.as_raw_handle()?;
    process.process_id()?;
    let moved = Arc::new(move |moved: Move| {
        if let Some(procs) = procs.upgrade() {
            procs.moved(moved);
        }
    });
    match lbterm_procs::Watch::new(handle, moved) {
        Ok(watch) => Some(watch),
        Err(err) if matches!(process.try_wait(), Ok(Some(_))) => {
            log::debug!("pane {pane_id}: its program ended before its job was made: {err}");
            None
        }
        Err(err) => {
            log::error!(
                "pane {pane_id}: no job for its processes ({err}), so its title and folder \
                 taken from them stay as first found"
            );
            None
        }
    }
}

/// What a new list changes that consumers show: the foreground process's name (a pane without a
/// title of its own is titled by it, get_title) and its cwd. A cwd not read (`unread`) is no
/// change yet
fn shown(old: &LocalProcessInfo, new: &LocalProcessInfo) -> (bool, bool) {
    (
        old.executable.file_name() != new.executable.file_name(),
        !unread(new) && old.cwd != new.cwd,
    )
}

/// Whether a walk couldn't read a process's cwd: one just started (the job's message comes as it
/// does), whose parameters its loader hasn't yet made pointers of (RtlNormalizeProcessParams), has
/// none to read, or one LBTerm may not open
fn unread(process: &LocalProcessInfo) -> bool {
    process.cwd.as_os_str().is_empty()
}

/// A list whose foreground's cwd wasn't read (`unread`) has it read again alone: the cwd the list
/// before had stands for it till then
fn keep_cwd(fresh: &mut Listed, old: Option<&Listed>) {
    if let Some(old) = old {
        fresh.info.foreground.cwd = old.info.foreground.cwd.clone();
    }
    fresh.cwd_output = u64::MAX;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn process(exe: &str, cwd: &str) -> LocalProcessInfo {
        LocalProcessInfo {
            pid: 1,
            ppid: 0,
            name: String::new(),
            executable: PathBuf::from(exe),
            argv: vec![],
            cwd: PathBuf::from(cwd),
            status: procinfo::LocalProcessStatus::Run,
            start_time: 0,
            #[cfg(windows)]
            console: 1,
            children: HashMap::new(),
        }
    }

    #[test]
    fn a_new_list_says_what_consumers_show_changed() {
        let pwsh = process(r"C:\Program Files\PowerShell\7\pwsh.exe", r"C:\work");
        // The same again: nothing to tell
        assert_eq!(shown(&pwsh, &pwsh), (false, false));
        // A program started at the prompt: the name a pane without a title of its own shows
        let sort = process(r"C:\Windows\System32\sort.exe", r"C:\work");
        assert_eq!(shown(&pwsh, &sort), (true, false));
        // The same program from another folder titles the pane alike
        let other = process(r"C:\Tools\pwsh.exe", r"C:\work");
        assert_eq!(shown(&pwsh, &other), (false, false));
        // cmd's cd
        let moved = process(r"C:\Program Files\PowerShell\7\pwsh.exe", r"C:\work\sub");
        assert_eq!(shown(&pwsh, &moved), (false, true));
        // A program just started, its cwd not readable yet: its name, and no cwd till it's read
        let started = process(r"C:\Windows\System32\PING.EXE", "");
        assert!(unread(&started));
        assert_eq!(shown(&pwsh, &started), (true, false));
    }

    #[test]
    fn a_cwd_not_read_yet_keeps_the_one_before_till_it_is() {
        let list = |fg: LocalProcessInfo| {
            Listed::walked(
                CachedProcInfo {
                    root: fg.clone(),
                    updated: Instant::now(),
                    foreground: fg,
                },
                Some(lbterm_procs::count(2, 2)),
                7,
            )
        };
        let old = list(process(
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            r"C:\work",
        ));
        let mut fresh = list(process(r"C:\Windows\System32\PING.EXE", ""));
        keep_cwd(&mut fresh, Some(&old));
        assert_eq!(fresh.info.foreground.cwd, PathBuf::from(r"C:\work"));
        // Never the pane's output count, so the next ask for the cwd reads it alone
        assert_eq!(fresh.cwd_output, u64::MAX);
        // The pane's first list has nothing to stand in: an empty cwd, read on the next ask
        let mut first = list(process(r"C:\Program Files\PowerShell\7\pwsh.exe", ""));
        keep_cwd(&mut first, None);
        assert!(unread(&first.info.foreground));
        assert_eq!(first.cwd_output, u64::MAX);
    }

    /// A pane whose cwd alone is its foreground's (its program names itself: cmd, Git Bash, a
    /// Claude Code session before its first OSC 7) walks nothing as other processes start and
    /// end, and walks once its foreground itself has ended: a cmd that runs a ping, then starts a
    /// second cmd that waits; the second is made the foreground by a FetchImmediate ask, and its
    /// end, which its job's message says (before the job's counts drop, as often as not), has the
    /// list walked again
    #[cfg(windows)]
    #[test]
    fn a_pane_showing_its_foregrounds_cwd_alone_walks_only_when_the_foreground_ends() {
        use std::io::{Read, Write};
        use std::os::windows::process::CommandExt;
        use std::time::Duration;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c"])
            .raw_arg(
                "echo ready& set /p x=& ping -n 2 127.0.0.1 >nul& \
                 cmd /d /c \"echo inner& set /p z=\"& set /p y=",
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        // Until it has said `said`, a byte at a time: set /p takes a line from what's written
        let mut heard = String::new();
        let mut wait_for = |said: &str| {
            let mut byte = [0u8; 1];
            while !heard.ends_with(said) {
                stdout.read_exact(&mut byte).unwrap();
                heard.push(byte[0] as char);
            }
        };
        wait_for("ready");
        let mut process: Box<dyn Child + Send> = Box::new(child);
        let procs = ProcShared::new(1, &mut process);
        procs.shows(false, true);
        let walks = |procs: &Arc<ProcShared>| procs.walks.load(Ordering::Relaxed);
        // The thread done with what it heard: at rest, and again a moment on
        let settle = |procs: &Arc<ProcShared>| {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                std::thread::sleep(Duration::from_millis(400));
                if !procs.refreshing.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(400));
                    if !procs.refreshing.load(Ordering::Acquire) {
                        return;
                    }
                }
                assert!(Instant::now() < until, "the thread never came to rest");
            }
        };
        let root_cwd = procs
            .list(pid, CachePolicy::AllowStale, Ask::Cwd)
            .map(|info| info.foreground.cwd.clone())
            .unwrap();
        assert_eq!(walks(&procs), 1, "the first ask walks");
        // ping starts and ends, then the second cmd starts: cmd is still the foreground the cwd
        // is shown for, so nothing is walked, and asks for the cwd walk nothing either
        stdin.write_all(b"go\r\n").unwrap();
        wait_for("inner");
        settle(&procs);
        assert_eq!(
            procs.list(pid, CachePolicy::AllowStale, Ask::Cwd).unwrap().foreground.cwd,
            root_cwd
        );
        settle(&procs);
        assert_eq!(walks(&procs), 1, "nothing walked for the processes that came and went");
        // The second cmd, made the foreground by a walk asked for at once
        let inner = procs
            .list(pid, CachePolicy::FetchImmediate, Ask::List)
            .map(|info| info.foreground.pid)
            .unwrap();
        assert_ne!(inner, pid, "the second cmd is the youngest");
        assert_eq!(walks(&procs), 2);
        // Its end: the job's message names it, and the list is walked
        stdin.write_all(b"\r\n").unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while lbterm_procs::running(inner) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        settle(&procs);
        // Once, or twice when the first walk caught it as it ended
        let after = walks(&procs);
        assert!((3..=4).contains(&after), "walked {} times in all", after);
        let foreground = procs
            .list(pid, CachePolicy::AllowStale, Ask::Cwd)
            .map(|info| info.foreground.pid)
            .unwrap();
        assert_ne!(foreground, inner, "the foreground that ended isn't listed");
        stdin.write_all(b"\r\n").unwrap();
        drop(stdin);
        process.wait().unwrap();
    }

    /// An ask for the cwd as last listed (a mux server's push, `Ask::Listed`) never reads it
    /// there and then: a cmd that moves to another folder, its pane printing as it does, is
    /// answered with the folder it had until the lbterm-procs thread has read the new one, which
    /// it says (`tell`, a task for the main thread); an ask for the cwd itself (`Ask::Cwd`) reads
    /// it at once
    #[cfg(windows)]
    #[test]
    fn a_push_takes_the_cwd_as_listed_and_the_thread_reads_it_again() {
        use std::io::{Read, Write};
        use std::os::windows::process::CommandExt;
        use std::sync::atomic::AtomicUsize;
        use std::time::Duration;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // What tell() hands the main thread, counted, there being no main thread here
        static TOLD: AtomicUsize = AtomicUsize::new(0);
        promise::spawn::set_schedulers(
            Box::new(|_task, _site| {
                TOLD.fetch_add(1, Ordering::Relaxed);
            }),
            Box::new(|_task, _site| {}),
        );
        let windir = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        // A process's cwd reads with a separator at its end, and in the case it was given
        let name = |p: &PathBuf| {
            p.to_string_lossy()
                .trim_end_matches(std::path::is_separator)
                .to_lowercase()
        };
        let at_windir = |cwd: &PathBuf| name(cwd) == name(&windir);
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c"])
            .raw_arg("echo ready& set /p x=& cd /d %SystemRoot%& echo moved& set /p y=")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .current_dir(std::env::temp_dir())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut heard = String::new();
        let mut wait_for = |said: &str| {
            let mut byte = [0u8; 1];
            while !heard.ends_with(said) {
                stdout.read_exact(&mut byte).unwrap();
                heard.push(byte[0] as char);
            }
        };
        wait_for("ready");
        let mut process: Box<dyn Child + Send> = Box::new(child);
        let procs = ProcShared::new(1, &mut process);
        procs.shows(false, true);
        let settle = |procs: &Arc<ProcShared>| {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                std::thread::sleep(Duration::from_millis(400));
                if !procs.refreshing.load(Ordering::Acquire) {
                    return;
                }
                assert!(Instant::now() < until, "the thread never came to rest");
            }
        };
        let cwd = |ask: Ask| {
            procs
                .list(pid, CachePolicy::AllowStale, ask)
                .map(|info| info.foreground.cwd.clone())
                .unwrap()
        };
        let first = cwd(Ask::Listed);
        assert_eq!(procs.walks.load(Ordering::Relaxed), 1, "the first ask walks");
        assert!(!at_windir(&first), "{:?}", first);
        settle(&procs);
        stdin.write_all(b"go\r\n").unwrap();
        wait_for("moved");
        procs.printed();
        let told = TOLD.load(Ordering::Relaxed);
        assert_eq!(cwd(Ask::Listed), first, "the push isn't held for a read");
        settle(&procs);
        let read = cwd(Ask::Listed);
        assert!(at_windir(&read), "the thread read it: {:?}", read);
        assert!(
            TOLD.load(Ordering::Relaxed) > told,
            "the thread said the cwd changed"
        );
        assert_eq!(procs.walks.load(Ordering::Relaxed), 1, "a read, not a walk");
        // An ask for the cwd itself, once the pane printed since, reads it there and then
        procs.printed();
        let read = cwd(Ask::Cwd);
        assert!(at_windir(&read), "{:?}", read);
        assert!(!procs.refreshing.load(Ordering::Acquire), "no thread for it");
        stdin.write_all(b"\r\n").unwrap();
        drop(stdin);
        process.wait().unwrap();
    }
}
