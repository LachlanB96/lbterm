//! The side buttons' hops between tabs (Lachlan's 8a and 9a of 2026-11-01; lbterm/NEW-TAB.md,
//! "Landing 4"), the choice of tab as logic tested without a window. wezterm-gui's
//! termwindow/hop.rs gathers the window's tabs at the press, on the GUI thread (each one's parked
//! flag, the rail's row as last drawn, the tabs it showed newest first) and activates the tab
//! [`pick`] names; the `LbtermTab` action is what the side buttons are bound to in Lachlan's
//! lbterm.lua, plain for Back and Forward and with Shift for Last and Waiting.

/// Which tab a hop goes to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The nearest tab before the open one that isn't parked, wrapping (the Back side button)
    Back,
    /// The nearest after it that isn't parked, wrapping (the Forward side button)
    Forward,
    /// The tab that's waited on him longest (Shift with Forward): blocked on him first, oldest
    /// first, then finished, oldest first; from a tab in that list, the next in it, wrapping
    Waiting,
    /// The tab he was on last that's still open and not parked (Shift with Back), so a second
    /// press goes back again, as Alt+Tab does
    Last,
}

/// How a tab waits on him, as its row on the rail says (lbterm_panel::claude::waits_on_you)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waits {
    /// Blocked on him (the orange pad: a permission prompt, a question, a login, or a tab waiting
    /// on another that asks him something too), else finished (your turn, done, extras, failed)
    pub asks: bool,
    /// When its row's status last changed, in Unix seconds: what its age and idle timer count from
    pub since: f64,
}

/// One of the window's tabs, in the window's order
#[derive(Debug, Clone, PartialEq)]
pub struct Hop {
    /// Its id
    pub tab: usize,
    /// Parked (the mux tab's flag): every hop passes it by
    pub parked: bool,
    /// The Conductor's tab, which its pad stands for: never a tab that waits on him
    pub conductor: bool,
    /// Whether and how it waits on him; `None` at work, without Claude, waiting on another tab or
    /// on its own background tasks, or not drawn on the rail
    pub waits: Option<Waits>,
}

/// The tab a hop `how` goes to from the window's `tabs`, `open` being the open one's place, with
/// `recent` the tabs the window showed, newest first: its place in `tabs`, or `None` when it goes
/// nowhere (every other tab parked, nothing waiting but the open tab, no tab shown before)
pub fn pick(tabs: &[Hop], open: Option<usize>, how: How, recent: &[usize]) -> Option<usize> {
    match how {
        How::Back => step(tabs, open?, -1),
        How::Forward => step(tabs, open?, 1),
        How::Waiting => {
            let list = waiting(tabs);
            let next = match open.and_then(|o| list.iter().position(|&i| i == o)) {
                Some(at) => list[(at + 1) % list.len()],
                None => *list.first()?,
            };
            (Some(next) != open).then_some(next)
        }
        How::Last => {
            let open_id = open.and_then(|o| tabs.get(o)).map(|t| t.tab);
            recent
                .iter()
                .filter(|&&id| Some(id) != open_id)
                .find_map(|&id| tabs.iter().position(|t| t.tab == id && !t.parked))
        }
    }
}

/// The nearest tab `dir` away from `open`, wrapping, that isn't parked
fn step(tabs: &[Hop], open: usize, dir: isize) -> Option<usize> {
    let n = tabs.len() as isize;
    (1..n)
        .map(|k| (open as isize + dir * k).rem_euclid(n) as usize)
        .find(|&i| !tabs[i].parked)
}

/// The tabs waiting on him, in the order Shift+Forward walks them: blocked on him first, then
/// finished, each oldest first, a tie in the window's order; parked ones and the Conductor's left
/// out
pub fn waiting(tabs: &[Hop]) -> Vec<usize> {
    let mut list: Vec<(usize, Waits)> = tabs
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.parked && !t.conductor)
        .filter_map(|(i, t)| t.waits.map(|w| (i, w)))
        .collect();
    list.sort_by(|(a, x), (b, y)| {
        y.asks
            .cmp(&x.asks)
            .then(x.since.total_cmp(&y.since))
            .then(a.cmp(b))
    });
    list.into_iter().map(|(i, _)| i).collect()
}

/// The tabs a window showed, newest first, for Shift+Back ([`How::Last`]): the open tab put in
/// front at each paint that shows another (wezterm-gui's render/tab_panel.rs, `tab_panel_tabs`),
/// tabs no longer in the window dropped then
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recent(Vec<usize>);

impl Recent {
    /// `tab` is the open tab now; `still(id)` says whether a tab is still in the window. Nothing
    /// changes while it's already in front, so a paint that shows the same tab costs one compare
    pub fn shown(&mut self, tab: usize, still: impl Fn(usize) -> bool) {
        if self.0.first() == Some(&tab) {
            return;
        }
        self.0.retain(|&t| t != tab && still(t));
        self.0.insert(0, tab);
    }

