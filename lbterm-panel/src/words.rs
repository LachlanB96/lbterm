//! Where each row's words go: the number on the pad and the label under it,
//! which show on the rail, and on the open panel the title and age (or the
//! subagents at work) on the first line, the time since Lachlan's last prompt,
//! the project and status (or the program's message, when it fits) on the second. The panel's font is
//! monospaced, so a word's width is its columns times the cell width.

use crate::claude::{Row, RowState};
use crate::layout::{Area, Layout};
use crate::palette;
use crate::text;

/// One run of text, its box and colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    /// Where it starts and how much room it has; a cell wider than the
    /// text, so the renderer never cuts its last character
    pub area: Area,
    pub colour: u32,
    /// The row it's in
    pub row: usize,
}

/// What a frame's words need to know about the font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Font {
    pub cell_width: f32,
    pub line_height: f32,
}

/// Every visible row's words, split by where they're drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placed {
    /// On the pads: always shown
    pub numbers: Vec<Word>,
    /// Under the pads, in the label font: always shown
    pub labels: Vec<Word>,
    /// Title, age, project and status: only on the open panel
    pub words: Vec<Word>,
    /// Behind the project names
    pub chips: Vec<Area>,
    /// The Conductor's badge's count, in the count's font (the pads' numbers', with the
    /// auto-hinter: the GUI's `tab_panel_count_font`): always shown
    pub counts: Vec<Word>,
    /// The eye-off mark of each tab hidden from the Conductor ([`marks`]), in the marks' font
    /// (the GUI's `tab_panel_mark_font`): always shown, and raised with the numbers over an open
    /// row's highlight
    pub marks: Vec<Word>,
}

/// nf-md-eye_off, the mark on a pad whose tab is hidden from the Conductor
pub const HIDDEN_GLYPH: &str = "\u{f0209}";
/// The chip on the open panel's row of a tab hidden from the Conductor
pub const HIDDEN_CHIP: &str = "Hidden";
/// The chip on the open panel's row of a parked tab (Lachlan's 8a)
pub const PARKED_CHIP: &str = "Parked";

/// A parked row's words on the rail dimmed: its number, its label and its marks, each blended
/// [`palette::PARKED`] of the way from `under(i)`, row `i`'s pill, to its colour, drawn opaque
/// (PITFALLS 248), as its pad and bars are (lbterm/NEW-TAB.md, "Landing 4", 8a). The open
/// panel's words aren't: its "Parked" chip says it
pub fn dim_parked(placed: &mut Placed, rows: &[Row], under: impl Fn(usize) -> u32) {
    let parked = |i: usize| rows.get(i).map_or(false, |r| r.parked);
    for word in placed
        .numbers
        .iter_mut()
        .chain(placed.labels.iter_mut())
        .chain(placed.marks.iter_mut())
        .filter(|w| parked(w.row))
    {
        word.colour = palette::parked(under(word.row), word.colour);
    }
}

/// The eye-off mark of every visible row hidden from the Conductor ([`Row::hidden`]), centred on
/// its disc ([`Layout::hidden_mark`]) in `font`, the marks' font, its glyph `glyph_width` wide;
/// `under(i)` is what's under row `i`'s pad, which its disc is drawn in, so the glyph is white at
/// 60% over it (lbterm/NEW-TAB.md, section 8)
pub fn marks(rows: &[Row], layout: &Layout, font: Font, glyph_width: f32, under: impl Fn(usize) -> u32) -> Vec<Word> {
    rows.iter()
        .enumerate()
        .take_while(|&(i, _)| layout.row_visible(i))
        .filter(|(_, row)| row.hidden)
        .map(|(i, _)| {
            let disc = layout.hidden_mark(i);
            Word {
                row: i,
                text: HIDDEN_GLYPH.to_string(),
                area: Area::new(
                    (disc.x + (disc.w - glyph_width) / 2.0).round(),
                    (disc.y + (disc.h - font.line_height) / 2.0).round(),
                    glyph_width + font.cell_width,
                    font.line_height,
                ),
                colour: palette::hidden_mark(under(i)),
            }
        })
        .collect()
}

/// What covers the labels while the panel is open, where each row's title says the same name
/// beside it (Lachlan's 12a of 2026-11-01, lbterm/NEW-TAB.md, "Landing 4"): for each visible row
/// of the `rows` (the tabs' rows, then the Starting row while there's one) that isn't `raised`,
/// its row and [`Layout::label_cover`]. A raised row's long highlight is drawn whole over its
/// label already, so it needs none. Drawn in the open parts' surface, in the pill's colour, they
/// fade in with the words over the labels drawn with the rows, and the labels fade out. Nothing
/// for a row whose label shows no line.
pub fn label_covers(layout: &Layout, rows: usize, raised: impl Fn(usize) -> bool) -> Vec<(usize, Area)> {
    (0..rows)
        .take_while(|&i| layout.row_visible(i))
        .filter(|&i| !raised(i))
        .filter_map(|i| Some((i, layout.label_cover(i)?)))
        .collect()
}

