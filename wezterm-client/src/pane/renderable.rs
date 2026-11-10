use crate::domain::ClientInner;
use crate::pane::clientpane::ClientPane;
use crate::pane::lbterm_poll::PollPace;
use anyhow::anyhow;
use codec::*;
use config::configuration;
use lru::LruCache;
use mux::pane::PaneId;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::Mux;
use promise::BrokenPromise;
use rangeset::*;
use ratelim::RateLimiter;
use std::cell::RefCell;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use termwiz::cell::{Cell, CellAttributes, Underline};
use termwiz::color::AnsiColor;
use termwiz::hyperlink::Rule;
use termwiz::image::{ImageCell, ImageData};
use termwiz::surface::{SequenceNo, SEQ_ZERO};
use url::Url;
use wezterm_term::{KeyCode, KeyModifiers, Line, StableRowIndex};

#[derive(Debug)]
enum LineEntry {
    // Up to date wrt. server and has been rendered at least once
    Line(Line),
    // Currently being downloaded from the server
    Fetching(Instant),
    // We have a version of the line locally and are treating it
    // as needing rendering because we are also in the process of
    // downloading a newer version from the server
    LineAndFetching(Line, Instant),
    // We have a local copy but it is stale and will need to be
    // fetched again
    Stale(Line),
}

impl LineEntry {
    fn kind(&self) -> (&'static str, Option<Instant>) {
        match self {
            Self::Line(_) => ("Line", None),
            Self::Fetching(since) => ("Fetching", Some(*since)),
            Self::LineAndFetching(_, since) => ("LineAndFetching", Some(*since)),
            Self::Stale(_) => ("Stale", None),
        }
    }
}

/// Every line `lines` holds made stale where it is, the cache's size and order kept. LBTerm:
/// upstream's make_all_stale emptied it into an unbounded cache, so after a pane's first resize (a
/// mux domain's every tab is resized as its window lays it out) or palette change, every line
/// ever pushed to the pane stayed in the window for good: six tabs printing without a pause grew a
/// test window by 20 to 30 MB a second (perf/mux-flood.ps1, 2026-10-30), 11.5 GB in seven minutes
/// on 2026-10-29. perf/mux-server kept the size by emptying it into a new cache of the same size,
/// which still freed and made a node for every line held, for every pane of every tab at each step
/// of a window's resize, a zoom or a palette (perf/mux-client): now each entry changes in place,
/// and nothing is freed or made
fn all_stale(lines: &mut LruCache<StableRowIndex, LineEntry>, now: Instant) {
    for (_, entry) in lines.iter_mut() {
        if let LineEntry::Line(_) = entry {
            if let LineEntry::Line(old) = std::mem::replace(entry, LineEntry::Fetching(now)) {
                *entry = LineEntry::Stale(old);
            }
        }
    }
}

/// LBTerm: what a push's dirty rows above the viewport do to the cache: each leaves it (upstream
/// popped the row, then asked make_stale to keep it Stale, which popped it again and found
/// nothing), so a line is fetched again when it's next read. Upstream walked every row of the
/// range, which after a resize is the pane's whole scrollback; this walks the range or the cache,
/// whichever is shorter, and leaves the cache as upstream's walk did
fn drop_rows_above(lines: &mut LruCache<StableRowIndex, LineEntry>, rows: Range<StableRowIndex>) {
    if rows.is_empty() {
        return;
    }
    if (rows.end - rows.start) as usize > lines.len() {
        let held: Vec<StableRowIndex> = lines
            .iter()
            .map(|(row, _)| *row)
            .filter(|row| rows.contains(row))
            .collect();
        for row in held {
            lines.pop(&row);
        }
    } else {
        for row in rows {
            lines.pop(&row);
        }
    }
}

/// LBTerm: the rows a push that moves the cursor names besides its own: the row it moves to, whose
/// line the push brings, and the row it leaves only when local echo drew a guess into that row
/// (`predicted`), which only the server's line puts right. Upstream fetched the row left at every
/// move, the server's word that it hadn't changed overruled: after a resize, whose rewrap can move
/// the cursor down a row and leave the row it was on as it was, each pane fetched that row
/// (perf/mux-resize-rows.ps1's drag, the three tabs out of view taking the drag's last size, one
/// GetLines each). A guess is drawn only while the server is slow to answer (should_predict)
fn cursor_moved(
    dirty: &mut RangeSet<StableRowIndex>,
    predicted: &RangeSet<StableRowIndex>,
    left: StableRowIndex,
    to: StableRowIndex,
) {
    if predicted.contains(left) {
        dirty.add(left);
    }
    dirty.add(to);
}

/// LBTerm: what a push's dirty rows do to the cache, its bonus lines put already and taken out of
/// `dirty` (apply_changes_to_surface, upstream's loop): a row above the viewport (`physical_top`)
/// leaves it, to be fetched when it's next read; a row in view is fetched, the line held kept to
/// draw meanwhile. Returns the rows to fetch. These are the rows a resize's rewrap changed too:
/// the push after it is the server's word on which (term's lbterm_rewrap; PITFALLS 142), so a
/// resize makes nothing else stale (clientpane.rs, resize)
fn rows_named(
    lines: &mut LruCache<StableRowIndex, LineEntry>,
    dirty: &RangeSet<StableRowIndex>,
    physical_top: StableRowIndex,
    now: Instant,
) -> RangeSet<StableRowIndex> {
    let mut to_fetch = RangeSet::new();
    for r in dirty.iter() {
        // LBTerm: a dirty row above the viewport only leaves the cache, so only rows the cache
        // holds are looked at there: after a resize the server says the pane's whole
        // scrollback is dirty (50,000 rows), which this walked a row at a time, two lookups a
        // row, for each pane's push (drop_rows_above)
        let above = r.start..r.end.min(physical_top).max(r.start);
        drop_rows_above(lines, above);
        for stable_row in r.start.max(physical_top)..r.end {
            // If a line is in the (probable) viewport region,
            // then we'll likely want to fetch it.
            // If it is outside that region, remove it from our cache
            // so that we'll fetch it on demand later.
            let prior = lines.pop(&stable_row);
            let prior_kind = prior.as_ref().map(|e| e.kind());
            to_fetch.add(stable_row);
            let entry = match prior {
                Some(LineEntry::Fetching(_)) | None => LineEntry::Fetching(now),
                Some(LineEntry::LineAndFetching(old, ..))
                | Some(LineEntry::Stale(old))
                | Some(LineEntry::Line(old)) => LineEntry::LineAndFetching(old, now),
            };
            log::trace!(
                "row {} {:?} -> {:?} due to dirty and IN viewport",
                stable_row,
                prior_kind,
                entry.kind()
            );
            lines.put(stable_row, entry);
        }
    }
    to_fetch
}

