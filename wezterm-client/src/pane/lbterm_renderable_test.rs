//! LBTerm: tests of what a client pane keeps of its lines (renderable.rs)

use super::*;
use std::num::NonZeroUsize;

fn line(text: &str) -> Line {
    Line::from_text(text, &CellAttributes::default(), 1, None)
}

fn text(entry: &LineEntry) -> (&'static str, String) {
    match entry {
        LineEntry::Line(l) => ("Line", l.as_str().trim_end().to_string()),
        LineEntry::Stale(l) => ("Stale", l.as_str().trim_end().to_string()),
        LineEntry::LineAndFetching(l, _) => ("LineAndFetching", l.as_str().trim_end().to_string()),
        LineEntry::Fetching(_) => ("Fetching", String::new()),
    }
}

#[test]
fn every_line_stale_keeps_the_caches_size() {
    let mut lines = LruCache::new(NonZeroUsize::new(4).unwrap());
    let then = Instant::now();
    lines.put(1, LineEntry::Line(line("one")));
    lines.put(2, LineEntry::Fetching(then));
    lines.put(3, LineEntry::LineAndFetching(line("three"), then));
    lines.put(4, LineEntry::Stale(line("four")));
    all_stale(&mut lines, Instant::now());
    assert_eq!(lines.cap().get(), 4, "the same size, not unbounded");
    assert_eq!(text(lines.peek(&1).unwrap()), ("Stale", "one".to_string()));
    assert!(matches!(lines.peek(&2), Some(LineEntry::Fetching(t)) if *t == then), "a fetch goes on");
    assert!(matches!(lines.peek(&3), Some(LineEntry::LineAndFetching(_, t)) if *t == then));
    assert_eq!(text(lines.peek(&3).unwrap()), ("LineAndFetching", "three".to_string()));
    assert_eq!(text(lines.peek(&4).unwrap()), ("Stale", "four".to_string()));

    // Its least recently used is still the first to go, and it holds no more than before
    for row in 5..1000 {
        lines.put(row, LineEntry::Line(line("new")));
    }
    assert_eq!(lines.len(), 4);
    assert!(lines.peek(&1).is_none());
    assert!(lines.peek(&999).is_some());
}

#[test]
fn the_order_is_kept() {
    let mut lines = LruCache::new(NonZeroUsize::new(3).unwrap());
    lines.put(1, LineEntry::Line(line("a")));
    lines.put(2, LineEntry::Line(line("b")));
    lines.put(3, LineEntry::Line(line("c")));
    lines.get(&1);
    all_stale(&mut lines, Instant::now());
    lines.put(4, LineEntry::Line(line("d")));
    assert!(lines.peek(&2).is_none(), "2 was the least recently used");
    assert!(lines.peek(&1).is_some() && lines.peek(&3).is_some());
}

#[test]
fn every_line_stale_is_made_where_it_is() {
    // perf/mux-server emptied the cache into a new one, a node freed and made for every line it
    // held, at every step of a window's resize, for every pane: now each changes in place. The
    // line made stale is the one that was held, not a copy (what was left on it stays)
    let mut lines = LruCache::new(NonZeroUsize::new(4).unwrap());
    let mark = std::sync::Arc::new(1u8);
    let held = line("kept");
    held.set_appdata(std::sync::Arc::clone(&mark));
    lines.put(1, LineEntry::Line(held));
    let before = lines.peek(&1).map(|e| e as *const LineEntry);
    all_stale(&mut lines, Instant::now());
    assert_eq!(lines.peek(&1).map(|e| e as *const LineEntry), before, "the same entry, not a new one");
    match lines.peek(&1) {
        Some(LineEntry::Stale(l)) => assert!(l.get_appdata().is_some(), "the line itself, its appdata with it"),
        _ => panic!("row 1 isn't stale"),
    }
}

fn url_rules() -> Vec<Rule> {
    vec![Rule::new(r"\b\w+://\S+", "$0").unwrap()]
}

