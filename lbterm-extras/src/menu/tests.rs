use super::*;

/// The menu's font in a test: 8 px a character, 17 px a line
const CELL: f64 = 8.0;
const LINE: f64 = 17.0;

fn measure(words: &str) -> f64 {
    words.chars().count() as f64 * CELL
}

/// The menu at 96 DPI, measured as the GUI does when it opens or turns the page
fn lay(at: (f64, f64), size: (f64, f64), facts: &Facts, page: Page) -> Layout {
    let text = Text {
        widest: widest(facts, page, 1.0, measure),
        line: LINE,
    };
    layout(at, size, 1.0, facts, page, text)
}

fn other(tab: usize, label: &str) -> Other {
    Other {
        tab,
        label: label.into(),
        colour: 0x5b7cff,
    }
}

/// A Claude tab in a project, with links on, another Claude tab open, two link rows, a tab
/// Close finished tabs would close and a Conductor that can hide it: every row there is but Park
/// tab, which a domain whose server can't park doesn't offer ([`parkable`] has it)
fn claude() -> Facts {
    Facts {
        claude: true,
        project: Some("Warden".into()),
        links_on: true,
        links: vec!["Stop waiting on Tab Wave".into(), "Wait on it again".into()],
        others: vec![other(4, "Tab Wave")],
        finished: vec![Named {
            tab: 4,
            label: "Tab Wave".into(),
        }],
        conductor: Some(Offer::Hide),
        park: None,
    }
}

/// [`claude`] on a domain that can park, the tab unparked
fn parkable() -> Facts {
    Facts {
        park: Some(false),
        ..claude()
    }
}

/// Lachlan's 8a (lbterm/NEW-TAB.md, "Landing 4"): Park tab, P, first in Wait on…'s group, on a
/// shell tab too; Unpark tab on a parked one; none where the tab can't be parked
#[test]
fn park_tab_is_first_in_its_group_on_every_tab_that_can_park() {
    use Item::*;
    let r = rows(&parkable(), Page::Main);
    let at = r.iter().position(|i| *i == Some(Park)).unwrap();
    assert_eq!((r[at - 1], r[at + 1], r[at + 2]), (None, Some(Hide), Some(WaitOn)), "{r:?}");
    assert_eq!(label(Park, &parkable()), "Park tab");
    let parked = Facts {
        park: Some(true),
        ..claude()
    };
    assert_eq!(label(Park, &parked), "Unpark tab");
    assert_eq!(key(Park), Some('P'));
    assert_eq!(shown_key(Park, &parkable()), Some('P'));
    assert_eq!(by_key('p', &items(&parked, Page::Main)), Some(Park));
    assert_eq!(answer(Key::Char('P'), Page::Main, &items(&parkable(), Page::Main), None), Did::Pick(Park));
    // No other row takes P
    for item in items(&parkable(), Page::Main) {
        assert!(item == Park || key(item) != Some('P'), "{item:?}");
    }
    // A shell tab's menu has it, alone in its group
    let shell = Facts {
        claude: false,
        ..parkable()
    };
    let r = rows(&shell, Page::Main);
    let at = r.iter().position(|i| *i == Some(Park)).unwrap();
    assert_eq!((r[at - 1], r[at + 1]), (None, None), "{r:?}");
    // Where it can't be parked (the Conductor's tab, an older server): no row, P goes nowhere
    assert!(!items(&claude(), Page::Main).contains(&Park));
    assert_eq!(answer(Key::Char('p'), Page::Main, &items(&claude(), Page::Main), None), Did::Nowhere);
    // Nor on Wait on…'s page
    assert!(!items(&parkable(), Page::WaitOn).contains(&Park));
    // The card is as wide as Unpark tab needs
    let l = lay((100.0, 100.0), (1200.0, 900.0), &parked, Page::Main);
    assert!(l.rows.iter().any(|(i, _)| *i == Some(Park)));
}

/// A shell tab in a project, everything else as on [`claude`]
fn shell() -> Facts {
    Facts {
        claude: false,
        ..claude()
    }
}

/// Today's menu: no Claude session, no project, no links, nothing finished
fn bare() -> Facts {
    Facts::default()
}

#[test]
fn a_claude_tab_has_every_row_in_order() {
    use Item::*;
    assert_eq!(
        rows(&claude(), Page::Main),
        vec![
            Some(Rename),
            Some(Colour(None)),
            Some(CopyLink),
            None,
            Some(NewSession),
            Some(Shell),
            None,
            Some(Hide),
            Some(WaitOn),
            None,
            Some(Link(0)),
            Some(Link(1)),
            None,
            Some(NewWindow),
            None,
            Some(CloseFinished),
            Some(CloseOthers),
            Some(Close),
        ]
    );
}

