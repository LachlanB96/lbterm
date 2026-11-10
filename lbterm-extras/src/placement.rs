//! LBTerm: where each window was, so it opens there again (Lachlan, 2026-10-25: after the
//! "Restart to update" pill the window came up on another screen at another size, and went
//! unresponsive). A window used to open where Windows puts new ones, sized for its tabs: on a
//! screen too small for them it was cut down, every tab's program was resized and redrew (nine
//! Claude sessions at once), and putting the window back resized them all again.
//!
//! Each window's place is its Windows placement, its normal rectangle and whether it's maximised,
//! kept in a file of its own (`window-<id>.txt`, the id its mux server knows it by, so a restart
//! finds each window's own) whenever it moves, is resized, maximised, restored or snapped. A new
//! window opens on that screen and is shown at that place, before anything is drawn, so its tabs
//! keep their size.

/// The folder a GUI keeps its windows' places in, when it isn't Lachlan's own LBTerm's (a test's)
pub const DIR_VAR: &str = "LBTERM_PLACEMENT_DIR";

/// A window's place: its normal rectangle, in Windows' workspace coordinates, and whether it's
/// maximised (or minimised from maximised)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub maximized: bool,
}

/// Smaller than this, a saved place is a mistake, not a window
const SMALLEST: i32 = 100;

impl Placement {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// One line: `left top right bottom maximized|normal`
    pub fn format(&self) -> String {
        format!(
            "{} {} {} {} {}\n",
            self.left,
            self.top,
            self.right,
            self.bottom,
            if self.maximized { "maximized" } else { "normal" }
        )
    }

    /// What `format` wrote; None for anything else, or a window too small to be one
    pub fn parse(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        let mut number = || words.next()?.parse::<i32>().ok();
        let (left, top, right, bottom) = (number()?, number()?, number()?, number()?);
        let maximized = match words.next()? {
            "maximized" => true,
            "normal" => false,
            _ => return None,
        };
        if words.next().is_some() {
            return None;
        }
        let placement = Self {
            left,
            top,
            right,
            bottom,
            maximized,
        };
        (placement.width() >= SMALLEST && placement.height() >= SMALLEST).then_some(placement)
    }
}

/// The file a window's place is kept in, by the id its mux server knows it by (its own id
/// outside a mux domain)
pub fn file_name(window: u64) -> String {
    format!("window-{window}.txt")
}

/// Which place a new window takes: its own, else, for the first window a GUI opens, the place
/// saved last (a new mux server numbers its windows afresh, and the main window is the one
/// that matters)
pub fn pick(own: Option<Placement>, newest: Option<Placement>, first: bool) -> Option<Placement> {
    own.or(newest.filter(|_| first))
}

#[cfg(test)]
mod test {
    use super::*;

    fn at(left: i32, top: i32, right: i32, bottom: i32, maximized: bool) -> Placement {
        Placement {
            left,
            top,
            right,
            bottom,
            maximized,
        }
    }

    #[test]
    fn a_place_reads_back_as_written() {
        // The portrait screen left of the right-most one, maximised; and a window on the
        // screen left of the primary, at negative coordinates
        for p in [at(3840, -177, 4920, 1743, true), at(-1900, 470, -300, 1400, false)] {
            assert_eq!(Placement::parse(&p.format()), Some(p));
        }
        assert_eq!(at(3840, -177, 4920, 1743, true).format(), "3840 -177 4920 1743 maximized\n");
    }

    #[test]
    fn anything_else_reads_as_nothing() {
        for text in [
            "",
            "1 2 3",
            "0 0 800 600 sideways",
            "0 0 800 600 normal extra",
            "a 0 800 600 normal",
            // Too small to be a window
            "0 0 50 600 normal",
            "0 0 800 -600 normal",
        ] {
            assert_eq!(Placement::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_window_takes_its_own_place_else_the_first_takes_the_last_saved() {
        let own = at(0, 0, 800, 600, false);
        let last = at(3840, -177, 4920, 1743, true);
        assert_eq!(pick(Some(own), Some(last), true), Some(own));
        assert_eq!(pick(Some(own), Some(last), false), Some(own));
        assert_eq!(pick(None, Some(last), true), Some(last));
        // Another window with no place of its own goes where Windows puts it
        assert_eq!(pick(None, Some(last), false), None);
        assert_eq!(pick(None, None, true), None);
    }

    #[test]
    fn its_file_is_named_by_the_window() {
        assert_eq!(file_name(0), "window-0.txt");
        assert_eq!(file_name(12), "window-12.txt");
    }
}