/// LBTerm: each line held for `rows` scanned for `rules`' implicit hyperlinks, in place, once
/// (a line says it was scanned until it's replaced). Upstream scanned every line as it came
/// (put_line), so every push of a busy tab out of view, and every line replaced before it was
/// ever drawn, was scanned on the GUI thread: with Lachlan's rules (WezTerm's and two of his) a
/// flooding tab's line took about 80 µs and its push 3.3 ms, and six of them out of view held
/// the GUI thread 64% of the time, 0.2 ms a push with no rules (perf/mux-flood.ps1, 2026-10-31).
/// Now a line is scanned when it's drawn or looked at for a link (ClientPane::apply_hyperlinks,
/// which the renderer and the pointer call first, as for a local pane), the same way as before:
/// each line on its own. A line whose text this pane matched in the last two screens of rows it
/// drew (`shown` a screen) takes those matches, as a line that only moved (lbterm_links.rs)
fn scan_rows(
    lines: &mut LruCache<StableRowIndex, LineEntry>,
    rows: Range<StableRowIndex>,
    rules: &[Rule],
    kept: &mut lbterm_links::KeptScans,
    shown: usize,
) {
    if rules.is_empty() {
        return;
    }
    for row in rows {
        match lines.peek_mut(&row) {
            Some(LineEntry::Line(line))
            | Some(LineEntry::LineAndFetching(line, _))
            | Some(LineEntry::Stale(line)) => kept.scan(line, rules, shown),
            Some(LineEntry::Fetching(_)) | None => {}
        }
    }
}

/// LBTerm: the server holds the pane from its push of `seqno`, the pane's last push applied
/// being `applied`'s: held (`held` set to `seqno` plus one) unless a push past it is applied
/// already. Whether it's held
fn lbterm_hold_from(held: &AtomicU64, applied: SequenceNo, seqno: SequenceNo) -> bool {
    if applied > seqno {
        return false;
    }
    held.store(seqno as u64 + 1, Ordering::Release);
    true
}

/// LBTerm: the connection that said the pane is held has ended, and a new one is up: a hold of
/// it (`held`) is said again from no push, so the new connection's first push of the pane, of
/// whatever seqno, ends it (a server started afresh numbers from its own start). Whether it was
/// held, and so needs that push asked for: the new connection owes nothing for a pane quiet since
/// and holds nothing of it, so nothing would come unasked (PITFALLS 201)
pub(crate) fn lbterm_hold_reconnected(held: &AtomicU64) -> bool {
    held.fetch_update(Ordering::AcqRel, Ordering::Acquire, |from| {
        (from != 0).then_some(1)
    })
    .is_ok()
}

/// LBTerm: a push of `seqno` applied: the server's hold of the pane (`held`, its seqno plus one)
/// ends when it's past the seqno the hold was said from, counted in `ended`
fn lbterm_hold_ends(held: &AtomicU64, ended: &AtomicU64, seqno: SequenceNo) {
    let from = held.load(Ordering::Acquire);
    if from != 0 && seqno as u64 >= from {
        held.store(0, Ordering::Release);
        ended.fetch_add(1, Ordering::AcqRel);
    }
}

/// LBTerm: a push or a fetch answer brought the pane rows (`RenderableInner::lbterm_rows`)
fn lbterm_rows_news(rows: &AtomicU64) {
    rows.fetch_add(1, Ordering::Release);
}

/// LBTerm: whether any line a push carries reads otherwise than the line held at its row. The
/// server sends the cursor's row in every push ("always send the cursor's row", sessionhandler.rs),
/// so a push of a title alone carries that line again as it was. Its text alone, since a scan for
/// links sees only that (lbterm_links.rs); a row held stale, being fetched with no line, or not
/// held counts as new. Stops at the first new one, so a push of new output compares one line
fn brings_new_text(
    lines: &LruCache<StableRowIndex, LineEntry>,
    carried: &[(StableRowIndex, Line)],
) -> bool {
    carried.iter().any(|(row, line)| match lines.peek(row) {
        Some(LineEntry::Line(held)) | Some(LineEntry::LineAndFetching(held, _)) => {
            held.as_str() != line.as_str()
        }
        _ => true,
    })
}

#[cfg(test)]
#[path = "lbterm_renderable_test.rs"]
mod lbterm_test;

pub struct RenderableInner {
    pub client: Arc<ClientInner>,
    remote_pane_id: PaneId,
    local_pane_id: PaneId,
    pub dead: bool,
    poll_in_progress: AtomicBool,
    /// LBTerm: when to poll, upstream's last_poll and poll_interval (lbterm_poll.rs)
    poll: PollPace,

