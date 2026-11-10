//! LBTerm: a connection's pushes of a pane out of its client's view, as the round sends them
//! (sessionhandler.rs's `lbterm_push_pane`, lbterm_flow.rs's `Pending`; lbterm/MUX.md, "Tabs out
//! of view: output held at the server"), and of a pane in view whose client says what its frames
//! read ("A shown pane's pace"), against a pane with a real terminal, so its seqnos and the rows
//! it says changed are the terminal's own.

use super::*;
use crate::lbterm_flow::{Outbox, Pending};
use mux::domain::DomainId;
use mux::pane::{ForEachPaneLogicalLine, LogicalLine, WithPaneLines};
use mux::renderable::{
    terminal_for_each_logical_line_in_stable_range_mut, terminal_get_cursor_position,
    terminal_get_dimensions, terminal_get_dirty_lines, terminal_get_lines,
    terminal_with_lines_mut,
};
use parking_lot::{MappedMutexGuard, MutexGuard};
use rangeset::RangeSet;
use std::ops::Range;
use termwiz::surface::Line;
use wezterm_term::{KeyCode, KeyModifiers, MouseEvent, Terminal, TerminalSize};

/// A pane that's a terminal and nothing else: what's printed to it goes straight in
struct TermPane {
    id: PaneId,
    term: parking_lot::Mutex<Terminal>,
    input: parking_lot::Mutex<Vec<u8>>,
}

impl TermPane {
    fn new(id: PaneId) -> Arc<TermPane> {
        config::use_test_configuration();
        Arc::new(TermPane {
            id,
            term: parking_lot::Mutex::new(Terminal::new(
                TerminalSize {
                    rows: 5,
                    cols: 20,
                    ..Default::default()
                },
                Arc::new(config::TermConfig::new()),
                "LBTerm",
                "test",
                Box::new(Vec::new()),
            )),
            input: parking_lot::Mutex::new(vec![]),
        })
    }

    fn print(&self, text: &str) {
        self.term.lock().advance_bytes(text);
    }
}

impl Pane for TermPane {
    fn pane_id(&self) -> PaneId {
        self.id
    }
    fn get_cursor_position(&self) -> StableCursorPosition {
        terminal_get_cursor_position(&mut self.term.lock())
    }
    fn get_current_seqno(&self) -> SequenceNo {
        self.term.lock().current_seqno()
    }
    fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        terminal_get_dirty_lines(&mut self.term.lock(), lines, seqno)
    }
    fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        terminal_get_lines(&mut self.term.lock(), lines)
    }
    fn with_lines_mut(&self, lines: Range<StableRowIndex>, with_lines: &mut dyn WithPaneLines) {
        terminal_with_lines_mut(&mut self.term.lock(), lines, with_lines)
    }
    fn for_each_logical_line_in_stable_range_mut(
        &self,
        lines: Range<StableRowIndex>,
        for_line: &mut dyn ForEachPaneLogicalLine,
    ) {
        terminal_for_each_logical_line_in_stable_range_mut(&mut self.term.lock(), lines, for_line)
    }
    fn get_logical_lines(&self, lines: Range<StableRowIndex>) -> Vec<LogicalLine> {
        mux::pane::impl_get_logical_lines_via_get_lines(self, lines)
    }
    fn get_dimensions(&self) -> RenderableDimensions {
        terminal_get_dimensions(&mut self.term.lock())
    }
    fn get_title(&self) -> String {
        self.term.lock().get_title().to_string()
    }
    fn send_paste(&self, _: &str) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn reader(&self) -> anyhow::Result<Option<Box<dyn std::io::Read + Send>>> {
        Ok(None)
    }
    fn writer(&self) -> MappedMutexGuard<'_, dyn std::io::Write> {
        MutexGuard::map(self.input.lock(), |w| w as &mut dyn std::io::Write)
    }
    fn resize(&self, _: TerminalSize) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn key_down(&self, _: KeyCode, _: KeyModifiers) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn key_up(&self, _: KeyCode, _: KeyModifiers) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn mouse_event(&self, _: MouseEvent) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn is_dead(&self) -> bool {
        false
    }
    fn palette(&self) -> ColorPalette {
        self.term.lock().palette()
    }
    fn domain_id(&self) -> DomainId {
        0
    }
    fn is_mouse_grabbed(&self) -> bool {
        false
    }
    fn is_alt_screen_active(&self) -> bool {
        false
    }
    fn get_current_working_dir(&self, _: mux::pane::CachePolicy) -> Option<Url> {
        self.term.lock().get_current_dir().cloned()
    }
}