/// The links a line's cells carry, each once, in order
fn links(line: &Line) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    for cell in line.visible_cells() {
        if let Some(link) = cell.attrs().hyperlink() {
            let uri = link.uri().to_string();
            if found.last() != Some(&uri) {
                found.push(uri);
            }
        }
    }
    found
}

fn held(lines: &LruCache<StableRowIndex, LineEntry>, row: StableRowIndex) -> Vec<String> {
    match lines.peek(&row) {
        Some(LineEntry::Line(l) | LineEntry::Stale(l) | LineEntry::LineAndFetching(l, _)) => links(l),
        _ => panic!("row {} isn't held", row),
    }
}

#[test]
fn a_line_is_scanned_for_links_only_when_its_row_is_asked_for() {
    let mut lines = LruCache::new(NonZeroUsize::new(8).unwrap());
    for row in 0..4 {
        lines.put(row, LineEntry::Line(line(&format!("row {row} https://example.com/{row} end"))));
    }
    // What a push puts in the cache is as it came: no links yet
    assert!(held(&lines, 0).is_empty());
    scan_rows(&mut lines, 1..3, &url_rules(), &mut lbterm_links::KeptScans::new(), 4);
    assert!(held(&lines, 0).is_empty(), "row 0 wasn't asked for");
    assert_eq!(held(&lines, 1), vec!["https://example.com/1"]);
    assert_eq!(held(&lines, 2), vec!["https://example.com/2"]);
    assert!(held(&lines, 3).is_empty(), "the range's end isn't in it");
}

#[test]
fn a_line_is_scanned_once_until_it_is_replaced() {
    let mut lines = LruCache::new(NonZeroUsize::new(4).unwrap());
    lines.put(1, LineEntry::Line(line("go to https://example.com/a now")));
    scan_rows(&mut lines, 1..2, &url_rules(), &mut lbterm_links::KeptScans::new(), 4);
    assert_eq!(held(&lines, 1), vec!["https://example.com/a"]);
    // Scanned again with other rules, it stays as the first scan left it: a frame scans what it
    // draws every time, and only a new line costs anything
    let words = vec![Rule::new(r"\bnow\b", "https://now.example/$0").unwrap()];
    scan_rows(&mut lines, 1..2, &words, &mut lbterm_links::KeptScans::new(), 4);
    assert_eq!(held(&lines, 1), vec!["https://example.com/a"]);
    // A newer line in its place, as a push brings it, is scanned afresh
    lines.put(1, LineEntry::Line(line("go to https://example.com/b now")));
    assert!(held(&lines, 1).is_empty());
    scan_rows(&mut lines, 1..2, &url_rules(), &mut lbterm_links::KeptScans::new(), 4);
    assert_eq!(held(&lines, 1), vec!["https://example.com/b"]);
}

#[test]
fn a_line_drawn_while_it_is_fetched_again_is_scanned_and_a_row_being_fetched_is_left() {
    let mut lines = LruCache::new(NonZeroUsize::new(4).unwrap());
    let now = Instant::now();
    lines.put(1, LineEntry::Stale(line("a https://example.com/stale")));
    lines.put(2, LineEntry::LineAndFetching(line("a https://example.com/fetching"), now));
    lines.put(3, LineEntry::Fetching(now));
    scan_rows(&mut lines, 0..5, &url_rules(), &mut lbterm_links::KeptScans::new(), 4);
    assert_eq!(held(&lines, 1), vec!["https://example.com/stale"]);
    assert_eq!(held(&lines, 2), vec!["https://example.com/fetching"]);
    assert!(matches!(lines.peek(&3), Some(LineEntry::Fetching(t)) if *t == now));
    assert!(lines.peek(&0).is_none() && lines.peek(&4).is_none(), "nothing made for rows not held");
}

