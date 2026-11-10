//! LBTerm: prompt marks on the scrollbar and the scroll strip (lbterm_extras::marks). A tick on
//! the track for each shell prompt (OSC 133) in the active pane's scrollback; a click on one
//! scrolls to it.
//!
//! A look reads only the rows written since the last one (`marks::plan`): after the first look
//! at a pane, a frame reads what its output wrote. The first look reads the screen, and its
//! scrollback a chunk of rows at a time between frames, newest first, so a long one never holds
//! a frame; so does a look after more than a chunk of rows went into the scrollback (a tab that
//! printed a long log in the background, `marks::split_since`). The GUI's own panes are read here, as they are. A pane in the mux server (a
//! ClientPane) is read from the lines the client already holds as the server has them, and the
//! server is asked which of the rest hold a prompt (GetPromptRows: the rows, never their lines),
//! one ask under way at a time, so no line of a scrollback is fetched to mark it
//! (lbterm/SCROLL-STRIP.md, "A shell's marks").

use crate::termwindow::{TermWindow, TermWindowNotif};
use ::window::WindowOps;
use codec::GetPromptRowsResponse;
use lbterm_extras::marks::{self, Plan, PromptRows, Seen};
use mux::lbterm_prompts;
use mux::pane::{Pane, PaneId, WithPaneLines};
use mux::Mux;
use rangeset::RangeSet;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wezterm_client::pane::ClientPane;
use wezterm_term::{Line, StableRowIndex};
use window::color::LinearRgba;

/// A mark's colour: the coral of the tab panel's light
const MARK: (u8, u8, u8) = (0xff, 0x8a, 0x80);
/// How far a click may miss a mark and still land on it, in pixels at 96 DPI
const SLOP: f32 = 3.0;
/// The most rows one chunk of a scrollback covers: the GUI thread looks at them between frames,
/// or for a mux pane the server's main thread, where every tab's output waits meanwhile (about
/// 1.6 us a row of a build's log on this PC, 2026-10-31). A server's answer is a few bytes
const ASK_ROWS: StableRowIndex = 5_000;

/// How long after an ask that failed (not for an old server) its rows are asked again
const RETRY_AFTER: Duration = Duration::from_secs(1);

/// Each ask's own number, so an answer to one a new width or screen made pointless is dropped
static ASKS: AtomicU64 = AtomicU64::new(1);
/// The server not answering is said once
static UNANSWERED_SAID: AtomicBool = AtomicBool::new(false);

/// A pane's screen: its primary screen and its alternate one number their rows apart
type Key = (PaneId, bool);

#[derive(Default)]
struct PaneMarks {
    rows: PromptRows,
    /// What the rows were last read at
    seen: Seen,
    /// A mux pane's rows to read that its client didn't hold as the server has them when they
    /// were to be read: the screen's are read once they come, the scrollback's asked of the server
    unread: RangeSet<StableRowIndex>,
    /// The scrollback's rows still to read (the GUI's own pane) or ask the server about (a mux
    /// pane's), and the chunk under way
    ask: RangeSet<StableRowIndex>,
    asking: Option<u64>,
    /// The server doesn't know GetPromptRows (a build from before it): never asked again
    unanswered: bool,
    /// An ask that failed otherwise (a pane gone as it's respawned, a connection lost): its
    /// rows are asked again at the first look after this
    retry_at: Option<Instant>,
    /// A chunk changed the marks since they were last drawn: the last chunk asks for a frame
    unshown: bool,
    /// The marks, made again only when the rows change
    marks: Option<(u64, Rc<Vec<StableRowIndex>>)>,
}

impl PaneMarks {
    fn marks(&mut self) -> Rc<Vec<StableRowIndex>> {
        let version = self.rows.version();
        match &self.marks {
            Some((v, marks)) if *v == version => Rc::clone(marks),
            _ => {
                let marks = Rc::new(self.rows.marks());
                self.marks = Some((version, Rc::clone(&marks)));
                marks
            }
        }
    }
}

/// Where the marks were drawn, for clicks on them
struct Drawn {
    pane: PaneId,
    track_top: usize,
    track_height: usize,
    mark_height: usize,
    marks: Rc<Vec<StableRowIndex>>,
    first_row: StableRowIndex,
    total_rows: usize,
}

#[derive(Default)]
pub struct MarksState {
    panes: HashMap<Key, PaneMarks>,
    drawn: Option<Drawn>,
}

/// Each line's row and whether it holds a prompt
#[derive(Default)]
struct Held(Vec<(StableRowIndex, bool)>);

