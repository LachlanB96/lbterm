use super::*;

/// What the Claude project's count (`@Claude:tokens`' `conversationTokens`) makes of the fixture,
/// to the dollar's millionth: `.claude/tests/cost-prices.test.ts` counts it again with that module
/// and holds this to it
const FIXTURE_USD: f64 = 0.673102;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/cost/fixture/sess.jsonl")
}

fn counted(main: &Path) -> (Fold, Vec<(Spender, Option<String>, Fold)>) {
    let mut fold = Fold::default();
    fold.read(main).unwrap();
    let dir = main.with_extension("");
    let agents = agent_transcripts(main)
        .into_iter()
        .map(|p| {
            let (s, w) = spender_of(p.strip_prefix(&dir).unwrap());
            let mut f = Fold::default();
            f.read(&p).unwrap();
            (s, w, f)
        })
        .collect();
    (fold, agents)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_fixture_costs_what_the_claude_projects_count_makes_it_split_by_what_spent_it() {
    let main = fixture();
    let found = agent_transcripts(&main);
    // A workflow's journal isn't a transcript
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|p| p.file_name().unwrap() != "journal.jsonl"));
    let (fold, agents) = counted(&main);
    let refs: Vec<(Spender, Option<String>, &Fold)> = agents.iter().map(|(s, w, f)| (*s, w.clone(), f)).collect();
    let c = cost(&fold, &refs);
    assert!(close(c.total(), FIXTURE_USD), "{} against {FIXTURE_USD}", c.total());
    // His turns: the first message counted once, as its last record says (1h writes at twice
    // input), the second, the synthetic one free, the sonnet one once, the one with no usage not
    assert!(close(c.part(Spender::Turns).usd.total(), 0.224552), "{:?}", c.part(Spender::Turns));
    assert_eq!(c.part(Spender::Turns).tokens, Tokens { input: 6013, writes: 21500, reads: 20000, output: 2500 });
    assert!(close(c.part(Spender::Turns).usd.writes, 0.16 + 0.0075));
    assert!(close(c.part(Spender::Subagents).usd.total(), 0.00151));
    assert_eq!(c.part(Spender::Subagents).runs, 1);
    assert!(close(c.part(Spender::Workflows).usd.total(), 0.43204 + 0.015));
    assert_eq!(c.part(Spender::Workflows).runs, 1, "two agents of one workflow");
    // Its steps in time order, the last the total; a mark where each started
    assert!(c.steps.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 <= w[1].1));
    assert!(close(c.steps.last().unwrap().1, FIXTURE_USD));
    let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp() as f64;
    assert_eq!(
        c.marks,
        vec![
            Mark { spender: Spender::Subagents, at: at("2026-11-09T10:20:00Z") },
            Mark { spender: Spender::Workflows, at: at("2026-11-09T10:30:00Z") },
        ]
    );
    assert!(close(c.at(at("2026-11-09T10:10:00Z")), 0.16804 + 0.035512));
    assert_eq!(c.at(at("2026-11-09T09:00:00Z")), 0.0);
    // Its compaction: the context it compacted, and the sonnet request's after it, as the Claude
    // project's split has them (`before`, `next`)
    assert_eq!(c.compactions, vec![Compaction { at: Some(at("2026-11-09T11:00:00Z")), before: Some(42000), next: Some(6000) }]);
}

