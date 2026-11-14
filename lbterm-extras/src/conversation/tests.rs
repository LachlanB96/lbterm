use super::*;
use serde_json::json;

/// One line of a transcript, as Claude Code writes it: `record` with its time
fn line(record: Value, at: &str) -> String {
    let mut record = record;
    record["timestamp"] = json!(format!("2026-10-28T{at}Z"));
    if record.get("isSidechain").is_none() {
        record["isSidechain"] = json!(false);
    }
    record.to_string()
}

fn prompt(text: &str, at: &str) -> String {
    line(json!({"type": "user", "origin": {"kind": "human"}, "message": {"role": "user", "content": text}}), at)
}

fn says(text: &str, at: &str) -> String {
    line(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}}), at)
}

fn tool(id: &str, name: &str, input: Value, at: &str) -> String {
    line(
        json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}),
        at,
    )
}

fn result(id: &str, text: &str, error: bool, at: &str) -> String {
    line(
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": text, "is_error": error}]}}),
        at,
    )
}

fn turn_end(ms: i64, at: &str) -> String {
    line(json!({"type": "system", "subtype": "turn_duration", "durationMs": ms}), at)
}

/// A made-up session: two prompts, Claude's words, a commit, an edit, a failure, a call turned
/// down, a subagent, a picker, an InMail, a question at a turn's end, an interrupt, a
/// compaction, and records the strip leaves alone
fn transcript() -> String {
    [
        line(json!({"type": "user", "isMeta": true, "message": {"role": "user", "content": "<local-command-caveat>x</local-command-caveat>"}}), "01:00:00.000"),
        line(json!({"type": "user", "message": {"role": "user", "content": "<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>"}}), "01:00:01.000"),
        prompt("Fix the build of the demo app", "01:00:02.000"),
        says("Looking at the **build** script first.", "01:00:03.000"),
        tool("t1", "Bash", json!({"command": "cargo build", "description": "Build it"}), "01:00:04.000"),
        result("t1", "error: could not compile `demo`\nmore", true, "01:00:05.000"),
        tool("t2", "Edit", json!({"file_path": "C:\\work\\demo\\src\\main.rs"}), "01:00:06.000"),
        result("t2", "ok", false, "01:00:07.000"),
        tool("t3", "Bash", json!({"command": "git -C C:/work/demo commit -am \"Fix the build\""}), "01:00:08.000"),
        result("t3", "[main 1234] Fix the build", false, "01:00:09.000"),
        tool("t4", "PowerShell", json!({"command": "Remove-Item x"}), "01:00:10.000"),
        result("t4", "The user doesn't want to proceed with this tool use. The tool use was rejected", true, "01:00:11.000"),
        line(json!({"type": "api-request-blob", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t9", "content": "x", "is_error": true}]}}), "01:00:11.500"),
        tool("t5", "Agent", json!({"description": "Search the logs", "subagent_type": "Explore"}), "01:00:12.000"),
        result("t5", "found", false, "01:00:13.000"),
        tool("t6", "Bash", json!({"command": "bun inmail.ts send Website \"New page\" --body -"}), "01:00:14.000"),
        result("t6", "sent", false, "01:00:15.000"),
        says("Fixed it.\n\nShould I push it too?", "01:00:16.000"),
        turn_end(14000, "01:00:17.000"),
        line(json!({"type": "user", "origin": {"kind": "task-notification"}, "message": {"role": "user", "content": "A background task finished"}}), "01:00:18.000"),
        prompt("Yes, and tidy up", "01:00:19.000"),
        line(json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}}), "01:00:20.000"),
        line(json!({"type": "system", "subtype": "compact_boundary", "content": "Conversation compacted"}), "01:00:21.000"),
        tool("t7", "AskUserQuestion", json!({"questions": [{"question": "Which branch?"}, {"question": "Push now?"}]}), "01:00:22.000"),
        result("t7", "answered", false, "01:00:23.000"),
        line(json!({"type": "assistant", "isSidechain": true, "message": {"role": "assistant", "content": [{"type": "text", "text": "a subagent's words"}]}}), "01:00:24.000"),
    ]
    .join("\n")
        + "\n"
}

/// The made-up session, line by line
pub(super) fn transcript_lines() -> Vec<String> {
    transcript().lines().map(String::from).collect()
}

fn read() -> Conversation {
    let mut f = Follow::default();
    f.feed(transcript().as_bytes());
    f.conversation
}

fn kinds(events: &[Event]) -> Vec<Kind> {
    events.iter().map(|e| e.kind).collect()
}

#[test]
fn the_blocks_are_what_claude_code_draws() {
    let c = read();
    let k: Vec<BlockKind> = c.blocks.iter().map(|b| b.kind).collect();
    use BlockKind::*;
    // /clear is a prompt; the runs of tool calls fold into one block until Claude writes again
    // or a call Claude Code shows on its own (the Edit, the subagent, the picker) comes between
    assert_eq!(
        k,
        vec![Prompt, Prompt, Text, Tools, Tool, Tools, Tool, Tools, Text, TurnEnd, Note, Prompt, Interrupt, Compaction, Tool]
    );
    assert_eq!(c.blocks[0].text, "/clear");
    assert_eq!(c.blocks[1].key.as_deref(), Some("Fix the build of the demo app"));
    assert_eq!(c.blocks[2].key.as_deref(), Some("lookingatthebuildscriptfirst"));
    // A call shown on its own is found by its header
    assert_eq!(c.blocks[4].key.as_deref(), Some("updatecworkdemosrcmainrs"));
    assert_eq!(c.blocks[6].key.as_deref(), Some("exploresearchthelogs"));
    assert_eq!(c.compacted_at, Some(13));
}

#[test]
fn every_kind_of_event_is_marked() {
    let c = read();
    let events = c.events();
    use Kind::*;
    assert_eq!(
        kinds(&events),
        vec![Prompt, Prompt, Error, Edit, Commit, Denied, Agent, Mail, Question, TurnEnd, Prompt, Interrupt, Compaction, Question]
    );
    let text = |kind: Kind| events.iter().find(|e| e.kind == kind).unwrap().text.clone();
    assert_eq!(text(Error), "Bash: error: could not compile `demo`");
    assert_eq!(text(Edit), "main.rs");
    assert_eq!(text(Commit), "Fix the build");
    assert_eq!(text(Denied), "PowerShell: Remove-Item x");
    assert_eq!(text(Agent), "Search the logs");
    assert!(text(Mail).contains("inmail.ts send Website"));
    assert_eq!(text(TurnEnd), "Worked for 14s");
    assert_eq!(events[8].text, "Should I push it too?");
    assert_eq!(events.last().unwrap().text, "Which branch? \u{b7} Push now?");
}

#[test]
fn a_permission_prompt_marks_the_call_it_was_for() {
    let mut c = read();
    // claudehook's record comes a moment after the call was written; another tool's is ignored
    let at = |t: &str| chrono::DateTime::parse_from_rfc3339(&format!("2026-10-28T{t}Z")).unwrap().timestamp_millis();
    c.permissions(&format!(
        "{}\n{}\nnot json\n{}\n",
        json!({"at": at("01:00:08.300"), "event": "PermissionRequest", "tool": "Bash"}),
        json!({"at": at("01:00:10.200"), "event": "PermissionRequest", "tool": "PowerShell"}),
        json!({"at": at("01:00:12.000"), "event": "PermissionDenied", "tool": "Agent"}),
    ));
    let events = c.events();
    let commit = events.iter().position(|e| e.kind == Kind::Commit).unwrap();
    assert_eq!(events[commit - 1].kind, Kind::Permission, "the commit was asked for");
    assert_eq!(events[commit - 1].text, "Bash: Fix the build");
    // The call turned down after the prompt is Denied, not Permission as well
    assert_eq!(events.iter().filter(|e| e.kind == Kind::Permission).count(), 1);
    assert_eq!(events.iter().filter(|e| e.kind == Kind::Denied).count(), 1);
}

#[test]
fn a_failed_commit_is_an_error_not_a_commit() {
    let mut f = Follow::default();
    let text = [
        prompt("commit it", "02:00:00.000"),
        tool("c1", "Bash", json!({"command": "git commit -m \"x\""}), "02:00:01.000"),
        result("c1", "gitleaks found a secret", true, "02:00:02.000"),
    ]
    .join("\n")
        + "\n";
    f.feed(text.as_bytes());
    assert_eq!(kinds(&f.conversation.events()), vec![Kind::Prompt, Kind::Error]);
}