impl WithPaneLines for Held {
    fn with_lines_mut(&mut self, first_row: StableRowIndex, lines: &mut [&mut Line]) {
        for (i, line) in lines.iter_mut().enumerate() {
            self.0.push((first_row + i as StableRowIndex, lbterm_prompts::is_prompt(line)));
        }
    }
}

/// The GUI's own pane's rows of `rows` that hold a prompt (lbterm_prompts::prompt_rows). With
/// `lbterm::lines` at debug (WEZTERM_LOG) each look says how many rows it read and how long it
/// took, waits for the pane's terminal included (perf/output-hold.ps1 reads them)
fn own_prompt_rows(pane: &dyn Pane, rows: Range<StableRowIndex>) -> (Range<StableRowIndex>, Vec<StableRowIndex>) {
    let timed = log::log_enabled!(target: "lbterm::lines", log::Level::Debug).then(Instant::now);
    let (looked, found) = lbterm_prompts::prompt_rows(pane, rows);
    if let Some(began) = timed {
        log::debug!(
            target: "lbterm::lines",
            "pane {}: prompt marks: {} rows looked at in {:.3} ms",
            pane.pane_id(),
            looked.end - looked.start,
            began.elapsed().as_secs_f64() * 1000.0
        );
    }
    (looked, found)
}

/// The GUI's own pane: what the plan says, read now, but a whole scrollback's rows, which are
/// read a chunk at a time between frames
fn read_own(pane: &dyn Pane, entry: &mut PaneMarks, plan: Plan) {
    match plan {
        Plan::Nothing => {}
        Plan::Everything { history, screen } => {
            entry.rows.clear();
            entry.ask = RangeSet::new();
            entry.ask.add_range(history);
            entry.asking = None;
            let (looked, found) = own_prompt_rows(pane, screen);
            entry.rows.set_range(looked, &found);
        }
        Plan::Since { history, screen, since } => {
            // More than a chunk into the scrollback since the last look: those rows a chunk at a
            // time between frames, newest first, the screen's now
            let (now, later) = marks::split_since(history, screen, ASK_ROWS);
            if let Some(later) = later {
                entry.ask.add_range(later);
            }
            for range in pane.get_changed_since(now, since).iter() {
                let (looked, found) = own_prompt_rows(pane, range.clone());
                entry.rows.set_range(looked, &found);
            }
        }
    }
}

/// A mux pane: the rows the plan says, from the lines its client holds; the scrollback's rows it
/// doesn't hold go to be asked of the server, the screen's wait for the client to have them
fn read_client(client: &ClientPane, entry: &mut PaneMarks, plan: Plan, top: StableRowIndex) {
    match plan {
        Plan::Nothing => {}
        Plan::Everything { history, screen } => {
            entry.rows.clear();
            entry.ask = RangeSet::new();
            entry.ask.add_range(history);
            // An ask under way is about rows numbered as they were: its answer is dropped
            entry.asking = None;
            entry.unanswered = false;
            entry.retry_at = None;
            entry.unread = RangeSet::new();
            entry.unread.add_range(screen);
        }
        Plan::Since { history, screen, since } => {
            // More than a chunk into the scrollback since the last look: asked of the server a
            // chunk at a time, as a first look's are, where the frame that showed the tab
            // looked through every row of it for the few the client held (a server that can't be
            // asked leaves only those)
            match marks::split_since(history.clone(), screen.clone(), ASK_ROWS) {
                (_, Some(later)) if !entry.unanswered => entry.ask.add_range(later),
                _ => entry.unread.add_range(history),
            }
            for range in client.get_changed_since(screen, since).iter() {
                entry.unread.add_range(range.clone());
            }
        }
    }
    if entry.unread.is_empty() {
        return;
    }
    let unread = std::mem::take(&mut entry.unread);
    let renderable = client.renderable.lock();
    for range in unread.iter() {
        let mut held = Held::default();
        let missing = renderable.lbterm_with_held_lines(range.clone(), &mut held);
        for (row, prompt) in held.0 {
            entry.rows.set(row, prompt);
        }
        for m in missing.iter() {
            entry.ask.add_range(m.start..m.end.min(top).max(m.start));
            entry.unread.add_range(m.start.max(top)..m.end.max(top));
        }
    }
}

/// The newest `most` rows of `rows` (`marks::newest`), taken out of it
fn take_newest(rows: &mut RangeSet<StableRowIndex>, most: StableRowIndex) -> Option<Range<StableRowIndex>> {
    let chunk = marks::newest(rows.iter(), most)?;
    rows.remove_range(chunk.clone());
    Some(chunk)
}

