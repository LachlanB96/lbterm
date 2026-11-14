//! LBTerm: the side buttons' hops between tabs, the `LbtermTab` action (Lachlan's 8a and 9a of
//! 2026-11-01; lbterm/NEW-TAB.md, "Landing 4"). Bound in his lbterm.lua to the Back and Forward
//! side buttons, plain and with Shift, and performed by the `lbterm-tab-back`, `-forward`,
//! `-waiting` and `-last` events LyreBird fires through the control pipe (lbterm/CONTROL.md).
//!
//! At the press, on the GUI thread: the window's tabs with each one's parked flag, the rail's
//! rows as last drawn and the tabs it showed (render/tab_panel.rs, `tab_panel_hops`), the choice
//! of tab `lbterm_extras::hop::pick`'s, then `activate_tab`, the same switch a click on a pad
//! makes. No file is read: until 8a each hop opened a marker file a tab under ~/.claude/parked.

use config::lbterm::LbtermTab;
use lbterm_extras::hop::{pick, How};

impl super::TermWindow {
    /// `LbtermTab`: to the tab `how` names, if there's one to go to; nothing otherwise (logged)
    pub(crate) fn lbterm_tab_action(&mut self, how: LbtermTab) {
        let how = match how {
            LbtermTab::Back => How::Back,
            LbtermTab::Forward => How::Forward,
            LbtermTab::Waiting => How::Waiting,
            LbtermTab::Last => How::Last,
        };
        let (tabs, open, recent) = self.tab_panel_hops();
        match pick(&tabs, open, how, &recent) {
            Some(to) => {
                log::debug!("hop: {how:?} from {open:?} to {to}");
                self.activate_tab(to as isize).ok();
            }
            None => log::debug!("hop: {how:?} from {open:?} goes nowhere"),
        }
    }
}
