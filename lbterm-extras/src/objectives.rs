//! Each Claude Code session's objectives (lbterm/OBJECTIVES.md), as Terminal's objectives.ts keeps
//! them in `<sid>.objectives.json` beside its summary: what Lachlan asked the session for (its
//! first request, the InMail it was told to deal with, what he asked for later), whether each is
//! open, done, waiting on him or dropped, and what showed it. The objectives panel down the right
//! of the window (wezterm-gui's render/objectives.rs) shows a mark per tab, level with its pad on
//! the tab rail; resting the pointer there opens a card of each tab's first objective, and resting
//! on one of its rows a second card with that session's whole list. What a mark is, and where each
//! card's words go, are here, where they can be tested.

use crate::strip::{elide, wrap};
use crate::tabs::Rect;
use serde::Deserialize;

/// Where an objective came from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    /// The session's first request
    First,
    /// A message it was told to deal with
    Inmail,
    /// Something he asked for later
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Done,
    /// It can't go on without Lachlan
    Waiting,
    Dropped,
}

impl Status {
    /// As the detail card's label column says it
    pub fn word(self) -> &'static str {
        match self {
            Status::Open => "Open",
            Status::Done => "Done",
            Status::Waiting => "Waiting",
            Status::Dropped => "Dropped",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    pub id: u64,
    pub text: String,
    pub from: From,
    pub status: Status,
    pub evidence: String,
}

/// One session's file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Objectives {
    /// The objective its last step worked on, 0 for none
    pub current: u64,
    pub list: Vec<Objective>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    current: u64,
    #[serde(default)]
    objectives: Vec<RawObjective>,
}

#[derive(Deserialize)]
struct RawObjective {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    evidence: String,
}

/// A `<sid>.objectives.json` as objectives.ts wrote it, or `None` if it isn't one or lists none.
/// A status or source it doesn't know reads as open, or as asked for later.
pub fn parse(text: &str) -> Option<Objectives> {
    let raw: RawFile = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let list: Vec<Objective> = raw
        .objectives
        .into_iter()
        .filter(|o| !o.text.trim().is_empty())
        .map(|o| Objective {
            id: o.id,
            text: o.text.split_whitespace().collect::<Vec<_>>().join(" "),
            from: match o.from.as_str() {
                "first" => From::First,
                "inmail" => From::Inmail,
                _ => From::Prompt,
            },
            status: match o.status.as_str() {
                "done" => Status::Done,
                "waiting" => Status::Waiting,
                "dropped" => Status::Dropped,
                _ => Status::Open,
            },
            evidence: o.evidence.split_whitespace().collect::<Vec<_>>().join(" "),
        })
        .collect();
    (!list.is_empty()).then_some(Objectives { current: raw.current, list })
}

/// What a tab's mark on the closed strip says about what its session was started for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Everything it was asked for is done: a green dot
    Done,
    /// What it was started for is done, and something asked for later is still open: a green ring
    FirstDone,
    /// What it was started for is still open, and its last step was on something asked for
    /// later: a yellow ring
    Drifted,
    /// What it was started for waits on Lachlan: an orange ring
    Waiting,
    /// What it was started for is still open: a grey ring
    Open,
}

impl Mark {
    /// Filled, or a ring
    pub fn filled(self) -> bool {
        self == Mark::Done
    }

    /// Its colour: the tab rail's green, the idle timer's yellow, the waiting pad's orange, and
    /// the labels' grey (lbterm_panel::palette)
    pub fn colour(self) -> u32 {
        match self {
            Mark::Done | Mark::FirstDone => GREEN,
            Mark::Drifted => YELLOW,
            Mark::Waiting => ORANGE,
            Mark::Open => GREY,
        }
    }

    /// For the state file
    pub fn name(self) -> &'static str {
        match self {
            Mark::Done => "done",
            Mark::FirstDone => "first-done",
            Mark::Drifted => "drifted",
            Mark::Waiting => "waiting",
            Mark::Open => "open",
        }
    }
}

const GREEN: u32 = 0x3DDC84;
const YELLOW: u32 = 0xF2E86D;
const ORANGE: u32 = 0xFF8A3D;
const GREY: u32 = 0xA8A8BC;

/// A status word's colour on the detail card: the tab rail's status words' (lbterm_panel::palette::status)
pub fn status_colour(status: Status) -> u32 {
    match status {
        Status::Done => 0x7CF07C,
        Status::Waiting => 0xFF9A62,
        Status::Open => 0xD0D0DC,
        Status::Dropped => 0x8A8A9E,
    }
}