    cursor_position: StableCursorPosition,
    pub dimensions: RenderableDimensions,

    lines: LruCache<StableRowIndex, LineEntry>,
    /// LBTerm: the links found in the lines drawn, kept by text (lbterm_links.rs)
    link_scans: lbterm_links::KeptScans,
    /// LBTerm: the rows local echo drew a guess into (predict_from_key_event, predict_from_paste)
    /// that no push has named since: only the server's line puts one right (cursor_moved)
    lbterm_predicted: RangeSet<StableRowIndex>,
    /// LBTerm: how many pushes and fetch answers have brought this pane rows, a line whose text
    /// it didn't hold or a row named changed (`lbterm_rows_news`, `brings_new_text`): a push of
    /// the title, the folder or the cursor alone, which carries the cursor's row as it was,
    /// leaves it. `ClientPane` keeps a clone and reads it in the GUI's mux subscription, which a
    /// push's PaneOutput reaches while this is borrowed, for whether a tab out of view has a
    /// screen to scan again (lbterm_extras::hidden_scan)
    pub(crate) lbterm_rows: Arc<AtomicU64>,
    /// LBTerm: the server's word that it holds this pane's lines (PaneHeld): the seqno it holds
    /// them from, plus one, or 0 while it holds none. Set by `lbterm_held_from`, cleared by the
    /// first push past it as that push is applied, before its PaneOutput. `ClientPane` keeps a
    /// clone, read without this one's lock by the window's frame wait (lbterm/MUX.md, "Tabs out
    /// of view: output held at the server")
    pub(crate) lbterm_held: Arc<AtomicU64>,
    /// LBTerm: how many of those holds a push ended, for hidden_scan's scan at once
    /// (`ClientPane::lbterm_holds_ended`)
    pub(crate) lbterm_holds_ended: Arc<AtomicU64>,
    /// LBTerm: the newest push applied and whether it's been said read since, for the server's
    /// pace of a shown pane (lbterm_drawn.rs): set here as each push is applied, taken without
    /// this one's lock through `ClientPane`'s clone by the window's paint
    pub(crate) lbterm_drawn: Arc<crate::lbterm_drawn::Drawn>,
    pub title: String,
    pub working_dir: Option<Url>,
    pub seqno: SequenceNo,

    fetch_limiter: RateLimiter,

    last_send_time: Instant,
    pub last_recv_time: Instant,
    last_late_dirty: Instant,
    last_input_rtt: u64,

    pub input_serial: InputSerial,
}

pub struct RenderableState {
    pub inner: RefCell<RenderableInner>,
}

impl RenderableInner {
    pub fn new(
        client: &Arc<ClientInner>,
        remote_pane_id: PaneId,
        local_pane_id: PaneId,
        dimensions: RenderableDimensions,
        title: &str,
        fetch_limiter: RateLimiter,
    ) -> Self {
        let now = Instant::now();

        Self {
            client: Arc::clone(client),
            remote_pane_id,
            local_pane_id,
            dead: false,
            poll_in_progress: AtomicBool::new(false),
            poll: PollPace::new(client.client.is_local, now),
            link_scans: lbterm_links::KeptScans::new(),
            lbterm_predicted: RangeSet::new(),
            lbterm_rows: Arc::new(AtomicU64::new(0)),
            lbterm_held: Arc::new(AtomicU64::new(0)),
            lbterm_holds_ended: Arc::new(AtomicU64::new(0)),
            lbterm_drawn: Arc::new(crate::lbterm_drawn::Drawn::default()),
            cursor_position: StableCursorPosition::default(),
            dimensions,
            lines: LruCache::new(
                NonZeroUsize::new(configuration().scrollback_lines.max(128)).unwrap(),
            ),
            title: title.to_string(),
            working_dir: None,
            fetch_limiter,
            last_send_time: now,
            last_recv_time: now,
            last_late_dirty: now,
            last_input_rtt: 0,
            input_serial: InputSerial::empty(),
            seqno: SEQ_ZERO,
        }
    }

    /// Returns true if we think we should display the laggy connection
    /// indicator.  If we're past our poll interval and more recently
    /// tried to send something than receive something, the UI is worth
    /// showing.
    pub fn is_tardy(&self) -> bool {
        let elapsed = self.last_recv_time.elapsed();
        // LBTerm: 3 s after input, as upstream's came to (lbterm_poll.rs, tardy_after)
        if elapsed > self.poll.tardy_after() {
            self.last_send_time > self.last_recv_time
        } else {
            false
        }
    }

    /// Predictive echo can be noisy when the link is working well,
    /// so we only employ it when it looks like the latency is high.
    /// LBTerm: public, so the GUI asks a key's own frame for the guess (wezterm-gui's
    /// termwindow/echo.rs)
    pub fn should_predict(&self) -> bool {
        self.client
            .local_echo_threshold_ms
            .map(|thresh| self.last_input_rtt >= thresh)
            .unwrap_or(false)
    }