#[test]
fn hide_from_conductor_needs_a_claude_tab_and_the_conductors_list() {
    use Item::*;
    // Without the list (no file, a Conductor from before 2026-11-02, or no Conductor): no row,
    // and H goes nowhere
    let none = Facts {
        conductor: None,
        ..claude()
    };
    assert!(!items(&none, Page::Main).contains(&Hide));
    assert_eq!(by_key('h', &items(&none, Page::Main)), None);
    // With it: in Wait on…'s group, before it, its words saying which, H either case
    assert_eq!(label(Hide, &claude()), "Hide from Conductor");
    let shown = Facts {
        conductor: Some(Offer::Show),
        ..claude()
    };
    assert_eq!(label(Hide, &shown), "Show to Conductor");
    assert_eq!(by_key('H', &items(&shown, Page::Main)), Some(Hide));
    assert_eq!(answer(Key::Char('h'), Page::Main, &items(&claude(), Page::Main), None), Did::Pick(Hide));
    // On its own, without Wait on…: a group of its own between Shell's and the links'
    let alone = Facts {
        links_on: false,
        ..claude()
    };
    let r = rows(&alone, Page::Main);
    let at = r.iter().position(|i| *i == Some(Hide)).unwrap();
    assert_eq!((r[at - 1], r[at + 1]), (None, None), "{r:?}");
    // A shell tab has none, whatever the GUI said (it says none there itself)
    assert!(!items(&shell(), Page::Main).contains(&Hide));
    // Nor has Wait on…'s page
    assert!(!items(&claude(), Page::WaitOn).contains(&Hide));
}

#[test]
fn while_a_pick_waits_its_row_is_greyed_and_picks_nothing() {
    use Item::*;
    // Lachlan's 11b of 2026-11-09: the row stays in its place, saying what it waits for, but no
    // key, highlight or click reaches it (a second request would race the first, PITFALLS 244)
    for (hide, words) in [(true, "Hiding from Conductor\u{2026}"), (false, "Showing to Conductor\u{2026}")] {
        let waiting = Facts {
            conductor: Some(Offer::Waiting { hide }),
            ..claude()
        };
        assert_eq!(label(Hide, &waiting), words);
        assert!(inert(Hide, &waiting));
        assert_eq!(rows(&waiting, Page::Main), rows(&claude(), Page::Main), "the same rows in the same places");
        assert!(shown(&waiting, Page::Main).contains(&Hide));
        assert!(!items(&waiting, Page::Main).contains(&Hide));
        assert_eq!(shown_key(Hide, &waiting), None, "no key at its right");
        assert_eq!(shown_key(Hide, &claude()), Some('H'));
        // H is a key no row takes: the card's edge flares
        let all = items(&waiting, Page::Main);
        assert_eq!(answer(Key::Char('h'), Page::Main, &all, None), Did::Nowhere);
        // The arrows pass over it: Down from Shell in its folder goes to Wait on…
        assert_eq!(answer(Key::Down, Page::Main, &all, Some(Shell)), Did::Focus(Some(WaitOn)));
        assert_eq!(answer(Key::Up, Page::Main, &all, Some(WaitOn)), Did::Focus(Some(Shell)));
        // The pointer on it lights nothing and a click there picks nothing, but it's on the card
        let l = lay((100.0, 100.0), (1200.0, 900.0), &waiting, Page::Main);
        assert_eq!(l.inert, vec![Hide]);
        let r = l.rows.iter().find(|(i, _)| *i == Some(Hide)).unwrap().1;
        assert_eq!(l.hit(r.x + 20.0, r.y + r.h / 2.0), None);
        assert!(l.card.contains(r.x + 20.0, r.y + r.h / 2.0));
        // Its words get the whole row, there being no key to leave room for
        assert_eq!(l.words_box(Hide, r).w, r.w - 2.0 * INSET);
        // Laid out the same as the live row, so the card doesn't jump when the answer comes
        let live = lay((100.0, 100.0), (1200.0, 900.0), &claude(), Page::Main);
        assert_eq!(live.rows, l.rows);
        assert_eq!(live.hit(r.x + 20.0, r.y + r.h / 2.0), Some(Hide));
    }
}

#[test]
fn a_shell_tab_has_no_copy_link_or_wait_on() {
    use Item::*;
    assert_eq!(
        rows(&shell(), Page::Main),
        vec![
            Some(Rename),
            Some(Colour(None)),
            None,
            Some(NewSession),
            Some(Shell),
            None,
            Some(Link(0)),
            Some(Link(1)),
            None,
            Some(NewWindow),
            None,
            Some(CloseFinished),
            Some(CloseOthers),
            Some(Close),
        ]
    );
}

#[test]
fn wait_on_needs_links_on_and_another_claude_tab() {
    let off = Facts {
        links_on: false,
        ..claude()
    };
    assert!(!items(&off, Page::Main).contains(&Item::WaitOn), "links off");
    let alone = Facts {
        others: vec![],
        ..claude()
    };
    assert!(!items(&alone, Page::Main).contains(&Item::WaitOn), "no other Claude tab");
    assert!(items(&claude(), Page::Main).contains(&Item::WaitOn));
}

#[test]
fn new_session_needs_a_project_and_names_it() {
    let nowhere = Facts {
        project: None,
        ..claude()
    };
    assert!(!items(&nowhere, Page::Main).contains(&Item::NewSession));
    assert_eq!(label(Item::NewSession, &claude()), "New session in Warden");
}

