//! A conversation the size of Lachlan's longest (2026-10-30: transcripts of 30 MB and more, one
//! of 5,800 rows), made up here so no real transcript is ever read into a test: what
//! `perf/fake-claude.ps1 -Reads` draws, 276 messages of a prompt, a fold of five file reads, a
//! 14-line answer and the turn's end, 5,801 rows in all, with each read's file in its record twice
//! as Claude Code writes it (the tool result's words and `toolUseResult`'s file), about 175 KB a
//! message and 46 MB in all, and a hook's progress record after each call.
//! `perf/conversation-cost.ps1` writes the same transcript for its test window.
//!
//! [`bench`] says what reading it and each append to it cost, when `LBTERM_CONVERSATION_BENCH` is
//! set (the run's numbers on stderr, and in that file when it names one); without it, it checks
//! only that the stand-in reads into the rows fake-claude.ps1 draws.

use super::*;
use serde_json::json;
use std::time::{Duration, Instant};

/// The messages of the long stand-in, and the reads in each one's fold
pub(super) const MESSAGES: usize = 276;
pub(super) const READS: usize = 5;
/// The lines of each file read: about 11 KB of code
pub(super) const FILE_LINES: usize = 200;
/// The session's id
pub(super) const SID: &str = "6a1f0c3e-2b7d-4e95-8c41-9d3b5f2e7a60";

/// What fake-claude.ps1 draws of the conversation, row by row: (kind, words)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Drawn {
    Banner,
    Blank,
    User,
    Fold,
    First,
    Text,
    Turn,
}

fn stamp_at(second: usize) -> String {
    let t = chrono::DateTime::from_timestamp(1_793_000_000 + second as i64, 0).unwrap();
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// One record, as Claude Code 2.1.312 writes it: the session's fields first, the record's own,
/// then its type after its message (so a search for the type reads past the message first)
fn record(fields: Value, kind: &str, second: usize, n: usize) -> String {
    let mut r = json!({
        "parentUuid": format!("00000000-0000-4000-8000-{:012}", n.saturating_sub(1)),
        "isSidechain": false,
        "userType": "external",
        "cwd": "C:\\work\\demo",
        "sessionId": SID,
        "version": "2.1.312",
        "gitBranch": "main",
    });
    for (k, v) in fields.as_object().unwrap() {
        r[k] = v.clone();
    }
    r["type"] = json!(kind);
    r["uuid"] = json!(format!("00000000-0000-4000-8000-{n:012}"));
    r["timestamp"] = json!(stamp_at(second));
    r.to_string()
}

/// A file of `lines` lines of code, as the Read tool returns it: numbered, an arrow after each
/// number
fn file(m: usize, r: usize, lines: usize) -> String {
    (1..=lines)
        .map(|i| format!("{i:6}\u{2192}    let value_{m}_{r}_{i} = compute(\"part {m}\", {i}) + offset; // step {i}\n"))
        .collect()
}

/// The words of message `m`'s prompt, and of its answer's line `j`, as fake-claude.ps1 draws them
pub(super) fn prompt_words(m: usize) -> String {
    format!("message number {m}, about part {m} of the work")
}
pub(super) fn answer_words(m: usize, j: usize) -> String {
    format!("answer {m}, line {j}")
}

/// The transcript: `messages` of them, each with `reads` file reads of `file_lines` lines folded
/// before its answer
pub(super) fn transcript(messages: usize, reads: usize, file_lines: usize) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    let push = |out: &mut String, line: String| {
        out.push_str(&line);
        out.push('\n');
    };
    for m in 1..=messages {
        let second = m * 60;
        n += 1;
        push(
            &mut out,
            record(
                json!({"message": {"role": "user", "content": prompt_words(m)}, "origin": {"kind": "human"}}),
                "user",
                second,
                n,
            ),
        );
        for r in 1..=reads {
            let id = format!("toolu_{m:04}_{r}");
            let path = format!("C:\\work\\demo\\src\\part{m}\\file{r}.rs");
            n += 1;
            push(
                &mut out,
                record(
                    json!({
                        "message": {"model": "claude-opus-4", "id": format!("msg_{m:04}_{r}"), "type": "message", "role": "assistant",
                            "content": [{"type": "tool_use", "id": id, "name": "Read", "input": {"file_path": path}}],
                            "stop_reason": "tool_use", "usage": {"input_tokens": 12, "cache_read_input_tokens": 40000, "output_tokens": 90}},
                        "requestId": format!("req_{m:04}_{r}"),
                    }),
                    "assistant",
                    second + r,
                    n,
                ),
            );
            n += 1;
            push(
                &mut out,
                record(
                    json!({"data": {"type": "hook_progress", "hookEvent": "PostToolUse", "hookName": "PostToolUse:Read", "command": "claudehook.exe posttool"},
                        "parentToolUseID": id, "toolUseID": id}),
                    "progress",
                    second + r,
                    n,
                ),
            );
            let content = file(m, r, file_lines);
            n += 1;
            push(
                &mut out,
                record(
                    json!({
                        "message": {"role": "user", "content": [{"tool_use_id": id, "type": "tool_result", "content": content}]},
                        "toolUseResult": {"type": "text", "file": {"filePath": path, "content": content, "numLines": file_lines, "startLine": 1, "totalLines": file_lines}},
                    }),
                    "user",
                    second + r,
                    n,
                ),
            );
        }
        let answer: Vec<String> = (1..=14).map(|j| answer_words(m, j)).collect();
        n += 1;
        push(
            &mut out,
            record(
                json!({
                    "message": {"model": "claude-opus-4", "id": format!("msg_{m:04}_t"), "type": "message", "role": "assistant",
                        "content": [{"type": "text", "text": answer.join("\n")}],
                        "stop_reason": "end_turn", "usage": {"input_tokens": 12, "cache_read_input_tokens": 40000, "output_tokens": 300}},
                    "requestId": format!("req_{m:04}_t"),
                }),
                "assistant",
                second + 30,
                n,
            ),
        );
        n += 1;
        push(&mut out, record(json!({"subtype": "turn_duration", "durationMs": 12000, "isMeta": false}), "system", second + 31, n));
    }
    out
}