#[test]
fn shell_commands_are_told_apart() {
    assert_eq!(shell_kind("git commit -m 'Add it'"), Some((Kind::Commit, "Add it".into())));
    assert_eq!(shell_kind("cd x && git -C y commit -q -am \"Two words\""), Some((Kind::Commit, "Two words".into())));
    assert_eq!(
        shell_kind("land feat/strip -m \"The strip. Check passed\""),
        Some((Kind::Commit, "land feat/strip The strip. Check passed".into()))
    );
    assert!(matches!(shell_kind("later add LBTerm \"x\" --why y"), Some((Kind::Mail, _))));
    assert!(matches!(shell_kind("bun C:/x/inmail.ts send Home \"hi\""), Some((Kind::Mail, _))));
    assert_eq!(shell_kind("bun inmail.ts list"), None);
    assert_eq!(shell_kind("git log --oneline | grep commit"), None);
    assert_eq!(shell_kind("echo land"), None);
}

#[test]
fn a_short_prompt_isnt_found_in_a_longer_one() {
    assert!(prompt_matches("yes", "yes"));
    assert!(!prompt_matches("yes, and tidy up", "yes"));
    // A long one shows its start, or is cut by the wrap or the header's ellipsis
    let said = "a long message about the scroll strip and its marks";
    let long: String = said.chars().take(prompts::KEY_CHARS).collect();
    assert!(prompt_matches(said, &long));
    assert!(prompt_matches("a long message about\u{2026}", &long));
    assert!(!prompt_matches("a long", &long), "too short to go by");
    assert!(matches_text("Answer 1", "answer1"));
    assert!(!matches_text("Answer 10", "answer1"));
}

#[test]
fn a_reply_asks_when_its_last_paragraph_does() {
    assert_eq!(asks("Done.\n\nWant me to push?"), Some("Want me to push?".into()));
    assert_eq!(asks("Is it?\n\nIt is."), None);
    assert_eq!(asks("```\nwhat?\n```\nok"), None);
    assert_eq!(asks("**Q1.** Which one\n- 1a. this\n\nThat's all."), Some("Q1. Which one".into()));
    assert_eq!(asks("Extras, yes or no each:\n- E4. more"), Some("Extras, yes or no each:".into()));
    assert_eq!(asks("> quoted?\n\nplain"), None);
}

#[test]
fn rows_follow_the_panes_width() {
    let mut b = Block::new(BlockKind::Text, 0);
    b.lines = vec![Line::Cut { width: 100, lead: 2 }, Line::words("", 2), Line::Cut { width: 30, lead: 2 }];
    // A blank line before it, then 100 columns over 48 (2 for "● "): 3 rows, an empty line, 1
    assert_eq!(b.rows(50), 1 + 3 + 1 + 1);
    assert_eq!(b.rows(200), 1 + 1 + 1 + 1);
    assert_eq!(Block::row(BlockKind::Tools, 0).rows(80), 2);
    assert_eq!(Block::new(BlockKind::TurnEnd, 0).rows(80), 2);
}

#[test]
fn words_wrap_as_ink_wraps_them() {
    let rows = |text: &str, room: usize| word_rows(&text.split(' ').map(width).collect::<Vec<_>>(), room);
    assert_eq!(rows("", 10), 1);
    assert_eq!(rows("one two three", 13), 1);
    // A word that doesn't fit goes down whole, where cutting by columns would split it
    assert_eq!(rows("aaaa bbbb cccc", 10), 2);
    assert_eq!(rows("aaaaaaaa bbbbbbbb", 10), 2);
    // One longer than a row is broken where the rows end, from the row it starts on
    assert_eq!(rows("ab cdefghijklmnopqrstuvwxyz", 10), 3);
    assert_eq!(rows(&"x".repeat(25), 10), 3);
}

#[test]
fn a_list_item_hangs_its_words_past_its_marker() {
    let drawn = rendered("Some words\n\n\n- an item that goes on\n12. a numbered one\n  - nested");
    assert_eq!(drawn, vec!["Some words", "", "- an item that goes on", "12. a numbered one", "  - nested"]);
    let lines = text_lines(&drawn);
    assert_eq!(lines[2], Line::words("an item that goes on", 4));
    assert_eq!(lines[3], Line::words("a numbered one", 6));
    assert_eq!(lines[4], Line::words("nested", 6));
    // A link shows its words, emphasis its words
    assert_eq!(rendered("See [e71c88f9](lbterm://session/e71c) and *this*, `code`"), vec!["See e71c88f9 and this, code"]);
    assert_eq!(rendered("* starred item"), vec!["* starred item"]);
}

#[test]
fn an_edit_draws_its_diff_numbered_and_cut_at_the_row() {
    let hunks = json!([
        {"oldStart": 98, "oldLines": 3, "newStart": 98, "newLines": 4, "lines": [" keep", "-gone", "+came", "+also"]},
        {"oldStart": 120, "oldLines": 1, "newStart": 121, "newLines": 1, "lines": [format!(" {}", "x".repeat(2500))]},
    ]);
    let (lines, keys) = diff_lines(hunks.as_array().unwrap(), None);
    // Three digits: 7 columns of margin, 6 and the number, a space and the mark
    let lead = DIFF_MARGIN + 6 + 3 + 2;
    assert_eq!(lines[0], Line::Cut { width: 4, lead });
    assert_eq!(keys[0], "98keep");
    assert_eq!(keys[1], "99gone");
    assert_eq!(keys[2], "99came");
    // "..." between the hunks
    assert_eq!(lines[4], Line::Row);
    // A line past 2000 characters is cut there
    assert_eq!(lines[5], Line::Cut { width: 2000 + " \u{2026} [+500 chars]".chars().count() as u16, lead });
    // Under a shell command, no more than so many lines and a row saying how many more
    let (lines, _) = diff_lines(hunks.as_array().unwrap(), Some(2));
    assert_eq!(lines, vec![Line::Cut { width: 4, lead }, Line::Cut { width: 4, lead }, Line::Row]);
}

#[test]
fn a_write_shows_the_files_first_ten_rows() {
    let input = json!({"file_path": "C:\\Users\\demo\\work\\notes.md", "content": "x"});
    let result = json!({"type": "create", "content": "one line\ntwo lines\nthree lines\n"});
    let (lines, keys) = tool_lines("Write", &input, Some(&result));
    assert_eq!(lines[1], Line::words("Wrote 3 lines to ~\\work\\notes.md", 5));
    assert_eq!(keys[0], "writeworknotesmd");
    let lead = DIFF_MARGIN + 9;
    assert_eq!(lines[2..], [Line::Cap { most: PREVIEW_ROWS }, Line::Capped { width: 8, lead }, Line::Capped { width: 9, lead }, Line::Capped { width: 11, lead }]);
    // Each line found by its number and words, as the row shows them
    assert_eq!(keys[3], "1oneline");
    assert_eq!(keys[5], "3threelines");
    let mut b = Block::new(BlockKind::Tool, 0);
    b.lines = lines;
    assert_eq!(b.rows(80), 1 + 2 + 3);
    // Forty lines: ten rows and "… +30 lines", the lines past them none
    let long = json!({"type": "create", "content": "x\n".repeat(40)});
    let (lines, _) = tool_lines("Write", &input, Some(&long));
    b.lines = lines;
    assert_eq!(b.rows(80), 1 + 2 + 10 + 1);
    assert_eq!(b.row_of_line(3 + 9, 80), 1 + 2 + 9, "the tenth line on the tenth row");
    // A line that wraps takes its rows from the ten
    let wide = json!({"type": "create", "content": format!("{}\nshort\n", "y".repeat(200))});
    let (lines, _) = tool_lines("Write", &input, Some(&wide));
    b.lines = lines;
    assert_eq!(b.rows(80), 1 + 2 + 4 + 1);
}

