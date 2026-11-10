//! LBTerm: what a connection's client thread has handed the GUI thread and it hasn't applied yet.
//!
//! Upstream's client thread read each PDU the server sent as soon as it came and handed every
//! unilateral one (a pane's changes, an alert, a title) to the GUI thread as a task of its own,
//! however far behind that thread was. With six tabs printing without a pause on a mux domain,
//! measured on 2026-10-30, a window's client thread decoded about 2,400 PDUs a second and its GUI
//! thread applied about 800: 12,682 were waiting after 15 s and the window held 2 GB.
//!
//! Now they go in one queue, in the order they came, which the GUI thread takes whole, one task
//! at a time, handing each on as upstream did, so they're applied in the same order; except that
//! a pane's pushes of its output among them are merged into one as they're handed over, on the
//! client thread, where the first of them was ([`Merger`]): each says what changed since the one
//! before, so applying the lines the last of them has, with every row any of them said changed,
//! leaves the pane as applying each in turn would, without laying out lines a later one
//! replaced, and the GUI thread applies at most one push a pane however many came. Once [`LIMIT`]
//! are handed over and not yet applied, the client thread stops reading the socket until the GUI
//! thread has applied them all, unless input waits on its answer or its echo ([`Input`]: a key's
//! echo mustn't sit behind what the server sent before it, and a GUI thread blocked on an answer,
//! as ClientPane's writer blocks it, must get it). Waiting for all of them, not for room for one
//! more, leaves the GUI thread nothing queued for a moment, so its loop goes round and paints.
//! Meanwhile the server, its socket full, works out the pushes it owes only as the window takes
//! them, each with all the pane's changes up to then (wezterm-mux-server-impl's lbterm_flow.rs),
//! so the window's output stays as current as its GUI thread can apply it.

use codec::{DecodedPdu, GetPaneRenderChangesResponse, Pdu};
use mux::pane::PaneId;
use rangeset::RangeSet;
use smol::channel::{bounded, Receiver, Sender};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wezterm_term::{Line, StableRowIndex};

/// How many PDUs handed to the GUI thread and not yet applied stop the client thread reading
/// more until they're all applied: more than a busy mux domain's queue in a frame (eight
/// Claude-like tabs push about 250 a second), and since a pane's pushes among them are merged as
/// they come, what the GUI thread applies of them is a push a pane
pub const LIMIT: usize = 32;

/// How long after the GUI thread last applied what it took before it takes more ([`Backlog::gap`]).
/// While input waits, the client thread reads on and hands over as it reads, and each take let
/// the next PDU handed over queue the next task at once: the GUI thread went from one to the next
/// without its message loop turning, so a frame waiting to paint (the echo the last one applied)
/// waited until the reading stopped, and with six tabs printing a key's echo took 93 ms at the
/// median to paint (2026-10-31). Waiting this long, the task leaves nothing queued: the loop paints
/// what's waiting, takes its messages and comes back to it. A wait this short ends on Windows' next
/// timer tick, up to 15.6 ms on: what comes meanwhile is merged into what it takes then. Only a
/// take after one that held the GUI thread [`GAP_AFTER`] or more waits for it.
pub const GAP: Duration = Duration::from_millis(1);

/// How long a take must have held the GUI thread for the next to wait out [`GAP`]: long enough
/// that the loop should turn between them. Any take within 1 ms of the last one's end waited for
/// the timer's tick, and under a busy mux domain's ordinary load that bunched the takes onto the
/// ticks: six Claude-like tabs and two printing in bursts (perf/mux-load.ps1, four runs of each
/// build in turn, 2026-10-31) had a key's echo shown in 25 ms at the median with every take
/// waiting (24 on the installed build), and 23 with none. There a take carried one PDU in 72
/// cases of 100 and at most 8, and held the GUI thread 1.1 ms at the median and 2.7 at the 90th
/// percentile; with six tabs flooding a take carried 17 at the median and held it 16.5 ms, and
/// the takes of under 16 PDUs, which a count would have let through, held it 8 to 18 ms, a frame
/// each (one run of each, logged take by take).
pub const GAP_AFTER: Duration = Duration::from_millis(4);

/// The tasks [`Backlog::hand`] asks to be queued on the GUI thread
#[derive(Debug, PartialEq, Eq)]
pub struct Handed {
    /// One to take everything ([`Backlog::take`])
    pub take_all: bool,
    /// One to take the pushes of the pane typed into ahead of the rest ([`Backlog::take_first`])
    pub take_first: bool,
}

/// The PDUs handed over and not yet taken, and how many are handed over and not yet applied
pub struct Backlog {
    state: Mutex<State>,
    unapplied: AtomicUsize,
    room_tx: Sender<()>,
    room_rx: Receiver<()>,
}

#[derive(Default)]
struct State {
    merger: Merger,
    /// A task to take them is queued on the GUI thread
    scheduled: bool,
    /// A task to take the pane typed into's is queued on the GUI thread ([`Backlog::take_first`])
    first_scheduled: bool,
    /// When the GUI thread last finished applying what it took
    applied_at: Option<Instant>,
    /// How many PDUs of each pane are handed over and not yet applied, taken or not: a push of a
    /// pane's title and folder alone is applied on the client thread only when none is
    /// ([`Backlog::waits`]), so nothing older of the pane can be applied after it
    waiting: HashMap<PaneId, usize>,
}

/// PDUs handed over (one, or a pane's pushes merged): applied when this is dropped, as what
/// applies them ends
pub struct Applied {
    backlog: Arc<Backlog>,
    count: usize,
    /// The pane they're for, if they're a pane's, whose [`State::waiting`] they leave as they're
    /// dropped, unless [`Applied::goes_on`] took it
    pane: Option<PaneId>,
}

impl Applied {
    fn new(backlog: Arc<Backlog>, decoded: &DecodedPdu, count: usize) -> Self {
        Self {
            backlog,
            count,
            pane: decoded.pdu.pane_id(),
        }
    }

    /// How many PDUs handed over this stands for
    pub fn count(&self) -> usize {
        self.count
    }

