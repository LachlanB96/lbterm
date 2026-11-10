//! The snapshot in pieces and its lookups held to what the whole-conversation passes they
//! replaced gave (2026-10-30): every prefix of a transcript snapshots, marks and lays out as if
//! made from nothing, and every row of every screen the tests know (the made-up ones, the real
//! capture's, the long stand-in's) is found where a pass over every block found it.

use super::long;
use super::*;
use serde_json::json;

impl Snapshot {
    /// What [`Snapshot::candidates`] was: every block's key tried
    fn candidates_by_scan(&self, row: &Row) -> Vec<usize> {
        match anchor(row) {
            Some(Anchor::Prompt(text)) => {
                let shown = prompts::one_line(text);
                self.blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.kind == BlockKind::Prompt)
                    .filter(|(_, b)| b.key.as_deref().is_some_and(|k| prompt_matches(&shown, k)))
                    .map(|(i, _)| i)
                    .collect()
            }
            Some(Anchor::Text(text)) => self
                .blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind == BlockKind::Text)
                .filter(|(_, b)| b.key.as_deref().is_some_and(|k| matches_text(text, k)))
                .map(|(i, _)| i)
                .collect(),
            None => vec![],
        }
    }

    /// What [`Snapshot::places`] was: every line whose key starts as the row does, tried
    fn places_by_scan(&self, layout: &Layout, row: &Row) -> Vec<(usize, usize)> {
        let firsts = self.candidates_by_scan(row);
        if !firsts.is_empty() {
            return firsts.into_iter().map(|b| (b, 1)).collect();
        }
        if row.band {
            return vec![];
        }
        let text = bare(&row.text);
        let head: String = text.chars().take(LINE_MIN).collect();
        if head.chars().count() < LINE_MIN {
            return vec![];
        }
        let mut out = vec![];
        for (b, block) in self.blocks.iter().enumerate() {
            for (k, key) in block.line_keys.iter().enumerate() {
                if !key.is_empty() && key.starts_with(&head) && shows_key(&text, key, LINE_KEY, LINE_KEY) {
                    out.push((b, block.row_of_line(k, layout.cols)));
                }
            }
        }
        out
    }

    /// What [`Snapshot::header_prompt`] was: every prompt tried
    fn header_prompt_by_scan(&self, layout: &Layout, row: &Row, near: Option<usize>) -> Option<usize> {
        let text = prompts::one_line(row.text.trim_start().strip_prefix(prompts::MARK)?);
        let with: Vec<usize> = self
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == BlockKind::Prompt && b.key.as_deref().is_some_and(|k| header_names(&text, k, &b.text)))
            .map(|(i, _)| i)
            .collect();
        match near {
            Some(n) => with.into_iter().min_by_key(|&b| layout.tops[b].abs_diff(n)),
            None => with.last().copied(),
        }
    }
}

/// The snapshot `c` would give made from nothing, every piece built again
fn from_nothing(c: &Conversation) -> Snapshot {
    let mut c = c.clone();
    c.dirty = 0;
    c.snapshot()
}

/// Feeds `lines` one at a time, and after each: the snapshot in pieces is the one made from
/// nothing, its marks are [`Conversation::events`], its layout grown from the last is the whole
/// layout, and its pieces before the first changed block are the last snapshot's own
fn grows_as_if_whole(lines: &[String], permissions_at: Option<(usize, &str)>) {
    let mut f = Follow::default();
    let mut last: Option<(Snapshot, Layout)> = None;
    for (n, line) in lines.iter().enumerate() {
        if let Some((_, text)) = permissions_at.filter(|(at, _)| *at == n) {
            f.conversation.permissions(text);
        }
        let revision = f.conversation.revision();
        f.feed(format!("{line}\n").as_bytes());
        let s = f.conversation.snapshot();
        assert_eq!(s, from_nothing(&f.conversation), "line {n}");
        assert_eq!(s.events.iter().cloned().collect::<Vec<_>>(), f.conversation.events(), "line {n}");
        assert_eq!(s.revision, f.conversation.revision());
        let layout = match &last {
            Some((was, was_layout)) => {
                let from = s.blocks.first_difference(&was.blocks);
                for c in 0..from / CHUNK {
                    assert!(s.blocks.shares_piece(&was.blocks, c), "line {n}: piece {c} copied");
                }
                if f.conversation.revision() == revision {
                    assert_eq!(from, s.blocks.len(), "line {n}: nothing new, nothing changed");
                }
                s.layout_from(was_layout, from)
            }
            None => s.layout(80),
        };
        assert_eq!(layout, s.layout(80), "line {n}");
        last = Some((s, layout));
    }
}

