//! The tab menu's Hide from Conductor and Show to Conductor, from the pick until the Conductor
//! says it (lbterm/NEW-TAB.md, section 8). LBTerm hides nothing itself: a pick fires
//! `lbterm-conductor-hide`, Lachlan's config runs `conductor hide|show` for each of the tab's
//! sessions, and the Conductor writes its button.json again with `hidden` naming every session
//! it's hidden from now ([`Said::hidden`]). Until that write, for up to [`PENDING`] (the tab
//! links' 5 s), the tab's row shows what was asked already ([`Picks::look`]); once the file says
//! it the pick is [`Answer::Done`] and the tab's pad glints, and if it hasn't in that time it's
//! [`Answer::NotDone`]: the row goes back to what the file says and the strip says so
//! ([`not_done`]).
//!
//! The GUI keeps one [`Picks`] a window (wezterm-gui's render/conductor_hide.rs) and settles it at
//! the top of each tab panel paint against the state file as the `lbterm-state-files` thread last
//! read it: a write of button.json repaints every window, and the pick's own end wakes a paint.

use super::{Cover, Offer, Said};
use std::time::{Duration, Instant};

/// How long a pick shows on the row before the Conductor's file says it: the tab links' (wezterm-
/// gui's termwindow/tab_links.rs, `PENDING`), as section 8 has it
pub const PENDING: Duration = Duration::from_secs(5);

/// One pick of the tab menu's row, waiting for the Conductor's word
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    /// Its tab, by id: the row that shows it, and the pad that glints
    pub tab: usize,
    /// The tab's Claude sessions as it was picked: every pane's `claude_session`
    pub sessions: Vec<String>,
    /// The tab's label as its row showed it, for the strip's words when it isn't done
    pub label: String,
    /// Hide from Conductor (true) or Show to Conductor (false)
    pub hide: bool,
    pub asked: Instant,
    /// The state file's `updatedAt` as it was picked. Only a write since can answer the pick: the
    /// Conductor writes button.json with a fresh `updatedAt` on every `hide` and `show`, hidden
    /// already or not (`@Conductor:button`), so a Show picked straight after a Hide it hasn't
    /// answered yet isn't taken as done by the file from before either (lbterm/PITFALLS.md, 236)
    pub then: Option<String>,
}

impl Pick {
    /// A pick made now, against the state file as it is now
    pub fn new(tab: usize, sessions: Vec<String>, label: String, hide: bool, said: Option<&Said>, asked: Instant) -> Self {
        Self {
            tab,
            sessions,
            label,
            hide,
            asked,
            then: said.and_then(|s| s.get("updatedAt")).map(str::to_string),
        }
    }

    /// Whether the file says what it asked: written since, and its list naming every session for
    /// a hide, none for a show (`conductor::confirmed`)
    fn done(&self, said: &Said) -> bool {
        said.get("updatedAt") != self.then.as_deref() && super::confirmed(said, &self.sessions, self.hide)
    }
}

/// How a tab's row looks for the Conductor's hiding, for the rail (the mark and the open panel's
/// chip, `Row::hidden`) and the state file (`hidden`, `hidden_pending`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    /// It shows hidden: every one of its sessions is on the Conductor's list, or a Hide picked
    /// for it waits for the Conductor's word
    pub hidden: bool,
    /// What it shows is a pick still waiting for the Conductor's word
    pub pending: bool,
}

/// What became of a pick
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The Conductor's file says it: the tab's pad glints (`copy_in`, `copy_out`)
    Done { tab: usize, hide: bool },
    /// It didn't within [`PENDING`]: the row goes back to what the file says, and the strip says
    /// [`not_done`] for its notice's 4 s
    NotDone { tab: usize, hide: bool, label: String },
}

/// Every pick a window has waiting, at most one a tab
#[derive(Debug, Clone, Default)]
pub struct Picks(Vec<Pick>);

impl Picks {
    /// A new pick for its tab: one that tab had waiting gives way to it. The GUI never asks a
    /// second time for a tab while one waits ([`Picks::offer`]'s row is greyed and picks nothing
    /// then, and the view's Show does nothing), so two opposite requests never race to the
    /// Conductor (lbterm/PITFALLS.md, 244)
    pub fn ask(&mut self, pick: Pick) {
        self.0.retain(|p| p.tab != pick.tab);
        self.0.push(pick);
    }