#[test]
fn an_edit_in_the_sessions_scratchpad_folds() {
    let mine = scratchpad_of(std::path::Path::new("C:\\Users\\demo\\.claude\\projects\\C--work\\abc-123.jsonl"));
    assert_eq!(mine, "\\claude\\c--work\\abc-123\\scratchpad\\");
    let scratch = json!({"file_path": "C:\\Users\\demo\\AppData\\Local\\Temp\\claude\\C--work\\abc-123\\scratchpad\\x.ts"});
    // Another session's, or the same session's run from another folder, shows
    let other = json!({"file_path": "C:/Users/demo/AppData/Local/Temp/claude/C--elsewhere/abc-123/scratchpad/x.ts"});
    let elsewhere = json!({"file_path": "C:\\work\\x.ts"});
    assert!(!shows_own("Edit", &scratch, &mine));
    assert!(!shows_own("Write", &scratch, &mine));
    assert!(shows_own("Edit", &other, &mine));
    assert!(shows_own("Edit", &elsewhere, &mine));
    assert!(shows_own("Edit", &scratch, ""), "with no transcript to say, it shows");
    assert!(!shows_own("Bash", &elsewhere, &mine));
}

#[test]
fn the_header_names_a_prompt_of_several_lines_by_all_of_them() {
    assert!(header_names("1b 2. none, I think it's fine", "1b", "1b 2. none, I think it's fine"));
    assert!(header_names("1b 2. none, I think it's f\u{2026}", "1b", "1b 2. none, I think it's fine"));
    assert!(!header_names("1b 2. no", "1b", "1b 2. none, I think it's fine"));
    assert!(!header_names("1c 2. none", "1b", "1b 2. none, I think it's fine"));
}

#[test]
fn the_layout_adds_up_and_knows_where_the_compaction_is() {
    let s = read().snapshot();
    let layout = s.layout(80);
    assert_eq!(layout.tops.len(), s.blocks.len());
    assert_eq!(layout.tops[0], BANNER_ROWS, "Claude Code's banner above the first message");
    let sum: usize = BANNER_ROWS + s.blocks.iter().map(|b| b.rows(80)).sum::<usize>();
    assert_eq!(layout.total, sum);
    assert_eq!(layout.reachable, layout.tops[13]);
    assert_eq!(layout.block_at(0), None, "the banner is no block's");
    assert_eq!(layout.block_at(BANNER_ROWS), Some(0));
    assert_eq!(layout.block_at(layout.tops[3] + 1), Some(3));
    assert_eq!(layout.block_at(layout.total + 5), Some(s.blocks.len() - 1));
}

fn row(text: &str, band: bool) -> Row {
    Row { text: text.into(), band, dim: false }
}

/// Claude Code's sticky header naming `prompt`: on the band, its words dim
fn header(prompt: &str) -> Row {
    Row { text: format!("\u{276f} {prompt}"), band: true, dim: true }
}

/// A screen of Claude Code's fullscreen view as 2.1.312 draws it: `area` rows of the
/// conversation, blank but for `at`, "Jump to bottom" on the last of them when `scrolled`, then
/// its effort line, the input box between its rules and a three-line status with the mode under
fn screen(area: usize, at: &[(usize, Row)], scrolled: bool) -> Vec<Row> {
    let mut rows = vec![row("", false); area];
    for (i, r) in at {
        rows[*i] = r.clone();
    }
    if scrolled {
        rows[area - 1] = row("                                     Jump to bottom (ctrl+End) \u{2193}", false);
    }
    let rule = "\u{2500}".repeat(80);
    for (t, band) in [("", false), (rule.as_str(), false), ("\u{276f} ", false), (rule.as_str(), false)] {
        rows.push(row(t, band));
    }
    for t in ["  \u{25b8} a summary", "  opus | folder | main", "  toggles", "  \u{23f5}\u{23f5} accept edits on"] {
        rows.push(row(t, false));
    }
    rows
}

#[test]
fn the_screen_says_where_the_conversation_ends_and_whether_its_scrolled() {
    let s = Screen::read(&screen(30, &[], false)).unwrap();
    assert_eq!((s.area, s.scrolled, s.header), (30, false, false));
    assert_eq!(s.half(), 15);
    // Scrolled up: "Jump to bottom" on the area's last row, and the header on its first
    let s = Screen::read(&screen(30, &[(0, header("Fix the build"))], true)).unwrap();
    assert_eq!((s.area, s.scrolled, s.header), (30, true, true));
    // Before 2.1.312 the header was off the band
    let s = Screen::read(&screen(30, &[(0, row("\u{276f} Fix the build", false))], true)).unwrap();
    assert!(s.header);
    // A message you sent at the top, in white on the band, isn't the header
    let s = Screen::read(&screen(30, &[(0, row("\u{276f} Fix the build", true))], true)).unwrap();
    assert!(!s.header);
    // A rule in the conversation (a markdown line) isn't the input box's
    let mut with_rule = screen(30, &[], false);
    with_rule[5] = row(&"\u{2500}".repeat(80), false);
    assert_eq!(Screen::read(&with_rule).unwrap().area, 30);
    // No input box (a dialog in its place, or a shell): nothing to say
    assert_eq!(Screen::read(&vec![row("PS C:\\> ", false); 40]), None);
    // A named conversation (Claude Code 2.1.314 on) has its name in the upper rule
    let mut named = screen(30, &[(0, header("Fix the build"))], true);
    named[31] = row(&format!("{} LBTerm InMail \u{2500}", "\u{2500}".repeat(64)), false);
    assert_eq!(Screen::rules(&named), Some((31, 33)));
    let s = Screen::read(&named).unwrap();
    assert_eq!((s.area, s.scrolled, s.header), (30, true, true));
}

#[test]
fn at_the_latest_the_view_is_the_end() {
    let s = read().snapshot();
    let layout = s.layout(80);
    // Whatever's on screen: not scrolled up is the latest
    let rows = screen(24, &[(10, row("\u{276f} Yes, and tidy up", true))], false);
    let view = s.locate(&layout, &rows, None).unwrap();
    assert_eq!(view, View { top: layout.total - 24, rows: 24 });
}

#[test]
fn scrolled_up_the_view_is_found_from_a_message_on_the_band() {
    let s = read().snapshot();
    let layout = s.layout(80);
    let rows = screen(24, &[(10, row("\u{276f} Yes, and tidy up", true))], true);
    let view = s.locate(&layout, &rows, None).unwrap();
    assert_eq!(view.rows, 24, "the input box isn't the conversation's");
    // The band is the block's second row: its first is the blank line before it
    assert_eq!(view.top, (layout.tops[11] + 1).saturating_sub(10).min(layout.total - 24));
}

#[test]
fn the_header_is_never_taken_for_the_message_it_names() {
    let s = read().snapshot();
    let layout = s.layout(80);
    // The header names the first prompt; Claude's words under it place the view
    let rows = screen(
        24,
        &[(0, header("Fix the build of the demo app")), (3, row("\u{25cf} Looking at the build script first.", false))],
        true,
    );
    let view = s.locate(&layout, &rows, None).unwrap();
    assert_eq!(view.top, (layout.tops[2] + 1).saturating_sub(3).min(layout.total - 24));
}

#[test]
fn scrolled_up_the_view_is_found_from_claudes_words() {
    let s = read().snapshot();
    let layout = s.layout(40);
    let rows = screen(12, &[(1, row("\u{25cf} Looking at the build script first.", false))], true);
    let view = s.locate(&layout, &rows, None).unwrap();
    assert_eq!(view.top, (layout.tops[2] + 1 - 1).min(layout.total - 12));
}

#[test]
fn the_same_words_twice_are_told_apart_by_where_the_view_was() {
    let mut f = Follow::default();
    let mut lines = vec![];
    for i in 0..6 {
        lines.push(prompt("yes", &format!("03:00:0{i}.000")));
        lines.push(says(&"word ".repeat(80), &format!("03:00:0{i}.500")));
    }
    f.feed((lines.join("\n") + "\n").as_bytes());
    let s = f.conversation.snapshot();
    let layout = s.layout(80);
    let rows = screen(14, &[(0, row("\u{276f} yes", true))], true);
    // With nothing to go by, the latest; near a view, the one nearest it
    let latest = s.locate(&layout, &rows, None).unwrap();
    assert_eq!(latest.top, (layout.tops[10] + 1).min(layout.total - 14));
    let near = s.locate(&layout, &rows, Some(layout.tops[4])).unwrap();
    assert_eq!(near.top, layout.tops[4] + 1);
}

#[test]
fn the_header_alone_keeps_the_view_inside_its_prompts_part() {
    let s = read().snapshot();
    let layout = s.layout(80);
    let rows = screen(10, &[(0, header("Fix the build of the demo app"))], true);
    let from = layout.tops[1] + 1;
    let view = s.locate(&layout, &rows, None).unwrap();
    assert_eq!(view.top, from);
    let view = s.locate(&layout, &rows, Some(from + 3)).unwrap();
    assert_eq!(view.top, from + 3);
    // A view from elsewhere is pulled into the part the header names
    let view = s.locate(&layout, &rows, Some(layout.total)).unwrap();
    assert!(view.top < layout.tops[11], "{view:?}");
    // Scrolled up with nothing on screen to go by
    assert_eq!(s.locate(&layout, &screen(10, &[], true), None), None);
}