#[test]
fn close_finished_tabs_only_while_one_qualifies_and_counts_them() {
    let none = Facts {
        finished: vec![],
        ..claude()
    };
    assert!(!items(&none, Page::Main).contains(&Item::CloseFinished));
    assert_eq!(label(Item::CloseFinished, &claude()), "Close finished tabs (1)");
}

#[test]
fn with_nothing_else_the_menu_is_todays_in_the_new_order() {
    use Item::*;
    assert_eq!(
        rows(&bare(), Page::Main),
        vec![
            Some(Rename),
            Some(Colour(None)),
            None,
            Some(Shell),
            None,
            Some(NewWindow),
            None,
            Some(CloseOthers),
            Some(Close),
        ]
    );
}

#[test]
fn rules_only_between_groups_with_rows() {
    // Every mix of what decides the rows: never a rule first, last or after another
    for bits in 0..256u32 {
        let on = |b: u32| bits & (1 << b) != 0;
        let facts = Facts {
            claude: on(0),
            project: on(1).then(|| "Warden".into()),
            links_on: on(2),
            links: if on(3) { vec!["a link".into()] } else { vec![] },
            others: if on(4) { vec![other(2, "Other")] } else { vec![] },
            finished: if on(5) {
                vec![Named {
                    tab: 2,
                    label: "Other".into(),
                }]
            } else {
                vec![]
            },
            conductor: on(6).then_some(Offer::Hide),
            park: on(7).then_some(false),
        };
        let r = rows(&facts, Page::Main);
        assert!(r.first().unwrap().is_some() && r.last().unwrap().is_some(), "{:?}", facts);
        assert!(r.windows(2).all(|w| w[0].is_some() || w[1].is_some()), "{:?}", facts);
        // Rename, Colour, Shell, Move to new window, Close other tabs and Close tab are always there
        let it = items(&facts, Page::Main);
        for always in [Item::Rename, Item::Colour(None), Item::Shell, Item::NewWindow, Item::CloseOthers, Item::Close] {
            assert!(it.contains(&always), "{:?} in {:?}", always, facts);
        }
    }
}

#[test]
fn every_key_is_one_letter_and_unique() {
    let every = [
        Item::Rename,
        Item::Colour(None),
        Item::CopyLink,
        Item::NewSession,
        Item::Shell,
        Item::Hide,
        Item::WaitOn,
        Item::Link(0),
        Item::NewWindow,
        Item::CloseFinished,
        Item::CloseOthers,
        Item::Close,
        Item::Back,
        Item::On(0),
    ];
    let keys: Vec<char> = every.iter().filter_map(|&i| key(i)).collect();
    assert_eq!(keys, vec!['R', 'L', 'N', 'S', 'H', 'W', 'M', 'F', 'O', 'X']);
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), keys.len(), "a key twice: {:?}", keys);
    assert!(keys.iter().all(|k| k.is_ascii_uppercase()));
}

#[test]
fn a_key_picks_its_row_when_the_row_is_there() {
    let all = items(&claude(), Page::Main);
    for &item in &all {
        if let Some(k) = key(item) {
            assert_eq!(by_key(k, &all), Some(item));
            assert_eq!(by_key(k.to_ascii_lowercase(), &all), Some(item), "either case");
        }
    }
    // A shell tab has no Copy link or Wait on…: their keys go nowhere
    let shell = items(&shell(), Page::Main);
    assert_eq!(by_key('l', &shell), None);
    assert_eq!(by_key('w', &shell), None);
    assert_eq!(by_key('f', &items(&bare(), Page::Main)), None, "nothing finished");
    assert_eq!(by_key('q', &all), None, "no row takes it");
}

#[test]
fn digits_pick_the_swatches() {
    let all = items(&bare(), Page::Main);
    assert_eq!(by_key('0', &all), Some(Item::Colour(None)));
    assert_eq!(by_key('1', &all), Some(Item::Colour(Some(0))));
    assert_eq!(by_key('8', &all), Some(Item::Colour(Some(7))));
    assert_eq!(by_key('9', &all), None, "eight colours");
    // Wait on…'s page has no swatches
    assert_eq!(by_key('1', &items(&claude(), Page::WaitOn)), None);
}

fn rail_tab(tab: usize) -> RailTab {
    RailTab {
        tab,
        label: format!("Tab {}", tab),
        claude: true,
        conductor: false,
        status: "Done".into(),
        unread: false,
        agents: 0,
        waiting: 0,
        linked: false,
        mark: Some(Mark::Done),
        colour: 0x5b7cff,
    }
}

fn closes(t: RailTab) -> bool {
    !finished(&[t]).is_empty()
}

#[test]
fn close_finished_takes_a_done_tab_read_and_still_with_its_green_dot() {
    assert!(closes(rail_tab(1)));
    assert!(closes(RailTab {
        status: "Your turn".into(),
        ..rail_tab(1)
    }));
}