#[test]
fn scanning_leaves_which_line_goes_first() {
    let mut lines = LruCache::new(NonZeroUsize::new(2).unwrap());
    lines.put(1, LineEntry::Line(line("https://example.com/1")));
    lines.put(2, LineEntry::Line(line("https://example.com/2")));
    scan_rows(&mut lines, 1..2, &url_rules(), &mut lbterm_links::KeptScans::new(), 4);
    lines.put(3, LineEntry::Line(line("three")));
    assert!(lines.peek(&1).is_none(), "1 was still the least recently used");
}

#[test]
fn a_row_sent_again_after_a_scroll_takes_the_links_its_line_had() {
    let mut lines = LruCache::new(NonZeroUsize::new(8).unwrap());
    let mut kept = lbterm_links::KeptScans::new();
    let rows = |first: usize| (first..first + 4).map(|i| format!("line {i} https://example.com/{i}")).collect::<Vec<_>>();
    for (row, text) in rows(0).iter().enumerate() {
        lines.put(row as StableRowIndex, LineEntry::Line(line(text)));
    }
    scan_rows(&mut lines, 0..4, &url_rules(), &mut kept, 4);
    assert_eq!(kept.matched, 4);
    // The view scrolls a line: the server sends every row again, each a new line, three of them
    // the lines the rows below had
    for (row, text) in rows(1).iter().enumerate() {
        lines.put(row as StableRowIndex, LineEntry::Line(line(text)));
    }
    scan_rows(&mut lines, 0..4, &url_rules(), &mut kept, 4);
    assert_eq!((kept.matched, kept.kept), (5, 3), "only the new line is matched");
    for row in 0..4 {
        assert_eq!(held(&lines, row), vec![format!("https://example.com/{}", row + 1)]);
    }
}

/// What a push's dirty rows above the viewport did to the cache in upstream's walk: each row
/// popped, then make_stale's own pop finding nothing
fn upstream_drop(lines: &mut LruCache<StableRowIndex, LineEntry>, rows: Range<StableRowIndex>) {
    for row in rows {
        let _prior = lines.pop(&row);
        // make_stale, which finds the row gone
        match lines.pop(&row) {
            Some(LineEntry::Stale(old))
            | Some(LineEntry::Line(old))
            | Some(LineEntry::LineAndFetching(old, _)) => {
                lines.put(row, LineEntry::Stale(old));
            }
            Some(LineEntry::Fetching(_)) | None => {}
        }
    }
}

