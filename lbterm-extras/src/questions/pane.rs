//! The questions bar's rows (lbterm/QUESTIONS.md, "The questions pane and where its questions
//! come from"). Lachlan's ask of 2026-11-11 and his picks of 2026-11-14: beside the colours a
//! hold shows, a bar the closed tab rail's size lists the questions of the held pane's
//! conversation, newest reply first, each by its colour and label, answered or not.
//!
//! [`rows`] is the one place the rows are made, from what a Claude Code session has today: the
//! session's `<sid>.questions.json` (its replies that asked something, [`Map`]) and the turns
//! read from its transcript ([`Turn`], on the conversation's reading thread, crate::conversation).
//! A session of ToolBelt's harness gives its questions as structure instead (question blocks of
//! items, and `answer` entries tying his reply to the block it answers), and for one of those only
//! this function changes: what it gives stays the same.

use super::Map;
use crate::conversation::{Chunks, Turn};
use crate::drafts::Pick;

/// One question in the pane
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The reply that asked it, by its id in the questions file
    pub reply: String,
    /// How many replies that asked something came after that one: 0 for the newest
    pub age: usize,
    /// Its label as asked (Q1, 2, E4), a stop after it dropped; else Q and its number, else Q and
    /// its place, as its marker says
    pub label: String,
    /// The number an answer names it by, if it has one
    pub number: Option<u32>,
    /// Its place in its reply, which is its colour ([`super::colour`]): the same as its bands'
    pub colour: usize,
    /// An extra offered after the questions (E4)
    pub extra: bool,
    /// The last of his picks that named it, in a prompt after its reply; for a reply that asked
    /// it alone, with no extra, his first prompt after it in words, as a pick of its number with
    /// no options and no mark (Lachlan's 6a of 2026-11-14)
    pub pick: Option<Pick>,
}

/// How much of its colour an answered question's pad shows in the questions bar: half, so the
/// ones still waiting on him stand out (Lachlan's 16a of 2026-11-14)
pub const ANSWERED_FILL: f64 = 0.5;

impl Row {
    /// He's answered it: a pick of his named it and didn't ask back (`?`, `??`, `?!`)
    pub fn answered(&self) -> bool {
        self.pick.as_ref().is_some_and(Pick::settles)
    }

    /// How much of its colour its pad shows in the questions bar: all of it while it waits on
    /// him, `ANSWERED_FILL` once he's answered it. Its tick stays as it is
    pub fn fill(&self) -> f64 {
        if self.answered() {
            ANSWERED_FILL
        } else {
            1.0
        }
    }
}