#[test]
fn close_finished_leaves_a_tab_that_asks_or_works() {
    for status in ["Question", "Extras", "Permission", "Login", "Blocked", "Failed", "Working", "Stuck? 8m", ""] {
        assert!(
            !closes(RailTab {
                status: status.into(),
                ..rail_tab(1)
            }),
            "{}",
            status
        );
    }
}

#[test]
fn close_finished_leaves_an_unread_tab() {
    assert!(!closes(RailTab {
        unread: true,
        ..rail_tab(1)
    }));
}

#[test]
fn close_finished_leaves_a_tab_with_agents_or_tasks() {
    assert!(!closes(RailTab {
        agents: 1,
        ..rail_tab(1)
    }));
    assert!(!closes(RailTab {
        waiting: 2,
        ..rail_tab(1)
    }));
}

/// A tab waiting on another through a tab link isn't finished, whatever its session said as it
/// handed the turn back: links.ts means to tell it to carry on (PITFALLS 153)
#[test]
fn close_finished_leaves_a_tab_that_waits_on_another() {
    for status in ["Done", "Your turn"] {
        assert!(!closes(RailTab {
            linked: true,
            status: status.into(),
            ..rail_tab(1)
        }));
    }
    let rail = [
        rail_tab(1),
        RailTab {
            linked: true,
            ..rail_tab(2)
        },
    ];
    let listed = vec![
        Named { tab: 1, label: "Tab 1".into() },
        Named { tab: 2, label: "Tab 2".into() },
    ];
    assert_eq!(to_close(&listed, &rail), vec![1]);
    // Still on Wait on…'s page: a tab can be waited on whatever it waits on itself
    assert_eq!(others(&rail, 1).len(), 1);
}

#[test]
fn close_finished_needs_the_green_dot() {
    for mark in [None, Some(Mark::FirstDone), Some(Mark::Drifted), Some(Mark::Waiting), Some(Mark::Open)] {
        assert!(!closes(RailTab { mark, ..rail_tab(1) }), "{:?}", mark);
    }
}

#[test]
fn close_finished_leaves_the_conductors_tab_and_shells() {
    assert!(!closes(RailTab {
        conductor: true,
        ..rail_tab(1)
    }));
    assert!(!closes(RailTab {
        claude: false,
        ..rail_tab(1)
    }));
}

#[test]
fn close_finished_keeps_the_rails_order() {
    let rail = [
        rail_tab(3),
        RailTab {
            unread: true,
            ..rail_tab(5)
        },
        rail_tab(1),
    ];
    let f = Facts::from_rail(3, true, None, true, vec![], &rail);
    assert_eq!(
        f.finished,
        vec![
            Named {
                tab: 3,
                label: "Tab 3".into()
            },
            Named {
                tab: 1,
                label: "Tab 1".into()
            }
        ]
    );
}

#[test]
fn the_second_line_lists_them_by_label() {
    let f = Facts {
        finished: ["Tab Wave", "Site publish", "Warden temps"]
            .iter()
            .enumerate()
            .map(|(tab, l)| Named {
                tab,
                label: l.to_string(),
            })
            .collect(),
        ..bare()
    };
    assert_eq!(finished_words(&f), "Tab Wave, Site publish, Warden temps");
    // Cut to the line's width as the words are measured, an ellipsis where it's cut
    assert_eq!(finished_line(&f, 1000.0, measure), "Tab Wave, Site publish, Warden temps");
    assert_eq!(finished_line(&f, 20.0 * CELL, measure), "Tab Wave, Site publ\u{2026}");
    // Spaces at the cut go before the ellipsis
    assert_eq!(finished_line(&f, 11.0 * CELL, measure), "Tab Wave,\u{2026}");
}

#[test]
fn a_cut_is_the_longest_that_fits_by_what_the_gui_measures() {
    // A proportional font: an i is narrow, an m wide
    let width = |w: &str| w.chars().map(|c| if c == 'i' { 3.0 } else if c == 'm' { 12.0 } else { 7.0 }).sum::<f64>();
    let text = "iiiiiiii mmmmmmmm";
    for room in [0.0, 6.0, 7.0, 20.0, 31.0, 40.0, 80.0, 150.0, 200.0, 1000.0] {
        let cut = cut_to(text, room, width);
        assert!(width(&cut) <= room || cut.is_empty(), "{} at {}", cut, room);
        if cut != text && !cut.is_empty() {
            // One character more wouldn't have fitted
            let kept = cut.trim_end_matches('\u{2026}').chars().count();
            let more: String = text.chars().take(kept + 1).collect();
            assert!(width(&format!("{}\u{2026}", more.trim_end())) > room || more.ends_with(' '), "{} at {}", cut, room);
        }
    }
    assert_eq!(cut_to(text, 6.0, width), "", "not even the ellipsis fits");
    assert_eq!(cut_to(text, 7.0, width), "\u{2026}");
    assert_eq!(cut_to(text, 31.0, width), "iiiiiiii\u{2026}");
    assert_eq!(cut_to("", 0.0, width), "");
}