    /// What applies them goes on as a task of its own (one that waits on the server: an image's
    /// data, a resync), and they count as applied as this is dropped, so the client thread reads
    /// on for its answer; but their pane waits for them until the [`PaneWaits`] this gives is
    /// dropped, as the task ends
    pub fn goes_on(&mut self) -> PaneWaits {
        PaneWaits {
            backlog: Arc::clone(&self.backlog),
            count: self.count,
            pane: self.pane.take(),
        }
    }
}

impl Drop for Applied {
    fn drop(&mut self) {
        if self.count > 0 {
            self.backlog.applied(self.count);
        }
        if let Some(pane) = self.pane {
            self.backlog.pane_applied(pane, self.count);
        }
    }
}

/// A pane's PDUs applied by a task that went on ([`Applied::goes_on`]): its pane waits for them
/// until this is dropped
pub struct PaneWaits {
    backlog: Arc<Backlog>,
    count: usize,
    pane: Option<PaneId>,
}

impl Drop for PaneWaits {
    fn drop(&mut self) {
        if let Some(pane) = self.pane {
            self.backlog.pane_applied(pane, self.count);
        }
    }
}

impl Backlog {
    pub fn new() -> Arc<Self> {
        let (room_tx, room_rx) = bounded(1);
        Arc::new(Self {
            state: Mutex::new(State::default()),
            unapplied: AtomicUsize::new(0),
            room_tx,
            room_rx,
        })
    }

    /// Hands `decoded` over, after those before it, a push of a pane's output merged with that
    /// pane's run of them, `first` the pane input last went to: the tasks to queue on the GUI
    /// thread, to take everything (none is queued yet) and to take `first`'s pushes ahead of the
    /// rest (this is a push of `first`'s, and none is queued for it yet)
    pub fn hand(&self, decoded: DecodedPdu, first: Option<PaneId>) -> Handed {
        self.unapplied.fetch_add(1, Ordering::AcqRel);
        let is_first = first.is_some() && push_of(&decoded) == first;
        let pane = decoded.pdu.pane_id();
        let mut state = self.state.lock().unwrap();
        if let Some(pane) = pane {
            *state.waiting.entry(pane).or_default() += 1;
        }
        state.merger.add(decoded);
        Handed {
            take_all: !std::mem::replace(&mut state.scheduled, true),
            take_first: is_first && !std::mem::replace(&mut state.first_scheduled, true),
        }
    }

    /// The pushes handed over of `pane_id`, the pane input last went to, in order, for the GUI
    /// thread's task for it: taken ahead of everything else, so its echo is applied, and its
    /// frame painted, without waiting for every other pane's pushes to be applied first. A push
    /// changes only its own pane's lines, so it can go ahead of what's for other panes; nothing
    /// else can (a pane focused, a clipboard set, a pane removed, an alert: two of those for two
    /// panes, taken out of order, could leave the window on the wrong pane or clipboard), so
    /// everything else stays where it came, for [`Backlog::take`].
    pub fn take_first(self: &Arc<Self>, pane_id: PaneId) -> Vec<(DecodedPdu, Applied)> {
        let items = {
            let mut state = self.state.lock().unwrap();
            state.first_scheduled = false;
            state.merger.take_pane(pane_id)
        };
        items
            .into_iter()
            .map(|(d, n)| {
                let applied = Applied::new(Arc::clone(self), &d, n);
                (d, applied)
            })
            .collect()
    }

    /// The pushes handed over of `pane_id`, in order, for a read that must see the pane as its
    /// server has it and has asked for its push (lbterm_current.rs): applied by the reader, at
    /// once, ahead of everything else, as [`Backlog::take_first`]'s are and for the same reason,
    /// the tasks queued to take the rest left as they are
    pub fn take_pane(self: &Arc<Self>, pane_id: PaneId) -> Vec<(DecodedPdu, Applied)> {
        let items = self.state.lock().unwrap().merger.take_pane(pane_id);
        items
            .into_iter()
            .map(|(d, n)| {
                let applied = Applied::new(Arc::clone(self), &d, n);
                (d, applied)
            })
            .collect()
    }

    /// Everything handed over, for the GUI thread's task, each with what marks it applied; what's
    /// handed over from now on queues another task
    pub fn take(self: &Arc<Self>) -> Vec<(DecodedPdu, Applied)> {
        let items = {
            let mut state = self.state.lock().unwrap();
            state.scheduled = false;
            state.merger.take()
        };
        items
            .into_iter()
            .map(|(d, n)| {
                let applied = Applied::new(Arc::clone(self), &d, n);
                (d, applied)
            })
            .collect()
    }

    /// The GUI thread has applied what it took, having started at `started`, at `now`
    pub fn drained(&self, started: Instant, now: Instant) {
        let held = now.saturating_duration_since(started);
        self.state.lock().unwrap().applied_at = (held >= GAP_AFTER).then_some(now);
    }

    /// How long the GUI thread's task should wait at `now` before it takes what's here: what's
    /// left of [`GAP`] since it last applied what it took, if that held it [`GAP_AFTER`] or more
    pub fn gap(&self, now: Instant) -> Option<Duration> {
        let applied_at = self.state.lock().unwrap().applied_at?;
        (applied_at + GAP)
            .checked_duration_since(now)
            .filter(|left| !left.is_zero())
    }

    fn applied(&self, n: usize) {
        if self.unapplied.fetch_sub(n, Ordering::AcqRel) == n {
            // All applied: a client thread holding off may read on
            let _ = self.room_tx.try_send(());
        }
    }

    fn pane_applied(&self, pane: PaneId, n: usize) {
        let mut state = self.state.lock().unwrap();
        if let Some(left) = state.waiting.get_mut(&pane) {
            *left = left.saturating_sub(n);
            if *left == 0 {
                state.waiting.remove(&pane);
            }
        }
    }

    /// Whether anything of `pane_id`'s (the server's id) is handed over and not yet applied:
    /// waiting to be taken, taken and not yet applied, or applied by a task still going on.
    /// Asked by the client thread, the only one that hands anything over, so a no stays
    /// a no until that thread hands the pane something again (lbterm_names.rs)
    pub fn waits(&self, pane_id: PaneId) -> bool {
        self.state.lock().unwrap().waiting.contains_key(&pane_id)
    }