/// A conversation long enough for several pieces: prompts, words, calls folded and shown on
/// their own, results changing their blocks in place, a question at a turn's end
fn several_pieces(messages: usize) -> Vec<String> {
    let mut out = vec![];
    let at = |s: usize| format!("2026-10-30T{:02}:{:02}:{:02}.000Z", 10 + s / 3600, (s / 60) % 60, s % 60);
    let rec = |mut r: Value, s: usize| {
        r["timestamp"] = json!(at(s));
        r["isSidechain"] = json!(false);
        r.to_string()
    };
    for m in 0..messages {
        let s = m * 20;
        out.push(rec(json!({"type": "user", "message": {"role": "user", "content": format!("message {m} of the work")}}), s));
        out.push(rec(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": format!("r{m}"), "name": "Bash", "input": {"command": if m % 7 == 0 { format!("git commit -m \"part {m}\"") } else { format!("cargo build -p part{m}") }}}]}}), s + 1));
        out.push(rec(json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": format!("r{m}"), "content": if m % 5 == 0 { "error: it broke" } else { "ok" }, "is_error": m % 5 == 0}]}}), s + 2));
        out.push(rec(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": format!("e{m}"), "name": "Edit", "input": {"file_path": format!("C:\\work\\demo\\src\\part{m}.rs")}}]}}), s + 3));
        out.push(rec(json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": format!("e{m}"), "content": "updated"}]},
            "toolUseResult": {"structuredPatch": [{"oldStart": 3, "oldLines": 1, "newStart": 3, "newLines": 2, "lines": [format!("-    let old_{m} = 1;"), format!("+    let new_{m} = 2;"), "+    // and another line".to_string()]}]}}), s + 4));
        let words = if m % 3 == 0 { format!("Done part {m}.\n\nShall I go on to part {}?", m + 1) } else { format!("Done part {m}, with the build passing.") };
        out.push(rec(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": words}]}}), s + 5));
        out.push(rec(json!({"type": "system", "subtype": "turn_duration", "durationMs": 9000}), s + 6));
    }
    out
}

#[test]
fn every_prefix_of_a_transcript_snapshots_as_if_made_whole() {
    let lines: Vec<String> = super::tests::transcript_lines();
    grows_as_if_whole(&lines, None);
    let real: Vec<String> = super::real::transcript_lines();
    grows_as_if_whole(&real, None);
}

#[test]
fn a_conversation_of_several_pieces_grows_a_piece_at_a_time() {
    let lines = several_pieces(120);
    // A hundred and twenty messages of five blocks each: more than four pieces
    grows_as_if_whole(&lines, None);
    let mut f = Follow::default();
    f.feed((lines.join("\n") + "\n").as_bytes());
    assert!(f.conversation.blocks.pieces() > 4, "{} pieces", f.conversation.blocks.pieces());
}

#[test]
fn permission_prompts_move_to_the_latest_call_as_calls_come() {
    let lines = several_pieces(40);
    // Recorded before the calls they were for are read: each moves to its call once it comes,
    // and again when a later call of that tool before it comes
    let mut permissions = String::new();
    for m in [3usize, 17, 33] {
        let at = chrono::DateTime::parse_from_rfc3339(&format!("2026-10-30T10:{:02}:{:02}.000Z", (m * 20 + 1) / 60, (m * 20 + 1) % 60)).unwrap();
        permissions += &format!("{{\"at\":{},\"event\":\"PermissionRequest\",\"tool\":\"Bash\"}}\n", at.timestamp_millis() + 300);
    }
    grows_as_if_whole(&lines, Some((0, &permissions)));
    grows_as_if_whole(&lines, Some((lines.len() / 2, &permissions)));
    let mut f = Follow::default();
    f.feed((lines.join("\n") + "\n").as_bytes());
    f.conversation.permissions(&permissions);
    let asked = f.conversation.snapshot().events.iter().filter(|e| e.kind == Kind::Permission).count();
    assert_eq!(asked, 3);
    // Taken away again: no marks, and the pieces they were on made again
    f.conversation.permissions("");
    let s = f.conversation.snapshot();
    assert_eq!(s.events.iter().filter(|e| e.kind == Kind::Permission).count(), 0);
    assert_eq!(s, from_nothing(&f.conversation));
}

/// Every lookup a screen's rows make, the same as the passes over every block
fn finds_as_the_scans_did(s: &Snapshot, layout: &Layout, rows: &[Row]) {
    for row in rows {
        assert_eq!(s.candidates(row), s.candidates_by_scan(row), "{:?}", row.text);
        assert_eq!(s.places(layout, row), s.places_by_scan(layout, row), "{:?}", row.text);
        for near in [None, Some(0), Some(layout.total / 2), Some(layout.total)] {
            assert_eq!(s.header_prompt(layout, row, near), s.header_prompt_by_scan(layout, row, near), "{:?}", row.text);
        }
    }
}

#[test]
fn every_row_is_found_where_a_pass_over_every_block_found_it() {
    // The real capture's screens
    let s = super::real::snapshot();
    let layout = s.layout(super::real::COLS);
    for rows in super::real::screens() {
        finds_as_the_scans_did(&s, &layout, &rows);
    }
    // The long stand-in's, paging up from the latest, and a header cut short
    let (mut f, _) = long::read_long();
    let s = f.conversation.snapshot();
    let layout = s.layout(110);
    let lines = long::drawn(long::MESSAGES);
    for offset in (0..lines.len()).step_by(997).chain([21, 43, 5_000]) {
        finds_as_the_scans_did(&s, &layout, &long::screen(&lines, offset, 43));
    }
    let cut = |text: &str, band: bool, dim: bool| Row { text: text.into(), band, dim };
    finds_as_the_scans_did(
        &s,
        &layout,
        &[
            cut("\u{276f} message number 27, about part\u{2026}", true, true),
            cut("\u{276f} message number 2\u{2026}", true, true),
            cut("\u{276f} message nu\u{2026}", true, true),
            cut("\u{276f} message number 140, about part 140 of the work", true, false),
            cut("\u{25cf} answer 140, line 1", false, false),
            cut("\u{25cf} answer 14", false, false),
            cut("\u{25cf} answ", false, false),
            cut("  answer 140, line 7", false, false),
            cut("  answer 140, line 7 and more words past the key's twenty-four", false, false),
        ],
    );
    // The made-up session's
    let lines = several_pieces(60);
    let mut f = Follow::default();
    f.feed((lines.join("\n") + "\n").as_bytes());
    let s = f.conversation.snapshot();
    let layout = s.layout(80);
    let rows: Vec<Row> = (0..60)
        .flat_map(|m| {
            [
                cut(&format!("\u{276f} message {m} of the work"), true, false),
                cut(&format!("\u{276f} message {m} of the work"), true, true),
                cut(&format!("\u{25cf} Done part {m}, with the build"), false, false),
                cut(&format!("  Shall I go on to part {}?", m + 1), false, false),
                cut(&format!("       {}     let new_{m} = 2;", 3), false, false),
            ]
        })
        .collect();
    finds_as_the_scans_did(&s, &layout, &rows);
}