/// The words of every visible row, the labels in `label`'s font. `width`
/// gives a character's columns.
pub fn place(rows: &[Row], layout: &Layout, font: Font, label: Font, width: impl Fn(char) -> usize + Copy) -> Placed {
    let (cell, line) = (font.cell_width, font.line_height);
    let columns = |room: f32| (room / cell).floor().max(0.0) as usize;
    let span = |s: &str| s.chars().map(width).sum::<usize>() as f32 * cell;
    let mut placed = Placed::default();

    for (i, row) in rows.iter().enumerate() {
        if !layout.row_visible(i) {
            break;
        }
        let pad = layout.pad(i);
        let number = row.index.to_string();
        let w = span(&number);
        placed.numbers.push(Word {
                row: i,
            area: Area::new(
                (pad.x + (pad.w - w) / 2.0).round(),
                (pad.y + (pad.h - line) / 2.0).round(),
                w + cell,
                line,
            ),
            text: number,
            colour: palette::number(row.state),
        });

        // Labels are on and the rail has room for a line of them (Layout::label_lines)
        if layout.label > 0.0 && layout.label(i).h > 0.0 {
            let room = layout.label(i);
            let lines = (room.h / label.line_height).round().max(1.0) as usize;
            let columns = (room.w / label.cell_width).floor().max(0.0) as usize;
            let colour = if row.active { palette::TITLE } else { palette::LABEL };
            for (k, text) in text::wrap(&row.label, columns, lines, width).into_iter().enumerate() {
                let w = text.chars().map(width).sum::<usize>() as f32 * label.cell_width;
                placed.labels.push(Word {
                    row: i,
                    area: Area::new(
                        (room.x + (room.w - w) / 2.0).round(),
                        room.y + k as f32 * label.line_height,
                        w + label.cell_width,
                        label.line_height,
                    ),
                    text,
                    colour,
                });
            }
        }

        let room = layout.text(i);
        let two_lines = room.h >= 2.0 * line + layout.px(2.0);
        let first = if two_lines {
            (room.y + (room.h - 2.0 * line - layout.px(1.0)) / 2.0).round()
        } else {
            (room.y + (room.h - line) / 2.0).round()
        };

        let mut title_room = room.w;
        // How long it has waited, or while it works, how many subagents are
        // at it too and how long it's guessed to have left
        let corner = if !row.age.is_empty() {
            Some((row.age.clone(), palette::age(row.level)))
        } else {
            let s = if row.agents == 1 { "" } else { "s" };
            match (row.agents, row.left.is_empty()) {
                (0, true) => None,
                (0, false) => Some(format!("{} left", row.left)),
                (n, true) => Some(format!("{n} agent{s}")),
                (n, false) => Some(format!("{n} agent{s} · {}", row.left)),
            }
            .map(|text| (text, palette::status(row.state)))
        };
        if let Some((text, colour)) = corner {
            let w = span(&text);
            placed.words.push(Word {
                row: i,
                text,
                area: Area::new(room.right() - w, first, w + cell, line),
                colour,
            });
            title_room -= w + layout.px(8.0);
        }
        placed.words.push(Word {
                row: i,
            // The label first, as the closed rail shows it, so a tab has one name open and closed
            // (Lachlan's pick, 2026-10-25): the name it was given, its session's label, its title
            text: text::fit(&row.label, columns(title_room), width),
            area: Area::new(room.x, first, title_room.max(0.0) + cell, line),
            colour: palette::TITLE,
        });
        if !two_lines {
            continue;
        }

        let second = first + line + layout.px(1.0);
        // What its conversation has cost, at the line's end under the age (Lachlan's E6,
        // 2026-11-07), in the project's grey: the project and the status fit in what's left
        let mut end = room.right();
        if !row.cost.is_empty() {
            let w = span(&row.cost);
            if w + layout.px(8.0) < room.w / 2.0 {
                placed.words.push(Word {
                    row: i,
                    text: row.cost.clone(),
                    area: Area::new(end - w, second, w + cell, line),
                    colour: palette::PROJECT,
                });
                end -= w + layout.px(8.0);
            }
        }
        let mut x = room.x;
        // How long ago Lachlan last prompted it (his ask of 2026-11-11), at the second line's
        // start before the project (his 1c of 2026-11-14, so the titles stay where they were), in
        // a quiet grey; only while the line keeps at least as much room again, so a narrow panel
        // drops it before it cuts the project and status to nothing
        if !row.prompted.is_empty() {
            let w = span(&row.prompted);
            if 2.0 * (w + layout.px(8.0)) <= end - x {
                placed.words.push(Word {
                    row: i,
                    text: row.prompted.clone(),
                    area: Area::new(x, second, w + cell, line),
                    colour: palette::PROMPTED,
                });
                x += w + layout.px(8.0);
            }
        }
        if !row.project.is_empty() {
            let inset = layout.px(4.0);
            let project = text::fit(&row.project, columns(end - x - 2.0 * inset), width);
            let w = span(&project);
            placed
                .chips
                .push(Area::new(x, second - layout.px(1.0), w + 2.0 * inset, line + layout.px(2.0)));
            placed.words.push(Word {
                row: i,
                text: project,
                area: Area::new(x + inset, second, w + cell, line),
                colour: palette::PROJECT,
            });
            x += w + 2.0 * inset + layout.px(6.0);
        }
        // Hidden from the Conductor: a chip in the project's look, its word white at 45%, after
        // the project (lbterm/NEW-TAB.md, section 8), when there's room for it whole
        if row.hidden {
            let inset = layout.px(4.0);
            let w = span(HIDDEN_CHIP);
            if x + w + 2.0 * inset <= end {
                placed
                    .chips
                    .push(Area::new(x, second - layout.px(1.0), w + 2.0 * inset, line + layout.px(2.0)));
                placed.words.push(Word {
                    row: i,
                    text: HIDDEN_CHIP.to_string(),
                    area: Area::new(x + inset, second, w + cell, line),
                    colour: palette::hidden_word(),
                });
                x += w + 2.0 * inset + layout.px(6.0);
            }
        }
        // Parked: a chip in the same look, after the project's and Hidden's (Lachlan's 8a,
        // lbterm/NEW-TAB.md, "Landing 4"), when there's room for it whole
        if row.parked {
            let inset = layout.px(4.0);
            let w = span(PARKED_CHIP);
            if x + w + 2.0 * inset <= end {
                placed
                    .chips
                    .push(Area::new(x, second - layout.px(1.0), w + 2.0 * inset, line + layout.px(2.0)));
                placed.words.push(Word {
                    row: i,
                    text: PARKED_CHIP.to_string(),
                    area: Area::new(x + inset, second, w + cell, line),
                    colour: palette::parked_word(),
                });
                x += w + 2.0 * inset + layout.px(6.0);
            }
        }
        let left = end - x;
        // The program's message when it fits whole: cut short, it says less
        // than the word
        let status = if !row.message.is_empty() && span(&row.message) <= left {
            &row.message
        } else {
            &row.status
        };
        // Waiting on its own tasks: green, as the work that's still going; a tab link's words
        // stay its own colour
        let state = if row.waiting > 0 && row.link.is_none() { RowState::Working } else { row.state };
        if !status.is_empty() && left >= cell {
            placed.words.push(Word {
                row: i,
                text: text::fit(status, columns(left), width),
                area: Area::new(x, second, left + cell, line),
                colour: palette::status(state),
            });
        }
    }
    placed
}

