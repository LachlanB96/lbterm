//! Where each row's words go: the number on the pad and the label under it,
//! which show on the rail, and on the open panel the title and age (or the
//! subagents at work) on the first line, the project and status (or the
//! program's message, when it fits) on the second. The panel's font is
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

        if layout.label > 0.0 {
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
        // at it too
        let corner = if !row.age.is_empty() {
            Some((row.age.clone(), palette::age(row.level)))
        } else if row.agents > 0 {
            let s = if row.agents == 1 { "" } else { "s" };
            Some((format!("{} agent{s}", row.agents), palette::status(row.state)))
        } else {
            None
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
            text: text::fit(&row.title, columns(title_room), width),
            area: Area::new(room.x, first, title_room.max(0.0) + cell, line),
            colour: palette::TITLE,
        });
        if !two_lines {
            continue;
        }

        let second = first + line + layout.px(1.0);
        let mut x = room.x;
        if !row.project.is_empty() {
            let inset = layout.px(4.0);
            let project = text::fit(&row.project, columns(room.w - 2.0 * inset), width);
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
        let left = room.right() - x;
        // The program's message when it fits whole: cut short, it says less
        // than the word
        let status = if !row.message.is_empty() && span(&row.message) <= left {
            &row.message
        } else {
            &row.status
        };
        // Waiting on its own tasks: green, as the work that's still going
        let state = if row.waiting > 0 { RowState::Working } else { row.state };
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
    /// On the open panel: its name, and what its state file says about it
    pub name: String,
    pub says: String,
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
    if layout.label > 0.0 {
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
            head: false,
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
            unread: false,
            age: "12m".into(),
            level: 2,
            bars: [Bar::Green, Bar::Green, Bar::Off],
            active: false,
            label: title.into(),
            waiting: 0,
        }
    }

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|w| w.text.as_str()).collect()
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
}
