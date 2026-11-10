//! LBTerm: telling a program typing into a pane from Lachlan. The GUI and the cli are both
//! clients of a mux, and both write to panes the same ways (WriteToPane, SendPaste). A window
//! says who it is as it attaches (SetClientId, in wezterm-client's `verify_version_compat`); the
//! cli never does. So a connection that types without having said who it is is `lbterm cli
//! send-text` (the Conductor, Relay answering a question, the Claude project) or a script like it. What
//! it types marks the pane with the `lbterm_typed` user var, which the tab rail answers with a
//! glint on the tab's pad (wezterm-gui's render/tab_panel.rs).

use crate::client::ClientId;

/// The user var the mark is: the time it typed, in Unix milliseconds
pub const USER_VAR: &str = "lbterm_typed";

/// Whether what a connection types is a program's: it never said who it is (`client`)
pub fn by_a_program(client: Option<&ClientId>) -> bool {
    client.is_none()
}

/// Whether a connection gets a pane's changes pushed as they come (its lines, its alerts): a
/// window does, since it draws the pane; a program's connection only for a pane it `asked` about.
/// Every connection used to get every pane's, each `lbterm cli` call too, so a busy pane's whole
/// screen was worked out on the server's main thread and sent to calls that drop it
/// (2026-10-29)
pub fn gets_changes(client: Option<&ClientId>, asked: bool) -> bool {
    !by_a_program(client) || asked
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_window_says_who_it_is_and_the_cli_doesnt() {
        assert!(by_a_program(None));
        assert!(!by_a_program(Some(&ClientId::new())));
    }

    #[test]
    fn only_a_window_or_who_asked_gets_a_panes_changes() {
        assert!(gets_changes(Some(&ClientId::new()), false));
        assert!(!gets_changes(None, false));
        assert!(gets_changes(None, true));
    }
}
