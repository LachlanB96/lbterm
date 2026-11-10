//! What a row shows, hashed in two parts for the compositor's rows (wezterm-gui's
//! render/tab_panel.rs and compositor.rs, `RowPlan`): what the closed rail draws of it (its pad,
//! timer, number and label, its marks), and what only the open panel's words show (the title line
//! with its age or agents, the project and the status, the progress bars). A row whose look
//! changes dissolves from its old one; while the panel is closed only the rail's look counts, so
//! a new status, message or age, which the closed rail doesn't show, no longer dissolves an
//! unchanged rail. Until 2026-10-31 the whole row was formatted with `{:?}` and hashed each frame,
//! one string a row, and any field moving restyled it on the rail.

use crate::claude::Row;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// The closed rail's look and the open panel's words' look of `row`
pub fn looks(row: &Row) -> (u64, u64) {
    // Every field named, so one added to Row has to be put in one look or the other
    let Row {
        index,
        title,
        project,
        state,
        status,
        message,
        agents,
        progress,
        estimated,
        left,
        agent_bars,
        unread,
        age,
        level,
        bars,
        active,
        label,
        waiting,
        extras,
        asks,
        link,
        quiet,
        cost,
        hidden,
        parked,
    } = row;
    let mut rail = DefaultHasher::new();
    // The pad (its state's colour, the idle bars, the rings for a question, extras or a quiet
    // reply, the spinner's disc or the unread dot, the eye-off mark of one hidden from the
    // Conductor), the number on it and the label under it, the pill lit for the open tab, and how
    // it stands on a link
    (index, *state as u8, (*bars).map(|b| b as u8), active, label, waiting, unread, extras, asks, link, quiet, hidden).hash(&mut rail);
    // Parked: the whole closed row dimmed (Lachlan's 8a)
    parked.hash(&mut rail);
    let mut words = DefaultHasher::new();
    // The words, the chips (the Hidden chip of one hidden from the Conductor among them) and the
    // bars' tracks (how far along glides in a layer of its own, so only whether there's a bar and
    // how many subagents' bars)
    (title, project, status, message, agents, progress.is_some(), estimated, left, agent_bars.len(), age, level, cost).hash(&mut words);
    // A tuple hashes up to twelve fields; the Parked chip
    (hidden, parked).hash(&mut words);
    (rail.finish(), words.finish())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::claude::{row, LinkState, RowState, Tab};
    use crate::idle::Bar;

    fn base() -> Row {
        row(&Tab { index: 3, ..Default::default() }, 1_000.0, 3_600.0).0
    }

    #[test]
    fn every_field_shows_in_one_look_or_the_other() {
        let (rail, words) = looks(&base());
        let rail_only: Vec<(&str, fn(&mut Row))> = vec![
            ("index", |r| r.index += 1),
            ("state", |r| r.state = RowState::Waiting),
            ("bars", |r| r.bars[1] = Bar::Red),
            ("active", |r| r.active = !r.active),
            ("label", |r| r.label.push('x')),
            ("waiting", |r| r.waiting += 1),
            ("unread", |r| r.unread = !r.unread),
            ("extras", |r| r.extras = !r.extras),
            ("asks", |r| r.asks = !r.asks),
            ("link", |r| r.link = Some(LinkState::Stuck)),
            ("quiet", |r| r.quiet = !r.quiet),
        ];
        for (name, change) in rail_only {
            let mut r = base();
            change(&mut r);
            let (r2, w2) = looks(&r);
            assert_ne!(r2, rail, "{name} is the closed rail's");
            assert_eq!(w2, words, "{name} isn't the words'");
        }
        let words_only: Vec<(&str, fn(&mut Row))> = vec![
            ("title", |r| r.title.push('x')),
            ("project", |r| r.project.push('x')),
            ("status", |r| r.status.push('x')),
            ("message", |r| r.message.push('x')),
            ("agents", |r| r.agents += 1),
            ("progress", |r| r.progress = Some(40)),
            ("estimated", |r| r.estimated = !r.estimated),
            ("left", |r| r.left.push('x')),
            ("agent_bars", |r| r.agent_bars.push(10)),
            ("age", |r| r.age.push('x')),
            ("level", |r| r.level += 1),
            ("cost", |r| r.cost.push('x')),
        ];
        for (name, change) in words_only {
            let mut r = base();
            change(&mut r);
            let (r2, w2) = looks(&r);
            assert_eq!(r2, rail, "{name} isn't the closed rail's");
            assert_ne!(w2, words, "{name} is the words'");
        }
        // Hidden from the Conductor shows in both: the mark on the pad, the chip in the words
        let mut r = base();
        r.hidden = true;
        let (r2, w2) = looks(&r);
        assert_ne!(r2, rail, "hidden is the closed rail's");
        assert_ne!(w2, words, "hidden is the words' too");
        // Parked too: the dimmed pad, the chip in the words (Lachlan's 8a)
        let mut r = base();
        r.parked = true;
        let (r2, w2) = looks(&r);
        assert_ne!(r2, rail, "parked is the closed rail's");
        assert_ne!(w2, words, "parked is the words' too");
    }

    #[test]
    fn how_far_along_a_bar_is_shows_in_neither() {
        let mut r = base();
        r.progress = Some(10);
        r.agent_bars = vec![20, 30];
        let before = looks(&r);
        r.progress = Some(90);
        r.agent_bars = vec![80, 70];
        assert_eq!(looks(&r), before, "the bars glide in a layer of their own");
    }
}