    /// How many are handed over and not yet applied
    pub fn unapplied(&self) -> usize {
        self.unapplied.load(Ordering::Acquire)
    }

    /// Whether the client thread should stop reading: `LIMIT` or more wait to be applied
    pub fn full(&self) -> bool {
        self.unapplied() >= LIMIT
    }

    /// Waits until the GUI thread has applied everything handed over
    pub async fn wait_until_applied(&self) {
        while self.unapplied() > 0 {
            if self.room_rx.recv().await.is_err() {
                return;
            }
        }
    }
}

/// The PDUs handed over, in the order they came, each pane's pushes of its output merged into
/// one where the first of them was, until a key's answer or a push with images, which go as they
/// came and start the pane's next run. Each with how many PDUs it stands for.
#[derive(Default)]
pub struct Merger {
    items: VecDeque<(DecodedPdu, usize)>,
    /// Each pane's run being merged: where it is in `items`
    runs: HashMap<PaneId, usize>,
}

impl Merger {
    /// `decoded` after what's here, merged into its pane's run when it's a push that can be
    pub fn add(&mut self, decoded: DecodedPdu) {
        let pane_id = match &decoded.pdu {
            Pdu::GetPaneRenderChangesResponse(push) if mergeable(push) => push.pane_id,
            Pdu::GetPaneRenderChangesResponse(push) => {
                self.runs.remove(&push.pane_id);
                self.items.push_back((decoded, 1));
                return;
            }
            _ => {
                self.items.push_back((decoded, 1));
                return;
            }
        };
        match self.runs.get(&pane_id) {
            Some(&i) => {
                let newer = match decoded.pdu {
                    Pdu::GetPaneRenderChangesResponse(push) => push,
                    _ => unreachable!(),
                };
                let (into, count) = &mut self.items[i];
                if let Pdu::GetPaneRenderChangesResponse(older) = &mut into.pdu {
                    let merged = merge_two(std::mem::replace(older, empty_push(pane_id)), newer);
                    *older = merged;
                }
                *count += 1;
            }
            None => {
                self.runs.insert(pane_id, self.items.len());
                self.items.push_back((decoded, 1));
            }
        }
    }

    /// Everything here, in order; the next added starts afresh
    pub fn take(&mut self) -> Vec<(DecodedPdu, usize)> {
        self.runs.clear();
        self.items.drain(..).collect()
    }

    /// The pushes here of `pane_id`, in order, the rest left as it was: the next of `pane_id`'s
    /// starts afresh, the other panes' runs go on
    pub fn take_pane(&mut self, pane_id: PaneId) -> Vec<(DecodedPdu, usize)> {
        let mut taken = vec![];
        let mut kept = VecDeque::with_capacity(self.items.len());
        // Where each item kept was, and is now
        let mut moved = HashMap::new();
        for (i, item) in self.items.drain(..).enumerate() {
            if push_of(&item.0) == Some(pane_id) {
                taken.push(item);
            } else {
                moved.insert(i, kept.len());
                kept.push_back(item);
            }
        }
        self.items = kept;
        self.runs.remove(&pane_id);
        for i in self.runs.values_mut() {
            *i = moved[&*i];
        }
        taken
    }
}

/// How long after the answer to input (a key, a paste, typing, the mouse) the client thread reads
/// on whatever the GUI thread has yet to apply, unless the pane's next push comes first: the echo
/// comes in a push of its own a few milliseconds after the answer (a shell's in 4 or 5 ms on a
/// test server, 2026-10-30), behind whatever the server had sent already, and holding off would
/// leave it there until the GUI thread had applied a batch for each socketful ahead of it. What's
/// read meanwhile is merged as it comes, so the GUI thread's work stays a push a pane.
pub const ECHO_WINDOW: Duration = Duration::from_millis(50);

/// The client thread's input to the server waiting on its answer, and the echo it waits for after
/// the last answer: no holding off while either says so ([`Backlog::full`]). WriteToPane is also
/// what ClientPane's writer blocks the GUI thread on, which with nothing read would never get its
/// answer. So is a read's ask of a held pane's push (lbterm_current.rs), which the GUI thread can
/// wait on with nothing else running ([`Input::awaited`]).
#[derive(Default)]
pub struct Input {
    /// The serial of each input sent and not yet answered, and its pane
    waiting: HashMap<u64, PaneId>,
    /// The serial of each read's ask sent and not yet answered: not input, so no echo after it
    /// and no pane taken first, but read on for all the same
    awaited: HashSet<u64>,
    /// The pane last answered and until when its echo is waited for: till its next push
    echo: Option<(PaneId, Instant)>,
    /// The pane input last went to, whose PDUs the GUI thread takes first ([`Backlog::take_first`])
    first: Option<PaneId>,
}

impl Input {
    /// `pdu` went out with `serial`
    pub fn sent(&mut self, serial: u64, pdu: &Pdu) {
        let pane_id = match pdu {
            Pdu::SendKeyDown(p) => p.pane_id,
            Pdu::SendPaste(p) => p.pane_id,
            Pdu::WriteToPane(p) => p.pane_id,
            Pdu::SendMouseEvent(p) => p.pane_id,
            _ => return,
        };
        self.waiting.insert(serial, pane_id);
        self.first = Some(pane_id);
    }

    /// A read's ask went out with `serial`, which something may be waiting on with nothing else
    /// running (lbterm_current.rs): read on until its answer
    pub fn awaited(&mut self, serial: u64) {
        self.awaited.insert(serial);
    }

    /// The pane input last went to
    pub fn first(&self) -> Option<PaneId> {
        self.first
    }

    /// The answer to `serial` came at `now`: whether it was a read's ask's ([`Input::awaited`])
    pub fn answered(&mut self, serial: u64, now: Instant) -> bool {
        if let Some(pane_id) = self.waiting.remove(&serial) {
            self.echo = Some((pane_id, now + ECHO_WINDOW));
        }
        self.awaited.remove(&serial)
    }