    /// The picks the file (`said`, none for no file) has answered or that have waited
    /// [`PENDING`] by `now`, taken out, in the order they were asked. One the file answers is
    /// done even at its last moment: the answer is looked for before the time
    pub fn settle(&mut self, said: Option<&Said>, now: Instant) -> Vec<Answer> {
        let mut answers = vec![];
        self.0.retain(|p| {
            if said.is_some_and(|s| p.done(s)) {
                answers.push(Answer::Done { tab: p.tab, hide: p.hide });
                false
            } else if now.saturating_duration_since(p.asked) >= PENDING {
                answers.push(Answer::NotDone {
                    tab: p.tab,
                    hide: p.hide,
                    label: p.label.clone(),
                });
                false
            } else {
                true
            }
        });
        answers
    }

    /// How tab `tab` looks, its Claude sessions `sessions`: what a pick waiting for it asked,
    /// else what the file says (hidden when its list names every one of them)
    pub fn look<S: AsRef<str>>(&self, tab: usize, sessions: &[S], said: Option<&Said>) -> Look {
        if let Some(p) = self.0.iter().find(|p| p.tab == tab) {
            return Look {
                hidden: p.hide,
                pending: true,
            };
        }
        let hidden = !sessions.is_empty()
            && said
                .and_then(|s| s.hidden.as_ref())
                .is_some_and(|h| h.cover(sessions) == Cover::AllOf);
        Look { hidden, pending: false }
    }

    /// The tab menu's row for tab `tab` (lbterm/NEW-TAB.md, section 7, row 6): none on a tab with
    /// no Claude session or while the file has no `hidden` list (`conductor::offer`),
    /// [`Offer::Waiting`] while a pick for the tab waits for the Conductor's word, else Show to
    /// Conductor when the file hides every one of its sessions, so the row and the mark agree, and
    /// Hide from Conductor otherwise. While one waits the row is greyed, saying what it waits for,
    /// and picks nothing (Lachlan's 11b of 2026-11-09), since the reverse, offered from the row
    /// as drawn, would start a second `conductor` beside the first, which the Conductor carries
    /// out in whatever order the two finish: a Show taken as done from its own write could see
    /// the Hide's land after it and leave the tab hidden (lbterm/PITFALLS.md, 244). The wait is
    /// 5 s at most ([`PENDING`])
    pub fn offer<S: AsRef<str>>(&self, tab: usize, sessions: &[S], said: Option<&Said>) -> Option<Offer> {
        let said = said?;
        super::offer(said, sessions)?;
        if let Some(p) = self.0.iter().find(|p| p.tab == tab) {
            return Some(Offer::Waiting { hide: p.hide });
        }
        Some(if self.look(tab, sessions, Some(said)).hidden { Offer::Show } else { Offer::Hide })
    }

    /// Whether a pick for tab `tab` waits for the Conductor's word
    pub fn waits(&self, tab: usize) -> bool {
        self.0.iter().any(|p| p.tab == tab)
    }