/// Forty messages, each with ten lines of answer
fn forty() -> Snapshot {
    let mut f = Follow::default();
    let mut lines = vec![];
    for i in 0..40 {
        lines.push(prompt(&format!("message number {i}"), &format!("04:00:{:02}.000", i)));
        lines.push(says(&format!("answer {i}\n").repeat(10), &format!("04:00:{:02}.500", i)));
    }
    f.feed((lines.join("\n") + "\n").as_bytes());
    f.conversation.snapshot()
}

#[test]
fn a_screen_halfway_up_without_jump_to_bottom_isnt_taken_for_the_latest() {
    let s = forty();
    let layout = s.layout(80);
    // Claude Code left "Jump to bottom" off: the rows say where it is
    let rows = screen(
        20,
        &[
            (2, row("\u{276f} message number 10", true)),
            (4, row("\u{25cf} answer 10", false)),
            (5, row("  answer 10", false)),
            (6, row("  answer 10", false)),
        ],
        false,
    );
    assert_eq!(s.locate(&layout, &rows, None).unwrap().top, layout.tops[20] + 1 - 2);
    // The last screen's rows: the latest
    let rows = screen(20, &[(4, row("\u{276f} message number 39", true)), (6, row("\u{25cf} answer 39", false))], false);
    assert_eq!(s.locate(&layout, &rows, None).unwrap().top, layout.total - 20);
}

#[test]
fn a_seek_sends_its_pages_in_one_burst_from_whichever_is_nearest() {
    let s = forty();
    let layout = s.layout(80);
    // At the latest, a message a few pages up: that many PgUps at once
    let at_end = screen(20, &[(4, row("\u{276f} message number 39", true))], false);
    let mut seek = Seek::new(&s, 70, 0).unwrap();
    let Step::Press(Key::PageUp, n) = seek.look(&s, &layout, &at_end, None) else { panic!() };
    let pages = (layout.total - 20 - layout.tops[70]) as f64 / 10.0;
    assert!((n as f64 - pages).abs() <= 2.0, "{n} PgUps for {pages:.1} pages");
    // At the latest, a message near the top: Ctrl+Home first
    let mut seek = Seek::new(&s, 2, 0).unwrap();
    assert_eq!(seek.look(&s, &layout, &at_end, None), Step::Press(Key::Top, 1));
    // Near the top, a message near the latest: Ctrl+End first
    let near_top = screen(20, &[(0, header("message number 1")), (3, row("\u{25cf} answer 1", false))], true);
    let mut seek = Seek::new(&s, 76, 0).unwrap();
    assert_eq!(seek.look(&s, &layout, &near_top, None), Step::Press(Key::Bottom, 1));
    // A few pages below: that many PgDns
    let mut seek = Seek::new(&s, 10, 0).unwrap();
    let step = seek.look(&s, &layout, &near_top, None);
    assert!(matches!(step, Step::Press(Key::PageDown, 2..=6)), "{step:?}");
}

#[test]
fn a_seek_brings_its_block_up_the_screen_once_its_there() {
    let s = forty();
    let layout = s.layout(80);
    let mut seek = Seek::new(&s, 10, 0).unwrap();
    // On screen in the upper half: there
    let high = screen(20, &[(0, header("message number 4")), (3, row("\u{276f} message number 5", true))], true);
    assert_eq!(seek.look(&s, &layout, &high, None), Step::Found(3));
    // Low on the screen: one PgDn more, then there
    let mut seek = Seek::new(&s, 10, 0).unwrap();
    let low = screen(20, &[(0, header("message number 4")), (15, row("\u{276f} message number 5", true))], true);
    assert_eq!(seek.look(&s, &layout, &low, None), Step::Press(Key::PageDown, 1));
    let raised = screen(20, &[(0, header("message number 4")), (5, row("\u{276f} message number 5", true))], true);
    assert_eq!(seek.look(&s, &layout, &raised, None), Step::Found(5));
    // At the latest it can't go further: there where it is
    let mut seek = Seek::new(&s, 78, 0).unwrap();
    let end = screen(20, &[(15, row("\u{276f} message number 39", true))], false);
    assert_eq!(seek.look(&s, &layout, &end, None), Step::Found(15));
}

#[test]
fn a_seek_gives_up_at_an_end_that_doesnt_move() {
    let s = read().snapshot();
    let layout = s.layout(80);
    // Claude's words before the compaction, the view at the latest: up, then up again goes
    // nowhere, so it isn't there
    let mut seek = Seek::new(&s, 2, 0).unwrap();
    let end = screen(10, &[(2, row("\u{276f} Yes, and tidy up", true))], false);
    assert!(matches!(seek.look(&s, &layout, &end, None), Step::Press(Key::PageUp | Key::Top, _)));
    let top = screen(10, &[(2, row("\u{276f} Yes, and tidy up", true))], true);
    assert!(matches!(seek.look(&s, &layout, &top, None), Step::Press(_, _)));
    assert_eq!(seek.look(&s, &layout, &top, None), Step::NotThere);
    // No input box on screen: nothing to scroll
    let mut seek = Seek::new(&s, 2, 0).unwrap();
    assert_eq!(seek.look(&s, &layout, &vec![row("", false); 20], None), Step::NotThere);
}

#[test]
fn tool_calls_are_found_by_the_words_above_them() {
    let s = read().snapshot();
    assert_eq!(s.anchor_for(3), Some(2));
    assert_eq!(s.anchor_for(2), Some(2));
    assert_eq!(s.anchor_for(5), Some(4), "a run after an edit is found by the edit's header");
    assert_eq!(s.anchor_for(10), Some(8), "a note is found by Claude's words before it");
    assert_eq!(Snapshot::default().anchor_for(0), None);
    assert!(Seek::new(&s, 3, 0).is_none(), "a run of tool calls has no words of its own");
}

#[test]
fn a_point_on_the_track_is_found_by_the_words_at_or_above_it_and_never_above_the_compaction() {
    let s = read().snapshot();
    let layout = s.layout(80);
    assert!(s.before_compaction(2));
    assert!(!s.before_compaction(14));
    // Rows above the compaction go to the first words after it: the picker's header
    assert_eq!(s.anchor_at(&layout, 0), Some(14));
    let mut f = Follow::default();
    f.feed(transcript().as_bytes());
    f.feed((says("After the compaction", "01:00:25.000") + "\n").as_bytes());
    let s = f.conversation.snapshot();
    let layout = s.layout(80);
    assert_eq!(s.anchor_at(&layout, 0), Some(14));
    assert_eq!(s.anchor_at(&layout, layout.total - 1), Some(15));
    // Without one, the words at or above
    let s = read_without_compaction();
    let layout = s.layout(80);
    assert_eq!(s.anchor_at(&layout, layout.tops[3] + 1), Some(2));
}

fn read_without_compaction() -> Snapshot {
    let text: String = transcript().lines().filter(|l| !l.contains("compact_boundary")).map(|l| format!("{l}\n")).collect();
    let mut f = Follow::default();
    f.feed(text.as_bytes());
    f.conversation.snapshot()
}

#[test]
fn the_card_lists_each_mark_level_with_them_and_inside_the_window() {
    let items = vec![
        CardItem { colour: 0xFF8A80, head: "You \u{b7} 14:02".into(), text: "one two three four five six seven eight nine ten".into(), lines: 2 },
        CardItem { colour: 0x3DDC84, head: "Commit \u{b7} 14:05".into(), text: "Fix it".into(), lines: 1 },
    ];
    // 140 px, less 10 px of padding each side: 17 columns of 7 px
    let c = card(&items, 500.0, (300.0, 40.0, 900.0), 140.0, (7.0, 16.0), 1.0).unwrap();
    assert_eq!(c.rect.x, 500.0 - 140.0);
    let texts: Vec<&str> = c.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts[0], "You \u{b7} 14:02");
    assert_eq!(texts[1], "one two three");
    assert!(texts[2].ends_with('\u{2026}'), "cut to its two lines: {texts:?}");
    assert_eq!(&texts[3..], ["Commit \u{b7} 14:05", "Fix it"]);
    assert_eq!(c.lines[3].tone, CardTone::Head(0x3DDC84));
    // Its first line level with the marks
    assert_eq!(c.lines[0].y, (300.0f64 - 10.0 - 8.0).floor() + 10.0);
    assert_eq!(c.items.len(), 2);
    assert!(c.items[0].y + c.items[0].h <= c.items[1].y + 0.01);
    // Kept above the window's bottom and below its top
    let low = card(&items, 500.0, (890.0, 40.0, 900.0), 181.0, (7.0, 16.0), 1.0).unwrap();
    assert!(low.rect.y + low.rect.h <= 900.0);
    let high = card(&items, 500.0, (0.0, 40.0, 900.0), 181.0, (7.0, 16.0), 1.0).unwrap();
    assert_eq!(high.rect.y, 40.0);
    assert!(card(&[], 500.0, (0.0, 40.0, 900.0), 181.0, (7.0, 16.0), 1.0).is_none());
}