/// A connection whose client shows pane 1 alone, and pane 2, a terminal, which the connection
/// has pushed once (its first push, whatever is shown), with the seqno that push carried
fn pushed_once() -> (SessionHandler, Arc<Mutex<Pending>>, Arc<TermPane>, Arc<dyn Pane>, SequenceNo) {
    let mut handler = SessionHandler::new(PduSender::new(|_| Ok(())));
    let pending = handler.lbterm_pending();
    pending.lock().unwrap().set_shown(&[ShownPane { pane_id: 1, drawn: None }]);
    let term = TermPane::new(2);
    let pane: Arc<dyn Pane> = term.clone();
    term.print("one\r\ntwo\r\n");
    assert!(pending.lock().unwrap().output(2));
    let first = round(&mut handler, &pending, &pane);
    let seqno = only_push(&first).seqno;
    assert_eq!(seqno, pane.get_current_seqno(), "the first push goes whole");
    (handler, pending, term, pane, seqno)
}

/// A round of what the connection owes, each taken as `fill` takes it and pushed as
/// `lbterm_push` pushes it: the PDUs it sends
fn round(handler: &mut SessionHandler, pending: &Mutex<Pending>, pane: &Arc<dyn Pane>) -> Vec<Pdu> {
    let mut out = Outbox::default();
    loop {
        let next = pending.lock().unwrap().take_next(None);
        match next {
            Some((pane_id, owed)) => {
                assert_eq!(pane_id, pane.pane_id());
                handler.lbterm_push_pane(pane, owed, &mut out).unwrap();
            }
            None => break,
        }
    }
    out.take_pdus()
}

/// The pushes among `pdus` (GetPaneRenderChangesResponse), in order
fn pushes(pdus: &[Pdu]) -> Vec<&GetPaneRenderChangesResponse> {
    pdus.iter()
        .filter_map(|pdu| match pdu {
            Pdu::GetPaneRenderChangesResponse(resp) => Some(resp),
            _ => None,
        })
        .collect()
}

fn only_push(pdus: &[Pdu]) -> &GetPaneRenderChangesResponse {
    let pushes = pushes(pdus);
    assert_eq!(pushes.len(), 1, "{:?}", pdus);
    pushes[0]
}

/// The PaneHeld among `pdus`
fn helds(pdus: &[Pdu]) -> Vec<PaneHeld> {
    pdus.iter()
        .filter_map(|pdu| match pdu {
            Pdu::PaneHeld(held) => Some(*held),
            _ => None,
        })
        .collect()
}

/// The one push among `pdus`, taken out
fn take_push(pdus: Vec<Pdu>) -> GetPaneRenderChangesResponse {
    let mut pushes: Vec<GetPaneRenderChangesResponse> = pdus
        .into_iter()
        .filter_map(|pdu| match pdu {
            Pdu::GetPaneRenderChangesResponse(resp) => Some(resp),
            _ => None,
        })
        .collect();
    assert_eq!(pushes.len(), 1);
    pushes.remove(0)
}

/// The text of the lines a push carried, by row
fn texts(resp: GetPaneRenderChangesResponse) -> Vec<(StableRowIndex, String)> {
    let (lines, _) = resp.bonus_lines.extract_data();
    lines
        .into_iter()
        .map(|(row, line)| (row, line.as_str().trim_end().to_string()))
        .collect()
}