    /// Compute a "prediction" and apply it to the line data that we
    /// have available, marking it as dirty so that it gets rendered.
    /// The prediction is basically just local echo.
    /// Open questions:
    /// how do we tell if the intent is to suppress local echo during eg:
    ///  * password prompt?  One option is to look back and see if the line
    ///                      looks like a password prompt.
    ///  * normal mode in vim: letter presses are typically movement or
    ///                        other editor commands
    /// There are bound to be a number of other edge cases that we should
    /// handle.
    fn apply_prediction(&mut self, c: KeyCode, line: &mut Line) {
        let text = line.as_str();
        if text.contains("sword") {
            // This line might be a password prompt.  Don't force
            // on local echo here, as we don't want to reveal content
            // from their password
            return;
        }
        // LBTerm: the guess changes the line without moving its sequence number, and the GUI
        // keeps the key its shapes are cached by on the line itself, by that number
        // (lbterm_lent.rs): forgotten, so the guess is drawn
        line.clear_appdata();

        match c {
            KeyCode::Enter => {
                self.cursor_position.x = 0;
                self.cursor_position.y += 1;
            }
            KeyCode::UpArrow => {
                self.cursor_position.y = self.cursor_position.y.saturating_sub(1);
            }
            KeyCode::DownArrow => {
                self.cursor_position.y += 1;
            }
            KeyCode::RightArrow => {
                self.cursor_position.x += 1;
            }
            KeyCode::LeftArrow => {
                self.cursor_position.x = self.cursor_position.x.saturating_sub(1);
            }
            KeyCode::Delete => {
                line.erase_cell(self.cursor_position.x, SEQ_ZERO);
            }
            KeyCode::Backspace => {
                if self.cursor_position.x > 0 {
                    line.erase_cell(self.cursor_position.x - 1, SEQ_ZERO);
                    self.cursor_position.x -= 1;
                }
            }
            KeyCode::Char(c) => {
                let cell = Cell::new(
                    c,
                    CellAttributes::default()
                        .set_underline(Underline::Double)
                        .clone(),
                );

                let width = cell.width();
                line.set_cell(self.cursor_position.x, cell, SEQ_ZERO);
                // Adjust the cursor to reflect the width of this new cell
                self.cursor_position.x += width;
            }
            _ => {}
        }
    }

    /// Based on a keypress, apply a "prediction" of what the terminal
    /// content will look like once we receive the response from the
    /// remote system.  The prediction helps to reduce perceived latency
    /// when a user is typing at any reasonable velocity.
    pub fn predict_from_key_event(&mut self, key: KeyCode, mods: KeyModifiers) {
        if !self.should_predict() {
            return;
        }

        let c = match key {
            KeyCode::LeftArrow
            | KeyCode::RightArrow
            | KeyCode::UpArrow
            | KeyCode::DownArrow
            | KeyCode::Delete
            | KeyCode::Backspace
            | KeyCode::Enter
            | KeyCode::Char(_) => key,
            _ => return,
        };
        if mods != KeyModifiers::NONE && mods != KeyModifiers::SHIFT {
            return;
        }

        let row = self.cursor_position.y;
        match self.lines.pop(&row) {
            Some(LineEntry::Stale(mut line)) | Some(LineEntry::Line(mut line)) => {
                self.apply_prediction(c, &mut line);
                self.lines.put(row, LineEntry::Line(line));
                self.lbterm_predicted.add(row);
            }
            Some(LineEntry::LineAndFetching(mut line, instant)) => {
                self.apply_prediction(c, &mut line);
                self.lines
                    .put(row, LineEntry::LineAndFetching(line, instant));
                self.lbterm_predicted.add(row);
            }
            Some(entry) => {
                self.lines.put(row, entry);
            }
            None => {}
        }
    }

    fn apply_paste_prediction(&mut self, row: usize, text: &str, line: &mut Line) {
        let attrs = CellAttributes::default()
            .set_underline(Underline::Double)
            .clone();

        let text_line = Line::from_text(text, &attrs, SEQ_ZERO, None);
        // LBTerm: as apply_prediction
        line.clear_appdata();

        if row == 0 {
            for cell in text_line.visible_cells() {
                line.set_cell(self.cursor_position.x, cell.as_cell(), SEQ_ZERO);
                self.cursor_position.x += cell.width();
            }
        } else {
            // The pasted line replaces the data for the existing line
            line.resize_and_clear(0, SEQ_ZERO, CellAttributes::default());
            line.append_line(text_line, SEQ_ZERO);
            self.cursor_position.x = line.len();
        }
    }

    pub fn predict_from_paste(&mut self, text: &str) {
        if !self.should_predict() {
            return;
        }

        let text = textwrap::fill(text, self.dimensions.cols);
        let lines: Vec<&str> = text.split("\n").collect();

        for (idx, paste_line) in lines.iter().enumerate() {
            let row = self.cursor_position.y + idx as StableRowIndex;

            match self.lines.pop(&row) {
                Some(LineEntry::Stale(mut line)) | Some(LineEntry::Line(mut line)) => {
                    self.apply_paste_prediction(idx, paste_line, &mut line);
                    self.lines.put(row, LineEntry::Line(line));
                    self.lbterm_predicted.add(row);
                }
                Some(LineEntry::LineAndFetching(mut line, instant)) => {
                    self.apply_paste_prediction(idx, paste_line, &mut line);
                    self.lines
                        .put(row, LineEntry::LineAndFetching(line, instant));
                    self.lbterm_predicted.add(row);
                }
                Some(entry) => {
                    self.lines.put(row, entry);
                }
                None => {}
            }
        }
        self.cursor_position.y += lines.len().saturating_sub(1) as StableRowIndex;
    }

    pub fn update_last_send(&mut self) {
        self.last_send_time = Instant::now();
        self.poll.sent();
    }

    /// LBTerm: the push applied last said read to the server now, when it hasn't been, as one
    /// PanesDrawn of this pane alone (lbterm_drawn.rs): for a push no frame will read
    pub(crate) fn lbterm_say_drawn(&self) {
        if !self.client.client.lbterm_says_drawn() {
            return;
        }
        if let Some(seqno) = self.lbterm_drawn.take() {
            self.client.client.lbterm_panes_drawn(vec![codec::PaneDrawn {
                pane_id: self.remote_pane_id,
                seqno,
            }]);
        }
    }