fn entries(lines: &LruCache<StableRowIndex, LineEntry>) -> Vec<(StableRowIndex, (&'static str, String))> {
    lines.iter().map(|(row, e)| (*row, text(e))).collect()
}

#[test]
fn dirty_rows_above_the_viewport_leave_the_cache_as_upstreams_walk_did() {
    let then = Instant::now();
    let fill = || {
        let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
        for row in (0..40).step_by(3) {
            lines.put(row, LineEntry::Line(line(&format!("row {row}"))));
        }
        lines.put(7, LineEntry::Fetching(then));
        lines.put(50_010, LineEntry::Stale(line("far down")));
        lines
    };
    // A range longer than the cache (a resize's whole scrollback) and one shorter, through rows
    // held and rows not
    for rows in [0..50_000, 5..14, 30..31, 41..45, 0..0] {
        let mut ours = fill();
        let mut theirs = fill();
        drop_rows_above(&mut ours, rows.clone());
        upstream_drop(&mut theirs, rows.clone());
        assert_eq!(entries(&ours), entries(&theirs), "rows {rows:?}: the same rows left, in the same order");
    }
    let mut lines = fill();
    drop_rows_above(&mut lines, 0..50_000);
    assert_eq!(entries(&lines), vec![(50_010, ("Stale", "far down".to_string()))]);
}

/// A line as the server's push brings it after a rewrap: sequence number 2
fn pushed(text: &str) -> Line {
    Line::from_text(text, &CellAttributes::default(), 2, None)
}

/// `rows` read as a frame reads them (get_lines and lend_rows, through read_row), the pane at
/// sequence number `seqno`: what each draws, and the rows the frame fetches
fn frame(
    lines: &mut LruCache<StableRowIndex, LineEntry>,
    rows: Range<StableRowIndex>,
    seqno: SequenceNo,
) -> (Vec<String>, Vec<StableRowIndex>) {
    let now = Instant::now();
    let mut drawn = vec![];
    let mut fetched = vec![];
    for row in rows {
        let (entry, fetch) = lbterm_lent::read_row(lines.pop(&row), seqno, now);
        if fetch {
            fetched.push(row);
        }
        drawn.push(match &entry {
            LineEntry::Line(l) | LineEntry::LineAndFetching(l, _) | LineEntry::Stale(l) => {
                l.as_str().trim_end().to_string()
            }
            LineEntry::Fetching(_) => String::new(),
        });
        lines.put(row, entry);
    }
    (drawn, fetched)
}

/// A pane's push applied as apply_changes_to_surface applies it: its bonus lines put, as put_line
/// puts one with no fetch, and taken out of its dirty rows, then the rest of those (rows_named)
fn push(
    lines: &mut LruCache<StableRowIndex, LineEntry>,
    dirty: Range<StableRowIndex>,
    bonus: &[(StableRowIndex, &str)],
    physical_top: StableRowIndex,
) -> Vec<StableRowIndex> {
    let mut dirty_rows = RangeSet::new();
    dirty_rows.add_range(dirty);
    for (row, text) in bonus {
        lines.put(*row, LineEntry::Line(pushed(text)));
        dirty_rows.remove(*row);
    }
    let fetch = rows_named(lines, &dirty_rows, physical_top, Instant::now());
    fetch.iter().flat_map(|r| r.clone()).collect()
}

fn screen(lines: &mut LruCache<StableRowIndex, LineEntry>, texts: &[&str]) {
    for (row, text) in texts.iter().enumerate() {
        lines.put(row as StableRowIndex, LineEntry::Line(line(text)));
    }
}

#[test]
fn a_resize_keeps_the_rows_held_and_takes_the_pushs_word_for_which_changed() {
    // Six rows held; the pane narrower, so its rewrap cut row 2 in two and moved rows 3 and 4 down
    // a row; the push after it names the rows from 2 on, with the lines of those in view, and
    // row 0, now above the viewport, which leaves the cache
    let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
    screen(&mut lines, &["zero", "one", "two, a long line", "three", "four", ""]);
    let fetch = push(&mut lines, 0..1, &[], 1);
    assert!(fetch.is_empty(), "a row above the viewport isn't fetched: {:?}", fetch);
    let fetch = push(
        &mut lines,
        2..6,
        &[(2, "two, a"), (3, "long line"), (4, "three"), (5, "four")],
        1,
    );
    assert!(fetch.is_empty(), "every row it names in view came with it: {:?}", fetch);
    assert!(lines.peek(&0).is_none(), "row 0 left the cache");
    let (drawn, fetched) = frame(&mut lines, 1..6, 2);
    assert_eq!(drawn, vec!["one", "two, a", "long line", "three", "four"]);
    assert!(fetched.is_empty(), "the frame after it fetches nothing: {:?}", fetched);
}

#[test]
fn a_row_the_push_names_without_its_line_is_fetched_and_the_old_line_drawn_meanwhile() {
    let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
    screen(&mut lines, &["zero", "one", "two"]);
    let fetch = push(&mut lines, 1..3, &[(2, "two, new")], 0);
    assert_eq!(fetch, vec![1], "the row named without its line is fetched");
    let (drawn, fetched) = frame(&mut lines, 0..3, 2);
    assert_eq!(drawn, vec!["zero", "one", "two, new"], "its old line drawn until the fetch brings it");
    assert!(fetched.is_empty(), "and the frame doesn't fetch it again: {:?}", fetched);
}

#[test]
fn a_resize_to_the_size_the_server_had_already_fetches_nothing() {
    // A pane closed beside this one: the server pruned it and resized this one itself, and its
    // push came before the window's own resize, whose push names nothing. Upstream made every row
    // stale at the resize, so the frame after it fetched the rows in view (budgets' close-pane, a
    // frame more than the installed build); now they're drawn as held
    let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
    screen(&mut lines, &["zero", "one", "two"]);
    let fetch = push(&mut lines, 0..3, &[(0, "zero, joined"), (1, "two"), (2, "")], 0);
    assert!(fetch.is_empty());
    let fetch = push(&mut lines, 0..0, &[], 0);
    assert!(fetch.is_empty());
    let (drawn, fetched) = frame(&mut lines, 0..3, 2);
    assert_eq!(drawn, vec!["zero, joined", "two", ""]);
    assert!(fetched.is_empty(), "{:?}", fetched);

    // What upstream's resize did to them: every row the frame reads fetched again
    all_stale(&mut lines, Instant::now());
    let (_, fetched) = frame(&mut lines, 0..3, 2);
    assert_eq!(fetched, vec![0, 1, 2]);
}

#[test]
fn a_tab_out_of_view_shown_before_its_push_draws_what_it_holds_and_the_push_puts_it_right() {
    // Resized while out of view, and shown before the push its resize asked for is applied: the
    // frame draws the rows as held, which the window has no newer word on, and fetches none; the
    // push then brings the rows the rewrap changed, and the next frame draws them. Upstream's
    // stale rows drew the same lines while they were fetched again
    let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
    screen(&mut lines, &["zero", "one, a long line", "two"]);
    let (drawn, fetched) = frame(&mut lines, 0..3, 1);
    assert_eq!(drawn, vec!["zero", "one, a long line", "two"]);
    assert!(fetched.is_empty());
    let fetch = push(&mut lines, 1..4, &[(1, "one, a"), (2, "long line"), (3, "two")], 0);
    assert!(fetch.is_empty());
    let (drawn, fetched) = frame(&mut lines, 0..4, 2);
    assert_eq!(drawn, vec!["zero", "one, a", "long line", "two"]);
    assert!(fetched.is_empty());
}

#[test]
fn a_fetch_answered_with_the_line_before_the_rewrap_is_named_by_the_push_after_it() {
    // A row fetched before the resize, its answer (the line as it was before the rewrap) put once
    // the window had resized: the push after the rewrap names the row, so it's fetched again, the
    // answer's line drawn meanwhile
    let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
    lines.put(0, LineEntry::Line(line("before the rewrap")));
    let fetch = push(&mut lines, 0..1, &[], 0);
    assert_eq!(fetch, vec![0]);
    assert_eq!(text(lines.peek(&0).unwrap()), ("LineAndFetching", "before the rewrap".to_string()));
}

#[test]
fn the_row_the_cursor_leaves_is_fetched_only_when_a_guess_was_drawn_into_it() {
    // A resize's rewrap moved the cursor from row 11 to row 12 and left row 11 as it was: the push
    // names row 12 (its line with it) and not row 11, which the window holds as the server has it
    let mut lines = LruCache::new(NonZeroUsize::new(64).unwrap());
    screen(&mut lines, &["zero", "one"]);
    let mut dirty = RangeSet::new();
    cursor_moved(&mut dirty, &RangeSet::new(), 0, 1);
    assert_eq!(dirty.iter_values().collect::<Vec<_>>(), vec![1], "the row moved to only");
    lines.put(1, LineEntry::Line(pushed("one, the cursor's")));
    dirty.remove(1);
    assert!(rows_named(&mut lines, &dirty, 0, Instant::now()).is_empty(), "nothing fetched");

    // Local echo drew a guess into row 0 (a slow server): the row left is the push's to put right
    let mut predicted = RangeSet::new();
    predicted.add(0);
    let mut dirty = RangeSet::new();
    cursor_moved(&mut dirty, &predicted, 0, 1);
    assert_eq!(dirty.iter_values().collect::<Vec<_>>(), vec![0, 1]);
    dirty.remove(1);
    let fetch = rows_named(&mut lines, &dirty, 0, Instant::now());
    assert_eq!(fetch.iter_values().collect::<Vec<_>>(), vec![0], "the guessed row fetched");
}

#[test]
fn a_push_carrying_the_cursors_row_as_it_was_brings_no_new_text() {
    // The server carries the cursor's row in every push, a title's alone too: the line again
    let mut lines = LruCache::new(NonZeroUsize::new(8).unwrap());
    lines.put(3, LineEntry::Line(line("$ claude")));
    assert!(!brings_new_text(&lines, &[(3, line("$ claude"))]));
    // Its colours changed and not its text: the same to a scan for links
    let mut red = CellAttributes::default();
    red.set_foreground(AnsiColor::Red);
    assert!(!brings_new_text(&lines, &[(3, Line::from_text("$ claude", &red, 2, None))]));
    // A line held while it's fetched again is held
    lines.put(4, LineEntry::LineAndFetching(line("four"), Instant::now()));
    assert!(!brings_new_text(&lines, &[(4, line("four"))]));
}

#[test]
fn a_push_with_a_new_line_or_a_row_not_held_brings_new_text() {
    let mut lines = LruCache::new(NonZeroUsize::new(8).unwrap());
    lines.put(3, LineEntry::Line(line("$ claude")));
    lines.put(5, LineEntry::Stale(line("five")));
    lines.put(6, LineEntry::Fetching(Instant::now()));
    assert!(brings_new_text(&lines, &[(3, line("$ claude --resume"))]));
    assert!(brings_new_text(&lines, &[(3, line("$ claude")), (4, line("new"))]), "row 4 isn't held");
    assert!(brings_new_text(&lines, &[(5, line("five"))]), "a stale row is news");
    assert!(brings_new_text(&lines, &[(6, line("six"))]), "a row being fetched with no line");
    assert!(!brings_new_text(&lines, &[]));
}

#[test]
fn a_hold_said_ends_at_the_first_push_past_it_and_not_before() {
    // PaneHeld at the push of seqno 40, which the window has applied: held
    let held = AtomicU64::new(0);
    let ended = AtomicU64::new(0);
    assert!(lbterm_hold_from(&held, 40, 40));
    assert_ne!(held.load(Ordering::Acquire), 0);
    // A push of the title alone during the hold carries the seqno it was held from again
    lbterm_hold_ends(&held, &ended, 40);
    assert_ne!(held.load(Ordering::Acquire), 0, "a title alone ends no hold");
    assert_eq!(ended.load(Ordering::Acquire), 0);
    // The push that ends it: everything since, past the seqno said
    lbterm_hold_ends(&held, &ended, 57);
    assert_eq!(held.load(Ordering::Acquire), 0);
    assert_eq!(ended.load(Ordering::Acquire), 1, "counted, for hidden_scan's scan at once");
    // Pushes after it end nothing more
    lbterm_hold_ends(&held, &ended, 60);
    assert_eq!(ended.load(Ordering::Acquire), 1);
}

#[test]
fn a_hold_said_after_the_push_past_it_was_applied_holds_nothing() {
    // The client thread merges a pane's pushes into the first one waiting (lbterm_backlog.rs),
    // which can be ahead of the word: the push past it is applied first
    let held = AtomicU64::new(0);
    assert!(!lbterm_hold_from(&held, 57, 40));
    assert_eq!(held.load(Ordering::Acquire), 0);
    // A seqno of 0 (a pane's first push) is held from too, not taken for "not held"
    assert!(lbterm_hold_from(&held, 0, 0));
    assert_ne!(held.load(Ordering::Acquire), 0);
    // And a push applied out of order (one whose images were fetched first) before the push said
    // leaves it held until the one past it
    let ended = AtomicU64::new(0);
    let held = AtomicU64::new(0);
    assert!(lbterm_hold_from(&held, 30, 40));
    lbterm_hold_ends(&held, &ended, 40);
    assert_ne!(held.load(Ordering::Acquire), 0);
    lbterm_hold_ends(&held, &ended, 41);
    assert_eq!(held.load(Ordering::Acquire), 0);
}

/// A reconnect keeps a pane held by the old connection held, from no push, so the new
/// connection's first push of it ends the hold whatever its seqno (a server started afresh numbers
/// from its own start), and says it needs that push asked; a pane not held stays as it is
#[test]
fn a_reconnect_holds_a_held_pane_until_the_new_connections_first_push() {
    let held = AtomicU64::new(0);
    let ended = AtomicU64::new(0);
    assert!(!lbterm_hold_reconnected(&held), "not held: nothing to ask");
    assert_eq!(held.load(Ordering::Acquire), 0);

    assert!(lbterm_hold_from(&held, 40, 40));
    assert!(lbterm_hold_reconnected(&held), "held: its push is asked");
    assert_ne!(held.load(Ordering::Acquire), 0, "and it's still held");
    lbterm_hold_ends(&held, &ended, 3);
    assert_eq!(held.load(Ordering::Acquire), 0, "a push of a low seqno ends it");
    assert_eq!(ended.load(Ordering::Acquire), 1);
}

/// The test's mux, set once for every test here that applies a push the window hears of (the
/// apply's PaneOutput), with no ssh agent, which a test has no use for
fn test_mux() -> Arc<Mux> {
    static MUX: std::sync::OnceLock<Arc<Mux>> = std::sync::OnceLock::new();
    Arc::clone(MUX.get_or_init(|| {
        let mut config = config::Config::default_config();
        config.mux_enable_ssh_agent = false;
        config::use_this_configuration(config);
        let mux = Arc::new(Mux::new(None));
        Mux::set_mux(&mux);
        mux
    }))
}

/// A client pane's renderable on a client with no connection, titled `◐ claude` at seqno 5, and
/// how many PaneOutput notices the pane has had
fn names_pane(local_pane_id: PaneId) -> (RenderableInner, Arc<AtomicU64>) {
    let mux = test_mux();
    let outputs = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&outputs);
    mux.subscribe(move |n| {
        if matches!(n, mux::MuxNotification::PaneOutput(p) if p == local_pane_id) {
            counted.fetch_add(1, Ordering::AcqRel);
        }
        true
    });
    let client = Arc::new(ClientInner::new(
        0,
        crate::client::Client::lbterm_unconnected(),
        None,
        false,
    ));
    let mut inner = RenderableInner::new(
        &client,
        local_pane_id + 1000,
        local_pane_id,
        RenderableDimensions {
            cols: 80,
            viewport_rows: 24,
            ..Default::default()
        },
        "◐ claude",
        RateLimiter::new(|config| config.ratelimit_mux_line_prefetches_per_second),
    );
    inner.seqno = 5;
    (inner, outputs)
}

