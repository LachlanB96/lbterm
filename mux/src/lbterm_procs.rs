//! LBTerm: when a pane's process list is walked again (localpane.rs's `divine_process_list`), and
//! the job that says when (lbterm/PERF.md, finding 49).
//!
//! On Windows a pane's foreground process and cwd come from walking its processes: a snapshot of
//! every process on the PC, then each of the pane's opened and read for its cwd and argv, about
//! 18 ms of CPU here. Every paint asks each pane its title, every mux push its title and cwd, and
//! a pane without a title of its own takes it from its foreground process. Upstream walked again
//! for any ask that found the list over 300 ms old, and LBTerm's first fix (4c705a05e) still did
//! for a pane that had printed in the 2 s before: a key's echo at a prompt is output, so each key
//! walked once, in the mux server on a mux domain and in the GUI locally, about 15% of a core at
//! eight keys a second.
//!
//! Now a pane's processes are a job of its own (`Watch`): its program is put in it as it starts,
//! and every process started under it joins it, so the job's counts of processes started and
//! running move exactly when one of them starts or ends, and the job says so on a completion port
//! the process's one `lbterm-jobs` thread waits on (asleep, at rest). A list taken at the counts
//! there are now is current; output alone, a key's echo, walks nothing. When they move, the job's
//! message has the list brought up to date for a pane whose title or cwd comes from its
//! processes, and so does an ask that finds them moved since, on a thread of its own, at most
//! every `PACE` while they go on moving (a build starting compilers); the thread ends when they
//! hold still. What a message changes is known from the message itself (`Move`): a process
//! starting is the pane's new foreground (the youngest), which a pane titled by its foreground
//! process shows, and a process ending changes nothing shown unless it's the listed foreground
//! (its pid, which the message carries). So a pane whose cwd alone is its foreground's (its
//! program names itself but doesn't say its cwd by OSC 7: cmd, Git Bash, a Claude Code session the
//! Conductor started until claudehook's first OSC 7) has the list walked only when its foreground
//! ends, and the statusline and hooks Claude Code starts every few seconds walk nothing; its cwd
//! stays its foreground's while that runs. An ask for the list itself (Lua's foreground process)
//! has it walked when the counts moved. A Claude Code session whose shell or hooks have said its
//! cwd shows neither, so its tool runs start no thread at all. The message for a process ending
//! can come before the job's counts drop (seen 2026-10-31), so an end is taken from the message,
//! never from the counts. A cwd moves without its processes (cmd's `cd`), so for a pane whose
//! shell doesn't say its cwd (OSC 7), an ask for it has the foreground process's cwd read again
//! alone, there and then, one process opened rather than a walk (tens of microseconds, less than
//! a thread made for it costs), when the pane has printed since the last read. A mux server's
//! push is the exception (lbterm_proclist's `Ask::Listed`): it ran at every push of a flooding
//! pane, on the server's main thread with the pane's terminal lock held (perf/mux-client's flood
//! trace, run 6: about 75 us a push, 15% of the busy thread), so a push takes the cwd as listed
//! and has this thread read it, at most every `PACE` while the pane goes on printing and once
//! after it stops, the change raising the alert that has the next push carry it.
//! What a walk or a read finds that consumers show (the foreground process's name, which titles a
//! pane without a title of its own, and its cwd) raises the alert that has them read it again:
//! the window's title and the rail, and a mux server's clients, whose push carries both. So a
//! program started at a prompt that prints nothing still shows in the title as it starts.
//!
//! The job limits nothing: breaking away stays allowed (a program starting one with
//! CREATE_BREAKAWAY_FROM_JOB still can), nothing is killed as it closes, and a program making jobs
//! of its own nests them in it (Windows 8 and later). A process that breaks away, or that the
//! pane's program started before it was put in the job (it's put in as the pane is made, a
//! moment after it starts), isn't counted: its start or end shows at the next that is.

use crate::pane::CachePolicy;
use std::time::Duration;

/// How long after a walk or a read began the next may begin, while the processes go on moving:
/// upstream's PROC_INFO_CACHE_TTL, which a walk on every ask over it was held to
pub const PACE: Duration = Duration::from_millis(300);