    /// When the first pick still waiting runs out
    pub fn due(&self) -> Option<Instant> {
        self.0.iter().map(|p| p.asked + PENDING).min()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The strip's words when the Conductor didn't do what was picked for the tab labelled `label`
pub fn not_done(label: &str, hide: bool) -> String {
    if hide {
        format!("The Conductor didn't hide {label}")
    } else {
        format!("The Conductor didn't show {label} again")
    }
}

/// The tab menu row's words: greyed while a pick waits, they say what it waits for
pub fn words(offer: Offer) -> &'static str {
    match offer {
        Offer::Hide => "Hide from Conductor",
        Offer::Show => "Show to Conductor",
        Offer::Waiting { hide: true } => "Hiding from Conductor\u{2026}",
        Offer::Waiting { hide: false } => "Showing to Conductor\u{2026}",
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::conductor::read;

    const A: &str = "a1b2c3d4-0000-4000-8000-000000000001";
    const B: &str = "a1b2c3d4-0000-4000-8000-000000000002";

    fn file(hidden: &[&str], at: u64) -> Said {
        let ids: Vec<String> = hidden.iter().map(|s| format!("\"{s}\"")).collect();
        read(&format!(r#"{{"state":"on","hidden":[{}],"updatedAt":"2026-11-09T10:00:{at:02}Z"}}"#, ids.join(","))).unwrap()
    }

    fn pick(tab: usize, sessions: &[&str], hide: bool, said: &Said, asked: Instant) -> Pick {
        Pick::new(tab, sessions.iter().map(|s| s.to_string()).collect(), "Tab Wave".into(), hide, Some(said), asked)
    }

    #[test]
    fn a_hide_shows_at_once_and_is_done_when_the_file_names_every_session() {
        let t = Instant::now();
        let before = file(&[], 0);
        let mut picks = Picks::default();
        picks.ask(pick(3, &[A, B], true, &before, t));
        // At once: hidden, waiting
        assert_eq!(picks.look(3, &[A, B], Some(&before)), Look { hidden: true, pending: true });
        assert_eq!(picks.look(4, &[A, B], Some(&before)), Look { hidden: false, pending: false }, "another tab's pick isn't this one's");
        assert_eq!(picks.due(), Some(t + PENDING));
        // Nothing written yet: still waiting
        assert_eq!(picks.settle(Some(&before), t + Duration::from_secs(1)), vec![]);
        // Written with only one of its sessions: not done
        let half = file(&[A], 1);
        assert_eq!(picks.settle(Some(&half), t + Duration::from_secs(2)), vec![]);
        assert!(picks.look(3, &[A, B], Some(&half)).pending);
        // Both: done, and the row says what the file says from then on
        let both = file(&[A, B], 2);
        assert_eq!(picks.settle(Some(&both), t + Duration::from_secs(3)), vec![Answer::Done { tab: 3, hide: true }]);
        assert!(picks.is_empty());
        assert_eq!(picks.due(), None);
        assert_eq!(picks.look(3, &[A, B], Some(&both)), Look { hidden: true, pending: false });
    }

    #[test]
    fn a_pick_the_file_doesnt_answer_in_five_seconds_goes_back_and_says_so() {
        let t = Instant::now();
        let before = file(&[], 0);
        let mut picks = Picks::default();
        picks.ask(pick(3, &[A], true, &before, t));
        assert_eq!(picks.settle(Some(&before), t + PENDING - Duration::from_millis(1)), vec![]);
        assert_eq!(
            picks.settle(Some(&before), t + PENDING),
            vec![Answer::NotDone { tab: 3, hide: true, label: "Tab Wave".into() }]
        );
        // Back to what the file says
        assert_eq!(picks.look(3, &[A], Some(&before)), Look::default());
        assert_eq!(not_done("Tab Wave", true), "The Conductor didn't hide Tab Wave");
        assert_eq!(not_done("Tab Wave", false), "The Conductor didn't show Tab Wave again");
    }

    #[test]
    fn an_answer_at_the_last_moment_is_done_and_late_is_not() {
        let t = Instant::now();
        let before = file(&[], 0);
        let mut picks = Picks::default();
        picks.ask(pick(3, &[A], true, &before, t));
        // Read by the paint the end's wake makes: the file answered by then counts
        let after = file(&[A], 4);
        assert_eq!(picks.settle(Some(&after), t + PENDING), vec![Answer::Done { tab: 3, hide: true }]);
        // No file at all: nothing answers, and the time still runs out (a slow PC's late paint
        // too: later, never sooner)
        picks.ask(pick(3, &[A], true, &before, t));
        assert_eq!(picks.settle(None, t + Duration::from_secs(4)), vec![]);
        assert!(matches!(picks.settle(None, t + Duration::from_secs(30))[..], [Answer::NotDone { .. }]));
    }

    #[test]
    fn a_show_is_done_once_the_file_names_none_of_its_sessions() {
        let t = Instant::now();
        let hidden = file(&[A, B], 0);
        let mut picks = Picks::default();
        assert_eq!(picks.offer(3, &[A, B], Some(&hidden)), Some(Offer::Show));
        picks.ask(pick(3, &[A, B], false, &hidden, t));
        assert_eq!(picks.look(3, &[A, B], Some(&hidden)), Look { hidden: false, pending: true });
        assert_eq!(picks.settle(Some(&file(&[B], 1)), t + Duration::from_secs(1)), vec![]);
        assert_eq!(picks.settle(Some(&file(&[], 2)), t + Duration::from_secs(2)), vec![Answer::Done { tab: 3, hide: false }]);
    }

    #[test]
    fn only_a_write_since_the_pick_answers_it() {
        // The file already says what's asked as the pick is made (the menu was open while a hide
        // run by hand wrote it): it's the file from before the pick, so it answers nothing, and
        // only the Conductor's write for this pick does
        let t = Instant::now();
        let already = file(&[A], 0);
        let mut picks = Picks::default();
        picks.ask(pick(3, &[A], true, &already, t));
        assert_eq!(picks.settle(Some(&already), t + Duration::from_millis(400)), vec![]);
        assert_eq!(
            picks.settle(Some(&file(&[A], 1)), t + Duration::from_millis(700)),
            vec![Answer::Done { tab: 3, hide: true }]
        );
    }

    #[test]
    fn no_second_request_for_a_tab_while_one_waits() {
        // A Hide waiting: its row shows hidden, and the menu's row is greyed, saying it's hiding,
        // and picks nothing until the Conductor has answered or 5 s have gone, so no Show races it
        // (PITFALLS 244; Lachlan's 11b of 2026-11-09)
        let t = Instant::now();
        let before = file(&[], 0);
        let mut picks = Picks::default();
        assert_eq!(picks.offer(3, &[A], Some(&before)), Some(Offer::Hide));
        picks.ask(pick(3, &[A], true, &before, t));
        assert!(picks.waits(3));
        assert_eq!(picks.look(3, &[A], Some(&before)), Look { hidden: true, pending: true });
        assert_eq!(
            picks.offer(3, &[A], Some(&before)),
            Some(Offer::Waiting { hide: true }),
            "greyed, saying it's hiding, while the Hide waits"
        );
        assert_eq!(picks.offer(5, &[B], Some(&before)), Some(Offer::Hide), "another tab's row is its own");
        // Answered: Show is offered from the file
        let hidden = file(&[A], 1);
        assert_eq!(picks.settle(Some(&hidden), t + Duration::from_secs(1)), vec![Answer::Done { tab: 3, hide: true }]);
        assert!(!picks.waits(3));
        assert_eq!(picks.offer(3, &[A], Some(&hidden)), Some(Offer::Show));
        // Run out: offered again as the file says
        picks.ask(pick(3, &[A], false, &hidden, t + Duration::from_secs(2)));
        assert_eq!(picks.offer(3, &[A], Some(&hidden)), Some(Offer::Waiting { hide: false }));
        assert!(matches!(picks.settle(Some(&hidden), t + Duration::from_secs(2) + PENDING)[..], [Answer::NotDone { .. }]));
        assert_eq!(picks.offer(3, &[A], Some(&hidden)), Some(Offer::Show));
    }

    #[test]
    fn the_row_follows_the_file_and_a_waiting_pick() {
        let none = file(&[], 0);
        let picks = Picks::default();
        assert_eq!(picks.offer(3, &[A], Some(&none)), Some(Offer::Hide));
        assert_eq!(picks.offer(3, &[A], Some(&file(&[A], 1))), Some(Offer::Show));
        // Some but not all of the tab's sessions hidden: Hide, which hides the rest, and no mark
        let some = file(&[A], 1);
        assert_eq!(picks.offer(3, &[A, B], Some(&some)), Some(Offer::Hide));
        assert!(!picks.look(3, &[A, B], Some(&some)).hidden);
        // A shell tab, a file without the list and no file offer nothing, and look visible
        let empty: [&str; 0] = [];
        assert_eq!(picks.offer(3, &empty, Some(&none)), None);
        assert_eq!(picks.look(3, &empty, Some(&file(&[A], 1))), Look::default());
        let old = read(r#"{"state":"on"}"#).unwrap();
        assert_eq!(picks.offer(3, &[A], Some(&old)), None);
        assert_eq!(picks.offer(3, &[A], None), None);
        assert_eq!(words(Offer::Hide), "Hide from Conductor");
        assert_eq!(words(Offer::Show), "Show to Conductor");
        assert_eq!(words(Offer::Waiting { hide: true }), "Hiding from Conductor\u{2026}");
        assert_eq!(words(Offer::Waiting { hide: false }), "Showing to Conductor\u{2026}");
    }

    #[test]
    fn picks_for_two_tabs_wait_apart() {
        let t = Instant::now();
        let before = file(&[], 0);
        let mut picks = Picks::default();
        picks.ask(pick(3, &[A], true, &before, t));
        picks.ask(pick(5, &[B], true, &before, t + Duration::from_secs(2)));
        assert_eq!(picks.due(), Some(t + PENDING));
        // The first's session hidden: it's done, the second waits on
        assert_eq!(picks.settle(Some(&file(&[A], 1)), t + Duration::from_secs(3)), vec![Answer::Done { tab: 3, hide: true }]);
        assert_eq!(picks.due(), Some(t + Duration::from_secs(2) + PENDING));
        assert!(picks.look(5, &[B], Some(&file(&[A], 1))).pending);
    }
}