fn names_push(pane: &RenderableInner, seqno: SequenceNo, title: &str) -> GetPaneRenderChangesResponse {
    GetPaneRenderChangesResponse {
        pane_id: pane.remote_pane_id,
        mouse_grabbed: false,
        alt_screen_active: false,
        cursor_position: pane.cursor_position,
        dimensions: pane.dimensions,
        dirty_lines: vec![],
        title: title.to_string(),
        working_dir: Some(Url::parse("file:///C:/work/").unwrap().into()),
        bonus_lines: vec![].into(),
        input_serial: None,
        seqno,
    }
}

/// A tab out of view's title (◐ to ◑), applied on the client thread: read back from where
/// get_title and get_current_working_dir read, its word said as every push's is, and nothing for
/// the window to hear of
#[test]
fn a_names_push_applied_here_is_read_back_and_said_with_no_notice() {
    let (mut pane, outputs) = names_pane(9101);
    let push = names_push(&pane, 5, "◑ claude");
    assert!(crate::lbterm_names::alone(&push));
    assert!(pane.lbterm_names_here(&push, true));
    assert_eq!(pane.title, "◑ claude", "what get_title reads");
    assert_eq!(
        pane.working_dir.as_ref().map(|u| u.as_str()),
        Some("file:///C:/work/"),
        "what get_current_working_dir reads"
    );
    assert_eq!(pane.lbterm_drawn.take(), Some(5), "the next frame that reads the pane says it");
    assert_eq!(outputs.load(Ordering::Acquire), 0, "no PaneOutput");

    // The server's read of a pane that printed nothing a push compares since: its seqno moves on
    let push = names_push(&pane, 8, "◐ claude");
    assert!(pane.lbterm_names_here(&push, true));
    assert_eq!((pane.title.as_str(), pane.seqno), ("◐ claude", 8));
    assert_eq!(pane.lbterm_drawn.take(), Some(8));
    assert_eq!(outputs.load(Ordering::Acquire), 0);
}