#[test]
fn wait_on_lists_the_other_claude_tabs_with_their_pads_colours() {
    let rail = [
        rail_tab(1),
        RailTab {
            claude: false,
            ..rail_tab(2)
        },
        RailTab {
            conductor: true,
            ..rail_tab(3)
        },
        RailTab {
            colour: 0xff8a3d,
            status: "Question".into(),
            ..rail_tab(4)
        },
    ];
    let f = Facts::from_rail(1, true, None, true, vec![], &rail);
    assert_eq!(
        f.others,
        vec![Other {
            tab: 4,
            label: "Tab 4".into(),
            colour: 0xff8a3d,
        }]
    );
    assert_eq!(rows(&f, Page::WaitOn), vec![Some(Item::Back), Some(Item::On(0))]);
    assert_eq!(label(Item::Back, &f), "\u{2039} Back");
    assert_eq!(label(Item::On(0), &f), "Tab 4");
    assert!(items(&f, Page::Main).contains(&Item::WaitOn));
}

#[test]
fn wait_ons_page_is_laid_out_and_hit_like_the_menu() {
    let f = Facts {
        others: vec![other(4, "Tab Wave"), other(6, "Site publish")],
        ..claude()
    };
    let l = lay((100.0, 50.0), (1200.0, 900.0), &f, Page::WaitOn);
    assert_eq!((l.card.x, l.card.y), (100.0, 50.0), "where the menu opened");
    assert_eq!(l.card.h, 2.0 * PAD + 3.0 * ROW);
    assert!(l.swatches.is_empty() && l.second.is_none());
    let middle = |r: &Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (x, y) = middle(&l.rows[0].1);
    assert_eq!(l.hit(x, y), Some(Item::Back));
    let (x, y) = middle(&l.rows[2].1);
    assert_eq!(l.hit(x, y), Some(Item::On(1)));
    let page = items(&f, Page::WaitOn);
    assert_eq!(step(Item::Back, 1, 0, &page), Item::On(0));
    assert_eq!(step(Item::Back, -1, 0, &page), Item::On(1), "wraps");
}

#[test]
fn wait_ons_tabs_have_their_pads_dot_before_their_words() {
    let f = Facts {
        others: vec![other(4, "Tab Wave"), other(6, "Site publish")],
        ..claude()
    };
    let l = lay((100.0, 50.0), (1200.0, 900.0), &f, Page::WaitOn);
    let (back, back_row) = l.rows[0];
    let (on, on_row) = l.rows[2];
    assert_eq!((back, on), (Some(Item::Back), Some(Item::On(1))));
    assert_eq!(l.dot_box(Item::Back, back_row), None, "‹ Back has no dot");
    let dot = l.dot_box(Item::On(1), on_row).unwrap();
    assert_eq!((dot.w, dot.h), (DOT, DOT));
    assert_eq!(dot.x, on_row.x + INSET);
    assert_eq!(dot.y + dot.h / 2.0, on_row.y + ROW / 2.0, "centred on the row's line");
    let words = l.words_box(Item::On(1), on_row);
    assert_eq!(words.x, dot.x + DOT + DOT_GAP, "the label after the dot");
    assert_eq!(words.x + words.w, on_row.x + on_row.w - INSET, "no key's room: the page's rows have no keys");
    assert_eq!(l.words_box(Item::Back, back_row).x, back_row.x + INSET);
    // The widest tab's words, its dot's room and the card's insets
    assert_eq!(l.card.w, WIDTH.max(measure("Site publish") + DOT + DOT_GAP + 2.0 * PAD + 2.0 * INSET));
    let long = Facts {
        others: vec![other(4, "A label long enough to widen the card past its 260 px")],
        ..claude()
    };
    let wide = lay((100.0, 50.0), (1200.0, 900.0), &long, Page::WaitOn);
    let (_, row) = wide.rows[1];
    assert_eq!(
        wide.words_box(Item::On(0), row).w,
        measure("A label long enough to widen the card past its 260 px"),
        "the dot's room is measured in, so the label fits"
    );
}

#[test]
fn close_finished_takes_only_what_it_listed_and_still_qualifies() {
    let named = |tab: usize| Named {
        tab,
        label: format!("Tab {}", tab),
    };
    let listed = vec![named(1), named(2), named(3)];
    // 2 started working again since the menu opened, and 4 finished since: neither is taken
    let rail = vec![
        rail_tab(1),
        RailTab {
            status: "Working".into(),
            ..rail_tab(2)
        },
        rail_tab(3),
        rail_tab(4),
    ];
    assert_eq!(to_close(&listed, &rail), vec![1, 3]);
    // In the rail's order now, whatever order they were listed in
    assert_eq!(to_close(&[named(3), named(1)], &rail), vec![1, 3]);
    // A tab gone from the rail since isn't there to close
    assert_eq!(to_close(&listed, &rail[2..]), vec![3]);
    assert!(to_close(&[], &rail).is_empty());
}