/// Out of view, a pane that prints has no push worked out: the client is told once that its
/// lines are held as of its last push, and once shown it's pushed everything since in one push
#[test]
fn a_held_pane_is_told_once_and_shown_gets_everything_since_in_one_push() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    for i in 0..12 {
        term.print(&format!("line {i}\r\n"));
        pending.lock().unwrap().output(2);
    }
    let told = round(&mut handler, &pending, &pane);
    assert_eq!(helds(&told), vec![PaneHeld { pane_id: 2, seqno: first }], "{:?}", told);
    assert!(pushes(&told).is_empty(), "no push of a held pane: {:?}", told);
    for i in 12..20 {
        term.print(&format!("line {i}\r\n"));
        pending.lock().unwrap().output(2);
    }
    assert!(!pending.lock().unwrap().owes(), "told once a hold");
    assert!(round(&mut handler, &pending, &pane).is_empty());

    // Shown: one push, past the seqno it was told, with the screen as the terminal has it and
    // every row printed since named for the client to make stale
    pending.lock().unwrap().set_shown(&[ShownPane { pane_id: 2, drawn: None }]);
    let shown = round(&mut handler, &pending, &pane);
    assert!(helds(&shown).is_empty());
    let push = take_push(shown);
    assert_eq!(push.seqno, pane.get_current_seqno());
    assert!(push.seqno > first);
    let mut named = RangeSet::new();
    for rows in &push.dirty_lines {
        named.add_range(rows.clone());
    }
    let top = push.dimensions.physical_top;
    assert!(named.contains(2) && named.contains(top - 1), "the rows scrolled past: {:?}", named);
    let rows: Vec<String> = texts(push).into_iter().map(|(_, t)| t).collect();
    assert!(rows.contains(&"line 19".to_string()), "{:?}", rows);
    assert!(!pending.lock().unwrap().holds(2));
}

/// A held pane's title goes at once, in a push of its title alone: no lines, no rows, and the
/// cursor, size and seqno its last push carried, ahead of the alert, which reads it
#[test]
fn a_held_panes_title_goes_alone_with_what_its_last_push_carried() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    let before = pane.get_cursor_position();
    term.print("\x1b]0;working\x07more\r\n");
    {
        let mut pending = pending.lock().unwrap();
        pending.output(2);
        pending.alert(2, Alert::WindowTitleChanged("working".to_string()));
    }
    let sent = round(&mut handler, &pending, &pane);
    assert!(matches!(sent[0], Pdu::PaneHeld(PaneHeld { pane_id: 2, seqno }) if seqno == first));
    assert!(matches!(sent[1], Pdu::GetPaneRenderChangesResponse(_)), "{:?}", sent);
    assert!(
        matches!(sent.last(), Some(Pdu::NotifyAlert(NotifyAlert { alert: Alert::WindowTitleChanged(_), .. }))),
        "the alert after the push it reads: {:?}",
        sent
    );
    let push = take_push(sent);
    assert_eq!(push.title, "working");
    assert_eq!(push.seqno, first, "the hold's seqno, which doesn't end it");
    assert_eq!(push.cursor_position, before, "as last pushed, not where it is now");
    assert!(push.dirty_lines.is_empty() && texts(push).is_empty(), "no lines");
    assert!(pending.lock().unwrap().holds(2));
    // The same title again is no push
    pending.lock().unwrap().alert(2, Alert::WindowTitleChanged("working".to_string()));
    assert!(pushes(&round(&mut handler, &pending, &pane)).is_empty());
}

/// A held pane that scrolls and sets its title, block after block (mux-flood's floods, a program
/// that says its progress in its title): each title goes alone, the hold kept, its word said once.
/// The names push once compared the whole dimensions with the last push's, whose screen top and
/// scrollback move with every line scrolled, so each title was a real push that ended the hold,
/// and the next output a new PaneHeld (PITFALLS 242: 3,404 of them in 15 s of mux-flood)
#[test]
fn a_held_panes_titles_go_alone_while_it_scrolls() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    let before = pane.get_dimensions();
    for n in 1..=3 {
        let title = format!("block {n}");
        // Twelve lines on a five-row screen: it scrolls
        term.print(&format!("\x1b]0;{title}\x07{}", "a line\r\n".repeat(12)));
        assert_ne!(pane.get_dimensions().physical_top, before.physical_top);
        {
            let mut pending = pending.lock().unwrap();
            pending.output(2);
            pending.alert(2, Alert::WindowTitleChanged(title.clone()));
        }
        let sent = round(&mut handler, &pending, &pane);
        assert_eq!(helds(&sent).len(), if n == 1 { 1 } else { 0 }, "the word once: {:?}", sent);
        let push = take_push(sent);
        assert_eq!(push.title, title);
        assert_eq!(push.seqno, first, "the hold's seqno, which doesn't end it");
        assert_eq!(push.dimensions, before, "as last pushed");
        assert!(push.dirty_lines.is_empty(), "no rows named");
        assert!(texts(push).is_empty(), "no lines");
        assert!(pending.lock().unwrap().holds(2), "block {n} kept the hold");
    }
}