/// One that would change more than its names goes to the GUI thread, as any push, and leaves
/// the pane as it was
#[test]
fn a_push_that_changes_more_than_its_names_is_left_to_the_gui_thread() {
    let (mut pane, outputs) = names_pane(9102);
    let mut moved = names_push(&pane, 5, "◑ claude");
    moved.cursor_position.y += 1;
    assert!(!pane.lbterm_names_here(&moved, true), "the cursor moved");
    assert!(!pane.lbterm_names_here(&names_push(&pane, 5, "◑ claude"), false), "a flag changed");
    assert!(!pane.lbterm_names_here(&names_push(&pane, 4, "◑ claude"), true), "an older seqno");
    assert!(lbterm_hold_from(&pane.lbterm_held, pane.seqno, 5));
    assert!(!pane.lbterm_names_here(&names_push(&pane, 6, "◑ claude"), true), "it ends the hold");
    assert_eq!(pane.title, "◐ claude", "nothing applied");
    assert_eq!(pane.seqno, 5);
    assert!(!pane.lbterm_drawn.unsaid());
    assert_eq!(outputs.load(Ordering::Acquire), 0);
}

/// A push of the pane's lines after the names push applied here is applied after it, as it came:
/// its title, folder and seqno are the pane's, and the window hears of it
#[test]
fn a_lines_push_after_a_names_push_applied_here_is_applied_in_order() {
    let (mut pane, outputs) = names_pane(9103);
    assert!(pane.lbterm_names_here(&names_push(&pane, 5, "◑ claude"), true));

    let mut lines = names_push(&pane, 7, "◐ claude");
    lines.working_dir = Some(Url::parse("file:///C:/work/sub/").unwrap().into());
    lines.dirty_lines = vec![0..1];
    let carried = vec![(0, line("printed"))];
    lines.bonus_lines = carried.clone().into();
    assert!(!crate::lbterm_names::alone(&lines), "a push of lines goes to the GUI thread");
    pane.apply_changes_to_surface(lines, carried, false);
    assert_eq!(pane.title, "◐ claude");
    assert_eq!(pane.working_dir.as_ref().map(|u| u.as_str()), Some("file:///C:/work/sub/"));
    assert_eq!(pane.seqno, 7);
    assert_eq!(pane.lbterm_drawn.take(), Some(7), "the newest push said");
    assert_eq!(outputs.load(Ordering::Acquire), 1, "its PaneOutput");
    assert!(matches!(pane.lines.peek(&0), Some(LineEntry::Line(l)) if l.as_str().trim_end() == "printed"));
}