#[test]
fn typing_replaces_the_old_name() {
    let mut n = Name::new("bravo");
    assert!(n.selected);
    for c in "moved".chars() {
        n.type_char(c);
    }
    assert_eq!(n.text, "moved");
    assert!(!n.selected);
}

#[test]
fn backspace_clears_the_selection_then_deletes() {
    let mut n = Name::new("bravo");
    n.backspace();
    assert_eq!(n.text, "");
    n.type_char('a');
    n.type_char('b');
    n.backspace();
    assert_eq!(n.text, "a");
}

#[test]
fn an_arrow_keeps_the_old_name_to_edit() {
    let mut n = Name::new("bravo");
    n.deselect();
    n.type_char('2');
    assert_eq!(n.text, "bravo2");
    n.backspace();
    n.backspace();
    assert_eq!(n.text, "brav");
}

#[test]
fn nothing_to_select_in_an_empty_name() {
    assert!(!Name::new("").selected);
}

#[test]
fn opens_down_and_right_of_the_pointer() {
    let l = lay((100.0, 50.0), (1200.0, 900.0), &bare(), Page::Main);
    assert_eq!((l.card.x, l.card.y), (100.0, 50.0));
    assert_eq!(l.card.w, WIDTH);
    // nine rows: six items and three rules
    assert_eq!(l.rows.len(), 9);
    assert_eq!(l.card.h, 2.0 * PAD + 6.0 * ROW + 3.0 * RULE);
}

#[test]
fn flips_to_stay_in_the_window() {
    let l = lay((1150.0, 880.0), (1200.0, 900.0), &claude(), Page::Main);
    assert!(l.card.x + l.card.w <= 1200.0);
    assert!(l.card.y + l.card.h <= 900.0);
    assert_eq!(l.card.x, 1150.0 - l.card.w);
}

#[test]
fn rows_and_swatches_are_hit() {
    let l = lay((0.0, 0.0), (1200.0, 900.0), &bare(), Page::Main);
    let middle = |r: &Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let (x, y) = middle(&l.rows[0].1);
    assert_eq!(l.hit(x, y), Some(Item::Rename));
    let (x, y) = middle(&l.rows[8].1);
    assert_eq!(l.hit(x, y), Some(Item::Close));
    // a rule does nothing
    let (x, y) = middle(&l.rows[2].1);
    assert_eq!(l.hit(x, y), None);
    // the first swatch is "no colour", the next the first colour
    let (x, y) = middle(&l.swatches[0].1);
    assert_eq!(l.hit(x, y), Some(Item::Colour(None)));
    let (x, y) = middle(&l.swatches[1].1);
    assert_eq!(l.hit(x, y), Some(Item::Colour(Some(0))));
    // outside the card
    assert_eq!(l.hit(900.0, 800.0), None);
}

#[test]
fn swatches_fit_beside_the_label() {
    let l = lay((0.0, 0.0), (1200.0, 900.0), &bare(), Page::Main);
    assert_eq!(l.swatches.len(), COLOURS.len() + 1);
    assert!(l.label_width > 50.0, "{}", l.label_width);
    let last = l.swatches.last().unwrap().1;
    assert!(last.x + last.w <= l.card.x + l.card.w);
    let colour = l.rows[1].1;
    let words = l.words_box(Item::Colour(None), colour);
    assert!(words.x + words.w <= l.swatches[0].1.x, "the words stop short of the swatches");
}

#[test]
fn close_finished_tabs_has_a_second_line() {
    let l = lay((0.0, 0.0), (1200.0, 900.0), &claude(), Page::Main);
    let (_, row) = *l.rows.iter().find(|(r, _)| *r == Some(Item::CloseFinished)).unwrap();
    assert_eq!(row.h, ROW + LINE);
    let second = l.second.expect("its second line");
    let first = l.words_box(Item::CloseFinished, row);
    assert_eq!(second.y, first.y + first.h, "right under its words");
    assert!(second.y + second.h <= row.y + row.h);
    assert!(second.x + second.w <= l.card.x + l.card.w);
    // The rows under it start under it
    let (_, next) = l.rows.iter().skip_while(|(r, _)| *r != Some(Item::CloseFinished)).nth(1).unwrap();
    assert_eq!(next.y, row.y + row.h);
}

#[test]
fn a_keyed_rows_words_leave_room_for_its_key() {
    let l = lay((0.0, 0.0), (1200.0, 900.0), &claude(), Page::Main);
    let row = l.rows[0].1;
    let words = l.words_box(Item::Rename, row);
    let key = l.key_box(row);
    assert!(words.x + words.w <= key.x);
    assert!(key.x + key.w <= row.x + row.w);
    // A link's row has no key: its words run to the end
    let (_, link) = *l.rows.iter().find(|(r, _)| *r == Some(Item::Link(0))).unwrap();
    assert!(l.words_box(Item::Link(0), link).w > words.w);
}

