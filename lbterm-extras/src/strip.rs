//! The window strip's words (lbterm/DESIGN.md, "The window strip"). With the tab panel listing
//! the tabs, the strip shows the active tab's title, then dimmer the summary Terminal's summary
//! worker last wrote for its Claude session, and by the window's buttons when that was ("22:22 ·
//! 2m ago"). Under the pointer, or while Alt+S pins it, the session's detail drops down in a
//! card. The GUI side is wezterm-gui/src/termwindow/render/strip.rs.
//!
//! With the compositor the card has a visual of its own, and DWM fades its opacity in on
//! `pane_enter` as it opens and out on `pane_exit` as it closes, from wherever it is. While it
//! fades out the card it showed is drawn again each paint, so there's something to fade.

use crate::tabs::Rect;
use chrono::NaiveDateTime;
use std::time::Duration;

/// Between the title and the summary
pub const SEPARATOR: &str = " \u{b7} ";
/// The fewest columns a summary is shown in: shorter, it says too little to be worth it
pub const MIN_SUMMARY: usize = 12;
/// What the caption needs left for the title and summary before it's dropped
pub const MIN_WORDS: usize = 30;
/// The columns between the words and the caption
pub const CAPTION_GAP: usize = 2;
/// The widest the detail's text runs, in columns
pub const CARD_COLUMNS: usize = 88;
/// The longest a detail line's label can be ("Finding")
const LABEL_MAX: usize = 12;

/// What the strip shows for the active tab.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Content {
    pub title: String,
    /// Its Claude session's summary, in a line
    pub summary: Option<String>,
    /// When the summary was written, and how long ago
    pub caption: Option<String>,
    /// The session's detail, a row a line
    pub detail: Vec<DetailRow>,
    /// The pane's last prompt on one line, for the strip's second row (lbterm_extras::prompts)
    pub prompt: Option<String>,
    /// A few words in the prompt's place for a moment: why a jump to it didn't get there
    pub note: Option<String>,
    /// A few words in the caption's place for a moment, for what happened to the window: the
    /// config reloaded, the tabs reattached after a restart, a restart under way
    pub notice: Option<String>,
    /// A chip after the title while the tab is in a mode that hides its other panes: zoomed
    pub badge: Option<String>,
}

/// How long a notice stays in the caption's place
pub const NOTICE_FOR: Duration = Duration::from_secs(4);

/// Which of the strip's rows say something else from `old` to `new` in the same tab: the
/// first's words (title, badge, summary, its time or a notice), the second's prompt or note.
/// Those dissolve into the new words (wezterm-gui's render/strip_swap.rs); the detail is the
/// card's.
pub fn changed_rows(old: &Content, new: &Content) -> (bool, bool) {
    let words = old.title != new.title
        || old.summary != new.summary
        || old.caption != new.caption
        || old.notice != new.notice
        || old.badge != new.badge;
    let prompt = old.prompt != new.prompt || old.note != new.note;
    (words, prompt)
}

/// The badge for a zoomed tab: which of its panes shows, in the order the tab lays them out
pub fn zoom_badge(shown: usize, panes: usize) -> String {
    format!("Zoomed \u{b7} {} of {panes}", shown + 1)
}

/// A restarted or reattached window's notice, from the version the GUI before it ran (when a
/// restart started this one), this one's, and how many tabs it found running
pub fn arrival_notice(restarted_from: Option<&str>, running: &str, tabs: usize) -> Option<String> {
    let back = match tabs {
        0 => None,
        1 => Some("1 tab back".to_string()),
        n => Some(format!("{n} tabs back")),
    };
    let what = match restarted_from.map(str::trim) {
        Some(from) if from != running.trim() => format!("Updated to {}", running.trim()),
        Some(_) => "Restarted".to_string(),
        None => {
            return match tabs {
                0 => None,
                1 => Some("Reattached 1 tab".to_string()),
                n => Some(format!("Reattached {n} tabs")),
            }
        }
    };
    Some(match back {
        Some(back) => format!("{what}{SEPARATOR}{back}"),
        None => what,
    })
}