#[test]
fn the_thumb_stands_for_the_view() {
    let layout = Layout { tops: vec![0], total: 1000, reachable: 0, cols: 80 };
    let (y, h) = thumb(&layout, View { top: 0, rows: 100 }, 500.0, 20.0);
    assert_eq!((y, h), (0.0, 50.0));
    let (y, h) = thumb(&layout, View { top: 900, rows: 100 }, 500.0, 20.0);
    assert_eq!((y, h), (450.0, 50.0));
    // Never shorter than the least
    let (_, h) = thumb(&Layout { total: 100_000, ..layout.clone() }, View { top: 0, rows: 100 }, 500.0, 20.0);
    assert_eq!(h, 20.0);
    // And a point on the track is the row whose view puts the thumb's top there
    assert_eq!(row_at(&layout, 100, 225.0, 500.0, 50.0), 450);
    assert_eq!(row_at(&layout, 100, -5.0, 500.0, 50.0), 0);
    assert_eq!(row_at(&layout, 100, 9999.0, 500.0, 50.0), 900);
}

#[test]
fn a_line_cut_short_waits_for_the_rest() {
    let text = transcript();
    let (a, b) = text.as_bytes().split_at(text.len() / 2);
    let mut f = Follow::default();
    f.feed(a);
    f.feed(b);
    assert_eq!(f.conversation.snapshot(), read().snapshot());
}

#[test]
fn a_transcript_is_found_in_its_projects_folder_and_followed_as_it_grows() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let sid = "5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35";
    std::fs::create_dir_all(dir.path().join("C--work-other"))?;
    std::fs::create_dir_all(dir.path().join("C--work-demo"))?;
    let path = dir.path().join("C--work-demo").join(format!("{sid}.jsonl"));
    assert_eq!(find_transcript(dir.path(), sid), None);
    std::fs::write(&path, prompt("first", "05:00:00.000") + "\n")?;
    assert_eq!(find_transcript(dir.path(), sid), Some(path.clone()));
    let mut f = Follow::default();
    assert!(f.read_more(&path));
    assert!(!f.read_more(&path), "nothing new");
    std::fs::write(&path, prompt("first", "05:00:00.000") + "\n" + &prompt("second", "05:00:01.000") + "\n")?;
    assert!(f.read_more(&path));
    assert_eq!(f.conversation.blocks.len(), 2);
    // Shorter: read again from the start
    std::fs::write(&path, prompt("again", "05:00:02.000") + "\n")?;
    assert!(f.read_more(&path));
    assert_eq!(f.conversation.blocks.len(), 1);
    Ok(())
}

#[test]
fn every_kind_has_a_name_a_label_a_colour_and_a_place() {
    let mut names: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
    names.dedup();
    assert_eq!(names.len(), 12);
    let mut ranks: Vec<u8> = Kind::ALL.iter().map(|k| k.rank()).collect();
    ranks.sort();
    ranks.dedup();
    assert_eq!(ranks.len(), 12, "no two kinds drawn at the same rank");
    assert_eq!(Kind::Prompt.shape(), Shape::Bar);
    assert_eq!(Kind::Compaction.shape(), Shape::Line);
    assert_eq!(duration(156_000), "2m 36s");
    assert_eq!(duration(3_780_000), "1h 3m");
    assert_eq!(cut("abcdef", 4), "abc\u{2026}");
}

/// Waits for `done`, up to five seconds
fn until(done: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    done()
}

/// A store over a projects folder of its own, with the sessions each wake was for
fn store(projects: &std::path::Path) -> (Conversations, Arc<std::sync::Mutex<Vec<String>>>) {
    let woken: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let w = Arc::clone(&woken);
    let store = Conversations::start(projects.to_path_buf(), None, move |sid: &str, _| w.lock().unwrap().push(sid.to_string()));
    (store, woken)
}

fn append(path: &std::path::Path, line: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
    writeln!(f, "{line}").unwrap();
}

#[test]
fn a_conversation_shown_is_read_as_it_grows_and_only_its_windows_are_told() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (a, b) = ("5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35", "0091c67b-1db2-470d-9e29-9bb1ffbda293");
    let folder = dir.path().join("C--work-demo");
    std::fs::create_dir_all(&folder)?;
    let (pa, pb) = (folder.join(format!("{a}.jsonl")), folder.join(format!("{b}.jsonl")));
    append(&pa, &prompt("first of a", "05:00:00.000"));
    append(&pb, &prompt("first of b", "05:00:00.000"));
    let (store, woken) = store(dir.path());
    // Nothing is read until it's shown
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(store.get(a).is_none() && woken.lock().unwrap().is_empty());
    store.show(vec![a.into()], vec![a.into(), b.into()]);
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 1)));
    assert!(until(|| *woken.lock().unwrap() == vec![a.to_string()]));
    // Its transcript growing is read at once, with nothing asking
    append(&pa, &says("an answer", "05:00:01.000"));
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 2)));
    // Another session's growing isn't read, and wakes nothing
    append(&pb, &says("b's answer", "05:00:01.000"));
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(store.get(b).is_none());
    assert!(woken.lock().unwrap().iter().all(|s| s == a));
    // Something written that shows nothing (a record the strip leaves alone): no new snapshot
    let before = store.get(a).unwrap();
    let wakes = woken.lock().unwrap().len();
    append(&pa, r#"{"type":"progress","data":{"type":"hook_progress"}}"#);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(Arc::ptr_eq(&before, &store.get(a).unwrap()));
    assert_eq!(woken.lock().unwrap().len(), wakes);
    Ok(())
}

#[test]
fn a_conversation_shown_again_is_read_on_from_where_it_stopped_while_a_pane_names_it() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let (a, b) = ("5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35", "0091c67b-1db2-470d-9e29-9bb1ffbda293");
    let folder = dir.path().join("C--work-demo");
    std::fs::create_dir_all(&folder)?;
    let (pa, pb) = (folder.join(format!("{a}.jsonl")), folder.join(format!("{b}.jsonl")));
    let first = prompt("first of a", "05:00:00.000");
    append(&pa, &first);
    append(&pb, &prompt("first of b", "05:00:00.000"));
    let (store, _) = store(dir.path());
    store.show(vec![a.into()], vec![a.into(), b.into()]);
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 1)));
    // Another tab's shown: a's pane is still there, so what was read of it is kept
    store.show(vec![b.into()], vec![a.into(), b.into()]);
    assert!(until(|| store.get(b).is_some()));
    append(&pa, &says("written while b was shown", "05:00:01.000"));
    // Its start rewritten to words of the same length that read as nothing: read from its start
    // again, the first prompt would be gone, read on from where it stopped, it's still there
    let mut text = std::fs::read_to_string(&pa)?;
    text.replace_range(..first.len(), &" ".repeat(first.len()));
    std::fs::write(&pa, text)?;
    store.show(vec![a.into()], vec![a.into(), b.into()]);
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 2)), "read on from where it stopped");
    assert_eq!(store.get(a).unwrap().blocks[0].text, "first of a");
    // No pane names it any more: let go, and shown again it's read from its start
    store.show(vec![b.into()], vec![b.into()]);
    assert!(until(|| store.get(a).is_none()));
    store.show(vec![a.into()], vec![a.into(), b.into()]);
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 1)));
    assert_eq!(store.get(a).unwrap().blocks[0].kind, BlockKind::Text);
    Ok(())
}

