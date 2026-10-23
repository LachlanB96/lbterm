//! LBTerm: telling a program typing into a pane from Lachlan. The GUI and the cli are both
//! clients of a mux, and both write to panes the same ways (WriteToPane, SendPaste). A window
//! says who it is as it attaches (SetClientId, in wezterm-client's `verify_version_compat`); the
//! cli never does. So a connection that types without having said who it is is `lbterm cli
//! send-text` (the Conductor, Relay answering a question, AI Dashboard) or a script like it. What
//! it types marks the pane with the `lbterm_typed` user var, which the tab rail answers with a
//! glint on the tab's pad (wezterm-gui's render/tab_panel.rs).

use crate::client::ClientId;

/// The user var the mark is: the time it typed, in Unix milliseconds
pub const USER_VAR: &str = "lbterm_typed";

/// Whether what a connection types is a program's: it never said who it is (`client`)
pub fn by_a_program(client: Option<&ClientId>) -> bool {
    client.is_none()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_window_says_who_it_is_and_the_cli_doesnt() {
        assert!(by_a_program(None));
        assert!(!by_a_program(Some(&ClientId::new())));
    }
}