/// One line of the detail: "Goal: find the chats" is labelled "Goal".
#[derive(Debug, Clone, PartialEq)]
pub struct DetailRow {
    pub label: Option<String>,
    pub text: String,
}

/// A detail line split at its label, when it starts with a short word and a colon.
pub fn detail_row(line: &str) -> DetailRow {
    if let Some((label, text)) = line.split_once(':') {
        let label = label.trim();
        if !label.is_empty()
            && label.chars().count() <= LABEL_MAX
            && label.chars().all(char::is_alphabetic)
        {
            return DetailRow {
                label: Some(label.to_string()),
                text: text.trim().to_string(),
            };
        }
    }
    DetailRow {
        label: None,
        text: line.trim().to_string(),
    }
}

/// When a summary was written and how long ago, as the summary pane says it: "22:22 · now",
/// "22:22 · 2m ago", "09:05 · 13h 17m ago", "Tue 22:22 · 2d 3h ago".
pub fn caption(written: NaiveDateTime, now: NaiveDateTime) -> String {
    let age = (now - written).num_seconds().max(0);
    let minutes = age / 60;
    let ago = if minutes < 1 {
        "now".to_string()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else if age < 86_400 {
        format!("{}h {}m ago", age / 3600, (age % 3600) / 60)
    } else {
        format!("{}d {}h ago", age / 86_400, (age % 86_400) / 3600)
    };
    let stamp = if age < 86_400 {
        written.format("%H:%M")
    } else {
        written.format("%a %H:%M")
    };
    format!("{stamp} \u{b7} {ago}")
}

/// How long until [`caption`] says something else: on the next minute of the age, or the next
/// hour once it's a day old.
pub fn caption_changes_in(written: NaiveDateTime, now: NaiveDateTime) -> Duration {
    let age = (now - written).num_milliseconds().max(0) as u64;
    let step = if age < 86_400_000 { 60_000 } else { 3_600_000 };
    Duration::from_millis((age / step + 1) * step - age)
}

fn columns(text: &str) -> usize {
    text.chars().count()
}

/// `text` in at most `width` columns, with an ellipsis where it's cut.
pub fn elide(text: &str, width: usize) -> String {
    if columns(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{}\u{2026}", kept.trim_end())
}

/// What fits on the strip in `width` columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    pub title: String,
    pub summary: Option<String>,
    pub caption: Option<String>,
    /// The caption is a notice, not the summary's time
    pub notice: bool,
    pub badge: Option<String>,
}

/// The columns a badge's chip takes beyond its words: its padding, and the gap before it
pub const BADGE_GAP: usize = 3;

/// Fit the title, summary and caption into `width` columns. The caption goes first when room
/// is short, then the summary; the title keeps its whole length while the summary still has
/// [`MIN_SUMMARY`] columns, and half the room when both are long. A notice takes the caption's
/// place, with or without a summary, and a badge goes after the title, before the summary.
pub fn fit(content: &Content, width: usize) -> Fitted {
    let notice = content.notice.as_ref();
    let caption = notice
        .or(content.summary.as_ref().and(content.caption.as_ref()))
        .filter(|c| width >= columns(c) + CAPTION_GAP + MIN_WORDS);
    let mut room = match caption {
        Some(c) => width - columns(c) - CAPTION_GAP,
        None => width,
    };
    let badge = content
        .badge
        .as_ref()
        .filter(|b| room >= columns(b) + BADGE_GAP + MIN_SUMMARY)
        .cloned();
    if let Some(b) = &badge {
        room -= columns(b) + BADGE_GAP;
    }
    let notice = caption.is_some() && notice.is_some();
    let sep = columns(SEPARATOR);
    let summary = content.summary.as_ref().filter(|_| room >= sep + MIN_SUMMARY + 1);
    match summary {
        Some(summary) => {
            let wanted = columns(&content.title);
            let title_room = wanted.min((room / 2).max(room.saturating_sub(columns(summary) + sep)));
            let title_room = title_room.min(room - sep - MIN_SUMMARY);
            let title = elide(&content.title, title_room);
            let summary_room = room - columns(&title) - sep;
            Fitted {
                title,
                summary: Some(elide(summary, summary_room)),
                caption: caption.cloned(),
                notice,
                badge,
            }
        }
        // The summary's time goes with it; a notice stays
        None => Fitted {
            title: elide(&content.title, room),
            summary: None,
            caption: caption.filter(|_| notice).cloned(),
            notice,
            badge,
        },
    }
}

/// The second row's prompt in `width` columns, and whether it had to be cut to fit (then
/// resting the pointer on it shows all of it)
pub fn fit_prompt(prompt: &str, width: usize) -> (String, bool) {
    let shown = elide(prompt, width);
    let cut = shown != prompt;
    (shown, cut)
}

/// `text` broken at spaces into lines of at most `width` columns; a word longer than that is
/// cut where the line ends.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![];
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_string();
        loop {
            let used = columns(&line);
            let gap = if used == 0 { 0 } else { 1 };
            if used + gap + columns(&word) <= width {
                if gap == 1 {
                    line.push(' ');
                }
                line.push_str(&word);
                break;
            }
            if used > 0 {
                lines.push(std::mem::take(&mut line));
                continue;
            }
            // A word wider than a whole line
            let head: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            lines.push(head);
            if word.is_empty() {
                break;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The detail card, laid out: where it goes and where each line of it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub rect: Rect,
    /// Where the labels start
    pub label_x: f64,
    /// Where the text starts
    pub text_x: f64,
    pub lines: Vec<CardLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    /// The row's label, on its first line
    pub label: Option<String>,
    pub text: String,
    /// The line's top
    pub y: f64,
}

/// Lay out the card for `rows`, its text starting at `text_left` (the strip's words) and its
/// top at `top`, between `min_left` (where the panes' room starts, clear of the tab panel) and
/// `max_right`. `cell` is a column's width and a line's height, in pixels; `scale` is the DPI
/// over 96.
pub fn card(
    rows: &[DetailRow],
    text_left: f64,
    top: f64,
    (min_left, max_right): (f64, f64),
    cell: (f64, f64),
    scale: f64,
) -> Option<Card> {
    if rows.is_empty() || cell.0 <= 0.0 {
        return None;
    }
    let pad = 12.0 * scale;
    let row_gap = 6.0 * scale;
    let label_columns = rows
        .iter()
        .filter_map(|r| r.label.as_ref().map(|l| columns(l)))
        .max()
        .map_or(0, |n| n + 2);
    // Its padding comes out of the panes' room's left edge when the text starts closer to it
    let left = (text_left - pad).max(min_left).min(text_left);
    let fit_columns = ((max_right - text_left - pad) / cell.0).floor().max(0.0) as usize;
    let text_columns = fit_columns
        .saturating_sub(label_columns)
        .min(CARD_COLUMNS)
        .max(8);
    let wrapped: Vec<(Option<String>, Vec<String>)> = rows
        .iter()
        .map(|r| (r.label.clone(), wrap(&r.text, text_columns)))
        .collect();
    let widest = wrapped
        .iter()
        .flat_map(|(_, lines)| lines.iter().map(|l| columns(l)))
        .max()
        .unwrap_or(0);

    let mut lines = vec![];
    let mut y = top + pad;
    for (i, (label, text)) in wrapped.into_iter().enumerate() {
        if i > 0 {
            y += row_gap;
        }
        for (j, line) in text.into_iter().enumerate() {
            lines.push(CardLine {
                label: if j == 0 { label.clone() } else { None },
                text: line,
                y,
            });
            y += cell.1;
        }
    }
    let width = (text_left - left) + pad + (label_columns + widest) as f64 * cell.0;
    Some(Card {
        rect: Rect {
            x: left,
            y: top,
            w: width.ceil(),
            h: (y + pad - top).ceil(),
        },
        label_x: text_left,
        text_x: text_left + label_columns as f64 * cell.0,
        lines,
    })
}

/// What a paint draws of the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardDraw {
    /// It's open: laid out from what the session says now
    Fresh,
    /// It's closing and its visual still shows something: the card it showed, as it was
    Last,
    None,
}

/// `open`: the card shows; `fading`: its visual is still on its way out.
pub fn card_draw(open: bool, fading: bool) -> CardDraw {
    match (open, fading) {
        (true, _) => CardDraw::Fresh,
        (false, true) => CardDraw::Last,
        (false, false) => CardDraw::None,
    }
}

/// How far the card's shadow reaches past it, at `scale` (the DPI over 96): round it, and
/// further below, as it falls
pub fn shadow_reach(scale: f64) -> (f64, f64) {
    (11.0 * scale, 3.0 * scale)
}

/// The pixels the card's visual covers, its shadow included, inside a `window` of that size:
/// x, y, width, height, edges rounded out. `None` when none of it is in the window.
pub fn visual_bounds(card: &Rect, scale: f64, window: (u32, u32)) -> Option<(i32, i32, u32, u32)> {
    let (by, down) = shadow_reach(scale);
    let x0 = (card.x - by).floor().max(0.0) as i32;
    let y0 = (card.y - by).floor().max(0.0) as i32;
    let x1 = ((card.x + card.w + by).ceil() as i32).min(window.0 as i32);
    let y1 = ((card.y + card.h + by + down).ceil() as i32).min(window.1 as i32);
    (x1 > x0 && y1 > y0).then(|| (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, h: u32, m: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day)
            .unwrap()
            .and_hms_opt(h, m, s)
            .unwrap()
    }

    #[test]
    fn the_caption_says_it_as_the_summary_pane_does() {
        let written = at(20, 22, 22, 0);
        assert_eq!(caption(written, at(20, 22, 22, 40)), "22:22 \u{b7} now");
        assert_eq!(caption(written, at(20, 22, 24, 59)), "22:22 \u{b7} 2m ago");
        assert_eq!(caption(written, at(21, 11, 39, 0)), "22:22 \u{b7} 13h 17m ago");
        // A day on, the stamp names the day: 20 October 2026 is a Tuesday
        assert_eq!(caption(written, at(23, 1, 30, 0)), "Tue 22:22 \u{b7} 2d 3h ago");
        // A clock that went back says now, not a negative age
        assert_eq!(caption(written, at(20, 22, 0, 0)), "22:22 \u{b7} now");
    }

    #[test]
    fn the_caption_changes_on_the_minute_then_on_the_hour() {
        let written = at(20, 22, 22, 0);
        assert_eq!(caption_changes_in(written, at(20, 22, 22, 40)), Duration::from_secs(20));
        assert_eq!(caption_changes_in(written, at(20, 23, 0, 0)), Duration::from_secs(60));
        assert_eq!(caption_changes_in(written, at(22, 0, 52, 0)), Duration::from_secs(30 * 60));
        // Each change named is a change of words
        let now = at(20, 22, 22, 40);
        let then = now + chrono::Duration::from_std(caption_changes_in(written, now)).unwrap();
        assert_ne!(caption(written, now), caption(written, then));
    }

    #[test]
    fn detail_lines_keep_their_labels_apart() {
        assert_eq!(
            detail_row("Goal: find the chats that vanished."),
            DetailRow {
                label: Some("Goal".into()),
                text: "find the chats that vanished.".into()
            }
        );
        assert_eq!(detail_row("Finding:closing a tab").label.as_deref(), Some("Finding"));
        // Not a label: a sentence, or a time
        assert_eq!(detail_row("Seven chats restored: all reopen").label, None);
        assert_eq!(detail_row("21:13 to 22:08").label, None);
        assert_eq!(detail_row("  plain words  ").text, "plain words");
    }

    #[test]
    fn eliding_cuts_with_an_ellipsis() {
        assert_eq!(elide("short", 10), "short");
        assert_eq!(elide("exactly ten", 11), "exactly ten");
        assert_eq!(elide("a summary too long", 10), "a summary\u{2026}");
        assert_eq!(elide("abc", 0), "");
    }

    fn content(title: &str, summary: Option<&str>) -> Content {
        Content {
            title: title.into(),
            summary: summary.map(String::from),
            caption: summary.map(|_| "22:22 \u{b7} 2m ago".to_string()),
            ..Content::default()
        }
    }

    const TITLE: &str = "Missing closed chats in AI dashboard";
    const SUMMARY: &str = "AI dashboard: 7 closed chats restored as Interrupted";

    #[test]
    fn a_wide_strip_shows_all_of_it() {
        let f = fit(&content(TITLE, Some(SUMMARY)), 120);
        assert_eq!(f.title, TITLE);
        assert_eq!(f.summary.as_deref(), Some(SUMMARY));
        assert_eq!(f.caption.as_deref(), Some("22:22 \u{b7} 2m ago"));
    }

    #[test]
    fn a_narrow_strip_cuts_the_summary_then_halves_the_title() {
        // 60 columns: the caption takes 16, so 44 for the words; the title is 36
        let f = fit(&content(TITLE, Some(SUMMARY)), 60);
        assert!(f.caption.is_some());
        let title = &f.title;
        let summary = f.summary.as_deref().unwrap();
        assert_eq!(columns(title) + columns(SEPARATOR) + columns(summary), 44);
        assert!(columns(title) <= 22, "{title:?}");
        assert!(summary.ends_with('\u{2026}'));
    }

    #[test]
    fn the_caption_goes_first_then_the_summary() {
        let f = fit(&content(TITLE, Some(SUMMARY)), 40);
        assert_eq!(f.caption, None);
        assert!(f.summary.is_some());
        assert!(columns(&f.title) + columns(SEPARATOR) + columns(f.summary.as_deref().unwrap()) <= 40);
        let f = fit(&content(TITLE, Some(SUMMARY)), 14);
        assert_eq!(f.summary, None);
        assert_eq!(columns(&f.title), 14);
    }

    #[test]
    fn a_tab_without_claude_shows_its_title_alone() {
        let f = fit(&content("pwsh", None), 120);
        assert_eq!(
            f,
            Fitted {
                title: "pwsh".into(),
                summary: None,
                caption: None,
                notice: false,
                badge: None,
            }
        );
    }

    #[test]
    fn a_notice_takes_the_captions_place_with_or_without_a_summary() {
        let mut c = content(TITLE, Some(SUMMARY));
        c.notice = Some("Config reloaded".into());
        let f = fit(&c, 120);
        assert_eq!(f.caption.as_deref(), Some("Config reloaded"));
        assert!(f.notice);
        assert_eq!(f.summary.as_deref(), Some(SUMMARY));
        let mut c = content("pwsh", None);
        c.notice = Some("Reattached 12 tabs".into());
        let f = fit(&c, 120);
        assert_eq!((f.caption.as_deref(), f.notice), (Some("Reattached 12 tabs"), true));
        // Short of room it goes first, as the caption does
        let f = fit(&c, 40);
        assert_eq!((f.caption, f.notice), (None, false));
    }

    #[test]
    fn a_badge_goes_after_the_title_and_the_summary_makes_room() {
        let mut c = content(TITLE, Some(SUMMARY));
        c.badge = Some(zoom_badge(0, 3));
        let f = fit(&c, 120);
        assert_eq!(f.badge.as_deref(), Some("Zoomed \u{b7} 1 of 3"));
        let used = columns(&f.title) + columns(SEPARATOR) + columns(f.summary.as_deref().unwrap());
        let badge = columns(f.badge.as_deref().unwrap()) + BADGE_GAP;
        assert!(used + badge <= 120 - columns("22:22 \u{b7} 2m ago") - CAPTION_GAP, "{used} + {badge}");
        // Narrow, it keeps the badge over the summary's last words
        let f = fit(&c, 60);
        assert!(f.badge.is_some());
        assert!(f.summary.as_deref().unwrap().ends_with('\u{2026}'));
    }

    #[test]
    fn only_the_rows_whose_words_change_dissolve() {
        let old = Content {
            title: "Terminal".into(),
            summary: Some("Building the strip's notices".into()),
            caption: Some("21:04 \u{b7} 2m ago".into()),
            prompt: Some("do all these".into()),
            ..Content::default()
        };
        let mut new = old.clone();
        assert_eq!(changed_rows(&old, &new), (false, false));
        new.caption = Some("21:04 \u{b7} 3m ago".into());
        assert_eq!(changed_rows(&old, &new), (true, false));
        let mut new = old.clone();
        new.prompt = Some("and the rail".into());
        assert_eq!(changed_rows(&old, &new), (false, true));
        let mut new = old.clone();
        new.notice = Some("Config reloaded".into());
        new.note = Some("Claude Code doesn't scroll back that far".into());
        assert_eq!(changed_rows(&old, &new), (true, true));
        let mut new = old.clone();
        new.badge = Some(zoom_badge(1, 2));
        assert_eq!(changed_rows(&old, &new), (true, false));
        // The detail is the card's, not the strip's
        let mut new = old.clone();
        new.detail = vec![detail_row("Goal: notices")];
        assert_eq!(changed_rows(&old, &new), (false, false));
    }

    #[test]
    fn a_restarted_window_says_what_it_came_back_as() {
        let new = "20261021-210000-bd0e7d9";
        assert_eq!(
            arrival_notice(Some("20261021-195310-af39a05"), new, 12).as_deref(),
            Some("Updated to 20261021-210000-bd0e7d9 \u{b7} 12 tabs back")
        );
        assert_eq!(arrival_notice(Some(new), new, 1).as_deref(), Some("Restarted \u{b7} 1 tab back"));
        assert_eq!(arrival_notice(Some(new), new, 0).as_deref(), Some("Restarted"));
        // Started again by hand after a crash, the tabs still running in the server
        assert_eq!(arrival_notice(None, new, 8).as_deref(), Some("Reattached 8 tabs"));
        assert_eq!(arrival_notice(None, new, 1).as_deref(), Some("Reattached 1 tab"));
        // A fresh start: nothing to say
        assert_eq!(arrival_notice(None, new, 0), None);
    }

    #[test]
    fn a_short_title_leaves_the_summary_the_rest() {
        let f = fit(&content("pwsh", Some(SUMMARY)), 50);
        assert_eq!(f.title, "pwsh");
        // The caption and its gap take 16, the title 4, the separator 3
        assert_eq!(columns(f.summary.as_deref().unwrap()), 50 - 16 - 4 - 3);
    }

    #[test]
    fn the_prompt_row_says_when_it_cut_the_prompt() {
        let prompt = "can we work on the previous user sent message feature?";
        assert_eq!(fit_prompt(prompt, 80), (prompt.to_string(), false));
        let (shown, cut) = fit_prompt(prompt, 20);
        assert!(cut);
        // Cut at a space, which goes before the ellipsis
        assert!(columns(&shown) <= 20);
        assert!(shown.ends_with('\u{2026}'));
    }

    #[test]
    fn wrapping_breaks_at_spaces() {
        assert_eq!(
            wrap("seven chats closed 21:13-22:08 are now under Interrupted", 20),
            vec!["seven chats closed", "21:13-22:08 are now", "under Interrupted"]
        );
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 10), vec![""]);
        for line in wrap(SUMMARY, 13) {
            assert!(columns(&line) <= 13, "{line:?}");
        }
    }

    #[test]
    fn the_card_lines_its_text_up_with_the_strip() {
        let rows: Vec<DetailRow> = [
            "Goal: find chats closed today in LBTerm that vanished from the AI Dashboard and restore them.",
            "Finding: closing a tab fires SessionEnd.",
            "Next: Ctrl+R in Obsidian.",
        ]
        .iter()
        .map(|l| detail_row(l))
        .collect();
        let cell = (7.2, 16.0);
        let c = card(&rows, 90.0, 32.0, (76.0, 1076.0), cell, 1.0).unwrap();
        // The labels start where the strip's words do, the text after the longest and two more
        assert_eq!(c.label_x, 90.0);
        assert_eq!(c.text_x, 90.0 + 9.0 * 7.2);
        assert_eq!(c.rect.x, 78.0);
        assert_eq!(c.rect.y, 32.0);
        assert!(c.rect.x + c.rect.w <= 1076.0);
        // One line a row here, each label on its row's first line, 6 px between rows
        assert_eq!(c.lines.len(), 3);
        assert_eq!(c.lines[0].label.as_deref(), Some("Goal"));
        assert_eq!(c.lines[1].y - c.lines[0].y, 16.0 + 6.0);
        assert_eq!(c.rect.h, (12.0 + 3.0 * 16.0 + 2.0 * 6.0 + 12.0_f64).ceil());
    }

    #[test]
    fn a_narrow_window_wraps_the_card() {
        let rows = vec![detail_row(
            "Goal: find chats closed today in LBTerm that vanished from the AI Dashboard and restore them.",
        )];
        let c = card(&rows, 90.0, 32.0, (76.0, 500.0), (7.2, 16.0), 1.0).unwrap();
        assert!(c.lines.len() > 1);
        assert_eq!(c.lines[0].label.as_deref(), Some("Goal"));
        assert_eq!(c.lines[1].label, None);
        assert!(c.rect.x + c.rect.w <= 500.0, "{:?}", c.rect);
        assert!(card(&[], 90.0, 32.0, (76.0, 500.0), (7.2, 16.0), 1.0).is_none());
    }

    #[test]
    fn a_closing_card_is_drawn_as_it_was_until_it_has_faded() {
        assert_eq!(card_draw(true, false), CardDraw::Fresh);
        assert_eq!(card_draw(true, true), CardDraw::Fresh);
        // Closed, while the compositor still shows it fading: the card it showed
        assert_eq!(card_draw(false, true), CardDraw::Last);
        // Faded, or no compositor to fade it: nothing
        assert_eq!(card_draw(false, false), CardDraw::None);
    }

    #[test]
    fn the_cards_visual_holds_its_shadow() {
        let card = Rect { x: 78.0, y: 32.0, w: 716.0, h: 84.0 };
        // 11 px round it, 3 more below, at 96 DPI
        assert_eq!(visual_bounds(&card, 1.0, (1080, 1890)), Some((67, 21, 738, 109)));
        // Fractional edges round outwards, at 150%
        let (by, down) = shadow_reach(1.5);
        assert_eq!((by, down), (16.5, 4.5));
        assert_eq!(visual_bounds(&card, 1.5, (1080, 1890)), Some((61, 15, 750, 122)));
        // Kept inside the window
        let wide = Rect { x: 4.0, y: 32.0, w: 1100.0, h: 84.0 };
        assert_eq!(visual_bounds(&wide, 1.0, (1080, 1890)), Some((0, 21, 1080, 109)));
        assert_eq!(visual_bounds(&card, 1.0, (50, 20)), None);
    }

    #[test]
    fn the_card_keeps_clear_of_the_tab_panel() {
        // The panes' text 6 px past where their room starts: the card starts with the room, its
        // labels where the strip's words are
        let rows = vec![detail_row("Goal: a short one.")];
        let c = card(&rows, 82.0, 32.0, (76.0, 1076.0), (7.2, 16.0), 1.0).unwrap();
        assert_eq!(c.rect.x, 76.0);
        assert_eq!(c.label_x, 82.0);
        assert_eq!(c.rect.x + c.rect.w, (82.0 + 12.0 + (6.0 + 12.0) * 7.2_f64).ceil());
    }
}
