//! LBTerm: a pane's output is coalesced for the time it's set to, not to Windows' next tick.
//!
//! The parser thread holds output back for `mux_output_parser_coalesce_delay_ms` (3 ms) after it
//! last read, so that a program drawing a frame in several writes shows it whole, by polling the
//! pane's socket with that timeout. On Windows the poll (WSAPoll) is a wait like any other, and
//! in a process that hasn't raised the timer's resolution (no LBTerm process does: it would raise
//! it for the whole PC) a 3 ms wait ends on the system's 15.6 ms tick: on 2026-10-30, 15.2 to 16.4
//! ms for a 3 ms WSAPoll or event wait, with the PC's timer at 0.5 ms for another program. So
//! every key's echo, and the last of every burst of output, waited about 15 ms in the parser
//! before it reached the terminal, in the mux server and in a window's own panes alike.
//!
//! [`Coalesce::wait`] waits on a high-resolution waitable timer instead
//! (CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, as the GUI's frame pacing does, window/src/os/windows/
//! pace.rs), which ends on time without raising anything for anyone else, and then asks the
//! socket without waiting whether more came. Output that came during the wait is read then
//! rather than as it came, and flushed at the same deadline as before.

use std::time::Duration;

/// The parser thread's wait for more output
pub struct Coalesce {
    #[cfg(windows)]
    timer: windows::Timer,
}

impl Coalesce {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            #[cfg(windows)]
            timer: windows::Timer::new()?,
        })
    }

    /// Waits `delay` for more of the pane's output: whether there's some to read now.
    /// `readable_now` asks the socket without waiting.
    pub fn wait(&self, delay: Duration, mut readable_now: impl FnMut() -> bool) -> bool {
        if readable_now() {
            return true;
        }
        if delay.is_zero() {
            return false;
        }
        #[cfg(windows)]
        self.timer.sleep(delay);
        #[cfg(not(windows))]
        std::thread::sleep(delay);
        readable_now()
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::io::Error;
    use std::ptr::{null, null_mut};
    use std::time::Duration;

    type Handle = *mut c_void;

    /// CreateWaitableTimerExW's flag for a timer that ends on time whatever the system's timer
    /// resolution (Windows 10 1803 and later)
    const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;
    const TIMER_MODIFY_STATE: u32 = 0x0002;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const WAIT_OBJECT_0: u32 = 0;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateWaitableTimerExW(
            attributes: *const c_void,
            name: *const u16,
            flags: u32,
            access: u32,
        ) -> Handle;
        fn SetWaitableTimerEx(
            timer: Handle,
            due: *const i64,
            period: i32,
            completion: *const c_void,
            arg: *const c_void,
            wake_context: *const c_void,
            tolerable_delay: u32,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, ms: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub struct Timer(Handle);

    // The handle is the thread's own; it's only made on one thread and used there
    unsafe impl Send for Timer {}

    impl Timer {
        pub fn new() -> anyhow::Result<Self> {
            let h = unsafe {
                CreateWaitableTimerExW(
                    null(),
                    null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_MODIFY_STATE | SYNCHRONIZE,
                )
            };
            if h.is_null() {
                anyhow::bail!(
                    "the output parser's high-resolution waitable timer couldn't be made: {}",
                    Error::last_os_error()
                );
            }
            Ok(Self(h))
        }

        /// Sleeps for `delay`, ending on time
        pub fn sleep(&self, delay: Duration) {
            // Relative, in 100 ns units
            let due = -((delay.as_nanos() / 100).max(1) as i64);
            let set = unsafe { SetWaitableTimerEx(self.0, &due, 0, null(), null(), null_mut(), 0) };
            if set == 0 {
                log::error!(
                    "the output parser's waitable timer couldn't be set: {}",
                    Error::last_os_error()
                );
                return;
            }
            let got = unsafe { WaitForSingleObject(self.0, INFINITE) };
            if got != WAIT_OBJECT_0 {
                log::error!(
                    "waiting on the output parser's timer failed ({got:#x}): {}",
                    Error::last_os_error()
                );
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Instant;

    #[test]
    fn readable_at_once_doesnt_wait() {
        let c = Coalesce::new().unwrap();
        let at = Instant::now();
        assert!(c.wait(Duration::from_millis(50), || true));
        assert!(at.elapsed() < Duration::from_millis(20));
    }

    #[test]
    fn nothing_more_waits_the_delay_and_asks_again() {
        let c = Coalesce::new().unwrap();
        let mut asked = 0;
        assert!(!c.wait(Duration::from_millis(3), || {
            asked += 1;
            false
        }));
        assert_eq!(asked, 2);
        let mut asked = 0;
        assert!(c.wait(Duration::from_millis(3), || {
            asked += 1;
            asked == 2
        }));
    }

    /// The wait ends on time, not on Windows' 15.6 ms tick, in a process (this test's) that never
    /// raised the timer's resolution. A 3 ms wait each of 40 times: the median under 10 ms, where
    /// an ordinary wait's was 15 to 16 (2026-10-30)
    #[test]
    fn a_3_ms_wait_takes_3_ms() {
        let c = Coalesce::new().unwrap();
        let mut took: Vec<Duration> = (0..40)
            .map(|_| {
                let at = Instant::now();
                c.wait(Duration::from_millis(3), || false);
                at.elapsed()
            })
            .collect();
        took.sort();
        let median = took[took.len() / 2];
        assert!(median >= Duration::from_millis(3), "{:?}", took);
        assert!(median < Duration::from_millis(10), "{:?}", took);
    }
}
