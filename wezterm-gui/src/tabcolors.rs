//! LBTerm: the colour each tab was given from its menu (lbterm_extras::tabs::COLOURS), shown as
//! an accent on its pad in the tab panel. Kept for the life of the GUI, by tab id, so a tab keeps
//! its colour when it's dragged to another window.

use mux::tab::TabId;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static COLOURS: RefCell<HashMap<TabId, usize>> = RefCell::new(HashMap::new());
}

/// The tab's colour, as an index into COLOURS
pub fn get(tab: TabId) -> Option<usize> {
    COLOURS.with(|c| c.borrow().get(&tab).copied())
}

/// Give the tab a colour, or take it away
pub fn set(tab: TabId, colour: Option<usize>) {
    COLOURS.with(|c| {
        let mut c = c.borrow_mut();
        match colour {
            Some(i) if i < lbterm_extras::tabs::COLOURS.len() => {
                c.insert(tab, i);
            }
            _ => {
                c.remove(&tab);
            }
        }
    });
}

/// The tab's colour as sRGB, if it has one
pub fn rgb(tab: TabId) -> Option<(u8, u8, u8)> {
    get(tab).map(|i| lbterm_extras::tabs::COLOURS[i].1)
}

/// Forget tabs that have closed
pub fn tidy() {
    let mux = mux::Mux::get();
    COLOURS.with(|c| c.borrow_mut().retain(|tab, _| mux.get_tab(*tab).is_some()));
}
