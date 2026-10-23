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

/// How many times to try building the GPU state again before giving up
pub const TRIES: usize = 30;

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
    }
}