#[test]
fn lbterm_same_size_is_the_size_alone() {
    let a = RenderableDimensions {
        cols: 80,
        viewport_rows: 24,
        scrollback_rows: 100,
        physical_top: 76,
        scrollback_top: 0,
        dpi: 96,
        pixel_width: 800,
        pixel_height: 480,
        reverse_video: false,
    };
    let scrolled = RenderableDimensions {
        scrollback_rows: 5000,
        physical_top: 4976,
        scrollback_top: 10,
        reverse_video: true,
        ..a
    };
    assert!(lbterm_same_size(&a, &scrolled));
    assert!(!lbterm_same_size(&a, &RenderableDimensions { cols: 81, ..a }));
    assert!(!lbterm_same_size(&a, &RenderableDimensions { viewport_rows: 30, ..a }));
    assert!(!lbterm_same_size(&a, &RenderableDimensions { pixel_height: 600, ..a }));
    assert!(!lbterm_same_size(&a, &RenderableDimensions { dpi: 144, ..a }));
}

/// A hold whose output the last push carried already (its notice came after that push) holds
/// nothing: no word of it, and the pane's next output starts a hold of its own
#[test]
fn a_hold_of_output_already_pushed_holds_nothing() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    pending.lock().unwrap().output(2);
    let sent = round(&mut handler, &pending, &pane);
    assert!(sent.is_empty(), "{:?}", sent);
    assert!(!pending.lock().unwrap().holds(2));
    term.print("three\r\n");
    pending.lock().unwrap().output(2);
    assert_eq!(
        helds(&round(&mut handler, &pending, &pane)),
        vec![PaneHeld { pane_id: 2, seqno: first }]
    );
}

/// A push sent outside the rounds (the window's poll of a held pane, a key's answer) after the
/// client was told the pane's lines are held goes whatever it finds, so the client knows its
/// copy is current, and ends the hold: the pane's next output is told of again
#[test]
fn a_poll_of_a_held_pane_is_answered_with_its_push_and_ends_the_hold() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    term.print("three\r\n");
    pending.lock().unwrap().output(2);
    assert_eq!(helds(&round(&mut handler, &pending, &pane)).len(), 1);

    let sent = Arc::new(Mutex::new(vec![]));
    let poll = |handler: &mut SessionHandler| {
        let to = Arc::clone(&sent);
        maybe_push_pane_changes(
            &pane,
            PduSender::new(move |pdu| {
                to.lock().unwrap().push(pdu.pdu);
                Ok(())
            }),
            handler.per_pane(2),
        )
        .unwrap();
        std::mem::take(&mut *sent.lock().unwrap())
    };
    let answer = poll(&mut handler);
    let push = only_push(&answer);
    assert!(push.seqno > first);
    assert!(!pending.lock().unwrap().holds(2), "the poll's push ended it");

    // Told of a hold whose pane then printed nothing it shows: its push still goes
    term.print("four\r\n");
    pending.lock().unwrap().output(2);
    let told = round(&mut handler, &pending, &pane);
    let at = helds(&told)[0].seqno;
    assert_eq!(at, push.seqno);
    handler.per_pane(2).lock().unwrap().seqno = pane.get_current_seqno();
    handler.per_pane(2).lock().unwrap().cursor_position = pane.get_cursor_position();
    let forced = poll(&mut handler);
    assert_eq!(pushes(&forced).len(), 1, "sent though nothing changed: {:?}", forced);
    assert!(pushes(&poll(&mut handler)).is_empty(), "and only that once");
}