/// One more of Claude's words at the end, as a record of its own: what each append is
pub(super) fn more_words(i: usize) -> String {
    record(
        json!({"message": {"model": "claude-opus-4", "id": format!("msg_more_{i}"), "type": "message", "role": "assistant",
            "content": [{"type": "text", "text": format!("more words {i}, written while the test watched")}],
            "usage": {"input_tokens": 1, "output_tokens": 9}}}),
        "assistant",
        1_000_000 + i,
        1_000_000 + i,
    )
}

/// A call shown on its own (an edit), then its result: the result changes the call's block in
/// place, the blocks' count the same
pub(super) fn an_edit(i: usize) -> (String, String) {
    let id = format!("toolu_edit_{i}");
    let call = record(
        json!({"message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": "Edit",
            "input": {"file_path": "C:\\work\\demo\\src\\main.rs", "old_string": "a", "new_string": "b"}}]}}),
        "assistant",
        2_000_000 + i,
        2_000_000 + i,
    );
    let result = record(
        json!({"message": {"role": "user", "content": [{"tool_use_id": id, "type": "tool_result", "content": "The file was updated"}]},
            "toolUseResult": {"filePath": "C:\\work\\demo\\src\\main.rs", "structuredPatch": [{"oldStart": 10, "oldLines": 1, "newStart": 10, "newLines": 1,
                "lines": [format!("-    let a = {i};"), format!("+    let b = {i};")]}]}}),
        "user",
        2_000_000 + i,
        2_000_001 + i,
    );
    (call, result)
}

/// What fake-claude.ps1 -Reads draws, row by row
pub(super) fn drawn(messages: usize) -> Vec<(Drawn, String)> {
    let mut rows: Vec<(Drawn, String)> = (0..BANNER_ROWS).map(|_| (Drawn::Banner, String::new())).collect();
    for m in 1..=messages {
        rows.push((Drawn::Blank, String::new()));
        rows.push((Drawn::User, prompt_words(m)));
        rows.push((Drawn::Blank, String::new()));
        rows.push((Drawn::Fold, format!("Read {READS} files")));
        rows.push((Drawn::Blank, String::new()));
        for j in 1..=14 {
            rows.push((if j == 1 { Drawn::First } else { Drawn::Text }, answer_words(m, j)));
        }
        rows.push((Drawn::Blank, String::new()));
        rows.push((Drawn::Turn, "Worked for 12s \u{b7} done 09:00".into()));
    }
    rows
}