/// What an ask for a pane's process list does
#[derive(Debug, PartialEq, Eq)]
pub enum Refresh {
    /// Walk the processes before answering
    Now,
    /// Answer with the list there is, and have it brought up to date on a thread of its own
    Background,
    /// Read the foreground process's cwd again before answering: one process opened, tens of
    /// microseconds, where a thread made for it took about 0.3 ms of a mux server's CPU
    ReadCwd,
    /// The list is as current as a walk would make it, or it's being brought up to date
    No,
}

/// What an ask for the process list does. `listed` is the job's count the list was taken at
/// (`None`: no list yet; for an ask for the cwd alone, `Some(None)`: its foreground's end comes as
/// its job's message, and other processes moving don't change it), `counted` the count now
/// (`None`: the pane has no job, which was said as it began), `refreshing` whether its thread is
/// at it already (which reads a cwd asked for meanwhile once it's done), `cwd_stale` whether the
/// ask wants the cwd and the pane printed since it was read.
pub fn refresh(
    policy: CachePolicy,
    listed: Option<Option<u64>>,
    counted: Option<u64>,
    refreshing: bool,
    cwd_stale: bool,
) -> Refresh {
    match (policy, listed) {
        (CachePolicy::FetchImmediate, _) | (_, None) => Refresh::Now,
        _ if refreshing => Refresh::No,
        (_, Some(listed)) if moved(listed, counted) => Refresh::Background,
        _ if cwd_stale => Refresh::ReadCwd,
        _ => Refresh::No,
    }
}

/// Whether the pane's processes started or ended since a list taken at the job's count `listed`
/// (`counted` now); a pane without a job never says so
pub fn moved(listed: Option<u64>, counted: Option<u64>) -> bool {
    matches!((listed, counted), (Some(listed), Some(counted)) if listed != counted)
}

/// What the thread bringing a list up to date does next
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// A walk is wanted: a process started in a pane titled by its foreground, the listed
    /// foreground ended, or something asked for the list itself and found the processes moved
    Walk,
    /// Only the cwd was asked for, and the pane printed since it was read: read it alone
    ReadCwd,
    /// Nothing: the thread ends
    Done,
}

/// `walk`: a walk is wanted; `cwd_stale`: the cwd was asked for and the pane printed since it
/// was read
pub fn next(walk: bool, cwd_stale: bool) -> Next {
    if walk {
        Next::Walk
    } else if cwd_stale {
        Next::ReadCwd
    } else {
        Next::Done
    }
}

/// What a job's message says moved
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// The process with this pid started in it: the pane's youngest, its foreground
    Started(u32),
    /// The process with this pid ended
    Ended(u32),
    /// Its last process ended: the pane's program is gone
    Empty,
}

/// Whether a pane shown as `shows_name` (titled by its foreground process) and `shows_cwd` (its
/// cwd its foreground's), its listed foreground `foreground`, wants a walk for `moved`: a process
/// started is its new foreground, which changes its name; the foreground ending changes both; any
/// other process ending changes neither
pub fn walks_for(moved: Move, foreground: Option<u32>, shows_name: bool, shows_cwd: bool) -> bool {
    match moved {
        Move::Started(_) => shows_name,
        Move::Ended(pid) => (shows_name || shows_cwd) && foreground == Some(pid),
        Move::Empty => false,
    }
}

/// How long the thread waits before its next walk or read: what's left of `PACE` since the last
/// began (`None`: none yet)
pub fn wait(since_last: Option<Duration>) -> Duration {
    since_last.map_or(Duration::ZERO, |since| PACE.saturating_sub(since))
}

/// The job's counts as one number: processes started, and running. Only the first goes up and,
/// while it stays, only the second down, so the two together never come back to what they were
/// and move exactly when a process starts or ends.
pub fn count(started: u32, running: u32) -> u64 {
    (started as u64) << 32 | running as u64
}

#[cfg(windows)]
pub use self::windows::{running, wait_ended, Watch};

/// Whether the process `pid` still runs: everywhere but Windows nothing has a job, so nothing
/// asks
#[cfg(not(windows))]
pub fn running(_pid: u32) -> bool {
    true
}