/// Output out of view that moves the pane's seqno and changes nothing a push carries (ConPTY's
/// title sent again as each program ends, a DECSET 2026 batch) holds nothing: no word of a hold
/// and no push, the next push starting from the read's seqno. The pane's next output that does
/// change something is told of once, and shown it's pushed once (PITFALLS 197)
#[test]
fn output_that_changes_nothing_a_push_carries_holds_nothing() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    term.print("\x1b[0m");
    assert!(pane.get_current_seqno() > first, "the seqno moved");
    pending.lock().unwrap().output(2);
    let sent = round(&mut handler, &pending, &pane);
    assert!(sent.is_empty(), "no word and no push: {:?}", sent);
    assert!(!pending.lock().unwrap().holds(2));
    assert!(!pending.lock().unwrap().owes());
    let quiet = pane.get_current_seqno();

    term.print("three\r\n");
    pending.lock().unwrap().output(2);
    assert_eq!(
        helds(&round(&mut handler, &pending, &pane)),
        vec![PaneHeld { pane_id: 2, seqno: quiet }],
        "told once, from the seqno the quiet read took"
    );
    pending.lock().unwrap().set_shown(&[ShownPane { pane_id: 2, drawn: None }]);
    let shown = round(&mut handler, &pending, &pane);
    let push = only_push(&shown);
    assert!(push.seqno > quiet);
    assert!(pushes(&round(&mut handler, &pending, &pane)).is_empty(), "and only once");
}

/// A held pane's title out of view after its size changed (a resize answered, the push it asks
/// still to come) goes as a real push, not one of its title alone, which would carry the size
/// its client was last sent and give it that back; the real push ends the hold
#[test]
fn a_held_panes_title_after_a_resize_goes_as_a_real_push() {
    let (mut handler, pending, term, pane, first) = pushed_once();
    term.term.lock().resize(TerminalSize {
        rows: 7,
        cols: 30,
        ..Default::default()
    });
    term.print("\x1b]2;resized\x07");
    {
        let mut pending = pending.lock().unwrap();
        pending.output(2);
        pending.alert(2, Alert::WindowTitleChanged("resized".to_string()));
    }
    let sent = round(&mut handler, &pending, &pane);
    assert!(helds(&sent).is_empty(), "{:?}", sent);
    let push = only_push(&sent);
    assert_eq!(push.title, "resized");
    assert_eq!(push.dimensions.cols, 30);
    assert_eq!(push.dimensions.viewport_rows, 7);
    assert!(push.seqno > first);
    assert!(!pending.lock().unwrap().holds(2), "the real push ended the hold");
}

// A shown pane's pace (lbterm/MUX.md, "A shown pane's pace"; FLOWS.md, pane output, What could
// go 12): the same pushes, of a pane the client shows, on a connection whose client says which
// push of it its frames have read (codec's PanesDrawn)

/// A connection whose client shows pane 2, a terminal, pushed once, and has said a frame read
/// that push: from here its round pushes of pane 2 wait for the word that a frame read the last
fn paced() -> (SessionHandler, Arc<Mutex<Pending>>, Arc<TermPane>, Arc<dyn Pane>, SequenceNo) {
    let mut handler = SessionHandler::new(PduSender::new(|_| Ok(())));
    let pending = handler.lbterm_pending();
    pending.lock().unwrap().set_shown(&[ShownPane { pane_id: 2, drawn: None }]);
    let term = TermPane::new(2);
    let pane: Arc<dyn Pane> = term.clone();
    term.print("one\r\n");
    pending.lock().unwrap().output(2);
    let first = only_push(&round(&mut handler, &pending, &pane)).seqno;
    handler.lbterm_drawn(&[PaneDrawn { pane_id: 2, seqno: first }]);
    (handler, pending, term, pane, first)
}

/// Prints `text` to the pane and owes the connection its output, as the pane's PaneOutput does
fn prints(term: &TermPane, pending: &Mutex<Pending>, text: &str) {
    term.print(text);
    pending.lock().unwrap().output(term.id);
}