impl TermWindow {
    /// The pane's prompt marks, from the rows written since the last look only: a long
    /// scrollback costs once, the first time, and on a mux pane that's the server's to say
    pub(crate) fn lbterm_prompt_marks(&mut self, pane: &Arc<dyn Pane>) -> Rc<Vec<StableRowIndex>> {
        // An overlay (search, copy mode) shows its pane's lines: the marks are that pane's
        let pane = Mux::get()
            .get_pane(pane.pane_id())
            .unwrap_or_else(|| Arc::clone(pane));
        let dims = pane.get_dimensions();
        let key = (pane.pane_id(), pane.is_alt_screen_active());
        let now = marks::Dims {
            first: dims.scrollback_top,
            top: dims.physical_top,
            end: dims.physical_top + dims.viewport_rows as StableRowIndex,
            cols: dims.cols,
            seqno: pane.get_current_seqno(),
        };
        if !self.lbterm_marks.panes.contains_key(&key) {
            // Forget panes that have closed, as a new one comes
            let mux = Mux::get();
            self.lbterm_marks.panes.retain(|(id, _), _| mux.get_pane(*id).is_some());
        }
        let entry = self.lbterm_marks.panes.entry(key).or_default();
        let plan = marks::plan(&entry.seen, &now);
        match pane.downcast_ref::<ClientPane>() {
            Some(client) => read_client(client, entry, plan, now.top),
            None => read_own(&*pane, entry, plan),
        }
        entry.seen = Seen {
            seqno: Some(now.seqno),
            cols: now.cols,
            top: now.top,
        };
        entry.rows.forget_before(now.first);
        if !entry.ask.is_empty() {
            entry.ask.remove_range(StableRowIndex::MIN..now.first);
        }
        // Drawn now, whatever chunks changed
        entry.unshown = false;
        let waiting = entry.retry_at.is_some_and(|at| Instant::now() < at);
        let ask = !entry.ask.is_empty() && entry.asking.is_none() && !entry.unanswered && !waiting;
        let marks = entry.marks();
        if ask {
            self.lbterm_marks_ask(key);
        }
        marks
    }