    /// A push of `pane_id`'s output came after its answer: the echo, if it's the pane waited on
    pub fn pushed(&mut self, pane_id: PaneId) {
        if matches!(self.echo, Some((p, _)) if p == pane_id) {
            self.echo = None;
        }
    }

    /// Whether the client thread must read on at `now` whatever the GUI thread has to apply
    pub fn reads_on(&self, now: Instant) -> bool {
        !self.waiting.is_empty()
            || !self.awaited.is_empty()
            || self.echo.map_or(false, |(_, until)| now < until)
    }
}

/// The pane `decoded` is a push of, if it's a push (GetPaneRenderChangesResponse)
fn push_of(decoded: &DecodedPdu) -> Option<PaneId> {
    match &decoded.pdu {
        Pdu::GetPaneRenderChangesResponse(push) => Some(push.pane_id),
        _ => None,
    }
}

/// Whether `push` can be merged with others of its pane: a push of the pane's output, not a key's
/// answer (whose input serial the window times its round trip and places its cursor by), and
/// with no images (whose data the window fetches before it has the lines)
fn mergeable(push: &GetPaneRenderChangesResponse) -> bool {
    push.input_serial.is_none() && !push.bonus_lines.lbterm_has_images()
}

fn empty_push(pane_id: PaneId) -> GetPaneRenderChangesResponse {
    GetPaneRenderChangesResponse {
        pane_id,
        mouse_grabbed: false,
        alt_screen_active: false,
        cursor_position: Default::default(),
        dimensions: Default::default(),
        dirty_lines: vec![],
        title: String::new(),
        working_dir: None,
        bonus_lines: vec![].into(),
        input_serial: None,
        seqno: 0,
    }
}

fn rows(ranges: &[std::ops::Range<StableRowIndex>]) -> RangeSet<StableRowIndex> {
    let mut set = RangeSet::new();
    for r in ranges {
        set.add_range(r.clone());
    }
    set
}