/// The first of `turns` after a reply written at `at` ended: the turn after the last one that
/// had written something by then. A reply ends before the worker writes it down, 5 to 20 s later,
/// and he can answer within that: the turn his answer starts is still writing after the worker's
/// time, so it isn't the one the reply ended in, unless the whole of it came and went in that
/// gap. `None` when the file doesn't say when (`at` of 0).
fn first_after(turns: &Chunks<Turn>, at: i64) -> Option<usize> {
    if at <= 0 {
        return None;
    }
    // The turns' last writes only grow: a turn starts after the last one's last record
    let (mut lo, mut hi) = (0, turns.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if turns[mid].last_ms <= at {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // A turn that's written nothing since its message (a prompt Claude hasn't answered yet, a
    // command Claude Code ran itself) didn't end a reply
    let ended = (0..lo).rev().find(|&t| turns[t].last_ms > turns[t].at_ms);
    Some(ended.map_or(0, |t| t + 1))
}

/// The questions pane's rows for one session: each question of its replies in `map`, newest
/// reply first and each reply's in its order, with the last pick of his that named it.
///
/// A prompt of his answers the questions it names by number ([`crate::drafts::picks_in`]: 1a,
/// 2., 2+, 3deg, "1+ but more salt"), each in the newest reply before it that asks that number:
/// a reply's labels start again at 1, so the newest is the one he's looking at, and a number it
/// doesn't ask is an older one's still. A prompt in words alone names none, but for a reply that
/// asks one question and no extra, which his first prompt after it answers whatever it says
/// (Lachlan's 6a of 2026-11-14: "yes, do it" is how he answers one question; 45 of his 272 first
/// answers in the four days to then were in words).
pub fn rows(map: &Map, turns: &Chunks<Turn>) -> Vec<Row> {
    let after: Vec<Option<usize>> = map.messages.iter().map(|m| first_after(turns, m.at)).collect();
    let mut picks: Vec<Vec<Option<Pick>>> = map.messages.iter().map(|m| vec![None; m.questions.len()]).collect();
    if let Some(from) = after.iter().flatten().min() {
        for t in *from..turns.len() {
            for pick in &turns[t].picks {
                // The newest reply that ended before this turn and asks this number
                let asked = (0..map.messages.len()).rev().find(|&j| {
                    after[j].is_some_and(|a| a <= t)
                        && map.messages[j].questions.iter().any(|q| q.number() == Some(pick.number))
                });
                let Some(j) = asked else {
                    continue;
                };
                for (qi, q) in map.messages[j].questions.iter().enumerate() {
                    if q.number() == Some(pick.number) {
                        picks[j][qi] = Some(pick.clone());
                    }
                }
            }
        }
    }
    // One question and no extra: his first prompt after the reply answers it, unless a newer
    // reply had ended by then (his words are that one's) or the prompt named something by number
    // (read above, as any answer is)
    for (j, message) in map.messages.iter().enumerate() {
        let [only] = message.questions.as_slice() else {
            continue;
        };
        if only.is_extra() || picks[j][0].is_some() {
            continue;
        }
        let Some(from) = after[j] else {
            continue;
        };
        let Some(t) = (from..turns.len()).find(|&t| turns[t].his) else {
            continue;
        };
        let newer_ended = (j + 1..map.messages.len()).any(|k| after[k].is_some_and(|a| a <= t));
        if newer_ended || !turns[t].picks.is_empty() {
            continue;
        }
        picks[j][0] = Some(Pick {
            number: only.number().unwrap_or(1),
            options: String::new(),
            mark: None,
        });
    }
    let newest = map.messages.len();
    let mut out = vec![];
    for (j, message) in map.messages.iter().enumerate().rev() {
        for (qi, q) in message.questions.iter().enumerate() {
            out.push(Row {
                reply: message.id.clone(),
                age: newest - 1 - j,
                label: q.name(qi).trim_end_matches(['.', ':', ')']).to_string(),
                number: q.number(),
                colour: qi,
                extra: q.is_extra(),
                pick: picks[j][qi].take(),
            });
        }
    }
    out
}

/// Where the bar's rows go down its list, top to bottom ([`place`])
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// Each row shown, by its index in the rows, with its slot's top from the list's top
    pub rows: Vec<(usize, f64)>,
    /// Every slot's height, the same for all, as the rail's rows are
    pub row_h: f64,
    /// The hairlines between one reply's rows and the next's, by their middle, from the list's top
    pub rules: Vec<f64>,
    /// The rows that didn't fit, newest first kept: the bar says "+N" for them in a slot of its
    /// own at `more_at`, so none is dropped quietly
    pub more: usize,
    pub more_at: f64,
}

/// Lay `rows` down the questions bar's list, `room` tall, as the closed tab rail lays its tabs
/// down its own (lbterm-panel's `Layout::row_height`): every slot `row_max` tall while they all
/// fit at that, shrinking together to fit more, down to `row_min`, with `rule_h` between two
/// replies' rows and a hairline in its middle. When they don't all fit at `row_min`, as many as
/// fit with a slot under them saying how many more there are. The newest reply's are first, so
/// the rows cut are the oldest. No rows: the first slot is where the GUI says "none".
pub fn place(rows: &[Row], room: f64, row_min: f64, row_max: f64, rule_h: f64) -> Placed {
    let n = rows.len();
    if n == 0 {
        return Placed { rows: vec![], row_h: row_max, rules: vec![], more: 0, more_at: 0.0 };
    }
    let breaks = rows.windows(2).filter(|w| w[0].age != w[1].age).count();
    let fit = ((room - breaks as f64 * rule_h) / n as f64).floor();
    let all = fit >= row_min;
    let row_h = if all { fit.min(row_max) } else { row_min };
    let mut tops = Vec::with_capacity(n);
    let mut rules = vec![];
    let mut y = 0.0;
    for (i, row) in rows.iter().enumerate() {
        if i > 0 && row.age != rows[i - 1].age {
            rules.push(y + rule_h / 2.0);
            y += rule_h;
        }
        tops.push(y);
        y += row_h;
    }
    if all {
        return Placed { rows: tops.into_iter().enumerate().collect(), row_h, rules, more: 0, more_at: y };
    }
    // The most rows that leave a slot for "+N" under them, at least none
    let shown = (0..n).rev().find(|&k| tops[k] + 2.0 * row_h <= room).map_or(0, |k| k + 1);
    let more_at = if shown == 0 { 0.0 } else { tops[shown - 1] + row_h };
    rules.retain(|&r| r < more_at);
    Placed {
        rows: tops.into_iter().take(shown).enumerate().collect(),
        row_h,
        rules,
        more: n - shown,
        more_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::{picks_in, Mark};
    use crate::questions::{Message, Question};

    fn question(label: &str, number: Option<u32>) -> Question {
        Question {
            label: label.into(),
            number,
            text: format!("{label} something"),
            pos: 0,
            about: vec![],
        }
    }

    fn reply(id: &str, at: i64, labels: &[&str]) -> Message {
        Message {
            id: id.into(),
            at,
            questions: labels.iter().map(|l| question(l, None)).collect(),
        }
    }

    /// Turns as (when its message was sent, when it last wrote, his prompt or none)
    fn turns(list: &[(i64, i64, Option<&str>)]) -> Chunks<Turn> {
        let mut out = Chunks::default();
        for (at_ms, last_ms, prompt) in list {
            out.push(Turn {
                at_ms: *at_ms,
                last_ms: *last_ms,
                picks: prompt.map(picks_in).unwrap_or_default(),
                his: prompt.is_some(),
            });
        }
        out
    }

    /// Each row as its label and whether it's answered, newest reply first
    fn shown(rows: &[Row]) -> Vec<(String, bool)> {
        rows.iter().map(|r| (r.label.clone(), r.answered())).collect()
    }

    fn row(rows: &[Row], reply: &str, label: &str) -> Row {
        rows.iter().find(|r| r.reply == reply && r.label == label).cloned().unwrap()
    }

    #[test]
    fn the_newest_replys_questions_come_first_each_in_its_colour() {
        let map = Map {
            messages: vec![reply("old", 1_000, &["Q1", "Q2"]), reply("new", 5_000, &["Q1", "Q2", "E3", "4"])],
        };
        let rows = rows(&map, &turns(&[(500, 900, Some("go")), (3_000, 4_000, Some("1a"))]));
        let ids: Vec<(&str, usize, usize, bool)> = rows.iter().map(|r| (r.reply.as_str(), r.age, r.colour, r.extra)).collect();
        assert_eq!(
            ids,
            vec![("new", 0, 0, false), ("new", 0, 1, false), ("new", 0, 2, true), ("new", 0, 3, false), ("old", 1, 0, false), ("old", 1, 1, false)]
        );
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), vec!["Q1", "Q2", "E3", "4", "Q1", "Q2"]);
        assert_eq!(rows.iter().map(|r| r.number).collect::<Vec<_>>(), vec![Some(1), Some(2), Some(3), Some(4), Some(1), Some(2)]);
    }

    #[test]
    fn a_prompt_after_the_reply_answers_and_the_one_it_answered_doesnt() {
        // Turn 0 asked; its reply's questions were written at 10 s. "1a" started turn 0, before
        // the reply: it answers nothing of it. Turn 1's "1+ but more salt" and "3deg" do.
        let map = Map {
            messages: vec![reply("r", 10_000, &["Q1", "Q2", "Q3"])],
        };
        let t = turns(&[(1_000, 4_000, Some("1a")), (20_000, 25_000, Some("1+ but more salt\n3deg"))]);
        let rows = rows(&map, &t);
        assert_eq!(shown(&rows), vec![("Q1".into(), true), ("Q2".into(), false), ("Q3".into(), true)]);
        assert_eq!(row(&rows, "r", "Q1").pick.unwrap().mark, Some(Mark::Yes));
        assert_eq!(row(&rows, "r", "Q3").pick.unwrap().options, "deg");
        // Nothing after it: all open
        let rows = super::rows(&map, &turns(&[(1_000, 4_000, Some("1a"))]));
        assert!(rows.iter().all(|r| r.pick.is_none()));
    }

    #[test]
    fn an_answer_before_the_worker_wrote_the_reply_down_still_answers_it() {
        // The reply ended at 4 s and the worker wrote it at 15 s; he answered at 8 s, and that
        // turn wrote on to 30 s
        let map = Map {
            messages: vec![reply("r", 15_000, &["Q1"])],
        };
        let t = turns(&[(1_000, 4_000, Some("start")), (8_000, 30_000, Some("1b"))]);
        assert_eq!(shown(&rows(&map, &t)), vec![("Q1".into(), true)]);
        // Sent but nothing written after it yet: still his answer
        let t = turns(&[(1_000, 4_000, Some("start")), (8_000, 8_000, Some("1b"))]);
        assert_eq!(shown(&rows(&map, &t)), vec![("Q1".into(), true)]);
    }

    #[test]
    fn a_number_answers_the_newest_reply_that_asks_it() {
        // "old" asks Q1 to Q3, "new" asks Q1 alone: his 1+ is new's, his 3- is old's (new
        // doesn't ask a 3), and old's Q1 and Q2 wait
        let map = Map {
            messages: vec![reply("old", 5_000, &["Q1", "Q2", "Q3"]), reply("new", 15_000, &["Q1"])],
        };
        let t = turns(&[(1_000, 2_000, None), (8_000, 10_000, Some("explain the first")), (20_000, 21_000, Some("1+\n3-"))]);
        let rows = rows(&map, &t);
        assert_eq!(
            shown(&rows),
            vec![("Q1".into(), true), ("Q1".into(), false), ("Q2".into(), false), ("Q3".into(), true)]
        );
        // The prompt between them answers old's alone
        let t = turns(&[(1_000, 2_000, None), (8_000, 10_000, Some("2a")), (20_000, 21_000, None)]);
        let rows = super::rows(&map, &t);
        assert!(row(&rows, "old", "Q2").answered());
        assert!(!row(&rows, "new", "Q1").answered());
    }

    #[test]
    fn an_extra_answered_no_is_answered_and_asking_back_isnt() {
        let map = Map {
            messages: vec![reply("r", 5_000, &["Q1", "Q2", "E3", "E4"])],
        };
        let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("1?\n2??\n3-\n4?!"))]);
        let rows = rows(&map, &t);
        assert_eq!(
            shown(&rows),
            vec![("Q1".into(), false), ("Q2".into(), false), ("E3".into(), true), ("E4".into(), false)]
        );
        assert_eq!(row(&rows, "r", "E3").pick.unwrap().mark, Some(Mark::No));
        // A later pick is the one that counts: he asked, Claude said more, he picked
        let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("1?")), (12_000, 13_000, Some("Thanks.\n1b"))]);
        assert_eq!(row(&super::rows(&map, &t), "r", "Q1").pick.unwrap().options, "b");
    }

    #[test]
    fn an_answered_questions_pad_shows_half_its_colour_and_an_open_ones_all_of_it() {
        // Lachlan's 16a of 2026-11-14
        let map = Map {
            messages: vec![reply("r", 5_000, &["Q1", "Q2", "Q3"])],
        };
        let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("1a\n2?"))]);
        let fills: Vec<f64> = rows(&map, &t).iter().map(Row::fill).collect();
        // Q1 answered; Q2 asked back, so still open; Q3 not named
        assert_eq!(fills, vec![ANSWERED_FILL, 1.0, 1.0]);
        assert_eq!(ANSWERED_FILL, 0.5);
    }

    #[test]
    fn a_reply_asking_one_question_is_answered_by_his_next_prompt_in_words() {
        // Lachlan's 6a of 2026-11-14
        let map = Map {
            messages: vec![reply("r", 5_000, &["Q1"])],
        };
        let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("yes, do that"))]);
        assert_eq!(shown(&rows(&map, &t)), vec![("Q1".into(), true)]);
        // A task finishing isn't his prompt: it waits for his
        let t = turns(&[(1_000, 2_000, None), (6_000, 7_000, None)]);
        assert_eq!(shown(&rows(&map, &t)), vec![("Q1".into(), false)]);
        // Asking back by number still leaves it open, as for any question
        let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("1?"))]);
        assert_eq!(shown(&rows(&map, &t)), vec![("Q1".into(), false)]);
        // An extra alone, or a question with an extra: words answer neither
        for labels in [&["E1"][..], &["Q1", "E2"][..]] {
            let map = Map {
                messages: vec![reply("r", 5_000, labels)],
            };
            let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("sounds good"))]);
            assert!(rows(&map, &t).iter().all(|r| !r.answered()), "{labels:?}");
        }
        // A newer reply had ended before his words: they're its, not the older one's
        let map = Map {
            messages: vec![reply("old", 5_000, &["Q1"]), reply("new", 15_000, &["Q1"])],
        };
        let t = turns(&[(1_000, 2_000, None), (8_000, 10_000, None), (20_000, 21_000, Some("sounds good"))]);
        let rows = rows(&map, &t);
        assert!(row(&rows, "new", "Q1").answered());
        assert!(!row(&rows, "old", "Q1").answered());
    }

    #[test]
    fn words_alone_answer_nothing_when_the_reply_asks_more_than_one_and_a_question_with_no_number_waits() {
        let mut map = Map {
            messages: vec![reply("r", 5_000, &["Q1"])],
        };
        map.messages[0].questions.push(question("", None));
        let t = turns(&[(1_000, 2_000, None), (8_000, 9_000, Some("yes, do that")), (10_000, 11_000, Some("2+"))]);
        let rows = rows(&map, &t);
        assert_eq!(shown(&rows), vec![("Q1".into(), false), ("Q2".into(), false)]);
        assert_eq!(rows[1].number, None);
    }

    #[test]
    fn a_reply_with_no_time_is_answered_by_nothing_and_one_before_every_turn_by_all() {
        let map = Map {
            messages: vec![reply("none", 0, &["Q1"]), reply("early", 500, &["Q2"])],
        };
        let t = turns(&[(1_000, 2_000, Some("1a\n2a"))]);
        let rows = rows(&map, &t);
        assert_eq!(row(&rows, "none", "Q1").pick, None);
        assert!(row(&rows, "early", "Q2").answered());
    }

    /// He answers an earlier reply while Claude works on something else: Claude Code writes his
    /// answer as a queued command, never a `user` record, and it still ticks the reply before it
    #[test]
    fn an_answer_typed_while_claude_worked_ticks_the_reply_before_it() {
        use crate::conversation::Follow;
        use serde_json::json;
        let at = |s: u32| format!("2026-10-28T04:00:{s:02}.000Z");
        let ms = |s: u32| chrono::DateTime::parse_from_rfc3339(&at(s)).unwrap().timestamp_millis();
        let records = [
            json!({"type": "user", "origin": {"kind": "human"}, "message": {"role": "user", "content": "Plan it"}, "timestamp": at(0)}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "Q1. Which?\n\nQ2. When?"}]}, "timestamp": at(2)}),
            json!({"type": "system", "subtype": "turn_duration", "durationMs": 2000, "timestamp": at(3)}),
            json!({"type": "user", "origin": {"kind": "human"}, "message": {"role": "user", "content": "Meanwhile, tidy the logs"}, "timestamp": at(20)}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "Tidying."}]}, "timestamp": at(21)}),
            json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": "1+\n2a", "commandMode": "prompt", "origin": {"kind": "human"}}, "timestamp": at(30)}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "Done, and 1 and 2a noted."}]}, "timestamp": at(40)}),
        ];
        let mut f = Follow::default();
        let text: String = records.iter().map(|r| format!("{r}\n")).collect();
        f.feed(text.as_bytes());
        let snap = f.conversation.snapshot();
        // The worker wrote the reply down 12 s after it ended
        let map = Map {
            messages: vec![reply("r", ms(3) + 12_000, &["Q1", "Q2"])],
        };
        assert_eq!(shown(&rows(&map, &snap.turns)), vec![("Q1".into(), true), ("Q2".into(), true)]);
        // Read as `user` records only, as before, it stayed open
        let typed: String = records[..5].iter().map(|r| format!("{r}\n")).collect();
        let mut f = Follow::default();
        f.feed(typed.as_bytes());
        assert_eq!(shown(&rows(&map, &f.conversation.snapshot().turns)), vec![("Q1".into(), false), ("Q2".into(), false)]);
    }

    #[test]
    fn a_label_with_a_stop_after_it_shows_without_it() {
        let map = Map {
            messages: vec![Message {
                id: "r".into(),
                at: 5_000,
                questions: vec![question("Q4.", None), question("", Some(2)), question("", None)],
            }],
        };
        let rows = rows(&map, &turns(&[]));
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), vec!["Q4", "Q2", "Q3"]);
        assert_eq!(rows.iter().map(|r| r.number).collect::<Vec<_>>(), vec![Some(4), Some(2), None]);
    }

    fn rows_of(ages: &[usize]) -> Vec<Row> {
        ages.iter()
            .enumerate()
            .map(|(i, &age)| Row {
                reply: format!("r{age}"),
                age,
                label: format!("Q{}", i + 1),
                number: Some(i as u32 + 1),
                colour: i,
                extra: false,
                pick: None,
            })
            .collect()
    }

    #[test]
    fn the_bar_lays_its_rows_down_as_the_rail_does_with_a_hairline_between_replies() {
        // Two of the newest reply's, then one of the reply before, in plenty of room: every slot
        // as tall as a slot gets (40), 10 between replies, as the rail's rows are when it has room
        let placed = place(&rows_of(&[0, 0, 1]), 1_000.0, 24.0, 40.0, 10.0);
        assert_eq!(placed.row_h, 40.0);
        assert_eq!(placed.rows, vec![(0, 0.0), (1, 40.0), (2, 90.0)]);
        assert_eq!(placed.rules, vec![85.0]);
        assert_eq!((placed.more, placed.more_at), (0, 130.0));
        // Less room: the slots shrink together to fit all three, (100 - 10) / 3 = 30
        let placed = place(&rows_of(&[0, 0, 1]), 100.0, 24.0, 40.0, 10.0);
        assert_eq!((placed.row_h, placed.more), (30.0, 0));
        assert_eq!(placed.rows, vec![(0, 0.0), (1, 30.0), (2, 70.0)]);
    }

    #[test]
    fn rows_that_dont_fit_at_their_least_are_counted_in_a_slot_of_their_own_never_dropped() {
        let rows = rows_of(&[0, 0, 0, 1, 1, 1]);
        // All six at their least take 6 x 20 + 10 = 130
        let placed = place(&rows, 130.0, 20.0, 40.0, 10.0);
        assert_eq!((placed.row_h, placed.more), (20.0, 0));
        // One short: the "+N" slot takes a row's room, so two rows go and it says "+2" where the
        // fifth was, the hairline before them kept
        let placed = place(&rows, 129.0, 20.0, 40.0, 10.0);
        assert_eq!(placed.rows.len(), 4);
        assert_eq!((placed.row_h, placed.more, placed.more_at), (20.0, 2, 90.0));
        assert_eq!(placed.rules, vec![65.0]);
        // Room for the newest reply and its "+3" slot only: the hairline after it goes too
        let placed = place(&rows, 85.0, 20.0, 40.0, 10.0);
        assert_eq!(placed.rows.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!((placed.more, placed.more_at, placed.rules.len()), (3, 60.0, 0));
        // No room for any: "+6" alone
        let placed = place(&rows, 30.0, 20.0, 40.0, 10.0);
        assert_eq!((placed.rows.len(), placed.more, placed.more_at), (0, 6, 0.0));
        // Every row's either shown or counted, and what's shown fits
        for room in [0.0, 40.0, 75.0, 99.0, 120.0, 129.0, 130.0, 500.0] {
            let placed = place(&rows, room, 20.0, 40.0, 10.0);
            assert_eq!(placed.rows.len() + placed.more, rows.len(), "room {room}");
            let end = if placed.more > 0 { placed.more_at + placed.row_h } else { placed.more_at };
            assert!(end <= room.max(placed.row_h), "room {room}: ends at {end}");
        }
    }

    #[test]
    fn a_session_with_no_questions_says_so_in_the_first_slot() {
        let placed = place(&[], 500.0, 20.0, 40.0, 10.0);
        assert_eq!((placed.rows.len(), placed.more, placed.more_at, placed.row_h), (0, 0, 0.0, 40.0));
    }
}