#[test]
fn a_fold_reads_on_from_its_last_whole_line_and_a_message_split_across_reads_counts_once() {
    let usage = |out: u64| format!(r#"{{"type":"assistant","timestamp":"2026-11-09T10:00:00Z","message":{{"id":"a","model":"claude-opus-5-5","usage":{{"input_tokens":1000,"output_tokens":{out}}}}}}}"#);
    let mut f = Fold::default();
    let first = format!("{}\n{}", usage(10), &usage(1000)[..20]);
    // The line not yet ended is left
    assert_eq!(f.fold(first.as_bytes()), usage(10).len() + 1);
    assert_eq!(f.requests.len(), 1);
    // Its next record of the same message replaces it
    f.fold(format!("{}\n", usage(1000)).as_bytes());
    assert_eq!(f.requests.len(), 1);
    assert_eq!(f.requests[0].tokens.output, 1000);
    // Another message between them makes a repeat of the first a request of its own, as the
    // Claude project's count has it
    let b = usage(5).replace("\"a\"", "\"b\"");
    f.fold(format!("{b}\n{}\n", usage(7)).as_bytes());
    assert_eq!(f.requests.len(), 3);
    // Records without an id are each a request
    let none = usage(1).replace(r#""id":"a","#, "");
    f.fold(format!("{none}\n{none}\n").as_bytes());
    assert_eq!(f.requests.len(), 5);
}

#[test]
fn a_file_written_again_shorter_is_read_again_from_its_start() {
    let dir = std::env::temp_dir().join(format!("lbterm-cost-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.jsonl");
    let line = r#"{"type":"assistant","message":{"id":"x","model":"opus","usage":{"input_tokens":1000000}}}"#;
    std::fs::write(&path, format!("{line}\n{}\n", line.replace("\"x\"", "\"y\""))).unwrap();
    let mut f = Fold::default();
    assert!(f.read(&path).unwrap());
    assert_eq!(f.requests.len(), 2);
    assert!(!f.read(&path).unwrap(), "nothing new");
    std::fs::write(&path, format!("{line}\n")).unwrap();
    assert!(f.read(&path).unwrap());
    assert_eq!(f.requests.len(), 1);
    assert!(close(f.requests[0].usd.input, 4.0));
    // Written again empty: nothing taken, but what it held went, so the count is made again
    std::fs::write(&path, "").unwrap();
    assert!(f.read(&path).unwrap(), "emptied, and not said");
    assert!(f.requests.is_empty());
    assert!(!f.read(&path).unwrap(), "nothing new");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_request_is_priced_by_its_models_family() {
    let u: Value = serde_json::from_str(r#"{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000,"cache_creation_input_tokens":1000000}"#).unwrap();
    let r = priced("Claude-OPUS-5-5", &u);
    assert_eq!(r.usd, Dollars { input: 4.0, writes: 5.0, reads: 0.2, output: 20.0 });
    assert_eq!(priced("claude-haiku-4-5", &u).usd.total(), 0.1 + 0.125 + 0.01 + 0.5);
    assert_eq!(priced("<synthetic>", &u).usd.total(), 0.0);
    assert_eq!(priced("<synthetic>", &u).tokens.input, 1_000_000);
    // The cache's two kinds of write by its own counts, when it gives them
    let split: Value = serde_json::from_str(r#"{"cache_creation_input_tokens":1000000,"cache_creation":{"ephemeral_5m_input_tokens":400000,"ephemeral_1h_input_tokens":400000}}"#).unwrap();
    assert!(close(priced("claude-sonnet-4-5", &split).usd.writes, 0.4 * 2.0 * 1.25 + 0.4 * 2.0 * 2.0));
}

#[test]
fn a_model_that_lists_apart_from_its_family_is_priced_by_its_own_row() {
    // The Claude project's run of 2026-11-11, `claude -p --model claude-fable-5-2`: 2 input tokens,
    // 21,944 written for an hour, 16,534 read and 4 output came to a total_cost_usd of 0.3545868,
    // where the family's row gave $0.4432
    let run: Value = serde_json::from_str(
        r#"{"input_tokens":2,"output_tokens":4,"cache_read_input_tokens":16534,"cache_creation_input_tokens":21944,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":21944}}"#,
    )
    .unwrap();
    assert!(close(priced("claude-fable-5-2", &run).usd.total(), 0.3545868));
    assert!(close(priced("claude-fable-5-1", &run).usd.total(), 0.4432335));

    let u: Value = serde_json::from_str(r#"{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}"#).unwrap();
    let own = Dollars { input: 8.0, writes: 0.0, reads: 0.2, output: 40.0 };
    let family = Dollars { input: 10.0, writes: 0.0, reads: 0.25, output: 50.0 };
    // Its dated, 1M-context and cloud names, any case
    for model in ["claude-fable-5-2", "Claude-FABLE-5-2", "claude-fable-5-2-20261101", "claude-fable-5-2[1m]", "us.anthropic.claude-fable-5-2-v1:0"] {
        assert_eq!(priced(model, &u).usd, own, "{model}");
    }
    // A digit straight after the name is another model, which its family prices
    for model in ["claude-fable-5", "claude-fable-5-1", "claude-fable-5-20", "claude-fable-5-25", "fable-5.2", "claude-fable-1"] {
        assert_eq!(priced(model, &u).usd, family, "{model}");
    }
    // A name that holds the row's twice is its model if either has no digit after it
    assert_eq!(priced("fable-5-20-fable-5-2", &u).usd, own);

    // Sonnet 5.5 reads its cache at half its family's price. The Claude project's two runs of
    // 2026-11-12, a minute apart: 2 input, 34,211 written for an hour and 4 output came to
    // $0.136888, then 2 input, 34,211 read and 4 output to $0.0034651, where the family's row
    // gives $0.0068862
    let wrote: Value = serde_json::from_str(
        r#"{"input_tokens":2,"output_tokens":4,"cache_creation_input_tokens":34211,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":34211}}"#,
    )
    .unwrap();
    let read: Value = serde_json::from_str(r#"{"input_tokens":2,"output_tokens":4,"cache_read_input_tokens":34211}"#).unwrap();
    assert!(close(priced("claude-sonnet-5-5", &wrote).usd.total(), 0.136888));
    assert!(close(priced("claude-sonnet-5-5", &read).usd.total(), 0.0034651));
    assert!(close(priced("claude-sonnet-5", &read).usd.total(), 0.0068862));
    assert_eq!(priced("claude-sonnet-5-5", &u).usd, Dollars { input: 2.0, writes: 0.0, reads: 0.1, output: 10.0 });
    for model in ["claude-sonnet-5", "claude-sonnet-4-5", "claude-sonnet-5-50"] {
        assert_eq!(priced(model, &u).usd, Dollars { input: 2.0, writes: 0.0, reads: 0.2, output: 10.0 }, "{model}");
    }
}

#[test]
fn a_transcript_under_a_workflows_folder_is_its_workflows() {
    assert_eq!(spender_of(Path::new("subagents/agent-a.jsonl")), (Spender::Subagents, None));
    assert_eq!(spender_of(Path::new("subagents/workflows/wf_1/agent-a.jsonl")), (Spender::Workflows, Some("wf_1".to_string())));
    assert_eq!(spender_of(Path::new("subagents/workflows/wf_1/x/agent-a.jsonl")), (Spender::Workflows, Some("wf_1".to_string())));
    assert_eq!(spender_of(Path::new("workflows.jsonl")), (Spender::Subagents, None));
}

#[test]
fn tokens_and_dollars_are_written_short() {
    assert_eq!(tokens(950), "950");
    assert_eq!(tokens(1_250), "1.2k");
    assert_eq!(tokens(340_400), "340k");
    assert_eq!(tokens(8_250_000), "8.2M");
    assert_eq!(tokens(12_600_000), "12M");
    assert_eq!(dollars(3.559), "$3.55");
    assert_eq!(dollars(1234.5), "$1234.50");
    assert_eq!(dollars(f64::NAN), "$0.00");
}

#[test]
fn the_pane_lays_out_the_total_a_row_a_spender_and_the_graph_beside_or_under() {
    let (fold, agents) = counted(&fixture());
    let refs: Vec<(Spender, Option<String>, &Fold)> = agents.iter().map(|(s, w, f)| (*s, w.clone(), f)).collect();
    let c = cost(&fold, &refs);
    let cell = (8.0, 17.0);
    let columns = |t: &str| t.chars().count() as f64 * 8.0;
    let wide = pane(Some(&c), 100.0, 50.0, 1600.0, cell, 1.0, 0, &columns);
    assert_eq!(wide.words[0].text, "$0.67");
    assert_eq!(wide.words[1].text, HEADING);
    assert_eq!(wide.rows.len(), 4);
    let texts: Vec<&str> = wide.words.iter().map(|w| w.text.as_str()).collect();
    for want in ["Your turns", "Subagents (1)", "Workflows (1)", "Compactions (1)", NOT_RECORDED, OVER_TIME, "input", "cache writes", "cache reads", "output", "workflow", "subagent", "10:10", "42k → 6.0k"] {
        assert!(texts.contains(&want), "{want} in {texts:?}");
    }
    // The shares' bar, first: a part a spender that spent anything, across the table
    let bar = &wide.fills[..3];
    assert!(bar.iter().all(|f| matches!(f.tone, Tone::Part(_)) && f.rect.y == bar[0].rect.y), "{bar:?}");
    let end = bar[2].rect.x + bar[2].rect.w;
    assert_eq!(end, bar[0].rect.x + TABLE * 8.0);
    // Beside the table, and everything inside the pane
    let graph = wide.graph.expect("a graph");
    assert!(graph.x > 100.0 + TABLE * 8.0, "{graph:?}");
    let inside = |r: &Rect, p: &Pane| r.x >= p.rect.x && r.y >= p.rect.y && r.x + r.w <= p.rect.x + p.rect.w + 0.5 && r.y + r.h <= p.rect.y + p.rect.h + 0.5;
    for f in &wide.fills {
        assert!(inside(&f.rect, &wide), "{f:?} in {:?}", wide.rect);
    }
    // The line inside the plot, rising left to right
    let line: Vec<&Fill> = wide.fills.iter().filter(|f| f.tone == Tone::Line).collect();
    assert!(line.len() >= 4);
    assert!(line.iter().all(|f| f.rect.x >= graph.x && f.rect.x + f.rect.w <= graph.x + graph.w + 0.5 && f.rect.y >= graph.y));
    let lowest = line.iter().map(|f| f.rect.y).fold(f64::INFINITY, f64::min);
    assert!(line.last().unwrap().rect.y <= lowest + 1.0, "it ends at its top");
    // Narrow: under the table, the pane taller
    let narrow = pane(Some(&c), 0.0, 0.0, 800.0, cell, 1.0, 0, &columns);
    let under = narrow.graph.expect("a graph under it");
    assert!(under.y > narrow.rows.last().unwrap().y);
    assert!(narrow.rect.h > wide.rect.h);
    // In another time zone the clock says its own time
    let east = pane(Some(&c), 100.0, 50.0, 1600.0, cell, 1.0, 11 * 3600, &columns);
    assert!(east.words.iter().any(|w| w.text == "21:10"), "{:?}", east.words.iter().map(|w| &w.text).collect::<Vec<_>>());
}

#[test]
fn the_pane_says_it_is_counting_until_the_count_is_in() {
    let p = pane(None, 0.0, 0.0, 900.0, (8.0, 17.0), 1.0, 0, &|t: &str| t.chars().count() as f64 * 8.0);
    assert_eq!(p.note.as_deref(), Some(COUNTING));
    assert_eq!(p.words.len(), 1);
    assert!(p.graph.is_none());
}

#[test]
fn without_the_claude_projects_split_the_pane_has_only_the_pills_dollars_and_window_and_says_why() {
    let mono = |t: &str| t.chars().count() as f64 * 8.0;
    let p = usage_pane(Some(0.67), Some(200_000), NOT_PUBLISHED, 100.0, 40.0, 900.0, (8.0, 17.0), &mono);
    let words: Vec<&str> = p.words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(words, ["$0.67", HEADING, "200k context window", NOT_PUBLISHED]);
    assert_eq!(p.note.as_deref(), Some(NOT_PUBLISHED));
    assert!(p.rows.is_empty() && p.fills.is_empty() && p.graph.is_none(), "nothing of a count: {p:?}");
    assert_eq!((p.rect.x, p.rect.y, p.rect.w), (100.0, 40.0, 900.0));
    // Each word inside the card, one under the other, the card to their last line and its pad
    for w in &p.words {
        assert!(w.x >= p.rect.x && w.x + w.w <= p.rect.x + p.rect.w + 1e-9 && w.y + 17.0 <= p.rect.y + p.rect.h, "{w:?} outside {:?}", p.rect);
    }
    assert!(p.words[1].x >= p.words[0].x + mono("$0.67"), "the heading after the dollars");
    assert!(p.words[2].y > p.words[0].y && p.words[3].y > p.words[2].y);
    assert_eq!(p.rect.h, 13.0 + 26.0 + 17.0 + 17.0 + 13.0);
    // Before Terminal's statusline has written the file, or Claude Code said the window: the line
    let p = usage_pane(None, None, NO_SUMMARIES, 0.0, 0.0, 900.0, (8.0, 17.0), &mono);
    assert_eq!(p.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), [NO_SUMMARIES]);
    let p = usage_pane(Some(12.345), None, NOT_PUBLISHED, 0.0, 0.0, 900.0, (8.0, 17.0), &mono);
    assert_eq!(p.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["$12.34", HEADING, NOT_PUBLISHED]);
}

/// The fixture conversation's session, whose split the Claude project's `rememberCost` wrote as
/// `src/cost/published/<it>.cost.json` (`.claude/tests/cost-prices.test.ts` writes it again with
/// that module and holds the file to it)
const PUBLISHED_SID: &str = "5d2c8e17-3a41-4b6f-9c0e-2f7a1b8d4e60";

fn published_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/cost/published")
}

#[test]
fn the_claude_projects_split_of_the_fixture_is_the_pane_its_own_count_makes_with_the_pills_total() {
    let split = read_published(&published_folder(), PUBLISHED_SID).unwrap();
    let (fold, agents) = counted(&fixture());
    let refs: Vec<(Spender, Option<String>, &Fold)> = agents.iter().map(|(s, w, f)| (*s, w.clone(), f)).collect();
    let own = cost(&fold, &refs);
    // Its total is the file's usd, to the cent as usage.json and the pill have it, not its parts'
    assert_eq!(split.usd, Some(0.67));
    assert_eq!(split.total(), 0.67);
    assert_eq!(own.usd, None);
    // Each row as the own count makes it: the tokens exactly, the dollars to the millionth
    for s in Spender::ALL {
        let (a, b) = (split.part(s), own.part(s));
        assert_eq!(a.tokens, b.tokens, "{s:?}");
        assert_eq!(a.runs, b.runs, "{s:?}");
        assert!(close(a.usd.total(), b.usd.total()), "{s:?}: {} against {}", a.usd.total(), b.usd.total());
        for (x, y) in [(a.usd.input, b.usd.input), (a.usd.writes, b.usd.writes), (a.usd.reads, b.usd.reads), (a.usd.output, b.usd.output)] {
            assert!(close(x, y), "{s:?}: {x} against {y}");
        }
    }
    assert_eq!(split.part(Spender::Workflows).runs, 1, "two agents of one run");
    assert_eq!(split.compactions, own.compactions);
    // The same marks; steps a minute, its last the parts' total
    assert_eq!(split.marks, own.marks);
    assert_eq!(split.steps.len(), 8);
    assert!(split.steps.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1));
    assert!(close(split.steps.last().unwrap().1, FIXTURE_USD));
    // Laid out as the count is: the same rows, words and graph but the total's
    let mono = |t: &str| t.chars().count() as f64 * 8.0;
    let a = pane(Some(&split), 0.0, 0.0, 1400.0, (8.0, 17.0), 1.0, 0, &mono);
    let b = pane(Some(&own), 0.0, 0.0, 1400.0, (8.0, 17.0), 1.0, 0, &mono);
    assert_eq!(a.rows, b.rows);
    assert!(a.graph.is_some() && a.note.is_none());
    let words = |p: &Pane| p.words.iter().map(|w| w.text.clone()).collect::<Vec<_>>();
    for word in ["$0.67", "Your turns", "Subagents (1)", "Workflows (1)", "Compactions (1)", OVER_TIME] {
        assert!(words(&a).iter().any(|w| w == word), "no {word} in {:?}", words(&a));
    }
}

#[test]
fn a_split_groups_workflow_agents_by_run_and_marks_each_runs_first_start() {
    // As the Claude project writes it for a conversation with labels, two subagents and two runs
    let text = r#"{"v":1,"usd":1.23,"spenders":[
        {"kind":"main","id":"5d2c8e17-3a41-4b6f-9c0e-2f7a1b8d4e60","run":null,"label":null,"start":1794218405000,"end":1794222601000,"requests":9,"usd":0.5,"tokens":[10,1000,20000,300],"dollars":[0.1,0.2,0.1,0.1]},
        {"kind":"subagent","id":"aexplore1","run":null,"label":"Find the reader","start":1794219000000,"end":1794219060000,"requests":2,"usd":0.03,"tokens":[1,2,3,4],"dollars":[0.01,0.01,0.005,0.005]},
        {"kind":"workflow","id":"a1","run":"wf_one","label":"track:build","start":1794219600000,"end":1794219700000,"requests":3,"usd":0.3,"tokens":[1,1,1,1],"dollars":[0.1,0.1,0.05,0.05]},
        {"kind":"subagent","id":"aexplore2","run":null,"label":null,"start":1794220000000,"end":1794220060000,"requests":1,"usd":0.02,"tokens":[1,1,1,1],"dollars":[0.005,0.005,0.005,0.005]},
        {"kind":"workflow","id":"a2","run":"wf_two","label":"flow:trace","start":1794220500000,"end":1794220600000,"requests":1,"usd":0.28,"tokens":[2,2,2,2],"dollars":[0.07,0.07,0.07,0.07]},
        {"kind":"workflow","id":"a3","run":"wf_one","label":"track:review","start":1794221000000,"end":1794221100000,"requests":1,"usd":0.1,"tokens":[1,1,1,1],"dollars":[0.025,0.025,0.025,0.025]}],
      "compactions":[{"at":1794221500000,"trigger":"auto","before":767304,"after":16146},{"at":1794222000000,"trigger":"manual","before":590823,"after":15105}],
      "steps":[[1794218400000,0.3],[1794219000000,0.6],[1794220500000,1.0],[1794222600000,1.23]]}"#;
    let c = published(text).unwrap();
    assert_eq!(c.total(), 1.23);
    assert_eq!((c.part(Spender::Subagents).runs, c.part(Spender::Workflows).runs), (2, 2));
    assert!(close(c.part(Spender::Subagents).usd.total(), 0.05));
    assert!(close(c.part(Spender::Workflows).usd.total(), 0.68));
    assert_eq!(c.part(Spender::Workflows).tokens, Tokens { input: 4, writes: 4, reads: 4, output: 4 });
    // Each compaction's time and sizes; `next` absent from a split written before 2026-11-13
    assert_eq!(
        c.compactions,
        vec![
            Compaction { at: Some(1794221500.0), before: Some(767304), next: None },
            Compaction { at: Some(1794222000.0), before: Some(590823), next: None },
        ]
    );
    let marks: Vec<(Spender, f64)> = c.marks.iter().map(|m| (m.spender, m.at)).collect();
    assert_eq!(
        marks,
        [(Spender::Subagents, 1794219000.0), (Spender::Workflows, 1794219600.0), (Spender::Subagents, 1794220000.0), (Spender::Workflows, 1794220500.0)],
        "wf_one marked at its first agent's start"
    );
    assert_eq!(c.steps[0], (1794218400.0, 0.3));
    assert_eq!(c.at(1794220000.0), 0.6);
}

#[test]
fn a_split_of_another_version_a_missing_one_and_one_not_in_its_shape_say_so() {
    let folder = tempfile::tempdir().unwrap();
    // Not written yet for this conversation: never $0
    assert_eq!(read_published(folder.path(), PUBLISHED_SID), Err(NOT_PUBLISHED.to_string()));
    // A version 2, whatever it holds
    std::fs::write(folder.path().join(format!("{PUBLISHED_SID}.cost.json")), r#"{"v":2,"usd":1.5,"by":{}}"#).unwrap();
    assert_eq!(
        read_published(folder.path(), PUBLISHED_SID),
        Err("The Claude project's split is version 2, and this LBTerm reads version 1".to_string())
    );
    for text in ["{\"usd\":1}", "not json", r#"{"v":1,"usd":1}"#, r#"{"v":1,"usd":1,"spenders":[{"kind":"robot","run":null,"start":1,"tokens":[0,0,0,0],"dollars":[0,0,0,0]}],"compactions":[],"steps":[]}"#] {
        let why = published(text).unwrap_err();
        assert!(why.starts_with(UNREADABLE), "{text}: {why}");
    }
    // A sid that isn't one is never made into a path
    assert!(read_published(folder.path(), "../usage").unwrap_err().starts_with(UNREADABLE));
}

#[test]
fn the_graphs_times_and_marks_have_room_for_their_words_as_the_font_lays_them_out() {
    // The title font's digits are wider than its column (PITFALLS 76): pane.png of 2026-11-09 cut
    // 21:00 to 21:0 and workflow to workflo when each word's room was its characters' columns
    let (fold, agents) = counted(&fixture());
    let refs: Vec<(Spender, Option<String>, &Fold)> = agents.iter().map(|(s, w, f)| (*s, w.clone(), f)).collect();
    let c = cost(&fold, &refs);
    let wider = |t: &str| t.chars().count() as f64 * 8.0 * 1.4;
    let p = pane(Some(&c), 100.0, 50.0, 1600.0, (8.0, 17.0), 1.0, 0, &wider);
    let graph = p.graph.expect("a graph");
    let measured: Vec<&Word> = p.words.iter().filter(|w| (w.text.len() == 5 && w.text.contains(':')) || w.text == "workflow" || w.text == "subagent" || w.text == p.words[0].text).collect();
    assert!(measured.len() >= 4, "{measured:?}");
    for w in measured {
        // More than its advances: the box model draws a glyph only while its ink ends short of the
        // room, and ink can pass the advance (PITFALLS 76)
        assert!(w.w > wider(&w.text), "{w:?} has more room than its {}", wider(&w.text));
        assert!(w.x + w.w <= p.rect.x + p.rect.w, "{w:?} inside {:?}", p.rect);
    }
    // No time over the one before it
    let times: Vec<&Word> = p.words.iter().filter(|w| w.text.len() == 5 && w.text.contains(':')).collect();
    assert!(times.windows(2).all(|a| a[0].x + wider(&a[0].text) < a[1].x), "{times:?}");
    // The heading starts past the total's measured width
    assert!(p.words[1].x >= p.words[0].x + wider(&p.words[0].text));
    // The plot starts past the widest of its dollars
    let dollars: Vec<&Word> = p.words.iter().filter(|w| w.text.starts_with('$') && w.y > p.words[0].y).collect();
    assert!(dollars.iter().all(|w| w.x + wider(&w.text) <= graph.x), "{dollars:?} left of {graph:?}");
}

#[test]
fn each_compaction_is_a_rule_down_the_graph_named_with_the_context_before_it_and_the_context_after() {
    let request = |t: &str, id: &str, model: &str, input: u64, reads: u64, writes: u64| {
        format!(r#"{{"type":"assistant","timestamp":"2026-11-09T{t}Z","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{input},"cache_read_input_tokens":{reads},"cache_creation_input_tokens":{writes},"output_tokens":100}}}}}}"#)
    };
    let boundary = |t: &str, pre: u64| format!(r#"{{"type":"system","subtype":"compact_boundary","timestamp":"2026-11-09T{t}Z","compactMetadata":{{"trigger":"manual","preTokens":{pre},"postTokens":12343}}}}"#);
    let lines = [
        request("10:00:00", "a", "claude-opus-5-5", 1000, 0, 500000),
        boundary("10:30:00", 545118),
        // Never sent, so not the request carried on with
        request("10:30:05", "s", "<synthetic>", 0, 0, 0),
        request("10:31:00", "b", "claude-opus-5-5", 3, 50000, 4643),
        request("10:40:00", "c", "claude-opus-5-5", 3, 54646, 900),
        // Compacted, and no request since
        boundary("10:50:00", 600000),
    ];
    let mut fold = Fold::default();
    fold.fold(format!("{}\n", lines.join("\n")).as_bytes());
    let sizes: Vec<(Option<u64>, Option<u64>)> = fold.compactions.iter().map(|k| (k.before, k.next)).collect();
    assert_eq!(sizes, [(Some(545118), Some(54646)), (Some(600000), None)]);
    let c = cost(&fold, &[]);
    let labels: Vec<Option<String>> = c.compactions.iter().map(Compaction::label).collect();
    assert_eq!(labels, [Some("545k → 54k".to_string()), Some("600k → …".to_string())]);
    // None for a compaction Claude Code recorded no size for
    assert_eq!(Compaction { at: Some(1.0), before: None, next: Some(5) }.label(), None);
    let columns = |t: &str| t.chars().count() as f64 * 8.0;
    let p = pane(Some(&c), 100.0, 50.0, 1600.0, (8.0, 17.0), 1.0, 0, &columns);
    let graph = p.graph.expect("a graph");
    // A rule each, down the plot from the heading's row (the table's square is the other grey)
    let rules: Vec<&Fill> = p.fills.iter().filter(|f| f.tone == Tone::Compactions && f.rect.h > 17.0).collect();
    assert_eq!(rules.len(), 2, "a grey rule each");
    assert!(rules.iter().all(|r| r.rect.y < graph.y && (r.rect.y + r.rect.h - (graph.y + graph.h)).abs() < 0.5), "{rules:?} down {graph:?}");
    // Under the line: drawn before it
    let first_line = p.fills.iter().position(|f| f.tone == Tone::Line).unwrap();
    assert!(p.fills.iter().position(|f| f == rules[1]).unwrap() < first_line);
    // The graph's time runs on to the compaction no request has followed: its rule at the plot's end
    assert!((rules[1].rect.x - (graph.x + graph.w - 1.0)).abs() <= 1.0, "{:?} at the end of {graph:?}", rules[1]);
    let named: Vec<&Word> = p.words.iter().filter(|w| w.tone == Tone::Compactions).collect();
    assert_eq!(named.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["545k → 54k", "600k → …"]);
    // In the heading's row, past "Over time"; the first right of its rule, the last, with no room
    // right of it, left of its rule
    let heading = p.words.iter().find(|w| w.text == OVER_TIME).unwrap();
    assert!(named.iter().all(|w| w.y == heading.y && w.x >= heading.x + columns(OVER_TIME)), "{named:?} beside {heading:?}");
    assert!(named[0].x >= rules[0].rect.x + rules[0].rect.w, "{:?} right of {:?}", named[0], rules[0]);
    assert!(named[1].x + columns(&named[1].text) <= rules[1].rect.x, "{:?} left of {:?}", named[1], rules[1]);
}

#[test]
fn a_compactions_name_never_crosses_another_compactions_rule() {
    // As perf/cost.ps1's stand-in split has them: two compactions three minutes apart near the end
    // of a fifty-minute graph, neither followed by a request yet. The first's name right of its
    // rule ran over the second's rule (the look of 2026-11-14), so it goes left of its own, and
    // the second's has room on neither side
    let t0 = 1794218400.0;
    let c = Cost {
        usd: Some(0.87),
        steps: vec![(t0, 0.2), (t0 + 600.0, 0.4), (t0 + 1800.0, 0.7), (t0 + 3000.0, 0.87)],
        compactions: vec![
            Compaction { at: Some(t0 + 2700.0), before: Some(700000), next: None },
            Compaction { at: Some(t0 + 2900.0), before: Some(500000), next: None },
        ],
        ..Cost::default()
    };
    let columns = |t: &str| t.chars().count() as f64 * 8.0;
    let p = pane(Some(&c), 100.0, 50.0, 1600.0, (8.0, 17.0), 1.0, 0, &columns);
    let rules: Vec<f64> = p.fills.iter().filter(|f| f.tone == Tone::Compactions && f.rect.h > 17.0).map(|f| f.rect.x).collect();
    assert_eq!(rules.len(), 2);
    let named: Vec<&Word> = p.words.iter().filter(|w| w.tone == Tone::Compactions).collect();
    assert_eq!(named.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["700k → …"]);
    assert!(named[0].x + columns(&named[0].text) <= rules[0], "{:?} left of its rule at {}", named[0], rules[0]);
    for w in &named {
        assert!(rules.iter().all(|x| *x + 1.0 <= w.x || w.x + columns(&w.text) <= *x), "{w:?} across a rule of {rules:?}");
    }
}