/// A shown pane's next push waits for the frame's word that it read the last: nothing goes
/// meanwhile however much it prints, a word for an older push ends nothing, and the push after
/// the word carries everything since, at the pane's seqno now
#[test]
fn a_shown_panes_push_waits_for_the_frames_word_then_carries_everything_since() {
    let (mut handler, pending, term, pane, first) = paced();
    prints(&term, &pending, "two\r\n");
    let second = only_push(&round(&mut handler, &pending, &pane)).seqno;
    assert!(second > first);
    for i in 0..6 {
        prints(&term, &pending, &format!("line {i}\r\n"));
        assert!(round(&mut handler, &pending, &pane).is_empty(), "no push before the word");
    }
    handler.lbterm_drawn(&[PaneDrawn { pane_id: 2, seqno: first }]);
    assert!(round(&mut handler, &pending, &pane).is_empty(), "the word for an older push");

    handler.lbterm_drawn(&[PaneDrawn { pane_id: 2, seqno: second }]);
    let push = take_push(round(&mut handler, &pending, &pane));
    assert_eq!(push.seqno, pane.get_current_seqno());
    let rows: Vec<String> = texts(push).into_iter().map(|(_, t)| t).collect();
    assert!(rows.contains(&"line 5".to_string()), "{:?}", rows);
    assert!(rows.contains(&"line 2".to_string()), "the screen as it is: {:?}", rows);
    assert!(round(&mut handler, &pending, &pane).is_empty(), "one push for it all");
}

/// A key's answer, its forced push, goes while the pane waits for a frame, and neither ends the
/// wait nor starts one; the key makes the pane the one typed into, and from then its echo goes
/// as it comes, though no frame has read the push before it, until a frame reads a push made
/// since the key
#[test]
fn a_keys_forced_push_and_its_echo_wait_for_no_frame() {
    let (mut handler, pending, term, pane, _) = paced();
    prints(&term, &pending, "two\r\n");
    let second = only_push(&round(&mut handler, &pending, &pane)).seqno;
    prints(&term, &pending, "busy\r\n");
    assert!(round(&mut handler, &pending, &pane).is_empty());

    let answer = handler
        .per_pane(2)
        .lock()
        .unwrap()
        .compute_changes(&pane, Some(InputSerial::empty()));
    assert_eq!(answer.map(|push| push.seqno), Some(pane.get_current_seqno()));
    assert!(round(&mut handler, &pending, &pane).is_empty(), "the pane still waits");

    // SendKeyDown's arm: the pane typed into, then the program's echo
    handler.lbterm_input(2);
    prints(&term, &pending, "k");
    let echo = take_push(round(&mut handler, &pending, &pane));
    assert_eq!(echo.seqno, pane.get_current_seqno());
    assert!(texts(echo).iter().any(|(_, t)| t == "k"), "the echo's row");
    // The word for the push before the echo ends nothing; the echo's next part goes too
    handler.lbterm_drawn(&[PaneDrawn { pane_id: 2, seqno: second }]);
    prints(&term, &pending, "j");
    let more = take_push(round(&mut handler, &pending, &pane));
    // The word for a push made since the key: paced again from its next
    handler.lbterm_drawn(&[PaneDrawn { pane_id: 2, seqno: more.seqno }]);
    prints(&term, &pending, "x");
    assert_eq!(pushes(&round(&mut handler, &pending, &pane)).len(), 1);
    prints(&term, &pending, "y");
    assert!(round(&mut handler, &pending, &pane).is_empty(), "waits for the frame again");
}

/// A connection whose client says what it shows but not what its frames read (this branch's
/// window before the pace, or one that never paints) is pushed every change as it comes
#[test]
fn a_connection_whose_client_never_says_what_its_frames_read_pushes_every_change() {
    let mut handler = SessionHandler::new(PduSender::new(|_| Ok(())));
    let pending = handler.lbterm_pending();
    pending.lock().unwrap().set_shown(&[ShownPane { pane_id: 2, drawn: None }]);
    let term = TermPane::new(2);
    let pane: Arc<dyn Pane> = term.clone();
    let mut last = 0;
    for i in 0..6 {
        prints(&term, &pending, &format!("line {i}\r\n"));
        let seqno = only_push(&round(&mut handler, &pending, &pane)).seqno;
        assert!(seqno > last);
        last = seqno;
    }
}