/// `older` then `newer`, of one pane, as one push: everything the newer says of the pane, every
/// row either said changed, and of the lines they carry the newer's, and the older's that the
/// newer didn't say changed again and that are still in its view. A row the newer said changed
/// without carrying it stays changed, to be fetched, as it would after applying both, and so do
/// the rows the cursor moved between (apply_changes_to_surface counts both changed when the
/// cursor moves). An older line that has scrolled up out of the newer's view goes as a row that
/// changed, not as a line: the window drops it from what it holds and fetches it if it's ever
/// looked at, rather than laying it out now (a flooding tab's pushes each carry a screen of new
/// rows, every one scrolled out of view by the next).
fn merge_two(
    older: GetPaneRenderChangesResponse,
    mut newer: GetPaneRenderChangesResponse,
) -> GetPaneRenderChangesResponse {
    let mut newer_dirty = rows(&newer.dirty_lines);
    if newer.cursor_position != older.cursor_position {
        newer_dirty.add(older.cursor_position.y);
        newer_dirty.add(newer.cursor_position.y);
    }
    let (newer_lines, _) = std::mem::take(&mut newer.bonus_lines).extract_data();
    let (older_lines, _) = older.bonus_lines.extract_data();
    let top = newer.dimensions.physical_top;
    let mut dirty = rows(&older.dirty_lines);
    dirty.add_set(&newer_dirty);
    let mut lines: BTreeMap<StableRowIndex, Line> = BTreeMap::new();
    for (row, line) in older_lines {
        if newer_dirty.contains(row) {
            continue;
        }
        if row < top {
            dirty.add(row);
        } else {
            lines.insert(row, line);
        }
    }
    for (row, line) in newer_lines {
        lines.insert(row, line);
    }
    // What the merged push carries is the window's, so its rows aren't to be fetched however the
    // older left them
    for row in lines.keys() {
        dirty.remove(*row);
    }
    newer.dirty_lines = dirty.iter().cloned().collect();
    newer.bonus_lines = lines.into_iter().collect::<Vec<_>>().into();
    newer
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::{
        InputSerial, NotifyAlert, PaneFocused, Pong, SendKeyDown, SetClipboard, WriteToPane,
    };
    use mux::renderable::{RenderableDimensions, StableCursorPosition};
    use std::collections::{HashMap, HashSet};
    use wezterm_term::{Alert, CellAttributes};

    fn pdu(serial: u64) -> DecodedPdu {
        DecodedPdu {
            serial,
            pdu: Pdu::Pong(Pong {}),
        }
    }

    impl Backlog {
        /// Hands `decoded` over with no pane typed into: whether a task to take it all is asked
        fn hand_all(&self, decoded: DecodedPdu) -> bool {
            let handed = self.hand(decoded, None);
            assert!(!handed.take_first);
            handed.take_all
        }
    }

    fn waits(b: &Backlog) -> bool {
        smol::future::block_on(async {
            futures::FutureExt::now_or_never(b.wait_until_applied()).is_none()
        })
    }

    #[test]
    fn input_reads_on_until_its_answer_and_the_echo_after() {
        let mut input = Input::default();
        let t0 = Instant::now();
        assert!(!input.reads_on(t0));
        input.sent(7, &Pdu::Pong(Pong {}));
        assert!(!input.reads_on(t0), "an answer that isn't to input holds nothing off");
        input.sent(8, &Pdu::WriteToPane(WriteToPane { pane_id: 1, data: vec![] }));
        assert!(input.reads_on(t0 + Duration::from_secs(60)), "until it's answered");
        input.answered(7, t0);
        assert!(input.reads_on(t0));
        input.answered(8, t0);
        assert!(input.reads_on(t0 + ECHO_WINDOW / 2), "the echo comes after the answer");
        assert!(!input.reads_on(t0 + ECHO_WINDOW));
    }

    #[test]
    fn a_reads_ask_reads_on_until_its_answer_with_no_echo_or_pane_first() {
        let mut input = Input::default();
        let t0 = Instant::now();
        input.awaited(9);
        assert!(input.reads_on(t0 + Duration::from_secs(60)), "until it's answered");
        assert_eq!(input.first(), None, "an ask isn't input: no pane is taken first for it");
        assert!(!input.answered(7, t0), "another answer isn't the ask's");
        assert!(input.reads_on(t0));
        assert!(input.answered(9, t0), "the ask's answer says so");
        assert!(!input.reads_on(t0), "and no echo is waited for after it");
        assert!(!input.answered(9, t0), "once");
    }

    #[test]
    fn the_panes_next_push_is_its_echo() {
        let mut input = Input::default();
        let t0 = Instant::now();
        input.sent(
            3,
            &Pdu::SendKeyDown(SendKeyDown {
                pane_id: 5,
                event: termwiz::input::KeyEvent {
                    key: termwiz::input::KeyCode::Char('x'),
                    modifiers: Default::default(),
                },
                input_serial: InputSerial::empty(),
            }),
        );
        input.answered(3, t0);
        input.pushed(4);
        assert!(input.reads_on(t0), "another pane's push isn't its echo");
        input.pushed(5);
        assert!(!input.reads_on(t0), "its own is");
    }

    #[test]
    fn one_task_takes_them_in_order() {
        let b = Backlog::new();
        assert!(b.hand_all(pdu(1)), "the first queues a task");
        assert!(!b.hand_all(pdu(2)), "the task queued takes this one too");
        assert!(!b.hand_all(pdu(3)));
        let taken = b.take();
        assert_eq!(taken.iter().map(|(d, _)| d.serial).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(b.unapplied(), 3, "taken isn't applied");
        assert!(b.hand_all(pdu(4)), "after the take, the next queues another task");
        drop(taken);
        assert_eq!(b.unapplied(), 1);
        let taken = b.take();
        assert_eq!(taken.len(), 1);
        drop(taken);
        assert_eq!(b.unapplied(), 0);
        assert!(b.take().is_empty());
    }

    #[test]
    fn the_pane_typed_into_is_taken_first_and_the_rest_left_as_they_were() {
        let b = Backlog::new();
        let push = |pane, seqno| DecodedPdu {
            serial: 0,
            pdu: make(&spec(pane, &[0..1], &[(0, "x")], 0, "t", seqno)),
        };
        let bell = |pane| DecodedPdu {
            serial: 0,
            pdu: Pdu::NotifyAlert(NotifyAlert {
                pane_id: pane,
                alert: Alert::Bell,
            }),
        };
        let all = Handed { take_all: true, take_first: false };
        let first = Handed { take_all: false, take_first: true };
        let none = Handed { take_all: false, take_first: false };
        assert_eq!(b.hand(push(1, 1), Some(2)), all);
        assert_eq!(b.hand(push(2, 1), Some(2)), first);
        assert_eq!(b.hand(bell(1), Some(2)), none);
        assert_eq!(b.hand(push(2, 2), Some(2)), none, "one task for it is queued");
        assert_eq!(b.unapplied(), 4);
        let taken = b.take_first(2);
        assert_eq!(
            taken
                .iter()
                .map(|(d, a)| (d.pdu.pane_id(), a.count()))
                .collect::<Vec<_>>(),
            vec![(Some(2), 2)],
            "its two pushes, merged"
        );
        assert_eq!(b.hand(push(1, 2), Some(2)), none, "pane 1's merges into its run");
        assert_eq!(b.hand(push(2, 3), Some(2)), first, "after its take, the next queues another");
        let rest = b.take();
        assert_eq!(
            rest.iter()
                .map(|(d, a)| (d.pdu.pdu_name(), d.pdu.pane_id(), a.count()))
                .collect::<Vec<_>>(),
            vec![
                ("GetPaneRenderChangesResponse", Some(1), 2),
                ("NotifyAlert", Some(1), 1),
                ("GetPaneRenderChangesResponse", Some(2), 1),
            ],
            "pane 1's run kept its place past the take, its second merged into it"
        );
        assert!(b.take_first(2).is_empty(), "taken already with the rest");
        drop((taken, rest));
        assert_eq!(b.unapplied(), 0);
    }

    #[test]
    fn a_read_takes_its_panes_pushes_and_leaves_the_tasks_queued_as_they_were() {
        let b = Backlog::new();
        let push = |pane, seqno| DecodedPdu {
            serial: 0,
            pdu: make(&spec(pane, &[0..1], &[(0, "x")], 0, "t", seqno)),
        };
        let held = |pane| DecodedPdu {
            serial: 0,
            pdu: Pdu::PaneHeld(codec::PaneHeld { pane_id: pane, seqno: 1 }),
        };
        let all = Handed { take_all: true, take_first: false };
        let first = Handed { take_all: false, take_first: true };
        let none = Handed { take_all: false, take_first: false };
        assert_eq!(b.hand(push(1, 1), Some(2)), all);
        assert_eq!(b.hand(held(3), Some(2)), none);
        assert_eq!(b.hand(push(2, 1), Some(2)), first);
        assert_eq!(b.hand(push(3, 2), Some(2)), none);
        assert_eq!(b.hand(push(3, 3), Some(2)), none, "merged into pane 3's run");
        let read = b.take_pane(3);
        assert_eq!(
            read.iter().map(|(d, a)| (d.pdu.pane_id(), a.count())).collect::<Vec<_>>(),
            vec![(Some(3), 2)],
            "its pushes alone, merged: the word of its hold stays where it came"
        );
        assert_eq!(b.hand(push(3, 4), Some(2)), none, "the tasks queued are still queued");
        let typed = b.take_first(2);
        assert_eq!(typed.len(), 1);
        let rest = b.take();
        assert_eq!(
            rest.iter()
                .map(|(d, a)| (d.pdu.pdu_name(), d.pdu.pane_id(), a.count()))
                .collect::<Vec<_>>(),
            vec![
                ("GetPaneRenderChangesResponse", Some(1), 1),
                ("PaneHeld", Some(3), 1),
                ("GetPaneRenderChangesResponse", Some(3), 1),
            ],
            "the rest in order, pane 3's push after the read a run of its own"
        );
        assert_eq!(b.unapplied(), 6);
        drop((read, typed, rest));
        assert_eq!(b.unapplied(), 0);
    }

    /// What the window ends on after applying `taken` in turn: the pane focused last, the clipboard
    /// set last, and every PDU's name and pane in the order applied
    fn applied_in_turn(
        taken: &[(DecodedPdu, Applied)],
    ) -> (Option<PaneId>, Option<String>, Vec<(&'static str, Option<PaneId>)>) {
        let mut focused = None;
        let mut clipboard = None;
        let mut order = vec![];
        for (d, _) in taken {
            match &d.pdu {
                Pdu::PaneFocused(f) => focused = Some(f.pane_id),
                Pdu::SetClipboard(c) => clipboard = c.clipboard.clone(),
                _ => {}
            }
            order.push((d.pdu.pdu_name(), d.pdu.pane_id()));
        }
        (focused, clipboard, order)
    }

    #[test]
    fn only_the_typed_panes_pushes_go_first() {
        // Lachlan types in A (pane 1), switches to B (pane 2) and straight back to A while the GUI
        // thread is busy: the server sends PaneFocused(B) then PaneFocused(A), and the window must
        // end on A; two panes' clipboards, likewise, on the later. Only A's push goes ahead
        let b = Backlog::new();
        let focus = |pane_id| DecodedPdu {
            serial: 0,
            pdu: Pdu::PaneFocused(PaneFocused { pane_id }),
        };
        let clip = |pane_id, text: &str| DecodedPdu {
            serial: 0,
            pdu: Pdu::SetClipboard(SetClipboard {
                pane_id,
                clipboard: Some(text.to_string()),
                selection: wezterm_term::ClipboardSelection::Clipboard,
            }),
        };
        let push = |pane, seqno| DecodedPdu {
            serial: 0,
            pdu: make(&spec(pane, &[0..1], &[(0, "x")], 0, "t", seqno)),
        };
        let none = Handed { take_all: false, take_first: false };
        assert_eq!(b.hand(push(3, 1), Some(1)), Handed { take_all: true, take_first: false });
        assert_eq!(b.hand(focus(2), Some(1)), none);
        assert_eq!(b.hand(clip(2, "older"), Some(1)), none);
        assert_eq!(b.hand(focus(1), Some(1)), none, "only a push of the pane asks for its task");
        assert_eq!(b.hand(clip(1, "newer"), Some(1)), none);
        assert_eq!(
            b.hand(
                DecodedPdu {
                    serial: 0,
                    pdu: Pdu::NotifyAlert(NotifyAlert {
                        pane_id: 1,
                        alert: Alert::Bell
                    })
                },
                Some(1)
            ),
            none
        );
        assert_eq!(b.hand(push(1, 1), Some(1)), Handed { take_all: false, take_first: true });
        let first = b.take_first(1);
        assert_eq!(
            applied_in_turn(&first).2,
            vec![("GetPaneRenderChangesResponse", Some(1))],
            "its push alone"
        );
        let rest = b.take();
        let (focused, clipboard, order) = applied_in_turn(&rest);
        assert_eq!(focused, Some(1), "the window ends on A, as the server does");
        assert_eq!(clipboard.as_deref(), Some("newer"));
        assert_eq!(
            order,
            vec![
                ("GetPaneRenderChangesResponse", Some(3)),
                ("PaneFocused", Some(2)),
                ("SetClipboard", Some(2)),
                ("PaneFocused", Some(1)),
                ("SetClipboard", Some(1)),
                ("NotifyAlert", Some(1)),
            ],
            "everything else in the order it came"
        );
        drop((first, rest));
        assert_eq!(b.unapplied(), 0);
    }

    #[test]
    fn a_take_right_after_a_long_one_waits_out_the_gap() {
        let b = Backlog::new();
        let t0 = Instant::now();
        let started = t0 - GAP_AFTER;
        assert_eq!(b.gap(t0), None, "nothing applied yet: no wait");
        b.drained(t0 - Duration::from_millis(1), t0);
        assert_eq!(b.gap(t0), None, "a short take: the loop turns anyway, no wait");
        b.drained(started, t0);
        assert_eq!(b.gap(t0), Some(GAP), "one that held the GUI thread a while");
        assert_eq!(b.gap(t0 + GAP / 4), Some(GAP - GAP / 4));
        assert_eq!(b.gap(t0 + GAP), None);
        assert_eq!(b.gap(t0 + GAP * 10), None, "a GUI thread that came back later takes at once");
        b.drained(started, t0);
        b.drained(t0, t0);
        assert_eq!(b.gap(t0), None, "the last take decides");
    }

    #[test]
    fn full_at_the_limit_and_held_until_all_are_applied() {
        let b = Backlog::new();
        assert!(!waits(&b), "nothing handed over: no wait");
        for i in 0..LIMIT as u64 - 1 {
            b.hand_all(pdu(i));
        }
        assert!(!b.full());
        b.hand_all(pdu(99));
        assert!(b.full());
        let mut taken = b.take();
        assert!(b.full(), "taken but not applied yet");
        assert!(waits(&b));
        taken.pop();
        assert!(!b.full(), "under the limit");
        assert!(waits(&b), "but a wait goes on until they're all applied");
        drop(taken);
        assert_eq!(b.unapplied(), 0);
        assert!(!waits(&b));
    }

    #[test]
    fn a_wait_ends_when_the_gui_thread_applies_them() {
        let b = Backlog::new();
        for i in 0..LIMIT as u64 * 3 {
            b.hand_all(pdu(i));
        }
        let taken = b.take();
        let gui = std::thread::spawn(move || {
            for applied in taken {
                std::thread::sleep(Duration::from_micros(200));
                drop(applied);
            }
        });
        smol::future::block_on(b.wait_until_applied());
        assert_eq!(b.unapplied(), 0);
        gui.join().unwrap();
    }

    const VIEW: usize = 10;

    /// A push as plain data, so the same one can be made twice
    #[derive(Clone, Debug)]
    struct Spec {
        pane: PaneId,
        dirty: Vec<std::ops::Range<StableRowIndex>>,
        carried: Vec<(StableRowIndex, String)>,
        cursor: StableRowIndex,
        top: StableRowIndex,
        title: String,
        seqno: usize,
        serial: bool,
    }

    fn spec(
        pane: PaneId,
        dirty: &[std::ops::Range<StableRowIndex>],
        carried: &[(StableRowIndex, &str)],
        cursor: StableRowIndex,
        title: &str,
        seqno: usize,
    ) -> Spec {
        Spec {
            pane,
            dirty: dirty.to_vec(),
            carried: carried.iter().map(|(r, t)| (*r, t.to_string())).collect(),
            cursor,
            top: 0,
            title: title.to_string(),
            seqno,
            serial: false,
        }
    }

    fn make(s: &Spec) -> Pdu {
        Pdu::GetPaneRenderChangesResponse(GetPaneRenderChangesResponse {
            pane_id: s.pane,
            mouse_grabbed: false,
            alt_screen_active: false,
            cursor_position: StableCursorPosition {
                y: s.cursor,
                ..Default::default()
            },
            dimensions: RenderableDimensions {
                physical_top: s.top,
                viewport_rows: VIEW,
                ..Default::default()
            },
            dirty_lines: s.dirty.clone(),
            title: s.title.clone(),
            working_dir: None,
            bonus_lines: s
                .carried
                .iter()
                .map(|(row, t)| (*row, Line::from_text(t, &CellAttributes::default(), 1, None)))
                .collect::<Vec<_>>()
                .into(),
            input_serial: if s.serial {
                Some(InputSerial::now())
            } else {
                None
            },
            seqno: s.seqno,
        })
    }

    /// What the window holds of a pane, as apply_changes_to_surface leaves it: a carried line put;
    /// a row said changed (or one the cursor moved from or to) without a line dropped, to be
    /// fetched when it's in view and stale above it
    #[derive(Default, Debug, PartialEq)]
    struct Window {
        lines: HashMap<StableRowIndex, String>,
        fetch: HashSet<StableRowIndex>,
        cursor: StableRowIndex,
        top: StableRowIndex,
        title: String,
        seqno: usize,
    }

    impl Window {
        fn apply(&mut self, push: GetPaneRenderChangesResponse) {
            let mut dirty = rows(&push.dirty_lines);
            if push.cursor_position.y != self.cursor {
                dirty.add(self.cursor);
                dirty.add(push.cursor_position.y);
            }
            self.cursor = push.cursor_position.y;
            self.top = push.dimensions.physical_top;
            let (carried, _) = push.bonus_lines.extract_data();
            for (row, line) in carried {
                dirty.remove(row);
                self.fetch.remove(&row);
                self.lines.insert(row, line.as_str().trim_end().to_string());
            }
            for r in dirty.iter() {
                for row in r.clone() {
                    self.lines.remove(&row);
                    if row >= self.top {
                        self.fetch.insert(row);
                    }
                }
            }
            self.title = push.title;
            self.seqno = push.seqno;
        }

        /// The same in view, and nothing above it held that `each` doesn't hold the same
        fn agrees_with(&self, each: &Window) -> Result<(), String> {
            let view = self.top..self.top + VIEW as StableRowIndex;
            if (self.cursor, self.top, &self.title, self.seqno)
                != (each.cursor, each.top, &each.title, each.seqno)
            {
                return Err(format!(
                    "state {:?} against {:?}",
                    (self.cursor, self.top, &self.title, self.seqno),
                    (each.cursor, each.top, &each.title, each.seqno)
                ));
            }
            for row in view.clone() {
                if self.lines.get(&row) != each.lines.get(&row)
                    || self.fetch.contains(&row) != each.fetch.contains(&row)
                {
                    return Err(format!(
                        "row {row} in view: {:?}/{} against {:?}/{}",
                        self.lines.get(&row),
                        self.fetch.contains(&row),
                        each.lines.get(&row),
                        each.fetch.contains(&row)
                    ));
                }
            }
            for (row, line) in &self.lines {
                if !view.contains(row) && each.lines.get(row) != Some(line) {
                    return Err(format!(
                        "row {row} above view held as {line:?}, {:?} each in turn",
                        each.lines.get(row)
                    ));
                }
            }
            Ok(())
        }
    }

    fn as_push(pdu: Pdu) -> GetPaneRenderChangesResponse {
        match pdu {
            Pdu::GetPaneRenderChangesResponse(p) => p,
            other => panic!("not a push: {:?}", other),
        }
    }

    /// The window after `specs` applied each in turn, and after them handed over (merged as they
    /// come) and taken
    fn both_ways(specs: &[Spec]) -> (Window, Window, usize) {
        let mut each = Window::default();
        for s in specs {
            each.apply(as_push(make(s)));
        }
        let b = Backlog::new();
        for s in specs {
            b.hand_all(DecodedPdu {
                serial: 0,
                pdu: make(s),
            });
        }
        let merged = b.take();
        let n = merged.len();
        let mut once = Window::default();
        for (d, _) in merged {
            once.apply(as_push(d.pdu));
        }
        assert_eq!(b.unapplied(), 0, "every push handed over was applied");
        (each, once, n)
    }

    #[test]
    fn a_panes_pushes_merge_where_the_first_was() {
        let b = Backlog::new();
        let pdus = vec![
            make(&spec(1, &[4..6], &[(4, "a1"), (5, "a2")], 5, "one", 1)),
            make(&spec(2, &[5..6], &[(5, "b1")], 5, "two", 1)),
            Pdu::NotifyAlert(NotifyAlert {
                pane_id: 1,
                alert: Alert::Bell,
            }),
            make(&spec(1, &[5..7], &[(6, "a3")], 6, "one again", 2)),
            make(&spec(2, &[6..7], &[(6, "b2")], 6, "two again", 2)),
        ];
        for pdu in pdus {
            b.hand_all(DecodedPdu { serial: 0, pdu });
        }
        assert_eq!(b.unapplied(), 5, "each PDU handed over counts");
        let merged = b.take();
        let names: Vec<_> = merged
            .iter()
            .map(|(d, a)| (d.pdu.pdu_name(), d.pdu.pane_id(), a.count()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("GetPaneRenderChangesResponse", Some(1), 2),
                ("GetPaneRenderChangesResponse", Some(2), 2),
                ("NotifyAlert", Some(1), 1),
            ],
            "a merged push for each pane where its first was, then the alert"
        );
        assert_eq!(b.unapplied(), 5);
        let one = match &merged[0].0.pdu {
            Pdu::GetPaneRenderChangesResponse(p) => p,
            _ => unreachable!(),
        };
        assert_eq!(
            (one.title.as_str(), one.seqno, one.cursor_position.y),
            ("one again", 2, 6)
        );
        drop(merged);
        assert_eq!(b.unapplied(), 0);
    }

    #[test]
    fn a_push_after_the_take_starts_a_run_of_its_own() {
        let b = Backlog::new();
        b.hand_all(DecodedPdu {
            serial: 0,
            pdu: make(&spec(1, &[0..1], &[(0, "a")], 0, "t", 1)),
        });
        let first = b.take();
        b.hand_all(DecodedPdu {
            serial: 0,
            pdu: make(&spec(1, &[1..2], &[(1, "b")], 1, "t", 2)),
        });
        assert_eq!(first.len(), 1);
        let first_push = match &first[0].0.pdu {
            Pdu::GetPaneRenderChangesResponse(p) => p.seqno,
            _ => unreachable!(),
        };
        assert_eq!(first_push, 1, "what the GUI thread took isn't merged into");
        let second = b.take();
        assert_eq!(second.len(), 1);
        drop((first, second));
        assert_eq!(b.unapplied(), 0);
    }

    #[test]
    fn merged_leaves_the_window_as_each_in_turn() {
        // Row 5 carried, then said changed without being carried: to be fetched
        let (each, once, n) = both_ways(&[
            spec(1, &[4..6], &[(4, "a"), (5, "b")], 5, "t1", 1),
            spec(1, &[5..6], &[], 5, "t2", 2),
            spec(1, &[7..9], &[(7, "c"), (8, "d")], 8, "t3", 3),
        ]);
        assert_eq!(n, 1);
        assert_eq!(each, once);
        assert!(once.fetch.contains(&5));
        assert_eq!(once.lines.get(&4).map(String::as_str), Some("a"));
        assert_eq!(once.title, "t3");
    }

    #[test]
    fn a_line_scrolled_out_of_view_goes_as_a_row_to_fetch() {
        // A flood: each push a screen of new rows, the view moved down past the last
        let mut a = spec(1, &[], &[(0, "a0"), (9, "a9")], 9, "t", 1);
        a.top = 0;
        let mut b = spec(1, &[], &[(20, "b0"), (29, "b9")], 29, "t", 2);
        b.top = 20;
        let (each, once, n) = both_ways(&[a, b]);
        assert_eq!(n, 1);
        once.agrees_with(&each).unwrap();
        assert_eq!(
            each.lines.get(&0).map(String::as_str),
            Some("a0"),
            "each in turn lays it out"
        );
        assert_eq!(once.lines.get(&0), None, "merged, it's left to fetch if it's looked at");
        assert_eq!(once.lines.get(&29).map(String::as_str), Some("b9"));
    }

    #[test]
    fn a_keys_answer_or_images_end_a_run() {
        let mut key = spec(1, &[1..2], &[(1, "k")], 1, "t", 2);
        key.serial = true;
        let specs = [
            spec(1, &[0..1], &[(0, "x")], 0, "t", 1),
            key,
            spec(1, &[2..3], &[(2, "y")], 2, "t", 3),
            spec(1, &[3..4], &[(3, "z")], 3, "t", 4),
        ];
        let b = Backlog::new();
        for s in &specs {
            b.hand_all(DecodedPdu {
                serial: 0,
                pdu: make(s),
            });
        }
        let counts: Vec<_> = b.take().iter().map(|(_, a)| a.count()).collect();
        assert_eq!(
            counts,
            vec![1, 1, 2],
            "the push before the key's answer, the answer, the two after merged"
        );
        let (each, once, _) = both_ways(&specs);
        assert_eq!(each, once);
    }

    /// Random runs of pushes, two panes interleaved and their views moving down, against applying
    /// each in turn
    #[test]
    fn random_runs_merge_to_the_same_window() {
        let mut seed: u64 = 0x5eed_1b7e_4d2a_9c31;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        for round in 0..3000 {
            let k = 1 + next(6) as usize;
            let mut specs = vec![];
            let mut tops = [0 as StableRowIndex; 3];
            for i in 0..k {
                let pane = 1 + next(2) as PaneId;
                if next(3) == 0 {
                    tops[pane] += next(12) as StableRowIndex;
                }
                let top = tops[pane];
                let in_view = |n: u64| top + n as StableRowIndex;
                let cursor = in_view(next(VIEW as u64));
                let mut dirty = vec![];
                for _ in 0..next(3) {
                    let start = next((top + VIEW as StableRowIndex) as u64 + 1) as StableRowIndex;
                    dirty.push(start..start + 1 + next(8) as StableRowIndex);
                }
                let mut carried = vec![(cursor, format!("c{round}.{i}"))];
                for _ in 0..next(5) {
                    let row = in_view(next(VIEW as u64));
                    if !carried.iter().any(|(r, _)| *r == row) {
                        carried.push((row, format!("r{round}.{i}.{row}")));
                    }
                }
                carried.sort_by_key(|(r, _)| *r);
                let mut s = spec(pane, &dirty, &[], cursor, &format!("t{i}"), i + 1);
                s.carried = carried;
                s.top = top;
                s.serial = next(5) == 0;
                specs.push(s);
            }
            for pane in [1, 2] {
                let mine: Vec<Spec> = specs.iter().filter(|s| s.pane == pane).cloned().collect();
                let (each, once, _) = both_ways(&mine);
                if let Err(err) = once.agrees_with(&each) {
                    panic!("round {}, pane {}: {}; {:?}", round, pane, err, mine);
                }
            }
        }
    }
}