impl Objectives {
    fn live(&self) -> impl Iterator<Item = &Objective> {
        self.list.iter().filter(|o| o.status != Status::Dropped)
    }

    /// What the session was started for: its first request and the InMail it was told to deal
    /// with, those not dropped; what was asked for later when there are none of those
    fn originals(&self) -> Vec<&Objective> {
        let originals: Vec<_> = self.live().filter(|o| o.from != From::Prompt).collect();
        if originals.is_empty() {
            self.live().collect()
        } else {
            originals
        }
    }

    /// The objective the session was started for, the one its row says: its first request, else
    /// the first message it was told to deal with, else the first it has
    pub fn first(&self) -> Option<&Objective> {
        self.list
            .iter()
            .find(|o| o.from == From::First)
            .or_else(|| self.list.iter().find(|o| o.from == From::Inmail))
            .or_else(|| self.list.first())
    }

    /// How many of those not dropped are done, of how many
    pub fn tally(&self) -> (usize, usize) {
        let live: Vec<_> = self.live().collect();
        (live.iter().filter(|o| o.status == Status::Done).count(), live.len())
    }

    /// The tab's mark; `None` when everything it was asked for was dropped
    pub fn mark(&self) -> Option<Mark> {
        let originals = self.originals();
        if originals.is_empty() {
            return None;
        }
        let open: Vec<_> = originals.iter().filter(|o| o.status != Status::Done).collect();
        if open.is_empty() {
            let (done, of) = self.tally();
            return Some(if done == of { Mark::Done } else { Mark::FirstDone });
        }
        let current = self.list.iter().find(|o| o.id == self.current && self.current != 0);
        let on_later = current.map_or(false, |c| c.from == From::Prompt && !originals.iter().any(|o| o.id == c.id));
        Some(if on_later {
            Mark::Drifted
        } else if open.iter().any(|o| o.status == Status::Waiting) {
            Mark::Waiting
        } else {
            Mark::Open
        })
    }
}

// ---------- the cards ----------

/// Inside a card's edge, and between an objective and the next on the detail card
const PAD: f64 = 12.0;
const GAP: f64 = 6.0;
/// The open card's mark, beside each row's words
pub const MARK: f64 = 8.0;

/// What a tab's row on the open card is told
#[derive(Debug, Clone, PartialEq)]
pub struct RowIn<'a> {
    /// The rail's row
    pub row: usize,
    /// The middle of its pad, and its slot's height
    pub middle: f64,
    pub height: f64,
    pub objectives: Option<&'a Objectives>,
    /// What the rail calls the tab, for a row with no objectives
    pub name: &'a str,
}

/// A row of the open card
#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
    pub row: usize,
    /// All of its slot across the card: what the pointer can be on
    pub rect: Rect,
    /// The mark's middle, its words' top, where they start
    pub mark_x: f64,
    pub text_y: f64,
    pub text_x: f64,
    pub mark: Option<Mark>,
    /// The first objective, or the tab's name, dimmer, without one
    pub text: String,
    pub has_objectives: bool,
    /// "2/3", right-aligned at `tally_x`'s end; empty with only one
    pub tally: String,
    pub tally_x: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub rect: Rect,
    pub rows: Vec<ListRow>,
}

/// The open card: `width` wide, its right edge at `right`, a row level with each tab's pad. `cell`
/// is a column's width and a line's height, in pixels; `scale` the DPI over 96.
pub fn list(rows: &[RowIn], right: f64, width: f64, cell: (f64, f64), scale: f64) -> Option<List> {
    if rows.is_empty() || cell.0 <= 0.0 || width <= 0.0 {
        return None;
    }
    let pad = PAD * scale;
    let mark = MARK * scale;
    let x = right - width;
    let tally_columns = 5;
    let text_x = x + pad + mark + 8.0 * scale;
    let tally_end = right - pad;
    let text_columns = (((tally_end - text_x) / cell.0).floor() as usize).saturating_sub(tally_columns + 1).max(4);
    let mut out = vec![];
    for r in rows {
        let first = r.objectives.and_then(|o| o.first());
        let text = match first {
            Some(o) => elide(&shown(o), text_columns),
            None => elide(r.name, text_columns),
        };
        let tally = r
            .objectives
            .map(|o| o.tally())
            .filter(|(_, of)| *of > 1)
            .map(|(done, of)| format!("{done}/{of}"))
            .unwrap_or_default();
        let top = r.middle - r.height / 2.0;
        out.push(ListRow {
            row: r.row,
            rect: Rect { x, y: top, w: width, h: r.height },
            mark_x: x + pad + mark / 2.0,
            text_y: (r.middle - cell.1 / 2.0).round(),
            text_x,
            mark: r.objectives.and_then(|o| o.mark()),
            text,
            has_objectives: first.is_some(),
            tally_x: tally_end - tally.chars().count() as f64 * cell.0,
            tally,
        });
    }
    let top = out.iter().map(|r| r.rect.y).fold(f64::INFINITY, f64::min) - pad / 2.0;
    let bottom = out.iter().map(|r| r.rect.y + r.rect.h).fold(f64::NEG_INFINITY, f64::max) + pad / 2.0;
    Some(List {
        rect: Rect { x, y: top.floor(), w: width.ceil(), h: (bottom - top).ceil() },
        rows: out,
    })
}