    /// Newest first
    pub fn tabs(&self) -> &[usize] {
        &self.0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn tab(id: usize) -> Hop {
        Hop {
            tab: id,
            parked: false,
            conductor: false,
            waits: None,
        }
    }

    fn parked(id: usize) -> Hop {
        Hop { parked: true, ..tab(id) }
    }

    fn asks(id: usize, since: f64) -> Hop {
        Hop {
            waits: Some(Waits { asks: true, since }),
            ..tab(id)
        }
    }

    fn finished(id: usize, since: f64) -> Hop {
        Hop {
            waits: Some(Waits { asks: false, since }),
            ..tab(id)
        }
    }

    #[test]
    fn back_and_forward_take_the_nearest_tab_not_parked_and_wrap() {
        let tabs = [tab(10), parked(11), tab(12), parked(13)];
        assert_eq!(pick(&tabs, Some(0), How::Forward, &[]), Some(2));
        assert_eq!(pick(&tabs, Some(2), How::Forward, &[]), Some(0), "wraps past the parked last");
        assert_eq!(pick(&tabs, Some(0), How::Back, &[]), Some(2), "wraps back past the parked last");
        assert_eq!(pick(&tabs, Some(2), How::Back, &[]), Some(0));
        // From a parked tab too: the hop leaves it
        assert_eq!(pick(&tabs, Some(1), How::Forward, &[]), Some(2));
        assert_eq!(pick(&tabs, Some(1), How::Back, &[]), Some(0));
        // A lone tab goes nowhere, nor does a window with none open
        assert_eq!(pick(&[tab(1)], Some(0), How::Forward, &[]), None);
        assert_eq!(pick(&tabs, None, How::Back, &[]), None);
    }

    #[test]
    fn every_other_tab_parked_goes_nowhere() {
        let tabs = [parked(1), tab(2), parked(3)];
        for how in [How::Back, How::Forward] {
            assert_eq!(pick(&tabs, Some(1), how, &[]), None, "{how:?}");
        }
        // Every tab parked, the open one too
        let all = [parked(1), parked(2), parked(3)];
        for how in [How::Back, How::Forward, How::Last] {
            assert_eq!(pick(&all, Some(0), how, &[2, 1, 3]), None, "{how:?}");
        }
        let all = [
            Hop { parked: true, ..asks(1, 5.0) },
            Hop { parked: true, ..finished(2, 1.0) },
        ];
        assert_eq!(pick(&all, Some(0), How::Waiting, &[]), None);
    }

    #[test]
    fn waiting_goes_to_the_oldest_blocked_on_him_then_the_oldest_finished() {
        let tabs = [
            tab(0),
            finished(1, 100.0),
            asks(2, 300.0),
            finished(3, 50.0),
            asks(4, 200.0),
            Hop { conductor: true, ..asks(5, 1.0) },
            Hop { parked: true, ..asks(6, 2.0) },
        ];
        assert_eq!(waiting(&tabs), vec![4, 2, 3, 1], "orange oldest first, then finished oldest first");
        // From a tab not in the list, to its first
        assert_eq!(pick(&tabs, Some(0), How::Waiting, &[]), Some(4));
        // From one in it, the next, so presses walk it, wrapping
        assert_eq!(pick(&tabs, Some(4), How::Waiting, &[]), Some(2));
        assert_eq!(pick(&tabs, Some(2), How::Waiting, &[]), Some(3));
        assert_eq!(pick(&tabs, Some(3), How::Waiting, &[]), Some(1));
        assert_eq!(pick(&tabs, Some(1), How::Waiting, &[]), Some(4));
        // From the Conductor's tab or a parked one, which aren't in it: its first
        assert_eq!(pick(&tabs, Some(5), How::Waiting, &[]), Some(4));
        assert_eq!(pick(&tabs, Some(6), How::Waiting, &[]), Some(4));
    }

    #[test]
    fn waiting_ties_go_in_the_windows_order_and_nothing_waiting_goes_nowhere() {
        let tabs = [finished(0, 10.0), asks(1, 10.0), asks(2, 10.0), finished(3, 10.0)];
        assert_eq!(waiting(&tabs), vec![1, 2, 0, 3]);
        let idle = [tab(0), tab(1), Hop { conductor: true, ..asks(2, 1.0) }];
        assert_eq!(pick(&idle, Some(0), How::Waiting, &[]), None, "nothing waiting");
        // The open tab the only one waiting: nowhere else to go
        let one = [asks(0, 1.0), tab(1)];
        assert_eq!(pick(&one, Some(0), How::Waiting, &[]), None);
        assert_eq!(pick(&one, Some(1), How::Waiting, &[]), Some(0));
    }

    #[test]
    fn last_goes_to_the_newest_tab_shown_still_open_and_not_parked() {
        let tabs = [tab(10), tab(11), parked(12), tab(13)];
        // Shown 13 now, 12 before it (since parked), 99 (since closed), 11, 10
        let recent = [13, 12, 99, 11, 10];
        assert_eq!(pick(&tabs, Some(3), How::Last, &recent), Some(1));
        // Nothing shown before, or only the open tab
        assert_eq!(pick(&tabs, Some(3), How::Last, &[]), None);
        assert_eq!(pick(&tabs, Some(3), How::Last, &[13]), None);
        // The last tab he was on closed: the one before it
        assert_eq!(pick(&tabs, Some(0), How::Last, &[10, 99, 13]), Some(3));
    }

    #[test]
    fn recent_keeps_the_tabs_shown_newest_first_so_last_goes_back_and_forth() {
        let mut recent = Recent::default();
        let open = |ids: &'static [usize]| move |id: usize| ids.contains(&id);
        recent.shown(1, open(&[1, 2, 3]));
        recent.shown(1, open(&[1, 2, 3]));
        recent.shown(2, open(&[1, 2, 3]));
        recent.shown(3, open(&[1, 2, 3]));
        assert_eq!(recent.tabs(), &[3, 2, 1]);
        let tabs = [tab(1), tab(2), tab(3)];
        // Shift+Back from 3 goes to 2, then the paint showing 2 puts it in front, and again to 3
        assert_eq!(pick(&tabs, Some(2), How::Last, recent.tabs()), Some(1));
        recent.shown(2, open(&[1, 2, 3]));
        assert_eq!(recent.tabs(), &[2, 3, 1]);
        assert_eq!(pick(&tabs, Some(1), How::Last, recent.tabs()), Some(2));
        // A closed tab is dropped the next time another is shown
        recent.shown(1, open(&[1, 2]));
        assert_eq!(recent.tabs(), &[1, 2]);
    }
}
