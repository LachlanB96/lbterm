//! LBTerm: the crash-restore prompt (lbterm_extras::restore). When LBTerm starts after one that
//! didn't close cleanly, its first window asks which of the old tabs to open again: all of
//! them (Enter), the ones picked, or none (Esc).

use lbterm_extras::restore::{command, describe, Picks, TabNote};
use mux::termwiztermtab::TermWizTerminal;
use termwiz::cell::{AttributeChange, Intensity};
use termwiz::color::{AnsiColor, ColorAttribute};
use termwiz::input::{InputEvent, KeyCode, KeyEvent, MouseButtons, MouseEvent};
use termwiz::surface::{Change, CursorVisibility, Position};
use termwiz::terminal::Terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    All,
    Picked,
    None,
}

const BUTTONS: [(Button, &str); 3] = [
    (Button::All, " Open all "),
    (Button::Picked, " Open picked "),
    (Button::None, " Don't open "),
];

/// Ask, and return the indices of the tabs to open (empty for none)
pub fn ask(term: &mut TermWizTerminal, tabs: &[TabNote]) -> anyhow::Result<Vec<usize>> {
    term.set_raw_mode()?;
    let rows: Vec<String> = tabs.iter().map(describe).collect();
    let claude: Vec<bool> = tabs.iter().map(|t| command(t, "", "").is_some()).collect();
    let mut picks = Picks::new(tabs.len());
    let mut hover: Option<Button> = None;
    let mut first_shown = 0usize;
    // A drag reports the button held on every move: only the press counts
    let mut was_down = false;

    loop {
        let size = term.get_screen_size()?;
        let x = 2usize;
        let width = size.cols.saturating_sub(2 * x).max(20);
        // heading, blank, rows, blank, help, blank, buttons
        let room = size.rows.saturating_sub(7).max(1);
        if picks.cursor < first_shown {
            first_shown = picks.cursor;
        } else if picks.cursor >= first_shown + room {
            first_shown = picks.cursor + 1 - room;
        }
        let list_top = 2usize;
        let shown = rows.len().min(room);
        let help_row = list_top + shown + 1;
        let button_row = help_row + 2;

        let mut button_spans = vec![];
        let mut bx = x;
        for (button, label) in BUTTONS {
            let label = if button == Button::Picked {
                format!(" Open picked ({}) ", picks.chosen().len())
            } else {
                label.to_string()
            };
            let w = label.chars().count() + 2;
            button_spans.push((button, bx, w, label));
            bx += w + 3;
        }

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorVisibility(CursorVisibility::Hidden),
            Change::CursorPosition {
                x: Position::Absolute(x),
                y: Position::Absolute(0),
            },
            AttributeChange::Intensity(Intensity::Bold).into(),
            Change::Text(format!(
                "LBTerm didn't close cleanly last time. Open its {} tab{} again?",
                rows.len(),
                if rows.len() == 1 { "" } else { "s" }
            )),
            AttributeChange::Intensity(Intensity::Normal).into(),
        ];
        for (line, i) in (first_shown..first_shown + shown).enumerate() {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(x),
                y: Position::Absolute(list_top + line),
            });
            let here = i == picks.cursor;
            if here {
                changes.push(AttributeChange::Reverse(true).into());
            }
            let mark = if picks.is_picked(i) { "[x]" } else { "[ ]" };
            let tag = if claude[i] { "  Claude" } else { "" };
            let text_room = width.saturating_sub(4 + tag.chars().count());
            let text: String = rows[i].chars().take(text_room).collect();
            let pad = text_room.saturating_sub(text.chars().count());
            changes.push(Change::Text(format!("{mark} {text}{}", " ".repeat(pad))));
            if !tag.is_empty() {
                changes.push(AttributeChange::Foreground(AnsiColor::Fuchsia.into()).into());
                changes.push(Change::Text(tag.to_string()));
                changes.push(AttributeChange::Foreground(ColorAttribute::Default).into());
            }
            if here {
                changes.push(AttributeChange::Reverse(false).into());
            }
        }
        changes.push(Change::CursorPosition {
            x: Position::Absolute(x),
            y: Position::Absolute(help_row),
        });
        changes.push(AttributeChange::Foreground(AnsiColor::Grey.into()).into());
        changes.push(Change::Text(
            "Space picks · A all · N none · Enter opens the picked · Esc none"
                .chars()
                .take(width)
                .collect::<String>(),
        ));
        changes.push(AttributeChange::Foreground(ColorAttribute::Default).into());
        for (button, bx, _, label) in &button_spans {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(*bx),
                y: Position::Absolute(button_row),
            });
            let lit = hover == Some(*button);
            if lit {
                changes.push(AttributeChange::Reverse(true).into());
            }
            changes.push(Change::Text(format!("[{label}]")));
            if lit {
                changes.push(AttributeChange::Reverse(false).into());
            }
        }
        term.render(&changes)?;
        term.flush()?;

        let Ok(Some(event)) = term.poll_input(None) else {
            return Ok(vec![]);
        };
        match event {
            InputEvent::Key(KeyEvent { key, .. }) => match key {
                KeyCode::UpArrow | KeyCode::Char('k') => picks.up(),
                KeyCode::DownArrow | KeyCode::Char('j') => picks.down(),
                KeyCode::Char(' ') => picks.toggle(picks.cursor),
                KeyCode::Char('a' | 'A') => picks.set_all(true),
                KeyCode::Char('n' | 'N') => picks.set_all(false),
                KeyCode::Enter => return Ok(picks.chosen()),
                KeyCode::Escape | KeyCode::Char('q' | 'Q') => return Ok(vec![]),
                _ => {}
            },
            InputEvent::Mouse(MouseEvent {
                x: mx,
                y: my,
                mouse_buttons,
                ..
            }) => {
                let (mx, my) = (mx as usize, my as usize);
                let down = mouse_buttons == MouseButtons::LEFT;
                let clicked = down && !was_down;
                was_down = down;
                hover = None;
                if my == button_row {
                    for (button, bx, w, _) in &button_spans {
                        if mx >= *bx && mx < bx + w {
                            hover = Some(*button);
                            if clicked {
                                return Ok(match button {
                                    Button::All => (0..rows.len()).collect(),
                                    Button::Picked => picks.chosen(),
                                    Button::None => vec![],
                                });
                            }
                        }
                    }
                } else if clicked && my >= list_top && my < list_top + shown {
                    let i = first_shown + (my - list_top);
                    picks.cursor = i;
                    picks.toggle(i);
                }
            }
            _ => {}
        }
    }
}