/// The screen fake-claude.ps1 draws `offset` rows up from the latest, `area` rows of
/// conversation over its eight of input box and status
pub(super) fn screen(lines: &[(Drawn, String)], offset: usize, area: usize) -> Vec<Row> {
    let max = lines.len().saturating_sub(area);
    let offset = offset.min(max);
    let first = lines.len().saturating_sub(area + offset);
    let scrolled = offset > 0;
    let plain = |text: String| Row { text, band: false, dim: false };
    let mut rows = vec![];
    for r in 0..area {
        let i = first + r;
        if r == 0 && scrolled && lines.get(i).is_some_and(|l| l.0 != Drawn::User) {
            if let Some(above) = lines[..i].iter().rev().find(|l| l.0 == Drawn::User) {
                rows.push(Row { text: format!("\u{276f} {}", above.1), band: true, dim: true });
                continue;
            }
        }
        if r == area - 1 && scrolled {
            rows.push(plain(format!("{:40}  Jump to bottom (ctrl+End) \u{2193} ", "")));
            continue;
        }
        let Some((kind, text)) = lines.get(i) else {
            rows.push(plain(String::new()));
            continue;
        };
        rows.push(match kind {
            Drawn::User => Row { text: format!("\u{276f} {text}"), band: true, dim: false },
            Drawn::First => plain(format!("\u{25cf} {text}")),
            Drawn::Text | Drawn::Fold => plain(format!("  {text}")),
            Drawn::Turn => plain(format!("\u{273b} {text}")),
            Drawn::Banner | Drawn::Blank => plain(text.clone()),
        });
    }
    let rule = "\u{2500}".repeat(110);
    for t in [String::new(), rule.clone(), "\u{276f} ".into(), rule] {
        rows.push(plain(t));
    }
    for t in ["  \u{25b8} a summary of the session", "  haiku | demo | main", "  toggles", "  accept edits on"] {
        rows.push(plain(t.into()));
    }
    rows
}

/// The long stand-in read whole
pub(super) fn read_long() -> (Follow, usize) {
    let text = transcript(MESSAGES, READS, FILE_LINES);
    let mut f = Follow::default();
    for chunk in text.as_bytes().chunks(READ_MAX as usize) {
        f.feed(chunk);
    }
    (f, text.len())
}

#[test]
fn the_long_stand_in_reads_into_the_rows_fake_claude_draws() {
    let (mut f, bytes) = read_long();
    assert!(bytes > 30_000_000, "as long as Lachlan's longest transcripts: {bytes} bytes");
    let s = f.conversation.snapshot();
    let layout = s.layout(110);
    let lines = drawn(MESSAGES);
    assert_eq!(layout.total, lines.len(), "the layout's rows are fake-claude.ps1's");
    assert_eq!(layout.total, 5_801);
    // Where the view is, at the latest and paging up through it
    let area = 43;
    for offset in [0, 21, 400, 2_900, 5_000, lines.len() - area] {
        let rows = screen(&lines, offset, area);
        let view = s.locate(&layout, &rows, None).unwrap();
        let top = lines.len() - area - offset.min(lines.len() - area);
        assert!(view.top.abs_diff(top) <= 1, "offset {offset}: found {} for {top}", view.top);
    }
}