#[test]
fn the_keyboard_skips_the_rules_and_wraps() {
    let all = items(&bare(), Page::Main);
    assert_eq!(step(Item::Rename, 1, 0, &all), Item::Colour(None));
    assert_eq!(step(Item::Colour(Some(2)), 1, 0, &all), Item::Shell);
    assert_eq!(step(Item::NewWindow, 1, 0, &all), Item::CloseOthers);
    assert_eq!(step(Item::Close, 1, 0, &all), Item::Rename);
    assert_eq!(step(Item::Rename, -1, 0, &all), Item::Close);
    // along the swatches, wrapping
    assert_eq!(step(Item::Colour(None), 0, 1, &all), Item::Colour(Some(0)));
    assert_eq!(step(Item::Colour(None), 0, -1, &all), Item::Colour(Some(COLOURS.len() - 1)));
    // through the link rows one by one, not as one row
    let all = items(&claude(), Page::Main);
    assert_eq!(step(Item::WaitOn, 1, 0, &all), Item::Link(0));
    assert_eq!(step(Item::Link(0), 1, 0, &all), Item::Link(1));
    assert_eq!(step(Item::Link(1), 1, 0, &all), Item::NewWindow);
    assert!(same_row(Item::Colour(None), Item::Colour(Some(3))) && !same_row(Item::Link(0), Item::Link(1)));
}

#[test]
fn the_widest_row_is_measured_with_its_keys_room() {
    // Only what the page shows, the swatches' row left out
    let f = claude();
    assert_eq!(widest(&f, Page::Main, 1.0, measure), measure("Close finished tabs (1)") + KEY);
    let keyed = widest(&bare(), Page::Main, 1.0, measure);
    assert_eq!(keyed, measure("Shell in its folder") + KEY);
    // A link's row has no key
    let long = "x".repeat(40);
    let linked = Facts {
        links: vec![long.clone()],
        ..bare()
    };
    assert_eq!(widest(&linked, Page::Main, 1.0, measure), measure(&long));
    // Wait on…'s tabs have no key, but their pad's dot before their words
    assert_eq!(widest(&f, Page::WaitOn, 1.0, measure), measure("Tab Wave") + DOT + DOT_GAP);
    // Twice the scale, twice the key's room
    assert_eq!(widest(&bare(), Page::Main, 2.0, measure), measure("Shell in its folder") + 2.0 * KEY);
    // Measured by what the GUI lays out, not by characters: a narrow font makes a narrow menu
    assert!(widest(&bare(), Page::Main, 1.0, |w| w.len() as f64 * 4.0) < keyed);
}

#[test]
fn a_long_row_widens_the_menu_inside_the_window() {
    let with = |links: Vec<String>, project: Option<&str>| Facts {
        links,
        project: project.map(String::from),
        ..bare()
    };
    let short = lay((0.0, 0.0), (1200.0, 900.0), &with(vec!["Stop waiting".into()], None), Page::Main);
    assert_eq!(short.card.w, WIDTH, "short words: as ever");
    let long = "x".repeat(50);
    let wide = lay((0.0, 0.0), (1200.0, 900.0), &with(vec![long.clone()], None), Page::Main);
    assert_eq!(wide.card.w, 50.0 * CELL + 2.0 * PAD + 2.0 * INSET);
    let last = wide.swatches.last().unwrap().1;
    assert!(last.x + last.w <= wide.card.x + wide.card.w);
    let narrow = lay((0.0, 0.0), (500.0, 900.0), &with(vec!["x".repeat(120)], None), Page::Main);
    assert_eq!(narrow.card.w, 500.0 - 2.0 * MARGIN, "no wider than the window");
    // New session names the project, with its key's room beside it
    let project = "A Project With A Long Name";
    let named = lay((0.0, 0.0), (1200.0, 900.0), &with(vec![], Some(project)), Page::Main);
    let words = (15 + project.len()) as f64 * CELL;
    assert_eq!(named.card.w, words + KEY + 2.0 * PAD + 2.0 * INSET);
}

#[test]
fn copy_link_names_the_project_unless_the_label_does() {
    let id = "6f1c0d4e-0000-4000-8000-000000000000";
    assert_eq!(
        copy_link("Tab Wave", id, Some("Terminal")),
        format!("[Tab Wave](lbterm://session/{}) in Terminal", id)
    );
    assert_eq!(copy_link("InMail · Lantern", id, Some("Lantern")), format!("[InMail · Lantern](lbterm://session/{})", id));
    assert_eq!(copy_link("inmail lantern", id, Some("Lantern")), format!("[inmail lantern](lbterm://session/{})", id), "any case");
    // The project's name has to be a word of its own at the label's end
    assert_eq!(
        copy_link("Heatwave", id, Some("Wave")),
        format!("[Heatwave](lbterm://session/{}) in Wave", id)
    );
    assert_eq!(copy_link("Tab Wave", id, None), format!("[Tab Wave](lbterm://session/{})", id));
}