/// What the Conductor's slot says (lbterm_extras::conductor).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeadSays {
    /// On its pad, where a tab's number goes
    pub glyph: String,
    pub glyph_colour: u32,
    /// Under the pad: its mode, or "off"
    pub label: String,
    pub label_colour: u32,
    /// On the pad's corner while its remote control is on
    pub mark: Option<String>,
    /// On the pad's other corner, on a cyan badge: how many tab connections are suggested with a
    /// member on this rail (lbterm_extras::links::badge; his E8), none at 0
    pub badge: Option<String>,
    /// On the open panel: its name, and what its state file says about it
    pub name: String,
    pub says: String,
}

/// How many times the badge's count is drawn over itself. Twice: once, in the auto-hinted face
/// at 96 DPI, a 2 had 9 of its pixels 0x60 or darker, the middle of its greens 99 on the cyan's
/// 204, and twice 19 and 62, its 1's stem 61 a row once and 31 twice (2026-11-02)
pub const COUNT_COATS: usize = 2;

/// Where the Conductor's badge goes ([`HeadSays::badge`], `Layout::head_badge`), sized for its
/// count in `font`, the pads' numbers'; none without one, or without the Conductor's slot
pub fn head_badge(says: &HeadSays, layout: &Layout, font: Font, width: impl Fn(char) -> usize) -> Option<Area> {
    let count = says.badge.as_deref().filter(|b| !b.is_empty() && layout.head)?;
    let w = count.chars().map(width).sum::<usize>() as f32 * font.cell_width;
    Some(layout.head_badge(w, font.line_height))
}