    /// LBTerm: `delta` applied here, on the client thread as it's read, when it's a push of the
    /// pane's title and folder alone that changes nothing else this pane holds
    /// (lbterm_names.rs): what `apply_changes_to_surface` would leave, without its PaneOutput,
    /// which for such a push only told the window of a title it hears of in the alert after it.
    /// Its title and folder are where `get_title` and `get_current_working_dir` read them
    /// (PITFALLS 163), and its word is said as every push's is (lbterm_drawn.rs). Whether it
    /// was; when not, it goes to the GUI thread as any push. The pane's mouse and screen flags,
    /// which ClientPane keeps, are the caller's to compare (`flags_same`)
    pub(crate) fn lbterm_names_here(
        &mut self,
        delta: &GetPaneRenderChangesResponse,
        flags_same: bool,
    ) -> bool {
        if !flags_same
            || self.should_predict()
            || !crate::lbterm_names::current(
                delta,
                self.seqno,
                &self.cursor_position,
                &self.dimensions,
                self.lbterm_held.load(Ordering::Acquire),
            )
        {
            return false;
        }
        let changes = delta.seqno != self.seqno
            || delta.title != self.title
            || delta.working_dir.as_ref().map(|u| &u.url) != self.working_dir.as_ref();
        self.poll.pushed();
        self.last_recv_time = Instant::now();
        self.title = delta.title.clone();
        self.working_dir = delta.working_dir.clone().map(Into::into);
        self.seqno = delta.seqno;
        self.lbterm_drawn.applied(delta.seqno);
        if !changes {
            // As the apply says a push that changes nothing: no frame will read it
            self.lbterm_say_drawn();
        }
        true
    }

    /// LBTerm: `flags_changed` says the delta changed whether the mouse is grabbed or the
    /// alternate screen is up, which the GUI draws from too (the scrollbar hides for either)
    pub fn apply_changes_to_surface(
        &mut self,
        delta: GetPaneRenderChangesResponse,
        bonus_lines: Vec<(StableRowIndex, Line)>,
        flags_changed: bool,
    ) {
        log::trace!(
            "apply_changes_to_surface local={} remote={}",
            self.local_pane_id,
            self.remote_pane_id
        );
        let now = Instant::now();
        self.poll.pushed();
        self.last_recv_time = now;

        // LBTerm: for whether it changes anything (below)
        let lbterm_dirty = !delta.dirty_lines.is_empty();
        let mut dirty = RangeSet::new();
        for r in delta.dirty_lines {
            dirty.add_range(r.clone());
        }
        if delta.cursor_position != self.cursor_position {
            // But note that the server may have sent this in bonus_lines;
            // we'll address that below
            // LBTerm: the row left is fetched again only when local echo drew a guess into it
            // (cursor_moved)
            cursor_moved(
                &mut dirty,
                &self.lbterm_predicted,
                self.cursor_position.y,
                delta.cursor_position.y,
            );
        }
        // LBTerm: each row this push names is put right, by its line or a fetch, a guess drawn
        // into it with it
        self.lbterm_predicted.remove_set(&dirty);

        // Keep track of the approximate round trip time by recording how
        // long it took for this response to come back
        if let Some(serial) = delta.input_serial {
            self.last_input_rtt = serial.elapsed_millis();
        }

        // LBTerm: whether the delta changes anything the pane shows (lbterm/PERF.md, finding
        // 68). The server answers every key with one straight away, before the program has
        // echoed it, for local echo's cursor, and its PaneOutput painted a frame that drew
        // nothing new and held the frame that drew the echo a pacing period behind it. A delta
        // with the server's seqno where it was, no lines dirty, the cursor, the size, the title
        // and the folder as they were, and every line it brings one held already says nothing:
        // no frame for it. Local echo (a guess the GUI drew, which the server's answer puts
        // right) and a line the cache doesn't hold still notify, as upstream did for every delta.
        let lbterm_changes = flags_changed
            || delta.seqno != self.seqno
            || lbterm_dirty
            || delta.cursor_position != self.cursor_position
            || delta.dimensions != self.dimensions
            || delta.title != self.title
            || delta.working_dir.as_ref().map(|u| &u.url) != self.working_dir.as_ref()
            || bonus_lines
                .iter()
                .any(|(row, _)| !matches!(self.lines.peek(row), Some(LineEntry::Line(_))))
            || self.should_predict();

        // When it comes to updating the cursor position, if the update was tagged
        // with keyboard input, we'll only take the position if the update comes from
        // the most recent key event.  This helps to prevent the cursor wiggling if the
        // user is typing more than one character per roundtrip interval--the wiggle
        // manifests because we may have already predicted a local cursor move forwards
        // by one character, and we may receive the response to the prior update after
        // we have rendered that, and then shortly receive the most recent response.
        // The result of that is that the cursor moves right one, left one and then
        // finally right one in quick succession.
        // If the delta was not from an input event then we trust it; this is most
        // like due to a unilateral movement by the application on the other end.
        if delta.input_serial.is_none()
            || delta.input_serial.unwrap_or(InputSerial::empty()) >= self.input_serial
        {
            self.cursor_position = delta.cursor_position;
        }
        self.dimensions = delta.dimensions;
        self.title = delta.title;
        self.working_dir = delta.working_dir.map(Into::into);
        log::trace!(
            "server says: seqno from {} -> {} for local_pane_id={}",
            self.seqno,
            delta.seqno,
            self.local_pane_id
        );
        self.seqno = delta.seqno;
        // LBTerm: the next frame that reads the pane says this push (lbterm_drawn.rs)
        self.lbterm_drawn.applied(delta.seqno);
        // LBTerm: a push past the seqno the server said it holds the pane from ends the hold,
        // before its PaneOutput, which the window's frame wait and hidden_scan take it from
        lbterm_hold_ends(&self.lbterm_held, &self.lbterm_holds_ended, delta.seqno);

        // LBTerm: whether a line it carries reads otherwise than the one held, looked at before
        // they go in (brings_new_text)
        let lbterm_new_text = brings_new_text(&self.lines, &bonus_lines);

        for (stable_row, line) in bonus_lines {
            log::trace!("bonus line {} seqno={}", stable_row, line.current_seqno());
            self.put_line(stable_row, line, None);
            dirty.remove(stable_row);
        }

        // LBTerm: news for the scan of a tab out of view (lbterm_rows): a line whose text is new,
        // or a row named changed that it didn't carry. Every push carries the cursor's row, so
        // one of the title, the folder or the cursor alone brings that line again and isn't
        // news. Counted before its PaneOutput, which the GUI's subscription takes as it's
        // notified
        if lbterm_new_text || !dirty.is_empty() {
            lbterm_rows_news(&self.lbterm_rows);
        }

        log::trace!(
            "apply_changes_to_surface: Generate PaneOutput event for local={}",
            self.local_pane_id
        );
        if lbterm_changes {
            Mux::get().notify(mux::MuxNotification::PaneOutput(self.local_pane_id));
        } else {
            log::debug!(
                target: "lbterm::echo",
                "pane {}: a delta that changes nothing, no PaneOutput",
                self.local_pane_id
            );
            // LBTerm: it asks for no frame, so nothing a frame will read is new: said read now,
            // or a server pacing the pane would wait on a frame that never comes (lbterm_drawn.rs)
            self.lbterm_say_drawn();
        }

        log::trace!("dirty as of seq {} -> {:?}", delta.seqno, dirty);
        let to_fetch = rows_named(&mut self.lines, &dirty, delta.dimensions.physical_top, now);
        if !to_fetch.is_empty() {
            if self.fetch_limiter.non_blocking_admittance_check(1) {
                self.schedule_fetch_lines(to_fetch, now);
            } else {
                log::warn!(
                    "exceeded fetch throttle, drop {:?} and mark stale",
                    to_fetch
                );
                for r in to_fetch.iter() {
                    for stable_row in r.clone() {
                        self.make_stale(stable_row);
                    }
                }
            }
        }
    }

