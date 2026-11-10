//! Losing the GPU device (lbterm/GPU-LOST.md): which of wgpu's errors mean the device has gone,
//! and when to try building the GPU state again.

use std::time::Duration;

/// Whether an error's text says the device went. wgpu passes DXGI's on: DEVICE_REMOVED,
/// DEVICE_HUNG and DEVICE_RESET (0x887A0005-7), or its own "device lost".
pub fn removed_message(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "device lost",
        "device is lost",
        "device_removed",
        "device removed",
        "device_hung",
        "device_reset",
        "0x887a0005",
        "0x887a0006",
        "0x887a0007",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// How many tries at building the GPU state again are logged one by one. The window keeps
/// trying after them, until the GPU answers (a driver install takes longer than a reset), and
/// says so once every TRIES tries
pub const TRIES: usize = 30;

/// Whether try `n` (from 1) gets a line of its own in the log
pub fn says(n: usize) -> bool {
    n <= TRIES || n % TRIES == 0
}

/// Whether a GPU state built again may be kept: never one on Windows' software renderer (WARP,
/// wgpu's DeviceType::Cpu) unless the config asks for it. While the GPU is still resetting, or
/// its driver being replaced, the hardware adapter doesn't answer and wgpu hands back WARP, which
/// draws on the CPU (about 30 cores at a time, ITMan's incidents of 2026-11-04 and 2026-11-07)
/// and nothing would move the window back once the GPU returned
pub fn takes(software: bool, software_asked: bool) -> bool {
    !software || software_asked
}

/// How long to wait before try `n` (from 1): half a second for the driver to come back, then
/// a second between tries
pub fn wait_before(n: usize) -> Duration {
    if n <= 1 {
        Duration::from_millis(500)
    } else {
        Duration::from_secs(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_removed_errors_are_recognised() {
        assert!(removed_message("Parent device is lost"));
        assert!(removed_message("DXGI_ERROR_DEVICE_REMOVED (0x887A0005)"));
        assert!(removed_message("Device hung: 0x887a0006"));
        assert!(!removed_message("Validation Error: buffer too small"));
        assert!(!removed_message("Out of memory"));
    }

    #[test]
    fn half_a_second_then_a_second_each() {
        assert_eq!(wait_before(1), Duration::from_millis(500));
        assert_eq!(wait_before(2), Duration::from_secs(1));
        assert_eq!(wait_before(TRIES), Duration::from_secs(1));
        // thirty tries is about half a minute
        let total: Duration = (1..=TRIES).map(wait_before).sum();
        assert!(total >= Duration::from_secs(29) && total <= Duration::from_secs(31));
        // and on, a second a try, until the GPU answers
        assert_eq!(wait_before(TRIES * 10), Duration::from_secs(1));
    }

    #[test]
    fn every_try_logged_at_first_then_one_in_thirty() {
        assert!((1..=TRIES).all(says));
        assert!(!says(TRIES + 1));
        assert!(!says(2 * TRIES - 1));
        assert!(says(2 * TRIES));
        assert_eq!((1..=10 * TRIES).filter(|&n| says(n)).count(), TRIES + 9);
    }

    #[test]
    fn the_software_renderer_only_when_asked() {
        // the hardware adapter: kept
        assert!(takes(false, false));
        // WARP while the GPU resets: tried again
        assert!(!takes(true, false));
        // webgpu_force_fallback_adapter, or a preferred adapter that is the CPU: kept
        assert!(takes(true, true));
    }
}