/// Waits for the process `pid` to have ended: everywhere but Windows nothing has a job to say it
/// ends
#[cfg(not(windows))]
pub fn wait_ended(_pid: u32, _max: Duration) {}

/// What a job's message `message` says moved, its pid `pid` (the message's overlapped pointer):
/// JOB_OBJECT_MSG_NEW_PROCESS, _EXIT_PROCESS and _ABNORMAL_EXIT_PROCESS, and
/// _ACTIVE_PROCESS_ZERO; the rest are limits, which it sets none of
pub fn says_moved(message: u32, pid: u32) -> Option<Move> {
    match message {
        6 => Some(Move::Started(pid)),
        7 | 8 => Some(Move::Ended(pid)),
        4 => Some(Move::Empty),
        _ => None,
    }
}

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::io::{Error, Result};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    /// JobObjectBasicAccountingInformation
    const BASIC_ACCOUNTING: i32 = 1;
    /// JobObjectAssociateCompletionPortInformation
    const ASSOCIATE_PORT: i32 = 7;
    /// JobObjectExtendedLimitInformation
    const EXTENDED_LIMITS: i32 = 9;
    /// JOB_OBJECT_LIMIT_BREAKAWAY_OK
    const BREAKAWAY_OK: u32 = 0x800;
    const INFINITE: u32 = u32::MAX;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x102;
    const ERROR_ACCESS_DENIED: i32 = 5;

    /// JOBOBJECT_ASSOCIATE_COMPLETION_PORT
    #[repr(C)]
    struct AssociatePort {
        key: *mut c_void,
        port: RawHandle,
    }

    /// JOBOBJECT_BASIC_ACCOUNTING_INFORMATION
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct Accounting {
        total_user_time: i64,
        total_kernel_time: i64,
        this_period_total_user_time: i64,
        this_period_total_kernel_time: i64,
        total_page_fault_count: u32,
        pub(super) total_processes: u32,
        pub(super) active_processes: u32,
        total_terminated_processes: u32,
    }

    /// JOBOBJECT_BASIC_LIMIT_INFORMATION
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct BasicLimits {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    /// JOBOBJECT_EXTENDED_LIMIT_INFORMATION, its IO_COUNTERS as six counts
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> RawHandle;
        fn SetInformationJobObject(
            job: RawHandle,
            class: i32,
            info: *const c_void,
            len: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
        fn QueryInformationJobObject(
            job: RawHandle,
            class: i32,
            info: *mut c_void,
            len: u32,
            returned: *mut u32,
        ) -> i32;
        fn CreateIoCompletionPort(
            file: RawHandle,
            existing: RawHandle,
            key: usize,
            threads: u32,
        ) -> RawHandle;
        fn GetQueuedCompletionStatus(
            port: RawHandle,
            bytes: *mut u32,
            key: *mut usize,
            overlapped: *mut *mut c_void,
            ms: u32,
        ) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> RawHandle;
        fn WaitForSingleObject(handle: RawHandle, ms: u32) -> u32;
    }

    /// Whether the process `pid` still runs: its handle not yet signalled. One that has ended
    /// while another process still holds its handle (its parent, which waits on it) opens but is
    /// signalled; one whose handles are all closed doesn't open. A process LBTerm may not open
    /// (an elevated one) is said to run, as its cwd can't be read either: its end is heard when
    /// the job's count next moves for a process LBTerm may open, or by a walk
    pub fn running(pid: u32) -> bool {
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return Error::last_os_error().raw_os_error() == Some(ERROR_ACCESS_DENIED);
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }

    /// Waits up to `max` for the process `pid` to have ended. Its job says it ends a moment
    /// before Windows has taken it out of the list of processes, which a walk at once still
    /// finds it in (2026-10-31: every walk 1 ms after ping's end found it, and one 300 ms on
    /// didn't); once its handle is signalled it's gone from the list
    pub fn wait_ended(pid: u32, max: std::time::Duration) {
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return;
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        let ms = max.as_millis().min(u32::MAX as u128 - 1) as u32;
        unsafe { WaitForSingleObject(handle.as_raw_handle(), ms) };
    }

    /// What a job's message calls, by the key its job tells the port
    type Moved = Arc<dyn Fn(super::Move) + Send + Sync>;
    static MOVED: Mutex<Option<HashMap<usize, Moved>>> = Mutex::new(None);
    static KEYS: AtomicUsize = AtomicUsize::new(1);
    /// The completion port every pane's job tells, made with its thread by the first job
    static PORT: OnceLock<std::result::Result<OwnedHandle, String>> = OnceLock::new();

    fn port() -> Result<RawHandle> {
        let port = PORT.get_or_init(|| {
            let port =
                unsafe { CreateIoCompletionPort(-1isize as RawHandle, std::ptr::null_mut(), 0, 1) };
            if port.is_null() {
                return Err(Error::last_os_error().to_string());
            }
            let port = unsafe { OwnedHandle::from_raw_handle(port) };
            let raw = port.as_raw_handle() as usize;
            std::thread::Builder::new()
                .name("lbterm-jobs".into())
                .spawn(move || listen(raw as RawHandle))
                .map_err(|err| err.to_string())?;
            Ok(port)
        });
        match port {
            Ok(port) => Ok(port.as_raw_handle()),
            Err(err) => Err(Error::other(err.clone())),
        }
    }

    /// The `lbterm-jobs` thread: asleep on the port, but for a message from a pane's job
    fn listen(port: RawHandle) {
        loop {
            let (mut message, mut key, mut overlapped) = (0u32, 0usize, std::ptr::null_mut());
            let ok = unsafe {
                GetQueuedCompletionStatus(port, &mut message, &mut key, &mut overlapped, INFINITE)
            };
            if ok == 0 {
                log::error!(
                    "the panes' jobs can't be heard any more, so a program starting or ending \
                     in one shows in its title only when something next asks: {}",
                    Error::last_os_error()
                );
                return;
            }
            if let Some(what) = super::says_moved(message, overlapped as usize as u32) {
                let moved = MOVED
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(|keys| keys.get(&key).cloned());
                if let Some(moved) = moved {
                    moved(what);
                }
            }
        }
    }

    /// A pane's processes, as a job of their own (the module's doc)
    pub struct Watch {
        job: OwnedHandle,
        key: usize,
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            if let Some(keys) = MOVED.lock().unwrap().as_mut() {
                keys.remove(&self.key);
            }
        }
    }

    impl Watch {
        /// A job for a pane's processes, with `process` (its program, just started) put in it;
        /// `moved` is called on the `lbterm-jobs` thread each time one of them starts or ends,
        /// saying which (`Move`)
        pub fn new(process: RawHandle, moved: Moved) -> Result<Watch> {
            let port = port()?;
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(Error::last_os_error());
            }
            let key = KEYS.fetch_add(1, Ordering::Relaxed);
            MOVED
                .lock()
                .unwrap()
                .get_or_insert_with(HashMap::new)
                .insert(key, moved);
            // Its Drop forgets the key, whatever fails below
            let watch = Watch {
                job: unsafe { OwnedHandle::from_raw_handle(job) },
                key,
            };
            let job = watch.job.as_raw_handle();
            let tell = AssociatePort {
                key: key as *mut c_void,
                port,
            };
            let told = unsafe {
                SetInformationJobObject(
                    job,
                    ASSOCIATE_PORT,
                    &tell as *const AssociatePort as *const c_void,
                    std::mem::size_of::<AssociatePort>() as u32,
                )
            };
            if told == 0 {
                return Err(Error::last_os_error());
            }
            let mut limits = ExtendedLimits::default();
            limits.basic.limit_flags = BREAKAWAY_OK;
            let set = unsafe {
                SetInformationJobObject(
                    job,
                    EXTENDED_LIMITS,
                    &limits as *const ExtendedLimits as *const c_void,
                    std::mem::size_of::<ExtendedLimits>() as u32,
                )
            };
            if set == 0 {
                return Err(Error::last_os_error());
            }
            if unsafe { AssignProcessToJobObject(job, process) } == 0 {
                return Err(Error::last_os_error());
            }
            Ok(watch)
        }

        /// The job's counts (`super::count`), a microsecond's call
        pub fn count(&self) -> Option<u64> {
            let mut info = Accounting::default();
            let ok = unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle(),
                    BASIC_ACCOUNTING,
                    &mut info as *mut Accounting as *mut c_void,
                    std::mem::size_of::<Accounting>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then(|| super::count(info.total_processes, info.active_processes))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const ASK: CachePolicy = CachePolicy::AllowStale;

    #[test]
    fn a_pane_with_no_list_yet_waits_for_the_walk() {
        for refreshing in [false, true] {
            assert_eq!(refresh(ASK, None, Some(1), refreshing, false), Refresh::Now);
            assert_eq!(refresh(ASK, None, None, refreshing, true), Refresh::Now);
        }
    }

    #[test]
    fn fetch_immediate_always_waits_for_the_walk() {
        for listed in [None, Some(None), Some(Some(1)), Some(Some(2))] {
            for refreshing in [false, true] {
                for cwd_stale in [false, true] {
                    assert_eq!(
                        refresh(
                            CachePolicy::FetchImmediate,
                            listed,
                            Some(2),
                            refreshing,
                            cwd_stale
                        ),
                        Refresh::Now
                    );
                }
            }
        }
    }

    #[test]
    fn output_alone_walks_nothing() {
        // A key's echo at a prompt: the pane printed, its processes didn't move
        assert_eq!(
            refresh(ASK, Some(Some(7)), Some(7), false, false),
            Refresh::No
        );
    }

    #[test]
    fn a_process_started_or_ended_has_the_list_brought_up_to_date_in_the_background() {
        let at_prompt = count(2, 1);
        let program_running = count(3, 2);
        let program_ended = count(3, 1);
        assert_eq!(
            refresh(
                ASK,
                Some(Some(at_prompt)),
                Some(program_running),
                false,
                false
            ),
            Refresh::Background
        );
        assert_eq!(
            refresh(
                ASK,
                Some(Some(program_running)),
                Some(program_ended),
                false,
                false
            ),
            Refresh::Background
        );
    }

    #[test]
    fn a_cwd_asked_for_after_output_is_read_again_alone() {
        assert_eq!(
            refresh(ASK, Some(Some(7)), Some(7), false, true),
            Refresh::ReadCwd
        );
        // A pane without a job reads its cwd again too: that needs no job
        assert_eq!(
            refresh(ASK, Some(None), None, false, true),
            Refresh::ReadCwd
        );
        // The processes moved as well: the walk reads it with them
        assert_eq!(
            refresh(ASK, Some(Some(7)), Some(8), false, true),
            Refresh::Background
        );
    }

    #[test]
    fn one_refresh_at_a_time() {
        // The thread at it reads a cwd asked for meanwhile once it's done (cwd_asked)
        for counted in [Some(1), Some(2)] {
            assert_eq!(
                refresh(ASK, Some(Some(1)), counted, true, true),
                Refresh::No
            );
        }
    }

    #[test]
    fn a_pane_without_a_job_never_walks_again_unasked() {
        // Its job couldn't be made, said as it began: only FetchImmediate walks it
        assert!(!moved(None, None));
        assert!(!moved(Some(1), None));
        assert_eq!(refresh(ASK, Some(None), None, false, false), Refresh::No);
    }

    #[test]
    fn the_counts_move_with_every_start_and_end_and_never_come_back() {
        // pwsh alone, then a program it starts, which starts one of its own, both ending
        let seen = [
            count(1, 1),
            count(2, 2),
            count(3, 3),
            count(3, 2),
            count(3, 1),
            count(4, 2),
            count(4, 1),
        ];
        for (i, a) in seen.iter().enumerate() {
            for b in &seen[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_thread_walks_when_a_walk_is_wanted_else_reads_the_cwd_else_ends() {
        assert_eq!(next(true, false), Next::Walk);
        assert_eq!(next(true, true), Next::Walk);
        assert_eq!(next(false, true), Next::ReadCwd);
        assert_eq!(next(false, false), Next::Done);
    }

    #[test]
    fn a_cwd_ask_needs_no_walk_for_other_processes_moving() {
        // An ask for the cwd alone goes by no count (Some(None)): its foreground's end is a
        // message; read alone after output, or nothing
        assert_eq!(refresh(ASK, Some(None), Some(9), false, false), Refresh::No);
        assert_eq!(
            refresh(ASK, Some(None), Some(9), false, true),
            Refresh::ReadCwd
        );
    }

    #[test]
    fn a_process_starting_walks_a_pane_titled_by_it_and_only_the_foregrounds_end_walks_the_rest() {
        let (fg, other) = (Some(40), 41);
        // Titled by its foreground: a new process is its new foreground
        assert!(walks_for(Move::Started(other), fg, true, false));
        assert!(walks_for(Move::Started(other), fg, true, true));
        // Its cwd alone shown (cmd, a Claude Code session before its first OSC 7): a program it
        // starts leaves its foreground and cwd as they are
        assert!(!walks_for(Move::Started(other), fg, false, true));
        // Another process ending changes nothing shown; the foreground's end changes both
        for (name, cwd) in [(true, false), (false, true), (true, true)] {
            assert!(!walks_for(Move::Ended(other), fg, name, cwd));
            assert!(walks_for(Move::Ended(40), fg, name, cwd));
        }
        // Nothing shown, or the pane's program gone: nothing
        assert!(!walks_for(Move::Started(other), fg, false, false));
        assert!(!walks_for(Move::Ended(40), fg, false, false));
        assert!(!walks_for(Move::Empty, fg, true, true));
        // No list yet: no foreground to end
        assert!(!walks_for(Move::Ended(40), None, true, true));
    }

    #[test]
    fn walks_and_reads_are_paced() {
        assert_eq!(wait(None), Duration::ZERO);
        assert_eq!(
            wait(Some(Duration::from_millis(100))),
            Duration::from_millis(200)
        );
        assert_eq!(wait(Some(PACE)), Duration::ZERO);
        assert_eq!(wait(Some(Duration::from_secs(5))), Duration::ZERO);
    }

    #[test]
    fn only_a_process_starting_or_ending_says_the_processes_moved() {
        // NEW_PROCESS, EXIT_PROCESS, ABNORMAL_EXIT_PROCESS, ACTIVE_PROCESS_ZERO
        assert_eq!(says_moved(6, 7), Some(Move::Started(7)));
        assert_eq!(says_moved(7, 7), Some(Move::Ended(7)));
        assert_eq!(says_moved(8, 7), Some(Move::Ended(7)));
        assert_eq!(says_moved(4, 0), Some(Move::Empty));
        // Limits the job never sets: end of job and process time, the active process limit,
        // memory limits, notification and cycle time limits, a silo's end
        for message in [1, 2, 3, 9, 10, 11, 12, 13] {
            assert_eq!(says_moved(message, 7), None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_job_structures_are_windows_sizes() {
        assert_eq!(std::mem::size_of::<windows::Accounting>(), 48);
        assert_eq!(std::mem::size_of::<windows::BasicLimits>(), 64);
        assert_eq!(std::mem::size_of::<windows::ExtendedLimits>(), 144);
    }

    /// A program started in the job, and one it starts, move its counts as each starts and ends,
    /// and the job says so on the port as they do
    #[cfg(windows)]
    #[test]
    fn a_job_counts_a_programs_processes_as_they_start_and_end_and_says_so() {
        use std::io::Write;
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Instant;
        // CREATE_NO_WINDOW: a console of its own that nobody sees, whatever the test runs under
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "set /p x= & ping -n 2 127.0.0.1 > nul"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let said = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&said);
        let watch = Watch::new(
            child.as_raw_handle(),
            Arc::new(move |_: Move| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
        let first = watch.count().unwrap();
        // The port says what the job had as it was told (cmd itself) too: wait that out
        let until = Instant::now() + Duration::from_secs(5);
        while said.load(Ordering::SeqCst) == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(100));
        let said_first = said.load(Ordering::SeqCst);
        // cmd waits on its stdin until it's told to go on, so nothing it starts is missed
        child.stdin.take().unwrap().write_all(b"go\r\n").unwrap();
        let mut seen = vec![first];
        let until = Instant::now() + Duration::from_secs(20);
        while child.try_wait().unwrap().is_none() && Instant::now() < until {
            let now = watch.count().unwrap();
            if Some(&now) != seen.last() {
                seen.push(now);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        child.wait().unwrap();
        let started = |c: u64| (c >> 32) as u32;
        let running = |c: u64| c as u32;
        // What ends with it (its console's host, were it in the job) may take a moment more
        let until = Instant::now() + Duration::from_secs(5);
        while running(watch.count().unwrap()) > 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        seen.push(watch.count().unwrap());
        // ping started under cmd: one more started than at first, and cmd's end leaves none
        assert!(
            started(*seen.last().unwrap()) > started(first),
            "counts seen: {:x?}",
            seen
        );
        assert_eq!(running(*seen.last().unwrap()), 0, "counts seen: {seen:x?}");
        assert!(moved(Some(first), watch.count()));
        // ping's start and end, then cmd's end: three at least, as they happened
        let until = Instant::now() + Duration::from_secs(5);
        while said.load(Ordering::SeqCst) < said_first + 3 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            said.load(Ordering::SeqCst) >= said_first + 3,
            "the port said {} times after cmd went on, {said_first} before; counts seen: {seen:x?}",
            said.load(Ordering::SeqCst) - said_first
        );
        // A watch gone says nothing more
        drop(watch);
    }

    /// A process that has ended doesn't run, though its handle is still held (as its parent's is
    /// while it waits on it); one waiting at its prompt does
    #[cfg(windows)]
    #[test]
    fn a_process_that_ended_doesnt_run_while_its_handle_is_held() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut waiting = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "set /p x="])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        assert!(running(waiting.id()));
        let mut ended = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "exit 3"])
            .stdout(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        ended.wait().unwrap();
        // `ended` still holds its handle here
        assert!(!running(ended.id()));
        // Waited for at once once it has ended; up to the wait while it runs
        let began = std::time::Instant::now();
        wait_ended(ended.id(), Duration::from_secs(5));
        assert!(began.elapsed() < Duration::from_secs(1));
        let began = std::time::Instant::now();
        wait_ended(waiting.id(), Duration::from_millis(100));
        assert!(began.elapsed() >= Duration::from_millis(90));
        assert!(running(waiting.id()));
        drop(waiting.stdin.take());
        waiting.wait().unwrap();
        assert!(!running(waiting.id()));
    }

    /// What a pane's cwd read alone costs against a walk of its processes, both of a program
    /// waiting at its prompt: the read opens one process, the walk snapshots every process on
    /// the PC (about 18 ms here, lbterm/PERF.md finding 49), so the read is many times cheaper
    #[cfg(windows)]
    #[test]
    fn a_cwd_read_alone_costs_a_small_part_of_a_walk() {
        use procinfo::LocalProcessInfo;
        use std::os::windows::process::CommandExt;
        use std::time::Instant;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        use std::io::Read;
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/d", "/c"])
            .raw_arg("echo ready& set /p x=")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let pid = child.id();
        // A process just started can't be read until its loader has made its parameters
        // pointers (RtlNormalizeProcessParams), and a read while cmd is still starting can find
        // its cwd and the next one not (the check, 2026-10-31, the first two reads a moment
        // apart): so the reads start once cmd has said it's running its command, at its prompt
        let mut out = child.stdout.take().unwrap();
        let mut ready = [0u8; 5];
        out.read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"ready");
        assert!(LocalProcessInfo::current_working_dir(pid).is_some());
        let reads = 200;
        let began = Instant::now();
        for _ in 0..reads {
            assert!(LocalProcessInfo::current_working_dir(pid).is_some());
        }
        let read = began.elapsed() / reads;
        let walks = 5;
        let began = Instant::now();
        for _ in 0..walks {
            assert!(LocalProcessInfo::with_root_pid(pid).is_some());
        }
        let walk = began.elapsed() / walks;
        drop(child.stdin.take());
        child.wait().unwrap();
        drop(out);
        println!("a cwd read {:?}, a walk {:?}", read, walk);
        assert!(read * 5 < walk, "a cwd read {:?}, a walk {:?}", read, walk);
    }
}