/// An objective's words on a card: a message is said to be one
fn shown(o: &Objective) -> String {
    match o.from {
        From::Inmail => format!("InMail: {}", o.text),
        _ => o.text.clone(),
    }
}

/// How a line of the detail card is drawn
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tone {
    /// What the tab is called, on top
    Title,
    /// An objective's status word, in its colour
    Status(Status),
    /// "now", under the status of the objective the last step was on
    Now,
    /// An objective's words
    Text,
    /// What showed its status, dimmer
    Evidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub x: f64,
    pub y: f64,
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub rect: Rect,
    pub lines: Vec<Line>,
}

/// The second card, for one tab: `width` wide, its right edge at `right`, its top level with the
/// row it's for (`top`), moved up to end by `bottom` when it would run past it, and no higher
/// than `ceiling`. Each objective is its status word, its words wrapped beside it, and what
/// showed its status under them.
pub fn detail(
    title: &str,
    o: &Objectives,
    right: f64,
    (top, ceiling, bottom): (f64, f64, f64),
    width: f64,
    cell: (f64, f64),
    scale: f64,
) -> Option<Detail> {
    if cell.0 <= 0.0 || width <= 0.0 {
        return None;
    }
    let pad = PAD * scale;
    let gap = GAP * scale;
    let x = right - width;
    let label_columns = 7 + 2;
    let text_x = x + pad + label_columns as f64 * cell.0;
    let columns = (((right - pad - text_x) / cell.0).floor() as usize).max(8);
    let title_columns = (((right - pad - (x + pad)) / cell.0).floor() as usize).max(8);

    let mut lines = vec![];
    let mut y = pad;
    lines.push(Line { x: x + pad, y, text: elide(title, title_columns), tone: Tone::Title });
    y += cell.1 + gap;
    for item in &o.list {
        let words = wrap(&shown(item), columns);
        let evidence = if item.evidence.is_empty() { vec![] } else { wrap(&item.evidence, columns) };
        let now = o.current != 0 && item.id == o.current;
        lines.push(Line { x: x + pad, y, text: item.status.word().to_string(), tone: Tone::Status(item.status) });
        if now {
            lines.push(Line { x: x + pad, y: y + cell.1, text: "now".to_string(), tone: Tone::Now });
        }
        let mut rows = 0;
        for w in words {
            lines.push(Line { x: text_x, y: y + rows as f64 * cell.1, text: w, tone: Tone::Text });
            rows += 1;
        }
        for e in evidence {
            lines.push(Line { x: text_x, y: y + rows as f64 * cell.1, text: e, tone: Tone::Evidence });
            rows += 1;
        }
        // "now" under the status word takes a line of its own when the words don't
        let rows = rows.max(if now { 2 } else { 1 });
        y += rows as f64 * cell.1 + gap;
    }
    let h = (y - gap + pad).ceil();
    let at = (top.min(bottom - h)).max(ceiling).floor();
    for line in &mut lines {
        line.y = (line.y + at).round();
    }
    Some(Detail { rect: Rect { x, y: at, w: width.ceil(), h }, lines })
}

