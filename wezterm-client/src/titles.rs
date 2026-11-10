//! LBTerm: the window titles a mux server has told this client, so none of them is sent back to
//! it. Upstream answers every local WindowTitleChanged with a set_window_title RPC a second later
//! (mux_notify_client_domain in domain.rs), the server's own titles included: every title a pane
//! set came back as one (a working Claude Code session sets its title every 960 ms), and one that
//! came back after a newer title had arrived set the server's window to the older title, which it
//! broadcast again. A title set here (Lua's `window:set_title`, the cli) still goes to the server.
use mux::window::WindowId;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Default)]
pub struct ServerTitles {
    /// The server's window id, and the title the server last gave that window
    heard: Mutex<HashMap<WindowId, String>>,
}

impl ServerTitles {
    /// The server says its window `remote` is titled `title`
    pub fn heard(&self, remote: WindowId, title: &str) {
        let mut heard = self.heard.lock();
        if heard.get(&remote).map(String::as_str) != Some(title) {
            heard.insert(remote, title.to_string());
        }
    }

    /// Whether `title` is what the server last said its window `remote` is titled: sending it
    /// would tell the server nothing
    pub fn is_theirs(&self, remote: WindowId, title: &str) -> bool {
        self.heard.lock().get(&remote).map(String::as_str) == Some(title)
    }

    /// The server's window `remote` has gone
    pub fn forget(&self, remote: WindowId) {
        self.heard.lock().remove(&remote);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_title_the_server_gave_is_theirs() {
        let titles = ServerTitles::default();
        assert!(!titles.is_theirs(3, "\u{25d0} work"), "nothing heard yet");
        titles.heard(3, "\u{25d0} work");
        assert!(titles.is_theirs(3, "\u{25d0} work"));
        // Another window's, or a title set here, isn't
        assert!(!titles.is_theirs(4, "\u{25d0} work"));
        assert!(!titles.is_theirs(3, "my own name"));
    }

    #[test]
    fn only_the_newest_title_heard_is_theirs() {
        let titles = ServerTitles::default();
        titles.heard(3, "\u{25d0} work");
        titles.heard(3, "\u{25d1} work");
        // The older one, sent back now, would turn the server's window back to it
        assert!(!titles.is_theirs(3, "\u{25d0} work"));
        assert!(titles.is_theirs(3, "\u{25d1} work"));
    }

    #[test]
    fn a_window_forgotten_has_no_titles() {
        let titles = ServerTitles::default();
        titles.heard(3, "work");
        titles.forget(3);
        assert!(!titles.is_theirs(3, "work"));
    }
}