#[test]
fn a_transcript_not_written_yet_is_read_once_it_is() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = "5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35";
    std::fs::create_dir_all(dir.path().join("C--work-other"))?;
    let (store, woken) = store(dir.path());
    store.show(vec![a.into()], vec![a.into()]);
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(store.get(a).is_none());
    // Its project's folder made, and the transcript in it
    let folder = dir.path().join("C--work-demo");
    std::fs::create_dir_all(&folder)?;
    append(&folder.join(format!("{a}.jsonl")), &prompt("the first prompt", "05:00:00.000"));
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 1)));
    // Its windows told, and no others: once, or twice when the thread found the file before its
    // first line was in it (a snapshot of nothing yet, then the line's), as a loaded PC has it
    assert!(until(|| {
        let woken = woken.lock().unwrap();
        !woken.is_empty() && woken.len() <= 2 && woken.iter().all(|s| s == a)
    }));
    // And followed from there
    append(&folder.join(format!("{a}.jsonl")), &says("an answer", "05:00:01.000"));
    assert!(until(|| store.get(a).is_some_and(|s| s.blocks.len() == 2)));
    Ok(())
}

#[test]
fn a_transcript_written_as_its_watch_is_made_is_read() -> anyhow::Result<()> {
    // The reading thread looks for a transcript not found yet, then watches the projects for one
    // of its name. Written in between, its folder and all, it says nothing to that watch: on a
    // loaded PC the thread can come to the look late, and the test above, which writes it 150 ms
    // after it's shown, waited 5 s for it in vain (2026-11-01; once in 280 runs under load on the
    // code before the fix, lbterm/PITFALLS.md, trap 60). Here it's written in that gap every time
    let dir = tempfile::tempdir()?;
    let a = "5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35";
    let folder = dir.path().join("C--work-demo");
    let path = folder.join(format!("{a}.jsonl"));
    let written = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let w = Arc::clone(&written);
    let store = Conversations::start_probed(
        dir.path().to_path_buf(),
        |_: &str, _| {},
        move || {
            if !w.swap(true, std::sync::atomic::Ordering::AcqRel) {
                std::fs::create_dir_all(&folder).unwrap();
                append(&path, &prompt("the first prompt", "05:00:00.000"));
            }
        },
    );
    store.show(vec![a.into()], vec![a.into()]);
    assert!(until(|| written.load(std::sync::atomic::Ordering::Acquire)), "written as the watch was made");
    assert!(
        until(|| store.get(a).is_some_and(|s| s.blocks.len() == 1)),
        "the transcript written between the look and the watch, read"
    );
    Ok(())
}

#[test]
fn a_transcript_written_as_a_second_session_joins_a_watch_that_holds_is_read() -> anyhow::Result<()> {
    // A first session not written yet has the projects folder watched with all its folders. A
    // second shown then changes only what the watch is for, not the folders it watches, so
    // Watch::follow says nothing changed. The second's transcript written after the reading
    // thread looked for it and before follow gave the watch its name was dropped by the watch's
    // filter, which held only the first's, and until 2026-11-02 nothing looked again: the strip
    // waited for Claude Code's next write (lbterm/PITFALLS.md, trap 63). Here it's written in that
    // gap every time, into a project's folder already there, and the hook waits long enough for
    // the watch's thread to have heard of it before follow goes on
    let dir = tempfile::tempdir()?;
    let a = "5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35";
    let b = "7a2c9e41-3f6d-4b18-9c05-d8e1b4a63f92";
    let folder = dir.path().join("C--work-demo");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join(format!("{b}.jsonl"));
    let follows = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let written = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (f, on, w) = (Arc::clone(&follows), Arc::clone(&armed), Arc::clone(&written));
    let store = Conversations::start_probed(
        dir.path().to_path_buf(),
        |_: &str, _| {},
        move || {
            f.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            if on.load(std::sync::atomic::Ordering::Acquire) && !w.swap(true, std::sync::atomic::Ordering::AcqRel) {
                append(&path, &prompt("the second's first prompt", "05:00:00.000"));
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        },
    );
    let live = vec![a.to_string(), b.to_string()];
    store.show(vec![a.into()], live.clone());
    assert!(until(|| follows.load(std::sync::atomic::Ordering::Acquire) >= 1), "the first's watch made");
    // Its follow done and the watch holding the projects folder
    std::thread::sleep(std::time::Duration::from_millis(300));
    armed.store(true, std::sync::atomic::Ordering::Release);
    store.show(live.clone(), live);
    assert!(until(|| written.load(std::sync::atomic::Ordering::Acquire)), "written as the second joined the watch");
    assert!(
        until(|| store.get(b).is_some_and(|s| s.blocks.len() == 1)),
        "the second's transcript, written between the look and the watch taking its name, read"
    );
    assert!(store.get(a).is_none(), "the first still not written");
    Ok(())
}

#[test]
fn a_transcript_written_shorter_is_read_again_with_a_revision_on_from_the_last() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("5d1e7f20-8c3b-4a96-b2d4-0e9f6a1c7b35.jsonl");
    for i in 0..3 {
        append(&path, &prompt(&format!("prompt number {i}"), "05:00:00.000"));
    }
    let mut f = Follow::default();
    assert!(f.read_more(&path));
    let s = f.conversation.snapshot();
    assert_eq!(s.blocks.len(), 3);
    // Nothing new: nothing read, the same revision
    assert!(!f.read_more(&path));
    assert_eq!(f.conversation.revision(), s.revision);
    // Written again shorter (a session started over in the same file): read from its start, and
    // its revision past the last given out, so the new snapshot can't be taken for the old
    std::fs::write(&path, prompt("one prompt", "05:00:00.000") + "\n")?;
    assert!(f.read_more(&path));
    let again = f.conversation.snapshot();
    assert_eq!(again.blocks.len(), 1);
    assert!(again.revision > s.revision, "{} after {}", again.revision, s.revision);
    // A transcript that's gone reads as nothing new
    std::fs::remove_file(&path)?;
    assert!(!f.read_more(&path));
    Ok(())
}

/// An `attachment` record, as Claude Code writes one, with the folder the session was in
fn attached(attachment: Value, cwd: &str, at: &str) -> String {
    line(json!({"type": "attachment", "cwd": cwd, "attachment": attachment}), at)
}

#[test]
fn the_context_is_every_file_loaded_since_the_last_compaction_from_the_transcripts_own_records() {
    // Paths with forward slashes: the context compares folders either way
    let own = "C:/Users/me/Work/Harbor";
    let files = json!({"type": "instructions", "files": [
        {"path": "C:/Users/me/.claude/CLAUDE.md", "type": "User", "content": "u".repeat(400)},
        {"path": "C:/Users/me/Work/Harbor/CLAUDE.md", "type": "Project", "content": "p".repeat(800)},
        {"path": "C:/Users/me/.claude/projects/C--Users-me-Work-Harbor/memory/MEMORY.md", "type": "AutoMem", "content": "m".repeat(40)},
    ]});
    let pulled = |path: &str| json!({"type": "nested_memory", "path": path, "displayPath": "CLAUDE.md",
        "content": {"path": path, "type": "Project", "content": "n".repeat(4000)}});
    let mut f = Follow::default();
    let mut feed = |lines: &[String]| {
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        f.feed(text.as_bytes());
        f.conversation.snapshot()
    };
    let start = feed(&[
        line(json!({"type": "user", "cwd": own, "message": {"role": "user", "content": "go"}}), "01:00:00"),
        attached(files.clone(), own, "01:00:01"),
    ]);
    let kinds: Vec<(crate::context::Kind, usize)> = start.context.files.iter().map(|f| (f.kind, f.tokens)).collect();
    use crate::context::Kind as K;
    assert_eq!(kinds, vec![(K::User, 100), (K::Project, 200), (K::Memory, 10)]);
    assert_eq!(start.context.own.as_deref(), Some(own));

    // A subagent's records are its own context, not this conversation's
    let side = feed(&[line(
        json!({"type": "attachment", "isSidechain": true, "cwd": own, "attachment": pulled("C:/Users/me/Work/Side/CLAUDE.md")}),
        "01:01:00",
    )]);
    assert_eq!(side.context.files.len(), 3);
    assert!(Arc::ptr_eq(&start.context, &side.context), "unchanged, so shared");

    // Another project's CLAUDE.md, pulled in by a read there, after a cd: its own folder stays
    let later = feed(&[attached(pulled("C:/Users/me/Work/Ledger/CLAUDE.md"), "C:/Users/me/Work/Ledger", "01:02:00")]);
    let last = later.context.files.last().unwrap();
    assert_eq!((last.kind, last.tokens, last.elsewhere), (K::Later, 1000, true));
    assert!(later.revision > side.revision, "a new snapshot is due");

    // Compacted: the pulled-in file goes, and the start's come back with the record after it
    let compacted = feed(&[
        line(json!({"type": "system", "subtype": "compact_boundary", "cwd": own}), "01:03:00"),
        attached(files, own, "01:03:01"),
    ]);
    assert_eq!(compacted.context.files.len(), 3);
    assert!(compacted.context.files.iter().all(|f| f.kind != K::Later));
}

