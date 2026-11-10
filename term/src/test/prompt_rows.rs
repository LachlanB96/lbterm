//! LBTerm: whether a line holds a shell's prompt, read off its attributes
//! (Line::has_semantic_type, which the scrollbar's and the scroll strip's
//! prompt marks look at for every row that changes), agrees with its
//! semantic zones, on the screen and in the scrollback, where lines are kept
//! as runs of attributes (lbterm/PERF.md, finding 37).

use super::*;
use k9::assert_equal as assert_eq;
use wezterm_cell::SemanticType;

/// The rows holding a prompt, by their attributes and by their zones
fn prompt_rows(term: &TestTerm) -> (Vec<usize>, Vec<usize>, Vec<Line>) {
    let screen = term.screen();
    let lines = screen.lines_in_phys_range(0..screen.scrollback_rows());
    let mut by_attrs = vec![];
    let mut by_zones = vec![];
    for (i, line) in lines.iter().enumerate() {
        if line.has_semantic_type(SemanticType::Prompt) {
            by_attrs.push(i);
        }
        if line
            .clone()
            .semantic_zone_ranges()
            .iter()
            .any(|z| z.semantic_type == SemanticType::Prompt)
        {
            by_zones.push(i);
        }
    }
    (by_attrs, by_zones, lines)
}

#[test]
fn a_prompt_is_found_by_its_attributes_as_by_its_zones() {
    let mut term = TestTerm::new(4, 20, 50);
    for n in 0..3 {
        term.print("\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\ls\r\n\x1b]133;C\x1b\\");
        for i in 0..6 {
            term.print(format!("output {n} {i}\r\n"));
        }
    }
    // A prompt still being typed at, on the screen
    term.print("\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\");
    let (by_attrs, by_zones, lines) = prompt_rows(&term);
    assert_eq!(by_attrs, vec![0, 7, 14, 21]);
    assert_eq!(by_attrs, by_zones);
    // The first three are in the scrollback, kept as runs of attributes
    assert!(format!("{:?}", lines[0]).contains("ClusteredLine"));
}

/// A line kept a cell at a time and the same line as runs of attributes, as the scrollback keeps
/// it, each with and without a prompt in it, wherever in the line
#[test]
fn either_way_a_line_is_kept_its_prompt_is_found() {
    let mut prompt = CellAttributes::default();
    prompt.set_semantic_type(SemanticType::Prompt);
    let mut input = CellAttributes::default();
    input.set_semantic_type(SemanticType::Input);
    let line = |cells: &[(&str, &CellAttributes)]| {
        Line::from_cells(
            cells
                .iter()
                .map(|(text, attrs)| Cell::new_grapheme(text, (*attrs).clone(), None))
                .collect(),
            SEQ_ZERO,
        )
    };
    let output = CellAttributes::default();
    let cases = vec![
        (
            line(&[("$", &prompt), (" ", &prompt), ("l", &input), ("s", &input)]),
            true,
        ),
        (
            line(&[("o", &output), ("u", &output), ("t", &output)]),
            false,
        ),
        (
            line(&[("o", &output), ("k", &output), (">", &prompt)]),
            true,
        ),
        (line(&[("x", &input), ("y", &input)]), false),
        (line(&[]), false),
    ];
    for (i, (line, has)) in cases.into_iter().enumerate() {
        let mut clustered = line.clone();
        clustered.compress_for_scrollback();
        assert!(
            format!("{:?}", clustered).contains("ClusteredLine"),
            "case {}",
            i
        );
        let zones = |mut l: Line| {
            l.semantic_zone_ranges()
                .iter()
                .any(|z| z.semantic_type == SemanticType::Prompt)
        };
        std::assert_eq!(
            line.has_semantic_type(SemanticType::Prompt),
            has,
            "case {i}, a cell at a time"
        );
        std::assert_eq!(
            clustered.has_semantic_type(SemanticType::Prompt),
            has,
            "case {i}, as runs"
        );
        std::assert_eq!(zones(line), has, "case {i}, its zones");
        std::assert_eq!(zones(clustered), has, "case {i}, its zones as runs");
    }
}

#[test]
fn a_prompt_overwritten_by_output_is_no_longer_one() {
    let mut term = TestTerm::new(4, 20, 50);
    term.print("\x1b]133;A\x1b\\$ \x1b]133;B\x1b\\");
    assert_eq!(prompt_rows(&term).0, vec![0]);
    // The program clears the line and writes over it as output
    term.print("\r\x1b]133;C\x1b\\\x1b[2Kdone\r\n");
    let (by_attrs, by_zones, _) = prompt_rows(&term);
    assert_eq!(by_attrs, Vec::<usize>::new());
    assert_eq!(by_attrs, by_zones);
}
