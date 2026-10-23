//! What goes back to the program, written into its pane's input

use crate::parse::sanitize_id;

const OSC: &str = "\x1b]";
const ST: &str = "\x1b\\";

/// The answer to `p=?`: what LBTerm supports. Windows toasts can't show icons from a
/// program, so `p` has no `icon`; LBTerm tracks closes, so `c=1`.
pub fn query(id: &str) -> String {
    format!(
        "{OSC}99;i={}:p=?;a=focus,report:c=1:o=always,unfocused,invisible:p=title,body,close,buttons,?,alive:s=system,silent:u=0,1,2:w=1{ST}",
        sanitize_id(id)
    )
}

/// The notification was clicked (with `a=report`)
pub fn activated(id: &str) -> String {
    format!("{OSC}99;i={};{ST}", sanitize_id(id))
}

/// Button `number` (counting from 1) was clicked (with `a=report`)
pub fn button(id: &str, number: usize) -> String {
    format!("{OSC}99;i={};{number}{ST}", sanitize_id(id))
}

/// The notification closed (with `c=1`)
pub fn closed(id: &str) -> String {
    format!("{OSC}99;i={}:p=close;{ST}", sanitize_id(id))
}

/// The answer to `p=alive`: the ids still open
pub fn alive<'a>(id: &str, open: impl IntoIterator<Item = &'a str>) -> String {
    let list: Vec<String> = open.into_iter().map(sanitize_id).collect();
    format!("{OSC}99;i={}:p=alive;{}{ST}", sanitize_id(id), list.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(activated("abc"), "\x1b]99;i=abc;\x1b\\");
        assert_eq!(button("abc", 2), "\x1b]99;i=abc;2\x1b\\");
        assert_eq!(closed("0"), "\x1b]99;i=0:p=close;\x1b\\");
        assert_eq!(alive("me", ["a", "b"]), "\x1b]99;i=me:p=alive;a,b\x1b\\");
        assert_eq!(alive("me", []), "\x1b]99;i=me:p=alive;\x1b\\");
    }

    #[test]
    fn the_query_answer_lists_what_is_supported() {
        let q = query("q1");
        assert!(q.starts_with("\x1b]99;i=q1:p=?;"));
        for part in ["a=focus,report", "c=1", "o=always,unfocused,invisible", "p=title,body", "s=system,silent", "w=1"] {
            assert!(q.contains(part), "{q:?} lacks {part}");
        }
    }

    #[test]
    fn ids_are_sanitized_before_they_go_back() {
        assert_eq!(activated("x\x1b]0;pwned\x07"), "\x1b]99;i=x0pwned;\x1b\\");
    }
}