#[test]
fn quoted_context_records_in_a_message_are_not_read_as_records() {
    let mut f = Follow::default();
    let quoted = r#"{"type":"instructions","files":[{"path":"C:/x/CLAUDE.md","type":"User","content":"x"}]}"#;
    f.feed(format!("{}\n", prompt(quoted, "01:00:00")).as_bytes());
    assert!(f.conversation.snapshot().context.files.is_empty());
}

/// The made-up time `at` on 2026-10-28, in Unix milliseconds
fn ms(at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&format!("2026-10-28T{at}Z")).unwrap().timestamp_millis()
}

#[test]
fn each_message_to_claude_starts_a_turn_with_the_questions_his_prompt_names() {
    use crate::drafts::{Mark, Pick};
    let c = read();
    let turns: Vec<&Turn> = c.turns.iter().collect();
    // /clear, his first prompt, the task finishing (no prompt of his), his second; a meta record,
    // a tool's result and an interrupt start none
    assert_eq!(turns.len(), 4, "{turns:#?}");
    assert_eq!(turns.iter().map(|t| t.at_ms).collect::<Vec<_>>(), vec![ms("01:00:01"), ms("01:00:02"), ms("01:00:18"), ms("01:00:19")]);
    // A turn last wrote what a snapshot shows: the first prompt's through its turn's end, the
    // second's through the picker's answer; a subagent's words aren't its
    assert_eq!(turns[1].last_ms, ms("01:00:17"));
    assert_eq!(turns[3].last_ms, ms("01:00:23"));
    assert!(turns.iter().all(|t| t.picks.is_empty()));

    // An answer's picks, line by line, past his words before them
    let mut f = Follow::default();
    let lines = [
        says("Q1. Which way?\n\nE2. Tidy up too?", "02:00:00"),
        turn_end(1000, "02:00:01"),
        prompt("ok\n1a\n2- not now", "02:00:30"),
        says("Done.", "02:00:31"),
    ];
    f.feed(format!("{}\n", lines.join("\n")).as_bytes());
    let snap = f.conversation.snapshot();
    let turn = snap.turns.iter().last().unwrap();
    assert_eq!(
        turn.picks,
        vec![Pick { number: 1, options: "a".into(), mark: None }, Pick { number: 2, options: "".into(), mark: Some(Mark::No) }]
    );
    assert_eq!((turn.at_ms, turn.last_ms), (ms("02:00:30"), ms("02:00:31")));
    // A snapshot shares the turns' pieces until one changes
    let again = f.conversation.snapshot();
    assert!(again.turns.shares_piece(&snap.turns, 0));
}

/// A queued message, as Claude Code writes one it took while Claude was working: a
/// `queue-operation` record, then an `attachment` of type `queued_command`, and no `user` record
fn queued(text: &str, origin: &str, at: &str) -> String {
    line(
        json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": text, "commandMode": "prompt", "origin": {"kind": origin}}}),
        at,
    )
}

#[test]
fn a_prompt_he_typed_while_claude_worked_starts_a_turn_with_the_questions_it_names() {
    use crate::drafts::{Mark, Pick};
    let mut f = Follow::default();
    let lines = [
        prompt("Build it", "03:00:00"),
        says("Building.", "03:00:01"),
        line(json!({"type": "queue-operation", "operation": "enqueue", "content": "1+\n2a"}), "03:00:02"),
        queued("1+\n2a", "human", "03:00:03"),
        says("Taking 1 and 2a.", "03:00:04"),
        queued("From Website: the page is up", "peer", "03:00:05"),
        line(
            json!({"type": "attachment", "attachment": {"type": "queued_command", "prompt": "<task-notification><summary>Agent \"x\" finished</summary></task-notification>", "commandMode": "task-notification", "origin": {"kind": "task-notification"}}}),
            "03:00:06",
        ),
    ];
    f.feed(format!("{}\n", lines.join("\n")).as_bytes());
    let snap = f.conversation.snapshot();
    let turns: Vec<&Turn> = snap.turns.iter().collect();
    // His prompt; his queued answer, which wrote on to Claude's words after it; another
    // session's message and a task finishing, with no picks
    assert_eq!(turns.iter().map(|t| t.at_ms).collect::<Vec<_>>(), vec![ms("03:00:00"), ms("03:00:03"), ms("03:00:05"), ms("03:00:06")]);
    assert_eq!(turns[0].last_ms, ms("03:00:01"));
    assert_eq!(turns[1].last_ms, ms("03:00:04"));
    assert_eq!(
        turns[1].picks,
        vec![Pick { number: 1, options: "".into(), mark: Some(Mark::Yes) }, Pick { number: 2, options: "a".into(), mark: None }]
    );
    assert!(turns[2].picks.is_empty() && turns[3].picks.is_empty());
    // Whose: his two, typed and queued; not the other session's or the task's
    assert_eq!(turns.iter().map(|t| t.his).collect::<Vec<_>>(), vec![true, true, false, false]);
    // His queued prompt is a prompt's block with its mark on the strip, as a typed one is
    // (Lachlan's 7a of 2026-11-14): Claude Code draws it with the same component. The other
    // session's message has none, and the task its line
    let k: Vec<BlockKind> = snap.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(k.iter().filter(|&&b| b == BlockKind::Prompt).count(), 2, "{k:?}");
    assert_eq!(k.last(), Some(&BlockKind::Note), "{k:?}");
    let events = f.conversation.events();
    let prompts: Vec<&str> = events.iter().filter(|e| e.kind == Kind::Prompt).map(|e| e.text.as_str()).collect();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(prompts[1].starts_with("1+"), "{prompts:?}");
}