/// The median of `n` runs of `f`, in microseconds
fn median(n: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<Duration> = (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    times.sort();
    times[n / 2].as_secs_f64() * 1e6
}

/// What reading the long stand-in and each append to it cost, with `LBTERM_CONVERSATION_BENCH`
/// set; nothing without
#[test]
fn bench() {
    let Some(out) = std::env::var_os("LBTERM_CONVERSATION_BENCH") else {
        return;
    };
    let mut report = vec![];
    let mut say = |line: String| {
        eprintln!("{line}");
        report.push(line);
    };
    let text = transcript(MESSAGES, READS, FILE_LINES);
    let mb = text.len() as f64 / 1_048_576.0;
    say(format!("stand-in: {} bytes ({mb:.1} MB), {MESSAGES} messages", text.len()));

    // Reading it from nothing, as a first look (or a look after it was forgotten) does
    let mut reads = vec![];
    for _ in 0..5 {
        let t = Instant::now();
        let mut f = Follow::default();
        for chunk in text.as_bytes().chunks(READ_MAX as usize) {
            f.feed(chunk);
        }
        reads.push(t.elapsed().as_secs_f64() * 1e3);
    }
    reads.sort_by(f64::total_cmp);
    say(format!("read from nothing: {:.1} ms ({:.2} ms/MB), runs {reads:.1?}", reads[2], reads[2] / mb));

    let (mut f, _) = read_long();
    let s = Arc::new(f.conversation.snapshot());
    say(format!("blocks {}, events {}, pieces {}", s.blocks.len(), s.events.len(), s.blocks.pieces()));
    say(format!(
        "snapshot from nothing: {:.0} us",
        median(21, || {
            let mut c = f.conversation.clone();
            c.dirty = 0;
            drop(c.snapshot());
        })
    ));
    say(format!("snapshot, nothing new: {:.1} us", median(21, || drop(f.conversation.snapshot()))));
    say(format!("events from nothing: {:.0} us", median(21, || drop(f.conversation.events()))));
    say(format!("layout whole: {:.0} us", median(21, || drop(s.layout(110)))));

    // Where the view is, read off the screen: at the latest, and scrolled halfway up
    let layout = s.layout(110);
    let lines = drawn(MESSAGES);
    let latest = screen(&lines, 0, 43);
    let middle = screen(&lines, 2_900, 43);
    say(format!("locate at the latest: {:.0} us", median(21, || { std::hint::black_box(s.locate(&layout, &latest, None)); })));
    say(format!("locate halfway up: {:.0} us", median(21, || { std::hint::black_box(s.locate(&layout, &middle, Some(2_850))); })));

    // Each append, as the reading thread and then the GUI take it: one of Claude's words, then an
    // edit and its result (the result changes the call's block in place). The reading thread gives
    // a snapshot out when the revision moved; the GUI lays it out from where it parts from the last
    let mut last = s;
    let mut last_layout = layout;
    let mut appends = vec![];
    let mut layouts = vec![];
    for i in 0..41 {
        let t = Instant::now();
        f.feed((more_words(i) + "\n").as_bytes());
        assert_ne!(f.conversation.revision(), last.revision);
        let next = Arc::new(f.conversation.snapshot());
        appends.push(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        let laid = next.layout_from(&last_layout, next.blocks.first_difference(&last.blocks));
        layouts.push(t.elapsed().as_secs_f64() * 1e6);
        assert_eq!(laid, next.layout(110));
        (last, last_layout) = (next, laid);
    }
    appends.sort_by(f64::total_cmp);
    layouts.sort_by(f64::total_cmp);
    say(format!("append of words, read to the snapshot: {:.0} us; laid out: {:.1} us", appends[20], layouts[20]));
    let mut results = vec![];
    for i in 0..41 {
        let (call, result) = an_edit(i);
        f.feed((call + "\n").as_bytes());
        last = Arc::new(f.conversation.snapshot());
        let t = Instant::now();
        f.feed((result + "\n").as_bytes());
        assert_ne!(f.conversation.revision(), last.revision);
        let next = Arc::new(f.conversation.snapshot());
        results.push(t.elapsed().as_secs_f64() * 1e6);
        last = next;
    }
    results.sort_by(f64::total_cmp);
    say(format!("append of a result (its call's block changed in place), read to the snapshot: {:.0} us", results[20]));
    drop(last);

    // A summary looked up, as the rail asks for every tab's on every frame
    let dir = tempfile::tempdir().unwrap();
    let sid = "5f0c2d7e-1b3a-4e6f-9a8b-7c6d5e4f3a21";
    let write = |suffix: &str, text: &str| std::fs::write(dir.path().join(format!("{sid}{suffix}")), text).unwrap();
    write(".txt", "Scroll strip: an append costs what it appends, the summaries cloned no more");
    write(".label.txt", "Scroll strip costs");
    write(".detail.txt", "Goal: the strip at rest costs nothing\nFinding: each append redid the conversation\nNext: measure it\nThen: hold it\nLast: land it\n");
    write(".cwd.txt", "C:\\work\\demo");
    let summaries = crate::summaries::Summaries::start(dir.path().to_path_buf(), |_| {});
    let deadline = Instant::now() + Duration::from_secs(5);
    while summaries.get(sid).is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(summaries.get(sid).is_some());
    let per = median(201, || {
        for _ in 0..50 {
            std::hint::black_box(summaries.get(sid));
        }
    });
    say(format!("50 summary lookups: {per:.1} us ({:.0} ns each)", per * 1000.0 / 50.0));

    if !out.is_empty() && out != "1" {
        std::fs::write(&out, report.join("\n") + "\n").unwrap();
    }
}