/// Where the Conductor's words go, as a row's do: the glyph on its pad and its label under it,
/// always shown (`numbers`, `labels`, the mark among the numbers), and on the open panel its
/// name above what its state file says (`words`). Their `row` is `usize::MAX`, no tab's.
pub fn head(says: &HeadSays, layout: &Layout, font: Font, label: Font, width: impl Fn(char) -> usize + Copy) -> Placed {
    let mut placed = Placed::default();
    if !layout.head {
        return placed;
    }
    let (cell, line) = (font.cell_width, font.line_height);
    let span = |s: &str, cell: f32| s.chars().map(width).sum::<usize>() as f32 * cell;
    let centred = |text: &str, room: Area, font: Font, colour: u32| {
        let w = span(text, font.cell_width);
        Word {
            row: usize::MAX,
            area: Area::new(
                (room.x + (room.w - w) / 2.0).round(),
                (room.y + (room.h - font.line_height) / 2.0).round(),
                w + font.cell_width,
                font.line_height,
            ),
            text: text.to_string(),
            colour,
        }
    };
    placed.numbers.push(centred(&says.glyph, layout.head_pad(), font, says.glyph_colour));
    if layout.label > 0.0 && layout.head_label().h > 0.0 {
        let room = layout.head_label();
        let columns = (room.w / label.cell_width).floor().max(0.0) as usize;
        let text = text::fit(&says.label, columns, width);
        let mut word = centred(&text, room, label, says.label_colour);
        word.area.y = room.y;
        placed.labels.push(word);
    }
    // In the labels' smaller font
    if let Some(mark) = &says.mark {
        placed.labels.push(centred(mark, layout.head_mark(), label, says.glyph_colour));
    }
    // The count of connections suggested, dark on its cyan badge (the GUI fills the badge), as
    // big as a tab's number on its pad (his "use a bigger one", 2026-11-01), placed by the pads'
    // numbers' font and drawn in the count's (`counts`)
    if let (Some(count), Some(badge)) = (&says.badge, head_badge(says, layout, font, width)) {
        let word = centred(count, badge, font, palette::CONNECTED_INK);
        placed.counts.extend(std::iter::repeat(word).take(COUNT_COATS));
    }

    let room = layout.head_text();
    let columns = (room.w / cell).floor().max(0.0) as usize;
    let two_lines = room.h >= 2.0 * line + layout.px(2.0) && !says.says.is_empty();
    let first = if two_lines {
        (room.y + (room.h - 2.0 * line - layout.px(1.0)) / 2.0).round()
    } else {
        (room.y + (room.h - line) / 2.0).round()
    };
    placed.words.push(Word {
        row: usize::MAX,
        text: text::fit(&says.name, columns, width),
        area: Area::new(room.x, first, room.w + cell, line),
        colour: palette::TITLE,
    });
    if two_lines {
        placed.words.push(Word {
            row: usize::MAX,
            text: text::fit(&says.says, columns, width),
            area: Area::new(room.x, first + line + layout.px(1.0), room.w + cell, line),
            colour: palette::PROJECT,
        });
    }
    placed
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::claude::RowState;
    use crate::idle::Bar;

    const FONT: Font = Font {
        cell_width: 7.0,
        line_height: 16.0,
    };
    const LABEL: Font = Font {
        cell_width: 5.0,
        line_height: 12.0,
    };

    fn one(_: char) -> usize {
        1
    }

    fn layout(rows: usize) -> Layout {
        Layout {
            left: 0.0,
            body_top: 38.0,
            bottom: 800.0,
            right: 1000.0,
            rail: 68.0,
            width: 290.0,
            scale: 1.0,
            rows,
            label: 0.0,
            label_line: 0.0,
            head: false,
            foot: false,
        }
    }

    fn row(index: usize, title: &str) -> Row {
        Row {
            index,
            title: title.into(),
            project: "Terminal".into(),
            state: RowState::Idle,
            status: "Your turn".into(),
            message: String::new(),
            agents: 0,
            progress: None,
            estimated: false,
            left: String::new(),
            agent_bars: vec![],
            unread: false,
            age: "12m".into(),
            level: 2,
            bars: [Bar::Green, Bar::Green, Bar::Off],
            active: false,
            label: title.into(),
            waiting: 0,
            extras: false,
            asks: false,
            link: None,
            quiet: false,
            cost: String::new(),
            prompted: String::new(),
            hidden: false,
            parked: false,
        }
    }

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|w| w.text.as_str()).collect()
    }

    #[test]
    fn the_open_panel_covers_every_label_but_the_raised_rows() {
        // Two-line labels of a 12 px line
        let labelled = |rows: usize| Layout { label: 24.0, label_line: 12.0, ..layout(rows) };
        let l = labelled(4);
        let covers = label_covers(&l, 4, |i| i == 1);
        assert_eq!(covers.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 2, 3]);
        // Over each label's words as they're placed, both lines: the text, if not the cell of
        // slack after it
        let rows: Vec<Row> = (0..4).map(|i| row(i, "Tab bar bugs")).collect();
        let placed = place(&rows, &l, FONT, LABEL, one);
        for (i, area) in &covers {
            assert_eq!(Some(*area), l.label_cover(*i));
            let words: Vec<&Word> = placed.labels.iter().filter(|w| w.row == *i).collect();
            assert!(!words.is_empty(), "row {i} has a label");
            for w in words {
                let text = w.text.chars().count() as f32 * LABEL.cell_width;
                assert!(w.area.x >= area.x && w.area.x + text <= area.right(), "row {i}: {w:?} past {area:?}");
                assert!(w.area.y >= area.y && w.area.bottom() <= area.bottom(), "row {i}: {w:?} past {area:?}");
            }
        }
        // Every row raised: nothing to cover
        assert!(label_covers(&l, 4, |_| true).is_empty());
        // Only the rows the rail shows
        let crowded = labelled(400);
        let shown = (0..400).take_while(|&i| crowded.row_visible(i)).count();
        assert!(shown < 400);
        assert!(label_covers(&crowded, 400, |_| false).len() <= shown);
        // No labels: nothing
        assert!(label_covers(&layout(4), 4, |_| false).is_empty());
    }

    #[test]
    fn the_numbers_sit_centred_on_their_pads() {
        let l = layout(2);
        let placed = place(&[row(1, "a"), row(12, "b")], &l, FONT, LABEL, one);
        assert_eq!(texts(&placed.numbers), ["1", "12"]);
        let pad = l.pad(1);
        let centre = placed.numbers[1].area.x + 14.0 / 2.0;
        assert!((centre - (pad.x + pad.w / 2.0)).abs() <= 0.5);
    }

    #[test]
    fn labels_wrap_under_their_pads_centred() {
        let mut l = layout(2);
        l.label = 2.0 * LABEL.line_height;
        let mut active = row(2, "LBterm tab bar bugs");
        active.label = "Tab bar bugs".into();
        active.active = true;
        let placed = place(&[row(1, "Lodestone"), active], &l, FONT, LABEL, one);
        // 38 px of label room: 7 columns of 5 px
        assert_eq!(texts(&placed.labels), ["Lodest…", "Tab bar", "bugs"]);
        let rows: Vec<usize> = placed.labels.iter().map(|w| w.row).collect();
        assert_eq!(rows, [0, 1, 1]);
        let room = l.label(1);
        let (first, second) = (&placed.labels[1], &placed.labels[2]);
        assert_eq!(first.area.y, room.y);
        assert_eq!(second.area.y, room.y + LABEL.line_height);
        // Centred on the pad
        let centre = |w: &Word| w.area.x + w.text.chars().count() as f32 * LABEL.cell_width / 2.0;
        assert!((centre(second) - (room.x + room.w / 2.0)).abs() <= 1.0);
        assert!(first.area.x >= room.x);
        // The open tab's label is as bright as a title
        assert_eq!(first.colour, palette::TITLE);
        assert_eq!(placed.labels[0].colour, palette::LABEL);
        // Without labels, none
        l.label = 0.0;
        assert!(place(&[row(1, "x")], &l, FONT, LABEL, one).labels.is_empty());
    }

    #[test]
    fn what_a_tab_has_cost_ends_its_second_line_and_the_status_fits_before_it() {
        let l = layout(1);
        let mut r = row(1, "Tab panel");
        r.cost = "$12.34".into();
        r.status = "Working on the rail's labels and its lights".into();
        let placed = place(&[r], &l, FONT, LABEL, one);
        let room = l.text(0);
        let cost = placed.words.iter().find(|w| w.text == "$12.34").expect("the cost");
        assert_eq!(cost.area.x + 6.0 * 7.0, room.right(), "it ends the line, under the age");
        assert_eq!(cost.colour, palette::PROJECT);
        let status = placed.words.iter().find(|w| w.text.starts_with("Working")).expect("the status");
        assert!(status.area.y == cost.area.y && status.area.x < cost.area.x);
        let status_end = status.area.x + status.text.chars().count() as f32 * 7.0;
        assert!(status_end + 8.0 <= cost.area.x, "the status cut short before it: {status_end}");
        assert!(status.text.ends_with('…'));
        // None known: the line as it was
        assert_eq!(texts(&place(&[row(1, "Tab panel")], &l, FONT, LABEL, one).words), ["12m", "Tab panel", "Terminal", "Your turn"]);
    }

    #[test]
    fn open_rows_have_title_age_project_and_status() {
        let l = layout(1);
        let placed = place(&[row(1, "Tab panel")], &l, FONT, LABEL, one);
        assert_eq!(texts(&placed.words), ["12m", "Tab panel", "Terminal", "Your turn"]);
        let room = l.text(0);
        let w = &placed.words;
        // The age ends at the room's right; the title starts past the pill
        assert_eq!(w[0].area.x + 3.0 * 7.0, room.right());
        assert_eq!(w[1].area.x, room.x);
        assert!(w[1].area.x > l.seam(false), "only the open panel shows words");
        // The second line sits below the first, the chip behind the project
        assert!(w[2].area.y > w[1].area.y);
        let chip = placed.chips[0];
        assert!(chip.x < w[2].area.x && chip.right() < w[3].area.x);
        assert!(chip.bottom() <= l.pill(0).bottom());
    }

    #[test]
    fn a_hidden_row_has_its_chip_after_the_project_and_its_mark_on_the_pad() {
        let l = layout(2);
        let mut hidden = row(1, "Tab panel");
        hidden.hidden = true;
        let rows = [hidden, row(2, "Other")];
        let placed = place(&rows, &l, FONT, LABEL, one);
        // The chip after the project's, its word white at 45% over the chip, before the status
        assert_eq!(texts(&placed.words)[..5], ["12m", "Tab panel", "Terminal", "Hidden", "Your turn"]);
        let word = placed.words.iter().find(|w| w.text == HIDDEN_CHIP).unwrap();
        assert_eq!(word.colour, palette::blend(palette::CHIP, 0xFFFFFF, 0.45));
        let (project, chip) = (placed.chips[0], placed.chips[1]);
        assert_eq!((chip.y, chip.h), (project.y, project.h), "in the project chip's look");
        assert!(chip.x > project.right() && chip.x < word.area.x);
        let status = placed.words.iter().find(|w| w.text == "Your turn").unwrap();
        assert!(chip.right() < status.area.x);
        // The other row has no chip of its own but its project's
        assert_eq!(placed.chips.len(), 3);
        // The mark: only on the hidden row, centred on its disc, white at 60% over what's under
        let font = Font { cell_width: 10.0, line_height: 12.0 };
        let marks = marks(&rows, &l, font, 10.0, |i| if i == 0 { 0x101018 } else { 0 });
        assert_eq!(marks.len(), 1);
        let (mark, disc) = (&marks[0], l.hidden_mark(0));
        assert_eq!((mark.row, mark.text.as_str()), (0, HIDDEN_GLYPH));
        assert_eq!(mark.colour, palette::blend(0x101018, 0xFFFFFF, 0.6));
        assert!((mark.area.x + 5.0 - (disc.x + disc.w / 2.0)).abs() <= 0.5);
        assert!((mark.area.y + 6.0 - (disc.y + disc.h / 2.0)).abs() <= 0.5);
        // Never a status colour
        for state in [RowState::Waiting, RowState::Working, RowState::Error] {
            assert_ne!(mark.colour, palette::status(state));
        }
    }

    /// A parked row (Lachlan's 8a): its chip after the project's and Hidden's, and on the rail its
    /// number, label and mark 40% of the way from its pill, opaque; the other rows and the open
    /// panel's words as they were
    #[test]
    fn a_parked_rows_chip_follows_hiddens_and_its_rail_words_are_dimmed() {
        let l = layout(2);
        let mut parked = row(1, "Tab panel");
        parked.parked = true;
        parked.hidden = true;
        let rows = [parked, row(2, "Other")];
        let mut placed = place(&rows, &l, FONT, LABEL, one);
        // Two chips leave the status what room is left: cut short here
        assert_eq!(texts(&placed.words)[..5], ["12m", "Tab panel", "Terminal", "Hidden", "Parked"]);
        assert!(texts(&placed.words)[5].starts_with("Yo"));
        let word = placed.words.iter().find(|w| w.text == PARKED_CHIP).unwrap();
        assert_eq!(word.colour, palette::blend(palette::CHIP, 0xFFFFFF, 0.45));
        assert_eq!(placed.chips.len(), 4, "the project's, Hidden, Parked and the other row's project");
        let font = Font { cell_width: 10.0, line_height: 12.0 };
        let under = |i: usize| if i == 0 { 0x2A2A3A } else { 0x101018 };
        placed.marks = marks(&rows, &l, font, 10.0, under);
        let before = placed.clone();
        dim_parked(&mut placed, &rows, under);
        for (list, was) in [
            (&placed.numbers, &before.numbers),
            (&placed.labels, &before.labels),
            (&placed.marks, &before.marks),
        ] {
            for (w, b) in list.iter().zip(was) {
                let want = if w.row == 0 { palette::blend(0x2A2A3A, b.colour, 0.4) } else { b.colour };
                assert_eq!(w.colour, want, "{:?}", w.text);
            }
        }
        // The parked row has a number and a mark to dim (this layout has room for no labels)
        assert!(placed.numbers.iter().any(|w| w.row == 0) && placed.marks.iter().any(|w| w.row == 0));
        assert_eq!(placed.words, before.words, "the open panel's words aren't dimmed");
        assert_eq!(palette::parked(0x000000, 0xFFFFFF), 0x666666);
    }

    #[test]
    fn a_hidden_rows_chip_gives_way_when_theres_no_room() {
        let mut l = layout(1);
        l.width = l.rail + 60.0;
        let mut hidden = row(1, "Tab panel");
        hidden.hidden = true;
        let placed = place(&[hidden], &l, FONT, LABEL, one);
        assert!(!texts(&placed.words).contains(&"Hidden"));
    }

    #[test]
    fn a_long_title_stops_short_of_the_age() {
        let l = layout(1);
        let long = "a title much too long for the panel to show in one go";
        let placed = place(&[row(1, long)], &l, FONT, LABEL, one);
        let (age, title) = (&placed.words[0], &placed.words[1]);
        assert!(title.text.ends_with('…'));
        let end = title.area.x + title.text.chars().count() as f32 * 7.0;
        assert!(end < age.area.x, "title ends at {end}, age starts at {}", age.area.x);
    }

    #[test]
    fn short_rows_keep_one_line() {
        let mut l = layout(40);
        l.bottom = 38.0 + 10.0 + 22.0 + 40.0 * 28.0 + 10.0;
        let placed = place(&[row(1, "x")], &l, FONT, LABEL, one);
        assert!(placed.chips.is_empty());
        assert_eq!(texts(&placed.words), ["12m", "x"]);
    }

    #[test]
    fn rows_off_the_bottom_have_no_words() {
        let mut l = layout(3);
        // The gutter, the ring, the list's pad, one row, the ring, the gutter
        l.bottom = 38.0 + 4.0 + 1.0 + 8.0 + 46.0 + 1.0 + 4.0;
        let rows = [row(1, "a"), row(2, "b"), row(3, "c")];
        let placed = place(&rows, &l, FONT, LABEL, one);
        assert_eq!(placed.numbers.len(), 1);
    }

    #[test]
    fn a_programs_message_takes_the_statuss_place_only_whole() {
        let l = layout(1);
        let mut r = row(1, "Deploy");
        r.state = RowState::Waiting;
        r.status = "Question".into();
        r.message = "Sydney?".into();
        assert_eq!(texts(&place(&[r.clone()], &l, FONT, LABEL, one).words)[3], "Sydney?");
        r.message = "answer: Which region should the deploy use? (Sydney · Oregon)".into();
        assert_eq!(texts(&place(&[r], &l, FONT, LABEL, one).words)[3], "Question");
    }

    #[test]
    fn a_long_status_stops_at_the_rows_right_edge() {
        let l = layout(1);
        let mut r = row(1, "Deploy");
        r.status = "Your turn, and a very long way past the edge".into();
        let placed = place(&[r], &l, FONT, LABEL, one);
        let status = &placed.words[3];
        assert!(status.text.ends_with('…'), "{}", status.text);
        let end = status.area.x + status.text.chars().count() as f32 * 7.0;
        assert!(end <= l.text(0).right() + 0.5);
    }

    #[test]
    fn subagents_at_work_take_the_ages_place() {
        let l = layout(1);
        let mut r = row(1, "Deploy");
        r.state = RowState::Working;
        r.status = "Working".into();
        r.age.clear();
        r.agents = 2;
        let placed = place(&[r.clone()], &l, FONT, LABEL, one);
        assert_eq!(texts(&placed.words), ["2 agents", "Deploy", "Terminal", "Working"]);
        assert_eq!(placed.words[0].colour, palette::status(RowState::Working));
        // An age, when there is one, wins
        r.age = "3m".into();
        assert_eq!(texts(&place(&[r], &l, FONT, LABEL, one).words)[0], "3m");
    }

    #[test]
    fn the_guessed_time_left_goes_in_the_corner() {
        let l = layout(1);
        let mut r = row(1, "Deploy");
        r.state = RowState::Working;
        r.status = "Working".into();
        r.age.clear();
        r.left = "~4 min".into();
        assert_eq!(texts(&place(&[r.clone()], &l, FONT, LABEL, one).words)[0], "~4 min left");
        r.agents = 2;
        let placed = place(&[r.clone()], &l, FONT, LABEL, one);
        assert_eq!(texts(&placed.words)[0], "2 agents · ~4 min");
        assert_eq!(placed.words[0].colour, palette::status(RowState::Working));
        // The title makes room for it
        assert!(placed.words[1].area.right() <= placed.words[0].area.x + l.px(8.0) + 1.0);
    }

    #[test]
    fn the_time_since_his_last_prompt_starts_the_second_line_before_the_project() {
        let l = layout(1);
        let room = l.text(0);
        let mut r = row(1, "Deploy");
        r.state = RowState::Working;
        r.status = "Working".into();
        r.age.clear();
        r.left = "~4 min".into();
        r.prompted = "4 min".into();
        let placed = place(&[r.clone()], &l, FONT, LABEL, one);
        let at = |text: &str| placed.words.iter().find(|w| w.text == text).unwrap_or_else(|| panic!("{text}"));
        let (since, title, project) = (at("4 min"), at("Deploy"), at("Terminal"));
        // At the second line's start (his 1c of 2026-11-14), in its quiet grey, on the project's
        // line, the title alone at the first line's start as it always was
        assert_eq!((since.area.x, since.area.y), (room.x, project.area.y));
        assert!(since.area.y > title.area.y);
        assert_eq!(since.colour, palette::PROMPTED);
        assert_eq!(title.area.x, room.x);
        // The project's chip after it, a gap between
        let inset = l.px(4.0);
        assert_eq!(project.area.x, room.x + 5.0 * 7.0 + l.px(8.0) + inset);
        assert_eq!(placed.chips[0].x, room.x + 5.0 * 7.0 + l.px(8.0));
        // None while it isn't known: the project at the line's start, as before, the title where
        // it was with a time
        r.prompted.clear();
        let without = place(&[r.clone()], &l, FONT, LABEL, one);
        assert!(without.words.iter().all(|w| w.colour != palette::PROMPTED));
        assert_eq!(without.words.iter().find(|w| w.text == "Terminal").unwrap().area.x, room.x + inset);
        assert_eq!(without.words.iter().find(|w| w.text == "Deploy").unwrap().area, title.area);
        // "just now" too, the status after the project
        r.prompted = "just now".into();
        let placed = place(&[r.clone()], &l, FONT, LABEL, one);
        let since = placed.words.iter().find(|w| w.text == "just now").expect("the time");
        let status = placed.words.iter().find(|w| w.text == "Working").expect("the status");
        assert_eq!(since.area.y, status.area.y);
        assert!(status.area.x > since.area.x + 8.0 * 7.0);
        // A panel too narrow to leave the line as much room again drops it rather than the project
        let narrow = Layout { width: 150.0, ..l };
        let placed = place(&[r], &narrow, FONT, LABEL, one);
        assert!(placed.words.iter().all(|w| w.text != "just now"));
    }

    #[test]
    fn no_claude_means_no_status_words() {
        let l = layout(1);
        let mut r = row(1, "pwsh");
        r.state = RowState::NoClaude;
        r.status = String::new();
        r.age.clear();
        let placed = place(&[r], &l, FONT, LABEL, one);
        assert_eq!(texts(&placed.words), ["pwsh", "Terminal"]);
    }

    #[test]
    fn waiting_on_tasks_reads_in_green() {
        let l = layout(1);
        let mut r = row(1, "Deploy");
        r.waiting = 2;
        r.status = "Waiting on 2 tasks".into();
        let placed = place(&[r], &l, FONT, LABEL, one);
        let status = &placed.words[3];
        assert_eq!(status.text, "Waiting on 2 tasks");
        assert_eq!(status.colour, palette::status(RowState::Working));
    }

    #[test]
    fn the_conductors_words_go_on_its_pad_and_its_open_row() {
        let mut l = layout(2);
        l.label = 24.0;
        let says = HeadSays {
            glyph: "G".into(),
            glyph_colour: palette::CONDUCTOR_ON,
            label: "most".into(),
            label_colour: palette::MODE_MOST,
            mark: Some("R".into()),
            badge: None,
            name: "Conductor".into(),
            says: "Conductor is on (accept most), with remote control since 09:05".into(),
        };
        assert_eq!(head(&says, &l, FONT, LABEL, one), Placed::default(), "no slot, no words");
        l.head = true;
        let placed = head(&says, &l, FONT, LABEL, one);
        // The glyph centred on the pad
        let (glyph, pad) = (&placed.numbers[0], l.head_pad());
        assert_eq!(placed.numbers.len(), 1);
        assert_eq!(glyph.text, "G");
        assert_eq!(glyph.area.x, (pad.x + (pad.w - 7.0) / 2.0).round());
        // The mode under it, in its colour, and the mark on the pad's corner in the label font
        assert_eq!(placed.labels.len(), 2);
        assert_eq!((placed.labels[0].text.as_str(), placed.labels[0].colour), ("most", palette::MODE_MOST));
        assert_eq!(placed.labels[0].area.y, l.head_label().y);
        assert_eq!(placed.labels[1].text, "R");
        assert!(placed.labels[1].area.y < pad.y);
        // Open: the name, and what it says cut to the room
        assert_eq!(texts(&placed.words)[0], "Conductor");
        let room = l.head_text();
        assert!(placed.words[1].text.chars().count() <= (room.w / 7.0) as usize);
        assert!(placed.words.iter().all(|w| w.row == usize::MAX && w.area.x == room.x));
        // Nothing of a tab's
        assert!(placed.chips.is_empty());
    }

    #[test]
    fn the_conductors_count_of_suggested_connections_goes_dark_on_its_badge() {
        let mut l = layout(2);
        l.label = 24.0;
        l.head = true;
        let mut says = HeadSays {
            glyph: "G".into(),
            glyph_colour: palette::CONDUCTOR_ON,
            label: "read".into(),
            label_colour: palette::MODE_READ,
            mark: Some("R".into()),
            ..Default::default()
        };
        // None at 0: no badge, only the glyph in the numbers, and the mode and the mark in the
        // labels
        assert_eq!(head_badge(&says, &l, FONT, one), None);
        let placed = head(&says, &l, FONT, LABEL, one);
        assert_eq!((placed.numbers.len(), placed.labels.len(), placed.counts.len()), (1, 2, 0));
        for (count, scale) in [("1", 1.0), ("2", 1.0), ("12", 1.0), ("1", 1.5), ("12", 1.5), ("1", 2.0), ("12", 2.0)] {
            l.scale = scale;
            let font = Font { cell_width: FONT.cell_width * scale, line_height: FONT.line_height * scale };
            says.badge = Some(count.into());
            let badge = head_badge(&says, &l, font, one).unwrap();
            let w = count.len() as f32 * font.cell_width;
            assert_eq!(badge, l.head_badge(w, font.line_height), "sized for {count} in the pads' numbers' font at {scale}");
            let placed = head(&says, &l, font, LABEL, one);
            // In the counts, which the GUI draws in the count's font, as many times over itself as
            // it's drawn; the numbers have the glyph alone
            assert_eq!(
                (placed.numbers.len(), placed.labels.len(), placed.counts.len()),
                (1, 2, COUNT_COATS),
                "{count} at {scale}"
            );
            let word = &placed.counts[0];
            assert!(placed.counts.iter().all(|w| w == word));
            assert_eq!((word.text.as_str(), word.colour, word.row), (count, palette::CONNECTED_INK, usize::MAX));
            // Centred on it, inside it across
            assert_eq!(word.area.x, (badge.x + (badge.w - w) / 2.0).round());
            assert_eq!(word.area.y, (badge.y + (badge.h - font.line_height) / 2.0).round());
            assert!(word.area.x >= badge.x && word.area.x + w <= badge.right(), "{count} at {scale}: {word:?} in {badge:?}");
        }
        // Two digits make it wider
        l.scale = 1.0;
        says.badge = Some("2".into());
        let two = head_badge(&says, &l, FONT, one).unwrap();
        says.badge = Some("12".into());
        assert!(head_badge(&says, &l, FONT, one).unwrap().w > two.w);
        // Nothing without the Conductor's slot
        l.head = false;
        assert_eq!(head_badge(&says, &l, FONT, one), None);
    }
}