    /// LBTerm: the server says it holds this pane's lines from its push of `seqno` (PaneHeld):
    /// held until a push past it, unless one is applied already, which the client thread's
    /// merge of a pane's pushes into the first one waiting can bring ahead of the word
    /// (lbterm_backlog.rs). Whether it's held
    pub fn lbterm_held_from(&mut self, seqno: SequenceNo) -> bool {
        lbterm_hold_from(&self.lbterm_held, self.seqno, seqno)
    }

    pub fn make_all_stale(&mut self) {
        all_stale(&mut self.lines, Instant::now());
    }

    fn make_stale(&mut self, stable_row: StableRowIndex) {
        match self.lines.pop(&stable_row) {
            Some(LineEntry::Stale(old))
            | Some(LineEntry::Line(old))
            | Some(LineEntry::LineAndFetching(old, _)) => {
                self.lines.put(stable_row, LineEntry::Stale(old));
            }
            Some(LineEntry::Fetching(_)) | None => {}
        }
    }

    /// LBTerm: each line held for `rows` scanned for implicit hyperlinks, once (scan_rows): what
    /// put_line did to every line as it came
    pub fn apply_hyperlinks(&mut self, rows: Range<StableRowIndex>, rules: &[Rule]) {
        scan_rows(
            &mut self.lines,
            rows,
            rules,
            &mut self.link_scans,
            self.dimensions.viewport_rows,
        );
    }

    fn put_line(
        &mut self,
        stable_row: StableRowIndex,
        mut line: Line,
        fetch_start: Option<Instant>,
    ) {
        // LBTerm: not scanned for hyperlinks here, but as it's drawn (scan_rows)
        let entry = if let Some(fetch_start) = fetch_start {
            // If we're completing a fetch, only replace entries that were
            // set to fetching as part of our fetch.  If they are now longer
            // tagged that way, then someone came along after us and changed
            // the state, so we should leave it alone

            match self.lines.pop(&stable_row) {
                Some(LineEntry::LineAndFetching(_, then)) | Some(LineEntry::Fetching(then))
                    if fetch_start == then =>
                {
                    log::trace!(
                        "row {} fetch done -> Line seq={} vs self.seq={}",
                        stable_row,
                        line.current_seqno(),
                        self.seqno
                    );
                    line.update_last_change_seqno(self.seqno);
                    LineEntry::Line(line)
                }
                Some(e) => {
                    // It changed since we started: leave it alone!
                    log::trace!(
                        "row {} {:?} changed since fetch started at {:?}, so leave it be",
                        stable_row,
                        e.kind(),
                        fetch_start
                    );
                    self.lines.put(stable_row, e);
                    return;
                }
                None => return,
            }
        } else {
            LineEntry::Line(line)
        };
        self.lines.put(stable_row, entry);
    }

    fn schedule_fetch_lines(&mut self, to_fetch: RangeSet<StableRowIndex>, now: Instant) {
        if to_fetch.is_empty() || self.dead {
            return;
        }

        let local_pane_id = self.local_pane_id;
        log::trace!(
            "will fetch lines {:?} for remote tab id {} at {:?}",
            to_fetch,
            self.remote_pane_id,
            now,
        );

        let client = Arc::clone(&self.client);
        let remote_pane_id = self.remote_pane_id;

        promise::spawn::spawn(async move {
            let result = client
                .client
                .get_lines(GetLines {
                    pane_id: remote_pane_id,
                    lines: to_fetch.clone().into(),
                })
                .await;

            let result = match result {
                Ok(result) => {
                    let lines =
                        hydrate_lines(Arc::clone(&client), remote_pane_id, result.lines).await;
                    Ok(lines)
                }
                Err(err) => Err(err),
            };
            Self::apply_lines(local_pane_id, result, to_fetch, now)
        })
        .detach();
    }

