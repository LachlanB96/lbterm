//! LBTerm: a write to a pane that didn't get there, told to the GUI.
//!
//! A pane's `writer()` is how a window writes to it rather than send it a key (a binding's
//! `SendString`, a voice draft, the scroll strip's keys, an IME's text, Lua's `pane:send_text`).
//! On a mux pane the write is sent from a task and not waited for (wezterm-client's
//! lbterm_link.rs), so a failure comes after the caller has gone on: whoever hears it calls
//! [`failed`], and the GUI shows it on the pane's scroll strip (Lachlan's 15b of 2026-11-07;
//! wezterm-gui's render/scroll_strip.rs, `lbterm_scroll_write_failed`).
//!
//! A hook, not a `MuxNotification`: a new variant would be an arm in every exhaustive match of
//! upstream's (the GUI's two, its front end, the server's dispatch, the client domain's, the mux
//! server's main), and only the GUI has a use for it. A process without a GUI sets none, and a
//! failure there is only logged, by whoever heard it.

use crate::pane::PaneId;

/// Whether a write that failed may still have reached the pane: the strip heads it "Not sent" or
/// "May not have arrived" (Lachlan's 4a of 2026-11-08)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached {
    /// It never went, or the server said no
    No,
    /// The connection ended before the server answered: it may have got there
    Maybe,
}

/// What hears a failed write: the pane (the window's own id for it), why, in words for Lachlan,
/// and whether it may have arrived anyway
pub type Hook = fn(pane: PaneId, why: String, reached: Reached);

static HOOK: std::sync::OnceLock<Hook> = std::sync::OnceLock::new();

/// Tell `hook` of every failed write from now on (the GUI, once, as it starts)
pub fn on_failed(hook: Hook) {
    let _ = HOOK.set(hook);
}

/// A write to `pane` didn't get there, for `why`, or may not have
pub fn failed(pane: PaneId, why: String, reached: Reached) {
    if let Some(hook) = HOOK.get() {
        hook(pane, why, reached);
    }
}
