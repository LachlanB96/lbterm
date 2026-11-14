//! LBTerm's own links, `lbterm://...` (an InMail message, a conversation, an image): what a click
//! on one does is the config's `open-uri` handler's, and nothing else knows them. Windows has no
//! app for the scheme, so one handed to it (LBTerm's default when no handler takes a link, and
//! the `OpenUri` action) puts up its "get an app for this link" dialog, in front, taking the
//! keyboard from whatever Lachlan was typing in. So LBTerm never hands its own links to Windows:
//! one nothing took opens nothing, and says so in the log (lbterm/PITFALLS.md, 69).

/// The scheme of LBTerm's own links, without its `:`
pub const SCHEME: &str = "lbterm";

/// Whether `uri` is one of LBTerm's own links: its scheme is `lbterm`, in any case, as URI
/// schemes are compared
pub fn is_own(uri: &str) -> bool {
    uri.len() > SCHEME.len()
        && uri.as_bytes()[SCHEME.len()] == b':'
        && uri[..SCHEME.len()].eq_ignore_ascii_case(SCHEME)
}

/// Whether LBTerm may hand `uri` to Windows to open: every link but its own
pub fn windows_opens(uri: &str) -> bool {
    !is_own(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_links_by_their_scheme_in_any_case() {
        for uri in [
            "lbterm://inmail/Terminal?message=2026-11-02-2023-from-Terminal-x.md",
            "lbterm://session/6f1c0d4e-0000-4000-8000-000000000000",
            "lbterm://image/C:/Users/x/shot.png?row=2&of=5",
            "LBTerm://inmail/LyreBird",
            "lbterm:",
        ] {
            assert!(is_own(uri), "{uri}");
            assert!(!windows_opens(uri), "{uri}");
        }
    }

    #[test]
    fn every_other_link_goes_to_windows() {
        for uri in [
            "https://github.com/wezterm/wezterm",
            "http://127.0.0.1:27151/d/terminal",
            "file:///C:/Users/x/notes.md",
            "mailto:someone@example.com",
            "lbtermx://inmail/LyreBird",
            "lbter://inmail/LyreBird",
            "xlbterm://inmail/LyreBird",
            "lbterm",
            "",
        ] {
            assert!(!is_own(uri), "{uri}");
            assert!(windows_opens(uri), "{uri}");
        }
    }

    #[test]
    fn a_multibyte_character_where_the_colon_goes_is_no_panic() {
        assert!(!is_own("lbterm\u{00e9}//x"));
        assert!(!is_own("lbter\u{00e9}"));
    }
}