    fn apply_lines(
        local_pane_id: PaneId,
        result: anyhow::Result<Vec<(StableRowIndex, Line)>>,
        to_fetch: RangeSet<StableRowIndex>,
        now: Instant,
    ) -> anyhow::Result<()> {
        let mux = Mux::get();
        let pane = mux
            .get_pane(local_pane_id)
            .ok_or_else(|| anyhow!("no such tab {}", local_pane_id))?;
        if let Some(client_tab) = pane.downcast_ref::<ClientPane>() {
            let renderable = client_tab.renderable.lock();
            let mut inner = renderable.inner.borrow_mut();

            match result {
                Ok(lines) => {
                    log::trace!("fetch complete for {:?} at {:?}", to_fetch, now);
                    // LBTerm: lines the pane holds now, news for a scan (lbterm_rows)
                    if !lines.is_empty() {
                        lbterm_rows_news(&inner.lbterm_rows);
                    }
                    for (stable_row, line) in lines.into_iter() {
                        inner.put_line(stable_row, line, Some(now));
                    }
                }
                Err(err) => {
                    log::error!("get_lines failed: {}", err);
                    for r in to_fetch.iter() {
                        for stable_row in r.clone() {
                            let entry = match inner.lines.pop(&stable_row) {
                                Some(LineEntry::Fetching(then)) if then == now => {
                                    // leave it popped
                                    continue;
                                }
                                Some(LineEntry::LineAndFetching(line, then)) if then == now => {
                                    // revert to just a line
                                    LineEntry::Line(line)
                                }
                                Some(entry) => entry,
                                None => continue,
                            };
                            inner.lines.put(stable_row, entry);
                        }
                    }
                }
            }
        }
        log::trace!(
            "Generate PaneOutput event for local_pane_id={}",
            local_pane_id
        );
        mux.notify(mux::MuxNotification::PaneOutput(local_pane_id));
        Ok(())
    }

    fn poll(&mut self) -> anyhow::Result<()> {
        if self.poll_in_progress.load(Ordering::SeqCst) {
            // We have a poll in progress
            return Ok(());
        }

        if !self.poll.due(Instant::now()) {
            return Ok(());
        }

        self.poll_in_progress.store(true, Ordering::SeqCst);
        let remote_pane_id = self.remote_pane_id;
        let local_pane_id = self.local_pane_id;
        let client = Arc::clone(&self.client);
        promise::spawn::spawn(ask_changes(client, remote_pane_id, local_pane_id, true)).detach();
        Ok(())
    }
}

/// LBTerm: the pane's changes asked of its server now (GetPaneRenderChanges), and what the answer
/// does: upstream's poll, which `poll` sends at the pace lbterm_poll.rs keeps and a resize sends
/// once the server has answered it, so a push comes to say what the rewrap changed
/// (clientpane.rs; lbterm/MUX.md, "A window's resize"). The changes come as a push of their own,
/// before the answer, which says only whether the pane lives. `poll` says the poll sent it, so the
/// answer lets the next one go
pub(crate) async fn ask_changes(
    client: Arc<ClientInner>,
    remote_pane_id: PaneId,
    local_pane_id: PaneId,
    poll: bool,
) -> anyhow::Result<()> {
    let alive = match client
        .client
        .get_pane_render_changes(GetPaneRenderChanges {
            pane_id: remote_pane_id,
        })
        .await
    {
        Ok(resp) => resp.is_alive,
        // if we got a timeout on a reconnectable, don't
        // consider the tab to be dead; that helps to
        // avoid having a tab get shuffled around
        Err(_) => client.client.is_reconnectable,
    };

    let mux = Mux::get();
    let tab = mux
        .get_pane(local_pane_id)
        .ok_or_else(|| anyhow!("no such tab {}", local_pane_id))?;
    if let Some(client_tab) = tab.downcast_ref::<ClientPane>() {
        let renderable = client_tab.renderable.lock();
        let mut inner = renderable.inner.borrow_mut();

        inner.dead = !alive;
        inner.last_recv_time = Instant::now();
        if poll {
            inner.poll_in_progress.store(false, Ordering::SeqCst);
        }
    }
    Ok(())
}

lazy_static::lazy_static! {
    static ref IMAGE_LRU: Mutex<LruCache<[u8;32], Arc<ImageData>>> = Mutex::new(LruCache::new(NonZeroUsize::new(128).unwrap()));
}