/// Where the closed strip's mark for a row goes: the strip's middle, the pad's middle; and how
/// wide it is, at `scale`
pub fn strip_mark(strip_x: f64, strip_w: f64, middle: f64, scale: f64) -> Rect {
    let d = (MARK * scale).round();
    Rect {
        x: (strip_x + (strip_w - d) / 2.0).round(),
        y: (middle - d / 2.0).round(),
        w: d,
        h: d,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn file(current: u64, list: &[(u64, &str, &str, &str)]) -> Objectives {
        let objectives: Vec<String> = list
            .iter()
            .map(|(id, from, status, text)| {
                format!(r#"{{"id":{id},"text":"{text}","from":"{from}","status":"{status}","evidence":"","at":1,"statusAt":1}}"#)
            })
            .collect();
        parse(&format!(r#"{{"sessionId":"x","updatedAt":1,"current":{current},"objectives":[{}]}}"#, objectives.join(","))).unwrap()
    }

    #[test]
    fn a_file_reads_as_its_objectives_and_anything_unknown_as_open_or_later() {
        let o = parse("\u{feff}{\"current\":2,\"objectives\":[{\"id\":1,\"text\":\"  Fix   the rail \",\"from\":\"first\",\"status\":\"done\",\"evidence\":\"Landed\"},{\"id\":2,\"text\":\"x\",\"from\":\"?\",\"status\":\"?\"},{\"id\":3,\"text\":\" \"}]}").unwrap();
        assert_eq!(o.current, 2);
        assert_eq!(o.list.len(), 2, "one with no words is left out");
        assert_eq!((o.list[0].text.as_str(), o.list[0].from, o.list[0].status, o.list[0].evidence.as_str()), ("Fix the rail", From::First, Status::Done, "Landed"));
        assert_eq!((o.list[1].from, o.list[1].status), (From::Prompt, Status::Open));
        assert_eq!(parse("{\"objectives\":[]}"), None);
        assert_eq!(parse("not json"), None);
    }

    #[test]
    fn the_mark_says_what_became_of_what_the_session_was_started_for() {
        assert_eq!(file(1, &[(1, "first", "open", "a")]).mark(), Some(Mark::Open));
        assert_eq!(file(1, &[(1, "first", "waiting", "a")]).mark(), Some(Mark::Waiting));
        assert_eq!(file(1, &[(1, "first", "done", "a")]).mark(), Some(Mark::Done));
        // on a detour: the first still open, the last step on something asked for later, done or not
        assert_eq!(file(2, &[(1, "first", "open", "a"), (2, "prompt", "open", "b")]).mark(), Some(Mark::Drifted));
        assert_eq!(file(2, &[(1, "first", "waiting", "a"), (2, "prompt", "done", "b")]).mark(), Some(Mark::Drifted));
        // back on the first
        assert_eq!(file(1, &[(1, "first", "open", "a"), (2, "prompt", "done", "b")]).mark(), Some(Mark::Open));
        // the first done, a later one open
        assert_eq!(file(2, &[(1, "first", "done", "a"), (2, "prompt", "open", "b")]).mark(), Some(Mark::FirstDone));
        assert_eq!(file(2, &[(1, "first", "done", "a"), (2, "prompt", "done", "b"), (3, "prompt", "dropped", "c")]).mark(), Some(Mark::Done));
        // InMail: each message is what it was started for
        assert_eq!(file(1, &[(1, "inmail", "done", "a"), (2, "inmail", "open", "b")]).mark(), Some(Mark::Open));
        // all it was started for dropped: what's left stands in for it
        assert_eq!(file(2, &[(1, "first", "dropped", "a"), (2, "prompt", "open", "b")]).mark(), Some(Mark::Open));
        assert_eq!(file(0, &[(1, "first", "dropped", "a")]).mark(), None);
    }

    #[test]
    fn the_first_objective_is_its_first_request_else_its_first_message() {
        assert_eq!(file(0, &[(1, "inmail", "open", "msg"), (2, "first", "open", "req")]).first().unwrap().text, "req");
        assert_eq!(file(0, &[(1, "prompt", "open", "later"), (2, "inmail", "open", "msg")]).first().unwrap().text, "msg");
        assert_eq!(file(0, &[(1, "first", "done", "a"), (2, "prompt", "open", "b"), (3, "prompt", "dropped", "c")]).tally(), (1, 2));
    }

    #[test]
    fn the_open_cards_rows_sit_level_with_the_pads_and_say_the_first_objective() {
        let a = file(1, &[(1, "first", "open", "Find why Defender uses 30% of the CPU"), (2, "prompt", "done", "b")]);
        let b = file(1, &[(1, "inmail", "done", "Fix the tab rail")]);
        let rows = [
            RowIn { row: 0, middle: 100.0, height: 46.0, objectives: Some(&a), name: "Defender" },
            RowIn { row: 1, middle: 146.0, height: 46.0, objectives: Some(&b), name: "InMail" },
            RowIn { row: 2, middle: 192.0, height: 46.0, objectives: None, name: "pwsh" },
        ];
        let l = list(&rows, 1000.0, 360.0, (7.0, 16.0), 1.0).unwrap();
        assert_eq!(l.rect.x, 640.0);
        assert_eq!((l.rect.y, l.rect.y + l.rect.h), (71.0, 221.0), "from the first slot's top to the last's bottom, half a pad out");
        assert_eq!(l.rows[0].text_y, 92.0, "its words centred on the pad");
        assert_eq!((l.rows[0].text.as_str(), l.rows[0].tally.as_str(), l.rows[0].mark), ("Find why Defender uses 30% of the CPU", "1/2", Some(Mark::Open)));
        assert_eq!((l.rows[1].text.as_str(), l.rows[1].tally.as_str(), l.rows[1].mark), ("InMail: Fix the tab rail", "", Some(Mark::Done)));
        assert_eq!((l.rows[2].text.as_str(), l.rows[2].has_objectives, l.rows[2].mark), ("pwsh", false, None));
        assert_eq!(l.rows[0].tally_x + 3.0 * 7.0, 988.0, "the tally ends a pad in from the right");
        assert!(l.rows.iter().all(|r| r.rect.contains(700.0, r.rect.y + 1.0)));
        // too long for the room: cut with an ellipsis
        let words = "word ".repeat(40);
        let long = file(1, &[(1, "first", "open", words.as_str())]);
        let l = list(&[RowIn { row: 0, middle: 50.0, height: 40.0, objectives: Some(&long), name: "" }], 300.0, 200.0, (7.0, 16.0), 1.0).unwrap();
        assert!(l.rows[0].text.ends_with('\u{2026}'));
        assert!((l.rows[0].text_x + l.rows[0].text.chars().count() as f64 * 7.0) <= 300.0 - 12.0);
    }

    #[test]
    fn the_detail_card_lists_every_objective_with_its_status_and_evidence() {
        let mut o = file(2, &[(1, "first", "done", "Find why the CPU runs at 91C"), (2, "prompt", "open", "Have the warden know the fast cores")]);
        o.list[0].evidence = "Nightly checks explained it".into();
        let d = detail("CPU heat check", &o, 628.0, (100.0, 40.0, 900.0), 360.0, (7.0, 16.0), 1.0).unwrap();
        assert_eq!((d.rect.x, d.rect.y), (268.0, 100.0));
        let said: Vec<_> = d.lines.iter().map(|l| (l.tone, l.text.as_str())).collect();
        assert_eq!(said, vec![
            (Tone::Title, "CPU heat check"),
            (Tone::Status(Status::Done), "Done"),
            (Tone::Text, "Find why the CPU runs at 91C"),
            (Tone::Evidence, "Nightly checks explained it"),
            (Tone::Status(Status::Open), "Open"),
            (Tone::Now, "now"),
            (Tone::Text, "Have the warden know the fast cores"),
        ]);
        let y = |i: usize| d.lines[i].y - d.rect.y;
        assert_eq!((y(0), y(1), y(3), y(4), y(5)), (12.0, 34.0, 50.0, 72.0, 88.0));
        assert_eq!(d.rect.h, 72.0 + 32.0 + 12.0, "the last objective's two lines, then the pad");
        // near the bottom it moves up to fit, but never above the ceiling
        let low = detail("x", &o, 628.0, (850.0, 40.0, 900.0), 360.0, (7.0, 16.0), 1.0).unwrap();
        assert_eq!(low.rect.y + low.rect.h, 900.0);
        let tiny = detail("x", &o, 628.0, (850.0, 820.0, 900.0), 360.0, (7.0, 16.0), 1.0).unwrap();
        assert_eq!(tiny.rect.y, 820.0);
    }

    #[test]
    fn a_strip_mark_is_centred_on_the_strip_and_the_pad() {
        assert_eq!(strip_mark(1000.0, 14.0, 100.0, 1.0), Rect { x: 1003.0, y: 96.0, w: 8.0, h: 8.0 });
        assert_eq!(strip_mark(1000.0, 21.0, 100.0, 1.5), Rect { x: 1005.0, y: 94.0, w: 12.0, h: 12.0 });
    }
}