    /// Takes the newest chunk of the scrollback's rows still to read, unless one is under way:
    /// read between frames on the GUI thread for the GUI's own pane, asked of the server for a
    /// mux pane's. Each chunk's end takes the next
    fn lbterm_marks_ask(&mut self, key: Key) {
        let Some(pane) = Mux::get().get_pane(key.0) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(entry) = self.lbterm_marks.panes.get_mut(&key) else {
            return;
        };
        if entry.asking.is_some() || entry.unanswered {
            return;
        }
        entry.retry_at = None;
        let Some(rows) = take_newest(&mut entry.ask, ASK_ROWS) else {
            return;
        };
        let id = ASKS.fetch_add(1, Ordering::Relaxed);
        entry.asking = Some(id);
        match pane.downcast_ref::<ClientPane>() {
            Some(client) => {
                let answer = client.renderable.lock().lbterm_prompt_rows(rows.clone());
                promise::spawn::spawn(async move {
                    let got = answer.await;
                    window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                        tw.lbterm_marks_answered(key, id, rows, got)
                    })));
                })
                .detach();
            }
            // Read after what's waiting on the GUI thread now (keys, a frame), not in this frame
            None => window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
                // Gone since: its marks go with it
                let Some(pane) = Mux::get().get_pane(key.0) else {
                    return;
                };
                let alt_screen = pane.is_alt_screen_active();
                let (looked, found) = own_prompt_rows(&*pane, rows.clone());
                let got = GetPromptRowsResponse {
                    pane_id: key.0,
                    looked,
                    rows: found,
                    alt_screen,
                };
                tw.lbterm_marks_answered(key, id, rows, Ok(got))
            }))),
        }
    }

    /// A chunk's rows read or answered: the marks they change, and the next chunk
    fn lbterm_marks_answered(
        &mut self,
        key: Key,
        id: u64,
        asked: Range<StableRowIndex>,
        got: anyhow::Result<GetPromptRowsResponse>,
    ) {
        let Some(entry) = self.lbterm_marks.panes.get_mut(&key) else {
            return;
        };
        if entry.asking != Some(id) {
            return;
        }
        entry.asking = None;
        match got {
            // The pane changed screens before its rows were looked at: asked again when it's
            // back, at the next look at this screen
            Ok(answer) if answer.alt_screen != key.1 => {
                entry.ask.add_range(asked);
                return;
            }
            Ok(answer) => {
                if entry.rows.set_range(answer.looked, &answer.rows) {
                    entry.unshown = true;
                }
            }
            // A server of a build from before GetPromptRows, which never will
            Err(err) if marks::server_lacks_prompt_rows(&format!("{err:#}")) => {
                entry.unanswered = true;
                entry.ask = RangeSet::new();
                if !UNANSWERED_SAID.swap(true, Ordering::Relaxed) {
                    log::warn!(
                        "scroll strip: the mux server doesn't know how to say which rows of pane {} \
                         hold prompts ({err:#}), so its marks are only the prompts this window has \
                         had on its screen (a server older than its GUI, until it's restarted)",
                        key.0
                    );
                }
            }
            // Anything else may not happen again (the pane respawned, the connection made
            // again): its rows are asked again at the first look a moment from now
            Err(err) => {
                log::debug!("scroll strip: asking which rows of pane {} hold prompts: {err:#}", key.0);
                entry.ask.add_range(asked);
                entry.retry_at = Some(Instant::now() + RETRY_AFTER);
                return;
            }
        }
        if !entry.ask.is_empty() && !entry.unanswered {
            self.lbterm_marks_ask(key);
            return;
        }
        // The last chunk: one frame for what they all changed, if the marks show (the active
        // pane's only)
        let unshown = std::mem::take(&mut entry.unshown);
        if unshown && self.get_active_pane_or_overlay().map(|p| p.pane_id()) == Some(key.0) {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    /// Draw the active pane's marks on the scrollbar's track: `track_top` and `track_height`
    /// are the track the thumb moves in, `x` and `width` the scrollbar's column
    pub(crate) fn paint_lbterm_marks(
        &mut self,
        layers: &mut crate::quad::TripleLayerQuadAllocator,
        pane: &Arc<dyn Pane>,
        x: f32,
        width: f32,
        track_top: usize,
        track_height: usize,
    ) -> anyhow::Result<()> {
        let rows = self.lbterm_prompt_marks(pane);
        let dims = pane.get_dimensions();
        let scale = self.dimensions.dpi as f32 / 96.0;
        let mark_height = (2.0 * scale).round().max(1.0) as usize;
        let ys = marks::track_positions(
            &rows,
            dims.scrollback_top,
            dims.scrollback_rows,
            track_height,
            mark_height,
        );
        let inset = (width * 0.2).round();
        let colour = LinearRgba::with_srgba(MARK.0, MARK.1, MARK.2, 0xe6);
        for y in ys {
            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    x + inset,
                    (track_top + y) as f32,
                    (width - 2.0 * inset).max(1.0),
                    mark_height as f32,
                ),
                colour,
            )?;
        }
        self.lbterm_marks.drawn = Some(Drawn {
            pane: pane.pane_id(),
            track_top,
            track_height,
            mark_height,
            marks: rows,
            first_row: dims.scrollback_top,
            total_rows: dims.scrollback_rows,
        });
        Ok(())
    }

    /// A click on the scrollbar's track: on a mark, scroll so its prompt is at the top and
    /// return true; anywhere else, false (and the click pages as usual)
    pub(crate) fn lbterm_mark_click(&mut self, pane: &Arc<dyn Pane>, y: isize) -> bool {
        let Some(drawn) = &self.lbterm_marks.drawn else {
            return false;
        };
        if drawn.pane != pane.pane_id() || y < drawn.track_top as isize {
            return false;
        }
        let scale = self.dimensions.dpi as f32 / 96.0;
        let Some(row) = marks::mark_at(
            &drawn.marks,
            drawn.first_row,
            drawn.total_rows,
            drawn.track_height,
            drawn.mark_height,
            y as usize - drawn.track_top,
            (SLOP * scale).round() as usize,
        ) else {
            return false;
        };
        let dims = pane.get_dimensions();
        self.set_viewport(pane.pane_id(), Some(row), dims);
        // LBTerm: the prompt it scrolled to is marked for a moment, as a prompt jump's is
        self.lbterm_mark_landed(pane.pane_id(), row);
        true
    }
}

/// LBTerm: the scrollbar's marks, for the state file (lbterm/STATE.md)
impl TermWindow {
    /// How many prompt marks the scrollbar showed for `pane` when it last drew them
    pub(crate) fn lbterm_state_marks(&self, pane: PaneId) -> usize {
        self.lbterm_marks
            .drawn
            .as_ref()
            .filter(|d| d.pane == pane)
            .map_or(0, |d| d.marks.len())
    }
}