pub(crate) async fn hydrate_lines(
    client: Arc<ClientInner>,
    pane_id: PaneId,
    serialized_lines: SerializedLines,
) -> Vec<(StableRowIndex, Line)> {
    let (lines, image_cells) = serialized_lines.extract_data();

    if image_cells.is_empty() {
        return lines;
    }

    let mut requests = HashMap::new();
    let mut data_by_hash = HashMap::new();
    for im in &image_cells {
        if let Some(data) = IMAGE_LRU.lock().unwrap().get(&im.data_hash) {
            data_by_hash.insert(im.data_hash, Arc::clone(data));
        } else {
            requests
                .entry(&im.data_hash)
                .or_insert_with(|| GetImageCell {
                    pane_id,
                    line_idx: im.line_idx,
                    cell_idx: im.cell_idx,
                    data_hash: im.data_hash,
                });
        }
    }

    for (_, request) in requests {
        match client.client.get_image_cell(request).await {
            Ok(GetImageCellResponse {
                data: Some(data), ..
            }) => {
                IMAGE_LRU
                    .lock()
                    .unwrap()
                    .put(data.hash(), Arc::clone(&data));
                data_by_hash.insert(data.hash(), data);
            }
            Ok(GetImageCellResponse { data: None, .. }) => {
                log::error!("no image data!");
            }

            Err(err) => {
                log::error!("failed to retrieve image {err:#}");
            }
        }
    }

    let mut line_by_idx = HashMap::new();
    for (line_idx, line) in lines {
        line_by_idx.insert(line_idx, line);
    }

    for im in image_cells {
        if let Some(data) = data_by_hash.get(&im.data_hash) {
            if let Some(line) = line_by_idx.get_mut(&im.line_idx) {
                if let Some(cell) = line.cells_mut_for_attr_changes_only().get_mut(im.cell_idx) {
                    cell.attrs_mut()
                        .attach_image(Box::new(ImageCell::with_z_index(
                            im.top_left,
                            im.bottom_right,
                            Arc::clone(data),
                            im.z_index,
                            im.padding_left,
                            im.padding_top,
                            im.padding_right,
                            im.padding_bottom,
                            im.image_id,
                            im.placement_id,
                        )));
                }
            }
        }
    }

    line_by_idx.into_iter().collect()
}

impl RenderableState {
    pub fn get_cursor_position(&self) -> StableCursorPosition {
        self.inner.borrow().cursor_position
    }

    pub fn get_lines(&self, lines: Range<StableRowIndex>) -> (StableRowIndex, Vec<Line>) {
        let mut inner = self.inner.borrow_mut();
        let mut result = vec![];
        let mut to_fetch = RangeSet::new();
        let now = Instant::now();

        for idx in lines.clone() {
            // LBTerm: what reading the row does to its entry is read_row's, which a frame lent
            // its rows uses too (lbterm_lent.rs); a line newer than the pane is marked as fetching,
            // where upstream marked it stale and threw the line it fetched away
            let (entry, fetch) = lbterm_lent::read_row(inner.lines.pop(&idx), inner.seqno, now);
            if fetch {
                to_fetch.add(idx);
            }
            result.push(match &entry {
                LineEntry::Line(line)
                | LineEntry::LineAndFetching(line, _)
                | LineEntry::Stale(line) => line.clone(),
                LineEntry::Fetching(_) => Line::with_width(inner.dimensions.cols, SEQ_ZERO),
            });

            if inner.client.overlay_lag_indicator && idx == inner.dimensions.physical_top {
                if inner.is_tardy() {
                    let status = format!(
                        "wezterm: {:.0?}⏳since last response",
                        inner.last_recv_time.elapsed()
                    );
                    // Right align it in the tab
                    let col = inner
                        .dimensions
                        .cols
                        .saturating_sub(wezterm_term::unicode_column_width(&status, None));

                    let mut attr = CellAttributes::default();
                    attr.set_foreground(AnsiColor::White);
                    attr.set_background(AnsiColor::Blue);

                    result
                        .last_mut()
                        .unwrap()
                        .overlay_text_with_attribute(col, &status, attr, SEQ_ZERO);
                }
            }

            inner.lines.put(idx, entry);
        }

        log::trace!(
            "get_lines: {:?}, num result lines={}, will fetch {:?}",
            lines,
            result.len(),
            to_fetch
        );

        inner.schedule_fetch_lines(to_fetch, now);
        (lines.start, result)
    }

    pub fn get_current_seqno(&self) -> SequenceNo {
        self.inner.borrow().seqno
    }

    pub fn get_changed_since(
        &self,
        lines: Range<StableRowIndex>,
        seqno: SequenceNo,
    ) -> RangeSet<StableRowIndex> {
        let mut inner = self.inner.borrow_mut();
        if let Err(err) = inner.poll() {
            // We allow for BrokenPromise here for now; for a TLS backed
            // session it indicates that we'll retry.  For a local unix
            // domain session it is terminal... but we will detect that
            // terminal condition elsewhere
            if let Err(err) = err.downcast::<BrokenPromise>() {
                log::error!("remote tab poll failed: {}, marking as dead", err);
                inner.dead = true;
            }
        }

        let mut result = RangeSet::new();
        for r in lines {
            match inner.lines.get(&r) {
                None => {
                    result.add(r);
                }
                Some(
                    LineEntry::Line(line)
                    | LineEntry::Stale(line)
                    | LineEntry::LineAndFetching(line, _),
                ) if line.changed_since(seqno) => {
                    result.add(r);
                }
                _ => {}
            }
        }

        // If we're behind receiving an update, invalidate the top row so
        // that the indicator will update in a more timely fashion
        if inner.is_tardy() {
            // ... but take care to avoid always reporting it as dirty, so
            // that we don't end up busy looping just to repaint it
            if inner.last_late_dirty.elapsed() >= Duration::from_secs(1) {
                result.add(inner.dimensions.physical_top);
                inner.last_late_dirty = Instant::now();
            }
        }

        if !result.is_empty() {
            log::trace!("get_changed_since: {} -> {:?}", seqno, result);
        }

        result
    }

    pub fn get_dimensions(&self) -> RenderableDimensions {
        self.inner.borrow().dimensions
    }
}

// LBTerm: the lines held, looked at without a fetch, and a pane's prompt rows asked of the server
// (the scroll strip's marks)
#[path = "lbterm_held.rs"]
mod lbterm_held;

// LBTerm: a frame's rows lent where they're held, not copied (wezterm-gui's render/pane_lines.rs)
#[path = "lbterm_lent.rs"]
mod lbterm_lent;

// LBTerm: the links found in a pane's lines, kept by text, so a line that moved isn't matched again
#[path = "lbterm_links.rs"]
mod lbterm_links;
