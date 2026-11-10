//! LBTerm: where this test binary crashed, said before it dies. An access violation in a test thread
//! ends the whole binary with 0xC0000005 and nothing else (no panic, no test's name), which is how
//! wezterm-gui's tests died about one run in six once the check ran them (2026-11-10). So a
//! vectored exception handler, put in before main by the C runtime's initialisers, prints the
//! faulting thread's name and stack for the first few access violations it sees. It's a
//! first-chance handler: a fault some code then handles itself would print too, so the stack
//! says whose it was. Tests only; LBTerm itself has none of it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut core::ffi::c_void,
    parameters: u32,
    information: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut core::ffi::c_void,
}

#[link(name = "kernel32")]
extern "system" {
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
    ) -> *mut core::ffi::c_void;
}

const ACCESS_VIOLATION: u32 = 0xC000_0005;
/// The most faults it prints: a fault some code handles itself, then many more, says no more
const SAID_AT_MOST: usize = 4;
static SAID: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn on_exception(pointers: *mut ExceptionPointers) -> i32 {
    let record = &*(*pointers).record;
    if record.code == ACCESS_VIOLATION && SAID.fetch_add(1, Ordering::Relaxed) < SAID_AT_MOST {
        let what = match record.information[0] {
            0 => "reading",
            1 => "writing",
            8 => "running",
            _ => "touching",
        };
        let thread = std::thread::current();
        let said = format!(
            "\nACCESS VIOLATION at {:?}, {what} {:#x}, on thread {}\n{}",
            record.address,
            record.information[1],
            thread.name().unwrap_or("(unnamed)"),
            std::backtrace::Backtrace::force_capture()
        );
        // Straight to the handle: eprintln! in a test's thread goes to libtest's capture of that
        // test's output, which dies with the binary
        use std::io::Write;
        let _ = std::io::stderr().lock().write_all(said.as_bytes());
        let _ = std::io::stderr().lock().write_all(b"\n");
    }
    // EXCEPTION_CONTINUE_SEARCH: what would have happened goes on happening
    0
}

/// Whether the handler went in: the C runtime ran `install`
static INSTALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn install() {
    let handle = unsafe { AddVectoredExceptionHandler(1, on_exception) };
    INSTALLED.store(!handle.is_null(), Ordering::Relaxed);
}

/// Run by the C runtime before main, as its initialisers in .CRT$XCU are
#[used]
#[link_section = ".CRT$XCU"]
static INSTALL: extern "C" fn() = install;

#[test]
fn the_handler_is_in_before_any_test_runs() {
    assert!(INSTALLED.load(Ordering::Relaxed), "the C runtime didn't run test_crash's initialiser");
}