/// The context bar's numbers from a transcript's own records (lbterm/CONTEXT.md, "The context
/// bar"): what's in use from each main-chain reply, the scale from the first request, Claude
/// Code's parts from its requests and skills list, a memory file read, and what a subagent, a
/// pulled-in file, a compaction and a resume do to them
#[test]
fn the_context_bars_numbers_come_from_the_replies_and_the_recorded_requests() {
    use crate::context::{InUse, Kind as K, Scale};
    use crate::context_bar::{sections, Waiting, What};
    let own = "C:/Users/me/Work/Harbor";
    let memory = "C:/Users/me/.claude/projects/C--Users-me-Work-Harbor/memory";
    let files = json!({"type": "instructions", "files": [
        {"path": "C:/Users/me/.claude/CLAUDE.md", "type": "User", "content": "u".repeat(4000)},
        {"path": "C:/Users/me/Work/Harbor/CLAUDE.md", "type": "Project", "content": "p".repeat(2000)},
        {"path": format!("{memory}/MEMORY.md"), "type": "AutoMem", "content": "m".repeat(400)},
    ]});
    let usage = |n: u64| json!({"input_tokens": 2, "cache_read_input_tokens": n - 1002, "cache_creation_input_tokens": 1000, "output_tokens": 50});
    let reply = |n: u64, at: &str| {
        line(json!({"type": "assistant", "cwd": own, "message": {"role": "assistant", "model": "m", "usage": usage(n),
            "content": [{"type": "text", "text": "Done."}]}}), at)
    };
    let tools = json!([{"name": "Read", "description": "d".repeat(3990)}]);
    let shape = |hash: &str, system: usize| {
        json!({"type": "api-request-shape", "shapeHash": hash, "shape": {"system": [{"type": "text", "text": "s".repeat(system)}], "tools": tools.clone()}}).to_string()
    };
    let blob = |hash: &str, chars: usize| {
        json!({"type": "api-request-blob", "hash": hash, "message": {"role": "user", "content": [{"type": "text", "text": "b".repeat(chars)}]}}).to_string()
    };
    let request = |shape: &str, keep: u64, tail: &[&str]| {
        json!({"type": "api-request", "querySource": "repl_main_thread", "shapeHash": shape, "keep": keep, "tail": tail}).to_string()
    };
    let skills = json!({"type": "skill_listing", "isInitial": true, "content": "x"});
    let mut f = Follow::default();
    let mut feed = |lines: &[String]| {
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        f.feed(text.as_bytes());
        f.conversation.snapshot()
    };
    let before = feed(&[
        line(json!({"type": "user", "cwd": own, "message": {"role": "user", "content": "go"}}), "01:00:00"),
        attached(files.clone(), own, "01:00:01"),
        line(json!({"type": "attachment", "cwd": own, "attachment": skills, "rendered": [{"content": "k".repeat(800)}]}), "01:00:01"),
    ]);
    assert_eq!(before.context.in_use, InUse::Unknown);
    assert_eq!(sections(&before.context, Some(200_000)), Err(Waiting::FirstReply));

    // The first request, its blobs, and its reply: what's in use, and the scale
    let tools_est = crate::context::tokens(&tools.to_string());
    let first = feed(&[shape("s1", 4000), blob("m1", 8000), blob("m2", 2000), request("s1", 0, &["m1", "m2"]), reply(30_000, "01:00:05")]);
    let estimated = (1000 + tools_est + 2500) as u64;
    assert_eq!(first.context.in_use, InUse::Known(30_000));
    assert_eq!(first.context.replies, 1, "a reply to a new request counts");
    assert_eq!(first.context.scale, Some(Scale { real: 30_000, estimated }));
    let parts = first.context.parts.unwrap();
    assert_eq!((parts.system, parts.tools, parts.skills), (1000, tools_est, 200));
    assert_eq!(sections(&first.context, None), Err(Waiting::Window), "a window size not yet known");

    // A subagent's reply is its own context's size, not this conversation's
    let side = feed(&[line(
        json!({"type": "assistant", "isSidechain": true, "message": {"role": "assistant", "usage": usage(500_000), "content": []}}),
        "01:01:00",
    )]);
    assert_eq!(side.context.in_use, InUse::Known(30_000));
    assert_eq!(side.context.replies, 1, "a subagent's reply isn't the conversation's");

    // Another project's CLAUDE.md pulled in, and a memory file read
    let pulled = json!({"type": "nested_memory", "path": "C:/Users/me/Work/Ledger/CLAUDE.md", "displayPath": "CLAUDE.md",
        "content": {"path": "C:/Users/me/Work/Ledger/CLAUDE.md", "type": "Project", "content": "n".repeat(4000)}});
    let read = feed(&[
        attached(pulled, "C:/Users/me/Work/Ledger", "01:02:00"),
        tool("r1", "Read", json!({"file_path": format!("{memory}/feedback_x.md")}), "01:02:01"),
        line(json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "r1", "content": "r".repeat(1200)}]}}), "01:02:02"),
        reply(36_000, "01:02:03"),
    ]);
    let tail: Vec<(K, usize, bool)> = read.context.files.iter().skip(3).map(|f| (f.kind, f.tokens, f.elsewhere)).collect();
    assert_eq!(tail, vec![(K::Later, 1000, true), (K::Remembered, 300, false)]);
    let bar = sections(&read.context, Some(200_000)).unwrap();
    let whats: Vec<What> = bar.sections.iter().map(|s| s.what).collect();
    use What as W;
    assert_eq!(whats, vec![W::User, W::Project, W::MemoryIndex, W::Elsewhere, W::Memory, W::System, W::Tools, W::Skills, W::Conversation, W::Free]);
    assert_eq!((bar.in_use, bar.window, bar.estimate), (36_000, 200_000, false));

    // A resume: the start's files again, a new process's first request and its blobs. Nothing is
    // dropped, the scale stays, and the blobs aren't read
    let resumed = feed(&[
        attached(files.clone(), own, "02:00:00"),
        shape("s2", 8000),
        blob("m9", 40_000),
        request("s2", 0, &["m9"]),
        reply(41_000, "02:00:05"),
    ]);
    assert_eq!(resumed.context.files.len(), 5, "what was pulled in is still in the history");
    assert_eq!(resumed.context.scale, first.context.scale);
    assert_eq!(resumed.context.parts.unwrap().system, 2000, "the latest request's system prompt");
    assert_eq!(resumed.context.in_use, InUse::Known(41_000));

    // A compaction: the files go, and what's in use is its own size until the next reply
    let compacted = feed(&[line(
        json!({"type": "system", "subtype": "compact_boundary", "cwd": own, "compactMetadata": {"trigger": "manual", "preTokens": 41_000, "postTokens": 3000}}),
        "03:00:00",
    )]);
    assert!(compacted.context.files.is_empty());
    assert_eq!(compacted.context.in_use, InUse::Compacted { post: 3000 });
    let after = feed(&[attached(files, own, "03:00:01")]);
    let bar = sections(&after.context, Some(200_000)).unwrap();
    assert!(bar.estimate);
    assert_eq!(bar.sections.iter().find(|s| s.what == W::Conversation).unwrap().tokens, 3000);
    let next = feed(&[request("s2", 4, &["m10"]), reply(52_000, "03:00:05")]);
    assert_eq!(next.context.in_use, InUse::Known(52_000));
    assert!(!sections(&next.context, Some(200_000)).unwrap().estimate);
    assert!(!f.conversation.wants_blobs(), "no blob is read once the scale is made");
}

/// With `LBTERM_CONTEXT_TRANSCRIPT` naming a transcript (one of your own: never copy it into the
/// repo), fails on purpose with the context bar's sections read from it by the reader, beside
/// what Claude Code's /context said the last time it ran there (its `contextUsage`), the
/// transcript read up to that record: lbterm/CONTEXT.md, "Measured, against Claude Code's own
/// /context". The window is /context's, else `LBTERM_CONTEXT_WINDOW`, else 1M. Ignored, so the
/// check never counts it passed for having nothing to read (lbterm/PITFALLS.md, 11): run it with
/// `lbterm.ps1 test -Package lbterm-extras -Ignored`.
#[test]
#[ignore = "a report on a transcript LBTERM_CONTEXT_TRANSCRIPT names: lbterm.ps1 test -Package lbterm-extras -Ignored"]
fn report_the_context_bar_on_an_own_transcript() {
    let Ok(path) = std::env::var("LBTERM_CONTEXT_TRANSCRIPT") else {
        return;
    };
    let text = std::fs::read_to_string(&path).expect("the transcript LBTERM_CONTEXT_TRANSCRIPT names");
    let lines: Vec<&str> = text.lines().collect();
    let asked = lines.iter().rposition(|l| l.contains(r#""contextUsage""#));
    let mut f = Follow::default();
    for l in &lines[..asked.unwrap_or(lines.len())] {
        f.feed(format!("{l}\n").as_bytes());
    }
    let snapshot = f.conversation.snapshot();
    let usage: Option<Value> = asked.and_then(|i| serde_json::from_str::<Value>(lines[i]).ok()).and_then(|r| r.get("contextUsage").cloned());
    let window = usage
        .as_ref()
        .and_then(|u| u.get("raw_max_tokens").and_then(Value::as_u64))
        .or_else(|| std::env::var("LBTERM_CONTEXT_WINDOW").ok().and_then(|w| w.parse().ok()))
        .unwrap_or(1_000_000);
    let c = &snapshot.context;
    let mut report = format!("in use {:?}, scale {:?}, parts {:?}, window {window}\n", c.in_use, c.scale, c.parts);
    match crate::context_bar::sections(c, Some(window)) {
        Err(waiting) => report += &format!("the bar waits: {}\n", waiting.words()),
        Ok(bar) => {
            for s in &bar.sections {
                report += &format!("  {:<14} {:>8}  {}\n", s.what.word(), s.tokens, s.path.as_deref().unwrap_or(""));
            }
        }
    }
    if let Some(u) = usage {
        report += &format!("/context: total {}\n", u.get("total_tokens").unwrap_or(&Value::Null));
        for c in u.get("categories").and_then(Value::as_array).into_iter().flatten() {
            report += &format!("  {:<26} {}\n", c.get("name").and_then(Value::as_str).unwrap_or(""), c.get("tokens").unwrap_or(&Value::Null));
        }
        for m in u.get("memory_files").and_then(Value::as_array).into_iter().flatten() {
            report += &format!("  memory file {:>8}  {}\n", m.get("tokens").unwrap_or(&Value::Null), m.get("path").and_then(Value::as_str).unwrap_or(""));
        }
    }
    panic!("the context bar on {path}:\n{report}");
}