#[test]
fn a_key_answers_as_its_row_would_and_no_row_brightens_the_hairline() {
    use Item::*;
    let items = items(&claude(), Page::Main);
    // A row's letter, in either case, picks it; a digit picks its swatch
    assert_eq!(answer(Key::Char('r'), Page::Main, &items, None), Did::Pick(Rename));
    assert_eq!(answer(Key::Char('X'), Page::Main, &items, Some(Shell)), Did::Pick(Close));
    assert_eq!(answer(Key::Char('0'), Page::Main, &items, None), Did::Pick(Colour(None)));
    assert_eq!(answer(Key::Char('3'), Page::Main, &items, None), Did::Pick(Colour(Some(2))));
    // Delete is Close tab
    assert_eq!(answer(Key::Delete, Page::Main, &items, None), Did::Pick(Close));
    // A letter no row has, a digit past the swatches, punctuation, a key with Ctrl: nowhere
    for key in [Key::Char('q'), Key::Char('9'), Key::Char(';'), Key::Other] {
        assert_eq!(answer(key, Page::Main, &items, None), Did::Nowhere, "{key:?}");
    }
    // A shell tab has no Copy link or Wait on…: their letters go nowhere there
    let shell_items = super::items(&shell(), Page::Main);
    assert_eq!(answer(Key::Char('l'), Page::Main, &shell_items, None), Did::Nowhere);
    assert_eq!(answer(Key::Char('w'), Page::Main, &shell_items, None), Did::Nowhere);
    // A modifier alone does nothing, and lights nothing
    assert_eq!(answer(Key::Modifier, Page::Main, &items, None), Did::Nothing);
}

#[test]
fn enter_with_no_row_lit_lights_the_first_and_then_picks_it() {
    let items = items(&claude(), Page::Main);
    let first = answer(Key::Enter, Page::Main, &items, None);
    assert_eq!(first, Did::Focus(Some(Item::Rename)));
    assert_eq!(answer(Key::Enter, Page::Main, &items, Some(Item::Rename)), Did::Pick(Item::Rename));
    // On Wait on…'s page the first row is Back
    let page = super::items(&claude(), Page::WaitOn);
    assert_eq!(answer(Key::Enter, Page::WaitOn, &page, None), Did::Focus(Some(Item::Back)));
}

#[test]
fn esc_closes_the_menu_and_on_wait_ons_page_goes_back_as_left_does() {
    let items = items(&claude(), Page::Main);
    assert_eq!(answer(Key::Escape, Page::Main, &items, None), Did::Close);
    // Left on the menu moves along the swatches, or stays where nothing is lit
    assert_eq!(answer(Key::Left, Page::Main, &items, None), Did::Focus(None));
    assert_eq!(
        answer(Key::Left, Page::Main, &items, Some(Item::Colour(Some(2)))),
        Did::Focus(Some(Item::Colour(Some(1))))
    );
    let page = super::items(&claude(), Page::WaitOn);
    assert_eq!(answer(Key::Escape, Page::WaitOn, &page, None), Did::Back);
    assert_eq!(answer(Key::Left, Page::WaitOn, &page, Some(Item::On(0))), Did::Back);
    // Its rows have no letters: a letter there goes nowhere
    assert_eq!(answer(Key::Char('r'), Page::WaitOn, &page, None), Did::Nowhere);
}

#[test]
fn the_arrows_light_the_first_or_last_row_from_nothing() {
    let items = items(&claude(), Page::Main);
    assert_eq!(answer(Key::Down, Page::Main, &items, None), Did::Focus(items.first().copied()));
    assert_eq!(answer(Key::Up, Page::Main, &items, None), Did::Focus(items.last().copied()));
}

#[test]
fn a_picked_item_flashes_its_row_or_its_swatch() {
    let facts = claude();
    let laid = lay((40.0, 30.0), (1200.0, 800.0), &facts, Page::Main);
    // A row: its whole rectangle, as the highlight lights it
    let row = |item: Item| laid.rows.iter().find(|(r, _)| *r == Some(item)).map(|(_, r)| *r);
    for item in [Item::Rename, Item::CopyLink, Item::NewSession, Item::WaitOn, Item::CloseFinished, Item::Close] {
        assert_eq!(laid.flash_box(item), row(item), "{item:?}");
        assert!(laid.flash_box(item).is_some(), "{item:?} is on the main page");
    }
    // A colour: its swatch's square, not the Colour row
    let teal = laid.swatches.iter().find(|(c, _)| *c == Some(4)).map(|(_, r)| *r);
    assert_eq!(laid.flash_box(Item::Colour(Some(4))), teal);
    assert!(laid.flash_box(Item::Colour(Some(4))).unwrap().w < laid.card.w / 4.0);
    assert_eq!(
        laid.flash_box(Item::Colour(None)),
        laid.swatches.iter().find(|(c, _)| c.is_none()).map(|(_, r)| *r)
    );
    // Wait on…'s page: ‹ Back and the other tabs have rows; the main page's items don't
    let page = lay((40.0, 30.0), (1200.0, 800.0), &facts, Page::WaitOn);
    assert!(page.flash_box(Item::Back).is_some());
    assert!(page.flash_box(Item::On(0)).is_some());
    assert_eq!(page.flash_box(Item::Rename), None);
    assert_eq!(page.flash_box(Item::Colour(Some(0))), None);
}
